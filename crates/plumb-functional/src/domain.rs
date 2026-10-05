//! Closed-vocabulary domain extraction (plan S2.1; compiler architecture §8).
//!
//! A supplied, already-acquired `domain_extraction` inference names Accepted Concept IDs with
//! exact Requirement byte groundings. Plumb derives every Entity, Attribute and
//! DomainRelationship name and type from those Accepted Concepts, validates the groundings with
//! the S1.5 normalization, and returns grounded HUMAN_CONFIRM proposals of Proposed domain
//! nodes. Dependent proposals only reference Entities already present in the current graph, so
//! extraction is iterative: the same request and artifact are replayed after Entity proposals
//! are applied. A relationship whose endpoint cardinality is unresolved becomes
//! `PLUMB.F2.DOMAIN.RELATION_TYPED` finding material until grounded evidence or a governed human
//! decision resolves it. Nothing here mutates a graph, calls a provider, reads a clock or
//! persists anything; `apply_patch` is used solely to dry-validate proposals in memory.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AgentKind, Attribute, AuditMeta, ConceptKind, DerivationRef, DomainRelationship, Edge,
    ElementStatus, Entity, EvidenceRef, ExtensionKey, Graph, Node, NodePayload, NodeType,
    RelationKind, RelationProperties,
};
use plumb_validation::{
    finding_id, finding_key, load_builtin_software_profile, GeneratedFinding, RuleMetadata,
    ViolationFacts,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::vocabulary::normalize_vocabulary_term;

/// Version of [`DomainContext`].
pub const DOMAIN_CONTEXT_VERSION: u32 = 1;

/// Version of the domain inference output.
pub const DOMAIN_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of domain extraction.
pub const DOMAIN_TASK_KIND: &str = "domain_extraction";

/// Extension key of the provenance-only [`DomainOrigin`].
pub const DOMAIN_ORIGIN_EXTENSION: &str = "plumb_functional:domain_origin";

const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s2-domain.md");
const SCHEMA_SOURCE: &str = include_str!("../../../schemas/inference/s2-domain.schema.json");
const DOMAIN_STAGE: StageId = StageId::S2;
const RELATION_TYPED_RULE: &str = "PLUMB.F2.DOMAIN.RELATION_TYPED";
const CARDINALITY_CONDITION_PREFIX: &str = "domain_relationship_cardinality_unresolved";
const CARDINALITY_DECISION_KIND: &str = "domain_relationship_cardinality";
const MERGE_UNAVAILABLE: &str = "MergeNodes is restricted to Requirement or Term nodes; \
domain-node consolidation is unavailable in S2.1.";
const CARDINALITY_RESOLUTION: &str = "Provide explicit cardinality evidence or record a \
governed domain relationship cardinality decision.";

// ============================================================================ errors

/// Why domain analysis could not run. Malformed supplied inference is an error, never treated
/// as absent inference and never turned into a Finding.
#[derive(Debug, Error)]
pub enum DomainError {
    #[error("invalid domain input: {reason}")]
    InvalidInput { reason: String },
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    #[error("invalid domain inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error("domain schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    #[error("domain output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("unknown Concept reference {concept_ref}")]
    UnknownConceptRef { concept_ref: Id },
    #[error("Concept {concept_ref} is {status:?}, not Accepted")]
    ConceptNotAccepted {
        concept_ref: Id,
        status: ElementStatus,
    },
    #[error("Concept {concept_ref} is {found:?}, expected {expected:?}")]
    WrongConceptKind {
        concept_ref: Id,
        expected: ConceptKind,
        found: ConceptKind,
    },
    #[error("invalid grounding {requirement_ref} {start}..{end}: {reason}")]
    InvalidGrounding {
        requirement_ref: Id,
        start: u64,
        end: u64,
        reason: String,
    },
    #[error("grounding {requirement_ref} {start}..{end} does not name Concept {concept_ref}")]
    GroundingConceptMismatch {
        concept_ref: Id,
        requirement_ref: Id,
        start: u64,
        end: u64,
    },
    #[error("duplicate domain candidate {candidate_ref}")]
    DuplicateCandidate { candidate_ref: Id },
    #[error("invalid cardinality {value:?}")]
    InvalidCardinality { value: String },
    #[error("invalid governed decision {decision_ref}: {reason}")]
    InvalidGovernedDecision { decision_ref: Id, reason: String },
    #[error("ambiguous cardinality of {candidate_ref}: decisions {decision_refs:?}")]
    AmbiguousCardinalityDecision {
        candidate_ref: Id,
        decision_refs: Vec<Id>,
    },
    #[error("existing element {node_ref} conflicts with the domain candidate at its ID")]
    ExistingCandidateConflict { node_ref: Id },
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    #[error("validation profile: {reason}")]
    ValidationProfile { reason: String },
    #[error("generated finding: {reason}")]
    GeneratedFinding { reason: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_input(reason: impl Into<String>) -> DomainError {
    DomainError::InvalidInput {
        reason: reason.into(),
    }
}

fn invalid_proposal(e: impl ToString) -> DomainError {
    DomainError::InvalidProposal {
        reason: e.to_string(),
    }
}

fn schema_invalid(reason: impl Into<String>) -> DomainError {
    DomainError::SchemaInvalid {
        reason: reason.into(),
    }
}

// ============================================================================ cardinality

/// The four canonical pilot endpoint multiplicities; there is no unresolved value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DomainCardinality {
    #[serde(rename = "0..1")]
    ZeroOrOne,
    #[serde(rename = "1")]
    One,
    #[serde(rename = "0..*")]
    ZeroOrMore,
    #[serde(rename = "1..*")]
    OneOrMore,
}

impl DomainCardinality {
    /// Every canonical value in declaration order.
    pub const ALL: [DomainCardinality; 4] = [
        DomainCardinality::ZeroOrOne,
        DomainCardinality::One,
        DomainCardinality::ZeroOrMore,
        DomainCardinality::OneOrMore,
    ];

    /// The exact wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            DomainCardinality::ZeroOrOne => "0..1",
            DomainCardinality::One => "1",
            DomainCardinality::ZeroOrMore => "0..*",
            DomainCardinality::OneOrMore => "1..*",
        }
    }

    /// Parses exactly one of the four canonical strings.
    pub fn parse(value: &str) -> Result<DomainCardinality, DomainError> {
        DomainCardinality::ALL
            .into_iter()
            .find(|c| c.as_str() == value)
            .ok_or_else(|| DomainError::InvalidCardinality {
                value: value.to_owned(),
            })
    }
}

// ============================================================================ context and request

/// One Accepted Requirement with its current statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainRequirementContext {
    pub requirement_ref: Id,
    pub statement: String,
}

/// One Accepted Concept of the closed vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainConceptContext {
    pub concept_ref: Id,
    pub name: String,
    pub definition: String,
    pub concept_kind: ConceptKind,
}

/// The exact domain inference context: all and only Accepted Requirements and Accepted
/// Concepts, each sorted by ID. Terms and domain proposal state are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainContext {
    pub version: u32,
    pub project_id: Id,
    pub requirements: Vec<DomainRequirementContext>,
    pub concepts: Vec<DomainConceptContext>,
}

