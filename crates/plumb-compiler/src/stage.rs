//! The pure PLAN/EVALUATE stage boundary and its deterministic contracts (compiler
//! architecture §§3, 28).

use std::collections::BTreeSet;

use plumb_artifacts::{Artifact, ArtifactKind, ArtifactStoreError};
use plumb_core::{to_canonical_json, CoreError, Hash, HashKind, StageId};
use plumb_inference::{InferenceArtifact, InferenceRequest, InferenceRequestMaterial};
use plumb_patch::{PatchSet, Proposal};
use plumb_psg::Graph;
use plumb_validation::ExternalValidationError;
pub use plumb_validation::{ExternalValidationArtifact, ExternalValidationRequest};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::context::{require_kind, CompileContext, Scope};

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

impl From<ExternalValidationError> for CompilerError {
    fn from(error: ExternalValidationError) -> Self {
        match error {
            ExternalValidationError::InvalidRequest(reason) => {
                CompilerError::InvalidExternalValidationRequest(reason)
            }
            ExternalValidationError::InvalidArtifact(reason) => {
                CompilerError::InvalidExternalValidationArtifact(reason)
            }
        }
    }
}

/// Fable #97: the require_text rule (non-empty, no surrounding whitespace or control
/// characters) and a `type/subtype` shape of RFC 6838 restricted names, optionally followed by
/// `; name=value` parameters.
fn require_media_type(value: &str) -> Result<(), String> {
    fn restricted_name(part: &str) -> bool {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "!#$&-^_.+".contains(c))
    }
    let invalid = || format!("invalid media type {value:?}");
    crate::context::require_text("media type", value).map_err(|_| invalid())?;
    let mut sections = value.split(';');
    let essence = sections.next().unwrap_or_default();
    let shaped = matches!(
        essence.split('/').collect::<Vec<_>>().as_slice(),
        [kind, subtype] if restricted_name(kind) && restricted_name(subtype)
    );
    let parameters_ok = sections.all(|parameter| {
        matches!(
            parameter.trim_start().split_once('='),
            Some((name, value)) if restricted_name(name) && !value.is_empty()
        )
    });
    if shaped && parameters_ok {
        Ok(())
    } else {
        Err(invalid())
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
///
/// R0.2 (architecture §20): `declared_absent_inference_requests` holds the sorted, unique IDs of
/// the planned `Fallback` inference requests deliberately not executed under the `null`
/// provider mode. No artifact exists for them; absence is never an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSet {
    pub deterministic_artifacts: Vec<ArtifactInput>,
    pub inference_artifacts: Vec<ArtifactInput>,
    pub validation_artifacts: Vec<ArtifactInput>,
    pub declared_absent_inference_requests: Vec<Hash>,
}

impl ArtifactSet {
    /// Builds a set with each list sorted by hash and no declared absence.
    pub fn new(
        deterministic_artifacts: Vec<ArtifactInput>,
        inference_artifacts: Vec<ArtifactInput>,
        validation_artifacts: Vec<ArtifactInput>,
    ) -> Result<ArtifactSet, CompilerError> {
        ArtifactSet::with_declared_absence(
            deterministic_artifacts,
            inference_artifacts,
            validation_artifacts,
            Vec::new(),
        )
    }

