//! Shared harness of the CLI end-to-end simulations (HR pilot, customer zero): a scripted
//! model recorded in-process and replayed through the real `plumb-cli` binary.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use plumb_api::application::{open_application, ApplicationRuntime};
use plumb_api::{ApiIdentity, CalendarConfig, DirectManagerFact, DirectManagerPredicates, Recipe};
use plumb_core::{to_canonical_json, CanonicalJson, FixedClock, Hash, Id, Timestamp};
use plumb_functional::{ExprCalendarDefinition, ExprStaticCalendarProvider};
use plumb_inference::{
    validate_binding, InferenceArtifact, InferenceProvider, InferenceRequest,
    InferenceRequestMaterial, ProviderError, ProviderExecution,
};
use plumb_store::BranchName;
use plumb_validation::load_builtin_software_profile;
use serde_json::{json, Value};

pub const ACTOR: &str = "agent:cli";
pub const ASSISTANT: &str = "agent:assistant";
pub const STAGE_ACTOR: &str = "agent:compiler";
pub const T0: &str = "2026-01-01T00:00:00.000000000Z";

pub fn id(s: &str) -> Id {
    s.parse().unwrap()
}

/// The in-process runtime matching `config` with `provider` (mirrors the CLI's own build).
pub fn runtime(config: &Value, provider: Arc<Scripted>) -> ApplicationRuntime {
    let calendars: Vec<CalendarConfig> =
        serde_json::from_value(config["calendars"].clone()).unwrap();
    let managers: Vec<DirectManagerFact> =
        serde_json::from_value(config["direct_managers"].clone()).unwrap();
    let opt = |k: &str| {
        let v = &config[k];
        if v.is_null() {
            None
        } else {
            Some(v.clone())
        }
    };
    ApplicationRuntime {
        inference_provider: provider,
        provider_mode: plumb_api::ProviderMode::Replay {
            fixtures_hash: Hash::content_sha256(b"sim recorder"),
        },
        provider_policy: serde_json::from_value(config["provider_policy"].clone()).unwrap(),
        session_turn_config: serde_json::from_value(config["session_turn"].clone()).unwrap(),
        assistant_actor_ref: id(ASSISTANT),
        stage_actor_ref: id(STAGE_ACTOR),
        calendars: Arc::new(
            ExprStaticCalendarProvider::new(
                calendars
                    .iter()
                    .map(|c| ExprCalendarDefinition::new(c.calendar_ref.clone(), [], [])),
            )
            .unwrap(),
        ),
        predicates: Arc::new(DirectManagerPredicates::new(&managers).unwrap()),
        calendar_configs: calendars,
        direct_managers: managers,
        expression_scopes: serde_json::from_value(config["expression_scopes"].clone()).unwrap(),
        calculation_scope_overrides: serde_json::from_value(
            config["calculation_scope_overrides"].clone(),
        )
        .unwrap(),
        runtime_semantic_values: serde_json::from_value(config["runtime_semantic_values"].clone())
            .unwrap(),
        validation_policy: serde_json::from_value(config["validation_policy"].clone()).unwrap(),
        vocabulary_policy: serde_json::from_value(config["vocabulary_policy"].clone()).unwrap(),
        duplicate_policy: serde_json::from_value(config["duplicate_policy"].clone()).unwrap(),
        calculation_scope: opt("calculation_scope").map(|v| serde_json::from_value(v).unwrap()),
        operation_scope: opt("operation_scope").map(|v| serde_json::from_value(v).unwrap()),
        process_scope: opt("process_scope").map(|v| serde_json::from_value(v).unwrap()),
        authorization_draft: opt("authorization_draft").map(|v| serde_json::from_value(v).unwrap()),
        scenario_derivation: opt("scenario_derivation").map(|v| serde_json::from_value(v).unwrap()),
        verification_applicability: opt("verification_applicability")
            .map(|v| serde_json::from_value(v).unwrap()),
        gates: serde_json::from_value(config["gates"].clone()).unwrap(),
        directory: config
            .get("directory")
            .map(|d| serde_json::from_value(d.clone()).unwrap()),
        question_routing: config
            .get("question_routing")
            .map(|r| serde_json::from_value(r.clone()).unwrap()),
    }
}

