//! Model routes (plan A0.2): glossary, entities and attributes, calculations, processes,
//! findings, assumptions, decisions, exports and revision diffs.
//!
//! Reads project typed PSG material, typed relations and existing analyses of one loaded
//! revision; writes are typed status or payload replacements committed through the shared
//! intake and compare-and-swap pipeline.

use std::collections::BTreeSet;

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plumb_core::{GateId, Id, StageId, Timestamp};
use plumb_functional::{project_functional_pilot, PilotDataClass};
use plumb_patch::{
    AcceptancePolicy, ElementPrecondition, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    edge_element_hash, node_element_hash, ConceptKind, ElementStatus, Graph, Node, NodePayload,
    NodeType, ProcessNodeKind, RelationKind,
};
use plumb_store::{LoadedRevision, RevisionId};
use plumb_validation::{analyze_process, ParallelAnalysis, ProcessAnalysisMode};
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::openapi::{
    Assumption, AssumptionPage, AttributeClassRequest, AttributeTypeRequest, AttributeUnitRequest,
    Calculation, CalculationPage, Decision, DecisionPage, Entity, EntityAttribute, EntityPage,
    EntityRelationship, EntityTransition, ExportFormat, Finding, FindingPage, GlossaryEditRequest,
    GlossaryPage, GlossaryTerm, ModelDiff, Process, ProcessPage, ScalarValue,
};
use crate::state::{
    check_if_match, is_clean_text, paginate, parse_body, parse_wire, read_json, wire, with_etag,
    ApiState, PageRequest, QueryParams,
};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/glossary", get(list_glossary))
        .route("/api/glossary/{id}/confirm", post(confirm_glossary))
        .route("/api/glossary/{id}/edit", post(edit_glossary))
        .route("/api/model/entities", get(list_entities))
        .route("/api/model/entities/{id}", get(get_entity))
        .route("/api/model/attributes/{id}/type", post(set_attribute_type))
        .route("/api/model/attributes/{id}/unit", post(set_attribute_unit))
        .route(
            "/api/model/attributes/{id}/class",
            post(set_attribute_class),
        )
        .route("/api/model/calculations", get(list_calculations))
        .route(
            "/api/model/calculations/{id}/confirm",
            post(confirm_calculation),
        )
        .route("/api/processes", get(list_processes))
        .route("/api/processes/{id}", get(get_process))
        .route("/api/findings", get(list_findings))
        .route("/api/assumptions", get(list_assumptions))
        .route("/api/decisions", get(list_decisions))
        .route("/api/model/export", get(export_model))
        .route("/api/model/diff", get(get_model_diff))
}

fn active(status: ElementStatus) -> bool {
    matches!(status, ElementStatus::Proposed | ElementStatus::Accepted)
}

/// The node `id` of `node_type`, or 404.
fn typed_node<'g>(
    graph: &'g Graph,
    id: &str,
    node_type: NodeType,
    what: &str,
) -> Result<&'g Node, ApiError> {
    let missing = || ApiError::not_found(format!("{what} {id} not found"));
    let id: Id = id.parse().map_err(|_| missing())?;
    graph
        .node(&id)
        .filter(|n| n.payload.node_type() == node_type)
        .ok_or_else(missing)
}

