//! Pilot data classification (plan S2.3; compiler architecture §8).
//!
//! Accepted Attributes are classified as `pii`, `financial` or `confidential`. A versioned
//! deterministic dictionary, matched by exact equality of the S1.5-normalized Attribute name,
//! yields AUTO_DERIVATION proposals; a supplied, already-acquired `data_classification`
//! inference may classify the remaining Attributes through HUMAN_CONFIRM proposals. Both
//! change only `Attribute.data_classification` through `ReplacePayload`. `None` stays
//! unresolved and is never defaulted, `"none"` is never stored, existing classifications are
//! never overwritten, and no Finding or Question is created. Nothing here mutates a graph,
//! calls a provider, reads a clock or persists anything; `apply_patch` is used solely to
//! dry-validate proposals in memory.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_patch::{
    apply_patch, AcceptancePolicy, ElementPrecondition, PatchSet, Proposal, ProposalMateriality,
    SemanticPatch,
};
use plumb_psg::{
    node_element_hash, Attribute, DerivationRef, ElementStatus, Graph, Node, NodePayload, NodeType,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::vocabulary::normalize_vocabulary_term;

/// Version of [`DataClassContext`].
pub const DATA_CLASS_CONTEXT_VERSION: u32 = 1;

/// Version of the data-classification inference output.
pub const DATA_CLASS_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of data classification.
pub const DATA_CLASS_TASK_KIND: &str = "data_classification";

/// Version of [`DATA_CLASS_DICTIONARY`]; any change to its aliases or mapping increments it.
pub const DATA_CLASS_DICTIONARY_VERSION: u32 = 1;

/// Dictionary v1: pre-normalized full-name aliases and their classification. Matching is
/// exact equality with the S1.5-normalized Attribute name; there is no confidential alias.
pub const DATA_CLASS_DICTIONARY: &[(&str, PilotDataClass)] = &[
    ("e mail", PilotDataClass::Pii),
    ("e mail address", PilotDataClass::Pii),
    ("email", PilotDataClass::Pii),
    ("email address", PilotDataClass::Pii),
    ("birth date", PilotDataClass::Pii),
    ("date of birth", PilotDataClass::Pii),
    ("dob", PilotDataClass::Pii),
    ("salary", PilotDataClass::Financial),
    ("iban", PilotDataClass::Financial),
    (
        "international bank account number",
        PilotDataClass::Financial,
    ),
];

const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s2-data-classification.md");
const SCHEMA_SOURCE: &str =
    include_str!("../../../schemas/inference/s2-data-classification.schema.json");
const DATA_CLASS_STAGE: StageId = StageId::S2;

// ============================================================================ errors

/// Why data classification could not run. Malformed supplied inference is an error, never
/// treated as absent inference.
#[derive(Debug, Error)]
pub enum DataClassError {
    #[error("invalid data-classification input: {reason}")]
    InvalidInput { reason: String },
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    #[error("invalid data-classification inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error("data-classification schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    #[error("data-classification output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("unknown attribute {attribute_ref}")]
    UnknownAttributeRef { attribute_ref: Id },
    #[error("attribute {attribute_ref} is classified by the dictionary")]
    DictionaryOwnedAttribute { attribute_ref: Id },
    #[error("duplicate classification of {attribute_ref}")]
    DuplicateCandidate { attribute_ref: Id },
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_input(reason: impl Into<String>) -> DataClassError {
    DataClassError::InvalidInput {
        reason: reason.into(),
    }
}

fn invalid_proposal(e: impl ToString) -> DataClassError {
    DataClassError::InvalidProposal {
        reason: e.to_string(),
    }
}

fn schema_invalid(reason: impl Into<String>) -> DataClassError {
    DataClassError::SchemaInvalid {
        reason: reason.into(),
    }
}

// ============================================================================ pilot classes

/// The three pilot classifications of profile plumb:software:2026.1. An S2.3 helper; the
/// core field stays `Option<String>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PilotDataClass {
    Pii,
    Financial,
    Confidential,
}

impl PilotDataClass {
    /// Every pilot classification.
    pub const ALL: [PilotDataClass; 3] = [
        PilotDataClass::Pii,
        PilotDataClass::Financial,
        PilotDataClass::Confidential,
    ];

    /// The exact stored string.
    pub fn as_str(self) -> &'static str {
        match self {
            PilotDataClass::Pii => "pii",
            PilotDataClass::Financial => "financial",
            PilotDataClass::Confidential => "confidential",
        }
    }

    /// Exactly one of the three stored strings; no case folding or trimming.
    pub fn parse(value: &str) -> Option<PilotDataClass> {
        PilotDataClass::ALL
            .into_iter()
            .find(|c| c.as_str() == value)
    }
}

