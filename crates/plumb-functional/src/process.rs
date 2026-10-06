//! AI-assisted Process proposals with safe control flow (plan S2.9; compiler architecture §8).
//!
//! A supplied, already-acquired `process_analysis` inference proposes Processes over an explicit
//! closed [`ProcessScope`] of Accepted Operations, performers, Events, Outcomes and Transitions.
//! Every semantic choice is a grounded scope reference; node identities come from local node
//! keys, and sequence comes only from grounded `next` candidates, never from requirement or
//! document position. Each candidate is applied in memory and qualified by the pure
//! `plumb_validation::process_analysis` kernel in ActiveOverlay mode; only fully qualified
//! candidates become one HumanConfirm proposal each. Nodes never carry a condition, exclusive
//! gateways, error events and subprocesses are never proposed, and nothing here emits findings,
//! mutates a graph, calls a provider, reads a clock or persists anything.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AuditMeta, DerivationRef, Edge, ElementStatus, EvidenceRef, ExtensionKey, Graph, Node,
    NodePayload, NodeType, OperationKind, OutcomeKind, Process, ProcessNode, ProcessNodeKind,
    RelationKind, RelationProperties,
};
use plumb_validation::process_analysis::{
    analyze_process, ParallelIssueKind, ProcessAnalysis, ProcessAnalysisMode, ShapeViolation,
    TaskResolutionIssue, UnsupportedSemantics,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

/// Version of [`ProcessContext`].
pub const PROCESS_CONTEXT_VERSION: u32 = 1;

/// Version of the process inference output.
pub const PROCESS_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of process analysis.
pub const PROCESS_TASK_KIND: &str = "process_analysis";

/// Extension key of the provenance [`ProcessOrigin`].
pub const PROCESS_ORIGIN_EXTENSION: &str = "plumb_functional:process_origin";

/// Extension key of the provenance [`ProcessNodeOrigin`].
pub const PROCESS_NODE_ORIGIN_EXTENSION: &str = "plumb_functional:process_node_origin";

const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s2-process.md");
const SCHEMA_SOURCE: &str = include_str!("../../../schemas/inference/s2-process.schema.json");
const PROCESS_STAGE: StageId = StageId::S2;
const NODE_KEY_MAX: usize = 64;

// ============================================================================ errors and issues

/// Why process analysis could not run. Malformed supplied inference, invalid scopes and invalid
/// requests are errors; semantic problems of a candidate are [`ProcessIssue`]s.
#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("invalid process input: {reason}")]
    InvalidInput { reason: String },
    #[error("invalid process scope {field}: {reason}")]
    Scope {
        field: String,
        node_ref: Option<Id>,
        reason: String,
    },
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    #[error("invalid process inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error("process schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    #[error("process output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("invalid grounding {requirement_ref} {start}..{end}: {reason}")]
    InvalidGrounding {
        requirement_ref: Id,
        start: u64,
        end: u64,
        reason: String,
    },
    #[error("invalid node key {node_key:?}")]
    InvalidNodeKey { node_key: String },
    #[error("{relation} target {target_ref} does not exist")]
    UnknownSemanticRef { relation: String, target_ref: Id },
    #[error("{relation} target {target_ref} is not in the process scope")]
    OutOfScopeRef { relation: String, target_ref: Id },
    #[error("{relation} target {target_ref} is not Accepted")]
    NonAcceptedSemanticRef { relation: String, target_ref: Id },
    #[error("{relation} target {target_ref} has the wrong node type {node_type}")]
    WrongSemanticRefType {
        relation: String,
        target_ref: Id,
        node_type: String,
    },
    #[error("duplicate {relation} target {target_ref}")]
    DuplicateRelationTarget { relation: String, target_ref: Id },
    #[error("duplicate process candidate {process_ref}")]
    DuplicateProcess { process_ref: Id },
    #[error("duplicate node key {node_key}")]
    DuplicateNodeKey { node_key: String },
    #[error("next references unknown node key {node_key}")]
    UnknownNextKey { node_key: String },
    #[error("self next on {node_key}")]
    SelfNext { node_key: String },
    #[error("duplicate next {from_key} -> {to_key}")]
    DuplicateNext { from_key: String, to_key: String },
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_input(reason: impl Into<String>) -> ProcessError {
    ProcessError::InvalidInput {
        reason: reason.into(),
    }
}

fn invalid_proposal(e: impl ToString) -> ProcessError {
    ProcessError::InvalidProposal {
        reason: e.to_string(),
    }
}

/// A deterministic candidate problem. These are analysis categories, not rule IDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProcessIssue {
    InferenceUnavailable,
    InvalidName {
        reason: String,
    },
    InvalidNodeShape {
        node_ref: Id,
        violation: ShapeViolation,
    },
    ProcessBoundaryViolation {
        edge_ref: Id,
    },
    MissingStart,
    MissingEnd,
    InvalidTerminal {
        node_ref: Id,
    },
    UnreachableNode {
        node_ref: Id,
    },
    UnresolvedServiceTask {
        node_ref: Id,
    },
    UnresolvedHumanResponsibility {
        node_ref: Id,
    },
    UnresolvedHumanOutcome {
        node_ref: Id,
    },
    AmbiguousStartTrigger {
        node_ref: Id,
    },
    ExclusiveGatewayUnrepresentable {
        node_ref: Id,
    },
    ErrorEventUnrepresentable {
        node_ref: Id,
    },
    SubprocessUnrepresentable {
        node_ref: Id,
    },
    ConditionSemanticsUnqualified {
        node_ref: Id,
    },
    Parallel {
        kind: ParallelIssueKind,
        split_ref: Option<Id>,
        affected_refs: Vec<Id>,
    },
    ExistingProcessConflict {
        process_ref: Id,
    },
    ExistingProcessNameConflict {
        process_ref: Id,
        existing_ref: Id,
    },
    ExistingProcessNodeConflict {
        node_ref: Id,
    },
    InvalidProposal {
        reason: String,
    },
}

