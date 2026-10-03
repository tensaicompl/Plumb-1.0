//! S1 requirement compilation (plan S1.1; compiler architecture §7).
//!
//! Compiles S0.4 requirement candidates, their EvidenceFragments and an optional validated
//! `requirement_classification` inference into human-confirmable Proposals, one `AddNode`
//! patch per Proposed Requirement, Goal, Need, Concern or Constraint. Nothing here mutates a
//! graph, calls a provider, reads a clock or persists anything; `requirement_kind`, `level` and
//! `modality` are never defaulted.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_import::{SegmentCandidate, SegmentClassification};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_patch::{AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch};
use plumb_psg::{
    AuditMeta, DerivationRef, ElementStatus, EvidenceRef, ExtensionKey, Graph, Modality, Node,
    NodePayload, NodeType, Requirement, RequirementKind, RequirementLevel,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::intent::{intent_payloads, validate_intents, IntentOutput};

/// Version of [`RequirementClassificationContext`].
pub const REQUIREMENT_CLASSIFICATION_CONTEXT_VERSION: u32 = 1;

/// Version of the classification inference output.
pub const REQUIREMENT_CLASSIFICATION_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of requirement classification.
pub const REQUIREMENT_CLASSIFICATION_TASK_KIND: &str = "requirement_classification";

/// Extension key of the [`SegmentOrigin`] of a node compiled from a segment candidate.
pub const SEGMENT_ORIGIN_EXTENSION: &str = "plumb_functional:segment_origin";

/// The exact prompt-template bytes; their Generic hash is the request's `prompt_template_hash`.
const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s1-classify-requirement.md");

/// The exact schema source; its byte hash is the request's `schema_hash`.
const SCHEMA_SOURCE: &str =
    include_str!("../../../schemas/inference/s1-requirement-classification.schema.json");

const CLASSIFICATION_STAGE: StageId = StageId::S1;

// ============================================================================ errors

/// Why requirement compilation could not run. Malformed supplied inference is an error, never
/// treated as absent inference.
#[derive(Debug, Error)]
pub enum RequirementCompilationError {
    /// The candidates, graph or classification request are not valid compilation input.
    #[error("invalid requirement compilation input: {reason}")]
    InvalidInput { reason: String },
    /// The InferenceRequest could not be constructed.
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    /// The inference artifact is not valid for the classification request.
    #[error("invalid classification inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    /// The classification schema does not compile.
    #[error("classification schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    /// The classification output does not satisfy the schema or does not decode.
    #[error("classification output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    /// A context candidate has no classification entry.
    #[error("candidate {candidate_ref} has no classification")]
    ClassificationCoverage { candidate_ref: Id },
    /// A classification entry names a candidate outside the request context.
    #[error("classification for unknown candidate {candidate_ref}")]
    UnknownCandidate { candidate_ref: Id },
    /// A candidate is classified more than once.
    #[error("candidate {candidate_ref} is classified more than once")]
    DuplicateClassification { candidate_ref: Id },
    /// The same intent kind is proposed twice for one candidate.
    #[error("duplicate {intent_kind} intent for candidate {candidate_ref}")]
    DuplicateIntent {
        candidate_ref: Id,
        intent_kind: &'static str,
    },
    /// An intent references an unknown candidate or an invalid stakeholder.
    #[error("invalid intent reference {reference} for candidate {candidate_ref}")]
    InvalidIntentReference { candidate_ref: Id, reference: Id },
    /// A proposed intent field is not acceptable as-is.
    #[error("invalid intent for candidate {candidate_ref}: {reason}")]
    InvalidIntent { candidate_ref: Id, reason: String },
    /// A proposed node or proposal is structurally invalid.
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    /// Canonicalization or hashing failed.
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_input(reason: impl Into<String>) -> RequirementCompilationError {
    RequirementCompilationError::InvalidInput {
        reason: reason.into(),
    }
}

pub(crate) fn invalid_proposal(reason: impl ToString) -> RequirementCompilationError {
    RequirementCompilationError::InvalidProposal {
        reason: reason.to_string(),
    }
}

// ============================================================================ context and request

/// The exact classification context: the requirement candidates with their exact source text
/// and the graph's Accepted stakeholders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementClassificationContext {
    pub version: u32,
    pub project_id: Id,
    pub candidates: Vec<RequirementClassificationContextCandidate>,
    pub stakeholders: Vec<RequirementClassificationStakeholder>,
}

/// One requirement candidate of a [`RequirementClassificationContext`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementClassificationContextCandidate {
    pub candidate_ref: Id,
    pub fragment_ref: Id,
    pub text: String,
}

