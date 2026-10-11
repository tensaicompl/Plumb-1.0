//! Application workflow material of the A0.3 runtime (Hotfix 065): the one recursive proposal
//! action classifier, the session identity, typed decoding of durable workflow records and the
//! public intake-report projection.
//!
//! The store keeps canonical opaque JSON; this layer deserializes it, validates it,
//! recanonicalizes it and requires byte equality, so corrupt material is reported and never
//! repaired silently.

use std::collections::BTreeMap;

use plumb_core::{to_canonical_json, Hash, Id, Timestamp};
use plumb_functional::resolution::ACCEPTANCE_DECISION_KIND;
use plumb_intake::{is_validation_conflict, IntakeReport, TermResolutionStatus};
use plumb_patch::{Proposal, SemanticPatch};
use plumb_psg::{
    contributes_to_semantic_hash, ElementStatus, Graph, NodePayload, NodeType, RelationKind,
};
use plumb_store::{WorkflowProposal, WorkflowProposalKind};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::openapi::{
    ConflictView, CoverageView, DuplicateCandidateView, Finding, IntakeReasonCode,
    IntakeReportView, ProposalKind,
};
use crate::state::{parse_wire, wire};

// ============================================================================ classification

/// The workflow action classes a patch can carry; each is checked recursively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SpecialClass {
    Link,
    Merge,
    Supersede,
}

/// Classifies `proposal` into exactly one [`ProposalKind`] (Hotfix 065): `Supersede` ->
/// supersede, `MergeNodes` -> merge, `AddEdge`/`ReplaceEdge` of kind `refines` -> link; more than
/// one distinct special class is 422 `E_PROPOSAL_ACTION_AMBIGUOUS`. Otherwise, ignoring
/// governance-only material (ResolutionDecision nodes, `resolves` edges and Question or Finding
/// lifecycle changes), adding a node of a semantic-hash type or a semantic relation is create,
/// and anything else is accept. The classification changes no materiality or policy.
pub(crate) fn classify(graph: &Graph, proposal: &Proposal) -> Result<ProposalKind, ApiError> {
    let mut leaves = Vec::new();
    flatten(&proposal.patch_set.patch, &mut leaves);
    // Node types of existing nodes and of nodes the patch itself adds.
    let mut added: BTreeMap<&Id, NodeType> = BTreeMap::new();
    for leaf in &leaves {
        if let SemanticPatch::AddNode { node } = leaf {
            added.insert(&node.id, node.payload.node_type());
        }
    }
    let node_type = |id: &Id| {
        added
            .get(id)
            .copied()
            .or_else(|| graph.node(id).map(|n| n.payload.node_type()))
    };
    let mut special = Vec::new();
    let mut creates = false;
    for leaf in &leaves {
        match leaf {
            SemanticPatch::Supersede { .. } => special.push(SpecialClass::Supersede),
            SemanticPatch::MergeNodes { .. } => special.push(SpecialClass::Merge),
            SemanticPatch::AddEdge { edge } if edge.kind == RelationKind::Refines => {
                special.push(SpecialClass::Link)
            }
            SemanticPatch::ReplaceEdge { kind, .. } if *kind == RelationKind::Refines => {
                special.push(SpecialClass::Link)
            }
            SemanticPatch::AddNode { node } => {
                let t = node.payload.node_type();
                if t != NodeType::ResolutionDecision && contributes_to_semantic_hash(t) {
                    creates = true;
                }
            }
            SemanticPatch::AddEdge { edge } => {
                let semantic = edge.kind != RelationKind::Resolves
                    && [&edge.from, &edge.to].iter().all(|id| {
                        node_type(id).is_some_and(|t| {
                            t != NodeType::ResolutionDecision && contributes_to_semantic_hash(t)
                        })
                    });
                if semantic {
                    creates = true;
                }
            }
            _ => {}
        }
    }
    special.sort();
    special.dedup();
    match special.as_slice() {
        [] if creates => Ok(ProposalKind::Create),
        [] => Ok(ProposalKind::Accept),
        [SpecialClass::Link] => Ok(ProposalKind::Link),
        [SpecialClass::Merge] => Ok(ProposalKind::Merge),
        [SpecialClass::Supersede] => Ok(ProposalKind::Supersede),
        _ => Err(ApiError::unprocessable(
            "E_PROPOSAL_ACTION_AMBIGUOUS",
            "the proposal carries more than one distinct action class",
        )
        .with_details(json!({ "proposal_ref": proposal.id }))),
    }
}

