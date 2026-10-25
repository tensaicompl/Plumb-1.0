//! Deterministic minimal DOCX import (plan S0.2). The visible evidence surface is the main body
//! of `word/document.xml`; `word/styles.xml` supplies optional heading and list properties.
//!
//! Everything happens in memory under fixed safety limits. Paragraphs and table rows become
//! extracted-text records joined by single LFs; visible text in structures outside the import
//! profile is flattened and reported, never dropped.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use plumb_psg::EvidenceLocator;
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::NsReader;
use zip::result::ZipError;
use zip::ZipArchive;

use crate::source::{
    build_with_parse, check_fragment_count, normalize_text, push_segment, FragmentKind,
    ImportAudit, ImportError, ImportWarning, ImportWarningCode, ImportedSource, ParseMetadata,
    Segment, SourceKind,
};

/// Largest accepted DOCX input.
pub const DOCX_MAX_INPUT_BYTES: u64 = 128 * 1024 * 1024;
/// Most entries accepted in a DOCX package.
pub const DOCX_MAX_ENTRIES: usize = 4096;
/// Largest accepted sum of declared uncompressed entry sizes.
pub const DOCX_MAX_TOTAL_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
/// Largest XML part actually read.
pub const DOCX_MAX_XML_PART_BYTES: u64 = 32 * 1024 * 1024;
/// Deepest open-element nesting accepted while parsing one XML part.
pub const DOCX_MAX_XML_DEPTH: usize = 256;
/// Largest accepted `w:gridSpan` value and summed span of one table row.
pub const DOCX_MAX_TABLE_COLUMNS: usize = 64;
/// Most rows accepted in one table.
pub const DOCX_MAX_TABLE_ROWS: usize = 10000;

/// The main document part.
pub const DOCX_DOCUMENT_PART: &str = "word/document.xml";
/// The optional style definitions part.
pub const DOCX_STYLES_PART: &str = "word/styles.xml";

/// The exact message of a flattened unsupported structure.
pub const DOCX_UNSUPPORTED_VISIBLE_MESSAGE: &str =
    "Visible DOCX content was flattened because its structure is outside the S0.2 import profile.";

/// The WordprocessingML namespaces: transitional and strict.
const WORDPROCESSINGML: [&str; 2] = [
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "http://purl.oclc.org/ooxml/wordprocessingml/main",
];

/// The markup-compatibility namespace of `mc:AlternateContent`.
const MARKUP_COMPATIBILITY: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// Longest `basedOn` chain followed.
const MAX_STYLE_DEPTH: usize = 32;

// ============================================================================ limits

fn limit(limit: &str, actual: u64, max: u64) -> Result<(), ImportError> {
    if actual <= max {
        Ok(())
    } else {
        Err(ImportError::DocxLimit {
            limit: limit.to_owned(),
            actual,
            max,
        })
    }
}

/// Checks the input size.
pub fn check_input_size(bytes: u64) -> Result<(), ImportError> {
    limit("input bytes", bytes, DOCX_MAX_INPUT_BYTES)
}

/// Checks the entry count.
pub fn check_entry_count(entries: usize) -> Result<(), ImportError> {
    limit("entry count", entries as u64, DOCX_MAX_ENTRIES as u64)
}

/// Checks the sum of declared uncompressed entry sizes.
pub fn check_total_uncompressed(total: u64) -> Result<(), ImportError> {
    limit(
        "total uncompressed bytes",
        total,
        DOCX_MAX_TOTAL_UNCOMPRESSED_BYTES,
    )
}

/// Checks the size of one XML part as read.
pub fn check_xml_part_size(bytes: u64) -> Result<(), ImportError> {
    limit("XML part bytes", bytes, DOCX_MAX_XML_PART_BYTES)
}

/// Checks the nesting depth of an element about to be added to the tree.
fn check_xml_depth(depth: usize) -> Result<(), ImportError> {
    limit("XML depth", depth as u64, DOCX_MAX_XML_DEPTH as u64)
}

