//! RFC 8785 (JCS) canonical JSON through `serde_json_canonicalizer`, and SHA-256 over
//! canonical bytes.
//!
//! This primitive canonicalizes exactly the value supplied to it. Plumb domain-specific
//! ordering of semantically unordered arrays is applied by semantic-envelope construction
//! before calling it (plan §6.3); it is not done here.

use serde::Serialize;

use crate::error::CoreError;
use crate::hash::{Hash, HashKind};

/// Serializes `value` to RFC 8785 canonical JSON bytes using `serde_json_canonicalizer`.
pub fn to_canonical_json<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, CoreError> {
    serde_json_canonicalizer::to_vec(&value).map_err(|e| CoreError::Canonicalization(e.to_string()))
}

/// SHA-256 over the canonical JSON bytes of `value`, as a hash of the given kind.
pub fn canonical_hash<T: Serialize + ?Sized>(kind: HashKind, value: &T) -> Result<Hash, CoreError> {
    let bytes = to_canonical_json(value)?;
    Ok(match kind {
        HashKind::Generic => Hash::content_sha256(&bytes),
        HashKind::Semantic => Hash::semantic_sha256(&bytes),
        HashKind::Evidence => Hash::evidence_sha256(&bytes),
    })
}
