//! The session turn pipeline (plan X1.4, Hotfix 060): transcript materialization, base-graph
//! retrieval and bounded context, inference request preparation, verification of an externally
//! persisted acquisition, typed output and provenance validation, X1.3 intake of every
//! proposal and the assistant transcript.
//!
//! Provider execution and inference-bundle persistence stay with the caller. The only direct
//! writes are immutable transcript source artifacts; the graph is never written and nothing is
//! accepted.

use std::collections::{BTreeMap, BTreeSet};

use plumb_artifacts::{Artifact, ArtifactKind, ArtifactStore};
use plumb_core::{to_canonical_json, Clock, Hash, Id, StageId, Timestamp};
use plumb_inference::{
    InferenceArtifact, InferenceRequest, InferenceRequestMaterial, PersistedInferenceRefs,
    ProviderExecution, JSON_MEDIA_TYPE,
};
use plumb_intake::{intake, IntakeReport, IntakeValidationInput, LexicalIndex};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    evidence_fragment_id, source_artifact_id, AuditMeta, ElementStatus, EvidenceFragment,
    EvidenceLocator, Graph, Node, NodePayload, SourceArtifact,
};
use serde_json::{json, Value};

use crate::model::{
    InspectedSessionOutput, Session, SessionContextItem, SessionContextSource, SessionError,
    SessionInferenceContext, SessionIntakeInputs, SessionModelOutput, SessionProposalCard,
    SessionRole, SessionTurnConfig, SessionTurnPlan, SessionTurnResult, SessionUserTurn,
    TranscriptTurn, SESSION_CONTEXT_VERSION, SESSION_MAX_PROPOSALS, SESSION_OUTPUT_VERSION,
    SESSION_TURN_TASK_KIND, TRANSCRIPT_MEDIA_TYPE, TRANSCRIPT_SOURCE_KIND, TRANSCRIPT_VERSION,
};

/// The committed session-turn prompt template.
const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/session-turn.md");
/// The committed session-turn output schema.
const OUTPUT_SCHEMA: &[u8] = include_bytes!("../../../schemas/inference/session-turn.schema.json");
const QUESTION_OPEN: &str = "Open";

// ============================================================================ transcript

/// The canonical transcript envelope bytes of one turn.
fn transcript_bytes(
    session_ref: &Id,
    role: SessionRole,
    actor_ref: &Id,
    timestamp: Timestamp,
    text: &str,
) -> Result<Vec<u8>, SessionError> {
    Ok(to_canonical_json(&json!({
        "version": TRANSCRIPT_VERSION,
        "session_ref": session_ref,
        "role": role,
        "actor_ref": actor_ref,
        "timestamp": timestamp,
        "text": text,
    }))?)
}

/// The turn, source and evidence identities derived from the transcript artifact hash.
fn transcript_turn(
    source_artifact_ref: Hash,
    role: SessionRole,
    actor_ref: &Id,
    timestamp: Timestamp,
    text: &str,
) -> Result<TranscriptTurn, SessionError> {
    let digest = source_artifact_ref
        .as_str()
        .rsplit(':')
        .next()
        .unwrap_or_default();
    let turn_ref: Id = format!("turn:{}", &digest[..16.min(digest.len())]).parse()?;
    let source_node_ref = source_artifact_id(&source_artifact_ref)?;
    let evidence_ref = evidence_fragment_id(&source_node_ref, &locator(&turn_ref))?;
    Ok(TranscriptTurn {
        turn_ref,
        role,
        actor_ref: actor_ref.clone(),
        timestamp,
        source_artifact_ref,
        source_node_ref,
        evidence_ref,
        text: text.to_owned(),
    })
}

fn locator(turn_ref: &Id) -> EvidenceLocator {
    EvidenceLocator::ConversationTurn {
        turn_id: turn_ref.clone(),
    }
}

