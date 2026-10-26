//! The per-phase compiler adapters of the A0.4 recipe matrix (Hotfix 067). PLAN and EVALUATE are
//! pure over the graph and the phase's typed inputs (which are exactly the inputs bound into the
//! child `CompileContext.config`); they call the existing domain builders and analysers and
//! implement no algorithm of their own.

use plumb_compiler::{
    ArtifactSet, CompileContext, CompilerError, CompilerStage, InferenceAbsencePolicy,
    PlannedInferenceRequest, Scope, StageEvaluation, StagePlan,
};
use plumb_core::{to_canonical_json, Hash, Id, StageId, Timestamp};
use plumb_functional::calculation::{CalculationAudit, CalculationInference};
use plumb_functional::duplicates::DuplicateResolutionProposal;
use plumb_functional::{
    analyze_authorization, analyze_calculations, analyze_data_classification, analyze_domain,
    analyze_lifecycle, analyze_operations, analyze_processes, analyze_requirement_duplicates,
    analyze_scenarios, analyze_vocabulary, build_calculation_request, build_data_class_request,
    build_directory_draft_request, build_domain_request, build_ears_request,
    build_lifecycle_request, build_operation_request, build_process_request,
    build_requirement_classification_request, build_scenario_request, build_vocabulary_request,
    compile_requirement_candidates, compile_verification_obligations, derive_scenarios,
    draft_directory, evaluate_ears_normalization, produce_directory, propose_scenario_run_updates,
    AuthorizationAudit, DataClassInference, DirectoryAudit, DirectoryCalendarBinding,
    DirectoryDisposition, DomainAudit, DomainInference, EarsInference, LifecycleAudit,
    LifecycleInference, OperationAudit, OperationInference, ProcessAudit, ProcessInference,
    RequirementClassificationInference, RequirementCompilationAudit, ScenarioAudit, ScenarioError,
    ScenarioInference, ScenarioRequest, VerificationAudit, VocabularyAudit, VocabularyInference,
};
use plumb_import::{
    build_segmentation_request, evaluate_segmentation, FallbackReason, SegmentCandidate,
    SegmentClassification, SegmentationAudit, SegmentationMode, SegmentationRequest,
};
use plumb_inference::{InferenceArtifact, InferenceRequest, ProviderPolicy};
use plumb_patch::Proposal;
use plumb_psg::{Graph, Node};
use plumb_validation::decision_table_analysis::analyze_decision_table;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::selector::Selections;
use crate::stage::acquire::{derivation_ref, inference_of};
use crate::stage::recipe::Phase;
use crate::stage::scenario_execution::{
    error_name, execute_scenarios, NamedFailure, ScenarioExecution,
};
use crate::state::{A03Runtime, A04Runtime};

/// The canonical segmentation intermediate persisted between the two extract phases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentationIntermediate {
    pub version: u32,
    pub candidates: Vec<SegmentCandidate>,
    pub mode: SegmentationMode,
    pub fallback_reason: Option<FallbackReason>,
    pub fallback_derivation: Option<Node>,
}

impl SegmentationIntermediate {
    /// Whether any candidate is a requirement candidate (else classification is skipped).
    pub fn has_requirement_candidate(&self) -> bool {
        self.candidates
            .iter()
            .any(|c| c.classification == SegmentClassification::RequirementCandidate)
    }
}

/// Serializes analyser material for the phase report (never fails: unserializable is null).
fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// What one phase evaluation produced besides the compiler evaluation.
pub(crate) struct PhaseOutput {
    pub(crate) evaluation: StageEvaluation,
    /// Canonical bytes of the phase's typed intermediate, if any.
    pub(crate) intermediate: Option<Vec<u8>>,
    /// ScenarioTrace artifacts to persist before any commit.
    pub(crate) traces: Vec<(Hash, Vec<u8>)>,
}

