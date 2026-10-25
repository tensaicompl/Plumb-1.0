//! A0.4 stage orchestration (Hotfix 067): the application `StageRecipeJob` service shared by
//! HTTP and the CLI. A recipe job runs one public `StageName` recipe as ordered phases; each
//! phase owns at most one unchanged compiler child `StageJob`. Capture, insertion and reuse are
//! synchronous; execution runs on one FIFO worker thread per application instance (or in-process
//! for the CLI) outside every store lock while calling inference.

pub(crate) mod acceptance;
pub(crate) mod acquire;
pub(crate) mod gates;
pub(crate) mod phases;
pub mod recipe;
pub(crate) mod scenario_execution;

use std::sync::mpsc;
use std::thread;

use plumb_artifacts::{ArtifactKind, ArtifactStore};
use plumb_compiler::{
    capture_stage_job, persist_compile_run, ArtifactInput, ArtifactSet, CompileContext, CompileRun,
    Scope, StageJob, StageJobStatus,
};
use plumb_core::{to_canonical_json, CanonicalJson, GateId, Hash, HashKind, Id, Timestamp};
use plumb_store::{
    LoadedRevision, NewStageRecipeJob, NewStageStep, RevisionId, StageRecipeJobRecord, StageStatus,
    WorkflowInsert,
};
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::openapi::{JobResponse, JobStatus, StageRunResponse};
use crate::state::ApiState;

pub use gates::{ReadyGate, ReadyOutcome, ReadyRule};
pub use phases::SegmentationIntermediate;
pub use recipe::{Phase, Recipe, RecipeCapture};

use acceptance::{decode_child, persist_child, settle, status_of, StepEnd};
use phases::{rules_projection, PhaseOutput, PhaseStage};
use recipe::{
    canonical_hash, capture, child_config, effective_targets, parse_scope, provider_projection,
    recipe_config, validate_scope, ScopeKind,
};

/// The compiler version bound into child compile contexts.
const COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The bound of a stored failure message.
const MAX_FAILURE_MESSAGE: usize = 512;

/// The operation ID of stage-produced design events.
const STAGE_OPERATION: &str = "run_stage";

/// A captured or reused recipe job.
#[derive(Debug, Clone, PartialEq)]
pub struct StageRun {
    pub job_id: Hash,
    pub response: StageRunResponse,
}

fn status_wire(status: StageStatus) -> JobStatus {
    match status {
        StageStatus::Queued => JobStatus::Queued,
        StageStatus::Acquiring => JobStatus::Acquiring,
        StageStatus::Evaluating => JobStatus::Evaluating,
        StageStatus::Ready => JobStatus::Ready,
        StageStatus::Stale => JobStatus::Stale,
        StageStatus::Failed => JobStatus::Failed,
        StageStatus::Committed => JobStatus::Committed,
    }
}

/// Status-derived progress (Hotfix 067).
pub fn progress(status: StageStatus) -> f64 {
    match status {
        StageStatus::Queued => 0.0,
        StageStatus::Acquiring => 0.25,
        StageStatus::Evaluating => 0.75,
        StageStatus::Ready | StageStatus::Stale | StageStatus::Failed | StageStatus::Committed => {
            1.0
        }
    }
}

/// The public view of a stored recipe job.
pub fn job_response(record: &StageRecipeJobRecord) -> JobResponse {
    let result_version = match record.status {
        StageStatus::Ready | StageStatus::Committed => record
            .result_revision_id
            .as_ref()
            .map(RevisionId::to_string),
        _ => None,
    };
    let error = match (record.status, &record.failure_code, &record.failure_message) {
        (StageStatus::Failed, Some(code), Some(message)) => Some(format!("{code}: {message}")),
        _ => None,
    };
    JobResponse {
        status: status_wire(record.status),
        progress: progress(record.status),
        base_revision: record.base_revision_id.to_string(),
        result_version,
        error,
    }
}

/// The bounded failure message of an error (at most 512 bytes, on a character boundary).
fn bounded(message: &str) -> String {
    if message.len() <= MAX_FAILURE_MESSAGE {
        return message.to_owned();
    }
    let mut end = MAX_FAILURE_MESSAGE;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].to_owned()
}

/// 500 `E_INTERNAL` naming the failed stage-store operation by its stable error name.
pub(crate) fn store_error(error: plumb_store::StoreError) -> ApiError {
    if let Some(typed) = ApiError::store_class(error.class()) {
        return typed.with_details(json!({ "subsystem": "stage_store" }));
    }
    ApiError::internal(&format!(
        "stage_store: {}",
        scenario_execution::error_name(&error)
    ))
}

/// The stored failure message: the error message and, when present, its structured details
/// (stable codes and names only; never secrets or traces).
fn failure_message(error: &ApiError) -> String {
    let body = error.body();
    match &body.details {
        Value::Object(map) if !map.is_empty() => format!("{} {}", body.message, body.details),
        _ => body.message.clone(),
    }
}

