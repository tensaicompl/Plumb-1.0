//! The twenty-four F2 evaluators: accepted functional semantics are typed, resolvable and
//! structurally sound (rulebook F2, plan S2.10, Hotfix 044).
//!
//! Every evaluator reads only Accepted PSG semantics, the validated F2 supplemental inputs of
//! the context, existing governed Finding state and the shared plumb-validation kernels
//! (expression scope, calculation, decision-table, process and authorization analysis). Nothing
//! here re-runs inference, matches nodes by name, calls a provider, observes time or mutates the
//! graph, and an inability to evaluate is an evaluator failure, never a pass.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use plumb_expr::{parse, typecheck, Ty};
use plumb_psg::registry::NodeCategory;
use plumb_psg::{
    Edge, ElementStatus, FindingSeverity, Graph, Node, NodePayload, NodeType, RelationKind,
    SeparationConstraintKind,
};

use crate::authorization_analysis::{accepted_role_hierarchy_cycles, accepted_separation_analysis};
use crate::calculation_analysis::{
    analyze_calculation_cycles, CalculationCycleAnalysis, CalculationIssue, CalculationScope,
};
use crate::decision_table_analysis::{analyze_decision_table, DecisionCoverage};
use crate::evaluator::{
    EvaluationError, EvaluatorFailure, EvaluatorRegistry, F2ValidationInputs, RuleEvaluation,
    RuleEvaluator, ValidationContext,
};
use crate::expression_scope::{
    attribute_expression_type, canonical_attribute_owner, validate_expression_bindings,
};
use crate::model::RuleMetadata;
use crate::process_analysis::{analyze_process, ProcessAnalysis, ProcessAnalysisMode};

const ATTRIBUTE_OWNER: &str = "PLUMB.F2.DOMAIN.ATTRIBUTE_OWNER";
const RELATION_TYPED: &str = "PLUMB.F2.DOMAIN.RELATION_TYPED";
const TRANSITION_COMPLETE: &str = "PLUMB.F2.STATE.TRANSITION_COMPLETE";
const REACHABILITY: &str = "PLUMB.F2.STATE.REACHABILITY";
const PERFORMER: &str = "PLUMB.F2.OPERATION.PERFORMER";
const IO_TYPED: &str = "PLUMB.F2.OPERATION.IO_TYPED";
const CALC_TYPECHECK: &str = "PLUMB.F2.CALC.TYPECHECK";
const CALC_NO_CYCLE: &str = "PLUMB.F2.CALC.NO_CYCLE";
const CALENDAR_DEFINED: &str = "PLUMB.F2.TIME.CALENDAR_DEFINED";
const TABLE_NO_OVERLAP: &str = "DMN.F2.TABLE.NO_OVERLAP";
const TABLE_COVERAGE: &str = "DMN.F2.TABLE.COVERAGE";
const PROCESS_START_END: &str = "BPMN.F2.PROCESS.START_END";
const PROCESS_REACHABLE: &str = "BPMN.F2.PROCESS.REACHABLE";
const TASK_RESOLVES: &str = "PLUMB.F2.PROCESS.TASK_RESOLVES";
const GATEWAY_BALANCED: &str = "PLUMB.F2.PROCESS.GATEWAY_BALANCED";
const PAYLOAD_TYPED: &str = "PLUMB.F2.EVENT.PAYLOAD_TYPED";
const EVENT_PRODUCER: &str = "PLUMB.F2.EVENT.PRODUCER";
const EVENT_CONSUMER: &str = "PLUMB.F2.EVENT.CONSUMER";
const PERMISSION_CONCRETE: &str = "RBAC.F2.PERMISSION.CONCRETE";
const HIERARCHY_ACYCLIC: &str = "RBAC.F2.ROLE.HIERARCHY_ACYCLIC";
const ROLE_SEPARATION: &str = "PLUMB.F2.ROLE.SEPARATION";
const SOD_NO_VIOLATION: &str = "PLUMB.F2.SOD.NO_VIOLATION";
const INVARIANT_EXPRESSIBLE: &str = "PLUMB.F2.INVARIANT.EXPRESSIBLE";
const NO_SEMANTIC_BLOCKER: &str = "PLUMB.F2.NO_UNRESOLVED_SEMANTIC_BLOCKER";

const STATE_REACHABILITY_UNREPRESENTABLE: &str = "STATE_REACHABILITY_UNREPRESENTABLE";
const F2_INPUTS_MISSING: &str = "F2_INPUTS_MISSING";
const CALCULATION_ANALYSIS_ERROR: &str = "CALCULATION_ANALYSIS_ERROR";
const DECISION_TABLE_ANALYSIS_UNAVAILABLE: &str = "DECISION_TABLE_ANALYSIS_UNAVAILABLE";
const PROCESS_ANALYSIS_ERROR: &str = "PROCESS_ANALYSIS_ERROR";
const PROCESS_UNSUPPORTED_SEMANTICS: &str = "PROCESS_UNSUPPORTED_SEMANTICS";
const EVENT_SOURCE_CLASSIFICATION_UNAVAILABLE: &str = "EVENT_SOURCE_CLASSIFICATION_UNAVAILABLE";
const EVENT_PURPOSE_CLASSIFICATION_UNAVAILABLE: &str = "EVENT_PURPOSE_CLASSIFICATION_UNAVAILABLE";
const INVARIANT_SCOPE_ERROR: &str = "INVARIANT_SCOPE_ERROR";

