//! Deterministic question-round composition (plan S3.2, Hotfix 046; compiler architecture §9).
//!
//! Accepted, Open, unassigned Questions routed to an Accepted Stakeholder and linked to a current
//! Finding are ordered per stakeholder (blocking first, then priority, then ID) and each
//! stakeholder receives at most one new round of `question_round_size` Questions per invocation.
//! A round is not a PSG node: its content-derived ID is written into each member's
//! `Question.round_ref` through one non-semantic proposal. Priority and routing come from S3.1
//! unchanged; nothing here infers, reroutes, reads files or observes time.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId};
use plumb_patch::{
    apply_patch, AcceptancePolicy, ElementPrecondition, PatchSet, Proposal, ProposalMateriality,
    SemanticPatch,
};
use plumb_psg::{
    node_element_hash, ElementStatus, Finding, FindingSeverity, Graph, Node, NodePayload, NodeType,
    Question,
};
use plumb_validation::expression_scope::canonical_attribute_owner;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

const ROUND_STAGE: StageId = StageId::S3;

/// The largest accepted `question_round_size`.
pub const MAX_QUESTION_ROUND_SIZE: usize = 1000;

/// The pilot Question lifecycle vocabulary; S3.1 creates `Open` and S3.2 never changes it.
pub const QUESTION_STATUS_OPEN: &str = "Open";
/// The non-open terminal statuses S3.2 excludes.
pub const TERMINAL_QUESTION_STATUSES: [&str; 3] = ["Answered", "Closed", "Superseded"];

/// Explicit orchestration configuration supplied by the caller; there is no default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionRoundConfig {
    pub question_round_size: usize,
}

impl QuestionRoundConfig {
    /// `question_round_size` must be within 1..=1000.
    pub fn validate(&self) -> Result<(), RoundError> {
        if (1..=MAX_QUESTION_ROUND_SIZE).contains(&self.question_round_size) {
            Ok(())
        } else {
            Err(RoundError::InvalidConfig {
                reason: format!(
                    "question_round_size {} is outside 1..={MAX_QUESTION_ROUND_SIZE}",
                    self.question_round_size
                ),
            })
        }
    }
}

/// Malformed input; no result is produced.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RoundError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("invalid round configuration: {reason}")]
    InvalidConfig { reason: String },
    #[error("question {question_ref} has status {status:?} outside the pilot vocabulary")]
    InvalidQuestionStatus { question_ref: Id, status: String },
    #[error("question {question_ref} has no canonical priority ({priority:?})")]
    InvalidQuestionPriority {
        question_ref: Id,
        priority: Option<String>,
    },
}

/// A presentation run of contiguous Questions sharing one entity anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionRoundGroup {
    pub entity_ref: Option<Id>,
    pub question_refs: Vec<Id>,
}

/// One composed round: the authoritative ordered members and their contiguous groups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionRound {
    pub id: Id,
    pub stakeholder_ref: Id,
    pub question_refs: Vec<Id>,
    pub groups: Vec<QuestionRoundGroup>,
}

/// Why a Question is stale for round composition.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum StaleReason {
    StakeholderNotAccepted { stakeholder_ref: Id },
    FindingMissing { finding_ref: Id },
    NotAFinding { finding_ref: Id },
    FindingNotAccepted { finding_ref: Id },
    FindingNotOpen { finding_ref: Id, status: String },
    FindingWaived { finding_ref: Id },
    AffectedRefNotAccepted { affected_ref: Id },
}

/// What happened to one Accepted Question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuestionRoundDisposition {
    AlreadyAssigned {
        question_ref: Id,
        round_ref: Id,
    },
    TerminalQuestion {
        question_ref: Id,
        status: String,
    },
    StaleQuestion {
        question_ref: Id,
        reason: StaleReason,
    },
    UnassignedQuestion {
        question_ref: Id,
    },
    DeferredByCapacity {
        question_ref: Id,
        stakeholder_ref: Id,
    },
    Assigned {
        question_ref: Id,
        round_ref: Id,
    },
    /// The round ID is already a PSG element ID; the round is not assigned.
    RoundIdCollision {
        question_ref: Id,
        round_ref: Id,
    },
    /// The assignment proposal does not dry-run; nothing is assigned.
    InvalidRoundProposal {
        question_ref: Id,
        reason: String,
    },
}

