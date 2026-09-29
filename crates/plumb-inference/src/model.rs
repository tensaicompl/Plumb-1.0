//! Inference requests, artifacts and provider executions, their acquisition as immutable
//! artifacts, and deterministic `DerivationRecord` materialization (compiler architecture
//! §§4.3-4.4, 19-20; metamodel §5.3).

use plumb_artifacts::{ArtifactKind, ArtifactStore, ArtifactStoreError};
use plumb_core::{
    to_canonical_json, CanonicalJson, CoreError, Hash, HashKind, Id, StageId, Timestamp,
};
use plumb_psg::{DerivationKind, DerivationRecord, DerivationRecordError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

/// Media type of the request and validated-inference artifacts.
pub const JSON_MEDIA_TYPE: &str = "application/json";

/// Why an inference value, acquisition bundle or derivation is invalid.
#[derive(Debug, Error)]
pub enum InferenceError {
    /// A provider identifier does not match `^[a-z][a-z0-9._-]*$`.
    #[error("invalid provider identifier {0:?}")]
    InvalidProvider(String),
    /// `ProviderPolicy.config` is not a JSON object.
    #[error("provider config must be a JSON object")]
    ProviderConfigNotObject,
    /// A free-text field is empty, padded with whitespace or contains control characters.
    #[error("invalid {field}: {value:?}")]
    InvalidText { field: &'static str, value: String },
    /// A hash field is not a generic `sha256:` hash.
    #[error("{field} must be a generic sha256: hash, got {kind:?}")]
    NonGenericHash { field: &'static str, kind: HashKind },
    /// A set-valued reference list contains the same entry twice.
    #[error("duplicate {field} entry {value}")]
    DuplicateRef { field: &'static str, value: String },
    /// A set-valued reference list is not in canonical sorted order.
    #[error("{field} is not sorted")]
    UnsortedRefs { field: &'static str },
    /// The supplied request ID is not the ID recomputed from the identity projection.
    #[error("request id {supplied} does not match recomputed id {recomputed}")]
    RequestIdMismatch { supplied: Hash, recomputed: Hash },
    /// `InferenceArtifact.request_hash` is not the request's ID.
    #[error("artifact request_hash {artifact} does not match request id {request}")]
    RequestHashMismatch { request: Hash, artifact: Hash },
    /// `InferenceArtifact.provider` is not the request's policy provider.
    #[error("artifact provider {artifact:?} does not match request provider {request:?}")]
    ProviderMismatch { request: String, artifact: String },
    /// `validated_output_hash` is not the content hash of `validated_output`.
    #[error("validated_output_hash {supplied} does not match output content hash {computed}")]
    ValidatedOutputHashMismatch { supplied: Hash, computed: Hash },
    /// `raw_response_hash` is not the SHA-256 of the raw response bytes.
    #[error("raw_response_hash {supplied} does not match raw response hash {computed}")]
    RawResponseHashMismatch { supplied: Hash, computed: Hash },
    /// A persisted raw-response reference differs from the artifact's raw_response_hash.
    #[error("persisted raw response ref {persisted} does not match raw_response_hash {expected}")]
    PersistedRawResponseMismatch { persisted: Hash, expected: Hash },
    /// `output_refs` contains the same value twice.
    #[error("duplicate output ref {0:?}")]
    DuplicateOutputRef(String),
    /// The materialized record fails `DerivationRecord::validate`.
    #[error(transparent)]
    InvalidDerivationRecord(#[from] DerivationRecordError),
    /// Canonical JSON or hashing failed.
    #[error(transparent)]
    Core(#[from] CoreError),
    /// Persisting an acquisition artifact failed.
    #[error(transparent)]
    Artifact(#[from] ArtifactStoreError),
}

fn require_generic(field: &'static str, hash: &Hash) -> Result<(), InferenceError> {
    match hash.kind() {
        HashKind::Generic => Ok(()),
        kind => Err(InferenceError::NonGenericHash { field, kind }),
    }
}

/// Non-empty, no leading/trailing whitespace, no control characters; otherwise exact.
fn require_text(field: &'static str, value: &str) -> Result<(), InferenceError> {
    let valid = !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control);
    if valid {
        Ok(())
    } else {
        Err(InferenceError::InvalidText {
            field,
            value: value.to_owned(),
        })
    }
}

/// `^[a-z][a-z0-9._-]*$`, with no normalization.
fn require_provider(value: &str) -> Result<(), InferenceError> {
    let mut bytes = value.bytes();
    let valid = bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(InferenceError::InvalidProvider(value.to_owned()))
    }
}

/// Requires `refs` to be strictly increasing (sorted, no duplicates).
fn require_sorted_set(field: &'static str, refs: &[Id]) -> Result<(), InferenceError> {
    for pair in refs.windows(2) {
        if pair[0] == pair[1] {
            return Err(InferenceError::DuplicateRef {
                field,
                value: pair[0].to_string(),
            });
        }
        if pair[0] > pair[1] {
            return Err(InferenceError::UnsortedRefs { field });
        }
    }
    Ok(())
}

/// Sorts `refs`, rejecting duplicates.
fn sorted_set(field: &'static str, mut refs: Vec<Id>) -> Result<Vec<Id>, InferenceError> {
    refs.sort();
    require_sorted_set(field, &refs)?;
    Ok(refs)
}

// ============================================================================ ProviderPolicy

/// Provider selection and opaque provider configuration; part of request identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ProviderPolicyFields")]
pub struct ProviderPolicy {
    pub provider: String,
    pub config: CanonicalJson,
}

impl ProviderPolicy {
    pub fn validate(&self) -> Result<(), InferenceError> {
        require_provider(&self.provider)?;
        if !self.config.as_value().is_object() {
            return Err(InferenceError::ProviderConfigNotObject);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderPolicyFields {
    provider: String,
    config: CanonicalJson,
}

impl TryFrom<ProviderPolicyFields> for ProviderPolicy {
    type Error = InferenceError;

    fn try_from(f: ProviderPolicyFields) -> Result<Self, Self::Error> {
        let policy = ProviderPolicy {
            provider: f.provider,
            config: f.config,
        };
        policy.validate()?;
        Ok(policy)
    }
}

// ============================================================================ InferenceRequest

/// A deterministic, content-identified request for inference (compiler architecture §4.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "InferenceRequestFields")]
pub struct InferenceRequest {
    pub id: Hash,
    pub stage: StageId,
    pub task_kind: String,
    pub input_refs: Vec<Id>,
    pub evidence_refs: Vec<Id>,
    pub context_hash: Hash,
    pub prompt_template_hash: Hash,
    pub schema_hash: Hash,
    pub provider_policy: ProviderPolicy,
}

impl InferenceRequest {
    /// Builds a request, storing `input_refs` and `evidence_refs` sorted and computing `id`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stage: StageId,
        task_kind: String,
        input_refs: Vec<Id>,
        evidence_refs: Vec<Id>,
        context_hash: Hash,
        prompt_template_hash: Hash,
        schema_hash: Hash,
        provider_policy: ProviderPolicy,
    ) -> Result<InferenceRequest, InferenceError> {
        let mut request = InferenceRequest {
            // Placeholder replaced below; identity never includes `id`.
            id: Hash::content_sha256(b""),
            stage,
            task_kind,
            input_refs: sorted_set("input_refs", input_refs)?,
            evidence_refs: sorted_set("evidence_refs", evidence_refs)?,
            context_hash,
            prompt_template_hash,
            schema_hash,
            provider_policy,
        };
        request.id = request.recompute_id()?;
        request.validate()?;
        Ok(request)
    }

    /// Exactly the fields that determine the request ID (everything except `id`).
    pub fn identity_projection(&self) -> Value {
        json!({
            "stage": self.stage,
            "task_kind": self.task_kind,
            "input_refs": self.input_refs,
            "evidence_refs": self.evidence_refs,
            "context_hash": self.context_hash,
            "prompt_template_hash": self.prompt_template_hash,
            "schema_hash": self.schema_hash,
            "provider_policy": self.provider_policy,
        })
    }

    /// Generic SHA-256 of the RFC 8785 canonical identity projection.
    pub fn recompute_id(&self) -> Result<Hash, InferenceError> {
        Ok(Hash::content_sha256(&to_canonical_json(
            &self.identity_projection(),
        )?))
    }

    /// Checks every field rule and that `id` equals the recomputed ID.
    pub fn validate(&self) -> Result<(), InferenceError> {
        require_generic("id", &self.id)?;
        require_text("task_kind", &self.task_kind)?;
        require_sorted_set("input_refs", &self.input_refs)?;
        require_sorted_set("evidence_refs", &self.evidence_refs)?;
        require_generic("context_hash", &self.context_hash)?;
        require_generic("prompt_template_hash", &self.prompt_template_hash)?;
        require_generic("schema_hash", &self.schema_hash)?;
        self.provider_policy.validate()?;
        let recomputed = self.recompute_id()?;
        if recomputed != self.id {
            return Err(InferenceError::RequestIdMismatch {
                supplied: self.id.clone(),
                recomputed,
            });
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InferenceRequestFields {
    id: Hash,
    stage: StageId,
    task_kind: String,
    input_refs: Vec<Id>,
    evidence_refs: Vec<Id>,
    context_hash: Hash,
    prompt_template_hash: Hash,
    schema_hash: Hash,
    provider_policy: ProviderPolicy,
}

impl TryFrom<InferenceRequestFields> for InferenceRequest {
    type Error = InferenceError;

    fn try_from(f: InferenceRequestFields) -> Result<Self, Self::Error> {
        let request = InferenceRequest {
            id: f.id,
            stage: f.stage,
            task_kind: f.task_kind,
            input_refs: f.input_refs,
            evidence_refs: f.evidence_refs,
            context_hash: f.context_hash,
            prompt_template_hash: f.prompt_template_hash,
            schema_hash: f.schema_hash,
            provider_policy: f.provider_policy,
        };
        request.validate()?;
        Ok(request)
    }
}

// ============================================================================ InferenceArtifact

/// The validated, replayable result of one inference (compiler architecture §4.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "InferenceArtifactFields")]
pub struct InferenceArtifact {
    pub request_hash: Hash,
    pub provider: String,
    pub model: String,
    pub parameters: CanonicalJson,
    pub raw_response_hash: Hash,
    pub validated_output: CanonicalJson,
    pub validated_output_hash: Hash,
}

impl InferenceArtifact {
    /// Checks the structural rules that do not need the request.
    pub fn validate(&self) -> Result<(), InferenceError> {
        require_generic("request_hash", &self.request_hash)?;
        require_provider(&self.provider)?;
        require_text("model", &self.model)?;
        require_generic("raw_response_hash", &self.raw_response_hash)?;
        require_generic("validated_output_hash", &self.validated_output_hash)?;
        let computed = self.validated_output.content_hash()?;
        if computed != self.validated_output_hash {
            return Err(InferenceError::ValidatedOutputHashMismatch {
                supplied: self.validated_output_hash.clone(),
                computed,
            });
        }
        Ok(())
    }

    /// Checks the structural rules and the invariants against `request`.
    pub fn validate_for(&self, request: &InferenceRequest) -> Result<(), InferenceError> {
        request.validate()?;
        self.validate()?;
        if self.request_hash != request.id {
            return Err(InferenceError::RequestHashMismatch {
                request: request.id.clone(),
                artifact: self.request_hash.clone(),
            });
        }
        if self.provider != request.provider_policy.provider {
            return Err(InferenceError::ProviderMismatch {
                request: request.provider_policy.provider.clone(),
                artifact: self.provider.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InferenceArtifactFields {
    request_hash: Hash,
    provider: String,
    model: String,
    parameters: CanonicalJson,
    raw_response_hash: Hash,
    validated_output: CanonicalJson,
    validated_output_hash: Hash,
}

impl TryFrom<InferenceArtifactFields> for InferenceArtifact {
    type Error = InferenceError;

    fn try_from(f: InferenceArtifactFields) -> Result<Self, Self::Error> {
        let artifact = InferenceArtifact {
            request_hash: f.request_hash,
            provider: f.provider,
            model: f.model,
            parameters: f.parameters,
            raw_response_hash: f.raw_response_hash,
            validated_output: f.validated_output,
            validated_output_hash: f.validated_output_hash,
        };
        artifact.validate()?;
        Ok(artifact)
    }
}

// ============================================================================ ProviderExecution

/// What a provider returns: the replay artifact plus the raw response kept outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderExecution {
    pub artifact: InferenceArtifact,
    pub raw_response_media_type: String,
    pub raw_response: Vec<u8>,
}

impl ProviderExecution {
    /// Checks the structural rules that do not need the request.
    pub fn validate(&self) -> Result<(), InferenceError> {
        self.artifact.validate()?;
        if self.raw_response_media_type.is_empty()
            || self.raw_response_media_type.chars().any(char::is_control)
        {
            return Err(InferenceError::InvalidText {
                field: "raw_response_media_type",
                value: self.raw_response_media_type.clone(),
            });
        }
        let computed = Hash::content_sha256(&self.raw_response);
        if computed != self.artifact.raw_response_hash {
            return Err(InferenceError::RawResponseHashMismatch {
                supplied: self.artifact.raw_response_hash.clone(),
                computed,
            });
        }
        Ok(())
    }

    /// Checks the structural rules and every artifact/request invariant.
    pub fn validate_for(&self, request: &InferenceRequest) -> Result<(), InferenceError> {
        self.validate()?;
        self.artifact.validate_for(request)
    }
}

// ============================================================================ acquisition

/// Artifact-store references of one persisted acquisition bundle; all generic hashes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedInferenceRefs {
    /// Hash of the canonical complete `InferenceRequest` (not required to equal `request.id`).
    pub request_artifact_ref: Hash,
    /// Hash of the exact raw response bytes (equals `raw_response_hash`).
    pub raw_response_ref: Hash,
    /// Hash of the canonical complete `InferenceArtifact` (not `validated_output_hash`).
    pub validated_inference_ref: Hash,
}

impl PersistedInferenceRefs {
    pub fn validate(&self) -> Result<(), InferenceError> {
        require_generic("request_artifact_ref", &self.request_artifact_ref)?;
        require_generic("raw_response_ref", &self.raw_response_ref)?;
        require_generic("validated_inference_ref", &self.validated_inference_ref)
    }
}

/// Persists the request, raw response and validated inference of one execution.
///
/// Everything is validated before the first write. The three puts are not one transaction: an
/// immutable artifact stored before a later failure may remain, a retry is idempotent, and no
/// accepted PSG state changes. This never takes part in a graph-commit transaction.
pub fn persist_inference_bundle<S: ArtifactStore>(
    store: &mut S,
    request: &InferenceRequest,
    execution: &ProviderExecution,
    created_at: Timestamp,
) -> Result<PersistedInferenceRefs, InferenceError> {
    execution.validate_for(request)?;
    let request_bytes = to_canonical_json(request)?;
    let artifact_bytes = to_canonical_json(&execution.artifact)?;
    let request_artifact_ref = store.put(
        ArtifactKind::InferenceRequest,
        JSON_MEDIA_TYPE,
        &request_bytes,
        created_at,
    )?;
    let raw_response_ref = store.put(
        ArtifactKind::InferenceResponse,
        &execution.raw_response_media_type,
        &execution.raw_response,
        created_at,
    )?;
    let validated_inference_ref = store.put(
        ArtifactKind::ValidatedInference,
        JSON_MEDIA_TYPE,
        &artifact_bytes,
        created_at,
    )?;
    if raw_response_ref != execution.artifact.raw_response_hash {
        return Err(InferenceError::PersistedRawResponseMismatch {
            persisted: raw_response_ref,
            expected: execution.artifact.raw_response_hash.clone(),
        });
    }
    let refs = PersistedInferenceRefs {
        request_artifact_ref,
        raw_response_ref,
        validated_inference_ref,
    };
    refs.validate()?;
    Ok(refs)
}

// ============================================================================ provenance

/// Materializes the deterministic `llm_inference` `DerivationRecord` of one acquisition.
///
/// `input_refs` is the sorted unique union of the request's input and evidence refs, the
/// request ID and the three persisted refs; `output_refs` must be unique and is stored sorted.
/// The ID is `drv:<first 16 hex of SHA-256(RFC 8785 JSON of the record without id)>`.
pub fn materialize_derivation_record(
    request: &InferenceRequest,
    execution: &ProviderExecution,
    persisted: &PersistedInferenceRefs,
    mut output_refs: Vec<String>,
    created_at: Timestamp,
) -> Result<DerivationRecord, InferenceError> {
    execution.validate_for(request)?;
    persisted.validate()?;
    if persisted.raw_response_ref != execution.artifact.raw_response_hash {
        return Err(InferenceError::PersistedRawResponseMismatch {
            persisted: persisted.raw_response_ref.clone(),
            expected: execution.artifact.raw_response_hash.clone(),
        });
    }
    output_refs.sort();
    if let Some(pair) = output_refs.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(InferenceError::DuplicateOutputRef(pair[0].clone()));
    }
    let mut input_refs: Vec<String> = request
        .input_refs
        .iter()
        .chain(&request.evidence_refs)
        .map(Id::to_string)
        .chain(
            [
                &request.id,
                &persisted.request_artifact_ref,
                &persisted.raw_response_ref,
                &persisted.validated_inference_ref,
            ]
            .into_iter()
            .map(Hash::to_string),
        )
        .collect();
    input_refs.sort();
    input_refs.dedup();

    let artifact = &execution.artifact;
    let mut record = DerivationRecord {
        // Placeholder replaced below; the identity input excludes `id`.
        id: "drv:0000000000000000".parse()?,
        kind: DerivationKind::LlmInference,
        stage: request.stage.as_str().to_owned(),
        input_refs,
        output_refs,
        created_at,
        provider: Some(artifact.provider.clone()),
        model: Some(artifact.model.clone()),
        prompt_template_hash: Some(request.prompt_template_hash.clone()),
        schema_hash: Some(request.schema_hash.clone()),
        context_hash: Some(request.context_hash.clone()),
        parameters: Some(artifact.parameters.as_value().clone()),
        raw_response_hash: Some(artifact.raw_response_hash.clone()),
        validated_output_hash: Some(artifact.validated_output_hash.clone()),
    };
    let mut body =
        serde_json::to_value(&record).map_err(|e| CoreError::Canonicalization(e.to_string()))?;
    if let Value::Object(map) = &mut body {
        map.remove("id");
    }
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    record.id = format!("drv:{}", &hex[..16]).parse()?;
    record.validate()?;
    Ok(record)
}