// ---------------------------------------------------------------------- scripted model

/// A scripted model: answers one inference task kind from its request context, or `None`
/// when it has no script for that kind.
pub type Model = fn(&str, &Value) -> Option<Value>;

/// Answers each task kind with the scripted `model`, records every execution and dumps every
/// request context for inspection.
pub struct Scripted {
    pub dir: PathBuf,
    pub calls: Mutex<Vec<ProviderExecution>>,
    pub model: Model,
}

pub fn execution(request: &InferenceRequest, output: Value) -> ProviderExecution {
    let raw_response = to_canonical_json(&output).unwrap();
    let validated_output = CanonicalJson::new(output);
    ProviderExecution {
        artifact: InferenceArtifact {
            request_hash: request.id.clone(),
            provider: request.provider_policy.provider.clone(),
            model: "scripted-hr".to_owned(),
            parameters: CanonicalJson::new(json!({})),
            raw_response_hash: Hash::content_sha256(&raw_response),
            validated_output_hash: validated_output.content_hash().unwrap(),
            validated_output,
        },
        raw_response_media_type: "application/json".to_owned(),
        raw_response,
    }
}

/// The first exact grounding of `phrase` (case-insensitive; a space may also be a hyphen) in
/// the statements of `requirements`, preferring the statement containing `prefer`.
pub fn ground(requirements: &[Value], phrase: &str, prefer: Option<&str>) -> Option<Value> {
    let variants = [phrase.to_owned(), phrase.replacen(' ', "-", 1)];
    let mut ordered: Vec<&Value> = requirements.iter().collect();
    if let Some(prefer) = prefer {
        ordered.sort_by_key(|r| !r["statement"].as_str().unwrap().contains(prefer));
    }
    for r in ordered {
        let statement = r["statement"].as_str().unwrap();
        let lower = statement.to_lowercase();
        for v in &variants {
            if let Some(start) = lower.find(v.as_str()) {
                return Some(
                    json!({"requirement_ref": r["requirement_ref"], "start": start,
                                   "end": start + v.len()}),
                );
            }
        }
    }
    None
}

/// The exact case-sensitive grounding of `needle` in the statement containing `within`.
pub fn at(requirements: &[Value], within: &str, needle: &str) -> Option<Value> {
    let r = requirements
        .iter()
        .find(|r| r["statement"].as_str().unwrap().contains(within))?;
    let start = r["statement"].as_str().unwrap().find(needle)?;
    Some(
        json!({"requirement_ref": r["requirement_ref"], "start": start, "end": start + needle.len()}),
    )
}

/// The node_ref of the context `section` item whose payload name is `name`.
pub fn named(context: &Value, section: &str, name: &str) -> Option<Value> {
    context[section]
        .as_array()?
        .iter()
        .find(|i| i["payload"]["data"]["name"] == name)
        .map(|i| i["node_ref"].clone())
}

impl InferenceProvider for Scripted {
    fn execute(
        &self,
        request: &InferenceRequest,
        material: &InferenceRequestMaterial,
    ) -> Result<ProviderExecution, ProviderError> {
        validate_binding(request, material)?;
        let context: Value = serde_json::from_slice(&material.context_json).unwrap();
        let ctx_dir = self.dir.join("contexts");
        std::fs::create_dir_all(&ctx_dir).unwrap();
        std::fs::write(
            ctx_dir.join(format!("{}.json", request.task_kind)),
            serde_json::to_vec_pretty(&context).unwrap(),
        )
        .unwrap();
        let Some(output) = (self.model)(request.task_kind.as_str(), &context) else {
            eprintln!("SCRIPTED: no script for task kind {}", request.task_kind);
            return Err(ProviderError::ProviderDisabled);
        };
        let executed = execution(request, output);
        self.calls.lock().unwrap().push(executed.clone());
        Ok(executed)
    }
}

// ---------------------------------------------------------------------- driver

pub struct Sim {
    pub dir: PathBuf,
    pub db: PathBuf,
    pub config_path: PathBuf,
    pub config: std::cell::RefCell<Value>,
    pub model: Model,
    /// Committed replay mode: `Some((dir, regenerate))` reads (or, when regenerating, writes)
    /// one recorded-executions file per stage invocation in `dir` instead of recording live.
    pub replay: Option<(PathBuf, bool)>,
    stage_count: std::cell::Cell<usize>,
    _tmp: Option<tempfile::TempDir>,
}

