//! Inference acquisition and provenance of A0.4 (Hotfix 066, Hotfix 067): every planned request
//! executes with its exact material outside every store lock, its verified bundle is persisted,
//! and EVALUATE receives only the validated-inference artifact. The analyser `DerivationRef` is
//! reconstructed through `materialize_derivation_record_from_artifact`; no identity is computed
//! here.
//!
//! R0.2 (architecture §20): under the `null` provider mode a plan with any `Required` request
//! fails with `E_INFERENCE_REQUIRED` before any artifact write or provider call, and the
//! `Fallback` requests of an otherwise acquirable plan are recorded as declared absent without
//! calling the provider. `replay` and `anthropic` execute every request exactly as before.

use std::collections::BTreeSet;

use plumb_artifacts::ArtifactKind;
use plumb_compiler::{ArtifactInput, ArtifactSet, InferenceAbsencePolicy, StagePlan};
use plumb_core::{to_canonical_json, Hash, Timestamp};
use plumb_inference::{
    materialize_derivation_record_from_artifact, persist_inference_bundle, InferenceArtifact,
    InferenceRequest, PersistedInferenceRefs, JSON_MEDIA_TYPE,
};
use plumb_psg::DerivationRef;
use serde_json::json;

use crate::error::ApiError;
use crate::runtime::ProviderMode;
use crate::stage::recipe::{Phase, Recipe};
use crate::stage::scenario_execution::error_name;
use crate::state::ApiState;

/// 502-class recipe failure `E_INFERENCE_FAILED` naming the provider or bundle error.
fn inference_failed<E: std::fmt::Debug>(error: &E) -> ApiError {
    ApiError::inference_failed(&error_name(error))
}

/// Acquires every artifact of `plan`: deterministic planned artifacts are stored by content
/// hash; each planned inference request is executed with its exact material outside every
/// store lock and persisted as one verified bundle. A non-empty external validation list is
/// `E_EXTERNAL_VALIDATOR_UNAVAILABLE` (the pilot has no validator transport). Under the `null`
/// provider mode the declared absence policies apply first (R0.2).
pub(crate) fn acquire(
    state: &ApiState,
    plan: &StagePlan,
    recipe: Recipe,
    phase: Phase,
    created_at: Timestamp,
) -> Result<ArtifactSet, ApiError> {
    if !plan.external_validation_requests.is_empty() {
        return Err(ApiError::conflict(
            "E_EXTERNAL_VALIDATOR_UNAVAILABLE",
            "the stage plan needs an external validator, and none is configured",
        )
        .with_details(json!({ "requests": plan.external_validation_requests.len() })));
    }
    let null = matches!(
        state.stage_runtime("provider_mode")?.provider_mode,
        ProviderMode::Null
    );
    if null {
        let required: BTreeSet<&str> = plan
            .inference_requests
            .iter()
            .filter(|r| r.absence_policy == InferenceAbsencePolicy::Required)
            .map(|r| r.request.task_kind.as_str())
            .collect();
        if !required.is_empty() {
            let kinds: Vec<&str> = required.into_iter().collect();
            return Err(ApiError::inference_required(
                recipe.as_str(),
                phase.as_str(),
                &kinds,
            ));
        }
    }
    let mut deterministic = Vec::with_capacity(plan.deterministic_artifacts.len());
    for planned in &plan.deterministic_artifacts {
        let hash = state
            .with_artifact_store(|store| {
                plumb_artifacts::ArtifactStore::put(
                    store,
                    planned.kind,
                    &planned.media_type,
                    &planned.bytes,
                    created_at,
                )
            })?
            .map_err(|e| ApiError::artifact_store(&e, "artifact_store"))?;
        deterministic.push(ArtifactInput {
            hash,
            kind: planned.kind,
            media_type: planned.media_type.clone(),
            bytes: planned.bytes.clone(),
        });
    }
    if null {
        // Every planned request is `Fallback` here: none is executed and no artifact exists.
        let absent = plan
            .inference_requests
            .iter()
            .map(|r| r.request.id.clone())
            .collect();
        return ArtifactSet::with_declared_absence(deterministic, Vec::new(), Vec::new(), absent)
            .map_err(|e| ApiError::internal_cause("artifact_set", &e));
    }
    let provider = state
        .runtime("inference_provider")?
        .inference_provider
        .clone();
    let mut inference = Vec::with_capacity(plan.inference_requests.len());
    for planned in &plan.inference_requests {
        // No lock is held here: the provider may perform network I/O.
        let execution = provider
            .execute(&planned.request, &planned.material)
            .map_err(|e| inference_failed(&e))?;
        let refs = state
            .with_artifact_store(|store| {
                persist_inference_bundle(store, &planned.request, &execution, created_at)
            })?
            .map_err(|e| inference_failed(&e))?;
        let bytes = to_canonical_json(&execution.artifact)
            .map_err(|e| ApiError::internal_cause("inference_artifact", &e))?;
        inference.push(ArtifactInput {
            hash: refs.validated_inference_ref,
            kind: ArtifactKind::ValidatedInference,
            media_type: JSON_MEDIA_TYPE.to_owned(),
            bytes,
        });
    }
    ArtifactSet::new(deterministic, inference, Vec::new())
        .map_err(|e| ApiError::internal_cause("artifact_set", &e))
}

