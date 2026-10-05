//! The eleven F1 evaluators: requirements are grounded, interpretable and free of unresolved
//! blocking quality defects (rulebook F1, plan S1.6).
//!
//! Every evaluator reads only the graph and the validated F1 supplemental inputs of the
//! context. Duplicate, contradiction and vocabulary conditions are read as current finding
//! state and supplied dependencies; requirement quality reuses plumb-lint. Nothing here
//! re-runs upstream analysis, calls a provider or observes time.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use plumb_lint::{lint_requirement, LintSeverity};
use plumb_psg::{
    ElementStatus, Graph, Modality, Node, NodePayload, NodeType, RelationKind, Requirement,
    RequirementKind, RequirementLevel,
};

use crate::evaluator::{
    governed_markers, EvaluationError, EvaluatorFailure, EvaluatorRegistry, F1ValidationInputs,
    F1VocabularyResolution, Marker, RuleEvaluation, RuleEvaluator, ValidationContext,
};
use crate::model::RuleMetadata;

const STATEMENT_PRESENT: &str = "ISO29148.F1.REQ.STATEMENT_PRESENT";
const GROUNDED: &str = "PLUMB.F1.REQ.GROUNDED";
const LINEAGE: &str = "ISO29148.F1.REQ.LINEAGE";
const TYPE_KNOWN: &str = "PLUMB.F1.REQ.TYPE_KNOWN";
const NO_DUPLICATE: &str = "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED";
const NO_CONTRADICTION: &str = "PLUMB.F1.REQ.NO_CONTRADICTION";
const TERMS_RESOLVED: &str = "PLUMB.F1.REQ.TERMS_RESOLVED";
const MODALITY_EXPLICIT: &str = "PLUMB.F1.REQ.MODALITY_EXPLICIT";
const CRITERIA: &str = "PLUMB.F1.REQ.CRITERIA_FOR_BEHAVIOR";
const QUALITY: &str = "PLUMB.F1.REQ.QUALITY_FINDINGS_CLEAR";
const SUPERSEDED: &str = "PLUMB.F1.REQ.SUPERSEDED_EXCLUDED";

/// Binds the eleven F1 evaluators. Rule metadata stays in the registry.
pub fn register_f1_evaluators(registry: &mut EvaluatorRegistry) -> Result<(), EvaluationError> {
    let evaluators: [(&str, RuleEvaluator); 11] = [
        (STATEMENT_PRESENT, statement_present),
        (GROUNDED, grounded),
        (LINEAGE, lineage),
        (TYPE_KNOWN, type_known),
        (NO_DUPLICATE, no_duplicate),
        (NO_CONTRADICTION, no_contradiction),
        (TERMS_RESOLVED, terms_resolved),
        (MODALITY_EXPLICIT, modality_explicit),
        (CRITERIA, criteria_for_behavior),
        (QUALITY, quality_findings_clear),
        (SUPERSEDED, superseded_excluded),
    ];
    for (rule_id, evaluator) in evaluators {
        registry.register(rule_id, evaluator)?;
    }
    Ok(())
}

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
    evaluation.map_err(|e| failure("F1_OUTPUT", e.to_string()))
}

/// PASS over the checked requirements, or one aggregate violation of the violating ones.
fn aggregate(
    checked: Vec<Id>,
    violating: BTreeSet<Id>,
    evidence: BTreeSet<String>,
    condition: &str,
    message: &str,
    resolution: &str,
) -> Outcome {
    output(if violating.is_empty() {
        RuleEvaluation::pass(checked, Vec::new())
    } else {
        RuleEvaluation::violation(
            violating.into_iter().collect(),
            evidence.into_iter().collect(),
            condition.to_owned(),
            message.to_owned(),
            Some(resolution.to_owned()),
        )
    })
}

