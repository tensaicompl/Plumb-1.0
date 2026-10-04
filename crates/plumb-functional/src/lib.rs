//! Functional semantics over the PSG. F0.14 provides the one-way functional.yaml v2
//! compatibility projection (compiler architecture §30); the PSG stays canonical and nothing
//! is imported back from functional.yaml. S1.1 compiles S0.4 requirement candidates into
//! HUMAN_CONFIRM proposals of Proposed requirement and intent nodes, S1.2 proposes grounded
//! EARS rewrites of their statements, and S1.3 detects duplicates and proposes governed merges
//! and supersessions, and S1.5 analyzes span-grounded vocabulary (compiler architecture §7).

pub mod duplicates;
pub mod ears;
mod intent;
pub mod model;
pub mod project;
pub mod requirements;
pub mod vocabulary;
mod vocabulary_exceptions;

// The merge/supersede wrapper enum is reached through the `duplicates` module path, because the
// F0.14 output-only guard keeps proposal type names out of this file.
pub use duplicates::{
    analyze_requirement_duplicates, DuplicateAnalysisResult, DuplicateError, DuplicateMatch,
    DuplicateMatchKind, DuplicatePolicy, JaccardScore, MergeDisposition,
};
pub use ears::{
    build_ears_request, evaluate_ears_normalization, EarsAcceptedSemantic, EarsContext, EarsError,
    EarsGrounding, EarsInference, EarsIssue, EarsNormalizationResult, EarsPattern, EarsRequest,
    EarsRequirementContext, EARS_CONTEXT_VERSION, EARS_OUTPUT_VERSION, EARS_TASK_KIND,
};
pub use model::{
    codes, ActorEntry, AssumptionEntry, AttributeEntry, CalculationEntry, CalendarEntry,
    EntityEntry, EntityNfr, EventEntry, FunctionalProjection, FunctionalV2, GlossaryEntry,
    InvariantEntry, LegacyActorKind, LegacyOutcomeKind, NextEntry, OperationEntry, OperationNfr,
    OutcomeEntry, ProcessEntry, ProjectionMetadata, ProjectionNotice, RelationshipEntry,
    RequirementEntry, RoleEntry, RoundingEntry, RuleEntry, ScenarioEntry, StateEntry, StepEntry,
    TransitionEntry, TriggerEntry, FUNCTIONAL_V2_PROJECTION_VERSION, FUNCTIONAL_V2_VERSION,
};
pub use project::{
    project_functional_v2, validate_against_schema, validate_metadata, ProjectionError,
    FUNCTIONAL_V2_SCHEMA,
};
pub use requirements::{
    build_requirement_classification_request, compile_requirement_candidates,
    RequirementClassificationContext, RequirementClassificationContextCandidate,
    RequirementClassificationInference, RequirementClassificationRequest,
    RequirementClassificationStakeholder, RequirementCompilationAudit, RequirementCompilationError,
    RequirementCompilationIssue, RequirementCompilationResult, SegmentOrigin,
    REQUIREMENT_CLASSIFICATION_CONTEXT_VERSION, REQUIREMENT_CLASSIFICATION_OUTPUT_VERSION,
    REQUIREMENT_CLASSIFICATION_TASK_KIND, SEGMENT_ORIGIN_EXTENSION,
};
pub use vocabulary::{
    analyze_vocabulary, build_vocabulary_request, normalize_vocabulary_term,
    VocabularyAnalysisResult, VocabularyAudit, VocabularyConflict, VocabularyContext,
    VocabularyCooccurrence, VocabularyError, VocabularyGroundedRange, VocabularyInference,
    VocabularyIssue, VocabularyLintContext, VocabularyMention, VocabularyOrigin,
    VocabularyOriginMention, VocabularyPolicy, VocabularyRequest, VocabularyRequirementContext,
    VocabularyTermCandidate, PILOT_VOCABULARY_LANGUAGE, VOCABULARY_CONTEXT_VERSION,
    VOCABULARY_NORMALIZATION_VERSION, VOCABULARY_ORIGIN_EXTENSION, VOCABULARY_OUTPUT_VERSION,
    VOCABULARY_TASK_KIND,
};