/// The four frozen DomainRelationship cardinalities.
const CARDINALITIES: [&str; 4] = ["0..1", "1", "0..*", "1..*"];

/// Binds the twenty-four F2 evaluators. Rule metadata stays in the registry.
pub fn register_f2_evaluators(registry: &mut EvaluatorRegistry) -> Result<(), EvaluationError> {
    let evaluators: [(&str, RuleEvaluator); 24] = [
        (ATTRIBUTE_OWNER, attribute_owner),
        (RELATION_TYPED, relation_typed),
        (TRANSITION_COMPLETE, transition_complete),
        (REACHABILITY, state_reachability),
        (PERFORMER, operation_performer),
        (IO_TYPED, operation_io_typed),
        (CALC_TYPECHECK, calculation_typecheck),
        (CALC_NO_CYCLE, calculation_no_cycle),
        (CALENDAR_DEFINED, calendar_defined),
        (TABLE_NO_OVERLAP, table_no_overlap),
        (TABLE_COVERAGE, table_coverage),
        (PROCESS_START_END, process_start_end),
        (PROCESS_REACHABLE, process_reachable),
        (TASK_RESOLVES, task_resolves),
        (GATEWAY_BALANCED, gateway_balanced),
        (PAYLOAD_TYPED, event_payload_typed),
        (EVENT_PRODUCER, event_producer),
        (EVENT_CONSUMER, event_consumer),
        (PERMISSION_CONCRETE, permission_concrete),
        (HIERARCHY_ACYCLIC, role_hierarchy_acyclic),
        (ROLE_SEPARATION, role_separation),
        (SOD_NO_VIOLATION, sod_no_violation),
        (INVARIANT_EXPRESSIBLE, invariant_expressible),
        (NO_SEMANTIC_BLOCKER, no_unresolved_semantic_blocker),
    ];
    for (rule_id, evaluator) in evaluators {
        registry.register(rule_id, evaluator)?;
    }
    Ok(())
}

// ============================================================================ outcomes

type Outcome = Result<RuleEvaluation, EvaluatorFailure>;

fn failure(code: &str, message: String) -> EvaluatorFailure {
    failure_on(code, message, BTreeSet::new())
}

/// A failure naming the elements that could not be evaluated (sorted and distinct).
fn failure_on(code: &str, message: String, targets: BTreeSet<Id>) -> EvaluatorFailure {
    EvaluatorFailure {
        code: code.to_owned(),
        message,
        targets: targets.into_iter().collect(),
        evidence: Vec::new(),
    }
}

fn output(evaluation: Result<RuleEvaluation, EvaluationError>) -> Outcome {
    evaluation.map_err(|e| failure("F2_OUTPUT", e.to_string()))
}

fn not_applicable(reason: &str) -> Outcome {
    output(RuleEvaluation::not_applicable(reason.to_owned()))
}

/// What a rule found: the checked elements and, for the violating ones, their evidence.
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

    fn violate(&mut self, id: &Id, evidence: String) {
        self.violating.insert(id.clone());
        self.evidence.insert(evidence);
    }

    /// PASS over the checked elements, or one aggregate violation of the violating ones.
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

fn inputs(ctx: &ValidationContext) -> Result<&F2ValidationInputs, EvaluatorFailure> {
    ctx.f2_inputs.as_ref().ok_or_else(|| {
        failure(
            F2_INPUTS_MISSING,
            "F2 validation inputs are required".to_owned(),
        )
    })
}

// ============================================================================ graph access

fn accepted(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
}

/// The Accepted nodes of one type, in ID order.
fn accepted_nodes(graph: &Graph, node_type: NodeType) -> Vec<&Node> {
    graph
        .node_ids_by_type(node_type)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| accepted(n))
        .collect()
}

/// Whether `id` names an Accepted node of one of `types`.
fn accepted_of(graph: &Graph, id: &Id, types: &[NodeType]) -> bool {
    graph
        .node(id)
        .is_some_and(|n| accepted(n) && types.contains(&n.payload.node_type()))
}

/// The Accepted `kind` edges leaving (`outgoing`) or entering `id`, in edge ID order.
fn accepted_edges<'g>(
    graph: &'g Graph,
    id: &Id,
    kind: RelationKind,
    outgoing: bool,
) -> Vec<&'g Edge> {
    let edge_ids = if outgoing {
        graph.outgoing_edge_ids(id)
    } else {
        graph.incoming_edge_ids(id)
    };
    edge_ids
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == kind && e.status == ElementStatus::Accepted)
        .collect()
}

