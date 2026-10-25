//! The CLI command implementations (A0.4, Hotfix 069): each command calls the same public
//! application services as HTTP over one in-process `ApiState`. A write uses the head it loads
//! as its If-Match equivalent and the store CAS as final authority; nothing is reloaded,
//! retried, rebased or resubmitted.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use plumb_api::application::{
    bootstrap_project_database, export_artifact, open_application, require_importable, round_size,
    ApplicationError, ProposalAction,
};
use plumb_api::error::ApiError;
use plumb_api::openapi::{ExportFormat, JobStatus};
use plumb_api::{serve_blocking, ApiState, ReadyRule, Recipe};
use plumb_core::{to_canonical_json, Clock, Id, SystemClock};
use plumb_import::docx::{check_input_size, DOCX_MAX_INPUT_BYTES};
use plumb_import::{check_text_input_size, ImportError, SourceKind, TEXT_MAX_INPUT_BYTES};
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

/// Whether `media_type` is DOCX; every other importable media type is Markdown or plain text.
fn is_docx(media_type: &str) -> bool {
    media_type == SourceKind::Docx.media_type()
}

/// The content limit of a supported import media type (architecture §33.1).
fn import_limit(media_type: &str) -> Result<u64, CliError> {
    require_importable(media_type)?;
    Ok(if is_docx(media_type) {
        DOCX_MAX_INPUT_BYTES
    } else {
        TEXT_MAX_INPUT_BYTES as u64
    })
}

/// Checks `bytes` against the media type's limit with the importer's own typed limit error.
fn check_import_size(media_type: &str, bytes: u64) -> Result<(), ImportError> {
    if is_docx(media_type) {
        check_input_size(bytes)
    } else {
        check_text_input_size(usize::try_from(bytes).unwrap_or(usize::MAX))
    }
}

/// The import service's rejection of an oversized file, as `create_source` reports it.
fn oversized(error: ImportError) -> CliError {
    CliError::Api(
        ApiError::invalid("the uploaded file cannot be imported as its declared media type")
            .with_details(json!({ "import_error": error.to_string() })),
    )
}

/// Reads `file` for import: the metadata length is checked against the declared media type's
/// limit before reading, then at most limit + 1 bytes are read, so an oversized or growing file
/// is never read into memory whole.
fn read_import_file(file: &Path, media_type: &str) -> Result<Vec<u8>, CliError> {
    let limit = import_limit(media_type)?;
    let check = |bytes: u64| check_import_size(media_type, bytes).map_err(oversized);
    let io = |e: std::io::Error| CliError::Io(format!("cannot read {}: {e}", file.display()));
    check(std::fs::metadata(file).map_err(io)?.len())?;
    let mut bytes = Vec::new();
    File::open(file)
        .map_err(io)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    check(bytes.len() as u64)?;
    Ok(bytes)
}

