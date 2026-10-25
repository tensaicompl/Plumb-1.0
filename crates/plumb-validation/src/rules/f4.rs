//! The seven F4 evaluators: functional behavior is executable or otherwise verifiable without
//! invented semantics (rulebook F4, plan S4.6, Hotfix 055).
//!
//! Every evaluator reads the Accepted PSG and the validated F4 supplemental inputs: resolved
//! verification applicability, content-bound Scenario type checks, current ScenarioRuns with
//! their S4.3 trace artifacts and the assumptions meant to be visible. Nothing here derives
//! applicability, re-validates Scenario semantics, executes or compares Scenarios, reads time,
//! mutates the graph or persists anything; finding identity, severity and waivers stay with the
//! evaluator engine.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use plumb_psg::{
    ElementStatus, Graph, NodePayload, NodeType, RelationKind, Scenario, ScenarioKind,
    ScenarioRunResult, VerificationKind,
};
use serde_json::Value;

use crate::evaluator::{
    EvaluationError, EvaluatorFailure, EvaluatorRegistry, F4ScenarioTypeStatus, F4ValidationInputs,
    RuleEvaluation, RuleEvaluator, ValidationContext,
};
use crate::model::RuleMetadata;

const VERIFICATION_LINK: &str = "PLUMB.F4.FUNCTION.VERIFICATION_LINK";
const FOR_EXECUTABLE_BEHAVIOR: &str = "PLUMB.F4.SCENARIO.FOR_EXECUTABLE_BEHAVIOR";
const GROUNDED: &str = "PLUMB.F4.SCENARIO.GROUNDED";
const TYPED: &str = "PLUMB.F4.SCENARIO.TYPED";
const NO_BLOCKING_UNDECIDABLE: &str = "PLUMB.F4.RUN.NO_BLOCKING_UNDECIDABLE";
const TRACE_COMPLETE: &str = "PLUMB.F4.RUN.TRACE_COMPLETE";
const ASSUMPTIONS_VISIBLE: &str = "PLUMB.F4.ASSUMPTIONS.VISIBLE";

const F4_INPUTS_MISSING: &str = "F4_INPUTS_MISSING";

/// The S4 derivation sources and anchors that ground a Scenario (Hotfix 055).
const GROUNDING_TYPES: [NodeType; 9] = [
    NodeType::Requirement,
    NodeType::AcceptanceCriterion,
    NodeType::Rule,
    NodeType::Transition,
    NodeType::DecisionTable,
    NodeType::Operation,
    NodeType::Event,
    NodeType::Calculation,
    NodeType::Calendar,
];

/// The frozen S4.2 execution step names.
const EXECUTION_STEPS: [&str; 22] = [
    "materialization_started",
    "semantic_resolved",
    "semantic_missing",
    "given_materialized",
    "footprint_declared",
    "performer_recorded",
    "authorization_checked",
    "precondition_evaluated",
    "rule_evaluated",
    "rule_action_not_executed",
    "decision_table_evaluated",
    "calculation_evaluated",
    "outcome_selected",
    "transition_trigger_recorded",
    "guard_evaluated",
    "transition_applied",
    "transition_effects_recorded",
    "event_occurred",
    "postcondition_evaluated",
    "invariant_evaluated",
    "event_emitted",
    "execution_completed",
];

/// The frozen S4.3 comparison step names.
const COMPARISON_STEPS: [&str; 11] = [
    "comparison_started",
    "expected_value_resolved",
    "expected_semantic_missing",
    "attribute_compared",
    "state_compared",
    "outcomes_compared",
    "events_compared",
    "rule_compared",
    "decision_output_compared",
    "calculation_compared",
    "comparison_completed",
];

/// Execution steps that carry business-semantic evidence (lifecycle steps excluded).
const SEMANTIC_STEPS: [&str; 17] = [
    "footprint_declared",
    "performer_recorded",
    "authorization_checked",
    "precondition_evaluated",
    "rule_evaluated",
    "rule_action_not_executed",
    "decision_table_evaluated",
    "calculation_evaluated",
    "outcome_selected",
    "transition_trigger_recorded",
    "guard_evaluated",
    "transition_applied",
    "transition_effects_recorded",
    "event_occurred",
    "postcondition_evaluated",
    "invariant_evaluated",
    "event_emitted",
];