impl DomainContext {
    /// Version 1 and strictly sorted requirements and concepts.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.version != DOMAIN_CONTEXT_VERSION {
            return Err(invalid_input("unsupported domain context version"));
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
            .concepts
            .windows(2)
            .any(|p| p[0].concept_ref >= p[1].concept_ref)
        {
            return Err(invalid_input("context concepts are not strictly sorted"));
        }
        Ok(())
    }

    /// Generic SHA-256 of the RFC 8785 canonical JSON of the validated context.
    pub fn content_hash(&self) -> Result<Hash, DomainError> {
        self.validate()?;
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn requirement(&self, id: &Id) -> Option<&DomainRequirementContext> {
        self.requirements
            .binary_search_by(|r| r.requirement_ref.cmp(id))
            .ok()
            .map(|i| &self.requirements[i])
    }

    fn concept(&self, id: &Id) -> Option<&DomainConceptContext> {
        self.concepts
            .binary_search_by(|c| c.concept_ref.cmp(id))
            .ok()
            .map(|i| &self.concepts[i])
    }

    /// The sorted union of the Requirement and Concept IDs.
    fn input_refs(&self) -> Vec<Id> {
        let refs: BTreeSet<&Id> = self
            .requirements
            .iter()
            .map(|r| &r.requirement_ref)
            .chain(self.concepts.iter().map(|c| &c.concept_ref))
            .collect();
        refs.into_iter().cloned().collect()
    }
}

/// A domain `InferenceRequest` together with the exact context it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainRequest {
    pub request: InferenceRequest,
    pub context: DomainContext,
}

impl DomainRequest {
    /// The request is a valid S2 domain_extraction request bound to this context, the
    /// committed prompt and schema, with exactly the context IDs as input refs. The evidence
    /// refs depend on the graph and are checked by [`analyze_domain`].
    pub fn validate(&self) -> Result<(), DomainError> {
        let request = &self.request;
        request
            .validate()
            .map_err(|e| invalid_input(format!("invalid inference request: {e}")))?;
        let context_hash = self.context.content_hash()?;
        let mismatch = if request.stage != DOMAIN_STAGE {
            Some("stage is not S2")
        } else if request.task_kind != DOMAIN_TASK_KIND {
            Some("task_kind is not domain_extraction")
        } else if request.input_refs != self.context.input_refs() {
            Some("input_refs differ from the Accepted Requirements and Concepts")
        } else if request.context_hash != context_hash {
            Some("context_hash differs from the context hash")
        } else if request.prompt_template_hash != Hash::content_sha256(PROMPT_TEMPLATE) {
            Some("prompt_template_hash differs from the domain prompt")
        } else if request.schema_hash != Hash::content_sha256(SCHEMA_SOURCE.as_bytes()) {
            Some("schema_hash differs from the domain schema")
        } else {
            None
        };
        match mismatch {
            Some(reason) => Err(invalid_input(reason)),
            None => Ok(()),
        }
    }
}

fn accepted_ids(graph: &Graph, node_type: NodeType) -> Vec<&Id> {
    graph
        .node_ids_by_type(node_type)
        .iter()
        .filter(|id| {
            graph
                .node(id)
                .is_some_and(|n| n.status == ElementStatus::Accepted)
        })
        .collect()
}

fn context_of(graph: &Graph) -> Result<DomainContext, DomainError> {
    let mut requirements = Vec::new();
    for id in accepted_ids(graph, NodeType::Requirement) {
        let Some(NodePayload::Requirement(requirement)) = graph.node(id).map(|n| &n.payload) else {
            return Err(invalid_input(format!("{id} is not a Requirement")));
        };
        requirements.push(DomainRequirementContext {
            requirement_ref: id.clone(),
            statement: requirement.statement.clone(),
        });
    }
    let mut concepts = Vec::new();
    for id in accepted_ids(graph, NodeType::Concept) {
        let Some(NodePayload::Concept(concept)) = graph.node(id).map(|n| &n.payload) else {
            return Err(invalid_input(format!("{id} is not a Concept")));
        };
        concepts.push(DomainConceptContext {
            concept_ref: id.clone(),
            name: concept.name.clone(),
            definition: concept.definition.clone(),
            concept_kind: concept.concept_kind,
        });
    }
    Ok(DomainContext {
        version: DOMAIN_CONTEXT_VERSION,
        project_id: graph.project_id().clone(),
        requirements,
        concepts,
    })
}

/// The sorted unique union of `Node.evidence` of the context's Requirements and Concepts.
fn context_evidence(graph: &Graph, context: &DomainContext) -> Vec<Id> {
    let refs: BTreeSet<Id> = context
        .input_refs()
        .iter()
        .filter_map(|id| graph.node(id))
        .flat_map(|n| n.evidence.iter().map(|e| e.as_id().clone()))
        .collect();
    refs.into_iter().collect()
}

/// Builds the S2 domain request over all Accepted Requirements and Accepted Concepts. The
/// provider policy is always the caller's; no provider is preferred.
pub fn build_domain_request(
    graph: &Graph,
    provider_policy: ProviderPolicy,
) -> Result<DomainRequest, DomainError> {
    let context = context_of(graph)?;
    let request = InferenceRequest::new(
        DOMAIN_STAGE,
        DOMAIN_TASK_KIND.to_owned(),
        context.input_refs(),
        context_evidence(graph, &context),
        context.content_hash()?,
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    let domain = DomainRequest { request, context };
    domain.validate()?;
    Ok(domain)
}

// ============================================================================ public results

/// A zero-based, end-exclusive UTF-8 byte range of a current Accepted Requirement statement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainGrounding {
    pub requirement_ref: Id,
    pub start: u64,
    pub end: u64,
}

/// Provenance only (`plumb_functional:domain_origin`): the Concepts and groundings a domain
/// node was derived from. Names, value types, nullability, relationship kinds, cardinality
/// values, status and confidence live only in the typed payload and the PSG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DomainOrigin {
    Entity {
        concept_ref: Id,
        grounding: DomainGrounding,
    },
    Attribute {
        concept_ref: Id,
        grounding: DomainGrounding,
        owner_concept_ref: Id,
        owner_grounding: DomainGrounding,
        value_type_concept_ref: Id,
        value_type_grounding: DomainGrounding,
        nullable_grounding: DomainGrounding,
    },
    DomainRelationship {
        concept_ref: Id,
        grounding: DomainGrounding,
        from_concept_ref: Id,
        from_grounding: DomainGrounding,
        to_concept_ref: Id,
        to_grounding: DomainGrounding,
        cardinality_from_grounding: Option<DomainGrounding>,
        cardinality_to_grounding: Option<DomainGrounding>,
        cardinality_decision_ref: Option<Id>,
    },
}

impl DomainOrigin {
    fn node_type(&self) -> NodeType {
        match self {
            DomainOrigin::Entity { .. } => NodeType::Entity,
            DomainOrigin::Attribute { .. } => NodeType::Attribute,
            DomainOrigin::DomainRelationship { .. } => NodeType::DomainRelationship,
        }
    }

    /// The deterministic candidate ID of this origin's semantic identity.
    fn candidate_ref(&self, project_id: &Id) -> Result<Id, CoreError> {
        match self {
            DomainOrigin::Entity { concept_ref, .. } => entity_id(project_id, concept_ref),
            DomainOrigin::Attribute {
                concept_ref,
                owner_concept_ref,
                ..
            } => attribute_id(project_id, owner_concept_ref, concept_ref),
            DomainOrigin::DomainRelationship {
                concept_ref,
                from_concept_ref,
                to_concept_ref,
                ..
            } => relationship_id(project_id, concept_ref, from_concept_ref, to_concept_ref),
        }
    }
}

