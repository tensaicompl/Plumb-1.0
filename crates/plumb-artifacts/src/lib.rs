//! Content-addressed, immutable compiler artifacts and their SQLite store
//! (compiler architecture §24).

pub mod model;
pub mod sqlite;
pub mod store;

pub use model::{Artifact, ArtifactKind, UnknownArtifactKind};
pub use sqlite::{ensure_artifact_schema, put_artifact_in_transaction, SqliteArtifactStore};
pub use store::{ArtifactStore, ArtifactStoreError};
