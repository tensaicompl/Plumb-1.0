//! The evidence-import contract shared by every importer: vocabularies, the extracted-text
//! artifact, importer node extensions, and deterministic construction of the SourceArtifact and
//! EvidenceFragment nodes (compiler architecture §6, metamodel §§5.1-5.2).
//!
//! Construction is a pure function of its inputs. Nothing here reads a clock or persists
//! anything: the returned artifact records are persisted later by acquisition.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use plumb_artifacts::{Artifact, ArtifactKind};
use plumb_core::{to_canonical_json, CoreError, Hash, HashKind, Id, Timestamp};
use plumb_psg::{
    evidence_fragment_id, source_artifact_id, AuditMeta, ElementStatus, EvidenceFragment,
    EvidenceLocator, ExtensionKey, Node, NodeError, NodePayload, SourceArtifact,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use thiserror::Error;

/// The stable code of a source that is not valid UTF-8.
pub const E_SOURCE_ENCODING: &str = "E_SOURCE_ENCODING";

/// The version of [`ExtractedTextArtifact`].
pub const EXTRACTED_TEXT_VERSION: u32 = 1;

/// Media type of the extracted-text artifact.
pub const EXTRACTED_TEXT_MEDIA_TYPE: &str = "application/json";

/// Extension key of the SourceArtifact node's [`SourceArtifactLinks`].
pub const SOURCE_ARTIFACTS_EXTENSION: &str = "plumb_import:source_artifacts";

/// Extension key of an EvidenceFragment node's [`FragmentMetadata`].
pub const FRAGMENT_EXTENSION: &str = "plumb_import:fragment";

/// Why a source could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImportError {
    /// The bytes are not valid UTF-8.
    #[error("source is not valid UTF-8 (valid up to byte {valid_up_to})")]
    SourceEncoding {
        valid_up_to: usize,
        error_len: Option<usize>,
    },
    #[error("invalid display name {0:?}")]
    InvalidDisplayName(String),
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("imported node is invalid: {0}")]
    InvalidNode(String),
    #[error("invalid import metadata: {0}")]
    InvalidMetadata(String),
}

impl ImportError {
    /// The stable error code, where the import contract defines one.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            ImportError::SourceEncoding { .. } => Some(E_SOURCE_ENCODING),
            _ => None,
        }
    }
}

impl From<NodeError> for ImportError {
    fn from(error: NodeError) -> Self {
        ImportError::InvalidNode(error.to_string())
    }
}

/// A string that is not a value of the named import vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown {vocabulary} value {value:?}")]
pub struct UnknownImportValue {
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
            type Err = UnknownImportValue;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                $name::ALL
                    .into_iter()
                    .find(|value| value.as_str() == s)
                    .ok_or_else(|| UnknownImportValue {
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
    /// The kind of an imported source (`SourceArtifact.source_kind`).
    SourceKind, 3 {
        Markdown => "markdown",
        PlainText => "plain_text",
        Docx => "docx",
    }
}

impl SourceKind {
    /// The media type of the original source bytes.
    pub fn media_type(self) -> &'static str {
        match self {
            SourceKind::Markdown => "text/markdown",
            SourceKind::PlainText => "text/plain",
            SourceKind::Docx => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            }
        }
    }
}

closed_vocabulary! {
    /// The structural kind of an evidence fragment.
    FragmentKind, 4 {
        Heading => "heading",
        ListItem => "list_item",
        TableRow => "table_row",
        Paragraph => "paragraph",
    }
}

/// The source-extracted artifact: the normalized text, wrapped so that its bytes never equal
/// the original source bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "ExtractedTextFields")]
pub struct ExtractedTextArtifact {
    pub version: u32,
    pub text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractedTextFields {
    version: u32,
    text: String,
}

impl TryFrom<ExtractedTextFields> for ExtractedTextArtifact {
    type Error = ImportError;

    fn try_from(f: ExtractedTextFields) -> Result<Self, Self::Error> {
        let artifact = ExtractedTextArtifact {
            version: f.version,
            text: f.text,
        };
        artifact.validate()?;
        Ok(artifact)
    }
}

impl ExtractedTextArtifact {
    pub fn new(text: String) -> ExtractedTextArtifact {
        ExtractedTextArtifact {
            version: EXTRACTED_TEXT_VERSION,
            text,
        }
    }

    pub fn validate(&self) -> Result<(), ImportError> {
        if self.version == EXTRACTED_TEXT_VERSION {
            Ok(())
        } else {
            Err(ImportError::InvalidMetadata(format!(
                "extracted text version {} is not {EXTRACTED_TEXT_VERSION}",
                self.version
            )))
        }
    }

    /// The persisted bytes: RFC 8785 canonical JSON.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CoreError> {
        to_canonical_json(self)
    }
}

