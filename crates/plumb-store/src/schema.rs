//! Store schema versions, the exact pilot SQLite schema (plan §6, Hotfix 065 workflow tables,
//! Hotfix 067 stage tables) and its initialization and migration policy.

use std::collections::BTreeSet;
use std::time::Duration;

use plumb_artifacts::ensure_artifact_schema;
use rusqlite::{Connection, TransactionBehavior};

use crate::StoreError;

/// Version of the physical SQLite schema recorded in `schema_meta`. Version 2 (Hotfix 065)
/// adds the durable workflow tables; version 3 (A0.4, Hotfix 067) admits the `stage` workflow
/// origin and adds the stage-orchestration tables. Valid v1 and v2 stores are migrated
/// transactionally.
pub const STORE_SCHEMA_VERSION: i64 = 3;

/// The earlier store schema versions that are migrated.
const STORE_SCHEMA_VERSION_V1: i64 = 1;
const STORE_SCHEMA_VERSION_V2: i64 = 2;

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

/// The stage-orchestration tables added by store schema v3 (Hotfix 067).
pub(crate) const STAGE_TABLES: [&str; 5] = [
    "stage_recipe_jobs",
    "stage_jobs",
    "stage_recipe_job_steps",
    "stage_job_proposals",
    "gate_reports",
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

/// The v3 `workflow_proposals` definition under `name` (Hotfix 067: the `stage` origin, whose
/// `origin_ref` is always NULL). Workflow rows never participate in a revision, its hashes or
/// its identity; they hold canonical opaque JSON and lifecycle state.
fn create_workflow_proposals(name: &str) -> String {
    format!(
        "
CREATE TABLE {name} (
  proposal_id TEXT PRIMARY KEY,
  base_revision_id TEXT NOT NULL,
  proposal_json TEXT NOT NULL,
  intake_json TEXT NOT NULL,
  proposal_kind TEXT NOT NULL
    CHECK (proposal_kind IN ('link', 'merge', 'supersede', 'create', 'accept')),
  state TEXT NOT NULL
    CHECK (state IN ('prepared', 'pending', 'accepted', 'rejected', 'failed')),
  origin_kind TEXT NOT NULL CHECK (origin_kind IN ('intake', 'question', 'session', 'stage')),
  origin_ref TEXT NULL,
  created_at TEXT NOT NULL,
  resolved_at TEXT NULL,
  resolved_revision_id TEXT NULL,

  CHECK (origin_kind <> 'stage' OR origin_ref IS NULL),
  FOREIGN KEY (base_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (resolved_revision_id) REFERENCES graph_revisions(id)
);
"
    )
}

/// The v2 -> v3 rebuild of `workflow_proposals`; every row is copied unchanged.
const REBUILD_WORKFLOW_PROPOSALS: &str = "
INSERT INTO workflow_proposals_v3 SELECT proposal_id, base_revision_id, proposal_json,
  intake_json, proposal_kind, state, origin_kind, origin_ref, created_at, resolved_at,
  resolved_revision_id FROM workflow_proposals;
DROP TABLE workflow_proposals;
ALTER TABLE workflow_proposals_v3 RENAME TO workflow_proposals;
";

/// The session tables of store schema v2 (Hotfix 065), unchanged in v3.
const CREATE_SESSION_TABLES: &str = "
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

/// The stage-orchestration tables of store schema v3 (Hotfix 067). JSON columns hold RFC 8785
/// text; typed decoding belongs to the application layer.
const CREATE_STAGE_TABLES: &str = "
CREATE TABLE stage_recipe_jobs (
  recipe_job_id TEXT PRIMARY KEY,
  recipe_job_json TEXT NOT NULL,
  config_json TEXT NOT NULL,
  recipe TEXT NOT NULL CHECK (recipe IN ('intake', 'lint', 'extract', 'consolidate', 'calc', 'rules',
    'analyse', 'flows', 'scenarios', 'tags', 'run')),
  branch TEXT NOT NULL,
  base_revision_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'acquiring', 'evaluating', 'ready', 'stale',
    'failed', 'committed')),
  result_revision_id TEXT NULL,
  failure_code TEXT NULL,
  failure_message TEXT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK ((status IN ('ready', 'committed')) = (result_revision_id IS NOT NULL)),
  CHECK ((status = 'failed') = (failure_code IS NOT NULL)),
  CHECK ((failure_code IS NULL) = (failure_message IS NULL)),
  FOREIGN KEY (base_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (result_revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE stage_jobs (
  stage_job_id TEXT PRIMARY KEY,
  stage_job_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'acquiring', 'evaluating', 'ready', 'stale',
    'failed', 'committed')),
  compile_run_ref TEXT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (compile_run_ref) REFERENCES artifacts(hash)
);

CREATE TABLE stage_recipe_job_steps (
  recipe_job_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  phase TEXT NOT NULL CHECK (phase IN ('i0', 'f1', 'f2', 'f4', 'segmentation', 'classification',
    'consolidate', 'calc', 'rules', 'analyse', 'flows', 'scenarios', 'tags', 'execute',
    'verification')),
  compiler_stage TEXT NOT NULL CHECK (compiler_stage IN ('S0', 'S1', 'S2', 'S3', 'S4')),
  input_revision_id TEXT NOT NULL,
  stage_job_id TEXT NULL,
  compile_run_ref TEXT NULL,
  intermediate_artifact_ref TEXT NULL,
  gate TEXT NULL CHECK (gate IN ('I0', 'F1', 'F2', 'F3', 'F4')),
  result_revision_id TEXT NULL,
  PRIMARY KEY (recipe_job_id, ordinal),
  UNIQUE (recipe_job_id, phase),
  FOREIGN KEY (recipe_job_id) REFERENCES stage_recipe_jobs(recipe_job_id),
  FOREIGN KEY (input_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (stage_job_id) REFERENCES stage_jobs(stage_job_id),
  FOREIGN KEY (compile_run_ref) REFERENCES artifacts(hash),
  FOREIGN KEY (intermediate_artifact_ref) REFERENCES artifacts(hash),
  FOREIGN KEY (result_revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE stage_job_proposals (
  proposal_id TEXT PRIMARY KEY,
  recipe_job_id TEXT NOT NULL,
  stage_job_id TEXT NOT NULL,
  compile_run_ref TEXT NOT NULL,
  FOREIGN KEY (proposal_id) REFERENCES workflow_proposals(proposal_id),
  FOREIGN KEY (recipe_job_id) REFERENCES stage_recipe_jobs(recipe_job_id),
  FOREIGN KEY (stage_job_id) REFERENCES stage_jobs(stage_job_id),
  FOREIGN KEY (compile_run_ref) REFERENCES artifacts(hash)
);

CREATE TABLE gate_reports (
  revision_id TEXT NOT NULL,
  gate TEXT NOT NULL CHECK (gate IN ('I0', 'F1', 'F2', 'F3', 'F4')),
  report_hash TEXT NOT NULL,
  report_json TEXT NOT NULL,
  supplemental_input_hash TEXT NULL,
  evaluation_at TEXT NULL,
  recipe_job_id TEXT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (revision_id, gate),
  CHECK ((gate = 'F3') = (evaluation_at IS NOT NULL)),
  FOREIGN KEY (revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (recipe_job_id) REFERENCES stage_recipe_jobs(recipe_job_id)
);
";

/// SQLite busy timeout: concurrent writers wait for each other instead of failing with BUSY.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

enum SchemaState {
    Fresh,
    /// A valid version-1 store that must be migrated to version 3.
    V1,
    /// A valid version-2 store that must be migrated to version 3.
    V2,
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
            .chain(STAGE_TABLES.iter())
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
        STORE_SCHEMA_VERSION_V2 => SchemaState::V2,
        found => return Err(StoreError::UnsupportedStoreSchemaVersion { found }),
    };
    let missing = missing_tables(&tables, &REQUIRED_V1_TABLES);
    if !missing.is_empty() {
        return Err(StoreError::CorruptStoreSchema {
            reason: format!("missing v1 tables: {}", missing.join(", ")),
        });
    }
    if let SchemaState::V2 | SchemaState::Current = state {
        let missing = missing_tables(&tables, &WORKFLOW_TABLES);
        if !missing.is_empty() {
            return Err(StoreError::CorruptStoreSchema {
                reason: format!("missing v2 tables: {}", missing.join(", ")),
            });
        }
    }
    if let SchemaState::Current = state {
        let missing = missing_tables(&tables, &STAGE_TABLES);
        if !missing.is_empty() {
            return Err(StoreError::CorruptStoreSchema {
                reason: format!("missing v3 tables: {}", missing.join(", ")),
            });
        }
    }
    Ok(state)
}

/// Configures `conn` and brings the database to store schema v3, or fails without altering it.
///
/// A fresh database (optionally holding only the F0.2 `artifacts` table) is initialized at v3 in
/// one transaction; a valid v1 or v2 store is migrated to v3 in one transaction (any failure
/// rolls the whole migration back; existing revision, artifact and workflow rows are never
/// rewritten, and v2 workflow proposals are copied unchanged into the rebuilt table); a
/// versioned v3 store is verified and left unchanged; anything else is rejected.
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
            tx.execute_batch(&create_workflow_proposals("workflow_proposals"))?;
            tx.execute_batch(CREATE_SESSION_TABLES)?;
            tx.execute_batch(CREATE_STAGE_TABLES)?;
            tx.execute(
                "INSERT INTO schema_meta (version) VALUES (?1)",
                [STORE_SCHEMA_VERSION],
            )?;
        }
        SchemaState::V1 => {
            tx.execute_batch(&create_workflow_proposals("workflow_proposals"))?;
            tx.execute_batch(CREATE_SESSION_TABLES)?;
            tx.execute_batch(CREATE_STAGE_TABLES)?;
            set_version(&tx, STORE_SCHEMA_VERSION_V1)?;
        }
        SchemaState::V2 => {
            tx.execute_batch(&create_workflow_proposals("workflow_proposals_v3"))?;
            tx.execute_batch(REBUILD_WORKFLOW_PROPOSALS)?;
            tx.execute_batch(CREATE_STAGE_TABLES)?;
            set_version(&tx, STORE_SCHEMA_VERSION_V2)?;
        }
        SchemaState::Current => {}
    }
    tx.commit()?;
    Ok(())
}

/// Moves `schema_meta` from `from` to the current version inside the migration transaction.
fn set_version(tx: &rusqlite::Transaction<'_>, from: i64) -> Result<(), StoreError> {
    let updated = tx.execute(
        "UPDATE schema_meta SET version = ?1 WHERE version = ?2",
        [STORE_SCHEMA_VERSION, from],
    )?;
    if updated != 1 {
        return Err(StoreError::CorruptStoreSchema {
            reason: "schema_meta changed during migration".to_owned(),
        });
    }
    Ok(())
}