/// One Accepted stakeholder a Need may reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementClassificationStakeholder {
    pub stakeholder_ref: Id,
    pub name: String,
}

impl RequirementClassificationContext {
    /// Version 1, at least one candidate, candidates and stakeholders strictly sorted.
    pub fn validate(&self) -> Result<(), RequirementCompilationError> {
        if self.version != REQUIREMENT_CLASSIFICATION_CONTEXT_VERSION {
            return Err(invalid_input(format!(
                "context version {} is not 1",
                self.version
            )));
        }
        if self.candidates.is_empty() {
            return Err(invalid_input("context has no requirement candidates"));
        }
        if self
            .candidates
            .windows(2)
            .any(|pair| pair[0].candidate_ref >= pair[1].candidate_ref)
        {
            return Err(invalid_input(
                "context candidates are not strictly sorted by candidate_ref",
            ));
        }
        if self
            .stakeholders
            .windows(2)
            .any(|pair| pair[0].stakeholder_ref >= pair[1].stakeholder_ref)
        {
            return Err(invalid_input(
                "context stakeholders are not strictly sorted by stakeholder_ref",
            ));
        }
        Ok(())
    }

    /// Generic SHA-256 of the RFC 8785 canonical JSON of the validated context.
    pub fn content_hash(&self) -> Result<Hash, RequirementCompilationError> {
        self.validate()?;
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn input_refs(&self) -> Vec<Id> {
        let refs: BTreeSet<&Id> = self
            .candidates
            .iter()
            .map(|c| &c.candidate_ref)
            .chain(self.stakeholders.iter().map(|s| &s.stakeholder_ref))
            .collect();
        refs.into_iter().cloned().collect()
    }

    fn evidence_refs(&self) -> Vec<Id> {
        let refs: BTreeSet<&Id> = self.candidates.iter().map(|c| &c.fragment_ref).collect();
        refs.into_iter().cloned().collect()
    }
}

/// A classification `InferenceRequest` together with the exact context it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementClassificationRequest {
    pub request: InferenceRequest,
    pub context: RequirementClassificationContext,
}

impl RequirementClassificationRequest {
    /// Validates both parts and their binding.
    pub fn new(
        request: InferenceRequest,
        context: RequirementClassificationContext,
    ) -> Result<RequirementClassificationRequest, RequirementCompilationError> {
        let classification = RequirementClassificationRequest { request, context };
        classification.validate()?;
        Ok(classification)
    }

    /// The request is a valid S1 requirement_classification request bound to this context and
    /// to the committed prompt and schema.
    pub fn validate(&self) -> Result<(), RequirementCompilationError> {
        let request = &self.request;
        request
            .validate()
            .map_err(|e| invalid_input(format!("invalid inference request: {e}")))?;
        let context_hash = self.context.content_hash()?;
        let mismatch = if request.stage != CLASSIFICATION_STAGE {
            Some("stage is not S1")
        } else if request.task_kind != REQUIREMENT_CLASSIFICATION_TASK_KIND {
            Some("task_kind is not requirement_classification")
        } else if request.input_refs != self.context.input_refs() {
            Some("input_refs differ from the context candidates and stakeholders")
        } else if request.evidence_refs != self.context.evidence_refs() {
            Some("evidence_refs differ from the context fragments")
        } else if request.context_hash != context_hash {
            Some("context_hash differs from the context hash")
        } else if request.prompt_template_hash != prompt_template_hash() {
            Some("prompt_template_hash differs from the classification prompt")
        } else if request.schema_hash != schema_hash() {
            Some("schema_hash differs from the classification schema")
        } else {
            None
        };
        match mismatch {
            Some(reason) => Err(invalid_input(reason)),
            None => Ok(()),
        }
    }
}

fn prompt_template_hash() -> Hash {
    Hash::content_sha256(PROMPT_TEMPLATE)
}