    /// Builds a set with each list sorted by hash and the given declared-absent inference
    /// request IDs (sorted; duplicates are rejected).
    pub fn with_declared_absence(
        mut deterministic_artifacts: Vec<ArtifactInput>,
        mut inference_artifacts: Vec<ArtifactInput>,
        mut validation_artifacts: Vec<ArtifactInput>,
        mut declared_absent_inference_requests: Vec<Hash>,
    ) -> Result<ArtifactSet, CompilerError> {
        for list in [
            &mut deterministic_artifacts,
            &mut inference_artifacts,
            &mut validation_artifacts,
        ] {
            list.sort_by(|a, b| a.hash.cmp(&b.hash));
        }
        declared_absent_inference_requests.sort();
        let set = ArtifactSet {
            deterministic_artifacts,
            inference_artifacts,
            validation_artifacts,
            declared_absent_inference_requests,
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
        for id in &self.declared_absent_inference_requests {
            require_kind("declared absent inference request", id, HashKind::Generic)
                .map_err(invalid)?;
        }
        require_strictly_sorted(
            "declared absent inference request",
            &self.declared_absent_inference_requests,
        )
        .map_err(invalid)?;
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

    /// Requires a one-to-one match between the plan and the acquired inputs. Each planned
    /// inference request has exactly one matching acquired result and is not declared absent,
    /// or (R0.2) has no result, is declared absent exactly once and has the `Fallback` policy.
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

        if let Some(unknown) = self
            .declared_absent_inference_requests
            .iter()
            .find(|id| !plan.inference_requests.iter().any(|r| &r.request.id == *id))
        {
            return Err(invalid(format!(
                "declared absent inference request {unknown} is not planned"
            )));
        }
        let inference: Vec<InferenceArtifact> = self
            .inference_artifacts
            .iter()
            .map(|input| canonical_json_input(input, "InferenceArtifact"))
            .collect::<Result<_, _>>()?;
        if let Some(both) = inference
            .iter()
            .find(|a| self.is_declared_absent(&a.request_hash))
        {
            return Err(invalid(format!(
                "inference request {} is both acquired and declared absent",
                both.request_hash
            )));
        }
        let absent = self.declared_absent_inference_requests.len();
        if self.inference_artifacts.len() + absent != plan.inference_requests.len() {
            return Err(invalid(format!(
                "{} inference inputs and {absent} declared absences for {} inference requests",
                self.inference_artifacts.len(),
                plan.inference_requests.len()
            )));
        }
        for planned in &plan.inference_requests {
            let request = &planned.request;
            let found: Vec<&InferenceArtifact> = inference
                .iter()
                .filter(|a| a.request_hash == request.id)
                .collect();
            if self.is_declared_absent(&request.id) {
                if planned.absence_policy != InferenceAbsencePolicy::Fallback {
                    return Err(invalid(format!(
                        "required inference request {} is declared absent",
                        request.id
                    )));
                }
                continue;
            }
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

impl ArtifactSet {
    /// Whether `request` is declared absent (R0.2).
    pub fn is_declared_absent(&self, request: &Hash) -> bool {
        self.declared_absent_inference_requests
            .binary_search(request)
            .is_ok()
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

// ============================================================================ PlannedInferenceRequest

/// What a planned inference request means when no provider is configured (R0.2, architecture
/// §20 "Provider optionality and declared fallback"). PLAN declares it; the provider never
/// chooses it and it is not part of the request identity or material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InferenceAbsencePolicy {
    /// The phase cannot run without the inference (`E_INFERENCE_REQUIRED` under `null`).
    Required,
    /// The consumer has a declared no-inference behavior; under `null` the request is
    /// recorded as declared absent and never executed.
    Fallback,
}

/// A planned inference request with the exact material it binds (Hotfix 066). PLAN emits both;
/// ACQUIRE hands both to the provider; EVALUATE still receives only acquired `ArtifactInput`s.
/// The material is acquisition input, not PSG state and not a second identity: the request ID
/// remains the only inference identity. R0.2 adds the declared `absence_policy`, which is plan
/// semantics only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PlannedInferenceRequestFields")]
pub struct PlannedInferenceRequest {
    pub request: InferenceRequest,
    pub material: InferenceRequestMaterial,
    pub absence_policy: InferenceAbsencePolicy,
}

impl PlannedInferenceRequest {
    /// Builds a planned request whose material binds the request, with its declared absence
    /// policy.
    pub fn new(
        request: InferenceRequest,
        material: InferenceRequestMaterial,
        absence_policy: InferenceAbsencePolicy,
    ) -> Result<PlannedInferenceRequest, CompilerError> {
        let planned = PlannedInferenceRequest {
            request,
            material,
            absence_policy,
        };
        planned.validate()?;
        Ok(planned)
    }

    /// The request validates and the material binds its ID, context, prompt and schema hashes.
    pub fn validate(&self) -> Result<(), CompilerError> {
        let invalid = CompilerError::InvalidStagePlan;
        self.request
            .validate()
            .map_err(|e| invalid(format!("inference request {}: {e}", self.request.id)))?;
        self.material
            .validate_for(&self.request)
            .map_err(|e| invalid(format!("inference material for {}: {e}", self.request.id)))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlannedInferenceRequestFields {
    request: InferenceRequest,
    material: InferenceRequestMaterial,
    absence_policy: InferenceAbsencePolicy,
}

impl TryFrom<PlannedInferenceRequestFields> for PlannedInferenceRequest {
    type Error = CompilerError;

    fn try_from(f: PlannedInferenceRequestFields) -> Result<Self, Self::Error> {
        PlannedInferenceRequest::new(f.request, f.material, f.absence_policy)
    }
}

// ============================================================================ StagePlan

/// What a stage needs: scope, deterministic artifacts and inference/validation requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StagePlanFields")]
pub struct StagePlan {
    pub scope: Scope,

    pub deterministic_artifacts: Vec<PlannedArtifact>,
    pub inference_requests: Vec<PlannedInferenceRequest>,
    pub external_validation_requests: Vec<ExternalValidationRequest>,
}

impl StagePlan {
    /// Builds a plan in canonical order, rejecting duplicates.
    pub fn new(
        scope: Scope,
        mut deterministic_artifacts: Vec<PlannedArtifact>,
        mut inference_requests: Vec<PlannedInferenceRequest>,
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
            r.request.id.clone()
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
        for planned in &self.inference_requests {
            planned.validate()?;
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
        let inference_ids: Vec<&Hash> = self
            .inference_requests
            .iter()
            .map(|r| &r.request.id)
            .collect();
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
        if let Some(request) = self
            .inference_requests
            .iter()
            .map(|r| &r.request)
            .find(|r| r.stage != stage)
        {
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
    inference_requests: Vec<PlannedInferenceRequest>,
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
