//! Store schema versions, the exact pilot SQLite schema (plan §6, Hotfix 065 workflow tables,
//! Hotfix 067 stage tables) and its initialization and migration policy.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use plumb_artifacts::ensure_artifact_schema;
use rusqlite::{params, Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::workflow::occurrence_id;
use crate::StoreError;

/// Version of the physical SQLite schema recorded in `schema_meta`. Version 2 (Hotfix 065)
/// adds the durable workflow tables; version 3 (A0.4, Hotfix 067) admits the `stage` workflow
/// origin and adds the stage-orchestration tables; version 4 (H0.10, architecture §33.11) keys
/// workflow proposals and their stage links by occurrence `(proposal_id, base_revision_id)`;
/// version 5 (H0.11, architecture §33.12) admits the terminal proposal state `abandoned`.
/// Valid v1, v2, v3 and v4 stores are migrated transactionally.
pub const STORE_SCHEMA_VERSION: i64 = 5;

/// The earlier store schema versions that are migrated.
const STORE_SCHEMA_VERSION_V1: i64 = 1;
const STORE_SCHEMA_VERSION_V2: i64 = 2;
const STORE_SCHEMA_VERSION_V3: i64 = 3;
const STORE_SCHEMA_VERSION_V4: i64 = 4;

/// Version of the persisted PSG interpretation/validation contract a snapshot was written under
/// (owned by `plumb-psg`, next to the serialized forms it versions).
pub use plumb_psg::PSG_SCHEMA_VERSION;

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

/// The v5 `workflow_proposals` definition under `name`: one row per proposal occurrence
/// `(proposal_id, base_revision_id)` with its durable `occurrence_id` (v4, H0.10, architecture
/// §33.11) and the terminal `abandoned` state (v5, H0.11); the `stage` origin (Hotfix 067) has
/// no `origin_ref`. Workflow rows never participate
/// in a revision, its hashes or its identity; they hold canonical opaque JSON and lifecycle
/// state.
fn create_workflow_proposals(name: &str) -> String {
    format!(
        "
CREATE TABLE {name} (
  proposal_id TEXT NOT NULL,
  base_revision_id TEXT NOT NULL,
  proposal_json TEXT NOT NULL,
  intake_json TEXT NOT NULL,
  proposal_kind TEXT NOT NULL
    CHECK (proposal_kind IN ('link', 'merge', 'supersede', 'create', 'accept')),
  state TEXT NOT NULL
    CHECK (state IN ('prepared', 'pending', 'accepted', 'rejected', 'failed', 'abandoned')),
  origin_kind TEXT NOT NULL CHECK (origin_kind IN ('intake', 'question', 'session', 'stage')),
  origin_ref TEXT NULL,
  created_at TEXT NOT NULL,
  resolved_at TEXT NULL,
  resolved_revision_id TEXT NULL,
  occurrence_id TEXT NOT NULL UNIQUE,

  PRIMARY KEY (proposal_id, base_revision_id),
  CHECK (origin_kind <> 'stage' OR origin_ref IS NULL),
  FOREIGN KEY (base_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (resolved_revision_id) REFERENCES graph_revisions(id)
);
"
    )
}

/// The v4 `stage_job_proposals` definition under `name`: the stage link of one proposal
/// occurrence.
fn create_stage_job_proposals(name: &str) -> String {
    format!(
        "
CREATE TABLE {name} (
  proposal_id TEXT NOT NULL,
  base_revision_id TEXT NOT NULL,
  recipe_job_id TEXT NOT NULL,
  stage_job_id TEXT NOT NULL,
  compile_run_ref TEXT NOT NULL,
  PRIMARY KEY (proposal_id, base_revision_id),
  FOREIGN KEY (proposal_id, base_revision_id)
    REFERENCES workflow_proposals(proposal_id, base_revision_id),
  FOREIGN KEY (recipe_job_id) REFERENCES stage_recipe_jobs(recipe_job_id),
  FOREIGN KEY (stage_job_id) REFERENCES stage_jobs(stage_job_id),
  FOREIGN KEY (compile_run_ref) REFERENCES artifacts(hash)
);
"
    )
}

/// The v2/v3 -> v5 rebuild of `workflow_proposals`: every row is copied unchanged as its own
/// occurrence, with its occurrence key computed from `(proposal_id, base_revision_id)`.
fn migrate_workflow_proposals(tx: &Transaction<'_>) -> Result<(), StoreError> {
    tx.execute_batch(&create_workflow_proposals("workflow_proposals_v4"))?;
    let keys: Vec<(String, String)> = {
        let mut stmt =
            tx.prepare("SELECT proposal_id, base_revision_id FROM workflow_proposals")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (proposal_id, base_revision_id) in keys {
        let corrupt = || StoreError::CorruptStoreSchema {
            reason: format!("workflow proposal {proposal_id} has an invalid key"),
        };
        let occurrence = occurrence_id(
            &proposal_id.parse().map_err(|_| corrupt())?,
            &base_revision_id.parse().map_err(|_| corrupt())?,
        )?;
        tx.execute(
            "INSERT INTO workflow_proposals_v4 SELECT proposal_id, base_revision_id, \
             proposal_json, intake_json, proposal_kind, state, origin_kind, origin_ref, \
             created_at, resolved_at, resolved_revision_id, ?2 FROM workflow_proposals \
             WHERE proposal_id = ?1",
            params![proposal_id, occurrence.as_str()],
        )?;
    }
    tx.execute_batch(
        "DROP TABLE workflow_proposals;
         ALTER TABLE workflow_proposals_v4 RENAME TO workflow_proposals;",
    )?;
    Ok(())
}

/// The v4 -> v5 rebuild of `workflow_proposals` (H0.11): only the state CHECK changes, so every
/// row is copied unchanged, column for column. The stage links keep referencing the table by
/// name.
fn migrate_v4_proposal_states(tx: &Transaction<'_>) -> Result<(), StoreError> {
    tx.execute_batch(&create_workflow_proposals("workflow_proposals_v5"))?;
    tx.execute_batch(
        "INSERT INTO workflow_proposals_v5 (proposal_id, base_revision_id, proposal_json, \
           intake_json, proposal_kind, state, origin_kind, origin_ref, created_at, resolved_at, \
           resolved_revision_id, occurrence_id) \
         SELECT proposal_id, base_revision_id, proposal_json, intake_json, proposal_kind, state, \
           origin_kind, origin_ref, created_at, resolved_at, resolved_revision_id, occurrence_id \
           FROM workflow_proposals;
         DROP TABLE workflow_proposals;
         ALTER TABLE workflow_proposals_v5 RENAME TO workflow_proposals;",
    )?;
    Ok(())
}

/// The v3 -> v4 rebuild of `stage_job_proposals`: each link is bound to the occurrence of its
/// proposal (v3 has exactly one row per proposal ID).
fn migrate_stage_job_proposals(tx: &Transaction<'_>) -> Result<(), StoreError> {
    tx.execute_batch(&create_stage_job_proposals("stage_job_proposals_v4"))?;
    tx.execute_batch(
        "INSERT INTO stage_job_proposals_v4 SELECT l.proposal_id, p.base_revision_id, \
           l.recipe_job_id, l.stage_job_id, l.compile_run_ref \
           FROM stage_job_proposals l JOIN workflow_proposals p ON p.proposal_id = l.proposal_id;
         DROP TABLE stage_job_proposals;
         ALTER TABLE stage_job_proposals_v4 RENAME TO stage_job_proposals;",
    )?;
    Ok(())
}

/// Every foreign key holds after a migration (deferred checks would otherwise only fail the
/// commit with an untyped error).
fn require_foreign_keys(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let violations: i64 =
        tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violations != 0 {
        return Err(StoreError::CorruptStoreSchema {
            reason: format!("{violations} foreign key violations after the migration"),
        });
    }
    Ok(())
}

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

/// `PRAGMA synchronous` value `FULL`: the frozen durability profile (Fable #199).
pub const SYNCHRONOUS_FULL: i64 = 2;

/// How a store connection opens its database (architecture §33.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOpenMode {
    /// `init` and tests: a fresh database is initialized at the current schema and an older
    /// store is migrated.
    CreateOrMigrate,
    /// Every other read-write open: the database must already hold a PLUMB project (an older
    /// store is still migrated in place); the file and the schema are never created.
    Existing,
    /// Read-only commands: SQLite read-only access, no journal-mode change, no migration and
    /// no write of any kind. The database must hold a current PLUMB project.
    ReadOnly,
}

/// The configured durability pragmas of one store connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Durability {
    /// `PRAGMA journal_mode` (lowercase).
    pub journal_mode: String,
    /// `PRAGMA synchronous` (0 OFF, 1 NORMAL, 2 FULL, 3 EXTRA).
    pub synchronous: i64,
}

/// The durability pragmas currently configured on `conn`.
pub(crate) fn durability_of(conn: &Connection) -> Result<Durability, StoreError> {
    Ok(Durability {
        journal_mode: conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?,
        synchronous: conn.query_row("PRAGMA synchronous", [], |row| row.get(0))?,
    })
}

enum SchemaState {
    Fresh,
    /// A valid version-1 store that must be migrated to version 5.
    V1,
    /// A valid version-2 store that must be migrated to version 5.
    V2,
    /// A valid version-3 store that must be migrated to version 5.
    V3,
    /// A valid version-4 store that must be migrated to version 5.
    V4,
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
        STORE_SCHEMA_VERSION_V3 => SchemaState::V3,
        STORE_SCHEMA_VERSION_V4 => SchemaState::V4,
        found => return Err(StoreError::UnsupportedStoreSchemaVersion { found }),
    };
    let missing = missing_tables(&tables, &REQUIRED_V1_TABLES);
    if !missing.is_empty() {
        return Err(StoreError::CorruptStoreSchema {
            reason: format!("missing v1 tables: {}", missing.join(", ")),
        });
    }
    if let SchemaState::V2 | SchemaState::V3 | SchemaState::V4 | SchemaState::Current = state {
        let missing = missing_tables(&tables, &WORKFLOW_TABLES);
        if !missing.is_empty() {
            return Err(StoreError::CorruptStoreSchema {
                reason: format!("missing v2 tables: {}", missing.join(", ")),
            });
        }
    }
    if let SchemaState::V3 | SchemaState::V4 | SchemaState::Current = state {
        let missing = missing_tables(&tables, &STAGE_TABLES);
        if !missing.is_empty() {
            return Err(StoreError::CorruptStoreSchema {
                reason: format!("missing v3 tables: {}", missing.join(", ")),
            });
        }
    }
    Ok(state)
}

