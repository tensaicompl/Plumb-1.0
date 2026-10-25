//! The pure functional interpreter run (plan S4.2, Hotfix 051).
//!
//! `run` admits one Accepted, Confirmed Scenario, materializes its Given state and executes its
//! When anchor: typed decision tables, calculations through the S2.6 evaluator, explicitly
//! scoped rule conditions, guards, invariants and pre/postconditions, role-level authorization
//! over the accepted role facts, state transitions, deterministic outcomes and produced events.
//! It never compares Scenario.then, never invents attribute writes, never mutates the graph and
//! uses no I/O, clock or provider other than the injected calendar and predicate.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use plumb_expr::{eval, EvalContext, EvalError, Value as ExprValue};
use plumb_functional::calculation::{evaluate_calculation, CalculationEvalError};
use plumb_functional::decision_table::{
    DecisionInputCell, DecisionOutputCell, DecisionTableSpec, DecisionType, HitPolicy,
    NumericInterval,
};
use plumb_functional::{
    authorization_facts, effective_security_roles, BusinessFailureKind, ScenarioLiteral,
    ScenarioWhen,
};
use plumb_psg::{ElementStatus, Graph, NodePayload, NodeType, OutcomeKind, RelationKind};

use crate::materialize::{
    admit, bind_values, expression_bindings, from_expr_value, materialize, runtime_values,
};
use crate::model::{
    accepted, missing_keys, AuthorizationDecision, CalculationEntry, CompiledExpression,
    EvaluationFailureKind, ExecutionFailure, ExecutionModel, ExecutionResult, ExecutionRun,
    ExecutionRuntime, ExecutionTraceStep, ExpressionSite, MissingSemantic, MissingSemanticKind,
    RuntimeState, RuntimeValue, SimulationError,
};

/// Why execution stopped before completing.
enum Stop {
    Error(SimulationError),
    Failed(ExecutionFailure),
    Undecidable,
}

impl From<SimulationError> for Stop {
    fn from(e: SimulationError) -> Stop {
        Stop::Error(e)
    }
}

type Step<T> = Result<T, Stop>;

fn index_u32(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

/// Accepted targets of Accepted `kind` edges from `from`, sorted and unique.
fn targets(graph: &Graph, from: &Id, kind: RelationKind, target: NodeType) -> Vec<Id> {
    let set: BTreeSet<Id> = graph
        .outgoing_edge_ids(from)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == kind && e.status == ElementStatus::Accepted)
        .filter(|e| accepted(graph, &e.to, target).is_some())
        .map(|e| e.to.clone())
        .collect();
    set.into_iter().collect()
}

/// Accepted sources of Accepted `kind` edges into `to`, sorted and unique.
fn sources(graph: &Graph, to: &Id, kind: RelationKind, source: NodeType) -> Vec<Id> {
    let set: BTreeSet<Id> = graph
        .incoming_edge_ids(to)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == kind && e.status == ElementStatus::Accepted)
        .filter(|e| accepted(graph, &e.from, source).is_some())
        .map(|e| e.from.clone())
        .collect();
    set.into_iter().collect()
}

fn failure_kind(e: &EvalError) -> Option<EvaluationFailureKind> {
    Some(match e {
        EvalError::DivisionByZero => EvaluationFailureKind::DivisionByZero,
        EvalError::Overflow => EvaluationFailureKind::Overflow,
        EvalError::InvalidPeriod | EvalError::InvalidPeriodLength { .. } => {
            EvaluationFailureKind::InvalidPeriod
        }
        EvalError::NonIntegralDateDuration => EvaluationFailureKind::NonIntegralDateDuration,
        EvalError::DateOutOfRange => EvaluationFailureKind::DateOutOfRange,
        EvalError::EmptyAggregate { .. } => EvaluationFailureKind::EmptyAggregate,
        EvalError::Calendar(_) => EvaluationFailureKind::CalendarUnavailable,
        EvalError::Predicate(_) => EvaluationFailureKind::PredicateUnavailable,
        EvalError::CalendarCoverageUnavailable { .. }
        | EvalError::Type(_)
        | EvalError::MissingValue { .. }
        | EvalError::ValueTypeMismatch { .. }
        | EvalError::UnitMismatch { .. } => return None,
    })
}

/// A data-dependent evaluator failure is Failed; a type contradiction is a model error.
fn eval_stop(owner_ref: &Id, e: &EvalError) -> Stop {
    match failure_kind(e) {
        Some(kind) => Stop::Failed(ExecutionFailure::EvaluationFailed {
            owner_ref: owner_ref.clone(),
            kind,
        }),
        None => Stop::Error(SimulationError::Evaluation {
            owner_ref: owner_ref.clone(),
            reason: e.to_string(),
        }),
    }
}

/// A canonical decimal string or integer as the exact decimal type of `like`.
fn parse_like<T: std::str::FromStr>(_like: &T, text: &str) -> Option<T> {
    text.parse().ok()
}

fn in_interval(interval: &NumericInterval, text: &str) -> Option<bool> {
    let Some(like) = interval.lower.as_ref().or(interval.upper.as_ref()) else {
        return Some(true);
    };
    let x = parse_like(&like.value, text)?;
    let lower_ok = interval.lower.as_ref().is_none_or(|b| {
        if b.inclusive {
            x >= b.value
        } else {
            x > b.value
        }
    });
    let upper_ok = interval.upper.as_ref().is_none_or(|b| {
        if b.inclusive {
            x <= b.value
        } else {
            x < b.value
        }
    });
    Some(lower_ok && upper_ok)
}