/// Checks a `w:gridSpan` value or the summed span of one table row.
fn check_table_columns(columns: u64) -> Result<(), ImportError> {
    limit("table columns", columns, DOCX_MAX_TABLE_COLUMNS as u64)
}

/// Checks the number of a table row about to be read.
fn check_table_rows(rows: usize) -> Result<(), ImportError> {
    limit("table rows", rows as u64, DOCX_MAX_TABLE_ROWS as u64)
}

// ============================================================================ package

fn archive_error(error: ZipError) -> ImportError {
    ImportError::DocxArchive {
        reason: error.to_string(),
    }
}

/// Opens the package and checks the package-level limits.
fn open(bytes: &[u8]) -> Result<ZipArchive<Cursor<&[u8]>>, ImportError> {
    check_input_size(bytes.len() as u64)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(archive_error)?;
    check_entry_count(archive.len())?;
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(archive_error)?;
        total = total.saturating_add(entry.size());
    }
    check_total_uncompressed(total)?;
    Ok(archive)
}

/// Reads one part with a bounded read; `None` when the part is absent.
fn read_part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    part: &str,
) -> Result<Option<String>, ImportError> {
    let entry = match archive.by_name(part) {
        Ok(entry) => entry,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(archive_error(error)),
    };
    let mut bytes = Vec::new();
    entry
        .take(DOCX_MAX_XML_PART_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ImportError::DocxArchive {
            reason: e.to_string(),
        })?;
    check_xml_part_size(bytes.len() as u64)?;
    let text = String::from_utf8(bytes).map_err(|e| xml_error(part, e.to_string()))?;
    Ok(Some(text))
}

fn xml_error(part: &str, reason: impl Into<String>) -> ImportError {
    ImportError::DocxXml {
        part: part.to_owned(),
        reason: reason.into(),
    }
}

// ============================================================================ XML tree

/// An element: `Some(local)` in a WordprocessingML namespace, `None` otherwise.
#[derive(Debug)]
struct Element {
    word: Option<String>,
    /// `Some(local)` in the markup-compatibility namespace.
    compat: Option<String>,
    /// WordprocessingML attributes by local name.
    attributes: BTreeMap<String, String>,
    children: Vec<Content>,
}

#[derive(Debug)]
enum Content {
    Element(Element),
    Text(String),
}

impl Element {
    fn is(&self, local: &str) -> bool {
        self.word.as_deref() == Some(local)
    }

    fn is_compat(&self, local: &str) -> bool {
        self.compat.as_deref() == Some(local)
    }

    fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Content::Element(e) => Some(e),
            Content::Text(_) => None,
        })
    }

    fn child(&self, local: &str) -> Option<&Element> {
        self.elements().find(|e| e.is(local))
    }

    fn attribute(&self, local: &str) -> Option<&str> {
        self.attributes.get(local).map(String::as_str)
    }

    /// Concatenated character data of this element.
    fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|c| match c {
                Content::Text(t) => Some(t.as_str()),
                Content::Element(_) => None,
            })
            .collect()
    }
}

fn is_wordprocessingml(namespace: &ResolveResult<'_>) -> bool {
    match namespace {
        ResolveResult::Bound(ns) => WORDPROCESSINGML
            .iter()
            .any(|uri| ns.as_ref() == uri.as_bytes()),
        _ => false,
    }
}