/// Construction-time start of the A0.4 runtime: fails in-flight children through `mark_failed`
/// and in-flight parents with `E_JOB_INTERRUPTED`, reconciles interrupted stage acceptances and
/// starts the recipe worker. Nothing resumes.
pub(crate) fn start(state: &ApiState) -> Result<(), ApiError> {
    let path = state
        .with_stage_store(|store| store.database_path())?
        .map(|db| {
            let mut name = db.into_os_string();
            name.push(".jobs.lock");
            std::path::PathBuf::from(name)
        });
    if !sole_live_process(state, path.as_deref())? {
        // Another live process has this database open and may be executing a job right now:
        // nothing is interrupted (a later sole opener does that). Reconciling acceptances
        // that already committed is idempotent and always safe.
        acceptance::reconcile_pending(state)?;
        return start_worker(state);
    }
    let now = state.now();
    let children = state
        .with_stage_store(|store| store.in_flight_stage_jobs())?
        .map_err(|_| ApiError::workflow_corrupt("stage jobs"))?;
    for record in children {
        let mut job = decode_child(&record.stage_job_json, &record.stage_job_id)?;
        job.mark_failed()
            .map_err(|_| ApiError::workflow_corrupt("stage job"))?;
        persist_child(state, &job, record.status, now)?;
    }
    state
        .with_stage_store(|store| {
            store.interrupt_recipe_jobs(
                "E_JOB_INTERRUPTED",
                "the job was interrupted by a process restart",
                now,
            )
        })?
        .map_err(crate::stage::store_error)?;
    acceptance::reconcile_pending(state)?;
    start_worker(state)
}

/// Every process that opens a database holds a shared lock on `<db>.jobs.lock` for its
/// lifetime (released by the OS even on a crash). Only a process that can first take the lock
/// exclusively is the sole live opener, so in-flight jobs it finds were really interrupted.
/// Without a database file (in memory) the opener is always sole.
fn sole_live_process(
    state: &ApiState,
    lock_path: Option<&std::path::Path>,
) -> Result<bool, ApiError> {
    let Some(lock_path) = lock_path else {
        return Ok(true);
    };
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)
    else {
        return Ok(true);
    };
    let sole = file.try_lock().is_ok();
    if sole {
        let _ = file.unlock();
    }
    let _ = file.lock_shared();
    state.stage_runtime("stage_start")?.hold_jobs_lock(file);
    Ok(sole)
}

/// Starts the recipe worker thread of `state`.
fn start_worker(state: &ApiState) -> Result<(), ApiError> {
    let (sender, receiver) = mpsc::channel::<Hash>();
    let weak = state.downgrade();
    thread::Builder::new()
        .name("plumb-stage-worker".into())
        .spawn(move || {
            for id in receiver {
                let Some(state) = weak.upgrade() else { break };
                let _ = state.execute_recipe_job(&id);
            }
        })
        .map_err(|_| ApiError::internal("stage_worker"))?;
    state.stage_runtime("stage_worker")?.set_queue(sender)
}

/// Mutable progress of one recipe execution.
struct Run<'a> {
    state: &'a ApiState,
    job_id: Hash,
    capture: RecipeCapture,
    recipe: Recipe,
    ordinal: u32,
    current: LoadedRevision,
    committed: bool,
    status: StageStatus,
}

