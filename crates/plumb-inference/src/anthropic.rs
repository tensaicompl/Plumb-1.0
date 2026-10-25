//! The Anthropic Messages API provider (plan X1.1, Hotfix 057), behind the `anthropic` feature.
//!
//! The provider implements the material-bound [`InferenceProvider`] trait (Hotfix 066). It
//! consumes only the request plus the hash-bound [`InferenceRequestMaterial`] supplied with that
//! exact call (it owns connection configuration only, never future request material), sends
//! the exact prompt as the system text and the canonical `{context, output_schema}` envelope as
//! the single user message, and validates the model's strict-JSON text locally against the exact
//! Draft 2020-12 Plumb schema. It never translates schemas into vendor constrained output,
//! repairs output, reads files or the environment, or touches a graph or store; only a
//! transient status (429, 529, 5xx) is sent again, on the bounded [`RetryPolicy`] (H0.3).
//!
//! H0.8 (architecture §33.8): it authenticates with `x-api-key` (never `Authorization`), sends
//! the configured `anthropic-version` and `anthropic-workspace-id` exactly when configured,
//! follows no redirect, marks the key header sensitive and never shows the key. The total and
//! connect timeouts are validated policy values (bound into the request identity), and an
//! elapsed timeout is [`ProviderError::Timeout`], distinct from other transport failures.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use plumb_core::{to_canonical_json, CanonicalJson, Hash};
use reqwest::blocking::Client;
use reqwest::header::HeaderValue;
use reqwest::{redirect, Url};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::model::{InferenceArtifact, InferenceRequest, ProviderExecution, JSON_MEDIA_TYPE};
use crate::provider::{
    verify_binding, InferenceProvider, InferenceRequestMaterial, ProviderError, ProviderFailure,
    RejectedResponse,
};

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
    #[serde(default)]
    effort: Option<String>,
}

/// The `output_config.effort` levels the Messages API accepts.
const EFFORT_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// The validated adapter configuration with its effective timeouts.
struct EffectiveConfig {
    model: String,
    max_tokens: u64,
    api_version: String,
    workspace_id: Option<String>,
    request_timeout: Duration,
    connect_timeout: Duration,
    effort: Option<String>,
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
        ("effort", "one of low, medium, high, xhigh, max"),
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
    if decoded
        .effort
        .as_deref()
        .is_some_and(|e| !EFFORT_LEVELS.contains(&e))
    {
        return Err(config_error(
            "effort must be one of low, medium, high, xhigh, max",
        ));
    }
    Ok(EffectiveConfig {
        effort: decoded.effort,
        model: decoded.model,
        max_tokens: decoded.max_tokens,
        api_version: decoded.api_version,
        workspace_id: decoded.workspace_id,
        request_timeout: Duration::from_millis(request_timeout_ms),
        connect_timeout: Duration::from_millis(connect_timeout_ms),
    })
}

impl EffectiveConfig {
    /// The generation parameters of the request body: exactly `model`, `max_tokens` and, when
    /// configured, `output_config.effort`. No sampling parameter is ever sent, and these are
    /// exactly what the artifact records as its `parameters` (Fable #66); the structured-output
    /// format derives from the bound schema and is not a parameter.
    fn parameters(&self) -> Value {
        let mut parameters = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
        });
        if let Some(effort) = &self.effort {
            parameters["output_config"] = json!({"effort": effort});
        }
        parameters
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

/// The keywords the structured-outputs JSON Schema subset rejects on any schema node
/// (measured against the live API); they are dropped from the constraint sent to the API and
/// still enforced locally by the full output schema.
const UNSUPPORTED_SCHEMA_KEYWORDS: [&str; 7] = [
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "maxItems",
    "uniqueItems",
];

/// A JSON value whose objects keep their member order. Constrained decoding follows a
/// schema's property order, so the structured-output constraint is built from the committed
/// schema bytes in their authored order (never from a key-sorted [`Value`]).
#[derive(Debug, Clone, PartialEq)]
enum Ordered {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Ordered>),
    Object(Vec<(String, Ordered)>),
}

