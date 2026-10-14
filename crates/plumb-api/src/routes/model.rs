//! Model routes (plan A0.2): glossary, entities and attributes, calculations, processes,
//! findings, assumptions, decisions, exports and revision diffs. A0.3 (Hotfix 065) adds the
//! Term merge, the CRUD view, calculation examples, rules and decision-table cells, linear
//! process editing with canonical Mermaid interchange, waivers and assumption resolution.
//! U2.1 (Hotfix 073) adds the Term merge preview and the shared semantic change preview
//! pipeline the requirement split preview also uses.
//!
//! Reads project typed PSG material, typed relations and existing analyses of one loaded
//! revision; writes are typed status or payload replacements committed through the shared
//! intake and compare-and-swap pipeline.

use std::collections::{BTreeMap, BTreeSet};

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plumb_artifacts::{ArtifactKind, ArtifactStore};
use plumb_core::{to_canonical_json, Id, StageId, Timestamp};
use plumb_functional::decision_table::DecisionOutputCell;
use plumb_functional::waiver::{create_waiver, WaiverError};
use plumb_functional::{
    analyze_decision_table, calculation_example, decode_decision_table, encode_decision_table,
    linear_process_steps, linear_step_key, propose_assumption_resolution,
    propose_linear_process_import, propose_process_step_change, propose_term_merge,
    set_output_cell, AssumptionResolutionError, CalculationExampleError, DecisionCellError,
    DecisionCoverage, ExactValue, LinearInterchangeError, PilotDataClass, ProcessStepChange,
    ProcessStepError, TermMergeError,
};
use plumb_patch::{
    apply_patch, AcceptancePolicy, ElementPrecondition, GraphDelta, PatchSet, Proposal,
    ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    edge_contributes_to_semantic_hash, edge_element_hash, node_contributes_to_semantic_hash,
    node_element_hash, ConceptKind, ElementStatus, Graph, Node, NodePayload, NodeType,
    ProcessNodeKind, RelationKind,
};
use plumb_store::{LoadedRevision, RevisionId, WorkflowOrigin};
use plumb_validation::waiver::WaiverRequest;
use plumb_validation::{analyze_process, ParallelAnalysis, ProcessAnalysisMode};
use plumb_validation::{GeneratedFinding, ValidationPolicy};
use serde_json::{json, Value};

use crate::application::export_artifact;
use crate::error::ApiError;
use crate::openapi::{
    Assumption, AssumptionPage, AttributeClassRequest, AttributeTypeRequest, AttributeUnitRequest,
    Calculation, CalculationExample, CalculationPage, CalculationValue, CrudCell, CrudMatrix,
    Decision, DecisionPage, Entity, EntityAttribute, EntityPage, EntityRelationship,
    EntityTransition, ExportFormat, FindingPage, GlossaryEditRequest, GlossaryMergeRequest,
    GlossaryPage, ModelDiff, Process, ProcessFlowView, ProcessFormat, ProcessImportRequest,
    ProcessNodeView, ProcessNodeViewKind, ProcessPage, ProcessStepRequest, ProposalKind,
    ReasonRequest, Rule, RuleCellRequest, RulePage, RuleRow, RuleViewKind, ScalarValue,
    SemanticChangePreviewResponse, SemanticElementDiff, WriteResult,
};
use crate::state::{
    check_if_match, is_clean_text, paginate, parse_body, parse_id, parse_wire, read_json,
    require_clean_reason, run_intake, wire, with_etag, ApiState, PageRequest, QueryParams,
};
use crate::workflow::{classify, finding_view, intake_view};

use super::proposals::{persist_pending, Origin};

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
        .route("/api/glossary/{id}/merge", post(merge_glossary))
        .route(
            "/api/glossary/{id}/merge/preview",
            post(preview_glossary_merge),
        )
        .route("/api/model/crud", get(get_crud_matrix))
        .route(
            "/api/model/calculations/{id}/example",
            get(get_calculation_example),
        )
        .route("/api/model/rules", get(list_rules))
        .route("/api/model/rules/{id}", get(get_rule))
        .route("/api/model/rules/{id}/cells", post(set_rule_cell))
        .route("/api/processes/{id}/steps", post(mutate_process_steps))
        .route("/api/processes/import", post(import_process))
        .route("/api/processes/{id}/export", get(export_process))
        .route("/api/findings/{id}/waive", post(waive_finding))
        .route("/api/assumptions/{id}/resolve", post(resolve_assumption))
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
    let items = state
        .glossary(&loaded)?
        .into_iter()
        .map(|term| (term.id.clone(), term))
        .collect();
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

