//! Durable stage-orchestration records of store schema v3 (A0.4, Hotfix 067): application
//! recipe jobs, their ordered steps, compiler stage jobs, stage proposal links and gate reports.
//!
//! Like the workflow store, this store keeps canonical opaque JSON (RFC 8785 text) together
//! with lifecycle metadata and owns identity, conditional transitions, canonical bytes, lookup
//! and durability; typed decoding and validation of the stored recipe job, compiler `StageJob`
//! and `GateReport` belong to the application layer. Reading re-canonicalizes every JSON column
//! and requires byte equality, so corruption is reported and never repaired.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use plumb_core::{Hash, Id, Timestamp};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde_json::Value;

use crate::branches::BranchName;
use crate::revisions::RevisionId;
use crate::schema::{attempt_error, durability_of, open_database, Durability, StoreOpenMode};
use crate::workflow::{
    canonical_text, corrupt, insert_proposal_on, parse_field, parse_optional, proposal_on,
    require_revision, NewWorkflowProposal, WorkflowInsert, WorkflowOrigin, WorkflowProposal,
    WorkflowProposalState,
};
use crate::StoreError;

/// The status vocabulary shared by recipe jobs and compiler stage jobs (the public `JobStatus`
/// and the X1.5 `StageJobStatus` wire strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StageStatus {
    Queued,
    Acquiring,
    Evaluating,
    Ready,
    Stale,
    Failed,
    Committed,
}

impl StageStatus {
    pub const ALL: [StageStatus; 7] = [
        StageStatus::Queued,
        StageStatus::Acquiring,
        StageStatus::Evaluating,
        StageStatus::Ready,
        StageStatus::Stale,
        StageStatus::Failed,
        StageStatus::Committed,
    ];

    /// The exact stored string.
    pub fn as_str(self) -> &'static str {
        match self {
            StageStatus::Queued => "queued",
            StageStatus::Acquiring => "acquiring",
            StageStatus::Evaluating => "evaluating",
            StageStatus::Ready => "ready",
            StageStatus::Stale => "stale",
            StageStatus::Failed => "failed",
            StageStatus::Committed => "committed",
        }
    }

    /// Queued, Acquiring and Evaluating are the in-flight states.
    pub fn is_in_flight(self) -> bool {
        matches!(
            self,
            StageStatus::Queued | StageStatus::Acquiring | StageStatus::Evaluating
        )
    }
}

impl FromStr for StageStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        StageStatus::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| s.to_owned())
    }
}

impl fmt::Display for StageStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The lease of an in-flight recipe job (H0.3, architecture §33.3): the owning process and the
/// instant after which another process may recover the job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageLease {
    /// `own:<first 16 hex of SHA-256(RFC 8785 {pid, process start time, db path})>`.
    pub owner_id: String,
    pub expires_at: Timestamp,
}

/// A recipe job to persist as `queued`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewStageRecipeJob {
    pub recipe_job_id: Hash,
    pub recipe_job_json: Value,
    pub config_json: Value,
    pub recipe: String,
    pub branch: BranchName,
    pub base_revision_id: RevisionId,
    pub created_at: Timestamp,
    /// The lease of the creating process, which will execute the queued job (`None`: ownerless).
    pub lease: Option<StageLease>,
}

/// One attempt of a recipe job (store schema v6, H0.3): attempts are numbered from 1 under the
/// job's unchanged ID; `outcome` and `ended_at` are set when the attempt ends, and `error` is the
/// canonical JSON `{code, message}` of a failed attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageAttemptRecord {
    pub recipe_job_id: Hash,
    pub attempt: u32,
    pub owner_id: Option<String>,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub outcome: Option<StageStatus>,
    pub error: Option<String>,
}

/// A persisted recipe job; the JSON fields are the exact stored canonical text.
#[derive(Debug, Clone, PartialEq)]
pub struct StageRecipeJobRecord {
    pub recipe_job_id: Hash,
    pub recipe_job_json: String,
    pub config_json: String,
    pub recipe: String,
    pub branch: BranchName,
    pub base_revision_id: RevisionId,
    pub status: StageStatus,
    pub result_revision_id: Option<RevisionId>,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    /// The current attempt (1 for a job never retried).
    pub attempt: u32,
    /// The process that created or last claimed the job.
    pub owner_id: Option<String>,
    /// The lease of an in-flight job; `None` once the job is terminal.
    pub lease_expires_at: Option<Timestamp>,
}

/// A persisted compiler stage job; `stage_job_json` is its exact stored canonical text.
#[derive(Debug, Clone, PartialEq)]
pub struct StageJobRecord {
    pub stage_job_id: Hash,
    pub stage_job_json: String,
    pub status: StageStatus,
    pub compile_run_ref: Option<Hash>,
    pub updated_at: Timestamp,
}

/// One ordered recipe step to persist.
#[derive(Debug, Clone, PartialEq)]
pub struct NewStageStep {
    pub recipe_job_id: Hash,
    pub ordinal: u32,
    pub phase: String,
    pub compiler_stage: String,
    pub input_revision_id: RevisionId,
}

/// A persisted recipe step.
#[derive(Debug, Clone, PartialEq)]
pub struct StageStepRecord {
    pub recipe_job_id: Hash,
    pub ordinal: u32,
    pub phase: String,
    pub compiler_stage: String,
    pub input_revision_id: RevisionId,
    pub stage_job_id: Option<Hash>,
    pub compile_run_ref: Option<Hash>,
    pub intermediate_artifact_ref: Option<Hash>,
    pub gate: Option<String>,
    pub result_revision_id: Option<RevisionId>,
}

/// The durable link of a stage-produced workflow proposal occurrence to its jobs and run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageProposalLink {
    pub proposal_id: Id,
    /// The base revision of the linked occurrence.
    pub base_revision_id: RevisionId,
    pub recipe_job_id: Hash,
    pub stage_job_id: Hash,
    pub compile_run_ref: Hash,
}