/// Requires the turn identities to be exactly those of its transcript envelope.
fn check_turn(
    turn: &TranscriptTurn,
    session_ref: &Id,
    role: SessionRole,
) -> Result<(), SessionError> {
    let bytes = transcript_bytes(
        session_ref,
        role,
        &turn.actor_ref,
        turn.timestamp,
        &turn.text,
    )?;
    let expected = transcript_turn(
        Hash::content_sha256(&bytes),
        role,
        &turn.actor_ref,
        turn.timestamp,
        &turn.text,
    )?;
    if &expected != turn {
        return Err(SessionError::PlanInvalid("transcript turn identities"));
    }
    Ok(())
}

/// The Accepted SourceArtifact and EvidenceFragment nodes of a turn.
fn transcript_nodes(turn: &TranscriptTurn) -> Result<[Node; 2], SessionError> {
    let node = |id: &Id, payload: NodePayload| -> Result<Node, SessionError> {
        Ok(Node {
            id: id.clone(),
            revision: 1,
            status: ElementStatus::Accepted,
            payload,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::new(),
            audit: AuditMeta::new(turn.actor_ref.clone(), turn.timestamp, None, None)?,
        })
    };
    let role = turn.role.as_str().to_owned();
    Ok([
        node(
            &turn.source_node_ref,
            NodePayload::SourceArtifact(SourceArtifact {
                source_kind: TRANSCRIPT_SOURCE_KIND.to_owned(),
                display_name: format!("Transcript {}", turn.turn_ref),
                content_hash: turn.source_artifact_ref.clone(),
                media_type: TRANSCRIPT_MEDIA_TYPE.to_owned(),
                external_uri: None,
                external_version: None,
                producer: Some(role.clone()),
                created_at_source: Some(turn.timestamp),
                language: None,
                classification: None,
            }),
        )?,
        node(
            &turn.evidence_ref,
            NodePayload::EvidenceFragment(EvidenceFragment {
                source_ref: turn.source_node_ref.clone(),
                locator: locator(&turn.turn_ref),
                content_hash: Hash::content_sha256(turn.text.as_bytes()),
                extracted_text: Some(turn.text.clone()),
                speaker: Some(role),
                source_timestamp: Some(turn.timestamp),
            }),
        )?,
    ])
}

/// The non-semantic proposal adding the transcript nodes `graph` lacks, or `None` when both
/// already exist exactly. Any other node at a transcript ID is a conflict.
fn transcript_proposal(
    graph: &Graph,
    turn: &TranscriptTurn,
    stage: StageId,
) -> Result<Option<Proposal>, SessionError> {
    let mut patches = Vec::new();
    for expected in transcript_nodes(turn)? {
        match graph.node(&expected.id) {
            None => patches.push(SemanticPatch::AddNode { node: expected }),
            Some(existing)
                if existing.status == ElementStatus::Accepted
                    && existing.payload == expected.payload => {}
            Some(existing) => return Err(SessionError::TranscriptConflict(existing.id.clone())),
        }
    }
    let patch = match patches.len() {
        0 => return Ok(None),
        1 => patches.remove(0),
        _ => SemanticPatch::Compound { patches },
    };
    let proposal = Proposal::new(
        stage,
        PatchSet {
            base_semantic_hash: graph.semantic_hash()?,
            patch,
        },
        Vec::new(),
        Vec::new(),
        ProposalMateriality::NonSemantic,
        AcceptancePolicy::AutoNonSemantic,
        None,
    )
    .map_err(|error| SessionError::InvalidProposal {
        proposal: turn.turn_ref.clone(),
        error,
    })?;
    Ok(Some(proposal))
}

struct Transcript {
    turn: TranscriptTurn,
    proposal: Option<Proposal>,
    intake: Option<IntakeReport>,
}