/// Whether an Accepted `kind` edge links `id` with an Accepted node of one of `types`.
fn linked(graph: &Graph, id: &Id, kind: RelationKind, outgoing: bool, types: &[NodeType]) -> bool {
    accepted_edges(graph, id, kind, outgoing)
        .iter()
        .any(|e| accepted_of(graph, if outgoing { &e.to } else { &e.from }, types))
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

/// The snake_case tag of a serde-tagged analysis value, for evidence.
fn tag<T: serde::Serialize>(value: &T, field: &str) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.get(field).and_then(|t| t.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

// ============================================================================ domain and state

fn attribute_owner(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut verdict = Verdict::default();
    for attribute in accepted_nodes(graph, NodeType::Attribute) {
        verdict.check(&attribute.id);
        if canonical_attribute_owner(graph, &attribute.id).is_none() {
            verdict.violate(&attribute.id, format!("owner:{}", attribute.id));
        }
    }
    verdict.conclude(
        "accepted_attribute_owner_not_unique",
        "Accepted attributes must have exactly one owning Accepted entity.",
        "Attach each attribute to exactly one owning entity through an accepted has_attribute relation.",
    )
}

/// The current supplemental findings of `code`: validated F2 analysis material whose affected
/// elements are all still Accepted, by finding ID.
fn supplemental_findings<'i>(
    graph: &Graph,
    inputs: &'i F2ValidationInputs,
    code: &str,
) -> Vec<(&'i Id, &'i [Id])> {
    inputs
        .analysis_findings
        .iter()
        .filter(|f| f.payload.code == code)
        .filter(|f| {
            f.payload
                .affected_refs
                .iter()
                .all(|r| graph.node(r).is_some_and(accepted))
        })
        .map(|f| (&f.id, f.payload.affected_refs.as_slice()))
        .collect()
}

fn add_supplemental(verdict: &mut Verdict, graph: &Graph, inputs: &F2ValidationInputs, code: &str) {
    for (finding, targets) in supplemental_findings(graph, inputs, code) {
        for target in targets {
            verdict.violate(target, finding.to_string());
        }
    }
}

fn relation_typed(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let mut verdict = Verdict::default();
    for node in accepted_nodes(graph, NodeType::DomainRelationship) {
        let NodePayload::DomainRelationship(r) = &node.payload else {
            continue;
        };
        verdict.check(&node.id);
        for (side, endpoint) in [("from", &r.from_entity), ("to", &r.to_entity)] {
            if !accepted_of(graph, endpoint, &[NodeType::Entity]) {
                verdict.violate(&node.id, format!("endpoint:{}:{side}", node.id));
            }
        }
        for (side, cardinality) in [("from", &r.cardinality_from), ("to", &r.cardinality_to)] {
            if !CARDINALITIES.contains(&cardinality.as_str()) {
                verdict.violate(&node.id, format!("cardinality:{}:{side}", node.id));
            }
        }
    }
    add_supplemental(&mut verdict, graph, inputs, RELATION_TYPED);
    verdict.conclude(
        "domain_relationship_untyped",
        "Accepted domain relationships need Accepted entity endpoints and resolved cardinalities.",
        "Resolve the relationship endpoints and cardinalities through governed semantic decisions.",
    )
}

fn transition_complete(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let mut verdict = Verdict::default();
    for node in accepted_nodes(graph, NodeType::Transition) {
        let NodePayload::Transition(t) = &node.payload else {
            continue;
        };
        let id = &node.id;
        verdict.check(id);
        for (side, state) in [("from", &t.from_state), ("to", &t.to_state)] {
            if !accepted_of(graph, state, &[NodeType::State]) {
                verdict.violate(id, format!("state:{id}:{side}"));
            }
        }
        let owner = graph.node(&t.stateful_ref).is_some_and(|n| {
            accepted(n) && NodeCategory::StateOwner.contains(n.payload.node_type())
        });
        if !owner {
            verdict.violate(id, format!("owner:{id}"));
        }
        let triggers = accepted_edges(graph, id, RelationKind::TransitionsVia, true);
        let trigger_valid = match triggers.as_slice() {
            [only] => accepted_of(graph, &only.to, &[NodeType::Operation, NodeType::Event]),
            _ => false,
        };
        if !trigger_valid {
            verdict.violate(id, format!("trigger:{id}"));
        }
    }
    add_supplemental(&mut verdict, graph, inputs, TRANSITION_COMPLETE);
    verdict.conclude(
        "state_transition_incomplete",
        "Accepted transitions need Accepted source and target states, a valid state owner and exactly one Operation/Event trigger.",
        "Provide the missing transition states, owner or the single Operation/Event trigger.",
    )
}

/// The v3 State payload has no initial, terminal or externally-entered markers, so reachability
/// is not representable; no start State is invented.
fn state_reachability(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let states: BTreeSet<Id> = accepted_nodes(graph, NodeType::State)
        .into_iter()
        .map(|n| n.id.clone())
        .collect();
    if states.is_empty() {
        return not_applicable("No accepted State is in scope.");
    }
    Err(failure_on(
        STATE_REACHABILITY_UNREPRESENTABLE,
        "The State payload cannot represent initial, terminal or externally-entered markers."
            .to_owned(),
        states,
    ))
}

// ============================================================================ operations

fn operation_performer(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut verdict = Verdict::default();
    for operation in accepted_nodes(graph, NodeType::Operation) {
        verdict.check(&operation.id);
        let performers = [NodeType::Actor, NodeType::BusinessRole];
        if !linked(
            graph,
            &operation.id,
            RelationKind::PerformedBy,
            true,
            &performers,
        ) {
            verdict.violate(&operation.id, format!("performer:{}", operation.id));
        }
    }
    verdict.conclude(
        "operation_performer_missing",
        "Accepted operations need an Accepted Actor or BusinessRole performer.",
        "Link the operation to its performing actor or business role.",
    )
}

fn operation_io_typed(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut verdict = Verdict::default();
    for node in accepted_nodes(graph, NodeType::Operation) {
        let NodePayload::Operation(op) = &node.payload else {
            continue;
        };
        let id = &node.id;
        verdict.check(id);
        for (field, schema) in [
            ("input_schema_ref", &op.input_schema_ref),
            ("output_schema_ref", &op.output_schema_ref),
        ] {
            if let Some(schema) = schema {
                if !accepted_of(graph, schema, &[NodeType::DataSchema]) {
                    verdict.violate(id, format!("{field}:{id}"));
                }
            }
        }
        for kind in [RelationKind::Reads, RelationKind::Writes] {
            for e in accepted_edges(graph, id, kind, true) {
                if !accepted_of(graph, &e.to, &[NodeType::Entity, NodeType::Attribute]) {
                    verdict.violate(id, format!("{}:{}", e.kind.as_str(), e.id));
                }
            }
        }
    }
    verdict.conclude(
        "operation_io_untyped",
        "Accepted operation schemas, reads and writes must reference Accepted typed domain elements.",
        "Point the operation schemas at Accepted DataSchemas and its reads/writes at Accepted entities or attributes.",
    )
}

// ============================================================================ calculations

/// The relocated calculation kernel over the explicit legacy scope overrides.
fn calculation_analysis(
    graph: &Graph,
    ctx: &ValidationContext,
) -> Result<CalculationCycleAnalysis, EvaluatorFailure> {
    let overrides: BTreeMap<Id, CalculationScope> = inputs(ctx)?
        .calculation_scope_overrides
        .iter()
        .map(|o| (o.calculation_ref.clone(), o.scope.clone()))
        .collect();
    analyze_calculation_cycles(graph, &overrides)
        .map_err(|e| failure(CALCULATION_ANALYSIS_ERROR, e.to_string()))
}

/// Scope issues that validated F2 inputs exclude; meeting one means the kernel could not
/// qualify deterministically.
fn hard_calculation_issue(issue: &CalculationIssue) -> bool {
    matches!(
        issue,
        CalculationIssue::ScopeUnavailable { .. }
            | CalculationIssue::UnexpectedScopeOverride { .. }
    )
}

fn calculation_typecheck(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let analysis = calculation_analysis(graph, ctx)?;
    let mut verdict = Verdict::default();
    let mut hard = BTreeSet::new();
    for q in &analysis.qualifications {
        let id = &q.calculation_ref;
        verdict.check(id);
        if q.qualification.issues.iter().any(hard_calculation_issue) {
            hard.insert(id.clone());
            continue;
        }
        for issue in &q.qualification.issues {
            verdict.violate(id, format!("{}:{id}", tag(issue, "issue")));
        }
        if q.qualification.issues.is_empty() && !q.qualification.is_qualified() {
            hard.insert(id.clone());
        }
    }
    if !hard.is_empty() {
        return Err(failure_on(
            CALCULATION_ANALYSIS_ERROR,
            "Accepted calculations could not be qualified deterministically.".to_owned(),
            hard,
        ));
    }
    verdict.conclude(
        "calculation_not_qualified",
        "Accepted calculations must parse, type-check and match their declared result type, unit and rounding.",
        "Correct the calculation expression, its explicit scope or its declared result type, unit or rounding.",
    )
}

fn calculation_no_cycle(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let analysis = calculation_analysis(graph, ctx)?;
    if !analysis.incomplete.is_empty() {
        return Err(failure_on(
            CALCULATION_ANALYSIS_ERROR,
            "Calculation dependencies could not be completed.".to_owned(),
            analysis.incomplete.iter().cloned().collect(),
        ));
    }
    let mut verdict = Verdict::default();
    for q in &analysis.qualifications {
        verdict.check(&q.calculation_ref);
    }
    for cycle in &analysis.cycles {
        let members = cycle
            .iter()
            .map(Id::to_string)
            .collect::<Vec<_>>()
            .join(",");
        for id in cycle {
            verdict.violate(id, format!("cycle:{members}"));
        }
    }
    verdict.conclude(
        "calculation_dependency_cycle",
        "Accepted calculations must not depend on themselves directly or transitively.",
        "Break the calculation dependency cycle.",
    )
}

fn calendar_defined(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let analysis = calculation_analysis(graph, ctx)?;
    let applicable: Vec<&Id> = analysis
        .qualifications
        .iter()
        .filter(|q| q.qualification.requires_calendar)
        .map(|q| &q.calculation_ref)
        .collect();
    if applicable.is_empty() {
        return not_applicable("No accepted calculation uses working_days.");
    }
    let mut verdict = Verdict::default();
    for id in applicable {
        verdict.check(id);
        let calendar = match graph.node(id).map(|n| &n.payload) {
            Some(NodePayload::Calculation(c)) => c.calendar_ref.as_ref(),
            _ => None,
        };
        let defined = calendar.is_some_and(|c| {
            graph.node(c).is_some_and(|n| {
                accepted(n)
                    && matches!(&n.payload, NodePayload::Calendar(cal) if valid_text(&cal.time_zone))
            })
        });
        if !defined {
            verdict.violate(id, format!("calendar:{id}"));
        }
    }
    verdict.conclude(
        "business_time_calendar_missing",
        "Calculations using working_days need an Accepted Calendar with a valid time zone.",
        "Link the calculation to an Accepted Calendar with an explicit time zone.",
    )
}

// ============================================================================ decision tables

/// The Accepted DecisionTables paired with their validated typed specifications and PSG hit
/// policies.
fn accepted_tables<'g>(
    graph: &'g Graph,
    inputs: &'g F2ValidationInputs,
) -> Result<
    Vec<(
        &'g Id,
        &'g str,
        &'g crate::decision_table_analysis::DecisionTableSpec,
    )>,
    EvaluatorFailure,