fn inputs(ctx: &ValidationContext) -> Result<&F1ValidationInputs, EvaluatorFailure> {
    ctx.f1_inputs.as_ref().ok_or_else(|| {
        failure(
            "F1_INPUTS_MISSING",
            "F1 validation inputs are required".to_owned(),
        )
    })
}

// ============================================================================ graph access

fn accepted(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
}

/// The Accepted Requirements, in ID order.
fn accepted_requirements(graph: &Graph) -> Vec<(&Node, &Requirement)> {
    graph
        .node_ids_by_type(NodeType::Requirement)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|node| accepted(node))
        .filter_map(|node| match &node.payload {
            NodePayload::Requirement(r) => Some((node, r)),
            _ => None,
        })
        .collect()
}

fn ids(requirements: &[(&Node, &Requirement)]) -> Vec<Id> {
    requirements.iter().map(|(n, _)| n.id.clone()).collect()
}

/// The Accepted nodes reached from `id` by Accepted `kind` edges in the given direction.
fn neighbours<'g>(graph: &'g Graph, id: &Id, kind: &RelationKind, outgoing: bool) -> Vec<&'g Node> {
    let edge_ids = if outgoing {
        graph.outgoing_edge_ids(id)
    } else {
        graph.incoming_edge_ids(id)
    };
    edge_ids
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| &e.kind == kind && e.status == ElementStatus::Accepted)
        .filter_map(|e| graph.node(if outgoing { &e.to } else { &e.from }))
        .filter(|n| accepted(n))
        .collect()
}

/// The requirements named by governed markers of one kind. The context already validated the
/// markers, so an error here is an evaluator inability.
fn marker_targets(
    graph: &Graph,
    select: impl Fn(&Marker) -> Option<&Id>,
) -> Result<BTreeSet<Id>, EvaluatorFailure> {
    let markers = governed_markers(graph).map_err(|e| failure("F1_GOVERNANCE", e))?;
    Ok(markers
        .iter()
        .filter_map(|m| select(&m.marker).cloned())
        .collect())
}

fn human_origins(graph: &Graph) -> Result<BTreeSet<Id>, EvaluatorFailure> {
    marker_targets(graph, |m| match m {
        Marker::HumanRequirementOrigin { requirement_ref } => Some(requirement_ref),
        _ => None,
    })
}

/// Active finding material of `code`: supplemental analysis findings and Accepted Open PSG
/// Finding nodes whose affected requirements are all currently Accepted, by Finding ID.
fn active_findings(
    graph: &Graph,
    inputs: &F1ValidationInputs,
    code: &str,
) -> BTreeMap<Id, Vec<Id>> {
    let all_accepted = |refs: &[Id]| {
        !refs.is_empty()
            && refs.iter().all(|r| {
                graph.node(r).is_some_and(|n| {
                    accepted(n) && matches!(n.payload, NodePayload::Requirement(_))
                })
            })
    };
    let mut active = BTreeMap::new();
    for finding in &inputs.analysis_findings {
        if finding.payload.code == code && all_accepted(&finding.payload.affected_refs) {
            active.insert(finding.id.clone(), finding.payload.affected_refs.clone());
        }
    }
    for id in graph.node_ids_by_type(NodeType::Finding) {
        let Some(node) = graph.node(id).filter(|n| accepted(n)) else {
            continue;
        };
        if let NodePayload::Finding(finding) = &node.payload {
            if finding.code == code
                && finding.status == "Open"
                && all_accepted(&finding.affected_refs)
            {
                active.insert(node.id.clone(), finding.affected_refs.clone());
            }
        }
    }
    active
}

fn finding_rule(
    graph: &Graph,
    ctx: &ValidationContext,
    code: &str,
    condition: &str,
    message: &str,
    resolution: &str,
) -> Outcome {
    let inputs = inputs(ctx)?;
    let active = active_findings(graph, inputs, code);
    let violating: BTreeSet<Id> = active.values().flatten().cloned().collect();
    let evidence: BTreeSet<String> = active.keys().map(Id::to_string).collect();
    aggregate(
        ids(&accepted_requirements(graph)),
        violating,
        evidence,
        condition,
        message,
        resolution,
    )
}