/// The dictionary classification of an Attribute name, if its S1.5-normalized form is an
/// alias.
pub fn dictionary_classification(name: &str) -> Option<(String, PilotDataClass)> {
    let key = normalize_vocabulary_term(name)?;
    DATA_CLASS_DICTIONARY
        .iter()
        .find(|(alias, _)| *alias == key)
        .map(|(_, class)| (key, *class))
}

// ============================================================================ context and request

/// One Accepted Attribute with its full semantics except the current classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataClassAttributeContext {
    pub attribute_ref: Id,
    pub name: String,
    pub value_type: String,
    pub nullable: bool,
    pub unit: Option<String>,
    pub precision: Option<u32>,
    pub enum_values: Option<Vec<String>>,
    pub dictionary_classification: Option<PilotDataClass>,
}

/// The exact data-classification context: all and only Accepted Attributes, strictly sorted.
/// The current `data_classification` is deliberately absent, so applying a classification
/// proposal does not stale the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataClassContext {
    pub version: u32,
    pub dictionary_version: u32,
    pub project_id: Id,
    pub attributes: Vec<DataClassAttributeContext>,
}

impl DataClassContext {
    /// Versions 1 and strictly sorted attributes.
    pub fn validate(&self) -> Result<(), DataClassError> {
        if self.version != DATA_CLASS_CONTEXT_VERSION
            || self.dictionary_version != DATA_CLASS_DICTIONARY_VERSION
        {
            return Err(invalid_input("unsupported context or dictionary version"));
        }
        if self
            .attributes
            .windows(2)
            .any(|p| p[0].attribute_ref >= p[1].attribute_ref)
        {
            return Err(invalid_input("context attributes are not strictly sorted"));
        }
        Ok(())
    }

    /// Generic SHA-256 of the RFC 8785 canonical JSON of the validated context.
    pub fn content_hash(&self) -> Result<Hash, DataClassError> {
        self.validate()?;
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn attribute(&self, id: &Id) -> Option<&DataClassAttributeContext> {
        self.attributes
            .binary_search_by(|a| a.attribute_ref.cmp(id))
            .ok()
            .map(|i| &self.attributes[i])
    }

    fn input_refs(&self) -> Vec<Id> {
        self.attributes
            .iter()
            .map(|a| a.attribute_ref.clone())
            .collect()
    }
}

/// A data-classification `InferenceRequest` together with the exact context it was built
/// from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataClassRequest {
    pub request: InferenceRequest,
    pub context: DataClassContext,
}

impl DataClassRequest {
    /// The request is a valid S2 data_classification request bound to this context, the
    /// committed prompt and schema, with exactly the Attribute IDs as input refs. The evidence
    /// refs depend on the graph and are checked by [`analyze_data_classification`].
    pub fn validate(&self) -> Result<(), DataClassError> {
        let request = &self.request;
        request
            .validate()
            .map_err(|e| invalid_input(format!("invalid inference request: {e}")))?;
        let context_hash = self.context.content_hash()?;
        let mismatch = if request.stage != DATA_CLASS_STAGE {
            Some("stage is not S2")
        } else if request.task_kind != DATA_CLASS_TASK_KIND {
            Some("task_kind is not data_classification")
        } else if request.input_refs != self.context.input_refs() {
            Some("input_refs differ from the Accepted Attributes")
        } else if request.context_hash != context_hash {
            Some("context_hash differs from the context hash")
        } else if request.prompt_template_hash != Hash::content_sha256(PROMPT_TEMPLATE) {
            Some("prompt_template_hash differs from the data-classification prompt")
        } else if request.schema_hash != Hash::content_sha256(SCHEMA_SOURCE.as_bytes()) {
            Some("schema_hash differs from the data-classification schema")
        } else {
            None
        };
        match mismatch {
            Some(reason) => Err(invalid_input(reason)),
            None => Ok(()),
        }
    }
}

