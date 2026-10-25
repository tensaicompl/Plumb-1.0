//! A0.3 contract tests for the workflow HTTP runtime (Hotfix 065).
//!
//! Every test lives in `http_workflow` (`cargo test -p plumb-api http_workflow`) and drives the
//! real Axum router with `tower::ServiceExt` over a temporary SQLite database holding the
//! revision, artifact and durable workflow stores. The full A0.3 runtime is injected explicitly:
//! a deterministic test inference provider, a static calendar provider and a test predicate
//! provider, never anything from the environment or the network. U2.1 (Hotfix 073) adds the
//! semantic change preview tests (split and Term merge previews, confirmation through the
//! existing proposal actions and stale-preview rejection); Hotfix 074 makes the split preview
//! create-class.

mod http_workflow {
    use std::path::{Path, PathBuf};
    use std::str::FromStr;
    use std::sync::{Arc, Mutex};

    use axum::body::{to_bytes, Body};
    use axum::http::{Method, Request, StatusCode};
    use axum::Router;
    use plumb_api::openapi::OPERATIONS;
    use plumb_api::{router, A03RuntimeConfig, ApiIdentity, ApiState};
    use plumb_artifacts::{ArtifactKind, ArtifactStore, SqliteArtifactStore};
    use plumb_core::{to_canonical_json, CanonicalJson, Clock, Hash, Id, Timestamp};
    use plumb_functional::question::{
        generate_questions, QuestionAudit, QuestionGenerationInput, StakeholderRoutingConfig,
    };
    use plumb_functional::{
        encode_decision_table, ExprCalendarDefinition, ExprPredicateError, ExprRefValue,
        ExprStaticCalendarProvider, PredicateProvider, CALCULATION_ORIGIN_EXTENSION,
    };
    use plumb_inference::{
        validate_binding, InferenceArtifact, InferenceProvider, InferenceRequest,
        InferenceRequestMaterial, ProviderError, ProviderExecution, ProviderPolicy,
    };
    use plumb_patch::{
        apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        edge_contributes_to_semantic_hash, node_contributes_to_semantic_hash, Edge, ElementStatus,
        EvidenceRef, Finding, FindingSeverity, Graph, Node, NodePayload, NodeType,
    };
    use plumb_session::SessionTurnConfig;
    use plumb_store::{
        BranchName, NewWorkflowProposal, RevisionWriteMeta, SqliteRevisionStore,
        SqliteWorkflowStore, WorkflowInsert, WorkflowOrigin, WorkflowProposalKind,
        WorkflowProposalState, WorkflowTurnState,
    };
    use plumb_validation::{
        finding_id, finding_key, load_builtin_software_profile, GeneratedFinding, RuleClass,
        ValidationProfile,
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const T0: &str = "2026-01-01T00:00:00.000000000Z";
    const NOW: &str = "2026-05-01T12:00:00.000000000Z";
    const ACTOR: &str = "agent:api";
    const ASSISTANT: &str = "agent:assistant";
    const HR_STAKEHOLDERS: &str = include_str!("../../../fixtures/hr-leave/stakeholders.yaml");
    const TRACE_LINK: &str = "plumb_sim:scenario_trace_artifact";

    // ------------------------------------------------------------------ graph builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn audit() -> Value {
        json!({"created_by": "agent:analyst", "created_at": T0, "updated_by": null, "updated_at": null})
    }

    fn node(node_id: &str, status: &str, tag: &str, data: Value) -> Node {
        serde_json::from_value(json!({
            "id": node_id, "revision": 1, "status": status,
            "payload": {"type": tag, "data": data},
            "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
            "audit": audit()
        }))
        .unwrap_or_else(|e| panic!("{node_id}: {e}"))
    }

    fn edge(edge_id: &str, kind: &str, from: &str, to: &str) -> Edge {
        serde_json::from_value(json!({
            "id": edge_id, "revision": 1, "status": "Accepted", "kind": kind,
            "from": from, "to": to, "properties": {}, "evidence": [], "derivations": [],
            "standards": [], "audit": audit()
        }))
        .unwrap_or_else(|e| panic!("{edge_id}: {e}"))
    }

    fn requirement(node_id: &str, status: &str, statement: &str) -> Node {
        node(
            node_id,
            status,
            "Requirement",
            json!({
                "statement": statement, "requirement_kind": "functional", "level": "system",
                "modality": "shall", "title": null, "rationale": null, "priority": null,
                "source_identifier": null, "verification_method": null, "owner_refs": null,
                "stakeholder_refs": null
            }),
        )
    }

    fn term(
        node_id: &str,
        status: &str,
        word: &str,
        language: &str,
        aliases: Option<Vec<&str>>,
    ) -> Node {
        node(
            node_id,
            status,
            "Term",
            json!({"term": word, "language": language, "definition_ref": null, "aliases": aliases, "status": null}),
        )
    }

    fn operation(node_id: &str) -> Node {
        node(
            node_id,
            "Accepted",
            "Operation",
            json!({
                "name": node_id, "operation_kind": "command", "input_schema_ref": null,
                "output_schema_ref": null, "preconditions": null, "postconditions": null,
                "idempotency": null, "transaction_semantics": null
            }),
        )
    }

    fn agent(node_id: &str, kind: &str) -> Node {
        node(node_id, "Accepted", "Agent", json!({"agent_kind": kind}))
    }

    fn stakeholder(node_id: &str) -> Node {
        node(
            node_id,
            "Accepted",
            "Stakeholder",
            json!({
                "name": node_id, "stakeholder_kind": "internal", "organization": null,
                "responsibilities": null, "contact_ref": null
            }),
        )
    }

    fn scenario(
        node_id: &str,
        status: &str,
        confirmation: &str,
        then_state: &str,
        derived_from: &str,
    ) -> Node {
        node(
            node_id,
            status,
            "Scenario",
            json!({
                "name": node_id, "scenario_kind": "state_transition",
                "given": [{"states_active": ["state:draft"]}],
                "when": {"operation": "op:submit"},
                "then": [{"states_active": [then_state]}],
                "requirement_refs": ["req:a"],
                "derived_from_refs": [derived_from],
                "confirmation_status": confirmation
            }),
        )
    }

    fn assumption(node_id: &str, status: &str) -> Node {
        node(
            node_id,
            "Accepted",
            "Assumption",
            json!({
                "statement": "Leave is counted in days.", "owner_ref": "stakeholder:hr",
                "status": status, "finding_ref": null, "default_value": null,
                "expires_at": null, "risk_ref": null
            }),
        )
    }

    /// The typed eligibility table encoded through the strict PSG adapter.
    fn eligibility_table() -> Value {
        let spec = plumb_functional::DecisionTableSpec {
            hit_policy: "FIRST".into(),
            inputs: vec![serde_json::from_value(json!({"name": "eligible", "ty": {"type": "bool"}, "domain": null})).unwrap()],
            outputs: vec![serde_json::from_value(json!({"name": "route", "ty": {"type": "enum", "members": ["Director", "Manager"]}, "domain": null})).unwrap()],
            rows: vec![
                serde_json::from_value(json!({"inputs": [{"cell": "bool", "value": true}], "outputs": [{"cell": "enum", "member": "Manager"}]})).unwrap(),
                serde_json::from_value(json!({"inputs": [{"cell": "bool", "value": false}], "outputs": [{"cell": "enum", "member": "Director"}]})).unwrap(),
            ],
            default_output: None,
        };
        serde_json::to_value(encode_decision_table("Eligibility", &spec).unwrap()).unwrap()
    }

    /// The fixture PSG before question generation.
    fn base_parts() -> (Vec<Node>, Vec<Edge>) {
        let mut calc = node(
            "calc:double",
            "Accepted",
            "Calculation",
            json!({
                "name": "double_days", "expression": "days * 2", "result_type": "Int",
                "unit": null, "rounding": null, "calendar_ref": null, "examples": null
            }),
        );
        let range = json!({"requirement_ref": "req:a", "start": 0, "end": 9});
        calc.extensions.insert(
            plumb_psg::ExtensionKey::from_str(CALCULATION_ORIGIN_EXTENSION).unwrap(),
            json!({
                "name_range": range, "expression_evidence": [range],
                "used_bindings": [{"node_ref": "attr:days", "symbol": "days", "exposure": {"kind": "root"}}]
            }),
        );
        let nodes = vec![
            agent(ACTOR, "human"),
            agent(ASSISTANT, "llm_model"),
            agent("agent:compiler", "compiler_stage"),
            stakeholder("stakeholder:hr"),
            stakeholder("stakeholder:architect"),
            requirement(
                "req:a",
                "Accepted",
                "Employees shall submit leave requests.",
            ),
            requirement(
                "req:b",
                "Accepted",
                "Managers shall approve leave requests.",
            ),
            requirement(
                "req:c",
                "Accepted",
                "Employees shall submit and cancel leave requests.",
            ),
            requirement(
                "req:p",
                "Proposed",
                "Employees shall cancel leave requests.",
            ),
            term(
                "term:leave",
                "Accepted",
                "leave request",
                "en",
                Some(vec!["absence"]),
            ),
            term(
                "term:vacation",
                "Proposed",
                "vacation",
                "en",
                Some(vec!["holiday"]),
            ),
            term("term:urlaub", "Accepted", "Urlaub", "de", None),
            node(
                "entity:request",
                "Accepted",
                "Entity",
                json!({"name": "LeaveRequest", "description": null, "aggregate_root": null}),
            ),
            node(
                "attr:days",
                "Accepted",
                "Attribute",
                json!({
                    "name": "days", "value_type": "Int", "nullable": false, "unit": null,
                    "precision": null, "enum_values": null, "data_classification": null
                }),
            ),
            node("state:draft", "Accepted", "State", json!({"name": "Draft"})),
            node(
                "state:submitted",
                "Accepted",
                "State",
                json!({"name": "Submitted"}),
            ),
            node(
                "trans:submit",
                "Accepted",
                "Transition",
                json!({
                    "stateful_ref": "entity:request", "from_state": "state:draft",
                    "to_state": "state:submitted", "guard_expr": null, "effect_refs": null
                }),
            ),
            operation("op:submit"),
            operation("op:approve"),
            operation("op:cancel"),
            node(
                "actor:clerk",
                "Accepted",
                "Actor",
                json!({"name": "Clerk", "actor_kind": "human"}),
            ),
            calc,
            node(
                "calc:legacy",
                "Accepted",
                "Calculation",
                json!({
                    "name": "legacy", "expression": "days", "result_type": "Int",
                    "unit": null, "rounding": null, "calendar_ref": null, "examples": null
                }),
            ),
            node(
                "rule:positive",
                "Accepted",
                "Rule",
                json!({"name": "Positive days", "rule_kind": "validation", "condition_expr": "days > 0", "action_expr": null}),
            ),
            node(
                "dt:eligibility",
                "Accepted",
                "DecisionTable",
                eligibility_table(),
            ),
            node(
                "dt:broken",
                "Accepted",
                "DecisionTable",
                json!({"name": "Broken", "hit_policy": "FIRST", "inputs": [], "outputs": [], "rows": [{"inputs": "nope"}]}),
            ),
            scenario(
                "scn:pass",
                "Accepted",
                "Confirmed",
                "state:submitted",
                "trans:submit",
            ),
            scenario(
                "scn:fail",
                "Accepted",
                "Confirmed",
                "state:draft",
                "trans:submit",
            ),
            scenario(
                "scn:derived-a",
                "Proposed",
                "Derived",
                "state:submitted",
                "dt:eligibility",
            ),
            scenario(
                "scn:derived-b",
                "Proposed",
                "Derived",
                "state:submitted",
                "dt:eligibility",
            ),
            assumption("assumption:active", "Accepted"),
            assumption("assumption:open", "Open"),
            node(
                "proc:branched",
                "Accepted",
                "Process",
                json!({"name": "Branched", "description": null, "process_kind": null}),
            ),
            node(
                "pn:b-start",
                "Accepted",
                "ProcessNode",
                json!({"process_ref": "proc:branched", "node_kind": "start", "operation_ref": null, "condition_expr": null, "message_ref": null, "timer_expr": null}),
            ),
            node(
                "pn:b-gate",
                "Accepted",
                "ProcessNode",
                json!({"process_ref": "proc:branched", "node_kind": "exclusive_gateway", "operation_ref": null, "condition_expr": null, "message_ref": null, "timer_expr": null}),
            ),
            node(
                "pn:b-end",
                "Accepted",
                "ProcessNode",
                json!({"process_ref": "proc:branched", "node_kind": "end", "operation_ref": null, "condition_expr": null, "message_ref": null, "timer_expr": null}),
            ),
        ];
        let edges = vec![
            edge("rel:spec", "specified_by", "req:a", "op:submit"),
            edge("rel:reads", "reads", "op:submit", "entity:request"),
            edge("rel:writes", "writes", "op:approve", "entity:request"),
            edge("rel:attr", "has_attribute", "entity:request", "attr:days"),
            edge("rel:draft", "has_state", "entity:request", "state:draft"),
            edge(
                "rel:submitted",
                "has_state",
                "entity:request",
                "state:submitted",
            ),
            edge("rel:via", "transitions_via", "trans:submit", "op:submit"),
            edge("rel:b1", "next", "pn:b-start", "pn:b-gate"),
            edge("rel:b2", "next", "pn:b-gate", "pn:b-end"),
        ];
        (nodes, edges)
    }

    /// The S3.1 performer finding of op:approve and op:submit.
    fn performer_finding() -> GeneratedFinding {
        let affected = vec![id("op:approve"), id("op:submit")];
        let condition = "operation_performer_missing";
        let key = finding_key("PLUMB.F2.OPERATION.PERFORMER", &affected, condition).unwrap();
        GeneratedFinding {
            id: finding_id(&key).unwrap(),
            key,
            semantic_condition_key: condition.into(),
            payload: Finding {
                code: "PLUMB.F2.OPERATION.PERFORMER".into(),
                family: "F2".into(),
                severity: FindingSeverity::Blocker,
                message: "Operations need a performer.".into(),
                status: "Open".into(),
                affected_refs: affected,
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            },
        }
    }

    /// The fixture graph with the real S3.1 Questions of the performer finding.
    fn fixture_graph() -> Graph {
        fixture_graph_without(&[])
    }

    /// The fixture graph without the listed nodes.
    fn fixture_graph_without(excluded: &[&str]) -> Graph {
        let (nodes, edges) = base_parts();
        let nodes = nodes
            .into_iter()
            .filter(|n| !excluded.contains(&n.id.as_str()))
            .collect();
        let graph =
            Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"));
        let generated = generate_questions(
            &graph,
            &QuestionGenerationInput {
                findings: vec![performer_finding()],
                routing: StakeholderRoutingConfig::from_yaml(HR_STAKEHOLDERS).unwrap(),
            },
            &QuestionAudit {
                created_by: id("agent:compiler"),
                created_at: ts(T0),
            },
        )
        .unwrap();
        assert!(generated.issues.is_empty(), "{:?}", generated.issues);
        apply_patch(&graph, &generated.proposal.unwrap().patch_set)
            .unwrap()
            .graph
    }

    // ------------------------------------------------------------------ runtime doubles

    /// Answers a session turn from the planned request only: a reply and, when enabled, one
    /// Proposed requirement grounded in the user turn's evidence. Never touches the network.
    struct TestProvider {
        mode: Mutex<ProviderMode>,
        /// The semantic base of generated proposals: the turn baseline the test expects.
        base: Mutex<Option<Hash>>,
        /// When set, a concurrent writer commits to this database during the provider call.
        race: Mutex<Option<PathBuf>>,
        /// Every request and material the route handed over (Hotfix 066).
        received: Mutex<Vec<(InferenceRequest, InferenceRequestMaterial)>>,
        /// When set, GET these routes on another thread during the provider call.
        probe: Mutex<Option<(Router, Vec<String>)>>,
        /// The probe statuses, or `None` when a probe did not finish in time (a held lock).
        probed: Mutex<Option<Option<Vec<StatusCode>>>>,
    }

