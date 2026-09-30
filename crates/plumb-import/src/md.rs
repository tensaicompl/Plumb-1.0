//! Markdown import. Classification precedence is exactly: table block, ATX heading, unordered
//! list item, ordered list item, paragraph. No general Markdown parser is used.

use std::sync::OnceLock;

use regex::Regex;

use crate::source::{
    build, decode, lines, lists_and_paragraphs, FragmentKind, ImportAudit, ImportError,
    ImportedSource, Line, Segment, SourceKind,
};
use crate::text::list_item;

fn is_atx_heading(line: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^#{1,6}[ \t]+(.+?)\s*$").expect("valid regex"))
        .is_match(line)
}

fn is_delimiter_cell(cell: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^:?-{3,}:?$").expect("valid regex"))
        .is_match(cell)
}

/// Splits a table line into cells: drop one optional leading and one optional trailing outer
/// `|`, split on `|`, trim ASCII spaces and tabs around each cell, keep empty interior cells.
pub fn table_cells(line: &str) -> Vec<String> {
    let inner = line.strip_prefix('|').unwrap_or(line);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner
        .split('|')
        .map(|cell| cell.trim_matches(|c| c == ' ' || c == '\t').to_owned())
        .collect()
}

/// Marks recognized table blocks as consumed and emits one TableRow per data row.
fn tables(lines: &[Line<'_>], consumed: &mut [bool], segments: &mut Vec<Segment>) {
    let mut index = 0;
    while index < lines.len() {
        let is_candidate = |line: &Line<'_>| !line.is_blank() && line.text.contains('|');
        if !is_candidate(&lines[index]) {
            index += 1;
            continue;
        }
        let start = index;
        while index < lines.len() && is_candidate(&lines[index]) {
            index += 1;
        }
        let block = &lines[start..index];
        let [header, delimiter, data @ ..] = block else {
            continue;
        };
        let delimiter_cells = table_cells(delimiter.text);
        if delimiter_cells.is_empty() || !delimiter_cells.iter().all(|c| is_delimiter_cell(c)) {
            continue;
        }
        let header_cells = table_cells(header.text);
        for flag in &mut consumed[start..index] {
            *flag = true;
        }
        for row in data {
            segments.push(Segment {
                kind: FragmentKind::TableRow,
                start: row.start,
                end: row.end,
                header_cells: Some(header_cells.clone()),
            });
        }
    }
}

fn heading_or_list(line: &str) -> Option<FragmentKind> {
    if is_atx_heading(line) {
        Some(FragmentKind::Heading)
    } else {
        list_item(line)
    }
}

/// Imports a UTF-8 Markdown source.
pub fn import_markdown(
    display_name: &str,
    bytes: &[u8],
    audit: &ImportAudit,
) -> Result<ImportedSource, ImportError> {
    let text = decode(bytes)?;
    let lines = lines(&text);
    let mut consumed = vec![false; lines.len()];
    let mut segments = Vec::new();
    tables(&lines, &mut consumed, &mut segments);
    lists_and_paragraphs(&lines, &consumed, heading_or_list, &mut segments);
    build(
        SourceKind::Markdown,
        display_name,
        bytes,
        text,
        segments,
        audit,
    )
}
