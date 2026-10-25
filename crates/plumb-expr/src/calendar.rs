//! Injected business calendars (plan S2.5, Hotfix 038).
//!
//! A calendar answers one question: is this date a working day. The static provider holds
//! programmatic definitions (weekend weekdays, holiday dates and optionally the closed list of
//! covered years); there is no clock, file, time-zone or holiday-service lookup, and the API is
//! Date-only. A date outside a calendar's declared coverage is never answered (H0.10): its
//! holidays are unknown, so it is neither a working nor a non-working day.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use thiserror::Error;
use time::{Date, Weekday};

/// Why a calendar question cannot be answered.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CalendarError {
    #[error("unknown calendar {calendar_ref}")]
    UnknownCalendar { calendar_ref: Id },
    #[error("calendar {calendar_ref} is defined twice")]
    DuplicateCalendar { calendar_ref: Id },
    #[error("calendar provider failure: {reason}")]
    Provider { reason: String },
    /// The date's year is outside the calendar's declared coverage.
    #[error("calendar {calendar_ref} does not cover {date}")]
    CoverageUnavailable { calendar_ref: Id, date: Date },
}

/// The injected source of working-day facts.
pub trait CalendarProvider {
    /// Whether `date` is a working day of the calendar `calendar_ref`.
    fn is_working_day(&self, calendar_ref: &Id, date: Date) -> Result<bool, CalendarError>;
}

/// One programmatic calendar: weekend weekdays, holiday dates and, when declared, the closed
/// list of covered years.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarDefinition {
    id: Id,
    weekend: BTreeSet<u8>,
    holidays: BTreeSet<Date>,
    coverage: Option<BTreeSet<i32>>,
}

impl CalendarDefinition {
    /// A calendar with the given weekend weekdays and holidays.
    pub fn new(
        id: Id,
        weekend: impl IntoIterator<Item = Weekday>,
        holidays: impl IntoIterator<Item = Date>,
    ) -> CalendarDefinition {
        CalendarDefinition {
            id,
            weekend: weekend
                .into_iter()
                .map(Weekday::number_from_monday)
                .collect(),
            holidays: holidays.into_iter().collect(),
            coverage: None,
        }
    }

    /// The same calendar declaring exactly `years` as its coverage. Without a declaration every
    /// date is answered (the calendar is taken as complete, the pre-H0.10 behaviour).
    pub fn with_coverage(mut self, years: impl IntoIterator<Item = i32>) -> CalendarDefinition {
        self.coverage = Some(years.into_iter().collect());
        self
    }

    /// The declared covered years, if any.
    pub fn coverage(&self) -> Option<&BTreeSet<i32>> {
        self.coverage.as_ref()
    }

    /// The calendar ID.
    pub fn id(&self) -> &Id {
        &self.id
    }

    /// A working day is neither a weekend day nor a holiday; a date outside the declared
    /// coverage is `CoverageUnavailable`.
    pub fn is_working_day(&self, date: Date) -> Result<bool, CalendarError> {
        if self
            .coverage
            .as_ref()
            .is_some_and(|years| !years.contains(&date.year()))
        {
            return Err(CalendarError::CoverageUnavailable {
                calendar_ref: self.id.clone(),
                date,
            });
        }
        Ok(!self.weekend.contains(&date.weekday().number_from_monday())
            && !self.holidays.contains(&date))
    }
}

/// A deterministic in-memory calendar provider.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticCalendarProvider {
    calendars: BTreeMap<Id, CalendarDefinition>,
}

impl StaticCalendarProvider {
    /// A provider of the given calendars; an ID may be defined once.
    pub fn new(
        calendars: impl IntoIterator<Item = CalendarDefinition>,
    ) -> Result<StaticCalendarProvider, CalendarError> {
        let mut provider = StaticCalendarProvider::default();
        for calendar in calendars {
            if provider.calendars.contains_key(calendar.id()) {
                return Err(CalendarError::DuplicateCalendar {
                    calendar_ref: calendar.id().clone(),
                });
            }
            provider.calendars.insert(calendar.id().clone(), calendar);
        }
        Ok(provider)
    }
}

impl CalendarProvider for StaticCalendarProvider {
    fn is_working_day(&self, calendar_ref: &Id, date: Date) -> Result<bool, CalendarError> {
        self.calendars
            .get(calendar_ref)
            .ok_or_else(|| CalendarError::UnknownCalendar {
                calendar_ref: calendar_ref.clone(),
            })?
            .is_working_day(date)
    }
}
