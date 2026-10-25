//! Runtime state and shared request plumbing of the pilot HTTP API (plan A0.2, Hotfix 063;
//! A0.3, Hotfix 065).
//!
//! `ApiState` owns one explicit branch, the SQLite revision and artifact stores behind
//! synchronous mutexes (never held across an await), the injected identity and clock, the
//! design-event broadcaster, the orchestration-registered gate reports and the validation
//! profile, plus the optional injected A0.3 runtime (durable workflow store, providers, session
//! configuration and simulator scopes) and the optional injected A0.4 stage runtime (durable stage
//! store, explicit recipe inputs and the recipe worker, Hotfix 067). Current gate reports are the
//! durable store-v3 reports with the startup-injected ones as compatibility fallback, returned
//! owned (Hotfix 068). There is no global graph or provider: every request loads
//! the branch head once and answers or writes against exactly that revision. Writes go through
//! the existing intake, the rationale guard and the store's compare-and-swap commit; nothing
//! mutates a graph in place.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, Weak};

use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use plumb_artifacts::{ArtifactKind, ArtifactStore, SqliteArtifactStore};
use plumb_core::{to_canonical_json, Clock, GateId, Hash, HashKind, Id, Timestamp};
use plumb_functional::resolution::{build_acceptance_companion, AcceptanceInput, ResolutionError};
use plumb_functional::{CalendarProvider, PredicateProvider};
use plumb_inference::{InferenceProvider, ProviderPolicy};
use plumb_intake::{
    intake, validate_required_rationale, IntakeAcceptanceError, IntakeError, IntakeReport,
    IntakeValidationInput,
};
use plumb_patch::{PatchSet, Proposal, SemanticPatch};
use plumb_psg::{ElementStatus, NodePayload};
use plumb_session::SessionTurnConfig;
use plumb_sim::{
    ExecutionCalculationScopeOverride, ExecutionExpressionScopeInput, RuntimeSemanticValue,
};
use plumb_store::{
    BranchName, CommitResult, GraphRevision, LoadedRevision, RevisionId, RevisionWriteMeta,
    SqliteRevisionStore, SqliteStageStore, SqliteWorkflowStore, StoreError,
};
use plumb_validation::{GateReport, ValidationProfile};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::broadcast;

use crate::error::{echo, ApiError};
use crate::openapi::{DesignEvent, ItemHealth, WriteResult};
use crate::runtime::{
    A04RuntimeConfig, CalendarConfig, DirectManagerFact, GateInputsConfig, ProviderMode,
};
use plumb_functional::{
    AuthorizationDraft, CalculationScope, DuplicatePolicy, OperationScope, ProcessScope,
    ScenarioDerivationInputs, VerificationApplicabilityInputs, VocabularyPolicy,
};
use plumb_validation::ValidationPolicy;

/// Capacity of the design-event channel; a slower subscriber is disconnected, never skipped.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// How one application instance uses its database (architecture §33.3, §33.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// The worker, startup recovery and reconciliation run; on-demand gate reports persist.
    Interactive,
    /// Read-only commands: the stores are opened read-only; no worker, no recovery or
    /// reconciliation writes, and gate reports evaluated on demand are not persisted.
    ReadOnly,
}

/// Why runtime state cannot be constructed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApiStateError {
    #[error("identity user must not be blank")]
    BlankUser,
    #[error("identity mode must not be blank")]
    BlankMode,
    #[error("identity roles must be sorted and unique")]
    UnsortedRoles,
    #[error("profile hashes cannot be computed: {0}")]
    Profile(String),
    #[error("expression scope owner {0} is configured more than once")]
    DuplicateExpressionScope(Id),
    #[error("invalid A0.4 runtime: {0}")]
    StageRuntime(String),
}

/// The typed A0.3 runtime configuration (Hotfix 065): exactly the injected material the A0.3
/// operations need, owned by one state instance. Nothing here is global, environment-selected
/// or built from a request.
pub struct A03RuntimeConfig {
    /// The durable workflow store over the same database as the revision store.
    pub workflow_store: SqliteWorkflowStore,
    pub inference_provider: Arc<dyn InferenceProvider + Send + Sync>,
    pub provider_policy: ProviderPolicy,
    pub session_turn_config: SessionTurnConfig,
    /// The Accepted Agent that speaks for sessions.
    pub assistant_actor_ref: Id,
    pub calendars: Arc<dyn CalendarProvider + Send + Sync>,
    pub predicates: Arc<dyn PredicateProvider + Send + Sync>,
    /// Explicit expression scopes, unique by `owner_ref`.
    pub expression_scopes: Vec<ExecutionExpressionScopeInput>,
    pub calculation_scope_overrides: Vec<ExecutionCalculationScopeOverride>,
    pub runtime_semantic_values: Vec<RuntimeSemanticValue>,
}

/// The validated A0.3 runtime held by the state.
pub(crate) struct A03Runtime {
    workflow_store: Mutex<SqliteWorkflowStore>,
    pub(crate) inference_provider: Arc<dyn InferenceProvider + Send + Sync>,
    pub(crate) provider_policy: ProviderPolicy,
    pub(crate) session_turn_config: SessionTurnConfig,
    pub(crate) assistant_actor_ref: Id,
    pub(crate) calendars: Arc<dyn CalendarProvider + Send + Sync>,
    pub(crate) predicates: Arc<dyn PredicateProvider + Send + Sync>,
    pub(crate) expression_scopes: Vec<ExecutionExpressionScopeInput>,
    pub(crate) calculation_scope_overrides: Vec<ExecutionCalculationScopeOverride>,
    pub(crate) runtime_semantic_values: Vec<RuntimeSemanticValue>,
    /// Sessions with turns being processed by this instance, whose prepared turns are not
    /// reconciled meanwhile (a concurrency guard only; workflow state stays durable).
    in_flight_sessions: Mutex<BTreeMap<Id, usize>>,
}

