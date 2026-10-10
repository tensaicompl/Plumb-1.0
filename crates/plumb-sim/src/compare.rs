//! Then comparison of a completed S4.2 execution (plan S4.3, Hotfix 052).
//!
//! [`compare_run`] re-admits the current Accepted, Confirmed Scenario, rejects stale execution
//! evidence, maps execution Failed and Undecidable to final verdicts and otherwise compares the
//! typed Then expectations with the actual execution semantics. Then SemanticRefs resolve only
//! through the unchanged S4.2 materializer; nothing is executed again, persisted or mutated.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{Hash, Id};
use plumb_expr::{map_pilot_value_type, Value as ExprValue};
use plumb_functional::decision_table::DecisionType;
use plumb_functional::{
    ScenarioGiven, ScenarioLiteral, ScenarioSemantics, ScenarioThen, ScenarioValue, ScenarioWhen,
};
use plumb_psg::{ElementStatus, NodePayload, NodeType, RelationKind, ScenarioRun};
use serde::Serialize;
use thiserror::Error;

use crate::materialize::{admit, materialize, runtime_values, to_expr_value};
use crate::model::{
    accepted, ExecutionFailure, ExecutionModel, ExecutionResult, ExecutionRun, ExecutionRuntime,
    ExecutionTraceStep, RuntimeState, SimulationError,
};
use crate::trace::{
    scenario_run, ComparisonTraceStep, ScenarioTraceArtifact, ScenarioTraceArtifactBody,
    ScenarioTraceStep, ScenarioVerdict, SCENARIO_TRACE_VERSION,
};

/// Why a comparison could not be made. Wraps the S4.2 [`SimulationError`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ComparisonError {
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error("execution of {scenario_ref} does not match the current Scenario specification")]
    StaleExecutionRun { scenario_ref: Id },
    #[error("expected value of {semantic_ref} contradicts its target: {reason}")]
    InvalidExpectedValue { semantic_ref: Id, reason: String },
    #[error("decision outputs of {scenario_ref} have no unique source: {decision_table_refs:?}")]
    AmbiguousComparisonSource {
        scenario_ref: Id,
        decision_table_refs: Vec<Id>,
    },
    #[error("trace serialization: {reason}")]
    TraceSerialization { reason: String },
}

/// Where a value diff sits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "location", rename_all = "snake_case")]
pub enum DiffLocation {
    AttributeValue,
    DecisionOutput { decision_table_ref: Id, column: u32 },
    CalculationResult,
}

/// One structured, typed comparison difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "diff", rename_all = "snake_case")]
pub enum ScenarioDiff {
    ExecutionFailure {
        semantic_ref: Id,
        failure: ExecutionFailure,
    },
    ValueMismatch {
        semantic_ref: Id,
        location: DiffLocation,
        expected: ScenarioLiteral,
        actual: ScenarioLiteral,
    },
    MissingActual {
        semantic_ref: Id,
        location: DiffLocation,
        expected: ScenarioLiteral,
    },
    MissingExpectedState {
        state_ref: Id,
    },
    RuleNotSatisfied {
        rule_ref: Id,
        actual: Option<bool>,
    },
    OutcomeSetMismatch {
        anchor_ref: Id,
        expected: Vec<Id>,
        actual: Vec<Id>,
        missing: Vec<Id>,
        unexpected: Vec<Id>,
    },
    EventSetMismatch {
        anchor_ref: Id,
        expected: Vec<Id>,
        actual: Vec<Id>,
        missing: Vec<Id>,
        unexpected: Vec<Id>,
    },
}

/// The comparison of one execution with its current Scenario.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioComparison {
    pub scenario_ref: Id,
    pub specification_hash: Hash,
    pub result: ScenarioVerdict,
    pub diffs: Vec<ScenarioDiff>,
    pub trace: Vec<ScenarioTraceStep>,
    pub scenario_run: ScenarioRun,
    pub trace_artifact: ScenarioTraceArtifact,
}

/// The exact owner of an execution failure.
fn failure_owner(failure: &ExecutionFailure) -> &Id {
    match failure {
        ExecutionFailure::AuthorizationDenied { operation_ref, .. }
        | ExecutionFailure::PreconditionRejected { operation_ref, .. }
        | ExecutionFailure::PostconditionViolation { operation_ref, .. } => operation_ref,
        ExecutionFailure::TransitionGuardRejected { transition_ref }
        | ExecutionFailure::InvalidSourceState { transition_ref, .. } => transition_ref,
        ExecutionFailure::InvariantViolation { invariant_ref } => invariant_ref,
        ExecutionFailure::NoDecisionRow { decision_table_ref } => decision_table_ref,
        ExecutionFailure::EvaluationFailed { owner_ref, .. } => owner_ref,
    }
}

