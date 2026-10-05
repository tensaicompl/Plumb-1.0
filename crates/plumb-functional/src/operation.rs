//! AI-assisted Operations with typed relations, Outcomes and Events (plan S2.7; compiler
//! architecture §8).
//!
//! A supplied, already-acquired `operation_analysis` inference proposes Operations over an
//! explicit, closed [`OperationScope`] of Accepted PSG nodes. Every proposed relation names a
//! scope ID grounded in exact requirement byte ranges; unknown, out-of-scope or wrongly typed
//! references are artifact errors and nothing is resolved by name. Plumb checks queries for
//! writes, requires a performer, and verifies that the read set covers the Entity and
//! Attribute inputs of every selected Calculation by replaying its S2.6 qualification. Eligible
//! candidates become one HumanConfirm proposal each, holding the Operation, its new Outcomes
//! and Events and canonical edges; `apply_patch` is used solely to dry-validate in memory.
//! Nothing here emits findings, mutates a graph, calls a provider, reads a clock or persists.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AuditMeta, DerivationRef, Edge, ElementStatus, EvidenceRef, ExtensionKey, Graph, Node,
    NodePayload, NodeType, Operation, OperationKind, Outcome, OutcomeKind, RelationKind,
    RelationProperties,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

use crate::calculation::qualify_accepted_calculation;
use crate::event::{
    event_id, reconcile_event, EventOrigin, EventReconciliation, EVENT_ORIGIN_EXTENSION,
};

/// Version of [`OperationContext`].
pub const OPERATION_CONTEXT_VERSION: u32 = 1;

/// Version of the operation inference output.
pub const OPERATION_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of operation analysis.
pub const OPERATION_TASK_KIND: &str = "operation_analysis";

/// Extension key of the provenance [`OperationOrigin`].
pub const OPERATION_ORIGIN_EXTENSION: &str = "plumb_functional:operation_origin";

/// Extension key of the provenance [`OutcomeOrigin`].
pub const OUTCOME_ORIGIN_EXTENSION: &str = "plumb_functional:outcome_origin";

const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s2-operation.md");
const SCHEMA_SOURCE: &str = include_str!("../../../schemas/inference/s2-operation.schema.json");
const OPERATION_STAGE: StageId = StageId::S2;

// ============================================================================ errors and issues

/// Why operation analysis could not run. Malformed supplied inference, invalid scopes and
/// invalid requests are errors; semantic problems of a candidate are [`OperationIssue`]s.
#[derive(Debug, Error)]
pub enum OperationError {
    #[error("invalid operation input: {reason}")]
    InvalidInput { reason: String },
    #[error("invalid operation scope {field}: {reason}")]
    Scope {
        field: String,
        node_ref: Option<Id>,
        reason: String,
    },
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    #[error("invalid operation inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error("operation schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    #[error("operation output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("invalid grounding {requirement_ref} {start}..{end}: {reason}")]
    InvalidGrounding {
        requirement_ref: Id,
        start: u64,
        end: u64,
        reason: String,
    },
    #[error("{relation} target {target_ref} does not exist")]
    UnknownSemanticRef { relation: String, target_ref: Id },
    #[error("{relation} target {target_ref} is not in the operation scope")]
    OutOfScopeRef { relation: String, target_ref: Id },
    #[error("{relation} target {target_ref} has the wrong node type {node_type}")]
    WrongTargetType {
        relation: String,
        target_ref: Id,
        node_type: String,
    },
    #[error("duplicate {relation} target {target_ref}")]
    DuplicateRelationTarget { relation: String, target_ref: Id },
    #[error("duplicate candidate identity {node_ref}")]
    DuplicateCandidate { node_ref: Id },
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_input(reason: impl Into<String>) -> OperationError {
    OperationError::InvalidInput {
        reason: reason.into(),
    }
}

fn invalid_proposal(e: impl ToString) -> OperationError {
    OperationError::InvalidProposal {
        reason: e.to_string(),
    }
}

/// A deterministic candidate problem. These are analysis categories, not rule IDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationIssue {
    InvalidName {
        node_ref: Id,
        reason: String,
    },
    QueryWritesState {
        operation_ref: Id,
    },
    MissingPerformer {
        operation_ref: Id,
    },
    MissingCalculationRead {
        calculation_ref: Id,
        semantic_ref: Id,
    },
    CalculationReadAnalysisUnavailable {
        calculation_ref: Id,
        reason: String,
    },
    ExistingOperationConflict {
        operation_ref: Id,
    },
    ExistingOutcomeConflict {
        outcome_ref: Id,
    },
    ExistingEventConflict {
        event_ref: Id,
    },
    ExistingEventNameConflict {
        event_ref: Id,
        existing_ref: Id,
    },
    InvalidProposal {
        reason: String,
    },
    InferenceUnavailable,
}

impl OperationIssue {
    fn is_existing_conflict(&self) -> bool {
        matches!(
            self,
            OperationIssue::ExistingOperationConflict { .. }
                | OperationIssue::ExistingOutcomeConflict { .. }
                | OperationIssue::ExistingEventConflict { .. }
                | OperationIssue::ExistingEventNameConflict { .. }
        )
    }
}

// ============================================================================ scope

/// The explicit closed scope of Accepted nodes operation inference may reference. Every list
/// is canonically strictly sorted and unique.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationScope {
    /// Accepted Actor or BusinessRole.
    pub performer_refs: Vec<Id>,
    /// Accepted Entity or Attribute: the only read and write targets.
    pub domain_refs: Vec<Id>,
    /// Accepted Calculations whose S2.6 qualification replays from their origin.
    pub calculation_refs: Vec<Id>,
    /// Accepted Rule or DecisionTable.
    pub governor_refs: Vec<Id>,
    /// Accepted Events usable for `produces` and `consumes`.
    pub event_refs: Vec<Id>,
    /// Accepted DataSchemas usable as operation input/output and Event payload schemas.
    pub data_schema_refs: Vec<Id>,
    /// Accepted Attributes usable as a single-Attribute Event payload.
    pub event_payload_attribute_refs: Vec<Id>,
    /// Read-only Accepted AcceptanceCriterion, Invariant, Rule, DecisionTable or Calculation
    /// context; it never creates relations by itself.
    pub support_refs: Vec<Id>,
}

fn is_accepted(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
}

fn is_active(status: ElementStatus) -> bool {
    matches!(status, ElementStatus::Proposed | ElementStatus::Accepted)
}

