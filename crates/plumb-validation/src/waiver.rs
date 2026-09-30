//! Governed waivers: recognizing the `ResolutionDecision` that waives one deterministic
//! finding and applying the rule's waiver policy (metamodel §6.3, rulebook §2.3).
//!
//! No clock is consulted. `ResolutionDecision` has no review or expiry field, so a waiver is
//! validated for owner, exact finding scope, rationale and accepted governance state only.

use plumb_core::{Hash, HashKind, Id};
use plumb_psg::{ElementStatus, Finding, Graph, NodePayload, RelationKind, ResolutionDecision};
use serde::{Deserialize, Serialize};

use crate::evaluator::{EvaluationError, ValidationPolicy};
use crate::finding::is_clean_text;
use crate::model::{RuleMetadata, WaiverPolicy};

/// The `kind` that marks a `ResolutionDecision.answer` as a waiver.
pub const WAIVER_KIND: &str = "waiver";

/// The exact `ResolutionDecision.answer` of a waiver decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaiverDecisionMarker {
    pub kind: String,
    pub rule_id: String,
    pub finding_key: Hash,
    pub finding_ref: Id,
}

/// A waiver that satisfied a violated rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedWaiver {
    pub rule_id: String,
    pub finding_ref: Id,
    pub finding_key: Hash,
    pub decision_ref: Id,
}

/// Looks in `graph` for the governed waiver of the finding `finding_ref` of `rule`, whose
/// violation targets are `targets` (sorted).
///
/// Returns `None` when no accepted waiver decision resolves the accepted finding. A decision
/// that claims to be a waiver but is malformed, forbidden, not enabled or ambiguous is an
/// error, never silently ignored, and so is a waiver claim on a node at the deterministic
/// Finding ID whose payload is not this finding.
pub fn governed_waiver(
    graph: &Graph,
    rule: &RuleMetadata,
    policy: &ValidationPolicy,
    finding_key: &Hash,
    finding_ref: &Id,
    targets: &[Id],
) -> Result<Option<AppliedWaiver>, EvaluationError> {
    let Some(finding_node) = graph.node(finding_ref) else {
        return Ok(None);
    };
    let NodePayload::Finding(finding) = &finding_node.payload else {
        return Ok(None);
    };
    if finding_node.status != ElementStatus::Accepted {
        return Ok(None);
    }

    let mut qualifying: Vec<Id> = Vec::new();
    for edge_id in graph.incoming_edge_ids(finding_ref) {
        let Some(edge) = graph.edge(edge_id) else {
            continue;
        };
        if edge.kind != RelationKind::Resolves || edge.status != ElementStatus::Accepted {
            continue;
        }
        let Some(decision_node) = graph.node(&edge.from) else {
            continue;
        };
        let NodePayload::ResolutionDecision(decision) = &decision_node.payload else {
            continue;
        };
        if decision_node.status != ElementStatus::Accepted || !claims_waiver(decision) {
            continue;
        }
        check_finding_identity(finding_ref, finding, rule, targets)?;
        check_waiver_decision(&decision_node.id, decision, rule, finding_key, finding_ref)?;
        qualifying.push(decision_node.id.clone());
    }

    // Two edges from one decision to the same finding are still one decision.
    qualifying.sort();
    qualifying.dedup();
    let decision_ref = match qualifying.as_slice() {
        [] => return Ok(None),
        [decision] => decision.clone(),
        _ => {
            return Err(EvaluationError::AmbiguousWaiver {
                rule_id: rule.id.clone(),
                finding_ref: finding_ref.clone(),
                decisions: qualifying,
            })
        }
    };

    match rule.waiver_policy {
        WaiverPolicy::Forbidden => Err(EvaluationError::ForbiddenWaiver {
            rule_id: rule.id.clone(),
            decision: decision_ref,
        }),
        WaiverPolicy::ProfileAllow if !policy.profile_allow_waiver_rule_ids.contains(&rule.id) => {
            Err(EvaluationError::ProfileWaiverNotEnabled {
                rule_id: rule.id.clone(),
                decision: decision_ref,
            })
        }
        WaiverPolicy::DecisionRequired | WaiverPolicy::ProfileAllow => Ok(Some(AppliedWaiver {
            rule_id: rule.id.clone(),
            finding_ref: finding_ref.clone(),
            finding_key: finding_key.clone(),
            decision_ref,
        })),
    }
}

/// Requires the accepted Finding at the deterministic ID to be the evaluated finding: same
/// rule code, gate family and sorted targets. Severity, message, resolution and waiver_ref
/// may legitimately differ.
fn check_finding_identity(
    finding_ref: &Id,
    finding: &Finding,
    rule: &RuleMetadata,
    targets: &[Id],
) -> Result<(), EvaluationError> {
    let mismatch = |reason: String| EvaluationError::FindingIdentityMismatch {
        finding_ref: finding_ref.clone(),
        reason,
    };
    if finding.code != rule.id {
        return Err(mismatch(format!(
            "code {:?} is not rule {:?}",
            finding.code, rule.id
        )));
    }
    if finding.family != rule.gate.as_str() {
        return Err(mismatch(format!(
            "family {:?} is not gate {}",
            finding.family, rule.gate
        )));
    }
    let mut expected = targets.to_vec();
    expected.sort();
    if finding.affected_refs != expected {
        return Err(mismatch(
            "affected_refs are not the violation targets".into(),
        ));
    }
    Ok(())
}

/// Whether the decision's answer is an object whose `kind` is the waiver kind.
fn claims_waiver(decision: &ResolutionDecision) -> bool {
    decision.answer.get("kind").and_then(|kind| kind.as_str()) == Some(WAIVER_KIND)
}

/// Requires a decision that claims to be a waiver to be exactly the waiver of this finding.
fn check_waiver_decision(
    decision_id: &Id,
    decision: &ResolutionDecision,
    rule: &RuleMetadata,
    finding_key: &Hash,
    finding_ref: &Id,
) -> Result<(), EvaluationError> {
    let malformed = |reason: String| EvaluationError::MalformedWaiverDecision {
        decision: decision_id.clone(),
        reason,
    };
    let marker: WaiverDecisionMarker = serde_json::from_value(decision.answer.clone())
        .map_err(|e| malformed(format!("answer is not a waiver marker: {e}")))?;
    if marker.finding_key.kind() != HashKind::Generic {
        return Err(malformed(format!(
            "finding_key {} is not a Generic hash",
            marker.finding_key
        )));
    }
    if marker.rule_id != rule.id {
        return Err(malformed(format!(
            "rule_id {:?} is not the evaluated rule {:?}",
            marker.rule_id, rule.id
        )));
    }
    if &marker.finding_key != finding_key {
        return Err(malformed(format!(
            "finding_key {} is not the deterministic key {finding_key}",
            marker.finding_key
        )));
    }
    if &marker.finding_ref != finding_ref {
        return Err(malformed(format!(
            "finding_ref {} is not the deterministic finding {finding_ref}",
            marker.finding_ref
        )));
    }
    if !decision.rationale.as_deref().is_some_and(is_clean_text) {
        return Err(malformed("rationale is missing or not clean text".into()));
    }
    Ok(())
}
