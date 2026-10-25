//! The Anthropic Messages API provider (plan X1.1, Hotfix 057), behind the `anthropic` feature.
//!
//! The provider implements the material-bound [`InferenceProvider`] trait (Hotfix 066). It
//! consumes only the request plus the hash-bound [`InferenceRequestMaterial`] supplied with that
//! exact call (it owns connection configuration only, never future request material), sends
//! the exact prompt as the system text and the canonical `{context, output_schema}` envelope as
//! the single user message, and validates the model's strict-JSON text locally against the exact
//! Draft 2020-12 Plumb schema. It never translates schemas into vendor constrained output,
//! repairs output, retries, reads files or the environment, or touches a graph or store.
//!
//! H0.8 (architecture §33.8): it authenticates with `x-api-key` (never `Authorization`), sends
//! the configured `anthropic-version` and `anthropic-workspace-id` exactly when configured,
//! follows no redirect, marks the key header sensitive and never shows the key. The total and
//! connect timeouts are validated policy values (bound into the request identity), and an
//! elapsed timeout is [`ProviderError::Timeout`], distinct from other transport failures.

use std::fmt;
use std::time::Duration;

use plumb_core::{to_canonical_json, CanonicalJson, Hash};
use reqwest::blocking::Client;
use reqwest::header::HeaderValue;
use reqwest::{redirect, Url};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::model::{InferenceArtifact, InferenceRequest, ProviderExecution, JSON_MEDIA_TYPE};
use crate::provider::{verify_binding, InferenceProvider, InferenceRequestMaterial, ProviderError};

/// The provider identifier this adapter serves.
pub const ANTHROPIC_PROVIDER: &str = "anthropic";
/// The default Messages API endpoint.
pub const ANTHROPIC_MESSAGES_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
/// The adapter version bound into `ProviderPolicy.config`.
pub const ANTHROPIC_ADAPTER_VERSION: u64 = 1;
/// The largest accepted `max_tokens`.
pub const ANTHROPIC_MAX_TOKENS_LIMIT: u64 = 131_072;

/// The largest accepted total request timeout (`request_timeout_ms`): two hours.
pub const ANTHROPIC_MAX_REQUEST_TIMEOUT_MS: u64 = 7_200_000;
/// The connect timeout used when `connect_timeout_ms` is not configured.
pub const ANTHROPIC_DEFAULT_CONNECT_TIMEOUT_MS: u64 = 10_000;
/// The fixed allowance of the default total timeout (connection, queueing, input processing).
const DEFAULT_TIMEOUT_BASE_MS: u64 = 120_000;
/// The per-output-token allowance of the default total timeout (a conservative ~33 tokens/s).
const DEFAULT_TIMEOUT_PER_TOKEN_MS: u64 = 30;
const MAX_DIAGNOSTIC_CHARS: usize = 1000;

/// The total request timeout used when `request_timeout_ms` is not configured: sized for the
/// configured `max_tokens` of a non-streaming request, so a long generation is not discarded
/// by the client after it was billed (Fable #63).
pub fn default_request_timeout_ms(max_tokens: u64) -> u64 {
    DEFAULT_TIMEOUT_BASE_MS
        .saturating_add(max_tokens.saturating_mul(DEFAULT_TIMEOUT_PER_TOKEN_MS))
        .min(ANTHROPIC_MAX_REQUEST_TIMEOUT_MS)
}

/// The exact `ProviderPolicy.config` of adapter version 1. The timeouts are part of the
/// policy, so they are bound into the request identity; omitted, they take their documented
/// defaults (which depend only on the other policy values).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnthropicConfig {
    adapter_version: u64,
    model: String,
    max_tokens: u64,
    api_version: String,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    request_timeout_ms: Option<u64>,
    #[serde(default)]
    connect_timeout_ms: Option<u64>,
}

/// The validated adapter configuration with its effective timeouts.
struct EffectiveConfig {
    model: String,
    max_tokens: u64,
    api_version: String,
    workspace_id: Option<String>,
    request_timeout: Duration,
    connect_timeout: Duration,
}

fn is_clean(text: &str) -> bool {
    !text.is_empty() && text.trim() == text && !text.chars().any(char::is_control)
}

fn config_error(reason: impl Into<String>) -> ProviderError {
    ProviderError::InvalidProviderConfig {
        reason: reason.into(),
    }
}

