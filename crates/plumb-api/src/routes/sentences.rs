//! Sentence routes (plan A0.3, Hotfix 065): a Sentence is a view of exactly one Requirement
//! node. Unknowns come only from Accepted open Questions whose context names the Requirement,
//! and entity membership follows only active `specified_by` and `reads`/`writes` relations.
//! Accept, reject and slot edits are ordinary human-confirm writes.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plumb_core::{Id, StageId};
use plumb_patch::{
    AcceptancePolicy, ElementPrecondition, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    node_element_hash, ElementStatus, Graph, Modality, Node, NodePayload, NodeType, RelationKind,
    Requirement, RequirementKind, RequirementLevel,
};
use plumb_store::LoadedRevision;
use serde_json::json;

use crate::error::ApiError;
use crate::openapi::{Sentence, SentenceEditRequest, SentencePage, SentenceSlots, SentenceUnknown};
use crate::state::{
    check_if_match, paginate, parse_body, parse_id, parse_wire, read_json, wire, ApiState,
    PageRequest, QueryParams,
};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/model/sentences", get(list_sentences))
        .route("/api/model/sentences/{id}/accept", post(accept_sentence))
        .route("/api/model/sentences/{id}/edit", post(edit_sentence))
        .route("/api/model/sentences/{id}/reject", post(reject_sentence))
}

fn active(status: ElementStatus) -> bool {
    matches!(status, ElementStatus::Proposed | ElementStatus::Accepted)
}

/// Targets of `node`'s active outgoing `kind` edges whose target node is active.
fn active_targets<'g>(graph: &'g Graph, node: &Id, kind: RelationKind) -> Vec<&'g Id> {
    graph
        .outgoing_edge_ids(node)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == kind && active(e.status))
        .filter(|e| graph.node(&e.to).is_some_and(|n| active(n.status)))
        .map(|e| &e.to)
        .collect()
}

/// Whether the Requirement belongs to `entity`: an active `specified_by` edge reaches an active
/// Operation with an active `reads` or `writes` edge to the Entity.
fn belongs_to(graph: &Graph, requirement: &Id, entity: &Id) -> bool {
    active_targets(graph, requirement, RelationKind::SpecifiedBy)
        .into_iter()
        .filter(|op| {
            graph
                .node(op)
                .is_some_and(|n| n.payload.node_type() == NodeType::Operation)
        })
        .any(|op| {
            [RelationKind::Reads, RelationKind::Writes]
                .into_iter()
                .any(|kind| active_targets(graph, op, kind).contains(&entity))
        })
}

/// The Sentence view of one Requirement node.
fn sentence(graph: &Graph, node: &Node, requirement: &Requirement) -> Result<Sentence, ApiError> {
    let mut unknowns: Vec<(String, String)> = Vec::new();
    for id in graph.node_ids_by_type(NodeType::Question) {
        let Some(question) = graph.node(id) else {
            continue;
        };
        if question.status != ElementStatus::Accepted {
            continue;
        }
        let NodePayload::Question(q) = &question.payload else {
            continue;
        };
        if q.status == "Open" && q.context_refs.iter().flatten().any(|r| r == &node.id) {
            unknowns.push((wire(&q.question_kind)?, question.id.to_string()));
        }
    }
    unknowns.sort();
    Ok(Sentence {
        id: node.id.to_string(),
        text: requirement.statement.clone(),
        slots: SentenceSlots {
            class: wire(&requirement.requirement_kind)?,
            level: wire(&requirement.level)?,
            modality: wire(&requirement.modality)?,
        },
        status: wire(&node.status)?,
        evidence: node
            .evidence
            .iter()
            .map(|e| e.as_id().to_string())
            .collect(),
        unknowns: unknowns
            .into_iter()
            .map(|(slot, question_id)| SentenceUnknown { slot, question_id })
            .collect(),
    })
}

/// `list_sentences`: every Requirement node, filtered by status and entity.
async fn list_sentences(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &["status", "entity"], true)?;
    let status = query
        .get("status")
        .map(|s| parse_wire::<ElementStatus>(s, "status"))
        .transpose()?;
    let loaded = state.load_head()?;
    let entity = match query.get("entity") {
        None => None,
        Some(text) => {
            let missing = || ApiError::not_found(format!("entity {text} not found"));
            let id = parse_id(text).ok_or_else(missing)?;
            loaded
                .graph
                .node(&id)
                .filter(|n| n.payload.node_type() == NodeType::Entity)
                .ok_or_else(missing)?;
            Some(id)
        }
    };
    let mut items = Vec::new();
    for id in loaded.graph.node_ids_by_type(NodeType::Requirement) {
        let Some(node) = loaded.graph.node(id) else {
            continue;
        };
        let NodePayload::Requirement(requirement) = &node.payload else {
            continue;
        };
        if status.is_some_and(|s| s != node.status)
            || entity
                .as_ref()
                .is_some_and(|e| !belongs_to(&loaded.graph, &node.id, e))
        {
            continue;
        }
        items.push((
            node.id.to_string(),
            sentence(&loaded.graph, node, requirement)?,
        ));
    }
    let request = PageRequest {
        operation_id: "list_sentences",
        path: Default::default(),
        query: &query,
    };
    let (items, next_cursor) = paginate(items, &request, &loaded)?;
    Ok(read_json(&loaded, SentencePage { items, next_cursor }))
}

