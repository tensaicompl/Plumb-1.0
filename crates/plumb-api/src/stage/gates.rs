//! Gate evaluation and durable GateReport persistence of A0.4 (Hotfix 067): the I0-F4
//! validation contexts built only from configured or mechanically derived material, the `ready`
//! chain and the store-v3 report rows.

use plumb_artifacts::{ArtifactKind, ArtifactStore};
use plumb_core::{to_canonical_json, FixedClock, GateId, Hash, Timestamp};
use plumb_functional::{analyze_assumption_expiry, validate_scenario};
use plumb_import::{
    build_evidence_manifest, evidence_manifest_artifact, SourceArtifactLinks,
    SOURCE_ARTIFACTS_EXTENSION,
};
use plumb_psg::{ElementStatus, NodePayload, NodeType};
use plumb_store::{GateReportRecord, LoadedRevision, NewGateReport};
use plumb_validation::evaluator::{
    F3DecisionArtifactInput, F3ValidationInputs, S3_RESOLUTION_KINDS,
};
use plumb_validation::{
    rules, scenario_specification_hash, EvaluatorRegistry, F4ScenarioRunInput,
    F4ScenarioTypeCheckInput, F4ScenarioTypeStatus, F4ValidationInputs, GateReport, GateResult,
    ValidationArtifactInput, ValidationContext, ValidationPolicy, ValidationRegistry, WAIVER_KIND,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::stage::scenario_execution::{current_run, error_name, link_key};
use crate::state::{A04Runtime, ApiState};

/// 409 `E_GATE_INPUTS_MISSING`: a gate needs configured supplemental inputs that are absent.
pub(crate) fn inputs_missing(gate: GateId) -> ApiError {
    ApiError::conflict(
        "E_GATE_INPUTS_MISSING",
        format!("the {gate} gate needs configured supplemental inputs"),
    )
    .with_details(json!({ "gate": gate.to_string() }))
}

/// 422 `E_STAGE_EVALUATION_FAILED` for a gate that cannot be evaluated.
fn evaluation_failed<E: std::fmt::Debug + std::fmt::Display>(gate: GateId, error: E) -> ApiError {
    ApiError::unprocessable(
        "E_STAGE_EVALUATION_FAILED",
        format!("the {gate} gate cannot be evaluated: {error}"),
    )
    .with_details(json!({ "gate": gate.to_string(), "error": error_name(&error) }))
}

/// Decodes a stored report: strict serde, canonical bytes, valid report and matching hash.
pub(crate) fn decode_report(record: &GateReportRecord) -> Result<GateReport, ApiError> {
    let corrupt = || ApiError::workflow_corrupt(&format!("gate report {}", record.revision_id));
    let report: GateReport = serde_json::from_str(&record.report_json).map_err(|_| corrupt())?;
    let canonical = to_canonical_json(&report).map_err(|_| corrupt())?;
    if canonical != record.report_json.as_bytes()
        || report.validate().is_err()
        || report.content_hash().map_err(|_| corrupt())? != record.report_hash
        || report.gate.to_string() != record.gate
    {
        return Err(corrupt());
    }
    Ok(report)
}

/// The evaluator registry with every NOW gate's evaluators.
fn registry(state: &ApiState) -> Result<EvaluatorRegistry, ApiError> {
    let metadata = ValidationRegistry::new(state.profile().clone())
        .map_err(|_| ApiError::internal("validation_registry"))?;
    let mut registry = EvaluatorRegistry::new(metadata);
    for register in [
        rules::register_i0_evaluators,
        rules::register_f1_evaluators,
        rules::register_f2_evaluators,
        rules::register_f3_evaluators,
        rules::register_f4_evaluators,
    ] {
        register(&mut registry).map_err(|_| ApiError::internal("validation_registry"))?;
    }
    Ok(registry)
}

/// The content hash of supplemental inputs.
fn input_hash<T: Serialize>(value: &T) -> Result<Hash, ApiError> {
    let bytes = to_canonical_json(value).map_err(|_| ApiError::internal("gate_inputs"))?;
    Ok(Hash::content_sha256(&bytes))
}

/// The I0 evidence material: the graph's EvidenceManifest artifact plus every stored original
/// and extracted artifact of each baseline SourceArtifact (a missing one stays absent).
fn i0_evidence(
    state: &ApiState,
    loaded: &LoadedRevision,
) -> Result<Vec<ValidationArtifactInput>, ApiError> {
    let graph = &loaded.graph;
    let mut out = Vec::new();
    if let Ok(manifest) = build_evidence_manifest(graph) {
        if let Ok(artifact) = evidence_manifest_artifact(&manifest, loaded.revision.created_at) {
            out.push(ValidationArtifactInput {
                hash: artifact.hash,
                kind: artifact.kind,
                media_type: artifact.media_type,
                bytes: artifact.bytes,
            });
        }
    }
    let key = SOURCE_ARTIFACTS_EXTENSION
        .parse()
        .map_err(|_| ApiError::internal("source_artifacts_extension"))?;
    let mut refs = Vec::new();
    for id in graph.node_ids_by_type(NodeType::SourceArtifact) {
        let Some(node) = graph.node(id) else { continue };
        let NodePayload::SourceArtifact(source) = &node.payload else {
            continue;
        };
        if !plumb_psg::is_baseline(node.status) {
            continue;
        }
        refs.push(source.content_hash.clone());
        if let Some(links) = node
            .extensions
            .get(&key)
            .and_then(|v| serde_json::from_value::<SourceArtifactLinks>(v.clone()).ok())
        {
            refs.push(links.extracted_ref);
        }
    }
    refs.sort();
    refs.dedup();
    for hash in refs {
        let stored = state.with_artifact_store(|store| store.get(&hash))?;
        if let Ok(artifact) = stored {
            if matches!(
                artifact.kind,
                ArtifactKind::SourceOriginal | ArtifactKind::SourceExtracted
            ) && !out.iter().any(|a| a.hash == artifact.hash)
            {
                out.push(ValidationArtifactInput {
                    hash: artifact.hash,
                    kind: artifact.kind,
                    media_type: artifact.media_type,
                    bytes: artifact.bytes,
                });
            }
        }
    }
    Ok(out)
}

/// F3 decision artifacts: exactly the Accepted current S3 and waiver decisions.
fn f3_decision_artifacts(
    state: &ApiState,
    loaded: &LoadedRevision,
) -> Result<Vec<F3DecisionArtifactInput>, ApiError> {
    let graph = &loaded.graph;
    let mut out = Vec::new();
    for id in graph.node_ids_by_type(NodeType::ResolutionDecision) {
        let Some(node) = graph.node(id) else { continue };
        let NodePayload::ResolutionDecision(decision) = &node.payload else {
            continue;
        };
        if node.status != ElementStatus::Accepted {
            continue;
        }
        let kind = decision.answer.get("kind").and_then(Value::as_str);
        if !kind.is_some_and(|k| S3_RESOLUTION_KINDS.contains(&k) || k == WAIVER_KIND) {
            continue;
        }
        let artifact = state
            .with_artifact_store(|store| store.get(&decision.patch_ref))?
            .map_err(|_| {
                ApiError::unprocessable(
                    "E_STAGE_EVALUATION_FAILED",
                    "a decision artifact is missing",
                )
                .with_details(json!({ "gate": "F3", "decision": node.id }))
            })?;
        out.push(F3DecisionArtifactInput {
            decision_ref: node.id.clone(),
            hash: decision.patch_ref.clone(),
            bytes: artifact.bytes,
        });
    }
    Ok(out)
}

/// F4 inputs: configured applicability and visible assumptions, one type check per in-scope
/// Scenario and the current runs of in-scope Scenarios with their linked traces.
fn f4_inputs(
    state: &ApiState,
    loaded: &LoadedRevision,
    a04: &A04Runtime,
) -> Result<F4ValidationInputs, ApiError> {
    let config = a04
        .gates
        .f4
        .as_ref()
        .ok_or_else(|| inputs_missing(GateId::F4))?;
    let graph = &loaded.graph;
    let mut checks = Vec::new();
    let mut runs = Vec::new();
    for scenario_ref in &config.applicability.in_scope_scenario_refs {
        let Some(NodePayload::Scenario(scenario)) = graph.node(scenario_ref).map(|n| &n.payload)
        else {
            continue; // validation rejects the stale applicability itself
        };
        let specification_hash = scenario_specification_hash(scenario_ref, scenario)
            .map_err(|e| evaluation_failed(GateId::F4, e))?;
        let status = match validate_scenario(graph, scenario) {
            Ok(_) => F4ScenarioTypeStatus::Valid,
            Err(error) => F4ScenarioTypeStatus::Invalid {
                issues: vec![error.to_string()],
            },
        };
        checks.push(F4ScenarioTypeCheckInput {
            scenario_ref: scenario_ref.clone(),
            specification_hash,
            status,
        });
        let (_, current) = current_run(graph, scenario_ref)?;
        if let Some((node, run)) = current {
            let link = node
                .extensions
                .get(&link_key()?)
                .and_then(Value::as_str)
                .and_then(|t| t.parse::<Hash>().ok())
                .ok_or_else(|| {
                    evaluation_failed(GateId::F4, "a scenario run trace link is invalid")
                })?;
            let artifact = state
                .with_artifact_store(|store| store.get(&link))?
                .map_err(|e| evaluation_failed(GateId::F4, e))?;
            runs.push(F4ScenarioRunInput {
                run: run.clone(),
                trace_artifact_hash: link,
                trace_artifact_bytes: artifact.bytes,
            });
        }
    }
    Ok(F4ValidationInputs::new(
        config.applicability.clone(),
        checks,
        runs,
        config.visible_assumption_refs.clone(),
    ))
}

/// One evaluated report with its provenance.
pub(crate) struct Evaluated {
    pub(crate) report: GateReport,
    pub(crate) supplemental_input_hash: Option<Hash>,
    pub(crate) evaluation_at: Option<Timestamp>,
}

/// Evaluates `gate` over `loaded` with the configured or derived material. F3 needs the
/// captured `evaluation_at`.
pub(crate) fn evaluate_gate(
    state: &ApiState,
    loaded: &LoadedRevision,
    gate: GateId,
    policy: &ValidationPolicy,
    evaluation_at: Option<Timestamp>,
) -> Result<Evaluated, ApiError> {
    let a04 = state.stage_runtime("stage_gates")?;
    let registry = registry(state)?;
    let graph = &loaded.graph;
    let evidence = if gate == GateId::I0 {
        i0_evidence(state, loaded)?
    } else {
        Vec::new()
    };
    let ctx = ValidationContext::new(
        graph,
        registry.metadata(),
        policy.clone(),
        loaded.revision.evidence_hash.clone(),
        evidence,
        Vec::new(),
    )
    .map_err(|e| evaluation_failed(gate, e))?;
    let (ctx, supplemental) = match gate {
        GateId::I0 => (ctx, None),
        GateId::F1 => {
            let inputs = a04.gates.f1.clone().ok_or_else(|| inputs_missing(gate))?;
            let hash = input_hash(&inputs)?;
            (
                ctx.with_f1_inputs(graph, inputs)
                    .map_err(|e| evaluation_failed(gate, e))?,
                Some(hash),
            )
        }
        GateId::F2 => {
            let inputs = a04.gates.f2.clone().ok_or_else(|| inputs_missing(gate))?;
            let hash = input_hash(&inputs)?;
            (
                ctx.with_f2_inputs(graph, inputs)
                    .map_err(|e| evaluation_failed(gate, e))?,
                Some(hash),
            )
        }
        GateId::F3 => {
            let at = evaluation_at.ok_or_else(|| inputs_missing(gate))?;
            let findings = analyze_assumption_expiry(graph, &FixedClock::new(at))
                .map_err(|e| evaluation_failed(gate, e))?
                .findings;
            let inputs = F3ValidationInputs::new(f3_decision_artifacts(state, loaded)?, findings);
            let hash = input_hash(&inputs)?;
            (
                ctx.with_f3_inputs(graph, inputs)
                    .map_err(|e| evaluation_failed(gate, e))?,
                Some(hash),
            )
        }
        GateId::F4 => {
            let inputs = f4_inputs(state, loaded, a04)?;
            let hash = input_hash(&inputs)?;
            (
                ctx.with_f4_inputs(graph, inputs)
                    .map_err(|e| evaluation_failed(gate, e))?,
                Some(hash),
            )
        }
        other => return Err(evaluation_failed(other, "not a NOW gate")),
    };
    let report = registry
        .evaluate_gate(gate, graph, &ctx)
        .map_err(|e| evaluation_failed(gate, e))?;
    Ok(Evaluated {
        report,
        supplemental_input_hash: supplemental,
        evaluation_at: (gate == GateId::F3).then_some(evaluation_at).flatten(),
    })
}

/// Persists an evaluated report for `loaded`'s revision (upsert per revision and gate).
pub(crate) fn persist_report(
    state: &ApiState,
    loaded: &LoadedRevision,
    evaluated: &Evaluated,
    recipe_job_id: Option<&Hash>,
    created_at: Timestamp,
) -> Result<GateReportRecord, ApiError> {
    let report = &evaluated.report;
    let record = NewGateReport {
        revision_id: loaded.revision.id.clone(),
        gate: report.gate.to_string(),
        report_hash: report
            .content_hash()
            .map_err(|_| ApiError::internal("gate_report"))?,
        report_json: serde_json::to_value(report).map_err(|_| ApiError::internal("gate_report"))?,
        supplemental_input_hash: evaluated.supplemental_input_hash.clone(),
        evaluation_at: evaluated.evaluation_at,
        recipe_job_id: recipe_job_id.cloned(),
        created_at,
    };
    state
        .with_stage_store(|store| store.upsert_gate_report(&record))?
        .map_err(crate::stage::store_error)
}

/// The result of one gate of `ready`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadyGate {
    pub gate: String,
    pub result: String,
    pub report_hash: Hash,
}