/// A gate report to upsert for one revision and gate.
#[derive(Debug, Clone, PartialEq)]
pub struct NewGateReport {
    pub revision_id: RevisionId,
    pub gate: String,
    pub report_hash: Hash,
    pub report_json: Value,
    pub supplemental_input_hash: Option<Hash>,
    pub evaluation_at: Option<Timestamp>,
    pub recipe_job_id: Option<Hash>,
    pub created_at: Timestamp,
}

/// A persisted gate report; `report_json` is its exact stored canonical text.
#[derive(Debug, Clone, PartialEq)]
pub struct GateReportRecord {
    pub revision_id: RevisionId,
    pub gate: String,
    pub report_hash: Hash,
    pub report_json: String,
    pub supplemental_input_hash: Option<Hash>,
    pub evaluation_at: Option<Timestamp>,
    pub recipe_job_id: Option<Hash>,
    pub created_at: Timestamp,
}

/// Durable stage-orchestration records over the pilot database (store schema v3).
///
/// Owns its own connection to the same database as the revision store. Every write runs in one
/// `BEGIN IMMEDIATE` transaction, so a failed operation leaves the database unchanged.
#[derive(Debug)]
pub struct SqliteStageStore {
    conn: Connection,
}

const RECIPE_COLUMNS: &str = "recipe_job_id, recipe_job_json, config_json, recipe, branch, \
     base_revision_id, status, result_revision_id, failure_code, failure_message, created_at, \
     updated_at, attempt, owner_id, lease_expires_at";

const ATTEMPT_COLUMNS: &str =
    "recipe_job_id, attempt, owner_id, started_at, ended_at, outcome, error";

const IN_FLIGHT: &str = "('queued', 'acquiring', 'evaluating')";

const STEP_COLUMNS: &str = "recipe_job_id, ordinal, phase, compiler_stage, input_revision_id, \
     stage_job_id, compile_run_ref, intermediate_artifact_ref, gate, result_revision_id";

const GATE_COLUMNS: &str = "revision_id, gate, report_hash, report_json, \
     supplemental_input_hash, evaluation_at, recipe_job_id, created_at";