// ============================================================================ evaluators

fn statement_present(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let requirements = accepted_requirements(graph);
    let violating = requirements
        .iter()
        .filter(|(_, r)| r.statement.chars().all(char::is_whitespace))
        .map(|(n, _)| n.id.clone())
        .collect();
    aggregate(
        ids(&requirements),
        violating,
        BTreeSet::new(),
        "accepted_requirement_statement_missing",
        "Accepted requirements must have a non-empty statement.",
        "Provide an explicit requirement statement grounded in authoritative evidence.",
    )
}

/// A cycle-protected walk of Accepted derived_from edges reaching an Accepted EvidenceFragment.
fn derives_from_evidence(graph: &Graph, start: &Id) -> bool {
    let mut visited: BTreeSet<&Id> = BTreeSet::from([start]);
    let mut frontier = vec![start];
    while let Some(id) = frontier.pop() {
        for next in neighbours(graph, id, &RelationKind::DerivedFrom, true) {
            if matches!(next.payload, NodePayload::EvidenceFragment(_)) {
                return true;
            }
            if visited.insert(&next.id) {
                frontier.push(&next.id);
            }
        }
    }
    false
}

fn grounded(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let requirements = accepted_requirements(graph);
    let human = human_origins(graph)?;
    let violating = requirements
        .iter()
        .filter(|(node, _)| {
            let evidenced = neighbours(graph, &node.id, &RelationKind::EvidencedBy, true)
                .iter()
                .any(|n| matches!(n.payload, NodePayload::EvidenceFragment(_)));
            !(!node.evidence.is_empty()
                || evidenced
                || derives_from_evidence(graph, &node.id)
                || human.contains(&node.id))
        })
        .map(|(n, _)| n.id.clone())
        .collect();
    aggregate(
        ids(&requirements),
        violating,
        BTreeSet::new(),
        "accepted_requirement_grounding_missing",
        "Accepted requirements must be grounded in evidence or an explicit human-origin decision.",
        "Attach authoritative evidence or record a governed human-origin decision.",
    )
}

fn lineage(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let applicable: Vec<(&Node, &Requirement)> = accepted_requirements(graph)
        .into_iter()
        .filter(|(_, r)| r.level != RequirementLevel::Stakeholder)
        .collect();
    if applicable.is_empty() {
        return output(RuleEvaluation::not_applicable(
            "No accepted non-stakeholder requirement requires intent-lineage validation."
                .to_owned(),
        ));
    }
    let human = human_origins(graph)?;
    let violating = applicable
        .iter()
        .filter(|(node, _)| {
            let id = &node.id;
            let addresses = neighbours(graph, id, &RelationKind::Addresses, true)
                .iter()
                .any(|n| matches!(n.payload, NodePayload::Goal(_) | NodePayload::Concern(_)));
            let refines = neighbours(graph, id, &RelationKind::Refines, true)
                .iter()
                .any(|n| matches!(n.payload, NodePayload::Requirement(_)));
            let decomposed = neighbours(graph, id, &RelationKind::DecomposesTo, false)
                .iter()
                .any(|n| matches!(n.payload, NodePayload::Requirement(_)));
            let derived = neighbours(graph, id, &RelationKind::DerivedFrom, true)
                .iter()
                .any(|n| {
                    matches!(
                        n.payload,
                        NodePayload::Need(_) | NodePayload::Goal(_) | NodePayload::Concern(_)
                    )
                });
            !(addresses || refines || decomposed || derived || human.contains(id))
        })
        .map(|(n, _)| n.id.clone())
        .collect();
    aggregate(
        ids(&applicable),
        violating,
        BTreeSet::new(),
        "accepted_requirement_lineage_missing",
        "Accepted non-stakeholder requirements must have explicit intent lineage or a human-origin decision.",
        "Link the requirement to its governing intent or record a governed human-origin decision.",
    )
}

