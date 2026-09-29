//! Graph deltas and the per-operation semantic diff (metamodel §20.4).

use std::collections::BTreeSet;

use plumb_core::{Hash, Id};
use serde::Serialize;

/// Whether a diff entry concerns a node or an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum DiffElementKind {
    Node,
    Edge,
}

/// What a leaf operation did to an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum DiffChange {
    Added,
    Removed,
    Modified,
}

/// One element change made by one leaf operation, with generic element hashes before and
/// after (`None` before an addition and after a removal). A `Modified` entry may have equal
/// hashes when only non-element-hash persisted data changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffEntry {
    pub element_kind: DiffElementKind,
    pub id: Id,
    /// Depth-first leaf ordinal; automatic changes of one leaf share its ordinal.
    pub operation_ordinal: u32,
    pub change: DiffChange,
    pub before_hash: Option<Hash>,
    pub after_hash: Option<Hash>,
}

/// The result of applying a patch set, relative to its base graph.
///
/// `touched_*` lists every element touched by any evaluated leaf operation (even if its final
/// state equals the base); `added_*`, `removed_*` and `modified_*` are the net base-vs-final
/// persisted difference. `diff` is sorted by element ID, then operation ordinal, then Node
/// before Edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphDelta {
    pub base_semantic_hash: Hash,
    pub result_semantic_hash: Hash,
    pub touched_nodes: BTreeSet<Id>,
    pub touched_edges: BTreeSet<Id>,
    pub added_nodes: BTreeSet<Id>,
    pub removed_nodes: BTreeSet<Id>,
    pub modified_nodes: BTreeSet<Id>,
    pub added_edges: BTreeSet<Id>,
    pub removed_edges: BTreeSet<Id>,
    pub modified_edges: BTreeSet<Id>,
    pub diff: Vec<DiffEntry>,
}

/// Sorts diff entries by element ID, then operation ordinal, then Node before Edge.
pub(crate) fn sort_diff(diff: &mut [DiffEntry]) {
    diff.sort_by(|a, b| {
        a.id.cmp(&b.id)
            .then(a.operation_ordinal.cmp(&b.operation_ordinal))
            .then(a.element_kind.cmp(&b.element_kind))
    });
}