/// One phase adapter: the recipe phase, its scope and targets and its typed inputs.
pub(crate) struct PhaseStage<'a> {
    pub(crate) phase: Phase,
    pub(crate) scope: Scope,
    pub(crate) targets: Vec<Id>,
    pub(crate) audit_actor: Id,
    pub(crate) audit_at: Timestamp,
    pub(crate) a03: &'a A03Runtime,
    pub(crate) a04: &'a A04Runtime,
    pub(crate) segmentation: Option<(Hash, SegmentationIntermediate)>,
}

/// A typed planning error: its debug form names it, its display form describes it.
trait PlanError: std::fmt::Debug + std::fmt::Display {}

impl<T: std::fmt::Debug + std::fmt::Display> PlanError for T {}

fn plan_failed(phase: Phase, error: impl std::fmt::Debug + std::fmt::Display) -> ApiError {
    ApiError::unprocessable(
        "E_STAGE_PLAN_FAILED",
        format!("phase {} cannot be planned: {error}", phase.as_str()),
    )
    .with_details(json!({ "phase": phase.as_str(), "error": error_name(&error) }))
}

/// The plan failure of inference material that could not be built (its builder reports text).
fn material_failed(phase: Phase, message: String) -> ApiError {
    plan_failed(
        phase,
        NamedFailure::new("InferenceRequestMaterialInvalid", message),
    )
}

