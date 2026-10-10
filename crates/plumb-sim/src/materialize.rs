//! Scenario admission, Given materialization, permitted semantic resolution and value
//! conversion (plan S4.2, Hotfix 051).
//!
//! The Scenario wire is decoded only through plumb-functional (`validate_scenario`); there is no
//! second decoder here. A Given SemanticRef resolves only from an Accepted Calculation example
//! with its exact key, from the literal inclusive argument of the accepted `working_days` call or
//! from an allowlisted runtime environment value; everything else stays missing and nothing is
//! defaulted.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{canonical_hash, Hash, HashKind, Id, Timestamp};
use plumb_expr::{
    map_pilot_value_type, parse, Ast, Identifier, Literal as ExprLiteral, RefValue, Symbol, Ty,
    TypeEnv, Value as ExprValue, ValueEnv,
};
use plumb_functional::{
    validate_scenario, CalculationBindingExposure, CalculationExampleV1, CalculationScopeBinding,
    ScenarioDependency, ScenarioLiteral, ScenarioSemantics, ScenarioValue,
    CALCULATION_EXAMPLE_VERSION, HOLIDAY_DATE_KEY, SCENARIO_CONFIRMED, WORKING_DAYS_INCLUSIVE_KEY,
};
use plumb_psg::{ElementStatus, Graph, Node, NodePayload, NodeType, RelationKind};
use serde_json::json;

use crate::model::{
    accepted, missing_keys, CalculationEntry, ExecutionInputKind, ExecutionModel, ExecutionRuntime,
    ExecutionTraceStep, MissingSemantic, MissingSemanticKind, QualifiedCalculation,
    ResolutionSource, RuntimeState, RuntimeValue, SimulationError, EXECUTION_CONTRACT_VERSION,
};

/// The frozen PlumbExpr callable whose third argument carries inclusive-end semantics.
const WORKING_DAYS: &str = "working_days";

/// Runtime environment keys a runtime semantic value may supply.
const RUNTIME_ENVIRONMENT_KEYS: [&str; 1] = [HOLIDAY_DATE_KEY];

// ============================================================================ admission

/// The admitted Scenario: its decoded semantics and specification hash.
pub(crate) struct Admitted {
    pub(crate) semantics: ScenarioSemantics,
    pub(crate) specification_hash: Hash,
}

/// Admits only an Accepted, Confirmed Scenario decoded through plumb-functional.
pub(crate) fn admit(
    model: &ExecutionModel<'_>,
    scenario_ref: &Id,
) -> Result<Admitted, SimulationError> {
    let node = model
        .graph
        .node(scenario_ref)
        .ok_or_else(|| SimulationError::NotAScenario {
            scenario_ref: scenario_ref.clone(),
        })?;
    let NodePayload::Scenario(scenario) = &node.payload else {
        return Err(SimulationError::NotAScenario {
            scenario_ref: scenario_ref.clone(),
        });
    };
    if node.status != ElementStatus::Accepted {
        return Err(SimulationError::ScenarioNotAccepted {
            scenario_ref: scenario_ref.clone(),
        });
    }
    if scenario.confirmation_status.as_deref() != Some(SCENARIO_CONFIRMED) {
        return Err(SimulationError::ScenarioNotConfirmed {
            scenario_ref: scenario_ref.clone(),
            confirmation_status: scenario.confirmation_status.clone(),
        });
    }
    let semantics =
        validate_scenario(model.graph, scenario).map_err(|e| SimulationError::InvalidScenario {
            scenario_ref: scenario_ref.clone(),
            reason: e.to_string(),
        })?;
    let specification_hash = canonical_hash(
        HashKind::Generic,
        &json!({
            "execution_contract_version": EXECUTION_CONTRACT_VERSION,
            "scenario_ref": scenario_ref,
            "scenario": scenario,
        }),
    )?;
    Ok(Admitted {
        semantics,
        specification_hash,
    })
}