fn scope_error(field: &str, node_ref: Option<&Id>, reason: impl Into<String>) -> OperationError {
    OperationError::Scope {
        field: field.to_owned(),
        node_ref: node_ref.cloned(),
        reason: reason.into(),
    }
}

/// Sorts one scope list, rejecting duplicates and anything not an Accepted node of `allowed`.
fn validated_list(
    graph: &Graph,
    field: &str,
    refs: &[Id],
    allowed: &[NodeType],
) -> Result<Vec<Id>, OperationError> {
    let mut sorted = refs.to_vec();
    sorted.sort();
    if let Some(pair) = sorted.windows(2).find(|p| p[0] == p[1]) {
        return Err(scope_error(field, Some(&pair[0]), "duplicate reference"));
    }
    for id in &sorted {
        let node = graph
            .node(id)
            .ok_or_else(|| scope_error(field, Some(id), "node does not exist"))?;
        if !is_accepted(node) {
            return Err(scope_error(field, Some(id), "node is not Accepted"));
        }
        let node_type = node.payload.node_type();
        if !allowed.contains(&node_type) {
            return Err(scope_error(
                field,
                Some(id),
                format!("node type {} is not allowed", node_type.as_str()),
            ));
        }
    }
    Ok(sorted)
}

fn validate_scope(graph: &Graph, scope: &OperationScope) -> Result<OperationScope, OperationError> {
    use NodeType as T;
    let calculation_refs = validated_list(
        graph,
        "calculation_refs",
        &scope.calculation_refs,
        &[T::Calculation],
    )?;
    for id in &calculation_refs {
        // Usable without a legacy override: the S2.6 origin replays the scope.
        let q = qualify_accepted_calculation(graph, id, None)
            .map_err(|e| scope_error("calculation_refs", Some(id), e.to_string()))?;
        if !q.from_origin {
            return Err(scope_error(
                "calculation_refs",
                Some(id),
                "legacy Calculation without an S2.6 origin; its scope is unavailable",
            ));
        }
    }
    Ok(OperationScope {
        performer_refs: validated_list(
            graph,
            "performer_refs",
            &scope.performer_refs,
            &[T::Actor, T::BusinessRole],
        )?,
        domain_refs: validated_list(
            graph,
            "domain_refs",
            &scope.domain_refs,
            &[T::Entity, T::Attribute],
        )?,
        calculation_refs,
        governor_refs: validated_list(
            graph,
            "governor_refs",
            &scope.governor_refs,
            &[T::Rule, T::DecisionTable],
        )?,
        event_refs: validated_list(graph, "event_refs", &scope.event_refs, &[T::Event])?,
        data_schema_refs: validated_list(
            graph,
            "data_schema_refs",
            &scope.data_schema_refs,
            &[T::DataSchema],
        )?,
        event_payload_attribute_refs: validated_list(
            graph,
            "event_payload_attribute_refs",
            &scope.event_payload_attribute_refs,
            &[T::Attribute],
        )?,
        support_refs: validated_list(
            graph,
            "support_refs",
            &scope.support_refs,
            &[
                T::AcceptanceCriterion,
                T::Invariant,
                T::Rule,
                T::DecisionTable,
                T::Calculation,
            ],
        )?,
    })
}

/// The canonical Accepted Entity owning an Attribute through an Accepted `has_attribute`.
fn accepted_owner<'g>(graph: &'g Graph, attribute: &Id) -> Option<&'g Id> {
    let owners: Vec<&Id> = graph
        .incoming_edge_ids(attribute)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == RelationKind::HasAttribute && e.status == ElementStatus::Accepted)
        .map(|e| &e.from)
        .filter(|from| {
            graph
                .node(from)
                .is_some_and(|n| is_accepted(n) && matches!(n.payload, NodePayload::Entity(_)))
        })
        .collect();
    match owners.as_slice() {
        [only] => Some(only),
        _ => None,
    }
}

// ============================================================================ request

/// One target Requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRequirementContext {
    pub requirement_ref: Id,
    pub status: ElementStatus,
    pub statement: String,
}

/// One scope node with its typed payload; `owner_ref` is the canonical owning Entity of an
/// Attribute and null otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationNodeContext {
    pub node_ref: Id,
    pub owner_ref: Option<Id>,
    pub payload: JsonValue,
}

/// A deterministic summary of one available Calculation; it is never evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationCalculationContext {
    pub calculation_ref: Id,
    pub name: String,
    pub result_type: String,
    pub unit: Option<String>,
    pub qualified: bool,
    pub dependencies: Vec<Id>,
}

/// The exact operation inference context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationContext {
    pub version: u32,
    pub project_id: Id,
    pub requirements: Vec<OperationRequirementContext>,
    pub performers: Vec<OperationNodeContext>,
    pub domain: Vec<OperationNodeContext>,
    pub calculations: Vec<OperationCalculationContext>,
    pub governors: Vec<OperationNodeContext>,
    pub events: Vec<OperationNodeContext>,
    pub data_schemas: Vec<OperationNodeContext>,
    pub event_payload_attributes: Vec<OperationNodeContext>,
    pub support: Vec<OperationNodeContext>,
}

impl OperationContext {
    /// Generic SHA-256 of the RFC 8785 canonical JSON of the context.
    pub fn content_hash(&self) -> Result<Hash, OperationError> {
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn requirement(&self, id: &Id) -> Option<&OperationRequirementContext> {
        self.requirements
            .binary_search_by(|r| r.requirement_ref.cmp(id))
            .ok()
            .map(|i| &self.requirements[i])
    }

    fn input_refs(&self) -> Vec<Id> {
        let nodes = [
            &self.performers,
            &self.domain,
            &self.governors,
            &self.events,
            &self.data_schemas,
            &self.event_payload_attributes,
            &self.support,
        ];
        let refs: BTreeSet<&Id> = self
            .requirements
            .iter()
            .map(|r| &r.requirement_ref)
            .chain(nodes.into_iter().flatten().map(|n| &n.node_ref))
            .chain(self.calculations.iter().map(|c| &c.calculation_ref))
            .collect();
        refs.into_iter().cloned().collect()
    }
}

/// An operation `InferenceRequest` with its exact context and validated scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRequest {
    pub request: InferenceRequest,
    pub context: OperationContext,
    pub scope: OperationScope,
}

