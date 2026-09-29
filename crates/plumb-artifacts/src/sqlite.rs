//! SQLite-backed artifact store.

use std::path::Path;

use plumb_core::{Hash, HashKind, Timestamp};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::model::{Artifact, ArtifactKind};
use crate::store::{ArtifactStore, ArtifactStoreError};

/// Connection initialization required by plan §6.
const CONNECTION_PRAGMAS: &str = "
    PRAGMA foreign_keys = ON;
    PRAGMA journal_mode = WAL;
    PRAGMA synchronous = NORMAL;
";

/// The `artifacts` table exactly as specified by plan §6.
const CREATE_ARTIFACTS: &str = "
    CREATE TABLE IF NOT EXISTS artifacts (
      hash TEXT PRIMARY KEY,
      kind TEXT NOT NULL,
      media_type TEXT NOT NULL,
      bytes BLOB NOT NULL,
      created_at TEXT NOT NULL
    );
";

/// An [`ArtifactStore`] persisted in a SQLite `artifacts` table.
#[derive(Debug)]
pub struct SqliteArtifactStore {
    conn: Connection,
}

impl SqliteArtifactStore {
    /// Opens (creating if needed) the artifact store in the SQLite database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ArtifactStoreError> {
        Self::init(Connection::open(path)?)
    }

    /// Opens an artifact store in a private in-memory SQLite database.
    pub fn open_in_memory() -> Result<Self, ArtifactStoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, ArtifactStoreError> {
        conn.execute_batch(CONNECTION_PRAGMAS)?;
        conn.execute_batch(CREATE_ARTIFACTS)?;
        Ok(Self { conn })
    }
}

fn require_generic(hash: &Hash) -> Result<(), ArtifactStoreError> {
    match hash.kind() {
        HashKind::Generic => Ok(()),
        HashKind::Semantic | HashKind::Evidence => {
            Err(ArtifactStoreError::NonGenericHash(hash.clone()))
        }
    }
}

impl ArtifactStore for SqliteArtifactStore {
    fn put(
        &mut self,
        kind: ArtifactKind,
        media_type: &str,
        bytes: &[u8],
        created_at: Timestamp,
    ) -> Result<Hash, ArtifactStoreError> {
        let hash = Hash::content_sha256(bytes);
        // IMMEDIATE takes the write lock up front, so the existence check and the insert are
        // one atomic step even when several connections race on the same bytes.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, String, Vec<u8>)> = tx
            .query_row(
                "SELECT kind, media_type, bytes FROM artifacts WHERE hash = ?1",
                [hash.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        match existing {
            None => {
                tx.execute(
                    "INSERT INTO artifacts (hash, kind, media_type, bytes, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        hash.as_str(),
                        kind.as_str(),
                        media_type,
                        bytes,
                        created_at.to_string()
                    ],
                )?;
                tx.commit()?;
                Ok(hash)
            }
            Some((existing_kind, existing_media_type, existing_bytes)) => {
                // Every branch below leaves the row untouched; dropping `tx` rolls back.
                if existing_bytes != bytes {
                    return Err(ArtifactStoreError::IntegrityViolation(hash));
                }
                let existing_kind: ArtifactKind = existing_kind.parse()?;
                if existing_kind != kind || existing_media_type != media_type {
                    return Err(ArtifactStoreError::ArtifactMetadataConflict {
                        hash,
                        existing_kind,
                        existing_media_type,
                        requested_kind: kind,
                        requested_media_type: media_type.to_owned(),
                    });
                }
                Ok(hash)
            }
        }
    }

    fn get(&self, hash: &Hash) -> Result<Artifact, ArtifactStoreError> {
        require_generic(hash)?;
        let row: Option<(String, String, Vec<u8>, String)> = self
            .conn
            .query_row(
                "SELECT kind, media_type, bytes, created_at FROM artifacts WHERE hash = ?1",
                [hash.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let (kind, media_type, bytes, created_at) =
            row.ok_or_else(|| ArtifactStoreError::NotFound(hash.clone()))?;
        let created_at = created_at.parse().map_err(|e: plumb_core::CoreError| {
            ArtifactStoreError::InvalidStoredRow {
                hash: hash.to_string(),
                reason: e.to_string(),
            }
        })?;
        Ok(Artifact {
            hash: hash.clone(),
            kind: kind.parse()?,
            media_type,
            bytes,
            created_at,
        })
    }

    fn exists(&self, hash: &Hash) -> Result<bool, ArtifactStoreError> {
        require_generic(hash)?;
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM artifacts WHERE hash = ?1)",
            [hash.as_str()],
            |row| row.get(0),
        )?)
    }
}
