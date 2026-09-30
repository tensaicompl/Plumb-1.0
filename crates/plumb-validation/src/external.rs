//! The canonical external-validation contracts: the deterministic request for an external
//! validator and its replayable result (compiler architecture §§5.5, 28).
//!
//! An external validator runs outside rule evaluation. Its persisted result is an input to
//! deterministic evaluation, never something the evaluator executes.

use plumb_core::{canonical_hash, CanonicalJson, CoreError, Hash, HashKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

/// Why an external-validation contract is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExternalValidationError {
    #[error("invalid external validation request: {0}")]
    InvalidRequest(String),
    #[error("invalid external validation artifact: {0}")]
    InvalidArtifact(String),
}

fn require_kind(what: &str, hash: &Hash, kind: HashKind) -> Result<(), String> {
    if hash.kind() == kind {
        Ok(())
    } else {
        Err(format!(
            "{what} must be a {kind:?} hash, got {:?}",
            hash.kind()
        ))
    }
}

/// Non-empty, no leading/trailing whitespace, no control characters.
fn require_text(what: &str, value: &str) -> Result<(), String> {
    if !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control) {
        Ok(())
    } else {
        Err(format!("invalid {what} {value:?}"))
    }
}

/// `^[a-z][a-z0-9._-]*$`.
fn require_identifier(what: &str, value: &str) -> Result<(), String> {
    let mut bytes = value.bytes();
    let valid = bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(format!("invalid {what} {value:?}"))
    }
}

/// Requires `hashes` to be strictly increasing (sorted, no duplicates).
fn require_strictly_sorted(what: &str, hashes: &[Hash]) -> Result<(), String> {
    for pair in hashes.windows(2) {
        if pair[0] == pair[1] {
            return Err(format!("duplicate {what} {}", pair[0]));
        }
        if pair[0] > pair[1] {
            return Err(format!("{what} entries are not sorted"));
        }
    }
    Ok(())
}

/// A deterministic request for an external validator (for example an API-spec linter).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ExternalValidationRequestFields")]
pub struct ExternalValidationRequest {
    pub id: Hash,
    pub validator: String,
    pub task_kind: String,
    pub input_artifact_refs: Vec<Hash>,
    pub config: CanonicalJson,
}

impl ExternalValidationRequest {
    /// Builds a request with `input_artifact_refs` sorted and `id` computed.
    pub fn new(
        validator: String,
        task_kind: String,
        mut input_artifact_refs: Vec<Hash>,
        config: CanonicalJson,
    ) -> Result<ExternalValidationRequest, ExternalValidationError> {
        input_artifact_refs.sort();
        let mut request = ExternalValidationRequest {
            // Placeholder replaced below; identity never includes `id`.
            id: Hash::content_sha256(b""),
            validator,
            task_kind,
            input_artifact_refs,
            config,
        };
        request.id = request
            .recompute_id()
            .map_err(|e| ExternalValidationError::InvalidRequest(e.to_string()))?;
        request.validate()?;
        Ok(request)
    }

    /// Exactly the fields that determine the request ID.
    pub fn identity_projection(&self) -> Value {
        json!({
            "validator": self.validator,
            "task_kind": self.task_kind,
            "input_artifact_refs": self.input_artifact_refs,
            "config": self.config,
        })
    }

    pub fn recompute_id(&self) -> Result<Hash, CoreError> {
        canonical_hash(HashKind::Generic, &self.identity_projection())
    }

    pub fn validate(&self) -> Result<(), ExternalValidationError> {
        let invalid = ExternalValidationError::InvalidRequest;
        require_kind("id", &self.id, HashKind::Generic).map_err(invalid)?;
        require_identifier("validator", &self.validator).map_err(invalid)?;
        require_text("task_kind", &self.task_kind).map_err(invalid)?;
        for r in &self.input_artifact_refs {
            require_kind("input_artifact_ref", r, HashKind::Generic).map_err(invalid)?;
        }
        require_strictly_sorted("input_artifact_ref", &self.input_artifact_refs)
            .map_err(invalid)?;
        if !self.config.as_value().is_object() {
            return Err(invalid("config must be a JSON object".into()));
        }
        let recomputed = self.recompute_id().map_err(|e| invalid(e.to_string()))?;
        if recomputed != self.id {
            return Err(invalid(format!(
                "id {} does not match recomputed id {recomputed}",
                self.id
            )));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalValidationRequestFields {
    id: Hash,
    validator: String,
    task_kind: String,
    input_artifact_refs: Vec<Hash>,
    config: CanonicalJson,
}

impl TryFrom<ExternalValidationRequestFields> for ExternalValidationRequest {
    type Error = ExternalValidationError;

    fn try_from(f: ExternalValidationRequestFields) -> Result<Self, Self::Error> {
        let request = ExternalValidationRequest {
            id: f.id,
            validator: f.validator,
            task_kind: f.task_kind,
            input_artifact_refs: f.input_artifact_refs,
            config: f.config,
        };
        request.validate()?;
        Ok(request)
    }
}

/// The replayable result of one external validation, supplied to deterministic evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ExternalValidationArtifactFields")]
pub struct ExternalValidationArtifact {
    pub request_hash: Hash,
    pub validator: String,
    pub validated_output: CanonicalJson,
    pub validated_output_hash: Hash,
}

impl ExternalValidationArtifact {
    /// Generic SHA-256 of the canonical JSON of the complete artifact: its content hash when
    /// persisted as `external-validation` / `application/json`.
    pub fn artifact_hash(&self) -> Result<Hash, CoreError> {
        canonical_hash(HashKind::Generic, self)
    }

    pub fn validate(&self) -> Result<(), ExternalValidationError> {
        let invalid = ExternalValidationError::InvalidArtifact;
        require_kind("request_hash", &self.request_hash, HashKind::Generic).map_err(invalid)?;
        require_identifier("validator", &self.validator).map_err(invalid)?;
        require_kind(
            "validated_output_hash",
            &self.validated_output_hash,
            HashKind::Generic,
        )
        .map_err(invalid)?;
        let computed = self
            .validated_output
            .content_hash()
            .map_err(|e| invalid(e.to_string()))?;
        if computed != self.validated_output_hash {
            return Err(invalid(format!(
                "validated_output_hash {} does not match output content hash {computed}",
                self.validated_output_hash
            )));
        }
        Ok(())
    }

    pub fn validate_for(
        &self,
        request: &ExternalValidationRequest,
    ) -> Result<(), ExternalValidationError> {
        request.validate()?;
        self.validate()?;
        let invalid = ExternalValidationError::InvalidArtifact;
        if self.request_hash != request.id {
            return Err(invalid(format!(
                "request_hash {} does not match request id {}",
                self.request_hash, request.id
            )));
        }
        if self.validator != request.validator {
            return Err(invalid(format!(
                "validator {:?} does not match request validator {:?}",
                self.validator, request.validator
            )));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalValidationArtifactFields {
    request_hash: Hash,
    validator: String,
    validated_output: CanonicalJson,
    validated_output_hash: Hash,
}

impl TryFrom<ExternalValidationArtifactFields> for ExternalValidationArtifact {
    type Error = ExternalValidationError;

    fn try_from(f: ExternalValidationArtifactFields) -> Result<Self, Self::Error> {
        let artifact = ExternalValidationArtifact {
            request_hash: f.request_hash,
            validator: f.validator,
            validated_output: f.validated_output,
            validated_output_hash: f.validated_output_hash,
        };
        artifact.validate()?;
        Ok(artifact)
    }
}
