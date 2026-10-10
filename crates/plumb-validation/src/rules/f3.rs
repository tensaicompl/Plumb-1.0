//! The six F3 evaluators: blocking ambiguities and semantic decisions are governed and resolved
//! (rulebook F3, plan S3.5, Hotfix 049).
//!
//! Every evaluator reads only Accepted PSG governance material and the validated F3 supplemental
//! inputs (decision artifacts and the S3.4 assumption-expiry findings). Nothing here reads time,
//! replays a patch, reaches an artifact store, mutates the graph or re-evaluates waiver policy;
//! an inability to evaluate is an evaluator failure, never a pass.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{HashKind, Id};
use plumb_psg::{
    AgentKind, Edge, ElementStatus, FindingSeverity, Graph, Node, NodePayload, NodeType,
    RelationKind, ResolutionDecision,
};
use serde_json::Value;

use crate::evaluator::{
    EvaluationError, EvaluatorFailure, EvaluatorRegistry, F3ValidationInputs, RuleEvaluation,
    RuleEvaluator, ValidationContext, S3_RESOLUTION_KINDS,
};
use crate::finding::{finding_id, is_clean_text};
use crate::model::RuleMetadata;
use crate::waiver::{WaiverDecisionMarker, WAIVER_KIND};

const NO_BLOCKING_OPEN: &str = "PLUMB.F3.QUESTION.NO_BLOCKING_OPEN";
const PATCH_APPLIED: &str = "PLUMB.F3.DECISION.PATCH_APPLIED";
const DECISION_EVIDENCE: &str = "PLUMB.F3.DECISION.EVIDENCE";
const ASSUMPTION_OWNER: &str = "PLUMB.F3.ASSUMPTION.OWNER";
const ASSUMPTION_NOT_EXPIRED: &str = "PLUMB.F3.ASSUMPTION.NOT_EXPIRED";
const WAIVER_DECISION: &str = "PLUMB.F3.WAIVER.DECISION";

const F3_INPUTS_MISSING: &str = "F3_INPUTS_MISSING";
const F3_QUESTION_STATUS: &str = "F3_QUESTION_STATUS";
const F3_QUESTION_LINK: &str = "F3_QUESTION_LINK";

const QUESTION_STATUSES: [&str; 4] = ["Open", "Answered", "Closed", "Superseded"];
const NO_BLOCKING_ASSUMPTION: &str = "No active assumptions are linked to blocking findings.";

/// Binds the six F3 evaluators. Rule metadata stays in the registry.
pub fn register_f3_evaluators(registry: &mut EvaluatorRegistry) -> Result<(), EvaluationError> {
    let evaluators: [(&str, RuleEvaluator); 6] = [
        (NO_BLOCKING_OPEN, no_blocking_open),
        (PATCH_APPLIED, patch_applied),
        (DECISION_EVIDENCE, decision_evidence),
        (ASSUMPTION_OWNER, assumption_owner),
        (ASSUMPTION_NOT_EXPIRED, assumption_not_expired),
        (WAIVER_DECISION, waiver_decision),
    ];
    for (rule_id, evaluator) in evaluators {
        registry.register(rule_id, evaluator)?;
    }
    Ok(())
}

// ============================================================================ outcomes

type Outcome = Result<RuleEvaluation, EvaluatorFailure>;

fn failure(code: &str, message: String, targets: BTreeSet<Id>) -> EvaluatorFailure {
    EvaluatorFailure {
        code: code.to_owned(),
        message,
        targets: targets.into_iter().collect(),
        evidence: Vec::new(),
    }
}

fn output(evaluation: Result<RuleEvaluation, EvaluationError>) -> Outcome {
    evaluation.map_err(|e| failure("F3_OUTPUT", e.to_string(), BTreeSet::new()))
}

/// PASS over the checked elements, or one aggregate violation of the violating ones.
fn conclude(
    checked: BTreeSet<Id>,
    violating: BTreeSet<Id>,
    condition: &str,
    message: &str,
    resolution: &str,
) -> Outcome {
    output(if violating.is_empty() {
        RuleEvaluation::pass(checked.into_iter().collect(), Vec::new())
    } else {
        RuleEvaluation::violation(
            violating.into_iter().collect(),
            Vec::new(),
            condition.to_owned(),
            message.to_owned(),
            Some(resolution.to_owned()),
        )
    })
}

