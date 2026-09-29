//! Typed references from PSG elements to evidence and derivations (metamodel §4.5).
//!
//! Both are transparent wrappers over a validated [`Id`]. They impose no prefix or namespace
//! restriction: whether the referenced ID resolves to evidence or to a `DerivationRecord` is
//! checked by graph-semantic validation, not here.

use plumb_core::Id;
use serde::{Deserialize, Serialize};

/// Reference to an evidence element. Serializes as the underlying ID string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EvidenceRef(Id);

impl EvidenceRef {
    /// The referenced ID.
    pub fn as_id(&self) -> &Id {
        &self.0
    }

    /// The exact referenced ID string.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Id> for EvidenceRef {
    fn from(id: Id) -> Self {
        Self(id)
    }
}

/// Reference to a derivation record. Serializes as the underlying ID string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DerivationRef(Id);

impl DerivationRef {
    /// The referenced ID.
    pub fn as_id(&self) -> &Id {
        &self.0
    }

    /// The exact referenced ID string.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Id> for DerivationRef {
    fn from(id: Id) -> Self {
        Self(id)
    }
}
