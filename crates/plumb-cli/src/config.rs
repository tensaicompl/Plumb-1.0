//! The CLI configuration (A0.4, Hotfix 067): one strict JSON file with exactly the frozen
//! fields, the calendar conversion, and the inference provider built once at startup from
//! `--provider` (null, replay fixtures, or the Anthropic key file).
//!
//! Nothing is read from the environment. The Anthropic key is read once from its file, never
//! printed, logged, hashed or stored in any configuration value.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use plumb_api::application::ApplicationRuntime;
use plumb_api::runtime::{
    CalendarConfig, DirectManagerFact, DirectManagerPredicates, GateInputsConfig, ProviderMode,
    WEEKDAY_NAMES,
};
use plumb_api::ApiIdentity;
use plumb_core::{to_canonical_json, Hash, Id};
use plumb_functional::{
    AuthorizationDraft, CalculationScope, DirectoryDraft, DuplicatePolicy, ExprCalendarDefinition,
    ExprStaticCalendarProvider, OperationScope, ProcessScope, ScenarioDerivationInputs,
    StakeholderRoutingConfig, VerificationApplicabilityInputs, VocabularyPolicy,
};
use plumb_inference::{
    InferenceProvider, MockProvider, NullProvider, ProviderExecution, ProviderPolicy,
};
use plumb_session::SessionTurnConfig;
use plumb_sim::{
    ExecutionCalculationScopeOverride, ExecutionExpressionScopeInput, RuntimeSemanticValue,
};
use plumb_validation::ValidationPolicy;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use time::{Date, Month, Weekday};

/// The exact top-level fields of the configuration file.
const FIELDS: [&str; 21] = [
    "version",
    "identity",
    "stage_actor_ref",
    "assistant_actor_ref",
    "provider_policy",
    "session_turn",
    "validation_policy",
    "calendars",
    "direct_managers",
    "expression_scopes",
    "calculation_scope_overrides",
    "runtime_semantic_values",
    "vocabulary_policy",
    "duplicate_policy",
    "calculation_scope",
    "operation_scope",
    "process_scope",
    "authorization_draft",
    "scenario_derivation",
    "verification_applicability",
    "gates",
];

/// The optional top-level fields (R0.6): absent means not configured; when present each is a
/// complete typed object (never `null`, never defaulted).
const OPTIONAL_FIELDS: [&str; 2] = ["directory", "question_routing"];

/// The exact fields of `gates`: only the genuinely external F1 and F4 inputs (R0.4). F2 and
/// every graph-derived restatement are assembled from the PSG and rejected as unknown fields.
const GATE_FIELDS: [&str; 2] = ["f1", "f4"];

/// Why the configuration, the provider or the key file cannot be used.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("configuration: {0}")]
    Invalid(String),
    #[error("replay fixtures: {0}")]
    Fixtures(String),
    /// The Anthropic provider is not compiled in (`E_PROVIDER_UNAVAILABLE`).
    #[error("the anthropic provider needs the llm feature")]
    #[cfg_attr(feature = "llm", allow(dead_code))]
    ProviderUnavailable,
    /// The key file is unusable; the message never contains key material.
    #[error("anthropic key file: {0}")]
    KeyFile(&'static str),
    /// `serve --listen` names a non-loopback address (`E_NON_LOOPBACK_BIND`, architecture
    /// §33.8): the pilot has no user authentication and binds only to 127.0.0.1 or [::1].
    #[error("E_NON_LOOPBACK_BIND: serve --listen {0} is not a loopback address: the pilot server binds only to 127.0.0.1 or [::1]")]
    NonLoopbackBind(std::net::SocketAddr),
}

/// The configured identity.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityConfig {
    pub user: String,
    pub actor_ref: Id,
    pub roles: Vec<String>,
    pub mode: String,
}

/// The decoded configuration file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliConfig {
    pub version: u32,
    pub identity: IdentityConfig,
    pub stage_actor_ref: Id,
    pub assistant_actor_ref: Id,
    pub provider_policy: ProviderPolicy,
    pub session_turn: SessionTurnConfig,
    pub validation_policy: ValidationPolicy,
    pub calendars: Vec<CalendarConfig>,
    pub direct_managers: Vec<DirectManagerFact>,
    pub expression_scopes: Vec<ExecutionExpressionScopeInput>,
    pub calculation_scope_overrides: Vec<ExecutionCalculationScopeOverride>,
    pub runtime_semantic_values: Vec<RuntimeSemanticValue>,
    pub vocabulary_policy: VocabularyPolicy,
    pub duplicate_policy: DuplicatePolicy,
    pub calculation_scope: Option<CalculationScope>,
    pub operation_scope: Option<OperationScope>,
    pub process_scope: Option<ProcessScope>,
    pub authorization_draft: Option<AuthorizationDraft>,
    pub scenario_derivation: Option<ScenarioDerivationInputs>,
    pub verification_applicability: Option<VerificationApplicabilityInputs>,
    pub gates: GateInputsConfig,
    /// The configured semantic directory (R0.6); absent means not configured.
    #[serde(default)]
    pub directory: Option<DirectoryDraft>,
    /// The S3.1 stakeholder routing of `ask` (R0.6); absent means not configured.
    #[serde(default)]
    pub question_routing: Option<StakeholderRoutingConfig>,
}

