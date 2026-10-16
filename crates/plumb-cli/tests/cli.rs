//! A0.4 CLI tests (Hotfix 067, Hotfix 069): the `plumb-cli` binary over a temporary pilot
//! database. Every command runs in-process over the shared application services; providers are
//! null or replayed fixtures recorded in-process from the exact request material. No test uses
//! the network or the environment.

mod cli {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use assert_cmd::Command;
    use plumb_api::application::{
        bootstrap_project_database, initialize_project_database, open_application,
    };
    use plumb_api::{ApiIdentity, Recipe};
    use plumb_core::{to_canonical_json, CanonicalJson, FixedClock, Hash, Id, Timestamp};
    use plumb_functional::VerificationApplicabilityInputs;
    use plumb_inference::{
        validate_binding, InferenceArtifact, InferenceProvider, InferenceRequest,
        InferenceRequestMaterial, ProviderError, ProviderExecution,
    };
    use plumb_store::{
        BranchName, SqliteRevisionStore, SqliteStageStore, SqliteWorkflowStore,
        WorkflowProposalState,
    };
    use plumb_validation::{load_builtin_software_profile, ValidationPolicy};
    use serde_json::{json, Value};

    const PROJECT: &str = "project:pilot";
    const ACTOR: &str = "agent:cli";
    const ASSISTANT: &str = "agent:assistant";
    const STAGE_ACTOR: &str = "agent:compiler";
    const T0: &str = "2026-01-01T00:00:00.000000000Z";
    const MARKDOWN: &[u8] =
        b"# Leave\n\nEmployees shall submit leave requests.\n\nManagers shall approve leave requests.\n";

    /// The 22 commands with their minimal required arguments.
    const COMMANDS: [&[&str]; 22] = [
        &["init", "--project", "project:pilot"],
        &[
            "import",
            "x.md",
            "--media-type",
            "text/markdown",
            "--name",
            "x.md",
        ],
        &["lint"],
        &["vocab"],
        &["extract", "--scope", "frag:a", "--scope", "frag:b"],
        &["consolidate"],
        &["calc"],
        &["rules"],
        &["analyse"],
        &["questions"],
        &[
            "rounds",
            "create",
            "--stakeholder",
            "stakeholder:a",
            "--max",
            "5",
        ],
        &[
            "apply",
            "proposal:x",
            "--action",
            "reject",
            "--reason",
            "no",
        ],
        &["accept-set", "proposal:x=accept"],
        &["flows"],
        &["scenarios"],
        &["run"],
        &["tags"],
        &["ready"],
        &["export", "--format", "functional"],
        &["report", "--out", "out"],
        &["serve", "--listen", "127.0.0.1:0"],
        &[
            "log-effort",
            "--file",
            "e.jsonl",
            "--kind",
            "analyst_days",
            "--amount",
            "1.5",
        ],
    ];

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    // ------------------------------------------------------------------ fixture

    struct Env {
        dir: tempfile::TempDir,
        db: PathBuf,
        config: PathBuf,
    }

    impl Env {
        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        /// The binary with `--db` and `--config`.
        fn cmd(&self) -> Command {
            let mut cmd = bin();
            cmd.arg("--db")
                .arg(&self.db)
                .arg("--config")
                .arg(&self.config);
            cmd
        }

        /// Runs `args` and returns (exit code, stdout, stderr).
        fn run(&self, args: &[&str]) -> (i32, String, String) {
            output(self.cmd().args(args))
        }

        /// Runs `args --json` and decodes stdout.
        fn json(&self, args: &[&str]) -> (i32, Value) {
            let (code, out, err) = self.run(&[args, &["--json"]].concat());
            let value = serde_json::from_str(&out)
                .unwrap_or_else(|_| panic!("stdout {out:?} stderr {err:?}"));
            (code, value)
        }

        fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.path(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }

        /// Imports the markdown fixture and returns its source ID.
        fn import(&self) -> String {
            let file = self.write("requirements.md", MARKDOWN);
            let (code, value) = self.json(&[
                "import",
                file.to_str().unwrap(),
                "--media-type",
                "text/markdown",
                "--name",
                "requirements.md",
            ]);
            assert_eq!(code, 0, "{value}");
            value["id"].as_str().unwrap().to_owned()
        }
    }

    fn bin() -> Command {
        Command::cargo_bin("plumb-cli").unwrap()
    }

    fn output(cmd: &mut Command) -> (i32, String, String) {
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    }

    /// The configuration with the given gates (every field present).
    fn config(gates: Value) -> Value {
        json!({
            "version": 1,
            "identity": {"user": "alice", "actor_ref": ACTOR, "roles": ["analyst"], "mode": "pilot"},
            "stage_actor_ref": STAGE_ACTOR,
            "assistant_actor_ref": ASSISTANT,
            "provider_policy": {"provider": "test", "config": {}},
            "session_turn": {"retrieval_top_k": 3, "max_open_questions": 3, "max_context_bytes": 65536},
            "validation_policy": serde_json::to_value(ValidationPolicy::default()).unwrap(),
            "calendars": [{"calendar_ref": "cal:office", "weekend": ["saturday", "sunday"], "holidays": ["2026-12-25"]}],
            "direct_managers": [{"manager_ref": "actor:manager", "employee_ref": "actor:employee"}],
            "expression_scopes": [],
            "calculation_scope_overrides": [],
            "runtime_semantic_values": [],
            "vocabulary_policy": {"language": "en"},
            "duplicate_policy": {"jaccard_threshold": "0.8"},
            "calculation_scope": null,
            "operation_scope": null,
            "process_scope": null,
            "authorization_draft": null,
            "scenario_derivation": null,
            "verification_applicability": serde_json::to_value(VerificationApplicabilityInputs::default()).unwrap(),
            "gates": gates,
        })
    }

    /// The external F1 configuration: the lint policy only (R0.4).
    fn f1_inputs() -> Value {
        json!({"lint_policy": {"severity_overrides": {}}})
    }

    /// The external F4 configuration (R0.4): in-scope Scenarios, required kinds per executable
    /// source and visible assumptions; applicability itself comes from S4.5 and the graph.
    fn f4_inputs() -> Value {
        json!({
            "in_scope_scenario_refs": [], "required_scenario_kinds": [],
            "visible_assumption_refs": []
        })
    }

    fn all_gates() -> Value {
        json!({"f1": f1_inputs(), "f4": f4_inputs()})
    }

