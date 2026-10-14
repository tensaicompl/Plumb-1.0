//! The CLI command implementations (A0.4, Hotfix 069): each command calls the same public
//! application services as HTTP over one in-process `ApiState`. A write uses the head it loads
//! as its If-Match equivalent and the store CAS as final authority; nothing is reloaded,
//! retried, rebased or resubmitted.

use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use plumb_api::application::{
    bootstrap_project_database, export_artifact, open_application, round_size, ApplicationError,
    ProposalAction,
};
use plumb_api::error::ApiError;
use plumb_api::openapi::{ExportFormat, JobStatus};
use plumb_api::{serve_blocking, ApiState, Recipe};
use plumb_core::{to_canonical_json, Clock, Id, SystemClock};
use plumb_store::BranchName;
use plumb_validation::load_builtin_software_profile;
use serde::Serialize;
use serde_json::{json, Value};
use thiserror::Error;

use crate::config::{CliConfig, ConfigError, Provider};

/// A failed command (exit code 1).
#[derive(Debug, Error)]
pub enum CliError {
    #[error("{}: {}", .0.body().code, .0.body().message)]
    Api(ApiError),
    #[error("E_PROVIDER_UNAVAILABLE: {0}")]
    ProviderUnavailable(ConfigError),
    #[error("{0}")]
    Config(ConfigError),
    #[error("{0}")]
    Application(#[from] ApplicationError),
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Io(String),
    /// A recipe that ended failed or stale; its job is still reported.
    #[error("the {recipe} recipe ended {status}")]
    Recipe {
        recipe: &'static str,
        status: &'static str,
        done: Box<Done>,
    },
}

impl From<ApiError> for CliError {
    fn from(error: ApiError) -> CliError {
        CliError::Api(error)
    }
}

impl From<ConfigError> for CliError {
    fn from(error: ConfigError) -> CliError {
        match error {
            ConfigError::ProviderUnavailable => CliError::ProviderUnavailable(error),
            other => CliError::Config(other),
        }
    }
}

impl CliError {
    /// The structured error printed with `--json`.
    pub fn json(&self) -> Value {
        match self {
            CliError::Api(error) => {
                let body = error.body();
                json!({ "code": body.code, "message": body.message, "details": body.details })
            }
            CliError::ProviderUnavailable(error) => {
                json!({ "code": "E_PROVIDER_UNAVAILABLE", "message": error.to_string() })
            }
            other => json!({ "message": other.to_string() }),
        }
    }
}

/// The result of a completed command.
#[derive(Debug)]
pub struct Done {
    /// The structured result (`--json`).
    pub json: Value,
    /// The human-readable result.
    pub text: String,
    /// Raw bytes written instead of text (`export`).
    pub raw: Option<Vec<u8>>,
    /// Whether an evaluated gate failed (exit code 2).
    pub gate_failed: bool,
}

impl Done {
    fn new<T: Serialize>(value: &T, text: String) -> Result<Done, CliError> {
        Ok(Done {
            json: serde_json::to_value(value).map_err(|e| CliError::Io(e.to_string()))?,
            text,
            raw: None,
            gate_failed: false,
        })
    }
}

/// `init --project <ID>` (R0.1): bootstraps a new project database at `db` on branch `main`
/// with the configured Agents; it never opens an existing database or builds a provider.
pub fn init(
    db: &Path,
    branch: &str,
    project: &str,
    config: &CliConfig,
    clock: &dyn Clock,
) -> Result<Done, CliError> {
    if branch != BranchName::main().as_str() {
        return Err(CliError::Usage(format!(
            "init creates only branch main, not {branch:?}"
        )));
    }
    let project: Id = project
        .parse()
        .map_err(|_| CliError::Usage(format!("{project:?} is not a project ID")))?;
    let (identity, stage_actor_ref, assistant_actor_ref) = config.bootstrap_actors()?;
    let profile = load_builtin_software_profile()
        .map_err(|e| CliError::Usage(format!("validation profile: {e}")))?;
    let result = bootstrap_project_database(
        db,
        project,
        &profile,
        &identity,
        &stage_actor_ref,
        &assistant_actor_ref,
        clock,
    )?;
    let text = format!(
        "project: {}\nbranch: {}\nrevision: {}\nsemantic_hash: {}\nevidence_hash: {}",
        result.project_id,
        result.branch,
        result.revision_id,
        result.semantic_hash,
        result.evidence_hash
    );
    Done::new(&result, text)
}

/// Opens the application over an existing, initialized database.
pub fn open(
    db: &Path,
    branch: &str,
    config: &CliConfig,
    provider: Provider,
) -> Result<ApiState, CliError> {
    if !db.is_file() {
        return Err(CliError::Usage(format!(
            "database {} does not exist",
            db.display()
        )));
    }
    let branch: BranchName = branch
        .parse()
        .map_err(|_| CliError::Usage(format!("{branch:?} is not a branch name")))?;
    let profile = load_builtin_software_profile()
        .map_err(|e| CliError::Usage(format!("validation profile: {e}")))?;
    let runtime = config.runtime(provider)?;
    Ok(open_application(
        db,
        branch,
        config.identity()?,
        Arc::new(SystemClock),
        profile,
        runtime,
    )?)
}

/// `import <FILE> --media-type --name`: the shared `create_source` service.
pub fn import(
    state: &ApiState,
    file: &Path,
    media_type: &str,
    name: &str,
) -> Result<Done, CliError> {
    let bytes = std::fs::read(file)
        .map_err(|e| CliError::Io(format!("cannot read {}: {e}", file.display())))?;
    let loaded = state.head()?;
    let result = state.create_source(&loaded, name, media_type, &bytes)?;
    let text = format!(
        "imported source {} ({} spans) at revision {}",
        result.id, result.spans_count, result.revision
    );
    Done::new(&result, text)
}

/// A stage command: captures the StageRecipeJob over the head, executes it in-process to
/// completion and reports it. A failed gate step is exit 2; a failed or stale recipe exit 1.
pub fn stage(state: &ApiState, recipe: Recipe, scope: Vec<String>) -> Result<Done, CliError> {
    let scope = if scope.is_empty() { None } else { Some(scope) };
    let run = state.run_stage(recipe, scope, None)?;
    state.execute_recipe_job(&run.job_id)?;
    let (job, _) = state.get_job(run.job_id.as_str())?;
    let gates = state.recipe_gate_results(&run.job_id)?;
    let mut value = serde_json::to_value(&job).map_err(|e| CliError::Io(e.to_string()))?;
    value["job_id"] = Value::String(run.job_id.to_string());
    let status = serde_json::to_value(job.status)
        .ok()
        .and_then(|s| s.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut text = format!(
        "job {}: {status} (base {}, result {})",
        run.job_id,
        job.base_revision,
        job.result_version.as_deref().unwrap_or("-")
    );
    for (gate, result) in &gates {
        text.push_str(&format!("\n{gate}: {result}"));
    }
    if let Some(error) = &job.error {
        text.push_str(&format!("\nerror: {error}"));
    }
    let done = Done {
        json: value,
        text,
        raw: None,
        gate_failed: gates.iter().any(|(_, result)| result == "fail"),
    };
    match job.status {
        JobStatus::Ready | JobStatus::Committed => Ok(done),
        JobStatus::Failed => Err(CliError::Recipe {
            recipe: recipe.as_str(),
            status: "failed",
            done: Box::new(done),
        }),
        JobStatus::Stale => Err(CliError::Recipe {
            recipe: recipe.as_str(),
            status: "stale",
            done: Box::new(done),
        }),
        // Executed in-process to completion: a nonterminal status is an execution error.
        _ => Err(CliError::Recipe {
            recipe: recipe.as_str(),
            status: "incomplete",
            done: Box::new(done),
        }),
    }
}

/// `vocab`: the complete glossary projection.
pub fn vocab(state: &ApiState) -> Result<Done, CliError> {
    let loaded = state.head()?;
    let terms = state.glossary(&loaded)?;
    let text = terms
        .iter()
        .map(|t| format!("{}\t{}\t{}", t.id, t.term, t.status))
        .collect::<Vec<_>>()
        .join("\n");
    Done::new(&terms, text)
}

/// `questions`: the complete Question projection.
pub fn questions(state: &ApiState) -> Result<Done, CliError> {
    let loaded = state.head()?;
    let questions = state.questions(&loaded)?;
    let mut lines = Vec::with_capacity(questions.len());
    for q in &questions {
        let status = serde_json::to_value(q.status).map_err(|e| CliError::Io(e.to_string()))?;
        lines.push(format!(
            "{}\t{}\t{}",
            q.id,
            status.as_str().unwrap_or_default(),
            q.text
        ));
    }
    Done::new(&questions, lines.join("\n"))
}

/// `rounds create --stakeholder --max`: the shared `create_round` service; `max` is explicit.
pub fn rounds_create(state: &ApiState, stakeholder: &str, max: u64) -> Result<Done, CliError> {
    let stakeholder: Id = stakeholder
        .parse()
        .map_err(|_| ApiError::invalid("stakeholder is not an ID"))?;
    let size = round_size(max)?;
    let loaded = state.head()?;
    let result = state.create_round(&loaded, &stakeholder, size)?;
    let text = format!(
        "round {} for {} ({} questions) at revision {}",
        result.round.id,
        result.round.stakeholder,
        result.round.questions.len(),
        result.revision
    );
    Done::new(&result, text)
}

/// `rounds show <ID>`: the shared round reconstruction.
pub fn rounds_show(state: &ApiState, id: &str) -> Result<Done, CliError> {
    let loaded = state.head()?;
    let round = state.round(&loaded, id)?;
    let text = format!(
        "round {} for {}\n{}",
        round.id,
        round.stakeholder,
        round.questions.join("\n")
    );
    Done::new(&round, text)
}

/// `apply <PROPOSAL_ID> --action [--reason]`: the shared proposal-action service.
pub fn apply(
    state: &ApiState,
    proposal: &str,
    action: ProposalAction,
    reason: Option<String>,
) -> Result<Done, CliError> {
    let loaded = state.head()?;
    let result = state.act_on_proposal(&loaded, proposal, action, reason)?;
    let text = format!(
        "{} {proposal}: revision {}",
        action.operation_id(),
        result.revision
    );
    Done::new(&result, text)
}

/// `ready`: I0, F1, F2, F3 and F4 on one head; any failed gate is exit 2.
pub fn ready(state: &ApiState) -> Result<Done, CliError> {
    let outcome = state.ready()?;
    let mut text = format!(
        "revision {} ({}) at {}",
        outcome.revision,
        outcome.semantic_hash,
        to_canonical_json(&outcome.evaluation_at)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default()
            .trim_matches('"')
    );
    for gate in &outcome.gates {
        text.push_str(&format!("\n{}: {}", gate.gate, gate.result));
    }
    let mut done = Done::new(&outcome, text)?;
    done.gate_failed = !outcome.passed();
    Ok(done)
}

/// `export --format`: the selected artifact of the shared pilot projection, written raw.
pub fn export(state: &ApiState, format: &str) -> Result<Done, CliError> {
    let format: ExportFormat = serde_json::from_value(Value::String(format.to_owned()))
        .map_err(|_| ApiError::invalid(format!("{format:?} is not an export format")))?;
    let loaded = state.head()?;
    let artifact = export_artifact(state.pilot_projection(&loaded)?, format);
    Ok(Done {
        json: Value::Null,
        text: String::new(),
        raw: Some(artifact.bytes),
        gate_failed: false,
    })
}

/// `report --out <DIR>`: the six artifacts of the shared pilot projection under their
/// `file_name()`.
pub fn report(state: &ApiState, out: &Path) -> Result<Done, CliError> {
    let loaded = state.head()?;
    let outputs = state.pilot_projection(&loaded)?;
    std::fs::create_dir_all(out)
        .map_err(|e| CliError::Io(format!("cannot create {}: {e}", out.display())))?;
    let mut files = Vec::new();
    for artifact in outputs.artifacts() {
        let name = artifact.kind.file_name();
        let path = out.join(name);
        std::fs::write(&path, &artifact.bytes)
            .map_err(|e| CliError::Io(format!("cannot write {}: {e}", path.display())))?;
        files.push(json!({ "file": name, "content_hash": artifact.content_hash }));
    }
    let text = files
        .iter()
        .map(|f| {
            format!(
                "{}\t{}",
                f["file"].as_str().unwrap_or_default(),
                f["content_hash"].as_str().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Done {
        json: json!({
            "revision": loaded.revision.id.to_string(),
            "semantic_hash": loaded.revision.semantic_hash.to_string(),
            "files": files,
        }),
        text,
        raw: None,
        gate_failed: false,
    })
}

/// `serve --listen`: the HTTP server of `plumb-api` over this state until Ctrl-C.
pub fn serve(state: ApiState, listen: SocketAddr) -> Result<Done, CliError> {
    serve_blocking(state, listen).map_err(|e| CliError::Io(format!("serve: {e}")))?;
    Ok(Done {
        json: json!({ "listen": listen.to_string() }),
        text: String::new(),
        raw: None,
        gate_failed: false,
    })
}

/// The two effort kinds.
pub const EFFORT_KINDS: [&str; 2] = ["stakeholder_hours", "analyst_days"];

/// `^[0-9]+(\.[0-9]{1,2})?$` and greater than zero.
fn valid_amount(amount: &str) -> bool {
    let (int, frac) = match amount.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (amount, None),
    };
    !int.is_empty()
        && int.bytes().all(|c| c.is_ascii_digit())
        && frac.is_none_or(|f| (1..=2).contains(&f.len()) && f.bytes().all(|c| c.is_ascii_digit()))
        && amount.bytes().any(|c| (b'1'..=b'9').contains(&c))
}

/// Non-empty, no surrounding whitespace, no control characters.
fn clean(text: &str) -> bool {
    !text.is_empty() && text.trim() == text && !text.chars().any(char::is_control)
}

/// `log-effort`: appends one RFC 8785 line `{version, recorded_at, actor_ref, kind, amount,
/// note}` and a newline to `file`; no PSG state, revision, event or gate is touched.
pub fn log_effort(
    config: &CliConfig,
    clock: &dyn Clock,
    file: &Path,
    kind: &str,
    amount: &str,
    note: Option<&str>,
) -> Result<Done, CliError> {
    if !EFFORT_KINDS.contains(&kind) {
        return Err(CliError::Usage(format!(
            "kind must be stakeholder_hours or analyst_days, not {kind:?}"
        )));
    }
    if !valid_amount(amount) {
        return Err(CliError::Usage(format!(
            "amount {amount:?} must match ^[0-9]+(\\.[0-9]{{1,2}})?$ and be greater than zero"
        )));
    }
    if let Some(note) = note {
        if !clean(note) {
            return Err(CliError::Usage(
                "note must be non-empty, without surrounding whitespace or control characters"
                    .into(),
            ));
        }
    }
    let record = json!({
        "version": 1,
        "recorded_at": clock.now(),
        "actor_ref": config.identity.actor_ref,
        "kind": kind,
        "amount": amount,
        "note": note,
    });
    let mut line = to_canonical_json(&record).map_err(|e| CliError::Io(e.to_string()))?;
    line.push(b'\n');
    let mut handle = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .map_err(|e| CliError::Io(format!("cannot open {}: {e}", file.display())))?;
    handle
        .write_all(&line)
        .map_err(|e| CliError::Io(format!("cannot append to {}: {e}", file.display())))?;
    let text = String::from_utf8_lossy(&line).trim_end().to_owned();
    Ok(Done {
        json: record,
        text,
        raw: None,
        gate_failed: false,
    })
}
