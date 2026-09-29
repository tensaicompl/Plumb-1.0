//! Plumb Specification Graph (PSG): envelope primitives, the complete `NodePayload` semantic
//! enum and the `Node` envelope (metamodel §§2-16, 24).
//!
//! `RelationKind` and `Edge` are added by task F0.5.

pub mod audit;
pub mod extensions;
pub mod node;
pub mod payload;
pub mod refs;
pub mod standards;
pub mod status;

pub use audit::{AuditMeta, AuditMetaError};
pub use extensions::{ExtensionKey, InvalidExtensionKey};
pub use node::{Node, NodeError};
pub use payload::*;
pub use refs::{DerivationRef, EvidenceRef};
pub use standards::{MappingRole, MappingStrength, StandardMapping};
pub use status::ElementStatus;