> {
    let mut tables = Vec::new();
    for node in accepted_nodes(graph, NodeType::DecisionTable) {
        let NodePayload::DecisionTable(table) = &node.payload else {
            continue;
        };
        let spec = inputs.decision_table(&node.id).ok_or_else(|| {
            failure_on(
                DECISION_TABLE_ANALYSIS_UNAVAILABLE,
                format!("no typed specification for {}", node.id),
                BTreeSet::from([node.id.clone()]),
            )
        })?;
        tables.push((&node.id, table.hit_policy.as_str(), spec));
    }
    Ok(tables)
}

fn table_no_overlap(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    // FIRST tables permit overlap by row order.
    let applicable: Vec<_> = accepted_tables(graph, inputs)?
        .into_iter()
        .filter(|(_, hit_policy, _)| *hit_policy != "FIRST")
        .collect();
    if applicable.is_empty() {
        return not_applicable("No accepted decision table forbids overlapping matches.");
    }
    let mut verdict = Verdict::default();
    let mut unavailable = BTreeSet::new();
    for (id, _, spec) in applicable {
        verdict.check(id);
        let analysis = analyze_decision_table(spec);
        if !analysis.structure.is_empty() {
            unavailable.insert(id.clone());
            continue;
        }
        for o in &analysis.illegal_overlaps {
            verdict.violate(id, format!("overlap:{id}:{}:{}", o.left_row, o.right_row));
        }
    }
    if !unavailable.is_empty() {
        return Err(failure_on(
            DECISION_TABLE_ANALYSIS_UNAVAILABLE,
            "Decision-table structure is unsupported or invalid, so overlap cannot be trusted."
                .to_owned(),
            unavailable,
        ));
    }
    verdict.conclude(
        "decision_table_illegal_overlap",
        "Decision tables whose hit policy forbids overlap have overlapping rows.",
        "Make the overlapping rows mutually exclusive or change the hit policy through a governed decision.",
    )
}

