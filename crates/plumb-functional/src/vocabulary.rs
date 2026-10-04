//! Span-grounded vocabulary analysis (plan S1.5; compiler architecture §7).
//!
//! A supplied, already-acquired `vocabulary_analysis` inference selects vocabulary mention
//! ranges in current Requirement statements, optionally with a v3 concept kind and a grounded
//! definition range. Plumb derives every text from those ranges, normalizes it
//! deterministically (English-only pilot), computes frequency and co-occurrence, builds the
//! plumb-lint term contexts, reconciles against Accepted vocabulary, reports conflicts and F1
//! finding material, and proposes Proposed Term and grounded Concept nodes for human
//! confirmation. Nothing here mutates a graph, calls a provider, reads a clock or persists
//! anything, and there is no fallback when inference is absent.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_lint::{LintTextRange, TermLintContext, TermMention};
use plumb_patch::{AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch};
use plumb_psg::{
    AuditMeta, Concept, ConceptKind, DerivationRef, ElementStatus, EvidenceRef, ExtensionKey,
    Graph, Node, NodePayload, NodeType, Term,
};
use plumb_validation::{
    load_builtin_software_profile, GeneratedFinding, RuleMetadata, ViolationFacts,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::vocabulary_exceptions::{INVARIANT_NOUNS, IRREGULAR_PLURALS};

/// The only language of the pilot normalization.
pub const PILOT_VOCABULARY_LANGUAGE: &str = "en";

/// Version of [`VocabularyContext`].
pub const VOCABULARY_CONTEXT_VERSION: u32 = 1;

/// Version of the vocabulary inference output.
pub const VOCABULARY_OUTPUT_VERSION: u32 = 1;

/// Version of the normalization algorithm; part of the inference context.
pub const VOCABULARY_NORMALIZATION_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of vocabulary analysis.
pub const VOCABULARY_TASK_KIND: &str = "vocabulary_analysis";

/// Extension key of the provenance-only [`VocabularyOrigin`].
pub const VOCABULARY_ORIGIN_EXTENSION: &str = "plumb_functional:vocabulary_origin";

const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s1-vocabulary.md");
const SCHEMA_SOURCE: &str = include_str!("../../../schemas/inference/s1-vocabulary.schema.json");
const VOCABULARY_STAGE: StageId = StageId::S1;
const TERMS_RESOLVED_RULE: &str = "PLUMB.F1.REQ.TERMS_RESOLVED";
const FINDING_RESOLUTION: &str = "Review the vocabulary evidence and establish one governed \
vocabulary interpretation before treating the term as resolved.";

// ============================================================================ errors

/// Why vocabulary analysis could not run. Malformed supplied inference is an error, never
/// treated as absent inference.
#[derive(Debug, Error)]
pub enum VocabularyError {
    #[error("invalid vocabulary input: {reason}")]
    InvalidInput { reason: String },
    #[error("unsupported vocabulary language {language:?}")]
    UnsupportedLanguage { language: String },
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    #[error("invalid vocabulary inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error("vocabulary schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    #[error("vocabulary output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("mention references unknown requirement {requirement_ref}")]
    UnknownRequirement { requirement_ref: Id },
    #[error("invalid mention {requirement_ref} {start}..{end}: {reason}")]
    InvalidMention {
        requirement_ref: Id,
        start: u64,
        end: u64,
        reason: String,
    },
    #[error("duplicate mention {requirement_ref} {start}..{end}")]
    DuplicateMention {
        requirement_ref: Id,
        start: u64,
        end: u64,
    },
    #[error("overlapping mentions in {requirement_ref}: {first_start}..{first_end} and {second_start}..{second_end}")]
    OverlappingMention {
        requirement_ref: Id,
        first_start: u64,
        first_end: u64,
        second_start: u64,
        second_end: u64,
    },
    #[error("invalid definition grounding {requirement_ref} {start}..{end}: {reason}")]
    InvalidDefinitionGrounding {
        requirement_ref: Id,
        start: u64,
        end: u64,
        reason: String,
    },
    #[error("existing node {node_ref} conflicts with the vocabulary candidate {normalized_key:?}")]
    ExistingVocabularyCandidateConflict {
        node_ref: Id,
        normalized_key: String,
    },
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    #[error("validation profile: {reason}")]
    ValidationProfile { reason: String },
    #[error("generated finding: {reason}")]
    GeneratedFinding { reason: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_input(reason: impl Into<String>) -> VocabularyError {
    VocabularyError::InvalidInput {
        reason: reason.into(),
    }
}

fn invalid_proposal(e: impl ToString) -> VocabularyError {
    VocabularyError::InvalidProposal {
        reason: e.to_string(),
    }
}

// ============================================================================ normalization

/// The English-only pilot policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyPolicy {
    pub language: String,
}

impl VocabularyPolicy {
    /// The language must be exactly `en`.
    pub fn validate(&self) -> Result<(), VocabularyError> {
        if self.language == PILOT_VOCABULARY_LANGUAGE {
            Ok(())
        } else {
            Err(VocabularyError::UnsupportedLanguage {
                language: self.language.clone(),
            })
        }
    }
}

/// Singularizes one lowercase token by the exception tables, then the suffix rules.
fn singularize(token: &str) -> String {
    if let Ok(i) = IRREGULAR_PLURALS.binary_search_by(|(plural, _)| (*plural).cmp(token)) {
        return IRREGULAR_PLURALS[i].1.to_owned();
    }
    if INVARIANT_NOUNS.binary_search(&token).is_ok() || !token.is_ascii() {
        return token.to_owned();
    }
    if token.len() > 3 && token.ends_with("ies") {
        return format!("{}y", &token[..token.len() - 3]);
    }
    if ["ches", "shes", "xes", "zes", "sses"]
        .iter()
        .any(|suffix| token.ends_with(suffix))
    {
        return token[..token.len() - 2].to_owned();
    }
    if token.ends_with('s') && !["ss", "us", "is"].iter().any(|s| token.ends_with(s)) {
        return token[..token.len() - 1].to_owned();
    }
    token.to_owned()
}

/// The pilot normalized key of a vocabulary surface: NFKC, Unicode lowercase, alphanumeric
/// tokens, one leading determiner removed, final token singularized, tokens joined by one
/// space. `None` when nothing remains.
pub fn normalize_vocabulary_term(surface: &str) -> Option<String> {
    let lowered: String = surface
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let mut tokens: Vec<String> = lowered
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect();
    if tokens
        .first()
        .is_some_and(|t| matches!(t.as_str(), "a" | "an" | "the"))
    {
        tokens.remove(0);
    }
    let last = tokens.pop()?;
    tokens.push(singularize(&last));
    Some(tokens.join(" "))
}

fn is_clean(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

// ============================================================================ context and request

/// One target Requirement with its current statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyRequirementContext {
    pub requirement_ref: Id,
    pub status: ElementStatus,
    pub statement: String,
}

/// The exact vocabulary inference context. Accepted vocabulary is deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyContext {
    pub version: u32,
    pub normalization_version: u32,
    pub project_id: Id,
    pub language: String,
    pub requirements: Vec<VocabularyRequirementContext>,
}

impl VocabularyContext {
    /// Versions 1, language en, non-empty strictly sorted Proposed/Accepted requirements.
    pub fn validate(&self) -> Result<(), VocabularyError> {
        if self.version != VOCABULARY_CONTEXT_VERSION
            || self.normalization_version != VOCABULARY_NORMALIZATION_VERSION
        {
            return Err(invalid_input(
                "unsupported context or normalization version",
            ));
        }
        if self.language != PILOT_VOCABULARY_LANGUAGE {
            return Err(VocabularyError::UnsupportedLanguage {
                language: self.language.clone(),
            });
        }
        if self.requirements.is_empty() {
            return Err(invalid_input("context has no target requirements"));
        }
        if self
            .requirements
            .windows(2)
            .any(|p| p[0].requirement_ref >= p[1].requirement_ref)
        {
            return Err(invalid_input(
                "context requirements are not strictly sorted",
            ));
        }
        if self
            .requirements
            .iter()
            .any(|r| !matches!(r.status, ElementStatus::Proposed | ElementStatus::Accepted))
        {
            return Err(invalid_input(
                "context requirement is not Proposed or Accepted",
            ));
        }
        Ok(())
    }

    /// Generic SHA-256 of the RFC 8785 canonical JSON of the validated context.
    pub fn content_hash(&self) -> Result<Hash, VocabularyError> {
        self.validate()?;
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn requirement(&self, id: &Id) -> Option<&VocabularyRequirementContext> {
        self.requirements
            .binary_search_by(|r| r.requirement_ref.cmp(id))
            .ok()
            .map(|i| &self.requirements[i])
    }
}

/// A vocabulary `InferenceRequest` together with the exact context it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyRequest {
    pub request: InferenceRequest,
    pub context: VocabularyContext,
}

fn evidence_of(graph: &Graph, ids: &[&Id]) -> Vec<Id> {
    let refs: BTreeSet<Id> = ids
        .iter()
        .filter_map(|id| graph.node(id))
        .flat_map(|n| n.evidence.iter().map(|e| e.as_id().clone()))
        .collect();
    refs.into_iter().collect()
}

impl VocabularyRequest {
    /// The request is a valid S1 vocabulary_analysis request bound to this context, the
    /// committed prompt and schema, with exactly the targets as input refs. The evidence refs
    /// depend on the graph and are checked by [`analyze_vocabulary`].
    pub fn validate(&self) -> Result<(), VocabularyError> {
        let request = &self.request;
        request
            .validate()
            .map_err(|e| invalid_input(format!("invalid inference request: {e}")))?;
        let context_hash = self.context.content_hash()?;
        let targets: Vec<Id> = self
            .context
            .requirements
            .iter()
            .map(|r| r.requirement_ref.clone())
            .collect();
        let mismatch = if request.stage != VOCABULARY_STAGE {
            Some("stage is not S1")
        } else if request.task_kind != VOCABULARY_TASK_KIND {
            Some("task_kind is not vocabulary_analysis")
        } else if request.input_refs != targets {
            Some("input_refs differ from the target requirements")
        } else if request.context_hash != context_hash {
            Some("context_hash differs from the context hash")
        } else if request.prompt_template_hash != Hash::content_sha256(PROMPT_TEMPLATE) {
            Some("prompt_template_hash differs from the vocabulary prompt")
        } else if request.schema_hash != Hash::content_sha256(SCHEMA_SOURCE.as_bytes()) {
            Some("schema_hash differs from the vocabulary schema")
        } else {
            None
        };
        match mismatch {
            Some(reason) => Err(invalid_input(reason)),
            None => Ok(()),
        }
    }
}

fn context_of(
    graph: &Graph,
    requirement_refs: &[Id],
    policy: &VocabularyPolicy,
) -> Result<VocabularyContext, VocabularyError> {
    policy.validate()?;
    let mut targets = BTreeSet::new();
    for id in requirement_refs {
        if !targets.insert(id) {
            return Err(invalid_input(format!("duplicate target requirement {id}")));
        }
    }
    let mut requirements = Vec::new();
    for id in targets {
        let node = graph
            .node(id)
            .ok_or_else(|| invalid_input(format!("requirement {id} does not exist")))?;
        let NodePayload::Requirement(requirement) = &node.payload else {
            return Err(invalid_input(format!("{id} is not a Requirement")));
        };
        if !matches!(
            node.status,
            ElementStatus::Proposed | ElementStatus::Accepted
        ) {
            return Err(invalid_input(format!(
                "requirement {id} status {:?} is not Proposed or Accepted",
                node.status
            )));
        }
        requirements.push(VocabularyRequirementContext {
            requirement_ref: id.clone(),
            status: node.status,
            statement: requirement.statement.clone(),
        });
    }
    Ok(VocabularyContext {
        version: VOCABULARY_CONTEXT_VERSION,
        normalization_version: VOCABULARY_NORMALIZATION_VERSION,
        project_id: graph.project_id().clone(),
        language: policy.language.clone(),
        requirements,
    })
}

/// Builds the S1 vocabulary request for the target Requirements. The provider policy is
/// always the caller's; no provider is preferred.
pub fn build_vocabulary_request(
    graph: &Graph,
    requirement_refs: &[Id],
    policy: &VocabularyPolicy,
    provider_policy: ProviderPolicy,
) -> Result<VocabularyRequest, VocabularyError> {
    let context = context_of(graph, requirement_refs, policy)?;
    let targets: Vec<&Id> = context
        .requirements
        .iter()
        .map(|r| &r.requirement_ref)
        .collect();
    let request = InferenceRequest::new(
        VOCABULARY_STAGE,
        VOCABULARY_TASK_KIND.to_owned(),
        targets.iter().map(|id| (*id).clone()).collect(),
        evidence_of(graph, &targets),
        context.content_hash()?,
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    let vocabulary = VocabularyRequest { request, context };
    vocabulary.validate()?;
    Ok(vocabulary)
}

// ============================================================================ output and results

/// A grounded range of a target Requirement's current statement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyGroundedRange {
    pub requirement_ref: Id,
    pub start: u64,
    pub end: u64,
}

/// One validated vocabulary mention. The range is a statement byte range, not an
/// EvidenceFragment range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyMention {
    pub requirement_ref: Id,
    pub range: LintTextRange,
    pub surface: String,
    pub normalized_key: String,
    pub concept_kind: Option<ConceptKind>,
    pub definition: Option<VocabularyGroundedRange>,
}

