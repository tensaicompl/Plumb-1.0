//! `semantic_hash` and `evidence_hash` projections (plan §6.2).
//!
//! Each hash is SHA-256 over the RFC 8785 canonical JSON of exactly one projection object,
//! built only from baseline-participating elements. Payload arrays keep their persisted order;
//! only the envelope `evidence` and `standards` collections are normalized.
//!
//! R0.5 (plan §6.2, architecture §4.8): a participating `ResolutionDecision` contributes without
//! its operational provenance (`decided_at`, `patch_ref`), and a proposal-acceptance companion
//! does not contribute at all, nor does any edge with such an endpoint. The element projection
//! (metamodel §20.1) keeps the complete record.

use plumb_core::{canonical_hash, to_canonical_json, CoreError, Hash, HashKind};
use serde_json::{json, Map, Value};

use crate::edge::Edge;
use crate::graph::{is_baseline, Graph};
use crate::node::Node;
use crate::payload::{NodePayload, NodeType};
use crate::refs::EvidenceRef;
use crate::standards::StandardMapping;

/// Node types excluded from `semantic_hash` entirely: evidence, provenance, derived
/// diagnostics/workflow and runtime/conformance/proof outputs (plan §6.2).
pub const SEMANTIC_HASH_EXCLUDED_NODE_TYPES: [NodeType; 12] = [
    NodeType::SourceArtifact,
    NodeType::EvidenceFragment,
    NodeType::DerivationRecord,
    NodeType::Agent,
    NodeType::Finding,
    NodeType::Question,
    NodeType::ScenarioRun,
    NodeType::TestExecution,
    NodeType::TestReceipt,
    NodeType::CodeBinding,
    NodeType::ArchitectureCheck,
    NodeType::CoverageRecord,
];

/// Whether nodes of this type can contribute to `semantic_hash`.
pub fn contributes_to_semantic_hash(node_type: NodeType) -> bool {
    !SEMANTIC_HASH_EXCLUDED_NODE_TYPES.contains(&node_type)
}

/// The `ResolutionDecision` answer kind of a proposal-acceptance companion (metamodel §6.3).
const PROPOSAL_ACCEPTANCE_KIND: &str = "proposal_acceptance";

/// The `ResolutionDecision` payload fields that are operational provenance (architecture §4.8):
/// omitted from the semantic projection, kept by the element projection.
const DECISION_PROVENANCE_FIELDS: [&str; 2] = ["decided_at", "patch_ref"];

/// Whether `node` is a proposal-acceptance companion: a `ResolutionDecision` with no
/// `question_ref` whose `answer.kind` is `proposal_acceptance` (metamodel §6.3).
pub fn is_proposal_acceptance_companion(node: &Node) -> bool {
    matches!(&node.payload, NodePayload::ResolutionDecision(d)
        if d.question_ref.is_none()
            && d.answer.get("kind").and_then(Value::as_str) == Some(PROPOSAL_ACCEPTANCE_KIND))
}

/// Whether `node` is part of the `semantic_hash` projection: baseline-participating, of a
/// contributing node type and not a proposal-acceptance companion (R0.5). This is the exact
/// rule `semantic_projection` uses.
pub fn node_contributes_to_semantic_hash(node: &Node) -> bool {
    is_baseline(node.status)
        && contributes_to_semantic_hash(node.payload.node_type())
        && !is_proposal_acceptance_companion(node)
}

