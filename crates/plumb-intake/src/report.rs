//! The patch-centric intake report contract (plan X1.3, Hotfix 059): analysis material about
//! one immutable Proposal against one current graph, its self-validation and the typed errors
//! of the acceptance-rationale guard.
//!
//! The report holds no graph, no proposal copy, no timestamp and has no content-hash identity.

use std::collections::BTreeSet;
use std::fmt;

use plumb_core::{GateId, Hash, HashKind, Id};
use plumb_patch::ImpactReport;
use plumb_psg::FindingSeverity;
use plumb_validation::GeneratedFinding;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::index::RetrievalDocumentKind;

/// The maximum number of same-kind lexical duplicate candidates kept per subject.
pub const INTAKE_DUPLICATE_CANDIDATE_LIMIT: usize = 20;

/// An Accepted base node whose typed payload equals the subject payload exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageMatch {
    pub subject_ref: Id,
    pub covered_by_ref: Id,
    /// Always exactly 1.0.
    pub score: f64,
}

/// An advisory same-kind BM25 hit for a subject; never proof of duplication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicateCandidate {
    pub subject_ref: Id,
    pub candidate_ref: Id,
    pub score: f64,
}

/// An explicit `refines` edge added or modified by the proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefinementLink {
    pub edge_ref: Id,
    pub from_ref: Id,
    pub to_ref: Id,
}

/// An explicit `conflicts_with` edge added or modified by the proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredConflict {
    pub edge_ref: Id,
    pub from_ref: Id,
    pub to_ref: Id,
}

/// How a proposed Term key resolves against the Accepted Term dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TermResolutionStatus {
    New,
    Resolved,
    Ambiguous,
    Unsearchable,
}

impl TermResolutionStatus {
    /// The exact wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            TermResolutionStatus::New => "new",
            TermResolutionStatus::Resolved => "resolved",
            TermResolutionStatus::Ambiguous => "ambiguous",
            TermResolutionStatus::Unsearchable => "unsearchable",
        }
    }
}

impl fmt::Display for TermResolutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The resolution of one proposed Term.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermResolution {
    pub term_ref: Id,
    pub key: Option<String>,
    pub matched_term_refs: Vec<Id>,
    pub status: TermResolutionStatus,
}

/// The lexical novelty of one subject against same-kind Accepted base documents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectNovelty {
    pub subject_ref: Id,
    pub kind: RetrievalDocumentKind,
    pub nearest_ref: Option<Id>,
    pub similarity: f64,
    pub novelty: f64,
}

/// Why a proposal requires a recorded rationale before acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeReasonCode {
    MaterialDecision,
    HumanDecision,
    ExactEquivalent,
    DeclaredConflict,
    ValidationConflict,
    AmbiguousTerm,
}

impl IntakeReasonCode {
    /// The exact wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            IntakeReasonCode::MaterialDecision => "material_decision",
            IntakeReasonCode::HumanDecision => "human_decision",
            IntakeReasonCode::ExactEquivalent => "exact_equivalent",
            IntakeReasonCode::DeclaredConflict => "declared_conflict",
            IntakeReasonCode::ValidationConflict => "validation_conflict",
            IntakeReasonCode::AmbiguousTerm => "ambiguous_term",
        }
    }
}

impl fmt::Display for IntakeReasonCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The intake analysis of one proposal against one current graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeReport {
    pub proposal_ref: Id,

    pub base_semantic_hash: Hash,
    pub candidate_semantic_hash: Hash,

    pub retrieval_corpus_hash: Hash,

    pub covered_by: Vec<CoverageMatch>,
    pub duplicate_candidates: Vec<DuplicateCandidate>,
    pub refines: Vec<RefinementLink>,

    pub term_resolutions: Vec<TermResolution>,

    pub declared_conflicts: Vec<DeclaredConflict>,

    pub validation_gates: Vec<GateId>,
    pub validation_findings: Vec<GeneratedFinding>,

    pub subject_novelty: Vec<SubjectNovelty>,
    pub novelty: Option<f64>,

    pub impact: ImpactReport,

    pub reason_codes: BTreeSet<IntakeReasonCode>,
    pub requires_reason: bool,
}

