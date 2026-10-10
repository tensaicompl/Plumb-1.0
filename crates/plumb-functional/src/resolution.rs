//! ResolutionDecision and answer-to-patch application (plan S3.3, Hotfix 047; compiler
//! architecture §9; metamodel §6.3).
//!
//! One human answer to an S3.1 Question becomes one human-decision proposal: an optional
//! explicit semantic answer effect, an Accepted ResolutionDecision with its governance edges and
//! the Question/Finding lifecycle updates. The mapper is identified only by (Finding code,
//! QuestionKind) for the six current templates and the subject only by `context_refs[0]`. The
//! effect is recorded in a separate content-addressed artifact whose hash is the decision's
//! `patch_ref`. Nothing here applies the proposal, reads files, observes time or infers.

use std::collections::BTreeMap;

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_expr::{parse, typecheck, Ty};
use plumb_patch::{
    apply_patch, AcceptancePolicy, ElementPrecondition, PatchSet, Proposal, ProposalMateriality,
    SemanticPatch,
};
use plumb_psg::{
    edge_element_hash, is_baseline, node_element_hash, AgentKind, AuditMeta, Edge, ElementStatus,
    Finding, Graph, Node, NodePayload, NodeType, Question, QuestionKind, RelationKind,
    RelationProperties, ResolutionDecision,
};
use plumb_validation::expression_scope::{validate_expression_bindings, ExpressionScopeBinding};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

const RESOLUTION_STAGE: StageId = StageId::S3;

/// The ResolutionPatchArtifact format version.
pub const RESOLUTION_ARTIFACT_VERSION: u32 = 1;

const OPEN: &str = "Open";
const ANSWERED: &str = "Answered";
const CLOSED: &str = "Closed";

// ============================================================================ templates

/// The six resolvable S3.1 template families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionTemplate {
    DomainRelationshipCardinality,
    StateTransitionTrigger,
    OperationPerformer,
    CalculationCalendar,
    PermissionBinding,
    InvariantFormula,
}

impl ResolutionTemplate {
    /// The template of the exact (Finding code, QuestionKind) pair; nothing else is consulted.
    pub fn of(code: &str, kind: QuestionKind) -> Option<ResolutionTemplate> {
        use ResolutionTemplate::*;
        match (code, kind) {
            ("PLUMB.F2.DOMAIN.RELATION_TYPED", QuestionKind::Cardinality) => {
                Some(DomainRelationshipCardinality)
            }
            ("PLUMB.F2.STATE.TRANSITION_COMPLETE", QuestionKind::PickOne) => {
                Some(StateTransitionTrigger)
            }
            ("PLUMB.F2.OPERATION.PERFORMER", QuestionKind::RoleAssignment) => {
                Some(OperationPerformer)
            }
            ("PLUMB.F2.TIME.CALENDAR_DEFINED", QuestionKind::Calendar) => Some(CalculationCalendar),
            ("RBAC.F2.PERMISSION.CONCRETE", QuestionKind::Permission) => Some(PermissionBinding),
            ("PLUMB.F2.INVARIANT.EXPRESSIBLE", QuestionKind::FormulaConfirm) => {
                Some(InvariantFormula)
            }
            _ => None,
        }
    }

    /// The `kind` of the governed answer marker.
    pub fn marker_kind(self) -> &'static str {
        match self {
            ResolutionTemplate::DomainRelationshipCardinality => "domain_relationship_cardinality",
            ResolutionTemplate::StateTransitionTrigger => "state_transition_trigger",
            ResolutionTemplate::OperationPerformer => "operation_performer",
            ResolutionTemplate::CalculationCalendar => "calculation_calendar",
            ResolutionTemplate::PermissionBinding => "permission_binding",
            ResolutionTemplate::InvariantFormula => "invariant_formula",
        }
    }

    /// Whether a clean rationale is mandatory for an initial answer (the domain.rs and state.rs
    /// governed readers require it).
    fn requires_rationale(self) -> bool {
        matches!(
            self,
            ResolutionTemplate::DomainRelationshipCardinality
                | ResolutionTemplate::StateTransitionTrigger
        )
    }
}

// ============================================================================ input and output

/// One human answer.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionInput {
    pub question_ref: Id,
    pub answer: JsonValue,
    pub decided_by: Id,
    pub decided_at: Timestamp,
    pub rationale: Option<String>,
    pub supersedes: Option<Id>,
}

/// Explicit orchestration context: the expression scopes of Invariants that may be answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolutionContext {
    pub invariant_bindings: BTreeMap<Id, Vec<ExpressionScopeBinding>>,
}

