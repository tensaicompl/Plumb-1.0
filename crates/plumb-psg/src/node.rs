//! The PSG node envelope (metamodel §§4.1, 4.5, 24.1).

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::audit::{AuditMeta, AuditMetaError};
use crate::extensions::ExtensionKey;
use crate::payload::{NodePayload, NodePayloadError, NodeType};
use crate::refs::{DerivationRef, EvidenceRef};
use crate::standards::StandardMapping;
use crate::status::ElementStatus;

/// Why a [`Node`] is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NodeError {
    /// Element revisions start at 1; 0 is invalid.
    #[error("node revision must be at least 1")]
    ZeroRevision,
    /// The payload fails its programmatic validation.
    #[error("invalid payload: {0}")]
    InvalidPayload(#[from] NodePayloadError),
    /// The audit metadata violates its invariants.
    #[error("invalid audit metadata: {0}")]
    InvalidAudit(#[from] AuditMetaError),
    /// The same evidence reference appears more than once.
    #[error("duplicate evidence reference {0:?}")]
    DuplicateEvidenceRef(EvidenceRef),
    /// The same derivation reference appears more than once.
    #[error("duplicate derivation reference {0:?}")]
    DuplicateDerivationRef(DerivationRef),
    /// The same standard mapping appears more than once (at `index`).
    #[error("duplicate standard mapping at index {index}")]
    DuplicateStandardMapping { index: usize },
    /// A payload that carries its own `id` disagrees with the node ID.
    #[error("{node_type:?} payload id {payload_id} does not match node id {node_id}")]
    PayloadIdMismatch {
        node_type: NodeType,
        node_id: Id,
        payload_id: Id,
    },
}

/// A PSG node: an immutable-ID envelope around a typed semantic payload.
///
/// There is no universal confidence field; confidence belongs to derivations (§4.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "NodeFields")]
pub struct Node {
    pub id: Id,
    pub revision: u32,
    pub status: ElementStatus,
    pub payload: NodePayload,
    pub evidence: Vec<EvidenceRef>,
    pub derivations: Vec<DerivationRef>,
    pub standards: Vec<StandardMapping>,
    pub tags: BTreeSet<String>,
    pub extensions: BTreeMap<ExtensionKey, Value>,
    pub audit: AuditMeta,
}

impl Node {
    /// Checks the envelope invariants: `revision != 0`, valid audit metadata, and
    /// `id` equality with `DerivationRecord` / `StandardsProfile` payload IDs.
    pub fn validate(&self) -> Result<(), NodeError> {
        if self.revision == 0 {
            return Err(NodeError::ZeroRevision);
        }
        self.payload.validate()?;
        self.audit.validate()?;
        check_unique_envelope_collections(&self.evidence, &self.derivations, &self.standards)
            .map_err(|duplicate| match duplicate {
                EnvelopeDuplicate::Evidence(r) => NodeError::DuplicateEvidenceRef(r),
                EnvelopeDuplicate::Derivation(r) => NodeError::DuplicateDerivationRef(r),
                EnvelopeDuplicate::Standard(index) => NodeError::DuplicateStandardMapping { index },
            })?;
        let payload_id = match &self.payload {
            NodePayload::DerivationRecord(record) => Some(&record.id),
            NodePayload::StandardsProfile(profile) => Some(&profile.id),
            _ => None,
        };
        match payload_id {
            Some(payload_id) if payload_id != &self.id => Err(NodeError::PayloadIdMismatch {
                node_type: self.payload.node_type(),
                node_id: self.id.clone(),
                payload_id: payload_id.clone(),
            }),
            _ => Ok(()),
        }
    }
}

/// The first exact duplicate found in an envelope collection.
pub(crate) enum EnvelopeDuplicate {
    Evidence(EvidenceRef),
    Derivation(DerivationRef),
    Standard(usize),
}

/// Rejects exact duplicates in the envelope `evidence`, `derivations` and `standards` vectors.
pub(crate) fn check_unique_envelope_collections(
    evidence: &[EvidenceRef],
    derivations: &[DerivationRef],
    standards: &[StandardMapping],
) -> Result<(), EnvelopeDuplicate> {
    if let Some(i) = first_duplicate(evidence) {
        return Err(EnvelopeDuplicate::Evidence(evidence[i].clone()));
    }
    if let Some(i) = first_duplicate(derivations) {
        return Err(EnvelopeDuplicate::Derivation(derivations[i].clone()));
    }
    if let Some(index) = first_duplicate(standards) {
        return Err(EnvelopeDuplicate::Standard(index));
    }
    Ok(())
}

/// Index of the first element equal to an earlier element.
fn first_duplicate<T: PartialEq>(items: &[T]) -> Option<usize> {
    (1..items.len()).find(|&i| items[..i].contains(&items[i]))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeFields {
    id: Id,
    revision: u32,
    status: ElementStatus,
    payload: NodePayload,
    evidence: Vec<EvidenceRef>,
    derivations: Vec<DerivationRef>,
    standards: Vec<StandardMapping>,
    tags: BTreeSet<String>,
    extensions: BTreeMap<ExtensionKey, Value>,
    audit: AuditMeta,
}

impl TryFrom<NodeFields> for Node {
    type Error = NodeError;

    fn try_from(f: NodeFields) -> Result<Self, Self::Error> {
        let node = Node {
            id: f.id,
            revision: f.revision,
            status: f.status,
            payload: f.payload,
            evidence: f.evidence,
            derivations: f.derivations,
            standards: f.standards,
            tags: f.tags,
            extensions: f.extensions,
            audit: f.audit,
        };
        node.validate()?;
        Ok(node)
    }
}
