//! Grounded lifecycle extraction (plan S2.2; compiler architecture §8).
//!
//! A supplied, already-acquired `lifecycle_extraction` inference names active state owners,
//! exact Accepted Requirement byte groundings of lifecycle states and transitions, an optional
//! trigger from the supplied Operation/Event list, and proposed invariant expressions. Plumb
//! derives every State name from its grounding with the S1.5 normalization, proposes Proposed
//! States owned through `has_state`, and proposes a Transition only when both endpoint States
//! exist and exactly one trigger is resolved, as one `transitions_via` edge. An unresolved
//! trigger is `PLUMB.F2.STATE.TRANSITION_COMPLETE` finding material, resolvable by a governed
//! human decision. There is no fallback: nothing is derived from capitalization or field
//! names, and no initial, terminal or externally-entered semantics are invented. Nothing here
//! mutates a graph, calls a provider, reads a clock or persists anything; `apply_patch` is
//! used solely to dry-validate proposals in memory.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AgentKind, AuditMeta, DerivationRef, Edge, ElementStatus, EvidenceRef, ExtensionKey, Graph,
    Node, NodeCategory, NodePayload, NodeType, RelationKind, RelationProperties, State, Transition,
};
use plumb_validation::{
    finding_id, finding_key, load_builtin_software_profile, GeneratedFinding, RuleMetadata,
    ViolationFacts,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::invariant::{
    invariant_id, invariant_payload, validate_expression, InvariantOrigin,
    INVARIANT_ORIGIN_EXTENSION,
};
use crate::vocabulary::normalize_vocabulary_term;

/// Version of [`LifecycleContext`].
pub const LIFECYCLE_CONTEXT_VERSION: u32 = 1;

/// Version of the lifecycle inference output.
pub const LIFECYCLE_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of lifecycle extraction.
pub const LIFECYCLE_TASK_KIND: &str = "lifecycle_extraction";

/// Extension key of the provenance-only [`StateOrigin`].
pub const STATE_ORIGIN_EXTENSION: &str = "plumb_functional:state_origin";

const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s2-lifecycle.md");
const SCHEMA_SOURCE: &str = include_str!("../../../schemas/inference/s2-lifecycle.schema.json");
const LIFECYCLE_STAGE: StageId = StageId::S2;
const TRANSITION_COMPLETE_RULE: &str = "PLUMB.F2.STATE.TRANSITION_COMPLETE";
const TRIGGER_CONDITION_PREFIX: &str = "state_transition_trigger_unresolved";
const TRIGGER_DECISION_KIND: &str = "state_transition_trigger";
const TRIGGER_RESOLUTION: &str =
    "Provide or select the Operation/Event that triggers this state transition.";

// ============================================================================ errors

/// Why lifecycle analysis could not run. Malformed supplied inference is an error, never
/// treated as absent inference and never turned into a Finding.
#[derive(Debug, Error)]
pub enum LifecycleError {
    #[error("invalid lifecycle input: {reason}")]
    InvalidInput { reason: String },
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    #[error("invalid lifecycle inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error("lifecycle schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    #[error("lifecycle output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("unknown state owner {owner_ref}")]
    UnknownOwnerRef { owner_ref: Id },
    #[error("invalid state owner {owner_ref}: {reason}")]
    InvalidOwner { owner_ref: Id, reason: String },
    #[error("invalid trigger {trigger_ref}: {reason}")]
    InvalidTriggerRef { trigger_ref: Id, reason: String },
    #[error("invalid grounding {requirement_ref} {start}..{end}: {reason}")]
    InvalidGrounding {
        requirement_ref: Id,
        start: u64,
        end: u64,
        reason: String,
    },
    #[error("invalid invariant expression {expression:?}")]
    InvalidExpression { expression: String },
    #[error("duplicate lifecycle candidate {candidate_ref}")]
    DuplicateCandidate { candidate_ref: Id },
    #[error("transition endpoint {state_ref} ({state_key:?}) is not a known state candidate")]
    UnknownStateCandidate { state_ref: Id, state_key: String },
    #[error("state {state_ref} is owned by {found:?}, not {expected}")]
    StateOwnerMismatch {
        state_ref: Id,
        expected: Id,
        found: Vec<Id>,
    },
    #[error("invalid governed decision {decision_ref}: {reason}")]
    InvalidGovernedDecision { decision_ref: Id, reason: String },
    #[error("ambiguous trigger of {candidate_ref}: decisions {decision_refs:?}")]
    AmbiguousTriggerDecision {
        candidate_ref: Id,
        decision_refs: Vec<Id>,
    },
    #[error("existing element {node_ref} conflicts with the lifecycle candidate at its ID")]
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

fn invalid_input(reason: impl Into<String>) -> LifecycleError {
    LifecycleError::InvalidInput {
        reason: reason.into(),
    }
}

fn invalid_proposal(e: impl ToString) -> LifecycleError {
    LifecycleError::InvalidProposal {
        reason: e.to_string(),
    }
}

fn schema_invalid(reason: impl Into<String>) -> LifecycleError {
    LifecycleError::SchemaInvalid {
        reason: reason.into(),
    }
}

// ============================================================================ context and request

/// One Accepted Requirement with its current statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleRequirementContext {
    pub requirement_ref: Id,
    pub statement: String,
}

/// `NodeType` on the wire as its exact payload type tag (plumb-psg defines no serde form).
mod node_type_wire {
    use plumb_psg::NodeType;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &NodeType, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(value.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<NodeType, D::Error> {
        let tag = String::deserialize(deserializer)?;
        NodeType::ALL
            .iter()
            .copied()
            .find(|t| t.as_str() == tag)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown node type {tag:?}")))
    }
}

/// One active (Proposed or Accepted) node of the StateOwner category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleOwnerContext {
    pub stateful_ref: Id,
    #[serde(with = "node_type_wire")]
    pub node_type: NodeType,
    pub name: String,
}

/// One active (Proposed or Accepted) Operation or Event, the closed trigger choice set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleTriggerContext {
    pub trigger_ref: Id,
    #[serde(with = "node_type_wire")]
    pub node_type: NodeType,
    pub name: String,
}

