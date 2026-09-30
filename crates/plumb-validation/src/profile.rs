//! Loading, validating, normalizing and hashing a [`ValidationProfile`] (compiler
//! architecture §5.5).
//!
//! The profile YAML is the authority for every rule ID, severity and text. It is read, never
//! rewritten; only the in-memory order of semantically unordered lists is normalized.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{canonical_hash, CoreError, GateId, Hash, HashKind};
use serde::Serialize;

use crate::model::{
    GateMetadata, RuleClass, RuleMetadata, RuleResultState, ValidationError, ValidationProfile,
    EXPECTED_RULE_COUNT,
};

/// Parses, validates and normalizes a profile from YAML text.
pub fn load_profile_yaml(yaml: &str) -> Result<ValidationProfile, ValidationError> {
    let mut profile: ValidationProfile =
        serde_yaml::from_str(yaml).map_err(|e| ValidationError::Yaml(e.to_string()))?;
    profile.validate()?;
    profile.normalize();
    Ok(profile)
}

/// The software profile shipped with Plumb, loaded through [`load_profile_yaml`].
pub fn load_builtin_software_profile() -> Result<ValidationProfile, ValidationError> {
    load_profile_yaml(include_str!(
        "../../../config/profiles/plumb-software-2026.1-rules.yaml"
    ))
}

/// The part of a profile that identifies the gate/rule pack on its own.
#[derive(Serialize)]
struct RulePack<'a> {
    result_states: &'a [RuleResultState],
    gates: &'a [GateMetadata],
    rules: &'a [RuleMetadata],
}

