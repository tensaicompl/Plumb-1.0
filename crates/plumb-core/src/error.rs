//! Errors for the core primitives.

use thiserror::Error;

/// Error returned when a core primitive cannot be constructed or produced.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreError {
    /// The string does not satisfy the `Id` grammar (plan §6.3).
    #[error("invalid id {0:?}")]
    InvalidId(String),
    /// The string is not one of the three accepted `Hash` forms (plan §6.3).
    #[error("invalid hash {0:?}")]
    InvalidHash(String),
    /// The string is not a valid RFC 3339 timestamp representable in canonical form.
    #[error("invalid timestamp {input:?}: {reason}")]
    InvalidTimestamp { input: String, reason: String },
    /// RFC 8785 canonical JSON serialization failed.
    #[error("canonical JSON serialization failed: {0}")]
    Canonicalization(String),
}