/// The artifacts behind a SourceArtifact node (`plumb_import:source_artifacts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "SourceArtifactLinksFields")]
pub struct SourceArtifactLinks {
    pub original_ref: Hash,
    pub extracted_ref: Hash,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceArtifactLinksFields {
    original_ref: Hash,
    extracted_ref: Hash,
}

impl TryFrom<SourceArtifactLinksFields> for SourceArtifactLinks {
    type Error = ImportError;

    fn try_from(f: SourceArtifactLinksFields) -> Result<Self, Self::Error> {
        let links = SourceArtifactLinks {
            original_ref: f.original_ref,
            extracted_ref: f.extracted_ref,
        };
        links.validate()?;
        Ok(links)
    }
}

impl SourceArtifactLinks {
    pub fn validate(&self) -> Result<(), ImportError> {
        require_generic("original_ref", &self.original_ref)?;
        require_generic("extracted_ref", &self.extracted_ref)
    }
}

/// Importer metadata of an EvidenceFragment node (`plumb_import:fragment`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "FragmentMetadataFields")]
pub struct FragmentMetadata {
    pub kind: FragmentKind,
    pub extracted_ref: Hash,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_cells: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FragmentMetadataFields {
    kind: FragmentKind,
    extracted_ref: Hash,
    #[serde(default)]
    header_cells: Option<Vec<String>>,
}

impl TryFrom<FragmentMetadataFields> for FragmentMetadata {
    type Error = ImportError;

    fn try_from(f: FragmentMetadataFields) -> Result<Self, Self::Error> {
        let metadata = FragmentMetadata {
            kind: f.kind,
            extracted_ref: f.extracted_ref,
            header_cells: f.header_cells,
        };
        metadata.validate()?;
        Ok(metadata)
    }
}

impl FragmentMetadata {
    /// A TableRow carries header cells; every other kind carries none.
    pub fn validate(&self) -> Result<(), ImportError> {
        require_generic("extracted_ref", &self.extracted_ref)?;
        if (self.kind == FragmentKind::TableRow) != self.header_cells.is_some() {
            return Err(ImportError::InvalidMetadata(format!(
                "{} fragment with{} header cells",
                self.kind,
                if self.header_cells.is_some() {
                    ""
                } else {
                    "out"
                }
            )));
        }
        Ok(())
    }
}

fn require_generic(what: &str, hash: &Hash) -> Result<(), ImportError> {
    if hash.kind() == HashKind::Generic {
        Ok(())
    } else {
        Err(ImportError::InvalidMetadata(format!(
            "{what} {hash} is not a Generic hash"
        )))
    }
}

/// Who imported the source and when, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// The result of importing one source. Nothing has been persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedSource {
    pub source: Node,
    pub fragments: Vec<Node>,

    pub original_artifact: Artifact,
    pub extracted_artifact: Artifact,
}

// ============================================================================ text model

/// One logical line of the normalized text: byte offsets exclude the line's LF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Line<'t> {
    pub start: usize,
    pub end: usize,
    pub text: &'t str,
}

impl Line<'_> {
    /// Blank means only ASCII spaces and tabs.
    pub fn is_blank(&self) -> bool {
        self.text.bytes().all(|b| b == b' ' || b == b'\t')
    }
}

/// A classified byte range of the normalized text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    pub kind: FragmentKind,
    pub start: usize,
    pub end: usize,
    pub header_cells: Option<Vec<String>>,
}

/// Decodes UTF-8 without loss and normalizes CRLF and lone CR to LF, nothing else.
pub(crate) fn decode(bytes: &[u8]) -> Result<String, ImportError> {
    let text = std::str::from_utf8(bytes).map_err(|e| ImportError::SourceEncoding {
        valid_up_to: e.valid_up_to(),
        error_len: e.error_len(),
    })?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Splits normalized text into lines; the empty line after a trailing LF is not a line.
pub(crate) fn lines(text: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            lines.push(Line {
                start,
                end: index,
                text: &text[start..index],
            });
            start = index + 1;
        }
    }
    if start < text.len() {
        lines.push(Line {
            start,
            end: text.len(),
            text: &text[start..],
        });
    }
    lines
}

