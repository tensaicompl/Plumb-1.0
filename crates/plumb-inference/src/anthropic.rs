//! The Anthropic Messages API provider (plan X1.1, Hotfix 057), behind the `anthropic` feature.
//!
//! The provider implements the material-bound [`InferenceProvider`] trait (Hotfix 066). It
//! consumes only the request plus the hash-bound [`InferenceRequestMaterial`] supplied with that
//! exact call (it owns connection configuration only, never future request material), sends
//! the exact prompt as the system text and the canonical `{context, output_schema}` envelope as
//! the single user message, and validates the model's strict-JSON text locally against the exact
//! Draft 2020-12 Plumb schema. It never translates schemas into vendor constrained output,
//! repairs output, retries, reads files or the environment, or touches a graph or store.

use std::fmt;
use std::time::Duration;

use plumb_core::{to_canonical_json, CanonicalJson, Hash};
use reqwest::blocking::Client;
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

const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_DIAGNOSTIC_CHARS: usize = 1000;

/// The exact `ProviderPolicy.config` of adapter version 1.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnthropicConfig {
    adapter_version: u64,
    model: String,
    max_tokens: u64,
    api_version: String,
    #[serde(default)]
    workspace_id: Option<String>,
}

fn is_clean(text: &str) -> bool {
    !text.is_empty() && text.trim() == text && !text.chars().any(char::is_control)
}

fn config_error(reason: impl Into<String>) -> ProviderError {
    ProviderError::InvalidProviderConfig {
        reason: reason.into(),
    }
}

fn decode_config(config: &Value) -> Result<AnthropicConfig, ProviderError> {
    if config.get("workspace_id").is_some_and(Value::is_null) {
        return Err(config_error("workspace_id must be a string when present"));
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
    Ok(decoded)
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_DIAGNOSTIC_CHARS).collect()
}

fn malformed(reason: impl Into<String>) -> ProviderError {
    ProviderError::MalformedResponse {
        reason: reason.into(),
    }
}

/// The Anthropic Messages API provider.
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    endpoint: Url,
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
        let client = Client::builder()
            .timeout(TIMEOUT)
            .redirect(redirect::Policy::none())
            .build()
            .map_err(|e| ProviderError::Transport {
                reason: e.without_url().to_string(),
            })?;
        Ok(AnthropicProvider {
            client,
            api_key,
            endpoint,
        })
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
        let body = to_canonical_json(&json!({
            "model": config.model,
            "max_tokens": config.max_tokens,
            "system": material.prompt,
            "messages": [{"role": "user", "content": user}],
        }))
        .map_err(|e| config_error(e.to_string()))?;

        let mut http = self
            .client
            .post(self.endpoint.clone())
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("anthropic-version", &config.api_version)
            .header("content-type", JSON_MEDIA_TYPE);
        if let Some(workspace) = &config.workspace_id {
            http = http.header("anthropic-workspace-id", workspace);
        }
        let transport = |e: reqwest::Error| ProviderError::Transport {
            reason: e.without_url().to_string(),
        };
        let response = http.body(body).send().map_err(transport)?;
        let status = response.status();
        let header_request_id = response
            .headers()
            .get("request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let raw = response.bytes().map_err(transport)?.to_vec();
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
        if let Err(errors) = material.compiled.validate(&output) {
            let mut errors: Vec<String> = errors.map(|e| e.to_string()).collect();
            errors.sort();
            return Err(ProviderError::SchemaInvalidOutput { errors });
        }

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
                parameters: policy.config.clone(),
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
