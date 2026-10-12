//! A0.4 CLI tests (Hotfix 067, Hotfix 069): the `plumb-cli` binary over a temporary pilot
//! database. Every command runs in-process over the shared application services; providers are
//! null or replayed fixtures recorded in-process from the exact request material. No test uses
//! the network or the environment.

mod cli {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use assert_cmd::Command;
    use plumb_api::application::{initialize_project_database, open_application};
    use plumb_api::{ApiIdentity, Recipe};
    use plumb_core::{to_canonical_json, CanonicalJson, FixedClock, Hash, Id, Timestamp};
    use plumb_functional::VerificationApplicabilityInputs;
    use plumb_inference::{
        validate_binding, InferenceArtifact, InferenceProvider, InferenceRequest,
        InferenceRequestMaterial, ProviderError, ProviderExecution,
    };
    use plumb_store::{BranchName, SqliteStageStore, SqliteWorkflowStore, WorkflowProposalState};
    use plumb_validation::{
        finding_id, finding_key, load_builtin_software_profile, ValidationPolicy,
    };
    use serde_json::{json, Value};

    const PROJECT: &str = "project:pilot";
    const ACTOR: &str = "agent:cli";
    const ASSISTANT: &str = "agent:assistant";
    const STAGE_ACTOR: &str = "agent:compiler";
    const T0: &str = "2026-01-01T00:00:00.000000000Z";
    const MARKDOWN: &[u8] =
        b"# Leave\n\nEmployees shall submit leave requests.\n\nManagers shall approve leave requests.\n";

    /// The 20 commands with their minimal required arguments.
    const COMMANDS: [&[&str]; 20] = [
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

    fn f1_inputs() -> Value {
        json!({"requirements": [], "analysis_findings": [], "lint_policy": {"severity_overrides": {}}})
    }

    fn f2_inputs(findings: Vec<Value>) -> Value {
        json!({
            "analysis_findings": findings, "calculation_scope_overrides": [],
            "decision_tables": [], "invariants": []
        })
    }

    fn f4_inputs() -> Value {
        json!({
            "applicability": {
                "source_refs": [], "executable_source_refs": [], "in_scope_scenario_refs": [],
                "blocking_scenario_refs": [], "executable_coverage": []
            },
            "visible_assumption_refs": []
        })
    }

    fn all_gates() -> Value {
        json!({"f1": f1_inputs(), "f2": f2_inputs(vec![]), "f4": f4_inputs()})
    }

    /// A blocker analysis finding with its frozen identity (`finding_key`, `finding_id`).
    fn blocker(code: &str, family: &str, condition: &str, target: &str) -> Value {
        let key = finding_key(code, &[id(target)], condition).unwrap();
        let finding = finding_id(&key).unwrap();
        json!({
            "key": key, "id": finding, "semantic_condition_key": condition,
            "payload": {
                "code": code, "family": family, "severity": "blocker", "status": "open",
                "message": "configured analysis finding",
                "affected_refs": [target], "suggested_resolution": null,
                "standard_rule_ref": null, "waiver_ref": null
            }
        })
    }

    /// A blocker F2 analysis finding over `target` (makes F2 fail).
    fn f2_blocker(target: &str) -> Value {
        blocker(
            "PLUMB.F2.DOMAIN.RELATION_TYPED",
            "F2",
            &format!("domain_relationship_cardinality_unresolved:{target}"),
            target,
        )
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
            provider_mode: plumb_api::ProviderMode::Null,
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
    fn cli_parses_all_twenty_commands() {
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

    // ------------------------------------------------------------------ configuration

    #[test]
    fn cli_configuration_is_strict() {
        let mut unknown = config(all_gates());
        unknown["extra"] = json!(1);
        let mut missing = config(all_gates());
        missing.as_object_mut().unwrap().remove("process_scope");
        let mut gate_unknown = config(all_gates());
        gate_unknown["gates"]["f3"] = Value::Null;
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
        // extract with the null provider: the recipe fails (exit 1) with its job printed.
        let (code, out, err) = env.run(&["extract", "--json"]);
        assert_eq!(code, 1, "{err}");
        let failed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(failed["status"], "failed");
        assert!(failed["error"]
            .as_str()
            .unwrap()
            .starts_with("E_INFERENCE_FAILED"));
        // calc without its configuration is E_STAGE_CONFIG_MISSING.
        let (code, _, err) = env.run(&["calc"]);
        assert_eq!(code, 1);
        assert!(err.contains("E_STAGE_CONFIG_MISSING"), "{err}");
    }

    #[test]
    fn cli_gate_failure_is_exit_two() {
        // A configured blocker F2 finding over the imported source fails F2.
        let probe = env();
        let source = probe.import();
        drop(probe);
        let env = env_with(config(json!({
            "f1": f1_inputs(), "f2": f2_inputs(vec![f2_blocker(&source)]), "f4": f4_inputs()
        })));
        assert_eq!(env.import(), source);
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
        assert_eq!(code == 0, all_pass);
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
