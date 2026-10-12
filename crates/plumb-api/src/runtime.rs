//! The validated A0.4 application runtime (Hotfix 067): the explicit, typed stage-orchestration
//! inputs of one application instance, injected when the state is built.
//!
//! It is additive to the A0.3 runtime (whose provider, provider policy, calendars, predicates,
//! expression scopes, calculation overrides and runtime semantic values it reuses unchanged) and
//! is never global, static, environment-discovered or built from request text. Every value is the
//! existing typed input of the domain helper that consumes it; nothing is inferred.

use std::collections::BTreeSet;

use plumb_core::{Hash, Id};
use plumb_functional::{
    AuthorizationDraft, CalculationScope, DuplicatePolicy, ExprPredicateError, ExprRefValue,
    OperationScope, PredicateProvider, ProcessScope, ScenarioDerivationInputs,
    VerificationApplicabilityInputs, VocabularyPolicy,
};
use plumb_store::SqliteStageStore;
use plumb_validation::{
    F1ValidationInputs, F2ValidationInputs, F4ApplicabilityInput, ValidationPolicy,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Why an A0.4 runtime value is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuntimeConfigError {
    #[error("direct-manager fact {0} names the same ref twice")]
    SelfManaged(Id),
    #[error("direct-manager fact ({manager}, {employee}) is listed twice")]
    DuplicateFact { manager: Id, employee: Id },
    #[error("employee {0} has more than one direct manager")]
    EmployeeListedTwice(Id),
    #[error("calendar {0} is defined twice")]
    DuplicateCalendar(Id),
    #[error("calendar {calendar}: {reason}")]
    InvalidCalendar { calendar: Id, reason: String },
}

/// One explicit pilot direct-manager fact (`manager_ref` directly manages `employee_ref`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectManagerFact {
    pub manager_ref: Id,
    pub employee_ref: Id,
}

/// The deterministic pilot predicate provider: `direct_manager(actor, employee)` is true
/// exactly for a configured `(actor.value_ref, employee.value_ref)` pair and false otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirectManagerPredicates {
    pairs: BTreeSet<(Id, Id)>,
}

impl DirectManagerPredicates {
    /// Builds the provider, rejecting a pair with equal refs, an exact duplicate pair and an
    /// employee listed twice.
    pub fn new(facts: &[DirectManagerFact]) -> Result<DirectManagerPredicates, RuntimeConfigError> {
        let mut pairs = BTreeSet::new();
        let mut employees = BTreeSet::new();
        for fact in facts {
            if fact.manager_ref == fact.employee_ref {
                return Err(RuntimeConfigError::SelfManaged(fact.manager_ref.clone()));
            }
            if !pairs.insert((fact.manager_ref.clone(), fact.employee_ref.clone())) {
                return Err(RuntimeConfigError::DuplicateFact {
                    manager: fact.manager_ref.clone(),
                    employee: fact.employee_ref.clone(),
                });
            }
            if !employees.insert(fact.employee_ref.clone()) {
                return Err(RuntimeConfigError::EmployeeListedTwice(
                    fact.employee_ref.clone(),
                ));
            }
        }
        Ok(DirectManagerPredicates { pairs })
    }
}

impl PredicateProvider for DirectManagerPredicates {
    fn direct_manager(
        &self,
        actor: &ExprRefValue,
        employee: &ExprRefValue,
    ) -> Result<bool, ExprPredicateError> {
        Ok(self
            .pairs
            .contains(&(actor.value_ref.clone(), employee.value_ref.clone())))
    }
}

/// One calendar as configured: `calendar_ref`, lowercase English weekday names and
/// `YYYY-MM-DD` holidays. Its typed `CalendarDefinition` is built by the configuration loader;
/// this wire form is what the stage configuration hash binds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarConfig {
    pub calendar_ref: Id,
    pub weekend: Vec<String>,
    pub holidays: Vec<String>,
}

/// The lowercase English weekday names a calendar may use.
pub const WEEKDAY_NAMES: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

