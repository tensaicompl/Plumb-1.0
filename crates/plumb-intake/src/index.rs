//! The deterministic in-memory lexical retrieval index over the accepted PSG (plan X1.2,
//! Hotfix 058): corpus projection, corpus identity, build, content-diff update and search.
//!
//! The index reads a constructed `Graph` and never changes it. Hits are advisory lexical
//! candidates ranked by BM25; deciding what a candidate means belongs to intake.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use plumb_core::{canonical_hash, to_canonical_json, CoreError, Hash, HashKind, Id};
use plumb_psg::{ElementStatus, Graph, NodePayload};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::bm25::{self, CorpusStats, TermCounts};

/// The version of the corpus projection, tokenizer and BM25 behaviour; bound into
/// `corpus_hash`.
pub const RETRIEVAL_INDEX_VERSION: u32 = 1;

/// The closed set of indexed payload families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalDocumentKind {
    Requirement,
    Concept,
    Rule,
    Calculation,
    Scenario,
    Evidence,
}

impl RetrievalDocumentKind {
    /// The exact wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            RetrievalDocumentKind::Requirement => "requirement",
            RetrievalDocumentKind::Concept => "concept",
            RetrievalDocumentKind::Rule => "rule",
            RetrievalDocumentKind::Calculation => "calculation",
            RetrievalDocumentKind::Scenario => "scenario",
            RetrievalDocumentKind::Evidence => "evidence",
        }
    }
}

impl fmt::Display for RetrievalDocumentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One ranked search result. The score is advisory ranking evidence only.
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalHit {
    pub id: Id,
    pub kind: RetrievalDocumentKind,
    pub score: f64,
}

/// What an incremental update did, by document ID.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexUpdateSummary {
    pub added: BTreeSet<Id>,
    pub changed: BTreeSet<Id>,
    pub removed: BTreeSet<Id>,
    pub unchanged: usize,
}

/// Index failures. A failed update leaves the index unchanged.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum IndexError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("graph project {actual} does not match index project {expected}; rebuild the index")]
    ProjectMismatch { expected: Id, actual: Id },
    #[error("graph profile {actual} does not match index profile {expected}; rebuild the index")]
    ProfileMismatch { expected: Id, actual: Id },
    #[error("non-finite BM25 score for document {document_ref}")]
    NonFiniteScore { document_ref: Id },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedDocument {
    kind: RetrievalDocumentKind,
    text: String,
    terms: TermCounts,
}

/// A deterministic in-memory BM25 index of the Accepted searchable PSG documents, bound to the
/// project and profile it was built for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexicalIndex {
    project_id: Id,
    profile_id: Id,
    semantic_hash: Hash,
    evidence_hash: Hash,
    corpus_hash: Hash,
    documents: BTreeMap<Id, IndexedDocument>,
    stats: CorpusStats,
}

/// The retrieval text of one node, or `None` when the node is not a retrieval document.
pub(crate) fn document_text(
    payload: &NodePayload,
) -> Result<Option<(RetrievalDocumentKind, String)>, CoreError> {
    fn lines<'a>(lines: impl IntoIterator<Item = Option<&'a str>>) -> String {
        lines.into_iter().flatten().collect::<Vec<_>>().join("\n")
    }
    Ok(Some(match payload {
        NodePayload::Requirement(r) => (RetrievalDocumentKind::Requirement, r.statement.clone()),
        NodePayload::Concept(c) => (
            RetrievalDocumentKind::Concept,
            lines([Some(c.name.as_str()), Some(c.definition.as_str())]),
        ),
        NodePayload::Rule(r) => (
            RetrievalDocumentKind::Rule,
            lines([
                Some(r.name.as_str()),
                r.condition_expr.as_deref(),
                r.action_expr.as_deref(),
            ]),
        ),
        NodePayload::Calculation(c) => (
            RetrievalDocumentKind::Calculation,
            lines([
                Some(c.name.as_str()),
                Some(c.expression.as_str()),
                Some(c.result_type.as_str()),
                c.unit.as_deref(),
                c.rounding.as_deref(),
            ]),
        ),
        NodePayload::Scenario(s) => {
            let search = to_canonical_json(&json!({
                "scenario_kind": s.scenario_kind,
                "given": s.given,
                "when": s.when,
                "then": s.then,
            }))?;
            let search = String::from_utf8(search)
                .map_err(|e| CoreError::Canonicalization(e.to_string()))?;
            (
                RetrievalDocumentKind::Scenario,
                lines([Some(s.name.as_str()), Some(search.as_str())]),
            )
        }
        NodePayload::EvidenceFragment(f) => match &f.extracted_text {
            Some(text) => (RetrievalDocumentKind::Evidence, text.clone()),
            None => return Ok(None),
        },
        _ => return Ok(None),
    }))
}

/// The current corpus projection: every Accepted, supported, token-bearing node in ID order.
fn project(graph: &Graph) -> Result<BTreeMap<Id, (RetrievalDocumentKind, String)>, IndexError> {
    let mut documents = BTreeMap::new();
    for (id, node) in graph.nodes() {
        if node.status != ElementStatus::Accepted {
            continue;
        }
        if let Some((kind, text)) = document_text(&node.payload)? {
            if bm25::has_tokens(&text) {
                documents.insert(id.clone(), (kind, text));
            }
        }
    }
    Ok(documents)
}