/// Every mention of one normalized key with deterministic statistics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyTermCandidate {
    pub normalized_key: String,
    pub language: String,
    pub surface_forms: Vec<String>,
    pub mentions: Vec<VocabularyMention>,
    pub frequency: u64,
    pub requirement_count: u64,
}

/// The number of distinct target Requirements containing both keys, `left_key < right_key`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyCooccurrence {
    pub left_key: String,
    pub right_key: String,
    pub requirement_count: u64,
}

/// One contributing mention of a proposed vocabulary node.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyOriginMention {
    pub requirement_ref: Id,
    pub start: u64,
    pub end: u64,
}

/// Provenance only (`plumb_functional:vocabulary_origin`); no kind, statistics or status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyOrigin {
    pub language: String,
    pub normalized_key: String,
    pub mentions: Vec<VocabularyOriginMention>,
    pub definition: Option<VocabularyGroundedRange>,
}

/// The plumb-lint term context of one Requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocabularyLintContext {
    pub requirement_ref: Id,
    pub context: TermLintContext,
}

/// A deterministic vocabulary conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "conflict", rename_all = "snake_case", deny_unknown_fields)]
pub enum VocabularyConflict {
    AcceptedTermKeyConflict {
        normalized_key: String,
        term_refs: Vec<Id>,
    },
    AcceptedConceptKeyConflict {
        normalized_key: String,
        concept_refs: Vec<Id>,
    },
    ConceptKindConflict {
        normalized_key: String,
        concept_kinds: Vec<ConceptKind>,
    },
    ConceptDefinitionConflict {
        normalized_key: String,
        definitions: Vec<String>,
    },
    AcceptedConceptKindConflict {
        normalized_key: String,
        concept_ref: Id,
        accepted_kind: ConceptKind,
        proposed_kinds: Vec<ConceptKind>,
    },
}

