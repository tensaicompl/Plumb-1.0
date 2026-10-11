//! Store schema versions, the exact pilot SQLite schema (plan §6, Hotfix 065 workflow tables)
//! and its initialization and migration policy.

use std::collections::BTreeSet;
use std::time::Duration;

use plumb_artifacts::ensure_artifact_schema;
use rusqlite::{Connection, TransactionBehavior};

use crate::StoreError;

/// Version of the physical SQLite schema recorded in `schema_meta`. Version 2 (Hotfix 065)
/// adds the durable workflow tables; a valid version-1 store is migrated transactionally.
pub const STORE_SCHEMA_VERSION: i64 = 2;

/// The only earlier store schema version that is migrated.
const STORE_SCHEMA_VERSION_V1: i64 = 1;

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

/// The durable workflow tables added by store schema v2 (Hotfix 065).
pub(crate) const WORKFLOW_TABLES: [&str; 3] = [
    "workflow_proposals",
    "workflow_sessions",
    "workflow_session_turns",
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

/// The workflow tables of store schema v2 (Hotfix 065). Workflow rows never participate in a
/// revision, its hashes or its identity; they hold canonical opaque JSON and lifecycle state.
const CREATE_WORKFLOW_TABLES: &str = "
CREATE TABLE workflow_proposals (
  proposal_id TEXT PRIMARY KEY,
  base_revision_id TEXT NOT NULL,
  proposal_json TEXT NOT NULL,
  intake_json TEXT NOT NULL,
  proposal_kind TEXT NOT NULL
    CHECK (proposal_kind IN ('link', 'merge', 'supersede', 'create', 'accept')),
  state TEXT NOT NULL
    CHECK (state IN ('prepared', 'pending', 'accepted', 'rejected', 'failed')),
  origin_kind TEXT NOT NULL CHECK (origin_kind IN ('intake', 'question', 'session')),
  origin_ref TEXT NULL,
  created_at TEXT NOT NULL,
  resolved_at TEXT NULL,
  resolved_revision_id TEXT NULL,

  FOREIGN KEY (base_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (resolved_revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE workflow_sessions (
  session_id TEXT PRIMARY KEY,
  session_json TEXT NOT NULL,
  owner_actor_ref TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE workflow_session_turns (
  session_id TEXT NOT NULL,
  turn_index INTEGER NOT NULL CHECK (turn_index >= 0),
  state TEXT NOT NULL CHECK (state IN ('prepared', 'committed', 'failed')),
  base_revision_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  expected_node_refs_json TEXT NOT NULL,
  proposal_ids_json TEXT NOT NULL,
  requires_reason_count INTEGER NOT NULL CHECK (requires_reason_count >= 0),
  committed_revision_id TEXT NULL,
  created_at TEXT NOT NULL,
  failure TEXT NULL,

  PRIMARY KEY (session_id, turn_index),
  FOREIGN KEY (session_id) REFERENCES workflow_sessions(session_id),
  FOREIGN KEY (base_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (committed_revision_id) REFERENCES graph_revisions(id)
);
";

/// SQLite busy timeout: concurrent writers wait for each other instead of failing with BUSY.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

enum SchemaState {
    Fresh,
    /// A valid version-1 store that must be migrated to version 2.
    V1,
    Current,
}

fn table_names(conn: &Connection) -> Result<BTreeSet<String>, StoreError> {
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(names)
}

fn missing_tables<'a>(tables: &BTreeSet<String>, required: &[&'a str]) -> Vec<&'a str> {
    required
        .iter()
        .copied()
        .filter(|t| !tables.contains(*t))
        .collect()
}

/// Classifies the database without modifying it.
fn classify(conn: &Connection) -> Result<SchemaState, StoreError> {
    let tables = table_names(conn)?;
    if !tables.contains("schema_meta") {
        if GRAPH_STORE_TABLES
            .iter()
            .chain(WORKFLOW_TABLES.iter())
            .any(|t| tables.contains(*t))
        {
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
    let state = match *version {
        STORE_SCHEMA_VERSION => SchemaState::Current,
        STORE_SCHEMA_VERSION_V1 => SchemaState::V1,
        found => return Err(StoreError::UnsupportedStoreSchemaVersion { found }),
    };
    let missing = missing_tables(&tables, &REQUIRED_V1_TABLES);
    if !missing.is_empty() {
        return Err(StoreError::CorruptStoreSchema {
            reason: format!("missing v1 tables: {}", missing.join(", ")),
        });
    }
    if let SchemaState::Current = state {
        let missing = missing_tables(&tables, &WORKFLOW_TABLES);
        if !missing.is_empty() {
            return Err(StoreError::CorruptStoreSchema {
                reason: format!("missing v2 tables: {}", missing.join(", ")),
            });
        }
    }
    Ok(state)
}

/// Configures `conn` and brings the database to store schema v2, or fails without altering it.
///
/// A fresh database (optionally holding only the F0.2 `artifacts` table) is initialized at v2 in
/// one transaction; a valid v1 store is migrated to v2 in one transaction (any failure rolls the
/// whole migration back; existing revision and artifact rows are never rewritten); a versioned
/// v2 store is verified and left unchanged; anything else is rejected.
pub(crate) fn open_connection(conn: &mut Connection) -> Result<(), StoreError> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    // Classify before the persistent journal-mode change so a rejected database is untouched.
    let state = classify(conn)?;
    conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
        row.get::<_, String>(0)
    })?;
    conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
    if let SchemaState::Current = state {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Another connection may have initialized or migrated the database while we waited.
    match classify(&tx)? {
        SchemaState::Fresh => {
            ensure_artifact_schema(&tx)?;
            tx.execute_batch(CREATE_STORE_TABLES)?;
            tx.execute_batch(CREATE_WORKFLOW_TABLES)?;
            tx.execute(
                "INSERT INTO schema_meta (version) VALUES (?1)",
                [STORE_SCHEMA_VERSION],
            )?;
        }
        SchemaState::V1 => {
            tx.execute_batch(CREATE_WORKFLOW_TABLES)?;
            let updated = tx.execute(
                "UPDATE schema_meta SET version = ?1 WHERE version = ?2",
                [STORE_SCHEMA_VERSION, STORE_SCHEMA_VERSION_V1],
            )?;
            if updated != 1 {
                return Err(StoreError::CorruptStoreSchema {
                    reason: "schema_meta changed during migration".to_owned(),
                });
            }
        }
        SchemaState::Current => {}
    }
    tx.commit()?;
    Ok(())
}
