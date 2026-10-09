//! The deterministic gate evaluator framework: policy, context, the rule evaluation contract,
//! evaluator registration, gate aggregation and the gate report (compiler architecture §29,
//! rulebook §§2, 5, 7).
//!
//! The framework is pure. It reads a graph and already-acquired external validation results,
//! runs the evaluators bound to one gate and returns a report. It mutates nothing, persists
//! nothing, runs no external validator and observes no time.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use plumb_artifacts::{Artifact, ArtifactKind};
use plumb_core::{canonical_hash, CoreError, GateId, Hash, HashKind, Id};
use plumb_lint::{LintInput, LintPolicy, LintTextRange};
use plumb_psg::{AgentKind, ElementStatus, Graph, NodePayload, NodeType};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::calculation_analysis::{calculation_origin_of, CalculationScope};
use crate::decision_table_analysis::DecisionTableSpec;
use crate::expression_scope::{validate_expression_bindings, ExpressionScopeBinding};
use crate::external::{ExternalValidationArtifact, ExternalValidationError};
use crate::finding::{finding_id, finding_key, is_clean_text, GeneratedFinding, ViolationFacts};
use crate::model::{
    RuleMetadata, RuleResultState, Severity, UnknownVocabularyValue, ValidationError, WaiverPolicy,
};
use crate::registry::ValidationRegistry;
use crate::waiver::{governed_waiver, AppliedWaiver};

/// Why the framework cannot produce a trustworthy gate report.
///
/// This is not a rule outcome: a rule that cannot be evaluated reports an
/// [`EvaluatorFailure`], which becomes an `ERROR` result inside a valid report.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EvaluationError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Metadata(#[from] ValidationError),
    #[error(transparent)]
    ExternalValidation(#[from] ExternalValidationError),

    #[error("invalid validation context: {0}")]
    InvalidContext(String),
    #[error("invalid validation policy: {0}")]
    InvalidPolicy(String),

    #[error("no rule {0} exists to bind an evaluator to")]
    UnknownEvaluatorRule(String),
    #[error("rule {rule_id} belongs to gate {gate}, which is not in the current scope")]
    EvaluatorForDeferredRule { rule_id: String, gate: GateId },
    #[error("rule {0} already has an evaluator")]
    DuplicateEvaluator(String),
    #[error("gate {gate} has rules without an evaluator: {rule_ids:?}")]
    MissingGateEvaluators { gate: GateId, rule_ids: Vec<String> },

    #[error("invalid evaluator output: {0}")]
    InvalidEvaluatorOutput(String),
    #[error("invalid semantic condition key {0:?}")]
    InvalidSemanticConditionKey(String),

    #[error("invalid finding key input: {0}")]
    InvalidFindingKey(String),
    #[error("invalid finding id: {0}")]
    InvalidFindingId(String),

    /// The node at a deterministic Finding ID is not the evaluated finding.
    #[error("finding {finding_ref} does not match the evaluated finding: {reason}")]
    FindingIdentityMismatch { finding_ref: Id, reason: String },
    #[error("waiver decision {decision} is malformed: {reason}")]
    MalformedWaiverDecision { decision: Id, reason: String },
    #[error("rule {rule_id} forbids waivers, but decision {decision} waives it")]
    ForbiddenWaiver { rule_id: String, decision: Id },
    #[error("decision {decision} waives rule {rule_id}, whose profile waiver is not enabled")]
    ProfileWaiverNotEnabled { rule_id: String, decision: Id },
    #[error("finding {finding_ref} of rule {rule_id} has several waiver decisions: {decisions:?}")]
    AmbiguousWaiver {
        rule_id: String,
        finding_ref: Id,
        decisions: Vec<Id>,
    },

    #[error("invalid gate report: {0}")]
    GateReportInvalid(String),
}

// ============================================================================ ValidationPolicy

/// Project-specific validation choices the profile YAML does not encode: which rules are
/// promoted to blocker and which `profile_allow` rules may be waived.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationPolicy {
    pub promoted_to_blocker_rule_ids: BTreeSet<String>,
    pub profile_allow_waiver_rule_ids: BTreeSet<String>,
}

impl ValidationPolicy {
    /// Every ID names a rule of `registry`, and only `profile_allow` rules are waiver-enabled.
    pub fn validate(&self, registry: &ValidationRegistry) -> Result<(), EvaluationError> {
        let invalid = EvaluationError::InvalidPolicy;
        for id in &self.promoted_to_blocker_rule_ids {
            if registry.rule(id).is_none() {
                return Err(invalid(format!("promoted rule {id:?} does not exist")));
            }
        }
        for id in &self.profile_allow_waiver_rule_ids {
            let Some(rule) = registry.rule(id) else {
                return Err(invalid(format!(
                    "waiver-enabled rule {id:?} does not exist"
                )));
            };
            if rule.waiver_policy != WaiverPolicy::ProfileAllow {
                return Err(invalid(format!(
                    "rule {id} has waiver policy {}, not profile_allow",
                    rule.waiver_policy
                )));
            }
        }
        Ok(())
    }

    /// The rule's declared severity, or blocker when the policy promotes the rule.
    pub fn effective_severity(&self, rule: &RuleMetadata) -> Severity {
        if self.promoted_to_blocker_rule_ids.contains(&rule.id) {
            Severity::Blocker
        } else {
            rule.severity
        }
    }

    /// Generic hash of the canonical policy.
    pub fn policy_hash(&self) -> Result<Hash, CoreError> {
        canonical_hash(HashKind::Generic, self)
    }
}

// ============================================================================ ValidationArtifactInput

/// An already-acquired immutable evidence artifact supplied to evaluation. It has no
/// `created_at`: acquisition time is unobservable to rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "ValidationArtifactInputFields")]
pub struct ValidationArtifactInput {
    pub hash: Hash,
    pub kind: ArtifactKind,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidationArtifactInputFields {
    hash: Hash,
    kind: ArtifactKind,
    media_type: String,
    bytes: Vec<u8>,
}

impl TryFrom<ValidationArtifactInputFields> for ValidationArtifactInput {
    type Error = EvaluationError;

    fn try_from(f: ValidationArtifactInputFields) -> Result<Self, Self::Error> {
        let input = ValidationArtifactInput {
            hash: f.hash,
            kind: f.kind,
            media_type: f.media_type,
            bytes: f.bytes,
        };
        input.validate()?;
        Ok(input)
    }
}

/// The artifact kinds evidence evaluation currently reads.
pub const EVIDENCE_ARTIFACT_KINDS: [ArtifactKind; 3] = [
    ArtifactKind::SourceOriginal,
    ArtifactKind::SourceExtracted,
    ArtifactKind::EvidenceManifest,
];

impl ValidationArtifactInput {
    /// The hash is Generic and is the SHA-256 of the exact bytes; the media type is clean.
    pub fn validate(&self) -> Result<(), EvaluationError> {
        let invalid = EvaluationError::InvalidContext;
        if self.hash.kind() != HashKind::Generic {
            return Err(invalid(format!(
                "evidence artifact {} is not a Generic hash",
                self.hash
            )));
        }
        if Hash::content_sha256(&self.bytes) != self.hash {
            return Err(invalid(format!(
                "evidence artifact {} does not match its bytes",
                self.hash
            )));
        }
        if !is_clean_text(&self.media_type) {
            return Err(invalid(format!(
                "evidence artifact {} has invalid media type {:?}",
                self.hash, self.media_type
            )));
        }
        Ok(())
    }
}

impl From<&Artifact> for ValidationArtifactInput {
    /// Copies hash, kind, media type and bytes; deliberately drops `created_at`.
    fn from(artifact: &Artifact) -> Self {
        ValidationArtifactInput {
            hash: artifact.hash.clone(),
            kind: artifact.kind,
            media_type: artifact.media_type.clone(),
            bytes: artifact.bytes.clone(),
        }
    }
}

/// Validates evidence inputs: each valid, of an evidence kind, strictly sorted by hash.
fn validate_evidence_artifacts(inputs: &[ValidationArtifactInput]) -> Result<(), EvaluationError> {
    for input in inputs {
        input.validate()?;
        if !EVIDENCE_ARTIFACT_KINDS.contains(&input.kind) {
            return Err(EvaluationError::InvalidContext(format!(
                "evidence artifact {} has kind {}, which evidence evaluation does not read",
                input.hash, input.kind
            )));
        }
    }
    let refs: Vec<&Hash> = inputs.iter().map(|i| &i.hash).collect();
    require_strictly_sorted("evidence artifact", &refs).map_err(EvaluationError::InvalidContext)
}

fn require_evidence_hash(hash: &Hash) -> Result<(), EvaluationError> {
    if hash.kind() == HashKind::Evidence {
        Ok(())
    } else {
        Err(EvaluationError::InvalidContext(format!(
            "baseline_evidence_hash {hash} is not an Evidence hash"
        )))
    }
}

// ============================================================================ ValidationContext

/// Everything an evaluation depends on besides the graph and the rule metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationContext {
    pub baseline_semantic_hash: Hash,
    /// The expected evidence baseline, supplied by orchestration. Whether the graph reproduces
    /// it is a rule result, not a context invariant.
    pub baseline_evidence_hash: Hash,