/// Whether `edge` is part of the `semantic_hash` projection of `graph`: baseline-participating
/// with both endpoint node types contributing (no separate relation-exclusion list) and no
/// proposal-acceptance companion endpoint (R0.5). This is the exact rule `semantic_projection`
/// uses.
pub fn edge_contributes_to_semantic_hash(graph: &Graph, edge: &Edge) -> bool {
    let endpoint_contributes = |id| {
        graph.node(id).is_some_and(|n| {
            contributes_to_semantic_hash(n.payload.node_type())
                && !is_proposal_acceptance_companion(n)
        })
    };
    is_baseline(edge.status) && endpoint_contributes(&edge.from) && endpoint_contributes(&edge.to)
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Evidence references sorted by ID string.
fn evidence_projection(evidence: &[EvidenceRef]) -> Value {
    let mut ids: Vec<&str> = evidence.iter().map(|r| r.as_str()).collect();
    ids.sort_unstable();
    json!(ids)
}

/// Standard mappings with `validator_rules` sorted, ordered by canonical bytes.
fn standards_projection(standards: &[StandardMapping]) -> Value {
    let mut mappings: Vec<(Vec<u8>, Value)> = standards
        .iter()
        .map(|mapping| {
            let mut normalized = mapping.clone();
            normalized.validator_rules.sort_unstable();
            let value = to_value(&normalized);
            (to_canonical_json(&value).unwrap_or_default(), value)
        })
        .collect();
    mappings.sort_by(|a, b| a.0.cmp(&b.0));
    Value::Array(mappings.into_iter().map(|(_, value)| value).collect())
}

/// The payload as serialized, except that a `View` omits `layout_ref` and `style_ref`.
fn payload_projection(payload: &NodePayload) -> Value {
    let mut value = to_value(payload);
    if matches!(payload, NodePayload::View(_)) {
        if let Some(data) = value.get_mut("data").and_then(Value::as_object_mut) {
            data.remove("layout_ref");
            data.remove("style_ref");
        }
    }
    value
}

/// The semantic payload projection: [`payload_projection`] with the operational provenance of a
/// `ResolutionDecision` omitted (absent, not `null`; R0.5).
fn semantic_payload_projection(payload: &NodePayload) -> Value {
    let mut value = payload_projection(payload);
    if matches!(payload, NodePayload::ResolutionDecision(_)) {
        if let Some(data) = value.get_mut("data").and_then(Value::as_object_mut) {
            for field in DECISION_PROVENANCE_FIELDS {
                data.remove(field);
            }
        }
    }
    value
}

/// The semantic node projection.
fn node_projection(node: &Node) -> Value {
    node_projection_with(node, semantic_payload_projection(&node.payload))
}

fn node_projection_with(node: &Node, payload: Value) -> Value {
    json!({
        "id": node.id,
        "status": node.status,
        "payload": payload,
        "evidence": evidence_projection(&node.evidence),
        "standards": standards_projection(&node.standards),
        "tags": node.tags,
        "extensions": node.extensions,
    })
}

fn edge_projection(edge: &Edge) -> Value {
    json!({
        "id": edge.id,
        "status": edge.status,
        "kind": edge.kind,
        "from": edge.from,
        "to": edge.to,
        "properties": edge.properties,
        "evidence": evidence_projection(&edge.evidence),
        "standards": standards_projection(&edge.standards),
    })
}

/// The element projection of a node (metamodel §20.1): the node projection over the complete
/// payload (a `ResolutionDecision` keeps `decided_at` and `patch_ref`) plus
/// `"element_kind": "node"`, for every status and node type.
pub fn node_element_projection(node: &Node) -> Value {
    with_element_kind(
        node_projection_with(node, payload_projection(&node.payload)),
        "node",
    )
}

/// The element projection of an edge (metamodel §20.1): the semantic edge projection plus
/// `"element_kind": "edge"`, for every status.
pub fn edge_element_projection(edge: &Edge) -> Value {
    with_element_kind(edge_projection(edge), "edge")
}

fn with_element_kind(mut projection: Value, kind: &str) -> Value {
    if let Some(object) = projection.as_object_mut() {
        object.insert("element_kind".into(), Value::from(kind));
    }
    projection
}

/// Generic `sha256:` element hash of a node, used by patch preconditions and revisions.
pub fn node_element_hash(node: &Node) -> Result<Hash, CoreError> {
    canonical_hash(HashKind::Generic, &node_element_projection(node))
}

/// Generic `sha256:` element hash of an edge, used by patch preconditions and revisions.
pub fn edge_element_hash(edge: &Edge) -> Result<Hash, CoreError> {
    canonical_hash(HashKind::Generic, &edge_element_projection(edge))
}

/// The exact `semantic_hash` input object: `{project_id, profile_id, nodes, edges}` with
/// participating nodes and edges sorted by ID.
pub fn semantic_projection(graph: &Graph) -> Value {
    let nodes: Vec<Value> = graph
        .nodes()
        .values()
        .filter(|n| node_contributes_to_semantic_hash(n))
        .map(node_projection)
        .collect();
    let edges: Vec<Value> = graph
        .edges()
        .values()
        .filter(|e| edge_contributes_to_semantic_hash(graph, e))
        .map(edge_projection)
        .collect();
    json!({
        "project_id": graph.project_id(),
        "profile_id": graph.profile_id(),
        "nodes": nodes,
        "edges": edges,
    })
}

/// The exact `evidence_hash` input object: `{sources, fragments}` from baseline-participating
/// SourceArtifact and EvidenceFragment nodes, each sorted by ID.
pub fn evidence_projection_object(graph: &Graph) -> Value {
    let mut sources = Vec::new();
    let mut fragments = Vec::new();
    for (id, node) in graph.nodes().iter().filter(|(_, n)| is_baseline(n.status)) {
        match &node.payload {
            NodePayload::SourceArtifact(source) => {
                let mut entry = Map::new();
                entry.insert("id".into(), to_value(id));
                entry.insert("content_hash".into(), to_value(&source.content_hash));
                sources.push(Value::Object(entry));
            }
            NodePayload::EvidenceFragment(fragment) => {
                let mut entry = Map::new();
                entry.insert("id".into(), to_value(id));
                entry.insert("source_ref".into(), to_value(&fragment.source_ref));
                entry.insert("locator".into(), to_value(&fragment.locator));
                entry.insert("content_hash".into(), to_value(&fragment.content_hash));
                fragments.push(Value::Object(entry));
            }
            _ => {}
        }
    }
    json!({"sources": sources, "fragments": fragments})
}

/// `psg:sha256:` hash of [`semantic_projection`].
pub fn semantic_hash(graph: &Graph) -> Result<Hash, CoreError> {
    canonical_hash(HashKind::Semantic, &semantic_projection(graph))
}

/// `ev:sha256:` hash of [`evidence_projection_object`].
pub fn evidence_hash(graph: &Graph) -> Result<Hash, CoreError> {
    canonical_hash(HashKind::Evidence, &evidence_projection_object(graph))
}