/// The typed issues of a structural analysis.
pub fn analysis_issues(analysis: &ProcessAnalysis) -> Vec<ProcessIssue> {
    let mut out = Vec::new();
    if analysis.missing_start() {
        out.push(ProcessIssue::MissingStart);
    }
    if analysis.missing_end() {
        out.push(ProcessIssue::MissingEnd);
    }
    for v in &analysis.node_shape_violations {
        out.push(match v.violation {
            ShapeViolation::AmbiguousStartTrigger => ProcessIssue::AmbiguousStartTrigger {
                node_ref: v.node_ref.clone(),
            },
            _ => ProcessIssue::InvalidNodeShape {
                node_ref: v.node_ref.clone(),
                violation: v.violation.clone(),
            },
        });
    }
    out.extend(analysis.boundary_violations.iter().map(|b| {
        ProcessIssue::ProcessBoundaryViolation {
            edge_ref: b.edge_ref.clone(),
        }
    }));
    out.extend(
        analysis
            .invalid_terminal_refs
            .iter()
            .map(|n| ProcessIssue::InvalidTerminal {
                node_ref: n.clone(),
            }),
    );
    out.extend(
        analysis
            .unreachable_refs
            .iter()
            .map(|n| ProcessIssue::UnreachableNode {
                node_ref: n.clone(),
            }),
    );
    for t in &analysis.unresolved_task_refs {
        let node_ref = t.node_ref.clone();
        out.push(match t.issue {
            TaskResolutionIssue::UnresolvedServiceTask => {
                ProcessIssue::UnresolvedServiceTask { node_ref }
            }
            TaskResolutionIssue::UnresolvedHumanResponsibility => {
                ProcessIssue::UnresolvedHumanResponsibility { node_ref }
            }
            TaskResolutionIssue::UnresolvedHumanOutcome => {
                ProcessIssue::UnresolvedHumanOutcome { node_ref }
            }
        });
    }
    for u in &analysis.unsupported_nodes {
        let node_ref = u.node_ref.clone();
        out.push(match u.semantics {
            UnsupportedSemantics::ExclusiveGateway => {
                ProcessIssue::ExclusiveGatewayUnrepresentable { node_ref }
            }
            UnsupportedSemantics::ErrorEvent => {
                ProcessIssue::ErrorEventUnrepresentable { node_ref }
            }
            UnsupportedSemantics::Subprocess => {
                ProcessIssue::SubprocessUnrepresentable { node_ref }
            }
            UnsupportedSemantics::ConditionExpression => {
                ProcessIssue::ConditionSemanticsUnqualified { node_ref }
            }
        });
    }
    out.extend(
        analysis
            .parallel_analysis
            .issues()
            .iter()
            .map(|p| ProcessIssue::Parallel {
                kind: p.kind,
                split_ref: p.split_ref.clone(),
                affected_refs: p.affected_refs.clone(),
            }),
    );
    out.sort();
    out.dedup();
    out
}

// ============================================================================ scope

/// The explicit closed scope of Accepted nodes process inference may reference. Every list is
/// canonically strictly sorted and unique.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessScope {
    pub operation_refs: Vec<Id>,
    pub performer_refs: Vec<Id>,
    pub event_refs: Vec<Id>,
    pub outcome_refs: Vec<Id>,
    /// Support context only; never converted into `next`.
    pub transition_refs: Vec<Id>,
}

fn is_active(status: ElementStatus) -> bool {
    matches!(status, ElementStatus::Proposed | ElementStatus::Accepted)
}

fn scope_error(field: &str, node_ref: Option<&Id>, reason: impl Into<String>) -> ProcessError {
    ProcessError::Scope {
        field: field.to_owned(),
        node_ref: node_ref.cloned(),
        reason: reason.into(),
    }
}

