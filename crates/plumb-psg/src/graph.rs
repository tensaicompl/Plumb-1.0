//! The validated PSG graph and its deterministic indexes (metamodel §§17.9, 18; plan §6.2).

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{to_canonical_json, CoreError, Hash, HashKind, Id};
use thiserror::Error;

use crate::edge::{Edge, EdgeError};
use crate::hash;
use crate::node::{Node, NodeError};
use crate::payload::{NodePayload, NodeType};
use crate::registry::{validate_relation_constraints, validate_relation_shapes, RelationViolation};
use crate::relations::{RelationKind, RelationProperties};
use crate::status::ElementStatus;

/// Whether an element with this status participates in the baseline: structural constraints,
/// duplicate-semantic-edge checks and hash projections (metamodel §17.9).
pub fn is_baseline(status: ElementStatus) -> bool {
    matches!(
        status,
        ElementStatus::Accepted
            | ElementStatus::Suspect
            | ElementStatus::Superseded
            | ElementStatus::Deprecated
    )
}

/// A graph validity violation. Owners and targets are element IDs (IDs are globally unique).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GraphViolation {
    #[error("duplicate node id {0}")]
    DuplicateNodeId(Id),
    #[error("duplicate edge id {0}")]
    DuplicateEdgeId(Id),
    #[error("id {0} is used by both a node and an edge")]
    NodeEdgeIdCollision(Id),
    #[error("node {node}: {error}")]
    InvalidNode { node: Id, error: NodeError },
    #[error("edge {edge}: {error}")]
    InvalidEdge { edge: Id, error: EdgeError },
    /// A relation shape or baseline relation constraint violation.
    #[error("{0}")]
    Relation(RelationViolation),
    #[error("baseline edge {edge} references {status:?} node {node}")]
    BaselineEdgeEndpointNotBaseline {
        edge: Id,
        node: Id,
        status: ElementStatus,
    },
    #[error("{owner}: evidence reference {target} does not exist")]
    EvidenceRefMissing { owner: Id, target: Id },
    #[error("{owner}: evidence reference {target} is a {node_type:?}, not an EvidenceFragment")]
    EvidenceRefWrongType {
        owner: Id,
        target: Id,
        node_type: NodeType,
    },
    #[error("{owner}: baseline element references {status:?} evidence {target}")]
    EvidenceRefNotBaseline {
        owner: Id,
        target: Id,
        status: ElementStatus,
    },
    #[error("{owner}: derivation reference {target} does not exist")]
    DerivationRefMissing { owner: Id, target: Id },
    #[error("{owner}: derivation reference {target} is a {node_type:?}, not a DerivationRecord")]
    DerivationRefWrongType {
        owner: Id,
        target: Id,
        node_type: NodeType,
    },
    #[error("{owner}: baseline element references {status:?} derivation {target}")]
    DerivationRefNotBaseline {
        owner: Id,
        target: Id,
        status: ElementStatus,
    },
    #[error("evidence fragment {fragment}: source {source_ref} does not exist")]
    EvidenceSourceMissing { fragment: Id, source_ref: Id },
    #[error("evidence fragment {fragment}: source {source_ref} is a {node_type:?}, not a SourceArtifact")]
    EvidenceSourceWrongType {
        fragment: Id,
        source_ref: Id,
        node_type: NodeType,
    },
    #[error("baseline evidence fragment {fragment} references {status:?} source {source_ref}")]
    EvidenceSourceNotBaseline {
        fragment: Id,
        source_ref: Id,
        status: ElementStatus,
    },
    #[error("node {node}: content_hash {hash} must be a generic sha256: hash")]
    NonGenericContentHash { node: Id, hash: Hash },
    #[error("source artifact {node} must have id {expected}")]
    SourceArtifactIdMismatch { node: Id, expected: String },
    #[error("evidence fragment {node} must have id {expected}")]
    EvidenceFragmentIdMismatch { node: Id, expected: String },
    #[error("baseline edge {edge} duplicates semantic relation of edge {duplicate_of}")]
    DuplicateSemanticEdge { edge: Id, duplicate_of: Id },
}

static NO_IDS: BTreeSet<Id> = BTreeSet::new();

/// A validated, immutable Plumb Specification Graph.
///
/// Storage and indexes are private and BTreeMap-backed; indexes contain every persisted element
/// regardless of status. There are no mutable accessors: changes produce a new `Graph`.
#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    project_id: Id,
    profile_id: Id,
    nodes: BTreeMap<Id, Node>,
    edges: BTreeMap<Id, Edge>,
    by_node_type: BTreeMap<NodeType, BTreeSet<Id>>,
    by_relation_kind: BTreeMap<RelationKind, BTreeSet<Id>>,
    outgoing: BTreeMap<Id, BTreeSet<Id>>,
    incoming: BTreeMap<Id, BTreeSet<Id>>,
}

