//! Governed node dispositions read by F2 (H0.7, architecture §33.7).
//!
//! Some profile alternatives cannot be inferred from graph structure: whether an Event has an
//! external source or a terminal/audit-only purpose, and which State of a state machine is its
//! initial state (or is terminal or entered externally). They are recorded as namespaced node
//! extensions written only through governed `SetNodeExtension` proposals of the R0.6 directory
//! (`event_dispositions`, `state_dispositions`), never inferred. This module owns their exact
//! wire form; F2 decodes them, and a value that does not decode is a rule violation of its node,
//! never an evaluator error.

use plumb_psg::{ExtensionKey, Node};
use serde::{Deserialize, Deserializer, Serialize};

/// The Event extension key.
pub const EVENT_DISPOSITION_EXTENSION: &str = "plumb_functional:event_disposition";

/// The State extension key.
pub const STATE_DISPOSITION_EXTENSION: &str = "plumb_functional:state_disposition";

/// The only disposition version.
pub const DISPOSITION_VERSION: u32 = 1;

/// Why a produced Event needs no consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalPurpose {
    Terminal,
    Audit,
}

/// `plumb_functional:event_disposition` =
/// `{"version": 1, "external_source": bool, "terminal_purpose": null | "terminal" | "audit"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDisposition {
    pub version: u32,
    pub external_source: bool,
    #[serde(deserialize_with = "present")]
    pub terminal_purpose: Option<TerminalPurpose>,
}

/// `plumb_functional:state_disposition` =
/// `{"version": 1, "initial": bool, "terminal": bool, "externally_entered": bool}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateDisposition {
    pub version: u32,
    pub initial: bool,
    pub terminal: bool,
    pub externally_entered: bool,
}

/// A required key whose value may be `null`.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<TerminalPurpose>, D::Error> {
    Option::<TerminalPurpose>::deserialize(d)
}

/// A disposition extension that is present but malformed or of another version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispositionInvalid;

/// A node's disposition: `Ok(None)` when absent.
fn decode<T: for<'de> Deserialize<'de>>(
    node: &Node,
    key: &str,
    version: impl Fn(&T) -> u32,
) -> Result<Option<T>, DispositionInvalid> {
    let key: ExtensionKey = key.parse().map_err(|_| DispositionInvalid)?;
    let Some(value) = node.extensions.get(&key) else {
        return Ok(None);
    };
    let decoded: T = serde_json::from_value(value.clone()).map_err(|_| DispositionInvalid)?;
    if version(&decoded) == DISPOSITION_VERSION {
        Ok(Some(decoded))
    } else {
        Err(DispositionInvalid)
    }
}

/// The Event disposition of `node`.
pub fn event_disposition(node: &Node) -> Result<Option<EventDisposition>, DispositionInvalid> {
    decode(node, EVENT_DISPOSITION_EXTENSION, |d: &EventDisposition| {
        d.version
    })
}

/// The State disposition of `node`.
pub fn state_disposition(node: &Node) -> Result<Option<StateDisposition>, DispositionInvalid> {
    decode(node, STATE_DISPOSITION_EXTENSION, |d: &StateDisposition| {
        d.version
    })
}
