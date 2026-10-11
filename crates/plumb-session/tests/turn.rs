//! X1.4 contract tests for the transcript-as-source session pipeline (Hotfix 060).
//!
//! Every test lives in `turn` (`cargo test -p plumb-session turn`). Acquisitions are mock
//! provider executions persisted with the existing `persist_inference_bundle` into an in-memory
//! artifact store; clocks are fixed or counting. Goldens were computed independently in Python
//! (sorted-key compact `json.dumps`, `hashlib.sha256`).

mod turn {
    use std::cell::Cell;
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_artifacts::{Artifact, ArtifactKind, ArtifactStore, ArtifactStoreError};
    use plumb_core::{
        to_canonical_json, CanonicalJson, Clock, FixedClock, Hash, Id, StageId, Timestamp,
    };
    use plumb_inference::{
        persist_inference_bundle, InferenceArtifact, InferenceProvider, MockProvider,
        PersistedInferenceRefs, ProviderExecution, ProviderPolicy,
    };
    use plumb_intake::{IntakeError, IntakeValidationInput, LexicalIndex};
    use plumb_patch::{
        apply_patch, AcceptancePolicy, ElementPrecondition, PatchError, PatchSet, Proposal,
        ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        evidence_fragment_id, node_element_hash, source_artifact_id, AuditMeta, ElementStatus,
        EvidenceFragment, EvidenceLocator, EvidenceRef, Finding, FindingSeverity, Graph, Modality,
        Node, NodePayload, Question, QuestionKind, Requirement, RequirementKind, RequirementLevel,
        SourceArtifact,
    };
    use plumb_session::*;
    use serde_json::{json, Value};

    use ElementStatus::{Accepted, Proposed, Suspect};

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const USER_AT: &str = "2026-03-01T09:00:00.000000000Z";
    const ASSISTANT_AT: &str = "2026-03-01T09:00:05.000000000Z";
    const ACQUIRED_AT: &str = "2026-03-01T09:00:03.000000000Z";
    const SESSION: &str = "session:s1";
    const ALICE: &str = "agent:alice";
    const ASSISTANT: &str = "agent:assistant";
    const TEXT: &str = "How should Managers approve leave?";
    const LEAVE: &str = "requirement:leave";
    const PAYROLL: &str = "requirement:payroll";
    const NEW_REQ: &str = "requirement:new";

    const PROMPT: &[u8] = include_bytes!("../../../prompts/session-turn.md");
    const SCHEMA: &[u8] = include_bytes!("../../../schemas/inference/session-turn.schema.json");
    const LIB_SRC: &str = include_str!("../src/lib.rs");
    const MODEL_SRC: &str = include_str!("../src/model.rs");
    const TURN_SRC: &str = include_str!("../src/turn.rs");

    // Independent goldens (Python hashlib over the exact files and envelope bytes).
    const PROMPT_HASH: &str =
        "sha256:4d23caab9d199142616e283a7928d099556056fdda8267850a44744ddfba7e0d";
    const SCHEMA_HASH: &str =
        "sha256:a3fd9c17a2bc1fffb333902bcfdabcbd5c75acd359c5b71cdcd2c034cae8ae92";
    const USER_ENVELOPE: &str = "{\"actor_ref\":\"agent:alice\",\"role\":\"user\",\"session_ref\":\"session:s1\",\"text\":\"How should Managers approve leave?\",\"timestamp\":\"2026-03-01T09:00:00.000000000Z\",\"version\":1}";
    const USER_ENVELOPE_HASH: &str =
        "sha256:6cafa82d123a3d6bc9da2d8c3e073ef0b50363c046d3e91cfae3c03b6deab80f";
    const USER_TURN: &str = "turn:6cafa82d123a3d6b";
    const USER_SOURCE: &str = "src:6cafa82d123a3d6b";
    const USER_EVIDENCE: &str = "evd:0d8c37784b4f79cb";
    const USER_TEXT_HASH: &str =
        "sha256:c0f7c2125287246bd446f5cab3497503d03e720295e19da9446b8344e2b4efed";

    // ------------------------------------------------------------------ store and clocks

    /// A deterministic in-memory artifact store with the `ArtifactStore` idempotency rules.
    #[derive(Debug, Clone, Default)]
    struct MemoryStore {
        artifacts: BTreeMap<Hash, Artifact>,
        puts: Vec<(ArtifactKind, String)>,
    }

    impl ArtifactStore for MemoryStore {
        fn put(
            &mut self,
            kind: ArtifactKind,
            media_type: &str,
            bytes: &[u8],
            created_at: Timestamp,
        ) -> Result<Hash, ArtifactStoreError> {
            let hash = Hash::content_sha256(bytes);
            self.puts.push((kind, media_type.to_owned()));
            if let Some(existing) = self.artifacts.get(&hash) {
                if existing.kind != kind || existing.media_type != media_type {
                    return Err(ArtifactStoreError::ArtifactMetadataConflict {
                        hash,
                        existing_kind: existing.kind,
                        existing_media_type: existing.media_type.clone(),
                        requested_kind: kind,
                        requested_media_type: media_type.to_owned(),
                    });
                }
                return Ok(hash);
            }
            self.artifacts.insert(
                hash.clone(),
                Artifact {
                    hash: hash.clone(),
                    kind,
                    media_type: media_type.to_owned(),
                    bytes: bytes.to_vec(),
                    created_at,
                },
            );
            Ok(hash)
        }

        fn get(&self, hash: &Hash) -> Result<Artifact, ArtifactStoreError> {
            self.artifacts
                .get(hash)
                .cloned()
                .ok_or_else(|| ArtifactStoreError::NotFound(hash.clone()))
        }

        fn exists(&self, hash: &Hash) -> Result<bool, ArtifactStoreError> {
            Ok(self.artifacts.contains_key(hash))
        }
    }

    /// A store whose every write fails.
    struct FailingStore;

    impl ArtifactStore for FailingStore {
        fn put(
            &mut self,
            _: ArtifactKind,
            _: &str,
            _: &[u8],
            _: Timestamp,
        ) -> Result<Hash, ArtifactStoreError> {
            Err(ArtifactStoreError::IntegrityViolation(
                Hash::content_sha256(b"x"),
            ))
        }

        fn get(&self, hash: &Hash) -> Result<Artifact, ArtifactStoreError> {
            Err(ArtifactStoreError::NotFound(hash.clone()))
        }

        fn exists(&self, _: &Hash) -> Result<bool, ArtifactStoreError> {
            Ok(false)
        }
    }

    /// A fixed clock that counts its reads.
    struct CountingClock {
        at: Timestamp,
        reads: Cell<usize>,
    }

    impl CountingClock {
        fn new(at: &str) -> CountingClock {
            CountingClock {
                at: ts(at),
                reads: Cell::new(0),
            }
        }
    }

    impl Clock for CountingClock {
        fn now(&self) -> Timestamp {
            self.reads.set(self.reads.get() + 1);
            self.at
        }
    }

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn node(node_id: &str, status: ElementStatus, payload: NodePayload) -> Node {
        Node {
            id: id(node_id),
            revision: 1,
            status,
            payload,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::new(),
            audit: AuditMeta::new(id("agent:analyst"), ts(AT), None, None).unwrap(),
        }
    }

    fn requirement(statement: &str) -> NodePayload {
        NodePayload::Requirement(Requirement {
            statement: statement.into(),
            requirement_kind: RequirementKind::Functional,
            level: RequirementLevel::System,
            modality: Modality::Shall,
            title: None,
            rationale: None,
            priority: None,
            source_identifier: None,
            verification_method: None,
            owner_refs: None,
            stakeholder_refs: None,
        })
    }

    fn req(node_id: &str, status: ElementStatus, statement: &str) -> Node {
        node(node_id, status, requirement(statement))
    }

    fn question(node_id: &str, status: &str, context: &[&str]) -> Node {
        question_with(
            node_id,
            Accepted,
            status,
            context,
            "Which approver applies?",
        )
    }

    fn question_with(
        node_id: &str,
        element_status: ElementStatus,
        status: &str,
        context: &[&str],
        prompt: &str,
    ) -> Node {
        node(
            node_id,
            element_status,
            NodePayload::Question(Question {
                finding_ref: id("finding:approver"),
                question_kind: QuestionKind::Text,
                prompt: prompt.into(),
                status: status.into(),
                answer_schema: None,
                stakeholder_ref: None,
                priority: None,
                round_ref: None,
                context_refs: Some(context.iter().map(|c| id(c)).collect()),
            }),
        )
    }

    /// An Accepted imported source and one Accepted fragment of it with `text`.
    fn imported(name: &str, text: &str) -> (Node, Node) {
        let content = Hash::content_sha256(name.as_bytes());
        let source_id = source_artifact_id(&content).unwrap();
        let locator = EvidenceLocator::TextRange {
            start: 0,
            end: text.len() as u64,
        };
        let fragment_id = evidence_fragment_id(&source_id, &locator).unwrap();
        let source = node(
            source_id.as_str(),
            Accepted,
            NodePayload::SourceArtifact(SourceArtifact {
                source_kind: "markdown".into(),
                display_name: name.into(),
                content_hash: content,
                media_type: "text/markdown".into(),
                external_uri: None,
                external_version: None,
                producer: None,
                created_at_source: None,
                language: None,
                classification: None,
            }),
        );
        let fragment = node(
            fragment_id.as_str(),
            Accepted,
            NodePayload::EvidenceFragment(EvidenceFragment {
                source_ref: source_id,
                locator,
                content_hash: Hash::content_sha256(text.as_bytes()),
                extracted_text: Some(text.into()),
                speaker: None,
                source_timestamp: None,
            }),
        );
        (source, fragment)
    }