impl Sim {
    pub fn new(config: Value, model: Model) -> Sim {
        let (dir, tmp) = match std::env::var("SIM_DIR") {
            Ok(d) => {
                let d = PathBuf::from(d);
                let _ = std::fs::remove_dir_all(&d);
                std::fs::create_dir_all(&d).unwrap();
                (d, None)
            }
            Err(_) => {
                let t = tempfile::tempdir().unwrap();
                (t.path().to_owned(), Some(t))
            }
        };
        let config_path = dir.join("plumb.json");
        std::fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        Sim {
            db: dir.join("plumb.sqlite"),
            dir,
            config_path,
            config: std::cell::RefCell::new(config),
            model,
            replay: None,
            stage_count: std::cell::Cell::new(0),
            _tmp: tmp,
        }
    }

    /// Runs the CLI; returns (exit code, stdout+stderr) and prints both.
    pub fn cli(&self, args: &[&str], extra: &[&str]) -> (i32, String) {
        let out = Command::cargo_bin("plumb-cli")
            .unwrap()
            .arg("--db")
            .arg(&self.db)
            .arg("--config")
            .arg(&self.config_path)
            .args(extra)
            .args(args)
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let code = out.status.code().unwrap();
        println!(
            "$ plumb {} {}\n{}[exit {code}]\n",
            extra.join(" "),
            args.join(" "),
            text
        );
        (code, text)
    }