fn inputs(ctx: &ValidationContext) -> Result<&F3ValidationInputs, EvaluatorFailure> {
    ctx.f3_inputs.as_ref().ok_or_else(|| {
        failure(
            F3_INPUTS_MISSING,
            "F3 validation inputs are required".to_owned(),
            BTreeSet::new(),
        )
    })
}

// ============================================================================ graph access

fn accepted(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
}

fn accepted_nodes(graph: &Graph, node_type: NodeType) -> Vec<&Node> {
    graph
        .node_ids_by_type(node_type)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| accepted(n))
        .collect()
}

fn accepted_of<'g>(graph: &'g Graph, id: &Id, node_type: NodeType) -> Option<&'g Node> {
    graph
        .node(id)
        .filter(|n| accepted(n) && n.payload.node_type() == node_type)
}

fn human(graph: &Graph, id: &Id) -> bool {
    graph.node(id).is_some_and(|n| {
        accepted(n)
            && matches!(&n.payload, NodePayload::Agent(a) if a.agent_kind == AgentKind::Human)
    })
}

/// Outgoing edges of `kind` from `id` with a status accepted by `keep`.
fn outgoing<'g>(
    graph: &'g Graph,
    id: &Id,
    kind: &RelationKind,
    keep: impl Fn(ElementStatus) -> bool,
) -> Vec<&'g Edge> {
    graph
        .outgoing_edge_ids(id)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| &e.kind == kind && keep(e.status))
        .collect()
}

fn accepted_edge_to(graph: &Graph, from: &Id, kind: &RelationKind, to: &Id) -> bool {
    outgoing(graph, from, kind, |s| s == ElementStatus::Accepted)
        .iter()
        .any(|e| &e.to == to)
}

fn marker_kind(decision: &ResolutionDecision) -> Option<&str> {
    decision.answer.get("kind").and_then(Value::as_str)
}

/// The marker field `field` as an ID.
fn marker_id(decision: &ResolutionDecision, field: &str) -> Option<Id> {
    decision
        .answer
        .get(field)
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
}

fn marker_str<'d>(decision: &'d ResolutionDecision, field: &str) -> Option<&'d str> {
    decision.answer.get(field).and_then(Value::as_str)
}

/// The current S3 decisions: Accepted, with one of the seven S3.3/S3.4 answer kinds.
fn current_s3_decisions(graph: &Graph) -> Vec<(&Node, &ResolutionDecision)> {
    accepted_nodes(graph, NodeType::ResolutionDecision)
        .into_iter()
        .filter_map(|n| match &n.payload {
            NodePayload::ResolutionDecision(d)
                if marker_kind(d)
                    .is_some_and(|k| S3_RESOLUTION_KINDS.contains(&k) || k == WAIVER_KIND) =>
            {
                Some((n, d))
            }
            _ => None,
        })
        .collect()
}

/// Whether the decision has the Accepted resolves edge of its family: S3.3 to its Question or
/// that Question's Finding; waiver to its marker's Finding.
fn governance_edge(graph: &Graph, node: &Node, decision: &ResolutionDecision) -> bool {
    let to = |target: &Id| accepted_edge_to(graph, &node.id, &RelationKind::Resolves, target);
    if marker_kind(decision) == Some(WAIVER_KIND) {
        return marker_id(decision, "finding_ref").is_some_and(|f| to(&f));
    }
    let Some(question_ref) = &decision.question_ref else {
        return false;
    };
    let finding_ref = match graph.node(question_ref).map(|n| &n.payload) {
        Some(NodePayload::Question(q)) => Some(&q.finding_ref),
        _ => None,
    };
    to(question_ref) || finding_ref.is_some_and(to)
}

/// The exact closed field set of each S3.3 marker kind.
fn s3_marker_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "domain_relationship_cardinality" => &[
            "candidate_ref",
            "cardinality_from",
            "cardinality_to",
            "kind",
        ],
        "state_transition_trigger" => &["candidate_ref", "kind", "trigger_ref"],
        "operation_performer" => &["kind", "operation_ref", "performer_ref"],
        "calculation_calendar" => &["calculation_ref", "calendar_ref", "kind"],
        "permission_binding" => &[
            "kind",
            "operation_ref",
            "permission_ref",
            "resource_scope_ref",
        ],
        "invariant_formula" => &["expression", "invariant_ref", "kind"],
        _ => &[],
    }
}

