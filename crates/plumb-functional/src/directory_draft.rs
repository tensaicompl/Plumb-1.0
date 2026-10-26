//! The inferred semantic directory (goal-approach deviation, any-domain run).
//!
//! When a project configures no [`DirectoryDraft`], the directory material its Accepted
//! requirements name (Actors, BusinessRoles, Rules and simple DecisionTables) is drafted by
//! inference instead of being authored in configuration, and the runtime configuration supplies
//! the rest (one Calendar per runtime calendar, one Stakeholder per routing stakeholder). The
//! result is an ordinary [`DirectoryDraft`] that [`crate::produce_directory`] turns into
//! `Proposed` nodes in `HUMAN_CONFIRM` proposals: nothing is accepted here.
//!
//! - Element IDs are derived from names (`actor:`, `brole:`, `rule:`, `dt:` plus the
//!   lower-case name with every other run of characters as one `-`), never from the model.
//! - Validation is per element (the any-domain tolerance): an element with an unusable name, a
//!   duplicate ID, an ID already in the graph, no supplied grounding or an invalid decision
//!   table is dropped with an [`DirectoryDraftIssue`]; schema and binding errors still reject
//!   the whole output.
//! - Without inference only the configuration-supplied elements are drafted.
//!
//! No clock, configuration file or global is read.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId};
use plumb_inference::{
    InferenceArtifact, InferenceError, InferenceRequest, InferenceRequestMaterial, ProviderError,
    ProviderPolicy,
};
use plumb_psg::{
    Actor, ActorKind, BusinessRole, ElementStatus, Graph, NodePayload, NodeType, Rule, RuleKind,
    Stakeholder,
};
use plumb_validation::decision_table_analysis::{
    analyze_decision_table, DecisionColumn, DecisionInputCell, DecisionOutputCell, DecisionRow,
    DecisionTableSpec, DecisionType,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::decision_table::encode_decision_table;
use crate::directory::{
    DirectoryCalendarDraft, DirectoryDecisionTableDraft, DirectoryDraft, DirectoryElement,
};

/// Version of [`DirectoryDraftContext`].
pub const DIRECTORY_DRAFT_CONTEXT_VERSION: u32 = 1;

/// Version of the directory-draft inference output.
pub const DIRECTORY_DRAFT_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of directory drafting.
pub const DIRECTORY_DRAFT_TASK_KIND: &str = "directory_draft";

/// The exact prompt-template bytes; their Generic hash is the request's `prompt_template_hash`.
const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s2-directory-draft.md");

/// The exact schema source; its byte hash is the request's `schema_hash`.
const SCHEMA_SOURCE: &str =
    include_str!("../../../schemas/inference/s2-directory-draft.schema.json");

const DIRECTORY_DRAFT_STAGE: StageId = StageId::S2;

/// The time zone of a configuration-supplied Calendar (the runtime calendar names none).
pub const DEFAULT_CALENDAR_TIME_ZONE: &str = "UTC";

// ============================================================================ errors

/// Why directory drafting could not run. Malformed supplied inference is an error.
#[derive(Debug, Error)]
pub enum DirectoryDraftError {
    #[error("invalid directory-draft input: {reason}")]
    InvalidInput { reason: String },
    #[error("directory-draft schema cannot be compiled: {reason}")]
    SchemaCompilation { reason: String },
    #[error("directory-draft output is invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("invalid inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error(transparent)]
    Inference(#[from] InferenceError),
    #[error(transparent)]
    InferenceMaterial(#[from] ProviderError),
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_input(reason: impl Into<String>) -> DirectoryDraftError {
    DirectoryDraftError::InvalidInput {
        reason: reason.into(),
    }
}

fn schema_invalid(reason: impl Into<String>) -> DirectoryDraftError {
    DirectoryDraftError::SchemaInvalid {
        reason: reason.into(),
    }
}

// ============================================================================ context

/// One Accepted Requirement in the context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryDraftRequirement {
    pub requirement_ref: Id,
    pub statement: String,
    pub evidence_refs: Vec<Id>,
}

/// One directory element the graph already has (any status but Rejected or Superseded).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryDraftExisting {
    pub node_ref: Id,
    pub node_type: String,
    pub name: String,
}

/// The exact directory-draft inference context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryDraftContext {
    pub version: u32,
    pub project_id: Id,
    pub requirements: Vec<DirectoryDraftRequirement>,
    pub existing: Vec<DirectoryDraftExisting>,
}

impl DirectoryDraftContext {
    fn input_refs(&self) -> Vec<Id> {
        let refs: BTreeSet<&Id> = self
            .requirements
            .iter()
            .map(|r| &r.requirement_ref)
            .chain(self.existing.iter().map(|e| &e.node_ref))
            .collect();
        refs.into_iter().cloned().collect()
    }

    fn evidence_refs(&self) -> Vec<Id> {
        let refs: BTreeSet<&Id> = self
            .requirements
            .iter()
            .flat_map(|r| &r.evidence_refs)
            .collect();
        refs.into_iter().cloned().collect()
    }
}

/// The directory element types and the name each payload carries.
fn directory_name(payload: &NodePayload) -> Option<(NodeType, &str)> {
    match payload {
        NodePayload::Actor(a) => Some((NodeType::Actor, &a.name)),
        NodePayload::BusinessRole(r) => Some((NodeType::BusinessRole, &r.name)),
        NodePayload::Rule(r) => Some((NodeType::Rule, &r.name)),
        NodePayload::DecisionTable(t) => Some((NodeType::DecisionTable, &t.name)),
        NodePayload::Stakeholder(s) => Some((NodeType::Stakeholder, &s.name)),
        _ => None,
    }
}

fn live(status: ElementStatus) -> bool {
    !matches!(status, ElementStatus::Rejected | ElementStatus::Superseded)
}

/// The context of `graph`: every Accepted Requirement and every live directory element, both in
/// ID order. At least one Accepted Requirement is required.
pub fn directory_draft_context(
    graph: &Graph,
) -> Result<DirectoryDraftContext, DirectoryDraftError> {
    let mut requirements = Vec::new();
    let mut existing = Vec::new();
    for node in graph.nodes().values() {
        if let NodePayload::Requirement(r) = &node.payload {
            if node.status == ElementStatus::Accepted {
                let mut evidence_refs: Vec<Id> =
                    node.evidence.iter().map(|e| e.as_id().clone()).collect();
                evidence_refs.sort();
                evidence_refs.dedup();
                requirements.push(DirectoryDraftRequirement {
                    requirement_ref: node.id.clone(),
                    statement: r.statement.clone(),
                    evidence_refs,
                });
            }
        } else if let Some((node_type, name)) = directory_name(&node.payload) {
            if live(node.status) {
                existing.push(DirectoryDraftExisting {
                    node_ref: node.id.clone(),
                    node_type: node_type.as_str().to_owned(),
                    name: name.to_owned(),
                });
            }
        }
    }
    if requirements.is_empty() {
        return Err(invalid_input("the graph has no Accepted Requirement"));
    }
    requirements.sort_by(|a, b| a.requirement_ref.cmp(&b.requirement_ref));
    existing.sort_by(|a, b| a.node_ref.cmp(&b.node_ref));
    Ok(DirectoryDraftContext {
        version: DIRECTORY_DRAFT_CONTEXT_VERSION,
        project_id: graph.project_id().clone(),
        requirements,
        existing,
    })
}

// ============================================================================ request

/// A directory-draft `InferenceRequest` together with the exact context it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryDraftRequest {
    pub request: InferenceRequest,
    pub context: DirectoryDraftContext,
}