impl SqliteStageStore {
    /// Opens the stage store at `path`, initializing or migrating the database to store schema
    /// v3 exactly as [`crate::SqliteRevisionStore::open`] does.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with(path, StoreOpenMode::CreateOrMigrate)
    }

    /// Opens the store of an existing PLUMB database at `path` read-write (architecture §33.9):
    /// the file and the schema are never created; an older store is migrated in place.
    pub fn open_existing(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with(path, StoreOpenMode::Existing)
    }

    /// Opens the store of an existing, current PLUMB database at `path` with SQLite read-only
    /// access and no journal-mode change; every write fails.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with(path, StoreOpenMode::ReadOnly)
    }

    /// Opens the store at `path` in `mode`.
    pub fn open_with(path: impl AsRef<Path>, mode: StoreOpenMode) -> Result<Self, StoreError> {
        let conn = open_database(path.as_ref(), mode)?;
        Ok(Self { conn })
    }

    /// The durability pragmas of this store's connection (Fable #199).
    pub fn durability(&self) -> Result<Durability, StoreError> {
        durability_of(&self.conn)
    }

    /// Sets the SQLite busy timeout of this store's connection.
    pub fn set_busy_timeout(&self, timeout: std::time::Duration) -> Result<(), StoreError> {
        Ok(self.conn.busy_timeout(timeout)?)
    }

    /// Takes and releases the write lock without writing: fails exactly when the database
    /// cannot be written (a read-only file is SQLite `READONLY`), before any stage work starts.
    pub fn require_writable(&mut self) -> Result<(), StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // A write statement that changes no row still needs a writable database.
        tx.execute("UPDATE schema_meta SET version = version WHERE 0", [])?;
        tx.commit()?;
        Ok(())
    }

    // ------------------------------------------------------------------ recipe jobs

    /// Inserts `job` as `queued`; an existing row with the same ID is returned unchanged.
    pub fn insert_recipe_job(
        &mut self,
        job: &NewStageRecipeJob,
    ) -> Result<WorkflowInsert<StageRecipeJobRecord>, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = recipe_on(&tx, &job.recipe_job_id)? {
            return Ok(WorkflowInsert::Existing(existing));
        }
        require_revision(&tx, &job.base_revision_id)?;
        tx.execute(
            &format!(
                "INSERT INTO stage_recipe_jobs ({RECIPE_COLUMNS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', NULL, NULL, NULL, ?7, ?7, 1, ?8, ?9)"
            ),
            params![
                job.recipe_job_id.as_str(),
                canonical_text(&job.recipe_job_json)?,
                canonical_text(&job.config_json)?,
                job.recipe,
                job.branch.as_str(),
                job.base_revision_id.as_str(),
                job.created_at.to_string(),
                job.lease.as_ref().map(|l| l.owner_id.as_str()),
                job.lease.as_ref().map(|l| l.expires_at.to_string()),
            ],
        )?;
        let inserted = recipe_on(&tx, &job.recipe_job_id)?
            .ok_or_else(|| not_found("stage_recipe_job", &job.recipe_job_id))?;
        tx.commit()?;
        Ok(WorkflowInsert::Inserted(inserted))
    }

    /// The recipe job with `id`, if stored.
    pub fn recipe_job(&self, id: &Hash) -> Result<Option<StageRecipeJobRecord>, StoreError> {
        recipe_on(&self.conn, id)
    }

    /// Every recipe job, ordered by ID.
    pub fn recipe_jobs(&self) -> Result<Vec<StageRecipeJobRecord>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {RECIPE_COLUMNS} FROM stage_recipe_jobs ORDER BY recipe_job_id"
        ))?;
        let raws = stmt
            .query_map([], raw_recipe)?
            .collect::<Result<Vec<_>, _>>()?;
        raws.into_iter().map(RawRecipe::decode).collect()
    }

    /// Starts a queued recipe job without an owner (see [`Self::claim_recipe_job_leased`]).
    pub fn claim_recipe_job(&mut self, id: &Hash, at: Timestamp) -> Result<bool, StoreError> {
        self.claim_with(id, None, at)
    }

    /// Starts a queued recipe job: `queued -> acquiring` only when it is still queued, leased to
    /// `lease`'s owner, with the record of its current attempt started at `at` (H0.3). Returns
    /// whether this caller claimed it, so a job is executed at most once.
    pub fn claim_recipe_job_leased(
        &mut self,
        id: &Hash,
        lease: &StageLease,
        at: Timestamp,
    ) -> Result<bool, StoreError> {
        self.claim_with(id, Some(lease), at)
    }

    fn claim_with(
        &mut self,
        id: &Hash,
        lease: Option<&StageLease>,
        at: Timestamp,
    ) -> Result<bool, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE stage_recipe_jobs SET status = 'acquiring', updated_at = ?1, owner_id = ?2, \
             lease_expires_at = ?3 WHERE recipe_job_id = ?4 AND status = 'queued'",
            params![
                at.to_string(),
                lease.map(|l| l.owner_id.as_str()),
                lease.map(|l| l.expires_at.to_string()),
                id.as_str()
            ],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        tx.execute(
            &format!(
                "INSERT INTO stage_recipe_job_attempts ({ATTEMPT_COLUMNS}) \
                 SELECT recipe_job_id, attempt, owner_id, ?1, NULL, NULL, NULL \
                 FROM stage_recipe_jobs WHERE recipe_job_id = ?2"
            ),
            params![at.to_string(), id.as_str()],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Extends the lease of every in-flight recipe job owned by `owner_id` to `expires_at` (the
    /// worker heartbeat); returns how many were renewed.
    pub fn renew_leases(
        &mut self,
        owner_id: &str,
        expires_at: Timestamp,
    ) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            &format!(
                "UPDATE stage_recipe_jobs SET lease_expires_at = ?1 \
                 WHERE owner_id = ?2 AND status IN {IN_FLIGHT}"
            ),
            params![expires_at.to_string(), owner_id],
        )?)
    }

    /// The hashes of the stored `validated-inference` artifacts whose JSON `request_hash` is
    /// `request_hash`, oldest first (Fable #62: a retried attempt reuses a completed request
    /// instead of calling the provider again). The artifacts themselves are read and verified
    /// through the artifact store.
    pub fn validated_inference_refs(&self, request_hash: &Hash) -> Result<Vec<Hash>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT hash FROM artifacts WHERE kind = ?1 AND json_valid(CAST(bytes AS TEXT)) \
             AND json_extract(CAST(bytes AS TEXT), '$.request_hash') = ?2 \
             ORDER BY created_at, hash",
        )?;
        let hashes = stmt
            .query_map(
                params![
                    plumb_artifacts::ArtifactKind::ValidatedInference.as_str(),
                    request_hash.as_str()
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(hashes.into_iter().filter_map(|h| h.parse().ok()).collect())
    }

    /// The IDs of every in-flight recipe job, in order; a row whose ID cannot be read is
    /// skipped.
    pub fn in_flight_recipe_job_ids(&self) -> Result<Vec<Hash>, StoreError> {
        in_flight_ids(&self.conn, "stage_recipe_jobs", "recipe_job_id")
    }

    /// The IDs of every in-flight compiler stage job, in order (rows are decoded one at a
    /// time by the caller, so one unreadable row never hides the others).
    pub fn in_flight_stage_job_ids(&self) -> Result<Vec<Hash>, StoreError> {
        in_flight_ids(&self.conn, "stage_jobs", "stage_job_id")
    }

    /// The in-flight recipe jobs another process may recover at `now`, in ID order: those
    /// without an owner or lease (pre-H0.3 rows) and those whose lease expired. A lease that
    /// cannot be read counts as expired; a row whose ID cannot be read is skipped.
    pub fn abandoned_recipe_jobs(&self, now: Timestamp) -> Result<Vec<Hash>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT recipe_job_id, owner_id, lease_expires_at FROM stage_recipe_jobs \
             WHERE status IN {IN_FLIGHT} ORDER BY recipe_job_id"
        ))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter(|(_, owner, lease)| {
                let live = owner.is_some()
                    && lease
                        .as_deref()
                        .and_then(|l| l.parse::<Timestamp>().ok())
                        .is_some_and(|expires| expires >= now);
                !live
            })
            .filter_map(|(id, _, _)| id.parse().ok())
            .collect())
    }

    /// Fails one in-flight recipe job with `code` and ends its current attempt, without decoding
    /// its JSON (recovery of an abandoned or unreadable row). Returns whether it was in flight.
    pub fn interrupt_recipe_job(
        &mut self,
        id: &Hash,
        code: &str,
        message: &str,
        at: Timestamp,
    ) -> Result<bool, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let interrupted = interrupt_on(&tx, id.as_str(), code, message, at)?;
        tx.commit()?;
        Ok(interrupted)
    }

    /// Every attempt record of a recipe job, in attempt order.
    pub fn recipe_job_attempts(&self, id: &Hash) -> Result<Vec<StageAttemptRecord>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ATTEMPT_COLUMNS} FROM stage_recipe_job_attempts WHERE recipe_job_id = ?1 \
             ORDER BY attempt"
        ))?;
        let raws = stmt
            .query_map([id.as_str()], raw_attempt)?
            .collect::<Result<Vec<_>, _>>()?;
        raws.into_iter().map(RawAttempt::decode).collect()
    }

    /// Moves a recipe job from one of `from` to `to` with its result revision and failure.
    /// Another current state is [`StoreError::WorkflowStateConflict`].
    pub fn transition_recipe_job(
        &mut self,
        id: &Hash,
        from: &[StageStatus],
        to: StageStatus,
        result_revision_id: Option<&RevisionId>,
        failure: Option<(&str, &str)>,
        at: Timestamp,
    ) -> Result<StageRecipeJobRecord, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = recipe_on(&tx, id)?.ok_or_else(|| not_found("stage_recipe_job", id))?;
        if !from.contains(&current.status) {
            return Err(StoreError::WorkflowStateConflict {
                kind: "stage_recipe_job",
                id: id.to_string(),
                expected: from
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("|"),
                actual: current.status.as_str().to_owned(),
            });
        }
        if let Some(revision) = result_revision_id {
            require_revision(&tx, revision)?;
        }
        // An in-flight job that settles ends its current attempt and gives up its lease.
        let settles = current.status.is_in_flight() && !to.is_in_flight();
        tx.execute(
            "UPDATE stage_recipe_jobs SET status = ?1, result_revision_id = ?2, \
             failure_code = ?3, failure_message = ?4, updated_at = ?5, \
             lease_expires_at = CASE WHEN ?7 THEN NULL ELSE lease_expires_at END \
             WHERE recipe_job_id = ?6",
            params![
                to.as_str(),
                result_revision_id.map(RevisionId::as_str),
                failure.map(|f| f.0),
                failure.map(|f| f.1),
                at.to_string(),
                id.as_str(),
                settles,
            ],
        )?;
        if settles {
            let error = failure.map(|(c, m)| attempt_error(c, m)).transpose()?;
            end_attempt(
                &tx,
                id.as_str(),
                current.attempt,
                current.owner_id.as_deref(),
                to,
                error.as_deref(),
                at,
            )?;
        }
        let updated = recipe_on(&tx, id)?.ok_or_else(|| not_found("stage_recipe_job", id))?;
        tx.commit()?;
        Ok(updated)
    }

    /// [`Self::retry_recipe_job_leased`] without an owner.
    pub fn retry_recipe_job(
        &mut self,
        id: &Hash,
        at: Timestamp,
    ) -> Result<StageRecipeJobRecord, StoreError> {
        self.retry_recipe_job_leased(id, None, at)
    }

    /// Re-queues a Failed recipe job as attempt N+1 under its unchanged ID (H0.3, §33.3):
    /// `failed -> queued` with its failure and result cleared, leased to `lease`'s owner, and the
    /// steps of the failed attempt removed, in one transaction; the record of attempt N is kept.
    /// Another current state is [`StoreError::WorkflowStateConflict`] and nothing changes.
    pub fn retry_recipe_job_leased(
        &mut self,
        id: &Hash,
        lease: Option<&StageLease>,
        at: Timestamp,
    ) -> Result<StageRecipeJobRecord, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = recipe_on(&tx, id)?.ok_or_else(|| not_found("stage_recipe_job", id))?;
        if current.status != StageStatus::Failed {
            return Err(StoreError::WorkflowStateConflict {
                kind: "stage_recipe_job",
                id: id.to_string(),
                expected: StageStatus::Failed.as_str().to_owned(),
                actual: current.status.as_str().to_owned(),
            });
        }
        tx.execute(
            "DELETE FROM stage_recipe_job_steps WHERE recipe_job_id = ?1",
            [id.as_str()],
        )?;
        tx.execute(
            "UPDATE stage_recipe_jobs SET status = 'queued', result_revision_id = NULL, \
             failure_code = NULL, failure_message = NULL, updated_at = ?1, \
             attempt = attempt + 1, owner_id = ?3, lease_expires_at = ?4 \
             WHERE recipe_job_id = ?2",
            params![
                at.to_string(),
                id.as_str(),
                lease.map(|l| l.owner_id.as_str()),
                lease.map(|l| l.expires_at.to_string())
            ],
        )?;
        let updated = recipe_on(&tx, id)?.ok_or_else(|| not_found("stage_recipe_job", id))?;
        tx.commit()?;
        Ok(updated)
    }

    /// The database file of this store (`None` for an in-memory database).
    pub fn database_path(&self) -> Option<std::path::PathBuf> {
        self.conn
            .path()
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from)
    }

    /// Fails every in-flight recipe job left by a previous process with `code` (A0.4 restart
    /// sweep); returns the failed IDs in order.
    pub fn interrupt_recipe_jobs(
        &mut self,
        code: &str,
        message: &str,
        at: Timestamp,
    ) -> Result<Vec<Hash>, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids: Vec<String> = {
            let mut stmt = tx.prepare(&format!(
                "SELECT recipe_job_id FROM stage_recipe_jobs \
                 WHERE status IN {IN_FLIGHT} ORDER BY recipe_job_id"
            ))?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for id in &ids {
            interrupt_on(&tx, id, code, message, at)?;
        }
        tx.commit()?;
        ids.iter()
            .map(|id| parse_field("stage_recipe_job", id, "recipe_job_id", id))
            .collect()
    }

    // ------------------------------------------------------------------ compiler stage jobs

    /// Inserts a compiler stage job; an existing row with the same ID is returned unchanged.
    pub fn insert_stage_job(
        &mut self,
        id: &Hash,
        stage_job_json: &Value,
        status: StageStatus,
        at: Timestamp,
    ) -> Result<WorkflowInsert<StageJobRecord>, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = stage_job_on(&tx, id)? {
            return Ok(WorkflowInsert::Existing(existing));
        }
        tx.execute(
            "INSERT INTO stage_jobs (stage_job_id, stage_job_json, status, compile_run_ref, \
             updated_at) VALUES (?1, ?2, ?3, NULL, ?4)",
            params![
                id.as_str(),
                canonical_text(stage_job_json)?,
                status.as_str(),
                at.to_string()
            ],
        )?;
        let inserted = stage_job_on(&tx, id)?.ok_or_else(|| not_found("stage_job", id))?;
        tx.commit()?;
        Ok(WorkflowInsert::Inserted(inserted))
    }

    /// Replaces the state of a stored compiler stage job whose current status is `from`.
    pub fn update_stage_job(
        &mut self,
        id: &Hash,
        from: StageStatus,
        stage_job_json: &Value,
        status: StageStatus,
        compile_run_ref: Option<&Hash>,
        at: Timestamp,
    ) -> Result<StageJobRecord, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = stage_job_on(&tx, id)?.ok_or_else(|| not_found("stage_job", id))?;
        if current.status != from {
            return Err(StoreError::WorkflowStateConflict {
                kind: "stage_job",
                id: id.to_string(),
                expected: from.as_str().to_owned(),
                actual: current.status.as_str().to_owned(),
            });
        }
        tx.execute(
            "UPDATE stage_jobs SET stage_job_json = ?1, status = ?2, compile_run_ref = ?3, \
             updated_at = ?4 WHERE stage_job_id = ?5",
            params![
                canonical_text(stage_job_json)?,
                status.as_str(),
                compile_run_ref.map(Hash::as_str),
                at.to_string(),
                id.as_str()
            ],
        )?;
        let updated = stage_job_on(&tx, id)?.ok_or_else(|| not_found("stage_job", id))?;
        tx.commit()?;
        Ok(updated)
    }

    /// The compiler stage job with `id`, if stored.
    pub fn stage_job(&self, id: &Hash) -> Result<Option<StageJobRecord>, StoreError> {
        stage_job_on(&self.conn, id)
    }

    /// Every compiler stage job in Queued, Acquiring or Evaluating, ordered by ID.
    pub fn in_flight_stage_jobs(&self) -> Result<Vec<StageJobRecord>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT stage_job_id, stage_job_json, status, compile_run_ref, updated_at \
             FROM stage_jobs WHERE status IN ('queued', 'acquiring', 'evaluating') \
             ORDER BY stage_job_id",
        )?;
        let raws = stmt
            .query_map([], raw_stage_job)?
            .collect::<Result<Vec<_>, _>>()?;
        raws.into_iter().map(RawStageJob::decode).collect()
    }

    // ------------------------------------------------------------------ steps

    /// Inserts the next ordered step of a recipe job.
    pub fn insert_step(&mut self, step: &NewStageStep) -> Result<StageStepRecord, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_revision(&tx, &step.input_revision_id)?;
        tx.execute(
            &format!(
                "INSERT INTO stage_recipe_job_steps ({STEP_COLUMNS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, NULL, NULL)"
            ),
            params![
                step.recipe_job_id.as_str(),
                step.ordinal,
                step.phase,
                step.compiler_stage,
                step.input_revision_id.as_str()
            ],
        )?;
        let inserted = step_on(&tx, &step.recipe_job_id, step.ordinal)?
            .ok_or_else(|| not_found("stage_recipe_job_step", &step.recipe_job_id))?;
        tx.commit()?;
        Ok(inserted)
    }

    /// Records the outputs of one step; `None` leaves a field unchanged.
    #[allow(clippy::too_many_arguments)]
    pub fn update_step(
        &mut self,
        recipe_job_id: &Hash,
        ordinal: u32,
        stage_job_id: Option<&Hash>,
        compile_run_ref: Option<&Hash>,
        intermediate_artifact_ref: Option<&Hash>,
        gate: Option<&str>,
        result_revision_id: Option<&RevisionId>,
    ) -> Result<StageStepRecord, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(revision) = result_revision_id {
            require_revision(&tx, revision)?;
        }
        let changed = tx.execute(
            "UPDATE stage_recipe_job_steps SET \
             stage_job_id = COALESCE(?1, stage_job_id), \
             compile_run_ref = COALESCE(?2, compile_run_ref), \
             intermediate_artifact_ref = COALESCE(?3, intermediate_artifact_ref), \
             gate = COALESCE(?4, gate), \
             result_revision_id = COALESCE(?5, result_revision_id) \
             WHERE recipe_job_id = ?6 AND ordinal = ?7",
            params![
                stage_job_id.map(Hash::as_str),
                compile_run_ref.map(Hash::as_str),
                intermediate_artifact_ref.map(Hash::as_str),
                gate,
                result_revision_id.map(RevisionId::as_str),
                recipe_job_id.as_str(),
                ordinal
            ],
        )?;
        if changed != 1 {
            return Err(not_found("stage_recipe_job_step", recipe_job_id));
        }
        let updated = step_on(&tx, recipe_job_id, ordinal)?
            .ok_or_else(|| not_found("stage_recipe_job_step", recipe_job_id))?;
        tx.commit()?;
        Ok(updated)
    }

    /// The steps of a recipe job in ordinal order.
    pub fn steps(&self, recipe_job_id: &Hash) -> Result<Vec<StageStepRecord>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {STEP_COLUMNS} FROM stage_recipe_job_steps WHERE recipe_job_id = ?1 \
             ORDER BY ordinal"
        ))?;
        let raws = stmt
            .query_map([recipe_job_id.as_str()], raw_step)?
            .collect::<Result<Vec<_>, _>>()?;
        raws.into_iter().map(RawStep::decode).collect()
    }

    // ------------------------------------------------------------------ proposal links

    /// Persists every stage proposal of one compiler run as `pending` together with its link,
    /// in one transaction. An occurrence (proposal ID and base revision) already linked keeps
    /// its first link and is returned as stored; an occurrence stored without a stage link is
    /// [`StoreError::StageProposalConflict`] and nothing is written. Occurrences of the same
    /// proposal on other baselines never conflict (H0.10).
    pub fn insert_stage_proposals(
        &mut self,
        recipe_job_id: &Hash,
        stage_job_id: &Hash,
        compile_run_ref: &Hash,
        proposals: &[NewWorkflowProposal],
    ) -> Result<Vec<WorkflowProposal>, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut stored = Vec::with_capacity(proposals.len());
        for proposal in proposals {
            let id = &proposal.proposal_id;
            if proposal.origin_kind != WorkflowOrigin::Stage {
                return Err(StoreError::WorkflowInvalidOrigin {
                    id: id.to_string(),
                    reason: "a stage proposal has origin_kind stage",
                });
            }
            let base = &proposal.base_revision_id;
            if let Some(existing) = proposal_on(&tx, id, base)? {
                if existing.origin_kind != WorkflowOrigin::Stage
                    || link_on(&tx, id, base)?.is_none()
                {
                    return Err(StoreError::StageProposalConflict { id: id.to_string() });
                }
                stored.push(existing);
                continue;
            }
            insert_proposal_on(&tx, proposal, WorkflowProposalState::Pending)?;
            tx.execute(
                "INSERT INTO stage_job_proposals (proposal_id, base_revision_id, recipe_job_id, \
                 stage_job_id, compile_run_ref) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id.as_str(),
                    base.as_str(),
                    recipe_job_id.as_str(),
                    stage_job_id.as_str(),
                    compile_run_ref.as_str()
                ],
            )?;
            stored.push(proposal_on(&tx, id, base)?.ok_or_else(|| {
                StoreError::WorkflowRecordNotFound {
                    kind: "proposal",
                    id: id.to_string(),
                }
            })?);
        }
        tx.commit()?;
        Ok(stored)
    }

    /// The stage link of a workflow proposal occurrence, if it is stage-produced.
    pub fn proposal_link(
        &self,
        proposal_id: &Id,
        base_revision_id: &RevisionId,
    ) -> Result<Option<StageProposalLink>, StoreError> {
        link_on(&self.conn, proposal_id, base_revision_id)
    }

    /// The links of every still-pending stage proposal occurrence, ordered by proposal ID and
    /// base revision.
    pub fn pending_proposal_links(&self) -> Result<Vec<StageProposalLink>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT l.proposal_id, l.base_revision_id, l.recipe_job_id, l.stage_job_id, \
             l.compile_run_ref FROM stage_job_proposals l JOIN workflow_proposals p \
             ON p.proposal_id = l.proposal_id AND p.base_revision_id = l.base_revision_id \
             WHERE p.state = 'pending' ORDER BY l.proposal_id, l.base_revision_id",
        )?;
        let rows = stmt
            .query_map([], raw_link)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(decode_link).collect()
    }

    // ------------------------------------------------------------------ gate reports

    /// Writes the report of one revision and gate, replacing an earlier evaluation of the same
    /// revision and gate.
    pub fn upsert_gate_report(
        &mut self,
        report: &NewGateReport,
    ) -> Result<GateReportRecord, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_revision(&tx, &report.revision_id)?;
        tx.execute(
            &format!(
                "INSERT INTO gate_reports ({GATE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 ON CONFLICT (revision_id, gate) DO UPDATE SET report_hash = excluded.report_hash, \
                 report_json = excluded.report_json, \
                 supplemental_input_hash = excluded.supplemental_input_hash, \
                 evaluation_at = excluded.evaluation_at, recipe_job_id = excluded.recipe_job_id, \
                 created_at = excluded.created_at"
            ),
            params![
                report.revision_id.as_str(),
                report.gate,
                report.report_hash.as_str(),
                canonical_text(&report.report_json)?,
                report.supplemental_input_hash.as_ref().map(Hash::as_str),
                report.evaluation_at.map(|t| t.to_string()),
                report.recipe_job_id.as_ref().map(Hash::as_str),
                report.created_at.to_string()
            ],
        )?;
        let stored = gate_report_on(&tx, &report.revision_id, &report.gate)?.ok_or_else(|| {
            StoreError::WorkflowRecordNotFound {
                kind: "gate_report",
                id: report.revision_id.to_string(),
            }
        })?;
        tx.commit()?;
        Ok(stored)
    }

    /// Every stored gate report, ordered by revision and gate.
    pub fn gate_reports(&self) -> Result<Vec<GateReportRecord>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {GATE_COLUMNS} FROM gate_reports ORDER BY revision_id, gate"
        ))?;
        let raws = stmt
            .query_map([], raw_gate)?
            .collect::<Result<Vec<_>, _>>()?;
        raws.into_iter().map(RawGate::decode).collect()
    }
}