/// Whether the answer is exactly the closed marker of its family.
fn closed_marker(decision: &ResolutionDecision) -> bool {
    let Some(kind) = marker_kind(decision) else {
        return false;
    };
    if kind == WAIVER_KIND {
        return serde_json::from_value::<WaiverDecisionMarker>(decision.answer.clone()).is_ok();
    }
    let Some(object) = decision.answer.as_object() else {
        return false;
    };
    let fields: Vec<&str> = object.keys().map(String::as_str).collect();
    fields == s3_marker_fields(kind) && object.values().all(Value::is_string)
}

// ============================================================================ questions

fn no_blocking_open(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    inputs(ctx)?;
    let mut applicable = BTreeSet::new();
    let mut open = BTreeSet::new();
    for node in accepted_nodes(graph, NodeType::Question) {
        let NodePayload::Question(question) = &node.payload else {
            continue;
        };
        // A missing or non-Accepted Finding leaves the Question outside the applicable set; a
        // node of another type at finding_ref is a broken link.
        let finding = match graph.node(&question.finding_ref) {
            None => continue,
            Some(n) => match &n.payload {
                NodePayload::Finding(f) => (n, f),
                _ => {
                    return Err(failure(
                        F3_QUESTION_LINK,
                        format!(
                            "question {} links non-finding {}",
                            node.id, question.finding_ref
                        ),
                        BTreeSet::from([node.id.clone()]),
                    ))
                }
            },
        };
        let (finding_node, finding) = finding;
        let blocking = accepted(finding_node)
            && finding.family == "F2"
            && finding.severity == FindingSeverity::Blocker;
        if !blocking {
            continue;
        }
        if !QUESTION_STATUSES.contains(&question.status.as_str()) {
            return Err(failure(
                F3_QUESTION_STATUS,
                format!("question {} has status {:?}", node.id, question.status),
                BTreeSet::from([node.id.clone()]),
            ));
        }
        applicable.insert(node.id.clone());
        if question.status == "Open" {
            open.insert(node.id.clone());
        }
    }
    conclude(
        applicable,
        open,
        "blocking_functional_question_open",
        "Blocking functional questions remain open.",
        "Answer, close or supersede the blocking questions through governed resolution.",
    )
}

// ============================================================================ decisions

/// Whether the current graph shows the deterministic postcondition of a current S3 decision.
fn postcondition(graph: &Graph, node: &Node, decision: &ResolutionDecision) -> bool {
    let id = |field: &str| marker_id(decision, field);
    match marker_kind(decision) {
        Some("domain_relationship_cardinality") => id("candidate_ref").is_some_and(|candidate| {
            accepted_of(graph, &candidate, NodeType::DomainRelationship).is_some_and(|n| {
                matches!(&n.payload, NodePayload::DomainRelationship(r)
                    if Some(r.cardinality_from.as_str()) == marker_str(decision, "cardinality_from")
                        && Some(r.cardinality_to.as_str()) == marker_str(decision, "cardinality_to"))
            })
        }),
        Some("state_transition_trigger") => match (id("candidate_ref"), id("trigger_ref")) {
            (Some(candidate), Some(trigger)) => {
                accepted_of(graph, &candidate, NodeType::Transition).is_some()
                    && matches!(
                        outgoing(graph, &candidate, &RelationKind::TransitionsVia, |s| s == ElementStatus::Accepted).as_slice(),
                        [only] if only.to == trigger
                    )
            }
            _ => false,
        },
        Some("operation_performer") => match (id("operation_ref"), id("performer_ref")) {
            (Some(operation), Some(performer)) => {
                accepted_of(graph, &operation, NodeType::Operation).is_some()
                    && accepted_edge_to(graph, &operation, &RelationKind::PerformedBy, &performer)
            }
            _ => false,
        },
        Some("calculation_calendar") => match (id("calculation_ref"), id("calendar_ref")) {
            (Some(calculation), Some(calendar)) => {
                accepted_of(graph, &calendar, NodeType::Calendar).is_some()
                    && accepted_of(graph, &calculation, NodeType::Calculation).is_some_and(|n| {
                        matches!(&n.payload, NodePayload::Calculation(c) if c.calendar_ref.as_ref() == Some(&calendar))
                    })
            }
            _ => false,
        },
        Some("permission_binding") => {
            match (id("permission_ref"), id("operation_ref"), id("resource_scope_ref")) {
                (Some(permission), Some(operation), Some(scope)) => {
                    let unique = |kind: RelationKind, target: &Id| {
                        matches!(
                            outgoing(graph, &permission, &kind, plumb_psg::is_baseline).as_slice(),
                            [only] if only.status == ElementStatus::Accepted && &only.to == target
                        )
                    };
                    accepted_of(graph, &permission, NodeType::Permission).is_some()
                        && unique(RelationKind::Permits, &operation)
                        && unique(RelationKind::ScopedTo, &scope)
                }
                _ => false,
            }
        }
        Some("invariant_formula") => id("invariant_ref").is_some_and(|invariant| {
            accepted_of(graph, &invariant, NodeType::Invariant).is_some_and(|n| {
                matches!(&n.payload, NodePayload::Invariant(i)
                    if Some(i.expression.as_str()) == marker_str(decision, "expression"))
            })
        }),
        Some(kind) if kind == WAIVER_KIND => id("finding_ref").is_some_and(|finding| {
            accepted_edge_to(graph, &node.id, &RelationKind::Resolves, &finding)
        }),
        _ => false,
    }
}