/// Targets of the node's Proposed or Accepted outgoing `kind` edges, sorted.
fn targets(graph: &Graph, node: &Id, kind: RelationKind) -> Vec<Id> {
    let mut ids: Vec<Id> = graph
        .outgoing_edge_ids(node)
        .iter()
        .filter_map(|id| graph.edge(id))
        .filter(|e| e.kind == kind && active(e.status))
        .map(|e| e.to.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn simple_page<T>(
    items: Vec<(String, T)>,
    operation_id: &'static str,
    query: &QueryParams,
    loaded: &LoadedRevision,
) -> Result<(Vec<T>, Option<String>), ApiError> {
    let request = PageRequest {
        operation_id,
        path: Default::default(),
        query,
    };
    paginate(items, &request, loaded)
}

// ============================================================================ writes

fn proposal(
    loaded: &LoadedRevision,
    stage: StageId,
    patch: SemanticPatch,
) -> Result<Proposal, ApiError> {
    Proposal::new(
        stage,
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
    .map_err(|_| ApiError::internal("proposal"))
}

fn precondition(node: &Node) -> Result<ElementPrecondition, ApiError> {
    Ok(ElementPrecondition {
        id: node.id.clone(),
        expected_hash: node_element_hash(node).map_err(|_| ApiError::internal("element_hash"))?,
    })
}

/// Proposes SetStatus Proposed -> Accepted for a Proposed node.
fn confirm(loaded: &LoadedRevision, node: &Node, stage: StageId) -> Result<Proposal, ApiError> {
    if node.status != ElementStatus::Proposed {
        return Err(ApiError::unprocessable(
            "E_SEMANTIC_INVARIANT",
            "only a Proposed element can be confirmed",
        )
        .with_details(json!({ "status": wire(&node.status)? })));
    }
    proposal(
        loaded,
        stage,
        SemanticPatch::SetStatus {
            target: precondition(node)?,
            from: ElementStatus::Proposed,
            to: ElementStatus::Accepted,
        },
    )
}

/// Proposes replacing the node's payload.
fn replace(
    loaded: &LoadedRevision,
    node: &Node,
    stage: StageId,
    payload: NodePayload,
) -> Result<Proposal, ApiError> {
    if payload == node.payload {
        return Err(ApiError::unprocessable(
            "E_NO_CHANGE",
            "the request changes nothing",
        ));
    }
    proposal(
        loaded,
        stage,
        SemanticPatch::ReplacePayload {
            target: precondition(node)?,
            payload,
        },
    )
}

/// `confirm_glossary`.
async fn confirm_glossary(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let node = typed_node(&loaded.graph, &id, NodeType::Term, "term")?;
    let proposal = confirm(&loaded, node, StageId::S1)?;
    Ok(Json(state.commit(&loaded, &proposal, created_at, "confirm_glossary")?).into_response())
}

/// `edit_glossary`: only aliases change; `term` is the S1.4 identity key and must be unchanged.
async fn edit_glossary(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: GlossaryEditRequest = parse_body(&body)?;
    let mut aliases: Vec<String> = request.aliases.clone().unwrap_or_default();
    if aliases.iter().any(|a| !is_clean_text(a)) {
        return Err(ApiError::invalid("aliases must be clean non-empty text"));
    }
    let unique: BTreeSet<&String> = aliases.iter().collect();
    if unique.len() != aliases.len() {
        return Err(ApiError::invalid("aliases must be unique"));
    }
    aliases.sort();
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let node = typed_node(&loaded.graph, &id, NodeType::Term, "term")?;
    let NodePayload::Term(term) = &node.payload else {
        return Err(ApiError::internal("term"));
    };
    if request.term != term.term {
        return Err(ApiError::unprocessable(
            "E_TERM_IDENTITY_CHANGE",
            "term is the vocabulary identity key and cannot be edited",
        ));
    }
    if aliases.contains(&term.term) {
        return Err(ApiError::invalid("an alias must differ from the term"));
    }
    let mut payload = term.clone();
    payload.aliases = (!aliases.is_empty()).then_some(aliases);
    let proposal = replace(&loaded, node, StageId::S1, NodePayload::Term(payload))?;
    Ok(Json(state.commit(&loaded, &proposal, created_at, "edit_glossary")?).into_response())
}

enum AttributeChange {
    Type(String),
    Unit(String),
    Class(PilotDataClass),
}

/// Shared attribute write: one typed field of the Attribute payload changes.
async fn attribute_write(
    state: ApiState,
    id: String,
    headers: HeaderMap,
    change: AttributeChange,
    operation_id: &'static str,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let node = typed_node(&loaded.graph, &id, NodeType::Attribute, "attribute")?;
    let NodePayload::Attribute(attribute) = &node.payload else {
        return Err(ApiError::internal("attribute"));
    };
    let mut payload = attribute.clone();
    match change {
        AttributeChange::Type(value_type) => {
            let known = loaded
                .graph
                .node_ids_by_type(NodeType::Concept)
                .iter()
                .any(|id| {
                    loaded.graph.node(id).is_some_and(|n| {
                        n.status == ElementStatus::Accepted
                            && matches!(&n.payload, NodePayload::Concept(c)
                            if c.concept_kind == ConceptKind::ValueType && c.name == value_type)
                    })
                });
            if !known {
                return Err(ApiError::unprocessable(
                    "E_SEMANTIC_INVARIANT",
                    "type must name an Accepted value_type Concept",
                ));
            }
            payload.value_type = value_type;
        }
        AttributeChange::Unit(unit) => payload.unit = Some(unit),
        AttributeChange::Class(class) => {
            if let Some(existing) = &attribute.data_classification {
                if existing == class.as_str() {
                    return Err(ApiError::unprocessable(
                        "E_NO_CHANGE",
                        "the request changes nothing",
                    ));
                }
                return Err(ApiError::conflict(
                    "E_ATTRIBUTE_ALREADY_CLASSIFIED",
                    "an existing data classification is never overwritten",
                )
                .with_details(json!({ "data_classification": existing })));
            }
            payload.data_classification = Some(class.as_str().to_owned());
        }
    }
    let proposal = replace(&loaded, node, StageId::S2, NodePayload::Attribute(payload))?;
    Ok(Json(state.commit(&loaded, &proposal, created_at, operation_id)?).into_response())
}

/// `set_attribute_type`.
async fn set_attribute_type(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: AttributeTypeRequest = parse_body(&body)?;
    if !is_clean_text(&request.value_type) {
        return Err(ApiError::invalid("type must be clean non-empty text"));
    }
    attribute_write(
        state,
        id,
        headers,
        AttributeChange::Type(request.value_type),
        "set_attribute_type",
    )
    .await
}

/// `set_attribute_unit`.
async fn set_attribute_unit(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: AttributeUnitRequest = parse_body(&body)?;
    if !is_clean_text(&request.unit) {
        return Err(ApiError::invalid("unit must be clean non-empty text"));
    }
    attribute_write(
        state,
        id,
        headers,
        AttributeChange::Unit(request.unit),
        "set_attribute_unit",
    )
    .await
}

/// `set_attribute_class`.
async fn set_attribute_class(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: AttributeClassRequest = parse_body(&body)?;
    let class = PilotDataClass::parse(&request.class)
        .ok_or_else(|| ApiError::invalid("class must be pii, financial or confidential"))?;
    attribute_write(
        state,
        id,
        headers,
        AttributeChange::Class(class),
        "set_attribute_class",
    )
    .await
}

/// `confirm_calculation`.
async fn confirm_calculation(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let node = typed_node(&loaded.graph, &id, NodeType::Calculation, "calculation")?;
    let proposal = confirm(&loaded, node, StageId::S2)?;
    Ok(Json(state.commit(&loaded, &proposal, created_at, "confirm_calculation")?).into_response())
}

// ============================================================================ glossary and entities

/// `list_glossary`.
async fn list_glossary(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &[], true)?;
    let loaded = state.load_head()?;
    let mut items = Vec::new();
    for id in loaded.graph.node_ids_by_type(NodeType::Term) {
        let Some(node) = loaded.graph.node(id) else {
            continue;
        };
        let NodePayload::Term(term) = &node.payload else {
            continue;
        };
        items.push((
            node.id.to_string(),
            GlossaryTerm {
                id: node.id.to_string(),
                term: term.term.clone(),
                language: term.language.clone(),
                aliases: term.aliases.clone().unwrap_or_default(),
                definition_ref: term.definition_ref.as_ref().map(Id::to_string),
                status: wire(&node.status)?,
            },
        ));
    }
    let (items, next_cursor) = simple_page(items, "list_glossary", &query, &loaded)?;
    Ok(read_json(&loaded, GlossaryPage { items, next_cursor }))
}

fn entity(graph: &Graph, node: &Node) -> Result<Entity, ApiError> {
    let NodePayload::Entity(payload) = &node.payload else {
        return Err(ApiError::internal("entity"));
    };
    let mut attributes = Vec::new();
    for id in targets(graph, &node.id, RelationKind::HasAttribute) {
        if let Some(NodePayload::Attribute(a)) = graph.node(&id).map(|n| &n.payload) {
            attributes.push(EntityAttribute {
                name: a.name.clone(),
                value_type: a.value_type.clone(),
                nullable: a.nullable,
                unit: a.unit.clone(),
                class: a.data_classification.clone(),
            });
        }
    }
    let mut relationships = Vec::new();
    for id in graph.node_ids_by_type(NodeType::DomainRelationship) {
        let Some(n) = graph.node(id).filter(|n| active(n.status)) else {
            continue;
        };
        if let NodePayload::DomainRelationship(r) = &n.payload {
            if r.from_entity == node.id {
                relationships.push(EntityRelationship {
                    id: n.id.to_string(),
                    target: r.to_entity.to_string(),
                    kind: r.relationship_kind.clone(),
                    cardinality: Some(format!("{}:{}", r.cardinality_from, r.cardinality_to)),
                });
            }
        }
    }
    let mut transitions = Vec::new();
    for id in graph.node_ids_by_type(NodeType::Transition) {
        let Some(n) = graph.node(id).filter(|n| active(n.status)) else {
            continue;
        };
        if let NodePayload::Transition(t) = &n.payload {
            if t.stateful_ref == node.id {
                let operation = targets(graph, &n.id, RelationKind::TransitionsVia)
                    .into_iter()
                    .find(|target| {
                        graph
                            .node(target)
                            .is_some_and(|t| t.payload.node_type() == NodeType::Operation)
                    })
                    .map(|id| id.to_string());
                transitions.push(EntityTransition {
                    from: t.from_state.to_string(),
                    to: t.to_state.to_string(),
                    operation,
                });
            }
        }
    }
    Ok(Entity {
        id: node.id.to_string(),
        name: payload.name.clone(),
        attributes,
        relationships,
        states: targets(graph, &node.id, RelationKind::HasState)
            .iter()
            .map(Id::to_string)
            .collect(),
        transitions,
        tags: node.tags.iter().cloned().collect(),
    })
}

/// `list_entities`.
async fn list_entities(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &[], true)?;
    let loaded = state.load_head()?;
    let mut items = Vec::new();
    for id in loaded.graph.node_ids_by_type(NodeType::Entity) {
        let Some(node) = loaded.graph.node(id) else {
            continue;
        };
        items.push((node.id.to_string(), entity(&loaded.graph, node)?));
    }
    let (items, next_cursor) = simple_page(items, "list_entities", &query, &loaded)?;
    Ok(read_json(&loaded, EntityPage { items, next_cursor }))
}

/// `get_entity`.
async fn get_entity(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let node = typed_node(&loaded.graph, &id, NodeType::Entity, "entity")?;
    let body = entity(&loaded.graph, node)?;
    Ok(read_json(&loaded, body))
}

// ============================================================================ calculations and processes

/// `list_calculations`.
async fn list_calculations(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &[], true)?;
    let loaded = state.load_head()?;
    let mut items = Vec::new();
    for id in loaded.graph.node_ids_by_type(NodeType::Calculation) {
        let Some(node) = loaded.graph.node(id) else {
            continue;
        };
        let NodePayload::Calculation(c) = &node.payload else {
            continue;
        };
        items.push((
            node.id.to_string(),
            Calculation {
                id: node.id.to_string(),
                name: c.name.clone(),
                expr: c.expression.clone(),
            },
        ));
    }
    let (items, next_cursor) = simple_page(items, "list_calculations", &query, &loaded)?;
    Ok(read_json(&loaded, CalculationPage { items, next_cursor }))
}

