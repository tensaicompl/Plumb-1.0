//! The pure functional interpreter (plan S4.2, Hotfix 051). An immutable [`ExecutionModel`] is
//! validated once from one graph baseline plus explicit supplemental inputs; [`run`] executes one
//! Accepted, Confirmed Scenario and returns Executed, Failed or Undecidable execution semantics
//! with a typed trace. Comparison with Scenario.then, structured diffs and ScenarioRun evidence
//! belong to later tasks; nothing here mutates a graph, persists, reads files or a clock, or calls
//! a provider other than the injected calendar and predicate.

mod execute;
mod materialize;
mod model;

pub use execute::run;
pub use model::{
    build_execution_model, missing_keys, AuthorizationDecision, EvaluationFailureKind,
    ExecutionCalculationScopeOverride, ExecutionExpressionScopeInput, ExecutionFailure,
    ExecutionInputKind, ExecutionInputs, ExecutionModel, ExecutionResult, ExecutionRun,
    ExecutionRuntime, ExecutionTraceStep, ExpressionSite, MissingSemantic, MissingSemanticKind,
    ResolutionSource, RuntimeSemanticValue, RuntimeState, RuntimeValue, SimulationError,
    EXECUTION_CONTRACT_VERSION,
};
