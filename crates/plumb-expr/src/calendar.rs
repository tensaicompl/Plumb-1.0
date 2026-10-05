//! Injected business calendars (plan S2.5, Hotfix 038).
//!
//! A calendar answers one question: is this date a working day. The static provider holds
//! programmatic definitions (weekend weekdays and holiday dates); there is no clock, file,
//! time-zone or holiday-service lookup, and the API is Date-only.

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
}

/// The injected source of working-day facts.
pub trait CalendarProvider {
    /// Whether `date` is a working day of the calendar `calendar_ref`.
    fn is_working_day(&self, calendar_ref: &Id, date: Date) -> Result<bool, CalendarError>;
}

/// One programmatic calendar: weekend weekdays and holiday dates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarDefinition {
    id: Id,
    weekend: BTreeSet<u8>,
    holidays: BTreeSet<Date>,
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
        }
    }

    /// The calendar ID.
    pub fn id(&self) -> &Id {
        &self.id
    }

    /// A working day is neither a weekend day nor a holiday.
    pub fn is_working_day(&self, date: Date) -> bool {
        !self.weekend.contains(&date.weekday().number_from_monday())
            && !self.holidays.contains(&date)
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
            .map(|c| c.is_working_day(date))
            .ok_or_else(|| CalendarError::UnknownCalendar {
                calendar_ref: calendar_ref.clone(),
            })
    }
}