/// Parses `xml` into a namespace-resolved element tree. An element is checked against
/// [`DOCX_MAX_XML_DEPTH`] before it is added to the tree.
fn parse_xml(part: &str, xml: &str) -> Result<Element, ImportError> {
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Element> = vec![Element {
        word: None,
        compat: None,
        attributes: BTreeMap::new(),
        children: Vec::new(),
    }];
    loop {
        let event = reader
            .read_event()
            .map_err(|e| xml_error(part, e.to_string()))?;
        match event {
            Event::Start(start) => {
                // The stack holds a synthetic root, so its length is the new element's depth.
                check_xml_depth(stack.len())?;
                let element = open_element(part, &reader, &start)?;
                stack.push(element);
            }
            Event::Empty(start) => {
                check_xml_depth(stack.len())?;
                let element = open_element(part, &reader, &start)?;
                push_child(&mut stack, Content::Element(element));
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err(xml_error(part, "unbalanced end tag"));
                }
                if let Some(element) = stack.pop() {
                    push_child(&mut stack, Content::Element(element));
                }
            }
            Event::Text(text) => {
                let text = text.decode().map_err(|e| xml_error(part, e.to_string()))?;
                push_text(&mut stack, &text);
            }
            Event::GeneralRef(reference) => {
                // Entity and character references are separate events; unescaping the reference
                // alone resolves exactly the predefined entities and character references.
                let name = reference
                    .decode()
                    .map_err(|e| xml_error(part, e.to_string()))?;
                let escaped = format!("&{name};");
                let text = unescape(&escaped).map_err(|e| xml_error(part, e.to_string()))?;
                push_text(&mut stack, &text);
            }
            Event::CData(data) => {
                let text = String::from_utf8(data.into_inner().into_owned())
                    .map_err(|e| xml_error(part, e.to_string()))?;
                push_text(&mut stack, &text);
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
        }
    }
    let [mut root] = <[Element; 1]>::try_from(stack)
        .map_err(|_| xml_error(part, "unclosed element at end of document"))?;
    let mut elements = std::mem::take(&mut root.children)
        .into_iter()
        .filter_map(|c| match c {
            Content::Element(e) => Some(e),
            Content::Text(_) => None,
        });
    match (elements.next(), elements.next()) {
        (Some(document), None) => Ok(document),
        _ => Err(xml_error(part, "expected exactly one root element")),
    }
}

fn open_element(
    part: &str,
    reader: &NsReader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
) -> Result<Element, ImportError> {
    let (namespace, _) = reader.resolver().resolve_element(start.name());
    let local = || String::from_utf8_lossy(start.local_name().as_ref()).into_owned();
    let word = is_wordprocessingml(&namespace).then(local);
    let compat = matches!(&namespace, ResolveResult::Bound(ns) if ns.as_ref() == MARKUP_COMPATIBILITY.as_bytes())
        .then(local);
    let mut attributes = BTreeMap::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| xml_error(part, e.to_string()))?;
        let (namespace, local) = reader.resolver().resolve_attribute(attribute.key);
        if is_wordprocessingml(&namespace) {
            let value = reader
                .decoder()
                .decode(&attribute.value)
                .map_err(|e| xml_error(part, e.to_string()))?;
            let value = unescape(&value).map_err(|e| xml_error(part, e.to_string()))?;
            attributes.insert(
                String::from_utf8_lossy(local.as_ref()).into_owned(),
                value.into_owned(),
            );
        }
    }
    Ok(Element {
        word,
        compat,
        attributes,
        children: Vec::new(),
    })
}

fn push_child(stack: &mut [Element], content: Content) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(content);
    }
}

/// Appends character data, merging it into an immediately preceding text child so that text
/// split around references forms one text node.
fn push_text(stack: &mut [Element], text: &str) {
    if let Some(parent) = stack.last_mut() {
        match parent.children.last_mut() {
            Some(Content::Text(previous)) => previous.push_str(text),
            _ => parent.children.push(Content::Text(text.to_owned())),
        }
    }
}

// ============================================================================ styles

/// Paragraph style properties that matter for classification.
#[derive(Debug, Default)]
struct Style {
    based_on: Option<String>,
    outline_level: Option<u32>,
    /// The numbering its own `w:numPr` sets: `Some(false)` when it removes numbering.
    numbered: Option<bool>,
}

type Styles = BTreeMap<String, Style>;

fn outline_level(part: &str, properties: Option<&Element>) -> Result<Option<u32>, ImportError> {
    let Some(level) = properties
        .and_then(|p| p.child("outlineLvl"))
        .map(|e| e.attribute("val").unwrap_or_default())
    else {
        return Ok(None);
    };
    level
        .parse::<u32>()
        .map(Some)
        .map_err(|_| xml_error(part, format!("invalid outlineLvl {level:?}")))
}

