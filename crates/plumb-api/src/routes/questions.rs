//! Question and round routes (plan A0.3, Hotfix 065). The Question API projects only persisted
//! Accepted Questions; answering builds the existing S3.3 resolution proposal and persists it as
//! a pending workflow proposal without committing; a round targets one stakeholder through the
//! S3.2 kernel and is a projection over `Question.round_ref`.

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
use plumb_functional::{
    compose_question_round_for_stakeholder, parse_priority, round_id, QuestionRoundConfig,
    QuestionRoundDisposition, RoundError, MAX_QUESTION_ROUND_SIZE,
};
use plumb_psg::{ElementStatus, FindingSeverity, Graph, Node, NodePayload, NodeType};
use plumb_store::WorkflowOrigin;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::openapi::{
    AnswerRequest, AnswerResponse, CreateRoundRequest, CreateRoundResponse, Question, QuestionKind,
    QuestionPage, QuestionStatus, Round,
};
use crate::state::{
    check_if_match, paginate, parse_body, parse_id, parse_wire, read_json, run_intake, wire,
    ApiState, PageRequest, QueryParams,
};
use crate::workflow::intake_view;

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

fn projection_invalid(question: &Id, field: &str, value: Value) -> ApiError {
    ApiError::conflict(
        "E_QUESTION_PROJECTION_INVALID",
        "a persisted Question is outside the pilot projection",
    )
    .with_details(json!({ "question": question, "field": field, "value": value }))
}

/// The view of one persisted Accepted Question; malformed material is a typed conflict.
fn question_view(node: &Node, q: &plumb_psg::Question) -> Result<Question, ApiError> {
    let status: QuestionStatus = serde_json::from_value(Value::String(q.status.clone()))
        .map_err(|_| projection_invalid(&node.id, "status", json!(q.status)))?;
    if q.answer_schema.as_ref().is_some_and(|s| !s.is_object()) {
        return Err(projection_invalid(
            &node.id,
            "answer_schema",
            q.answer_schema.clone().unwrap_or(Value::Null),
        ));
    }
    let priority = match &q.priority {
        None => None,
        Some(text) => Some(
            parse_priority(text)
                .ok_or_else(|| projection_invalid(&node.id, "priority", json!(text)))?,
        ),
    };
    let kind: QuestionKind = parse_wire(&wire(&q.question_kind)?, "question kind")
        .map_err(|_| ApiError::internal("question_kind"))?;
    Ok(Question {
        id: node.id.to_string(),
        kind,
        text: q.prompt.clone(),
        status,
        answer_schema: q.answer_schema.clone(),
        stakeholder_ref: q.stakeholder_ref.as_ref().map(Id::to_string),
        priority,
        round_ref: q.round_ref.as_ref().map(Id::to_string),
        context_refs: q.context_refs.iter().flatten().map(Id::to_string).collect(),
    })
}

/// Every Accepted Question node with its payload.
fn accepted_questions(graph: &Graph) -> impl Iterator<Item = (&Node, &plumb_psg::Question)> {
    graph
        .node_ids_by_type(NodeType::Question)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| n.status == ElementStatus::Accepted)
        .filter_map(|n| match &n.payload {
            NodePayload::Question(q) => Some((n, q)),
            _ => None,
        })
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
    for (node, q) in accepted_questions(&loaded.graph) {
        let view = question_view(node, q)?;
        if stakeholder
            .as_ref()
            .is_some_and(|s| q.stakeholder_ref.as_ref() != Some(s))
            || round
                .as_ref()
                .is_some_and(|r| q.round_ref.as_ref() != Some(r))
            || status.is_some_and(|s| s != view.status)
        {
            continue;
        }
        items.push((node.id.to_string(), view));
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
/// workflow proposal; nothing is committed.
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
        intake: intake_view(&persisted.intake)?,
        patch_preview: serde_json::to_value(&persisted.proposal.patch_set.patch)
            .map_err(|_| ApiError::internal("patch_preview"))?,
    })
    .into_response())
}

