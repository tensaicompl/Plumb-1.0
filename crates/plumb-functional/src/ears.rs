//! Grounded EARS normalization (plan S1.2; compiler architecture §7).
//!
//! The model proposes only an EARS pattern and grounding references into the original source
//! statement or into exact Accepted semantic strings; Plumb renders the normalized statement
//! with fixed connectives and the Requirement's existing modality, and wraps it in one
//! Semantic / HUMAN_CONFIRM `ReplacePayload` Proposal per changed Requirement. The original
//! wording is recovered from evidence and `plumb_functional:segment_origin`, never stored twice.
//! Nothing here mutates a graph, calls a provider, reads a clock or persists anything.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_patch::{
    AcceptancePolicy, ElementPrecondition, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    contributes_to_semantic_hash, node_element_hash, DerivationRef, ElementStatus, ExtensionKey,
    Graph, Modality, Node, NodePayload, Requirement, RequirementKind, RequirementLevel,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::requirements::{parse_statement, SegmentOrigin, SEGMENT_ORIGIN_EXTENSION};

/// Version of [`EarsContext`].
pub const EARS_CONTEXT_VERSION: u32 = 1;

/// Version of the EARS inference output.
pub const EARS_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of EARS normalization.
pub const EARS_TASK_KIND: &str = "ears_normalization";

/// The exact prompt-template bytes; their Generic hash is the request's `prompt_template_hash`.
const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s1-ears.md");

/// The exact schema source; its byte hash is the request's `schema_hash`.
const SCHEMA_SOURCE: &str = include_str!("../../../schemas/inference/s1-ears.schema.json");

const EARS_STAGE: StageId = StageId::S1;

// ============================================================================ errors

/// Why EARS normalization could not run. Malformed supplied inference is an error, never
/// treated as absent inference.
#[derive(Debug, Error)]
pub enum EarsError {
    /// The targets, graph or EARS request are not valid input.
    #[error("invalid EARS input: {reason}")]
    InvalidInput { reason: String },
    /// The InferenceRequest could not be constructed.
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    /// The inference artifact is not valid for the EARS request.
    #[error("invalid EARS inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    /// The EARS schema does not compile.
    #[error("EARS schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    /// The output does not satisfy the schema or does not decode.
    #[error("EARS output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    /// A target Requirement has no output entry.
    #[error("requirement {requirement_ref} has no EARS output entry")]
    OutputCoverage { requirement_ref: Id },
    /// An output entry names a Requirement outside the request.
    #[error("EARS output for unknown requirement {requirement_ref}")]
    UnknownRequirement { requirement_ref: Id },
    /// A Requirement has more than one output entry.
    #[error("requirement {requirement_ref} has more than one EARS output entry")]
    DuplicateRequirementOutput { requirement_ref: Id },
    /// The trigger/condition presence does not match the pattern.
    #[error("invalid {pattern} suggestion for {requirement_ref}: {reason}")]
    InvalidPattern {
        requirement_ref: Id,
        pattern: &'static str,
        reason: &'static str,
    },
    /// A grounding does not resolve exactly to source or Accepted semantic text.
    #[error("invalid {component} grounding for {requirement_ref}: {reason}")]
    InvalidGrounding {
        requirement_ref: Id,
        component: &'static str,
        reason: String,
    },
    /// A resolved component cannot be rendered as-is.
    #[error("invalid {component} component for {requirement_ref}: {reason}")]
    InvalidComponent {
        requirement_ref: Id,
        component: &'static str,
        reason: &'static str,
    },
    /// The proposal could not be built.
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    /// Canonicalization or hashing failed.
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_input(reason: impl Into<String>) -> EarsError {
    EarsError::InvalidInput {
        reason: reason.into(),
    }
}

fn schema_invalid(reason: impl Into<String>) -> EarsError {
    EarsError::SchemaInvalid {
        reason: reason.into(),
    }
}

// ============================================================================ vocabulary

/// The supported pilot EARS patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EarsPattern {
    Ubiquitous,
    EventDriven,
    StateDriven,
    OptionalFeature,
    UnwantedBehavior,
}

impl EarsPattern {
    /// Every value, in contract order.
    pub const ALL: [EarsPattern; 5] = [
        EarsPattern::Ubiquitous,
        EarsPattern::EventDriven,
        EarsPattern::StateDriven,
        EarsPattern::OptionalFeature,
        EarsPattern::UnwantedBehavior,
    ];

