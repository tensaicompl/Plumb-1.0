//! Functional business Events for S2.7 operation analysis (compiler architecture §8).
//!
//! An Event's identity is project-global by its exact name. A newly proposed Event carries no
//! `semantic_type` (no vocabulary is frozen) and its payload reference is absent, an Accepted
//! DataSchema or a single Accepted Attribute. Existing Events are reused only when compatible
//! and are never overwritten; a same-name Event under another ID is a conflict. No
//! EventContract, Message, Channel or DataSchema is created here.

use plumb_core::{to_canonical_json, CoreError, Hash, Id};
use plumb_psg::{ElementStatus, Graph, NodePayload, NodeType};
use serde::{Deserialize, Serialize};

use crate::operation::{OperationGroundedRange, OperationIssue};

/// Extension key of the provenance [`EventOrigin`] of a newly proposed Event.
pub const EVENT_ORIGIN_EXTENSION: &str = "plumb_functional:event_origin";

/// Provenance of a newly proposed Event (`plumb_functional:event_origin`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventOrigin {
    pub evidence: Vec<OperationGroundedRange>,
}

/// `event:<first 16 hex of SHA-256(RFC 8785 {project_id, name, node_type: event})>`.
pub fn event_id(project_id: &Id, name: &str) -> Result<Id, CoreError> {
    let body = serde_json::json!({"project_id": project_id, "name": name, "node_type": "event"});
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("event:{}", &hex[..16]).parse()
}

/// How a produced Event candidate relates to the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventReconciliation {
    /// No node exists at the deterministic ID: propose a new Event.
    New,
    /// A compatible active Event exists at the deterministic ID and is reused unchanged.
    Reuse,
}

fn is_active(status: ElementStatus) -> bool {
    matches!(status, ElementStatus::Proposed | ElementStatus::Accepted)
}

/// Reconciles a new produced Event candidate with the graph. A candidate without payload
/// reference may reuse a richer existing Event; a candidate with `Some(X)` requires the
/// existing Event to carry exactly `Some(X)`. Existing `semantic_type` is never erased.
pub(crate) fn reconcile_event(
    graph: &Graph,
    event_ref: &Id,
    name: &str,
    payload_schema_ref: Option<&Id>,
) -> Result<EventReconciliation, OperationIssue> {
    for other in graph.node_ids_by_type(NodeType::Event) {
        if other == event_ref {
            continue;
        }
        let Some(node) = graph.node(other) else {
            continue;
        };
        if let NodePayload::Event(e) = &node.payload {
            if is_active(node.status) && e.name == name {
                return Err(OperationIssue::ExistingEventNameConflict {
                    event_ref: event_ref.clone(),
                    existing_ref: other.clone(),
                });
            }
        }
    }
    let Some(existing) = graph.node(event_ref) else {
        return Ok(EventReconciliation::New);
    };
    let conflict = || OperationIssue::ExistingEventConflict {
        event_ref: event_ref.clone(),
    };
    let NodePayload::Event(e) = &existing.payload else {
        return Err(conflict());
    };
    if !is_active(existing.status) || e.name != name {
        return Err(conflict());
    }
    match payload_schema_ref {
        None => Ok(EventReconciliation::Reuse),
        Some(x) if e.payload_schema_ref.as_ref() == Some(x) => Ok(EventReconciliation::Reuse),
        Some(_) => Err(conflict()),
    }
}
