//! Scenario routes (plan A0.3, Hotfix 065). The Scenario view exposes the canonical JSON of the
//! stored Given, When and Then values, the node status and the separate confirmation status.
//! Confirmation, correction and run updates go through `plumb_functional::scenario`; runs use
//! the existing simulator and keep one stable ScenarioRun node per Scenario whose
//! `plumb_sim:scenario_trace_artifact` extension links its current ScenarioTrace artifact. A
//! committed graph never pairs a new run payload with an old link.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plumb_artifacts::{ArtifactKind, ArtifactStore, ArtifactStoreError};
use plumb_core::{to_canonical_json, Hash, HashKind, Id};
use plumb_functional::{
    decode_decision_table, propose_derived_scenario_confirmation, propose_scenario_confirmation,
    propose_scenario_correction, propose_scenario_run_updates, ScenarioDecisionTableInput,
    ScenarioLifecycleError, ScenarioRunUpdate, SCENARIO_CONFIRMED,
};
use plumb_psg::{
    ElementStatus, ExtensionKey, Graph, Node, NodePayload, NodeType, Scenario as PsgScenario,
    ScenarioRun as PsgScenarioRun,
};
use plumb_sim::{
    build_execution_model, compare_run, run, scenario_run_id, ExecutionInputs, ExecutionRuntime,
    SCENARIO_TRACE_LINK_EXTENSION,
};
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::openapi::{
    ConfirmDerivedRequest, Scenario, ScenarioConfirmationStatus, ScenarioCorrectRequest,
    ScenarioPage, ScenarioRun, ScenarioRunRequest, ScenarioRunResult, ScenarioStatus, WriteResult,
};
use crate::state::{
    check_if_match, paginate, parse_body, parse_id, parse_wire, read_json, wire, ApiState,
    PageRequest, QueryParams,
};

use super::questions::error_name;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/scenarios", get(list_scenarios))
        .route(
            "/api/scenarios/confirm-derived",
            post(confirm_derived_scenarios),
        )
        .route("/api/scenarios/run", post(run_scenarios))
        .route("/api/scenarios/{id}/confirm", post(confirm_scenario))
        .route("/api/scenarios/{id}/correct", post(correct_scenario))
        .route("/api/scenarios/{id}/run", get(get_scenario_run))
}

/// RFC 8785 text of a JSON value.
fn canonical_text<T: serde::Serialize>(value: &T) -> Result<String, ApiError> {
    let bytes = to_canonical_json(value).map_err(|_| ApiError::internal("canonical_json"))?;
    String::from_utf8(bytes).map_err(|_| ApiError::internal("canonical_json"))
}

fn trace_conflict(code: &'static str, scenario: &Id, reason: &str) -> ApiError {
    ApiError::conflict(code, "the scenario run's trace link or artifact is invalid")
        .with_details(json!({ "scenario": scenario, "reason": reason }))
}

fn run_collision(run_ref: &Id) -> ApiError {
    ApiError::unprocessable(
        "E_SCENARIO_RUN_COLLISION",
        "the stable scenario run ID holds another element",
    )
    .with_details(json!({ "scenario_run": run_ref }))
}

fn link_key() -> Result<ExtensionKey, ApiError> {
    SCENARIO_TRACE_LINK_EXTENSION
        .parse()
        .map_err(|_| ApiError::internal("scenario_trace_link"))
}

/// A current ScenarioRun node with its payload.
type CurrentRun<'g> = Option<(&'g Node, &'g PsgScenarioRun)>;

/// The current ScenarioRun node of `scenario` with its payload, if any; any other element at the
/// stable ID is a collision.
fn current_run<'g>(graph: &'g Graph, scenario: &Id) -> Result<(Id, CurrentRun<'g>), ApiError> {
    let run_ref = scenario_run_id(graph.project_id(), scenario)
        .map_err(|_| ApiError::internal("scenario_run_id"))?;
    if graph.edge(&run_ref).is_some() {
        return Err(run_collision(&run_ref));
    }
    let Some(node) = graph.node(&run_ref) else {
        return Ok((run_ref, None));
    };
    match &node.payload {
        NodePayload::ScenarioRun(payload)
            if node.status == ElementStatus::Accepted && &payload.scenario_ref == scenario =>
        {
            Ok((run_ref, Some((node, payload))))
        }
        _ => Err(run_collision(&run_ref)),
    }
}