/// The exact lifecycle inference context, every list strictly sorted by ID. States,
/// Transitions and Invariants are deliberately absent, so applying their proposals does not
/// stale the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleContext {
    pub version: u32,
    pub project_id: Id,
    pub requirements: Vec<LifecycleRequirementContext>,
    pub owners: Vec<LifecycleOwnerContext>,
    pub triggers: Vec<LifecycleTriggerContext>,
}

fn strictly_sorted<T>(items: &[T], key: impl Fn(&T) -> &Id) -> bool {
    items.windows(2).all(|p| key(&p[0]) < key(&p[1]))
}

impl LifecycleContext {
    /// Version 1, strictly sorted lists, StateOwner owners and Operation/Event triggers.
    pub fn validate(&self) -> Result<(), LifecycleError> {
        if self.version != LIFECYCLE_CONTEXT_VERSION {
            return Err(invalid_input("unsupported lifecycle context version"));
        }
        if !strictly_sorted(&self.requirements, |r| &r.requirement_ref)
            || !strictly_sorted(&self.owners, |o| &o.stateful_ref)
            || !strictly_sorted(&self.triggers, |t| &t.trigger_ref)
        {
            return Err(invalid_input("context lists are not strictly sorted"));
        }
        if self
            .owners
            .iter()
            .any(|o| !NodeCategory::StateOwner.contains(o.node_type))
        {
            return Err(invalid_input("context owner is not a StateOwner"));
        }
        if self
            .triggers
            .iter()
            .any(|t| !matches!(t.node_type, NodeType::Operation | NodeType::Event))
        {
            return Err(invalid_input(
                "context trigger is not an Operation or Event",
            ));
        }
        Ok(())
    }

    /// Generic SHA-256 of the RFC 8785 canonical JSON of the validated context.
    pub fn content_hash(&self) -> Result<Hash, LifecycleError> {
        self.validate()?;
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn requirement(&self, id: &Id) -> Option<&LifecycleRequirementContext> {
        self.requirements
            .binary_search_by(|r| r.requirement_ref.cmp(id))
            .ok()
            .map(|i| &self.requirements[i])
    }

    fn owner(&self, id: &Id) -> Option<&LifecycleOwnerContext> {
        self.owners
            .binary_search_by(|o| o.stateful_ref.cmp(id))
            .ok()
            .map(|i| &self.owners[i])
    }

    fn trigger(&self, id: &Id) -> Option<&LifecycleTriggerContext> {
        self.triggers
            .binary_search_by(|t| t.trigger_ref.cmp(id))
            .ok()
            .map(|i| &self.triggers[i])
    }

    /// The sorted union of the Requirement, owner and trigger IDs.
    fn input_refs(&self) -> Vec<Id> {
        let refs: BTreeSet<&Id> = self
            .requirements
            .iter()
            .map(|r| &r.requirement_ref)
            .chain(self.owners.iter().map(|o| &o.stateful_ref))
            .chain(self.triggers.iter().map(|t| &t.trigger_ref))
            .collect();
        refs.into_iter().cloned().collect()
    }
}

/// A lifecycle `InferenceRequest` together with the exact context it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleRequest {
    pub request: InferenceRequest,
    pub context: LifecycleContext,
}

impl LifecycleRequest {
    /// The request is a valid S2 lifecycle_extraction request bound to this context, the
    /// committed prompt and schema, with exactly the context IDs as input refs. The evidence
    /// refs depend on the graph and are checked by [`analyze_lifecycle`].
    pub fn validate(&self) -> Result<(), LifecycleError> {
        let request = &self.request;
        request
            .validate()
            .map_err(|e| invalid_input(format!("invalid inference request: {e}")))?;
        let context_hash = self.context.content_hash()?;
        let mismatch = if request.stage != LIFECYCLE_STAGE {
            Some("stage is not S2")
        } else if request.task_kind != LIFECYCLE_TASK_KIND {
            Some("task_kind is not lifecycle_extraction")
        } else if request.input_refs != self.context.input_refs() {
            Some("input_refs differ from the context requirements, owners and triggers")
        } else if request.context_hash != context_hash {
            Some("context_hash differs from the context hash")
        } else if request.prompt_template_hash != Hash::content_sha256(PROMPT_TEMPLATE) {
            Some("prompt_template_hash differs from the lifecycle prompt")
        } else if request.schema_hash != Hash::content_sha256(SCHEMA_SOURCE.as_bytes()) {
            Some("schema_hash differs from the lifecycle schema")
        } else {
            None
        };
        match mismatch {
            Some(reason) => Err(invalid_input(reason)),
            None => Ok(()),
        }
    }
}

fn is_active(node: &Node) -> bool {
    matches!(
        node.status,
        ElementStatus::Proposed | ElementStatus::Accepted
    )
}

/// The display name of a StateOwner from its typed payload.
fn owner_name(node: &Node) -> Result<String, LifecycleError> {
    let name = match &node.payload {
        NodePayload::Entity(e) => &e.name,
        NodePayload::Process(p) => &p.name,
        NodePayload::SoftwareSystem(a)
        | NodePayload::Container(a)
        | NodePayload::Component(a)
        | NodePayload::Module(a)
        | NodePayload::Interface(a)
        | NodePayload::DataStore(a)
        | NodePayload::ExternalSystem(a)
        | NodePayload::DeploymentNode(a)
        | NodePayload::RuntimeEnvironment(a)
        | NodePayload::NetworkZone(a) => &a.name,
        _ => {
            return Err(LifecycleError::InvalidOwner {
                owner_ref: node.id.clone(),
                reason: "not a StateOwner payload".into(),
            })
        }
    };
    if name.is_empty() || name.trim() != name || name.chars().any(char::is_control) {
        return Err(LifecycleError::InvalidOwner {
            owner_ref: node.id.clone(),
            reason: "owner payload has no usable name".into(),
        });
    }
    Ok(name.clone())
}

