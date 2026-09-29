//! Plumb Specification Graph (PSG): envelope primitives, the complete `NodePayload` semantic
//! enum, the `Node` and `Edge` envelopes, and the typed relation registry
//! (metamodel §§2-18, 24).

pub mod audit;
pub mod edge;
pub mod extensions;
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
pub use node::{Node, NodeError};
pub use payload::*;
pub use refs::{DerivationRef, EvidenceRef};
pub use registry::{
    allowed_schema_roles, relation_def, validate_relations, Cardinality, CyclePolicy,
    Directionality, NodeCategory, RelationDef, RelationPropertySchema, RelationViolation,
    TypePredicate, TypeTerm, RELATION_REGISTRY,
};
pub use relations::{
    RelationKind, RelationKindError, RelationProperties, RelationPropertiesError,
    SchemaBindingRole, SchemaForProperties,
};
pub use standards::{MappingRole, MappingStrength, StandardMapping};
pub use status::ElementStatus;
