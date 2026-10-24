//! Gate and readiness routes (plan A0.2): current gate reports and the uncalculated pilot
//! readiness.

use axum::extract::{Path, RawQuery, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use plumb_core::{GateId, Id};
use plumb_psg::NodeType;
use plumb_validation::{
    Applicability, AppliedWaiver, EvaluatorFailure, GateResult, RuleResult, RuleResultState,
};

use crate::error::ApiError;
use crate::openapi::{
    GateName, GateRuleApplicability, GateRuleApplicabilityState, GateRuleError, GateRuleResult,
    GateRuleState, GateStatus, GateSummaryView, GateWaiver, Readiness, ReadinessGroup,
    ReadinessSubScores,
};
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

/// `get_gates`: the current reports in I0, F1, F2, F3, F4 order. A gate without a current
/// report is evaluated on demand (and persisted, as `ready` does); a gate that cannot be
/// evaluated (e.g. its analysis never ran) is unevaluated and omitted, never reported as failed.
async fn get_gates(State(state): State<ApiState>) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let mut gates = Vec::new();
    let mut reports = Vec::new();
    for gate in [GateId::I0, GateId::F1, GateId::F2, GateId::F3, GateId::F4] {
        if let Ok(report) = state.current_or_evaluated_report(&loaded, gate) {
            reports.push(report);
        }
    }
    for report in reports {
        let Ok(gate) = parse_wire::<GateName>(&wire(&report.gate)?, "gate") else {
            continue; // Later gates are outside the NOW API.
        };
        let mut findings: Vec<String> = report.findings.iter().map(|f| f.id.to_string()).collect();
        findings.sort();
        let rules = report
            .rules
            .iter()
            .map(rule_view)
            .collect::<Result<Vec<_>, _>>()?;
        gates.push(GateStatus {
            gate,
            passed: report.result == GateResult::Pass,
            findings,
            rules,
            waivers: report.waivers.iter().map(waiver_view).collect(),
            summary: GateSummaryView {
                blocker_failed: report.summary.blocker_failed,
                blocker_waived: report.summary.blocker_waived,
                warnings: report.summary.warnings,
            },
        });
    }
    Ok(read_json(&loaded, gates))
}

/// The API state of a validation rule result; exhaustive, so a new state must be considered here.
fn rule_state(state: RuleResultState) -> GateRuleState {
    match state {
        RuleResultState::Pass => GateRuleState::Pass,
        RuleResultState::Fail => GateRuleState::Fail,
        RuleResultState::Warn => GateRuleState::Warn,
        RuleResultState::NotApplicable => GateRuleState::NotApplicable,
        RuleResultState::Waived => GateRuleState::Waived,
        RuleResultState::Error => GateRuleState::Error,
    }
}

fn ids(ids: &[Id]) -> Vec<String> {
    ids.iter().map(Id::to_string).collect()
}

/// The direct projection of one already-validated report rule result (Hotfix 082).
fn rule_view(rule: &RuleResult) -> Result<GateRuleResult, ApiError> {
    let applicability = match &rule.applicability {
        Applicability::Applicable => GateRuleApplicability {
            state: GateRuleApplicabilityState::Applicable,
            reason: None,
        },
        Applicability::NotApplicable { reason } => GateRuleApplicability {
            state: GateRuleApplicabilityState::NotApplicable,
            reason: Some(reason.clone()),
        },
    };
    Ok(GateRuleResult {
        rule_id: rule.rule_id.clone(),
        state: rule_state(rule.state),
        declared_severity: wire(&rule.declared_severity)?,
        effective_severity: wire(&rule.effective_severity)?,
        applicability,
        targets: ids(&rule.targets),
        evidence: rule.evidence.clone(),
        semantic_condition_key: rule.semantic_condition_key.clone(),
        finding_ref: rule.finding_ref.as_ref().map(Id::to_string),
        waiver_ref: rule.waiver_ref.as_ref().map(Id::to_string),
        error: rule.error.as_ref().map(error_view),
    })
}

fn error_view(failure: &EvaluatorFailure) -> GateRuleError {
    GateRuleError {
        code: failure.code.clone(),
        message: failure.message.clone(),
        targets: ids(&failure.targets),
        evidence: failure.evidence.clone(),
    }
}

/// The direct projection of one applied governed waiver of the report.
fn waiver_view(waiver: &AppliedWaiver) -> GateWaiver {
    GateWaiver {
        rule_id: waiver.rule_id.clone(),
        finding_ref: waiver.finding_ref.to_string(),
        finding_key: waiver.finding_key.to_string(),
        decision_ref: waiver.decision_ref.to_string(),
    }
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