impl QuestionRoundDisposition {
    pub fn question_ref(&self) -> &Id {
        match self {
            QuestionRoundDisposition::AlreadyAssigned { question_ref, .. }
            | QuestionRoundDisposition::TerminalQuestion { question_ref, .. }
            | QuestionRoundDisposition::StaleQuestion { question_ref, .. }
            | QuestionRoundDisposition::UnassignedQuestion { question_ref }
            | QuestionRoundDisposition::DeferredByCapacity { question_ref, .. }
            | QuestionRoundDisposition::Assigned { question_ref, .. }
            | QuestionRoundDisposition::RoundIdCollision { question_ref, .. }
            | QuestionRoundDisposition::InvalidRoundProposal { question_ref, .. } => question_ref,
        }
    }

    fn kind_rank(&self) -> u8 {
        match self {
            QuestionRoundDisposition::AlreadyAssigned { .. } => 0,
            QuestionRoundDisposition::TerminalQuestion { .. } => 1,
            QuestionRoundDisposition::StaleQuestion { .. } => 2,
            QuestionRoundDisposition::UnassignedQuestion { .. } => 3,
            QuestionRoundDisposition::DeferredByCapacity { .. } => 4,
            QuestionRoundDisposition::Assigned { .. } => 5,
            QuestionRoundDisposition::RoundIdCollision { .. } => 6,
            QuestionRoundDisposition::InvalidRoundProposal { .. } => 7,
        }
    }
}

/// Everything one composition produced; nothing has been applied.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionRoundResult {
    /// By stakeholder, then round ID.
    pub rounds: Vec<QuestionRound>,
    pub proposal: Option<Proposal>,
    /// By Question ID, then disposition kind.
    pub dispositions: Vec<QuestionRoundDisposition>,
}

/// Parses an S3.1 priority: canonical unsigned decimal fitting `u64`.
pub fn parse_priority(text: &str) -> Option<u64> {
    let canonical = text == "0"
        || (text.starts_with(|c: char| ('1'..='9').contains(&c))
            && text.chars().all(|c| c.is_ascii_digit()));
    if !canonical {
        return None;
    }
    text.parse::<u64>().ok().filter(|v| v.to_string() == text)
}

/// `rnd:<16 hex of SHA-256(RFC 8785 {project_id, stakeholder_ref, question_refs})>`.
pub fn round_id(
    project_id: &Id,
    stakeholder_ref: &Id,
    question_refs: &[Id],
) -> Result<Id, CoreError> {
    let body = json!({
        "project_id": project_id.to_string(),
        "stakeholder_ref": stakeholder_ref.to_string(),
        "question_refs": question_refs.iter().map(Id::to_string).collect::<Vec<_>>(),
    });
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("rnd:{}", &hex[..16]).parse()
}

/// The single Entity anchor of a Finding: its Accepted Entity targets and the unique Accepted
/// owners of its Accepted Attribute targets; exactly one distinct candidate, else none.
pub fn entity_anchor(graph: &Graph, finding: &Finding) -> Option<Id> {
    let mut candidates = BTreeSet::new();
    for r in &finding.affected_refs {
        let Some(node) = graph
            .node(r)
            .filter(|n| n.status == ElementStatus::Accepted)
        else {
            continue;
        };
        match node.payload.node_type() {
            NodeType::Entity => {
                candidates.insert(r.clone());
            }
            NodeType::Attribute => {
                if let Some(owner) = canonical_attribute_owner(graph, r) {
                    candidates.insert(owner.clone());
                }
            }
            _ => {}
        }
    }
    match candidates.len() {
        1 => candidates.into_iter().next(),
        _ => None,
    }
}

/// Maximal contiguous runs of equal entity anchors, in the given order.
fn contiguous_groups(members: &[(Id, Option<Id>)]) -> Vec<QuestionRoundGroup> {
    let mut groups: Vec<QuestionRoundGroup> = Vec::new();
    for (question_ref, entity_ref) in members {
        match groups.last_mut() {
            Some(g) if &g.entity_ref == entity_ref => g.question_refs.push(question_ref.clone()),
            _ => groups.push(QuestionRoundGroup {
                entity_ref: entity_ref.clone(),
                question_refs: vec![question_ref.clone()],
            }),
        }
    }
    groups
}