/// Persists the canonical transcript of one turn, then derives its nodes, the proposal adding
/// them and its intake, and checks the in-memory transcript-enriched graph. Shared by user and
/// assistant turns.
#[allow(clippy::too_many_arguments)]
fn materialize_transcript<S: ArtifactStore>(
    store: &mut S,
    graph: &Graph,
    session_ref: &Id,
    role: SessionRole,
    actor_ref: &Id,
    timestamp: Timestamp,
    text: &str,
    stage: StageId,
) -> Result<Transcript, SessionError> {
    let bytes = transcript_bytes(session_ref, role, actor_ref, timestamp, text)?;
    let stored = store.put(
        ArtifactKind::SourceOriginal,
        TRANSCRIPT_MEDIA_TYPE,
        &bytes,
        timestamp,
    )?;
    let computed = Hash::content_sha256(&bytes);
    if stored != computed {
        return Err(SessionError::TranscriptHashMismatch {
            returned: stored,
            computed,
        });
    }
    let turn = transcript_turn(stored, role, actor_ref, timestamp, text)?;
    let proposal = transcript_proposal(graph, &turn, stage)?;
    let intake = match &proposal {
        None => None,
        Some(p) => {
            let report = intake(graph, p, &IntakeValidationInput::empty())?;
            if report.requires_reason {
                return Err(SessionError::TranscriptReasonRequired(p.id.clone()));
            }
            // The in-memory transcript-enriched graph must keep the semantic hash.
            enriched(graph, Some(p))?;
            Some(report)
        }
    };
    Ok(Transcript {
        turn,
        proposal,
        intake,
    })
}

/// `graph` with the transcript proposal applied in memory; the semantic hash must not change.
fn enriched(graph: &Graph, proposal: Option<&Proposal>) -> Result<Graph, SessionError> {
    let Some(proposal) = proposal else {
        return Ok(graph.clone());
    };
    let candidate = apply_patch(graph, &proposal.patch_set)?.graph;
    if candidate.semantic_hash()? != graph.semantic_hash()? {
        return Err(SessionError::SemanticHashChanged);
    }
    Ok(candidate)
}

// ============================================================================ context

fn check_session(graph: &Graph, session: &Session) -> Result<(), SessionError> {
    session.validate()?;
    if &session.project_ref != graph.project_id() {
        return Err(SessionError::ProjectMismatch {
            session: session.project_ref.clone(),
            graph: graph.project_id().clone(),
        });
    }
    Ok(())
}

fn check_scope(graph: &Graph, session: &Session) -> Result<(), SessionError> {
    for scope_ref in &session.scope_refs {
        let node = graph
            .node(scope_ref)
            .ok_or_else(|| SessionError::MissingScopeRef(scope_ref.clone()))?;
        if node.status != ElementStatus::Accepted {
            return Err(SessionError::ScopeRefNotAccepted {
                scope_ref: scope_ref.clone(),
                status: node.status,
            });
        }
    }
    Ok(())
}

fn context_item(
    source: SessionContextSource,
    node: &Node,
) -> Result<SessionContextItem, SessionError> {
    let mut wire = serde_json::to_value(&node.payload)
        .map_err(|e| plumb_core::CoreError::Canonicalization(e.to_string()))?;
    let node_type = wire
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let payload = wire.get_mut("data").map(Value::take).unwrap_or(Value::Null);
    Ok(SessionContextItem {
        source,
        node_ref: node.id.clone(),
        node_type,
        payload,
    })
}

fn item_bytes(item: &SessionContextItem) -> Result<usize, SessionError> {
    Ok(to_canonical_json(item)?.len())
}

fn is_open_question(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
        && matches!(&node.payload, NodePayload::Question(q) if q.status == QUESTION_OPEN)
}

fn is_accepted_evidence(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
        && matches!(node.payload, NodePayload::EvidenceFragment(_))
}