impl Run<'_> {
    /// Moves the parent between acquiring and evaluating.
    fn set_status(&mut self, to: StageStatus) -> Result<(), StepEnd> {
        if self.status == to {
            return Ok(());
        }
        self.state
            .with_stage_store(|store| {
                store.transition_recipe_job(
                    &self.job_id,
                    &[StageStatus::Acquiring, StageStatus::Evaluating],
                    to,
                    None,
                    None,
                    self.state.now(),
                )
            })?
            .map_err(crate::stage::store_error)?;
        self.status = to;
        Ok(())
    }

    /// Records the next ordered step.
    fn step(&mut self, phase: Phase, input: &RevisionId) -> Result<u32, StepEnd> {
        let ordinal = self.ordinal;
        self.state
            .with_stage_store(|store| {
                store.insert_step(&NewStageStep {
                    recipe_job_id: self.job_id.clone(),
                    ordinal,
                    phase: phase.as_str().to_owned(),
                    compiler_stage: phase.stage().as_str().to_owned(),
                    input_revision_id: input.clone(),
                })
            })?
            .map_err(crate::stage::store_error)?;
        self.ordinal += 1;
        Ok(ordinal)
    }

    /// Requires the branch head to still be `base`.
    fn require_head(&self, base: &LoadedRevision) -> Result<LoadedRevision, StepEnd> {
        let head = self.state.load_head()?;
        if head.revision.id != base.revision.id {
            return Err(StepEnd::Stale);
        }
        Ok(head)
    }

    /// A gate phase: evaluates and persists the report of `gate` over the current revision.
    fn gate_step(&mut self, phase: Phase, gate: GateId) -> Result<(), StepEnd> {
        self.set_status(StageStatus::Evaluating)?;
        let current = self.current.clone();
        let ordinal = self.step(phase, &current.revision.id)?;
        let a04 = self.state.stage_runtime("stage_gates")?;
        let evaluated =
            gates::evaluate_gate(self.state, &current, gate, &a04.validation_policy, None)?;
        gates::persist_report(
            self.state,
            &current,
            &evaluated,
            Some(&self.job_id),
            self.state.now(),
        )?;
        self.state
            .with_stage_store(|store| {
                store.update_step(
                    &self.job_id,
                    ordinal,
                    None,
                    None,
                    None,
                    Some(&gate.to_string()),
                    Some(&current.revision.id),
                )
            })?
            .map_err(crate::stage::store_error)?;
        Ok(())
    }

    /// The `rules` analysis phase: one Projection artifact, no child.
    fn rules_step(&mut self, tables: &[Id]) -> Result<(), StepEnd> {
        self.set_status(StageStatus::Evaluating)?;
        let current = self.current.clone();
        let ordinal = self.step(Phase::Rules, &current.revision.id)?;
        let bytes = rules_projection(&current.graph, tables)?;
        let hash = put_projection(self.state, &bytes)?;
        self.state
            .with_stage_store(|store| {
                store.update_step(&self.job_id, ordinal, None, None, Some(&hash), None, None)
            })?
            .map_err(crate::stage::store_error)?;
        Ok(())
    }

    /// A child phase over the current revision: capture, reuse or run, persist, settle.
    fn child_step(&mut self, stage: PhaseStage<'_>) -> Result<PhaseOutput, StepEnd> {
        let state = self.state;
        let phase = stage.phase;
        let base = self.current.clone();
        let head = self.require_head(&base)?;
        let a03 = state.runtime("stage_runtime")?;
        let a04 = state.stage_runtime("stage_runtime")?;
        let provider = provider_projection(self.recipe, a03, &a04.provider_mode)?;
        let config = child_config(
            self.recipe,
            phase,
            &a04.stage_actor_ref,
            base.revision.created_at,
            &provider,
            stage.inputs()?,
        );
        let ctx = CompileContext::from_loaded_revision(
            &base,
            COMPILER_VERSION.to_owned(),
            CanonicalJson::new(config),
        )
        .map_err(|e| ApiError::internal(&format!("compile_context: {e}")))?;
        let mut job = capture_stage_job(
            state.branch().clone(),
            &head.revision.id,
            &base,
            phase.stage(),
            stage.scope.clone(),
            &ctx,
        )
        .map_err(|e| ApiError::internal(&format!("stage_job: {e}")))?;
        let ordinal = self.step(phase, &base.revision.id)?;
        let now = state.now();
        let job_value = serde_json::to_value(&job).map_err(|_| ApiError::internal("stage_job"))?;
        let inserted = state
            .with_stage_store(|store| {
                store.insert_stage_job(job.id(), &job_value, StageStatus::Queued, now)
            })?
            .map_err(crate::stage::store_error)?;
        // The step names its child as soon as it exists (a failed child stays visible).
        state
            .with_stage_store(|store| {
                store.update_step(
                    &self.job_id,
                    ordinal,
                    Some(job.id()),
                    None,
                    None,
                    None,
                    None,
                )
            })?
            .map_err(crate::stage::store_error)?;
        let (output, run_ref, run) = match inserted {
            WorkflowInsert::Existing(record) => {
                job = decode_child(&record.stage_job_json, &record.stage_job_id)?;
                match job.status() {
                    StageJobStatus::Ready | StageJobStatus::Committed => {}
                    StageJobStatus::Stale => return Err(StepEnd::Stale),
                    _ => {
                        return Err(StepEnd::Failed(
                            ApiError::conflict(
                                "E_STAGE_CHILD_FAILED",
                                "a stored compiler child of this phase did not complete",
                            )
                            .with_details(
                                json!({ "stage_job_id": job.id(), "phase": phase.as_str() }),
                            ),
                        ))
                    }
                }
                let run_ref = record
                    .compile_run_ref
                    .clone()
                    .ok_or_else(|| ApiError::workflow_corrupt("stage job run"))?;
                let run = acceptance::load_run(state, &run_ref)?;
                let artifacts = artifacts_of(state, &run)?;
                let output = stage.evaluate_phase(&base.graph, &artifacts)?;
                if output.evaluation.output_hash().ok() != Some(run.output_hash.clone()) {
                    return Err(StepEnd::Failed(ApiError::workflow_corrupt("stage job run")));
                }
                (output, run_ref, run)
            }
            WorkflowInsert::Inserted(_) => match self.run_child(&stage, &ctx, &base, &mut job) {
                Ok(done) => done,
                Err(end) => {
                    if matches!(
                        job.status(),
                        StageJobStatus::Queued
                            | StageJobStatus::Acquiring
                            | StageJobStatus::Evaluating
                    ) {
                        let from = status_of(&job);
                        if job.mark_failed().is_ok() {
                            let _ = persist_child(state, &job, from, state.now());
                        }
                    }
                    return Err(end);
                }
            },
        };
        let intermediate = match &output.intermediate {
            Some(bytes) => Some(put_projection(state, bytes)?),
            None => None,
        };
        state
            .with_stage_store(|store| {
                store.update_step(
                    &self.job_id,
                    ordinal,
                    Some(job.id()),
                    Some(&run_ref),
                    intermediate.as_ref(),
                    None,
                    None,
                )
            })?
            .map_err(crate::stage::store_error)?;
        match job.status() {
            StageJobStatus::Stale => return Err(StepEnd::Stale),
            StageJobStatus::Committed => {
                if let Some(revision) = job.committed_revision().cloned() {
                    self.advance(ordinal, &revision)?;
                }
                return Ok(output);
            }
            _ => {}
        }
        self.set_status(StageStatus::Evaluating)?;
        // Traces are persisted before any commit, by their existing hash.
        for (hash, bytes) in &output.traces {
            let stored = state
                .with_artifact_store(|store| {
                    store.put(ArtifactKind::ScenarioTrace, "application/json", bytes, now)
                })?
                .map_err(|_| ApiError::internal("artifact_store"))?;
            if &stored != hash {
                return Err(StepEnd::Failed(ApiError::internal("artifact_store")));
            }
        }
        if let Some(revision) = settle(
            state,
            &self.job_id,
            &mut job,
            &run_ref,
            &run,
            &output.evaluation,
            &base,
            STAGE_OPERATION,
        )? {
            self.advance(ordinal, &revision)?;
        }
        Ok(output)
    }

    /// Moves the current revision after a commit and records it on the step.
    fn advance(&mut self, ordinal: u32, revision: &RevisionId) -> Result<(), StepEnd> {
        self.committed = true;
        self.current = self.state.load_revision(revision)?;
        self.state
            .with_stage_store(|store| {
                store.update_step(
                    &self.job_id,
                    ordinal,
                    None,
                    None,
                    None,
                    None,
                    Some(revision),
                )
            })?
            .map_err(crate::stage::store_error)?;
        Ok(())
    }

    /// Runs a newly captured child: acquire, evaluate, persist the run, complete.
    fn run_child(
        &mut self,
        stage: &PhaseStage<'_>,
        ctx: &CompileContext,
        base: &LoadedRevision,
        job: &mut StageJob,
    ) -> Result<(PhaseOutput, Hash, CompileRun), StepEnd> {
        let state = self.state;
        let invalid =
            |e: plumb_compiler::StageJobError| ApiError::internal(&format!("stage_job: {e}"));
        self.set_status(StageStatus::Acquiring)?;
        job.start_acquiring().map_err(invalid)?;
        persist_child(state, job, StageStatus::Queued, state.now())?;
        let plan = stage.plan_phase(&base.graph)?;
        let artifacts = acquire::acquire(state, &plan, self.recipe, stage.phase, state.now())?;
        artifacts
            .validate_for_plan(&plan)
            .map_err(|e| ApiError::internal(&format!("artifact_set: {e}")))?;
        job.start_evaluating().map_err(invalid)?;
        persist_child(state, job, StageStatus::Acquiring, state.now())?;
        self.set_status(StageStatus::Evaluating)?;
        let output = stage.evaluate_phase(&base.graph, &artifacts)?;
        let run = CompileRun::new(stage.phase.stage(), ctx, &artifacts, &output.evaluation)
            .map_err(|e| ApiError::internal(&format!("compile_run: {e}")))?;
        let persisted = state
            .with_artifact_store(|store| {
                persist_compile_run(store, &run, &output.evaluation, state.now())
            })?
            .map_err(|e| ApiError::internal(&format!("compile_run: {e}")))?;
        let head = state.load_head()?;
        job.complete(&plan, &run, &head.revision.id)
            .map_err(invalid)?;
        acceptance::persist_child_with_run(
            state,
            job,
            StageStatus::Evaluating,
            Some(&persisted.run_artifact_ref),
            state.now(),
        )?;
        Ok((output, persisted.run_artifact_ref, run))
    }
}