fn node_contexts(graph: &Graph, refs: &[Id]) -> Result<Vec<OperationNodeContext>, OperationError> {
    refs.iter()
        .map(|id| {
            let node = graph
                .node(id)
                .ok_or_else(|| invalid_input(format!("{id} does not exist")))?;
            let owner_ref = match node.payload {
                NodePayload::Attribute(_) => accepted_owner(graph, id).cloned(),
                _ => None,
            };
            Ok(OperationNodeContext {
                node_ref: id.clone(),
                owner_ref,
                payload: serde_json::to_value(&node.payload)
                    .map_err(|e| invalid_input(e.to_string()))?,
            })
        })
        .collect()
}

fn context_of(
    graph: &Graph,
    target_requirement_refs: &[Id],
    scope: &OperationScope,
) -> Result<(OperationContext, OperationScope), OperationError> {
    let scope = validate_scope(graph, scope)?;
    let mut targets = BTreeSet::new();
    for id in target_requirement_refs {
        if !targets.insert(id) {
            return Err(invalid_input(format!("duplicate target requirement {id}")));
        }
    }
    if targets.is_empty() {
        return Err(invalid_input("no target requirement"));
    }
    let mut requirements = Vec::new();
    for id in targets {
        let node = graph
            .node(id)
            .ok_or_else(|| invalid_input(format!("requirement {id} does not exist")))?;
        let NodePayload::Requirement(r) = &node.payload else {
            return Err(invalid_input(format!("{id} is not a Requirement")));
        };
        if !is_active(node.status) {
            return Err(invalid_input(format!(
                "requirement {id} is not Proposed or Accepted"
            )));
        }
        requirements.push(OperationRequirementContext {
            requirement_ref: id.clone(),
            status: node.status,
            statement: r.statement.clone(),
        });
    }
    let mut calculations = Vec::new();
    for id in &scope.calculation_refs {
        let Some(NodePayload::Calculation(c)) = graph.node(id).map(|n| &n.payload) else {
            return Err(invalid_input(format!("{id} is not a Calculation")));
        };
        let q = qualify_accepted_calculation(graph, id, None)
            .map_err(|e| invalid_input(e.to_string()))?;
        calculations.push(OperationCalculationContext {
            calculation_ref: id.clone(),
            name: c.name.clone(),
            result_type: c.result_type.clone(),
            unit: c.unit.clone(),
            qualified: q.qualification.is_qualified(),
            dependencies: q.qualification.dependencies.clone(),
        });
    }
    let context = OperationContext {
        version: OPERATION_CONTEXT_VERSION,
        project_id: graph.project_id().clone(),
        requirements,
        performers: node_contexts(graph, &scope.performer_refs)?,
        domain: node_contexts(graph, &scope.domain_refs)?,
        calculations,
        governors: node_contexts(graph, &scope.governor_refs)?,
        events: node_contexts(graph, &scope.event_refs)?,
        data_schemas: node_contexts(graph, &scope.data_schema_refs)?,
        event_payload_attributes: node_contexts(graph, &scope.event_payload_attribute_refs)?,
        support: node_contexts(graph, &scope.support_refs)?,
    };
    Ok((context, scope))
}

/// The EvidenceRefs of the target Requirements and support context, sorted and unique.
fn context_evidence(graph: &Graph, context: &OperationContext) -> Vec<Id> {
    let refs: BTreeSet<Id> = context
        .requirements
        .iter()
        .map(|r| &r.requirement_ref)
        .chain(context.support.iter().map(|s| &s.node_ref))
        .filter_map(|id| graph.node(id))
        .flat_map(|n| n.evidence.iter().map(|e| e.as_id().clone()))
        .collect();
    refs.into_iter().collect()
}

/// Builds the S2 operation request over explicit target Requirements and an explicit scope.
pub fn build_operation_request(
    graph: &Graph,
    target_requirement_refs: &[Id],
    scope: &OperationScope,
    provider_policy: ProviderPolicy,
) -> Result<OperationRequest, OperationError> {
    let (context, scope) = context_of(graph, target_requirement_refs, scope)?;
    let request = InferenceRequest::new(
        OPERATION_STAGE,
        OPERATION_TASK_KIND.to_owned(),
        context.input_refs(),
        context_evidence(graph, &context),
        context.content_hash()?,
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    Ok(OperationRequest {
        request,
        context,
        scope,
    })
}

impl OperationRequest {
    /// The request is bound to its context, the committed prompt and schema.
    pub fn validate(&self) -> Result<(), OperationError> {
        let r = &self.request;
        r.validate()
            .map_err(|e| invalid_input(format!("invalid inference request: {e}")))?;
        let ok = r.stage == OPERATION_STAGE
            && r.task_kind == OPERATION_TASK_KIND
            && r.input_refs == self.context.input_refs()
            && r.context_hash == self.context.content_hash()?
            && r.prompt_template_hash == Hash::content_sha256(PROMPT_TEMPLATE)
            && r.schema_hash == Hash::content_sha256(SCHEMA_SOURCE.as_bytes())
            && self.context.version == OPERATION_CONTEXT_VERSION;
        if ok {
            Ok(())
        } else {
            Err(invalid_input(
                "request does not match its context, prompt or schema",
            ))
        }
    }
}

// ============================================================================ output

/// A zero-based, end-exclusive UTF-8 byte range of a target Requirement statement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationGroundedRange {
    pub requirement_ref: Id,
    pub start: u64,
    pub end: u64,
}

/// Provenance of a proposed Operation (`plumb_functional:operation_origin`). Relations are
/// canonical edges and are not duplicated here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationOrigin {
    pub evidence: Vec<OperationGroundedRange>,
}