fn numeric_text(literal: &ScenarioLiteral) -> Option<String> {
    match literal {
        ScenarioLiteral::Int { value } => Some(value.to_string()),
        ScenarioLiteral::Decimal { value, unit: None } => Some(value.clone()),
        _ => None,
    }
}

fn cell_matches(cell: &DecisionInputCell, literal: &ScenarioLiteral) -> bool {
    match (cell, literal) {
        (DecisionInputCell::Any, _) => true,
        (DecisionInputCell::Bool { value }, ScenarioLiteral::Bool { value: v }) => value == v,
        (DecisionInputCell::EnumSet { members }, ScenarioLiteral::Enum { value }) => {
            members.contains(value)
        }
        (DecisionInputCell::Interval { interval }, literal) => numeric_text(literal)
            .and_then(|text| in_interval(interval, &text))
            .unwrap_or(false),
        _ => false,
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
            value
                .split_once('.')
                .map_or(0, |(_, fraction)| fraction.len())
                <= usize::try_from(*scale).unwrap_or(usize::MAX)
        }
        _ => false,
    }
}

fn output_literal(cell: &DecisionOutputCell) -> RuntimeValue {
    match cell {
        DecisionOutputCell::Bool { value } => ScenarioLiteral::Bool { value: *value },
        DecisionOutputCell::Enum { member } => ScenarioLiteral::Enum {
            value: member.clone(),
        },
        DecisionOutputCell::Int { value } => ScenarioLiteral::Int { value: *value },
        DecisionOutputCell::Decimal { value } => ScenarioLiteral::Decimal {
            value: value.to_string(),
            unit: None,
        },
    }
}

struct Executor<'m, 'g, 'r> {
    model: &'m ExecutionModel<'g>,
    runtime: &'r ExecutionRuntime<'r>,
    state: RuntimeState,
    decision_inputs: BTreeMap<u32, RuntimeValue>,
    trace: Vec<ExecutionTraceStep>,
    missing: BTreeSet<MissingSemantic>,
    /// Calculations already attempted in this run (memo; the value is in the state).
    attempted: BTreeSet<Id>,
    rule_results: BTreeMap<Id, bool>,
    outcome_refs: Vec<Id>,
    /// Elements whose Invariants apply: the anchor, footprint and applied transition subjects.
    invariant_targets: BTreeSet<Id>,
}

