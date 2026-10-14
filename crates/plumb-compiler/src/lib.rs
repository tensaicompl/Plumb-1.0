//! The deterministic compiler-stage boundary: pure PLAN/EVALUATE contracts, acquired-artifact
//! inputs and compile-run records (compiler architecture §§3, 4.2, 28).
//!
//! Stages receive no provider, artifact or revision store, clock, network client or commit
//! capability. Only the compile-run persistence helper writes artifacts.
//!
//! X1.5 adds the deterministic asynchronous stage job lifecycle and its revision/head
//! protection (Hotfix 061): a state model that executes, schedules and persists nothing and
//! leaves the final compare-and-swap to the revision store.
//!
//! Hotfix 066 binds each planned inference request to its exact material
//! ([`PlannedInferenceRequest`]) so acquisition hands the provider both.

pub mod context;
pub mod job;
pub mod run;
pub mod stage;

pub use context::{CompileContext, Scope};
pub use job::{capture_stage_job, StageJob, StageJobError, StageJobStatus};
pub use run::{persist_compile_run, CompileRun, PersistedCompileRun};
pub use stage::{
    ArtifactInput, ArtifactSet, CompilerError, CompilerStage, ExternalValidationArtifact,
    ExternalValidationRequest, InferenceAbsencePolicy, PlannedArtifact, PlannedInferenceRequest,
    StageEvaluation, StagePlan, JSON_MEDIA_TYPE,
};