impl Graph {
    /// Builds and validates a graph. Duplicate node IDs, duplicate edge IDs and node/edge ID
    /// collisions are rejected before any map is built (nothing is silently overwritten); in
    /// that case only those violations are returned. Otherwise every violation is returned.
    pub fn new(
        project_id: Id,
        profile_id: Id,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    ) -> Result<Graph, Vec<GraphViolation>> {
        let mut duplicate_nodes: BTreeSet<Id> = BTreeSet::new();
        let mut node_map: BTreeMap<Id, Node> = BTreeMap::new();
        for node in nodes {
            if node_map.contains_key(&node.id) {
                duplicate_nodes.insert(node.id.clone());
            } else {
                node_map.insert(node.id.clone(), node);
            }
        }
        let mut duplicate_edges: BTreeSet<Id> = BTreeSet::new();
        let mut edge_map: BTreeMap<Id, Edge> = BTreeMap::new();
        for edge in edges {
            if edge_map.contains_key(&edge.id) {
                duplicate_edges.insert(edge.id.clone());
            } else {
                edge_map.insert(edge.id.clone(), edge);
            }
        }
        let collisions: BTreeSet<&Id> = edge_map
            .keys()
            .filter(|id| node_map.contains_key(*id))
            .collect();
        if !duplicate_nodes.is_empty() || !duplicate_edges.is_empty() || !collisions.is_empty() {
            let mut violations: Vec<GraphViolation> = duplicate_nodes
                .into_iter()
                .map(GraphViolation::DuplicateNodeId)
                .collect();
            violations.extend(
                duplicate_edges
                    .into_iter()
                    .map(GraphViolation::DuplicateEdgeId),
            );
            violations.extend(
                collisions
                    .into_iter()
                    .map(|id| GraphViolation::NodeEdgeIdCollision(id.clone())),
            );
            return Err(violations);
        }

        let mut by_node_type: BTreeMap<NodeType, BTreeSet<Id>> = BTreeMap::new();
        for (id, node) in &node_map {
            by_node_type
                .entry(node.payload.node_type())
                .or_default()
                .insert(id.clone());
        }
        let mut by_relation_kind: BTreeMap<RelationKind, BTreeSet<Id>> = BTreeMap::new();
        let mut outgoing: BTreeMap<Id, BTreeSet<Id>> = BTreeMap::new();
        let mut incoming: BTreeMap<Id, BTreeSet<Id>> = BTreeMap::new();
        for (id, edge) in &edge_map {
            by_relation_kind
                .entry(edge.kind.clone())
                .or_default()
                .insert(id.clone());
            outgoing
                .entry(edge.from.clone())
                .or_default()
                .insert(id.clone());
            incoming
                .entry(edge.to.clone())
                .or_default()
                .insert(id.clone());
        }

        let graph = Graph {
            project_id,
            profile_id,
            nodes: node_map,
            edges: edge_map,
            by_node_type,
            by_relation_kind,
            outgoing,
            incoming,
        };
        graph.validate()?;
        Ok(graph)
    }

    /// Re-runs every graph validity check (metamodel §17.9); violations are deterministic.
    pub fn validate(&self) -> Result<(), Vec<GraphViolation>> {
        let violations = self.collect_violations();
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }

    pub fn project_id(&self) -> &Id {
        &self.project_id
    }

    pub fn profile_id(&self) -> &Id {
        &self.profile_id
    }

    pub fn node(&self, id: &Id) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn edge(&self, id: &Id) -> Option<&Edge> {
        self.edges.get(id)
    }

    pub fn nodes(&self) -> &BTreeMap<Id, Node> {
        &self.nodes
    }

    pub fn edges(&self) -> &BTreeMap<Id, Edge> {
        &self.edges
    }

    /// IDs of every node of `node_type`, any status, in ID order.
    pub fn node_ids_by_type(&self, node_type: NodeType) -> &BTreeSet<Id> {
        self.by_node_type.get(&node_type).unwrap_or(&NO_IDS)
    }

    /// IDs of every edge of `kind`, any status, in ID order.
    pub fn edge_ids_by_kind(&self, kind: &RelationKind) -> &BTreeSet<Id> {
        self.by_relation_kind.get(kind).unwrap_or(&NO_IDS)
    }

    /// IDs of every edge whose source is `node`, any status, in ID order.
    pub fn outgoing_edge_ids(&self, node: &Id) -> &BTreeSet<Id> {
        self.outgoing.get(node).unwrap_or(&NO_IDS)
    }

    /// IDs of every edge whose target is `node`, any status, in ID order.
    pub fn incoming_edge_ids(&self, node: &Id) -> &BTreeSet<Id> {
        self.incoming.get(node).unwrap_or(&NO_IDS)
    }