    /// The exact wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            EarsPattern::Ubiquitous => "ubiquitous",
            EarsPattern::EventDriven => "event_driven",
            EarsPattern::StateDriven => "state_driven",
            EarsPattern::OptionalFeature => "optional_feature",
            EarsPattern::UnwantedBehavior => "unwanted_behavior",
        }
    }
}

/// Where one EARS component's exact text comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EarsGrounding {
    /// Zero-based, end-exclusive UTF-8 byte range of the requirement's `source_statement`.
    SourceRange { start: u64, end: u64 },
    /// An exact string value of one Accepted semantic catalog entry.
    AcceptedSemantic { semantic_ref: Id, text: String },
}

// ============================================================================ context and request

/// One Accepted semantic node whose exact string values may ground EARS components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EarsAcceptedSemantic {
    pub semantic_ref: Id,
    pub values: Vec<String>,
}

/// One target Requirement: its current statement and its original source statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EarsRequirementContext {
    pub requirement_ref: Id,
    pub status: ElementStatus,
    pub current_statement: String,
    pub source_statement: String,
    pub requirement_kind: RequirementKind,
    pub level: RequirementLevel,
    pub modality: Modality,
    pub source_identifier: Option<String>,
    pub evidence_refs: Vec<Id>,
    pub segment_origin: SegmentOrigin,
}

/// The exact EARS inference context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EarsContext {
    pub version: u32,
    pub project_id: Id,
    pub requirements: Vec<EarsRequirementContext>,
    pub accepted_semantics: Vec<EarsAcceptedSemantic>,
}

fn strictly_sorted<T: Ord>(items: impl Iterator<Item = T>) -> bool {
    let items: Vec<T> = items.collect();
    items.windows(2).all(|pair| pair[0] < pair[1])
}

impl EarsContext {
    /// Version 1, at least one requirement, strictly sorted requirements, catalog entries and
    /// values, non-empty catalog values.
    pub fn validate(&self) -> Result<(), EarsError> {
        if self.version != EARS_CONTEXT_VERSION {
            return Err(invalid_input(format!(
                "context version {} is not 1",
                self.version
            )));
        }
        if self.requirements.is_empty() {
            return Err(invalid_input("context has no target requirements"));
        }
        if !strictly_sorted(self.requirements.iter().map(|r| &r.requirement_ref)) {
            return Err(invalid_input(
                "context requirements are not strictly sorted",
            ));
        }
        if self
            .requirements
            .iter()
            .any(|r| !strictly_sorted(r.evidence_refs.iter()) || r.source_statement.is_empty())
        {
            return Err(invalid_input(
                "context requirement evidence is unsorted or source statement is empty",
            ));
        }
        if !strictly_sorted(self.accepted_semantics.iter().map(|s| &s.semantic_ref)) {
            return Err(invalid_input(
                "context accepted semantics are not strictly sorted",
            ));
        }
        if self
            .accepted_semantics
            .iter()
            .any(|s| s.values.is_empty() || !strictly_sorted(s.values.iter()))
        {
            return Err(invalid_input(
                "accepted semantic values are empty or not strictly sorted",
            ));
        }
        Ok(())
    }