/// The IDs in `column` of the in-flight rows of `table`, in order; unreadable IDs are skipped.
fn in_flight_ids(conn: &Connection, table: &str, column: &str) -> Result<Vec<Hash>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {column} FROM {table} WHERE status IN {IN_FLIGHT} ORDER BY {column}"
    ))?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids.into_iter().filter_map(|id| id.parse().ok()).collect())
}

/// Fails the in-flight recipe job `id` with `code` and ends its current attempt; false when it
/// is not in flight. Only plain columns are read, so an unreadable JSON column never blocks it.
fn interrupt_on(
    tx: &Connection,
    id: &str,
    code: &str,
    message: &str,
    at: Timestamp,
) -> Result<bool, StoreError> {
    let current: Option<(String, i64, Option<String>)> = tx
        .query_row(
            "SELECT status, attempt, owner_id FROM stage_recipe_jobs WHERE recipe_job_id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((status, attempt, owner)) = current else {
        return Ok(false);
    };
    if !status
        .parse::<StageStatus>()
        .is_ok_and(StageStatus::is_in_flight)
    {
        return Ok(false);
    }
    tx.execute(
        "UPDATE stage_recipe_jobs SET status = 'failed', result_revision_id = NULL, \
         failure_code = ?1, failure_message = ?2, updated_at = ?3, lease_expires_at = NULL \
         WHERE recipe_job_id = ?4",
        params![code, message, at.to_string(), id],
    )?;
    let attempt =
        u32::try_from(attempt).map_err(|_| corrupt("stage_recipe_job", id, "invalid attempt"))?;
    end_attempt(
        tx,
        id,
        attempt,
        owner.as_deref(),
        StageStatus::Failed,
        Some(&attempt_error(code, message)?),
        at,
    )?;
    Ok(true)
}