/// Why an intake report violates its contract.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IntakeReportError {
    #[error("{field} has hash kind {actual:?}, expected {expected:?}")]
    HashKind {
        field: &'static str,
        expected: HashKind,
        actual: HashKind,
    },
    #[error("{collection} is not in canonical order or has duplicates")]
    Unordered { collection: &'static str },
    #[error("coverage of {subject_ref} by {covered_by_ref} must have score exactly 1.0")]
    CoverageScore { subject_ref: Id, covered_by_ref: Id },
    #[error(
        "duplicate candidate {candidate_ref} of {subject_ref} must have a finite positive score"
    )]
    CandidateScore { subject_ref: Id, candidate_ref: Id },
    #[error("subject {0} is its own duplicate candidate")]
    SelfCandidate(Id),
    #[error("subject {0} has more than the duplicate candidate limit")]
    CandidateLimit(Id),
    #[error("term resolution of {0} is inconsistent with its status")]
    TermResolution(Id),
    #[error("novelty of {0} is not finite within 0..=1 or does not match its similarity")]
    SubjectNovelty(Id),
    #[error("overall novelty is not the minimum subject novelty")]
    Novelty,
    #[error("reason code {0} does not match the report content")]
    ReasonCode(IntakeReasonCode),
    #[error("requires_reason does not match reason_codes")]
    RequiresReason,
}

/// Why a proposal cannot be accepted under its intake report.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IntakeAcceptanceError {
    #[error("intake report is for proposal {report}, not {proposal}")]
    ReportProposalMismatch { proposal: Id, report: Id },
    #[error("proposal {0} requires a recorded ResolutionDecision rationale")]
    ReasonRequired(Id),
    #[error("decision node {0} is not Accepted")]
    DecisionNotAccepted(Id),
    #[error("decision node {0} is not a ResolutionDecision")]
    DecisionWrongType(Id),
    #[error("decision node {decision} is not bound to proposal {proposal}")]
    DecisionProposalMismatch { decision: Id, proposal: Id },
    #[error("decision node {0} has no non-blank rationale")]
    MissingRationale(Id),
}

/// Requires `items` to be strictly increasing under `key`.
fn strictly_sorted<T, K: Ord>(
    collection: &'static str,
    items: &[T],
    key: impl Fn(&T) -> K,
) -> Result<(), IntakeReportError> {
    if items.windows(2).all(|w| key(&w[0]) < key(&w[1])) {
        Ok(())
    } else {
        Err(IntakeReportError::Unordered { collection })
    }
}