/// The content-addressed semantic answer effect; its canonical hash is `patch_ref`. It never
/// contains the decision, governance edges or lifecycle updates, so the hash is not circular.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionPatchArtifact {
    pub version: u32,
    pub question_ref: Id,
    pub base_semantic_hash: Hash,
    pub effect_patch: Option<SemanticPatch>,
}

impl ResolutionPatchArtifact {
    /// RFC 8785 canonical bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CoreError> {
        to_canonical_json(self)
    }

    /// Generic SHA-256 of the canonical bytes.
    pub fn content_hash(&self) -> Result<Hash, CoreError> {
        Ok(Hash::content_sha256(&self.canonical_bytes()?))
    }
}

/// One proposed resolution; nothing has been applied.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionResult {
    pub template: ResolutionTemplate,
    pub decision_ref: Id,
    pub decision: ResolutionDecision,
    pub patch_artifact: ResolutionPatchArtifact,
    /// The canonical artifact bytes to persist; their hash is `decision.patch_ref`.
    pub patch_artifact_bytes: Vec<u8>,
    pub proposal: Proposal,
}

/// Why an answer cannot be resolved.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolutionError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("question {0} does not exist")]
    QuestionMissing(Id),
    #[error("{0} is not a Question")]
    NotAQuestion(Id),
    #[error("question {0} is not Accepted")]
    QuestionNotAccepted(Id),
    #[error("question {question_ref} has status {status:?}, not Open")]
    QuestionNotOpen { question_ref: Id, status: String },
    #[error(
        "question {question_ref} has status {status:?}; only Answered Questions are superseded"
    )]
    QuestionNotAnsweredForSupersession { question_ref: Id, status: String },
    #[error("question {question_ref} is already resolved by {decision_ref}")]
    AlreadyResolved { question_ref: Id, decision_ref: Id },
    #[error("finding {0} does not exist")]
    FindingMissing(Id),
    #[error("{0} is not a Finding")]
    NotAFinding(Id),
    #[error("finding {0} is not Accepted")]
    FindingNotAccepted(Id),
    #[error("finding {finding_ref} has status {status:?}")]
    FindingNotOpen { finding_ref: Id, status: String },
    #[error("finding {0} is waived")]
    FindingWaived(Id),
    #[error("affected element {0} is missing or not Accepted")]
    AffectedRefNotAccepted(Id),
    #[error("question {0} has no context_refs subject")]
    MissingQuestionContext(Id),
    #[error("finding code {code} with question kind {kind:?} has no resolution template")]
    UnsupportedResolutionTemplate { code: String, kind: QuestionKind },
    #[error("question {question_ref} violates its template contract: {reason}")]
    InvalidQuestionContract { question_ref: Id, reason: String },
    #[error("answer does not satisfy the question schema: {reason}")]
    AnswerSchemaInvalid { reason: String },
    #[error("answer reference {0} does not exist")]
    AnswerReferenceMissing(Id),
    #[error("answer reference {0} is not Accepted")]
    AnswerReferenceNotAccepted(Id),
    #[error("answer reference {reference} is not a {expected}")]
    AnswerReferenceWrongType { reference: Id, expected: String },
    #[error("{0} is not an Accepted human Agent")]
    InvalidDecisionActor(Id),
    #[error("rationale is missing or not clean text")]
    InvalidRationale,
    #[error("question {question_ref} no longer describes the graph: {reason}")]
    StaleResolutionQuestion { question_ref: Id, reason: String },
    #[error("no explicit expression scope for invariant {0}")]
    InvariantScopeUnavailable(Id),
    #[error("invariant expression is invalid: {reason}")]
    InvalidInvariantExpression { reason: String },
    #[error("invariant expression has type {actual}, not Bool")]
    InvariantNotBoolean { actual: String },
    #[error("superseded decision {decision_ref} is invalid: {reason}")]
    InvalidSupersededDecision { decision_ref: Id, reason: String },
    #[error("the graph no longer matches superseded decision {decision_ref}: {reason}")]
    SupersessionStateMismatch { decision_ref: Id, reason: String },
    #[error("permission {permission_ref} has {count} baseline {relation} edges")]
    InvalidPermissionStructure {
        permission_ref: Id,
        relation: String,
        count: usize,
    },
    #[error("permission relation {edge_ref} is {status:?} and cannot be repaired")]
    UnsupportedPermissionEdgeLifecycle { edge_ref: Id, status: ElementStatus },
    #[error("decision ID {0} is already used by different material")]
    ResolutionDecisionIdCollision(Id),
    #[error("resolution proposal is invalid: {reason}")]
    InvalidResolutionProposal { reason: String },
}

// ============================================================================ identities