impl CalendarConfig {
    /// Checks the wire form: known weekday names, `YYYY-MM-DD` shaped holidays, no duplicates
    /// within the calendar. Date validity is checked when the typed calendar is built.
    pub fn validate(&self) -> Result<(), RuntimeConfigError> {
        let invalid = |reason: String| RuntimeConfigError::InvalidCalendar {
            calendar: self.calendar_ref.clone(),
            reason,
        };
        let mut days = BTreeSet::new();
        for day in &self.weekend {
            if !WEEKDAY_NAMES.contains(&day.as_str()) {
                return Err(invalid(format!("{day:?} is not a weekday name")));
            }
            if !days.insert(day) {
                return Err(invalid(format!("weekday {day} is listed twice")));
            }
        }
        let mut holidays = BTreeSet::new();
        for date in &self.holidays {
            let b = date.as_bytes();
            let shaped = b.len() == 10
                && b[4] == b'-'
                && b[7] == b'-'
                && b.iter()
                    .enumerate()
                    .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
            if !shaped {
                return Err(invalid(format!("{date:?} is not YYYY-MM-DD")));
            }
            if !holidays.insert(date) {
                return Err(invalid(format!("holiday {date} is listed twice")));
            }
        }
        Ok(())
    }
}

/// The configured F4 material: explicit applicability and visible assumptions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct F4GateConfig {
    pub applicability: F4ApplicabilityInput,
    pub visible_assumption_refs: Vec<Id>,
}

/// The configured gate supplemental inputs.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateInputsConfig {
    pub f1: Option<F1ValidationInputs>,
    pub f2: Option<F2ValidationInputs>,
    pub f4: Option<F4GateConfig>,
}

/// How the inference provider of the A0.3 runtime was constructed (bound into stage
/// configuration hashes; secrets never are).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderMode {
    Null,
    /// Replay of a validated execution list whose canonical content hashes to `fixtures_hash`.
    Replay {
        fixtures_hash: Hash,
    },
    Anthropic,
}

impl ProviderMode {
    /// The wire name of the mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderMode::Null => "null",
            ProviderMode::Replay { .. } => "replay",
            ProviderMode::Anthropic => "anthropic",
        }
    }

    /// The replay content hash, if any.
    pub fn fixtures_hash(&self) -> Option<&Hash> {
        match self {
            ProviderMode::Replay { fixtures_hash } => Some(fixtures_hash),
            _ => None,
        }
    }
}

/// The A0.4 runtime injected into the state (Hotfix 067).
pub struct A04RuntimeConfig {
    /// The durable stage store over the same database as the revision store.
    pub stage_store: SqliteStageStore,
    /// The Agent that authors deterministic stage audits.
    pub stage_actor_ref: Id,
    pub provider_mode: ProviderMode,
    pub validation_policy: ValidationPolicy,
    pub vocabulary_policy: VocabularyPolicy,
    pub duplicate_policy: DuplicatePolicy,
    /// The configured calendars (already built into the A0.3 calendar provider).
    pub calendars: Vec<CalendarConfig>,
    /// The configured direct-manager facts (already built into the A0.3 predicate provider).
    pub direct_managers: Vec<DirectManagerFact>,
    pub calculation_scope: Option<CalculationScope>,
    pub operation_scope: Option<OperationScope>,
    pub process_scope: Option<ProcessScope>,
    pub authorization_draft: Option<AuthorizationDraft>,
    pub scenario_derivation: Option<ScenarioDerivationInputs>,
    pub verification_applicability: Option<VerificationApplicabilityInputs>,
    pub gates: GateInputsConfig,
}

impl A04RuntimeConfig {
    /// Validates the configured values that are not validated by their consumers.
    pub fn validate(&self) -> Result<(), RuntimeConfigError> {
        let mut ids = BTreeSet::new();
        for calendar in &self.calendars {
            calendar.validate()?;
            if !ids.insert(&calendar.calendar_ref) {
                return Err(RuntimeConfigError::DuplicateCalendar(
                    calendar.calendar_ref.clone(),
                ));
            }
        }
        DirectManagerPredicates::new(&self.direct_managers)?;
        Ok(())
    }
}
