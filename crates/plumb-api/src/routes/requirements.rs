//! Requirement routes (plan A0.2): requirement reads projected from the PSG and the current F1
//! gate report, classification and human-reviewed EARS replacement. A0.3 (Hotfix 065) adds the
//! governed split, duplicate supersession and requirement-scoped waiver through their domain
//! helpers. U2.1 (Hotfix 073) adds the split preview over the shared semantic change preview.

use std::collections::BTreeSet;

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plumb_core::{GateId, Id, StageId, Timestamp};
use plumb_functional::{
    propose_duplicate_supersession, propose_requirement_split, DuplicateMarkError,
    RequirementSplitError, RequirementSplitInput,
};
use plumb_patch::{
    AcceptancePolicy, ElementPrecondition, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    node_element_hash, ElementStatus, Graph, Node, NodePayload, NodeType, RelationKind,
    Requirement, RequirementKind,
};
use plumb_store::LoadedRevision;
use plumb_validation::GateReport;

use crate::error::ApiError;
use crate::openapi::{
    AcceptEarsRequest, ClassifyRequest, EvidenceExcerpt, MarkDuplicateRequest, ProposalKind,
    RequirementCoverage, RequirementDetail, RequirementLinks, RequirementPage, RequirementSummary,
    SplitRequest, WaiveRequirementFindingRequest,
};
use crate::state::{
    check_if_match, is_clean_text, paginate, parse_body, parse_id, parse_wire, read_json,
    require_clean_reason, wire, ApiState, PageRequest, QueryParams,
};

use super::model::{preview_instant, require_previewed, semantic_change_preview};

/// The F1 rule whose evidence records requirement lint diagnostics.
const QUALITY_RULE: &str = "PLUMB.F1.REQ.QUALITY_FINDINGS_CLEAR";

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/requirements", get(list_requirements))
        .route("/api/requirements/{id}", get(get_requirement))
        .route(
            "/api/requirements/{id}/classify",
            post(classify_requirement),
        )
        .route(
            "/api/requirements/{id}/accept-ears",
            post(accept_requirement_ears),
        )
        .route("/api/requirements/{id}/split", post(split_requirement))
        .route(
            "/api/requirements/{id}/split/preview",
            post(preview_requirement_split),
        )
        .route(
            "/api/requirements/{id}/mark-duplicate",
            post(mark_requirement_duplicate),
        )
        .route(
            "/api/requirements/{id}/waive-finding",
            post(waive_requirement_finding),
        )
}

fn active(status: ElementStatus) -> bool {
    matches!(status, ElementStatus::Proposed | ElementStatus::Accepted)
}

/// Targets of the requirement's Proposed or Accepted `specified_by` edges of `node_type`.
fn specified_by(graph: &Graph, requirement: &Id, node_type: NodeType) -> Vec<String> {
    let mut targets: Vec<String> = graph
        .outgoing_edge_ids(requirement)
        .iter()
        .filter_map(|id| graph.edge(id))
        .filter(|e| e.kind == RelationKind::SpecifiedBy && active(e.status))
        .filter(|e| {
            graph
                .node(&e.to)
                .is_some_and(|n| n.payload.node_type() == node_type)
        })
        .map(|e| e.to.to_string())
        .collect();
    targets.sort();
    targets.dedup();
    targets
}

/// Scenario nodes whose requirement_refs contain the requirement.
fn scenarios(graph: &Graph, requirement: &Id) -> Vec<String> {
    let mut ids: Vec<String> = graph
        .node_ids_by_type(NodeType::Scenario)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| {
            matches!(&n.payload, NodePayload::Scenario(s)
            if s.requirement_refs.iter().flatten().any(|r| r == requirement))
        })
        .map(|n| n.id.to_string())
        .collect();
    ids.sort();
    ids
}

/// Lint rule IDs of the F1 quality evidence `lint:{rule}:{requirement}:{start}:{end}` naming the
/// requirement.
fn smells(report: &GateReport, requirement: &Id) -> Vec<String> {
    let mut rules = BTreeSet::new();
    for rule in report.rules.iter().filter(|r| r.rule_id == QUALITY_RULE) {
        for entry in &rule.evidence {
            let Some(rest) = entry.strip_prefix("lint:") else {
                continue;
            };
            let Some((lint_rule, rest)) = rest.split_once(':') else {
                continue;
            };
            let mut tail = rest.rsplitn(3, ':');
            let (Some(_end), Some(_start), Some(target)) = (tail.next(), tail.next(), tail.next())
            else {
                continue;
            };
            if target == requirement.as_str() {
                rules.insert(lint_rule.to_owned());
            }
        }
    }
    rules.into_iter().collect()
}