fn schema_hash() -> Hash {
    Hash::content_sha256(SCHEMA_SOURCE.as_bytes())
}

/// A requirement candidate resolved against its EvidenceFragment.
pub(crate) struct ResolvedCandidate<'a> {
    pub(crate) candidate: &'a SegmentCandidate,
    pub(crate) text: &'a str,
}

/// The requirement candidates, sorted by ID, with their exact source slices.
fn resolve_candidates<'a>(
    graph: &'a Graph,
    candidates: &'a [SegmentCandidate],
) -> Result<Vec<ResolvedCandidate<'a>>, RequirementCompilationError> {
    let mut resolved: BTreeMap<&Id, ResolvedCandidate<'a>> = BTreeMap::new();
    for candidate in candidates
        .iter()
        .filter(|c| c.classification == SegmentClassification::RequirementCandidate)
    {
        let reason = |what: &str| invalid_input(format!("candidate {}: {what}", candidate.id));
        let fragment = match graph.node(&candidate.fragment_ref).map(|n| &n.payload) {
            Some(NodePayload::EvidenceFragment(fragment)) => fragment,
            Some(_) => return Err(reason("fragment_ref is not an EvidenceFragment")),
            None => return Err(reason("fragment_ref does not resolve in the graph")),
        };
        let text = fragment
            .extracted_text
            .as_deref()
            .ok_or_else(|| reason("EvidenceFragment has no extracted_text"))?;
        let (Ok(start), Ok(end)) = (
            usize::try_from(candidate.start),
            usize::try_from(candidate.end),
        ) else {
            return Err(reason("range exceeds the fragment text"));
        };
        if start >= end
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            return Err(reason(
                "range is not a valid UTF-8 range of the fragment text",
            ));
        }
        let entry = ResolvedCandidate {
            candidate,
            text: &text[start..end],
        };
        if resolved.insert(&candidate.id, entry).is_some() {
            return Err(reason("duplicate candidate ID"));
        }
    }
    if resolved.is_empty() {
        return Err(invalid_input("no requirement candidates"));
    }
    Ok(resolved.into_values().collect())
}

fn context_of(
    graph: &Graph,
    resolved: &[ResolvedCandidate<'_>],
) -> RequirementClassificationContext {
    let stakeholders = graph
        .node_ids_by_type(NodeType::Stakeholder)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|node| node.status == ElementStatus::Accepted)
        .filter_map(|node| match &node.payload {
            NodePayload::Stakeholder(stakeholder) => Some(RequirementClassificationStakeholder {
                stakeholder_ref: node.id.clone(),
                name: stakeholder.name.clone(),
            }),
            _ => None,
        })
        .collect();
    RequirementClassificationContext {
        version: REQUIREMENT_CLASSIFICATION_CONTEXT_VERSION,
        project_id: graph.project_id().clone(),
        candidates: resolved
            .iter()
            .map(|r| RequirementClassificationContextCandidate {
                candidate_ref: r.candidate.id.clone(),
                fragment_ref: r.candidate.fragment_ref.clone(),
                text: r.text.to_owned(),
            })
            .collect(),
        stakeholders,
    }
}

/// Builds the S1 classification request for the requirement candidates among `candidates`.
/// The provider policy is always the caller's; no provider is preferred.
pub fn build_requirement_classification_request(
    graph: &Graph,
    candidates: &[SegmentCandidate],
    provider_policy: ProviderPolicy,
) -> Result<RequirementClassificationRequest, RequirementCompilationError> {
    let context = context_of(graph, &resolve_candidates(graph, candidates)?);
    let request = InferenceRequest::new(
        CLASSIFICATION_STAGE,
        REQUIREMENT_CLASSIFICATION_TASK_KIND.to_owned(),
        context.input_refs(),
        context.evidence_refs(),
        context.content_hash()?,
        prompt_template_hash(),
        schema_hash(),
        provider_policy,
    )?;
    RequirementClassificationRequest::new(request, context)
}

// ============================================================================ classification output

