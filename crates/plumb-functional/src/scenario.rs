//! Deterministic Scenario derivation (plan S4.1, Hotfix 050; compiler architecture §10).
//!
//! S4.1 derives Scenario skeletons of exactly three families: decision-table rule rows, state
//! transitions and explicit boundaries. Material the graph cannot carry arrives as explicit
//! [`ScenarioDerivationInputs`] (typed decision-table specs and boundary obligations), never as
//! PSG truth. Scenario values are closed literals or explicit [`ScenarioValue::SemanticRef`]
//! dependencies; a supplied, already-acquired `scenario_values` inference may fill only declared
//! literal slots within their exact contracts and never answers business semantics. Every new
//! Scenario is one Proposed HUMAN_CONFIRM proposal. Nothing here mutates a graph, persists,
//! reads files, reads a clock or calls a provider; `apply_patch` only dry-validates proposals
//! in memory.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_expr::{map_pilot_value_type, Ast, Literal as ExprLiteral, Symbol, Ty, Unit};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AuditMeta, DerivationRef, ElementStatus, EvidenceRef, Graph, Node, NodePayload, NodeType,
    RelationKind, Scenario, ScenarioKind,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;

use crate::calculation::{
    qualify_accepted_calculation, CalculationBindingExposure, CalculationScopeBinding,
    PreparedCalculation,
};
use crate::decision_table::{
    analyze_decision_table, DecisionInputCell, DecisionOutputCell, DecisionTableSpec, DecisionType,
    NumericDomain, NumericInterval,
};

/// Version of [`ScenarioInferenceContext`].
pub const SCENARIO_CONTEXT_VERSION: u32 = 1;

/// Version of the scenario-values inference output.
pub const SCENARIO_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of scenario literal filling.
pub const SCENARIO_TASK_KIND: &str = "scenario_values";

/// Version of the pilot `Calculation.examples` entry encoding.
pub const CALCULATION_EXAMPLE_VERSION: u32 = 1;

/// Generic semantic key of the third `working_days` argument.
pub const WORKING_DAYS_INCLUSIVE_KEY: &str = "inclusive_end";

/// Generic semantic key of a concrete holiday date of a Calendar.
pub const HOLIDAY_DATE_KEY: &str = "holiday_date";

/// Prefix of the semantic key of a boundary scenario's expected Calculation result.
pub const CALCULATION_RESULT_KEY_PREFIX: &str = "result:";

/// The `confirmation_status` S4.1 writes.
pub const SCENARIO_DERIVED: &str = "Derived";

/// The `confirmation_status` of a human-confirmed Scenario.
pub const SCENARIO_CONFIRMED: &str = "Confirmed";

const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s4-scenario.md");
const SCHEMA_SOURCE: &str = include_str!("../../../schemas/inference/s4-scenario.schema.json");
const SCENARIO_STAGE: StageId = StageId::S4;
const WORKING_DAYS: &str = "working_days";

// ============================================================================ errors

/// Why scenario derivation could not run. Every error stops derivation; nothing is proposed.
#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("invalid scenario input: {reason}")]
    InvalidInput { reason: String },
    #[error("invalid decision-table input {decision_table_ref}: {reason}")]
    InvalidDecisionTableInput {
        decision_table_ref: Id,
        reason: String,
    },
    #[error("invalid decision-table spec {decision_table_ref}: {reason}")]
    InvalidDecisionTableSpec {
        decision_table_ref: Id,
        reason: String,
    },
    #[error("Accepted DecisionTable {decision_table_ref} has no decision-table input")]
    MissingDecisionTableInput { decision_table_ref: Id },
    #[error("invalid boundary input {boundary_key} for {subject_ref}: {reason}")]
    InvalidBoundaryInput {
        boundary_key: String,
        subject_ref: Id,
        reason: String,
    },
    #[error("invalid transition {transition_ref}: {reason}")]
    InvalidTransition { transition_ref: Id, reason: String },
    #[error("transition {transition_ref} has no Accepted transitions_via trigger")]
    MissingTrigger { transition_ref: Id },
    #[error("transition {transition_ref} has several Accepted transitions_via triggers")]
    AmbiguousTrigger { transition_ref: Id },
    #[error("invalid example of calculation {calculation_ref}: {reason}")]
    InvalidCalculationExample { calculation_ref: Id, reason: String },
    #[error("calculation {calculation_ref} has several examples with key {key}")]
    AmbiguousCalculationExample { calculation_ref: Id, key: String },
    #[error("example {key} of calculation {calculation_ref} contradicts the boundary: {reason}")]
    ExampleContractMismatch {
        calculation_ref: Id,
        key: String,
        reason: String,
    },
    #[error("invalid scenario payload: {reason}")]
    InvalidScenario { reason: String },
    #[error("no derived scenario {scenario_ref}")]
    UnknownScenario { scenario_ref: Id },
    #[error("scenario {scenario_ref} has no literal slot to infer")]
    NoInferenceSlots { scenario_ref: Id },
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    #[error("scenario request is stale or does not match its skeleton: {reason}")]
    StaleScenarioRequest { reason: String },
    #[error("several inferences for scenario {scenario_ref}")]
    DuplicateInference { scenario_ref: Id },
    #[error("invalid scenario inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error("scenario schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    #[error("scenario output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("inference output names scenario {found}, not {expected}")]
    ScenarioMismatch { expected: Id, found: Id },
    #[error("inference output repeats or misorders slot {slot_id}")]
    DuplicateSlot { slot_id: String },
    #[error("inference output names unknown slot {slot_id}")]
    UnknownSlot { slot_id: String },
    #[error("value for slot {slot_id} violates its contract: {reason}")]
    SlotContractViolation { slot_id: String, reason: String },
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_input(reason: impl Into<String>) -> ScenarioError {
    ScenarioError::InvalidInput {
        reason: reason.into(),
    }
}

fn invalid_scenario(reason: impl Into<String>) -> ScenarioError {
    ScenarioError::InvalidScenario {
        reason: reason.into(),
    }
}

fn invalid_proposal(e: impl ToString) -> ScenarioError {
    ScenarioError::InvalidProposal {
        reason: e.to_string(),
    }
}

fn schema_invalid(reason: impl Into<String>) -> ScenarioError {
    ScenarioError::SchemaInvalid {
        reason: reason.into(),
    }
}

// ============================================================================ literals and values

/// A closed scenario literal. Decimals are canonical strings; there is no float.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScenarioLiteral {
    Bool {
        value: bool,
    },
    Int {
        value: i64,
    },
    Decimal {
        value: String,
        unit: Option<String>,
    },
    String {
        value: String,
    },
    Date {
        value: String,
    },
    #[serde(rename = "datetime")]
    DateTime {
        value: Timestamp,
    },
    Enum {
        value: String,
    },
}

/// Clean text: non-empty, exactly trimmed and free of control characters.
fn is_clean_text(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

/// The decimal of a canonical decimal string (Display reproduces it exactly).
fn canonical_decimal(value: &str) -> Option<Decimal> {
    Decimal::from_str(value)
        .ok()
        .filter(|d| d.to_string() == value)
}

fn parse_date(value: &str) -> Option<time::Date> {
    let format = time::macros::format_description!("[year]-[month]-[day]");
    time::Date::parse(value, &format)
        .ok()
        .filter(|d| d.format(&format).ok().as_deref() == Some(value))
}

impl ScenarioLiteral {
    /// Checks the literal's own shape: canonical decimal and unit, clean string, real date and
    /// non-empty enum member.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            ScenarioLiteral::Bool { .. }
            | ScenarioLiteral::Int { .. }
            | ScenarioLiteral::DateTime { .. } => Ok(()),
            ScenarioLiteral::Decimal { value, unit } => {
                if canonical_decimal(value).is_none() {
                    return Err(format!("{value:?} is not a canonical decimal"));
                }
                match unit {
                    Some(u) if Unit::new(u).is_err() => Err(format!("{u:?} is not a unit")),
                    _ => Ok(()),
                }
            }
            ScenarioLiteral::String { value } => {
                if is_clean_text(value) {
                    Ok(())
                } else {
                    Err(format!("{value:?} is not clean text"))
                }
            }
            ScenarioLiteral::Date { value } => match parse_date(value) {
                Some(_) => Ok(()),
                None => Err(format!("{value:?} is not a YYYY-MM-DD calendar date")),
            },
            ScenarioLiteral::Enum { value } => {
                if value.is_empty() {
                    Err("empty enum member".to_owned())
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Whether the literal conforms to a PlumbExpr type of a qualified calculation scope.
    fn conforms(&self, ty: &Ty, prepared: &PreparedCalculation) -> bool {
        if self.validate().is_err() {
            return false;
        }
        match (self, ty) {
            (ScenarioLiteral::Bool { .. }, Ty::Bool)
            | (ScenarioLiteral::Int { .. }, Ty::Int)
            | (ScenarioLiteral::String { .. }, Ty::String)
            | (ScenarioLiteral::Date { .. }, Ty::Date)
            | (ScenarioLiteral::DateTime { .. }, Ty::DateTime) => true,
            (ScenarioLiteral::Decimal { value, unit: None }, Ty::Decimal(scale)) => {
                canonical_decimal(value).is_some_and(|d| d.scale() <= u32::from(*scale))
            }
            (ScenarioLiteral::Decimal { unit: Some(u), .. }, Ty::Quantity(q)) => u == q.as_str(),
            (ScenarioLiteral::Enum { value }, Ty::Enum(enum_ref)) => prepared
                .type_env
                .enum_members(enum_ref)
                .is_some_and(|m| m.iter().any(|s| s.as_str() == value)),
            _ => false,
        }
    }
}

/// An explicit unresolved business-semantic dependency.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioDependency {
    pub owner_ref: Id,
    pub key: String,
}

/// A persisted executable scenario value: a literal or an unresolved semantic reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioValue {
    Literal(ScenarioLiteral),
    SemanticRef(ScenarioDependency),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticRefWire {
    kind: String,
    owner_ref: Id,
    key: String,
}

const SEMANTIC_REF_KIND: &str = "semantic_ref";

impl Serialize for ScenarioValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ScenarioValue::Literal(literal) => literal.serialize(serializer),
            ScenarioValue::SemanticRef(d) => SemanticRefWire {
                kind: SEMANTIC_REF_KIND.to_owned(),
                owner_ref: d.owner_ref.clone(),
                key: d.key.clone(),
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ScenarioValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let value = Value::deserialize(deserializer)?;
        if value.get("kind").and_then(Value::as_str) == Some(SEMANTIC_REF_KIND) {
            let wire: SemanticRefWire = serde_json::from_value(value).map_err(D::Error::custom)?;
            Ok(ScenarioValue::SemanticRef(ScenarioDependency {
                owner_ref: wire.owner_ref,
                key: wire.key,
            }))
        } else {
            Ok(ScenarioValue::Literal(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            ))
        }
    }
}

// ============================================================================ contracts

/// One numeric bound; `inclusive` is never lost.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioNumericBound {
    /// A canonical decimal string.
    pub value: String,
    pub inclusive: bool,
}

/// An optional lower and upper bound, mirroring `NumericInterval`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioNumericConstraint {
    pub lower: Option<ScenarioNumericBound>,
    pub upper: Option<ScenarioNumericBound>,
}

/// One decoded bound: its value and inclusivity, or unbounded.
type DecimalBound = Option<(Decimal, bool)>;

