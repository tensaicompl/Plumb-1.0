//! Validation profile metadata: the typed rule-pack model, its YAML loader and hashes, the
//! metadata registry and the external-validation contracts (compiler architecture §5.5).

pub mod external;
pub mod model;
pub mod profile;
pub mod registry;

pub use external::{
    ExternalValidationArtifact, ExternalValidationError, ExternalValidationRequest,
};
pub use model::{
    EvaluationMode, GateMetadata, RuleClass, RuleMetadata, RuleResultState, Severity,
    StandardDefinition, StandardRef, UnknownVocabularyValue, ValidationError, ValidationProfile,
    WaiverPolicy, EXPECTED_RULE_COUNT,
};
pub use profile::{load_builtin_software_profile, load_profile_yaml};
pub use registry::{ValidationRegistry, EXPECTED_NOW_RULE_COUNT, NOW_GATES};