    /// The base graph: an evidenced leave requirement, a payroll requirement, a finding,
    /// questions in several states and an unrelated evidence fragment.
    fn base_nodes() -> Vec<Node> {
        let (source, fragment) = imported("policy.md", "Managers approve leave requests.");
        let (other_source, other_fragment) = imported("payroll.md", "Payroll runs monthly.");
        let mut leave = req(LEAVE, Accepted, "Managers approve leave requests.");
        leave.evidence = vec![fragment.id.clone().into()];
        vec![
            source,
            fragment,
            other_source,
            other_fragment,
            leave,
            req(PAYROLL, Accepted, "Payroll exports salaries monthly."),
            node(
                "finding:approver",
                Accepted,
                NodePayload::Finding(Finding {
                    code: "PLUMB.F2.APPROVER".into(),
                    family: "F2".into(),
                    severity: FindingSeverity::Warn,
                    message: "Approver unclear.".into(),
                    status: "Open".into(),
                    affected_refs: vec![id(LEAVE)],
                    standard_rule_ref: None,
                    suggested_resolution: None,
                    waiver_ref: None,
                }),
            ),
            question("question:q1", "Open", &[LEAVE]),
            question("question:q2", "Answered", &[LEAVE]),
            question("question:q3", "Open", &[PAYROLL]),
            question_with("question:q4", Proposed, "Open", &[LEAVE], "Draft?"),
            question("question:q5", "Closed", &[LEAVE]),
            question("question:q6", "Superseded", &[LEAVE]),
        ]
    }

    fn graph_of(nodes: Vec<Node>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, Vec::new()).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn base() -> Graph {
        graph_of(base_nodes())
    }

    fn fragment_of(g: &Graph, name: &str) -> Id {
        let content = Hash::content_sha256(name.as_bytes());
        let source = source_artifact_id(&content).unwrap();
        g.nodes()
            .values()
            .find(|n| matches!(&n.payload, NodePayload::EvidenceFragment(f) if f.source_ref == source))
            .unwrap()
            .id
            .clone()
    }

    fn session(scope: &[&str]) -> Session {
        Session::new(
            id(SESSION),
            id(PROJECT),
            scope.iter().map(|s| id(s)).collect(),
            id(ASSISTANT),
        )
        .unwrap()
    }