/// The numbering a `w:pPr` sets: `None` without `w:numPr`; otherwise numbered exactly when the
/// `w:numPr` has a `w:numId` other than 0 (ECMA-376 §17.9.18: numId 0, or none, removes
/// numbering, including numbering inherited from the style chain).
fn numbering(properties: Option<&Element>) -> Option<bool> {
    let numbering = properties?.child("numPr")?;
    Some(
        numbering
            .child("numId")
            .and_then(|n| n.attribute("val"))
            .is_some_and(|id| id.trim() != "0"),
    )
}

fn parse_styles(xml: &str) -> Result<Styles, ImportError> {
    let root = parse_xml(DOCX_STYLES_PART, xml)?;
    if !root.is("styles") {
        return Err(xml_error(DOCX_STYLES_PART, "root is not w:styles"));
    }
    let mut styles = Styles::new();
    for style in root.elements().filter(|e| e.is("style")) {
        if style.attribute("type").unwrap_or("paragraph") != "paragraph" {
            continue;
        }
        let Some(id) = style.attribute("styleId") else {
            continue;
        };
        let properties = style.child("pPr");
        styles.insert(
            id.to_owned(),
            Style {
                based_on: style
                    .child("basedOn")
                    .and_then(|b| b.attribute("val"))
                    .map(str::to_owned),
                outline_level: outline_level(DOCX_STYLES_PART, properties)?,
                numbered: numbering(properties),
            },
        );
    }
    // Every chain must end within the depth limit, cycles included.
    for id in styles.keys() {
        style_chain(&styles, id)?;
    }
    Ok(styles)
}

/// The styles of `id`'s basedOn chain, nearest first.
fn style_chain<'s>(styles: &'s Styles, id: &str) -> Result<Vec<&'s Style>, ImportError> {
    let mut chain = Vec::new();
    let mut next = Some(id.to_owned());
    while let Some(id) = next {
        let Some(style) = styles.get(&id) else {
            break;
        };
        if chain.len() == MAX_STYLE_DEPTH {
            return Err(xml_error(
                DOCX_STYLES_PART,
                format!("style chain from {id:?} is cyclic or deeper than {MAX_STYLE_DEPTH}"),
            ));
        }
        chain.push(style);
        next = style.based_on.clone();
    }
    Ok(chain)
}

fn classify(paragraph: &Element, styles: &Styles) -> Result<FragmentKind, ImportError> {
    let properties = paragraph.child("pPr");
    let style_id = properties
        .and_then(|p| p.child("pStyle"))
        .and_then(|s| s.attribute("val"));
    let chain = match style_id {
        Some(id) => style_chain(styles, id)?,
        None => Vec::new(),
    };
    let level = match outline_level(DOCX_DOCUMENT_PART, properties)? {
        Some(level) => Some(level),
        None => chain.iter().find_map(|s| s.outline_level),
    };
    let heading = match level {
        Some(level) => level <= 5,
        None => style_id.is_some_and(|id| {
            matches!(
                id,
                "Heading1" | "Heading2" | "Heading3" | "Heading4" | "Heading5" | "Heading6"
            )
        }),
    };
    if heading {
        return Ok(FragmentKind::Heading);
    }
    // The nearest numPr decides: the paragraph's own, then the style chain's.
    let numbered = numbering(properties)
        .or_else(|| chain.iter().find_map(|s| s.numbered))
        .unwrap_or(false);
    Ok(if numbered {
        FragmentKind::ListItem
    } else {
        FragmentKind::Paragraph
    })
}

// ============================================================================ visible text

/// Collects visible text and whether any unsupported structure contributed to it.
struct Visible {
    text: String,
    unsupported: bool,
}

impl Visible {
    fn new() -> Visible {
        Visible {
            text: String::new(),
            unsupported: false,
        }
    }
}