impl Executor<'_, '_, '_> {
    fn miss(&mut self, owner_ref: &Id, key: impl Into<String>, kind: MissingSemanticKind) {
        self.missing.insert(MissingSemantic {
            owner_ref: owner_ref.clone(),
            key: key.into(),
            kind,
        });
    }

    /// A data-dependent evaluator failure is Failed and a type contradiction is a model error;
    /// a date outside the calendar's declared coverage makes the run Undecidable (§33.11).
    fn eval_stop(&mut self, owner_ref: &Id, e: &EvalError) -> Stop {
        if let EvalError::CalendarCoverageUnavailable { calendar_ref, date } = e {
            self.miss(
                calendar_ref,
                missing_keys::calendar_coverage(date.year()),
                MissingSemanticKind::ModelCondition,
            );
            return Stop::Undecidable;
        }
        eval_stop(owner_ref, e)
    }

    /// Ends the current phase: any collected missing value makes the run Undecidable.
    fn checkpoint(&self) -> Step<()> {
        if self.missing.is_empty() {
            Ok(())
        } else {
            Err(Stop::Undecidable)
        }
    }

    // ------------------------------------------------------------------ calculations

    fn calculation(&mut self, calculation_ref: &Id) -> Step<()> {
        if !self.attempted.insert(calculation_ref.clone()) {
            return Ok(());
        }
        let model = self.model;
        let qualified = match model.calculations.get(calculation_ref) {
            Some(CalculationEntry::Qualified(q)) => q,
            Some(CalculationEntry::ScopeUnavailable) => {
                self.miss(
                    calculation_ref,
                    missing_keys::CALCULATION_SCOPE,
                    MissingSemanticKind::CalculationScope,
                );
                return Ok(());
            }
            None => {
                return Err(Stop::Error(SimulationError::InvalidCalculation {
                    calculation_ref: calculation_ref.clone(),
                    reason: "not an Accepted Calculation of the model".to_owned(),
                }))
            }
        };
        for dependency in &qualified.dependencies {
            self.calculation(dependency)?;
        }
        let Some(values) = bind_values(
            model.graph,
            &qualified.prepared.type_env,
            &qualified.used_bindings,
            &self.state,
            &mut self.missing,
        )?
        else {
            return Ok(());
        };
        let value = evaluate_calculation(
            &qualified.prepared,
            &values,
            self.runtime.calendars,
            self.runtime.predicates,
        )
        .map_err(|e| match e {
            CalculationEvalError::Eval(e) => self.eval_stop(calculation_ref, &e),
            CalculationEvalError::Rounding { .. } => {
                Stop::Failed(ExecutionFailure::EvaluationFailed {
                    owner_ref: calculation_ref.clone(),
                    kind: EvaluationFailureKind::Rounding,
                })
            }
            CalculationEvalError::ResultScaleExceeded { .. } => {
                Stop::Failed(ExecutionFailure::EvaluationFailed {
                    owner_ref: calculation_ref.clone(),
                    kind: EvaluationFailureKind::ResultScaleExceeded,
                })
            }
            CalculationEvalError::ReservedBindingConflict => {
                Stop::Error(SimulationError::Evaluation {
                    owner_ref: calculation_ref.clone(),
                    reason: e.to_string(),
                })
            }
        })?;
        let literal =
            from_expr_value(&value).ok_or_else(|| SimulationError::UnsupportedResultValue {
                owner_ref: calculation_ref.clone(),
            })?;
        self.state
            .calculation_results
            .insert(calculation_ref.clone(), literal.clone());
        self.trace.push(ExecutionTraceStep::CalculationEvaluated {
            calculation_ref: calculation_ref.clone(),
            value: literal,
        });
        Ok(())
    }

    // ------------------------------------------------------------------ expressions

    /// A Boolean expression of `owner_ref` under its explicit scope; `None` is missing.
    fn expression(&mut self, owner_ref: &Id, site: ExpressionSite) -> Step<Option<bool>> {
        let model = self.model;
        if !model.scopes.contains_key(owner_ref) {
            self.miss(
                owner_ref,
                missing_keys::EXPRESSION_SCOPE,
                MissingSemanticKind::ExpressionScope,
            );
            return Ok(None);
        }
        let Some(compiled) = model.expressions.get(&(owner_ref.clone(), site)) else {
            return Err(Stop::Error(SimulationError::InvalidExpression {
                owner_ref: owner_ref.clone(),
                site,
                reason: "the expression is not part of the model".to_owned(),
            }));
        };
        self.evaluate(owner_ref, compiled, owner_ref, site.unsupported_key())
    }

    /// Evaluates a compiled Boolean expression under the explicit scope of `scope_owner` (which
    /// exists); an Unparseable expression is the missing `(unsupported_owner, unsupported_key)`.
    fn evaluate(
        &mut self,
        scope_owner: &Id,
        compiled: &CompiledExpression,
        unsupported_owner: &Id,
        unsupported_key: String,
    ) -> Step<Option<bool>> {
        let model = self.model;
        let owner_ref = scope_owner;
        let Some(scope) = model.scopes.get(owner_ref) else {
            return Err(Stop::Error(SimulationError::Evaluation {
                owner_ref: owner_ref.clone(),
                reason: "the expression scope is not part of the model".to_owned(),
            }));
        };
        let ast = match compiled {
            CompiledExpression::Executable(ast) => ast,
            CompiledExpression::Unparseable => {
                self.miss(
                    unsupported_owner,
                    unsupported_key,
                    MissingSemanticKind::UnsupportedExpression,
                );
                return Ok(None);
            }
        };
        let used = expression_bindings(ast, &scope.roots, &scope.fields);
        for binding in &used {
            if self
                .model
                .graph
                .node(&binding.node_ref)
                .map(|n| n.payload.node_type())
                == Some(NodeType::Calculation)
            {
                self.calculation(&binding.node_ref)?;
            }
        }
        let Some(values) = bind_values(
            model.graph,
            &scope.env,
            &used,
            &self.state,
            &mut self.missing,
        )?
        else {
            return Ok(None);
        };
        let value = eval(
            ast,
            &EvalContext {
                types: &scope.env,
                values: &values,
                calendars: self.runtime.calendars,
                predicates: self.runtime.predicates,
            },
        )
        .map_err(|e| self.eval_stop(owner_ref, &e))?;
        match value {
            ExprValue::Bool(b) => Ok(Some(b)),
            _ => Err(Stop::Error(SimulationError::Evaluation {
                owner_ref: owner_ref.clone(),
                reason: "the expression is not Boolean".to_owned(),
            })),
        }
    }

    /// A three-valued conjunction: the first known false fails; otherwise missing values make
    /// the phase Undecidable.
    fn conjunction(&self, results: &[(ExecutionFailure, Option<bool>)]) -> Step<()> {
        if let Some((failure, _)) = results.iter().find(|(_, r)| *r == Some(false)) {
            return Err(Stop::Failed(failure.clone()));
        }
        self.checkpoint()
    }

    // ------------------------------------------------------------------ rules and tables

    fn rule(&mut self, rule_ref: &Id) -> Step<()> {
        let graph = self.model.graph;
        let Some(NodePayload::Rule(rule)) =
            accepted(graph, rule_ref, NodeType::Rule).map(|n| &n.payload)
        else {
            return Err(Stop::Error(SimulationError::InvalidScenario {
                scenario_ref: rule_ref.clone(),
                reason: "not an Accepted Rule".to_owned(),
            }));
        };
        let has_condition = rule.condition_expr.is_some();
        let has_action = rule.action_expr.is_some();
        for calculation_ref in targets(
            graph,
            rule_ref,
            RelationKind::UsesCalculation,
            NodeType::Calculation,
        ) {
            self.calculation(&calculation_ref)?;
        }
        if has_condition {
            if let Some(result) = self.expression(rule_ref, ExpressionSite::RuleCondition)? {
                self.rule_results.insert(rule_ref.clone(), result);
                self.trace.push(ExecutionTraceStep::RuleEvaluated {
                    rule_ref: rule_ref.clone(),
                    result,
                });
            }
        } else {
            self.miss(
                rule_ref,
                missing_keys::RULE_CONDITION,
                MissingSemanticKind::RuleCondition,
            );
        }
        if has_action {
            self.trace.push(ExecutionTraceStep::RuleActionNotExecuted {
                rule_ref: rule_ref.clone(),
            });
        }
        Ok(())
    }

    fn decision_table(&mut self, table_ref: &Id, inputs: &BTreeMap<u32, RuntimeValue>) -> Step<()> {
        let model = self.model;
        let Some(spec) = model.decision_tables.get(table_ref) else {
            self.miss(
                table_ref,
                missing_keys::DECISION_TABLE_SPEC,
                MissingSemanticKind::DecisionTableSpec,
            );
            return Ok(());
        };
        let spec: &DecisionTableSpec = spec;
        let mismatch = |index: u32, reason: &str| {
            Stop::Error(SimulationError::DecisionInputMismatch {
                decision_table_ref: table_ref.clone(),
                index,
                reason: reason.to_owned(),
            })
        };
        if let Some(index) = inputs.keys().find(|i| **i as usize >= spec.inputs.len()) {
            return Err(mismatch(*index, "no such input column"));
        }
        let mut values = Vec::new();
        for (index, column) in spec.inputs.iter().enumerate() {
            match inputs.get(&index_u32(index)) {
                Some(literal) if column_accepts(&column.ty, literal) => values.push(literal),
                Some(_) => {
                    return Err(mismatch(
                        index_u32(index),
                        "value does not match the column type",
                    ))
                }
                None => self.miss(
                    table_ref,
                    missing_keys::decision_input(index),
                    MissingSemanticKind::DecisionInput,
                ),
            }
        }
        if values.len() != spec.inputs.len() {
            return Ok(());
        }
        let matching: Vec<u32> = spec
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.inputs
                    .iter()
                    .zip(&values)
                    .all(|(cell, literal)| cell_matches(cell, literal))
            })
            .map(|(i, _)| index_u32(i))
            .collect();
        let row = match (HitPolicy::parse(&spec.hit_policy), matching.as_slice()) {
            (Some(HitPolicy::Unique), [_, _, ..]) => {
                return Err(Stop::Error(SimulationError::AmbiguousDecisionRow {
                    decision_table_ref: table_ref.clone(),
                    rows: matching,
                }))
            }
            (Some(_), rows) => rows.first().copied(),
            (None, _) => {
                return Err(Stop::Error(SimulationError::InvalidDecisionTableSpec {
                    decision_table_ref: table_ref.clone(),
                    reason: "hit policy is neither UNIQUE nor FIRST".to_owned(),
                }))
            }
        };
        let outputs = match row {
            Some(r) => &spec.rows[r as usize].outputs,
            None => spec.default_output.as_ref().ok_or_else(|| {
                Stop::Failed(ExecutionFailure::NoDecisionRow {
                    decision_table_ref: table_ref.clone(),
                })
            })?,
        };
        let outputs: BTreeMap<u32, RuntimeValue> = outputs
            .iter()
            .enumerate()
            .map(|(i, cell)| (index_u32(i), output_literal(cell)))
            .collect();
        self.state
            .decision_outputs
            .insert(table_ref.clone(), outputs);
        self.trace.push(ExecutionTraceStep::DecisionTableEvaluated {
            decision_table_ref: table_ref.clone(),
            row,
        });
        Ok(())
    }

    // ------------------------------------------------------------------ transitions

    /// Runs Accepted Transitions triggered by `trigger_ref` whose from_state is active.
    fn triggered_transitions(&mut self, trigger_ref: &Id) -> Step<()> {
        let graph = self.model.graph;
        let candidates: Vec<Id> = sources(
            graph,
            trigger_ref,
            RelationKind::TransitionsVia,
            NodeType::Transition,
        )
        .into_iter()
        .filter(
            |t| match accepted(graph, t, NodeType::Transition).map(|n| &n.payload) {
                Some(NodePayload::Transition(tr)) => {
                    self.state.active_states.contains(&tr.from_state)
                }
                _ => false,
            },
        )
        .collect();
        let mut by_source: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
        for t in &candidates {
            if let Some(NodePayload::Transition(tr)) = graph.node(t).map(|n| &n.payload) {
                by_source
                    .entry(tr.from_state.clone())
                    .or_default()
                    .push(t.clone());
            }
        }
        if let Some((from_state, refs)) = by_source.into_iter().find(|(_, refs)| refs.len() > 1) {
            return Err(Stop::Error(SimulationError::ConflictingTransitions {
                from_state,
                transition_refs: refs,
            }));
        }
        let mut guards = Vec::new();
        for t in &candidates {
            guards.push((
                ExecutionFailure::TransitionGuardRejected {
                    transition_ref: t.clone(),
                },
                self.guard(t)?,
            ));
        }
        self.conjunction(&guards)?;
        for t in &candidates {
            self.apply_transition(t);
        }
        Ok(())
    }

    /// The guard of a Transition: absent is true.
    fn guard(&mut self, transition_ref: &Id) -> Step<Option<bool>> {
        let has_guard = matches!(
            self.model.graph.node(transition_ref).map(|n| &n.payload),
            Some(NodePayload::Transition(t)) if t.guard_expr.is_some()
        );
        if !has_guard {
            return Ok(Some(true));
        }
        let result = self.expression(transition_ref, ExpressionSite::TransitionGuard)?;
        if let Some(result) = result {
            self.trace.push(ExecutionTraceStep::GuardEvaluated {
                transition_ref: transition_ref.clone(),
                result,
            });
        }
        Ok(result)
    }

    fn apply_transition(&mut self, transition_ref: &Id) {
        let Some(NodePayload::Transition(t)) =
            self.model.graph.node(transition_ref).map(|n| &n.payload)
        else {
            return;
        };
        let t = t.clone();
        self.state.active_states.remove(&t.from_state);
        self.state.active_states.insert(t.to_state.clone());
        self.invariant_targets.insert(t.stateful_ref.clone());
        self.trace.push(ExecutionTraceStep::TransitionApplied {
            transition_ref: transition_ref.clone(),
            from_state: t.from_state,
            to_state: t.to_state,
        });
        let mut effects = t.effect_refs.unwrap_or_default();
        effects.sort();
        effects.dedup();
        if !effects.is_empty() {
            self.trace
                .push(ExecutionTraceStep::TransitionEffectsRecorded {
                    transition_ref: transition_ref.clone(),
                    effect_refs: effects,
                });
        }
    }

    fn direct_transition(&mut self, transition_ref: &Id) -> Step<()> {
        let graph = self.model.graph;
        let Some(NodePayload::Transition(t)) =
            accepted(graph, transition_ref, NodeType::Transition).map(|n| &n.payload)
        else {
            return Err(Stop::Error(SimulationError::InvalidScenario {
                scenario_ref: transition_ref.clone(),
                reason: "not an Accepted Transition".to_owned(),
            }));
        };
        let from_state = t.from_state.clone();
        let trigger_ref = graph
            .outgoing_edge_ids(transition_ref)
            .iter()
            .filter_map(|e| graph.edge(e))
            .find(|e| e.kind == RelationKind::TransitionsVia && e.status == ElementStatus::Accepted)
            .map(|e| e.to.clone());
        self.trace
            .push(ExecutionTraceStep::TransitionTriggerRecorded {
                transition_ref: transition_ref.clone(),
                trigger_ref,
            });
        if !self.state.active_states.contains(&from_state) {
            return Err(Stop::Failed(ExecutionFailure::InvalidSourceState {
                transition_ref: transition_ref.clone(),
                from_state,
            }));
        }
        let guard = self.guard(transition_ref)?;
        self.conjunction(&[(
            ExecutionFailure::TransitionGuardRejected {
                transition_ref: transition_ref.clone(),
            },
            guard,
        )])?;
        self.apply_transition(transition_ref);
        Ok(())
    }

    // ------------------------------------------------------------------ operations

    fn authorize(&mut self, operation_ref: &Id) -> Step<()> {
        let graph = self.model.graph;
        let permissions = sources(
            graph,
            operation_ref,
            RelationKind::Permits,
            NodeType::Permission,
        );
        let actor_ref = self.runtime.actor_ref.clone();
        let checked = |decision: AuthorizationDecision| ExecutionTraceStep::AuthorizationChecked {
            operation_ref: operation_ref.clone(),
            actor_ref: actor_ref.clone(),
            decision,
        };
        if permissions.is_empty() {
            self.trace
                .push(checked(AuthorizationDecision::NotApplicable));
            return Ok(());
        }
        let Some(actor) = &actor_ref else {
            self.miss(
                operation_ref,
                missing_keys::RUNTIME_ACTOR,
                MissingSemanticKind::RuntimeActor,
            );
            return self.checkpoint();
        };
        if accepted(graph, actor, NodeType::Principal).is_none()
            && accepted(graph, actor, NodeType::Actor).is_none()
        {
            return Err(Stop::Error(SimulationError::InvalidRuntimeActor {
                actor_ref: actor.clone(),
            }));
        }
        let roles: BTreeSet<Id> =
            effective_security_roles(&authorization_facts(graph, true), actor)
                .into_iter()
                .collect();
        let granting: Vec<Id> = permissions
            .into_iter()
            .filter(|p| {
                sources(graph, p, RelationKind::Grants, NodeType::SecurityRole)
                    .iter()
                    .any(|r| roles.contains(r))
            })
            .collect();
        if granting.is_empty() {
            self.trace.push(checked(AuthorizationDecision::Denied));
            return Err(Stop::Failed(ExecutionFailure::AuthorizationDenied {
                operation_ref: operation_ref.clone(),
                actor_ref: actor.clone(),
            }));
        }
        let (unconditioned, conditioned): (Vec<Id>, Vec<Id>) =
            granting.into_iter().partition(|p| {
                targets(
                    graph,
                    p,
                    RelationKind::ConditionedBy,
                    NodeType::PolicyCondition,
                )
                .is_empty()
            });
        if unconditioned.is_empty() {
            self.trace
                .push(checked(AuthorizationDecision::ConditionsNotEvaluated {
                    permission_refs: conditioned.clone(),
                }));
            for p in &conditioned {
                self.miss(
                    p,
                    missing_keys::POLICY_CONDITION,
                    MissingSemanticKind::PolicyCondition,
                );
            }
            return self.checkpoint();
        }
        let scopes: BTreeSet<Id> = unconditioned
            .iter()
            .flat_map(|p| targets(graph, p, RelationKind::ScopedTo, NodeType::ResourceScope))
            .collect();
        self.trace.push(checked(AuthorizationDecision::Allowed {
            permission_refs: unconditioned,
            unevaluated_scope_refs: scopes.into_iter().collect(),
        }));
        Ok(())
    }

    /// Pre- or postconditions in listed order as a three-valued conjunction.
    fn conditions(&mut self, operation_ref: &Id, count: usize, post: bool) -> Step<()> {
        let mut results = Vec::new();
        for i in 0..count {
            let index = index_u32(i);
            let (site, failure) = if post {
                (
                    ExpressionSite::Postcondition { index },
                    ExecutionFailure::PostconditionViolation {
                        operation_ref: operation_ref.clone(),
                        index,
                    },
                )
            } else {
                (
                    ExpressionSite::Precondition { index },
                    ExecutionFailure::PreconditionRejected {
                        operation_ref: operation_ref.clone(),
                        index,
                    },
                )
            };
            let result = self.expression(operation_ref, site)?;
            if let Some(result) = result {
                self.trace.push(if post {
                    ExecutionTraceStep::PostconditionEvaluated {
                        operation_ref: operation_ref.clone(),
                        index,
                        result,
                    }
                } else {
                    ExecutionTraceStep::PreconditionEvaluated {
                        operation_ref: operation_ref.clone(),
                        index,
                        result,
                    }
                });
            }
            results.push((failure, result));
        }
        self.conjunction(&results)
    }

    fn operation(&mut self, operation_ref: &Id) -> Step<Vec<Id>> {
        let graph = self.model.graph;
        let Some(NodePayload::Operation(op)) =
            accepted(graph, operation_ref, NodeType::Operation).map(|n| &n.payload)
        else {
            return Err(Stop::Error(SimulationError::InvalidScenario {
                scenario_ref: operation_ref.clone(),
                reason: "not an Accepted Operation".to_owned(),
            }));
        };
        let footprint = |kind: RelationKind| -> Vec<Id> {
            let mut out = targets(graph, operation_ref, kind.clone(), NodeType::Attribute);
            out.extend(targets(graph, operation_ref, kind, NodeType::Entity));
            out.sort();
            out
        };
        let (reads, writes) = (
            footprint(RelationKind::Reads),
            footprint(RelationKind::Writes),
        );
        self.invariant_targets.extend(reads.iter().cloned());
        self.invariant_targets.extend(writes.iter().cloned());
        self.trace.push(ExecutionTraceStep::FootprintDeclared {
            operation_ref: operation_ref.clone(),
            reads,
            writes,
        });
        let mut performers = targets(
            graph,
            operation_ref,
            RelationKind::PerformedBy,
            NodeType::Actor,
        );
        performers.extend(targets(
            graph,
            operation_ref,
            RelationKind::PerformedBy,
            NodeType::BusinessRole,
        ));
        performers.sort();
        self.trace.push(ExecutionTraceStep::PerformerRecorded {
            operation_ref: operation_ref.clone(),
            performer_refs: performers,
        });
        let pre = op.preconditions.as_ref().map_or(0, Vec::len);
        let post = op.postconditions.as_ref().map_or(0, Vec::len);

        self.authorize(operation_ref)?;
        self.conditions(operation_ref, pre, false)?;

        for rule_ref in targets(
            graph,
            operation_ref,
            RelationKind::GovernedBy,
            NodeType::Rule,
        ) {
            self.rule(&rule_ref)?;
        }
        let tables = targets(
            graph,
            operation_ref,
            RelationKind::GovernedBy,
            NodeType::DecisionTable,
        );
        // The Given decision inputs name no table: with several governing tables they cannot
        // be routed, a dedicated typed condition rather than silently discarded inputs
        // (H0.4, Fable #117). Without inputs every table reports its own missing columns.
        if tables.len() > 1 && !self.decision_inputs.is_empty() {
            self.miss(
                operation_ref,
                missing_keys::DECISION_INPUT_ROUTING,
                MissingSemanticKind::ModelCondition,
            );
        } else {
            let inputs = self.decision_inputs.clone();
            for table_ref in &tables {
                self.decision_table(table_ref, &inputs)?;
            }
        }
        self.checkpoint()?;

        for calculation_ref in targets(
            graph,
            operation_ref,
            RelationKind::UsesCalculation,
            NodeType::Calculation,
        ) {
            self.calculation(&calculation_ref)?;
        }
        self.checkpoint()?;

        // Outcome selection keeps its frozen trace position (S4.2): the interpreter never
        // writes attributes and every calculation has run, so a `when` over the scope sees the
        // post-execution values already.
        self.success_outcome(operation_ref)?;
        self.triggered_transitions(operation_ref)?;
        self.conditions(operation_ref, post, true)?;
        Ok(targets(
            graph,
            operation_ref,
            RelationKind::Produces,
            NodeType::Event,
        ))
    }

    // ------------------------------------------------------------------ outcomes

    /// Whether `outcome_ref` carries selection material for `operation_ref`; an invalid value
    /// counts as material (it never selects).
    fn has_selection(&self, outcome_ref: &Id, operation_ref: &Id) -> bool {
        match self.model.outcome_selections.get(outcome_ref) {
            None => false,
            Some(None) => true,
            Some(Some(selection)) => selection.entry(operation_ref).is_some(),
        }
    }

    fn select_outcome(&mut self, operation_ref: &Id, outcome_ref: &Id) {
        self.outcome_refs.push(outcome_ref.clone());
        self.trace.push(ExecutionTraceStep::OutcomeSelected {
            operation_ref: operation_ref.clone(),
            outcome_ref: outcome_ref.clone(),
        });
    }

    fn outcome_undecidable(&mut self, operation_ref: &Id) -> Step<()> {
        self.miss(
            operation_ref,
            missing_keys::OUTCOME_SELECTION,
            MissingSemanticKind::OutcomeSelection,
        );
        self.checkpoint()
    }

    /// Successful execution (architecture §33.4): the candidates are the Operation's Accepted
    /// `success`/`partial` Outcomes. One candidate without selection material is selected;
    /// otherwise every candidate needs a `when` for this Operation over the post-execution
    /// state and exactly one true selects it. Anything else is Undecidable OUTCOME_SELECTION.
    fn success_outcome(&mut self, operation_ref: &Id) -> Step<()> {
        let graph = self.model.graph;
        let candidates: Vec<Id> = targets(
            graph,
            operation_ref,
            RelationKind::Produces,
            NodeType::Outcome,
        )
        .into_iter()
        .filter(|o| {
            matches!(
                graph.node(o).map(|n| &n.payload),
                Some(NodePayload::Outcome(outcome))
                    if matches!(outcome.outcome_kind, OutcomeKind::Success | OutcomeKind::Partial)
            )
        })
        .collect();
        match candidates.as_slice() {
            [] => return Ok(()),
            [only] if !self.has_selection(only, operation_ref) => {
                self.select_outcome(operation_ref, only);
                return Ok(());
            }
            _ => {}
        }
        let model = self.model;
        let mut chosen = Vec::new();
        let mut complete = true;
        for candidate in &candidates {
            let has_when = matches!(
                model.outcome_selections.get(candidate),
                Some(Some(selection))
                    if selection.entry(operation_ref).is_some_and(|e| e.when.is_some())
            );
            if !has_when {
                complete = false;
                continue;
            }
            if !model.scopes.contains_key(operation_ref) {
                self.miss(
                    operation_ref,
                    missing_keys::EXPRESSION_SCOPE,
                    MissingSemanticKind::ExpressionScope,
                );
                continue;
            }
            let Some(compiled) = model
                .outcome_conditions
                .get(&(candidate.clone(), operation_ref.clone()))
            else {
                return Err(Stop::Error(SimulationError::InvalidOutcomeSelection {
                    outcome_ref: candidate.clone(),
                    operation_ref: operation_ref.clone(),
                    reason: "the condition is not part of the model".to_owned(),
                }));
            };
            let key = missing_keys::OUTCOME_SELECTION.to_owned();
            if self.evaluate(operation_ref, compiled, candidate, key)? == Some(true) {
                chosen.push(candidate.clone());
            }
        }
        if !complete {
            return self.outcome_undecidable(operation_ref);
        }
        self.checkpoint()?;
        match chosen.as_slice() {
            [one] => {
                self.select_outcome(operation_ref, one);
                Ok(())
            }
            _ => self.outcome_undecidable(operation_ref),
        }
    }

    // ------------------------------------------------------------------ invariants

    fn invariants(&mut self) -> Step<()> {
        let graph = self.model.graph;
        let applicable: Vec<Id> = graph
            .node_ids_by_type(NodeType::Invariant)
            .iter()
            .filter(
                |id| match accepted(graph, id, NodeType::Invariant).map(|n| &n.payload) {
                    Some(NodePayload::Invariant(i)) => {
                        self.invariant_targets.contains(&i.scope_ref)
                    }
                    _ => false,
                },
            )
            .cloned()
            .collect();
        let mut results = Vec::new();
        for invariant_ref in applicable {
            let result = self.expression(&invariant_ref, ExpressionSite::Invariant)?;
            if let Some(result) = result {
                self.trace.push(ExecutionTraceStep::InvariantEvaluated {
                    invariant_ref: invariant_ref.clone(),
                    result,
                });
            }
            results.push((
                ExecutionFailure::InvariantViolation { invariant_ref },
                result,
            ));
        }
        self.conjunction(&results)
    }

    // ------------------------------------------------------------------ anchors

    fn execute(&mut self, when: &ScenarioWhen) -> Step<Vec<Id>> {
        let mut events = Vec::new();
        match when {
            ScenarioWhen::Operation(id) => {
                self.invariant_targets.insert(id.clone());
                events = self.operation(id)?;
            }
            ScenarioWhen::Event(id) => {
                self.invariant_targets.insert(id.clone());
                self.trace.push(ExecutionTraceStep::EventOccurred {
                    event_ref: id.clone(),
                });
                self.triggered_transitions(id)?;
            }
            ScenarioWhen::Transition(id) => {
                self.invariant_targets.insert(id.clone());
                self.direct_transition(id)?;
            }
            ScenarioWhen::Rule(id) => {
                self.invariant_targets.insert(id.clone());
                self.rule(id)?;
            }
            ScenarioWhen::Calculation(id) => {
                self.invariant_targets.insert(id.clone());
                self.calculation(id)?;
            }
            ScenarioWhen::DecisionTable(id) => {
                self.invariant_targets.insert(id.clone());
                let inputs = self.decision_inputs.clone();
                self.decision_table(id, &inputs)?;
            }
        }
        self.checkpoint()?;
        self.invariants()?;
        for event_ref in &events {
            self.trace.push(ExecutionTraceStep::EventEmitted {
                event_ref: event_ref.clone(),
            });
        }
        self.trace.push(ExecutionTraceStep::ExecutionCompleted);
        Ok(events)
    }
}

