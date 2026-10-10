//! Deterministic Question generation and routing (plan S3.1, Hotfix 045; compiler architecture
//! §9).
//!
//! Current GeneratedFinding material is validated, matched against the closed
//! [`crate::question_templates`] registry, prioritized by severity weight times the finding's
//! impact blast radius (plumb-patch §22 closure), routed through the supplied stakeholder
//! configuration to an Accepted Stakeholder or the analyst fallback, and identified by template
//! and canonical slots. Missing question-driving Findings and new Questions become Accepted
//! governance nodes in at most one non-semantic proposal. Nothing here evaluates gates, infers,
//! reads files, observes time or rewrites persisted lifecycle state.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{to_canonical_json, CoreError, Hash, HashKind, Id, StageId, Timestamp};
use plumb_patch::{
    apply_patch, impact_reachable_nodes, AcceptancePolicy, ImpactError, PatchSet, Proposal,
    ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AuditMeta, ElementStatus, FindingSeverity, Graph, Node, NodePayload, NodeType, Question,
};
use plumb_validation::{finding_id, finding_key, GeneratedFinding};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};
use thiserror::Error;

use crate::question_templates::{
    match_template, Choices, ConditionMatch, Expansion, QuestionTemplate, UnmappedReason,
};

const QUESTION_STAGE: StageId = StageId::S3;

/// The routing key every template falls back to.
pub const DEFAULT_ROUTING_KEY: &str = "default";

/// The routing-directory role that receives unroutable questions.
pub const ANALYST_ROLE: &str = "analyst";

// ============================================================================ routing config

/// One person of the routing directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingStakeholder {
    pub id: Id,
    pub name: String,
    pub roles: Vec<String>,
}

/// The stakeholder routing configuration (`stakeholders.yaml` shape). Each routing list is an
/// ordered preference list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StakeholderRoutingConfig {
    pub stakeholders: Vec<RoutingStakeholder>,
    pub routing: BTreeMap<String, Vec<Id>>,
}

fn clean(value: &str) -> bool {
    !value.trim().is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

impl StakeholderRoutingConfig {
    /// Parses and validates supplied YAML text. No file is read.
    pub fn from_yaml(text: &str) -> Result<StakeholderRoutingConfig, QuestionError> {
        let config: StakeholderRoutingConfig =
            serde_yaml::from_str(text).map_err(|e| QuestionError::InvalidRoutingConfig {
                reason: e.to_string(),
            })?;
        config.validate()?;
        Ok(config)
    }

    /// Unique stakeholder IDs, clean names and roles, clean routing keys and route targets that
    /// exist in the directory without duplicates.
    pub fn validate(&self) -> Result<(), QuestionError> {
        let invalid = |reason: String| QuestionError::InvalidRoutingConfig { reason };
        let mut ids = BTreeSet::new();
        for s in &self.stakeholders {
            if !ids.insert(&s.id) {
                return Err(invalid(format!("duplicate stakeholder {}", s.id)));
            }
            if !clean(&s.name) {
                return Err(invalid(format!("stakeholder {} has an invalid name", s.id)));
            }
            if s.roles.is_empty() {
                return Err(invalid(format!("stakeholder {} has no roles", s.id)));
            }
            let mut roles = BTreeSet::new();
            for role in &s.roles {
                if !clean(role) || !roles.insert(role) {
                    return Err(invalid(format!(
                        "stakeholder {} has an invalid or duplicate role {role:?}",
                        s.id
                    )));
                }
            }
        }
        for (key, targets) in &self.routing {
            if !clean(key) {
                return Err(invalid(format!("invalid routing key {key:?}")));
            }
            let mut seen = BTreeSet::new();
            for target in targets {
                if !ids.contains(target) {
                    return Err(invalid(format!(
                        "routing {key} names {target}, which is not in the directory"
                    )));
                }
                if !seen.insert(target) {
                    return Err(invalid(format!("routing {key} repeats {target}")));
                }
            }
        }
        Ok(())
    }
}

// ============================================================================ input and output

/// Current GeneratedFinding material and the parsed routing configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionGenerationInput {
    pub findings: Vec<GeneratedFinding>,
    pub routing: StakeholderRoutingConfig,
}