    pub profile_id: Id,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,

    pub policy: ValidationPolicy,

    pub external_validation_artifacts: Vec<ExternalValidationArtifact>,

    /// Already-acquired evidence artifacts, sorted by hash and unique.
    pub evidence_artifacts: Vec<ValidationArtifactInput>,

    /// The deterministic F1 supplemental inputs; required by F1 and ignored by other gates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub f1_inputs: Option<F1ValidationInputs>,
    /// The deterministic F2 supplemental inputs; required by the F2 rules that need them and
    /// ignored by other gates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub f2_inputs: Option<F2ValidationInputs>,
}

impl ValidationContext {
    /// Binds a context to `graph` and `registry`. The artifacts are validated, stored in
    /// hash order and must be distinct. Missing evidence a rule needs is that rule's violation,
    /// not a context error.
    pub fn new(
        graph: &Graph,
        registry: &ValidationRegistry,
        policy: ValidationPolicy,
        baseline_evidence_hash: Hash,
        mut evidence_artifacts: Vec<ValidationArtifactInput>,
        external_validation_artifacts: Vec<ExternalValidationArtifact>,
    ) -> Result<ValidationContext, EvaluationError> {
        require_graph_profile(graph, registry)?;
        policy.validate(registry)?;
        require_evidence_hash(&baseline_evidence_hash)?;
        evidence_artifacts.sort_by(|a, b| a.hash.cmp(&b.hash));
        validate_evidence_artifacts(&evidence_artifacts)?;
        let mut keyed: Vec<(Hash, ExternalValidationArtifact)> = Vec::new();
        for artifact in external_validation_artifacts {
            artifact.validate()?;
            keyed.push((artifact.artifact_hash()?, artifact));
        }
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(pair) = keyed.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(EvaluationError::InvalidContext(format!(
                "duplicate external validation artifact {}",
                pair[0].0
            )));
        }
        let profile = registry.profile();
        Ok(ValidationContext {
            baseline_semantic_hash: graph.semantic_hash()?,
            baseline_evidence_hash,
            profile_id: profile.profile_id.clone(),
            profile_hash: profile.profile_hash()?,
            rule_pack_hash: profile.rule_pack_hash()?,
            policy,
            external_validation_artifacts: keyed.into_iter().map(|(_, a)| a).collect(),
            evidence_artifacts,
            f1_inputs: None,
            f2_inputs: None,
        })
    }

    /// Validates `inputs` against `graph` and installs them as the F1 supplemental inputs.
    pub fn with_f1_inputs(
        self,
        graph: &Graph,
        inputs: F1ValidationInputs,
    ) -> Result<ValidationContext, EvaluationError> {
        inputs.validate_for(graph)?;
        Ok(ValidationContext {
            f1_inputs: Some(inputs),
            ..self
        })
    }

    /// Validates `inputs` against `graph` and installs them as the F2 supplemental inputs.
    pub fn with_f2_inputs(
        self,
        graph: &Graph,
        inputs: F2ValidationInputs,
    ) -> Result<ValidationContext, EvaluationError> {
        inputs.validate_for(graph)?;
        Ok(ValidationContext {
            f2_inputs: Some(inputs),
            ..self
        })
    }

    /// The evidence artifact with `hash`, if supplied. No I/O.
    pub fn evidence_artifact(&self, hash: &Hash) -> Option<&ValidationArtifactInput> {
        self.evidence_artifacts
            .binary_search_by(|input| input.hash.cmp(hash))
            .ok()
            .map(|index| &self.evidence_artifacts[index])
    }

    /// The hashes of the evidence artifacts, in canonical hash order.
    pub fn evidence_artifact_refs(&self) -> Vec<Hash> {
        self.evidence_artifacts
            .iter()
            .map(|input| input.hash.clone())
            .collect()
    }

    /// The Generic hashes of the external validation artifacts, in stored order.
    pub fn validation_artifact_refs(&self) -> Result<Vec<Hash>, CoreError> {
        self.external_validation_artifacts
            .iter()
            .map(ExternalValidationArtifact::artifact_hash)
            .collect()
    }

    /// Requires this context to describe exactly `graph` and `registry`.
    pub fn validate_for(
        &self,
        graph: &Graph,
        registry: &ValidationRegistry,
    ) -> Result<(), EvaluationError> {
        let invalid = EvaluationError::InvalidContext;
        require_graph_profile(graph, registry)?;
        require_evidence_hash(&self.baseline_evidence_hash)?;
        validate_evidence_artifacts(&self.evidence_artifacts)?;
        if self.baseline_semantic_hash != graph.semantic_hash()? {
            return Err(invalid(
                "baseline_semantic_hash is not the graph's semantic hash".into(),
            ));
        }
        let profile = registry.profile();
        if self.profile_id != profile.profile_id {
            return Err(invalid(format!(
                "profile_id {} is not the registry profile {}",
                self.profile_id, profile.profile_id
            )));
        }
        if self.profile_hash != profile.profile_hash()? {
            return Err(invalid(
                "profile_hash is not the registry profile's hash".into(),
            ));
        }
        if self.rule_pack_hash != profile.rule_pack_hash()? {
            return Err(invalid(
                "rule_pack_hash is not the registry profile's rule-pack hash".into(),
            ));
        }
        self.policy.validate(registry)?;
        for artifact in &self.external_validation_artifacts {
            artifact.validate()?;
        }
        require_strictly_sorted(
            "external validation artifact",
            &self.validation_artifact_refs()?,
        )
        .map_err(invalid)?;
        if let Some(inputs) = &self.f1_inputs {
            inputs.validate_for(graph)?;
        }
        if let Some(inputs) = &self.f2_inputs {
            inputs.validate_for(graph)?;
        }
        Ok(())
    }
}

/// The evaluated graph must declare the profile whose metadata drives the evaluation.
fn require_graph_profile(
    graph: &Graph,
    registry: &ValidationRegistry,
) -> Result<(), EvaluationError> {
    let profile_id = &registry.profile().profile_id;
    if graph.profile_id() == profile_id {
        Ok(())
    } else {
        Err(EvaluationError::InvalidContext(format!(
            "graph profile {} is not the registry profile {profile_id}",
            graph.profile_id()
        )))
    }
}

// ============================================================================ evaluator contract

/// Whether a rule applied to the evaluated graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", try_from = "BTreeMap<String, String>")]
pub enum Applicability {
    #[serde(rename = "APPLICABLE")]
    Applicable,
    #[serde(rename = "NOT_APPLICABLE")]
    NotApplicable { reason: String },
}

impl TryFrom<BTreeMap<String, String>> for Applicability {
    type Error = EvaluationError;

    /// Exactly `{"state":"APPLICABLE"}` or `{"state":"NOT_APPLICABLE","reason":"..."}`.
    fn try_from(mut fields: BTreeMap<String, String>) -> Result<Self, Self::Error> {
        let state = fields.remove("state");
        let reason = fields.remove("reason");
        let applicability = match (state.as_deref(), reason, fields.is_empty()) {
            (Some("APPLICABLE"), None, true) => Applicability::Applicable,
            (Some("NOT_APPLICABLE"), Some(reason), true) => Applicability::NotApplicable { reason },
            _ => {
                return Err(EvaluationError::InvalidEvaluatorOutput(
                    "malformed applicability".into(),
                ))
            }
        };
        applicability.validate()?;
        Ok(applicability)
    }
}