impl ScenarioNumericConstraint {
    fn from_interval(interval: &NumericInterval) -> ScenarioNumericConstraint {
        let bound = |b: &crate::decision_table::NumericBound| ScenarioNumericBound {
            value: b.value.to_string(),
            inclusive: b.inclusive,
        };
        ScenarioNumericConstraint {
            lower: interval.lower.as_ref().map(bound),
            upper: interval.upper.as_ref().map(bound),
        }
    }

    fn from_domain(domain: &NumericDomain) -> ScenarioNumericConstraint {
        ScenarioNumericConstraint {
            lower: Some(ScenarioNumericBound {
                value: domain.lower.to_string(),
                inclusive: true,
            }),
            upper: Some(ScenarioNumericBound {
                value: domain.upper.to_string(),
                inclusive: true,
            }),
        }
    }

    fn decimals(&self) -> Option<(DecimalBound, DecimalBound)> {
        let bound = |b: &Option<ScenarioNumericBound>| match b {
            None => Some(None),
            Some(b) => canonical_decimal(&b.value).map(|d| Some((d, b.inclusive))),
        };
        Some((bound(&self.lower)?, bound(&self.upper)?))
    }

    fn validate(&self) -> Result<(), String> {
        let (lower, upper) = self
            .decimals()
            .ok_or("interval bound is not a canonical decimal")?;
        if let (Some((l, li)), Some((u, ui))) = (lower, upper) {
            if l > u {
                return Err("interval lower bound exceeds upper bound".to_owned());
            }
            if l == u && !(li && ui) {
                return Err("empty interval".to_owned());
            }
        }
        Ok(())
    }

    fn contains(&self, value: Decimal) -> bool {
        let Some((lower, upper)) = self.decimals() else {
            return false;
        };
        let above = lower.is_none_or(|(l, inclusive)| value > l || (inclusive && value == l));
        let below = upper.is_none_or(|(u, inclusive)| value < u || (inclusive && value == u));
        above && below
    }
}

/// The exact typed contract of an inference-fillable literal slot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScenarioValueContract {
    Bool,
    Enum {
        values: Vec<String>,
    },
    Integer {
        interval: Option<ScenarioNumericConstraint>,
    },
    Decimal {
        scale: u32,
        interval: Option<ScenarioNumericConstraint>,
        unit: Option<String>,
    },
    String {
        values: Option<Vec<String>>,
    },
    Date,
    #[serde(rename = "datetime")]
    DateTime,
}

fn strictly_sorted<T: Ord>(items: &[T]) -> bool {
    items.windows(2).all(|p| p[0] < p[1])
}

impl ScenarioValueContract {
    /// Checks the contract's own shape.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            ScenarioValueContract::Enum { values } => {
                if values.is_empty()
                    || !strictly_sorted(values)
                    || values.iter().any(String::is_empty)
                {
                    return Err("enum values must be non-empty, strictly sorted members".to_owned());
                }
                Ok(())
            }
            ScenarioValueContract::Integer { interval } => interval
                .as_ref()
                .map_or(Ok(()), ScenarioNumericConstraint::validate),
            ScenarioValueContract::Decimal {
                scale,
                interval,
                unit,
            } => {
                if *scale > u32::from(plumb_expr::MAX_DECIMAL_SCALE) {
                    return Err(format!("decimal scale {scale} exceeds 28"));
                }
                if let Some(u) = unit {
                    Unit::new(u).map_err(|_| format!("{u:?} is not a unit"))?;
                }
                interval
                    .as_ref()
                    .map_or(Ok(()), ScenarioNumericConstraint::validate)
            }
            ScenarioValueContract::String {
                values: Some(values),
            } => {
                if values.is_empty()
                    || !strictly_sorted(values)
                    || !values.iter().all(|v| is_clean_text(v))
                {
                    return Err(
                        "string values must be non-empty, strictly sorted clean text".to_owned(),
                    );
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Whether `literal` satisfies this exact contract.
    pub fn check(&self, literal: &ScenarioLiteral) -> Result<(), String> {
        literal.validate()?;
        let ok = match (self, literal) {
            (ScenarioValueContract::Bool, ScenarioLiteral::Bool { .. })
            | (ScenarioValueContract::Date, ScenarioLiteral::Date { .. })
            | (ScenarioValueContract::DateTime, ScenarioLiteral::DateTime { .. }) => true,
            (ScenarioValueContract::Enum { values }, ScenarioLiteral::Enum { value }) => {
                values.contains(value)
            }
            (ScenarioValueContract::Integer { interval }, ScenarioLiteral::Int { value }) => {
                interval
                    .as_ref()
                    .is_none_or(|i| i.contains(Decimal::from(*value)))
            }
            (
                ScenarioValueContract::Decimal {
                    scale,
                    interval,
                    unit,
                },
                ScenarioLiteral::Decimal {
                    value,
                    unit: literal_unit,
                },
            ) => canonical_decimal(value).is_some_and(|d| {
                d.scale() <= *scale
                    && literal_unit == unit
                    && interval.as_ref().is_none_or(|i| i.contains(d))
            }),
            (ScenarioValueContract::String { values }, ScenarioLiteral::String { value }) => {
                values.as_ref().is_none_or(|v| v.contains(value))
            }
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err(format!("{literal:?} does not satisfy {self:?}"))
        }
    }
}

/// A literal position that inference may fill.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioValueSlot {
    pub slot_id: String,
    pub contract: ScenarioValueContract,
}

// ============================================================================ Given / When / Then

/// The single semantic anchor of `Scenario.when`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioWhen {
    Operation(Id),
    Event(Id),
    Transition(Id),
    Rule(Id),
    Calculation(Id),
    DecisionTable(Id),
}

impl ScenarioWhen {
    fn anchor(&self) -> &Id {
        match self {
            ScenarioWhen::Operation(id)
            | ScenarioWhen::Event(id)
            | ScenarioWhen::Transition(id)
            | ScenarioWhen::Rule(id)
            | ScenarioWhen::Calculation(id)
            | ScenarioWhen::DecisionTable(id) => id,
        }
    }

    fn node_type(&self) -> NodeType {
        match self {
            ScenarioWhen::Operation(_) => NodeType::Operation,
            ScenarioWhen::Event(_) => NodeType::Event,
            ScenarioWhen::Transition(_) => NodeType::Transition,
            ScenarioWhen::Rule(_) => NodeType::Rule,
            ScenarioWhen::Calculation(_) => NodeType::Calculation,
            ScenarioWhen::DecisionTable(_) => NodeType::DecisionTable,
        }
    }

    fn for_node(node: &Node) -> Option<ScenarioWhen> {
        let id = node.id.clone();
        Some(match node.payload.node_type() {
            NodeType::Operation => ScenarioWhen::Operation(id),
            NodeType::Event => ScenarioWhen::Event(id),
            NodeType::Transition => ScenarioWhen::Transition(id),
            NodeType::Rule => ScenarioWhen::Rule(id),
            NodeType::Calculation => ScenarioWhen::Calculation(id),
            NodeType::DecisionTable => ScenarioWhen::DecisionTable(id),
            _ => return None,
        })
    }

    /// The exact one-key wire object.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// Decodes the exact one-key wire object.
    pub fn from_value(value: &Value) -> Result<ScenarioWhen, ScenarioError> {
        if value.as_object().is_none_or(|o| o.len() != 1) {
            return Err(invalid_scenario("when must be a one-key object"));
        }
        serde_json::from_value(value.clone()).map_err(|e| invalid_scenario(format!("when: {e}")))
    }
}

/// `Scenario.given`, generic over the value position (persisted values or skeleton positions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioGiven<V> {
    pub attribute_values: BTreeMap<Id, V>,
    pub entities_exist: Vec<Id>,
    pub states_active: Vec<Id>,
    pub decision_inputs: BTreeMap<u32, V>,
    pub calculation_semantics: BTreeMap<Id, BTreeMap<String, V>>,
}

/// `Scenario.then`, generic over the value position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioThen<V> {
    pub attribute_values: BTreeMap<Id, V>,
    pub states_active: Vec<Id>,
    pub outcomes: Vec<Id>,
    pub events: Vec<Id>,
    pub rules_satisfied: Vec<Id>,
    pub decision_outputs: BTreeMap<u32, V>,
    pub calculation_results: BTreeMap<Id, V>,
}

impl<V> Default for ScenarioGiven<V> {
    fn default() -> Self {
        ScenarioGiven {
            attribute_values: BTreeMap::new(),
            entities_exist: Vec::new(),
            states_active: Vec::new(),
            decision_inputs: BTreeMap::new(),
            calculation_semantics: BTreeMap::new(),
        }
    }
}

impl<V> Default for ScenarioThen<V> {
    fn default() -> Self {
        ScenarioThen {
            attribute_values: BTreeMap::new(),
            states_active: Vec::new(),
            outcomes: Vec::new(),
            events: Vec::new(),
            rules_satisfied: Vec::new(),
            decision_outputs: BTreeMap::new(),
            calculation_results: BTreeMap::new(),
        }
    }
}

fn id_map<V>(map: &BTreeMap<Id, V>, f: &impl Fn(&V) -> Value) -> Value {
    Value::Object(map.iter().map(|(k, v)| (k.to_string(), f(v))).collect())
}

fn index_map<V>(map: &BTreeMap<u32, V>, f: &impl Fn(&V) -> Value) -> Value {
    Value::Object(map.iter().map(|(k, v)| (k.to_string(), f(v))).collect())
}

fn id_list(ids: &[Id]) -> Value {
    Value::Array(ids.iter().map(|i| Value::String(i.to_string())).collect())
}

fn category(entries: &mut Vec<Value>, name: &str, value: Value, present: bool) {
    if present {
        entries.push(json!({ name: value }));
    }
}

impl<V> ScenarioGiven<V> {
    /// The grouped wire entries in canonical category order; empty categories are omitted.
    pub fn to_values(&self, f: impl Fn(&V) -> Value) -> Vec<Value> {
        let mut out = Vec::new();
        let semantics = Value::Object(
            self.calculation_semantics
                .iter()
                .map(|(k, m)| {
                    let inner: Map<String, Value> =
                        m.iter().map(|(key, v)| (key.clone(), f(v))).collect();
                    (k.to_string(), Value::Object(inner))
                })
                .collect(),
        );
        category(
            &mut out,
            "attribute_values",
            id_map(&self.attribute_values, &f),
            !self.attribute_values.is_empty(),
        );
        category(
            &mut out,
            "entities_exist",
            id_list(&self.entities_exist),
            !self.entities_exist.is_empty(),
        );
        category(
            &mut out,
            "states_active",
            id_list(&self.states_active),
            !self.states_active.is_empty(),
        );
        category(
            &mut out,
            "decision_inputs",
            index_map(&self.decision_inputs, &f),
            !self.decision_inputs.is_empty(),
        );
        category(
            &mut out,
            "calculation_semantics",
            semantics,
            !self.calculation_semantics.is_empty(),
        );
        out
    }

