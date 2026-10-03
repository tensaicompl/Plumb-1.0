//! S1 requirement segmentation (plan S0.4; compiler architecture §7).
//!
//! Deterministic request construction, consumption of an already-acquired inference artifact,
//! segmentation post-validation, the deterministic fallback and its recovery provenance. Nothing
//! here calls a provider, reads a clock or persists anything: acquisition happens outside, and
//! evaluation receives a supplied `InferenceArtifact` or its absence.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_psg::{
    is_baseline, AuditMeta, DerivationKind, DerivationRecord, ElementStatus, ExtensionKey, Node,
    NodePayload,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};
use thiserror::Error;

use crate::fallback::fallback_segments;
use crate::source::{FragmentKind, FragmentMetadata, UnknownImportValue, FRAGMENT_EXTENSION};

/// Version of [`SegmentationContext`].
pub const SEGMENTATION_CONTEXT_VERSION: u32 = 1;

/// Version of the segmentation inference output (`s1-segmentation.schema.json`).
pub const SEGMENTATION_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of requirement segmentation.
pub const SEGMENTATION_TASK_KIND: &str = "requirement_segmentation";

/// Code of [`SegmentationError::IntakeUncovered`].
pub const E_INTAKE_UNCOVERED: &str = "E_INTAKE_UNCOVERED";

/// The exact prompt-template bytes; their Generic hash is the request's `prompt_template_hash`.
const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s1-segment-requirements.md");

/// The exact schema source; its byte hash is the request's `schema_hash`.
const SCHEMA_SOURCE: &str = include_str!("../../../schemas/inference/s1-segmentation.schema.json");

const SEGMENTATION_STAGE: StageId = StageId::S1;

// ============================================================================ errors

/// Why segmentation could not produce a result.
#[derive(Debug, Error)]
pub enum SegmentationError {
    /// The supplied fragments, context or request are not valid segmentation input.
    #[error("invalid segmentation input{}: {reason}", element_suffix(.element_ref))]
    InvalidInput {
        element_ref: Option<Id>,
        reason: String,
    },
    /// A segment references a fragment that is not part of the request.
    #[error("segment references unknown fragment {fragment_ref}")]
    UnknownFragment { fragment_ref: Id },
    /// A segment range is empty, out of bounds or not on UTF-8 character boundaries.
    #[error("invalid segment range {start}..{end} in {fragment_ref}: {reason}")]
    InvalidRange {
        fragment_ref: Id,
        start: u64,
        end: u64,
        reason: String,
    },
    /// Two segments of one fragment cover the same bytes.
    #[error("segments {first_start}..{first_end} and {second_start}..{second_end} of {fragment_ref} overlap")]
    Overlap {
        fragment_ref: Id,
        first_start: u64,
        first_end: u64,
        second_start: u64,
        second_end: u64,
    },
    /// A segment candidate has a wrong ID or invalid flags.
    #[error("invalid segment candidate {candidate_ref}: {reason}")]
    InvalidCandidate { candidate_ref: Id, reason: String },
    /// Bytes of requested fragments are covered by no segment.
    #[error("{} uncovered evidence range(s)", .gaps.len())]
    IntakeUncovered { gaps: Vec<SegmentGap> },
    /// The InferenceRequest could not be constructed or is invalid.
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    /// The segmentation schema does not compile.
    #[error("segmentation schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    /// The fallback recovery derivation could not be built.
    #[error("invalid fallback derivation: {reason}")]
    InvalidDerivation { reason: String },
    /// Canonicalization or hashing failed.
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn element_suffix(element_ref: &Option<Id>) -> String {
    element_ref
        .as_ref()
        .map(|id| format!(" ({id})"))
        .unwrap_or_default()
}

impl SegmentationError {
    /// The stable error code, where one is defined.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            SegmentationError::IntakeUncovered { .. } => Some(E_INTAKE_UNCOVERED),
            _ => None,
        }
    }
}

