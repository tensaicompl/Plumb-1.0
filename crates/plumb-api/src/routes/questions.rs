//! Question and round routes (plan A0.3, Hotfix 065). The Question API projects only persisted
//! Accepted Questions; answering builds the existing S3.3 resolution proposal and persists it as
//! a pending workflow proposal without committing; a round targets one stakeholder through the
//! S3.2 kernel and is a projection over `Question.round_ref`. U2.2 (Hotfix 076) returns the
//! authoritative workflow class of the answer's pending Proposal.

use std::collections::BTreeMap;

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plumb_artifacts::{ArtifactKind, ArtifactStore};
use plumb_core::Id;
use plumb_functional::resolution::{
    resolve_question, ResolutionContext, ResolutionError, ResolutionInput,
};
use plumb_psg::{ElementStatus, NodeType};
use plumb_store::WorkflowOrigin;

use crate::application::round_size;
use crate::error::ApiError;
use crate::openapi::{
    AnswerRequest, AnswerResponse, CreateRoundRequest, QuestionPage, QuestionStatus,
};
use crate::state::{
    check_if_match, paginate, parse_body, parse_id, parse_wire, read_json, run_intake, ApiState,
    PageRequest, QueryParams,
};
use crate::workflow::{classify, intake_view};

use super::proposals::{persist_pending, Origin};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/questions", get(list_questions))
        .route("/api/questions/{id}/answer", post(answer_question))
        .route("/api/rounds", post(create_round))
        .route("/api/rounds/{id}", get(get_round))
}

/// The variant identifier of a domain error: its stable name, never its payload.
pub(super) fn error_name<T: std::fmt::Debug>(error: &T) -> String {
    format!("{error:?}")
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// `list_questions`: persisted Accepted Questions filtered by stakeholder, round and status.
async fn list_questions(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &["stakeholder", "round", "status"], true)?;
    let id_filter = |key: &str| -> Result<Option<Id>, ApiError> {
        query
            .get(key)
            .map(|v| parse_id(v).ok_or_else(|| ApiError::invalid(format!("{key} is not an ID"))))
            .transpose()
    };
    let stakeholder = id_filter("stakeholder")?;
    let round = id_filter("round")?;
    let status = query
        .get("status")
        .map(|s| parse_wire::<QuestionStatus>(s, "status"))
        .transpose()?;
    let loaded = state.load_head()?;
    let mut items = Vec::new();
    for view in state.questions(&loaded)? {
        if stakeholder
            .as_ref()
            .is_some_and(|s| view.stakeholder_ref != Some(s.to_string()))
            || round
                .as_ref()
                .is_some_and(|r| view.round_ref != Some(r.to_string()))
            || status.is_some_and(|s| s != view.status)
        {
            continue;
        }
        items.push((view.id.clone(), view));
    }
    let request = PageRequest {
        operation_id: "list_questions",
        path: Default::default(),
        query: &query,
    };
    let (items, next_cursor) = paginate(items, &request, &loaded)?;
    Ok(read_json(&loaded, QuestionPage { items, next_cursor }))
}

/// `answer_question`: the S3.3 resolution proposal of one answer, persisted as a pending
/// workflow proposal; nothing is committed. The response carries the authoritative workflow
/// class of that exact Proposal (Hotfix 076), classified before any artifact, intake or
/// workflow persistence so a classifier error leaves nothing behind.
async fn answer_question(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: AnswerRequest = parse_body(&body)?;
    if !request.answer.is_object() {
        return Err(ApiError::invalid("answer must be a JSON object"));
    }
    let supersedes = request
        .supersedes
        .as_deref()
        .map(|s| parse_id(s).ok_or_else(|| ApiError::invalid("supersedes is not an ID")))
        .transpose()?;
    let runtime = state.runtime("workflow_store")?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let question_ref =
        parse_id(&id).ok_or_else(|| ApiError::not_found(format!("question {id} not found")))?;
    // Invariant bindings come only from injected scopes owned by Accepted Invariants.
    let invariant_bindings: BTreeMap<Id, Vec<_>> = runtime
        .expression_scopes
        .iter()
        .filter(|scope| {
            loaded.graph.node(&scope.owner_ref).is_some_and(|n| {
                n.status == ElementStatus::Accepted && n.payload.node_type() == NodeType::Invariant
            })
        })
        .map(|scope| (scope.owner_ref.clone(), scope.bindings.clone()))
        .collect();
    let result = resolve_question(
        &loaded.graph,
        &ResolutionInput {
            question_ref: question_ref.clone(),
            answer: request.answer,
            decided_by: state.actor_ref().clone(),
            decided_at: created_at,
            rationale: request.rationale,
            supersedes,
        },
        &ResolutionContext { invariant_bindings },
    )
    .map_err(|error| match error {
        ResolutionError::QuestionMissing(id) | ResolutionError::NotAQuestion(id) => {
            ApiError::not_found(format!("question {id} not found"))
        }
        other => ApiError::semantic(&error_name(&other), other.to_string()),
    })?;
    let action = classify(&loaded.graph, &result.proposal)?;
    let stored = state.with_artifact_store(|store| {
        store.put(
            ArtifactKind::Patch,
            "application/json",
            &result.patch_artifact_bytes,
            created_at,
        )
    })?;
    match stored {
        Ok(hash) if hash == result.decision.patch_ref => {}
        _ => return Err(ApiError::internal("artifact_store")),
    }
    let report = run_intake(&loaded, &result.proposal)?;
    let persisted = persist_pending(
        &state,
        &loaded,
        &result.proposal,
        &report,
        Origin {
            kind: WorkflowOrigin::Question,
            reference: Some(question_ref),
        },
        created_at,
    )?;
    let revision = loaded.revision.id.to_string();
    Ok(Json(AnswerResponse {
        version: revision.clone(),
        revision,
        semantic_hash: loaded.revision.semantic_hash.to_string(),
        events: Vec::new(),
        proposal_id: persisted.proposal.id.to_string(),
        action,
        intake: intake_view(&persisted.intake)?,
        patch_preview: serde_json::to_value(&persisted.proposal.patch_set.patch)
            .map_err(|_| ApiError::internal("patch_preview"))?,
    })
    .into_response())
}

/// `create_round`: the transport of the shared round service (stakeholder and size are
/// validated before If-Match).
async fn create_round(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: CreateRoundRequest = parse_body(&body)?;
    let stakeholder = parse_id(&request.stakeholder)
        .ok_or_else(|| ApiError::invalid("stakeholder is not an ID"))?;
    let size = round_size(request.max)?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let body = state.create_round(&loaded, &stakeholder, size)?;
    Ok(Json(body).into_response())
}

/// `get_round`: the shared reconstruction of round `id` on the head.
async fn get_round(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let round = state.round(&loaded, &id)?;
    Ok(read_json(&loaded, round))
}
