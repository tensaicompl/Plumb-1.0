//! Validated identifier newtype (plan §6.3).
//!
//! Grammar: `^[a-z][a-z0-9_-]*:[A-Za-z0-9._-]+(?::[A-Za-z0-9._-]+)*$`.
//! Parsing performs no normalization and preserves the exact validated string.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::CoreError;

/// Longest accepted serialized Id in bytes: namespace, colons and body segments. The grammar is
/// ASCII-only, so this is also the longest Id in characters.
pub const ID_MAX_BYTES: usize = 255;

/// An immutable, validated Plumb identifier such as `req:HR-001`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(String);

impl Id {
    /// Returns the exact validated identifier string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_namespace_start(c: char) -> bool {
    c.is_ascii_lowercase()
}

fn is_namespace_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'
}

fn is_body_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'
}

fn is_valid_id(s: &str) -> bool {
    let mut segments = s.split(':');
    let namespace = segments.next().unwrap_or_default();
    let mut namespace_chars = namespace.chars();
    let namespace_ok = namespace_chars.next().is_some_and(is_namespace_start)
        && namespace_chars.all(is_namespace_char);
    if !namespace_ok {
        return false;
    }
    let mut body_segments = 0usize;
    for segment in segments {
        if segment.is_empty() || !segment.chars().all(is_body_char) {
            return false;
        }
        body_segments += 1;
    }
    body_segments >= 1
}

impl TryFrom<String> for Id {
    type Error = CoreError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() <= ID_MAX_BYTES && is_valid_id(&value) {
            Ok(Self(value))
        } else {
            Err(CoreError::InvalidId(value))
        }
    }
}

impl FromStr for Id {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s.to_owned())
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &[&str] = &[
        "src:0123456789abcdef",
        "req:HR-001",
        "req:hr:leave-request",
        "rev:42:0123456789abcdef",
        "project:leave-management",
        "profile:plumb-software-2026.1",
        "resolution:01K123456789ABCDEFGHJKMNPQ",
    ];

    const INVALID: &[&str] = &[
        "",
        "req",
        "Req:ABC",
        "req:",
        "req::ABC",
        "req:ABC:",
        "req:ABC DEF",
        " req:ABC",
        "req:ABC ",
        "req:ABC/DEF",
    ];

    #[test]
    fn plan_valid_examples_parse_and_preserve_exact_string() {
        for s in VALID {
            let id: Id = s
                .parse()
                .unwrap_or_else(|e| panic!("{s:?} should be valid: {e}"));
            assert_eq!(id.as_str(), *s);
            assert_eq!(id.to_string(), *s);
            assert_eq!(id.as_ref(), *s);
            assert_eq!(Id::try_from((*s).to_owned()).unwrap(), id);
        }
    }

    #[test]
    fn plan_invalid_examples_are_rejected() {
        for s in INVALID {
            assert_eq!(
                s.parse::<Id>(),
                Err(CoreError::InvalidId((*s).to_owned())),
                "{s:?} should be invalid"
            );
            assert!(Id::try_from((*s).to_owned()).is_err());
        }
    }

    #[test]
    fn other_whitespace_and_non_ascii_are_rejected() {
        for s in [
            "req:ABC\n",
            "req:\tABC",
            "req:ÄBC",
            "rëq:ABC",
            "1req:ABC",
            ":ABC",
        ] {
            assert!(s.parse::<Id>().is_err(), "{s:?} should be invalid");
        }
    }

    #[test]
    fn serde_round_trip_is_a_plain_json_string() {
        for s in VALID {
            let id: Id = s.parse().unwrap();
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, serde_json::to_string(s).unwrap());
            let back: Id = serde_json::from_str(&json).unwrap();
            assert_eq!(back, id);
        }
    }

    #[test]
    fn serde_deserialization_rejects_invalid_ids() {
        for s in INVALID {
            let json = serde_json::to_string(s).unwrap();
            assert!(
                serde_json::from_str::<Id>(&json).is_err(),
                "{s:?} should not deserialize"
            );
        }
        assert!(serde_json::from_str::<Id>("42").is_err());
    }

    /// An otherwise-valid Id of exactly `len` bytes.
    fn id_of_len(len: usize) -> String {
        let id = format!("req:{}", "A".repeat(len - "req:".len()));
        assert_eq!(id.len(), len);
        id
    }

    #[test]
    fn id_of_exactly_max_bytes_is_accepted_unchanged() {
        let s = id_of_len(ID_MAX_BYTES);
        let id: Id = s.parse().unwrap();
        assert_eq!(id.as_str().as_bytes(), s.as_bytes());
        assert_eq!(Id::try_from(s.clone()).unwrap(), id);
        let back: Id = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.as_str(), s);
    }

    #[test]
    fn id_longer_than_max_bytes_is_rejected_without_truncation() {
        for len in [ID_MAX_BYTES + 1, ID_MAX_BYTES + 2, 4096] {
            let s = id_of_len(len);
            assert_eq!(s.parse::<Id>(), Err(CoreError::InvalidId(s.clone())));
            assert_eq!(
                Id::try_from(s.clone()),
                Err(CoreError::InvalidId(s.clone()))
            );
            let json = serde_json::to_string(&s).unwrap();
            assert!(serde_json::from_str::<Id>(&json).is_err(), "{len} bytes");
        }
        let multi_segment = format!("rev:42:{}", "a".repeat(ID_MAX_BYTES + 1 - "rev:42:".len()));
        assert_eq!(multi_segment.len(), ID_MAX_BYTES + 1);
        assert!(multi_segment.parse::<Id>().is_err());
    }
}
