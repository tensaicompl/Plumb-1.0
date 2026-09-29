//! Element lifecycle status (metamodel §4.3).

use serde::{Deserialize, Serialize};

/// Status of a PSG node or edge.
///
/// Serialized as exactly the case-sensitive variant name; unknown values are rejected.
/// v2's `Confirmed` does not exist (it became `Accepted`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ElementStatus {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
    Deprecated,
    Suspect,
}
