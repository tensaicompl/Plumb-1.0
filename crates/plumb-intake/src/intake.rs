//! Patch-centric intake analysis (plan X1.3, Hotfix 059): a pure dry run of one immutable
//! Proposal against the current graph, composed from the patch engine (candidate, delta,
//! impact), the X1.2 lexical index, tokenizer and text projection, supplied candidate gate
//! reports and explicit PSG relations and Terms; plus the pure acceptance-rationale guard.
//!
//! Nothing here writes, persists, reads time or calls a model. Rule semantics arrive only
//! through supplied gate reports.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{CoreError, GateId, Id};
use plumb_expr::ExprError;
use plumb_patch::{
    apply_patch, compute_impact, AcceptancePolicy, ImpactError, PatchError, Proposal,
    ProposalError, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{ElementStatus, Graph, Node, NodePayload, RelationKind};
use plumb_validation::{EvaluationError, GateReport, GeneratedFinding};
use thiserror::Error;

use crate::bm25::tokenize;
use crate::index::{document_text, IndexError, LexicalIndex, RetrievalDocumentKind};
use crate::report::{
    finding_requires_reason, CoverageMatch, DeclaredConflict, DuplicateCandidate,
    IntakeAcceptanceError, IntakeReasonCode, IntakeReport, IntakeReportError, RefinementLink,
    SubjectNovelty, TermResolution, TermResolutionStatus, INTAKE_DUPLICATE_CANDIDATE_LIMIT,
};

/// Why intake produced no report.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum IntakeError {
    #[error("invalid proposal: {0}")]
    Proposal(#[from] ProposalError),
    #[error("proposal patch cannot be applied: {0}")]
    Patch(#[from] PatchError),
    #[error("impact cannot be computed: {0}")]
    Impact(#[from] ImpactError),
    #[error("lexical index failure: {0}")]
    Index(#[from] IndexError),
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("invalid {gate} candidate gate report: {error}")]
    Validation {
        gate: GateId,
        error: EvaluationError,
    },
    #[error("more than one candidate gate report for {gate}")]
    DuplicateValidationGate { gate: GateId },
    #[error("{gate} candidate gate report is not bound to the candidate semantic hash")]
    ValidationReportBaselineMismatch { gate: GateId },
    #[error("{gate} candidate gate report is not for the candidate profile")]
    ValidationReportProfileMismatch { gate: GateId },
    #[error("{gate} candidate gate report does not share the validation context of the others")]
    IncoherentValidationReports { gate: GateId },
    #[error("candidate gate reports disagree on finding {finding}")]
    ConflictingValidationFinding { finding: Id },
    #[error("invalid intake report: {0}")]
    Report(#[from] IntakeReportError),
    /// A proposed expression exceeds a PlumbExpr safety limit; the source is not retained.
    #[error("a proposed expression exceeds {limit}: {actual} above the limit {max}")]
    ExpressionLimit {
        limit: &'static str,
        actual: usize,
        max: usize,
    },
}

/// The candidate gate reports supplied to intake, unique per gate and in `GateId::ALL` order.
/// Empty means no semantic gate evidence was supplied.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntakeValidationInput {
    candidate_reports: Vec<GateReport>,
}

impl IntakeValidationInput {
    /// Orders the reports canonically; a second report for one gate is an error.
    pub fn new(mut candidate_reports: Vec<GateReport>) -> Result<Self, IntakeError> {
        candidate_reports.sort_by_key(|r| gate_position(r.gate));
        for pair in candidate_reports.windows(2) {
            if pair[0].gate == pair[1].gate {
                return Err(IntakeError::DuplicateValidationGate { gate: pair[0].gate });
            }
        }
        Ok(IntakeValidationInput { candidate_reports })
    }

    /// No candidate gate reports.
    pub fn empty() -> Self {
        IntakeValidationInput::default()
    }

    pub fn candidate_reports(&self) -> &[GateReport] {
        &self.candidate_reports
    }
}

fn gate_position(gate: GateId) -> usize {
    GateId::ALL
        .iter()
        .position(|g| *g == gate)
        .unwrap_or(GateId::ALL.len())
}

/// Binds every report to the candidate graph and requires one shared validation context.
fn check_reports(
    candidate: &Graph,
    candidate_hash: &plumb_core::Hash,
    reports: &[GateReport],
) -> Result<(), IntakeError> {
    let first = reports.first();
    for (index, report) in reports.iter().enumerate() {
        let gate = report.gate;
        if index > 0 && reports[index - 1].gate == gate {
            return Err(IntakeError::DuplicateValidationGate { gate });
        }
        report
            .validate()
            .map_err(|error| IntakeError::Validation { gate, error })?;
        if &report.baseline_semantic_hash != candidate_hash {
            return Err(IntakeError::ValidationReportBaselineMismatch { gate });
        }
        if &report.profile_id != candidate.profile_id() {
            return Err(IntakeError::ValidationReportProfileMismatch { gate });
        }
        if let Some(first) = first {
            let coherent = report.profile_id == first.profile_id
                && report.profile_hash == first.profile_hash
                && report.rule_pack_hash == first.rule_pack_hash
                && report.policy == first.policy
                && report.baseline_evidence_hash == first.baseline_evidence_hash
                && report.validation_artifact_refs == first.validation_artifact_refs
                && report.evidence_artifact_refs == first.evidence_artifact_refs;
            if !coherent {
                return Err(IntakeError::IncoherentValidationReports { gate });
            }
        }
    }
    Ok(())
}

/// Whether a candidate node is new material of the proposal: Proposed or Accepted, and added,
/// payload-changed or moved into Accepted. Envelope-only changes do not count.
fn is_proposed_material(base: &Graph, node: &Node) -> bool {
    if !matches!(
        node.status,
        ElementStatus::Proposed | ElementStatus::Accepted
    ) {
        return false;
    }
    match base.node(&node.id) {
        None => true,
        Some(old) => {
            old.payload != node.payload
                || (old.status != ElementStatus::Accepted && node.status == ElementStatus::Accepted)
        }
    }
}

struct Subject<'a> {
    node: &'a Node,
    kind: RetrievalDocumentKind,
    text: String,
}

/// `tokenize(text).join(" ")`, or `None` when the text has no token.
fn term_key(text: &str) -> Option<String> {
    let tokens = tokenize(text);
    (!tokens.is_empty()).then(|| tokens.join(" "))
}

fn token_set(text: &str) -> BTreeSet<String> {
    tokenize(text).into_iter().collect()
}

/// The novelty text of one node (Fable #18): its retrieval text, except that a Scenario is
/// projected to its name and the string leaves of given/when/then only, without the JSON keys
/// and `scenario_kind` that every Scenario document shares.
fn novelty_text(
    payload: &NodePayload,
) -> Result<Option<(RetrievalDocumentKind, String)>, CoreError> {
    fn string_leaves<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
        match value {
            serde_json::Value::String(s) => out.push(s),
            serde_json::Value::Array(items) => items.iter().for_each(|v| string_leaves(v, out)),
            serde_json::Value::Object(map) => map.values().for_each(|v| string_leaves(v, out)),
            _ => {}
        }
    }
    let NodePayload::Scenario(s) = payload else {
        return document_text(payload);
    };
    let mut lines = vec![s.name.as_str()];
    s.given.iter().for_each(|v| string_leaves(v, &mut lines));
    string_leaves(&s.when, &mut lines);
    s.then.iter().for_each(|v| string_leaves(v, &mut lines));
    Ok(Some((RetrievalDocumentKind::Scenario, lines.join("\n"))))
}

/// The expression-bearing fields of a payload (compiler architecture §33.1), absent optional
/// fields omitted.
fn payload_expressions(payload: &NodePayload) -> Vec<&str> {
    match payload {
        NodePayload::Transition(t) => t.guard_expr.iter().map(String::as_str).collect(),
        NodePayload::Invariant(i) => vec![i.expression.as_str()],
        NodePayload::ProcessNode(p) => [&p.condition_expr, &p.timer_expr]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect(),
        NodePayload::Rule(r) => [&r.condition_expr, &r.action_expr]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect(),
        NodePayload::Calculation(c) => vec![c.expression.as_str()],
        NodePayload::PolicyCondition(c) => vec![c.expression.as_str()],
        _ => Vec::new(),
    }
}

/// Rejects a patch carrying an expression above a PlumbExpr safety limit, in `AddNode` and
/// `ReplacePayload` payloads at any `Compound` depth. Only the limits are enforced here: any
/// other parse outcome leaves intake unchanged.
fn check_expression_limits(patch: &SemanticPatch) -> Result<(), IntakeError> {
    let payload = match patch {
        SemanticPatch::Compound { patches } => {
            return patches.iter().try_for_each(check_expression_limits)
        }
        SemanticPatch::AddNode { node } => &node.payload,
        SemanticPatch::ReplacePayload { payload, .. } => payload,
        _ => return Ok(()),
    };
    for expression in payload_expressions(payload) {
        if let Err(ExprError::LimitExceeded { limit, actual, max }) = plumb_expr::parse(expression)
        {
            return Err(IntakeError::ExpressionLimit { limit, actual, max });
        }
    }
    Ok(())
}

/// Analyzes `proposal` against `graph` without changing either. A patch that cannot be applied
/// to `graph` is an error, never a report.
pub fn intake(
    graph: &Graph,
    proposal: &Proposal,
    validation: &IntakeValidationInput,
) -> Result<IntakeReport, IntakeError> {
    proposal.validate()?;
    check_expression_limits(&proposal.patch_set.patch)?;
    let applied = apply_patch(graph, &proposal.patch_set)?;
    let (candidate, delta) = (&applied.graph, &applied.delta);
    let impact = compute_impact(graph, candidate, delta)?;
    let base_semantic_hash = graph.semantic_hash()?;
    let candidate_semantic_hash = candidate.semantic_hash()?;
    let reports = validation.candidate_reports();
    check_reports(candidate, &candidate_semantic_hash, reports)?;
    let index = LexicalIndex::build(graph)?;

    // Proposal material: the added or modified candidate nodes that carry new content.
    let changed: BTreeSet<&Id> = delta
        .added_nodes
        .iter()
        .chain(&delta.modified_nodes)
        .collect();
    let material: Vec<&Node> = changed
        .into_iter()
        .filter_map(|id| candidate.node(id))
        .filter(|node| is_proposed_material(graph, node))
        .collect();
    let mut subjects = Vec::new();
    for node in &material {
        if let Some((kind, text)) = document_text(&node.payload)? {
            if !tokenize(&text).is_empty() {
                subjects.push(Subject { node, kind, text });
            }
        }
    }

    // Exact typed coverage by Accepted base nodes of the same type.
    let mut covered_by = Vec::new();
    for subject in &subjects {
        for id in graph.node_ids_by_type(subject.node.payload.node_type()) {
            let Some(base) = graph.node(id) else { continue };
            if base.status == ElementStatus::Accepted && base.payload == subject.node.payload {
                covered_by.push(CoverageMatch {
                    subject_ref: subject.node.id.clone(),
                    covered_by_ref: id.clone(),
                    score: 1.0,
                });
            }
        }
    }

    // Scenario scaffolding: the retrieval terms every Accepted base Scenario document carries
    // (JSON keys, shared kinds). With fewer than two Scenario documents nothing is
    // scaffolding, so a lone base Scenario still yields candidates.
    let mut scenario_terms: BTreeMap<&Id, BTreeSet<String>> = BTreeMap::new();
    for (id, node) in graph.nodes() {
        if node.status != ElementStatus::Accepted {
            continue;
        }
        if let Some((RetrievalDocumentKind::Scenario, text)) = document_text(&node.payload)? {
            scenario_terms.insert(id, token_set(&text));
        }
    }
    let scaffolding: BTreeSet<String> = if scenario_terms.len() < 2 {
        BTreeSet::new()
    } else {
        let mut documents = scenario_terms.values();
        let first = documents.next().cloned().unwrap_or_default();
        documents.fold(first, |common, terms| {
            common.intersection(terms).cloned().collect()
        })
    };
    // A Scenario candidate whose only shared terms are scaffolding is no candidate.
    let shares_content = |subject: &Subject<'_>, candidate: &Id| {
        if subject.kind != RetrievalDocumentKind::Scenario {
            return true;
        }
        let Some(terms) = scenario_terms.get(candidate) else {
            return true;
        };
        token_set(&subject.text)
            .intersection(terms)
            .any(|term| !scaffolding.contains(term))
    };

    // Advisory same-kind lexical candidates from the Accepted base corpus.
    let mut duplicate_candidates = Vec::new();
    for subject in &subjects {
        let hits = index.search(&subject.text, index.document_count())?;
        duplicate_candidates.extend(
            hits.into_iter()
                .filter(|hit| hit.kind == subject.kind && hit.id != subject.node.id)
                .filter(|hit| shares_content(subject, &hit.id))
                .take(INTAKE_DUPLICATE_CANDIDATE_LIMIT)
                .map(|hit| DuplicateCandidate {
                    subject_ref: subject.node.id.clone(),
                    candidate_ref: hit.id,
                    score: hit.score,
                }),
        );
    }

    // Explicit refines / conflicts_with edges the patch added or modified.
    let mut refines = Vec::new();
    let mut declared_conflicts = Vec::new();
    let changed_edges: BTreeSet<&Id> = delta
        .added_edges
        .iter()
        .chain(&delta.modified_edges)
        .collect();
    for edge in changed_edges
        .into_iter()
        .filter_map(|id| candidate.edge(id))
    {
        if !matches!(
            edge.status,
            ElementStatus::Proposed | ElementStatus::Accepted
        ) {
            continue;
        }
        let (edge_ref, from_ref, to_ref) = (edge.id.clone(), edge.from.clone(), edge.to.clone());
        match edge.kind {
            RelationKind::Refines => refines.push(RefinementLink {
                edge_ref,
                from_ref,
                to_ref,
            }),
            RelationKind::ConflictsWith => declared_conflicts.push(DeclaredConflict {
                edge_ref,
                from_ref,
                to_ref,
            }),
            _ => {}
        }
    }

    // Term resolution against the Accepted base Term and alias keys.
    let mut dictionary: BTreeMap<String, BTreeSet<Id>> = BTreeMap::new();
    for (id, node) in graph.nodes() {
        let NodePayload::Term(term) = &node.payload else {
            continue;
        };
        if node.status != ElementStatus::Accepted {
            continue;
        }
        let names = std::iter::once(&term.term).chain(term.aliases.iter().flatten());
        for key in names.filter_map(|name| term_key(name)) {
            dictionary.entry(key).or_default().insert(id.clone());
        }
    }
    let mut term_resolutions = Vec::new();
    for node in &material {
        let NodePayload::Term(term) = &node.payload else {
            continue;
        };
        let key = term_key(&term.term);
        let matched_term_refs: Vec<Id> = key
            .as_ref()
            .and_then(|k| dictionary.get(k))
            .map(|ids| ids.iter().cloned().collect())
            .unwrap_or_default();
        let status = match (&key, matched_term_refs.len()) {
            (None, _) => TermResolutionStatus::Unsearchable,
            (Some(_), 0) => TermResolutionStatus::New,
            (Some(_), 1) => TermResolutionStatus::Resolved,
            (Some(_), _) => TermResolutionStatus::Ambiguous,
        };
        term_resolutions.push(TermResolution {
            term_ref: node.id.clone(),
            key,
            matched_term_refs,
            status,
        });
    }

    // Jaccard novelty against the Accepted, token-bearing base documents of the same kind, over
    // the novelty projection (a Scenario's name and string leaves, never its JSON keys).
    let mut base_documents: BTreeMap<RetrievalDocumentKind, Vec<(&Id, BTreeSet<String>)>> =
        BTreeMap::new();
    for (id, node) in graph.nodes() {
        if node.status != ElementStatus::Accepted {
            continue;
        }
        if let Some((kind, text)) = novelty_text(&node.payload)? {
            let tokens = token_set(&text);
            if !tokens.is_empty() {
                base_documents.entry(kind).or_default().push((id, tokens));
            }
        }
    }
    let mut subject_novelty = Vec::new();
    for subject in &subjects {
        let tokens = match novelty_text(&subject.node.payload)? {
            Some((_, text)) => token_set(&text),
            None => BTreeSet::new(),
        };
        // The nearest document is the first most similar one with any shared term (Fable #21).
        let mut nearest: Option<(&Id, f64)> = None;
        for (id, other) in base_documents.get(&subject.kind).into_iter().flatten() {
            let shared = tokens.intersection(other).count() as f64;
            let union = tokens.union(other).count() as f64;
            let similarity = shared / union;
            if similarity > 0.0 && nearest.is_none_or(|(_, best)| similarity > best) {
                nearest = Some((id, similarity));
            }
        }
        let similarity = nearest.map_or(0.0, |(_, s)| s);
        subject_novelty.push(SubjectNovelty {
            subject_ref: subject.node.id.clone(),
            kind: subject.kind,
            nearest_ref: nearest.map(|(id, _)| id.clone()),
            similarity,
            novelty: 1.0 - similarity,
        });
    }
    let novelty = subject_novelty
        .iter()
        .map(|s| s.novelty)
        .min_by(f64::total_cmp);

    // Exact findings of the supplied reports that touch the proposal scope.
    let mut scope: BTreeSet<&Id> = delta
        .touched_nodes
        .iter()
        .chain(&delta.added_nodes)
        .chain(&delta.modified_nodes)
        .chain(&impact.dirty.node_ids)
        .collect();
    for id in &delta.touched_edges {
        if let Some(edge) = candidate.edge(id).or_else(|| graph.edge(id)) {
            scope.insert(&edge.from);
            scope.insert(&edge.to);
        }
    }
    let mut relevant: BTreeMap<Id, &GeneratedFinding> = BTreeMap::new();
    for finding in reports.iter().flat_map(|r| &r.findings) {
        if !finding
            .payload
            .affected_refs
            .iter()
            .any(|r| scope.contains(r))
        {
            continue;
        }
        match relevant.get(&finding.id) {
            Some(existing) if *existing != finding => {
                return Err(IntakeError::ConflictingValidationFinding {
                    finding: finding.id.clone(),
                })
            }
            Some(_) => {}
            None => {
                relevant.insert(finding.id.clone(), finding);
            }
        }
    }
    let validation_findings: Vec<GeneratedFinding> = relevant.into_values().cloned().collect();

    let mut reason_codes = BTreeSet::new();
    if proposal.materiality == ProposalMateriality::MaterialDecision {
        reason_codes.insert(IntakeReasonCode::MaterialDecision);
    }
    if proposal.acceptance_policy == AcceptancePolicy::HumanDecision {
        reason_codes.insert(IntakeReasonCode::HumanDecision);
    }
    if covered_by.iter().any(|c| c.subject_ref != c.covered_by_ref) {
        reason_codes.insert(IntakeReasonCode::ExactEquivalent);
    }
    if !declared_conflicts.is_empty() {
        reason_codes.insert(IntakeReasonCode::DeclaredConflict);
    }
    if validation_findings.iter().any(finding_requires_reason) {
        reason_codes.insert(IntakeReasonCode::ValidationConflict);
    }
    if term_resolutions
        .iter()
        .any(|t| t.status == TermResolutionStatus::Ambiguous)
    {
        reason_codes.insert(IntakeReasonCode::AmbiguousTerm);
    }

    let report = IntakeReport {
        proposal_ref: proposal.id.clone(),
        base_semantic_hash,
        candidate_semantic_hash,
        retrieval_corpus_hash: index.corpus_hash().clone(),
        covered_by,
        duplicate_candidates,
        refines,
        term_resolutions,
        declared_conflicts,
        validation_gates: reports.iter().map(|r| r.gate).collect(),
        validation_findings,
        subject_novelty,
        novelty,
        impact,
        requires_reason: !reason_codes.is_empty(),
        reason_codes,
    };
    report.validate()?;
    Ok(report)
}

