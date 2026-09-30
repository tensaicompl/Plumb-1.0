//! Functional semantics over the PSG. F0.14 provides the one-way functional.yaml v2
//! compatibility projection (compiler architecture §30); the PSG stays canonical and nothing
//! is imported back from functional.yaml.

pub mod model;
pub mod project;

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