/// Binds the seven F4 evaluators. Rule metadata stays in the registry.
pub fn register_f4_evaluators(registry: &mut EvaluatorRegistry) -> Result<(), EvaluationError> {
    let evaluators: [(&str, RuleEvaluator); 7] = [
        (VERIFICATION_LINK, verification_link),
        (FOR_EXECUTABLE_BEHAVIOR, for_executable_behavior),
        (GROUNDED, grounded),
        (TYPED, typed),
        (NO_BLOCKING_UNDECIDABLE, no_blocking_undecidable),
        (TRACE_COMPLETE, trace_complete),
        (ASSUMPTIONS_VISIBLE, assumptions_visible),
    ];
    for (rule_id, evaluator) in evaluators {
        registry.register(rule_id, evaluator)?;
    }
    Ok(())
}

// ============================================================================ outcomes

type Outcome = Result<RuleEvaluation, EvaluatorFailure>;

fn failure(code: &str, message: String) -> EvaluatorFailure {
    EvaluatorFailure {
        code: code.to_owned(),
        message,
        targets: Vec::new(),
        evidence: Vec::new(),
    }
}

fn output(evaluation: Result<RuleEvaluation, EvaluationError>) -> Outcome {
    evaluation.map_err(|e| failure("F4_OUTPUT", e.to_string()))
}

fn not_applicable(reason: &str) -> Outcome {
    output(RuleEvaluation::not_applicable(reason.to_owned()))
}

/// Deterministic aggregate of one rule: checked targets, violating targets and evidence.
#[derive(Default)]
struct Verdict {
    checked: BTreeSet<Id>,
    violating: BTreeSet<Id>,
    evidence: BTreeSet<String>,
}

impl Verdict {
    fn check(&mut self, id: &Id) {
        self.checked.insert(id.clone());
    }

    fn violate(&mut self, id: &Id, evidence: Option<String>) {
        self.violating.insert(id.clone());
        self.evidence.extend(evidence);
    }

    /// PASS over the checked targets, or one aggregate violation of the violating ones.
    fn conclude(self, condition: &str, message: &str, resolution: &str) -> Outcome {
        output(if self.violating.is_empty() {
            RuleEvaluation::pass(self.checked.into_iter().collect(), Vec::new())
        } else {
            RuleEvaluation::violation(
                self.violating.into_iter().collect(),
                self.evidence.into_iter().collect(),
                condition.to_owned(),
                message.to_owned(),
                Some(resolution.to_owned()),
            )
        })
    }
}

fn inputs(ctx: &ValidationContext) -> Result<&F4ValidationInputs, EvaluatorFailure> {
    ctx.f4_inputs.as_ref().ok_or_else(|| {
        failure(
            F4_INPUTS_MISSING,
            "F4 validation inputs are required".to_owned(),
        )
    })
}

// ============================================================================ graph access

fn accepted_node(graph: &Graph, id: &Id) -> bool {
    graph
        .node(id)
        .is_some_and(|n| n.status == ElementStatus::Accepted)
}

/// Accepted targets of Accepted `kind` edges from `from`.
fn accepted_targets(graph: &Graph, from: &Id, kind: RelationKind) -> BTreeSet<Id> {
    graph
        .outgoing_edge_ids(from)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == kind && e.status == ElementStatus::Accepted)
        .filter(|e| accepted_node(graph, &e.to))
        .map(|e| e.to.clone())
        .collect()
}

/// The Accepted VerificationObligations a source is verified by, with their kinds.
fn obligations(graph: &Graph, source: &Id) -> BTreeMap<Id, VerificationKind> {
    accepted_targets(graph, source, RelationKind::VerifiedBy)
        .into_iter()
        .filter_map(|o| match graph.node(&o).map(|n| &n.payload) {
            Some(NodePayload::VerificationObligation(v)) => Some((o, v.verification_kind)),
            _ => None,
        })
        .collect()
}

fn scenario<'g>(graph: &'g Graph, id: &Id) -> Option<&'g Scenario> {
    match graph.node(id).map(|n| &n.payload) {
        Some(NodePayload::Scenario(s)) => Some(s),
        _ => None,
    }
}