/// Provenance of a proposed Outcome (`plumb_functional:outcome_origin`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeOrigin {
    pub operation_ref: Id,
    pub evidence: Vec<OperationGroundedRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationOutput {
    version: u32,
    operations: Vec<OutputOperation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputGroundedRef {
    target_ref: Id,
    evidence: Vec<OperationGroundedRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputOutcome {
    name: String,
    outcome_kind: OutcomeKind,
    evidence: Vec<OperationGroundedRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputEvent {
    name: String,
    payload_schema_ref: Option<Id>,
    evidence: Vec<OperationGroundedRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputOperation {
    name: String,
    operation_kind: OperationKind,
    evidence: Vec<OperationGroundedRange>,
    performers: Vec<OutputGroundedRef>,
    reads: Vec<OutputGroundedRef>,
    writes: Vec<OutputGroundedRef>,
    governed_by: Vec<OutputGroundedRef>,
    uses_calculation: Vec<OutputGroundedRef>,
    input_schema: Option<OutputGroundedRef>,
    output_schema: Option<OutputGroundedRef>,
    outcomes: Vec<OutputOutcome>,
    new_produced_events: Vec<OutputEvent>,
    produced_event_refs: Vec<OutputGroundedRef>,
    consumed_event_refs: Vec<OutputGroundedRef>,
}

fn compile_schema() -> Result<JSONSchema, OperationError> {
    let schema: JsonValue =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| OperationError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| OperationError::SchemaCompilation {
            reason: e.to_string(),
        })
}

/// Validates evidence ranges against the target statements; returns them sorted.
fn grounded_evidence(
    context: &OperationContext,
    ranges: &[OperationGroundedRange],
) -> Result<Vec<OperationGroundedRange>, OperationError> {
    let invalid = |g: &OperationGroundedRange, reason: &str| OperationError::InvalidGrounding {
        requirement_ref: g.requirement_ref.clone(),
        start: g.start,
        end: g.end,
        reason: reason.to_owned(),
    };
    for g in ranges {
        let requirement = context
            .requirement(&g.requirement_ref)
            .ok_or_else(|| invalid(g, "not a target requirement"))?;
        let text = &requirement.statement;
        let (s, e) = match (usize::try_from(g.start), usize::try_from(g.end)) {
            (Ok(s), Ok(e)) if s < e && e <= text.len() => (s, e),
            _ => return Err(invalid(g, "not a non-empty range of the statement")),
        };
        if !text.is_char_boundary(s) || !text.is_char_boundary(e) {
            return Err(invalid(g, "not on UTF-8 character boundaries"));
        }
    }
    let mut sorted = ranges.to_vec();
    sorted.sort();
    if let Some(pair) = sorted.windows(2).find(|p| p[0] == p[1]) {
        return Err(invalid(&pair[0], "duplicate evidence range"));
    }
    if sorted.is_empty() {
        return Err(invalid_input("empty evidence"));
    }
    Ok(sorted)
}

/// One validated grounded relation target.
#[derive(Debug, Clone)]
struct Grounded {
    target: Id,
    evidence: Vec<OperationGroundedRange>,
}

/// Checks that `target` is in one of `lists`; otherwise reports it as unknown, out of scope
/// (right type, not exposed) or wrongly typed.
fn resolve(
    graph: &Graph,
    relation: &str,
    lists: &[&[Id]],
    allowed: &[NodeType],
    target: &Id,
) -> Result<(), OperationError> {
    if lists.iter().any(|l| l.binary_search(target).is_ok()) {
        return Ok(());
    }
    match graph.node(target) {
        None => Err(OperationError::UnknownSemanticRef {
            relation: relation.to_owned(),
            target_ref: target.clone(),
        }),
        Some(n) if allowed.contains(&n.payload.node_type()) => Err(OperationError::OutOfScopeRef {
            relation: relation.to_owned(),
            target_ref: target.clone(),
        }),
        Some(n) => Err(OperationError::WrongTargetType {
            relation: relation.to_owned(),
            target_ref: target.clone(),
            node_type: n.payload.node_type().as_str().to_owned(),
        }),
    }
}

/// Validates one relation family: grounding, closed-scope targets and unique targets.
fn family(
    graph: &Graph,
    context: &OperationContext,
    relation: &str,
    refs: &[OutputGroundedRef],
    list: &[Id],
    allowed: &[NodeType],
) -> Result<Vec<Grounded>, OperationError> {
    let mut out: Vec<Grounded> = Vec::new();
    for r in refs {
        resolve(graph, relation, &[list], allowed, &r.target_ref)?;
        out.push(Grounded {
            target: r.target_ref.clone(),
            evidence: grounded_evidence(context, &r.evidence)?,
        });
    }
    out.sort_by(|a, b| a.target.cmp(&b.target));
    if let Some(pair) = out.windows(2).find(|p| p[0].target == p[1].target) {
        return Err(OperationError::DuplicateRelationTarget {
            relation: relation.to_owned(),
            target_ref: pair[0].target.clone(),
        });
    }
    Ok(out)
}

fn targets(list: &[Grounded]) -> Vec<Id> {
    list.iter().map(|g| g.target.clone()).collect()
}

/// Accepted semantic names are trimmed, non-empty and free of control characters.
fn name_problem(name: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        Some("the name is empty")
    } else if name.trim() != name {
        Some("the name is not trimmed")
    } else if name.chars().any(char::is_control) {
        Some("the name contains control characters")
    } else {
        None
    }
}

fn short_id(prefix: &str, body: &JsonValue) -> Result<Id, CoreError> {
    let digest = Hash::content_sha256(&to_canonical_json(body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("{prefix}:{}", &hex[..16]).parse()
}

/// `operation:<16 hex of SHA-256(RFC 8785 {project_id, name, node_type: operation})>`.
fn operation_id(project_id: &Id, name: &str) -> Result<Id, CoreError> {
    short_id(
        "operation",
        &json!({"project_id": project_id, "name": name, "node_type": "operation"}),
    )
}

/// `outcome:<16 hex of SHA-256(RFC 8785 {project_id, operation_ref, name, node_type: outcome})>`.
fn outcome_id(project_id: &Id, operation_ref: &Id, name: &str) -> Result<Id, CoreError> {
    short_id(
        "outcome",
        &json!({
            "project_id": project_id,
            "operation_ref": operation_ref,
            "name": name,
            "node_type": "outcome"
        }),
    )
}

/// `rel:<16 hex of SHA-256(RFC 8785 {kind, from, to})>`.
fn edge_id(kind: &RelationKind, from: &Id, to: &Id) -> Result<Id, CoreError> {
    short_id("rel", &json!({"kind": kind, "from": from, "to": to}))
}

// ============================================================================ calculation reads

/// The Entity and Attribute inputs a selected Calculation structurally requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationReadAnalysis {
    pub calculation_ref: Id,
    /// Sorted by node ID.
    pub required_reads: Vec<Id>,
}

fn visit_calculation(
    graph: &Graph,
    calculation_ref: &Id,
    stack: &mut Vec<Id>,
    done: &mut BTreeSet<Id>,
    out: &mut BTreeSet<Id>,
) -> Result<(), String> {
    if stack.contains(calculation_ref) {
        return Err(format!("{calculation_ref} is in a dependency cycle"));
    }
    if done.contains(calculation_ref) {
        return Ok(());
    }
    let q = qualify_accepted_calculation(graph, calculation_ref, None)
        .map_err(|e| format!("{calculation_ref}: {e}"))?;
    if !q.from_origin {
        return Err(format!("{calculation_ref} has no replayable S2.6 origin"));
    }
    if !q.qualification.is_qualified() {
        return Err(format!(
            "{calculation_ref} is not qualified: {:?}",
            q.qualification.issues
        ));
    }
    stack.push(calculation_ref.clone());
    for binding in &q.qualification.used_bindings {
        let is_domain = graph.node(&binding.node_ref).is_some_and(|n| {
            matches!(
                n.payload,
                NodePayload::Entity(_) | NodePayload::Attribute(_)
            )
        });
        if is_domain {
            out.insert(binding.node_ref.clone());
        }
    }
    for dependency in &q.qualification.dependencies {
        visit_calculation(graph, dependency, stack, done, out)?;
    }
    stack.pop();
    done.insert(calculation_ref.clone());
    Ok(())
}

/// Replays the S2.6 qualification of a Calculation and its transitive Calculation
/// dependencies and collects their Entity and Attribute inputs.
fn calculation_reads(graph: &Graph, calculation_ref: &Id) -> Result<Vec<Id>, String> {
    let mut out = BTreeSet::new();
    visit_calculation(
        graph,
        calculation_ref,
        &mut Vec::new(),
        &mut BTreeSet::new(),
        &mut out,
    )?;
    Ok(out.into_iter().collect())
}

// ============================================================================ analysis

/// An already-acquired operation artifact and the caller's provenance ref for it.
#[derive(Debug, Clone)]
pub struct OperationInference<'a> {
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

/// Who owns the Proposed nodes and edges and when, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// What happened to one inferred Operation candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationDisposition {
    Proposed { proposal_ref: Id },
    Ineligible,
    IdempotentReplay,
    ExistingEquivalent,
    ExistingConflict,
}

/// One validated Outcome of a candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationOutcomeAnalysis {
    pub outcome_ref: Id,
    pub name: String,
    pub outcome_kind: OutcomeKind,
}

/// One validated new produced Event of a candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationEventAnalysis {
    pub event_ref: Id,
    pub name: String,
    pub payload_schema_ref: Option<Id>,
    /// Whether an existing compatible Event is reused instead of proposing a new node.
    pub reused: bool,
}