/// The validated A0.4 stage runtime held by the state (Hotfix 067).
pub(crate) struct A04Runtime {
    stage_store: Mutex<SqliteStageStore>,
    pub(crate) stage_actor_ref: Id,
    pub(crate) provider_mode: ProviderMode,
    pub(crate) validation_policy: ValidationPolicy,
    pub(crate) vocabulary_policy: VocabularyPolicy,
    pub(crate) duplicate_policy: DuplicatePolicy,
    pub(crate) calendars: Vec<CalendarConfig>,
    pub(crate) direct_managers: Vec<DirectManagerFact>,
    pub(crate) calculation_scope: Option<CalculationScope>,
    pub(crate) operation_scope: Option<OperationScope>,
    pub(crate) process_scope: Option<ProcessScope>,
    pub(crate) authorization_draft: Option<AuthorizationDraft>,
    pub(crate) scenario_derivation: Option<ScenarioDerivationInputs>,
    pub(crate) verification_applicability: Option<VerificationApplicabilityInputs>,
    pub(crate) gates: GateInputsConfig,
    pub(crate) directory: Option<plumb_functional::DirectoryDraft>,
    pub(crate) question_routing: Option<plumb_functional::StakeholderRoutingConfig>,
    /// The recipe worker, its queue, the lease heartbeat and the owner identity (H0.3).
    pub(crate) lifecycle: crate::stage::worker::Lifecycle,
    /// The shared `<db>.jobs.lock` held while this instance is open (see `stage::start`).
    jobs_lock: Mutex<Option<std::fs::File>>,
}

impl A04Runtime {
    fn new(config: A04RuntimeConfig) -> Result<A04Runtime, ApiStateError> {
        config
            .validate()
            .map_err(|e| ApiStateError::StageRuntime(e.to_string()))?;
        Ok(A04Runtime {
            stage_store: Mutex::new(config.stage_store),
            stage_actor_ref: config.stage_actor_ref,
            provider_mode: config.provider_mode,
            validation_policy: config.validation_policy,
            vocabulary_policy: config.vocabulary_policy,
            duplicate_policy: config.duplicate_policy,
            calendars: config.calendars,
            direct_managers: config.direct_managers,
            calculation_scope: config.calculation_scope,
            operation_scope: config.operation_scope,
            process_scope: config.process_scope,
            authorization_draft: config.authorization_draft,
            scenario_derivation: config.scenario_derivation,
            verification_applicability: config.verification_applicability,
            gates: config.gates,
            directory: config.directory,
            question_routing: config.question_routing,
            lifecycle: crate::stage::worker::Lifecycle::default(),
            jobs_lock: Mutex::new(None),
        })
    }

    /// Keeps this instance's shared jobs lock until the instance is dropped.
    pub(crate) fn hold_jobs_lock(&self, file: std::fs::File) {
        if let Ok(mut held) = self.jobs_lock.lock() {
            *held = Some(file);
        }
    }
}

/// A non-owning handle to the state, held by the recipe worker so it never keeps the state
/// alive.
#[derive(Clone)]
pub(crate) struct WeakState(Weak<Inner>);

impl WeakState {
    pub(crate) fn upgrade(&self) -> Option<ApiState> {
        self.0.upgrade().map(|inner| ApiState { inner })
    }
}

/// What a governed or plain write committed: the exact PatchSet and the store result.
pub(crate) struct Committed {
    pub(crate) patch_set: PatchSet,
    pub(crate) commit: CommitResult,
}

/// Marks a session as having a turn in flight until dropped.
pub(crate) struct InFlightTurn<'a> {
    runtime: &'a A03Runtime,
    session: Id,
}

impl Drop for InFlightTurn<'_> {
    fn drop(&mut self) {
        if let Ok(mut sessions) = self.runtime.in_flight_sessions.lock() {
            if let Some(count) = sessions.get_mut(&self.session) {
                *count -= 1;
                if *count == 0 {
                    sessions.remove(&self.session);
                }
            }
        }
    }
}

impl A03Runtime {
    /// Marks `session` as having a turn in flight in this instance.
    pub(crate) fn begin_turn(&self, session: &Id) -> Result<InFlightTurn<'_>, ApiError> {
        let mut in_flight = self
            .in_flight_sessions
            .lock()
            .map_err(|e| ApiError::internal_cause("session_turns", &e))?;
        // Fable #207: one turn per session at a time, refused before any write or provider call.
        if in_flight.contains_key(session) {
            return Err(ApiError::conflict(
                "E_SESSION_TURN_IN_FLIGHT",
                "a turn of this session is still in flight",
            )
            .with_details(json!({ "session": session })));
        }
        *in_flight.entry(session.clone()).or_default() += 1;
        Ok(InFlightTurn {
            runtime: self,
            session: session.clone(),
        })
    }

    /// Whether a turn of `session` is in flight in this instance.
    pub(crate) fn turn_in_flight(&self, session: &Id) -> Result<bool, ApiError> {
        Ok(self
            .in_flight_sessions
            .lock()
            .map_err(|e| ApiError::internal_cause("session_turns", &e))?
            .contains_key(session))
    }
    fn new(config: A03RuntimeConfig) -> Result<A03Runtime, ApiStateError> {
        let mut owners = BTreeSet::new();
        for scope in &config.expression_scopes {
            if !owners.insert(&scope.owner_ref) {
                return Err(ApiStateError::DuplicateExpressionScope(
                    scope.owner_ref.clone(),
                ));
            }
        }
        Ok(A03Runtime {
            workflow_store: Mutex::new(config.workflow_store),
            inference_provider: config.inference_provider,
            provider_policy: config.provider_policy,
            session_turn_config: config.session_turn_config,
            assistant_actor_ref: config.assistant_actor_ref,
            calendars: config.calendars,
            predicates: config.predicates,
            expression_scopes: config.expression_scopes,
            calculation_scope_overrides: config.calculation_scope_overrides,
            runtime_semantic_values: config.runtime_semantic_values,
            in_flight_sessions: Mutex::new(BTreeMap::new()),
        })
    }
}

/// The injected API identity. `actor_ref` is provenance only and never returned by `/api/me`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiIdentity {
    user: String,
    actor_ref: Id,
    roles: Vec<String>,
    mode: String,
}

impl ApiIdentity {
    /// Requires a non-blank user and mode and sorted, unique roles.
    pub fn new(
        user: String,
        actor_ref: Id,
        roles: Vec<String>,
        mode: String,
    ) -> Result<ApiIdentity, ApiStateError> {
        if user.trim().is_empty() {
            return Err(ApiStateError::BlankUser);
        }
        if mode.trim().is_empty() {
            return Err(ApiStateError::BlankMode);
        }
        if roles.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ApiStateError::UnsortedRoles);
        }
        Ok(ApiIdentity {
            user,
            actor_ref,
            roles,
            mode,
        })
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn actor_ref(&self) -> &Id {
        &self.actor_ref
    }

    pub fn roles(&self) -> &[String] {
        &self.roles
    }

    pub fn mode(&self) -> &str {
        &self.mode
    }
}

