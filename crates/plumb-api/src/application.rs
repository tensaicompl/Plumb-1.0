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
    AuthorizationDraft, CalculationScope, CalendarProvider, DuplicatePolicy, OperationScope,
    PredicateProvider, ProcessScope, ScenarioDerivationInputs, VerificationApplicabilityInputs,
    VocabularyPolicy,
};
use plumb_inference::{InferenceProvider, ProviderPolicy};
use plumb_psg::{Agent, AgentKind, AuditMeta, ElementStatus, Graph, Node, NodePayload};
use plumb_session::SessionTurnConfig;
use plumb_sim::{
    ExecutionCalculationScopeOverride, ExecutionExpressionScopeInput, RuntimeSemanticValue,
};
use plumb_store::{
    BranchName, RevisionId, RevisionWriteMeta, SqliteRevisionStore, SqliteStageStore,
    SqliteWorkflowStore, StoreError,
};
use plumb_validation::{ValidationPolicy, ValidationProfile};
use serde::Serialize;
use thiserror::Error;

use crate::runtime::{
    A04RuntimeConfig, CalendarConfig, DirectManagerFact, GateInputsConfig, ProviderMode,
};
use crate::state::{A03RuntimeConfig, ApiIdentity, ApiState, ApiStateError};

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
}

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
    pub expression_scopes: Vec<ExecutionExpressionScopeInput>,
    pub calculation_scope_overrides: Vec<ExecutionCalculationScopeOverride>,
    pub runtime_semantic_values: Vec<RuntimeSemanticValue>,
    pub validation_policy: ValidationPolicy,
    pub vocabulary_policy: VocabularyPolicy,
    pub duplicate_policy: DuplicatePolicy,
    pub calculation_scope: Option<CalculationScope>,
    pub operation_scope: Option<OperationScope>,
    pub process_scope: Option<ProcessScope>,
    pub authorization_draft: Option<AuthorizationDraft>,
    pub scenario_derivation: Option<ScenarioDerivationInputs>,
    pub verification_applicability: Option<VerificationApplicabilityInputs>,
    pub gates: GateInputsConfig,
}