/// Selects the context items of a turn: every scope node (mandatory), then open questions,
/// then retrieval hits, each optional item kept whole only when it still fits the budget.
fn select_context(
    graph: &Graph,
    session: &Session,
    text: &str,
    config: &SessionTurnConfig,
) -> Result<(Vec<SessionContextItem>, usize), SessionError> {
    let mut items = Vec::new();
    let mut selected = BTreeSet::new();
    let mut used = 0usize;
    for scope_ref in &session.scope_refs {
        let node = graph
            .node(scope_ref)
            .ok_or_else(|| SessionError::MissingScopeRef(scope_ref.clone()))?;
        let item = context_item(SessionContextSource::Scope, node)?;
        used += item_bytes(&item)?;
        selected.insert(scope_ref.clone());
        items.push(item);
    }
    if used > config.max_context_bytes {
        return Err(SessionError::ContextBudgetExceeded {
            required: used,
            limit: config.max_context_bytes,
        });
    }

    let scope: BTreeSet<&Id> = session.scope_refs.iter().collect();
    let questions: Vec<&Node> = graph
        .nodes()
        .values()
        .filter(|node| is_open_question(node) && !selected.contains(&node.id))
        .filter(|node| {
            scope.is_empty()
                || matches!(&node.payload, NodePayload::Question(q)
                    if q.context_refs.iter().flatten().any(|r| scope.contains(r)))
        })
        .take(config.max_open_questions)
        .collect();
    let mut optional: Vec<(SessionContextSource, &Node)> = questions
        .into_iter()
        .map(|node| (SessionContextSource::OpenQuestion, node))
        .collect();

    if config.retrieval_top_k > 0 {
        let index = LexicalIndex::build(graph)?;
        for hit in index.search(text, config.retrieval_top_k)? {
            if let Some(node) = graph.node(&hit.id) {
                optional.push((SessionContextSource::Retrieval, node));
            }
        }
    }
    for (source, node) in optional {
        if selected.contains(&node.id) {
            continue;
        }
        let item = context_item(source, node)?;
        let size = item_bytes(&item)?;
        if used + size <= config.max_context_bytes {
            used += size;
            selected.insert(node.id.clone());
            items.push(item);
        }
    }
    Ok((items, used))
}

/// The open questions and evidence the selected context exposes, with the user turn first.
fn allowed_refs(
    graph: &Graph,
    items: &[SessionContextItem],
    user_evidence_ref: &Id,
) -> (Vec<Id>, Vec<Id>) {
    let mut questions = BTreeSet::new();
    let mut evidence = BTreeSet::from([user_evidence_ref.clone()]);
    for item in items {
        let Some(node) = graph.node(&item.node_ref) else {
            continue;
        };
        if is_open_question(node) {
            questions.insert(node.id.clone());
        }
        if is_accepted_evidence(node) {
            evidence.insert(node.id.clone());
        }
        for reference in &node.evidence {
            if graph
                .node(reference.as_id())
                .is_some_and(is_accepted_evidence)
            {
                evidence.insert(reference.as_id().clone());
            }
        }
    }
    (
        questions.into_iter().collect(),
        evidence.into_iter().collect(),
    )
}

// ============================================================================ prepare