struct Inner {
    branch: BranchName,
    revision_store: Mutex<SqliteRevisionStore>,
    artifact_store: Mutex<SqliteArtifactStore>,
    identity: ApiIdentity,
    clock: Arc<dyn Clock + Send + Sync>,
    events: broadcast::Sender<DesignEvent>,
    gate_reports: Vec<GateReport>,
    profile: ValidationProfile,
    profile_hash: Hash,
    rule_pack_hash: Hash,
    runtime: Option<A03Runtime>,
    stage: Option<A04Runtime>,
    mode: OpenMode,
}

/// Cheaply cloneable shared state of the pilot HTTP API.
#[derive(Clone)]
pub struct ApiState {
    inner: Arc<Inner>,
}

impl ApiState {
    /// Builds the state for one explicit branch. `gate_reports` are the orchestration-supplied
    /// reports; only those matching the loaded head are ever presented as current.
    /// The A0.3 runtime is absent: every operation that needs it answers 503.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        branch: BranchName,
        revision_store: SqliteRevisionStore,
        artifact_store: SqliteArtifactStore,
        identity: ApiIdentity,
        clock: Arc<dyn Clock + Send + Sync>,
        gate_reports: Vec<GateReport>,
        profile: ValidationProfile,
    ) -> Result<ApiState, ApiStateError> {
        ApiState::build(
            branch,
            revision_store,
            artifact_store,
            identity,
            clock,
            gate_reports,
            profile,
            None,
        )
    }

    /// Builds the full A0.3 state: the core state plus the injected A0.3 runtime (Hotfix 065).
    /// A duplicate expression-scope owner is a construction error.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_runtime(
        branch: BranchName,
        revision_store: SqliteRevisionStore,
        artifact_store: SqliteArtifactStore,
        identity: ApiIdentity,
        clock: Arc<dyn Clock + Send + Sync>,
        gate_reports: Vec<GateReport>,
        profile: ValidationProfile,
        runtime: A03RuntimeConfig,
    ) -> Result<ApiState, ApiStateError> {
        let runtime = A03Runtime::new(runtime)?;
        ApiState::build(
            branch,
            revision_store,
            artifact_store,
            identity,
            clock,
            gate_reports,
            profile,
            Some(runtime),
        )
    }

    /// Builds the full A0.4 state: the A0.3 state plus the injected A0.4 stage runtime
    /// (Hotfix 067). Construction fails every recipe job and compiler child left in flight by a
    /// previous process (`E_JOB_INTERRUPTED`; nothing resumes), reconciles interrupted stage
    /// acceptances and starts this instance's recipe worker.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_stage_runtime(
        branch: BranchName,
        revision_store: SqliteRevisionStore,
        artifact_store: SqliteArtifactStore,
        identity: ApiIdentity,
        clock: Arc<dyn Clock + Send + Sync>,
        gate_reports: Vec<GateReport>,
        profile: ValidationProfile,
        runtime: A03RuntimeConfig,
        stage: A04RuntimeConfig,
    ) -> Result<ApiState, ApiStateError> {
        ApiState::new_with_stage_runtime_in(
            OpenMode::Interactive,
            branch,
            revision_store,
            artifact_store,
            identity,
            clock,
            gate_reports,
            profile,
            runtime,
            stage,
        )
    }

    /// [`ApiState::new_with_stage_runtime`] in `mode`: a `ReadOnly` instance starts no worker
    /// and performs no startup recovery or reconciliation (architecture §33.3, §33.9).
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_stage_runtime_in(
        mode: OpenMode,
        branch: BranchName,
        revision_store: SqliteRevisionStore,
        artifact_store: SqliteArtifactStore,
        identity: ApiIdentity,
        clock: Arc<dyn Clock + Send + Sync>,
        gate_reports: Vec<GateReport>,
        profile: ValidationProfile,
        runtime: A03RuntimeConfig,
        stage: A04RuntimeConfig,
    ) -> Result<ApiState, ApiStateError> {
        let runtime = A03Runtime::new(runtime)?;
        let stage = A04Runtime::new(stage)?;
        let mut state = ApiState::build(
            branch,
            revision_store,
            artifact_store,
            identity,
            clock,
            gate_reports,
            profile,
            Some(runtime),
        )?;
        let inner = Arc::get_mut(&mut state.inner)
            .ok_or_else(|| ApiStateError::StageRuntime("state is shared".into()))?;
        inner.stage = Some(stage);
        inner.mode = mode;
        if mode == OpenMode::ReadOnly {
            return Ok(state);
        }
        crate::stage::start(&state).map_err(|e| {
            ApiStateError::StageRuntime(format!("{}: {}", e.body().code, e.body().message))
        })?;
        Ok(state)
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        branch: BranchName,
        revision_store: SqliteRevisionStore,
        artifact_store: SqliteArtifactStore,
        identity: ApiIdentity,
        clock: Arc<dyn Clock + Send + Sync>,
        gate_reports: Vec<GateReport>,
        profile: ValidationProfile,
        runtime: Option<A03Runtime>,
    ) -> Result<ApiState, ApiStateError> {
        let profile_hash = profile
            .profile_hash()
            .map_err(|e| ApiStateError::Profile(e.to_string()))?;
        let rule_pack_hash = profile
            .rule_pack_hash()
            .map_err(|e| ApiStateError::Profile(e.to_string()))?;
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Ok(ApiState {
            inner: Arc::new(Inner {
                branch,
                revision_store: Mutex::new(revision_store),
                artifact_store: Mutex::new(artifact_store),
                identity,
                clock,
                events,
                gate_reports,
                profile,
                profile_hash,
                rule_pack_hash,
                runtime,
                stage: None,
                mode: OpenMode::Interactive,
            }),
        })
    }

    /// Whether this instance was opened read-only.
    pub fn is_read_only(&self) -> bool {
        self.inner.mode == OpenMode::ReadOnly
    }

    /// Releases this instance's shared jobs lock: from now on the instance no longer counts as
    /// a live opener of the database (a later opener may reconcile its in-flight jobs).
    pub fn release_jobs_lock(&self) {
        if let Some(a04) = self.inner.stage.as_ref() {
            if let Ok(mut held) = a04.jobs_lock.lock() {
                *held = None;
            }
        }
    }

    pub fn identity(&self) -> &ApiIdentity {
        &self.inner.identity
    }

    /// A new subscription to future design events.
    pub fn subscribe(&self) -> broadcast::Receiver<DesignEvent> {
        self.inner.events.subscribe()
    }

    pub(crate) fn profile(&self) -> &ValidationProfile {
        &self.inner.profile
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.inner.clock.now()
    }

    pub(crate) fn clock(&self) -> &(dyn Clock + Send + Sync) {
        self.inner.clock.as_ref()
    }

    /// The injected API actor (provenance).
    pub(crate) fn actor_ref(&self) -> &Id {
        &self.inner.identity.actor_ref
    }

    /// The A0.3 runtime, or 503 naming the missing `capability`.
    pub(crate) fn runtime(&self, capability: &str) -> Result<&A03Runtime, ApiError> {
        self.inner
            .runtime
            .as_ref()
            .ok_or_else(|| ApiError::runtime_unavailable(capability))
    }

    /// Whether inference can run: false exactly when the stage runtime configures the `null`
    /// provider mode (H0.11, Fable #355).
    pub(crate) fn inference_available(&self) -> bool {
        !matches!(
            self.inner.stage.as_ref().map(|a04| &a04.provider_mode),
            Some(ProviderMode::Null)
        )
    }

    /// The A0.4 stage runtime, or 503 naming the missing `capability`.
    pub(crate) fn stage_runtime(&self, capability: &str) -> Result<&A04Runtime, ApiError> {
        self.inner
            .stage
            .as_ref()
            .ok_or_else(|| ApiError::runtime_unavailable(capability))
    }

    /// Runs `f` with the durable stage store; 503 when the A0.4 runtime is absent.
    pub(crate) fn with_stage_store<R>(
        &self,
        f: impl FnOnce(&mut SqliteStageStore) -> R,
    ) -> Result<R, ApiError> {
        let runtime = self.stage_runtime("stage_store")?;
        let mut store = runtime
            .stage_store
            .lock()
            .map_err(|e| ApiError::internal_cause("stage_store", &e))?;
        Ok(f(&mut store))
    }

    /// The branch this state reads and writes.
    pub(crate) fn branch(&self) -> &BranchName {
        &self.inner.branch
    }

    /// A non-owning handle for the recipe worker.
    pub(crate) fn downgrade(&self) -> WeakState {
        WeakState(Arc::downgrade(&self.inner))
    }

    /// Runs `f` with the durable workflow store; 503 when the runtime is absent.
    pub(crate) fn with_workflow_store<R>(
        &self,
        f: impl FnOnce(&mut SqliteWorkflowStore) -> R,
    ) -> Result<R, ApiError> {
        let runtime = self.runtime("workflow_store")?;
        let mut store = runtime
            .workflow_store
            .lock()
            .map_err(|e| ApiError::internal_cause("workflow_store", &e))?;
        Ok(f(&mut store))
    }

    pub(crate) fn with_artifact_store<R>(
        &self,
        f: impl FnOnce(&mut SqliteArtifactStore) -> R,
    ) -> Result<R, ApiError> {
        let mut store = self
            .inner
            .artifact_store
            .lock()
            .map_err(|e| ApiError::internal_cause("artifact_store", &e))?;
        Ok(f(&mut store))
    }

    pub(crate) fn with_revision_store<R>(
        &self,
        f: impl FnOnce(&mut SqliteRevisionStore) -> R,
    ) -> Result<R, ApiError> {
        let mut store = self
            .inner
            .revision_store
            .lock()
            .map_err(|e| ApiError::internal_cause("revision_store", &e))?;
        Ok(f(&mut store))
    }

    /// Loads the branch head once: the only revision a request reads or writes against.
    pub(crate) fn load_head(&self) -> Result<LoadedRevision, ApiError> {
        self.with_revision_store(|store| {
            let head = store
                .head(&self.inner.branch)
                .map_err(|e| ApiError::store(&e, "revision_store"))?
                .ok_or_else(|| {
                    ApiError::store(
                        &StoreError::BranchNotFound(self.inner.branch.clone()),
                        "branch_head",
                    )
                })?;
            store
                .load_revision(&head)
                .map_err(|e| ApiError::store(&e, "revision_store"))
        })?
    }

    /// Walks the branch lineage from `head` towards the root and returns the most recent
    /// revision satisfying `found` (H0.10 crash reconciliation). The walk stops after `stop`
    /// (inclusive) when given, so it never looks behind the revision work was based on.
    pub(crate) fn find_on_branch(
        &self,
        head: &GraphRevision,
        stop: Option<&RevisionId>,
        mut found: impl FnMut(&GraphRevision) -> bool,
    ) -> Result<Option<GraphRevision>, ApiError> {
        let mut cursor = head.clone();
        loop {
            if found(&cursor) {
                return Ok(Some(cursor));
            }
            if stop.is_some_and(|stop| &cursor.id == stop || cursor.version <= stop.version()) {
                return Ok(None);
            }
            match cursor.parent.clone() {
                Some(parent) => cursor = self.load_revision(&parent)?.revision,
                None => return Ok(None),
            }
        }
    }

    /// The revision on the branch lineage of `head` committed directly from `parent`, if any.
    pub(crate) fn child_on_branch(
        &self,
        head: &GraphRevision,
        parent: &RevisionId,
    ) -> Result<Option<GraphRevision>, ApiError> {
        self.find_on_branch(head, Some(parent), |r| r.parent.as_ref() == Some(parent))
    }

    /// Loads an immutable revision by ID; an unknown ID is 404.
    pub(crate) fn load_revision(&self, id: &RevisionId) -> Result<LoadedRevision, ApiError> {
        self.with_revision_store(|store| match store.load_revision(id) {
            Ok(loaded) => Ok(loaded),
            Err(StoreError::RevisionNotFound(_)) => {
                Err(ApiError::not_found(format!("revision {id} not found")))
            }
            Err(e) => Err(ApiError::store(&e, "revision_store")),
        })?
    }

    /// Whether `report` is current for `loaded` under this profile (the A0.2 currency rule),
    /// and (H0.10, §33.11) has a result for every rule of its gate in the active profile.
    fn is_current(&self, report: &GateReport, loaded: &LoadedRevision) -> bool {
        let inner = &self.inner;
        let complete = inner
            .profile
            .gates
            .iter()
            .find(|g| g.id == report.gate)
            .is_some_and(|gate| {
                gate.rule_ids
                    .iter()
                    .all(|id| report.rules.iter().any(|r| &r.rule_id == id))
            });
        complete
            && report.validate().is_ok()
            && report.baseline_semantic_hash == loaded.revision.semantic_hash
            && report.baseline_evidence_hash == loaded.revision.evidence_hash
            && report.profile_id == inner.profile.profile_id
            && report.profile_hash == inner.profile_hash
            && report.rule_pack_hash == inner.rule_pack_hash
    }

    /// The gate reports that are current for `loaded`, owned, in I0, F1, F2, F3, F4 order and one
    /// per gate (Hotfix 067, Hotfix 068). Durable store-v3 reports take precedence: among the
    /// current durable rows of a gate the head revision's row wins, else the latest `created_at`,
    /// ties by revision ID; the startup-injected reports are the compatibility fallback. A report
    /// persisted by this process is current on the next read. Corrupt stored reports are 500.
    pub(crate) fn current_gate_reports(
        &self,
        loaded: &LoadedRevision,
    ) -> Result<Vec<GateReport>, ApiError> {
        let mut durable: BTreeMap<usize, (bool, Timestamp, RevisionId, GateReport)> =
            BTreeMap::new();
        if self.inner.stage.is_some() {
            let records = self
                .with_stage_store(|store| store.gate_reports())?
                .map_err(|_| ApiError::workflow_corrupt("gate reports"))?;
            // H0.10: a durable report is current only for the inputs assembled now (computed
            // once per gate and evaluation time; an assembly failure means not current).
            let mut inputs: BTreeMap<(GateId, Option<Timestamp>), Option<Option<Hash>>> =
                BTreeMap::new();
            for record in records {
                let report = crate::stage::gates::decode_report(&record)?;
                if !self.is_current(&report, loaded) {
                    continue;
                }
                let assembled = inputs
                    .entry((report.gate, record.evaluation_at))
                    .or_insert_with(|| {
                        crate::stage::gates::current_input_hash(
                            self,
                            loaded,
                            report.gate,
                            record.evaluation_at,
                        )
                        .ok()
                    });
                if assembled.as_ref() != Some(&record.supplemental_input_hash) {
                    continue;
                }
                let Some(position) = GateId::ALL.iter().position(|g| *g == report.gate) else {
                    continue;
                };
                let head = record.revision_id == loaded.revision.id;
                let key = (head, record.created_at, record.revision_id.clone());
                let better = match durable.get(&position) {
                    None => true,
                    Some((h, at, rev, _)) => {
                        (key.0, key.1, std::cmp::Reverse(&key.2))
                            > (*h, *at, std::cmp::Reverse(rev))
                    }
                };
                if better {
                    durable.insert(position, (key.0, key.1, key.2, report));
                }
            }
        }
        let mut current: Vec<GateReport> = Vec::new();
        for (position, gate) in GateId::ALL.iter().enumerate() {
            if let Some((_, _, _, report)) = durable.remove(&position) {
                current.push(report);
            } else if let Some(report) = self
                .inner
                .gate_reports
                .iter()
                .find(|r| r.gate == *gate && self.is_current(r, loaded))
            {
                current.push(report.clone());
            }
        }
        Ok(current)
    }

    /// Whether any report of `gate` exists, durable or startup-injected, for any revision.
    pub(crate) fn has_any_gate_report(&self, gate: GateId) -> Result<bool, ApiError> {
        if self.inner.gate_reports.iter().any(|r| r.gate == gate) {
            return Ok(true);
        }
        if self.inner.stage.is_none() {
            return Ok(false);
        }
        let records = self
            .with_stage_store(|store| store.gate_reports())?
            .map_err(|_| ApiError::workflow_corrupt("gate reports"))?;
        Ok(records.iter().any(|r| r.gate == gate.as_str()))
    }

    /// The current report of `gate`, owned, or 409 `E_GATE_REPORT_UNAVAILABLE`.
    pub(crate) fn current_gate_report(
        &self,
        loaded: &LoadedRevision,
        gate: GateId,
    ) -> Result<GateReport, ApiError> {
        self.current_gate_reports(loaded)?
            .into_iter()
            .find(|r| r.gate == gate)
            .ok_or_else(|| {
                ApiError::conflict(
                    "E_GATE_REPORT_UNAVAILABLE",
                    format!("no current {gate} gate report for the branch head"),
                )
                .with_details(json!({ "gate": gate.to_string() }))
            })
    }

    /// Runs the existing intake and rationale guard on `proposal` against `loaded`, then
    /// commits it with the loaded revision as expected head and broadcasts one design event.
    /// A moved head is 412 built from the reloaded head; nothing is retried or rebased.
    pub(crate) fn commit(
        &self,
        loaded: &LoadedRevision,
        proposal: &Proposal,
        created_at: Timestamp,
        operation_id: &'static str,
    ) -> Result<WriteResult, ApiError> {
        self.commit_with_artifacts(loaded, proposal, &[], created_at, operation_id)
    }

    /// [`ApiState::commit`] that stores `artifacts` in the revision-commit transaction
    /// (Fable #368): a lost or rejected CAS leaves none of them behind.
    pub(crate) fn commit_with_artifacts(
        &self,
        loaded: &LoadedRevision,
        proposal: &Proposal,
        artifacts: &[plumb_artifacts::Artifact],
        created_at: Timestamp,
        operation_id: &'static str,
    ) -> Result<WriteResult, ApiError> {
        let report = run_intake(loaded, proposal)?;
        match validate_required_rationale(proposal, &report, None) {
            Ok(()) => {}
            Err(IntakeAcceptanceError::ReasonRequired(_)) => {
                return Err(reason_required(proposal, &report))
            }
            Err(e) => return Err(ApiError::internal_cause("intake_rationale", &e)),
        }
        self.commit_patch_set_with_artifacts(
            loaded,
            &proposal.patch_set,
            Vec::new(),
            artifacts,
            created_at,
            operation_id,
        )
        .map(|(write, _)| write)
    }

    /// The A0.3 write pipeline (Hotfix 065): X1.3 intake of `proposal` against the loaded head,
    /// then either a plain commit or, for a governed acceptance (`governed`, or any write whose
    /// intake requires a reason), the proposal-acceptance companion decision committed with the
    /// unchanged original effect in one CAS. A required but absent reason is 409
    /// `E_INTAKE_REASON_REQUIRED` with nothing committed or persisted.
    pub(crate) fn write(
        &self,
        loaded: &LoadedRevision,
        proposal: &Proposal,
        governed: bool,
        reason: Option<&str>,
        created_at: Timestamp,
        operation_id: &'static str,
    ) -> Result<WriteResult, ApiError> {
        let report = run_intake(loaded, proposal)?;
        self.write_with_report(
            loaded,
            proposal,
            &report,
            governed,
            reason,
            created_at,
            operation_id,
        )
    }

    /// [`ApiState::write`] with an intake report already produced against `loaded`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_with_report(
        &self,
        loaded: &LoadedRevision,
        proposal: &Proposal,
        report: &IntakeReport,
        governed: bool,
        reason: Option<&str>,
        created_at: Timestamp,
        operation_id: &'static str,
    ) -> Result<WriteResult, ApiError> {
        self.write_with_report_detailed(
            loaded,
            proposal,
            report,
            governed,
            reason,
            created_at,
            operation_id,
        )
        .map(|(write, _)| write)
    }

    /// [`ApiState::write_with_report`] also returning the exact committed PatchSet (the
    /// proposal's own, or the governed acceptance companion) and the store result (A0.4).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_with_report_detailed(
        &self,
        loaded: &LoadedRevision,
        proposal: &Proposal,
        report: &IntakeReport,
        governed: bool,
        reason: Option<&str>,
        created_at: Timestamp,
        operation_id: &'static str,
    ) -> Result<(WriteResult, Committed), ApiError> {
        if report.requires_reason && reason.is_none() {
            return Err(reason_required(proposal, report));
        }
        if !(governed || report.requires_reason) {
            return self.commit_patch_set_detailed(
                loaded,
                &proposal.patch_set,
                Vec::new(),
                created_at,
                operation_id,
            );
        }
        // The sorted unique wire strings of the gating report.
        let reason_codes: BTreeSet<String> =
            report.reason_codes.iter().map(|c| c.to_string()).collect();
        let companion = build_acceptance_companion(
            &loaded.graph,
            proposal,
            &AcceptanceInput {
                reason_codes: reason_codes.into_iter().collect(),
                decided_by: self.inner.identity.actor_ref.clone(),
                decided_at: created_at,
                rationale: reason.map(str::to_owned),
            },
        )
        .map_err(companion_error)?;
        match validate_required_rationale(proposal, report, Some(&companion.decision_node)) {
            Ok(()) => {}
            Err(IntakeAcceptanceError::ReasonRequired(_))
            | Err(IntakeAcceptanceError::MissingRationale(_)) => {
                return Err(reason_required(proposal, report))
            }
            Err(e) => return Err(ApiError::internal_cause("intake_rationale", &e)),
        }
        // The original effect is persisted before the CAS; an orphan after a lost CAS is kept.
        let stored = self.with_artifact_store(|store| {
            store.put(
                ArtifactKind::Patch,
                "application/json",
                &companion.original_patch_bytes,
                created_at,
            )
        })?;
        match stored {
            Ok(hash) if hash == companion.patch_ref => {}
            Err(e) => return Err(ApiError::artifact_store(&e, "artifact_store")),
            _ => return Err(ApiError::internal("artifact_store")),
        }
        let mut decision_refs = introduced_decisions(&proposal.patch_set.patch);
        decision_refs.insert(companion.decision_ref.clone());
        self.commit_patch_set_detailed(
            loaded,
            &companion.patch_set,
            decision_refs.into_iter().collect(),
            created_at,
            operation_id,
        )
    }

    /// Commits `patch_set` with the loaded revision as expected head and broadcasts one design
    /// event. A moved head is 412 built from the reloaded head; nothing is retried or rebased.
    pub(crate) fn commit_patch_set(
        &self,
        loaded: &LoadedRevision,
        patch_set: &PatchSet,
        decision_refs: Vec<Id>,
        created_at: Timestamp,
        operation_id: &'static str,
    ) -> Result<WriteResult, ApiError> {
        self.commit_patch_set_detailed(loaded, patch_set, decision_refs, created_at, operation_id)
            .map(|(write, _)| write)
    }

    /// [`ApiState::commit_patch_set`] also returning the committed PatchSet and store result.
    pub(crate) fn commit_patch_set_detailed(
        &self,
        loaded: &LoadedRevision,
        patch_set: &PatchSet,
        decision_refs: Vec<Id>,
        created_at: Timestamp,
        operation_id: &'static str,
    ) -> Result<(WriteResult, Committed), ApiError> {
        self.commit_patch_set_with_artifacts(
            loaded,
            patch_set,
            decision_refs,
            &[],
            created_at,
            operation_id,
        )
    }

    /// [`ApiState::commit_patch_set_detailed`] storing `artifacts` in the same transaction.
    fn commit_patch_set_with_artifacts(
        &self,
        loaded: &LoadedRevision,
        patch_set: &PatchSet,
        decision_refs: Vec<Id>,
        artifacts: &[plumb_artifacts::Artifact],
        created_at: Timestamp,
        operation_id: &'static str,
    ) -> Result<(WriteResult, Committed), ApiError> {
        let meta = RevisionWriteMeta {
            decision_refs,
            created_by: self.inner.identity.actor_ref.clone(),
            created_at,
        };
        let committed = self.with_revision_store(|store| {
            store.commit_with_artifacts(
                &self.inner.branch,
                &loaded.revision.id,
                patch_set,
                meta,
                artifacts,
            )
        })?;
        let result = match committed {
            Ok(result) => result,
            Err(StoreError::StaleBase { .. }) => {
                let current = self.load_head()?;
                return Err(precondition_failed(&current.revision));
            }
            Err(StoreError::Patch(error)) => {
                return Err(ApiError::unprocessable(
                    "E_PATCH_REJECTED",
                    "the proposed change cannot be applied to the current graph",
                )
                .with_details(json!({ "patch_error": error.to_string() })))
            }
            Err(e) => return Err(ApiError::store(&e, "revision_store")),
        };
        let delta = &result.delta;
        let mut ids: Vec<String> = delta
            .added_nodes
            .iter()
            .chain(&delta.removed_nodes)
            .chain(&delta.modified_nodes)
            .chain(&delta.added_edges)
            .chain(&delta.removed_edges)
            .chain(&delta.modified_edges)
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        ids.dedup();
        let revision = result.revision.id.to_string();
        let event = DesignEvent {
            kind: operation_id.to_owned(),
            ids,
            version: revision.clone(),
        };
        // No subscriber is not an error: the event stream is forward-only.
        let _ = self.inner.events.send(event.clone());
        let write = WriteResult {
            version: revision.clone(),
            revision,
            semantic_hash: result.revision.semantic_hash.to_string(),
            events: vec![event],
        };
        Ok((
            write,
            Committed {
                patch_set: patch_set.clone(),
                commit: result,
            },
        ))
    }
}

