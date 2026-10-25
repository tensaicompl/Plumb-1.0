//! Process-owned stage execution (H0.3, architecture §33.3): the owner identity and leases of
//! this process, the recipe worker and its lease heartbeat, startup recovery of abandoned jobs,
//! persistence of every worker error, and graceful shutdown.
//!
//! - **Ownership.** Every process that executes stage work has an `owner_id` =
//!   `own:<first 16 hex of SHA-256(RFC 8785 {db_path, pid, process_start_time})>`. Creating,
//!   retrying or claiming a job leases it to that owner until now + [`STAGE_LEASE_SECONDS`]; the
//!   heartbeat renews the leases of the owner's in-flight jobs every
//!   [`STAGE_HEARTBEAT_SECONDS`].
//! - **Recovery.** Startup recovery interrupts (`E_JOB_INTERRUPTED`) only in-flight jobs whose
//!   lease expired or that have no owner. When the shared `<db>.jobs.lock` proves that no other
//!   process has the database open, every in-flight job is abandoned and recovered at once.
//!   Recovery is per-row fault tolerant (Fable #183) and settles a job whose proposals were
//!   already persisted Ready over them (Fable #188).
//! - **Worker errors.** An `Err` or a panic while executing a claimed job persists the job
//!   Failed with `E_INTERNAL` and its bounded message, is logged, and the worker keeps serving
//!   (Fable #179).
//! - **Shutdown.** The queue closes to new claims; the in-flight job stops at its next safe step
//!   boundary (after a provider call returned and its bundle is persisted, or before the next
//!   phase starts) and is Failed with `E_JOB_INTERRUPTED`; a job that does not reach one within
//!   the grace period is persisted Failed with `E_JOB_INTERRUPTED` by the shutdown itself.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use plumb_core::{to_canonical_json, Hash, Timestamp};
use plumb_store::{StageLease, StageStatus};
use serde_json::json;

use crate::error::ApiError;
use crate::state::ApiState;

use super::acceptance::{self, decode_child, persist_child};
use super::{bounded, decode_capture, failure_message, JOB_INTERRUPTED};

/// How long a claimed or created job stays leased to its owner without renewal.
pub const STAGE_LEASE_SECONDS: u64 = 60;
/// How often the heartbeat renews the leases of this process's in-flight jobs.
pub const STAGE_HEARTBEAT_SECONDS: u64 = 15;
/// How long a graceful shutdown waits for the in-flight job to reach a safe step boundary.
pub const STAGE_SHUTDOWN_GRACE_SECONDS: u64 = 30;

/// The failure code of a row that recovery could not decode or reconcile.
const CORRUPT: &str = "E_WORKFLOW_CORRUPT";

/// The lifecycle of one application instance's stage execution.
#[derive(Default)]
pub(crate) struct Lifecycle {
    owner_id: OnceLock<String>,
    queue: Mutex<Option<mpsc::Sender<Hash>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    heartbeat: Mutex<Option<mpsc::Sender<()>>>,
    closing: AtomicBool,
    current: Mutex<Option<Hash>>,
}

impl Lifecycle {
    /// Hands `id` to the worker; false when no worker runs or the queue is closed.
    pub(crate) fn enqueue(&self, id: &Hash) -> bool {
        if self.closing.load(Ordering::SeqCst) {
            return false;
        }
        match self.queue.lock() {
            Ok(queue) => queue
                .as_ref()
                .is_some_and(|sender| sender.send(id.clone()).is_ok()),
            Err(_) => false,
        }
    }

    pub(crate) fn owner_id(&self) -> Option<&str> {
        self.owner_id.get().map(String::as_str)
    }

    fn set_current(&self, id: Option<Hash>) {
        if let Ok(mut current) = self.current.lock() {
            *current = id;
        }
    }
}

/// The instant this process first computed an owner identity (stable for its lifetime).
fn process_start_time() -> &'static str {
    static START: OnceLock<String> = OnceLock::new();
    START.get_or_init(|| {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}.{:09}", since.as_secs(), since.subsec_nanos())
    })
}

/// `own:<first 16 hex of SHA-256(RFC 8785 {db_path, pid, process_start_time})>`.
pub(crate) fn owner_id_for(db_path: Option<&Path>) -> String {
    let path = db_path.map_or_else(|| ":memory:".to_owned(), |p| p.display().to_string());
    let material = json!({
        "db_path": path,
        "pid": std::process::id(),
        "process_start_time": process_start_time(),
    });
    let bytes = to_canonical_json(&material).unwrap_or_default();
    let hash = Hash::content_sha256(&bytes);
    let hex = hash.as_str().rsplit(':').next().unwrap_or_default();
    format!("own:{}", &hex[..16])
}