impl VocabularyConflict {
    fn key(&self) -> &str {
        match self {
            VocabularyConflict::AcceptedTermKeyConflict { normalized_key, .. }
            | VocabularyConflict::AcceptedConceptKeyConflict { normalized_key, .. }
            | VocabularyConflict::ConceptKindConflict { normalized_key, .. }
            | VocabularyConflict::ConceptDefinitionConflict { normalized_key, .. }
            | VocabularyConflict::AcceptedConceptKindConflict { normalized_key, .. } => {
                normalized_key
            }
        }
    }

    /// The semantic condition key prefix and the finding message of this conflict.
    fn finding_text(&self) -> (&'static str, String) {
        let key = self.key();
        match self {
            VocabularyConflict::ConceptKindConflict { .. } => (
                "vocabulary_concept_kind_conflict",
                format!("Vocabulary term \"{key}\" has conflicting concept-kind proposals."),
            ),
            VocabularyConflict::AcceptedConceptKindConflict { .. } => (
                "vocabulary_accepted_concept_kind_conflict",
                format!(
                    "Vocabulary term \"{key}\" conflicts with the kind of an accepted Concept."
                ),
            ),
            VocabularyConflict::ConceptDefinitionConflict { .. } => (
                "vocabulary_concept_definition_conflict",
                format!("Vocabulary term \"{key}\" has conflicting grounded Concept definitions."),
            ),
            VocabularyConflict::AcceptedTermKeyConflict { .. } => (
                "vocabulary_accepted_term_key_conflict",
                format!("Vocabulary key \"{key}\" resolves to more than one accepted Term."),
            ),
            VocabularyConflict::AcceptedConceptKeyConflict { .. } => (
                "vocabulary_accepted_concept_key_conflict",
                format!("Vocabulary key \"{key}\" resolves to more than one accepted Concept."),
            ),
        }
    }