fn invalid_input(element_ref: Option<&Id>, reason: impl Into<String>) -> SegmentationError {
    SegmentationError::InvalidInput {
        element_ref: element_ref.cloned(),
        reason: reason.into(),
    }
}

// ============================================================================ vocabularies

closed_vocabulary! {
    /// How a segment range is classified.
    SegmentClassification, 2 {
        RequirementCandidate => "requirement_candidate",
        NonRequirement => "non_requirement",
    }
}

closed_vocabulary! {
    /// A flag on a segment candidate. `unassigned_text`: the deterministic fallback did not
    /// identify the text as a requirement candidate and retained it as non-requirement to
    /// preserve coverage.
    SegmentFlag, 1 {
        UnassignedText => "unassigned_text",
    }
}

closed_vocabulary! {
    /// Which path produced the candidates.
    SegmentationMode, 2 {
        Inference => "inference",
        Fallback => "fallback",
    }
}

closed_vocabulary! {
    /// Why the deterministic fallback was used.
    FallbackReason, 3 {
        MissingInference => "missing_inference",
        InvalidInferenceArtifact => "invalid_inference_artifact",
        SchemaInvalid => "schema_invalid",
    }
}

// ============================================================================ context and request

/// The exact inference context: fragment IDs and their exact extracted text, sorted by ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentationContext {
    pub version: u32,
    pub fragments: Vec<SegmentationContextFragment>,
}

/// One fragment of a [`SegmentationContext`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentationContextFragment {
    pub fragment_ref: Id,
    pub text: String,
}

impl SegmentationContext {
    /// Version 1, at least one fragment, strictly sorted unique fragment refs, non-empty text.
    pub fn validate(&self) -> Result<(), SegmentationError> {
        if self.version != SEGMENTATION_CONTEXT_VERSION {
            return Err(invalid_input(
                None,
                format!("context version {} is not 1", self.version),
            ));
        }
        if self.fragments.is_empty() {
            return Err(invalid_input(None, "context has no evidence fragments"));
        }
        for pair in self.fragments.windows(2) {
            if pair[0].fragment_ref >= pair[1].fragment_ref {
                return Err(invalid_input(
                    Some(&pair[1].fragment_ref),
                    "context fragments are not strictly sorted by fragment_ref",
                ));
            }
        }
        if let Some(fragment) = self.fragments.iter().find(|f| f.text.is_empty()) {
            return Err(invalid_input(
                Some(&fragment.fragment_ref),
                "fragment text is empty",
            ));
        }
        Ok(())
    }

    /// Generic SHA-256 of the RFC 8785 canonical JSON of the validated context.
    pub fn content_hash(&self) -> Result<Hash, SegmentationError> {
        self.validate()?;
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn text_of(&self, fragment_ref: &Id) -> Option<&str> {
        self.fragments
            .binary_search_by(|f| f.fragment_ref.cmp(fragment_ref))
            .ok()
            .map(|i| self.fragments[i].text.as_str())
    }
}

/// A segmentation `InferenceRequest` together with the exact context it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentationRequest {
    pub request: InferenceRequest,
    pub context: SegmentationContext,
}

impl SegmentationRequest {
    /// Validates both parts and their binding.
    pub fn new(
        request: InferenceRequest,
        context: SegmentationContext,
    ) -> Result<SegmentationRequest, SegmentationError> {
        let segmentation = SegmentationRequest { request, context };
        segmentation.validate()?;
        Ok(segmentation)
    }

