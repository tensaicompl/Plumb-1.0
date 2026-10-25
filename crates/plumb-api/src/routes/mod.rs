//! Router composition of the pilot runtime handlers: the 30 A0.2-owned operations (plan A0.2,
//! Hotfix 063), the 40 A0.3-owned operations (plan A0.3, Hotfix 065) and the two A0.4-owned
//! stage and job operations (plan A0.4, Hotfix 067): all 72 operations.

use std::path::Path;
use std::time::Instant;

use axum::extract::{DefaultBodyLimit, MatchedPath, Request};
use axum::http::{header, StatusCode};
use axum::middleware::{from_fn, map_response, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;

use crate::error::ApiError;
use crate::server::{is_loopback_authority, is_loopback_origin};
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

pub(crate) use model::waive as waive_finding;
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

/// Replaces a framework or extractor rejection (a non-JSON error response) with the typed
/// `{code, message, details}` envelope of its status (architecture §33.12): the body limit is the
/// typed 413 of `limit_bytes`; the framework's own text is never echoed.
fn typed_framework_error(limit_bytes: usize, response: Response) -> Response {
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) || is_typed(&response) {
        return response;
    }
    let error = match status {
        StatusCode::PAYLOAD_TOO_LARGE => ApiError::payload_too_large(limit_bytes),
        StatusCode::NOT_FOUND => ApiError::route_not_found(),
        StatusCode::METHOD_NOT_ALLOWED => ApiError::method_not_allowed(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => ApiError::unsupported_request_media_type(),
        s if s.is_client_error() => ApiError::invalid("the request is malformed"),
        _ => ApiError::internal("http_framework"),
    };
    let mut typed = error.into_response();
    // Keep the framework's status-specific headers (e.g. Allow, Upgrade) next to the typed body.
    for (name, value) in response.headers() {
        if name != header::CONTENT_TYPE && name != header::CONTENT_LENGTH {
            typed.headers_mut().insert(name.clone(), value.clone());
        }
    }
    *typed.status_mut() = status;
    typed
}

/// One tracing record per request (Fable #202): method, matched route (never the raw path, which
/// may carry client text), status and latency.
async fn trace_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |p| p.as_str().to_owned());
    let started = Instant::now();
    let response = next.run(request).await;
    tracing::info!(
        target: "plumb_api::http",
        method = %method,
        route = %route,
        status = response.status().as_u16(),
        latency_ms = started.elapsed().as_millis() as u64,
        "request"
    );
    response
}

/// The typed 404 of a path no route serves.
async fn route_not_found() -> ApiError {
    ApiError::route_not_found()
}

/// The typed 405 of a served path with another method.
async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed()
}

/// Loopback-only serving (architecture §33.8): a request without a loopback `Host` is 403
/// `E_FORBIDDEN_HOST`; a request whose `Origin` is present and not an `http` loopback origin is
/// 403 `E_FORBIDDEN_ORIGIN`. It runs before routing, body limits and every handler.
async fn require_loopback(request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host_ok = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(is_loopback_authority);
    if !host_ok {
        return ApiError::forbidden_host().into_response();
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        if !origin.to_str().ok().is_some_and(is_loopback_origin) {
            return ApiError::forbidden_origin().into_response();
        }
    }
    next.run(request).await
}

/// The pilot HTTP router over `state`. `POST /api/sources` carries its own body limit, set on
/// its route and therefore overriding the router-wide default of [`MAX_JSON_BODY_BYTES`].
pub fn router(state: ApiState) -> Router {
    router_with_ui(state, None)
}

/// [`router`] that also serves the built UI from `ui_dir` on the same origin (local install):
/// a non-API path that names no file returns the UI's `index.html`, so client-side routes load
/// the app; `/api` paths keep the typed envelope. Without `ui_dir` it is exactly [`router`].
pub fn router_with_ui(state: ApiState, ui_dir: Option<&Path>) -> Router {
    let api = Router::new()
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
        .merge(stages::routes());
    let api = match ui_dir {
        None => api.fallback(route_not_found),
        Some(dir) => {
            let files = tower_http::services::ServeDir::new(dir)
                .fallback(tower_http::services::ServeFile::new(dir.join("index.html")));
            api.fallback(move |request: Request| {
                let files = files.clone();
                async move {
                    let path = request.uri().path();
                    if path == "/api" || path.starts_with("/api/") {
                        return ApiError::route_not_found().into_response();
                    }
                    match tower::ServiceExt::oneshot(files, request).await {
                        Ok(response) => response.into_response(),
                        Err(never) => match never {},
                    }
                }
            })
        }
    };
    api.method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .layer(map_response(|response: Response| async move {
            typed_framework_error(MAX_JSON_BODY_BYTES, response)
        }))
        .layer(from_fn(trace_request))
        .layer(from_fn(require_loopback))
        .with_state(state)
}
