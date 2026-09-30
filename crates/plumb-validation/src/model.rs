//! The typed validation-profile metadata model: closed vocabularies, standards, gates, rules
//! and the profile itself (compiler architecture §5.5, rulebook §§1-4).
//!
//! Every rule, gate and standard is data loaded from the profile YAML. This module defines
//! shapes and vocabularies only; it holds no rule identifier and evaluates nothing.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use plumb_core::{GateId, Id};
use plumb_psg::{MappingRole, MappingStrength};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// The number of rules a profile of this family declares. A schema invariant, not rule metadata.
pub const EXPECTED_RULE_COUNT: usize = 133;

/// A string that is not a value of the named closed validation vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown {vocabulary} value {value:?}")]
pub struct UnknownVocabularyValue {
    pub vocabulary: &'static str,
    pub value: String,
}

/// Defines a closed vocabulary serialized as exactly the listed wire strings.
macro_rules! closed_vocabulary {
    (
        $(#[$meta:meta])*
        $name:ident, $count:literal { $($variant:ident => $wire:literal),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every value, in contract order.
            pub const ALL: [$name; $count] = [$($name::$variant),+];

            /// The exact wire string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $wire),+
                }
            }
        }

        impl FromStr for $name {
            type Err = UnknownVocabularyValue;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                $name::ALL
                    .into_iter()
                    .find(|value| value.as_str() == s)
                    .ok_or_else(|| UnknownVocabularyValue {
                        vocabulary: stringify!($name),
                        value: s.to_owned(),
                    })
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                value.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

closed_vocabulary! {
    /// Where a rule's authority comes from (rulebook §1).
    RuleClass, 5 {
        ExternalStandard => "external_standard",
        ExternalInterop => "external_interop",
        PlumbCore => "plumb_core",
        ProfilePolicy => "profile_policy",
        OrganizationPolicy => "organization_policy",
    }
}

closed_vocabulary! {
    /// The outcome vocabulary of one rule evaluation (rulebook §2). Exactly six values.
    RuleResultState, 6 {
        Pass => "PASS",
        Fail => "FAIL",
        Warn => "WARN",
        NotApplicable => "NOT_APPLICABLE",
        Waived => "WAIVED",
        Error => "ERROR",
    }
}

closed_vocabulary! {
    /// How much a failing rule weighs against its gate (rulebook §2.1).
    Severity, 4 {
        Blocker => "blocker",
        Error => "error",
        Warn => "warn",
        Info => "info",
    }
}

closed_vocabulary! {
    /// Whether and how a rule may be waived (rulebook §2.3).
    WaiverPolicy, 3 {
        Forbidden => "forbidden",
        DecisionRequired => "decision_required",
        ProfileAllow => "profile_allow",
    }
}

closed_vocabulary! {
    /// The evaluation contract of a rule. External validators run outside it.
    EvaluationMode, 1 {
        Deterministic => "deterministic",
    }
}

/// One entry of the profile's standards catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardDefinition {
    pub standard: String,
    pub version: String,
    pub role: MappingRole,
    pub note: String,
}

/// A rule's reference to a catalog standard, with its rule-specific note and strength.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardRef {
    pub standard: String,
    pub version: String,
    pub role: MappingRole,
    pub note: String,
    pub mapping_strength: MappingStrength,
}

/// A gate and the rules it owns. `pass_algorithm` is descriptive text, not executable logic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateMetadata {
    pub id: GateId,
    pub purpose: String,
    pub rule_ids: Vec<String>,
    pub pass_algorithm: String,
}

/// The declarative metadata of one rule, exactly as the profile YAML states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleMetadata {
    pub id: String,
    pub gate: GateId,
    pub title: String,

    pub severity: Severity,
    pub rule_class: RuleClass,

    pub applies_when: String,
    pub check: String,
    pub pass_condition: String,

    pub waiver_policy: WaiverPolicy,

    pub standard_reference: Option<StandardRef>,
    pub remediation: Option<String>,

    pub evaluation: EvaluationMode,
    pub finding_status_on_fail: String,
}

/// A validation profile: exactly the nine top-level fields of the profile YAML.
///
/// Deserialization checks shape only. [`crate::load_profile_yaml`] and
/// [`ValidationProfile::validate`] enforce the profile invariants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationProfile {
    pub profile_id: Id,
    pub profile_version: String,
    pub metamodel: String,
    pub status: String,

    pub rule_classes: BTreeMap<RuleClass, String>,
    pub result_states: Vec<RuleResultState>,
    pub standards: BTreeMap<String, StandardDefinition>,

    pub gates: Vec<GateMetadata>,
    pub rules: Vec<RuleMetadata>,
}

/// Why a profile could not be loaded, or why a gate cannot be evaluated yet.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {
    /// The YAML is not parseable into the profile shape (syntax, unknown field or value).
    #[error("profile YAML is invalid: {0}")]
    Yaml(String),
    /// A structural profile defect with no more specific variant.
    #[error("invalid profile: {reason}")]
    InvalidProfile { reason: String },
    #[error("invalid profile text in {field}: {reason}")]
    InvalidProfileText { field: String, reason: &'static str },
    #[error("profile declares {actual} rules, expected {expected}")]
    RuleCountMismatch { expected: usize, actual: usize },
    #[error("rule {0} is defined more than once")]
    DuplicateRuleId(String),
    #[error("profile declares {actual} gates, expected {expected}")]
    GateCountMismatch { expected: usize, actual: usize },
    #[error("gate {0} is defined more than once")]
    DuplicateGate(GateId),
    /// A rule ID is listed twice, within `gate` or also by `other_gate`.
    #[error("rule {rule_id} is listed by gate {gate} and again by gate {other_gate}")]
    DuplicateGateRule {
        rule_id: String,
        gate: GateId,
        other_gate: GateId,
    },
    #[error("gate {gate} lists rule {rule_id}, which has no metadata")]
    UnknownGateRule { gate: GateId, rule_id: String },
    #[error("rule {0} is not listed by any gate")]
    OrphanRule(String),
    #[error("rule {rule_id} names gate {declared} but is listed by gate {listed_by}")]
    RuleGateMismatch {
        rule_id: String,
        declared: GateId,
        listed_by: GateId,
    },
    #[error("result_states must be exactly the six rule result states, each once")]
    ResultStatesMismatch,
    #[error("rule_classes must declare exactly the five rule classes")]
    RuleClassesMismatch,
    #[error("standards catalog defines {standard} {version} ({role:?}) more than once")]
    DuplicateStandardDefinition {
        standard: String,
        version: String,
        role: MappingRole,
    },
    #[error("rule {rule_id} references {standard} {version} ({role:?}), which the catalog does not define")]
    UnknownStandardReference {
        rule_id: String,
        standard: String,
        version: String,
        role: MappingRole,
    },
    #[error("{actual} rules require a NOW evaluator, expected {expected}")]
    NowRuleCountMismatch { expected: usize, actual: usize },
    /// The gate's metadata is loaded, but no evaluator exists for it in the current scope.
    #[error("gate {0} is not implemented in the current scope")]
    GateNotImplemented(GateId),
}