/// An eligible Question with its ordering keys.
struct Eligible<'g> {
    node: &'g Node,
    question: &'g Question,
    blocking: bool,
    priority: u64,
    entity_ref: Option<Id>,
}

/// Why the linked Finding is not current, if it is not.
fn finding_staleness<'g>(graph: &'g Graph, finding_ref: &Id) -> Result<&'g Finding, StaleReason> {
    let node = graph
        .node(finding_ref)
        .ok_or_else(|| StaleReason::FindingMissing {
            finding_ref: finding_ref.clone(),
        })?;
    let NodePayload::Finding(finding) = &node.payload else {
        return Err(StaleReason::NotAFinding {
            finding_ref: finding_ref.clone(),
        });
    };
    if node.status != ElementStatus::Accepted {
        return Err(StaleReason::FindingNotAccepted {
            finding_ref: finding_ref.clone(),
        });
    }
    if finding.status != QUESTION_STATUS_OPEN {
        return Err(StaleReason::FindingNotOpen {
            finding_ref: finding_ref.clone(),
            status: finding.status.clone(),
        });
    }
    if finding.waiver_ref.is_some() {
        return Err(StaleReason::FindingWaived {
            finding_ref: finding_ref.clone(),
        });
    }
    if let Some(r) = finding.affected_refs.iter().find(|r| {
        graph
            .node(r)
            .is_none_or(|n| n.status != ElementStatus::Accepted)
    }) {
        return Err(StaleReason::AffectedRefNotAccepted {
            affected_ref: r.clone(),
        });
    }
    Ok(finding)
}