/// Stores a canonical Projection intermediate and returns its hash.
fn put_projection(state: &ApiState, bytes: &[u8]) -> Result<Hash, ApiError> {
    state
        .with_artifact_store(|store| {
            store.put(
                ArtifactKind::Projection,
                "application/json",
                bytes,
                state.now(),
            )
        })?
        .map_err(|_| ApiError::internal("artifact_store"))
}

/// Rebuilds the exact acquired ArtifactSet of a persisted run (reuse of a stored child),
/// including its declared-absent inference requests (R0.2).
fn artifacts_of(state: &ApiState, run: &CompileRun) -> Result<ArtifactSet, ApiError> {
    let load = |hashes: &[Hash]| -> Result<Vec<ArtifactInput>, ApiError> {
        hashes
            .iter()
            .map(|hash| {
                let artifact = state
                    .get_artifact(hash)?
                    .map_err(|_| ApiError::workflow_corrupt("stage run artifact"))?;
                Ok(ArtifactInput {
                    hash: artifact.hash,
                    kind: artifact.kind,
                    media_type: artifact.media_type,
                    bytes: artifact.bytes,
                })
            })
            .collect()
    };
    ArtifactSet::with_declared_absence(
        load(&run.deterministic_artifacts)?,
        load(&run.inference_artifacts)?,
        load(&run.validation_artifacts)?,
        run.declared_absent_inference_requests.clone(),
    )
    .map_err(|_| ApiError::workflow_corrupt("stage run artifacts"))
}

/// Loads a Projection intermediate by hash: kind, media type, canonical bytes, strict decode.
fn load_segmentation(state: &ApiState, hash: &Hash) -> Result<SegmentationIntermediate, ApiError> {
    let corrupt = || ApiError::workflow_corrupt(&format!("segmentation intermediate {hash}"));
    let artifact = state.get_artifact(hash)?.map_err(|_| corrupt())?;
    if artifact.kind != ArtifactKind::Projection
        || artifact.media_type != "application/json"
        || &Hash::content_sha256(&artifact.bytes) != hash
    {
        return Err(corrupt());
    }
    let value: SegmentationIntermediate =
        serde_json::from_slice(&artifact.bytes).map_err(|_| corrupt())?;
    if to_canonical_json(&value).map_err(|_| corrupt())? != artifact.bytes {
        return Err(corrupt());
    }
    Ok(value)
}