    fn order_key(&self) -> (String, String) {
        let payload = serde_json::to_string(self).unwrap_or_default();
        (self.key().to_owned(), payload)
    }
}

/// A deterministic, non-conflict outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case")]
pub enum VocabularyIssue {
    /// No vocabulary inference was supplied; there is no fallback.
    InferenceUnavailable,
}

/// Who owns the Proposed vocabulary nodes and when, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// An already-acquired vocabulary artifact and the caller's provenance ref for it.
#[derive(Debug, Clone)]
pub struct VocabularyInference<'a> {
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

/// Everything one analysis produced; nothing has been applied or persisted.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VocabularyAnalysisResult {
    pub candidates: Vec<VocabularyTermCandidate>,
    pub cooccurrences: Vec<VocabularyCooccurrence>,
    pub lint_contexts: Vec<VocabularyLintContext>,
    pub conflicts: Vec<VocabularyConflict>,
    pub findings: Vec<GeneratedFinding>,
    pub proposals: Vec<Proposal>,
    pub issues: Vec<VocabularyIssue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VocabularyOutput {
    version: u32,
    mentions: Vec<OutputMention>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputMention {
    requirement_ref: Id,
    start: u64,
    end: u64,
    concept_kind: Option<ConceptKind>,
    definition: Option<VocabularyGroundedRange>,
}

fn schema_invalid(reason: impl Into<String>) -> VocabularyError {
    VocabularyError::SchemaInvalid {
        reason: reason.into(),
    }
}

fn compile_schema() -> Result<JSONSchema, VocabularyError> {
    let schema: Value =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| VocabularyError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| VocabularyError::SchemaCompilation {
            reason: e.to_string(),
        })
}

/// The `[start, end)` byte bounds of a valid UTF-8 range of `text`.
fn bounds(text: &str, start: u64, end: u64) -> Option<(usize, usize)> {
    let (s, e) = (usize::try_from(start).ok()?, usize::try_from(end).ok()?);
    (s < e && e <= text.len() && text.is_char_boundary(s) && text.is_char_boundary(e))
        .then_some((s, e))
}

