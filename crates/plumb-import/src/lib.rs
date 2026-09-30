//! Source import: deterministic construction of SourceArtifact and EvidenceFragment nodes and
//! the source artifacts from caller-supplied bytes (compiler architecture §6). Importing reads
//! no clock and persists nothing; acquisition stores the returned artifacts.

pub mod md;
pub mod source;
pub mod text;

pub use md::{import_markdown, table_cells};
pub use source::{
    ExtractedTextArtifact, FragmentKind, FragmentMetadata, ImportAudit, ImportError,
    ImportedSource, SourceArtifactLinks, SourceKind, UnknownImportValue, EXTRACTED_TEXT_MEDIA_TYPE,
    EXTRACTED_TEXT_VERSION, E_SOURCE_ENCODING, FRAGMENT_EXTENSION, SOURCE_ARTIFACTS_EXTENSION,
};
pub use text::import_plain_text;
