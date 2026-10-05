//! PlumbExpr units and types (plan S2.4, Hotfix 037).
//!
//! `Unit` is an open, canonical lowercase symbol compared by exact equality; there is no
//! conversion or dimensional algebra. `Ty` is the pilot type vocabulary. Assignability,
//! coercion and semantic typing belong to the S2.5 type checker.

use std::fmt;
use std::str::FromStr;

use plumb_core::Id;
use thiserror::Error;

/// The largest `Ty::Decimal` fractional scale (`rust_decimal`).
pub const MAX_DECIMAL_SCALE: u8 = 28;

/// Why a unit symbol or a type is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TypeError {
    #[error("invalid unit symbol {0:?}")]
    InvalidUnit(String),
    #[error("decimal scale {0} exceeds {MAX_DECIMAL_SCALE}")]
    InvalidDecimalScale(u8),
}

/// A canonical unit symbol `[a-z][a-z0-9_]*`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unit(String);

impl Unit {
    /// Validates the canonical unit symbol syntax.
    pub fn new(symbol: &str) -> Result<Unit, TypeError> {
        let mut chars = symbol.chars();
        let valid = chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if valid {
            Ok(Unit(symbol.to_owned()))
        } else {
            Err(TypeError::InvalidUnit(symbol.to_owned()))
        }
    }

    /// The canonical symbol.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Unit {
    type Err = TypeError;

    fn from_str(s: &str) -> Result<Unit, TypeError> {
        Unit::new(s)
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The pilot PlumbExpr types. `String` is added by Hotfix 037 for the HR String value type;
/// there is no Optional, Null, Unknown or Any type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    Int,
    /// Fractional scale `0..=28`.
    Decimal(u8),
    Bool,
    String,
    Date,
    DateTime,
    Duration(Unit),
    Quantity(Unit),
    Enum(Id),
    Ref(Id),
    List(Box<Ty>),
}

impl Ty {
    /// Recursively validates decimal scales; units and IDs are valid by construction.
    pub fn validate(&self) -> Result<(), TypeError> {
        match self {
            Ty::Decimal(scale) if *scale > MAX_DECIMAL_SCALE => {
                Err(TypeError::InvalidDecimalScale(*scale))
            }
            Ty::List(inner) => inner.validate(),
            _ => Ok(()),
        }
    }
}