fn flatten<'a>(patch: &'a SemanticPatch, out: &mut Vec<&'a SemanticPatch>) {
    match patch {
        SemanticPatch::Compound { patches } => patches.iter().for_each(|p| flatten(p, out)),
        leaf => out.push(leaf),
    }
}

/// The store vocabulary of a [`ProposalKind`].
pub(crate) fn store_kind(kind: ProposalKind) -> WorkflowProposalKind {
    match kind {
        ProposalKind::Link => WorkflowProposalKind::Link,
        ProposalKind::Merge => WorkflowProposalKind::Merge,
        ProposalKind::Supersede => WorkflowProposalKind::Supersede,
        ProposalKind::Create => WorkflowProposalKind::Create,
        ProposalKind::Accept => WorkflowProposalKind::Accept,
    }
}

/// 409 `E_PROPOSAL_ACTION_MISMATCH` unless `actual` is `required`.
pub(crate) fn require_kind(
    proposal: &Proposal,
    actual: ProposalKind,
    required: ProposalKind,
) -> Result<(), ApiError> {
    if actual == required {
        return Ok(());
    }
    Err(ApiError::conflict(
        "E_PROPOSAL_ACTION_MISMATCH",
        "the proposal's action class does not match the requested action",
    )
    .with_details(json!({
        "proposal_ref": proposal.id,
        "kind": wire(&actual)?,
        "requested": wire(&required)?,
    })))
}

// ============================================================================ identities

/// `ses:<first 16 hex of SHA-256(RFC 8785 {project_id, scope_refs, owner_actor_ref,
/// assistant_actor_ref, created_at})>` (Hotfix 065). Sessions are workflow state, not PSG
/// semantics, so the identity lives with the application workflow.
pub(crate) fn session_id(
    project_id: &Id,
    scope_refs: &[Id],
    owner_actor_ref: &Id,
    assistant_actor_ref: &Id,
    created_at: &Timestamp,
) -> Result<Id, ApiError> {
    let bytes = to_canonical_json(&json!({
        "project_id": project_id,
        "scope_refs": scope_refs,
        "owner_actor_ref": owner_actor_ref,
        "assistant_actor_ref": assistant_actor_ref,
        "created_at": created_at,
    }))
    .map_err(|_| ApiError::internal("session_id"))?;
    let digest = Hash::content_sha256(&bytes);
    format!("ses:{}", &digest.as_str()["sha256:".len()..][..16])
        .parse()
        .map_err(|_| ApiError::internal("session_id"))
}

// ============================================================================ stored records

/// The canonical JSON value of `value` for the workflow store.
pub(crate) fn stored_json<T: Serialize>(value: &T) -> Result<Value, ApiError> {
    serde_json::to_value(value).map_err(|_| ApiError::internal("workflow_json"))
}

/// Deserializes stored canonical text and requires it to be exactly the RFC 8785 encoding of
/// the typed value.
pub(crate) fn decode_canonical<T: DeserializeOwned + Serialize>(
    text: &str,
    what: &str,
) -> Result<T, ApiError> {
    let value: T = serde_json::from_str(text).map_err(|_| ApiError::workflow_corrupt(what))?;
    let canonical = to_canonical_json(&value).map_err(|_| ApiError::workflow_corrupt(what))?;
    if canonical != text.as_bytes() {
        return Err(ApiError::workflow_corrupt(what));
    }
    Ok(value)
}

/// A durable workflow proposal with its typed, validated material.
pub(crate) struct StoredProposal {
    pub(crate) record: WorkflowProposal,
    pub(crate) proposal: Proposal,
    pub(crate) intake: IntakeReport,
}