/// Who owns the Proposed domain elements and when, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// An already-acquired domain artifact and the caller's provenance ref for it.
#[derive(Debug, Clone)]
pub struct DomainInference<'a> {
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

/// A deterministic, non-conflict outcome.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum DomainIssue {
    /// No domain inference was supplied; there is no fallback.
    InferenceUnavailable,
    /// The graph has no Accepted Concept, so there is no vocabulary to extract with.
    AcceptedVocabularyUnavailable,
    /// The candidate waits for these Concepts' Entities to exist in the graph.
    EntityDependencyPending {
        candidate_ref: Id,
        concept_refs: Vec<Id>,
    },
    /// These Concepts' Entities are in conflict, so no dependent proposal can be built.
    EntityDependencyConflicted {
        candidate_ref: Id,
        concept_refs: Vec<Id>,
    },
}

/// A deterministic conflict among existing domain nodes of one domain-origin identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "conflict", rename_all = "snake_case", deny_unknown_fields)]
pub enum DomainConflict {
    /// More than one active node carries the same domain-origin identity.
    DuplicateDomainOrigin {
        candidate_ref: Id,
        node_refs: Vec<Id>,
    },
    /// Active nodes of one domain-origin identity disagree in payload or owner.
    ExistingOriginSemanticConflict {
        candidate_ref: Id,
        node_refs: Vec<Id>,
    },
}

/// Whether existing domain nodes of one identity are, or can be, consolidated.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case", deny_unknown_fields)]
pub enum DomainMergeDisposition {
    /// No existing active node has this identity.
    NotDuplicate,
    /// Exactly one equivalent node already exists and is reused.
    ExistingEquivalent { node_ref: Id },
    /// The duplicates' payloads differ; nothing is merged.
    UnavailablePayloadDifference,
    /// The duplicate Attributes have different owners; nothing is merged.
    UnavailableOwnerDifference,
    /// Equivalent duplicates cannot be consolidated (MergeNodes is Requirement/Term-only);
    /// `reason` is diagnostic only.
    UnavailablePatchConstraint { reason: String },
}

/// The consolidation disposition of one domain-origin identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainMergeOutcome {
    pub candidate_ref: Id,
    pub node_refs: Vec<Id>,
    pub disposition: DomainMergeDisposition,
}

/// Everything one analysis produced, canonically ordered; nothing has been applied.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DomainAnalysisResult {
    pub proposals: Vec<Proposal>,
    pub findings: Vec<GeneratedFinding>,
    pub issues: Vec<DomainIssue>,
    pub conflicts: Vec<DomainConflict>,
    pub merge_dispositions: Vec<DomainMergeOutcome>,
}

