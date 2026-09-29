//! The PSG node envelope (metamodel §§4.1, 4.5, 24.1).

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::audit::{AuditMeta, AuditMetaError};
use crate::extensions::ExtensionKey;
use crate::payload::{NodePayload, NodeType};
use crate::refs::{DerivationRef, EvidenceRef};
use crate::standards::StandardMapping;
use crate::status::ElementStatus;

/// Why a [`Node`] is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NodeError {
    /// Element revisions start at 1; 0 is invalid.
    #[error("node revision must be at least 1")]
    ZeroRevision,
    /// The audit metadata violates its invariants.
    #[error("invalid audit metadata: {0}")]
    InvalidAudit(#[from] AuditMetaError),
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
        self.audit.validate()?;
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
