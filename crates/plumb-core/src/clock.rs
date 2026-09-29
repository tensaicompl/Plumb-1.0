//! Canonical UTC timestamps and injectable clocks (plan §6.3).
//!
//! `SystemClock` is the only code in this crate that reads system time.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::format_description::well_known::Rfc3339;
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

use crate::error::CoreError;

/// Canonical form: UTC, `Z` suffix, exactly nine fractional-second digits.
const CANONICAL_FORMAT: &[BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:9]Z");

/// A UTC-normalized instant with nanosecond precision.
///
/// Displays and serializes as `2026-09-29T12:34:56.123456789Z`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    /// The instant as a UTC `OffsetDateTime`.
    pub fn as_offset_datetime(&self) -> OffsetDateTime {
        self.0
    }
}

impl TryFrom<OffsetDateTime> for Timestamp {
    type Error = CoreError;

    /// Normalizes to UTC. Fails, rather than panicking, when the UTC-normalized year is
    /// outside `0000..=9999`, because the canonical form requires exactly four year digits.
    fn try_from(value: OffsetDateTime) -> Result<Self, Self::Error> {
        value
            .checked_to_offset(UtcOffset::UTC)
            .filter(|utc| (0..=9999).contains(&utc.year()))
            .map(Self)
            .ok_or_else(|| CoreError::InvalidTimestamp {
                input: value.to_string(),
                reason: "UTC-normalized year is outside the canonical range 0000-9999".to_owned(),
            })
    }
}

impl FromStr for Timestamp {
    type Err = CoreError;

    /// Parses a valid RFC 3339 timestamp (`T` or single-space date/time separator, any
    /// numeric offset) and converts it to the equivalent UTC instant. Surrounding whitespace
    /// is rejected.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) {
            return Err(CoreError::InvalidTimestamp {
                input: s.to_owned(),
                reason: "surrounding whitespace is not allowed".to_owned(),
            });
        }
        let parsed =
            OffsetDateTime::parse(s, &Rfc3339).map_err(|e| CoreError::InvalidTimestamp {
                input: s.to_owned(),
                reason: e.to_string(),
            })?;
        Self::try_from(parsed).map_err(|e| match e {
            CoreError::InvalidTimestamp { reason, .. } => CoreError::InvalidTimestamp {
                input: s.to_owned(),
                reason,
            },
            other => other,
        })
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = self.0.format(CANONICAL_FORMAT).map_err(|_| fmt::Error)?;
        f.write_str(&text)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// Source of the current time. Inject this instead of reading wall-clock time.
pub trait Clock {
    /// The current instant.
    fn now(&self) -> Timestamp;
}

/// Reads the operating-system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp(OffsetDateTime::now_utc())
    }
}

/// Always returns the timestamp it was constructed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock {
    at: Timestamp,
}

impl FixedClock {
    /// A clock that always returns `at`.
    pub fn new(at: Timestamp) -> Self {
        Self { at }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        s.parse()
            .unwrap_or_else(|e| panic!("{s:?} should parse: {e}"))
    }

    fn is_canonical_form(s: &str) -> bool {
        // YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ
        let b = s.as_bytes();
        b.len() == 30
            && b.iter().enumerate().all(|(i, &c)| match i {
                4 | 7 => c == b'-',
                10 => c == b'T',
                13 | 16 => c == b':',
                19 => c == b'.',
                29 => c == b'Z',
                _ => c.is_ascii_digit(),
            })
    }

    #[test]
    fn utc_input_serializes_canonically() {
        let t = ts("2026-09-29T12:34:56.123456789Z");
        assert_eq!(t.to_string(), "2026-09-29T12:34:56.123456789Z");
        assert_eq!(t.as_offset_datetime().offset(), UtcOffset::UTC);
    }

    #[test]
    fn non_utc_offset_is_normalized_to_the_same_utc_instant() {
        let plus = ts("2026-09-29T14:34:56.123456789+02:00");
        assert_eq!(plus.to_string(), "2026-09-29T12:34:56.123456789Z");
        assert_eq!(plus, ts("2026-09-29T12:34:56.123456789Z"));
        let minus = ts("2026-09-29T07:04:56.123456789-05:30");
        assert_eq!(minus.to_string(), "2026-09-29T12:34:56.123456789Z");
    }

    #[test]
    fn exactly_nine_fractional_digits_are_always_written() {
        assert_eq!(
            ts("2026-09-29T12:34:56Z").to_string(),
            "2026-09-29T12:34:56.000000000Z"
        );
        assert_eq!(
            ts("2026-09-29T12:34:56.5Z").to_string(),
            "2026-09-29T12:34:56.500000000Z"
        );
        for s in [
            "2026-09-29T12:34:56Z",
            "2026-09-29T12:34:56.1+01:00",
            "2026-09-29T12:34:56.123456789Z",
        ] {
            assert!(is_canonical_form(&ts(s).to_string()), "{s:?}");
        }
        assert!(is_canonical_form(&SystemClock.now().to_string()));
    }