/// Visible text of any subtree: w:t text, w:tab as TAB, w:br and w:cr as LF; deleted,
/// move-from and field-instruction content excluded.
fn flatten(element: &Element, out: &mut String) {
    if element.is("del")
        || element.is("moveFrom")
        || element.is("instrText")
        || element.is("delText")
    {
        return;
    }
    if element.is_compat("AlternateContent") {
        // One alternative only: the first mc:Choice, or mc:Fallback when there is no Choice.
        let alternative = element
            .elements()
            .find(|e| e.is_compat("Choice"))
            .or_else(|| element.elements().find(|e| e.is_compat("Fallback")));
        if let Some(alternative) = alternative {
            flatten(alternative, out);
        }
        return;
    }
    if element.is("t") {
        out.push_str(&element.text());
        return;
    }
    if element.is("tab") {
        out.push('\t');
        return;
    }
    if element.is("br") || element.is("cr") {
        out.push('\n');
        return;
    }
    for child in element.elements() {
        flatten(child, out);
    }
}

/// Flattens an unsupported structure; returns whether it had visible text.
fn flatten_unsupported(element: &Element, visible: &mut Visible) {
    let mut text = String::new();
    flatten(element, &mut text);
    if !text.is_empty() {
        visible.text.push_str(&text);
        visible.unsupported = true;
    }
}

/// Run content: text, tabs and breaks; anything else with visible text is unsupported.
fn run_text(run: &Element, visible: &mut Visible) {
    for child in run.elements() {
        match child.word.as_deref() {
            Some("t") => visible.text.push_str(&child.text()),
            Some("tab") => visible.text.push('\t'),
            Some("br" | "cr") => visible.text.push('\n'),
            Some("rPr" | "instrText" | "delText") => {}
            _ => flatten_unsupported(child, visible),
        }
    }
}

/// Paragraph content: runs, including inserted and moved-to runs.
fn paragraph_content(container: &Element, visible: &mut Visible) {
    for child in container.elements() {
        match child.word.as_deref() {
            Some("pPr" | "del" | "moveFrom") => {}
            Some("r") => run_text(child, visible),
            Some("ins" | "moveTo") => paragraph_content(child, visible),
            _ => flatten_unsupported(child, visible),
        }
    }
}

fn paragraph_text(paragraph: &Element) -> Visible {
    let mut visible = Visible::new();
    paragraph_content(paragraph, &mut visible);
    visible
}

// ============================================================================ document

/// Whether record text is evidence: anything but whitespace only. Whitespace-only text stays
/// in the extracted artifact without a fragment.
fn yields_fragment(text: &str) -> bool {
    !text.chars().all(char::is_whitespace)
}

/// One extracted-text record and the fragment it yields, if any.
struct Record {
    text: String,
    fragment: Option<(FragmentKind, Option<Vec<String>>)>,
}

struct Importer<'s> {
    styles: &'s Styles,
    records: Vec<Record>,
    warnings: Vec<ImportWarning>,
    /// EvidenceFragments the records yield so far.
    fragments: usize,
}