/// The Operation whose modeled business failures a rejection of the `when` anchor selects
/// among: the anchor Operation, or the Accepted triggering Operation of a Transition anchor.
fn rejecting_operation(graph: &Graph, when: &ScenarioWhen) -> Option<Id> {
    match when {
        ScenarioWhen::Operation(id) => Some(id.clone()),
        ScenarioWhen::Transition(transition_ref) => {
            let triggers = targets(
                graph,
                transition_ref,
                RelationKind::TransitionsVia,
                NodeType::Operation,
            );
            match triggers.as_slice() {
                [operation_ref] => Some(operation_ref.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The Accepted `business_failure` Outcomes of `operation_ref` whose selection entry lists
/// `kind`; `None` when an invalid selection value makes the candidates unknown.
fn rejection_outcomes(
    model: &ExecutionModel<'_>,
    operation_ref: &Id,
    kind: BusinessFailureKind,
) -> Option<Vec<Id>> {
    let graph = model.graph;
    let mut out = Vec::new();
    for outcome_ref in targets(
        graph,
        operation_ref,
        RelationKind::Produces,
        NodeType::Outcome,
    ) {
        let business = matches!(
            graph.node(&outcome_ref).map(|n| &n.payload),
            Some(NodePayload::Outcome(o)) if o.outcome_kind == OutcomeKind::BusinessFailure
        );
        if !business {
            continue;
        }
        match model.outcome_selections.get(&outcome_ref) {
            None => {}
            Some(None) => return None,
            Some(Some(selection)) => {
                let listed = selection
                    .entry(operation_ref)
                    .and_then(|e| e.on_failures.as_ref())
                    .is_some_and(|kinds| kinds.contains(&kind));
                if listed {
                    out.push(outcome_ref);
                }
            }
        }
    }
    Some(out)
}

fn undecidable(
    mut trace: Vec<ExecutionTraceStep>,
    missing: BTreeSet<MissingSemantic>,
) -> ExecutionResult {
    let missing: Vec<MissingSemantic> = missing.into_iter().collect();
    trace.push(ExecutionTraceStep::SemanticMissing {
        missing: missing.clone(),
    });
    ExecutionResult::Undecidable { missing, trace }
}

/// Executes one Accepted, Confirmed Scenario. Pure: the graph, the Scenario and the caller's
/// state are never mutated and nothing is persisted.
///
/// A content-dependent model condition makes only this run Undecidable with a typed
/// `ModelCondition` (H0.4, Fable #116); every other error is no verdict. A business rejection
/// with exactly one modeled `business_failure` Outcome is `Rejected` (architecture §33.4).
pub fn run(
    model: &ExecutionModel<'_>,
    scenario_ref: &Id,
    runtime: &ExecutionRuntime<'_>,
) -> Result<ExecutionRun, SimulationError> {
    let admitted = admit(model, scenario_ref)?;
    let runtime_values = runtime_values(model, runtime)?;
    let finish = |result: ExecutionResult| ExecutionRun {
        scenario_ref: scenario_ref.clone(),
        specification_hash: admitted.specification_hash.clone(),
        result,
    };
    let given = match materialize(model, scenario_ref, &admitted.semantics, &runtime_values) {
        Ok(given) => given,
        Err(e) => {
            let condition = e.model_condition().ok_or(e)?;
            let trace = vec![ExecutionTraceStep::MaterializationStarted {
                scenario_ref: scenario_ref.clone(),
            }];
            return Ok(finish(undecidable(trace, BTreeSet::from([condition]))));
        }
    };
    let trace = given.trace;
    if !given.missing.is_empty() {
        return Ok(finish(undecidable(trace, given.missing)));
    }
    let pre_state = given.state.clone();
    let mut executor = Executor {
        model,
        runtime,
        state: given.state,
        decision_inputs: given.decision_inputs,
        trace,
        missing: BTreeSet::new(),
        attempted: BTreeSet::new(),
        rule_results: BTreeMap::new(),
        outcome_refs: Vec::new(),
        invariant_targets: BTreeSet::new(),
    };
    let when = &admitted.semantics.when;
    let outcome = executor.execute(when);
    let mut trace = executor.trace;
    Ok(finish(match outcome {
        Ok(event_refs) => ExecutionResult::Executed {
            state: executor.state,
            outcome_refs: executor.outcome_refs,
            event_refs,
            rule_results: executor.rule_results,
            trace,
        },
        Err(Stop::Failed(failure)) => {
            let rejection = failure
                .business_kind()
                .zip(rejecting_operation(model.graph, when));
            let Some((kind, operation_ref)) = rejection else {
                return Ok(finish(ExecutionResult::Failed { failure, trace }));
            };
            match rejection_outcomes(model, &operation_ref, kind).as_deref() {
                Some([]) => ExecutionResult::Failed { failure, trace },
                Some([outcome_ref]) => {
                    trace.push(ExecutionTraceStep::OutcomeSelected {
                        operation_ref,
                        outcome_ref: outcome_ref.clone(),
                    });
                    trace.push(ExecutionTraceStep::ExecutionCompleted);
                    ExecutionResult::Rejected {
                        failure,
                        outcome_ref: outcome_ref.clone(),
                        state: pre_state,
                        rule_results: executor.rule_results,
                        trace,
                    }
                }
                _ => undecidable(
                    trace,
                    BTreeSet::from([MissingSemantic {
                        owner_ref: operation_ref,
                        key: missing_keys::OUTCOME_SELECTION.to_owned(),
                        kind: MissingSemanticKind::OutcomeSelection,
                    }]),
                ),
            }
        }
        Err(Stop::Undecidable) => undecidable(trace, executor.missing),
        Err(Stop::Error(e)) => {
            let condition = e.model_condition().ok_or(e)?;
            let mut missing = executor.missing;
            missing.insert(condition);
            undecidable(trace, missing)
        }
    }))
}