fn traceability(s: &Scenario) -> impl Iterator<Item = &Id> {
    s.requirement_refs
        .iter()
        .flatten()
        .chain(s.derived_from_refs.iter().flatten())
}

fn kind_text(kind: ScenarioKind) -> String {
    match serde_json::to_value(kind) {
        Ok(Value::String(s)) => s,
        _ => format!("{kind:?}"),
    }
}

// ============================================================================ verification

fn verification_link(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let sources = &inputs.applicability.source_refs;
    if sources.is_empty() {
        return not_applicable("no implementation-scoped verification sources");
    }
    let mut verdict = Verdict::default();
    for source in sources {
        verdict.check(source);
        if obligations(graph, source).is_empty() {
            verdict.violate(source, None);
        }
    }
    verdict.conclude(
        "implementation_scoped_source_unverified",
        "Implementation-scoped verification sources must link to an accepted VerificationObligation.",
        "Accept or create the required VerificationObligation and verified_by relation for each affected source.",
    )
}

fn for_executable_behavior(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let a = &inputs.applicability;
    if a.executable_source_refs.is_empty() {
        return not_applicable("no executable verification sources");
    }
    let in_scope: BTreeSet<&Id> = a.in_scope_scenario_refs.iter().collect();
    let mut verdict = Verdict::default();
    for source in &a.executable_source_refs {
        verdict.check(source);
        let implementations: BTreeSet<Id> = obligations(graph, source)
            .into_iter()
            .filter(|(_, kind)| *kind == VerificationKind::Scenario)
            .flat_map(|(o, _)| accepted_targets(graph, &o, RelationKind::ImplementedAs))
            .filter(|s| in_scope.contains(s))
            .collect();
        if implementations.is_empty() {
            verdict.violate(source, Some(format!("missing_scenario:{source}")));
        }
        let kinds: BTreeSet<ScenarioKind> = implementations
            .iter()
            .filter_map(|s| scenario(graph, s).map(|s| s.scenario_kind))
            .collect();
        for required in inputs.required_kinds(source).unwrap_or_default() {
            if !kinds.contains(required) {
                verdict.violate(
                    source,
                    Some(format!("missing_kind:{source}:{}", kind_text(*required))),
                );
            }
        }
    }
    verdict.conclude(
        "executable_source_scenario_coverage_incomplete",
        "Executable verification sources need accepted Confirmed Scenario coverage for the resolved profile.",
        "Accept representative Scenario implementations and every ScenarioKind explicitly required by the resolved profile.",
    )
}

// ============================================================================ scenarios

fn grounded(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let scenarios = &inputs.applicability.in_scope_scenario_refs;
    if scenarios.is_empty() {
        return not_applicable("no in-scope Scenarios");
    }
    let mut verdict = Verdict::default();
    for id in scenarios {
        verdict.check(id);
        let grounded = scenario(graph, id).is_some_and(|s| {
            traceability(s).any(|r| {
                graph.node(r).is_some_and(|n| {
                    n.status == ElementStatus::Accepted
                        && GROUNDING_TYPES.contains(&n.payload.node_type())
                })
            })
        });
        if !grounded {
            verdict.violate(id, None);
        }
    }
    verdict.conclude(
        "scenario_not_grounded",
        "In-scope accepted Scenarios must trace to an accepted semantic derivation source.",
        "Add or correct requirement_refs/derived_from_refs using an accepted S4 grounding source.",
    )
}

fn typed(_: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let scenarios = &inputs.applicability.in_scope_scenario_refs;
    if scenarios.is_empty() {
        return not_applicable("no in-scope Scenarios");
    }
    let mut verdict = Verdict::default();
    for id in scenarios {
        verdict.check(id);
        match inputs.type_check(id).map(|c| &c.status) {
            Some(F4ScenarioTypeStatus::Valid) => {}
            Some(F4ScenarioTypeStatus::Invalid { issues }) => {
                for issue in issues {
                    verdict.violate(id, Some(format!("type_issue:{id}:{issue}")));
                }
            }
            None => {
                return Err(failure(
                    "F4_TYPE_CHECK_MISSING",
                    format!("no bound type check for {id}"),
                ))
            }
        }
    }
    verdict.conclude(
        "scenario_not_typed",
        "In-scope Scenario Given/When/Then material must satisfy the S4 typed scenario contract.",
        "Correct the Scenario fixture types, units, enum domains, references or operation inputs and revalidate it.",
    )
}