fn process(graph: &Graph, node: &Node) -> Result<Process, ApiError> {
    let NodePayload::Process(payload) = &node.payload else {
        return Err(ApiError::internal("process"));
    };
    let mode = if node.status == ElementStatus::Accepted {
        ProcessAnalysisMode::AcceptedOnly
    } else {
        ProcessAnalysisMode::ActiveOverlay
    };
    let analysis = analyze_process(graph, &node.id, mode)
        .map_err(|_| ApiError::internal("process_analysis"))?;
    let mut lanes = BTreeSet::new();
    let mut branches = Vec::new();
    for member in &analysis.node_refs {
        lanes.extend(
            targets(graph, member, RelationKind::PerformedBy)
                .into_iter()
                .map(|t| t.to_string()),
        );
        if let Some(NodePayload::ProcessNode(p)) = graph.node(member).map(|n| &n.payload) {
            if matches!(
                p.node_kind,
                ProcessNodeKind::ExclusiveGateway | ProcessNodeKind::ParallelSplit
            ) {
                branches.push(member.to_string());
            }
        }
    }
    branches.sort();
    let mut validation = BTreeSet::new();
    if analysis.missing_start() {
        validation.insert("missing_start".to_owned());
    }
    if analysis.missing_end() {
        validation.insert("missing_end".to_owned());
    }
    validation.extend(
        analysis
            .unreachable_refs
            .iter()
            .map(|r| format!("unreachable:{r}")),
    );
    validation.extend(
        analysis
            .invalid_terminal_refs
            .iter()
            .map(|r| format!("invalid_terminal:{r}")),
    );
    validation.extend(
        analysis
            .unresolved_task_refs
            .iter()
            .map(|t| format!("unresolved_task:{}", t.node_ref)),
    );
    validation.extend(
        analysis
            .unsupported_nodes
            .iter()
            .map(|u| format!("unsupported_node:{}", u.node_ref)),
    );
    validation.extend(
        analysis
            .boundary_violations
            .iter()
            .map(|b| format!("boundary_violation:{}", b.edge_ref)),
    );
    validation.extend(
        analysis
            .node_shape_violations
            .iter()
            .map(|v| format!("node_shape:{}", v.node_ref)),
    );
    if let ParallelAnalysis::Analyzed { issues, .. } = &analysis.parallel_analysis {
        for issue in issues {
            let kind = wire(&issue.kind)?;
            for affected in &issue.affected_refs {
                validation.insert(format!("{kind}:{affected}"));
            }
        }
    }
    let mut steps: Vec<String> = analysis.node_refs.iter().map(Id::to_string).collect();
    steps.sort();
    Ok(Process {
        id: node.id.to_string(),
        name: payload.name.clone(),
        lanes: lanes.into_iter().collect(),
        steps,
        branches,
        validation: validation.into_iter().collect(),
    })
}