/// The `missing` entries of an Undecidable run: the MissingSemantic entries of the persisted
/// semantic-missing execution and expected-semantic-missing comparison steps, in trace order.
fn missing_entries(trace: &[Value]) -> Result<Vec<String>, ApiError> {
    let mut out = Vec::new();
    for entry in trace {
        let phase = entry.get("phase").and_then(Value::as_str);
        let step = entry.get("step");
        let name = step.and_then(|s| s.get("step")).and_then(Value::as_str);
        let carries = matches!(
            (phase, name),
            (Some("execution"), Some("semantic_missing"))
                | (Some("comparison"), Some("expected_semantic_missing"))
        );
        if !carries {
            continue;
        }
        let missing = step
            .and_then(|s| s.get("missing"))
            .and_then(Value::as_array)
            .ok_or_else(|| ApiError::internal("scenario_trace"))?;
        for m in missing {
            out.push(canonical_text(m)?);
        }
    }
    Ok(out)
}

/// The validated projection of the current ScenarioRun of `scenario`: the trace link and the
/// linked artifact must exist, have the frozen kind, media type and canonical bytes, and agree
/// with the node. Fail diffs come only from the artifact. Nothing is ever repaired.
fn run_view(
    state: &ApiState,
    graph: &Graph,
    scenario: &Id,
) -> Result<Option<ScenarioRun>, ApiError> {
    let (_, current) = current_run(graph, scenario)?;
    let Some((node, payload)) = current else {
        return Ok(None);
    };
    let link = node.extensions.get(&link_key()?).ok_or_else(|| {
        trace_conflict(
            "E_SCENARIO_RUN_TRACE_LINK_INVALID",
            scenario,
            "the link is absent",
        )
    })?;
    let hash: Hash = link
        .as_str()
        .and_then(|text| text.parse().ok())
        .filter(|h: &Hash| h.kind() == HashKind::Generic)
        .ok_or_else(|| {
            trace_conflict(
                "E_SCENARIO_RUN_TRACE_LINK_INVALID",
                scenario,
                "the link is not a generic sha256 hash string",
            )
        })?;
    let artifact = match state.with_artifact_store(|store| store.get(&hash))? {
        Ok(artifact) => artifact,
        Err(ArtifactStoreError::NotFound(_)) => {
            return Err(trace_conflict(
                "E_SCENARIO_RUN_TRACE_ARTIFACT_MISSING",
                scenario,
                "the linked artifact does not exist",
            ))
        }
        Err(_) => return Err(ApiError::internal("artifact_store")),
    };
    if artifact.kind != ArtifactKind::ScenarioTrace || artifact.media_type != "application/json" {
        return Err(trace_conflict(
            "E_SCENARIO_RUN_TRACE_ARTIFACT_INVALID",
            scenario,
            "the linked artifact is not a JSON scenario trace",
        ));
    }
    let mismatch = |reason: &str| trace_conflict("E_SCENARIO_RUN_TRACE_MISMATCH", scenario, reason);
    let body: Value = serde_json::from_slice(&artifact.bytes)
        .map_err(|_| mismatch("the artifact is not JSON"))?;
    if to_canonical_json(&body).ok().as_deref() != Some(artifact.bytes.as_slice()) {
        return Err(mismatch("the artifact is not canonical RFC 8785 JSON"));
    }
    let agrees = body.get("scenario_ref") == Some(&json!(payload.scenario_ref))
        && body.get("specification_hash") == Some(&json!(payload.specification_hash))
        && body.get("result") == Some(&json!(payload.result))
        && body.get("trace") == Some(&Value::Array(payload.trace.clone()));
    if !agrees {
        return Err(mismatch("the artifact disagrees with the scenario run"));
    }
    let result: ScenarioRunResult = parse_wire(&wire(&payload.result)?, "result")
        .map_err(|_| ApiError::internal("scenario_run_result"))?;
    let trace = payload
        .trace
        .iter()
        .map(canonical_text)
        .collect::<Result<Vec<_>, _>>()?;
    let (diff, missing) = match result {
        ScenarioRunResult::Pass => (None, None),
        ScenarioRunResult::Fail => {
            let diffs = body
                .get("diffs")
                .and_then(Value::as_array)
                .ok_or_else(|| mismatch("the artifact has no diffs"))?;
            (
                Some(
                    diffs
                        .iter()
                        .map(canonical_text)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                None,
            )
        }
        ScenarioRunResult::Undecidable => (None, Some(missing_entries(&payload.trace)?)),
    };
    Ok(Some(ScenarioRun {
        result,
        diff,
        missing,
        trace,
    }))
}

/// The Scenario view; a confirmation status outside Derived and Confirmed is a typed conflict.
fn scenario_view(
    state: &ApiState,
    graph: &Graph,
    node: &Node,
    scenario: &PsgScenario,
) -> Result<Scenario, ApiError> {
    let confirmation_status = match scenario.confirmation_status.as_deref() {
        Some("Derived") => ScenarioConfirmationStatus::Derived,
        Some("Confirmed") => ScenarioConfirmationStatus::Confirmed,
        other => {
            return Err(ApiError::conflict(
                "E_SCENARIO_PROJECTION_INVALID",
                "the scenario has no Derived or Confirmed confirmation status",
            )
            .with_details(json!({ "scenario": node.id, "confirmation_status": other })))
        }
    };
    let status: ScenarioStatus = parse_wire(&wire(&node.status)?, "status")
        .map_err(|_| ApiError::internal("scenario_status"))?;
    Ok(Scenario {
        id: node.id.to_string(),
        given: scenario
            .given
            .iter()
            .map(canonical_text)
            .collect::<Result<_, _>>()?,
        when: canonical_text(&scenario.when)?,
        then: scenario
            .then
            .iter()
            .map(canonical_text)
            .collect::<Result<_, _>>()?,
        status,
        confirmation_status,
        derived_from: scenario
            .derived_from_refs
            .iter()
            .flatten()
            .map(Id::to_string)
            .collect(),
        run: run_view(state, graph, &node.id)?,
    })
}

/// `list_scenarios`: every Scenario node filtered by requirement, status and derivation.
async fn list_scenarios(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(
        query.as_deref(),
        &["requirement", "status", "derived"],
        true,
    )?;
    let requirement = query
        .get("requirement")
        .map(|r| parse_id(r).ok_or_else(|| ApiError::invalid("requirement is not an ID")))
        .transpose()?;
    let status = query
        .get("status")
        .map(|s| parse_wire::<ScenarioStatus>(s, "status"))
        .transpose()?;
    let derived = match query.get("derived") {
        None => None,
        Some("true") => Some(ScenarioConfirmationStatus::Derived),
        Some("false") => Some(ScenarioConfirmationStatus::Confirmed),
        Some(_) => return Err(ApiError::invalid("derived must be true or false")),
    };
    let loaded = state.load_head()?;
    let mut items = Vec::new();
    for id in loaded.graph.node_ids_by_type(NodeType::Scenario) {
        let Some(node) = loaded.graph.node(id) else {
            continue;
        };
        let NodePayload::Scenario(scenario) = &node.payload else {
            continue;
        };
        if requirement
            .as_ref()
            .is_some_and(|r| !scenario.requirement_refs.iter().flatten().any(|x| x == r))
        {
            continue;
        }
        let view = scenario_view(&state, &loaded.graph, node, scenario)?;
        if status.is_some_and(|s| s != view.status)
            || derived.is_some_and(|d| d != view.confirmation_status)
        {
            continue;
        }
        items.push((node.id.to_string(), view));
    }
    let request = PageRequest {
        operation_id: "list_scenarios",
        path: Default::default(),
        query: &query,
    };
    let (items, next_cursor) = paginate(items, &request, &loaded)?;
    Ok(read_json(&loaded, ScenarioPage { items, next_cursor }))
}

/// The Scenario node `id`, or 404.
fn scenario_node<'g>(graph: &'g Graph, id: &str) -> Result<&'g Node, ApiError> {
    let missing = || ApiError::not_found(format!("scenario {id} not found"));
    parse_id(id)
        .and_then(|i| graph.node(&i))
        .filter(|n| n.payload.node_type() == NodeType::Scenario)
        .ok_or_else(missing)
}

