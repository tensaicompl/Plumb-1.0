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

use crate::calculation_analysis::{
    calculation_origin_of, validate_calculation_bindings, CalculationBindingExposure,
};
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

/// The current S3 decisions: Accepted, with one of the ten S3.3/S3.4 answer kinds.
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

/// The JSON type of one closed marker field.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkerType {
    String,
    Bool,
}

use MarkerType::{Bool as B, String as S};

/// The exact closed field set of each S3.3 marker kind, with each field's JSON type (Hotfix 083:
/// the calculation inclusive-end and holiday-policy markers carry JSON booleans).
fn s3_marker_fields(kind: &str) -> &'static [(&'static str, MarkerType)] {
    match kind {
        "domain_relationship_cardinality" => &[
            ("candidate_ref", S),
            ("cardinality_from", S),
            ("cardinality_to", S),
            ("kind", S),
        ],
        "state_transition_trigger" => &[("candidate_ref", S), ("kind", S), ("trigger_ref", S)],
        "operation_performer" => &[("kind", S), ("operation_ref", S), ("performer_ref", S)],
        "calculation_calendar" => &[("calculation_ref", S), ("calendar_ref", S), ("kind", S)],
        "permission_binding" => &[
            ("kind", S),
            ("operation_ref", S),
            ("permission_ref", S),
            ("resource_scope_ref", S),
        ],
        "invariant_formula" => &[("expression", S), ("invariant_ref", S), ("kind", S)],
        "calculation_fraction" => &[
            ("attribute_ref", S),
            ("calculation_ref", S),
            ("decimal", S),
            ("kind", S),
            ("semantic_key", S),
            ("unit", S),
        ],
        "calculation_inclusive_end" => &[("calculation_ref", S), ("inclusive_end", B), ("kind", S)],
        "calculation_holiday_policy" => &[
            ("calculation_ref", S),
            ("calendar_ref", S),
            ("holiday_consumes_working_day", B),
            ("kind", S),
        ],
        _ => &[],
    }
}

/// Whether the answer is exactly the closed marker of its family: the exact field set, each
/// with its exact JSON type.
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
    let expected = s3_marker_fields(kind);
    expected.len() == object.len()
        && expected.iter().all(|(field, ty)| {
            object.get(*field).is_some_and(|value| match ty {
                MarkerType::String => value.is_string(),
                MarkerType::Bool => value.is_boolean(),
            })
        })
}

fn marker_bool(decision: &ResolutionDecision, field: &str) -> Option<bool> {
    decision.answer.get(field).and_then(Value::as_bool)
}

/// The arguments of the single `working_days` call of `expression`, parsed through PlumbExpr.
fn single_working_days_args(expression: &str) -> Option<Vec<plumb_expr::Ast>> {
    use plumb_expr::Ast;
    fn collect(ast: &Ast, out: &mut Vec<Vec<Ast>>) {
        match ast {
            Ast::Call { function, args } => {
                if function.as_str() == "working_days" {
                    out.push(args.clone());
                }
                args.iter().for_each(|a| collect(a, out));
            }
            Ast::List(items) => items.iter().for_each(|a| collect(a, out)),
            Ast::Unary { expr, .. } => collect(expr, out),
            Ast::Binary { left, right, .. } => {
                collect(left, out);
                collect(right, out);
            }
            Ast::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                collect(condition, out);
                collect(then_expr, out);
                collect(else_expr, out);
            }
            Ast::Literal(_) | Ast::Identifier(_) => {}
        }
    }
    let ast = plumb_expr::parse(expression).ok()?;
    let mut calls = Vec::new();
    collect(&ast, &mut calls);
    match <[Vec<Ast>; 1]>::try_from(calls) {
        Ok([args]) => Some(args),
        Err(_) => None,
    }
}