fn validated_list(
    graph: &Graph,
    field: &str,
    refs: &[Id],
    allowed: &[NodeType],
) -> Result<Vec<Id>, ProcessError> {
    let mut sorted = refs.to_vec();
    sorted.sort();
    if let Some(pair) = sorted.windows(2).find(|p| p[0] == p[1]) {
        return Err(scope_error(field, Some(&pair[0]), "duplicate reference"));
    }
    for id in &sorted {
        let node = graph
            .node(id)
            .ok_or_else(|| scope_error(field, Some(id), "node does not exist"))?;
        if node.status != ElementStatus::Accepted {
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

fn validate_scope(graph: &Graph, scope: &ProcessScope) -> Result<ProcessScope, ProcessError> {
    use NodeType as T;
    Ok(ProcessScope {
        operation_refs: validated_list(
            graph,
            "operation_refs",
            &scope.operation_refs,
            &[T::Operation],
        )?,
        performer_refs: validated_list(
            graph,
            "performer_refs",
            &scope.performer_refs,
            &[T::Actor, T::BusinessRole],
        )?,
        event_refs: validated_list(graph, "event_refs", &scope.event_refs, &[T::Event])?,
        outcome_refs: validated_list(graph, "outcome_refs", &scope.outcome_refs, &[T::Outcome])?,
        transition_refs: validated_list(
            graph,
            "transition_refs",
            &scope.transition_refs,
            &[T::Transition],
        )?,
    })
}

// ============================================================================ request

/// One target Requirement (no document position).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessRequirementContext {
    pub requirement_ref: Id,
    pub status: ElementStatus,
    pub statement: String,
}

/// A deterministic summary of one scoped Operation and its Accepted relations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessOperationContext {
    pub operation_ref: Id,
    pub name: String,
    pub operation_kind: OperationKind,
    pub performer_refs: Vec<Id>,
    pub produced_refs: Vec<Id>,
    pub consumed_event_refs: Vec<Id>,
}

/// One scoped performer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessPerformerContext {
    pub performer_ref: Id,
    pub node_type: String,
    pub name: String,
}

/// One scoped Event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessEventContext {
    pub event_ref: Id,
    pub name: String,
}

/// One scoped Outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessOutcomeContext {
    pub outcome_ref: Id,
    pub name: String,
    pub outcome_kind: OutcomeKind,
}

/// A deterministic summary of one scoped Transition; support context only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessTransitionContext {
    pub transition_ref: Id,
    pub stateful_ref: Id,
    pub from_state: Id,
    pub to_state: Id,
    pub transitions_via_refs: Vec<Id>,
}

/// The exact process inference context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessContext {
    pub version: u32,
    pub project_id: Id,
    pub requirements: Vec<ProcessRequirementContext>,
    pub operations: Vec<ProcessOperationContext>,
    pub performers: Vec<ProcessPerformerContext>,
    pub events: Vec<ProcessEventContext>,
    pub outcomes: Vec<ProcessOutcomeContext>,
    pub transitions: Vec<ProcessTransitionContext>,
}

impl ProcessContext {
    /// Generic SHA-256 of the RFC 8785 canonical JSON of the context.
    pub fn content_hash(&self) -> Result<Hash, ProcessError> {
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn requirement(&self, id: &Id) -> Option<&ProcessRequirementContext> {
        self.requirements
            .binary_search_by(|r| r.requirement_ref.cmp(id))
            .ok()
            .map(|i| &self.requirements[i])
    }

    fn input_refs(&self) -> Vec<Id> {
        let refs: BTreeSet<&Id> = self
            .requirements
            .iter()
            .map(|r| &r.requirement_ref)
            .chain(self.operations.iter().map(|o| &o.operation_ref))
            .chain(self.performers.iter().map(|p| &p.performer_ref))
            .chain(self.events.iter().map(|e| &e.event_ref))
            .chain(self.outcomes.iter().map(|o| &o.outcome_ref))
            .chain(self.transitions.iter().map(|t| &t.transition_ref))
            .collect();
        refs.into_iter().cloned().collect()
    }
}

/// A process `InferenceRequest` with its exact context and validated scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessRequest {
    pub request: InferenceRequest,
    pub context: ProcessContext,
    pub scope: ProcessScope,
}

fn accepted_targets(graph: &Graph, from: &Id, kind: RelationKind) -> Vec<Id> {
    let set: BTreeSet<Id> = graph
        .outgoing_edge_ids(from)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == kind && e.status == ElementStatus::Accepted)
        .filter(|e| {
            graph
                .node(&e.to)
                .is_some_and(|n| n.status == ElementStatus::Accepted)
        })
        .map(|e| e.to.clone())
        .collect();
    set.into_iter().collect()
}