/// `list_processes`.
async fn list_processes(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &[], true)?;
    let loaded = state.load_head()?;
    let mut items = Vec::new();
    for id in loaded.graph.node_ids_by_type(NodeType::Process) {
        let Some(node) = loaded.graph.node(id) else {
            continue;
        };
        items.push((node.id.to_string(), process(&loaded.graph, node)?));
    }
    let (items, next_cursor) = simple_page(items, "list_processes", &query, &loaded)?;
    Ok(read_json(&loaded, ProcessPage { items, next_cursor }))
}

/// `get_process`.
async fn get_process(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let node = typed_node(&loaded.graph, &id, NodeType::Process, "process")?;
    let body = process(&loaded.graph, node)?;
    Ok(read_json(&loaded, body))
}

// ============================================================================ findings, assumptions, decisions

/// `list_findings`: persisted Finding nodes with exact filters.
async fn list_findings(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(
        query.as_deref(),
        &["status", "severity", "code", "family"],
        true,
    )?;
    let loaded = state.load_head()?;
    let mut items = Vec::new();
    for id in loaded.graph.node_ids_by_type(NodeType::Finding) {
        let Some(node) = loaded.graph.node(id) else {
            continue;
        };
        let NodePayload::Finding(f) = &node.payload else {
            continue;
        };
        let severity = wire(&f.severity)?;
        let matches = query.get("status").is_none_or(|v| v == f.status)
            && query.get("severity").is_none_or(|v| v == severity)
            && query.get("code").is_none_or(|v| v == f.code)
            && query.get("family").is_none_or(|v| v == f.family);
        if !matches {
            continue;
        }
        items.push((
            node.id.to_string(),
            Finding {
                id: node.id.to_string(),
                code: f.code.clone(),
                family: f.family.clone(),
                severity,
                status: f.status.clone(),
                message: f.message.clone(),
                affected_refs: f.affected_refs.iter().map(Id::to_string).collect(),
                waiver_ref: f.waiver_ref.as_ref().map(Id::to_string),
            },
        ));
    }
    let (items, next_cursor) = simple_page(items, "list_findings", &query, &loaded)?;
    Ok(read_json(&loaded, FindingPage { items, next_cursor }))
}