impl Applicability {
    /// A not-applicable outcome with a recorded, deterministic reason.
    pub fn not_applicable(reason: String) -> Result<Applicability, EvaluationError> {
        let applicability = Applicability::NotApplicable { reason };
        applicability.validate()?;
        Ok(applicability)
    }

    pub fn validate(&self) -> Result<(), EvaluationError> {
        match self {
            Applicability::Applicable => Ok(()),
            Applicability::NotApplicable { reason } => require_clean("reason", reason),
        }
    }
}

/// What one evaluator found. The engine, not the evaluator, derives the final result state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleEvaluation {
    Pass {
        targets: Vec<Id>,
        evidence: Vec<String>,
    },

    Violation {
        targets: Vec<Id>,
        evidence: Vec<String>,
        semantic_condition_key: String,
        message: String,
        suggested_resolution: Option<String>,
    },

    NotApplicable {
        reason: String,
    },
}

impl RuleEvaluation {
    /// A passing outcome; targets and evidence are sorted and must be distinct.
    pub fn pass(
        mut targets: Vec<Id>,
        mut evidence: Vec<String>,
    ) -> Result<RuleEvaluation, EvaluationError> {
        targets.sort();
        evidence.sort();
        let evaluation = RuleEvaluation::Pass { targets, evidence };
        evaluation.validate()?;
        Ok(evaluation)
    }

    /// A violated condition; targets and evidence are sorted and must be distinct.
    pub fn violation(
        mut targets: Vec<Id>,
        mut evidence: Vec<String>,
        semantic_condition_key: String,
        message: String,
        suggested_resolution: Option<String>,
    ) -> Result<RuleEvaluation, EvaluationError> {
        targets.sort();
        evidence.sort();
        let evaluation = RuleEvaluation::Violation {
            targets,
            evidence,
            semantic_condition_key,
            message,
            suggested_resolution,
        };
        evaluation.validate()?;
        Ok(evaluation)
    }

    /// The rule does not apply, for the recorded deterministic reason.
    pub fn not_applicable(reason: String) -> Result<RuleEvaluation, EvaluationError> {
        let evaluation = RuleEvaluation::NotApplicable { reason };
        evaluation.validate()?;
        Ok(evaluation)
    }

    /// The canonical-form check the engine applies to every evaluator output.
    pub fn validate(&self) -> Result<(), EvaluationError> {
        match self {
            RuleEvaluation::Pass { targets, evidence } => {
                require_canonical_collections(targets, evidence)
            }
            RuleEvaluation::Violation {
                targets,
                evidence,
                semantic_condition_key,
                message,
                ..
            } => {
                require_canonical_collections(targets, evidence)?;
                if !is_clean_text(semantic_condition_key) {
                    return Err(EvaluationError::InvalidSemanticConditionKey(
                        semantic_condition_key.clone(),
                    ));
                }
                if message.is_empty() {
                    return Err(EvaluationError::InvalidEvaluatorOutput(
                        "violation message is empty".into(),
                    ));
                }
                Ok(())
            }
            RuleEvaluation::NotApplicable { reason } => require_clean("reason", reason),
        }
    }
}

/// An evaluator could not evaluate its rule. The rule result becomes `ERROR`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluatorFailure {
    pub code: String,
    pub message: String,
    pub targets: Vec<Id>,
    pub evidence: Vec<String>,
}

impl EvaluatorFailure {
    /// A failure; targets and evidence are sorted and must be distinct.
    pub fn new(
        code: String,
        message: String,
        mut targets: Vec<Id>,
        mut evidence: Vec<String>,
    ) -> Result<EvaluatorFailure, EvaluationError> {
        targets.sort();
        evidence.sort();
        let failure = EvaluatorFailure {
            code,
            message,
            targets,
            evidence,
        };
        failure.validate()?;
        Ok(failure)
    }

    pub fn validate(&self) -> Result<(), EvaluationError> {
        require_clean("failure code", &self.code)?;
        if self.message.is_empty() {
            return Err(EvaluationError::InvalidEvaluatorOutput(
                "failure message is empty".into(),
            ));
        }
        require_canonical_collections(&self.targets, &self.evidence)
    }
}

/// A rule evaluator: a pure function of the graph, the context and the rule's own metadata.
pub type RuleEvaluator =
    fn(&Graph, &ValidationContext, &RuleMetadata) -> Result<RuleEvaluation, EvaluatorFailure>;

/// The result state of a violation that no waiver covers.
pub fn violation_state(effective_severity: Severity) -> RuleResultState {
    match effective_severity {
        Severity::Blocker | Severity::Error => RuleResultState::Fail,
        Severity::Warn | Severity::Info => RuleResultState::Warn,
    }
}

fn require_clean(what: &str, text: &str) -> Result<(), EvaluationError> {
    if is_clean_text(text) {
        Ok(())
    } else {
        Err(EvaluationError::InvalidEvaluatorOutput(format!(
            "invalid {what} {text:?}"
        )))
    }
}

/// Requires `items` to be strictly increasing (sorted, no duplicates).
fn require_strictly_sorted<T: Ord + fmt::Display>(what: &str, items: &[T]) -> Result<(), String> {
    for pair in items.windows(2) {
        if pair[0] == pair[1] {
            return Err(format!("duplicate {what} {}", pair[0]));
        }
        if pair[0] > pair[1] {
            return Err(format!("{what} entries are not sorted"));
        }
    }
    Ok(())
}

fn require_canonical_collections(
    targets: &[Id],
    evidence: &[String],
) -> Result<(), EvaluationError> {
    let invalid = EvaluationError::InvalidEvaluatorOutput;
    require_strictly_sorted("target", targets).map_err(invalid)?;
    require_strictly_sorted("evidence", evidence).map_err(invalid)?;
    for item in evidence {
        require_clean("evidence", item)?;
    }
    Ok(())
}

// ============================================================================ RuleResult

/// The final, engine-derived result of one rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RuleResultFields")]
pub struct RuleResult {
    pub rule_id: String,

    pub state: RuleResultState,

    pub declared_severity: Severity,
    pub effective_severity: Severity,

    pub applicability: Applicability,

    pub targets: Vec<Id>,
    pub evidence: Vec<String>,

    pub semantic_condition_key: Option<String>,

    pub finding_ref: Option<Id>,
    pub waiver_ref: Option<Id>,

    pub error: Option<EvaluatorFailure>,
}