/// Who materializes the governance nodes, and when (caller supplied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// Malformed input; no result is produced.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QuestionError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Impact(#[from] ImpactError),
    #[error("invalid routing configuration: {reason}")]
    InvalidRoutingConfig { reason: String },
    #[error("finding {0} is supplied more than once")]
    DuplicateFinding(Id),
    #[error("invalid finding {finding_ref}: {reason}")]
    InvalidFinding { finding_ref: Id, reason: String },
    #[error("priority of finding {0} overflows")]
    PriorityOverflow(Id),
    #[error("invalid audit: {reason}")]
    InvalidAudit { reason: String },
}

/// How a Question's stakeholder was chosen.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "route", rename_all = "snake_case", deny_unknown_fields)]
pub enum RouteDisposition {
    /// The first Accepted Stakeholder of the routing list of `routing_key`.
    Configured { routing_key: String },
    /// The smallest Accepted analyst of the routing directory.
    AnalystFallback,
    /// No configured or analyst Stakeholder is Accepted; the Question is unassigned.
    AnalystRoleUnassigned,
}

/// Whether a generated Question is new or already persisted identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionMaterialization {
    New,
    Existing,
}

/// One deterministic Question.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GeneratedQuestion {
    pub question_ref: Id,
    pub template_id: String,
    pub finding_ref: Id,
    /// The canonical slot object the ID is derived from.
    pub slots: JsonValue,
    pub payload: Question,
    pub priority: u64,
    pub blast_radius: u64,
    pub route: RouteDisposition,
    pub materialization: QuestionMaterialization,
}

/// What happened to a finding or candidate that is not a problem.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuestionDisposition {
    /// The finding carries a waiver; no Question.
    WaivedFinding { finding_ref: Id },
    /// Affected targets required by the template are no longer Accepted; no Question.
    StaleFinding {
        finding_ref: Id,
        stale_refs: Vec<Id>,
    },
    /// The identical Question is already persisted; no patch.
    ExistingQuestion { question_ref: Id },
    /// Another candidate with the same template and canonical slots was kept.
    SuppressedDuplicate {
        question_ref: Id,
        template_id: String,
        finding_ref: Id,
    },
    /// The Finding node is already persisted with the exact payload.
    ExistingFinding { finding_ref: Id },
}

/// A problem preventing a Question or the proposal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuestionIssue {
    /// A choice template has no Accepted choice.
    NoAnswerChoices {
        finding_ref: Id,
        template_id: String,
        subject_ref: Id,
    },
    /// The condition suffix of a prefix template is not an ID.
    InvalidCandidateRef {
        finding_ref: Id,
        template_id: String,
        suffix: String,
    },
    /// A different element is persisted at the finding ID.
    ExistingFindingConflict { finding_ref: Id, reason: String },
    /// A different element is persisted at the Question ID.
    ExistingQuestionConflict { question_ref: Id, reason: String },
    /// The proposal does not dry-run.
    InvalidQuestionProposal { reason: String },
}

/// A finding no template maps.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnmappedFinding {
    pub finding_ref: Id,
    pub code: String,
    pub semantic_condition_key: String,
    pub reason: UnmappedReason,
}

/// Everything one generation produced; nothing has been applied or persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionGenerationResult {
    /// By descending priority, then Question ID.
    pub questions: Vec<GeneratedQuestion>,
    pub proposal: Option<Proposal>,
    pub dispositions: Vec<QuestionDisposition>,
    pub unmapped: Vec<UnmappedFinding>,
    pub issues: Vec<QuestionIssue>,
}

// ============================================================================ priority and identity

/// The frozen integer severity weights.
pub fn severity_weight(severity: FindingSeverity) -> u64 {
    match severity {
        FindingSeverity::Blocker => 4,
        FindingSeverity::Error => 3,
        FindingSeverity::Warn => 2,
        FindingSeverity::Info => 1,
    }
}

/// `q:<16 hex of SHA-256(RFC 8785 {project_id, template_id, slots})>`.
pub fn question_id(project_id: &Id, template_id: &str, slots: &JsonValue) -> Result<Id, CoreError> {
    let body = serde_json::json!({
        "project_id": project_id.to_string(),
        "template_id": template_id,
        "slots": slots,
    });
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("q:{}", &hex[..16]).parse()
}

/// One Question before suppression and reconciliation.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionCandidate {
    pub question_ref: Id,
    pub template_id: String,
    pub finding_ref: Id,
    pub slots: JsonValue,
    pub payload: Question,
    pub priority: u64,
    pub blast_radius: u64,
    pub route: RouteDisposition,
}

