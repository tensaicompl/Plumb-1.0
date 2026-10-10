//! Functional semantics over the PSG. F0.14 provides the one-way functional.yaml v2
//! compatibility projection (compiler architecture §30); the PSG stays canonical and nothing
//! is imported back from functional.yaml. S1.1 compiles S0.4 requirement candidates into
//! HUMAN_CONFIRM proposals of Proposed requirement and intent nodes, S1.2 proposes grounded
//! EARS rewrites of their statements, and S1.3 detects duplicates and proposes governed merges
//! and supersessions, and S1.5 analyzes span-grounded vocabulary (compiler architecture §7).
//! S2.1 proposes closed-vocabulary Entity, Attribute and DomainRelationship nodes grounded in
//! Accepted Concepts (compiler architecture §8).
//! S2.2 proposes grounded States, triggered Transitions and unqualified Invariants
//! (compiler architecture §8).
//! S2.3 classifies Accepted Attributes from a versioned dictionary and inference.
//! S2.6 qualifies grounded calculations over explicit PlumbExpr scopes and analyzes typed
//! decision tables.
//! S2.7 proposes grounded Operations with typed relations, Outcomes and Events over a closed
//! scope.
//! S2.8 compiles explicit structured authorization semantics and analyzes role hierarchy and
//! static separation of duty.
//! S2.9 proposes grounded Processes with safe control flow, qualified by the plumb-validation
//! process-analysis kernel.
//! S3.1 generates deterministic, routed governance Questions from current finding material
//! through a closed template registry (compiler architecture §9).
//! S3.2 composes at most one deterministic question round per stakeholder and assigns it through
//! Question.round_ref.

pub mod authorization;
pub mod calculation;
pub mod data_class;
pub mod decision_table;
pub mod domain;
pub mod duplicates;
pub mod ears;
pub mod event;
mod intent;
pub mod invariant;
pub mod model;
pub mod operation;
pub mod process;
pub mod project;
pub mod question;
pub mod question_templates;
pub mod requirements;
pub mod round;
pub mod state;
pub mod vocabulary;
mod vocabulary_exceptions;