impl RuleResult {
    /// Checks the field combination each result state requires.
    pub fn validate(&self) -> Result<(), EvaluationError> {
        let invalid = |reason: &str| {
            EvaluationError::GateReportInvalid(format!("rule {}: {reason}", self.rule_id))
        };
        if !is_clean_text(&self.rule_id) {
            return Err(EvaluationError::GateReportInvalid(format!(
                "invalid rule id {:?}",
                self.rule_id
            )));
        }
        if self.effective_severity != self.declared_severity
            && self.effective_severity != Severity::Blocker
        {
            return Err(invalid(
                "effective severity is neither declared nor blocker",
            ));
        }
        self.applicability
            .validate()
            .and_then(|()| require_canonical_collections(&self.targets, &self.evidence))
            .map_err(|e| invalid(&e.to_string()))?;
        if let Some(error) = &self.error {
            error.validate().map_err(|e| invalid(&e.to_string()))?;
        }
        if self
            .semantic_condition_key
            .as_deref()
            .is_some_and(|key| !is_clean_text(key))
        {
            return Err(invalid("invalid semantic condition key"));
        }

        let applicable = self.applicability == Applicability::Applicable;
        let key = self.semantic_condition_key.is_some();
        let finding = self.finding_ref.is_some();
        let waiver = self.waiver_ref.is_some();
        let error = self.error.is_some();
        let violation_state = violation_state(self.effective_severity);
        let consistent = match self.state {
            RuleResultState::Pass => applicable && !key && !finding && !waiver && !error,
            RuleResultState::NotApplicable => {
                !applicable
                    && self.targets.is_empty()
                    && self.evidence.is_empty()
                    && !key
                    && !finding
                    && !waiver
                    && !error
            }
            RuleResultState::Fail | RuleResultState::Warn => {
                applicable && key && finding && !waiver && !error && self.state == violation_state
            }
            RuleResultState::Waived => applicable && key && finding && waiver && !error,
            RuleResultState::Error => applicable && !key && !finding && !waiver && error,
        };
        if consistent {
            Ok(())
        } else {
            Err(invalid("fields do not match the result state"))
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleResultFields {
    rule_id: String,
    state: RuleResultState,
    declared_severity: Severity,
    effective_severity: Severity,
    applicability: Applicability,
    targets: Vec<Id>,
    evidence: Vec<String>,
    semantic_condition_key: Option<String>,
    finding_ref: Option<Id>,
    waiver_ref: Option<Id>,
    error: Option<EvaluatorFailure>,
}

impl TryFrom<RuleResultFields> for RuleResult {
    type Error = EvaluationError;

    fn try_from(f: RuleResultFields) -> Result<Self, Self::Error> {
        let result = RuleResult {
            rule_id: f.rule_id,
            state: f.state,
            declared_severity: f.declared_severity,
            effective_severity: f.effective_severity,
            applicability: f.applicability,
            targets: f.targets,
            evidence: f.evidence,
            semantic_condition_key: f.semantic_condition_key,
            finding_ref: f.finding_ref,
            waiver_ref: f.waiver_ref,
            error: f.error,
        };
        result.validate()?;
        Ok(result)
    }
}

// ============================================================================ GateReport

/// The outcome of a gate. Rule-level states such as `WARN` or `WAIVED` are not gate outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GateResult {
    Pass,
    Fail,
}

impl GateResult {
    pub const ALL: [GateResult; 2] = [GateResult::Pass, GateResult::Fail];

    /// The exact wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            GateResult::Pass => "PASS",
            GateResult::Fail => "FAIL",
        }
    }
}

impl std::str::FromStr for GateResult {
    type Err = UnknownVocabularyValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GateResult::ALL
            .into_iter()
            .find(|value| value.as_str() == s)
            .ok_or_else(|| UnknownVocabularyValue {
                vocabulary: "GateResult",
                value: s.to_owned(),
            })
    }
}

impl fmt::Display for GateResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for GateResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GateResult {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// The counts that decide and explain a gate result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateSummary {
    pub blocker_failed: u32,
    pub blocker_waived: u32,
    pub warnings: u32,
}

impl GateSummary {
    /// Counts effective-blocker failures and waivers, and all warnings.
    pub fn of(rules: &[RuleResult]) -> GateSummary {
        let mut summary = GateSummary::default();
        for rule in rules {
            let blocker = rule.effective_severity == Severity::Blocker;
            match rule.state {
                RuleResultState::Fail | RuleResultState::Error if blocker => {
                    summary.blocker_failed += 1;
                }
                RuleResultState::Waived if blocker => summary.blocker_waived += 1,
                RuleResultState::Warn => summary.warnings += 1,
                _ => {}
            }
        }
        summary
    }

    /// A gate passes exactly when no effective-blocker rule failed or errored.
    pub fn result(&self) -> GateResult {
        if self.blocker_failed == 0 {
            GateResult::Pass
        } else {
            GateResult::Fail
        }
    }
}

/// The deterministic result of evaluating one gate. It carries no timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "GateReportFields")]
pub struct GateReport {
    pub gate: GateId,

    pub baseline_semantic_hash: Hash,
    pub baseline_evidence_hash: Hash,

    pub profile_id: Id,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,

    pub policy: ValidationPolicy,

    pub validation_artifact_refs: Vec<Hash>,
    pub evidence_artifact_refs: Vec<Hash>,

    pub result: GateResult,

    pub rules: Vec<RuleResult>,

    pub findings: Vec<GeneratedFinding>,
    pub waivers: Vec<AppliedWaiver>,

    pub summary: GateSummary,

    /// The hash of the F1 supplemental inputs: present exactly for F1 reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub f1_input_hash: Option<Hash>,
}

impl GateReport {
    /// Generic hash of the canonical report.
    pub fn content_hash(&self) -> Result<Hash, CoreError> {
        canonical_hash(HashKind::Generic, self)
    }

    /// Checks hash kinds, canonical ordering, and that findings, waivers, summary and result
    /// are exactly what the rule results imply.
    pub fn validate(&self) -> Result<(), EvaluationError> {
        let invalid = |reason: String| EvaluationError::GateReportInvalid(reason);
        if self.baseline_semantic_hash.kind() != HashKind::Semantic {
            return Err(invalid(
                "baseline_semantic_hash is not a Semantic hash".into(),
            ));
        }
        if self.baseline_evidence_hash.kind() != HashKind::Evidence {
            return Err(invalid(
                "baseline_evidence_hash is not an Evidence hash".into(),
            ));
        }
        require_strictly_sorted("evidence artifact ref", &self.evidence_artifact_refs)
            .map_err(invalid)?;
        for (what, hash) in [
            ("profile_hash", &self.profile_hash),
            ("rule_pack_hash", &self.rule_pack_hash),
        ]
        .into_iter()
        .chain(
            self.validation_artifact_refs
                .iter()
                .chain(&self.evidence_artifact_refs)
                .map(|h| ("artifact ref", h)),
        ) {
            if hash.kind() != HashKind::Generic {
                return Err(invalid(format!("{what} {hash} is not a Generic hash")));
            }
        }
        require_strictly_sorted("validation artifact ref", &self.validation_artifact_refs)
            .map_err(invalid)?;
        match (&self.f1_input_hash, self.gate == GateId::F1) {
            (Some(hash), true) if hash.kind() == HashKind::Generic => {}
            (None, false) => {}
            (Some(hash), true) => {
                return Err(invalid(format!(
                    "f1_input_hash {hash} is not a Generic hash"
                )))
            }
            (None, true) => return Err(invalid("an F1 report has no f1_input_hash".into())),
            (Some(_), false) => {
                return Err(invalid("only F1 reports carry an f1_input_hash".into()))
            }
        }

        for rule in &self.rules {
            rule.validate()?;
        }
        let rule_ids: Vec<&String> = self.rules.iter().map(|r| &r.rule_id).collect();
        require_strictly_sorted("rule result", &rule_ids).map_err(invalid)?;
        let finding_ids: Vec<&Id> = self.findings.iter().map(|f| &f.id).collect();
        require_strictly_sorted("finding", &finding_ids).map_err(invalid)?;
        let waiver_keys: Vec<WaiverOrder<'_>> = self.waivers.iter().map(WaiverOrder).collect();
        require_strictly_sorted("waiver", &waiver_keys).map_err(invalid)?;

        let referenced: BTreeSet<&Id> = self
            .rules
            .iter()
            .filter_map(|r| r.finding_ref.as_ref())
            .collect();
        let generated: BTreeSet<&Id> = finding_ids.into_iter().collect();
        if referenced != generated {
            return Err(invalid("findings do not match the rule results".into()));
        }
        let waived: BTreeSet<(&str, Option<&Id>, Option<&Id>)> = self
            .rules
            .iter()
            .filter(|r| r.state == RuleResultState::Waived)
            .map(|r| {
                (
                    r.rule_id.as_str(),
                    r.finding_ref.as_ref(),
                    r.waiver_ref.as_ref(),
                )
            })
            .collect();
        let applied: BTreeSet<(&str, Option<&Id>, Option<&Id>)> = self
            .waivers
            .iter()
            .map(|w| {
                (
                    w.rule_id.as_str(),
                    Some(&w.finding_ref),
                    Some(&w.decision_ref),
                )
            })
            .collect();
        if waived != applied {
            return Err(invalid("waivers do not match the rule results".into()));
        }

        if self.summary != GateSummary::of(&self.rules) {
            return Err(invalid("summary does not match the rule results".into()));
        }
        if self.result != self.summary.result() {
            return Err(invalid("result does not match the summary".into()));
        }
        Ok(())
    }
}

