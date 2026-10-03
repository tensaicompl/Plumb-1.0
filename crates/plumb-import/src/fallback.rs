//! Deterministic segmentation fallback (plan S0.4), used when no usable inference artifact is
//! supplied. A list item is one requirement candidate; any other fragment is split into ASCII
//! sentences classified by whole-word modal tokens. Text the heuristic does not select stays
//! covered as `non_requirement` flagged `unassigned_text`, so every byte is covered exactly once.
//! No provider, schema or clock is used.

use plumb_core::Id;

use crate::segment::{SegmentCandidate, SegmentClassification, SegmentFlag, SegmentationError};
use crate::source::FragmentKind;

/// The requirement modal tokens, matched ASCII case-insensitively as whole tokens.
const MODALS: [&str; 4] = ["shall", "must", "should", "can"];

/// The candidates of one fragment, in source order, fully partitioning `text`.
pub(crate) fn fallback_segments(
    fragment_ref: &Id,
    text: &str,
    kind: FragmentKind,
) -> Result<Vec<SegmentCandidate>, SegmentationError> {
    if kind == FragmentKind::ListItem {
        return Ok(vec![SegmentCandidate::new(
            fragment_ref.clone(),
            0,
            text.len() as u64,
            SegmentClassification::RequirementCandidate,
            Vec::new(),
        )?]);
    }
    sentences(text)
        .into_iter()
        .map(|(start, end)| {
            let (classification, flags) = if contains_modal(&text.as_bytes()[start..end]) {
                (SegmentClassification::RequirementCandidate, Vec::new())
            } else {
                (
                    SegmentClassification::NonRequirement,
                    vec![SegmentFlag::UnassignedText],
                )
            };
            SegmentCandidate::new(
                fragment_ref.clone(),
                start as u64,
                end as u64,
                classification,
                flags,
            )
        })
        .collect()
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// Sentence byte ranges covering all of `text`. A sentence ends at an ASCII `.`, `?` or `!`
/// followed by the end of text or ASCII whitespace, and owns that terminator and all whitespace
/// immediately after it; leading whitespace belongs to the first sentence and the unterminated
/// remainder is the last one. Terminators and whitespace are ASCII, so every boundary is a
/// UTF-8 character boundary.
fn sentences(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let terminates = matches!(bytes[i], b'.' | b'?' | b'!')
            && bytes.get(i + 1).is_none_or(|&next| is_space(next));
        if terminates {
            let mut end = i + 1;
            while end < bytes.len() && is_space(bytes[end]) {
                end += 1;
            }
            ranges.push((start, end));
            start = end;
            i = end;
        } else {
            i += 1;
        }
    }
    if start < bytes.len() {
        ranges.push((start, bytes.len()));
    }
    ranges
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether a whole `[A-Za-z0-9_]` token of `sentence` is a modal, ignoring ASCII case.
fn contains_modal(sentence: &[u8]) -> bool {
    sentence.split(|&byte| !is_token_byte(byte)).any(|token| {
        MODALS
            .iter()
            .any(|modal| token.eq_ignore_ascii_case(modal.as_bytes()))
    })
}
