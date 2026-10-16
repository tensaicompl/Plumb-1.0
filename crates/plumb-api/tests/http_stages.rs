//! A0.4 contract tests for stage orchestration (Hotfix 067, Hotfix 068).
//!
//! Every test lives in `http_stages` (`cargo test -p plumb-api http_stages`). The full A0.4
//! runtime is injected explicitly over a temporary SQLite database: a deterministic test
//! inference provider that answers from the exact request material, static calendars and
//! direct-manager predicates; never the environment or the network.

mod http_stages {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use axum::body::{to_bytes, Body};
    use axum::http::{Method, Request, StatusCode};
    use axum::Router;
    use plumb_api::application::{open_application, ApplicationRuntime};
    use plumb_api::{
        router, ApiIdentity, ApiState, CalendarConfig, DirectManagerFact, DirectManagerPredicates,
        F1GateConfig, F4GateConfig, GateInputsConfig, ProviderMode, Recipe,
    };
    use plumb_artifacts::{ArtifactKind, ArtifactStore, SqliteArtifactStore};
    use plumb_core::{to_canonical_json, CanonicalJson, Clock, Hash, Id, Timestamp};
    use plumb_functional::{
        resolution::{build_acceptance_companion, AcceptanceInput},
        DuplicatePolicy, ExprCalendarDefinition, ExprStaticCalendarProvider, VocabularyPolicy,
    };
    use plumb_inference::{
        materialize_derivation_record, validate_binding, InferenceArtifact, InferenceProvider,
        InferenceRequest, InferenceRequestMaterial, PersistedInferenceRefs, ProviderError,
        ProviderExecution, ProviderPolicy,
    };
    use plumb_patch::{PatchSet, Proposal, SemanticPatch};
    use plumb_psg::{Edge, Graph, Node, NodePayload};
    use plumb_session::SessionTurnConfig;
    use plumb_store::{
        BranchName, RevisionWriteMeta, SqliteRevisionStore, SqliteStageStore, SqliteWorkflowStore,
        StageStatus, WorkflowProposalState,
    };
    use plumb_validation::{load_builtin_software_profile, ValidationPolicy};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const T0: &str = "2026-01-01T00:00:00.000000000Z";
    const NOW: &str = "2026-05-01T12:00:00.000000000Z";
    const ACTOR: &str = "agent:api";
    const ASSISTANT: &str = "agent:assistant";
    const STAGE_ACTOR: &str = "agent:compiler";

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
        .unwrap()
    }

    fn agent(node_id: &str, kind: &str) -> Node {
        node(node_id, "Accepted", "Agent", json!({"agent_kind": kind}))
    }

    fn attribute(node_id: &str, name: &str) -> Node {
        node(
            node_id,
            "Accepted",
            "Attribute",
            json!({
                "name": name, "value_type": "String", "nullable": false, "unit": null,
                "precision": null, "enum_values": null, "data_classification": null
            }),
        )
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

    /// S1.1-origin Requirements (R0.4: consolidate recovers each source statement): one
    /// imported Markdown paragraph per statement, its EvidenceFragment as the only evidence and
    /// the `plumb_functional:segment_origin` covering the whole paragraph. Returns the source,
    /// fragments and Requirements.
    fn sourced_requirements(requirements: &[(&str, &str, &str)]) -> Vec<Node> {
        let markdown: String = requirements
            .iter()
            .map(|(_, _, statement)| format!("{statement}\n\n"))
            .collect();
        let source = plumb_import::import_markdown(
            "requirements.md",
            markdown.as_bytes(),
            &plumb_import::ImportAudit {
                created_by: id(ACTOR),
                created_at: ts(T0),
            },
        )
        .unwrap();
        let mut nodes = vec![source.source.clone()];
        nodes.extend(source.fragments.clone());
        for (i, (node_id, status, statement)) in requirements.iter().enumerate() {
            let fragment = source
                .fragments
                .iter()
                .find(|f| {
                    matches!(&f.payload, NodePayload::EvidenceFragment(e)
                        if e.extracted_text.as_deref() == Some(*statement))
                })
                .unwrap_or_else(|| panic!("no fragment for {statement:?}"));
            let mut value = serde_json::to_value(requirement(node_id, status, statement)).unwrap();
            value["evidence"] = json!([fragment.id]);
            value["extensions"] = json!({"plumb_functional:segment_origin": {
                "candidate_ref": format!("seg:{:016x}", i + 1),
                "fragment_ref": fragment.id, "start": 0, "end": statement.len()
            }});
            nodes.push(serde_json::from_value(value).unwrap());
        }
        nodes
    }

    /// The base agents every fixture graph carries.
    fn agents() -> Vec<Node> {
        vec![
            agent(ACTOR, "human"),
            agent(ASSISTANT, "llm_model"),
            agent(STAGE_ACTOR, "compiler_stage"),
        ]
    }

    /// An entity with the given Attribute names (`attr:<index>`).
    fn data_graph(names: &[&str]) -> Graph {
        let mut nodes = agents();
        nodes.push(node(
            "entity:employee",
            "Accepted",
            "Entity",
            json!({"name": "Employee", "description": null, "aggregate_root": null}),
        ));
        let mut edges = Vec::new();
        for (i, name) in names.iter().enumerate() {
            nodes.push(attribute(&format!("attr:{i}"), name));
            edges.push(edge(
                &format!("rel:attr{i}"),
                "has_attribute",
                "entity:employee",
                &format!("attr:{i}"),
            ));
        }
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap()
    }

    // ------------------------------------------------------------------ test provider

    /// Answers each request from its exact material only (Hotfix 066), by task kind. A hook may
    /// run once during a call (to simulate a concurrent writer).
    struct TestProvider {
        /// Data classifications to return: (attribute ref, class).
        classes: Mutex<Vec<(String, String)>>,
        calls: Mutex<
            Vec<(
                InferenceRequest,
                InferenceRequestMaterial,
                ProviderExecution,
            )>,
        >,
        hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        fail: Mutex<bool>,
    }

    impl TestProvider {
        fn new() -> Arc<TestProvider> {
            Arc::new(TestProvider {
                classes: Mutex::new(Vec::new()),
                calls: Mutex::new(Vec::new()),
                hook: Mutex::new(None),
                fail: Mutex::new(false),
            })
        }
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
            validate_binding(request, material)?;
            if *self.fail.lock().unwrap() {
                return Err(ProviderError::ProviderDisabled);
            }
            let context: Value = serde_json::from_slice(&material.context_json).unwrap();
            let output = match request.task_kind.as_str() {
                "requirement_segmentation" => {
                    let segments: Vec<Value> = context["fragments"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|f| {
                            json!({
                                "fragment_ref": f["fragment_ref"],
                                "start": 0,
                                "end": f["text"].as_str().unwrap().len(),
                                "classification": "requirement_candidate",
                            })
                        })
                        .collect();
                    json!({"version": 1, "segments": segments})
                }
                "requirement_classification" => {
                    let classes: Vec<Value> = context["candidates"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|c| {
                            json!({
                                "candidate_ref": c["candidate_ref"],
                                "requirement_kind": "functional",
                                "level": "system",
                            })
                        })
                        .collect();
                    json!({"version": 1, "classifications": classes, "intents": []})
                }
                // R0.4: consolidate and analyse answer with empty, schema-valid analyses.
                "vocabulary_analysis" => json!({"version": 1, "mentions": []}),
                "ears_normalization" => {
                    let entries: Vec<Value> = context["requirements"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| json!({"requirement_ref": r["requirement_ref"], "suggestion": null}))
                        .collect();
                    json!({"version": 1, "requirements": entries})
                }
                "domain_extraction" => {
                    json!({"version": 1, "entities": [], "attributes": [], "relationships": []})
                }
                "lifecycle_extraction" => {
                    json!({"version": 1, "states": [], "transitions": [], "invariants": []})
                }
                "data_classification" => {
                    let classes: Vec<Value> = self
                        .classes
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|(a, c)| json!({"attribute_ref": a, "classification": c}))
                        .collect();
                    json!({"version": 1, "classifications": classes})
                }
                other => panic!("unexpected task kind {other}"),
            };
            let executed = execution(request, output);
            self.calls
                .lock()
                .unwrap()
                .push((request.clone(), material.clone(), executed.clone()));
            if let Some(hook) = self.hook.lock().unwrap().take() {
                hook();
            }
            Ok(executed)
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

    fn identity() -> ApiIdentity {
        ApiIdentity::new(
            "alice".into(),
            id(ACTOR),
            vec!["analyst".into()],
            "pilot".into(),
        )
        .unwrap()
    }

    fn fixed() -> Arc<dyn Clock + Send + Sync> {
        Arc::new(plumb_core::FixedClock::new(ts(NOW)))
    }

    /// Explicit gate material for the stage runtime. `null_provider` selects the `null`
    /// provider mode (R0.2); otherwise the test provider plays the `replay` role, so every
    /// planned request executes against it.
    #[derive(Clone, Default)]
    struct Gates {
        f4: Option<F4GateConfig>,
        f1: Option<F1GateConfig>,
        null_provider: bool,
        scenario_derivation: Option<plumb_functional::ScenarioDerivationInputs>,
        /// The S4.5 verification applicability (default: no sources).
        verification: Option<plumb_functional::VerificationApplicabilityInputs>,
    }

    /// The replay mode of the test provider (R0.2: only `null` declares absence).
    fn replay_mode() -> ProviderMode {
        ProviderMode::Replay {
            fixtures_hash: Hash::content_sha256(b"http_stages test provider"),
        }
    }

    fn runtime(
        provider: Arc<dyn InferenceProvider + Send + Sync>,
        gates: &Gates,
    ) -> ApplicationRuntime {
        let calendars = vec![CalendarConfig {
            calendar_ref: id("cal:office"),
            weekend: vec!["saturday".into(), "sunday".into()],
            holidays: vec![],
        }];
        let managers = vec![DirectManagerFact {
            manager_ref: id("actor:manager"),
            employee_ref: id("actor:employee"),
        }];
        ApplicationRuntime {
            inference_provider: provider,
            provider_mode: if gates.null_provider {
                ProviderMode::Null
            } else {
                replay_mode()
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
                    id("cal:office"),
                    [],
                    [],
                )])
                .unwrap(),
            ),
            predicates: Arc::new(DirectManagerPredicates::new(&managers).unwrap()),
            calendar_configs: calendars,
            direct_managers: managers,
            expression_scopes: Vec::new(),
            calculation_scope_overrides: Vec::new(),
            runtime_semantic_values: Vec::new(),
            validation_policy: ValidationPolicy::default(),
            vocabulary_policy: VocabularyPolicy {
                language: "en".into(),
            },
            duplicate_policy: serde_json::from_value::<DuplicatePolicy>(
                json!({"jaccard_threshold": "0.8"}),
            )
            .unwrap(),
            calculation_scope: None,
            operation_scope: None,
            process_scope: None,
            authorization_draft: None,
            scenario_derivation: gates.scenario_derivation.clone(),
            verification_applicability: Some(gates.verification.clone().unwrap_or_default()),
            gates: GateInputsConfig {
                f1: gates.f1.clone(),
                f4: gates.f4.clone(),
            },
            directory: None,
            question_routing: None,
        }
    }

    fn open(
        path: &Path,
        provider: Arc<dyn InferenceProvider + Send + Sync>,
        gates: &Gates,
    ) -> ApiState {
        open_application(
            path,
            BranchName::main(),
            identity(),
            fixed(),
            load_builtin_software_profile().unwrap(),
            runtime(provider, gates),
        )
        .unwrap()
    }

    fn app_with(graph: Graph, artifacts: &[plumb_artifacts::Artifact], gates: Gates) -> App {
        app_using(graph, artifacts, gates, None)
    }

    /// The app over `graph` whose runtime uses `inference` instead of the test provider (which
    /// then stays unused and records nothing).
    fn app_using(
        graph: Graph,
        artifacts: &[plumb_artifacts::Artifact],
        gates: Gates,
        inference: Option<Arc<dyn InferenceProvider + Send + Sync>>,
    ) -> App {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plumb.sqlite");
        let profile = load_builtin_software_profile().unwrap();
        let mut store = SqliteRevisionStore::open(&path).unwrap();
        store
            .create_initial_revision(
                &graph,
                profile.profile_hash().unwrap(),
                profile.rule_pack_hash().unwrap(),
                RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id(ACTOR),
                    created_at: ts(T0),
                },
            )
            .unwrap();
        drop(store);
        let mut art = SqliteArtifactStore::open(&path).unwrap();
        for a in artifacts {
            art.put(a.kind, &a.media_type, &a.bytes, a.created_at)
                .unwrap();
        }
        drop(art);
        let provider = TestProvider::new();
        let inference = inference.unwrap_or_else(|| provider.clone());
        let state = open(&path, inference, &gates);
        let router = router(state.clone());
        App {
            _dir: dir,
            path,
            state,
            router,
            provider,
        }
    }

    fn app(graph: Graph) -> App {
        app_with(graph, &[], Gates::default())
    }

    struct Reply {
        status: StatusCode,
        etag: Option<String>,
        bytes: Vec<u8>,
    }

    impl Reply {
        fn json(&self) -> Value {
            serde_json::from_slice(&self.bytes)
                .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&self.bytes)))
        }
        fn code(&self) -> String {
            self.json()["code"].as_str().unwrap_or_default().to_owned()
        }
    }

    async fn send(app: &App, request: Request<Body>) -> Reply {
        let response = app.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let etag = response
            .headers()
            .get("etag")
            .map(|v| v.to_str().unwrap().to_owned());
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        Reply {
            status,
            etag,
            bytes,
        }
    }

    async fn get(app: &App, uri: &str) -> Reply {
        send(app, Request::get(uri).body(Body::empty()).unwrap()).await
    }

    async fn head_hash(app: &App) -> String {
        get(app, "/api/model/version").await.json()["semantic_hash"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn post(app: &App, uri: &str, if_match: Option<&str>, body: Value) -> Reply {
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(tag) = if_match {
            builder = builder.header("if-match", tag);
        }
        send(
            app,
            builder
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
    }

    async fn run_stage(app: &App, stage: &str, scope: Value) -> Reply {
        let tag = head_hash(app).await;
        post(
            app,
            &format!("/api/stages/{stage}/run"),
            Some(&tag),
            json!({ "scope": scope }),
        )
        .await
    }

    /// Polls `get_job` until the job leaves the in-flight states.
    async fn wait_job(app: &App, job: &str) -> Value {
        for _ in 0..400 {
            let r = get(app, &format!("/api/jobs/{job}")).await;
            assert_eq!(
                r.status,
                StatusCode::OK,
                "{}",
                String::from_utf8_lossy(&r.bytes)
            );
            let body = r.json();
            if !["queued", "acquiring", "evaluating"].contains(&body["status"].as_str().unwrap()) {
                return body;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("job {job} did not finish");
    }

    /// Runs a recipe in-process (as the CLI does) and returns its job ID.
    fn run_inline(app: &App, recipe: Recipe, scope: Option<Vec<String>>) -> Hash {
        let run = app.state.run_stage(recipe, scope, None).unwrap();
        app.state.execute_recipe_job(&run.job_id).unwrap();
        run.job_id
    }

    fn job(app: &App, job_id: &Hash) -> Value {
        serde_json::to_value(app.state.get_job(job_id.as_str()).unwrap().0).unwrap()
    }

    fn workflow(app: &App) -> SqliteWorkflowStore {
        SqliteWorkflowStore::open(&app.path).unwrap()
    }

    fn stage_store(app: &App) -> SqliteStageStore {
        SqliteStageStore::open(&app.path).unwrap()
    }

    fn head(app: &App) -> plumb_store::LoadedRevision {
        let store = SqliteRevisionStore::open(&app.path).unwrap();
        let rev = store.head(&BranchName::main()).unwrap().unwrap();
        store.load_revision(&rev).unwrap()
    }

    /// The pending stage proposals of a recipe job, decoded.
    fn pending(app: &App, job_id: &Hash) -> Vec<Proposal> {
        let store = stage_store(app);
        let wf = workflow(app);
        let mut out: Vec<Proposal> = store
            .pending_proposal_links()
            .unwrap()
            .into_iter()
            .filter(|l| &l.recipe_job_id == job_id)
            .map(|l| {
                let record = wf.proposal(&l.proposal_id).unwrap().unwrap();
                serde_json::from_str(&record.proposal_json).unwrap()
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    // ------------------------------------------------------------------ identity

    #[tokio::test]
    async fn http_stages_recipe_identity_is_the_frozen_projection() {
        let app = app(data_graph(&["Email"]));
        let loaded = head(&app);
        let run = app.state.run_stage(Recipe::Intake, None, None).unwrap();
        // Independently: the configuration projection and the capture, RFC 8785 + SHA-256.
        let config = json!({
            "version": 1,
            "recipe": "intake",
            "provider": null,
            "stage_actor_ref": STAGE_ACTOR,
            "inputs": {"validation_policy": {"promoted_to_blocker_rule_ids": [], "profile_allow_waiver_rule_ids": []}},
        });
        let config_hash = Hash::content_sha256(&to_canonical_json(&config).unwrap());
        let capture = json!({
            "version": 1,
            "branch": "main",
            "initial_base_revision": loaded.revision.id,
            "initial_base_semantic_hash": loaded.revision.semantic_hash,
            "recipe": "intake",
            "scope": null,
            "profile_hash": loaded.revision.profile_hash,
            "config_hash": config_hash,
        });
        let expected = Hash::content_sha256(&to_canonical_json(&capture).unwrap());
        assert_eq!(run.job_id, expected);
        // Re-running reuses the job; scope order is normalized; scope and recipe matter.
        assert_eq!(
            app.state
                .run_stage(Recipe::Intake, None, None)
                .unwrap()
                .job_id,
            expected
        );
        let a = app
            .state
            .run_stage(Recipe::Rules, Some(vec![]), None)
            .unwrap()
            .job_id;
        assert_ne!(a, expected);
        // The provider of a non-inference recipe is not bound, a different recipe is.
        let tags = app
            .state
            .run_stage(Recipe::Tags, None, None)
            .unwrap()
            .job_id;
        assert_ne!(tags, expected);
        let stored = stage_store(&app).recipe_job(&tags).unwrap().unwrap();
        let config: Value = serde_json::from_str(&stored.config_json).unwrap();
        assert_eq!(config["provider"]["mode"], "replay");
        assert_eq!(config["provider"]["policy"]["provider"], "test");
        assert_eq!(
            config["provider"]["replay_fixtures_hash"],
            json!(Hash::content_sha256(b"http_stages test provider"))
        );
        assert!(!stored.recipe_job_json.contains("2026-05-01"));
    }

    // ------------------------------------------------------------------ HTTP contract

    #[tokio::test]
    async fn http_stages_run_stage_and_get_job_contract() {
        let app = app(data_graph(&["Email"]));
        let tag = head_hash(&app).await;
        let before = head(&app).revision.id;
        // If-Match is required and literal.
        let r = post(&app, "/api/stages/intake/run", None, json!({})).await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::BAD_REQUEST, "E_INVALID_REQUEST")
        );
        let r = post(
            &app,
            "/api/stages/intake/run",
            Some("psg:sha256:0000000000000000000000000000000000000000000000000000000000000000"),
            json!({}),
        )
        .await;
        assert_eq!(r.status, StatusCode::PRECONDITION_FAILED);
        // Scope errors.
        for (stage, scope, status, code) in [
            (
                "tags",
                json!(["attr:0"]),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_STAGE_SCOPE_INVALID",
            ),
            (
                "intake",
                json!([]),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_STAGE_SCOPE_INVALID",
            ),
            (
                "rules",
                json!(["dt:none"]),
                StatusCode::NOT_FOUND,
                "E_NOT_FOUND",
            ),
            (
                "rules",
                json!(["attr:0"]),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_STAGE_SCOPE_INVALID",
            ),
            (
                "rules",
                json!(["dt:a", "dt:a"]),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "rules",
                json!([" dt:a"]),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "rules",
                json!(["dt:*"]),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
        ] {
            let r = post(
                &app,
                &format!("/api/stages/{stage}/run"),
                Some(&tag),
                json!({"scope": scope}),
            )
            .await;
            assert_eq!(
                (r.status, r.code().as_str()),
                (status, code),
                "{stage} {scope}"
            );
        }
        let r = post(&app, "/api/stages/nope/run", Some(&tag), json!({})).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        let r = post(
            &app,
            "/api/stages/intake/run",
            Some(&tag),
            json!({"scope": null, "x": 1}),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        // Missing required configuration.
        for (stage, missing) in [
            ("lint", json!(["gates.f1"])),
            ("calc", json!(["calculation_scope"])),
            ("flows", json!(["operation_scope", "process_scope"])),
            ("scenarios", json!(["scenario_derivation"])),
            ("run", json!(["gates.f4"])),
        ] {
            let r = post(
                &app,
                &format!("/api/stages/{stage}/run"),
                Some(&tag),
                json!({}),
            )
            .await;
            assert_eq!(
                (r.status, r.code().as_str()),
                (StatusCode::CONFLICT, "E_STAGE_CONFIG_MISSING"),
                "{stage}"
            );
            assert_eq!(r.json()["details"]["missing"], missing);
        }
        // A captured job: the base revision, no events, no revision change.
        let r = post(&app, "/api/stages/intake/run", Some(&tag), json!({})).await;
        assert_eq!(
            r.status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&r.bytes)
        );
        let body = r.json();
        assert_eq!(body["version"], body["revision"]);
        assert_eq!(body["revision"], before.to_string());
        assert_eq!(body["semantic_hash"], tag);
        assert_eq!(body["events"], json!([]));
        let job_id = body["job_id"].as_str().unwrap().to_owned();
        let done = wait_job(&app, &job_id).await;
        assert_eq!(done["status"], "ready");
        assert_eq!(done["progress"], 1.0);
        assert_eq!(done["base_revision"], before.to_string());
        assert_eq!(done["result_version"], before.to_string());
        assert_eq!(done["error"], Value::Null);
        assert_eq!(head(&app).revision.id, before);
        // Re-posting returns the same job without new work.
        let again = post(&app, "/api/stages/intake/run", Some(&tag), json!({})).await;
        assert_eq!(again.json()["job_id"], job_id.as_str());
        // get_job: ETag, malformed and unknown IDs.
        let r = get(&app, &format!("/api/jobs/{job_id}")).await;
        assert_eq!(r.etag.as_deref(), Some(tag.as_str()));
        let r = get(&app, "/api/jobs/not-a-hash").await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::BAD_REQUEST, "E_INVALID_REQUEST")
        );
        let r = get(&app, &format!("/api/jobs/sha256:{}", "0".repeat(64))).await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::NOT_FOUND, "E_NOT_FOUND")
        );
        // The durable I0 report is current on the very next read.
        let gates = get(&app, "/api/gates").await.json();
        assert!(gates.to_string().contains("I0"), "{gates}");
    }

    #[tokio::test]
    async fn http_stages_ready_job_becomes_stale_when_the_head_moves() {
        let app = app(data_graph(&["Email"]));
        let job_id = run_inline(&app, Recipe::Intake, None);
        assert_eq!(job(&app, &job_id)["status"], "ready");
        // A later, unrelated write moves the head.
        let loaded = head(&app);
        SqliteRevisionStore::open(&app.path)
            .unwrap()
            .commit(
                &BranchName::main(),
                &loaded.revision.id,
                &PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch: SemanticPatch::AddNode {
                        node: requirement("req:new", "Proposed", "Employees shall log in."),
                    },
                },
                RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id(ACTOR),
                    created_at: ts(NOW),
                },
            )
            .unwrap();
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "stale");
        assert_eq!(view["result_version"], Value::Null);
        assert_eq!(view["progress"], 1.0);
    }

    // ------------------------------------------------------------------ tags: Cases A-D

    fn data_classes(app: &App) -> Vec<(String, Option<String>)> {
        let loaded = head(app);
        let mut out: Vec<(String, Option<String>)> = loaded
            .graph
            .nodes()
            .values()
            .filter_map(|n| match &n.payload {
                NodePayload::Attribute(a) => {
                    Some((n.id.to_string(), a.data_classification.clone()))
                }
                _ => None,
            })
            .collect();
        out.sort();
        out
    }

    #[tokio::test]
    async fn http_stages_tags_singleton_auto_commits() {
        let app = app(data_graph(&["Email"]));
        let semantic_before = head(&app).revision.semantic_hash;
        let job_id = run_inline(&app, Recipe::Tags, None);
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "committed", "{view}");
        let loaded = head(&app);
        assert_eq!(view["result_version"], loaded.revision.id.to_string());
        assert_ne!(loaded.revision.semantic_hash, semantic_before);
        assert_eq!(
            data_classes(&app),
            vec![("attr:0".into(), Some("pii".into()))]
        );
        assert!(pending(&app, &job_id).is_empty());
    }

    #[tokio::test]
    async fn http_stages_tags_multiple_auto_and_mixed_sets_stay_pending() {
        // Case D: two dictionary hits, no HUMAN proposal.
        let app = app(data_graph(&["Email", "Salary"]));
        let before = head(&app).revision.id;
        let job_id = run_inline(&app, Recipe::Tags, None);
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "ready", "{view}");
        assert_eq!(view["result_version"], before.to_string());
        assert_eq!(head(&app).revision.id, before);
        let props = pending(&app, &job_id);
        assert_eq!(props.len(), 2);
        assert!(props
            .iter()
            .all(|p| p.acceptance_policy == plumb_patch::AcceptancePolicy::AutoDerivation));

        // Case C: one dictionary hit and one inferred HUMAN classification.
        drop(app);
        let app = self::app(data_graph(&["Email", "Notes"]));
        *app.provider.classes.lock().unwrap() = vec![("attr:1".into(), "confidential".into())];
        let before = head(&app).revision.id;
        let job_id = run_inline(&app, Recipe::Tags, None);
        assert_eq!(job(&app, &job_id)["status"], "ready");
        assert_eq!(head(&app).revision.id, before);
        let props = pending(&app, &job_id);
        let policies: Vec<_> = props.iter().map(|p| p.acceptance_policy).collect();
        assert!(policies.contains(&plumb_patch::AcceptancePolicy::AutoDerivation));
        assert!(policies.contains(&plumb_patch::AcceptancePolicy::HumanConfirm));
        // The HUMAN proposal carries the reconstructed inference DerivationRef.
        let human = props
            .iter()
            .find(|p| p.acceptance_policy == plumb_patch::AcceptancePolicy::HumanConfirm)
            .unwrap();
        let (request, _, executed) = app.provider.calls.lock().unwrap()[0].clone();
        let artifacts = SqliteArtifactStore::open(&app.path).unwrap();
        let request_ref = Hash::content_sha256(&to_canonical_json(&request).unwrap());
        assert!(artifacts.exists(&request_ref).unwrap());
        let validated_ref = Hash::content_sha256(&to_canonical_json(&executed.artifact).unwrap());
        let persisted = PersistedInferenceRefs {
            request_artifact_ref: request_ref,
            raw_response_ref: executed.artifact.raw_response_hash.clone(),
            validated_inference_ref: validated_ref.clone(),
        };
        let record = materialize_derivation_record(
            &request,
            &executed,
            &persisted,
            vec![validated_ref.to_string()],
            head(&app).revision.created_at,
        )
        .unwrap();
        assert_eq!(
            human.derivation_refs,
            vec![plumb_psg::DerivationRef::from(record.id.clone())]
        );
        // The provenance record is never committed to the PSG.
        assert!(head(&app).graph.node(&record.id).is_none());
    }

    #[tokio::test]
    async fn http_stages_governed_acceptance_records_the_child_and_stales_siblings() {
        let app = app(data_graph(&["Email", "Salary"]));
        let job_id = run_inline(&app, Recipe::Tags, None);
        let props = pending(&app, &job_id);
        assert_eq!(props.len(), 2);
        let tag = head_hash(&app).await;
        let r = post(
            &app,
            &format!("/api/proposals/{}/accept", props[0].id),
            Some(&tag),
            json!({"reason": null}),
        )
        .await;
        assert_eq!(
            r.status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&r.bytes)
        );
        let committed_revision = r.json()["revision"].as_str().unwrap().to_owned();
        // The child is Committed with the original output; the parent is committed.
        let steps = stage_store(&app).steps(&job_id).unwrap();
        let child_id = steps[0].stage_job_id.clone().unwrap();
        let child = stage_store(&app).stage_job(&child_id).unwrap().unwrap();
        assert_eq!(child.status, StageStatus::Committed);
        let output_ref = Hash::content_sha256(&to_canonical_json(&props[0]).unwrap());
        assert!(child.stage_job_json.contains(output_ref.as_str()));
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "committed");
        assert_eq!(view["result_version"], committed_revision);
        assert_eq!(
            workflow(&app)
                .proposal(&props[0].id)
                .unwrap()
                .unwrap()
                .state,
            WorkflowProposalState::Accepted
        );
        // The accepted revision is the A0.3 governed companion (original effect + decision).
        let loaded = head(&app);
        assert!(!loaded.revision.decision_refs.is_empty());
        // The sibling is immutable and stale against the child.
        let tag = head_hash(&app).await;
        let r = post(
            &app,
            &format!("/api/proposals/{}/accept", props[1].id),
            Some(&tag),
            json!({"reason": null}),
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "E_PROPOSAL_BASELINE_CHANGED")
        );
    }

    #[tokio::test]
    async fn http_stages_interrupted_governed_acceptance_is_recovered_without_recommit() {
        let app = app(data_graph(&["Email", "Salary"]));
        let job_id = run_inline(&app, Recipe::Tags, None);
        let props = pending(&app, &job_id);
        let target = props[0].clone();
        // Simulate a crash right after the A0.3 governed CAS: the companion is committed by the
        // store, the workflow row, child and parent are untouched.
        let loaded = head(&app);
        let companion = build_acceptance_companion(
            &loaded.graph,
            &target,
            &AcceptanceInput {
                reason_codes: Vec::new(),
                decided_by: id(ACTOR),
                decided_at: ts(NOW),
                rationale: None,
            },
        )
        .unwrap();
        let mut art = SqliteArtifactStore::open(&app.path).unwrap();
        art.put(
            ArtifactKind::Patch,
            "application/json",
            &companion.original_patch_bytes,
            ts(NOW),
        )
        .unwrap();
        let commit = SqliteRevisionStore::open(&app.path)
            .unwrap()
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
        // A restart reconciles: workflow accepted, child committed, parent committed.
        let state = open(&app.path, app.provider.clone(), &Gates::default());
        let after = head(&app);
        assert_eq!(after.revision.id, commit.revision.id, "no second commit");
        assert_eq!(
            workflow(&app).proposal(&target.id).unwrap().unwrap().state,
            WorkflowProposalState::Accepted
        );
        let child_id = stage_store(&app).steps(&job_id).unwrap()[0]
            .stage_job_id
            .clone()
            .unwrap();
        assert_eq!(
            stage_store(&app)
                .stage_job(&child_id)
                .unwrap()
                .unwrap()
                .status,
            StageStatus::Committed
        );
        let (view, _) = state.get_job(job_id.as_str()).unwrap();
        assert_eq!(serde_json::to_value(view).unwrap()["status"], "committed");
    }

    // ------------------------------------------------------------------ extract

    fn imported() -> plumb_import::ImportedSource {
        plumb_import::import_markdown(
            "requirements.md",
            b"# Leave\n\nEmployees shall submit leave requests.\n",
            &plumb_import::ImportAudit {
                created_by: id(ACTOR),
                created_at: ts(T0),
            },
        )
        .unwrap()
    }

    fn source_app() -> App {
        let source = imported();
        let mut nodes = agents();
        nodes.push(source.source.clone());
        nodes.extend(source.fragments.clone());
        let graph = Graph::new(id(PROJECT), id(PROFILE), nodes, vec![]).unwrap();
        app_with(
            graph,
            &[
                source.original_artifact.clone(),
                source.extracted_artifact.clone(),
            ],
            Gates::default(),
        )
    }

    #[tokio::test]
    async fn http_stages_extract_runs_two_children_over_one_projection() {
        let app = source_app();
        let job_id = run_inline(&app, Recipe::Extract, None);
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "ready", "{view}");
        let steps = stage_store(&app).steps(&job_id).unwrap();
        assert_eq!(
            steps.iter().map(|s| s.phase.as_str()).collect::<Vec<_>>(),
            ["segmentation", "classification"]
        );
        assert_ne!(steps[0].stage_job_id, steps[1].stage_job_id);
        // The segmentation Projection is canonical and drives classification exactly.
        let projection_ref = steps[0].intermediate_artifact_ref.clone().unwrap();
        let artifact = SqliteArtifactStore::open(&app.path)
            .unwrap()
            .get(&projection_ref)
            .unwrap();
        assert_eq!(artifact.kind, ArtifactKind::Projection);
        assert_eq!(artifact.media_type, "application/json");
        let projection: Value = serde_json::from_slice(&artifact.bytes).unwrap();
        assert_eq!(to_canonical_json(&projection).unwrap(), artifact.bytes);
        assert_eq!(projection["version"], 1);
        assert_eq!(projection["mode"], "inference");
        let calls = app.provider.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2);
        let context: Value = serde_json::from_slice(&calls[1].1.context_json).unwrap();
        let mut classified: Vec<&Value> = context["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| &c["candidate_ref"])
            .collect();
        let mut segmented: Vec<&Value> = projection["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["classification"] == "requirement_candidate")
            .map(|c| &c["id"])
            .collect();
        classified.sort_by_key(|v| v.as_str().unwrap().to_owned());
        segmented.sort_by_key(|v| v.as_str().unwrap().to_owned());
        assert_eq!(classified, segmented);
        // The classification child is bound to the projection in its config.
        let child = stage_store(&app)
            .stage_job(steps[1].stage_job_id.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(child.status, StageStatus::Ready);
        let props = pending(&app, &job_id);
        assert!(!props.is_empty());
        assert!(props
            .iter()
            .all(|p| p.acceptance_policy == plumb_patch::AcceptancePolicy::HumanConfirm));
    }

    #[tokio::test]
    async fn http_stages_extract_is_stale_when_the_head_moves_between_phases() {
        let app = source_app();
        let path = app.path.clone();
        *app.provider.hook.lock().unwrap() = Some(Box::new(move || {
            let mut store = SqliteRevisionStore::open(&path).unwrap();
            let head = store.head(&BranchName::main()).unwrap().unwrap();
            let loaded = store.load_revision(&head).unwrap();
            store
                .commit(
                    &BranchName::main(),
                    &head,
                    &PatchSet {
                        base_semantic_hash: loaded.revision.semantic_hash.clone(),
                        patch: SemanticPatch::AddNode {
                            node: requirement(
                                "req:racer",
                                "Proposed",
                                "Another writer was faster.",
                            ),
                        },
                    },
                    RevisionWriteMeta {
                        decision_refs: vec![],
                        created_by: id(ACTOR),
                        created_at: ts(NOW),
                    },
                )
                .unwrap();
        }));
        let job_id = run_inline(&app, Recipe::Extract, None);
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "stale", "{view}");
        assert_eq!(view["result_version"], Value::Null);
        // Only segmentation ran; no classification, no replay on the new head.
        assert_eq!(app.provider.calls.lock().unwrap().len(), 1);
        assert!(pending(&app, &job_id).is_empty());
    }

    #[tokio::test]
    async fn http_stages_provider_failure_fails_the_recipe_and_its_child() {
        let app = app(data_graph(&["Email"]));
        *app.provider.fail.lock().unwrap() = true;
        let job_id = run_inline(&app, Recipe::Tags, None);
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "failed");
        assert!(view["error"]
            .as_str()
            .unwrap()
            .starts_with("E_INFERENCE_FAILED: "));
        let child = stage_store(&app).steps(&job_id).unwrap()[0]
            .stage_job_id
            .clone()
            .unwrap();
        assert_eq!(
            stage_store(&app).stage_job(&child).unwrap().unwrap().status,
            StageStatus::Failed
        );
        // A repeated run returns the same terminal job; nothing is reset.
        let again = app.state.run_stage(Recipe::Tags, None, None).unwrap();
        assert_eq!(again.job_id, job_id);
        app.state.execute_recipe_job(&again.job_id).unwrap();
        assert_eq!(job(&app, &job_id)["status"], "failed");
    }

    // ------------------------------------------------------------------ restart

    #[tokio::test]
    async fn http_stages_restart_fails_interrupted_jobs() {
        let app = app(data_graph(&["Email"]));
        let queued = app.state.run_stage(Recipe::Intake, None, None).unwrap();
        assert_eq!(job(&app, &queued.job_id)["status"], "queued");
        let state = open(&app.path, app.provider.clone(), &Gates::default());
        let (view, _) = state.get_job(queued.job_id.as_str()).unwrap();
        let view = serde_json::to_value(view).unwrap();
        assert_eq!(view["status"], "failed");
        assert!(view["error"]
            .as_str()
            .unwrap()
            .starts_with("E_JOB_INTERRUPTED: "));
        // No hidden resume.
        state.execute_recipe_job(&queued.job_id).unwrap();
        assert_eq!(
            serde_json::to_value(state.get_job(queued.job_id.as_str()).unwrap().0).unwrap()
                ["status"],
            "failed"
        );
    }

    // ------------------------------------------------------------------ gates

    #[tokio::test]
    async fn http_stages_ready_requires_configured_inputs() {
        let app = app(data_graph(&["Email"]));
        let err = app.state.ready().unwrap_err();
        assert_eq!(err.body().code, "E_GATE_INPUTS_MISSING");
    }

    #[tokio::test]
    async fn http_stages_ready_evaluates_the_chain_and_persists_reports() {
        let app = app_with(data_graph(&["Email"]), &[], chain_gates());
        let outcome = app.state.ready().unwrap();
        assert_eq!(outcome.evaluation_at, ts(NOW));
        assert_eq!(
            outcome
                .gates
                .iter()
                .map(|g| g.gate.as_str())
                .collect::<Vec<_>>(),
            ["I0", "F1", "F2", "F3", "F4"]
        );
        // R0.4: zero Accepted Requirements is a product precondition, not a gate result.
        assert!(outcome.gates.iter().all(|g| g.result == "pass"));
        assert_eq!(outcome.preconditions, ["no_accepted_requirements"]);
        assert!(!outcome.passed());
        let reports = stage_store(&app).gate_reports().unwrap();
        assert_eq!(reports.len(), 5);
        let f3 = reports.iter().find(|r| r.gate == "F3").unwrap();
        assert_eq!(f3.evaluation_at, Some(ts(NOW)));
        assert!(reports
            .iter()
            .filter(|r| r.gate != "F3")
            .all(|r| r.evaluation_at.is_none()));
        // The durable reports are current for every gate read immediately.
        let r = send(
            &app,
            Request::get("/api/gates").body(Body::empty()).unwrap(),
        )
        .await;
        let text = String::from_utf8_lossy(&r.bytes);
        for gate in ["I0", "F1", "F2", "F3", "F4"] {
            assert!(text.contains(gate), "{gate}: {text}");
        }
    }

    // ------------------------------------------------------------------ run

    fn scenario(node_id: &str, then_state: &str) -> Node {
        node(
            node_id,
            "Accepted",
            "Scenario",
            json!({
                "name": node_id, "scenario_kind": "state_transition",
                "given": [{"states_active": ["state:draft"]}],
                "when": {"operation": "op:submit"},
                "then": [{"states_active": [then_state]}],
                "requirement_refs": ["req:a"],
                "derived_from_refs": ["trans:submit"],
                "confirmation_status": "Confirmed"
            }),
        )
    }

    fn run_graph() -> Graph {
        let mut nodes = agents();
        nodes.extend(sourced_requirements(&[(
            "req:a",
            "Accepted",
            "Employees shall submit leave requests.",
        )]));
        nodes.extend([
            node(
                "entity:request",
                "Accepted",
                "Entity",
                json!({"name": "LeaveRequest", "description": null, "aggregate_root": null}),
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
            node(
                "op:submit",
                "Accepted",
                "Operation",
                json!({
                    "name": "op:submit", "operation_kind": "command", "input_schema_ref": null,
                    "output_schema_ref": null, "preconditions": null, "postconditions": null,
                    "idempotency": null, "transaction_semantics": null
                }),
            ),
            scenario("scn:pass", "state:submitted"),
            scenario("scn:fail", "state:draft"),
        ]);
        let edges = vec![
            edge("rel:spec", "specified_by", "req:a", "op:submit"),
            edge("rel:draft", "has_state", "entity:request", "state:draft"),
            edge(
                "rel:submitted",
                "has_state",
                "entity:request",
                "state:submitted",
            ),
            edge("rel:via", "transitions_via", "trans:submit", "op:submit"),
        ];
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap()
    }

    #[tokio::test]
    async fn http_stages_run_commits_runs_then_evaluates_f4_on_the_committed_revision() {
        // R0.4: the blocking Scenarios come from the S4.5 applicability over the graph (both
        // Scenarios cover the blocking executable op:submit); only the in-scope Scenarios and
        // the required kinds are external.
        let gates = Gates {
            f4: Some(F4GateConfig {
                in_scope_scenario_refs: vec![id("scn:fail"), id("scn:pass")],
                required_scenario_kinds: vec![serde_json::from_value(json!({
                    "source_ref": "op:submit", "required_scenario_kinds": []
                }))
                .unwrap()],
                visible_assumption_refs: vec![],
            }),
            verification: Some(
                serde_json::from_value(json!({"sources": [
                    {"source_ref": "op:submit", "mode": "executable_scenario", "blocking": true}
                ]}))
                .unwrap(),
            ),
            ..Gates::default()
        };
        let app = app_with(run_graph(), &[], gates);
        let before = head(&app);
        let job_id = run_inline(&app, Recipe::Run, None);
        let view = job(&app, &job_id);
        // The ScenarioRun update committed; the failing blocking run makes F4 FAIL, which is an
        // evaluated result and not a job failure.
        assert_eq!(view["status"], "committed", "{view}");
        let after = head(&app);
        assert_ne!(after.revision.id, before.revision.id);
        assert_eq!(after.revision.semantic_hash, before.revision.semantic_hash);
        assert_eq!(view["result_version"], after.revision.id.to_string());
        let steps = stage_store(&app).steps(&job_id).unwrap();
        assert_eq!(
            steps.iter().map(|s| s.phase.as_str()).collect::<Vec<_>>(),
            ["execute", "verification", "f4"]
        );
        assert_eq!(steps[0].input_revision_id, before.revision.id);
        assert_eq!(steps[1].input_revision_id, after.revision.id);
        assert_eq!(
            steps[2].result_revision_id.as_ref(),
            Some(&after.revision.id)
        );
        // Every linked trace was persisted.
        let artifacts = SqliteArtifactStore::open(&app.path).unwrap();
        let mut runs = 0;
        for n in after.graph.nodes().values() {
            if let NodePayload::ScenarioRun(_) = &n.payload {
                runs += 1;
                let link = n.extensions.values().next().unwrap().as_str().unwrap();
                let trace = artifacts.get(&link.parse().unwrap()).unwrap();
                assert_eq!(trace.kind, ArtifactKind::ScenarioTrace);
            }
        }
        assert_eq!(runs, 2);
        // F4 was evaluated on the committed revision and failed.
        let results = app.state.recipe_gate_results(&job_id).unwrap();
        assert_eq!(results, vec![("F4".to_owned(), "fail".to_owned())]);
        let f4 = stage_store(&app)
            .gate_reports()
            .unwrap()
            .into_iter()
            .find(|r| r.gate == "F4")
            .unwrap();
        assert_eq!(f4.revision_id, after.revision.id);
        assert_eq!(f4.recipe_job_id, Some(job_id.clone()));
        // A run with nothing new to record creates no revision.
        let again = run_inline(&app, Recipe::Run, None);
        assert_eq!(job(&app, &again)["status"], "ready");
        assert_eq!(head(&app).revision.id, after.revision.id);
    }

    #[tokio::test]
    async fn http_stages_f4_needs_valid_explicit_external_inputs() {
        // R0.4: an executable source without explicit required kinds is a missing external
        // input naming the source; a configured Scenario in a forbidden state is stale.
        let verification: plumb_functional::VerificationApplicabilityInputs =
            serde_json::from_value(json!({"sources": [
                {"source_ref": "op:submit", "mode": "executable_scenario", "blocking": false}
            ]}))
            .unwrap();
        let gates = Gates {
            f4: Some(F4GateConfig {
                in_scope_scenario_refs: vec![id("scn:pass")],
                required_scenario_kinds: vec![],
                visible_assumption_refs: vec![],
            }),
            verification: Some(verification.clone()),
            ..Gates::default()
        };
        let app = app_with(run_graph(), &[], gates);
        let job_id = run_inline(&app, Recipe::Run, Some(vec!["scn:pass".into()]));
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "failed", "{view}");
        assert!(view["error"]
            .as_str()
            .unwrap()
            .starts_with("E_GATE_INPUTS_MISSING: "));
        let details = failure_details(&view);
        assert_eq!(details["field"], "required_scenario_kinds");
        assert_eq!(details["elements"], json!(["op:submit"]));

        let mut graph = run_graph();
        graph = Graph::new(
            id(PROJECT),
            id(PROFILE),
            graph
                .nodes()
                .values()
                .cloned()
                .map(|mut n| {
                    if n.id == id("scn:fail") {
                        n.status = plumb_psg::ElementStatus::Deprecated;
                    }
                    n
                })
                .collect(),
            graph.edges().values().cloned().collect(),
        )
        .unwrap();
        let gates = Gates {
            f4: Some(F4GateConfig {
                in_scope_scenario_refs: vec![id("scn:fail")],
                required_scenario_kinds: vec![],
                visible_assumption_refs: vec![],
            }),
            ..Gates::default()
        };
        let app = app_with(graph, &[], gates);
        let job_id = run_inline(&app, Recipe::Run, Some(vec!["scn:pass".into()]));
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "failed", "{view}");
        assert!(view["error"]
            .as_str()
            .unwrap()
            .starts_with("E_GATE_INPUTS_STALE: "));
        assert_eq!(failure_details(&view)["elements"], json!(["scn:fail"]));
    }

    /// The external F1 configuration: the lint policy only (R0.4).
    fn f1_config() -> F1GateConfig {
        serde_json::from_value(json!({"lint_policy": {"severity_overrides": {}}})).unwrap()
    }

    fn chain_gates() -> Gates {
        Gates {
            f1: Some(f1_config()),
            f4: Some(F4GateConfig {
                in_scope_scenario_refs: vec![],
                required_scenario_kinds: vec![],
                visible_assumption_refs: vec![],
            }),
            ..Gates::default()
        }
    }

    /// A state over `path` with the full ready-chain configuration.
    fn ready_state(app: &App) -> ApiState {
        let gates = chain_gates();
        let rt = runtime(app.provider.clone(), &gates);
        open_application(
            &app.path,
            BranchName::main(),
            identity(),
            fixed(),
            load_builtin_software_profile().unwrap(),
            rt,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn http_stages_worker_executes_queued_jobs() {
        let app = app(data_graph(&["Email"]));
        let r = run_stage(&app, "tags", Value::Null).await;
        assert_eq!(
            r.status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&r.bytes)
        );
        let done = wait_job(&app, r.json()["job_id"].as_str().unwrap()).await;
        assert_eq!(done["status"], "committed", "{done}");
    }

    #[tokio::test]
    async fn http_stages_f3_takes_only_current_s3_and_waiver_decisions() {
        // A governed acceptance adds a proposal_acceptance decision; F3 must ignore it.
        let app = app(data_graph(&["Email", "Salary"]));
        let job_id = run_inline(&app, Recipe::Tags, None);
        let props = pending(&app, &job_id);
        let tag = head_hash(&app).await;
        let r = post(
            &app,
            &format!("/api/proposals/{}/accept", props[0].id),
            Some(&tag),
            json!({"reason": null}),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK);
        assert!(!head(&app).revision.decision_refs.is_empty());
        let state = ready_state(&app);
        let outcome = state.ready().unwrap();
        let f3 = outcome.gates.iter().find(|g| g.gate == "F3").unwrap();
        assert_eq!(f3.result, "pass");
    }

    #[tokio::test]
    async fn http_stages_broken_trace_link_is_an_f4_context_error() {
        let f4 = F4GateConfig {
            in_scope_scenario_refs: vec![id("scn:pass")],
            required_scenario_kinds: vec![],
            visible_assumption_refs: vec![],
        };
        let gates = Gates {
            f1: Some(f1_config()),
            f4: Some(f4),
            ..Gates::default()
        };
        let app = app_with(run_graph(), &[], gates.clone());
        let job_id = run_inline(&app, Recipe::Run, Some(vec!["scn:pass".into()]));
        assert_eq!(job(&app, &job_id)["status"], "committed");
        // The durable F1/F2 analyses of the Accepted Requirement exist (R0.4).
        let consolidate = run_inline(&app, Recipe::Consolidate, None);
        assert_eq!(
            job(&app, &consolidate)["status"],
            "ready",
            "{}",
            job(&app, &consolidate)
        );
        let analyse = run_inline(&app, Recipe::Analyse, None);
        assert_eq!(
            job(&app, &analyse)["status"],
            "ready",
            "{}",
            job(&app, &analyse)
        );
        // Point the run at a trace that does not exist.
        let loaded = head(&app);
        let run_node = loaded
            .graph
            .nodes()
            .values()
            .find(|n| matches!(n.payload, NodePayload::ScenarioRun(_)))
            .unwrap()
            .clone();
        let key = run_node.extensions.keys().next().unwrap().clone();
        SqliteRevisionStore::open(&app.path)
            .unwrap()
            .commit(
                &BranchName::main(),
                &loaded.revision.id,
                &PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch: SemanticPatch::SetNodeExtension {
                        target: plumb_patch::ElementPrecondition {
                            id: run_node.id.clone(),
                            expected_hash: plumb_psg::node_element_hash(&run_node).unwrap(),
                        },
                        key,
                        value: json!(format!("sha256:{}", "a".repeat(64))),
                    },
                },
                RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id(ACTOR),
                    created_at: ts(NOW),
                },
            )
            .unwrap();
        let rt = runtime(app.provider.clone(), &gates);
        let state = open_application(
            &app.path,
            BranchName::main(),
            identity(),
            fixed(),
            load_builtin_software_profile().unwrap(),
            rt,
        )
        .unwrap();
        let err = state.ready().unwrap_err();
        assert_eq!(err.body().code, "E_STAGE_EVALUATION_FAILED", "{err:?}");
    }

    #[tokio::test]
    async fn http_stages_lint_evaluates_assembled_f1_only() {
        let gates = chain_gates();
        let app = app_with(data_graph(&["Email"]), &[], gates);
        let job_id = run_inline(&app, Recipe::Lint, None);
        assert_eq!(job(&app, &job_id)["status"], "ready");
        let results = app.state.recipe_gate_results(&job_id).unwrap();
        assert_eq!(results, vec![("F1".to_owned(), "pass".to_owned())]);
        let report = stage_store(&app)
            .gate_reports()
            .unwrap()
            .into_iter()
            .find(|r| r.gate == "F1")
            .unwrap();
        // R0.4: the hash binds the assembled inputs (no Accepted Requirement here).
        let assembled: plumb_validation::F1ValidationInputs = serde_json::from_value(json!({
            "requirements": [], "analysis_findings": [],
            "lint_policy": {"severity_overrides": {}}
        }))
        .unwrap();
        let expected = Hash::content_sha256(&to_canonical_json(&assembled).unwrap());
        assert_eq!(report.supplemental_input_hash, Some(expected));
    }

    // ------------------------------------------------------------------ R0.4 gate input assembly

    const REQ_A: &str = "Employees shall submit leave requests.";
    const REQ_B: &str = "Managers shall approve leave requests.";

    /// req:a Accepted and req:b Proposed, both S1.1-origin.
    fn assembly_graph() -> Graph {
        let mut nodes = agents();
        nodes.extend(sourced_requirements(&[
            ("req:a", "Accepted", REQ_A),
            ("req:b", "Proposed", REQ_B),
        ]));
        Graph::new(id(PROJECT), id(PROFILE), nodes, vec![]).unwrap()
    }

    /// Commits `patch` directly on the head (a concurrent writer; no restart of the state).
    fn commit_patch(app: &App, patch: impl FnOnce(&plumb_store::LoadedRevision) -> SemanticPatch) {
        let loaded = head(app);
        SqliteRevisionStore::open(&app.path)
            .unwrap()
            .commit(
                &BranchName::main(),
                &loaded.revision.id,
                &PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch: patch(&loaded),
                },
                RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id(ACTOR),
                    created_at: ts(NOW),
                },
            )
            .unwrap();
    }

    /// Accepts the Proposed Requirement `node_id` on the head.
    fn accept_requirement(app: &App, node_id: &str) {
        commit_patch(app, |loaded| {
            let node = loaded.graph.node(&id(node_id)).unwrap();
            SemanticPatch::SetStatus {
                target: plumb_patch::ElementPrecondition {
                    id: node.id.clone(),
                    expected_hash: plumb_psg::node_element_hash(node).unwrap(),
                },
                from: plumb_psg::ElementStatus::Proposed,
                to: plumb_psg::ElementStatus::Accepted,
            }
        });
    }

    /// The exact F1 inputs assembled for the Accepted Requirements of the head: no vocabulary
    /// mention (the test provider returns none), no finding and the configured lint policy.
    fn expected_f1_hash(app: &App) -> Hash {
        let loaded = head(app);
        let mut requirements = Vec::new();
        for node in loaded.graph.nodes().values() {
            let NodePayload::Requirement(r) = &node.payload else {
                continue;
            };
            if node.status != plumb_psg::ElementStatus::Accepted {
                continue;
            }
            requirements.push(json!({
                "requirement_ref": node.id, "vocabulary_dependencies": [],
                "lint_input": {
                    "requirement_ref": node.id, "statement": r.statement,
                    "evidence_refs": node.evidence, "source_anchor": null, "term_context": null
                }
            }));
        }
        let inputs: plumb_validation::F1ValidationInputs = serde_json::from_value(json!({
            "requirements": requirements, "analysis_findings": [],
            "lint_policy": {"severity_overrides": {}}
        }))
        .unwrap();
        Hash::content_sha256(&to_canonical_json(&inputs).unwrap())
    }

    /// The F1 report of the lint job `job_id`.
    fn f1_report(app: &App, job_id: &Hash) -> plumb_store::GateReportRecord {
        stage_store(app)
            .gate_reports()
            .unwrap()
            .into_iter()
            .find(|r| r.gate == "F1" && r.recipe_job_id.as_ref() == Some(job_id))
            .unwrap()
    }

    #[tokio::test]
    async fn gate_inputs_follow_new_accepted_requirement_without_restart() {
        let app = app_with(assembly_graph(), &[], chain_gates());
        assert_eq!(
            job(&app, &run_inline(&app, Recipe::Consolidate, None))["status"],
            "ready"
        );
        let first = run_inline(&app, Recipe::Lint, None);
        assert_eq!(
            job(&app, &first)["status"],
            "ready",
            "{}",
            job(&app, &first)
        );
        let before = expected_f1_hash(&app);
        assert_eq!(
            f1_report(&app, &first).supplemental_input_hash,
            Some(before.clone())
        );

        // A newly Accepted Requirement is assembled by the same running state: the frozen
        // pre-R0.4 configuration would have stayed at {req:a} and failed exact coverage.
        accept_requirement(&app, "req:b");
        assert_eq!(
            job(&app, &run_inline(&app, Recipe::Consolidate, None))["status"],
            "ready"
        );
        let second = run_inline(&app, Recipe::Lint, None);
        assert_eq!(
            job(&app, &second)["status"],
            "ready",
            "{}",
            job(&app, &second)
        );
        assert_eq!(
            app.state.recipe_gate_results(&second).unwrap(),
            vec![("F1".to_owned(), "pass".to_owned())]
        );
        let after = expected_f1_hash(&app);
        assert_ne!(after, before);
        assert_eq!(
            f1_report(&app, &second).supplemental_input_hash,
            Some(after)
        );
    }

    #[tokio::test]
    async fn gate_inputs_stale_until_consolidate_rerun() {
        let app = app_with(assembly_graph(), &[], chain_gates());
        let stale = |job_id: &Hash, reason: &str| {
            let view = job(&app, job_id);
            assert_eq!(view["status"], "failed", "{view}");
            assert!(
                view["error"]
                    .as_str()
                    .unwrap()
                    .starts_with("E_GATE_INPUTS_STALE: "),
                "{view}"
            );
            let details = failure_details(&view);
            assert_eq!(details["gate"], "F1");
            assert_eq!(details["recipe"], "consolidate");
            assert_eq!(details["reason"], reason);
        };
        let ready_stale = |reason: &str| {
            let err = app.state.ready().unwrap_err();
            let body = err.body();
            assert_eq!(body.code, "E_GATE_INPUTS_STALE");
            let details = serde_json::to_value(&body.details).unwrap();
            assert_eq!(details["gate"], "F1");
            assert_eq!(details["recipe"], "consolidate");
            assert_eq!(details["reason"], reason);
        };
        // No consolidate run yet: ready and lint both name consolidate. (A failed recipe job
        // is identified by its capture, so lint is re-run below only on a moved head.)
        ready_stale("no_completed_run");
        stale(&run_inline(&app, Recipe::Lint, None), "no_completed_run");
        assert_eq!(
            job(&app, &run_inline(&app, Recipe::Consolidate, None))["status"],
            "ready"
        );
        // ready now gets past F1 (F2 then names its own analyse recipe).
        let err = app.state.ready().unwrap_err();
        let details = serde_json::to_value(&err.body().details).unwrap();
        assert_eq!(
            (details["gate"].as_str(), details["recipe"].as_str()),
            (Some("F2"), Some("analyse"))
        );
        // The graph moves: the latest consolidate artifact no longer answers the current request.
        accept_requirement(&app, "req:b");
        ready_stale("inference_request_changed");
        stale(
            &run_inline(&app, Recipe::Lint, None),
            "inference_request_changed",
        );
        // Re-running consolidate (the ordinary product operation) repairs it on the next head.
        assert_eq!(
            job(&app, &run_inline(&app, Recipe::Consolidate, None))["status"],
            "ready"
        );
        let err = app.state.ready().unwrap_err();
        assert_eq!(
            serde_json::to_value(&err.body().details).unwrap()["gate"],
            "F2"
        );
    }

    /// An Accepted Invariant over the Employee entity (its bindings are external).
    fn invariant(node_id: &str, scope: &str) -> Node {
        node(
            node_id,
            "Accepted",
            "Invariant",
            json!({"scope_ref": scope, "expression": "true"}),
        )
    }

    #[tokio::test]
    async fn gate_inputs_missing_names_external_element() {
        let mut graph = data_graph(&["Email"]);
        graph = Graph::new(
            id(PROJECT),
            id(PROFILE),
            graph
                .nodes()
                .values()
                .cloned()
                .chain([
                    invariant("inv:b", "entity:employee"),
                    invariant("inv:a", "entity:employee"),
                ])
                .collect(),
            graph.edges().values().cloned().collect(),
        )
        .unwrap();
        let app = app_with(graph, &[], chain_gates());
        let err = app.state.ready().unwrap_err();
        let body = err.body();
        assert_eq!(body.code, "E_GATE_INPUTS_MISSING");
        let details = serde_json::to_value(&body.details).unwrap();
        assert_eq!(details["gate"], "F2");
        assert_eq!(details["field"], "expression_scopes");
        assert_eq!(details["elements"], json!(["inv:a", "inv:b"]));
    }

    #[test]
    fn gate_config_rejects_graph_derived_restatement() {
        let lint_policy = json!({"severity_overrides": {}});
        let f4 = json!({
            "in_scope_scenario_refs": [], "required_scenario_kinds": [],
            "visible_assumption_refs": []
        });
        let valid = json!({"f1": {"lint_policy": lint_policy}, "f4": f4});
        serde_json::from_value::<GateInputsConfig>(valid).unwrap();
        let restated = [
            json!({"f1": {"lint_policy": lint_policy, "requirements": []}}),
            json!({"f1": {"lint_policy": lint_policy, "analysis_findings": []}}),
            json!({"f2": {"analysis_findings": [], "calculation_scope_overrides": [],
                          "decision_tables": [], "invariants": []}}),
            json!({"f4": {"applicability": {}, "visible_assumption_refs": []}}),
            json!({"f4": {
                "in_scope_scenario_refs": [], "required_scenario_kinds": [],
                "visible_assumption_refs": [], "blocking_scenario_refs": []
            }}),
            json!({"f4": {
                "in_scope_scenario_refs": [], "required_scenario_kinds": [],
                "visible_assumption_refs": [], "source_refs": []
            }}),
        ];
        for config in restated {
            let err = serde_json::from_value::<GateInputsConfig>(config.clone()).unwrap_err();
            assert!(err.to_string().contains("unknown field"), "{config}: {err}");
        }
        // The F1 policy itself stays strict.
        let err = serde_json::from_value::<GateInputsConfig>(
            json!({"f1": {"lint_policy": {"severity_overrides": {}, "extra": 1}}}),
        )
        .unwrap_err();
        assert!(err.to_string().contains("lint_policy"), "{err}");
    }

    #[tokio::test]
    async fn analyse_always_evaluates_assembled_f2_and_reports_genuinely_missing_inputs() {
        // Fable #190: no F2 configuration exists any more, yet Analyse evaluates F2 from the
        // assembled inputs (here the durable S2.1/S2.2 analyses of the Accepted Requirement).
        let app = app_with(assembly_graph(), &[], Gates::default());
        let job_id = run_inline(&app, Recipe::Analyse, None);
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "ready", "{view}");
        assert_eq!(
            stage_store(&app)
                .steps(&job_id)
                .unwrap()
                .iter()
                .map(|s| s.phase.as_str())
                .collect::<Vec<_>>(),
            ["analyse", "f2"]
        );
        assert_eq!(
            app.state.recipe_gate_results(&job_id).unwrap(),
            vec![("F2".to_owned(), "pass".to_owned())]
        );

        // A genuine missing external input (an Accepted Invariant without explicit bindings)
        // fails the same recipe explicitly instead of skipping F2.
        let base = assembly_graph();
        let graph = Graph::new(
            id(PROJECT),
            id(PROFILE),
            base.nodes()
                .values()
                .cloned()
                .chain([
                    node(
                        "entity:request",
                        "Accepted",
                        "Entity",
                        json!({"name": "LeaveRequest", "description": null, "aggregate_root": null}),
                    ),
                    invariant("inv:request", "entity:request"),
                ])
                .collect(),
            vec![],
        )
        .unwrap();
        let app = app_with(graph, &[], Gates::default());
        let job_id = run_inline(&app, Recipe::Analyse, None);
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "failed", "{view}");
        assert!(
            view["error"]
                .as_str()
                .unwrap()
                .starts_with("E_GATE_INPUTS_MISSING: "),
            "{view}"
        );
        let details = failure_details(&view);
        assert_eq!(details["gate"], "F2");
        assert_eq!(details["elements"], json!(["inv:request"]));
    }

    // ------------------------------------------------------------------ rules

    #[tokio::test]
    async fn http_stages_rules_is_analysis_only() {
        let app = app(data_graph(&["Email"]));
        let before = head(&app).revision.id;
        let job_id = run_inline(&app, Recipe::Rules, None);
        // No Accepted DecisionTable: an empty target set completes with no step.
        assert_eq!(job(&app, &job_id)["status"], "ready");
        assert!(stage_store(&app).steps(&job_id).unwrap().is_empty());
        assert_eq!(head(&app).revision.id, before);
    }

    // ------------------------------------------------------------------ R0.2 provider optionality

    fn null_gates() -> Gates {
        Gates {
            null_provider: true,
            ..Gates::default()
        }
    }

    /// The persisted CompileRun of step `ordinal` of `job_id`.
    fn step_run(app: &App, job_id: &Hash, ordinal: usize) -> plumb_compiler::CompileRun {
        let run_ref = stage_store(app).steps(job_id).unwrap()[ordinal]
            .compile_run_ref
            .clone()
            .unwrap();
        let artifact = SqliteArtifactStore::open(&app.path)
            .unwrap()
            .get(&run_ref)
            .unwrap();
        assert_eq!(artifact.kind, ArtifactKind::CompileRun);
        serde_json::from_slice(&artifact.bytes).unwrap()
    }

    fn exists(app: &App, hash: &Hash) -> bool {
        SqliteArtifactStore::open(&app.path)
            .unwrap()
            .exists(hash)
            .unwrap()
    }

    fn request_ref(request: &InferenceRequest) -> Hash {
        Hash::content_sha256(&to_canonical_json(request).unwrap())
    }

    /// The `{details}` JSON suffix of a stored recipe failure.
    fn failure_details(view: &Value) -> Value {
        let error = view["error"].as_str().unwrap();
        serde_json::from_str(&error[error.find('{').unwrap()..]).unwrap()
    }

    fn policy() -> ProviderPolicy {
        ProviderPolicy {
            provider: "test".into(),
            config: CanonicalJson::new(json!({})),
        }
    }

    #[tokio::test]
    async fn null_provider_extract_uses_deterministic_segmentation_fallback() {
        let source = imported();
        let mut nodes = agents();
        nodes.push(source.source.clone());
        nodes.extend(source.fragments.clone());
        let graph = Graph::new(id(PROJECT), id(PROFILE), nodes, vec![]).unwrap();
        let app = app_with(
            graph,
            &[
                source.original_artifact.clone(),
                source.extracted_artifact.clone(),
            ],
            null_gates(),
        );
        let before = head(&app).revision.id;
        let job_id = run_inline(&app, Recipe::Extract, None);
        // The provider is never called under null.
        assert!(app.provider.calls.lock().unwrap().is_empty());

        // Segmentation: the existing deterministic fallback with MissingInference.
        let fragments = source.fragments.clone();
        let request = plumb_import::build_segmentation_request(&fragments, policy()).unwrap();
        let expected = plumb_import::evaluate_segmentation(
            &request,
            &fragments,
            None,
            &plumb_import::SegmentationAudit {
                created_by: id(STAGE_ACTOR),
                created_at: ts(T0),
            },
        )
        .unwrap();
        assert_eq!(expected.mode, plumb_import::SegmentationMode::Fallback);
        assert_eq!(
            expected.fallback_reason,
            Some(plumb_import::FallbackReason::MissingInference)
        );
        let steps = stage_store(&app).steps(&job_id).unwrap();
        assert_eq!(steps[0].phase.as_str(), "segmentation");
        let projection = SqliteArtifactStore::open(&app.path)
            .unwrap()
            .get(steps[0].intermediate_artifact_ref.as_ref().unwrap())
            .unwrap();
        let stored: plumb_api::stage::SegmentationIntermediate =
            serde_json::from_slice(&projection.bytes).unwrap();
        assert_eq!(stored.mode, plumb_import::SegmentationMode::Fallback);
        assert_eq!(
            stored.fallback_reason,
            Some(plumb_import::FallbackReason::MissingInference)
        );
        assert_eq!(stored.candidates, expected.candidates);
        assert_eq!(stored.fallback_derivation, expected.fallback_derivation);
        assert!(
            stored.fallback_derivation.is_some(),
            "the recovery derivation"
        );

        // The run records the request as declared absent; no inference artifact exists.
        let run = step_run(&app, &job_id, 0);
        assert_eq!(
            run.declared_absent_inference_requests,
            vec![request.request.id.clone()]
        );
        assert!(run.inference_artifacts.is_empty());
        assert!(!exists(&app, &request_ref(&request.request)));

        // Classification is Required: the recipe fails at its front with E_INFERENCE_REQUIRED.
        assert!(stored.has_requirement_candidate());
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "failed", "{view}");
        assert!(view["error"]
            .as_str()
            .unwrap()
            .starts_with("E_INFERENCE_REQUIRED: "));
        assert_eq!(
            failure_details(&view),
            json!({"recipe": "extract", "phase": "classification",
                   "task_kinds": ["requirement_classification"]})
        );
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].phase.as_str(), "classification");
        assert_eq!(head(&app).revision.id, before);
    }

    #[tokio::test]
    async fn null_provider_required_phase_fails_with_inference_required_before_provider_call() {
        let graph = || {
            let mut nodes = agents();
            nodes.push(requirement(
                "req:a",
                "Accepted",
                "Employees shall submit leave requests.",
            ));
            Graph::new(id(PROJECT), id(PROFILE), nodes, vec![]).unwrap()
        };
        // Consolidate needs segment-origin requirements to plan; its two Required kinds are
        // covered with every other kind by the acquisition unit tests, classification by the
        // extract test above.
        for (recipe, phase, kinds) in [(
            Recipe::Analyse,
            "analyse",
            json!(["domain_extraction", "lifecycle_extraction"]),
        )] {
            let app = app_with(graph(), &[], null_gates());
            let before = head(&app).revision;
            let job_id = run_inline(&app, recipe, None);
            let view = job(&app, &job_id);
            assert_eq!(view["status"], "failed", "{view}");
            assert!(view["error"]
                .as_str()
                .unwrap()
                .starts_with("E_INFERENCE_REQUIRED: "));
            assert_eq!(
                failure_details(&view),
                json!({"recipe": recipe.as_str(), "phase": phase, "task_kinds": kinds})
            );
            // Zero provider calls.
            assert!(app.provider.calls.lock().unwrap().is_empty());
            // The child failed at the front of ACQUIRE: failed job and step records only.
            let steps = stage_store(&app).steps(&job_id).unwrap();
            assert_eq!(steps.len(), 1);
            assert_eq!(steps[0].compile_run_ref, None);
            assert_eq!(steps[0].intermediate_artifact_ref, None);
            let child = stage_store(&app)
                .stage_job(steps[0].stage_job_id.as_ref().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(child.status, StageStatus::Failed);
            assert_eq!(child.compile_run_ref, None);
            // No request, raw-response or validated-inference artifact, no proposal, no
            // revision.
            let g = head(&app).graph;
            let requests: Vec<InferenceRequest> = vec![
                plumb_functional::build_domain_request(&g, policy())
                    .unwrap()
                    .request,
                plumb_functional::build_lifecycle_request(&g, policy())
                    .unwrap()
                    .request,
            ];
            for r in &requests {
                assert!(!exists(&app, &request_ref(r)), "{}", r.task_kind);
            }
            assert!(stage_store(&app)
                .pending_proposal_links()
                .unwrap()
                .is_empty());
            assert!(pending(&app, &job_id).is_empty());
            let after = head(&app).revision;
            assert_eq!(after.id, before.id);
            assert_eq!(after.semantic_hash, before.semantic_hash);
        }
    }

    #[tokio::test]
    async fn null_provider_tags_keep_dictionary_proposals_and_leave_inference_targets_unresolved() {
        let app = app_with(data_graph(&["Email", "Notes"]), &[], null_gates());
        // An inferred class would be returned if the provider were (wrongly) called.
        *app.provider.classes.lock().unwrap() = vec![("attr:1".into(), "confidential".into())];
        let job_id = run_inline(&app, Recipe::Tags, None);
        assert!(app.provider.calls.lock().unwrap().is_empty());
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "committed", "{view}");
        // The dictionary hit commits; the inference-only attribute stays unresolved.
        assert_eq!(
            data_classes(&app),
            vec![
                ("attr:0".into(), Some("pii".into())),
                ("attr:1".into(), None)
            ]
        );
        let request =
            plumb_functional::build_data_class_request(&data_graph(&["Email", "Notes"]), policy())
                .unwrap();
        let run = step_run(&app, &job_id, 0);
        assert_eq!(
            run.declared_absent_inference_requests,
            vec![request.request.id.clone()]
        );
        assert!(run.inference_artifacts.is_empty());
        // The same deterministic result as the analyser without inference.
        let direct = plumb_functional::analyze_data_classification(
            &data_graph(&["Email", "Notes"]),
            &request,
            None,
        )
        .unwrap();
        assert_eq!(direct.proposals.len(), 1);
        assert_eq!(run.output_proposal_refs.len(), 1);
    }

    /// A transition (slot-free skeleton) and a decision table whose row 0 has a value slot.
    fn scenario_graph() -> Graph {
        let mut nodes = agents();
        nodes.extend([
            node(
                "entity:request",
                "Accepted",
                "Entity",
                json!({"name": "LeaveRequest", "description": null, "aggregate_root": null}),
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
            node(
                "op:submit",
                "Accepted",
                "Operation",
                json!({
                    "name": "op:submit", "operation_kind": "command", "input_schema_ref": null,
                    "output_schema_ref": null, "preconditions": null, "postconditions": null,
                    "idempotency": null, "transaction_semantics": null
                }),
            ),
            node(
                "table:days",
                "Accepted",
                "DecisionTable",
                json!({"name": "Days", "hit_policy": "FIRST", "inputs": [], "outputs": [], "rows": []}),
            ),
        ]);
        let edges = vec![
            edge("rel:draft", "has_state", "entity:request", "state:draft"),
            edge(
                "rel:submitted",
                "has_state",
                "entity:request",
                "state:submitted",
            ),
            edge("rel:via", "transitions_via", "trans:submit", "op:submit"),
        ];
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap()
    }

    fn scenario_inputs() -> plumb_functional::ScenarioDerivationInputs {
        serde_json::from_value(json!({
            "decision_tables": [{
                "decision_table_ref": "table:days",
                "spec": {
                    "hit_policy": "FIRST",
                    "inputs": [{"name": "days", "ty": {"type": "int"}, "domain": {"lower": "0", "upper": "30"}}],
                    "outputs": [{"name": "eligible", "ty": {"type": "bool"}, "domain": null}],
                    "rows": [{
                        "inputs": [{"cell": "interval", "interval": {
                            "lower": {"value": "5", "inclusive": false},
                            "upper": {"value": "10", "inclusive": true}}}],
                        "outputs": [{"cell": "bool", "value": true}]
                    }],
                    "default_output": null
                }
            }],
            "boundaries": []
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn null_provider_scenarios_propose_only_slot_free_skeletons() {
        let gates = Gates {
            null_provider: true,
            scenario_derivation: Some(scenario_inputs()),
            ..Gates::default()
        };
        let app = app_with(scenario_graph(), &[], gates);
        let graph = head(&app).graph;
        let inputs = scenario_inputs();
        let skeletons = plumb_functional::analyze_scenarios(&graph, &inputs).unwrap();
        let (free, slotted): (Vec<_>, Vec<_>) =
            skeletons.iter().partition(|s| s.value_slots.is_empty());
        assert!(!free.is_empty() && !slotted.is_empty(), "{skeletons:?}");
        let job_id = run_inline(&app, Recipe::Scenarios, None);
        assert!(app.provider.calls.lock().unwrap().is_empty());
        let view = job(&app, &job_id);
        assert!(
            ["ready", "committed"].contains(&view["status"].as_str().unwrap()),
            "{view}"
        );
        // Exactly the deterministic no-inference derivation: slot skeletons stay unresolved.
        let direct = plumb_functional::derive_scenarios(
            &graph,
            &inputs,
            &plumb_functional::ScenarioAudit {
                created_by: id(STAGE_ACTOR),
                created_at: ts(T0),
            },
            &[],
        )
        .unwrap();
        let mut expected: Vec<Hash> = direct
            .proposals
            .iter()
            .map(|p| Hash::content_sha256(&to_canonical_json(p).unwrap()))
            .collect();
        expected.sort();
        let run = step_run(&app, &job_id, 0);
        assert_eq!(run.output_proposal_refs, expected);
        assert!(!expected.is_empty());
        let mut absent: Vec<Hash> = slotted
            .iter()
            .map(|s| {
                plumb_functional::build_scenario_request(&graph, &inputs, &s.scenario_ref, policy())
                    .unwrap()
                    .request
                    .id
            })
            .collect();
        absent.sort();
        assert_eq!(run.declared_absent_inference_requests, absent);
        assert!(run.inference_artifacts.is_empty());
        for p in &direct.proposals {
            let text = serde_json::to_string(&p.patch_set).unwrap();
            for s in &slotted {
                assert!(
                    !text.contains(s.scenario_ref.as_str()),
                    "slot scenario proposed"
                );
            }
        }
    }

    /// Counts the calls of a wrapped provider.
    struct Counting<P> {
        inner: P,
        calls: Mutex<usize>,
    }

    impl<P: InferenceProvider> InferenceProvider for Counting<P> {
        fn execute(
            &self,
            request: &InferenceRequest,
            material: &InferenceRequestMaterial,
        ) -> Result<ProviderExecution, ProviderError> {
            *self.calls.lock().unwrap() += 1;
            self.inner.execute(request, material)
        }
    }

    #[tokio::test]
    async fn replay_missing_fixture_still_fails_with_inference_failed() {
        let graph = || data_graph(&["Email", "Notes"]);
        // Record the exact execution with the test provider (replay role) ...
        let recording = app(graph());
        *recording.provider.classes.lock().unwrap() =
            vec![("attr:1".into(), "confidential".into())];
        let recorded = run_inline(&recording, Recipe::Tags, None);
        assert_eq!(job(&recording, &recorded)["status"], "ready");
        let executions: Vec<ProviderExecution> = recording
            .provider
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|(_, _, e)| e.clone())
            .collect();
        assert_eq!(executions.len(), 1);
        // ... a real MockProvider with that fixture executes the Fallback request normally.
        let mock = |fixtures: Vec<ProviderExecution>| {
            Arc::new(Counting {
                inner: plumb_inference::MockProvider::with_fixtures(
                    fixtures
                        .into_iter()
                        .map(|e| (e.artifact.request_hash.clone(), e)),
                )
                .unwrap(),
                calls: Mutex::new(0),
            })
        };
        let hit = mock(executions);
        let app = app_using(graph(), &[], Gates::default(), Some(hit.clone()));
        let job_id = run_inline(&app, Recipe::Tags, None);
        assert_eq!(job(&app, &job_id)["status"], "ready");
        assert_eq!(*hit.calls.lock().unwrap(), 1);
        let run = step_run(&app, &job_id, 0);
        assert!(run.declared_absent_inference_requests.is_empty());
        assert_eq!(run.inference_artifacts.len(), 1);

        // A replay miss is a failure, never a deterministic fallback.
        let miss = mock(Vec::new());
        let app = app_using(graph(), &[], Gates::default(), Some(miss.clone()));
        let before = head(&app).revision.id;
        let job_id = run_inline(&app, Recipe::Tags, None);
        let view = job(&app, &job_id);
        assert_eq!(view["status"], "failed", "{view}");
        assert!(view["error"]
            .as_str()
            .unwrap()
            .starts_with("E_INFERENCE_FAILED: "));
        assert_eq!(failure_details(&view), json!({"error": "FixtureNotFound"}));
        assert_eq!(*miss.calls.lock().unwrap(), 1, "the provider was called");
        assert_eq!(
            stage_store(&app).steps(&job_id).unwrap()[0].compile_run_ref,
            None
        );
        assert_eq!(head(&app).revision.id, before);
    }

    #[tokio::test]
    async fn null_provider_session_turn_stays_inference_failed() {
        let mut nodes = agents();
        nodes.push(requirement(
            "req:a",
            "Accepted",
            "Employees shall submit leave requests.",
        ));
        let graph = Graph::new(id(PROJECT), id(PROFILE), nodes, vec![]).unwrap();
        let app = app_using(
            graph,
            &[],
            null_gates(),
            Some(Arc::new(plumb_inference::NullProvider)),
        );
        let tag = head_hash(&app).await;
        let created = post(
            &app,
            "/api/sessions",
            Some(&tag),
            json!({"scope": ["req:a"]}),
        )
        .await;
        assert_eq!(
            created.status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&created.bytes)
        );
        let session = created.json()["id"].as_str().unwrap().to_owned();
        let tag = head_hash(&app).await;
        let r = post(
            &app,
            &format!("/api/sessions/{session}/turns"),
            Some(&tag),
            json!({"text": "Managers shall approve leave requests.", "stage": "S1"}),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_GATEWAY);
        assert_eq!(r.code(), "E_INFERENCE_FAILED");
        assert_eq!(r.json()["details"]["error"], "ProviderDisabled");
    }

    // ------------------------------------------------------------------ R0.3 proposal sets

    /// Two dictionary attributes (one tags child, two outputs) and a transition (one scenarios
    /// child, one output) on one base.
    fn set_graph() -> Graph {
        let mut nodes: Vec<Node> = scenario_graph().nodes().values().cloned().collect();
        let mut edges: Vec<Edge> = scenario_graph().edges().values().cloned().collect();
        nodes.push(node(
            "entity:employee",
            "Accepted",
            "Entity",
            json!({"name": "Employee", "description": null, "aggregate_root": null}),
        ));
        for (i, name) in ["Email", "Salary"].iter().enumerate() {
            nodes.push(attribute(&format!("attr:{i}"), name));
            edges.push(edge(
                &format!("rel:attr{i}"),
                "has_attribute",
                "entity:employee",
                &format!("attr:{i}"),
            ));
        }
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap()
    }

    struct SetFixture {
        app: App,
        tags_job: Hash,
        scenarios_job: Hash,
        tags: Vec<Proposal>,
        scenarios: Vec<Proposal>,
        manual: Proposal,
    }

    async fn set_fixture() -> SetFixture {
        let gates = Gates {
            null_provider: true,
            scenario_derivation: Some(scenario_inputs()),
            ..Gates::default()
        };
        let app = app_with(set_graph(), &[], gates);
        let tags_job = run_inline(&app, Recipe::Tags, None);
        assert_eq!(job(&app, &tags_job)["status"], "ready");
        let scenarios_job = run_inline(&app, Recipe::Scenarios, None);
        assert_eq!(job(&app, &scenarios_job)["status"], "ready");
        let tags = pending(&app, &tags_job);
        let scenarios = pending(&app, &scenarios_job);
        assert_eq!((tags.len(), scenarios.len()), (2, 1));
        // A non-stage proposal through the intake endpoint on the same head.
        let manual = Proposal::new(
            plumb_core::StageId::S1,
            PatchSet {
                base_semantic_hash: head(&app).revision.semantic_hash,
                patch: SemanticPatch::AddNode {
                    node: requirement("req:manual", "Proposed", "HR shall export payroll."),
                },
            },
            vec![],
            vec![],
            plumb_patch::ProposalMateriality::Semantic,
            plumb_patch::AcceptancePolicy::HumanConfirm,
            None,
        )
        .unwrap();
        let tag = head_hash(&app).await;
        let r = post(
            &app,
            "/api/proposals/intake",
            Some(&tag),
            json!({"kind": "create", "payload": manual, "evidence": []}),
        )
        .await;
        assert_eq!(
            r.status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&r.bytes)
        );
        SetFixture {
            app,
            tags_job,
            scenarios_job,
            tags,
            scenarios,
            manual,
        }
    }

    impl SetFixture {
        fn all(&self) -> Vec<&Proposal> {
            let mut all: Vec<&Proposal> = self
                .tags
                .iter()
                .chain(&self.scenarios)
                .chain([&self.manual])
                .collect();
            all.sort_by(|a, b| a.id.cmp(&b.id));
            all
        }

        /// Every member with the action of its stored class.
        fn members(&self) -> Vec<(String, plumb_api::application::ProposalAction)> {
            use plumb_api::application::ProposalAction as A;
            self.all()
                .iter()
                .map(|p| {
                    let record = workflow(&self.app).proposal(&p.id).unwrap().unwrap();
                    let action = match record.proposal_kind.as_str() {
                        "accept" => A::Accept,
                        "create" => A::Create,
                        "link" => A::Link,
                        "merge" => A::Merge,
                        _ => A::Supersede,
                    };
                    (p.id.to_string(), action)
                })
                .collect()
        }

        /// The stored child of recipe job `job_id`.
        fn child(&self, job_id: &Hash) -> plumb_compiler::StageJob {
            let steps = stage_store(&self.app).steps(job_id).unwrap();
            let record = stage_store(&self.app)
                .stage_job(steps[0].stage_job_id.as_ref().unwrap())
                .unwrap()
                .unwrap();
            serde_json::from_str(&record.stage_job_json).unwrap()
        }

        /// Asserts the complete finalised state of the one batch revision `revision`.
        fn assert_finalised(&self, revision: &plumb_store::RevisionId) {
            for p in self.all() {
                let record = workflow(&self.app).proposal(&p.id).unwrap().unwrap();
                assert_eq!(record.state, WorkflowProposalState::Accepted, "{}", p.id);
                assert_eq!(record.resolved_revision_id.as_ref(), Some(revision));
            }
            let refs = |ps: &[Proposal]| {
                let mut r: Vec<Hash> = ps
                    .iter()
                    .map(|p| Hash::content_sha256(&to_canonical_json(p).unwrap()))
                    .collect();
                r.sort();
                r
            };
            for (job_id, outputs) in [
                (&self.tags_job, &self.tags),
                (&self.scenarios_job, &self.scenarios),
            ] {
                let child = self.child(job_id);
                assert_eq!(child.status(), plumb_compiler::StageJobStatus::Committed);
                assert_eq!(child.committed_revision(), Some(revision));
                assert_eq!(child.committed_output_ref(), None);
                assert_eq!(child.committed_output_refs(), refs(outputs).as_slice());
                let view = job(&self.app, job_id);
                assert_eq!(view["status"], "committed", "{view}");
                assert_eq!(view["result_version"], revision.to_string());
            }
            assert!(stage_store(&self.app)
                .proposal_link(&self.manual.id)
                .unwrap()
                .is_none());
        }
    }

    #[tokio::test]
    async fn accept_proposal_set_records_every_stage_child_output() {
        let f = set_fixture().await;
        let before = head(&f.app).revision;
        let write = f
            .app
            .state
            .accept_proposal_set(&head(&f.app), &f.members(), None)
            .unwrap();
        let after = head(&f.app).revision;
        assert_eq!(after.version, before.version + 1);
        assert_eq!(write.revision, after.id.to_string());
        // Two outputs of one child, one of another, and a non-stage member: one revision.
        f.assert_finalised(&after.id);
        assert_eq!(after.decision_refs.len(), 4);
    }

    #[tokio::test]
    async fn accept_proposal_set_failure_leaves_stage_jobs_untouched() {
        let f = set_fixture().await;
        // A misclassified non-stage member after the stage members validated read-only.
        let mut members = f.members();
        for m in members.iter_mut() {
            if m.0 == f.manual.id.as_str() {
                m.1 = plumb_api::application::ProposalAction::Accept;
            }
        }
        let error = f
            .app
            .state
            .accept_proposal_set(&head(&f.app), &members, None)
            .unwrap_err();
        assert_eq!(error.body().code, "E_PROPOSAL_ACTION_MISMATCH");
        for job_id in [&f.tags_job, &f.scenarios_job] {
            assert_eq!(
                f.child(job_id).status(),
                plumb_compiler::StageJobStatus::Ready
            );
            assert_eq!(job(&f.app, job_id)["status"], "ready");
        }
        // A moved head: the stage members are stale, and the rejected set leaves the stale
        // children and parents exactly as stored (no Stale transition by the set).
        let mut store = SqliteRevisionStore::open(&f.app.path).unwrap();
        let loaded = head(&f.app);
        store
            .commit(
                &BranchName::main(),
                &loaded.revision.id,
                &PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch: SemanticPatch::AddNode {
                        node: requirement("req:other", "Proposed", "Another writer."),
                    },
                },
                RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id(ACTOR),
                    created_at: ts(NOW),
                },
            )
            .unwrap();
        // Stage members only, so the read-only stage freshness check meets the stale child.
        let stage_only: Vec<_> = f
            .members()
            .into_iter()
            .filter(|m| m.0 != f.manual.id.as_str())
            .collect();
        let error = f
            .app
            .state
            .accept_proposal_set(&head(&f.app), &stage_only, None)
            .unwrap_err();
        assert_eq!(error.body().code, "E_PROPOSAL_BASELINE_CHANGED");
        for job_id in [&f.tags_job, &f.scenarios_job] {
            assert_eq!(
                f.child(job_id).status(),
                plumb_compiler::StageJobStatus::Ready
            );
            let parent = stage_store(&f.app).recipe_job(job_id).unwrap().unwrap();
            assert_eq!(parent.status, StageStatus::Ready);
        }
        for p in f.all() {
            let state = workflow(&f.app).proposal(&p.id).unwrap().unwrap().state;
            assert_eq!(state, WorkflowProposalState::Pending);
        }
    }

    #[tokio::test]
    async fn accept_proposal_set_crash_is_reconciled_without_recommit() {
        let f = set_fixture().await;
        let loaded = head(&f.app);
        // The exact set commit, then a crash before any bookkeeping.
        let members: Vec<&Proposal> = f.all();
        let companions: Vec<_> = members
            .iter()
            .map(|p| {
                let report = plumb_intake::intake(
                    &loaded.graph,
                    p,
                    &plumb_intake::IntakeValidationInput::empty(),
                )
                .unwrap();
                let codes: std::collections::BTreeSet<String> =
                    report.reason_codes.iter().map(|c| c.to_string()).collect();
                build_acceptance_companion(
                    &loaded.graph,
                    p,
                    &AcceptanceInput {
                        reason_codes: codes.into_iter().collect(),
                        decided_by: id(ACTOR),
                        decided_at: ts(NOW),
                        rationale: None,
                    },
                )
                .unwrap()
            })
            .collect();
        let mut artifacts = SqliteArtifactStore::open(&f.app.path).unwrap();
        for c in &companions {
            artifacts
                .put(
                    ArtifactKind::Patch,
                    "application/json",
                    &c.original_patch_bytes,
                    ts(NOW),
                )
                .unwrap();
        }
        let envelope = PatchSet {
            base_semantic_hash: loaded.revision.semantic_hash.clone(),
            patch: SemanticPatch::Compound {
                patches: members
                    .iter()
                    .map(|p| p.patch_set.patch.clone())
                    .chain(companions.iter().map(|c| SemanticPatch::AddNode {
                        node: c.decision_node.clone(),
                    }))
                    .collect(),
            },
        };
        let mut decision_refs: Vec<Id> =
            companions.iter().map(|c| c.decision_ref.clone()).collect();
        decision_refs.sort();
        let committed = SqliteRevisionStore::open(&f.app.path)
            .unwrap()
            .commit(
                &BranchName::main(),
                &loaded.revision.id,
                &envelope,
                RevisionWriteMeta {
                    decision_refs,
                    created_by: id(ACTOR),
                    created_at: ts(NOW),
                },
            )
            .unwrap();
        let revision = committed.revision.id.clone();
        // Partial finalisation: one stage member was already resolved before the crash.
        workflow(&f.app)
            .resolve_proposal(
                &f.tags[1].id,
                WorkflowProposalState::Accepted,
                &revision,
                ts(NOW),
            )
            .unwrap();
        // Restart: startup reconciliation finishes everything from the companions.
        let restarted = open(
            &f.app.path,
            f.app.provider.clone(),
            &Gates {
                null_provider: true,
                scenario_derivation: Some(scenario_inputs()),
                ..Gates::default()
            },
        );
        drop(restarted);
        f.assert_finalised(&revision);
        assert_eq!(head(&f.app).revision.id, revision, "never re-committed");
        // Idempotent.
        let again = open(&f.app.path, f.app.provider.clone(), &Gates::default());
        drop(again);
        f.assert_finalised(&revision);
        assert_eq!(head(&f.app).revision.id, revision);
    }
}