fn evaluation_failed(phase: Phase, error: impl std::fmt::Debug + std::fmt::Display) -> ApiError {
    ApiError::unprocessable(
        "E_STAGE_EVALUATION_FAILED",
        format!("phase {} cannot be evaluated: {error}", phase.as_str()),
    )
    .with_details(json!({ "phase": phase.as_str(), "error": error_name(&error) }))
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, ApiError> {
    serde_json::to_value(value).map_err(|e| ApiError::internal_cause("phase_inputs", &e))
}

/// The inference requests a scenarios phase plans, in skeleton order.
fn scenario_requests(
    graph: &Graph,
    inputs: &plumb_functional::ScenarioDerivationInputs,
    policy: &ProviderPolicy,
) -> Result<Vec<ScenarioRequest>, String> {
    let mut requests = Vec::new();
    for skeleton in analyze_scenarios(graph, inputs).map_err(|e| e.to_string())? {
        match build_scenario_request(graph, inputs, &skeleton.scenario_ref, policy.clone()) {
            Ok(request) => requests.push(request),
            Err(ScenarioError::NoInferenceSlots { .. }) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(requests)
}

impl<'a> PhaseStage<'a> {
    fn policy(&self) -> ProviderPolicy {
        self.a03.provider_policy.clone()
    }

    /// The typed inputs bound into the child configuration (Hotfix 067), resolved against
    /// the phase's input `graph`, with the configured selector forms under `selections`.
    pub(crate) fn inputs(&self, graph: &Graph) -> Result<Value, ApiError> {
        let a04 = self.a04;
        let a03 = self.a03;
        let mut selections = Selections::default();
        let inputs = match self.phase {
            Phase::Classification => {
                let (hash, segmentation) = self
                    .segmentation
                    .as_ref()
                    .ok_or_else(|| ApiError::internal("segmentation_intermediate"))?;
                json!({ "segmentation_ref": hash, "segmentation": to_value(segmentation)? })
            }
            Phase::Consolidate => json!({
                "vocabulary_policy": to_value(&a04.vocabulary_policy)?,
                "duplicate_policy": to_value(&a04.duplicate_policy)?,
            }),
            Phase::DirectoryDraft => json!({ "calendars": to_value(&a04.calendars)? }),
            Phase::Calc => {
                selections.record("calculation_scope", a04.calculation_scope.as_ref());
                json!({ "calculation_scope": to_value(&a04.calculation_scope_for(graph)?)? })
            }
            Phase::Flows => {
                selections.record("operation_scope", a04.operation_scope.as_ref());
                selections.record("process_scope", a04.process_scope.as_ref());
                selections.record("authorization_draft", a04.authorization_draft.as_ref());
                json!({
                    "operation_scope": to_value(&a04.operation_scope_for(graph)?)?,
                    "process_scope": to_value(&a04.process_scope_for(graph)?)?,
                    "authorization_draft": to_value(&a04.authorization_draft_for(graph)?)?,
                })
            }
            Phase::Scenarios => {
                selections.record("scenario_derivation", a04.scenario_derivation.as_ref());
                json!({ "scenario_derivation": to_value(&a04.scenario_derivation_for(graph)?)? })
            }
            Phase::Execute => {
                selections.record_entries("expression_scopes", &a03.expression_scopes);
                selections.record_entries(
                    "calculation_scope_overrides",
                    &a03.calculation_scope_overrides,
                );
                selections.record_entries("runtime_semantic_values", &a03.runtime_semantic_values);
                json!({
                    "calendars": to_value(&a04.calendars)?,
                    "direct_managers": to_value(&a04.direct_managers)?,
                    "expression_scopes": to_value(&a03.expression_scopes_for(graph)?)?,
                    "calculation_scope_overrides":
                        to_value(&a03.calculation_scope_overrides_for(graph)?)?,
                    "runtime_semantic_values":
                        to_value(&a03.runtime_semantic_values_for(graph)?)?,
                })
            }
            Phase::Verification => {
                selections.record(
                    "verification_applicability",
                    a04.verification_applicability.as_ref(),
                );
                json!({
                    "verification_applicability":
                        to_value(&a04.verification_applicability_for(graph)?)?,
                })
            }
            _ => json!({}),
        };
        Ok(selections.into_inputs(inputs))
    }

    fn fragments(&self, graph: &Graph) -> Vec<Node> {
        self.targets
            .iter()
            .filter_map(|id| graph.node(id).cloned())
            .collect()
    }

    fn candidates(&self) -> Result<&[SegmentCandidate], ApiError> {
        self.segmentation
            .as_ref()
            .map(|(_, s)| s.candidates.as_slice())
            .ok_or_else(|| ApiError::internal("segmentation_intermediate"))
    }

    /// One planned request with its explicitly declared absence policy (R0.2, architecture
    /// §20): `requirement_segmentation`, `scenario_values` and `data_classification` are
    /// `Fallback`; every other planned task kind is `Required`.
    fn planned(
        phase: Phase,
        request: &InferenceRequest,
        material: Result<plumb_inference::InferenceRequestMaterial, String>,
        absence_policy: InferenceAbsencePolicy,
    ) -> Result<PlannedInferenceRequest, ApiError> {
        let material = material.map_err(|e| material_failed(phase, e))?;
        PlannedInferenceRequest::new(request.clone(), material, absence_policy)
            .map_err(|e| plan_failed(phase, e))
    }

    /// PLAN: the phase's inference requests with their exact material.
    pub(crate) fn plan_phase(&self, graph: &Graph) -> Result<StagePlan, ApiError> {
        let phase = self.phase;
        let fail = |e: &dyn PlanError| plan_failed(phase, e);
        let m = |r: Result<plumb_inference::InferenceRequestMaterial, String>| r;
        let mut planned = Vec::new();
        match phase {
            Phase::DirectoryDraft => {
                let r =
                    build_directory_draft_request(graph, self.policy()).map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &r.request,
                    m(r.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Fallback,
                )?);
            }
            Phase::Segmentation => {
                let r = build_segmentation_request(&self.fragments(graph), self.policy())
                    .map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &r.request,
                    m(r.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Fallback,
                )?);
            }
            Phase::Classification => {
                let r = build_requirement_classification_request(
                    graph,
                    self.candidates()?,
                    self.policy(),
                )
                .map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &r.request,
                    m(r.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Required,
                )?);
            }
            Phase::Consolidate => {
                let v = build_vocabulary_request(
                    graph,
                    &self.targets,
                    &self.a04.vocabulary_policy,
                    self.policy(),
                )
                .map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &v.request,
                    m(v.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Required,
                )?);
                let e = build_ears_request(graph, &self.targets, self.policy())
                    .map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &e.request,
                    m(e.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Required,
                )?);
            }
            Phase::Calc => {
                let scope = self
                    .a04
                    .calculation_scope_for(graph)?
                    .ok_or_else(|| ApiError::internal("calculation_scope"))?;
                let r = build_calculation_request(graph, &self.targets, &scope, self.policy())
                    .map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &r.request,
                    m(r.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Required,
                )?);
            }
            Phase::Analyse => {
                let d = build_domain_request(graph, self.policy()).map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &d.request,
                    m(d.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Required,
                )?);
                let l = build_lifecycle_request(graph, self.policy()).map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &l.request,
                    m(l.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Required,
                )?);
            }
            Phase::Flows => {
                let op = self
                    .a04
                    .operation_scope_for(graph)?
                    .ok_or_else(|| ApiError::internal("operation_scope"))?;
                let pr = self
                    .a04
                    .process_scope_for(graph)?
                    .ok_or_else(|| ApiError::internal("process_scope"))?;
                let o = build_operation_request(graph, &self.targets, &op, self.policy())
                    .map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &o.request,
                    m(o.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Required,
                )?);
                let p = build_process_request(graph, &self.targets, &pr, self.policy())
                    .map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &p.request,
                    m(p.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Required,
                )?);
            }
            Phase::Scenarios => {
                let inputs = &self
                    .a04
                    .scenario_derivation_for(graph)?
                    .ok_or_else(|| ApiError::internal("scenario_derivation"))?;
                for r in scenario_requests(graph, inputs, &self.policy()).map_err(|e| fail(&e))? {
                    planned.push(Self::planned(
                        phase,
                        &r.request,
                        m(r.inference_material().map_err(|e| e.to_string())),
                        InferenceAbsencePolicy::Fallback,
                    )?);
                }
            }
            Phase::Tags => {
                let r = build_data_class_request(graph, self.policy()).map_err(|e| fail(&e))?;
                planned.push(Self::planned(
                    phase,
                    &r.request,
                    m(r.inference_material().map_err(|e| e.to_string())),
                    InferenceAbsencePolicy::Fallback,
                )?);
            }
            Phase::Execute | Phase::Verification => {}
            Phase::I0 | Phase::F1 | Phase::F2 | Phase::F4 | Phase::Rules | Phase::Directory => {
                return Err(ApiError::internal("phase_without_child"));
            }
        }
        StagePlan::new(self.scope.clone(), Vec::new(), planned, Vec::new())
            .map_err(|e| plan_failed(phase, e))
    }

    /// EVALUATE: the domain analysers over the acquired artifacts only.
    pub(crate) fn evaluate_phase(
        &self,
        graph: &Graph,
        artifacts: &ArtifactSet,
    ) -> Result<PhaseOutput, ApiError> {
        let phase = self.phase;
        let fail = |e: &dyn std::fmt::Display| evaluation_failed(phase, e.to_string());
        let at = self.audit_at;
        let actor = self.audit_actor.clone();
        let acquired = |request: &InferenceRequest| -> Result<(InferenceArtifact, plumb_psg::DerivationRef), ApiError> {
            let (artifact, input) = inference_of(artifacts, request).map_err(|e| fail(&e))?;
            let derivation = derivation_ref(request, &artifact, input, at).map_err(|e| fail(&e))?;
            Ok((artifact, derivation))
        };
        let mut proposals: Vec<Proposal> = Vec::new();
        let mut intermediate = None;
        let mut traces = Vec::new();
        let mut dispositions: Vec<serde_json::Value> = Vec::new();
        let mut report = |analyser: &str, kind: &str, items: serde_json::Value| {
            if let serde_json::Value::Array(items) = items {
                for item in items {
                    dispositions.push(json!({"analyser": analyser, "kind": kind, "detail": item}));
                }
            }
        };
        match phase {
            Phase::DirectoryDraft => {
                let r =
                    build_directory_draft_request(graph, self.policy()).map_err(|e| fail(&e))?;
                // Without inference only the configuration-supplied elements are drafted.
                let artifact = if artifacts.is_declared_absent(&r.request.id) {
                    None
                } else {
                    Some(inference_of(artifacts, &r.request).map_err(|e| fail(&e))?.0)
                };
                let drafted =
                    draft_directory(graph, &r, artifact.as_ref()).map_err(|e| fail(&e))?;
                report("directory", "issue", to_json(&drafted.issues));
                let bindings: Vec<DirectoryCalendarBinding> = self
                    .a04
                    .calendars
                    .iter()
                    .map(|c| DirectoryCalendarBinding {
                        calendar_ref: c.calendar_ref.clone(),
                        weekend: c.weekend.clone(),
                        coverage: c.coverage.clone(),
                    })
                    .collect();
                let audit = DirectoryAudit {
                    created_by: actor,
                    created_at: at,
                };
                let produced = produce_directory(graph, &drafted.draft, &bindings, &audit)
                    .map_err(|e| fail(&e))?;
                for outcome in produced.outcomes {
                    if let DirectoryDisposition::Proposed { proposal } = outcome.disposition {
                        proposals.push(*proposal);
                    }
                }
            }
            Phase::Segmentation => {
                let fragments = self.fragments(graph);
                let r: SegmentationRequest =
                    build_segmentation_request(&fragments, self.policy()).map_err(|e| fail(&e))?;
                // R0.2: only an explicitly declared-absent request takes the existing
                // no-inference path; a missing acquisition is never an implicit fallback.
                let artifact = if artifacts.is_declared_absent(&r.request.id) {
                    None
                } else {
                    Some(inference_of(artifacts, &r.request).map_err(|e| fail(&e))?.0)
                };
                let audit = SegmentationAudit {
                    created_by: actor,
                    created_at: at,
                };
                let result = evaluate_segmentation(&r, &fragments, artifact.as_ref(), &audit)
                    .map_err(|e| fail(&e))?;
                let value = SegmentationIntermediate {
                    version: 1,
                    candidates: result.candidates,
                    mode: result.mode,
                    fallback_reason: result.fallback_reason,
                    fallback_derivation: result.fallback_derivation,
                };
                intermediate = Some(to_canonical_json(&value).map_err(|e| fail(&e))?);
            }
            Phase::Classification => {
                let candidates = self.candidates()?;
                let r = build_requirement_classification_request(graph, candidates, self.policy())
                    .map_err(|e| fail(&e))?;
                let (artifact, derivation_ref) = acquired(&r.request)?;
                let audit = RequirementCompilationAudit {
                    created_by: actor,
                    created_at: at,
                };
                let inference = RequirementClassificationInference {
                    artifact: &artifact,
                    derivation_ref,
                };
                let result =
                    compile_requirement_candidates(graph, candidates, &r, Some(inference), &audit)
                        .map_err(|e| fail(&e))?;
                report("requirements", "unresolved", to_json(&result.unresolved));
                proposals = result.proposals;
            }
            Phase::Consolidate => {
                let policy = &self.a04.vocabulary_policy;
                let v = build_vocabulary_request(graph, &self.targets, policy, self.policy())
                    .map_err(|e| fail(&e))?;
                let (artifact, derivation_ref) = acquired(&v.request)?;
                let audit = VocabularyAudit {
                    created_by: actor.clone(),
                    created_at: at,
                };
                let inference = VocabularyInference {
                    artifact: &artifact,
                    derivation_ref,
                };
                let result = analyze_vocabulary(graph, &v, policy, Some(inference), &audit)
                    .map_err(|e| fail(&e))?;
                report("vocabulary", "disposition", to_json(&result.dispositions));
                report("vocabulary", "conflict", to_json(&result.conflicts));
                report("vocabulary", "issue", to_json(&result.issues));
                proposals.extend(result.proposals);
                let e = build_ears_request(graph, &self.targets, self.policy())
                    .map_err(|e| fail(&e))?;
                let (artifact, derivation_ref) = acquired(&e.request)?;
                let inference = EarsInference {
                    artifact: &artifact,
                    derivation_ref,
                };
                proposals.extend(
                    evaluate_ears_normalization(graph, &e, Some(inference))
                        .map_err(|e| fail(&e))?
                        .proposals,
                );
                proposals.extend(
                    analyze_requirement_duplicates(graph, &self.a04.duplicate_policy)
                        .map_err(|e| fail(&e))?
                        .proposals
                        .into_iter()
                        .map(|d| match d {
                            DuplicateResolutionProposal::Merge { proposal, .. }
                            | DuplicateResolutionProposal::Supersede { proposal, .. } => proposal,
                        }),
                );
            }
            Phase::Calc => {
                let scope = self
                    .a04
                    .calculation_scope_for(graph)?
                    .ok_or_else(|| ApiError::internal("calculation_scope"))?;
                let r = build_calculation_request(graph, &self.targets, &scope, self.policy())
                    .map_err(|e| fail(&e))?;
                let (artifact, derivation_ref) = acquired(&r.request)?;
                let audit = CalculationAudit {
                    created_by: actor,
                    created_at: at,
                };
                let inference = CalculationInference {
                    artifact: &artifact,
                    derivation_ref,
                };
                let result = analyze_calculations(graph, &r, Some(inference), &audit)
                    .map_err(|e| fail(&e))?;
                report("calculations", "issue", to_json(&result.issues));
                let ineligible: Vec<Value> = result
                    .candidates
                    .iter()
                    .filter(|c| !c.qualification.issues.is_empty())
                    .map(|c| {
                        json!({"disposition": format!("{:?}", c.disposition),
                                    "issues": to_json(&c.qualification.issues)})
                    })
                    .collect();
                report("calculations", "candidate", Value::Array(ineligible));
                proposals = result.proposals;
            }
            Phase::Analyse => {
                let d = build_domain_request(graph, self.policy()).map_err(|e| fail(&e))?;
                let (artifact, derivation_ref) = acquired(&d.request)?;
                let audit = DomainAudit {
                    created_by: actor.clone(),
                    created_at: at,
                };
                let inference = DomainInference {
                    artifact: &artifact,
                    derivation_ref,
                };
                let result =
                    analyze_domain(graph, &d, Some(inference), &audit).map_err(|e| fail(&e))?;
                report("domain", "issue", to_json(&result.issues));
                report("domain", "conflict", to_json(&result.conflicts));
                proposals.extend(result.proposals);
                let l = build_lifecycle_request(graph, self.policy()).map_err(|e| fail(&e))?;
                let (artifact, derivation_ref) = acquired(&l.request)?;
                let audit = LifecycleAudit {
                    created_by: actor,
                    created_at: at,
                };
                let inference = LifecycleInference {
                    artifact: &artifact,
                    derivation_ref,
                };
                let result =
                    analyze_lifecycle(graph, &l, Some(inference), &audit).map_err(|e| fail(&e))?;
                report("lifecycle", "issue", to_json(&result.issues));
                report("lifecycle", "conflict", to_json(&result.conflicts));
                proposals.extend(result.proposals);
            }
            Phase::Flows => {
                let op = self
                    .a04
                    .operation_scope_for(graph)?
                    .ok_or_else(|| ApiError::internal("operation_scope"))?;
                let pr = self
                    .a04
                    .process_scope_for(graph)?
                    .ok_or_else(|| ApiError::internal("process_scope"))?;
                let o = build_operation_request(graph, &self.targets, &op, self.policy())
                    .map_err(|e| fail(&e))?;
                let (artifact, derivation_ref) = acquired(&o.request)?;
                let audit = OperationAudit {
                    created_by: actor.clone(),
                    created_at: at,
                };
                let inference = OperationInference {
                    artifact: &artifact,
                    derivation_ref,
                };
                let result =
                    analyze_operations(graph, &o, Some(inference), &audit).map_err(|e| fail(&e))?;
                report("operations", "issue", to_json(&result.issues));
                proposals.extend(result.proposals);
                let p = build_process_request(graph, &self.targets, &pr, self.policy())
                    .map_err(|e| fail(&e))?;
                let (artifact, derivation_ref) = acquired(&p.request)?;
                let audit = ProcessAudit {
                    created_by: actor.clone(),
                    created_at: at,
                };
                let inference = ProcessInference {
                    artifact: &artifact,
                    derivation_ref,
                };
                let result =
                    analyze_processes(graph, &p, Some(inference), &audit).map_err(|e| fail(&e))?;
                report("processes", "issue", to_json(&result.issues));
                proposals.extend(result.proposals);
                if let Some(draft) = &self.a04.authorization_draft_for(graph)? {
                    let audit = AuthorizationAudit {
                        created_by: actor,
                        created_at: at,
                    };
                    proposals.extend(
                        analyze_authorization(graph, draft, &audit)
                            .map_err(|e| fail(&e))?
                            .proposal,
                    );
                }
            }
            Phase::Scenarios => {
                let inputs = &self
                    .a04
                    .scenario_derivation_for(graph)?
                    .ok_or_else(|| ApiError::internal("scenario_derivation"))?;
                let requests =
                    scenario_requests(graph, inputs, &self.policy()).map_err(|e| fail(&e))?;
                // R0.2: a declared-absent request contributes no inference; its value-slot
                // skeleton stays unresolved and nothing is fabricated.
                let mut acquired_all = Vec::with_capacity(requests.len());
                for r in &requests {
                    if !artifacts.is_declared_absent(&r.request.id) {
                        acquired_all.push((r, acquired(&r.request)?));
                    }
                }
                let inferences: Vec<ScenarioInference<'_>> = acquired_all
                    .iter()
                    .map(|(request, (artifact, derivation_ref))| ScenarioInference {
                        request,
                        artifact,
                        derivation_ref: derivation_ref.clone(),
                    })
                    .collect();
                let audit = ScenarioAudit {
                    created_by: actor,
                    created_at: at,
                };
                proposals = derive_scenarios(graph, inputs, &audit, &inferences)
                    .map_err(|e| fail(&e))?
                    .proposals;
            }
            Phase::Tags => {
                let r = build_data_class_request(graph, self.policy()).map_err(|e| fail(&e))?;
                // R0.2: a declared-absent request yields only the dictionary proposals.
                let acquired_tag = if artifacts.is_declared_absent(&r.request.id) {
                    None
                } else {
                    Some(acquired(&r.request)?)
                };
                let inference =
                    acquired_tag
                        .as_ref()
                        .map(|(artifact, derivation_ref)| DataClassInference {
                            artifact,
                            derivation_ref: derivation_ref.clone(),
                        });
                proposals = analyze_data_classification(graph, &r, inference)
                    .map_err(|e| fail(&e))?
                    .proposals;
            }
            Phase::Execute => {
                let executed = execute_scenarios(
                    graph,
                    &self.targets,
                    &ScenarioExecution {
                        actor_ref: &actor,
                        calendars: self.a03.calendars.as_ref(),
                        predicates: self.a03.predicates.as_ref(),
                        expression_scopes: &self.a03.expression_scopes_for(graph)?,
                        calculation_scope_overrides: &self
                            .a03
                            .calculation_scope_overrides_for(graph)?,
                        runtime_semantic_values: &self.a03.runtime_semantic_values_for(graph)?,
                    },
                )?;
                let mut updates = Vec::with_capacity(executed.len());
                for item in executed {
                    traces.push((item.trace_hash, item.trace_bytes));
                    updates.push(item.update);
                }
                proposals.extend(
                    propose_scenario_run_updates(graph, &updates, &actor, at)
                        .map_err(|e| fail(&e))?,
                );
            }
            Phase::Verification => {
                let inputs = &self
                    .a04
                    .verification_applicability_for(graph)?
                    .ok_or_else(|| ApiError::internal("verification_applicability"))?;
                let audit = VerificationAudit {
                    created_by: actor,
                    created_at: at,
                };
                proposals = compile_verification_obligations(graph, inputs, &audit)
                    .map_err(|e| fail(&e))?
                    .proposals;
            }
            Phase::I0 | Phase::F1 | Phase::F2 | Phase::F4 | Phase::Rules | Phase::Directory => {
                return Err(ApiError::internal("phase_without_child"));
            }
        }
        let evaluation = StageEvaluation::new(None, proposals).map_err(|e| fail(&e))?;
        // What the analysers could not turn into proposals, with their reasons (H0.5: no
        // caller reads only `.proposals`). Segmentation keeps its own typed intermediate; every
        // other phase persists these dispositions as its intermediate projection.
        if intermediate.is_none() && !dispositions.is_empty() {
            let value = json!({"version": 1, "dispositions": dispositions});
            intermediate = Some(to_canonical_json(&value).map_err(|e| fail(&e))?);
        }
        Ok(PhaseOutput {
            evaluation,
            intermediate,
            traces,
        })
    }
}

