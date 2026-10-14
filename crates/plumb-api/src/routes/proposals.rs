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
    LoadedRevision, NewWorkflowProposal, WorkflowInsert, WorkflowOrigin, WorkflowProposalState,
};

pub(super) use crate::application::workflow_error;
use crate::application::{baseline_changed, not_pending, ProposalAction};
use crate::error::ApiError;
use crate::openapi::{IntakeRequest, IntakeResponse, ProposalActionRequest};
use crate::state::{check_if_match, parse_body, require_clean_reason, run_intake, ApiState};
use crate::workflow::{
    classify, decode_proposal, intake_view, require_kind, store_kind, stored_json, StoredProposal,
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
        intake: intake_view(&stored.intake, state.profile())?,
    })
    .into_response())
}

/// The optional clean reason of a proposal action.
fn action_reason(body: &Bytes) -> Result<Option<String>, ApiError> {
    let request: ProposalActionRequest = parse_body(body)?;
    if let Some(reason) = &request.reason {
        require_clean_reason(reason)?;
    }
    Ok(request.reason)
}

/// Shared transport of the six proposal actions: the reason is validated before the runtime
/// and If-Match checks; the action itself is the shared application service.
async fn act(
    state: ApiState,
    id: String,
    headers: HeaderMap,
    body: Bytes,
    action: ProposalAction,
) -> Result<Response, ApiError> {
    let reason = action_reason(&body)?;
    state.runtime("workflow_store")?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let write = state.act_on_proposal(&loaded, &id, action, reason)?;
    Ok(Json(write).into_response())
}

/// `link_proposal`.
async fn link_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(state, id, headers, body, ProposalAction::Link).await
}

/// `merge_proposal`.
async fn merge_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(state, id, headers, body, ProposalAction::Merge).await
}

/// `supersede_proposal`.
async fn supersede_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(state, id, headers, body, ProposalAction::Supersede).await
}

/// `create_proposal`.
async fn create_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(state, id, headers, body, ProposalAction::Create).await
}

/// `accept_proposal`.
async fn accept_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(state, id, headers, body, ProposalAction::Accept).await
}

/// `reject_proposal`: pending -> rejected without changing the PSG.
async fn reject_proposal(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    act(state, id, headers, body, ProposalAction::Reject).await
}