/// `list_assumptions`: a stored default that is not a JSON scalar is reported, never dropped.
async fn list_assumptions(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &[], true)?;
    let loaded = state.load_head()?;
    let mut items = Vec::new();
    for id in loaded.graph.node_ids_by_type(NodeType::Assumption) {
        let Some(node) = loaded.graph.node(id) else {
            continue;
        };
        let NodePayload::Assumption(a) = &node.payload else {
            continue;
        };
        let default_value = match &a.default_value {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(ScalarValue::Text(s.clone())),
            Some(Value::Bool(b)) => Some(ScalarValue::Boolean(*b)),
            Some(Value::Number(n)) => match n.as_f64() {
                Some(f) => Some(ScalarValue::Number(f)),
                None => return Err(unrepresentable(&node.id)),
            },
            Some(_) => return Err(unrepresentable(&node.id)),
        };
        items.push((
            node.id.to_string(),
            Assumption {
                id: node.id.to_string(),
                statement: a.statement.clone(),
                owner_ref: a.owner_ref.to_string(),
                status: a.status.clone(),
                finding_ref: a.finding_ref.as_ref().map(Id::to_string),
                default_value,
                expires_at: a.expires_at.map(|t| t.to_string()),
                risk_ref: a.risk_ref.as_ref().map(Id::to_string),
            },
        ));
    }
    let (items, next_cursor) = simple_page(items, "list_assumptions", &query, &loaded)?;
    Ok(read_json(&loaded, AssumptionPage { items, next_cursor }))
}

