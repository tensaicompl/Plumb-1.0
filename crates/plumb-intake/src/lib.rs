//! Plumb intake. X1.2 provides deterministic lexical BM25 retrieval over the accepted PSG
//! (Hotfix 058); X1.3 provides patch-centric intake analysis of immutable proposals and the
//! acceptance-rationale guard (Hotfix 059).

mod bm25;
mod index;
mod intake;
mod report;

pub use bm25::{tokenize, BM25_B, BM25_K1};
pub use index::{
    IndexError, IndexUpdateSummary, LexicalIndex, RetrievalDocumentKind, RetrievalHit,
    RETRIEVAL_INDEX_VERSION,
};
pub use intake::{
    intake, is_validation_conflict, validate_required_rationale, IntakeError, IntakeValidationInput,
};
pub use report::{
    CoverageMatch, DeclaredConflict, DuplicateCandidate, IntakeAcceptanceError, IntakeReasonCode,
    IntakeReport, IntakeReportError, RefinementLink, SubjectNovelty, TermResolution,
    TermResolutionStatus, INTAKE_DUPLICATE_CANDIDATE_LIMIT,
};