/// Every Accepted Attribute node with its payload, sorted by ID.
fn accepted_attributes(graph: &Graph) -> Vec<(&Node, &Attribute)> {
    graph
        .node_ids_by_type(NodeType::Attribute)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| n.status == ElementStatus::Accepted)
        .filter_map(|n| match &n.payload {
            NodePayload::Attribute(a) => Some((n, a)),
            _ => None,
        })
        .collect()
}

fn context_of(graph: &Graph) -> DataClassContext {
    DataClassContext {
        version: DATA_CLASS_CONTEXT_VERSION,
        dictionary_version: DATA_CLASS_DICTIONARY_VERSION,
        project_id: graph.project_id().clone(),
        attributes: accepted_attributes(graph)
            .into_iter()
            .map(|(node, a)| DataClassAttributeContext {
                attribute_ref: node.id.clone(),
                name: a.name.clone(),
                value_type: a.value_type.clone(),
                nullable: a.nullable,
                unit: a.unit.clone(),
                precision: a.precision,
                enum_values: a.enum_values.clone(),
                dictionary_classification: dictionary_classification(&a.name).map(|(_, c)| c),
            })
            .collect(),
    }
}

/// The sorted unique union of `Node.evidence` of the context Attributes only.
fn context_evidence(graph: &Graph, context: &DataClassContext) -> Vec<Id> {
    let refs: BTreeSet<Id> = context
        .attributes
        .iter()
        .filter_map(|a| graph.node(&a.attribute_ref))
        .flat_map(|n| n.evidence.iter().map(|e| e.as_id().clone()))
        .collect();
    refs.into_iter().collect()
}

/// Builds the S2 data-classification request over all Accepted Attributes. The provider
/// policy is always the caller's.
pub fn build_data_class_request(
    graph: &Graph,
    provider_policy: ProviderPolicy,
) -> Result<DataClassRequest, DataClassError> {
    let context = context_of(graph);
    let request = InferenceRequest::new(
        DATA_CLASS_STAGE,
        DATA_CLASS_TASK_KIND.to_owned(),
        context.input_refs(),
        context_evidence(graph, &context),
        context.content_hash()?,
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    let data_class = DataClassRequest { request, context };
    data_class.validate()?;
    Ok(data_class)
}

// ============================================================================ results

/// An already-acquired data-classification artifact and the caller's provenance ref for it.
#[derive(Debug, Clone)]
pub struct DataClassInference<'a> {
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

/// A deterministic dictionary match of an Accepted Attribute name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataClassDictionaryHit {
    pub attribute_ref: Id,
    pub normalized_name: String,
    pub classification: PilotDataClass,
}

/// Which path proposed a classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClassSource {
    Dictionary,
    Inference,
}

/// An existing classification that S2.3 does not overwrite or repair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "conflict", rename_all = "snake_case", deny_unknown_fields)]
pub enum DataClassConflict {
    /// The stored value is not one of the pilot classifications.
    UnsupportedExistingClassification { attribute_ref: Id, existing: String },
    /// The stored pilot classification differs from the proposed one.
    ExistingClassificationMismatch {
        attribute_ref: Id,
        existing: PilotDataClass,
        proposed: PilotDataClass,
        source: DataClassSource,
    },
}

impl DataClassConflict {
    fn order_key(&self) -> (Id, String) {
        let attribute_ref = match self {
            DataClassConflict::UnsupportedExistingClassification { attribute_ref, .. }
            | DataClassConflict::ExistingClassificationMismatch { attribute_ref, .. } => {
                attribute_ref.clone()
            }
        };
        (
            attribute_ref,
            serde_json::to_string(self).unwrap_or_default(),
        )
    }
}