fn context_of(
    graph: &Graph,
    target_requirement_refs: &[Id],
    scope: &ProcessScope,
) -> Result<(ProcessContext, ProcessScope), ProcessError> {
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
        requirements.push(ProcessRequirementContext {
            requirement_ref: id.clone(),
            status: node.status,
            statement: r.statement.clone(),
        });
    }
    let payload = |id: &Id| graph.node(id).map(|n| &n.payload);
    let mut operations = Vec::new();
    for id in &scope.operation_refs {
        let Some(NodePayload::Operation(o)) = payload(id) else {
            return Err(invalid_input(format!("{id} is not an Operation")));
        };
        operations.push(ProcessOperationContext {
            operation_ref: id.clone(),
            name: o.name.clone(),
            operation_kind: o.operation_kind,
            performer_refs: accepted_targets(graph, id, RelationKind::PerformedBy),
            produced_refs: accepted_targets(graph, id, RelationKind::Produces),
            consumed_event_refs: accepted_targets(graph, id, RelationKind::Consumes),
        });
    }
    let mut performers = Vec::new();
    for id in &scope.performer_refs {
        let (node_type, name) = match payload(id) {
            Some(NodePayload::Actor(a)) => ("Actor", a.name.clone()),
            Some(NodePayload::BusinessRole(r)) => ("BusinessRole", r.name.clone()),
            _ => return Err(invalid_input(format!("{id} is not a performer"))),
        };
        performers.push(ProcessPerformerContext {
            performer_ref: id.clone(),
            node_type: node_type.to_owned(),
            name,
        });
    }
    let mut events = Vec::new();
    for id in &scope.event_refs {
        let Some(NodePayload::Event(e)) = payload(id) else {
            return Err(invalid_input(format!("{id} is not an Event")));
        };
        events.push(ProcessEventContext {
            event_ref: id.clone(),
            name: e.name.clone(),
        });
    }
    let mut outcomes = Vec::new();
    for id in &scope.outcome_refs {
        let Some(NodePayload::Outcome(o)) = payload(id) else {
            return Err(invalid_input(format!("{id} is not an Outcome")));
        };
        outcomes.push(ProcessOutcomeContext {
            outcome_ref: id.clone(),
            name: o.name.clone(),
            outcome_kind: o.outcome_kind,
        });
    }
    let mut transitions = Vec::new();
    for id in &scope.transition_refs {
        let Some(NodePayload::Transition(t)) = payload(id) else {
            return Err(invalid_input(format!("{id} is not a Transition")));
        };
        transitions.push(ProcessTransitionContext {
            transition_ref: id.clone(),
            stateful_ref: t.stateful_ref.clone(),
            from_state: t.from_state.clone(),
            to_state: t.to_state.clone(),
            transitions_via_refs: accepted_targets(graph, id, RelationKind::TransitionsVia),
        });
    }
    let context = ProcessContext {
        version: PROCESS_CONTEXT_VERSION,
        project_id: graph.project_id().clone(),
        requirements,
        operations,
        performers,
        events,
        outcomes,
        transitions,
    };
    Ok((context, scope))
}

/// The EvidenceRefs of the target Requirements and the scoped semantic context.
fn context_evidence(graph: &Graph, context: &ProcessContext) -> Vec<Id> {
    let refs: BTreeSet<Id> = context
        .input_refs()
        .iter()
        .filter_map(|id| graph.node(id))
        .flat_map(|n| n.evidence.iter().map(|e| e.as_id().clone()))
        .collect();
    refs.into_iter().collect()
}