/// X1.3 intake of `proposal` against exactly the loaded head.
pub(crate) fn run_intake(
    loaded: &LoadedRevision,
    proposal: &Proposal,
) -> Result<IntakeReport, ApiError> {
    match intake(&loaded.graph, proposal, &IntakeValidationInput::empty()) {
        Ok(report) => Ok(report),
        Err(IntakeError::Patch(error)) => Err(ApiError::unprocessable(
            "E_PATCH_REJECTED",
            "the proposed change cannot be applied to the current graph",
        )
        .with_details(json!({ "patch_error": error.to_string() }))),
        Err(IntakeError::ExpressionLimit { limit, actual, max }) => Err(ApiError::unprocessable(
            "E_EXPRESSION_LIMIT",
            "the proposal contains an expression exceeding a safety limit",
        )
        .with_details(json!({ "limit": limit, "actual": actual, "max": max }))),
        Err(e) => Err(ApiError::internal_cause("intake", &e)),
    }
}

/// 409 `E_INTAKE_REASON_REQUIRED` for `proposal`.
pub(crate) fn reason_required(proposal: &Proposal, report: &IntakeReport) -> ApiError {
    let codes: Vec<String> = report.reason_codes.iter().map(|c| c.to_string()).collect();
    ApiError::conflict(
        "E_INTAKE_REASON_REQUIRED",
        "intake requires a recorded reason before this change can be accepted",
    )
    .with_details(json!({
        "proposal_ref": proposal.id,
        "reason_codes": codes,
    }))
}