    /// Generic SHA-256 of the RFC 8785 canonical JSON of the validated context.
    pub fn content_hash(&self) -> Result<Hash, EarsError> {
        self.validate()?;
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn input_refs(&self) -> Vec<Id> {
        let refs: BTreeSet<&Id> = self
            .requirements
            .iter()
            .map(|r| &r.requirement_ref)
            .chain(self.accepted_semantics.iter().map(|s| &s.semantic_ref))
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

/// An EARS `InferenceRequest` together with the exact context it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EarsRequest {
    pub request: InferenceRequest,
    pub context: EarsContext,
}

impl EarsRequest {
    /// Validates both parts and their binding.
    pub fn new(request: InferenceRequest, context: EarsContext) -> Result<EarsRequest, EarsError> {
        let ears = EarsRequest { request, context };
        ears.validate()?;
        Ok(ears)
    }

    /// The request is a valid S1 ears_normalization request bound to this context and to the
    /// committed prompt and schema.
    pub fn validate(&self) -> Result<(), EarsError> {
        let request = &self.request;
        request
            .validate()
            .map_err(|e| invalid_input(format!("invalid inference request: {e}")))?;
        let context_hash = self.context.content_hash()?;
        let mismatch = if request.stage != EARS_STAGE {
            Some("stage is not S1")
        } else if request.task_kind != EARS_TASK_KIND {
            Some("task_kind is not ears_normalization")
        } else if request.input_refs != self.context.input_refs() {
            Some("input_refs differ from the context requirements and accepted semantics")
        } else if request.evidence_refs != self.context.evidence_refs() {
            Some("evidence_refs differ from the target requirement evidence")
        } else if request.context_hash != context_hash {
            Some("context_hash differs from the context hash")
        } else if request.prompt_template_hash != Hash::content_sha256(PROMPT_TEMPLATE) {
            Some("prompt_template_hash differs from the EARS prompt")
        } else if request.schema_hash != Hash::content_sha256(SCHEMA_SOURCE.as_bytes()) {
            Some("schema_hash differs from the EARS schema")
        } else {
            None
        };
        match mismatch {
            Some(reason) => Err(invalid_input(reason)),
            None => Ok(()),
        }
    }
}

/// The clean original statement of an S1.1-origin Requirement, recovered from its evidence
/// and segment origin through the S1.1 source-prefix parser.
fn recover_source_statement(
    graph: &Graph,
    node: &Node,
) -> Result<(SegmentOrigin, String), EarsError> {
    let reason = |what: &str| invalid_input(format!("requirement {}: {what}", node.id));
    let key: ExtensionKey = SEGMENT_ORIGIN_EXTENSION
        .parse()
        .map_err(|e: plumb_psg::InvalidExtensionKey| reason(&e.to_string()))?;
    let value = node
        .extensions
        .get(&key)
        .ok_or_else(|| reason("missing segment origin"))?;
    let origin: SegmentOrigin = serde_json::from_value(value.clone())
        .map_err(|e| reason(&format!("malformed segment origin: {e}")))?;
    if !node
        .evidence
        .iter()
        .any(|r| r.as_id() == &origin.fragment_ref)
    {
        return Err(reason(
            "segment origin fragment is not in the node evidence",
        ));
    }
    let fragment = match graph.node(&origin.fragment_ref).map(|n| &n.payload) {
        Some(NodePayload::EvidenceFragment(fragment)) => fragment,
        Some(_) => return Err(reason("segment origin fragment is not an EvidenceFragment")),
        None => return Err(reason("segment origin fragment does not resolve")),
    };
    let text = fragment
        .extracted_text
        .as_deref()
        .ok_or_else(|| reason("segment origin fragment has no extracted_text"))?;
    let (Ok(start), Ok(end)) = (usize::try_from(origin.start), usize::try_from(origin.end)) else {
        return Err(reason("segment origin range exceeds the fragment text"));
    };
    if start >= end
        || end > text.len()
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return Err(reason("segment origin range is not a valid UTF-8 range"));
    }
    let statement = parse_statement(&text[start..end]).statement;
    if statement.is_empty() {
        return Err(reason("recovered source statement is empty"));
    }
    Ok((origin, statement))
}

fn target_context(
    graph: &Graph,
    requirement_ref: &Id,
) -> Result<EarsRequirementContext, EarsError> {
    let node = graph
        .node(requirement_ref)
        .ok_or_else(|| invalid_input(format!("requirement {requirement_ref} does not exist")))?;
    let NodePayload::Requirement(requirement) = &node.payload else {
        return Err(invalid_input(format!(
            "{requirement_ref} is not a Requirement"
        )));
    };
    if !matches!(
        node.status,
        ElementStatus::Proposed | ElementStatus::Accepted
    ) {
        return Err(invalid_input(format!(
            "requirement {requirement_ref} status {:?} is not Proposed or Accepted",
            node.status
        )));
    }
    let (segment_origin, source_statement) = recover_source_statement(graph, node)?;
    let evidence_refs: BTreeSet<&Id> = node.evidence.iter().map(|r| r.as_id()).collect();
    Ok(EarsRequirementContext {
        requirement_ref: requirement_ref.clone(),
        status: node.status,
        current_statement: requirement.statement.clone(),
        source_statement,
        requirement_kind: requirement.requirement_kind,
        level: requirement.level,
        modality: requirement.modality,
        source_identifier: requirement.source_identifier.clone(),
        evidence_refs: evidence_refs.into_iter().cloned().collect(),
        segment_origin,
    })
}

/// Every non-empty string scalar in value positions of `value`.
fn collect_strings(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(s) if !s.is_empty() => {
            out.insert(s.clone());
        }
        Value::Array(items) => items.iter().for_each(|v| collect_strings(v, out)),
        Value::Object(map) => map.values().for_each(|v| collect_strings(v, out)),
        _ => {}
    }
}

