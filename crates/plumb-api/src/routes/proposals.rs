//! Proposal routes (plan A0.3, Hotfix 065): durable pending proposals bound to the exact
//! revision they were built on. Intake persists an already-built typed Proposal with its X1.3
//! report; link, merge, supersede, create and accept commit it unchanged (with the
//! proposal-acceptance companion decision) only when its derived action class matches, its
//! baseline revision is still the head and a fresh intake allows it. Nothing is rebased,
//! rewritten or retried.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use plumb_core::{Id, Timestamp};
use plumb_intake::IntakeReport;
use plumb_patch::Proposal;
use plumb_store::{
    LoadedRevision, NewWorkflowProposal, RevisionId, StoreError, WorkflowInsert, WorkflowOrigin,
    WorkflowProposal, WorkflowProposalState,
};
use serde_json::json;

use crate::error::ApiError;
use crate::openapi::{
    IntakeRequest, IntakeResponse, ProposalActionRequest, ProposalKind, WriteResult,
};
use crate::state::{
    check_if_match, parse_body, parse_id, require_clean_reason, run_intake, ApiState,
};
use crate::workflow::{
    acceptance_decisions, classify, decode_proposal, intake_view, require_kind, store_kind,
    stored_json, StoredProposal,
};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/proposals/intake", post(intake_proposal))
        .route("/api/proposals/{id}/link", post(link_proposal))
        .route("/api/proposals/{id}/merge", post(merge_proposal))
        .route("/api/proposals/{id}/supersede", post(supersede_proposal))
        .route("/api/proposals/{id}/create", post(create_proposal))
        .route("/api/proposals/{id}/accept", post(accept_proposal))
        .route("/api/proposals/{id}/reject", post(reject_proposal))
}

/// Maps a workflow-store failure: a corrupt record is 500 `E_WORKFLOW_CORRUPT`, anything else
/// unexpected is 500 `E_INTERNAL`.
pub(super) fn workflow_error(error: StoreError) -> ApiError {
    match error {
        StoreError::CorruptWorkflowRecord { kind, id, .. } => {
            ApiError::workflow_corrupt(&format!("{kind} {id}"))
        }
        StoreError::WorkflowStateConflict { id, actual, .. } => {
            ApiError::conflict("E_PROPOSAL_NOT_PENDING", "the proposal is not pending")
                .with_details(json!({ "proposal": id, "state": actual }))
        }
        _ => ApiError::internal("workflow_store"),
    }
}

/// 409 `E_PROPOSAL_NOT_PENDING`.
fn not_pending(record: &WorkflowProposal) -> ApiError {
    ApiError::conflict("E_PROPOSAL_NOT_PENDING", "the proposal is not pending")
        .with_details(json!({ "proposal": record.proposal_id, "state": record.state.as_str() }))
}

/// The store vocabulary of a workflow origin.
pub(super) struct Origin {
    pub(super) kind: WorkflowOrigin,
    pub(super) reference: Option<Id>,
}

/// Persists `proposal` and its X1.3 `report` as a pending workflow proposal bound to the loaded
/// revision. An identical pending proposal is idempotent (its stored record is returned); the
/// same ID with different bytes is corrupt; an identical non-pending one is not pending.
pub(super) fn persist_pending(
    state: &ApiState,
    loaded: &LoadedRevision,
    proposal: &Proposal,
    report: &IntakeReport,
    origin: Origin,
    created_at: Timestamp,
) -> Result<StoredProposal, ApiError> {
    let kind = classify(&loaded.graph, proposal)?;
    let record = NewWorkflowProposal {
        proposal_id: proposal.id.clone(),
        base_revision_id: loaded.revision.id.clone(),
        proposal_json: stored_json(proposal)?,
        intake_json: stored_json(report)?,
        proposal_kind: store_kind(kind),
        origin_kind: origin.kind,
        origin_ref: origin.reference,
        created_at,
    };
    let inserted = state
        .with_workflow_store(|store| {
            store.insert_proposal(&record, WorkflowProposalState::Pending)
        })?
        .map_err(workflow_error)?;
    match inserted {
        WorkflowInsert::Inserted(stored) => decode_proposal(stored),
        WorkflowInsert::Existing(stored) => {
            let existing = decode_proposal(stored)?;
            if existing.proposal != *proposal {
                return Err(ApiError::workflow_corrupt(&format!(
                    "proposal {}",
                    proposal.id
                )));
            }
            if existing.record.state != WorkflowProposalState::Pending {
                return Err(not_pending(&existing.record));
            }
            Ok(existing)
        }
    }
}

