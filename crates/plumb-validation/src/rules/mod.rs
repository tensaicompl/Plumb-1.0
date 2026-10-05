//! Production rule evaluators, registered gate by gate.
//!
//! Each stage task that implements a gate's rules adds its module here and registers exactly
//! that gate's evaluators.

mod f1;
mod i0;

pub use f1::register_f1_evaluators;
pub use i0::register_i0_evaluators;