/// The Accepted, semantic-hash-contributing nodes other than the targets, with their exact
/// sorted unique string values.
fn accepted_semantics(
    graph: &Graph,
    targets: &BTreeSet<&Id>,
) -> Result<Vec<EarsAcceptedSemantic>, EarsError> {
    let mut catalog = Vec::new();
    for (id, node) in graph.nodes() {
        if node.status != ElementStatus::Accepted
            || !contributes_to_semantic_hash(node.payload.node_type())
            || targets.contains(id)
        {
            continue;
        }
        let payload = serde_json::to_value(&node.payload)
            .map_err(|e| invalid_input(format!("payload of {id} is not JSON: {e}")))?;
        let mut values = BTreeSet::new();
        collect_strings(&payload, &mut values);
        if !values.is_empty() {
            catalog.push(EarsAcceptedSemantic {
                semantic_ref: id.clone(),
                values: values.into_iter().collect(),
            });
        }
    }
    Ok(catalog)
}

fn context_of(graph: &Graph, requirement_refs: &[Id]) -> Result<EarsContext, EarsError> {
    let mut targets = BTreeSet::new();
    for requirement_ref in requirement_refs {
        if !targets.insert(requirement_ref) {
            return Err(invalid_input(format!(
                "duplicate target requirement {requirement_ref}"
            )));
        }
    }
    let requirements = targets
        .iter()
        .map(|id| target_context(graph, id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EarsContext {
        version: EARS_CONTEXT_VERSION,
        project_id: graph.project_id().clone(),
        requirements,
        accepted_semantics: accepted_semantics(graph, &targets)?,
    })
}

/// Builds the S1 EARS request for the target Requirements read from `graph`. The provider
/// policy is always the caller's; no provider is preferred.
pub fn build_ears_request(
    graph: &Graph,
    requirement_refs: &[Id],
    provider_policy: ProviderPolicy,
) -> Result<EarsRequest, EarsError> {
    let context = context_of(graph, requirement_refs)?;
    let request = InferenceRequest::new(
        EARS_STAGE,
        EARS_TASK_KIND.to_owned(),
        context.input_refs(),
        context.evidence_refs(),
        context.content_hash()?,
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    EarsRequest::new(request, context)
}

// ============================================================================ inference output

/// An already-acquired EARS artifact and the caller's provenance ref for it.
#[derive(Debug, Clone)]
pub struct EarsInference<'a> {
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EarsOutput {
    version: u32,
    requirements: Vec<EarsOutputEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EarsOutputEntry {
    requirement_ref: Id,
    suggestion: Option<EarsSuggestion>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EarsSuggestion {
    pattern: EarsPattern,
    actor: EarsGrounding,
    trigger: Option<EarsGrounding>,
    condition: Option<EarsGrounding>,
    response: EarsGrounding,
}

fn compile_schema() -> Result<JSONSchema, EarsError> {
    let schema: Value =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| EarsError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| EarsError::SchemaCompilation {
            reason: e.to_string(),
        })
}

/// Validates a supplied artifact up to output coverage; returns suggestions keyed by target.
fn validated_suggestions(
    request: &EarsRequest,
    artifact: &InferenceArtifact,
) -> Result<BTreeMap<Id, Option<EarsSuggestion>>, EarsError> {
    artifact
        .validate_for(&request.request)
        .map_err(|e| EarsError::InvalidInferenceArtifact {
            reason: e.to_string(),
        })?;
    let output = artifact.validated_output.as_value();
    if let Err(errors) = compile_schema()?.validate(output) {
        let mut messages: Vec<String> = errors
            .map(|e| format!("{e} at {}", e.instance_path))
            .collect();
        messages.sort();
        return Err(schema_invalid(messages.join("; ")));
    }
    let decoded: EarsOutput =
        serde_json::from_value(output.clone()).map_err(|e| schema_invalid(e.to_string()))?;
    if decoded.version != EARS_OUTPUT_VERSION {
        return Err(schema_invalid(format!(
            "output version {} is not 1",
            decoded.version
        )));
    }
    let known: BTreeSet<&Id> = request
        .context
        .requirements
        .iter()
        .map(|r| &r.requirement_ref)
        .collect();
    let mut entries = decoded.requirements;
    entries.sort_by(|a, b| a.requirement_ref.cmp(&b.requirement_ref));
    if let Some(unknown) = entries.iter().find(|e| !known.contains(&e.requirement_ref)) {
        return Err(EarsError::UnknownRequirement {
            requirement_ref: unknown.requirement_ref.clone(),
        });
    }
    let mut suggestions = BTreeMap::new();
    for entry in entries {
        if suggestions.contains_key(&entry.requirement_ref) {
            return Err(EarsError::DuplicateRequirementOutput {
                requirement_ref: entry.requirement_ref,
            });
        }
        suggestions.insert(entry.requirement_ref, entry.suggestion);
    }
    if let Some(missing) = known.iter().find(|id| !suggestions.contains_key(**id)) {
        return Err(EarsError::OutputCoverage {
            requirement_ref: (*missing).clone(),
        });
    }
    Ok(suggestions)
}

// ============================================================================ grounding and rendering

const PUNCTUATION: [char; 6] = [',', ';', ':', '.', '?', '!'];
const TERMINATORS: [char; 3] = ['.', '?', '!'];

/// The exact text a grounding names, without any transformation.
fn resolve_grounding(
    requirement: &EarsRequirementContext,
    catalog: &[EarsAcceptedSemantic],
    component: &'static str,
    grounding: &EarsGrounding,
) -> Result<String, EarsError> {
    let invalid = |reason: String| EarsError::InvalidGrounding {
        requirement_ref: requirement.requirement_ref.clone(),
        component,
        reason,
    };
    match grounding {
        EarsGrounding::SourceRange { start, end } => {
            let source = &requirement.source_statement;
            let (Ok(s), Ok(e)) = (usize::try_from(*start), usize::try_from(*end)) else {
                return Err(invalid(format!("range {start}..{end} exceeds the source")));
            };
            if s >= e
                || e > source.len()
                || !source.is_char_boundary(s)
                || !source.is_char_boundary(e)
            {
                return Err(invalid(format!(
                    "range {start}..{end} is not a valid UTF-8 range of the source statement"
                )));
            }
            Ok(source[s..e].to_owned())
        }
        EarsGrounding::AcceptedSemantic { semantic_ref, text } => {
            let entry = catalog
                .binary_search_by(|s| s.semantic_ref.cmp(semantic_ref))
                .map(|i| &catalog[i])
                .map_err(|_| {
                    invalid(format!(
                        "{semantic_ref} is not an Accepted semantic in context"
                    ))
                })?;
            if entry.values.binary_search(text).is_err() {
                return Err(invalid(format!(
                    "text is not an exact value of {semantic_ref}"
                )));
            }
            Ok(text.clone())
        }
    }
}

/// Clean text: non-empty, no outer ASCII whitespace, no ASCII control characters, and the
/// component's punctuation boundaries.
fn check_component(
    requirement_ref: &Id,
    component: &'static str,
    text: &str,
) -> Result<(), EarsError> {
    let invalid = |reason: &'static str| EarsError::InvalidComponent {
        requirement_ref: requirement_ref.clone(),
        component,
        reason,
    };
    if text.is_empty() {
        return Err(invalid("component is empty"));
    }
    if text.starts_with(|c: char| c.is_ascii_whitespace())
        || text.ends_with(|c: char| c.is_ascii_whitespace())
    {
        return Err(invalid("component has leading or trailing whitespace"));
    }
    if text.chars().any(|c| c.is_ascii_control()) {
        return Err(invalid("component contains a control character"));
    }
    let (no_leading, no_trailing): (&[char], &[char]) = match component {
        "actor" => (&[], &PUNCTUATION),
        "trigger" | "condition" => (&PUNCTUATION, &PUNCTUATION),
        _ => (&[], &TERMINATORS),
    };
    if text.starts_with(no_leading) || text.ends_with(no_trailing) {
        return Err(invalid(
            "component begins or ends with renderer punctuation",
        ));
    }
    Ok(())
}

fn modal_text(modality: Modality) -> &'static str {
    match modality {
        Modality::Shall => "shall",
        Modality::ShallNot => "shall not",
        Modality::Should => "should",
        Modality::May => "may",
    }
}

/// The one deterministic EARS renderer.
fn render(
    pattern: EarsPattern,
    actor: &str,
    lead: Option<&str>,
    modality: Modality,
    response: &str,
) -> String {
    let main = format!("{actor} {} {response}.", modal_text(modality));
    match (pattern, lead) {
        (EarsPattern::EventDriven, Some(trigger)) => format!("When {trigger}, {main}"),
        (EarsPattern::StateDriven, Some(condition)) => format!("While {condition}, {main}"),
        (EarsPattern::OptionalFeature, Some(condition)) => format!("Where {condition}, {main}"),
        (EarsPattern::UnwantedBehavior, Some(condition)) => {
            format!("If {condition}, then {main}")
        }
        _ => main,
    }
}

/// The trigger or condition a pattern requires, after checking trigger/condition presence.
fn pattern_lead<'s>(
    requirement_ref: &Id,
    suggestion: &'s EarsSuggestion,
) -> Result<(&'static str, Option<&'s EarsGrounding>), EarsError> {
    let pattern = suggestion.pattern;
    let invalid = |reason: &'static str| EarsError::InvalidPattern {
        requirement_ref: requirement_ref.clone(),
        pattern: pattern.as_str(),
        reason,
    };
    match (pattern, &suggestion.trigger, &suggestion.condition) {
        (EarsPattern::Ubiquitous, None, None) => Ok(("", None)),
        (EarsPattern::Ubiquitous, _, _) => Err(invalid("ubiquitous takes no trigger or condition")),
        (EarsPattern::EventDriven, Some(trigger), None) => Ok(("trigger", Some(trigger))),
        (EarsPattern::EventDriven, _, _) => {
            Err(invalid("event_driven takes a trigger and no condition"))
        }
        (_, None, Some(condition)) => Ok(("condition", Some(condition))),
        _ => Err(invalid("pattern takes a condition and no trigger")),
    }
}