/// The canonical runtime environment values; anything that would answer business semantics is
/// rejected.
pub(crate) fn runtime_values(
    model: &ExecutionModel<'_>,
    runtime: &ExecutionRuntime<'_>,
) -> Result<BTreeMap<(Id, String), ScenarioLiteral>, SimulationError> {
    let mut out = BTreeMap::new();
    for v in &runtime.semantic_values {
        let disallowed = || SimulationError::DisallowedRuntimeValue {
            owner_ref: v.owner_ref.clone(),
            key: v.key.clone(),
        };
        if !RUNTIME_ENVIRONMENT_KEYS.contains(&v.key.as_str())
            || accepted(model.graph, &v.owner_ref, NodeType::Calendar).is_none()
        {
            return Err(disallowed());
        }
        if !matches!(v.value, ScenarioLiteral::Date { .. }) || v.value.validate().is_err() {
            return Err(SimulationError::InvalidInput {
                input: ExecutionInputKind::RuntimeSemanticValue,
                owner_ref: v.owner_ref.clone(),
                reason: format!("{} must be a canonical Date literal", v.key),
            });
        }
        if out
            .insert((v.owner_ref.clone(), v.key.clone()), v.value.clone())
            .is_some()
        {
            return Err(SimulationError::DuplicateInput {
                input: ExecutionInputKind::RuntimeSemanticValue,
                owner_ref: v.owner_ref.clone(),
            });
        }
    }
    Ok(out)
}

// ============================================================================ value conversion

fn decimal_scale(value: &str) -> usize {
    value
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len())
}

/// A canonical `YYYY-MM-DD` date through the plumb-core timestamp parser (UTC midnight).
fn date_value(value: &str) -> Option<ExprValue> {
    let at: Timestamp = format!("{value}T00:00:00Z").parse().ok()?;
    Some(ExprValue::Date(at.as_offset_datetime().date()))
}

/// A literal as a PlumbExpr value of exactly `ty`; `None` when they do not match (no coercion).
pub(crate) fn to_expr_value(
    literal: &ScenarioLiteral,
    ty: &Ty,
    is_member: &dyn Fn(&Id, &str) -> bool,
) -> Option<ExprValue> {
    literal.validate().ok()?;
    Some(match (literal, ty) {
        (ScenarioLiteral::Bool { value }, Ty::Bool) => ExprValue::Bool(*value),
        (ScenarioLiteral::Int { value }, Ty::Int) => ExprValue::Int(*value),
        (ScenarioLiteral::Decimal { value, unit: None }, Ty::Decimal(scale))
            if decimal_scale(value) <= usize::from(*scale) =>
        {
            ExprValue::Decimal(value.parse().ok()?)
        }
        (
            ScenarioLiteral::Decimal {
                value,
                unit: Some(u),
            },
            Ty::Quantity(q),
        ) if u == q.as_str() => ExprValue::Quantity {
            value: value.parse().ok()?,
            unit: q.clone(),
        },
        (ScenarioLiteral::String { value }, Ty::String) => ExprValue::String(value.clone()),
        (ScenarioLiteral::Date { value }, Ty::Date) => date_value(value)?,
        (ScenarioLiteral::DateTime { value }, Ty::DateTime) => ExprValue::DateTime(*value),
        (ScenarioLiteral::Enum { value }, Ty::Enum(enum_ref)) if is_member(enum_ref, value) => {
            ExprValue::Enum {
                enum_ref: enum_ref.clone(),
                member: Symbol::new(value).ok()?,
            }
        }
        _ => return None,
    })
}

