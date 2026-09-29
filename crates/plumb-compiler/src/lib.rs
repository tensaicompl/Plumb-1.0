//! The deterministic compiler-stage boundary: pure PLAN/EVALUATE contracts, acquired-artifact
//! inputs and compile-run records (compiler architecture §§3, 4.2, 28).
//!
//! Stages receive no provider, artifact or revision store, clock, network client or commit
//! capability. Only the compile-run persistence helper writes artifacts.

pub mod context;
pub mod run;
pub mod stage;

pub use context::{CompileContext, Scope};
pub use run::{persist_compile_run, CompileRun, PersistedCompileRun};
pub use stage::{
    ArtifactInput, ArtifactSet, CompilerError, CompilerStage, ExternalValidationArtifact,
    ExternalValidationRequest, PlannedArtifact, StageEvaluation, StagePlan, JSON_MEDIA_TYPE,
};