fn short_id(prefix: &str, body: &JsonValue) -> Result<Id, CoreError> {
    let digest = Hash::content_sha256(&to_canonical_json(body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("{prefix}:{}", &hex[..16]).parse()
}

/// `rel:<16 hex of SHA-256(RFC 8785 {kind, from, to})>`.
pub fn relation_id(kind: &RelationKind, from: &Id, to: &Id) -> Result<Id, CoreError> {
    short_id(
        "rel",
        &json!({"kind": kind.as_str(), "from": from.to_string(), "to": to.to_string()}),
    )
}

/// `dec:<16 hex of SHA-256(RFC 8785 {project_id, question_ref, answer, decided_by, decided_at,
/// patch_ref, supersedes})>`; the rationale is not part of decision identity.
pub fn decision_id(
    project_id: &Id,
    question_ref: &Id,
    answer: &JsonValue,
    decided_by: &Id,
    decided_at: &Timestamp,
    patch_ref: &Hash,
    supersedes: Option<&Id>,
) -> Result<Id, CoreError> {
    short_id(
        "dec",
        &json!({
            "project_id": project_id.to_string(),
            "question_ref": question_ref.to_string(),
            "answer": answer,
            "decided_by": decided_by.to_string(),
            "decided_at": serde_json::to_value(decided_at).map_err(|e| CoreError::Canonicalization(e.to_string()))?,
            "patch_ref": patch_ref.as_str(),
            "supersedes": supersedes.map(Id::to_string),
        }),
    )
}

// ============================================================================ graph helpers

fn accepted(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
}

fn clean(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

/// An answer reference that must be an Accepted node of one of `types`.
fn accepted_ref<'g>(
    graph: &'g Graph,
    reference: &Id,
    types: &[NodeType],
) -> Result<&'g Node, ResolutionError> {
    let node = graph
        .node(reference)
        .ok_or_else(|| ResolutionError::AnswerReferenceMissing(reference.clone()))?;
    if !types.contains(&node.payload.node_type()) {
        return Err(ResolutionError::AnswerReferenceWrongType {
            reference: reference.clone(),
            expected: types
                .iter()
                .map(|t| t.as_str())
                .collect::<Vec<_>>()
                .join(" or "),
        });
    }
    if !accepted(node) {
        return Err(ResolutionError::AnswerReferenceNotAccepted(
            reference.clone(),
        ));
    }
    Ok(node)
}

/// The subject of a per-target template: an Accepted node of `node_type`.
fn accepted_subject<'g>(
    graph: &'g Graph,
    question_ref: &Id,
    subject: &Id,
    node_type: NodeType,
) -> Result<&'g Node, ResolutionError> {
    graph
        .node(subject)
        .filter(|n| accepted(n) && n.payload.node_type() == node_type)
        .ok_or_else(|| ResolutionError::InvalidQuestionContract {
            question_ref: question_ref.clone(),
            reason: format!(
                "subject {subject} is not an Accepted {}",
                node_type.as_str()
            ),
        })
}

fn answer_id(answer: &JsonValue, field: &str) -> Result<Id, ResolutionError> {
    let text = answer
        .get(field)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| ResolutionError::AnswerSchemaInvalid {
            reason: format!("{field} is not a string"),
        })?;
    text.parse()
        .map_err(|_| ResolutionError::AnswerSchemaInvalid {
            reason: format!("{field} {text:?} is not an ID"),
        })
}

fn precondition_node(node: &Node) -> Result<ElementPrecondition, CoreError> {
    Ok(ElementPrecondition {
        id: node.id.clone(),
        expected_hash: node_element_hash(node)?,
    })
}

fn precondition_edge(edge: &Edge) -> Result<ElementPrecondition, CoreError> {
    Ok(ElementPrecondition {
        id: edge.id.clone(),
        expected_hash: edge_element_hash(edge)?,
    })
}

/// A new Accepted edge with a deterministic ID and the decision audit.
fn new_edge(kind: RelationKind, from: &Id, to: &Id, audit: &AuditMeta) -> Result<Edge, CoreError> {
    Ok(Edge {
        id: relation_id(&kind, from, to)?,
        revision: 1,
        status: ElementStatus::Accepted,
        kind,
        from: from.clone(),
        to: to.clone(),
        properties: RelationProperties::None,
        evidence: Vec::new(),
        derivations: Vec::new(),
        standards: Vec::new(),
        audit: audit.clone(),
    })
}

/// Outgoing edges of `kind` from `id` whose status satisfies `keep`.
fn outgoing<'g>(
    graph: &'g Graph,
    id: &Id,
    kind: &RelationKind,
    keep: impl Fn(ElementStatus) -> bool,
) -> Vec<&'g Edge> {
    graph
        .outgoing_edge_ids(id)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| &e.kind == kind && keep(e.status))
        .collect()
}

fn single(patches: Vec<SemanticPatch>) -> Option<SemanticPatch> {
    match patches.len() {
        0 => None,
        1 => patches.into_iter().next(),
        _ => Some(SemanticPatch::Compound { patches }),
    }
}

