//! Plain-text import: ordered list items, unordered list items and paragraphs only. There is
//! no heading or table inference.

use std::sync::OnceLock;

use regex::Regex;

use crate::source::{
    build, decode, lines, lists_and_paragraphs, FragmentKind, ImportAudit, ImportError,
    ImportedSource, SourceKind,
};

/// An unordered list line.
pub(crate) fn is_unordered_item(line: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]*[-+*][ \t]+(.+)$").expect("valid regex"))
        .is_match(line)
}

/// An ordered list line.
pub(crate) fn is_ordered_item(line: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]*[0-9]+[.)][ \t]+(.+)$").expect("valid regex"))
        .is_match(line)
}

/// A list line of either kind.
pub(crate) fn list_item(line: &str) -> Option<FragmentKind> {
    (is_unordered_item(line) || is_ordered_item(line)).then_some(FragmentKind::ListItem)
}

/// Imports a UTF-8 plain-text source.
pub fn import_plain_text(
    display_name: &str,
    bytes: &[u8],
    audit: &ImportAudit,
) -> Result<ImportedSource, ImportError> {
    let text = decode(bytes)?;
    let lines = lines(&text);
    let consumed = vec![false; lines.len()];
    let mut segments = Vec::new();
    lists_and_paragraphs(&lines, &consumed, list_item, &mut segments);
    build(
        SourceKind::PlainText,
        display_name,
        bytes,
        text,
        segments,
        audit,
    )
}
