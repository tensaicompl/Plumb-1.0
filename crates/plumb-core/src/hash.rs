//! Validated SHA-256 hash string newtype (plan §6.3).
//!
//! Exactly three forms are accepted, each followed by 64 lowercase hex digits:
//! `sha256:` (generic), `psg:sha256:` (PSG semantic) and `ev:sha256:` (evidence).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::error::CoreError;

const GENERIC_PREFIX: &str = "sha256:";
const SEMANTIC_PREFIX: &str = "psg:sha256:";
const EVIDENCE_PREFIX: &str = "ev:sha256:";
const DIGEST_HEX_LEN: usize = 64;

/// Which of the three accepted hash forms a [`Hash`] has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash)]
pub enum HashKind {
    /// `sha256:` — generic/content/artifact/config/rule/output hash.
    Generic,
    /// `psg:sha256:` — PSG semantic hash.
    Semantic,
    /// `ev:sha256:` — evidence hash.
    Evidence,
}

impl HashKind {
    fn prefix(self) -> &'static str {
        match self {
            HashKind::Generic => GENERIC_PREFIX,
            HashKind::Semantic => SEMANTIC_PREFIX,
            HashKind::Evidence => EVIDENCE_PREFIX,
        }
    }
}

/// A validated, prefixed, lowercase SHA-256 hash string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash)]
pub struct Hash(String);

impl Hash {
    /// SHA-256 of `bytes` as a generic `sha256:` hash.
    pub fn content_sha256(bytes: &[u8]) -> Hash {
        Self::digest(HashKind::Generic, bytes)
    }

    /// SHA-256 of `bytes` as a PSG semantic `psg:sha256:` hash.
    pub fn semantic_sha256(bytes: &[u8]) -> Hash {
        Self::digest(HashKind::Semantic, bytes)
    }

    /// SHA-256 of `bytes` as an evidence `ev:sha256:` hash.
    pub fn evidence_sha256(bytes: &[u8]) -> Hash {
        Self::digest(HashKind::Evidence, bytes)
    }

    fn digest(kind: HashKind, bytes: &[u8]) -> Hash {
        Hash(format!(
            "{}{}",
            kind.prefix(),
            hex::encode(Sha256::digest(bytes))
        ))
    }

    /// The form of this hash, determined by its prefix.
    pub fn kind(&self) -> HashKind {
        // Validation guarantees exactly one prefix matches; the longer prefixes are checked first.
        if self.0.starts_with(SEMANTIC_PREFIX) {
            HashKind::Semantic
        } else if self.0.starts_with(EVIDENCE_PREFIX) {
            HashKind::Evidence
        } else {
            HashKind::Generic
        }
    }

    /// Returns the exact validated hash string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_valid_hash(s: &str) -> bool {
    let digest = [SEMANTIC_PREFIX, EVIDENCE_PREFIX, GENERIC_PREFIX]
        .iter()
        .find_map(|prefix| s.strip_prefix(prefix));
    digest.is_some_and(|hex| {
        hex.len() == DIGEST_HEX_LEN && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

impl TryFrom<String> for Hash {
    type Error = CoreError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_valid_hash(&value) {
            Ok(Self(value))
        } else {
            Err(CoreError::InvalidHash(value))
        }
    }
}

impl FromStr for Hash {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s.to_owned())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Hash {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for Hash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}