/// The canonical waiver order: rule ID, finding, decision.
struct WaiverOrder<'a>(&'a AppliedWaiver);

impl PartialEq for WaiverOrder<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for WaiverOrder<'_> {}

impl PartialOrd for WaiverOrder<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WaiverOrder<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}

impl WaiverOrder<'_> {
    fn key(&self) -> (&str, &Id, &Id) {
        (&self.0.rule_id, &self.0.finding_ref, &self.0.decision_ref)
    }
}

impl fmt::Display for WaiverOrder<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (rule_id, finding_ref, decision_ref) = self.key();
        write!(f, "{rule_id}/{finding_ref}/{decision_ref}")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GateReportFields {
    gate: GateId,
    baseline_semantic_hash: Hash,
    baseline_evidence_hash: Hash,
    profile_id: Id,
    profile_hash: Hash,
    rule_pack_hash: Hash,
    policy: ValidationPolicy,
    validation_artifact_refs: Vec<Hash>,
    evidence_artifact_refs: Vec<Hash>,
    result: GateResult,
    rules: Vec<RuleResult>,
    findings: Vec<GeneratedFinding>,
    waivers: Vec<AppliedWaiver>,
    summary: GateSummary,
    #[serde(default)]
    f1_input_hash: Option<Hash>,
}

impl TryFrom<GateReportFields> for GateReport {
    type Error = EvaluationError;

    fn try_from(f: GateReportFields) -> Result<Self, Self::Error> {
        let report = GateReport {
            gate: f.gate,
            baseline_semantic_hash: f.baseline_semantic_hash,
            baseline_evidence_hash: f.baseline_evidence_hash,
            profile_id: f.profile_id,
            profile_hash: f.profile_hash,
            rule_pack_hash: f.rule_pack_hash,
            policy: f.policy,
            validation_artifact_refs: f.validation_artifact_refs,
            evidence_artifact_refs: f.evidence_artifact_refs,
            result: f.result,
            rules: f.rules,
            findings: f.findings,
            waivers: f.waivers,
            summary: f.summary,
            f1_input_hash: f.f1_input_hash,
        };
        report.validate()?;
        Ok(report)
    }
}

// ============================================================================ EvaluatorRegistry

/// The execution-layer registry: the rule metadata plus the evaluators bound so far.
///
/// Binding is gate-incremental. A gate can be evaluated once every rule it owns has an
/// evaluator, whatever the state of the other gates.
#[derive(Clone)]
pub struct EvaluatorRegistry {
    metadata: ValidationRegistry,
    evaluators: BTreeMap<String, RuleEvaluator>,
}

impl fmt::Debug for EvaluatorRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvaluatorRegistry")
            .field("profile_id", &self.metadata.profile().profile_id)
            .field(
                "bound_rule_ids",
                &self.evaluators.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl EvaluatorRegistry {
    /// A registry with no evaluator bound.
    pub fn new(metadata: ValidationRegistry) -> EvaluatorRegistry {
        EvaluatorRegistry {
            metadata,
            evaluators: BTreeMap::new(),
        }
    }

    pub fn metadata(&self) -> &ValidationRegistry {
        &self.metadata
    }

    /// Binds `evaluator` to a rule of a NOW gate that has no evaluator yet.
    pub fn register(
        &mut self,
        rule_id: &str,
        evaluator: RuleEvaluator,
    ) -> Result<(), EvaluationError> {
        let Some(rule) = self.metadata.rule(rule_id) else {
            return Err(EvaluationError::UnknownEvaluatorRule(rule_id.to_owned()));
        };
        if !ValidationRegistry::is_now_gate(rule.gate) {
            return Err(EvaluationError::EvaluatorForDeferredRule {
                rule_id: rule_id.to_owned(),
                gate: rule.gate,
            });
        }
        if self.evaluators.contains_key(rule_id) {
            return Err(EvaluationError::DuplicateEvaluator(rule_id.to_owned()));
        }
        self.evaluators.insert(rule_id.to_owned(), evaluator);
        Ok(())
    }

    pub fn has_evaluator(&self, rule_id: &str) -> bool {
        self.evaluators.contains_key(rule_id)
    }

    /// The rules of `gate` that have no evaluator, in rule ID order.
    pub fn missing_for_gate(&self, gate: GateId) -> Vec<&str> {
        self.metadata
            .gate(gate)
            .rule_ids
            .iter()
            .map(String::as_str)
            .filter(|id| !self.has_evaluator(id))
            .collect()
    }

    /// Evaluates every rule of `gate` over `graph` and returns the deterministic report.
    pub fn evaluate_gate(
        &self,
        gate: GateId,
        graph: &Graph,
        ctx: &ValidationContext,
    ) -> Result<GateReport, EvaluationError> {
        ctx.validate_for(graph, &self.metadata)?;
        ValidationRegistry::require_now_gate(gate)?;
        let missing = self.missing_for_gate(gate);
        if !missing.is_empty() {
            return Err(EvaluationError::MissingGateEvaluators {
                gate,
                rule_ids: missing.into_iter().map(str::to_owned).collect(),
            });
        }
        // F1 reads its supplemental inputs; every other gate ignores them entirely.
        let f1_input_hash = if gate == GateId::F1 {
            let Some(inputs) = &ctx.f1_inputs else {
                return Err(EvaluationError::InvalidContext(
                    "F1 validation inputs are required".into(),
                ));
            };
            Some(inputs.content_hash()?)
        } else {
            None
        };

        let mut rules = Vec::new();
        let mut findings = Vec::new();
        let mut waivers = Vec::new();
        // Gate rules are normalized into lexicographic rule ID order.
        for rule in self.metadata.rules_for_gate(gate) {
            let Some(evaluator) = self.evaluators.get(&rule.id) else {
                continue;
            };
            let outcome = evaluate_rule(rule, *evaluator, graph, ctx)?;
            rules.push(outcome.result);
            findings.extend(outcome.finding);
            waivers.extend(outcome.waiver);
        }
        findings.sort_by(|a, b| a.id.cmp(&b.id));
        waivers.sort_by(|a, b| WaiverOrder(a).cmp(&WaiverOrder(b)));

        let summary = GateSummary::of(&rules);
        let report = GateReport {
            gate,
            baseline_semantic_hash: ctx.baseline_semantic_hash.clone(),
            baseline_evidence_hash: ctx.baseline_evidence_hash.clone(),
            profile_id: ctx.profile_id.clone(),
            profile_hash: ctx.profile_hash.clone(),
            rule_pack_hash: ctx.rule_pack_hash.clone(),
            policy: ctx.policy.clone(),
            validation_artifact_refs: ctx.validation_artifact_refs()?,
            evidence_artifact_refs: ctx.evidence_artifact_refs(),
            result: summary.result(),
            rules,
            findings,
            waivers,
            summary,
            f1_input_hash,
        };
        report.validate()?;
        Ok(report)
    }
}

/// One rule's result with the finding and waiver it produced.
struct RuleOutcome {
    result: RuleResult,
    finding: Option<GeneratedFinding>,
    waiver: Option<AppliedWaiver>,
}

fn evaluate_rule(
    rule: &RuleMetadata,
    evaluator: RuleEvaluator,
    graph: &Graph,
    ctx: &ValidationContext,
) -> Result<RuleOutcome, EvaluationError> {
    let malformed = |error: EvaluationError| match error {
        EvaluationError::InvalidEvaluatorOutput(reason) => {
            EvaluationError::InvalidEvaluatorOutput(format!("rule {}: {reason}", rule.id))
        }
        other => other,
    };
    let effective_severity = ctx.policy.effective_severity(rule);
    let mut result = RuleResult {
        rule_id: rule.id.clone(),
        state: RuleResultState::Pass,
        declared_severity: rule.severity,
        effective_severity,
        applicability: Applicability::Applicable,
        targets: Vec::new(),
        evidence: Vec::new(),
        semantic_condition_key: None,
        finding_ref: None,
        waiver_ref: None,
        error: None,
    };
    let mut finding = None;
    let mut waiver = None;

    match evaluator(graph, ctx, rule) {
        Err(failure) => {
            failure.validate().map_err(malformed)?;
            result.state = RuleResultState::Error;
            result.targets = failure.targets.clone();
            result.evidence = failure.evidence.clone();
            result.error = Some(failure);
        }
        Ok(evaluation) => {
            evaluation.validate().map_err(malformed)?;
            match evaluation {
                RuleEvaluation::Pass { targets, evidence } => {
                    result.targets = targets;
                    result.evidence = evidence;
                }
                RuleEvaluation::NotApplicable { reason } => {
                    result.state = RuleResultState::NotApplicable;
                    result.applicability = Applicability::NotApplicable { reason };
                }
                RuleEvaluation::Violation {
                    targets,
                    evidence,
                    semantic_condition_key,
                    message,
                    suggested_resolution,
                } => {
                    let facts = ViolationFacts {
                        targets: &targets,
                        semantic_condition_key: &semantic_condition_key,
                        message: &message,
                        suggested_resolution: suggested_resolution.as_deref(),
                    };
                    let mut generated =
                        GeneratedFinding::for_violation(rule, effective_severity, &facts, None)?;
                    waiver = governed_waiver(
                        graph,
                        rule,
                        &ctx.policy,
                        &generated.key,
                        &generated.id,
                        &generated.payload.affected_refs,
                    )?;
                    result.state = match &waiver {
                        Some(applied) => {
                            generated.payload.waiver_ref = Some(applied.decision_ref.clone());
                            result.waiver_ref = Some(applied.decision_ref.clone());
                            RuleResultState::Waived
                        }
                        None => violation_state(effective_severity),
                    };
                    result.finding_ref = Some(generated.id.clone());
                    result.semantic_condition_key = Some(semantic_condition_key);
                    result.targets = targets;
                    result.evidence = evidence;
                    finding = Some(generated);
                }
            }
        }
    }
    Ok(RuleOutcome {
        result,
        finding,
        waiver,
    })
}

// ============================================================================ F1 supplemental inputs

/// The deterministic F1 supplemental inputs compiled before validation (plan S1.6): one entry
/// per Accepted Requirement, the current upstream finding material and the lint policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct F1ValidationInputs {
    pub requirements: Vec<F1RequirementValidationInput>,
    pub analysis_findings: Vec<GeneratedFinding>,
    pub lint_policy: LintPolicy,
}