/// `create_round`: one targeted round for an Accepted Stakeholder through the S3.2 kernel; the
/// non-semantic round assignment is committed and the composed round returned.
async fn create_round(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: CreateRoundRequest = parse_body(&body)?;
    let stakeholder = parse_id(&request.stakeholder)
        .ok_or_else(|| ApiError::invalid("stakeholder is not an ID"))?;
    let size = usize::try_from(request.max)
        .ok()
        .filter(|n| (1..=MAX_QUESTION_ROUND_SIZE).contains(n))
        .ok_or_else(|| {
            ApiError::invalid(format!("max must be within 1..={MAX_QUESTION_ROUND_SIZE}"))
        })?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let result = compose_question_round_for_stakeholder(
        &loaded.graph,
        &stakeholder,
        &QuestionRoundConfig {
            question_round_size: size,
        },
    )
    .map_err(|error| match error {
        RoundError::StakeholderMissing(id) => {
            ApiError::not_found(format!("stakeholder {id} not found"))
        }
        RoundError::StakeholderInvalid(id) => ApiError::conflict(
            "E_ROUND_STAKEHOLDER_INVALID",
            "the stakeholder is not an Accepted Stakeholder",
        )
        .with_details(json!({ "stakeholder": id })),
        other => ApiError::conflict(
            "E_ROUND_COMPOSITION_FAILED",
            "the round cannot be composed from the current questions",
        )
        .with_details(json!({ "error": error_name(&other), "reason": other.to_string() })),
    })?;
    let failed = result.dispositions.iter().any(|d| {
        matches!(
            d,
            QuestionRoundDisposition::RoundIdCollision { .. }
                | QuestionRoundDisposition::InvalidRoundProposal { .. }
        )
    });
    let (Some(round), Some(proposal), false) =
        (result.rounds.first(), result.proposal.as_ref(), failed)
    else {
        if failed {
            return Err(ApiError::conflict(
                "E_ROUND_COMPOSITION_FAILED",
                "the round cannot be composed from the current questions",
            )
            .with_details(json!({
                "dispositions": serde_json::to_value(&result.dispositions)
                    .map_err(|_| ApiError::internal("dispositions"))?
            })));
        }
        return Err(ApiError::conflict(
            "E_NO_QUESTIONS_FOR_ROUND",
            "the stakeholder has no eligible unassigned question",
        )
        .with_details(json!({ "stakeholder": stakeholder })));
    };
    let write = state.write(&loaded, proposal, false, None, created_at, "create_round")?;
    Ok(Json(CreateRoundResponse {
        version: write.version,
        revision: write.revision,
        semantic_hash: write.semantic_hash,
        events: write.events,
        round: Round {
            id: round.id.to_string(),
            stakeholder: round.stakeholder_ref.to_string(),
            questions: round.question_refs.iter().map(Id::to_string).collect(),
        },
    })
    .into_response())
}

/// `get_round`: reconstructs the round from the Accepted Questions whose `round_ref` is the
/// path ID and verifies its stakeholder, ordering and deterministic identity.
async fn get_round(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let missing = || ApiError::not_found(format!("round {id} not found"));
    let round_ref = parse_id(&id).ok_or_else(missing)?;
    let inconsistent = |reason: &str| {
        ApiError::conflict(
            "E_ROUND_INCONSISTENT",
            "the round's questions do not reproduce its identity",
        )
        .with_details(json!({ "round": round_ref, "reason": reason }))
    };
    let mut members: Vec<(bool, u64, Id)> = Vec::new();
    let mut stakeholders = std::collections::BTreeSet::new();
    for (node, q) in accepted_questions(&loaded.graph) {
        if q.round_ref.as_ref() != Some(&round_ref) {
            continue;
        }
        let Some(stakeholder) = &q.stakeholder_ref else {
            return Err(inconsistent("a member has no stakeholder"));
        };
        stakeholders.insert(stakeholder.clone());
        let priority = q
            .priority
            .as_deref()
            .and_then(parse_priority)
            .ok_or_else(|| inconsistent("a member has no canonical priority"))?;
        let blocking = match loaded.graph.node(&q.finding_ref).map(|n| &n.payload) {
            Some(NodePayload::Finding(f)) => f.severity == FindingSeverity::Blocker,
            _ => return Err(inconsistent("a member's finding is missing")),
        };
        members.push((blocking, priority, node.id.clone()));
    }
    if members.is_empty() {
        return Err(missing());
    }
    let stakeholders: Vec<Id> = stakeholders.into_iter().collect();
    let [stakeholder] = stakeholders.as_slice() else {
        return Err(inconsistent("members have different stakeholders"));
    };
    members.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    let ordered: Vec<Id> = members.into_iter().map(|(_, _, id)| id).collect();
    let expected = round_id(loaded.graph.project_id(), stakeholder, &ordered)
        .map_err(|_| ApiError::internal("round_id"))?;
    if expected != round_ref {
        return Err(inconsistent(
            "the round ID does not match its ordered members",
        ));
    }
    Ok(read_json(
        &loaded,
        Round {
            id: round_ref.to_string(),
            stakeholder: stakeholder.to_string(),
            questions: ordered.iter().map(Id::to_string).collect(),
        },
    ))
}