// The merge/supersede wrapper enum is reached through the `duplicates` module path, because the
// F0.14 output-only guard keeps proposal type names out of this file.
pub use authorization::{
    accepted_role_hierarchy_cycles, accepted_separation_analysis, analyze_authorization,
    authorization_facts, effective_security_roles, role_hierarchy_cycles, separation_analysis,
    AuthorizationAnalysisResult, AuthorizationAudit, AuthorizationDisposition, AuthorizationDraft,
    AuthorizationError, AuthorizationFacts, AuthorizationIssue, PermissionDraft,
    PolicyConditionDraft, PrincipalDraft, RoleAssignmentDraft, RoleInheritanceDraft,
    SecurityRoleDraft, SeparationAnalysis, SeparationConstraintDraft, StaticSodViolation,
};
pub use calculation::{
    analyze_calculation_cycles, analyze_calculations, build_calculation_request,
    default_field_binding, default_root_binding, evaluate_calculation,
    qualify_accepted_calculation, CalculationBindingExposure, CalculationError, CalculationIssue,
    CalculationScope, CalculationScopeBinding, CALCULATION_CALENDAR_SYMBOL,
    CALCULATION_CONTEXT_VERSION, CALCULATION_ORIGIN_EXTENSION, CALCULATION_OUTPUT_VERSION,
    CALCULATION_TASK_KIND,
};
pub use data_class::{
    analyze_data_classification, build_data_class_request, dictionary_classification,
    DataClassAnalysisResult, DataClassAttributeContext, DataClassConflict, DataClassContext,
    DataClassDictionaryHit, DataClassError, DataClassInference, DataClassIssue, DataClassRequest,
    DataClassSource, PilotDataClass, DATA_CLASS_CONTEXT_VERSION, DATA_CLASS_DICTIONARY,
    DATA_CLASS_DICTIONARY_VERSION, DATA_CLASS_OUTPUT_VERSION, DATA_CLASS_TASK_KIND,
};
pub use decision_table::{
    analyze_decision_table, DecisionCoverage, DecisionTableAnalysis, DecisionTableSpec,
    MAX_COVERAGE_WITNESSES, MAX_FINITE_DECISION_POINTS,
};
pub use domain::{
    analyze_domain, build_domain_request, DomainAnalysisResult, DomainAudit, DomainCardinality,
    DomainConceptContext, DomainConflict, DomainContext, DomainError, DomainGrounding,
    DomainInference, DomainIssue, DomainMergeDisposition, DomainMergeOutcome, DomainOrigin,
    DomainRequest, DomainRequirementContext, DOMAIN_CONTEXT_VERSION, DOMAIN_ORIGIN_EXTENSION,
    DOMAIN_OUTPUT_VERSION, DOMAIN_TASK_KIND,
};
pub use duplicates::{
    analyze_requirement_duplicates, DuplicateAnalysisResult, DuplicateError, DuplicateMatch,
    DuplicateMatchKind, DuplicatePolicy, JaccardScore, MergeDisposition,
};
pub use ears::{
    build_ears_request, evaluate_ears_normalization, EarsAcceptedSemantic, EarsContext, EarsError,
    EarsGrounding, EarsInference, EarsIssue, EarsNormalizationResult, EarsPattern, EarsRequest,
    EarsRequirementContext, EARS_CONTEXT_VERSION, EARS_OUTPUT_VERSION, EARS_TASK_KIND,
};
pub use event::{event_id, EventOrigin, EVENT_ORIGIN_EXTENSION};
pub use invariant::{InvariantOrigin, INVARIANT_ORIGIN_EXTENSION};
pub use model::{
    codes, ActorEntry, AssumptionEntry, AttributeEntry, CalculationEntry, CalendarEntry,
    EntityEntry, EntityNfr, EventEntry, FunctionalProjection, FunctionalV2, GlossaryEntry,
    InvariantEntry, LegacyActorKind, LegacyOutcomeKind, NextEntry, OperationEntry, OperationNfr,
    OutcomeEntry, ProcessEntry, ProjectionMetadata, ProjectionNotice, RelationshipEntry,
    RequirementEntry, RoleEntry, RoundingEntry, RuleEntry, ScenarioEntry, StateEntry, StepEntry,
    TransitionEntry, TriggerEntry, FUNCTIONAL_V2_PROJECTION_VERSION, FUNCTIONAL_V2_VERSION,
};
pub use operation::{
    analyze_operations, build_operation_request, CalculationReadAnalysis, OperationAnalysisResult,
    OperationAudit, OperationCandidateAnalysis, OperationContext, OperationDisposition,
    OperationError, OperationGroundedRange, OperationInference, OperationIssue, OperationOrigin,
    OperationRequest, OperationScope, OutcomeOrigin, OPERATION_CONTEXT_VERSION,
    OPERATION_ORIGIN_EXTENSION, OPERATION_OUTPUT_VERSION, OPERATION_TASK_KIND,
    OUTCOME_ORIGIN_EXTENSION,
};
pub use process::{
    analysis_issues, analyze_processes, build_process_request, ProcessAudit,
    ProcessCandidateAnalysis, ProcessCompilationResult, ProcessContext, ProcessDisposition,
    ProcessError, ProcessGroundedRange, ProcessInference, ProcessIssue, ProcessNodeOrigin,
    ProcessOrigin, ProcessRequest, ProcessScope, PROCESS_CONTEXT_VERSION,
    PROCESS_NODE_ORIGIN_EXTENSION, PROCESS_ORIGIN_EXTENSION, PROCESS_OUTPUT_VERSION,
    PROCESS_TASK_KIND,
};
pub use project::{
    project_functional_v2, validate_against_schema, validate_metadata, ProjectionError,
    FUNCTIONAL_V2_SCHEMA,
};
pub use question::{
    generate_questions, question_id, route_question, severity_weight, GeneratedQuestion,
    QuestionAudit, QuestionDisposition, QuestionError, QuestionGenerationInput,
    QuestionGenerationResult, QuestionIssue, QuestionMaterialization, RouteDisposition,
    RoutingStakeholder, StakeholderRoutingConfig, UnmappedFinding,
};
pub use question_templates::{QuestionTemplate, UnmappedReason, QUESTION_TEMPLATES};
pub use requirements::{
    build_requirement_classification_request, compile_requirement_candidates,
    RequirementClassificationContext, RequirementClassificationContextCandidate,
    RequirementClassificationInference, RequirementClassificationRequest,
    RequirementClassificationStakeholder, RequirementCompilationAudit, RequirementCompilationError,
    RequirementCompilationIssue, RequirementCompilationResult, SegmentOrigin,
    REQUIREMENT_CLASSIFICATION_CONTEXT_VERSION, REQUIREMENT_CLASSIFICATION_OUTPUT_VERSION,
    REQUIREMENT_CLASSIFICATION_TASK_KIND, SEGMENT_ORIGIN_EXTENSION,
};
pub use round::{
    compose_question_rounds, entity_anchor, parse_priority, round_id, QuestionRound,
    QuestionRoundConfig, QuestionRoundDisposition, QuestionRoundGroup, QuestionRoundResult,
    RoundError, StaleReason, MAX_QUESTION_ROUND_SIZE,
};
pub use state::{
    analyze_lifecycle, build_lifecycle_request, LifecycleAnalysisResult, LifecycleAudit,
    LifecycleConflict, LifecycleContext, LifecycleError, LifecycleExisting, LifecycleGrounding,
    LifecycleInference, LifecycleIssue, LifecycleOwnerContext, LifecycleRequest,
    LifecycleRequirementContext, LifecycleTriggerContext, StateOrigin, LIFECYCLE_CONTEXT_VERSION,
    LIFECYCLE_OUTPUT_VERSION, LIFECYCLE_TASK_KIND, STATE_ORIGIN_EXTENSION,
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