fn patch_applied(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let mut checked = BTreeSet::new();
    let mut violating = BTreeSet::new();
    for (node, decision) in current_s3_decisions(graph) {
        checked.insert(node.id.clone());
        let applied = inputs.decision_artifact(&node.id).is_some()
            && governance_edge(graph, node, decision)
            && postcondition(graph, node, decision);
        if !applied {
            violating.insert(node.id.clone());
        }
    }
    conclude(
        checked,
        violating,
        "accepted_resolution_decision_not_applied",
        "Accepted resolution decisions are not fully reflected in the current graph.",
        "Reapply or supersede the governed decision so the current graph reflects its accepted effect.",
    )
}

fn decision_evidence(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let mut checked = BTreeSet::new();
    let mut violating = BTreeSet::new();
    for (node, decision) in current_s3_decisions(graph) {
        checked.insert(node.id.clone());
        let kind = marker_kind(decision).unwrap_or_default();
        let rationale_required = matches!(
            kind,
            "domain_relationship_cardinality" | "state_transition_trigger"
        ) || kind == WAIVER_KIND
            || decision.supersedes.is_some();
        let rationale_ok = match decision.rationale.as_deref() {
            Some(text) => is_clean_text(text),
            None => !rationale_required,
        };
        let valid = human(graph, &decision.decided_by)
            && closed_marker(decision)
            && inputs.decision_artifact(&node.id).is_some()
            && governance_edge(graph, node, decision)
            && rationale_ok;
        if !valid {
            violating.insert(node.id.clone());
        }
    }
    conclude(
        checked,
        violating,
        "material_resolution_decision_governance_incomplete",
        "Material resolution decisions lack required governance or provenance.",
        "Repair or supersede the decision with the required human actor, artifact, target and rationale.",
    )
}

// ============================================================================ assumptions

/// Active Accepted assumptions linked to an Accepted blocker Finding.
fn blocking_assumptions(graph: &Graph) -> Vec<(&Node, &plumb_psg::Assumption)> {
    accepted_nodes(graph, NodeType::Assumption)
        .into_iter()
        .filter_map(|n| match &n.payload {
            NodePayload::Assumption(a) if a.status == "Accepted" => Some((n, a)),
            _ => None,
        })
        .filter(|(_, a)| {
            a.finding_ref.as_ref().is_some_and(|f| {
                accepted_of(graph, f, NodeType::Finding).is_some_and(|n| {
                    matches!(&n.payload, NodePayload::Finding(f) if f.severity == FindingSeverity::Blocker)
                })
            })
        })
        .collect()
}

fn not_applicable(reason: &str) -> Outcome {
    output(RuleEvaluation::not_applicable(reason.to_owned()))
}