// ============================================================================ runs

fn no_blocking_undecidable(_: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let blocking = &inputs.applicability.blocking_scenario_refs;
    if blocking.is_empty() {
        return not_applicable("no blocking executable Scenarios");
    }
    let mut verdict = Verdict::default();
    for id in blocking {
        verdict.check(id);
        match inputs.scenario_run(id).map(|r| r.run.result) {
            Some(ScenarioRunResult::Pass) => {}
            Some(ScenarioRunResult::Fail) => verdict.violate(id, Some(format!("run_fail:{id}"))),
            Some(ScenarioRunResult::Undecidable) => {
                verdict.violate(id, Some(format!("run_undecidable:{id}")))
            }
            None => verdict.violate(id, Some(format!("missing_run:{id}"))),
        }
    }
    verdict.conclude(
        "blocking_scenario_run_not_acceptable",
        "Every blocking executable Scenario must have a current passing ScenarioRun.",
        "Execute the current confirmed Scenario and resolve failures or missing semantics until its comparison verdict is Pass.",
    )
}

/// The first stable reason a trace is incomplete, if any.
fn trace_issue(
    scenario_ref: &Id,
    result: ScenarioRunResult,
    trace: &[Value],
) -> Option<&'static str> {
    let scenario_value = serde_json::to_value(scenario_ref).unwrap_or(Value::Null);
    let result_value = serde_json::to_value(result).unwrap_or(Value::Null);
    let mut entries = Vec::new();
    for entry in trace {
        let (Some(phase), Some(step)) = (
            entry.get("phase").and_then(Value::as_str),
            entry.get("step").filter(|s| s.is_object()),
        ) else {
            return Some("trace_malformed");
        };
        if entry.as_object().is_some_and(|o| o.len() != 2) {
            return Some("trace_malformed");
        }
        let Some(name) = step.get("step").and_then(Value::as_str) else {
            return Some("trace_malformed");
        };
        let known = match phase {
            "execution" => EXECUTION_STEPS.contains(&name),
            "comparison" => COMPARISON_STEPS.contains(&name),
            _ => return Some("trace_unknown_phase"),
        };
        if !known {
            return Some("trace_unknown_step");
        }
        entries.push((phase == "execution", name, step));
    }
    let execution: Vec<_> = entries.iter().filter(|e| e.0).collect();
    let comparison: Vec<_> = entries.iter().filter(|e| !e.0).collect();
    let Some(first) = execution.first() else {
        return Some("trace_missing_execution_phase");
    };
    let Some(first_comparison) = comparison.first() else {
        return Some("trace_missing_comparison_phase");
    };
    if entries.iter().skip(execution.len()).any(|e| e.0) || !entries[0].0 {
        return Some("trace_bad_order");
    }
    if first.1 != "materialization_started" || first.2.get("scenario_ref") != Some(&scenario_value)
    {
        return Some("trace_bad_start");
    }
    if first_comparison.1 != "comparison_started"
        || first_comparison.2.get("scenario_ref") != Some(&scenario_value)
    {
        return Some("trace_bad_comparison_start");
    }
    let last = entries.last()?;
    if last.1 != "comparison_completed" {
        return Some("trace_incomplete");
    }
    if last.2.get("result") != Some(&result_value) {
        return Some("trace_result_mismatch");
    }
    let diff_count_ok = last
        .2
        .get("diff_count")
        .and_then(Value::as_u64)
        .is_some_and(|n| u32::try_from(n).is_ok());
    if !diff_count_ok {
        return Some("trace_bad_diff_count");
    }
    let semantic = execution.iter().any(|e| SEMANTIC_STEPS.contains(&e.1));
    let missing = entries
        .iter()
        .any(|e| matches!(e.1, "semantic_missing" | "expected_semantic_missing"));
    let explained = semantic || (result == ScenarioRunResult::Undecidable && missing);
    if !explained {
        return Some("trace_no_semantic_evidence");
    }
    // Fable #165: the execution phase ends with its S4.2 terminal step: execution_completed
    // for a completed execution, semantic_missing for an undecidable one, and a stopped
    // (failed) execution is only a Fail run whose comparison records no expectation steps.
    let terminal = execution.last().map(|e| e.1);
    let stopped = comparison.len() == 2;
    let ends = match (terminal, result) {
        (Some("execution_completed"), _) => true,
        (Some("semantic_missing"), ScenarioRunResult::Undecidable) => true,
        (Some("semantic_missing"), _) => false,
        (_, ScenarioRunResult::Fail) => stopped,
        _ => false,
    };
    if !ends {
        return Some("trace_execution_incomplete");
    }
    None
}