/// `at` + `seconds`, saturating at `at` when out of the canonical range.
fn plus(at: Timestamp, seconds: u64) -> Timestamp {
    Timestamp::try_from(at.as_offset_datetime() + Duration::from_secs(seconds)).unwrap_or(at)
}

/// 409 `E_JOB_INTERRUPTED`: the job stopped at a safe step boundary because its process is
/// shutting down (retryable).
pub(crate) fn interrupted(reason: &str) -> ApiError {
    ApiError::conflict(
        JOB_INTERRUPTED,
        "the job was interrupted because its process is shutting down",
    )
    .with_details(json!({ "reason": reason }))
}

impl ApiState {
    fn lifecycle(&self) -> Option<&Lifecycle> {
        self.stage_runtime("stage_worker")
            .ok()
            .map(|a04| &a04.lifecycle)
    }

    /// The lease this process takes on a job it creates, retries or claims now; `None` when
    /// this instance executes no stage work (read-only).
    pub(crate) fn stage_lease(&self) -> Option<StageLease> {
        let owner = self.lifecycle()?.owner_id()?;
        Some(StageLease {
            owner_id: owner.to_owned(),
            expires_at: plus(self.now(), STAGE_LEASE_SECONDS),
        })
    }

    /// Whether this instance's in-flight job must stop at its next safe step boundary.
    pub(crate) fn stage_stop_requested(&self) -> bool {
        self.lifecycle()
            .is_some_and(|l| l.closing.load(Ordering::SeqCst))
    }

    /// Hands a queued recipe job to this instance's worker. When no worker can take it (the
    /// queue is closed or the worker is gone) the job, if queued by this process, is failed
    /// as interrupted (a rerun retries it) and the call is 503 `E_STAGE_WORKER_UNAVAILABLE`.
    pub fn enqueue_recipe_job(&self, id: &Hash) -> Result<(), ApiError> {
        let a04 = self.stage_runtime("stage_worker")?;
        if a04.lifecycle.enqueue(id) {
            return Ok(());
        }
        let owner = a04.lifecycle.owner_id().map(str::to_owned);
        let record = self
            .with_stage_store(|store| store.recipe_job(id))?
            .ok()
            .flatten();
        if let Some(record) = record {
            if record.status == StageStatus::Queued && record.owner_id == owner {
                self.with_stage_store(|store| {
                    store.interrupt_recipe_job(
                        id,
                        JOB_INTERRUPTED,
                        "the stage worker of this process was not running",
                        self.now(),
                    )
                })?
                .map_err(super::store_error)?;
            }
        }
        Err(ApiError::stage_worker_unavailable(id.as_str()))
    }

    /// Closes this instance's job queue to new claims and asks the in-flight job to stop at its
    /// next safe step boundary (the first half of a graceful shutdown).
    pub fn begin_stage_shutdown(&self) {
        let Some(lifecycle) = self.lifecycle() else {
            return;
        };
        lifecycle.closing.store(true, Ordering::SeqCst);
        if let Ok(mut queue) = lifecycle.queue.lock() {
            queue.take();
        }
    }

    /// Waits up to `grace` for the worker to finish and joins it. Returns false when the
    /// in-flight job did not reach a safe step boundary in time; it is then persisted Failed
    /// with `E_JOB_INTERRUPTED` and the worker is left detached.
    pub fn finish_stage_shutdown(&self, grace: Duration) -> bool {
        let Some(lifecycle) = self.lifecycle() else {
            return true;
        };
        let handle = lifecycle.worker.lock().ok().and_then(|mut w| w.take());
        let deadline = Instant::now() + grace;
        let drained = match handle {
            None => true,
            Some(handle) => {
                while !handle.is_finished() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(5));
                }
                if handle.is_finished() {
                    let _ = handle.join();
                    true
                } else {
                    let current = lifecycle.current.lock().ok().and_then(|c| c.clone());
                    if let Some(id) = current {
                        tracing::error!(
                            target: "plumb_api::stage",
                            job_id = %id,
                            "the in-flight job missed the shutdown grace period"
                        );
                        fail_in_flight(
                            self,
                            &id,
                            JOB_INTERRUPTED,
                            "the job did not reach a safe step boundary within the shutdown \
                             grace period",
                        );
                    }
                    false
                }
            }
        };
        if let Ok(mut heartbeat) = lifecycle.heartbeat.lock() {
            heartbeat.take();
        }
        drained
    }

    /// The whole graceful shutdown with the frozen [`STAGE_SHUTDOWN_GRACE_SECONDS`].
    pub fn shutdown_stages(&self) -> bool {
        self.begin_stage_shutdown();
        self.finish_stage_shutdown(Duration::from_secs(STAGE_SHUTDOWN_GRACE_SECONDS))
    }

    /// Executes a claimed job on the worker: any `Err` or panic persists the job Failed with
    /// `E_INTERNAL` and its bounded message and is logged; nothing is discarded.
    fn execute_guarded(&self, id: &Hash) {
        let lifecycle = self.lifecycle();
        if let Some(l) = lifecycle {
            l.set_current(Some(id.clone()));
        }
        let outcome = catch_unwind(AssertUnwindSafe(|| self.execute_recipe_job(id)));
        if let Some(l) = lifecycle {
            l.set_current(None);
        }
        let failure = match outcome {
            Ok(Ok(())) => return,
            Ok(Err(error)) => failure_message(&error),
            Err(panic) => {
                let text = panic
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic".to_owned());
                format!("the job panicked: {text}")
            }
        };
        let message = bounded(&failure);
        tracing::error!(
            target: "plumb_api::stage",
            job_id = %id,
            error = %message,
            "executing a claimed recipe job failed"
        );
        fail_in_flight(self, id, "E_INTERNAL", &message);
    }

    fn renew_stage_leases(&self) {
        let Some(lease) = self.stage_lease() else {
            return;
        };
        let renewed =
            self.with_stage_store(|store| store.renew_leases(&lease.owner_id, lease.expires_at));
        if !matches!(renewed, Ok(Ok(_))) {
            tracing::error!(target: "plumb_api::stage", "renewing the stage leases failed");
        }
    }
}

