//! Typed HTTP errors of the pilot runtime (plan A0.2, Hotfix 063; A0.3, Hotfix 065).
//!
//! Every error body is `{code, message, details}`; `412` additionally carries the current
//! branch-head `revision` and `semantic_hash`. Stable subsystem error names may appear inside
//! `details`; Rust debug output, panic text, SQLite messages and paths never form the contract.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::{json, Value};

use crate::openapi::PreconditionFailedResponse;

/// A typed API error with its HTTP status and stable code.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError(Box<ApiErrorBody>);

/// The content of an [`ApiError`].
#[derive(Debug, Clone, PartialEq)]
pub struct ApiErrorBody {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Value,
    /// The current branch head, present exactly for `412`.
    pub head: Option<(String, String)>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
    details: &'a Value,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError(Box::new(ApiErrorBody {
            status,
            code,
            message: message.into(),
            details: json!({}),
            head: None,
        }))
    }

    /// The error content.
    pub fn body(&self) -> &ApiErrorBody {
        &self.0
    }

    /// Attaches structured details.
    pub fn with_details(mut self, details: Value) -> ApiError {
        self.0.details = details;
        self
    }

    /// 400 `E_INVALID_REQUEST`: malformed path, query, body, header or multipart input.
    pub fn invalid(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, "E_INVALID_REQUEST", message)
    }

    /// 400 with a frozen input code.
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, code, message)
    }

    /// 400 `E_CURSOR_INVALID`.
    pub fn cursor_invalid(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, "E_CURSOR_INVALID", message)
    }

    /// 400 `E_CURSOR_REQUEST_MISMATCH`.
    pub fn cursor_request_mismatch() -> ApiError {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "E_CURSOR_REQUEST_MISMATCH",
            "the cursor belongs to another request",
        )
    }

    /// 409 `E_CURSOR_BASELINE_CHANGED`.
    pub fn cursor_baseline_changed() -> ApiError {
        ApiError::new(
            StatusCode::CONFLICT,
            "E_CURSOR_BASELINE_CHANGED",
            "the cursor was issued at another branch-head revision or semantic hash",
        )
    }

    /// 404 `E_NOT_FOUND`.
    pub fn not_found(what: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "E_NOT_FOUND", what)
    }

    /// 409 with a frozen conflict code.
    pub fn conflict(code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::CONFLICT, code, message)
    }

    /// 412 `E_PRECONDITION_FAILED` carrying the current head.
    pub fn precondition_failed(revision: String, semantic_hash: String) -> ApiError {
        let mut error = ApiError::new(
            StatusCode::PRECONDITION_FAILED,
            "E_PRECONDITION_FAILED",
            "If-Match is not the current semantic_hash",
        );
        error.0.head = Some((revision, semantic_hash));
        error
    }

    /// 415 `E_UNSUPPORTED_MEDIA_TYPE`.
    pub fn unsupported_media_type(media_type: &str) -> ApiError {
        ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "E_UNSUPPORTED_MEDIA_TYPE",
            "unsupported source media type",
        )
        .with_details(json!({ "media_type": media_type }))
    }

    /// 422 with a frozen semantic code.
    pub fn unprocessable(code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
    }

    /// 422 `E_SEMANTIC_INVARIANT` for a domain-service rejection or a target in the wrong
    /// lifecycle state; `error` is the stable error name of the rejecting service.
    pub fn semantic(error: &str, message: impl Into<String>) -> ApiError {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_SEMANTIC_INVARIANT",
            message,
        )
        .with_details(json!({ "error": error }))
    }

    /// 403 with a frozen code.
    pub fn forbidden(code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::FORBIDDEN, code, message)
    }

    /// 502 `E_INFERENCE_FAILED`: provider execution or provider-output inspection failed;
    /// `error` is the stable error name.
    pub fn inference_failed(error: &str) -> ApiError {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "E_INFERENCE_FAILED",
            "the inference provider or its output inspection failed",
        )
        .with_details(json!({ "error": error }))
    }

    /// 409 `E_INFERENCE_REQUIRED` (R0.2, architecture §20): under the `null` provider mode a
    /// recipe phase plans `Required` inference requests, so it fails before any provider call
    /// or artifact write. `task_kinds` are the phase's sorted, unique required task kinds.
    pub fn inference_required(recipe: &str, phase: &str, task_kinds: &[&str]) -> ApiError {
        ApiError::conflict(
            "E_INFERENCE_REQUIRED",
            format!(
                "phase {phase} of the {recipe} recipe requires inference, and the null provider is configured"
            ),
        )
        .with_details(json!({ "recipe": recipe, "phase": phase, "task_kinds": task_kinds }))
    }

    /// 503 `E_A03_RUNTIME_UNAVAILABLE`: the injected A0.3 runtime lacks `capability`.
    pub fn runtime_unavailable(capability: &str) -> ApiError {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "E_A03_RUNTIME_UNAVAILABLE",
            "this operation needs runtime material that is not configured",
        )
        .with_details(json!({ "capability": capability }))
    }

    /// 500 `E_WORKFLOW_CORRUPT`: impossible or corrupt persisted workflow state.
    pub fn workflow_corrupt(what: &str) -> ApiError {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "E_WORKFLOW_CORRUPT",
            "persisted workflow state is corrupt",
        )
        .with_details(json!({ "record": what }))
    }

    /// 500 `E_INTERNAL`; `subsystem` names the failing component, never its raw message.
    pub fn internal(subsystem: &str) -> ApiError {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "E_INTERNAL",
            "unexpected internal failure",
        )
        .with_details(json!({ "subsystem": subsystem }))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let error = *self.0;
        match error.head {
            Some((revision, semantic_hash)) => (
                error.status,
                Json(PreconditionFailedResponse {
                    code: error.code.to_owned(),
                    message: error.message,
                    details: error.details,
                    revision,
                    semantic_hash,
                }),
            )
                .into_response(),
            None => (
                error.status,
                Json(ErrorBody {
                    code: error.code,
                    message: &error.message,
                    details: &error.details,
                }),
            )
                .into_response(),
        }
    }
}