/// Loads and decodes the workflow proposal `id` (404 when unknown), then reconciles an
/// interrupted acceptance: a pending proposal whose acceptance decision is already in the graph
/// becomes accepted; several such decisions are corrupt workflow state.
pub(super) fn load_reconciled(
    state: &ApiState,
    loaded: &LoadedRevision,
    id: &str,
) -> Result<StoredProposal, ApiError> {
    let missing = || ApiError::not_found(format!("proposal {id} not found"));
    let proposal_id = parse_id(id).ok_or_else(missing)?;
    let record = state
        .with_workflow_store(|store| store.proposal(&proposal_id))?
        .map_err(workflow_error)?
        .ok_or_else(missing)?;
    let stored = decode_proposal(record)?;
    if stored.record.state != WorkflowProposalState::Pending {
        return Ok(stored);
    }
    let decisions = acceptance_decisions(&loaded.graph, &proposal_id);
    match decisions.as_slice() {
        [] => Ok(stored),
        [(_, decided_at)] => {
            let repaired = state
                .with_workflow_store(|store| {
                    store.resolve_proposal(
                        &proposal_id,
                        WorkflowProposalState::Accepted,
                        &loaded.revision.id,
                        *decided_at,
                    )
                })?
                .map_err(workflow_error)?;
            decode_proposal(repaired)
        }
        _ => Err(ApiError::workflow_corrupt(&format!(
            "proposal {proposal_id}"
        ))),
    }
}

/// `intake_proposal`: persists an already-built typed Proposal as pending without graph
/// mutation and returns its X1.3 intake projection.
async fn intake_proposal(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: IntakeRequest = parse_body(&body)?;
    let proposal: Proposal = serde_json::from_value(request.payload)
        .map_err(|e| ApiError::invalid(format!("payload is not a valid Proposal: {e}")))?;
    state.runtime("workflow_store")?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    if proposal.patch_set.base_semantic_hash != loaded.revision.semantic_hash {
        return Err(baseline_changed(&proposal.id));
    }
    let kind = classify(&loaded.graph, &proposal)?;
    require_kind(&proposal, kind, request.kind)?;
    let evidence: Vec<String> = proposal
        .evidence_refs
        .iter()
        .map(|e| e.as_str().to_owned())
        .collect();
    if request.evidence != evidence {
        return Err(ApiError::bad_request(
            "E_PROPOSAL_EVIDENCE_MISMATCH",
            "evidence must be exactly the proposal's sorted evidence references",
        ));
    }
    let report = run_intake(&loaded, &proposal)?;
    let stored = persist_pending(
        &state,
        &loaded,
        &proposal,
        &report,
        Origin {
            kind: WorkflowOrigin::Intake,
            reference: None,
        },
        created_at,
    )?;
    let revision = loaded.revision.id.to_string();
    Ok(Json(IntakeResponse {
        version: revision.clone(),
        revision,
        semantic_hash: loaded.revision.semantic_hash.to_string(),
        events: Vec::new(),
        intake: intake_view(&stored.intake)?,
    })
    .into_response())
}

/// 409 `E_PROPOSAL_BASELINE_CHANGED`.
fn baseline_changed(proposal: &Id) -> ApiError {
    ApiError::conflict(
        "E_PROPOSAL_BASELINE_CHANGED",
        "the proposal was built on another revision; it is never rebased",
    )
    .with_details(json!({ "proposal_ref": proposal }))
}

