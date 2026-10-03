//! Validation: the typed rule-pack model, its YAML loader and hashes, the metadata registry,
//! the external-validation contracts and the deterministic gate evaluator framework
//! (compiler architecture §§5.5, 29).

pub mod evaluator;
pub mod external;
pub mod finding;
pub mod model;
pub mod profile;
pub mod registry;
pub mod rules;
pub mod waiver;

pub use evaluator::{
    violation_state, Applicability, EvaluationError, EvaluatorFailure, EvaluatorRegistry,
    GateReport, GateResult, GateSummary, RuleEvaluation, RuleEvaluator, RuleResult,
    ValidationArtifactInput, ValidationContext, ValidationPolicy, EVIDENCE_ARTIFACT_KINDS,
};
pub use external::{
    ExternalValidationArtifact, ExternalValidationError, ExternalValidationRequest,
};
pub use finding::{finding_id, finding_key, finding_severity, GeneratedFinding, ViolationFacts};
pub use model::{
    EvaluationMode, GateMetadata, RuleClass, RuleMetadata, RuleResultState, Severity,
    StandardDefinition, StandardRef, UnknownVocabularyValue, ValidationError, ValidationProfile,
    WaiverPolicy, EXPECTED_RULE_COUNT,
};
pub use profile::{load_builtin_software_profile, load_profile_yaml};
pub use registry::{ValidationRegistry, EXPECTED_NOW_RULE_COUNT, NOW_GATES};
pub use waiver::{governed_waiver, AppliedWaiver, WaiverDecisionMarker, WAIVER_KIND};
