//! The S4.2 execution model, its explicit supplemental inputs, the runtime invocation context
//! and the typed result, failure, missing-semantic and trace vocabulary (plan S4.2,
//! Hotfix 051).
//!
//! [`build_execution_model`] validates one graph baseline plus explicit supplemental inputs
//! once: typed decision-table specs, scope overrides for legacy calculations and explicit
//! expression scopes. Nothing is inferred from names, nothing is persisted and the graph is only
//! borrowed for reading.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{CoreError, Hash, Id};
use plumb_expr::{parse, typecheck, Ast, CalendarProvider, PredicateProvider, Ty, TypeEnv};
use plumb_functional::calculation::{
    analyze_calculation_cycles, validate_calculation_bindings, CalculationIssue,
    PreparedCalculation,
};
use plumb_functional::decision_table::{analyze_decision_table, DecisionTableSpec, HitPolicy};
use plumb_functional::{
    CalculationScope, CalculationScopeBinding, ScenarioDecisionTableInput, ScenarioLiteral,
};
use plumb_psg::{semantic_hash, ElementStatus, Graph, Node, NodePayload, NodeType};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Version of the S4.2 execution contract, part of every specification hash.
pub const EXECUTION_CONTRACT_VERSION: u32 = 1;

/// A runtime value: the S4.1 closed literal (no float).
pub type RuntimeValue = ScenarioLiteral;

// ============================================================================ inputs

/// An explicit scope for an Accepted Calculation without an S2.6 origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionCalculationScopeOverride {
    pub calculation_ref: Id,
    pub scope: CalculationScope,
}

/// The explicit expression scope of one Accepted Operation, Rule, Transition or Invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionExpressionScopeInput {
    pub owner_ref: Id,
    pub bindings: Vec<CalculationScopeBinding>,
}

/// Semantics the PSG cannot carry, supplied explicitly; never persisted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionInputs {
    pub decision_tables: Vec<ScenarioDecisionTableInput>,
    pub calculation_scope_overrides: Vec<ExecutionCalculationScopeOverride>,
    pub expression_scopes: Vec<ExecutionExpressionScopeInput>,
}

/// One runtime environment value (only `holiday_date` of an Accepted Calendar is allowed).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSemanticValue {
    pub owner_ref: Id,
    pub key: String,
    pub value: ScenarioLiteral,
}

/// Invocation context of one run: never PSG truth and never stored in a Scenario.
pub struct ExecutionRuntime<'a> {
    pub actor_ref: Option<Id>,
    pub semantic_values: Vec<RuntimeSemanticValue>,
    pub calendars: &'a dyn CalendarProvider,
    pub predicates: &'a dyn PredicateProvider,
}

// ============================================================================ state and results

/// The private runtime state of one run; Then values never enter it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RuntimeState {
    pub attributes: BTreeMap<Id, RuntimeValue>,
    pub entities: BTreeSet<Id>,
    pub active_states: BTreeSet<Id>,
    pub decision_outputs: BTreeMap<Id, BTreeMap<u32, RuntimeValue>>,
    pub calculation_results: BTreeMap<Id, RuntimeValue>,
}

/// Why a value is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingSemanticKind {
    SemanticRef,
    AttributeValue,
    EntityInstance,
    ExpressionScope,
    RuntimeActor,
    DecisionInput,
    DecisionTableSpec,
    CalculationScope,
    RuleCondition,
    UnsupportedExpression,
    PolicyCondition,
    OutcomeSelection,
}

/// One missing value, identified by `(owner_ref, key)`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissingSemantic {
    pub owner_ref: Id,
    pub key: String,
    pub kind: MissingSemanticKind,
}

/// The frozen generic missing-semantic keys.
pub mod missing_keys {
    pub const ATTRIBUTE_VALUE: &str = "value";
    pub const ENTITY_INSTANCE: &str = "exists";
    pub const EXPRESSION_SCOPE: &str = "expression_scope";
    pub const RUNTIME_ACTOR: &str = "runtime_actor";
    pub const DECISION_TABLE_SPEC: &str = "decision_table_spec";
    pub const CALCULATION_SCOPE: &str = "calculation_scope";
    pub const RULE_CONDITION: &str = "condition_expr";
    pub const POLICY_CONDITION: &str = "policy_condition";
    pub const OUTCOME_SELECTION: &str = "outcome_selection";