/// Prepares one user turn: persists its transcript, retrieves and bounds its context from
/// `graph`, and builds the inference request and material the caller acquires externally.
/// Reads `clock` exactly once; never writes the graph or calls a provider.
pub fn prepare_turn<S: ArtifactStore>(
    graph: &Graph,
    session: &Session,
    turn: &SessionUserTurn,
    config: &SessionTurnConfig,
    store: &mut S,
    clock: &dyn Clock,
) -> Result<SessionTurnPlan, SessionError> {
    check_session(graph, session)?;
    if turn.text.trim().is_empty() {
        return Err(SessionError::BlankUserText);
    }
    turn.provider_policy.validate()?;
    config.validate()?;
    check_scope(graph, session)?;

    let timestamp = clock.now();
    let transcript = materialize_transcript(
        store,
        graph,
        &session.id,
        SessionRole::User,
        &turn.user_actor_ref,
        timestamp,
        &turn.text,
        turn.active_stage,
    )?;
    let user_turn = transcript.turn;

    // Retrieval and context come from the base graph, before the current transcript.
    let (items, context_item_bytes) = select_context(graph, session, &turn.text, config)?;
    let (allowed_open_question_refs, allowed_evidence_refs) =
        allowed_refs(graph, &items, &user_turn.evidence_ref);
    let context = SessionInferenceContext {
        version: SESSION_CONTEXT_VERSION,
        session_ref: session.id.clone(),
        project_ref: graph.project_id().clone(),
        profile_ref: graph.profile_id().clone(),
        base_semantic_hash: graph.semantic_hash()?,
        active_stage: turn.active_stage,
        user_turn_ref: user_turn.turn_ref.clone(),
        user_evidence_ref: user_turn.evidence_ref.clone(),
        user_text: turn.text.clone(),
        scope_refs: session.scope_refs.clone(),
        items,
        allowed_open_question_refs,
        allowed_evidence_refs,
        context_item_bytes,
    };
    let (request, material) = request_for(&context, turn)?;
    let plan = SessionTurnPlan {
        session_ref: session.id.clone(),
        active_stage: turn.active_stage,
        user_turn,
        user_transcript_proposal: transcript.proposal,
        user_transcript_intake: transcript.intake,
        context,
        request,
        material,
    };
    plan.validate(graph)?;
    Ok(plan)
}

fn request_for(
    context: &SessionInferenceContext,
    turn: &SessionUserTurn,
) -> Result<(InferenceRequest, InferenceRequestMaterial), SessionError> {
    let context_json = to_canonical_json(context)?;
    let request = InferenceRequest::new(
        turn.active_stage,
        SESSION_TURN_TASK_KIND.to_owned(),
        context.items.iter().map(|i| i.node_ref.clone()).collect(),
        context.allowed_evidence_refs.clone(),
        Hash::content_sha256(&context_json),
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(OUTPUT_SCHEMA),
        turn.provider_policy.clone(),
    )?;
    let material = InferenceRequestMaterial::new(
        &request,
        context_json,
        PROMPT_TEMPLATE.to_vec(),
        OUTPUT_SCHEMA.to_vec(),
    )?;
    Ok((request, material))
}

impl SessionTurnPlan {
    /// Checks that the plan is internally bound (session, stage, transcript identities,
    /// context, request and material) and that its transcript proposal is exactly the one
    /// `graph` needs.
    pub fn validate(&self, graph: &Graph) -> Result<(), SessionError> {
        let invalid = SessionError::PlanInvalid;
        let context = &self.context;
        check_turn(&self.user_turn, &self.session_ref, SessionRole::User)?;
        if context.version != SESSION_CONTEXT_VERSION
            || context.session_ref != self.session_ref
            || context.active_stage != self.active_stage
            || self.request.stage != self.active_stage
        {
            return Err(invalid("session or stage binding"));
        }
        if &context.project_ref != graph.project_id()
            || &context.profile_ref != graph.profile_id()
            || context.base_semantic_hash != graph.semantic_hash()?
        {
            return Err(invalid("graph binding"));
        }
        if context.user_turn_ref != self.user_turn.turn_ref
            || context.user_evidence_ref != self.user_turn.evidence_ref
            || context.user_text != self.user_turn.text
            || !context
                .allowed_evidence_refs
                .contains(&self.user_turn.evidence_ref)
        {
            return Err(invalid("user turn binding"));
        }
        let mut item_bytes_total = 0;
        for item in &context.items {
            let node = graph
                .node(&item.node_ref)
                .ok_or(invalid("context item node"))?;
            if &context_item(item.source, node)? != item {
                return Err(invalid("context item payload"));
            }
            item_bytes_total += item_bytes(item)?;
        }
        let (questions, evidence) =
            allowed_refs(graph, &context.items, &self.user_turn.evidence_ref);
        if questions != context.allowed_open_question_refs
            || evidence != context.allowed_evidence_refs
        {
            return Err(invalid("allowed refs"));
        }
        if item_bytes_total != context.context_item_bytes {
            return Err(invalid("context byte count"));
        }
        self.request.validate()?;
        self.material.validate_for(&self.request)?;
        let expected_inputs: BTreeSet<&Id> = context.items.iter().map(|i| &i.node_ref).collect();
        if self.request.task_kind != SESSION_TURN_TASK_KIND
            || self.request.input_refs.iter().collect::<BTreeSet<_>>() != expected_inputs
            || self.request.input_refs.len() != context.items.len()
            || self.request.evidence_refs != context.allowed_evidence_refs
            || self.request.context_hash != Hash::content_sha256(&to_canonical_json(context)?)
            || self.material.context_json != to_canonical_json(context)?
            || self.material.prompt_template != PROMPT_TEMPLATE
            || self.material.schema_json != OUTPUT_SCHEMA
        {
            return Err(invalid("request binding"));
        }
        let expected = transcript_proposal(graph, &self.user_turn, self.active_stage)?;
        if expected != self.user_transcript_proposal {
            return Err(invalid("user transcript proposal"));
        }
        let intake_bound = match (&self.user_transcript_proposal, &self.user_transcript_intake) {
            (None, None) => true,
            (Some(p), Some(report)) => report.proposal_ref == p.id && !report.requires_reason,
            _ => false,
        };
        if !intake_bound {
            return Err(invalid("user transcript intake"));
        }
        Ok(())
    }
}

