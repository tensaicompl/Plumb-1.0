//! The session runtime data contracts (plan X1.4, Hotfix 060): the session, its turns and
//! configuration, transcript turns, the bounded inference context, the turn plan, the inspected
//! model output, per-proposal intake inputs and the display result.
//!
//! A session is runtime orchestration data around a caller-supplied active compiler stage; it
//! is not a PSG element and not a stage of its own.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use plumb_artifacts::{ArtifactKind, ArtifactStoreError};
use plumb_core::{CoreError, Hash, Id, StageId, Timestamp};
use plumb_inference::{
    InferenceError, InferenceRequest, InferenceRequestMaterial, PersistedInferenceRefs,
    ProviderError, ProviderPolicy,
};
use plumb_intake::{IndexError, IntakeError, IntakeReport, IntakeValidationInput};
use plumb_patch::{PatchError, Proposal, ProposalError};
use plumb_psg::{AuditMetaError, ElementStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The inference task kind of a session turn.
pub const SESSION_TURN_TASK_KIND: &str = "session_turn";
/// The maximum number of proposals one session turn may return.
pub const SESSION_MAX_PROPOSALS: usize = 20;
/// The version of [`SessionInferenceContext`].
pub const SESSION_CONTEXT_VERSION: u32 = 1;
/// The version of the session-turn model output.
pub const SESSION_OUTPUT_VERSION: u32 = 1;
/// The version of the canonical transcript envelope.
pub const TRANSCRIPT_VERSION: u32 = 1;
/// The media type of canonical transcript source artifacts.
pub const TRANSCRIPT_MEDIA_TYPE: &str = "application/vnd.plumb.transcript+json";
/// The `SourceArtifact.source_kind` of transcript turns.
pub const TRANSCRIPT_SOURCE_KIND: &str = "transcript";

/// One conversation: its project, its explicit scope and the assistant actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SessionFields")]
pub struct Session {
    pub id: Id,
    pub project_ref: Id,
    /// Sorted and unique.
    pub scope_refs: Vec<Id>,
    pub assistant_actor_ref: Id,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionFields {
    id: Id,
    project_ref: Id,
    scope_refs: Vec<Id>,
    assistant_actor_ref: Id,
}

impl TryFrom<SessionFields> for Session {
    type Error = SessionError;

    fn try_from(f: SessionFields) -> Result<Self, Self::Error> {
        let session = Session {
            id: f.id,
            project_ref: f.project_ref,
            scope_refs: f.scope_refs,
            assistant_actor_ref: f.assistant_actor_ref,
        };
        session.validate()?;
        Ok(session)
    }
}

impl Session {
    /// Builds a session with its scope sorted; a repeated scope ref is an error.
    pub fn new(
        id: Id,
        project_ref: Id,
        mut scope_refs: Vec<Id>,
        assistant_actor_ref: Id,
    ) -> Result<Session, SessionError> {
        scope_refs.sort();
        let session = Session {
            id,
            project_ref,
            scope_refs,
            assistant_actor_ref,
        };
        session.validate()?;
        Ok(session)
    }

    /// Requires `scope_refs` to be strictly increasing.
    pub fn validate(&self) -> Result<(), SessionError> {
        for pair in self.scope_refs.windows(2) {
            if pair[0] == pair[1] {
                return Err(SessionError::DuplicateScopeRef(pair[0].clone()));
            }
            if pair[0] > pair[1] {
                return Err(SessionError::UnsortedScopeRefs);
            }
        }
        Ok(())
    }
}

/// Who spoke a transcript turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    User,
    Assistant,
}

impl SessionRole {
    /// The exact wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            SessionRole::User => "user",
            SessionRole::Assistant => "assistant",
        }
    }
}

impl fmt::Display for SessionRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One user turn and the stage it is spoken in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUserTurn {
    /// Stored and sent exactly; must not be whitespace-only.
    pub text: String,
    pub user_actor_ref: Id,
    pub active_stage: StageId,
    pub provider_policy: ProviderPolicy,
}

/// Bounds of the inference context of one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTurnConfig {
    /// 0 disables retrieval context.
    pub retrieval_top_k: usize,
    /// 0 disables open-question context.
    pub max_open_questions: usize,
    /// Bound on the summed canonical byte length of the context items; must be positive.
    pub max_context_bytes: usize,
}

impl SessionTurnConfig {
    pub fn validate(&self) -> Result<(), SessionError> {
        if self.max_context_bytes == 0 {
            return Err(SessionError::ZeroContextBudget);
        }
        Ok(())
    }
}

