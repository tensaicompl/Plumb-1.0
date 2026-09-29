//! A JSON value whose persisted and hashed form is its RFC 8785 canonical serialization.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::canonical::to_canonical_json;
use crate::error::CoreError;
use crate::hash::Hash;

/// A transparent JSON value wrapper: it serializes as the underlying value, and its bytes and
/// content hash are always taken over RFC 8785 canonical JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CanonicalJson(Value);

impl CanonicalJson {
    pub fn new(value: Value) -> CanonicalJson {
        CanonicalJson(value)
    }

    pub fn as_value(&self) -> &Value {
        &self.0
    }

    pub fn into_value(self) -> Value {
        self.0
    }

    /// RFC 8785 canonical JSON bytes of the value.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CoreError> {
        to_canonical_json(&self.0)
    }

    /// Generic `sha256:` hash of the canonical bytes.
    pub fn content_hash(&self) -> Result<Hash, CoreError> {
        Ok(Hash::content_sha256(&self.canonical_bytes()?))
    }
}