/// Fails the in-flight job `id` (and its in-flight children) with `code`; best effort, logged.
fn fail_in_flight(state: &ApiState, id: &Hash, code: &str, message: &str) {
    if let Err(error) = fail_children(state, id) {
        tracing::error!(target: "plumb_api::stage", job_id = %id, code = error.body().code,
            "failing the children of a recipe job failed");
    }
    let failed =
        state.with_stage_store(|store| store.interrupt_recipe_job(id, code, message, state.now()));
    if !matches!(failed, Ok(Ok(_))) {
        tracing::error!(target: "plumb_api::stage", job_id = %id, "persisting a failed job failed");
    }
}

/// Fails every in-flight compiler child recorded by the steps of `id`.
fn fail_children(state: &ApiState, id: &Hash) -> Result<(), ApiError> {
    let steps = state
        .with_stage_store(|store| store.steps(id))?
        .map_err(super::store_error)?;
    for child_id in steps.iter().filter_map(|s| s.stage_job_id.as_ref()) {
        fail_child(state, child_id)?;
    }
    Ok(())
}

/// Fails one compiler child if it is still in flight.
fn fail_child(state: &ApiState, child_id: &Hash) -> Result<(), ApiError> {
    let Some(record) = state
        .with_stage_store(|store| store.stage_job(child_id))?
        .map_err(|_| ApiError::workflow_corrupt(&format!("stage job {child_id}")))?
    else {
        return Ok(());
    };
    if !record.status.is_in_flight() {
        return Ok(());
    }
    let mut job = decode_child(&record.stage_job_json, child_id)?;
    job.mark_failed()
        .map_err(|_| ApiError::workflow_corrupt("stage job"))?;
    persist_child(state, &job, record.status, state.now())
}

/// Construction-time start of the stage runtime: the owner identity, startup recovery, the
/// recipe worker and the lease heartbeat. Nothing resumes.
pub(crate) fn start(state: &ApiState, sole: bool) -> Result<(), ApiError> {
    let a04 = state.stage_runtime("stage_start")?;
    let db = state.with_stage_store(|store| store.database_path())?;
    let _ = a04.lifecycle.owner_id.set(owner_id_for(db.as_deref()));
    // An interactive instance executes and recovers stage work: it needs a writable database
    // even when there is nothing to recover (a read-only file is E_DATABASE_READ_ONLY here).
    state
        .with_stage_store(|store| store.require_writable())?
        .map_err(super::store_error)?;
    recover(state, sole)?;
    start_worker(state)?;
    start_heartbeat(state)
}