/// An already-acquired classification artifact and the caller's provenance ref for it.
#[derive(Debug, Clone)]
pub struct RequirementClassificationInference<'a> {
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassificationOutput {
    version: u32,
    classifications: Vec<ClassificationEntry>,
    intents: Vec<IntentOutput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassificationEntry {
    candidate_ref: Id,
    requirement_kind: Option<RequirementKind>,
    level: Option<RequirementLevel>,
}

/// The classifier's proposal for one candidate; `None` means it declined to guess.
#[derive(Clone, Copy)]
struct Classification {
    requirement_kind: Option<RequirementKind>,
    level: Option<RequirementLevel>,
}

/// Validated classification output, keyed by candidate ID.
struct ValidatedClassification {
    classifications: BTreeMap<Id, Classification>,
    intents: Vec<IntentOutput>,
}

fn compile_schema() -> Result<JSONSchema, RequirementCompilationError> {
    let schema: Value = serde_json::from_str(SCHEMA_SOURCE).map_err(|e| {
        RequirementCompilationError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        }
    })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| RequirementCompilationError::SchemaCompilation {
            reason: e.to_string(),
        })
}

fn schema_invalid(reason: impl Into<String>) -> RequirementCompilationError {
    RequirementCompilationError::SchemaInvalid {
        reason: reason.into(),
    }
}

/// Validates a supplied artifact in the contract order; any failure rejects the whole output.
fn validate_classification(
    graph: &Graph,
    request: &RequirementClassificationRequest,
    artifact: &InferenceArtifact,
) -> Result<ValidatedClassification, RequirementCompilationError> {
    artifact.validate_for(&request.request).map_err(|e| {
        RequirementCompilationError::InvalidInferenceArtifact {
            reason: e.to_string(),
        }
    })?;
    let output = artifact.validated_output.as_value();
    let schema = compile_schema()?;
    if let Err(errors) = schema.validate(output) {
        let mut messages: Vec<String> = errors
            .map(|e| format!("{e} at {}", e.instance_path))
            .collect();
        messages.sort();
        return Err(schema_invalid(messages.join("; ")));
    }
    let decoded: ClassificationOutput =
        serde_json::from_value(output.clone()).map_err(|e| schema_invalid(e.to_string()))?;
    if decoded.version != REQUIREMENT_CLASSIFICATION_OUTPUT_VERSION {
        return Err(schema_invalid(format!(
            "output version {} is not 1",
            decoded.version
        )));
    }
    let known: BTreeSet<&Id> = request
        .context
        .candidates
        .iter()
        .map(|c| &c.candidate_ref)
        .collect();
    let mut classifications = BTreeMap::new();
    let mut entries = decoded.classifications;
    entries.sort_by(|a, b| a.candidate_ref.cmp(&b.candidate_ref));
    for entry in entries {
        if !known.contains(&entry.candidate_ref) {
            return Err(RequirementCompilationError::UnknownCandidate {
                candidate_ref: entry.candidate_ref,
            });
        }
        let classification = Classification {
            requirement_kind: entry.requirement_kind,
            level: entry.level,
        };
        if classifications
            .insert(entry.candidate_ref.clone(), classification)
            .is_some()
        {
            return Err(RequirementCompilationError::DuplicateClassification {
                candidate_ref: entry.candidate_ref,
            });
        }
    }
    if let Some(missing) = known.iter().find(|id| !classifications.contains_key(**id)) {
        return Err(RequirementCompilationError::ClassificationCoverage {
            candidate_ref: (*missing).clone(),
        });
    }
    let intents = validate_intents(graph, &request.context, decoded.intents)?;
    Ok(ValidatedClassification {
        classifications,
        intents,
    })
}

// ============================================================================ statement parsing

/// The deterministic reading of one candidate's source text.
pub(crate) struct ParsedStatement {
    pub(crate) source_identifier: Option<String>,
    pub(crate) explicit_kind: Option<RequirementKind>,
    pub(crate) statement: String,
}

fn is_ascii_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn source_prefix() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?:(?:[0-9]+[.)]|[-*+])[ \t]+)?(?:\*\*([A-Za-z][A-Za-z0-9._-]*)\*\*|([A-Za-z][A-Za-z0-9._-]*))[ \t]+\[(functional|quality|interface|data|security|operational|compliance|transition|constraint)\][ \t\r\n]",
        )
        .expect("valid source-prefix regex")
    })
}

