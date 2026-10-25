//! SQLite-backed artifact store.

use std::path::Path;

use plumb_core::{Hash, HashKind, Timestamp};
use rusqlite::{
    params, Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior,
};

use crate::model::{Artifact, ArtifactKind};
use crate::store::{ArtifactStore, ArtifactStoreError};

/// Connection initialization required by plan §6, with the frozen durability profile
/// (Fable #199, `docs/operations.md`): WAL journal and `synchronous = FULL`.
const CONNECTION_PRAGMAS: &str = "
    PRAGMA foreign_keys = ON;
    PRAGMA journal_mode = WAL;
    PRAGMA synchronous = FULL;
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

    /// Opens the artifact store of an existing database at `path` read-write. The file and
    /// the `artifacts` table are never created ([`ArtifactStoreError::NotInitialized`]).
    pub fn open_existing(path: impl AsRef<Path>) -> Result<Self, ArtifactStoreError> {
        let flags = OpenFlags::default() - OpenFlags::SQLITE_OPEN_CREATE;
        let conn = open_flags(path.as_ref(), flags)?;
        require_artifacts_table(&conn)?;
        conn.execute_batch(CONNECTION_PRAGMAS)?;
        Ok(Self { conn })
    }

    /// Opens the artifact store of an existing database at `path` with SQLite read-only access
    /// and no journal-mode change; every `put` fails.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, ArtifactStoreError> {
        let flags = (OpenFlags::default()
            - OpenFlags::SQLITE_OPEN_CREATE
            - OpenFlags::SQLITE_OPEN_READ_WRITE)
            | OpenFlags::SQLITE_OPEN_READ_ONLY;
        let conn = open_flags(path.as_ref(), flags)?;
        require_artifacts_table(&conn)?;
        Ok(Self { conn })
    }

    /// Opens an artifact store in a private in-memory SQLite database.
    pub fn open_in_memory() -> Result<Self, ArtifactStoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, ArtifactStoreError> {
        conn.execute_batch(CONNECTION_PRAGMAS)?;
        ensure_artifact_schema(&conn)?;
        Ok(Self { conn })
    }

    /// The configured `PRAGMA synchronous` of this connection (2 is `FULL`).
    pub fn synchronous(&self) -> Result<i64, ArtifactStoreError> {
        Ok(self
            .conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))?)
    }
}

/// Opens `path` with `flags`; a missing or non-SQLite file is
/// [`ArtifactStoreError::NotInitialized`].
fn open_flags(path: &Path, flags: OpenFlags) -> Result<Connection, ArtifactStoreError> {
    let not_initialized = |e: &rusqlite::Error| {
        matches!(
            e.sqlite_error_code(),
            Some(rusqlite::ErrorCode::CannotOpen | rusqlite::ErrorCode::NotADatabase)
        )
    };
    let conn = Connection::open_with_flags(path, flags).map_err(|e| {
        if not_initialized(&e) {
            ArtifactStoreError::NotInitialized
        } else {
            e.into()
        }
    })?;
    // A non-SQLite file only fails on the first read.
    match conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'artifacts')",
        [],
        |row| row.get::<_, bool>(0),
    ) {
        Err(e) if not_initialized(&e) => Err(ArtifactStoreError::NotInitialized),
        Err(e) => Err(e.into()),
        Ok(_) => Ok(conn),
    }
}

fn require_artifacts_table(conn: &Connection) -> Result<(), ArtifactStoreError> {
    let present: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'artifacts')",
        [],
        |row| row.get(0),
    )?;
    if present {
        Ok(())
    } else {
        Err(ArtifactStoreError::NotInitialized)
    }
}

/// Requires the stored `bytes` to hash to the requested and stored key `hash` (H0.10): a
/// mismatch is typed corruption and the bytes are never returned as a valid artifact.
pub fn verify_artifact_bytes(hash: &Hash, bytes: &[u8]) -> Result<(), ArtifactStoreError> {
    if Hash::content_sha256(bytes) == *hash {
        Ok(())
    } else {
        Err(ArtifactStoreError::Corrupt {
            hash: hash.clone(),
            reason: "the stored bytes do not hash to the artifact key",
        })
    }
}

/// Creates the `artifacts` table on `conn` if it does not already exist.
///
/// Lets another component that owns the SQLite connection (the revision store) share the
/// artifact table without opening a separate [`SqliteArtifactStore`].
pub fn ensure_artifact_schema(conn: &Connection) -> Result<(), ArtifactStoreError> {
    conn.execute_batch(CREATE_ARTIFACTS)?;
    Ok(())
}

/// Stores `bytes` inside the caller's transaction `tx` and returns their generic `sha256:` hash.
///
/// Applies exactly the [`ArtifactStore::put`] rules: storing already-present bytes with the same
/// kind and media type is idempotent and keeps the original `created_at`; a different kind or
/// media type is [`ArtifactStoreError::ArtifactMetadataConflict`]; stored bytes that differ from
/// `bytes` are [`ArtifactStoreError::IntegrityViolation`]. Existing rows are never updated or
/// deleted. The caller owns `tx`: this function never begins, commits or rolls it back.
pub fn put_artifact_in_transaction(
    tx: &Transaction<'_>,
    kind: ArtifactKind,
    media_type: &str,
    bytes: &[u8],
    created_at: Timestamp,
) -> Result<Hash, ArtifactStoreError> {
    let hash = Hash::content_sha256(bytes);
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
            Ok(hash)
        }
        Some((existing_kind, existing_media_type, existing_bytes)) => {
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
        // IMMEDIATE takes the write lock up front, so the existence check and the insert are
        // one atomic step even when several connections race on the same bytes.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // On error `tx` is dropped, which rolls back.
        let hash = put_artifact_in_transaction(&tx, kind, media_type, bytes, created_at)?;
        tx.commit()?;
        Ok(hash)
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
        verify_artifact_bytes(hash, &bytes)?;
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