/// Ends the record of `attempt` with `outcome` at `at`; an attempt that never started (a queued
/// job settled directly) is recorded as started at `at`. An already ended record is kept.
fn end_attempt(
    tx: &Connection,
    id: &str,
    attempt: u32,
    owner: Option<&str>,
    outcome: StageStatus,
    error: Option<&str>,
    at: Timestamp,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT OR IGNORE INTO stage_recipe_job_attempts (recipe_job_id, attempt, owner_id, \
         started_at, ended_at, outcome, error) VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL)",
        params![id, attempt, owner, at.to_string()],
    )?;
    tx.execute(
        "UPDATE stage_recipe_job_attempts SET ended_at = ?1, outcome = ?2, error = ?3 \
         WHERE recipe_job_id = ?4 AND attempt = ?5 AND ended_at IS NULL",
        params![at.to_string(), outcome.as_str(), error, id, attempt],
    )?;
    Ok(())
}

// ============================================================================ decoding

struct RawAttempt {
    recipe_job_id: String,
    attempt: i64,
    owner_id: Option<String>,
    started_at: String,
    ended_at: Option<String>,
    outcome: Option<String>,
    error: Option<String>,
}

fn raw_attempt(row: &Row<'_>) -> rusqlite::Result<RawAttempt> {
    Ok(RawAttempt {
        recipe_job_id: row.get(0)?,
        attempt: row.get(1)?,
        owner_id: row.get(2)?,
        started_at: row.get(3)?,
        ended_at: row.get(4)?,
        outcome: row.get(5)?,
        error: row.get(6)?,
    })
}

