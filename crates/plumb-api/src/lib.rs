//! Plumb pilot HTTP API. A0.1 provides the generated pilot OpenAPI contract with its v3
//! overrides (Hotfix 062); A0.2 provides the router and runtime state of the 30 A0.2-owned
//! operations (Hotfix 063); A0.3 adds the 40 A0.3-owned operations over the injected A0.3
//! runtime and the durable workflow store (Hotfix 065); A0.4 adds the stage orchestration
//! service, the two stage/job operations, the application construction layer and the blocking
//! server boundary (Hotfix 067, Hotfix 068). The listener lives in `server`, never here.

pub mod application;
pub mod error;
pub mod error_catalogue;
pub mod openapi;
pub mod routes;
pub mod runtime;
pub mod server;
pub mod stage;
pub mod state;
mod store_error;
mod workflow;

pub use openapi::{
    openapi_document, render_api_markdown, render_openapi_json, ApiDoc, OpenApiContractError,
};
pub use routes::router;
pub use runtime::{
    A04RuntimeConfig, CalendarConfig, DirectManagerFact, DirectManagerPredicates, F1GateConfig,
    F4GateConfig, GateInputsConfig, ProviderMode, RuntimeConfigError,
};
pub use server::{serve_blocking, ServeError};
pub use stage::{ReadyGate, ReadyOutcome, ReadyRule, Recipe, StageRun};
pub use state::{A03RuntimeConfig, ApiIdentity, ApiState, ApiStateError, OpenMode};