fn decode_config(config: &Value) -> Result<EffectiveConfig, ProviderError> {
    for (field, kind) in [
        ("workspace_id", "a string"),
        ("request_timeout_ms", "an integer"),
        ("connect_timeout_ms", "an integer"),
    ] {
        if config.get(field).is_some_and(Value::is_null) {
            return Err(config_error(format!("{field} must be {kind} when present")));
        }
    }
    let decoded: AnthropicConfig =
        serde_json::from_value(config.clone()).map_err(|e| config_error(e.to_string()))?;
    if decoded.adapter_version != ANTHROPIC_ADAPTER_VERSION {
        return Err(config_error("adapter_version must be 1"));
    }
    if !is_clean(&decoded.model) {
        return Err(config_error("model must be clean non-empty text"));
    }
    if !(1..=ANTHROPIC_MAX_TOKENS_LIMIT).contains(&decoded.max_tokens) {
        return Err(config_error("max_tokens must be within 1..=131072"));
    }
    if !is_clean(&decoded.api_version) {
        return Err(config_error("api_version must be clean non-empty text"));
    }
    if decoded
        .workspace_id
        .as_deref()
        .is_some_and(|w| !is_clean(w))
    {
        return Err(config_error("workspace_id must be clean non-empty text"));
    }
    let request_timeout_ms = decoded
        .request_timeout_ms
        .unwrap_or_else(|| default_request_timeout_ms(decoded.max_tokens));
    if !(1..=ANTHROPIC_MAX_REQUEST_TIMEOUT_MS).contains(&request_timeout_ms) {
        return Err(config_error(
            "request_timeout_ms must be within 1..=7200000",
        ));
    }
    let connect_timeout_ms = decoded
        .connect_timeout_ms
        .unwrap_or(ANTHROPIC_DEFAULT_CONNECT_TIMEOUT_MS.min(request_timeout_ms));
    if !(1..=request_timeout_ms).contains(&connect_timeout_ms) {
        return Err(config_error(
            "connect_timeout_ms must be within 1..=request_timeout_ms",
        ));
    }
    Ok(EffectiveConfig {
        model: decoded.model,
        max_tokens: decoded.max_tokens,
        api_version: decoded.api_version,
        workspace_id: decoded.workspace_id,
        request_timeout: Duration::from_millis(request_timeout_ms),
        connect_timeout: Duration::from_millis(connect_timeout_ms),
    })
}

impl EffectiveConfig {
    /// The generation parameters of the request body: exactly `model` and `max_tokens`. No
    /// sampling parameter is ever sent, and these are exactly what the artifact records as its
    /// `parameters` (Fable #66).
    fn parameters(&self) -> Value {
        json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
        })
    }
}

/// Validates an adapter-version-1 `ProviderPolicy.config` without any I/O (for eager
/// configuration checks and tests).
pub fn validate_config(config: &Value) -> Result<(), ProviderError> {
    decode_config(config).map(|_| ())
}

/// The generation parameters a valid `config` sends in the request body, as recorded in the
/// inference artifact.
pub fn request_parameters(config: &Value) -> Result<CanonicalJson, ProviderError> {
    decode_config(config).map(|c| CanonicalJson::new(c.parameters()))
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_DIAGNOSTIC_CHARS).collect()
}

fn malformed(reason: impl Into<String>) -> ProviderError {
    ProviderError::MalformedResponse {
        reason: reason.into(),
    }
}

/// The Anthropic Messages API provider. It holds the credential and endpoint only; each call
/// builds its HTTP client from the request's validated policy (connect timeout, redirects
/// disabled), so every timeout is bound into the request identity.
pub struct AnthropicProvider {
    api_key: String,
    endpoint: Url,
}

/// A reqwest failure as a typed provider error: a timeout is distinct from other transport
/// failures. The URL (and so any proxy or endpoint detail) is dropped; the key is never in it.
fn transport_error(error: reqwest::Error) -> ProviderError {
    let timed_out = is_timeout(&error);
    let reason = error.without_url().to_string();
    if timed_out {
        ProviderError::Timeout { reason }
    } else {
        ProviderError::Transport { reason }
    }
}

/// Whether a reqwest error (or any error in its source chain) is a timeout.
fn is_timeout(error: &reqwest::Error) -> bool {
    if error.is_timeout() {
        return true;
    }
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        if cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
        {
            return true;
        }
        source = cause.source();
    }
    false
}

impl fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnthropicProvider")
            .field("endpoint", &self.endpoint.as_str())
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl AnthropicProvider {
    /// A provider for the default endpoint with an explicitly supplied credential.
    pub fn new(api_key: impl Into<String>) -> Result<AnthropicProvider, ProviderError> {
        AnthropicProvider::with_endpoint(api_key, ANTHROPIC_MESSAGES_ENDPOINT)
    }

    /// A provider for an explicit http(s) endpoint (local tests or a proxy).
    pub fn with_endpoint(
        api_key: impl Into<String>,
        endpoint: &str,
    ) -> Result<AnthropicProvider, ProviderError> {
        let api_key = api_key.into();
        if !is_clean(&api_key) {
            return Err(ProviderError::InvalidSecret);
        }
        let invalid = |reason: &str| ProviderError::InvalidEndpoint {
            reason: reason.to_owned(),
        };
        let endpoint = Url::parse(endpoint).map_err(|_| invalid("not a URL"))?;
        if !matches!(endpoint.scheme(), "http" | "https") {
            return Err(invalid("scheme must be http or https"));
        }
        if !endpoint.username().is_empty() || endpoint.password().is_some() {
            return Err(invalid("embedded credentials are not allowed"));
        }
        if endpoint.host_str().is_none_or(str::is_empty) {
            return Err(invalid("missing host"));
        }
        Ok(AnthropicProvider { api_key, endpoint })
    }

    /// The credential as a sensitive header value (redacted from reqwest's own debug output).
    fn key_header(&self) -> Result<HeaderValue, ProviderError> {
        let mut value =
            HeaderValue::from_str(&self.api_key).map_err(|_| ProviderError::InvalidSecret)?;
        value.set_sensitive(true);
        Ok(value)
    }
}