    /// The request is a valid S1 requirement_segmentation request bound to this context, the
    /// committed prompt and schema, with no input refs and exactly the context fragments as
    /// evidence refs.
    pub fn validate(&self) -> Result<(), SegmentationError> {
        self.request.validate()?;
        let request = &self.request;
        let context_hash = self.context.content_hash()?;
        let evidence_refs: Vec<&Id> = self
            .context
            .fragments
            .iter()
            .map(|f| &f.fragment_ref)
            .collect();
        let mismatch = if request.stage != SEGMENTATION_STAGE {
            Some("stage is not S1")
        } else if request.task_kind != SEGMENTATION_TASK_KIND {
            Some("task_kind is not requirement_segmentation")
        } else if !request.input_refs.is_empty() {
            Some("input_refs is not empty")
        } else if request.evidence_refs.iter().collect::<Vec<_>>() != evidence_refs {
            Some("evidence_refs differ from the context fragments")
        } else if request.context_hash != context_hash {
            Some("context_hash differs from the context hash")
        } else if request.prompt_template_hash != prompt_template_hash() {
            Some("prompt_template_hash differs from the segmentation prompt")
        } else if request.schema_hash != schema_hash() {
            Some("schema_hash differs from the segmentation schema")
        } else {
            None
        };
        match mismatch {
            Some(reason) => Err(invalid_input(None, reason)),
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

/// A validated input fragment: its exact text and importer kind.
struct InputFragment<'n> {
    text: &'n str,
    kind: FragmentKind,
}

/// Validates the input nodes and indexes them by ID. Input is never repaired.
fn input_fragments(
    fragments: &[Node],
) -> Result<BTreeMap<&Id, InputFragment<'_>>, SegmentationError> {
    let key: ExtensionKey = FRAGMENT_EXTENSION
        .parse()
        .map_err(|e: plumb_psg::InvalidExtensionKey| invalid_input(None, e.to_string()))?;
    let mut inputs = BTreeMap::new();
    for node in fragments {
        let id = Some(&node.id);
        node.validate()
            .map_err(|e| invalid_input(id, format!("invalid node: {e}")))?;
        let NodePayload::EvidenceFragment(fragment) = &node.payload else {
            return Err(invalid_input(id, "node is not an EvidenceFragment"));
        };
        if !is_baseline(node.status) {
            return Err(invalid_input(
                id,
                format!(
                    "fragment status {:?} is not baseline-participating",
                    node.status
                ),
            ));
        }
        let text = match fragment.extracted_text.as_deref() {
            None => return Err(invalid_input(id, "fragment has no extracted_text")),
            Some("") => return Err(invalid_input(id, "fragment extracted_text is empty")),
            Some(text) => text,
        };
        let value = node
            .extensions
            .get(&key)
            .ok_or_else(|| invalid_input(id, format!("missing {FRAGMENT_EXTENSION}")))?;
        let metadata: FragmentMetadata = serde_json::from_value(value.clone())
            .map_err(|e| invalid_input(id, format!("malformed {FRAGMENT_EXTENSION}: {e}")))?;
        let input = InputFragment {
            text,
            kind: metadata.kind,
        };
        if inputs.insert(&node.id, input).is_some() {
            return Err(invalid_input(id, "duplicate fragment ID"));
        }
    }
    Ok(inputs)
}

fn context_of(inputs: &BTreeMap<&Id, InputFragment<'_>>) -> SegmentationContext {
    SegmentationContext {
        version: SEGMENTATION_CONTEXT_VERSION,
        fragments: inputs
            .iter()
            .map(|(id, input)| SegmentationContextFragment {
                fragment_ref: (*id).clone(),
                text: input.text.to_owned(),
            })
            .collect(),
    }
}

/// Builds the S1 segmentation request for baseline EvidenceFragment nodes. The provider policy
/// is always the caller's; no provider is preferred.
pub fn build_segmentation_request(
    fragments: &[Node],
    provider_policy: ProviderPolicy,
) -> Result<SegmentationRequest, SegmentationError> {
    let context = context_of(&input_fragments(fragments)?);
    let request = InferenceRequest::new(
        SEGMENTATION_STAGE,
        SEGMENTATION_TASK_KIND.to_owned(),
        Vec::new(),
        context
            .fragments
            .iter()
            .map(|f| f.fragment_ref.clone())
            .collect(),
        context.content_hash()?,
        prompt_template_hash(),
        schema_hash(),
        provider_policy,
    )?;
    SegmentationRequest::new(request, context)
}

// ============================================================================ candidates

/// One classified range of one EvidenceFragment's exact extracted text. `start`/`end` are
/// zero-based, end-exclusive UTF-8 byte offsets into `EvidenceFragment.extracted_text`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentCandidate {
    pub id: Id,
    pub fragment_ref: Id,
    pub start: u64,
    pub end: u64,
    pub classification: SegmentClassification,
    pub flags: Vec<SegmentFlag>,
}