/// The deterministic postcondition of a current `calculation_fraction` decision (Hotfix 083):
/// the Accepted Calculation's unit, its valid S2.6 root binding of the input, and exactly one
/// governed v1 example with the marker's fraction. Only serialized `examples` material is read.
fn fraction_applied(graph: &Graph, decision: &ResolutionDecision) -> bool {
    let (Some(calculation_ref), Some(attribute_ref), Some(key), Some(decimal), Some(unit)) = (
        marker_id(decision, "calculation_ref"),
        marker_id(decision, "attribute_ref"),
        marker_str(decision, "semantic_key"),
        marker_str(decision, "decimal"),
        marker_str(decision, "unit"),
    ) else {
        return false;
    };
    let Some(node) = accepted_of(graph, &calculation_ref, NodeType::Calculation) else {
        return false;
    };
    let NodePayload::Calculation(calculation) = &node.payload else {
        return false;
    };
    if calculation.unit.as_deref() != Some(unit) {
        return false;
    }
    let Some(origin) = calculation_origin_of(node) else {
        return false;
    };
    if validate_calculation_bindings(graph, &origin.used_bindings).is_err() {
        return false;
    }
    let roots: Vec<&str> = origin
        .used_bindings
        .iter()
        .filter(|b| b.node_ref == attribute_ref && b.exposure == CalculationBindingExposure::Root)
        .map(|b| b.symbol.as_str())
        .collect();
    let [symbol] = roots.as_slice() else {
        return false;
    };
    let governed: Vec<&Value> = calculation
        .examples
        .iter()
        .flatten()
        .filter(|e| e.get("key").and_then(Value::as_str) == Some(key))
        .collect();
    let [example] = governed.as_slice() else {
        return false;
    };
    let input = serde_json::json!([{"symbol": symbol,
                                    "value": {"kind": "decimal", "value": decimal, "unit": null}}]);
    let result = serde_json::json!({"kind": "decimal", "value": decimal, "unit": unit});
    example.get("version") == Some(&Value::from(1))
        && example.get("inputs") == Some(&input)
        && example.get("expected_result") == Some(&result)
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
        // A Question on an Accepted blocker Finding of any family is blocking (Fable #299).
        let blocking = accepted(finding_node) && finding.severity == FindingSeverity::Blocker;
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
        Some("calculation_fraction") => fraction_applied(graph, decision),
        Some("calculation_inclusive_end") => {
            match (id("calculation_ref"), marker_bool(decision, "inclusive_end")) {
                (Some(calculation), Some(inclusive_end)) => {
                    accepted_of(graph, &calculation, NodeType::Calculation).is_some_and(|n| {
                        matches!(&n.payload, NodePayload::Calculation(c)
                            if single_working_days_args(&c.expression).is_some_and(|args| {
                                args.get(2) == Some(&plumb_expr::Ast::Literal(
                                    plumb_expr::Literal::Bool(inclusive_end),
                                ))
                            }))
                    })
                }
                _ => false,
            }
        }
        Some("calculation_holiday_policy") => match (
            id("calculation_ref"),
            id("calendar_ref"),
            marker_bool(decision, "holiday_consumes_working_day"),
        ) {
            // A configured holiday is a non-working day; only `false` is a governed policy.
            (Some(calculation), Some(calendar), Some(false)) => {
                accepted_of(graph, &calendar, NodeType::Calendar).is_some()
                    && accepted_of(graph, &calculation, NodeType::Calculation).is_some_and(|n| {
                        matches!(&n.payload, NodePayload::Calculation(c) if c.calendar_ref.as_ref() == Some(&calendar))
                    })
            }
            _ => false,
        },
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
        // Exactly the profile condition (H0.7): owner and expiry/review date. The profile never
        // marks an assumption permanent, and risk_ref is not part of the condition.
        if !(owner_ok && assumption.expires_at.is_some()) {
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

#[cfg(test)]
mod calculation_decision_tests {
    //! Hotfix 083: the three calculation answer families are current S3 decisions with closed,
    //! typed markers and deterministic PATCH_APPLIED postconditions.

    use std::collections::BTreeMap;

    use plumb_core::{to_canonical_json, Hash, Timestamp};
    use plumb_psg::{
        Agent, Attribute, AuditMeta, Calculation, Calendar, Entity, ExtensionKey, Finding,
        Question, QuestionKind, RelationProperties,
    };
    use serde_json::json;

    use super::*;
    use crate::calculation_analysis::{
        CalculationGroundedRange, CalculationOrigin, CALCULATION_ORIGIN_EXTENSION,
    };
    use crate::evaluator::{F3DecisionArtifactInput, ValidationPolicy};
    use crate::expression_scope::{ExpressionBindingExposure, ExpressionScopeBinding};
    use crate::{load_builtin_software_profile, ValidationRegistry};

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const CALC: &str = "calc:days";
    const KEY: &str = "calculation_fraction:governed";

    fn rid(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn at() -> Timestamp {
        AT.parse().unwrap()
    }

    fn audit() -> AuditMeta {
        AuditMeta::new(rid("agent:analyst"), at(), None, None).unwrap()
    }

    fn node(id: &str, payload: NodePayload) -> Node {
        Node {
            id: rid(id),
            revision: 1,
            status: ElementStatus::Accepted,
            payload,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: Default::default(),
            extensions: Default::default(),
            audit: audit(),
        }
    }

    fn edge(id: &str, kind: RelationKind, from: &str, to: &str) -> Edge {
        Edge {
            id: rid(id),
            revision: 1,
            status: ElementStatus::Accepted,
            kind,
            from: rid(from),
            to: rid(to),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: audit(),
        }
    }

    fn attribute(id: &str, value_type: &str) -> Node {
        node(
            id,
            NodePayload::Attribute(Attribute {
                name: id.into(),
                value_type: value_type.into(),
                nullable: false,
                unit: None,
                precision: None,
                enum_values: None,
                data_classification: None,
            }),
        )
    }

    fn calendar(id: &str) -> Node {
        node(
            id,
            NodePayload::Calendar(Calendar {
                time_zone: "UTC".into(),
                week_pattern: json!(["monday", "friday"]),
                region: None,
                holiday_source: None,
            }),
        )
    }

    fn governed_example(decimal: &str) -> Value {
        json!({"version": 1, "key": KEY,
               "inputs": [{"symbol": "part",
                           "value": {"kind": "decimal", "value": decimal, "unit": null}}],
               "expected_result": {"kind": "decimal", "value": decimal, "unit": "working_day"}})
    }

    /// The applied calculation: inclusive end `true`, the governed fraction example, `cal:test`.
    fn calculation() -> Calculation {
        Calculation {
            name: "Requested days".into(),
            expression: "working_days([begin, finish], calendar, true) * part".into(),
            result_type: "Decimal(2)".into(),
            unit: Some("working_day".into()),
            rounding: None,
            calendar_ref: Some(rid("cal:test")),
            examples: Some(vec![
                json!({"version": 1, "key": "unrelated", "inputs": [], "expected_result": null}),
                governed_example("0.5"),
            ]),
        }
    }

    fn markers() -> BTreeMap<&'static str, Value> {
        BTreeMap::from([
            (
                "calculation_fraction",
                json!({"kind": "calculation_fraction", "calculation_ref": CALC,
                       "attribute_ref": "attr:part", "semantic_key": KEY,
                       "decimal": "0.5", "unit": "working_day"}),
            ),
            (
                "calculation_inclusive_end",
                json!({"kind": "calculation_inclusive_end", "calculation_ref": CALC,
                       "inclusive_end": true}),
            ),
            (
                "calculation_holiday_policy",
                json!({"kind": "calculation_holiday_policy", "calculation_ref": CALC,
                       "calendar_ref": "cal:test", "holiday_consumes_working_day": false}),
            ),
        ])
    }

    struct Fixture {
        graph: Graph,
        inputs: F3ValidationInputs,
        decisions: BTreeMap<&'static str, Id>,
    }

    /// One graph with three current decisions, each with its Answered Question, Closed Finding,
    /// resolves edge and content-addressed artifact.
    fn fixture(calc: Calculation, markers: BTreeMap<&'static str, Value>) -> Fixture {
        let bindings: Vec<ExpressionScopeBinding> = [
            ("begin", "attr:begin"),
            ("finish", "attr:finish"),
            ("part", "attr:part"),
        ]
        .iter()
        .map(|(symbol, node_ref)| ExpressionScopeBinding {
            node_ref: rid(node_ref),
            symbol: (*symbol).into(),
            exposure: ExpressionBindingExposure::Root,
        })
        .collect();
        let mut calc_node = node(CALC, NodePayload::Calculation(calc));
        calc_node.extensions.insert(
            CALCULATION_ORIGIN_EXTENSION
                .parse::<ExtensionKey>()
                .unwrap(),
            serde_json::to_value(CalculationOrigin {
                name_range: CalculationGroundedRange {
                    requirement_ref: rid("req:days"),
                    start: 0,
                    end: 4,
                },
                expression_evidence: Vec::new(),
                used_bindings: bindings,
            })
            .unwrap(),
        );
        let mut nodes = vec![
            node(
                "agent:human",
                NodePayload::Agent(Agent {
                    agent_kind: AgentKind::Human,
                }),
            ),
            node(
                "entity:leave",
                NodePayload::Entity(Entity {
                    name: "Leave".into(),
                    description: None,
                    aggregate_root: None,
                }),
            ),
            attribute("attr:begin", "Date"),
            attribute("attr:finish", "Date"),
            attribute("attr:part", "Decimal(2)"),
            calendar("cal:test"),
            calendar("cal:other"),
            calc_node,
        ];
        let mut edges: Vec<Edge> = ["attr:begin", "attr:finish", "attr:part"]
            .iter()
            .enumerate()
            .map(|(i, attr)| {
                edge(
                    &format!("rel:has{i}"),
                    RelationKind::HasAttribute,
                    "entity:leave",
                    attr,
                )
            })
            .collect();
        let pre = Graph::new(
            rid("project:test"),
            rid("profile:plumb-software-2026.1"),
            nodes.clone(),
            edges.clone(),
        )
        .unwrap();
        let base_semantic_hash = pre.semantic_hash().unwrap();
        let mut artifacts = Vec::new();
        let mut decisions = BTreeMap::new();
        for (i, (kind, marker)) in markers.into_iter().enumerate() {
            let (finding_ref, question_ref, decision_ref) = (
                format!("fnd:{i:016}"),
                format!("q:{i:016}"),
                format!("dec:{i:016}"),
            );
            nodes.push(node(
                &finding_ref,
                NodePayload::Finding(Finding {
                    code: "PLUMB.F2.CALC.TYPECHECK".into(),
                    family: "F2".into(),
                    severity: FindingSeverity::Blocker,
                    message: "unresolved".into(),
                    status: "Closed".into(),
                    affected_refs: vec![rid(CALC)],
                    standard_rule_ref: None,
                    suggested_resolution: None,
                    waiver_ref: None,
                }),
            ));
            nodes.push(node(
                &question_ref,
                NodePayload::Question(Question {
                    finding_ref: rid(&finding_ref),
                    question_kind: QuestionKind::YesNo,
                    prompt: "Decided?".into(),
                    status: "Answered".into(),
                    answer_schema: None,
                    stakeholder_ref: None,
                    priority: None,
                    round_ref: None,
                    context_refs: Some(vec![rid(CALC)]),
                }),
            ));
            let bytes = to_canonical_json(&json!({
                "version": 1, "question_ref": question_ref,
                "base_semantic_hash": base_semantic_hash, "effect_patch": null
            }))
            .unwrap();
            let hash = Hash::content_sha256(&bytes);
            nodes.push(node(
                &decision_ref,
                NodePayload::ResolutionDecision(ResolutionDecision {
                    question_ref: Some(rid(&question_ref)),
                    proposal_ref: None,
                    answer: marker,
                    decided_by: rid("agent:human"),
                    decided_at: at(),
                    patch_ref: hash.clone(),
                    rationale: None,
                    supersedes: None,
                }),
            ));
            edges.push(edge(
                &format!("rel:res{i}"),
                RelationKind::Resolves,
                &decision_ref,
                &question_ref,
            ));
            artifacts.push(F3DecisionArtifactInput {
                decision_ref: rid(&decision_ref),
                hash,
                bytes,
            });
            decisions.insert(kind, rid(&decision_ref));
        }
        let graph = Graph::new(
            rid("project:test"),
            rid("profile:plumb-software-2026.1"),
            nodes,
            edges,
        )
        .unwrap();
        Fixture {
            graph,
            inputs: F3ValidationInputs::new(artifacts, Vec::new()),
            decisions,
        }
    }

    /// PLUMB.F3.DECISION.PATCH_APPLIED over the fixture: the violating decisions (empty = PASS).
    fn violating(fx: &Fixture, evaluator: RuleEvaluator, rule_id: &str) -> Vec<Id> {
        let profile = load_builtin_software_profile().unwrap();
        let registry = ValidationRegistry::new(profile.clone()).unwrap();
        let ctx = ValidationContext::new(
            &fx.graph,
            &registry,
            ValidationPolicy::default(),
            fx.graph.evidence_hash().unwrap(),
            vec![],
            vec![],
        )
        .unwrap()
        .with_f3_inputs(&fx.graph, fx.inputs.clone())
        .unwrap();
        let meta = profile.rules.iter().find(|r| r.id == rule_id).unwrap();
        match evaluator(&fx.graph, &ctx, meta).unwrap() {
            RuleEvaluation::Pass { .. } => Vec::new(),
            RuleEvaluation::Violation { targets, .. } => targets,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn decision<'g>(fx: &'g Fixture, kind: &str) -> (&'g Node, &'g ResolutionDecision) {
        let n = fx.graph.node(&fx.decisions[kind]).unwrap();
        match &n.payload {
            NodePayload::ResolutionDecision(d) => (n, d),
            _ => unreachable!(),
        }
    }

    #[test]
    fn nine_s3_resolution_kinds_are_recognised() {
        assert_eq!(
            S3_RESOLUTION_KINDS,
            [
                "domain_relationship_cardinality",
                "state_transition_trigger",
                "operation_performer",
                "calculation_calendar",
                "permission_binding",
                "invariant_formula",
                "calculation_fraction",
                "calculation_inclusive_end",
                "calculation_holiday_policy",
            ]
        );
        let fx = fixture(calculation(), markers());
        assert_eq!(current_s3_decisions(&fx.graph).len(), 3);
    }

    #[test]
    fn applied_calculation_decisions_pass_patch_applied_and_evidence() {
        let fx = fixture(calculation(), markers());
        for kind in markers().keys() {
            let (n, d) = decision(&fx, kind);
            assert!(closed_marker(d), "{kind}");
            assert!(postcondition(&fx.graph, n, d), "{kind}");
        }
        assert_eq!(
            violating(&fx, patch_applied, PATCH_APPLIED),
            Vec::<Id>::new()
        );
        // No rationale is required for an initial calculation decision.
        assert_eq!(
            violating(&fx, decision_evidence, DECISION_EVIDENCE),
            Vec::<Id>::new()
        );
    }

    #[test]
    fn calculation_markers_are_closed_and_typed() {
        for (kind, marker) in markers() {
            let object = marker.as_object().unwrap().clone();
            let check = |answer: Value| {
                closed_marker(&ResolutionDecision {
                    question_ref: None,
                    proposal_ref: None,
                    answer,
                    decided_by: rid("agent:human"),
                    decided_at: at(),
                    patch_ref: Hash::content_sha256(b"x"),
                    rationale: None,
                    supersedes: None,
                })
            };
            assert!(check(marker.clone()), "{kind}");
            for field in object.keys().filter(|f| *f != "kind") {
                let mut missing = object.clone();
                missing.remove(field);
                assert!(!check(Value::Object(missing)), "{kind} without {field}");
                // Wrong JSON type: a string becomes a boolean and a boolean a string.
                let mut wrong = object.clone();
                let flipped = match &object[field] {
                    Value::Bool(b) => Value::String(b.to_string()),
                    _ => Value::Bool(true),
                };
                wrong.insert(field.clone(), flipped);
                assert!(!check(Value::Object(wrong)), "{kind} {field} retyped");
            }
            let mut extra = object.clone();
            extra.insert("note".into(), json!("extra"));
            assert!(!check(Value::Object(extra)), "{kind} with an extra field");
        }
    }

    #[test]
    fn calculation_decisions_not_reflected_in_the_graph_fail_patch_applied() {
        // The governed fraction example changed.
        let mut calc = calculation();
        calc.examples = Some(vec![governed_example("0.25")]);
        let fx = fixture(calc, markers());
        assert_eq!(
            violating(&fx, patch_applied, PATCH_APPLIED),
            vec![fx.decisions["calculation_fraction"].clone()]
        );
        // A duplicate governed example is not exactly one.
        let mut calc = calculation();
        calc.examples = Some(vec![governed_example("0.5"), governed_example("0.5")]);
        let fx = fixture(calc, markers());
        assert_eq!(
            violating(&fx, patch_applied, PATCH_APPLIED),
            vec![fx.decisions["calculation_fraction"].clone()]
        );
        // The working_days argument no longer carries the governed boolean.
        let mut calc = calculation();
        calc.expression = "working_days([begin, finish], calendar, false) * part".into();
        let fx = fixture(calc, markers());
        assert_eq!(
            violating(&fx, patch_applied, PATCH_APPLIED),
            vec![fx.decisions["calculation_inclusive_end"].clone()]
        );
        // The calculation uses another calendar.
        let mut calc = calculation();
        calc.calendar_ref = Some(rid("cal:other"));
        let fx = fixture(calc, markers());
        assert_eq!(
            violating(&fx, patch_applied, PATCH_APPLIED),
            vec![fx.decisions["calculation_holiday_policy"].clone()]
        );
        // A holiday that consumes a working day can never be a governed postcondition.
        let mut consuming = markers();
        consuming.insert(
            "calculation_holiday_policy",
            json!({"kind": "calculation_holiday_policy", "calculation_ref": CALC,
                   "calendar_ref": "cal:test", "holiday_consumes_working_day": true}),
        );
        let fx = fixture(calculation(), consuming);
        let (n, d) = decision(&fx, "calculation_holiday_policy");
        assert!(closed_marker(d));
        assert!(!postcondition(&fx.graph, n, d));
        assert_eq!(
            violating(&fx, patch_applied, PATCH_APPLIED),
            vec![fx.decisions["calculation_holiday_policy"].clone()]
        );
    }
}