/// Typed `RequirementKind` and `RequirementLevel` make unknown values unrepresentable.
fn type_known(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    output(RuleEvaluation::pass(
        ids(&accepted_requirements(graph)),
        Vec::new(),
    ))
}

fn no_duplicate(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    finding_rule(
        graph,
        ctx,
        NO_DUPLICATE,
        "accepted_requirement_duplicates_open",
        "Accepted requirements have unresolved duplicate findings.",
        "Resolve, merge, supersede or govern the duplicate requirement pairs.",
    )
}

fn no_contradiction(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    finding_rule(
        graph,
        ctx,
        NO_CONTRADICTION,
        "accepted_requirement_contradictions_open",
        "Accepted requirements have unresolved contradiction findings.",
        "Resolve the conflicting requirements through governed semantic decisions.",
    )
}

fn terms_resolved(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let mut violating = BTreeSet::new();
    let mut evidence = BTreeSet::new();
    for input in &inputs.requirements {
        for dep in &input.vocabulary_dependencies {
            if dep.resolution == F1VocabularyResolution::Unresolved {
                violating.insert(input.requirement_ref.clone());
                evidence.insert(format!(
                    "term:{}:{}:{}:{}",
                    input.requirement_ref, dep.range.start, dep.range.end, dep.normalized_key
                ));
            }
        }
    }
    for (finding, targets) in active_findings(graph, inputs, TERMS_RESOLVED) {
        violating.extend(targets);
        evidence.insert(finding.to_string());
    }
    aggregate(
        ids(&accepted_requirements(graph)),
        violating,
        evidence,
        "accepted_requirement_terms_unresolved",
        "Accepted requirements contain unresolved semantic vocabulary dependencies.",
        "Accept or govern the required vocabulary concepts, or declare the dependency as a governed external identifier.",
    )
}

/// The controlling modal reading of a statement under the pilot lexical contract
/// (Hotfix 032).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalReading {
    Supported(Modality),
    Unsupported,
}

/// Maximal ASCII word runs `[A-Za-z0-9_]` as byte ranges, in byte order.
fn word_tokens(statement: &str) -> Vec<(usize, usize)> {
    let bytes = statement.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if is_word(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_word(bytes[i]) {
                i += 1;
            }
            out.push((start, i));
        } else {
            i += 1;
        }
    }
    out
}

/// The first token among shall, should, may, must and can controls; an immediately
/// following `not` separated only by space, tab, CR or LF negates it. Later modal
/// words do not participate, and `cannot` is not a candidate token.
fn controlling_modal(statement: &str) -> Option<ModalReading> {
    let bytes = statement.as_bytes();
    let tokens = word_tokens(statement);
    let (index, lower) = tokens
        .iter()
        .enumerate()
        .find_map(|(index, &(start, end))| {
            let lower = statement[start..end].to_ascii_lowercase();
            matches!(lower.as_str(), "shall" | "should" | "may" | "must" | "can")
                .then_some((index, lower))
        })?;
    let end = tokens[index].1;
    let negated = tokens
        .get(index + 1)
        .is_some_and(|&(next_start, next_end)| {
            statement[next_start..next_end].eq_ignore_ascii_case("not")
                && bytes[end..next_start]
                    .iter()
                    .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        });
    Some(match (lower.as_str(), negated) {
        ("shall", false) => ModalReading::Supported(Modality::Shall),
        ("shall", true) => ModalReading::Supported(Modality::ShallNot),
        ("should", false) => ModalReading::Supported(Modality::Should),
        ("may", false) => ModalReading::Supported(Modality::May),
        _ => ModalReading::Unsupported,
    })
}