impl RawAttempt {
    fn decode(self) -> Result<StageAttemptRecord, StoreError> {
        const K: &str = "stage_recipe_job_attempt";
        let id = self.recipe_job_id.clone();
        if let Some(error) = &self.error {
            require_canonical(K, &id, "error", error)?;
        }
        Ok(StageAttemptRecord {
            recipe_job_id: parse_field(K, &id, "recipe_job_id", &self.recipe_job_id)?,
            attempt: u32::try_from(self.attempt).map_err(|_| corrupt(K, &id, "invalid attempt"))?,
            owner_id: self.owner_id,
            started_at: parse_field(K, &id, "started_at", &self.started_at)?,
            ended_at: parse_optional(K, &id, "ended_at", self.ended_at)?,
            outcome: parse_optional(K, &id, "outcome", self.outcome)?,
            error: self.error,
        })
    }
}

fn not_found(kind: &'static str, id: &Hash) -> StoreError {
    StoreError::WorkflowRecordNotFound {
        kind,
        id: id.to_string(),
    }
}

/// Requires `text` to be exactly the RFC 8785 encoding of its JSON value.
fn require_canonical(
    kind: &'static str,
    id: &str,
    field: &str,
    text: &str,
) -> Result<(), StoreError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|_| corrupt(kind, id, format!("{field} is not JSON")))?;
    if canonical_text(&value)? != text {
        return Err(corrupt(kind, id, format!("{field} is not canonical JSON")));
    }
    Ok(())
}