fn context_of(graph: &Graph) -> Result<LifecycleContext, LifecycleError> {
    let mut requirements = Vec::new();
    let mut owners = Vec::new();
    let mut triggers = Vec::new();
    for node in graph.nodes().values() {
        let node_type = node.payload.node_type();
        match &node.payload {
            NodePayload::Requirement(r) if node.status == ElementStatus::Accepted => {
                requirements.push(LifecycleRequirementContext {
                    requirement_ref: node.id.clone(),
                    statement: r.statement.clone(),
                });
            }
            NodePayload::Operation(o) if is_active(node) => {
                triggers.push(LifecycleTriggerContext {
                    trigger_ref: node.id.clone(),
                    node_type,
                    name: o.name.clone(),
                });
            }
            NodePayload::Event(e) if is_active(node) => {
                triggers.push(LifecycleTriggerContext {
                    trigger_ref: node.id.clone(),
                    node_type,
                    name: e.name.clone(),
                });
            }
            _ if is_active(node) && NodeCategory::StateOwner.contains(node_type) => {
                owners.push(LifecycleOwnerContext {
                    stateful_ref: node.id.clone(),
                    node_type,
                    name: owner_name(node)?,
                });
            }
            _ => {}
        }
    }
    Ok(LifecycleContext {
        version: LIFECYCLE_CONTEXT_VERSION,
        project_id: graph.project_id().clone(),
        requirements,
        owners,
        triggers,
    })
}

/// The sorted unique union of `Node.evidence` of every context element.
fn context_evidence(graph: &Graph, context: &LifecycleContext) -> Vec<Id> {
    let refs: BTreeSet<Id> = context
        .input_refs()
        .iter()
        .filter_map(|id| graph.node(id))
        .flat_map(|n| n.evidence.iter().map(|e| e.as_id().clone()))
        .collect();
    refs.into_iter().collect()
}

/// Builds the S2 lifecycle request over the Accepted Requirements, active state owners and
/// active Operation/Event triggers. The provider policy is always the caller's.
pub fn build_lifecycle_request(
    graph: &Graph,
    provider_policy: ProviderPolicy,
) -> Result<LifecycleRequest, LifecycleError> {
    let context = context_of(graph)?;
    let request = InferenceRequest::new(
        LIFECYCLE_STAGE,
        LIFECYCLE_TASK_KIND.to_owned(),
        context.input_refs(),
        context_evidence(graph, &context),
        context.content_hash()?,
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    let lifecycle = LifecycleRequest { request, context };
    lifecycle.validate()?;
    Ok(lifecycle)
}

// ============================================================================ public results

/// A zero-based, end-exclusive UTF-8 byte range of a current Accepted Requirement statement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleGrounding {
    pub requirement_ref: Id,
    pub start: u64,
    pub end: u64,
}

/// Provenance only (`plumb_functional:state_origin`) of a State or Transition. Names,
/// payload fields, status and confidence live only in the typed payload and the PSG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StateOrigin {
    State {
        stateful_ref: Id,
        state_key: String,
        grounding: LifecycleGrounding,
    },
    Transition {
        stateful_ref: Id,
        grounding: LifecycleGrounding,
        from_state_grounding: LifecycleGrounding,
        to_state_grounding: LifecycleGrounding,
        trigger_ref: Id,
        trigger_decision_ref: Option<Id>,
    },
}

/// Who owns the Proposed lifecycle elements and when, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// An already-acquired lifecycle artifact and the caller's provenance ref for it.
#[derive(Debug, Clone)]
pub struct LifecycleInference<'a> {
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

/// A deterministic, non-conflict outcome.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum LifecycleIssue {
    /// No lifecycle inference was supplied; there is no fallback.
    InferenceUnavailable,
    /// The graph has no active state owner, so no lifecycle can be attached.
    StateOwnerUnavailable,
    /// The transition waits for these endpoint States to exist in the graph.
    StateDependencyPending {
        candidate_ref: Id,
        state_refs: Vec<Id>,
    },
    /// These endpoint States are duplicated or conflicting; nothing is built.
    StateDependencyConflicted {
        candidate_ref: Id,
        state_refs: Vec<Id>,
    },
}

/// A deterministic conflict among existing States of one state-origin identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "conflict", rename_all = "snake_case", deny_unknown_fields)]
pub enum LifecycleConflict {
    /// More than one active State carries the same owner and state key.
    DuplicateStateOrigin {
        candidate_ref: Id,
        node_refs: Vec<Id>,
    },
    /// The one active State of this owner and key has another payload or not exactly its
    /// owner as `has_state` owner.
    ExistingStateOriginConflict {
        candidate_ref: Id,
        node_refs: Vec<Id>,
    },
}

/// An existing element reused for a candidate instead of a duplicate proposal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleExisting {
    pub candidate_ref: Id,
    pub node_ref: Id,
}

/// Everything one analysis produced, canonically ordered; nothing has been applied.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LifecycleAnalysisResult {
    pub proposals: Vec<Proposal>,
    pub findings: Vec<GeneratedFinding>,
    pub issues: Vec<LifecycleIssue>,
    pub conflicts: Vec<LifecycleConflict>,
    pub existing: Vec<LifecycleExisting>,
}

