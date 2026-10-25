//! Deterministic Markdown renderers of the functional pilot (plan S4.7, Hotfix 056): the
//! glossary and the model report.
//!
//! Semantic strings are escaped by one helper and otherwise reproduced unchanged; nothing here
//! evaluates a gate, computes readiness, reads time or touches a file.

use std::collections::BTreeMap;

use plumb_core::{Hash, Id};
use plumb_psg::{ElementStatus, Graph, NodePayload};
use plumb_validation::GateReport;

use crate::model::{ProjectionMetadata, ProjectionNotice};

/// The fixed glossary text of a Term without an Accepted Concept definition.
pub(crate) const NO_DEFINITION: &str = "_No accepted definition._";

/// The fixed model-report readiness statement.
pub(crate) const READINESS_STATEMENT: &str =
    "Readiness is advisory only and is not calculated by S4.7. It does not alter the F4 gate result.";

/// Escapes Markdown control characters of a semantic string and normalizes line endings to LF.
/// Line-start block markers (`-`, `+`, `=` setext underlines and ordered-list numerals, after
/// up to three spaces) are escaped too, so every line renders as the literal text (Fable #144).
pub(crate) fn escape(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<String> = normalized.split('\n').map(escape_line).collect();
    lines.join("\n")
}

/// [`escape`] of one line.
fn escape_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 2);
    let indent = line.len() - line.trim_start_matches(' ').len();
    let (lead, rest) = line.split_at(indent.min(3));
    out.push_str(lead);
    // A list bullet or setext underline at the line start; `#`, `>` and `*` are escaped below.
    if rest.starts_with(['-', '+', '=']) {
        out.push('\\');
    }
    // An ordered-list marker: up to nine digits followed by `.` or `)`.
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let ordered = (1..=9).contains(&digits) && rest[digits..].starts_with(['.', ')']);
    for (index, c) in rest.char_indices() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|'
        ) || (ordered && index == digits)
        {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A CommonMark code span of a semantic string: content is literal (no escapes apply inside a
/// code span), the fence is longer than any backtick run in the text and a leading or trailing
/// backtick is padded with a space. Line endings are normalized to LF.
fn code(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let longest = normalized
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = if normalized.starts_with('`') || normalized.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{normalized}{pad}{fence}")
}

/// Joins blocks with one blank line and ends the document with exactly one newline.
fn document(blocks: Vec<String>) -> String {
    let mut text = blocks.join("\n\n");
    text.push('\n');
    text
}

fn accepted<'g>(graph: &'g Graph, id: &Id) -> Option<&'g NodePayload> {
    graph
        .node(id)
        .filter(|n| n.status == ElementStatus::Accepted)
        .map(|n| &n.payload)
}

/// glossary.md: every Accepted Term in node-ID order.
pub(crate) fn render_glossary(graph: &Graph) -> String {
    let mut blocks = vec!["# Glossary".to_owned()];
    for (id, node) in graph.nodes() {
        let NodePayload::Term(term) = &node.payload else {
            continue;
        };
        if node.status != ElementStatus::Accepted {
            continue;
        }
        blocks.push(format!("## `{id}` — {}", escape(&term.term)));
        let mut facts = vec![
            format!("- Language: {}", code(&term.language)),
            format!(
                "- Status: {}",
                code(term.status.as_deref().unwrap_or("unspecified"))
            ),
        ];
        let mut aliases = term.aliases.clone().unwrap_or_default();
        aliases.sort();
        aliases.dedup();
        if !aliases.is_empty() {
            let rendered: Vec<String> = aliases.iter().map(|a| escape(a)).collect();
            facts.push(format!("- Aliases: {}", rendered.join(", ")));
        }
        blocks.push(facts.join("\n"));
        let definition = term
            .definition_ref
            .as_ref()
            .and_then(|r| match accepted(graph, r) {
                Some(NodePayload::Concept(c)) => Some(escape(&c.definition)),
                _ => None,
            })
            .unwrap_or_else(|| NO_DEFINITION.to_owned());
        blocks.push(definition);
    }
    document(blocks)
}

/// The inputs of model-report.md beyond the graph.
pub(crate) struct ModelReportInput<'a> {
    pub(crate) gate_report: &'a GateReport,
    pub(crate) gate_report_hash: &'a Hash,
    pub(crate) functional: &'a ProjectionMetadata,
    pub(crate) evidence_hash: &'a Hash,
    pub(crate) semantic_hash: &'a Hash,
}

fn notices(title: &str, notices: &[ProjectionNotice]) -> Option<String> {
    if notices.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for notice in notices {
        let mut refs = notice.source_refs.clone();
        refs.sort();
        let refs: Vec<String> = refs.iter().map(|r| format!("`{r}`")).collect();
        lines.push(format!(
            "- {} ({}): {}",
            code(&notice.code),
            refs.join(", "),
            escape(&notice.message)
        ));
    }
    Some(format!("### {title}\n\n{}", lines.join("\n")))
}