/// A PlumbExpr result as a lossless literal; Duration, Ref and List have no literal.
pub(crate) fn from_expr_value(value: &ExprValue) -> Option<ScenarioLiteral> {
    Some(match value {
        ExprValue::Bool(value) => ScenarioLiteral::Bool { value: *value },
        ExprValue::Int(value) => ScenarioLiteral::Int { value: *value },
        ExprValue::Decimal(d) => ScenarioLiteral::Decimal {
            value: d.to_string(),
            unit: None,
        },
        ExprValue::Quantity { value, unit } => ScenarioLiteral::Decimal {
            value: value.to_string(),
            unit: Some(unit.as_str().to_owned()),
        },
        ExprValue::String(value) => ScenarioLiteral::String {
            value: value.clone(),
        },
        ExprValue::Date(d) => ScenarioLiteral::Date {
            value: format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day()),
        },
        ExprValue::DateTime(value) => ScenarioLiteral::DateTime { value: *value },
        ExprValue::Enum { member, .. } => ScenarioLiteral::Enum {
            value: member.as_str().to_owned(),
        },
        ExprValue::Duration { .. } | ExprValue::Ref(_) | ExprValue::List(_) => return None,
    })
    .filter(|literal| literal.validate().is_ok())
}

/// Membership in a TypeEnv enum.
pub(crate) fn env_member(env: &TypeEnv) -> impl Fn(&Id, &str) -> bool + '_ {
    |enum_ref, value| {
        env.enum_members(enum_ref)
            .is_some_and(|members| members.iter().any(|m| m.as_str() == value))
    }
}

/// Whether a literal conforms to the pilot type of an Accepted Attribute.
fn conforms_to_attribute(node: &Node, literal: &ScenarioLiteral) -> bool {
    let NodePayload::Attribute(a) = &node.payload else {
        return false;
    };
    let Ok(ty) = map_pilot_value_type(&a.value_type, a.unit.as_deref(), Some(&node.id)) else {
        return false;
    };
    let members = a.enum_values.clone().unwrap_or_default();
    let is_member = |_: &Id, value: &str| members.iter().any(|m| m == value);
    to_expr_value(literal, &ty, &is_member).is_some()
}

// ============================================================================ graph helpers

/// The canonical Accepted Entity owning an Attribute through one Accepted `has_attribute`.
pub(crate) fn attribute_owner<'g>(graph: &'g Graph, attribute: &Id) -> Option<&'g Id> {
    let owners: Vec<&Id> = graph
        .incoming_edge_ids(attribute)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == RelationKind::HasAttribute && e.status == ElementStatus::Accepted)
        .filter(|e| accepted(graph, &e.from, NodeType::Entity).is_some())
        .map(|e| &e.from)
        .collect();
    match owners.as_slice() {
        [owner] => Some(owner),
        _ => None,
    }
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

/// Every identifier of an expression, in source order.
pub(crate) fn collect_identifiers<'a>(ast: &'a Ast, out: &mut Vec<&'a Identifier>) {
    match ast {
        Ast::Identifier(identifier) => out.push(identifier),
        Ast::Call { args, .. } | Ast::List(args) => {
            args.iter().for_each(|a| collect_identifiers(a, out))
        }
        Ast::Unary { expr, .. } => collect_identifiers(expr, out),
        Ast::Binary { left, right, .. } => {
            collect_identifiers(left, out);
            collect_identifiers(right, out);
        }
        Ast::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_identifiers(condition, out);
            collect_identifiers(then_expr, out);
            collect_identifiers(else_expr, out);
        }
        Ast::Literal(_) => {}
    }
}

/// The literal Bool third argument of the single `working_days` call, if literal.
fn inclusive_end_literal(
    model: &ExecutionModel<'_>,
    calculation_ref: &Id,
) -> Result<Option<bool>, SimulationError> {
    let invalid = |reason: String| SimulationError::InvalidCalculation {
        calculation_ref: calculation_ref.clone(),
        reason,
    };
    let parsed;
    let ast = match model.calculations.get(calculation_ref) {
        Some(CalculationEntry::Qualified(q)) => &q.prepared.ast,
        _ => {
            let Some(NodePayload::Calculation(c)) =
                accepted(model.graph, calculation_ref, NodeType::Calculation).map(|n| &n.payload)
            else {
                return Err(invalid("not an Accepted Calculation".to_owned()));
            };
            parsed = parse(&c.expression).map_err(|e| invalid(e.to_string()))?;
            &parsed
        }
    };
    let mut calls = Vec::new();
    collect_calls(ast, &mut calls);
    let [args] = calls.as_slice() else {
        return Err(invalid(format!(
            "expected exactly one working_days call, found {}",
            calls.len()
        )));
    };
    Ok(match args.get(2) {
        Some(Ast::Literal(ExprLiteral::Bool(value))) => Some(*value),
        Some(_) => None,
        None => return Err(invalid("working_days has no inclusive argument".to_owned())),
    })
}

