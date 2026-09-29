//! The validation gate identifiers of the compiler pipeline (compiler architecture §2).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// One validation gate, serialized as exactly its name (`I0`, `F1`, …, `C1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GateId {
    I0,
    F1,
    F2,
    F3,
    F4,
    Q1,
    A1,
    A2,
    A3,
    A4,
    D1,
    D2,
    C1,
}

/// A string that is not one of the 13 gate identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown gate id {0:?}")]
pub struct UnknownGateId(pub String);

impl GateId {
    /// Every gate, in dependency-chain order.
    pub const ALL: [GateId; 13] = [
        GateId::I0,
        GateId::F1,
        GateId::F2,
        GateId::F3,
        GateId::F4,
        GateId::Q1,
        GateId::A1,
        GateId::A2,
        GateId::A3,
        GateId::A4,
        GateId::D1,
        GateId::D2,
        GateId::C1,
    ];

    /// The exact serialized string for this gate.
    pub fn as_str(self) -> &'static str {
        match self {
            GateId::I0 => "I0",
            GateId::F1 => "F1",
            GateId::F2 => "F2",
            GateId::F3 => "F3",
            GateId::F4 => "F4",
            GateId::Q1 => "Q1",
            GateId::A1 => "A1",
            GateId::A2 => "A2",
            GateId::A3 => "A3",
            GateId::A4 => "A4",
            GateId::D1 => "D1",
            GateId::D2 => "D2",
            GateId::C1 => "C1",
        }
    }

    /// Position in [`GateId::ALL`] (`0` for `I0` .. `12` for `C1`).
    pub fn ordinal(self) -> usize {
        self as usize
    }
}

impl FromStr for GateId {
    type Err = UnknownGateId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GateId::ALL
            .into_iter()
            .find(|gate| gate.as_str() == s)
            .ok_or_else(|| UnknownGateId(s.to_owned()))
    }
}

impl fmt::Display for GateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for GateId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GateId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}