    /// The `psg:sha256:` semantic hash (plan §6.2).
    pub fn semantic_hash(&self) -> Result<Hash, CoreError> {
        hash::semantic_hash(self)
    }

    /// The `ev:sha256:` evidence hash (plan §6.2).
    pub fn evidence_hash(&self) -> Result<Hash, CoreError> {
        hash::evidence_hash(self)
    }

    fn status_of(&self, id: &Id) -> Option<ElementStatus> {
        self.nodes
            .get(id)
            .map(|n| n.status)
            .or_else(|| self.edges.get(id).map(|e| e.status))
    }

    fn collect_violations(&self) -> Vec<GraphViolation> {
        let mut v = Vec::new();
        // (1) node and (2) edge local validation.
        for (id, node) in &self.nodes {
            if let Err(error) = node.validate() {
                v.push(GraphViolation::InvalidNode {
                    node: id.clone(),
                    error,
                });
            }
        }
        for (id, edge) in &self.edges {
            if let Err(error) = edge.validate() {
                v.push(GraphViolation::InvalidEdge {
                    edge: id.clone(),
                    error,
                });
            }
        }
        // (3) global ID uniqueness (duplicates within each map are impossible here).
        for id in self.edges.keys().filter(|id| self.nodes.contains_key(*id)) {
            v.push(GraphViolation::NodeEdgeIdCollision(id.clone()));
        }
        // (4) relation shapes for every edge; edge-local failures were reported in (2).
        let all_types: BTreeMap<Id, NodeType> = self
            .nodes
            .iter()
            .map(|(id, n)| (id.clone(), n.payload.node_type()))
            .collect();
        if let Err(shape) = validate_relation_shapes(&all_types, self.edges.values()) {
            v.extend(
                shape
                    .into_iter()
                    .filter(|r| !matches!(r, RelationViolation::InvalidEdge { .. }))
                    .map(GraphViolation::Relation),
            );
        }
        // (5) baseline edge endpoints.
        for (id, edge) in self.edges.iter().filter(|(_, e)| is_baseline(e.status)) {
            for endpoint in [&edge.from, &edge.to] {
                if let Some(node) = self.nodes.get(endpoint) {
                    if !is_baseline(node.status) {
                        v.push(GraphViolation::BaselineEdgeEndpointNotBaseline {
                            edge: id.clone(),
                            node: endpoint.clone(),
                            status: node.status,
                        });
                    }
                }
            }
        }
        // (6) evidence and (7) derivation references, nodes then edges.
        let owners = self
            .nodes
            .iter()
            .map(|(id, n)| (id, &n.evidence, &n.derivations))
            .chain(
                self.edges
                    .iter()
                    .map(|(id, e)| (id, &e.evidence, &e.derivations)),
            );
        let mut derivation_violations = Vec::new();
        for (owner, evidence, derivations) in owners {
            let owner_baseline = self.status_of(owner).is_some_and(is_baseline);
            for target in evidence.iter().map(|r| r.as_id()) {
                match self.nodes.get(target) {
                    None => v.push(GraphViolation::EvidenceRefMissing {
                        owner: owner.clone(),
                        target: target.clone(),
                    }),
                    Some(n) if n.payload.node_type() != NodeType::EvidenceFragment => {
                        v.push(GraphViolation::EvidenceRefWrongType {
                            owner: owner.clone(),
                            target: target.clone(),
                            node_type: n.payload.node_type(),
                        })
                    }
                    Some(n) if owner_baseline && !is_baseline(n.status) => {
                        v.push(GraphViolation::EvidenceRefNotBaseline {
                            owner: owner.clone(),
                            target: target.clone(),
                            status: n.status,
                        })
                    }
                    Some(_) => {}
                }
            }
            for target in derivations.iter().map(|r| r.as_id()) {
                match self.nodes.get(target) {
                    None => derivation_violations.push(GraphViolation::DerivationRefMissing {
                        owner: owner.clone(),
                        target: target.clone(),
                    }),
                    Some(n) if n.payload.node_type() != NodeType::DerivationRecord => {
                        derivation_violations.push(GraphViolation::DerivationRefWrongType {
                            owner: owner.clone(),
                            target: target.clone(),
                            node_type: n.payload.node_type(),
                        })
                    }
                    Some(n) if owner_baseline && !is_baseline(n.status) => derivation_violations
                        .push(GraphViolation::DerivationRefNotBaseline {
                            owner: owner.clone(),
                            target: target.clone(),
                            status: n.status,
                        }),
                    Some(_) => {}
                }
            }
        }
        v.extend(derivation_violations);
        // (8) evidence sources.
        for (id, node) in &self.nodes {
            if let NodePayload::EvidenceFragment(fragment) = &node.payload {
                match self.nodes.get(&fragment.source_ref) {
                    None => v.push(GraphViolation::EvidenceSourceMissing {
                        fragment: id.clone(),
                        source_ref: fragment.source_ref.clone(),
                    }),
                    Some(s) if s.payload.node_type() != NodeType::SourceArtifact => {
                        v.push(GraphViolation::EvidenceSourceWrongType {
                            fragment: id.clone(),
                            source_ref: fragment.source_ref.clone(),
                            node_type: s.payload.node_type(),
                        })
                    }
                    Some(s) if is_baseline(node.status) && !is_baseline(s.status) => {
                        v.push(GraphViolation::EvidenceSourceNotBaseline {
                            fragment: id.clone(),
                            source_ref: fragment.source_ref.clone(),
                            status: s.status,
                        })
                    }
                    Some(_) => {}
                }
            }
        }
        // (9) content-hash kinds, (10) source IDs and (11) fragment IDs.
        let mut id_violations = Vec::new();
        for (id, node) in &self.nodes {
            let (content_hash, expected) = match &node.payload {
                NodePayload::SourceArtifact(source) => (
                    &source.content_hash,
                    expected_source_id(&source.content_hash),
                ),
                NodePayload::EvidenceFragment(fragment) => (
                    &fragment.content_hash,
                    expected_fragment_id(&fragment.source_ref, &fragment.locator),
                ),
                _ => continue,
            };
            if content_hash.kind() != HashKind::Generic {
                v.push(GraphViolation::NonGenericContentHash {
                    node: id.clone(),
                    hash: content_hash.clone(),
                });
            }
            if id.as_str() != expected {
                id_violations.push(match node.payload {
                    NodePayload::SourceArtifact(_) => GraphViolation::SourceArtifactIdMismatch {
                        node: id.clone(),
                        expected,
                    },
                    _ => GraphViolation::EvidenceFragmentIdMismatch {
                        node: id.clone(),
                        expected,
                    },
                });
            }
        }
        id_violations.sort_by_key(|violation| match violation {
            GraphViolation::SourceArtifactIdMismatch { .. } => 0,
            _ => 1,
        });
        v.extend(id_violations);
        // (12) baseline semantic-edge duplicates.
        let mut seen: BTreeMap<(String, Id, Id, String), &Id> = BTreeMap::new();
        for (id, edge) in self.edges.iter().filter(|(_, e)| is_baseline(e.status)) {
            let key = semantic_relation_key(edge);
            match seen.get(&key) {
                Some(first) => v.push(GraphViolation::DuplicateSemanticEdge {
                    edge: id.clone(),
                    duplicate_of: (*first).clone(),
                }),
                None => {
                    seen.insert(key, id);
                }
            }
        }
        // (13) cardinality and (14) cycles over the baseline subset.
        let baseline_types: BTreeMap<Id, NodeType> = self
            .nodes
            .iter()
            .filter(|(_, n)| is_baseline(n.status))
            .map(|(id, n)| (id.clone(), n.payload.node_type()))
            .collect();
        let baseline_edges = self.edges.values().filter(|e| is_baseline(e.status));
        if let Err(constraints) = validate_relation_constraints(&baseline_types, baseline_edges) {
            v.extend(constraints.into_iter().map(GraphViolation::Relation));
        }
        v
    }
}

