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
    GateEvaluationState, GateName, GateRuleApplicability, GateRuleApplicabilityState,
    GateRuleError, GateRuleResult, GateRuleState, GateStateReason, GateStatus, GateSummaryView,
    GateWaiver, Readiness, ReadinessGroup, ReadinessSubScores,
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

/// `get_gates`: every NOW gate in I0, F1, F2, F3, F4 order (H0.11). A gate without a current
/// report is evaluated on demand (and persisted, as `ready` does). A gate that cannot be
/// evaluated is reported `stale` when only reports of earlier heads exist and `not_evaluated`
/// otherwise, with the typed reason; it is never reported as failed and never omitted. A
/// server-side failure (corrupt persisted state) fails the read.
async fn get_gates(State(state): State<ApiState>) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let mut gates = Vec::new();
    for gate in [GateId::I0, GateId::F1, GateId::F2, GateId::F3, GateId::F4] {
        let name = parse_wire::<GateName>(&wire(&gate)?, "gate")?;
        match state.current_or_evaluated_report(&loaded, gate) {
            Ok(report) => gates.push(current_view(name, &report)?),
            Err(error) if error.body().status.is_server_error() => return Err(error),
            Err(error) => {
                let state = if state.has_any_gate_report(gate)? {
                    GateEvaluationState::Stale
                } else {
                    GateEvaluationState::NotEvaluated
                };
                gates.push(GateStatus {
                    gate: name,
                    state,
                    reason: Some(GateStateReason {
                        code: error.body().code.to_owned(),
                        message: error.body().message.clone(),
                    }),
                    passed: false,
                    findings: Vec::new(),
                    rules: Vec::new(),
                    waivers: Vec::new(),
                    summary: GateSummaryView {
                        blocker_failed: 0,
                        blocker_waived: 0,
                        warnings: 0,
                    },
                });
            }
        }
    }
    Ok(read_json(&loaded, gates))
}

/// The projection of a report current for the head.
fn current_view(
    gate: GateName,
    report: &plumb_validation::GateReport,
) -> Result<GateStatus, ApiError> {
    let mut findings: Vec<String> = report.findings.iter().map(|f| f.id.to_string()).collect();
    findings.sort();
    let rules = report
        .rules
        .iter()
        .map(rule_view)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GateStatus {
        gate,
        state: GateEvaluationState::Current,
        reason: None,
        passed: report.result == GateResult::Pass,
        findings,
        rules,
        waivers: report.waivers.iter().map(waiver_view).collect(),
        summary: GateSummaryView {
            blocker_failed: report.summary.blocker_failed,
            blocker_waived: report.summary.blocker_waived,
            warnings: report.summary.warnings,
        },
    })
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
