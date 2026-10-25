//! Optional S1 intent proposals (plan S1.1): Goal, Need, Concern and Constraint nodes proposed
//! by a validated classification inference for one requirement candidate. Nothing is created
//! from wording alone, no field is defaulted and no relation between intent nodes is guessed.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use plumb_psg::{
    Concern, Constraint, ConstraintCategory, ConstraintStrength, ElementStatus, Goal, Graph, Need,
    NodePayload,
};
use serde::Deserialize;

use crate::requirements::{RequirementClassificationContext, RequirementCompilationIssue};

/// One intent entry of the classification output, tagged by `intent_kind`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "intent_kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum IntentOutput {
    Goal {
        candidate_ref: Id,
    },
    Need {
        candidate_ref: Id,
        stakeholder_refs: Vec<Id>,
    },
    Concern {
        candidate_ref: Id,
        name: String,
        description: String,
    },
    Constraint {
        candidate_ref: Id,
        constraint_category: ConstraintCategory,
        strength: ConstraintStrength,
    },
}

impl IntentOutput {
    fn candidate_ref(&self) -> &Id {
        match self {
            IntentOutput::Goal { candidate_ref }
            | IntentOutput::Need { candidate_ref, .. }
            | IntentOutput::Concern { candidate_ref, .. }
            | IntentOutput::Constraint { candidate_ref, .. } => candidate_ref,
        }
    }

    /// The intent kind wire string, which is also the proposed node type.
    fn kind(&self) -> &'static str {
        match self {
            IntentOutput::Goal { .. } => "goal",
            IntentOutput::Need { .. } => "need",
            IntentOutput::Concern { .. } => "concern",
            IntentOutput::Constraint { .. } => "constraint",
        }
    }
}

/// Non-empty, no leading/trailing whitespace and no control characters.
fn grounded_text(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

/// Validates every intent against the context and graph. Intents are optional proposals, so an
/// invalid one is dropped with an `intent_dropped` issue instead of rejecting the whole output
/// (goal-approach deviation from plan S1.1 item 12, after the first real-model run): a
/// repeated (candidate_ref, intent_kind) pair drops every entry of the pair, so no entry is
/// preferred. Returns the valid intents sorted by (candidate_ref, intent_kind) and the issues.
pub(crate) fn validate_intents(
    graph: &Graph,
    context: &RequirementClassificationContext,
    mut intents: Vec<IntentOutput>,
) -> (Vec<IntentOutput>, Vec<RequirementCompilationIssue>) {
    intents.sort_by(|a, b| (a.candidate_ref(), a.kind()).cmp(&(b.candidate_ref(), b.kind())));
    let candidates: BTreeSet<&Id> = context
        .candidates
        .iter()
        .map(|c| &c.candidate_ref)
        .collect();
    let stakeholders: BTreeSet<&Id> = context
        .stakeholders
        .iter()
        .map(|s| &s.stakeholder_ref)
        .collect();
    let mut pairs: BTreeMap<(Id, &'static str), usize> = BTreeMap::new();
    for intent in &intents {
        *pairs
            .entry((intent.candidate_ref().clone(), intent.kind()))
            .or_default() += 1;
    }
    let mut valid = Vec::new();
    let mut issues = Vec::new();
    let mut drop = |intent: &IntentOutput, reason: &str| {
        issues.push(RequirementCompilationIssue::IntentDropped {
            candidate_ref: intent.candidate_ref().clone(),
            intent_kind: intent.kind().to_owned(),
            reason: reason.to_owned(),
        });
    };
    let mut reported_duplicates = BTreeSet::new();
    for intent in intents {
        let key = (intent.candidate_ref().clone(), intent.kind());
        if pairs[&key] > 1 {
            if reported_duplicates.insert(key) {
                drop(&intent, "duplicate_intent");
            }
            continue;
        }
        if !candidates.contains(intent.candidate_ref()) {
            drop(&intent, "candidate_not_in_context");
            continue;
        }
        let reason = match &intent {
            IntentOutput::Need {
                stakeholder_refs, ..
            } => {
                let unique: BTreeSet<&Id> = stakeholder_refs.iter().collect();
                if stakeholder_refs.is_empty() || unique.len() != stakeholder_refs.len() {
                    Some("need_stakeholders_empty_or_repeated")
                } else if stakeholder_refs.iter().any(|reference| {
                    !stakeholders.contains(reference)
                        || !graph.node(reference).is_some_and(|node| {
                            node.status == ElementStatus::Accepted
                                && matches!(node.payload, NodePayload::Stakeholder(_))
                        })
                }) {
                    Some("stakeholder_not_supplied")
                } else {
                    None
                }
            }
            IntentOutput::Concern {
                name, description, ..
            } => (!grounded_text(name) || !grounded_text(description))
                .then_some("concern_text_not_grounded"),
            IntentOutput::Goal { .. } | IntentOutput::Constraint { .. } => None,
        };
        match reason {
            Some(reason) => drop(&intent, reason),
            None => valid.push(intent),
        }
    }
    (valid, issues)
}

/// The (node type, ID prefix, payload) of every validated intent of `candidate_ref`. Goal,
/// Need and Constraint statements are the deterministic clean statement, never model text.
pub(crate) fn intent_payloads(
    intents: &[IntentOutput],
    candidate_ref: &Id,
    statement: &str,
) -> Vec<(&'static str, &'static str, NodePayload)> {
    intents
        .iter()
        .filter(|intent| intent.candidate_ref() == candidate_ref)
        .map(|intent| match intent {
            IntentOutput::Goal { .. } => (
                "goal",
                "goal",
                NodePayload::Goal(Goal {
                    statement: statement.to_owned(),
                    success_measures: None,
                    priority: None,
                }),
            ),
            IntentOutput::Need {
                stakeholder_refs, ..
            } => {
                let mut refs = stakeholder_refs.clone();
                refs.sort();
                (
                    "need",
                    "need",
                    NodePayload::Need(Need {
                        statement: statement.to_owned(),
                        stakeholder_refs: refs,
                        goal_refs: None,
                        context: None,
                    }),
                )
            }
            IntentOutput::Concern {
                name, description, ..
            } => (
                "concern",
                "concern",
                NodePayload::Concern(Concern {
                    name: name.clone(),
                    description: description.clone(),
                }),
            ),
            IntentOutput::Constraint {
                constraint_category,
                strength,
                ..
            } => (
                "constraint",
                "constraint",
                NodePayload::Constraint(Constraint {
                    statement: statement.to_owned(),
                    constraint_category: *constraint_category,
                    strength: *strength,
                }),
            ),
        })
        .collect()
}