// ============================================================================ inspect

fn load(
    store: &impl ArtifactStore,
    reference: &Hash,
    kind: ArtifactKind,
) -> Result<Artifact, SessionError> {
    let artifact = store.get(reference)?;
    if artifact.kind != kind {
        return Err(SessionError::PersistedArtifactKind {
            reference: reference.clone(),
            expected: kind,
            actual: artifact.kind,
        });
    }
    Ok(artifact)
}

fn load_json<T: serde::de::DeserializeOwned>(
    store: &impl ArtifactStore,
    reference: &Hash,
    kind: ArtifactKind,
) -> Result<T, SessionError> {
    let artifact = load(store, reference, kind)?;
    if artifact.media_type != JSON_MEDIA_TYPE {
        return Err(SessionError::PersistedArtifactMediaType {
            reference: reference.clone(),
            actual: artifact.media_type,
        });
    }
    serde_json::from_slice(&artifact.bytes).map_err(|e| SessionError::PersistedArtifactDecode {
        reference: reference.clone(),
        reason: e.to_string(),
    })
}

/// Loads and verifies the three persisted artifacts of the plan's acquisition.
fn verified_acquisition(
    plan: &SessionTurnPlan,
    store: &impl ArtifactStore,
    persisted: &PersistedInferenceRefs,
) -> Result<InferenceArtifact, SessionError> {
    persisted.validate()?;
    let request: InferenceRequest = load_json(
        store,
        &persisted.request_artifact_ref,
        ArtifactKind::InferenceRequest,
    )?;
    if request != plan.request {
        return Err(SessionError::PersistedRequestMismatch);
    }
    let artifact: InferenceArtifact = load_json(
        store,
        &persisted.validated_inference_ref,
        ArtifactKind::ValidatedInference,
    )?;
    artifact.validate_for(&plan.request)?;
    let raw = load(
        store,
        &persisted.raw_response_ref,
        ArtifactKind::InferenceResponse,
    )?;
    if raw.hash != persisted.raw_response_ref || raw.hash != artifact.raw_response_hash {
        return Err(SessionError::PersistedRawResponseMismatch {
            persisted: raw.hash,
            expected: artifact.raw_response_hash,
        });
    }
    ProviderExecution {
        artifact: artifact.clone(),
        raw_response_media_type: raw.media_type,
        raw_response: raw.bytes,
    }
    .validate_for(&plan.request)?;
    Ok(artifact)
}

