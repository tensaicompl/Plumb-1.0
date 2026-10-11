//! Requirement routes (plan A0.2): requirement reads projected from the PSG and the current F1
//! gate report, classification and human-reviewed EARS replacement.

use std::collections::BTreeSet;

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plumb_core::{GateId, Id, StageId};
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
    AcceptEarsRequest, ClassifyRequest, RequirementCoverage, RequirementDetail, RequirementLinks,
    RequirementPage, RequirementSummary,
};
use crate::state::{
    check_if_match, is_clean_text, paginate, parse_body, parse_wire, read_json, wire, ApiState,
    PageRequest, QueryParams,
};

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

fn summary(
    graph: &Graph,
    f1: &GateReport,
    node: &Node,
    requirement: &Requirement,
) -> Result<RequirementSummary, ApiError> {
    Ok(RequirementSummary {
        id: node.id.to_string(),
        text: requirement.statement.clone(),
        class: wire(&requirement.requirement_kind)?,
        status: wire(&node.status)?,
        smells: smells(f1, &node.id),
        coverage: RequirementCoverage {
            operations: specified_by(graph, &node.id, NodeType::Operation),
            scenarios: scenarios(graph, &node.id),
        },
        readiness: None,
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
    let f1 = state.current_gate_report(&loaded, GateId::F1)?;
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
            summary(&loaded.graph, f1, node, requirement)?,
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
    let f1 = state.current_gate_report(&loaded, GateId::F1)?;
    let summary = summary(&loaded.graph, f1, node, payload)?;
    let mut spans: Vec<String> = node
        .evidence
        .iter()
        .map(|e| e.as_id().to_string())
        .collect();
    spans.sort();
    let body = RequirementDetail {
        id: summary.id,
        text: summary.text,
        class: summary.class,
        status: summary.status,
        smells: summary.smells,
        coverage: summary.coverage.clone(),
        readiness: None,
        spans,
        lint_findings: lint_findings(f1, &node.id),
        ears_suggestion: None,
        linked: RequirementLinks {
            operations: summary.coverage.operations,
            processes: specified_by(&loaded.graph, &node.id, NodeType::Process),
            scenarios: summary.coverage.scenarios,
        },
    };
    Ok(read_json(&loaded, body))
}

/// Proposes replacing the Requirement's payload under its element-hash precondition.
fn replace_requirement(
    loaded: &LoadedRevision,
    node: &Node,
    payload: Requirement,
) -> Result<Proposal, ApiError> {
    let expected_hash = node_element_hash(node).map_err(|_| ApiError::internal("element_hash"))?;
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
    .map_err(|_| ApiError::internal("proposal"))
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