/// A JSON document decoded exactly as [`Value`], except that an object with a duplicate member
/// name at any depth is an error (never "last one wins").
struct StrictJson(Value);

impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer
            .deserialize_any(StrictJsonVisitor)
            .map(StrictJson)
    }
}

struct StrictJsonVisitor;

impl<'de> serde::de::Visitor<'de> for StrictJsonVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::from(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::from(v))
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(StrictJson(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let StrictJson(value) = map.next_value()?;
            if object.contains_key(&key) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate object key {key:?}"
                )));
            }
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}

/// Requires `value` to be an object with exactly `fields` (nullable values must be present).
fn exact_fields(value: &Value, fields: &[&str], what: &str) -> Result<(), ConfigError> {
    exact_fields_with(value, fields, &[], what)
}

/// [`exact_fields`] that also admits the `optional` fields, which may be absent.
fn exact_fields_with(
    value: &Value,
    fields: &[&str],
    optional: &[&str],
    what: &str,
) -> Result<(), ConfigError> {
    let Value::Object(map) = value else {
        return Err(ConfigError::Invalid(format!(
            "{what} must be a JSON object"
        )));
    };
    let present: BTreeSet<&str> = map
        .keys()
        .map(String::as_str)
        .filter(|k| !optional.contains(k))
        .collect();
    let expected: BTreeSet<&str> = fields.iter().copied().collect();
    if let Some(unknown) = present.difference(&expected).next() {
        return Err(ConfigError::Invalid(format!(
            "{what} has unknown field {unknown:?}"
        )));
    }
    if let Some(missing) = expected.difference(&present).next() {
        return Err(ConfigError::Invalid(format!(
            "{what} is missing field {missing:?}"
        )));
    }
    Ok(())
}

impl CliConfig {
    /// Reads and strictly decodes the configuration file at `path`.
    pub fn load(path: &Path) -> Result<CliConfig, ConfigError> {
        let bytes = std::fs::read(path)
            .map_err(|e| ConfigError::Invalid(format!("cannot read {}: {e}", path.display())))?;
        CliConfig::parse(&bytes)
    }

    /// Strictly decodes configuration bytes: JSON without duplicate object keys at any depth
    /// (architecture §33.9), exactly the frozen fields, version 1, and every value in its
    /// existing typed serde form.
    pub fn parse(bytes: &[u8]) -> Result<CliConfig, ConfigError> {
        let StrictJson(value) = serde_json::from_slice(bytes)
            .map_err(|e| ConfigError::Invalid(format!("not strict JSON: {e}")))?;
        exact_fields_with(&value, &FIELDS, &OPTIONAL_FIELDS, "the configuration")?;
        exact_fields(&value["gates"], &GATE_FIELDS, "gates")?;
        for field in OPTIONAL_FIELDS {
            if value.get(field).is_some_and(|v| !v.is_object()) {
                return Err(ConfigError::Invalid(format!(
                    "{field} must be a JSON object when present"
                )));
            }
        }
        let config: CliConfig =
            serde_json::from_value(value).map_err(|e| ConfigError::Invalid(e.to_string()))?;
        if config.version != 1 {
            return Err(ConfigError::Invalid(format!(
                "version must be 1, not {}",
                config.version
            )));
        }
        if let Some(routing) = &config.question_routing {
            routing
                .validate()
                .map_err(|e| ConfigError::Invalid(format!("question_routing: {e}")))?;
        }
        if let Some(directory) = &config.directory {
            directory
                .validate()
                .map_err(|e| ConfigError::Invalid(format!("directory: {e}")))?;
            for calendar in &directory.calendars {
                if !config
                    .calendars
                    .iter()
                    .any(|c| c.calendar_ref == calendar.calendar_ref)
                {
                    return Err(ConfigError::Invalid(format!(
                        "directory: calendar {} names unknown calendar {}",
                        calendar.id, calendar.calendar_ref
                    )));
                }
            }
        }
        Ok(config)
    }

