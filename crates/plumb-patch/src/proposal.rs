//! The canonical, patch-centric compiler `Proposal` (compiler architecture §4.5).
//!
//! A proposal is immutable stage output: a `PatchSet` plus provenance and acceptance metadata.
//! Its semantic base is `PatchSet.base_semantic_hash`; job revisions, intake reports and impact
//! are derived elsewhere and are never embedded here.

use std::fmt;
use std::str::FromStr;

use plumb_core::{to_canonical_json, CoreError, Hash, HashKind, Id, StageId};
use plumb_psg::{DerivationRef, EvidenceRef};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};
use thiserror::Error;

use crate::model::PatchSet;

/// A string that is not one of the values of a closed proposal enum.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown {kind} {value:?}")]
pub struct UnknownProposalValue {
    pub kind: &'static str,
    pub value: String,
}

macro_rules! wire_enum {
    ($(#[$meta:meta])* $name:ident, $kind:literal { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every value, in specification order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The exact wire string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }

        impl FromStr for $name {
            type Err = UnknownProposalValue;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                $name::ALL
                    .iter()
                    .copied()
                    .find(|v| v.as_str() == s)
                    .ok_or_else(|| UnknownProposalValue { kind: $kind, value: s.to_owned() })
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
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(serde::de::Error::custom)
            }
        }
    };
}

wire_enum!(
    /// Whether a proposal changes engineering semantics and whether it is a material decision.
    ProposalMateriality, "proposal materiality" {
        NonSemantic => "non_semantic",
        Semantic => "semantic",
        MaterialDecision => "material_decision",
    }
);

wire_enum!(
    /// How a proposal may be accepted (compiler architecture §4.6). LLM-originated accepted
    /// semantics must not use `AUTO_DERIVATION`; that is enforced by intake/acceptance.
    AcceptancePolicy, "acceptance policy" {
        AutoDerivation => "AUTO_DERIVATION",
        AutoNonSemantic => "AUTO_NON_SEMANTIC",
        HumanConfirm => "HUMAN_CONFIRM",
        HumanDecision => "HUMAN_DECISION",
        ProfilePolicy => "PROFILE_POLICY",
    }
);