impl QuestionCandidate {
    /// The exact suppression key: template ID and RFC 8785 canonical slots.
    pub fn suppression_key(&self) -> Result<(String, Vec<u8>), CoreError> {
        Ok((self.template_id.clone(), to_canonical_json(&self.slots)?))
    }
}

/// Keeps one candidate per suppression key, the first in canonical (key, finding, question)
/// order, and records the rest as suppressed. No text similarity is used.
pub fn suppress_duplicates(
    mut candidates: Vec<QuestionCandidate>,
) -> Result<(Vec<QuestionCandidate>, Vec<QuestionDisposition>), CoreError> {
    let mut keyed = Vec::with_capacity(candidates.len());
    for c in candidates.drain(..) {
        keyed.push((c.suppression_key()?, c));
    }
    keyed.sort_by(|(ka, a), (kb, b)| {
        ka.cmp(kb)
            .then_with(|| a.finding_ref.cmp(&b.finding_ref))
            .then_with(|| a.question_ref.cmp(&b.question_ref))
    });
    let mut kept: Vec<QuestionCandidate> = Vec::new();
    let mut suppressed = Vec::new();
    let mut last: Option<(String, Vec<u8>)> = None;
    for (key, c) in keyed {
        if last.as_ref() == Some(&key) {
            suppressed.push(QuestionDisposition::SuppressedDuplicate {
                question_ref: c.question_ref,
                template_id: c.template_id,
                finding_ref: c.finding_ref,
            });
        } else {
            last = Some(key);
            kept.push(c);
        }
    }
    Ok((kept, suppressed))
}

// ============================================================================ routing

fn accepted_stakeholder(graph: &Graph, id: &Id) -> bool {
    graph.node(id).is_some_and(|n| {
        n.status == ElementStatus::Accepted && n.payload.node_type() == NodeType::Stakeholder
    })
}

/// Routes `routing_key` (else `default`) to the first Accepted Stakeholder in preference order,
/// else the smallest Accepted analyst, else nobody. No person is invented or matched by name.
pub fn route_question(
    graph: &Graph,
    config: &StakeholderRoutingConfig,
    routing_key: &str,
) -> (Option<Id>, RouteDisposition) {
    let list = config
        .routing
        .get_key_value(routing_key)
        .or_else(|| config.routing.get_key_value(DEFAULT_ROUTING_KEY));
    if let Some((key, candidates)) = list {
        if let Some(id) = candidates.iter().find(|id| accepted_stakeholder(graph, id)) {
            return (
                Some(id.clone()),
                RouteDisposition::Configured {
                    routing_key: key.clone(),
                },
            );
        }
    }
    let analyst = config
        .stakeholders
        .iter()
        .filter(|s| s.roles.iter().any(|r| r == ANALYST_ROLE))
        .map(|s| &s.id)
        .filter(|id| accepted_stakeholder(graph, id))
        .min();
    match analyst {
        Some(id) => (Some(id.clone()), RouteDisposition::AnalystFallback),
        None => (None, RouteDisposition::AnalystRoleUnassigned),
    }
}

// ============================================================================ generation

/// Structural validity of current finding material (waiver and currency are separate).
fn validate_finding(graph: &Graph, finding: &GeneratedFinding) -> Result<(), QuestionError> {
    let invalid = |reason: &str| QuestionError::InvalidFinding {
        finding_ref: finding.id.clone(),
        reason: reason.to_owned(),
    };
    let p = &finding.payload;
    if finding.key.kind() != HashKind::Generic {
        return Err(invalid("finding key is not a Generic hash"));
    }
    let material = |e: plumb_validation::EvaluationError| invalid(&e.to_string());
    if finding_id(&finding.key).map_err(material)? != finding.id {
        return Err(invalid("finding ID does not match its key"));
    }
    if p.affected_refs.is_empty() {
        return Err(invalid("finding has no affected refs"));
    }
    if p.affected_refs.windows(2).any(|w| w[0] >= w[1]) {
        return Err(invalid("affected refs are not strictly sorted"));
    }
    if finding_key(&p.code, &p.affected_refs, &finding.semantic_condition_key).map_err(material)?
        != finding.key
    {
        return Err(invalid("finding key does not match its content"));
    }
    if let Some(missing) = p.affected_refs.iter().find(|r| graph.node(r).is_none()) {
        return Err(invalid(&format!("affected ref {missing} does not exist")));
    }
    Ok(())
}