    fn policy() -> ProviderPolicy {
        ProviderPolicy {
            provider: "mock".into(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn user_turn(text: &str, stage: StageId) -> SessionUserTurn {
        SessionUserTurn {
            text: text.into(),
            user_actor_ref: id(ALICE),
            active_stage: stage,
            provider_policy: policy(),
        }
    }

    fn config() -> SessionTurnConfig {
        SessionTurnConfig {
            retrieval_top_k: 5,
            max_open_questions: 5,
            max_context_bytes: 100_000,
        }
    }

    struct Fixture {
        graph: Graph,
        session: Session,
        store: MemoryStore,
        plan: SessionTurnPlan,
    }

    fn prepared_with(
        graph: Graph,
        session: Session,
        text: &str,
        stage: StageId,
        config: SessionTurnConfig,
    ) -> Fixture {
        let mut store = MemoryStore::default();
        let plan = prepare_turn(
            &graph,
            &session,
            &user_turn(text, stage),
            &config,
            &mut store,
            &FixedClock::new(ts(USER_AT)),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        Fixture {
            graph,
            session,
            store,
            plan,
        }
    }

    fn prepared() -> Fixture {
        prepared_with(base(), session(&[]), TEXT, StageId::S2, config())
    }

    fn execution(request: &plumb_inference::InferenceRequest, output: Value) -> ProviderExecution {
        let raw_response = to_canonical_json(&output).unwrap();
        let validated_output = CanonicalJson::new(output);
        ProviderExecution {
            artifact: InferenceArtifact {
                request_hash: request.id.clone(),
                provider: request.provider_policy.provider.clone(),
                model: "mock-model".to_owned(),
                parameters: CanonicalJson::new(json!({})),
                raw_response_hash: Hash::content_sha256(&raw_response),
                validated_output_hash: validated_output.content_hash().unwrap(),
                validated_output,
            },
            raw_response_media_type: "application/json".to_owned(),
            raw_response,
        }
    }

    /// The caller's external acquisition: a mock provider execution of the planned request,
    /// persisted with the existing bundle helper.
    fn acquire(f: &mut Fixture, output: Value) -> PersistedInferenceRefs {
        f.plan.material.validate_for(&f.plan.request).unwrap();
        let provider = MockProvider::with_fixtures([(
            f.plan.request.id.clone(),
            execution(&f.plan.request, output),
        )])
        .unwrap();
        let executed = provider.execute(&f.plan.request, &f.plan.material).unwrap();
        persist_inference_bundle(&mut f.store, &f.plan.request, &executed, ts(ACQUIRED_AT)).unwrap()
    }

    fn semantic(f: &Fixture, node_id: &str, statement: &str, evidence: &[&Id]) -> Proposal {
        proposal_as(
            f,
            f.plan.active_stage,
            SemanticPatch::AddNode {
                node: req(node_id, Proposed, statement),
            },
            evidence,
            AcceptancePolicy::HumanConfirm,
        )
    }

    fn proposal_as(
        f: &Fixture,
        stage: StageId,
        patch: SemanticPatch,
        evidence: &[&Id],
        policy: AcceptancePolicy,
    ) -> Proposal {
        Proposal::new(
            stage,
            PatchSet {
                base_semantic_hash: f.graph.semantic_hash().unwrap(),
                patch,
            },
            evidence
                .iter()
                .map(|e| EvidenceRef::from((*e).clone()))
                .collect(),
            Vec::new(),
            ProposalMateriality::Semantic,
            policy,
            None,
        )
        .unwrap()
    }

    /// A proposal revising the Accepted leave requirement's statement.
    fn revise_leave(f: &Fixture, evidence: &[&Id]) -> Proposal {
        let leave = f.graph.node(&id(LEAVE)).unwrap();
        proposal_as(
            f,
            f.plan.active_stage,
            SemanticPatch::ReplacePayload {
                target: ElementPrecondition {
                    id: id(LEAVE),
                    expected_hash: node_element_hash(leave).unwrap(),
                },
                payload: requirement("Managers approve leave requests within two days."),
            },
            evidence,
            AcceptancePolicy::HumanConfirm,
        )
    }

    fn output(reply: &str, proposals: &[&Proposal], question: Option<&str>) -> Value {
        json!({
            "version": 1,
            "reply": reply,
            "proposals": proposals.iter().map(|p| serde_json::to_value(p).unwrap()).collect::<Vec<_>>(),
            "asked_question_ref": question,
        })
    }

    fn inspect(
        f: &Fixture,
        persisted: &PersistedInferenceRefs,
    ) -> Result<InspectedSessionOutput, SessionError> {
        inspect_turn_output(&f.graph, &f.plan, &f.store, persisted)
    }

    fn inspect_value(
        f: &mut Fixture,
        value: Value,
    ) -> Result<InspectedSessionOutput, SessionError> {
        let persisted = acquire(f, value);
        inspect(f, &persisted)
    }

    fn empty_inputs(proposals: &[&Proposal]) -> SessionIntakeInputs {
        SessionIntakeInputs::new(
            proposals
                .iter()
                .map(|p| (p.id.clone(), IntakeValidationInput::empty())),
        )
        .unwrap()
    }

    fn complete(
        f: &mut Fixture,
        inspected: &InspectedSessionOutput,
        inputs: &SessionIntakeInputs,
        clock: &dyn Clock,
    ) -> Result<SessionTurnResult, SessionError> {
        complete_turn(
            &f.graph,
            &f.session,
            &f.plan,
            inspected,
            inputs,
            &mut f.store,
            clock,
        )
    }

    /// The transcript-enriched turn graph, independently re-applied.
    fn turn_graph(f: &Fixture) -> Graph {
        match &f.plan.user_transcript_proposal {
            Some(p) => apply_patch(&f.graph, &p.patch_set).unwrap().graph,
            None => f.graph.clone(),
        }
    }

    fn user_evidence(f: &Fixture) -> Id {
        f.plan.user_turn.evidence_ref.clone()
    }

    fn item_refs(plan: &SessionTurnPlan) -> Vec<(String, String)> {
        plan.context
            .items
            .iter()
            .map(|i| (i.source.to_string(), i.node_ref.to_string()))
            .collect()
    }

    // ------------------------------------------------------------------ transcript

    #[test]
    fn user_transcript_artifact_and_identities() {
        let f = prepared();
        let turn = &f.plan.user_turn;
        assert_eq!(turn.turn_ref, id(USER_TURN));
        assert_eq!(turn.source_node_ref, id(USER_SOURCE));
        assert_eq!(turn.evidence_ref, id(USER_EVIDENCE));
        assert_eq!(turn.source_artifact_ref.as_str(), USER_ENVELOPE_HASH);
        assert_eq!(turn.role, SessionRole::User);
        assert_eq!(turn.actor_ref, id(ALICE));
        assert_eq!(turn.timestamp, ts(USER_AT));
        assert_eq!(turn.text, TEXT);
        let artifact = f.store.get(&turn.source_artifact_ref).unwrap();
        assert_eq!(artifact.kind, ArtifactKind::SourceOriginal);
        assert_eq!(artifact.media_type, "application/vnd.plumb.transcript+json");
        assert_eq!(artifact.media_type, TRANSCRIPT_MEDIA_TYPE);
        assert_eq!(artifact.bytes, USER_ENVELOPE.as_bytes());
        assert_eq!(
            Hash::content_sha256(USER_ENVELOPE.as_bytes()).as_str(),
            USER_ENVELOPE_HASH
        );
        // The only session write so far is the user transcript.
        assert_eq!(
            f.store.puts,
            [(
                ArtifactKind::SourceOriginal,
                TRANSCRIPT_MEDIA_TYPE.to_owned()
            )]
        );
    }

    #[test]
    fn transcript_payloads_and_proposal_policy() {
        let f = prepared();
        let p = f.plan.user_transcript_proposal.as_ref().unwrap();
        assert_eq!(p.stage, StageId::S2);
        assert_eq!(p.materiality, ProposalMateriality::NonSemantic);
        assert_eq!(p.acceptance_policy, AcceptancePolicy::AutoNonSemantic);
        assert!(p.evidence_refs.is_empty());
        assert!(p.derivation_refs.is_empty());
        assert_eq!(p.confidence, None);
        assert_eq!(
            p.patch_set.base_semantic_hash,
            f.graph.semantic_hash().unwrap()
        );
        let SemanticPatch::Compound { patches } = &p.patch_set.patch else {
            panic!("{:?}", p.patch_set.patch);
        };
        let nodes: Vec<&Node> = patches
            .iter()
            .map(|patch| match patch {
                SemanticPatch::AddNode { node } => node,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(nodes.len(), 2);
        let (source, fragment) = (nodes[0], nodes[1]);
        for n in [source, fragment] {
            assert_eq!(n.status, Accepted);
            assert_eq!(n.revision, 1);
            assert_eq!(
                n.audit,
                AuditMeta::new(id(ALICE), ts(USER_AT), None, None).unwrap()
            );
            assert!(n.evidence.is_empty() && n.derivations.is_empty() && n.tags.is_empty());
        }
        assert_eq!(source.id, id(USER_SOURCE));
        assert_eq!(
            source.payload,
            NodePayload::SourceArtifact(SourceArtifact {
                source_kind: "transcript".into(),
                display_name: format!("Transcript {USER_TURN}"),
                content_hash: USER_ENVELOPE_HASH.parse().unwrap(),
                media_type: "application/vnd.plumb.transcript+json".into(),
                external_uri: None,
                external_version: None,
                producer: Some("user".into()),
                created_at_source: Some(ts(USER_AT)),
                language: None,
                classification: None,
            })
        );
        assert_eq!(fragment.id, id(USER_EVIDENCE));
        assert_eq!(
            fragment.payload,
            NodePayload::EvidenceFragment(EvidenceFragment {
                source_ref: id(USER_SOURCE),
                locator: EvidenceLocator::ConversationTurn {
                    turn_id: id(USER_TURN)
                },
                content_hash: USER_TEXT_HASH.parse().unwrap(),
                extracted_text: Some(TEXT.into()),
                speaker: Some("user".into()),
                source_timestamp: Some(ts(USER_AT)),
            })
        );
        // Transcript intake: no reason.
        let intake = f.plan.user_transcript_intake.as_ref().unwrap();
        assert_eq!(intake.proposal_ref, p.id);
        assert!(!intake.requires_reason);
    }

    #[test]
    fn transcript_keeps_semantic_hash_and_changes_evidence_hash() {
        let f = prepared();
        let enriched = turn_graph(&f);
        assert_eq!(
            enriched.semantic_hash().unwrap(),
            f.graph.semantic_hash().unwrap()
        );
        assert_ne!(
            enriched.evidence_hash().unwrap(),
            f.graph.evidence_hash().unwrap()
        );
        assert_eq!(enriched.node(&id(USER_EVIDENCE)).unwrap().status, Accepted);
    }

    #[test]
    fn transcript_is_idempotent_against_existing_exact_nodes() {
        let f = prepared();
        let enriched = turn_graph(&f);
        let again = prepared_with(enriched.clone(), session(&[]), TEXT, StageId::S2, config());
        assert_eq!(again.plan.user_transcript_proposal, None);
        assert_eq!(again.plan.user_transcript_intake, None);
        assert_eq!(again.plan.user_turn, f.plan.user_turn);
        // Only the SourceArtifact present: only the fragment is proposed.
        let source = enriched.node(&id(USER_SOURCE)).unwrap().clone();
        let mut nodes = base_nodes();
        nodes.push(source);
        let partial = prepared_with(graph_of(nodes), session(&[]), TEXT, StageId::S2, config());
        let p = partial.plan.user_transcript_proposal.unwrap();
        assert!(
            matches!(&p.patch_set.patch, SemanticPatch::AddNode { node } if node.id == id(USER_EVIDENCE))
        );
    }

    #[test]
    fn transcript_collision_is_a_conflict() {
        let f = prepared();
        let enriched = turn_graph(&f);
        let mut source = enriched.node(&id(USER_SOURCE)).unwrap().clone();
        if let NodePayload::SourceArtifact(s) = &mut source.payload {
            s.display_name = "Something else".into();
        }
        let mut nodes = base_nodes();
        nodes.push(source.clone());
        let conflicting = graph_of(nodes);
        let err = prepare_turn(
            &conflicting,
            &session(&[]),
            &user_turn(TEXT, StageId::S2),
            &config(),
            &mut MemoryStore::default(),
            &FixedClock::new(ts(USER_AT)),
        )
        .unwrap_err();
        assert!(
            matches!(err, SessionError::TranscriptConflict(ref n) if *n == id(USER_SOURCE)),
            "{err:?}"
        );
        // Same payload but not Accepted is also a conflict.
        let mut proposed = enriched.node(&id(USER_SOURCE)).unwrap().clone();
        proposed.status = Proposed;
        let mut nodes = base_nodes();
        nodes.push(proposed);
        let err = prepare_turn(
            &graph_of(nodes),
            &session(&[]),
            &user_turn(TEXT, StageId::S2),
            &config(),
            &mut MemoryStore::default(),
            &FixedClock::new(ts(USER_AT)),
        )
        .unwrap_err();
        assert!(
            matches!(err, SessionError::TranscriptConflict(_)),
            "{err:?}"
        );
    }

    #[test]
    fn transcript_store_failure_prepares_nothing() {
        let err = prepare_turn(
            &base(),
            &session(&[]),
            &user_turn(TEXT, StageId::S2),
            &config(),
            &mut FailingStore,
            &FixedClock::new(ts(USER_AT)),
        )
        .unwrap_err();
        assert!(matches!(err, SessionError::Artifact(_)), "{err:?}");
    }

    // ------------------------------------------------------------------ session input

    #[test]
    fn session_input_errors() {
        let g = base();
        let run = |s: &Session, turn: SessionUserTurn, cfg: SessionTurnConfig| {
            let clock = CountingClock::new(USER_AT);
            let mut store = MemoryStore::default();
            let result = prepare_turn(&g, s, &turn, &cfg, &mut store, &clock);
            (result, clock.reads.get(), store.puts.len())
        };
        let other = Session::new(id(SESSION), id("project:other"), vec![], id(ASSISTANT)).unwrap();
        let (r, reads, puts) = run(&other, user_turn(TEXT, StageId::S2), config());
        assert!(matches!(r, Err(SessionError::ProjectMismatch { .. })));
        assert_eq!((reads, puts), (0, 0));
        let (r, reads, _) = run(&session(&[]), user_turn(" \n\t ", StageId::S2), config());
        assert!(matches!(r, Err(SessionError::BlankUserText)));
        assert_eq!(reads, 0);
        let zero = SessionTurnConfig {
            max_context_bytes: 0,
            ..config()
        };
        assert!(matches!(
            run(&session(&[]), user_turn(TEXT, StageId::S2), zero).0,
            Err(SessionError::ZeroContextBudget)
        ));
        assert!(matches!(
            run(
                &session(&["requirement:missing"]),
                user_turn(TEXT, StageId::S2),
                config()
            )
            .0,
            Err(SessionError::MissingScopeRef(_))
        ));
        assert!(matches!(
            run(
                &session(&["question:q4"]),
                user_turn(TEXT, StageId::S2),
                config()
            )
            .0,
            Err(SessionError::ScopeRefNotAccepted {
                status: Proposed,
                ..
            })
        ));
        let mut nodes = base_nodes();
        nodes.push(req("requirement:suspect", Suspect, "Suspect requirement."));
        let gs = graph_of(nodes);
        let r = prepare_turn(
            &gs,
            &session(&["requirement:suspect"]),
            &user_turn(TEXT, StageId::S2),
            &config(),
            &mut MemoryStore::default(),
            &FixedClock::new(ts(USER_AT)),
        );
        assert!(matches!(
            r,
            Err(SessionError::ScopeRefNotAccepted {
                status: Suspect,
                ..
            })
        ));
        // Duplicate scope refs.
        assert!(matches!(
            Session::new(
                id(SESSION),
                id(PROJECT),
                vec![id(LEAVE), id(LEAVE)],
                id(ASSISTANT)
            ),
            Err(SessionError::DuplicateScopeRef(_))
        ));
        // Deserialization keeps the invariants.
        let bad = json!({"id": SESSION, "project_ref": PROJECT,
            "scope_refs": [PAYROLL, LEAVE], "assistant_actor_ref": ASSISTANT});
        assert!(serde_json::from_value::<Session>(bad).is_err());
        let extra = json!({"id": SESSION, "project_ref": PROJECT, "scope_refs": [],
            "assistant_actor_ref": ASSISTANT, "x": 1});
        assert!(serde_json::from_value::<Session>(extra).is_err());
    }

    #[test]
    fn scope_order_does_not_matter() {
        let a = Session::new(
            id(SESSION),
            id(PROJECT),
            vec![id(PAYROLL), id(LEAVE)],
            id(ASSISTANT),
        )
        .unwrap();
        let b = session(&[LEAVE, PAYROLL]);
        assert_eq!(a, b);
        assert_eq!(a.scope_refs, [id(LEAVE), id(PAYROLL)]);
        let fa = prepared_with(base(), a, TEXT, StageId::S2, config());
        let fb = prepared_with(base(), b, TEXT, StageId::S2, config());
        assert_eq!(fa.plan, fb.plan);
        let scope: Vec<_> = item_refs(&fa.plan).into_iter().take(2).collect();
        assert_eq!(
            scope,
            [
                ("scope".to_owned(), LEAVE.to_owned()),
                ("scope".to_owned(), PAYROLL.to_owned())
            ]
        );
    }

    // ------------------------------------------------------------------ context

    #[test]
    fn retrieval_uses_the_raw_text_against_the_base_corpus() {
        let f = prepared_with(
            base(),
            session(&[]),
            TEXT,
            StageId::S2,
            SessionTurnConfig {
                max_open_questions: 0,
                ..config()
            },
        );
        let expected: Vec<String> = LexicalIndex::build(&f.graph)
            .unwrap()
            .search(TEXT, 5)
            .unwrap()
            .into_iter()
            .map(|h| h.id.to_string())
            .collect();
        assert!(!expected.is_empty());
        let got: Vec<String> = f
            .plan
            .context
            .items
            .iter()
            .map(|i| i.node_ref.to_string())
            .collect();
        assert_eq!(got, expected);
        assert!(f
            .plan
            .context
            .items
            .iter()
            .all(|i| i.source == SessionContextSource::Retrieval));
        // The current transcript fragment is never retrieved for itself.
        assert!(!got.contains(&USER_EVIDENCE.to_owned()));
        // Retrieval over the transcript-enriched graph would differ (corpus statistics).
        assert_ne!(
            LexicalIndex::build(&turn_graph(&f)).unwrap().corpus_hash(),
            LexicalIndex::build(&f.graph).unwrap().corpus_hash()
        );
    }

    #[test]
    fn context_priority_scope_questions_retrieval() {
        let f = prepared_with(base(), session(&[PAYROLL]), TEXT, StageId::S2, config());
        let refs = item_refs(&f.plan);
        let sources: Vec<&str> = refs.iter().map(|(s, _)| s.as_str()).collect();
        // Scope first, then scoped open questions, then retrieval.
        assert_eq!(refs[0], ("scope".into(), PAYROLL.into()));
        assert_eq!(refs[1], ("open_question".into(), "question:q3".into()));
        assert!(sources[2..].iter().all(|s| *s == "retrieval"));
        // With scope PAYROLL only q3 (context PAYROLL) is offered; q1 (LEAVE) is not.
        assert_eq!(
            f.plan.context.allowed_open_question_refs,
            [id("question:q3")]
        );
        // Retrieval excludes the already selected scope node.
        let retrieved: Vec<&String> = refs[2..].iter().map(|(_, r)| r).collect();
        assert!(!retrieved.contains(&&PAYROLL.to_owned()));
        assert!(retrieved.contains(&&LEAVE.to_owned()));
    }

    #[test]
    fn open_question_eligibility_and_bound() {
        let nq = |max| {
            prepared_with(
                base(),
                session(&[]),
                TEXT,
                StageId::S2,
                SessionTurnConfig {
                    retrieval_top_k: 0,
                    max_open_questions: max,
                    max_context_bytes: 100_000,
                },
            )
            .plan
        };
        let all = nq(10);
        // Accepted Open only: not Answered, Closed, Superseded or Proposed; ID order.
        assert_eq!(
            item_refs(&all),
            [
                ("open_question".into(), "question:q1".into()),
                ("open_question".into(), "question:q3".into())
            ]
        );
        assert_eq!(
            all.context.allowed_open_question_refs,
            [id("question:q1"), id("question:q3")]
        );
        assert_eq!(
            item_refs(&nq(1)),
            [("open_question".into(), "question:q1".into())]
        );
        assert!(nq(0).context.items.is_empty());
        assert!(nq(0).context.allowed_open_question_refs.is_empty());
    }

    #[test]
    fn context_items_carry_only_the_semantic_payload() {
        let f = prepared_with(base(), session(&[LEAVE]), TEXT, StageId::S2, config());
        let item = &f.plan.context.items[0];
        assert_eq!(item.node_type, "Requirement");
        let leave = f.graph.node(&id(LEAVE)).unwrap();
        let wire = serde_json::to_value(&leave.payload).unwrap();
        assert_eq!(item.payload, wire["data"]);
        let encoded = serde_json::to_value(item).unwrap();
        assert_eq!(
            encoded.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["node_ref", "node_type", "payload", "source"]
        );
    }

    #[test]
    fn context_byte_budget() {
        let full = prepared_with(base(), session(&[LEAVE]), TEXT, StageId::S2, config()).plan;
        let sizes: Vec<usize> = full
            .context
            .items
            .iter()
            .map(|i| to_canonical_json(i).unwrap().len())
            .collect();
        assert_eq!(full.context.context_item_bytes, sizes.iter().sum::<usize>());
        // Mandatory scope that does not fit fails; nothing is dropped.
        let err = prepare_turn(
            &base(),
            &session(&[LEAVE]),
            &user_turn(TEXT, StageId::S2),
            &SessionTurnConfig {
                max_context_bytes: sizes[0] - 1,
                ..config()
            },
            &mut MemoryStore::default(),
            &FixedClock::new(ts(USER_AT)),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            SessionError::ContextBudgetExceeded { required, limit } if required == sizes[0] && limit == sizes[0] - 1
        ));
        // Exactly the scope fits.
        let tight = prepared_with(
            base(),
            session(&[LEAVE]),
            TEXT,
            StageId::S2,
            SessionTurnConfig {
                max_context_bytes: sizes[0],
                ..config()
            },
        )
        .plan;
        assert_eq!(item_refs(&tight), [("scope".into(), LEAVE.into())]);
        assert_eq!(tight.context.context_item_bytes, sizes[0]);
    }

    #[test]
    fn optional_items_are_skipped_whole_and_later_ones_may_fit() {
        // A large open question first, then a small one.
        let mut nodes = base_nodes();
        nodes.push(question_with(
            "question:a-large",
            Accepted,
            "Open",
            &[PAYROLL],
            &"Very long question text. ".repeat(40),
        ));
        nodes.push(question_with(
            "question:b-small",
            Accepted,
            "Open",
            &[PAYROLL],
            "Short?",
        ));
        let g = graph_of(nodes);
        let cfg = |bytes| SessionTurnConfig {
            retrieval_top_k: 0,
            max_open_questions: 10,
            max_context_bytes: bytes,
        };
        let scope_only = prepared_with(
            g.clone(),
            session(&[PAYROLL]),
            TEXT,
            StageId::S2,
            cfg(100_000),
        )
        .plan;
        let sizes: BTreeMap<String, usize> = scope_only
            .context
            .items
            .iter()
            .map(|i| (i.node_ref.to_string(), to_canonical_json(i).unwrap().len()))
            .collect();
        let budget = sizes[PAYROLL] + sizes["question:b-small"] + sizes["question:q3"];
        assert!(sizes["question:a-large"] > sizes["question:b-small"] + sizes["question:q3"]);
        let plan = prepared_with(g, session(&[PAYROLL]), TEXT, StageId::S2, cfg(budget)).plan;
        assert_eq!(
            item_refs(&plan),
            [
                ("scope".into(), PAYROLL.into()),
                ("open_question".into(), "question:b-small".into()),
                ("open_question".into(), "question:q3".into())
            ]
        );
        assert_eq!(plan.context.context_item_bytes, budget);
    }

    #[test]
    fn allowed_evidence_set() {
        let g = base();
        let f = prepared_with(g.clone(), session(&[LEAVE]), TEXT, StageId::S2, config());
        let policy_fragment = fragment_of(&g, "policy.md");
        let ctx = &f.plan.context;
        assert_eq!(ctx.user_evidence_ref, id(USER_EVIDENCE));
        assert!(ctx.allowed_evidence_refs.contains(&id(USER_EVIDENCE)));
        // Evidence of the scoped requirement is exposed.
        assert!(ctx.allowed_evidence_refs.contains(&policy_fragment));
        let mut sorted = ctx.allowed_evidence_refs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, ctx.allowed_evidence_refs);
        // Without context, only the user turn.
        let bare = prepared_with(
            g,
            session(&[]),
            "zzz",
            StageId::S2,
            SessionTurnConfig {
                retrieval_top_k: 0,
                max_open_questions: 0,
                max_context_bytes: 1,
            },
        );
        assert_eq!(
            bare.plan.context.allowed_evidence_refs,
            std::slice::from_ref(&bare.plan.user_turn.evidence_ref)
        );
        assert!(bare.plan.context.items.is_empty());
    }

    // ------------------------------------------------------------------ request

    #[test]
    fn request_identity_and_material() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(Hash::content_sha256(SCHEMA).as_str(), SCHEMA_HASH);
        assert!(PROMPT.ends_with(b"proves it.\n"));
        for stage in [StageId::S1, StageId::S4] {
            let f = prepared_with(base(), session(&[LEAVE]), TEXT, stage, config());
            let (plan, ctx) = (&f.plan, &f.plan.context);
            assert_eq!(plan.active_stage, stage);
            assert_eq!(plan.request.stage, stage);
            assert_eq!(ctx.active_stage, stage);
            assert_eq!(plan.user_transcript_proposal.as_ref().unwrap().stage, stage);
            assert_eq!(plan.request.task_kind, "session_turn");
            assert_eq!(SESSION_TURN_TASK_KIND, "session_turn");
            assert_eq!(plan.request.prompt_template_hash.as_str(), PROMPT_HASH);
            assert_eq!(plan.request.schema_hash.as_str(), SCHEMA_HASH);
            let context_json = to_canonical_json(ctx).unwrap();
            assert_eq!(plan.material.context_json, context_json);
            assert_eq!(
                plan.request.context_hash,
                Hash::content_sha256(&context_json)
            );
            assert_eq!(plan.material.prompt_template, PROMPT);
            assert_eq!(plan.material.schema_json, SCHEMA);
            assert_eq!(plan.material.request_hash, plan.request.id);
            plan.material.validate_for(&plan.request).unwrap();
            let mut inputs: Vec<Id> = ctx.items.iter().map(|i| i.node_ref.clone()).collect();
            inputs.sort();
            assert_eq!(plan.request.input_refs, inputs);
            assert_eq!(plan.request.evidence_refs, ctx.allowed_evidence_refs);
            assert_eq!(plan.request.provider_policy, policy());
            assert_eq!(ctx.version, 1);
            assert_eq!(ctx.session_ref, id(SESSION));
            assert_eq!(ctx.project_ref, id(PROJECT));
            assert_eq!(ctx.profile_ref, id(PROFILE));
            assert_eq!(ctx.base_semantic_hash, f.graph.semantic_hash().unwrap());
            assert_eq!(ctx.user_text, TEXT);
            assert_eq!(ctx.scope_refs, [id(LEAVE)]);
            plan.validate(&f.graph).unwrap();
        }
    }

    #[test]
    fn plan_validation_detects_tampering() {
        let f = prepared();
        let mut p = f.plan.clone();
        p.context.user_text = "Other".into();
        assert!(matches!(
            p.validate(&f.graph),
            Err(SessionError::PlanInvalid(_))
        ));
        let mut p = f.plan.clone();
        p.user_turn.timestamp = ts(AT);
        assert!(matches!(
            p.validate(&f.graph),
            Err(SessionError::PlanInvalid(_))
        ));
        let mut p = f.plan.clone();
        p.user_transcript_proposal = None;
        assert!(matches!(
            p.validate(&f.graph),
            Err(SessionError::PlanInvalid(_))
        ));
        let mut p = f.plan.clone();
        p.material.prompt_template.push(b' ');
        assert!(p.validate(&f.graph).is_err());
        let mut p = f.plan.clone();
        p.active_stage = StageId::S5;
        assert!(matches!(
            p.validate(&f.graph),
            Err(SessionError::PlanInvalid(_))
        ));
        // Against another graph.
        let other = graph_of(vec![req(LEAVE, Accepted, "Changed.")]);
        assert!(f.plan.validate(&other).is_err());
    }

    // ------------------------------------------------------------------ acquisition

    fn valid_output(f: &Fixture) -> (Value, Proposal) {
        let p = semantic(
            f,
            NEW_REQ,
            "Managers approve leave within two days.",
            &[&user_evidence(f)],
        );
        (output("Here is a proposal.", &[&p], Some("question:q1")), p)
    }

    #[test]
    fn persisted_acquisition_is_inspected() {
        let mut f = prepared();
        let (value, p) = valid_output(&f);
        let persisted = acquire(&mut f, value);
        let inspected = inspect(&f, &persisted).unwrap();
        assert_eq!(inspected.reply, "Here is a proposal.");
        assert_eq!(inspected.proposals, [p]);
        assert_eq!(inspected.asked_question_ref, Some(id("question:q1")));
        assert_eq!(inspected.request_ref, f.plan.request.id);
        assert_eq!(inspected.persisted_inference, persisted);
        // Repeatable.
        assert_eq!(inspect(&f, &persisted).unwrap(), inspected);
    }

    #[test]
    fn persisted_acquisition_failures() {
        let mut f = prepared();
        let (value, _) = valid_output(&f);
        let persisted = acquire(&mut f, value.clone());
        // Each artifact missing in turn.
        for which in 0..3 {
            let mut store = f.store.clone();
            let missing = [
                &persisted.request_artifact_ref,
                &persisted.raw_response_ref,
                &persisted.validated_inference_ref,
            ][which];
            store.artifacts.remove(missing);
            let err = inspect_turn_output(&f.graph, &f.plan, &store, &persisted).unwrap_err();
            assert!(
                matches!(err, SessionError::Artifact(ArtifactStoreError::NotFound(_))),
                "{err:?}"
            );
        }
        // Wrong kinds: refs swapped.
        let swapped = PersistedInferenceRefs {
            request_artifact_ref: persisted.validated_inference_ref.clone(),
            raw_response_ref: persisted.raw_response_ref.clone(),
            validated_inference_ref: persisted.request_artifact_ref.clone(),
        };
        assert!(matches!(
            inspect(&f, &swapped),
            Err(SessionError::PersistedArtifactKind { .. })
        ));
        // The user transcript is not an inference response.
        let transcript = PersistedInferenceRefs {
            raw_response_ref: f.plan.user_turn.source_artifact_ref.clone(),
            ..persisted.clone()
        };
        assert!(matches!(
            inspect(&f, &transcript),
            Err(SessionError::PersistedArtifactKind {
                expected: ArtifactKind::InferenceResponse,
                ..
            })
        ));
        // Wrong media type on the request artifact.
        let mut store = f.store.clone();
        store
            .artifacts
            .get_mut(&persisted.request_artifact_ref)
            .unwrap()
            .media_type = "text/plain".into();
        assert!(matches!(
            inspect_turn_output(&f.graph, &f.plan, &store, &persisted),
            Err(SessionError::PersistedArtifactMediaType { .. })
        ));
        // Raw response that is not the artifact's.
        let other_raw = f
            .store
            .put(
                ArtifactKind::InferenceResponse,
                "application/json",
                b"{}",
                ts(ACQUIRED_AT),
            )
            .unwrap();
        let raw_mismatch = PersistedInferenceRefs {
            raw_response_ref: other_raw,
            ..persisted.clone()
        };
        assert!(matches!(
            inspect(&f, &raw_mismatch),
            Err(SessionError::PersistedRawResponseMismatch { .. })
        ));
    }

    #[test]
    fn acquisition_of_another_request_is_rejected() {
        let mut f = prepared();
        let mut other = prepared_with(base(), session(&[LEAVE]), TEXT, StageId::S2, config());
        let (value, _) = valid_output(&other);
        let foreign = acquire(&mut other, value.clone());
        for (hash, artifact) in &other.store.artifacts {
            f.store.artifacts.insert(hash.clone(), artifact.clone());
        }
        assert!(matches!(
            inspect(&f, &foreign),
            Err(SessionError::PersistedRequestMismatch)
        ));
        // The right request artifact with a validated inference for another request.
        let own = acquire(&mut f, value);
        let mixed = PersistedInferenceRefs {
            validated_inference_ref: foreign.validated_inference_ref.clone(),
            raw_response_ref: foreign.raw_response_ref.clone(),
            ..own
        };
        assert!(matches!(
            inspect(&f, &mixed),
            Err(SessionError::Inference(_))
        ));
    }

    // ------------------------------------------------------------------ output validation

    #[test]
    fn output_shape_rules() {
        let mut f = prepared();
        let p = semantic(
            &f,
            NEW_REQ,
            "Managers approve leave fast.",
            &[&user_evidence(&f)],
        );
        let mut extra = output("Reply.", &[&p], None);
        extra["unexpected"] = json!(true);
        assert!(matches!(
            inspect_value(&mut f, extra),
            Err(SessionError::OutputDecode(_))
        ));
        let mut version = output("Reply.", &[&p], None);
        version["version"] = json!(2);
        assert!(matches!(
            inspect_value(&mut f, version),
            Err(SessionError::OutputVersion(2))
        ));
        assert!(matches!(
            inspect_value(&mut f, output(" \n ", &[&p], None)),
            Err(SessionError::BlankReply)
        ));
        let many: Vec<Proposal> = (0..21)
            .map(|i| {
                semantic(
                    &f,
                    &format!("requirement:n{i:02}"),
                    "Text.",
                    &[&user_evidence(&f)],
                )
            })
            .collect();
        let refs: Vec<&Proposal> = many.iter().collect();
        assert!(matches!(
            inspect_value(&mut f, output("Reply.", &refs, None)),
            Err(SessionError::TooManyProposals(21))
        ));
        assert!(matches!(
            inspect_value(&mut f, output("Reply.", &[&p, &p], None)),
            Err(SessionError::DuplicateProposal(_))
        ));
        // Exactly 20 are allowed; the reply is kept exactly.
        let twenty: Vec<&Proposal> = refs[..20].to_vec();
        let ok = inspect_value(&mut f, output("  Reply with spaces.\n", &twenty, None)).unwrap();
        assert_eq!(ok.proposals.len(), SESSION_MAX_PROPOSALS);
        assert_eq!(ok.reply, "  Reply with spaces.\n");
        // A malformed proposal does not decode.
        let mut broken = output("Reply.", &[&p], None);
        broken["proposals"][0]["id"] = json!("prop:0000000000000000");
        assert!(matches!(
            inspect_value(&mut f, broken),
            Err(SessionError::OutputDecode(_))
        ));
    }

    #[test]
    fn proposal_binding_rules() {
        let mut f = prepared();
        let ev = user_evidence(&f);
        let add = |n: &str| SemanticPatch::AddNode {
            node: req(n, Proposed, "Managers approve leave fast."),
        };
        let wrong_stage = proposal_as(
            &f,
            StageId::S7,
            add(NEW_REQ),
            &[&ev],
            AcceptancePolicy::HumanConfirm,
        );
        assert!(matches!(
            inspect_value(&mut f, output("R.", &[&wrong_stage], None)),
            Err(SessionError::ProposalStageMismatch {
                expected: StageId::S2,
                actual: StageId::S7,
                ..
            })
        ));
        let mut stale = Proposal::new(
            StageId::S2,
            PatchSet {
                base_semantic_hash: graph_of(vec![req(LEAVE, Accepted, "Old.")])
                    .semantic_hash()
                    .unwrap(),
                patch: add(NEW_REQ),
            },
            vec![ev.clone().into()],
            vec![],
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .unwrap();
        assert!(matches!(
            inspect_value(&mut f, output("R.", &[&stale], None)),
            Err(SessionError::ProposalBaseMismatch(_))
        ));
        stale = proposal_as(
            &f,
            StageId::S2,
            add(NEW_REQ),
            &[&ev],
            AcceptancePolicy::AutoDerivation,
        );
        assert!(matches!(
            inspect_value(&mut f, output("R.", &[&stale], None)),
            Err(SessionError::AutoDerivationProposal(_))
        ));
        let derived = Proposal::new(
            StageId::S2,
            PatchSet {
                base_semantic_hash: f.graph.semantic_hash().unwrap(),
                patch: add(NEW_REQ),
            },
            vec![ev.clone().into()],
            vec![id("drv:0123456789abcdef").into()],
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .unwrap();
        assert!(matches!(
            inspect_value(&mut f, output("R.", &[&derived], None)),
            Err(SessionError::ProposalDerivationRefs(_))
        ));
        // Other non-AUTO_DERIVATION policies are fine.
        for policy in [
            AcceptancePolicy::HumanDecision,
            AcceptancePolicy::ProfilePolicy,
            AcceptancePolicy::AutoNonSemantic,
        ] {
            let p = proposal_as(&f, StageId::S2, add(NEW_REQ), &[&ev], policy);
            inspect_value(&mut f, output("R.", &[&p], None)).unwrap();
        }
    }

    #[test]
    fn proposal_evidence_provenance() {
        let mut f = prepared_with(base(), session(&[LEAVE]), TEXT, StageId::S2, config());
        let user = user_evidence(&f);
        let policy_fragment = fragment_of(&f.graph, "policy.md");
        let payroll_fragment = fragment_of(&f.graph, "payroll.md");
        assert!(!f
            .plan
            .context
            .allowed_evidence_refs
            .contains(&payroll_fragment));
        let with =
            |f: &Fixture, ev: &[&Id]| semantic(f, NEW_REQ, "Managers approve leave fast.", ev);
        // No evidence.
        let none = with(&f, &[]);
        assert!(matches!(
            inspect_value(&mut f, output("R.", &[&none], None)),
            Err(SessionError::ProposalWithoutEvidence(_))
        ));
        // Invented evidence ID.
        let invented = with(&f, &[&id("evd:ffffffffffffffff")]);
        assert!(matches!(
            inspect_value(&mut f, output("R.", &[&invented], None)),
            Err(SessionError::EvidenceNotAllowed { .. })
        ));
        // Accepted evidence that was not in the model context.
        let unseen = with(&f, &[&user, &payroll_fragment]);
        assert!(matches!(
            inspect_value(&mut f, output("R.", &[&unseen], None)),
            Err(SessionError::EvidenceNotAllowed { .. })
        ));
        // The user turn, accepted context evidence, or both pass.
        for ev in [
            vec![&user],
            vec![&policy_fragment],
            vec![&user, &policy_fragment],
        ] {
            let p = with(&f, &ev);
            let inspected = inspect_value(&mut f, output("R.", &[&p], None)).unwrap();
            assert_eq!(inspected.proposals, [p]);
        }
    }

    #[test]
    fn asked_question_rules() {
        let mut f = prepared_with(base(), session(&[LEAVE]), TEXT, StageId::S2, config());
        assert_eq!(
            f.plan.context.allowed_open_question_refs,
            [id("question:q1")]
        );
        let ok = inspect_value(&mut f, output("R.", &[], Some("question:q1"))).unwrap();
        assert_eq!(ok.asked_question_ref, Some(id("question:q1")));
        assert_eq!(
            inspect_value(&mut f, output("R.", &[], None))
                .unwrap()
                .asked_question_ref,
            None
        );
        // Invented, unexposed (open but out of scope), Answered, Closed and Proposed.
        for q in [
            "question:invented",
            "question:q3",
            "question:q2",
            "question:q5",
            "question:q4",
        ] {
            let err = inspect_value(&mut f, output("R.", &[], Some(q))).unwrap_err();
            assert!(
                matches!(err, SessionError::QuestionNotAllowed(_)),
                "{q}: {err:?}"
            );
        }
        // A plan claiming a closed question was offered is not a valid plan.
        let mut plan = f.plan.clone();
        plan.context
            .allowed_open_question_refs
            .push(id("question:q2"));
        assert!(matches!(
            plan.validate(&f.graph),
            Err(SessionError::PlanInvalid(_))
        ));
    }

    // ------------------------------------------------------------------ completion

    #[test]
    fn completion_runs_intake_in_model_order_then_the_assistant_transcript() {
        let mut f = prepared();
        let ev = user_evidence(&f);
        let second = semantic(
            &f,
            "requirement:b",
            "Payroll exports salaries weekly.",
            &[&ev],
        );
        let first = semantic(
            &f,
            "requirement:z",
            "Managers approve leave within two days.",
            &[&ev],
        );
        let (a, b) = if first.id > second.id {
            (&first, &second)
        } else {
            (&second, &first)
        };
        // Model order: the larger ID first.
        let persisted = acquire(
            &mut f,
            output("Two proposals.", &[a, b], Some("question:q1")),
        );
        let inspected = inspect(&f, &persisted).unwrap();
        let clock = CountingClock::new(ASSISTANT_AT);
        let before = f.graph.clone();
        let puts_before = f.store.puts.len();
        let result = complete(&mut f, &inspected, &empty_inputs(&[a, b]), &clock).unwrap();
        assert_eq!(clock.reads.get(), 1);
        assert_eq!(f.graph, before);
        let order: Vec<&Id> = result.proposals.iter().map(|c| &c.proposal.id).collect();
        assert_eq!(order, [&a.id, &b.id]);
        let turn_graph = turn_graph(&f);
        for card in &result.proposals {
            assert_eq!(card.intake.proposal_ref, card.proposal.id);
            assert!(card.intake.validation_gates.is_empty());
            assert_eq!(
                card.intake,
                plumb_intake::intake(&turn_graph, &card.proposal, &IntakeValidationInput::empty())
                    .unwrap()
            );
        }
        assert_eq!(result.reply, "Two proposals.");
        assert_eq!(result.asked_question_ref, Some(id("question:q1")));
        assert_eq!(result.request_ref, f.plan.request.id);
        assert_eq!(result.persisted_inference, persisted);
        assert_eq!(result.user_turn, f.plan.user_turn);
        assert_eq!(
            result.user_transcript_proposal,
            f.plan.user_transcript_proposal
        );
        // Exactly one more artifact write: the assistant transcript.
        assert_eq!(
            &f.store.puts[puts_before..],
            [(
                ArtifactKind::SourceOriginal,
                TRANSCRIPT_MEDIA_TYPE.to_owned()
            )]
        );
        // The assistant transcript contract.
        let turn = &result.assistant_turn;
        assert_eq!(turn.role, SessionRole::Assistant);
        assert_eq!(turn.actor_ref, id(ASSISTANT));
        assert_eq!(turn.timestamp, ts(ASSISTANT_AT));
        assert_eq!(turn.text, "Two proposals.");
        let envelope = format!(
            "{{\"actor_ref\":\"{ASSISTANT}\",\"role\":\"assistant\",\"session_ref\":\"{SESSION}\",\"text\":\"Two proposals.\",\"timestamp\":\"{ASSISTANT_AT}\",\"version\":1}}"
        );
        let artifact = f.store.get(&turn.source_artifact_ref).unwrap();
        assert_eq!(artifact.bytes, envelope.as_bytes());
        assert_eq!(artifact.kind, ArtifactKind::SourceOriginal);
        assert_eq!(
            turn.source_artifact_ref,
            Hash::content_sha256(envelope.as_bytes())
        );
        let digest = &turn.source_artifact_ref.as_str()["sha256:".len()..];
        assert_eq!(turn.turn_ref, id(&format!("turn:{}", &digest[..16])));
        assert_eq!(
            turn.source_node_ref,
            source_artifact_id(&turn.source_artifact_ref).unwrap()
        );
        assert_eq!(
            turn.evidence_ref,
            evidence_fragment_id(
                &turn.source_node_ref,
                &EvidenceLocator::ConversationTurn {
                    turn_id: turn.turn_ref.clone()
                }
            )
            .unwrap()
        );
        let ap = result.assistant_transcript_proposal.as_ref().unwrap();
        assert_eq!(ap.materiality, ProposalMateriality::NonSemantic);
        assert_eq!(ap.acceptance_policy, AcceptancePolicy::AutoNonSemantic);
        assert_eq!(ap.stage, StageId::S2);
        assert_eq!(
            ap.patch_set.base_semantic_hash,
            f.graph.semantic_hash().unwrap()
        );
        let applied = apply_patch(&turn_graph, &ap.patch_set).unwrap().graph;
        let fragment = applied.node(&turn.evidence_ref).unwrap();
        assert_eq!(fragment.status, Accepted);
        assert!(matches!(&fragment.payload, NodePayload::EvidenceFragment(e)
            if e.speaker.as_deref() == Some("assistant")
                && e.extracted_text.as_deref() == Some("Two proposals.")
                && e.source_timestamp == Some(ts(ASSISTANT_AT))));
        let source = applied.node(&turn.source_node_ref).unwrap();
        assert!(matches!(&source.payload, NodePayload::SourceArtifact(s)
            if s.producer.as_deref() == Some("assistant") && s.source_kind == "transcript"));
        assert_eq!(source.audit.created_by, id(ASSISTANT));
        let ai = result.assistant_transcript_intake.as_ref().unwrap();
        assert_eq!(ai.proposal_ref, ap.id);
        assert!(!ai.requires_reason);
    }

    #[test]
    fn intake_inputs_must_cover_exactly_the_proposals() {
        let mut f = prepared();
        let ev = user_evidence(&f);
        let a = semantic(&f, "requirement:a1", "Managers approve leave.", &[&ev]);
        let b = semantic(&f, "requirement:b1", "Payroll exports weekly.", &[&ev]);
        let persisted = acquire(&mut f, output("R.", &[&a, &b], None));
        let inspected = inspect(&f, &persisted).unwrap();
        let clock = CountingClock::new(ASSISTANT_AT);
        assert!(matches!(
            complete(&mut f, &inspected, &empty_inputs(&[&a]), &clock),
            Err(SessionError::MissingIntakeInput(ref m)) if *m == b.id
        ));
        let c = semantic(&f, "requirement:c1", "Other.", &[&ev]);
        assert!(matches!(
            complete(&mut f, &inspected, &empty_inputs(&[&a, &b, &c]), &clock),
            Err(SessionError::UnknownIntakeInput(ref u)) if *u == c.id
        ));
        assert!(matches!(
            SessionIntakeInputs::new([
                (a.id.clone(), IntakeValidationInput::empty()),
                (a.id.clone(), IntakeValidationInput::empty())
            ]),
            Err(SessionError::DuplicateIntakeInput(_))
        ));
        assert_eq!(clock.reads.get(), 0);
        complete(&mut f, &inspected, &empty_inputs(&[&a, &b]), &clock).unwrap();
        assert_eq!(clock.reads.get(), 1);
        // No proposals: an empty map.
        let mut g = prepared();
        let persisted = acquire(&mut g, output("Nothing to propose.", &[], None));
        let inspected = inspect(&g, &persisted).unwrap();
        let result = complete(&mut g, &inspected, &SessionIntakeInputs::default(), &clock).unwrap();
        assert!(result.proposals.is_empty());
    }

    #[test]
    fn intake_failure_fails_the_turn_without_assistant_transcript() {
        let mut f = prepared();
        let ev = user_evidence(&f);
        // Adding a node whose ID already exists passes session checks but fails X1.3 intake.
        let clash = proposal_as(
            &f,
            StageId::S2,
            SemanticPatch::AddNode {
                node: req(LEAVE, Proposed, "Duplicate ID."),
            },
            &[&ev],
            AcceptancePolicy::HumanConfirm,
        );
        let persisted = acquire(&mut f, output("R.", &[&clash], None));
        let inspected = inspect(&f, &persisted).unwrap();
        let clock = CountingClock::new(ASSISTANT_AT);
        let puts_before = f.store.puts.len();
        let err = complete(&mut f, &inspected, &empty_inputs(&[&clash]), &clock).unwrap_err();
        assert!(
            matches!(
                err,
                SessionError::ProposalIntake {
                    error: IntakeError::Patch(PatchError::IdAlreadyExists(_)),
                    ..
                }
            ),
            "{err:?}"
        );
        assert_eq!(clock.reads.get(), 0);
        assert_eq!(f.store.puts.len(), puts_before);
    }

    #[test]
    fn completion_rejects_foreign_or_tampered_output() {
        let mut f = prepared();
        let (value, p) = valid_output(&f);
        let persisted = acquire(&mut f, value);
        let inspected = inspect(&f, &persisted).unwrap();
        let clock = CountingClock::new(ASSISTANT_AT);
        let inputs = empty_inputs(&[&p]);
        let mut foreign = inspected.clone();
        foreign.request_ref = Hash::content_sha256(b"other");
        assert!(matches!(
            complete(&mut f, &foreign, &inputs, &clock),
            Err(SessionError::InspectedRequestMismatch { .. })
        ));
        let mut tampered = inspected.clone();
        tampered.asked_question_ref = Some(id("question:invented"));
        assert!(matches!(
            complete(&mut f, &tampered, &inputs, &clock),
            Err(SessionError::QuestionNotAllowed(_))
        ));
        let other_session =
            Session::new(id("session:other"), id(PROJECT), vec![], id(ASSISTANT)).unwrap();
        assert!(matches!(
            complete_turn(
                &f.graph,
                &other_session,
                &f.plan,
                &inspected,
                &inputs,
                &mut f.store,
                &clock
            ),
            Err(SessionError::SessionMismatch { .. })
        ));
        assert_eq!(clock.reads.get(), 0);
    }

    // ------------------------------------------------------------------ validation reports

    #[test]
    fn candidate_gate_report_is_carried_through_intake() {
        let mut f = prepared();
        let ev = user_evidence(&f);
        let p = revise_leave(&f, &[&ev]);
        let persisted = acquire(&mut f, output("R.", &[&p], None));
        let inspected = inspect(&f, &persisted).unwrap();
        // The external validator evaluates the candidate of the proposal over the
        // transcript-enriched turn graph. The report below is the real F1 GateReport the
        // existing evaluator produced for exactly that candidate (see `F1_REPORT`).
        let candidate = apply_patch(&turn_graph(&f), &p.patch_set).unwrap().graph;
        let input =
            IntakeValidationInput::new(vec![serde_json::from_str(F1_REPORT).unwrap()]).unwrap();
        let report = input.candidate_reports()[0].clone();
        assert_eq!(
            report.baseline_semantic_hash,
            candidate.semantic_hash().unwrap()
        );
        assert_eq!(
            report.baseline_evidence_hash,
            candidate.evidence_hash().unwrap()
        );
        let inputs = SessionIntakeInputs::new([(p.id.clone(), input)]).unwrap();
        let result = complete(
            &mut f,
            &inspected,
            &inputs,
            &FixedClock::new(ts(ASSISTANT_AT)),
        )
        .unwrap();
        let card = &result.proposals[0];
        assert_eq!(card.intake.validation_gates, [report.gate]);
        // Exactly the report's findings on the revised requirement, copied unchanged; the
        // finding affecting only the untouched payroll requirement is not surfaced.
        let relevant: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.payload.affected_refs.contains(&id(LEAVE)))
            .cloned()
            .collect();
        assert_eq!(relevant.len(), 3);
        assert_eq!(card.intake.validation_findings, relevant);
        assert!(report
            .findings
            .iter()
            .any(|f| !f.payload.affected_refs.contains(&id(LEAVE))));
        assert!(card
            .intake
            .reason_codes
            .contains(&plumb_intake::IntakeReasonCode::ValidationConflict));
    }

    /// The real F1 GateReport (RFC 8785 JSON) produced by the existing plumb-validation
    /// registry (I0 and F1 evaluators, default policy, F1 inputs for every Accepted Requirement)
    /// over the candidate of `revise_leave` applied to the transcript-enriched turn graph of
    /// `prepared()`. Generated once with the real evaluator; plumb-validation is not a session
    /// dependency, so the session test consumes the report through its public wire form.
    const F1_REPORT: &str = r#"{"baseline_evidence_hash":"ev:sha256:2ebc6cab51e24c3a684e367aaec455f19b0a2277681ce494c3d7c2c262d5eea5","baseline_semantic_hash":"psg:sha256:363d979a486ccb49a249217da1b13e0d3d2d010602a821d49527642de5b12b12","evidence_artifact_refs":[],"f1_input_hash":"sha256:2c163fa512c2ac6216088e6109a1fcd7c473d7298ca2f04f686babf8092e5536","findings":[{"id":"fnd:20303daae1250ea2","key":"sha256:20303daae1250ea20d61c1e1f2c603c4e4e122c4f85b076a98b4a6ac5c5b8764","payload":{"affected_refs":["requirement:leave","requirement:payroll"],"code":"PLUMB.F1.REQ.MODALITY_EXPLICIT","family":"F1","message":"Accepted requirements must express the same explicit modality as their typed Requirement.modality.","severity":"error","standard_rule_ref":null,"status":"Open","suggested_resolution":"Correct the requirement wording or its governed modality classification.","waiver_ref":null},"semantic_condition_key":"accepted_requirement_modality_inconsistent"},{"id":"fnd:27a6f0c04a36ac52","key":"sha256:27a6f0c04a36ac52c3c7472c7bd56c7b9cade6be6f45e7b86bb85c545dbfa930","payload":{"affected_refs":["requirement:leave","requirement:payroll"],"code":"ISO29148.F1.REQ.LINEAGE","family":"F1","message":"Accepted non-stakeholder requirements must have explicit intent lineage or a human-origin decision.","severity":"error","standard_rule_ref":"ISO29148.F1.REQ.LINEAGE","status":"Open","suggested_resolution":"Link the requirement to its governing intent or record a governed human-origin decision.","waiver_ref":null},"semantic_condition_key":"accepted_requirement_lineage_missing"},{"id":"fnd:58d029f511c1bec6","key":"sha256:58d029f511c1bec6892f6b940429d86ca864fd16ef50b92e64291e01b4e3f6a6","payload":{"affected_refs":["requirement:payroll"],"code":"PLUMB.F1.REQ.GROUNDED","family":"F1","message":"Accepted requirements must be grounded in evidence or an explicit human-origin decision.","severity":"blocker","standard_rule_ref":null,"status":"Open","suggested_resolution":"Attach authoritative evidence or record a governed human-origin decision.","waiver_ref":null},"semantic_condition_key":"accepted_requirement_grounding_missing"},{"id":"fnd:d715b66a92741436","key":"sha256:d715b66a92741436f8ae38d6ffc54c31fe58d56308fc2eb03fdcf37aa40c04ef","payload":{"affected_refs":["requirement:leave","requirement:payroll"],"code":"PLUMB.F1.REQ.CRITERIA_FOR_BEHAVIOR","family":"F1","message":"Accepted behavioral requirements in implementation scope need acceptance criteria, executable semantics, or an explicit deferred-verification decision.","severity":"error","standard_rule_ref":null,"status":"Open","suggested_resolution":"Add governed acceptance criteria or executable semantics, or record a governed deferred-verification decision.","waiver_ref":null},"semantic_condition_key":"behavior_requirement_criteria_missing"}],"gate":"F1","policy":{"profile_allow_waiver_rule_ids":[],"promoted_to_blocker_rule_ids":[]},"profile_hash":"sha256:e6dc73ec3f1847e5e7fc5d71aef64b71c15a3e93838e64c7f246f275c65af055","profile_id":"profile:plumb-software-2026.1","result":"FAIL","rule_pack_hash":"sha256:b3a48aa0d0dc47b612697bbf35fa778499509343f74de43459fbcc145e21d33e","rules":[{"applicability":{"state":"APPLICABLE"},"declared_severity":"error","effective_severity":"error","error":null,"evidence":[],"finding_ref":"fnd:27a6f0c04a36ac52","rule_id":"ISO29148.F1.REQ.LINEAGE","semantic_condition_key":"accepted_requirement_lineage_missing","state":"FAIL","targets":["requirement:leave","requirement:payroll"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":[],"finding_ref":null,"rule_id":"ISO29148.F1.REQ.STATEMENT_PRESENT","semantic_condition_key":null,"state":"PASS","targets":["requirement:leave","requirement:payroll"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"error","effective_severity":"error","error":null,"evidence":[],"finding_ref":"fnd:d715b66a92741436","rule_id":"PLUMB.F1.REQ.CRITERIA_FOR_BEHAVIOR","semantic_condition_key":"behavior_requirement_criteria_missing","state":"FAIL","targets":["requirement:leave","requirement:payroll"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":[],"finding_ref":"fnd:58d029f511c1bec6","rule_id":"PLUMB.F1.REQ.GROUNDED","semantic_condition_key":"accepted_requirement_grounding_missing","state":"FAIL","targets":["requirement:payroll"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"error","effective_severity":"error","error":null,"evidence":[],"finding_ref":"fnd:20303daae1250ea2","rule_id":"PLUMB.F1.REQ.MODALITY_EXPLICIT","semantic_condition_key":"accepted_requirement_modality_inconsistent","state":"FAIL","targets":["requirement:leave","requirement:payroll"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":[],"finding_ref":null,"rule_id":"PLUMB.F1.REQ.NO_CONTRADICTION","semantic_condition_key":null,"state":"PASS","targets":["requirement:leave","requirement:payroll"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":[],"finding_ref":null,"rule_id":"PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED","semantic_condition_key":null,"state":"PASS","targets":["requirement:leave","requirement:payroll"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":[],"finding_ref":null,"rule_id":"PLUMB.F1.REQ.QUALITY_FINDINGS_CLEAR","semantic_condition_key":null,"state":"PASS","targets":["requirement:leave","requirement:payroll"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":[],"finding_ref":null,"rule_id":"PLUMB.F1.REQ.SUPERSEDED_EXCLUDED","semantic_condition_key":null,"state":"PASS","targets":["requirement:leave","requirement:payroll"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":[],"finding_ref":null,"rule_id":"PLUMB.F1.REQ.TERMS_RESOLVED","semantic_condition_key":null,"state":"PASS","targets":["requirement:leave","requirement:payroll"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"error","effective_severity":"error","error":null,"evidence":[],"finding_ref":null,"rule_id":"PLUMB.F1.REQ.TYPE_KNOWN","semantic_condition_key":null,"state":"PASS","targets":["requirement:leave","requirement:payroll"],"waiver_ref":null}],"summary":{"blocker_failed":1,"blocker_waived":0,"warnings":0},"validation_artifact_refs":[],"waivers":[]}"#;

    // ------------------------------------------------------------------ determinism

    #[test]
    fn preparation_inspection_and_completion_are_repeatable() {
        let run = |nodes: Vec<Node>, scope: &[&str]| {
            let mut f = prepared_with(graph_of(nodes), session(scope), TEXT, StageId::S3, config());
            let ev = user_evidence(&f);
            let p = semantic(
                &f,
                NEW_REQ,
                "Managers approve leave within two days.",
                &[&ev],
            );
            let persisted = acquire(&mut f, output("Done.", &[&p], Some("question:q1")));
            let inspected = inspect(&f, &persisted).unwrap();
            let result = complete(
                &mut f,
                &inspected,
                &empty_inputs(&[&p]),
                &FixedClock::new(ts(ASSISTANT_AT)),
            )
            .unwrap();
            (f.plan, inspected, result)
        };
        let first = run(base_nodes(), &[LEAVE, PAYROLL]);
        let mut reversed = base_nodes();
        reversed.reverse();
        let second = run(reversed, &[PAYROLL, LEAVE]);
        assert_eq!(first.0, second.0);
        assert_eq!(first.1, second.1);
        assert_eq!(first.2, second.2);
    }

    #[test]
    fn persisted_replay_completes_identically() {
        let mut f = prepared();
        let (value, p) = valid_output(&f);
        let persisted = acquire(&mut f, value);
        let replay = |store: &mut MemoryStore| {
            let g = base();
            let s = session(&[]);
            let inspected = inspect_turn_output(&g, &f.plan, store, &persisted).unwrap();
            complete_turn(
                &g,
                &s,
                &f.plan,
                &inspected,
                &empty_inputs(&[&p]),
                store,
                &FixedClock::new(ts(ASSISTANT_AT)),
            )
            .unwrap()
        };
        let mut store_a = f.store.clone();
        let mut store_b = f.store.clone();
        let a = replay(&mut store_a);
        let b = replay(&mut store_b);
        assert_eq!(a, b);
        // A second completion into the same store is idempotent.
        assert_eq!(replay(&mut store_a), a);
        assert_eq!(store_a.artifacts, store_b.artifacts);
    }

    #[test]
    fn no_graph_mutation_across_the_pipeline() {
        let g = base();
        let before = g.clone();
        let mut f = prepared_with(g, session(&[LEAVE]), TEXT, StageId::S2, config());
        let (value, p) = valid_output(&f);
        let persisted = acquire(&mut f, value);
        let inspected = inspect(&f, &persisted).unwrap();
        complete(
            &mut f,
            &inspected,
            &empty_inputs(&[&p]),
            &FixedClock::new(ts(ASSISTANT_AT)),
        )
        .unwrap();
        assert_eq!(f.graph, before);
    }

    // ------------------------------------------------------------------ source guards

    fn production() -> String {
        // Strip line comments so the guards inspect code, not prose.
        [LIB_SRC, MODEL_SRC, TURN_SRC]
            .iter()
            .flat_map(|src| src.lines())
            .map(|line| line.split("//").next().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn source_guards() {
        let code = production();
        for token in [
            "GraphStore",
            "RevisionStore",
            "move_head",
            "commit(",
            "expected_head",
            ".execute(",
            "InferenceProvider",
            "AnthropicProvider",
            "MockProvider",
            "NullProvider",
            "reqwest",
            "std::net",
            "tokio",
            "std::fs",
            "File::open",
            "PathBuf",
            "SystemClock",
            "SystemTime",
            "Utc::now",
            "Instant::now",
            "persist_inference_bundle",
            "BM25",
            "tokenize",
            "jaccard",
            "Jaccard",
            "EvaluatorRegistry",
            "register_f1_evaluators",
            "evaluate_gate",
            "plumb_validation",
            "IntakeReasonCode",
            "covered_by",
            "duplicate_candidates",
            "StageId::S",
            "ResolutionDecision",
        ] {
            assert!(
                !code.contains(token),
                "session production code contains {token:?}"
            );
        }
        // Exactly one artifact write: the shared transcript routine.
        assert_eq!(code.matches(".put(").count(), 1);
        assert!(code.contains("ArtifactKind::SourceOriginal"));
        // Inference artifact kinds are only loaded, never written.
        for kind in [
            "ArtifactKind::InferenceRequest",
            "ArtifactKind::InferenceResponse",
            "ArtifactKind::ValidatedInference",
        ] {
            assert!(code.contains(kind));
        }
        // Prompt and schema are compile-time includes.
        assert!(code.contains("include_bytes!(\"../../../prompts/session-turn.md\")"));
        assert!(code
            .contains("include_bytes!(\"../../../schemas/inference/session-turn.schema.json\")"));
        // Exactly one clock read per transcript routine caller.
        assert_eq!(code.matches("clock.now()").count(), 2);
    }
}