/// Trims outer ASCII whitespace and strips a complete source metadata prefix, or nothing.
pub(crate) fn parse_statement(source: &str) -> ParsedStatement {
    let trimmed = source.trim_matches(is_ascii_space);
    let Some(captures) = source_prefix().captures(trimmed) else {
        return ParsedStatement {
            source_identifier: None,
            explicit_kind: None,
            statement: trimmed.to_owned(),
        };
    };
    let token = captures
        .get(1)
        .or_else(|| captures.get(2))
        .map(|m| m.as_str().to_owned());
    let kind = serde_json::from_value(Value::from(&captures[3])).ok();
    let remainder = &trimmed[captures.get(0).map_or(0, |m| m.end())..];
    ParsedStatement {
        source_identifier: token,
        explicit_kind: kind,
        statement: remainder.trim_matches(is_ascii_space).to_owned(),
    }
}

/// The deterministic modality of a clean statement, or the unresolved phrase.
enum ModalityReading {
    Modality(Modality),
    Unsupported(&'static str),
    Missing,
}

/// The first whole ASCII word token among shall, should, may, must and can controls; a
/// following `not` (separated only by ASCII whitespace) negates it.
fn read_modality(statement: &str) -> ModalityReading {
    let bytes = statement.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut tokens: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if is_word(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_word(bytes[i]) {
                i += 1;
            }
            tokens.push((start, i));
        } else {
            i += 1;
        }
    }
    let word = |(start, end): (usize, usize)| &statement[start..end];
    for (index, &token) in tokens.iter().enumerate() {
        let lower = word(token).to_ascii_lowercase();
        if !matches!(lower.as_str(), "shall" | "should" | "may" | "must" | "can") {
            continue;
        }
        let negated = tokens.get(index + 1).is_some_and(|&next| {
            word(next).eq_ignore_ascii_case("not")
                && bytes[token.1..next.0]
                    .iter()
                    .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        });
        return match (lower.as_str(), negated) {
            ("shall", false) => ModalityReading::Modality(Modality::Shall),
            ("shall", true) => ModalityReading::Modality(Modality::ShallNot),
            ("should", false) => ModalityReading::Modality(Modality::Should),
            ("should", true) => ModalityReading::Unsupported("should not"),
            ("may", false) => ModalityReading::Modality(Modality::May),
            ("may", true) => ModalityReading::Unsupported("may not"),
            ("must", false) => ModalityReading::Unsupported("must"),
            ("must", true) => ModalityReading::Unsupported("must not"),
            _ => ModalityReading::Unsupported("can"),
        };
    }
    ModalityReading::Missing
}

// ============================================================================ nodes and proposals

/// The exact candidate range a compiled node came from (`plumb_functional:segment_origin`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentOrigin {
    pub candidate_ref: Id,
    pub fragment_ref: Id,
    pub start: u64,
    pub end: u64,
}

/// Who owns the Proposed nodes and when, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementCompilationAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// `<prefix>:<first 16 hex of SHA-256(RFC 8785 {project_id, candidate_ref, node_type})>`.
pub(crate) fn semantic_node_id(
    project_id: &Id,
    candidate_ref: &Id,
    node_type: &str,
    prefix: &str,
) -> Result<Id, CoreError> {
    let body = json!({
        "project_id": project_id,
        "candidate_ref": candidate_ref,
        "node_type": node_type,
    });
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("{prefix}:{}", &hex[..16]).parse()
}

/// Everything needed to emit proposals for one candidate.
pub(crate) struct ProposalContext<'a> {
    pub(crate) graph: &'a Graph,
    pub(crate) base_semantic_hash: &'a Hash,
    pub(crate) derivation_ref: &'a DerivationRef,
    pub(crate) audit: &'a RequirementCompilationAudit,
}

