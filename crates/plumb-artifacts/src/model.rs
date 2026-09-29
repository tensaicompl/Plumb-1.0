//! Artifact model: the closed set of artifact kinds and the persisted artifact record.

use std::fmt;
use std::str::FromStr;

use plumb_core::{Hash, Timestamp};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// The family an artifact belongs to (compiler architecture §24).
///
/// Serialized and persisted as exactly the kebab-case strings returned by [`ArtifactKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactKind {
    SourceOriginal,
    SourceExtracted,
    EvidenceManifest,
    InferenceRequest,
    InferenceResponse,
    ValidatedInference,
    ExternalValidation,
    Projection,
    Proposal,
    Patch,
    Diff,
    ImpactReport,
    GateReport,
    ConformanceReport,
    ScenarioTrace,
    TestReceipt,
    ArchitectureCheck,
    CompileRun,
}

impl ArtifactKind {
    /// Every artifact kind, in specification order.
    pub const ALL: [ArtifactKind; 18] = [
        ArtifactKind::SourceOriginal,
        ArtifactKind::SourceExtracted,
        ArtifactKind::EvidenceManifest,
        ArtifactKind::InferenceRequest,
        ArtifactKind::InferenceResponse,
        ArtifactKind::ValidatedInference,
        ArtifactKind::ExternalValidation,
        ArtifactKind::Projection,
        ArtifactKind::Proposal,
        ArtifactKind::Patch,
        ArtifactKind::Diff,
        ArtifactKind::ImpactReport,
        ArtifactKind::GateReport,
        ArtifactKind::ConformanceReport,
        ArtifactKind::ScenarioTrace,
        ArtifactKind::TestReceipt,
        ArtifactKind::ArchitectureCheck,
        ArtifactKind::CompileRun,
    ];

    /// The exact persisted/serialized string for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::SourceOriginal => "source-original",
            ArtifactKind::SourceExtracted => "source-extracted",
            ArtifactKind::EvidenceManifest => "evidence-manifest",
            ArtifactKind::InferenceRequest => "inference-request",
            ArtifactKind::InferenceResponse => "inference-response",
            ArtifactKind::ValidatedInference => "validated-inference",
            ArtifactKind::ExternalValidation => "external-validation",
            ArtifactKind::Projection => "projection",
            ArtifactKind::Proposal => "proposal",
            ArtifactKind::Patch => "patch",
            ArtifactKind::Diff => "diff",
            ArtifactKind::ImpactReport => "impact-report",
            ArtifactKind::GateReport => "gate-report",
            ArtifactKind::ConformanceReport => "conformance-report",
            ArtifactKind::ScenarioTrace => "scenario-trace",
            ArtifactKind::TestReceipt => "test-receipt",
            ArtifactKind::ArchitectureCheck => "architecture-check",
            ArtifactKind::CompileRun => "compile-run",
        }
    }
}

/// A persisted or supplied kind string that is not one of the 18 artifact kinds.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown artifact kind {0:?}")]
pub struct UnknownArtifactKind(pub String);

impl FromStr for ArtifactKind {
    type Err = UnknownArtifactKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ArtifactKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == s)
            .ok_or_else(|| UnknownArtifactKind(s.to_owned()))
    }
}

impl fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ArtifactKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ArtifactKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// An immutable, content-addressed artifact as persisted by the artifact store.
///
/// `hash` is the generic `sha256:` hash of `bytes`; `kind`, `media_type` and `created_at`
/// are persisted metadata that do not participate in the hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub hash: Hash,
    pub kind: ArtifactKind,
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub created_at: Timestamp,
}
