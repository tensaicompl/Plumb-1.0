//! Store schema versions, the exact pilot SQLite schema (plan §6) and its initialization policy.

use std::collections::BTreeSet;
use std::time::Duration;

use plumb_artifacts::ensure_artifact_schema;
use rusqlite::{Connection, TransactionBehavior};

use crate::StoreError;

/// Version of the physical SQLite schema recorded in `schema_meta`.
pub const STORE_SCHEMA_VERSION: i64 = 1;

/// Version of the persisted PSG interpretation/validation contract a snapshot was written under.
pub const PSG_SCHEMA_VERSION: u32 = 1;

/// Tables owned by the graph store; any of them without `schema_meta` is an unversioned store.
pub(crate) const GRAPH_STORE_TABLES: [&str; 5] = [
    "graph_revisions",
    "revision_nodes",
    "revision_edges",
    "branch_heads",
    "ledger_events",
];

/// Every table a v1 store must contain.
const REQUIRED_V1_TABLES: [&str; 7] = [
    "schema_meta",
    "artifacts",
    "graph_revisions",
    "revision_nodes",
    "revision_edges",
    "branch_heads",
    "ledger_events",
];

/// The store-owned part of the plan §6 schema; `artifacts` comes from `plumb-artifacts`.
const CREATE_STORE_TABLES: &str = "
CREATE TABLE schema_meta (
  version INTEGER NOT NULL
);

CREATE TABLE graph_revisions (
  id TEXT PRIMARY KEY,
  version INTEGER NOT NULL UNIQUE CHECK (version > 0),

  parent_id TEXT NULL,

  project_id TEXT NOT NULL,
  psg_schema_version INTEGER NOT NULL CHECK (psg_schema_version > 0),

  semantic_hash TEXT NOT NULL,
  evidence_hash TEXT NOT NULL,

  profile_ref TEXT NOT NULL,
  profile_hash TEXT NOT NULL,
  rule_pack_hash TEXT NOT NULL,

  patch_artifact_hash TEXT NULL,
  decision_refs_json TEXT NOT NULL,

  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,

  FOREIGN KEY (parent_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (patch_artifact_hash) REFERENCES artifacts(hash)
);

CREATE TABLE revision_nodes (
  revision_id TEXT NOT NULL,
  node_id TEXT NOT NULL,
  node_json TEXT NOT NULL,

  PRIMARY KEY (revision_id, node_id),
  FOREIGN KEY (revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE revision_edges (
  revision_id TEXT NOT NULL,
  edge_id TEXT NOT NULL,
  edge_json TEXT NOT NULL,

  PRIMARY KEY (revision_id, edge_id),
  FOREIGN KEY (revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE branch_heads (
  name TEXT PRIMARY KEY,
  revision_id TEXT NOT NULL,
  updated_at TEXT NOT NULL,

  FOREIGN KEY (revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE ledger_events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  revision_id TEXT NULL,
  event_json TEXT NOT NULL,

  FOREIGN KEY (revision_id) REFERENCES graph_revisions(id)
);
";

/// SQLite busy timeout: concurrent writers wait for each other instead of failing with BUSY.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

enum SchemaState {
    Fresh,
    Current,
}

fn table_names(conn: &Connection) -> Result<BTreeSet<String>, StoreError> {
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(names)
}

/// Classifies the database without modifying it.
fn classify(conn: &Connection) -> Result<SchemaState, StoreError> {
    let tables = table_names(conn)?;
    if !tables.contains("schema_meta") {
        if GRAPH_STORE_TABLES.iter().any(|t| tables.contains(*t)) {
            return Err(StoreError::UnversionedStore);
        }
        return Ok(SchemaState::Fresh);
    }
    let mut stmt = conn.prepare("SELECT version FROM schema_meta")?;
    let versions = stmt
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let [version] = versions.as_slice() else {
        return Err(StoreError::CorruptStoreSchema {
            reason: format!(
                "schema_meta has {} rows, expected exactly 1",
                versions.len()
            ),
        });
    };
    if *version != STORE_SCHEMA_VERSION {
        return Err(StoreError::UnsupportedStoreSchemaVersion { found: *version });
    }
    let missing: Vec<&str> = REQUIRED_V1_TABLES
        .into_iter()
        .filter(|t| !tables.contains(*t))
        .collect();
    if !missing.is_empty() {
        return Err(StoreError::CorruptStoreSchema {
            reason: format!("missing v1 tables: {}", missing.join(", ")),
        });
    }
    Ok(SchemaState::Current)
}

/// Configures `conn` and brings the database to store schema v1, or fails without altering it.
///
/// A fresh database (optionally holding only the F0.2 `artifacts` table) is initialized in one
/// transaction; a versioned v1 store is verified and left unchanged; anything else is rejected.
pub(crate) fn open_connection(conn: &mut Connection) -> Result<(), StoreError> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    // Classify before the persistent journal-mode change so a rejected database is untouched.
    let state = classify(conn)?;
    conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
        row.get::<_, String>(0)
    })?;
    conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
    if let SchemaState::Fresh = state {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Another connection may have initialized the database while we waited for the lock.
        if let SchemaState::Fresh = classify(&tx)? {
            ensure_artifact_schema(&tx)?;
            tx.execute_batch(CREATE_STORE_TABLES)?;
            tx.execute(
                "INSERT INTO schema_meta (version) VALUES (?1)",
                [STORE_SCHEMA_VERSION],
            )?;
        }
        tx.commit()?;
    }
    Ok(())
}