fn anchor(when: &ScenarioWhen) -> &Id {
    match when {
        ScenarioWhen::Operation(id)
        | ScenarioWhen::Event(id)
        | ScenarioWhen::Transition(id)
        | ScenarioWhen::Rule(id)
        | ScenarioWhen::Calculation(id)
        | ScenarioWhen::DecisionTable(id) => id,
    }
}

fn decimal(text: &str) -> Option<ExprValue> {
    Some(ExprValue::Decimal(text.parse().ok()?))
}

/// Typed equality: decimals numerically with exactly equal units, everything else exactly.
pub(crate) fn literals_equal(expected: &ScenarioLiteral, actual: &ScenarioLiteral) -> bool {
    match (expected, actual) {
        (
            ScenarioLiteral::Decimal { value: x, unit: u },
            ScenarioLiteral::Decimal { value: y, unit: v },
        ) => u == v && decimal(x).is_some() && decimal(x) == decimal(y),
        _ => expected == actual,
    }
}

fn column_accepts(ty: &DecisionType, literal: &ScenarioLiteral) -> bool {
    if literal.validate().is_err() {
        return false;
    }
    match (ty, literal) {
        (DecisionType::Bool, ScenarioLiteral::Bool { .. })
        | (DecisionType::Int, ScenarioLiteral::Int { .. }) => true,
        (DecisionType::Enum { members }, ScenarioLiteral::Enum { value }) => {
            members.contains(value)
        }
        (DecisionType::Decimal { scale }, ScenarioLiteral::Decimal { value, unit: None }) => {
            value.split_once('.').map_or(0, |(_, f)| f.len())
                <= usize::try_from(*scale).unwrap_or(usize::MAX)
        }
        _ => false,
    }
}

fn invalid(semantic_ref: &Id, reason: impl Into<String>) -> ComparisonError {
    ComparisonError::InvalidExpectedValue {
        semantic_ref: semantic_ref.clone(),
        reason: reason.into(),
    }
}

/// The actual execution semantics of an Executed run.
struct Actual<'a> {
    state: &'a RuntimeState,
    outcome_refs: &'a [Id],
    event_refs: &'a [Id],
    rule_results: &'a BTreeMap<Id, bool>,
}

/// The verdict, diffs (canonical order) and comparison steps (Then order) of an Executed run.
type Compared = (ScenarioVerdict, Vec<ScenarioDiff>, Vec<ComparisonTraceStep>);

/// Checks an expected calculation result against the Calculation's declared type and unit.
fn check_calculation_type(
    model: &ExecutionModel<'_>,
    calculation_ref: &Id,
    literal: &ScenarioLiteral,
) -> Result<(), ComparisonError> {
    let Some(NodePayload::Calculation(c)) =
        accepted(model.graph, calculation_ref, NodeType::Calculation).map(|n| &n.payload)
    else {
        return Err(invalid(calculation_ref, "not an Accepted Calculation"));
    };
    let ty = map_pilot_value_type(&c.result_type, c.unit.as_deref(), Some(calculation_ref))
        .map_err(|e| invalid(calculation_ref, e.to_string()))?;
    let no_members = |_: &Id, _: &str| false;
    to_expr_value(literal, &ty, &no_members)
        .map(|_| ())
        .ok_or_else(|| {
            invalid(
                calculation_ref,
                "expected result does not match the result type",
            )
        })
}

/// The DecisionTable whose outputs a Then may assert.
fn decision_source(
    model: &ExecutionModel<'_>,
    scenario_ref: &Id,
    when: &ScenarioWhen,
) -> Result<Id, ComparisonError> {
    let graph = model.graph;
    match when {
        ScenarioWhen::DecisionTable(table_ref) => Ok(table_ref.clone()),
        ScenarioWhen::Operation(operation_ref) => {
            let tables: BTreeSet<Id> = graph
                .outgoing_edge_ids(operation_ref)
                .iter()
                .filter_map(|e| graph.edge(e))
                .filter(|e| {
                    e.kind == RelationKind::GovernedBy && e.status == ElementStatus::Accepted
                })
                .filter(|e| accepted(graph, &e.to, NodeType::DecisionTable).is_some())
                .map(|e| e.to.clone())
                .collect();
            let tables: Vec<Id> = tables.into_iter().collect();
            match tables.as_slice() {
                [table_ref] => Ok(table_ref.clone()),
                [] => Err(invalid(
                    operation_ref,
                    "the Operation has no governing DecisionTable",
                )),
                _ => Err(ComparisonError::AmbiguousComparisonSource {
                    scenario_ref: scenario_ref.clone(),
                    decision_table_refs: tables,
                }),
            }
        }
        other => Err(invalid(
            anchor(other),
            "this anchor has no decision outputs",
        )),
    }
}