fn table_coverage(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let tables = accepted_tables(graph, inputs)?;
    if tables.is_empty() {
        return not_applicable("No accepted decision table is in scope.");
    }
    let mut verdict = Verdict::default();
    let mut unavailable = BTreeSet::new();
    for (id, _, spec) in tables {
        verdict.check(id);
        match analyze_decision_table(spec).coverage {
            DecisionCoverage::Complete | DecisionCoverage::CompleteByDefault => {}
            DecisionCoverage::Incomplete { witnesses } => {
                verdict.violate(id, format!("uncovered:{id}:{}", witnesses.len()));
            }
            DecisionCoverage::IncompleteNumeric { gaps } => {
                verdict.violate(id, format!("uncovered_numeric:{id}:{}", gaps.len()));
            }
            DecisionCoverage::NotAnalyzable { .. } | DecisionCoverage::StructureInvalid => {
                unavailable.insert(id.clone());
            }
        }
    }
    if !unavailable.is_empty() {
        return Err(failure_on(
            DECISION_TABLE_ANALYSIS_UNAVAILABLE,
            "Decision-table coverage is not analyzable in the pilot subset.".to_owned(),
            unavailable,
        ));
    }
    verdict.conclude(
        "decision_table_domain_uncovered",
        "Accepted decision tables must cover their input domains or declare a default.",
        "Add rows for the uncovered input combinations or an explicit default output.",
    )
}

// ============================================================================ processes