/// Why a proposal is invalid.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ProposalError {
    /// `patch_set.base_semantic_hash` is not a semantic hash.
    #[error("patch_set.base_semantic_hash must be a semantic hash, got {0:?}")]
    InvalidBaseHashKind(HashKind),
    /// A provenance ref list contains the same ID twice.
    #[error("duplicate {field} entry {value}")]
    DuplicateRef { field: &'static str, value: String },
    /// A provenance ref list is not in canonical sorted order.
    #[error("{field} is not sorted")]
    UnsortedRefs { field: &'static str },
    /// `confidence` is not a finite value in `0.0..=1.0`.
    #[error("confidence {0} must be finite and within 0.0..=1.0")]
    InvalidConfidence(f32),
    /// The supplied ID is not the ID recomputed from the identity projection.
    #[error("proposal id {supplied} does not match recomputed id {recomputed}")]
    IdMismatch { supplied: Id, recomputed: Id },
    /// Canonical JSON or hashing failed.
    #[error(transparent)]
    Core(#[from] CoreError),
}

/// Requires `refs` (compared by their ID strings) to be strictly increasing.
fn check_refs<T: Ord>(
    field: &'static str,
    refs: &[T],
    as_str: impl Fn(&T) -> &str,
) -> Result<(), ProposalError> {
    for pair in refs.windows(2) {
        if pair[0] == pair[1] {
            return Err(ProposalError::DuplicateRef {
                field,
                value: as_str(&pair[0]).to_owned(),
            });
        }
        if pair[0] > pair[1] {
            return Err(ProposalError::UnsortedRefs { field });
        }
    }
    Ok(())
}

/// Immutable stage output proposing one `PatchSet` for intake and acceptance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ProposalFields")]
pub struct Proposal {
    pub id: Id,
    pub stage: StageId,
    pub patch_set: PatchSet,

    pub evidence_refs: Vec<EvidenceRef>,
    pub derivation_refs: Vec<DerivationRef>,

    pub materiality: ProposalMateriality,
    pub acceptance_policy: AcceptancePolicy,
    /// Advisory only; never drives gates, intake, acceptance or readiness.
    pub confidence: Option<f32>,
}

impl Proposal {
    /// Builds a proposal, sorting the provenance refs and computing the ID.
    pub fn new(
        stage: StageId,
        patch_set: PatchSet,
        mut evidence_refs: Vec<EvidenceRef>,
        mut derivation_refs: Vec<DerivationRef>,
        materiality: ProposalMateriality,
        acceptance_policy: AcceptancePolicy,
        confidence: Option<f32>,
    ) -> Result<Proposal, ProposalError> {
        evidence_refs.sort();
        derivation_refs.sort();
        let mut proposal = Proposal {
            // Placeholder replaced below; the identity input excludes `id`.
            id: "prop:0000000000000000".parse()?,
            stage,
            patch_set,
            evidence_refs,
            derivation_refs,
            materiality,
            acceptance_policy,
            confidence,
        };
        proposal.id = proposal.recompute_id()?;
        proposal.validate()?;
        Ok(proposal)
    }

    /// Exactly the fields that determine the proposal ID (everything except `id`).
    pub fn identity_projection(&self) -> Value {
        json!({
            "stage": self.stage,
            "patch_set": self.patch_set,
            "evidence_refs": self.evidence_refs,
            "derivation_refs": self.derivation_refs,
            "materiality": self.materiality,
            "acceptance_policy": self.acceptance_policy,
            "confidence": self.confidence,
        })
    }

    /// `prop:<first 16 hex of SHA-256(RFC 8785 identity projection)>`.
    pub fn recompute_id(&self) -> Result<Id, ProposalError> {
        let digest = Hash::content_sha256(&to_canonical_json(&self.identity_projection())?);
        let hex = &digest.as_str()["sha256:".len()..];
        Ok(format!("prop:{}", &hex[..16]).parse()?)
    }

    /// Checks every field rule and that `id` equals the recomputed ID.
    pub fn validate(&self) -> Result<(), ProposalError> {
        let base_kind = self.patch_set.base_semantic_hash.kind();
        if base_kind != HashKind::Semantic {
            return Err(ProposalError::InvalidBaseHashKind(base_kind));
        }
        check_refs("evidence_refs", &self.evidence_refs, EvidenceRef::as_str)?;
        check_refs(
            "derivation_refs",
            &self.derivation_refs,
            DerivationRef::as_str,
        )?;
        if let Some(c) = self.confidence {
            if !c.is_finite() || !(0.0..=1.0).contains(&c) {
                return Err(ProposalError::InvalidConfidence(c));
            }
        }
        let recomputed = self.recompute_id()?;
        if recomputed != self.id {
            return Err(ProposalError::IdMismatch {
                supplied: self.id.clone(),
                recomputed,
            });
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalFields {
    id: Id,
    stage: StageId,
    patch_set: PatchSet,
    evidence_refs: Vec<EvidenceRef>,
    derivation_refs: Vec<DerivationRef>,
    materiality: ProposalMateriality,
    acceptance_policy: AcceptancePolicy,
    confidence: Option<f32>,
}

impl TryFrom<ProposalFields> for Proposal {
    type Error = ProposalError;

    fn try_from(f: ProposalFields) -> Result<Self, Self::Error> {
        let proposal = Proposal {
            id: f.id,
            stage: f.stage,
            patch_set: f.patch_set,
            evidence_refs: f.evidence_refs,
            derivation_refs: f.derivation_refs,
            materiality: f.materiality,
            acceptance_policy: f.acceptance_policy,
            confidence: f.confidence,
        };
        proposal.validate()?;
        Ok(proposal)
    }
}
