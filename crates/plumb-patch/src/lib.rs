//! Serializable semantic patches and their deterministic, atomic application to an immutable
//! PSG `Graph` (metamodel §20).

pub mod apply;
pub mod diff;
pub mod model;

pub use apply::{apply_patch, ApplyResult, PatchError};
pub use diff::{DiffChange, DiffElementKind, DiffEntry, GraphDelta};
pub use model::{ElementPrecondition, MergePolicy, PatchModelError, PatchSet, SemanticPatch};
