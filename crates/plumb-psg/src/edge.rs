//! The PSG edge envelope (metamodel §§4.2, 4.5, 17.8).

use plumb_core::Id;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::audit::{AuditMeta, AuditMetaError};
use crate::refs::{DerivationRef, EvidenceRef};
use crate::relations::{RelationKind, RelationProperties, RelationPropertiesError};
use crate::standards::StandardMapping;
use crate::status::ElementStatus;

/// Why an [`Edge`] is invalid (local invariants only; endpoint types are registry validation).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EdgeError {
    /// Element revisions start at 1; 0 is invalid.
    #[error("edge revision must be at least 1")]
    ZeroRevision,
    /// The audit metadata violates its invariants.
    #[error("invalid audit metadata: {0}")]
    InvalidAudit(#[from] AuditMetaError),
    /// The properties do not fit the relation kind.
    #[error("invalid relation properties: {0}")]
    InvalidProperties(#[from] RelationPropertiesError),
}

/// A typed PSG relation between two nodes.
///
/// There is no universal confidence field and no free-form property map for core relations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "EdgeFields")]
pub struct Edge {
    pub id: Id,
    pub revision: u32,
    pub status: ElementStatus,
    pub kind: RelationKind,
    pub from: Id,
    pub to: Id,
    pub properties: RelationProperties,
    pub evidence: Vec<EvidenceRef>,
    pub derivations: Vec<DerivationRef>,
    pub standards: Vec<StandardMapping>,
    pub audit: AuditMeta,
}

impl Edge {
    /// Checks the local invariants: `revision != 0`, valid audit metadata, and
    /// `properties` compatible with `kind`.
    pub fn validate(&self) -> Result<(), EdgeError> {
        if self.revision == 0 {
            return Err(EdgeError::ZeroRevision);
        }
        self.audit.validate()?;
        self.properties.check_compatible(&self.kind)?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeFields {
    id: Id,
    revision: u32,
    status: ElementStatus,
    kind: RelationKind,
    from: Id,
    to: Id,
    properties: Map<String, Value>,
    evidence: Vec<EvidenceRef>,
    derivations: Vec<DerivationRef>,
    standards: Vec<StandardMapping>,
    audit: AuditMeta,
}

impl TryFrom<EdgeFields> for Edge {
    type Error = EdgeError;

    fn try_from(f: EdgeFields) -> Result<Self, Self::Error> {
        let properties = RelationProperties::from_json(&f.kind, f.properties)?;
        let edge = Edge {
            id: f.id,
            revision: f.revision,
            status: f.status,
            kind: f.kind,
            from: f.from,
            to: f.to,
            properties,
            evidence: f.evidence,
            derivations: f.derivations,
            standards: f.standards,
            audit: f.audit,
        };
        edge.validate()?;
        Ok(edge)
    }
}