/// Validates the supplied artifact in the contract order and returns the mentions sorted by
/// requirement, start and end, with their grounded definition texts.
fn validated_mentions(
    context: &VocabularyContext,
    request: &InferenceRequest,
    artifact: &InferenceArtifact,
) -> Result<Vec<(VocabularyMention, Option<String>)>, VocabularyError> {
    artifact
        .validate_for(request)
        .map_err(|e| VocabularyError::InvalidInferenceArtifact {
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
    let decoded: VocabularyOutput =
        serde_json::from_value(output.clone()).map_err(|e| schema_invalid(e.to_string()))?;
    if decoded.version != VOCABULARY_OUTPUT_VERSION {
        return Err(schema_invalid(format!(
            "output version {} is not 1",
            decoded.version
        )));
    }
    let mut mentions = decoded.mentions;
    mentions.sort_by(|a, b| {
        (&a.requirement_ref, a.start, a.end).cmp(&(&b.requirement_ref, b.start, b.end))
    });
    for m in &mentions {
        if context.requirement(&m.requirement_ref).is_none() {
            return Err(VocabularyError::UnknownRequirement {
                requirement_ref: m.requirement_ref.clone(),
            });
        }
        if m.concept_kind.is_none() && m.definition.is_some() {
            return Err(schema_invalid("a definition requires a concept kind"));
        }
    }
    let mut keyed = Vec::new();
    for m in &mentions {
        let Some(target) = context.requirement(&m.requirement_ref) else {
            return Err(VocabularyError::UnknownRequirement {
                requirement_ref: m.requirement_ref.clone(),
            });
        };
        let statement = &target.statement;
        let invalid = |reason: &str| VocabularyError::InvalidMention {
            requirement_ref: m.requirement_ref.clone(),
            start: m.start,
            end: m.end,
            reason: reason.to_owned(),
        };
        let (s, e) = bounds(statement, m.start, m.end)
            .ok_or_else(|| invalid("not a valid UTF-8 range of the statement"))?;
        let surface = &statement[s..e];
        let key = normalize_vocabulary_term(surface)
            .ok_or_else(|| invalid("the selected text has no vocabulary token"))?;
        keyed.push((surface.to_owned(), key));
    }
    for pair in mentions.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if a.requirement_ref != b.requirement_ref {
            continue;
        }
        if (a.start, a.end) == (b.start, b.end) {
            return Err(VocabularyError::DuplicateMention {
                requirement_ref: a.requirement_ref.clone(),
                start: a.start,
                end: a.end,
            });
        }
        if b.start < a.end {
            return Err(VocabularyError::OverlappingMention {
                requirement_ref: a.requirement_ref.clone(),
                first_start: a.start,
                first_end: a.end,
                second_start: b.start,
                second_end: b.end,
            });
        }
    }
    let mut out = Vec::new();
    for (m, (surface, normalized_key)) in mentions.into_iter().zip(keyed) {
        let definition_text = match &m.definition {
            None => None,
            Some(range) => {
                let invalid = |reason: &str| VocabularyError::InvalidDefinitionGrounding {
                    requirement_ref: range.requirement_ref.clone(),
                    start: range.start,
                    end: range.end,
                    reason: reason.to_owned(),
                };
                let target = context
                    .requirement(&range.requirement_ref)
                    .ok_or_else(|| invalid("unknown requirement"))?;
                let (s, e) = bounds(&target.statement, range.start, range.end)
                    .ok_or_else(|| invalid("not a valid UTF-8 range of the statement"))?;
                let text = &target.statement[s..e];
                if !is_clean(text) {
                    return Err(invalid(
                        "definition text is empty, padded or contains control characters",
                    ));
                }
                Some(text.to_owned())
            }
        };
        out.push((
            VocabularyMention {
                requirement_ref: m.requirement_ref,
                range: LintTextRange {
                    start: m.start,
                    end: m.end,
                },
                surface,
                normalized_key,
                concept_kind: m.concept_kind,
                definition: m.definition,
            },
            definition_text,
        ));
    }
    Ok(out)
}

// ============================================================================ accepted vocabulary

/// Accepted English Terms and Accepted Concepts indexed by normalized key.
struct AcceptedVocabulary<'g> {
    terms: BTreeMap<String, BTreeSet<&'g Id>>,
    concepts: BTreeMap<String, Vec<(&'g Id, ConceptKind)>>,
}

impl AcceptedVocabulary<'_> {
    fn defined_keys(&self) -> BTreeSet<String> {
        self.terms
            .keys()
            .chain(self.concepts.keys())
            .cloned()
            .collect()
    }

    /// The single Accepted Concept of `key`, when exactly one exists.
    fn single_concept(&self, key: &str) -> Option<(&Id, ConceptKind)> {
        match self.concepts.get(key).map(Vec::as_slice) {
            Some([(id, kind)]) => Some((id, *kind)),
            _ => None,
        }
    }
}

fn accepted_vocabulary(graph: &Graph) -> AcceptedVocabulary<'_> {
    let mut terms: BTreeMap<String, BTreeSet<&Id>> = BTreeMap::new();
    for id in graph.node_ids_by_type(NodeType::Term) {
        let Some(node) = graph
            .node(id)
            .filter(|n| n.status == ElementStatus::Accepted)
        else {
            continue;
        };
        let NodePayload::Term(term) = &node.payload else {
            continue;
        };
        if term.language != PILOT_VOCABULARY_LANGUAGE {
            continue;
        }
        for name in std::iter::once(&term.term).chain(term.aliases.iter().flatten()) {
            if let Some(key) = normalize_vocabulary_term(name) {
                terms.entry(key).or_default().insert(&node.id);
            }
        }
    }
    let mut concepts: BTreeMap<String, Vec<(&Id, ConceptKind)>> = BTreeMap::new();
    for id in graph.node_ids_by_type(NodeType::Concept) {
        let Some(node) = graph
            .node(id)
            .filter(|n| n.status == ElementStatus::Accepted)
        else {
            continue;
        };
        let NodePayload::Concept(concept) = &node.payload else {
            continue;
        };
        if let Some(key) = normalize_vocabulary_term(&concept.name) {
            concepts
                .entry(key)
                .or_default()
                .push((&node.id, concept.concept_kind));
        }
    }
    AcceptedVocabulary { terms, concepts }
}