/// `import <FILE> --media-type --name`: the shared `create_source` service.
pub fn import(
    state: &ApiState,
    file: &Path,
    media_type: &str,
    name: &str,
) -> Result<Done, CliError> {
    let bytes = read_import_file(file, media_type)?;
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
    let gates = state.recipe_gate_details(&run.job_id)?;
    let dispositions = state.recipe_dispositions(&run.job_id)?;
    let mut value = serde_json::to_value(&job).map_err(|e| CliError::Io(e.to_string()))?;
    value["job_id"] = Value::String(run.job_id.to_string());
    value["dispositions"] = Value::Array(dispositions.clone());
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
    for (gate, result, failed) in &gates {
        text.push_str(&format!("\n{gate}: {result}"));
        push_failed_rules(&mut text, failed);
    }
    for d in &dispositions {
        text.push_str(&format!(
            "\n  not proposed ({} {} {}): {}",
            d["phase"].as_str().unwrap_or("?"),
            d["analyser"].as_str().unwrap_or("?"),
            d["kind"].as_str().unwrap_or("?"),
            d["detail"]
        ));
    }
    if let Some(error) = &job.error {
        text.push_str(&format!("\nerror: {error}"));
    }
    let done = Done {
        json: value,
        text,
        raw: None,
        gate_failed: gates.iter().any(|(_, result, _)| result == "fail"),
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

/// `directory` (R0.6): persists the configured directory as pending Semantic / HUMAN_CONFIRM
/// proposals through the shared directory service; nothing is committed.
pub fn directory(state: &ApiState) -> Result<Done, CliError> {
    let loaded = state.head()?;
    let elements = state.persist_directory(&loaded)?;
    let text = elements
        .iter()
        .map(|e| match &e.proposal_id {
            Some(proposal) => format!(
                "{} {}: proposed {} ({})",
                e.node_type,
                e.element_ref,
                proposal,
                e.action.as_deref().unwrap_or_default()
            ),
            None => format!("{} {}: existing", e.node_type, e.element_ref),
        })
        .collect::<Vec<_>>()
        .join("\n");
    Done::new(&elements, text)
}

/// `confirm <ELEMENT_ID>... [--reason]` (R0.6): the shared generic governed confirmation.
pub fn confirm(
    state: &ApiState,
    elements: &[String],
    reason: Option<String>,
) -> Result<Done, CliError> {
    // Terms, Calculations and Scenarios have dedicated confirmations (one revision each);
    // every other element goes through the generic governed confirmation in one revision.
    let mut generic = Vec::new();
    let mut last = None;
    for element in elements {
        let loaded = state.head()?;
        match state.confirm_dedicated(&loaded, element)? {
            Some(result) => last = Some(result),
            None => generic.push(element.clone()),
        }
    }
    if !generic.is_empty() {
        let loaded = state.head()?;
        last = Some(state.confirm_elements(&loaded, &generic, reason)?);
    }
    let result = last.expect("confirm needs at least one element");
    let text = format!(
        "confirmed {}: revision {}",
        elements.join(" "),
        result.revision
    );
    Done::new(&result, text)
}

/// `findings list`: every current gate finding with disposition and governance (read-only).
pub fn findings_list(state: &ApiState) -> Result<Done, CliError> {
    let loaded = state.head()?;
    let findings = state.findings(&loaded)?;
    let text = findings
        .iter()
        .map(|f| {
            format!(
                "{} {} {} {} {}{}{} [{}]",
                f.id,
                f.gate,
                f.severity,
                f.code,
                f.disposition,
                if f.governed { " governed" } else { "" },
                if f.materialized { " materialized" } else { "" },
                f.affected_refs.join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Done::new(&findings, text)
}

/// `findings materialize`: persist actionable findings and close stale Open ones.
pub fn findings_materialize(state: &ApiState) -> Result<Done, CliError> {
    let loaded = state.head()?;
    match state.materialize_gate_findings(&loaded)? {
        Some(result) => {
            let text = format!("materialized findings: revision {}", result.revision);
            Done::new(&result, text)
        }
        None => Done::new(&serde_json::Value::Null, "no change".to_owned()),
    }
}

/// `findings waive`: the governed waiver of one current, materialized finding.
pub fn findings_waive(state: &ApiState, finding: &str, reason: &str) -> Result<Done, CliError> {
    let loaded = state.head()?;
    let result = state.waive_finding(&loaded, finding, reason)?;
    let text = format!("waived {finding}: revision {}", result.revision);
    Done::new(&result, text)
}

/// `set-attribute-type`: the shared `set_attribute_type` service on the head.
pub fn set_attribute_type(
    state: &ApiState,
    attribute: &str,
    value_type: &str,
) -> Result<Done, CliError> {
    let loaded = state.head()?;
    let result = state.set_attribute_type(&loaded, attribute, value_type)?;
    let text = format!(
        "attribute {attribute} typed {value_type}: revision {}",
        result.revision
    );
    Done::new(&result, text)
}

/// `ask` (R0.6): materializes questions from the current F2 report findings with the
/// configured question routing through the shared materialization service.
pub fn ask(state: &ApiState) -> Result<Done, CliError> {
    let loaded = state.head()?;
    let materialized = state.ask(&loaded)?;
    let questions: Vec<Value> = materialized
        .generation
        .questions
        .iter()
        .map(|q| {
            json!({
                "question_ref": q.question_ref,
                "template_id": q.template_id,
                "finding_ref": q.finding_ref,
                "stakeholder_ref": q.payload.stakeholder_ref,
                "priority": q.priority,
                "materialization": q.materialization,
            })
        })
        .collect();
    let value = json!({
        "questions": questions,
        "unmapped": materialized.generation.unmapped,
        "write": materialized.write,
    });
    let mut text = match &materialized.write {
        Some(write) => format!("revision {}", write.revision),
        None => "no change".to_owned(),
    };
    for q in &materialized.generation.questions {
        text.push_str(&format!("\n{} {}", q.question_ref, q.template_id));
    }
    Done::new(&value, text)
}

/// `answer <QUESTION_ID> --answer <JSON>` (R0.6): the shared S3.3 answer service; the result
/// is a pending governed proposal, never a commit.
pub fn answer(
    state: &ApiState,
    question: &str,
    answer: &str,
    rationale: Option<String>,
) -> Result<Done, CliError> {
    let value: Value = serde_json::from_str(answer)
        .map_err(|e| CliError::Usage(format!("--answer is not JSON: {e}")))?;
    if !value.is_object() {
        return Err(CliError::Api(ApiError::invalid(
            "answer must be a JSON object",
        )));
    }
    let loaded = state.head()?;
    let response = state.answer_question(&loaded, question, value, rationale, None)?;
    let action = serde_json::to_value(response.action)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let text = format!(
        "proposal {} ({action}) on revision {}",
        response.proposal_id, response.revision
    );
    Done::new(&response, text)
}

/// `proposals` (R0.6): every pending proposal sorted by ID, read-only.
pub fn proposals(state: &ApiState, refresh: bool) -> Result<Done, CliError> {
    let mut refreshed = Vec::new();
    if refresh {
        let loaded = state.head()?;
        for (stale, outcome) in state.refresh_stale_answers(&loaded)? {
            refreshed.push(match outcome {
                Ok(new) => format!("{stale} re-submitted as {new}"),
                Err(why) => format!("{stale} rejected: {why}"),
            });
        }
    }
    let pending = state.pending_proposals()?;
    let text = pending
        .iter()
        .map(|p| {
            let origin = match (&p.recipe_job_id, &p.stage_job_id, &p.origin_ref) {
                (Some(recipe), Some(stage), _) => {
                    format!("{} recipe_job={recipe} stage_job={stage}", p.origin)
                }
                (_, _, Some(reference)) => format!("{} {reference}", p.origin),
                _ => p.origin.clone(),
            };
            format!(
                "{} {} origin={} base={} requires_reason={}{}",
                p.proposal_id,
                p.action,
                origin,
                p.base_revision,
                p.requires_reason,
                if p.stale { " STALE" } else { "" }
            )
        })
        .collect::<Vec<_>>();
    let text = refreshed
        .into_iter()
        .chain(text)
        .collect::<Vec<_>>()
        .join("\n");
    Done::new(&pending, text)
}

/// The five positive set actions (R0.3); `reject` is not a set action.
const SET_ACTIONS: [(&str, ProposalAction); 5] = [
    ("accept", ProposalAction::Accept),
    ("create", ProposalAction::Create),
    ("link", ProposalAction::Link),
    ("merge", ProposalAction::Merge),
    ("supersede", ProposalAction::Supersede),
];

/// Validates `accept-set` members `<PROPOSAL_ID>=<ACTION>`: a valid ID, a positive action and
/// no duplicate proposal; a malformed member is a usage error.
pub fn parse_set_members(members: &[String]) -> Result<(), CliError> {
    set_members(members).map(|_| ())
}

fn set_members(members: &[String]) -> Result<Vec<(String, ProposalAction)>, CliError> {
    let usage = |m: String| CliError::Usage(m);
    if members.is_empty() {
        return Err(usage(
            "accept-set needs at least one <PROPOSAL_ID>=<ACTION>".into(),
        ));
    }
    let mut parsed: Vec<(String, ProposalAction)> = Vec::with_capacity(members.len());
    for member in members {
        let (id, action) = member
            .split_once('=')
            .ok_or_else(|| usage(format!("{member:?} is not <PROPOSAL_ID>=<ACTION>")))?;
        if id.is_empty() || action.is_empty() {
            return Err(usage(format!("{member:?} is not <PROPOSAL_ID>=<ACTION>")));
        }
        if id.parse::<Id>().is_err() {
            return Err(usage(format!("{id:?} is not a proposal ID")));
        }
        let action = SET_ACTIONS
            .iter()
            .find(|(name, _)| *name == action)
            .map(|(_, a)| *a)
            .ok_or_else(|| {
                usage(format!(
                    "{action:?} is not a set action (accept, create, link, merge, supersede)"
                ))
            })?;
        if parsed.iter().any(|(seen, _)| seen == id) {
            return Err(usage(format!("proposal {id} occurs twice")));
        }
        parsed.push((id.to_owned(), action));
    }
    Ok(parsed)
}

/// `accept-set <PROPOSAL_ID>=<ACTION>... [--reason]`: the shared `accept_proposal_set`
/// service over one loaded head; nothing is reloaded, retried or rebased.
pub fn accept_set(
    state: &ApiState,
    members: Vec<String>,
    reason: Option<String>,
) -> Result<Done, CliError> {
    let members = set_members(&members)?;
    let loaded = state.head()?;
    let result = state.accept_proposal_set(&loaded, &members, reason)?;
    let text = format!(
        "accepted {} proposals: revision {}",
        members.len(),
        result.revision
    );
    Done::new(&result, text)
}

/// `ready`: I0, F1, F2, F3 and F4 on one head; any failed gate or unmet product precondition
/// (R0.4) is exit 2.
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
        push_failed_rules(&mut text, &gate.failed_rules);
    }
    for precondition in &outcome.preconditions {
        text.push_str(&format!("\nprecondition: {precondition}"));
    }
    let mut done = Done::new(&outcome, text)?;
    done.gate_failed = !outcome.passed();
    Ok(done)
}

/// One indented line per non-passing rule: state, severity, rule and its first targets.
fn push_failed_rules(text: &mut String, rules: &[ReadyRule]) {
    for rule in rules {
        let shown: Vec<String> = rule.targets.iter().take(5).map(Id::to_string).collect();
        let more = rule.targets.len().saturating_sub(shown.len());
        text.push_str(&format!(
            "\n  {} {} {} [{}{}]",
            rule.state.as_str().unwrap_or("?"),
            rule.severity.as_str().unwrap_or("?"),
            rule.rule_id,
            shown.join(" "),
            if more > 0 {
                format!(" +{more} more")
            } else {
                String::new()
            }
        ));
    }
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
