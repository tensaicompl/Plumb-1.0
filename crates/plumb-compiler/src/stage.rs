//! The pure PLAN/EVALUATE stage boundary and its deterministic contracts (compiler
//! architecture §§3, 28).

use std::collections::BTreeSet;

use plumb_artifacts::{Artifact, ArtifactKind, ArtifactStoreError};
use plumb_core::{to_canonical_json, CanonicalJson, CoreError, Hash, HashKind, StageId};
use plumb_inference::{InferenceArtifact, InferenceRequest};
use plumb_patch::{PatchSet, Proposal};
use plumb_psg::Graph;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::context::{require_kind, require_text, CompileContext, Scope};

/// Media type of every JSON compiler artifact.
pub const JSON_MEDIA_TYPE: &str = "application/json";

/// A compiler stage. PLAN and EVALUATE are pure: the trait exposes no provider, artifact or
/// revision store, branch, clock, network client or commit capability.
pub trait CompilerStage {
    fn id(&self) -> StageId;

    fn plan(&self, graph: &Graph, ctx: &CompileContext) -> Result<StagePlan, CompilerError>;

    fn evaluate(
        &self,
        graph: &Graph,
        ctx: &CompileContext,
        plan: &StagePlan,
        artifacts: &ArtifactSet,
    ) -> Result<StageEvaluation, CompilerError>;
}