// ============================================================================ proposals

/// `<node_type>:<first 16 hex of SHA-256(RFC 8785 {project_id, language, normalized_key,
/// node_type})>`.
fn vocabulary_node_id(project_id: &Id, key: &str, node_type: &str) -> Result<Id, CoreError> {
    let body = json!({
        "project_id": project_id,
        "language": PILOT_VOCABULARY_LANGUAGE,
        "normalized_key": key,
        "node_type": node_type,
    });
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("{node_type}:{}", &hex[..16]).parse()
}

struct ProposalContext<'a> {
    graph: &'a Graph,
    base_semantic_hash: &'a Hash,
    derivation_ref: &'a DerivationRef,
    audit: &'a VocabularyAudit,
}

/// The AddNode proposal of a new vocabulary node, or None when the identical node is already
/// pending or already Accepted at its deterministic ID.
fn propose_vocabulary_node(
    context: &ProposalContext<'_>,
    id: Id,
    key: &str,
    payload: NodePayload,
    evidence: Vec<Id>,
    origin: &VocabularyOrigin,
) -> Result<Option<Proposal>, VocabularyError> {
    let extension: ExtensionKey = VOCABULARY_ORIGIN_EXTENSION
        .parse()
        .map_err(invalid_proposal)?;
    let node = Node {
        id: id.clone(),
        revision: 1,
        status: ElementStatus::Proposed,
        payload,
        evidence: evidence.into_iter().map(EvidenceRef::from).collect(),
        derivations: Vec::new(),
        standards: Vec::new(),
        tags: BTreeSet::new(),
        extensions: BTreeMap::from([(
            extension,
            serde_json::to_value(origin).map_err(invalid_proposal)?,
        )]),
        audit: AuditMeta::new(
            context.audit.created_by.clone(),
            context.audit.created_at,
            None,
            None,
        )
        .map_err(invalid_proposal)?,
    };
    node.validate().map_err(invalid_proposal)?;
    if let Some(existing) = context.graph.node(&id) {
        let same_type = existing.payload.node_type() == node.payload.node_type();
        let pending = existing.status == ElementStatus::Proposed
            && existing.payload == node.payload
            && existing.evidence == node.evidence
            && existing.extensions == node.extensions;
        if same_type && (pending || existing.status == ElementStatus::Accepted) {
            return Ok(None);
        }
        return Err(VocabularyError::ExistingVocabularyCandidateConflict {
            node_ref: id,
            normalized_key: key.to_owned(),
        });
    }
    let evidence_refs = node.evidence.clone();
    Proposal::new(
        VOCABULARY_STAGE,
        PatchSet {
            base_semantic_hash: context.base_semantic_hash.clone(),
            patch: SemanticPatch::AddNode { node },
        },
        evidence_refs,
        vec![context.derivation_ref.clone()],
        ProposalMateriality::Semantic,
        AcceptancePolicy::HumanConfirm,
        None,
    )
    .map(Some)
    .map_err(invalid_proposal)
}

fn added_node_id(proposal: &Proposal) -> Option<&Id> {
    match &proposal.patch_set.patch {
        SemanticPatch::AddNode { node } => Some(&node.id),
        _ => None,
    }
}

fn terms_resolved_rule(graph: &Graph) -> Result<RuleMetadata, VocabularyError> {
    let profile =
        load_builtin_software_profile().map_err(|e| VocabularyError::ValidationProfile {
            reason: e.to_string(),
        })?;
    if graph.profile_id() != &profile.profile_id {
        return Err(invalid_input(format!(
            "graph profile {} is not the built-in profile {}",
            graph.profile_id(),
            profile.profile_id
        )));
    }
    profile
        .rules
        .into_iter()
        .find(|r| r.id == TERMS_RESOLVED_RULE)
        .ok_or_else(|| VocabularyError::ValidationProfile {
            reason: format!("profile has no rule {TERMS_RESOLVED_RULE}"),
        })
}

// ============================================================================ analysis