impl DirectoryDraftRequest {
    /// The exact acquisition material of this request: the RFC 8785 canonical context and the
    /// committed prompt and schema bytes, bound to the request.
    pub fn inference_material(&self) -> Result<InferenceRequestMaterial, DirectoryDraftError> {
        Ok(InferenceRequestMaterial::new(
            &self.request,
            to_canonical_json(&self.context)?,
            PROMPT_TEMPLATE.to_vec(),
            SCHEMA_SOURCE.as_bytes().to_vec(),
        )?)
    }
}

/// Builds the S2 `directory_draft` request over the context of `graph`.
pub fn build_directory_draft_request(
    graph: &Graph,
    provider_policy: ProviderPolicy,
) -> Result<DirectoryDraftRequest, DirectoryDraftError> {
    let context = directory_draft_context(graph)?;
    let request = InferenceRequest::new(
        DIRECTORY_DRAFT_STAGE,
        DIRECTORY_DRAFT_TASK_KIND.to_owned(),
        context.input_refs(),
        context.evidence_refs(),
        Hash::content_sha256(&to_canonical_json(&context)?),
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    Ok(DirectoryDraftRequest { request, context })
}

// ============================================================================ output

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftOutput {
    version: u32,
    actors: Vec<ActorOutput>,
    business_roles: Vec<RoleOutput>,
    rules: Vec<RuleOutput>,
    decision_tables: Vec<TableOutput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorOutput {
    name: String,
    actor_kind: ActorKind,
    grounding_refs: Vec<Id>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleOutput {
    name: String,
    grounding_refs: Vec<Id>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleOutput {
    name: String,
    rule_kind: RuleKind,
    grounding_refs: Vec<Id>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableOutput {
    name: String,
    grounding_refs: Vec<Id>,
    hit_policy: String,
    inputs: Vec<ColumnOutput>,
    outputs: Vec<ColumnOutput>,
    rows: Vec<RowOutput>,
}

/// A column as the flat schema carries it (one object shape keeps the API grammar small).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnOutput {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    members: Vec<String>,
    scale: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowOutput {
    inputs: Vec<InputCellOutput>,
    outputs: Vec<OutputCellOutput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputCellOutput {
    cell: String,
    value: Option<bool>,
    members: Vec<String>,
}

/// An output cell: the value is text in the column's type.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputCellOutput {
    cell: String,
    value: String,
}

fn compile_schema() -> Result<JSONSchema, DirectoryDraftError> {
    let schema: Value = serde_json::from_str(SCHEMA_SOURCE).map_err(|e| {
        DirectoryDraftError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        }
    })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| DirectoryDraftError::SchemaCompilation {
            reason: e.to_string(),
        })
}

// ============================================================================ drafting

/// A configured stakeholder the draft carries (question routing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryDraftStakeholder {
    pub id: Id,
    pub name: String,
    pub stakeholder_kind: String,
}

/// The configuration-supplied part of the draft.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirectoryDraftSeed {
    /// The runtime calendar refs, each drafted as one Calendar.
    pub calendar_refs: Vec<Id>,
    pub stakeholders: Vec<DirectoryDraftStakeholder>,
}

/// Why one drafted element was left out.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryDraftIssue {
    pub family: String,
    pub name: String,
    pub reason: String,
}