/// Checks the decoded output against the plan, the original graph and the
/// transcript-enriched turn graph.
fn check_output(
    graph: &Graph,
    turn_graph: &Graph,
    plan: &SessionTurnPlan,
    reply: &str,
    proposals: &[Proposal],
    asked_question_ref: Option<&Id>,
) -> Result<(), SessionError> {
    if reply.trim().is_empty() {
        return Err(SessionError::BlankReply);
    }
    if proposals.len() > SESSION_MAX_PROPOSALS {
        return Err(SessionError::TooManyProposals(proposals.len()));
    }
    let mut seen = BTreeSet::new();
    for proposal in proposals {
        if !seen.insert(&proposal.id) {
            return Err(SessionError::DuplicateProposal(proposal.id.clone()));
        }
    }
    let base = graph.semantic_hash()?;
    let allowed: BTreeSet<&Id> = plan.context.allowed_evidence_refs.iter().collect();
    for proposal in proposals {
        let id = &proposal.id;
        proposal
            .validate()
            .map_err(|error| SessionError::InvalidProposal {
                proposal: id.clone(),
                error,
            })?;
        if proposal.stage != plan.active_stage {
            return Err(SessionError::ProposalStageMismatch {
                proposal: id.clone(),
                expected: plan.active_stage,
                actual: proposal.stage,
            });
        }
        if proposal.patch_set.base_semantic_hash != base
            || proposal.patch_set.base_semantic_hash != turn_graph.semantic_hash()?
        {
            return Err(SessionError::ProposalBaseMismatch(id.clone()));
        }
        if proposal.acceptance_policy == AcceptancePolicy::AutoDerivation {
            return Err(SessionError::AutoDerivationProposal(id.clone()));
        }
        if !proposal.derivation_refs.is_empty() {
            return Err(SessionError::ProposalDerivationRefs(id.clone()));
        }
        if proposal.evidence_refs.is_empty() {
            return Err(SessionError::ProposalWithoutEvidence(id.clone()));
        }
        let mut provenance = false;
        for reference in &proposal.evidence_refs {
            let evidence = reference.as_id();
            if !allowed.contains(evidence) {
                return Err(SessionError::EvidenceNotAllowed {
                    proposal: id.clone(),
                    evidence: evidence.clone(),
                });
            }
            if !turn_graph.node(evidence).is_some_and(is_accepted_evidence) {
                return Err(SessionError::EvidenceNotAccepted {
                    proposal: id.clone(),
                    evidence: evidence.clone(),
                });
            }
            provenance |= evidence == &plan.user_turn.evidence_ref
                || graph.node(evidence).is_some_and(is_accepted_evidence);
        }
        if !provenance {
            return Err(SessionError::ProposalWithoutTurnProvenance(id.clone()));
        }
    }
    if let Some(question) = asked_question_ref {
        if !plan.context.allowed_open_question_refs.contains(question) {
            return Err(SessionError::QuestionNotAllowed(question.clone()));
        }
        if !turn_graph.node(question).is_some_and(is_open_question) {
            return Err(SessionError::QuestionNotOpen(question.clone()));
        }
    }
    Ok(())
}

/// Verifies the externally persisted acquisition of `plan` and validates its typed output.
/// Mutates nothing and runs no intake.
pub fn inspect_turn_output<S: ArtifactStore>(
    graph: &Graph,
    plan: &SessionTurnPlan,
    store: &S,
    persisted: &PersistedInferenceRefs,
) -> Result<InspectedSessionOutput, SessionError> {
    plan.validate(graph)?;
    let turn_graph = enriched(graph, plan.user_transcript_proposal.as_ref())?;
    let artifact = verified_acquisition(plan, store, persisted)?;
    let output: SessionModelOutput =
        serde_json::from_value(artifact.validated_output.as_value().clone())
            .map_err(|e| SessionError::OutputDecode(e.to_string()))?;
    if output.version != SESSION_OUTPUT_VERSION {
        return Err(SessionError::OutputVersion(output.version));
    }
    check_output(
        graph,
        &turn_graph,
        plan,
        &output.reply,
        &output.proposals,
        output.asked_question_ref.as_ref(),
    )?;
    Ok(InspectedSessionOutput {
        request_ref: plan.request.id.clone(),
        persisted_inference: persisted.clone(),
        reply: output.reply,
        proposals: output.proposals,
        asked_question_ref: output.asked_question_ref,
    })
}