/// Analyzes the request's target Requirements with a supplied vocabulary inference. Absent
/// inference yields only InferenceUnavailable; malformed inference is an error.
pub fn analyze_vocabulary(
    graph: &Graph,
    vocabulary_request: &VocabularyRequest,
    policy: &VocabularyPolicy,
    inference: Option<VocabularyInference<'_>>,
    audit: &VocabularyAudit,
) -> Result<VocabularyAnalysisResult, VocabularyError> {
    vocabulary_request.validate()?;
    let targets: Vec<Id> = vocabulary_request
        .context
        .requirements
        .iter()
        .map(|r| r.requirement_ref.clone())
        .collect();
    if context_of(graph, &targets, policy)? != vocabulary_request.context {
        return Err(invalid_input(
            "graph does not match the vocabulary request context",
        ));
    }
    let target_refs: Vec<&Id> = targets.iter().collect();
    if vocabulary_request.request.evidence_refs != evidence_of(graph, &target_refs) {
        return Err(invalid_input(
            "evidence_refs differ from the target requirements' evidence",
        ));
    }
    let context = &vocabulary_request.context;
    let Some(inference) = inference else {
        return Ok(VocabularyAnalysisResult {
            candidates: Vec::new(),
            cooccurrences: Vec::new(),
            lint_contexts: Vec::new(),
            conflicts: Vec::new(),
            findings: Vec::new(),
            proposals: Vec::new(),
            issues: vec![VocabularyIssue::InferenceUnavailable],
        });
    };
    let mentions = validated_mentions(context, &vocabulary_request.request, inference.artifact)?;
    let accepted = accepted_vocabulary(graph);

    // Candidates and statistics.
    let mut by_key: BTreeMap<String, Vec<(VocabularyMention, Option<String>)>> = BTreeMap::new();
    for (mention, definition) in mentions.iter().cloned() {
        by_key
            .entry(mention.normalized_key.clone())
            .or_default()
            .push((mention, definition));
    }
    let mut candidates = Vec::new();
    let mut keys_by_requirement: BTreeMap<&Id, BTreeSet<&str>> = BTreeMap::new();
    for (key, entries) in &by_key {
        let requirements: BTreeSet<&Id> = entries.iter().map(|(m, _)| &m.requirement_ref).collect();
        for r in &requirements {
            keys_by_requirement.entry(r).or_default().insert(key);
        }
        candidates.push(VocabularyTermCandidate {
            normalized_key: key.clone(),
            language: PILOT_VOCABULARY_LANGUAGE.to_owned(),
            surface_forms: entries
                .iter()
                .map(|(m, _)| m.surface.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            mentions: entries.iter().map(|(m, _)| m.clone()).collect(),
            frequency: entries.len() as u64,
            requirement_count: requirements.len() as u64,
        });
    }
    let mut pairs: BTreeMap<(&str, &str), u64> = BTreeMap::new();
    for keys in keys_by_requirement.values() {
        let keys: Vec<&str> = keys.iter().copied().collect();
        for (i, left) in keys.iter().enumerate() {
            for right in &keys[i + 1..] {
                *pairs.entry((left, right)).or_default() += 1;
            }
        }
    }
    let cooccurrences = pairs
        .into_iter()
        .map(|((l, r), n)| VocabularyCooccurrence {
            left_key: l.to_owned(),
            right_key: r.to_owned(),
            requirement_count: n,
        })
        .collect();

    // plumb-lint term contexts.
    let defined_term_keys = accepted.defined_keys();
    let mut lint_mentions: BTreeMap<&Id, Vec<TermMention>> = BTreeMap::new();
    for (m, _) in &mentions {
        lint_mentions
            .entry(&m.requirement_ref)
            .or_default()
            .push(TermMention {
                range: m.range,
                normalized_key: m.normalized_key.clone(),
            });
    }
    let lint_contexts = lint_mentions
        .into_iter()
        .map(|(requirement_ref, mut mentions)| {
            mentions.sort();
            VocabularyLintContext {
                requirement_ref: requirement_ref.clone(),
                context: TermLintContext {
                    defined_term_keys: defined_term_keys.clone(),
                    mentions,
                },
            }
        })
        .collect();

    // Conflicts, findings and proposals per key.
    let base_semantic_hash = graph.semantic_hash()?;
    let proposal_context = ProposalContext {
        graph,
        base_semantic_hash: &base_semantic_hash,
        derivation_ref: &inference.derivation_ref,
        audit,
    };
    let mut conflicts = Vec::new();
    let mut proposals = Vec::new();
    let mut accepted_mentions: BTreeMap<&str, BTreeSet<Id>> = BTreeMap::new();
    for (key, entries) in &by_key {
        for (m, _) in entries {
            if context
                .requirement(&m.requirement_ref)
                .is_some_and(|r| r.status == ElementStatus::Accepted)
            {
                accepted_mentions
                    .entry(key)
                    .or_default()
                    .insert(m.requirement_ref.clone());
            }
        }
        let term_refs = accepted.terms.get(key);
        let concept_refs = accepted.concepts.get(key);
        let kinds: BTreeSet<ConceptKind> =
            entries.iter().filter_map(|(m, _)| m.concept_kind).collect();
        let definitions: BTreeSet<&String> =
            entries.iter().filter_map(|(_, d)| d.as_ref()).collect();
        let mut key_conflicts = Vec::new();
        if let Some(refs) = term_refs.filter(|r| r.len() > 1) {
            key_conflicts.push(VocabularyConflict::AcceptedTermKeyConflict {
                normalized_key: key.clone(),
                term_refs: refs.iter().map(|id| (*id).clone()).collect(),
            });
        }
        if let Some(refs) = concept_refs.filter(|r| r.len() > 1) {
            let ids: BTreeSet<&Id> = refs.iter().map(|(id, _)| *id).collect();
            key_conflicts.push(VocabularyConflict::AcceptedConceptKeyConflict {
                normalized_key: key.clone(),
                concept_refs: ids.into_iter().cloned().collect(),
            });
        }
        if kinds.len() > 1 {
            key_conflicts.push(VocabularyConflict::ConceptKindConflict {
                normalized_key: key.clone(),
                concept_kinds: kinds.iter().copied().collect(),
            });
        }
        if definitions.len() > 1 {
            key_conflicts.push(VocabularyConflict::ConceptDefinitionConflict {
                normalized_key: key.clone(),
                definitions: definitions.iter().map(|d| (*d).clone()).collect(),
            });
        }
        let single_concept = accepted.single_concept(key);
        if let Some((concept_ref, accepted_kind)) = single_concept {
            if kinds.iter().any(|k| *k != accepted_kind) {
                key_conflicts.push(VocabularyConflict::AcceptedConceptKindConflict {
                    normalized_key: key.clone(),
                    concept_ref: concept_ref.clone(),
                    accepted_kind,
                    proposed_kinds: kinds.iter().copied().collect(),
                });
            }
        }

        let mention_requirements: Vec<&Id> = entries
            .iter()
            .map(|(m, _)| &m.requirement_ref)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let origin_mentions: Vec<VocabularyOriginMention> = entries
            .iter()
            .map(|(m, _)| VocabularyOriginMention {
                requirement_ref: m.requirement_ref.clone(),
                start: m.range.start,
                end: m.range.end,
            })
            .collect();

        if term_refs.is_none() {
            let surfaces: BTreeSet<&String> = entries.iter().map(|(m, _)| &m.surface).collect();
            let aliases: Vec<String> = surfaces
                .into_iter()
                .filter(|s| s.as_str() != key)
                .cloned()
                .collect();
            let term = Term {
                term: key.clone(),
                language: PILOT_VOCABULARY_LANGUAGE.to_owned(),
                definition_ref: single_concept.map(|(id, _)| id.clone()),
                aliases: (!aliases.is_empty()).then_some(aliases),
                status: None,
            };
            let origin = VocabularyOrigin {
                language: PILOT_VOCABULARY_LANGUAGE.to_owned(),
                normalized_key: key.clone(),
                mentions: origin_mentions.clone(),
                definition: None,
            };
            let id = vocabulary_node_id(graph.project_id(), key, "term")?;
            if let Some(p) = propose_vocabulary_node(
                &proposal_context,
                id,
                key,
                NodePayload::Term(term),
                evidence_of(graph, &mention_requirements),
                &origin,
            )? {
                proposals.push(p);
            }
        }

        // Eligible only without an Accepted Concept (single or ambiguous), with exactly one
        // kind and one grounded definition text (so no kind or definition conflict).
        let consensus = match (concept_refs, kinds.len(), definitions.len()) {
            (None, 1, 1) => kinds.first().copied().zip(definitions.first().copied()),
            _ => None,
        };
        if let Some((kind, definition)) = consensus {
            let definition = definition.clone();
            let grounding: BTreeSet<&VocabularyGroundedRange> = entries
                .iter()
                .filter_map(|(m, _)| m.definition.as_ref())
                .collect();
            let mut evidence_requirements = mention_requirements.clone();
            evidence_requirements.extend(grounding.iter().map(|g| &g.requirement_ref));
            let origin = VocabularyOrigin {
                language: PILOT_VOCABULARY_LANGUAGE.to_owned(),
                normalized_key: key.clone(),
                mentions: origin_mentions,
                definition: grounding.iter().next().map(|g| (*g).clone()),
            };
            let concept = Concept {
                name: key.clone(),
                definition,
                concept_kind: kind,
            };
            let id = vocabulary_node_id(graph.project_id(), key, "concept")?;
            if let Some(p) = propose_vocabulary_node(
                &proposal_context,
                id,
                key,
                NodePayload::Concept(concept),
                evidence_of(graph, &evidence_requirements),
                &origin,
            )? {
                proposals.push(p);
            }
        }
        conflicts.extend(key_conflicts);
    }
    conflicts.sort_by_key(|c| c.order_key());

    // F1 finding material for conflicts touching Accepted Requirements.
    let mut findings = BTreeMap::new();
    let needs_rule = conflicts.iter().any(|c| {
        accepted_mentions
            .get(c.key())
            .is_some_and(|t| !t.is_empty())
    });
    let rule = if needs_rule {
        Some(terms_resolved_rule(graph)?)
    } else {
        None
    };
    for conflict in &conflicts {
        let (Some(rule), Some(targets)) = (
            &rule,
            accepted_mentions
                .get(conflict.key())
                .filter(|t| !t.is_empty()),
        ) else {
            continue;
        };
        let (prefix, message) = conflict.finding_text();
        let condition = format!("{prefix}:{}", conflict.key());
        let targets: Vec<Id> = targets.iter().cloned().collect();
        let finding = GeneratedFinding::for_violation(
            rule,
            rule.severity,
            &ViolationFacts {
                targets: &targets,
                semantic_condition_key: &condition,
                message: &message,
                suggested_resolution: Some(FINDING_RESOLUTION),
            },
            None,
        )
        .map_err(|e| VocabularyError::GeneratedFinding {
            reason: e.to_string(),
        })?;
        findings.insert(finding.id.clone(), finding);
    }

    proposals.sort_by(|a, b| added_node_id(a).cmp(&added_node_id(b)));
    Ok(VocabularyAnalysisResult {
        candidates,
        cooccurrences,
        lint_contexts,
        conflicts,
        findings: findings.into_values().collect(),
        proposals,
        issues: Vec::new(),
    })
}