/// One persisted transcript turn and its deterministic PSG identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptTurn {
    /// `turn:<first 16 hex of the transcript artifact digest>`.
    pub turn_ref: Id,
    pub role: SessionRole,
    pub actor_ref: Id,
    pub timestamp: Timestamp,
    /// The transcript `SourceOriginal` artifact hash, also the SourceArtifact content hash.
    pub source_artifact_ref: Hash,
    pub source_node_ref: Id,
    pub evidence_ref: Id,
    pub text: String,
}

/// Why a node is in the inference context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionContextSource {
    Scope,
    OpenQuestion,
    Retrieval,
}

impl SessionContextSource {
    /// The exact wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            SessionContextSource::Scope => "scope",
            SessionContextSource::OpenQuestion => "open_question",
            SessionContextSource::Retrieval => "retrieval",
        }
    }
}

impl fmt::Display for SessionContextSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One context node: its semantic payload only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionContextItem {
    pub source: SessionContextSource,
    pub node_ref: Id,
    /// The payload wire type tag.
    pub node_type: String,
    /// The payload wire data.
    pub payload: Value,
}

/// The exact context of one session-turn inference; its canonical JSON is the request
/// material's `context_json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionInferenceContext {
    pub version: u32,
    pub session_ref: Id,
    pub project_ref: Id,
    pub profile_ref: Id,
    pub base_semantic_hash: Hash,
    pub active_stage: StageId,

    pub user_turn_ref: Id,
    pub user_evidence_ref: Id,
    pub user_text: String,

    pub scope_refs: Vec<Id>,
    pub items: Vec<SessionContextItem>,

    pub allowed_open_question_refs: Vec<Id>,
    pub allowed_evidence_refs: Vec<Id>,

    /// The summed RFC 8785 canonical byte length of `items`.
    pub context_item_bytes: usize,
}

/// Everything a caller needs to acquire one session-turn inference.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionTurnPlan {
    pub session_ref: Id,
    pub active_stage: StageId,

    pub user_turn: TranscriptTurn,
    pub user_transcript_proposal: Option<Proposal>,
    pub user_transcript_intake: Option<IntakeReport>,

    pub context: SessionInferenceContext,

    pub request: InferenceRequest,
    pub material: InferenceRequestMaterial,
}

/// The typed session-turn model output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionModelOutput {
    pub version: u32,
    pub reply: String,
    pub proposals: Vec<Proposal>,
    pub asked_question_ref: Option<Id>,
}

/// A validated model output bound to the request and persisted acquisition it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct InspectedSessionOutput {
    pub request_ref: Hash,
    pub persisted_inference: PersistedInferenceRefs,
    pub reply: String,
    /// In persisted model order.
    pub proposals: Vec<Proposal>,
    pub asked_question_ref: Option<Id>,
}

/// The caller's explicit candidate-validation input for each proposal of a turn.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionIntakeInputs {
    by_proposal: BTreeMap<Id, IntakeValidationInput>,
}

impl SessionIntakeInputs {
    /// One input per proposal ID; a repeated ID is an error.
    pub fn new(
        inputs: impl IntoIterator<Item = (Id, IntakeValidationInput)>,
    ) -> Result<SessionIntakeInputs, SessionError> {
        let mut by_proposal = BTreeMap::new();
        for (proposal_ref, input) in inputs {
            if by_proposal.contains_key(&proposal_ref) {
                return Err(SessionError::DuplicateIntakeInput(proposal_ref));
            }
            by_proposal.insert(proposal_ref, input);
        }
        Ok(SessionIntakeInputs { by_proposal })
    }

    pub fn get(&self, proposal_ref: &Id) -> Option<&IntakeValidationInput> {
        self.by_proposal.get(proposal_ref)
    }

    pub fn proposal_refs(&self) -> BTreeSet<&Id> {
        self.by_proposal.keys().collect()
    }
}

/// One proposal and its intake analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionProposalCard {
    pub proposal: Proposal,
    pub intake: IntakeReport,
}

/// The display material of one completed turn. Nothing in it is accepted or committed.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionTurnResult {
    pub request_ref: Hash,
    pub persisted_inference: PersistedInferenceRefs,

    pub user_turn: TranscriptTurn,
    pub user_transcript_proposal: Option<Proposal>,
    pub user_transcript_intake: Option<IntakeReport>,

    pub assistant_turn: TranscriptTurn,
    pub assistant_transcript_proposal: Option<Proposal>,
    pub assistant_transcript_intake: Option<IntakeReport>,

    pub reply: String,
    /// In persisted model order.
    pub proposals: Vec<SessionProposalCard>,
    pub asked_question_ref: Option<Id>,
}