    /// `decision_input:<zero-based column index>`.
    pub fn decision_input(index: usize) -> String {
        format!("decision_input:{index}")
    }
}

/// The position of an executable expression of its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "site", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpressionSite {
    Precondition { index: u32 },
    Postcondition { index: u32 },
    RuleCondition,
    TransitionGuard,
    Invariant,
}

impl ExpressionSite {
    /// The missing-semantic key of an unparseable pre/postcondition.
    pub fn unsupported_key(self) -> Option<String> {
        match self {
            ExpressionSite::Precondition { index } => Some(format!("precondition:{index}")),
            ExpressionSite::Postcondition { index } => Some(format!("postcondition:{index}")),
            _ => None,
        }
    }
}

/// A data-dependent evaluator failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationFailureKind {
    DivisionByZero,
    Overflow,
    InvalidPeriod,
    NonIntegralDateDuration,
    DateOutOfRange,
    EmptyAggregate,
    Rounding,
    ResultScaleExceeded,
    CalendarUnavailable,
    PredicateUnavailable,
}

/// Accepted semantics explicitly reject execution with complete inputs. Never a Then mismatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "failure", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionFailure {
    AuthorizationDenied {
        operation_ref: Id,
        actor_ref: Id,
    },
    PreconditionRejected {
        operation_ref: Id,
        index: u32,
    },
    PostconditionViolation {
        operation_ref: Id,
        index: u32,
    },
    TransitionGuardRejected {
        transition_ref: Id,
    },
    InvalidSourceState {
        transition_ref: Id,
        from_state: Id,
    },
    InvariantViolation {
        invariant_ref: Id,
    },
    NoDecisionRow {
        decision_table_ref: Id,
    },
    EvaluationFailed {
        owner_ref: Id,
        kind: EvaluationFailureKind,
    },
}

/// How a Scenario SemanticRef was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    CalculationExample,
    CalculationExpression,
    RuntimeValue,
}

/// The authorization decision of an Operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthorizationDecision {
    /// No Accepted Permission permits the Operation.
    NotApplicable,
    /// Role-level grant by unconditioned permissions; resource scopes are not evaluated.
    Allowed {
        permission_refs: Vec<Id>,
        unevaluated_scope_refs: Vec<Id>,
    },
    Denied,
    /// Only conditioned permissions grant; their conditions are not evaluated.
    ConditionsNotEvaluated {
        permission_refs: Vec<Id>,
    },
}

/// One typed execution step, in execution order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum ExecutionTraceStep {
    MaterializationStarted {
        scenario_ref: Id,
    },
    SemanticResolved {
        owner_ref: Id,
        key: String,
        source: ResolutionSource,
    },
    SemanticMissing {
        missing: Vec<MissingSemantic>,
    },
    GivenMaterialized {
        attribute_refs: Vec<Id>,
        entity_refs: Vec<Id>,
        state_refs: Vec<Id>,
    },
    FootprintDeclared {
        operation_ref: Id,
        reads: Vec<Id>,
        writes: Vec<Id>,
    },
    PerformerRecorded {
        operation_ref: Id,
        performer_refs: Vec<Id>,
    },
    AuthorizationChecked {
        operation_ref: Id,
        actor_ref: Option<Id>,
        decision: AuthorizationDecision,
    },
    PreconditionEvaluated {
        operation_ref: Id,
        index: u32,
        result: bool,
    },
    RuleEvaluated {
        rule_ref: Id,
        result: bool,
    },
    RuleActionNotExecuted {
        rule_ref: Id,
    },
    DecisionTableEvaluated {
        decision_table_ref: Id,
        /// The zero-based selected row; None when the default output was used.
        row: Option<u32>,
    },
    CalculationEvaluated {
        calculation_ref: Id,
        value: RuntimeValue,
    },
    OutcomeSelected {
        operation_ref: Id,
        outcome_ref: Id,
    },
    TransitionTriggerRecorded {
        transition_ref: Id,
        trigger_ref: Option<Id>,
    },
    GuardEvaluated {
        transition_ref: Id,
        result: bool,
    },
    TransitionApplied {
        transition_ref: Id,
        from_state: Id,
        to_state: Id,
    },
    TransitionEffectsRecorded {
        transition_ref: Id,
        effect_refs: Vec<Id>,
    },
    EventOccurred {
        event_ref: Id,
    },
    PostconditionEvaluated {
        operation_ref: Id,
        index: u32,
        result: bool,
    },
    InvariantEvaluated {
        invariant_ref: Id,
        result: bool,
    },
    EventEmitted {
        event_ref: Id,
    },
    ExecutionCompleted,
}