/// The adapter as a compiler stage: PLAN and EVALUATE are the pure phase functions.
impl CompilerStage for PhaseStage<'_> {
    fn id(&self) -> StageId {
        self.phase.stage()
    }

    fn plan(&self, graph: &Graph, ctx: &CompileContext) -> Result<StagePlan, CompilerError> {
        ctx.validate_for_graph(graph)?;
        self.plan_phase(graph)
            .map_err(|e| stage_failure(self.phase, e))
    }

    fn evaluate(
        &self,
        graph: &Graph,
        ctx: &CompileContext,
        plan: &StagePlan,
        artifacts: &ArtifactSet,
    ) -> Result<StageEvaluation, CompilerError> {
        ctx.validate_for_graph(graph)?;
        plan.validate(self.id(), graph)?;
        artifacts.validate_for_plan(plan)?;
        self.evaluate_phase(graph, artifacts)
            .map(|o| o.evaluation)
            .map_err(|e| stage_failure(self.phase, e))
    }
}

fn stage_failure(phase: Phase, error: ApiError) -> CompilerError {
    CompilerError::StageFailure {
        stage: phase.stage(),
        code: error.body().code.to_owned(),
        message: error.body().message.clone(),
    }
}

/// The deterministic `rules` analysis Projection: every selected Accepted DecisionTable decoded
/// strictly and analysed, sorted by ref.
pub(crate) fn rules_projection(graph: &Graph, tables: &[Id]) -> Result<Vec<u8>, ApiError> {
    let phase = Phase::Rules;
    let mut entries = Vec::with_capacity(tables.len());
    for id in tables {
        let Some(plumb_psg::NodePayload::DecisionTable(table)) = graph.node(id).map(|n| &n.payload)
        else {
            return Err(evaluation_failed(
                phase,
                format!("{id} is not a DecisionTable"),
            ));
        };
        let spec = plumb_functional::decode_decision_table(table).map_err(|e| {
            ApiError::conflict(
                "E_DECISION_TABLE_ENCODING_INVALID",
                "the persisted decision table encoding is malformed",
            )
            .with_details(json!({ "decision_table": id, "reason": e.to_string() }))
        })?;
        entries.push(json!({
            "decision_table_ref": id,
            "analysis": to_value(&analyze_decision_table(&spec))?,
        }));
    }
    to_canonical_json(&json!({ "version": 1, "tables": entries }))
        .map_err(|e| ApiError::internal_cause("rules_projection", &e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fable #182: a phase plan failure names its typed error in details.error, including when the
    /// underlying builder reports only text.
    #[test]
    fn phase_plan_failures_name_the_typed_error() {
        let text = material_failed(Phase::Segmentation, "bad material".to_owned());
        assert_eq!(text.body().code, "E_STAGE_PLAN_FAILED");
        assert_eq!(
            text.body().details,
            json!({"phase": "segmentation", "error": "InferenceRequestMaterialInvalid"})
        );
        let typed = plan_failed(
            Phase::Classification,
            serde_json::from_str::<Value>("{").unwrap_err(),
        );
        assert_eq!(typed.body().details["error"], "Error");
        let fail = |e: &dyn PlanError| plan_failed(Phase::Calc, e);
        let named = fail(&NamedFailure::new("CalcPlanInvalid", "x"));
        assert_eq!(named.body().details["error"], "CalcPlanInvalid");
        // Why a bare string is never passed: its debug form names nothing.
        assert_eq!(error_name(&"bad material"), "");
    }
}