// ============================================================================ permission repair

/// The unique baseline `kind` edge of a Permission (Proposed and Rejected alternatives ignored).
fn authoritative_edge<'g>(
    graph: &'g Graph,
    permission: &Id,
    kind: RelationKind,
) -> Result<&'g Edge, ResolutionError> {
    let edges = outgoing(graph, permission, &kind, is_baseline);
    match edges.as_slice() {
        [only] => Ok(only),
        _ => Err(ResolutionError::InvalidPermissionStructure {
            permission_ref: permission.clone(),
            relation: kind.as_str().to_owned(),
            count: edges.len(),
        }),
    }
}

/// Whether a permission relation is concrete: an Accepted edge to an Accepted `target_type`.
fn concrete(graph: &Graph, edge: &Edge, target_type: NodeType) -> bool {
    edge.status == ElementStatus::Accepted
        && graph
            .node(&edge.to)
            .is_some_and(|n| accepted(n) && n.payload.node_type() == target_type)
}

/// Normalizes one authoritative permission edge to Accepted on `target`, preserving its ID,
/// kind, from and properties; a Suspect edge on another target is retargeted first and then
/// promoted under the intermediate element hash.
fn normalize_edge(edge: &Edge, target: &Id) -> Result<Vec<SemanticPatch>, ResolutionError> {
    let replace = |edge: &Edge| -> Result<SemanticPatch, ResolutionError> {
        Ok(SemanticPatch::ReplaceEdge {
            target: precondition_edge(edge)?,
            kind: edge.kind.clone(),
            from: edge.from.clone(),
            to: target.clone(),
            properties: edge.properties.clone(),
        })
    };
    match edge.status {
        ElementStatus::Accepted if &edge.to == target => Ok(Vec::new()),
        ElementStatus::Accepted => Ok(vec![replace(edge)?]),
        ElementStatus::Suspect => {
            let mut patches = Vec::new();
            let mut current = edge.clone();
            if &edge.to != target {
                patches.push(replace(edge)?);
                current.to = target.clone();
            }
            patches.push(SemanticPatch::SetStatus {
                target: precondition_edge(&current)?,
                from: ElementStatus::Suspect,
                to: ElementStatus::Accepted,
            });
            Ok(patches)
        }
        status => Err(ResolutionError::UnsupportedPermissionEdgeLifecycle {
            edge_ref: edge.id.clone(),
            status,
        }),
    }
}

// ============================================================================ resolution

/// What one mapper produced.
struct Mapped {
    marker: JsonValue,
    effect: Option<SemanticPatch>,
}

fn stale(question_ref: &Id, reason: &str) -> ResolutionError {
    ResolutionError::StaleResolutionQuestion {
        question_ref: question_ref.clone(),
        reason: reason.to_owned(),
    }
}

fn mismatch(old: &Id, reason: &str) -> ResolutionError {
    ResolutionError::SupersessionStateMismatch {
        decision_ref: old.clone(),
        reason: reason.to_owned(),
    }
}

/// The marker field `field` of a superseded decision as an ID.
fn old_ref(old: &ResolutionDecision, old_id: &Id, field: &str) -> Result<Id, ResolutionError> {
    old.answer
        .get(field)
        .and_then(JsonValue::as_str)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| ResolutionError::InvalidSupersededDecision {
            decision_ref: old_id.clone(),
            reason: format!("marker has no {field}"),
        })
}