impl ApiState {
    /// Captures `recipe` over the branch head and inserts or reuses its recipe job (the
    /// synchronous part of `run_stage`; execution is separate). `if_match`, when given, must be
    /// the head's semantic hash (412 otherwise).
    pub fn run_stage(
        &self,
        recipe: Recipe,
        scope: Option<Vec<String>>,
        if_match: Option<&str>,
    ) -> Result<StageRun, ApiError> {
        let a03 = self.runtime("stage_runtime")?;
        let a04 = self.stage_runtime("stage_runtime")?;
        let loaded = self.load_head()?;
        if let Some(tag) = if_match {
            if tag != loaded.revision.semantic_hash.as_str() {
                return Err(crate::state::precondition_failed(&loaded.revision));
            }
        }
        let scope = parse_scope(scope.as_deref())?;
        validate_scope(recipe, &loaded.graph, scope.as_deref())?;
        let missing = recipe.missing_config(a04);
        if !missing.is_empty() {
            return Err(ApiError::conflict(
                "E_STAGE_CONFIG_MISSING",
                format!(
                    "the {} recipe needs configuration that is absent",
                    recipe.as_str()
                ),
            )
            .with_details(json!({ "recipe": recipe.as_str(), "missing": missing })));
        }
        let config = recipe_config(recipe, a03, a04)?;
        let capture = capture(self.branch(), &loaded, recipe, scope, &config)?;
        let job_id = capture.id()?;
        let new = NewStageRecipeJob {
            recipe_job_id: job_id.clone(),
            recipe_job_json: serde_json::to_value(&capture)
                .map_err(|_| ApiError::internal("stage_job"))?,
            config_json: config,
            recipe: recipe.as_str().to_owned(),
            branch: self.branch().clone(),
            base_revision_id: loaded.revision.id.clone(),
            created_at: self.now(),
        };
        let stored = self
            .with_stage_store(|store| store.insert_recipe_job(&new))?
            .map_err(crate::stage::store_error)?;
        let record = match stored {
            WorkflowInsert::Inserted(r) | WorkflowInsert::Existing(r) => r,
        };
        let stored_capture = decode_capture(&record)?;
        if stored_capture != capture {
            return Err(ApiError::workflow_corrupt(&format!(
                "stage recipe job {job_id}"
            )));
        }
        let revision = capture.initial_base_revision.to_string();
        Ok(StageRun {
            job_id: job_id.clone(),
            response: StageRunResponse {
                version: revision.clone(),
                revision,
                semantic_hash: capture.initial_base_semantic_hash.to_string(),
                events: Vec::new(),
                job_id: job_id.to_string(),
            },
        })
    }

    /// Hands a queued recipe job to this instance's worker.
    pub fn enqueue_recipe_job(&self, id: &Hash) -> Result<(), ApiError> {
        self.stage_runtime("stage_worker")?.enqueue(id)?;
        Ok(())
    }

    /// Executes a queued recipe job to completion in the calling thread (the worker, or the CLI).
    /// A job that is not queued is left unchanged (executed at most once).
    pub fn execute_recipe_job(&self, id: &Hash) -> Result<(), ApiError> {
        let now = self.now();
        let claimed = self
            .with_stage_store(|store| store.claim_recipe_job(id, now))?
            .map_err(crate::stage::store_error)?;
        if !claimed {
            return Ok(());
        }
        let record = self.recipe_record(id)?;
        let outcome = decode_capture(&record).and_then(|capture| {
            let recipe = capture.recipe()?;
            let current = self.load_revision(&capture.initial_base_revision)?;
            Ok(Run {
                state: self,
                job_id: id.clone(),
                capture,
                recipe,
                ordinal: 0,
                current,
                committed: false,
                status: StageStatus::Acquiring,
            })
        });
        let result = match outcome {
            Ok(mut run) => {
                execute(&mut run).map(|()| (run.committed, run.current.revision.id.clone()))
            }
            Err(error) => Err(StepEnd::Failed(error)),
        };
        let from = [StageStatus::Acquiring, StageStatus::Evaluating];
        let finished = self.with_stage_store(|store| match &result {
            Ok((committed, revision)) => store.transition_recipe_job(
                id,
                &from,
                if *committed {
                    StageStatus::Committed
                } else {
                    StageStatus::Ready
                },
                Some(revision),
                None,
                self.now(),
            ),
            Err(StepEnd::Stale) => {
                store.transition_recipe_job(id, &from, StageStatus::Stale, None, None, self.now())
            }
            Err(StepEnd::Failed(error)) => store.transition_recipe_job(
                id,
                &from,
                StageStatus::Failed,
                None,
                Some((error.body().code, &bounded(&failure_message(error)))),
                self.now(),
            ),
        })?;
        finished.map_err(crate::stage::store_error)?;
        Ok(())
    }

    fn recipe_record(&self, id: &Hash) -> Result<StageRecipeJobRecord, ApiError> {
        self.with_stage_store(|store| store.recipe_job(id))?
            .map_err(|_| ApiError::workflow_corrupt(&format!("stage recipe job {id}")))?
            .ok_or_else(|| ApiError::not_found(format!("job {id} not found")))
    }