/// Opens `path` in `mode` and brings it to (or verifies) the current store schema.
///
/// `Existing` and `ReadOnly` never create the file; a missing, zero-byte, non-SQLite or foreign
/// database, or one without the PLUMB tables, is [`StoreError::DatabaseNotInitialized`].
pub(crate) fn open_database(path: &Path, mode: StoreOpenMode) -> Result<Connection, StoreError> {
    let flags = match mode {
        StoreOpenMode::CreateOrMigrate => OpenFlags::default(),
        StoreOpenMode::Existing => OpenFlags::default() - OpenFlags::SQLITE_OPEN_CREATE,
        StoreOpenMode::ReadOnly => {
            (OpenFlags::default()
                - OpenFlags::SQLITE_OPEN_CREATE
                - OpenFlags::SQLITE_OPEN_READ_WRITE)
                | OpenFlags::SQLITE_OPEN_READ_ONLY
        }
    };
    let opened = Connection::open_with_flags(path, flags);
    let mut conn = match opened {
        Err(error) if mode != StoreOpenMode::CreateOrMigrate && is_not_a_project(&error) => {
            return Err(StoreError::DatabaseNotInitialized)
        }
        other => other?,
    };
    match open_connection(&mut conn, mode) {
        Err(StoreError::Sqlite(error))
            if mode != StoreOpenMode::CreateOrMigrate && is_not_a_project(&error) =>
        {
            Err(StoreError::DatabaseNotInitialized)
        }
        Err(error) => Err(error),
        Ok(()) => Ok(conn),
    }
}