/// Resolves and checks every component of a pattern-checked suggestion and renders it.
fn render_suggestion(
    requirement: &EarsRequirementContext,
    catalog: &[EarsAcceptedSemantic],
    suggestion: &EarsSuggestion,
) -> Result<String, EarsError> {
    let id = &requirement.requirement_ref;
    let (lead_name, lead) = pattern_lead(id, suggestion)?;
    let actor = resolve_grounding(requirement, catalog, "actor", &suggestion.actor)?;
    let lead = match lead {
        Some(grounding) => Some(resolve_grounding(
            requirement,
            catalog,
            lead_name,
            grounding,
        )?),
        None => None,
    };
    let response = resolve_grounding(requirement, catalog, "response", &suggestion.response)?;
    check_component(id, "actor", &actor)?;
    if let Some(lead) = &lead {
        check_component(id, lead_name, lead)?;
    }
    check_component(id, "response", &response)?;
    Ok(render(
        suggestion.pattern,
        &actor,
        lead.as_deref(),
        requirement.modality,
        &response,
    ))
}

// ============================================================================ evaluation

/// A deterministic reason a target produced no Proposal. Not a PSG Finding; no rule IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum EarsIssue {
    InferenceUnavailable { requirement_ref: Id },
    NoSuggestion { requirement_ref: Id },
    AlreadyNormalized { requirement_ref: Id },
}