/// The API kind of a PSG ProcessNode kind (exhaustive; Hotfix 079).
fn process_node_view_kind(kind: ProcessNodeKind) -> ProcessNodeViewKind {
    match kind {
        ProcessNodeKind::Start => ProcessNodeViewKind::Start,
        ProcessNodeKind::End => ProcessNodeViewKind::End,
        ProcessNodeKind::HumanTask => ProcessNodeViewKind::HumanTask,
        ProcessNodeKind::ServiceTask => ProcessNodeViewKind::ServiceTask,
        ProcessNodeKind::ExclusiveGateway => ProcessNodeViewKind::ExclusiveGateway,
        ProcessNodeKind::ParallelSplit => ProcessNodeViewKind::ParallelSplit,
        ProcessNodeKind::ParallelJoin => ProcessNodeViewKind::ParallelJoin,
        ProcessNodeKind::MessageEvent => ProcessNodeViewKind::MessageEvent,
        ProcessNodeKind::TimerEvent => ProcessNodeViewKind::TimerEvent,
        ProcessNodeKind::ErrorEvent => ProcessNodeViewKind::ErrorEvent,
        ProcessNodeKind::Subprocess => ProcessNodeViewKind::Subprocess,
    }
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
    // Hotfix 079: the structured projection of every member node and the active `next`
    // topology between members.
    let members: BTreeSet<&Id> = analysis.node_refs.iter().collect();
    let refs = |member: &Id, kind: RelationKind| -> Vec<String> {
        let set: BTreeSet<String> = targets(graph, member, kind)
            .into_iter()
            .map(|t| t.to_string())
            .collect();
        set.into_iter().collect()
    };
    let mut nodes = Vec::new();
    let mut flows = BTreeSet::new();
    for member in &members {
        let Some(NodePayload::ProcessNode(p)) = graph.node(member).map(|n| &n.payload) else {
            continue;
        };
        nodes.push(ProcessNodeView {
            id: member.to_string(),
            kind: process_node_view_kind(p.node_kind),
            operation_ref: p.operation_ref.as_ref().map(Id::to_string),
            performer_refs: refs(member, RelationKind::PerformedBy),
            consumes_refs: refs(member, RelationKind::Consumes),
            produces_refs: refs(member, RelationKind::Produces),
            condition_expr: p.condition_expr.clone(),
            message_ref: p.message_ref.as_ref().map(Id::to_string),
            timer_expr: p.timer_expr.clone(),
        });
        for to in targets(graph, member, RelationKind::Next) {
            if members.contains(&to) {
                flows.insert((member.to_string(), to.to_string()));
            }
        }
    }
    Ok(Process {
        id: node.id.to_string(),
        name: payload.name.clone(),
        lanes: lanes.into_iter().collect(),
        steps,
        branches,
        validation: validation.into_iter().collect(),
        nodes,
        flows: flows
            .into_iter()
            .map(|(from, to)| ProcessFlowView { from, to })
            .collect(),
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
            finding_view(&node.id, f, state.profile())?,
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
    let outputs = state.pilot_projection(&loaded)?;
    let artifact = export_artifact(outputs, format);
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

// ============================================================================ A0.3 (Hotfix 065)

/// A MaterialDecision / HUMAN_DECISION proposal of `patch` on the loaded head.
fn material_proposal(
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
        ProposalMateriality::MaterialDecision,
        AcceptancePolicy::HumanDecision,
        None,
    )
    .map_err(|_| ApiError::internal("proposal"))
}

fn id_collision(id: &Id) -> ApiError {
    ApiError::conflict("E_ID_COLLISION", "a deterministic identity is already used")
        .with_details(json!({ "id": id }))
}

// ---------------------------------------------------------------------------- glossary

/// A validated Term merge: the loaded head, the exact `propose_term_merge` Proposal, the
/// clean reason and the one Clock read.
struct TermMerge {
    loaded: LoadedRevision,
    proposal: Proposal,
    reason: String,
    created_at: Timestamp,
}

/// The shared validation and Proposal construction of `merge_glossary` and
/// `preview_glossary_merge`, in the frozen order.
fn term_merge(
    state: &ApiState,
    id: &str,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<TermMerge, ApiError> {
    let request: GlossaryMergeRequest = parse_body(body)?;
    let into = parse_id(&request.into).ok_or_else(|| ApiError::invalid("into is not an ID"))?;
    if request.into == id {
        return Err(ApiError::invalid("a term cannot be merged into itself"));
    }
    require_clean_reason(&request.reason)?;
    let loaded = state.load_head()?;
    check_if_match(headers, &loaded)?;
    let created_at = state.now();
    let source = typed_node(&loaded.graph, id, NodeType::Term, "term")?;
    let target = typed_node(&loaded.graph, into.as_str(), NodeType::Term, "term")?;
    let proposal =
        propose_term_merge(&loaded.graph, &source.id, &target.id).map_err(|error| match error {
            TermMergeError::Conflict { field, .. } => ApiError::conflict(
                "E_TERM_MERGE_CONFLICT",
                "the terms differ in language or definition",
            )
            .with_details(json!({ "field": field })),
            TermMergeError::NotApplicable { reason } => ApiError::unprocessable(
                "E_PATCH_REJECTED",
                "the proposed change cannot be applied to the current graph",
            )
            .with_details(json!({ "patch_error": reason })),
            other => ApiError::semantic("TermMergeError", other.to_string()),
        })?;
    Ok(TermMerge {
        loaded,
        proposal,
        reason: request.reason,
        created_at,
    })
}

/// `merge_glossary`: the source Term (path) merges into the Accepted target through
/// `plumb_functional::vocabulary`.
async fn merge_glossary(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let merge = term_merge(&state, &id, &headers, &body)?;
    let write = state.write(
        &merge.loaded,
        &merge.proposal,
        true,
        Some(&merge.reason),
        merge.created_at,
        "merge_glossary",
    )?;
    Ok(Json(write).into_response())
}

/// `preview_glossary_merge` (U2.1, Hotfix 073): the exact `merge_glossary` Proposal, persisted
/// pending with its semantic element diff and committed only by `POST
/// /api/proposals/{id}/merge`.
async fn preview_glossary_merge(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let merge = term_merge(&state, &id, &headers, &body)?;
    semantic_change_preview(
        &state,
        &merge.loaded,
        &merge.proposal,
        ProposalKind::Merge,
        merge.created_at,
    )
}

// ---------------------------------------------------------------------------- previews

/// The shared semantic change preview (Hotfix 073) of an exact, already-built domain Proposal:
/// its server action class must be `expected` (anything else is an internal error, never a
/// preview); `apply_patch` dry-applies it once to the loaded graph for the semantic element
/// diff; X1.3 intake runs; the Proposal and report are persisted pending (origin intake) bound
/// to the loaded revision. Nothing is committed: no CAS, revision or DesignEvent.
pub(super) fn semantic_change_preview(
    state: &ApiState,
    loaded: &LoadedRevision,
    proposal: &Proposal,
    expected: ProposalKind,
    created_at: Timestamp,
) -> Result<Response, ApiError> {
    let action = classify(&loaded.graph, proposal)?;
    if action != expected {
        return Err(ApiError::internal("preview_action"));
    }
    let applied = apply_patch(&loaded.graph, &proposal.patch_set).map_err(|error| {
        ApiError::unprocessable(
            "E_PATCH_REJECTED",
            "the proposed change cannot be applied to the current graph",
        )
        .with_details(json!({ "patch_error": error.to_string() }))
    })?;
    let diff = semantic_element_diff(&loaded.graph, &applied.graph, &applied.delta);
    let report = run_intake(loaded, proposal)?;
    let stored = persist_pending(
        state,
        loaded,
        proposal,
        &report,
        Origin {
            kind: WorkflowOrigin::Intake,
            reference: None,
        },
        created_at,
    )?;
    let revision = loaded.revision.id.to_string();
    Ok(Json(SemanticChangePreviewResponse {
        version: revision.clone(),
        revision,
        semantic_hash: loaded.revision.semantic_hash.to_string(),
        events: Vec::new(),
        proposal_id: stored.proposal.id.to_string(),
        action,
        intake: intake_view(&stored.intake, state.profile())?,
        diff,
    })
    .into_response())
}

/// The semantic element diff of one `apply_patch` result: added elements that contribute to
/// `semantic_hash` in the candidate graph, removed elements that contributed in the base graph,
/// and modified elements that contribute in either graph (a lifecycle move into or out of the
/// projection stays modified). Node and edge IDs are merged, unique and sorted ascending.
fn semantic_element_diff(
    base: &Graph,
    candidate: &Graph,
    delta: &GraphDelta,
) -> SemanticElementDiff {
    let node_in = |graph: &Graph, id: &Id| {
        graph
            .node(id)
            .is_some_and(node_contributes_to_semantic_hash)
    };
    let edge_in = |graph: &Graph, id: &Id| {
        graph
            .edge(id)
            .is_some_and(|edge| edge_contributes_to_semantic_hash(graph, edge))
    };
    let collect = |nodes: &BTreeSet<Id>,
                   edges: &BTreeSet<Id>,
                   keep_node: &dyn Fn(&Id) -> bool,
                   keep_edge: &dyn Fn(&Id) -> bool| {
        let ids: BTreeSet<String> = nodes
            .iter()
            .filter(|id| keep_node(id))
            .chain(edges.iter().filter(|id| keep_edge(id)))
            .map(Id::to_string)
            .collect();
        ids.into_iter().collect::<Vec<String>>()
    };
    SemanticElementDiff {
        base_semantic_hash: delta.base_semantic_hash.to_string(),
        candidate_semantic_hash: delta.result_semantic_hash.to_string(),
        added: collect(
            &delta.added_nodes,
            &delta.added_edges,
            &|id| node_in(candidate, id),
            &|id| edge_in(candidate, id),
        ),
        removed: collect(
            &delta.removed_nodes,
            &delta.removed_edges,
            &|id| node_in(base, id),
            &|id| edge_in(base, id),
        ),
        modified: collect(
            &delta.modified_nodes,
            &delta.modified_edges,
            &|id| node_in(base, id) || node_in(candidate, id),
            &|id| edge_in(base, id) || edge_in(candidate, id),
        ),
    }
}

// ---------------------------------------------------------------------------- CRUD

/// `get_crud_matrix`: reads exactly from active `reads` edges; a written pair reports create,
/// update and delete as null because the PSG does not distinguish them.
async fn get_crud_matrix(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    QueryParams::parse(query.as_deref(), &[], false)?;
    let loaded = state.load_head()?;
    let g = &loaded.graph;
    let active_of = |node_type: NodeType| -> Vec<Id> {
        g.node_ids_by_type(node_type)
            .iter()
            .filter(|id| g.node(id).is_some_and(|n| active(n.status)))
            .cloned()
            .collect()
    };
    let operations = active_of(NodeType::Operation);
    let entities = active_of(NodeType::Entity);
    let related = |op: &Id, ent: &Id, kind: RelationKind| {
        g.outgoing_edge_ids(op)
            .iter()
            .filter_map(|e| g.edge(e))
            .any(|e| e.kind == kind && active(e.status) && &e.to == ent)
    };
    let mut cells = Vec::with_capacity(operations.len() * entities.len());
    for op in &operations {
        for ent in &entities {
            let written = related(op, ent, RelationKind::Writes);
            let unknown = if written { None } else { Some(false) };
            cells.push(CrudCell {
                op: op.to_string(),
                ent: ent.to_string(),
                c: unknown,
                r: related(op, ent, RelationKind::Reads),
                u: unknown,
                d: unknown,
            });
        }
    }
    Ok(read_json(
        &loaded,
        CrudMatrix {
            operations: operations.iter().map(Id::to_string).collect(),
            entities: entities.iter().map(Id::to_string).collect(),
            cells,
        },
    ))
}

// ---------------------------------------------------------------------------- calculation example

/// One JSON object whose keys must be unique (duplicate keys are rejected, never overwritten).
struct UniqueObject(Vec<(String, Value)>);

impl<'de> serde::Deserialize<'de> for UniqueObject {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueObject;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<UniqueObject, A::Error> {
                let mut entries: Vec<(String, Value)> = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    if entries.iter().any(|(k, _)| *k == key) {
                        return Err(serde::de::Error::custom(format!("duplicate key {key:?}")));
                    }
                    entries.push((key, value));
                }
                Ok(UniqueObject(entries))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

/// `get_calculation_example`: an exact example of an Accepted Calculation through
/// `plumb_functional::calculation` and the injected providers.
async fn get_calculation_example(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &["inputs"], false)?;
    let mut inputs: BTreeMap<String, ExactValue> = BTreeMap::new();
    if let Some(text) = query.get("inputs") {
        let object: UniqueObject = serde_json::from_str(text)
            .map_err(|e| ApiError::invalid(format!("inputs must be one JSON object: {e}")))?;
        for (key, value) in object.0 {
            let typed: CalculationValue = serde_json::from_value(value)
                .map_err(|e| ApiError::invalid(format!("input {key:?}: {e}")))?;
            let exact: ExactValue = serde_json::to_value(&typed)
                .and_then(serde_json::from_value)
                .map_err(|_| ApiError::internal("calculation_value"))?;
            inputs.insert(key, exact);
        }
    }
    let runtime = state.runtime("calculation_providers")?;
    let loaded = state.load_head()?;
    let target = typed_node(&loaded.graph, &id, NodeType::Calculation, "calculation")?;
    let example = calculation_example(
        &loaded.graph,
        &target.id,
        &inputs,
        runtime.calendars.as_ref(),
        runtime.predicates.as_ref(),
    )
    .map_err(|error| match error {
        CalculationExampleError::NotACalculation(id) => {
            ApiError::not_found(format!("calculation {id} not found"))
        }
        CalculationExampleError::ScopeUnavailable(id) => ApiError::conflict(
            "E_CALCULATION_SCOPE_UNAVAILABLE",
            "the calculation has no replayable scope",
        )
        .with_details(json!({ "calculation": id })),
        CalculationExampleError::InvalidInput { name, reason } => {
            ApiError::invalid(format!("input {name:?}: {reason}"))
        }
        CalculationExampleError::Evaluation { reason } => ApiError::unprocessable(
            "E_CALCULATION_EVALUATION",
            "the calculation cannot be evaluated with these inputs",
        )
        .with_details(json!({ "reason": reason })),
        CalculationExampleError::Unrepresentable { reason } => ApiError::unprocessable(
            "E_CALCULATION_VALUE_UNREPRESENTABLE",
            "the result has no exact calculation value form",
        )
        .with_details(json!({ "reason": reason })),
        other => ApiError::semantic("CalculationExampleError", other.to_string()),
    })?;
    let value: CalculationValue = serde_json::to_value(&example.value)
        .and_then(serde_json::from_value)
        .map_err(|_| ApiError::internal("calculation_value"))?;
    Ok(read_json(
        &loaded,
        CalculationExample {
            expr: example.expr,
            rendered_example: example.rendered_example,
            value,
        },
    ))
}

// ---------------------------------------------------------------------------- rules

fn encoding_invalid(id: &Id, error: impl ToString) -> ApiError {
    ApiError::conflict(
        "E_DECISION_TABLE_ENCODING_INVALID",
        "the persisted decision table encoding is malformed",
    )
    .with_details(json!({ "decision_table": id, "reason": error.to_string() }))
}

/// RFC 8785 text of a typed value.
fn canonical_text<T: serde::Serialize>(value: &T) -> Result<String, ApiError> {
    let bytes = to_canonical_json(value).map_err(|_| ApiError::internal("canonical_json"))?;
    String::from_utf8(bytes).map_err(|_| ApiError::internal("canonical_json"))
}

/// The Rule view of a Rule or DecisionTable node.
fn rule_view(node: &Node) -> Result<Option<Rule>, ApiError> {
    match &node.payload {
        NodePayload::Rule(rule) => Ok(Some(Rule {
            id: node.id.to_string(),
            name: rule.name.clone(),
            kind: RuleViewKind::Rule,
            conditions: rule.condition_expr.iter().cloned().collect(),
            rows: None,
            uncovered_cells: None,
            overlaps: None,
        })),
        NodePayload::DecisionTable(table) => {
            let spec = decode_decision_table(table).map_err(|e| encoding_invalid(&node.id, e))?;
            let conditions = spec
                .inputs
                .iter()
                .map(canonical_text)
                .collect::<Result<Vec<_>, _>>()?;
            let mut rows = Vec::with_capacity(spec.rows.len());
            for row in &spec.rows {
                rows.push(RuleRow {
                    when: row
                        .inputs
                        .iter()
                        .map(canonical_text)
                        .collect::<Result<_, _>>()?,
                    then: row
                        .outputs
                        .iter()
                        .map(canonical_text)
                        .collect::<Result<_, _>>()?,
                });
            }
            let analysis = analyze_decision_table(&spec);
            let overlaps = analysis.structure.is_empty().then(|| {
                analysis
                    .overlaps
                    .iter()
                    .map(|o| format!("{}:{}", o.left_row, o.right_row))
                    .collect()
            });
            let uncovered_cells = match &analysis.coverage {
                DecisionCoverage::Complete | DecisionCoverage::CompleteByDefault => {
                    Some(Vec::new())
                }
                DecisionCoverage::Incomplete { witnesses } => Some(
                    witnesses
                        .iter()
                        .map(canonical_text)
                        .collect::<Result<_, _>>()?,
                ),
                DecisionCoverage::IncompleteNumeric { gaps } => {
                    Some(gaps.iter().map(canonical_text).collect::<Result<_, _>>()?)
                }
                DecisionCoverage::NotAnalyzable { .. } | DecisionCoverage::StructureInvalid => None,
            };
            Ok(Some(Rule {
                id: node.id.to_string(),
                name: table.name.clone(),
                kind: RuleViewKind::DecisionTable,
                conditions,
                rows: Some(rows),
                uncovered_cells,
                overlaps,
            }))
        }
        _ => Ok(None),
    }
}

/// `list_rules`: every Rule and DecisionTable node.
async fn list_rules(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &[], true)?;
    let loaded = state.load_head()?;
    let mut items = Vec::new();
    for node_type in [NodeType::Rule, NodeType::DecisionTable] {
        for id in loaded.graph.node_ids_by_type(node_type) {
            let Some(node) = loaded.graph.node(id) else {
                continue;
            };
            if let Some(view) = rule_view(node)? {
                items.push((node.id.to_string(), view));
            }
        }
    }
    let (items, next_cursor) = simple_page(items, "list_rules", &query, &loaded)?;
    Ok(read_json(&loaded, RulePage { items, next_cursor }))
}

/// `get_rule`.
async fn get_rule(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let missing = || ApiError::not_found(format!("rule {id} not found"));
    let node = parse_id(&id)
        .and_then(|i| loaded.graph.node(&i))
        .ok_or_else(missing)?;
    let view = rule_view(node)?.ok_or_else(missing)?;
    Ok(read_json(&loaded, view))
}

/// Parses `row:<row>:output:<column>` with canonical zero-based indexes.
fn parse_cell(cell: &str) -> Option<(usize, usize)> {
    let rest = cell.strip_prefix("row:")?;
    let (row, column) = rest.split_once(":output:")?;
    let index = |text: &str| {
        let canonical = text == "0"
            || (!text.is_empty()
                && !text.starts_with('0')
                && text.bytes().all(|b| b.is_ascii_digit()));
        canonical.then(|| text.parse::<usize>().ok()).flatten()
    };
    Some((index(row)?, index(column)?))
}

/// `set_rule_cell`: replaces exactly one output cell of a DecisionTable through the strict
/// adapter; a governed human decision with a mandatory reason.
async fn set_rule_cell(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: RuleCellRequest = parse_body(&body)?;
    let (row, column) = parse_cell(&request.cell)
        .ok_or_else(|| ApiError::invalid("cell must be row:<row>:output:<column>"))?;
    let outcome: DecisionOutputCell = serde_json::from_str(&request.outcome)
        .map_err(|e| ApiError::invalid(format!("outcome is not a decision output cell: {e}")))?;
    if canonical_text(&outcome)? != request.outcome {
        return Err(ApiError::invalid(
            "outcome must be its own RFC 8785 encoding",
        ));
    }
    require_clean_reason(&request.reason)?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let node = parse_id(&id)
        .and_then(|i| loaded.graph.node(&i))
        .ok_or_else(|| ApiError::not_found(format!("rule {id} not found")))?;
    let NodePayload::DecisionTable(table) = &node.payload else {
        return Err(ApiError::conflict(
            "E_RULE_NOT_DECISION_TABLE",
            "only a DecisionTable has editable cells",
        )
        .with_details(json!({ "rule": node.id })));
    };
    let spec = decode_decision_table(table).map_err(|e| encoding_invalid(&node.id, e))?;
    let changed = set_output_cell(&spec, row, column, outcome).map_err(|error| match error {
        DecisionCellError::RowOutOfRange { .. } | DecisionCellError::ColumnOutOfRange { .. } => {
            ApiError::invalid(format!("cell {}: {error}", request.cell))
        }
        DecisionCellError::NoChange => {
            ApiError::unprocessable("E_NO_CHANGE", "the cell already has this outcome")
        }
        DecisionCellError::InvalidOutputCell { .. } => {
            ApiError::semantic("DecisionCellError", error.to_string())
        }
    })?;
    let encoded =
        encode_decision_table(&table.name, &changed).map_err(|e| encoding_invalid(&node.id, e))?;
    let proposal = material_proposal(
        &loaded,
        StageId::S2,
        SemanticPatch::ReplacePayload {
            target: precondition(node)?,
            payload: NodePayload::DecisionTable(encoded),
        },
    )?;
    let write = state.write(
        &loaded,
        &proposal,
        true,
        Some(&request.reason),
        created_at,
        "set_rule_cell",
    )?;
    Ok(Json(write).into_response())
}

// ---------------------------------------------------------------------------- processes

/// `mutate_process_steps`: one step change of a strict linear Accepted Process through
/// `plumb_functional::process`.
async fn mutate_process_steps(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: ProcessStepRequest = parse_body(&body)?;
    let reason = match &request {
        ProcessStepRequest::Add { reason, .. }
        | ProcessStepRequest::Move { reason, .. }
        | ProcessStepRequest::Remove { reason, .. }
        | ProcessStepRequest::MoveLane { reason, .. } => reason.clone(),
    };
    if let Some(reason) = &reason {
        require_clean_reason(reason)?;
    }
    let position =
        |p: u64| usize::try_from(p).map_err(|_| ApiError::invalid("position is too large"));
    let ref_of = |text: &str| {
        parse_id(text).ok_or_else(|| ApiError::invalid(format!("{text:?} is not an ID")))
    };
    let change = match &request {
        ProcessStepRequest::Add {
            operation_ref,
            position: p,
            ..
        } => ProcessStepChange::Add {
            operation_ref: ref_of(operation_ref)?,
            position: position(*p)?,
        },
        ProcessStepRequest::Move {
            node_ref,
            position: p,
            ..
        } => ProcessStepChange::Move {
            node_ref: ref_of(node_ref)?,
            position: position(*p)?,
        },
        ProcessStepRequest::Remove { node_ref, .. } => ProcessStepChange::Remove {
            node_ref: ref_of(node_ref)?,
        },
        ProcessStepRequest::MoveLane {
            node_ref,
            performer_ref,
            ..
        } => ProcessStepChange::MoveLane {
            node_ref: ref_of(node_ref)?,
            performer_ref: ref_of(performer_ref)?,
        },
    };
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let process = typed_node(&loaded.graph, &id, NodeType::Process, "process")?;
    let proposal = propose_process_step_change(
        &loaded.graph,
        &process.id,
        &change,
        state.actor_ref(),
        created_at,
    )
    .map_err(|error| match error {
        ProcessStepError::Linear(e) => ApiError::conflict(
            "E_PROCESS_NOT_LINEAR",
            "step editing is defined only for a strict linear Accepted Process",
        )
        .with_details(json!({ "reason": e.to_string() })),
        ProcessStepError::Missing(id) => ApiError::not_found(format!("{id} not found")),
        ProcessStepError::PositionOutOfRange { .. } => ApiError::invalid(error.to_string()),
        ProcessStepError::NoChange(_) => ApiError::unprocessable(
            "E_NO_CHANGE",
            "the requested process step change makes no semantic change",
        ),
        ProcessStepError::HasRelations { node_ref, edges } => ApiError::conflict(
            "E_PROCESS_STEP_HAS_RELATIONS",
            "the step has relations other than its flow",
        )
        .with_details(json!({ "node_ref": node_ref, "edges": edges })),
        ProcessStepError::IdCollision(id) => id_collision(&id),
        ProcessStepError::MutationInvalid { reason } => ApiError::unprocessable(
            "E_PROCESS_MUTATION_INVALID",
            "the changed process fails process analysis",
        )
        .with_details(json!({ "reason": reason })),
        other => ApiError::semantic("ProcessStepError", other.to_string()),
    })?;
    let write = state.write(
        &loaded,
        &proposal,
        false,
        reason.as_deref(),
        created_at,
        "mutate_process_steps",
    )?;
    Ok(Json(write).into_response())
}

/// The canonical Mermaid text of a linear process (Hotfix 065).
fn render_mermaid(name: &str, operation_refs: &[Id]) -> Result<String, ApiError> {
    let name = serde_json::to_string(name).map_err(|_| ApiError::internal("mermaid"))?;
    let mut text = format!("flowchart TD\n%% plumb-process-name: {name}\n  start((Start))\n");
    let keys: Vec<String> = (0..operation_refs.len()).map(linear_step_key).collect();
    for (key, operation) in keys.iter().zip(operation_refs) {
        text.push_str(&format!("  {key}[\"{operation}\"]\n"));
    }
    text.push_str("  end((End))\n");
    let mut order = vec!["start".to_owned()];
    order.extend(keys);
    order.push("end".to_owned());
    for pair in order.windows(2) {
        text.push_str(&format!("  {} --> {}\n", pair[0], pair[1]));
    }
    Ok(text)
}

/// Parses exactly the canonical Mermaid subset: the name and the operation of each step, then
/// requires the canonical re-rendering to equal `text` byte for byte.
fn parse_mermaid(text: &str) -> Result<(String, Vec<Id>), ApiError> {
    let invalid = || {
        ApiError::bad_request(
            "E_MERMAID_INVALID",
            "the text is not canonical Plumb Mermaid",
        )
    };
    let mut lines = text.split('\n');
    if lines.next() != Some("flowchart TD") {
        return Err(invalid());
    }
    let name_line = lines.next().ok_or_else(invalid)?;
    let name: String = serde_json::from_str(
        name_line
            .strip_prefix("%% plumb-process-name: ")
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())?;
    if lines.next() != Some("  start((Start))") {
        return Err(invalid());
    }
    let mut operations = Vec::new();
    for line in lines {
        let Some(rest) = line.strip_prefix(&format!("  {}[\"", linear_step_key(operations.len())))
        else {
            break;
        };
        let operation = rest.strip_suffix("\"]").ok_or_else(invalid)?;
        operations.push(parse_id(operation).ok_or_else(invalid)?);
    }
    if render_mermaid(&name, &operations)? != text {
        return Err(invalid());
    }
    Ok((name, operations))
}

/// `import_process`: a canonical Mermaid linear process imported through
/// `plumb_functional::process` with the existing process identities; never overwrites.
async fn import_process(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: ProcessImportRequest = parse_body(&body)?;
    let ProcessFormat::Mermaid = request.format;
    let normalized = request.text.replace("\r\n", "\n").replace('\r', "\n");
    let (name, operations) = parse_mermaid(&normalized)?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let proposal = propose_linear_process_import(
        &loaded.graph,
        &name,
        &operations,
        state.actor_ref(),
        created_at,
    )
    .map_err(|error| match error {
        LinearInterchangeError::InvalidName(_) => {
            ApiError::bad_request("E_MERMAID_INVALID", error.to_string())
        }
        LinearInterchangeError::OperationInvalid(_) => {
            ApiError::semantic("LinearInterchangeError", error.to_string())
        }
        LinearInterchangeError::NoChange(_) => {
            ApiError::unprocessable("E_NO_CHANGE", "an identical process already exists")
        }
        LinearInterchangeError::Conflict(id) => ApiError::conflict(
            "E_PROCESS_IMPORT_CONFLICT",
            "the process exists with different material",
        )
        .with_details(json!({ "process": id })),
        LinearInterchangeError::IdCollision(id) => id_collision(&id),
        other => ApiError::semantic("LinearInterchangeError", other.to_string()),
    })?;
    let write = state.write(
        &loaded,
        &proposal,
        false,
        None,
        created_at,
        "import_process",
    )?;
    Ok(Json(write).into_response())
}

/// `export_process`: the canonical Mermaid text of a representable linear process.
async fn export_process(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &["format"], false)?;
    let _format: ProcessFormat = parse_wire(
        query
            .get("format")
            .ok_or_else(|| ApiError::invalid("format is required"))?,
        "format",
    )?;
    let loaded = state.load_head()?;
    let process = typed_node(&loaded.graph, &id, NodeType::Process, "process")?;
    let steps = linear_process_steps(&loaded.graph, &process.id).map_err(|e| {
        ApiError::conflict(
            "E_MERMAID_UNREPRESENTABLE",
            "the process is outside the canonical Mermaid subset",
        )
        .with_details(json!({ "reason": e.to_string() }))
    })?;
    let text = render_mermaid(&steps.name, &steps.operation_refs)?;
    let mut response = text.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    Ok(with_etag(&loaded, response))
}

// ---------------------------------------------------------------------------- waivers

/// The Hotfix 048 waiver of current finding `finding_id` (Hotfix 065): the waiver material
/// comes from the current, head-bound gate reports and is checked against the persisted
/// Finding; `requirement` additionally scopes it to an Accepted Requirement.
pub(super) fn waive(
    state: &ApiState,
    loaded: &LoadedRevision,
    finding_id: &Id,
    requirement: Option<&Id>,
    reason: &str,
    created_at: Timestamp,
    operation_id: &'static str,
) -> Result<WriteResult, ApiError> {
    let reports = state.current_gate_reports(loaded)?;
    let generated: Vec<(&GeneratedFinding, &ValidationPolicy)> = reports
        .iter()
        .flat_map(|r| r.findings.iter().map(move |f| (f, &r.policy)))
        .filter(|(f, _)| &f.id == finding_id)
        .collect();
    let Some((first, policy)) = generated.first().copied() else {
        return Err(ApiError::conflict(
            "E_FINDING_NOT_CURRENT",
            "no current gate report generated this finding",
        )
        .with_details(json!({ "finding": finding_id })));
    };
    if generated.iter().any(|(f, p)| {
        f.key != first.key
            || f.semantic_condition_key != first.semantic_condition_key
            || f.payload != first.payload
            || *p != policy
    }) {
        return Err(ApiError::conflict(
            "E_GATE_REPORT_INCOHERENT",
            "current gate reports disagree about this finding",
        )
        .with_details(json!({ "finding": finding_id })));
    }
    let material_mismatch = || {
        ApiError::conflict(
            "E_FINDING_MATERIAL_MISMATCH",
            "the persisted finding does not carry the evaluated identity",
        )
        .with_details(json!({ "finding": finding_id }))
    };
    let persisted = loaded
        .graph
        .node(finding_id)
        .ok_or_else(material_mismatch)?;
    let NodePayload::Finding(finding) = &persisted.payload else {
        return Err(material_mismatch());
    };
    if persisted.status != ElementStatus::Accepted
        || finding.code != first.payload.code
        || finding.family != first.payload.family
        || finding.affected_refs != first.payload.affected_refs
    {
        return Err(material_mismatch());
    }
    if let Some(requirement) = requirement {
        if !first.payload.affected_refs.contains(requirement) {
            return Err(ApiError::conflict(
                "E_FINDING_NOT_FOR_REQUIREMENT",
                "the finding does not affect this requirement",
            )
            .with_details(json!({ "finding": finding_id, "requirement": requirement })));
        }
    }
    let rule = state
        .profile()
        .rules
        .iter()
        .find(|r| r.id == first.payload.code)
        .ok_or_else(|| {
            ApiError::conflict(
                "E_GATE_REPORT_INCOHERENT",
                "the finding's rule is not part of the current profile",
            )
            .with_details(json!({ "finding": finding_id }))
        })?;
    let created = create_waiver(
        &loaded.graph,
        &WaiverRequest {
            rule,
            targets: &first.payload.affected_refs,
            semantic_condition_key: &first.semantic_condition_key,
            decided_by: state.actor_ref(),
            decided_at: created_at,
            rationale: reason,
        },
        policy,
    )
    .map_err(|error| {
        let name = match &error {
            WaiverError::Material(_) => "WaiverCreationError",
            WaiverError::InvalidWaiverProposal { .. } => "WaiverError",
        };
        ApiError::semantic(name, error.to_string())
    })?;
    let stored = state.with_artifact_store(|store| {
        store.put(
            ArtifactKind::Patch,
            "application/json",
            &created.artifact_bytes,
            created_at,
        )
    })?;
    match stored {
        Ok(hash) if hash == created.decision.patch_ref => {}
        _ => return Err(ApiError::internal("artifact_store")),
    }
    state.write(
        loaded,
        &created.proposal,
        true,
        Some(reason),
        created_at,
        operation_id,
    )
}

/// `waive_finding`.
async fn waive_finding(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: ReasonRequest = parse_body(&body)?;
    require_clean_reason(&request.reason)?;
    let finding_id =
        parse_id(&id).ok_or_else(|| ApiError::not_found(format!("finding {id} not found")))?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let write = waive(
        &state,
        &loaded,
        &finding_id,
        None,
        &request.reason,
        created_at,
        "waive_finding",
    )?;
    Ok(Json(write).into_response())
}

// ---------------------------------------------------------------------------- assumptions

/// `resolve_assumption`: Accepted -> Resolved through `plumb_functional::assumption`.
async fn resolve_assumption(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: ReasonRequest = parse_body(&body)?;
    require_clean_reason(&request.reason)?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let node = typed_node(&loaded.graph, &id, NodeType::Assumption, "assumption")?;
    let proposal =
        propose_assumption_resolution(&loaded.graph, &node.id).map_err(|error| match error {
            AssumptionResolutionError::AlreadyResolved(_) => {
                ApiError::unprocessable("E_NO_CHANGE", "the assumption is already Resolved")
            }
            AssumptionResolutionError::NotActive { status, .. } => ApiError::conflict(
                "E_ASSUMPTION_NOT_ACTIVE",
                "only an Accepted assumption can be resolved",
            )
            .with_details(json!({ "status": status })),
            other => ApiError::semantic("AssumptionResolutionError", other.to_string()),
        })?;
    let write = state.write(
        &loaded,
        &proposal,
        true,
        Some(&request.reason),
        created_at,
        "resolve_assumption",
    )?;
    Ok(Json(write).into_response())
}