    /// The configured identity.
    pub fn identity(&self) -> Result<ApiIdentity, ConfigError> {
        let i = &self.identity;
        ApiIdentity::new(
            i.user.clone(),
            i.actor_ref.clone(),
            i.roles.clone(),
            i.mode.clone(),
        )
        .map_err(|e| ConfigError::Invalid(format!("identity: {e}")))
    }

    /// The validated bootstrap actors of `init` (R0.1): the identity and the stage and
    /// assistant actor refs, which must be pairwise distinct.
    pub fn bootstrap_actors(&self) -> Result<(ApiIdentity, Id, Id), ConfigError> {
        let identity = self.identity()?;
        let actors: BTreeSet<&Id> = [
            &self.identity.actor_ref,
            &self.stage_actor_ref,
            &self.assistant_actor_ref,
        ]
        .into_iter()
        .collect();
        if actors.len() != 3 {
            return Err(ConfigError::Invalid(
                "identity.actor_ref, stage_actor_ref and assistant_actor_ref must be pairwise \
                 distinct"
                    .into(),
            ));
        }
        Ok((
            identity,
            self.stage_actor_ref.clone(),
            self.assistant_actor_ref.clone(),
        ))
    }

    /// The typed application runtime with the given provider.
    pub fn runtime(&self, provider: Provider) -> Result<ApplicationRuntime, ConfigError> {
        // Calculations reference the PSG Calendar element (the directory ID), so each
        // directory calendar is also registered under that ID with its runtime definition.
        let mut runtime_calendars = self.calendars.clone();
        for calendar in self.directory.iter().flat_map(|d| &d.calendars) {
            if calendar.id == calendar.calendar_ref {
                continue;
            }
            if let Some(runtime) = self
                .calendars
                .iter()
                .find(|c| c.calendar_ref == calendar.calendar_ref)
            {
                let mut alias = runtime.clone();
                alias.calendar_ref = calendar.id.clone();
                runtime_calendars.push(alias);
            }
        }
        let calendars = calendar_provider(&runtime_calendars)?;
        let predicates = DirectManagerPredicates::new(&self.direct_managers)
            .map_err(|e| ConfigError::Invalid(e.to_string()))?;
        Ok(ApplicationRuntime {
            inference_provider: provider.provider,
            provider_mode: provider.mode,
            provider_policy: self.provider_policy.clone(),
            session_turn_config: self.session_turn,
            assistant_actor_ref: self.assistant_actor_ref.clone(),
            stage_actor_ref: self.stage_actor_ref.clone(),
            calendars: Arc::new(calendars),
            predicates: Arc::new(predicates),
            calendar_configs: self.calendars.clone(),
            direct_managers: self.direct_managers.clone(),
            expression_scopes: self.expression_scopes.clone(),
            calculation_scope_overrides: self.calculation_scope_overrides.clone(),
            runtime_semantic_values: self.runtime_semantic_values.clone(),
            validation_policy: self.validation_policy.clone(),
            vocabulary_policy: self.vocabulary_policy.clone(),
            duplicate_policy: self.duplicate_policy.clone(),
            calculation_scope: self.calculation_scope.clone(),
            operation_scope: self.operation_scope.clone(),
            process_scope: self.process_scope.clone(),
            authorization_draft: self.authorization_draft.clone(),
            scenario_derivation: self.scenario_derivation.clone(),
            verification_applicability: self.verification_applicability.clone(),
            gates: self.gates.clone(),
            directory: self.directory.clone(),
            question_routing: self.question_routing.clone(),
        })
    }
}

/// The weekday of a lowercase English name.
fn weekday(name: &str) -> Option<Weekday> {
    const DAYS: [Weekday; 7] = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ];
    WEEKDAY_NAMES
        .iter()
        .position(|n| *n == name)
        .map(|i| DAYS[i])
}

/// The date of a `YYYY-MM-DD` string.
fn date(text: &str) -> Option<Date> {
    let b = text.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let year: i32 = text[0..4].parse().ok()?;
    let month: u8 = text[5..7].parse().ok()?;
    let day: u8 = text[8..10].parse().ok()?;
    if !text[0..4]
        .bytes()
        .chain(text[5..7].bytes())
        .chain(text[8..10].bytes())
        .all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()
}

