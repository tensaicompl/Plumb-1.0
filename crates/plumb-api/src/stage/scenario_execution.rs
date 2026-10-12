//! The A0.3 scenario execution (Hotfix 065) as one shared pure application function (A0.4,
//! Hotfix 067): `run_scenarios` and the `run` recipe call exactly this, so the execution
//! algorithm, trace format and update semantics are identical on both paths.

use plumb_core::Hash;
use plumb_core::Id;
use plumb_functional::{
    decode_decision_table, CalendarProvider, PredicateProvider, ScenarioDecisionTableInput,
    ScenarioRunUpdate, SCENARIO_CONFIRMED,
};
use plumb_psg::{
    ElementStatus, ExtensionKey, Graph, Node, NodePayload, NodeType, ScenarioRun as PsgScenarioRun,
};
use plumb_sim::{
    build_execution_model, compare_run, run, scenario_run_id, ExecutionCalculationScopeOverride,
    ExecutionExpressionScopeInput, ExecutionInputs, ExecutionRuntime, RuntimeSemanticValue,
    SCENARIO_TRACE_LINK_EXTENSION,
};
use serde_json::json;

use crate::error::ApiError;

/// The error name of a typed error: its debug output up to the first non-identifier character.
pub(crate) fn error_name<T: std::fmt::Debug>(error: &T) -> String {
    format!("{error:?}")
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

pub(crate) fn run_collision(run_ref: &Id) -> ApiError {
    ApiError::unprocessable(
        "E_SCENARIO_RUN_COLLISION",
        "the stable scenario run ID holds another element",
    )
    .with_details(json!({ "scenario_run": run_ref }))
}

pub(crate) fn link_key() -> Result<ExtensionKey, ApiError> {
    SCENARIO_TRACE_LINK_EXTENSION
        .parse()
        .map_err(|_| ApiError::internal("scenario_trace_link"))
}

/// A current ScenarioRun node with its payload.
pub(crate) type CurrentRun<'g> = Option<(&'g Node, &'g PsgScenarioRun)>;

/// The current ScenarioRun node of `scenario` with its payload, if any; any other element at the
/// stable ID is a collision.
pub(crate) fn current_run<'g>(
    graph: &'g Graph,
    scenario: &Id,
) -> Result<(Id, CurrentRun<'g>), ApiError> {
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

/// Whether a Scenario is runnable: Accepted and Confirmed, matching simulator admission.
pub(crate) fn runnable(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
        && matches!(&node.payload, NodePayload::Scenario(s)
            if s.confirmation_status.as_deref() == Some(SCENARIO_CONFIRMED))
}

/// The injected runtime material of one execution.
pub(crate) struct ScenarioExecution<'a> {
    pub(crate) actor_ref: &'a Id,
    pub(crate) calendars: &'a dyn CalendarProvider,
    pub(crate) predicates: &'a dyn PredicateProvider,
    pub(crate) expression_scopes: &'a [ExecutionExpressionScopeInput],
    pub(crate) calculation_scope_overrides: &'a [ExecutionCalculationScopeOverride],
    pub(crate) runtime_semantic_values: &'a [RuntimeSemanticValue],
}

/// One executed Scenario: its canonical ScenarioTrace artifact and its run update.
pub(crate) struct ExecutedScenario {
    pub(crate) trace_hash: Hash,
    pub(crate) trace_bytes: Vec<u8>,
    pub(crate) update: ScenarioRunUpdate,
}

/// Runs `selected` (already admitted runnable Scenarios) over `graph`: strict decoding of every
/// Accepted decision table, the execution model, one run and comparison per Scenario and its
/// stable run update linking the trace artifact. Nothing is persisted here.
pub(crate) fn execute_scenarios(
    graph: &Graph,
    selected: &[Id],
    execution: &ScenarioExecution<'_>,
) -> Result<Vec<ExecutedScenario>, ApiError> {
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
            calculation_scope_overrides: execution.calculation_scope_overrides.to_vec(),
            expression_scopes: execution.expression_scopes.to_vec(),
        },
    )
    .map_err(|e| simulation(&e, e.to_string()))?;
    let execution_runtime = ExecutionRuntime {
        actor_ref: Some(execution.actor_ref.clone()),
        semantic_values: execution.runtime_semantic_values.to_vec(),
        calendars: execution.calendars,
        predicates: execution.predicates,
    };
    let mut executed = Vec::with_capacity(selected.len());
    for scenario in selected {
        let run_result =
            run(&model, scenario, &execution_runtime).map_err(|e| simulation(&e, e.to_string()))?;
        let comparison = compare_run(&model, &run_result, &execution_runtime)
            .map_err(|e| simulation(&e, e.to_string()))?;
        let artifact = comparison.trace_artifact;
        let (run_ref, _) = current_run(graph, scenario)?;
        executed.push(ExecutedScenario {
            trace_hash: artifact.hash.clone(),
            trace_bytes: artifact.bytes,
            update: ScenarioRunUpdate {
                run_ref,
                scenario_ref: scenario.clone(),
                payload: comparison.scenario_run,
                link_key: link_key()?,
                link: artifact.hash,
            },
        });
    }
    Ok(executed)
}