impl Ordered {
    fn get(&self, key: &str) -> Option<&Ordered> {
        match self {
            Ordered::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn keys(&self) -> Option<Vec<String>> {
        match self {
            Ordered::Object(members) => Some(members.iter().map(|(k, _)| k.clone()).collect()),
            _ => None,
        }
    }

    fn strings(&self) -> Option<Vec<String>> {
        match self {
            Ordered::Array(items) => items
                .iter()
                .map(|v| match v {
                    Ordered::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            _ => None,
        }
    }

    fn from_value(value: &Value) -> Ordered {
        match value {
            Value::Null => Ordered::Null,
            Value::Bool(b) => Ordered::Bool(*b),
            Value::Number(n) => Ordered::Number(n.clone()),
            Value::String(s) => Ordered::String(s.clone()),
            Value::Array(items) => Ordered::Array(items.iter().map(Ordered::from_value).collect()),
            Value::Object(map) => Ordered::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), Ordered::from_value(v)))
                    .collect(),
            ),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Ordered::Null => Value::Null,
            Ordered::Bool(b) => Value::Bool(*b),
            Ordered::Number(n) => Value::Number(n.clone()),
            Ordered::String(s) => Value::String(s.clone()),
            Ordered::Array(items) => Value::Array(items.iter().map(Ordered::to_value).collect()),
            Ordered::Object(members) => Value::Object(
                members
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_value()))
                    .collect(),
            ),
        }
    }
}