fn modality_explicit(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let requirements = accepted_requirements(graph);
    let violating = requirements
        .iter()
        .filter(|(_, r)| {
            controlling_modal(&r.statement) != Some(ModalReading::Supported(r.modality))
        })
        .map(|(n, _)| n.id.clone())
        .collect();
    aggregate(
        ids(&requirements),
        violating,
        BTreeSet::new(),
        "accepted_requirement_modality_inconsistent",
        "Accepted requirements must express the same explicit modality as their typed Requirement.modality.",
        "Correct the requirement wording or its governed modality classification.",
    )
}

fn criteria_for_behavior(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let applicable: Vec<(&Node, &Requirement)> = accepted_requirements(graph)
        .into_iter()
        .filter(|(_, r)| {
            matches!(
                r.requirement_kind,
                RequirementKind::Functional
                    | RequirementKind::Interface
                    | RequirementKind::Security
            )
        })
        .collect();
    if applicable.is_empty() {
        return output(RuleEvaluation::not_applicable(
            "No accepted behavioral requirement is in the F1 implementation scope.".to_owned(),
        ));
    }
    let deferred = marker_targets(graph, |m| match m {
        Marker::DeferredVerification { requirement_ref } => Some(requirement_ref),
        _ => None,
    })?;
    let violating = applicable
        .iter()
        .filter(|(node, _)| {
            let id = &node.id;
            let criterion = neighbours(graph, id, &RelationKind::DerivedFrom, false)
                .iter()
                .any(|n| matches!(n.payload, NodePayload::AcceptanceCriterion(_)));
            let specified = neighbours(graph, id, &RelationKind::SpecifiedBy, true)
                .iter()
                .any(|n| {
                    matches!(
                        n.payload,
                        NodePayload::Operation(_)
                            | NodePayload::Process(_)
                            | NodePayload::Rule(_)
                            | NodePayload::QualityScenario(_)
                    )
                });
            !(criterion || specified || deferred.contains(id))
        })
        .map(|(n, _)| n.id.clone())
        .collect();
    aggregate(
        ids(&applicable),
        violating,
        BTreeSet::new(),
        "behavior_requirement_criteria_missing",
        "Accepted behavioral requirements in implementation scope need acceptance criteria, executable semantics, or an explicit deferred-verification decision.",
        "Add governed acceptance criteria or executable semantics, or record a governed deferred-verification decision.",
    )
}

fn quality_findings_clear(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let inputs = inputs(ctx)?;
    let mut violating = BTreeSet::new();
    let mut evidence = BTreeSet::new();
    for input in &inputs.requirements {
        let result = lint_requirement(&input.lint_input, &inputs.lint_policy)
            .map_err(|e| failure("F1_LINT_EVALUATION", e.to_string()))?;
        for d in result
            .diagnostics
            .iter()
            .filter(|d| d.effective_severity == LintSeverity::Error)
        {
            violating.insert(input.requirement_ref.clone());
            evidence.insert(format!(
                "lint:{}:{}:{}:{}",
                d.rule_id.as_str(),
                input.requirement_ref,
                d.statement_range.start,
                d.statement_range.end
            ));
        }
    }
    aggregate(
        ids(&accepted_requirements(graph)),
        violating,
        evidence,
        "accepted_requirement_quality_errors_open",
        "Accepted requirements have unresolved error-severity deterministic lint diagnostics.",
        "Resolve or govern the high-severity requirement-quality diagnostics.",
    )
}

fn superseded_excluded(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let requirements = accepted_requirements(graph);
    let violating = requirements
        .iter()
        .filter(|(node, _)| {
            graph
                .incoming_edge_ids(&node.id)
                .iter()
                .filter_map(|e| graph.edge(e))
                .any(|e| e.kind == RelationKind::Supersedes && e.status == ElementStatus::Accepted)
        })
        .map(|(n, _)| n.id.clone())
        .collect();
    aggregate(
        ids(&requirements),
        violating,
        BTreeSet::new(),
        "superseded_requirement_still_active",
        "Requirements targeted by an accepted supersedes relation must not remain Accepted.",
        "Apply the governed supersession status transition so the old requirement is excluded from active obligation scope.",
    )
}