/// Why a session turn could not be prepared, inspected or completed.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Audit(#[from] AuditMetaError),
    #[error("artifact store: {0}")]
    Artifact(#[from] ArtifactStoreError),
    #[error("inference: {0}")]
    Inference(#[from] InferenceError),
    #[error("inference request material: {0}")]
    Material(#[from] ProviderError),
    #[error("transcript patch: {0}")]
    Patch(#[from] PatchError),
    #[error("transcript intake: {0}")]
    Intake(#[from] IntakeError),
    #[error("lexical index: {0}")]
    Index(#[from] IndexError),

    #[error("scope ref {0} is repeated")]
    DuplicateScopeRef(Id),
    #[error("scope refs are not sorted")]
    UnsortedScopeRefs,
    #[error("session project {session} does not match graph project {graph}")]
    ProjectMismatch { session: Id, graph: Id },
    #[error("scope ref {0} does not exist")]
    MissingScopeRef(Id),
    #[error("scope ref {scope_ref} is {status:?}, not Accepted")]
    ScopeRefNotAccepted {
        scope_ref: Id,
        status: ElementStatus,
    },
    #[error("user turn text is blank")]
    BlankUserText,
    #[error("max_context_bytes must be positive")]
    ZeroContextBudget,

    #[error("artifact store returned {returned} for transcript bytes hashing to {computed}")]
    TranscriptHashMismatch { returned: Hash, computed: Hash },
    #[error("node {0} already exists with other transcript content")]
    TranscriptConflict(Id),
    #[error("transcript proposal {0} unexpectedly requires a reason")]
    TranscriptReasonRequired(Id),
    #[error("mandatory scope context needs {required} bytes, more than the {limit} allowed")]
    ContextBudgetExceeded { required: usize, limit: usize },
    #[error("turn plan is inconsistent: {0}")]
    PlanInvalid(&'static str),
    #[error("the transcript-enriched graph changed the semantic hash")]
    SemanticHashChanged,

    #[error("persisted artifact {reference} is {actual}, expected {expected}")]
    PersistedArtifactKind {
        reference: Hash,
        expected: ArtifactKind,
        actual: ArtifactKind,
    },
    #[error("persisted artifact {reference} has media type {actual:?}")]
    PersistedArtifactMediaType { reference: Hash, actual: String },
    #[error("persisted artifact {reference} cannot be decoded: {reason}")]
    PersistedArtifactDecode { reference: Hash, reason: String },
    #[error("persisted inference request is not the planned request")]
    PersistedRequestMismatch,
    #[error("persisted raw response {persisted} does not match the inference artifact {expected}")]
    PersistedRawResponseMismatch { persisted: Hash, expected: Hash },

    #[error("model output does not decode: {0}")]
    OutputDecode(String),
    #[error("model output version {0} is not 1")]
    OutputVersion(u32),
    #[error("model reply is blank")]
    BlankReply,
    #[error("{0} proposals exceed the session limit")]
    TooManyProposals(usize),
    #[error("proposal {0} is repeated")]
    DuplicateProposal(Id),
    #[error("proposal {proposal} is invalid: {error}")]
    InvalidProposal { proposal: Id, error: ProposalError },
    #[error("proposal {proposal} is for stage {actual}, not {expected}")]
    ProposalStageMismatch {
        proposal: Id,
        expected: StageId,
        actual: StageId,
    },
    #[error("proposal {0} is not based on the current semantic hash")]
    ProposalBaseMismatch(Id),
    #[error("proposal {0} uses AUTO_DERIVATION")]
    AutoDerivationProposal(Id),
    #[error("proposal {0} carries derivation refs")]
    ProposalDerivationRefs(Id),
    #[error("proposal {0} cites no evidence")]
    ProposalWithoutEvidence(Id),
    #[error("proposal {proposal} cites evidence {evidence} outside the turn context")]
    EvidenceNotAllowed { proposal: Id, evidence: Id },
    #[error("proposal {proposal} cites {evidence}, not an Accepted EvidenceFragment")]
    EvidenceNotAccepted { proposal: Id, evidence: Id },
    #[error("proposal {0} cites neither the user turn nor previously Accepted evidence")]
    ProposalWithoutTurnProvenance(Id),
    #[error("question {0} was not offered as an open question")]
    QuestionNotAllowed(Id),
    #[error("question {0} is not an Accepted Open Question")]
    QuestionNotOpen(Id),

    #[error("inspected output belongs to request {inspected}, not {planned}")]
    InspectedRequestMismatch { inspected: Hash, planned: Hash },
    #[error("plan belongs to session {plan}, not {session}")]
    SessionMismatch { plan: Id, session: Id },
    #[error("intake input for proposal {0} is repeated")]
    DuplicateIntakeInput(Id),
    #[error("no intake input for proposal {0}")]
    MissingIntakeInput(Id),
    #[error("intake input for unknown proposal {0}")]
    UnknownIntakeInput(Id),
    #[error("intake of proposal {proposal} failed: {error}")]
    ProposalIntake { proposal: Id, error: IntakeError },
}