fn finding_node(finding: &GeneratedFinding, audit: &AuditMeta) -> Node {
    Node {
        id: finding.id.clone(),
        revision: 1,
        status: ElementStatus::Accepted,
        payload: NodePayload::Finding(finding.payload.clone()),
        evidence: Vec::new(),
        derivations: Vec::new(),
        standards: Vec::new(),
        tags: BTreeSet::new(),
        extensions: BTreeMap::new(),
        audit: audit.clone(),
    }
}

fn question_node(id: &Id, payload: &Question, audit: &AuditMeta) -> Node {
    Node {
        id: id.clone(),
        revision: 1,
        status: ElementStatus::Accepted,
        payload: NodePayload::Question(payload.clone()),
        evidence: Vec::new(),
        derivations: Vec::new(),
        standards: Vec::new(),
        tags: BTreeSet::new(),
        extensions: BTreeMap::new(),
        audit: audit.clone(),
    }
}

/// Whether two Questions agree on every generated field; `status` and `round_ref` are
/// lifecycle-managed and ignored.
fn same_generated_fields(a: &Question, b: &Question) -> bool {
    a.finding_ref == b.finding_ref
        && a.question_kind == b.question_kind
        && a.prompt == b.prompt
        && a.answer_schema == b.answer_schema
        && a.stakeholder_ref == b.stakeholder_ref
        && a.priority == b.priority
        && a.context_refs == b.context_refs
}

/// The persisted Question context (Hotfix 047): the exact subject first, then the other affected
/// refs in ascending ID order.
pub fn subject_first_context(subject: &Id, affected_refs: &[Id]) -> Vec<Id> {
    std::iter::once(subject.clone())
        .chain(affected_refs.iter().filter(|r| *r != subject).cloned())
        .collect()
}

/// The candidates of one current, mapped finding.
fn expand(
    graph: &Graph,
    finding: &GeneratedFinding,
    template: &QuestionTemplate,
    routing: &StakeholderRoutingConfig,
    candidates: &mut Vec<QuestionCandidate>,
    dispositions: &mut Vec<QuestionDisposition>,
    issues: &mut Vec<QuestionIssue>,
) -> Result<(), QuestionError> {
    let p = &finding.payload;
    let affected = &p.affected_refs;
    // Currency: every affected target the template needs must still be Accepted.
    let stale: Vec<Id> = affected
        .iter()
        .filter(|r| {
            graph
                .node(r)
                .is_none_or(|n| n.status != ElementStatus::Accepted)
        })
        .cloned()
        .collect();
    if !stale.is_empty() {
        dispositions.push(QuestionDisposition::StaleFinding {
            finding_ref: finding.id.clone(),
            stale_refs: stale,
        });
        return Ok(());
    }
    let subjects: Vec<Id> = match template.expansion {
        Expansion::Candidate => {
            let prefix = match template.condition {
                ConditionMatch::Prefix(prefix) => prefix,
                ConditionMatch::Exact(_) => {
                    unreachable!("candidate templates match by prefix")
                }
            };
            let suffix = &finding.semantic_condition_key[prefix.len()..];
            match suffix.parse::<Id>() {
                Ok(candidate) => vec![candidate],
                Err(_) => {
                    issues.push(QuestionIssue::InvalidCandidateRef {
                        finding_ref: finding.id.clone(),
                        template_id: template.template_id.to_owned(),
                        suffix: suffix.to_owned(),
                    });
                    return Ok(());
                }
            }
        }
        Expansion::PerTarget(node_type) => {
            if let Some(wrong) = affected.iter().find(|r| {
                graph
                    .node(r)
                    .is_some_and(|n| n.payload.node_type() != node_type)
            }) {
                return Err(QuestionError::InvalidFinding {
                    finding_ref: finding.id.clone(),
                    reason: format!(
                        "affected ref {wrong} is not a {} as {} requires",
                        node_type.as_str(),
                        template.template_id
                    ),
                });
            }
            affected.clone()
        }
    };
    let choices = Choices::of(graph, template.choices);
    if choices.is_unanswerable() {
        for subject in subjects {
            issues.push(QuestionIssue::NoAnswerChoices {
                finding_ref: finding.id.clone(),
                template_id: template.template_id.to_owned(),
                subject_ref: subject,
            });
        }
        return Ok(());
    }
    // Finding impact: the same blast radius for every expanded Question.
    let seeds: BTreeSet<Id> = affected.iter().cloned().collect();
    let blast_radius = impact_reachable_nodes(graph, &seeds)?.len() as u64;
    let priority = severity_weight(p.severity)
        .checked_mul(blast_radius)
        .ok_or_else(|| QuestionError::PriorityOverflow(finding.id.clone()))?;
    let (stakeholder_ref, route) = route_question(graph, routing, template.routing_key);
    let answer_schema = template.answer_schema(&choices);
    for subject in subjects {
        let mut slots: Map<String, JsonValue> =
            template.base_slots(&finding.id, affected, &subject, &choices);
        slots.insert("priority".into(), JsonValue::String(priority.to_string()));
        slots.insert(
            "stakeholder_ref".into(),
            stakeholder_ref
                .as_ref()
                .map_or(JsonValue::Null, |s| JsonValue::String(s.to_string())),
        );
        let slots = JsonValue::Object(slots);
        let question_ref = question_id(graph.project_id(), template.template_id, &slots)?;
        candidates.push(QuestionCandidate {
            question_ref,
            template_id: template.template_id.to_owned(),
            finding_ref: finding.id.clone(),
            slots,
            payload: Question {
                finding_ref: finding.id.clone(),
                question_kind: template.kind,
                prompt: template.prompt(&subject),
                status: "Open".to_owned(),
                answer_schema: Some(answer_schema.clone()),
                stakeholder_ref: stakeholder_ref.clone(),
                priority: Some(priority.to_string()),
                round_ref: None,
                context_refs: Some(subject_first_context(&subject, affected)),
            },
            priority,
            blast_radius,
            route: route.clone(),
        });
    }
    Ok(())
}

