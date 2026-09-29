//! Audit metadata for PSG elements (metamodel §4.5).
//!
//! Audit metadata is operational: it is excluded from `semantic_hash`. Nothing here reads
//! wall-clock time; callers supply every timestamp.

use plumb_core::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Why an [`AuditMeta`] value is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuditMetaError {
    /// `updated_by` is present but `updated_at` is absent.
    #[error("updated_by is set but updated_at is missing")]
    UpdatedByWithoutUpdatedAt,
    /// `updated_at` is present but `updated_by` is absent.
    #[error("updated_at is set but updated_by is missing")]
    UpdatedAtWithoutUpdatedBy,
    /// `updated_at` is earlier than `created_at`.
    #[error("updated_at {updated_at} is earlier than created_at {created_at}")]
    UpdatedBeforeCreated {
        created_at: Timestamp,
        updated_at: Timestamp,
    },
}

/// Who created and last updated an element, and when.
///
/// Invariants (checked by [`AuditMeta::validate`] and enforced on deserialization, never
/// repaired): `updated_by` and `updated_at` are both present or both absent, and when present
/// `updated_at >= created_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "AuditMetaFields")]
pub struct AuditMeta {
    pub created_by: Id,
    pub created_at: Timestamp,
    pub updated_by: Option<Id>,
    pub updated_at: Option<Timestamp>,
}

impl AuditMeta {
    /// Builds audit metadata, rejecting any combination that violates the invariants.
    pub fn new(
        created_by: Id,
        created_at: Timestamp,
        updated_by: Option<Id>,
        updated_at: Option<Timestamp>,
    ) -> Result<Self, AuditMetaError> {
        let meta = Self {
            created_by,
            created_at,
            updated_by,
            updated_at,
        };
        meta.validate()?;
        Ok(meta)
    }

    /// Checks the invariants of an existing value, e.g. one built by struct literal.
    pub fn validate(&self) -> Result<(), AuditMetaError> {
        match (&self.updated_by, self.updated_at) {
            (None, None) => Ok(()),
            (Some(_), None) => Err(AuditMetaError::UpdatedByWithoutUpdatedAt),
            (None, Some(_)) => Err(AuditMetaError::UpdatedAtWithoutUpdatedBy),
            (Some(_), Some(updated_at)) if updated_at < self.created_at => {
                Err(AuditMetaError::UpdatedBeforeCreated {
                    created_at: self.created_at,
                    updated_at,
                })
            }
            (Some(_), Some(_)) => Ok(()),
        }
    }
}

/// Unvalidated wire form of [`AuditMeta`]; deserialization goes through `TryFrom`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditMetaFields {
    created_by: Id,
    created_at: Timestamp,
    updated_by: Option<Id>,
    updated_at: Option<Timestamp>,
}

impl TryFrom<AuditMetaFields> for AuditMeta {
    type Error = AuditMetaError;

    fn try_from(fields: AuditMetaFields) -> Result<Self, Self::Error> {
        Self::new(
            fields.created_by,
            fields.created_at,
            fields.updated_by,
            fields.updated_at,
        )
    }
}