/// A missing file (`CANTOPEN`) or a file that is not SQLite (`NOTADB`).
fn is_not_a_project(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::CannotOpen | rusqlite::ErrorCode::NotADatabase)
    )
}

/// Configures `conn` and brings the database to store schema v5, or fails without altering it.
///
/// In `CreateOrMigrate` mode a fresh database (optionally holding only the F0.2 `artifacts`
/// table) is initialized at v5 in one transaction; in every mode a fresh database is otherwise
/// [`StoreError::DatabaseNotInitialized`]. A valid v1, v2, v3 or v4 store is migrated to v5 in one
/// transaction (never in `ReadOnly` mode: [`StoreError::DatabaseReadOnly`]); any failure rolls
/// the whole migration back; existing revision, artifact and workflow rows are never rewritten
/// (v2/v3 workflow proposals are copied unchanged into the rebuilt table, each as its own
/// occurrence; v4 rows are copied unchanged); a versioned v5 store is verified and left
/// unchanged; anything else is rejected.
///
/// The durability profile is frozen (Fable #199, `docs/operations.md`): WAL journal and
/// `synchronous = FULL`, so a committed transaction survives power loss. A `ReadOnly`
/// connection never changes the persistent journal mode.
pub(crate) fn open_connection(
    conn: &mut Connection,
    mode: StoreOpenMode,
) -> Result<(), StoreError> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    // Classify before the persistent journal-mode change so a rejected database is untouched.
    let state = classify(conn)?;
    if mode != StoreOpenMode::CreateOrMigrate && matches!(state, SchemaState::Fresh) {
        return Err(StoreError::DatabaseNotInitialized);
    }
    if mode == StoreOpenMode::ReadOnly {
        return match state {
            SchemaState::Current => Ok(()),
            _ => Err(StoreError::DatabaseReadOnly),
        };
    }
    conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
        row.get::<_, String>(0)
    })?;
    conn.execute_batch("PRAGMA synchronous = FULL;")?;
    if let SchemaState::Current = state {
        return Ok(());
    }
    // The table rebuilds of a migration run with foreign-key enforcement off (the SQLite
    // ALTER TABLE procedure; the pragma is a no-op inside a transaction), and every key is
    // checked before the commit.
    conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
    let migrated = migrate(conn, mode);
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrated
}

