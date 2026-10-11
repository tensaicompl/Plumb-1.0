//! Plumb pilot HTTP API. A0.1 provides the generated pilot OpenAPI contract with its v3
//! overrides (Hotfix 062); runtime handlers are added by later tasks.

pub mod openapi;

pub use openapi::{
    openapi_document, render_api_markdown, render_openapi_json, ApiDoc, OpenApiContractError,
};