/// One inferred Operation candidate with its deterministic identity and canonical material.
/// All ID lists are sorted.
#[derive(Debug, Clone)]
pub struct OperationCandidateAnalysis {
    pub operation_ref: Id,
    pub name: String,
    pub operation_kind: OperationKind,
    pub performers: Vec<Id>,
    pub reads: Vec<Id>,
    pub writes: Vec<Id>,
    pub governed_by: Vec<Id>,
    pub uses_calculation: Vec<Id>,
    pub input_schema_ref: Option<Id>,
    pub output_schema_ref: Option<Id>,
    pub outcomes: Vec<OperationOutcomeAnalysis>,
    pub new_events: Vec<OperationEventAnalysis>,
    /// Every Event the Operation produces (new or selected).
    pub produced_events: Vec<Id>,
    pub consumed_events: Vec<Id>,
    pub calculation_reads: Vec<CalculationReadAnalysis>,
    /// Requirements traced by `specified_by`.
    pub specified_by: Vec<Id>,
    pub issues: Vec<OperationIssue>,
    pub disposition: OperationDisposition,
}

/// Everything one analysis produced; nothing has been applied or persisted.
#[derive(Debug, Clone)]
pub struct OperationAnalysisResult {
    pub candidates: Vec<OperationCandidateAnalysis>,
    pub proposals: Vec<Proposal>,
    pub issues: Vec<OperationIssue>,
}

/// One planned edge.
struct PlannedEdge {
    kind: RelationKind,
    from: Id,
    to: Id,
    ranges: Vec<OperationGroundedRange>,
    /// `specified_by` uses the evidence of its Requirement itself.
    requirement_evidence: bool,
}

const OPERATION_OUT_KINDS: [RelationKind; 7] = [
    RelationKind::PerformedBy,
    RelationKind::Reads,
    RelationKind::Writes,
    RelationKind::Produces,
    RelationKind::Consumes,
    RelationKind::GovernedBy,
    RelationKind::UsesCalculation,
];

type RelationKey = (String, Id, Id);

fn relation_key(kind: &RelationKind, from: &Id, to: &Id) -> Result<RelationKey, OperationError> {
    let kind = serde_json::to_value(kind)
        .map_err(|e| invalid_input(e.to_string()))?
        .as_str()
        .unwrap_or_default()
        .to_owned();
    Ok((kind, from.clone(), to.clone()))
}

/// The active relations of an existing Operation: its outgoing operation relations and the
/// incoming `specified_by` traces.
fn existing_relations(
    graph: &Graph,
    operation: &Id,
) -> Result<BTreeSet<RelationKey>, OperationError> {
    let mut out = BTreeSet::new();
    for e in graph
        .outgoing_edge_ids(operation)
        .iter()
        .chain(graph.incoming_edge_ids(operation))
        .filter_map(|id| graph.edge(id))
    {
        let relevant = if &e.from == operation {
            OPERATION_OUT_KINDS.contains(&e.kind)
        } else {
            e.kind == RelationKind::SpecifiedBy
        };
        if relevant && is_active(e.status) {
            out.insert(relation_key(&e.kind, &e.from, &e.to)?);
        }
    }
    Ok(out)
}

struct Builder<'a> {
    graph: &'a Graph,
    base_semantic_hash: Hash,
    derivation_ref: &'a DerivationRef,
    audit: &'a OperationAudit,
}