    fn map<W>(&self, f: &impl Fn(&V) -> W) -> ScenarioGiven<W> {
        ScenarioGiven {
            attribute_values: self
                .attribute_values
                .iter()
                .map(|(k, v)| (k.clone(), f(v)))
                .collect(),
            entities_exist: self.entities_exist.clone(),
            states_active: self.states_active.clone(),
            decision_inputs: self
                .decision_inputs
                .iter()
                .map(|(k, v)| (*k, f(v)))
                .collect(),
            calculation_semantics: self
                .calculation_semantics
                .iter()
                .map(|(k, m)| {
                    (
                        k.clone(),
                        m.iter().map(|(key, v)| (key.clone(), f(v))).collect(),
                    )
                })
                .collect(),
        }
    }

    fn values(&self) -> Vec<&V> {
        let mut out: Vec<&V> = self.attribute_values.values().collect();
        out.extend(self.decision_inputs.values());
        out.extend(
            self.calculation_semantics
                .values()
                .flat_map(BTreeMap::values),
        );
        out
    }
}

impl<V> ScenarioThen<V> {
    /// The grouped wire entries in canonical category order; empty categories are omitted.
    pub fn to_values(&self, f: impl Fn(&V) -> Value) -> Vec<Value> {
        let mut out = Vec::new();
        category(
            &mut out,
            "attribute_values",
            id_map(&self.attribute_values, &f),
            !self.attribute_values.is_empty(),
        );
        category(
            &mut out,
            "states_active",
            id_list(&self.states_active),
            !self.states_active.is_empty(),
        );
        category(
            &mut out,
            "outcomes",
            id_list(&self.outcomes),
            !self.outcomes.is_empty(),
        );
        category(
            &mut out,
            "events",
            id_list(&self.events),
            !self.events.is_empty(),
        );
        category(
            &mut out,
            "rules_satisfied",
            id_list(&self.rules_satisfied),
            !self.rules_satisfied.is_empty(),
        );
        category(
            &mut out,
            "decision_outputs",
            index_map(&self.decision_outputs, &f),
            !self.decision_outputs.is_empty(),
        );
        category(
            &mut out,
            "calculation_results",
            id_map(&self.calculation_results, &f),
            !self.calculation_results.is_empty(),
        );
        out
    }

    fn map<W>(&self, f: &impl Fn(&V) -> W) -> ScenarioThen<W> {
        ScenarioThen {
            attribute_values: self
                .attribute_values
                .iter()
                .map(|(k, v)| (k.clone(), f(v)))
                .collect(),
            states_active: self.states_active.clone(),
            outcomes: self.outcomes.clone(),
            events: self.events.clone(),
            rules_satisfied: self.rules_satisfied.clone(),
            decision_outputs: self
                .decision_outputs
                .iter()
                .map(|(k, v)| (*k, f(v)))
                .collect(),
            calculation_results: self
                .calculation_results
                .iter()
                .map(|(k, v)| (k.clone(), f(v)))
                .collect(),
        }
    }

    fn values(&self) -> Vec<&V> {
        let mut out: Vec<&V> = self.attribute_values.values().collect();
        out.extend(self.decision_outputs.values());
        out.extend(self.calculation_results.values());
        out
    }
}

// --------------------------------------------------------------------------- wire decoding

fn one_key(entry: &Value) -> Result<(&str, &Value), ScenarioError> {
    match entry.as_object() {
        Some(o) if o.len() == 1 => {
            let (k, v) = o
                .iter()
                .next()
                .ok_or_else(|| invalid_scenario("empty entry"))?;
            Ok((k.as_str(), v))
        }
        _ => Err(invalid_scenario(
            "every given/then entry must be a one-key object",
        )),
    }
}

fn decode_ids(name: &str, value: &Value) -> Result<Vec<Id>, ScenarioError> {
    let ids: Vec<Id> = serde_json::from_value(value.clone())
        .map_err(|e| invalid_scenario(format!("{name}: {e}")))?;
    if ids.is_empty() || !strictly_sorted(&ids) {
        return Err(invalid_scenario(format!(
            "{name} must be non-empty, strictly sorted and unique"
        )));
    }
    Ok(ids)
}

fn decode_value(value: &Value) -> Result<ScenarioValue, ScenarioError> {
    let decoded: ScenarioValue =
        serde_json::from_value(value.clone()).map_err(|e| invalid_scenario(e.to_string()))?;
    // Closed and canonical: re-encoding must reproduce the exact object.
    if serde_json::to_value(&decoded).ok().as_ref() != Some(value) {
        return Err(invalid_scenario(format!(
            "{value} is not a canonical scenario value"
        )));
    }
    match &decoded {
        ScenarioValue::Literal(l) => l.validate().map_err(invalid_scenario)?,
        ScenarioValue::SemanticRef(d) if !is_clean_text(&d.key) => {
            return Err(invalid_scenario("semantic_ref key is not clean text"));
        }
        ScenarioValue::SemanticRef(_) => {}
    }
    Ok(decoded)
}

fn decode_id_map(name: &str, value: &Value) -> Result<BTreeMap<Id, ScenarioValue>, ScenarioError> {
    let object = value
        .as_object()
        .filter(|o| !o.is_empty())
        .ok_or_else(|| invalid_scenario(format!("{name} must be a non-empty object")))?;
    object
        .iter()
        .map(|(k, v)| {
            let id =
                Id::from_str(k).map_err(|e| invalid_scenario(format!("{name} key {k}: {e}")))?;
            Ok((id, decode_value(v)?))
        })
        .collect()
}

fn decode_index_map(
    name: &str,
    value: &Value,
) -> Result<BTreeMap<u32, ScenarioValue>, ScenarioError> {
    let object = value
        .as_object()
        .filter(|o| !o.is_empty())
        .ok_or_else(|| invalid_scenario(format!("{name} must be a non-empty object")))?;
    object
        .iter()
        .map(|(k, v)| {
            let index: u32 = k
                .parse()
                .ok()
                .filter(|i: &u32| i.to_string() == *k)
                .ok_or_else(|| invalid_scenario(format!("{name} key {k} is not a column index")))?;
            Ok((index, decode_value(v)?))
        })
        .collect()
}

fn ordered_categories<'v>(
    entries: &'v [Value],
    order: &[&str],
) -> Result<Vec<(&'v str, &'v Value)>, ScenarioError> {
    let mut last = None;
    let mut out = Vec::new();
    for entry in entries {
        let (name, value) = one_key(entry)?;
        let rank = order
            .iter()
            .position(|c| *c == name)
            .ok_or_else(|| invalid_scenario(format!("unknown category {name}")))?;
        if last.is_some_and(|l| rank <= l) {
            return Err(invalid_scenario(format!(
                "category {name} repeated or out of order"
            )));
        }
        last = Some(rank);
        out.push((name, value));
    }
    Ok(out)
}

const GIVEN_ORDER: [&str; 5] = [
    "attribute_values",
    "entities_exist",
    "states_active",
    "decision_inputs",
    "calculation_semantics",
];

const THEN_ORDER: [&str; 7] = [
    "attribute_values",
    "states_active",
    "outcomes",
    "events",
    "rules_satisfied",
    "decision_outputs",
    "calculation_results",
];

impl ScenarioGiven<ScenarioValue> {
    /// Decodes the exact grouped wire form.
    pub fn from_values(entries: &[Value]) -> Result<Self, ScenarioError> {
        let mut given = ScenarioGiven::default();
        for (name, value) in ordered_categories(entries, &GIVEN_ORDER)? {
            match name {
                "attribute_values" => given.attribute_values = decode_id_map(name, value)?,
                "entities_exist" => given.entities_exist = decode_ids(name, value)?,
                "states_active" => given.states_active = decode_ids(name, value)?,
                "decision_inputs" => given.decision_inputs = decode_index_map(name, value)?,
                _ => {
                    let object = value.as_object().filter(|o| !o.is_empty()).ok_or_else(|| {
                        invalid_scenario("calculation_semantics must be non-empty")
                    })?;
                    for (k, inner) in object {
                        let id = Id::from_str(k).map_err(|e| {
                            invalid_scenario(format!("calculation_semantics key {k}: {e}"))
                        })?;
                        let inner =
                            inner.as_object().filter(|o| !o.is_empty()).ok_or_else(|| {
                                invalid_scenario("calculation_semantics entry must be non-empty")
                            })?;
                        let mut keyed = BTreeMap::new();
                        for (key, v) in inner {
                            if !is_clean_text(key) {
                                return Err(invalid_scenario("semantic key is not clean text"));
                            }
                            keyed.insert(key.clone(), decode_value(v)?);
                        }
                        given.calculation_semantics.insert(id, keyed);
                    }
                }
            }
        }
        Ok(given)
    }
}

impl ScenarioThen<ScenarioValue> {
    /// Decodes the exact grouped wire form.
    pub fn from_values(entries: &[Value]) -> Result<Self, ScenarioError> {
        let mut then = ScenarioThen::default();
        for (name, value) in ordered_categories(entries, &THEN_ORDER)? {
            match name {
                "attribute_values" => then.attribute_values = decode_id_map(name, value)?,
                "states_active" => then.states_active = decode_ids(name, value)?,
                "outcomes" => then.outcomes = decode_ids(name, value)?,
                "events" => then.events = decode_ids(name, value)?,
                "rules_satisfied" => then.rules_satisfied = decode_ids(name, value)?,
                "decision_outputs" => {
                    then.decision_outputs = decode_index_map(name, value)?;
                    if then
                        .decision_outputs
                        .values()
                        .any(|v| !matches!(v, ScenarioValue::Literal(_)))
                    {
                        return Err(invalid_scenario("decision_outputs hold literals only"));
                    }
                }
                _ => then.calculation_results = decode_id_map(name, value)?,
            }
        }
        Ok(then)
    }
}

fn value_json(v: &ScenarioValue) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// The typed reading of an S4 Scenario payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioSemantics {
    pub given: ScenarioGiven<ScenarioValue>,
    pub when: ScenarioWhen,
    pub then: ScenarioThen<ScenarioValue>,
}

impl ScenarioSemantics {
    /// Decodes the controlled S4 Given/When/Then of a Scenario payload.
    pub fn from_scenario(scenario: &Scenario) -> Result<ScenarioSemantics, ScenarioError> {
        Ok(ScenarioSemantics {
            given: ScenarioGiven::from_values(&scenario.given)?,
            when: ScenarioWhen::from_value(&scenario.when)?,
            then: ScenarioThen::from_values(&scenario.then)?,
        })
    }
}

fn accepted_of<'g>(graph: &'g Graph, id: &Id, node_type: NodeType) -> Option<&'g Node> {
    graph
        .node(id)
        .filter(|n| n.status == ElementStatus::Accepted && n.payload.node_type() == node_type)
}