#[allow(clippy::too_many_arguments)]
fn map_answer(
    graph: &Graph,
    template: ResolutionTemplate,
    question_ref: &Id,
    subject: &Id,
    answer: &JsonValue,
    context: &ResolutionContext,
    superseded: Option<(&Id, &ResolutionDecision)>,
    audit: &AuditMeta,
) -> Result<Mapped, ResolutionError> {
    let s = subject.to_string();
    match template {
        ResolutionTemplate::DomainRelationshipCardinality => {
            if !subject.as_str().starts_with("domainrel:") {
                return Err(ResolutionError::InvalidQuestionContract {
                    question_ref: question_ref.clone(),
                    reason: format!("subject {subject} is not a domainrel: candidate"),
                });
            }
            Ok(Mapped {
                marker: json!({"kind": template.marker_kind(), "candidate_ref": s,
                               "cardinality_from": answer["cardinality_from"],
                               "cardinality_to": answer["cardinality_to"]}),
                effect: None,
            })
        }
        ResolutionTemplate::StateTransitionTrigger => {
            if !subject.as_str().starts_with("transition:") {
                return Err(ResolutionError::InvalidQuestionContract {
                    question_ref: question_ref.clone(),
                    reason: format!("subject {subject} is not a transition: candidate"),
                });
            }
            let trigger = answer_id(answer, "trigger_ref")?;
            accepted_ref(graph, &trigger, &[NodeType::Operation, NodeType::Event])?;
            Ok(Mapped {
                marker: json!({"kind": template.marker_kind(), "candidate_ref": s,
                               "trigger_ref": trigger.to_string()}),
                effect: None,
            })
        }
        ResolutionTemplate::OperationPerformer => {
            accepted_subject(graph, question_ref, subject, NodeType::Operation)?;
            let performer = answer_id(answer, "performer_ref")?;
            accepted_ref(
                graph,
                &performer,
                &[NodeType::Actor, NodeType::BusinessRole],
            )?;
            let performed_by = outgoing(graph, subject, &RelationKind::PerformedBy, |st| {
                st == ElementStatus::Accepted
            });
            let effect = match superseded {
                None => {
                    if !performed_by.is_empty() {
                        return Err(stale(
                            question_ref,
                            "the operation already has an Accepted performer",
                        ));
                    }
                    SemanticPatch::AddEdge {
                        edge: new_edge(RelationKind::PerformedBy, subject, &performer, audit)?,
                    }
                }
                Some((old_id, old)) => {
                    let old_performer = old_ref(old, old_id, "performer_ref")?;
                    let matching: Vec<&&Edge> = performed_by
                        .iter()
                        .filter(|e| e.to == old_performer)
                        .collect();
                    let [edge] = matching.as_slice() else {
                        return Err(mismatch(
                            old_id,
                            "no unique Accepted performed_by matches the old performer",
                        ));
                    };
                    SemanticPatch::ReplaceEdge {
                        target: precondition_edge(edge)?,
                        kind: edge.kind.clone(),
                        from: edge.from.clone(),
                        to: performer.clone(),
                        properties: edge.properties.clone(),
                    }
                }
            };
            Ok(Mapped {
                marker: json!({"kind": template.marker_kind(), "operation_ref": s,
                               "performer_ref": performer.to_string()}),
                effect: Some(effect),
            })
        }
        ResolutionTemplate::CalculationCalendar => {
            let node = accepted_subject(graph, question_ref, subject, NodeType::Calculation)?;
            let calendar = answer_id(answer, "calendar_ref")?;
            accepted_ref(graph, &calendar, &[NodeType::Calendar])?;
            let NodePayload::Calculation(calculation) = &node.payload else {
                unreachable!("subject is a Calculation")
            };
            match superseded {
                None if calculation.calendar_ref.is_some() => {
                    return Err(stale(
                        question_ref,
                        "the calculation already has a calendar",
                    ));
                }
                Some((old_id, old)) => {
                    let old_calendar = old_ref(old, old_id, "calendar_ref")?;
                    if calculation.calendar_ref.as_ref() != Some(&old_calendar) {
                        return Err(mismatch(
                            old_id,
                            "the calculation calendar is not the old answer",
                        ));
                    }
                }
                None => {}
            }
            let mut payload = calculation.clone();
            payload.calendar_ref = Some(calendar.clone());
            Ok(Mapped {
                marker: json!({"kind": template.marker_kind(), "calculation_ref": s,
                               "calendar_ref": calendar.to_string()}),
                effect: Some(SemanticPatch::ReplacePayload {
                    target: precondition_node(node)?,
                    payload: NodePayload::Calculation(payload),
                }),
            })
        }
        ResolutionTemplate::PermissionBinding => {
            accepted_subject(graph, question_ref, subject, NodeType::Permission)?;
            let operation = answer_id(answer, "operation_ref")?;
            let scope = answer_id(answer, "resource_scope_ref")?;
            accepted_ref(graph, &operation, &[NodeType::Operation])?;
            accepted_ref(graph, &scope, &[NodeType::ResourceScope])?;
            let permits = authoritative_edge(graph, subject, RelationKind::Permits)?;
            let scoped_to = authoritative_edge(graph, subject, RelationKind::ScopedTo)?;
            match superseded {
                None => {
                    if concrete(graph, permits, NodeType::Operation)
                        && concrete(graph, scoped_to, NodeType::ResourceScope)
                    {
                        return Err(stale(
                            question_ref,
                            "both permission relations are already concrete",
                        ));
                    }
                }
                Some((old_id, old)) => {
                    let old_operation = old_ref(old, old_id, "operation_ref")?;
                    let old_scope = old_ref(old, old_id, "resource_scope_ref")?;
                    let matches = permits.status == ElementStatus::Accepted
                        && permits.to == old_operation
                        && scoped_to.status == ElementStatus::Accepted
                        && scoped_to.to == old_scope;
                    if !matches {
                        return Err(mismatch(
                            old_id,
                            "the permission relations are not the old answer",
                        ));
                    }
                }
            }
            let mut patches = normalize_edge(permits, &operation)?;
            patches.extend(normalize_edge(scoped_to, &scope)?);
            Ok(Mapped {
                marker: json!({"kind": template.marker_kind(), "permission_ref": s,
                               "operation_ref": operation.to_string(),
                               "resource_scope_ref": scope.to_string()}),
                effect: single(patches),
            })
        }
        ResolutionTemplate::InvariantFormula => {
            let node = accepted_subject(graph, question_ref, subject, NodeType::Invariant)?;
            let expression = answer
                .get("expression")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| ResolutionError::AnswerSchemaInvalid {
                    reason: "expression is not a string".into(),
                })?;
            let bindings = context
                .invariant_bindings
                .get(subject)
                .ok_or_else(|| ResolutionError::InvariantScopeUnavailable(subject.clone()))?;
            let scope = validate_expression_bindings(graph, bindings, &[]).map_err(|issue| {
                ResolutionError::InvalidInvariantExpression {
                    reason: format!("invalid expression scope: {issue:?}"),
                }
            })?;
            let ast =
                parse(expression).map_err(|e| ResolutionError::InvalidInvariantExpression {
                    reason: e.to_string(),
                })?;
            match typecheck(&ast, &scope.env) {
                Ok(Ty::Bool) => {}
                Ok(actual) => {
                    return Err(ResolutionError::InvariantNotBoolean {
                        actual: format!("{actual:?}"),
                    })
                }
                Err(e) => {
                    return Err(ResolutionError::InvalidInvariantExpression {
                        reason: e.to_string(),
                    })
                }
            }
            let NodePayload::Invariant(invariant) = &node.payload else {
                unreachable!("subject is an Invariant")
            };
            if let Some((old_id, old)) = superseded {
                let old_expression = old.answer.get("expression").and_then(JsonValue::as_str);
                if old_expression != Some(invariant.expression.as_str()) {
                    return Err(mismatch(
                        old_id,
                        "the invariant expression is not the old answer",
                    ));
                }
            }
            let mut payload = invariant.clone();
            payload.expression = expression.to_owned();
            Ok(Mapped {
                marker: json!({"kind": template.marker_kind(), "invariant_ref": s,
                               "expression": expression}),
                effect: Some(SemanticPatch::ReplacePayload {
                    target: precondition_node(node)?,
                    payload: NodePayload::Invariant(payload),
                }),
            })
        }
    }
}

