//! Plumb pilot HTTP API. A0.1 provides the generated pilot OpenAPI contract with its v3
//! overrides (Hotfix 062); A0.2 provides the router and runtime state of the 30 A0.2-owned
//! operations (Hotfix 063). No server binary, listener or deployment setup lives here.

pub mod error;
pub mod openapi;
pub mod routes;
pub mod state;

pub use openapi::{
    openapi_document, render_api_markdown, render_openapi_json, ApiDoc, OpenApiContractError,
};
pub use routes::router;
pub use state::{ApiIdentity, ApiState, ApiStateError};