// ============================================================================ inference output

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DomainOutput {
    version: u32,
    entities: Vec<OutputEntity>,
    attributes: Vec<OutputAttribute>,
    relationships: Vec<OutputRelationship>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputEntity {
    concept_ref: Id,
    grounding: DomainGrounding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputValueType {
    concept_ref: Id,
    grounding: DomainGrounding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputNullable {
    value: bool,
    grounding: DomainGrounding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputAttribute {
    concept_ref: Id,
    grounding: DomainGrounding,
    owner_concept_ref: Id,
    owner_grounding: DomainGrounding,
    value_type: OutputValueType,
    nullable: OutputNullable,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputCardinality {
    value: DomainCardinality,
    grounding: DomainGrounding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputRelationship {
    concept_ref: Id,
    grounding: DomainGrounding,
    from_concept_ref: Id,
    from_grounding: DomainGrounding,
    to_concept_ref: Id,
    to_grounding: DomainGrounding,
    cardinality_from: Option<OutputCardinality>,
    cardinality_to: Option<OutputCardinality>,
}

fn compile_schema() -> Result<JSONSchema, DomainError> {
    let schema: Value =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| DomainError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| DomainError::SchemaCompilation {
            reason: e.to_string(),
        })
}

/// The decoded, schema-valid output with every candidate list in identity order.
fn decoded_output(
    request: &InferenceRequest,
    artifact: &InferenceArtifact,
) -> Result<DomainOutput, DomainError> {
    artifact
        .validate_for(request)
        .map_err(|e| DomainError::InvalidInferenceArtifact {
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
    let mut decoded: DomainOutput =
        serde_json::from_value(output.clone()).map_err(|e| schema_invalid(e.to_string()))?;
    if decoded.version != DOMAIN_OUTPUT_VERSION {
        return Err(schema_invalid(format!(
            "output version {} is not 1",
            decoded.version
        )));
    }
    decoded
        .entities
        .sort_by(|a, b| a.concept_ref.cmp(&b.concept_ref));
    decoded.attributes.sort_by(|a, b| {
        (&a.owner_concept_ref, &a.concept_ref).cmp(&(&b.owner_concept_ref, &b.concept_ref))
    });
    decoded.relationships.sort_by(|a, b| {
        (&a.concept_ref, &a.from_concept_ref, &a.to_concept_ref).cmp(&(
            &b.concept_ref,
            &b.from_concept_ref,
            &b.to_concept_ref,
        ))
    });
    Ok(decoded)
}

/// Rejects duplicate semantic identities in one category (lists are identity-sorted).
fn reject_duplicates<T, K: PartialEq>(
    items: &[T],
    key: impl Fn(&T) -> K,
    candidate_ref: impl Fn(&T) -> Result<Id, CoreError>,
) -> Result<(), DomainError> {
    for pair in items.windows(2) {
        if key(&pair[0]) == key(&pair[1]) {
            return Err(DomainError::DuplicateCandidate {
                candidate_ref: candidate_ref(&pair[0])?,
            });
        }
    }
    Ok(())
}

/// A Concept reference of a candidate with the kind its role requires (`None`: any kind).
struct ConceptUse<'o> {
    concept_ref: &'o Id,
    kind: Option<ConceptKind>,
    grounding: &'o DomainGrounding,
}

fn concept_uses(output: &DomainOutput) -> Vec<ConceptUse<'_>> {
    let mut uses = Vec::new();
    for e in &output.entities {
        uses.push(ConceptUse {
            concept_ref: &e.concept_ref,
            kind: Some(ConceptKind::ObjectType),
            grounding: &e.grounding,
        });
    }
    for a in &output.attributes {
        uses.push(ConceptUse {
            concept_ref: &a.concept_ref,
            kind: None,
            grounding: &a.grounding,
        });
        uses.push(ConceptUse {
            concept_ref: &a.owner_concept_ref,
            kind: Some(ConceptKind::ObjectType),
            grounding: &a.owner_grounding,
        });
        uses.push(ConceptUse {
            concept_ref: &a.value_type.concept_ref,
            kind: Some(ConceptKind::ValueType),
            grounding: &a.value_type.grounding,
        });
    }
    for r in &output.relationships {
        uses.push(ConceptUse {
            concept_ref: &r.concept_ref,
            kind: Some(ConceptKind::FactType),
            grounding: &r.grounding,
        });
        uses.push(ConceptUse {
            concept_ref: &r.from_concept_ref,
            kind: Some(ConceptKind::ObjectType),
            grounding: &r.from_grounding,
        });
        uses.push(ConceptUse {
            concept_ref: &r.to_concept_ref,
            kind: Some(ConceptKind::ObjectType),
            grounding: &r.to_grounding,
        });
    }
    uses
}

/// Every grounding of the output, including nullable and cardinality groundings.
fn all_groundings(output: &DomainOutput) -> Vec<&DomainGrounding> {
    let mut groundings: Vec<&DomainGrounding> =
        concept_uses(output).iter().map(|u| u.grounding).collect();
    for a in &output.attributes {
        groundings.push(&a.nullable.grounding);
    }
    for r in &output.relationships {
        groundings.extend(r.cardinality_from.iter().map(|c| &c.grounding));
        groundings.extend(r.cardinality_to.iter().map(|c| &c.grounding));
    }
    groundings
}

/// The `[start, end)` byte bounds of a valid UTF-8 range of `text`.
fn bounds(text: &str, start: u64, end: u64) -> Option<(usize, usize)> {
    let (s, e) = (usize::try_from(start).ok()?, usize::try_from(end).ok()?);
    (s < e && e <= text.len() && text.is_char_boundary(s) && text.is_char_boundary(e))
        .then_some((s, e))
}

fn grounded_text<'c>(
    context: &'c DomainContext,
    g: &DomainGrounding,
) -> Result<&'c str, DomainError> {
    let invalid = |reason: &str| DomainError::InvalidGrounding {
        requirement_ref: g.requirement_ref.clone(),
        start: g.start,
        end: g.end,
        reason: reason.to_owned(),
    };
    let requirement = context
        .requirement(&g.requirement_ref)
        .ok_or_else(|| invalid("not a current Accepted Requirement"))?;
    let (s, e) = bounds(&requirement.statement, g.start, g.end)
        .ok_or_else(|| invalid("not a valid UTF-8 range of the statement"))?;
    Ok(&requirement.statement[s..e])
}

/// Steps 7-11 of the contract order over the decoded output.
fn validate_output(
    graph: &Graph,
    context: &DomainContext,
    output: &DomainOutput,
) -> Result<(), DomainError> {
    let project = graph.project_id();
    reject_duplicates(
        &output.entities,
        |e| e.concept_ref.clone(),
        |e| entity_id(project, &e.concept_ref),
    )?;
    reject_duplicates(
        &output.attributes,
        |a| (a.owner_concept_ref.clone(), a.concept_ref.clone()),
        |a| attribute_id(project, &a.owner_concept_ref, &a.concept_ref),
    )?;
    reject_duplicates(
        &output.relationships,
        |r| {
            (
                r.concept_ref.clone(),
                r.from_concept_ref.clone(),
                r.to_concept_ref.clone(),
            )
        },
        |r| {
            relationship_id(
                project,
                &r.concept_ref,
                &r.from_concept_ref,
                &r.to_concept_ref,
            )
        },
    )?;
    let uses = concept_uses(output);
    for u in &uses {
        if context.concept(u.concept_ref).is_none() {
            return Err(match graph.node(u.concept_ref) {
                Some(node) if matches!(node.payload, NodePayload::Concept(_)) => {
                    DomainError::ConceptNotAccepted {
                        concept_ref: u.concept_ref.clone(),
                        status: node.status,
                    }
                }
                _ => DomainError::UnknownConceptRef {
                    concept_ref: u.concept_ref.clone(),
                },
            });
        }
    }
    for u in &uses {
        let found = context
            .concept(u.concept_ref)
            .map(|c| c.concept_kind)
            .ok_or_else(|| DomainError::UnknownConceptRef {
                concept_ref: u.concept_ref.clone(),
            })?;
        if let Some(expected) = u.kind.filter(|k| *k != found) {
            return Err(DomainError::WrongConceptKind {
                concept_ref: u.concept_ref.clone(),
                expected,
                found,
            });
        }
    }
    for g in all_groundings(output) {
        grounded_text(context, g)?;
    }
    for u in &uses {
        let text = grounded_text(context, u.grounding)?;
        let name = context
            .concept(u.concept_ref)
            .map(|c| c.name.as_str())
            .unwrap_or_default();
        let grounded = normalize_vocabulary_term(text);
        if grounded.is_none() || grounded != normalize_vocabulary_term(name) {
            return Err(DomainError::GroundingConceptMismatch {
                concept_ref: u.concept_ref.clone(),
                requirement_ref: u.grounding.requirement_ref.clone(),
                start: u.grounding.start,
                end: u.grounding.end,
            });
        }
    }
    Ok(())
}

// ============================================================================ identity

fn short_id(prefix: &str, body: &Value) -> Result<Id, CoreError> {
    let digest = Hash::content_sha256(&to_canonical_json(body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("{prefix}:{}", &hex[..16]).parse()
}

/// `entity:<16 hex of SHA-256(RFC 8785 {project_id, concept_ref, node_type: entity})>`.
fn entity_id(project_id: &Id, concept_ref: &Id) -> Result<Id, CoreError> {
    short_id(
        "entity",
        &json!({"project_id": project_id, "concept_ref": concept_ref, "node_type": "entity"}),
    )
}

/// `attr:<16 hex of SHA-256(RFC 8785 {project_id, owner_concept_ref, attribute_concept_ref,
/// node_type: attribute})>`; the value type and nullability are not part of the identity.
fn attribute_id(
    project_id: &Id,
    owner_concept_ref: &Id,
    attribute_concept_ref: &Id,
) -> Result<Id, CoreError> {
    short_id(
        "attr",
        &json!({
            "project_id": project_id,
            "owner_concept_ref": owner_concept_ref,
            "attribute_concept_ref": attribute_concept_ref,
            "node_type": "attribute",
        }),
    )
}

/// `domainrel:<16 hex of SHA-256(RFC 8785 {project_id, relationship_concept_ref,
/// from_concept_ref, to_concept_ref, node_type: domain_relationship})>`; cardinality is not
/// part of the identity. This is also the relationship candidate key.
fn relationship_id(
    project_id: &Id,
    relationship_concept_ref: &Id,
    from_concept_ref: &Id,
    to_concept_ref: &Id,
) -> Result<Id, CoreError> {
    short_id(
        "domainrel",
        &json!({
            "project_id": project_id,
            "relationship_concept_ref": relationship_concept_ref,
            "from_concept_ref": from_concept_ref,
            "to_concept_ref": to_concept_ref,
            "node_type": "domain_relationship",
        }),
    )
}

/// `rel:<16 hex of SHA-256(RFC 8785 {kind, from, to})>`.
fn edge_id(kind: &RelationKind, from: &Id, to: &Id) -> Result<Id, CoreError> {
    short_id("rel", &json!({"kind": kind, "from": from, "to": to}))
}

// ============================================================================ existing domain nodes

/// One active (Proposed or Accepted) domain node with a valid domain origin.
struct ActiveDomainNode<'g> {
    node: &'g Node,
}

fn origin_of(node: &Node) -> Result<Option<DomainOrigin>, DomainError> {
    let Some((_, value)) = node
        .extensions
        .iter()
        .find(|(key, _)| key.as_str() == DOMAIN_ORIGIN_EXTENSION)
    else {
        return Ok(None);
    };
    let origin: DomainOrigin = serde_json::from_value(value.clone())
        .map_err(|e| invalid_input(format!("{} has a malformed domain origin: {e}", node.id)))?;
    if origin.node_type() != node.payload.node_type() {
        return Err(invalid_input(format!(
            "{} has a domain origin of another node type",
            node.id
        )));
    }
    Ok(Some(origin))
}

/// Active domain nodes grouped by the candidate ID of their origin identity.
fn active_domain_nodes(
    graph: &Graph,
) -> Result<BTreeMap<Id, Vec<ActiveDomainNode<'_>>>, DomainError> {
    let mut by_identity: BTreeMap<Id, Vec<ActiveDomainNode<'_>>> = BTreeMap::new();
    for node_type in [
        NodeType::Entity,
        NodeType::Attribute,
        NodeType::DomainRelationship,
    ] {
        for id in graph.node_ids_by_type(node_type) {
            let Some(node) = graph
                .node(id)
                .filter(|n| matches!(n.status, ElementStatus::Proposed | ElementStatus::Accepted))
            else {
                continue;
            };
            if let Some(origin) = origin_of(node)? {
                by_identity
                    .entry(origin.candidate_ref(graph.project_id())?)
                    .or_default()
                    .push(ActiveDomainNode { node });
            }
        }
    }
    Ok(by_identity)
}

/// The active Entities owning `attribute` through active `has_attribute` edges.
fn owners<'g>(graph: &'g Graph, attribute: &Id) -> BTreeSet<&'g Id> {
    graph
        .incoming_edge_ids(attribute)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| {
            e.kind == RelationKind::HasAttribute
                && matches!(e.status, ElementStatus::Proposed | ElementStatus::Accepted)
        })
        .map(|e| &e.from)
        .collect()
}