/// The drafted directory and the elements left out.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryDraftResult {
    pub draft: DirectoryDraft,
    pub issues: Vec<DirectoryDraftIssue>,
}

/// The ID local part of a name: lower-case ASCII letters and digits, every other run of
/// characters one `-`, no leading or trailing `-`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && !name.chars().any(char::is_control)).then(|| name.to_owned())
}

struct Drafting<'a> {
    graph: &'a Graph,
    known: BTreeSet<&'a Id>,
    taken: BTreeSet<Id>,
    issues: Vec<DirectoryDraftIssue>,
}

impl Drafting<'_> {
    fn drop(&mut self, family: &str, name: &str, reason: impl Into<String>) {
        self.issues.push(DirectoryDraftIssue {
            family: family.to_owned(),
            name: name.to_owned(),
            reason: reason.into(),
        });
    }

    /// The derived ID and the usable grounding of one element, or `None` (with an issue).
    fn admit(
        &mut self,
        family: &str,
        prefix: &str,
        name: &str,
        grounding: &[Id],
    ) -> Option<(Id, String, Vec<Id>)> {
        let Some(clean) = clean_name(name) else {
            self.drop(
                family,
                name,
                "the name is empty or contains a control character",
            );
            return None;
        };
        let local = slug(&clean);
        let Ok(id) = format!("{prefix}:{local}").parse::<Id>() else {
            self.drop(family, &clean, "no element ID can be derived from the name");
            return None;
        };
        if self.graph.node(&id).is_some() {
            self.drop(family, &clean, format!("{id} is already in the model"));
            return None;
        }
        if !self.taken.insert(id.clone()) {
            self.drop(family, &clean, format!("{id} is drafted twice"));
            return None;
        }
        let refs: BTreeSet<Id> = grounding
            .iter()
            .filter(|r| self.known.contains(r))
            .cloned()
            .collect();
        if refs.is_empty() {
            self.drop(family, &clean, "it names no supplied requirement");
            return None;
        }
        Some((id, clean, refs.into_iter().collect()))
    }
}