impl EarsIssue {
    /// The target the issue belongs to.
    pub fn requirement_ref(&self) -> &Id {
        match self {
            EarsIssue::InferenceUnavailable { requirement_ref }
            | EarsIssue::NoSuggestion { requirement_ref }
            | EarsIssue::AlreadyNormalized { requirement_ref } => requirement_ref,
        }
    }

    /// The exact issue name.
    pub fn as_str(&self) -> &'static str {
        match self {
            EarsIssue::InferenceUnavailable { .. } => "inference_unavailable",
            EarsIssue::NoSuggestion { .. } => "no_suggestion",
            EarsIssue::AlreadyNormalized { .. } => "already_normalized",
        }
    }
}

/// ReplacePayload proposals sorted by target Requirement ID, and issues sorted by
/// requirement_ref then issue name. Nothing has been applied or persisted.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EarsNormalizationResult {
    pub proposals: Vec<Proposal>,
    pub issues: Vec<EarsIssue>,
}

/// The HUMAN_CONFIRM proposal replacing only the target's statement.
fn propose_statement(
    graph: &Graph,
    base_semantic_hash: &Hash,
    requirement_ref: &Id,
    statement: String,
    derivation_ref: &DerivationRef,
) -> Result<Proposal, EarsError> {
    let invalid = |e: &dyn ToString| EarsError::InvalidProposal {
        reason: e.to_string(),
    };
    let node = graph
        .node(requirement_ref)
        .ok_or_else(|| invalid(&"target requirement vanished"))?;
    let NodePayload::Requirement(requirement) = &node.payload else {
        return Err(invalid(&"target is not a Requirement"));
    };
    let updated = Requirement {
        statement,
        ..requirement.clone()
    };
    Proposal::new(
        EARS_STAGE,
        PatchSet {
            base_semantic_hash: base_semantic_hash.clone(),
            patch: SemanticPatch::ReplacePayload {
                target: ElementPrecondition {
                    id: node.id.clone(),
                    expected_hash: node_element_hash(node)?,
                },
                payload: NodePayload::Requirement(updated),
            },
        },
        node.evidence.clone(),
        vec![derivation_ref.clone()],
        ProposalMateriality::Semantic,
        AcceptancePolicy::HumanConfirm,
        None,
    )
    .map_err(|e| invalid(&e))
}

