//! Validation: the typed rule-pack model, its YAML loader and hashes, the metadata registry,
//! the external-validation contracts and the deterministic gate evaluator framework
//! (compiler architecture §§5.5, 29), plus the pure Process graph-analysis kernel (S2.9), which
//! is not an evaluator.

pub mod authorization_analysis;
pub mod calculation_analysis;
pub mod decision_table_analysis;
pub mod evaluator;
pub mod expression_scope;
pub mod external;
pub mod finding;
pub mod model;
pub mod process_analysis;
pub mod profile;
pub mod registry;
pub mod rules;
pub mod waiver;

pub use evaluator::{
    violation_state, Applicability, EvaluationError, EvaluatorFailure, EvaluatorRegistry,
    F1RequirementValidationInput, F1ValidationInputs, F1VocabularyDependency,
    F1VocabularyResolution, F2CalculationScopeOverride, F2DecisionTableInput, F2InvariantInput,
    F2ValidationInputs, GateReport, GateResult, GateSummary, RuleEvaluation, RuleEvaluator,
    RuleResult, ValidationArtifactInput, ValidationContext, ValidationPolicy,
    EVIDENCE_ARTIFACT_KINDS, F1_ANALYSIS_FINDING_CODES, F2_ANALYSIS_FINDINGS,
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
pub use process_analysis::{
    analyze_process, BoundaryViolation, NodeShapeViolation, ParallelAnalysis, ParallelIssue,
    ParallelIssueKind, ParallelSplitAnalysis, ProcessAnalysis, ProcessAnalysisError,
    ProcessAnalysisMode, ShapeViolation, TaskResolutionIssue, UnresolvedTask, UnsupportedNode,
    UnsupportedSemantics,
};
pub use profile::{load_builtin_software_profile, load_profile_yaml};
pub use registry::{ValidationRegistry, EXPECTED_NOW_RULE_COUNT, NOW_GATES};
pub use waiver::{governed_waiver, AppliedWaiver, WaiverDecisionMarker, WAIVER_KIND};