/// Enum members are a set: sorted and unique in the typed spec.
fn member_set(members: &[String]) -> Vec<String> {
    let set: BTreeSet<&String> = members.iter().collect();
    set.into_iter().cloned().collect()
}

/// The typed column of a flat one: bool and enum inputs, any of the four output types.
fn typed_column(c: &ColumnOutput, input: bool) -> Result<DecisionColumn, String> {
    let ty = match (c.ty.as_str(), input) {
        ("bool", _) => DecisionType::Bool,
        ("enum", _) if !c.members.is_empty() => DecisionType::Enum {
            members: member_set(&c.members),
        },
        ("int", false) => DecisionType::Int,
        ("decimal", false) => DecisionType::Decimal {
            scale: c
                .scale
                .filter(|s| *s <= 28)
                .ok_or_else(|| format!("column {} has no decimal scale", c.name))?,
        },
        _ => return Err(format!("column {} has an unsupported type", c.name)),
    };
    Ok(DecisionColumn {
        name: c.name.clone(),
        ty,
        domain: None,
    })
}

fn input_cell(cell: &InputCellOutput) -> Result<DecisionInputCell, String> {
    match (cell.cell.as_str(), cell.value) {
        ("any", _) => Ok(DecisionInputCell::Any),
        ("bool", Some(value)) => Ok(DecisionInputCell::Bool { value }),
        ("enum_set", _) => Ok(DecisionInputCell::EnumSet {
            members: member_set(&cell.members),
        }),
        _ => Err(format!("input cell {} has no value", cell.cell)),
    }
}

fn output_cell(cell: &OutputCellOutput) -> Result<DecisionOutputCell, String> {
    let text = cell.value.trim();
    let bad = || format!("{text} is not a {} value", cell.cell);
    Ok(match cell.cell.as_str() {
        "bool" => DecisionOutputCell::Bool {
            value: text.parse().map_err(|_| bad())?,
        },
        "enum" => DecisionOutputCell::Enum {
            member: text.to_owned(),
        },
        "int" => DecisionOutputCell::Int {
            value: text.parse().map_err(|_| bad())?,
        },
        "decimal" => DecisionOutputCell::Decimal {
            value: text.parse::<Decimal>().map_err(|_| bad())?,
        },
        _ => return Err(bad()),
    })
}

/// The typed spec of one drafted table, or why it is unusable.
fn table_spec(table: &TableOutput) -> Result<DecisionTableSpec, String> {
    let mut rows = Vec::with_capacity(table.rows.len());
    for (index, row) in table.rows.iter().enumerate() {
        let at = |e: String| format!("row {index}: {e}");
        rows.push(DecisionRow {
            inputs: row
                .inputs
                .iter()
                .map(input_cell)
                .collect::<Result<_, _>>()
                .map_err(at)?,
            outputs: row
                .outputs
                .iter()
                .map(output_cell)
                .collect::<Result<_, _>>()
                .map_err(at)?,
        });
    }
    if table.inputs.is_empty() || table.outputs.is_empty() || rows.is_empty() {
        return Err("a table needs input and output columns and rows".to_owned());
    }
    let spec = DecisionTableSpec {
        hit_policy: table.hit_policy.clone(),
        inputs: table
            .inputs
            .iter()
            .map(|c| typed_column(c, true))
            .collect::<Result<_, _>>()?,
        outputs: table
            .outputs
            .iter()
            .map(|c| typed_column(c, false))
            .collect::<Result<_, _>>()?,
        rows,
        default_output: None,
    };
    let analysis = analyze_decision_table(&spec);
    if !analysis.structure.is_empty() {
        return Err(format!("the table is malformed: {:?}", analysis.structure));
    }
    if !analysis.illegal_overlaps.is_empty() {
        return Err("rows overlap under hit policy UNIQUE".to_owned());
    }
    encode_decision_table(&table.name, &spec).map_err(|e| e.to_string())?;
    Ok(spec)
}