/// IDs of the F1 report's findings whose affected refs contain the requirement.
fn lint_findings(report: &GateReport, requirement: &Id) -> Vec<String> {
    let mut ids: Vec<String> = report
        .findings
        .iter()
        .filter(|f| f.payload.affected_refs.contains(requirement))
        .map(|f| f.id.to_string())
        .collect();
    ids.sort();
    ids
}

/// The current F1 gate report of the branch head, if any. Requirement reads never require one
/// (R0.4): without it, lint results are reported as unavailable through `lint_current`.
fn current_f1(state: &ApiState, loaded: &LoadedRevision) -> Result<Option<GateReport>, ApiError> {
    Ok(state
        .current_gate_reports(loaded)?
        .into_iter()
        .find(|r| r.gate == GateId::F1))
}

fn summary(
    graph: &Graph,
    f1: Option<&GateReport>,
    node: &Node,
    requirement: &Requirement,
) -> Result<RequirementSummary, ApiError> {
    Ok(RequirementSummary {
        id: node.id.to_string(),
        text: requirement.statement.clone(),
        class: wire(&requirement.requirement_kind)?,
        status: wire(&node.status)?,
        smells: f1.map(|r| smells(r, &node.id)).unwrap_or_default(),
        coverage: RequirementCoverage {
            operations: specified_by(graph, &node.id, NodeType::Operation),
            scenarios: scenarios(graph, &node.id),
        },
        readiness: None,
        lint_current: f1.is_some(),
    })
}

/// `list_requirements`.
async fn list_requirements(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &["status", "class", "q"], true)?;
    let status = query
        .get("status")
        .map(|s| parse_wire::<ElementStatus>(s, "status"))
        .transpose()?;
    let class = query
        .get("class")
        .map(|c| parse_wire::<RequirementKind>(c, "class"))
        .transpose()?;
    let needle = match query.get("q") {
        Some(q) if q.trim().is_empty() => return Err(ApiError::invalid("q must not be blank")),
        Some(q) => Some(q.to_lowercase()),
        None => None,
    };
    let loaded = state.load_head()?;
    let f1 = current_f1(&state, &loaded)?;
    let mut items = Vec::new();
    for id in loaded.graph.node_ids_by_type(NodeType::Requirement) {
        let Some(node) = loaded.graph.node(id) else {
            continue;
        };
        let NodePayload::Requirement(requirement) = &node.payload else {
            continue;
        };
        if status.is_some_and(|s| s != node.status)
            || class.is_some_and(|c| c != requirement.requirement_kind)
            || needle
                .as_ref()
                .is_some_and(|n| !requirement.statement.to_lowercase().contains(n.as_str()))
        {
            continue;
        }
        items.push((
            node.id.to_string(),
            summary(&loaded.graph, f1.as_ref(), node, requirement)?,
        ));
    }
    let request = PageRequest {
        operation_id: "list_requirements",
        path: Default::default(),
        query: &query,
    };
    let (items, next_cursor) = paginate(items, &request, &loaded)?;
    Ok(read_json(&loaded, RequirementPage { items, next_cursor }))
}

/// The Requirement node `id`, or 404.
fn requirement<'g>(graph: &'g Graph, id: &str) -> Result<(&'g Node, &'g Requirement), ApiError> {
    let missing = || ApiError::not_found(format!("requirement {id} not found"));
    let id: Id = id.parse().map_err(|_| missing())?;
    let node = graph.node(&id).ok_or_else(missing)?;
    match &node.payload {
        NodePayload::Requirement(r) => Ok((node, r)),
        _ => Err(missing()),
    }
}

/// `get_requirement`.
async fn get_requirement(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let (node, payload) = requirement(&loaded.graph, &id)?;
    let f1 = current_f1(&state, &loaded)?;
    let summary = summary(&loaded.graph, f1.as_ref(), node, payload)?;
    // R0.6: the persisted pending S1.2 EARS proposal, never a regenerated suggestion.
    let ears = state.pending_ears_proposal(&loaded, &node.id)?;
    let mut spans: Vec<String> = node
        .evidence
        .iter()
        .map(|e| e.as_id().to_string())
        .collect();
    spans.sort();
    // Fable #351: the exact extracted source text of each evidence reference.
    let evidence = spans
        .iter()
        .map(|span| {
            let fragment = span
                .parse::<Id>()
                .ok()
                .and_then(|id| match loaded.graph.node(&id) {
                    Some(Node {
                        payload: NodePayload::EvidenceFragment(fragment),
                        ..
                    }) => Some(fragment),
                    _ => None,
                });
            EvidenceExcerpt {
                id: span.clone(),
                source_ref: fragment.map(|f| f.source_ref.to_string()),
                text: fragment.and_then(|f| f.extracted_text.clone()),
            }
        })
        .collect();
    let body = RequirementDetail {
        id: summary.id,
        text: summary.text,
        class: summary.class,
        status: summary.status,
        smells: summary.smells,
        coverage: summary.coverage.clone(),
        readiness: None,
        spans,
        evidence,
        lint_findings: f1
            .as_ref()
            .map(|r| lint_findings(r, &node.id))
            .unwrap_or_default(),
        ears_suggestion: ears.as_ref().map(|(text, _)| text.clone()),
        ears_proposal_ref: ears.as_ref().map(|(_, id)| id.to_string()),
        linked: RequirementLinks {
            operations: summary.coverage.operations,
            processes: specified_by(&loaded.graph, &node.id, NodeType::Process),
            scenarios: summary.coverage.scenarios,
        },
        lint_current: summary.lint_current,
    };
    Ok(read_json(&loaded, body))
}

