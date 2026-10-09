//! Typed decision-table analysis (plan S2.6; compiler architecture §8).
//!
//! The one implementation lives in `plumb_validation::decision_table_analysis` (Hotfix 044), so
//! S2.10 validation reuses it without a crate cycle; this module keeps the S2.6 public paths.

pub use plumb_validation::decision_table_analysis::*;
