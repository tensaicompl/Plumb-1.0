//! Validation profile metadata: the typed rule-pack model, its YAML loader and hashes, and
//! the metadata registry (compiler architecture §5.5). Rule evaluation is not part of it.

pub mod model;
pub mod profile;
pub mod registry;

pub use model::{
    EvaluationMode, GateMetadata, RuleClass, RuleMetadata, RuleResultState, Severity,
    StandardDefinition, StandardRef, UnknownVocabularyValue, ValidationError, ValidationProfile,
    WaiverPolicy, EXPECTED_RULE_COUNT,
};
pub use profile::{load_builtin_software_profile, load_profile_yaml};
pub use registry::{ValidationRegistry, EXPECTED_NOW_RULE_COUNT, NOW_GATES};