fn is_clean_text(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

/// Every example of a qualified calculation under the strict S4.1 v1 contract.
fn calculation_examples(
    node: &Node,
    qualified: &QualifiedCalculation,
) -> Result<Vec<CalculationExampleV1>, SimulationError> {
    let NodePayload::Calculation(calculation) = &node.payload else {
        return Ok(Vec::new());
    };
    let invalid = |reason: String| SimulationError::InvalidCalculationExample {
        calculation_ref: node.id.clone(),
        reason,
    };
    let env = &qualified.prepared.type_env;
    let is_member = env_member(env);
    let mut out = Vec::new();
    for value in calculation.examples.iter().flatten() {
        let example: CalculationExampleV1 =
            serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
        if serde_json::to_value(&example).ok().as_ref() != Some(value) {
            return Err(invalid("example is not in canonical v1 form".to_owned()));
        }
        if example.version != CALCULATION_EXAMPLE_VERSION || !is_clean_text(&example.key) {
            return Err(invalid("example version or key is invalid".to_owned()));
        }
        let symbols: Vec<&String> = example.inputs.iter().map(|i| &i.symbol).collect();
        if symbols.is_empty() || symbols.windows(2).any(|w| w[0] >= w[1]) {
            return Err(invalid(
                "example inputs must be non-empty, sorted and unique".to_owned(),
            ));
        }
        for input in &example.inputs {
            let ty = qualified
                .used_bindings
                .iter()
                .find(|b| {
                    b.symbol == input.symbol && b.exposure == CalculationBindingExposure::Root
                })
                .and_then(|_| Symbol::new(&input.symbol).ok())
                .and_then(|s| env.root(&s))
                .ok_or_else(|| invalid(format!("{} is not a scope input", input.symbol)))?;
            if to_expr_value(&input.value, ty, &is_member).is_none() {
                return Err(invalid(format!("{} does not type-check", input.symbol)));
            }
        }
        if let Some(result) = &example.expected_result {
            if to_expr_value(result, &qualified.prepared.expected_ty, &is_member).is_none() {
                return Err(invalid("expected_result does not type-check".to_owned()));
            }
        }
        out.push(example);
    }
    Ok(out)
}

/// The value of a Calculation-owned semantic key in the position of `attribute_ref`, from the
/// single Accepted example with that key.
fn example_value(
    model: &ExecutionModel<'_>,
    dependency: &ScenarioDependency,
    attribute_ref: &Id,
) -> Result<Option<ScenarioLiteral>, SimulationError> {
    let calculation_ref = &dependency.owner_ref;
    let (Some(node), Some(CalculationEntry::Qualified(qualified))) = (
        accepted(model.graph, calculation_ref, NodeType::Calculation),
        model.calculations.get(calculation_ref),
    ) else {
        return Ok(None);
    };
    let examples = calculation_examples(node, qualified)?;
    let matching: Vec<&CalculationExampleV1> = examples
        .iter()
        .filter(|e| e.key == dependency.key)
        .collect();
    let example = match matching.as_slice() {
        [] => return Ok(None),
        [example] => example,
        _ => {
            return Err(SimulationError::AmbiguousCalculationExample {
                calculation_ref: calculation_ref.clone(),
                key: dependency.key.clone(),
            })
        }
    };
    let symbol = qualified
        .used_bindings
        .iter()
        .find(|b| &b.node_ref == attribute_ref && b.exposure == CalculationBindingExposure::Root)
        .map(|b| &b.symbol);
    example
        .inputs
        .iter()
        .find(|i| Some(&i.symbol) == symbol)
        .map(|i| Some(i.value.clone()))
        .ok_or_else(|| SimulationError::InvalidCalculationExample {
            calculation_ref: calculation_ref.clone(),
            reason: format!(
                "the {} example does not bind {attribute_ref}",
                dependency.key
            ),
        })
}

// ============================================================================ materialization

/// Where a Given value sits.
enum Position<'a> {
    Attribute(&'a Id),
    CalculationSemantic,
    DecisionInput,
}

/// The materialized Given: the initial state, decision inputs, trace and missing values.
pub(crate) struct Given {
    pub(crate) state: RuntimeState,
    pub(crate) decision_inputs: BTreeMap<u32, RuntimeValue>,
    pub(crate) trace: Vec<ExecutionTraceStep>,
    pub(crate) missing: BTreeSet<MissingSemantic>,
}

struct Materializer<'m, 'g> {
    model: &'m ExecutionModel<'g>,
    runtime_values: &'m BTreeMap<(Id, String), ScenarioLiteral>,
    trace: Vec<ExecutionTraceStep>,
    missing: BTreeSet<MissingSemantic>,
}

impl Materializer<'_, '_> {
    /// Resolves a SemanticRef only through the permitted sources; `None` is missing.
    fn resolve(
        &mut self,
        dependency: &ScenarioDependency,
        position: &Position<'_>,
    ) -> Result<Option<ScenarioLiteral>, SimulationError> {
        let graph = self.model.graph;
        let owner = &dependency.owner_ref;
        let key = dependency.key.as_str();
        let resolved = if accepted(graph, owner, NodeType::Calculation).is_some() {
            if key == WORKING_DAYS_INCLUSIVE_KEY {
                inclusive_end_literal(self.model, owner)?.map(|value| {
                    (
                        ScenarioLiteral::Bool { value },
                        ResolutionSource::CalculationExpression,
                    )
                })
            } else if let Position::Attribute(attribute_ref) = position {
                example_value(self.model, dependency, attribute_ref)?
                    .map(|v| (v, ResolutionSource::CalculationExample))
            } else {
                None
            }
        } else if accepted(graph, owner, NodeType::Calendar).is_some() && key == HOLIDAY_DATE_KEY {
            self.runtime_values
                .get(&(owner.clone(), key.to_owned()))
                .map(|v| (v.clone(), ResolutionSource::RuntimeValue))
        } else {
            None
        };
        Ok(match resolved {
            Some((value, source)) => {
                self.trace.push(ExecutionTraceStep::SemanticResolved {
                    owner_ref: owner.clone(),
                    key: key.to_owned(),
                    source,
                });
                Some(value)
            }
            None => {
                self.missing.insert(MissingSemantic {
                    owner_ref: owner.clone(),
                    key: key.to_owned(),
                    kind: MissingSemanticKind::SemanticRef,
                });
                None
            }
        })
    }

    fn value(
        &mut self,
        value: &ScenarioValue,
        position: &Position<'_>,
    ) -> Result<Option<ScenarioLiteral>, SimulationError> {
        match value {
            ScenarioValue::Literal(literal) => Ok(Some(literal.clone())),
            ScenarioValue::SemanticRef(dependency) => self.resolve(dependency, position),
        }
    }
}

/// Materializes the Given state; any missing value stops execution before any effect.
pub(crate) fn materialize(
    model: &ExecutionModel<'_>,
    scenario_ref: &Id,
    semantics: &ScenarioSemantics,
    runtime_values: &BTreeMap<(Id, String), ScenarioLiteral>,
) -> Result<Given, SimulationError> {
    let graph = model.graph;
    let given = &semantics.given;
    let mut m = Materializer {
        model,
        runtime_values,
        trace: vec![ExecutionTraceStep::MaterializationStarted {
            scenario_ref: scenario_ref.clone(),
        }],
        missing: BTreeSet::new(),
    };
    let mut state = RuntimeState::default();
    for (attribute_ref, value) in &given.attribute_values {
        if let Some(literal) = m.value(value, &Position::Attribute(attribute_ref))? {
            let node = accepted(graph, attribute_ref, NodeType::Attribute).ok_or_else(|| {
                SimulationError::InvalidScenario {
                    scenario_ref: scenario_ref.clone(),
                    reason: format!("{attribute_ref} is not an Accepted Attribute"),
                }
            })?;
            if !conforms_to_attribute(node, &literal) {
                return Err(SimulationError::ValueTypeMismatch {
                    owner_ref: attribute_ref.clone(),
                    reason: format!("{literal:?} does not match the Attribute type"),
                });
            }
            state.attributes.insert(attribute_ref.clone(), literal);
        }
    }
    for (calculation_ref, entries) in &given.calculation_semantics {
        for (key, value) in entries {
            let literal = m.value(value, &Position::CalculationSemantic)?;
            if let (Some(literal), WORKING_DAYS_INCLUSIVE_KEY) = (&literal, key.as_str()) {
                let accepted_value = inclusive_end_literal(model, calculation_ref)?;
                if accepted_value
                    .map(|value| ScenarioLiteral::Bool { value })
                    .as_ref()
                    != Some(literal)
                {
                    return Err(SimulationError::SemanticContradiction {
                        owner_ref: calculation_ref.clone(),
                        key: key.clone(),
                    });
                }
            }
        }
    }
    let mut decision_inputs = BTreeMap::new();
    for (index, value) in &given.decision_inputs {
        if let Some(literal) = m.value(value, &Position::DecisionInput)? {
            decision_inputs.insert(*index, literal);
        }
    }
    state.entities = given.entities_exist.iter().cloned().collect();
    state.active_states = given.states_active.iter().cloned().collect();
    let Materializer {
        mut trace, missing, ..
    } = m;
    if missing.is_empty() {
        trace.push(ExecutionTraceStep::GivenMaterialized {
            attribute_refs: state.attributes.keys().cloned().collect(),
            entity_refs: state.entities.iter().cloned().collect(),
            state_refs: state.active_states.iter().cloned().collect(),
        });
    }
    Ok(Given {
        state,
        decision_inputs,
        trace,
        missing,
    })
}

// ============================================================================ value environments

/// The bindings an expression uses: the Root binding of each identifier and, for a navigation
/// of a Root Entity, its Field binding. Unbound identifiers (contextual enum members) are
/// skipped; the expression already type-checked.
pub(crate) fn expression_bindings(
    ast: &Ast,
    roots: &BTreeMap<String, CalculationScopeBinding>,
    fields: &BTreeMap<(Id, String), CalculationScopeBinding>,
) -> Vec<CalculationScopeBinding> {
    let mut identifiers = Vec::new();
    collect_identifiers(ast, &mut identifiers);
    let mut out = BTreeSet::new();
    for identifier in identifiers {
        let segments = identifier.segments();
        let Some(root) = roots.get(segments[0].as_str()) else {
            continue;
        };
        out.insert(root.clone());
        if let Some(field) = segments.get(1) {
            if let Some(binding) = fields.get(&(root.node_ref.clone(), field.as_str().to_owned())) {
                out.insert(binding.clone());
            }
        }
    }
    out.into_iter().collect()
}

fn missing(owner_ref: &Id, key: &str, kind: MissingSemanticKind) -> MissingSemantic {
    MissingSemantic {
        owner_ref: owner_ref.clone(),
        key: key.to_owned(),
        kind,
    }
}

/// The runtime values of `used` bindings. Absent Attribute values and Entity instances are added
/// to `out_missing`; an absent Calculation result (already reported) only makes the result
/// `None`.
pub(crate) fn bind_values(
    graph: &Graph,
    env: &TypeEnv,
    used: &[CalculationScopeBinding],
    state: &RuntimeState,
    out_missing: &mut BTreeSet<MissingSemantic>,
) -> Result<Option<ValueEnv>, SimulationError> {
    let is_member = env_member(env);
    let mut values = ValueEnv::new();
    let mut complete = true;
    let mismatch = |owner_ref: &Id| SimulationError::ValueTypeMismatch {
        owner_ref: owner_ref.clone(),
        reason: "the runtime value does not match its scope type".to_owned(),
    };
    let symbol = |s: &str| {
        Symbol::new(s).map_err(|e| SimulationError::Evaluation {
            owner_ref: graph.project_id().clone(),
            reason: e.to_string(),
        })
    };
    for binding in used {
        if binding.exposure != CalculationBindingExposure::Root {
            continue;
        }
        let root = symbol(&binding.symbol)?;
        let Some(ty) = env.root(&root) else {
            return Err(mismatch(&binding.node_ref));
        };
        let node_type = graph.node(&binding.node_ref).map(|n| n.payload.node_type());
        let value = match node_type {
            Some(NodeType::Attribute) => match state.attributes.get(&binding.node_ref) {
                Some(literal) => Some(
                    to_expr_value(literal, ty, &is_member)
                        .ok_or_else(|| mismatch(&binding.node_ref))?,
                ),
                None => {
                    out_missing.insert(missing(
                        &binding.node_ref,
                        missing_keys::ATTRIBUTE_VALUE,
                        MissingSemanticKind::AttributeValue,
                    ));
                    None
                }
            },
            Some(NodeType::Calculation) => match state.calculation_results.get(&binding.node_ref) {
                Some(literal) => Some(
                    to_expr_value(literal, ty, &is_member)
                        .ok_or_else(|| mismatch(&binding.node_ref))?,
                ),
                None => None,
            },
            Some(NodeType::Entity) => {
                let entity = &binding.node_ref;
                let present = state.entities.contains(entity)
                    || state
                        .attributes
                        .keys()
                        .any(|a| attribute_owner(graph, a) == Some(entity));
                if !present {
                    out_missing.insert(missing(
                        entity,
                        missing_keys::ENTITY_INSTANCE,
                        MissingSemanticKind::EntityInstance,
                    ));
                    None
                } else {
                    let mut fields = BTreeMap::new();
                    let mut fields_complete = true;
                    for field in used.iter().filter(|b| {
                        b.exposure
                            == CalculationBindingExposure::Field {
                                owner_ref: entity.clone(),
                            }
                    }) {
                        let name = symbol(&field.symbol)?;
                        let field_ty = env
                            .field(entity, &name)
                            .ok_or_else(|| mismatch(&field.node_ref))?;
                        match state.attributes.get(&field.node_ref) {
                            Some(literal) => {
                                fields.insert(
                                    name,
                                    to_expr_value(literal, field_ty, &is_member)
                                        .ok_or_else(|| mismatch(&field.node_ref))?,
                                );
                            }
                            None => {
                                fields_complete = false;
                                out_missing.insert(missing(
                                    &field.node_ref,
                                    missing_keys::ATTRIBUTE_VALUE,
                                    MissingSemanticKind::AttributeValue,
                                ));
                            }
                        }
                    }
                    fields_complete.then(|| {
                        ExprValue::Ref(RefValue {
                            type_ref: entity.clone(),
                            value_ref: entity.clone(),
                            fields,
                        })
                    })
                }
            }
            _ => return Err(mismatch(&binding.node_ref)),
        };
        match value {
            Some(value) => values
                .bind(root, value)
                .map_err(|_| mismatch(&binding.node_ref))?,
            None => complete = false,
        }
    }
    Ok(complete.then_some(values))
}
