//! Outcome selection material (H0.4, architecture §33.4).
//!
//! An Outcome may carry the node extension [`OUTCOME_SELECTION_EXTENSION`] =
//! `{"version": 1, "selections": [...]}` whose entries, sorted by `operation_ref`, are either
//! `{"operation_ref": Id, "when": <PlumbExpr Bool source>}` (only on `success` or `partial`
//! Outcomes) or `{"operation_ref": Id, "on_failures": [sorted unique business failure kinds]}`
//! (only on `business_failure` Outcomes). `technical_failure` Outcomes are never selected, so
//! they carry no selection. The value is written only through a governed `SetNodeExtension`
//! proposal from the R0.6 directory and read by the simulator; this module owns its frozen
//! shape and validation.

use plumb_core::Id;
use plumb_psg::{ElementStatus, Graph, NodePayload, NodeType, OutcomeKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The node extension key of the outcome selection material.
pub const OUTCOME_SELECTION_EXTENSION: &str = "plumb_sim:outcome_selection";
/// The frozen version of the outcome selection value.
pub const OUTCOME_SELECTION_VERSION: u32 = 1;

/// A business rejection kind (architecture §33.4). Declared in wire-name order, so the derived
/// order is the sorted order of the wire names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessFailureKind {
    AuthorizationDenied,
    InvalidSourceState,
    InvariantViolation,
    NoDecisionRow,
    PreconditionRejected,
    TransitionGuardRejected,
}

impl BusinessFailureKind {
    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            BusinessFailureKind::AuthorizationDenied => "authorization_denied",
            BusinessFailureKind::InvalidSourceState => "invalid_source_state",
            BusinessFailureKind::InvariantViolation => "invariant_violation",
            BusinessFailureKind::NoDecisionRow => "no_decision_row",
            BusinessFailureKind::PreconditionRejected => "precondition_rejected",
            BusinessFailureKind::TransitionGuardRejected => "transition_guard_rejected",
        }
    }
}

/// One selection entry: exactly one of `when` and `on_failures` is present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeSelectionEntry {
    pub operation_ref: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_failures: Option<Vec<BusinessFailureKind>>,
}

/// The frozen `{"version": 1, "selections": [...]}` value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeSelection {
    pub version: u32,
    pub selections: Vec<OutcomeSelectionEntry>,
}

/// Why an outcome selection value is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OutcomeSelectionError {
    #[error("outcome selection of {outcome_ref} is malformed: {reason}")]
    Malformed { outcome_ref: Id, reason: String },
    #[error("outcome selection of {outcome_ref} is invalid: {reason}")]
    Invalid { outcome_ref: Id, reason: String },
}

impl OutcomeSelection {
    /// The selection entry for `operation_ref`, if any.
    pub fn entry(&self, operation_ref: &Id) -> Option<&OutcomeSelectionEntry> {
        self.selections
            .iter()
            .find(|e| &e.operation_ref == operation_ref)
    }

    /// Decodes the exact canonical wire value: unknown keys, `null` members and any value whose
    /// re-encoding differs are malformed.
    pub fn from_value(outcome_ref: &Id, value: &Value) -> Result<Self, OutcomeSelectionError> {
        let malformed = |reason: String| OutcomeSelectionError::Malformed {
            outcome_ref: outcome_ref.clone(),
            reason,
        };
        let decoded: OutcomeSelection =
            serde_json::from_value(value.clone()).map_err(|e| malformed(e.to_string()))?;
        let encoded = serde_json::to_value(&decoded).map_err(|e| malformed(e.to_string()))?;
        if &encoded != value {
            return Err(malformed("the value is not in its exact wire form".into()));
        }
        Ok(decoded)
    }

    /// The canonical wire value.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// Validates the shape against the Outcome kind and the graph: version 1, a non-empty
    /// selection list strictly sorted by Accepted Operation `operation_ref`, `when` (parseable
    /// PlumbExpr) only on `success`/`partial`, `on_failures` (non-empty, sorted, unique) only on
    /// `business_failure`, and nothing on `technical_failure`.
    pub fn validate(&self, graph: &Graph, outcome_ref: &Id) -> Result<(), OutcomeSelectionError> {
        let invalid = |reason: String| OutcomeSelectionError::Invalid {
            outcome_ref: outcome_ref.clone(),
            reason,
        };
        let Some(NodePayload::Outcome(outcome)) = graph.node(outcome_ref).map(|n| &n.payload)
        else {
            return Err(invalid("the target is not an Outcome".into()));
        };
        if self.version != OUTCOME_SELECTION_VERSION {
            return Err(invalid(format!("unsupported version {}", self.version)));
        }
        if self.selections.is_empty() {
            return Err(invalid("the selection list is empty".into()));
        }
        if !self
            .selections
            .windows(2)
            .all(|p| p[0].operation_ref < p[1].operation_ref)
        {
            return Err(invalid(
                "entries are not strictly sorted by operation_ref".into(),
            ));
        }
        for entry in &self.selections {
            let op = &entry.operation_ref;
            let operation = graph.node(op).filter(|n| {
                n.status == ElementStatus::Accepted
                    && n.payload.node_type() == NodeType::Operation
            });
            if operation.is_none() {
                return Err(invalid(format!("{op} is not an Accepted Operation")));
            }
            match (&entry.when, &entry.on_failures, &outcome.outcome_kind) {
                (Some(when), None, OutcomeKind::Success | OutcomeKind::Partial) => {
                    plumb_expr::parse(when)
                        .map_err(|e| invalid(format!("when of {op} is not PlumbExpr: {e}")))?;
                }
                (None, Some(kinds), OutcomeKind::BusinessFailure) => {
                    if kinds.is_empty() || !kinds.windows(2).all(|p| p[0] < p[1]) {
                        return Err(invalid(format!(
                            "on_failures of {op} must be non-empty, sorted and unique"
                        )));
                    }
                }
                (Some(_), Some(_), _) | (None, None, _) => {
                    return Err(invalid(format!(
                        "the entry of {op} needs exactly one of when and on_failures"
                    )))
                }
                (_, _, kind) => {
                    return Err(invalid(format!(
                        "the entry of {op} does not apply to a {kind:?} Outcome"
                    )))
                }
            }
        }
        Ok(())
    }
}