    /// `get_job`: the recipe job `id` with the only legal observation, a Ready job whose result
    /// revision is no longer the head becoming stale (with its Ready children).
    pub fn get_job(&self, id: &str) -> Result<(JobResponse, LoadedRevision), ApiError> {
        let hash: Hash = id
            .parse()
            .ok()
            .filter(|h: &Hash| h.kind() == HashKind::Generic && h.as_str() == id)
            .ok_or_else(|| ApiError::invalid(format!("{id:?} is not a job ID")))?;
        let mut record = self.recipe_record(&hash)?;
        let loaded = self.load_head()?;
        if record.status == StageStatus::Ready {
            // A stage acceptance interrupted after its workflow resolution is completed here.
            let steps = self
                .with_stage_store(|store| store.steps(&hash))?
                .map_err(crate::stage::store_error)?;
            for child_id in steps.iter().filter_map(|s| s.stage_job_id.as_ref()) {
                let child = acceptance::load_child(self, child_id)?;
                if let Some(revision) = child.committed_revision() {
                    if record.result_revision_id.as_ref() != Some(revision) {
                        acceptance::finish_parent(self, &hash, revision)?;
                    }
                }
            }
            record = self.recipe_record(&hash)?;
        }
        if record.status == StageStatus::Ready
            && record.result_revision_id.as_ref() != Some(&loaded.revision.id)
        {
            let steps = self
                .with_stage_store(|store| store.steps(&hash))?
                .map_err(crate::stage::store_error)?;
            for child_id in steps.iter().filter_map(|s| s.stage_job_id.as_ref()) {
                let mut child = acceptance::load_child(self, child_id)?;
                if child.status() == StageJobStatus::Ready
                    && child.observe_head(&loaded.revision.id) == StageJobStatus::Stale
                {
                    persist_child(self, &child, StageStatus::Ready, self.now())?;
                }
            }
            acceptance::mark_parent_stale(self, &hash)?;
            record = self.recipe_record(&hash)?;
        }
        Ok((job_response(&record), loaded))
    }

    /// The gate results recorded by the steps of a recipe job: `(gate, "pass" | "fail")`.
    pub fn recipe_gate_results(&self, id: &Hash) -> Result<Vec<(String, String)>, ApiError> {
        Ok(self
            .recipe_gate_details(id)?
            .into_iter()
            .map(|(gate, result, _)| (gate, result))
            .collect())
    }

    /// What the phases of recipe job `id` could not turn into proposals, with their reasons,
    /// in step order (each entry carries its phase, analyser, kind and detail).
    pub fn recipe_dispositions(&self, id: &Hash) -> Result<Vec<Value>, ApiError> {
        let steps = self
            .with_stage_store(|store| store.steps(id))?
            .map_err(crate::stage::store_error)?;
        let mut out = Vec::new();
        for step in steps {
            let Some(hash) = &step.intermediate_artifact_ref else {
                continue;
            };
            let artifact = self
                .get_artifact(hash)?
                .map_err(|_| ApiError::workflow_corrupt(&format!("step intermediate {hash}")))?;
            let Ok(value) = serde_json::from_slice::<Value>(&artifact.bytes) else {
                continue;
            };
            for item in value
                .get("dispositions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let mut item = item.clone();
                item["phase"] = Value::String(step.phase.clone());
                out.push(item);
            }
        }
        Ok(out)
    }

    /// [`ApiState::recipe_gate_results`] with the non-passing rules of each report.
    pub fn recipe_gate_details(
        &self,
        id: &Hash,
    ) -> Result<Vec<(String, String, Vec<gates::ReadyRule>)>, ApiError> {
        let steps = self
            .with_stage_store(|store| store.steps(id))?
            .map_err(crate::stage::store_error)?;
        let reports = self
            .with_stage_store(|store| store.gate_reports())?
            .map_err(|_| ApiError::workflow_corrupt("gate reports"))?;
        let mut out = Vec::new();
        for step in steps {
            let (Some(gate), Some(revision)) = (&step.gate, &step.result_revision_id) else {
                continue;
            };
            if let Some(record) = reports
                .iter()
                .find(|r| &r.gate == gate && &r.revision_id == revision)
            {
                let report = gates::decode_report(record)?;
                out.push((
                    gate.clone(),
                    gates::result_wire(report.result),
                    gates::ReadyRule::failed(&report),
                ));
            }
        }
        Ok(out)
    }
}

/// Decodes the stored capture: strict serde, canonical bytes, matching identity.
fn decode_capture(record: &StageRecipeJobRecord) -> Result<RecipeCapture, ApiError> {
    let id = &record.recipe_job_id;
    let corrupt = || ApiError::workflow_corrupt(&format!("stage recipe job {id}"));
    let capture: RecipeCapture =
        serde_json::from_str(&record.recipe_job_json).map_err(|_| corrupt())?;
    if to_canonical_json(&capture).map_err(|_| corrupt())? != record.recipe_job_json.as_bytes()
        || &capture.id()? != id
        || canonical_hash(
            &serde_json::from_str::<Value>(&record.config_json).map_err(|_| corrupt())?,
        )? != capture.config_hash
    {
        return Err(corrupt());
    }
    Ok(capture)
}

/// The compiler scope of a phase.
fn compiler_scope(scope: Option<&[Id]>) -> Result<Scope, ApiError> {
    match scope {
        None => Ok(Scope::Project),
        Some(ids) => Scope::elements(ids.to_vec()).map_err(|_| ApiError::internal("stage_scope")),
    }
}