impl SegmentCandidate {
    pub(crate) fn new(
        fragment_ref: Id,
        start: u64,
        end: u64,
        classification: SegmentClassification,
        flags: Vec<SegmentFlag>,
    ) -> Result<SegmentCandidate, SegmentationError> {
        Ok(SegmentCandidate {
            id: candidate_id(&fragment_ref, start, end, classification)?,
            fragment_ref,
            start,
            end,
            classification,
            flags,
        })
    }

    /// Exact result order: fragment_ref, start, end, classification wire value.
    fn order_key(&self) -> (&Id, u64, u64, &'static str) {
        (
            &self.fragment_ref,
            self.start,
            self.end,
            self.classification.as_str(),
        )
    }
}

/// `seg:<first 16 hex of SHA-256(RFC 8785 {fragment_ref, start, end, classification})>`.
fn candidate_id(
    fragment_ref: &Id,
    start: u64,
    end: u64,
    classification: SegmentClassification,
) -> Result<Id, CoreError> {
    let body = json!({
        "fragment_ref": fragment_ref,
        "start": start,
        "end": end,
        "classification": classification.as_str(),
    });
    short_id("seg", &body)
}

fn short_id(prefix: &str, body: &Value) -> Result<Id, CoreError> {
    let digest = Hash::content_sha256(&to_canonical_json(body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("{prefix}:{}", &hex[..16]).parse()
}

/// A byte range of a requested fragment covered by no segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentGap {
    pub fragment_ref: Id,
    pub start: u64,
    pub end: u64,
}

/// The one partition validator shared by the inference and fallback paths. Sorts `candidates`
/// into result order, then checks, in order: every fragment exists in the context; every range
/// and candidate is valid; no two candidates of a fragment overlap; no byte is uncovered.
fn validate_partition(
    context: &SegmentationContext,
    candidates: &mut [SegmentCandidate],
) -> Result<(), SegmentationError> {
    candidates.sort_by(|a, b| a.order_key().cmp(&b.order_key()));
    for candidate in candidates.iter() {
        if context.text_of(&candidate.fragment_ref).is_none() {
            return Err(SegmentationError::UnknownFragment {
                fragment_ref: candidate.fragment_ref.clone(),
            });
        }
    }
    for candidate in candidates.iter() {
        let text = context.text_of(&candidate.fragment_ref).unwrap_or_default();
        validate_range(text, candidate)?;
        validate_candidate(candidate)?;
    }
    for pair in candidates.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if a.fragment_ref == b.fragment_ref && b.start < a.end {
            return Err(SegmentationError::Overlap {
                fragment_ref: a.fragment_ref.clone(),
                first_start: a.start,
                first_end: a.end,
                second_start: b.start,
                second_end: b.end,
            });
        }
    }
    let mut gaps = Vec::new();
    for fragment in &context.fragments {
        let mut covered = 0u64;
        for candidate in candidates
            .iter()
            .filter(|c| c.fragment_ref == fragment.fragment_ref)
        {
            if candidate.start > covered {
                gaps.push(gap(&fragment.fragment_ref, covered, candidate.start));
            }
            covered = candidate.end;
        }
        let len = fragment.text.len() as u64;
        if covered < len {
            gaps.push(gap(&fragment.fragment_ref, covered, len));
        }
    }
    if gaps.is_empty() {
        Ok(())
    } else {
        Err(SegmentationError::IntakeUncovered { gaps })
    }
}

