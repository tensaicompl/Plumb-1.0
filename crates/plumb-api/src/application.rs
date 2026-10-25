//! The application construction layer (A0.4, Hotfix 067): opens every store over one pilot
//! database and builds the full `ApiState`, so the CLI and the server share one persistence
//! setup and never reproduce it. It also initializes an empty project database and bootstraps a
//! new project database for the CLI `init` command (R0.1).

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use plumb_artifacts::SqliteArtifactStore;
use plumb_core::{Clock, Hash, Id, Timestamp};
use plumb_functional::{
    AuthorizationDraft, CalculationScope, CalendarProvider, DirectoryDraft, DuplicatePolicy,
    OperationScope, PredicateProvider, ProcessScope, ScenarioDerivationInputs,
    StakeholderRoutingConfig, VerificationApplicabilityInputs, VocabularyPolicy,
};
use plumb_inference::{InferenceProvider, ProviderPolicy};
use plumb_psg::{Agent, AgentKind, AuditMeta, ElementStatus, Graph, Node, NodePayload};
use plumb_session::SessionTurnConfig;
use plumb_sim::{
    ExecutionCalculationScopeOverride, ExecutionExpressionScopeInput, RuntimeSemanticValue,
};
use plumb_store::{
    BranchName, RevisionId, RevisionWriteMeta, SqliteRevisionStore, SqliteStageStore,
    SqliteWorkflowStore, StoreError, StoreOpenMode,
};
use plumb_validation::{ValidationPolicy, ValidationProfile};
use serde::Serialize;
use thiserror::Error;

use crate::runtime::{
    A04RuntimeConfig, CalendarConfig, DirectManagerFact, GateInputsConfig, ProviderMode,
};
use crate::selector::{Selectable, SelectorSchema};
use crate::state::{A03RuntimeConfig, ApiIdentity, ApiState, ApiStateError, OpenMode};

/// Why the application cannot be opened or initialized.
#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    State(#[from] ApiStateError),
    #[error("artifact store: {0}")]
    Artifacts(String),
    #[error("profile: {0}")]
    Profile(String),
    #[error("graph: {0}")]
    Graph(String),
    /// R0.1: the project cannot be bootstrapped (existing path or invalid configured actors).
    #[error("bootstrap: {0}")]
    Bootstrap(String),
    /// The requested branch has no head in this database.
    #[error("branch {0} does not exist in this database (only main exists; branches cannot be created yet)")]
    UnknownBranch(String),
    /// A runtime policy a later stage or session would use is invalid (architecture §33.9):
    /// rejected when the application runtime is built, before any inference.
    #[error("invalid runtime configuration: {field}: {reason}")]
    InvalidRuntime { field: &'static str, reason: String },
}

impl ApplicationError {
    /// The stable error code of this failure, when it has one (architecture §33.9).
    pub fn code(&self) -> Option<&'static str> {
        match self {
            ApplicationError::Store(error) => {
                crate::error::ApiError::store_class(error.class()).map(|e| e.body().code)
            }
            ApplicationError::UnknownBranch(_) => Some("E_BRANCH_NOT_FOUND"),
            ApplicationError::InvalidRuntime { .. } => Some("E_RUNTIME_CONFIG_INVALID"),
            ApplicationError::Artifacts(_) => None,
            // The stage start reports its typed API error as `CODE: message`.
            ApplicationError::State(ApiStateError::StageRuntime(message)) => {
                STATE_CODES.iter().copied().find(|code| {
                    message
                        .strip_prefix(code)
                        .is_some_and(|rest| rest.starts_with(':'))
                })
            }
            ApplicationError::State(ApiStateError::DuplicateExpressionScope(_)) => {
                Some("E_RUNTIME_CONFIG_INVALID")
            }
            _ => None,
        }
    }
}

/// The typed store codes a stage start can report (see [`ApplicationError::code`]).
const STATE_CODES: [&str; 6] = [
    "E_BRANCH_NOT_FOUND",
    "E_DATABASE_NOT_INITIALIZED",
    "E_DATABASE_BUSY",
    "E_DATABASE_READ_ONLY",
    "E_STORE_CORRUPT",
    "E_STORE_VERSION_UNSUPPORTED",
];

/// The typed runtime material of one application instance: the A0.3 runtime inputs and the
/// A0.4 stage inputs, each already validated by its loader.
pub struct ApplicationRuntime {
    pub inference_provider: Arc<dyn InferenceProvider + Send + Sync>,
    pub provider_mode: ProviderMode,
    pub provider_policy: ProviderPolicy,
    pub session_turn_config: SessionTurnConfig,
    pub assistant_actor_ref: Id,
    pub stage_actor_ref: Id,
    pub calendars: Arc<dyn CalendarProvider + Send + Sync>,
    pub predicates: Arc<dyn PredicateProvider + Send + Sync>,
    pub calendar_configs: Vec<CalendarConfig>,
    pub direct_managers: Vec<DirectManagerFact>,
    /// The A0.3 entry lists; an entry may name its elements by selector (`crate::selector`).
    pub expression_scopes: Vec<Selectable<ExecutionExpressionScopeInput>>,
    pub calculation_scope_overrides: Vec<Selectable<ExecutionCalculationScopeOverride>>,
    pub runtime_semantic_values: Vec<Selectable<RuntimeSemanticValue>>,
    pub validation_policy: ValidationPolicy,
    pub vocabulary_policy: VocabularyPolicy,
    pub duplicate_policy: DuplicatePolicy,
    /// The stage inputs, each in its explicit form or naming elements by selector, resolved
    /// against the graph each stage runs on.
    pub calculation_scope: Option<Selectable<CalculationScope>>,
    pub operation_scope: Option<Selectable<OperationScope>>,
    pub process_scope: Option<Selectable<ProcessScope>>,
    pub authorization_draft: Option<Selectable<AuthorizationDraft>>,
    pub scenario_derivation: Option<Selectable<ScenarioDerivationInputs>>,
    pub verification_applicability: Option<Selectable<VerificationApplicabilityInputs>>,
    pub gates: GateInputsConfig,
    /// The configured semantic directory (R0.6); `None` when not configured.
    pub directory: Option<DirectoryDraft>,
    /// The S3.1 question routing (R0.6); `None` when not configured.
    pub question_routing: Option<StakeholderRoutingConfig>,
}

/// Opens the revision, artifact, workflow and stage stores over `db` and builds the full A0.4
/// state for `branch` (startup-injected gate reports: none; durable reports are current), in
/// the `Interactive` mode.
pub fn open_application(
    db: &Path,
    branch: BranchName,
    identity: ApiIdentity,
    clock: Arc<dyn Clock + Send + Sync>,
    profile: ValidationProfile,
    runtime: ApplicationRuntime,
) -> Result<ApiState, ApplicationError> {
    open_application_in(
        OpenMode::Interactive,
        db,
        branch,
        identity,
        clock,
        profile,
        runtime,
    )
}

/// Every runtime policy a later stage or session uses is valid (architecture §33.9), and the
/// identity, stage and assistant actors are pairwise distinct. Checked before any store opens.
pub fn validate_runtime(
    identity: &ApiIdentity,
    runtime: &ApplicationRuntime,
) -> Result<(), ApplicationError> {
    let invalid =
        |field: &'static str, reason: String| ApplicationError::InvalidRuntime { field, reason };
    let actors: BTreeSet<&Id> = [
        identity.actor_ref(),
        &runtime.stage_actor_ref,
        &runtime.assistant_actor_ref,
    ]
    .into_iter()
    .collect();
    if actors.len() != 3 {
        return Err(invalid(
            "identity",
            "identity.actor_ref, stage_actor_ref and assistant_actor_ref must be pairwise \
             distinct"
                .to_owned(),
        ));
    }
    runtime
        .duplicate_policy
        .validate()
        .map_err(|e| invalid("duplicate_policy", e.to_string()))?;
    runtime
        .vocabulary_policy
        .validate()
        .map_err(|e| invalid("vocabulary_policy", e.to_string()))?;
    runtime
        .session_turn_config
        .validate()
        .map_err(|e| invalid("session_turn", e.to_string()))?;
    runtime
        .provider_policy
        .validate()
        .map_err(|e| invalid("provider_policy", e.to_string()))?;
    let mut calendars = BTreeSet::new();
    for calendar in &runtime.calendar_configs {
        calendar
            .validate()
            .map_err(|e| invalid("calendars", e.to_string()))?;
        if !calendars.insert(&calendar.calendar_ref) {
            return Err(invalid(
                "calendars",
                format!("calendar {} is defined twice", calendar.calendar_ref),
            ));
        }
    }
    crate::runtime::DirectManagerPredicates::new(&runtime.direct_managers)
        .map_err(|e| invalid("direct_managers", e.to_string()))?;
    validate_selectors(runtime)?;
    // The explicit forms are checked here; a selector form is checked once resolved.
    let mut owners = BTreeSet::new();
    for scope in runtime
        .expression_scopes
        .iter()
        .filter_map(Selectable::explicit)
    {
        if !owners.insert(&scope.owner_ref) {
            return Err(invalid(
                "expression_scopes",
                format!("owner {} is configured more than once", scope.owner_ref),
            ));
        }
    }
    if let Some(inputs) = runtime
        .scenario_derivation
        .as_ref()
        .and_then(Selectable::explicit)
    {
        let tables: Vec<&Id> = inputs
            .decision_tables
            .iter()
            .map(|t| &t.decision_table_ref)
            .collect();
        if tables.windows(2).any(|w| w[0] >= w[1]) {
            return Err(invalid(
                "scenario_derivation",
                "decision-table inputs are unsorted or duplicated".to_owned(),
            ));
        }
        let boundaries: Vec<(&String, &Id)> = inputs
            .boundaries
            .iter()
            .map(|b| (&b.boundary_key, &b.subject_ref))
            .collect();
        if boundaries.windows(2).any(|w| w[0] >= w[1]) {
            return Err(invalid(
                "scenario_derivation",
                "boundary inputs are unsorted or duplicated".to_owned(),
            ));
        }
    }
    if let Some(inputs) = runtime
        .verification_applicability
        .as_ref()
        .and_then(Selectable::explicit)
    {
        let mut sources = BTreeSet::new();
        for source in &inputs.sources {
            if !sources.insert(&source.source_ref) {
                return Err(invalid(
                    "verification_applicability",
                    format!("source {} is listed more than once", source.source_ref),
                ));
            }
        }
    }
    Ok(())
}

/// Every selector-form stage value is well formed, selects only node types its position
/// admits and has the shape of its typed value (`E_RUNTIME_CONFIG_INVALID` naming the field).
fn validate_selectors(runtime: &ApplicationRuntime) -> Result<(), ApplicationError> {
    fn check<T: SelectorSchema>(
        field: &'static str,
        value: Option<&Selectable<T>>,
    ) -> Result<(), ApplicationError> {
        match value.map(Selectable::validate) {
            Some(Err(e)) => Err(ApplicationError::InvalidRuntime {
                field,
                reason: e.to_string(),
            }),
            _ => Ok(()),
        }
    }
    fn entries<T: SelectorSchema>(
        field: &'static str,
        values: &[Selectable<T>],
    ) -> Result<(), ApplicationError> {
        values.iter().try_for_each(|v| check(field, Some(v)))
    }
    entries("expression_scopes", &runtime.expression_scopes)?;
    entries(
        "calculation_scope_overrides",
        &runtime.calculation_scope_overrides,
    )?;
    entries("runtime_semantic_values", &runtime.runtime_semantic_values)?;
    check("calculation_scope", runtime.calculation_scope.as_ref())?;
    check("operation_scope", runtime.operation_scope.as_ref())?;
    check("process_scope", runtime.process_scope.as_ref())?;
    check("authorization_draft", runtime.authorization_draft.as_ref())?;
    check("scenario_derivation", runtime.scenario_derivation.as_ref())?;
    check(
        "verification_applicability",
        runtime.verification_applicability.as_ref(),
    )?;
    check("gates.f4", runtime.gates.f4.as_ref())
}

