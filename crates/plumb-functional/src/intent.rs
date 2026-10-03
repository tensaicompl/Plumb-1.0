//! Optional S1 intent proposals (plan S1.1): Goal, Need, Concern and Constraint nodes proposed
//! by a validated classification inference for one requirement candidate. Nothing is created
//! from wording alone, no field is defaulted and no relation between intent nodes is guessed.

use std::collections::BTreeSet;

use plumb_core::Id;
use plumb_psg::{
    Concern, Constraint, ConstraintCategory, ConstraintStrength, ElementStatus, Goal, Graph, Need,
    NodePayload,
};
use serde::Deserialize;

use crate::requirements::{RequirementClassificationContext, RequirementCompilationError};

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

/// Validates every intent against the context and graph and returns them sorted by
/// (candidate_ref, intent_kind); one invalid intent rejects the whole output.
pub(crate) fn validate_intents(
    graph: &Graph,
    context: &RequirementClassificationContext,
    mut intents: Vec<IntentOutput>,
) -> Result<Vec<IntentOutput>, RequirementCompilationError> {
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
    let mut seen = BTreeSet::new();
    for intent in &intents {
        let candidate_ref = intent.candidate_ref();
        if !candidates.contains(candidate_ref) {
            return Err(RequirementCompilationError::InvalidIntentReference {
                candidate_ref: candidate_ref.clone(),
                reference: candidate_ref.clone(),
            });
        }
        if !seen.insert((candidate_ref, intent.kind())) {
            return Err(RequirementCompilationError::DuplicateIntent {
                candidate_ref: candidate_ref.clone(),
                intent_kind: intent.kind(),
            });
        }
        match intent {
            IntentOutput::Need {
                stakeholder_refs, ..
            } => {
                let unique: BTreeSet<&Id> = stakeholder_refs.iter().collect();
                if stakeholder_refs.is_empty() || unique.len() != stakeholder_refs.len() {
                    return Err(RequirementCompilationError::InvalidIntent {
                        candidate_ref: candidate_ref.clone(),
                        reason: "need stakeholder_refs must be non-empty and unique".to_owned(),
                    });
                }
                for reference in stakeholder_refs {
                    let accepted = graph.node(reference).is_some_and(|node| {
                        node.status == ElementStatus::Accepted
                            && matches!(node.payload, NodePayload::Stakeholder(_))
                    });
                    if !stakeholders.contains(reference) || !accepted {
                        return Err(RequirementCompilationError::InvalidIntentReference {
                            candidate_ref: candidate_ref.clone(),
                            reference: reference.clone(),
                        });
                    }
                }
            }
            IntentOutput::Concern {
                name, description, ..
            } => {
                if !grounded_text(name) || !grounded_text(description) {
                    return Err(RequirementCompilationError::InvalidIntent {
                        candidate_ref: candidate_ref.clone(),
                        reason: "concern name and description must be non-empty, without \
                                 surrounding whitespace or control characters"
                            .to_owned(),
                    });
                }
            }
            IntentOutput::Goal { .. } | IntentOutput::Constraint { .. } => {}
        }
    }
    Ok(intents)
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