impl serde::Serialize for Ordered {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self {
            Ordered::Null => serializer.serialize_unit(),
            Ordered::Bool(b) => serializer.serialize_bool(*b),
            Ordered::Number(n) => n.serialize(serializer),
            Ordered::String(s) => serializer.serialize_str(s),
            Ordered::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Ordered::Object(members) => {
                let mut map = serializer.serialize_map(Some(members.len()))?;
                for (k, v) in members {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for Ordered {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Ordered, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Ordered;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_unit<E>(self) -> Result<Ordered, E> {
                Ok(Ordered::Null)
            }
            fn visit_bool<E>(self, b: bool) -> Result<Ordered, E> {
                Ok(Ordered::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<Ordered, E> {
                Ok(Ordered::Number(n.into()))
            }
            fn visit_u64<E>(self, n: u64) -> Result<Ordered, E> {
                Ok(Ordered::Number(n.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, n: f64) -> Result<Ordered, E> {
                serde_json::Number::from_f64(n)
                    .map(Ordered::Number)
                    .ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E>(self, s: &str) -> Result<Ordered, E> {
                Ok(Ordered::String(s.to_owned()))
            }
            fn visit_string<E>(self, s: String) -> Result<Ordered, E> {
                Ok(Ordered::String(s))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Ordered, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Ordered::Array(items))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Ordered, A::Error> {
                let mut members = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, Ordered>()? {
                    members.push((k, v));
                }
                Ok(Ordered::Object(members))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// The structured-outputs constraint for `schema`: the bound output schema reduced to the
/// subset the Messages API accepts (`oneOf` becomes `anyOf`, a `minItems` above 1 and every
/// keyword of [`UNSUPPORTED_SCHEMA_KEYWORDS`] are dropped, and object variants that require
/// the same keys are merged), or `None` when the schema has an object the subset cannot
/// express (every object must set `additionalProperties: false`). The reduction only widens
/// what the API admits; validation of the answer against the full schema is unchanged. A
/// [`Value`] has no member order, so the adapter itself reduces the committed schema bytes.
pub fn api_output_schema(schema: &Value) -> Option<Value> {
    reduce_schema(&Ordered::from_value(schema)).map(|o| o.to_value())
}

/// [`api_output_schema`] over the committed schema bytes, keeping their member order.
fn api_output_constraint(schema_json: &[u8]) -> Option<Ordered> {
    reduce_schema(&serde_json::from_slice::<Ordered>(schema_json).ok()?)
}

fn reduce_schema(node: &Ordered) -> Option<Ordered> {
    match node {
        Ordered::Object(members) => {
            let is_object = matches!(node.get("type"), Some(Ordered::String(t)) if t == "object")
                || node.get("properties").is_some();
            if is_object && node.get("additionalProperties") != Some(&Ordered::Bool(false)) {
                return None;
            }
            let mut out = Vec::with_capacity(members.len());
            for (key, value) in members {
                let key = key.as_str();
                if UNSUPPORTED_SCHEMA_KEYWORDS.contains(&key)
                    || (key == "minItems"
                        && matches!(value, Ordered::Number(n) if n.as_u64().is_some_and(|n| n > 1)))
                {
                    continue;
                }
                let value = match key {
                    // Property names and definition names are data, not keywords.
                    "properties" | "$defs" | "definitions" => {
                        let Ordered::Object(entries) = value else {
                            return None;
                        };
                        Ordered::Object(
                            entries
                                .iter()
                                .map(|(name, member)| Some((name.clone(), reduce_schema(member)?)))
                                .collect::<Option<Vec<_>>>()?,
                        )
                    }
                    "oneOf" | "anyOf" => {
                        let Ordered::Array(branches) = value else {
                            return None;
                        };
                        let branches = branches
                            .iter()
                            .map(reduce_schema)
                            .collect::<Option<Vec<_>>>()?;
                        Ordered::Array(merge_same_required(branches))
                    }
                    _ => reduce_schema(value)?,
                };
                let key = if key == "oneOf" { "anyOf" } else { key };
                out.push((key.to_owned(), value));
            }
            Some(Ordered::Object(out))
        }
        Ordered::Array(items) => items
            .iter()
            .map(reduce_schema)
            .collect::<Option<Vec<_>>>()
            .map(Ordered::Array),
        other => Some(other.clone()),
    }
}

/// The alternatives of an `anyOf` with every closed object variant that requires exactly its
/// own property keys merged into one object when all such variants share the same key set
/// (each property takes the union of the variants' schemas, in the first variant's property
/// order). This keeps the compiled grammar within the API's limits without making any
/// property optional; other variants are kept.
fn merge_same_required(branches: Vec<Ordered>) -> Vec<Ordered> {
    let shape = |branch: &Ordered| -> Option<BTreeSet<String>> {
        let required: BTreeSet<String> = branch.get("required")?.strings()?.into_iter().collect();
        let properties: BTreeSet<String> = branch.get("properties")?.keys()?.into_iter().collect();
        (properties == required).then_some(required)
    };
    let shapes: Vec<Option<BTreeSet<String>>> = branches.iter().map(shape).collect();
    let objects: Vec<&BTreeSet<String>> = shapes.iter().flatten().collect();
    if objects.len() < 2 || objects.iter().any(|k| *k != objects[0]) {
        return branches;
    }
    let mut order: Vec<String> = Vec::new();
    let mut required: Vec<String> = Vec::new();
    let mut alternatives: BTreeMap<String, Vec<Ordered>> = BTreeMap::new();
    let mut kept = Vec::new();
    for (branch, shape) in branches.into_iter().zip(&shapes) {
        if shape.is_none() {
            kept.push(branch);
            continue;
        }
        if required.is_empty() {
            required = branch
                .get("required")
                .and_then(Ordered::strings)
                .unwrap_or_default();
        }
        if let Some(Ordered::Object(properties)) = branch.get("properties") {
            for (name, schema) in properties {
                if !order.contains(name) {
                    order.push(name.clone());
                }
                let entry = alternatives.entry(name.clone()).or_default();
                let flattened = match schema {
                    Ordered::Object(m) if m.len() == 1 && m[0].0 == "anyOf" => match &m[0].1 {
                        Ordered::Array(items) => items.clone(),
                        _ => vec![schema.clone()],
                    },
                    _ => vec![schema.clone()],
                };
                for alternative in flattened {
                    if !entry.contains(&alternative) {
                        entry.push(alternative);
                    }
                }
            }
        }
    }
    let properties = order
        .into_iter()
        .map(|name| {
            let mut options = alternatives.remove(&name).unwrap_or_default();
            let schema = if options.len() == 1 {
                options.remove(0)
            } else {
                Ordered::Object(vec![("anyOf".to_owned(), Ordered::Array(options))])
            };
            (name, schema)
        })
        .collect();
    kept.push(Ordered::Object(vec![
        ("type".to_owned(), Ordered::String("object".to_owned())),
        ("additionalProperties".to_owned(), Ordered::Bool(false)),
        (
            "required".to_owned(),
            Ordered::Array(required.into_iter().map(Ordered::String).collect()),
        ),
        ("properties".to_owned(), Ordered::Object(properties)),
    ]));
    kept
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_DIAGNOSTIC_CHARS).collect()
}

fn malformed(reason: impl Into<String>) -> ProviderError {
    ProviderError::MalformedResponse {
        reason: reason.into(),
    }
}

/// The bounded retry schedule of transient provider statuses (H0.3, Fable #62): a 429, 529 or
/// other 5xx answer is sent again, unchanged, up to `max_attempts` sends in total, after the
/// provider's `retry-after` (seconds) when it is at most `max_retry_after`, otherwise after
/// `base_delay * 2^(n-1)` capped at `max_delay`. A `retry-after` beyond the bound ends the
/// call with the typed remote error. Every other status, transport failure, timeout (the
/// generation may have been billed) and output rejection is final at once; output is never
/// repaired. The schedule is connection behaviour only, never part of a request identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub max_retry_after: Duration,
}

impl Default for RetryPolicy {
    fn default() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(8),
            max_retry_after: Duration::from_secs(60),
        }
    }
}

impl RetryPolicy {
    /// The wait before send `attempt + 1` after a transient answer, or `None` to give up.
    fn delay(&self, attempt: u32, retry_after: Option<&str>) -> Option<Duration> {
        if attempt >= self.max_attempts {
            return None;
        }
        if let Some(seconds) = retry_after.and_then(|v| v.trim().parse::<u64>().ok()) {
            let wait = Duration::from_secs(seconds);
            return (wait <= self.max_retry_after).then_some(wait);
        }
        let factor = 1u32 << (attempt.saturating_sub(1)).min(16);
        Some(self.base_delay.saturating_mul(factor).min(self.max_delay))
    }
}

/// 429, 529 and every other 5xx are transient.
fn is_transient(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// The Anthropic Messages API provider. It holds the credential, endpoint and retry schedule
/// only; each call builds its HTTP client from the request's validated policy (connect timeout,
/// redirects disabled), so every timeout is bound into the request identity.
pub struct AnthropicProvider {
    api_key: String,
    endpoint: Url,
    retry: RetryPolicy,
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
        Ok(AnthropicProvider {
            api_key,
            endpoint,
            retry: RetryPolicy::default(),
        })
    }

    /// This provider with another transient-status retry schedule.
    pub fn with_retry_policy(mut self, retry: RetryPolicy) -> AnthropicProvider {
        self.retry = retry;
        self
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
        self.execute_detailed(request, material)
            .map_err(|failure| failure.error)
    }

    /// Fable #253: an answered call whose answer is rejected keeps the exact raw answer.
    fn execute_detailed(
        &self,
        request: &InferenceRequest,
        material: &InferenceRequestMaterial,
    ) -> Result<ProviderExecution, ProviderFailure> {
        // The binding is validated before any provider-specific work and before any network.
        let schema_json = &material.schema_json;
        let material = verify_binding(request, material)?;
        let policy = &request.provider_policy;
        if policy.provider != ANTHROPIC_PROVIDER {
            return Err(ProviderError::UnsupportedProvider {
                provider: policy.provider.clone(),
            }
            .into());
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
        // Structured outputs: the bound output schema constrains decoding, so the answer is
        // JSON of that shape (never fenced prose); it is still validated in full below. The
        // constraint keeps the committed schema's member order (decoding follows it), so the
        // body is written from an order-preserving value rather than canonicalized.
        let mut body = Ordered::from_value(&body);
        if let (Some(constraint), Ordered::Object(members)) =
            (api_output_constraint(schema_json), &mut body)
        {
            let format = Ordered::Object(vec![
                ("type".to_owned(), Ordered::String("json_schema".to_owned())),
                ("schema".to_owned(), constraint),
            ]);
            match members.iter_mut().find(|(k, _)| k == "output_config") {
                Some((_, Ordered::Object(output_config))) => {
                    output_config.push(("format".to_owned(), format));
                }
                _ => members.push((
                    "output_config".to_owned(),
                    Ordered::Object(vec![("format".to_owned(), format)]),
                )),
            }
        }
        let body = serde_json::to_vec(&body).map_err(|e| config_error(e.to_string()))?;

        let client = Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .redirect(redirect::Policy::none())
            .build()
            .map_err(transport_error)?;
        let mut attempt = 0;
        let (raw, content_type) = loop {
            attempt += 1;
            let mut http = client
                .post(self.endpoint.clone())
                .header("x-api-key", self.key_header()?)
                .header("anthropic-version", &config.api_version)
                .header("content-type", JSON_MEDIA_TYPE);
            if let Some(workspace) = &config.workspace_id {
                http = http.header("anthropic-workspace-id", workspace);
            }
            let response = http.body(body.clone()).send().map_err(transport_error)?;
            let status = response.status();
            let header = |name: &str| {
                response
                    .headers()
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned)
            };
            let header_request_id = header("request-id");
            let retry_after = header("retry-after");
            let content_type = header("content-type");
            let raw = response.bytes().map_err(transport_error)?.to_vec();
            if status.is_success() {
                break (raw, content_type);
            }
            let error = remote_error(status.as_u16(), header_request_id, &raw);
            match is_transient(status.as_u16())
                .then(|| self.retry.delay(attempt, retry_after.as_deref()))
                .flatten()
            {
                Some(wait) => std::thread::sleep(wait),
                None => return Err(error.into()),
            }
        };
        let media_type = content_type
            .and_then(|t| t.split(';').next().map(|t| t.trim().to_ascii_lowercase()))
            .filter(|t| !t.is_empty() && is_clean(t))
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        let rejected = |error: ProviderError| ProviderFailure {
            error,
            rejected_response: Some(Box::new(RejectedResponse {
                media_type: media_type.clone(),
                bytes: raw.clone(),
            })),
        };
        let inspect = || -> Result<ProviderExecution, ProviderError> {
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
                raw_response: raw.clone(),
            };
            execution
                .validate_for(request)
                .map_err(ProviderError::InvalidProviderExecution)?;
            Ok(execution)
        };
        inspect().map_err(rejected)
    }
}