/// The execution semantics of one run; S4.3 owns comparison with the Scenario expectations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ExecutionResult {
    Executed {
        state: RuntimeState,
        outcome_refs: Vec<Id>,
        event_refs: Vec<Id>,
        rule_results: BTreeMap<Id, bool>,
        trace: Vec<ExecutionTraceStep>,
    },
    Failed {
        failure: ExecutionFailure,
        trace: Vec<ExecutionTraceStep>,
    },
    Undecidable {
        missing: Vec<MissingSemantic>,
        trace: Vec<ExecutionTraceStep>,
    },
}

/// One run: the Scenario, its specification hash and the execution result. Not a ScenarioRun.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionRun {
    pub scenario_ref: Id,
    pub specification_hash: Hash,
    pub result: ExecutionResult,
}

// ============================================================================ errors

/// Which supplemental input an input error refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionInputKind {
    DecisionTable,
    CalculationScopeOverride,
    ExpressionScope,
    RuntimeSemanticValue,
}

/// The model, its inputs, the runtime or the Scenario is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SimulationError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("duplicate {input:?} input for {owner_ref}")]
    DuplicateInput {
        input: ExecutionInputKind,
        owner_ref: Id,
    },
    #[error("stale or invalid {input:?} input for {owner_ref}: {reason}")]
    InvalidInput {
        input: ExecutionInputKind,
        owner_ref: Id,
        reason: String,
    },
    #[error("invalid decision-table spec {decision_table_ref}: {reason}")]
    InvalidDecisionTableSpec {
        decision_table_ref: Id,
        reason: String,
    },
    #[error("calculation {calculation_ref} does not qualify: {reason}")]
    InvalidCalculation { calculation_ref: Id, reason: String },
    #[error("accepted calculations form a dependency cycle: {calculation_refs:?}")]
    CalculationCycle { calculation_refs: Vec<Id> },
    #[error("invalid expression {site:?} of {owner_ref}: {reason}")]
    InvalidExpression {
        owner_ref: Id,
        site: ExpressionSite,
        reason: String,
    },
    #[error("{scenario_ref} is not a Scenario")]
    NotAScenario { scenario_ref: Id },
    #[error("{scenario_ref} is not Accepted")]
    ScenarioNotAccepted { scenario_ref: Id },
    #[error("{scenario_ref} is not Confirmed: {confirmation_status:?}")]
    ScenarioNotConfirmed {
        scenario_ref: Id,
        confirmation_status: Option<String>,
    },
    #[error("invalid scenario {scenario_ref}: {reason}")]
    InvalidScenario { scenario_ref: Id, reason: String },
    #[error("runtime value {owner_ref}/{key} is not an allowed runtime environment value")]
    DisallowedRuntimeValue { owner_ref: Id, key: String },
    #[error("runtime actor {actor_ref} is not an Accepted Principal or Actor")]
    InvalidRuntimeActor { actor_ref: Id },
    #[error("value of {owner_ref} does not match its type: {reason}")]
    ValueTypeMismatch { owner_ref: Id, reason: String },
    #[error("invalid example of calculation {calculation_ref}: {reason}")]
    InvalidCalculationExample { calculation_ref: Id, reason: String },
    #[error("calculation {calculation_ref} has several examples with key {key}")]
    AmbiguousCalculationExample { calculation_ref: Id, key: String },
    #[error("{owner_ref}/{key} contradicts the accepted semantics")]
    SemanticContradiction { owner_ref: Id, key: String },
    #[error("UNIQUE decision table {decision_table_ref} matches rows {rows:?}")]
    AmbiguousDecisionRow {
        decision_table_ref: Id,
        rows: Vec<u32>,
    },
    #[error("decision input {index} of {decision_table_ref}: {reason}")]
    DecisionInputMismatch {
        decision_table_ref: Id,
        index: u32,
        reason: String,
    },
    #[error("transitions {transition_refs:?} leave {from_state} at once")]
    ConflictingTransitions {
        from_state: Id,
        transition_refs: Vec<Id>,
    },
    #[error("result of {owner_ref} is not a supported runtime value")]
    UnsupportedResultValue { owner_ref: Id },
    #[error("evaluation of {owner_ref} contradicts its validated model: {reason}")]
    Evaluation { owner_ref: Id, reason: String },
}