/// The API error of a failed proposal-acceptance companion build (shared by the single and the
/// set acceptance paths).
pub(crate) fn companion_error(error: ResolutionError) -> ApiError {
    match error {
        ResolutionError::ResolutionDecisionIdCollision(id) => ApiError::conflict(
            "E_ID_COLLISION",
            "the acceptance decision ID is already used",
        )
        .with_details(json!({ "id": id })),
        ResolutionError::InvalidResolutionProposal { reason } => ApiError::unprocessable(
            "E_PATCH_REJECTED",
            "the proposed change cannot be applied to the current graph",
        )
        .with_details(json!({ "patch_error": reason })),
        other => ApiError::semantic("ResolutionError", other.to_string()),
    }
}

/// The Accepted ResolutionDecisions a patch introduces (its `AddNode` leaves).
pub(crate) fn introduced_decisions(patch: &SemanticPatch) -> BTreeSet<Id> {
    match patch {
        SemanticPatch::Compound { patches } => {
            patches.iter().flat_map(introduced_decisions).collect()
        }
        SemanticPatch::AddNode { node }
            if node.status == ElementStatus::Accepted
                && matches!(node.payload, NodePayload::ResolutionDecision(_)) =>
        {
            BTreeSet::from([node.id.clone()])
        }
        _ => BTreeSet::new(),
    }
}