fn lifecycle_error(error: ScenarioLifecycleError) -> ApiError {
    match error {
        ScenarioLifecycleError::NotAScenario(id) => {
            ApiError::not_found(format!("scenario {id} not found"))
        }
        ScenarioLifecycleError::NotConfirmable { scenario_ref, .. } => ApiError::conflict(
            "E_SCENARIO_NOT_CONFIRMABLE",
            "only a Proposed, Derived scenario can be confirmed",
        )
        .with_details(json!({ "scenario": scenario_ref })),
        ScenarioLifecycleError::StateInvalid { scenario_ref, .. } => ApiError::conflict(
            "E_SCENARIO_STATE_INVALID",
            "only a Proposed Derived or an Accepted Confirmed scenario can be corrected",
        )
        .with_details(json!({ "scenario": scenario_ref })),
        ScenarioLifecycleError::DecisionTableMissing(id) => {
            ApiError::not_found(format!("decision table {id} not found"))
        }
        ScenarioLifecycleError::DecisionTableInvalid(id) => ApiError::conflict(
            "E_DECISION_TABLE_INVALID",
            "table_id is not an Accepted DecisionTable",
        )
        .with_details(json!({ "decision_table": id })),
        ScenarioLifecycleError::NoConfirmableDerived(id) => ApiError::conflict(
            "E_NO_CONFIRMABLE_DERIVED_SCENARIOS",
            "no Proposed, Derived scenario of this table remains",
        )
        .with_details(json!({ "decision_table": id })),
        ScenarioLifecycleError::InvalidThen { reason } => {
            ApiError::invalid(format!("then is not a valid Then wire: {reason}"))
        }
        ScenarioLifecycleError::NoChange(_) => {
            ApiError::unprocessable("E_NO_CHANGE", "the scenario already has this Then")
        }
        other => ApiError::semantic(&error_name(&other), other.to_string()),
    }
}