/// Whether one intake validation finding is a surfaced validation conflict: exactly the unwaived
/// Blocker or Error findings behind [`IntakeReasonCode::ValidationConflict`] (Hotfix 065, the
/// intake report projection).
pub fn is_validation_conflict(finding: &GeneratedFinding) -> bool {
    finding_requires_reason(finding)
}

/// The pure acceptance-rationale guard that acceptance paths call before accepting
/// `proposal`: when its report requires a reason, `decision_node` must be an Accepted
/// ResolutionDecision bound to the proposal with a non-blank rationale.
pub fn validate_required_rationale(
    proposal: &Proposal,
    report: &IntakeReport,
    decision_node: Option<&Node>,
) -> Result<(), IntakeAcceptanceError> {
    if report.proposal_ref != proposal.id {
        return Err(IntakeAcceptanceError::ReportProposalMismatch {
            proposal: proposal.id.clone(),
            report: report.proposal_ref.clone(),
        });
    }
    if !report.requires_reason {
        return Ok(());
    }
    let Some(node) = decision_node else {
        return Err(IntakeAcceptanceError::ReasonRequired(proposal.id.clone()));
    };
    if node.status != ElementStatus::Accepted {
        return Err(IntakeAcceptanceError::DecisionNotAccepted(node.id.clone()));
    }
    let NodePayload::ResolutionDecision(decision) = &node.payload else {
        return Err(IntakeAcceptanceError::DecisionWrongType(node.id.clone()));
    };
    if decision.proposal_ref.as_ref() != Some(&proposal.id) {
        return Err(IntakeAcceptanceError::DecisionProposalMismatch {
            decision: node.id.clone(),
            proposal: proposal.id.clone(),
        });
    }
    match &decision.rationale {
        Some(rationale) if !rationale.trim().is_empty() => Ok(()),
        _ => Err(IntakeAcceptanceError::MissingRationale(node.id.clone())),
    }
}
