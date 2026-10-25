//! The inference provider boundary (compiler architecture §§19-20).
//!
//! A provider receives one request together with its exact, hash-bound
//! [`InferenceRequestMaterial`] (Hotfix 066): the canonical context, the exact prompt template
//! and the exact schema source whose hashes the request carries. It has no graph, revision,
//! store, branch, commit or artifact-store capability: it cannot mutate PSG or persist
//! artifacts, and it never needs future request material installed at construction.

use std::collections::BTreeMap;

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, Hash, HashKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::model::{InferenceError, InferenceRequest, ProviderExecution};

/// Executes one inference request with its exact material and returns the execution for
/// acquisition to persist. Every implementation validates the request/material binding
/// ([`validate_binding`]) before any provider-specific work.
pub trait InferenceProvider {
    fn execute(
        &self,
        request: &InferenceRequest,
        material: &InferenceRequestMaterial,
    ) -> Result<ProviderExecution, ProviderError>;
}

/// Validates `request` and that `material` belongs to it exactly (request ID, context, prompt
/// and schema hashes, canonical context, UTF-8 prompt, compilable schema). No I/O.
pub fn validate_binding(
    request: &InferenceRequest,
    material: &InferenceRequestMaterial,
) -> Result<(), ProviderError> {
    verify_binding(request, material).map(|_| ())
}

pub(crate) fn verify_binding(
    request: &InferenceRequest,
    material: &InferenceRequestMaterial,
) -> Result<VerifiedMaterial, ProviderError> {
    request.validate().map_err(ProviderError::InvalidRequest)?;
    material.verify(request)
}

/// Why a provider could not produce an execution.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// Inference is disabled (the null provider).
    #[error("inference provider is disabled")]
    ProviderDisabled,
    /// The request itself is invalid.
    #[error("invalid inference request: {0}")]
    InvalidRequest(InferenceError),
    /// No replay fixture is registered for this request ID.
    #[error("no fixture registered for request {0}")]
    FixtureNotFound(Hash),
    /// A fixture key is not a generic `sha256:` hash.
    #[error("fixture key {0} is not a generic sha256: hash")]
    FixtureKeyNotGeneric(Hash),
    /// A fixture's artifact belongs to a different request than its key.
    #[error("fixture key {key} does not match artifact request_hash {request_hash}")]
    FixtureRequestMismatch { key: Hash, request_hash: Hash },
    /// A fixture is already registered for this request ID.
    #[error("a fixture is already registered for request {0}")]
    DuplicateFixture(Hash),
    /// A fixture violates the execution or request invariants.
    #[error("invalid fixture: {0}")]
    InvalidFixture(InferenceError),
    /// The request's policy names another provider.
    #[error("provider {provider:?} is not supported by this provider")]
    UnsupportedProvider { provider: String },
    /// The provider policy configuration violates the adapter contract.
    #[error("invalid provider config: {reason}")]
    InvalidProviderConfig { reason: String },
    /// The credential is empty, padded or contains control characters (never shown).
    #[error("invalid provider secret")]
    InvalidSecret,
    /// The endpoint is not an http(s) URL without embedded credentials.
    #[error("invalid provider endpoint: {reason}")]
    InvalidEndpoint { reason: String },
    /// No request material is registered for this request ID.
    #[error("no request material registered for request {0}")]
    MaterialNotFound(Hash),
    /// Request material is already registered for this request ID.
    #[error("request material is already registered for request {0}")]
    DuplicateMaterial(Hash),
    /// The material names another request (or a non-generic hash).
    #[error("request material {material} does not belong to request {request}")]
    MaterialRequestMismatch { material: Hash, request: Hash },
    /// Material bytes do not hash to the request's hash for that field.
    #[error("request material {field} does not match the request hash")]
    MaterialHashMismatch { field: &'static str },
    /// The context bytes are not RFC 8785 canonical JSON.
    #[error("request material context is not canonical JSON")]
    NonCanonicalContext,
    /// The prompt template is not valid UTF-8.
    #[error("request material prompt template is not valid UTF-8")]
    InvalidPromptEncoding,
    /// The schema source is not a compilable Draft 2020-12 JSON Schema.
    #[error("request material schema is invalid: {reason}")]
    InvalidSchema { reason: String },
    /// The request could not be sent or the response could not be received.
    #[error("provider transport failure: {reason}")]
    Transport { reason: String },
    /// The configured connect or total request timeout elapsed (distinct from other transport
    /// failures: the generation may have been billed).
    #[error("provider request timed out: {reason}")]
    Timeout { reason: String },
    /// The provider answered with a non-success HTTP status.
    #[error("provider returned HTTP {status} ({error_type:?}): {message:?}")]
    Remote {
        status: u16,
        error_type: Option<String>,
        message: Option<String>,
        request_id: Option<String>,
    },
    /// A success response does not have the expected shape.
    #[error("malformed provider response: {reason}")]
    MalformedResponse { reason: String },
    /// The model stopped for a reason with no defined success semantics.
    #[error("unexpected stop reason {stop_reason:?}")]
    UnexpectedStopReason { stop_reason: String },
    /// The model refused the request.
    #[error("the model refused the request")]
    Refusal,
    /// The output was truncated by the token limit.
    #[error("the model output is incomplete (max_tokens)")]
    IncompleteOutput,
    /// The output text is not exactly one JSON value.
    #[error("model output is not JSON: {reason}")]
    InvalidOutputJson { reason: String },
    /// The output JSON violates the exact request schema.
    #[error("model output violates the request schema: {errors:?}")]
    SchemaInvalidOutput { errors: Vec<String> },
    /// The constructed execution fails its own invariants.
    #[error("invalid provider execution: {0}")]
    InvalidProviderExecution(InferenceError),
}