/// How an identity is represented in the current graph.
enum Presence<'g> {
    /// No active node; a new proposal may be built.
    Absent,
    /// The node at the deterministic ID is identical (or exactly one active node is
    /// equivalent); `active` says whether it can serve as a dependency.
    Present { node: &'g Node, active: bool },
    /// Several or disagreeing active nodes; nothing is built from this identity.
    Conflicted,
}

/// Checks the deterministic ID and the active same-origin nodes against the expected node
/// (`expected_payload` is `None` when it cannot be built yet, as for an unresolved
/// relationship); `owner` is the expected owner of an Attribute.
fn presence<'g>(
    graph: &'g Graph,
    active: &BTreeMap<Id, Vec<ActiveDomainNode<'g>>>,
    candidate_ref: &Id,
    expected_type: NodeType,
    expected_payload: Option<&NodePayload>,
    owner: Option<&Id>,
) -> Result<Presence<'g>, DomainError> {
    let project = graph.project_id();
    if let Some(existing) = graph.node(candidate_ref) {
        let origin = origin_of(existing)?;
        let same_identity = match &origin {
            Some(o) => &o.candidate_ref(project)? == candidate_ref,
            None => false,
        };
        let same_payload = expected_payload.is_none_or(|p| &existing.payload == p);
        if existing.payload.node_type() != expected_type || !same_identity || !same_payload {
            return Err(DomainError::ExistingCandidateConflict {
                node_ref: candidate_ref.clone(),
            });
        }
    }
    let nodes = active.get(candidate_ref).map(Vec::as_slice).unwrap_or(&[]);
    match nodes {
        [] => Ok(match graph.node(candidate_ref) {
            Some(node) => Presence::Present {
                node,
                active: false,
            },
            None => Presence::Absent,
        }),
        [only] => {
            let same_payload = expected_payload.is_none_or(|p| &only.node.payload == p);
            let same_owner =
                owner.is_none_or(|o| owners(graph, &only.node.id) == BTreeSet::from([o]));
            Ok(if same_payload && same_owner {
                Presence::Present {
                    node: only.node,
                    active: true,
                }
            } else {
                Presence::Conflicted
            })
        }
        _ => Ok(Presence::Conflicted),
    }
}

/// The active Entity of an ObjectType Concept, when exactly one equivalent exists.
enum EntityResolution<'g> {
    Resolved(&'g Id),
    Pending,
    Conflicted,
}

fn resolve_entity<'g>(
    graph: &'g Graph,
    context: &DomainContext,
    active: &BTreeMap<Id, Vec<ActiveDomainNode<'g>>>,
    concept_ref: &Id,
) -> Result<EntityResolution<'g>, DomainError> {
    let id = entity_id(graph.project_id(), concept_ref)?;
    let payload = entity_payload(context, concept_ref);
    Ok(
        match presence(graph, active, &id, NodeType::Entity, Some(&payload), None)? {
            Presence::Present { node, active: true } => EntityResolution::Resolved(&node.id),
            Presence::Present { active: false, .. } | Presence::Absent => EntityResolution::Pending,
            Presence::Conflicted => EntityResolution::Conflicted,
        },
    )
}

fn concept_name(context: &DomainContext, concept_ref: &Id) -> String {
    context
        .concept(concept_ref)
        .map(|c| c.name.clone())
        .unwrap_or_default()
}

fn entity_payload(context: &DomainContext, concept_ref: &Id) -> NodePayload {
    NodePayload::Entity(Entity {
        name: concept_name(context, concept_ref),
        description: None,
        aggregate_root: None,
    })
}

// ============================================================================ governed cardinality

/// The exact `ResolutionDecision.answer` of a governed cardinality decision.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CardinalityMarker {
    #[allow(dead_code)]
    kind: String,
    candidate_ref: Id,
    cardinality_from: String,
    cardinality_to: String,
}

/// A qualifying Accepted human cardinality decision.
struct CardinalityDecision {
    decision_ref: Id,
    candidate_ref: Id,
    from: DomainCardinality,
    to: DomainCardinality,
    /// The RELATION_TYPED Findings it resolves, directly or through a Question.
    resolved_findings: BTreeSet<Id>,
}

fn is_clean_text(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

/// The Accepted RELATION_TYPED Finding `id`, if it is one.
fn relation_typed_finding<'g>(graph: &'g Graph, id: &Id) -> Option<&'g Id> {
    graph
        .node(id)
        .filter(|n| {
            n.status == ElementStatus::Accepted
                && matches!(&n.payload, NodePayload::Finding(f) if f.code == RELATION_TYPED_RULE)
        })
        .map(|n| &n.id)
}

