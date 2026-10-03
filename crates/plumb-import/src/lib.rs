//! Source import: deterministic construction of SourceArtifact and EvidenceFragment nodes and
//! the source artifacts from caller-supplied bytes (compiler architecture §6). Importing reads
//! no clock and persists nothing; acquisition stores the returned artifacts.

// Declared first: segment.rs reuses the importer closed-vocabulary macro.
#[macro_use]
pub mod source;

pub mod docx;
mod fallback;
pub mod manifest;
pub mod md;
pub mod segment;
pub mod text;

pub use docx::import_docx;
pub use manifest::{
    build_evidence_manifest, evidence_manifest_artifact, parse_evidence_manifest,
    validate_evidence_manifest, EvidenceManifest, EvidenceManifestFragment, EvidenceManifestSource,
    ManifestError, EVIDENCE_MANIFEST_VERSION,
};
pub use md::{import_markdown, table_cells};
pub use segment::{
    build_segmentation_request, evaluate_segmentation, FallbackReason, SegmentCandidate,
    SegmentClassification, SegmentFlag, SegmentGap, SegmentationAudit, SegmentationContext,
    SegmentationContextFragment, SegmentationError, SegmentationMode, SegmentationRequest,
    SegmentationResult, E_INTAKE_UNCOVERED, SEGMENTATION_CONTEXT_VERSION,
    SEGMENTATION_OUTPUT_VERSION, SEGMENTATION_TASK_KIND,
};
pub use source::{
    source_parse_metadata, ExtractedTextArtifact, FragmentKind, FragmentMetadata, ImportAudit,
    ImportError, ImportWarning, ImportWarningCode, ImportedSource, ParseMetadata, ParseStatus,
    SourceArtifactLinks, SourceKind, UnknownImportValue, EXTRACTED_TEXT_MEDIA_TYPE,
    EXTRACTED_TEXT_VERSION, E_DOCX_ARCHIVE, E_DOCX_LIMIT, E_DOCX_MISSING_PART, E_DOCX_XML,
    E_SOURCE_ENCODING, FRAGMENT_EXTENSION, PARSE_EXTENSION, SOURCE_ARTIFACTS_EXTENSION,
};
pub use text::import_plain_text;