struct RawRecipe {
    recipe_job_id: String,
    recipe_job_json: String,
    config_json: String,
    recipe: String,
    branch: String,
    base_revision_id: String,
    status: String,
    result_revision_id: Option<String>,
    failure_code: Option<String>,
    failure_message: Option<String>,
    created_at: String,
    updated_at: String,
    attempt: i64,
    owner_id: Option<String>,
    lease_expires_at: Option<String>,
}

fn raw_recipe(row: &Row<'_>) -> rusqlite::Result<RawRecipe> {
    Ok(RawRecipe {
        recipe_job_id: row.get(0)?,
        recipe_job_json: row.get(1)?,
        config_json: row.get(2)?,
        recipe: row.get(3)?,
        branch: row.get(4)?,
        base_revision_id: row.get(5)?,
        status: row.get(6)?,
        result_revision_id: row.get(7)?,
        failure_code: row.get(8)?,
        failure_message: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        attempt: row.get(12)?,
        owner_id: row.get(13)?,
        lease_expires_at: row.get(14)?,
    })
}

impl RawRecipe {
    fn decode(self) -> Result<StageRecipeJobRecord, StoreError> {
        const K: &str = "stage_recipe_job";
        let id = self.recipe_job_id.clone();
        require_canonical(K, &id, "recipe_job_json", &self.recipe_job_json)?;
        require_canonical(K, &id, "config_json", &self.config_json)?;
        Ok(StageRecipeJobRecord {
            recipe_job_id: parse_field(K, &id, "recipe_job_id", &self.recipe_job_id)?,
            recipe_job_json: self.recipe_job_json,
            config_json: self.config_json,
            recipe: self.recipe,
            branch: parse_field(K, &id, "branch", &self.branch)?,
            base_revision_id: parse_field(K, &id, "base_revision_id", &self.base_revision_id)?,
            status: parse_field(K, &id, "status", &self.status)?,
            result_revision_id: parse_optional(
                K,
                &id,
                "result_revision_id",
                self.result_revision_id,
            )?,
            failure_code: self.failure_code,
            failure_message: self.failure_message,
            created_at: parse_field(K, &id, "created_at", &self.created_at)?,
            updated_at: parse_field(K, &id, "updated_at", &self.updated_at)?,
            attempt: u32::try_from(self.attempt).map_err(|_| corrupt(K, &id, "invalid attempt"))?,
            owner_id: self.owner_id,
            lease_expires_at: parse_optional(K, &id, "lease_expires_at", self.lease_expires_at)?,
        })
    }
}

fn recipe_on(conn: &Connection, id: &Hash) -> Result<Option<StageRecipeJobRecord>, StoreError> {
    let raw = conn
        .query_row(
            &format!("SELECT {RECIPE_COLUMNS} FROM stage_recipe_jobs WHERE recipe_job_id = ?1"),
            [id.as_str()],
            raw_recipe,
        )
        .optional()?;
    raw.map(RawRecipe::decode).transpose()
}

struct RawStageJob {
    stage_job_id: String,
    stage_job_json: String,
    status: String,
    compile_run_ref: Option<String>,
    updated_at: String,
}