/// Executes the recipe's frozen phases (compiler architecture recipe matrix).
fn execute(run: &mut Run<'_>) -> Result<(), StepEnd> {
    let state = run.state;
    let a03 = state.runtime("stage_runtime")?;
    let a04 = state.stage_runtime("stage_runtime")?;
    let recipe = run.recipe;
    let base = run.current.clone();
    // A queued recipe can only run against its captured base.
    run.require_head(&base)?;
    let scope = run.capture.scope.clone();
    let targets = effective_targets(recipe, &base.graph, scope.as_deref());
    if recipe.scope_kind() != ScopeKind::Whole && targets.is_empty() {
        return Ok(());
    }
    let audit_actor = a04.stage_actor_ref.clone();
    let stage = |phase: Phase, scope: Scope, targets: Vec<Id>, at: Timestamp| PhaseStage {
        phase,
        scope,
        targets,
        audit_actor: audit_actor.clone(),
        audit_at: at,
        a03,
        a04,
        segmentation: None,
    };
    let at = base.revision.created_at;
    let scope_of = compiler_scope(scope.as_deref())?;
    match recipe {
        Recipe::Intake => run.gate_step(Phase::I0, GateId::I0)?,
        Recipe::Lint => run.gate_step(Phase::F1, GateId::F1)?,
        Recipe::Rules => run.rules_step(&targets)?,
        Recipe::Extract => {
            let output = run.child_step(stage(
                Phase::Segmentation,
                scope_of.clone(),
                targets.clone(),
                at,
            ))?;
            let bytes = output
                .intermediate
                .ok_or_else(|| ApiError::internal("segmentation_intermediate"))?;
            let hash = Hash::content_sha256(&bytes);
            let segmentation = load_segmentation(state, &hash)?;
            if !segmentation.has_requirement_candidate() {
                return Ok(());
            }
            run.require_head(&base)?;
            let mut classification = stage(Phase::Classification, scope_of, targets, at);
            classification.segmentation = Some((hash, segmentation));
            run.child_step(classification)?;
        }
        Recipe::Consolidate => {
            run.child_step(stage(Phase::Consolidate, scope_of, targets, at))?;
        }
        Recipe::Calc => {
            run.child_step(stage(Phase::Calc, scope_of, targets, at))?;
        }
        Recipe::Analyse => {
            run.child_step(stage(Phase::Analyse, scope_of, targets, at))?;
            // Fable #190 (R0.4): F2 is always evaluated; its inputs are assembled from the
            // graph, so an absent external input fails it explicitly instead of skipping it.
            run.gate_step(Phase::F2, GateId::F2)?;
        }
        Recipe::Flows => {
            run.child_step(stage(Phase::Flows, scope_of, targets, at))?;
        }
        Recipe::Scenarios => {
            run.child_step(stage(Phase::Scenarios, scope_of, targets, at))?;
        }
        Recipe::Tags => {
            run.child_step(stage(Phase::Tags, scope_of, targets, at))?;
        }
        Recipe::Run => {
            run.child_step(stage(Phase::Execute, scope_of, targets, at))?;
            let after = run.current.revision.created_at;
            run.child_step(stage(
                Phase::Verification,
                Scope::Project,
                Vec::new(),
                after,
            ))?;
            run.gate_step(Phase::F4, GateId::F4)?;
        }
    }
    Ok(())
}

