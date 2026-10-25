//! Session routes (plan A0.3, Hotfix 065). A session is durable workflow state owned by its
//! creator. A turn runs the existing X1.4 pipeline once on one baseline, calls the injected
//! provider outside every store lock, records a prepared turn and its generated proposals, and
//! commits the user and assistant transcript material in one non-semantic CAS; the generated
//! proposals become pending only after that commit, bound to the post-transcript revision. A
//! lost race fails the turn; an interrupted turn is reconciled from its exact transcript nodes.

use std::collections::BTreeSet;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plumb_core::{Id, StageId};
use plumb_inference::persist_inference_bundle;
use plumb_intake::{IntakeReport, IntakeValidationInput};
use plumb_patch::{PatchSet, SemanticPatch};
use plumb_psg::{ElementStatus, NodeType};
use plumb_session::{
    complete_turn, inspect_turn_output, prepare_turn, Session, SessionIntakeInputs, SessionUserTurn,
};
use plumb_store::{
    LoadedRevision, NewWorkflowProposal, NewWorkflowSession, NewWorkflowTurn, StoreError,
    WorkflowInsert, WorkflowOrigin, WorkflowSession, WorkflowTurn, WorkflowTurnState,
};
use serde_json::json;

use crate::error::ApiError;
use crate::openapi::{
    CreateSessionRequest, CreateSessionResponse, SessionDetail, SessionMetrics,
    SessionProposalView, SessionStage, SessionTurnRequest, SessionTurnResponse,
};
use crate::state::{check_if_match, parse_body, parse_id, read_json, wire, ApiState};
use crate::workflow::{
    classify, decode_canonical, decode_proposal, intake_view, session_id, store_kind, stored_json,
};

use super::proposals::workflow_error;
use super::questions::error_name;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/{id}", get(get_session))
        .route("/api/sessions/{id}/metrics", get(get_session_metrics))
        .route("/api/sessions/{id}/turns", post(create_session_turn))
}

/// The exact StageId of a session stage.
fn stage_id(stage: SessionStage) -> StageId {
    match stage {
        SessionStage::S0 => StageId::S0,
        SessionStage::S1 => StageId::S1,
        SessionStage::S2 => StageId::S2,
        SessionStage::S3 => StageId::S3,
        SessionStage::S4 => StageId::S4,
        SessionStage::S5 => StageId::S5,
        SessionStage::S6 => StageId::S6,
        SessionStage::S7 => StageId::S7,
        SessionStage::S8 => StageId::S8,
        SessionStage::S9 => StageId::S9,
        SessionStage::S10 => StageId::S10,
        SessionStage::S11 => StageId::S11,
        SessionStage::S12 => StageId::S12,
    }
}

/// `create_session`: a durable session over an Accepted scope, owned by the injected identity.
async fn create_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: CreateSessionRequest = parse_body(&body)?;
    let mut scope = request
        .scope
        .iter()
        .map(|s| {
            parse_id(s).ok_or_else(|| {
                ApiError::invalid(format!("scope {} is not an ID", crate::error::echo(s)))
            })
        })
        .collect::<Result<Vec<Id>, _>>()?;
    let runtime = state.runtime("workflow_store")?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    if !state.inference_available() {
        return Err(ApiError::conflict(
            "E_INFERENCE_UNAVAILABLE",
            "sessions need an inference provider, and the null provider is configured",
        ));
    }
    let created_at = state.now();
    for id in &scope {
        let node = loaded
            .graph
            .node(id)
            .ok_or_else(|| ApiError::not_found(format!("scope node {id} not found")))?;
        if node.status != ElementStatus::Accepted {
            return Err(ApiError::semantic(
                "ScopeNotAccepted",
                format!("scope node {id} is not Accepted"),
            ));
        }
    }
    scope.sort();
    scope.dedup();
    let assistant = &runtime.assistant_actor_ref;
    let assistant_ok = loaded.graph.node(assistant).is_some_and(|n| {
        n.status == ElementStatus::Accepted && n.payload.node_type() == NodeType::Agent
    });
    if !assistant_ok {
        return Err(ApiError::semantic(
            "AssistantNotAccepted",
            "the configured assistant is not an Accepted Agent",
        ));
    }
    let owner = state.actor_ref().clone();
    let id = session_id(
        loaded.graph.project_id(),
        &scope,
        &owner,
        assistant,
        &created_at,
    )?;
    let session = Session::new(
        id.clone(),
        loaded.graph.project_id().clone(),
        scope,
        assistant.clone(),
    )
    .map_err(|e| ApiError::semantic(&error_name(&e), e.to_string()))?;
    let record = NewWorkflowSession {
        session_id: id.clone(),
        session_json: stored_json(&session)?,
        owner_actor_ref: owner,
        created_at,
    };
    let inserted = state
        .with_workflow_store(|store| store.insert_session(&record))?
        .map_err(workflow_error)?;
    if let WorkflowInsert::Existing(existing) = inserted {
        let stored: Session = decode_canonical(&existing.session_json, &format!("session {id}"))?;
        if stored != session || existing.owner_actor_ref != record.owner_actor_ref {
            return Err(ApiError::conflict(
                "E_ID_COLLISION",
                "the session identity is already used",
            )
            .with_details(json!({ "id": id })));
        }
    }
    let revision = loaded.revision.id.to_string();
    Ok(Json(CreateSessionResponse {
        version: revision.clone(),
        revision,
        semantic_hash: loaded.revision.semantic_hash.to_string(),
        events: Vec::new(),
        id: id.to_string(),
    })
    .into_response())
}