/// The `ready` outcome: one loaded head, the captured `evaluation_at` and every gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadyOutcome {
    pub revision: String,
    pub semantic_hash: String,
    pub evaluation_at: Timestamp,
    pub gates: Vec<ReadyGate>,
}

impl ReadyOutcome {
    /// Whether every gate passed.
    pub fn passed(&self) -> bool {
        self.gates.iter().all(|g| g.result == "pass")
    }
}

impl ApiState {
    /// `ready`: evaluates I0, F1, F2, F3 and F4 on one loaded head and persists every report.
    /// F1, F2 and F4 configuration is required before anything is evaluated; `evaluation_at`
    /// is read once from the injected clock for F3.
    pub fn ready(&self) -> Result<ReadyOutcome, ApiError> {
        let a04 = self.stage_runtime("ready")?;
        for (gate, present) in [
            (GateId::F1, a04.gates.f1.is_some()),
            (GateId::F2, a04.gates.f2.is_some()),
            (GateId::F4, a04.gates.f4.is_some()),
        ] {
            if !present {
                return Err(inputs_missing(gate));
            }
        }
        let loaded = self.load_head()?;
        let evaluation_at = self.now();
        let mut gates = Vec::new();
        for gate in [GateId::I0, GateId::F1, GateId::F2, GateId::F3, GateId::F4] {
            let evaluated = evaluate_gate(
                self,
                &loaded,
                gate,
                &a04.validation_policy,
                Some(evaluation_at),
            )?;
            let record = persist_report(self, &loaded, &evaluated, None, evaluation_at)?;
            gates.push(ReadyGate {
                gate: gate.to_string(),
                result: result_wire(evaluated.report.result),
                report_hash: record.report_hash,
            });
        }
        Ok(ReadyOutcome {
            revision: loaded.revision.id.to_string(),
            semantic_hash: loaded.revision.semantic_hash.to_string(),
            evaluation_at,
            gates,
        })
    }
}

/// The wire string of a gate result.
pub(crate) fn result_wire(result: GateResult) -> String {
    match result {
        GateResult::Pass => "pass".to_owned(),
        GateResult::Fail => "fail".to_owned(),
    }
}
