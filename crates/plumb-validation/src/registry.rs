//! The validation metadata registry: lookup of loaded gate and rule metadata and the
//! NOW/deferred evaluator requirement (compiler architecture §5.5).
//!
//! The registry answers which rules exist and which of them need an evaluator in the current
//! scope. It holds no evaluator and no copy of the metadata: everything is read from the one
//! [`ValidationProfile`] it owns.

use std::collections::BTreeSet;

use plumb_core::GateId;

use crate::model::{GateMetadata, RuleMetadata, ValidationError, ValidationProfile};

/// The gates whose rules require an evaluator in the current implementation scope.
pub const NOW_GATES: [GateId; 5] = [GateId::I0, GateId::F1, GateId::F2, GateId::F3, GateId::F4];

/// The number of rules the NOW gates of a profile of this family own.
pub const EXPECTED_NOW_RULE_COUNT: usize = 54;

/// Deterministic lookup over one validated, normalized [`ValidationProfile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationRegistry {
    profile: ValidationProfile,
}

impl ValidationRegistry {
    /// Validates and normalizes `profile` and checks the NOW evaluator requirement.
    pub fn new(mut profile: ValidationProfile) -> Result<Self, ValidationError> {
        profile.validate()?;
        profile.normalize();
        let registry = Self { profile };
        let actual = registry.required_now_rule_ids().len();
        if actual != EXPECTED_NOW_RULE_COUNT {
            return Err(ValidationError::NowRuleCountMismatch {
                expected: EXPECTED_NOW_RULE_COUNT,
                actual,
            });
        }
        Ok(registry)
    }

    pub fn profile(&self) -> &ValidationProfile {
        &self.profile
    }

    /// The metadata of rule `id`, if the profile defines it.
    pub fn rule(&self, id: &str) -> Option<&RuleMetadata> {
        // Rules are normalized into lexicographic ID order.
        self.profile
            .rules
            .binary_search_by(|rule| rule.id.as_str().cmp(id))
            .ok()
            .map(|index| &self.profile.rules[index])
    }

    /// The metadata of `gate`. Every gate is loaded, whether or not it is in NOW scope.
    pub fn gate(&self, gate: GateId) -> &GateMetadata {
        // Gates are normalized into `GateId::ALL` order, one entry per gate.
        &self.profile.gates[gate.ordinal()]
    }

    /// The rules `gate` owns, in lexicographic ID order.
    pub fn rules_for_gate(&self, gate: GateId) -> Vec<&RuleMetadata> {
        self.gate(gate)
            .rule_ids
            .iter()
            .filter_map(|id| self.rule(id))
            .collect()
    }

    /// The IDs of every rule that requires an evaluator in NOW scope.
    pub fn required_now_rule_ids(&self) -> BTreeSet<&str> {
        self.rule_ids_where(Self::is_now_gate)
    }

    /// The IDs of every rule whose metadata is loaded but whose evaluator is deferred.
    pub fn deferred_rule_ids(&self) -> BTreeSet<&str> {
        self.rule_ids_where(|gate| !Self::is_now_gate(gate))
    }

    pub fn is_now_gate(gate: GateId) -> bool {
        NOW_GATES.contains(&gate)
    }

    /// Succeeds for a NOW gate; a later gate has metadata but no evaluator yet.
    pub fn require_now_gate(gate: GateId) -> Result<(), ValidationError> {
        if Self::is_now_gate(gate) {
            Ok(())
        } else {
            Err(ValidationError::GateNotImplemented(gate))
        }
    }

    fn rule_ids_where(&self, include: impl Fn(GateId) -> bool) -> BTreeSet<&str> {
        self.profile
            .gates
            .iter()
            .filter(|gate| include(gate.id))
            .flat_map(|gate| gate.rule_ids.iter().map(String::as_str))
            .collect()
    }
}