/// 412 for `revision`.
pub(crate) fn precondition_failed(revision: &GraphRevision) -> ApiError {
    ApiError::precondition_failed(revision.id.to_string(), revision.semantic_hash.to_string())
}

/// Whether `value` names exactly `revision` (architecture §33.2): the strong entity-tag (the
/// quoted revision ID) or the bare revision ID. Weak validators, `*`, lists and the semantic
/// hash never match.
pub(crate) fn if_match_names(value: &str, revision: &GraphRevision) -> bool {
    let tag = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value);
    tag == revision.id.as_str()
}

/// Requires `If-Match` to name the loaded head revision exactly (400 when absent, 412 otherwise).
pub(crate) fn check_if_match(headers: &HeaderMap, loaded: &LoadedRevision) -> Result<(), ApiError> {
    let value = headers
        .get("if-match")
        .ok_or_else(|| ApiError::invalid("the If-Match header is required"))?
        .to_str()
        .map_err(|_| ApiError::invalid("the If-Match header is not valid text"))?;
    if !if_match_names(value, &loaded.revision) {
        return Err(precondition_failed(&loaded.revision));
    }
    Ok(())
}

/// A JSON read response carrying `ETag` = the loaded revision's strong entity-tag.
pub(crate) fn read_json<T: Serialize>(loaded: &LoadedRevision, body: T) -> Response {
    with_etag(loaded, Json(body).into_response())
}