// Every field compares structurally without floating point, so equality is reflexive.
impl Eq for F1ValidationInputs {}

/// The F1 inputs of one Accepted Requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct F1RequirementValidationInput {
    pub requirement_ref: Id,
    pub vocabulary_dependencies: Vec<F1VocabularyDependency>,
    pub lint_input: LintInput,
}

/// One semantic vocabulary dependency of the current statement, resolved upstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct F1VocabularyDependency {
    pub normalized_key: String,
    pub range: LintTextRange,
    pub resolution: F1VocabularyResolution,
}

/// How a vocabulary dependency resolves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum F1VocabularyResolution {
    AcceptedTerm {
        term_ref: Id,
    },
    AcceptedConcept {
        concept_ref: Id,
    },
    ExternalIdentifier {
        decision_ref: Id,
        identifier: String,
    },
    Unresolved,
}

/// The analysis-finding codes F1 supplemental inputs may carry.
pub const F1_ANALYSIS_FINDING_CODES: [&str; 3] = [
    "PLUMB.F1.REQ.NO_CONTRADICTION",
    "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED",
    "PLUMB.F1.REQ.TERMS_RESOLVED",
];

impl F1ValidationInputs {
    /// Builds canonical inputs: requirements sorted by ID, dependencies by start, end and key,
    /// findings by ID. Validation against a graph happens in
    /// [`ValidationContext::with_f1_inputs`].
    pub fn new(
        mut requirements: Vec<F1RequirementValidationInput>,
        mut analysis_findings: Vec<GeneratedFinding>,
        lint_policy: LintPolicy,
    ) -> F1ValidationInputs {
        for requirement in &mut requirements {
            requirement
                .vocabulary_dependencies
                .sort_by(|a, b| dependency_key(a).cmp(&dependency_key(b)));
        }
        requirements.sort_by(|a, b| a.requirement_ref.cmp(&b.requirement_ref));
        analysis_findings.sort_by(|a, b| a.id.cmp(&b.id));
        F1ValidationInputs {
            requirements,
            analysis_findings,
            lint_policy,
        }
    }

    /// Generic SHA-256 of the RFC 8785 canonical JSON of the complete inputs.
    pub fn content_hash(&self) -> Result<Hash, CoreError> {
        canonical_hash(HashKind::Generic, self)
    }

    /// Requires exact Accepted Requirement coverage, current statements and evidence, valid
    /// canonical dependencies and resolutions, valid governed decisions and exact finding
    /// identities. Stale or malformed inputs are a context error, never a rule result.
    pub fn validate_for(&self, graph: &Graph) -> Result<(), EvaluationError> {
        let invalid = EvaluationError::InvalidContext;
        let markers = governed_markers(graph).map_err(invalid)?;
        let accepted: Vec<&Id> = graph
            .node_ids_by_type(NodeType::Requirement)
            .iter()
            .filter(|id| {
                graph
                    .node(id)
                    .is_some_and(|n| n.status == ElementStatus::Accepted)
            })
            .collect();
        let supplied: Vec<&Id> = self
            .requirements
            .iter()
            .map(|r| &r.requirement_ref)
            .collect();
        require_strictly_sorted("F1 requirement input", &supplied).map_err(invalid)?;
        if supplied != accepted {
            return Err(invalid(
                "F1 requirement inputs must cover exactly the Accepted Requirements".into(),
            ));
        }
        for input in &self.requirements {
            validate_requirement_input(graph, input, &markers).map_err(invalid)?;
        }
        let finding_ids: Vec<&Id> = self.analysis_findings.iter().map(|f| &f.id).collect();
        require_strictly_sorted("F1 analysis finding", &finding_ids).map_err(invalid)?;
        for finding in &self.analysis_findings {
            validate_analysis_finding(graph, finding).map_err(invalid)?;
        }
        Ok(())
    }
}

fn dependency_key(d: &F1VocabularyDependency) -> (u64, u64, &str) {
    (d.range.start, d.range.end, d.normalized_key.as_str())
}

fn validate_requirement_input(
    graph: &Graph,
    input: &F1RequirementValidationInput,
    markers: &[GovernedMarker],
) -> Result<(), String> {
    let id = &input.requirement_ref;
    let node = graph
        .node(id)
        .ok_or_else(|| format!("requirement {id} does not exist"))?;
    let NodePayload::Requirement(requirement) = &node.payload else {
        return Err(format!("{id} is not a Requirement"));
    };
    let lint = &input.lint_input;
    if &lint.requirement_ref != id {
        return Err(format!("lint input of {id} names {}", lint.requirement_ref));
    }
    if lint.statement != requirement.statement {
        return Err(format!("lint input statement of {id} is stale"));
    }
    let evidence: BTreeSet<&Id> = node.evidence.iter().map(|e| e.as_id()).collect();
    if lint.evidence_refs.iter().collect::<Vec<_>>() != evidence.into_iter().collect::<Vec<_>>() {
        return Err(format!("lint input evidence_refs of {id} are stale"));
    }
    if lint.term_context.is_some() {
        return Err(format!(
            "lint input of {id} carries a term context; TERMS_RESOLVED owns vocabulary"
        ));
    }
    let statement = &requirement.statement;
    let deps = &input.vocabulary_dependencies;
    for pair in deps.windows(2) {
        if dependency_key(&pair[0]) >= dependency_key(&pair[1]) {
            return Err(format!(
                "vocabulary dependencies of {id} are not strictly sorted"
            ));
        }
        if pair[1].range.start < pair[0].range.end {
            return Err(format!("vocabulary dependencies of {id} overlap"));
        }
    }
    for dep in deps {
        if !is_clean_text(&dep.normalized_key) {
            return Err(format!(
                "vocabulary key {:?} of {id} is not clean",
                dep.normalized_key
            ));
        }
        let (start, end) = (
            usize::try_from(dep.range.start).map_err(|e| e.to_string())?,
            usize::try_from(dep.range.end).map_err(|e| e.to_string())?,
        );
        if start >= end
            || end > statement.len()
            || !statement.is_char_boundary(start)
            || !statement.is_char_boundary(end)
        {
            return Err(format!(
                "vocabulary range {start}..{end} of {id} is invalid"
            ));
        }
        match &dep.resolution {
            F1VocabularyResolution::AcceptedTerm { term_ref } => {
                accepted_payload(graph, term_ref, |p| matches!(p, NodePayload::Term(_)))?;
            }
            F1VocabularyResolution::AcceptedConcept { concept_ref } => {
                accepted_payload(graph, concept_ref, |p| matches!(p, NodePayload::Concept(_)))?;
            }
            F1VocabularyResolution::ExternalIdentifier {
                decision_ref,
                identifier,
            } => {
                if !is_clean_text(identifier) {
                    return Err(format!("external identifier {identifier:?} is not clean"));
                }
                let governed = markers.iter().any(|m| {
                    &m.decision_ref == decision_ref
                        && m.marker
                            == Marker::ExternalVocabularyIdentifier {
                                requirement_ref: id.clone(),
                                normalized_key: dep.normalized_key.clone(),
                                identifier: identifier.clone(),
                            }
                });
                if !governed {
                    return Err(format!(
                        "{decision_ref} is not a governed external identifier decision for {id}"
                    ));
                }
            }
            F1VocabularyResolution::Unresolved => {}
        }
    }
    Ok(())
}