/// Composes at most one new round per stakeholder and the non-semantic proposal assigning
/// `round_ref` to its Questions.
pub fn compose_question_rounds(
    graph: &Graph,
    config: &QuestionRoundConfig,
) -> Result<QuestionRoundResult, RoundError> {
    config.validate()?;
    let mut dispositions = Vec::new();
    let mut by_stakeholder: BTreeMap<Id, Vec<Eligible<'_>>> = BTreeMap::new();
    for id in graph.node_ids_by_type(NodeType::Question) {
        let Some(node) = graph
            .node(id)
            .filter(|n| n.status == ElementStatus::Accepted)
        else {
            continue;
        };
        let NodePayload::Question(question) = &node.payload else {
            continue;
        };
        let question_ref = node.id.clone();
        if question.status != QUESTION_STATUS_OPEN {
            if TERMINAL_QUESTION_STATUSES.contains(&question.status.as_str()) {
                dispositions.push(QuestionRoundDisposition::TerminalQuestion {
                    question_ref,
                    status: question.status.clone(),
                });
                continue;
            }
            return Err(RoundError::InvalidQuestionStatus {
                question_ref,
                status: question.status.clone(),
            });
        }
        if let Some(round_ref) = &question.round_ref {
            dispositions.push(QuestionRoundDisposition::AlreadyAssigned {
                question_ref,
                round_ref: round_ref.clone(),
            });
            continue;
        }
        let Some(stakeholder) = &question.stakeholder_ref else {
            dispositions.push(QuestionRoundDisposition::UnassignedQuestion { question_ref });
            continue;
        };
        let stakeholder_accepted = graph.node(stakeholder).is_some_and(|n| {
            n.status == ElementStatus::Accepted && n.payload.node_type() == NodeType::Stakeholder
        });
        if !stakeholder_accepted {
            dispositions.push(QuestionRoundDisposition::StaleQuestion {
                question_ref,
                reason: StaleReason::StakeholderNotAccepted {
                    stakeholder_ref: stakeholder.clone(),
                },
            });
            continue;
        }
        let finding = match finding_staleness(graph, &question.finding_ref) {
            Ok(finding) => finding,
            Err(reason) => {
                dispositions.push(QuestionRoundDisposition::StaleQuestion {
                    question_ref,
                    reason,
                });
                continue;
            }
        };
        let priority = question
            .priority
            .as_deref()
            .and_then(parse_priority)
            .ok_or_else(|| RoundError::InvalidQuestionPriority {
                question_ref: question_ref.clone(),
                priority: question.priority.clone(),
            })?;
        by_stakeholder
            .entry(stakeholder.clone())
            .or_default()
            .push(Eligible {
                node,
                question,
                blocking: finding.severity == FindingSeverity::Blocker,
                priority,
                entity_ref: entity_anchor(graph, finding),
            });
    }

    let mut rounds = Vec::new();
    let mut assignments: Vec<(&Node, &Question, Id)> = Vec::new();
    for (stakeholder, mut eligible) in by_stakeholder {
        // Blocking first, then priority descending, then Question ID ascending.
        eligible.sort_by(|a, b| {
            b.blocking
                .cmp(&a.blocking)
                .then_with(|| b.priority.cmp(&a.priority))
                .then_with(|| a.node.id.cmp(&b.node.id))
        });
        let split = config.question_round_size.min(eligible.len());
        let (selected, deferred) = eligible.split_at(split);
        for e in deferred {
            dispositions.push(QuestionRoundDisposition::DeferredByCapacity {
                question_ref: e.node.id.clone(),
                stakeholder_ref: stakeholder.clone(),
            });
        }
        let question_refs: Vec<Id> = selected.iter().map(|e| e.node.id.clone()).collect();
        let id = round_id(graph.project_id(), &stakeholder, &question_refs)?;
        if graph.node(&id).is_some() || graph.edge(&id).is_some() {
            for q in &question_refs {
                dispositions.push(QuestionRoundDisposition::RoundIdCollision {
                    question_ref: q.clone(),
                    round_ref: id.clone(),
                });
            }
            continue;
        }
        let members: Vec<(Id, Option<Id>)> = selected
            .iter()
            .map(|e| (e.node.id.clone(), e.entity_ref.clone()))
            .collect();
        for e in selected {
            assignments.push((e.node, e.question, id.clone()));
        }
        rounds.push(QuestionRound {
            id,
            stakeholder_ref: stakeholder,
            question_refs,
            groups: contiguous_groups(&members),
        });
    }

    // One proposal: round_ref updates in Question ID order under CAS preconditions.
    assignments.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    let mut patches = Vec::with_capacity(assignments.len());
    for (node, question, round_ref) in &assignments {
        let mut payload = (*question).clone();
        payload.round_ref = Some(round_ref.clone());
        patches.push(SemanticPatch::ReplacePayload {
            target: ElementPrecondition {
                id: node.id.clone(),
                expected_hash: node_element_hash(node)?,
            },
            payload: NodePayload::Question(payload),
        });
    }
    let mut proposal = None;
    if !patches.is_empty() {
        let patch = if patches.len() == 1 {
            patches.remove(0)
        } else {
            SemanticPatch::Compound { patches }
        };
        let built = Proposal::new(
            ROUND_STAGE,
            PatchSet {
                base_semantic_hash: graph.semantic_hash()?,
                patch,
            },
            Vec::new(),
            Vec::new(),
            ProposalMateriality::NonSemantic,
            AcceptancePolicy::AutoNonSemantic,
            None,
        )
        .map_err(|e| e.to_string())
        .and_then(|p| {
            apply_patch(graph, &p.patch_set)
                .map(|_| p)
                .map_err(|e| format!("proposal does not apply: {e}"))
        });
        match built {
            Ok(p) => {
                for (node, _, round_ref) in &assignments {
                    dispositions.push(QuestionRoundDisposition::Assigned {
                        question_ref: node.id.clone(),
                        round_ref: round_ref.clone(),
                    });
                }
                proposal = Some(p);
            }
            Err(reason) => {
                for (node, _, _) in &assignments {
                    dispositions.push(QuestionRoundDisposition::InvalidRoundProposal {
                        question_ref: node.id.clone(),
                        reason: reason.clone(),
                    });
                }
                rounds.clear();
            }
        }
    }

    rounds.sort_by(|a, b| {
        a.stakeholder_ref
            .cmp(&b.stakeholder_ref)
            .then_with(|| a.id.cmp(&b.id))
    });
    dispositions.sort_by(|a, b| {
        a.question_ref()
            .cmp(b.question_ref())
            .then_with(|| a.kind_rank().cmp(&b.kind_rank()))
    });
    Ok(QuestionRoundResult {
        rounds,
        proposal,
        dispositions,
    })
}