    #[test]
    fn nanosecond_precision_is_retained() {
        let t = ts("2026-09-29T12:34:56.000000001Z");
        assert_eq!(t.as_offset_datetime().nanosecond(), 1);
        assert_eq!(t.to_string(), "2026-09-29T12:34:56.000000001Z");
        assert_ne!(t, ts("2026-09-29T12:34:56.000000002Z"));
    }

    #[test]
    fn serde_round_trip_uses_the_canonical_string() {
        let t = ts("2026-09-29T14:34:56.123456789+02:00");
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"2026-09-29T12:34:56.123456789Z\"");
        let back: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        let from_offset: Timestamp =
            serde_json::from_str("\"2026-09-29T14:34:56.123456789+02:00\"").unwrap();
        assert_eq!(from_offset, t);
    }

    #[test]
    fn invalid_timestamps_are_rejected() {
        for s in [
            "",
            "not a timestamp",
            "2026-09-29T12:34:56",
            "2026-13-01T00:00:00Z",
            " 2026-09-29T12:34:56Z",
            "0000-01-01T00:30:00+01:00",
        ] {
            assert!(s.parse::<Timestamp>().is_err(), "{s:?} should be rejected");
            let json = serde_json::to_string(s).unwrap();
            assert!(
                serde_json::from_str::<Timestamp>(&json).is_err(),
                "{s:?} should not deserialize"
            );
        }
    }

    #[test]
    fn space_separator_is_accepted_and_serializes_with_t() {
        let t = ts("2026-09-29 12:34:56.123456789Z");
        assert_eq!(t.to_string(), "2026-09-29T12:34:56.123456789Z");
        assert_eq!(t, ts("2026-09-29T12:34:56.123456789Z"));
        assert_eq!(
            ts("2026-09-29 14:34:56+02:00").to_string(),
            "2026-09-29T12:34:56.000000000Z"
        );
    }

    #[test]
    fn surrounding_whitespace_is_rejected() {
        for s in [
            " 2026-09-29T12:34:56Z",
            "2026-09-29T12:34:56Z ",
            "\t2026-09-29T12:34:56Z",
            "2026-09-29T12:34:56Z\n",
        ] {
            assert!(s.parse::<Timestamp>().is_err(), "{s:?} should be rejected");
        }
    }

    #[test]
    fn boundary_years_are_accepted() {
        assert_eq!(
            ts("0000-01-01T00:00:00Z").to_string(),
            "0000-01-01T00:00:00.000000000Z"
        );
        assert_eq!(
            ts("9999-12-31T23:59:59.999999999Z").to_string(),
            "9999-12-31T23:59:59.999999999Z"
        );
    }

    #[test]
    fn years_outside_the_canonical_range_are_rejected_without_panicking() {
        // Below: UTC normalization moves year 0000 to year -0001.
        assert!("0000-01-01T00:30:00+01:00".parse::<Timestamp>().is_err());
        // Above: UTC normalization moves past 9999-12-31 (not representable by OffsetDateTime).
        assert!("9999-12-31T23:30:00-01:00".parse::<Timestamp>().is_err());
        // Directly constructed OffsetDateTime values outside 0000..=9999.
        let below = time::macros::datetime!(-0001-12-31 23:59:59 UTC);
        assert!(Timestamp::try_from(below).is_err());
        let above_after_normalization = time::macros::datetime!(9999-12-31 23:30:00 -01:00);
        assert!(Timestamp::try_from(above_after_normalization).is_err());
    }

    #[test]
    fn offset_normalization_inside_the_range_is_accepted() {
        assert_eq!(
            ts("0000-01-01T01:30:00+01:00").to_string(),
            "0000-01-01T00:30:00.000000000Z"
        );
        assert_eq!(
            ts("9999-12-31T22:30:00-01:00").to_string(),
            "9999-12-31T23:30:00.000000000Z"
        );
        let direct = time::macros::datetime!(0001-01-01 00:30:00 +01:00);
        assert_eq!(
            Timestamp::try_from(direct).unwrap().to_string(),
            "0000-12-31T23:30:00.000000000Z"
        );
    }

    #[test]
    fn fixed_clock_is_repeatable() {
        let at = ts("2026-09-29T12:34:56.123456789Z");
        let clock = FixedClock::new(at);
        assert_eq!(clock.now(), at);
        assert_eq!(clock.now(), clock.now());
        assert_eq!(clock.now().to_string(), "2026-09-29T12:34:56.123456789Z");
    }

    #[test]
    fn system_clock_returns_utc() {
        assert_eq!(
            SystemClock.now().as_offset_datetime().offset(),
            UtcOffset::UTC
        );
    }
}