/// The S2.9 AcceptedOnly analysis of every Accepted Process, in ID order.
fn process_analyses(graph: &Graph) -> Result<Vec<ProcessAnalysis>, EvaluatorFailure> {
    accepted_nodes(graph, NodeType::Process)
        .into_iter()
        .map(|p| {
            analyze_process(graph, &p.id, ProcessAnalysisMode::AcceptedOnly).map_err(|e| {
                failure_on(
                    PROCESS_ANALYSIS_ERROR,
                    e.to_string(),
                    BTreeSet::from([p.id.clone()]),
                )
            })
        })
        .collect()
}

/// Analyses whose executable semantics are all supported; any unsupported node makes the
/// rule an ERROR rather than a proven process.
fn supported(analyses: &[ProcessAnalysis]) -> Result<(), EvaluatorFailure> {
    let unsupported: BTreeSet<Id> = analyses
        .iter()
        .filter(|a| !a.unsupported_nodes.is_empty())
        .flat_map(|a| {
            std::iter::once(a.process_ref.clone())
                .chain(a.unsupported_nodes.iter().map(|u| u.node_ref.clone()))
        })
        .collect();
    if unsupported.is_empty() {
        Ok(())
    } else {
        Err(failure_on(
            PROCESS_UNSUPPORTED_SEMANTICS,
            "Accepted processes contain executable semantics outside the pilot subset.".to_owned(),
            unsupported,
        ))
    }
}

/// The supported analyses of the Accepted Processes, or NOT_APPLICABLE without any.
fn process_rule(
    graph: &Graph,
    judge: impl Fn(&ProcessAnalysis, &mut Verdict),
    condition: &str,
    message: &str,
    resolution: &str,
) -> Outcome {
    let analyses = process_analyses(graph)?;
    if analyses.is_empty() {
        return not_applicable("No accepted Process is in scope.");
    }
    supported(&analyses)?;
    let mut verdict = Verdict::default();
    for analysis in &analyses {
        verdict.check(&analysis.process_ref);
        judge(analysis, &mut verdict);
    }
    verdict.conclude(condition, message, resolution)
}

fn process_start_end(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    process_rule(
        graph,
        |a, verdict| {
            let p = &a.process_ref;
            if a.missing_start() {
                verdict.violate(p, format!("missing_start:{p}"));
            }
            if a.missing_end() {
                verdict.violate(p, format!("missing_end:{p}"));
            }
            let entry_or_end: BTreeSet<&Id> = a.start_refs.iter().chain(&a.end_refs).collect();
            for s in &a.node_shape_violations {
                if entry_or_end.contains(&s.node_ref) {
                    verdict.violate(
                        &s.node_ref,
                        format!("shape:{}:{}", s.node_ref, tag(&s.violation, "shape")),
                    );
                }
            }
            for t in &a.invalid_terminal_refs {
                verdict.violate(t, format!("invalid_terminal:{t}"));
            }
            for b in &a.boundary_violations {
                if entry_or_end.contains(&b.from) || entry_or_end.contains(&b.to) {
                    verdict.violate(p, format!("boundary:{}", b.edge_ref));
                }
            }
        },
        "process_entry_or_completion_invalid",
        "Accepted processes need valid start and end semantics on every terminal path.",
        "Add or correct the process start and end nodes and their sequence flows.",
    )
}

fn process_reachable(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    process_rule(
        graph,
        |a, verdict| {
            for n in &a.unreachable_refs {
                verdict.violate(n, format!("unreachable:{n}"));
            }
            for b in &a.boundary_violations {
                verdict.violate(&a.process_ref, format!("boundary:{}", b.edge_ref));
            }
        },
        "process_node_unreachable",
        "Accepted process nodes must be reachable from a start within their process.",
        "Connect the unreachable nodes through sequence flow inside the process.",
    )
}

fn task_resolves(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    process_rule(
        graph,
        |a, verdict| {
            for t in &a.unresolved_task_refs {
                let issue = serde_json::to_value(t.issue)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown".to_owned());
                verdict.violate(&t.node_ref, format!("{issue}:{}", t.node_ref));
            }
        },
        "process_task_unresolved",
        "Accepted process tasks must resolve to an operation or an explicit human responsibility and outcome.",
        "Link each task to its operation, or to its performer and produced outcome.",
    )
}

fn gateway_balanced(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let analyses: Vec<ProcessAnalysis> = process_analyses(graph)?
        .into_iter()
        .filter(|a| {
            !matches!(
                a.parallel_analysis,
                crate::process_analysis::ParallelAnalysis::NotApplicable
            )
        })
        .collect();
    if analyses.is_empty() {
        return not_applicable("No accepted Process contains parallel split/join semantics.");
    }
    supported(&analyses)?;
    let mut verdict = Verdict::default();
    for a in &analyses {
        verdict.check(&a.process_ref);
        for issue in a.parallel_analysis.issues() {
            let kind = serde_json::to_value(issue.kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| "unknown".to_owned());
            let anchor = issue.split_ref.as_ref().unwrap_or(&a.process_ref);
            verdict.violate(&a.process_ref, format!("{kind}:{anchor}"));
            for r in &issue.affected_refs {
                verdict.violate(r, format!("{kind}:{anchor}"));
            }
        }
    }
    verdict.conclude(
        "process_parallel_unbalanced",
        "Parallel branches of accepted processes must join compatibly.",
        "Give each parallel split one matching join reached by all of its branches.",
    )
}

