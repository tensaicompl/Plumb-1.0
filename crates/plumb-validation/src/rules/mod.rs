//! Production rule evaluators, registered gate by gate.
//!
//! Each stage task that implements a gate's rules adds its module here and registers exactly
//! that gate's evaluators.

mod i0;

pub use i0::register_i0_evaluators;