/// Validates an S4 Scenario payload against the graph: the controlled wire, Accepted
/// references of the expected types, SemanticRef owners (Calculation or Calendar), traceability
/// and the pilot confirmation status.
pub fn validate_scenario(
    graph: &Graph,
    scenario: &Scenario,
) -> Result<ScenarioSemantics, ScenarioError> {
    let semantics = ScenarioSemantics::from_scenario(scenario)?;
    let need = |id: &Id, node_type: NodeType| {
        accepted_of(graph, id, node_type)
            .map(|_| ())
            .ok_or_else(|| invalid_scenario(format!("{id} is not an Accepted {node_type:?}")))
    };
    need(semantics.when.anchor(), semantics.when.node_type())?;
    let g = &semantics.given;
    let t = &semantics.then;
    for id in g.attribute_values.keys().chain(t.attribute_values.keys()) {
        need(id, NodeType::Attribute)?;
    }
    for id in &g.entities_exist {
        need(id, NodeType::Entity)?;
    }
    for id in g.states_active.iter().chain(&t.states_active) {
        need(id, NodeType::State)?;
    }
    for id in &t.outcomes {
        need(id, NodeType::Outcome)?;
    }
    for id in &t.events {
        need(id, NodeType::Event)?;
    }
    for id in &t.rules_satisfied {
        need(id, NodeType::Rule)?;
    }
    for id in g
        .calculation_semantics
        .keys()
        .chain(t.calculation_results.keys())
    {
        need(id, NodeType::Calculation)?;
    }
    for value in g.values().into_iter().chain(t.values()) {
        if let ScenarioValue::SemanticRef(d) = value {
            if accepted_of(graph, &d.owner_ref, NodeType::Calculation).is_none()
                && accepted_of(graph, &d.owner_ref, NodeType::Calendar).is_none()
            {
                return Err(invalid_scenario(format!(
                    "semantic_ref owner {} is not an Accepted Calculation or Calendar",
                    d.owner_ref
                )));
            }
        }
    }
    let refs_ok = |refs: &Option<Vec<Id>>| {
        refs.as_ref()
            .is_none_or(|r| !r.is_empty() && strictly_sorted(r))
    };
    if !refs_ok(&scenario.requirement_refs) || !refs_ok(&scenario.derived_from_refs) {
        return Err(invalid_scenario(
            "traceability refs must be non-empty, sorted and unique",
        ));
    }
    if scenario.requirement_refs.is_none() && scenario.derived_from_refs.is_none() {
        return Err(invalid_scenario(
            "a scenario needs requirement_refs or derived_from_refs",
        ));
    }
    for id in scenario.requirement_refs.iter().flatten() {
        need(id, NodeType::Requirement)?;
    }
    for id in scenario.derived_from_refs.iter().flatten() {
        if graph
            .node(id)
            .is_none_or(|n| n.status != ElementStatus::Accepted)
        {
            return Err(invalid_scenario(format!("{id} is not an Accepted source")));
        }
    }
    match scenario.confirmation_status.as_deref() {
        Some(SCENARIO_DERIVED | SCENARIO_CONFIRMED) => {}
        other => {
            return Err(invalid_scenario(format!(
                "confirmation_status {other:?} is not pilot"
            )));
        }
    }
    if !is_clean_text(&scenario.name) {
        return Err(invalid_scenario("scenario name is not clean text"));
    }
    Ok(semantics)
}

// ============================================================================ Calculation.examples v1

/// One input of a pilot Calculation example.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationExampleInput {
    pub symbol: String,
    pub value: ScenarioLiteral,
}

/// The pilot `Calculation.examples[]` entry (version 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationExampleV1 {
    pub version: u32,
    pub key: String,
    pub inputs: Vec<CalculationExampleInput>,
    pub expected_result: Option<ScenarioLiteral>,
}

/// One Accepted Calculation with clean S2.6 qualification material.
struct QualifiedCalculation<'g> {
    calculation: &'g plumb_psg::Calculation,
    used_bindings: Vec<CalculationScopeBinding>,
    prepared: PreparedCalculation,
}

impl QualifiedCalculation<'_> {
    fn root_binding(&self, symbol: &str) -> Option<&CalculationScopeBinding> {
        self.used_bindings
            .iter()
            .find(|b| b.symbol == symbol && b.exposure == CalculationBindingExposure::Root)
    }

    fn root_ty(&self, symbol: &str) -> Option<&Ty> {
        Symbol::new(symbol)
            .ok()
            .and_then(|s| self.prepared.type_env.root(&s))
    }

    /// Every example decoded and type-checked against the qualified scope.
    fn examples(&self, calculation_ref: &Id) -> Result<Vec<CalculationExampleV1>, ScenarioError> {
        let invalid = |reason: String| ScenarioError::InvalidCalculationExample {
            calculation_ref: calculation_ref.clone(),
            reason,
        };
        let mut out = Vec::new();
        for value in self.calculation.examples.iter().flatten() {
            let example: CalculationExampleV1 =
                serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
            if serde_json::to_value(&example).ok().as_ref() != Some(value) {
                return Err(invalid("example is not in canonical v1 form".to_owned()));
            }
            if example.version != CALCULATION_EXAMPLE_VERSION {
                return Err(invalid(format!(
                    "example version {} is not 1",
                    example.version
                )));
            }
            if !is_clean_text(&example.key) {
                return Err(invalid("example key is not clean text".to_owned()));
            }
            let symbols: Vec<&String> = example.inputs.iter().map(|i| &i.symbol).collect();
            if symbols.is_empty() || !strictly_sorted(&symbols) {
                return Err(invalid(
                    "example inputs must be non-empty, sorted and unique".to_owned(),
                ));
            }
            for input in &example.inputs {
                let ty = self
                    .root_binding(&input.symbol)
                    .and_then(|_| self.root_ty(&input.symbol))
                    .ok_or_else(|| invalid(format!("{} is not a scope input", input.symbol)))?;
                if !input.value.conforms(ty, &self.prepared) {
                    return Err(invalid(format!("{} does not type-check", input.symbol)));
                }
            }
            if let Some(result) = &example.expected_result {
                if !result.conforms(&self.prepared.expected_ty, &self.prepared) {
                    return Err(invalid("expected_result does not type-check".to_owned()));
                }
            }
            out.push(example);
        }
        Ok(out)
    }

    /// The single `working_days` call and its period Attributes.
    fn working_days(&self, graph: &Graph) -> Result<WorkingDaysCall<'_>, String> {
        let mut calls = Vec::new();
        collect_calls(&self.prepared.ast, &mut calls);
        let [args] = calls.as_slice() else {
            return Err(format!(
                "expected exactly one working_days call, found {}",
                calls.len()
            ));
        };
        let (Some(Ast::List(period)), Some(inclusive)) = (args.first(), args.get(2)) else {
            return Err("working_days needs a period list and an inclusive argument".to_owned());
        };
        let [Ast::Identifier(start), Ast::Identifier(end)] = period.as_slice() else {
            return Err("the working_days period must be two identifiers".to_owned());
        };
        let resolve = |identifier: &plumb_expr::Identifier| -> Result<Id, String> {
            let segments = identifier.segments();
            let node_ref = match segments {
                [root] => self.root_binding(root.as_str()).map(|b| b.node_ref.clone()),
                [owner, field] => self.root_binding(owner.as_str()).and_then(|owner| {
                    self.used_bindings
                        .iter()
                        .find(|b| {
                            b.symbol == field.as_str()
                                && b.exposure
                                    == CalculationBindingExposure::Field {
                                        owner_ref: owner.node_ref.clone(),
                                    }
                        })
                        .map(|b| b.node_ref.clone())
                }),
                _ => None,
            }
            .ok_or_else(|| format!("{identifier} is not bound to an Attribute"))?;
            match accepted_of(graph, &node_ref, NodeType::Attribute).map(|n| &n.payload) {
                Some(NodePayload::Attribute(a)) if a.value_type == "Date" => Ok(node_ref),
                _ => Err(format!("{identifier} is not a Date Attribute")),
            }
        };
        let (start, end) = (resolve(start)?, resolve(end)?);
        if start == end {
            return Err("the working_days period start and end are the same Attribute".to_owned());
        }
        Ok(WorkingDaysCall {
            start,
            end,
            inclusive,
        })
    }
}

struct WorkingDaysCall<'a> {
    start: Id,
    end: Id,
    inclusive: &'a Ast,
}

fn collect_calls<'a>(ast: &'a Ast, out: &mut Vec<&'a Vec<Ast>>) {
    match ast {
        Ast::Call { function, args } => {
            if function.as_str() == WORKING_DAYS {
                out.push(args);
            }
            args.iter().for_each(|a| collect_calls(a, out));
        }
        Ast::List(items) => items.iter().for_each(|a| collect_calls(a, out)),
        Ast::Unary { expr, .. } => collect_calls(expr, out),
        Ast::Binary { left, right, .. } => {
            collect_calls(left, out);
            collect_calls(right, out);
        }
        Ast::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_calls(condition, out);
            collect_calls(then_expr, out);
            collect_calls(else_expr, out);
        }
        Ast::Literal(_) | Ast::Identifier(_) => {}
    }
}

fn qualified_calculation<'g>(
    graph: &'g Graph,
    calculation_ref: &Id,
) -> Result<QualifiedCalculation<'g>, String> {
    let calculation =
        match accepted_of(graph, calculation_ref, NodeType::Calculation).map(|n| &n.payload) {
            Some(NodePayload::Calculation(c)) => c,
            _ => return Err(format!("{calculation_ref} is not an Accepted Calculation")),
        };
    let accepted =
        qualify_accepted_calculation(graph, calculation_ref, None).map_err(|e| e.to_string())?;
    let qualification = accepted.qualification;
    match qualification.prepared {
        Some(prepared) if qualification.issues.is_empty() => Ok(QualifiedCalculation {
            calculation,
            used_bindings: qualification.used_bindings,
            prepared,
        }),
        _ => Err(format!("{calculation_ref} does not qualify cleanly")),
    }
}

// ============================================================================ derivation inputs

/// The explicit typed spec of one Accepted DecisionTable (the PSG Values are never decoded).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioDecisionTableInput {
    pub decision_table_ref: Id,
    pub spec: DecisionTableSpec,
}

/// The closed boundary kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScenarioBoundaryKind {
    Numeric {
        attribute_ref: Id,
        value_contract: ScenarioValueContract,
    },
    CalculationSemantic {
        calculation_ref: Id,
        semantic_key: String,
        input_symbol: String,
        value_contract: ScenarioValueContract,
    },
    CalendarHoliday {
        calculation_ref: Id,
        calendar_ref: Id,
    },
    InclusiveEnd {
        calculation_ref: Id,
    },
}

/// An explicit boundary obligation: which boundary deserves a Scenario, never its answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioBoundaryInput {
    pub boundary_key: String,
    pub source_refs: Vec<Id>,
    pub subject_ref: Id,
    pub kind: ScenarioBoundaryKind,
}

/// Deterministic caller-supplied derivation material; never PSG truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioDerivationInputs {
    pub decision_tables: Vec<ScenarioDecisionTableInput>,
    pub boundaries: Vec<ScenarioBoundaryInput>,
}

impl ScenarioDerivationInputs {
    /// Canonical inputs: decision tables by ref, boundaries by (boundary_key, subject_ref).
    /// Graph validation happens during derivation.
    pub fn new(
        mut decision_tables: Vec<ScenarioDecisionTableInput>,
        mut boundaries: Vec<ScenarioBoundaryInput>,
    ) -> ScenarioDerivationInputs {
        decision_tables.sort_by(|a, b| a.decision_table_ref.cmp(&b.decision_table_ref));
        boundaries.sort_by(|a, b| {
            (&a.boundary_key, &a.subject_ref).cmp(&(&b.boundary_key, &b.subject_ref))
        });
        ScenarioDerivationInputs {
            decision_tables,
            boundaries,
        }
    }