/// Every Accepted decision claiming the cardinality marker kind for a current relationship
/// candidate, validated; other answers, non-Accepted decisions and decisions naming any other
/// candidate are ignored, and a malformed or ungoverned current claim is an error.
fn cardinality_decisions(
    graph: &Graph,
    current: &BTreeSet<&Id>,
) -> Result<Vec<CardinalityDecision>, DomainError> {
    let mut decisions = Vec::new();
    for id in graph.node_ids_by_type(NodeType::ResolutionDecision) {
        let Some(node) = graph.node(id) else {
            continue;
        };
        let NodePayload::ResolutionDecision(decision) = &node.payload else {
            continue;
        };
        if node.status != ElementStatus::Accepted
            || decision.answer.get("kind") != Some(&Value::from(CARDINALITY_DECISION_KIND))
        {
            continue;
        }
        // Only decisions naming a current relationship candidate are this run's concern;
        // F3 owns the correctness of every other decision.
        let names_current = decision
            .answer
            .get("candidate_ref")
            .and_then(Value::as_str)
            .is_some_and(|c| current.iter().any(|id| id.as_str() == c));
        if !names_current {
            continue;
        }
        let invalid = |reason: String| DomainError::InvalidGovernedDecision {
            decision_ref: id.clone(),
            reason,
        };
        let marker: CardinalityMarker = serde_json::from_value(decision.answer.clone())
            .map_err(|e| invalid(format!("malformed cardinality marker: {e}")))?;
        if !marker.candidate_ref.as_str().starts_with("domainrel:") {
            return Err(invalid(format!(
                "candidate_ref {} is not a domain relationship candidate",
                marker.candidate_ref
            )));
        }
        let from = DomainCardinality::parse(&marker.cardinality_from)?;
        let to = DomainCardinality::parse(&marker.cardinality_to)?;
        if !decision.rationale.as_deref().is_some_and(is_clean_text) {
            return Err(invalid("rationale is missing or not clean text".into()));
        }
        let human = graph.node(&decision.decided_by).is_some_and(|agent| {
            agent.status == ElementStatus::Accepted
                && matches!(&agent.payload, NodePayload::Agent(a) if a.agent_kind == AgentKind::Human)
        });
        if !human {
            return Err(invalid("not decided by an Accepted human Agent".into()));
        }
        let mut resolved_findings = BTreeSet::new();
        for edge_id in graph.outgoing_edge_ids(id) {
            let Some(edge) = graph.edge(edge_id).filter(|e| {
                e.kind == RelationKind::Resolves && e.status == ElementStatus::Accepted
            }) else {
                continue;
            };
            let finding = match graph.node(&edge.to).map(|n| (n.status, &n.payload)) {
                Some((ElementStatus::Accepted, NodePayload::Question(q))) => {
                    relation_typed_finding(graph, &q.finding_ref)
                }
                _ => relation_typed_finding(graph, &edge.to),
            };
            if let Some(finding) = finding {
                resolved_findings.insert(finding.clone());
            }
        }
        if resolved_findings.is_empty() {
            return Err(invalid(format!(
                "no Accepted resolves edge to an Accepted {RELATION_TYPED_RULE} Finding or a \
                 Question of one"
            )));
        }
        decisions.push(CardinalityDecision {
            decision_ref: id.clone(),
            candidate_ref: marker.candidate_ref,
            from,
            to,
            resolved_findings,
        });
    }
    Ok(decisions)
}

// ============================================================================ proposals

struct Builder<'a> {
    graph: &'a Graph,
    base_semantic_hash: Hash,
    derivation_ref: &'a DerivationRef,
    audit: &'a DomainAudit,
    extension: ExtensionKey,
}

