//! Plumb Specification Graph (PSG): envelope primitives, the complete `NodePayload` semantic
//! enum, the `Node` and `Edge` envelopes, the typed relation registry, and the validated
//! `Graph` with its semantic/evidence hashes (metamodel §§2-24; plan §6.2).

pub mod audit;
pub mod edge;
pub mod extensions;
pub mod graph;
pub mod hash;
pub mod node;
pub mod payload;
pub mod refs;
pub mod registry;
pub mod relations;
pub mod standards;
pub mod status;

pub use audit::{AuditMeta, AuditMetaError};
pub use edge::{Edge, EdgeError};
pub use extensions::{ExtensionKey, InvalidExtensionKey};
pub use graph::{is_baseline, Graph, GraphViolation};
pub use hash::{
    contributes_to_semantic_hash, edge_contributes_to_semantic_hash, edge_element_hash,
    edge_element_projection, evidence_hash, evidence_projection_object,
    node_contributes_to_semantic_hash, node_element_hash, node_element_projection, semantic_hash,
    semantic_projection, SEMANTIC_HASH_EXCLUDED_NODE_TYPES,
};
pub use node::{Node, NodeError};
pub use payload::*;
pub use refs::{DerivationRef, EvidenceRef};
pub use registry::{
    allowed_schema_roles, relation_def, validate_relation_constraints, validate_relation_shapes,
    validate_relations, Cardinality, CyclePolicy, Directionality, NodeCategory, RelationDef,
    RelationPropertySchema, RelationViolation, TypePredicate, TypeTerm, RELATION_REGISTRY,
};
pub use relations::{
    RelationKind, RelationKindError, RelationProperties, RelationPropertiesError,
    SchemaBindingRole, SchemaForProperties,
};
pub use standards::{MappingRole, MappingStrength, StandardMapping};
pub use status::ElementStatus;