/// [`open_application`] in `mode` (architecture §33.9): the runtime is validated first; every
/// store then opens the existing database without ever creating a schema (`ReadOnly`: SQLite
/// read-only access, no journal-mode change, no worker, recovery or reconciliation).
pub fn open_application_in(
    mode: OpenMode,
    db: &Path,
    branch: BranchName,
    identity: ApiIdentity,
    clock: Arc<dyn Clock + Send + Sync>,
    profile: ValidationProfile,
    runtime: ApplicationRuntime,
) -> Result<ApiState, ApplicationError> {
    validate_runtime(&identity, &runtime)?;
    let store_mode = match mode {
        OpenMode::Interactive => StoreOpenMode::Existing,
        OpenMode::ReadOnly => StoreOpenMode::ReadOnly,
    };
    let revision_store = SqliteRevisionStore::open_with(db, store_mode)?;
    if revision_store.head(&branch)?.is_none() {
        return Err(ApplicationError::UnknownBranch(branch.to_string()));
    }
    let artifact_store = match mode {
        OpenMode::Interactive => SqliteArtifactStore::open_existing(db),
        OpenMode::ReadOnly => SqliteArtifactStore::open_read_only(db),
    }
    .map_err(|e| match e {
        plumb_artifacts::ArtifactStoreError::NotInitialized => {
            ApplicationError::Store(StoreError::DatabaseNotInitialized)
        }
        other => ApplicationError::Artifacts(other.to_string()),
    })?;
    let workflow_store = SqliteWorkflowStore::open_with(db, store_mode)?;
    let stage_store = SqliteStageStore::open_with(db, store_mode)?;
    let a03 = A03RuntimeConfig {
        workflow_store,
        inference_provider: runtime.inference_provider,
        provider_policy: runtime.provider_policy,
        session_turn_config: runtime.session_turn_config,
        assistant_actor_ref: runtime.assistant_actor_ref,
        calendars: runtime.calendars,
        predicates: runtime.predicates,
        expression_scopes: runtime.expression_scopes,
        calculation_scope_overrides: runtime.calculation_scope_overrides,
        runtime_semantic_values: runtime.runtime_semantic_values,
    };
    let a04 = A04RuntimeConfig {
        stage_store,
        stage_actor_ref: runtime.stage_actor_ref,
        provider_mode: runtime.provider_mode,
        validation_policy: runtime.validation_policy,
        vocabulary_policy: runtime.vocabulary_policy,
        duplicate_policy: runtime.duplicate_policy,
        calendars: runtime.calendar_configs,
        direct_managers: runtime.direct_managers,
        calculation_scope: runtime.calculation_scope,
        operation_scope: runtime.operation_scope,
        process_scope: runtime.process_scope,
        authorization_draft: runtime.authorization_draft,
        scenario_derivation: runtime.scenario_derivation,
        verification_applicability: runtime.verification_applicability,
        gates: runtime.gates,
        directory: runtime.directory,
        question_routing: runtime.question_routing,
    };
    Ok(ApiState::new_with_stage_runtime_in(
        mode,
        branch,
        revision_store,
        artifact_store,
        identity,
        clock,
        Vec::new(),
        profile,
        a03,
        a04,
    )?)
}

/// Creates the initial revision of an empty project graph in a fresh database at `db`.
pub fn initialize_project_database(
    db: &Path,
    project_id: Id,
    profile: &ValidationProfile,
    created_by: Id,
    created_at: Timestamp,
) -> Result<RevisionId, ApplicationError> {
    let graph = Graph::new(
        project_id,
        profile.profile_id.clone(),
        Vec::new(),
        Vec::new(),
    )
    .map_err(|e| ApplicationError::Graph(format!("{e:?}")))?;
    let mut store = SqliteRevisionStore::open(db)?;
    let revision = store.create_initial_revision(
        &graph,
        profile
            .profile_hash()
            .map_err(|e| ApplicationError::Profile(e.to_string()))?,
        profile
            .rule_pack_hash()
            .map_err(|e| ApplicationError::Profile(e.to_string()))?,
        RevisionWriteMeta {
            decision_refs: Vec::new(),
            created_by,
            created_at,
        },
    )?;
    Ok(revision.id)
}

/// The result of bootstrapping a new project database (R0.1): exactly what `init` reports.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectBootstrap {
    pub project_id: Id,
    pub branch: BranchName,
    pub revision_id: RevisionId,
    pub semantic_hash: Hash,
    pub evidence_hash: Hash,
}

/// The SQLite sidecar paths of `db` (rollback journal and WAL files).
fn sqlite_sidecars(db: &Path) -> [PathBuf; 3] {
    ["-journal", "-wal", "-shm"].map(|suffix| {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    })
}

/// The three configured provenance Agents of a new project, all Accepted at revision 1 with
/// no evidence, derivations, standards, tags or extensions, created by `created_by` at `at`.
fn bootstrap_agents(
    identity_actor_ref: &Id,
    stage_actor_ref: &Id,
    assistant_actor_ref: &Id,
    created_by: &Id,
    at: Timestamp,
) -> Result<Vec<Node>, ApplicationError> {
    let actors = [
        (identity_actor_ref, AgentKind::Human),
        (stage_actor_ref, AgentKind::CompilerStage),
        (assistant_actor_ref, AgentKind::LlmModel),
    ];
    actors
        .into_iter()
        .map(|(id, agent_kind)| {
            let audit = AuditMeta::new(created_by.clone(), at, None, None)
                .map_err(|e| ApplicationError::Graph(format!("{e:?}")))?;
            Ok(Node {
                id: id.clone(),
                revision: 1,
                status: ElementStatus::Accepted,
                payload: NodePayload::Agent(Agent { agent_kind }),
                evidence: Vec::new(),
                derivations: Vec::new(),
                standards: Vec::new(),
                tags: BTreeSet::new(),
                extensions: Default::default(),
                audit,
            })
        })
        .collect()
}

