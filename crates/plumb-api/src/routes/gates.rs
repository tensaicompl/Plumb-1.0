//! Gate and readiness routes (plan A0.2): current gate reports and the uncalculated pilot
//! readiness.

use axum::extract::{Path, RawQuery, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use plumb_psg::NodeType;
use plumb_validation::GateResult;

use crate::error::ApiError;
use crate::openapi::{GateName, GateStatus, Readiness, ReadinessGroup, ReadinessSubScores};
use crate::state::{parse_wire, read_json, wire, ApiState, QueryParams};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/gates", get(get_gates))
        .route(
            "/api/requirements/{id}/readiness",
            get(get_requirement_readiness),
        )
        .route("/api/readiness", get(get_readiness))
}

/// `get_gates`: the current registered reports in I0, F1, F2, F3, F4 order; a missing gate is
/// unevaluated and omitted, never reported as failed.
async fn get_gates(State(state): State<ApiState>) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let mut gates = Vec::new();
    for report in state.current_gate_reports(&loaded)? {
        let Ok(gate) = parse_wire::<GateName>(&wire(&report.gate)?, "gate") else {
            continue; // Later gates are outside the NOW API.
        };
        let mut findings: Vec<String> = report.findings.iter().map(|f| f.id.to_string()).collect();
        findings.sort();
        gates.push(GateStatus {
            gate,
            passed: report.result == GateResult::Pass,
            findings,
        });
    }
    Ok(read_json(&loaded, gates))
}

/// Readiness is not calculated by the pilot runtime: every value is null.
fn uncalculated() -> Readiness {
    Readiness {
        overall: None,
        sub: ReadinessSubScores {
            clarity: None,
            completeness: None,
            consistency: None,
            verifiability: None,
            confirmation: None,
        },
        contributing: Vec::new(),
    }
}

/// `get_requirement_readiness`.
async fn get_requirement_readiness(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let exists = id
        .parse()
        .ok()
        .and_then(|id| loaded.graph.node(&id))
        .is_some_and(|n| n.payload.node_type() == NodeType::Requirement);
    if !exists {
        return Err(ApiError::not_found(format!("requirement {id} not found")));
    }
    Ok(read_json(&loaded, uncalculated()))
}

/// `get_readiness`.
async fn get_readiness(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &["group"], false)?;
    if let Some(group) = query.get("group") {
        parse_wire::<ReadinessGroup>(group, "group")?;
    }
    let loaded = state.load_head()?;
    Ok(read_json(&loaded, uncalculated()))
}
