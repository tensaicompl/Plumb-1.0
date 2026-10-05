//! PlumbExpr language v1. S2.4 (Hotfix 037): the pest grammar, the typed syntax tree, the
//! pilot `Ty` vocabulary and the canonical unparser. S2.5 (Hotfix 038): the explicit type and
//! value environments, the type checker, the evaluator with its frozen callables, injected
//! calendars and predicates, rounding and the example renderer. The crate is PSG-agnostic and
//! never executes arbitrary code.

mod ast;
mod calendar;
mod eval;
mod parser;
mod render;
mod typecheck;
mod types;

pub use ast::{
    unparse, Ast, BinaryOp, Identifier, Literal, NameError, Symbol, UnaryOp,
    PLUMB_EXPR_LANGUAGE_VERSION, RESERVED_WORDS,
};
pub use calendar::{CalendarDefinition, CalendarError, CalendarProvider, StaticCalendarProvider};
pub use eval::{
    eval, round_decimal, EvalContext, EvalError, PredicateError, PredicateProvider, RefValue,
    RoundingError, RoundingSpec, Value, ValueEnv, ValueEnvError,
};
pub use parser::{canonicalize, parse, ParseError};
pub use render::render_example;
pub use typecheck::{
    map_pilot_value_type, typecheck, TypeBindingError, TypeCheckError, TypeEnv, TypeEnvError,
    DAY_UNIT, WORKING_DAY_UNIT,
};
pub use types::{Ty, TypeError, Unit, MAX_DECIMAL_SCALE};