/// The configuration-supplied directory of `graph`: one Calendar per runtime calendar and one
/// Stakeholder per routing stakeholder, each unless the graph already holds it. No inference.
pub fn seed_directory(graph: &Graph, seed: &DirectoryDraftSeed) -> DirectoryDraft {
    let mut draft = empty_draft();
    let mut taken = BTreeSet::new();
    let calendar_covered: BTreeSet<&str> = graph
        .nodes()
        .values()
        .filter(|n| live(n.status) && matches!(n.payload, NodePayload::Calendar(_)))
        .map(|n| n.id.as_str())
        .collect();
    for calendar_ref in &seed.calendar_refs {
        let local = calendar_ref
            .as_str()
            .split_once(':')
            .map_or(calendar_ref.as_str(), |(_, local)| local);
        let Ok(id) = format!("calendar:{local}").parse::<Id>() else {
            continue;
        };
        if calendar_covered.contains(id.as_str()) || !taken.insert(id.clone()) {
            continue;
        }
        draft.calendars.push(DirectoryCalendarDraft {
            id,
            calendar_ref: calendar_ref.clone(),
            time_zone: DEFAULT_CALENDAR_TIME_ZONE.to_owned(),
            region: None,
            holiday_source: None,
            grounding_refs: Vec::new(),
        });
    }
    for stakeholder in &seed.stakeholders {
        if graph.node(&stakeholder.id).is_some() || !taken.insert(stakeholder.id.clone()) {
            continue;
        }
        draft.stakeholders.push(DirectoryElement {
            id: stakeholder.id.clone(),
            payload: Stakeholder {
                name: stakeholder.name.clone(),
                stakeholder_kind: stakeholder.stakeholder_kind.clone(),
                organization: None,
                responsibilities: None,
                contact_ref: None,
            },
            grounding_refs: Vec::new(),
        });
    }
    draft
}

fn empty_draft() -> DirectoryDraft {
    DirectoryDraft {
        stakeholders: Vec::new(),
        actors: Vec::new(),
        business_roles: Vec::new(),
        calendars: Vec::new(),
        rules: Vec::new(),
        decision_tables: Vec::new(),
        event_dispositions: Vec::new(),
        state_dispositions: Vec::new(),
        outcome_selections: Vec::new(),
    }
}