    /// Generic SHA-256 of the RFC 8785 canonical inputs.
    pub fn content_hash(&self) -> Result<Hash, ScenarioError> {
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn validate_order(&self) -> Result<(), ScenarioError> {
        let tables: Vec<&Id> = self
            .decision_tables
            .iter()
            .map(|t| &t.decision_table_ref)
            .collect();
        if !strictly_sorted(&tables) {
            return Err(invalid_input(
                "decision-table inputs are unsorted or duplicated",
            ));
        }
        let boundaries: Vec<(&String, &Id)> = self
            .boundaries
            .iter()
            .map(|b| (&b.boundary_key, &b.subject_ref))
            .collect();
        if !strictly_sorted(&boundaries) {
            return Err(invalid_input("boundary inputs are unsorted or duplicated"));
        }
        Ok(())
    }
}

// ============================================================================ skeletons

/// A skeleton value position: a fixed literal, a fillable slot or a semantic dependency with
/// its accepted resolution, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkeletonValue {
    Literal(ScenarioLiteral),
    Slot(String),
    Semantic {
        dependency: ScenarioDependency,
        resolved: Option<ScenarioLiteral>,
    },
}

impl SkeletonValue {
    fn semantic(owner_ref: &Id, key: &str, resolved: Option<ScenarioLiteral>) -> SkeletonValue {
        SkeletonValue::Semantic {
            dependency: ScenarioDependency {
                owner_ref: owner_ref.clone(),
                key: key.to_owned(),
            },
            resolved,
        }
    }

    /// Identity form: slots and semantic positions abstract their values.
    fn identity_json(&self) -> Value {
        match self {
            SkeletonValue::Literal(l) => serde_json::to_value(l).unwrap_or(Value::Null),
            SkeletonValue::Slot(slot_id) => json!({"kind": "slot", "slot_id": slot_id}),
            SkeletonValue::Semantic { dependency, .. } => {
                value_json(&ScenarioValue::SemanticRef(dependency.clone()))
            }
        }
    }

    /// Full material form, including the resolution state.
    fn material_json(&self) -> Value {
        match self {
            SkeletonValue::Semantic {
                dependency,
                resolved,
            } => json!({
                "kind": "semantic",
                "owner_ref": dependency.owner_ref,
                "key": dependency.key,
                "resolved": resolved,
            }),
            other => other.identity_json(),
        }
    }
}

/// The derivation family of a skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioFamily {
    RuleRow,
    StateTransition,
    Boundary,
}

impl ScenarioFamily {
    fn kind(self) -> ScenarioKind {
        match self {
            ScenarioFamily::RuleRow => ScenarioKind::RuleRow,
            ScenarioFamily::StateTransition => ScenarioKind::StateTransition,
            ScenarioFamily::Boundary => ScenarioKind::Boundary,
        }
    }
}

/// One deterministic scenario obligation with its literal slots and semantic dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioSkeleton {
    pub scenario_ref: Id,
    pub family: ScenarioFamily,
    pub name: String,
    pub given: ScenarioGiven<SkeletonValue>,
    pub when: ScenarioWhen,
    pub then: ScenarioThen<SkeletonValue>,
    pub requirement_refs: Vec<Id>,
    pub derived_from_refs: Vec<Id>,
    /// The fillable literal slots, sorted by slot_id.
    pub value_slots: Vec<ScenarioValueSlot>,
    /// The unresolved semantic dependencies, sorted and unique.
    pub unresolved_dependencies: Vec<ScenarioDependency>,
}

impl ScenarioSkeleton {
    /// The ScenarioKind of the family.
    pub fn scenario_kind(&self) -> ScenarioKind {
        self.family.kind()
    }

    fn positions(&self) -> Vec<&SkeletonValue> {
        let mut out = self.given.values();
        out.extend(self.then.values());
        out
    }

    /// Generic SHA-256 of the canonical skeleton material, including resolution state and slot
    /// contracts; it binds inference to the current materialization.
    pub fn content_hash(&self) -> Result<Hash, ScenarioError> {
        let body = json!({
            "scenario_ref": self.scenario_ref,
            "scenario_kind": self.scenario_kind(),
            "name": self.name,
            "given": self.given.to_values(SkeletonValue::material_json),
            "when": self.when.to_value(),
            "then": self.then.to_values(SkeletonValue::material_json),
            "requirement_refs": self.requirement_refs,
            "derived_from_refs": self.derived_from_refs,
            "value_slots": self.value_slots,
            "unresolved_dependencies": self.unresolved_dependencies,
        });
        Ok(Hash::content_sha256(&to_canonical_json(&body)?))
    }

    fn slot(&self, slot_id: &str) -> Option<&ScenarioValueSlot> {
        self.value_slots
            .binary_search_by(|s| s.slot_id.as_str().cmp(slot_id))
            .ok()
            .map(|i| &self.value_slots[i])
    }

    /// The persisted Scenario once every slot has a literal; `None` while any slot is open.
    pub fn materialize(&self, values: &BTreeMap<String, ScenarioLiteral>) -> Option<Scenario> {
        if self
            .value_slots
            .iter()
            .any(|s| !values.contains_key(&s.slot_id))
        {
            return None;
        }
        let concrete = |v: &SkeletonValue| -> ScenarioValue {
            match v {
                SkeletonValue::Literal(l) => ScenarioValue::Literal(l.clone()),
                SkeletonValue::Slot(id) => ScenarioValue::Literal(values[id].clone()),
                SkeletonValue::Semantic {
                    resolved: Some(l), ..
                } => ScenarioValue::Literal(l.clone()),
                SkeletonValue::Semantic {
                    dependency,
                    resolved: None,
                } => ScenarioValue::SemanticRef(dependency.clone()),
            }
        };
        let optional = |refs: &Vec<Id>| (!refs.is_empty()).then(|| refs.clone());
        Some(Scenario {
            name: self.name.clone(),
            scenario_kind: self.scenario_kind(),
            given: self.given.map(&concrete).to_values(value_json),
            when: self.when.to_value(),
            then: self.then.map(&concrete).to_values(value_json),
            requirement_refs: optional(&self.requirement_refs),
            derived_from_refs: optional(&self.derived_from_refs),
            confirmation_status: Some(SCENARIO_DERIVED.to_owned()),
        })
    }

    /// Whether a persisted Scenario is this obligation, and whether it is current: same name,
    /// kind, traceability and structure, equal literals, contract-valid slot literals and the
    /// same SemanticRef or its resolved literal at semantic positions.
    fn matches(&self, scenario: &Scenario) -> Option<bool> {
        let semantics = ScenarioSemantics::from_scenario(scenario).ok()?;
        let optional = |refs: &Vec<Id>| (!refs.is_empty()).then(|| refs.clone());
        if scenario.name != self.name
            || scenario.scenario_kind != self.scenario_kind()
            || scenario.requirement_refs != optional(&self.requirement_refs)
            || scenario.derived_from_refs != optional(&self.derived_from_refs)
            || semantics.when != self.when
        {
            return None;
        }
        let position = |sk: &SkeletonValue, v: &ScenarioValue| -> Option<bool> {
            match (sk, v) {
                (SkeletonValue::Literal(l), ScenarioValue::Literal(x)) if l == x => Some(true),
                (SkeletonValue::Slot(id), ScenarioValue::Literal(x)) => {
                    self.slot(id)?.contract.check(x).ok().map(|_| true)
                }
                (
                    SkeletonValue::Semantic {
                        dependency,
                        resolved,
                    },
                    ScenarioValue::SemanticRef(d),
                ) if d == dependency => Some(resolved.is_none()),
                (
                    SkeletonValue::Semantic {
                        resolved: Some(r), ..
                    },
                    ScenarioValue::Literal(x),
                ) if r == x => Some(true),
                _ => None,
            }
        };
        let mut current = true;
        let mut maps =
            |a: Vec<(String, &SkeletonValue)>, b: Vec<(String, &ScenarioValue)>| -> Option<()> {
                if a.len() != b.len() {
                    return None;
                }
                for ((ka, va), (kb, vb)) in a.into_iter().zip(b) {
                    if ka != kb {
                        return None;
                    }
                    current &= position(va, vb)?;
                }
                Some(())
            };
        maps(flatten_given(&self.given), flatten_given(&semantics.given))?;
        maps(flatten_then(&self.then), flatten_then(&semantics.then))?;
        let g = &semantics.given;
        let t = &semantics.then;
        if g.entities_exist != self.given.entities_exist
            || g.states_active != self.given.states_active
            || t.states_active != self.then.states_active
            || t.outcomes != self.then.outcomes
            || t.events != self.then.events
            || t.rules_satisfied != self.then.rules_satisfied
        {
            return None;
        }
        Some(current)
    }
}

fn flatten_given<V>(g: &ScenarioGiven<V>) -> Vec<(String, &V)> {
    let mut out: Vec<(String, &V)> = g
        .attribute_values
        .iter()
        .map(|(k, v)| (format!("a:{k}"), v))
        .collect();
    out.extend(g.decision_inputs.iter().map(|(k, v)| (format!("d:{k}"), v)));
    for (calc, m) in &g.calculation_semantics {
        out.extend(m.iter().map(|(key, v)| (format!("c:{calc}:{key}"), v)));
    }
    out
}

fn flatten_then<V>(t: &ScenarioThen<V>) -> Vec<(String, &V)> {
    let mut out: Vec<(String, &V)> = t
        .attribute_values
        .iter()
        .map(|(k, v)| (format!("a:{k}"), v))
        .collect();
    out.extend(
        t.decision_outputs
            .iter()
            .map(|(k, v)| (format!("d:{k}"), v)),
    );
    out.extend(
        t.calculation_results
            .iter()
            .map(|(k, v)| (format!("r:{k}"), v)),
    );
    out
}

/// Scenario ID: `scenario:<first 16 hex of SHA-256(RFC 8785 body)>` over the obligation only.
fn scenario_id(project_id: &Id, draft: &SkeletonDraft) -> Result<Id, ScenarioError> {
    let family = draft.family;
    let mut body = json!({
        "project_id": project_id,
        "scenario_kind": family.kind(),
        "derivation_family": family.kind(),
        "source_refs": draft.source_refs,
        "skeleton_semantics": {
            "given": draft.given.to_values(SkeletonValue::identity_json),
            "when": draft.when.to_value(),
            "then": draft.then.to_values(SkeletonValue::identity_json),
        },
    });
    if let Some(row) = draft.row_index {
        body["row_index"] = json!(row);
    }
    if let Some(key) = &draft.boundary_key {
        body["boundary_key"] = json!(key);
    }
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    Ok(format!("scenario:{}", &hex[..16]).parse()?)
}

struct SkeletonDraft {
    family: ScenarioFamily,
    source_refs: Vec<Id>,
    row_index: Option<usize>,
    boundary_key: Option<String>,
    name: String,
    given: ScenarioGiven<SkeletonValue>,
    when: ScenarioWhen,
    then: ScenarioThen<SkeletonValue>,
    requirement_refs: Vec<Id>,
    derived_from_refs: Vec<Id>,
    slots: Vec<ScenarioValueSlot>,
}

impl SkeletonDraft {
    fn finish(self, project_id: &Id) -> Result<ScenarioSkeleton, ScenarioError> {
        let scenario_ref = scenario_id(project_id, &self)?;
        let mut value_slots = self.slots;
        value_slots.sort();
        let mut skeleton = ScenarioSkeleton {
            scenario_ref,
            family: self.family,
            name: self.name,
            given: self.given,
            when: self.when,
            then: self.then,
            requirement_refs: self.requirement_refs,
            derived_from_refs: self.derived_from_refs,
            value_slots,
            unresolved_dependencies: Vec::new(),
        };
        let unresolved: BTreeSet<ScenarioDependency> = skeleton
            .positions()
            .into_iter()
            .filter_map(|p| match p {
                SkeletonValue::Semantic {
                    dependency,
                    resolved: None,
                } => Some(dependency.clone()),
                _ => None,
            })
            .collect();
        skeleton.unresolved_dependencies = unresolved.into_iter().collect();
        Ok(skeleton)
    }
}