/// Proposes replacing the Requirement's payload under its element-hash precondition.
fn replace_requirement(
    loaded: &LoadedRevision,
    node: &Node,
    payload: Requirement,
) -> Result<Proposal, ApiError> {
    let expected_hash =
        node_element_hash(node).map_err(|e| ApiError::internal_cause("element_hash", &e))?;
    Proposal::new(
        StageId::S1,
        PatchSet {
            base_semantic_hash: loaded.revision.semantic_hash.clone(),
            patch: SemanticPatch::ReplacePayload {
                target: ElementPrecondition {
                    id: node.id.clone(),
                    expected_hash,
                },
                payload: NodePayload::Requirement(payload),
            },
        },
        Vec::new(),
        Vec::new(),
        ProposalMateriality::Semantic,
        AcceptancePolicy::HumanConfirm,
        None,
    )
    .map_err(|e| ApiError::internal_cause("proposal", &e))
}

/// `classify_requirement`: changes only `requirement_kind`.
async fn classify_requirement(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: ClassifyRequest = parse_body(&body)?;
    let kind: RequirementKind = parse_wire(&request.class, "class")?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let (node, current) = requirement(&loaded.graph, &id)?;
    if !matches!(
        node.status,
        ElementStatus::Proposed | ElementStatus::Accepted
    ) {
        return Err(ApiError::semantic(
            "RequirementNotEditable",
            "the requirement is not Proposed or Accepted",
        ));
    }
    if current.requirement_kind == kind {
        return Err(ApiError::unprocessable(
            "E_NO_CHANGE",
            "the requirement already has this class",
        ));
    }
    let mut payload = current.clone();
    payload.requirement_kind = kind;
    let proposal = replace_requirement(&loaded, node, payload)?;
    let write = state.commit(&loaded, &proposal, created_at, "classify_requirement")?;
    Ok(Json(write).into_response())
}

/// `accept_requirement_ears`: the reviewed text replaces only `statement`.
async fn accept_requirement_ears(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: AcceptEarsRequest = parse_body(&body)?;
    if !is_clean_text(&request.text) {
        return Err(ApiError::invalid(
            "text must be non-empty, without surrounding whitespace or control characters",
        ));
    }
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let (node, current) = requirement(&loaded.graph, &id)?;
    if !matches!(
        node.status,
        ElementStatus::Proposed | ElementStatus::Accepted
    ) {
        return Err(ApiError::semantic(
            "RequirementNotEditable",
            "the requirement is not Proposed or Accepted",
        ));
    }
    if current.statement == request.text {
        return Err(ApiError::unprocessable(
            "E_NO_CHANGE",
            "the statement already has this text",
        ));
    }
    let mut payload = current.clone();
    payload.statement = request.text;
    let proposal = replace_requirement(&loaded, node, payload)?;
    let write = state.commit(&loaded, &proposal, created_at, "accept_requirement_ears")?;
    Ok(Json(write).into_response())
}

// ============================================================================ A0.3 (Hotfix 065)

/// A validated requirement split: the loaded head, the exact `propose_requirement_split`
/// Proposal, the clean reason, the one Clock read and the reviewed preview it submits, if any.
struct RequirementSplit {
    loaded: LoadedRevision,
    proposal: Proposal,
    reason: String,
    created_at: Timestamp,
    preview: Option<crate::openapi::PreviewConfirmation>,
}