/// Drafts the inferred directory of `request`'s graph: the validated Actors, BusinessRoles,
/// Rules and DecisionTables of `artifact` (nothing without it). Issues are sorted. The
/// configuration-supplied elements are [`seed_directory`]'s.
pub fn draft_directory(
    graph: &Graph,
    request: &DirectoryDraftRequest,
    artifact: Option<&InferenceArtifact>,
) -> Result<DirectoryDraftResult, DirectoryDraftError> {
    if directory_draft_context(graph)? != request.context {
        return Err(invalid_input(
            "graph does not match the directory-draft request context",
        ));
    }
    let mut draft = empty_draft();
    let mut drafting = Drafting {
        graph,
        known: request
            .context
            .requirements
            .iter()
            .map(|r| &r.requirement_ref)
            .collect(),
        taken: BTreeSet::new(),
        issues: Vec::new(),
    };
    if let Some(artifact) = artifact {
        let output = validated_output(request, artifact)?;
        let existing: BTreeSet<(String, String)> = request
            .context
            .existing
            .iter()
            .map(|e| (e.node_type.clone(), e.name.to_lowercase()))
            .collect();
        let repeats = |node_type: NodeType, name: &str| {
            existing.contains(&(node_type.as_str().to_owned(), name.trim().to_lowercase()))
        };
        for a in &output.actors {
            if repeats(NodeType::Actor, &a.name) {
                drafting.drop("actors", &a.name, "an Actor with this name exists");
            } else if let Some((id, name, grounding_refs)) =
                drafting.admit("actors", "actor", &a.name, &a.grounding_refs)
            {
                draft.actors.push(DirectoryElement {
                    id,
                    payload: Actor {
                        name,
                        actor_kind: a.actor_kind,
                    },
                    grounding_refs,
                });
            }
        }
        for r in &output.business_roles {
            if repeats(NodeType::BusinessRole, &r.name) {
                drafting.drop(
                    "business_roles",
                    &r.name,
                    "a BusinessRole with this name exists",
                );
            } else if let Some((id, name, grounding_refs)) =
                drafting.admit("business_roles", "brole", &r.name, &r.grounding_refs)
            {
                draft.business_roles.push(DirectoryElement {
                    id,
                    payload: BusinessRole { name },
                    grounding_refs,
                });
            }
        }
        for r in &output.rules {
            if repeats(NodeType::Rule, &r.name) {
                drafting.drop("rules", &r.name, "a Rule with this name exists");
            } else if let Some((id, name, grounding_refs)) =
                drafting.admit("rules", "rule", &r.name, &r.grounding_refs)
            {
                draft.rules.push(DirectoryElement {
                    id,
                    payload: Rule {
                        name,
                        rule_kind: r.rule_kind,
                        condition_expr: None,
                        action_expr: None,
                    },
                    grounding_refs,
                });
            }
        }
        for t in &output.decision_tables {
            if repeats(NodeType::DecisionTable, &t.name) {
                drafting.drop(
                    "decision_tables",
                    &t.name,
                    "a DecisionTable with this name exists",
                );
                continue;
            }
            let spec = match table_spec(t) {
                Ok(spec) => spec,
                Err(reason) => {
                    drafting.drop("decision_tables", &t.name, reason);
                    continue;
                }
            };
            if let Some((id, name, grounding_refs)) =
                drafting.admit("decision_tables", "dt", &t.name, &t.grounding_refs)
            {
                draft.decision_tables.push(DirectoryDecisionTableDraft {
                    id,
                    name,
                    spec,
                    grounding_refs,
                });
            }
        }
    }
    let mut issues = drafting.issues;
    issues.sort();
    Ok(DirectoryDraftResult { draft, issues })
}

/// Validates a supplied artifact against the request, the schema and the output version.
fn validated_output(
    request: &DirectoryDraftRequest,
    artifact: &InferenceArtifact,
) -> Result<DraftOutput, DirectoryDraftError> {
    artifact.validate_for(&request.request).map_err(|e| {
        DirectoryDraftError::InvalidInferenceArtifact {
            reason: e.to_string(),
        }
    })?;
    let output = artifact.validated_output.as_value();
    if let Err(errors) = compile_schema()?.validate(output) {
        let mut messages: Vec<String> = errors
            .map(|e| format!("{e} at {}", e.instance_path))
            .collect();
        messages.sort();
        return Err(schema_invalid(messages.join("; ")));
    }
    let decoded: DraftOutput =
        serde_json::from_value(output.clone()).map_err(|e| schema_invalid(e.to_string()))?;
    if decoded.version != DIRECTORY_DRAFT_OUTPUT_VERSION {
        return Err(schema_invalid(format!(
            "output version {} is not 1",
            decoded.version
        )));
    }
    Ok(decoded)
}

/// The names of drafted elements by family, for reports.
pub fn drafted_names(draft: &DirectoryDraft) -> BTreeMap<&'static str, Vec<String>> {
    let mut names = BTreeMap::new();
    names.insert(
        "actors",
        draft
            .actors
            .iter()
            .map(|e| e.payload.name.clone())
            .collect(),
    );
    names.insert(
        "business_roles",
        draft
            .business_roles
            .iter()
            .map(|e| e.payload.name.clone())
            .collect(),
    );
    names.insert(
        "rules",
        draft.rules.iter().map(|e| e.payload.name.clone()).collect(),
    );
    names.insert(
        "decision_tables",
        draft
            .decision_tables
            .iter()
            .map(|t| t.name.clone())
            .collect(),
    );
    names
}