/// Sorted expected, actual, missing and unexpected members of a set mismatch.
type SetMismatch = (Vec<Id>, Vec<Id>, Vec<Id>, Vec<Id>);

fn set_diff(expected: &[Id], actual: &[Id]) -> Option<SetMismatch> {
    let e: BTreeSet<&Id> = expected.iter().collect();
    let a: BTreeSet<&Id> = actual.iter().collect();
    if e == a {
        return None;
    }
    let sorted = |s: &BTreeSet<&Id>| s.iter().map(|i| (*i).clone()).collect::<Vec<Id>>();
    Some((
        sorted(&e),
        sorted(&a),
        e.difference(&a).map(|i| (*i).clone()).collect(),
        a.difference(&e).map(|i| (*i).clone()).collect(),
    ))
}

fn compare_then(
    model: &ExecutionModel<'_>,
    scenario_ref: &Id,
    semantics: &ScenarioSemantics,
    runtime: &ExecutionRuntime<'_>,
    actual: &Actual<'_>,
) -> Result<Compared, ComparisonError> {
    let then = &semantics.then;
    let mut steps = Vec::new();

    // Resolve Then SemanticRefs through the unchanged S4.2 materializer (a Given-shaped probe):
    // attribute expectations in attribute positions, calculation-result references elsewhere.
    let mut probe_given: ScenarioGiven<ScenarioValue> = ScenarioGiven {
        attribute_values: then.attribute_values.clone(),
        ..ScenarioGiven::default()
    };
    let calculation_positions: Vec<(&Id, &ScenarioValue)> =
        then.calculation_results.iter().collect();
    for (index, (_, value)) in calculation_positions.iter().enumerate() {
        if matches!(value, ScenarioValue::SemanticRef(_)) {
            let index =
                u32::try_from(index).map_err(|_| invalid(scenario_ref, "too many expectations"))?;
            probe_given.decision_inputs.insert(index, (*value).clone());
        }
    }
    let probe = ScenarioSemantics {
        given: probe_given,
        when: semantics.when.clone(),
        then: ScenarioThen::default(),
    };
    let values = runtime_values(model, runtime)?;
    let resolved = materialize(model, scenario_ref, &probe, &values)?;
    let mut seen = BTreeSet::new();
    for step in &resolved.trace {
        if let ExecutionTraceStep::SemanticResolved {
            owner_ref,
            key,
            source,
        } = step
        {
            if seen.insert((owner_ref.clone(), key.clone())) {
                steps.push(ComparisonTraceStep::ExpectedValueResolved {
                    owner_ref: owner_ref.clone(),
                    key: key.clone(),
                    source: *source,
                });
            }
        }
    }
    if !resolved.missing.is_empty() {
        steps.push(ComparisonTraceStep::ExpectedSemanticMissing {
            missing: resolved.missing.into_iter().collect(),
        });
        return Ok((ScenarioVerdict::Undecidable, Vec::new(), steps));
    }

    let expected_attributes = &resolved.state.attributes;
    let mut expected_calculations: BTreeMap<Id, ScenarioLiteral> = BTreeMap::new();
    for (index, (calculation_ref, value)) in calculation_positions.iter().enumerate() {
        let literal = match value {
            ScenarioValue::Literal(literal) => literal.clone(),
            ScenarioValue::SemanticRef(_) => u32::try_from(index)
                .ok()
                .and_then(|i| resolved.decision_inputs.get(&i))
                .cloned()
                .ok_or_else(|| {
                    invalid(
                        calculation_ref,
                        "the expected result did not resolve to a value",
                    )
                })?,
        };
        check_calculation_type(model, calculation_ref, &literal)?;
        expected_calculations.insert((*calculation_ref).clone(), literal);
    }
    let mut expected_outputs: BTreeMap<u32, ScenarioLiteral> = BTreeMap::new();
    let mut output_table = None;
    if !then.decision_outputs.is_empty() {
        let table_ref = decision_source(model, scenario_ref, &semantics.when)?;
        let spec = model
            .decision_tables
            .get(&table_ref)
            .ok_or_else(|| invalid(&table_ref, "no typed spec for the DecisionTable"))?;
        for (column, value) in &then.decision_outputs {
            let ScenarioValue::Literal(literal) = value else {
                return Err(invalid(&table_ref, "decision outputs must be literals"));
            };
            let ty = spec
                .outputs
                .get(*column as usize)
                .map(|c| &c.ty)
                .ok_or_else(|| invalid(&table_ref, format!("unknown output column {column}")))?;
            if !column_accepts(ty, literal) {
                return Err(invalid(
                    &table_ref,
                    format!("output column {column} type mismatch"),
                ));
            }
            expected_outputs.insert(*column, literal.clone());
        }
        output_table = Some(table_ref);
    }

    let anchor_ref = anchor(&semantics.when).clone();
    let (mut attribute_diffs, mut state_diffs, mut rule_diffs) =
        (Vec::new(), Vec::new(), Vec::new());
    let (mut output_diffs, mut calculation_diffs, mut set_diffs) =
        (Vec::new(), Vec::new(), Vec::new());

    let value_diff = |semantic_ref: &Id,
                      location: DiffLocation,
                      expected: &ScenarioLiteral,
                      actual: Option<&ScenarioLiteral>|
     -> Option<ScenarioDiff> {
        match actual {
            None => Some(ScenarioDiff::MissingActual {
                semantic_ref: semantic_ref.clone(),
                location,
                expected: expected.clone(),
            }),
            Some(a) if !literals_equal(expected, a) => Some(ScenarioDiff::ValueMismatch {
                semantic_ref: semantic_ref.clone(),
                location,
                expected: expected.clone(),
                actual: a.clone(),
            }),
            Some(_) => None,
        }
    };

    // Then category order: attribute_values, states_active, outcomes, events, rules_satisfied,
    // decision_outputs, calculation_results.
    for (attribute_ref, expected) in expected_attributes {
        let diff = value_diff(
            attribute_ref,
            DiffLocation::AttributeValue,
            expected,
            actual.state.attributes.get(attribute_ref),
        );
        steps.push(ComparisonTraceStep::AttributeCompared {
            attribute_ref: attribute_ref.clone(),
            matched: diff.is_none(),
        });
        attribute_diffs.extend(diff);
    }
    let expected_states: BTreeSet<&Id> = then.states_active.iter().collect();
    for state_ref in expected_states {
        let matched = actual.state.active_states.contains(state_ref);
        steps.push(ComparisonTraceStep::StateCompared {
            state_ref: state_ref.clone(),
            matched,
        });
        if !matched {
            state_diffs.push(ScenarioDiff::MissingExpectedState {
                state_ref: state_ref.clone(),
            });
        }
    }
    if !then.outcomes.is_empty() {
        let diff = set_diff(&then.outcomes, actual.outcome_refs);
        steps.push(ComparisonTraceStep::OutcomesCompared {
            anchor_ref: anchor_ref.clone(),
            matched: diff.is_none(),
        });
        if let Some((expected, actual, missing, unexpected)) = diff {
            set_diffs.push(ScenarioDiff::OutcomeSetMismatch {
                anchor_ref: anchor_ref.clone(),
                expected,
                actual,
                missing,
                unexpected,
            });
        }
    }
    if !then.events.is_empty() {
        let diff = set_diff(&then.events, actual.event_refs);
        steps.push(ComparisonTraceStep::EventsCompared {
            anchor_ref: anchor_ref.clone(),
            matched: diff.is_none(),
        });
        if let Some((expected, actual, missing, unexpected)) = diff {
            set_diffs.push(ScenarioDiff::EventSetMismatch {
                anchor_ref: anchor_ref.clone(),
                expected,
                actual,
                missing,
                unexpected,
            });
        }
    }
    let expected_rules: BTreeSet<&Id> = then.rules_satisfied.iter().collect();
    for rule_ref in expected_rules {
        let result = actual.rule_results.get(rule_ref).copied();
        let matched = result == Some(true);
        steps.push(ComparisonTraceStep::RuleCompared {
            rule_ref: rule_ref.clone(),
            matched,
        });
        if !matched {
            rule_diffs.push(ScenarioDiff::RuleNotSatisfied {
                rule_ref: rule_ref.clone(),
                actual: result,
            });
        }
    }
    if let Some(table_ref) = &output_table {
        let outputs = actual.state.decision_outputs.get(table_ref);
        for (column, expected) in &expected_outputs {
            let diff = value_diff(
                table_ref,
                DiffLocation::DecisionOutput {
                    decision_table_ref: table_ref.clone(),
                    column: *column,
                },
                expected,
                outputs.and_then(|o| o.get(column)),
            );
            steps.push(ComparisonTraceStep::DecisionOutputCompared {
                decision_table_ref: table_ref.clone(),
                column: *column,
                matched: diff.is_none(),
            });
            output_diffs.extend(diff);
        }
    }
    for (calculation_ref, expected) in &expected_calculations {
        let diff = value_diff(
            calculation_ref,
            DiffLocation::CalculationResult,
            expected,
            actual.state.calculation_results.get(calculation_ref),
        );
        steps.push(ComparisonTraceStep::CalculationCompared {
            calculation_ref: calculation_ref.clone(),
            matched: diff.is_none(),
        });
        calculation_diffs.extend(diff);
    }

    // Canonical diff order: attributes, states, rules, decision outputs, calculations,
    // outcome set, event set.
    let mut diffs = attribute_diffs;
    diffs.extend(state_diffs);
    diffs.extend(rule_diffs);
    diffs.extend(output_diffs);
    diffs.extend(calculation_diffs);
    diffs.extend(set_diffs);
    let verdict = if diffs.is_empty() {
        ScenarioVerdict::Pass
    } else {
        ScenarioVerdict::Fail
    };
    Ok((verdict, diffs, steps))
}