// ============================================================================ rule rows

fn decision_contract(
    ty: &DecisionType,
    interval: Option<ScenarioNumericConstraint>,
) -> Option<ScenarioValueContract> {
    Some(match ty {
        DecisionType::Bool => ScenarioValueContract::Bool,
        DecisionType::Enum { members } => {
            let set: BTreeSet<String> = members.iter().cloned().collect();
            ScenarioValueContract::Enum {
                values: set.into_iter().collect(),
            }
        }
        DecisionType::Int => ScenarioValueContract::Integer { interval },
        DecisionType::Decimal { scale } => ScenarioValueContract::Decimal {
            scale: *scale,
            interval,
            unit: None,
        },
        DecisionType::Unsupported { .. } => return None,
    })
}

fn numeric_literal(ty: &DecisionType, value: Decimal) -> Option<ScenarioLiteral> {
    match ty {
        DecisionType::Int => {
            let integral = value.fract().is_zero();
            i64::try_from(value.trunc().mantissa() / 10i128.pow(value.trunc().scale()))
                .ok()
                .filter(|_| integral)
                .map(|value| ScenarioLiteral::Int { value })
        }
        DecisionType::Decimal { scale } => {
            (value.scale() <= *scale).then(|| ScenarioLiteral::Decimal {
                value: value.to_string(),
                unit: None,
            })
        }
        _ => None,
    }
}

fn rule_row_skeletons(
    graph: &Graph,
    inputs: &ScenarioDerivationInputs,
) -> Result<Vec<SkeletonDraft>, ScenarioError> {
    let supplied: BTreeSet<&Id> = inputs
        .decision_tables
        .iter()
        .map(|t| &t.decision_table_ref)
        .collect();
    for id in graph.node_ids_by_type(NodeType::DecisionTable) {
        if accepted_of(graph, id, NodeType::DecisionTable).is_some() && !supplied.contains(id) {
            return Err(ScenarioError::MissingDecisionTableInput {
                decision_table_ref: id.clone(),
            });
        }
    }
    let mut drafts = Vec::new();
    for input in &inputs.decision_tables {
        let table_ref = &input.decision_table_ref;
        let bad_input = |reason: &str| ScenarioError::InvalidDecisionTableInput {
            decision_table_ref: table_ref.clone(),
            reason: reason.to_owned(),
        };
        let bad_spec = |reason: String| ScenarioError::InvalidDecisionTableSpec {
            decision_table_ref: table_ref.clone(),
            reason,
        };
        let table = match accepted_of(graph, table_ref, NodeType::DecisionTable).map(|n| &n.payload)
        {
            Some(NodePayload::DecisionTable(t)) => t,
            _ => return Err(bad_input("not an Accepted DecisionTable")),
        };
        let spec = &input.spec;
        if spec.hit_policy != table.hit_policy {
            return Err(bad_input(
                "spec hit_policy differs from the DecisionTable hit_policy",
            ));
        }
        let analysis = analyze_decision_table(spec);
        if !analysis.structure.is_empty() {
            return Err(bad_spec(format!("{:?}", analysis.structure)));
        }
        for (row_index, row) in spec.rows.iter().enumerate() {
            let mut given = ScenarioGiven::default();
            let mut slots = Vec::new();
            for (column_index, (cell, column)) in row.inputs.iter().zip(&spec.inputs).enumerate() {
                let column_key = u32::try_from(column_index)
                    .map_err(|_| bad_spec("too many columns".to_owned()))?;
                let slot_id = format!("decision-input:{table_ref}:{row_index}:{column_index}");
                let mut slot = |contract: Option<ScenarioValueContract>| -> Result<SkeletonValue, ScenarioError> {
                    let contract = contract.ok_or_else(|| bad_spec(format!("column {column_index} is unsupported")))?;
                    contract.validate().map_err(&bad_spec)?;
                    slots.push(ScenarioValueSlot {
                        slot_id: slot_id.clone(),
                        contract,
                    });
                    Ok(SkeletonValue::Slot(slot_id.clone()))
                };
                let value = match cell {
                    DecisionInputCell::Bool { value } => {
                        SkeletonValue::Literal(ScenarioLiteral::Bool { value: *value })
                    }
                    DecisionInputCell::EnumSet { members } if members.len() == 1 => {
                        SkeletonValue::Literal(ScenarioLiteral::Enum {
                            value: members[0].clone(),
                        })
                    }
                    DecisionInputCell::EnumSet { members } => {
                        let set: BTreeSet<String> = members.iter().cloned().collect();
                        slot(Some(ScenarioValueContract::Enum {
                            values: set.into_iter().collect(),
                        }))?
                    }
                    DecisionInputCell::Interval { interval } => {
                        match (&interval.lower, &interval.upper) {
                            (Some(l), Some(u))
                                if l.value == u.value && l.inclusive && u.inclusive =>
                            {
                                SkeletonValue::Literal(
                                    numeric_literal(&column.ty, l.value).ok_or_else(|| {
                                        bad_spec(format!(
                                            "point {} does not fit column {column_index}",
                                            l.value
                                        ))
                                    })?,
                                )
                            }
                            _ => slot(decision_contract(
                                &column.ty,
                                Some(ScenarioNumericConstraint::from_interval(interval)),
                            ))?,
                        }
                    }
                    DecisionInputCell::Any => {
                        let interval = column
                            .domain
                            .as_ref()
                            .map(ScenarioNumericConstraint::from_domain);
                        let interval = match column.ty {
                            DecisionType::Int | DecisionType::Decimal { .. } => interval,
                            _ => None,
                        };
                        slot(decision_contract(&column.ty, interval))?
                    }
                };
                given.decision_inputs.insert(column_key, value);
            }
            let mut then = ScenarioThen::default();
            for (column_index, cell) in row.outputs.iter().enumerate() {
                let literal = match cell {
                    DecisionOutputCell::Bool { value } => ScenarioLiteral::Bool { value: *value },
                    DecisionOutputCell::Enum { member } => ScenarioLiteral::Enum {
                        value: member.clone(),
                    },
                    DecisionOutputCell::Int { value } => ScenarioLiteral::Int { value: *value },
                    DecisionOutputCell::Decimal { value } => ScenarioLiteral::Decimal {
                        value: value.to_string(),
                        unit: None,
                    },
                };
                let key = u32::try_from(column_index)
                    .map_err(|_| bad_spec("too many columns".to_owned()))?;
                then.decision_outputs
                    .insert(key, SkeletonValue::Literal(literal));
            }
            drafts.push(SkeletonDraft {
                family: ScenarioFamily::RuleRow,
                source_refs: vec![table_ref.clone()],
                row_index: Some(row_index),
                boundary_key: None,
                name: format!("Rule row {table_ref} row {row_index}"),
                given,
                when: ScenarioWhen::DecisionTable(table_ref.clone()),
                then,
                requirement_refs: Vec::new(),
                derived_from_refs: vec![table_ref.clone()],
                slots,
            });
        }
    }
    Ok(drafts)
}

// ============================================================================ transitions

fn transition_skeletons(graph: &Graph) -> Result<Vec<SkeletonDraft>, ScenarioError> {
    let mut drafts = Vec::new();
    for id in graph.node_ids_by_type(NodeType::Transition) {
        let Some(node) = accepted_of(graph, id, NodeType::Transition) else {
            continue;
        };
        let NodePayload::Transition(transition) = &node.payload else {
            continue;
        };
        let invalid = |reason: &str| ScenarioError::InvalidTransition {
            transition_ref: id.clone(),
            reason: reason.to_owned(),
        };
        if accepted_of(graph, &transition.from_state, NodeType::State).is_none() {
            return Err(invalid("from_state is not an Accepted State"));
        }
        if accepted_of(graph, &transition.to_state, NodeType::State).is_none() {
            return Err(invalid("to_state is not an Accepted State"));
        }
        let triggers: Vec<&Id> = graph
            .outgoing_edge_ids(id)
            .iter()
            .filter_map(|e| graph.edge(e))
            .filter(|e| {
                e.kind == RelationKind::TransitionsVia && e.status == ElementStatus::Accepted
            })
            .map(|e| &e.to)
            .collect();
        let trigger = match triggers.as_slice() {
            [] => {
                return Err(ScenarioError::MissingTrigger {
                    transition_ref: id.clone(),
                })
            }
            [only] => *only,
            _ => {
                return Err(ScenarioError::AmbiguousTrigger {
                    transition_ref: id.clone(),
                })
            }
        };
        let when = if accepted_of(graph, trigger, NodeType::Operation).is_some() {
            ScenarioWhen::Operation(trigger.clone())
        } else if accepted_of(graph, trigger, NodeType::Event).is_some() {
            ScenarioWhen::Event(trigger.clone())
        } else {
            return Err(invalid("trigger is not an Accepted Operation or Event"));
        };
        let given = ScenarioGiven {
            states_active: vec![transition.from_state.clone()],
            ..ScenarioGiven::default()
        };
        let then = ScenarioThen {
            states_active: vec![transition.to_state.clone()],
            ..ScenarioThen::default()
        };
        drafts.push(SkeletonDraft {
            family: ScenarioFamily::StateTransition,
            source_refs: vec![id.clone()],
            row_index: None,
            boundary_key: None,
            name: format!("Transition {id}"),
            given,
            when,
            then,
            requirement_refs: Vec::new(),
            derived_from_refs: vec![id.clone()],
            slots: Vec::new(),
        });
    }
    Ok(drafts)
}

// ============================================================================ boundaries

const SOURCE_TYPES: [NodeType; 7] = [
    NodeType::Requirement,
    NodeType::AcceptanceCriterion,
    NodeType::Calculation,
    NodeType::Rule,
    NodeType::DecisionTable,
    NodeType::Transition,
    NodeType::Calendar,
];

/// Whether a contract matches an Accepted Attribute's exact type.
fn attribute_contract(
    attribute_ref: &Id,
    attribute: &plumb_psg::Attribute,
    contract: &ScenarioValueContract,
) -> bool {
    let ty = map_pilot_value_type(&attribute.value_type, None, Some(attribute_ref));
    let unit = attribute.unit.as_ref();
    match (ty, contract) {
        (Ok(Ty::Bool), ScenarioValueContract::Bool)
        | (Ok(Ty::String), ScenarioValueContract::String { .. })
        | (Ok(Ty::Date), ScenarioValueContract::Date)
        | (Ok(Ty::DateTime), ScenarioValueContract::DateTime) => unit.is_none(),
        (Ok(Ty::Int), ScenarioValueContract::Integer { .. }) => unit.is_none(),
        (Ok(Ty::Decimal(n)), ScenarioValueContract::Decimal { scale, unit: u, .. }) => {
            *scale == u32::from(n) && u.as_ref() == unit
        }
        (Ok(Ty::Enum(_)), ScenarioValueContract::Enum { values }) => {
            unit.is_none()
                && attribute
                    .enum_values
                    .as_ref()
                    .is_some_and(|members| values.iter().all(|v| members.contains(v)))
        }
        _ => false,
    }
}

