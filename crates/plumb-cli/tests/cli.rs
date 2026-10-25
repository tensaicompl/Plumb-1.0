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
    const COMMANDS: [&[&str]; 29] = [
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
        // R0.6.
        &["directory"],
        &["confirm", "req:a", "actor:b", "--reason", "Reviewed."],
        &["findings", "list"],
        &["set-attribute-type", "attr:a", "--type", "Date"],
        &["ask"],
        &["answer", "q:0000000000000001", "--answer", "{}"],
        &["proposals"],
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
            directory: config
                .get("directory")
                .map(|d| serde_json::from_value(d.clone()).unwrap()),
            question_routing: config
                .get("question_routing")
                .map(|r| serde_json::from_value(r.clone()).unwrap()),
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
    fn cli_parses_all_twenty_nine_commands() {
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
                "directory",
                "confirm",
                "findings",
                "set-attribute-type",
                "ask",
                "answer",
                "proposals",
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
            // The projection evaluates the F4 gate report on demand when the head has none.
            (vec!["export", "--format", "functional"], 0, ""),
            (vec!["report", "--out", out.to_str().unwrap()], 0, ""),
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
        // The same file again exists: identity ignores the audit metadata (Fable #314).
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
        assert!(err.contains("E_SOURCE_EXISTS"), "{err}");
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

    /// Runs `import FILE --media-type MEDIA --name NAME --json`: (exit code, stdout, stderr).
    fn import_file(env: &Env, file: &Path, media_type: &str, name: &str) -> (i32, String, String) {
        env.run(&[
            "import",
            file.to_str().unwrap(),
            "--media-type",
            media_type,
            "--name",
            name,
            "--json",
        ])
    }

    /// Asserts the CLI's typed oversize rejection carrying the importer's limit error.
    fn assert_oversized(result: (i32, String, String), import_error: &str) {
        let (code, out, err) = result;
        assert_eq!(code, 1, "{out} {err}");
        let error: Value = serde_json::from_str(err.trim()).unwrap_or_else(|_| panic!("{err}"));
        assert_eq!(
            error,
            json!({
                "code": "E_INVALID_REQUEST",
                "message": "the uploaded file cannot be imported as its declared media type",
                "details": {"import_error": import_error}
            })
        );
    }

    fn sparse(env: &Env, name: &str, len: u64) -> PathBuf {
        let path = env.path(name);
        std::fs::File::create(&path).unwrap().set_len(len).unwrap();
        path
    }

    #[test]
    fn cli_import_rejects_oversized_files_before_reading_them() {
        let env = env();
        let before = head_graph(&env.db).revision;
        const DOCX_MEDIA: &str =
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
        // Metadata above the limit: rejected from the metadata length alone, before any read.
        // The 64 GiB sparse file could not be read into memory.
        let huge = 64 * 1024 * 1024 * 1024_u64;
        for (media_type, name, max, message) in [
            (
                "text/markdown",
                "huge.md",
                16_777_216_u64,
                "source input bytes",
            ),
            ("text/plain", "huge.txt", 16_777_216, "source input bytes"),
            (DOCX_MEDIA, "huge.docx", 134_217_728, "DOCX input bytes"),
        ] {
            let file = sparse(&env, name, huge);
            assert_oversized(
                import_file(&env, &file, media_type, name),
                &format!("{message} is {huge}, above the limit {max}"),
            );
            // Exactly one byte above the limit.
            let file = sparse(&env, name, max + 1);
            assert_oversized(
                import_file(&env, &file, media_type, name),
                &format!("{message} is {}, above the limit {max}", max + 1),
            );
        }
        // Misleading metadata (a device reporting length 0): the read stops at limit + 1.
        assert_oversized(
            import_file(&env, Path::new("/dev/zero"), "text/plain", "zero.txt"),
            "source input bytes is 16777217, above the limit 16777216",
        );
        // An unsupported media type is still refused, before the file is touched.
        let (code, _, err) = import_file(&env, &env.path("missing.bin"), "application/pdf", "x");
        assert_eq!(code, 1);
        assert!(err.contains("E_UNSUPPORTED_MEDIA_TYPE"), "{err}");
        // Nothing was imported.
        let after = head_graph(&env.db).revision;
        assert_eq!(after.id, before.id);
        assert_eq!(after.semantic_hash, before.semantic_hash);
    }

    #[test]
    fn cli_import_reads_files_of_exactly_the_limit() {
        let env = env();
        let before = head_graph(&env.db).revision;
        const DOCX_MEDIA: &str =
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
        // Exactly the limit passes the CLI's size checks and reaches the importer, which then
        // rejects the content itself (invalid UTF-8, not a ZIP archive), not its size.
        let file = env.write("limit.txt", &vec![0xFF; 16_777_216]);
        assert_oversized(
            import_file(&env, &file, "text/plain", "limit.txt"),
            "source is not valid UTF-8 (valid up to byte 0)",
        );
        let file = sparse(&env, "limit.docx", 134_217_728);
        let (code, _, err) = import_file(&env, &file, DOCX_MEDIA, "limit.docx");
        assert_eq!(code, 1);
        let error: Value = serde_json::from_str(err.trim()).unwrap();
        let import_error = error["details"]["import_error"].as_str().unwrap();
        assert!(
            import_error.starts_with("DOCX archive is unreadable"),
            "{import_error}"
        );
        assert_eq!(head_graph(&env.db).revision.id, before.id);
        // A small ordinary import keeps working.
        let source = env.import();
        assert!(head_graph(&env.db).graph.node(&id(&source)).is_some());
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

    // ------------------------------------------------------------------ R0.6 product surfaces

    /// A six-family directory (no grounding: the fixture has no Accepted Requirement) and a
    /// question routing to stakeholder:hr.
    fn directory_config() -> Value {
        use plumb_validation::decision_table_analysis::{
            DecisionColumn, DecisionInputCell, DecisionOutputCell, DecisionRow, DecisionTableSpec,
            DecisionType,
        };
        let column = |name: &str| DecisionColumn {
            name: name.into(),
            ty: DecisionType::Bool,
            domain: None,
        };
        let spec = DecisionTableSpec {
            hit_policy: "UNIQUE".into(),
            inputs: vec![column("eligible")],
            outputs: vec![column("approve")],
            rows: [true, false]
                .into_iter()
                .map(|v| DecisionRow {
                    inputs: vec![DecisionInputCell::Bool { value: v }],
                    outputs: vec![DecisionOutputCell::Bool { value: v }],
                })
                .collect(),
            default_output: None,
        };
        let mut c = config(all_gates());
        c["directory"] = json!({
            "stakeholders": [{"id": "stakeholder:hr", "payload": {
                "name": "HR", "stakeholder_kind": "internal", "organization": null,
                "responsibilities": null, "contact_ref": null}}],
            "actors": [{"id": "actor:clerk", "payload": {"name": "Clerk", "actor_kind": "human"}}],
            "business_roles": [{"id": "brole:approver", "payload": {"name": "Approver"}}],
            "calendars": [{"id": "cal:office", "calendar_ref": "cal:office",
                           "time_zone": "Europe/Amsterdam", "region": "NL", "holiday_source": null}],
            "rules": [{"id": "rule:balance", "payload": {"name": "Balance", "rule_kind": "business",
                       "condition_expr": null, "action_expr": null}}],
            "decision_tables": [{"id": "dt:approval", "name": "Approval", "spec": spec}]
        });
        c["question_routing"] = json!({
            "stakeholders": [{"id": "stakeholder:hr", "name": "HR", "roles": ["analyst"]}],
            "routing": {"default": ["stakeholder:hr"]}
        });
        c
    }

    /// The head revision of `db`.
    fn head_graph(db: &Path) -> plumb_store::LoadedRevision {
        let store = SqliteRevisionStore::open(db).unwrap();
        let head = store.head(&BranchName::main()).unwrap().unwrap();
        store.load_revision(&head).unwrap()
    }

    fn status_of(db: &Path, element: &str) -> String {
        let g = head_graph(db).graph;
        serde_json::to_value(g.node(&id(element)).unwrap().status)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn cli_r06_configuration_fields_are_optional_and_strict() {
        // A configuration without the two optional fields stays valid (env() above).
        let plain = env();
        let (code, _, err) = plain.run(&["vocab"]);
        assert_eq!(code, 0, "{err}");
        // Complete values parse.
        let full = env_with(directory_config());
        let (code, _, err) = full.run(&["vocab"]);
        assert_eq!(code, 0, "{err}");
        // Present but null, incomplete or invalid values fail; nothing is defaulted.
        let mut cases = Vec::new();
        let mut c = directory_config();
        c["directory"] = Value::Null;
        cases.push(("null directory", c));
        let mut c = directory_config();
        c["directory"].as_object_mut().unwrap().remove("rules");
        cases.push(("incomplete directory", c));
        let mut c = directory_config();
        c["directory"]["actors"][0]["payload"]
            .as_object_mut()
            .unwrap()
            .remove("actor_kind");
        cases.push(("incomplete actor", c));
        let mut c = directory_config();
        c["directory"]["calendars"][0]["calendar_ref"] = json!("cal:unknown");
        cases.push(("unknown calendar", c));
        let mut c = directory_config();
        c["directory"]["rules"][0]["id"] = json!("actor:clerk");
        cases.push(("duplicate element", c));
        let mut c = directory_config();
        c["question_routing"]["routing"]["default"] = json!(["stakeholder:nobody"]);
        cases.push(("invalid routing", c));
        let mut c = directory_config();
        c["question_routing"]
            .as_object_mut()
            .unwrap()
            .remove("stakeholders");
        cases.push(("incomplete routing", c));
        let mut c = directory_config();
        c["directory_extra"] = json!({});
        cases.push(("unknown field", c));
        for (name, c) in cases {
            let bad = env_with(c);
            let (code, _, err) = bad.run(&["vocab"]);
            assert_eq!(code, 1, "{name}: {err}");
            assert!(err.contains("configuration"), "{name}: {err}");
        }
        // Absent configuration is a clear typed error, never an empty success.
        let plain = env();
        for command in ["directory", "ask"] {
            let (code, _, err) = plain.run(&[command, "--json"]);
            assert_eq!(code, 1, "{command}: {err}");
            assert!(err.contains("E_STAGE_CONFIG_MISSING"), "{command}: {err}");
        }
    }

    #[test]
    fn directory_confirm_ask_answer_round_trip() {
        // R0.1 bootstrap: the configured human Agent decides every governed acceptance.
        let env = bare_env_with(directory_config());
        let (code, _, err) = env.run(&["init", "--project", PROJECT]);
        assert_eq!(code, 0, "{err}");
        env.import();
        // An Accepted Operation without a performer: F2 PLUMB.F2.OPERATION.PERFORMER.
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
        // directory: one pending HUMAN_CONFIRM create proposal per element; no commit.
        let before = head_graph(&env.db).revision.semantic_hash;
        let (code, elements) = env.json(&["directory"]);
        assert_eq!(code, 0, "{elements}");
        let elements = elements.as_array().unwrap().clone();
        assert_eq!(
            elements
                .iter()
                .map(|e| e["element_ref"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "actor:clerk",
                "brole:approver",
                "cal:office",
                "dt:approval",
                "rule:balance",
                "stakeholder:hr"
            ]
        );
        assert_eq!(head_graph(&env.db).revision.semantic_hash, before);
        let proposal_of = |element: &str| -> String {
            let e = elements
                .iter()
                .find(|e| e["element_ref"] == element)
                .unwrap();
            assert_eq!(e["disposition"], "proposed");
            assert_eq!(e["action"], "create");
            let proposal = e["proposal_id"].as_str().unwrap().to_owned();
            assert_eq!(
                proposal_state(&env.db, &proposal),
                WorkflowProposalState::Pending
            );
            proposal
        };
        // Accept the sibling actor and stakeholder proposals together (R0.3 set acceptance on
        // their shared base): the nodes become Proposed.
        let (code, write) = env.json(&[
            "accept-set",
            &format!("{}=create", proposal_of("actor:clerk")),
            &format!("{}=create", proposal_of("stakeholder:hr")),
            "--reason",
            "Directory reviewed.",
        ]);
        assert_eq!(code, 0, "{write}");
        for element in ["actor:clerk", "stakeholder:hr"] {
            assert_eq!(status_of(&env.db, element), "Proposed");
        }
        // confirm promotes them through the generic governed confirmation.
        let (code, write) = env.json(&[
            "confirm",
            "stakeholder:hr",
            "actor:clerk",
            "--reason",
            "Confirmed.",
        ]);
        assert_eq!(code, 0, "{write}");
        assert_eq!(status_of(&env.db, "actor:clerk"), "Accepted");
        assert_eq!(status_of(&env.db, "stakeholder:hr"), "Accepted");
        // A current F2 report (ready persists it), then ask materializes its questions.
        let (code, ready) = env.json(&["ready"]);
        assert_eq!(code, 2, "{ready}");
        let (code, asked) = env.json(&["ask"]);
        assert_eq!(code, 0, "{asked}");
        assert!(asked["write"].is_object(), "{asked}");
        let performer = asked["questions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["template_id"] == "f2.operation.performer.v1")
            .unwrap_or_else(|| panic!("{asked}"))
            .clone();
        assert_eq!(performer["stakeholder_ref"], "stakeholder:hr");
        let question = performer["question_ref"].as_str().unwrap();
        assert!(head_graph(&env.db).graph.node(&id(question)).is_some());
        // answer: a pending Question-origin governed proposal through the shared service.
        let (code, answered) = env.json(&[
            "answer",
            question,
            "--answer",
            r#"{"performer_ref": "actor:clerk"}"#,
        ]);
        assert_eq!(code, 0, "{answered}");
        let proposal = answered["proposal_id"].as_str().unwrap();
        let record = SqliteWorkflowStore::open(&env.db)
            .unwrap()
            .proposal(&id(proposal))
            .unwrap()
            .unwrap();
        assert_eq!(record.state, WorkflowProposalState::Pending);
        assert_eq!(record.origin_kind.as_str(), "question");
        assert_eq!(record.origin_ref, Some(id(question)));
        assert_eq!(answered["action"], record.proposal_kind.as_str());
        // A non-object answer is refused like HTTP.
        let (code, _, err) = env.run(&["answer", question, "--answer", "[1]"]);
        assert_eq!(code, 1);
        assert!(err.contains("E_INVALID_REQUEST"), "{err}");
    }

    #[test]
    fn proposals_lists_pending_with_class_origin_and_reason() {
        let env = env_with(directory_config());
        env.import();
        // Stage-origin proposals (extract, replayed).
        let fixtures = record(&env, "extract");
        let f = fixtures.to_str().unwrap();
        let (code, job) = env.json(&["--provider", "replay", "--replay-fixtures", f, "extract"]);
        assert_eq!(code, 0, "{job}");
        // Intake-origin proposals (directory) on the same base.
        let (code, _) = env.json(&["directory"]);
        assert_eq!(code, 0);
        let first_base = SqliteRevisionStore::open(&env.db)
            .unwrap()
            .head(&BranchName::main())
            .unwrap()
            .unwrap();
        // Accepting one stage proposal moves the head: every other pending proposal becomes
        // stale but stays pending and listed.
        let stage_links = SqliteStageStore::open(&env.db)
            .unwrap()
            .pending_proposal_links()
            .unwrap();
        let accepted = stage_links[0].proposal_id.to_string();
        let (code, write) = env.json(&[
            "apply",
            &accepted,
            "--action",
            "create",
            "--reason",
            "Accepted.",
        ]);
        assert_eq!(code, 0, "{write}");
        let (code, listed) = env.json(&["proposals"]);
        assert_eq!(code, 0, "{listed}");
        let listed = listed.as_array().unwrap().clone();
        // Every pending workflow proposal, sorted by ID, and nothing else.
        let wf = SqliteWorkflowStore::open(&env.db).unwrap();
        let pending = wf.pending_proposals().unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|p| p["proposal_id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>(),
            pending
                .iter()
                .map(|p| p.proposal_id.to_string())
                .collect::<Vec<_>>()
        );
        let mut sorted: Vec<String> = listed
            .iter()
            .map(|p| p["proposal_id"].as_str().unwrap().to_owned())
            .collect();
        sorted.sort();
        assert_eq!(
            sorted,
            listed
                .iter()
                .map(|p| p["proposal_id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        );
        assert!(!listed.iter().any(|p| p["proposal_id"] == accepted.as_str()));
        let stage = SqliteStageStore::open(&env.db).unwrap();
        let mut origins = std::collections::BTreeSet::new();
        for (view, record) in listed.iter().zip(&pending) {
            let intake: Value = serde_json::from_str(&record.intake_json).unwrap();
            assert_eq!(view["requires_reason"], intake["requires_reason"], "{view}");
            assert_eq!(view["action"], record.proposal_kind.as_str());
            assert_eq!(view["origin"], record.origin_kind.as_str());
            assert_eq!(view["base_revision"], record.base_revision_id.to_string());
            // Stale: built on the earlier head, still pending and listed.
            assert_eq!(record.base_revision_id, first_base);
            origins.insert(record.origin_kind.as_str());
            match stage.proposal_link(&record.proposal_id).unwrap() {
                Some(link) => {
                    assert_eq!(view["origin"], "stage");
                    assert_eq!(view["recipe_job_id"], link.recipe_job_id.to_string());
                    assert_eq!(view["stage_job_id"], link.stage_job_id.to_string());
                    assert_eq!(view["origin_ref"], Value::Null);
                }
                None => {
                    assert_eq!(view["origin"], "intake");
                    assert_eq!(view["recipe_job_id"], Value::Null);
                    assert_eq!(view["stage_job_id"], Value::Null);
                    assert_eq!(view["origin_ref"], Value::Null);
                }
            }
        }
        assert_eq!(
            origins,
            std::collections::BTreeSet::from(["intake", "stage"])
        );
        // The text form carries the same records; the listing changed nothing.
        let (code, text, err) = env.run(&["proposals"]);
        assert_eq!(code, 0, "{err}");
        for view in &listed {
            let line = text
                .lines()
                .find(|l| l.starts_with(view["proposal_id"].as_str().unwrap()))
                .unwrap_or_else(|| panic!("{text}"));
            assert!(line.contains(view["action"].as_str().unwrap()), "{line}");
            assert!(
                line.contains(&format!("requires_reason={}", view["requires_reason"])),
                "{line}"
            );
        }
        assert_eq!(wf.pending_proposals().unwrap(), pending);
    }

    #[test]
    fn cli_ready_export_and_report_share_one_projection() {
        let env = env();
        env.import();
        // Without a current F4 report the projection evaluates it on demand.
        let (code, _, err) = env.run(&["export", "--format", "functional"]);
        assert_eq!(code, 0, "{err}");
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