/// `confirm_scenario`: Proposed + Derived -> Accepted + Confirmed in one proposal.
async fn confirm_scenario(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let node = scenario_node(&loaded.graph, &id)?;
    let proposal =
        propose_scenario_confirmation(&loaded.graph, &node.id).map_err(lifecycle_error)?;
    let write = state.write(
        &loaded,
        &proposal,
        false,
        None,
        created_at,
        "confirm_scenario",
    )?;
    Ok(Json(write).into_response())
}

/// `confirm_derived_scenarios`: confirms every already-derived Scenario of one Accepted
/// DecisionTable in one proposal; never derives.
async fn confirm_derived_scenarios(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: ConfirmDerivedRequest = parse_body(&body)?;
    let table = parse_id(&request.table_id).ok_or_else(|| {
        ApiError::not_found(format!("decision table {} not found", request.table_id))
    })?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let (proposal, _) =
        propose_derived_scenario_confirmation(&loaded.graph, &table).map_err(lifecycle_error)?;
    let write = state.write(
        &loaded,
        &proposal,
        false,
        None,
        created_at,
        "confirm_derived_scenarios",
    )?;
    Ok(Json(write).into_response())
}

/// `correct_scenario`: replaces only Then; each entry must be its own RFC 8785 text.
async fn correct_scenario(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: ScenarioCorrectRequest = parse_body(&body)?;
    let mut then = Vec::with_capacity(request.then.len());
    for (index, text) in request.then.iter().enumerate() {
        let value: Value = serde_json::from_str(text)
            .map_err(|_| ApiError::invalid(format!("then[{index}] is not JSON")))?;
        if &canonical_text(&value)? != text {
            return Err(ApiError::invalid(format!(
                "then[{index}] must be its own RFC 8785 encoding"
            )));
        }
        then.push(value);
    }
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let node = scenario_node(&loaded.graph, &id)?;
    let proposal =
        propose_scenario_correction(&loaded.graph, &node.id, then).map_err(lifecycle_error)?;
    let write = state.write(
        &loaded,
        &proposal,
        false,
        None,
        created_at,
        "correct_scenario",
    )?;
    Ok(Json(write).into_response())
}

/// Whether a Scenario is runnable: Accepted and Confirmed, matching simulator admission.
fn runnable(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
        && matches!(&node.payload, NodePayload::Scenario(s)
            if s.confirmation_status.as_deref() == Some(SCENARIO_CONFIRMED))
}