/// The shared validation and Proposal construction of `split_requirement` and
/// `preview_requirement_split`, in the frozen order.
fn requirement_split(
    state: &ApiState,
    id: &str,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<RequirementSplit, ApiError> {
    let request: SplitRequest = parse_body(body)?;
    if request.parts.len() < 2 {
        return Err(ApiError::invalid("parts needs at least two entries"));
    }
    for (index, part) in request.parts.iter().enumerate() {
        if !is_clean_text(part) {
            return Err(ApiError::invalid(format!(
                "part {index} must be non-empty, without surrounding whitespace or control characters"
            )));
        }
        if request.parts[..index].contains(part) {
            return Err(ApiError::invalid(format!(
                "part {index} repeats an earlier part"
            )));
        }
    }
    require_clean_reason(&request.reason)?;
    let loaded = state.load_head()?;
    check_if_match(headers, &loaded)?;
    let created_at = state.now();
    // H0.11: a submitted preview is rebuilt at its own instant, so an unchanged change on an
    // unchanged head is exactly the previewed Proposal.
    let at = match &request.preview {
        Some(preview) => preview_instant(&loaded, preview, created_at)?,
        None => created_at,
    };
    let (node, _) = requirement(&loaded.graph, id)?;
    let proposal = propose_requirement_split(
        &loaded.graph,
        &RequirementSplitInput {
            parent_ref: node.id.clone(),
            parts: request.parts,
            actor_ref: state.actor_ref().clone(),
            at,
        },
    )
    .map_err(|error| match error {
        RequirementSplitError::IdCollision(id) => {
            ApiError::conflict("E_ID_COLLISION", "a split identity is already used")
                .with_details(serde_json::json!({ "id": id }))
        }
        other => ApiError::semantic("RequirementSplitError", other.to_string()),
    })?;
    Ok(RequirementSplit {
        loaded,
        proposal,
        reason: request.reason,
        created_at,
        preview: request.preview,
    })
}

/// `split_requirement`: an explicit human-governed split into Accepted children, through
/// `plumb_functional::requirements`; the parent is superseded.
async fn split_requirement(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let split = requirement_split(&state, &id, &headers, &body)?;
    if let Some(preview) = &split.preview {
        require_previewed(&split.proposal, preview)?;
    }
    let write = state.write(
        &split.loaded,
        &split.proposal,
        true,
        Some(&split.reason),
        split.created_at,
        "split_requirement",
    )?;
    Ok(Json(write).into_response())
}

/// `preview_requirement_split` (U2.1, Hotfix 073; H0.11): the exact `split_requirement`
/// Proposal with its semantic element diff, never persisted; `split_requirement` with `preview`
/// submits it (a create-class Proposal, Hotfix 074: the split adds child Requirements and
/// `decomposes_to` edges and supersedes the parent only through `SetStatus`).
async fn preview_requirement_split(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let split = requirement_split(&state, &id, &headers, &body)?;
    semantic_change_preview(
        &state,
        &split.loaded,
        &split.proposal,
        ProposalKind::Create,
        split.created_at,
    )
}

/// `mark_requirement_duplicate`: canonical supersession of the duplicate (path) by `of`,
/// through `plumb_functional::duplicates`; never a node merge.
async fn mark_requirement_duplicate(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: MarkDuplicateRequest = parse_body(&body)?;
    let of = parse_id(&request.of).ok_or_else(|| ApiError::invalid("of is not an ID"))?;
    if request.of == id {
        return Err(ApiError::invalid("a requirement cannot duplicate itself"));
    }
    require_clean_reason(&request.reason)?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let (duplicate, _) = requirement(&loaded.graph, &id)?;
    let (replacement, _) = requirement(&loaded.graph, of.as_str())?;
    let proposal = propose_duplicate_supersession(
        &loaded.graph,
        &duplicate.id,
        &replacement.id,
        state.actor_ref(),
        created_at,
    )
    .map_err(|error| match error {
        DuplicateMarkError::IdCollision(id) => ApiError::conflict(
            "E_ID_COLLISION",
            "the supersedes edge identity is already used",
        )
        .with_details(serde_json::json!({ "id": id })),
        other => ApiError::semantic("DuplicateMarkError", other.to_string()),
    })?;
    let write = state.write(
        &loaded,
        &proposal,
        true,
        Some(&request.reason),
        created_at,
        "mark_requirement_duplicate",
    )?;
    Ok(Json(write).into_response())
}

/// `waive_requirement_finding`: the Hotfix 048 waiver of a current finding that affects the
/// Accepted path Requirement.
async fn waive_requirement_finding(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: WaiveRequirementFindingRequest = parse_body(&body)?;
    let finding_id = parse_id(&request.finding_id)
        .ok_or_else(|| ApiError::invalid("finding_id is not an ID"))?;
    require_clean_reason(&request.reason)?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let (node, _) = requirement(&loaded.graph, &id)?;
    if node.status != ElementStatus::Accepted {
        return Err(ApiError::semantic(
            "RequirementNotAccepted",
            "the requirement is not Accepted",
        ));
    }
    let write = super::model::waive(
        &state,
        &loaded,
        &finding_id,
        Some(&node.id),
        &request.reason,
        created_at,
        "waive_requirement_finding",
    )?;
    Ok(Json(write).into_response())
}