/// The current linked Finding; a supersession allows it to be Closed already.
fn current_finding<'g>(
    graph: &'g Graph,
    question: &Question,
    superseding: bool,
) -> Result<(&'g Node, &'g Finding), ResolutionError> {
    let finding_ref = &question.finding_ref;
    let node = graph
        .node(finding_ref)
        .ok_or_else(|| ResolutionError::FindingMissing(finding_ref.clone()))?;
    let NodePayload::Finding(finding) = &node.payload else {
        return Err(ResolutionError::NotAFinding(finding_ref.clone()));
    };
    if !accepted(node) {
        return Err(ResolutionError::FindingNotAccepted(finding_ref.clone()));
    }
    let status_ok = finding.status == OPEN || (superseding && finding.status == CLOSED);
    if !status_ok {
        return Err(ResolutionError::FindingNotOpen {
            finding_ref: finding_ref.clone(),
            status: finding.status.clone(),
        });
    }
    if finding.waiver_ref.is_some() {
        return Err(ResolutionError::FindingWaived(finding_ref.clone()));
    }
    if let Some(r) = finding
        .affected_refs
        .iter()
        .find(|r| graph.node(r).is_none_or(|n| !accepted(n)))
    {
        return Err(ResolutionError::AffectedRefNotAccepted(r.clone()));
    }
    Ok((node, finding))
}

/// An Accepted decision already resolving this Question, if any.
fn existing_resolution(graph: &Graph, question_ref: &Id) -> Option<Id> {
    graph
        .node_ids_by_type(NodeType::ResolutionDecision)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| accepted(n))
        .find(|n| {
            matches!(&n.payload, NodePayload::ResolutionDecision(d)
                if d.question_ref.as_ref() == Some(question_ref))
        })
        .map(|n| n.id.clone())
}