fn gap(fragment_ref: &Id, start: u64, end: u64) -> SegmentGap {
    SegmentGap {
        fragment_ref: fragment_ref.clone(),
        start,
        end,
    }
}

fn validate_range(text: &str, candidate: &SegmentCandidate) -> Result<(), SegmentationError> {
    let invalid = |reason: &str| SegmentationError::InvalidRange {
        fragment_ref: candidate.fragment_ref.clone(),
        start: candidate.start,
        end: candidate.end,
        reason: reason.to_owned(),
    };
    if candidate.start >= candidate.end {
        return Err(invalid("start is not before end"));
    }
    let (Ok(start), Ok(end)) = (
        usize::try_from(candidate.start),
        usize::try_from(candidate.end),
    ) else {
        return Err(invalid("offset exceeds the fragment text"));
    };
    if end > text.len() {
        return Err(invalid("end exceeds the fragment text byte length"));
    }
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return Err(invalid("offset is not on a UTF-8 character boundary"));
    }
    Ok(())
}

fn validate_candidate(candidate: &SegmentCandidate) -> Result<(), SegmentationError> {
    let invalid = |reason: &str| SegmentationError::InvalidCandidate {
        candidate_ref: candidate.id.clone(),
        reason: reason.to_owned(),
    };
    let expected = candidate_id(
        &candidate.fragment_ref,
        candidate.start,
        candidate.end,
        candidate.classification,
    )?;
    if candidate.id != expected {
        return Err(invalid("id differs from the recomputed candidate ID"));
    }
    if candidate.flags.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid("flags are not sorted and unique"));
    }
    if candidate.flags.contains(&SegmentFlag::UnassignedText)
        && candidate.classification != SegmentClassification::NonRequirement
    {
        return Err(invalid("unassigned_text on a requirement candidate"));
    }
    Ok(())
}

// ============================================================================ inference output

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentationInferenceOutput {
    version: u32,
    segments: Vec<SegmentationInferenceSegment>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentationInferenceSegment {
    fragment_ref: Id,
    start: u64,
    end: u64,
    classification: SegmentClassification,
}

fn compile_schema() -> Result<JSONSchema, SegmentationError> {
    let schema: Value =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| SegmentationError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| SegmentationError::SchemaCompilation {
            reason: e.to_string(),
        })
}

/// The usable segments of a supplied artifact, or why the fallback is used instead.
fn usable_segments(
    request: &InferenceRequest,
    inference: Option<&InferenceArtifact>,
) -> Result<Result<Vec<SegmentationInferenceSegment>, FallbackReason>, SegmentationError> {
    let Some(artifact) = inference else {
        return Ok(Err(FallbackReason::MissingInference));
    };
    if artifact.validate_for(request).is_err() {
        return Ok(Err(FallbackReason::InvalidInferenceArtifact));
    }
    let output = artifact.validated_output.as_value();
    if !compile_schema()?.is_valid(output) {
        return Ok(Err(FallbackReason::SchemaInvalid));
    }
    Ok(
        match serde_json::from_value::<SegmentationInferenceOutput>(output.clone()) {
            Ok(decoded) if decoded.version == SEGMENTATION_OUTPUT_VERSION => Ok(decoded.segments),
            _ => Err(FallbackReason::SchemaInvalid),
        },
    )
}

// ============================================================================ evaluation

/// Who will own the fallback provenance and when, supplied by the caller. `created_by` is the
/// Agent ID the caller materializes; segmentation creates no Agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentationAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// The candidates of one segmentation. Inference mode carries no fallback reason or
/// derivation; fallback mode carries both. Nothing has been persisted or added to a graph.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentationResult {
    pub candidates: Vec<SegmentCandidate>,
    pub mode: SegmentationMode,
    pub fallback_reason: Option<FallbackReason>,
    pub fallback_derivation: Option<Node>,
}