// ============================================================================ complete

/// Runs X1.3 intake for every inspected proposal with its explicit validation input, then
/// persists the assistant transcript. Reads `clock` exactly once, only after every intake
/// succeeded; returns display material and writes no graph.
pub fn complete_turn<S: ArtifactStore>(
    graph: &Graph,
    session: &Session,
    plan: &SessionTurnPlan,
    inspected: &InspectedSessionOutput,
    intake_inputs: &SessionIntakeInputs,
    store: &mut S,
    clock: &dyn Clock,
) -> Result<SessionTurnResult, SessionError> {
    check_session(graph, session)?;
    if plan.session_ref != session.id {
        return Err(SessionError::SessionMismatch {
            plan: plan.session_ref.clone(),
            session: session.id.clone(),
        });
    }
    plan.validate(graph)?;
    if inspected.request_ref != plan.request.id {
        return Err(SessionError::InspectedRequestMismatch {
            inspected: inspected.request_ref.clone(),
            planned: plan.request.id.clone(),
        });
    }
    let turn_graph = enriched(graph, plan.user_transcript_proposal.as_ref())?;
    check_output(
        graph,
        &turn_graph,
        plan,
        &inspected.reply,
        &inspected.proposals,
        inspected.asked_question_ref.as_ref(),
    )?;
    let proposal_refs: BTreeSet<&Id> = inspected.proposals.iter().map(|p| &p.id).collect();
    for proposal_ref in &proposal_refs {
        if intake_inputs.get(proposal_ref).is_none() {
            return Err(SessionError::MissingIntakeInput((*proposal_ref).clone()));
        }
    }
    if let Some(unknown) = intake_inputs
        .proposal_refs()
        .into_iter()
        .find(|id| !proposal_refs.contains(id))
    {
        return Err(SessionError::UnknownIntakeInput(unknown.clone()));
    }

    let mut cards = Vec::with_capacity(inspected.proposals.len());
    for proposal in &inspected.proposals {
        let input = intake_inputs
            .get(&proposal.id)
            .ok_or_else(|| SessionError::MissingIntakeInput(proposal.id.clone()))?;
        let report =
            intake(&turn_graph, proposal, input).map_err(|error| SessionError::ProposalIntake {
                proposal: proposal.id.clone(),
                error,
            })?;
        cards.push(SessionProposalCard {
            proposal: proposal.clone(),
            intake: report,
        });
    }

    let timestamp = clock.now();
    let assistant = materialize_transcript(
        store,
        &turn_graph,
        &session.id,
        SessionRole::Assistant,
        &session.assistant_actor_ref,
        timestamp,
        &inspected.reply,
        plan.active_stage,
    )?;
    Ok(SessionTurnResult {
        request_ref: plan.request.id.clone(),
        persisted_inference: inspected.persisted_inference.clone(),
        user_turn: plan.user_turn.clone(),
        user_transcript_proposal: plan.user_transcript_proposal.clone(),
        user_transcript_intake: plan.user_transcript_intake.clone(),
        assistant_turn: assistant.turn,
        assistant_transcript_proposal: assistant.proposal,
        assistant_transcript_intake: assistant.intake,
        reply: inspected.reply.clone(),
        proposals: cards,
        asked_question_ref: inspected.asked_question_ref.clone(),
    })
}