/// The durable session `id` owned by the current actor: 404 when unknown, 403 otherwise.
fn owned_session(state: &ApiState, id: &str) -> Result<(WorkflowSession, Session), ApiError> {
    let missing = || ApiError::not_found(format!("session {id} not found"));
    let session_ref = parse_id(id).ok_or_else(missing)?;
    let record = state
        .with_workflow_store(|store| store.session(&session_ref))?
        .map_err(workflow_error)?
        .ok_or_else(missing)?;
    if &record.owner_actor_ref != state.actor_ref() {
        return Err(ApiError::forbidden(
            "E_SESSION_NOT_OWNED",
            "the session belongs to another actor",
        ));
    }
    let session: Session = decode_canonical(&record.session_json, &format!("session {id}"))?;
    if session.id != record.session_id {
        return Err(ApiError::workflow_corrupt(&format!("session {id}")));
    }
    Ok((record, session))
}

/// Whether `id` exists in `loaded` with the transcript node type its prefix names.
fn transcript_present(loaded: &LoadedRevision, id: &Id) -> bool {
    let expected = if id.as_str().starts_with("src:") {
        NodeType::SourceArtifact
    } else {
        NodeType::EvidenceFragment
    };
    loaded
        .graph
        .node(id)
        .is_some_and(|n| n.payload.node_type() == expected)
}

/// Reconciles every prepared turn of `session` not in flight in this instance: all expected
/// transcript nodes present promote it (proposals bound to the current revision), none present
/// fail it, and a partial set is corrupt workflow state. Returns the turns afterwards.
fn reconciled_turns(
    state: &ApiState,
    loaded: &LoadedRevision,
    session: &Id,
) -> Result<Vec<WorkflowTurn>, ApiError> {
    let runtime = state.runtime("workflow_store")?;
    if runtime.turn_in_flight(session)? {
        return state
            .with_workflow_store(|store| store.turns(session))?
            .map_err(workflow_error);
    }
    reconcile_turns(state, loaded, session)
}

/// Settles the Prepared turns of `session` against `loaded` (the caller holds no other live
/// turn of it): committed when every expected node is present, failed when none is.
fn reconcile_turns(
    state: &ApiState,
    loaded: &LoadedRevision,
    session: &Id,
) -> Result<Vec<WorkflowTurn>, ApiError> {
    let turns = state
        .with_workflow_store(|store| store.turns(session))?
        .map_err(workflow_error)?;
    let mut out = Vec::with_capacity(turns.len());
    for turn in turns {
        if turn.state != WorkflowTurnState::Prepared {
            out.push(turn);
            continue;
        }
        let present = turn
            .expected_node_refs
            .iter()
            .filter(|id| transcript_present(loaded, id))
            .count();
        let index = turn.turn_index;
        let updated = if present == turn.expected_node_refs.len() {
            state.with_workflow_store(|store| {
                store.commit_turn(session, index, &loaded.revision.id)
            })?
        } else if present == 0 {
            state.with_workflow_store(|store| store.fail_turn(session, index, "INTERRUPTED"))?
        } else {
            return Err(ApiError::workflow_corrupt(&format!(
                "turn {session}#{index}"
            )));
        };
        out.push(updated.map_err(workflow_error)?);
    }
    Ok(out)
}

