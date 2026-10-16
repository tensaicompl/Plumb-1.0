//! `plumb`: the pilot CLI (A0.4). Every command runs in-process over the same public application
//! services as HTTP; `serve` hands the same state to the `plumb-api` server.
//!
//! Exit codes: 0 success; 2 the operation completed but a gate it evaluated failed (`lint` F1,
//! `analyse` F2, `run` F4, `ready` any); 1 any operational, configuration, specification,
//! validation, provider or persistence error and a failed or stale recipe (including argument
//! errors).

mod commands;
mod config;

use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use plumb_api::application::ProposalAction;
use plumb_api::Recipe;
use plumb_core::SystemClock;

use crate::commands::{CliError, Done};
use crate::config::{CliConfig, Provider, ProviderChoice};

#[derive(Debug, Parser)]
#[command(name = "plumb", version, about = "Plumb pilot CLI")]
struct Cli {
    /// The pilot SQLite database (required by every command except log-effort; init creates it).
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    /// The branch.
    #[arg(long, global = true, default_value = "main")]
    branch: String,
    /// The strict JSON configuration file (required by every command).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// The inference provider.
    #[arg(long, global = true, value_enum, default_value_t = ProviderArg::Null)]
    provider: ProviderArg,
    /// The replay fixtures (exactly with --provider replay).
    #[arg(long, global = true)]
    replay_fixtures: Option<PathBuf>,
    /// The Anthropic key file (exactly with --provider anthropic).
    #[arg(long, global = true)]
    anthropic_api_key_file: Option<PathBuf>,
    /// Print the structured result as JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ProviderArg {
    Null,
    Replay,
    Anthropic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ActionArg {
    Accept,
    Create,
    Link,
    Merge,
    Supersede,
    Reject,
}

impl ActionArg {
    fn action(self) -> ProposalAction {
        match self {
            ActionArg::Accept => ProposalAction::Accept,
            ActionArg::Create => ProposalAction::Create,
            ActionArg::Link => ProposalAction::Link,
            ActionArg::Merge => ProposalAction::Merge,
            ActionArg::Supersede => ProposalAction::Supersede,
            ActionArg::Reject => ProposalAction::Reject,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum EffortKind {
    #[value(name = "stakeholder_hours")]
    StakeholderHours,
    #[value(name = "analyst_days")]
    AnalystDays,
}

impl EffortKind {
    fn as_str(self) -> &'static str {
        match self {
            EffortKind::StakeholderHours => "stakeholder_hours",
            EffortKind::AnalystDays => "analyst_days",
        }
    }
}

#[derive(Debug, clap::Args)]
struct Scope {
    /// An exact element ID (repeatable); absent means the recipe's whole eligible set.
    #[arg(long = "scope")]
    scope: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new project database on branch main.
    Init {
        #[arg(long)]
        project: String,
    },
    /// Import one source file as evidence.
    Import {
        file: PathBuf,
        #[arg(long)]
        media_type: String,
        #[arg(long)]
        name: String,
    },
    /// Run the lint recipe (F1).
    Lint(Scope),
    /// List the glossary.
    Vocab,
    /// Run the extract recipe.
    Extract(Scope),
    /// Run the consolidate recipe.
    Consolidate(Scope),
    /// Run the calc recipe.
    Calc(Scope),
    /// Run the rules recipe.
    Rules(Scope),
    /// Run the analyse recipe (F2 when configured).
    Analyse(Scope),
    /// List the questions.
    Questions,
    /// Create or show question rounds.
    Rounds {
        #[command(subcommand)]
        command: RoundsCommand,
    },
    /// Act on a pending proposal.
    Apply {
        proposal: String,
        #[arg(long, value_enum)]
        action: ActionArg,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Accept compatible pending proposals of the head atomically in one revision.
    AcceptSet {
        /// `<PROPOSAL_ID>=<ACTION>` with ACTION accept, create, link, merge or supersede.
        #[arg(required = true, value_name = "PROPOSAL_ID=ACTION")]
        members: Vec<String>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Run the flows recipe.
    Flows(Scope),
    /// Run the scenarios recipe.
    Scenarios(Scope),
    /// Run the run recipe (F4).
    Run(Scope),
    /// Run the tags recipe.
    Tags(Scope),
    /// Evaluate I0, F1, F2, F3 and F4 on the head.
    Ready,
    /// Export one artifact of the pilot projection.
    Export {
        #[arg(long)]
        format: String,
    },
    /// Write the six pilot projection artifacts.
    Report {
        #[arg(long)]
        out: PathBuf,
    },
    /// Serve the HTTP API.
    Serve {
        #[arg(long, default_value = "127.0.0.1:8080")]
        listen: SocketAddr,
    },
    /// Persist the configured directory proposals (pending; nothing is committed).
    Directory,
    /// Promote Proposed elements to Accepted through the generic governed confirmation.
    Confirm {
        /// The element IDs to confirm (at least one).
        #[arg(required = true, value_name = "ELEMENT_ID")]
        elements: Vec<String>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Materialize questions from the current F2 report with the configured question routing.
    Ask,
    /// Answer a question (a pending governed proposal).
    Answer {
        question: String,
        /// The answer as a JSON object.
        #[arg(long)]
        answer: String,
    },
    /// List every pending proposal with the information needed to act on it.
    Proposals,
    /// Append one pilot effort record.
    LogEffort {
        #[arg(long)]
        file: PathBuf,
        #[arg(long, value_enum)]
        kind: EffortKind,
        #[arg(long)]
        amount: String,
        #[arg(long)]
        note: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum RoundsCommand {
    /// Compose one round for a stakeholder.
    Create {
        #[arg(long)]
        stakeholder: String,
        /// The maximum number of questions (1..=1000; required, no default).
        #[arg(long)]
        max: u64,
    },
    /// Show one round.
    Show { id: String },
}

impl Command {
    fn recipe(&self) -> Option<(Recipe, &[String])> {
        let (name, scope) = match self {
            Command::Lint(s) => ("lint", s),
            Command::Extract(s) => ("extract", s),
            Command::Consolidate(s) => ("consolidate", s),
            Command::Calc(s) => ("calc", s),
            Command::Rules(s) => ("rules", s),
            Command::Analyse(s) => ("analyse", s),
            Command::Flows(s) => ("flows", s),
            Command::Scenarios(s) => ("scenarios", s),
            Command::Run(s) => ("run", s),
            Command::Tags(s) => ("tags", s),
            _ => return None,
        };
        Recipe::parse(name).map(|recipe| (recipe, scope.scope.as_slice()))
    }
}

/// Rejects provider option conflicts.
fn provider_choice(cli: &Cli) -> Result<ProviderChoice<'_>, CliError> {
    let usage = |m: &str| Err(CliError::Usage(m.to_owned()));
    match (
        cli.provider,
        &cli.replay_fixtures,
        &cli.anthropic_api_key_file,
    ) {
        (ProviderArg::Null, None, None) => Ok(ProviderChoice::Null),
        (ProviderArg::Replay, Some(fixtures), None) => Ok(ProviderChoice::Replay { fixtures }),
        (ProviderArg::Anthropic, None, Some(key_file)) => {
            Ok(ProviderChoice::Anthropic { key_file })
        }
        (ProviderArg::Replay, None, _) => usage("--provider replay requires --replay-fixtures"),
        (ProviderArg::Anthropic, _, None) => {
            usage("--provider anthropic requires --anthropic-api-key-file")
        }
        (_, Some(_), _) => usage("--replay-fixtures is allowed only with --provider replay"),
        (_, _, Some(_)) => {
            usage("--anthropic-api-key-file is allowed only with --provider anthropic")
        }
    }
}

fn execute(cli: Cli) -> Result<Done, CliError> {
    let choice = provider_choice(&cli)?;
    let config_path = cli
        .config
        .as_deref()
        .ok_or_else(|| CliError::Usage("--config is required".into()))?;
    let config = CliConfig::load(config_path)?;
    if let Command::LogEffort {
        file,
        kind,
        amount,
        note,
    } = &cli.command
    {
        return commands::log_effort(
            &config,
            &SystemClock,
            file,
            kind.as_str(),
            amount,
            note.as_deref(),
        );
    }
    let db = cli
        .db
        .as_deref()
        .ok_or_else(|| CliError::Usage("--db is required".into()))?;
    // init creates the database, so it is dispatched before `commands::open` and never builds
    // a provider.
    if let Command::Init { project } = &cli.command {
        return commands::init(db, &cli.branch, project, &config, &SystemClock);
    }
    // accept-set members are parsed before anything is opened or written.
    if let Command::AcceptSet { members, .. } = &cli.command {
        commands::parse_set_members(members)?;
    }
    let provider = Provider::build(choice)?;
    let state = commands::open(db, &cli.branch, &config, provider)?;
    if let Some((recipe, scope)) = cli.command.recipe() {
        return commands::stage(&state, recipe, scope.to_vec());
    }
    match cli.command {
        Command::Import {
            file,
            media_type,
            name,
        } => commands::import(&state, &file, &media_type, &name),
        Command::Vocab => commands::vocab(&state),
        Command::Questions => commands::questions(&state),
        Command::Rounds {
            command: RoundsCommand::Create { stakeholder, max },
        } => commands::rounds_create(&state, &stakeholder, max),
        Command::Rounds {
            command: RoundsCommand::Show { id },
        } => commands::rounds_show(&state, &id),
        Command::Apply {
            proposal,
            action,
            reason,
        } => commands::apply(&state, &proposal, action.action(), reason),
        Command::AcceptSet { members, reason } => commands::accept_set(&state, members, reason),
        Command::Ready => commands::ready(&state),
        Command::Export { format } => commands::export(&state, &format),
        Command::Report { out } => commands::report(&state, &out),
        Command::Serve { listen } => commands::serve(state, listen),
        Command::Directory => commands::directory(&state),
        Command::Confirm { elements, reason } => commands::confirm(&state, &elements, reason),
        Command::Ask => commands::ask(&state),
        Command::Answer { question, answer } => commands::answer(&state, &question, &answer),
        Command::Proposals => commands::proposals(&state),
        _ => Err(CliError::Usage("unsupported command".into())),
    }
}

/// Prints a result: raw bytes, JSON or text.
fn print(done: &Done, json: bool) {
    let mut out = std::io::stdout().lock();
    if let Some(raw) = &done.raw {
        let _ = out.write_all(raw);
    } else if json {
        let _ = writeln!(out, "{}", done.json);
    } else if !done.text.is_empty() {
        let _ = writeln!(out, "{}", done.text);
    }
    let _ = out.flush();
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            // Help and version are successes; every argument error is exit 1, never the gate
            // failure code 2.
            return if error.use_stderr() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    let json = cli.json;
    match execute(cli) {
        Ok(done) => {
            print(&done, json);
            if done.gate_failed {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            if let CliError::Recipe { done, .. } = &error {
                print(done, json);
            }
            if json {
                eprintln!("{}", error.json());
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::from(1)
        }
    }
}