fn unrepresentable(id: &Id) -> ApiError {
    ApiError::unprocessable(
        "E_UNREPRESENTABLE_ASSUMPTION_DEFAULT",
        "an assumption default is not a JSON string, number or boolean",
    )
    .with_details(json!({ "assumption": id }))
}

/// `list_decisions`: ResolutionDecision nodes; `kind` is the answer marker's kind.
async fn list_decisions(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &["by", "kind", "since"], true)?;
    let since = query
        .get("since")
        .map(|s| {
            s.parse::<Timestamp>()
                .map_err(|_| ApiError::invalid("since must be an RFC 3339 timestamp"))
        })
        .transpose()?;
    let loaded = state.load_head()?;
    let mut items = Vec::new();
    for id in loaded.graph.node_ids_by_type(NodeType::ResolutionDecision) {
        let Some(node) = loaded.graph.node(id) else {
            continue;
        };
        let NodePayload::ResolutionDecision(d) = &node.payload else {
            continue;
        };
        let kind = d.answer.get("kind").and_then(Value::as_str);
        let matches = query.get("by").is_none_or(|v| v == d.decided_by.as_str())
            && query.get("kind").is_none_or(|v| Some(v) == kind)
            && since.is_none_or(|s| d.decided_at >= s);
        if !matches {
            continue;
        }
        items.push((
            node.id.to_string(),
            Decision {
                id: node.id.to_string(),
                proposal_ref: d.proposal_ref.as_ref().map(Id::to_string),
                question_ref: d.question_ref.as_ref().map(Id::to_string),
                decided_by: d.decided_by.to_string(),
                decided_at: d.decided_at.to_string(),
                rationale: d.rationale.clone(),
            },
        ));
    }
    let (items, next_cursor) = simple_page(items, "list_decisions", &query, &loaded)?;
    Ok(read_json(&loaded, DecisionPage { items, next_cursor }))
}