// ============================================================================ model

/// A validated expression scope: its bindings by root symbol and by (owner, field) and the
/// TypeEnv of the S2.6 adapter.
#[derive(Debug, Clone)]
pub(crate) struct ExpressionScope {
    pub(crate) roots: BTreeMap<String, CalculationScopeBinding>,
    pub(crate) fields: BTreeMap<(Id, String), CalculationScopeBinding>,
    pub(crate) env: TypeEnv,
}

/// One validated expression of a scoped owner.
#[derive(Debug, Clone)]
pub(crate) enum CompiledExpression {
    Executable(Ast),
    /// A pre/postcondition that is not PlumbExpr: Undecidable at run time.
    Unparseable,
}

/// A qualified Accepted Calculation.
#[derive(Debug, Clone)]
pub(crate) struct QualifiedCalculation {
    pub(crate) prepared: PreparedCalculation,
    pub(crate) used_bindings: Vec<CalculationScopeBinding>,
    pub(crate) dependencies: Vec<Id>,
}

/// The execution material of one Accepted Calculation.
#[derive(Debug, Clone)]
pub(crate) enum CalculationEntry {
    Qualified(Box<QualifiedCalculation>),
    /// No S2.6 origin and no explicit override: the scope is unavailable, never guessed.
    ScopeUnavailable,
}

/// A validated, immutable execution model over one graph baseline.
pub struct ExecutionModel<'g> {
    pub(crate) graph: &'g Graph,
    project_id: Id,
    semantic_hash: Hash,
    pub(crate) decision_tables: BTreeMap<Id, DecisionTableSpec>,
    pub(crate) calculations: BTreeMap<Id, CalculationEntry>,
    pub(crate) scopes: BTreeMap<Id, ExpressionScope>,
    pub(crate) expressions: BTreeMap<(Id, ExpressionSite), CompiledExpression>,
}

impl ExecutionModel<'_> {
    /// The graph's project.
    pub fn project_id(&self) -> &Id {
        &self.project_id
    }

    /// The semantic hash of the graph baseline the model was built from.
    pub fn semantic_hash(&self) -> &Hash {
        &self.semantic_hash
    }
}

pub(crate) fn accepted<'g>(graph: &'g Graph, id: &Id, node_type: NodeType) -> Option<&'g Node> {
    graph
        .node(id)
        .filter(|n| n.status == ElementStatus::Accepted && n.payload.node_type() == node_type)
}

fn input_error(
    input: ExecutionInputKind,
    owner_ref: &Id,
    reason: impl Into<String>,
) -> SimulationError {
    SimulationError::InvalidInput {
        input,
        owner_ref: owner_ref.clone(),
        reason: reason.into(),
    }
}

/// Sorts by ref and rejects duplicate refs.
fn canonical_by<T>(
    mut items: Vec<T>,
    input: ExecutionInputKind,
    key: impl Fn(&T) -> &Id,
) -> Result<Vec<T>, SimulationError> {
    items.sort_by(|a, b| key(a).cmp(key(b)));
    for pair in items.windows(2) {
        if key(&pair[0]) == key(&pair[1]) {
            return Err(SimulationError::DuplicateInput {
                input,
                owner_ref: key(&pair[0]).clone(),
            });
        }
    }
    Ok(items)
}

