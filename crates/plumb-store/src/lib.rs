//! Immutable PSG graph revisions, branch heads and compare-and-swap commit over the pilot
//! SQLite database (compiler architecture §§4.1, 5.8, 21; plan §6).

pub mod branches;
pub mod revisions;
pub mod schema;
pub mod sqlite;

use plumb_artifacts::ArtifactStoreError;
use plumb_core::{CoreError, Hash, Id};
use plumb_patch::PatchError;
use plumb_psg::{ElementStatus, NodeType};
use thiserror::Error;

pub use branches::{BranchName, InvalidBranchName};
pub use revisions::{
    GraphRevision, InvalidRevisionId, LoadedRevision, RevisionId, RevisionWriteMeta,
};
pub use schema::{PSG_SCHEMA_VERSION, STORE_SCHEMA_VERSION};
pub use sqlite::SqliteRevisionStore;

/// Why a revision-store operation failed. A failed write leaves the database unchanged.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The underlying SQLite operation failed.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A core primitive could not be produced (canonical JSON, hashes).
    #[error(transparent)]
    Core(#[from] CoreError),
    /// Storing the accepted Patch artifact failed.
    #[error(transparent)]
    Artifact(#[from] ArtifactStoreError),
    /// The PatchSet could not be applied to the head graph.
    #[error(transparent)]
    Patch(#[from] PatchError),
    /// A value could not be serialized for persistence.
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),

    /// The store already holds a revision or branch; one database holds exactly one project.
    #[error("the revision store is already initialized")]
    AlreadyInitialized,

    /// `schema_meta.version` is not [`STORE_SCHEMA_VERSION`].
    #[error("unsupported store schema version {found}")]
    UnsupportedStoreSchemaVersion { found: i64 },
    /// A versioned store is missing required tables or has a malformed `schema_meta`.
    #[error("corrupt store schema: {reason}")]
    CorruptStoreSchema { reason: String },
    /// Graph-store tables exist without `schema_meta`; the schema version is unknown.
    #[error("graph-store tables exist without schema_meta")]
    UnversionedStore,

    /// A stored revision was written under a different PSG schema version.
    #[error("revision {revision} has unsupported PSG schema version {found}")]
    UnsupportedPsgSchemaVersion { revision: String, found: i64 },
    /// No revision is stored under this ID.
    #[error("revision {0} not found")]
    RevisionNotFound(RevisionId),
    /// A stored revision fails an integrity check.
    #[error("corrupt revision {revision}: {reason}")]
    CorruptRevision { revision: String, reason: String },

    #[error(transparent)]
    InvalidRevisionId(#[from] InvalidRevisionId),
    #[error(transparent)]
    InvalidBranchName(#[from] InvalidBranchName),

    /// `profile_hash` must be a generic `sha256:` hash.
    #[error("profile hash must be a generic sha256: hash, got {0}")]
    InvalidProfileHashKind(Hash),
    /// `rule_pack_hash` must be a generic `sha256:` hash.
    #[error("rule pack hash must be a generic sha256: hash, got {0}")]
    InvalidRulePackHashKind(Hash),
    /// A stored accepted Patch artifact reference is not a generic `sha256:` hash.
    #[error("revision {revision} patch artifact ref must be a generic sha256: hash, got {hash}")]
    InvalidPatchArtifactHashKind { revision: String, hash: String },

    #[error("branch {0} already exists")]
    BranchAlreadyExists(BranchName),
    #[error("branch {0} not found")]
    BranchNotFound(BranchName),
    /// The branch head is not the expected head (`STALE_BASE`); the caller must re-evaluate.
    #[error("stale base on branch {branch}: expected {expected}, actual {actual}")]
    StaleBase {
        branch: BranchName,
        expected: RevisionId,
        actual: RevisionId,
    },

    /// The next global revision version is outside the positive SQLite INTEGER domain.
    #[error("revision version overflow")]
    VersionOverflow,

    #[error("duplicate decision ref {0}")]
    DuplicateDecisionRef(Id),
    #[error("decision ref {0} does not resolve to a node")]
    DecisionRefMissing(Id),
    #[error("decision ref {id} is a {node_type:?} node, not a ResolutionDecision")]
    DecisionRefWrongType { id: Id, node_type: NodeType },
    #[error("decision ref {id} has non-baseline status {status:?}")]
    DecisionRefNotBaseline { id: Id, status: ElementStatus },
}