/// A Proposed node for `candidate` wrapped in its own HUMAN_CONFIRM semantic Proposal.
pub(crate) fn propose_node(
    context: &ProposalContext<'_>,
    candidate: &SegmentCandidate,
    node_type: &str,
    prefix: &str,
    payload: NodePayload,
) -> Result<Proposal, RequirementCompilationError> {
    let origin = SegmentOrigin {
        candidate_ref: candidate.id.clone(),
        fragment_ref: candidate.fragment_ref.clone(),
        start: candidate.start,
        end: candidate.end,
    };
    let key: ExtensionKey = SEGMENT_ORIGIN_EXTENSION.parse().map_err(invalid_proposal)?;
    let origin = serde_json::to_value(&origin).map_err(invalid_proposal)?;
    let node = Node {
        id: semantic_node_id(context.graph.project_id(), &candidate.id, node_type, prefix)?,
        revision: 1,
        status: ElementStatus::Proposed,
        payload,
        evidence: vec![EvidenceRef::from(candidate.fragment_ref.clone())],
        derivations: Vec::new(),
        standards: Vec::new(),
        tags: BTreeSet::new(),
        extensions: BTreeMap::from([(key, origin)]),
        audit: AuditMeta::new(
            context.audit.created_by.clone(),
            context.audit.created_at,
            None,
            None,
        )
        .map_err(invalid_proposal)?,
    };
    node.validate().map_err(invalid_proposal)?;
    Proposal::new(
        CLASSIFICATION_STAGE,
        PatchSet {
            base_semantic_hash: context.base_semantic_hash.clone(),
            patch: SemanticPatch::AddNode { node },
        },
        vec![EvidenceRef::from(candidate.fragment_ref.clone())],
        vec![context.derivation_ref.clone()],
        ProposalMateriality::Semantic,
        AcceptancePolicy::HumanConfirm,
        None,
    )
    .map_err(invalid_proposal)
}

fn proposed_node_id(proposal: &Proposal) -> Option<&Id> {
    match &proposal.patch_set.patch {
        SemanticPatch::AddNode { node } => Some(&node.id),
        _ => None,
    }
}

// ============================================================================ compilation

/// A deterministic reason a candidate produced no Requirement proposal. These are compilation
/// outcomes, not PSG Finding nodes, and carry no rule IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequirementCompilationIssue {
    ClassificationUnavailable { candidate_ref: Id },
    RequirementKindUnknown { candidate_ref: Id },
    RequirementLevelUnknown { candidate_ref: Id },
    MissingModality { candidate_ref: Id },
    UnsupportedModality { candidate_ref: Id, token: String },
    EmptyStatement { candidate_ref: Id },
}

impl RequirementCompilationIssue {
    /// The candidate the issue belongs to.
    pub fn candidate_ref(&self) -> &Id {
        match self {
            RequirementCompilationIssue::ClassificationUnavailable { candidate_ref }
            | RequirementCompilationIssue::RequirementKindUnknown { candidate_ref }
            | RequirementCompilationIssue::RequirementLevelUnknown { candidate_ref }
            | RequirementCompilationIssue::MissingModality { candidate_ref }
            | RequirementCompilationIssue::UnsupportedModality { candidate_ref, .. }
            | RequirementCompilationIssue::EmptyStatement { candidate_ref } => candidate_ref,
        }
    }

    /// The exact issue name.
    pub fn as_str(&self) -> &'static str {
        match self {
            RequirementCompilationIssue::ClassificationUnavailable { .. } => {
                "classification_unavailable"
            }
            RequirementCompilationIssue::RequirementKindUnknown { .. } => {
                "requirement_kind_unknown"
            }
            RequirementCompilationIssue::RequirementLevelUnknown { .. } => {
                "requirement_level_unknown"
            }
            RequirementCompilationIssue::MissingModality { .. } => "missing_modality",
            RequirementCompilationIssue::UnsupportedModality { .. } => "unsupported_modality",
            RequirementCompilationIssue::EmptyStatement { .. } => "empty_statement",
        }
    }

    fn order_key(&self) -> (&Id, &'static str, &str) {
        let token = match self {
            RequirementCompilationIssue::UnsupportedModality { token, .. } => token.as_str(),
            _ => "",
        };
        (self.candidate_ref(), self.as_str(), token)
    }
}

/// Proposals sorted by proposed node ID and unresolved issues in canonical order. Nothing has
/// been applied, committed or persisted.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RequirementCompilationResult {
    pub proposals: Vec<Proposal>,
    pub unresolved: Vec<RequirementCompilationIssue>,
}

