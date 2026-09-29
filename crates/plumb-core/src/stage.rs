//! The canonical compiler pipeline stage identifiers (compiler architecture §2).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// One stage of the canonical compiler pipeline, serialized as exactly `S0` .. `S12`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StageId {
    S0,
    S1,
    S2,
    S3,
    S4,
    S5,
    S6,
    S7,
    S8,
    S9,
    S10,
    S11,
    S12,
}

/// A string that is not one of the 13 stage identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown stage id {0:?}")]
pub struct UnknownStageId(pub String);

impl StageId {
    /// Every stage, in pipeline order.
    pub const ALL: [StageId; 13] = [
        StageId::S0,
        StageId::S1,
        StageId::S2,
        StageId::S3,
        StageId::S4,
        StageId::S5,
        StageId::S6,
        StageId::S7,
        StageId::S8,
        StageId::S9,
        StageId::S10,
        StageId::S11,
        StageId::S12,
    ];

    /// The exact serialized string for this stage.
    pub fn as_str(self) -> &'static str {
        match self {
            StageId::S0 => "S0",
            StageId::S1 => "S1",
            StageId::S2 => "S2",
            StageId::S3 => "S3",
            StageId::S4 => "S4",
            StageId::S5 => "S5",
            StageId::S6 => "S6",
            StageId::S7 => "S7",
            StageId::S8 => "S8",
            StageId::S9 => "S9",
            StageId::S10 => "S10",
            StageId::S11 => "S11",
            StageId::S12 => "S12",
        }
    }
}

impl FromStr for StageId {
    type Err = UnknownStageId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        StageId::ALL
            .into_iter()
            .find(|stage| stage.as_str() == s)
            .ok_or_else(|| UnknownStageId(s.to_owned()))
    }
}

impl fmt::Display for StageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for StageId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StageId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}
