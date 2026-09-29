//! Plumb Specification Graph (PSG) envelope primitives (metamodel §§2-4, 16).
//!
//! `NodePayload` and `Node` are added by task F0.4; `RelationKind` and `Edge` by task F0.5.

pub mod audit;
pub mod extensions;
pub mod refs;
pub mod standards;
pub mod status;

pub use audit::{AuditMeta, AuditMetaError};
pub use extensions::{ExtensionKey, InvalidExtensionKey};
pub use refs::{DerivationRef, EvidenceRef};
pub use standards::{MappingRole, MappingStrength, StandardMapping};
pub use status::ElementStatus;