/// Builds the S2 process request over explicit target Requirements and an explicit scope.
pub fn build_process_request(
    graph: &Graph,
    target_requirement_refs: &[Id],
    scope: &ProcessScope,
    provider_policy: ProviderPolicy,
) -> Result<ProcessRequest, ProcessError> {
    let (context, scope) = context_of(graph, target_requirement_refs, scope)?;
    let request = InferenceRequest::new(
        PROCESS_STAGE,
        PROCESS_TASK_KIND.to_owned(),
        context.input_refs(),
        context_evidence(graph, &context),
        context.content_hash()?,
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    Ok(ProcessRequest {
        request,
        context,
        scope,
    })
}

impl ProcessRequest {
    /// The request is bound to its context, the committed prompt and schema.
    pub fn validate(&self) -> Result<(), ProcessError> {
        let r = &self.request;
        r.validate()
            .map_err(|e| invalid_input(format!("invalid inference request: {e}")))?;
        let ok = r.stage == PROCESS_STAGE
            && r.task_kind == PROCESS_TASK_KIND
            && r.input_refs == self.context.input_refs()
            && r.context_hash == self.context.content_hash()?
            && r.prompt_template_hash == Hash::content_sha256(PROMPT_TEMPLATE)
            && r.schema_hash == Hash::content_sha256(SCHEMA_SOURCE.as_bytes())
            && self.context.version == PROCESS_CONTEXT_VERSION;
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
pub struct ProcessGroundedRange {
    pub requirement_ref: Id,
    pub start: u64,
    pub end: u64,
}

/// Provenance of a proposed Process (`plumb_functional:process_origin`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessOrigin {
    pub evidence: Vec<ProcessGroundedRange>,
}

/// Provenance and replay identity of a proposed ProcessNode
/// (`plumb_functional:process_node_origin`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessNodeOrigin {
    pub node_key: String,
    pub evidence: Vec<ProcessGroundedRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessOutput {
    version: u32,
    processes: Vec<OutputProcess>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputGroundedRef {
    target_ref: Id,
    evidence: Vec<ProcessGroundedRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputNode {
    node_key: String,
    node_kind: ProcessNodeKind,
    operation: Option<OutputGroundedRef>,
    performers: Vec<OutputGroundedRef>,
    produces: Vec<OutputGroundedRef>,
    message_ref: Option<OutputGroundedRef>,
    timer_expr: Option<String>,
    evidence: Vec<ProcessGroundedRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputNext {
    from_key: String,
    to_key: String,
    evidence: Vec<ProcessGroundedRange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputProcess {
    name: String,
    evidence: Vec<ProcessGroundedRange>,
    nodes: Vec<OutputNode>,
    next: Vec<OutputNext>,
}

fn compile_schema() -> Result<JSONSchema, ProcessError> {
    let schema: JsonValue =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| ProcessError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| ProcessError::SchemaCompilation {
            reason: e.to_string(),
        })
}

fn grounded_evidence(
    context: &ProcessContext,
    ranges: &[ProcessGroundedRange],
) -> Result<Vec<ProcessGroundedRange>, ProcessError> {
    let invalid = |g: &ProcessGroundedRange, reason: &str| ProcessError::InvalidGrounding {
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

/// `^[A-Za-z][A-Za-z0-9_-]{0,63}$`, checked exactly.
fn valid_node_key(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= NODE_KEY_MAX
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

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

/// `process:<16 hex of SHA-256(RFC 8785 {project_id, name, node_type: process})>`.
fn process_id(project_id: &Id, name: &str) -> Result<Id, CoreError> {
    short_id(
        "process",
        &json!({"project_id": project_id, "name": name, "node_type": "process"}),
    )
}

/// `process_node:<16 hex of SHA-256(RFC 8785 {project_id, process_ref, node_key, node_type})>`.
fn process_node_id(project_id: &Id, process_ref: &Id, node_key: &str) -> Result<Id, CoreError> {
    short_id(
        "process_node",
        &json!({
            "project_id": project_id,
            "process_ref": process_ref,
            "node_key": node_key,
            "node_type": "process_node"
        }),
    )
}

/// `rel:<16 hex of SHA-256(RFC 8785 {kind, from, to})>`.
fn edge_id(kind: &RelationKind, from: &Id, to: &Id) -> Result<Id, CoreError> {
    short_id("rel", &json!({"kind": kind, "from": from, "to": to}))
}

/// Resolves a grounded semantic reference against its scope lists.
fn resolve(
    graph: &Graph,
    relation: &str,
    lists: &[&[Id]],
    allowed: &[NodeType],
    target: &Id,
) -> Result<(), ProcessError> {
    if lists.iter().any(|l| l.binary_search(target).is_ok()) {
        return Ok(());
    }
    let relation = relation.to_owned();
    let target_ref = target.clone();
    match graph.node(target) {
        None => Err(ProcessError::UnknownSemanticRef {
            relation,
            target_ref,
        }),
        Some(n) if !allowed.contains(&n.payload.node_type()) => {
            Err(ProcessError::WrongSemanticRefType {
                relation,
                target_ref,
                node_type: n.payload.node_type().as_str().to_owned(),
            })
        }
        Some(n) if n.status != ElementStatus::Accepted => {
            Err(ProcessError::NonAcceptedSemanticRef {
                relation,
                target_ref,
            })
        }
        Some(_) => Err(ProcessError::OutOfScopeRef {
            relation,
            target_ref,
        }),
    }
}

/// A validated grounded relation target.
#[derive(Debug, Clone)]
struct Grounded {
    target: Id,
    evidence: Vec<ProcessGroundedRange>,
}

fn grounded_ref(
    graph: &Graph,
    context: &ProcessContext,
    relation: &str,
    r: &OutputGroundedRef,
    lists: &[&[Id]],
    allowed: &[NodeType],
) -> Result<Grounded, ProcessError> {
    resolve(graph, relation, lists, allowed, &r.target_ref)?;
    Ok(Grounded {
        target: r.target_ref.clone(),
        evidence: grounded_evidence(context, &r.evidence)?,
    })
}

fn grounded_family(
    graph: &Graph,
    context: &ProcessContext,
    relation: &str,
    refs: &[OutputGroundedRef],
    lists: &[&[Id]],
    allowed: &[NodeType],
) -> Result<Vec<Grounded>, ProcessError> {
    let mut out = refs
        .iter()
        .map(|r| grounded_ref(graph, context, relation, r, lists, allowed))
        .collect::<Result<Vec<_>, _>>()?;
    out.sort_by(|a, b| a.target.cmp(&b.target));
    if let Some(pair) = out.windows(2).find(|p| p[0].target == p[1].target) {
        return Err(ProcessError::DuplicateRelationTarget {
            relation: relation.to_owned(),
            target_ref: pair[0].target.clone(),
        });
    }
    Ok(out)
}

// ============================================================================ analysis

/// An already-acquired process artifact and the caller's provenance ref for it.
#[derive(Debug, Clone)]
pub struct ProcessInference<'a> {
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

/// Who owns the Proposed elements and when, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// What happened to one inferred Process candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessDisposition {
    Proposed { proposal_ref: Id },
    Ineligible,
    IdempotentReplay,
    ExistingEquivalent,
    ExistingConflict,
}

/// One inferred Process candidate with its identities, structural analysis and disposition.
#[derive(Debug, Clone)]
pub struct ProcessCandidateAnalysis {
    pub process_ref: Id,
    pub name: String,
    /// Node keys to their deterministic ProcessNode IDs.
    pub node_refs: BTreeMap<String, Id>,
    /// ActiveOverlay analysis of the in-memory dry-run graph, when it could be built.
    pub analysis: Option<ProcessAnalysis>,
    pub issues: Vec<ProcessIssue>,
    pub disposition: ProcessDisposition,
}

/// Everything one compilation produced; nothing has been applied or persisted.
#[derive(Debug, Clone)]
pub struct ProcessCompilationResult {
    pub candidates: Vec<ProcessCandidateAnalysis>,
    pub proposals: Vec<Proposal>,
    pub issues: Vec<ProcessIssue>,
}

type RelationKey = (String, Id, Id);

/// The canonical semantic subgraph of one Process.
#[derive(Debug, PartialEq)]
struct Subgraph {
    process: NodePayload,
    nodes: BTreeMap<Id, (NodePayload, Option<String>)>,
    relations: BTreeSet<RelationKey>,
}

const MEMBER_RELATIONS: [RelationKind; 4] = [
    RelationKind::Next,
    RelationKind::PerformedBy,
    RelationKind::Produces,
    RelationKind::Consumes,
];

fn relation_key(kind: &RelationKind, from: &Id, to: &Id) -> RelationKey {
    (kind.as_str().to_owned(), from.clone(), to.clone())
}

fn node_key_of(node: &Node) -> Option<String> {
    node.extensions
        .iter()
        .find(|(k, _)| k.as_str() == PROCESS_NODE_ORIGIN_EXTENSION)
        .and_then(|(_, v)| serde_json::from_value::<ProcessNodeOrigin>(v.clone()).ok())
        .map(|o| o.node_key)
}

fn existing_subgraph(graph: &Graph, process: &Node) -> Subgraph {
    let nodes: BTreeMap<Id, (NodePayload, Option<String>)> = graph
        .node_ids_by_type(NodeType::ProcessNode)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| is_active(n.status))
        .filter(
            |n| matches!(&n.payload, NodePayload::ProcessNode(p) if p.process_ref == process.id),
        )
        .map(|n| (n.id.clone(), (n.payload.clone(), node_key_of(n))))
        .collect();
    let mut relations = BTreeSet::new();
    for member in nodes.keys() {
        for e in graph
            .outgoing_edge_ids(member)
            .iter()
            .filter_map(|e| graph.edge(e))
        {
            if MEMBER_RELATIONS.contains(&e.kind) && is_active(e.status) {
                relations.insert(relation_key(&e.kind, &e.from, &e.to));
            }
        }
    }
    Subgraph {
        process: process.payload.clone(),
        nodes,
        relations,
    }
}

struct PlannedEdge {
    kind: RelationKind,
    from: Id,
    to: Id,
    evidence: Vec<ProcessGroundedRange>,
}

struct Builder<'a> {
    graph: &'a Graph,
    audit: &'a ProcessAudit,
}

impl Builder<'_> {
    fn evidence(&self, ranges: &[ProcessGroundedRange]) -> Vec<EvidenceRef> {
        let refs: BTreeSet<&EvidenceRef> = ranges
            .iter()
            .filter_map(|g| self.graph.node(&g.requirement_ref))
            .flat_map(|n| n.evidence.iter())
            .collect();
        refs.into_iter().cloned().collect()
    }

    fn meta(&self) -> Result<AuditMeta, ProcessError> {
        AuditMeta::new(
            self.audit.created_by.clone(),
            self.audit.created_at,
            None,
            None,
        )
        .map_err(invalid_proposal)
    }

    fn node(
        &self,
        id: &Id,
        payload: NodePayload,
        ranges: &[ProcessGroundedRange],
        extension: &str,
        origin: JsonValue,
    ) -> Result<Node, ProcessError> {
        let key: ExtensionKey = extension.parse().map_err(invalid_proposal)?;
        Ok(Node {
            id: id.clone(),
            revision: 1,
            status: ElementStatus::Proposed,
            payload,
            evidence: self.evidence(ranges),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::from([(key, origin)]),
            audit: self.meta()?,
        })
    }

    fn edge(&self, e: &PlannedEdge) -> Result<Edge, ProcessError> {
        Ok(Edge {
            id: edge_id(&e.kind, &e.from, &e.to)?,
            revision: 1,
            status: ElementStatus::Proposed,
            kind: e.kind.clone(),
            from: e.from.clone(),
            to: e.to.clone(),
            properties: RelationProperties::None,
            evidence: self.evidence(&e.evidence),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: self.meta()?,
        })
    }
}

fn origin_json(value: impl Serialize) -> Result<JsonValue, ProcessError> {
    serde_json::to_value(value).map_err(invalid_proposal)
}

/// Analyzes a supplied process inference against its request. Absent inference yields only
/// InferenceUnavailable; malformed inference is an error.
pub fn analyze_processes(
    graph: &Graph,
    request: &ProcessRequest,
    inference: Option<ProcessInference<'_>>,
    audit: &ProcessAudit,
) -> Result<ProcessCompilationResult, ProcessError> {
    use NodeType as T;
    request.validate()?;
    let targets: Vec<Id> = request
        .context
        .requirements
        .iter()
        .map(|r| r.requirement_ref.clone())
        .collect();
    let (context, scope) = context_of(graph, &targets, &request.scope)?;
    if context != request.context || scope != request.scope {
        return Err(invalid_input(
            "graph does not match the process request context",
        ));
    }
    if request.request.evidence_refs != context_evidence(graph, &context) {
        return Err(invalid_input(
            "evidence_refs differ from the context evidence",
        ));
    }
    let Some(inference) = inference else {
        return Ok(ProcessCompilationResult {
            candidates: Vec::new(),
            proposals: Vec::new(),
            issues: vec![ProcessIssue::InferenceUnavailable],
        });
    };
    inference
        .artifact
        .validate_for(&request.request)
        .map_err(|e| ProcessError::InvalidInferenceArtifact {
            reason: e.to_string(),
        })?;
    let output = inference.artifact.validated_output.as_value();
    if let Err(errors) = compile_schema()?.validate(output) {
        let mut messages: Vec<String> = errors
            .map(|e| format!("{e} at {}", e.instance_path))
            .collect();
        messages.sort();
        return Err(ProcessError::SchemaInvalid {
            reason: messages.join("; "),
        });
    }
    let decoded: ProcessOutput =
        serde_json::from_value(output.clone()).map_err(|e| ProcessError::SchemaInvalid {
            reason: e.to_string(),
        })?;
    if decoded.version != PROCESS_OUTPUT_VERSION {
        return Err(ProcessError::SchemaInvalid {
            reason: format!("output version {} is not 1", decoded.version),
        });
    }
    let project = graph.project_id();
    let builder = Builder { graph, audit };
    let base = graph.semantic_hash()?;
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    let mut proposals = Vec::new();
    for c in decoded.processes {
        // ------------------------------------------------ artifact contract (hard errors)
        let evidence = grounded_evidence(&context, &c.evidence)?;
        let process_ref = process_id(project, &c.name)?;
        if !seen.insert(process_ref.clone()) {
            return Err(ProcessError::DuplicateProcess { process_ref });
        }
        let mut node_refs: BTreeMap<String, Id> = BTreeMap::new();
        let mut planned_nodes: Vec<(Id, NodePayload, Vec<ProcessGroundedRange>, String)> =
            Vec::new();
        let mut planned_edges: Vec<PlannedEdge> = Vec::new();
        for n in &c.nodes {
            if !valid_node_key(&n.node_key) {
                return Err(ProcessError::InvalidNodeKey {
                    node_key: n.node_key.clone(),
                });
            }
            let node_ref = process_node_id(project, &process_ref, &n.node_key)?;
            if node_refs
                .insert(n.node_key.clone(), node_ref.clone())
                .is_some()
            {
                return Err(ProcessError::DuplicateNodeKey {
                    node_key: n.node_key.clone(),
                });
            }
            let node_evidence = grounded_evidence(&context, &n.evidence)?;
            let operation = n
                .operation
                .as_ref()
                .map(|r| {
                    grounded_ref(
                        graph,
                        &context,
                        "operation",
                        r,
                        &[&scope.operation_refs],
                        &[T::Operation],
                    )
                })
                .transpose()?;
            let message = n
                .message_ref
                .as_ref()
                .map(|r| {
                    grounded_ref(
                        graph,
                        &context,
                        "message_ref",
                        r,
                        &[&scope.event_refs],
                        &[T::Event],
                    )
                })
                .transpose()?;
            let performers = grounded_family(
                graph,
                &context,
                "performed_by",
                &n.performers,
                &[&scope.performer_refs],
                &[T::Actor, T::BusinessRole],
            )?;
            let produces = grounded_family(
                graph,
                &context,
                "produces",
                &n.produces,
                &[&scope.outcome_refs, &scope.event_refs],
                &[T::Outcome, T::Event],
            )?;
            let payload = NodePayload::ProcessNode(ProcessNode {
                process_ref: process_ref.clone(),
                node_kind: n.node_kind,
                operation_ref: operation.as_ref().map(|g| g.target.clone()),
                condition_expr: None,
                message_ref: message.as_ref().map(|g| g.target.clone()),
                timer_expr: n.timer_expr.clone(),
            });
            for (kind, list) in [
                (RelationKind::PerformedBy, &performers),
                (RelationKind::Produces, &produces),
            ] {
                for g in list {
                    planned_edges.push(PlannedEdge {
                        kind: kind.clone(),
                        from: node_ref.clone(),
                        to: g.target.clone(),
                        evidence: g.evidence.clone(),
                    });
                }
            }
            if let Some(g) = &message {
                planned_edges.push(PlannedEdge {
                    kind: RelationKind::Consumes,
                    from: node_ref.clone(),
                    to: g.target.clone(),
                    evidence: g.evidence.clone(),
                });
            }
            planned_nodes.push((node_ref, payload, node_evidence, n.node_key.clone()));
        }
        let mut next_pairs = BTreeSet::new();
        for e in &c.next {
            for key in [&e.from_key, &e.to_key] {
                if !node_refs.contains_key(key) {
                    return Err(ProcessError::UnknownNextKey {
                        node_key: key.clone(),
                    });
                }
            }
            if e.from_key == e.to_key {
                return Err(ProcessError::SelfNext {
                    node_key: e.from_key.clone(),
                });
            }
            if !next_pairs.insert((e.from_key.clone(), e.to_key.clone())) {
                return Err(ProcessError::DuplicateNext {
                    from_key: e.from_key.clone(),
                    to_key: e.to_key.clone(),
                });
            }
            planned_edges.push(PlannedEdge {
                kind: RelationKind::Next,
                from: node_refs[&e.from_key].clone(),
                to: node_refs[&e.to_key].clone(),
                evidence: grounded_evidence(&context, &e.evidence)?,
            });
        }

        // ------------------------------------------------ qualification and reconciliation
        let mut issues = Vec::new();
        if let Some(reason) = name_problem(&c.name) {
            issues.push(ProcessIssue::InvalidName {
                reason: reason.to_owned(),
            });
        }
        let process_payload = NodePayload::Process(Process {
            name: c.name.clone(),
            description: None,
            process_kind: None,
        });
        let candidate_subgraph = Subgraph {
            process: process_payload.clone(),
            nodes: planned_nodes
                .iter()
                .map(|(id, payload, _, key)| (id.clone(), (payload.clone(), Some(key.clone()))))
                .collect(),
            relations: planned_edges
                .iter()
                .map(|e| relation_key(&e.kind, &e.from, &e.to))
                .collect(),
        };
        let mut disposition = ProcessDisposition::Ineligible;
        let mut analysis = None;
        if let Some(existing) = graph.node(&process_ref) {
            let same = existing_subgraph(graph, existing) == candidate_subgraph;
            disposition = match existing.status {
                ElementStatus::Proposed if same && issues.is_empty() => {
                    ProcessDisposition::IdempotentReplay
                }
                ElementStatus::Accepted if same && issues.is_empty() => {
                    ProcessDisposition::ExistingEquivalent
                }
                _ => {
                    issues.push(ProcessIssue::ExistingProcessConflict {
                        process_ref: process_ref.clone(),
                    });
                    ProcessDisposition::ExistingConflict
                }
            };
        } else {
            for other in graph.node_ids_by_type(NodeType::Process) {
                if let Some(n) = graph.node(other) {
                    if let NodePayload::Process(p) = &n.payload {
                        if is_active(n.status) && p.name == c.name && other != &process_ref {
                            issues.push(ProcessIssue::ExistingProcessNameConflict {
                                process_ref: process_ref.clone(),
                                existing_ref: other.clone(),
                            });
                        }
                    }
                }
            }
            for (id, _, _, _) in &planned_nodes {
                if graph.node(id).is_some() {
                    issues.push(ProcessIssue::ExistingProcessNodeConflict {
                        node_ref: id.clone(),
                    });
                }
            }
            if issues.iter().any(|i| {
                matches!(
                    i,
                    ProcessIssue::ExistingProcessNameConflict { .. }
                        | ProcessIssue::ExistingProcessNodeConflict { .. }
                )
            }) {
                disposition = ProcessDisposition::ExistingConflict;
            } else {
                let mut nodes = vec![builder.node(
                    &process_ref,
                    process_payload,
                    &evidence,
                    PROCESS_ORIGIN_EXTENSION,
                    origin_json(ProcessOrigin {
                        evidence: evidence.clone(),
                    })?,
                )?];
                let mut members = Vec::new();
                for (id, payload, ranges, key) in &planned_nodes {
                    members.push(builder.node(
                        id,
                        payload.clone(),
                        ranges,
                        PROCESS_NODE_ORIGIN_EXTENSION,
                        origin_json(ProcessNodeOrigin {
                            node_key: key.clone(),
                            evidence: ranges.clone(),
                        })?,
                    )?);
                }
                members.sort_by(|a, b| a.id.cmp(&b.id));
                nodes.extend(members);
                let mut edges = planned_edges
                    .iter()
                    .map(|e| builder.edge(e))
                    .collect::<Result<Vec<_>, _>>()?;
                edges.sort_by(|a, b| a.id.cmp(&b.id));
                let mut proposal_evidence: BTreeSet<EvidenceRef> = BTreeSet::new();
                for n in &nodes {
                    proposal_evidence.extend(n.evidence.iter().cloned());
                }
                for e in &edges {
                    proposal_evidence.extend(e.evidence.iter().cloned());
                }
                let patches = nodes
                    .into_iter()
                    .map(|node| SemanticPatch::AddNode { node })
                    .chain(
                        edges
                            .into_iter()
                            .map(|edge| SemanticPatch::AddEdge { edge }),
                    )
                    .collect();
                let proposal = Proposal::new(
                    PROCESS_STAGE,
                    PatchSet {
                        base_semantic_hash: base.clone(),
                        patch: SemanticPatch::Compound { patches },
                    },
                    proposal_evidence.into_iter().collect(),
                    vec![inference.derivation_ref.clone()],
                    ProposalMateriality::Semantic,
                    AcceptancePolicy::HumanConfirm,
                    None,
                )
                .map_err(invalid_proposal)?;
                // Temporary in-memory dry-run, then ActiveOverlay qualification; the graph is
                // discarded.
                match apply_patch(graph, &proposal.patch_set) {
                    Err(e) => issues.push(ProcessIssue::InvalidProposal {
                        reason: format!("proposal does not apply: {e}"),
                    }),
                    Ok(applied) => {
                        let a = analyze_process(
                            &applied.graph,
                            &process_ref,
                            ProcessAnalysisMode::ActiveOverlay,
                        )
                        .map_err(invalid_proposal)?;
                        issues.extend(analysis_issues(&a));
                        analysis = Some(a);
                        if issues.is_empty() {
                            disposition = ProcessDisposition::Proposed {
                                proposal_ref: proposal.id.clone(),
                            };
                            proposals.push(proposal);
                        }
                    }
                }
            }
        }
        issues.sort();
        issues.dedup();
        candidates.push(ProcessCandidateAnalysis {
            process_ref,
            name: c.name,
            node_refs,
            analysis,
            issues,
            disposition,
        });
    }
    candidates.sort_by(|a, b| a.process_ref.cmp(&b.process_ref));
    proposals.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ProcessCompilationResult {
        candidates,
        proposals,
        issues: Vec::new(),
    })
}