/// Initializes or migrates a database that is not at the current version, in one transaction.
fn migrate(conn: &mut Connection, mode: StoreOpenMode) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Another connection may have initialized or migrated the database while we waited.
    match classify(&tx)? {
        SchemaState::Fresh => {
            if mode != StoreOpenMode::CreateOrMigrate {
                return Err(StoreError::DatabaseNotInitialized);
            }
            ensure_artifact_schema(&tx)?;
            tx.execute_batch(CREATE_STORE_TABLES)?;
            tx.execute_batch(&create_workflow_proposals("workflow_proposals"))?;
            tx.execute_batch(CREATE_SESSION_TABLES)?;
            tx.execute_batch(CREATE_STAGE_TABLES)?;
            tx.execute_batch(&create_stage_job_proposals("stage_job_proposals"))?;
            tx.execute(
                "INSERT INTO schema_meta (version) VALUES (?1)",
                [STORE_SCHEMA_VERSION],
            )?;
        }
        SchemaState::V1 => {
            tx.execute_batch(&create_workflow_proposals("workflow_proposals"))?;
            tx.execute_batch(CREATE_SESSION_TABLES)?;
            tx.execute_batch(CREATE_STAGE_TABLES)?;
            tx.execute_batch(&create_stage_job_proposals("stage_job_proposals"))?;
            set_version(&tx, STORE_SCHEMA_VERSION_V1)?;
        }
        SchemaState::V2 => {
            migrate_workflow_proposals(&tx)?;
            tx.execute_batch(CREATE_STAGE_TABLES)?;
            tx.execute_batch(&create_stage_job_proposals("stage_job_proposals"))?;
            require_foreign_keys(&tx)?;
            set_version(&tx, STORE_SCHEMA_VERSION_V2)?;
        }
        SchemaState::V3 => {
            migrate_workflow_proposals(&tx)?;
            migrate_stage_job_proposals(&tx)?;
            require_foreign_keys(&tx)?;
            set_version(&tx, STORE_SCHEMA_VERSION_V3)?;
        }
        SchemaState::V4 => {
            migrate_v4_proposal_states(&tx)?;
            require_foreign_keys(&tx)?;
            set_version(&tx, STORE_SCHEMA_VERSION_V4)?;
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