// ============================================================================ request material

/// The exact deterministic acquisition material of one request, bound to it by hashes. It is
/// not accepted PSG state, cannot mutate a graph and is not a second request identity: it is
/// identified only by the request ID it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceRequestMaterial {
    pub request_hash: Hash,
    /// RFC 8785 canonical JSON of the stage context.
    pub context_json: Vec<u8>,
    /// The exact committed prompt-template bytes.
    pub prompt_template: Vec<u8>,
    /// The exact committed schema-source bytes.
    pub schema_json: Vec<u8>,
}

/// Material verified against its request: parsed context, prompt text and compiled schema.
#[cfg_attr(not(feature = "anthropic"), allow(dead_code))]
pub(crate) struct VerifiedMaterial {
    pub(crate) context: Value,
    pub(crate) prompt: String,
    pub(crate) schema: Value,
    pub(crate) compiled: JSONSchema,
}

impl VerifiedMaterial {
    /// Validates `output` against the compiled task output schema; every violation is listed
    /// (sorted) in [`ProviderError::SchemaInvalidOutput`].
    pub(crate) fn validate_output(&self, output: &Value) -> Result<(), ProviderError> {
        if let Err(errors) = self.compiled.validate(output) {
            let mut errors: Vec<String> = errors.map(|e| e.to_string()).collect();
            errors.sort();
            return Err(ProviderError::SchemaInvalidOutput { errors });
        }
        Ok(())
    }
}

impl InferenceRequestMaterial {
    /// The material of `request` from its exact context, prompt and schema bytes; it is returned
    /// only when it already satisfies [`InferenceRequestMaterial::validate_for`].
    pub fn new(
        request: &InferenceRequest,
        context_json: Vec<u8>,
        prompt_template: Vec<u8>,
        schema_json: Vec<u8>,
    ) -> Result<InferenceRequestMaterial, ProviderError> {
        let material = InferenceRequestMaterial {
            request_hash: request.id.clone(),
            context_json,
            prompt_template,
            schema_json,
        };
        material.validate_for(request)?;
        Ok(material)
    }

    /// Deterministically verifies the material against `request`; no network, files or
    /// environment.
    pub fn validate_for(&self, request: &InferenceRequest) -> Result<(), ProviderError> {
        self.verify(request).map(|_| ())
    }

    pub(crate) fn verify(
        &self,
        request: &InferenceRequest,
    ) -> Result<VerifiedMaterial, ProviderError> {
        if self.request_hash.kind() != HashKind::Generic || self.request_hash != request.id {
            return Err(ProviderError::MaterialRequestMismatch {
                material: self.request_hash.clone(),
                request: request.id.clone(),
            });
        }
        let checks: [(&'static str, &[u8], &Hash); 3] = [
            ("context_json", &self.context_json, &request.context_hash),
            (
                "prompt_template",
                &self.prompt_template,
                &request.prompt_template_hash,
            ),
            ("schema_json", &self.schema_json, &request.schema_hash),
        ];
        for (field, bytes, expected) in checks {
            if &Hash::content_sha256(bytes) != expected {
                return Err(ProviderError::MaterialHashMismatch { field });
            }
        }
        let context: Value = serde_json::from_slice(&self.context_json)
            .map_err(|_| ProviderError::NonCanonicalContext)?;
        let canonical =
            to_canonical_json(&context).map_err(|_| ProviderError::NonCanonicalContext)?;
        if canonical != self.context_json {
            return Err(ProviderError::NonCanonicalContext);
        }
        let prompt = String::from_utf8(self.prompt_template.clone())
            .map_err(|_| ProviderError::InvalidPromptEncoding)?;
        let schema: Value = serde_json::from_slice(&self.schema_json).map_err(|e| {
            ProviderError::InvalidSchema {
                reason: format!("not JSON: {e}"),
            }
        })?;
        let compiled = JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&schema)
            .map_err(|e| ProviderError::InvalidSchema {
                reason: e.to_string(),
            })?;
        Ok(VerifiedMaterial {
            context,
            prompt,
            schema,
            compiled,
        })
    }
}

/// Immutable in-memory request material keyed by request ID: a utility collection; no provider
/// looks material up here (Hotfix 066).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InferenceRequestMaterialSet {
    materials: BTreeMap<Hash, InferenceRequestMaterial>,
}

impl InferenceRequestMaterialSet {
    /// An empty set.
    pub fn new() -> InferenceRequestMaterialSet {
        InferenceRequestMaterialSet::default()
    }

    /// A set of `materials`; a repeated request hash is an error.
    pub fn from_materials(
        materials: impl IntoIterator<Item = InferenceRequestMaterial>,
    ) -> Result<InferenceRequestMaterialSet, ProviderError> {
        let mut set = InferenceRequestMaterialSet::new();
        for material in materials {
            set.register(material)?;
        }
        Ok(set)
    }

    /// Adds material; never overwrites an existing request hash.
    pub fn register(&mut self, material: InferenceRequestMaterial) -> Result<(), ProviderError> {
        if self.materials.contains_key(&material.request_hash) {
            return Err(ProviderError::DuplicateMaterial(material.request_hash));
        }
        self.materials
            .insert(material.request_hash.clone(), material);
        Ok(())
    }

    /// The material of a request ID.
    pub fn get(&self, request_hash: &Hash) -> Option<&InferenceRequestMaterial> {
        self.materials.get(request_hash)
    }

    /// The number of registered materials.
    pub fn len(&self) -> usize {
        self.materials.len()
    }

    /// Whether no material is registered.
    pub fn is_empty(&self) -> bool {
        self.materials.is_empty()
    }
}