/// Evaluates a supplied, already-acquired EARS inference for the request's targets. Absent
/// inference yields only InferenceUnavailable issues; malformed inference is an error.
pub fn evaluate_ears_normalization(
    graph: &Graph,
    ears_request: &EarsRequest,
    inference: Option<EarsInference<'_>>,
) -> Result<EarsNormalizationResult, EarsError> {
    ears_request.validate()?;
    let targets: Vec<Id> = ears_request
        .context
        .requirements
        .iter()
        .map(|r| r.requirement_ref.clone())
        .collect();
    if context_of(graph, &targets)? != ears_request.context {
        return Err(invalid_input(
            "graph does not match the EARS request context",
        ));
    }
    let context = &ears_request.context;
    let mut proposals = Vec::new();
    let mut issues = Vec::new();
    let Some(inference) = inference else {
        issues.extend(
            context
                .requirements
                .iter()
                .map(|r| EarsIssue::InferenceUnavailable {
                    requirement_ref: r.requirement_ref.clone(),
                }),
        );
        return Ok(EarsNormalizationResult { proposals, issues });
    };
    let suggestions = validated_suggestions(ears_request, inference.artifact)?;
    for (requirement_ref, suggestion) in &suggestions {
        if let Some(suggestion) = suggestion {
            pattern_lead(requirement_ref, suggestion)?;
        }
    }
    let mut rendered = Vec::new();
    for requirement in &context.requirements {
        let requirement_ref = requirement.requirement_ref.clone();
        match &suggestions[&requirement_ref] {
            None => issues.push(EarsIssue::NoSuggestion { requirement_ref }),
            Some(suggestion) => {
                let statement =
                    render_suggestion(requirement, &context.accepted_semantics, suggestion)?;
                if statement == requirement.current_statement {
                    issues.push(EarsIssue::AlreadyNormalized { requirement_ref });
                } else {
                    rendered.push((requirement_ref, statement));
                }
            }
        }
    }
    let base_semantic_hash = graph.semantic_hash()?;
    for (requirement_ref, statement) in rendered {
        proposals.push(propose_statement(
            graph,
            &base_semantic_hash,
            &requirement_ref,
            statement,
            &inference.derivation_ref,
        )?);
    }
    issues
        .sort_by(|a, b| (a.requirement_ref(), a.as_str()).cmp(&(b.requirement_ref(), b.as_str())));
    Ok(EarsNormalizationResult { proposals, issues })
}