/// Adds `ETag` = the strong entity-tag of the loaded revision ID (`"rev:<version>:<hex>"`).
pub(crate) fn with_etag(loaded: &LoadedRevision, mut response: Response) -> Response {
    if let Ok(value) = HeaderValue::from_str(&format!("\"{}\"", loaded.revision.id)) {
        response.headers_mut().insert("etag", value);
    }
    response
}

// ============================================================================ wire vocabulary

/// The serde wire string of a closed enum value.
pub(crate) fn wire<T: Serialize>(value: &T) -> Result<String, ApiError> {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => Ok(s),
        _ => Err(ApiError::internal("wire_vocabulary")),
    }
}

/// Parses a closed enum from its exact serde wire string.
pub(crate) fn parse_wire<T: serde::de::DeserializeOwned>(
    text: &str,
    what: &str,
) -> Result<T, ApiError> {
    serde_json::from_value(Value::String(text.to_owned()))
        .map_err(|_| ApiError::invalid(format!("{what} {} is not a valid value", echo(text))))
}

/// Non-empty text without surrounding whitespace or control characters.
pub(crate) fn is_clean_text(text: &str) -> bool {
    !text.is_empty() && text.trim() == text && !text.chars().any(char::is_control)
}

/// 400 unless `reason` is clean text (non-empty, exactly trimmed, no control characters); it is
/// never normalized.
pub(crate) fn require_clean_reason(reason: &str) -> Result<(), ApiError> {
    if is_clean_text(reason) {
        Ok(())
    } else {
        Err(ApiError::invalid(
            "reason must be non-empty, without surrounding whitespace or control characters",
        ))
    }
}

/// A path or body ID; `None` when the text is not an ID.
pub(crate) fn parse_id(text: &str) -> Option<Id> {
    text.parse().ok()
}

/// Parses a JSON request body, mapping any failure to 400.
pub(crate) fn parse_body<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body)
        .map_err(|e| ApiError::invalid(format!("invalid request body: {}", echo(&e.to_string()))))
}

// ============================================================================ query and cursor

/// The parsed query string of a request: distinct keys only.
pub(crate) struct QueryParams {
    values: BTreeMap<String, String>,
}

