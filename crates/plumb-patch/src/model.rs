//! The serializable `SemanticPatch` AST (metamodel §§20.2-20.3).

use plumb_core::{CoreError, Hash, HashKind, Id};
use plumb_psg::{
    edge_element_hash, node_element_hash, Edge, ElementStatus, EvidenceRef, Node, NodePayload,
    RelationKind, RelationProperties, RelationPropertiesError, StandardMapping,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

/// A hash of the wrong kind in a patch structure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PatchModelError {
    /// `PatchSet.base_semantic_hash` must be a `psg:sha256:` hash.
    #[error("base_semantic_hash must be a semantic hash, got {0:?}")]
    InvalidBaseHashKind(HashKind),
    /// `ElementPrecondition.expected_hash` must be a generic `sha256:` hash.
    #[error("expected_hash must be a generic element hash, got {0:?}")]
    InvalidExpectedHashKind(HashKind),
    /// `ReplaceEdge.properties` does not fit `ReplaceEdge.kind`.
    #[error("invalid ReplaceEdge properties: {0}")]
    InvalidRelationProperties(#[from] RelationPropertiesError),
}

/// The serialized replay and compare-and-swap unit: a patch plus the semantic hash of the
/// graph it must be applied to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PatchSetFields")]
pub struct PatchSet {
    pub base_semantic_hash: Hash,
    pub patch: SemanticPatch,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchSetFields {
    base_semantic_hash: Hash,
    patch: SemanticPatch,
}

impl TryFrom<PatchSetFields> for PatchSet {
    type Error = PatchModelError;

    fn try_from(f: PatchSetFields) -> Result<Self, Self::Error> {
        match f.base_semantic_hash.kind() {
            HashKind::Semantic => Ok(PatchSet {
                base_semantic_hash: f.base_semantic_hash,
                patch: f.patch,
            }),
            other => Err(PatchModelError::InvalidBaseHashKind(other)),
        }
    }
}

/// The element a leaf operation targets and the generic element hash it must currently have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ElementPreconditionFields")]
pub struct ElementPrecondition {
    pub id: Id,
    pub expected_hash: Hash,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ElementPreconditionFields {
    id: Id,
    expected_hash: Hash,
}

impl TryFrom<ElementPreconditionFields> for ElementPrecondition {
    type Error = PatchModelError;

    fn try_from(f: ElementPreconditionFields) -> Result<Self, Self::Error> {
        match f.expected_hash.kind() {
            HashKind::Generic => Ok(ElementPrecondition {
                id: f.id,
                expected_hash: f.expected_hash,
            }),
            other => Err(PatchModelError::InvalidExpectedHashKind(other)),
        }
    }
}

/// How `MergeNodes` combines nodes. The pilot has exactly one policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergePolicy {
    /// Keep the keep node's id/status/payload/audit; union evidence, derivations, standards,
    /// tags and extensions.
    KeepPayloadUnionMetadata,
}

/// A serializable semantic change (metamodel §20.3). Exactly 12 variants; the `op` tag is the
/// variant name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields, try_from = "SemanticPatchFields")]
pub enum SemanticPatch {
    AddNode {
        node: Node,
    },
    RemoveNode {
        target: ElementPrecondition,
    },
    ReplacePayload {
        target: ElementPrecondition,
        payload: NodePayload,
    },
    SetStatus {
        target: ElementPrecondition,
        from: ElementStatus,
        to: ElementStatus,
    },
    AddEdge {
        edge: Edge,
    },
    ReplaceEdge {
        target: ElementPrecondition,
        kind: RelationKind,
        from: Id,
        to: Id,
        properties: RelationProperties,
    },
    RemoveEdge {
        target: ElementPrecondition,
    },
    MergeNodes {
        keep: ElementPrecondition,
        merge: Vec<ElementPrecondition>,
        field_policy: MergePolicy,
    },
    Supersede {
        old: ElementPrecondition,
        new: ElementPrecondition,
        edge: Edge,
    },
    AttachEvidence {
        target: ElementPrecondition,
        evidence: EvidenceRef,
    },
    AttachStandardMapping {
        target: ElementPrecondition,
        mapping: StandardMapping,
    },
    Compound {
        patches: Vec<SemanticPatch>,
    },
}

/// Wire form of [`SemanticPatch`]: identical except that `ReplaceEdge.properties` is read as a
/// JSON object and typed against `ReplaceEdge.kind` (edge properties are kind-dependent).
#[derive(Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
enum SemanticPatchFields {
    AddNode {
        node: Node,
    },
    RemoveNode {
        target: ElementPrecondition,
    },
    ReplacePayload {
        target: ElementPrecondition,
        payload: NodePayload,
    },
    SetStatus {
        target: ElementPrecondition,
        from: ElementStatus,
        to: ElementStatus,
    },
    AddEdge {
        edge: Edge,
    },
    ReplaceEdge {
        target: ElementPrecondition,
        kind: RelationKind,
        from: Id,
        to: Id,
        properties: Map<String, Value>,
    },
    RemoveEdge {
        target: ElementPrecondition,
    },
    MergeNodes {
        keep: ElementPrecondition,
        merge: Vec<ElementPrecondition>,
        field_policy: MergePolicy,
    },
    Supersede {
        old: ElementPrecondition,
        new: ElementPrecondition,
        edge: Edge,
    },
    AttachEvidence {
        target: ElementPrecondition,
        evidence: EvidenceRef,
    },
    AttachStandardMapping {
        target: ElementPrecondition,
        mapping: StandardMapping,
    },
    Compound {
        patches: Vec<SemanticPatch>,
    },
}

impl TryFrom<SemanticPatchFields> for SemanticPatch {
    type Error = PatchModelError;

    fn try_from(f: SemanticPatchFields) -> Result<Self, Self::Error> {
        use SemanticPatchFields as F;
        Ok(match f {
            F::AddNode { node } => SemanticPatch::AddNode { node },
            F::RemoveNode { target } => SemanticPatch::RemoveNode { target },
            F::ReplacePayload { target, payload } => {
                SemanticPatch::ReplacePayload { target, payload }
            }
            F::SetStatus { target, from, to } => SemanticPatch::SetStatus { target, from, to },
            F::AddEdge { edge } => SemanticPatch::AddEdge { edge },
            F::ReplaceEdge {
                target,
                kind,
                from,
                to,
                properties,
            } => {
                let properties = RelationProperties::from_json(&kind, properties)?;
                SemanticPatch::ReplaceEdge {
                    target,
                    kind,
                    from,
                    to,
                    properties,
                }
            }
            F::RemoveEdge { target } => SemanticPatch::RemoveEdge { target },
            F::MergeNodes {
                keep,
                merge,
                field_policy,
            } => SemanticPatch::MergeNodes {
                keep,
                merge,
                field_policy,
            },
            F::Supersede { old, new, edge } => SemanticPatch::Supersede { old, new, edge },
            F::AttachEvidence { target, evidence } => {
                SemanticPatch::AttachEvidence { target, evidence }
            }
            F::AttachStandardMapping { target, mapping } => {
                SemanticPatch::AttachStandardMapping { target, mapping }
            }
            F::Compound { patches } => SemanticPatch::Compound { patches },
        })
    }
}

impl SemanticPatch {
    /// The inverse derivable entirely from this patch's input, if any: `AddNode` ->
    /// `RemoveNode` and `AddEdge` -> `RemoveEdge` (using the supplied element's hash), and a
    /// `Compound` whose children are all invertible (children reversed). Every other variant
    /// returns `None`; previous values are never fabricated.
    pub fn inverse(&self) -> Result<Option<SemanticPatch>, CoreError> {
        Ok(match self {
            SemanticPatch::AddNode { node } => Some(SemanticPatch::RemoveNode {
                target: ElementPrecondition {
                    id: node.id.clone(),
                    expected_hash: node_element_hash(node)?,
                },
            }),
            SemanticPatch::AddEdge { edge } => Some(SemanticPatch::RemoveEdge {
                target: ElementPrecondition {
                    id: edge.id.clone(),
                    expected_hash: edge_element_hash(edge)?,
                },
            }),
            SemanticPatch::Compound { patches } => {
                let mut inverses = Vec::with_capacity(patches.len());
                for patch in patches.iter().rev() {
                    match patch.inverse()? {
                        Some(inverse) => inverses.push(inverse),
                        None => return Ok(None),
                    }
                }
                Some(SemanticPatch::Compound { patches: inverses })
            }
            SemanticPatch::RemoveNode { .. }
            | SemanticPatch::ReplacePayload { .. }
            | SemanticPatch::SetStatus { .. }
            | SemanticPatch::ReplaceEdge { .. }
            | SemanticPatch::RemoveEdge { .. }
            | SemanticPatch::MergeNodes { .. }
            | SemanticPatch::Supersede { .. }
            | SemanticPatch::AttachEvidence { .. }
            | SemanticPatch::AttachStandardMapping { .. } => None,
        })
    }
}
