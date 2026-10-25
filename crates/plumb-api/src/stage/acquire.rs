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
use plumb_inference::provider::ProviderFailure;
use plumb_inference::{
    materialize_derivation_record_from_artifact, persist_inference_bundle, InferenceArtifact,
    InferenceRequest, PersistedInferenceRefs, ProviderError, JSON_MEDIA_TYPE,
};
use plumb_psg::DerivationRef;
use serde_json::json;

use crate::error::ApiError;
use crate::runtime::ProviderMode;
use crate::stage::recipe::{Phase, Recipe};
use crate::stage::scenario_execution::error_name;
use crate::state::ApiState;

/// The most schema-validation errors a failure detail keeps, and the bound of each.
const MAX_SCHEMA_ERRORS: usize = 16;
const MAX_DETAIL_TEXT: usize = 200;

/// `text` cut to at most [`MAX_DETAIL_TEXT`] bytes on a character boundary.
fn bounded_text(text: &str) -> String {
    let mut end = text.len().min(MAX_DETAIL_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// 502-class recipe failure `E_INFERENCE_FAILED` with the bounded typed detail of a failed
/// acquisition of `request` (Fable #253): the provider or bundle error class, the request ID
/// and task kind, the HTTP status and provider request ID of a remote error, the sorted
/// schema-validation errors of a schema-invalid answer and, when stored, `response_ref`, the
/// InferenceResponse artifact of the rejected raw answer.
fn inference_failed<E: std::fmt::Debug>(
    error: &E,
    request: &InferenceRequest,
    provider_error: Option<&ProviderError>,
    response_ref: Option<&Hash>,
) -> ApiError {
    let mut details = json!({
        "error": error_name(error),
        "request_id": request.id,
        "task_kind": request.task_kind,
    });
    match provider_error {
        Some(ProviderError::Remote {
            status, request_id, ..
        }) => {
            details["http_status"] = json!(status);
            if let Some(id) = request_id {
                details["provider_request_id"] = json!(bounded_text(id));
            }
        }
        Some(ProviderError::SchemaInvalidOutput { errors }) => {
            let mut sorted: Vec<String> = errors.iter().map(|e| bounded_text(e)).collect();
            sorted.sort();
            sorted.truncate(MAX_SCHEMA_ERRORS);
            details["schema_errors"] = json!(sorted);
        }
        _ => {}
    }
    if let Some(hash) = response_ref {
        details["response_ref"] = json!(hash);
    }
    ApiError::inference_failed(&error_name(error)).with_details(details)
}

/// The failure of one provider call, with its rejected raw answer stored by content hash.
fn provider_failed(
    state: &ApiState,
    request: &InferenceRequest,
    failure: ProviderFailure,
    created_at: Timestamp,
) -> ApiError {
    let response_ref = failure.rejected_response.and_then(|rejected| {
        let stored = state.with_artifact_store(|store| {
            plumb_artifacts::ArtifactStore::put(
                store,
                ArtifactKind::InferenceResponse,
                &rejected.media_type,
                &rejected.bytes,
                created_at,
            )
        });
        match stored {
            Ok(Ok(hash)) => Some(hash),
            _ => {
                tracing::error!(target: "plumb_api::stage", request_id = %request.id,
                    "storing a rejected provider answer failed");
                None
            }
        }
    });
    inference_failed(
        &failure.error,
        request,
        Some(&failure.error),
        response_ref.as_ref(),
    )
}

/// Acquires every artifact of `plan`: deterministic planned artifacts are stored by content
/// hash; each planned inference request is executed with its exact material outside every
/// store lock and persisted as one verified bundle. A non-empty external validation list is
/// `E_EXTERNAL_VALIDATOR_UNAVAILABLE` (the pilot has no validator transport). Under the `null`
/// provider mode the declared absence policies apply first (R0.2).
#[cfg(test)]
pub(crate) fn acquire(
    state: &ApiState,
    plan: &StagePlan,
    recipe: Recipe,
    phase: Phase,
    created_at: Timestamp,
) -> Result<ArtifactSet, ApiError> {
    acquire_with(state, plan, recipe, phase, created_at, false)
}

/// [`acquire`] for a retried attempt when `reuse` (H0.3, Fable #62): a planned request whose
/// validated inference an earlier attempt already persisted is reused instead of calling the
/// provider again, so a retry never re-bills or re-generates a completed request.
pub(crate) fn acquire_with(
    state: &ApiState,
    plan: &StagePlan,
    recipe: Recipe,
    phase: Phase,
    created_at: Timestamp,
    reuse: bool,
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
        // Fable #62: a request completed by an earlier attempt is reused, never re-billed.
        if reuse {
            if let Some(input) = persisted_inference(state, &planned.request)? {
                inference.push(input);
                continue;
            }
        }
        // No lock is held here: the provider may perform network I/O.
        let execution = provider
            .execute_detailed(&planned.request, &planned.material)
            .map_err(|failure| provider_failed(state, &planned.request, failure, created_at))?;
        let refs = state
            .with_artifact_store(|store| {
                persist_inference_bundle(store, &planned.request, &execution, created_at)
            })?
            .map_err(|e| inference_failed(&e, &planned.request, None, None))?;
        let bytes = to_canonical_json(&execution.artifact)
            .map_err(|e| ApiError::internal_cause("inference_artifact", &e))?;
        inference.push(ArtifactInput {
            hash: refs.validated_inference_ref,
            kind: ArtifactKind::ValidatedInference,
            media_type: JSON_MEDIA_TYPE.to_owned(),
            bytes,
        });
        // H0.3: after a provider call returned and its bundle is persisted is a safe step
        // boundary; a shutting-down process stops here (a retry reuses the bundle).
        if state.stage_stop_requested() {
            return Err(crate::stage::worker::interrupted("after_provider_call"));
        }
    }
    ArtifactSet::new(deterministic, inference, Vec::new())
        .map_err(|e| ApiError::internal_cause("artifact_set", &e))
}

/// The persisted, verified validated-inference artifact of `request`, if an earlier attempt
/// completed it: the stored bytes hash to their ref, decode strictly to canonical JSON, belong to
/// `request`, and their request and raw-response artifacts are stored too.
fn persisted_inference(
    state: &ApiState,
    request: &InferenceRequest,
) -> Result<Option<ArtifactInput>, ApiError> {
    let refs = state
        .with_stage_store(|store| store.validated_inference_refs(&request.id))?
        .map_err(crate::stage::store_error)?;
    let request_ref = Hash::content_sha256(
        &to_canonical_json(request)
            .map_err(|e| ApiError::internal_cause("inference_request", &e))?,
    );
    for hash in refs {
        let Ok(artifact) = state.get_artifact(&hash)? else {
            continue;
        };
        let Ok(decoded) = serde_json::from_slice::<InferenceArtifact>(&artifact.bytes) else {
            continue;
        };
        let canonical = to_canonical_json(&decoded).ok();
        let exists = |h: &Hash| {
            state
                .with_artifact_store(|store| plumb_artifacts::ArtifactStore::exists(store, h))
                .ok()
                .and_then(Result::ok)
                .unwrap_or(false)
        };
        if canonical.as_deref() == Some(artifact.bytes.as_slice())
            && artifact.media_type == JSON_MEDIA_TYPE
            && decoded.validate_for(request).is_ok()
            && exists(&request_ref)
            && exists(&decoded.raw_response_hash)
        {
            return Ok(Some(ArtifactInput {
                hash,
                kind: ArtifactKind::ValidatedInference,
                media_type: JSON_MEDIA_TYPE.to_owned(),
                bytes: artifact.bytes,
            }));
        }
    }
    Ok(None)
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
        // Fable #253: the typed detail names the request and its task kind.
        let detail = |error: &str| {
            json!({"error": error, "request_id": fallback.request.id,
                "task_kind": "requirement_segmentation"})
        };
        assert_eq!(error.body().details, detail("FixtureNotFound"));
        // Even ProviderDisabled is a failure when the configured mode is not null.
        for mode in [replay(), ProviderMode::Anthropic] {
            let dir = tempfile::tempdir().unwrap();
            let provider = Arc::new(Recording::default());
            *provider.error.lock().unwrap() = Some(|_| ProviderError::ProviderDisabled);
            let state = state(&dir, mode, provider.clone());
            let error =
                acquire(&state, &p, Recipe::Extract, Phase::Segmentation, at()).unwrap_err();
            assert_eq!(error.body().code, "E_INFERENCE_FAILED");
            assert_eq!(error.body().details, detail("ProviderDisabled"));
            assert_eq!(
                provider.calls.lock().unwrap().len(),
                1,
                "the provider was called"
            );
            assert!(!exists(&state, &request_ref(&fallback)));
        }
    }
}