impl LexicalIndex {
    /// Builds the index of `graph` from scratch.
    pub fn build(graph: &Graph) -> Result<LexicalIndex, IndexError> {
        let documents = project(graph)?
            .into_iter()
            .map(|(id, (kind, text))| {
                let terms = TermCounts::from_text(&text);
                (id, IndexedDocument { kind, text, terms })
            })
            .collect();
        LexicalIndex::assemble(graph, documents)
    }

    /// Refreshes the index to `graph` of the same project and profile by diffing the current
    /// corpus projection against the stored documents. The result equals
    /// `LexicalIndex::build(graph)`; on error the index is unchanged.
    pub fn update(&mut self, graph: &Graph) -> Result<IndexUpdateSummary, IndexError> {
        if graph.project_id() != &self.project_id {
            return Err(IndexError::ProjectMismatch {
                expected: self.project_id.clone(),
                actual: graph.project_id().clone(),
            });
        }
        if graph.profile_id() != &self.profile_id {
            return Err(IndexError::ProfileMismatch {
                expected: self.profile_id.clone(),
                actual: graph.profile_id().clone(),
            });
        }
        let current = project(graph)?;
        let mut summary = IndexUpdateSummary {
            removed: self
                .documents
                .keys()
                .filter(|id| !current.contains_key(*id))
                .cloned()
                .collect(),
            ..IndexUpdateSummary::default()
        };
        let mut documents = BTreeMap::new();
        for (id, (kind, text)) in current {
            let document = match self.documents.get(&id) {
                Some(old) if old.kind == kind && old.text == text => {
                    summary.unchanged += 1;
                    old.clone()
                }
                previous => {
                    if previous.is_some() {
                        summary.changed.insert(id.clone());
                    } else {
                        summary.added.insert(id.clone());
                    }
                    let terms = TermCounts::from_text(&text);
                    IndexedDocument { kind, text, terms }
                }
            };
            documents.insert(id, document);
        }
        *self = LexicalIndex::assemble(graph, documents)?;
        Ok(summary)
    }

    /// Computes global statistics and identities for a complete document set.
    fn assemble(
        graph: &Graph,
        documents: BTreeMap<Id, IndexedDocument>,
    ) -> Result<LexicalIndex, IndexError> {
        let stats = CorpusStats::compute(documents.values().map(|d| &d.terms));
        let entries: Vec<_> = documents
            .iter()
            .map(|(id, d)| json!({"id": id.as_str(), "kind": d.kind.as_str(), "text": d.text}))
            .collect();
        let corpus_hash = canonical_hash(
            HashKind::Generic,
            &json!({
                "version": RETRIEVAL_INDEX_VERSION,
                "project_id": graph.project_id().as_str(),
                "profile_id": graph.profile_id().as_str(),
                "documents": entries,
            }),
        )?;
        Ok(LexicalIndex {
            project_id: graph.project_id().clone(),
            profile_id: graph.profile_id().clone(),
            semantic_hash: graph.semantic_hash()?,
            evidence_hash: graph.evidence_hash()?,
            corpus_hash,
            documents,
            stats,
        })
    }

    /// The `top_k` positive-score BM25 hits for `query`, by score descending then ID ascending.
    /// Fails only if a score is not finite, which is an index invariant failure.
    pub fn search(&self, query: &str, top_k: usize) -> Result<Vec<RetrievalHit>, IndexError> {
        let Some(average_length) = self.stats.average_length() else {
            return Ok(Vec::new());
        };
        if top_k == 0 {
            return Ok(Vec::new());
        }
        let terms = self.stats.weighted_terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let mut scored = Vec::new();
        for (id, document) in &self.documents {
            let score = bm25::score(&document.terms, &terms, average_length);
            if !score.is_finite() {
                return Err(IndexError::NonFiniteScore {
                    document_ref: id.clone(),
                });
            }
            if score > 0.0 {
                scored.push((id, score));
            }
        }
        scored.sort_by(bm25::rank_order);
        scored.truncate(top_k);
        Ok(scored
            .into_iter()
            .map(|(id, score)| RetrievalHit {
                id: id.clone(),
                kind: self.documents[id].kind,
                score,
            })
            .collect())
    }

    /// The Generic hash of the canonical corpus projection.
    pub fn corpus_hash(&self) -> &Hash {
        &self.corpus_hash
    }

    /// The companion semantic hash of the indexed graph.
    pub fn semantic_hash(&self) -> &Hash {
        &self.semantic_hash
    }

    /// The companion evidence hash of the indexed graph.
    pub fn evidence_hash(&self) -> &Hash {
        &self.evidence_hash
    }

    pub fn project_id(&self) -> &Id {
        &self.project_id
    }

    pub fn profile_id(&self) -> &Id {
        &self.profile_id
    }

    /// N: the number of indexed documents.
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }
}
