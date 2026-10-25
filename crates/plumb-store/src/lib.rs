//! Immutable PSG graph revisions, branch heads and compare-and-swap commit over the pilot
//! SQLite database (compiler architecture §§4.1, 5.8, 21; plan §6), plus the durable workflow
//! records of store schema v2 (Hotfix 065) and the stage-orchestration records of store schema v3
//! (A0.4, Hotfix 067).

pub mod branches;
pub mod revisions;
pub mod schema;
pub mod sqlite;
pub mod stages;
pub mod workflow;

use plumb_artifacts::ArtifactStoreError;
use plumb_core::{CoreError, Hash, Id};
use plumb_patch::PatchError;
use plumb_psg::{ElementStatus, NodeType};
use thiserror::Error;

pub use branches::{BranchName, InvalidBranchName};
pub use revisions::{
    CommitResult, GraphRevision, InvalidRevisionId, LoadedRevision, RevisionId, RevisionWriteMeta,
};
pub use schema::{
    Durability, StoreOpenMode, PSG_SCHEMA_VERSION, STORE_SCHEMA_VERSION, SYNCHRONOUS_FULL,
};
pub use sqlite::SqliteRevisionStore;
pub use stages::{
    GateReportRecord, NewGateReport, NewStageRecipeJob, NewStageStep, SqliteStageStore,
    StageAttemptRecord, StageJobRecord, StageLease, StageProposalLink, StageRecipeJobRecord,
    StageStatus, StageStepRecord,
};
pub use workflow::occurrence_id;
pub use workflow::{
    NewWorkflowProposal, NewWorkflowSession, NewWorkflowTurn, SqliteWorkflowStore, WorkflowInsert,
    WorkflowOrigin, WorkflowProposal, WorkflowProposalKind, WorkflowProposalState, WorkflowSession,
    WorkflowTurn, WorkflowTurnState,
};

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

    /// An existing-database open found no PLUMB project: a missing, zero-byte, non-SQLite or
    /// foreign database, or one without the PLUMB tables (architecture §33.9).
    #[error("the database is not an initialized PLUMB project")]
    DatabaseNotInitialized,
    /// A read-only open needs a write (a schema migration).
    #[error("the database is opened read-only and needs a schema migration")]
    DatabaseReadOnly,

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

    /// No workflow record of this kind is stored under this ID.
    #[error("workflow {kind} {id} not found")]
    WorkflowRecordNotFound { kind: &'static str, id: String },
    /// A workflow record with this ID already exists.
    #[error("workflow {kind} {id} already exists")]
    WorkflowRecordExists { kind: &'static str, id: String },
    /// A workflow transition found the record in another state.
    #[error("workflow {kind} {id} is {actual}, expected {expected}")]
    WorkflowStateConflict {
        kind: &'static str,
        id: String,
        expected: String,
        actual: String,
    },
    /// A workflow transition to this state is not defined.
    #[error("workflow {kind} {id} cannot move to {to}")]
    WorkflowInvalidTransition {
        kind: &'static str,
        id: String,
        to: &'static str,
    },
    /// A workflow proposal's origin is not allowed (Hotfix 067: a stage proposal has no ref).
    #[error("workflow proposal {id} has an invalid origin: {reason}")]
    WorkflowInvalidOrigin { id: String, reason: &'static str },
    /// A stage proposal ID is already stored under another origin or without a stage link.
    #[error("stage proposal {id} conflicts with a stored workflow proposal")]
    StageProposalConflict { id: String },
    /// An accepted proposal must resolve to a revision on its branch lineage after its base
    /// (Fable #1).
    #[error("proposal {id} cannot be accepted by revision {revision}: {reason}")]
    WorkflowResolutionNotDescendant {
        id: String,
        revision: String,
        reason: &'static str,
    },
    /// A stored workflow record fails an integrity check.
    #[error("corrupt workflow {kind} {id}: {reason}")]
    CorruptWorkflowRecord {
        kind: &'static str,
        id: String,
        reason: String,
    },
}

/// The typed class of a [`StoreError`] (architecture §33.9): what the layers above map to their
/// stable error codes. Everything not listed keeps its operation-specific mapping (`Other`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreErrorClass {
    /// The branch has no head.
    BranchNotFound,
    /// The database is not an initialized PLUMB project.
    NotInitialized,
    /// SQLite busy or locked.
    Busy,
    /// A write on a read-only database.
    ReadOnly,
    /// Detected store corruption (schema, revision, artifact or workflow integrity).
    Corrupt,
    /// The stored data was written under an unsupported store or PSG schema version.
    UnsupportedVersion,
    /// Any other failure.
    Other,
}

/// The class of a raw SQLite failure.
fn sqlite_class(error: &rusqlite::Error) -> StoreErrorClass {
    use rusqlite::ErrorCode;
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => StoreErrorClass::Busy,
        Some(ErrorCode::ReadOnly) => StoreErrorClass::ReadOnly,
        Some(ErrorCode::DatabaseCorrupt) => StoreErrorClass::Corrupt,
        Some(ErrorCode::NotADatabase) => StoreErrorClass::NotInitialized,
        _ => StoreErrorClass::Other,
    }
}

/// The class of an artifact-store failure.
pub fn artifact_error_class(error: &ArtifactStoreError) -> StoreErrorClass {
    match error {
        ArtifactStoreError::Sqlite(e) => sqlite_class(e),
        ArtifactStoreError::Corrupt { .. }
        | ArtifactStoreError::IntegrityViolation(_)
        | ArtifactStoreError::InvalidStoredRow { .. }
        | ArtifactStoreError::UnknownArtifactKind(_) => StoreErrorClass::Corrupt,
        ArtifactStoreError::NotInitialized => StoreErrorClass::NotInitialized,
        _ => StoreErrorClass::Other,
    }
}

impl StoreError {
    /// The typed class of this error.
    pub fn class(&self) -> StoreErrorClass {
        match self {
            StoreError::Sqlite(e) => sqlite_class(e),
            StoreError::Artifact(e) => artifact_error_class(e),
            StoreError::BranchNotFound(_) => StoreErrorClass::BranchNotFound,
            StoreError::DatabaseNotInitialized => StoreErrorClass::NotInitialized,
            StoreError::DatabaseReadOnly => StoreErrorClass::ReadOnly,
            StoreError::UnsupportedStoreSchemaVersion { .. }
            | StoreError::UnsupportedPsgSchemaVersion { .. } => StoreErrorClass::UnsupportedVersion,
            StoreError::CorruptStoreSchema { .. }
            | StoreError::UnversionedStore
            | StoreError::CorruptRevision { .. }
            | StoreError::InvalidPatchArtifactHashKind { .. }
            | StoreError::CorruptWorkflowRecord { .. } => StoreErrorClass::Corrupt,
            _ => StoreErrorClass::Other,
        }
    }
}