/// Classifies each line not yet consumed as a list item (when `list` matches) or as part of a
/// paragraph, appending the segments in source order.
pub(crate) fn lists_and_paragraphs(
    lines: &[Line<'_>],
    consumed: &[bool],
    single_line: impl Fn(&str) -> Option<FragmentKind>,
    segments: &mut Vec<Segment>,
) {
    let mut paragraph: Option<(usize, usize)> = None;
    let close = |paragraph: &mut Option<(usize, usize)>, segments: &mut Vec<Segment>| {
        if let Some((start, end)) = paragraph.take() {
            segments.push(Segment {
                kind: FragmentKind::Paragraph,
                start,
                end,
                header_cells: None,
            });
        }
    };
    for (index, line) in lines.iter().enumerate() {
        if consumed[index] || line.is_blank() {
            close(&mut paragraph, segments);
            continue;
        }
        if let Some(kind) = single_line(line.text) {
            close(&mut paragraph, segments);
            segments.push(Segment {
                kind,
                start: line.start,
                end: line.end,
                header_cells: None,
            });
            continue;
        }
        paragraph = Some(match paragraph {
            Some((start, _)) => (start, line.end),
            None => (line.start, line.end),
        });
    }
    close(&mut paragraph, segments);
}

// ============================================================================ node construction

fn validate_display_name(display_name: &str) -> Result<(), ImportError> {
    if !display_name.is_empty()
        && display_name.trim() == display_name
        && !display_name.chars().any(char::is_control)
    {
        Ok(())
    } else {
        Err(ImportError::InvalidDisplayName(display_name.to_owned()))
    }
}

fn extension(
    key: &str,
    value: &impl Serialize,
) -> Result<BTreeMap<ExtensionKey, Value>, ImportError> {
    let key: ExtensionKey = key
        .parse()
        .map_err(|e: plumb_psg::InvalidExtensionKey| ImportError::InvalidMetadata(e.to_string()))?;
    let value =
        serde_json::to_value(value).map_err(|e| ImportError::InvalidMetadata(e.to_string()))?;
    Ok(BTreeMap::from([(key, value)]))
}

fn envelope(
    id: Id,
    payload: NodePayload,
    extensions: BTreeMap<ExtensionKey, Value>,
    audit: &ImportAudit,
) -> Result<Node, ImportError> {
    let node = Node {
        id,
        revision: 1,
        status: ElementStatus::Accepted,
        payload,
        evidence: Vec::new(),
        derivations: Vec::new(),
        standards: Vec::new(),
        tags: BTreeSet::new(),
        extensions,
        audit: AuditMeta {
            created_by: audit.created_by.clone(),
            created_at: audit.created_at,
            updated_by: None,
            updated_at: None,
        },
    };
    node.validate()?;
    Ok(node)
}

/// Builds the ImportedSource of `bytes` from the classified segments of its normalized text.
pub(crate) fn build(
    kind: SourceKind,
    display_name: &str,
    bytes: &[u8],
    text: String,
    mut segments: Vec<Segment>,
    audit: &ImportAudit,
) -> Result<ImportedSource, ImportError> {
    validate_display_name(display_name)?;
    let raw_hash = Hash::content_sha256(bytes);
    let extracted = ExtractedTextArtifact::new(text);
    let extracted_bytes = extracted.canonical_bytes()?;
    let extracted_hash = Hash::content_sha256(&extracted_bytes);

    let original_artifact = Artifact {
        hash: raw_hash.clone(),
        kind: ArtifactKind::SourceOriginal,
        media_type: kind.media_type().to_owned(),
        bytes: bytes.to_vec(),
        created_at: audit.created_at,
    };
    let extracted_artifact = Artifact {
        hash: extracted_hash.clone(),
        kind: ArtifactKind::SourceExtracted,
        media_type: EXTRACTED_TEXT_MEDIA_TYPE.to_owned(),
        bytes: extracted_bytes,
        created_at: audit.created_at,
    };

    let source_id = source_artifact_id(&raw_hash)?;
    let links = SourceArtifactLinks {
        original_ref: raw_hash.clone(),
        extracted_ref: extracted_hash.clone(),
    };
    let source = envelope(
        source_id.clone(),
        NodePayload::SourceArtifact(SourceArtifact {
            source_kind: kind.as_str().to_owned(),
            display_name: display_name.to_owned(),
            content_hash: raw_hash,
            media_type: kind.media_type().to_owned(),
            external_uri: None,
            external_version: None,
            producer: None,
            created_at_source: None,
            language: None,
            classification: None,
        }),
        extension(SOURCE_ARTIFACTS_EXTENSION, &links)?,
        audit,
    )?;

    segments.sort_by_key(|segment| segment.start);
    let mut fragments = Vec::with_capacity(segments.len());
    for segment in segments {
        let fragment_text = &extracted.text[segment.start..segment.end];
        let locator = EvidenceLocator::TextRange {
            start: segment.start as u64,
            end: segment.end as u64,
        };
        let metadata = FragmentMetadata {
            kind: segment.kind,
            extracted_ref: extracted_hash.clone(),
            header_cells: segment.header_cells,
        };
        metadata.validate()?;
        fragments.push(envelope(
            evidence_fragment_id(&source_id, &locator)?,
            NodePayload::EvidenceFragment(EvidenceFragment {
                source_ref: source_id.clone(),
                locator,
                content_hash: Hash::content_sha256(fragment_text.as_bytes()),
                extracted_text: Some(fragment_text.to_owned()),
                speaker: None,
                source_timestamp: None,
            }),
            extension(FRAGMENT_EXTENSION, &metadata)?,
            audit,
        )?);
    }
    Ok(ImportedSource {
        source,
        fragments,
        original_artifact,
        extracted_artifact,
    })
}