/// Decodes and validates a stored proposal; corrupt material is 500 `E_WORKFLOW_CORRUPT`.
pub(crate) fn decode_proposal(record: WorkflowProposal) -> Result<StoredProposal, ApiError> {
    let what = format!("proposal {}", record.proposal_id);
    let proposal: Proposal = decode_canonical(&record.proposal_json, &what)?;
    let intake: IntakeReport = decode_canonical(&record.intake_json, &what)?;
    if proposal.id != record.proposal_id
        || proposal.validate().is_err()
        || intake.validate().is_err()
        || intake.proposal_ref != proposal.id
    {
        return Err(ApiError::workflow_corrupt(&what));
    }
    Ok(StoredProposal {
        record,
        proposal,
        intake,
    })
}

/// The Accepted proposal-acceptance decisions of `proposal_ref` in `graph`.
pub(crate) fn acceptance_decisions(graph: &Graph, proposal_ref: &Id) -> Vec<(Id, Timestamp)> {
    graph
        .node_ids_by_type(NodeType::ResolutionDecision)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| n.status == ElementStatus::Accepted)
        .filter_map(|n| match &n.payload {
            NodePayload::ResolutionDecision(d)
                if d.proposal_ref.as_ref() == Some(proposal_ref)
                    && d.answer.get("kind").and_then(Value::as_str)
                        == Some(ACCEPTANCE_DECISION_KIND) =>
            {
                Some((n.id.clone(), d.decided_at))
            }
            _ => None,
        })
        .collect()
}

// ============================================================================ intake projection

/// The public projection of an X1.3 intake report (report data only).
pub(crate) fn intake_view(report: &IntakeReport) -> Result<IntakeReportView, ApiError> {
    let mut conflicts_with: Vec<ConflictView> = report
        .declared_conflicts
        .iter()
        .map(|c| ConflictView {
            target_ref: c.to_ref.to_string(),
            reason: "declared_conflict".to_owned(),
        })
        .collect();
    let mut validation_findings = Vec::with_capacity(report.validation_findings.len());
    for finding in &report.validation_findings {
        if is_validation_conflict(finding) {
            conflicts_with.push(ConflictView {
                target_ref: finding.id.to_string(),
                reason: finding.payload.code.clone(),
            });
        }
        validation_findings.push(Finding {
            id: finding.id.to_string(),
            code: finding.payload.code.clone(),
            family: finding.payload.family.clone(),
            severity: wire(&finding.payload.severity)?,
            status: finding.payload.status.clone(),
            message: finding.payload.message.clone(),
            affected_refs: finding
                .payload
                .affected_refs
                .iter()
                .map(Id::to_string)
                .collect(),
            waiver_ref: finding.payload.waiver_ref.as_ref().map(Id::to_string),
        });
    }
    let reason_codes = report
        .reason_codes
        .iter()
        .map(|c| parse_wire::<IntakeReasonCode>(c.as_str(), "reason code"))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ApiError::internal("intake_projection"))?;
    Ok(IntakeReportView {
        covered_by: report
            .covered_by
            .iter()
            .map(|c| CoverageView {
                subject_ref: c.subject_ref.to_string(),
                covered_by_ref: c.covered_by_ref.to_string(),
                score: c.score,
            })
            .collect(),
        conflicts_with,
        refines: report
            .refines
            .iter()
            .map(|r| r.to_ref.to_string())
            .collect(),
        new_terms: report
            .term_resolutions
            .iter()
            .filter(|t| t.status == TermResolutionStatus::New)
            .map(|t| t.term_ref.to_string())
            .collect(),
        novelty: report.novelty,
        duplicate_candidates: report
            .duplicate_candidates
            .iter()
            .map(|d| DuplicateCandidateView {
                subject_ref: d.subject_ref.to_string(),
                candidate_ref: d.candidate_ref.to_string(),
                score: d.score,
            })
            .collect(),
        validation_findings,
        reason_codes,
        requires_reason: report.requires_reason,
    })
}