/// The optional clean reason of a proposal action.
fn action_reason(body: &Bytes) -> Result<Option<String>, ApiError> {
    let request: ProposalActionRequest = parse_body(body)?;
    if let Some(reason) = &request.reason {
        require_clean_reason(reason)?;
    }
    Ok(request.reason)
}

/// Shared link/merge/supersede/create/accept: commits the stored proposal unchanged with its
/// proposal-acceptance decision when its derived class is `required`.
async fn act(
    state: ApiState,
    id: String,
    headers: HeaderMap,
    body: Bytes,
    required: ProposalKind,
    operation_id: &'static str,
) -> Result<Response, ApiError> {
    let reason = action_reason(&body)?;
    state.runtime("workflow_store")?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let stored = load_reconciled(&state, &loaded, &id)?;
    if stored.record.state != WorkflowProposalState::Pending {
        return Err(not_pending(&stored.record));
    }
    if stored.record.base_revision_id != loaded.revision.id {
        return Err(baseline_changed(&stored.proposal.id));
    }
    let kind = classify(&loaded.graph, &stored.proposal)?;
    require_kind(&stored.proposal, kind, required)?;
    // Stored intake is history; acceptance re-runs X1.3 on the current head.
    let report = run_intake(&loaded, &stored.proposal)?;
    let write = state.write_with_report(
        &loaded,
        &stored.proposal,
        &report,
        true,
        reason.as_deref(),
        created_at,
        operation_id,
    )?;
    finalize(&state, &stored.proposal.id, &write, created_at)?;
    Ok(Json(write).into_response())
}

/// Moves the committed proposal pending -> accepted; an interruption here is reconciled later
/// from the acceptance decision in the graph.
fn finalize(
    state: &ApiState,
    proposal_id: &Id,
    write: &WriteResult,
    resolved_at: Timestamp,
) -> Result<(), ApiError> {
    let revision: RevisionId = write
        .revision
        .parse()
        .map_err(|_| ApiError::internal("revision_id"))?;
    state
        .with_workflow_store(|store| {
            store.resolve_proposal(
                proposal_id,
                WorkflowProposalState::Accepted,
                &revision,
                resolved_at,
            )
        })?
        .map_err(workflow_error)?;
    Ok(())
}

/// `link_proposal`.
async fn link_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(
        state,
        id,
        headers,
        body,
        ProposalKind::Link,
        "link_proposal",
    )
    .await
}

/// `merge_proposal`.
async fn merge_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(
        state,
        id,
        headers,
        body,
        ProposalKind::Merge,
        "merge_proposal",
    )
    .await
}

/// `supersede_proposal`.
async fn supersede_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(
        state,
        id,
        headers,
        body,
        ProposalKind::Supersede,
        "supersede_proposal",
    )
    .await
}

/// `create_proposal`.
async fn create_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(
        state,
        id,
        headers,
        body,
        ProposalKind::Create,
        "create_proposal",
    )
    .await
}

/// `accept_proposal`.
async fn accept_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(
        state,
        id,
        headers,
        body,
        ProposalKind::Accept,
        "accept_proposal",
    )
    .await
}

/// `reject_proposal`: pending -> rejected without changing the PSG.
async fn reject_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    action_reason(&body)?;
    state.runtime("workflow_store")?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let resolved_at = state.now();
    let stored = load_reconciled(&state, &loaded, &id)?;
    if stored.record.state != WorkflowProposalState::Pending {
        return Err(not_pending(&stored.record));
    }
    state
        .with_workflow_store(|store| {
            store.resolve_proposal(
                &stored.proposal.id,
                WorkflowProposalState::Rejected,
                &loaded.revision.id,
                resolved_at,
            )
        })?
        .map_err(workflow_error)?;
    let revision = loaded.revision.id.to_string();
    Ok(Json(WriteResult {
        version: revision.clone(),
        revision,
        semantic_hash: loaded.revision.semantic_hash.to_string(),
        events: Vec::new(),
    })
    .into_response())
}
