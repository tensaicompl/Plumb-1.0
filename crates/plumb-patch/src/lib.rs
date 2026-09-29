//! Serializable semantic patches and their deterministic, atomic application to an immutable
//! PSG `Graph` (metamodel §20).

pub mod apply;
pub mod diff;
pub mod impact;
pub mod model;
pub mod proposal;

pub use apply::{apply_patch, ApplyResult, PatchError};
pub use diff::{DiffChange, DiffElementKind, DiffEntry, GraphDelta};
pub use impact::{
    compute_impact, earliest_gate, impact_direction, projection_families, AffectedGateNamespaceSet,
    AffectedProjectionSet, ChangedSet, DirtySet, ImpactDirection, ImpactError, ImpactReport,
    ProjectionKind, UnknownProjectionKind,
};
pub use model::{ElementPrecondition, MergePolicy, PatchModelError, PatchSet, SemanticPatch};
pub use proposal::{
    AcceptancePolicy, Proposal, ProposalError, ProposalMateriality, UnknownProposalValue,
};
