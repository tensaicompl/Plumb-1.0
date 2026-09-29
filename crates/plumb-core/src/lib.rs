//! Plumb core primitives: validated IDs, hashes, canonical timestamps and clocks,
//! and RFC 8785 canonical JSON (plan §6.3).

pub mod canonical;
pub mod canonical_json;
pub mod clock;
pub mod error;
pub mod gate;
pub mod hash;
pub mod id;
pub mod stage;

pub use canonical::{canonical_hash, to_canonical_json};
pub use canonical_json::CanonicalJson;
pub use clock::{Clock, FixedClock, SystemClock, Timestamp};
pub use error::CoreError;
pub use gate::{GateId, UnknownGateId};
pub use hash::{Hash, HashKind};
pub use id::Id;
pub use stage::{StageId, UnknownStageId};