/// Generates the deterministic Questions of the current findings and at most one non-semantic
/// proposal materializing the missing Findings and the new Questions.
pub fn generate_questions(
    graph: &Graph,
    input: &QuestionGenerationInput,
    audit: &QuestionAudit,
) -> Result<QuestionGenerationResult, QuestionError> {
    input.routing.validate()?;
    let meta =
        AuditMeta::new(audit.created_by.clone(), audit.created_at, None, None).map_err(|e| {
            QuestionError::InvalidAudit {
                reason: e.to_string(),
            }
        })?;
    let mut findings: Vec<&GeneratedFinding> = input.findings.iter().collect();
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(w) = findings.windows(2).find(|w| w[0].id == w[1].id) {
        return Err(QuestionError::DuplicateFinding(w[0].id.clone()));
    }

    let mut candidates = Vec::new();
    let mut dispositions = Vec::new();
    let mut unmapped = Vec::new();
    let mut issues = Vec::new();
    for finding in &findings {
        validate_finding(graph, finding)?;
        if finding.payload.waiver_ref.is_some() {
            dispositions.push(QuestionDisposition::WaivedFinding {
                finding_ref: finding.id.clone(),
            });
            continue;
        }
        if finding.payload.status != "Open" {
            return Err(QuestionError::InvalidFinding {
                finding_ref: finding.id.clone(),
                reason: format!("status {:?} is not Open", finding.payload.status),
            });
        }
        match match_template(&finding.payload.code, &finding.semantic_condition_key) {
            Err(reason) => unmapped.push(UnmappedFinding {
                finding_ref: finding.id.clone(),
                code: finding.payload.code.clone(),
                semantic_condition_key: finding.semantic_condition_key.clone(),
                reason,
            }),
            Ok(template) => expand(
                graph,
                finding,
                template,
                &input.routing,
                &mut candidates,
                &mut dispositions,
                &mut issues,
            )?,
        }
    }
    let (candidates, suppressed) = suppress_duplicates(candidates)?;
    dispositions.extend(suppressed);

    // Finding reconciliation: materialize missing question-driving Findings, reuse exact ones.
    let by_id: BTreeMap<&Id, &GeneratedFinding> = findings.iter().map(|f| (&f.id, *f)).collect();
    let driving: BTreeSet<&Id> = candidates.iter().map(|c| &c.finding_ref).collect();
    let mut new_findings = Vec::new();
    let mut conflicted: BTreeSet<Id> = BTreeSet::new();
    for finding_ref in driving {
        let finding = by_id[finding_ref];
        match graph.node(finding_ref) {
            None => new_findings.push(finding_node(finding, &meta)),
            Some(node) => {
                let reason = match &node.payload {
                    NodePayload::Finding(_) if node.status != ElementStatus::Accepted => {
                        Some(format!("persisted Finding is {:?}", node.status))
                    }
                    NodePayload::Finding(existing) if *existing != finding.payload => {
                        Some("persisted Finding payload differs from the current finding".into())
                    }
                    NodePayload::Finding(_) => None,
                    other => Some(format!(
                        "{} is persisted at the finding ID",
                        other.node_type().as_str()
                    )),
                };
                match reason {
                    None => dispositions.push(QuestionDisposition::ExistingFinding {
                        finding_ref: finding_ref.clone(),
                    }),
                    Some(reason) => {
                        issues.push(QuestionIssue::ExistingFindingConflict {
                            finding_ref: finding_ref.clone(),
                            reason,
                        });
                        conflicted.insert(finding_ref.clone());
                    }
                }
            }
        }
    }

    // Question reconciliation.
    let mut questions = Vec::new();
    let mut new_questions = Vec::new();
    for c in candidates {
        if conflicted.contains(&c.finding_ref) {
            continue;
        }
        let materialization = match graph.node(&c.question_ref) {
            None => {
                new_questions.push(question_node(&c.question_ref, &c.payload, &meta));
                QuestionMaterialization::New
            }
            Some(node) => {
                let reason = match &node.payload {
                    NodePayload::Question(_) if node.status != ElementStatus::Accepted => {
                        Some(format!("persisted Question is {:?}", node.status))
                    }
                    NodePayload::Question(existing)
                        if !same_generated_fields(existing, &c.payload) =>
                    {
                        Some("persisted Question differs in generated fields".into())
                    }
                    NodePayload::Question(_) => None,
                    other => Some(format!(
                        "{} is persisted at the Question ID",
                        other.node_type().as_str()
                    )),
                };
                if let Some(reason) = reason {
                    issues.push(QuestionIssue::ExistingQuestionConflict {
                        question_ref: c.question_ref.clone(),
                        reason,
                    });
                    continue;
                }
                dispositions.push(QuestionDisposition::ExistingQuestion {
                    question_ref: c.question_ref.clone(),
                });
                QuestionMaterialization::Existing
            }
        };
        questions.push(GeneratedQuestion {
            question_ref: c.question_ref,
            template_id: c.template_id,
            finding_ref: c.finding_ref,
            slots: c.slots,
            payload: c.payload,
            priority: c.priority,
            blast_radius: c.blast_radius,
            route: c.route,
            materialization,
        });
    }
    // Findings only for surviving Questions.
    let surviving: BTreeSet<&Id> = questions.iter().map(|q| &q.finding_ref).collect();
    new_findings.retain(|n| surviving.contains(&n.id));

    // One atomic non-semantic proposal: Findings by ID, then Questions by ID.
    new_findings.sort_by(|a, b| a.id.cmp(&b.id));
    new_questions.sort_by(|a, b| a.id.cmp(&b.id));
    let mut patches: Vec<SemanticPatch> = new_findings
        .into_iter()
        .chain(new_questions)
        .map(|node| SemanticPatch::AddNode { node })
        .collect();
    let proposal = if patches.is_empty() {
        None
    } else {
        let patch = if patches.len() == 1 {
            patches.remove(0)
        } else {
            SemanticPatch::Compound { patches }
        };
        let built = Proposal::new(
            QUESTION_STAGE,
            PatchSet {
                base_semantic_hash: graph.semantic_hash()?,
                patch,
            },
            Vec::new(),
            Vec::new(),
            ProposalMateriality::NonSemantic,
            AcceptancePolicy::AutoNonSemantic,
            None,
        );
        match built {
            Err(e) => {
                issues.push(QuestionIssue::InvalidQuestionProposal {
                    reason: e.to_string(),
                });
                None
            }
            Ok(proposal) => match apply_patch(graph, &proposal.patch_set) {
                Err(e) => {
                    issues.push(QuestionIssue::InvalidQuestionProposal {
                        reason: format!("proposal does not apply: {e}"),
                    });
                    None
                }
                Ok(_) => Some(proposal),
            },
        }
    };

    questions.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.question_ref.cmp(&b.question_ref))
    });
    dispositions.sort();
    dispositions.dedup();
    unmapped.sort();
    issues.sort();
    issues.dedup();
    Ok(QuestionGenerationResult {
        questions,
        proposal,
        dispositions,
        unmapped,
        issues,
    })
}