/// A deterministic, non-conflict outcome.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum DataClassIssue {
    /// Inference-eligible Attributes exist but no inference was supplied; no fallback.
    InferenceUnavailable { attribute_refs: Vec<Id> },
}

/// Everything one analysis produced, canonically ordered; nothing has been applied.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DataClassAnalysisResult {
    pub dictionary_hits: Vec<DataClassDictionaryHit>,
    pub proposals: Vec<Proposal>,
    pub unresolved_refs: Vec<Id>,
    pub conflicts: Vec<DataClassConflict>,
    pub issues: Vec<DataClassIssue>,
}

// ============================================================================ inference output

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataClassOutput {
    version: u32,
    classifications: Vec<OutputClassification>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputClassification {
    attribute_ref: Id,
    classification: PilotDataClass,
}

fn compile_schema() -> Result<JSONSchema, DataClassError> {
    let schema: Value =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| DataClassError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| DataClassError::SchemaCompilation {
            reason: e.to_string(),
        })
}

/// The validated inference classifications by Attribute.
fn inferred_classes(
    context: &DataClassContext,
    request: &InferenceRequest,
    artifact: &InferenceArtifact,
) -> Result<BTreeMap<Id, PilotDataClass>, DataClassError> {
    artifact
        .validate_for(request)
        .map_err(|e| DataClassError::InvalidInferenceArtifact {
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
    let decoded: DataClassOutput =
        serde_json::from_value(output.clone()).map_err(|e| schema_invalid(e.to_string()))?;
    if decoded.version != DATA_CLASS_OUTPUT_VERSION {
        return Err(schema_invalid(format!(
            "output version {} is not 1",
            decoded.version
        )));
    }
    let mut items = decoded.classifications;
    items.sort_by(|a, b| a.attribute_ref.cmp(&b.attribute_ref));
    for item in &items {
        let Some(attribute) = context.attribute(&item.attribute_ref) else {
            return Err(DataClassError::UnknownAttributeRef {
                attribute_ref: item.attribute_ref.clone(),
            });
        };
        if attribute.dictionary_classification.is_some() {
            return Err(DataClassError::DictionaryOwnedAttribute {
                attribute_ref: item.attribute_ref.clone(),
            });
        }
    }
    let mut classes = BTreeMap::new();
    for item in items {
        if classes
            .insert(item.attribute_ref.clone(), item.classification)
            .is_some()
        {
            return Err(DataClassError::DuplicateCandidate {
                attribute_ref: item.attribute_ref,
            });
        }
    }
    Ok(classes)
}

// ============================================================================ proposals

/// The dry-validated ReplacePayload proposal setting only `data_classification`.
fn classification_proposal(
    graph: &Graph,
    node: &Node,
    attribute: &Attribute,
    class: PilotDataClass,
    acceptance_policy: AcceptancePolicy,
    derivation_refs: Vec<DerivationRef>,
) -> Result<Proposal, DataClassError> {
    let mut updated = attribute.clone();
    updated.data_classification = Some(class.as_str().to_owned());
    let evidence: BTreeSet<_> = node.evidence.iter().cloned().collect();
    let proposal = Proposal::new(
        DATA_CLASS_STAGE,
        PatchSet {
            base_semantic_hash: graph.semantic_hash()?,
            patch: SemanticPatch::ReplacePayload {
                target: ElementPrecondition {
                    id: node.id.clone(),
                    expected_hash: node_element_hash(node)?,
                },
                payload: NodePayload::Attribute(updated),
            },
        },
        evidence.into_iter().collect(),
        derivation_refs,
        ProposalMateriality::Semantic,
        acceptance_policy,
        None,
    )
    .map_err(invalid_proposal)?;
    // In-memory dry validation by the canonical engine; the candidate graph is discarded.
    apply_patch(graph, &proposal.patch_set)
        .map_err(|e| invalid_proposal(format!("proposal does not apply: {e}")))?;
    Ok(proposal)
}

// ============================================================================ analysis

/// Classifies the request's Accepted Attributes: dictionary hits always, and the remaining
/// unresolved Attributes from a supplied inference. Absent inference is an issue only when
/// inference-eligible Attributes exist; malformed inference is an error.
pub fn analyze_data_classification(
    graph: &Graph,
    request: &DataClassRequest,
    inference: Option<DataClassInference<'_>>,
) -> Result<DataClassAnalysisResult, DataClassError> {
    request.validate()?;
    if context_of(graph) != request.context {
        return Err(invalid_input(
            "graph does not match the data-classification request context",
        ));
    }
    if request.request.evidence_refs != context_evidence(graph, &request.context) {
        return Err(invalid_input(
            "evidence_refs differ from the Accepted Attributes' evidence",
        ));
    }
    let context = &request.context;
    let inferred = match &inference {
        Some(i) => Some(inferred_classes(context, &request.request, i.artifact)?),
        None => None,
    };

    let mut dictionary_hits = Vec::new();
    let mut proposals = Vec::new();
    let mut conflicts = Vec::new();
    let mut targets = Vec::new();
    for (node, attribute) in accepted_attributes(graph) {
        let existing = match attribute.data_classification.as_deref() {
            None => None,
            Some(value) => match PilotDataClass::parse(value) {
                Some(class) => Some(class),
                None => {
                    conflicts.push(DataClassConflict::UnsupportedExistingClassification {
                        attribute_ref: node.id.clone(),
                        existing: value.to_owned(),
                    });
                    if let Some((normalized_name, classification)) =
                        dictionary_classification(&attribute.name)
                    {
                        dictionary_hits.push(DataClassDictionaryHit {
                            attribute_ref: node.id.clone(),
                            normalized_name,
                            classification,
                        });
                    }
                    continue;
                }
            },
        };
        if let Some((normalized_name, class)) = dictionary_classification(&attribute.name) {
            dictionary_hits.push(DataClassDictionaryHit {
                attribute_ref: node.id.clone(),
                normalized_name,
                classification: class,
            });
            match existing {
                None => proposals.push(classification_proposal(
                    graph,
                    node,
                    attribute,
                    class,
                    AcceptancePolicy::AutoDerivation,
                    Vec::new(),
                )?),
                Some(current) if current != class => {
                    conflicts.push(DataClassConflict::ExistingClassificationMismatch {
                        attribute_ref: node.id.clone(),
                        existing: current,
                        proposed: class,
                        source: DataClassSource::Dictionary,
                    });
                }
                Some(_) => {}
            }
            continue;
        }
        // Non-dictionary Attribute: the inference path.
        let proposed = inferred.as_ref().and_then(|m| m.get(&node.id)).copied();
        match (existing, proposed) {
            (None, None) => targets.push(node.id.clone()),
            (None, Some(class)) => {
                let derivation = inference
                    .as_ref()
                    .map(|i| i.derivation_ref.clone())
                    .ok_or_else(|| invalid_proposal("inference proposal without inference"))?;
                proposals.push(classification_proposal(
                    graph,
                    node,
                    attribute,
                    class,
                    AcceptancePolicy::HumanConfirm,
                    vec![derivation],
                )?);
            }
            (Some(current), Some(class)) if current != class => {
                conflicts.push(DataClassConflict::ExistingClassificationMismatch {
                    attribute_ref: node.id.clone(),
                    existing: current,
                    proposed: class,
                    source: DataClassSource::Inference,
                });
            }
            (Some(_), _) => {}
        }
    }

    let mut issues = Vec::new();
    if inference.is_none() && !targets.is_empty() {
        issues.push(DataClassIssue::InferenceUnavailable {
            attribute_refs: targets.clone(),
        });
    }
    dictionary_hits.sort();
    proposals.sort_by(|a, b| a.id.cmp(&b.id));
    conflicts.sort_by_key(DataClassConflict::order_key);
    targets.sort();
    Ok(DataClassAnalysisResult {
        dictionary_hits,
        proposals,
        unresolved_refs: targets,
        conflicts,
        issues,
    })
}