impl Builder<'_> {
    fn audit_meta(&self) -> Result<AuditMeta, OperationError> {
        AuditMeta::new(
            self.audit.created_by.clone(),
            self.audit.created_at,
            None,
            None,
        )
        .map_err(invalid_proposal)
    }

    /// The sorted unique union of `Node.evidence` of the grounding Requirements.
    fn evidence(&self, ranges: &[OperationGroundedRange]) -> Vec<EvidenceRef> {
        let refs: BTreeSet<&EvidenceRef> = ranges
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
        ranges: &[OperationGroundedRange],
        extension: &str,
        origin: JsonValue,
    ) -> Result<Node, OperationError> {
        let key: ExtensionKey = extension.parse().map_err(invalid_proposal)?;
        let node = Node {
            id,
            revision: 1,
            status: ElementStatus::Proposed,
            payload,
            evidence: self.evidence(ranges),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::from([(key, origin)]),
            audit: self.audit_meta()?,
        };
        node.validate().map_err(invalid_proposal)?;
        Ok(node)
    }

    fn edge(&self, planned: &PlannedEdge) -> Result<Edge, OperationError> {
        let evidence = if planned.requirement_evidence {
            self.graph
                .node(&planned.from)
                .map(|n| {
                    let set: BTreeSet<&EvidenceRef> = n.evidence.iter().collect();
                    set.into_iter().cloned().collect()
                })
                .unwrap_or_default()
        } else {
            self.evidence(&planned.ranges)
        };
        Ok(Edge {
            id: edge_id(&planned.kind, &planned.from, &planned.to)?,
            revision: 1,
            status: ElementStatus::Proposed,
            kind: planned.kind.clone(),
            from: planned.from.clone(),
            to: planned.to.clone(),
            properties: RelationProperties::None,
            evidence,
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: self.audit_meta()?,
        })
    }

    /// One Compound proposal: the new nodes (Operation first, then by ID) and the edges by ID.
    fn propose(&self, nodes: Vec<Node>, mut edges: Vec<Edge>) -> Result<Proposal, OperationError> {
        let mut evidence: BTreeSet<EvidenceRef> = BTreeSet::new();
        for n in &nodes {
            evidence.extend(n.evidence.iter().cloned());
        }
        for e in &edges {
            evidence.extend(e.evidence.iter().cloned());
        }
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        let patches = nodes
            .into_iter()
            .map(|node| SemanticPatch::AddNode { node })
            .chain(
                edges
                    .into_iter()
                    .map(|edge| SemanticPatch::AddEdge { edge }),
            )
            .collect();
        Proposal::new(
            OPERATION_STAGE,
            PatchSet {
                base_semantic_hash: self.base_semantic_hash.clone(),
                patch: SemanticPatch::Compound { patches },
            },
            evidence.into_iter().collect(),
            vec![self.derivation_ref.clone()],
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .map_err(invalid_proposal)
    }
}

fn origin_json(value: impl Serialize) -> Result<JsonValue, OperationError> {
    serde_json::to_value(value).map_err(invalid_proposal)
}

