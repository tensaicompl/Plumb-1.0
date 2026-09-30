//! Production rule evaluators, registered gate by gate.
//!
//! The evaluator framework ships without any rule implementation. Each stage task that
//! implements a gate's rules adds its module here and registers exactly that gate's evaluators.
