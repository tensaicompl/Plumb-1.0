//! The pure functional interpreter. S4.2 (Hotfix 051) is execution: an immutable
//! [`ExecutionModel`] is validated once from one graph baseline plus explicit supplemental inputs
//! and [`run`] executes one Accepted, Confirmed Scenario, returning Executed, Failed or Undecidable
//! execution semantics with a typed trace. S4.3 (Hotfix 052) is comparison and trace material:
//! [`compare_run`] compares a completed execution with the Scenario's Then expectations and
//! builds ScenarioRun and canonical scenario-trace artifact material. S4.4 (Hotfix 053) is
//! incremental re-execution: [`rerun_dirty_scenarios`] reruns the Confirmed Scenarios that an
//! F0.11 dirty set reaches and returns S4 execution-finding material. Nothing here mutates a
//! graph, persists, reads files or a clock, or calls a provider other than the injected calendar
//! and predicate.

mod compare;
mod execute;
mod incremental;
mod materialize;
mod model;
mod trace;

// The S4.3 modules' public items are exactly the comparison and trace API.
pub use compare::*;
pub use execute::run;
pub use incremental::{
    rerun_dirty_scenarios, scenario_execution_finding, scenario_run_currency,
    scenario_run_currency_since, select_dirty_scenarios, IncrementalError,
    IncrementalScenarioPolicy, IncrementalScenarioResult, IncrementalScenarioRuntime,
    ScenarioRunCurrency, SCENARIO_FAIL_CODE, SCENARIO_FAIL_CONDITION, SCENARIO_FINDING_FAMILY,
    SCENARIO_UNDECIDABLE_CODE, SCENARIO_UNDECIDABLE_CONDITION,
};
pub use model::{
    build_execution_model, missing_keys, AuthorizationDecision, EvaluationFailureKind,
    ExecutionCalculationScopeOverride, ExecutionExpressionScopeInput, ExecutionFailure,
    ExecutionInputKind, ExecutionInputs, ExecutionModel, ExecutionResult, ExecutionRun,
    ExecutionRuntime, ExecutionTraceStep, ExpressionSite, MissingSemantic, MissingSemanticKind,
    ResolutionSource, RuntimeSemanticValue, RuntimeState, RuntimeValue, SimulationError,
    EXECUTION_CONTRACT_VERSION,
};
pub use trace::*;