    pub fn json(&self, args: &[&str]) -> Value {
        let out = Command::cargo_bin("plumb-cli")
            .unwrap()
            .arg("--db")
            .arg(&self.db)
            .arg("--config")
            .arg(&self.config_path)
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
            panic!(
                "{args:?}: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
    }

    /// Runs `recipe`: records the scripted executions over a copy of the database, then
    /// replays them through the CLI over the real database.
    pub fn stage(&self, recipe: &str) -> (i32, String) {
        let n = self.stage_count.get() + 1;
        self.stage_count.set(n);
        let committed = self
            .replay
            .as_ref()
            .map(|(dir, regenerate)| (dir.join(format!("{n:02}-{recipe}.json")), *regenerate));
        if let Some((file, false)) = &committed {
            // Network-free replay of the committed executions; a stage without a file made
            // no inference call when it was recorded.
            if !file.exists() {
                return self.cli(&[recipe], &[]);
            }
            return self.cli(
                &[recipe],
                &[
                    "--provider",
                    "replay",
                    "--replay-fixtures",
                    file.to_str().unwrap(),
                ],
            );
        }
        let copy = self.dir.join("recording.sqlite");
        std::fs::copy(&self.db, &copy).unwrap();
        let scripted = Arc::new(Scripted {
            dir: self.dir.clone(),
            calls: Mutex::new(Vec::new()),
            model: self.model,
        });
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
            Arc::new(FixedClock::new(T0.parse::<Timestamp>().unwrap())),
            load_builtin_software_profile().unwrap(),
            runtime(&self.config.borrow(), scripted.clone()),
        )
        .unwrap();
        match state.run_stage(Recipe::parse(recipe).unwrap(), None, None) {
            Ok(run) => {
                if let Err(e) = state.execute_recipe_job(&run.job_id) {
                    println!("(recording {recipe}: {e:?})");
                }
            }
            Err(e) => println!("(recording {recipe} not started: {e:?})"),
        }
        let calls = scripted.calls.lock().unwrap().clone();
        drop(state);
        let _ = std::fs::remove_file(&copy);
        if calls.is_empty() {
            return self.cli(&[recipe], &[]);
        }
        let fixtures = self.dir.join(format!("replay-{recipe}.json"));
        std::fs::write(&fixtures, serde_json::to_vec(&calls).unwrap()).unwrap();
        if let Some((file, true)) = &committed {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, serde_json::to_vec_pretty(&calls).unwrap()).unwrap();
        }
        self.cli(
            &[recipe],
            &[
                "--provider",
                "replay",
                "--replay-fixtures",
                fixtures.to_str().unwrap(),
            ],
        )
    }

    /// Replaces one top-level configuration value (what an analyst edits between runs).
    pub fn set(&self, key: &str, value: Value) {
        let mut config = self.config.borrow_mut();
        config[key] = value;
        std::fs::write(
            &self.config_path,
            serde_json::to_vec_pretty(&*config).unwrap(),
        )
        .unwrap();
        println!("(config {key} updated)\n");
    }

    /// The IDs of the Accepted nodes of `node_type`.
    pub fn accepted(&self, node_type: &str) -> Vec<String> {
        self.nodes()
            .into_iter()
            .filter(|n| n["status"] == "Accepted" && n["payload"]["type"] == node_type)
            .map(|n| n["id"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Every node of the head as JSON (read-only inspection).
    pub fn nodes(&self) -> Vec<Value> {
        let store = plumb_store::SqliteRevisionStore::open(&self.db).unwrap();
        let head = store.head(&BranchName::main()).unwrap().unwrap();
        let loaded = store.load_revision(&head).unwrap();
        loaded
            .graph
            .nodes()
            .values()
            .map(|n| serde_json::to_value(n).unwrap())
            .collect()
    }

    /// Prints node counts by type and status.
    pub fn census(&self) {
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        for n in self.nodes() {
            let key = format!(
                "{}/{}",
                n["payload"]["type"].as_str().unwrap_or("?"),
                n["status"].as_str().unwrap_or("?")
            );
            *counts.entry(key).or_default() += 1;
        }
        println!("census: {counts:?}\n");
    }

    /// Confirms every Proposed node of the given payload types through the CLI.
    pub fn confirm_all(&self, types: &[&str]) {
        let ids: Vec<String> = self
            .nodes()
            .into_iter()
            .filter(|n| {
                n["status"] == "Proposed"
                    && (types.is_empty()
                        || types.contains(&n["payload"]["type"].as_str().unwrap_or("")))
            })
            .map(|n| n["id"].as_str().unwrap().to_owned())
            .collect();
        if ids.is_empty() {
            println!("(nothing to confirm for {types:?})\n");
            return;
        }
        let mut args = vec!["confirm"];
        args.extend(ids.iter().map(String::as_str));
        args.extend(["--reason", "Reviewed in simulation."]);
        self.cli(&args, &[]);
    }

    /// Runs `recipe`, accepts and confirms everything it proposes, and reruns it until it
    /// proposes nothing new (what a reviewing analyst does after enrichment).
    pub fn settle(&self, recipe: &str) {
        for round in 1..=4 {
            println!("===== {recipe} (round {round})");
            let (code, _) = self.stage(recipe);
            let pending = self.json(&["proposals"]);
            let count = pending.as_array().map_or(0, Vec::len);
            if count == 0 {
                self.census();
                return;
            }
            self.accept_all();
            self.confirm_all(&[]);
            self.census();
            if code != 0 {
                return;
            }
        }
        println!("!!!!! {recipe} still proposing after 4 rounds");
    }

    /// Accepts every pending proposal of the head as one set (falling back to one by one).
    pub fn accept_all(&self) {
        let pending = self.json(&["proposals"]);
        let items = pending
            .as_array()
            .cloned()
            .unwrap_or_else(|| pending["proposals"].as_array().cloned().unwrap_or_default());
        if items.is_empty() {
            println!("(no pending proposals)\n");
            return;
        }
        let members: Vec<String> = items
            .iter()
            .map(|p| {
                format!(
                    "{}={}",
                    p["proposal_id"].as_str().unwrap(),
                    p["action"].as_str().unwrap()
                )
            })
            .collect();
        let mut args = vec!["accept-set"];
        args.extend(members.iter().map(String::as_str));
        args.extend(["--reason", "Reviewed in simulation."]);
        let (code, _) = self.cli(&args, &[]);
        if code != 0 {
            for p in &items {
                self.cli(
                    &[
                        "apply",
                        p["proposal_id"].as_str().unwrap(),
                        "--action",
                        p["action"].as_str().unwrap(),
                        "--reason",
                        "Reviewed in simulation.",
                    ],
                    &[],
                );
            }
        }
    }
}
