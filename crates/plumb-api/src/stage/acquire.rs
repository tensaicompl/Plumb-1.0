//! Inference acquisition and provenance of A0.4 (Hotfix 066, Hotfix 067): every planned request
//! executes with its exact material outside every store lock, its verified bundle is persisted,
//! and EVALUATE receives only the validated-inference artifact. The analyser `DerivationRef` is
//! reconstructed through `materialize_derivation_record_from_artifact`; no identity is computed
//! here.

use plumb_artifacts::ArtifactKind;
use plumb_compiler::{ArtifactInput, ArtifactSet, StagePlan};
use plumb_core::{to_canonical_json, Hash, Timestamp};
use plumb_inference::{
    materialize_derivation_record_from_artifact, persist_inference_bundle, InferenceArtifact,
    InferenceRequest, PersistedInferenceRefs, JSON_MEDIA_TYPE,
};
use plumb_psg::DerivationRef;
use serde_json::json;

use crate::error::ApiError;
use crate::stage::scenario_execution::error_name;
use crate::state::ApiState;

/// 502-class recipe failure `E_INFERENCE_FAILED` naming the provider or bundle error.
fn inference_failed<E: std::fmt::Debug>(error: &E) -> ApiError {
    ApiError::inference_failed(&error_name(error))
}

/// Acquires every artifact of `plan`: deterministic planned artifacts are stored by content
/// hash; each planned inference request is executed with its exact material outside every
/// store lock and persisted as one verified bundle. A non-empty external validation list is
/// `E_EXTERNAL_VALIDATOR_UNAVAILABLE` (the pilot has no validator transport).
pub(crate) fn acquire(
    state: &ApiState,
    plan: &StagePlan,
    created_at: Timestamp,
) -> Result<ArtifactSet, ApiError> {
    if !plan.external_validation_requests.is_empty() {
        return Err(ApiError::conflict(
            "E_EXTERNAL_VALIDATOR_UNAVAILABLE",
            "the stage plan needs an external validator, and none is configured",
        )
        .with_details(json!({ "requests": plan.external_validation_requests.len() })));
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
            .map_err(|_| ApiError::internal("artifact_store"))?;
        deterministic.push(ArtifactInput {
            hash,
            kind: planned.kind,
            media_type: planned.media_type.clone(),
            bytes: planned.bytes.clone(),
        });
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
            .map_err(|_| ApiError::internal("inference_artifact"))?;
        inference.push(ArtifactInput {
            hash: refs.validated_inference_ref,
            kind: ArtifactKind::ValidatedInference,
            media_type: JSON_MEDIA_TYPE.to_owned(),
            bytes,
        });
    }
    ArtifactSet::new(deterministic, inference, Vec::new())
        .map_err(|e| ApiError::internal(&format!("artifact_set: {}", error_name(&e))))
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
