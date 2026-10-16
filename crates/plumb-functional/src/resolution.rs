//! ResolutionDecision and answer-to-patch application (plan S3.3, Hotfix 047; compiler
//! architecture §9; metamodel §6.3).
//!
//! One human answer to an S3.1 Question becomes one human-decision proposal: an optional
//! explicit semantic answer effect, an Accepted ResolutionDecision with its governance edges and
//! the Question/Finding lifecycle updates. The mapper is identified only by (Finding code,
//! QuestionKind) for the nine current templates and the subject only by `context_refs[0]`; the
//! three calculation families (Hotfix 083) also prove their exact condition key. The
//! effect is recorded in a separate content-addressed artifact whose hash is the decision's
//! `patch_ref`. Nothing here applies the proposal, reads files, observes time or infers.

use std::collections::BTreeMap;
use std::str::FromStr;

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_expr::{parse, typecheck, Ty, Unit};
use plumb_patch::{
    apply_patch, AcceptancePolicy, ElementPrecondition, PatchSet, Proposal, ProposalMateriality,
    SemanticPatch,
};
use plumb_psg::{
    edge_element_hash, is_baseline, node_element_hash, AgentKind, AuditMeta, Edge, ElementStatus,
    Finding, Graph, Node, NodePayload, NodeType, Question, QuestionKind, RelationKind,
    RelationProperties, ResolutionDecision,
};
use plumb_validation::calculation_analysis::{
    calculation_origin_of, validate_calculation_bindings, CalculationBindingExposure,
};
use plumb_validation::expression_scope::{
    attribute_expression_type, validate_expression_bindings, ExpressionScopeBinding,
};
use plumb_validation::{finding_id, finding_key};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

use crate::calculation::{
    calculation_fraction_semantic_key, rewrite_working_days_inclusive_end, InclusiveEndRewriteError,
};
use crate::question_templates::{
    CALCULATION_FRACTION_CONDITION_PREFIX, CALCULATION_HOLIDAY_POLICY_CONDITION_PREFIX,
    CALCULATION_INCLUSIVE_END_CONDITION_PREFIX,
};
use crate::scenario::{CalculationExampleInput, CalculationExampleV1, ScenarioLiteral};

const RESOLUTION_STAGE: StageId = StageId::S3;

/// The ResolutionPatchArtifact format version.
pub const RESOLUTION_ARTIFACT_VERSION: u32 = 1;

const OPEN: &str = "Open";
const ANSWERED: &str = "Answered";
const CLOSED: &str = "Closed";

// ============================================================================ templates

/// The nine resolvable S3.1 template families (Hotfix 083 adds the last three).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionTemplate {
    DomainRelationshipCardinality,
    StateTransitionTrigger,
    OperationPerformer,
    CalculationCalendar,
    PermissionBinding,
    InvariantFormula,
    CalculationFraction,
    CalculationInclusiveEnd,
    CalculationHolidayPolicy,
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
            ("PLUMB.F2.CALC.TYPECHECK", QuestionKind::Number) => Some(CalculationFraction),
            ("PLUMB.F2.CALC.TYPECHECK", QuestionKind::YesNo) => Some(CalculationInclusiveEnd),
            ("PLUMB.F2.TIME.CALENDAR_DEFINED", QuestionKind::YesNo) => {
                Some(CalculationHolidayPolicy)
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
            ResolutionTemplate::CalculationFraction => "calculation_fraction",
            ResolutionTemplate::CalculationInclusiveEnd => "calculation_inclusive_end",
            ResolutionTemplate::CalculationHolidayPolicy => "calculation_holiday_policy",
        }
    }

    /// The frozen condition prefix whose exact `prefix + subject` key the linked Finding must
    /// carry (Hotfix 083); the original six families are identified by (code, kind) alone.
    fn condition_prefix(self) -> Option<&'static str> {
        match self {
            ResolutionTemplate::CalculationFraction => Some(CALCULATION_FRACTION_CONDITION_PREFIX),
            ResolutionTemplate::CalculationInclusiveEnd => {
                Some(CALCULATION_INCLUSIVE_END_CONDITION_PREFIX)
            }
            ResolutionTemplate::CalculationHolidayPolicy => {
                Some(CALCULATION_HOLIDAY_POLICY_CONDITION_PREFIX)
            }
            _ => None,
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
    #[error("calculation {calculation_ref} cannot take this governed answer: {reason}")]
    InvalidCalculationAnswer { calculation_ref: Id, reason: String },
    #[error("a configured holiday never consumes a working day; holiday_consumes_working_day = true is unsupported")]
    UnsupportedHolidayPolicy,
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

/// `dec:<16 hex of SHA-256(RFC 8785 {project_id, question_ref, answer, decided_by,
/// supersedes})>` (R0.5, architecture §4.8). The rationale is not part of decision identity, and
/// `decided_at` and `patch_ref` are operational provenance: persisted on the decision, never
/// identity.
pub fn decision_id(
    project_id: &Id,
    question_ref: &Id,
    answer: &JsonValue,
    decided_by: &Id,
    supersedes: Option<&Id>,
) -> Result<Id, CoreError> {
    short_id(
        "dec",
        &json!({
            "project_id": project_id.to_string(),
            "question_ref": question_ref.to_string(),
            "answer": answer,
            "decided_by": decided_by.to_string(),
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

/// A canonical fraction: decimal text equal to its normalized form (no sign prefix, no trailing
/// zeros), never a binary number.
fn canonical_decimal(text: &str) -> Option<Decimal> {
    Decimal::from_str(text)
        .ok()
        .filter(|d| d.normalize().to_string() == text)
}

fn answer_str<'a>(answer: &'a JsonValue, field: &str) -> Result<&'a str, ResolutionError> {
    answer
        .get(field)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| ResolutionError::AnswerSchemaInvalid {
            reason: format!("{field} is not a string"),
        })
}

fn answer_bool(answer: &JsonValue, field: &str) -> Result<bool, ResolutionError> {
    answer
        .get(field)
        .and_then(JsonValue::as_bool)
        .ok_or_else(|| ResolutionError::AnswerSchemaInvalid {
            reason: format!("{field} is not a boolean"),
        })
}

/// The marker field `field` of a superseded decision as text.
fn old_str<'d>(
    old: &'d ResolutionDecision,
    old_id: &Id,
    field: &str,
) -> Result<&'d str, ResolutionError> {
    old.answer
        .get(field)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| ResolutionError::InvalidSupersededDecision {
            decision_ref: old_id.clone(),
            reason: format!("marker has no {field}"),
        })
}