fn raw_stage_job(row: &Row<'_>) -> rusqlite::Result<RawStageJob> {
    Ok(RawStageJob {
        stage_job_id: row.get(0)?,
        stage_job_json: row.get(1)?,
        status: row.get(2)?,
        compile_run_ref: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

impl RawStageJob {
    fn decode(self) -> Result<StageJobRecord, StoreError> {
        const K: &str = "stage_job";
        let id = self.stage_job_id.clone();
        require_canonical(K, &id, "stage_job_json", &self.stage_job_json)?;
        Ok(StageJobRecord {
            stage_job_id: parse_field(K, &id, "stage_job_id", &self.stage_job_id)?,
            stage_job_json: self.stage_job_json,
            status: parse_field(K, &id, "status", &self.status)?,
            compile_run_ref: parse_optional(K, &id, "compile_run_ref", self.compile_run_ref)?,
            updated_at: parse_field(K, &id, "updated_at", &self.updated_at)?,
        })
    }
}

fn stage_job_on(conn: &Connection, id: &Hash) -> Result<Option<StageJobRecord>, StoreError> {
    let raw = conn
        .query_row(
            "SELECT stage_job_id, stage_job_json, status, compile_run_ref, updated_at \
             FROM stage_jobs WHERE stage_job_id = ?1",
            [id.as_str()],
            raw_stage_job,
        )
        .optional()?;
    raw.map(RawStageJob::decode).transpose()
}

struct RawStep {
    recipe_job_id: String,
    ordinal: i64,
    phase: String,
    compiler_stage: String,
    input_revision_id: String,
    stage_job_id: Option<String>,
    compile_run_ref: Option<String>,
    intermediate_artifact_ref: Option<String>,
    gate: Option<String>,
    result_revision_id: Option<String>,
}

fn raw_step(row: &Row<'_>) -> rusqlite::Result<RawStep> {
    Ok(RawStep {
        recipe_job_id: row.get(0)?,
        ordinal: row.get(1)?,
        phase: row.get(2)?,
        compiler_stage: row.get(3)?,
        input_revision_id: row.get(4)?,
        stage_job_id: row.get(5)?,
        compile_run_ref: row.get(6)?,
        intermediate_artifact_ref: row.get(7)?,
        gate: row.get(8)?,
        result_revision_id: row.get(9)?,
    })
}

impl RawStep {
    fn decode(self) -> Result<StageStepRecord, StoreError> {
        const K: &str = "stage_recipe_job_step";
        let id = self.recipe_job_id.clone();
        Ok(StageStepRecord {
            recipe_job_id: parse_field(K, &id, "recipe_job_id", &self.recipe_job_id)?,
            ordinal: u32::try_from(self.ordinal).map_err(|_| corrupt(K, &id, "invalid ordinal"))?,
            phase: self.phase,
            compiler_stage: self.compiler_stage,
            input_revision_id: parse_field(K, &id, "input_revision_id", &self.input_revision_id)?,
            stage_job_id: parse_optional(K, &id, "stage_job_id", self.stage_job_id)?,
            compile_run_ref: parse_optional(K, &id, "compile_run_ref", self.compile_run_ref)?,
            intermediate_artifact_ref: parse_optional(
                K,
                &id,
                "intermediate_artifact_ref",
                self.intermediate_artifact_ref,
            )?,
            gate: self.gate,
            result_revision_id: parse_optional(
                K,
                &id,
                "result_revision_id",
                self.result_revision_id,
            )?,
        })
    }
}

fn step_on(
    conn: &Connection,
    recipe_job_id: &Hash,
    ordinal: u32,
) -> Result<Option<StageStepRecord>, StoreError> {
    let raw = conn
        .query_row(
            &format!(
                "SELECT {STEP_COLUMNS} FROM stage_recipe_job_steps \
                 WHERE recipe_job_id = ?1 AND ordinal = ?2"
            ),
            params![recipe_job_id.as_str(), ordinal],
            raw_step,
        )
        .optional()?;
    raw.map(RawStep::decode).transpose()
}

type RawLink = (String, String, String, String, String);

fn raw_link(row: &Row<'_>) -> rusqlite::Result<RawLink> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn decode_link(raw: RawLink) -> Result<StageProposalLink, StoreError> {
    const K: &str = "stage_job_proposal";
    let (proposal_id, base_revision_id, recipe_job_id, stage_job_id, compile_run_ref) = raw;
    let id = proposal_id.clone();
    Ok(StageProposalLink {
        proposal_id: parse_field(K, &id, "proposal_id", &proposal_id)?,
        base_revision_id: parse_field(K, &id, "base_revision_id", &base_revision_id)?,
        recipe_job_id: parse_field(K, &id, "recipe_job_id", &recipe_job_id)?,
        stage_job_id: parse_field(K, &id, "stage_job_id", &stage_job_id)?,
        compile_run_ref: parse_field(K, &id, "compile_run_ref", &compile_run_ref)?,
    })
}

fn link_on(
    conn: &Connection,
    proposal_id: &Id,
    base_revision_id: &RevisionId,
) -> Result<Option<StageProposalLink>, StoreError> {
    let raw = conn
        .query_row(
            "SELECT proposal_id, base_revision_id, recipe_job_id, stage_job_id, compile_run_ref \
             FROM stage_job_proposals WHERE proposal_id = ?1 AND base_revision_id = ?2",
            [proposal_id.as_str(), base_revision_id.as_str()],
            raw_link,
        )
        .optional()?;
    raw.map(decode_link).transpose()
}

struct RawGate {
    revision_id: String,
    gate: String,
    report_hash: String,
    report_json: String,
    supplemental_input_hash: Option<String>,
    evaluation_at: Option<String>,
    recipe_job_id: Option<String>,
    created_at: String,
}

fn raw_gate(row: &Row<'_>) -> rusqlite::Result<RawGate> {
    Ok(RawGate {
        revision_id: row.get(0)?,
        gate: row.get(1)?,
        report_hash: row.get(2)?,
        report_json: row.get(3)?,
        supplemental_input_hash: row.get(4)?,
        evaluation_at: row.get(5)?,
        recipe_job_id: row.get(6)?,
        created_at: row.get(7)?,
    })
}

impl RawGate {
    fn decode(self) -> Result<GateReportRecord, StoreError> {
        const K: &str = "gate_report";
        let id = format!("{}/{}", self.revision_id, self.gate);
        require_canonical(K, &id, "report_json", &self.report_json)?;
        Ok(GateReportRecord {
            revision_id: parse_field(K, &id, "revision_id", &self.revision_id)?,
            gate: self.gate,
            report_hash: parse_field(K, &id, "report_hash", &self.report_hash)?,
            report_json: self.report_json,
            supplemental_input_hash: parse_optional(
                K,
                &id,
                "supplemental_input_hash",
                self.supplemental_input_hash,
            )?,
            evaluation_at: parse_optional(K, &id, "evaluation_at", self.evaluation_at)?,
            recipe_job_id: parse_optional(K, &id, "recipe_job_id", self.recipe_job_id)?,
            created_at: parse_field(K, &id, "created_at", &self.created_at)?,
        })
    }
}

fn gate_report_on(
    conn: &Connection,
    revision_id: &RevisionId,
    gate: &str,
) -> Result<Option<GateReportRecord>, StoreError> {
    let raw = conn
        .query_row(
            &format!(
                "SELECT {GATE_COLUMNS} FROM gate_reports WHERE revision_id = ?1 AND gate = ?2"
            ),
            params![revision_id.as_str(), gate],
            raw_gate,
        )
        .optional()?;
    raw.map(RawGate::decode).transpose()
}
