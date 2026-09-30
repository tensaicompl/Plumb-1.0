//! Deterministic finding identity and generated finding material (compiler architecture §29,
//! rulebook §2.2).
//!
//! A finding is identified by its rule, its targets and the evaluator's semantic condition
//! key, never by severity, message, policy or waiver. This module produces the finding
//! payload only; node envelopes and persistence belong to stage logic.

use std::collections::BTreeSet;

use plumb_core::{Hash, HashKind, Id};
use plumb_psg::{Finding, FindingSeverity};
use serde::{Deserialize, Serialize};

use crate::evaluator::EvaluationError;
use crate::model::{RuleMetadata, Severity};

/// Number of digest hex digits in a Finding node ID.
const FINDING_ID_HEX_LEN: usize = 16;

/// Non-empty, no leading/trailing whitespace, no control characters.
pub(crate) fn is_clean_text(text: &str) -> bool {
    !text.is_empty() && text.trim() == text && !text.chars().any(char::is_control)
}

/// The exact hashed bytes: `rule_id 0x00 (target 0x00)* semantic_condition_key`, with targets
/// in `Id` order.
fn finding_key_bytes(
    rule_id: &str,
    targets: &[Id],
    semantic_condition_key: &str,
) -> Result<Vec<u8>, EvaluationError> {
    if !is_clean_text(rule_id) {
        return Err(EvaluationError::InvalidFindingKey(format!(
            "invalid rule id {rule_id:?}"
        )));
    }
    if !is_clean_text(semantic_condition_key) {
        return Err(EvaluationError::InvalidSemanticConditionKey(
            semantic_condition_key.to_owned(),
        ));
    }
    let sorted: BTreeSet<&Id> = targets.iter().collect();
    if sorted.len() != targets.len() {
        return Err(EvaluationError::InvalidFindingKey(
            "duplicate target".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(rule_id.as_bytes());
    bytes.push(0);
    for target in sorted {
        bytes.extend_from_slice(target.as_str().as_bytes());
        bytes.push(0);
    }
    bytes.extend_from_slice(semantic_condition_key.as_bytes());
    Ok(bytes)
}

/// The deterministic Generic finding key of a violated condition.
pub fn finding_key(
    rule_id: &str,
    targets: &[Id],
    semantic_condition_key: &str,
) -> Result<Hash, EvaluationError> {
    finding_key_bytes(rule_id, targets, semantic_condition_key)
        .map(|bytes| Hash::content_sha256(&bytes))
}

/// The deterministic Finding node ID: `fnd:` and the first 16 hex digits of the key digest.
pub fn finding_id(key: &Hash) -> Result<Id, EvaluationError> {
    if key.kind() != HashKind::Generic {
        return Err(EvaluationError::InvalidFindingId(format!(
            "finding key {key} is not a Generic hash"
        )));
    }
    let digest = key.as_str().rsplit(':').next().unwrap_or_default();
    format!("fnd:{}", &digest[..FINDING_ID_HEX_LEN])
        .parse()
        .map_err(|e: plumb_core::CoreError| EvaluationError::InvalidFindingId(e.to_string()))
}

/// The PSG finding severity of a validation severity. The two vocabularies stay separate.
pub fn finding_severity(severity: Severity) -> FindingSeverity {
    match severity {
        Severity::Blocker => FindingSeverity::Blocker,
        Severity::Error => FindingSeverity::Error,
        Severity::Warn => FindingSeverity::Warn,
        Severity::Info => FindingSeverity::Info,
    }
}

/// The deterministic finding produced by one violation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedFinding {
    pub key: Hash,
    pub id: Id,
    pub semantic_condition_key: String,
    pub payload: Finding,
}

/// What a violation contributes to its finding besides the rule metadata.
pub struct ViolationFacts<'a> {
    pub targets: &'a [Id],
    pub semantic_condition_key: &'a str,
    pub message: &'a str,
    pub suggested_resolution: Option<&'a str>,
}

impl GeneratedFinding {
    /// Builds the finding of `rule` for one violation. `effective_severity` is the severity
    /// after policy promotion; `waiver_ref` is the applied waiver decision, if any.
    pub fn for_violation(
        rule: &RuleMetadata,
        effective_severity: Severity,
        violation: &ViolationFacts<'_>,
        waiver_ref: Option<Id>,
    ) -> Result<GeneratedFinding, EvaluationError> {
        let key = finding_key(
            &rule.id,
            violation.targets,
            violation.semantic_condition_key,
        )?;
        let id = finding_id(&key)?;
        let mut affected_refs = violation.targets.to_vec();
        affected_refs.sort();
        Ok(GeneratedFinding {
            key,
            id,
            semantic_condition_key: violation.semantic_condition_key.to_owned(),
            payload: Finding {
                code: rule.id.clone(),
                family: rule.gate.as_str().to_owned(),
                severity: finding_severity(effective_severity),
                message: violation.message.to_owned(),
                status: rule.finding_status_on_fail.clone(),
                affected_refs,
                standard_rule_ref: rule.standard_reference.as_ref().map(|_| rule.id.clone()),
                suggested_resolution: violation.suggested_resolution.map(str::to_owned),
                waiver_ref,
            },
        })
    }
}