/// The governed fraction example of `key` (Hotfix 083): one input, the exact root symbol, the
/// fraction without a unit, and the same fraction in the calculation's result unit.
fn fraction_example(
    key: &str,
    symbol: &str,
    decimal: &str,
    unit: &str,
) -> Result<JsonValue, CoreError> {
    serde_json::to_value(CalculationExampleV1 {
        version: 1,
        key: key.to_owned(),
        inputs: vec![CalculationExampleInput {
            symbol: symbol.to_owned(),
            value: ScenarioLiteral::Decimal {
                value: decimal.to_owned(),
                unit: None,
            },
        }],
        expected_result: Some(ScenarioLiteral::Decimal {
            value: decimal.to_owned(),
            unit: Some(unit.to_owned()),
        }),
    })
    .map_err(|e| CoreError::Canonicalization(e.to_string()))
}

/// `ReplacePayload` of an Accepted Calculation, or no effect when the payload is unchanged.
fn calculation_effect(
    node: &Node,
    current: &plumb_psg::Calculation,
    payload: plumb_psg::Calculation,
) -> Result<Option<SemanticPatch>, ResolutionError> {
    if &payload == current {
        return Ok(None);
    }
    Ok(Some(SemanticPatch::ReplacePayload {
        target: precondition_node(node)?,
        payload: NodePayload::Calculation(payload),
    }))
}

