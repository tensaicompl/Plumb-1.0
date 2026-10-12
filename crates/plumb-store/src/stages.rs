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
use crate::schema::open_connection;
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

/// The durable link of a stage-produced workflow proposal to its jobs and run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageProposalLink {
    pub proposal_id: Id,
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
     updated_at";

const STEP_COLUMNS: &str = "recipe_job_id, ordinal, phase, compiler_stage, input_revision_id, \
     stage_job_id, compile_run_ref, intermediate_artifact_ref, gate, result_revision_id";

const GATE_COLUMNS: &str = "revision_id, gate, report_hash, report_json, \
     supplemental_input_hash, evaluation_at, recipe_job_id, created_at";

impl SqliteStageStore {
    /// Opens the stage store at `path`, initializing or migrating the database to store schema
    /// v3 exactly as [`crate::SqliteRevisionStore::open`] does.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let mut conn = Connection::open(path)?;
        open_connection(&mut conn)?;
        Ok(Self { conn })
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
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', NULL, NULL, NULL, ?7, ?7)"
            ),
            params![
                job.recipe_job_id.as_str(),
                canonical_text(&job.recipe_job_json)?,
                canonical_text(&job.config_json)?,
                job.recipe,
                job.branch.as_str(),
                job.base_revision_id.as_str(),
                job.created_at.to_string(),
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

    /// Starts a queued recipe job: `queued -> acquiring` only when it is still queued. Returns
    /// whether this caller claimed it, so a job is executed at most once.
    pub fn claim_recipe_job(&mut self, id: &Hash, at: Timestamp) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "UPDATE stage_recipe_jobs SET status = 'acquiring', updated_at = ?1 \
             WHERE recipe_job_id = ?2 AND status = 'queued'",
            params![at.to_string(), id.as_str()],
        )?;
        Ok(changed == 1)
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
        tx.execute(
            "UPDATE stage_recipe_jobs SET status = ?1, result_revision_id = ?2, \
             failure_code = ?3, failure_message = ?4, updated_at = ?5 WHERE recipe_job_id = ?6",
            params![
                to.as_str(),
                result_revision_id.map(RevisionId::as_str),
                failure.map(|f| f.0),
                failure.map(|f| f.1),
                at.to_string(),
                id.as_str(),
            ],
        )?;
        let updated = recipe_on(&tx, id)?.ok_or_else(|| not_found("stage_recipe_job", id))?;
        tx.commit()?;
        Ok(updated)
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
            let mut stmt = tx.prepare(
                "SELECT recipe_job_id FROM stage_recipe_jobs \
                 WHERE status IN ('queued', 'acquiring', 'evaluating') ORDER BY recipe_job_id",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        tx.execute(
            "UPDATE stage_recipe_jobs SET status = 'failed', result_revision_id = NULL, \
             failure_code = ?1, failure_message = ?2, updated_at = ?3 \
             WHERE status IN ('queued', 'acquiring', 'evaluating')",
            params![code, message, at.to_string()],
        )?;
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
    /// in one transaction. A proposal already linked keeps its first link and is returned as
    /// stored; a proposal ID stored without a stage link is
    /// [`StoreError::StageProposalConflict`] and nothing is written.
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
            if let Some(existing) = proposal_on(&tx, id)? {
                if existing.origin_kind != WorkflowOrigin::Stage || link_on(&tx, id)?.is_none() {
                    return Err(StoreError::StageProposalConflict { id: id.to_string() });
                }
                stored.push(existing);
                continue;
            }
            insert_proposal_on(&tx, proposal, WorkflowProposalState::Pending)?;
            tx.execute(
                "INSERT INTO stage_job_proposals (proposal_id, recipe_job_id, stage_job_id, \
                 compile_run_ref) VALUES (?1, ?2, ?3, ?4)",
                params![
                    id.as_str(),
                    recipe_job_id.as_str(),
                    stage_job_id.as_str(),
                    compile_run_ref.as_str()
                ],
            )?;
            stored.push(proposal_on(&tx, id)?.ok_or_else(|| {
                StoreError::WorkflowRecordNotFound {
                    kind: "proposal",
                    id: id.to_string(),
                }
            })?);
        }
        tx.commit()?;
        Ok(stored)
    }

    /// The stage link of a workflow proposal, if it is stage-produced.
    pub fn proposal_link(&self, proposal_id: &Id) -> Result<Option<StageProposalLink>, StoreError> {
        link_on(&self.conn, proposal_id)
    }

    /// The links of every still-pending stage proposal, ordered by proposal ID.
    pub fn pending_proposal_links(&self) -> Result<Vec<StageProposalLink>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT l.proposal_id, l.recipe_job_id, l.stage_job_id, l.compile_run_ref \
             FROM stage_job_proposals l JOIN workflow_proposals p \
             ON p.proposal_id = l.proposal_id WHERE p.state = 'pending' ORDER BY l.proposal_id",
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

// ============================================================================ decoding

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

fn raw_link(row: &Row<'_>) -> rusqlite::Result<(String, String, String, String)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn decode_link(raw: (String, String, String, String)) -> Result<StageProposalLink, StoreError> {
    const K: &str = "stage_job_proposal";
    let (proposal_id, recipe_job_id, stage_job_id, compile_run_ref) = raw;
    let id = proposal_id.clone();
    Ok(StageProposalLink {
        proposal_id: parse_field(K, &id, "proposal_id", &proposal_id)?,
        recipe_job_id: parse_field(K, &id, "recipe_job_id", &recipe_job_id)?,
        stage_job_id: parse_field(K, &id, "stage_job_id", &stage_job_id)?,
        compile_run_ref: parse_field(K, &id, "compile_run_ref", &compile_run_ref)?,
    })
}

fn link_on(conn: &Connection, proposal_id: &Id) -> Result<Option<StageProposalLink>, StoreError> {
    let raw = conn
        .query_row(
            "SELECT proposal_id, recipe_job_id, stage_job_id, compile_run_ref \
             FROM stage_job_proposals WHERE proposal_id = ?1",
            [proposal_id.as_str()],
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