/// `src:<first 16 hex of the content hash digest>` (plan §6.1).
fn expected_source_id(content_hash: &Hash) -> String {
    format!("src:{}", digest_prefix(content_hash.as_str()))
}

/// `evd:<first 16 hex of SHA-256(source_ref || "|" || RFC 8785 JSON of locator)>` (plan §6.1).
fn expected_fragment_id(source_ref: &Id, locator: &crate::payload::EvidenceLocator) -> String {
    let mut bytes = source_ref.as_str().as_bytes().to_vec();
    bytes.push(b'|');
    // Locator values are validated finite, so canonicalization cannot fail; an empty digest
    // input on failure would simply never match a real ID.
    bytes.extend(to_canonical_json(locator).unwrap_or_default());
    format!(
        "evd:{}",
        digest_prefix(Hash::content_sha256(&bytes).as_str())
    )
}

fn digest_prefix(hash: &str) -> &str {
    let digest = hash.rsplit(':').next().unwrap_or_default();
    &digest[..16.min(digest.len())]
}

/// The semantic relation key of an edge (metamodel §17.9).
fn semantic_relation_key(edge: &Edge) -> (String, Id, Id, String) {
    let discriminator = match &edge.properties {
        RelationProperties::None => String::new(),
        RelationProperties::SchemaFor(p) => format!("{:?}", p.role),
        RelationProperties::Extension(map) => {
            String::from_utf8(to_canonical_json(map).unwrap_or_default()).unwrap_or_default()
        }
    };
    (
        edge.kind.as_str().to_owned(),
        edge.from.clone(),
        edge.to.clone(),
        discriminator,
    )
}