fn accepted_payload(
    graph: &Graph,
    id: &Id,
    expected: impl Fn(&NodePayload) -> bool,
) -> Result<(), String> {
    match graph.node(id) {
        Some(node) if node.status == ElementStatus::Accepted && expected(&node.payload) => Ok(()),
        _ => Err(format!(
            "{id} is not an Accepted node of the claimed vocabulary type"
        )),
    }
}

fn validate_analysis_finding(graph: &Graph, finding: &GeneratedFinding) -> Result<(), String> {
    let payload = &finding.payload;
    let id = &finding.id;
    if !F1_ANALYSIS_FINDING_CODES.contains(&payload.code.as_str()) {
        return Err(format!("analysis finding {id} has code {}", payload.code));
    }
    if payload.family != "F1" {
        return Err(format!("analysis finding {id} is not of family F1"));
    }
    if finding.key.kind() != HashKind::Generic {
        return Err(format!("analysis finding {id} key is not Generic"));
    }
    require_strictly_sorted("affected ref", &payload.affected_refs)?;
    let key = finding_key(
        &payload.code,
        &payload.affected_refs,
        &finding.semantic_condition_key,
    )
    .map_err(|e| e.to_string())?;
    if key != finding.key || finding_id(&key).map_err(|e| e.to_string())? != *id {
        return Err(format!(
            "analysis finding {id} identity does not match its content"
        ));
    }
    for target in &payload.affected_refs {
        if !graph
            .node(target)
            .is_some_and(|n| matches!(n.payload, NodePayload::Requirement(_)))
        {
            return Err(format!(
                "analysis finding {id} targets non-Requirement {target}"
            ));
        }
    }
    Ok(())
}

// ============================================================================ F2 supplemental inputs

/// The deterministic F2 supplemental inputs (Hotfix 044): current upstream analysis findings,
/// explicit scopes of legacy Calculations, typed specifications of Accepted DecisionTables and
/// explicit expression scopes of Accepted Invariants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct F2ValidationInputs {
    pub analysis_findings: Vec<GeneratedFinding>,
    pub calculation_scope_overrides: Vec<F2CalculationScopeOverride>,
    pub decision_tables: Vec<F2DecisionTableInput>,
    pub invariants: Vec<F2InvariantInput>,
}

// Every field compares structurally without floating point, so equality is reflexive.
impl Eq for F2ValidationInputs {}

/// The explicit scope of an Accepted Calculation without a valid S2.6 origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct F2CalculationScopeOverride {
    pub calculation_ref: Id,
    pub scope: CalculationScope,
}

/// The typed analysis specification bound to one Accepted DecisionTable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct F2DecisionTableInput {
    pub decision_table_ref: Id,
    pub spec: DecisionTableSpec,
}

/// The explicit expression scope of one Accepted Invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct F2InvariantInput {
    pub invariant_ref: Id,
    pub bindings: Vec<ExpressionScopeBinding>,
}

/// The analysis-finding codes F2 supplemental inputs may carry, with their required semantic
/// condition prefixes.
pub const F2_ANALYSIS_FINDINGS: [(&str, &str); 2] = [
    (
        "PLUMB.F2.DOMAIN.RELATION_TYPED",
        "domain_relationship_cardinality_unresolved:",
    ),
    (
        "PLUMB.F2.STATE.TRANSITION_COMPLETE",
        "state_transition_trigger_unresolved:",
    ),
];

impl F2ValidationInputs {
    /// Builds canonical inputs: findings by ID, overrides, tables and invariants by node ID,
    /// scope bindings and calendars sorted. Decision-table rows keep their order (FIRST tables
    /// depend on it). Validation against a graph happens in
    /// [`ValidationContext::with_f2_inputs`].
    pub fn new(
        mut analysis_findings: Vec<GeneratedFinding>,
        mut calculation_scope_overrides: Vec<F2CalculationScopeOverride>,
        mut decision_tables: Vec<F2DecisionTableInput>,
        mut invariants: Vec<F2InvariantInput>,
    ) -> F2ValidationInputs {
        analysis_findings.sort_by(|a, b| a.id.cmp(&b.id));
        for o in &mut calculation_scope_overrides {
            o.scope.bindings.sort();
            o.scope.calendar_refs.sort();
        }
        calculation_scope_overrides.sort_by(|a, b| a.calculation_ref.cmp(&b.calculation_ref));
        decision_tables.sort_by(|a, b| a.decision_table_ref.cmp(&b.decision_table_ref));
        for i in &mut invariants {
            i.bindings.sort();
        }
        invariants.sort_by(|a, b| a.invariant_ref.cmp(&b.invariant_ref));
        F2ValidationInputs {
            analysis_findings,
            calculation_scope_overrides,
            decision_tables,
            invariants,
        }
    }

    /// Generic SHA-256 of the RFC 8785 canonical JSON of the complete inputs.
    pub fn content_hash(&self) -> Result<Hash, CoreError> {
        canonical_hash(HashKind::Generic, self)
    }

    /// The scope override of a Calculation, if supplied.
    pub fn calculation_scope_override(&self, calculation_ref: &Id) -> Option<&CalculationScope> {
        self.calculation_scope_overrides
            .binary_search_by(|o| o.calculation_ref.cmp(calculation_ref))
            .ok()
            .map(|i| &self.calculation_scope_overrides[i].scope)
    }

    /// The typed specification of a DecisionTable, if supplied.
    pub fn decision_table(&self, decision_table_ref: &Id) -> Option<&DecisionTableSpec> {
        self.decision_tables
            .binary_search_by(|t| t.decision_table_ref.cmp(decision_table_ref))
            .ok()
            .map(|i| &self.decision_tables[i].spec)
    }

    /// The expression bindings of an Invariant, if supplied.
    pub fn invariant_bindings(&self, invariant_ref: &Id) -> Option<&[ExpressionScopeBinding]> {
        self.invariants
            .binary_search_by(|i| i.invariant_ref.cmp(invariant_ref))
            .ok()
            .map(|i| self.invariants[i].bindings.as_slice())
    }