/// Compares a completed execution with its current Scenario and builds the ScenarioRun and
/// scenario-trace artifact material. The runtime only serves Then SemanticRef resolution.
pub fn compare_run(
    model: &ExecutionModel<'_>,
    execution: &ExecutionRun,
    runtime: &ExecutionRuntime<'_>,
) -> Result<ScenarioComparison, ComparisonError> {
    let scenario_ref = &execution.scenario_ref;
    let admitted = admit(model, scenario_ref)?;
    if admitted.specification_hash != execution.specification_hash {
        return Err(ComparisonError::StaleExecutionRun {
            scenario_ref: scenario_ref.clone(),
        });
    }
    let (execution_trace, (verdict, diffs, steps)) = match &execution.result {
        ExecutionResult::Failed { failure, trace } => (
            trace,
            (
                ScenarioVerdict::Fail,
                vec![ScenarioDiff::ExecutionFailure {
                    semantic_ref: failure_owner(failure).clone(),
                    failure: failure.clone(),
                }],
                Vec::new(),
            ),
        ),
        ExecutionResult::Undecidable { trace, .. } => (
            trace,
            (ScenarioVerdict::Undecidable, Vec::new(), Vec::new()),
        ),
        ExecutionResult::Executed {
            state,
            outcome_refs,
            event_refs,
            rule_results,
            trace,
        } => (
            trace,
            compare_then(
                model,
                scenario_ref,
                &admitted.semantics,
                runtime,
                &Actual {
                    state,
                    outcome_refs,
                    event_refs,
                    rule_results,
                },
            )?,
        ),
    };
    let mut trace: Vec<ScenarioTraceStep> = execution_trace
        .iter()
        .cloned()
        .map(ScenarioTraceStep::Execution)
        .collect();
    trace.push(ScenarioTraceStep::Comparison(
        ComparisonTraceStep::ComparisonStarted {
            scenario_ref: scenario_ref.clone(),
        },
    ));
    trace.extend(steps.into_iter().map(ScenarioTraceStep::Comparison));
    trace.push(ScenarioTraceStep::Comparison(
        ComparisonTraceStep::ComparisonCompleted {
            result: verdict,
            diff_count: u32::try_from(diffs.len()).unwrap_or(u32::MAX),
        },
    ));
    let body = ScenarioTraceArtifactBody {
        version: SCENARIO_TRACE_VERSION,
        model_semantic_hash: model.semantic_hash().clone(),
        scenario_ref: scenario_ref.clone(),
        specification_hash: execution.specification_hash.clone(),
        result: verdict.run_result(),
        trace: trace.clone(),
        diffs: diffs.clone(),
    };
    let scenario_run = scenario_run(&body)?;
    let trace_artifact = ScenarioTraceArtifact::from_body(body)?;
    Ok(ScenarioComparison {
        scenario_ref: scenario_ref.clone(),
        specification_hash: execution.specification_hash.clone(),
        result: verdict,
        diffs,
        trace,
        scenario_run,
        trace_artifact,
    })
}