/// model-report.md: a deterministic summary of the baseline, the supplied F4 report, accepted
/// counts and the compatibility projection. It never re-decides the gate.
pub(crate) fn render_model_report(graph: &Graph, input: &ModelReportInput<'_>) -> String {
    let report = input.gate_report;
    let mut blocks = vec!["# Model Report".to_owned(), "## Baseline".to_owned()];
    blocks.push(
        [
            format!("- Project ID: `{}`", graph.project_id()),
            format!("- Profile ID: `{}`", report.profile_id),
            format!("- Semantic hash: `{}`", input.semantic_hash),
            format!("- Evidence hash: `{}`", input.evidence_hash),
            format!("- Profile hash: `{}`", report.profile_hash),
            format!("- Rule-pack hash: `{}`", report.rule_pack_hash),
        ]
        .join("\n"),
    );

    blocks.push("## F4 Gate".to_owned());
    blocks.push(
        [
            format!("- Result: `{}`", report.result.as_str()),
            format!("- Gate-report content hash: `{}`", input.gate_report_hash),
            format!("- Blocker failures: {}", report.summary.blocker_failed),
            format!("- Blocker waivers: {}", report.summary.blocker_waived),
            format!("- Warnings: {}", report.summary.warnings),
            format!("- Generated findings: {}", report.findings.len()),
            format!("- Applied waivers: {}", report.waivers.len()),
        ]
        .join("\n"),
    );

    blocks.push("## Accepted Model Counts".to_owned());
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for node in graph.nodes().values() {
        if node.status == ElementStatus::Accepted {
            *counts.entry(node.payload.node_type().as_str()).or_default() += 1;
        }
    }
    let mut table = vec![
        "| Node type | Accepted |".to_owned(),
        "| --- | ---: |".to_owned(),
    ];
    table.extend(counts.iter().map(|(t, n)| format!("| {t} | {n} |")));
    blocks.push(table.join("\n"));
    let edges = graph
        .edges()
        .values()
        .filter(|e| e.status == ElementStatus::Accepted)
        .count();
    blocks.push(format!("- Accepted edges: {edges}"));

    let f = input.functional;
    blocks.push("## Functional Compatibility Projection".to_owned());
    blocks.push(
        [
            format!("- functional.yaml content hash: `{}`", f.content_hash),
            format!("- Projection version: {}", f.projection_version),
            format!("- Warnings: {}", f.warnings.len()),
            format!("- Lossy mappings: {}", f.lossy_mappings.len()),
        ]
        .join("\n"),
    );
    blocks.extend(notices("Warnings", &f.warnings));
    blocks.extend(notices("Lossy mappings", &f.lossy_mappings));

    let (mut total, mut confirmed, mut derived) = (0usize, 0usize, 0usize);
    let mut decisions = 0usize;
    for node in graph.nodes().values() {
        if node.status != ElementStatus::Accepted {
            continue;
        }
        match &node.payload {
            NodePayload::Scenario(s) => {
                total += 1;
                match s.confirmation_status.as_deref() {
                    Some("Confirmed") => confirmed += 1,
                    Some("Derived") => derived += 1,
                    _ => {}
                }
            }
            NodePayload::ResolutionDecision(_) => decisions += 1,
            _ => {}
        }
    }
    blocks.push("## Scenarios".to_owned());
    blocks.push(
        [
            format!("- Accepted Scenarios: {total}"),
            format!("- Confirmed Scenarios: {confirmed}"),
            format!("- Derived Scenarios: {derived}"),
            format!(
                "- Other/unspecified confirmation status: {}",
                total - confirmed - derived
            ),
        ]
        .join("\n"),
    );
    blocks.push("## Decisions".to_owned());
    blocks.push(format!("- Accepted ResolutionDecisions: {decisions}"));
    blocks.push("## Readiness".to_owned());
    blocks.push(READINESS_STATEMENT.to_owned());
    document(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fable #144: line-start block markers of a (multi-line) semantic string are escaped, so
    /// the string renders unchanged instead of as a list, quote, heading or setext heading.
    #[test]
    fn escape_neutralizes_line_start_block_markers() {
        let text = "- item\n+ plus\n> quote\n# head\n1. one\n12) two\n===\n---\n  - indented\nmid - dash 3. ok";
        assert_eq!(
            escape(text),
            "\\- item\n\\+ plus\n\\> quote\n\\# head\n1\\. one\n12\\) two\n\\===\n\\---\n  \\- indented\nmid - dash 3. ok"
        );
        // Plain text, including CRLF, is unchanged apart from LF normalization.
        assert_eq!(
            escape("The system shall\r\nrecord leave."),
            "The system shall\nrecord leave."
        );
        assert_eq!(escape("2026 is a year"), "2026 is a year");
    }
}