    /// Requires canonical order, valid supplemental findings, exactly one override per Accepted
    /// Calculation without an S2.6 origin (none for others), exactly one typed specification per
    /// Accepted DecisionTable with its PSG hit policy and exactly one valid binding set per
    /// Accepted Invariant. Stale or malformed inputs are a context error, never a rule result.
    pub fn validate_for(&self, graph: &Graph) -> Result<(), EvaluationError> {
        let invalid = EvaluationError::InvalidContext;
        let accepted = |node_type: NodeType| -> Vec<&Id> {
            graph
                .node_ids_by_type(node_type)
                .iter()
                .filter(|id| {
                    graph
                        .node(id)
                        .is_some_and(|n| n.status == ElementStatus::Accepted)
                })
                .collect()
        };
        // Findings.
        let finding_ids: Vec<&Id> = self.analysis_findings.iter().map(|f| &f.id).collect();
        require_strictly_sorted("F2 analysis finding", &finding_ids).map_err(invalid)?;
        for finding in &self.analysis_findings {
            validate_f2_analysis_finding(graph, finding).map_err(invalid)?;
        }
        // Calculation overrides.
        let override_refs: Vec<&Id> = self
            .calculation_scope_overrides
            .iter()
            .map(|o| &o.calculation_ref)
            .collect();
        require_strictly_sorted("F2 calculation scope override", &override_refs)
            .map_err(invalid)?;
        for o in &self.calculation_scope_overrides {
            let node = graph
                .node(&o.calculation_ref)
                .filter(|n| n.status == ElementStatus::Accepted)
                .filter(|n| matches!(n.payload, NodePayload::Calculation(_)))
                .ok_or_else(|| {
                    invalid(format!(
                        "scope override {} is not an Accepted Calculation",
                        o.calculation_ref
                    ))
                })?;
            if calculation_origin_of(node).is_some() {
                return Err(invalid(format!(
                    "{} has an S2.6 calculation origin; a scope override is forbidden",
                    o.calculation_ref
                )));
            }
            if o.scope.bindings.windows(2).any(|p| p[0] > p[1])
                || o.scope.calendar_refs.windows(2).any(|p| p[0] > p[1])
            {
                return Err(invalid(format!(
                    "scope override of {} is not canonically sorted",
                    o.calculation_ref
                )));
            }
        }
        for id in accepted(NodeType::Calculation) {
            let node = graph.node(id).expect("listed node");
            if calculation_origin_of(node).is_none()
                && self.calculation_scope_override(id).is_none()
            {
                return Err(invalid(format!(
                    "Accepted Calculation {id} without an S2.6 origin needs a scope override"
                )));
            }
        }
        // Decision tables.
        let table_refs: Vec<&Id> = self
            .decision_tables
            .iter()
            .map(|t| &t.decision_table_ref)
            .collect();
        require_strictly_sorted("F2 decision-table input", &table_refs).map_err(invalid)?;
        if table_refs != accepted(NodeType::DecisionTable) {
            return Err(invalid(
                "F2 decision-table inputs must cover exactly the Accepted DecisionTables".into(),
            ));
        }
        for t in &self.decision_tables {
            let Some(NodePayload::DecisionTable(table)) =
                graph.node(&t.decision_table_ref).map(|n| &n.payload)
            else {
                return Err(invalid(format!(
                    "{} is not a DecisionTable",
                    t.decision_table_ref
                )));
            };
            if t.spec.hit_policy != table.hit_policy {
                return Err(invalid(format!(
                    "decision-table input of {} has hit policy {:?}, the PSG has {:?}",
                    t.decision_table_ref, t.spec.hit_policy, table.hit_policy
                )));
            }
        }
        // Invariants.
        let invariant_refs: Vec<&Id> = self.invariants.iter().map(|i| &i.invariant_ref).collect();
        require_strictly_sorted("F2 invariant input", &invariant_refs).map_err(invalid)?;
        if invariant_refs != accepted(NodeType::Invariant) {
            return Err(invalid(
                "F2 invariant inputs must cover exactly the Accepted Invariants".into(),
            ));
        }
        for i in &self.invariants {
            let Some(NodePayload::Invariant(invariant)) =
                graph.node(&i.invariant_ref).map(|n| &n.payload)
            else {
                return Err(invalid(format!("{} is not an Invariant", i.invariant_ref)));
            };
            if !graph
                .node(&invariant.scope_ref)
                .is_some_and(|n| n.status == ElementStatus::Accepted)
            {
                return Err(invalid(format!(
                    "scope_ref {} of {} is not an Accepted node",
                    invariant.scope_ref, i.invariant_ref
                )));
            }
            if i.bindings.windows(2).any(|p| p[0] > p[1]) {
                return Err(invalid(format!(
                    "bindings of {} are not canonically sorted",
                    i.invariant_ref
                )));
            }
            validate_expression_bindings(graph, &i.bindings, &[]).map_err(|issue| {
                invalid(format!(
                    "expression scope of {} is invalid: {issue:?}",
                    i.invariant_ref
                ))
            })?;
        }
        Ok(())
    }
}

fn validate_f2_analysis_finding(graph: &Graph, finding: &GeneratedFinding) -> Result<(), String> {
    let payload = &finding.payload;
    let id = &finding.id;
    let Some((_, prefix)) = F2_ANALYSIS_FINDINGS
        .iter()
        .find(|(code, _)| *code == payload.code)
    else {
        return Err(format!("analysis finding {id} has code {}", payload.code));
    };
    if !finding.semantic_condition_key.starts_with(prefix) {
        return Err(format!(
            "analysis finding {id} condition does not start with {prefix}"
        ));
    }
    if payload.family != "F2" {
        return Err(format!("analysis finding {id} is not of family F2"));
    }
    if finding.key.kind() != HashKind::Generic {
        return Err(format!("analysis finding {id} key is not Generic"));
    }
    require_strictly_sorted("affected ref", &payload.affected_refs)?;
    let key = finding_key(
        &payload.code,
        &payload.affected_refs,
        &finding.semantic_condition_key,
    )
    .map_err(|e| e.to_string())?;
    if key != finding.key || finding_id(&key).map_err(|e| e.to_string())? != *id {
        return Err(format!(
            "analysis finding {id} identity does not match its content"
        ));
    }
    for target in &payload.affected_refs {
        if graph.node(target).is_none() {
            return Err(format!("analysis finding {id} targets missing {target}"));
        }
    }
    Ok(())
}

// ============================================================================ governed decisions

/// The S1.6 decision markers, decoded exactly.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Marker {
    HumanRequirementOrigin {
        requirement_ref: Id,
    },
    ExternalVocabularyIdentifier {
        requirement_ref: Id,
        normalized_key: String,
        identifier: String,
    },
    DeferredVerification {
        requirement_ref: Id,
    },
}

const MARKER_KINDS: [&str; 3] = [
    "human_requirement_origin",
    "external_vocabulary_identifier",
    "deferred_verification",
];

/// A governed S1.6 decision marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GovernedMarker {
    pub(crate) decision_ref: Id,
    pub(crate) marker: Marker,
}

/// Every Accepted decision claiming an S1.6 marker kind, validated by the common governance
/// check: exact marker, clean rationale and an Accepted human Agent as decider. Unrelated
/// decision kinds and non-Accepted decisions are ignored; a malformed or ungoverned claim is
/// an error.
pub(crate) fn governed_markers(graph: &Graph) -> Result<Vec<GovernedMarker>, String> {
    let mut out = Vec::new();
    for id in graph.node_ids_by_type(NodeType::ResolutionDecision) {
        let Some(node) = graph.node(id) else {
            continue;
        };
        let NodePayload::ResolutionDecision(decision) = &node.payload else {
            continue;
        };
        let claimed = decision
            .answer
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| MARKER_KINDS.contains(&kind));
        if node.status != ElementStatus::Accepted || !claimed {
            continue;
        }
        let marker: Marker = serde_json::from_value(decision.answer.clone())
            .map_err(|e| format!("decision {id} has a malformed marker: {e}"))?;
        if let Marker::ExternalVocabularyIdentifier {
            normalized_key,
            identifier,
            ..
        } = &marker
        {
            if !is_clean_text(normalized_key) || !is_clean_text(identifier) {
                return Err(format!("decision {id} has unclean marker text"));
            }
        }
        if !decision.rationale.as_deref().is_some_and(is_clean_text) {
            return Err(format!("decision {id} lacks a clean rationale"));
        }
        let human = graph.node(&decision.decided_by).is_some_and(|agent| {
            agent.status == ElementStatus::Accepted
                && matches!(&agent.payload, NodePayload::Agent(a) if a.agent_kind == AgentKind::Human)
        });
        if !human {
            return Err(format!(
                "decision {id} is not decided by an Accepted human Agent"
            ));
        }
        out.push(GovernedMarker {
            decision_ref: id.clone(),
            marker,
        });
    }
    Ok(out)
}
