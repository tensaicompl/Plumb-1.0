//! The artifact store contract (compiler architecture §24).

use plumb_core::{Hash, Timestamp};
use thiserror::Error;

use crate::model::{Artifact, ArtifactKind, UnknownArtifactKind};

/// Errors returned by an [`ArtifactStore`].
#[derive(Debug, Error)]
pub enum ArtifactStoreError {
    /// No artifact is stored under this hash.
    #[error("artifact {0} not found")]
    NotFound(Hash),
    /// Artifact-store keys must be generic `sha256:` hashes.
    #[error("artifact store keys must be generic sha256: hashes, got {0}")]
    NonGenericHash(Hash),
    /// The same bytes are already stored with a different kind or media type.
    #[error(
        "artifact {hash} already stored as {existing_kind} ({existing_media_type}); \
         put requested {requested_kind} ({requested_media_type})"
    )]
    ArtifactMetadataConflict {
        hash: Hash,
        existing_kind: ArtifactKind,
        existing_media_type: String,
        requested_kind: ArtifactKind,
        requested_media_type: String,
    },
    /// The bytes stored under this hash differ from the supplied bytes.
    #[error("integrity error: stored bytes for {0} differ from the supplied bytes")]
    IntegrityViolation(Hash),
    /// A persisted kind string is not one of the artifact kinds.
    #[error(transparent)]
    UnknownArtifactKind(#[from] UnknownArtifactKind),
    /// A persisted row holds a value that is not a valid primitive (e.g. its timestamp).
    #[error("invalid persisted artifact row for {hash}: {reason}")]
    InvalidStoredRow { hash: String, reason: String },
    /// The underlying SQLite operation failed.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// A content-addressed, append-only artifact store.
///
/// There is deliberately no update, replace or delete operation: artifact bytes and their
/// persisted metadata are immutable after the first successful insertion.
pub trait ArtifactStore {
    /// Stores `bytes` and returns their generic `sha256:` hash.
    ///
    /// Storing already-present bytes with the same kind and media type is idempotent and keeps
    /// the original `created_at`. A different kind or media type returns
    /// [`ArtifactStoreError::ArtifactMetadataConflict`] and leaves the stored artifact unchanged.
    fn put(
        &mut self,
        kind: ArtifactKind,
        media_type: &str,
        bytes: &[u8],
        created_at: Timestamp,
    ) -> Result<Hash, ArtifactStoreError>;

    /// Loads the artifact stored under `hash`, or [`ArtifactStoreError::NotFound`].
    fn get(&self, hash: &Hash) -> Result<Artifact, ArtifactStoreError>;

    /// Whether an artifact is stored under `hash`.
    fn exists(&self, hash: &Hash) -> Result<bool, ArtifactStoreError>;
}