fn boundary_skeleton(
    graph: &Graph,
    input: &ScenarioBoundaryInput,
) -> Result<SkeletonDraft, ScenarioError> {
    let invalid = |reason: String| ScenarioError::InvalidBoundaryInput {
        boundary_key: input.boundary_key.clone(),
        subject_ref: input.subject_ref.clone(),
        reason,
    };
    if !is_clean_text(&input.boundary_key) {
        return Err(invalid("boundary_key is not clean text".to_owned()));
    }
    if input.source_refs.is_empty() || !strictly_sorted(&input.source_refs) {
        return Err(invalid(
            "source_refs must be non-empty, sorted and unique".to_owned(),
        ));
    }
    for source in &input.source_refs {
        let ok = graph.node(source).is_some_and(|n| {
            n.status == ElementStatus::Accepted && SOURCE_TYPES.contains(&n.payload.node_type())
        });
        if !ok {
            return Err(invalid(format!(
                "source {source} is not an Accepted semantic source"
            )));
        }
    }
    let subject = graph
        .node(&input.subject_ref)
        .filter(|n| n.status == ElementStatus::Accepted)
        .ok_or_else(|| invalid("subject is not an Accepted node".to_owned()))?;
    let when = ScenarioWhen::for_node(subject)
        .ok_or_else(|| invalid("subject cannot anchor a scenario".to_owned()))?;
    let result_key = format!("{CALCULATION_RESULT_KEY_PREFIX}{}", input.boundary_key);
    let mut given = ScenarioGiven::default();
    let mut then = ScenarioThen::default();
    let mut slots = Vec::new();
    let calculation_subject =
        |calculation_ref: &Id| -> Result<QualifiedCalculation<'_>, ScenarioError> {
            if *calculation_ref != input.subject_ref {
                return Err(invalid("subject_ref must equal calculation_ref".to_owned()));
            }
            qualified_calculation(graph, calculation_ref).map_err(&invalid)
        };
    let attribute =
        |attribute_ref: &Id| match accepted_of(graph, attribute_ref, NodeType::Attribute)
            .map(|n| &n.payload)
        {
            Some(NodePayload::Attribute(a)) => Ok(a),
            _ => Err(invalid(format!(
                "{attribute_ref} is not an Accepted Attribute"
            ))),
        };
    match &input.kind {
        ScenarioBoundaryKind::Numeric {
            attribute_ref,
            value_contract,
        } => {
            value_contract.validate().map_err(&invalid)?;
            let numeric = matches!(
                value_contract,
                ScenarioValueContract::Integer { .. } | ScenarioValueContract::Decimal { .. }
            );
            if !numeric
                || !attribute_contract(attribute_ref, attribute(attribute_ref)?, value_contract)
            {
                return Err(invalid(
                    "value_contract does not match the numeric Attribute".to_owned(),
                ));
            }
            let slot_id = format!("boundary:{}:value", input.boundary_key);
            slots.push(ScenarioValueSlot {
                slot_id: slot_id.clone(),
                contract: value_contract.clone(),
            });
            given
                .attribute_values
                .insert(attribute_ref.clone(), SkeletonValue::Slot(slot_id));
            if let ScenarioWhen::Calculation(calculation_ref) = &when {
                then.calculation_results.insert(
                    calculation_ref.clone(),
                    SkeletonValue::semantic(calculation_ref, &result_key, None),
                );
            }
        }
        ScenarioBoundaryKind::CalculationSemantic {
            calculation_ref,
            semantic_key,
            input_symbol,
            value_contract,
        } => {
            let qualified = calculation_subject(calculation_ref)?;
            if !is_clean_text(semantic_key) {
                return Err(invalid("semantic_key is not clean text".to_owned()));
            }
            value_contract.validate().map_err(&invalid)?;
            let binding = qualified.root_binding(input_symbol).ok_or_else(|| {
                invalid(format!(
                    "{input_symbol} is not a Root input of the calculation"
                ))
            })?;
            let attribute_ref = binding.node_ref.clone();
            if !attribute_contract(&attribute_ref, attribute(&attribute_ref)?, value_contract) {
                return Err(invalid(
                    "value_contract does not match the bound Attribute".to_owned(),
                ));
            }
            let examples = qualified.examples(calculation_ref)?;
            let matching: Vec<&CalculationExampleV1> =
                examples.iter().filter(|e| &e.key == semantic_key).collect();
            let (input_value, result) = match matching.as_slice() {
                [] => (None, None),
                [example] => {
                    let mismatch = |reason: String| ScenarioError::ExampleContractMismatch {
                        calculation_ref: calculation_ref.clone(),
                        key: semantic_key.clone(),
                        reason,
                    };
                    let literal = example
                        .inputs
                        .iter()
                        .find(|i| &i.symbol == input_symbol)
                        .map(|i| i.value.clone())
                        .ok_or_else(|| {
                            mismatch(format!("the example does not bind {input_symbol}"))
                        })?;
                    value_contract.check(&literal).map_err(mismatch)?;
                    (Some(literal), example.expected_result.clone())
                }
                _ => {
                    return Err(ScenarioError::AmbiguousCalculationExample {
                        calculation_ref: calculation_ref.clone(),
                        key: semantic_key.clone(),
                    })
                }
            };
            given.attribute_values.insert(
                attribute_ref,
                SkeletonValue::semantic(calculation_ref, semantic_key, input_value),
            );
            then.calculation_results.insert(
                calculation_ref.clone(),
                SkeletonValue::semantic(calculation_ref, &result_key, result),
            );
        }
        ScenarioBoundaryKind::CalendarHoliday {
            calculation_ref,
            calendar_ref,
        } => {
            let qualified = calculation_subject(calculation_ref)?;
            if qualified.calculation.calendar_ref.as_ref() != Some(calendar_ref)
                || accepted_of(graph, calendar_ref, NodeType::Calendar).is_none()
            {
                return Err(invalid(
                    "calendar_ref is not the calculation's Accepted Calendar".to_owned(),
                ));
            }
            let call = qualified.working_days(graph).map_err(&invalid)?;
            for period in [call.start, call.end] {
                given.attribute_values.insert(
                    period,
                    SkeletonValue::semantic(calendar_ref, HOLIDAY_DATE_KEY, None),
                );
            }
            then.calculation_results.insert(
                calculation_ref.clone(),
                SkeletonValue::semantic(calculation_ref, &result_key, None),
            );
        }
        ScenarioBoundaryKind::InclusiveEnd { calculation_ref } => {
            let qualified = calculation_subject(calculation_ref)?;
            let call = qualified.working_days(graph).map_err(&invalid)?;
            let resolved = match call.inclusive {
                Ast::Literal(ExprLiteral::Bool(value)) => {
                    Some(ScenarioLiteral::Bool { value: *value })
                }
                _ => None,
            };
            for period in [call.start, call.end] {
                let slot_id = format!("given:attribute:{period}");
                slots.push(ScenarioValueSlot {
                    slot_id: slot_id.clone(),
                    contract: ScenarioValueContract::Date,
                });
                given
                    .attribute_values
                    .insert(period, SkeletonValue::Slot(slot_id));
            }
            given.calculation_semantics.insert(
                calculation_ref.clone(),
                BTreeMap::from([(
                    WORKING_DAYS_INCLUSIVE_KEY.to_owned(),
                    SkeletonValue::semantic(calculation_ref, WORKING_DAYS_INCLUSIVE_KEY, resolved),
                )]),
            );
            then.calculation_results.insert(
                calculation_ref.clone(),
                SkeletonValue::semantic(calculation_ref, &result_key, None),
            );
        }
    }
    let derived: BTreeSet<Id> = input
        .source_refs
        .iter()
        .cloned()
        .chain([input.subject_ref.clone()])
        .collect();
    let derived_from_refs: Vec<Id> = derived.into_iter().collect();
    let requirement_refs: Vec<Id> = input
        .source_refs
        .iter()
        .filter(|r| accepted_of(graph, r, NodeType::Requirement).is_some())
        .cloned()
        .collect();
    Ok(SkeletonDraft {
        family: ScenarioFamily::Boundary,
        source_refs: derived_from_refs.clone(),
        row_index: None,
        boundary_key: Some(input.boundary_key.clone()),
        name: format!("Boundary {} for {}", input.boundary_key, input.subject_ref),
        given,
        when,
        then,
        requirement_refs,
        derived_from_refs,
        slots,
    })
}

// ============================================================================ analysis

/// Derives every scenario skeleton from the graph and the explicit inputs, sorted by
/// scenario_ref. Any invalid input stops derivation.
pub fn analyze_scenarios(
    graph: &Graph,
    inputs: &ScenarioDerivationInputs,
) -> Result<Vec<ScenarioSkeleton>, ScenarioError> {
    inputs.validate_order()?;
    let mut drafts = rule_row_skeletons(graph, inputs)?;
    drafts.extend(transition_skeletons(graph)?);
    for boundary in &inputs.boundaries {
        drafts.push(boundary_skeleton(graph, boundary)?);
    }
    let mut skeletons = drafts
        .into_iter()
        .map(|d| d.finish(graph.project_id()))
        .collect::<Result<Vec<_>, _>>()?;
    skeletons.sort_by(|a, b| a.scenario_ref.cmp(&b.scenario_ref));
    if let Some(pair) = skeletons
        .windows(2)
        .find(|p| p[0].scenario_ref == p[1].scenario_ref)
    {
        return Err(invalid_input(format!(
            "two obligations derive scenario {}",
            pair[0].scenario_ref
        )));
    }
    Ok(skeletons)
}

// ============================================================================ inference request

/// The S4 scenario-values inference context: literal slots of one skeleton only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioInferenceContext {
    pub version: u32,
    pub project_id: Id,
    pub scenario_ref: Id,
    pub skeleton_hash: Hash,
    pub slots: Vec<ScenarioValueSlot>,
}

impl ScenarioInferenceContext {
    /// Generic SHA-256 of the RFC 8785 canonical context.
    pub fn content_hash(&self) -> Result<Hash, ScenarioError> {
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }
}

/// A scenario-values `InferenceRequest` together with the exact context it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioRequest {
    pub request: InferenceRequest,
    pub context: ScenarioInferenceContext,
}

/// An already-acquired scenario-values artifact for one request, with the caller's provenance.
#[derive(Debug, Clone)]
pub struct ScenarioInference<'a> {
    pub request: &'a ScenarioRequest,
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

fn skeleton_refs(skeleton: &ScenarioSkeleton) -> Vec<Id> {
    let mut refs: BTreeSet<Id> = BTreeSet::new();
    refs.extend(skeleton.requirement_refs.iter().cloned());
    refs.extend(skeleton.derived_from_refs.iter().cloned());
    refs.insert(skeleton.when.anchor().clone());
    let g = &skeleton.given;
    let t = &skeleton.then;
    refs.extend(g.attribute_values.keys().cloned());
    refs.extend(g.entities_exist.iter().cloned());
    refs.extend(g.states_active.iter().cloned());
    refs.extend(g.calculation_semantics.keys().cloned());
    refs.extend(t.attribute_values.keys().cloned());
    refs.extend(t.states_active.iter().cloned());
    refs.extend(t.outcomes.iter().cloned());
    refs.extend(t.events.iter().cloned());
    refs.extend(t.rules_satisfied.iter().cloned());
    refs.extend(t.calculation_results.keys().cloned());
    for position in skeleton.positions() {
        if let SkeletonValue::Semantic { dependency, .. } = position {
            refs.insert(dependency.owner_ref.clone());
        }
    }
    refs.into_iter().collect()
}