// ============================================================================ events

const EVENT_ACTORS: [NodeType; 2] = [NodeType::Operation, NodeType::ProcessNode];

fn event_payload_typed(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let applicable: Vec<(&Id, &Id)> = accepted_nodes(graph, NodeType::Event)
        .into_iter()
        .filter_map(|n| match &n.payload {
            NodePayload::Event(e) => e.payload_schema_ref.as_ref().map(|s| (&n.id, s)),
            _ => None,
        })
        .collect();
    if applicable.is_empty() {
        return not_applicable("No accepted Event carries a payload schema reference.");
    }
    let mut verdict = Verdict::default();
    for (id, schema) in applicable {
        verdict.check(id);
        let typed = graph
            .node(schema)
            .filter(|n| accepted(n))
            .is_some_and(|n| match n.payload {
                NodePayload::DataSchema(_) => true,
                NodePayload::Attribute(_) => attribute_expression_type(n).is_ok(),
                _ => false,
            });
        if !typed {
            verdict.violate(id, format!("payload:{id}"));
        }
    }
    verdict.conclude(
        "event_payload_untyped",
        "Event payloads must reference an Accepted DataSchema or a typed Accepted Attribute.",
        "Point the event payload at an Accepted DataSchema or an Attribute with a supported value type.",
    )
}

/// Events whose classification the PSG cannot represent are an ERROR, never a FAIL.
fn event_rule(
    graph: &Graph,
    considered: RelationKind,
    proving: RelationKind,
    code: &str,
    not_applicable_reason: &str,
    message: &str,
) -> Outcome {
    let events: Vec<&Id> = accepted_nodes(graph, NodeType::Event)
        .into_iter()
        .filter(|e| linked(graph, &e.id, considered.clone(), false, &EVENT_ACTORS))
        .map(|e| &e.id)
        .collect();
    if events.is_empty() {
        return not_applicable(not_applicable_reason);
    }
    let unproven: BTreeSet<Id> = events
        .iter()
        .filter(|e| !linked(graph, e, proving.clone(), false, &EVENT_ACTORS))
        .map(|e| (*e).clone())
        .collect();
    if !unproven.is_empty() {
        return Err(failure_on(code, message.to_owned(), unproven));
    }
    output(RuleEvaluation::pass(
        events.into_iter().cloned().collect(),
        Vec::new(),
    ))
}

fn event_producer(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    event_rule(
        graph,
        RelationKind::Consumes,
        RelationKind::Produces,
        EVENT_SOURCE_CLASSIFICATION_UNAVAILABLE,
        "No accepted Event is consumed by an accepted Operation or ProcessNode.",
        "Consumed events have no accepted producer and the PSG cannot represent an external source.",
    )
}

fn event_consumer(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    event_rule(
        graph,
        RelationKind::Produces,
        RelationKind::Consumes,
        EVENT_PURPOSE_CLASSIFICATION_UNAVAILABLE,
        "No accepted Event is produced by an accepted Operation or ProcessNode.",
        "Produced events have no accepted consumer and the PSG cannot represent a terminal or audit-only purpose.",
    )
}

// ============================================================================ authorization

fn permission_concrete(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut verdict = Verdict::default();
    for permission in accepted_nodes(graph, NodeType::Permission) {
        let id = &permission.id;
        verdict.check(id);
        for (kind, target) in [
            (RelationKind::Permits, NodeType::Operation),
            (RelationKind::ScopedTo, NodeType::ResourceScope),
        ] {
            let edges = accepted_edges(graph, id, kind.clone(), true);
            let concrete = match edges.as_slice() {
                [only] => accepted_of(graph, &only.to, &[target]),
                _ => false,
            };
            if !concrete {
                verdict.violate(id, format!("{}:{id}", kind.as_str()));
            }
        }
    }
    verdict.conclude(
        "permission_not_concrete",
        "Accepted permissions must permit exactly one Accepted Operation within exactly one Accepted ResourceScope.",
        "Link the permission to its single operation and resource scope.",
    )
}

fn role_hierarchy_acyclic(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut verdict = Verdict::default();
    for role in accepted_nodes(graph, NodeType::SecurityRole) {
        verdict.check(&role.id);
    }
    for cycle in accepted_role_hierarchy_cycles(graph) {
        let members = cycle
            .iter()
            .map(Id::to_string)
            .collect::<Vec<_>>()
            .join(",");
        for id in &cycle {
            verdict.violate(id, format!("cycle:{members}"));
        }
    }
    verdict.conclude(
        "security_role_hierarchy_cycle",
        "The accepted security-role hierarchy contains a cycle.",
        "Remove an inherits_role relation so the hierarchy becomes acyclic.",
    )
}