impl QueryParams {
    /// Parses `query`, rejecting repeated keys and keys outside `allowed` (plus `cursor` and
    /// `limit` when `pageable`).
    pub(crate) fn parse(
        query: Option<&str>,
        allowed: &[&str],
        pageable: bool,
    ) -> Result<QueryParams, ApiError> {
        let pairs: Vec<(String, String)> = match query {
            None | Some("") => Vec::new(),
            Some(q) => {
                axum::extract::Query::<Vec<(String, String)>>::try_from_uri(
                    &format!("/?{q}")
                        .parse()
                        .map_err(|_| ApiError::invalid("malformed query string"))?,
                )
                .map_err(|_| ApiError::invalid("malformed query string"))?
                .0
            }
        };
        let mut values = BTreeMap::new();
        for (key, value) in pairs {
            let known = allowed.contains(&key.as_str())
                || (pageable && (key == "cursor" || key == "limit"));
            if !known {
                return Err(ApiError::invalid(format!(
                    "unknown query parameter {}",
                    echo(&key)
                )));
            }
            if values.insert(key.clone(), value).is_some() {
                return Err(ApiError::invalid(format!(
                    "repeated query parameter {}",
                    echo(&key)
                )));
            }
        }
        Ok(QueryParams { values })
    }

    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// The present non-pagination parameters: the request-hash query object.
    fn filters(&self) -> BTreeMap<&str, &str> {
        self.values
            .iter()
            .filter(|(k, _)| k.as_str() != "cursor" && k.as_str() != "limit")
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

/// Cursor v2: exactly these four fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    after_id: String,
    request_hash: Hash,
    revision: RevisionId,
    semantic_hash: Hash,
}

/// Generic SHA-256 of RFC 8785 `{operation_id, path, query}`.
fn request_hash(
    operation_id: &str,
    path: &BTreeMap<&str, &str>,
    query: &BTreeMap<&str, &str>,
) -> Result<Hash, ApiError> {
    let bytes = to_canonical_json(&json!({
        "operation_id": operation_id,
        "path": path,
        "query": query,
    }))
    .map_err(|e| ApiError::internal_cause("request_hash", &e))?;
    Ok(Hash::content_sha256(&bytes))
}

fn encode_cursor(cursor: &Cursor) -> Result<String, ApiError> {
    let bytes = to_canonical_json(cursor).map_err(|e| ApiError::internal_cause("cursor", &e))?;
    Ok(hex::encode(bytes))
}

/// Decodes a cursor, accepting only lowercase canonical encodings of the four-field object.
fn decode_cursor(text: &str) -> Result<Cursor, ApiError> {
    let invalid = || ApiError::cursor_invalid("the cursor is malformed");
    if text.is_empty()
        || !text.len().is_multiple_of(2)
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let bytes = hex::decode(text).map_err(|_| invalid())?;
    let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let canonical = to_canonical_json(&cursor).map_err(|_| invalid())?;
    if canonical != bytes
        || cursor.request_hash.kind() != HashKind::Generic
        || cursor.semantic_hash.kind() != HashKind::Semantic
        || cursor.after_id.parse::<Id>().is_err()
    {
        return Err(invalid());
    }
    Ok(cursor)
}

/// One pageable request: its operation, path parameters and query.
pub(crate) struct PageRequest<'a> {
    pub(crate) operation_id: &'static str,
    pub(crate) path: BTreeMap<&'a str, &'a str>,
    pub(crate) query: &'a QueryParams,
}

/// A page of `items` (already filtered) for `request` at `loaded`: sorted by ID, continued
/// exclusively after the cursor's `after_id`, then limited.
pub(crate) fn paginate<T>(
    mut items: Vec<(String, T)>,
    request: &PageRequest<'_>,
    loaded: &LoadedRevision,
) -> Result<(Vec<T>, Option<String>), ApiError> {
    items.sort_by(|a, b| a.0.cmp(&b.0));
    paginate_ordered(items, request, loaded)
}

/// One item of a robust collection (H0.11): its projection, or the health marker of its failure.
pub(crate) type Projected<T> = Result<T, ItemHealth>;

/// The health marker of the item `id` whose own projection failed with `error`. A failure of a
/// subsystem rather than of the item's persisted material fails the whole read.
pub(crate) fn health_marker(id: &str, error: ApiError) -> Result<ItemHealth, ApiError> {
    let body = error.body();
    if matches!(body.code, "E_INTERNAL" | "E_A03_RUNTIME_UNAVAILABLE") {
        return Err(error);
    }
    Ok(ItemHealth {
        id: id.to_owned(),
        code: body.code.to_owned(),
        message: body.message.clone(),
    })
}

/// The projection of item `id`, or its health marker.
pub(crate) fn project<T>(id: &str, view: Result<T, ApiError>) -> Result<Projected<T>, ApiError> {
    match view {
        Ok(view) => Ok(Ok(view)),
        Err(error) => health_marker(id, error).map(Err),
    }
}

/// [`paginate`] over projected items: the page's healthy items and its health markers.
#[allow(clippy::type_complexity)]
pub(crate) fn paginate_projected<T>(
    items: Vec<(String, Projected<T>)>,
    request: &PageRequest<'_>,
    loaded: &LoadedRevision,
) -> Result<(Vec<T>, Vec<ItemHealth>, Option<String>), ApiError> {
    let (page, next_cursor) = paginate(items, request, loaded)?;
    let mut healthy = Vec::new();
    let mut health = Vec::new();
    for item in page {
        match item {
            Ok(view) => healthy.push(view),
            Err(marker) => health.push(marker),
        }
    }
    Ok((healthy, health, next_cursor))
}

/// A page of `items` in their given (ranked) order: continued exclusively after the cursor's
/// `after_id` within exactly this sequence, then limited.
pub(crate) fn paginate_ordered<T>(
    items: Vec<(String, T)>,
    request: &PageRequest<'_>,
    loaded: &LoadedRevision,
) -> Result<(Vec<T>, Option<String>), ApiError> {
    let limit = match request.query.get("limit") {
        None => None,
        Some(text) => match text.parse::<u64>() {
            Ok(n) if n >= 1 => Some(usize::try_from(n).unwrap_or(usize::MAX)),
            _ => return Err(ApiError::invalid("limit must be an integer of at least 1")),
        },
    };
    let request_hash = request_hash(
        request.operation_id,
        &request.path,
        &request.query.filters(),
    )?;
    let mut start = 0;
    if let Some(text) = request.query.get("cursor") {
        let cursor = decode_cursor(text)?;
        if cursor.request_hash != request_hash {
            return Err(ApiError::cursor_request_mismatch());
        }
        if cursor.revision != loaded.revision.id
            || cursor.semantic_hash != loaded.revision.semantic_hash
        {
            return Err(ApiError::cursor_baseline_changed());
        }
        let position = items
            .iter()
            .position(|(id, _)| *id == cursor.after_id)
            .ok_or_else(|| {
                ApiError::cursor_invalid("the cursor's after_id is not in this collection")
            })?;
        start = position + 1;
    }
    let remaining = items.len() - start;
    let take = limit.map_or(remaining, |l| l.min(remaining));
    let end = start + take;
    let next_cursor = if end < items.len() && take > 0 {
        Some(encode_cursor(&Cursor {
            after_id: items[end - 1].0.clone(),
            request_hash,
            revision: loaded.revision.id.clone(),
            semantic_hash: loaded.revision.semantic_hash.clone(),
        })?)
    } else {
        None
    };
    let page = items
        .into_iter()
        .skip(start)
        .take(take)
        .map(|(_, item)| item)
        .collect();
    Ok((page, next_cursor))
}