/// `run_scenarios`: runs the selected runnable Scenarios with the injected runtime, persists
/// each ScenarioTrace artifact before the CAS and updates every stable ScenarioRun in one
/// non-semantic proposal; nothing changes means no CAS.
async fn run_scenarios(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request: ScenarioRunRequest = parse_body(&body)?;
    let mut scope = Vec::with_capacity(request.scope.len());
    for text in &request.scope {
        let id =
            parse_id(text).ok_or_else(|| ApiError::invalid(format!("{text:?} is not an ID")))?;
        if id.as_str() != text {
            return Err(ApiError::invalid(format!("{text:?} is not a canonical ID")));
        }
        scope.push(id);
    }
    if scope.windows(2).any(|w| w[0] >= w[1]) {
        return Err(ApiError::invalid(
            "scope must be strictly sorted and unique",
        ));
    }
    let runtime = state.runtime("scenario_runtime")?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let graph = &loaded.graph;
    let selected: Vec<Id> = if scope.is_empty() {
        let all: Vec<Id> = graph
            .node_ids_by_type(NodeType::Scenario)
            .iter()
            .filter(|id| graph.node(id).is_some_and(runnable))
            .cloned()
            .collect();
        if all.is_empty() {
            return Err(ApiError::conflict(
                "E_NO_RUNNABLE_SCENARIOS",
                "no Accepted, Confirmed scenario exists",
            ));
        }
        all
    } else {
        for id in &scope {
            let node = scenario_node(graph, id.as_str())?;
            if !runnable(node) {
                return Err(ApiError::conflict(
                    "E_SCENARIO_NOT_RUNNABLE",
                    "only an Accepted, Confirmed scenario runs",
                )
                .with_details(json!({ "scenario": id })));
            }
        }
        scope
    };
    let mut decision_tables = Vec::new();
    for id in graph.node_ids_by_type(NodeType::DecisionTable) {
        let Some(node) = graph.node(id) else {
            continue;
        };
        let NodePayload::DecisionTable(table) = &node.payload else {
            continue;
        };
        if node.status != ElementStatus::Accepted {
            continue;
        }
        let spec = decode_decision_table(table).map_err(|e| {
            ApiError::conflict(
                "E_DECISION_TABLE_ENCODING_INVALID",
                "the persisted decision table encoding is malformed",
            )
            .with_details(json!({ "decision_table": node.id, "reason": e.to_string() }))
        })?;
        decision_tables.push(ScenarioDecisionTableInput {
            decision_table_ref: node.id.clone(),
            spec,
        });
    }
    let simulation =
        |e: &dyn std::fmt::Debug, text: String| ApiError::semantic(&error_name(&e), text);
    let model = build_execution_model(
        graph,
        ExecutionInputs {
            decision_tables,
            calculation_scope_overrides: runtime.calculation_scope_overrides.clone(),
            expression_scopes: runtime.expression_scopes.clone(),
        },
    )
    .map_err(|e| simulation(&e, e.to_string()))?;
    let execution_runtime = ExecutionRuntime {
        actor_ref: Some(state.actor_ref().clone()),
        semantic_values: runtime.runtime_semantic_values.clone(),
        calendars: runtime.calendars.as_ref(),
        predicates: runtime.predicates.as_ref(),
    };
    let mut updates = Vec::with_capacity(selected.len());
    for scenario in &selected {
        let execution =
            run(&model, scenario, &execution_runtime).map_err(|e| simulation(&e, e.to_string()))?;
        let comparison = compare_run(&model, &execution, &execution_runtime)
            .map_err(|e| simulation(&e, e.to_string()))?;
        let artifact = &comparison.trace_artifact;
        let stored = state.with_artifact_store(|store| {
            store.put(
                ArtifactKind::ScenarioTrace,
                "application/json",
                &artifact.bytes,
                created_at,
            )
        })?;
        match stored {
            Ok(hash) if hash == artifact.hash => {}
            _ => return Err(ApiError::internal("artifact_store")),
        }
        let (run_ref, _) = current_run(graph, scenario)?;
        updates.push(ScenarioRunUpdate {
            run_ref,
            scenario_ref: scenario.clone(),
            payload: comparison.scenario_run,
            link_key: link_key()?,
            link: artifact.hash.clone(),
        });
    }
    let proposal = propose_scenario_run_updates(graph, &updates, state.actor_ref(), created_at)
        .map_err(|error| match error {
            ScenarioLifecycleError::RunCollision(id) => run_collision(&id),
            other => ApiError::semantic(&error_name(&other), other.to_string()),
        })?;
    let Some(proposal) = proposal else {
        let revision = loaded.revision.id.to_string();
        return Ok(Json(WriteResult {
            version: revision.clone(),
            revision,
            semantic_hash: loaded.revision.semantic_hash.to_string(),
            events: Vec::new(),
        })
        .into_response());
    };
    let write = state.write(&loaded, &proposal, false, None, created_at, "run_scenarios")?;
    Ok(Json(write).into_response())
}

/// `get_scenario_run`: the validated current run of the path Scenario (404 when none).
async fn get_scenario_run(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let node = scenario_node(&loaded.graph, &id)?;
    let view = run_view(&state, &loaded.graph, &node.id)?
        .ok_or_else(|| ApiError::not_found(format!("scenario {id} has no run")))?;
    Ok(read_json(&loaded, view))
}