/// Tagged payloads make dual BusinessRole/SecurityRole typing unrepresentable; the concrete
/// check is that every Accepted role assignment is an explicit Principal/Actor -> SecurityRole
/// edge.
fn role_separation(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut verdict = Verdict::default();
    for edge_id in graph.edge_ids_by_kind(&RelationKind::AssignedRole) {
        let Some(e) = graph
            .edge(edge_id)
            .filter(|e| e.status == ElementStatus::Accepted)
        else {
            continue;
        };
        verdict.check(&e.from);
        let valid = accepted_of(graph, &e.from, &[NodeType::Principal, NodeType::Actor])
            && accepted_of(graph, &e.to, &[NodeType::SecurityRole]);
        if !valid {
            for endpoint in [&e.from, &e.to] {
                if graph.node(endpoint).is_some() {
                    verdict.violate(endpoint, format!("assigned_role:{}", e.id));
                }
            }
        }
    }
    verdict.conclude(
        "role_assignment_not_explicit",
        "Accepted role assignments must run from an Accepted Principal or Actor to an Accepted SecurityRole.",
        "Assign security roles only to principals or actors through explicit assigned_role relations.",
    )
}

fn sod_no_violation(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let constraints: Vec<&Id> = accepted_nodes(graph, NodeType::SeparationConstraint)
        .into_iter()
        .filter(|n| {
            matches!(&n.payload, NodePayload::SeparationConstraint(c)
                if c.constraint_kind == SeparationConstraintKind::StaticSeparationOfDuty)
        })
        .map(|n| &n.id)
        .collect();
    if constraints.is_empty() {
        return not_applicable("No accepted static separation-of-duty constraint is in scope.");
    }
    let mut verdict = Verdict::default();
    for id in constraints {
        verdict.check(id);
    }
    for v in accepted_separation_analysis(graph).static_sod_violations {
        let evidence = format!("sod:{}:{}", v.constraint_ref, v.subject_ref);
        verdict.violate(&v.constraint_ref, evidence.clone());
        verdict.violate(&v.subject_ref, evidence);
    }
    verdict.conclude(
        "static_separation_of_duty_violated",
        "Accepted role assignments grant a combination forbidden by a static separation-of-duty constraint.",
        "Remove one of the conflicting role assignments or record a governed exception.",
    )
}

// ============================================================================ invariants and blockers

fn invariant_expressible(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let mut verdict = Verdict::default();
    for node in accepted_nodes(graph, NodeType::Invariant) {
        let NodePayload::Invariant(invariant) = &node.payload else {
            continue;
        };
        let id = &node.id;
        verdict.check(id);
        let scope_error = |reason: String| {
            failure_on(INVARIANT_SCOPE_ERROR, reason, BTreeSet::from([id.clone()]))
        };
        let bindings = inputs
            .invariant_bindings(id)
            .ok_or_else(|| scope_error(format!("no expression scope for {id}")))?;
        let scope = validate_expression_bindings(graph, bindings, &[])
            .map_err(|issue| scope_error(format!("{issue:?}")))?;
        match parse(&invariant.expression) {
            Err(_) => verdict.violate(id, format!("parse:{id}")),
            Ok(ast) => match typecheck(&ast, &scope.env) {
                Err(_) => verdict.violate(id, format!("type:{id}")),
                Ok(Ty::Bool) => {}
                Ok(_) => verdict.violate(id, format!("not_boolean:{id}")),
            },
        }
    }
    verdict.conclude(
        "invariant_not_expressible",
        "Accepted invariants must parse and type-check as boolean PlumbExpr predicates under their explicit scope.",
        "Correct the invariant expression or its explicit expression scope.",
    )
}

/// Blocker material entering the gate: Accepted Open F2 blocker Finding nodes and supplemental
/// F2 blocker findings. Same-run results are not introspected.
fn no_unresolved_semantic_blocker(
    graph: &Graph,
    ctx: &ValidationContext,
    _: &RuleMetadata,
) -> Outcome {
    let inputs = inputs(ctx)?;
    let mut verdict = Verdict::default();
    for node in accepted_nodes(graph, NodeType::Finding) {
        if let NodePayload::Finding(f) = &node.payload {
            if f.family == "F2"
                && f.severity == FindingSeverity::Blocker
                && f.status == "Open"
                && f.code != NO_SEMANTIC_BLOCKER
            {
                verdict.violate(&node.id, node.id.to_string());
            }
        }
    }
    for f in &inputs.analysis_findings {
        let p = &f.payload;
        if p.family == "F2"
            && p.severity == FindingSeverity::Blocker
            && p.code != NO_SEMANTIC_BLOCKER
        {
            for target in &p.affected_refs {
                verdict.violate(target, f.id.to_string());
            }
        }
    }
    verdict.conclude(
        "functional_semantic_blocker_open",
        "Unresolved blocking functional-semantic findings remain.",
        "Resolve or govern the open F2 blocker findings.",
    )
}