fn decision_tables(
    graph: &Graph,
    inputs: Vec<ScenarioDecisionTableInput>,
) -> Result<BTreeMap<Id, DecisionTableSpec>, SimulationError> {
    let kind = ExecutionInputKind::DecisionTable;
    let mut out = BTreeMap::new();
    for input in canonical_by(inputs, kind, |i| &i.decision_table_ref)? {
        let table_ref = &input.decision_table_ref;
        let Some(NodePayload::DecisionTable(table)) =
            accepted(graph, table_ref, NodeType::DecisionTable).map(|n| &n.payload)
        else {
            return Err(input_error(
                kind,
                table_ref,
                "not an Accepted DecisionTable",
            ));
        };
        if input.spec.hit_policy != table.hit_policy {
            return Err(input_error(
                kind,
                table_ref,
                "spec hit_policy differs from the DecisionTable hit_policy",
            ));
        }
        let bad_spec = |reason: String| SimulationError::InvalidDecisionTableSpec {
            decision_table_ref: table_ref.clone(),
            reason,
        };
        if HitPolicy::parse(&input.spec.hit_policy).is_none() {
            return Err(bad_spec(
                "hit policy is neither UNIQUE nor FIRST".to_owned(),
            ));
        }
        let analysis = analyze_decision_table(&input.spec);
        if !analysis.structure.is_empty() {
            return Err(bad_spec(format!("{:?}", analysis.structure)));
        }
        if u32::try_from(input.spec.inputs.len().max(input.spec.outputs.len())).is_err()
            || u32::try_from(input.spec.rows.len()).is_err()
        {
            return Err(bad_spec("too many columns or rows".to_owned()));
        }
        out.insert(input.decision_table_ref, input.spec);
    }
    Ok(out)
}

fn calculations(
    graph: &Graph,
    overrides: Vec<ExecutionCalculationScopeOverride>,
) -> Result<BTreeMap<Id, CalculationEntry>, SimulationError> {
    let kind = ExecutionInputKind::CalculationScopeOverride;
    let mut by_ref = BTreeMap::new();
    for o in canonical_by(overrides, kind, |o| &o.calculation_ref)? {
        if accepted(graph, &o.calculation_ref, NodeType::Calculation).is_none() {
            return Err(input_error(
                kind,
                &o.calculation_ref,
                "not an Accepted Calculation",
            ));
        }
        by_ref.insert(o.calculation_ref, o.scope);
    }
    let analysis = analyze_calculation_cycles(graph, &by_ref).map_err(|e| {
        SimulationError::InvalidCalculation {
            calculation_ref: graph.project_id().clone(),
            reason: e.to_string(),
        }
    })?;
    if let Some(cycle) = analysis.cycles.first() {
        return Err(SimulationError::CalculationCycle {
            calculation_refs: cycle.clone(),
        });
    }
    let mut out = BTreeMap::new();
    for q in analysis.qualifications {
        let issues = &q.qualification.issues;
        let entry = if issues
            .iter()
            .any(|i| matches!(i, CalculationIssue::ScopeUnavailable { .. }))
        {
            CalculationEntry::ScopeUnavailable
        } else {
            match (&q.qualification.prepared, issues.is_empty()) {
                (Some(prepared), true) => {
                    CalculationEntry::Qualified(Box::new(QualifiedCalculation {
                        prepared: prepared.clone(),
                        used_bindings: q.qualification.used_bindings.clone(),
                        dependencies: q.qualification.dependencies.clone(),
                    }))
                }
                _ => {
                    return Err(SimulationError::InvalidCalculation {
                        calculation_ref: q.calculation_ref.clone(),
                        reason: format!("{issues:?}"),
                    })
                }
            }
        };
        out.insert(q.calculation_ref.clone(), entry);
    }
    Ok(out)
}