// ============================================================================ inference output

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleOutput {
    version: u32,
    states: Vec<OutputState>,
    transitions: Vec<OutputTransition>,
    invariants: Vec<OutputInvariant>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputState {
    stateful_ref: Id,
    grounding: LifecycleGrounding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputTransition {
    stateful_ref: Id,
    grounding: LifecycleGrounding,
    from_state: LifecycleGrounding,
    to_state: LifecycleGrounding,
    trigger_ref: Option<Id>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputInvariant {
    scope_ref: Id,
    grounding: LifecycleGrounding,
    expression: String,
}

fn compile_schema() -> Result<JSONSchema, LifecycleError> {
    let schema: Value =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| LifecycleError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| LifecycleError::SchemaCompilation {
            reason: e.to_string(),
        })
}

fn decoded_output(
    request: &InferenceRequest,
    artifact: &InferenceArtifact,
) -> Result<LifecycleOutput, LifecycleError> {
    artifact
        .validate_for(request)
        .map_err(|e| LifecycleError::InvalidInferenceArtifact {
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
    let decoded: LifecycleOutput =
        serde_json::from_value(output.clone()).map_err(|e| schema_invalid(e.to_string()))?;
    if decoded.version != LIFECYCLE_OUTPUT_VERSION {
        return Err(schema_invalid(format!(
            "output version {} is not 1",
            decoded.version
        )));
    }
    Ok(decoded)
}

/// The grounded slice: a known Accepted Requirement, a non-empty UTF-8 range on character
/// boundaries, without surrounding whitespace or control characters. Nothing is repaired.
fn grounded_text<'c>(
    context: &'c LifecycleContext,
    g: &LifecycleGrounding,
) -> Result<&'c str, LifecycleError> {
    let invalid = |reason: &str| LifecycleError::InvalidGrounding {
        requirement_ref: g.requirement_ref.clone(),
        start: g.start,
        end: g.end,
        reason: reason.to_owned(),
    };
    let requirement = context
        .requirement(&g.requirement_ref)
        .ok_or_else(|| invalid("not a current Accepted Requirement"))?;
    let text = &requirement.statement;
    let (s, e) = match (usize::try_from(g.start), usize::try_from(g.end)) {
        (Ok(s), Ok(e)) if s < e && e <= text.len() => (s, e),
        _ => return Err(invalid("not a non-empty range of the statement")),
    };
    if !text.is_char_boundary(s) || !text.is_char_boundary(e) {
        return Err(invalid("not on UTF-8 character boundaries"));
    }
    let slice = &text[s..e];
    if slice.trim() != slice || slice.chars().any(char::is_control) {
        return Err(invalid(
            "slice has surrounding whitespace or control characters",
        ));
    }
    Ok(slice)
}

/// The normalized state key of a grounded state mention.
fn state_key(context: &LifecycleContext, g: &LifecycleGrounding) -> Result<String, LifecycleError> {
    normalize_vocabulary_term(grounded_text(context, g)?).ok_or_else(|| {
        LifecycleError::InvalidGrounding {
            requirement_ref: g.requirement_ref.clone(),
            start: g.start,
            end: g.end,
            reason: "the grounded text has no state token".into(),
        }
    })
}

// ============================================================================ identity

/// `<prefix>:<first 16 lowercase hex of SHA-256(RFC 8785 body)>`.
pub(crate) fn short_id(prefix: &str, body: &Value) -> Result<Id, CoreError> {
    let digest = Hash::content_sha256(&to_canonical_json(body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("{prefix}:{}", &hex[..16]).parse()
}

/// `state:<16 hex of SHA-256(RFC 8785 {project_id, stateful_ref, state_key, node_type})>`.
fn state_id(project_id: &Id, stateful_ref: &Id, state_key: &str) -> Result<Id, CoreError> {
    short_id(
        "state",
        &json!({
            "project_id": project_id,
            "stateful_ref": stateful_ref,
            "state_key": state_key,
            "node_type": "state",
        }),
    )
}

/// `transition:<16 hex of SHA-256(RFC 8785 {project_id, stateful_ref, from_state, to_state,
/// grounding, node_type})>`; the trigger is not part of the identity.
fn transition_id(
    project_id: &Id,
    stateful_ref: &Id,
    from_state: &Id,
    to_state: &Id,
    grounding: &LifecycleGrounding,
) -> Result<Id, CoreError> {
    short_id(
        "transition",
        &json!({
            "project_id": project_id,
            "stateful_ref": stateful_ref,
            "from_state": from_state,
            "to_state": to_state,
            "grounding": {
                "requirement_ref": grounding.requirement_ref,
                "start": grounding.start,
                "end": grounding.end,
            },
            "node_type": "transition",
        }),
    )
}

/// `rel:<16 hex of SHA-256(RFC 8785 {kind, from, to})>`.
fn edge_id(kind: &RelationKind, from: &Id, to: &Id) -> Result<Id, CoreError> {
    short_id("rel", &json!({"kind": kind, "from": from, "to": to}))
}

// ============================================================================ validated candidates

struct StateCandidate {
    id: Id,
    stateful_ref: Id,
    key: String,
    grounding: LifecycleGrounding,
}

struct TransitionCandidate {
    id: Id,
    stateful_ref: Id,
    from: (Id, String),
    to: (Id, String),
    grounding: LifecycleGrounding,
    from_grounding: LifecycleGrounding,
    to_grounding: LifecycleGrounding,
    trigger_ref: Option<Id>,
}

struct InvariantCandidate {
    id: Id,
    scope_ref: Id,
    grounding: LifecycleGrounding,
    expression: String,
}

struct Candidates {
    states: Vec<StateCandidate>,
    transitions: Vec<TransitionCandidate>,
    invariants: Vec<InvariantCandidate>,
}

fn check_owner(
    graph: &Graph,
    context: &LifecycleContext,
    owner_ref: &Id,
) -> Result<(), LifecycleError> {
    if context.owner(owner_ref).is_some() {
        return Ok(());
    }
    match graph.node(owner_ref) {
        Some(node) if !NodeCategory::StateOwner.contains(node.payload.node_type()) => {
            Err(LifecycleError::InvalidOwner {
                owner_ref: owner_ref.clone(),
                reason: format!("{:?} is not a StateOwner", node.payload.node_type()),
            })
        }
        _ => Err(LifecycleError::UnknownOwnerRef {
            owner_ref: owner_ref.clone(),
        }),
    }
}

fn check_trigger(context: &LifecycleContext, trigger_ref: &Id) -> Result<(), LifecycleError> {
    match context.trigger(trigger_ref) {
        Some(_) => Ok(()),
        None => Err(LifecycleError::InvalidTriggerRef {
            trigger_ref: trigger_ref.clone(),
            reason: "not an active Operation or Event of the request context".into(),
        }),
    }
}

/// Steps 7-11 of the contract order: owners, triggers, groundings, keys/IDs, duplicates.
fn validated_candidates(
    graph: &Graph,
    context: &LifecycleContext,
    output: LifecycleOutput,
) -> Result<Candidates, LifecycleError> {
    let project = graph.project_id();
    for owner in output
        .states
        .iter()
        .map(|s| &s.stateful_ref)
        .chain(output.transitions.iter().map(|t| &t.stateful_ref))
        .chain(output.invariants.iter().map(|i| &i.scope_ref))
    {
        check_owner(graph, context, owner)?;
    }
    for t in &output.transitions {
        if let Some(trigger) = &t.trigger_ref {
            check_trigger(context, trigger)?;
        }
    }
    for g in output
        .states
        .iter()
        .map(|s| &s.grounding)
        .chain(
            output
                .transitions
                .iter()
                .flat_map(|t| [&t.grounding, &t.from_state, &t.to_state]),
        )
        .chain(output.invariants.iter().map(|i| &i.grounding))
    {
        grounded_text(context, g)?;
    }
    for i in &output.invariants {
        validate_expression(&i.expression)?;
    }

    let mut states = Vec::new();
    for s in output.states {
        let key = state_key(context, &s.grounding)?;
        states.push(StateCandidate {
            id: state_id(project, &s.stateful_ref, &key)?,
            stateful_ref: s.stateful_ref,
            key,
            grounding: s.grounding,
        });
    }
    let mut transitions = Vec::new();
    for t in output.transitions {
        let from_key = state_key(context, &t.from_state)?;
        let to_key = state_key(context, &t.to_state)?;
        let from = state_id(project, &t.stateful_ref, &from_key)?;
        let to = state_id(project, &t.stateful_ref, &to_key)?;
        transitions.push(TransitionCandidate {
            id: transition_id(project, &t.stateful_ref, &from, &to, &t.grounding)?,
            stateful_ref: t.stateful_ref,
            from: (from, from_key),
            to: (to, to_key),
            grounding: t.grounding,
            from_grounding: t.from_state,
            to_grounding: t.to_state,
            trigger_ref: t.trigger_ref,
        });
    }
    let mut invariants = Vec::new();
    for i in output.invariants {
        invariants.push(InvariantCandidate {
            id: invariant_id(project, &i.scope_ref, &i.grounding)?,
            scope_ref: i.scope_ref,
            grounding: i.grounding,
            expression: i.expression,
        });
    }
    states.sort_by(|a, b| a.id.cmp(&b.id));
    transitions.sort_by(|a, b| a.id.cmp(&b.id));
    invariants.sort_by(|a, b| a.id.cmp(&b.id));
    for ids in [
        states.iter().map(|s| &s.id).collect::<Vec<_>>(),
        transitions.iter().map(|t| &t.id).collect(),
        invariants.iter().map(|i| &i.id).collect(),
    ] {
        if let Some(pair) = ids.windows(2).find(|p| p[0] == p[1]) {
            return Err(LifecycleError::DuplicateCandidate {
                candidate_ref: pair[0].clone(),
            });
        }
    }
    Ok(Candidates {
        states,
        transitions,
        invariants,
    })
}

// ============================================================================ existing states

fn origin_value<'n>(node: &'n Node, key: &str) -> Option<&'n Value> {
    node.extensions
        .iter()
        .find(|(k, _)| k.as_str() == key)
        .map(|(_, v)| v)
}

fn state_origin_of(node: &Node) -> Result<Option<StateOrigin>, LifecycleError> {
    let Some(value) = origin_value(node, STATE_ORIGIN_EXTENSION) else {
        return Ok(None);
    };
    let origin: StateOrigin = serde_json::from_value(value.clone())
        .map_err(|e| invalid_input(format!("{} has a malformed state origin: {e}", node.id)))?;
    let expected = match origin {
        StateOrigin::State { .. } => NodeType::State,
        StateOrigin::Transition { .. } => NodeType::Transition,
    };
    if expected != node.payload.node_type() {
        return Err(invalid_input(format!(
            "{} has a state origin of another node type",
            node.id
        )));
    }
    Ok(Some(origin))
}

/// The active `has_state` owners of `state`.
fn state_owners<'g>(graph: &'g Graph, state: &Id) -> BTreeSet<&'g Id> {
    graph
        .incoming_edge_ids(state)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| {
            e.kind == RelationKind::HasState
                && matches!(e.status, ElementStatus::Proposed | ElementStatus::Accepted)
        })
        .map(|e| &e.from)
        .collect()
}

