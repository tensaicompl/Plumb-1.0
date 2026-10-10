//! The pure functional interpreter. S4.2 (Hotfix 051) is execution: an immutable
//! [`ExecutionModel`] is validated once from one graph baseline plus explicit supplemental inputs
//! and [`run`] executes one Accepted, Confirmed Scenario, returning Executed, Failed or Undecidable
//! execution semantics with a typed trace. S4.3 (Hotfix 052) is comparison and trace material:
//! [`compare_run`] compares a completed execution with the Scenario's Then expectations and
//! builds ScenarioRun and canonical scenario-trace artifact material. Nothing here mutates a
//! graph, persists, reads files or a clock, or calls a provider other than the injected calendar
//! and predicate.

mod compare;
mod execute;
mod materialize;
mod model;
mod trace;

// The S4.3 modules' public items are exactly the comparison and trace API.
pub use compare::*;
pub use execute::run;
pub use model::{
    build_execution_model, missing_keys, AuthorizationDecision, EvaluationFailureKind,
    ExecutionCalculationScopeOverride, ExecutionExpressionScopeInput, ExecutionFailure,
    ExecutionInputKind, ExecutionInputs, ExecutionModel, ExecutionResult, ExecutionRun,
    ExecutionRuntime, ExecutionTraceStep, ExpressionSite, MissingSemantic, MissingSemanticKind,
    ResolutionSource, RuntimeSemanticValue, RuntimeState, RuntimeValue, SimulationError,
    EXECUTION_CONTRACT_VERSION,
};
pub use trace::*;