/// A non-success response as a typed remote error with bounded diagnostics.
fn remote_error(status: u16, header_request_id: Option<String>, body: &[u8]) -> ProviderError {
    let parsed: Option<Value> = serde_json::from_slice(body).ok();
    let field = |pointer: &str| {
        parsed
            .as_ref()
            .and_then(|v| v.pointer(pointer))
            .and_then(Value::as_str)
            .map(bounded)
    };
    ProviderError::Remote {
        status,
        error_type: field("/error/type"),
        message: field("/error/message"),
        request_id: field("/request_id").or(header_request_id.map(|r| bounded(&r))),
    }
}

impl InferenceProvider for AnthropicProvider {
    fn execute(
        &self,
        request: &InferenceRequest,
        material: &InferenceRequestMaterial,
    ) -> Result<ProviderExecution, ProviderError> {
        // The binding is validated before any provider-specific work and before any network.
        let material = verify_binding(request, material)?;
        let policy = &request.provider_policy;
        if policy.provider != ANTHROPIC_PROVIDER {
            return Err(ProviderError::UnsupportedProvider {
                provider: policy.provider.clone(),
            });
        }
        let config = decode_config(policy.config.as_value())?;

        let envelope = to_canonical_json(&json!({
            "context": material.context,
            "output_schema": material.schema,
        }))
        .map_err(|e| config_error(e.to_string()))?;
        let user = String::from_utf8(envelope).map_err(|e| config_error(e.to_string()))?;
        let parameters = config.parameters();
        let mut body = parameters.clone();
        body["system"] = json!(material.prompt);
        body["messages"] = json!([{"role": "user", "content": user}]);
        let body = to_canonical_json(&body).map_err(|e| config_error(e.to_string()))?;

        let client = Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .redirect(redirect::Policy::none())
            .build()
            .map_err(transport_error)?;
        let mut http = client
            .post(self.endpoint.clone())
            .header("x-api-key", self.key_header()?)
            .header("anthropic-version", &config.api_version)
            .header("content-type", JSON_MEDIA_TYPE);
        if let Some(workspace) = &config.workspace_id {
            http = http.header("anthropic-workspace-id", workspace);
        }
        let response = http.body(body).send().map_err(transport_error)?;
        let status = response.status();
        let header_request_id = response
            .headers()
            .get("request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let raw = response.bytes().map_err(transport_error)?.to_vec();
        if !status.is_success() {
            return Err(remote_error(status.as_u16(), header_request_id, &raw));
        }

        let message: Value =
            serde_json::from_slice(&raw).map_err(|_| malformed("response is not JSON"))?;
        if message.get("type").and_then(Value::as_str) != Some("message") {
            return Err(malformed("type is not message"));
        }
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            return Err(malformed("role is not assistant"));
        }
        let model = message
            .get("model")
            .and_then(Value::as_str)
            .filter(|m| is_clean(m))
            .ok_or_else(|| malformed("model is missing or not clean text"))?
            .to_owned();
        let content = message
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| malformed("content is not an array"))?;
        let stop_reason = message
            .get("stop_reason")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed("stop_reason is missing"))?;
        match stop_reason {
            "end_turn" => {}
            "refusal" => return Err(ProviderError::Refusal),
            "max_tokens" => return Err(ProviderError::IncompleteOutput),
            other => {
                return Err(ProviderError::UnexpectedStopReason {
                    stop_reason: bounded(other),
                })
            }
        }
        let texts: Vec<&Value> = content
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .collect();
        let [text_block] = texts.as_slice() else {
            return Err(malformed(format!(
                "expected exactly one text block, found {}",
                texts.len()
            )));
        };
        let text = text_block
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed("text block has no text"))?;
        let output: Value =
            serde_json::from_str(text).map_err(|e| ProviderError::InvalidOutputJson {
                reason: e.to_string(),
            })?;
        material.validate_output(&output)?;

        let validated_output = CanonicalJson::new(output);
        let validated_output_hash =
            validated_output
                .content_hash()
                .map_err(|e| ProviderError::InvalidOutputJson {
                    reason: e.to_string(),
                })?;
        let execution = ProviderExecution {
            artifact: InferenceArtifact {
                request_hash: request.id.clone(),
                provider: policy.provider.clone(),
                model,
                parameters: CanonicalJson::new(parameters),
                raw_response_hash: Hash::content_sha256(&raw),
                validated_output,
                validated_output_hash,
            },
            raw_response_media_type: JSON_MEDIA_TYPE.to_owned(),
            raw_response: raw,
        };
        execution
            .validate_for(request)
            .map_err(ProviderError::InvalidProviderExecution)?;
        Ok(execution)
    }
}