    /// Commits one Accepted node on the head of `db` through the revision store's CAS commit
    /// (test setup for graph content no CLI command creates yet).
    fn commit_node(db: &Path, node: Value) {
        let mut store = SqliteRevisionStore::open(db).unwrap();
        let head = store.head(&BranchName::main()).unwrap().unwrap();
        let loaded = store.load_revision(&head).unwrap();
        let patch_set = serde_json::from_value(json!({
            "base_semantic_hash": loaded.revision.semantic_hash,
            "patch": {"op": "AddNode", "node": node}
        }))
        .unwrap();
        store
            .commit(
                &BranchName::main(),
                &head,
                &patch_set,
                plumb_store::RevisionWriteMeta {
                    decision_refs: vec![],
                    created_by: id(ACTOR),
                    created_at: ts(T0),
                },
            )
            .unwrap();
    }

    /// A fresh initialized database and a configuration file.
    fn env_with(config: Value) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("plumb.sqlite");
        let profile = load_builtin_software_profile().unwrap();
        initialize_project_database(&db, id(PROJECT), &profile, id(ACTOR), ts(T0)).unwrap();
        let config_path = dir.path().join("plumb.json");
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        Env {
            dir,
            db,
            config: config_path,
        }
    }

    fn env() -> Env {
        env_with(config(all_gates()))
    }

    /// A configuration file and a database path that does not exist yet (R0.1 `init`).
    fn bare_env_with(config: Value) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("plumb.sqlite");
        let config_path = dir.path().join("plumb.json");
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        Env {
            dir,
            db,
            config: config_path,
        }
    }

    fn bare_env() -> Env {
        bare_env_with(config(all_gates()))
    }

    // ------------------------------------------------------------------ recording provider

    /// Answers segmentation and classification from the exact request material and records
    /// every execution (the replay fixtures of the CLI).
    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<ProviderExecution>>,
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

    impl InferenceProvider for Recorder {
        fn execute(
            &self,
            request: &InferenceRequest,
            material: &InferenceRequestMaterial,
        ) -> Result<ProviderExecution, ProviderError> {
            validate_binding(request, material)?;
            let context: Value = serde_json::from_slice(&material.context_json).unwrap();
            let output = match request.task_kind.as_str() {
                "requirement_segmentation" => {
                    let segments: Vec<Value> = context["fragments"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|f| {
                            json!({
                                "fragment_ref": f["fragment_ref"], "start": 0,
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
                                "requirement_kind": "functional", "level": "system",
                            })
                        })
                        .collect();
                    json!({"version": 1, "classifications": classes, "intents": []})
                }
                "domain_extraction" => {
                    json!({"version": 1, "entities": [], "attributes": [], "relationships": []})
                }
                "lifecycle_extraction" => {
                    json!({"version": 1, "states": [], "transitions": [], "invariants": []})
                }
                other => panic!("unexpected task kind {other}"),
            };
            let executed = execution(request, output);
            self.calls.lock().unwrap().push(executed.clone());
            Ok(executed)
        }
    }

    /// Records the executions of `recipe` over a copy of `env`'s database (the CLI later
    /// replays them over the original) and writes them as a replay fixture file.
    fn record(env: &Env, recipe: &str) -> PathBuf {
        let copy = env.path("recording.sqlite");
        std::fs::copy(&env.db, &copy).unwrap();
        let config: Value = serde_json::from_slice(&std::fs::read(&env.config).unwrap()).unwrap();
        let recorder = Arc::new(Recorder::default());
        let cli_config = plumb_cli_config(&config, recorder.clone());
        let state = open_application(
            &copy,
            BranchName::main(),
            ApiIdentity::new(
                "alice".into(),
                id(ACTOR),
                vec!["analyst".into()],
                "pilot".into(),
            )
            .unwrap(),
            Arc::new(FixedClock::new(ts(T0))),
            load_builtin_software_profile().unwrap(),
            cli_config,
        )
        .unwrap();
        let run = state
            .run_stage(Recipe::parse(recipe).unwrap(), None, None)
            .unwrap();
        state.execute_recipe_job(&run.job_id).unwrap();
        let calls = recorder.calls.lock().unwrap().clone();
        assert_eq!(
            calls.len(),
            2,
            "segmentation and classification are recorded"
        );
        env.write("fixtures.json", &serde_json::to_vec(&calls).unwrap())
    }

    /// The in-process runtime matching the configuration file (provider replaced).
    fn plumb_cli_config(
        config: &Value,
        provider: Arc<Recorder>,
    ) -> plumb_api::application::ApplicationRuntime {
        use plumb_api::{CalendarConfig, DirectManagerFact, DirectManagerPredicates};
        use plumb_functional::{ExprCalendarDefinition, ExprStaticCalendarProvider};
        let calendars: Vec<CalendarConfig> =
            serde_json::from_value(config["calendars"].clone()).unwrap();
        let managers: Vec<DirectManagerFact> =
            serde_json::from_value(config["direct_managers"].clone()).unwrap();
        plumb_api::application::ApplicationRuntime {
            inference_provider: provider,
            // The recorder plays the replay role (R0.2: `null` never calls a provider).
            provider_mode: plumb_api::ProviderMode::Replay {
                fixtures_hash: Hash::content_sha256(b"cli recorder"),
            },
            provider_policy: serde_json::from_value(config["provider_policy"].clone()).unwrap(),
            session_turn_config: serde_json::from_value(config["session_turn"].clone()).unwrap(),
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
            vocabulary_policy: serde_json::from_value(config["vocabulary_policy"].clone()).unwrap(),
            duplicate_policy: serde_json::from_value(config["duplicate_policy"].clone()).unwrap(),
            calculation_scope: None,
            operation_scope: None,
            process_scope: None,
            authorization_draft: None,
            scenario_derivation: None,
            verification_applicability: Some(VerificationApplicabilityInputs::default()),
            gates: serde_json::from_value(config["gates"].clone()).unwrap(),
        }
    }

    fn pending_proposals(db: &Path) -> Vec<String> {
        SqliteStageStore::open(db)
            .unwrap()
            .pending_proposal_links()
            .unwrap()
            .into_iter()
            .map(|l| l.proposal_id.to_string())
            .collect()
    }

    fn proposal_state(db: &Path, proposal: &str) -> WorkflowProposalState {
        SqliteWorkflowStore::open(db)
            .unwrap()
            .proposal(&id(proposal))
            .unwrap()
            .unwrap()
            .state
    }

    // ------------------------------------------------------------------ parsing

    #[test]
    fn cli_parses_all_twenty_two_commands() {
        // Without --config every parsed command stops at configuration: exit 1, not a clap
        // error, so each argument tree parses.
        for args in COMMANDS {
            let (code, _, err) = output(bin().args(args));
            assert_eq!(code, 1, "{args:?}: {err}");
            assert!(err.contains("--config is required"), "{args:?}: {err}");
        }
        let (code, out, _) = output(bin().arg("--help"));
        assert_eq!(code, 0);
        let commands: Vec<&str> = out
            .lines()
            .skip_while(|l| !l.starts_with("Commands:"))
            .skip(1)
            .take_while(|l| !l.is_empty())
            .filter_map(|l| l.split_whitespace().next())
            .filter(|c| *c != "help")
            .collect();
        assert_eq!(
            commands,
            [
                "init",
                "import",
                "lint",
                "vocab",
                "extract",
                "consolidate",
                "calc",
                "rules",
                "analyse",
                "questions",
                "rounds",
                "apply",
                "accept-set",
                "flows",
                "scenarios",
                "run",
                "tags",
                "ready",
                "export",
                "report",
                "serve",
                "log-effort"
            ]
        );
    }

    #[test]
    fn cli_argument_errors_are_exit_one() {
        for args in [
            &["unknown"][..],
            &["apply", "proposal:x", "--action", "approve"],
            &["serve", "--listen", "not-an-address"],
            &[
                "log-effort",
                "--file",
                "e",
                "--kind",
                "hours",
                "--amount",
                "1",
            ],
            &["lint", "--provider", "remote"],
        ] {
            let (code, _, err) = output(bin().args(args));
            assert_eq!(code, 1, "{args:?}: {err}");
        }
        let (code, out, _) = output(bin().args(["serve", "--help"]));
        assert_eq!(code, 0);
        assert!(out.contains("[default: 127.0.0.1:8080]"), "{out}");
    }

    #[test]
    fn cli_rounds_create_requires_an_explicit_max() {
        let env = env();
        let (code, _, err) = env.run(&["rounds", "create", "--stakeholder", "stakeholder:a"]);
        assert_eq!(code, 1);
        assert!(err.contains("--max"), "{err}");
        for max in ["0", "1001"] {
            let (code, _, err) = env.run(&[
                "rounds",
                "create",
                "--stakeholder",
                "stakeholder:a",
                "--max",
                max,
            ]);
            assert_eq!(code, 1, "{max}");
            assert!(err.contains("E_INVALID_REQUEST"), "{max}: {err}");
        }
        // 1 and 1000 pass validation and reach the shared round service.
        for max in ["1", "1000"] {
            let (code, _, err) = env.run(&[
                "rounds",
                "create",
                "--stakeholder",
                "stakeholder:a",
                "--max",
                max,
            ]);
            assert_eq!(code, 1, "{max}");
            assert!(err.contains("E_NOT_FOUND"), "{max}: {err}");
            assert!(err.contains("stakeholder stakeholder:a"), "{max}: {err}");
        }
        let (code, _, err) = env.run(&["rounds", "show", "round:missing"]);
        assert_eq!(code, 1);
        assert!(err.contains("E_NOT_FOUND"), "{err}");
    }

    // ------------------------------------------------------------------ bootstrap (R0.1)

    /// The five labelled values of a successful text `init`.
    fn init_fields(out: &str) -> Vec<(String, String)> {
        out.lines()
            .map(|l| {
                let (k, v) = l.split_once(": ").unwrap_or_else(|| panic!("{out}"));
                (k.to_owned(), v.to_owned())
            })
            .collect()
    }

    /// The SQLite sidecar paths of `db`.
    fn sidecars(db: &Path) -> Vec<PathBuf> {
        ["-journal", "-wal", "-shm"]
            .iter()
            .map(|suffix| {
                let mut name = db.as_os_str().to_owned();
                name.push(suffix);
                PathBuf::from(name)
            })
            .collect()
    }

    fn assert_no_database(db: &Path) {
        assert!(db.symlink_metadata().is_err(), "{} exists", db.display());
        for sidecar in sidecars(db) {
            assert!(sidecar.symlink_metadata().is_err(), "{}", sidecar.display());
        }
    }

    /// The semantic and evidence hashes of the legacy empty project graph with the same
    /// project and profile (`initialize_project_database`, unchanged).
    fn empty_graph_hashes(dir: &Path) -> (String, String) {
        let db = dir.join("empty.sqlite");
        let profile = load_builtin_software_profile().unwrap();
        let revision =
            initialize_project_database(&db, id(PROJECT), &profile, id(ACTOR), ts(T0)).unwrap();
        let store = SqliteRevisionStore::open(&db).unwrap();
        assert_eq!(
            store.head(&BranchName::main()).unwrap(),
            Some(revision.clone())
        );
        let loaded = store.load_revision(&revision).unwrap();
        assert!(loaded.graph.nodes().is_empty() && loaded.graph.edges().is_empty());
        assert_eq!(loaded.revision.created_by, id(ACTOR));
        assert_eq!(loaded.revision.created_at, ts(T0));
        (
            loaded.revision.semantic_hash.to_string(),
            loaded.revision.evidence_hash.to_string(),
        )
    }

    #[test]
    fn init_creates_project_database_with_configured_agents() {
        let env = bare_env();
        let (code, out, err) = env.run(&["init", "--project", PROJECT, "--branch", "main"]);
        assert_eq!(code, 0, "{err}");
        assert_eq!(err, "");
        let fields = init_fields(&out);
        let labels: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            labels,
            [
                "project",
                "branch",
                "revision",
                "semantic_hash",
                "evidence_hash"
            ]
        );

        let store = SqliteRevisionStore::open(&env.db).unwrap();
        let head = store.head(&BranchName::main()).unwrap().unwrap();
        let loaded = store.load_revision(&head).unwrap();
        let revision = &loaded.revision;
        assert_eq!(revision.version, 1);
        assert_eq!(revision.parent, None);
        assert_eq!(revision.project_id, id(PROJECT));
        assert_eq!(revision.created_by, id(ACTOR));
        assert!(revision.decision_refs.is_empty());
        assert_eq!(revision.accepted_patch_ref, None);
        let profile = load_builtin_software_profile().unwrap();
        assert_eq!(revision.profile_ref, profile.profile_id);
        assert_eq!(revision.profile_hash, profile.profile_hash().unwrap());
        assert_eq!(revision.rule_pack_hash, profile.rule_pack_hash().unwrap());
        assert_eq!(
            fields.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(),
            [
                PROJECT.to_owned(),
                "main".to_owned(),
                revision.id.to_string(),
                revision.semantic_hash.to_string(),
                revision.evidence_hash.to_string(),
            ]
        );

        // Exactly the three configured Agents, nothing else.
        assert!(loaded.graph.edges().is_empty());
        let nodes = loaded.graph.nodes();
        assert_eq!(
            nodes.keys().map(Id::to_string).collect::<Vec<_>>(),
            [ASSISTANT, STAGE_ACTOR, ACTOR]
                .map(str::to_owned)
                .to_vec()
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        );
        let created_at = serde_json::to_value(revision.created_at).unwrap();
        for (actor, kind) in [
            (ACTOR, "human"),
            (STAGE_ACTOR, "compiler_stage"),
            (ASSISTANT, "llm_model"),
        ] {
            let node = serde_json::to_value(&nodes[&id(actor)]).unwrap();
            assert_eq!(node["id"], actor);
            assert_eq!(node["revision"], 1, "{node}");
            assert_eq!(node["status"], "Accepted", "{node}");
            assert_eq!(
                node["payload"],
                json!({"type": "Agent", "data": {"agent_kind": kind}}),
                "{node}"
            );
            for empty in ["evidence", "derivations", "standards", "tags"] {
                assert_eq!(node[empty], json!([]), "{empty}: {node}");
            }
            assert_eq!(node["extensions"], json!({}), "{node}");
            assert_eq!(node["audit"]["created_by"], ACTOR, "{node}");
            assert_eq!(node["audit"]["created_at"], created_at, "{node}");
            for none in ["updated_by", "updated_at"] {
                assert!(node["audit"].get(none).is_none_or(Value::is_null), "{node}");
            }
        }

        // Agents are excluded from semantic_hash: the genesis hashes equal the empty graph's.
        let (semantic, evidence) = empty_graph_hashes(env.dir.path());
        assert_eq!(revision.semantic_hash.to_string(), semantic);
        assert_eq!(revision.evidence_hash.to_string(), evidence);

        // --json exposes exactly the same five fields.
        let other = env.path("json.sqlite");
        let (code, out, err) = output(
            bin()
                .arg("--db")
                .arg(&other)
                .arg("--config")
                .arg(&env.config)
                .args(["init", "--project", PROJECT, "--json"]),
        );
        assert_eq!(code, 0, "{err}");
        let value: Value = serde_json::from_str(&out).unwrap();
        let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "branch",
                "evidence_hash",
                "project_id",
                "revision_id",
                "semantic_hash"
            ]
        );
        assert_eq!(value["project_id"], PROJECT);
        assert_eq!(value["branch"], "main");
        assert_eq!(value["semantic_hash"], semantic.as_str());
        assert_eq!(value["evidence_hash"], evidence.as_str());
    }

    #[test]
    fn init_bootstrap_service_reads_the_clock_once() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("service.sqlite");
        let identity = ApiIdentity::new(
            "alice".into(),
            id(ACTOR),
            vec!["analyst".into()],
            "pilot".into(),
        )
        .unwrap();
        let result = bootstrap_project_database(
            &db,
            id(PROJECT),
            &load_builtin_software_profile().unwrap(),
            &identity,
            &id(STAGE_ACTOR),
            &id(ASSISTANT),
            &FixedClock::new(ts(T0)),
        )
        .unwrap();
        let store = SqliteRevisionStore::open(&db).unwrap();
        let loaded = store.load_revision(&result.revision_id).unwrap();
        assert_eq!(loaded.revision.created_at, ts(T0));
        for node in loaded.graph.nodes().values() {
            assert_eq!(node.audit.created_at, ts(T0));
            assert_eq!(node.audit.created_by, id(ACTOR));
        }
        assert_eq!(
            result.semantic_hash.to_string(),
            empty_graph_hashes(dir.path()).0
        );
        // The service itself rejects duplicate actors before creating anything.
        let duplicate = dir.path().join("duplicate.sqlite");
        let error = bootstrap_project_database(
            &duplicate,
            id(PROJECT),
            &load_builtin_software_profile().unwrap(),
            &identity,
            &id(ACTOR),
            &id(ASSISTANT),
            &FixedClock::new(ts(T0)),
        )
        .unwrap_err();
        assert!(error.to_string().contains("pairwise distinct"), "{error}");
        assert_no_database(&duplicate);
    }

    #[test]
    fn init_refuses_existing_path_without_writing() {
        // An initialized SQLite database, an empty file and a non-SQLite file.
        let env = env();
        let initialized = std::fs::read(&env.db).unwrap();
        let empty = env.write("empty.sqlite", b"");
        let text = env.write("text.sqlite", b"not a database\n");
        for (path, bytes) in [
            (env.db.clone(), initialized),
            (empty, Vec::new()),
            (text, b"not a database\n".to_vec()),
        ] {
            let (code, out, err) = output(
                bin()
                    .arg("--db")
                    .arg(&path)
                    .arg("--config")
                    .arg(&env.config)
                    .args(["init", "--project", PROJECT]),
            );
            assert_eq!(code, 1, "{}: {out}", path.display());
            assert_eq!(out, "");
            assert!(err.contains("cannot create"), "{err}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{}", path.display());
            for sidecar in sidecars(&path) {
                assert!(sidecar.symlink_metadata().is_err(), "{}", sidecar.display());
            }
        }
        // The legacy database is still the unchanged initialized project.
        let (code, value) = env.json(&["vocab"]);
        assert_eq!((code, value), (0, json!([])));

        // A directory.
        let directory = env.path("directory.sqlite");
        std::fs::create_dir(&directory).unwrap();
        let (code, _, err) = output(
            bin()
                .arg("--db")
                .arg(&directory)
                .arg("--config")
                .arg(&env.config)
                .args(["init", "--project", PROJECT]),
        );
        assert_eq!(code, 1, "{err}");
        assert!(directory.is_dir());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);

        // A stray SQLite sidecar without its database.
        let stray = env.path("stray.sqlite");
        let wal = env.write("stray.sqlite-wal", b"stale wal");
        let (code, _, err) = output(
            bin()
                .arg("--db")
                .arg(&stray)
                .arg("--config")
                .arg(&env.config)
                .args(["init", "--project", PROJECT]),
        );
        assert_eq!(code, 1, "{err}");
        assert!(err.contains("already exists"), "{err}");
        assert!(stray.symlink_metadata().is_err());
        assert_eq!(std::fs::read(&wal).unwrap(), b"stale wal");
    }

    #[test]
    fn init_failure_leaves_no_database_file() {
        // Rejections before the path is reserved.
        let env = bare_env();
        for (args, message) in [
            (
                &["init", "--project", PROJECT, "--branch", "feature"][..],
                "only branch main",
            ),
            (&["init", "--project", "not an id"], "is not a project ID"),
            (&["init"], "--project"),
        ] {
            let (code, out, err) = env.run(args);
            assert_eq!(code, 1, "{args:?}: {out}");
            assert!(err.contains(message), "{args:?}: {err}");
            assert_no_database(&env.db);
        }
        let (code, _, err) =
            output(
                bin()
                    .arg("--config")
                    .arg(&env.config)
                    .args(["init", "--project", PROJECT]),
            );
        assert_eq!(code, 1);
        assert!(err.contains("--db is required"), "{err}");
        let (code, _, err) =
            output(
                bin()
                    .arg("--db")
                    .arg(&env.db)
                    .args(["init", "--project", PROJECT]),
            );
        assert_eq!(code, 1);
        assert!(err.contains("--config is required"), "{err}");
        assert_no_database(&env.db);

        // Pairwise-distinct configured actors are checked before anything is created.
        for (field, value) in [
            ("stage_actor_ref", ACTOR),
            ("assistant_actor_ref", ACTOR),
            ("assistant_actor_ref", STAGE_ACTOR),
        ] {
            let mut duplicate = config(all_gates());
            duplicate[field] = json!(value);
            let env = bare_env_with(duplicate);
            let (code, _, err) = env.run(&["init", "--project", PROJECT]);
            assert_eq!(code, 1, "{field}");
            assert!(err.contains("pairwise distinct"), "{field}: {err}");
            assert_no_database(&env.db);
        }

        // A failure after the path is reserved: the file is created, then SQLite cannot open a
        // path longer than its pathname limit; the attempt removes what it created.
        let mut deep = env.dir.path().to_path_buf();
        for i in 0..4 {
            deep.push(format!("{i}{}", "d".repeat(150)));
        }
        std::fs::create_dir_all(&deep).unwrap();
        let long = deep.join("plumb.sqlite");
        assert!(long.as_os_str().len() > 600);
        let (code, out, err) = output(
            bin()
                .arg("--db")
                .arg(&long)
                .arg("--config")
                .arg(&env.config)
                .args(["init", "--project", PROJECT]),
        );
        assert_eq!(code, 1, "{out}");
        assert!(!err.contains("cannot create"), "{err}");
        assert_no_database(&long);
        assert_eq!(std::fs::read_dir(&deep).unwrap().count(), 0);
    }

    #[test]
    fn init_database_opens_for_every_command() {
        let env = bare_env();
        // Before init, ordinary commands report the missing database.
        let (code, _, err) = env.run(&["vocab"]);
        assert_eq!(code, 1);
        assert!(err.contains("does not exist"), "{err}");
        let (code, _, err) = env.run(&["init", "--project", PROJECT]);
        assert_eq!(code, 0, "{err}");
        let source = env.write("x.md", MARKDOWN);
        let out = env.path("out");
        // Every database command reaches its application logic over the new database.
        let runs: Vec<(Vec<&str>, i32, &str)> = vec![
            (vec!["vocab"], 0, ""),
            (vec!["questions"], 0, ""),
            (vec!["rounds", "show", "round:missing"], 1, "E_NOT_FOUND"),
            (
                vec![
                    "rounds",
                    "create",
                    "--stakeholder",
                    "stakeholder:a",
                    "--max",
                    "5",
                ],
                1,
                "E_NOT_FOUND",
            ),
            (
                vec![
                    "apply",
                    "proposal:x",
                    "--action",
                    "reject",
                    "--reason",
                    "no",
                ],
                1,
                "E_NOT_FOUND",
            ),
            // The projection needs the existing F4 gate report (a domain result, not R0.1).
            (
                vec!["export", "--format", "functional"],
                1,
                "E_GATE_REPORT_UNAVAILABLE",
            ),
            (
                vec!["report", "--out", out.to_str().unwrap()],
                1,
                "E_GATE_REPORT_UNAVAILABLE",
            ),
            (
                vec![
                    "import",
                    source.to_str().unwrap(),
                    "--media-type",
                    "text/markdown",
                    "--name",
                    "x.md",
                ],
                0,
                "",
            ),
        ];
        for (args, expected, message) in runs {
            let (code, _, err) = env.run(&args);
            assert_eq!(code, expected, "{args:?}: {err}");
            assert!(err.contains(message), "{args:?}: {err}");
            assert!(!err.contains("does not exist"), "{args:?}: {err}");
        }
        // The recipe commands and ready reach the stage and gate services (their outcomes on a
        // genesis graph are the existing domain results, not database errors).
        for recipe in [
            "lint",
            "extract",
            "consolidate",
            "calc",
            "rules",
            "analyse",
            "flows",
            "scenarios",
            "run",
            "tags",
        ] {
            let (code, out, err) = env.run(&[recipe]);
            assert!(
                out.starts_with("job ") || err.contains("E_"),
                "{recipe}: {code} {out} {err}"
            );
            assert!(!err.contains("does not exist"), "{recipe}: {err}");
        }
        let (code, out, err) = env.run(&["ready"]);
        assert!(code == 0 || code == 2, "{out} {err}");
        assert!(out.starts_with("revision rev:"), "{out}");
        // serve opens the same application state.
        open_application(
            &env.db,
            BranchName::main(),
            ApiIdentity::new(
                "alice".into(),
                id(ACTOR),
                vec!["analyst".into()],
                "pilot".into(),
            )
            .unwrap(),
            Arc::new(FixedClock::new(ts(T0))),
            load_builtin_software_profile().unwrap(),
            plumb_cli_config(
                &serde_json::from_slice(&std::fs::read(&env.config).unwrap()).unwrap(),
                Arc::new(Recorder::default()),
            ),
        )
        .unwrap();
    }

    // ------------------------------------------------------------------ configuration

    #[test]
    fn cli_configuration_is_strict() {
        let mut unknown = config(all_gates());
        unknown["extra"] = json!(1);
        let mut missing = config(all_gates());
        missing.as_object_mut().unwrap().remove("process_scope");
        let mut gate_unknown = config(all_gates());
        gate_unknown["gates"]["f3"] = Value::Null;
        // R0.4: graph-derived gate truth is never configuration.
        let mut f2_restated = config(all_gates());
        f2_restated["gates"]["f2"] = json!({
            "analysis_findings": [], "calculation_scope_overrides": [],
            "decision_tables": [], "invariants": []
        });
        let mut f1_restated = config(all_gates());
        f1_restated["gates"]["f1"]["requirements"] = json!([]);
        let mut f4_restated = config(all_gates());
        f4_restated["gates"]["f4"]["blocking_scenario_refs"] = json!([]);
        let mut version = config(all_gates());
        version["version"] = json!(2);
        let mut calendar = config(all_gates());
        calendar["calendars"][0]["holidays"] = json!(["2026-02-30"]);
        let mut managers = config(all_gates());
        managers["direct_managers"] = json!([
            {"manager_ref": "actor:a", "employee_ref": "actor:e"},
            {"manager_ref": "actor:b", "employee_ref": "actor:e"}
        ]);
        for (name, value) in [
            ("unknown", unknown),
            ("missing", missing),
            ("gate_unknown", gate_unknown),
            ("f2_restated", f2_restated),
            ("f1_restated", f1_restated),
            ("f4_restated", f4_restated),
            ("version", version),
            ("calendar", calendar),
            ("managers", managers),
        ] {
            let env = env_with(value);
            let (code, _, err) = env.run(&["vocab"]);
            assert_eq!(code, 1, "{name}: {err}");
            assert!(err.contains("configuration"), "{name}: {err}");
        }
        let env = env();
        let (code, _, err) = output(bin().arg("--config").arg(&env.config).arg("vocab"));
        assert_eq!(code, 1);
        assert!(err.contains("--db is required"), "{err}");
    }

    // ------------------------------------------------------------------ providers

    #[test]
    fn cli_provider_options_conflict() {
        let env = env();
        let fixtures = env.write("fixtures.json", b"[]");
        let key = env.write("key", b"k\n");
        let f = fixtures.to_str().unwrap();
        let k = key.to_str().unwrap();
        for args in [
            &["--provider", "replay", "vocab"][..],
            &["--replay-fixtures", f, "vocab"],
            &[
                "--provider",
                "replay",
                "--replay-fixtures",
                f,
                "--anthropic-api-key-file",
                k,
                "vocab",
            ],
            &["--provider", "anthropic", "vocab"],
            &["--anthropic-api-key-file", k, "vocab"],
            &["--provider", "null", "--replay-fixtures", f, "vocab"],
        ] {
            let (code, _, err) = env.run(args);
            assert_eq!(code, 1, "{args:?}: {err}");
            assert!(err.contains("--"), "{args:?}: {err}");
        }
        // Replay fixtures are strict: duplicates and unknown fields are rejected.
        let bad = env.write("bad.json", br#"[{"unknown": 1}]"#);
        let (code, _, err) = env.run(&[
            "--provider",
            "replay",
            "--replay-fixtures",
            bad.to_str().unwrap(),
            "vocab",
        ]);
        assert_eq!(code, 1);
        assert!(err.contains("replay fixtures"), "{err}");
        let (code, _, _) = env.run(&["--provider", "replay", "--replay-fixtures", f, "vocab"]);
        assert_eq!(code, 0);
    }

    #[cfg(not(feature = "llm"))]
    #[test]
    fn cli_anthropic_without_llm_is_provider_unavailable() {
        let env = env();
        let key = env.write("key", b"secret-value\n");
        let (code, out, err) = env.run(&[
            "--provider",
            "anthropic",
            "--anthropic-api-key-file",
            key.to_str().unwrap(),
            "vocab",
        ]);
        assert_eq!(code, 1);
        assert!(err.contains("E_PROVIDER_UNAVAILABLE"), "{err}");
        assert!(!out.contains("secret-value") && !err.contains("secret-value"));
    }

    #[cfg(feature = "llm")]
    #[test]
    fn cli_anthropic_key_file_is_validated_and_never_printed() {
        let env = env();
        for (name, bytes) in [
            ("empty", &b""[..]),
            ("newline", b"\n"),
            ("leading", b" secret-value\n"),
            ("trailing", b"secret-value \n"),
            ("two_newlines", b"secret-value\n\n"),
            ("control", b"secret\tvalue\n"),
        ] {
            let key = env.write(name, bytes);
            let (code, out, err) = env.run(&[
                "--provider",
                "anthropic",
                "--anthropic-api-key-file",
                key.to_str().unwrap(),
                "vocab",
            ]);
            assert_eq!(code, 1, "{name}: {err}");
            assert!(err.contains("anthropic key file"), "{name}: {err}");
            assert!(!out.contains("secret") && !err.contains("secret"), "{name}");
        }
        for (name, bytes) in [
            ("lf", &b"secret-value\n"[..]),
            ("crlf", b"secret-value\r\n"),
            ("bare", b"secret-value"),
        ] {
            let key = env.write(name, bytes);
            let (code, out, err) = env.run(&[
                "--provider",
                "anthropic",
                "--anthropic-api-key-file",
                key.to_str().unwrap(),
                "vocab",
                "--json",
            ]);
            assert_eq!(code, 0, "{name}: {err}");
            assert!(!out.contains("secret") && !err.contains("secret"), "{name}");
        }
    }

    // ------------------------------------------------------------------ shared services

    #[test]
    fn cli_import_vocab_questions_use_the_shared_services() {
        let env = env();
        let source = env.import();
        assert!(source.starts_with("src:"), "{source}");
        // The same file again is the shared service's conflict (its audit differs).
        let file = env.path("requirements.md");
        let (code, _, err) = env.run(&[
            "import",
            file.to_str().unwrap(),
            "--media-type",
            "text/markdown",
            "--name",
            "requirements.md",
        ]);
        assert_eq!(code, 1);
        assert!(err.contains("E_SOURCE_CONFLICT"), "{err}");
        let (code, _, err) = env.run(&[
            "import",
            file.to_str().unwrap(),
            "--media-type",
            "application/pdf",
            "--name",
            "x",
        ]);
        assert_eq!(code, 1);
        assert!(err.contains("E_UNSUPPORTED_MEDIA_TYPE"), "{err}");
        let (code, value) = env.json(&["vocab"]);
        assert_eq!((code, value), (0, json!([])));
        let (code, value) = env.json(&["questions"]);
        assert_eq!((code, value), (0, json!([])));
    }

    #[test]
    fn cli_extract_replay_then_apply_accept_and_reject() {
        let env = env();
        env.import();
        let fixtures = record(&env, "extract");
        let f = fixtures.to_str().unwrap();
        let (code, job) = env.json(&["--provider", "replay", "--replay-fixtures", f, "extract"]);
        assert_eq!(code, 0, "{job}");
        assert_eq!(job["status"], "ready", "{job}");
        assert!(job["job_id"].as_str().unwrap().starts_with("sha256:"));
        assert_eq!(job["progress"], 1.0);
        let pending = pending_proposals(&env.db);
        assert!(!pending.is_empty());
        // Reject: pending -> rejected without a PSG change.
        let (code, write) = env.json(&["apply", &pending[0], "--action", "reject"]);
        assert_eq!(code, 0, "{write}");
        assert_eq!(write["revision"], job["base_revision"]);
        assert_eq!(write["events"], json!([]));
        assert_eq!(
            proposal_state(&env.db, &pending[0]),
            WorkflowProposalState::Rejected
        );
        // A second action on it is the shared not-pending conflict.
        let (code, _, err) = env.run(&["apply", &pending[0], "--action", "accept"]);
        assert_eq!(code, 1);
        assert!(err.contains("E_PROPOSAL_NOT_PENDING"), "{err}");
        assert_eq!(pending.len(), 2, "one proposal per requirement candidate");
        let next = &pending[1];
        // The class must match: a create proposal is not a merge.
        let (code, _, err) = env.run(&["apply", next, "--action", "merge"]);
        assert_eq!(code, 1);
        assert!(err.contains("E_PROPOSAL_ACTION_MISMATCH"), "{err}");
        let (code, write) = env.json(&["apply", next, "--action", "create"]);
        assert_eq!(code, 0, "{write}");
        assert_ne!(write["revision"], job["base_revision"]);
        assert_eq!(write["events"].as_array().unwrap().len(), 1);
        assert_eq!(
            proposal_state(&env.db, next),
            WorkflowProposalState::Accepted
        );
        let (code, _, err) = env.run(&["apply", "proposal:missing", "--action", "accept"]);
        assert_eq!(code, 1);
        assert!(err.contains("E_NOT_FOUND"), "{err}");
        let (code, _, err) =
            env.run(&["apply", &pending[0], "--action", "accept", "--reason", " x"]);
        assert_eq!(code, 1);
        assert!(err.contains("E_INVALID_REQUEST"), "{err}");
    }

    #[test]
    fn cli_accept_set_parses_members_before_any_write() {
        let env = env();
        let before = std::fs::read(&env.db).unwrap();
        for (args, message) in [
            (
                &["accept-set", "proposal:x"][..],
                "is not <PROPOSAL_ID>=<ACTION>",
            ),
            (&["accept-set", "=create"], "is not <PROPOSAL_ID>=<ACTION>"),
            (
                &["accept-set", "proposal:x="],
                "is not <PROPOSAL_ID>=<ACTION>",
            ),
            (&["accept-set", "not an id=create"], "is not a proposal ID"),
            (&["accept-set", "proposal:x=approve"], "is not a set action"),
            (&["accept-set", "proposal:x=reject"], "is not a set action"),
            (
                &["accept-set", "proposal:x=create", "proposal:x=accept"],
                "occurs twice",
            ),
            (&["accept-set"], "PROPOSAL"),
        ] {
            let (code, out, err) = env.run(args);
            assert_eq!(code, 1, "{args:?}: {out}");
            assert!(err.contains(message), "{args:?}: {err}");
        }
        assert_eq!(
            std::fs::read(&env.db).unwrap(),
            before,
            "nothing was opened or written"
        );
    }

    #[test]
    fn cli_accept_set_commits_sibling_stage_outputs_in_one_revision() {
        let env = env();
        env.import();
        let fixtures = record(&env, "extract");
        let f = fixtures.to_str().unwrap();
        let (code, job) = env.json(&["--provider", "replay", "--replay-fixtures", f, "extract"]);
        assert_eq!(code, 0, "{job}");
        let pending = pending_proposals(&env.db);
        assert_eq!(pending.len(), 2);
        let link = SqliteStageStore::open(&env.db)
            .unwrap()
            .proposal_link(&id(&pending[0]))
            .unwrap()
            .unwrap();
        // Caller order reversed; both are create proposals of one classification child.
        let members: Vec<String> = pending
            .iter()
            .rev()
            .map(|p| format!("{p}=create"))
            .collect();
        let mut args = vec!["accept-set"];
        args.extend(members.iter().map(String::as_str));
        args.extend(["--reason", "Batch accepted."]);
        let (code, write) = env.json(&args);
        assert_eq!(code, 0, "{write}");
        assert_ne!(write["revision"], job["base_revision"]);
        assert_eq!(write["events"].as_array().unwrap().len(), 1);
        assert_eq!(write["events"][0]["kind"], "accept_proposal_set");
        let store = SqliteWorkflowStore::open(&env.db).unwrap();
        for p in &pending {
            let record = store.proposal(&id(p)).unwrap().unwrap();
            assert_eq!(record.state, WorkflowProposalState::Accepted);
            assert_eq!(
                record.resolved_revision_id.as_ref().map(|r| r.to_string()),
                write["revision"].as_str().map(str::to_owned)
            );
        }
        // The child records both outputs once.
        let child = SqliteStageStore::open(&env.db)
            .unwrap()
            .stage_job(&link.stage_job_id)
            .unwrap()
            .unwrap();
        let child: Value = serde_json::from_str(&child.stage_job_json).unwrap();
        assert_eq!(child["status"], "committed");
        assert_eq!(child["committed_output_refs"].as_array().unwrap().len(), 2);
        assert_eq!(child["committed_output_ref"], Value::Null);
        // Text output and a second attempt.
        let (code, _, err) = env.run(&args);
        assert_eq!(code, 1);
        assert!(err.contains("E_PROPOSAL_NOT_PENDING"), "{err}");
    }

    // ------------------------------------------------------------------ stages and gates

    #[test]
    fn cli_stage_commands_report_jobs_and_exit_codes() {
        // lint: F1 passes -> exit 0 with the JobResponse and job_id.
        let env = env();
        env.import();
        let (code, job) = env.json(&["lint"]);
        assert_eq!(code, 0, "{job}");
        assert_eq!(job["status"], "ready");
        assert!(job["job_id"].is_string());
        assert_eq!(job["error"], Value::Null);
        for key in [
            "status",
            "progress",
            "base_revision",
            "result_version",
            "error",
            "job_id",
        ] {
            assert!(job.get(key).is_some(), "{key}");
        }
        // The same capture is the same job.
        let (_, again) = env.json(&["lint"]);
        assert_eq!(again["job_id"], job["job_id"]);
        // A whole-scope recipe rejects a scope.
        let (code, _, err) = env.run(&["lint", "--scope", "src:x"]);
        assert_eq!(code, 1);
        assert!(
            err.contains("E_STAGE_SCOPE_INVALID") || err.contains("E_NOT_FOUND"),
            "{err}"
        );
        // extract with the null provider (R0.2): segmentation takes its deterministic fallback,
        // and the Required classification phase fails the recipe (exit 1) with its job printed.
        let (code, out, err) = env.run(&["extract", "--json"]);
        assert_eq!(code, 1, "{err}");
        let failed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(failed["status"], "failed");
        let error = failed["error"].as_str().unwrap();
        assert!(error.starts_with("E_INFERENCE_REQUIRED"), "{error}");
        assert!(
            error.contains(r#""task_kinds":["requirement_classification"]"#),
            "{error}"
        );
        assert!(err.contains("the extract recipe ended failed"), "{err}");
        // calc without its configuration is E_STAGE_CONFIG_MISSING.
        let (code, _, err) = env.run(&["calc"]);
        assert_eq!(code, 1);
        assert!(err.contains("E_STAGE_CONFIG_MISSING"), "{err}");
    }

    #[test]
    fn cli_gate_failure_is_exit_two() {
        // R0.4: F2 fails on genuine graph content (an Accepted Operation without a performer
        // violates PLUMB.F2.OPERATION.PERFORMER), never on configured findings.
        let env = env();
        env.import();
        commit_node(
            &env.db,
            json!({
                "id": "op:submit", "revision": 1, "status": "Accepted",
                "payload": {"type": "Operation", "data": {
                    "name": "op:submit", "operation_kind": "command", "input_schema_ref": null,
                    "output_schema_ref": null, "preconditions": null, "postconditions": null,
                    "idempotency": null, "transaction_semantics": null
                }},
                "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
                "audit": {"created_by": ACTOR, "created_at": T0, "updated_by": null, "updated_at": null}
            }),
        );
        // analyse completes (replayed domain and lifecycle inference) and its failed F2 gate is
        // exit 2, never a failed job.
        let fixtures = record(&env, "analyse");
        let f = fixtures.to_str().unwrap();
        let replay = ["--provider", "replay", "--replay-fixtures", f];
        let (code, job) = env.json(&[&replay[..], &["analyse"]].concat());
        assert_eq!(code, 2, "{job}");
        assert_eq!(job["status"], "ready");
        assert_eq!(job["error"], Value::Null);
        let (code, out, err) = env.run(&[&replay[..], &["analyse"]].concat());
        assert_eq!(code, 2, "{err}");
        assert!(out.contains("F2: fail"), "{out}");
        let (code, ready) = env.json(&["ready"]);
        assert_eq!(code, 2, "{ready}");
        let f2 = ready["gates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["gate"] == "F2")
            .unwrap();
        assert_eq!(f2["result"], "fail");
    }

    #[test]
    fn ready_fails_without_accepted_requirements() {
        // R0.4: zero Accepted Requirements is a product precondition. Every gate is evaluated
        // and reported as it is (no fabricated gate failure); ready still exits 2.
        let env = env();
        env.import();
        let (code, ready) = env.json(&["ready"]);
        assert_eq!(code, 2, "{ready}");
        assert_eq!(ready["preconditions"], json!(["no_accepted_requirements"]));
        assert!(
            ready["gates"]
                .as_array()
                .unwrap()
                .iter()
                .all(|g| g["result"] == "pass"),
            "{ready}"
        );
        let (code, out, err) = env.run(&["ready"]);
        assert_eq!(code, 2, "{err}");
        assert!(
            out.lines()
                .any(|l| l == "precondition: no_accepted_requirements"),
            "{out}"
        );
        assert!(!out.contains(": fail"), "{out}");
    }

    #[test]
    fn cli_ready_export_and_report_share_one_projection() {
        let env = env();
        env.import();
        // Without a current F4 report the projection is unavailable (exit 1).
        let (code, _, _) = env.run(&["export", "--format", "functional"]);
        assert_eq!(code, 1);
        let (code, ready) = env.json(&["ready"]);
        assert!(code == 0 || code == 2, "{ready}");
        assert_eq!(
            ready["gates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|g| g["gate"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["I0", "F1", "F2", "F3", "F4"]
        );
        for key in ["revision", "semantic_hash", "evaluation_at"] {
            assert!(ready[key].is_string(), "{key}");
        }
        for gate in ready["gates"].as_array().unwrap() {
            assert!(gate["report_hash"].as_str().unwrap().starts_with("sha256:"));
        }
        let all_pass = ready["gates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g["result"] == "pass");
        // R0.4: success also needs every product precondition (none fails here only when
        // Accepted Requirements exist).
        let preconditions_hold = ready["preconditions"].as_array().unwrap().is_empty();
        assert_eq!(code == 0, all_pass && preconditions_hold);
        let out = env.path("report");
        let (code, report) = env.json(&["report", "--out", out.to_str().unwrap()]);
        assert_eq!(code, 0, "{report}");
        let names = [
            "functional.yaml",
            "glossary.md",
            "scenarios.yaml",
            "decisions.jsonl",
            "model-report.md",
            "gate-report.json",
        ];
        for name in names {
            assert!(out.join(name).is_file(), "{name}");
        }
        for (format, file) in [
            ("functional", "functional.yaml"),
            ("report", "model-report.md"),
            ("decisions", "decisions.jsonl"),
            ("glossary", "glossary.md"),
        ] {
            let exported = env
                .cmd()
                .args(["export", "--format", format])
                .output()
                .unwrap();
            assert_eq!(exported.status.code(), Some(0), "{format}");
            assert_eq!(
                exported.stdout,
                std::fs::read(out.join(file)).unwrap(),
                "{format}"
            );
        }
        let (code, _, err) = env.run(&["export", "--format", "yaml"]);
        assert_eq!(code, 1);
        assert!(err.contains("E_INVALID_REQUEST"), "{err}");
    }

    // ------------------------------------------------------------------ log-effort

    #[test]
    fn cli_log_effort_appends_canonical_lines_only() {
        let env = env();
        let file = env.path("effort.jsonl");
        let f = file.to_str().unwrap();
        // No --db is needed and the database is never touched.
        let base = |args: &[&str]| {
            let mut cmd = bin();
            cmd.arg("--config").arg(&env.config).args(args);
            output(&mut cmd)
        };
        let (code, _, err) = base(&[
            "log-effort",
            "--file",
            f,
            "--kind",
            "stakeholder_hours",
            "--amount",
            "2.5",
            "--note",
            "workshop",
        ]);
        assert_eq!(code, 0, "{err}");
        let (code, _, err) = base(&[
            "log-effort",
            "--file",
            f,
            "--kind",
            "analyst_days",
            "--amount",
            "1",
        ]);
        assert_eq!(code, 0, "{err}");
        let text = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        assert_eq!(lines.len(), 2);
        for line in &lines {
            assert!(line.ends_with('\n'));
            let value: Value = serde_json::from_str(line.trim_end()).unwrap();
            assert_eq!(
                to_canonical_json(&value).unwrap(),
                line.trim_end().as_bytes(),
                "RFC 8785"
            );
            assert_eq!(value["version"], 1);
            assert_eq!(value["actor_ref"], ACTOR);
            assert!(value["recorded_at"].is_string());
        }
        let first: Value = serde_json::from_str(lines[0].trim_end()).unwrap();
        assert_eq!(first["kind"], "stakeholder_hours");
        assert_eq!(first["amount"], "2.5");
        assert_eq!(first["note"], "workshop");
        let second: Value = serde_json::from_str(lines[1].trim_end()).unwrap();
        assert_eq!(second["note"], Value::Null);
        assert_eq!(
            second.as_object().unwrap().keys().collect::<Vec<_>>(),
            [
                "actor_ref",
                "amount",
                "kind",
                "note",
                "recorded_at",
                "version"
            ]
        );
        for amount in ["0", "0.00", "1.234", "-1", "1.", ".5", "1e3", " 1"] {
            let (code, _, err) = base(&[
                "log-effort",
                "--file",
                f,
                "--kind",
                "analyst_days",
                "--amount",
                amount,
            ]);
            assert_eq!(code, 1, "{amount}: {err}");
        }
        let (code, _, _) = base(&[
            "log-effort",
            "--file",
            f,
            "--kind",
            "analyst_days",
            "--amount",
            "1",
            "--note",
            " x",
        ]);
        assert_eq!(code, 1);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            text,
            "rejections append nothing"
        );
        // The pilot database is unchanged: no revision, event or gate.
        let (code, vocab) = env.json(&["vocab"]);
        assert_eq!((code, vocab), (0, json!([])));
    }
}
