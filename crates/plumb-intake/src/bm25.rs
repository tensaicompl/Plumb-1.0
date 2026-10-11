//! Lexical BM25 mechanics of the retrieval index (plan X1.2, Hotfix 058): the version-1
//! tokenizer, the frozen constants, corpus statistics, scoring and ranking.
//!
//! This module knows nothing about the specification model; it works on plain text and token
//! counts. Every map is ordered so no iteration order can reach a result.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// The BM25 term-frequency saturation constant. Not configurable.
pub const BM25_K1: f64 = 1.2;

/// The BM25 document-length normalization constant. Not configurable.
pub const BM25_B: f64 = 0.75;

/// The version-1 tokenizer: every Unicode scalar is mapped through `char::to_lowercase` (which
/// may emit several scalars), then each maximal run of `char::is_alphanumeric` scalars is one
/// token and every other scalar separates. No normalization, stop words, stemming or synonyms.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            current.push(c);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Whether `text` yields at least one token, without building the tokens.
pub(crate) fn has_tokens(text: &str) -> bool {
    text.chars()
        .flat_map(char::to_lowercase)
        .any(char::is_alphanumeric)
}

/// The token statistics of one document: its length (token count including repetitions) and
/// the exact frequency of every term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TermCounts {
    pub(crate) length: usize,
    pub(crate) frequencies: BTreeMap<String, usize>,
}

impl TermCounts {
    pub(crate) fn from_text(text: &str) -> TermCounts {
        let tokens = tokenize(text);
        let mut frequencies: BTreeMap<String, usize> = BTreeMap::new();
        let length = tokens.len();
        for token in tokens {
            *frequencies.entry(token).or_default() += 1;
        }
        TermCounts {
            length,
            frequencies,
        }
    }
}

/// Global corpus statistics, always recomputed from the complete current document set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CorpusStats {
    /// N: the number of indexed documents.
    pub(crate) documents: usize,
    /// The sum of all document lengths.
    pub(crate) total_length: usize,
    /// df: the number of documents containing each term at least once.
    pub(crate) document_frequencies: BTreeMap<String, usize>,
}

impl CorpusStats {
    pub(crate) fn compute<'a>(documents: impl IntoIterator<Item = &'a TermCounts>) -> CorpusStats {
        let mut stats = CorpusStats::default();
        for counts in documents {
            stats.documents += 1;
            stats.total_length += counts.length;
            for term in counts.frequencies.keys() {
                *stats.document_frequencies.entry(term.clone()).or_default() += 1;
            }
        }
        stats
    }

    /// avgdl, defined only for a non-empty corpus.
    pub(crate) fn average_length(&self) -> Option<f64> {
        (self.documents > 0).then(|| self.total_length as f64 / self.documents as f64)
    }

    /// `ln(1 + (N - df + 0.5) / (df + 0.5))`, or `None` for a term absent from the corpus.
    pub(crate) fn idf(&self, term: &str) -> Option<f64> {
        let df = *self.document_frequencies.get(term)?;
        let (n, df) = (self.documents as f64, df as f64);
        Some((1.0 + (n - df + 0.5) / (df + 0.5)).ln())
    }

    /// The unique query terms present in the corpus with their IDF, in term order.
    pub(crate) fn weighted_terms(&self, query: &str) -> Vec<(String, f64)> {
        let terms: BTreeSet<String> = tokenize(query).into_iter().collect();
        terms
            .into_iter()
            .filter_map(|term| self.idf(&term).map(|idf| (term, idf)))
            .collect()
    }
}

/// The BM25 score of one document: the sum over the weighted unique query terms it contains of
/// `idf * tf * (k1 + 1) / (tf + k1 * (1 - b + b * dl / avgdl))`.
pub(crate) fn score(counts: &TermCounts, terms: &[(String, f64)], average_length: f64) -> f64 {
    let norm = BM25_K1 * (1.0 - BM25_B + BM25_B * counts.length as f64 / average_length);
    terms
        .iter()
        .filter_map(|(term, idf)| {
            let tf = *counts.frequencies.get(term)? as f64;
            Some(idf * tf * (BM25_K1 + 1.0) / (tf + norm))
        })
        .sum()
}

/// Hit order: score descending by `f64::total_cmp`, then key ascending.
pub(crate) fn rank_order<K: Ord>(a: &(K, f64), b: &(K, f64)) -> Ordering {
    b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0))
}