fn source_evidence(graph: &Graph, skeleton: &ScenarioSkeleton) -> BTreeSet<EvidenceRef> {
    skeleton
        .derived_from_refs
        .iter()
        .chain(&skeleton.requirement_refs)
        .filter_map(|id| graph.node(id))
        .flat_map(|n| n.evidence.iter().cloned())
        .collect()
}

fn request_for(
    graph: &Graph,
    skeleton: &ScenarioSkeleton,
    provider_policy: ProviderPolicy,
) -> Result<ScenarioRequest, ScenarioError> {
    if skeleton.value_slots.is_empty() {
        return Err(ScenarioError::NoInferenceSlots {
            scenario_ref: skeleton.scenario_ref.clone(),
        });
    }
    let context = ScenarioInferenceContext {
        version: SCENARIO_CONTEXT_VERSION,
        project_id: graph.project_id().clone(),
        scenario_ref: skeleton.scenario_ref.clone(),
        skeleton_hash: skeleton.content_hash()?,
        slots: skeleton.value_slots.clone(),
    };
    let request = InferenceRequest::new(
        SCENARIO_STAGE,
        SCENARIO_TASK_KIND.to_owned(),
        skeleton_refs(skeleton),
        source_evidence(graph, skeleton)
            .iter()
            .map(|e| e.as_id().clone())
            .collect(),
        context.content_hash()?,
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    Ok(ScenarioRequest { request, context })
}

/// Builds the S4 scenario-values request for one derived skeleton with literal slots. The
/// provider policy is always the caller's; no provider runs.
pub fn build_scenario_request(
    graph: &Graph,
    inputs: &ScenarioDerivationInputs,
    scenario_ref: &Id,
    provider_policy: ProviderPolicy,
) -> Result<ScenarioRequest, ScenarioError> {
    let skeletons = analyze_scenarios(graph, inputs)?;
    let skeleton = skeletons
        .iter()
        .find(|s| &s.scenario_ref == scenario_ref)
        .ok_or_else(|| ScenarioError::UnknownScenario {
            scenario_ref: scenario_ref.clone(),
        })?;
    request_for(graph, skeleton, provider_policy)
}

// ============================================================================ inference output

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioOutput {
    version: u32,
    scenario_ref: Id,
    values: Vec<OutputValue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputValue {
    slot_id: String,
    value: ScenarioLiteral,
}

fn compile_schema() -> Result<JSONSchema, ScenarioError> {
    let schema: Value =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| ScenarioError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| ScenarioError::SchemaCompilation {
            reason: e.to_string(),
        })
}

/// The validated literal values of one inference, keyed by slot.
fn inferred_values(
    graph: &Graph,
    skeleton: &ScenarioSkeleton,
    inference: &ScenarioInference<'_>,
) -> Result<BTreeMap<String, ScenarioLiteral>, ScenarioError> {
    let request = inference.request;
    request.request.validate()?;
    let expected = request_for(graph, skeleton, request.request.provider_policy.clone())?;
    if &expected != request {
        return Err(ScenarioError::StaleScenarioRequest {
            reason: "the request differs from the current skeleton's request".to_owned(),
        });
    }
    inference
        .artifact
        .validate_for(&request.request)
        .map_err(|e| ScenarioError::InvalidInferenceArtifact {
            reason: e.to_string(),
        })?;
    let output = inference.artifact.validated_output.as_value();
    if let Err(errors) = compile_schema()?.validate(output) {
        let mut messages: Vec<String> = errors
            .map(|e| format!("{e} at {}", e.instance_path))
            .collect();
        messages.sort();
        return Err(schema_invalid(messages.join("; ")));
    }
    let decoded: ScenarioOutput =
        serde_json::from_value(output.clone()).map_err(|e| schema_invalid(e.to_string()))?;
    if decoded.version != SCENARIO_OUTPUT_VERSION {
        return Err(schema_invalid(format!(
            "output version {} is not 1",
            decoded.version
        )));
    }
    if decoded.scenario_ref != skeleton.scenario_ref {
        return Err(ScenarioError::ScenarioMismatch {
            expected: skeleton.scenario_ref.clone(),
            found: decoded.scenario_ref,
        });
    }
    let mut values = BTreeMap::new();
    let mut last: Option<String> = None;
    for item in decoded.values {
        if last.as_ref().is_some_and(|l| *l >= item.slot_id) {
            return Err(ScenarioError::DuplicateSlot {
                slot_id: item.slot_id,
            });
        }
        last = Some(item.slot_id.clone());
        let slot = skeleton
            .slot(&item.slot_id)
            .ok_or_else(|| ScenarioError::UnknownSlot {
                slot_id: item.slot_id.clone(),
            })?;
        slot.contract.check(&item.value).map_err(|reason| {
            ScenarioError::SlotContractViolation {
                slot_id: item.slot_id.clone(),
                reason,
            }
        })?;
        values.insert(item.slot_id, item.value);
    }
    Ok(values)
}

// ============================================================================ derivation result

/// Audit metadata of new Scenario nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// A matching Scenario already at the deterministic ID.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioExisting {
    pub scenario_ref: Id,
    /// Accepted (ExistingAccepted) rather than Proposed (Existing).
    pub accepted: bool,
    /// Whether it holds the current materialization (no newer semantic resolution).
    pub current: bool,
}

/// Different material at a deterministic Scenario ID; never overwritten.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioIdCollision {
    pub scenario_ref: Id,
    pub reason: String,
}

/// The deterministic S4.1 output; every collection is sorted.
#[derive(Debug, Clone)]
pub struct ScenarioDerivationResult {
    pub skeletons: Vec<ScenarioSkeleton>,
    /// One proposal per new fully materialized Scenario.
    pub proposals: Vec<Proposal>,
    pub existing: Vec<ScenarioExisting>,
    pub collisions: Vec<ScenarioIdCollision>,
    /// Skeletons that still have unfilled literal slots and no matching Scenario.
    pub unresolved: Vec<Id>,
}

enum Reconciliation {
    New,
    Existing(ScenarioExisting),
    Collision(String),
}

fn reconcile(graph: &Graph, skeleton: &ScenarioSkeleton) -> Reconciliation {
    let Some(node) = graph.node(&skeleton.scenario_ref) else {
        return Reconciliation::New;
    };
    let NodePayload::Scenario(scenario) = &node.payload else {
        return Reconciliation::Collision("a non-Scenario node holds the ID".to_owned());
    };
    let status_ok = matches!(
        (node.status, scenario.confirmation_status.as_deref()),
        (ElementStatus::Proposed, Some(SCENARIO_DERIVED))
            | (
                ElementStatus::Accepted,
                Some(SCENARIO_DERIVED | SCENARIO_CONFIRMED)
            )
    );
    match skeleton.matches(scenario).filter(|_| status_ok) {
        Some(current) => Reconciliation::Existing(ScenarioExisting {
            scenario_ref: skeleton.scenario_ref.clone(),
            accepted: node.status == ElementStatus::Accepted,
            current,
        }),
        None => Reconciliation::Collision(
            "the Scenario at the ID differs from the obligation".to_owned(),
        ),
    }
}

fn scenario_proposal(
    graph: &Graph,
    skeleton: &ScenarioSkeleton,
    scenario: Scenario,
    audit: &ScenarioAudit,
    derivation_refs: Vec<DerivationRef>,
) -> Result<Proposal, ScenarioError> {
    validate_scenario(graph, &scenario)?;
    let evidence: Vec<EvidenceRef> = source_evidence(graph, skeleton).into_iter().collect();
    let node = Node {
        id: skeleton.scenario_ref.clone(),
        revision: 1,
        status: ElementStatus::Proposed,
        payload: NodePayload::Scenario(scenario),
        evidence: evidence.clone(),
        derivations: Vec::new(),
        standards: Vec::new(),
        tags: BTreeSet::new(),
        extensions: BTreeMap::new(),
        audit: AuditMeta::new(audit.created_by.clone(), audit.created_at, None, None)
            .map_err(invalid_proposal)?,
    };
    let proposal = Proposal::new(
        SCENARIO_STAGE,
        PatchSet {
            base_semantic_hash: graph.semantic_hash()?,
            patch: SemanticPatch::AddNode { node },
        },
        evidence,
        derivation_refs,
        ProposalMateriality::Semantic,
        AcceptancePolicy::HumanConfirm,
        None,
    )
    .map_err(invalid_proposal)?;
    // In-memory dry validation by the canonical engine; the candidate graph is discarded.
    apply_patch(graph, &proposal.patch_set)
        .map_err(|e| invalid_proposal(format!("proposal does not apply: {e}")))?;
    Ok(proposal)
}

/// Derives skeletons, merges validated inference literals and proposes every new fully
/// materialized Scenario; existing matches and collisions are reported, never overwritten.
pub fn derive_scenarios(
    graph: &Graph,
    inputs: &ScenarioDerivationInputs,
    audit: &ScenarioAudit,
    inferences: &[ScenarioInference<'_>],
) -> Result<ScenarioDerivationResult, ScenarioError> {
    let skeletons = analyze_scenarios(graph, inputs)?;
    let mut inferred: BTreeMap<Id, (BTreeMap<String, ScenarioLiteral>, DerivationRef)> =
        BTreeMap::new();
    for inference in inferences {
        let scenario_ref = &inference.request.context.scenario_ref;
        let skeleton = skeletons
            .iter()
            .find(|s| &s.scenario_ref == scenario_ref)
            .ok_or_else(|| ScenarioError::StaleScenarioRequest {
                reason: format!("no current skeleton {scenario_ref}"),
            })?;
        let values = inferred_values(graph, skeleton, inference)?;
        if inferred
            .insert(
                scenario_ref.clone(),
                (values, inference.derivation_ref.clone()),
            )
            .is_some()
        {
            return Err(ScenarioError::DuplicateInference {
                scenario_ref: scenario_ref.clone(),
            });
        }
    }
    let mut proposals = Vec::new();
    let mut existing = Vec::new();
    let mut collisions = Vec::new();
    let mut unresolved = Vec::new();
    let empty = BTreeMap::new();
    for skeleton in &skeletons {
        match reconcile(graph, skeleton) {
            Reconciliation::Existing(e) => existing.push(e),
            Reconciliation::Collision(reason) => collisions.push(ScenarioIdCollision {
                scenario_ref: skeleton.scenario_ref.clone(),
                reason,
            }),
            Reconciliation::New => {
                let (values, derivation) = match inferred.get(&skeleton.scenario_ref) {
                    Some((values, derivation)) => (values, Some(derivation)),
                    None => (&empty, None),
                };
                match skeleton.materialize(values) {
                    None => unresolved.push(skeleton.scenario_ref.clone()),
                    Some(scenario) => {
                        let derivation_refs = match derivation {
                            Some(d) if !skeleton.value_slots.is_empty() => vec![d.clone()],
                            _ => Vec::new(),
                        };
                        proposals.push(scenario_proposal(
                            graph,
                            skeleton,
                            scenario,
                            audit,
                            derivation_refs,
                        )?);
                    }
                }
            }
        }
    }
    proposals.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ScenarioDerivationResult {
        skeletons,
        proposals,
        existing,
        collisions,
        unresolved,
    })
}