/// Startup recovery (§33.3, Fables #183 and #188): every abandoned in-flight job is settled to
/// its committed revision, settled Ready over its persisted proposals or interrupted; a row that
/// cannot be decoded or reconciled is failed with `E_WORKFLOW_CORRUPT` and logged, and the
/// others are still processed.
fn recover(state: &ApiState, sole: bool) -> Result<(), ApiError> {
    let now = state.now();
    let abandoned = state
        .with_stage_store(|store| {
            if sole {
                store.in_flight_recipe_job_ids()
            } else {
                store.abandoned_recipe_jobs(now)
            }
        })?
        .map_err(super::store_error)?;
    let head = state.load_head()?;
    for id in &abandoned {
        if let Err(error) = recover_job(state, &head, id) {
            let message = bounded(&format!(
                "{}: {}",
                error.body().code,
                failure_message(&error)
            ));
            tracing::error!(
                target: "plumb_api::stage",
                job_id = %id,
                error = %message,
                "an abandoned recipe job could not be recovered"
            );
            fail_in_flight(state, id, CORRUPT, &message);
        }
    }
    if sole {
        // Without any other live opener, every in-flight child is abandoned too.
        let children = state
            .with_stage_store(|store| store.in_flight_stage_job_ids())?
            .map_err(super::store_error)?;
        for child_id in children {
            if let Err(error) = fail_child(state, &child_id) {
                tracing::error!(target: "plumb_api::stage", stage_job_id = %child_id,
                    code = error.body().code, "an abandoned stage job could not be recovered");
            }
        }
    }
    acceptance::reconcile_pending(state)
}

/// Recovers one abandoned recipe job.
fn recover_job(
    state: &ApiState,
    head: &plumb_store::LoadedRevision,
    id: &Hash,
) -> Result<(), ApiError> {
    let record = state
        .with_stage_store(|store| store.recipe_job(id))?
        .map_err(|_| ApiError::workflow_corrupt(&format!("stage recipe job {id}")))?
        .ok_or_else(|| ApiError::workflow_corrupt(&format!("stage recipe job {id}")))?;
    if !record.status.is_in_flight() {
        return Ok(());
    }
    decode_capture(&record)?;
    // H0.10: committed plain work is settled before anything in flight is interrupted.
    if acceptance::settle_plain_parent(state, head, &record)? {
        return Ok(());
    }
    // Fable #188: persisted pending proposals keep a parent that can reflect their acceptance.
    acceptance::settle_over_pending(state, &record)?;
    fail_children(state, id)?;
    state
        .with_stage_store(|store| {
            store.interrupt_recipe_job(
                id,
                JOB_INTERRUPTED,
                "the job was interrupted by a process restart",
                state.now(),
            )
        })?
        .map_err(super::store_error)?;
    Ok(())
}

/// Starts the recipe worker thread of `state`.
fn start_worker(state: &ApiState) -> Result<(), ApiError> {
    let (sender, receiver) = mpsc::channel::<Hash>();
    let weak = state.downgrade();
    let handle = thread::Builder::new()
        .name("plumb-stage-worker".into())
        .spawn(move || {
            for id in receiver {
                let Some(state) = weak.upgrade() else { break };
                if state.stage_stop_requested() {
                    // Closed to new claims: a job still queued here is interrupted.
                    if let Ok(Ok(Some(record))) = state.with_stage_store(|s| s.recipe_job(&id)) {
                        if record.status == StageStatus::Queued {
                            fail_in_flight(
                                &state,
                                &id,
                                JOB_INTERRUPTED,
                                "the process shut down before the job started",
                            );
                        }
                    }
                    continue;
                }
                state.execute_guarded(&id);
            }
        })
        .map_err(|e| ApiError::internal_cause("stage_worker", &e))?;
    let lifecycle = &state.stage_runtime("stage_worker")?.lifecycle;
    *lifecycle
        .queue
        .lock()
        .map_err(|e| ApiError::internal_cause("stage_queue", &e))? = Some(sender);
    *lifecycle
        .worker
        .lock()
        .map_err(|e| ApiError::internal_cause("stage_worker", &e))? = Some(handle);
    Ok(())
}

/// Starts the lease heartbeat of `state`: it renews this owner's leases every
/// [`STAGE_HEARTBEAT_SECONDS`] and stops with the instance or its shutdown.
fn start_heartbeat(state: &ApiState) -> Result<(), ApiError> {
    let (stop, stopped) = mpsc::channel::<()>();
    let weak = state.downgrade();
    thread::Builder::new()
        .name("plumb-stage-heartbeat".into())
        .spawn(move || {
            // A message or a dropped sender (the instance or its shutdown) stops the heartbeat.
            while let Err(mpsc::RecvTimeoutError::Timeout) =
                stopped.recv_timeout(Duration::from_secs(STAGE_HEARTBEAT_SECONDS))
            {
                let Some(state) = weak.upgrade() else { break };
                state.renew_stage_leases();
            }
        })
        .map_err(|e| ApiError::internal_cause("stage_heartbeat", &e))?;
    *state
        .stage_runtime("stage_heartbeat")?
        .lifecycle
        .heartbeat
        .lock()
        .map_err(|e| ApiError::internal_cause("stage_heartbeat", &e))? = Some(stop);
    Ok(())
}