/// Bootstraps a new project database at `db` (R0.1, architecture §21 "Project bootstrap").
///
/// The three actor IDs must be pairwise distinct; this is checked before anything is created.
/// The path is reserved with an exclusive create, so an existing path (any file, directory or
/// SQLite sidecar) is refused without being opened or changed. The genesis revision is written
/// through `SqliteRevisionStore::create_initial_revision` on branch `main` with the profile's
/// ID and hashes; its graph holds exactly the three configured Agents. The clock is read once
/// for the Agent audits and the revision. When any step after the reservation fails, the file
/// and the sidecars this attempt created are removed. `initialize_project_database` is
/// unchanged.
pub fn bootstrap_project_database(
    db: &Path,
    project_id: Id,
    profile: &ValidationProfile,
    identity: &ApiIdentity,
    stage_actor_ref: &Id,
    assistant_actor_ref: &Id,
    clock: &dyn Clock,
) -> Result<ProjectBootstrap, ApplicationError> {
    let identity_actor_ref = identity.actor_ref();
    let distinct: BTreeSet<&Id> = [identity_actor_ref, stage_actor_ref, assistant_actor_ref]
        .into_iter()
        .collect();
    if distinct.len() != 3 {
        return Err(ApplicationError::Bootstrap(
            "identity.actor_ref, stage_actor_ref and assistant_actor_ref must be pairwise distinct"
                .into(),
        ));
    }
    let sidecars = sqlite_sidecars(db);
    if let Some(existing) = sidecars.iter().find(|p| p.symlink_metadata().is_ok()) {
        return Err(ApplicationError::Bootstrap(format!(
            "{} already exists",
            existing.display()
        )));
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(db)
        .map_err(|e| ApplicationError::Bootstrap(format!("cannot create {}: {e}", db.display())))?;
    let at = clock.now();
    let created_by = identity_actor_ref.clone();
    let written = (|| {
        let nodes = bootstrap_agents(
            identity_actor_ref,
            stage_actor_ref,
            assistant_actor_ref,
            &created_by,
            at,
        )?;
        let graph = Graph::new(project_id, profile.profile_id.clone(), nodes, Vec::new())
            .map_err(|e| ApplicationError::Graph(format!("{e:?}")))?;
        let mut store = SqliteRevisionStore::open(db)?;
        let revision = store.create_initial_revision(
            &graph,
            profile
                .profile_hash()
                .map_err(|e| ApplicationError::Profile(e.to_string()))?,
            profile
                .rule_pack_hash()
                .map_err(|e| ApplicationError::Profile(e.to_string()))?,
            RevisionWriteMeta {
                decision_refs: Vec::new(),
                created_by,
                created_at: at,
            },
        )?;
        Ok(ProjectBootstrap {
            project_id: revision.project_id,
            branch: BranchName::main(),
            revision_id: revision.id,
            semantic_hash: revision.semantic_hash,
            evidence_hash: revision.evidence_hash,
        })
    })();
    if written.is_err() {
        // Only this attempt's material: the reserved file and sidecars absent before it.
        let _ = std::fs::remove_file(db);
        for sidecar in &sidecars {
            let _ = std::fs::remove_file(sidecar);
        }
    }
    written
}

// ============================================================================ shared services

/// The transport-neutral application services shared by HTTP and the CLI (Hotfix 069). HTTP
/// handlers parse transport (path, query, cursor, If-Match, multipart, body), call these and
/// render the existing responses; the CLI calls them directly. Each takes the baseline the
/// caller loaded (and, for HTTP, checked against If-Match); writes never retry or rebase.
mod services {
    use plumb_artifacts::ArtifactStore;
    use plumb_core::{GateId, Id, StageId};
    use plumb_functional::resolution::{
        resolve_question, ResolutionContext, ResolutionError, ResolutionInput,
    };
    use plumb_functional::{
        compose_question_round_for_stakeholder, generate_questions, parse_priority,
        project_functional_pilot, round_id, FunctionalPilotOutputs, ProjectionArtifact,
        QuestionAudit, QuestionGenerationInput, QuestionGenerationResult, QuestionRoundConfig,
        QuestionRoundDisposition, RoundError, MAX_QUESTION_ROUND_SIZE,
    };
    use plumb_functional::{
        produce_directory, propose_scenario_confirmation, DirectoryAudit, DirectoryCalendarBinding,
        DirectoryDisposition,
    };
    use plumb_import::{
        import_docx, import_markdown, import_plain_text, ImportAudit, ImportedSource,
    };
    use plumb_patch::{
        earliest_gate, AcceptancePolicy, ElementPrecondition, PatchSet, Proposal,
        ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        contributes_to_semantic_hash, edge_element_hash, is_baseline, node_element_hash, AuditMeta,
        ConceptKind, ElementStatus, FindingSeverity, Graph, Node, NodePayload, NodeType,
    };
    use plumb_store::{
        LoadedRevision, NewWorkflowProposal, StoreError, WorkflowInsert, WorkflowOrigin,
        WorkflowProposal, WorkflowProposalState,
    };
    use plumb_validation::{GeneratedFinding, ValidationPolicy};
    use serde::Serialize;
    use serde_json::{json, Value};

    use crate::error::{echo, ApiError};
    use crate::openapi::{
        AnswerResponse, CreateRoundResponse, ExportFormat, GlossaryTerm, ProposalKind, Question,
        QuestionKind, QuestionStatus, Round, SourceUploadResponse, WriteResult,
    };
    use crate::stage::acceptance as stage_acceptance;
    use crate::stage::recipe::Phase;
    use crate::stage::scenario_execution::error_name;
    use crate::state::{
        companion_error, introduced_decisions, parse_id, parse_wire, reason_required,
        require_clean_reason, run_intake, wire, ApiState,
    };
    use crate::workflow::{
        acceptance_decisions, classify, decode_proposal, intake_view, require_kind, store_kind,
        stored_json, StoredProposal,
    };

    const MARKDOWN: &str = "text/markdown";
    const PLAIN_TEXT: &str = "text/plain";
    const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

    type Importer =
        fn(&str, &[u8], &ImportAudit) -> Result<ImportedSource, plumb_import::ImportError>;

    /// The exact importer of a declared media type, or 415.
    fn importer(media_type: &str) -> Result<Importer, ApiError> {
        match media_type {
            MARKDOWN => Ok(import_markdown),
            PLAIN_TEXT => Ok(import_plain_text),
            DOCX => Ok(import_docx),
            other => Err(ApiError::unsupported_media_type(other)),
        }
    }

    /// 415 unless `media_type` has an importer (checked by HTTP before If-Match).
    pub fn require_importable(media_type: &str) -> Result<(), ApiError> {
        importer(media_type).map(|_| ())
    }

    /// The validated round size `max` (1..=1000; no default), or 400.
    pub fn round_size(max: u64) -> Result<usize, ApiError> {
        usize::try_from(max)
            .ok()
            .filter(|n| (1..=MAX_QUESTION_ROUND_SIZE).contains(n))
            .ok_or_else(|| {
                ApiError::invalid(format!("max must be within 1..={MAX_QUESTION_ROUND_SIZE}"))
            })
    }

    /// The artifact of the pilot projection that `format` selects (HTTP and CLI `export`).
    pub fn export_artifact(
        outputs: FunctionalPilotOutputs,
        format: ExportFormat,
    ) -> ProjectionArtifact {
        match format {
            ExportFormat::Functional => outputs.functional_yaml,
            ExportFormat::Report => outputs.model_report_md,
            ExportFormat::Decisions => outputs.decisions_jsonl,
            ExportFormat::Glossary => outputs.glossary_md,
            ExportFormat::Scenarios => outputs.scenarios_yaml,
            ExportFormat::GateReport => outputs.gate_report_json,
        }
    }

    /// The application vocabulary of the six proposal actions.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ProposalAction {
        Accept,
        Create,
        Link,
        Merge,
        Supersede,
        Reject,
    }

    impl ProposalAction {
        /// The operation ID of the matching HTTP endpoint (design-event kind).
        pub fn operation_id(self) -> &'static str {
            match self {
                ProposalAction::Accept => "accept_proposal",
                ProposalAction::Create => "create_proposal",
                ProposalAction::Link => "link_proposal",
                ProposalAction::Merge => "merge_proposal",
                ProposalAction::Supersede => "supersede_proposal",
                ProposalAction::Reject => "reject_proposal",
            }
        }

        fn kind(self) -> Option<ProposalKind> {
            match self {
                ProposalAction::Accept => Some(ProposalKind::Accept),
                ProposalAction::Create => Some(ProposalKind::Create),
                ProposalAction::Link => Some(ProposalKind::Link),
                ProposalAction::Merge => Some(ProposalKind::Merge),
                ProposalAction::Supersede => Some(ProposalKind::Supersede),
                ProposalAction::Reject => None,
            }
        }
    }

    /// What one question materialisation produced (Hotfix 083): the exact S3.1 generation
    /// result and, when its non-semantic proposal was committed, the write.
    #[derive(Debug, Clone, PartialEq)]
    pub struct MaterializedQuestions {
        pub generation: QuestionGenerationResult,
        /// `None` when nothing was missing: no revision, no event, no write.
        pub write: Option<WriteResult>,
    }

    /// Maps a workflow-store failure: a corrupt record is 500 `E_WORKFLOW_CORRUPT`, a state
    /// conflict is 409 `E_PROPOSAL_NOT_PENDING`, anything else 500 `E_INTERNAL`.
    pub(crate) fn workflow_error(error: StoreError) -> ApiError {
        match error {
            StoreError::CorruptWorkflowRecord { kind, id, .. } => {
                ApiError::workflow_corrupt(&format!("{kind} {id}"))
            }
            StoreError::WorkflowStateConflict { id, actual, .. } => {
                ApiError::conflict("E_PROPOSAL_NOT_PENDING", "the proposal is not pending")
                    .with_details(json!({ "proposal": id, "state": actual }))
            }
            // Fable #1: an acceptance must resolve to a descendant of the proposal's base.
            StoreError::WorkflowResolutionNotDescendant {
                id,
                revision,
                reason,
            } => ApiError::conflict(
                "E_PROPOSAL_RESOLUTION_INVALID",
                "the resolution revision does not descend from the proposal's base",
            )
            .with_details(json!({ "proposal": id, "revision": revision, "reason": reason })),
            other => ApiError::store(&other, "workflow_store"),
        }
    }

    /// 409 `E_PROPOSAL_NOT_PENDING`.
    pub(crate) fn not_pending(record: &WorkflowProposal) -> ApiError {
        ApiError::conflict("E_PROPOSAL_NOT_PENDING", "the proposal is not pending")
            .with_details(json!({ "proposal": record.proposal_id, "state": record.state.as_str() }))
    }

    /// 409 `E_PROPOSAL_BASELINE_CHANGED`.
    pub(crate) fn baseline_changed(proposal: &Id) -> ApiError {
        ApiError::conflict(
            "E_PROPOSAL_BASELINE_CHANGED",
            "the proposal was built on another revision; it is never rebased",
        )
        .with_details(json!({ "proposal_ref": proposal }))
    }

    /// Loads and decodes the occurrence of workflow proposal `id` that a lookup on `loaded`
    /// addresses (404 when unknown; H0.10): its pending occurrence on the loaded head, else its
    /// newest occurrence (not pending, or built on another baseline). It then reconciles an
    /// interrupted acceptance: a pending occurrence on an older base whose acceptance decision is
    /// already in the graph becomes accepted (with its stage child and parent, A0.4); several
    /// such decisions are corrupt workflow state.
    pub(crate) fn load_reconciled(
        state: &ApiState,
        loaded: &LoadedRevision,
        id: &str,
    ) -> Result<StoredProposal, ApiError> {
        let missing = || ApiError::not_found(format!("proposal {id} not found"));
        let proposal_id = parse_id(id).ok_or_else(missing)?;
        let record = state
            .with_workflow_store(|store| store.current_proposal(&proposal_id, &loaded.revision.id))?
            .map_err(workflow_error)?
            .ok_or_else(missing)?;
        let stored = decode_proposal(record)?;
        // An acceptance always moves the head, so a pending occurrence on the head itself has
        // no committed acceptance to reconcile.
        if stored.record.state != WorkflowProposalState::Pending
            || stored.record.base_revision_id == loaded.revision.id
        {
            return Ok(stored);
        }
        let base = stored.record.base_revision_id.clone();
        let decisions = acceptance_decisions(&loaded.graph, &proposal_id);
        match decisions.as_slice() {
            [] => Ok(stored),
            [(decision_ref, decided_at)] => {
                // Fable #196: resolved with the revision that committed the decision.
                let accepted =
                    stage_acceptance::decision_revision(state, &loaded.revision, decision_ref)?;
                let repaired = state
                    .with_workflow_store(|store| {
                        store.resolve_proposal(
                            &proposal_id,
                            &base,
                            WorkflowProposalState::Accepted,
                            &accepted,
                            *decided_at,
                        )
                    })?
                    .map_err(workflow_error)?;
                if let Some(link) = stage_acceptance::link_of(state, &proposal_id, &base)? {
                    stage_acceptance::reconcile(state, loaded, &link)?;
                }
                decode_proposal(repaired)
            }
            _ => Err(ApiError::workflow_corrupt(&format!(
                "proposal {proposal_id}"
            ))),
        }
    }

    /// One element of the configured directory service (R0.6).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct DirectoryElementView {
        pub element_ref: String,
        pub node_type: String,
        /// `proposed` (a pending proposal was persisted) or `existing`.
        pub disposition: String,
        pub proposal_id: Option<String>,
        /// The positive action the pending proposal requires.
        pub action: Option<String>,
        pub requires_reason: Option<bool>,
    }

    /// One pending workflow proposal of the proposal discovery (R0.6).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct PendingProposalView {
        pub proposal_id: String,
        /// The positive action it requires: accept, create, link, merge or supersede.
        pub action: String,
        /// The workflow origin: intake, question, session or stage.
        pub origin: String,
        pub origin_ref: Option<String>,
        /// The stage recipe job and stage job of a stage-origin proposal.
        pub recipe_job_id: Option<String>,
        pub stage_job_id: Option<String>,
        pub base_revision: String,
        /// Whether its stored X1.3 intake requires a reason.
        pub requires_reason: bool,
        /// Whether it was built on another revision than the current head: it can only be
        /// rejected, or rebuilt (a stage rerun, or `refresh_stale_answers` for answers).
        pub stale: bool,
    }

    /// One lint diagnostic of an Accepted Requirement (R0.7): observable, and blocking only
    /// through F1 QUALITY_FINDINGS_CLEAR at error severity.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct LintDiagnosticView {
        pub requirement_ref: String,
        pub source_identifier: Option<String>,
        pub rule_id: String,
        pub severity: String,
        pub message: String,
    }

    /// One current gate finding with its H0.6 disposition and governance state.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct FindingView {
        pub id: String,
        pub gate: String,
        pub code: String,
        pub severity: String,
        pub message: String,
        pub affected_refs: Vec<String>,
        /// `question_resolvable`, `waivable` or `non_waivable` (architecture §33.6).
        pub disposition: String,
        /// Whether recorded governance (an answered Question or a waiver) covers it.
        pub governed: bool,
        /// Whether it exists as a durable Finding node.
        pub materialized: bool,
    }

    /// One stale answer proposal and its new proposal ID, or why it was not re-submitted.
    pub type AnswerRefresh = (String, Result<String, String>);

    /// The question-answering ResolutionDecision a proposal adds, if any.
    fn answer_decision(patch: &SemanticPatch) -> Option<&plumb_psg::ResolutionDecision> {
        match patch {
            SemanticPatch::AddNode { node } => match &node.payload {
                NodePayload::ResolutionDecision(d) if d.question_ref.is_some() => Some(d),
                _ => None,
            },
            SemanticPatch::Compound { patches } => patches.iter().find_map(answer_decision),
            _ => None,
        }
    }

    const GOVERNANCE: &str = "governance_type";
    const PROVENANCE_OR_PROOF: &str = "provenance_or_proof_type";
    const DEDICATED: &str = "dedicated_confirmation";

    /// Why the generic confirmation refuses a node type (architecture §2): every type excluded
    /// from `semantic_hash` (provenance, workflow and proof outputs), the governance types
    /// ResolutionDecision and Assumption, and the dedicated Term, Calculation and Scenario
    /// contracts. Requirement and every other semantic type are eligible.
    fn confirmation_refusal(node_type: NodeType) -> Option<&'static str> {
        if !contributes_to_semantic_hash(node_type) {
            return Some(PROVENANCE_OR_PROOF);
        }
        match node_type {
            NodeType::ResolutionDecision | NodeType::Assumption => Some(GOVERNANCE),
            NodeType::Term | NodeType::Calculation | NodeType::Scenario => Some(DEDICATED),
            _ => None,
        }
    }

    /// The stage of a gate (architecture §2 stage table).
    fn gate_stage(gate: GateId) -> StageId {
        match gate {
            GateId::I0 => StageId::S0,
            GateId::F1 => StageId::S1,
            GateId::F2 => StageId::S2,
            GateId::F3 => StageId::S3,
            GateId::F4 => StageId::S4,
            GateId::Q1 => StageId::S5,
            GateId::A1 => StageId::S6,
            GateId::A2 => StageId::S7,
            GateId::A3 => StageId::S8,
            GateId::A4 => StageId::S9,
            GateId::D1 => StageId::S10,
            GateId::D2 => StageId::S11,
            GateId::C1 => StageId::S12,
        }
    }

    /// 409 `E_STAGE_CONFIG_MISSING` for an absent R0.6 runtime value.
    fn config_missing(field: &str) -> ApiError {
        ApiError::conflict(
            "E_STAGE_CONFIG_MISSING",
            format!("the {field} configuration is absent"),
        )
        .with_details(json!({ "missing": [field] }))
    }

    /// Persists `proposal` and its X1.3 `report` as a pending workflow proposal bound to the
    /// loaded revision (the storage invariants of the intake route): an identical pending
    /// proposal is idempotent, the same ID with different material is corrupt, and an
    /// identical non-pending one is not pending.
    fn persist_pending_proposal(
        state: &ApiState,
        loaded: &LoadedRevision,
        proposal: &Proposal,
        report: &plumb_intake::IntakeReport,
        origin_kind: WorkflowOrigin,
        origin_ref: Option<Id>,
        created_at: plumb_core::Timestamp,
    ) -> Result<StoredProposal, ApiError> {
        let kind = classify(&loaded.graph, proposal)?;
        let record = NewWorkflowProposal {
            proposal_id: proposal.id.clone(),
            base_revision_id: loaded.revision.id.clone(),
            proposal_json: stored_json(proposal)?,
            intake_json: stored_json(report)?,
            proposal_kind: store_kind(kind),
            origin_kind,
            origin_ref,
            created_at,
        };
        let inserted = state
            .with_workflow_store(|store| {
                store.insert_proposal(&record, WorkflowProposalState::Pending)
            })?
            .map_err(workflow_error)?;
        match inserted {
            WorkflowInsert::Inserted(stored) => decode_proposal(stored),
            WorkflowInsert::Existing(stored) => {
                let existing = decode_proposal(stored)?;
                if existing.proposal != *proposal {
                    return Err(ApiError::workflow_corrupt(&format!(
                        "proposal {}",
                        proposal.id
                    )));
                }
                if existing.record.state != WorkflowProposalState::Pending {
                    return Err(not_pending(&existing.record));
                }
                Ok(existing)
            }
        }
    }

    /// The view of one persisted Accepted Question; malformed material is a typed conflict.
    fn question_view(node: &Node, q: &plumb_psg::Question) -> Result<Question, ApiError> {
        let invalid = |field: &str, value: Value| {
            ApiError::conflict(
                "E_QUESTION_PROJECTION_INVALID",
                "a persisted Question is outside the pilot projection",
            )
            .with_details(json!({ "question": node.id, "field": field, "value": value }))
        };
        let status: QuestionStatus = serde_json::from_value(Value::String(q.status.clone()))
            .map_err(|_| invalid("status", json!(q.status)))?;
        if q.answer_schema.as_ref().is_some_and(|s| !s.is_object()) {
            return Err(invalid(
                "answer_schema",
                q.answer_schema.clone().unwrap_or(Value::Null),
            ));
        }
        let priority = match &q.priority {
            None => None,
            Some(text) => {
                Some(parse_priority(text).ok_or_else(|| invalid("priority", json!(text)))?)
            }
        };
        let kind: QuestionKind = parse_wire(&wire(&q.question_kind)?, "question kind")
            .map_err(|e| ApiError::internal_cause("question_kind", &e))?;
        Ok(Question {
            id: node.id.to_string(),
            kind,
            text: q.prompt.clone(),
            status,
            answer_schema: q.answer_schema.clone(),
            stakeholder_ref: q.stakeholder_ref.as_ref().map(Id::to_string),
            priority,
            round_ref: q.round_ref.as_ref().map(Id::to_string),
            context_refs: q.context_refs.iter().flatten().map(Id::to_string).collect(),
        })
    }

    /// Every Accepted Question node with its payload, by node ID.
    fn accepted_questions(graph: &Graph) -> impl Iterator<Item = (&Node, &plumb_psg::Question)> {
        graph
            .node_ids_by_type(NodeType::Question)
            .iter()
            .filter_map(|id| graph.node(id))
            .filter(|n| n.status == ElementStatus::Accepted)
            .filter_map(|n| match &n.payload {
                NodePayload::Question(q) => Some((n, q)),
                _ => None,
            })
    }

    impl ApiState {
        /// The branch head, loaded once as the baseline of a CLI command (its If-Match
        /// equivalent; the store CAS stays the final authority).
        pub fn head(&self) -> Result<LoadedRevision, ApiError> {
            self.load_head()
        }

        /// `create_source`: imports `bytes` with the importer of the declared `media_type` as
        /// one non-semantic evidence commit on `loaded` (artifacts persisted first).
        pub fn create_source(
            &self,
            loaded: &LoadedRevision,
            display_name: &str,
            media_type: &str,
            bytes: &[u8],
        ) -> Result<SourceUploadResponse, ApiError> {
            let importer = importer(media_type)?;
            let created_at = self.now();
            let imported = importer(
                display_name,
                bytes,
                &ImportAudit {
                    created_by: self.identity().actor_ref().clone(),
                    created_at,
                },
            )
            .map_err(|e| {
                ApiError::invalid("the uploaded file cannot be imported as its declared media type")
                    .with_details(json!({ "import_error": e.to_string() }))
            })?;
            // Fable #321: a source without any EvidenceFragment carries no evidence; refused
            // before any artifact or revision is written.
            if imported.fragments.is_empty() {
                return Err(ApiError::bad_request(
                    "E_SOURCE_EMPTY",
                    "the uploaded file yields no evidence fragments",
                ));
            }
            if let Some(existing) = loaded.graph.node(&imported.source.id) {
                // Fable #314: identity is the payload and extensions; audit metadata (who and
                // when) differs between uploads of the same source.
                let same = existing.payload == imported.source.payload
                    && existing.extensions == imported.source.extensions;
                let code = if same {
                    "E_SOURCE_EXISTS"
                } else {
                    "E_SOURCE_CONFLICT"
                };
                return Err(
                    ApiError::conflict(code, "a source with this content already exists")
                        .with_details(json!({ "id": imported.source.id })),
                );
            }
            let mut patches = vec![SemanticPatch::AddNode {
                node: imported.source.clone(),
            }];
            patches.extend(
                imported
                    .fragments
                    .iter()
                    .map(|node| SemanticPatch::AddNode { node: node.clone() }),
            );
            let proposal = Proposal::new(
                StageId::S0,
                PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch: SemanticPatch::Compound { patches },
                },
                Vec::new(),
                Vec::new(),
                ProposalMateriality::NonSemantic,
                AcceptancePolicy::AutoNonSemantic,
                None,
            )
            .map_err(|e| ApiError::internal_cause("proposal", &e))?;
            // Fable #368: the original and extracted artifacts are stored in the
            // revision-commit transaction, never left unreferenced by a lost CAS.
            let write = self.commit_with_artifacts(
                loaded,
                &proposal,
                &[
                    imported.original_artifact.clone(),
                    imported.extracted_artifact.clone(),
                ],
                created_at,
                "create_source",
            )?;
            let NodePayload::SourceArtifact(source) = &imported.source.payload else {
                return Err(ApiError::internal("importer"));
            };
            Ok(SourceUploadResponse {
                version: write.version,
                revision: write.revision,
                semantic_hash: write.semantic_hash,
                events: write.events,
                id: imported.source.id.to_string(),
                hash: source.content_hash.to_string(),
                spans_count: imported.fragments.len() as u64,
            })
        }

        /// The complete glossary projection of `loaded`: every Term, by node ID.
        pub fn glossary(&self, loaded: &LoadedRevision) -> Result<Vec<GlossaryTerm>, ApiError> {
            let mut items = Vec::new();
            for id in loaded.graph.node_ids_by_type(NodeType::Term) {
                let Some(node) = loaded.graph.node(id) else {
                    continue;
                };
                let NodePayload::Term(term) = &node.payload else {
                    continue;
                };
                items.push(GlossaryTerm {
                    id: node.id.to_string(),
                    term: term.term.clone(),
                    language: term.language.clone(),
                    aliases: term.aliases.clone().unwrap_or_default(),
                    definition_ref: term.definition_ref.as_ref().map(Id::to_string),
                    status: wire(&node.status)?,
                });
            }
            Ok(items)
        }

        /// Every persisted Accepted Question of `loaded` by node ID with its own projection, so a
        /// malformed Question degrades a list to a health marker instead of failing it (H0.11).
        pub fn question_items(
            &self,
            loaded: &LoadedRevision,
        ) -> Vec<(String, Result<Question, ApiError>)> {
            accepted_questions(&loaded.graph)
                .map(|(node, q)| (node.id.to_string(), question_view(node, q)))
                .collect()
        }

        /// The complete Question projection of `loaded`: every persisted Accepted Question, by
        /// node ID; malformed material is a typed conflict.
        pub fn questions(&self, loaded: &LoadedRevision) -> Result<Vec<Question>, ApiError> {
            accepted_questions(&loaded.graph)
                .map(|(node, q)| question_view(node, q))
                .collect()
        }

        /// `get_round`: reconstructs round `id` from the Accepted Questions carrying it and
        /// verifies its stakeholder, ordering and deterministic identity.
        pub fn round(&self, loaded: &LoadedRevision, id: &str) -> Result<Round, ApiError> {
            let missing = || ApiError::not_found(format!("round {id} not found"));
            let round_ref = parse_id(id).ok_or_else(missing)?;
            let inconsistent = |reason: &str| {
                ApiError::conflict(
                    "E_ROUND_INCONSISTENT",
                    "the round's questions do not reproduce its identity",
                )
                .with_details(json!({ "round": round_ref, "reason": reason }))
            };
            let mut members: Vec<(bool, u64, Id)> = Vec::new();
            let mut stakeholders = std::collections::BTreeSet::new();
            for (node, q) in accepted_questions(&loaded.graph) {
                if q.round_ref.as_ref() != Some(&round_ref) {
                    continue;
                }
                let Some(stakeholder) = &q.stakeholder_ref else {
                    return Err(inconsistent("a member has no stakeholder"));
                };
                stakeholders.insert(stakeholder.clone());
                let priority = q
                    .priority
                    .as_deref()
                    .and_then(parse_priority)
                    .ok_or_else(|| inconsistent("a member has no canonical priority"))?;
                let blocking = match loaded.graph.node(&q.finding_ref).map(|n| &n.payload) {
                    Some(NodePayload::Finding(f)) => f.severity == FindingSeverity::Blocker,
                    _ => return Err(inconsistent("a member's finding is missing")),
                };
                members.push((blocking, priority, node.id.clone()));
            }
            if members.is_empty() {
                return Err(missing());
            }
            let stakeholders: Vec<Id> = stakeholders.into_iter().collect();
            let [stakeholder] = stakeholders.as_slice() else {
                return Err(inconsistent("members have different stakeholders"));
            };
            members.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
            let ordered: Vec<Id> = members.into_iter().map(|(_, _, id)| id).collect();
            let expected = round_id(loaded.graph.project_id(), stakeholder, &ordered)
                .map_err(|e| ApiError::internal_cause("round_id", &e))?;
            if expected != round_ref {
                return Err(inconsistent(
                    "the round ID does not match its ordered members",
                ));
            }
            Ok(Round {
                id: round_ref.to_string(),
                stakeholder: stakeholder.to_string(),
                questions: ordered.iter().map(Id::to_string).collect(),
            })
        }

        /// `create_round`: one targeted round of at most `size` questions (validated by
        /// [`round_size`]) for an Accepted Stakeholder through the S3.2 kernel; the
        /// non-semantic round assignment is committed on `loaded`.
        pub fn create_round(
            &self,
            loaded: &LoadedRevision,
            stakeholder: &Id,
            size: usize,
        ) -> Result<CreateRoundResponse, ApiError> {
            let size = round_size(size as u64)?;
            let created_at = self.now();
            let result = compose_question_round_for_stakeholder(
                &loaded.graph,
                stakeholder,
                &QuestionRoundConfig {
                    question_round_size: size,
                },
            )
            .map_err(|error| match error {
                RoundError::StakeholderMissing(id) => {
                    ApiError::not_found(format!("stakeholder {id} not found"))
                }
                RoundError::StakeholderInvalid(id) => ApiError::conflict(
                    "E_ROUND_STAKEHOLDER_INVALID",
                    "the stakeholder is not an Accepted Stakeholder",
                )
                .with_details(json!({ "stakeholder": id })),
                other => ApiError::conflict(
                    "E_ROUND_COMPOSITION_FAILED",
                    "the round cannot be composed from the current questions",
                )
                .with_details(json!({ "error": error_name(&other), "reason": other.to_string() })),
            })?;
            let failed = result.dispositions.iter().any(|d| {
                matches!(
                    d,
                    QuestionRoundDisposition::RoundIdCollision { .. }
                        | QuestionRoundDisposition::InvalidRoundProposal { .. }
                )
            });
            let (Some(round), Some(proposal), false) =
                (result.rounds.first(), result.proposal.as_ref(), failed)
            else {
                if failed {
                    return Err(ApiError::conflict(
                        "E_ROUND_COMPOSITION_FAILED",
                        "the round cannot be composed from the current questions",
                    )
                    .with_details(json!({
                        "dispositions": serde_json::to_value(&result.dispositions)
                            .map_err(|e| ApiError::internal_cause("dispositions", &e))?
                    })));
                }
                return Err(ApiError::conflict(
                    "E_NO_QUESTIONS_FOR_ROUND",
                    "the stakeholder has no eligible unassigned question",
                )
                .with_details(json!({ "stakeholder": stakeholder })));
            };
            let write = self.write(loaded, proposal, false, None, created_at, "create_round")?;
            Ok(CreateRoundResponse {
                version: write.version,
                revision: write.revision,
                semantic_hash: write.semantic_hash,
                events: write.events,
                round: Round {
                    id: round.id.to_string(),
                    stakeholder: round.stakeholder_ref.to_string(),
                    questions: round.question_refs.iter().map(Id::to_string).collect(),
                },
            })
        }

        /// `materialize_questions` (Hotfix 083): runs the pure S3.1 kernel over `loaded` for the
        /// supplied current findings and routing, with the configured stage actor and the
        /// injected clock as audit, and commits its exact non-semantic proposal through the
        /// ordinary CAS write. A structural kernel issue fails before anything is committed;
        /// unmapped findings are reported, not an error. When every Finding and Question
        /// already exists identically there is no proposal, and nothing is written.
        pub fn materialize_questions(
            &self,
            loaded: &LoadedRevision,
            input: &QuestionGenerationInput,
        ) -> Result<MaterializedQuestions, ApiError> {
            let created_by = self
                .stage_runtime("materialize_questions")?
                .stage_actor_ref
                .clone();
            let created_at = self.now();
            let generation = generate_questions(
                &loaded.graph,
                input,
                &QuestionAudit {
                    created_by,
                    created_at,
                },
            )
            .map_err(|error| {
                ApiError::unprocessable(
                    "E_QUESTION_GENERATION_INVALID",
                    "the question generation input is invalid",
                )
                .with_details(json!({ "reason": error.to_string() }))
            })?;
            if !generation.issues.is_empty() {
                return Err(ApiError::conflict(
                    "E_QUESTION_GENERATION_ISSUES",
                    "the questions cannot be materialized from the current graph",
                )
                .with_details(json!({
                    "issues": serde_json::to_value(&generation.issues)
                        .map_err(|e| ApiError::internal_cause("question_issues", &e))?
                })));
            }
            let write = match &generation.proposal {
                None => None,
                Some(proposal) => Some(self.write(
                    loaded,
                    proposal,
                    false,
                    None,
                    created_at,
                    "materialize_questions",
                )?),
            };
            Ok(MaterializedQuestions { generation, write })
        }

        /// One proposal action on `loaded`: accept, create, link, merge and supersede commit the
        /// stored proposal unchanged through the governed A0.3 path (with the X1.5 governed
        /// recorder for stage-linked proposals) when its class matches; reject moves it
        /// pending -> rejected without changing the PSG.
        pub fn act_on_proposal(
            &self,
            loaded: &LoadedRevision,
            id: &str,
            action: ProposalAction,
            reason: Option<String>,
        ) -> Result<WriteResult, ApiError> {
            if let Some(reason) = &reason {
                require_clean_reason(reason)?;
            }
            self.runtime("workflow_store")?;
            let created_at = self.now();
            let stored = load_reconciled(self, loaded, id)?;
            if stored.record.state != WorkflowProposalState::Pending {
                return Err(not_pending(&stored.record));
            }
            let Some(required) = action.kind() else {
                return self.reject_stored(loaded, &stored, created_at);
            };
            if stored.record.base_revision_id != loaded.revision.id {
                return Err(baseline_changed(&stored.proposal.id));
            }
            let kind = classify(&loaded.graph, &stored.proposal)?;
            require_kind(&stored.proposal, kind, required)?;
            // A stage-produced proposal is first proven an exact, fresh output of its child
            // StageJob (A0.4, Hotfix 067); the governance below is the unchanged A0.3 path.
            let mut stage = match stage_acceptance::link_of(
                self,
                &stored.proposal.id,
                &stored.record.base_revision_id,
            )? {
                Some(link) => Some(stage_acceptance::prepare(
                    self,
                    loaded,
                    link,
                    &stored.proposal,
                )?),
                None => None,
            };
            // Stored intake is history; acceptance re-runs X1.3 on the current head.
            let report = run_intake(loaded, &stored.proposal)?;
            let (write, committed) = self.write_with_report_detailed(
                loaded,
                &stored.proposal,
                &report,
                true,
                reason.as_deref(),
                created_at,
                action.operation_id(),
            )?;
            if let Some(acceptance) = stage.as_mut() {
                stage_acceptance::record(self, acceptance, &stored.proposal, &committed)?;
            }
            // Moves the committed proposal pending -> accepted; an interruption here is
            // reconciled later from the acceptance decision in the graph.
            let revision = committed.commit.revision.id.clone();
            self.with_workflow_store(|store| {
                store.resolve_proposal(
                    &stored.proposal.id,
                    &stored.record.base_revision_id,
                    WorkflowProposalState::Accepted,
                    &revision,
                    created_at,
                )
            })?
            .map_err(workflow_error)?;
            if let Some(acceptance) = &stage {
                stage_acceptance::finish_parent(self, &acceptance.link.recipe_job_id, &revision)?;
            }
            Ok(write)
        }

        /// Rejects exactly the pending occurrence `stored` (pending -> rejected) without
        /// changing the PSG; the resolution is recorded on the loaded head.
        fn reject_stored(
            &self,
            loaded: &LoadedRevision,
            stored: &StoredProposal,
            created_at: plumb_core::Timestamp,
        ) -> Result<WriteResult, ApiError> {
            self.with_workflow_store(|store| {
                store.resolve_proposal(
                    &stored.proposal.id,
                    &stored.record.base_revision_id,
                    WorkflowProposalState::Rejected,
                    &loaded.revision.id,
                    created_at,
                )
            })?
            .map_err(workflow_error)?;
            let revision = loaded.revision.id.to_string();
            Ok(WriteResult {
                version: revision.clone(),
                revision,
                semantic_hash: loaded.revision.semantic_hash.to_string(),
                events: Vec::new(),
            })
        }

        /// Accepts a set of pending proposals built on the loaded head atomically in one
        /// revision (R0.3, architecture §4.7). `members` names each proposal with the explicit
        /// positive action it must match (`reject` is not a set action); `reason` applies to
        /// every member. Everything is validated against exactly `loaded` before any write:
        /// distinct, existing, pending members on this base; matching classes; fresh intake,
        /// reasons and rationale; exact fresh stage outputs (read-only); individually
        /// applicable patches whose element effects are pairwise disjoint; and an applicable
        /// composition. The commit is one CAS of `Compound [originals in ascending proposal-ID
        /// order, then one acceptance companion per member in the same order]`, each original
        /// unchanged and persisted as its own Patch artifact. A failure before the CAS leaves
        /// no revision and no workflow or stage transition; a moved head is 412, never retried.
        pub fn accept_proposal_set(
            &self,
            loaded: &LoadedRevision,
            members: &[(String, ProposalAction)],
            reason: Option<String>,
        ) -> Result<WriteResult, ApiError> {
            if let Some(reason) = &reason {
                require_clean_reason(reason)?;
            }
            self.runtime("workflow_store")?;
            // One governed acceptance event: one write instant for every member.
            let created_at = self.now();
            if members.is_empty() {
                return Err(ApiError::invalid(
                    "a proposal set needs at least one member",
                ));
            }
            let mut requested: Vec<(Id, ProposalKind)> = Vec::with_capacity(members.len());
            for (text, action) in members {
                let id = parse_id(text).ok_or_else(|| {
                    ApiError::not_found(format!("proposal {} not found", echo(text)))
                })?;
                let kind = action.kind().ok_or_else(|| {
                    ApiError::invalid("reject is not a proposal-set acceptance action")
                })?;
                if requested.iter().any(|(seen, _)| seen == &id) {
                    return Err(ApiError::invalid("a proposal occurs twice in the set")
                        .with_details(json!({ "proposal_ref": id })));
                }
                requested.push((id, kind));
            }
            // Caller order never defines commit order.
            requested.sort_by(|a, b| a.0.cmp(&b.0));

            struct Member {
                proposal: Proposal,
                companion: plumb_functional::resolution::AcceptanceCompanion,
                effects: std::collections::BTreeSet<Id>,
                link: Option<plumb_store::StageProposalLink>,
            }
            let mut validated: Vec<Member> = Vec::with_capacity(requested.len());
            for (id, required) in &requested {
                let missing = || ApiError::not_found(format!("proposal {id} not found"));
                let record = self
                    .with_workflow_store(|store| store.current_proposal(id, &loaded.revision.id))?
                    .map_err(workflow_error)?
                    .ok_or_else(missing)?;
                let stored = decode_proposal(record)?;
                if stored.record.state != WorkflowProposalState::Pending {
                    return Err(not_pending(&stored.record));
                }
                let base = stored.record.base_revision_id.clone();
                let proposal = stored.proposal;
                // A stage output is first proven an exact, fresh output of its Ready child,
                // read-only: a rejected set never persists a child or parent transition.
                let link = stage_acceptance::link_of(self, &proposal.id, &base)?;
                if let Some(link) = &link {
                    stage_acceptance::validate_set_member(self, loaded, link, &proposal)?;
                }
                if base != loaded.revision.id
                    || proposal.patch_set.base_semantic_hash != loaded.revision.semantic_hash
                {
                    return Err(baseline_changed(&proposal.id));
                }
                let kind = classify(&loaded.graph, &proposal)?;
                require_kind(&proposal, kind, *required)?;
                let report = run_intake(loaded, &proposal)?;
                if report.requires_reason && reason.is_none() {
                    return Err(reason_required(&proposal, &report));
                }
                let reason_codes: std::collections::BTreeSet<String> =
                    report.reason_codes.iter().map(|c| c.to_string()).collect();
                let companion = plumb_functional::resolution::build_acceptance_companion(
                    &loaded.graph,
                    &proposal,
                    &plumb_functional::resolution::AcceptanceInput {
                        reason_codes: reason_codes.into_iter().collect(),
                        decided_by: self.identity().actor_ref().clone(),
                        decided_at: created_at,
                        rationale: reason.clone(),
                    },
                )
                .map_err(companion_error)?;
                match plumb_intake::validate_required_rationale(
                    &proposal,
                    &report,
                    Some(&companion.decision_node),
                ) {
                    Ok(()) => {}
                    Err(plumb_intake::IntakeAcceptanceError::ReasonRequired(_))
                    | Err(plumb_intake::IntakeAcceptanceError::MissingRationale(_)) => {
                        return Err(reason_required(&proposal, &report))
                    }
                    Err(e) => return Err(ApiError::internal_cause("intake_rationale", &e)),
                }
                // The member's own effect on the untouched base.
                let delta = plumb_patch::apply_patch(&loaded.graph, &proposal.patch_set)
                    .map_err(|error| {
                        ApiError::unprocessable(
                            "E_PATCH_REJECTED",
                            "the proposed change cannot be applied to the current graph",
                        )
                        .with_details(json!({ "patch_error": error.to_string() }))
                    })?
                    .delta;
                let effects = delta
                    .touched_nodes
                    .iter()
                    .chain(&delta.touched_edges)
                    .chain(&delta.added_nodes)
                    .chain(&delta.removed_nodes)
                    .chain(&delta.modified_nodes)
                    .chain(&delta.added_edges)
                    .chain(&delta.removed_edges)
                    .chain(&delta.modified_edges)
                    .cloned()
                    .collect();
                validated.push(Member {
                    proposal,
                    companion,
                    effects,
                    link,
                });
            }
            // Compatibility: the members' independent element effects are pairwise disjoint.
            for (i, a) in validated.iter().enumerate() {
                for b in &validated[i + 1..] {
                    let shared: Vec<&Id> = a.effects.intersection(&b.effects).collect();
                    if !shared.is_empty() {
                        return Err(ApiError::proposal_set_conflict(json!({
                            "reason": "overlapping_effects",
                            "proposals": [a.proposal.id, b.proposal.id],
                            "elements": shared,
                        })));
                    }
                }
            }
            let patch_set = PatchSet {
                base_semantic_hash: loaded.revision.semantic_hash.clone(),
                patch: SemanticPatch::Compound {
                    patches: validated
                        .iter()
                        .map(|m| m.proposal.patch_set.patch.clone())
                        .chain(validated.iter().map(|m| SemanticPatch::AddNode {
                            node: m.companion.decision_node.clone(),
                        }))
                        .collect(),
                },
            };
            if plumb_patch::apply_patch(&loaded.graph, &patch_set).is_err() {
                let elements: std::collections::BTreeSet<&Id> =
                    validated.iter().flat_map(|m| &m.effects).collect();
                return Err(ApiError::proposal_set_conflict(json!({
                    "reason": "composition_not_applicable",
                    "proposals": validated.iter().map(|m| &m.proposal.id).collect::<Vec<_>>(),
                    "elements": elements,
                })));
            }
            // Each original effect is persisted unchanged before the CAS; an orphan after a
            // lost CAS is kept (as on the single path).
            for member in &validated {
                let stored = self.with_artifact_store(|store| {
                    store.put(
                        plumb_artifacts::ArtifactKind::Patch,
                        "application/json",
                        &member.companion.original_patch_bytes,
                        created_at,
                    )
                })?;
                match stored {
                    Ok(hash) if hash == member.companion.patch_ref => {}
                    Err(e) => return Err(ApiError::artifact_store(&e, "artifact_store")),
                    _ => return Err(ApiError::internal("artifact_store")),
                }
            }
            let mut decision_refs = std::collections::BTreeSet::new();
            for member in &validated {
                decision_refs.extend(introduced_decisions(&member.proposal.patch_set.patch));
                decision_refs.insert(member.companion.decision_ref.clone());
            }
            let (write, committed) = self.commit_patch_set_detailed(
                loaded,
                &patch_set,
                decision_refs.into_iter().collect(),
                created_at,
                "accept_proposal_set",
            )?;
            // Post-commit bookkeeping (recoverable from the companions; never re-committed).
            let proposals: Vec<Proposal> = validated.iter().map(|m| m.proposal.clone()).collect();
            let mut children: std::collections::BTreeMap<
                plumb_core::Hash,
                (plumb_store::StageProposalLink, Vec<Id>),
            > = std::collections::BTreeMap::new();
            for member in &validated {
                if let Some(link) = &member.link {
                    children
                        .entry(link.stage_job_id.clone())
                        .or_insert_with(|| (link.clone(), Vec::new()))
                        .1
                        .push(member.proposal.id.clone());
                }
            }
            for (link, owned) in children.values() {
                stage_acceptance::record_set(self, link, &proposals, owned, &committed)?;
            }
            let revision = committed.commit.revision.id.clone();
            for member in &validated {
                self.with_workflow_store(|store| {
                    store.resolve_proposal(
                        &member.proposal.id,
                        &loaded.revision.id,
                        WorkflowProposalState::Accepted,
                        &revision,
                        created_at,
                    )
                })?
                .map_err(workflow_error)?;
            }
            let parents: std::collections::BTreeSet<&plumb_core::Hash> = children
                .values()
                .map(|(link, _)| &link.recipe_job_id)
                .collect();
            for parent in parents {
                stage_acceptance::finish_parent(self, parent, &revision)?;
            }
            Ok(write)
        }

        // ==================================================================== R0.6 services

        /// The S3.3 answer service (R0.6): resolves `question_id` with `answer` on exactly
        /// `loaded`, persists the content-addressed patch artifact, runs X1.3 intake and stores
        /// the result as a pending Question-origin workflow proposal; nothing is committed. The
        /// response carries the authoritative workflow class of that exact Proposal (Hotfix
        /// 076), classified before any artifact, intake or workflow persistence. HTTP and CLI
        /// share it; transport validation (body, If-Match) stays with the caller.
        pub fn answer_question(
            &self,
            loaded: &LoadedRevision,
            question_id: &str,
            answer: Value,
            rationale: Option<String>,
            supersedes: Option<Id>,
        ) -> Result<AnswerResponse, ApiError> {
            let runtime = self.runtime("workflow_store")?;
            let created_at = self.now();
            let question_ref = parse_id(question_id)
                .ok_or_else(|| ApiError::not_found(format!("question {question_id} not found")))?;
            // Invariant bindings come only from injected scopes owned by Accepted Invariants.
            let invariant_bindings: std::collections::BTreeMap<Id, Vec<_>> = runtime
                .expression_scopes_for(&loaded.graph)?
                .into_iter()
                .filter(|scope| {
                    loaded.graph.node(&scope.owner_ref).is_some_and(|n| {
                        n.status == ElementStatus::Accepted
                            && n.payload.node_type() == NodeType::Invariant
                    })
                })
                .map(|scope| (scope.owner_ref, scope.bindings))
                .collect();
            let result = resolve_question(
                &loaded.graph,
                &ResolutionInput {
                    question_ref: question_ref.clone(),
                    answer,
                    decided_by: self.actor_ref().clone(),
                    decided_at: created_at,
                    rationale,
                    supersedes,
                },
                &ResolutionContext { invariant_bindings },
            )
            .map_err(|error| match error {
                ResolutionError::QuestionMissing(id) | ResolutionError::NotAQuestion(id) => {
                    ApiError::not_found(format!("question {id} not found"))
                }
                other => ApiError::semantic(&error_name(&other), other.to_string()),
            })?;
            let action = classify(&loaded.graph, &result.proposal)?;
            let stored = self.with_artifact_store(|store| {
                store.put(
                    plumb_artifacts::ArtifactKind::Patch,
                    "application/json",
                    &result.patch_artifact_bytes,
                    created_at,
                )
            })?;
            match stored {
                Ok(hash) if hash == result.decision.patch_ref => {}
                Err(e) => return Err(ApiError::artifact_store(&e, "artifact_store")),
                _ => return Err(ApiError::internal("artifact_store")),
            }
            let report = run_intake(loaded, &result.proposal)?;
            let persisted = persist_pending_proposal(
                self,
                loaded,
                &result.proposal,
                &report,
                WorkflowOrigin::Question,
                Some(question_ref),
                created_at,
            )?;
            let revision = loaded.revision.id.to_string();
            Ok(AnswerResponse {
                version: revision.clone(),
                revision,
                semantic_hash: loaded.revision.semantic_hash.to_string(),
                events: Vec::new(),
                proposal_id: persisted.proposal.id.to_string(),
                action,
                intake: intake_view(&persisted.intake, self.profile())?,
                patch_preview: serde_json::to_value(&persisted.proposal.patch_set.patch)
                    .map_err(|e| ApiError::internal_cause("patch_preview", &e))?,
            })
        }

        /// The configured directory service (R0.6, architecture §2): produces the configured
        /// directory on exactly `loaded` (one clock read, the stage actor as audit) and
        /// persists every generated Semantic / HUMAN_CONFIRM proposal as a pending
        /// Intake-origin workflow proposal after X1.3 intake. Nothing is committed, accepted or
        /// promoted; the outcomes are in element-ID order.
        pub fn persist_directory(
            &self,
            loaded: &LoadedRevision,
        ) -> Result<Vec<DirectoryElementView>, ApiError> {
            self.runtime("workflow_store")?;
            let a04 = self.stage_runtime("directory")?;
            let draft = a04
                .directory
                .as_ref()
                .ok_or_else(|| config_missing("directory"))?;
            let created_at = self.now();
            let bindings: Vec<DirectoryCalendarBinding> = a04
                .calendars
                .iter()
                .map(|c| DirectoryCalendarBinding {
                    calendar_ref: c.calendar_ref.clone(),
                    weekend: c.weekend.clone(),
                })
                .collect();
            let result = produce_directory(
                &loaded.graph,
                draft,
                &bindings,
                &DirectoryAudit {
                    created_by: a04.stage_actor_ref.clone(),
                    created_at,
                },
            )
            .map_err(|error| {
                ApiError::unprocessable(
                    "E_SEMANTIC_INVARIANT",
                    "the configured directory cannot be produced",
                )
                .with_details(json!({ "error": error_name(&error), "reason": error.to_string() }))
            })?;
            let mut views = Vec::with_capacity(result.outcomes.len());
            for outcome in result.outcomes {
                let node_type = outcome.node_type.as_str().to_owned();
                let element_ref = outcome.element_ref.to_string();
                match outcome.disposition {
                    DirectoryDisposition::Existing => views.push(DirectoryElementView {
                        element_ref,
                        node_type,
                        disposition: "existing".into(),
                        proposal_id: None,
                        action: None,
                        requires_reason: None,
                    }),
                    DirectoryDisposition::Proposed { proposal } => {
                        let report = run_intake(loaded, &proposal)?;
                        let stored = persist_pending_proposal(
                            self,
                            loaded,
                            &proposal,
                            &report,
                            WorkflowOrigin::Intake,
                            None,
                            created_at,
                        )?;
                        views.push(DirectoryElementView {
                            element_ref,
                            node_type,
                            disposition: "proposed".into(),
                            proposal_id: Some(stored.proposal.id.to_string()),
                            action: Some(stored.record.proposal_kind.as_str().to_owned()),
                            requires_reason: Some(stored.intake.requires_reason),
                        });
                    }
                }
            }
            Ok(views)
        }

        /// The generic governed confirmation (R0.6, architecture §2): one Semantic /
        /// HUMAN_CONFIRM proposal of sequential `SetStatus Proposed -> Accepted` leaves under
        /// exact element-hash preconditions for the explicitly listed existing elements (nodes
        /// by ID, then edges by ID), committed through the governed acceptance path (X1.3
        /// intake, acceptance companion, one CAS on `loaded`). It never creates or edits
        /// content. It refuses governance, provenance and proof types, the dedicated Term,
        /// Calculation and Scenario contracts, non-Proposed elements and Proposed edges whose
        /// endpoints would not both be baseline-participating after the same proposal.
        /// Requirement is eligible.
        pub fn confirm_elements(
            &self,
            loaded: &LoadedRevision,
            element_ids: &[String],
            reason: Option<String>,
        ) -> Result<WriteResult, ApiError> {
            if let Some(reason) = &reason {
                require_clean_reason(reason)?;
            }
            if element_ids.is_empty() {
                return Err(ApiError::invalid(
                    "confirmation needs at least one element ID",
                ));
            }
            let mut ids = std::collections::BTreeSet::new();
            for text in element_ids {
                let id = parse_id(text)
                    .ok_or_else(|| ApiError::invalid(format!("{} is not an ID", echo(text))))?;
                if !ids.insert(id.clone()) {
                    return Err(ApiError::invalid("an element is listed twice")
                        .with_details(json!({ "element": id })));
                }
            }
            let created_at = self.now();
            let graph = &loaded.graph;
            let refuse = |element: &Id, node_type: &str, reason: &str| {
                ApiError::unprocessable(
                    "E_SEMANTIC_INVARIANT",
                    "the element cannot be confirmed by the generic confirmation",
                )
                .with_details(json!({
                    "element": element,
                    "node_type": node_type,
                    "reason": reason,
                }))
            };
            let mut nodes = Vec::new();
            let mut edges = Vec::new();
            for id in &ids {
                if let Some(node) = graph.node(id) {
                    let t = node.payload.node_type();
                    if let Some(why) = confirmation_refusal(t) {
                        return Err(refuse(id, t.as_str(), why));
                    }
                    if node.status != ElementStatus::Proposed {
                        return Err(refuse(id, t.as_str(), "not_proposed"));
                    }
                    nodes.push(node);
                } else if let Some(edge) = graph.edge(id) {
                    if edge.status != ElementStatus::Proposed {
                        return Err(refuse(id, edge.kind.as_str(), "not_proposed"));
                    }
                    edges.push(edge);
                } else {
                    return Err(ApiError::not_found(format!("element {id} not found")));
                }
            }
            // Edge endpoints: no governance, provenance or proof endpoint, and each endpoint
            // baseline-participating after this proposal (already, or confirmed here).
            let promoted: std::collections::BTreeSet<&Id> = nodes.iter().map(|n| &n.id).collect();
            // A confirmed node brings its own Proposed relations along when the other endpoint
            // is (or becomes) baseline-participating: an Attribute cannot be Accepted without
            // its has_attribute edge, and callers cannot be expected to know edge IDs.
            let listed: std::collections::BTreeSet<&Id> = edges.iter().map(|e| &e.id).collect();
            let implied: Vec<_> = graph
                .edges()
                .values()
                .filter(|e| e.status == ElementStatus::Proposed && !listed.contains(&e.id))
                .filter(|e| promoted.contains(&e.from) || promoted.contains(&e.to))
                .filter(|e| {
                    [&e.from, &e.to].iter().all(|end| {
                        graph.node(end).is_some_and(|n| {
                            (is_baseline(n.status) || promoted.contains(end))
                                && !matches!(
                                    confirmation_refusal(n.payload.node_type()),
                                    Some(GOVERNANCE) | Some(PROVENANCE_OR_PROOF)
                                )
                        })
                    })
                })
                .collect();
            edges.extend(implied);
            for edge in &edges {
                for end in [&edge.from, &edge.to] {
                    let Some(node) = graph.node(end) else {
                        return Err(refuse(&edge.id, edge.kind.as_str(), "endpoint_missing"));
                    };
                    if matches!(
                        confirmation_refusal(node.payload.node_type()),
                        Some(GOVERNANCE) | Some(PROVENANCE_OR_PROOF)
                    ) {
                        return Err(refuse(
                            &edge.id,
                            edge.kind.as_str(),
                            "governance_or_provenance_endpoint",
                        ));
                    }
                    if !(is_baseline(node.status) || promoted.contains(end)) {
                        return Err(refuse(
                            &edge.id,
                            edge.kind.as_str(),
                            "endpoint_not_baseline_participating",
                        ));
                    }
                }
            }
            // The proposal stage: the earliest stage owning a confirmed element.
            let stage = nodes
                .iter()
                .map(|n| n.payload.node_type())
                .chain(
                    edges
                        .iter()
                        .flat_map(|e| [&e.from, &e.to])
                        .filter_map(|end| graph.node(end))
                        .map(|n| n.payload.node_type()),
                )
                .map(|t| gate_stage(earliest_gate(t)))
                .min()
                .unwrap_or(StageId::S1);
            let mut leaves = Vec::with_capacity(nodes.len() + edges.len());
            for node in &nodes {
                leaves.push(SemanticPatch::SetStatus {
                    target: ElementPrecondition {
                        id: node.id.clone(),
                        expected_hash: node_element_hash(node)
                            .map_err(|e| ApiError::internal_cause("element_hash", &e))?,
                    },
                    from: ElementStatus::Proposed,
                    to: ElementStatus::Accepted,
                });
            }
            for edge in &edges {
                leaves.push(SemanticPatch::SetStatus {
                    target: ElementPrecondition {
                        id: edge.id.clone(),
                        expected_hash: edge_element_hash(edge)
                            .map_err(|e| ApiError::internal_cause("element_hash", &e))?,
                    },
                    from: ElementStatus::Proposed,
                    to: ElementStatus::Accepted,
                });
            }
            let patch = if leaves.len() == 1 {
                leaves.remove(0)
            } else {
                SemanticPatch::Compound { patches: leaves }
            };
            let proposal = Proposal::new(
                stage,
                PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch,
                },
                Vec::new(),
                Vec::new(),
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
                None,
            )
            .map_err(|e| ApiError::internal_cause("proposal", &e))?;
            self.write(
                loaded,
                &proposal,
                true,
                reason.as_deref(),
                created_at,
                "confirm_elements",
            )
        }

        /// `set_attribute_type` (HTTP and CLI): replaces only `Attribute.value_type` with the
        /// name of an Accepted value_type Concept or a canonical pilot primitive type (String,
        /// Int, Bool, Date, DateTime, Decimal(n)). Domain extraction names Concepts, while
        /// expressions and scenarios only type primitives, so without primitives no Attribute
        /// could ever be used by a Calculation.
        pub fn set_attribute_type(
            &self,
            loaded: &LoadedRevision,
            id: &str,
            value_type: &str,
        ) -> Result<WriteResult, ApiError> {
            let missing = || ApiError::not_found(format!("attribute {id} not found"));
            let node = parse_id(id)
                .and_then(|i| loaded.graph.node(&i))
                .filter(|n| n.payload.node_type() == NodeType::Attribute)
                .ok_or_else(missing)?;
            let NodePayload::Attribute(attribute) = &node.payload else {
                return Err(ApiError::internal("attribute"));
            };
            let primitive = value_type != "Enum"
                && plumb_functional::map_pilot_value_type(value_type, None, None).is_ok();
            let concept = loaded
                .graph
                .node_ids_by_type(NodeType::Concept)
                .iter()
                .any(|id| {
                    loaded.graph.node(id).is_some_and(|n| {
                        n.status == ElementStatus::Accepted
                            && matches!(&n.payload, NodePayload::Concept(c)
                            if c.concept_kind == ConceptKind::ValueType && c.name == value_type)
                    })
                });
            if !primitive && !concept {
                return Err(ApiError::unprocessable(
                    "E_SEMANTIC_INVARIANT",
                    "type must be a pilot primitive type or name an Accepted value_type Concept",
                ));
            }
            if attribute.value_type == value_type {
                return Err(ApiError::unprocessable(
                    "E_NO_CHANGE",
                    "the request changes nothing",
                ));
            }
            let mut payload = attribute.clone();
            payload.value_type = value_type.to_owned();
            let proposal = Proposal::new(
                StageId::S2,
                PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch: SemanticPatch::ReplacePayload {
                        target: ElementPrecondition {
                            id: node.id.clone(),
                            expected_hash: node_element_hash(node)
                                .map_err(|e| ApiError::internal_cause("element_hash", &e))?,
                        },
                        payload: NodePayload::Attribute(payload),
                    },
                },
                Vec::new(),
                Vec::new(),
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
                None,
            )
            .map_err(|e| ApiError::internal_cause("proposal", &e))?;
            self.commit(loaded, &proposal, self.now(), "set_attribute_type")
        }

        /// The dedicated confirmation of one Proposed Term (S1), Calculation (S2) or Scenario
        /// (S4, Proposed + Derived -> Accepted + Confirmed): the same proposals and commit paths
        /// as `confirm_glossary`, `confirm_calculation` and `confirm_scenario`, so the CLI
        /// reaches every confirmation the HTTP API offers. `None` when `element` is not a node
        /// of a dedicated type (the caller then uses [`ApiState::confirm_elements`]).
        pub fn confirm_dedicated(
            &self,
            loaded: &LoadedRevision,
            element: &str,
        ) -> Result<Option<WriteResult>, ApiError> {
            let id = parse_id(element)
                .ok_or_else(|| ApiError::invalid(format!("{} is not an ID", echo(element))))?;
            let Some(node) = loaded.graph.node(&id) else {
                return Ok(None);
            };
            let created_at = self.now();
            let (stage, operation_id) = match node.payload.node_type() {
                NodeType::Term => (StageId::S1, "confirm_glossary"),
                NodeType::Calculation => (StageId::S2, "confirm_calculation"),
                NodeType::Scenario => {
                    let proposal = propose_scenario_confirmation(&loaded.graph, &id)
                        .map_err(crate::routes::scenario_lifecycle_error)?;
                    return self
                        .write(
                            loaded,
                            &proposal,
                            false,
                            None,
                            created_at,
                            "confirm_scenario",
                        )
                        .map(Some);
                }
                _ => return Ok(None),
            };
            if node.status != ElementStatus::Proposed {
                return Err(ApiError::unprocessable(
                    "E_SEMANTIC_INVARIANT",
                    "only a Proposed element can be confirmed",
                )
                .with_details(json!({ "status": wire(&node.status)? })));
            }
            let proposal = Proposal::new(
                stage,
                PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch: SemanticPatch::SetStatus {
                        target: ElementPrecondition {
                            id: node.id.clone(),
                            expected_hash: node_element_hash(node)
                                .map_err(|e| ApiError::internal_cause("element_hash", &e))?,
                        },
                        from: ElementStatus::Proposed,
                        to: ElementStatus::Accepted,
                    },
                },
                Vec::new(),
                Vec::new(),
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
                None,
            )
            .map_err(|e| ApiError::internal_cause("proposal", &e))?;
            self.commit(loaded, &proposal, created_at, operation_id)
                .map(Some)
        }

        /// Every lint diagnostic (errors and warnings) of the Accepted Requirements of `loaded`
        /// under the configured F1 lint policy, in requirement-ID order (read-only, R0.7).
        pub fn lint_diagnostics(
            &self,
            loaded: &LoadedRevision,
        ) -> Result<Vec<LintDiagnosticView>, ApiError> {
            let policy = match &self.stage_runtime("lint_diagnostics")?.gates.f1 {
                Some(f1) => {
                    f1.inputs_template()
                        .map_err(|_| ApiError::internal("lint_policy"))?
                        .lint_policy
                }
                None => Default::default(),
            };
            let mut views = Vec::new();
            for id in loaded.graph.node_ids_by_type(NodeType::Requirement) {
                let Some(node) = loaded.graph.node(id) else {
                    continue;
                };
                let NodePayload::Requirement(requirement) = &node.payload else {
                    continue;
                };
                if node.status != ElementStatus::Accepted || requirement.statement.trim().is_empty()
                {
                    continue;
                }
                let input = plumb_lint::LintInput {
                    requirement_ref: node.id.clone(),
                    statement: requirement.statement.clone(),
                    evidence_refs: node.evidence.iter().map(|e| e.as_id().clone()).collect(),
                    source_anchor: None,
                    term_context: None,
                };
                let Ok(result) = plumb_lint::lint_requirement(&input, &policy) else {
                    continue;
                };
                for d in result.diagnostics {
                    views.push(LintDiagnosticView {
                        requirement_ref: node.id.to_string(),
                        source_identifier: requirement.source_identifier.clone(),
                        rule_id: d.rule_id.to_string(),
                        severity: d.effective_severity.to_string(),
                        message: d.message,
                    });
                }
            }
            Ok(views)
        }

        /// The current gate findings (every evaluable NOW gate plus the per-candidate F2
        /// analysis findings), each with its disposition and governance state (H0.6).
        pub fn findings(&self, loaded: &LoadedRevision) -> Result<Vec<FindingView>, ApiError> {
            let policy = self.stage_runtime("findings")?.validation_policy.clone();
            let governed = plumb_validation::governed_finding_refs(&loaded.graph);
            let mut all: Vec<(String, GeneratedFinding)> = Vec::new();
            for report in self.current_or_evaluated_reports(loaded) {
                for finding in report.findings {
                    all.push((report.gate.to_string(), finding));
                }
            }
            if let Ok(analysis) = self.f2_analysis_findings(loaded) {
                all.extend(analysis.into_iter().map(|f| ("F2".to_owned(), f)));
            }
            let mut views: Vec<FindingView> = Vec::new();
            for (gate, finding) in all {
                if views.iter().any(|v| v.id == finding.id.as_str()) {
                    continue;
                }
                let disposition = self.disposition(&finding, &policy);
                views.push(FindingView {
                    id: finding.id.to_string(),
                    gate,
                    code: finding.payload.code.clone(),
                    severity: wire(&finding.payload.severity)?,
                    message: finding.payload.message.clone(),
                    affected_refs: finding
                        .payload
                        .affected_refs
                        .iter()
                        .map(Id::to_string)
                        .collect(),
                    disposition: disposition.to_owned(),
                    governed: governed.contains(&finding.id),
                    materialized: loaded.graph.node(&finding.id).is_some(),
                });
            }
            views.sort_by(|a, b| (&a.gate, &a.id).cmp(&(&b.gate, &b.id)));
            Ok(views)
        }

        /// The H0.6 disposition of one finding: a matching Question template makes it
        /// question_resolvable; a decision_required (or profile-enabled profile_allow) rule
        /// makes it waivable; anything else can only be fixed in the model.
        fn disposition(
            &self,
            finding: &GeneratedFinding,
            policy: &ValidationPolicy,
        ) -> &'static str {
            if plumb_functional::question_templates::match_template(
                &finding.payload.code,
                &finding.semantic_condition_key,
            )
            .is_ok()
            {
                return "question_resolvable";
            }
            let rule = self
                .profile()
                .rules
                .iter()
                .find(|r| r.id == finding.payload.code);
            match rule.map(|r| r.waiver_policy) {
                Some(plumb_validation::WaiverPolicy::DecisionRequired) => "waivable",
                Some(plumb_validation::WaiverPolicy::ProfileAllow)
                    if policy
                        .profile_allow_waiver_rule_ids
                        .contains(&finding.payload.code) =>
                {
                    "waivable"
                }
                _ => "non_waivable",
            }
        }

        /// `materialize_gate_findings` (H0.6): one non-semantic AutoNonSemantic proposal that
        /// adds every current actionable (waivable or question-resolvable) gate finding absent
        /// from the graph as an Accepted Finding node, and closes (status `Resolved`) every Open
        /// Finding node that no current gate or analysis finding produces any more. `None` when
        /// nothing changes.
        pub fn materialize_gate_findings(
            &self,
            loaded: &LoadedRevision,
        ) -> Result<Option<WriteResult>, ApiError> {
            let a04 = self.stage_runtime("materialize_gate_findings")?;
            let created_at = self.now();
            let audit = AuditMeta::new(a04.stage_actor_ref.clone(), created_at, None, None)
                .map_err(|e| ApiError::internal_cause("audit", &e))?;
            let policy = a04.validation_policy.clone();
            let reports = self.current_or_evaluated_reports(loaded);
            let mut current: std::collections::BTreeSet<Id> = reports
                .iter()
                .flat_map(|r| r.findings.iter().map(|f| f.id.clone()))
                .collect();
            if let Ok(analysis) = self.f2_analysis_findings(loaded) {
                current.extend(analysis.into_iter().map(|f| f.id));
            }
            let mut patches = Vec::new();
            let mut added = std::collections::BTreeSet::new();
            for finding in reports.iter().flat_map(|r| r.findings.iter()) {
                if loaded.graph.node(&finding.id).is_some()
                    || !added.insert(finding.id.clone())
                    || self.disposition(finding, &policy) == "non_waivable"
                {
                    continue;
                }
                patches.push(SemanticPatch::AddNode {
                    node: Node {
                        id: finding.id.clone(),
                        revision: 1,
                        status: ElementStatus::Accepted,
                        payload: NodePayload::Finding(finding.payload.clone()),
                        evidence: Vec::new(),
                        derivations: Vec::new(),
                        standards: Vec::new(),
                        tags: Default::default(),
                        extensions: Default::default(),
                        audit: audit.clone(),
                    },
                });
            }
            let governed = plumb_validation::governed_finding_refs(&loaded.graph);
            for id in loaded.graph.node_ids_by_type(NodeType::Finding) {
                let Some(node) = loaded.graph.node(id) else {
                    continue;
                };
                let NodePayload::Finding(f) = &node.payload else {
                    continue;
                };
                if node.status != ElementStatus::Accepted
                    || f.status != "Open"
                    || current.contains(id)
                    || governed.contains(id)
                {
                    continue;
                }
                let mut closed = f.clone();
                closed.status = "Resolved".to_owned();
                patches.push(SemanticPatch::ReplacePayload {
                    target: ElementPrecondition {
                        id: node.id.clone(),
                        expected_hash: node_element_hash(node)
                            .map_err(|e| ApiError::internal_cause("element_hash", &e))?,
                    },
                    payload: NodePayload::Finding(closed),
                });
            }
            if patches.is_empty() {
                return Ok(None);
            }
            let patch = if patches.len() == 1 {
                patches.remove(0)
            } else {
                SemanticPatch::Compound { patches }
            };
            let proposal = Proposal::new(
                StageId::S0,
                PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch,
                },
                Vec::new(),
                Vec::new(),
                ProposalMateriality::NonSemantic,
                AcceptancePolicy::AutoNonSemantic,
                None,
            )
            .map_err(|e| ApiError::internal_cause("proposal", &e))?;
            self.write(
                loaded,
                &proposal,
                false,
                None,
                created_at,
                "materialize_gate_findings",
            )
            .map(Some)
        }

        /// `waive_finding` (HTTP and CLI): the governed waiver of one current, materialized
        /// gate finding by the caller's actor with `reason`.
        pub fn waive_finding(
            &self,
            loaded: &LoadedRevision,
            finding: &str,
            reason: &str,
        ) -> Result<WriteResult, ApiError> {
            require_clean_reason(reason)?;
            let finding_id =
                parse_id(finding).ok_or_else(|| ApiError::invalid("finding is not an ID"))?;
            crate::routes::waive_finding(
                self,
                loaded,
                &finding_id,
                None,
                reason,
                self.now(),
                "waive_finding",
            )
        }

        /// `ask` (R0.6): materializes Questions from the findings of the current persisted F2
        /// report of `loaded` with the configured `question_routing`, through the shared
        /// [`ApiState::materialize_questions`] service.
        pub fn ask(&self, loaded: &LoadedRevision) -> Result<MaterializedQuestions, ApiError> {
            let routing = self
                .stage_runtime("ask")?
                .question_routing
                .clone()
                .ok_or_else(|| config_missing("question_routing"))?;
            let f2 = self.current_or_evaluated_report(loaded, GateId::F2)?;
            let mut findings = f2.findings;
            for finding in self.f2_analysis_findings(loaded)? {
                if !findings.iter().any(|f| f.id == finding.id) {
                    findings.push(finding);
                }
            }
            self.materialize_questions(loaded, &QuestionGenerationInput { findings, routing })
        }

        /// Every pending workflow proposal (R0.6 proposal discovery), sorted by proposal ID,
        /// with the information needed to act on it. Read-only: stale pending proposals are
        /// listed as they are (their cleanup is H0.10). A stage-origin proposal carries its
        /// recipe and stage job from its durable stage link; a missing link is corrupt state.
        pub fn pending_proposals(&self) -> Result<Vec<PendingProposalView>, ApiError> {
            self.runtime("workflow_store")?;
            let head = self.load_head()?.revision.id;
            let records = self
                .with_workflow_store(|store| store.pending_proposals())?
                .map_err(workflow_error)?;
            let mut views = Vec::with_capacity(records.len());
            for record in records {
                let stored = decode_proposal(record)?;
                let record = &stored.record;
                let (recipe_job_id, stage_job_id) = match record.origin_kind {
                    WorkflowOrigin::Stage => {
                        let link = stage_acceptance::link_of(
                            self,
                            &record.proposal_id,
                            &record.base_revision_id,
                        )?
                        .ok_or_else(|| {
                            ApiError::workflow_corrupt(&format!(
                                "stage link of proposal {}",
                                record.proposal_id
                            ))
                        })?;
                        (
                            Some(link.recipe_job_id.to_string()),
                            Some(link.stage_job_id.to_string()),
                        )
                    }
                    _ => (None, None),
                };
                views.push(PendingProposalView {
                    proposal_id: record.proposal_id.to_string(),
                    action: record.proposal_kind.as_str().to_owned(),
                    origin: record.origin_kind.as_str().to_owned(),
                    origin_ref: record.origin_ref.as_ref().map(Id::to_string),
                    recipe_job_id,
                    stage_job_id,
                    base_revision: record.base_revision_id.to_string(),
                    requires_reason: stored.intake.requires_reason,
                    stale: record.base_revision_id != head,
                });
            }
            views.sort_by(|a, b| a.proposal_id.cmp(&b.proposal_id));
            Ok(views)
        }

        /// Re-submits every stale pending answer proposal (a Question answer built on another
        /// revision than `loaded`) on `loaded` from its recorded decision: the original answer
        /// is the decision marker restricted to the Question's answer schema properties, with
        /// the same rationale and supersession. The stale proposal is then rejected. Nothing is
        /// rebased: the governed answer is resolved again on the current graph. An answer that
        /// no longer applies (e.g. already decided) only rejects the stale proposal. Returns
        /// (stale proposal, new proposal or the reason it was not re-submitted).
        pub fn refresh_stale_answers(
            &self,
            loaded: &LoadedRevision,
        ) -> Result<Vec<AnswerRefresh>, ApiError> {
            let mut outcomes = Vec::new();
            for view in self.pending_proposals()? {
                if !view.stale || view.origin != "question" {
                    continue;
                }
                // Exactly this stale occurrence (H0.10): an identical occurrence on the head
                // is a different record and is never rejected here.
                let corrupt =
                    || ApiError::workflow_corrupt(&format!("proposal {}", view.proposal_id));
                let (id, base): (Id, plumb_store::RevisionId) = (
                    view.proposal_id.parse().map_err(|_| corrupt())?,
                    view.base_revision.parse().map_err(|_| corrupt())?,
                );
                let record = self
                    .with_workflow_store(|store| store.proposal(&id, &base))?
                    .map_err(workflow_error)?
                    .ok_or_else(corrupt)?;
                let stored = decode_proposal(record)?;
                let decision = answer_decision(&stored.proposal.patch_set.patch);
                let resubmitted = match decision {
                    None => Err("the proposal records no answer decision".to_owned()),
                    Some(decision) => {
                        let question = decision
                            .question_ref
                            .as_ref()
                            .and_then(|q| loaded.graph.node(q))
                            .and_then(|n| match &n.payload {
                                NodePayload::Question(q) => Some((n.id.clone(), q.clone())),
                                _ => None,
                            });
                        match question {
                            None => Err("its question no longer exists".to_owned()),
                            Some((question_ref, question)) => {
                                let keys: Vec<String> = question
                                    .answer_schema
                                    .as_ref()
                                    .and_then(|s| s.get("properties"))
                                    .and_then(Value::as_object)
                                    .map(|p| p.keys().cloned().collect())
                                    .unwrap_or_default();
                                let answer: serde_json::Map<String, Value> = keys
                                    .iter()
                                    .filter_map(|k| {
                                        decision.answer.get(k).map(|v| (k.clone(), v.clone()))
                                    })
                                    .collect();
                                self.answer_question(
                                    loaded,
                                    question_ref.as_str(),
                                    Value::Object(answer),
                                    decision.rationale.clone(),
                                    decision.supersedes.clone(),
                                )
                                .map(|r| r.proposal_id)
                                .map_err(|e| e.body().message.clone())
                            }
                        }
                    }
                };
                if stored.record.state == WorkflowProposalState::Pending {
                    self.reject_stored(loaded, &stored, self.now())?;
                }
                outcomes.push((view.proposal_id, resubmitted));
            }
            Ok(outcomes)
        }

        /// The pending S1.2 EARS proposal of `requirement` on `loaded` (R0.6): a pending
        /// stage-origin workflow proposal built on exactly this head, linked to a consolidate
        /// (S1) child job, whose S1 patch is one `ReplacePayload` of that Requirement under its
        /// current element hash changing only its statement. Returns the normalized statement
        /// and the proposal ID; nothing is regenerated. Without a workflow or stage runtime
        /// there is none; several distinct matches are corrupt state.
        pub fn pending_ears_proposal(
            &self,
            loaded: &LoadedRevision,
            requirement: &Id,
        ) -> Result<Option<(String, Id)>, ApiError> {
            if self.runtime("workflow_store").is_err() || self.stage_runtime("ears").is_err() {
                return Ok(None);
            }
            let Some(node) = loaded.graph.node(requirement) else {
                return Ok(None);
            };
            let NodePayload::Requirement(current) = &node.payload else {
                return Ok(None);
            };
            let current_hash = node_element_hash(node)
                .map_err(|e| ApiError::internal_cause("element_hash", &e))?;
            let records = self
                .with_workflow_store(|store| store.pending_proposals())?
                .map_err(workflow_error)?;
            let mut found: Vec<(String, Id)> = Vec::new();
            for record in records {
                if record.origin_kind != WorkflowOrigin::Stage
                    || record.base_revision_id != loaded.revision.id
                {
                    continue;
                }
                let stored = decode_proposal(record)?;
                let proposal = &stored.proposal;
                if proposal.stage != StageId::S1
                    || proposal.patch_set.base_semantic_hash != loaded.revision.semantic_hash
                {
                    continue;
                }
                let SemanticPatch::ReplacePayload { target, payload } = &proposal.patch_set.patch
                else {
                    continue;
                };
                let NodePayload::Requirement(updated) = payload else {
                    continue;
                };
                if &target.id != requirement || target.expected_hash != current_hash {
                    continue;
                }
                // Only the statement changes: the S1.2 normalization effect.
                let normalized = plumb_psg::Requirement {
                    statement: current.statement.clone(),
                    ..updated.clone()
                };
                if &normalized != current || updated.statement == current.statement {
                    continue;
                }
                let Some(link) =
                    stage_acceptance::link_of(self, &proposal.id, &loaded.revision.id)?
                else {
                    return Err(ApiError::workflow_corrupt(&format!(
                        "stage link of proposal {}",
                        proposal.id
                    )));
                };
                let steps = self
                    .with_stage_store(|store| store.steps(&link.recipe_job_id))?
                    .map_err(|_| ApiError::workflow_corrupt("stage steps"))?;
                let consolidate = steps.iter().any(|step| {
                    step.stage_job_id.as_ref() == Some(&link.stage_job_id)
                        && step.phase == Phase::Consolidate.as_str()
                        && step.compiler_stage == StageId::S1.as_str()
                });
                if consolidate {
                    found.push((updated.statement.clone(), proposal.id.clone()));
                }
            }
            match found.len() {
                0 => Ok(None),
                1 => Ok(found.pop()),
                _ => Err(ApiError::workflow_corrupt(&format!(
                    "pending EARS proposals of {requirement}"
                ))),
            }
        }

        /// The pilot projection of `loaded` over the active profile and the current F4 report:
        /// `project_functional_pilot` called once for HTTP export and CLI export and report.
        pub fn pilot_projection(
            &self,
            loaded: &LoadedRevision,
        ) -> Result<FunctionalPilotOutputs, ApiError> {
            let f4 = self.current_or_evaluated_report(loaded, GateId::F4)?;
            project_functional_pilot(&loaded.graph, self.profile(), &f4).map_err(|e| {
                ApiError::unprocessable("E_SEMANTIC_INVARIANT", "the model cannot be projected")
                    .with_details(json!({ "projection_error": e.to_string() }))
            })
        }
    }
}

pub(crate) use services::{baseline_changed, not_pending, workflow_error};
pub use services::{
    export_artifact, require_importable, round_size, DirectoryElementView, FindingView,
    MaterializedQuestions, PendingProposalView, ProposalAction,
};