/// The validated-inference artifact of `request` among `artifacts` with its input hash.
pub(crate) fn inference_of<'a>(
    artifacts: &'a ArtifactSet,
    request: &InferenceRequest,
) -> Result<(InferenceArtifact, &'a ArtifactInput), String> {
    for input in &artifacts.inference_artifacts {
        let artifact: InferenceArtifact =
            serde_json::from_slice(&input.bytes).map_err(|e| e.to_string())?;
        if artifact.request_hash == request.id {
            return Ok((artifact, input));
        }
    }
    Err(format!("no acquired inference for request {}", request.id))
}

/// The analyser `DerivationRef` of one acquisition, reconstructed purely from the request, the
/// acquired artifact input and the frozen rules: `output_refs = [validated_inference_ref]` and
/// `created_at` = the child input revision's `created_at`.
pub(crate) fn derivation_ref(
    request: &InferenceRequest,
    artifact: &InferenceArtifact,
    input: &ArtifactInput,
    created_at: Timestamp,
) -> Result<DerivationRef, String> {
    let request_bytes = to_canonical_json(request).map_err(|e| e.to_string())?;
    let persisted = PersistedInferenceRefs {
        request_artifact_ref: Hash::content_sha256(&request_bytes),
        raw_response_ref: artifact.raw_response_hash.clone(),
        validated_inference_ref: input.hash.clone(),
    };
    let record = materialize_derivation_record_from_artifact(
        request,
        artifact,
        &persisted,
        vec![input.hash.to_string()],
        created_at,
    )
    .map_err(|e| e.to_string())?;
    Ok(DerivationRef::from(record.id))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use plumb_artifacts::{ArtifactKind, ArtifactStore};
    use plumb_compiler::{
        InferenceAbsencePolicy, PlannedArtifact, PlannedInferenceRequest, Scope, StagePlan,
    };
    use plumb_core::{to_canonical_json, Hash};
    use plumb_inference::{MockProvider, ProviderError};
    use serde_json::json;

    use super::acquire;
    use crate::runtime::ProviderMode;
    use crate::stage::recipe::{Phase, Recipe};
    use crate::stage::test_support::{at, request, state, Recording};
    use crate::state::ApiState;

    use InferenceAbsencePolicy::{Fallback, Required};

    /// The eight Required task kinds of the frozen R0.2 census.
    const REQUIRED: [&str; 8] = [
        "requirement_classification",
        "vocabulary_analysis",
        "ears_normalization",
        "calculation_analysis",
        "domain_extraction",
        "lifecycle_extraction",
        "operation_analysis",
        "process_analysis",
    ];
    const FALLBACK: [&str; 3] = [
        "requirement_segmentation",
        "scenario_values",
        "data_classification",
    ];

    fn listing() -> PlannedArtifact {
        PlannedArtifact {
            kind: ArtifactKind::Projection,
            media_type: "text/plain".into(),
            bytes: b"synthetic deterministic input".to_vec(),
        }
    }

    fn planned(
        task_kind: &str,
        salt: &str,
        policy: InferenceAbsencePolicy,
    ) -> PlannedInferenceRequest {
        let (r, m) = request(task_kind, salt);
        PlannedInferenceRequest::new(r, m, policy).unwrap()
    }

    fn plan(requests: Vec<PlannedInferenceRequest>, deterministic: bool) -> StagePlan {
        let artifacts = if deterministic {
            vec![listing()]
        } else {
            vec![]
        };
        StagePlan::new(Scope::Project, artifacts, requests, vec![]).unwrap()
    }

    fn exists(state: &ApiState, hash: &Hash) -> bool {
        state
            .with_artifact_store(|s| s.exists(hash))
            .unwrap()
            .unwrap()
    }

    fn request_ref(p: &PlannedInferenceRequest) -> Hash {
        Hash::content_sha256(&to_canonical_json(&p.request).unwrap())
    }

    #[test]
    fn null_required_fails_before_any_provider_call_or_write() {
        for kind in REQUIRED {
            let dir = tempfile::tempdir().unwrap();
            let provider = Arc::new(Recording::default());
            let state = state(&dir, ProviderMode::Null, provider.clone());
            let required = planned(kind, "x", Required);
            let p = plan(vec![required.clone()], true);
            let error =
                acquire(&state, &p, Recipe::Consolidate, Phase::Consolidate, at()).unwrap_err();
            let body = error.body();
            assert_eq!(error.body().status.as_u16(), 409, "{kind}");
            assert_eq!(body.code, "E_INFERENCE_REQUIRED");
            assert_eq!(
                body.details,
                json!({"recipe": "consolidate", "phase": "consolidate", "task_kinds": [kind]})
            );
            assert!(provider.calls.lock().unwrap().is_empty(), "{kind}");
            // Nothing is written: not the planned deterministic artifact, not the request.
            assert!(!exists(&state, &listing().content_hash()), "{kind}");
            assert!(!exists(&state, &request_ref(&required)), "{kind}");
        }
    }

    #[test]
    fn null_required_task_kinds_are_sorted_unique_and_block_fallback_members() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(Recording::default());
        let state = state(&dir, ProviderMode::Null, provider.clone());
        let fallback = planned("requirement_segmentation", "f", Fallback);
        let p = plan(
            vec![
                planned("vocabulary_analysis", "a", Required),
                planned("ears_normalization", "b", Required),
                planned("ears_normalization", "c", Required),
                fallback.clone(),
            ],
            true,
        );
        let error = acquire(&state, &p, Recipe::Consolidate, Phase::Consolidate, at()).unwrap_err();
        assert_eq!(
            error.body().details["task_kinds"],
            json!(["ears_normalization", "vocabulary_analysis"])
        );
        assert!(provider.calls.lock().unwrap().is_empty());
        assert!(!exists(&state, &listing().content_hash()));
        assert!(!exists(&state, &request_ref(&fallback)));
    }

    #[test]
    fn null_fallback_declares_absence_without_provider_calls() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(Recording::default());
        let state = state(&dir, ProviderMode::Null, provider.clone());
        let requests: Vec<_> = FALLBACK.iter().map(|k| planned(k, "f", Fallback)).collect();
        let p = plan(requests.clone(), true);
        let set = acquire(&state, &p, Recipe::Tags, Phase::Tags, at()).unwrap();
        set.validate_for_plan(&p).unwrap();
        let mut ids: Vec<Hash> = requests.iter().map(|r| r.request.id.clone()).collect();
        ids.sort();
        assert_eq!(set.declared_absent_inference_requests, ids);
        assert!(set.inference_artifacts.is_empty());
        assert!(provider.calls.lock().unwrap().is_empty());
        // The ordinary deterministic planned artifact is still acquired.
        assert_eq!(set.deterministic_artifacts.len(), 1);
        assert!(exists(&state, &listing().content_hash()));
        for r in &requests {
            assert!(!exists(&state, &request_ref(r)));
        }
        // A plan without inference behaves as before.
        let none = acquire(&state, &plan(vec![], true), Recipe::Tags, Phase::Tags, at()).unwrap();
        assert!(none.declared_absent_inference_requests.is_empty());
    }

    #[test]
    fn replay_executes_fallback_requests_and_never_declares_absence() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(Recording::default());
        let mode = ProviderMode::Replay {
            fixtures_hash: Hash::content_sha256(b"fixtures"),
        };
        let state = state(&dir, mode, provider.clone());
        let fallback = planned("requirement_segmentation", "f", Fallback);
        let required = planned("requirement_classification", "r", Required);
        let p = plan(vec![fallback.clone(), required], false);
        let set = acquire(&state, &p, Recipe::Extract, Phase::Segmentation, at()).unwrap();
        set.validate_for_plan(&p).unwrap();
        assert!(set.declared_absent_inference_requests.is_empty());
        assert_eq!(set.inference_artifacts.len(), 2);
        assert!(!set.is_declared_absent(&fallback.request.id));
        assert_eq!(provider.calls.lock().unwrap().len(), 2);
        assert!(exists(&state, &request_ref(&fallback)));
    }

    #[test]
    fn non_null_provider_errors_stay_inference_failed() {
        let fallback = planned("requirement_segmentation", "f", Fallback);
        let p = plan(vec![fallback.clone()], false);
        let replay = || ProviderMode::Replay {
            fixtures_hash: Hash::content_sha256(b"fixtures"),
        };
        // A replay miss is never absence: the real MockProvider without a fixture.
        let dir = tempfile::tempdir().unwrap();
        let miss = state(
            &dir,
            replay(),
            Arc::new(MockProvider::with_fixtures(Vec::new()).unwrap()),
        );
        let error = acquire(&miss, &p, Recipe::Extract, Phase::Segmentation, at()).unwrap_err();
        assert_eq!(error.body().status.as_u16(), 502);
        assert_eq!(error.body().code, "E_INFERENCE_FAILED");
        assert_eq!(error.body().details, json!({"error": "FixtureNotFound"}));
        // Even ProviderDisabled is a failure when the configured mode is not null.
        for mode in [replay(), ProviderMode::Anthropic] {
            let dir = tempfile::tempdir().unwrap();
            let provider = Arc::new(Recording::default());
            *provider.error.lock().unwrap() = Some(|_| ProviderError::ProviderDisabled);
            let state = state(&dir, mode, provider.clone());
            let error =
                acquire(&state, &p, Recipe::Extract, Phase::Segmentation, at()).unwrap_err();
            assert_eq!(error.body().code, "E_INFERENCE_FAILED");
            assert_eq!(error.body().details, json!({"error": "ProviderDisabled"}));
            assert_eq!(
                provider.calls.lock().unwrap().len(),
                1,
                "the provider was called"
            );
            assert!(!exists(&state, &request_ref(&fallback)));
        }
    }
}