fn assumption_owner(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    inputs(ctx)?;
    let scope = blocking_assumptions(graph);
    if scope.is_empty() {
        return not_applicable(NO_BLOCKING_ASSUMPTION);
    }
    let mut checked = BTreeSet::new();
    let mut violating = BTreeSet::new();
    for (node, assumption) in scope {
        checked.insert(node.id.clone());
        let owner_ok = graph.node(&assumption.owner_ref).is_some_and(|o| {
            accepted(o)
                && matches!(
                    o.payload.node_type(),
                    NodeType::Agent | NodeType::Stakeholder
                )
        });
        if !(owner_ok && assumption.expires_at.is_some() && assumption.risk_ref.is_none()) {
            violating.insert(node.id.clone());
        }
    }
    conclude(
        checked,
        violating,
        "blocking_assumption_governance_incomplete",
        "Blocking assumptions require an accepted owner and explicit expiry.",
        "Assign an accepted owner and expiry, or resolve/supersede the assumption.",
    )
}

fn assumption_not_expired(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let scope: BTreeSet<Id> = blocking_assumptions(graph)
        .into_iter()
        .map(|(n, _)| n.id.clone())
        .collect();
    if scope.is_empty() {
        return not_applicable(NO_BLOCKING_ASSUMPTION);
    }
    let expired: BTreeSet<Id> = inputs
        .assumption_expiry_findings
        .iter()
        .flat_map(|f| f.payload.affected_refs.iter())
        .filter(|r| scope.contains(*r))
        .cloned()
        .collect();
    conclude(
        scope,
        expired,
        "active_blocking_assumption_expired",
        "Active blocking assumptions have expired.",
        "Review, resolve, supersede or explicitly replace the expired assumptions.",
    )
}

// ============================================================================ waivers

fn waiver_decision(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    // Every Accepted waiver decision with its Accepted resolved Findings.
    let mut waivers: Vec<(&Node, &ResolutionDecision, Vec<Id>)> = Vec::new();
    let mut governing: BTreeMap<Id, BTreeSet<Id>> = BTreeMap::new();
    for node in accepted_nodes(graph, NodeType::ResolutionDecision) {
        let NodePayload::ResolutionDecision(decision) = &node.payload else {
            continue;
        };
        if marker_kind(decision) != Some(WAIVER_KIND) {
            continue;
        }
        let targets: Vec<Id> = outgoing(graph, &node.id, &RelationKind::Resolves, |s| {
            s == ElementStatus::Accepted
        })
        .iter()
        .filter(|e| accepted_of(graph, &e.to, NodeType::Finding).is_some())
        .map(|e| e.to.clone())
        .collect();
        for t in &targets {
            governing
                .entry(t.clone())
                .or_default()
                .insert(node.id.clone());
        }
        waivers.push((node, decision, targets));
    }
    let severe = |finding: &Id| {
        graph.node(finding).is_some_and(|n| {
            matches!(&n.payload, NodePayload::Finding(f)
                if matches!(f.severity, FindingSeverity::Blocker | FindingSeverity::Error))
        })
    };
    let mut checked = BTreeSet::new();
    let mut violating = BTreeSet::new();
    for (node, decision, targets) in &waivers {
        if !targets.iter().any(severe) {
            continue;
        }
        checked.insert(node.id.clone());
        let marker = serde_json::from_value::<WaiverDecisionMarker>(decision.answer.clone()).ok();
        let valid = marker.as_ref().is_some_and(|m| {
            let finding = graph
                .node(&m.finding_ref)
                .filter(|n| accepted(n))
                .and_then(|n| match &n.payload {
                    NodePayload::Finding(f) => Some(f),
                    _ => None,
                });
            targets.contains(&m.finding_ref)
                && finding.is_some_and(|f| f.code == m.rule_id)
                && m.finding_key.kind() == HashKind::Generic
                && finding_id(&m.finding_key).is_ok_and(|id| id == m.finding_ref)
                && governing
                    .get(&m.finding_ref)
                    .is_some_and(|ds| ds.len() == 1)
        }) && human(graph, &decision.decided_by)
            && decision.rationale.as_deref().is_some_and(is_clean_text)
            && inputs.decision_artifact(&node.id).is_some();
        if !valid {
            violating.insert(node.id.clone());
        }
    }
    if checked.is_empty() {
        return not_applicable("No active blocker/error waiver decisions exist.");
    }
    conclude(
        checked,
        violating,
        "blocking_waiver_governance_invalid",
        "Blocking or error waivers lack valid explicit governance.",
        "Replace the waiver with a valid governed ResolutionDecision for the exact finding.",
    )
}