/// Segments the request's fragments from a supplied, already-acquired inference artifact, or
/// with the deterministic fallback when it is absent, invalid for the request or schema-invalid.
/// Schema-valid output that does not partition every fragment exactly is an error, never
/// replaced by the fallback.
pub fn evaluate_segmentation(
    segmentation_request: &SegmentationRequest,
    fragments: &[Node],
    inference: Option<&InferenceArtifact>,
    audit: &SegmentationAudit,
) -> Result<SegmentationResult, SegmentationError> {
    segmentation_request.validate()?;
    let inputs = input_fragments(fragments)?;
    if context_of(&inputs) != segmentation_request.context {
        return Err(invalid_input(
            None,
            "fragments do not match the segmentation request context",
        ));
    }
    let context = &segmentation_request.context;
    match usable_segments(&segmentation_request.request, inference)? {
        Ok(segments) => {
            let mut candidates = segments
                .into_iter()
                .map(|s| {
                    SegmentCandidate::new(
                        s.fragment_ref,
                        s.start,
                        s.end,
                        s.classification,
                        Vec::new(),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            validate_partition(context, &mut candidates)?;
            Ok(SegmentationResult {
                candidates,
                mode: SegmentationMode::Inference,
                fallback_reason: None,
                fallback_derivation: None,
            })
        }
        Err(reason) => {
            let mut candidates = Vec::new();
            for (id, input) in &inputs {
                candidates.extend(fallback_segments(id, input.text, input.kind)?);
            }
            validate_partition(context, &mut candidates)?;
            let derivation = recovery_derivation(segmentation_request, &candidates, audit)?;
            Ok(SegmentationResult {
                candidates,
                mode: SegmentationMode::Fallback,
                fallback_reason: Some(reason),
                fallback_derivation: Some(derivation),
            })
        }
    }
}

/// The detached `recovery` DerivationRecord node of a fallback segmentation.
fn recovery_derivation(
    segmentation_request: &SegmentationRequest,
    candidates: &[SegmentCandidate],
    audit: &SegmentationAudit,
) -> Result<Node, SegmentationError> {
    let invalid = |reason: String| SegmentationError::InvalidDerivation { reason };
    let input_refs: BTreeSet<String> = segmentation_request
        .context
        .fragments
        .iter()
        .map(|f| f.fragment_ref.to_string())
        .chain([segmentation_request.request.id.to_string()])
        .collect();
    let output_refs: BTreeSet<String> = candidates.iter().map(|c| c.id.to_string()).collect();
    let mut record = DerivationRecord {
        // Placeholder replaced below; the identity input excludes `id`.
        id: "drv:0000000000000000".parse()?,
        kind: DerivationKind::Recovery,
        stage: SEGMENTATION_STAGE.as_str().to_owned(),
        input_refs: input_refs.into_iter().collect(),
        output_refs: output_refs.into_iter().collect(),
        created_at: audit.created_at,
        provider: None,
        model: None,
        prompt_template_hash: None,
        schema_hash: None,
        context_hash: None,
        parameters: None,
        raw_response_hash: None,
        validated_output_hash: None,
    };
    let mut body = serde_json::to_value(&record).map_err(|e| invalid(e.to_string()))?;
    if let Value::Object(map) = &mut body {
        map.remove("id");
    }
    record.id = short_id("drv", &body)?;
    record.validate().map_err(|e| invalid(e.to_string()))?;
    let node = Node {
        id: record.id.clone(),
        revision: 1,
        status: ElementStatus::Accepted,
        payload: NodePayload::DerivationRecord(record),
        evidence: Vec::new(),
        derivations: Vec::new(),
        standards: Vec::new(),
        tags: BTreeSet::new(),
        extensions: BTreeMap::new(),
        audit: AuditMeta::new(audit.created_by.clone(), audit.created_at, None, None)
            .map_err(|e| invalid(e.to_string()))?,
    };
    node.validate().map_err(|e| invalid(e.to_string()))?;
    Ok(node)
}
