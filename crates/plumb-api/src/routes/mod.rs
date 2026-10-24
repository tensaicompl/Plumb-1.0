//! Router composition of the pilot runtime handlers: the 30 A0.2-owned operations (plan A0.2,
//! Hotfix 063), the 40 A0.3-owned operations (plan A0.3, Hotfix 065) and the two A0.4-owned
//! stage and job operations (plan A0.4, Hotfix 067): all 72 operations.

use axum::extract::DefaultBodyLimit;
use axum::http::{header, StatusCode};
use axum::middleware::map_response;
use axum::response::{IntoResponse, Response};
use axum::Router;

use crate::error::ApiError;
use crate::state::ApiState;

mod gates;
mod meta;
mod model;
mod proposals;
mod questions;
mod requirements;
mod scenarios;
mod sentences;
mod sessions;
mod sources;
mod stages;

pub(crate) use scenarios::lifecycle_error as scenario_lifecycle_error;

/// Largest request body of `POST /api/sources` (architecture §33.1).
pub const MAX_SOURCE_UPLOAD_BODY_BYTES: usize = 129 * 1024 * 1024;

/// Largest request body of every other route (architecture §33.1).
pub const MAX_JSON_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Whether `response` is a typed PLUMB error rather than a framework rejection.
fn is_typed(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes() == b"application/json")
}

/// Replaces the framework's plain-text body-limit rejection with the typed 413 of `limit_bytes`.
fn typed_payload_too_large(limit_bytes: usize, response: Response) -> Response {
    if response.status() == StatusCode::PAYLOAD_TOO_LARGE && !is_typed(&response) {
        ApiError::payload_too_large(limit_bytes).into_response()
    } else {
        response
    }
}

/// The pilot HTTP router over `state`. `POST /api/sources` carries its own body limit, set on
/// its route and therefore overriding the router-wide default of [`MAX_JSON_BODY_BYTES`].
pub fn router(state: ApiState) -> Router {
    Router::new()
        .merge(meta::routes())
        .merge(sources::routes())
        .merge(requirements::routes())
        .merge(gates::routes())
        .merge(model::routes())
        .merge(sentences::routes())
        .merge(questions::routes())
        .merge(proposals::routes())
        .merge(sessions::routes())
        .merge(scenarios::routes())
        .merge(stages::routes())
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .layer(map_response(|response: Response| async move {
            typed_payload_too_large(MAX_JSON_BODY_BYTES, response)
        }))
        .with_state(state)
}