/// Builds the human-decision proposal resolving one Question.
pub fn resolve_question(
    graph: &Graph,
    input: &ResolutionInput,
    context: &ResolutionContext,
) -> Result<ResolutionResult, ResolutionError> {
    let question_ref = &input.question_ref;
    let question_node = graph
        .node(question_ref)
        .ok_or_else(|| ResolutionError::QuestionMissing(question_ref.clone()))?;
    let NodePayload::Question(question) = &question_node.payload else {
        return Err(ResolutionError::NotAQuestion(question_ref.clone()));
    };
    if !accepted(question_node) {
        return Err(ResolutionError::QuestionNotAccepted(question_ref.clone()));
    }
    let superseding = input.supersedes.is_some();
    match (superseding, question.status.as_str()) {
        (false, OPEN) | (true, ANSWERED) => {}
        (false, ANSWERED) => {
            if let Some(decision_ref) = existing_resolution(graph, question_ref) {
                return Err(ResolutionError::AlreadyResolved {
                    question_ref: question_ref.clone(),
                    decision_ref,
                });
            }
            return Err(ResolutionError::QuestionNotOpen {
                question_ref: question_ref.clone(),
                status: question.status.clone(),
            });
        }
        (false, status) => {
            return Err(ResolutionError::QuestionNotOpen {
                question_ref: question_ref.clone(),
                status: status.to_owned(),
            })
        }
        (true, status) => {
            return Err(ResolutionError::QuestionNotAnsweredForSupersession {
                question_ref: question_ref.clone(),
                status: status.to_owned(),
            })
        }
    }
    let (finding_node, finding) = current_finding(graph, question, superseding)?;
    let subject = question
        .context_refs
        .as_ref()
        .and_then(|c| c.first())
        .ok_or_else(|| ResolutionError::MissingQuestionContext(question_ref.clone()))?;
    let template =
        ResolutionTemplate::of(&finding.code, question.question_kind).ok_or_else(|| {
            ResolutionError::UnsupportedResolutionTemplate {
                code: finding.code.clone(),
                kind: question.question_kind,
            }
        })?;

    // The persisted closed answer schema decides answer shape.
    let schema = question.answer_schema.as_ref().ok_or_else(|| {
        ResolutionError::InvalidQuestionContract {
            question_ref: question_ref.clone(),
            reason: "the question has no answer schema".into(),
        }
    })?;
    let compiled = JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(schema)
        .map_err(|e| ResolutionError::InvalidQuestionContract {
            question_ref: question_ref.clone(),
            reason: format!("answer schema does not compile: {e}"),
        })?;
    if let Err(errors) = compiled.validate(&input.answer) {
        let mut reasons: Vec<String> = errors.map(|e| e.to_string()).collect();
        reasons.sort();
        return Err(ResolutionError::AnswerSchemaInvalid {
            reason: reasons.join("; "),
        });
    }

    // Governance: the human actor and the rationale.
    let human = graph.node(&input.decided_by).is_some_and(|n| {
        accepted(n)
            && matches!(&n.payload, NodePayload::Agent(a) if a.agent_kind == AgentKind::Human)
    });
    if !human {
        return Err(ResolutionError::InvalidDecisionActor(
            input.decided_by.clone(),
        ));
    }
    let rationale_required = template.requires_rationale() || superseding;
    match &input.rationale {
        Some(text) if !clean(text) => return Err(ResolutionError::InvalidRationale),
        None if rationale_required => return Err(ResolutionError::InvalidRationale),
        _ => {}
    }

    // The superseded decision.
    let superseded = match &input.supersedes {
        None => None,
        Some(old_id) => {
            let invalid = |reason: &str| ResolutionError::InvalidSupersededDecision {
                decision_ref: old_id.clone(),
                reason: reason.to_owned(),
            };
            let old_node = graph
                .node(old_id)
                .ok_or_else(|| invalid("it does not exist"))?;
            let NodePayload::ResolutionDecision(old) = &old_node.payload else {
                return Err(invalid("it is not a ResolutionDecision"));
            };
            if old.question_ref.as_ref() != Some(question_ref) {
                return Err(invalid("it resolves another Question"));
            }
            if !accepted(old_node) {
                // A replay of an already-applied supersession creates nothing new.
                let replay = graph
                    .node_ids_by_type(NodeType::ResolutionDecision)
                    .iter()
                    .find(|id| {
                        graph.node(id).is_some_and(|n| {
                            accepted(n)
                                && matches!(&n.payload, NodePayload::ResolutionDecision(d)
                                if d.supersedes.as_ref() == Some(old_id))
                        })
                    });
                return Err(match replay {
                    Some(decision_ref) => ResolutionError::AlreadyResolved {
                        question_ref: question_ref.clone(),
                        decision_ref: decision_ref.clone(),
                    },
                    None => invalid("it is not Accepted"),
                });
            }
            if old.answer.get("kind").and_then(JsonValue::as_str) != Some(template.marker_kind()) {
                return Err(invalid("it belongs to another template family"));
            }
            Some((old_id, old_node, old))
        }
    };

    let audit =
        AuditMeta::new(input.decided_by.clone(), input.decided_at, None, None).map_err(|e| {
            ResolutionError::InvalidResolutionProposal {
                reason: e.to_string(),
            }
        })?;
    let mapped = map_answer(
        graph,
        template,
        question_ref,
        subject,
        &input.answer,
        context,
        superseded.as_ref().map(|(id, _, d)| (*id, *d)),
        &audit,
    )?;

    // The content-addressed effect artifact, hashed before the decision exists.
    let base_semantic_hash = graph.semantic_hash()?;
    let patch_artifact = ResolutionPatchArtifact {
        version: RESOLUTION_ARTIFACT_VERSION,
        question_ref: question_ref.clone(),
        base_semantic_hash: base_semantic_hash.clone(),
        effect_patch: mapped.effect.clone(),
    };
    let patch_artifact_bytes = patch_artifact.canonical_bytes()?;
    let patch_ref = Hash::content_sha256(&patch_artifact_bytes);
    let decision_ref = decision_id(
        graph.project_id(),
        question_ref,
        &mapped.marker,
        &input.decided_by,
        &input.decided_at,
        &patch_ref,
        input.supersedes.as_ref(),
    )?;
    let decision = ResolutionDecision {
        question_ref: Some(question_ref.clone()),
        proposal_ref: None,
        answer: mapped.marker,
        decided_by: input.decided_by.clone(),
        decided_at: input.decided_at,
        patch_ref,
        rationale: input.rationale.clone(),
        supersedes: input.supersedes.clone(),
    };
    if let Some(existing) = graph.node(&decision_ref) {
        let exact = accepted(existing)
            && matches!(&existing.payload, NodePayload::ResolutionDecision(d) if *d == decision);
        return Err(if exact {
            ResolutionError::AlreadyResolved {
                question_ref: question_ref.clone(),
                decision_ref,
            }
        } else {
            ResolutionError::ResolutionDecisionIdCollision(decision_ref)
        });
    }
    if graph.edge(&decision_ref).is_some() {
        return Err(ResolutionError::ResolutionDecisionIdCollision(decision_ref));
    }

    // Proposal: effect, decision, resolves edges, supersession, lifecycle.
    let mut patches = Vec::new();
    patches.extend(mapped.effect);
    patches.push(SemanticPatch::AddNode {
        node: Node {
            id: decision_ref.clone(),
            revision: 1,
            status: ElementStatus::Accepted,
            payload: NodePayload::ResolutionDecision(decision.clone()),
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: Default::default(),
            extensions: Default::default(),
            audit: audit.clone(),
        },
    });
    for target in [question_ref, &finding_node.id] {
        patches.push(SemanticPatch::AddEdge {
            edge: new_edge(RelationKind::Resolves, &decision_ref, target, &audit)?,
        });
    }
    match &superseded {
        Some((old_id, old_node, _)) => {
            patches.push(SemanticPatch::AddEdge {
                edge: new_edge(RelationKind::Supersedes, &decision_ref, old_id, &audit)?,
            });
            patches.push(SemanticPatch::SetStatus {
                target: precondition_node(old_node)?,
                from: ElementStatus::Accepted,
                to: ElementStatus::Superseded,
            });
        }
        None => {
            let mut answered = question.clone();
            answered.status = ANSWERED.to_owned();
            patches.push(SemanticPatch::ReplacePayload {
                target: precondition_node(question_node)?,
                payload: NodePayload::Question(answered),
            });
            let sibling_open = graph.node_ids_by_type(NodeType::Question).iter().any(|id| {
                id != question_ref
                    && graph.node(id).is_some_and(|n| {
                        accepted(n)
                            && matches!(&n.payload, NodePayload::Question(q)
                                if q.finding_ref == finding_node.id && q.status == OPEN)
                    })
            });
            if !sibling_open {
                let mut closed = finding.clone();
                closed.status = CLOSED.to_owned();
                patches.push(SemanticPatch::ReplacePayload {
                    target: precondition_node(finding_node)?,
                    payload: NodePayload::Finding(closed),
                });
            }
        }
    }
    let invalid = |e: String| ResolutionError::InvalidResolutionProposal { reason: e };
    let proposal = Proposal::new(
        RESOLUTION_STAGE,
        PatchSet {
            base_semantic_hash,
            patch: SemanticPatch::Compound { patches },
        },
        Vec::new(),
        Vec::new(),
        ProposalMateriality::MaterialDecision,
        AcceptancePolicy::HumanDecision,
        None,
    )
    .map_err(|e| invalid(e.to_string()))?;
    apply_patch(graph, &proposal.patch_set)
        .map_err(|e| invalid(format!("proposal does not apply: {e}")))?;
    Ok(ResolutionResult {
        template,
        decision_ref,
        decision,
        patch_artifact,
        patch_artifact_bytes,
        proposal,
    })
}