impl Builder<'_> {
    fn audit_meta(&self) -> Result<AuditMeta, DomainError> {
        AuditMeta::new(
            self.audit.created_by.clone(),
            self.audit.created_at,
            None,
            None,
        )
        .map_err(invalid_proposal)
    }

    /// The sorted unique union of `Node.evidence` of the grounding Requirements.
    fn evidence(&self, groundings: &[&DomainGrounding]) -> Vec<EvidenceRef> {
        let refs: BTreeSet<&EvidenceRef> = groundings
            .iter()
            .filter_map(|g| self.graph.node(&g.requirement_ref))
            .flat_map(|n| n.evidence.iter())
            .collect();
        refs.into_iter().cloned().collect()
    }

    fn node(
        &self,
        id: Id,
        payload: NodePayload,
        evidence: &[EvidenceRef],
        origin: &DomainOrigin,
    ) -> Result<Node, DomainError> {
        let node = Node {
            id,
            revision: 1,
            status: ElementStatus::Proposed,
            payload,
            evidence: evidence.to_vec(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::from([(
                self.extension.clone(),
                serde_json::to_value(origin).map_err(invalid_proposal)?,
            )]),
            audit: self.audit_meta()?,
        };
        node.validate().map_err(invalid_proposal)?;
        Ok(node)
    }

    fn edge(
        &self,
        kind: RelationKind,
        from: &Id,
        to: &Id,
        evidence: &[EvidenceRef],
    ) -> Result<Edge, DomainError> {
        Ok(Edge {
            id: edge_id(&kind, from, to)?,
            revision: 1,
            status: ElementStatus::Proposed,
            kind,
            from: from.clone(),
            to: to.clone(),
            properties: RelationProperties::None,
            evidence: evidence.to_vec(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: self.audit_meta()?,
        })
    }

    /// A dry-validated Compound of AddNode then the AddEdges in edge-ID order.
    fn propose(&self, node: Node, mut edges: Vec<Edge>) -> Result<Proposal, DomainError> {
        let evidence = node.evidence.clone();
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        let mut patches = vec![SemanticPatch::AddNode { node }];
        patches.extend(
            edges
                .into_iter()
                .map(|edge| SemanticPatch::AddEdge { edge }),
        );
        let proposal = Proposal::new(
            DOMAIN_STAGE,
            PatchSet {
                base_semantic_hash: self.base_semantic_hash.clone(),
                patch: SemanticPatch::Compound { patches },
            },
            evidence,
            vec![self.derivation_ref.clone()],
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .map_err(invalid_proposal)?;
        // In-memory dry validation by the canonical engine; the candidate graph is discarded.
        apply_patch(self.graph, &proposal.patch_set)
            .map_err(|e| invalid_proposal(format!("proposal does not apply: {e}")))?;
        Ok(proposal)
    }
}

/// The dry-validated Attribute proposal; the applied candidate must give the Attribute
/// exactly one active owner.
fn propose_attribute(
    builder: &Builder<'_>,
    node: Node,
    edges: Vec<Edge>,
) -> Result<Proposal, DomainError> {
    let id = node.id.clone();
    let proposal = builder.propose(node, edges)?;
    let applied = apply_patch(builder.graph, &proposal.patch_set).map_err(invalid_proposal)?;
    if owners(&applied.graph, &id).len() != 1 {
        return Err(invalid_proposal(format!(
            "attribute {id} does not have exactly one owning Entity"
        )));
    }
    Ok(proposal)
}

// ============================================================================ consolidation

/// Conflicts and the unavailable consolidation of every identity carried by more than one
/// active node. Canonical MergeNodes is Requirement/Term-only, so S2.1 never builds a domain
/// merge, never selects a keeper and never rewrites or rewires anything.
fn report_duplicates(
    graph: &Graph,
    active: &BTreeMap<Id, Vec<ActiveDomainNode<'_>>>,
    conflicts: &mut Vec<DomainConflict>,
    dispositions: &mut Vec<DomainMergeOutcome>,
) {
    for (candidate_ref, nodes) in active {
        if nodes.len() < 2 {
            continue;
        }
        let node_refs: Vec<Id> = nodes
            .iter()
            .map(|n| n.node.id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        conflicts.push(DomainConflict::DuplicateDomainOrigin {
            candidate_ref: candidate_ref.clone(),
            node_refs: node_refs.clone(),
        });
        let first = nodes[0].node;
        let disagreement = if nodes.iter().any(|n| n.node.payload != first.payload) {
            Some(DomainMergeDisposition::UnavailablePayloadDifference)
        } else if first.payload.node_type() == NodeType::Attribute
            && nodes
                .iter()
                .any(|n| owners(graph, &n.node.id) != owners(graph, &first.id))
        {
            Some(DomainMergeDisposition::UnavailableOwnerDifference)
        } else {
            None
        };
        let disposition = match disagreement {
            Some(unavailable) => {
                conflicts.push(DomainConflict::ExistingOriginSemanticConflict {
                    candidate_ref: candidate_ref.clone(),
                    node_refs: node_refs.clone(),
                });
                unavailable
            }
            None => DomainMergeDisposition::UnavailablePatchConstraint {
                reason: MERGE_UNAVAILABLE.to_owned(),
            },
        };
        dispositions.push(DomainMergeOutcome {
            candidate_ref: candidate_ref.clone(),
            node_refs,
            disposition,
        });
    }
}

// ============================================================================ analysis

fn relation_typed_rule(graph: &Graph) -> Result<RuleMetadata, DomainError> {
    let profile = load_builtin_software_profile().map_err(|e| DomainError::ValidationProfile {
        reason: e.to_string(),
    })?;
    if graph.profile_id() != &profile.profile_id {
        return Err(DomainError::ValidationProfile {
            reason: format!(
                "graph profile {} is not the built-in profile {}",
                graph.profile_id(),
                profile.profile_id
            ),
        });
    }
    profile
        .rules
        .into_iter()
        .find(|r| r.id == RELATION_TYPED_RULE)
        .ok_or_else(|| DomainError::ValidationProfile {
            reason: format!("profile has no rule {RELATION_TYPED_RULE}"),
        })
}

/// The finding targets of a relationship candidate: the sorted unique Accepted Requirements
/// grounding its FactType and both endpoint Concepts.
fn relationship_targets(r: &OutputRelationship) -> Vec<Id> {
    let targets: BTreeSet<&Id> = [&r.grounding, &r.from_grounding, &r.to_grounding]
        .into_iter()
        .map(|g| &g.requirement_ref)
        .collect();
    targets.into_iter().cloned().collect()
}

fn condition_key(candidate_ref: &Id) -> String {
    format!("{CARDINALITY_CONDITION_PREFIX}:{candidate_ref}")
}

/// Analyzes the request's Accepted Requirements and Concepts with a supplied domain inference.
/// Absent inference yields only InferenceUnavailable; malformed inference is an error.
pub fn analyze_domain(
    graph: &Graph,
    request: &DomainRequest,
    inference: Option<DomainInference<'_>>,
    audit: &DomainAudit,
) -> Result<DomainAnalysisResult, DomainError> {
    request.validate()?;
    if context_of(graph)? != request.context {
        return Err(invalid_input(
            "graph does not match the domain request context",
        ));
    }
    if request.request.evidence_refs != context_evidence(graph, &request.context) {
        return Err(invalid_input(
            "evidence_refs differ from the Accepted Requirements' and Concepts' evidence",
        ));
    }
    let mut result = DomainAnalysisResult {
        proposals: Vec::new(),
        findings: Vec::new(),
        issues: Vec::new(),
        conflicts: Vec::new(),
        merge_dispositions: Vec::new(),
    };
    let Some(inference) = inference else {
        result.issues.push(DomainIssue::InferenceUnavailable);
        return Ok(result);
    };
    let context = &request.context;
    let output = decoded_output(&request.request, inference.artifact)?;
    validate_output(graph, context, &output)?;
    if context.concepts.is_empty() {
        result
            .issues
            .push(DomainIssue::AcceptedVocabularyUnavailable);
    }

    // Governed cardinality decisions for current relationship candidates, bound both ways: a
    // decision naming current candidate C must resolve C's finding, and must not resolve the
    // finding of another current candidate.
    let project = graph.project_id();
    let mut candidate_findings: BTreeMap<Id, Id> = BTreeMap::new();
    for r in &output.relationships {
        let candidate_ref = relationship_id(
            project,
            &r.concept_ref,
            &r.from_concept_ref,
            &r.to_concept_ref,
        )?;
        let key = finding_key(
            RELATION_TYPED_RULE,
            &relationship_targets(r),
            &condition_key(&candidate_ref),
        )
        .map_err(|e| DomainError::GeneratedFinding {
            reason: e.to_string(),
        })?;
        let finding = finding_id(&key).map_err(|e| DomainError::GeneratedFinding {
            reason: e.to_string(),
        })?;
        candidate_findings.insert(candidate_ref, finding);
    }
    let mut decided: BTreeMap<Id, Vec<CardinalityDecision>> = BTreeMap::new();
    let current: BTreeSet<&Id> = candidate_findings.keys().collect();
    for d in cardinality_decisions(graph, &current)? {
        for (candidate_ref, finding) in &candidate_findings {
            let names = &d.candidate_ref == candidate_ref;
            let resolves = d.resolved_findings.contains(finding);
            if names != resolves {
                return Err(DomainError::InvalidGovernedDecision {
                    decision_ref: d.decision_ref.clone(),
                    reason: format!(
                        "candidate_ref {} is not bound to the finding {finding} of candidate \
                         {candidate_ref}",
                        d.candidate_ref
                    ),
                });
            }
        }
        decided.entry(d.candidate_ref.clone()).or_default().push(d);
    }
    if let Some((candidate_ref, ds)) = decided.iter().find(|(_, ds)| ds.len() > 1) {
        return Err(DomainError::AmbiguousCardinalityDecision {
            candidate_ref: candidate_ref.clone(),
            decision_refs: ds.iter().map(|d| d.decision_ref.clone()).collect(),
        });
    }

    let active = active_domain_nodes(graph)?;
    let builder = Builder {
        graph,
        base_semantic_hash: graph.semantic_hash()?,
        derivation_ref: &inference.derivation_ref,
        audit,
        extension: DOMAIN_ORIGIN_EXTENSION.parse().map_err(invalid_proposal)?,
    };
    let mut proposals = Vec::new();
    let mut findings = BTreeMap::new();
    let mut issues = BTreeSet::new();
    let mut dispositions = Vec::new();
    let mut record = |candidate_ref: &Id, presence: &Presence<'_>| {
        let disposition = match presence {
            Presence::Absent => Some(DomainMergeDisposition::NotDuplicate),
            Presence::Present { node, .. } => Some(DomainMergeDisposition::ExistingEquivalent {
                node_ref: node.id.clone(),
            }),
            Presence::Conflicted => None,
        };
        if let Some(disposition) = disposition {
            let node_refs = match presence {
                Presence::Present { node, .. } => vec![node.id.clone()],
                _ => Vec::new(),
            };
            dispositions.push(DomainMergeOutcome {
                candidate_ref: candidate_ref.clone(),
                node_refs,
                disposition,
            });
        }
    };

    // Entities.
    for e in &output.entities {
        let id = entity_id(project, &e.concept_ref)?;
        let payload = entity_payload(context, &e.concept_ref);
        let state = presence(graph, &active, &id, NodeType::Entity, Some(&payload), None)?;
        record(&id, &state);
        if !matches!(state, Presence::Absent) {
            continue;
        }
        let evidence = builder.evidence(&[&e.grounding]);
        let origin = DomainOrigin::Entity {
            concept_ref: e.concept_ref.clone(),
            grounding: e.grounding.clone(),
        };
        let node = builder.node(id.clone(), payload, &evidence, &origin)?;
        let derived = builder.edge(RelationKind::DerivedFrom, &id, &e.concept_ref, &evidence)?;
        proposals.push(builder.propose(node, vec![derived])?);
    }

    // Attributes.
    for a in &output.attributes {
        let id = attribute_id(project, &a.owner_concept_ref, &a.concept_ref)?;
        let owner = match resolve_entity(graph, context, &active, &a.owner_concept_ref)? {
            EntityResolution::Resolved(owner) => owner,
            EntityResolution::Pending => {
                issues.insert(DomainIssue::EntityDependencyPending {
                    candidate_ref: id,
                    concept_refs: vec![a.owner_concept_ref.clone()],
                });
                continue;
            }
            EntityResolution::Conflicted => {
                issues.insert(DomainIssue::EntityDependencyConflicted {
                    candidate_ref: id,
                    concept_refs: vec![a.owner_concept_ref.clone()],
                });
                continue;
            }
        };
        let payload = NodePayload::Attribute(Attribute {
            name: concept_name(context, &a.concept_ref),
            value_type: concept_name(context, &a.value_type.concept_ref),
            nullable: a.nullable.value,
            unit: None,
            precision: None,
            enum_values: None,
            data_classification: None,
        });
        let state = presence(
            graph,
            &active,
            &id,
            NodeType::Attribute,
            Some(&payload),
            Some(owner),
        )?;
        record(&id, &state);
        if !matches!(state, Presence::Absent) {
            continue;
        }
        let evidence = builder.evidence(&[
            &a.grounding,
            &a.owner_grounding,
            &a.value_type.grounding,
            &a.nullable.grounding,
        ]);
        let origin = DomainOrigin::Attribute {
            concept_ref: a.concept_ref.clone(),
            grounding: a.grounding.clone(),
            owner_concept_ref: a.owner_concept_ref.clone(),
            owner_grounding: a.owner_grounding.clone(),
            value_type_concept_ref: a.value_type.concept_ref.clone(),
            value_type_grounding: a.value_type.grounding.clone(),
            nullable_grounding: a.nullable.grounding.clone(),
        };
        let node = builder.node(id.clone(), payload, &evidence, &origin)?;
        let mut edges = vec![
            builder.edge(RelationKind::HasAttribute, owner, &id, &evidence)?,
            builder.edge(RelationKind::DerivedFrom, &id, &a.concept_ref, &evidence)?,
        ];
        if a.value_type.concept_ref != a.concept_ref {
            edges.push(builder.edge(
                RelationKind::DerivedFrom,
                &id,
                &a.value_type.concept_ref,
                &evidence,
            )?);
        }
        proposals.push(propose_attribute(&builder, node, edges)?);
    }

    // DomainRelationships.
    let mut rule = None;
    for r in &output.relationships {
        let id = relationship_id(
            project,
            &r.concept_ref,
            &r.from_concept_ref,
            &r.to_concept_ref,
        )?;
        let mut endpoints = Vec::new();
        let mut pending = BTreeSet::new();
        let mut conflicted = BTreeSet::new();
        for concept_ref in [&r.from_concept_ref, &r.to_concept_ref] {
            match resolve_entity(graph, context, &active, concept_ref)? {
                EntityResolution::Resolved(entity) => endpoints.push(entity),
                EntityResolution::Pending => {
                    pending.insert(concept_ref.clone());
                }
                EntityResolution::Conflicted => {
                    conflicted.insert(concept_ref.clone());
                }
            }
        }
        if !conflicted.is_empty() {
            issues.insert(DomainIssue::EntityDependencyConflicted {
                candidate_ref: id,
                concept_refs: conflicted.into_iter().collect(),
            });
            continue;
        }
        if !pending.is_empty() {
            issues.insert(DomainIssue::EntityDependencyPending {
                candidate_ref: id,
                concept_refs: pending.into_iter().collect(),
            });
            continue;
        }
        let decision = decided.get(&id).and_then(|ds| ds.first());
        let cardinality = match decision {
            Some(d) => Some((d.from, d.to)),
            None => match (&r.cardinality_from, &r.cardinality_to) {
                (Some(from), Some(to)) => Some((from.value, to.value)),
                _ => None,
            },
        };
        let payload = cardinality.map(|(from, to)| {
            NodePayload::DomainRelationship(DomainRelationship {
                from_entity: endpoints[0].clone(),
                to_entity: endpoints[1].clone(),
                relationship_kind: concept_name(context, &r.concept_ref),
                cardinality_from: from.as_str().to_owned(),
                cardinality_to: to.as_str().to_owned(),
                name: None,
                snapshot_semantics: None,
                ownership: None,
            })
        });
        let state = presence(
            graph,
            &active,
            &id,
            NodeType::DomainRelationship,
            payload.as_ref(),
            None,
        )?;
        record(&id, &state);
        if !matches!(state, Presence::Absent) {
            continue;
        }
        let Some(payload) = payload else {
            if rule.is_none() {
                rule = Some(relation_typed_rule(graph)?);
            }
            let Some(rule) = &rule else {
                continue;
            };
            let message =
                format!("Domain relationship candidate {id} has unresolved endpoint cardinality.");
            let finding = GeneratedFinding::for_violation(
                rule,
                rule.severity,
                &ViolationFacts {
                    targets: &relationship_targets(r),
                    semantic_condition_key: &condition_key(&id),
                    message: &message,
                    suggested_resolution: Some(CARDINALITY_RESOLUTION),
                },
                None,
            )
            .map_err(|e| DomainError::GeneratedFinding {
                reason: e.to_string(),
            })?;
            findings.insert(finding.id.clone(), finding);
            continue;
        };
        let (cardinality_from_grounding, cardinality_to_grounding) = match decision {
            Some(_) => (None, None),
            None => (
                r.cardinality_from.as_ref().map(|c| c.grounding.clone()),
                r.cardinality_to.as_ref().map(|c| c.grounding.clone()),
            ),
        };
        let mut groundings = vec![&r.grounding, &r.from_grounding, &r.to_grounding];
        groundings.extend(cardinality_from_grounding.iter());
        groundings.extend(cardinality_to_grounding.iter());
        let evidence = builder.evidence(&groundings);
        let origin = DomainOrigin::DomainRelationship {
            concept_ref: r.concept_ref.clone(),
            grounding: r.grounding.clone(),
            from_concept_ref: r.from_concept_ref.clone(),
            from_grounding: r.from_grounding.clone(),
            to_concept_ref: r.to_concept_ref.clone(),
            to_grounding: r.to_grounding.clone(),
            cardinality_from_grounding: cardinality_from_grounding.clone(),
            cardinality_to_grounding: cardinality_to_grounding.clone(),
            cardinality_decision_ref: decision.map(|d| d.decision_ref.clone()),
        };
        let node = builder.node(id.clone(), payload, &evidence, &origin)?;
        let derived = builder.edge(RelationKind::DerivedFrom, &id, &r.concept_ref, &evidence)?;
        proposals.push(builder.propose(node, vec![derived])?);
    }

    let mut conflicts = Vec::new();
    report_duplicates(graph, &active, &mut conflicts, &mut dispositions);

    proposals.sort_by(|a, b| a.id.cmp(&b.id));
    conflicts.sort();
    dispositions.sort();
    result.issues.extend(issues);
    result.issues.sort();
    result.proposals = proposals;
    result.findings = findings.into_values().collect();
    result.conflicts = conflicts;
    result.merge_dispositions = dispositions;
    Ok(result)
}