    /// GETs `uris` through `router` from a fresh thread and runtime, giving up after 20 s.
    fn probe_routes(router: Router, uris: Vec<String>) -> Option<Vec<StatusCode>> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            let statuses = runtime.block_on(async {
                let mut statuses = Vec::new();
                for uri in uris {
                    let request = Request::get(uri.as_str()).body(Body::empty()).unwrap();
                    statuses.push(send(&router, request).await.status);
                }
                statuses
            });
            let _ = tx.send(statuses);
        });
        rx.recv_timeout(std::time::Duration::from_secs(20)).ok()
    }

    impl TestProvider {
        fn set(&self, mode: ProviderMode, base: Option<&str>) {
            *self.mode.lock().unwrap() = mode;
            *self.base.lock().unwrap() = base.map(|b| b.parse().unwrap());
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum ProviderMode {
        Reply,
        Propose,
        Fail,
    }

    fn execution(request: &InferenceRequest, output: Value) -> ProviderExecution {
        let raw_response = to_canonical_json(&output).unwrap();
        let validated_output = CanonicalJson::new(output);
        ProviderExecution {
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
        }
    }

    impl InferenceProvider for TestProvider {
        fn execute(
            &self,
            request: &InferenceRequest,
            material: &InferenceRequestMaterial,
        ) -> Result<ProviderExecution, ProviderError> {
            // Like every provider, bind the material before any work.
            validate_binding(request, material)?;
            self.received
                .lock()
                .unwrap()
                .push((request.clone(), material.clone()));
            if let Some((router, uris)) = self.probe.lock().unwrap().take() {
                *self.probed.lock().unwrap() = Some(probe_routes(router, uris));
            }
            if let Some(path) = self.race.lock().unwrap().take() {
                concurrent_commit(&path);
            }
            let mode = *self.mode.lock().unwrap();
            let proposals = match mode {
                ProviderMode::Fail => return Err(ProviderError::ProviderDisabled),
                ProviderMode::Reply => Vec::new(),
                ProviderMode::Propose => {
                    let evidence = request.evidence_refs.last().unwrap().clone();
                    let mut new = requirement(
                        &format!("req:s-{}", &request.id.as_str()["sha256:".len()..][..12]),
                        "Proposed",
                        "Employees shall see their remaining leave.",
                    );
                    new.evidence = vec![EvidenceRef::from(evidence.clone())];
                    let base = self.base.lock().unwrap().clone().unwrap();
                    vec![serde_json::to_value(
                        Proposal::new(
                            request.stage,
                            PatchSet {
                                base_semantic_hash: base,
                                patch: SemanticPatch::AddNode { node: new },
                            },
                            vec![EvidenceRef::from(evidence.clone())],
                            Vec::new(),
                            ProposalMateriality::Semantic,
                            AcceptancePolicy::HumanConfirm,
                            None,
                        )
                        .unwrap(),
                    )
                    .unwrap()]
                }
            };
            Ok(execution(
                request,
                json!({"version": 1, "reply": "Noted.", "proposals": proposals, "asked_question_ref": null}),
            ))
        }
    }

    /// Another writer: commits one Proposed requirement directly to the store.
    fn concurrent_commit(path: &Path) {
        let mut store = SqliteRevisionStore::open(path).unwrap();
        let head = store.head(&BranchName::main()).unwrap().unwrap();
        let loaded = store.load_revision(&head).unwrap();
        store
            .commit(
                &BranchName::main(),
                &head,
                &PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch: SemanticPatch::AddNode {
                        node: requirement("req:racer", "Proposed", "Another writer was faster."),
                    },
                },
                RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id("agent:other"),
                    created_at: ts(NOW),
                },
            )
            .unwrap();
    }

    struct NoPredicates;

    impl PredicateProvider for NoPredicates {
        fn direct_manager(
            &self,
            _: &ExprRefValue,
            _: &ExprRefValue,
        ) -> Result<bool, ExprPredicateError> {
            Err(ExprPredicateError {
                reason: "no predicates in this test".into(),
            })
        }
    }

    // ------------------------------------------------------------------ app fixture

    struct App {
        _dir: tempfile::TempDir,
        path: PathBuf,
        state: ApiState,
        router: Router,
        provider: Arc<TestProvider>,
    }

    fn profile() -> ValidationProfile {
        load_builtin_software_profile().unwrap()
    }

    fn fixed() -> Arc<dyn Clock + Send + Sync> {
        Arc::new(plumb_core::FixedClock::new(ts(NOW)))
    }

    fn identity(actor: &str) -> ApiIdentity {
        ApiIdentity::new(
            "alice".into(),
            id(actor),
            vec!["analyst".into()],
            "pilot".into(),
        )
        .unwrap()
    }

    fn runtime(path: &Path, provider: Arc<TestProvider>) -> A03RuntimeConfig {
        A03RuntimeConfig {
            workflow_store: SqliteWorkflowStore::open(path).unwrap(),
            inference_provider: provider,
            provider_policy: ProviderPolicy {
                provider: "test".into(),
                config: CanonicalJson::new(json!({})),
            },
            session_turn_config: SessionTurnConfig {
                retrieval_top_k: 3,
                max_open_questions: 3,
                max_context_bytes: 65536,
            },
            assistant_actor_ref: id(ASSISTANT),
            calendars: Arc::new(
                ExprStaticCalendarProvider::new([ExprCalendarDefinition::new(
                    id("cal:office"),
                    [],
                    [],
                )])
                .unwrap(),
            ),
            predicates: Arc::new(NoPredicates),
            expression_scopes: Vec::new(),
            calculation_scope_overrides: Vec::new(),
            runtime_semantic_values: Vec::new(),
        }
    }

    /// Builds the full runtime state over the database at `path`.
    fn state_at(
        path: &Path,
        provider: Arc<TestProvider>,
        reports: Vec<plumb_validation::GateReport>,
        actor: &str,
    ) -> ApiState {
        state_at_profile(path, provider, reports, actor, profile())
    }

    fn state_at_profile(
        path: &Path,
        provider: Arc<TestProvider>,
        reports: Vec<plumb_validation::GateReport>,
        actor: &str,
        profile: ValidationProfile,
    ) -> ApiState {
        ApiState::new_with_runtime(
            BranchName::main(),
            SqliteRevisionStore::open(path).unwrap(),
            SqliteArtifactStore::open(path).unwrap(),
            identity(actor),
            fixed(),
            reports,
            profile,
            runtime(path, provider),
        )
        .unwrap()
    }

    fn app_with(
        graph: Graph,
        reports: impl FnOnce(&Graph) -> Vec<plumb_validation::GateReport>,
    ) -> App {
        app_with_profile(graph, reports, profile())
    }

    /// [`app_with`] governed by one explicit validated `profile` (Hotfix 081): it supplies the
    /// initial revision's profile and rule-pack hashes and the `ApiState` profile alike.
    fn app_with_profile(
        graph: Graph,
        reports: impl FnOnce(&Graph) -> Vec<plumb_validation::GateReport>,
        profile: ValidationProfile,
    ) -> App {
        profile.validate().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plumb.sqlite");
        let mut store = SqliteRevisionStore::open(&path).unwrap();
        let rev = store
            .create_initial_revision(
                &graph,
                profile.profile_hash().unwrap(),
                profile.rule_pack_hash().unwrap(),
                RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id("agent:other"),
                    created_at: ts(T0),
                },
            )
            .unwrap();
        let head = store.load_revision(&rev.id).unwrap();
        drop(store);
        let provider = Arc::new(TestProvider {
            mode: Mutex::new(ProviderMode::Reply),
            base: Mutex::new(None),
            race: Mutex::new(None),
            received: Mutex::new(Vec::new()),
            probe: Mutex::new(None),
            probed: Mutex::new(None),
        });
        let state = state_at_profile(
            &path,
            provider.clone(),
            reports(&head.graph),
            ACTOR,
            profile,
        );
        let router = router(state.clone());
        App {
            _dir: dir,
            path,
            state,
            router,
            provider,
        }
    }

    fn app() -> App {
        app_with(fixture_graph(), |_| Vec::new())
    }

    /// Rebuilds the state over the same database, as after a process restart.
    fn restarted(app: App, actor: &str) -> App {
        let App {
            _dir,
            path,
            provider,
            ..
        } = app;
        let state = state_at(&path, provider.clone(), Vec::new(), actor);
        let router = router(state.clone());
        App {
            _dir,
            path,
            state,
            router,
            provider,
        }
    }

    struct Reply {
        status: StatusCode,
        headers: axum::http::HeaderMap,
        bytes: Vec<u8>,
    }

    impl Reply {
        fn json(&self) -> Value {
            serde_json::from_slice(&self.bytes)
                .unwrap_or_else(|e| panic!("{e}: {:?}", String::from_utf8_lossy(&self.bytes)))
        }

        fn code(&self) -> String {
            self.json()["code"].as_str().unwrap_or_default().to_owned()
        }

        fn text(&self) -> String {
            String::from_utf8(self.bytes.clone()).unwrap()
        }
    }

    async fn send(router: &Router, request: Request<Body>) -> Reply {
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        Reply {
            status,
            headers,
            bytes,
        }
    }

    async fn get(app: &App, uri: &str) -> Reply {
        send(&app.router, Request::get(uri).body(Body::empty()).unwrap()).await
    }

    async fn post(app: &App, uri: &str, if_match: Option<&str>, body: Option<Value>) -> Reply {
        let mut builder = Request::post(uri);
        if let Some(tag) = if_match {
            builder = builder.header("if-match", tag);
        }
        let body = match body {
            Some(v) => {
                builder = builder.header("content-type", "application/json");
                Body::from(serde_json::to_vec(&v).unwrap())
            }
            None => Body::empty(),
        };
        send(&app.router, builder.body(body).unwrap()).await
    }

    /// The head semantic hash (the current If-Match value).
    async fn semantic(app: &App) -> String {
        get(app, "/api/model/version").await.json()["semantic_hash"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn revision(app: &App) -> String {
        get(app, "/api/model/version").await.json()["revision"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// A POST with the current If-Match.
    async fn write(app: &App, uri: &str, body: Option<Value>) -> Reply {
        let tag = semantic(app).await;
        post(app, uri, Some(&tag), body).await
    }

    /// The head graph straight from the store.
    fn head(app: &App) -> plumb_store::LoadedRevision {
        let store = SqliteRevisionStore::open(&app.path).unwrap();
        let rev = store.head(&BranchName::main()).unwrap().unwrap();
        store.load_revision(&rev).unwrap()
    }

    fn workflow(app: &App) -> SqliteWorkflowStore {
        SqliteWorkflowStore::open(&app.path).unwrap()
    }

    fn assert_error(reply: &Reply, status: StatusCode, code: &str) {
        assert_eq!(
            (reply.status, reply.code().as_str()),
            (status, code),
            "{}",
            String::from_utf8_lossy(&reply.bytes)
        );
    }

    /// The single question about `subject` (context_refs[0]).
    fn question_about(graph: &Graph, subject: &str) -> Id {
        graph
            .nodes()
            .values()
            .find(|n| {
                matches!(&n.payload, NodePayload::Question(q)
                    if q.context_refs.as_ref().is_some_and(|c| c[0] == id(subject)))
            })
            .unwrap_or_else(|| panic!("no question about {subject}"))
            .id
            .clone()
    }

    // ================================================================== tests

    #[tokio::test]
    async fn http_workflow_fixture_is_valid() {
        let app = app();
        let r = get(&app, "/api/model/version").await;
        assert_eq!(r.status, StatusCode::OK);
        // The fixture carries the real S3.1 performer Questions and the A0.3 runtime.
        let g = head(&app).graph;
        assert_eq!(performer_questions(&g).len(), 2);
        assert!(g
            .node(&id(ACTOR))
            .is_some_and(|n| n.payload.node_type() == NodeType::Agent));
        assert_eq!(app.state.identity().user(), "alice");
    }

    // ------------------------------------------------------------------ search

    /// A cursor v2 continuing after `after_id` for `query` at the current head.
    async fn forged_cursor(app: &App, operation_id: &str, query: Value, after_id: &str) -> String {
        let request_hash = Hash::content_sha256(
            &to_canonical_json(&json!({"operation_id": operation_id, "path": {}, "query": query}))
                .unwrap(),
        );
        let cursor = json!({
            "after_id": after_id,
            "request_hash": request_hash.as_str(),
            "revision": revision(app).await,
            "semantic_hash": semantic(app).await,
        });
        hex::encode(to_canonical_json(&cursor).unwrap())
    }

    #[tokio::test]
    async fn http_workflow_search_ranks_filters_and_pages() {
        let app = app();
        let r = get(&app, "/api/search?q=leave").await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let items = r.json()["items"].as_array().unwrap().clone();
        assert!(items.len() >= 3);
        // Ranked by BM25 score descending, then ID ascending; positive scores only.
        for pair in items.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let (sa, sb) = (a["score"].as_f64().unwrap(), b["score"].as_f64().unwrap());
            assert!(sa > 0.0 && sb > 0.0);
            assert!(sa > sb || (sa == sb && a["id"].as_str() < b["id"].as_str()));
        }
        let req_a = items.iter().find(|i| i["id"] == "req:a").unwrap();
        assert_eq!(req_a["kind"], "requirement");
        assert_eq!(req_a["path"], "/api/requirements/req:a");
        assert_eq!(req_a["snippet"], "Employees shall submit leave requests.");
        // Scope filters by retrieval family; a family without a route has a null path.
        let rules = get(&app, "/api/search?q=positive&scope=rule").await.json();
        assert_eq!(rules["items"][0]["id"], "rule:positive");
        assert_eq!(rules["items"][0]["path"], "/api/model/rules/rule:positive");
        assert!(rules["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["kind"] == "rule"));
        let calcs = get(&app, "/api/search?q=double_days&scope=calculation")
            .await
            .json();
        assert_eq!(calcs["items"][0]["path"], Value::Null);
        // Cursor v2 follows the ranked order exactly.
        let first = get(&app, "/api/search?q=leave&limit=1").await.json();
        assert_eq!(first["items"][0], items[0]);
        let cursor = first["next_cursor"].as_str().unwrap();
        let second = get(
            &app,
            &format!("/api/search?q=leave&limit=1&cursor={cursor}"),
        )
        .await
        .json();
        assert_eq!(second["items"][0], items[1]);
        let forged = forged_cursor(&app, "search_model", json!({"q": "leave"}), "req:p").await;
        assert_error(
            &get(&app, &format!("/api/search?q=leave&cursor={forged}")).await,
            StatusCode::BAD_REQUEST,
            "E_CURSOR_INVALID",
        );
        let other = get(&app, &format!("/api/search?q=approve&cursor={cursor}")).await;
        assert_error(&other, StatusCode::BAD_REQUEST, "E_CURSOR_REQUEST_MISMATCH");
        for bad in [
            "/api/search",
            "/api/search?q=%20",
            "/api/search?q=x&scope=everything",
        ] {
            assert_error(
                &get(&app, bad).await,
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            );
        }
    }

    // ------------------------------------------------------------------ requirements

    /// The Accepted proposal-acceptance decisions of the head.
    fn acceptance_decisions(graph: &Graph) -> Vec<(Id, plumb_psg::ResolutionDecision)> {
        graph
            .nodes()
            .values()
            .filter_map(|n| match &n.payload {
                NodePayload::ResolutionDecision(d) if d.answer["kind"] == "proposal_acceptance" => {
                    Some((n.id.clone(), d.clone()))
                }
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn http_workflow_split_is_governed_with_a_companion_decision() {
        let app = app();
        let body = json!({"parts": ["Employees shall submit leave requests.", "Employees shall cancel leave requests."], "reason": "Two behaviours."});
        // Body validation precedes If-Match; If-Match is required.
        assert_error(
            &post(
                &app,
                "/api/requirements/req:c/split",
                Some("psg:sha256:stale"),
                Some(body.clone()),
            )
            .await,
            StatusCode::PRECONDITION_FAILED,
            "E_PRECONDITION_FAILED",
        );
        for bad in [
            json!({"parts": ["Only one."], "reason": "x"}),
            json!({"parts": ["A.", " B."], "reason": "x"}),
            json!({"parts": ["A.", "A."], "reason": "x"}),
            json!({"parts": ["A.", "B."], "reason": " padded"}),
        ] {
            assert_error(
                &write(&app, "/api/requirements/req:c/split", Some(bad)).await,
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            );
        }
        assert_error(
            &write(
                &app,
                "/api/requirements/req:nowhere/split",
                Some(body.clone()),
            )
            .await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        assert_error(
            &write(&app, "/api/requirements/req:p/split", Some(body.clone())).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_SEMANTIC_INVARIANT",
        );
        let before = revision(&app).await;
        let r = write(&app, "/api/requirements/req:c/split", Some(body)).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let w = r.json();
        assert_ne!(w["revision"].as_str().unwrap(), before);
        assert_eq!(w["events"][0]["kind"], "split_requirement");
        let loaded = head(&app);
        let g = &loaded.graph;
        assert_eq!(
            g.node(&id("req:c")).unwrap().status,
            ElementStatus::Superseded
        );
        let children: Vec<&Node> = g
            .nodes()
            .values()
            .filter(|n| {
                n.extensions
                    .keys()
                    .any(|k| k.as_str() == "plumb_functional:requirement_split_origin")
            })
            .collect();
        assert_eq!(children.len(), 2);
        // The companion decision is in decision_refs and its patch artifact is the original.
        let decisions = acceptance_decisions(g);
        assert_eq!(decisions.len(), 1);
        let (decision_ref, decision) = &decisions[0];
        assert_eq!(decision.question_ref, None);
        assert_eq!(decision.rationale.as_deref(), Some("Two behaviours."));
        assert_eq!(decision.decided_by, id(ACTOR));
        assert_eq!(decision.decided_at, ts(NOW));
        // Reason codes are the fresh intake's sorted wire strings (one child restates req:a).
        assert_eq!(
            decision.answer,
            json!({"kind": "proposal_acceptance", "reason_codes": ["exact_equivalent", "human_decision", "material_decision"]})
        );
        assert_eq!(loaded.revision.decision_refs, vec![decision_ref.clone()]);
        let artifacts = SqliteArtifactStore::open(&app.path).unwrap();
        let original = artifacts.get(&decision.patch_ref).unwrap();
        assert_eq!(original.kind, ArtifactKind::Patch);
        let original: PatchSet = serde_json::from_slice(&original.bytes).unwrap();
        // The accepted (final) patch is Compound {original, AddNode companion}.
        let accepted = artifacts
            .get(loaded.revision.accepted_patch_ref.as_ref().unwrap())
            .unwrap();
        let accepted: PatchSet = serde_json::from_slice(&accepted.bytes).unwrap();
        let SemanticPatch::Compound { patches } = &accepted.patch else {
            panic!("not a Compound")
        };
        assert_eq!(patches[0], original.patch);
        assert!(matches!(&patches[1], SemanticPatch::AddNode { node } if &node.id == decision_ref));
    }

    #[tokio::test]
    async fn http_workflow_duplicate_supersession_and_term_merge() {
        let app = app();
        assert_error(
            &write(
                &app,
                "/api/requirements/req:b/mark-duplicate",
                Some(json!({"of": "req:b", "reason": "Same."})),
            )
            .await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &write(
                &app,
                "/api/requirements/req:b/mark-duplicate",
                Some(json!({"of": "req:ghost", "reason": "Same."})),
            )
            .await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        let r = write(
            &app,
            "/api/requirements/req:b/mark-duplicate",
            Some(json!({"of": "req:a", "reason": "Same."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let g = head(&app).graph;
        assert_eq!(
            g.node(&id("req:b")).unwrap().status,
            ElementStatus::Superseded
        );
        assert_eq!(
            g.node(&id("req:a")).unwrap().status,
            ElementStatus::Accepted
        );
        assert!(g
            .edges()
            .values()
            .any(|e| e.kind == plumb_psg::RelationKind::Supersedes
                && e.from == id("req:a")
                && e.to == id("req:b")));

        // Term merge: language conflict, then the alias union under the real merge.
        assert_error(
            &write(
                &app,
                "/api/glossary/term:urlaub/merge",
                Some(json!({"into": "term:leave", "reason": "Same."})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_TERM_MERGE_CONFLICT",
        );
        assert_error(
            &write(
                &app,
                "/api/glossary/term:leave/merge",
                Some(json!({"into": "term:leave", "reason": "Same."})),
            )
            .await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        let r = write(
            &app,
            "/api/glossary/term:vacation/merge",
            Some(json!({"into": "term:leave", "reason": "Synonym."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let g = head(&app).graph;
        assert!(g.node(&id("term:vacation")).is_none());
        let NodePayload::Term(t) = &g.node(&id("term:leave")).unwrap().payload else {
            panic!()
        };
        assert_eq!(
            t.aliases,
            Some(vec![
                "absence".to_owned(),
                "holiday".to_owned(),
                "vacation".to_owned()
            ])
        );
        assert_eq!(acceptance_decisions(&g).len(), 2);
    }

    // ------------------------------------------------------------------ semantic change previews (Hotfix 073)

    const SPLIT_BODY: &str = r#"{"parts": ["Employees shall submit leave requests.", "Employees shall cancel leave requests."], "reason": "Two behaviours."}"#;
    const MERGE_BODY: &str = r#"{"into": "term:leave", "reason": "Synonym."}"#;

    /// The branch-head identity as `GET /api/model/version` reports it.
    async fn version(app: &App) -> Value {
        get(app, "/api/model/version").await.json()
    }

    /// The expected semantic element diff of `patch_set` on `base`, recomputed independently from
    /// the apply_patch delta: added in the candidate, removed in the base, modified in either.
    fn expected_diff(base: &Graph, patch_set: &PatchSet) -> Value {
        let applied = apply_patch(base, patch_set).unwrap();
        let (candidate, delta) = (&applied.graph, &applied.delta);
        let node = |g: &Graph, i: &Id| g.node(i).is_some_and(node_contributes_to_semantic_hash);
        let edge = |g: &Graph, i: &Id| {
            g.edge(i)
                .is_some_and(|e| edge_contributes_to_semantic_hash(g, e))
        };
        let ids = |list: Vec<&Id>| {
            let mut v: Vec<String> = list.into_iter().map(|i| i.to_string()).collect();
            v.sort();
            v.dedup();
            v
        };
        let added = ids(delta
            .added_nodes
            .iter()
            .filter(|i| node(candidate, i))
            .chain(delta.added_edges.iter().filter(|i| edge(candidate, i)))
            .collect());
        let removed = ids(delta
            .removed_nodes
            .iter()
            .filter(|i| node(base, i))
            .chain(delta.removed_edges.iter().filter(|i| edge(base, i)))
            .collect());
        let modified = ids(delta
            .modified_nodes
            .iter()
            .filter(|i| node(base, i) || node(candidate, i))
            .chain(
                delta
                    .modified_edges
                    .iter()
                    .filter(|i| edge(base, i) || edge(candidate, i)),
            )
            .collect());
        json!({
            "base_semantic_hash": delta.base_semantic_hash.to_string(),
            "candidate_semantic_hash": delta.result_semantic_hash.to_string(),
            "added": added, "removed": removed, "modified": modified
        })
    }

    /// Asserts that `reply` is a preview of a pending `kind` proposal persisted on the unchanged
    /// head `before` (a `GET /api/model/version` body) and returns the stored Proposal.
    fn assert_preview(
        app: &App,
        reply: &Reply,
        before: &Value,
        base: &Graph,
        action: &str,
        kind: WorkflowProposalKind,
    ) -> Proposal {
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text());
        let body = reply.json();
        // Baseline metadata, no events, no candidate revision.
        assert_eq!(body["version"], before["revision"]);
        assert_eq!(body["revision"], before["revision"]);
        assert_eq!(body["semantic_hash"], before["semantic_hash"]);
        assert_eq!(body["diff"]["base_semantic_hash"], before["semantic_hash"]);
        assert_eq!(body["events"], json!([]));
        assert_eq!(body["action"], action);
        let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            vec![
                "action",
                "diff",
                "events",
                "intake",
                "proposal_id",
                "revision",
                "semantic_hash",
                "version"
            ]
        );
        // The real pending workflow proposal, bound to the exact baseline revision.
        let stored = workflow(app)
            .proposal(&id(body["proposal_id"].as_str().unwrap()))
            .unwrap()
            .unwrap();
        assert_eq!(stored.state, WorkflowProposalState::Pending);
        assert_eq!(stored.proposal_kind, kind);
        assert_eq!(stored.origin_kind, WorkflowOrigin::Intake);
        assert_eq!(stored.origin_ref, None);
        assert_eq!(
            stored.base_revision_id.to_string(),
            before["revision"].as_str().unwrap()
        );
        let proposal: Proposal = serde_json::from_str(&stored.proposal_json).unwrap();
        assert_eq!(
            proposal.id.to_string(),
            body["proposal_id"].as_str().unwrap()
        );
        assert_eq!(
            proposal.patch_set.base_semantic_hash.to_string(),
            before["semantic_hash"].as_str().unwrap()
        );
        // The diff is exactly the semantic element effect of the persisted PatchSet.
        assert_eq!(body["diff"], expected_diff(base, &proposal.patch_set));
        for list in ["added", "removed", "modified"] {
            let ids: Vec<&str> = body["diff"][list]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            let mut sorted = ids.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(ids, sorted, "{list} sorted and unique");
        }
        proposal
    }

    #[tokio::test]
    async fn http_workflow_split_preview_then_create_commits_the_exact_proposal() {
        let app = app();
        let body: Value = serde_json::from_str(SPLIT_BODY).unwrap();
        let uri = "/api/requirements/req:c/split/preview";
        // If-Match is required and must be current.
        assert_error(
            &post(&app, uri, None, Some(body.clone())).await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &post(&app, uri, Some("psg:sha256:stale"), Some(body.clone())).await,
            StatusCode::PRECONDITION_FAILED,
            "E_PRECONDITION_FAILED",
        );
        let before = version(&app).await;
        let base = head(&app);
        let r = write(&app, uri, Some(body)).await;
        let proposal = assert_preview(
            &app,
            &r,
            &before,
            &base.graph,
            "create",
            WorkflowProposalKind::Create,
        );
        let preview = r.json();
        // The stored split Proposal classifies independently as Create (Hotfix 074): it adds
        // Requirements and decomposes_to edges and carries no Supersede primitive.
        let mut leaves = vec![&proposal.patch_set.patch];
        let mut flat = Vec::new();
        while let Some(p) = leaves.pop() {
            match p {
                SemanticPatch::Compound { patches } => leaves.extend(patches.iter()),
                leaf => flat.push(leaf),
            }
        }
        assert!(!flat
            .iter()
            .any(|p| matches!(p, SemanticPatch::Supersede { .. })));
        assert!(flat
            .iter()
            .any(|p| matches!(p, SemanticPatch::AddNode { .. })));
        assert!(flat.iter().any(|p| matches!(
            p,
            SemanticPatch::SetStatus {
                to: ElementStatus::Superseded,
                ..
            }
        )));
        // Nothing was committed: same revision, semantic and evidence hashes.
        assert_eq!(version(&app).await, before);
        assert_eq!(head(&app).revision.id, base.revision.id);
        // The split's domain effect: req:c stays in the projection as Superseded (modified), and
        // the two Accepted children with their decomposes_to edges are added.
        let added: Vec<String> = serde_json::from_value(preview["diff"]["added"].clone()).unwrap();
        let children: Vec<&String> = added.iter().filter(|i| i.starts_with("req:")).collect();
        assert_eq!(children.len(), 2, "{added:?}");
        assert!(preview["diff"]["modified"]
            .as_array()
            .unwrap()
            .contains(&json!("req:c")));
        // No acceptance companion is fabricated by the preview.
        let in_diff: Vec<&Value> = ["added", "removed", "modified"]
            .iter()
            .flat_map(|l| preview["diff"][*l].as_array().unwrap())
            .collect();
        assert!(acceptance_decisions(&head(&app).graph).is_empty());
        // An identical preview on the same head is the same pending proposal.
        let again = write(&app, uri, Some(serde_json::from_str(SPLIT_BODY).unwrap())).await;
        assert_eq!(again.json()["proposal_id"], preview["proposal_id"]);
        assert_eq!(version(&app).await, before);
        // The normal A0.3 action-kind check refuses a supersede action on the Create-class split
        // Proposal; nothing is committed and the proposal stays pending.
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{}/supersede", proposal.id),
                Some(json!({"reason": "Two behaviours."})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_ACTION_MISMATCH",
        );
        assert_eq!(version(&app).await, before);
        assert_eq!(
            workflow(&app)
                .proposal(&proposal.id)
                .unwrap()
                .unwrap()
                .state,
            WorkflowProposalState::Pending
        );
        // The direct split endpoint is not involved: confirmation is the create action.
        let r = write(
            &app,
            &format!("/api/proposals/{}/create", proposal.id),
            Some(json!({"reason": "Two behaviours."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let committed = r.json();
        assert_ne!(committed["revision"], before["revision"]);
        let loaded = head(&app);
        assert_eq!(
            loaded.revision.id.to_string(),
            committed["revision"].as_str().unwrap()
        );
        let g = &loaded.graph;
        assert_eq!(
            g.node(&id("req:c")).unwrap().status,
            ElementStatus::Superseded
        );
        for child in &children {
            assert_eq!(g.node(&id(child)).unwrap().status, ElementStatus::Accepted);
            assert!(g.edges().values().any(|e| {
                e.kind == plumb_psg::RelationKind::DecomposesTo
                    && e.from == id("req:c")
                    && e.to == id(child)
                    && e.status == ElementStatus::Accepted
            }));
        }
        let stored = workflow(&app).proposal(&proposal.id).unwrap().unwrap();
        assert_eq!(stored.state, WorkflowProposalState::Accepted);
        // Governance adds the acceptance ResolutionDecision on top of the previewed effect: the
        // decision is not part of the preview diff, and (R0.5) as a proposal-acceptance
        // companion it is excluded from semantic_hash, so the committed semantic hash is exactly
        // the previewed candidate's.
        let decisions = acceptance_decisions(g);
        assert_eq!(decisions.len(), 1);
        let decision = decisions[0].0.to_string();
        assert!(!in_diff.contains(&&json!(decision)));
        assert_eq!(decisions[0].1.proposal_ref, Some(proposal.id.clone()));
        assert_eq!(
            committed["semantic_hash"].as_str().unwrap(),
            loaded.revision.semantic_hash.to_string()
        );
        assert_eq!(
            committed["semantic_hash"], preview["diff"]["candidate_semantic_hash"],
            "the proposal-acceptance companion does not participate in semantic_hash"
        );
        // Every previewed semantic effect is present in the committed revision.
        for added in preview["diff"]["added"].as_array().unwrap() {
            let i = id(added.as_str().unwrap());
            assert!(g.node(&i).is_some() || g.edge(&i).is_some(), "{i}");
        }
    }

    fn in_list(diff: &Value, list: &str, element: &str) -> bool {
        diff[list].as_array().unwrap().contains(&json!(element))
    }

    #[tokio::test]
    async fn http_workflow_term_merge_preview_then_merge_commits_the_exact_proposal() {
        let app = app();
        let uri = "/api/glossary/term:vacation/merge/preview";
        let body: Value = serde_json::from_str(MERGE_BODY).unwrap();
        assert_error(
            &post(&app, uri, None, Some(body.clone())).await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &post(&app, uri, Some("psg:sha256:stale"), Some(body.clone())).await,
            StatusCode::PRECONDITION_FAILED,
            "E_PRECONDITION_FAILED",
        );
        let before = version(&app).await;
        let base = head(&app);
        let r = write(&app, uri, Some(body)).await;
        let proposal = assert_preview(
            &app,
            &r,
            &before,
            &base.graph,
            "merge",
            WorkflowProposalKind::Merge,
        );
        let preview = r.json();
        assert_eq!(version(&app).await, before);
        assert!(head(&app).graph.node(&id("term:vacation")).is_some());
        // Hotfix 074 semantic projection: the Proposed source Term is outside the base semantic
        // projection, so merging it away does not put it in removed; the Accepted target is
        // modified. The diff lists semantic-hash participants, not every touched element.
        assert_eq!(
            base.graph.node(&id("term:vacation")).unwrap().status,
            ElementStatus::Proposed
        );
        assert!(!node_contributes_to_semantic_hash(
            base.graph.node(&id("term:vacation")).unwrap()
        ));
        let touched = apply_patch(&base.graph, &proposal.patch_set).unwrap().delta;
        assert!(touched.removed_nodes.contains(&id("term:vacation")));
        assert_eq!(preview["diff"]["removed"], json!([]));
        assert!(!in_list(&preview["diff"], "added", "term:vacation"));
        assert!(!in_list(&preview["diff"], "modified", "term:vacation"));
        assert!(in_list(&preview["diff"], "modified", "term:leave"));
        let r = write(
            &app,
            &format!("/api/proposals/{}/merge", proposal.id),
            Some(json!({"reason": "Synonym."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let g = head(&app).graph;
        assert!(g.node(&id("term:vacation")).is_none());
        let NodePayload::Term(t) = &g.node(&id("term:leave")).unwrap().payload else {
            panic!()
        };
        assert_eq!(
            t.aliases,
            Some(vec![
                "absence".to_owned(),
                "holiday".to_owned(),
                "vacation".to_owned()
            ])
        );
        assert_eq!(
            workflow(&app)
                .proposal(&proposal.id)
                .unwrap()
                .unwrap()
                .state,
            WorkflowProposalState::Accepted
        );
        assert_eq!(acceptance_decisions(&g).len(), 1);
    }

    #[tokio::test]
    async fn http_workflow_stale_previews_are_rejected_without_rebase() {
        let app = app();
        let split = write(
            &app,
            "/api/requirements/req:c/split/preview",
            Some(serde_json::from_str(SPLIT_BODY).unwrap()),
        )
        .await;
        let merge = write(
            &app,
            "/api/glossary/term:vacation/merge/preview",
            Some(serde_json::from_str(MERGE_BODY).unwrap()),
        )
        .await;
        assert_eq!(
            (split.status, merge.status),
            (StatusCode::OK, StatusCode::OK)
        );
        let a = version(&app).await;
        // Another write advances the head from revision A to revision B.
        let r = write(
            &app,
            "/api/requirements/req:b/mark-duplicate",
            Some(json!({"of": "req:a", "reason": "Same."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let b = version(&app).await;
        assert_ne!(a["revision"], b["revision"]);
        for (reply, action, reason) in [
            (&split, "create", "Two behaviours."),
            (&merge, "merge", "Synonym."),
        ] {
            let proposal_id = reply.json()["proposal_id"].as_str().unwrap().to_owned();
            // The confirmation carries the latest If-Match; the proposal stays bound to A.
            let r = write(
                &app,
                &format!("/api/proposals/{proposal_id}/{action}"),
                Some(json!({"reason": reason})),
            )
            .await;
            assert_error(&r, StatusCode::CONFLICT, "E_PROPOSAL_BASELINE_CHANGED");
            let stored = workflow(&app).proposal(&id(&proposal_id)).unwrap().unwrap();
            assert_eq!(stored.state, WorkflowProposalState::Pending);
            assert_eq!(
                stored.base_revision_id.to_string(),
                a["revision"].as_str().unwrap()
            );
        }
        // Nothing was committed, rebased or retried.
        assert_eq!(version(&app).await, b);
        let g = head(&app).graph;
        assert_eq!(
            g.node(&id("req:c")).unwrap().status,
            ElementStatus::Accepted
        );
        assert!(g.node(&id("term:vacation")).is_some());
    }

    #[tokio::test]
    async fn http_workflow_previews_validate_exactly_like_the_direct_writes() {
        let app = app();
        let before = version(&app).await;
        for bad in [
            json!({"parts": ["Only one."], "reason": "x"}),
            json!({"parts": ["A.", " B."], "reason": "x"}),
            json!({"parts": ["A.", "A."], "reason": "x"}),
            json!({"parts": ["A.", "B."], "reason": " padded"}),
            json!({"parts": ["A.", "B."]}),
        ] {
            for uri in [
                "/api/requirements/req:c/split",
                "/api/requirements/req:c/split/preview",
            ] {
                assert_error(
                    &write(&app, uri, Some(bad.clone())).await,
                    StatusCode::BAD_REQUEST,
                    "E_INVALID_REQUEST",
                );
            }
        }
        let split: Value = serde_json::from_str(SPLIT_BODY).unwrap();
        for (uri, status, code) in [
            (
                "/api/requirements/req:nowhere/split/preview",
                StatusCode::NOT_FOUND,
                "E_NOT_FOUND",
            ),
            (
                "/api/requirements/req:p/split/preview",
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_SEMANTIC_INVARIANT",
            ),
        ] {
            assert_error(&write(&app, uri, Some(split.clone())).await, status, code);
        }
        for (uri, body, status, code) in [
            (
                "/api/glossary/term:urlaub/merge/preview",
                json!({"into": "term:leave", "reason": "Same."}),
                StatusCode::CONFLICT,
                "E_TERM_MERGE_CONFLICT",
            ),
            (
                "/api/glossary/term:leave/merge/preview",
                json!({"into": "term:leave", "reason": "Same."}),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/glossary/term:vacation/merge/preview",
                json!({"into": "not an id", "reason": "Same."}),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/glossary/term:ghost/merge/preview",
                json!({"into": "term:leave", "reason": "Same."}),
                StatusCode::NOT_FOUND,
                "E_NOT_FOUND",
            ),
        ] {
            assert_error(&write(&app, uri, Some(body)).await, status, code);
        }
        // No rejected preview persisted or committed anything.
        assert_eq!(version(&app).await, before);
    }

    // ------------------------------------------------------------------ sentences and CRUD

    #[tokio::test]
    async fn http_workflow_sentences_project_and_write_requirements() {
        let app = app();
        let r = get(&app, "/api/model/sentences").await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let items = r.json()["items"].as_array().unwrap().clone();
        let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["req:a", "req:b", "req:c", "req:p"]);
        assert_eq!(
            items[0],
            json!({
                "id": "req:a", "text": "Employees shall submit leave requests.",
                "slots": {"class": "functional", "level": "system", "modality": "shall"},
                "status": "Accepted", "evidence": [], "unknowns": []
            })
        );
        let proposed = get(&app, "/api/model/sentences?status=Proposed")
            .await
            .json();
        assert_eq!(proposed["items"].as_array().unwrap().len(), 1);
        let by_entity = get(&app, "/api/model/sentences?entity=entity:request")
            .await
            .json();
        let ids: Vec<&str> = by_entity["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["req:a"]);
        assert_error(
            &get(&app, "/api/model/sentences?entity=op:submit").await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        assert_error(
            &get(&app, "/api/model/sentences?status=Open").await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        let page = get(&app, "/api/model/sentences?limit=2").await.json();
        let next = get(
            &app,
            &format!(
                "/api/model/sentences?limit=2&cursor={}",
                page["next_cursor"].as_str().unwrap()
            ),
        )
        .await
        .json();
        assert_eq!(next["items"][0]["id"], "req:c");

        // Writes: edit slots, accept and reject only Proposed sentences.
        assert_error(
            &write(&app, "/api/model/sentences/req:a/accept", None).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_SEMANTIC_INVARIANT",
        );
        assert_error(
            &write(
                &app,
                "/api/model/sentences/req:a/edit",
                Some(json!({"slots": {"class": null, "level": null, "modality": null}})),
            )
            .await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &write(
                &app,
                "/api/model/sentences/req:a/edit",
                Some(json!({"slots": {"class": "bogus", "level": null, "modality": null}})),
            )
            .await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &write(
                &app,
                "/api/model/sentences/req:a/edit",
                Some(json!({"slots": {"class": "functional", "level": null, "modality": null}})),
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_NO_CHANGE",
        );
        let r = write(
            &app,
            "/api/model/sentences/req:a/edit",
            Some(json!({"slots": {"class": null, "level": null, "modality": "should"}})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let sentence = get(&app, "/api/model/sentences?status=Accepted&limit=1")
            .await
            .json();
        assert_eq!(sentence["items"][0]["slots"]["modality"], "should");
        assert_eq!(
            sentence["items"][0]["text"],
            "Employees shall submit leave requests."
        );
        let r = write(&app, "/api/model/sentences/req:p/accept", None).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(
            head(&app).graph.node(&id("req:p")).unwrap().status,
            ElementStatus::Accepted
        );
        assert_error(
            &write(&app, "/api/model/sentences/req:p/reject", None).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_SEMANTIC_INVARIANT",
        );
        assert_error(
            &write(&app, "/api/model/sentences/op:submit/reject", None).await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
    }

    #[tokio::test]
    async fn http_workflow_crud_reports_reads_and_unknown_writes() {
        let app = app();
        let r = get(&app, "/api/model/crud").await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let m = r.json();
        assert_eq!(
            m["operations"],
            json!(["op:approve", "op:cancel", "op:submit"])
        );
        assert_eq!(m["entities"], json!(["entity:request"]));
        assert_eq!(
            m["cells"],
            json!([
                {"op": "op:approve", "ent": "entity:request", "c": null, "r": false, "u": null, "d": null},
                {"op": "op:cancel", "ent": "entity:request", "c": false, "r": false, "u": false, "d": false},
                {"op": "op:submit", "ent": "entity:request", "c": false, "r": true, "u": false, "d": false}
            ])
        );
    }

    // ------------------------------------------------------------------ gate reports

    fn f1_report(graph: &Graph) -> plumb_validation::GateReport {
        use plumb_validation::{
            rules, EvaluatorRegistry, F1RequirementValidationInput, F1ValidationInputs,
            ValidationContext, ValidationPolicy, ValidationRegistry,
        };
        let metadata = ValidationRegistry::new(profile()).unwrap();
        let mut registry = EvaluatorRegistry::new(metadata.clone());
        rules::register_f1_evaluators(&mut registry).unwrap();
        let requirements = graph
            .nodes()
            .values()
            .filter(|n| n.status == ElementStatus::Accepted)
            .filter_map(|n| match &n.payload {
                NodePayload::Requirement(r) => Some((n, r)),
                _ => None,
            })
            .map(|(n, r)| F1RequirementValidationInput {
                requirement_ref: n.id.clone(),
                vocabulary_dependencies: vec![],
                lint_input: serde_json::from_value(json!({
                    "requirement_ref": n.id, "statement": r.statement,
                    "evidence_refs": [], "source_anchor": null, "term_context": null
                }))
                .unwrap(),
            })
            .collect();
        let ctx = ValidationContext::new(
            graph,
            &metadata,
            ValidationPolicy::default(),
            graph.evidence_hash().unwrap(),
            vec![],
            vec![],
        )
        .unwrap()
        .with_f1_inputs(
            graph,
            F1ValidationInputs::new(requirements, vec![], Default::default()),
        )
        .unwrap();
        let report = registry
            .evaluate_gate(plumb_core::GateId::F1, graph, &ctx)
            .unwrap();
        report.validate().unwrap();
        report
    }

    /// The fixture plus the Accepted Finding nodes of `report`; findings are excluded from the
    /// semantic hash, so the report stays current for the head.
    fn with_findings(graph: &Graph, report: &plumb_validation::GateReport) -> Graph {
        let mut nodes: Vec<Node> = graph.nodes().values().cloned().collect();
        for f in &report.findings {
            let mut n = node(
                f.id.as_str(),
                "Accepted",
                "Finding",
                json!({
                    "code": "x", "family": "x", "severity": "info", "message": "x", "status": "Open",
                    "affected_refs": [], "standard_rule_ref": null, "suggested_resolution": null,
                    "waiver_ref": null
                }),
            );
            n.payload = NodePayload::Finding(f.payload.clone());
            nodes.push(n);
        }
        Graph::new(
            id(PROJECT),
            id(PROFILE),
            nodes,
            graph.edges().values().cloned().collect(),
        )
        .unwrap()
    }

    const CRITERIA: &str = "fnd:6a35cd766c904bcb";

    fn waiver_app() -> App {
        let g = fixture_graph();
        let report = f1_report(&g);
        let g = with_findings(&g, &report);
        app_with(g, |head| vec![f1_report_for(head, &report)])
    }

    /// `report` itself, checked to still be current for `head`.
    fn f1_report_for(
        head: &Graph,
        report: &plumb_validation::GateReport,
    ) -> plumb_validation::GateReport {
        assert_eq!(report.baseline_semantic_hash, head.semantic_hash().unwrap());
        assert_eq!(report.baseline_evidence_hash, head.evidence_hash().unwrap());
        report.clone()
    }

    /// R0.4: requirement list and detail never require a current F1 report; `lint_current`
    /// states whether the lint projection is current, so an unavailable lint is never read as
    /// a clean one.
    #[tokio::test]
    async fn requirements_read_without_current_f1_report() {
        let g = fixture_graph();
        let report = f1_report(&g);
        // The requirement the current report has lint findings for.
        let affected = report
            .findings
            .iter()
            .flat_map(|f| f.payload.affected_refs.iter())
            .find(|r| {
                matches!(
                    g.node(r).map(|n| &n.payload),
                    Some(NodePayload::Requirement(_))
                )
            })
            .cloned()
            .expect("the fixture F1 report has a requirement finding");
        let mut expected: Vec<String> = report
            .findings
            .iter()
            .filter(|f| f.payload.affected_refs.contains(&affected))
            .map(|f| f.id.to_string())
            .collect();
        expected.sort();
        let detail_uri = format!("/api/requirements/{affected}");

        // Without a current F1 report: 200, lint_current false, lint unavailable.
        let g_findings = with_findings(&g, &report);
        let bare = app_with(g_findings.clone(), |_| Vec::new());
        let list = get(&bare, "/api/requirements").await;
        assert_eq!(list.status, StatusCode::OK);
        let list = list.json();
        let items = list["items"].as_array().unwrap();
        assert!(!items.is_empty());
        for item in items {
            assert_eq!(item["lint_current"], json!(false), "{item}");
            assert_eq!(item["smells"], json!([]), "{item}");
            assert_eq!(item["readiness"], Value::Null, "{item}");
        }
        let detail = get(&bare, &detail_uri).await;
        assert_eq!(detail.status, StatusCode::OK);
        let detail = detail.json();
        assert_eq!(detail["lint_current"], json!(false));
        assert_eq!(detail["smells"], json!([]));
        assert_eq!(detail["lint_findings"], json!([]));
        assert_eq!(detail["readiness"], Value::Null);

        // With a current F1 report over the same head: lint_current true and the normal
        // projection of that report is restored.
        let current = app_with(g_findings, |head| vec![f1_report_for(head, &report)]);
        let list = get(&current, "/api/requirements").await.json();
        for item in list["items"].as_array().unwrap() {
            assert_eq!(item["lint_current"], json!(true), "{item}");
        }
        let detail = get(&current, &detail_uri).await.json();
        assert_eq!(detail["lint_current"], json!(true));
        assert_eq!(detail["lint_findings"], json!(expected));
        assert!(!expected.is_empty());
    }

    #[tokio::test]
    async fn http_workflow_waivers_come_from_current_reports() {
        let app = waiver_app();
        assert_error(
            &write(
                &app,
                "/api/findings/fnd:0000000000000000/waive",
                Some(json!({"reason": "Accepted risk."})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_FINDING_NOT_CURRENT",
        );
        assert_error(
            &write(
                &app,
                "/api/requirements/req:a/waive-finding",
                Some(json!({"finding_id": CRITERIA, "reason": "Accepted risk."})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_FINDING_NOT_FOR_REQUIREMENT",
        );
        assert_error(
            &write(
                &app,
                "/api/findings/fnd:6a35cd766c904bcb/waive",
                Some(json!({"reason": ""})),
            )
            .await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        let r = write(
            &app,
            "/api/requirements/req:b/waive-finding",
            Some(json!({"finding_id": CRITERIA, "reason": "Accepted risk."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let loaded = head(&app);
        let waiver: Vec<(Id, plumb_psg::ResolutionDecision)> = loaded
            .graph
            .nodes()
            .values()
            .filter_map(|n| match &n.payload {
                NodePayload::ResolutionDecision(d) if d.answer["kind"] == "waiver" => {
                    Some((n.id.clone(), d.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(waiver.len(), 1);
        let (waiver_ref, decision) = &waiver[0];
        assert_eq!(decision.question_ref, None);
        assert!(decision
            .proposal_ref
            .as_ref()
            .unwrap()
            .as_str()
            .starts_with("waiver-subject:"));
        assert_eq!(decision.rationale.as_deref(), Some("Accepted risk."));
        // Its artifact was persisted under its patch_ref before the CAS.
        let artifacts = SqliteArtifactStore::open(&app.path).unwrap();
        assert_eq!(
            artifacts.get(&decision.patch_ref).unwrap().kind,
            ArtifactKind::Patch
        );
        let companions = acceptance_decisions(&loaded.graph);
        assert_eq!(companions.len(), 1);
        let mut refs = vec![waiver_ref.clone(), companions[0].0.clone()];
        refs.sort();
        assert_eq!(loaded.revision.decision_refs, refs);
        // A stale report (the head moved) is never used.
        let stale = write(
            &app,
            "/api/findings/fnd:527fa18e68670baa/waive",
            Some(json!({"reason": "Accepted risk."})),
        )
        .await;
        assert_error(&stale, StatusCode::CONFLICT, "E_FINDING_NOT_CURRENT");
    }

    #[tokio::test]
    async fn http_workflow_waive_finding_and_material_mismatch() {
        let g = fixture_graph();
        let report = f1_report(&g);
        // The persisted finding of LINEAGE carries other affected refs than the report.
        let mut tampered = report.clone();
        tampered.findings.clear();
        let mut graph = with_findings(&g, &report);
        let mut nodes: Vec<Node> = graph.nodes().values().cloned().collect();
        for n in nodes
            .iter_mut()
            .filter(|n| n.id == id("fnd:527fa18e68670baa"))
        {
            if let NodePayload::Finding(f) = &mut n.payload {
                f.affected_refs = vec![id("req:a")];
            }
        }
        graph = Graph::new(
            id(PROJECT),
            id(PROFILE),
            nodes,
            graph.edges().values().cloned().collect(),
        )
        .unwrap();
        let app = app_with(graph, |head| vec![f1_report_for(head, &report)]);
        assert_error(
            &write(
                &app,
                "/api/findings/fnd:527fa18e68670baa/waive",
                Some(json!({"reason": "Accepted risk."})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_FINDING_MATERIAL_MISMATCH",
        );
        let r = write(
            &app,
            &format!("/api/findings/{CRITERIA}/waive"),
            Some(json!({"reason": "Accepted risk."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let _ = tampered;
    }

    // ------------------------------------------------------------------ calculations

    fn encode(text: &str) -> String {
        text.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }

    async fn example(app: &App, calc: &str, inputs: &str) -> Reply {
        get(
            app,
            &format!(
                "/api/model/calculations/{calc}/example?inputs={}",
                encode(inputs)
            ),
        )
        .await
    }

    #[tokio::test]
    async fn http_workflow_calculation_examples_are_exact() {
        let app = app();
        let r = example(
            &app,
            "calc:double",
            r#"{"days":{"type":"integer","value":3}}"#,
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(
            r.json(),
            json!({
                "expr": "days * 2",
                "rendered_example": "Given days = 3; days * 2 evaluates to 6.",
                "value": {"type": "integer", "value": 6}
            })
        );
        assert_error(
            &example(&app, "calc:legacy", "{}").await,
            StatusCode::CONFLICT,
            "E_CALCULATION_SCOPE_UNAVAILABLE",
        );
        assert_error(
            &example(&app, "calc:ghost", "{}").await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        assert_error(
            &example(&app, "attr:days", "{}").await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        for bad in [
            "[1]",
            r#"{"days":{"type":"integer","value":3},"days":{"type":"integer","value":4}}"#,
            r#"{"nobody":{"type":"integer","value":3}}"#,
            r#"{"days":{"type":"float","value":3}}"#,
            r#"{"days":{"type":"decimal","value":"01.5"}}"#,
        ] {
            assert_error(
                &example(&app, "calc:double", bad).await,
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            );
        }
        assert_error(
            &example(&app, "calc:double", "{}").await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_CALCULATION_EVALUATION",
        );
        assert_error(
            &example(
                &app,
                "calc:double",
                r#"{"days":{"type":"boolean","value":true}}"#,
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_CALCULATION_EVALUATION",
        );
    }

    // ------------------------------------------------------------------ rules

    #[tokio::test]
    async fn http_workflow_rules_project_and_edit_one_cell() {
        let app = app();
        // A malformed persisted table is a typed conflict, never skipped.
        assert_error(
            &get(&app, "/api/model/rules").await,
            StatusCode::CONFLICT,
            "E_DECISION_TABLE_ENCODING_INVALID",
        );
        assert_error(
            &get(&app, "/api/model/rules/dt:broken").await,
            StatusCode::CONFLICT,
            "E_DECISION_TABLE_ENCODING_INVALID",
        );
        let scalar = get(&app, "/api/model/rules/rule:positive").await.json();
        assert_eq!(
            scalar,
            json!({"id": "rule:positive", "name": "Positive days", "kind": "rule", "conditions": ["days > 0"], "rows": null, "uncovered_cells": null, "overlaps": null})
        );
        let table = get(&app, "/api/model/rules/dt:eligibility").await.json();
        assert_eq!(
            table,
            json!({
                "id": "dt:eligibility", "name": "Eligibility", "kind": "decision_table",
                "conditions": [r#"{"domain":null,"name":"eligible","ty":{"type":"bool"}}"#],
                "rows": [
                    {"when": [r#"{"cell":"bool","value":true}"#], "then": [r#"{"cell":"enum","member":"Manager"}"#]},
                    {"when": [r#"{"cell":"bool","value":false}"#], "then": [r#"{"cell":"enum","member":"Director"}"#]}
                ],
                "uncovered_cells": [],
                "overlaps": []
            })
        );
        assert_error(
            &get(&app, "/api/model/rules/op:submit").await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        let cell = |cell: &str, outcome: &str| json!({"cell": cell, "outcome": outcome, "reason": "Policy change."});
        let director = r#"{"cell":"enum","member":"Director"}"#;
        for (body, status, code) in [
            (
                cell("row:0:output:1", director),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                cell("row:00:output:0", director),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                cell("row:0:output:0", r#"{"member":"Director","cell":"enum"}"#),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                cell("row:0:output:0", r#"{"cell":"enum","member":"Nobody"}"#),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_SEMANTIC_INVARIANT",
            ),
            (
                cell("row:1:output:0", director),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_NO_CHANGE",
            ),
            (
                json!({"cell": "row:0:output:0", "outcome": director, "reason": ""}),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
        ] {
            assert_error(
                &write(&app, "/api/model/rules/dt:eligibility/cells", Some(body)).await,
                status,
                code,
            );
        }
        assert_error(
            &write(
                &app,
                "/api/model/rules/rule:positive/cells",
                Some(cell("row:0:output:0", director)),
            )
            .await,
            StatusCode::CONFLICT,
            "E_RULE_NOT_DECISION_TABLE",
        );
        assert_error(
            &write(
                &app,
                "/api/model/rules/dt:broken/cells",
                Some(cell("row:0:output:0", director)),
            )
            .await,
            StatusCode::CONFLICT,
            "E_DECISION_TABLE_ENCODING_INVALID",
        );
        let r = write(
            &app,
            "/api/model/rules/dt:eligibility/cells",
            Some(cell("row:0:output:0", director)),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let table = get(&app, "/api/model/rules/dt:eligibility").await.json();
        assert_eq!(table["rows"][0]["then"], json!([director]));
        assert_eq!(
            table["rows"][0]["when"],
            json!([r#"{"cell":"bool","value":true}"#])
        );
        assert_eq!(acceptance_decisions(&head(&app).graph).len(), 1);
    }

    // ------------------------------------------------------------------ processes

    fn mermaid(name: &str, operations: &[&str]) -> String {
        let mut text = format!(
            "flowchart TD\n%% plumb-process-name: {}\n  start((Start))\n",
            serde_json::to_string(name).unwrap()
        );
        let keys: Vec<String> = (0..operations.len())
            .map(|i| format!("step_{i:04}"))
            .collect();
        for (k, op) in keys.iter().zip(operations) {
            text.push_str(&format!("  {k}[\"{op}\"]\n"));
        }
        text.push_str("  end((End))\n");
        let mut order = vec!["start".to_owned()];
        order.extend(keys);
        order.push("end".to_owned());
        for pair in order.windows(2) {
            text.push_str(&format!("  {} --> {}\n", pair[0], pair[1]));
        }
        text
    }

    #[tokio::test]
    async fn http_workflow_linear_processes_and_canonical_mermaid() {
        let app = app();
        let text = mermaid("Leave Flow", &["op:submit", "op:approve"]);
        let import = |t: String| json!({"format": "mermaid", "text": t});
        for bad in [
            text.replace("  start((Start))", "start((Start))"),
            text.trim_end().to_owned(),
            text.replace("flowchart TD", "flowchart LR"),
            format!("{text}%% extra\n"),
        ] {
            assert_error(
                &write(&app, "/api/processes/import", Some(import(bad))).await,
                StatusCode::BAD_REQUEST,
                "E_MERMAID_INVALID",
            );
        }
        assert_error(
            &write(
                &app,
                "/api/processes/import",
                Some(import(mermaid("Leave Flow", &["op:ghost"]))),
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_SEMANTIC_INVARIANT",
        );
        // CRLF is normalized before the byte-exact canonical check.
        let r = write(
            &app,
            "/api/processes/import",
            Some(import(text.replace('\n', "\r\n"))),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let process = plumb_functional::process_id(&id(PROJECT), "Leave Flow").unwrap();
        let exported = get(
            &app,
            &format!("/api/processes/{process}/export?format=mermaid"),
        )
        .await;
        assert_eq!(exported.status, StatusCode::OK);
        assert_eq!(exported.text(), text);
        assert!(exported.headers.get("etag").is_some());
        assert_error(
            &write(&app, "/api/processes/import", Some(import(text.clone()))).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_NO_CHANGE",
        );
        assert_error(
            &write(
                &app,
                "/api/processes/import",
                Some(import(mermaid("Leave Flow", &["op:submit"]))),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROCESS_IMPORT_CONFLICT",
        );
        // Step editing rewires only the flow and keeps the canonical form.
        let steps = format!("/api/processes/{process}/steps");
        let r = write(
            &app,
            &steps,
            Some(json!({"op": "add", "operation_ref": "op:cancel", "position": 1})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let after = get(
            &app,
            &format!("/api/processes/{process}/export?format=mermaid"),
        )
        .await
        .text();
        assert_eq!(
            after,
            mermaid("Leave Flow", &["op:submit", "op:cancel", "op:approve"])
        );
        let interior = plumb_functional::linear_process(&head(&app).graph, &process)
            .unwrap()
            .interior;
        let r = write(
            &app,
            &steps,
            Some(json!({"op": "move", "node_ref": interior[0].as_str(), "position": 2})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(
            get(
                &app,
                &format!("/api/processes/{process}/export?format=mermaid")
            )
            .await
            .text(),
            mermaid("Leave Flow", &["op:cancel", "op:approve", "op:submit"])
        );
        let r = write(
            &app,
            &steps,
            Some(json!({"op": "remove", "node_ref": interior[1].as_str()})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(
            get(
                &app,
                &format!("/api/processes/{process}/export?format=mermaid")
            )
            .await
            .text(),
            mermaid("Leave Flow", &["op:approve", "op:submit"])
        );
        for (body, status, code) in [
            (
                json!({"op": "add", "operation_ref": "op:cancel", "position": 9}),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                json!({"op": "add", "operation_ref": "op:ghost", "position": 0}),
                StatusCode::NOT_FOUND,
                "E_NOT_FOUND",
            ),
            (
                json!({"op": "move", "node_ref": interior[0].as_str(), "position": 1}),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_NO_CHANGE",
            ),
            (
                json!({"op": "swap"}),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
        ] {
            assert_error(&write(&app, &steps, Some(body)).await, status, code);
        }
        assert_error(
            &write(
                &app,
                "/api/processes/proc:branched/steps",
                Some(json!({"op": "remove", "node_ref": "pn:b-gate"})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROCESS_NOT_LINEAR",
        );
        assert_error(
            &get(&app, "/api/processes/proc:branched/export?format=mermaid").await,
            StatusCode::CONFLICT,
            "E_MERMAID_UNREPRESENTABLE",
        );
        assert_error(
            &get(&app, "/api/processes/proc:branched/export").await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
    }

    /// Active performed_by targets of `node` on the head, sorted.
    fn performers_of(app: &App, node: &Id) -> Vec<String> {
        let g = head(app).graph;
        let mut out: Vec<String> = g
            .outgoing_edge_ids(node)
            .iter()
            .filter_map(|e| g.edge(e))
            .filter(|e| {
                e.kind == plumb_psg::RelationKind::PerformedBy
                    && matches!(e.status, ElementStatus::Accepted | ElementStatus::Proposed)
            })
            .map(|e| e.to.to_string())
            .collect();
        out.sort();
        out
    }

    #[tokio::test]
    async fn http_workflow_move_lane_changes_only_the_performer() {
        let app = app();
        let text = mermaid("Leave Flow", &["op:submit", "op:approve"]);
        let r = write(
            &app,
            "/api/processes/import",
            Some(json!({"format": "mermaid", "text": text})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let process = plumb_functional::process_id(&id(PROJECT), "Leave Flow").unwrap();
        let linear = plumb_functional::linear_process(&head(&app).graph, &process).unwrap();
        let (task, other_task) = (linear.interior[0].clone(), linear.interior[1].clone());
        // A BusinessRole, an unrelated relation of the task and a branch (non-linear flow).
        direct_commit(
            &app,
            SemanticPatch::Compound {
                patches: vec![
                    SemanticPatch::AddNode {
                        node: node(
                            "brole:approver",
                            "Accepted",
                            "BusinessRole",
                            json!({"name": "Approver"}),
                        ),
                    },
                    SemanticPatch::AddEdge {
                        edge: edge(
                            "rel:lane-perf",
                            "performed_by",
                            task.as_str(),
                            "actor:clerk",
                        ),
                    },
                    SemanticPatch::AddNode {
                        node: node(
                            "outcome:approved",
                            "Accepted",
                            "Outcome",
                            json!({"name": "Approved", "outcome_kind": "success"}),
                        ),
                    },
                    SemanticPatch::AddEdge {
                        edge: edge(
                            "rel:lane-spec",
                            "produces",
                            task.as_str(),
                            "outcome:approved",
                        ),
                    },
                    SemanticPatch::AddEdge {
                        edge: edge(
                            "rel:lane-branch",
                            "next",
                            linear.start.as_str(),
                            other_task.as_str(),
                        ),
                    },
                ],
            },
        );
        assert!(plumb_functional::linear_process(&head(&app).graph, &process).is_err());
        let steps = format!("/api/processes/{process}/steps");
        let lane = |performer: &str| json!({"op": "move_lane", "node_ref": task.as_str(), "performer_ref": performer, "reason": null});
        // If-Match is required.
        assert_error(
            &post(&app, &steps, None, Some(lane("brole:approver"))).await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &post(
                &app,
                &steps,
                Some("psg:sha256:stale"),
                Some(lane("brole:approver")),
            )
            .await,
            StatusCode::PRECONDITION_FAILED,
            "E_PRECONDITION_FAILED",
        );
        // Linear step changes refuse the branched process; a valid lane move does not.
        assert_error(
            &write(
                &app,
                &steps,
                Some(json!({"op": "move", "node_ref": task.as_str(), "position": 1})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROCESS_NOT_LINEAR",
        );
        let before = head(&app).graph;
        let flows = |g: &plumb_psg::Graph| -> Vec<(String, String, String)> {
            let mut v: Vec<(String, String, String)> = g
                .edges()
                .values()
                .filter(|e| e.kind == plumb_psg::RelationKind::Next)
                .map(|e| (e.id.to_string(), e.from.to_string(), e.to.to_string()))
                .collect();
            v.sort();
            v
        };
        let r = write(&app, &steps, Some(lane("brole:approver"))).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let after = head(&app).graph;
        assert_eq!(
            performers_of(&app, &task),
            vec!["brole:approver".to_owned()]
        );
        assert_eq!(flows(&after), flows(&before));
        assert!(after.edge(&id("rel:lane-spec")).is_some());
        assert_eq!(
            after.node(&task).unwrap().payload,
            before.node(&task).unwrap().payload
        );
        // The projection reports the new lane; the same performer is no change.
        let projected = get(&app, &format!("/api/processes/{process}")).await.json();
        let view = projected["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == task.as_str())
            .unwrap()
            .clone();
        assert_eq!(view["performer_refs"], json!(["brole:approver"]));
        assert_error(
            &write(&app, &steps, Some(lane("brole:approver"))).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_NO_CHANGE",
        );
        // Invalid targets and performers commit nothing.
        let revision_before = revision(&app).await;
        for (body, status, code) in [
            (
                json!({"op": "move_lane", "node_ref": linear.start.as_str(), "performer_ref": "actor:clerk"}),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_SEMANTIC_INVARIANT",
            ),
            (
                json!({"op": "move_lane", "node_ref": "process_node:ghost", "performer_ref": "actor:clerk"}),
                StatusCode::NOT_FOUND,
                "E_NOT_FOUND",
            ),
            (
                json!({"op": "move_lane", "node_ref": task.as_str(), "performer_ref": "stakeholder:hr"}),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_SEMANTIC_INVARIANT",
            ),
            (
                json!({"op": "move_lane", "node_ref": task.as_str(), "performer_ref": "op:submit"}),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_SEMANTIC_INVARIANT",
            ),
            (
                json!({"op": "move_lane", "node_ref": task.as_str(), "performer_ref": "actor:ghost"}),
                StatusCode::NOT_FOUND,
                "E_NOT_FOUND",
            ),
            (
                json!({"op": "movelane", "node_ref": task.as_str(), "performer_ref": "actor:clerk"}),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            // The reason is read and must be clean text when present.
            (
                json!({"op": "move_lane", "node_ref": task.as_str(), "performer_ref": "actor:clerk", "reason": " padded"}),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
        ] {
            assert_error(&write(&app, &steps, Some(body)).await, status, code);
        }
        assert_eq!(revision(&app).await, revision_before);
        // A clean reason goes through the ordinary shared write path.
        let r = write(
            &app,
            &steps,
            Some(json!({"op": "move_lane", "node_ref": task.as_str(), "performer_ref": "actor:clerk", "reason": "Clerks approve now."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(performers_of(&app, &task), vec!["actor:clerk".to_owned()]);
    }

    // ------------------------------------------------------------------ assumptions

    #[tokio::test]
    async fn http_workflow_assumption_resolution_changes_only_its_status() {
        let app = app();
        assert_error(
            &write(
                &app,
                "/api/assumptions/assumption:active/resolve",
                Some(json!({"reason": " x"})),
            )
            .await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &write(
                &app,
                "/api/assumptions/assumption:open/resolve",
                Some(json!({"reason": "Done."})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_ASSUMPTION_NOT_ACTIVE",
        );
        assert_error(
            &write(
                &app,
                "/api/assumptions/req:a/resolve",
                Some(json!({"reason": "Done."})),
            )
            .await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        let before = head(&app)
            .graph
            .node(&id("assumption:active"))
            .unwrap()
            .clone();
        let r = write(
            &app,
            "/api/assumptions/assumption:active/resolve",
            Some(json!({"reason": "Confirmed by HR."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let after = head(&app)
            .graph
            .node(&id("assumption:active"))
            .unwrap()
            .clone();
        assert_eq!(after.status, ElementStatus::Accepted);
        let (NodePayload::Assumption(a), NodePayload::Assumption(b)) =
            (&before.payload, &after.payload)
        else {
            panic!()
        };
        let mut expected = a.clone();
        expected.status = "Resolved".into();
        assert_eq!(b, &expected);
        assert_error(
            &write(
                &app,
                "/api/assumptions/assumption:active/resolve",
                Some(json!({"reason": "Again."})),
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_NO_CHANGE",
        );
    }

    // ------------------------------------------------------------------ questions and answers

    fn performer_questions(graph: &Graph) -> Vec<Id> {
        graph
            .nodes()
            .values()
            .filter(|n| matches!(n.payload, NodePayload::Question(_)))
            .map(|n| n.id.clone())
            .collect()
    }

    #[tokio::test]
    async fn http_workflow_questions_project_persisted_questions() {
        let app = app();
        let questions = performer_questions(&head(&app).graph);
        assert!(!questions.is_empty());
        let r = get(&app, "/api/questions").await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let items = r.json()["items"].as_array().unwrap().clone();
        assert_eq!(items.len(), questions.len());
        let first = &items[0];
        assert_eq!(first["kind"], "RoleAssignment");
        assert_eq!(first["status"], "Open");
        assert_eq!(first["stakeholder_ref"], "stakeholder:hr");
        assert!(first["answer_schema"].is_object());
        assert!(first["priority"].is_u64());
        assert_eq!(first["round_ref"], Value::Null);
        assert!(first["context_refs"].is_array());
        assert_eq!(
            get(&app, "/api/questions?stakeholder=stakeholder:architect")
                .await
                .json()["items"],
            json!([])
        );
        assert_eq!(
            get(
                &app,
                "/api/questions?status=Open&stakeholder=stakeholder:hr"
            )
            .await
            .json()["items"]
                .as_array()
                .unwrap()
                .len(),
            items.len()
        );
        assert_error(
            &get(&app, "/api/questions?status=open").await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &get(&app, "/api/questions?round=not%20an%20id").await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
    }

    async fn answer(app: &App, question: &Id, answer: Value) -> Reply {
        write(
            app,
            &format!("/api/questions/{question}/answer"),
            Some(json!({"answer": answer, "rationale": null, "supersedes": null})),
        )
        .await
    }

    #[tokio::test]
    async fn http_workflow_end_to_end_answer_preview_accept_and_stale_if_match() {
        let app = app();
        let graph = head(&app).graph;
        let question = question_about(&graph, "op:approve");
        // The wire shape is enforced before the domain sees the answer.
        for bad in [json!("actor:clerk"), json!(["actor:clerk"]), json!(1)] {
            assert_error(
                &answer(&app, &question, bad).await,
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            );
        }
        assert_error(
            &answer(&app, &id("q:0000000000000000"), json!({})).await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        assert_error(
            &answer(&app, &question, json!({"performer_ref": "actor:ghost"})).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_SEMANTIC_INVARIANT",
        );
        let before = revision(&app).await;
        let tag = semantic(&app).await;
        let r = answer(&app, &question, json!({"performer_ref": "actor:clerk"})).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let body = r.json();
        // Nothing is committed: same revision, no event, a pending proposal and a preview.
        assert_eq!(body["revision"], before);
        assert_eq!(body["events"], json!([]));
        let proposal_id = body["proposal_id"].as_str().unwrap().to_owned();
        assert_eq!(body["patch_preview"]["op"], "Compound");
        assert!(body["intake"]["reason_codes"]
            .as_array()
            .unwrap()
            .contains(&json!("human_decision")));
        let stored = workflow(&app).proposal(&id(&proposal_id)).unwrap().unwrap();
        assert_eq!(stored.state, WorkflowProposalState::Pending);
        assert_eq!(stored.origin_kind, WorkflowOrigin::Question);
        assert_eq!(stored.origin_ref, Some(question.clone()));
        assert_eq!(stored.base_revision_id.as_str(), before);
        // Hotfix 076: the response carries the authoritative class of exactly this stored
        // Proposal (a performer relation is a create action), and describes no other Proposal.
        assert_eq!(body["action"], "create");
        assert_eq!(stored.proposal_kind, WorkflowProposalKind::Create);
        let persisted: Proposal = serde_json::from_str(&stored.proposal_json).unwrap();
        assert_eq!(persisted.id.as_str(), proposal_id);
        assert_eq!(
            body["patch_preview"],
            serde_json::to_value(&persisted.patch_set.patch).unwrap()
        );
        assert_eq!(persisted.patch_set.base_semantic_hash.to_string(), tag);
        // The proposal adds a performer relation, so it is a create action.
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{proposal_id}/accept"),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_ACTION_MISMATCH",
        );
        // Accept: the revision advances with the S3.3 decision and the companion.
        let r = write(
            &app,
            &format!("/api/proposals/{proposal_id}/create"),
            Some(json!({"reason": "HR confirmed."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let accepted_revision = r.json()["revision"].as_str().unwrap().to_owned();
        assert_ne!(accepted_revision, before);
        let loaded = head(&app);
        assert!(loaded
            .graph
            .edges()
            .values()
            .any(|e| e.kind == plumb_psg::RelationKind::PerformedBy
                && e.from == id("op:approve")
                && e.to == id("actor:clerk")));
        let companions = acceptance_decisions(&loaded.graph);
        assert_eq!(companions.len(), 1);
        assert_eq!(companions[0].1.proposal_ref, Some(id(&proposal_id)));
        assert_eq!(companions[0].1.rationale.as_deref(), Some("HR confirmed."));
        assert_eq!(loaded.revision.decision_refs.len(), 2);
        let stored = workflow(&app).proposal(&id(&proposal_id)).unwrap().unwrap();
        assert_eq!(stored.state, WorkflowProposalState::Accepted);
        assert_eq!(
            stored.resolved_revision_id.unwrap().as_str(),
            accepted_revision
        );
        // The old If-Match is now stale: 412 with the current head, nothing retried.
        let stale = post(
            &app,
            &format!("/api/proposals/{proposal_id}/create"),
            Some(&tag),
            Some(json!({"reason": null})),
        )
        .await;
        assert_error(
            &stale,
            StatusCode::PRECONDITION_FAILED,
            "E_PRECONDITION_FAILED",
        );
        assert_eq!(stale.json()["revision"], accepted_revision);
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{proposal_id}/create"),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_NOT_PENDING",
        );
    }

    /// A complete typed proposal on the current head adding one Proposed requirement.
    async fn requirement_proposal(app: &App, node_id: &str) -> Proposal {
        Proposal::new(
            plumb_core::StageId::S1,
            PatchSet {
                base_semantic_hash: semantic(app).await.parse().unwrap(),
                patch: SemanticPatch::AddNode {
                    node: requirement(node_id, "Proposed", "Payroll shall export leave totals."),
                },
            },
            Vec::new(),
            Vec::new(),
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .unwrap()
    }

    async fn intake(app: &App, kind: &str, proposal: &Proposal) -> Reply {
        write(
            app,
            "/api/proposals/intake",
            Some(json!({"kind": kind, "payload": proposal, "evidence": []})),
        )
        .await
    }

    // ------------------------------------------------------------------ Hotfix 083

    const STAGE_ACTOR: &str = "agent:stage";
    const FRACTION_CALC: &str = "calc:requested";

    /// A generic pre-answer graph: one Calculation over a working-day period with an unresolved
    /// inclusive end, no calendar and a fractional Decimal input bound by its S2.6 origin.
    fn calculation_answer_graph() -> Graph {
        let attribute = |id: &str, value_type: &str| {
            node(
                id,
                "Accepted",
                "Attribute",
                json!({"name": id, "value_type": value_type, "nullable": false, "unit": null,
                       "precision": null, "enum_values": null, "data_classification": null}),
            )
        };
        let mut calc = node(
            FRACTION_CALC,
            "Accepted",
            "Calculation",
            json!({
                "name": "requested_working_days",
                "expression": "working_days([begin, finish], calendar, inclusive_end) * part",
                "result_type": "Decimal(2)", "unit": "working_day", "rounding": null,
                "calendar_ref": null, "examples": null
            }),
        );
        let range = json!({"requirement_ref": "req:requested", "start": 0, "end": 4});
        calc.extensions.insert(
            plumb_psg::ExtensionKey::from_str(CALCULATION_ORIGIN_EXTENSION).unwrap(),
            json!({
                "name_range": range, "expression_evidence": [],
                "used_bindings": [
                    {"node_ref": "attr:begin", "symbol": "begin", "exposure": {"kind": "root"}},
                    {"node_ref": "attr:finish", "symbol": "finish", "exposure": {"kind": "root"}},
                    {"node_ref": "attr:flag", "symbol": "inclusive_end", "exposure": {"kind": "root"}},
                    {"node_ref": "attr:part", "symbol": "part", "exposure": {"kind": "root"}}
                ]
            }),
        );
        let nodes = vec![
            agent(ACTOR, "human"),
            agent(ASSISTANT, "llm_model"),
            agent(STAGE_ACTOR, "compiler_stage"),
            stakeholder("stakeholder:owner"),
            node(
                "entity:period",
                "Accepted",
                "Entity",
                json!({"name": "Period", "description": null, "aggregate_root": null}),
            ),
            attribute("attr:begin", "Date"),
            attribute("attr:finish", "Date"),
            attribute("attr:flag", "Bool"),
            attribute("attr:part", "Decimal(2)"),
            node(
                "cal:test",
                "Accepted",
                "Calendar",
                json!({"time_zone": "UTC", "week_pattern": ["monday", "friday"],
                       "region": null, "holiday_source": null}),
            ),
            calc,
        ];
        let edges = ["attr:begin", "attr:finish", "attr:flag", "attr:part"]
            .iter()
            .enumerate()
            .map(|(i, attr)| {
                edge(
                    &format!("rel:period{i}"),
                    "has_attribute",
                    "entity:period",
                    attr,
                )
            })
            .collect();
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    /// The full A0.3 + A0.4 application over a database initialized with `graph`.
    fn application_app(graph: Graph) -> App {
        application_app_at(graph, NOW)
    }

    /// [`application_app`] whose injected FixedClock reads `at` (R0.5 two-clock replay).
    fn application_app_at(graph: Graph, at: &str) -> App {
        application_app_with(graph, at, None)
    }

    /// [`application_app_at`] whose inference runs on `inference` in the replay role instead of
    /// the null provider (R0.6 consolidate fixtures).
    fn application_app_with(
        graph: Graph,
        at: &str,
        inference: Option<Arc<dyn InferenceProvider + Send + Sync>>,
    ) -> App {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plumb.sqlite");
        let profile = profile();
        SqliteRevisionStore::open(&path)
            .unwrap()
            .create_initial_revision(
                &graph,
                profile.profile_hash().unwrap(),
                profile.rule_pack_hash().unwrap(),
                RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id("agent:other"),
                    created_at: ts(T0),
                },
            )
            .unwrap();
        let provider = Arc::new(TestProvider {
            mode: Mutex::new(ProviderMode::Reply),
            base: Mutex::new(None),
            race: Mutex::new(None),
            received: Mutex::new(Vec::new()),
            probe: Mutex::new(None),
            probed: Mutex::new(None),
        });
        let runtime = plumb_api::application::ApplicationRuntime {
            inference_provider: inference
                .clone()
                .unwrap_or_else(|| provider.clone() as Arc<dyn InferenceProvider + Send + Sync>),
            provider_mode: match &inference {
                None => plumb_api::ProviderMode::Null,
                Some(_) => plumb_api::ProviderMode::Replay {
                    fixtures_hash: Hash::content_sha256(b"http_workflow consolidate provider"),
                },
            },
            provider_policy: ProviderPolicy {
                provider: "test".into(),
                config: CanonicalJson::new(json!({})),
            },
            session_turn_config: SessionTurnConfig {
                retrieval_top_k: 3,
                max_open_questions: 3,
                max_context_bytes: 65536,
            },
            assistant_actor_ref: id(ASSISTANT),
            stage_actor_ref: id(STAGE_ACTOR),
            calendars: Arc::new(
                ExprStaticCalendarProvider::new([ExprCalendarDefinition::new(
                    id("cal:test"),
                    [],
                    [],
                )])
                .unwrap(),
            ),
            predicates: Arc::new(NoPredicates),
            calendar_configs: vec![plumb_api::CalendarConfig {
                calendar_ref: id("cal:test"),
                weekend: vec!["saturday".into(), "sunday".into()],
                holidays: vec![],
            }],
            direct_managers: Vec::new(),
            expression_scopes: Vec::new(),
            calculation_scope_overrides: Vec::new(),
            runtime_semantic_values: Vec::new(),
            validation_policy: plumb_validation::ValidationPolicy::default(),
            vocabulary_policy: plumb_functional::VocabularyPolicy {
                language: "en".into(),
            },
            duplicate_policy: serde_json::from_value(json!({"jaccard_threshold": "0.8"})).unwrap(),
            calculation_scope: None,
            operation_scope: None,
            process_scope: None,
            authorization_draft: None,
            scenario_derivation: None,
            verification_applicability: Some(Default::default()),
            gates: plumb_api::GateInputsConfig { f1: None, f4: None },
            directory: None,
            question_routing: None,
        };
        let state = plumb_api::application::open_application(
            &path,
            BranchName::main(),
            identity(ACTOR),
            Arc::new(plumb_core::FixedClock::new(ts(at))),
            profile,
            runtime,
        )
        .unwrap();
        let router = router(state.clone());
        App {
            _dir: dir,
            path,
            state,
            router,
            provider,
        }
    }

    fn calculation_gap(code: &str, condition: &str, affected: &[&str]) -> GeneratedFinding {
        let mut affected: Vec<Id> = affected.iter().map(|a| id(a)).collect();
        affected.sort();
        let key = finding_key(code, &affected, condition).unwrap();
        GeneratedFinding {
            id: finding_id(&key).unwrap(),
            key,
            semantic_condition_key: condition.into(),
            payload: Finding {
                code: code.into(),
                family: "F2".into(),
                severity: FindingSeverity::Blocker,
                message: "The calculation semantic is unresolved.".into(),
                status: "Open".into(),
                affected_refs: affected,
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            },
        }
    }

    /// The three explicit upstream calculation findings of the generic graph.
    fn calculation_gaps() -> Vec<GeneratedFinding> {
        vec![
            calculation_gap(
                "PLUMB.F2.CALC.TYPECHECK",
                &format!(
                    "{}{FRACTION_CALC}",
                    plumb_functional::CALCULATION_FRACTION_CONDITION_PREFIX
                ),
                &[FRACTION_CALC, "attr:part"],
            ),
            calculation_gap(
                "PLUMB.F2.CALC.TYPECHECK",
                &format!(
                    "{}{FRACTION_CALC}",
                    plumb_functional::CALCULATION_INCLUSIVE_END_CONDITION_PREFIX
                ),
                &[FRACTION_CALC],
            ),
            calculation_gap(
                "PLUMB.F2.TIME.CALENDAR_DEFINED",
                &format!(
                    "{}{FRACTION_CALC}",
                    plumb_functional::CALCULATION_HOLIDAY_POLICY_CONDITION_PREFIX
                ),
                &[FRACTION_CALC],
            ),
        ]
    }

    fn owner_routing() -> StakeholderRoutingConfig {
        StakeholderRoutingConfig {
            stakeholders: vec![plumb_functional::RoutingStakeholder {
                id: id("stakeholder:owner"),
                name: "Owner".into(),
                roles: vec!["analyst".into()],
            }],
            routing: [
                ("calendar".to_owned(), vec![id("stakeholder:owner")]),
                ("formula".to_owned(), vec![id("stakeholder:owner")]),
            ]
            .into(),
        }
    }

    fn questions_of(graph: &Graph) -> Vec<(Id, plumb_psg::Question)> {
        graph
            .nodes()
            .values()
            .filter_map(|n| match &n.payload {
                NodePayload::Question(q) => Some((n.id.clone(), q.clone())),
                _ => None,
            })
            .collect()
    }

    fn finding_count(graph: &Graph) -> usize {
        graph
            .nodes()
            .values()
            .filter(|n| matches!(n.payload, NodePayload::Finding(_)))
            .count()
    }

    #[tokio::test]
    async fn http_workflow_question_materialization_service_commits_once() {
        let app = application_app(calculation_answer_graph());
        let before = head(&app);
        let input = QuestionGenerationInput {
            findings: calculation_gaps(),
            routing: owner_routing(),
        };
        let first = app
            .state
            .materialize_questions(&app.state.head().unwrap(), &input)
            .unwrap();
        assert!(first.generation.issues.is_empty());
        assert!(first.generation.unmapped.is_empty());
        assert_eq!(first.generation.questions.len(), 3);
        let write = first.write.clone().unwrap();
        let after = head(&app);
        // One ordinary CAS revision; the governance material leaves the semantic hash alone.
        assert_ne!(after.revision.id, before.revision.id);
        assert_eq!(write.revision, after.revision.id.to_string());
        assert_eq!(after.revision.semantic_hash, before.revision.semantic_hash);
        assert_eq!(finding_count(&after.graph), 3);
        let questions = questions_of(&after.graph);
        assert_eq!(questions.len(), 3);
        let mut templates: Vec<&str> = first
            .generation
            .questions
            .iter()
            .map(|q| q.template_id.as_str())
            .collect();
        templates.sort();
        assert_eq!(
            templates,
            [
                "f2.calculation.fraction.v1",
                "f2.calculation.holiday-policy.v1",
                "f2.calculation.inclusive-end.v1"
            ]
        );
        // The persisted Questions are exactly the kernel's; the audit is the stage actor.
        for generated in &first.generation.questions {
            let persisted = after.graph.node(&generated.question_ref).unwrap();
            assert_eq!(
                persisted.payload,
                NodePayload::Question(generated.payload.clone())
            );
            assert_eq!(persisted.audit.created_by, id(STAGE_ACTOR));
            assert_eq!(
                generated.payload.stakeholder_ref,
                Some(id("stakeholder:owner"))
            );
        }
        // An identical second materialization writes nothing.
        let second = app
            .state
            .materialize_questions(&app.state.head().unwrap(), &input)
            .unwrap();
        assert_eq!(second.write, None);
        assert_eq!(second.generation.proposal, None);
        assert!(second
            .generation
            .questions
            .iter()
            .all(|q| q.materialization == plumb_functional::QuestionMaterialization::Existing));
        let again = head(&app);
        assert_eq!(again.revision.id, after.revision.id);
        assert_eq!(questions_of(&again.graph).len(), 3);
    }

    #[tokio::test]
    async fn http_workflow_question_materialization_fails_whole_on_kernel_issues() {
        let app = application_app(calculation_answer_graph());
        let before = head(&app);
        let mut findings = calculation_gaps();
        // A candidate suffix that is not an ID is a structural kernel issue.
        findings.push(calculation_gap(
            "PLUMB.F2.CALC.TYPECHECK",
            "calculation_fraction_unresolved:not an id",
            &[FRACTION_CALC],
        ));
        let error = app
            .state
            .materialize_questions(
                &app.state.head().unwrap(),
                &QuestionGenerationInput {
                    findings,
                    routing: owner_routing(),
                },
            )
            .unwrap_err();
        assert_eq!(error.body().code, "E_QUESTION_GENERATION_ISSUES");
        let after = head(&app);
        assert_eq!(after.revision.id, before.revision.id);
        assert_eq!(finding_count(&after.graph), 0);
        assert!(questions_of(&after.graph).is_empty());
    }

    #[tokio::test]
    async fn http_workflow_calculation_answers_commit_through_the_governed_path() {
        let app = application_app(calculation_answer_graph());
        let materialized = app
            .state
            .materialize_questions(
                &app.state.head().unwrap(),
                &QuestionGenerationInput {
                    findings: calculation_gaps(),
                    routing: owner_routing(),
                },
            )
            .unwrap();
        let question_of = |template_id: &str| {
            materialized
                .generation
                .questions
                .iter()
                .find(|q| q.template_id == template_id)
                .unwrap()
                .question_ref
                .clone()
        };
        let answers = [
            (
                question_of("f2.calculation.fraction.v1"),
                json!({"decimal": "0.5", "unit": "working_day"}),
            ),
            (
                question_of("f2.calculation.inclusive-end.v1"),
                json!({"inclusive_end": true}),
            ),
            (
                question_of("f2.calculation.holiday-policy.v1"),
                json!({"holiday_consumes_working_day": false, "calendar_ref": "cal:test"}),
            ),
        ];
        let mut decisions = Vec::new();
        for (question, value) in answers {
            let before = revision(&app).await;
            let r = answer(&app, &question, value).await;
            assert_eq!(r.status, StatusCode::OK, "{}", r.text());
            let body = r.json();
            // A pending proposal with intake and preview; nothing committed yet.
            assert_eq!(body["revision"], before);
            assert_eq!(body["events"], json!([]));
            assert_eq!(body["patch_preview"]["op"], "Compound");
            assert!(body["intake"].is_object());
            let proposal_id = body["proposal_id"].as_str().unwrap().to_owned();
            let stored = workflow(&app).proposal(&id(&proposal_id)).unwrap().unwrap();
            assert_eq!(stored.state, WorkflowProposalState::Pending);
            assert_eq!(stored.origin_kind, WorkflowOrigin::Question);
            assert_eq!(revision(&app).await, before);
            // A calculation payload change (or a pure confirmation) is an accept action.
            assert_eq!(body["action"], "accept");
            let r = write(
                &app,
                &format!("/api/proposals/{proposal_id}/accept"),
                Some(json!({"reason": "Confirmed by the owner."})),
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{}", r.text());
            assert_ne!(revision(&app).await, before);
            let stored = workflow(&app).proposal(&id(&proposal_id)).unwrap().unwrap();
            assert_eq!(stored.state, WorkflowProposalState::Accepted);
            let persisted: Proposal = serde_json::from_str(&stored.proposal_json).unwrap();
            decisions.push(persisted);
        }
        let loaded = head(&app);
        let NodePayload::Calculation(calc) =
            &loaded.graph.node(&id(FRACTION_CALC)).unwrap().payload
        else {
            unreachable!()
        };
        // Fraction: one canonical governed example.
        let key = plumb_functional::calculation_fraction_semantic_key(
            &id(FRACTION_CALC),
            &id("attr:part"),
        )
        .unwrap();
        assert_eq!(
            calc.examples,
            Some(vec![json!({"version": 1, "key": key,
                "inputs": [{"symbol": "part", "value": {"kind": "decimal", "value": "0.5", "unit": null}}],
                "expected_result": {"kind": "decimal", "value": "0.5", "unit": "working_day"}})])
        );
        // Inclusive end: the working_days third argument is the boolean literal (the
        // canonical PlumbExpr unparse of the rewritten AST).
        assert_eq!(
            calc.expression,
            "working_days([begin, finish], calendar, true) * part"
        );
        // Holiday policy: the selected calendar.
        assert_eq!(calc.calendar_ref, Some(id("cal:test")));
        // Exactly three Accepted S3 decisions, each with its persisted artifact.
        let s3: Vec<(&Node, &plumb_psg::ResolutionDecision)> = loaded
            .graph
            .nodes()
            .values()
            .filter(|n| n.status == ElementStatus::Accepted)
            .filter_map(|n| match &n.payload {
                NodePayload::ResolutionDecision(d) if d.question_ref.is_some() => Some((n, d)),
                _ => None,
            })
            .collect();
        let mut kinds: Vec<&str> = s3
            .iter()
            .map(|(_, d)| d.answer["kind"].as_str().unwrap())
            .collect();
        kinds.sort();
        assert_eq!(
            kinds,
            [
                "calculation_fraction",
                "calculation_holiday_policy",
                "calculation_inclusive_end"
            ]
        );
        let artifacts = SqliteArtifactStore::open(&app.path).unwrap();
        for (_, decision) in &s3 {
            assert_eq!(
                artifacts.get(&decision.patch_ref).unwrap().hash,
                decision.patch_ref
            );
        }
        for (_, q) in questions_of(&loaded.graph) {
            assert_eq!(q.status, "Answered");
        }
        assert_eq!(decisions.len(), 3);
    }

    /// The governed HR calculation workflow on `app`: materialize the three calculation
    /// Questions, answer each through the HTTP answer path and accept each proposal with a reason
    /// (which records a proposal-acceptance companion). Returns the final head.
    async fn governed_calculation_run(app: &App) -> plumb_store::LoadedRevision {
        let materialized = app
            .state
            .materialize_questions(
                &app.state.head().unwrap(),
                &QuestionGenerationInput {
                    findings: calculation_gaps(),
                    routing: owner_routing(),
                },
            )
            .unwrap();
        let question_of = |template_id: &str| {
            materialized
                .generation
                .questions
                .iter()
                .find(|q| q.template_id == template_id)
                .unwrap()
                .question_ref
                .clone()
        };
        for (question, value) in [
            (
                question_of("f2.calculation.fraction.v1"),
                json!({"decimal": "0.5", "unit": "working_day"}),
            ),
            (
                question_of("f2.calculation.inclusive-end.v1"),
                json!({"inclusive_end": true}),
            ),
            (
                question_of("f2.calculation.holiday-policy.v1"),
                json!({"holiday_consumes_working_day": false, "calendar_ref": "cal:test"}),
            ),
        ] {
            let r = answer(app, &question, value).await;
            assert_eq!(r.status, StatusCode::OK, "{}", r.text());
            let proposal_id = r.json()["proposal_id"].as_str().unwrap().to_owned();
            let r = write(
                app,
                &format!("/api/proposals/{proposal_id}/accept"),
                Some(json!({"reason": "Confirmed by the owner."})),
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        }
        head(app)
    }

    /// The Accepted question-bound (S3.3) decisions of `graph` by question.
    fn s3_decisions(
        graph: &Graph,
    ) -> std::collections::BTreeMap<Id, (Id, plumb_psg::ResolutionDecision)> {
        graph
            .nodes()
            .values()
            .filter(|n| n.status == ElementStatus::Accepted)
            .filter_map(|n| match &n.payload {
                NodePayload::ResolutionDecision(d) if d.question_ref.is_some() => {
                    Some((d.question_ref.clone().unwrap(), (n.id.clone(), d.clone())))
                }
                _ => None,
            })
            .collect()
    }

    /// R0.5: the same source, answers, actors, policy and acceptances under two FixedClock
    /// instants end with the identical semantic_hash, although the operational provenance
    /// (decision instants, companion identities and revision times) differs.
    #[tokio::test]
    async fn two_clock_runs_have_identical_final_semantic_hash() {
        let early =
            application_app_at(calculation_answer_graph(), "2026-05-01T12:00:00.000000000Z");
        let late = application_app_at(calculation_answer_graph(), "2027-02-14T08:30:00.000000000Z");
        let a = governed_calculation_run(&early).await;
        let b = governed_calculation_run(&late).await;

        // The governed outcome is identical: same Questions, same S3.3 decision IDs, answers,
        // actors and rationale.
        let (da, db) = (s3_decisions(&a.graph), s3_decisions(&b.graph));
        assert_eq!(da.len(), 3);
        assert_eq!(da.keys().collect::<Vec<_>>(), db.keys().collect::<Vec<_>>());
        for (question, (id_a, dec_a)) in &da {
            let (id_b, dec_b) = &db[question];
            assert_eq!(id_a, id_b, "{question}");
            assert_eq!(
                (
                    &dec_a.answer,
                    &dec_a.decided_by,
                    &dec_a.rationale,
                    &dec_a.supersedes
                ),
                (
                    &dec_b.answer,
                    &dec_b.decided_by,
                    &dec_b.rationale,
                    &dec_b.supersedes
                )
            );
            // Operational provenance differs and stays persisted in the complete record.
            assert_ne!(dec_a.decided_at, dec_b.decided_at);
            assert_ne!(
                plumb_psg::node_element_hash(a.graph.node(id_a).unwrap()).unwrap(),
                plumb_psg::node_element_hash(b.graph.node(id_b).unwrap()).unwrap()
            );
        }
        // Proposal-acceptance companions: recorded in both, with clock-dependent identities.
        let (ca, cb) = (
            acceptance_decisions(&a.graph),
            acceptance_decisions(&b.graph),
        );
        assert_eq!(ca.len(), 3);
        assert_eq!(cb.len(), 3);
        let ids_a: std::collections::BTreeSet<&Id> = ca.iter().map(|(i, _)| i).collect();
        let ids_b: std::collections::BTreeSet<&Id> = cb.iter().map(|(i, _)| i).collect();
        assert!(ids_a.is_disjoint(&ids_b));
        // Different revision times.
        assert_ne!(a.revision.created_at, b.revision.created_at);

        // The final semantic identity is the same.
        assert_eq!(a.revision.semantic_hash, b.revision.semantic_hash);
        assert_eq!(
            a.graph.semantic_hash().unwrap(),
            b.graph.semantic_hash().unwrap()
        );
        // ... and the governed answers did change it from the source baseline.
        assert_ne!(
            a.revision.semantic_hash,
            calculation_answer_graph().semantic_hash().unwrap()
        );
    }

    /// R0.5: rematerializing a Question whose routing changed keeps its ID and refreshes it in
    /// place through one non-semantic ReplacePayload; no second Question appears.
    #[tokio::test]
    async fn rematerialization_updates_routing_without_new_question() {
        let base = calculation_answer_graph();
        let graph = Graph::new(
            base.project_id().clone(),
            base.profile_id().clone(),
            base.nodes()
                .values()
                .cloned()
                .chain([stakeholder("stakeholder:deputy")])
                .collect(),
            base.edges().values().cloned().collect(),
        )
        .unwrap();
        let app = application_app(graph);
        let findings = calculation_gaps();
        let first = app
            .state
            .materialize_questions(
                &app.state.head().unwrap(),
                &QuestionGenerationInput {
                    findings: findings.clone(),
                    routing: owner_routing(),
                },
            )
            .unwrap();
        assert!(first.write.is_some());
        let materialized = head(&app);
        let before: std::collections::BTreeMap<Id, plumb_psg::Question> =
            questions_of(&materialized.graph).into_iter().collect();
        assert_eq!(before.len(), 3);
        assert!(before
            .values()
            .all(|q| q.stakeholder_ref == Some(id("stakeholder:owner"))));

        // The routing changes: calendar questions now go to the deputy first.
        let mut rerouted = owner_routing();
        rerouted
            .stakeholders
            .push(plumb_functional::RoutingStakeholder {
                id: id("stakeholder:deputy"),
                name: "Deputy".into(),
                roles: vec!["analyst".into()],
            });
        rerouted.routing.insert(
            "calendar".to_owned(),
            vec![id("stakeholder:deputy"), id("stakeholder:owner")],
        );
        let second = app
            .state
            .materialize_questions(
                &app.state.head().unwrap(),
                &QuestionGenerationInput {
                    findings,
                    routing: rerouted,
                },
            )
            .unwrap();
        assert!(
            second.generation.issues.is_empty(),
            "{:?}",
            second.generation.issues
        );
        let updated: Vec<&Id> = second
            .generation
            .questions
            .iter()
            .filter(|q| q.materialization == plumb_functional::QuestionMaterialization::Updated)
            .map(|q| &q.question_ref)
            .collect();
        assert!(!updated.is_empty());
        // The proposal is non-semantic and refreshes existing Questions only.
        let proposal = second.generation.proposal.clone().unwrap();
        assert_eq!(proposal.materiality, ProposalMateriality::NonSemantic);
        assert_eq!(
            proposal.acceptance_policy,
            AcceptancePolicy::AutoNonSemantic
        );
        let patches = match &proposal.patch_set.patch {
            SemanticPatch::Compound { patches } => patches.clone(),
            single => vec![single.clone()],
        };
        assert_eq!(patches.len(), updated.len());
        for patch in &patches {
            let SemanticPatch::ReplacePayload { target, .. } = patch else {
                panic!("expected only ReplacePayload patches, got {patch:?}");
            };
            assert!(updated.contains(&&target.id));
        }
        // Committed through the ordinary write path: one new revision, same semantic_hash.
        let write = second.write.clone().unwrap();
        let after = head(&app);
        assert_eq!(write.revision, after.revision.id.to_string());
        assert_ne!(after.revision.id, materialized.revision.id);
        assert_eq!(
            after.revision.semantic_hash,
            materialized.revision.semantic_hash
        );
        // Same Question IDs, no second Question, routing refreshed, everything else kept.
        let now: std::collections::BTreeMap<Id, plumb_psg::Question> =
            questions_of(&after.graph).into_iter().collect();
        assert_eq!(
            now.keys().collect::<Vec<_>>(),
            before.keys().collect::<Vec<_>>()
        );
        for (question_ref, question) in &now {
            let old = &before[question_ref];
            if updated.contains(&question_ref) {
                assert_eq!(question.stakeholder_ref, Some(id("stakeholder:deputy")));
                let mut expected = old.clone();
                expected.stakeholder_ref = question.stakeholder_ref.clone();
                expected.priority = question.priority.clone();
                assert_eq!(question, &expected);
            } else {
                assert_eq!(question, old);
            }
        }
        // Rematerializing again is idempotent.
        let third = app
            .state
            .materialize_questions(
                &app.state.head().unwrap(),
                &QuestionGenerationInput {
                    findings: calculation_gaps(),
                    routing: {
                        let mut r = owner_routing();
                        r.stakeholders.push(plumb_functional::RoutingStakeholder {
                            id: id("stakeholder:deputy"),
                            name: "Deputy".into(),
                            roles: vec!["analyst".into()],
                        });
                        r.routing.insert(
                            "calendar".to_owned(),
                            vec![id("stakeholder:deputy"), id("stakeholder:owner")],
                        );
                        r
                    },
                },
            )
            .unwrap();
        assert_eq!(third.write, None);
    }

    // ------------------------------------------------------------------ R0.6 generic confirmation

    /// The base fixture plus Proposed directory-like nodes and edges.
    fn confirmation_graph() -> Graph {
        let (mut nodes, mut edges) = base_parts();
        nodes.extend([
            node(
                "actor:new",
                "Proposed",
                "Actor",
                json!({"name": "New", "actor_kind": "human"}),
            ),
            node(
                "brole:new",
                "Proposed",
                "BusinessRole",
                json!({"name": "Approver"}),
            ),
            node(
                "brole:other",
                "Proposed",
                "BusinessRole",
                json!({"name": "Other"}),
            ),
            node(
                "calc:proposed",
                "Proposed",
                "Calculation",
                json!({
                    "name": "proposed", "expression": "days", "result_type": "Int",
                    "unit": null, "rounding": null, "calendar_ref": null, "examples": null
                }),
            ),
        ]);
        let mut performed = edge(
            "rel:performed-new",
            "performed_by",
            "op:approve",
            "actor:new",
        );
        performed.status = ElementStatus::Proposed;
        let mut dangling = edge(
            "rel:performed-other",
            "performed_by",
            "op:cancel",
            "brole:other",
        );
        dangling.status = ElementStatus::Proposed;
        edges.extend([performed, dangling]);
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap()
    }

    fn confirm(
        app: &App,
        ids: &[&str],
    ) -> Result<plumb_api::openapi::WriteResult, plumb_api::error::ApiError> {
        let ids: Vec<String> = ids.iter().map(|s| (*s).to_owned()).collect();
        app.state.confirm_elements(
            &app.state.head().unwrap(),
            &ids,
            Some("Confirmed by the analyst.".into()),
        )
    }

    #[tokio::test]
    async fn generic_confirmation_promotes_listed_proposed_elements() {
        let app = application_app(confirmation_graph());
        let before = head(&app);
        let write = confirm(&app, &["rel:performed-new", "brole:new", "actor:new"]).unwrap();
        let after = head(&app);
        // One atomic revision with exactly the listed status changes.
        assert_eq!(write.revision, after.revision.id.to_string());
        assert_eq!(
            after.revision.id.version(),
            before.revision.id.version() + 1
        );
        for element in ["actor:new", "brole:new"] {
            let old = before.graph.node(&id(element)).unwrap();
            let new = after.graph.node(&id(element)).unwrap();
            assert_eq!(new.status, ElementStatus::Accepted, "{element}");
            assert_eq!(new.payload, old.payload, "no content edit");
            assert_eq!(new.evidence, old.evidence);
        }
        let edge = after.graph.edge(&id("rel:performed-new")).unwrap();
        assert_eq!(edge.status, ElementStatus::Accepted);
        // Untouched Proposed elements stay Proposed.
        assert_eq!(
            after.graph.node(&id("brole:other")).unwrap().status,
            ElementStatus::Proposed
        );
        // The governed acceptance recorded its companion with the given rationale.
        let companions = acceptance_decisions(&after.graph);
        assert_eq!(companions.len(), 1);
        assert_eq!(
            companions[0].1.rationale.as_deref(),
            Some("Confirmed by the analyst.")
        );
        // The committed effect is exactly the sequential SetStatus leaves: nodes, then edges.
        let committed = SqliteArtifactStore::open(&app.path)
            .unwrap()
            .get(&companions[0].1.patch_ref)
            .unwrap();
        let patch_set: PatchSet = serde_json::from_slice(&committed.bytes).unwrap();
        assert_eq!(patch_set.base_semantic_hash, before.revision.semantic_hash);
        let SemanticPatch::Compound { patches } = &patch_set.patch else {
            panic!("expected sequential leaves");
        };
        let targets: Vec<String> = patches
            .iter()
            .map(|p| match p {
                SemanticPatch::SetStatus { target, from, to } => {
                    assert_eq!(
                        (*from, *to),
                        (ElementStatus::Proposed, ElementStatus::Accepted)
                    );
                    target.id.to_string()
                }
                other => panic!("not a SetStatus leaf: {other:?}"),
            })
            .collect();
        assert_eq!(targets, ["actor:new", "brole:new", "rel:performed-new"]);
        // An edge whose endpoint stays Proposed is refused, as is a non-Proposed element.
        let err = confirm(&app, &["rel:performed-other"]).unwrap_err();
        assert_eq!(err.body().code, "E_SEMANTIC_INVARIANT");
        let err = confirm(&app, &["actor:new"]).unwrap_err();
        assert_eq!(err.body().code, "E_SEMANTIC_INVARIANT");
        // The edge and its endpoint together are accepted.
        confirm(&app, &["rel:performed-other", "brole:other"]).unwrap();
        // Input is an explicit non-empty set of existing elements.
        assert_eq!(
            confirm(&app, &[]).unwrap_err().body().code,
            "E_INVALID_REQUEST"
        );
        assert_eq!(
            confirm(&app, &["actor:clerk", "actor:clerk"])
                .unwrap_err()
                .body()
                .code,
            "E_INVALID_REQUEST"
        );
        assert_eq!(
            confirm(&app, &["actor:missing"]).unwrap_err().body().code,
            "E_NOT_FOUND"
        );
    }

    #[tokio::test]
    async fn generic_confirmation_refuses_dedicated_and_governance_types() {
        let app = application_app(confirmation_graph());
        let before = head(&app);
        for (element, reason) in [
            ("term:vacation", "dedicated_confirmation"),
            ("calc:proposed", "dedicated_confirmation"),
            ("scn:derived-a", "dedicated_confirmation"),
            ("assumption:active", "governance_type"),
            ("agent:compiler", "provenance_or_proof_type"),
            ("stakeholder:hr", "not_proposed"),
        ] {
            let err = confirm(&app, &[element]).unwrap_err();
            let body = err.body();
            assert_eq!(body.code, "E_SEMANTIC_INVARIANT", "{element}");
            let details = serde_json::to_value(&body.details).unwrap();
            assert_eq!(details["reason"], reason, "{element}");
            assert_eq!(details["element"], element);
        }
        // A refused member refuses the whole set: nothing is partially confirmed.
        let err = confirm(&app, &["actor:new", "term:vacation"]).unwrap_err();
        assert_eq!(err.body().code, "E_SEMANTIC_INVARIANT");
        let after = head(&app);
        assert_eq!(after.revision.id, before.revision.id);
        assert_eq!(
            after.graph.node(&id("actor:new")).unwrap().status,
            ElementStatus::Proposed
        );
    }

    #[tokio::test]
    async fn generic_confirmation_promotes_proposed_requirement() {
        // Fable #301: an S1.x Proposed Requirement reaches Accepted through the same generic
        // governed confirmation as every other eligible element.
        let app = application_app(confirmation_graph());
        let before = head(&app);
        assert_eq!(
            before.graph.node(&id("req:p")).unwrap().status,
            ElementStatus::Proposed
        );
        let write = confirm(&app, &["req:p"]).unwrap();
        let after = head(&app);
        assert_eq!(write.revision, after.revision.id.to_string());
        let node = after.graph.node(&id("req:p")).unwrap();
        assert_eq!(node.status, ElementStatus::Accepted);
        assert_eq!(
            node.payload,
            before.graph.node(&id("req:p")).unwrap().payload
        );
        assert_eq!(acceptance_decisions(&after.graph).len(), 1);
        assert_ne!(after.revision.semantic_hash, before.revision.semantic_hash);
    }

    // ------------------------------------------------------------------ R0.6 answer service

    #[tokio::test]
    async fn answer_route_uses_answer_service_unchanged() {
        let app = application_app(calculation_answer_graph());
        let materialized = app
            .state
            .materialize_questions(
                &app.state.head().unwrap(),
                &QuestionGenerationInput {
                    findings: calculation_gaps(),
                    routing: owner_routing(),
                },
            )
            .unwrap();
        let question = materialized
            .generation
            .questions
            .iter()
            .find(|q| q.template_id == "f2.calculation.inclusive-end.v1")
            .unwrap()
            .question_ref
            .clone();
        let loaded = head(&app);
        // The pre-R0.6 route logic, computed independently: S3.3 on the loaded head with the
        // request actor and the injected clock instant.
        let expected = plumb_functional::resolution::resolve_question(
            &loaded.graph,
            &plumb_functional::resolution::ResolutionInput {
                question_ref: question.clone(),
                answer: json!({"inclusive_end": true}),
                decided_by: id(ACTOR),
                decided_at: ts(NOW),
                rationale: Some("The policy says so.".into()),
                supersedes: None,
            },
            &plumb_functional::resolution::ResolutionContext {
                invariant_bindings: Default::default(),
            },
        )
        .unwrap();
        let tag = semantic(&app).await;
        let r = post(
            &app,
            &format!("/api/questions/{question}/answer"),
            Some(&tag),
            Some(
                json!({"answer": {"inclusive_end": true}, "rationale": "The policy says so.",
                        "supersedes": null}),
            ),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let body = r.json();
        // The response contract is unchanged: exact keys and exact values.
        assert_eq!(
            body.as_object().unwrap().keys().collect::<Vec<_>>(),
            [
                "action",
                "events",
                "intake",
                "patch_preview",
                "proposal_id",
                "revision",
                "semantic_hash",
                "version"
            ]
        );
        assert_eq!(body["proposal_id"], expected.proposal.id.to_string());
        assert_eq!(
            body["patch_preview"],
            serde_json::to_value(&expected.proposal.patch_set.patch).unwrap()
        );
        assert_eq!(body["action"], "accept");
        assert_eq!(body["revision"], loaded.revision.id.to_string());
        assert_eq!(body["version"], loaded.revision.id.to_string());
        assert_eq!(
            body["semantic_hash"],
            loaded.revision.semantic_hash.to_string()
        );
        assert_eq!(body["events"], json!([]));
        // Nothing committed; the pending Question-origin proposal and its artifact persist.
        assert_eq!(head(&app).revision.id, loaded.revision.id);
        let stored = workflow(&app)
            .proposal(&expected.proposal.id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.state, WorkflowProposalState::Pending);
        assert_eq!(stored.origin_kind, WorkflowOrigin::Question);
        assert_eq!(stored.origin_ref, Some(question.clone()));
        assert_eq!(stored.base_revision_id, loaded.revision.id);
        assert_eq!(stored.proposal_kind, WorkflowProposalKind::Accept);
        let persisted: Proposal = serde_json::from_str(&stored.proposal_json).unwrap();
        assert_eq!(persisted, expected.proposal);
        let artifact = SqliteArtifactStore::open(&app.path)
            .unwrap()
            .get(&expected.decision.patch_ref)
            .unwrap();
        assert_eq!(artifact.bytes, expected.patch_artifact_bytes);
        assert_eq!(artifact.kind, ArtifactKind::Patch);
        // The service is the same for a direct (CLI) caller: an identical answer is the same
        // idempotent pending proposal.
        let direct = app
            .state
            .answer_question(
                &head(&app),
                question.as_str(),
                json!({"inclusive_end": true}),
                Some("The policy says so.".into()),
                None,
            )
            .unwrap();
        assert_eq!(serde_json::to_value(&direct).unwrap(), body);
        // Errors keep their codes and order: body validation, then If-Match, then lookup.
        let uri = format!("/api/questions/{question}/answer");
        assert_error(
            &post(
                &app,
                &uri,
                Some(&tag),
                Some(json!({"answer": [1], "rationale": null, "supersedes": null})),
            )
            .await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &post(
                &app,
                &uri,
                Some(&tag),
                Some(json!({"answer": {}, "rationale": null, "supersedes": "not an id"})),
            )
            .await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &post(
                &app,
                "/api/questions/q:0000000000000000/answer",
                Some(&tag),
                Some(json!({"answer": {}, "rationale": null, "supersedes": null})),
            )
            .await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
    }

    // ------------------------------------------------------------------ R0.6 EARS projection

    /// Answers consolidate deterministically: no vocabulary mention and, for every target, a
    /// ubiquitous EARS suggestion grounded in its source statement.
    struct ConsolidateProvider {
        calls: Mutex<usize>,
    }

    impl InferenceProvider for ConsolidateProvider {
        fn execute(
            &self,
            request: &InferenceRequest,
            material: &InferenceRequestMaterial,
        ) -> Result<ProviderExecution, ProviderError> {
            validate_binding(request, material)?;
            *self.calls.lock().unwrap() += 1;
            let context: Value = serde_json::from_slice(&material.context_json).unwrap();
            let output = match request.task_kind.as_str() {
                "vocabulary_analysis" => json!({"version": 1, "mentions": []}),
                "ears_normalization" => {
                    let entries: Vec<Value> = context["requirements"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| {
                            let source = r["source_statement"].as_str().unwrap();
                            let range = |part: &str| {
                                let start = source.find(part).unwrap();
                                json!({"kind": "source_range", "start": start, "end": start + part.len()})
                            };
                            json!({"requirement_ref": r["requirement_ref"], "suggestion": {
                                "pattern": "ubiquitous", "actor": range("the clerk"),
                                "trigger": null, "condition": null,
                                "response": range("pack the order")
                            }})
                        })
                        .collect();
                    json!({"version": 1, "requirements": entries})
                }
                other => panic!("unexpected task kind {other}"),
            };
            Ok(execution(request, output))
        }
    }

    /// The base fixture plus one S1.1-origin Accepted Requirement `req:ears`.
    fn ears_graph() -> Graph {
        let (mut nodes, edges) = base_parts();
        let statement = "Always, the clerk shall pack the order.";
        let source = plumb_import::import_markdown(
            "requirements.md",
            format!("{statement}\n").as_bytes(),
            &plumb_import::ImportAudit {
                created_by: id(ACTOR),
                created_at: ts(T0),
            },
        )
        .unwrap();
        let fragment = source
            .fragments
            .iter()
            .find(|f| {
                matches!(&f.payload, NodePayload::EvidenceFragment(e)
                    if e.extracted_text.as_deref() == Some(statement))
            })
            .unwrap()
            .clone();
        let mut value =
            serde_json::to_value(requirement("req:ears", "Accepted", statement)).unwrap();
        value["evidence"] = json!([fragment.id]);
        value["extensions"] = json!({"plumb_functional:segment_origin": {
            "candidate_ref": "seg:0000000000000001", "fragment_ref": fragment.id,
            "start": 0, "end": statement.len()
        }});
        nodes.push(source.source.clone());
        nodes.extend(source.fragments.clone());
        nodes.push(serde_json::from_value(value).unwrap());
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap()
    }

    #[tokio::test]
    async fn requirement_detail_surfaces_pending_ears_proposal() {
        let provider = Arc::new(ConsolidateProvider {
            calls: Mutex::new(0),
        });
        let app = application_app_with(ears_graph(), NOW, Some(provider.clone()));
        let detail_uri = "/api/requirements/req:ears";
        // Before consolidate: no pending EARS proposal, both fields null.
        let detail = get(&app, detail_uri).await.json();
        assert_eq!(detail["ears_suggestion"], Value::Null);
        assert_eq!(detail["ears_proposal_ref"], Value::Null);
        // An unrelated intake-origin ReplacePayload of the same Requirement is never EARS.
        let loaded = head(&app);
        let current = loaded.graph.node(&id("req:ears")).unwrap();
        let NodePayload::Requirement(mut edited) = current.payload.clone() else {
            unreachable!()
        };
        edited.statement = "The clerk shall pack every order.".into();
        let unrelated = Proposal::new(
            plumb_core::StageId::S1,
            PatchSet {
                base_semantic_hash: loaded.revision.semantic_hash.clone(),
                patch: SemanticPatch::ReplacePayload {
                    target: pre(&loaded.graph, "req:ears"),
                    payload: NodePayload::Requirement(edited),
                },
            },
            current.evidence.clone(),
            vec![],
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .unwrap();
        let r = write(
            &app,
            "/api/proposals/intake",
            Some(json!({"payload": unrelated, "kind": "accept",
                        "evidence": unrelated.evidence_refs})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let detail = get(&app, detail_uri).await.json();
        assert_eq!(detail["ears_proposal_ref"], Value::Null);
        // The real consolidate recipe (scoped to the S1.1-origin Requirement) persists the
        // stage-linked S1.2 EARS proposal pending.
        let run = app
            .state
            .run_stage(
                plumb_api::Recipe::Consolidate,
                Some(vec!["req:ears".into()]),
                None,
            )
            .unwrap();
        app.state.execute_recipe_job(&run.job_id).unwrap();
        let links = plumb_store::SqliteStageStore::open(&app.path)
            .unwrap()
            .pending_proposal_links()
            .unwrap();
        assert_eq!(links.len(), 1, "one EARS proposal");
        let ears_ref = links[0].proposal_id.clone();
        let calls = *provider.calls.lock().unwrap();
        let detail = get(&app, detail_uri).await.json();
        assert_eq!(detail["ears_suggestion"], "the clerk shall pack the order.");
        assert_eq!(detail["ears_proposal_ref"], ears_ref.to_string());
        // The stored proposal is the authority; reading regenerates nothing.
        let stored: Proposal = serde_json::from_str(
            &workflow(&app)
                .proposal(&ears_ref)
                .unwrap()
                .unwrap()
                .proposal_json,
        )
        .unwrap();
        let SemanticPatch::ReplacePayload {
            payload: NodePayload::Requirement(r),
            ..
        } = &stored.patch_set.patch
        else {
            panic!("not an EARS ReplacePayload");
        };
        assert_eq!(detail["ears_suggestion"], r.statement.as_str());
        assert_eq!(*provider.calls.lock().unwrap(), calls);
        assert_eq!(links.len(), 1);
        // Other requirements have none.
        let other = get(&app, "/api/requirements/req:a").await.json();
        assert_eq!(other["ears_suggestion"], Value::Null);
        assert_eq!(other["ears_proposal_ref"], Value::Null);
        // Accepting it through the governed proposal acceptance applies the statement; the
        // detail then has no pending EARS proposal.
        let r = write(
            &app,
            &format!("/api/proposals/{ears_ref}/accept"),
            Some(json!({"reason": "EARS form reviewed."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let detail = get(&app, detail_uri).await.json();
        assert_eq!(detail["text"], "the clerk shall pack the order.");
        assert_eq!(detail["ears_suggestion"], Value::Null);
        assert_eq!(detail["ears_proposal_ref"], Value::Null);
    }

    /// Hotfix 081: an actual intake response projects each validation finding through the
    /// shared Finding projection with `rule_class` read from the active profile's metadata for
    /// the exact finding code. Production intake runs with no candidate gate reports
    /// (`IntakeValidationInput::empty()`), so the pending proposal is stored with a real X1.3
    /// intake report generated from a real F1 report; re-submitting the identical proposal
    /// returns that stored intake through `intake_view`. The active profile is a validated
    /// test-local clone that reclassifies one finding's rule, so the projection must use it.
    #[tokio::test]
    async fn http_workflow_intake_validation_findings_carry_profile_rule_class() {
        let graph = fixture_graph();
        // A finding-touched Accepted requirement whose title the proposal replaces.
        let target = f1_report(&graph)
            .findings
            .iter()
            .flat_map(|f| &f.payload.affected_refs)
            .find(|r| {
                graph
                    .node(r)
                    .is_some_and(|n| matches!(n.payload, NodePayload::Requirement(_)))
            })
            .unwrap()
            .clone();
        let NodePayload::Requirement(mut payload) = graph.node(&target).unwrap().payload.clone()
        else {
            unreachable!()
        };
        payload.title = Some("Leave totals export".into());
        let proposal = Proposal::new(
            plumb_core::StageId::S1,
            PatchSet {
                base_semantic_hash: graph.semantic_hash().unwrap(),
                patch: SemanticPatch::ReplacePayload {
                    target: pre(&graph, target.as_str()),
                    payload: NodePayload::Requirement(payload),
                },
            },
            vec![],
            vec![],
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .unwrap();
        // Intake candidate reports are evaluated over the proposal's candidate graph.
        let candidate = apply_patch(&graph, &proposal.patch_set).unwrap().graph;
        let report = f1_report(&candidate);
        let generated = plumb_intake::intake(
            &graph,
            &proposal,
            &plumb_intake::IntakeValidationInput::new(vec![report]).unwrap(),
        )
        .unwrap();
        assert!(!generated.validation_findings.is_empty());

        // The test-local active profile: the first finding's rule becomes organization_policy.
        let mut active = profile();
        let reclassified = generated.validation_findings[0].payload.code.clone();
        let rule = active
            .rules
            .iter_mut()
            .find(|rule| rule.id == reclassified)
            .unwrap();
        assert_ne!(rule.rule_class, RuleClass::OrganizationPolicy);
        rule.rule_class = RuleClass::OrganizationPolicy;
        active.validate().unwrap();
        let app = app_with_profile(graph, |_| Vec::new(), active.clone());
        let revision = head(&app).revision;
        assert_eq!(revision.profile_hash, active.profile_hash().unwrap());
        assert_eq!(revision.rule_pack_hash, active.rule_pack_hash().unwrap());

        let inserted = workflow(&app)
            .insert_proposal(
                &NewWorkflowProposal {
                    proposal_id: proposal.id.clone(),
                    base_revision_id: revision.id.clone(),
                    proposal_json: serde_json::to_value(&proposal).unwrap(),
                    intake_json: serde_json::to_value(&generated).unwrap(),
                    proposal_kind: WorkflowProposalKind::Accept,
                    origin_kind: WorkflowOrigin::Intake,
                    origin_ref: None,
                    created_at: ts(T0),
                },
                WorkflowProposalState::Pending,
            )
            .unwrap();
        assert!(matches!(inserted, WorkflowInsert::Inserted(_)));

        let r = intake(&app, "accept", &proposal).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let findings = r.json()["intake"]["validation_findings"]
            .as_array()
            .unwrap()
            .clone();
        assert!(!findings.is_empty());
        assert_eq!(findings.len(), generated.validation_findings.len());
        for finding in &findings {
            let code = finding["code"].as_str().unwrap();
            let rule = active.rules.iter().find(|rule| rule.id == code).unwrap();
            assert_eq!(
                finding["rule_class"],
                json!(rule.rule_class.as_str()),
                "{code}"
            );
            assert_eq!(finding["family"], json!(rule.gate.as_str()), "{code}");
        }
        assert!(findings
            .iter()
            .any(|f| f["code"] == json!(reclassified)
                && f["rule_class"] == json!("organization_policy")));
    }

    #[tokio::test]
    async fn http_workflow_proposal_intake_actions_and_baseline() {
        let app = app();
        let proposal = requirement_proposal(&app, "req:new").await;
        assert_error(
            &intake(&app, "link", &proposal).await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_ACTION_MISMATCH",
        );
        assert_error(
            &write(&app, "/api/proposals/intake", Some(json!({"kind": "create", "payload": proposal, "evidence": ["evd:0000000000000000"]}))).await,
            StatusCode::BAD_REQUEST,
            "E_PROPOSAL_EVIDENCE_MISMATCH",
        );
        assert_error(
            &write(
                &app,
                "/api/proposals/intake",
                Some(json!({"kind": "create", "payload": {"id": "prop:x"}, "evidence": []})),
            )
            .await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        let r = intake(&app, "create", &proposal).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(r.json()["events"], json!([]));
        assert_eq!(r.json()["intake"]["requires_reason"], false);
        // An identical pending intake is idempotent.
        assert_eq!(intake(&app, "create", &proposal).await.json(), r.json());
        // The stored proposal survives a restart.
        let app = restarted(app, ACTOR);
        assert_eq!(
            workflow(&app)
                .proposal(&proposal.id)
                .unwrap()
                .unwrap()
                .state,
            WorkflowProposalState::Pending
        );
        // A non-semantic commit moves the revision but not the semantic hash: still a
        // baseline change for a stored proposal, which is never rebased.
        let r = write(
            &app,
            "/api/rounds",
            Some(json!({"stakeholder": "stakeholder:hr", "max": 1})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{}/create", proposal.id),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_BASELINE_CHANGED",
        );
        // A proposal built on another semantic base is refused at intake.
        let mut stale = requirement_proposal(&app, "req:other").await;
        stale.patch_set.base_semantic_hash =
            "psg:sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .parse()
                .unwrap();
        let stale = Proposal::new(
            stale.stage,
            stale.patch_set,
            vec![],
            vec![],
            stale.materiality,
            stale.acceptance_policy,
            None,
        )
        .unwrap();
        assert_error(
            &intake(&app, "create", &stale).await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_BASELINE_CHANGED",
        );
        // Fresh proposal: reject keeps the PSG and closes the proposal.
        let fresh = requirement_proposal(&app, "req:third").await;
        assert_eq!(intake(&app, "create", &fresh).await.status, StatusCode::OK);
        let before = revision(&app).await;
        let r = write(
            &app,
            &format!("/api/proposals/{}/reject", fresh.id),
            Some(json!({"reason": "Out of scope."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(r.json()["revision"], before);
        assert_eq!(r.json()["events"], json!([]));
        let record = workflow(&app).proposal(&fresh.id).unwrap().unwrap();
        assert_eq!(record.state, WorkflowProposalState::Rejected);
        assert_eq!(record.resolved_revision_id.unwrap().as_str(), before);
        assert_error(
            &intake(&app, "create", &fresh).await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_NOT_PENDING",
        );
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{}/reject", fresh.id),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_NOT_PENDING",
        );
        assert_error(
            &write(
                &app,
                "/api/proposals/prop:0000000000000000/accept",
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        // An accept-class proposal commits with a companion even without a reason.
        let accept = Proposal::new(
            plumb_core::StageId::S1,
            PatchSet {
                base_semantic_hash: semantic(&app).await.parse().unwrap(),
                patch: SemanticPatch::SetStatus {
                    target: plumb_patch::ElementPrecondition {
                        id: id("req:p"),
                        expected_hash: plumb_psg::node_element_hash(
                            head(&app).graph.node(&id("req:p")).unwrap(),
                        )
                        .unwrap(),
                    },
                    from: ElementStatus::Proposed,
                    to: ElementStatus::Accepted,
                },
            },
            vec![],
            vec![],
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .unwrap();
        assert_eq!(intake(&app, "accept", &accept).await.status, StatusCode::OK);
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{}/link", accept.id),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_ACTION_MISMATCH",
        );
        let r = write(
            &app,
            &format!("/api/proposals/{}/accept", accept.id),
            Some(json!({"reason": null})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let companions = acceptance_decisions(&head(&app).graph);
        assert_eq!(companions.len(), 1);
        assert_eq!(companions[0].1.rationale, None);
        assert_eq!(companions[0].1.answer["reason_codes"], json!([]));
    }

    #[tokio::test]
    async fn http_workflow_interrupted_acceptance_is_reconciled() {
        let app = app();
        let proposal = requirement_proposal(&app, "req:crash").await;
        assert_eq!(
            intake(&app, "create", &proposal).await.status,
            StatusCode::OK
        );
        // Simulate a crash after the graph CAS: the acceptance commit exists, the workflow row
        // was never finalized.
        let loaded = head(&app);
        let companion = plumb_functional::resolution::build_acceptance_companion(
            &loaded.graph,
            &proposal,
            &plumb_functional::resolution::AcceptanceInput {
                reason_codes: vec![],
                decided_by: id(ACTOR),
                decided_at: ts(NOW),
                rationale: None,
            },
        )
        .unwrap();
        let mut store = SqliteRevisionStore::open(&app.path).unwrap();
        store
            .commit(
                &BranchName::main(),
                &loaded.revision.id,
                &companion.patch_set,
                RevisionWriteMeta {
                    decision_refs: vec![companion.decision_ref.clone()],
                    created_by: id(ACTOR),
                    created_at: ts(NOW),
                },
            )
            .unwrap();
        drop(store);
        let app = restarted(app, ACTOR);
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{}/create", proposal.id),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_NOT_PENDING",
        );
        let record = workflow(&app).proposal(&proposal.id).unwrap().unwrap();
        assert_eq!(record.state, WorkflowProposalState::Accepted);
        assert_eq!(record.resolved_at, Some(ts(NOW)));
        // Two conflicting acceptance decisions are corrupt workflow state.
        let other = requirement_proposal(&app, "req:twice").await;
        assert_eq!(intake(&app, "create", &other).await.status, StatusCode::OK);
        let loaded = head(&app);
        let mut extra = Vec::new();
        for (n, at) in [
            (1, "2026-05-01T12:00:01.000000000Z"),
            (2, "2026-05-01T12:00:02.000000000Z"),
        ] {
            let mut decision = node(
                &format!("dec:000000000000000{n}"),
                "Accepted",
                "ResolutionDecision",
                json!({
                    "question_ref": null, "proposal_ref": other.id.as_str(),
                    "answer": {"kind": "proposal_acceptance", "reason_codes": []},
                    "decided_by": ACTOR, "decided_at": at,
                    "patch_ref": Hash::content_sha256(b"x").as_str(), "rationale": null, "supersedes": null
                }),
            );
            decision.audit = plumb_psg::AuditMeta::new(id(ACTOR), ts(NOW), None, None).unwrap();
            extra.push(SemanticPatch::AddNode { node: decision });
        }
        let mut store = SqliteRevisionStore::open(&app.path).unwrap();
        store
            .commit(
                &BranchName::main(),
                &loaded.revision.id,
                &PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch: SemanticPatch::Compound { patches: extra },
                },
                RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id(ACTOR),
                    created_at: ts(NOW),
                },
            )
            .unwrap();
        drop(store);
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{}/create", other.id),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::INTERNAL_SERVER_ERROR,
            "E_WORKFLOW_CORRUPT",
        );
    }

    // ------------------------------------------------------------------ rounds

    #[tokio::test]
    async fn http_workflow_targeted_rounds_create_and_reconstruct() {
        let app = app();
        for (body, status, code) in [
            (
                json!({"stakeholder": "stakeholder:hr", "max": 0}),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                json!({"stakeholder": "stakeholder:hr", "max": 1001}),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                json!({"stakeholder": "stakeholder:ghost", "max": 1}),
                StatusCode::NOT_FOUND,
                "E_NOT_FOUND",
            ),
            (
                json!({"stakeholder": "op:submit", "max": 1}),
                StatusCode::CONFLICT,
                "E_ROUND_STAKEHOLDER_INVALID",
            ),
            (
                json!({"stakeholder": "stakeholder:architect", "max": 1}),
                StatusCode::CONFLICT,
                "E_NO_QUESTIONS_FOR_ROUND",
            ),
        ] {
            assert_error(&write(&app, "/api/rounds", Some(body)).await, status, code);
        }
        let semantic_before = semantic(&app).await;
        let r = write(
            &app,
            "/api/rounds",
            Some(json!({"stakeholder": "stakeholder:hr", "max": 1})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let body = r.json();
        // Non-semantic: the semantic hash is unchanged while the revision advances.
        assert_eq!(body["semantic_hash"], semantic_before);
        assert_eq!(body["events"][0]["kind"], "create_round");
        let round = body["round"].clone();
        assert_eq!(round["stakeholder"], "stakeholder:hr");
        assert_eq!(round["questions"].as_array().unwrap().len(), 1);
        let round_id = round["id"].as_str().unwrap();
        let got = get(&app, &format!("/api/rounds/{round_id}")).await;
        assert_eq!(got.status, StatusCode::OK, "{}", got.text());
        assert_eq!(got.json(), round);
        let member = round["questions"][0].as_str().unwrap();
        let listed = get(&app, &format!("/api/questions?round={round_id}"))
            .await
            .json();
        assert_eq!(listed["items"][0]["id"], member);
        assert_eq!(listed["items"][0]["round_ref"], round_id);
        assert_error(
            &get(&app, "/api/rounds/rnd:0000000000000000").await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
    }

    // ------------------------------------------------------------------ sessions

    async fn create_session(app: &App, scope: Value) -> Reply {
        write(app, "/api/sessions", Some(json!({"scope": scope}))).await
    }

    async fn turn(app: &App, session: &str, text: &str) -> Reply {
        write(
            app,
            &format!("/api/sessions/{session}/turns"),
            Some(json!({"text": text, "stage": "S1"})),
        )
        .await
    }

    #[tokio::test]
    async fn http_workflow_sessions_are_durable_and_owned() {
        let app = app();
        assert_error(
            &create_session(&app, json!(["req:ghost"])).await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        assert_error(
            &create_session(&app, json!(["req:p"])).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_SEMANTIC_INVARIANT",
        );
        assert_error(
            &create_session(&app, json!(["not an id"])).await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        let before = revision(&app).await;
        let r = create_session(&app, json!(["req:b", "req:a", "req:a"])).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(r.json()["revision"], before);
        assert_eq!(r.json()["events"], json!([]));
        let session = r.json()["id"].as_str().unwrap().to_owned();
        assert!(session.starts_with("ses:"));
        let detail = get(&app, &format!("/api/sessions/{session}")).await;
        assert_eq!(detail.status, StatusCode::OK, "{}", detail.text());
        assert_eq!(
            detail.json(),
            json!({"id": session, "scope": ["req:a", "req:b"], "assistant_actor_ref": ASSISTANT})
        );
        assert_error(
            &get(&app, "/api/sessions/ses:0000000000000000").await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        // Durable across a restart; owned by its creator only.
        let app = restarted(app, ACTOR);
        assert_eq!(
            get(&app, &format!("/api/sessions/{session}")).await.status,
            StatusCode::OK
        );
        let other = restarted(app, "agent:other");
        assert_error(
            &get(&other, &format!("/api/sessions/{session}")).await,
            StatusCode::FORBIDDEN,
            "E_SESSION_NOT_OWNED",
        );
        assert_error(
            &get(&other, &format!("/api/sessions/{session}/metrics")).await,
            StatusCode::FORBIDDEN,
            "E_SESSION_NOT_OWNED",
        );
        assert_error(
            &turn(&other, &session, "Hello").await,
            StatusCode::FORBIDDEN,
            "E_SESSION_NOT_OWNED",
        );
    }

    #[tokio::test]
    async fn http_workflow_session_turns_commit_transcript_then_bind_proposals() {
        let app = app();
        let session = create_session(&app, json!(["req:a"])).await.json()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_error(
            &turn(&app, &session, "  ").await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        assert_error(
            &write(
                &app,
                &format!("/api/sessions/{session}/turns"),
                Some(json!({"text": "Hi", "stage": "S13"})),
            )
            .await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        let semantic_before = semantic(&app).await;
        let revision_before = revision(&app).await;
        let r = turn(&app, &session, "Employees request leave.").await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let body = r.json();
        assert_eq!(body["reply"], "Noted.");
        assert_eq!(body["proposals"], json!([]));
        assert_eq!(body["asked_question"], Value::Null);
        // One transcript CAS: one event, a new revision, the same semantic hash.
        assert_eq!(body["events"].as_array().unwrap().len(), 1);
        assert_ne!(body["revision"], revision_before);
        assert_eq!(body["semantic_hash"], semantic_before);
        let turns = workflow(&app).turns(&id(&session)).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].state, WorkflowTurnState::Committed);
        assert_eq!(
            turns[0].committed_revision_id.as_ref().unwrap().as_str(),
            body["revision"].as_str().unwrap()
        );
        assert_eq!(turns[0].expected_node_refs.len(), 4);
        let g = head(&app).graph;
        for expected in &turns[0].expected_node_refs {
            assert!(g.node(expected).is_some(), "{expected}");
        }

        // A turn with a generated proposal: prepared, then pending on the transcript revision.
        app.provider
            .set(ProviderMode::Propose, Some(&semantic(&app).await));
        let r = turn(&app, &session, "Employees should see their balance.").await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let body = r.json();
        let proposals = body["proposals"].as_array().unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0]["kind"], "create");
        assert_eq!(proposals[0]["payload"]["op"], "AddNode");
        let proposal_id = proposals[0]["id"].as_str().unwrap();
        let stored = workflow(&app).proposal(&id(proposal_id)).unwrap().unwrap();
        assert_eq!(stored.state, WorkflowProposalState::Pending);
        assert_eq!(
            stored.base_revision_id.as_str(),
            body["revision"].as_str().unwrap()
        );
        assert_eq!(stored.origin_ref, Some(id(&session)));
        // Hotfix 077: the card kind is the authoritative ProposalKind of exactly the stored
        // proposal, so its one positive action endpoint is /create.
        assert_eq!(stored.proposal_kind, WorkflowProposalKind::Create);
        assert_eq!(stored.proposal_id.as_str(), proposal_id);
        let persisted: Proposal = serde_json::from_str(&stored.proposal_json).unwrap();
        assert_eq!(persisted.id.as_str(), proposal_id);
        assert_eq!(
            proposals[0]["payload"],
            serde_json::to_value(&persisted.patch_set.patch).unwrap()
        );
        let metrics = get(&app, &format!("/api/sessions/{session}/metrics"))
            .await
            .json();
        assert_eq!(
            metrics,
            json!({"turns": 2, "proposals": 1, "requires_reason": 0})
        );
        // The generated proposal is accepted through the proposal endpoints on that revision.
        let r = write(
            &app,
            &format!("/api/proposals/{proposal_id}/create"),
            Some(json!({"reason": null})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());

        // Provider failure: 502, nothing committed, no turn recorded.
        app.provider.set(ProviderMode::Fail, None);
        let before = revision(&app).await;
        let r = turn(&app, &session, "Anything else?").await;
        assert_error(&r, StatusCode::BAD_GATEWAY, "E_INFERENCE_FAILED");
        assert_eq!(r.json()["details"]["error"], "ProviderDisabled");
        assert_eq!(revision(&app).await, before);
        assert_eq!(workflow(&app).turns(&id(&session)).unwrap().len(), 2);

        // A lost transcript race: 412 without retry; the turn and its proposals fail.
        app.provider
            .set(ProviderMode::Propose, Some(&semantic(&app).await));
        *app.provider.race.lock().unwrap() = Some(app.path.clone());
        let r = turn(&app, &session, "One more requirement.").await;
        assert_error(&r, StatusCode::PRECONDITION_FAILED, "E_PRECONDITION_FAILED");
        let turns = workflow(&app).turns(&id(&session)).unwrap();
        let last = turns.last().unwrap();
        assert_eq!(last.state, WorkflowTurnState::Failed);
        assert_eq!(last.failure.as_deref(), Some("STALE_BASE"));
        for proposal in &last.proposal_ids {
            assert_eq!(
                workflow(&app).proposal(proposal).unwrap().unwrap().state,
                WorkflowProposalState::Failed
            );
        }
        // Metrics count committed turns only.
        let metrics = get(&app, &format!("/api/sessions/{session}/metrics"))
            .await
            .json();
        assert_eq!(
            metrics,
            json!({"turns": 2, "proposals": 1, "requires_reason": 0})
        );
        // A stale If-Match is 412 before anything runs.
        let stale = post(
            &app,
            &format!("/api/sessions/{session}/turns"),
            Some("psg:sha256:0000000000000000000000000000000000000000000000000000000000000000"),
            Some(json!({"text": "x", "stage": "S1"})),
        )
        .await;
        assert_error(
            &stale,
            StatusCode::PRECONDITION_FAILED,
            "E_PRECONDITION_FAILED",
        );
    }

    /// Hotfix 066 regression: the session route hands the provider the request and the exact
    /// material of the same plan, with no store lock held during the call, and the persisted
    /// request is the one the provider saw.
    #[tokio::test]
    async fn http_workflow_session_turns_hand_the_provider_their_plan_material() {
        let app = run_app();
        // A scenario run whose trace read takes the artifact store lock.
        let r = write(&app, "/api/scenarios/run", Some(json!({"scope": []}))).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let session = create_session(&app, json!(["req:a"])).await.json()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        // While the provider runs, the workflow, revision and artifact stores stay available.
        *app.provider.probe.lock().unwrap() = Some((
            app.router.clone(),
            vec![
                format!("/api/sessions/{session}"),
                "/api/scenarios/scn:pass/run".to_owned(),
            ],
        ));
        for text in ["Employees request leave.", "Managers approve leave."] {
            let r = turn(&app, &session, text).await;
            assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        }
        assert_eq!(
            app.provider.probed.lock().unwrap().clone(),
            Some(Some(vec![StatusCode::OK, StatusCode::OK]))
        );
        let received = app.provider.received.lock().unwrap().clone();
        assert_eq!(received.len(), 2);
        assert_ne!(received[0].0.id, received[1].0.id);
        let artifacts = SqliteArtifactStore::open(&app.path).unwrap();
        for (request, material) in &received {
            assert_eq!(material.request_hash, request.id);
            material.validate_for(request).unwrap();
            assert!(!material.prompt_template.is_empty());
            let context: Value = serde_json::from_slice(&material.context_json).unwrap();
            assert!(context.is_object());
            // persist_inference_bundle stored the very request the provider executed.
            let stored = artifacts
                .get(&Hash::content_sha256(&to_canonical_json(request).unwrap()))
                .unwrap();
            assert_eq!(stored.kind, ArtifactKind::InferenceRequest);
            let persisted: InferenceRequest = serde_json::from_slice(&stored.bytes).unwrap();
            assert_eq!(&persisted, request);
        }
        // Each request carries its own material; crossing them is rejected before any work.
        assert!(validate_binding(&received[0].0, &received[1].1).is_err());
        // The transcript semantics are unchanged: two committed turns.
        let turns = workflow(&app).turns(&id(&session)).unwrap();
        assert_eq!(turns.len(), 2);
        assert!(turns
            .iter()
            .all(|t| t.state == WorkflowTurnState::Committed));
    }

    #[tokio::test]
    async fn http_workflow_interrupted_turns_are_reconciled() {
        let app = app();
        let session = create_session(&app, json!(["req:a"])).await.json()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            turn(&app, &session, "First turn.").await.status,
            StatusCode::OK
        );
        let committed = workflow(&app).turns(&id(&session)).unwrap()[0].clone();
        let loaded = head(&app);
        // Prepared turns left behind by a crash, reconciled on the next access.
        let prepare = |expected: Vec<Id>| {
            let mut wf = workflow(&app);
            wf.prepare_turn(
                &plumb_store::NewWorkflowTurn {
                    session_id: id(&session),
                    base_revision_id: loaded.revision.id.clone(),
                    stage: "S1".into(),
                    expected_node_refs: expected,
                    proposal_ids: vec![],
                    requires_reason_count: 0,
                    created_at: ts(NOW),
                },
                &[],
            )
            .unwrap()
        };
        // All expected transcript nodes present: promoted to committed on the current head.
        let present = prepare(committed.expected_node_refs.clone());
        // None present: failed.
        let absent = prepare(vec![id("evd:00000000000000aa"), id("src:00000000000000aa")]);
        let app = restarted(app, ACTOR);
        assert_eq!(
            get(&app, &format!("/api/sessions/{session}")).await.status,
            StatusCode::OK
        );
        let turns = workflow(&app).turns(&id(&session)).unwrap();
        assert_eq!(
            turns[present.turn_index as usize].state,
            WorkflowTurnState::Committed
        );
        assert_eq!(
            turns[present.turn_index as usize]
                .committed_revision_id
                .as_ref(),
            Some(&head(&app).revision.id)
        );
        assert_eq!(
            turns[absent.turn_index as usize].state,
            WorkflowTurnState::Failed
        );
        assert_eq!(
            get(&app, &format!("/api/sessions/{session}/metrics"))
                .await
                .json()["turns"],
            2
        );
        // A partial set is corrupt workflow state.
        let mut partial = committed.expected_node_refs.clone();
        partial.truncate(1);
        partial.push(id("src:00000000000000bb"));
        partial.sort();
        workflow(&app)
            .prepare_turn(
                &plumb_store::NewWorkflowTurn {
                    session_id: id(&session),
                    base_revision_id: loaded.revision.id.clone(),
                    stage: "S1".into(),
                    expected_node_refs: partial,
                    proposal_ids: vec![],
                    requires_reason_count: 0,
                    created_at: ts(NOW),
                },
                &[],
            )
            .unwrap();
        assert_error(
            &get(&app, &format!("/api/sessions/{session}")).await,
            StatusCode::INTERNAL_SERVER_ERROR,
            "E_WORKFLOW_CORRUPT",
        );
    }

    /// Fable #368: import artifacts are persisted in the revision-commit transaction, so an
    /// import whose CAS is lost leaves no unreferenced original or extracted artifact.
    #[tokio::test]
    async fn lost_import_commit_persists_no_artifacts() {
        let app = app();
        let stale = app.state.head().unwrap();
        concurrent_commit(&app.path);
        let bytes = b"# Leave\n\nEmployees shall record their overtime.\n";
        let err = app
            .state
            .create_source(&stale, "overtime.md", "text/markdown", bytes)
            .unwrap_err();
        assert_eq!(err.body().code, "E_PRECONDITION_FAILED");
        let artifacts = SqliteArtifactStore::open(&app.path).unwrap();
        assert!(!artifacts.exists(&Hash::content_sha256(bytes)).unwrap());
        // The committed import stores them with its revision.
        let loaded = app.state.head().unwrap();
        app.state
            .create_source(&loaded, "overtime.md", "text/markdown", bytes)
            .unwrap();
        assert!(artifacts.exists(&Hash::content_sha256(bytes)).unwrap());
    }

    /// H0.10 (§33.11): a turn whose transcript CAS committed before a crash is recovered onto
    /// the revision that CAS produced, never onto the head current at read time, and its
    /// proposals stay bound to that revision.
    #[tokio::test]
    async fn recovered_turn_keeps_its_base_revision() {
        let app = app();
        let session = create_session(&app, json!(["req:a"])).await.json()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let base = head(&app).revision.id;
        app.provider
            .set(ProviderMode::Propose, Some(&semantic(&app).await));
        let r = turn(&app, &session, "Employees should see their balance.").await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let transcript_revision = head(&app).revision.id;
        let turn_record = workflow(&app).turns(&id(&session)).unwrap()[0].clone();
        assert_eq!(turn_record.proposal_ids.len(), 1);
        let proposal_id = turn_record.proposal_ids[0].clone();
        // Rewind the workflow rows to the instant after the transcript CAS (the crash point):
        // the turn and its proposal are still prepared on the turn base.
        {
            let conn = rusqlite::Connection::open(&app.path).unwrap();
            conn.execute(
                "UPDATE workflow_session_turns SET state = 'prepared', \
                 committed_revision_id = NULL WHERE session_id = ?1",
                [session.as_str()],
            )
            .unwrap();
            conn.execute(
                "UPDATE workflow_proposals SET state = 'prepared', base_revision_id = ?1 \
                 WHERE proposal_id = ?2",
                [base.as_str(), proposal_id.as_str()],
            )
            .unwrap();
        }
        // Another writer moves the head before anyone reads the session again.
        concurrent_commit(&app.path);
        let moved = head(&app).revision.id;
        assert_ne!(moved, transcript_revision);
        let app = restarted(app, ACTOR);
        assert_eq!(
            get(&app, &format!("/api/sessions/{session}")).await.status,
            StatusCode::OK
        );
        let recovered = workflow(&app).turns(&id(&session)).unwrap()[0].clone();
        assert_eq!(recovered.state, WorkflowTurnState::Committed);
        assert_eq!(
            recovered.committed_revision_id.as_ref(),
            Some(&transcript_revision),
            "the turn records the revision its transcript CAS produced"
        );
        let stored = workflow(&app).proposal(&proposal_id).unwrap().unwrap();
        assert_eq!(stored.state, WorkflowProposalState::Pending);
        assert_eq!(stored.base_revision_id, transcript_revision);
        // Never rebound to the moved head: the proposal is not acceptable there.
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{proposal_id}/create"),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_BASELINE_CHANGED",
        );
    }

    // ------------------------------------------------------------------ scenarios

    fn run_ref(scenario: &str) -> Id {
        plumb_sim::scenario_run_id(&id(PROJECT), &id(scenario)).unwrap()
    }

    /// Commits `patch` directly to the store (another writer).
    fn direct_commit(app: &App, patch: SemanticPatch) {
        let mut store = SqliteRevisionStore::open(&app.path).unwrap();
        let head = store.head(&BranchName::main()).unwrap().unwrap();
        let loaded = store.load_revision(&head).unwrap();
        store
            .commit(
                &BranchName::main(),
                &head,
                &PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch,
                },
                RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id("agent:other"),
                    created_at: ts(NOW),
                },
            )
            .unwrap();
    }

    fn link_of(app: &App, scenario: &str) -> Value {
        head(app)
            .graph
            .node(&run_ref(scenario))
            .unwrap()
            .extensions
            .iter()
            .find(|(k, _)| k.as_str() == TRACE_LINK)
            .map(|(_, v)| v.clone())
            .unwrap()
    }

    #[tokio::test]
    async fn http_workflow_scenario_projection_and_lifecycle() {
        let app = app();
        let r = get(&app, "/api/scenarios").await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let items = r.json()["items"].as_array().unwrap().clone();
        let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            ["scn:derived-a", "scn:derived-b", "scn:fail", "scn:pass"]
        );
        assert_eq!(
            items[3],
            json!({
                "id": "scn:pass",
                "given": [r#"{"states_active":["state:draft"]}"#],
                "when": r#"{"operation":"op:submit"}"#,
                "then": [r#"{"states_active":["state:submitted"]}"#],
                "status": "Accepted", "confirmation_status": "Confirmed",
                "derived_from": ["trans:submit"], "run": null
            })
        );
        let derived = get(&app, "/api/scenarios?derived=true").await.json();
        assert_eq!(derived["items"].as_array().unwrap().len(), 2);
        let accepted = get(&app, "/api/scenarios?status=Accepted&requirement=req:a")
            .await
            .json();
        assert_eq!(accepted["items"].as_array().unwrap().len(), 2);
        assert_eq!(
            get(&app, "/api/scenarios?requirement=req:b").await.json()["items"],
            json!([])
        );
        assert_error(
            &get(&app, "/api/scenarios?derived=yes").await,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
        );
        let page = get(&app, "/api/scenarios?limit=3").await.json();
        let rest = get(
            &app,
            &format!(
                "/api/scenarios?limit=3&cursor={}",
                page["next_cursor"].as_str().unwrap()
            ),
        )
        .await
        .json();
        assert_eq!(rest["items"][0]["id"], "scn:pass");
        assert_eq!(rest["next_cursor"], Value::Null);

        // Confirmation: one proposal, Accepted + Confirmed; never twice.
        assert_error(
            &write(&app, "/api/scenarios/scn:pass/confirm", None).await,
            StatusCode::CONFLICT,
            "E_SCENARIO_NOT_CONFIRMABLE",
        );
        let r = write(&app, "/api/scenarios/scn:derived-a/confirm", None).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let a = get(&app, "/api/scenarios?derived=false").await.json();
        assert!(a["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == "scn:derived-a" && i["status"] == "Accepted"));
        // Bulk confirmation of one table's remaining derived scenarios; never derives.
        assert_error(
            &write(
                &app,
                "/api/scenarios/confirm-derived",
                Some(json!({"table_id": "dt:ghost"})),
            )
            .await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        assert_error(
            &write(
                &app,
                "/api/scenarios/confirm-derived",
                Some(json!({"table_id": "op:submit"})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_DECISION_TABLE_INVALID",
        );
        let r = write(
            &app,
            "/api/scenarios/confirm-derived",
            Some(json!({"table_id": "dt:eligibility"})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(
            get(&app, "/api/scenarios?derived=true").await.json()["items"],
            json!([])
        );
        assert_error(
            &write(
                &app,
                "/api/scenarios/confirm-derived",
                Some(json!({"table_id": "dt:eligibility"})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_NO_CONFIRMABLE_DERIVED_SCENARIOS",
        );
        // Correction changes only Then and keeps the lifecycle.
        for bad in [
            json!([r#"{ "states_active":["state:draft"]}"#]),
            json!(["not json"]),
        ] {
            assert_error(
                &write(
                    &app,
                    "/api/scenarios/scn:fail/correct",
                    Some(json!({"then": bad})),
                )
                .await,
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            );
        }
        assert_error(
            &write(
                &app,
                "/api/scenarios/scn:fail/correct",
                Some(json!({"then": [r#"{"states_active":["state:draft"]}"#]})),
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_NO_CHANGE",
        );
        assert_error(
            &write(
                &app,
                "/api/scenarios/scn:fail/correct",
                Some(json!({"then": [r#"{"states_active":["state:ghost"]}"#]})),
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_SEMANTIC_INVARIANT",
        );
        assert_error(
            &write(
                &app,
                "/api/scenarios/req:a/correct",
                Some(json!({"then": []})),
            )
            .await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
    }

    /// The fixture without the malformed decision table, so the simulator can run.
    fn run_app() -> App {
        app_with(fixture_graph_without(&["dt:broken"]), |_| Vec::new())
    }

    #[tokio::test]
    async fn http_workflow_scenario_runs_link_their_trace_atomically() {
        // A malformed Accepted decision table blocks execution: never skipped.
        let broken = app();
        assert_error(
            &write(&broken, "/api/scenarios/run", Some(json!({"scope": []}))).await,
            StatusCode::CONFLICT,
            "E_DECISION_TABLE_ENCODING_INVALID",
        );
        let app = run_app();
        for (scope, status, code) in [
            (
                json!(["scn:pass", "scn:fail"]),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                json!(["scn:fail", "scn:fail"]),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (json!(["scn:ghost"]), StatusCode::NOT_FOUND, "E_NOT_FOUND"),
            (
                json!(["scn:derived-a"]),
                StatusCode::CONFLICT,
                "E_SCENARIO_NOT_RUNNABLE",
            ),
        ] {
            assert_error(
                &write(&app, "/api/scenarios/run", Some(json!({"scope": scope}))).await,
                status,
                code,
            );
        }
        assert_error(
            &get(&app, "/api/scenarios/scn:pass/run").await,
            StatusCode::NOT_FOUND,
            "E_NOT_FOUND",
        );
        let semantic_before = semantic(&app).await;
        // First run of every runnable scenario: nodes added with payload and link.
        let r = write(&app, "/api/scenarios/run", Some(json!({"scope": []}))).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(r.json()["semantic_hash"], semantic_before);
        let first_revision = r.json()["revision"].as_str().unwrap().to_owned();
        let pass = get(&app, "/api/scenarios/scn:pass/run").await;
        assert_eq!(pass.status, StatusCode::OK, "{}", pass.text());
        let pass = pass.json();
        assert_eq!(pass["result"], "pass");
        assert_eq!(
            (pass["diff"].clone(), pass["missing"].clone()),
            (Value::Null, Value::Null)
        );
        assert!(!pass["trace"].as_array().unwrap().is_empty());
        let fail = get(&app, "/api/scenarios/scn:fail/run").await.json();
        assert_eq!(fail["result"], "fail");
        let diff = fail["diff"].as_array().unwrap();
        assert_eq!(diff.len(), 1);
        assert!(diff[0].as_str().unwrap().contains("missing_expected_state"));
        // The Scenario list carries the same run projection.
        let listed = get(&app, "/api/scenarios?limit=4").await.json();
        let listed_pass = listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == "scn:pass")
            .unwrap()
            .clone();
        assert_eq!(listed_pass["run"], pass);
        // The link names the persisted ScenarioTrace artifact.
        let link = link_of(&app, "scn:fail");
        let hash: Hash = link.as_str().unwrap().parse().unwrap();
        let artifact = SqliteArtifactStore::open(&app.path)
            .unwrap()
            .get(&hash)
            .unwrap();
        assert_eq!(artifact.kind, ArtifactKind::ScenarioTrace);
        assert_eq!(artifact.media_type, "application/json");
        // An identical re-run commits nothing.
        let r = write(
            &app,
            "/api/scenarios/run",
            Some(json!({"scope": ["scn:fail", "scn:pass"]})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(r.json()["revision"], first_revision);
        assert_eq!(r.json()["events"], json!([]));
        // Correcting Then changes the run result: payload and link change in one patch.
        let r = write(
            &app,
            "/api/scenarios/scn:fail/correct",
            Some(json!({"then": [r#"{"states_active":["state:submitted"]}"#]})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let r = write(
            &app,
            "/api/scenarios/run",
            Some(json!({"scope": ["scn:fail"]})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let artifacts = SqliteArtifactStore::open(&app.path).unwrap();
        let accepted = artifacts
            .get(head(&app).revision.accepted_patch_ref.as_ref().unwrap())
            .unwrap();
        let accepted: PatchSet = serde_json::from_slice(&accepted.bytes).unwrap();
        let SemanticPatch::Compound { patches } = &accepted.patch else {
            panic!()
        };
        assert!(matches!(&patches[0], SemanticPatch::ReplacePayload { .. }));
        assert!(matches!(
            &patches[1],
            SemanticPatch::SetNodeExtension { .. }
        ));
        let node = head(&app).graph.node(&run_ref("scn:fail")).unwrap().clone();
        assert_eq!(node.revision, 2);
        assert_ne!(link_of(&app, "scn:fail"), link);
        assert_eq!(
            get(&app, "/api/scenarios/scn:fail/run").await.json()["result"],
            "pass"
        );
    }

    #[tokio::test]
    async fn http_workflow_scenario_run_trace_integrity_is_never_repaired() {
        let app = run_app();
        assert_eq!(
            write(&app, "/api/scenarios/run", Some(json!({"scope": []})))
                .await
                .status,
            StatusCode::OK
        );
        let set_link = |app: &App, value: Value| {
            let node = head(app).graph.node(&run_ref("scn:pass")).unwrap().clone();
            direct_commit(
                app,
                SemanticPatch::SetNodeExtension {
                    target: plumb_patch::ElementPrecondition {
                        id: node.id.clone(),
                        expected_hash: plumb_psg::node_element_hash(&node).unwrap(),
                    },
                    key: TRACE_LINK.parse().unwrap(),
                    value,
                },
            )
        };
        set_link(&app, json!(7));
        assert_error(
            &get(&app, "/api/scenarios/scn:pass/run").await,
            StatusCode::CONFLICT,
            "E_SCENARIO_RUN_TRACE_LINK_INVALID",
        );
        set_link(&app, json!(format!("sha256:{}", "a".repeat(64))));
        assert_error(
            &get(&app, "/api/scenarios/scn:pass/run").await,
            StatusCode::CONFLICT,
            "E_SCENARIO_RUN_TRACE_ARTIFACT_MISSING",
        );
        let patch = SqliteArtifactStore::open(&app.path)
            .unwrap()
            .put(ArtifactKind::Patch, "application/json", b"{}", ts(NOW))
            .unwrap();
        set_link(&app, json!(patch.as_str()));
        assert_error(
            &get(&app, "/api/scenarios/scn:pass/run").await,
            StatusCode::CONFLICT,
            "E_SCENARIO_RUN_TRACE_ARTIFACT_INVALID",
        );
        // The trace of another scenario disagrees with this run.
        set_link(&app, link_of(&app, "scn:fail"));
        assert_error(
            &get(&app, "/api/scenarios/scn:pass/run").await,
            StatusCode::CONFLICT,
            "E_SCENARIO_RUN_TRACE_MISMATCH",
        );
        // The list fails the same way; nothing is fabricated.
        assert_error(
            &get(&app, "/api/scenarios").await,
            StatusCode::CONFLICT,
            "E_SCENARIO_RUN_TRACE_MISMATCH",
        );
        // A re-run repairs the link through a normal governed update.
        let r = write(
            &app,
            "/api/scenarios/run",
            Some(json!({"scope": ["scn:pass"]})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(
            get(&app, "/api/scenarios/scn:pass/run").await.json()["result"],
            "pass"
        );
    }

    #[tokio::test]
    async fn http_workflow_scenario_run_identity_collisions_and_empty_selection() {
        let app = run_app();
        direct_commit(
            &app,
            SemanticPatch::AddNode {
                node: requirement(run_ref("scn:pass").as_str(), "Proposed", "Not a run."),
            },
        );
        assert_error(
            &write(
                &app,
                "/api/scenarios/run",
                Some(json!({"scope": ["scn:pass"]})),
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_SCENARIO_RUN_COLLISION",
        );
        // Without runnable scenarios the empty scope is a conflict.
        let (nodes, edges) = base_parts();
        let nodes = nodes
            .into_iter()
            .filter(|n| !["scn:pass", "scn:fail", "dt:broken"].contains(&n.id.as_str()))
            .collect();
        let none = app_with(
            Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap(),
            |_| vec![],
        );
        assert_error(
            &write(&none, "/api/scenarios/run", Some(json!({"scope": []}))).await,
            StatusCode::CONFLICT,
            "E_NO_RUNNABLE_SCENARIOS",
        );
        // Rules page under cursor v2 when every table is well formed.
        let page = get(&none, "/api/model/rules?limit=1").await;
        assert_eq!(page.status, StatusCode::OK, "{}", page.text());
        let next = page.json()["next_cursor"].as_str().unwrap().to_owned();
        let rest = get(&none, &format!("/api/model/rules?limit=1&cursor={next}"))
            .await
            .json();
        assert_eq!(page.json()["items"][0]["id"], "dt:eligibility");
        assert_eq!(rest["items"][0]["id"], "rule:positive");
        assert_eq!(rest["next_cursor"], Value::Null);
    }

    // ------------------------------------------------------------------ runtime and If-Match

    #[tokio::test]
    async fn http_workflow_absent_runtime_is_a_typed_503() {
        let full = app();
        let state = ApiState::new(
            BranchName::main(),
            SqliteRevisionStore::open(&full.path).unwrap(),
            SqliteArtifactStore::open(&full.path).unwrap(),
            identity(ACTOR),
            fixed(),
            Vec::new(),
            profile(),
        )
        .unwrap();
        let core = App {
            _dir: tempfile::tempdir().unwrap(),
            path: full.path.clone(),
            state: state.clone(),
            router: router(state),
            provider: full.provider.clone(),
        };
        let question = question_about(&head(&core).graph, "op:approve");
        let proposal = requirement_proposal(&core, "req:core").await;
        let cases: Vec<(Method, String, Option<Value>)> = vec![
            (
                Method::POST,
                "/api/proposals/intake".into(),
                Some(json!({"kind": "create", "payload": proposal, "evidence": []})),
            ),
            (
                Method::POST,
                format!("/api/questions/{question}/answer"),
                Some(
                    json!({"answer": {"performer_ref": "actor:clerk"}, "rationale": null, "supersedes": null}),
                ),
            ),
            (
                Method::POST,
                "/api/proposals/prop:0000000000000000/accept".into(),
                Some(json!({"reason": null})),
            ),
            (
                Method::POST,
                "/api/proposals/prop:0000000000000000/reject".into(),
                Some(json!({"reason": null})),
            ),
            (
                Method::POST,
                "/api/sessions".into(),
                Some(json!({"scope": ["req:a"]})),
            ),
            (
                Method::POST,
                "/api/sessions/ses:0000000000000000/turns".into(),
                Some(json!({"text": "x", "stage": "S1"})),
            ),
            (
                Method::GET,
                "/api/sessions/ses:0000000000000000".into(),
                None,
            ),
            (
                Method::GET,
                "/api/sessions/ses:0000000000000000/metrics".into(),
                None,
            ),
            (
                Method::GET,
                "/api/model/calculations/calc:double/example".into(),
                None,
            ),
            (
                Method::POST,
                "/api/scenarios/run".into(),
                Some(json!({"scope": []})),
            ),
        ];
        for (method, uri, body) in cases {
            let r = if method == Method::GET {
                get(&core, &uri).await
            } else {
                write(&core, &uri, body).await
            };
            assert_error(
                &r,
                StatusCode::SERVICE_UNAVAILABLE,
                "E_A03_RUNTIME_UNAVAILABLE",
            );
            assert!(
                r.json()["details"]["capability"]
                    .as_str()
                    .is_some_and(|c| !c.is_empty()),
                "{uri}"
            );
        }
        // Operations that need no runtime work normally on the core state.
        assert_eq!(
            get(&core, "/api/search?q=leave").await.status,
            StatusCode::OK
        );
        assert_eq!(get(&core, "/api/questions").await.status, StatusCode::OK);
        assert_eq!(
            write(&core, "/api/scenarios/scn:derived-a/confirm", None)
                .await
                .status,
            StatusCode::OK
        );
        // A duplicate expression-scope owner is a construction error of the full runtime.
        let mut config = runtime(&full.path, full.provider.clone());
        let scope: plumb_sim::ExecutionExpressionScopeInput =
            serde_json::from_value(json!({"owner_ref": "rule:positive", "bindings": []})).unwrap();
        config.expression_scopes = vec![scope.clone(), scope];
        let duplicate = ApiState::new_with_runtime(
            BranchName::main(),
            SqliteRevisionStore::open(&full.path).unwrap(),
            SqliteArtifactStore::open(&full.path).unwrap(),
            identity(ACTOR),
            fixed(),
            Vec::new(),
            profile(),
            config,
        );
        assert!(
            matches!(duplicate, Err(plumb_api::ApiStateError::DuplicateExpressionScope(owner)) if owner == id("rule:positive"))
        );
    }

    /// A request-valid body for every A0.3 POST, so If-Match is what decides.
    fn post_cases(question: &Id) -> Vec<(&'static str, String, Option<Value>)> {
        vec![
            (
                "split_requirement",
                "/api/requirements/req:c/split".into(),
                Some(json!({"parts": ["A.", "B."], "reason": "x"})),
            ),
            (
                "waive_requirement_finding",
                "/api/requirements/req:b/waive-finding".into(),
                Some(json!({"finding_id": CRITERIA, "reason": "x"})),
            ),
            (
                "mark_requirement_duplicate",
                "/api/requirements/req:b/mark-duplicate".into(),
                Some(json!({"of": "req:a", "reason": "x"})),
            ),
            (
                "merge_glossary",
                "/api/glossary/term:vacation/merge".into(),
                Some(json!({"into": "term:leave", "reason": "x"})),
            ),
            (
                "accept_sentence",
                "/api/model/sentences/req:p/accept".into(),
                None,
            ),
            (
                "edit_sentence",
                "/api/model/sentences/req:a/edit".into(),
                Some(json!({"slots": {"class": null, "level": null, "modality": "may"}})),
            ),
            (
                "reject_sentence",
                "/api/model/sentences/req:p/reject".into(),
                None,
            ),
            (
                "set_rule_cell",
                "/api/model/rules/dt:eligibility/cells".into(),
                Some(
                    json!({"cell": "row:0:output:0", "outcome": r#"{"cell":"enum","member":"Director"}"#, "reason": "x"}),
                ),
            ),
            (
                "mutate_process_steps",
                "/api/processes/proc:branched/steps".into(),
                Some(json!({"op": "remove", "node_ref": "pn:b-gate"})),
            ),
            (
                "import_process",
                "/api/processes/import".into(),
                Some(json!({"format": "mermaid", "text": mermaid("Leave Flow", &["op:submit"])})),
            ),
            (
                "confirm_scenario",
                "/api/scenarios/scn:derived-a/confirm".into(),
                None,
            ),
            (
                "correct_scenario",
                "/api/scenarios/scn:fail/correct".into(),
                Some(json!({"then": [r#"{"states_active":["state:submitted"]}"#]})),
            ),
            (
                "confirm_derived_scenarios",
                "/api/scenarios/confirm-derived".into(),
                Some(json!({"table_id": "dt:eligibility"})),
            ),
            (
                "run_scenarios",
                "/api/scenarios/run".into(),
                Some(json!({"scope": []})),
            ),
            (
                "waive_finding",
                format!("/api/findings/{CRITERIA}/waive"),
                Some(json!({"reason": "x"})),
            ),
            (
                "resolve_assumption",
                "/api/assumptions/assumption:active/resolve".into(),
                Some(json!({"reason": "x"})),
            ),
            (
                "answer_question",
                format!("/api/questions/{question}/answer"),
                Some(
                    json!({"answer": {"performer_ref": "actor:clerk"}, "rationale": null, "supersedes": null}),
                ),
            ),
            (
                "create_round",
                "/api/rounds".into(),
                Some(json!({"stakeholder": "stakeholder:hr", "max": 1})),
            ),
            (
                "intake_proposal",
                "/api/proposals/intake".into(),
                Some(json!({"kind": "create", "payload": {}, "evidence": []})),
            ),
            (
                "link_proposal",
                "/api/proposals/prop:0000000000000000/link".into(),
                Some(json!({"reason": null})),
            ),
            (
                "merge_proposal",
                "/api/proposals/prop:0000000000000000/merge".into(),
                Some(json!({"reason": null})),
            ),
            (
                "supersede_proposal",
                "/api/proposals/prop:0000000000000000/supersede".into(),
                Some(json!({"reason": null})),
            ),
            (
                "create_proposal",
                "/api/proposals/prop:0000000000000000/create".into(),
                Some(json!({"reason": null})),
            ),
            (
                "reject_proposal",
                "/api/proposals/prop:0000000000000000/reject".into(),
                Some(json!({"reason": null})),
            ),
            (
                "accept_proposal",
                "/api/proposals/prop:0000000000000000/accept".into(),
                Some(json!({"reason": null})),
            ),
            (
                "create_session",
                "/api/sessions".into(),
                Some(json!({"scope": ["req:a"]})),
            ),
            (
                "create_session_turn",
                "/api/sessions/ses:0000000000000000/turns".into(),
                Some(json!({"text": "x", "stage": "S1"})),
            ),
        ]
    }

    #[tokio::test]
    async fn http_workflow_every_a03_post_requires_the_current_if_match() {
        let app = waiver_app();
        let question = question_about(&head(&app).graph, "op:approve");
        let cases = post_cases(&question);
        let posts: Vec<&str> = OPERATIONS
            .iter()
            .filter(|s| s.owner == "A0.3" && s.method == "POST")
            .map(|s| s.operation_id)
            .collect();
        assert_eq!(posts.len(), 27);
        let mut covered: Vec<&str> = cases.iter().map(|c| c.0).collect();
        covered.sort();
        let mut expected = posts.clone();
        expected.sort();
        assert_eq!(covered, expected);
        let stale = "psg:sha256:0000000000000000000000000000000000000000000000000000000000000000";
        let current_revision = revision(&app).await;
        for (operation, uri, body) in &cases {
            // intake_proposal decodes its payload first; give it a real one.
            let body = if *operation == "intake_proposal" {
                Some(
                    json!({"kind": "create", "payload": requirement_proposal(&app, "req:if-match").await, "evidence": []}),
                )
            } else {
                body.clone()
            };
            let r = post(&app, uri, Some(stale), body.clone()).await;
            assert_error(&r, StatusCode::PRECONDITION_FAILED, "E_PRECONDITION_FAILED");
            assert_eq!(r.json()["revision"], current_revision, "{operation}");
            let missing = post(&app, uri, None, body).await;
            assert_error(&missing, StatusCode::BAD_REQUEST, "E_INVALID_REQUEST");
        }
        // Nothing was written by any rejected request.
        assert_eq!(revision(&app).await, current_revision);
    }

    #[tokio::test]
    async fn http_workflow_question_pages_follow_cursor_v2() {
        let app = app();
        let all = get(&app, "/api/questions").await.json()["items"]
            .as_array()
            .unwrap()
            .clone();
        assert!(all.len() >= 2);
        let first = get(&app, "/api/questions?limit=1").await.json();
        assert_eq!(first["items"][0], all[0]);
        let next = first["next_cursor"].as_str().unwrap();
        let second = get(&app, &format!("/api/questions?limit=1&cursor={next}"))
            .await
            .json();
        assert_eq!(second["items"][0], all[1]);
        // A cursor of one filter set is refused for another.
        let other = get(
            &app,
            &format!("/api/questions?status=Open&limit=1&cursor={next}"),
        )
        .await;
        assert_error(&other, StatusCode::BAD_REQUEST, "E_CURSOR_REQUEST_MISMATCH");
        // After the head moves, the old cursor names another baseline.
        let r = write(
            &app,
            "/api/rounds",
            Some(json!({"stakeholder": "stakeholder:hr", "max": 1})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK);
        let moved = get(&app, &format!("/api/questions?limit=1&cursor={next}")).await;
        assert_error(&moved, StatusCode::CONFLICT, "E_CURSOR_BASELINE_CHANGED");
    }

    // ------------------------------------------------------------------ action classes

    fn pre(graph: &Graph, node_id: &str) -> plumb_patch::ElementPrecondition {
        plumb_patch::ElementPrecondition {
            id: id(node_id),
            expected_hash: plumb_psg::node_element_hash(graph.node(&id(node_id)).unwrap()).unwrap(),
        }
    }

    async fn typed(
        app: &App,
        patch: SemanticPatch,
        materiality: ProposalMateriality,
        policy: AcceptancePolicy,
    ) -> Proposal {
        Proposal::new(
            plumb_core::StageId::S1,
            PatchSet {
                base_semantic_hash: semantic(app).await.parse().unwrap(),
                patch,
            },
            vec![],
            vec![],
            materiality,
            policy,
            None,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn http_workflow_link_merge_and_supersede_actions_commit_unchanged_proposals() {
        let app = app();
        let g = head(&app).graph;
        let refines = edge("rel:refines-b-a", "refines", "req:b", "req:a");
        let link = typed(
            &app,
            SemanticPatch::AddEdge { edge: refines },
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
        )
        .await;
        let merge = typed(
            &app,
            SemanticPatch::MergeNodes {
                keep: pre(&g, "term:leave"),
                merge: vec![pre(&g, "term:vacation")],
                field_policy: plumb_patch::MergePolicy::KeepPayloadUnionMetadata,
            },
            ProposalMateriality::MaterialDecision,
            AcceptancePolicy::HumanDecision,
        )
        .await;
        let mut supersedes = edge("rel:sup-c-b", "supersedes", "req:c", "req:b");
        supersedes.audit = plumb_psg::AuditMeta::new(id(ACTOR), ts(NOW), None, None).unwrap();
        let supersede_patch = SemanticPatch::Supersede {
            old: pre(&g, "req:b"),
            new: pre(&g, "req:c"),
            edge: supersedes,
        };
        let supersede = typed(
            &app,
            supersede_patch.clone(),
            ProposalMateriality::MaterialDecision,
            AcceptancePolicy::HumanDecision,
        )
        .await;
        // Mixed special classes are ambiguous.
        let ambiguous = typed(
            &app,
            SemanticPatch::Compound {
                patches: vec![
                    supersede_patch,
                    SemanticPatch::MergeNodes {
                        keep: pre(&g, "term:leave"),
                        merge: vec![pre(&g, "term:vacation")],
                        field_policy: plumb_patch::MergePolicy::KeepPayloadUnionMetadata,
                    },
                ],
            },
            ProposalMateriality::MaterialDecision,
            AcceptancePolicy::HumanDecision,
        )
        .await;
        assert_error(
            &intake(&app, "supersede", &ambiguous).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "E_PROPOSAL_ACTION_AMBIGUOUS",
        );
        for (kind, proposal) in [
            ("link", &link),
            ("merge", &merge),
            ("supersede", &supersede),
        ] {
            let r = intake(&app, kind, proposal).await;
            assert_eq!(r.status, StatusCode::OK, "{kind}: {}", r.text());
        }
        // Each action requires its own class; a material proposal requires a reason.
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{}/create", link.id),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_ACTION_MISMATCH",
        );
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{}/merge", merge.id),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_INTAKE_REASON_REQUIRED",
        );
        let r = write(
            &app,
            &format!("/api/proposals/{}/link", link.id),
            Some(json!({"reason": null})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        // The head moved: the other stored proposals are bound to the old revision.
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{}/merge", merge.id),
                Some(json!({"reason": "Same term."})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_BASELINE_CHANGED",
        );
        let merge = typed(
            &app,
            SemanticPatch::MergeNodes {
                keep: pre(&head(&app).graph, "term:leave"),
                merge: vec![pre(&head(&app).graph, "term:vacation")],
                field_policy: plumb_patch::MergePolicy::KeepPayloadUnionMetadata,
            },
            ProposalMateriality::MaterialDecision,
            AcceptancePolicy::HumanDecision,
        )
        .await;
        assert_eq!(intake(&app, "merge", &merge).await.status, StatusCode::OK);
        let r = write(
            &app,
            &format!("/api/proposals/{}/merge", merge.id),
            Some(json!({"reason": "Same term."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert!(head(&app).graph.node(&id("term:vacation")).is_none());
        let g = head(&app).graph;
        let mut supersedes = edge("rel:sup-c-b", "supersedes", "req:c", "req:b");
        supersedes.audit = plumb_psg::AuditMeta::new(id(ACTOR), ts(NOW), None, None).unwrap();
        let supersede = typed(
            &app,
            SemanticPatch::Supersede {
                old: pre(&g, "req:b"),
                new: pre(&g, "req:c"),
                edge: supersedes,
            },
            ProposalMateriality::MaterialDecision,
            AcceptancePolicy::HumanDecision,
        )
        .await;
        assert_eq!(
            intake(&app, "supersede", &supersede).await.status,
            StatusCode::OK
        );
        let r = write(
            &app,
            &format!("/api/proposals/{}/supersede", supersede.id),
            Some(json!({"reason": "Replaced."})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        let g = head(&app).graph;
        assert_eq!(
            g.node(&id("req:b")).unwrap().status,
            ElementStatus::Superseded
        );
        // Every committed action recorded exactly its own companion decision.
        let companions = acceptance_decisions(&g);
        assert_eq!(companions.len(), 3);
        let proposals: Vec<Id> = companions
            .iter()
            .map(|(_, d)| d.proposal_ref.clone().unwrap())
            .collect();
        for p in [&link.id, &merge.id, &supersede.id] {
            assert!(proposals.contains(p));
            assert_eq!(
                workflow(&app).proposal(p).unwrap().unwrap().state,
                WorkflowProposalState::Accepted
            );
        }
        // Rejecting a Proposed sentence.
        let r = write(&app, "/api/model/sentences/req:p/reject", None).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_eq!(
            head(&app).graph.node(&id("req:p")).unwrap().status,
            ElementStatus::Rejected
        );
    }

    // ------------------------------------------------------------------ R0.3 proposal sets

    use plumb_api::application::ProposalAction;

    /// Registers `proposal` pending on the head through the intake endpoint.
    async fn pending_on_head(app: &App, kind: &str, proposal: Proposal) -> Proposal {
        let r = intake(app, kind, &proposal).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        proposal
    }

    async fn create(app: &App, node_id: &str, statement: &str) -> Proposal {
        let p = typed(
            app,
            SemanticPatch::AddNode {
                node: requirement(node_id, "Proposed", statement),
            },
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
        )
        .await;
        pending_on_head(app, "create", p).await
    }

    fn accept_set(
        app: &App,
        members: &[(&Proposal, ProposalAction)],
        reason: Option<&str>,
    ) -> Result<plumb_api::openapi::WriteResult, plumb_api::error::ApiError> {
        let members: Vec<(String, ProposalAction)> = members
            .iter()
            .map(|(p, a)| (p.id.to_string(), *a))
            .collect();
        app.state
            .accept_proposal_set(&head(app), &members, reason.map(str::to_owned))
    }

    fn state_of(app: &App, p: &Proposal) -> WorkflowProposalState {
        workflow(app).proposal(&p.id).unwrap().unwrap().state
    }

    fn set_error(
        result: Result<plumb_api::openapi::WriteResult, plumb_api::error::ApiError>,
        code: &str,
    ) -> Value {
        match result {
            Err(error) => {
                assert_eq!(error.body().code, code, "{:?}", error.body());
                error.body().details.clone()
            }
            Ok(write) => panic!("expected {code}, got {write:?}"),
        }
    }

    /// The committed PatchSet of the head revision.
    fn accepted_patch_set(app: &App) -> PatchSet {
        let patch_ref = head(app).revision.accepted_patch_ref.unwrap();
        let artifact = SqliteArtifactStore::open(&app.path)
            .unwrap()
            .get(&patch_ref)
            .unwrap();
        serde_json::from_slice(&artifact.bytes).unwrap()
    }

    fn decision_of(patch: &SemanticPatch) -> (Id, plumb_psg::ResolutionDecision) {
        match patch {
            SemanticPatch::AddNode { node } => match &node.payload {
                NodePayload::ResolutionDecision(d) => (node.id.clone(), d.clone()),
                other => panic!("not a decision: {other:?}"),
            },
            other => panic!("not an AddNode: {other:?}"),
        }
    }

    #[tokio::test]
    async fn accept_proposal_set_commits_one_revision_with_original_patches_and_companions() {
        let app = app();
        let graph = head(&app).graph;
        let a = create(&app, "req:n1", "Payroll shall export leave totals.").await;
        // A nested member Compound stays one unchanged outer child.
        let nested = pending_on_head(
            &app,
            "create",
            typed(
                &app,
                SemanticPatch::Compound {
                    patches: vec![
                        SemanticPatch::AddNode {
                            node: requirement("req:n2", "Proposed", "HR shall audit leave."),
                        },
                        SemanticPatch::AddNode {
                            node: requirement("req:n3", "Proposed", "HR shall archive leave."),
                        },
                    ],
                },
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
            )
            .await,
        )
        .await;
        let accept = pending_on_head(
            &app,
            "accept",
            typed(
                &app,
                SemanticPatch::SetStatus {
                    target: pre(&graph, "req:p"),
                    from: ElementStatus::Proposed,
                    to: ElementStatus::Accepted,
                },
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
            )
            .await,
        )
        .await;
        let originals: Vec<String> = [&a, &nested, &accept]
            .iter()
            .map(|p| {
                workflow(&app)
                    .proposal(&p.id)
                    .unwrap()
                    .unwrap()
                    .proposal_json
            })
            .collect();
        let before = head(&app).revision;
        // Caller order is deliberately not proposal-ID order.
        let mut caller = vec![
            (&accept, ProposalAction::Accept),
            (&a, ProposalAction::Create),
            (&nested, ProposalAction::Create),
        ];
        caller.sort_by(|x, y| y.0.id.cmp(&x.0.id));
        let write = accept_set(&app, &caller, Some("Batch review.")).unwrap();
        let after = head(&app).revision;
        // Exactly one new revision, a child of the loaded head.
        assert_eq!(after.version, before.version + 1);
        assert_eq!(after.parent.as_ref(), Some(&before.id));
        assert_eq!(write.revision, after.id.to_string());
        assert_eq!(write.events.len(), 1);
        assert_eq!(write.events[0].kind, "accept_proposal_set");
        // Originals first in ascending proposal-ID order, then one companion each.
        let mut ordered = [&a, &nested, &accept];
        ordered.sort_by(|x, y| x.id.cmp(&y.id));
        let set = accepted_patch_set(&app);
        assert_eq!(set.base_semantic_hash, before.semantic_hash);
        let SemanticPatch::Compound { patches } = &set.patch else {
            panic!("not a compound")
        };
        assert_eq!(patches.len(), 6);
        let artifacts = SqliteArtifactStore::open(&app.path).unwrap();
        for (i, p) in ordered.iter().enumerate() {
            assert_eq!(patches[i], p.patch_set.patch, "original {i} unchanged");
            let (decision_id, decision) = decision_of(&patches[3 + i]);
            assert_eq!(decision.proposal_ref.as_ref(), Some(&p.id));
            assert_eq!(decision.question_ref, None);
            assert_eq!(decision.answer["kind"], "proposal_acceptance");
            assert_eq!(decision.rationale.as_deref(), Some("Batch review."));
            assert_eq!(decision.decided_by, id(ACTOR));
            assert_eq!(decision.supersedes, None);
            // patch_ref is the member's own unchanged original PatchSet, persisted as a Patch.
            let bytes = to_canonical_json(&p.patch_set).unwrap();
            assert_eq!(decision.patch_ref, Hash::content_sha256(&bytes));
            let stored = artifacts.get(&decision.patch_ref).unwrap();
            assert_eq!(stored.kind, ArtifactKind::Patch);
            assert_eq!(stored.bytes, bytes);
            assert!(after.decision_refs.contains(&decision_id));
            assert!(head(&app).graph.node(&decision_id).is_some());
        }
        // One write instant for every companion.
        let instants: std::collections::BTreeSet<String> = (3..6)
            .map(|i| decision_of(&patches[i]).1.decided_at.to_string())
            .collect();
        assert_eq!(instants.len(), 1);
        // The nested member Compound is one outer child, unflattened.
        assert!(patches
            .iter()
            .take(3)
            .any(|p| matches!(p, SemanticPatch::Compound { patches } if patches.len() == 2)));
        // Every member is accepted on the one revision; stored proposals are unchanged.
        for (p, original) in [&a, &nested, &accept].iter().zip(&originals) {
            let record = workflow(&app).proposal(&p.id).unwrap().unwrap();
            assert_eq!(record.state, WorkflowProposalState::Accepted);
            assert_eq!(record.resolved_revision_id.as_ref(), Some(&after.id));
            assert_eq!(&record.proposal_json, original);
        }
        assert_eq!(
            head(&app).graph.node(&id("req:p")).unwrap().status,
            ElementStatus::Accepted
        );
    }

    #[tokio::test]
    async fn accept_proposal_set_records_decisions_introduced_by_originals() {
        let app = app();
        let introduced = node(
            "dec:00000000000000c1",
            "Accepted",
            "ResolutionDecision",
            json!({
                "question_ref": null, "proposal_ref": "prop:00000000000000c1",
                "answer": {"kind": "note"}, "decided_by": ACTOR, "decided_at": T0,
                "patch_ref": Hash::content_sha256(b"note"), "rationale": null, "supersedes": null
            }),
        );
        let with_decision = pending_on_head(
            &app,
            "create",
            typed(
                &app,
                SemanticPatch::Compound {
                    patches: vec![
                        SemanticPatch::AddNode {
                            node: requirement("req:n7", "Proposed", "HR shall note leave."),
                        },
                        SemanticPatch::AddNode { node: introduced },
                    ],
                },
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
            )
            .await,
        )
        .await;
        let other = create(&app, "req:n8", "HR shall export notes.").await;
        accept_set(
            &app,
            &[
                (&with_decision, ProposalAction::Create),
                (&other, ProposalAction::Create),
            ],
            None,
        )
        .unwrap();
        let refs = head(&app).revision.decision_refs;
        assert!(refs.contains(&id("dec:00000000000000c1")), "{refs:?}");
        assert_eq!(refs.len(), 3, "the introduced decision and two companions");
        assert!(refs.windows(2).all(|w| w[0] < w[1]));
    }

    #[tokio::test]
    async fn accept_proposal_set_conflict_is_atomic() {
        // Two patches touching the same node.
        let app = app();
        let ok = create(&app, "req:n1", "Payroll shall export leave totals.").await;
        let x = create(&app, "req:dup", "HR shall approve leave.").await;
        let y = create(&app, "req:dup", "HR shall reject leave.").await;
        let before = head(&app).revision;
        let details = set_error(
            accept_set(
                &app,
                &[
                    (&ok, ProposalAction::Create),
                    (&x, ProposalAction::Create),
                    (&y, ProposalAction::Create),
                ],
                None,
            ),
            "E_PROPOSAL_SET_CONFLICT",
        );
        let mut pair = vec![x.id.clone(), y.id.clone()];
        pair.sort();
        assert_eq!(
            details,
            json!({"reason": "overlapping_effects", "proposals": pair, "elements": ["req:dup"]})
        );
        assert_eq!(head(&app).revision.id, before.id);
        for p in [&ok, &x, &y] {
            assert_eq!(state_of(&app, p), WorkflowProposalState::Pending);
        }
        // Two patches touching the same edge.
        let edge_patch = |from: &str| SemanticPatch::AddEdge {
            edge: edge("rel:dup", "refines", from, "req:b"),
        };
        let e1 = pending_on_head(
            &app,
            "link",
            typed(
                &app,
                edge_patch("req:a"),
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
            )
            .await,
        )
        .await;
        let e2 = pending_on_head(
            &app,
            "link",
            typed(
                &app,
                edge_patch("req:c"),
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
            )
            .await,
        )
        .await;
        let details = set_error(
            accept_set(
                &app,
                &[(&e1, ProposalAction::Link), (&e2, ProposalAction::Link)],
                Some("Linking."),
            ),
            "E_PROPOSAL_SET_CONFLICT",
        );
        assert!(details["elements"]
            .as_array()
            .unwrap()
            .contains(&json!("rel:dup")));
        assert_eq!(head(&app).revision.id, before.id);
        assert_eq!(state_of(&app, &e1), WorkflowProposalState::Pending);
        // The compatible member alone still commits afterwards.
        accept_set(&app, &[(&ok, ProposalAction::Create)], None).unwrap();
        assert_eq!(state_of(&app, &ok), WorkflowProposalState::Accepted);
    }

    #[tokio::test]
    async fn accept_proposal_set_rejects_stale_or_misclassified_member() {
        let app = app();
        let stale = create(&app, "req:n1", "Payroll shall export leave totals.").await;
        direct_commit(
            &app,
            SemanticPatch::AddNode {
                node: requirement("req:other", "Proposed", "Another writer added this."),
            },
        );
        let fresh = create(&app, "req:n2", "HR shall audit leave.").await;
        let second = create(&app, "req:n3", "HR shall archive leave.").await;
        let before = head(&app).revision;
        let unchanged = |app: &App| {
            assert_eq!(head(app).revision.id, before.id);
            for p in [&fresh, &second] {
                assert_eq!(state_of(app, p), WorkflowProposalState::Pending);
            }
        };
        // One stale member rejects the whole set (never rebased).
        let details = set_error(
            accept_set(
                &app,
                &[
                    (&fresh, ProposalAction::Create),
                    (&stale, ProposalAction::Create),
                ],
                None,
            ),
            "E_PROPOSAL_BASELINE_CHANGED",
        );
        assert_eq!(details["proposal_ref"], json!(stale.id));
        unchanged(&app);
        assert_eq!(state_of(&app, &stale), WorkflowProposalState::Pending);
        // One misclassified action.
        set_error(
            accept_set(
                &app,
                &[
                    (&fresh, ProposalAction::Create),
                    (&second, ProposalAction::Accept),
                ],
                None,
            ),
            "E_PROPOSAL_ACTION_MISMATCH",
        );
        unchanged(&app);
        // Duplicate members, an empty set and reject are rejected before anything is read.
        set_error(
            accept_set(
                &app,
                &[
                    (&fresh, ProposalAction::Create),
                    (&fresh, ProposalAction::Create),
                ],
                None,
            ),
            "E_INVALID_REQUEST",
        );
        set_error(accept_set(&app, &[], None), "E_INVALID_REQUEST");
        set_error(
            accept_set(&app, &[(&fresh, ProposalAction::Reject)], None),
            "E_INVALID_REQUEST",
        );
        set_error(
            accept_set(&app, &[(&fresh, ProposalAction::Create)], Some(" padded ")),
            "E_INVALID_REQUEST",
        );
        unchanged(&app);
        // One reason-required member without a reason.
        let material = pending_on_head(
            &app,
            "create",
            typed(
                &app,
                SemanticPatch::AddNode {
                    node: requirement("req:m", "Proposed", "HR shall record material leave."),
                },
                ProposalMateriality::MaterialDecision,
                AcceptancePolicy::HumanDecision,
            )
            .await,
        )
        .await;
        set_error(
            accept_set(
                &app,
                &[
                    (&fresh, ProposalAction::Create),
                    (&material, ProposalAction::Create),
                ],
                None,
            ),
            "E_INTAKE_REASON_REQUIRED",
        );
        unchanged(&app);
        // One non-Pending member.
        app.state
            .act_on_proposal(
                &head(&app),
                second.id.as_str(),
                ProposalAction::Reject,
                None,
            )
            .unwrap();
        set_error(
            accept_set(
                &app,
                &[
                    (&fresh, ProposalAction::Create),
                    (&second, ProposalAction::Create),
                ],
                None,
            ),
            "E_PROPOSAL_NOT_PENDING",
        );
        assert_eq!(head(&app).revision.id, before.id);
        assert_eq!(state_of(&app, &fresh), WorkflowProposalState::Pending);
        // With the common reason both commit; the reason is every companion's rationale.
        accept_set(
            &app,
            &[
                (&material, ProposalAction::Create),
                (&fresh, ProposalAction::Create),
            ],
            Some("Approved by HR."),
        )
        .unwrap();
        let SemanticPatch::Compound { patches } = accepted_patch_set(&app).patch else {
            panic!()
        };
        for p in &patches[2..] {
            assert_eq!(
                decision_of(p).1.rationale.as_deref(),
                Some("Approved by HR.")
            );
        }
    }

    #[tokio::test]
    async fn accept_proposal_set_cas_race_changes_nothing_and_is_not_retried() {
        let app = app();
        let a = create(&app, "req:n1", "Payroll shall export leave totals.").await;
        let b = create(&app, "req:n2", "HR shall audit leave.").await;
        // Validation runs against the loaded head; another writer moves the branch first.
        let loaded = head(&app);
        direct_commit(
            &app,
            SemanticPatch::AddNode {
                node: requirement("req:other", "Proposed", "Another writer added this."),
            },
        );
        let moved = head(&app).revision;
        let members = vec![
            (a.id.to_string(), ProposalAction::Create),
            (b.id.to_string(), ProposalAction::Create),
        ];
        let error = app
            .state
            .accept_proposal_set(&loaded, &members, None)
            .unwrap_err();
        assert_eq!(error.body().code, "E_PRECONDITION_FAILED");
        assert_eq!(
            head(&app).revision.id,
            moved.id,
            "no retry, no second commit"
        );
        for p in [&a, &b] {
            assert_eq!(state_of(&app, p), WorkflowProposalState::Pending);
        }
    }

    #[tokio::test]
    async fn single_proposal_sibling_still_baseline_changed() {
        let app = app();
        let a = create(&app, "req:n1", "Payroll shall export leave totals.").await;
        let b = create(&app, "req:n2", "HR shall audit leave.").await;
        let r = write(
            &app,
            &format!("/api/proposals/{}/create", a.id),
            Some(json!({"reason": null})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
        assert_error(
            &write(
                &app,
                &format!("/api/proposals/{}/create", b.id),
                Some(json!({"reason": null})),
            )
            .await,
            StatusCode::CONFLICT,
            "E_PROPOSAL_BASELINE_CHANGED",
        );
        // The set path never rebases either.
        set_error(
            accept_set(&app, &[(&b, ProposalAction::Create)], None),
            "E_PROPOSAL_BASELINE_CHANGED",
        );
        assert_eq!(state_of(&app, &b), WorkflowProposalState::Pending);
    }
}