impl Importer<'_> {
    /// Appends a record with its text normalised like Markdown and plain text (line endings,
    /// NFC), checking the fragment budget before a fragment-yielding record is added. A record
    /// yields a fragment exactly when it has a kind and text that is not only whitespace.
    fn push_record(&mut self, mut record: Record) -> Result<(), ImportError> {
        record.text = normalize_text(&record.text);
        if !yields_fragment(&record.text) {
            record.fragment = None;
        }
        if record.fragment.is_some() {
            check_fragment_count(self.fragments + 1)?;
            self.fragments += 1;
        }
        self.records.push(record);
        Ok(())
    }

    fn warn(&mut self, xpath: String) {
        self.warnings.push(ImportWarning {
            code: ImportWarningCode::DocxUnsupportedVisible,
            locator: EvidenceLocator::XmlPath { xpath },
            message: DOCX_UNSUPPORTED_VISIBLE_MESSAGE.to_owned(),
        });
    }

    fn body(&mut self, body: &Element) -> Result<(), ImportError> {
        for (index, child) in body.elements().enumerate() {
            let path = format!("/w:document/w:body/*[{}]", index + 1);
            match child.word.as_deref() {
                Some("p") => self.paragraph(child, &path)?,
                Some("tbl") => self.table(child, &path)?,
                _ => {
                    let mut visible = Visible::new();
                    flatten_unsupported(child, &mut visible);
                    if visible.unsupported {
                        self.warn(path);
                        self.push_record(Record {
                            text: visible.text,
                            fragment: Some((FragmentKind::Paragraph, None)),
                        })?;
                    }
                }
            }
        }
        Ok(())
    }

    fn paragraph(&mut self, paragraph: &Element, path: &str) -> Result<(), ImportError> {
        let kind = classify(paragraph, self.styles)?;
        let visible = paragraph_text(paragraph);
        if visible.unsupported {
            self.warn(path.to_owned());
        }
        let fragment = (!visible.text.is_empty()).then_some((kind, None));
        self.push_record(Record {
            text: visible.text,
            fragment,
        })
    }

    fn table(&mut self, table: &Element, path: &str) -> Result<(), ImportError> {
        let mut header: Option<Vec<String>> = None;
        let mut header_record = None;
        let mut row_number = 0;
        for (index, child) in table.elements().enumerate() {
            match child.word.as_deref() {
                Some("tblPr" | "tblGrid") => {}
                Some("tr") => {
                    row_number += 1;
                    check_table_rows(row_number)?;
                    let row_path = format!("{path}/w:tr[{row_number}]");
                    let cells = self.row(child, &row_path)?;
                    let text = cells.join("\t");
                    match &header {
                        None => {
                            header = Some(cells);
                            header_record = Some(self.records.len());
                            self.push_record(Record {
                                text,
                                fragment: None,
                            })?;
                        }
                        Some(header_cells) => {
                            let fragment =
                                Some((FragmentKind::TableRow, Some(header_cells.clone())));
                            self.push_record(Record { text, fragment })?;
                        }
                    }
                }
                _ => {
                    let mut visible = Visible::new();
                    flatten_unsupported(child, &mut visible);
                    if visible.unsupported {
                        self.warn(format!("{path}/*[{}]", index + 1));
                        self.push_record(Record {
                            text: visible.text,
                            fragment: Some((FragmentKind::Paragraph, None)),
                        })?;
                    }
                }
            }
        }
        // A table with only a header row keeps its text as a Paragraph fragment, never a
        // silent omission.
        if row_number == 1 {
            if let Some(index) = header_record {
                if yields_fragment(&self.records[index].text) {
                    check_fragment_count(self.fragments + 1)?;
                    self.fragments += 1;
                    self.records[index].fragment = Some((FragmentKind::Paragraph, None));
                }
            }
        }
        Ok(())
    }

    /// The logical cells of a row, with merges expanded. The summed span is checked against
    /// [`DOCX_MAX_TABLE_COLUMNS`] before any cell is added.
    fn row(&mut self, row: &Element, path: &str) -> Result<Vec<String>, ImportError> {
        let mut cells = Vec::new();
        let mut width: u64 = 0;
        let mut cell_number = 0;
        for child in row.elements() {
            match child.word.as_deref() {
                Some("trPr" | "tblPrEx") => {}
                Some("tc") => {
                    cell_number += 1;
                    let cell_path = format!("{path}/w:tc[{cell_number}]");
                    self.cell(child, &cell_path, &mut width, &mut cells)?;
                }
                _ => {
                    let mut visible = Visible::new();
                    flatten_unsupported(child, &mut visible);
                    if visible.unsupported {
                        self.warn(path.to_owned());
                        width = add_columns(width, 1)?;
                        cells.push(visible.text);
                    }
                }
            }
        }
        // Header cells are fragment metadata, so they carry the record normalisation too.
        Ok(cells.iter().map(|cell| normalize_text(cell)).collect())
    }

    fn cell(
        &mut self,
        cell: &Element,
        path: &str,
        width: &mut u64,
        cells: &mut Vec<String>,
    ) -> Result<(), ImportError> {
        let properties = cell.child("tcPr");
        let span = match properties
            .and_then(|p| p.child("gridSpan"))
            .map(|g| g.attribute("val").unwrap_or_default())
        {
            None => 1,
            Some(value) => match value.parse::<u64>() {
                Ok(span) if span >= 1 => {
                    check_table_columns(span)?;
                    span
                }
                _ => {
                    return Err(xml_error(
                        DOCX_DOCUMENT_PART,
                        format!("invalid gridSpan {value:?}"),
                    ))
                }
            },
        };
        let continuation = match properties.and_then(|p| p.child("vMerge")) {
            None => false,
            Some(merge) => match merge.attribute("val") {
                None | Some("continue") => true,
                Some("restart") => false,
                Some(other) => {
                    return Err(xml_error(
                        DOCX_DOCUMENT_PART,
                        format!("invalid vMerge {other:?}"),
                    ))
                }
            },
        };
        // Checked before the cell or its span is materialized; a valid span is at most 64.
        *width = add_columns(*width, span)?;
        let mut paragraphs: Vec<String> = Vec::new();
        let mut unsupported = false;
        let mut paragraph_number = 0;
        for child in cell.elements() {
            match child.word.as_deref() {
                Some("tcPr") => {}
                Some("p") => {
                    paragraph_number += 1;
                    let visible = paragraph_text(child);
                    if visible.unsupported {
                        self.warn(format!("{path}/w:p[{paragraph_number}]"));
                    }
                    paragraphs.push(visible.text);
                }
                _ => {
                    let mut visible = Visible::new();
                    flatten_unsupported(child, &mut visible);
                    if visible.unsupported {
                        unsupported = true;
                        paragraphs.push(visible.text);
                    }
                }
            }
        }
        let text = paragraphs.join("\n");
        if unsupported || (continuation && !text.is_empty()) {
            self.warn(path.to_owned());
        }
        cells.push(text);
        // A checked span is in 1..=DOCX_MAX_TABLE_COLUMNS, so the conversion cannot truncate.
        cells.extend(std::iter::repeat_n(String::new(), span as usize - 1));
        Ok(())
    }
}