/// Opens the revision, artifact, workflow and stage stores over `db` and builds the full A0.4
/// state for `branch` (startup-injected gate reports: none; durable reports are current).
pub fn open_application(
    db: &Path,
    branch: BranchName,
    identity: ApiIdentity,
    clock: Arc<dyn Clock + Send + Sync>,
    profile: ValidationProfile,
    runtime: ApplicationRuntime,
) -> Result<ApiState, ApplicationError> {
    let revision_store = SqliteRevisionStore::open(db)?;
    let artifact_store =
        SqliteArtifactStore::open(db).map_err(|e| ApplicationError::Artifacts(e.to_string()))?;
    let workflow_store = SqliteWorkflowStore::open(db)?;
    let stage_store = SqliteStageStore::open(db)?;
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
    };
    Ok(ApiState::new_with_stage_runtime(
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
    use plumb_functional::{
        compose_question_round_for_stakeholder, generate_questions, parse_priority,
        project_functional_pilot, round_id, FunctionalPilotOutputs, ProjectionArtifact,
        QuestionAudit, QuestionGenerationInput, QuestionGenerationResult, QuestionRoundConfig,
        QuestionRoundDisposition, RoundError, MAX_QUESTION_ROUND_SIZE,
    };
    use plumb_import::{
        import_docx, import_markdown, import_plain_text, ImportAudit, ImportedSource,
    };
    use plumb_patch::{AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch};
    use plumb_psg::{ElementStatus, FindingSeverity, Graph, Node, NodePayload, NodeType};
    use plumb_store::{LoadedRevision, StoreError, WorkflowProposal, WorkflowProposalState};
    use serde_json::{json, Value};

    use crate::error::ApiError;
    use crate::openapi::{
        CreateRoundResponse, ExportFormat, GlossaryTerm, ProposalKind, Question, QuestionKind,
        QuestionStatus, Round, SourceUploadResponse, WriteResult,
    };
    use crate::stage::acceptance as stage_acceptance;
    use crate::stage::scenario_execution::error_name;
    use crate::state::{
        companion_error, introduced_decisions, parse_id, parse_wire, reason_required,
        require_clean_reason, run_intake, wire, ApiState,
    };
    use crate::workflow::{
        acceptance_decisions, classify, decode_proposal, require_kind, StoredProposal,
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
            _ => ApiError::internal("workflow_store"),
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

    /// Loads and decodes the workflow proposal `id` (404 when unknown), then reconciles an
    /// interrupted acceptance: a pending proposal whose acceptance decision is already in the
    /// graph becomes accepted (with its stage child and parent, A0.4); several such decisions
    /// are corrupt workflow state.
    pub(crate) fn load_reconciled(
        state: &ApiState,
        loaded: &LoadedRevision,
        id: &str,
    ) -> Result<StoredProposal, ApiError> {
        let missing = || ApiError::not_found(format!("proposal {id} not found"));
        let proposal_id = parse_id(id).ok_or_else(missing)?;
        let record = state
            .with_workflow_store(|store| store.proposal(&proposal_id))?
            .map_err(workflow_error)?
            .ok_or_else(missing)?;
        let stored = decode_proposal(record)?;
        if stored.record.state != WorkflowProposalState::Pending {
            return Ok(stored);
        }
        let decisions = acceptance_decisions(&loaded.graph, &proposal_id);
        match decisions.as_slice() {
            [] => Ok(stored),
            [(_, decided_at)] => {
                let repaired = state
                    .with_workflow_store(|store| {
                        store.resolve_proposal(
                            &proposal_id,
                            WorkflowProposalState::Accepted,
                            &loaded.revision.id,
                            *decided_at,
                        )
                    })?
                    .map_err(workflow_error)?;
                if let Some(link) = stage_acceptance::link_of(state, &proposal_id)? {
                    stage_acceptance::reconcile(state, loaded, &link)?;
                }
                decode_proposal(repaired)
            }
            _ => Err(ApiError::workflow_corrupt(&format!(
                "proposal {proposal_id}"
            ))),
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
            .map_err(|_| ApiError::internal("question_kind"))?;
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
            if let Some(existing) = loaded.graph.node(&imported.source.id) {
                let code = if existing == &imported.source {
                    "E_SOURCE_EXISTS"
                } else {
                    "E_SOURCE_CONFLICT"
                };
                return Err(
                    ApiError::conflict(code, "a source with this content already exists")
                        .with_details(json!({ "id": imported.source.id })),
                );
            }
            for artifact in [&imported.original_artifact, &imported.extracted_artifact] {
                let stored = self.with_artifact_store(|store| {
                    store.put(
                        artifact.kind,
                        &artifact.media_type,
                        &artifact.bytes,
                        artifact.created_at,
                    )
                })?;
                match stored {
                    Ok(hash) if hash == artifact.hash => {}
                    _ => return Err(ApiError::internal("artifact_store")),
                }
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
            .map_err(|_| ApiError::internal("proposal"))?;
            let write = self.commit(loaded, &proposal, created_at, "create_source")?;
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
                .map_err(|_| ApiError::internal("round_id"))?;
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
                            .map_err(|_| ApiError::internal("dispositions"))?
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
                        .map_err(|_| ApiError::internal("question_issues"))?
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
                self.with_workflow_store(|store| {
                    store.resolve_proposal(
                        &stored.proposal.id,
                        WorkflowProposalState::Rejected,
                        &loaded.revision.id,
                        created_at,
                    )
                })?
                .map_err(workflow_error)?;
                let revision = loaded.revision.id.to_string();
                return Ok(WriteResult {
                    version: revision.clone(),
                    revision,
                    semantic_hash: loaded.revision.semantic_hash.to_string(),
                    events: Vec::new(),
                });
            };
            if stored.record.base_revision_id != loaded.revision.id {
                return Err(baseline_changed(&stored.proposal.id));
            }
            let kind = classify(&loaded.graph, &stored.proposal)?;
            require_kind(&stored.proposal, kind, required)?;
            // A stage-produced proposal is first proven an exact, fresh output of its child
            // StageJob (A0.4, Hotfix 067); the governance below is the unchanged A0.3 path.
            let mut stage = match stage_acceptance::link_of(self, &stored.proposal.id)? {
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
                let id = parse_id(text)
                    .ok_or_else(|| ApiError::not_found(format!("proposal {text} not found")))?;
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
                    .with_workflow_store(|store| store.proposal(id))?
                    .map_err(workflow_error)?
                    .ok_or_else(missing)?;
                let stored = decode_proposal(record)?;
                if stored.record.state != WorkflowProposalState::Pending {
                    return Err(not_pending(&stored.record));
                }
                let proposal = stored.proposal;
                // A stage output is first proven an exact, fresh output of its Ready child,
                // read-only: a rejected set never persists a child or parent transition.
                let link = stage_acceptance::link_of(self, &proposal.id)?;
                if let Some(link) = &link {
                    stage_acceptance::validate_set_member(self, loaded, link, &proposal)?;
                }
                if stored.record.base_revision_id != loaded.revision.id
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
                    Err(_) => return Err(ApiError::internal("intake_rationale")),
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

        /// The pilot projection of `loaded` over the active profile and the current F4 report:
        /// `project_functional_pilot` called once for HTTP export and CLI export and report.
        pub fn pilot_projection(
            &self,
            loaded: &LoadedRevision,
        ) -> Result<FunctionalPilotOutputs, ApiError> {
            let f4 = self.current_gate_report(loaded, GateId::F4)?;
            project_functional_pilot(&loaded.graph, self.profile(), &f4).map_err(|e| {
                ApiError::unprocessable("E_SEMANTIC_INVARIANT", "the model cannot be projected")
                    .with_details(json!({ "projection_error": e.to_string() }))
            })
        }
    }
}

pub(crate) use services::{baseline_changed, not_pending, workflow_error};
pub use services::{
    export_artifact, require_importable, round_size, MaterializedQuestions, ProposalAction,
};