/// The Requirement node behind sentence `id`, or 404.
fn requirement<'g>(graph: &'g Graph, id: &str) -> Result<(&'g Node, &'g Requirement), ApiError> {
    let missing = || ApiError::not_found(format!("sentence {id} not found"));
    let node = parse_id(id)
        .and_then(|i| graph.node(&i))
        .ok_or_else(missing)?;
    match &node.payload {
        NodePayload::Requirement(r) => Ok((node, r)),
        _ => Err(missing()),
    }
}

fn precondition(node: &Node) -> Result<ElementPrecondition, ApiError> {
    Ok(ElementPrecondition {
        id: node.id.clone(),
        expected_hash: node_element_hash(node)
            .map_err(|e| ApiError::internal_cause("element_hash", &e))?,
    })
}

/// An S1 Semantic HUMAN_CONFIRM proposal of `patch` on the loaded head.
fn confirm_proposal(loaded: &LoadedRevision, patch: SemanticPatch) -> Result<Proposal, ApiError> {
    Proposal::new(
        StageId::S1,
        PatchSet {
            base_semantic_hash: loaded.revision.semantic_hash.clone(),
            patch,
        },
        Vec::new(),
        Vec::new(),
        ProposalMateriality::Semantic,
        AcceptancePolicy::HumanConfirm,
        None,
    )
    .map_err(|e| ApiError::internal_cause("proposal", &e))
}

/// Shared accept/reject: SetStatus of a Proposed Requirement.
async fn set_sentence_status(
    state: ApiState,
    id: String,
    headers: HeaderMap,
    to: ElementStatus,
    operation_id: &'static str,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let (node, _) = requirement(&loaded.graph, &id)?;
    if node.status != ElementStatus::Proposed {
        return Err(ApiError::semantic(
            "SentenceNotProposed",
            "only a Proposed sentence can be accepted or rejected",
        )
        .with_details(json!({ "error": "SentenceNotProposed", "status": wire(&node.status)? })));
    }
    let proposal = confirm_proposal(
        &loaded,
        SemanticPatch::SetStatus {
            target: precondition(node)?,
            from: ElementStatus::Proposed,
            to,
        },
    )?;
    let write = state.write(&loaded, &proposal, false, None, created_at, operation_id)?;
    Ok(Json(write).into_response())
}

/// `accept_sentence`: Proposed -> Accepted.
async fn accept_sentence(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    set_sentence_status(
        state,
        id,
        headers,
        ElementStatus::Accepted,
        "accept_sentence",
    )
    .await
}

/// `reject_sentence`: Proposed -> Rejected.
async fn reject_sentence(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    set_sentence_status(
        state,
        id,
        headers,
        ElementStatus::Rejected,
        "reject_sentence",
    )
    .await
}

/// `edit_sentence`: replaces only the supplied slots (class, level, modality) of a Proposed or
/// Accepted Requirement; the statement is never edited.
async fn edit_sentence(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: SentenceEditRequest = parse_body(&body)?;
    let slots = request.slots;
    if slots.class.is_none() && slots.level.is_none() && slots.modality.is_none() {
        return Err(ApiError::invalid(
            "slots must name at least one of class, level and modality",
        ));
    }
    let class = slots
        .class
        .as_deref()
        .map(|v| parse_wire::<RequirementKind>(v, "class"))
        .transpose()?;
    let level = slots
        .level
        .as_deref()
        .map(|v| parse_wire::<RequirementLevel>(v, "level"))
        .transpose()?;
    let modality = slots
        .modality
        .as_deref()
        .map(|v| parse_wire::<Modality>(v, "modality"))
        .transpose()?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let (node, current) = requirement(&loaded.graph, &id)?;
    if !active(node.status) {
        return Err(ApiError::semantic(
            "SentenceNotActive",
            "only a Proposed or Accepted sentence can be edited",
        )
        .with_details(json!({ "error": "SentenceNotActive", "status": wire(&node.status)? })));
    }
    let mut payload = current.clone();
    if let Some(class) = class {
        payload.requirement_kind = class;
    }
    if let Some(level) = level {
        payload.level = level;
    }
    if let Some(modality) = modality {
        payload.modality = modality;
    }
    if &payload == current {
        return Err(ApiError::unprocessable(
            "E_NO_CHANGE",
            "the request changes nothing",
        ));
    }
    let proposal = confirm_proposal(
        &loaded,
        SemanticPatch::ReplacePayload {
            target: precondition(node)?,
            payload: NodePayload::Requirement(payload),
        },
    )?;
    let write = state.write(&loaded, &proposal, false, None, created_at, "edit_sentence")?;
    Ok(Json(write).into_response())
}