fn unit_interval(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

/// Whether a relevant validation finding by itself requires a reason: an unwaived Blocker or
/// Error.
pub(crate) fn finding_requires_reason(finding: &GeneratedFinding) -> bool {
    finding.payload.waiver_ref.is_none()
        && matches!(
            finding.payload.severity,
            FindingSeverity::Blocker | FindingSeverity::Error
        )
}

impl IntakeReport {
    /// Checks hash kinds, score and novelty ranges, every frozen ordering and uniqueness, and
    /// that the content-derivable reason codes and `requires_reason` are consistent.
    pub fn validate(&self) -> Result<(), IntakeReportError> {
        for (field, hash, expected) in [
            (
                "base_semantic_hash",
                &self.base_semantic_hash,
                HashKind::Semantic,
            ),
            (
                "candidate_semantic_hash",
                &self.candidate_semantic_hash,
                HashKind::Semantic,
            ),
            (
                "retrieval_corpus_hash",
                &self.retrieval_corpus_hash,
                HashKind::Generic,
            ),
        ] {
            if hash.kind() != expected {
                return Err(IntakeReportError::HashKind {
                    field,
                    expected,
                    actual: hash.kind(),
                });
            }
        }

        strictly_sorted("covered_by", &self.covered_by, |c| {
            (c.subject_ref.clone(), c.covered_by_ref.clone())
        })?;
        for c in &self.covered_by {
            if c.score != 1.0 {
                return Err(IntakeReportError::CoverageScore {
                    subject_ref: c.subject_ref.clone(),
                    covered_by_ref: c.covered_by_ref.clone(),
                });
            }
        }

        for c in &self.duplicate_candidates {
            if !(c.score.is_finite() && c.score > 0.0) {
                return Err(IntakeReportError::CandidateScore {
                    subject_ref: c.subject_ref.clone(),
                    candidate_ref: c.candidate_ref.clone(),
                });
            }
            if c.subject_ref == c.candidate_ref {
                return Err(IntakeReportError::SelfCandidate(c.subject_ref.clone()));
            }
        }
        let ordered = self.duplicate_candidates.windows(2).all(|w| {
            let (a, b) = (&w[0], &w[1]);
            a.subject_ref
                .cmp(&b.subject_ref)
                .then_with(|| b.score.total_cmp(&a.score))
                .then_with(|| a.candidate_ref.cmp(&b.candidate_ref))
                .is_lt()
        });
        let distinct: BTreeSet<(&Id, &Id)> = self
            .duplicate_candidates
            .iter()
            .map(|c| (&c.subject_ref, &c.candidate_ref))
            .collect();
        if !ordered || distinct.len() != self.duplicate_candidates.len() {
            return Err(IntakeReportError::Unordered {
                collection: "duplicate_candidates",
            });
        }
        let mut per_subject: std::collections::BTreeMap<&Id, usize> = Default::default();
        for c in &self.duplicate_candidates {
            let count = per_subject.entry(&c.subject_ref).or_default();
            *count += 1;
            if *count > INTAKE_DUPLICATE_CANDIDATE_LIMIT {
                return Err(IntakeReportError::CandidateLimit(c.subject_ref.clone()));
            }
        }

        strictly_sorted("refines", &self.refines, |r| r.edge_ref.clone())?;
        strictly_sorted("declared_conflicts", &self.declared_conflicts, |c| {
            c.edge_ref.clone()
        })?;

        strictly_sorted("term_resolutions", &self.term_resolutions, |t| {
            t.term_ref.clone()
        })?;
        for t in &self.term_resolutions {
            strictly_sorted("matched_term_refs", &t.matched_term_refs, Id::clone)?;
            let consistent = match t.status {
                TermResolutionStatus::Unsearchable => {
                    t.key.is_none() && t.matched_term_refs.is_empty()
                }
                TermResolutionStatus::New => t.key.is_some() && t.matched_term_refs.is_empty(),
                TermResolutionStatus::Resolved => t.key.is_some() && t.matched_term_refs.len() == 1,
                TermResolutionStatus::Ambiguous => t.key.is_some() && t.matched_term_refs.len() > 1,
            };
            if !consistent {
                return Err(IntakeReportError::TermResolution(t.term_ref.clone()));
            }
        }

        strictly_sorted("validation_gates", &self.validation_gates, |g| {
            GateId::ALL.iter().position(|x| x == g)
        })?;
        strictly_sorted("validation_findings", &self.validation_findings, |f| {
            f.id.clone()
        })?;

        strictly_sorted("subject_novelty", &self.subject_novelty, |s| {
            s.subject_ref.clone()
        })?;
        for s in &self.subject_novelty {
            let consistent = unit_interval(s.similarity)
                && unit_interval(s.novelty)
                && s.novelty == 1.0 - s.similarity
                && (s.nearest_ref.is_some() || s.similarity == 0.0);
            if !consistent {
                return Err(IntakeReportError::SubjectNovelty(s.subject_ref.clone()));
            }
        }
        let minimum = self
            .subject_novelty
            .iter()
            .map(|s| s.novelty)
            .min_by(f64::total_cmp);
        if self.novelty != minimum {
            return Err(IntakeReportError::Novelty);
        }

        let derived = [
            (
                IntakeReasonCode::ExactEquivalent,
                self.covered_by
                    .iter()
                    .any(|c| c.subject_ref != c.covered_by_ref),
            ),
            (
                IntakeReasonCode::DeclaredConflict,
                !self.declared_conflicts.is_empty(),
            ),
            (
                IntakeReasonCode::ValidationConflict,
                self.validation_findings.iter().any(finding_requires_reason),
            ),
            (
                IntakeReasonCode::AmbiguousTerm,
                self.term_resolutions
                    .iter()
                    .any(|t| t.status == TermResolutionStatus::Ambiguous),
            ),
        ];
        for (code, expected) in derived {
            if self.reason_codes.contains(&code) != expected {
                return Err(IntakeReportError::ReasonCode(code));
            }
        }
        if self.requires_reason == self.reason_codes.is_empty() {
            return Err(IntakeReportError::RequiresReason);
        }
        Ok(())
    }
}