/// The executable expressions of an owner, by site.
fn owner_expressions(node: &Node) -> Option<Vec<(ExpressionSite, &str)>> {
    let index = |i: usize| u32::try_from(i).unwrap_or(u32::MAX);
    Some(match &node.payload {
        NodePayload::Operation(op) => {
            let pre = op
                .preconditions
                .iter()
                .flatten()
                .enumerate()
                .map(|(i, e)| (ExpressionSite::Precondition { index: index(i) }, e.as_str()));
            let post = op
                .postconditions
                .iter()
                .flatten()
                .enumerate()
                .map(|(i, e)| {
                    (
                        ExpressionSite::Postcondition { index: index(i) },
                        e.as_str(),
                    )
                });
            pre.chain(post).collect()
        }
        NodePayload::Rule(rule) => rule
            .condition_expr
            .iter()
            .map(|e| (ExpressionSite::RuleCondition, e.as_str()))
            .collect(),
        NodePayload::Transition(t) => t
            .guard_expr
            .iter()
            .map(|e| (ExpressionSite::TransitionGuard, e.as_str()))
            .collect(),
        NodePayload::Invariant(i) => vec![(ExpressionSite::Invariant, i.expression.as_str())],
        _ => return None,
    })
}

type ScopedExpressions = (
    BTreeMap<Id, ExpressionScope>,
    BTreeMap<(Id, ExpressionSite), CompiledExpression>,
);

fn expression_scopes(
    graph: &Graph,
    inputs: Vec<ExecutionExpressionScopeInput>,
) -> Result<ScopedExpressions, SimulationError> {
    let kind = ExecutionInputKind::ExpressionScope;
    let mut scopes = BTreeMap::new();
    let mut expressions = BTreeMap::new();
    for input in canonical_by(inputs, kind, |i| &i.owner_ref)? {
        let owner_ref = &input.owner_ref;
        let node = graph
            .node(owner_ref)
            .filter(|n| n.status == ElementStatus::Accepted)
            .ok_or_else(|| input_error(kind, owner_ref, "not an Accepted node"))?;
        let sites = owner_expressions(node).ok_or_else(|| {
            input_error(
                kind,
                owner_ref,
                "not an Operation, Rule, Transition or Invariant",
            )
        })?;
        let validated = validate_calculation_bindings(graph, &input.bindings)
            .map_err(|issue| input_error(kind, owner_ref, format!("{issue:?}")))?;
        let scope = ExpressionScope {
            roots: validated.roots,
            fields: validated.fields,
            env: validated.env,
        };
        for (site, text) in sites {
            let invalid = |reason: String| SimulationError::InvalidExpression {
                owner_ref: owner_ref.clone(),
                site,
                reason,
            };
            let compiled = match parse(text) {
                Ok(ast) => {
                    match typecheck(&ast, &scope.env) {
                        Ok(Ty::Bool) => {}
                        Ok(ty) => return Err(invalid(format!("type {ty:?} is not Bool"))),
                        Err(e) => return Err(invalid(e.to_string())),
                    }
                    CompiledExpression::Executable(ast)
                }
                Err(_) if site.unsupported_key().is_some() => CompiledExpression::Unparseable,
                Err(e) => return Err(invalid(e.to_string())),
            };
            expressions.insert((owner_ref.clone(), site), compiled);
        }
        scopes.insert(input.owner_ref, scope);
    }
    Ok((scopes, expressions))
}

/// Validates the graph baseline and the explicit supplemental inputs into an execution model.
pub fn build_execution_model(
    graph: &Graph,
    inputs: ExecutionInputs,
) -> Result<ExecutionModel<'_>, SimulationError> {
    let decision_tables = decision_tables(graph, inputs.decision_tables)?;
    let calculations = calculations(graph, inputs.calculation_scope_overrides)?;
    let (scopes, expressions) = expression_scopes(graph, inputs.expression_scopes)?;
    Ok(ExecutionModel {
        graph,
        project_id: graph.project_id().clone(),
        semantic_hash: semantic_hash(graph)?,
        decision_tables,
        calculations,
        scopes,
        expressions,
    })
}