/// Active States grouped by the candidate ID of their state-origin identity.
fn active_states(graph: &Graph) -> Result<BTreeMap<Id, Vec<&Node>>, LifecycleError> {
    let mut by_identity: BTreeMap<Id, Vec<&Node>> = BTreeMap::new();
    for id in graph.node_ids_by_type(NodeType::State) {
        let Some(node) = graph.node(id).filter(|n| is_active(n)) else {
            continue;
        };
        if let Some(StateOrigin::State {
            stateful_ref,
            state_key,
            ..
        }) = state_origin_of(node)?
        {
            by_identity
                .entry(state_id(graph.project_id(), &stateful_ref, &state_key)?)
                .or_default()
                .push(node);
        }
    }
    Ok(by_identity)
}

/// How a State identity is represented in the current graph.
enum StatePresence<'g> {
    /// No active State; a new proposal may be built.
    Absent,
    /// The node at the deterministic ID is identical but not active.
    Inactive(&'g Node),
    /// Exactly one compatible active State.
    Present(&'g Node),
    /// Exactly one active State whose only `has_state` owner is another owner.
    OwnerMismatch(&'g Node, Vec<Id>),
    /// One incompatible active State.
    Incompatible(&'g Node),
    /// Several active States.
    Duplicated(Vec<&'g Node>),
}

fn state_presence<'g>(
    graph: &'g Graph,
    active: &BTreeMap<Id, Vec<&'g Node>>,
    id: &Id,
    stateful_ref: &Id,
    key: &str,
) -> Result<StatePresence<'g>, LifecycleError> {
    let payload = NodePayload::State(State {
        name: key.to_owned(),
    });
    if let Some(existing) = graph.node(id) {
        let same_origin = matches!(
            state_origin_of(existing)?,
            Some(StateOrigin::State { stateful_ref: o, state_key: k, .. })
                if &o == stateful_ref && k == key
        );
        if existing.payload != payload || !same_origin {
            return Err(LifecycleError::ExistingCandidateConflict {
                node_ref: id.clone(),
            });
        }
    }
    let nodes = active.get(id).map(Vec::as_slice).unwrap_or(&[]);
    Ok(match nodes {
        [] => match graph.node(id) {
            Some(node) => StatePresence::Inactive(node),
            None => StatePresence::Absent,
        },
        [only] => {
            let owners = state_owners(graph, &only.id);
            if only.payload != payload {
                StatePresence::Incompatible(only)
            } else if owners == BTreeSet::from([stateful_ref]) {
                StatePresence::Present(only)
            } else if owners.len() == 1 {
                StatePresence::OwnerMismatch(only, owners.into_iter().cloned().collect())
            } else {
                StatePresence::Incompatible(only)
            }
        }
        many => StatePresence::Duplicated(many.to_vec()),
    })
}

// ============================================================================ governed triggers

/// The exact `ResolutionDecision.answer` of a governed trigger decision.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerMarker {
    #[allow(dead_code)]
    kind: String,
    candidate_ref: Id,
    trigger_ref: Id,
}

struct TriggerDecision {
    decision_ref: Id,
    candidate_ref: Id,
    trigger_ref: Id,
    resolved_findings: BTreeSet<Id>,
}

fn is_clean_text(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

fn transition_complete_finding<'g>(graph: &'g Graph, id: &Id) -> Option<&'g Id> {
    graph
        .node(id)
        .filter(|n| {
            n.status == ElementStatus::Accepted
                && matches!(&n.payload, NodePayload::Finding(f) if f.code == TRANSITION_COMPLETE_RULE)
        })
        .map(|n| &n.id)
}