/// Compiles the requirement candidates into HUMAN_CONFIRM proposals of Proposed nodes. Absent
/// classification leaves every candidate unresolved; malformed classification is an error.
pub fn compile_requirement_candidates(
    graph: &Graph,
    candidates: &[SegmentCandidate],
    classification_request: &RequirementClassificationRequest,
    classification: Option<RequirementClassificationInference<'_>>,
    audit: &RequirementCompilationAudit,
) -> Result<RequirementCompilationResult, RequirementCompilationError> {
    classification_request.validate()?;
    let resolved = resolve_candidates(graph, candidates)?;
    if context_of(graph, &resolved) != classification_request.context {
        return Err(invalid_input(
            "graph and candidates do not match the classification request context",
        ));
    }
    let mut proposals = Vec::new();
    let mut unresolved = Vec::new();
    let Some(inference) = classification else {
        for r in &resolved {
            let candidate_ref = r.candidate.id.clone();
            unresolved.push(if parse_statement(r.text).statement.is_empty() {
                RequirementCompilationIssue::EmptyStatement { candidate_ref }
            } else {
                RequirementCompilationIssue::ClassificationUnavailable { candidate_ref }
            });
        }
        return Ok(finish(proposals, unresolved));
    };
    let validated = validate_classification(graph, classification_request, inference.artifact)?;
    let base_semantic_hash = graph.semantic_hash()?;
    let context = ProposalContext {
        graph,
        base_semantic_hash: &base_semantic_hash,
        derivation_ref: &inference.derivation_ref,
        audit,
    };
    for r in &resolved {
        let candidate_ref = &r.candidate.id;
        let parsed = parse_statement(r.text);
        if parsed.statement.is_empty() {
            unresolved.push(RequirementCompilationIssue::EmptyStatement {
                candidate_ref: candidate_ref.clone(),
            });
            continue;
        }
        let classified = validated.classifications[candidate_ref];
        let mut issues = Vec::new();
        let requirement_kind = parsed.explicit_kind.or(classified.requirement_kind);
        if requirement_kind.is_none() {
            issues.push(RequirementCompilationIssue::RequirementKindUnknown {
                candidate_ref: candidate_ref.clone(),
            });
        }
        if classified.level.is_none() {
            issues.push(RequirementCompilationIssue::RequirementLevelUnknown {
                candidate_ref: candidate_ref.clone(),
            });
        }
        let modality = match read_modality(&parsed.statement) {
            ModalityReading::Modality(modality) => Some(modality),
            ModalityReading::Unsupported(token) => {
                issues.push(RequirementCompilationIssue::UnsupportedModality {
                    candidate_ref: candidate_ref.clone(),
                    token: token.to_owned(),
                });
                None
            }
            ModalityReading::Missing => {
                issues.push(RequirementCompilationIssue::MissingModality {
                    candidate_ref: candidate_ref.clone(),
                });
                None
            }
        };
        if let (Some(requirement_kind), Some(level), Some(modality)) =
            (requirement_kind, classified.level, modality)
        {
            let requirement = Requirement {
                statement: parsed.statement.clone(),
                requirement_kind,
                level,
                modality,
                title: None,
                rationale: None,
                priority: None,
                source_identifier: parsed.source_identifier.clone(),
                verification_method: None,
                owner_refs: None,
                stakeholder_refs: None,
            };
            proposals.push(propose_node(
                &context,
                r.candidate,
                "requirement",
                "req",
                NodePayload::Requirement(requirement),
            )?);
        }
        unresolved.extend(issues);
        for (node_type, prefix, payload) in
            intent_payloads(&validated.intents, candidate_ref, &parsed.statement)
        {
            proposals.push(propose_node(
                &context,
                r.candidate,
                node_type,
                prefix,
                payload,
            )?);
        }
    }
    Ok(finish(proposals, unresolved))
}

fn finish(
    mut proposals: Vec<Proposal>,
    mut unresolved: Vec<RequirementCompilationIssue>,
) -> RequirementCompilationResult {
    proposals.sort_by(|a, b| proposed_node_id(a).cmp(&proposed_node_id(b)));
    unresolved.sort_by(|a, b| a.order_key().cmp(&b.order_key()));
    RequirementCompilationResult {
        proposals,
        unresolved,
    }
}