/// `get_session`.
async fn get_session(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let (_, session) = owned_session(&state, &id)?;
    let loaded = state.load_head()?;
    reconciled_turns(&state, &loaded, &session.id)?;
    Ok(read_json(
        &loaded,
        SessionDetail {
            id: session.id.to_string(),
            scope: session.scope_refs.iter().map(Id::to_string).collect(),
            assistant_actor_ref: session.assistant_actor_ref.to_string(),
        },
    ))
}

/// `get_session_metrics`: committed turns only, their generated proposals and those whose
/// stored intake report requires a reason.
async fn get_session_metrics(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let (_, session) = owned_session(&state, &id)?;
    let loaded = state.load_head()?;
    let turns = reconciled_turns(&state, &loaded, &session.id)?;
    let mut metrics = SessionMetrics {
        turns: 0,
        proposals: 0,
        requires_reason: 0,
    };
    for turn in turns
        .iter()
        .filter(|t| t.state == WorkflowTurnState::Committed)
    {
        metrics.turns += 1;
        for proposal_id in &turn.proposal_ids {
            let record = state
                .with_workflow_store(|store| store.proposal(proposal_id))?
                .map_err(workflow_error)?
                .ok_or_else(|| ApiError::workflow_corrupt(&format!("proposal {proposal_id}")))?;
            let stored = decode_proposal(record)?;
            metrics.proposals += 1;
            if stored.intake.requires_reason {
                metrics.requires_reason += 1;
            }
        }
    }
    Ok(read_json(&loaded, metrics))
}

/// 502 `E_INFERENCE_FAILED` naming the failing step's stable error.
fn inference_failed<E: std::fmt::Debug>(error: &E) -> ApiError {
    ApiError::inference_failed(&error_name(error))
}

