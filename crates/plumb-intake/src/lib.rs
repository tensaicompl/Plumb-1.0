//! Plumb intake. X1.2 provides deterministic lexical BM25 retrieval over the accepted PSG
//! (Hotfix 058).

mod bm25;
mod index;

pub use bm25::{tokenize, BM25_B, BM25_K1};
pub use index::{
    IndexError, IndexUpdateSummary, LexicalIndex, RetrievalDocumentKind, RetrievalHit,
    RETRIEVAL_INDEX_VERSION,
};