#[allow(clippy::too_many_arguments)]
fn map_answer(
    graph: &Graph,
    template: ResolutionTemplate,
    question_ref: &Id,
    subject: &Id,
    context_refs: &[Id],
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
        ResolutionTemplate::CalculationFraction => {
            let node = accepted_subject(graph, question_ref, subject, NodeType::Calculation)?;
            let NodePayload::Calculation(calculation) = &node.payload else {
                unreachable!("subject is a Calculation")
            };
            let invalid = |reason: String| ResolutionError::InvalidCalculationAnswer {
                calculation_ref: subject.clone(),
                reason,
            };
            // The fractional input: exactly one other Accepted Attribute of the Question context.
            let attributes: Vec<&Node> = context_refs
                .iter()
                .filter(|r| *r != subject)
                .filter_map(|r| graph.node(r))
                .filter(|n| accepted(n) && n.payload.node_type() == NodeType::Attribute)
                .collect();
            let [attribute_node] = attributes.as_slice() else {
                return Err(invalid(format!(
                    "the question context names {} Accepted Attributes, not exactly one",
                    attributes.len()
                )));
            };
            // A unitless Decimal input (a unit or another value type maps to another Ty).
            let scale = match attribute_expression_type(attribute_node) {
                Ok((Ty::Decimal(scale), None)) => scale,
                _ => {
                    return Err(invalid(format!(
                        "{} is not a unitless Decimal Attribute",
                        attribute_node.id
                    )))
                }
            };
            // The S2.6 origin is the only binding authority.
            let origin = calculation_origin_of(node)
                .ok_or_else(|| invalid("the calculation has no valid CalculationOrigin".into()))?;
            validate_calculation_bindings(graph, &origin.used_bindings).map_err(|issue| {
                invalid(format!(
                    "the CalculationOrigin bindings are stale: {issue:?}"
                ))
            })?;
            let roots: Vec<&ExpressionScopeBinding> = origin
                .used_bindings
                .iter()
                .filter(|b| {
                    b.node_ref == attribute_node.id
                        && b.exposure == CalculationBindingExposure::Root
                })
                .collect();
            let [binding] = roots.as_slice() else {
                return Err(invalid(format!(
                    "{} has {} root bindings, not exactly one",
                    attribute_node.id,
                    roots.len()
                )));
            };
            let decimal = answer_str(answer, "decimal")?;
            let unit = answer_str(answer, "unit")?;
            let value = canonical_decimal(decimal)
                .ok_or_else(|| invalid(format!("{decimal:?} is not a canonical decimal")))?;
            if value <= Decimal::ZERO || value > Decimal::ONE {
                return Err(invalid(format!(
                    "{decimal} is not within 0 < fraction <= 1"
                )));
            }
            if value.scale() > u32::from(scale) {
                return Err(invalid(format!(
                    "{decimal} exceeds the input scale {scale}"
                )));
            }
            if Unit::new(unit).is_err() {
                return Err(invalid(format!("{unit:?} is not a unit")));
            }
            if calculation.unit.as_deref() != Some(unit) {
                return Err(invalid(format!(
                    "{unit} is not the calculation result unit {:?}",
                    calculation.unit
                )));
            }
            let key = calculation_fraction_semantic_key(subject, &attribute_node.id)?;
            let mut examples = calculation.examples.clone().unwrap_or_default();
            let governed: Vec<usize> = examples
                .iter()
                .enumerate()
                .filter(|(_, e)| e.get("key").and_then(JsonValue::as_str) == Some(key.as_str()))
                .map(|(i, _)| i)
                .collect();
            let new_example = fraction_example(&key, &binding.symbol, decimal, unit)?;
            match superseded {
                None => {
                    if !governed.is_empty() {
                        return Err(stale(
                            question_ref,
                            "the governed fraction example already exists",
                        ));
                    }
                    examples.push(new_example);
                }
                Some((old_id, old)) => {
                    if old_str(old, old_id, "semantic_key")? != key
                        || old_str(old, old_id, "attribute_ref")? != attribute_node.id.as_str()
                    {
                        return Err(mismatch(old_id, "the old decision governs another input"));
                    }
                    let previous = fraction_example(
                        &key,
                        &binding.symbol,
                        old_str(old, old_id, "decimal")?,
                        old_str(old, old_id, "unit")?,
                    )?;
                    let [index] = governed.as_slice() else {
                        return Err(mismatch(old_id, "no unique governed fraction example"));
                    };
                    if examples[*index] != previous {
                        return Err(mismatch(
                            old_id,
                            "the governed fraction example is not the old answer",
                        ));
                    }
                    examples[*index] = new_example;
                }
            }
            let mut payload = calculation.clone();
            payload.examples = Some(examples);
            Ok(Mapped {
                marker: json!({"kind": template.marker_kind(), "calculation_ref": s,
                               "attribute_ref": attribute_node.id.to_string(),
                               "semantic_key": key, "decimal": decimal, "unit": unit}),
                effect: calculation_effect(node, calculation, payload)?,
            })
        }
        ResolutionTemplate::CalculationInclusiveEnd => {
            let node = accepted_subject(graph, question_ref, subject, NodeType::Calculation)?;
            let NodePayload::Calculation(calculation) = &node.payload else {
                unreachable!("subject is a Calculation")
            };
            let inclusive_end = answer_bool(answer, "inclusive_end")?;
            let current = match superseded {
                None => None,
                Some((old_id, old)) => Some(
                    old.answer
                        .get("inclusive_end")
                        .and_then(JsonValue::as_bool)
                        .ok_or_else(|| ResolutionError::InvalidSupersededDecision {
                            decision_ref: old_id.clone(),
                            reason: "marker has no boolean inclusive_end".into(),
                        })?,
                ),
            };
            let expression =
                rewrite_working_days_inclusive_end(&calculation.expression, current, inclusive_end)
                    .map_err(|error| match (&error, superseded) {
                        (InclusiveEndRewriteError::UnexpectedInclusiveArgument { found }, None)
                            if found == "true" || found == "false" =>
                        {
                            stale(
                                question_ref,
                                "the inclusive-end argument is already decided",
                            )
                        }
                        (
                            InclusiveEndRewriteError::UnexpectedInclusiveArgument { .. },
                            Some((old_id, _)),
                        ) => mismatch(old_id, "the inclusive-end argument is not the old answer"),
                        _ => ResolutionError::InvalidCalculationAnswer {
                            calculation_ref: subject.clone(),
                            reason: error.to_string(),
                        },
                    })?;
            let mut payload = calculation.clone();
            payload.expression = expression;
            Ok(Mapped {
                marker: json!({"kind": template.marker_kind(), "calculation_ref": s,
                               "inclusive_end": inclusive_end}),
                effect: calculation_effect(node, calculation, payload)?,
            })
        }
        ResolutionTemplate::CalculationHolidayPolicy => {
            let node = accepted_subject(graph, question_ref, subject, NodeType::Calculation)?;
            let NodePayload::Calculation(calculation) = &node.payload else {
                unreachable!("subject is a Calculation")
            };
            // PlumbExpr calendars treat every configured holiday as a non-working day; the answer
            // can only confirm that fixed policy.
            if answer_bool(answer, "holiday_consumes_working_day")? {
                return Err(ResolutionError::UnsupportedHolidayPolicy);
            }
            let calendar = answer_id(answer, "calendar_ref")?;
            accepted_ref(graph, &calendar, &[NodeType::Calendar])?;
            match superseded {
                None => {
                    if calculation
                        .calendar_ref
                        .as_ref()
                        .is_some_and(|current| current != &calendar)
                    {
                        return Err(stale(
                            question_ref,
                            "the calculation already uses another calendar",
                        ));
                    }
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
            }
            let mut payload = calculation.clone();
            payload.calendar_ref = Some(calendar.clone());
            Ok(Mapped {
                marker: json!({"kind": template.marker_kind(), "calculation_ref": s,
                               "calendar_ref": calendar.to_string(),
                               "holiday_consumes_working_day": false}),
                effect: calculation_effect(node, calculation, payload)?,
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
    // Hotfix 083: a calculation family also requires the linked Finding to be exactly the
    // `prefix + subject` condition; the deterministic Finding ID proves its condition key.
    if let Some(prefix) = template.condition_prefix() {
        let contract = |reason: String| ResolutionError::InvalidQuestionContract {
            question_ref: question_ref.clone(),
            reason,
        };
        let condition = format!("{prefix}{subject}");
        let key = finding_key(&finding.code, &finding.affected_refs, &condition)
            .map_err(|e| contract(e.to_string()))?;
        if finding_id(&key).map_err(|e| contract(e.to_string()))? != finding_node.id {
            return Err(contract(format!(
                "the linked finding is not the {condition} condition"
            )));
        }
    }

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
        question.context_refs.as_deref().unwrap_or_default(),
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

// ============================================================================ governed acceptance

/// The `answer.kind` of a proposal-acceptance companion decision (Hotfix 065, metamodel §6.3).
pub const ACCEPTANCE_DECISION_KIND: &str = "proposal_acceptance";

/// `dec:<16 hex of SHA-256(RFC 8785 {kind: proposal_acceptance, project_id, proposal_ref,
/// decided_by, decided_at, patch_ref})>`; the rationale and reason codes are not part of the
/// identity (metamodel §6.3, Hotfix 065).
pub fn acceptance_decision_id(
    project_id: &Id,
    proposal_ref: &Id,
    decided_by: &Id,
    decided_at: &Timestamp,
    patch_ref: &Hash,
) -> Result<Id, CoreError> {
    short_id(
        "dec",
        &json!({
            "kind": ACCEPTANCE_DECISION_KIND,
            "project_id": project_id.to_string(),
            "proposal_ref": proposal_ref.to_string(),
            "decided_by": decided_by.to_string(),
            "decided_at": serde_json::to_value(decided_at).map_err(|e| CoreError::Canonicalization(e.to_string()))?,
            "patch_ref": patch_ref.as_str(),
        }),
    )
}

/// One governed acceptance of an already-built proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptanceInput {
    /// The sorted unique wire strings of the intake report that gated the acceptance.
    pub reason_codes: Vec<String>,
    pub decided_by: Id,
    pub decided_at: Timestamp,
    /// The clean caller-supplied reason, if any.
    pub rationale: Option<String>,
}

/// The companion decision and the combined acceptance patch; nothing has been applied.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptanceCompanion {
    pub decision_ref: Id,
    /// The Accepted, proposal-only companion ResolutionDecision node.
    pub decision_node: Node,
    /// RFC 8785 bytes of the original PatchSet, to persist as a Patch artifact; their generic
    /// SHA-256 is the companion's `patch_ref`.
    pub original_patch_bytes: Vec<u8>,
    pub patch_ref: Hash,
    /// `Compound {original patch, AddNode companion}` on the original base semantic hash,
    /// dry-run against the graph.
    pub patch_set: PatchSet,
}

/// Builds the proposal-acceptance companion of `original` (metamodel §6.3, Hotfix 065): an
/// Accepted ResolutionDecision `{question_ref None, proposal_ref original.id, answer {kind,
/// reason_codes}}` whose `patch_ref` is the original PatchSet, added in one Compound after the
/// unchanged original effect. The original proposal is never rewritten or re-identified.
pub fn build_acceptance_companion(
    graph: &Graph,
    original: &Proposal,
    input: &AcceptanceInput,
) -> Result<AcceptanceCompanion, ResolutionError> {
    let invalid = |reason: String| ResolutionError::InvalidResolutionProposal { reason };
    if let Some(rationale) = &input.rationale {
        if !clean(rationale) {
            return Err(ResolutionError::InvalidRationale);
        }
    }
    let mut reason_codes = input.reason_codes.clone();
    reason_codes.sort();
    reason_codes.dedup();
    if reason_codes != input.reason_codes || reason_codes.iter().any(|c| !clean(c)) {
        return Err(invalid(
            "reason codes must be sorted, unique, clean wire strings".to_owned(),
        ));
    }
    let original_patch_bytes = to_canonical_json(&original.patch_set)?;
    let patch_ref = Hash::content_sha256(&original_patch_bytes);
    let decision_ref = acceptance_decision_id(
        graph.project_id(),
        &original.id,
        &input.decided_by,
        &input.decided_at,
        &patch_ref,
    )?;
    if graph.node(&decision_ref).is_some() || graph.edge(&decision_ref).is_some() {
        return Err(ResolutionError::ResolutionDecisionIdCollision(decision_ref));
    }
    let decision = ResolutionDecision {
        question_ref: None,
        proposal_ref: Some(original.id.clone()),
        answer: json!({"kind": ACCEPTANCE_DECISION_KIND, "reason_codes": reason_codes}),
        decided_by: input.decided_by.clone(),
        decided_at: input.decided_at,
        patch_ref: patch_ref.clone(),
        rationale: input.rationale.clone(),
        supersedes: None,
    };
    let audit = AuditMeta::new(input.decided_by.clone(), input.decided_at, None, None)
        .map_err(|e| invalid(e.to_string()))?;
    let decision_node = Node {
        id: decision_ref.clone(),
        revision: 1,
        status: ElementStatus::Accepted,
        payload: NodePayload::ResolutionDecision(decision),
        evidence: Vec::new(),
        derivations: Vec::new(),
        standards: Vec::new(),
        tags: Default::default(),
        extensions: Default::default(),
        audit,
    };
    let patch_set = PatchSet {
        base_semantic_hash: original.patch_set.base_semantic_hash.clone(),
        patch: SemanticPatch::Compound {
            patches: vec![
                original.patch_set.patch.clone(),
                SemanticPatch::AddNode {
                    node: decision_node.clone(),
                },
            ],
        },
    };
    apply_patch(graph, &patch_set)
        .map_err(|e| invalid(format!("acceptance patch does not apply: {e}")))?;
    Ok(AcceptanceCompanion {
        decision_ref,
        decision_node,
        original_patch_bytes,
        patch_ref,
        patch_set,
    })
}

#[cfg(test)]
mod calculation_answer_tests {
    //! Hotfix 083: the three generic calculation answer families through `resolve_question`
    //! over a non-HR graph; every effect is observed by dry-applying the returned proposal.

    use std::collections::BTreeSet;

    use plumb_psg::{
        Agent, Attribute, Calculation, Calendar, Entity, ExtensionKey, FindingSeverity,
    };
    use plumb_validation::calculation_analysis::{
        CalculationGroundedRange, CalculationOrigin, CALCULATION_ORIGIN_EXTENSION,
    };
    use plumb_validation::expression_scope::ExpressionBindingExposure;

    use super::*;
    use crate::calculation::calculation_fraction_semantic_key;
    use crate::question_templates::{Choices, QUESTION_TEMPLATES};

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const DECIDED: &str = "2026-03-01T09:30:00.000000000Z";
    const HUMAN: &str = "agent:human";
    const CALC: &str = "calc:days";
    const EXPRESSION: &str = "working_days([begin, finish], calendar, inclusive_end) * part";

    fn rid(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn audit() -> AuditMeta {
        AuditMeta::new(rid("agent:analyst"), AT.parse().unwrap(), None, None).unwrap()
    }

    fn node(id: &str, payload: NodePayload) -> Node {
        Node {
            id: rid(id),
            revision: 1,
            status: ElementStatus::Accepted,
            payload,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: Default::default(),
            extensions: Default::default(),
            audit: audit(),
        }
    }

    fn attribute(id: &str, value_type: &str, unit: Option<&str>) -> Node {
        node(
            id,
            NodePayload::Attribute(Attribute {
                name: id.into(),
                value_type: value_type.into(),
                nullable: false,
                unit: unit.map(str::to_owned),
                precision: None,
                enum_values: None,
                data_classification: None,
            }),
        )
    }

    fn calendar(id: &str) -> Node {
        node(
            id,
            NodePayload::Calendar(Calendar {
                time_zone: "UTC".into(),
                week_pattern: json!(["monday", "tuesday", "wednesday", "thursday", "friday"]),
                region: None,
                holiday_source: None,
            }),
        )
    }

    fn root(symbol: &str, node_ref: &str) -> ExpressionScopeBinding {
        ExpressionScopeBinding {
            node_ref: rid(node_ref),
            symbol: symbol.into(),
            exposure: ExpressionBindingExposure::Root,
        }
    }

    /// The calculation with an S2.6 origin binding `bindings`.
    fn calculation(
        expression: &str,
        calendar_ref: Option<&str>,
        bindings: Option<Vec<ExpressionScopeBinding>>,
    ) -> Node {
        let mut n = node(
            CALC,
            NodePayload::Calculation(Calculation {
                name: "Requested days".into(),
                expression: expression.into(),
                result_type: "Decimal(2)".into(),
                unit: Some("working_day".into()),
                rounding: None,
                calendar_ref: calendar_ref.map(rid),
                examples: Some(vec![
                    json!({"version": 1, "key": "unrelated", "inputs": [], "expected_result": null}),
                ]),
            }),
        );
        if let Some(used_bindings) = bindings {
            let origin = CalculationOrigin {
                name_range: CalculationGroundedRange {
                    requirement_ref: rid("req:days"),
                    start: 0,
                    end: 4,
                },
                expression_evidence: Vec::new(),
                used_bindings,
            };
            n.extensions.insert(
                CALCULATION_ORIGIN_EXTENSION
                    .parse::<ExtensionKey>()
                    .unwrap(),
                serde_json::to_value(origin).unwrap(),
            );
        }
        n
    }

    fn bindings() -> Vec<ExpressionScopeBinding> {
        vec![
            root("begin", "attr:begin"),
            root("finish", "attr:finish"),
            root("inclusive_end", "attr:flag"),
            root("part", "attr:part"),
        ]
    }

    fn graph_with(calc: Node, extra: Vec<Node>) -> Graph {
        let mut nodes = vec![
            node(
                HUMAN,
                NodePayload::Agent(Agent {
                    agent_kind: AgentKind::Human,
                }),
            ),
            attribute("attr:begin", "Date", None),
            attribute("attr:finish", "Date", None),
            attribute("attr:flag", "Bool", None),
            attribute("attr:part", "Decimal(2)", None),
            attribute("attr:hours", "Decimal(2)", Some("hour")),
            attribute("attr:spare", "Decimal(2)", None),
            calendar("cal:test"),
            calendar("cal:other"),
            calc,
        ];
        nodes.push(node(
            "entity:leave",
            NodePayload::Entity(Entity {
                name: "Leave".into(),
                description: None,
                aggregate_root: None,
            }),
        ));
        nodes.extend(extra);
        let edges = [
            "attr:begin",
            "attr:finish",
            "attr:flag",
            "attr:hours",
            "attr:part",
            "attr:spare",
        ]
        .iter()
        .map(|attr| Edge {
            id: relation_id(
                &RelationKind::HasAttribute,
                &rid("entity:leave"),
                &rid(attr),
            )
            .unwrap(),
            revision: 1,
            status: ElementStatus::Accepted,
            kind: RelationKind::HasAttribute,
            from: rid("entity:leave"),
            to: rid(attr),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: audit(),
        })
        .collect();
        Graph::new(rid("project:test"), rid("profile:test"), nodes, edges).unwrap()
    }

    /// `graph` plus the Accepted Open Finding of `code`/`condition` and its Open Question of the
    /// template `template_id` about `subject`, exactly as S3.1 would persist them.
    fn asked(
        graph: &Graph,
        template_id: &str,
        code: &str,
        condition: &str,
        affected: &[&str],
    ) -> (Graph, Id) {
        let template = QUESTION_TEMPLATES
            .iter()
            .find(|t| t.template_id == template_id)
            .unwrap();
        let mut affected: Vec<Id> = affected.iter().map(|a| rid(a)).collect();
        affected.sort();
        let key = finding_key(code, &affected, condition).unwrap();
        let finding_ref = finding_id(&key).unwrap();
        let mut nodes: Vec<Node> = graph.nodes().values().cloned().collect();
        nodes.push(node(
            finding_ref.as_str(),
            NodePayload::Finding(Finding {
                code: code.into(),
                family: "F2".into(),
                severity: FindingSeverity::Blocker,
                message: "unresolved".into(),
                status: OPEN.into(),
                affected_refs: affected.clone(),
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            }),
        ));
        let subject = rid(CALC);
        let question_ref = rid("q:asked");
        nodes.push(node(
            question_ref.as_str(),
            NodePayload::Question(Question {
                finding_ref,
                question_kind: template.kind,
                prompt: "What is decided?".into(),
                status: OPEN.into(),
                answer_schema: Some(template.answer_schema(&Choices::of(graph, template.choices))),
                stakeholder_ref: None,
                priority: Some("4".into()),
                round_ref: None,
                context_refs: Some(crate::question::subject_first_context(&subject, &affected)),
            }),
        ));
        let graph = Graph::new(
            graph.project_id().clone(),
            rid("profile:test"),
            nodes,
            graph.edges().values().cloned().collect(),
        )
        .unwrap();
        (graph, question_ref)
    }

    fn fraction_asked(graph: &Graph, affected: &[&str]) -> (Graph, Id) {
        asked(
            graph,
            "f2.calculation.fraction.v1",
            "PLUMB.F2.CALC.TYPECHECK",
            &format!("{CALCULATION_FRACTION_CONDITION_PREFIX}{CALC}"),
            affected,
        )
    }

    fn inclusive_asked(graph: &Graph) -> (Graph, Id) {
        asked(
            graph,
            "f2.calculation.inclusive-end.v1",
            "PLUMB.F2.CALC.TYPECHECK",
            &format!("{CALCULATION_INCLUSIVE_END_CONDITION_PREFIX}{CALC}"),
            &[CALC],
        )
    }

    fn holiday_asked(graph: &Graph) -> (Graph, Id) {
        asked(
            graph,
            "f2.calculation.holiday-policy.v1",
            "PLUMB.F2.TIME.CALENDAR_DEFINED",
            &format!("{CALCULATION_HOLIDAY_POLICY_CONDITION_PREFIX}{CALC}"),
            &[CALC],
        )
    }

    fn answer(
        graph: &Graph,
        question: &Id,
        answer: JsonValue,
        supersedes: Option<&Id>,
    ) -> Result<ResolutionResult, ResolutionError> {
        resolve_question(
            graph,
            &ResolutionInput {
                question_ref: question.clone(),
                answer,
                decided_by: rid(HUMAN),
                decided_at: DECIDED.parse().unwrap(),
                rationale: supersedes.map(|_| "Corrected by the owner.".to_owned()),
                supersedes: supersedes.cloned(),
            },
            &ResolutionContext::default(),
        )
    }

    fn applied(graph: &Graph, result: &ResolutionResult) -> Graph {
        apply_patch(graph, &result.proposal.patch_set)
            .unwrap()
            .graph
    }

    fn calc_of(graph: &Graph) -> Calculation {
        match &graph.node(&rid(CALC)).unwrap().payload {
            NodePayload::Calculation(c) => c.clone(),
            _ => unreachable!(),
        }
    }

    fn status_of(graph: &Graph, id: &Id) -> String {
        match &graph.node(id).unwrap().payload {
            NodePayload::Question(q) => q.status.clone(),
            NodePayload::Finding(f) => f.status.clone(),
            _ => unreachable!(),
        }
    }

    fn base() -> Graph {
        graph_with(calculation(EXPRESSION, None, Some(bindings())), Vec::new())
    }

    // ------------------------------------------------------------------ fraction

    #[test]
    fn fraction_answer_adds_one_governed_example() {
        let (g, q) = fraction_asked(&base(), &[CALC, "attr:part"]);
        assert_eq!(
            ResolutionTemplate::of("PLUMB.F2.CALC.TYPECHECK", QuestionKind::Number),
            Some(ResolutionTemplate::CalculationFraction)
        );
        let r = answer(
            &g,
            &q,
            json!({"decimal": "0.5", "unit": "working_day"}),
            None,
        )
        .unwrap();
        assert_eq!(r.template, ResolutionTemplate::CalculationFraction);
        let key = calculation_fraction_semantic_key(&rid(CALC), &rid("attr:part")).unwrap();
        assert_eq!(
            r.decision.answer,
            json!({"kind": "calculation_fraction", "calculation_ref": CALC,
                   "attribute_ref": "attr:part", "semantic_key": key,
                   "decimal": "0.5", "unit": "working_day"})
        );
        let after = applied(&g, &r);
        let examples = calc_of(&after).examples.unwrap();
        // The unrelated example is kept; exactly one governed example is added.
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0]["key"], "unrelated");
        assert_eq!(
            examples[1],
            json!({"version": 1, "key": key,
                   "inputs": [{"symbol": "part",
                               "value": {"kind": "decimal", "value": "0.5", "unit": null}}],
                   "expected_result": {"kind": "decimal", "value": "0.5", "unit": "working_day"}})
        );
        assert_eq!(
            serde_json::from_value::<CalculationExampleV1>(examples[1].clone())
                .unwrap()
                .expected_result,
            Some(ScenarioLiteral::Decimal {
                value: "0.5".into(),
                unit: Some("working_day".into())
            })
        );
        assert_eq!(status_of(&after, &q), ANSWERED);
        let finding = match &g.node(&q).unwrap().payload {
            NodePayload::Question(question) => question.finding_ref.clone(),
            _ => unreachable!(),
        };
        assert_eq!(status_of(&after, &finding), CLOSED);
        assert!(after.node(&r.decision_ref).is_some());
        // The effect artifact carries the ReplacePayload.
        assert!(matches!(
            r.patch_artifact.effect_patch,
            Some(SemanticPatch::ReplacePayload { .. })
        ));
    }

    #[test]
    fn fraction_supersession_replaces_only_the_governed_example() {
        let (g, q) = fraction_asked(&base(), &[CALC, "attr:part"]);
        let first = answer(
            &g,
            &q,
            json!({"decimal": "0.5", "unit": "working_day"}),
            None,
        )
        .unwrap();
        let after = applied(&g, &first);
        // An initial answer cannot be repeated once the example exists.
        let second = answer(
            &after,
            &q,
            json!({"decimal": "0.25", "unit": "working_day"}),
            Some(&first.decision_ref),
        )
        .unwrap();
        let after = applied(&after, &second);
        let examples = calc_of(&after).examples.unwrap();
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0]["key"], "unrelated");
        assert_eq!(examples[1]["inputs"][0]["value"]["value"], "0.25");
        assert_eq!(examples[1]["expected_result"]["value"], "0.25");
    }

    #[test]
    fn fraction_answers_outside_the_contract_are_rejected() {
        let invalid = |r: Result<ResolutionResult, ResolutionError>| {
            matches!(r, Err(ResolutionError::InvalidCalculationAnswer { .. }))
        };
        let (g, q) = fraction_asked(&base(), &[CALC, "attr:part"]);
        for (decimal, unit) in [
            ("0.50", "working_day"),
            ("0", "working_day"),
            ("-0.5", "working_day"),
            ("1.5", "working_day"),
            ("0.125", "working_day"),
            ("half", "working_day"),
            ("0.5", "working day"),
            ("0.5", "day"),
        ] {
            assert!(
                invalid(answer(
                    &g,
                    &q,
                    json!({"decimal": decimal, "unit": unit}),
                    None
                )),
                "{decimal} {unit}"
            );
        }
        let half = json!({"decimal": "0.5", "unit": "working_day"});
        // The fractional input is not a unitless Decimal Attribute.
        for attr in ["attr:flag", "attr:hours"] {
            let mut b = bindings();
            b.push(root("other", attr));
            let (g, q) = fraction_asked(
                &graph_with(calculation(EXPRESSION, None, Some(b)), Vec::new()),
                &[CALC, attr],
            );
            assert!(invalid(answer(&g, &q, half.clone(), None)), "{attr}");
        }
        // No CalculationOrigin.
        let (g, q) = fraction_asked(
            &graph_with(calculation(EXPRESSION, None, None), Vec::new()),
            &[CALC, "attr:part"],
        );
        assert!(invalid(answer(&g, &q, half.clone(), None)));
        // No root binding of the input.
        let (g, q) = fraction_asked(&base(), &[CALC, "attr:spare"]);
        assert!(invalid(answer(&g, &q, half.clone(), None)));
        // A stale binding.
        let mut b = bindings();
        b.push(root("gone", "attr:missing"));
        let (g, q) = fraction_asked(
            &graph_with(calculation(EXPRESSION, None, Some(b)), Vec::new()),
            &[CALC, "attr:part"],
        );
        assert!(invalid(answer(&g, &q, half.clone(), None)));
        // Two candidate Attributes.
        let (g, q) = fraction_asked(&base(), &[CALC, "attr:part", "attr:spare"]);
        assert!(invalid(answer(&g, &q, half.clone(), None)));
        // The governed key already exists on an initial answer.
        let (g, q) = fraction_asked(&base(), &[CALC, "attr:part"]);
        let r = answer(&g, &q, half.clone(), None).unwrap();
        let mut existing = calc_of(&applied(&g, &r));
        existing.calendar_ref = None;
        let (g2, q2) = fraction_asked(
            &graph_with(
                {
                    let mut c = calculation(EXPRESSION, None, Some(bindings()));
                    c.payload = NodePayload::Calculation(existing);
                    c
                },
                Vec::new(),
            ),
            &[CALC, "attr:part"],
        );
        assert!(matches!(
            answer(&g2, &q2, half.clone(), None),
            Err(ResolutionError::StaleResolutionQuestion { .. })
        ));
        // A Question whose Finding has another condition does not gain the fraction mapper.
        let (g, q) = asked(
            &base(),
            "f2.calculation.fraction.v1",
            "PLUMB.F2.CALC.TYPECHECK",
            &format!("{CALCULATION_INCLUSIVE_END_CONDITION_PREFIX}{CALC}"),
            &[CALC, "attr:part"],
        );
        assert!(matches!(
            answer(&g, &q, half.clone(), None),
            Err(ResolutionError::InvalidQuestionContract { .. })
        ));
        let (g, q) = asked(
            &base(),
            "f2.calculation.fraction.v1",
            "PLUMB.F2.CALC.TYPECHECK",
            "calculation_fraction_unresolved:calc:other",
            &[CALC, "attr:part"],
        );
        assert!(matches!(
            answer(&g, &q, half, None),
            Err(ResolutionError::InvalidQuestionContract { .. })
        ));
    }

    // ------------------------------------------------------------------ inclusive end

    #[test]
    fn inclusive_end_answer_sets_only_the_working_days_argument() {
        let expression =
            "working_days([begin, finish], calendar, inclusive_end) * part + inclusive_end_bonus";
        let (g, q) = inclusive_asked(&graph_with(
            calculation(expression, None, Some(bindings())),
            Vec::new(),
        ));
        let r = answer(&g, &q, json!({"inclusive_end": true}), None).unwrap();
        assert_eq!(r.template, ResolutionTemplate::CalculationInclusiveEnd);
        assert_eq!(
            r.decision.answer,
            json!({"kind": "calculation_inclusive_end", "calculation_ref": CALC, "inclusive_end": true})
        );
        assert!(r.decision.answer["inclusive_end"].is_boolean());
        let after = calc_of(&applied(&g, &r));
        assert_eq!(
            parse(&after.expression).unwrap(),
            parse("working_days([begin, finish], calendar, true) * part + inclusive_end_bonus")
                .unwrap()
        );
        // Everything except the expression is unchanged.
        let before = calc_of(&g);
        assert_eq!(
            Calculation {
                expression: before.expression.clone(),
                ..after
            },
            before
        );
    }

    #[test]
    fn inclusive_end_answers_never_guess() {
        for (expression, stale_expected) in [
            ("part * 2", false),
            (
                "working_days([begin, finish], calendar, inclusive_end) + working_days([begin, finish], calendar, inclusive_end)",
                false,
            ),
            ("working_days([begin, finish], calendar)", false),
            ("working_days([begin, finish], calendar, true)", true),
            ("working_days([begin, finish], calendar, not inclusive_end)", false),
        ] {
            let (g, q) = inclusive_asked(&graph_with(calculation(expression, None, Some(bindings())), Vec::new()));
            let result = answer(&g, &q, json!({"inclusive_end": true}), None);
            if stale_expected {
                assert!(
                    matches!(result, Err(ResolutionError::StaleResolutionQuestion { .. })),
                    "{expression}"
                );
            } else {
                assert!(
                    matches!(result, Err(ResolutionError::InvalidCalculationAnswer { .. })),
                    "{expression}"
                );
            }
        }
        let (g, q) = asked(
            &base(),
            "f2.calculation.inclusive-end.v1",
            "PLUMB.F2.CALC.TYPECHECK",
            &format!("{CALCULATION_FRACTION_CONDITION_PREFIX}{CALC}"),
            &[CALC],
        );
        assert!(matches!(
            answer(&g, &q, json!({"inclusive_end": true}), None),
            Err(ResolutionError::InvalidQuestionContract { .. })
        ));
    }

    // ------------------------------------------------------------------ holiday policy

    #[test]
    fn holiday_policy_answer_binds_the_calendar() {
        assert_eq!(
            ResolutionTemplate::of("PLUMB.F2.TIME.CALENDAR_DEFINED", QuestionKind::YesNo),
            Some(ResolutionTemplate::CalculationHolidayPolicy)
        );
        assert_eq!(
            ResolutionTemplate::of("PLUMB.F2.TIME.CALENDAR_DEFINED", QuestionKind::Calendar),
            Some(ResolutionTemplate::CalculationCalendar)
        );
        let (g, q) = holiday_asked(&base());
        let ans = json!({"holiday_consumes_working_day": false, "calendar_ref": "cal:test"});
        let r = answer(&g, &q, ans.clone(), None).unwrap();
        assert_eq!(
            r.decision.answer,
            json!({"kind": "calculation_holiday_policy", "calculation_ref": CALC,
                   "calendar_ref": "cal:test", "holiday_consumes_working_day": false})
        );
        assert_eq!(
            r.decision.answer["holiday_consumes_working_day"],
            json!(false)
        );
        assert_eq!(
            calc_of(&applied(&g, &r)).calendar_ref,
            Some(rid("cal:test"))
        );
        // The same current calendar: a governed confirmation without a semantic effect.
        let (g, q) = holiday_asked(&graph_with(
            calculation(EXPRESSION, Some("cal:test"), Some(bindings())),
            Vec::new(),
        ));
        let r = answer(&g, &q, ans.clone(), None).unwrap();
        assert_eq!(r.patch_artifact.effect_patch, None);
        let after = applied(&g, &r);
        assert_eq!(calc_of(&after), calc_of(&g));
        assert_eq!(status_of(&after, &q), ANSWERED);
        // Another current calendar is never switched silently.
        let (g, q) = holiday_asked(&graph_with(
            calculation(EXPRESSION, Some("cal:other"), Some(bindings())),
            Vec::new(),
        ));
        assert!(matches!(
            answer(&g, &q, ans, None),
            Err(ResolutionError::StaleResolutionQuestion { .. })
        ));
    }

    #[test]
    fn holiday_consuming_a_working_day_is_unsupported() {
        let (g, q) = holiday_asked(&base());
        let result = answer(
            &g,
            &q,
            json!({"holiday_consumes_working_day": true, "calendar_ref": "cal:test"}),
            None,
        );
        assert_eq!(
            result.err(),
            Some(ResolutionError::UnsupportedHolidayPolicy)
        );
        // Nothing changed: the Question is still Open and the calculation has no calendar.
        assert_eq!(status_of(&g, &q), OPEN);
        assert_eq!(calc_of(&g).calendar_ref, None);
        let names: BTreeSet<&str> = [
            "calculation_fraction",
            "calculation_inclusive_end",
            "calculation_holiday_policy",
        ]
        .into();
        assert_eq!(
            [
                ResolutionTemplate::CalculationFraction,
                ResolutionTemplate::CalculationInclusiveEnd,
                ResolutionTemplate::CalculationHolidayPolicy
            ]
            .map(ResolutionTemplate::marker_kind)
            .into_iter()
            .collect::<BTreeSet<_>>(),
            names
        );
    }
}