/// Every Accepted decision claiming the trigger marker kind for a current transition
/// candidate, validated; other answers, non-Accepted decisions and decisions naming any
/// other candidate are ignored, and a malformed or ungoverned current claim is an error.
fn trigger_decisions(
    graph: &Graph,
    current: &BTreeSet<&Id>,
) -> Result<Vec<TriggerDecision>, LifecycleError> {
    let mut decisions = Vec::new();
    for id in graph.node_ids_by_type(NodeType::ResolutionDecision) {
        let Some(node) = graph.node(id) else {
            continue;
        };
        let NodePayload::ResolutionDecision(decision) = &node.payload else {
            continue;
        };
        if node.status != ElementStatus::Accepted
            || decision.answer.get("kind") != Some(&Value::from(TRIGGER_DECISION_KIND))
        {
            continue;
        }
        let names_current = decision
            .answer
            .get("candidate_ref")
            .and_then(Value::as_str)
            .is_some_and(|c| current.iter().any(|id| id.as_str() == c));
        if !names_current {
            continue;
        }
        let invalid = |reason: String| LifecycleError::InvalidGovernedDecision {
            decision_ref: id.clone(),
            reason,
        };
        let marker: TriggerMarker = serde_json::from_value(decision.answer.clone())
            .map_err(|e| invalid(format!("malformed trigger marker: {e}")))?;
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
        let trigger_ok = graph.node(&marker.trigger_ref).is_some_and(|t| {
            is_active(t) && matches!(t.payload.node_type(), NodeType::Operation | NodeType::Event)
        });
        if !trigger_ok {
            return Err(LifecycleError::InvalidTriggerRef {
                trigger_ref: marker.trigger_ref,
                reason: format!("decision {id} names no active Operation or Event"),
            });
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
                    transition_complete_finding(graph, &q.finding_ref)
                }
                _ => transition_complete_finding(graph, &edge.to),
            };
            if let Some(finding) = finding {
                resolved_findings.insert(finding.clone());
            }
        }
        if resolved_findings.is_empty() {
            return Err(invalid(format!(
                "no Accepted resolves edge to an Accepted {TRANSITION_COMPLETE_RULE} Finding \
                 or a Question of one"
            )));
        }
        decisions.push(TriggerDecision {
            decision_ref: id.clone(),
            candidate_ref: marker.candidate_ref,
            trigger_ref: marker.trigger_ref,
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
    audit: &'a LifecycleAudit,
}

impl Builder<'_> {
    fn audit_meta(&self) -> Result<AuditMeta, LifecycleError> {
        AuditMeta::new(
            self.audit.created_by.clone(),
            self.audit.created_at,
            None,
            None,
        )
        .map_err(invalid_proposal)
    }

    /// The sorted unique union of `Node.evidence` of the grounding Requirements.
    fn evidence(&self, groundings: &[&LifecycleGrounding]) -> Vec<EvidenceRef> {
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
        extension: &str,
        origin: Value,
    ) -> Result<Node, LifecycleError> {
        let key: ExtensionKey = extension.parse().map_err(invalid_proposal)?;
        let node = Node {
            id,
            revision: 1,
            status: ElementStatus::Proposed,
            payload,
            evidence: evidence.to_vec(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::from([(key, origin)]),
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
    ) -> Result<Edge, LifecycleError> {
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

    /// A dry-validated proposal: AddNode alone, or a Compound of AddNode then one AddEdge.
    fn propose(&self, node: Node, edge: Option<Edge>) -> Result<Proposal, LifecycleError> {
        let evidence = node.evidence.clone();
        let patch = match edge {
            None => SemanticPatch::AddNode { node },
            Some(edge) => SemanticPatch::Compound {
                patches: vec![
                    SemanticPatch::AddNode { node },
                    SemanticPatch::AddEdge { edge },
                ],
            },
        };
        let proposal = Proposal::new(
            LIFECYCLE_STAGE,
            PatchSet {
                base_semantic_hash: self.base_semantic_hash.clone(),
                patch,
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

/// Requires a node at a deterministic ID to have exactly the expected payload and origin.
fn existing_matches(
    graph: &Graph,
    id: &Id,
    payload: &NodePayload,
    extension: &str,
    origin: &Value,
) -> Result<bool, LifecycleError> {
    match graph.node(id) {
        None => Ok(false),
        Some(existing)
            if &existing.payload == payload
                && origin_value(existing, extension) == Some(origin) =>
        {
            Ok(true)
        }
        Some(_) => Err(LifecycleError::ExistingCandidateConflict {
            node_ref: id.clone(),
        }),
    }
}

// ============================================================================ analysis

fn transition_complete_rule(graph: &Graph) -> Result<RuleMetadata, LifecycleError> {
    let profile =
        load_builtin_software_profile().map_err(|e| LifecycleError::ValidationProfile {
            reason: e.to_string(),
        })?;
    if graph.profile_id() != &profile.profile_id {
        return Err(LifecycleError::ValidationProfile {
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
        .find(|r| r.id == TRANSITION_COMPLETE_RULE)
        .ok_or_else(|| LifecycleError::ValidationProfile {
            reason: format!("profile has no rule {TRANSITION_COMPLETE_RULE}"),
        })
}

/// The sorted unique Accepted Requirements of the transition, from-state and to-state
/// groundings.
fn transition_targets(t: &TransitionCandidate) -> Vec<Id> {
    let targets: BTreeSet<&Id> = [&t.grounding, &t.from_grounding, &t.to_grounding]
        .into_iter()
        .map(|g| &g.requirement_ref)
        .collect();
    targets.into_iter().cloned().collect()
}

fn condition_key(candidate_ref: &Id) -> String {
    format!("{TRIGGER_CONDITION_PREFIX}:{candidate_ref}")
}

/// Analyzes the request's lifecycle context with a supplied lifecycle inference. Absent
/// inference yields only InferenceUnavailable; malformed inference is an error.
pub fn analyze_lifecycle(
    graph: &Graph,
    request: &LifecycleRequest,
    inference: Option<LifecycleInference<'_>>,
    audit: &LifecycleAudit,
) -> Result<LifecycleAnalysisResult, LifecycleError> {
    request.validate()?;
    if context_of(graph)? != request.context {
        return Err(invalid_input(
            "graph does not match the lifecycle request context",
        ));
    }
    if request.request.evidence_refs != context_evidence(graph, &request.context) {
        return Err(invalid_input(
            "evidence_refs differ from the lifecycle context evidence",
        ));
    }
    let mut result = LifecycleAnalysisResult {
        proposals: Vec::new(),
        findings: Vec::new(),
        issues: Vec::new(),
        conflicts: Vec::new(),
        existing: Vec::new(),
    };
    let Some(inference) = inference else {
        result.issues.push(LifecycleIssue::InferenceUnavailable);
        return Ok(result);
    };
    let context = &request.context;
    let output = decoded_output(&request.request, inference.artifact)?;
    let candidates = validated_candidates(graph, context, output)?;
    if context.owners.is_empty() {
        result.issues.push(LifecycleIssue::StateOwnerUnavailable);
    }

    // Endpoint resolution: same-output State candidates or existing active States.
    let active = active_states(graph)?;
    let output_states: BTreeSet<&Id> = candidates.states.iter().map(|s| &s.id).collect();
    for t in &candidates.transitions {
        for (state_ref, key) in [&t.from, &t.to] {
            if !output_states.contains(state_ref) && !active.contains_key(state_ref) {
                return Err(LifecycleError::UnknownStateCandidate {
                    state_ref: state_ref.clone(),
                    state_key: key.clone(),
                });
            }
        }
    }

    // Governed trigger decisions for current candidates, bound both ways to their findings.
    let mut candidate_findings: BTreeMap<&Id, Id> = BTreeMap::new();
    for t in &candidates.transitions {
        let key = finding_key(
            TRANSITION_COMPLETE_RULE,
            &transition_targets(t),
            &condition_key(&t.id),
        )
        .map_err(|e| LifecycleError::GeneratedFinding {
            reason: e.to_string(),
        })?;
        let finding = finding_id(&key).map_err(|e| LifecycleError::GeneratedFinding {
            reason: e.to_string(),
        })?;
        candidate_findings.insert(&t.id, finding);
    }
    let current: BTreeSet<&Id> = candidate_findings.keys().copied().collect();
    let mut decided: BTreeMap<Id, Vec<TriggerDecision>> = BTreeMap::new();
    for d in trigger_decisions(graph, &current)? {
        for (candidate_ref, finding) in &candidate_findings {
            if (&d.candidate_ref == *candidate_ref) != d.resolved_findings.contains(finding) {
                return Err(LifecycleError::InvalidGovernedDecision {
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
        return Err(LifecycleError::AmbiguousTriggerDecision {
            candidate_ref: candidate_ref.clone(),
            decision_refs: ds.iter().map(|d| d.decision_ref.clone()).collect(),
        });
    }

    let builder = Builder {
        graph,
        base_semantic_hash: graph.semantic_hash()?,
        derivation_ref: &inference.derivation_ref,
        audit,
    };
    let mut proposals = Vec::new();
    let mut findings = BTreeMap::new();
    let mut issues = BTreeSet::new();
    let mut conflicts = BTreeSet::new();
    let mut existing = BTreeSet::new();

    // States.
    for s in &candidates.states {
        match state_presence(graph, &active, &s.id, &s.stateful_ref, &s.key)? {
            StatePresence::Absent => {}
            StatePresence::Inactive(node) | StatePresence::Present(node) => {
                existing.insert(LifecycleExisting {
                    candidate_ref: s.id.clone(),
                    node_ref: node.id.clone(),
                });
                continue;
            }
            StatePresence::OwnerMismatch(node, _) | StatePresence::Incompatible(node) => {
                conflicts.insert(LifecycleConflict::ExistingStateOriginConflict {
                    candidate_ref: s.id.clone(),
                    node_refs: vec![node.id.clone()],
                });
                continue;
            }
            StatePresence::Duplicated(nodes) => {
                conflicts.insert(LifecycleConflict::DuplicateStateOrigin {
                    candidate_ref: s.id.clone(),
                    node_refs: nodes.iter().map(|n| n.id.clone()).collect(),
                });
                continue;
            }
        }
        let evidence = builder.evidence(&[&s.grounding]);
        let origin = serde_json::to_value(StateOrigin::State {
            stateful_ref: s.stateful_ref.clone(),
            state_key: s.key.clone(),
            grounding: s.grounding.clone(),
        })
        .map_err(invalid_proposal)?;
        let node = builder.node(
            s.id.clone(),
            NodePayload::State(State {
                name: s.key.clone(),
            }),
            &evidence,
            STATE_ORIGIN_EXTENSION,
            origin,
        )?;
        let owner = builder.edge(RelationKind::HasState, &s.stateful_ref, &s.id, &evidence)?;
        proposals.push(builder.propose(node, Some(owner))?);
    }
    // Duplicate state origins of the graph are always reported.
    for (candidate_ref, nodes) in &active {
        if nodes.len() > 1 {
            conflicts.insert(LifecycleConflict::DuplicateStateOrigin {
                candidate_ref: candidate_ref.clone(),
                node_refs: nodes.iter().map(|n| n.id.clone()).collect(),
            });
        }
    }

    // Transitions.
    let mut rule = None;
    for t in &candidates.transitions {
        let mut pending = BTreeSet::new();
        let mut conflicted = BTreeSet::new();
        for (state_ref, key) in [&t.from, &t.to] {
            match state_presence(graph, &active, state_ref, &t.stateful_ref, key)? {
                StatePresence::Present(_) => {}
                StatePresence::Absent | StatePresence::Inactive(_) => {
                    pending.insert(state_ref.clone());
                }
                StatePresence::OwnerMismatch(node, found) => {
                    return Err(LifecycleError::StateOwnerMismatch {
                        state_ref: node.id.clone(),
                        expected: t.stateful_ref.clone(),
                        found,
                    });
                }
                StatePresence::Incompatible(_) | StatePresence::Duplicated(_) => {
                    conflicted.insert(state_ref.clone());
                }
            }
        }
        if !conflicted.is_empty() {
            issues.insert(LifecycleIssue::StateDependencyConflicted {
                candidate_ref: t.id.clone(),
                state_refs: conflicted.into_iter().collect(),
            });
            continue;
        }
        if !pending.is_empty() {
            issues.insert(LifecycleIssue::StateDependencyPending {
                candidate_ref: t.id.clone(),
                state_refs: pending.into_iter().collect(),
            });
            continue;
        }
        let payload = NodePayload::Transition(Transition {
            stateful_ref: t.stateful_ref.clone(),
            from_state: t.from.0.clone(),
            to_state: t.to.0.clone(),
            guard_expr: None,
            effect_refs: None,
        });
        if let Some(node) = graph.node(&t.id) {
            let same_origin = matches!(
                state_origin_of(node)?,
                Some(StateOrigin::Transition { stateful_ref, grounding, from_state_grounding, to_state_grounding, .. })
                    if stateful_ref == t.stateful_ref
                        && grounding == t.grounding
                        && from_state_grounding == t.from_grounding
                        && to_state_grounding == t.to_grounding
            );
            if node.payload != payload || !same_origin {
                return Err(LifecycleError::ExistingCandidateConflict {
                    node_ref: t.id.clone(),
                });
            }
            existing.insert(LifecycleExisting {
                candidate_ref: t.id.clone(),
                node_ref: t.id.clone(),
            });
            continue;
        }
        let decision = decided.get(&t.id).and_then(|ds| ds.first());
        let trigger = match decision {
            Some(d) => Some(&d.trigger_ref),
            None => t.trigger_ref.as_ref(),
        };
        let Some(trigger) = trigger else {
            if rule.is_none() {
                rule = Some(transition_complete_rule(graph)?);
            }
            let Some(rule) = &rule else {
                continue;
            };
            let message = format!(
                "State transition candidate {} has no resolved Operation/Event trigger.",
                t.id
            );
            let finding = GeneratedFinding::for_violation(
                rule,
                rule.severity,
                &ViolationFacts {
                    targets: &transition_targets(t),
                    semantic_condition_key: &condition_key(&t.id),
                    message: &message,
                    suggested_resolution: Some(TRIGGER_RESOLUTION),
                },
                None,
            )
            .map_err(|e| LifecycleError::GeneratedFinding {
                reason: e.to_string(),
            })?;
            findings.insert(finding.id.clone(), finding);
            continue;
        };
        let evidence = builder.evidence(&[&t.grounding, &t.from_grounding, &t.to_grounding]);
        let origin = serde_json::to_value(StateOrigin::Transition {
            stateful_ref: t.stateful_ref.clone(),
            grounding: t.grounding.clone(),
            from_state_grounding: t.from_grounding.clone(),
            to_state_grounding: t.to_grounding.clone(),
            trigger_ref: trigger.clone(),
            trigger_decision_ref: decision.map(|d| d.decision_ref.clone()),
        })
        .map_err(invalid_proposal)?;
        let node = builder.node(
            t.id.clone(),
            payload,
            &evidence,
            STATE_ORIGIN_EXTENSION,
            origin,
        )?;
        let via = builder.edge(RelationKind::TransitionsVia, &t.id, trigger, &evidence)?;
        proposals.push(builder.propose(node, Some(via))?);
    }

    // Invariants.
    for i in &candidates.invariants {
        let payload = invariant_payload(&i.scope_ref, &i.expression);
        let origin = serde_json::to_value(InvariantOrigin {
            scope_ref: i.scope_ref.clone(),
            grounding: i.grounding.clone(),
        })
        .map_err(invalid_proposal)?;
        if existing_matches(graph, &i.id, &payload, INVARIANT_ORIGIN_EXTENSION, &origin)? {
            existing.insert(LifecycleExisting {
                candidate_ref: i.id.clone(),
                node_ref: i.id.clone(),
            });
            continue;
        }
        let evidence = builder.evidence(&[&i.grounding]);
        let node = builder.node(
            i.id.clone(),
            payload,
            &evidence,
            INVARIANT_ORIGIN_EXTENSION,
            origin,
        )?;
        proposals.push(builder.propose(node, None)?);
    }

    proposals.sort_by(|a, b| a.id.cmp(&b.id));
    result.issues.extend(issues);
    result.issues.sort();
    result.proposals = proposals;
    result.findings = findings.into_values().collect();
    result.conflicts = conflicts.into_iter().collect();
    result.existing = existing.into_iter().collect();
    Ok(result)
}