/// The summed span of a row after adding `span` columns, checked without overflow.
fn add_columns(width: u64, span: u64) -> Result<u64, ImportError> {
    let width = width.saturating_add(span);
    check_table_columns(width)?;
    Ok(width)
}

/// Imports a DOCX package.
pub fn import_docx(
    display_name: &str,
    bytes: &[u8],
    audit: &ImportAudit,
) -> Result<ImportedSource, ImportError> {
    let mut archive = open(bytes)?;
    let document = read_part(&mut archive, DOCX_DOCUMENT_PART)?.ok_or_else(|| {
        ImportError::DocxMissingPart {
            part: DOCX_DOCUMENT_PART.to_owned(),
        }
    })?;
    let styles = match read_part(&mut archive, DOCX_STYLES_PART)? {
        Some(xml) => parse_styles(&xml)?,
        None => Styles::new(),
    };
    let root = parse_xml(DOCX_DOCUMENT_PART, &document)?;
    if !root.is("document") {
        return Err(xml_error(DOCX_DOCUMENT_PART, "root is not w:document"));
    }
    let body = root
        .child("body")
        .ok_or_else(|| xml_error(DOCX_DOCUMENT_PART, "w:document has no w:body"))?;

    let mut importer = Importer {
        styles: &styles,
        records: Vec::new(),
        warnings: Vec::new(),
        fragments: 0,
    };
    importer.body(body)?;

    let mut text = String::new();
    let mut segments = Vec::new();
    for (index, record) in importer.records.into_iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        let start = text.len();
        text.push_str(&record.text);
        if let Some((kind, header_cells)) = record.fragment {
            push_segment(
                &mut segments,
                Segment {
                    kind,
                    start,
                    end: text.len(),
                    header_cells,
                },
            )?;
        }
    }
    let parse = ParseMetadata::from_warnings(importer.warnings)?;
    build_with_parse(
        SourceKind::Docx,
        display_name,
        bytes,
        text,
        segments,
        parse,
        audit,
    )
}