/// Analyzes a supplied operation inference against its request. Absent inference yields only
/// InferenceUnavailable; malformed inference is an error.
pub fn analyze_operations(
    graph: &Graph,
    request: &OperationRequest,
    inference: Option<OperationInference<'_>>,
    audit: &OperationAudit,
) -> Result<OperationAnalysisResult, OperationError> {
    use NodeType as T;
    request.validate()?;
    let target_refs: Vec<Id> = request
        .context
        .requirements
        .iter()
        .map(|r| r.requirement_ref.clone())
        .collect();
    let (context, scope) = context_of(graph, &target_refs, &request.scope)?;
    if context != request.context || scope != request.scope {
        return Err(invalid_input(
            "graph does not match the operation request context",
        ));
    }
    if request.request.evidence_refs != context_evidence(graph, &context) {
        return Err(invalid_input(
            "evidence_refs differ from the context evidence",
        ));
    }
    let Some(inference) = inference else {
        return Ok(OperationAnalysisResult {
            candidates: Vec::new(),
            proposals: Vec::new(),
            issues: vec![OperationIssue::InferenceUnavailable],
        });
    };
    inference
        .artifact
        .validate_for(&request.request)
        .map_err(|e| OperationError::InvalidInferenceArtifact {
            reason: e.to_string(),
        })?;
    let output = inference.artifact.validated_output.as_value();
    if let Err(errors) = compile_schema()?.validate(output) {
        let mut messages: Vec<String> = errors
            .map(|e| format!("{e} at {}", e.instance_path))
            .collect();
        messages.sort();
        return Err(OperationError::SchemaInvalid {
            reason: messages.join("; "),
        });
    }
    let decoded: OperationOutput =
        serde_json::from_value(output.clone()).map_err(|e| OperationError::SchemaInvalid {
            reason: e.to_string(),
        })?;
    if decoded.version != OPERATION_OUTPUT_VERSION {
        return Err(OperationError::SchemaInvalid {
            reason: format!("output version {} is not 1", decoded.version),
        });
    }
    let project = graph.project_id();
    let builder = Builder {
        graph,
        base_semantic_hash: graph.semantic_hash()?,
        derivation_ref: &inference.derivation_ref,
        audit,
    };
    let mut candidates = Vec::new();
    let mut proposals = Vec::new();
    let mut seen = BTreeSet::new();
    for c in decoded.operations {
        // ------------------------------------------------ artifact contract (hard errors)
        let evidence = grounded_evidence(&context, &c.evidence)?;
        let operation_ref = operation_id(project, &c.name)?;
        if !seen.insert(operation_ref.clone()) {
            return Err(OperationError::DuplicateCandidate {
                node_ref: operation_ref,
            });
        }
        let performers = family(
            graph,
            &context,
            "performed_by",
            &c.performers,
            &scope.performer_refs,
            &[T::Actor, T::BusinessRole],
        )?;
        let domain = [T::Entity, T::Attribute];
        let reads = family(
            graph,
            &context,
            "reads",
            &c.reads,
            &scope.domain_refs,
            &domain,
        )?;
        let writes = family(
            graph,
            &context,
            "writes",
            &c.writes,
            &scope.domain_refs,
            &domain,
        )?;
        let governed_by = family(
            graph,
            &context,
            "governed_by",
            &c.governed_by,
            &scope.governor_refs,
            &[T::Rule, T::DecisionTable],
        )?;
        let uses_calculation = family(
            graph,
            &context,
            "uses_calculation",
            &c.uses_calculation,
            &scope.calculation_refs,
            &[T::Calculation],
        )?;
        let produced_refs = family(
            graph,
            &context,
            "produces",
            &c.produced_event_refs,
            &scope.event_refs,
            &[T::Event],
        )?;
        let consumed = family(
            graph,
            &context,
            "consumes",
            &c.consumed_event_refs,
            &scope.event_refs,
            &[T::Event],
        )?;
        let schema = |relation: &str, r: &Option<OutputGroundedRef>| match r {
            None => Ok(None),
            Some(r) => family(
                graph,
                &context,
                relation,
                std::slice::from_ref(r),
                &scope.data_schema_refs,
                &[T::DataSchema],
            )
            .map(|mut v| v.pop()),
        };
        let input_schema = schema("input_schema", &c.input_schema)?;
        let output_schema = schema("output_schema", &c.output_schema)?;

        let mut issues = Vec::new();
        if let Some(reason) = name_problem(&c.name) {
            issues.push(OperationIssue::InvalidName {
                node_ref: operation_ref.clone(),
                reason: reason.to_owned(),
            });
        }

        let mut outcomes: Vec<(OperationOutcomeAnalysis, Vec<OperationGroundedRange>)> = Vec::new();
        for o in &c.outcomes {
            let ranges = grounded_evidence(&context, &o.evidence)?;
            let outcome_ref = outcome_id(project, &operation_ref, &o.name)?;
            if let Some(reason) = name_problem(&o.name) {
                issues.push(OperationIssue::InvalidName {
                    node_ref: outcome_ref.clone(),
                    reason: reason.to_owned(),
                });
            }
            outcomes.push((
                OperationOutcomeAnalysis {
                    outcome_ref,
                    name: o.name.clone(),
                    outcome_kind: o.outcome_kind,
                },
                ranges,
            ));
        }
        outcomes.sort_by(|a, b| a.0.outcome_ref.cmp(&b.0.outcome_ref));
        if let Some(pair) = outcomes
            .windows(2)
            .find(|p| p[0].0.outcome_ref == p[1].0.outcome_ref)
        {
            return Err(OperationError::DuplicateCandidate {
                node_ref: pair[0].0.outcome_ref.clone(),
            });
        }

        let mut new_events: Vec<(OperationEventAnalysis, Vec<OperationGroundedRange>)> = Vec::new();
        for e in &c.new_produced_events {
            let ranges = grounded_evidence(&context, &e.evidence)?;
            if let Some(payload) = &e.payload_schema_ref {
                resolve(
                    graph,
                    "event_payload",
                    &[&scope.data_schema_refs, &scope.event_payload_attribute_refs],
                    &[T::DataSchema, T::Attribute],
                    payload,
                )?;
            }
            let event_ref = event_id(project, &e.name)?;
            if let Some(reason) = name_problem(&e.name) {
                issues.push(OperationIssue::InvalidName {
                    node_ref: event_ref.clone(),
                    reason: reason.to_owned(),
                });
            }
            new_events.push((
                OperationEventAnalysis {
                    event_ref,
                    name: e.name.clone(),
                    payload_schema_ref: e.payload_schema_ref.clone(),
                    reused: false,
                },
                ranges,
            ));
        }
        new_events.sort_by(|a, b| a.0.event_ref.cmp(&b.0.event_ref));
        if let Some(pair) = new_events
            .windows(2)
            .find(|p| p[0].0.event_ref == p[1].0.event_ref)
        {
            return Err(OperationError::DuplicateCandidate {
                node_ref: pair[0].0.event_ref.clone(),
            });
        }
        for (e, _) in &new_events {
            if produced_refs.iter().any(|g| g.target == e.event_ref) {
                return Err(OperationError::DuplicateRelationTarget {
                    relation: "produces".into(),
                    target_ref: e.event_ref.clone(),
                });
            }
        }

        // ------------------------------------------------ semantic qualification (issues)
        if c.operation_kind == OperationKind::Query && !writes.is_empty() {
            issues.push(OperationIssue::QueryWritesState {
                operation_ref: operation_ref.clone(),
            });
        }
        if performers.is_empty() {
            issues.push(OperationIssue::MissingPerformer {
                operation_ref: operation_ref.clone(),
            });
        }
        let read_set: BTreeSet<&Id> = reads.iter().map(|g| &g.target).collect();
        let mut calculation_read_analyses = Vec::new();
        for calculation in &uses_calculation {
            match calculation_reads(graph, &calculation.target) {
                Err(reason) => issues.push(OperationIssue::CalculationReadAnalysisUnavailable {
                    calculation_ref: calculation.target.clone(),
                    reason,
                }),
                Ok(required) => {
                    for semantic_ref in &required {
                        let covered = read_set.contains(semantic_ref)
                            || matches!(
                                graph.node(semantic_ref).map(|n| &n.payload),
                                Some(NodePayload::Attribute(_))
                            ) && accepted_owner(graph, semantic_ref)
                                .is_some_and(|owner| read_set.contains(owner));
                        if !covered {
                            issues.push(OperationIssue::MissingCalculationRead {
                                calculation_ref: calculation.target.clone(),
                                semantic_ref: semantic_ref.clone(),
                            });
                        }
                    }
                    calculation_read_analyses.push(CalculationReadAnalysis {
                        calculation_ref: calculation.target.clone(),
                        required_reads: required,
                    });
                }
            }
        }

        // ------------------------------------------------ reconciliation of children
        let mut reused_outcomes = BTreeSet::new();
        for (o, _) in &outcomes {
            let payload = NodePayload::Outcome(Outcome {
                name: o.name.clone(),
                outcome_kind: o.outcome_kind,
            });
            match graph.node(&o.outcome_ref) {
                None => {}
                Some(existing) if is_active(existing.status) && existing.payload == payload => {
                    reused_outcomes.insert(o.outcome_ref.clone());
                }
                Some(_) => issues.push(OperationIssue::ExistingOutcomeConflict {
                    outcome_ref: o.outcome_ref.clone(),
                }),
            }
        }
        for (e, _) in &mut new_events {
            match reconcile_event(graph, &e.event_ref, &e.name, e.payload_schema_ref.as_ref()) {
                Ok(EventReconciliation::New) => {}
                Ok(EventReconciliation::Reuse) => e.reused = true,
                Err(issue) => issues.push(issue),
            }
        }

        // ------------------------------------------------ planned material
        let payload = NodePayload::Operation(Operation {
            name: c.name.clone(),
            operation_kind: c.operation_kind,
            input_schema_ref: input_schema.as_ref().map(|g| g.target.clone()),
            output_schema_ref: output_schema.as_ref().map(|g| g.target.clone()),
            preconditions: None,
            postconditions: None,
            idempotency: None,
            transaction_semantics: None,
        });
        let mut planned = Vec::new();
        let mut plan = |kind: RelationKind, list: &[Grounded]| {
            for g in list {
                planned.push(PlannedEdge {
                    kind: kind.clone(),
                    from: operation_ref.clone(),
                    to: g.target.clone(),
                    ranges: g.evidence.clone(),
                    requirement_evidence: false,
                });
            }
        };
        plan(RelationKind::PerformedBy, &performers);
        plan(RelationKind::Reads, &reads);
        plan(RelationKind::Writes, &writes);
        plan(RelationKind::GovernedBy, &governed_by);
        plan(RelationKind::UsesCalculation, &uses_calculation);
        plan(RelationKind::Produces, &produced_refs);
        plan(RelationKind::Consumes, &consumed);
        for (o, ranges) in &outcomes {
            planned.push(PlannedEdge {
                kind: RelationKind::Produces,
                from: operation_ref.clone(),
                to: o.outcome_ref.clone(),
                ranges: ranges.clone(),
                requirement_evidence: false,
            });
        }
        for (e, ranges) in &new_events {
            planned.push(PlannedEdge {
                kind: RelationKind::Produces,
                from: operation_ref.clone(),
                to: e.event_ref.clone(),
                ranges: ranges.clone(),
                requirement_evidence: false,
            });
        }
        // Every target Requirement in the candidate's overall evidence is traced.
        let specified_by: Vec<Id> = evidence
            .iter()
            .chain(planned.iter().flat_map(|p| p.ranges.iter()))
            .chain(
                input_schema
                    .iter()
                    .chain(output_schema.iter())
                    .flat_map(|g| g.evidence.iter()),
            )
            .map(|g| g.requirement_ref.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for requirement in &specified_by {
            planned.push(PlannedEdge {
                kind: RelationKind::SpecifiedBy,
                from: requirement.clone(),
                to: operation_ref.clone(),
                ranges: Vec::new(),
                requirement_evidence: true,
            });
        }

        // ------------------------------------------------ disposition
        let mut disposition = OperationDisposition::Ineligible;
        if issues.is_empty() {
            let expected: BTreeSet<RelationKey> = planned
                .iter()
                .map(|p| relation_key(&p.kind, &p.from, &p.to))
                .collect::<Result<_, _>>()?;
            match graph.node(&operation_ref) {
                Some(existing) => {
                    let same = existing.payload == payload
                        && existing_relations(graph, &operation_ref)? == expected;
                    disposition = match existing.status {
                        ElementStatus::Proposed if same => OperationDisposition::IdempotentReplay,
                        ElementStatus::Accepted if same => OperationDisposition::ExistingEquivalent,
                        _ => {
                            issues.push(OperationIssue::ExistingOperationConflict {
                                operation_ref: operation_ref.clone(),
                            });
                            OperationDisposition::ExistingConflict
                        }
                    };
                }
                None => {
                    let mut nodes = vec![builder.node(
                        operation_ref.clone(),
                        payload,
                        &evidence,
                        OPERATION_ORIGIN_EXTENSION,
                        origin_json(OperationOrigin {
                            evidence: evidence.clone(),
                        })?,
                    )?];
                    let mut children = Vec::new();
                    for (o, ranges) in &outcomes {
                        if reused_outcomes.contains(&o.outcome_ref) {
                            continue;
                        }
                        children.push(builder.node(
                            o.outcome_ref.clone(),
                            NodePayload::Outcome(Outcome {
                                name: o.name.clone(),
                                outcome_kind: o.outcome_kind,
                            }),
                            ranges,
                            OUTCOME_ORIGIN_EXTENSION,
                            origin_json(OutcomeOrigin {
                                operation_ref: operation_ref.clone(),
                                evidence: ranges.clone(),
                            })?,
                        )?);
                    }
                    for (e, ranges) in &new_events {
                        if e.reused {
                            continue;
                        }
                        children.push(builder.node(
                            e.event_ref.clone(),
                            NodePayload::Event(plumb_psg::Event {
                                name: e.name.clone(),
                                payload_schema_ref: e.payload_schema_ref.clone(),
                                semantic_type: None,
                            }),
                            ranges,
                            EVENT_ORIGIN_EXTENSION,
                            origin_json(EventOrigin {
                                evidence: ranges.clone(),
                            })?,
                        )?);
                    }
                    children.sort_by(|a, b| a.id.cmp(&b.id));
                    nodes.extend(children);
                    let edges = planned
                        .iter()
                        .map(|p| builder.edge(p))
                        .collect::<Result<Vec<_>, _>>()?;
                    let proposal = builder.propose(nodes, edges)?;
                    // In-memory dry validation by the canonical engine; the graph is discarded.
                    match apply_patch(graph, &proposal.patch_set) {
                        Ok(_) => {
                            disposition = OperationDisposition::Proposed {
                                proposal_ref: proposal.id.clone(),
                            };
                            proposals.push(proposal);
                        }
                        Err(e) => issues.push(OperationIssue::InvalidProposal {
                            reason: format!("proposal does not apply: {e}"),
                        }),
                    }
                }
            }
        } else if issues.iter().any(OperationIssue::is_existing_conflict) {
            disposition = OperationDisposition::ExistingConflict;
        }
        issues.sort();
        let mut produced_events: Vec<Id> = produced_refs
            .iter()
            .map(|g| g.target.clone())
            .chain(new_events.iter().map(|(e, _)| e.event_ref.clone()))
            .collect();
        produced_events.sort();
        candidates.push(OperationCandidateAnalysis {
            operation_ref,
            name: c.name,
            operation_kind: c.operation_kind,
            performers: targets(&performers),
            reads: targets(&reads),
            writes: targets(&writes),
            governed_by: targets(&governed_by),
            uses_calculation: targets(&uses_calculation),
            input_schema_ref: input_schema.map(|g| g.target),
            output_schema_ref: output_schema.map(|g| g.target),
            outcomes: outcomes.into_iter().map(|(o, _)| o).collect(),
            new_events: new_events.into_iter().map(|(e, _)| e).collect(),
            produced_events,
            consumed_events: targets(&consumed),
            calculation_reads: calculation_read_analyses,
            specified_by,
            issues,
            disposition,
        });
    }
    candidates.sort_by(|a, b| a.operation_ref.cmp(&b.operation_ref));
    proposals.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(OperationAnalysisResult {
        candidates,
        proposals,
        issues: Vec::new(),
    })
}
