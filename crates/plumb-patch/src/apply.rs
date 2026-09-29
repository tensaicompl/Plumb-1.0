//! Deterministic, atomic application of a `PatchSet` to an immutable `Graph`
//! (metamodel §§4.5, 20).

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{to_canonical_json, CoreError, Hash, HashKind, Id};
use plumb_psg::{
    edge_element_hash, node_element_hash, Edge, EdgeError, ElementStatus, Graph, GraphViolation,
    Node, NodeError, NodePayload, NodeType, RelationKind,
};
use serde_json::Value;
use thiserror::Error;

use crate::diff::{sort_diff, DiffChange, DiffElementKind, DiffEntry, GraphDelta};
use crate::model::{ElementPrecondition, PatchSet, SemanticPatch};

/// Why a patch set could not be applied. No candidate graph or delta exists on failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PatchError {
    #[error("base_semantic_hash must be a semantic hash, got {0:?}")]
    InvalidBaseHashKind(HashKind),
    #[error("stale base: patch expects {expected}, graph is {actual}")]
    StaleBase { expected: Hash, actual: Hash },
    #[error("expected_hash of {id} must be a generic element hash, got {kind:?}")]
    InvalidExpectedHashKind { id: Id, kind: HashKind },
    #[error("element {0} not found")]
    ElementNotFound(Id),
    #[error("element {0} is not a node")]
    ExpectedNode(Id),
    #[error("element {0} is not an edge")]
    ExpectedEdge(Id),
    #[error("element {id} has hash {actual}, patch expects {expected}")]
    ElementHashMismatch {
        id: Id,
        expected: Hash,
        actual: Hash,
    },
    #[error("id {0} already exists")]
    IdAlreadyExists(Id),
    #[error("id {0} was already used in this patch set and cannot be reused")]
    IdReused(Id),
    #[error("new element {id} must have revision 1, got {revision}")]
    NewElementRevisionNotOne { id: Id, revision: u32 },
    #[error("element {id} has status {actual:?}, patch expects {expected:?}")]
    StatusMismatch {
        id: Id,
        expected: ElementStatus,
        actual: ElementStatus,
    },
    #[error("SetStatus on {0} does not change the status")]
    StatusNoOp(Id),
    #[error("ReplacePayload on {id} would change the node type from {from:?} to {to:?}")]
    NodeTypeChange {
        id: Id,
        from: NodeType,
        to: NodeType,
    },
    #[error("ReplacePayload on {id} would change identity field {field}")]
    DerivedIdentityChange { id: Id, field: &'static str },
    #[error("node {node} has incident edges {edges:?}")]
    IncidentEdgesPreventRemoval { node: Id, edges: Vec<Id> },
    #[error("{removed} is still referenced by {referenced_by}")]
    ReferencedElement { removed: Id, referenced_by: Id },
    #[error("Compound must contain at least one patch")]
    EmptyCompound,
    #[error("MergeNodes must merge at least one node")]
    EmptyMerge,
    #[error("MergeNodes lists {0} more than once")]
    DuplicateMergeTarget(Id),
    #[error("MergeNodes lists the keep node {0} among the merged nodes")]
    MergeContainsKeep(Id),
    #[error("MergeNodes node {id} is a {found:?}, keep is a {expected:?}")]
    MergeTypeMismatch {
        id: Id,
        expected: NodeType,
        found: NodeType,
    },
    #[error("MergeNodes does not support {0:?} nodes")]
    MergeTypeUnsupported(NodeType),
    #[error("MergeNodes found conflicting values for extension {key}")]
    MergeExtensionConflict { key: String },
    #[error("invalid Supersede: {0}")]
    InvalidSupersede(&'static str),
    #[error("evidence {evidence} is already attached to {target}")]
    DuplicateEvidenceAttachment { target: Id, evidence: Id },
    #[error("the standard mapping is already attached to {0}")]
    DuplicateStandardMappingAttachment(Id),
    #[error("revision of {0} would overflow")]
    RevisionOverflow(Id),
    #[error("node {id}: {error}")]
    InvalidLocalNode { id: Id, error: NodeError },
    #[error("edge {id}: {error}")]
    InvalidLocalEdge { id: Id, error: EdgeError },
    #[error("final graph is invalid: {0:?}")]
    FinalGraphInvalid(Vec<GraphViolation>),
    #[error("canonicalization failed: {0}")]
    Canonicalization(#[from] CoreError),
}

/// A successfully applied patch set: the new validated graph and its delta.
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyResult {
    pub graph: Graph,
    pub delta: GraphDelta,
}

/// Applies `patch_set` to `base` atomically. `base` is never modified; on any failure no
/// candidate graph or delta is returned.
pub fn apply_patch(base: &Graph, patch_set: &PatchSet) -> Result<ApplyResult, PatchError> {
    let kind = patch_set.base_semantic_hash.kind();
    if kind != HashKind::Semantic {
        return Err(PatchError::InvalidBaseHashKind(kind));
    }
    let base_semantic_hash = base.semantic_hash()?;
    if patch_set.base_semantic_hash != base_semantic_hash {
        return Err(PatchError::StaleBase {
            expected: patch_set.base_semantic_hash.clone(),
            actual: base_semantic_hash,
        });
    }

    let mut work = Working::new(base);
    let mut leaves = Vec::new();
    flatten(&patch_set.patch, &mut leaves)?;
    for (ordinal, leaf) in leaves.into_iter().enumerate() {
        let ordinal = u32::try_from(ordinal).unwrap_or(u32::MAX);
        work.apply_leaf(ordinal, leaf)?;
    }
    work.finalize_revisions(base)?;

    let Working {
        nodes,
        edges,
        touched_nodes,
        touched_edges,
        mut diff,
        ..
    } = work;
    let graph = Graph::new(
        base.project_id().clone(),
        base.profile_id().clone(),
        nodes.into_values().collect(),
        edges.into_values().collect(),
    )
    .map_err(PatchError::FinalGraphInvalid)?;

    sort_diff(&mut diff);
    let (added_nodes, removed_nodes, modified_nodes) = net_changes(base.nodes(), graph.nodes());
    let (added_edges, removed_edges, modified_edges) = net_changes(base.edges(), graph.edges());
    let delta = GraphDelta {
        base_semantic_hash,
        result_semantic_hash: graph.semantic_hash()?,
        touched_nodes,
        touched_edges,
        added_nodes,
        removed_nodes,
        modified_nodes,
        added_edges,
        removed_edges,
        modified_edges,
        diff,
    };
    Ok(ApplyResult { graph, delta })
}

/// Flattens nested Compounds depth-first in declared order, rejecting empty Compounds.
fn flatten<'a>(
    patch: &'a SemanticPatch,
    out: &mut Vec<&'a SemanticPatch>,
) -> Result<(), PatchError> {
    match patch {
        SemanticPatch::Compound { patches } => {
            if patches.is_empty() {
                return Err(PatchError::EmptyCompound);
            }
            for child in patches {
                flatten(child, out)?;
            }
        }
        leaf => out.push(leaf),
    }
    Ok(())
}

fn net_changes<T: PartialEq>(
    base: &BTreeMap<Id, T>,
    result: &BTreeMap<Id, T>,
) -> (BTreeSet<Id>, BTreeSet<Id>, BTreeSet<Id>) {
    let added = result
        .keys()
        .filter(|id| !base.contains_key(*id))
        .cloned()
        .collect();
    let removed = base
        .keys()
        .filter(|id| !result.contains_key(*id))
        .cloned()
        .collect();
    let modified = base
        .iter()
        .filter(|(id, element)| result.get(*id).is_some_and(|r| r != *element))
        .map(|(id, _)| id.clone())
        .collect();
    (added, removed, modified)
}

/// The mutable candidate built from a cloned base; discarded on any failure.
struct Working {
    nodes: BTreeMap<Id, Node>,
    edges: BTreeMap<Id, Edge>,
    /// Every ID that existed in the base or was used during this patch set.
    used_ids: BTreeSet<Id>,
    touched_nodes: BTreeSet<Id>,
    touched_edges: BTreeSet<Id>,
    diff: Vec<DiffEntry>,
}

impl Working {
    fn new(base: &Graph) -> Self {
        let used_ids = base
            .nodes()
            .keys()
            .chain(base.edges().keys())
            .cloned()
            .collect();
        Working {
            nodes: base.nodes().clone(),
            edges: base.edges().clone(),
            used_ids,
            touched_nodes: BTreeSet::new(),
            touched_edges: BTreeSet::new(),
            diff: Vec::new(),
        }
    }

    // ------------------------------------------------------------------ preconditions

    fn check_kind(pre: &ElementPrecondition) -> Result<(), PatchError> {
        match pre.expected_hash.kind() {
            HashKind::Generic => Ok(()),
            kind => Err(PatchError::InvalidExpectedHashKind {
                id: pre.id.clone(),
                kind,
            }),
        }
    }

    fn check_hash(pre: &ElementPrecondition, actual: Hash) -> Result<(), PatchError> {
        if actual == pre.expected_hash {
            Ok(())
        } else {
            Err(PatchError::ElementHashMismatch {
                id: pre.id.clone(),
                expected: pre.expected_hash.clone(),
                actual,
            })
        }
    }

    fn node_at(&self, pre: &ElementPrecondition) -> Result<&Node, PatchError> {
        Self::check_kind(pre)?;
        let node = match self.nodes.get(&pre.id) {
            Some(node) => node,
            None if self.edges.contains_key(&pre.id) => {
                return Err(PatchError::ExpectedNode(pre.id.clone()))
            }
            None => return Err(PatchError::ElementNotFound(pre.id.clone())),
        };
        Self::check_hash(pre, node_element_hash(node)?)?;
        Ok(node)
    }

    fn edge_at(&self, pre: &ElementPrecondition) -> Result<&Edge, PatchError> {
        Self::check_kind(pre)?;
        let edge = match self.edges.get(&pre.id) {
            Some(edge) => edge,
            None if self.nodes.contains_key(&pre.id) => {
                return Err(PatchError::ExpectedEdge(pre.id.clone()))
            }
            None => return Err(PatchError::ElementNotFound(pre.id.clone())),
        };
        Self::check_hash(pre, edge_element_hash(edge)?)?;
        Ok(edge)
    }

    /// Resolves a precondition to a node or an edge.
    fn element_at(&self, pre: &ElementPrecondition) -> Result<DiffElementKind, PatchError> {
        Self::check_kind(pre)?;
        if let Some(node) = self.nodes.get(&pre.id) {
            Self::check_hash(pre, node_element_hash(node)?)?;
            Ok(DiffElementKind::Node)
        } else if let Some(edge) = self.edges.get(&pre.id) {
            Self::check_hash(pre, edge_element_hash(edge)?)?;
            Ok(DiffElementKind::Edge)
        } else {
            Err(PatchError::ElementNotFound(pre.id.clone()))
        }
    }

    fn check_new_id(&self, id: &Id) -> Result<(), PatchError> {
        if self.nodes.contains_key(id) || self.edges.contains_key(id) {
            Err(PatchError::IdAlreadyExists(id.clone()))
        } else if self.used_ids.contains(id) {
            Err(PatchError::IdReused(id.clone()))
        } else {
            Ok(())
        }
    }

    /// Conservative guard: no surviving element may contain `removed` as an exact JSON string.
    fn check_unreferenced(&self, removed: &Id, excluded: &BTreeSet<&Id>) -> Result<(), PatchError> {
        let refers = |id: &Id, value: Value| {
            !excluded.contains(id) && contains_string(&value, removed.as_str())
        };
        let node_ref = self
            .nodes
            .iter()
            .find(|(id, node)| refers(id, serde_json::to_value(node).unwrap_or(Value::Null)))
            .map(|(id, _)| id);
        let edge_ref = self
            .edges
            .iter()
            .find(|(id, edge)| refers(id, serde_json::to_value(edge).unwrap_or(Value::Null)))
            .map(|(id, _)| id);
        let referencing = match (node_ref, edge_ref) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        match referencing {
            Some(by) => Err(PatchError::ReferencedElement {
                removed: removed.clone(),
                referenced_by: by.clone(),
            }),
            None => Ok(()),
        }
    }

    fn incident_edges(&self, node: &Id) -> Vec<Id> {
        self.edges
            .values()
            .filter(|e| &e.from == node || &e.to == node)
            .map(|e| e.id.clone())
            .collect()
    }

    // ------------------------------------------------------------------ recording

    fn record_node(
        &mut self,
        ordinal: u32,
        id: &Id,
        change: DiffChange,
        before: Option<Hash>,
        after: Option<Hash>,
    ) {
        self.touched_nodes.insert(id.clone());
        self.diff.push(DiffEntry {
            element_kind: DiffElementKind::Node,
            id: id.clone(),
            operation_ordinal: ordinal,
            change,
            before_hash: before,
            after_hash: after,
        });
    }

    fn record_edge(
        &mut self,
        ordinal: u32,
        id: &Id,
        change: DiffChange,
        before: Option<Hash>,
        after: Option<Hash>,
    ) {
        self.touched_edges.insert(id.clone());
        self.diff.push(DiffEntry {
            element_kind: DiffElementKind::Edge,
            id: id.clone(),
            operation_ordinal: ordinal,
            change,
            before_hash: before,
            after_hash: after,
        });
    }

    /// Replaces a node after local validation and records the modification.
    fn put_node(&mut self, ordinal: u32, node: Node) -> Result<(), PatchError> {
        node.validate()
            .map_err(|error| PatchError::InvalidLocalNode {
                id: node.id.clone(),
                error,
            })?;
        let before = node_element_hash(&self.nodes[&node.id])?;
        let after = node_element_hash(&node)?;
        let id = node.id.clone();
        self.nodes.insert(id.clone(), node);
        self.record_node(
            ordinal,
            &id,
            DiffChange::Modified,
            Some(before),
            Some(after),
        );
        Ok(())
    }

    /// Replaces an edge after local validation and records the modification.
    fn put_edge(&mut self, ordinal: u32, edge: Edge) -> Result<(), PatchError> {
        edge.validate()
            .map_err(|error| PatchError::InvalidLocalEdge {
                id: edge.id.clone(),
                error,
            })?;
        let before = edge_element_hash(&self.edges[&edge.id])?;
        let after = edge_element_hash(&edge)?;
        let id = edge.id.clone();
        self.edges.insert(id.clone(), edge);
        self.record_edge(
            ordinal,
            &id,
            DiffChange::Modified,
            Some(before),
            Some(after),
        );
        Ok(())
    }

    // ------------------------------------------------------------------ leaf operations

    fn apply_leaf(&mut self, ordinal: u32, patch: &SemanticPatch) -> Result<(), PatchError> {
        match patch {
            SemanticPatch::AddNode { node } => {
                self.check_new_id(&node.id)?;
                if node.revision != 1 {
                    return Err(PatchError::NewElementRevisionNotOne {
                        id: node.id.clone(),
                        revision: node.revision,
                    });
                }
                node.validate()
                    .map_err(|error| PatchError::InvalidLocalNode {
                        id: node.id.clone(),
                        error,
                    })?;
                let after = node_element_hash(node)?;
                self.used_ids.insert(node.id.clone());
                self.nodes.insert(node.id.clone(), node.clone());
                self.record_node(ordinal, &node.id, DiffChange::Added, None, Some(after));
            }
            SemanticPatch::AddEdge { edge } => {
                self.check_new_id(&edge.id)?;
                if edge.revision != 1 {
                    return Err(PatchError::NewElementRevisionNotOne {
                        id: edge.id.clone(),
                        revision: edge.revision,
                    });
                }
                edge.validate()
                    .map_err(|error| PatchError::InvalidLocalEdge {
                        id: edge.id.clone(),
                        error,
                    })?;
                let after = edge_element_hash(edge)?;
                self.used_ids.insert(edge.id.clone());
                self.edges.insert(edge.id.clone(), edge.clone());
                self.record_edge(ordinal, &edge.id, DiffChange::Added, None, Some(after));
            }
            SemanticPatch::RemoveNode { target } => {
                let before = node_element_hash(self.node_at(target)?)?;
                let incident = self.incident_edges(&target.id);
                if !incident.is_empty() {
                    return Err(PatchError::IncidentEdgesPreventRemoval {
                        node: target.id.clone(),
                        edges: incident,
                    });
                }
                self.check_unreferenced(&target.id, &BTreeSet::from([&target.id]))?;
                self.nodes.remove(&target.id);
                self.record_node(ordinal, &target.id, DiffChange::Removed, Some(before), None);
            }
            SemanticPatch::RemoveEdge { target } => {
                let before = edge_element_hash(self.edge_at(target)?)?;
                self.check_unreferenced(&target.id, &BTreeSet::from([&target.id]))?;
                self.edges.remove(&target.id);
                self.record_edge(ordinal, &target.id, DiffChange::Removed, Some(before), None);
            }
            SemanticPatch::ReplacePayload { target, payload } => {
                let current = self.node_at(target)?;
                let (from, to) = (current.payload.node_type(), payload.node_type());
                if from != to {
                    return Err(PatchError::NodeTypeChange {
                        id: target.id.clone(),
                        from,
                        to,
                    });
                }
                check_derived_identity(&target.id, &current.payload, payload)?;
                let mut node = current.clone();
                node.payload = payload.clone();
                self.put_node(ordinal, node)?;
            }
            SemanticPatch::SetStatus { target, from, to } => {
                let element = self.element_at(target)?;
                let actual = match element {
                    DiffElementKind::Node => self.nodes[&target.id].status,
                    DiffElementKind::Edge => self.edges[&target.id].status,
                };
                if actual != *from {
                    return Err(PatchError::StatusMismatch {
                        id: target.id.clone(),
                        expected: *from,
                        actual,
                    });
                }
                if from == to {
                    return Err(PatchError::StatusNoOp(target.id.clone()));
                }
                match element {
                    DiffElementKind::Node => {
                        let mut node = self.nodes[&target.id].clone();
                        node.status = *to;
                        self.put_node(ordinal, node)?;
                    }
                    DiffElementKind::Edge => {
                        let mut edge = self.edges[&target.id].clone();
                        edge.status = *to;
                        self.put_edge(ordinal, edge)?;
                    }
                }
            }
            SemanticPatch::ReplaceEdge {
                target,
                kind,
                from,
                to,
                properties,
            } => {
                let mut edge = self.edge_at(target)?.clone();
                edge.kind = kind.clone();
                edge.from = from.clone();
                edge.to = to.clone();
                edge.properties = properties.clone();
                self.put_edge(ordinal, edge)?;
            }
            SemanticPatch::MergeNodes {
                keep,
                merge,
                field_policy: _,
            } => self.merge_nodes(ordinal, keep, merge)?,
            SemanticPatch::Supersede { old, new, edge } => {
                self.supersede(ordinal, old, new, edge)?
            }
            SemanticPatch::AttachEvidence { target, evidence } => match self.element_at(target)? {
                DiffElementKind::Node => {
                    let mut node = self.nodes[&target.id].clone();
                    if node.evidence.contains(evidence) {
                        return Err(PatchError::DuplicateEvidenceAttachment {
                            target: target.id.clone(),
                            evidence: evidence.as_id().clone(),
                        });
                    }
                    node.evidence.push(evidence.clone());
                    self.put_node(ordinal, node)?;
                }
                DiffElementKind::Edge => {
                    let mut edge = self.edges[&target.id].clone();
                    if edge.evidence.contains(evidence) {
                        return Err(PatchError::DuplicateEvidenceAttachment {
                            target: target.id.clone(),
                            evidence: evidence.as_id().clone(),
                        });
                    }
                    edge.evidence.push(evidence.clone());
                    self.put_edge(ordinal, edge)?;
                }
            },
            SemanticPatch::AttachStandardMapping { target, mapping } => {
                match self.element_at(target)? {
                    DiffElementKind::Node => {
                        let mut node = self.nodes[&target.id].clone();
                        if node.standards.contains(mapping) {
                            return Err(PatchError::DuplicateStandardMappingAttachment(
                                target.id.clone(),
                            ));
                        }
                        node.standards.push(mapping.clone());
                        self.put_node(ordinal, node)?;
                    }
                    DiffElementKind::Edge => {
                        let mut edge = self.edges[&target.id].clone();
                        if edge.standards.contains(mapping) {
                            return Err(PatchError::DuplicateStandardMappingAttachment(
                                target.id.clone(),
                            ));
                        }
                        edge.standards.push(mapping.clone());
                        self.put_edge(ordinal, edge)?;
                    }
                }
            }
            SemanticPatch::Compound { .. } => {
                // Compounds are flattened before leaf application.
                return Err(PatchError::EmptyCompound);
            }
        }
        Ok(())
    }

    fn merge_nodes(
        &mut self,
        ordinal: u32,
        keep: &ElementPrecondition,
        merge: &[ElementPrecondition],
    ) -> Result<(), PatchError> {
        if merge.is_empty() {
            return Err(PatchError::EmptyMerge);
        }
        if merge.iter().any(|m| m.id == keep.id) {
            return Err(PatchError::MergeContainsKeep(keep.id.clone()));
        }
        let mut seen = BTreeSet::new();
        for m in merge {
            if !seen.insert(&m.id) {
                return Err(PatchError::DuplicateMergeTarget(m.id.clone()));
            }
        }
        let keep_node = self.node_at(keep)?;
        let keep_type = keep_node.payload.node_type();
        let mut merged = Vec::with_capacity(merge.len());
        for m in merge {
            let node = self.node_at(m)?;
            let found = node.payload.node_type();
            if found != keep_type {
                return Err(PatchError::MergeTypeMismatch {
                    id: m.id.clone(),
                    expected: keep_type,
                    found,
                });
            }
            merged.push(node);
        }
        if !matches!(keep_type, NodeType::Requirement | NodeType::Term) {
            return Err(PatchError::MergeTypeUnsupported(keep_type));
        }
        for m in merge {
            let incident = self.incident_edges(&m.id);
            if !incident.is_empty() {
                return Err(PatchError::IncidentEdgesPreventRemoval {
                    node: m.id.clone(),
                    edges: incident,
                });
            }
        }
        let merged_ids: BTreeSet<&Id> = merge.iter().map(|m| &m.id).collect();
        for m in merge {
            self.check_unreferenced(&m.id, &merged_ids)?;
        }

        let mut result = keep_node.clone();
        for node in &merged {
            for r in &node.evidence {
                if !result.evidence.contains(r) {
                    result.evidence.push(r.clone());
                }
            }
            for r in &node.derivations {
                if !result.derivations.contains(r) {
                    result.derivations.push(r.clone());
                }
            }
            for s in &node.standards {
                if !result.standards.contains(s) {
                    result.standards.push(s.clone());
                }
            }
            result.tags.extend(node.tags.iter().cloned());
            for (key, value) in &node.extensions {
                match result.extensions.get(key) {
                    Some(existing) if existing != value => {
                        return Err(PatchError::MergeExtensionConflict {
                            key: key.to_string(),
                        })
                    }
                    Some(_) => {}
                    None => {
                        result.extensions.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        result.evidence.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        result
            .derivations
            .sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let mut keyed = Vec::with_capacity(result.standards.len());
        for mapping in result.standards.drain(..) {
            keyed.push((to_canonical_json(&mapping)?, mapping));
        }
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        result.standards = keyed.into_iter().map(|(_, m)| m).collect();

        let removed: Vec<(Id, Hash)> = merged
            .iter()
            .map(|n| Ok((n.id.clone(), node_element_hash(n)?)))
            .collect::<Result<_, CoreError>>()?;
        self.put_node(ordinal, result)?;
        for (id, before) in removed {
            self.nodes.remove(&id);
            self.record_node(ordinal, &id, DiffChange::Removed, Some(before), None);
        }
        Ok(())
    }

    fn supersede(
        &mut self,
        ordinal: u32,
        old: &ElementPrecondition,
        new: &ElementPrecondition,
        edge: &Edge,
    ) -> Result<(), PatchError> {
        self.node_at(old)?;
        self.node_at(new)?;
        if old.id == new.id {
            return Err(PatchError::InvalidSupersede("old and new must differ"));
        }
        self.check_new_id(&edge.id)?;
        if edge.revision != 1 {
            return Err(PatchError::NewElementRevisionNotOne {
                id: edge.id.clone(),
                revision: edge.revision,
            });
        }
        if edge.kind != RelationKind::Supersedes {
            return Err(PatchError::InvalidSupersede("edge kind must be supersedes"));
        }
        if edge.from != new.id || edge.to != old.id {
            return Err(PatchError::InvalidSupersede(
                "edge must point from the new node to the old node",
            ));
        }
        if edge.status != ElementStatus::Accepted {
            return Err(PatchError::InvalidSupersede("edge status must be Accepted"));
        }
        edge.validate()
            .map_err(|error| PatchError::InvalidLocalEdge {
                id: edge.id.clone(),
                error,
            })?;
        let mut old_node = self.nodes[&old.id].clone();
        old_node.status = ElementStatus::Superseded;
        self.put_node(ordinal, old_node)?;
        self.touched_nodes.insert(new.id.clone());
        let after = edge_element_hash(edge)?;
        self.used_ids.insert(edge.id.clone());
        self.edges.insert(edge.id.clone(), edge.clone());
        self.record_edge(ordinal, &edge.id, DiffChange::Added, None, Some(after));
        Ok(())
    }

    // ------------------------------------------------------------------ revisions

    /// Finalizes element revisions once per patch set by the element-hash rule.
    fn finalize_revisions(&mut self, base: &Graph) -> Result<(), PatchError> {
        for (id, node) in self.nodes.iter_mut() {
            node.revision = match base.node(id) {
                Some(original) if node_element_hash(original)? != node_element_hash(node)? => {
                    original
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| PatchError::RevisionOverflow(id.clone()))?
                }
                Some(original) => original.revision,
                None => 1,
            };
        }
        for (id, edge) in self.edges.iter_mut() {
            edge.revision = match base.edge(id) {
                Some(original) if edge_element_hash(original)? != edge_element_hash(edge)? => {
                    original
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| PatchError::RevisionOverflow(id.clone()))?
                }
                Some(original) => original.revision,
                None => 1,
            };
        }
        Ok(())
    }
}

/// `SourceArtifact.content_hash` and `EvidenceFragment.source_ref`/`locator` define derived
/// node IDs and must not change in place.
fn check_derived_identity(
    id: &Id,
    current: &NodePayload,
    replacement: &NodePayload,
) -> Result<(), PatchError> {
    let changed = match (current, replacement) {
        (NodePayload::SourceArtifact(a), NodePayload::SourceArtifact(b))
            if a.content_hash != b.content_hash =>
        {
            Some("content_hash")
        }
        (NodePayload::EvidenceFragment(a), NodePayload::EvidenceFragment(b))
            if a.source_ref != b.source_ref =>
        {
            Some("source_ref")
        }
        (NodePayload::EvidenceFragment(a), NodePayload::EvidenceFragment(b))
            if a.locator != b.locator =>
        {
            Some("locator")
        }
        _ => None,
    };
    match changed {
        Some(field) => Err(PatchError::DerivedIdentityChange {
            id: id.clone(),
            field,
        }),
        None => Ok(()),
    }
}

/// Whether any JSON string value anywhere in `value` equals `needle` exactly.
///
/// Object property names are not references, so only values are inspected.
fn contains_string(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(s) => s == needle,
        Value::Array(items) => items.iter().any(|v| contains_string(v, needle)),
        Value::Object(map) => map.values().any(|v| contains_string(v, needle)),
        _ => false,
    }
}