impl ValidationProfile {
    /// Checks every profile invariant. Independent of the order of gates, rules and rule IDs.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.validate_identity()?;
        self.validate_vocabularies()?;
        self.validate_standards_catalog()?;
        let listed_by = self.validate_gates()?;
        self.validate_rules(&listed_by)
    }

    /// Puts the semantically unordered lists into their canonical order: result states in
    /// [`RuleResultState::ALL`] order, gates in [`GateId::ALL`] order, and each gate's rule IDs
    /// and the rules in lexicographic ID order.
    pub fn normalize(&mut self) {
        self.result_states.sort();
        self.gates.sort_by_key(|gate| gate.id);
        for gate in &mut self.gates {
            gate.rule_ids.sort();
        }
        self.rules.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// Generic hash of the canonical JSON of the complete normalized profile.
    pub fn profile_hash(&self) -> Result<Hash, CoreError> {
        let mut normalized = self.clone();
        normalized.normalize();
        canonical_hash(HashKind::Generic, &normalized)
    }

    /// Generic hash of the canonical JSON of exactly the normalized result states, gates and
    /// rules: the rule pack without profile identity, class descriptions or standards catalog.
    pub fn rule_pack_hash(&self) -> Result<Hash, CoreError> {
        let mut normalized = self.clone();
        normalized.normalize();
        canonical_hash(
            HashKind::Generic,
            &RulePack {
                result_states: &normalized.result_states,
                gates: &normalized.gates,
                rules: &normalized.rules,
            },
        )
    }

    fn validate_identity(&self) -> Result<(), ValidationError> {
        check_text("profile_version", &self.profile_version, false)?;
        check_text("metamodel", &self.metamodel, false)?;
        check_text("status", &self.status, false)
    }

    fn validate_vocabularies(&self) -> Result<(), ValidationError> {
        if !self.rule_classes.keys().copied().eq(RuleClass::ALL) {
            return Err(ValidationError::RuleClassesMismatch);
        }
        for (class, description) in &self.rule_classes {
            check_non_empty(&format!("rule_classes.{class}"), description)?;
        }
        let declared: BTreeSet<RuleResultState> = self.result_states.iter().copied().collect();
        if self.result_states.len() != RuleResultState::ALL.len()
            || !declared.into_iter().eq(RuleResultState::ALL)
        {
            return Err(ValidationError::ResultStatesMismatch);
        }
        Ok(())
    }

    fn validate_standards_catalog(&self) -> Result<(), ValidationError> {
        let mut seen = BTreeSet::new();
        for (key, definition) in &self.standards {
            check_text("standards key", key, false)?;
            let field = |name: &str| format!("standards.{key}.{name}");
            check_text(&field("standard"), &definition.standard, false)?;
            check_text(&field("version"), &definition.version, false)?;
            check_text(&field("note"), &definition.note, false)?;
            if !seen.insert((&definition.standard, &definition.version, definition.role)) {
                return Err(ValidationError::DuplicateStandardDefinition {
                    standard: definition.standard.clone(),
                    version: definition.version.clone(),
                    role: definition.role,
                });
            }
        }
        Ok(())
    }

    /// Validates the gates and returns, for every listed rule ID, the gate that lists it.
    fn validate_gates(&self) -> Result<BTreeMap<&str, GateId>, ValidationError> {
        let mut gates = BTreeSet::new();
        for gate in &self.gates {
            if !gates.insert(gate.id) {
                return Err(ValidationError::DuplicateGate(gate.id));
            }
        }
        if self.gates.len() != GateId::ALL.len() {
            return Err(ValidationError::GateCountMismatch {
                expected: GateId::ALL.len(),
                actual: self.gates.len(),
            });
        }
        let mut listed_by = BTreeMap::new();
        for gate in &self.gates {
            check_non_empty(&format!("gates.{}.purpose", gate.id), &gate.purpose)?;
            check_non_empty(
                &format!("gates.{}.pass_algorithm", gate.id),
                &gate.pass_algorithm,
            )?;
            if gate.rule_ids.is_empty() {
                return Err(ValidationError::InvalidProfile {
                    reason: format!("gate {} lists no rules", gate.id),
                });
            }
            for rule_id in &gate.rule_ids {
                if let Some(other_gate) = listed_by.insert(rule_id.as_str(), gate.id) {
                    return Err(ValidationError::DuplicateGateRule {
                        rule_id: rule_id.clone(),
                        gate: gate.id,
                        other_gate,
                    });
                }
            }
        }
        Ok(listed_by)
    }

    fn validate_rules(&self, listed_by: &BTreeMap<&str, GateId>) -> Result<(), ValidationError> {
        let mut rule_ids = BTreeSet::new();
        for rule in &self.rules {
            if !rule_ids.insert(rule.id.as_str()) {
                return Err(ValidationError::DuplicateRuleId(rule.id.clone()));
            }
        }
        if self.rules.len() != EXPECTED_RULE_COUNT {
            return Err(ValidationError::RuleCountMismatch {
                expected: EXPECTED_RULE_COUNT,
                actual: self.rules.len(),
            });
        }
        for rule in &self.rules {
            self.validate_rule(rule)?;
        }
        for (rule_id, gate) in listed_by {
            if !rule_ids.contains(rule_id) {
                return Err(ValidationError::UnknownGateRule {
                    gate: *gate,
                    rule_id: (*rule_id).to_owned(),
                });
            }
        }
        for rule in &self.rules {
            match listed_by.get(rule.id.as_str()) {
                None => return Err(ValidationError::OrphanRule(rule.id.clone())),
                Some(gate) if *gate != rule.gate => {
                    return Err(ValidationError::RuleGateMismatch {
                        rule_id: rule.id.clone(),
                        declared: rule.gate,
                        listed_by: *gate,
                    })
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    fn validate_rule(&self, rule: &RuleMetadata) -> Result<(), ValidationError> {
        check_text("rules.id", &rule.id, true)?;
        let field = |name: &str| format!("rules.{}.{name}", rule.id);
        check_text(&field("title"), &rule.title, true)?;
        check_text(&field("applies_when"), &rule.applies_when, true)?;
        check_text(&field("check"), &rule.check, true)?;
        check_text(&field("pass_condition"), &rule.pass_condition, true)?;
        check_text(
            &field("finding_status_on_fail"),
            &rule.finding_status_on_fail,
            true,
        )?;
        if let Some(remediation) = &rule.remediation {
            check_text(&field("remediation"), remediation, true)?;
        }
        if let Some(reference) = &rule.standard_reference {
            let field = |name: &str| format!("rules.{}.standard_reference.{name}", rule.id);
            check_text(&field("standard"), &reference.standard, false)?;
            check_text(&field("version"), &reference.version, false)?;
            check_text(&field("note"), &reference.note, false)?;
            let resolves = self.standards.values().any(|definition| {
                definition.standard == reference.standard
                    && definition.version == reference.version
                    && definition.role == reference.role
            });
            if !resolves {
                return Err(ValidationError::UnknownStandardReference {
                    rule_id: rule.id.clone(),
                    standard: reference.standard.clone(),
                    version: reference.version.clone(),
                    role: reference.role,
                });
            }
        }
        Ok(())
    }
}

fn invalid_text(field: &str, reason: &'static str) -> ValidationError {
    ValidationError::InvalidProfileText {
        field: field.to_owned(),
        reason,
    }
}

fn check_non_empty(field: &str, text: &str) -> Result<(), ValidationError> {
    if text.is_empty() {
        return Err(invalid_text(field, "empty"));
    }
    Ok(())
}

/// Non-empty, not surrounded by whitespace, no control characters. `allow_newline` permits
/// the embedded newline a folded YAML scalar may legitimately contain.
fn check_text(field: &str, text: &str, allow_newline: bool) -> Result<(), ValidationError> {
    check_non_empty(field, text)?;
    if text.trim() != text {
        return Err(invalid_text(field, "leading or trailing whitespace"));
    }
    if text
        .chars()
        .any(|c| c.is_control() && !(allow_newline && c == '\n'))
    {
        return Err(invalid_text(field, "control character"));
    }
    Ok(())
}