fn trace_complete(_: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    if inputs.scenario_runs.is_empty() {
        return not_applicable("no current ScenarioRuns");
    }
    let mut verdict = Verdict::default();
    for input in &inputs.scenario_runs {
        let run = &input.run;
        verdict.check(&run.scenario_ref);
        if let Some(reason) = trace_issue(&run.scenario_ref, run.result, &run.trace) {
            verdict.violate(
                &run.scenario_ref,
                Some(format!("{reason}:{}", run.scenario_ref)),
            );
        }
    }
    verdict.conclude(
        "scenario_trace_incomplete",
        "Current ScenarioRun traces must contain ordered business-semantic evidence sufficient to explain their result.",
        "Regenerate the ScenarioRun using the current S4 execution/comparison trace contract.",
    )
}

// ============================================================================ assumptions

/// Sources, in-scope Scenarios, the obligations verifying the sources and the Scenarios'
/// Accepted traceability refs. Nothing further is traversed.
fn coverage_frontier(graph: &Graph, inputs: &F4ValidationInputs) -> BTreeSet<Id> {
    let a = &inputs.applicability;
    let mut frontier: BTreeSet<Id> = a
        .source_refs
        .iter()
        .chain(&a.in_scope_scenario_refs)
        .cloned()
        .collect();
    for source in &a.source_refs {
        frontier.extend(obligations(graph, source).into_keys());
    }
    for id in &a.in_scope_scenario_refs {
        if let Some(s) = scenario(graph, id) {
            frontier.extend(traceability(s).filter(|r| accepted_node(graph, r)).cloned());
        }
    }
    frontier
}

fn assumptions_visible(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let frontier = coverage_frontier(graph, inputs);
    let expected: BTreeSet<Id> = graph
        .node_ids_by_type(NodeType::Assumption)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| n.status == ElementStatus::Accepted)
        .filter_map(|n| match &n.payload {
            NodePayload::Assumption(a) if a.status == "Accepted" => {
                a.finding_ref.as_ref().map(|f| (n.id.clone(), f))
            }
            _ => None,
        })
        .filter(|(_, finding_ref)| match graph.node(finding_ref) {
            Some(f) if f.status == ElementStatus::Accepted => match &f.payload {
                NodePayload::Finding(finding) => {
                    finding.affected_refs.iter().any(|r| frontier.contains(r))
                }
                _ => false,
            },
            _ => false,
        })
        .map(|(id, _)| id)
        .collect();
    let visible: BTreeSet<Id> = inputs.visible_assumption_refs.iter().cloned().collect();
    if expected.is_empty() && visible.is_empty() {
        return not_applicable("no active assumptions reachable from covered obligations");
    }
    let mut evidence: BTreeSet<String> = visible
        .iter()
        .map(|id| format!("visible_assumption:{id}"))
        .collect();
    if expected == visible {
        return output(RuleEvaluation::pass(
            visible.into_iter().collect(),
            evidence.into_iter().collect(),
        ));
    }
    evidence.extend(
        expected
            .iter()
            .map(|id| format!("expected_assumption:{id}")),
    );
    let differing: Vec<Id> = expected.symmetric_difference(&visible).cloned().collect();
    output(RuleEvaluation::violation(
        differing,
        evidence.into_iter().collect(),
        "execution_assumptions_not_visible".to_owned(),
        "F4 gate output must surface exactly the active assumptions reachable from covered verification obligations.".to_owned(),
        Some("Update the F4 visibility material so every reachable active assumption, and no unrelated assumption, is surfaced.".to_owned()),
    ))
}