/// In-crate support for the R0.2 acquisition and reconstruction unit tests: an `ApiState` over
/// a fresh temporary project database with an explicit provider mode and a recording provider.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Arc, Mutex};

    use plumb_core::{CanonicalJson, FixedClock, Hash, Id, StageId, Timestamp};
    use plumb_functional::{ExprCalendarDefinition, ExprStaticCalendarProvider};
    use plumb_inference::{
        InferenceArtifact, InferenceProvider, InferenceRequest, InferenceRequestMaterial,
        ProviderError, ProviderExecution, ProviderPolicy,
    };
    use plumb_session::SessionTurnConfig;
    use plumb_store::BranchName;
    use plumb_validation::{load_builtin_software_profile, ValidationPolicy};
    use serde_json::json;

    use crate::application::{initialize_project_database, open_application, ApplicationRuntime};
    use crate::runtime::{DirectManagerPredicates, GateInputsConfig, ProviderMode};
    use crate::state::{ApiIdentity, ApiState};

    pub(crate) const PROMPT: &[u8] = b"prompt";
    pub(crate) const SCHEMA: &[u8] = b"{}";

    pub(crate) fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    pub(crate) fn at() -> Timestamp {
        "2026-01-01T00:00:00.000000000Z".parse().unwrap()
    }

    /// A provider failure to inject.
    pub(crate) type Failure = fn(&InferenceRequest) -> ProviderError;

    /// Answers every request with `{}`, records each call, or fails with `error`.
    #[derive(Default)]
    pub(crate) struct Recording {
        pub(crate) calls: Mutex<Vec<String>>,
        pub(crate) error: Mutex<Option<Failure>>,
    }

    impl InferenceProvider for Recording {
        fn execute(
            &self,
            request: &InferenceRequest,
            material: &InferenceRequestMaterial,
        ) -> Result<ProviderExecution, ProviderError> {
            plumb_inference::validate_binding(request, material)?;
            self.calls.lock().unwrap().push(request.task_kind.clone());
            if let Some(error) = *self.error.lock().unwrap() {
                return Err(error(request));
            }
            let raw_response = b"{}".to_vec();
            let validated_output = CanonicalJson::new(json!({}));
            Ok(ProviderExecution {
                artifact: InferenceArtifact {
                    request_hash: request.id.clone(),
                    provider: request.provider_policy.provider.clone(),
                    model: "test-model".to_owned(),
                    parameters: CanonicalJson::new(json!({})),
                    raw_response_hash: Hash::content_sha256(&raw_response),
                    validated_output_hash: validated_output.content_hash().unwrap(),
                    validated_output,
                },
                raw_response_media_type: "application/json".to_owned(),
                raw_response,
            })
        }
    }

    /// A planned request of `task_kind`, distinguished by `salt`.
    pub(crate) fn request(
        task_kind: &str,
        salt: &str,
    ) -> (InferenceRequest, InferenceRequestMaterial) {
        let context = plumb_core::to_canonical_json(&json!({ "salt": salt })).unwrap();
        let request = InferenceRequest::new(
            StageId::S1,
            task_kind.to_owned(),
            vec![id("req:a")],
            vec![],
            Hash::content_sha256(&context),
            Hash::content_sha256(PROMPT),
            Hash::content_sha256(SCHEMA),
            ProviderPolicy {
                provider: "test".to_owned(),
                config: CanonicalJson::new(json!({})),
            },
        )
        .unwrap();
        let material =
            InferenceRequestMaterial::new(&request, context, PROMPT.to_vec(), SCHEMA.to_vec())
                .unwrap();
        (request, material)
    }

    /// A state over a new project database with `mode` and `provider`.
    pub(crate) fn state(
        dir: &tempfile::TempDir,
        mode: ProviderMode,
        provider: Arc<dyn InferenceProvider + Send + Sync>,
    ) -> ApiState {
        let db = dir.path().join("plumb.sqlite");
        let profile = load_builtin_software_profile().unwrap();
        initialize_project_database(&db, id("project:pilot"), &profile, id("agent:api"), at())
            .unwrap();
        let runtime = ApplicationRuntime {
            inference_provider: provider,
            provider_mode: mode,
            provider_policy: ProviderPolicy {
                provider: "test".into(),
                config: CanonicalJson::new(json!({})),
            },
            session_turn_config: SessionTurnConfig {
                retrieval_top_k: 3,
                max_open_questions: 3,
                max_context_bytes: 65536,
            },
            assistant_actor_ref: id("agent:assistant"),
            stage_actor_ref: id("agent:compiler"),
            calendars: Arc::new(
                ExprStaticCalendarProvider::new([ExprCalendarDefinition::new(
                    id("cal:office"),
                    [],
                    [],
                )])
                .unwrap(),
            ),
            predicates: Arc::new(DirectManagerPredicates::new(&[]).unwrap()),
            calendar_configs: Vec::new(),
            direct_managers: Vec::new(),
            expression_scopes: Vec::new(),
            calculation_scope_overrides: Vec::new(),
            runtime_semantic_values: Vec::new(),
            validation_policy: ValidationPolicy::default(),
            vocabulary_policy: serde_json::from_value(json!({"language": "en"})).unwrap(),
            duplicate_policy: serde_json::from_value(json!({"jaccard_threshold": "0.8"})).unwrap(),
            calculation_scope: None,
            operation_scope: None,
            process_scope: None,
            authorization_draft: None,
            scenario_derivation: None,
            verification_applicability: None,
            gates: GateInputsConfig { f1: None, f4: None },
            directory: None,
            question_routing: None,
        };
        open_application(
            &db,
            BranchName::main(),
            ApiIdentity::new(
                "alice".into(),
                id("agent:api"),
                vec!["analyst".into()],
                "pilot".into(),
            )
            .unwrap(),
            Arc::new(FixedClock::new(at())),
            profile,
            runtime,
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use plumb_compiler::CompileRun;
    use plumb_core::{Hash, StageId};

    use super::test_support::{id, state, Recording};
    use super::*;
    use crate::runtime::ProviderMode;

    /// A persisted run with the given declared absence (no artifacts to load).
    fn run(absent: Vec<Hash>) -> CompileRun {
        let mut run = CompileRun {
            id: Hash::content_sha256(b""),
            stage: StageId::S1,
            input_revision: "rev:1:0123456789abcdef".parse().unwrap(),
            input_semantic_hash: Hash::semantic_sha256(b"graph"),
            profile_ref: id("profile:plumb-software-2026.1"),
            profile_hash: Hash::content_sha256(b"profile"),
            rule_pack_hash: Hash::content_sha256(b"rules"),
            compiler_version: "0.1.0".into(),
            config_hash: Hash::content_sha256(b"config"),
            deterministic_artifacts: vec![],
            inference_artifacts: vec![],
            validation_artifacts: vec![],
            output_derivation_patch_ref: None,
            output_proposal_refs: vec![],
            output_hash: Hash::content_sha256(b"output"),
            declared_absent_inference_requests: absent,
        };
        run.id = run.recompute_id().unwrap();
        run.validate().unwrap();
        run
    }

    #[test]
    fn artifacts_of_reconstructs_declared_absence() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir, ProviderMode::Null, Arc::new(Recording::default()));
        let mut absent = vec![Hash::content_sha256(b"b"), Hash::content_sha256(b"a")];
        absent.sort();
        let rebuilt = artifacts_of(&state, &run(absent.clone())).unwrap();
        assert_eq!(rebuilt.declared_absent_inference_requests, absent);
        assert!(rebuilt.inference_artifacts.is_empty());
        // A legacy run without absence reconstructs the ordinary empty-absence set.
        let legacy = artifacts_of(&state, &run(vec![])).unwrap();
        assert!(legacy.declared_absent_inference_requests.is_empty());
        assert_eq!(legacy, ArtifactSet::new(vec![], vec![], vec![]).unwrap());
    }
}
