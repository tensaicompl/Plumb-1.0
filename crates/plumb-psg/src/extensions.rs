//! Namespace-qualified extension keys (metamodel §4.5).
//!
//! Grammar: `^[a-z][a-z0-9_-]*:[A-Za-z0-9._-]+$` — exactly one colon, no whitespace,
//! no trimming or normalization; the exact validated string is preserved.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// A string that does not satisfy the `ExtensionKey` grammar.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid extension key {0:?}")]
pub struct InvalidExtensionKey(pub String);

/// A validated `namespace:key` extension key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExtensionKey(String);

impl ExtensionKey {
    /// Returns the exact validated key string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_valid_extension_key(s: &str) -> bool {
    let Some((namespace, key)) = s.split_once(':') else {
        return false;
    };
    let mut namespace_chars = namespace.chars();
    let namespace_ok = namespace_chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
        && namespace_chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    let key_ok = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    namespace_ok && key_ok
}

impl TryFrom<String> for ExtensionKey {
    type Error = InvalidExtensionKey;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_valid_extension_key(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidExtensionKey(value))
        }
    }
}

impl FromStr for ExtensionKey {
    type Err = InvalidExtensionKey;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s.to_owned())
    }
}

impl fmt::Display for ExtensionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ExtensionKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for ExtensionKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ExtensionKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}