/// Why a compiler contract is invalid or a stage failed.
#[derive(Debug, Error)]
pub enum CompilerError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Artifact(#[from] ArtifactStoreError),

    #[error("invalid compile context: {0}")]
    InvalidContext(String),
    #[error("invalid scope: {0}")]
    InvalidScope(String),
    #[error("invalid planned artifact: {0}")]
    InvalidPlannedArtifact(String),
    #[error("invalid external validation request: {0}")]
    InvalidExternalValidationRequest(String),
    #[error("invalid external validation artifact: {0}")]
    InvalidExternalValidationArtifact(String),
    #[error("invalid artifact input: {0}")]
    InvalidArtifactInput(String),
    #[error("invalid artifact set: {0}")]
    InvalidArtifactSet(String),
    #[error("invalid stage plan: {0}")]
    InvalidStagePlan(String),
    #[error("invalid stage evaluation: {0}")]
    InvalidStageEvaluation(String),

    #[error("invalid compile run: {0}")]
    InvalidCompileRun(String),

    /// A stage-specific operational or domain failure.
    #[error("stage {stage} failed ({code}): {message}")]
    StageFailure {
        stage: StageId,
        code: String,
        message: String,
    },
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

fn require_media_type(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(char::is_control) {
        Err(format!("invalid media type {value:?}"))
    } else {
        Ok(())
    }
}

/// Requires `keys` to be strictly increasing (sorted, no duplicates).
fn require_strictly_sorted<K: Ord + std::fmt::Display>(
    what: &str,
    keys: &[K],
) -> Result<(), String> {
    for pair in keys.windows(2) {
        if pair[0] == pair[1] {
            return Err(format!("duplicate {what} {}", pair[0]));
        }
        if pair[0] > pair[1] {
            return Err(format!("{what} entries are not sorted"));
        }
    }
    Ok(())
}

/// Sorts `items` by `key` and rejects duplicate keys.
fn sort_unique<T, K: Ord + Clone + std::fmt::Display>(
    what: &str,
    items: &mut [T],
    key: impl Fn(&T) -> K,
) -> Result<(), String> {
    items.sort_by_key(|item| key(item));
    let keys: Vec<K> = items.iter().map(key).collect();
    require_strictly_sorted(what, &keys)
}

fn canonical_hash_of<T: Serialize>(value: &T) -> Result<Hash, CoreError> {
    Ok(Hash::content_sha256(&to_canonical_json(value)?))
}

// ============================================================================ PlannedArtifact

/// A deterministic artifact produced by PLAN. It has no timestamp: acquisition persists it
/// later with an explicitly supplied one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PlannedArtifactFields")]
pub struct PlannedArtifact {
    pub kind: ArtifactKind,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

impl PlannedArtifact {
    /// Generic SHA-256 of the exact bytes.
    pub fn content_hash(&self) -> Hash {
        Hash::content_sha256(&self.bytes)
    }

    pub fn validate(&self) -> Result<(), CompilerError> {
        require_media_type(&self.media_type).map_err(CompilerError::InvalidPlannedArtifact)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlannedArtifactFields {
    kind: ArtifactKind,
    media_type: String,
    bytes: Vec<u8>,
}

impl TryFrom<PlannedArtifactFields> for PlannedArtifact {
    type Error = CompilerError;

    fn try_from(f: PlannedArtifactFields) -> Result<Self, Self::Error> {
        let artifact = PlannedArtifact {
            kind: f.kind,
            media_type: f.media_type,
            bytes: f.bytes,
        };
        artifact.validate()?;
        Ok(artifact)
    }
}

// ============================================================================ external validation

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
    ) -> Result<ExternalValidationRequest, CompilerError> {
        input_artifact_refs.sort();
        let mut request = ExternalValidationRequest {
            // Placeholder replaced below; identity never includes `id`.
            id: Hash::content_sha256(b""),
            validator,
            task_kind,
            input_artifact_refs,
            config,
        };
        request.id = request.recompute_id()?;
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
        canonical_hash_of(&self.identity_projection())
    }

    pub fn validate(&self) -> Result<(), CompilerError> {
        let invalid = CompilerError::InvalidExternalValidationRequest;
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
        let recomputed = self.recompute_id()?;
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
    type Error = CompilerError;

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

/// The replayable result of one external validation, supplied to EVALUATE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ExternalValidationArtifactFields")]
pub struct ExternalValidationArtifact {
    pub request_hash: Hash,
    pub validator: String,
    pub validated_output: CanonicalJson,
    pub validated_output_hash: Hash,
}

impl ExternalValidationArtifact {
    pub fn validate(&self) -> Result<(), CompilerError> {
        let invalid = CompilerError::InvalidExternalValidationArtifact;
        require_kind("request_hash", &self.request_hash, HashKind::Generic).map_err(invalid)?;
        require_identifier("validator", &self.validator).map_err(invalid)?;
        require_kind(
            "validated_output_hash",
            &self.validated_output_hash,
            HashKind::Generic,
        )
        .map_err(invalid)?;
        let computed = self.validated_output.content_hash()?;
        if computed != self.validated_output_hash {
            return Err(invalid(format!(
                "validated_output_hash {} does not match output content hash {computed}",
                self.validated_output_hash
            )));
        }
        Ok(())
    }

    pub fn validate_for(&self, request: &ExternalValidationRequest) -> Result<(), CompilerError> {
        request.validate()?;
        self.validate()?;
        let invalid = CompilerError::InvalidExternalValidationArtifact;
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
    type Error = CompilerError;

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

// ============================================================================ ArtifactInput / ArtifactSet

/// An acquired artifact as seen by EVALUATE: no `created_at`, so acquisition time is
/// unobservable to stage evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactInput {
    pub hash: Hash,
    pub kind: ArtifactKind,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

impl ArtifactInput {
    pub fn validate(&self) -> Result<(), CompilerError> {
        let invalid = CompilerError::InvalidArtifactInput;
        require_kind("hash", &self.hash, HashKind::Generic).map_err(invalid)?;
        if Hash::content_sha256(&self.bytes) != self.hash {
            return Err(invalid(format!(
                "hash {} does not match the bytes",
                self.hash
            )));
        }
        require_media_type(&self.media_type).map_err(invalid)
    }
}

impl From<&Artifact> for ArtifactInput {
    /// Deliberately drops `created_at`.
    fn from(artifact: &Artifact) -> Self {
        ArtifactInput {
            hash: artifact.hash.clone(),
            kind: artifact.kind,
            media_type: artifact.media_type.clone(),
            bytes: artifact.bytes.clone(),
        }
    }
}

/// The exact artifacts acquired for one `StagePlan`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSet {
    pub deterministic_artifacts: Vec<ArtifactInput>,
    pub inference_artifacts: Vec<ArtifactInput>,
    pub validation_artifacts: Vec<ArtifactInput>,
}

impl ArtifactSet {
    /// Builds a set with each list sorted by hash.
    pub fn new(
        mut deterministic_artifacts: Vec<ArtifactInput>,
        mut inference_artifacts: Vec<ArtifactInput>,
        mut validation_artifacts: Vec<ArtifactInput>,
    ) -> Result<ArtifactSet, CompilerError> {
        for list in [
            &mut deterministic_artifacts,
            &mut inference_artifacts,
            &mut validation_artifacts,
        ] {
            list.sort_by(|a, b| a.hash.cmp(&b.hash));
        }
        let set = ArtifactSet {
            deterministic_artifacts,
            inference_artifacts,
            validation_artifacts,
        };
        set.validate()?;
        Ok(set)
    }

    /// All hashes of the set, deterministic, inference and validation lists in that order.
    pub fn hashes(&self) -> impl Iterator<Item = &Hash> {
        self.deterministic_artifacts
            .iter()
            .chain(&self.inference_artifacts)
            .chain(&self.validation_artifacts)
            .map(|a| &a.hash)
    }

    pub fn validate(&self) -> Result<(), CompilerError> {
        let invalid = CompilerError::InvalidArtifactSet;
        for (what, list) in [
            ("deterministic artifact", &self.deterministic_artifacts),
            ("inference artifact", &self.inference_artifacts),
            ("validation artifact", &self.validation_artifacts),
        ] {
            for input in list {
                input.validate()?;
            }
            let hashes: Vec<&Hash> = list.iter().map(|a| &a.hash).collect();
            require_strictly_sorted(what, &hashes).map_err(invalid)?;
        }
        let mut seen = BTreeSet::new();
        if let Some(dup) = self.hashes().find(|h| !seen.insert(*h)) {
            return Err(invalid(format!("hash {dup} occurs in more than one list")));
        }
        for (what, list, kind) in [
            (
                "inference",
                &self.inference_artifacts,
                ArtifactKind::ValidatedInference,
            ),
            (
                "validation",
                &self.validation_artifacts,
                ArtifactKind::ExternalValidation,
            ),
        ] {
            if let Some(bad) = list
                .iter()
                .find(|a| a.kind != kind || a.media_type != JSON_MEDIA_TYPE)
            {
                return Err(invalid(format!(
                    "{what} input {} is {} ({}), expected {kind} ({JSON_MEDIA_TYPE})",
                    bad.hash, bad.kind, bad.media_type
                )));
            }
        }
        Ok(())
    }

    /// Requires a one-to-one match between the plan and the acquired inputs.
    pub fn validate_for_plan(&self, plan: &StagePlan) -> Result<(), CompilerError> {
        self.validate()?;
        let invalid = CompilerError::InvalidArtifactSet;

        if self.deterministic_artifacts.len() != plan.deterministic_artifacts.len() {
            return Err(invalid(format!(
                "{} deterministic inputs for {} planned artifacts",
                self.deterministic_artifacts.len(),
                plan.deterministic_artifacts.len()
            )));
        }
        for planned in &plan.deterministic_artifacts {
            let hash = planned.content_hash();
            let matches = self
                .deterministic_artifacts
                .iter()
                .filter(|a| {
                    a.hash == hash
                        && a.kind == planned.kind
                        && a.media_type == planned.media_type
                        && a.bytes == planned.bytes
                })
                .count();
            if matches != 1 {
                return Err(invalid(format!(
                    "planned artifact {hash} has no exact input"
                )));
            }
        }

        if self.inference_artifacts.len() != plan.inference_requests.len() {
            return Err(invalid(format!(
                "{} inference inputs for {} inference requests",
                self.inference_artifacts.len(),
                plan.inference_requests.len()
            )));
        }
        let inference: Vec<InferenceArtifact> = self
            .inference_artifacts
            .iter()
            .map(|input| canonical_json_input(input, "InferenceArtifact"))
            .collect::<Result<_, _>>()?;
        for request in &plan.inference_requests {
            let found: Vec<&InferenceArtifact> = inference
                .iter()
                .filter(|a| a.request_hash == request.id)
                .collect();
            let [artifact] = found.as_slice() else {
                return Err(invalid(format!(
                    "inference request {} has {} results",
                    request.id,
                    found.len()
                )));
            };
            artifact
                .validate_for(request)
                .map_err(|e| invalid(format!("inference result for {}: {e}", request.id)))?;
        }

        if self.validation_artifacts.len() != plan.external_validation_requests.len() {
            return Err(invalid(format!(
                "{} validation inputs for {} validation requests",
                self.validation_artifacts.len(),
                plan.external_validation_requests.len()
            )));
        }
        let validation: Vec<ExternalValidationArtifact> = self
            .validation_artifacts
            .iter()
            .map(|input| canonical_json_input(input, "ExternalValidationArtifact"))
            .collect::<Result<_, _>>()?;
        for request in &plan.external_validation_requests {
            let found: Vec<&ExternalValidationArtifact> = validation
                .iter()
                .filter(|a| a.request_hash == request.id)
                .collect();
            let [artifact] = found.as_slice() else {
                return Err(invalid(format!(
                    "validation request {} has {} results",
                    request.id,
                    found.len()
                )));
            };
            artifact
                .validate_for(request)
                .map_err(|e| invalid(format!("validation result for {}: {e}", request.id)))?;
        }
        Ok(())
    }
}

/// Parses `input` as the canonical JSON of a `T`.
fn canonical_json_input<T: serde::de::DeserializeOwned + Serialize>(
    input: &ArtifactInput,
    what: &str,
) -> Result<T, CompilerError> {
    let invalid = CompilerError::InvalidArtifactSet;
    let value: T = serde_json::from_slice(&input.bytes)
        .map_err(|e| invalid(format!("input {} is not a {what}: {e}", input.hash)))?;
    if to_canonical_json(&value)? != input.bytes {
        return Err(invalid(format!(
            "input {} is not canonical {what} JSON",
            input.hash
        )));
    }
    Ok(value)
}

// ============================================================================ StagePlan

/// What a stage needs: scope, deterministic artifacts and inference/validation requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StagePlanFields")]
pub struct StagePlan {
    pub scope: Scope,

    pub deterministic_artifacts: Vec<PlannedArtifact>,
    pub inference_requests: Vec<InferenceRequest>,
    pub external_validation_requests: Vec<ExternalValidationRequest>,
}

impl StagePlan {
    /// Builds a plan in canonical order, rejecting duplicates.
    pub fn new(
        scope: Scope,
        mut deterministic_artifacts: Vec<PlannedArtifact>,
        mut inference_requests: Vec<InferenceRequest>,
        mut external_validation_requests: Vec<ExternalValidationRequest>,
    ) -> Result<StagePlan, CompilerError> {
        let invalid = CompilerError::InvalidStagePlan;
        sort_unique(
            "planned artifact",
            &mut deterministic_artifacts,
            PlannedArtifact::content_hash,
        )
        .map_err(invalid)?;
        sort_unique("inference request", &mut inference_requests, |r| {
            r.id.clone()
        })
        .map_err(invalid)?;
        sort_unique(
            "external validation request",
            &mut external_validation_requests,
            |r| r.id.clone(),
        )
        .map_err(invalid)?;
        let plan = StagePlan {
            scope,
            deterministic_artifacts,
            inference_requests,
            external_validation_requests,
        };
        plan.validate_structure()?;
        Ok(plan)
    }

    /// Nested contracts are valid and every list is in canonical order without duplicates.
    pub fn validate_structure(&self) -> Result<(), CompilerError> {
        let invalid = CompilerError::InvalidStagePlan;
        self.scope.validate()?;
        for artifact in &self.deterministic_artifacts {
            artifact.validate()?;
        }
        for request in &self.inference_requests {
            request
                .validate()
                .map_err(|e| invalid(format!("inference request {}: {e}", request.id)))?;
        }
        for request in &self.external_validation_requests {
            request.validate()?;
        }
        let artifact_hashes: Vec<Hash> = self
            .deterministic_artifacts
            .iter()
            .map(PlannedArtifact::content_hash)
            .collect();
        require_strictly_sorted("planned artifact", &artifact_hashes).map_err(invalid)?;
        let inference_ids: Vec<&Hash> = self.inference_requests.iter().map(|r| &r.id).collect();
        require_strictly_sorted("inference request", &inference_ids).map_err(invalid)?;
        let validation_ids: Vec<&Hash> = self
            .external_validation_requests
            .iter()
            .map(|r| &r.id)
            .collect();
        require_strictly_sorted("external validation request", &validation_ids).map_err(invalid)
    }

    /// Also requires the scope to resolve in `graph` and every inference request to belong to
    /// `stage`.
    pub fn validate(&self, stage: StageId, graph: &Graph) -> Result<(), CompilerError> {
        self.validate_structure()?;
        self.scope.validate_for_graph(graph)?;
        if let Some(request) = self.inference_requests.iter().find(|r| r.stage != stage) {
            return Err(CompilerError::InvalidStagePlan(format!(
                "inference request {} is for stage {}, not {stage}",
                request.id, request.stage
            )));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StagePlanFields {
    scope: Scope,
    deterministic_artifacts: Vec<PlannedArtifact>,
    inference_requests: Vec<InferenceRequest>,
    external_validation_requests: Vec<ExternalValidationRequest>,
}

impl TryFrom<StagePlanFields> for StagePlan {
    type Error = CompilerError;

    fn try_from(f: StagePlanFields) -> Result<Self, Self::Error> {
        let plan = StagePlan {
            scope: f.scope,
            deterministic_artifacts: f.deterministic_artifacts,
            inference_requests: f.inference_requests,
            external_validation_requests: f.external_validation_requests,
        };
        plan.validate_structure()?;
        Ok(plan)
    }
}

// ============================================================================ StageEvaluation

/// Stage output: an optional deterministic derivation patch set and proposals. Findings and
/// questions are PSG nodes inside these patch sets; impact and intake are derived later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StageEvaluationFields")]
pub struct StageEvaluation {
    pub derivation_patch_set: Option<PatchSet>,
    pub proposals: Vec<Proposal>,
}

impl StageEvaluation {
    /// Builds an evaluation with proposals sorted by ID, rejecting duplicates.
    pub fn new(
        derivation_patch_set: Option<PatchSet>,
        mut proposals: Vec<Proposal>,
    ) -> Result<StageEvaluation, CompilerError> {
        sort_unique("proposal", &mut proposals, |p| p.id.clone())
            .map_err(CompilerError::InvalidStageEvaluation)?;
        let evaluation = StageEvaluation {
            derivation_patch_set,
            proposals,
        };
        evaluation.validate_structure()?;
        Ok(evaluation)
    }

    /// Every proposal validates and proposals are sorted by ID without duplicates.
    pub fn validate_structure(&self) -> Result<(), CompilerError> {
        let invalid = CompilerError::InvalidStageEvaluation;
        if let Some(patch_set) = &self.derivation_patch_set {
            require_kind(
                "derivation base_semantic_hash",
                &patch_set.base_semantic_hash,
                HashKind::Semantic,
            )
            .map_err(invalid)?;
        }
        for proposal in &self.proposals {
            proposal
                .validate()
                .map_err(|e| invalid(format!("proposal {}: {e}", proposal.id)))?;
        }
        let ids: Vec<&plumb_core::Id> = self.proposals.iter().map(|p| &p.id).collect();
        require_strictly_sorted("proposal", &ids).map_err(invalid)
    }

    /// Also requires every patch set to be based on `input_semantic_hash` and every proposal to
    /// belong to `stage`.
    pub fn validate(
        &self,
        stage: StageId,
        input_semantic_hash: &Hash,
    ) -> Result<(), CompilerError> {
        self.validate_structure()?;
        let invalid = CompilerError::InvalidStageEvaluation;
        if let Some(patch_set) = &self.derivation_patch_set {
            if &patch_set.base_semantic_hash != input_semantic_hash {
                return Err(invalid(
                    "derivation patch set is not based on the input semantic hash".into(),
                ));
            }
        }
        for proposal in &self.proposals {
            if proposal.stage != stage {
                return Err(invalid(format!(
                    "proposal {} is for stage {}, not {stage}",
                    proposal.id, proposal.stage
                )));
            }
            if &proposal.patch_set.base_semantic_hash != input_semantic_hash {
                return Err(invalid(format!(
                    "proposal {} is not based on the input semantic hash",
                    proposal.id
                )));
            }
        }
        Ok(())
    }

    /// Generic SHA-256 of the canonical evaluation (`CompileRun.output_hash`).
    pub fn output_hash(&self) -> Result<Hash, CoreError> {
        canonical_hash_of(self)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StageEvaluationFields {
    derivation_patch_set: Option<PatchSet>,
    proposals: Vec<Proposal>,
}

impl TryFrom<StageEvaluationFields> for StageEvaluation {
    type Error = CompilerError;

    fn try_from(f: StageEvaluationFields) -> Result<Self, Self::Error> {
        let evaluation = StageEvaluation {
            derivation_patch_set: f.derivation_patch_set,
            proposals: f.proposals,
        };
        evaluation.validate_structure()?;
        Ok(evaluation)
    }
}