/// Converts the configured calendars into one static calendar provider: valid weekday names
/// and dates, no duplicate calendar, weekday or holiday.
pub fn calendar_provider(
    calendars: &[CalendarConfig],
) -> Result<ExprStaticCalendarProvider, ConfigError> {
    let mut definitions = Vec::with_capacity(calendars.len());
    let mut ids = BTreeSet::new();
    for calendar in calendars {
        calendar
            .validate()
            .map_err(|e| ConfigError::Invalid(e.to_string()))?;
        if !ids.insert(&calendar.calendar_ref) {
            return Err(ConfigError::Invalid(format!(
                "calendar {} is defined twice",
                calendar.calendar_ref
            )));
        }
        let weekend = calendar
            .weekend
            .iter()
            .map(|d| {
                weekday(d)
                    .ok_or_else(|| ConfigError::Invalid(format!("{d:?} is not a weekday name")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let holidays = calendar
            .holidays
            .iter()
            .map(|d| {
                date(d).ok_or_else(|| {
                    ConfigError::Invalid(format!(
                        "calendar {}: {d:?} is not a valid YYYY-MM-DD date",
                        calendar.calendar_ref
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        definitions.push(ExprCalendarDefinition::new(
            calendar.calendar_ref.clone(),
            weekend,
            holidays,
        ));
    }
    ExprStaticCalendarProvider::new(definitions).map_err(|e| ConfigError::Invalid(format!("{e:?}")))
}

/// The inference provider of one CLI process and its hash-bound mode.
pub struct Provider {
    pub provider: Arc<dyn InferenceProvider + Send + Sync>,
    pub mode: ProviderMode,
}

/// The provider selection given on the command line (conflicts already rejected).
pub enum ProviderChoice<'a> {
    Null,
    Replay { fixtures: &'a Path },
    Anthropic { key_file: &'a Path },
}

impl Provider {
    /// Builds the provider once.
    pub fn build(choice: ProviderChoice<'_>) -> Result<Provider, ConfigError> {
        match choice {
            ProviderChoice::Null => Ok(Provider {
                provider: Arc::new(NullProvider),
                mode: ProviderMode::Null,
            }),
            ProviderChoice::Replay { fixtures } => replay(fixtures),
            ProviderChoice::Anthropic { key_file } => anthropic(key_file),
        }
    }
}

/// `replay`: a strict JSON array of `ProviderExecution` keyed by `artifact.request_hash`
/// (duplicates rejected); the mode binds the canonical hash of the executions by key.
fn replay(path: &Path) -> Result<Provider, ConfigError> {
    let bytes = std::fs::read(path)
        .map_err(|e| ConfigError::Fixtures(format!("cannot read {}: {e}", path.display())))?;
    let mut executions: Vec<ProviderExecution> =
        serde_json::from_slice(&bytes).map_err(|e| ConfigError::Fixtures(e.to_string()))?;
    let provider = MockProvider::with_fixtures(
        executions
            .iter()
            .map(|e| (e.artifact.request_hash.clone(), e.clone())),
    )
    .map_err(|e| ConfigError::Fixtures(format!("{e:?}")))?;
    executions.sort_by(|a, b| a.artifact.request_hash.cmp(&b.artifact.request_hash));
    let canonical =
        to_canonical_json(&executions).map_err(|e| ConfigError::Fixtures(e.to_string()))?;
    Ok(Provider {
        provider: Arc::new(provider),
        mode: ProviderMode::Replay {
            fixtures_hash: Hash::content_sha256(&canonical),
        },
    })
}

/// Reads the Anthropic key once: exactly one trailing `\n` or `\r\n` is removed; an empty key,
/// surrounding whitespace or any control character is rejected. Errors never include the key.
#[cfg_attr(not(feature = "llm"), allow(dead_code))]
pub fn read_key_file(path: &Path) -> Result<String, ConfigError> {
    let bytes = std::fs::read(path).map_err(|_| ConfigError::KeyFile("cannot be read"))?;
    let text = String::from_utf8(bytes).map_err(|_| ConfigError::KeyFile("is not UTF-8"))?;
    let key = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(&text);
    if key.is_empty() {
        return Err(ConfigError::KeyFile("is empty"));
    }
    if key.trim() != key {
        return Err(ConfigError::KeyFile("has surrounding whitespace"));
    }
    if key.chars().any(char::is_control) {
        return Err(ConfigError::KeyFile("contains a control character"));
    }
    Ok(key.to_owned())
}

#[cfg(feature = "llm")]
fn anthropic(key_file: &Path) -> Result<Provider, ConfigError> {
    let key = read_key_file(key_file)?;
    let provider = plumb_inference::AnthropicProvider::new(key)
        .map_err(|_| ConfigError::KeyFile("is not accepted by the provider"))?;
    Ok(Provider {
        provider: Arc::new(provider),
        mode: ProviderMode::Anthropic,
    })
}

#[cfg(not(feature = "llm"))]
fn anthropic(_key_file: &Path) -> Result<Provider, ConfigError> {
    Err(ConfigError::ProviderUnavailable)
}