/// `create_session_turn`: one X1.4 turn on one baseline (see the module documentation).
async fn create_session_turn(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: SessionTurnRequest = parse_body(&body)?;
    if request.text.trim().is_empty() {
        return Err(ApiError::invalid("text must not be blank"));
    }
    let runtime = state.runtime("inference_provider")?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let (_, session) = owned_session(&state, &id)?;
    // Claimed before any workflow write or provider call; a concurrent turn is 409.
    let _in_flight = runtime.begin_turn(&session.id)?;
    reconcile_turns(&state, &loaded, &session.id)?;
    let turn = SessionUserTurn {
        text: request.text,
        user_actor_ref: state.actor_ref().clone(),
        active_stage: stage_id(request.stage),
        provider_policy: runtime.provider_policy.clone(),
    };
    let plan = state
        .with_artifact_store(|store| {
            prepare_turn(
                &loaded.graph,
                &session,
                &turn,
                &runtime.session_turn_config,
                store,
                state.clock(),
            )
        })?
        .map_err(|e| ApiError::semantic(&error_name(&e), e.to_string()))?;
    // The provider runs off the async executor and outside every store lock, with the request
    // and the exact material of this same plan (Hotfix 066).
    let provider = runtime.inference_provider.clone();
    let inference_request = plan.request.clone();
    let inference_material = plan.material.clone();
    let execution = tokio::task::spawn_blocking(move || {
        provider.execute(&inference_request, &inference_material)
    })
    .await
    .map_err(|_| ApiError::internal("inference_task"))?
    .map_err(|e| inference_failed(&e))?;
    let persisted = state
        .with_artifact_store(|store| {
            persist_inference_bundle(store, &plan.request, &execution, created_at)
        })?
        .map_err(|e| inference_failed(&e))?;
    let inspected = state
        .with_artifact_store(|store| inspect_turn_output(&loaded.graph, &plan, store, &persisted))?
        .map_err(|e| inference_failed(&e))?;
    let inputs = SessionIntakeInputs::new(
        inspected
            .proposals
            .iter()
            .map(|p| (p.id.clone(), IntakeValidationInput::empty())),
    )
    .map_err(|e| inference_failed(&e))?;
    let result = state
        .with_artifact_store(|store| {
            complete_turn(
                &loaded.graph,
                &session,
                &plan,
                &inspected,
                &inputs,
                store,
                state.clock(),
            )
        })?
        .map_err(|e| inference_failed(&e))?;

    // X1.4 already ran intake on the transcript patches; their reports gate the commit.
    let transcripts: Vec<(&plumb_patch::Proposal, &IntakeReport)> = [
        (
            result.user_transcript_proposal.as_ref(),
            result.user_transcript_intake.as_ref(),
        ),
        (
            result.assistant_transcript_proposal.as_ref(),
            result.assistant_transcript_intake.as_ref(),
        ),
    ]
    .into_iter()
    .filter_map(|(p, r)| p.zip(r))
    .collect();
    if let Some((proposal, report)) = transcripts.iter().find(|(_, r)| r.requires_reason) {
        return Err(crate::state::reason_required(proposal, report));
    }

    // The prepared turn and its generated proposals are durable before any CAS.
    let mut proposals = Vec::with_capacity(result.proposals.len());
    let mut views = Vec::with_capacity(result.proposals.len());
    for card in &result.proposals {
        let kind = classify(&loaded.graph, &card.proposal)?;
        proposals.push(NewWorkflowProposal {
            proposal_id: card.proposal.id.clone(),
            base_revision_id: loaded.revision.id.clone(),
            proposal_json: stored_json(&card.proposal)?,
            intake_json: stored_json(&card.intake)?,
            proposal_kind: store_kind(kind),
            origin_kind: WorkflowOrigin::Session,
            origin_ref: Some(session.id.clone()),
            created_at,
        });
        views.push(SessionProposalView {
            id: card.proposal.id.to_string(),
            kind,
            payload: serde_json::to_value(&card.proposal.patch_set.patch)
                .map_err(|_| ApiError::internal("proposal_payload"))?,
            intake: intake_view(&card.intake, state.profile())?,
        });
    }
    let expected: BTreeSet<Id> = [&result.user_turn, &result.assistant_turn]
        .into_iter()
        .flat_map(|t| [t.source_node_ref.clone(), t.evidence_ref.clone()])
        .collect();
    let prepared = NewWorkflowTurn {
        session_id: session.id.clone(),
        base_revision_id: loaded.revision.id.clone(),
        stage: wire(&request.stage)?,
        expected_node_refs: expected.into_iter().collect(),
        proposal_ids: proposals.iter().map(|p| p.proposal_id.clone()).collect(),
        requires_reason_count: u32::try_from(
            result
                .proposals
                .iter()
                .filter(|c| c.intake.requires_reason)
                .count(),
        )
        .map_err(|_| ApiError::internal("session_turn"))?,
        created_at,
    };
    let turn_record = state
        .with_workflow_store(|store| store.prepare_turn(&prepared, &proposals))?
        .map_err(|error| match error {
            StoreError::WorkflowRecordExists { id, .. } => ApiError::conflict(
                "E_ID_COLLISION",
                "a generated proposal identity is already used",
            )
            .with_details(json!({ "id": id })),
            other => workflow_error(other),
        })?;
    let index = turn_record.turn_index;

    // One CAS for the present transcript patches (user first), at the turn baseline.
    let mut patches: Vec<SemanticPatch> = transcripts
        .iter()
        .map(|(p, _)| p.patch_set.patch.clone())
        .collect();
    let (write, bound) = if patches.is_empty() {
        let revision = loaded.revision.id.to_string();
        (
            crate::openapi::WriteResult {
                version: revision.clone(),
                revision,
                semantic_hash: loaded.revision.semantic_hash.to_string(),
                events: Vec::new(),
            },
            loaded.revision.id.clone(),
        )
    } else {
        let patch = if patches.len() == 1 {
            patches.remove(0)
        } else {
            SemanticPatch::Compound { patches }
        };
        let patch_set = PatchSet {
            base_semantic_hash: loaded.revision.semantic_hash.clone(),
            patch,
        };
        match state.commit_patch_set(
            &loaded,
            &patch_set,
            Vec::new(),
            created_at,
            "create_session_turn",
        ) {
            Ok(write) => {
                let bound = write
                    .revision
                    .parse()
                    .map_err(|_| ApiError::internal("revision_id"))?;
                (write, bound)
            }
            Err(error) => {
                let failure = if error.body().code == "E_PRECONDITION_FAILED" {
                    "STALE_BASE"
                } else {
                    error.body().code
                };
                state
                    .with_workflow_store(|store| store.fail_turn(&session.id, index, failure))?
                    .map_err(workflow_error)?;
                return Err(error);
            }
        }
    };
    state
        .with_workflow_store(|store| store.commit_turn(&session.id, index, &bound))?
        .map_err(workflow_error)?;
    Ok(Json(SessionTurnResponse {
        version: write.version,
        revision: write.revision,
        semantic_hash: write.semantic_hash,
        events: write.events,
        reply: result.reply,
        proposals: views,
        asked_question: result.asked_question_ref.as_ref().map(Id::to_string),
    })
    .into_response())
}