// ============================================================================ export and diff

/// `export_model`: the selected artifact of the existing pilot projection, which needs the
/// current F4 report.
async fn export_model(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &["format"], false)?;
    let format: ExportFormat = parse_wire(
        query
            .get("format")
            .ok_or_else(|| ApiError::invalid("format is required"))?,
        "format",
    )?;
    let loaded = state.load_head()?;
    let f4 = state.current_gate_report(&loaded, GateId::F4)?;
    let outputs = project_functional_pilot(&loaded.graph, state.profile(), f4).map_err(|e| {
        ApiError::unprocessable("E_SEMANTIC_INVARIANT", "the model cannot be projected")
            .with_details(json!({ "projection_error": e.to_string() }))
    })?;
    let artifact = match format {
        ExportFormat::Functional => outputs.functional_yaml,
        ExportFormat::Report => outputs.model_report_md,
        ExportFormat::Decisions => outputs.decisions_jsonl,
        ExportFormat::Glossary => outputs.glossary_md,
    };
    let mut response = artifact.bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    Ok(with_etag(&loaded, response))
}

/// `get_model_diff`: stable node and edge IDs added, removed or changed between two revisions.
async fn get_model_diff(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &["from", "to"], false)?;
    let parse = |key: &str| {
        query
            .get(key)
            .map(|v| {
                v.parse::<RevisionId>()
                    .map_err(|_| ApiError::invalid(format!("{key} must be a revision ID")))
            })
            .transpose()
    };
    let (from_id, to_id) = (parse("from")?, parse("to")?);
    let loaded = state.load_head()?;
    let to = match to_id {
        Some(id) if id == loaded.revision.id => loaded.clone(),
        Some(id) => state.load_revision(&id)?,
        None => loaded.clone(),
    };
    let from_id = match from_id {
        Some(id) => id,
        None => to.revision.parent.clone().ok_or_else(|| {
            ApiError::unprocessable("E_DIFF_NO_PARENT", "the target revision has no parent")
        })?,
    };
    let from = state.load_revision(&from_id)?;
    let (mut added, mut removed, mut modified) = (Vec::new(), Vec::new(), Vec::new());
    for (id, node) in to.graph.nodes() {
        match from.graph.node(id) {
            None => added.push(id.to_string()),
            Some(old) if element_differs(node_element_hash(old), node_element_hash(node))? => {
                modified.push(id.to_string())
            }
            Some(_) => {}
        }
    }
    for (id, edge) in to.graph.edges() {
        match from.graph.edge(id) {
            None => added.push(id.to_string()),
            Some(old) if element_differs(edge_element_hash(old), edge_element_hash(edge))? => {
                modified.push(id.to_string())
            }
            Some(_) => {}
        }
    }
    removed.extend(
        from.graph
            .nodes()
            .keys()
            .filter(|id| to.graph.node(id).is_none())
            .map(Id::to_string),
    );
    removed.extend(
        from.graph
            .edges()
            .keys()
            .filter(|id| to.graph.edge(id).is_none())
            .map(Id::to_string),
    );
    for list in [&mut added, &mut removed, &mut modified] {
        list.sort();
    }
    let body = ModelDiff {
        from: from.revision.id.to_string(),
        to: to.revision.id.to_string(),
        added,
        removed,
        modified,
    };
    Ok(read_json(&loaded, body))
}

fn element_differs<E>(
    old: Result<plumb_core::Hash, E>,
    new: Result<plumb_core::Hash, E>,
) -> Result<bool, ApiError> {
    match (old, new) {
        (Ok(a), Ok(b)) => Ok(a != b),
        _ => Err(ApiError::internal("element_hash")),
    }
}
