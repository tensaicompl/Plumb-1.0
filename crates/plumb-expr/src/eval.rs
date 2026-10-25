//! PlumbExpr runtime values, evaluation and rounding (plan S2.5, Hotfix 038).
//!
//! Evaluation always type-checks first and then follows the static types: contextual enum
//! members, Int-to-Decimal promotion and the literal-zero rule are resolved exactly as the
//! checker resolved them. `and`, `or`, `->` and conditionals short-circuit. There is no Null:
//! a known identifier without a runtime value is `MissingValue`. Only the frozen callables
//! execute; the domain predicate and calendars are injected services. Arithmetic is checked
//! and uses `rust_decimal` only.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use plumb_core::{Id, Timestamp};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use thiserror::Error;

use crate::ast::{Ast, BinaryOp, Identifier, Literal, Symbol, UnaryOp};
use crate::calendar::{CalendarError, CalendarProvider};
use crate::typecheck::{
    check, check_pair, is_integral, map_pilot_value_type, resolve, unit, Resolved,
    TypeBindingError, TypeCheckError, TypeEnv, DAY_UNIT, WORKING_DAY_UNIT,
};
use crate::types::{Ty, Unit, MAX_DECIMAL_SCALE};

// ============================================================================ values

/// A runtime value. There is no Null, Any, Object or Function value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Decimal(Decimal),
    Bool(bool),
    String(String),
    Date(time::Date),
    DateTime(Timestamp),
    Duration { value: Decimal, unit: Unit },
    Quantity { value: Decimal, unit: Unit },
    Enum { enum_ref: Id, member: Symbol },
    Ref(RefValue),
    List(Vec<Value>),
}

/// A reference: its type, its identity and the field values carried for navigation. Only
/// `value_ref` is the reference identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefValue {
    pub type_ref: Id,
    pub value_ref: Id,
    pub fields: BTreeMap<Symbol, Value>,
}

/// Why a ValueEnv binding cannot be added.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValueEnvError {
    #[error("root {name} is already bound to another value")]
    BindingConflict { name: Symbol },
}

/// The explicit runtime values of root symbols.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValueEnv {
    roots: BTreeMap<Symbol, Value>,
}

impl ValueEnv {
    /// An empty environment.
    pub fn new() -> ValueEnv {
        ValueEnv::default()
    }

    /// Binds a root symbol; rebinding to the identical value is idempotent.
    pub fn bind(&mut self, name: Symbol, value: Value) -> Result<(), ValueEnvError> {
        match self.roots.get(&name) {
            Some(existing) if *existing != value => Err(ValueEnvError::BindingConflict { name }),
            _ => {
                self.roots.insert(name, value);
                Ok(())
            }
        }
    }

    /// The value of a root symbol.
    pub fn get(&self, name: &Symbol) -> Option<&Value> {
        self.roots.get(name)
    }

    /// The value at a dotted path, walking Ref fields; `None` when any step is absent or a
    /// non-Ref is navigated.
    pub fn lookup(&self, identifier: &Identifier) -> Option<&Value> {
        match self.navigate(identifier) {
            Navigation::Found(value) => Some(value),
            Navigation::Absent | Navigation::ThroughNonRef => None,
        }
    }

    /// The value at a dotted path, telling an absent step from a non-Ref value that a further
    /// segment navigates through (Fable #150).
    fn navigate(&self, identifier: &Identifier) -> Navigation<'_> {
        let segments = identifier.segments();
        let Some(mut current) = self.roots.get(&segments[0]) else {
            return Navigation::Absent;
        };
        for field in &segments[1..] {
            match current {
                Value::Ref(r) => match r.fields.get(field) {
                    Some(next) => current = next,
                    None => return Navigation::Absent,
                },
                _ => return Navigation::ThroughNonRef,
            }
        }
        Navigation::Found(current)
    }
}

/// The outcome of walking a dotted path through a ValueEnv.
enum Navigation<'a> {
    Found(&'a Value),
    Absent,
    ThroughNonRef,
}

fn non_ref_step(identifier: &Identifier) -> EvalError {
    value_mismatch(&format!("{identifier}: navigates through a non-Ref value"))
}

/// Why the injected domain predicate failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("predicate failure: {reason}")]
pub struct PredicateError {
    pub reason: String,
}

/// The one injected domain predicate of the pilot callable set.
pub trait PredicateProvider {
    /// Whether `actor` is the direct manager of `employee`.
    fn direct_manager(&self, actor: &RefValue, employee: &RefValue)
        -> Result<bool, PredicateError>;
}

/// Everything an evaluation may consult.
pub struct EvalContext<'a> {
    pub types: &'a TypeEnv,
    pub values: &'a ValueEnv,
    pub calendars: &'a dyn CalendarProvider,
    pub predicates: &'a dyn PredicateProvider,
}

/// Why an expression could not be evaluated. Nothing falls back to a default value.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EvalError {
    #[error("type error: {0}")]
    Type(#[from] TypeCheckError),
    #[error("missing value for {identifier}")]
    MissingValue { identifier: String },
    #[error("value does not match its type: {context}")]
    ValueTypeMismatch { context: String },
    #[error("division by zero")]
    DivisionByZero,
    #[error("arithmetic overflow")]
    Overflow,
    #[error("unit mismatch: {left} and {right}")]
    UnitMismatch { left: Unit, right: Unit },
    #[error("the period start is after its end")]
    InvalidPeriod,
    #[error("a period has exactly two dates, got {found}")]
    InvalidPeriodLength { found: usize },
    #[error("date arithmetic needs a whole number of days")]
    NonIntegralDateDuration,
    #[error("date out of range")]
    DateOutOfRange,
    /// The date is outside the business calendar's declared coverage (H0.10): the calculation
    /// is undecidable, never evaluated with an assumed working day.
    #[error("calendar {calendar_ref} does not cover {date}")]
    CalendarCoverageUnavailable { calendar_ref: Id, date: time::Date },
    #[error("calendar: {0}")]
    Calendar(CalendarError),
    #[error("{0}")]
    Predicate(#[from] PredicateError),
    #[error("{function} of an empty list")]
    EmptyAggregate { function: String },
}

impl From<CalendarError> for EvalError {
    fn from(e: CalendarError) -> EvalError {
        match e {
            CalendarError::CoverageUnavailable { calendar_ref, date } => {
                EvalError::CalendarCoverageUnavailable { calendar_ref, date }
            }
            other => EvalError::Calendar(other),
        }
    }
}

fn value_mismatch(context: &str) -> EvalError {
    EvalError::ValueTypeMismatch {
        context: context.to_owned(),
    }
}

// ============================================================================ helpers

fn to_decimal(value: &Value) -> Option<Decimal> {
    match value {
        Value::Int(i) => Some(Decimal::from(*i)),
        Value::Decimal(d) => Some(*d),
        _ => None,
    }
}

/// Whether `value` conforms to `ty` (exact units, enum and ref identities, decimal scale).
fn conforms(env: &TypeEnv, value: &Value, ty: &Ty) -> bool {
    match (value, ty) {
        (Value::Int(_), Ty::Int)
        | (Value::Bool(_), Ty::Bool)
        | (Value::String(_), Ty::String)
        | (Value::Date(_), Ty::Date)
        | (Value::DateTime(_), Ty::DateTime) => true,
        (Value::Decimal(d), Ty::Decimal(scale)) => d.scale() <= u32::from(*scale),
        (Value::Duration { unit, .. }, Ty::Duration(u))
        | (Value::Quantity { unit, .. }, Ty::Quantity(u)) => unit == u,
        (Value::Enum { enum_ref, member }, Ty::Enum(e)) => {
            enum_ref == e
                && env
                    .enum_members(e)
                    .is_some_and(|members| members.contains(member))
        }
        (Value::Ref(r), Ty::Ref(t)) => r.type_ref == *t,
        (Value::List(items), Ty::List(inner)) => items.iter().all(|v| conforms(env, v, inner)),
        _ => false,
    }
}

/// Promotes a value to its static type: Int to Decimal, literal zero to a zero quantity or
/// duration, lists element-wise.
fn coerce(value: Value, ty: &Ty) -> Value {
    match (value, ty) {
        (Value::Int(i), Ty::Decimal(_)) => Value::Decimal(Decimal::from(i)),
        (v @ (Value::Int(_) | Value::Decimal(_)), Ty::Quantity(u) | Ty::Duration(u)) => {
            let zero = to_decimal(&v).unwrap_or(Decimal::ZERO);
            if matches!(ty, Ty::Quantity(_)) {
                Value::Quantity {
                    value: zero,
                    unit: u.clone(),
                }
            } else {
                Value::Duration {
                    value: zero,
                    unit: u.clone(),
                }
            }
        }
        (Value::List(items), Ty::List(inner)) => {
            Value::List(items.into_iter().map(|v| coerce(v, inner)).collect())
        }
        (v, _) => v,
    }
}

fn as_bool(value: Value) -> Result<bool, EvalError> {
    match value {
        Value::Bool(b) => Ok(b),
        _ => Err(value_mismatch("expected Bool")),
    }
}

fn as_date(value: &Value) -> Result<time::Date, EvalError> {
    match value {
        Value::Date(d) => Ok(*d),
        _ => Err(value_mismatch("expected Date")),
    }
}

fn as_ref(value: Value) -> Result<RefValue, EvalError> {
    match value {
        Value::Ref(r) => Ok(r),
        _ => Err(value_mismatch("expected Ref")),
    }
}

fn values_equal(a: &Value, b: &Value) -> Result<bool, EvalError> {
    Ok(match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Int(_) | Value::Decimal(_), Value::Int(_) | Value::Decimal(_)) => {
            to_decimal(a) == to_decimal(b)
        }
        (Value::Quantity { value: x, unit: u }, Value::Quantity { value: y, unit: v })
        | (Value::Duration { value: x, unit: u }, Value::Duration { value: y, unit: v }) => {
            u == v && x == y
        }
        (Value::Quantity { value, .. } | Value::Duration { value, .. }, s)
        | (s, Value::Quantity { value, .. } | Value::Duration { value, .. })
            if to_decimal(s).is_some() =>
        {
            Some(*value) == to_decimal(s)
        }
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Date(x), Value::Date(y)) => x == y,
        (Value::DateTime(x), Value::DateTime(y)) => x == y,
        (
            Value::Enum {
                enum_ref: e,
                member: m,
            },
            Value::Enum {
                enum_ref: f,
                member: n,
            },
        ) => e == f && m == n,
        (Value::Ref(x), Value::Ref(y)) => x.type_ref == y.type_ref && x.value_ref == y.value_ref,
        (Value::List(x), Value::List(y)) => {
            if x.len() != y.len() {
                return Ok(false);
            }
            for (l, r) in x.iter().zip(y) {
                if !values_equal(l, r)? {
                    return Ok(false);
                }
            }
            true
        }
        _ => return Err(value_mismatch("incomparable values")),
    })
}

fn compare(a: &Value, b: &Value) -> Result<Ordering, EvalError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Int(_) | Value::Decimal(_), Value::Int(_) | Value::Decimal(_)) => {
            Ok(to_decimal(a).cmp(&to_decimal(b)))
        }
        (Value::Quantity { value: x, unit: u }, Value::Quantity { value: y, unit: v })
        | (Value::Duration { value: x, unit: u }, Value::Duration { value: y, unit: v })
            if u == v =>
        {
            Ok(x.cmp(y))
        }
        (Value::Quantity { value, .. } | Value::Duration { value, .. }, s)
            if to_decimal(s).is_some() =>
        {
            Ok(value.cmp(&to_decimal(s).unwrap_or_default()))
        }
        (s, Value::Quantity { value, .. } | Value::Duration { value, .. })
            if to_decimal(s).is_some() =>
        {
            Ok(to_decimal(s).unwrap_or_default().cmp(value))
        }
        (Value::Date(x), Value::Date(y)) => Ok(x.cmp(y)),
        (Value::DateTime(x), Value::DateTime(y)) => Ok(x.cmp(y)),
        _ => Err(value_mismatch("unordered values")),
    }
}

fn decimal_op(op: BinaryOp, x: Decimal, y: Decimal) -> Result<Decimal, EvalError> {
    let result = match op {
        BinaryOp::Add => x.checked_add(y),
        BinaryOp::Subtract => x.checked_sub(y),
        BinaryOp::Multiply => x.checked_mul(y),
        BinaryOp::Divide | BinaryOp::Remainder if y.is_zero() => {
            return Err(EvalError::DivisionByZero)
        }
        BinaryOp::Divide => x.checked_div(y),
        BinaryOp::Remainder => x.checked_rem(y),
        _ => return Err(value_mismatch("not an arithmetic operator")),
    };
    result.ok_or(EvalError::Overflow)
}

fn int_op(op: BinaryOp, x: i64, y: i64) -> Result<Value, EvalError> {
    let checked = match op {
        BinaryOp::Add => x.checked_add(y),
        BinaryOp::Subtract => x.checked_sub(y),
        BinaryOp::Multiply => x.checked_mul(y),
        BinaryOp::Remainder if y == 0 => return Err(EvalError::DivisionByZero),
        BinaryOp::Remainder => x.checked_rem(y),
        BinaryOp::Divide => {
            return decimal_op(op, Decimal::from(x), Decimal::from(y)).map(Value::Decimal)
        }
        _ => return Err(value_mismatch("not an arithmetic operator")),
    };
    checked.map(Value::Int).ok_or(EvalError::Overflow)
}

fn date_shift(date: time::Date, days: &Decimal, subtract: bool) -> Result<time::Date, EvalError> {
    if !is_integral(days) {
        return Err(EvalError::NonIntegralDateDuration);
    }
    let n = days.to_i64().ok_or(EvalError::DateOutOfRange)?;
    // Bounded well within time::Duration's range; beyond it no valid Date exists anyway.
    if n.unsigned_abs() > 10_000 * 366 {
        return Err(EvalError::DateOutOfRange);
    }
    let shift = time::Duration::days(n);
    let shifted = if subtract {
        date.checked_sub(shift)
    } else {
        date.checked_add(shift)
    };
    // `time` admits years down to -9999; PlumbExpr dates are years 0000..=9999 only.
    shifted
        .filter(|d| (0..=9999).contains(&d.year()))
        .ok_or(EvalError::DateOutOfRange)
}

fn arithmetic(op: BinaryOp, a: Value, b: Value) -> Result<Value, EvalError> {
    let unit_mismatch = |u: &Unit, v: &Unit| EvalError::UnitMismatch {
        left: u.clone(),
        right: v.clone(),
    };
    match (&a, &b) {
        (Value::Int(x), Value::Int(y)) => int_op(op, *x, *y),
        (Value::Int(_) | Value::Decimal(_), Value::Int(_) | Value::Decimal(_)) => {
            let (x, y) = (
                to_decimal(&a).unwrap_or_default(),
                to_decimal(&b).unwrap_or_default(),
            );
            decimal_op(op, x, y).map(Value::Decimal)
        }
        (Value::Quantity { value: x, unit: u }, Value::Quantity { value: y, unit: v }) => {
            if u != v {
                return Err(unit_mismatch(u, v));
            }
            Ok(Value::Quantity {
                value: decimal_op(op, *x, *y)?,
                unit: u.clone(),
            })
        }
        (Value::Duration { value: x, unit: u }, Value::Duration { value: y, unit: v }) => {
            if u != v {
                return Err(unit_mismatch(u, v));
            }
            Ok(Value::Duration {
                value: decimal_op(op, *x, *y)?,
                unit: u.clone(),
            })
        }
        (Value::Quantity { value, unit } | Value::Duration { value, unit }, s)
            if to_decimal(s).is_some() =>
        {
            let result = decimal_op(op, *value, to_decimal(s).unwrap_or_default())?;
            Ok(match a {
                Value::Quantity { .. } => Value::Quantity {
                    value: result,
                    unit: unit.clone(),
                },
                _ => Value::Duration {
                    value: result,
                    unit: unit.clone(),
                },
            })
        }
        (s, Value::Quantity { value, unit } | Value::Duration { value, unit })
            if to_decimal(s).is_some() && op == BinaryOp::Multiply =>
        {
            let result = decimal_op(op, to_decimal(s).unwrap_or_default(), *value)?;
            Ok(match b {
                Value::Quantity { .. } => Value::Quantity {
                    value: result,
                    unit: unit.clone(),
                },
                _ => Value::Duration {
                    value: result,
                    unit: unit.clone(),
                },
            })
        }
        (Value::Date(d), Value::Duration { value, unit }) if unit.as_str() == DAY_UNIT => {
            date_shift(*d, value, op == BinaryOp::Subtract).map(Value::Date)
        }
        (Value::Date(x), Value::Date(y)) if op == BinaryOp::Subtract => Ok(Value::Duration {
            value: Decimal::from((*x - *y).whole_days()),
            unit: unit(DAY_UNIT),
        }),
        _ => Err(value_mismatch("invalid arithmetic operands")),
    }
}

// ============================================================================ evaluator

/// Evaluates an expression. It is type-checked first and never evaluated dynamically typed.
pub fn eval(ast: &Ast, context: &EvalContext<'_>) -> Result<Value, EvalError> {
    let ty = check(context.types, ast, None)?;
    let value = Evaluator { context }.node(ast, None)?;
    Ok(coerce(value, &ty))
}

struct Evaluator<'c, 'a> {
    context: &'c EvalContext<'a>,
}

impl Evaluator<'_, '_> {
    fn types(&self) -> &TypeEnv {
        self.context.types
    }

    /// The value at a bound path, checked against its static type.
    fn lookup(&self, identifier: &Identifier, ty: &Ty) -> Result<Value, EvalError> {
        let value = match self.context.values.navigate(identifier) {
            Navigation::Found(value) => value,
            Navigation::Absent => {
                return Err(EvalError::MissingValue {
                    identifier: identifier.to_string(),
                })
            }
            Navigation::ThroughNonRef => return Err(non_ref_step(identifier)),
        };
        self.check_path(identifier)?;
        if conforms(self.types(), value, ty) {
            Ok(value.clone())
        } else {
            Err(value_mismatch(&format!("{identifier} is not a {ty:?}")))
        }
    }

    /// Every navigated Ref has its declared ref type.
    fn check_path(&self, identifier: &Identifier) -> Result<(), EvalError> {
        let segments = identifier.segments();
        let Some(Ty::Ref(mut ref_type)) = self.types().root(&segments[0]).cloned() else {
            return Ok(());
        };
        let mut current = self.context.values.get(&segments[0]);
        for field in &segments[1..] {
            let Some(Value::Ref(r)) = current else {
                return Err(value_mismatch(&format!("{identifier}: not a Ref value")));
            };
            if r.type_ref != ref_type {
                return Err(value_mismatch(&format!("{identifier}: wrong ref type")));
            }
            current = r.fields.get(field);
            if let Some(Ty::Ref(next)) = self.types().field(&ref_type, field).cloned() {
                ref_type = next;
            }
        }
        Ok(())
    }

    fn node(&self, ast: &Ast, expected: Option<&Ty>) -> Result<Value, EvalError> {
        match ast {
            Ast::Literal(literal) => Ok(match literal {
                Literal::Int(i) => Value::Int(*i),
                Literal::Decimal(d) => Value::Decimal(*d),
                Literal::Bool(b) => Value::Bool(*b),
                Literal::String(s) => Value::String(s.clone()),
                Literal::Date(d) => Value::Date(*d),
                Literal::DateTime(t) => Value::DateTime(*t),
                Literal::Duration { value, unit } => Value::Duration {
                    value: *value,
                    unit: unit.clone(),
                },
                Literal::Quantity { value, unit } => Value::Quantity {
                    value: *value,
                    unit: unit.clone(),
                },
            }),
            Ast::Identifier(identifier) => match resolve(self.types(), identifier, expected)? {
                Resolved::EnumMember(enum_ref, member) => Ok(Value::Enum { enum_ref, member }),
                Resolved::Bound(ty) => self.lookup(identifier, &ty),
            },
            Ast::List(items) => {
                let ty = check(self.types(), ast, expected)?;
                let Ty::List(element) = &ty else {
                    return Err(value_mismatch("list type"));
                };
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(coerce(self.node(item, Some(element))?, element));
                }
                Ok(Value::List(values))
            }
            Ast::Call { function, args } => self.call(function, args),
            Ast::Unary { op, expr } => {
                let value = self.node(expr, None)?;
                match (op, value) {
                    (UnaryOp::Not, v) => Ok(Value::Bool(!as_bool(v)?)),
                    (UnaryOp::Negate, Value::Int(i)) => {
                        i.checked_neg().map(Value::Int).ok_or(EvalError::Overflow)
                    }
                    (UnaryOp::Negate, Value::Decimal(d)) => Ok(Value::Decimal(-d)),
                    (UnaryOp::Negate, Value::Quantity { value, unit }) => Ok(Value::Quantity {
                        value: -value,
                        unit,
                    }),
                    (UnaryOp::Negate, Value::Duration { value, unit }) => Ok(Value::Duration {
                        value: -value,
                        unit,
                    }),
                    _ => Err(value_mismatch("negation")),
                }
            }
            Ast::Binary { op, left, right } => self.binary(*op, left, right),
            Ast::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let ty = check(self.types(), ast, expected)?;
                let branch = if as_bool(self.node(condition, Some(&Ty::Bool))?)? {
                    then_expr
                } else {
                    else_expr
                };
                Ok(coerce(self.node(branch, Some(&ty))?, &ty))
            }
        }
    }

    fn binary(&self, op: BinaryOp, left: &Ast, right: &Ast) -> Result<Value, EvalError> {
        let types = self.types();
        match op {
            BinaryOp::And => Ok(Value::Bool(
                as_bool(self.node(left, Some(&Ty::Bool))?)?
                    && as_bool(self.node(right, Some(&Ty::Bool))?)?,
            )),
            BinaryOp::Or => Ok(Value::Bool(
                as_bool(self.node(left, Some(&Ty::Bool))?)?
                    || as_bool(self.node(right, Some(&Ty::Bool))?)?,
            )),
            BinaryOp::Implies => Ok(Value::Bool(
                !as_bool(self.node(left, Some(&Ty::Bool))?)?
                    || as_bool(self.node(right, Some(&Ty::Bool))?)?,
            )),
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::LessThan
            | BinaryOp::LessThanOrEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterThanOrEqual => {
                let (lt, rt) = check_pair(types, left, right)?;
                let l = self.node(left, Some(&rt))?;
                let r = self.node(right, Some(&lt))?;
                let result = match op {
                    BinaryOp::Equal => values_equal(&l, &r)?,
                    BinaryOp::NotEqual => !values_equal(&l, &r)?,
                    BinaryOp::LessThan => compare(&l, &r)? == Ordering::Less,
                    BinaryOp::LessThanOrEqual => compare(&l, &r)? != Ordering::Greater,
                    BinaryOp::GreaterThan => compare(&l, &r)? == Ordering::Greater,
                    _ => compare(&l, &r)? != Ordering::Less,
                };
                Ok(Value::Bool(result))
            }
            BinaryOp::In => {
                let (l, r) = match check(types, left, None) {
                    Ok(lt) => (
                        self.node(left, None)?,
                        self.node(right, Some(&Ty::List(Box::new(lt))))?,
                    ),
                    Err(_) => {
                        let rt = check(types, right, None)?;
                        let Ty::List(element) = &rt else {
                            return Err(value_mismatch("in"));
                        };
                        (self.node(left, Some(element))?, self.node(right, None)?)
                    }
                };
                let Value::List(items) = r else {
                    return Err(value_mismatch("in"));
                };
                for item in &items {
                    if values_equal(&l, item)? {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            arithmetic_op => {
                let l = self.node(left, None)?;
                let r = self.node(right, None)?;
                arithmetic(arithmetic_op, l, r)
            }
        }
    }

    fn call(&self, function: &Symbol, args: &[Ast]) -> Result<Value, EvalError> {
        let name = function.as_str();
        match name {
            "exists" => {
                let Ast::Identifier(identifier) = &args[0] else {
                    return Err(value_mismatch("exists argument"));
                };
                let Resolved::Bound(ty) = resolve(self.types(), identifier, None)? else {
                    return Err(value_mismatch("exists argument"));
                };
                match self.context.values.navigate(identifier) {
                    Navigation::Absent => Ok(Value::Bool(false)),
                    Navigation::ThroughNonRef => Err(non_ref_step(identifier)),
                    Navigation::Found(value) => {
                        self.check_path(identifier)?;
                        if conforms(self.types(), value, &ty) {
                            Ok(Value::Bool(true))
                        } else {
                            Err(value_mismatch(&format!("{identifier} is not a {ty:?}")))
                        }
                    }
                }
            }
            "direct_manager" => {
                let actor = as_ref(self.node(&args[0], None)?)?;
                let employee = as_ref(self.node(&args[1], None)?)?;
                Ok(Value::Bool(
                    self.context.predicates.direct_manager(&actor, &employee)?,
                ))
            }
            "working_days" => {
                let Value::List(period) =
                    self.node(&args[0], Some(&Ty::List(Box::new(Ty::Date))))?
                else {
                    return Err(value_mismatch("working_days period"));
                };
                if period.len() != 2 {
                    return Err(EvalError::InvalidPeriodLength {
                        found: period.len(),
                    });
                }
                let (start, end) = (as_date(&period[0])?, as_date(&period[1])?);
                let calendar = as_ref(self.node(&args[1], None)?)?.value_ref;
                let inclusive = as_bool(self.node(&args[2], None)?)?;
                if start > end {
                    return Err(EvalError::InvalidPeriod);
                }
                let mut count: i64 = 0;
                let mut day = start;
                loop {
                    if (day < end || inclusive)
                        && self.context.calendars.is_working_day(&calendar, day)?
                    {
                        count += 1;
                    }
                    if day >= end {
                        break;
                    }
                    day = day.next_day().ok_or(EvalError::DateOutOfRange)?;
                }
                Ok(Value::Quantity {
                    value: Decimal::from(count),
                    unit: unit(WORKING_DAY_UNIT),
                })
            }
            "days_between" => {
                let start = as_date(&self.node(&args[0], None)?)?;
                let end = as_date(&self.node(&args[1], None)?)?;
                if start > end {
                    return Err(EvalError::InvalidPeriod);
                }
                Ok(Value::Int((end - start).whole_days()))
            }
            "count" | "any" | "all" => {
                let items = match &args[0] {
                    Ast::List(items) if items.is_empty() => Vec::new(),
                    arg => match self.node(arg, None)? {
                        Value::List(items) => items,
                        _ => return Err(value_mismatch(name)),
                    },
                };
                match name {
                    "count" => i64::try_from(items.len())
                        .map(Value::Int)
                        .map_err(|_| EvalError::Overflow),
                    "any" => {
                        let mut result = false;
                        for item in items {
                            result |= as_bool(item)?;
                        }
                        Ok(Value::Bool(result))
                    }
                    _ => {
                        let mut result = true;
                        for item in items {
                            result &= as_bool(item)?;
                        }
                        Ok(Value::Bool(result))
                    }
                }
            }
            // sum, min, max (typecheck admits no other name)
            _ => {
                let Ty::List(element) = check(self.types(), &args[0], None)? else {
                    return Err(value_mismatch(name));
                };
                let Value::List(items) = self.node(&args[0], None)? else {
                    return Err(value_mismatch(name));
                };
                let mut items = items.into_iter();
                let mut acc = items.next().ok_or_else(|| EvalError::EmptyAggregate {
                    function: name.to_owned(),
                })?;
                for item in items {
                    acc = match name {
                        "sum" => arithmetic(BinaryOp::Add, acc, item)?,
                        "min" if compare(&item, &acc)? == Ordering::Less => item,
                        "max" if compare(&item, &acc)? == Ordering::Greater => item,
                        _ => acc,
                    };
                }
                Ok(coerce(acc, &element))
            }
        }
    }
}

// ============================================================================ display

fn write_string(f: &mut fmt::Formatter<'_>, value: &str) -> fmt::Result {
    f.write_str("\"")?;
    for c in value.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            '\u{8}' => f.write_str("\\b")?,
            '\u{c}' => f.write_str("\\f")?,
            other => write!(f, "{other}")?,
        }
    }
    f.write_str("\"")
}

/// The deterministic example-renderer form of a value.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{i}"),
            Value::Decimal(d) => write!(f, "{d}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::String(s) => write_string(f, s),
            Value::Date(d) => write!(
                f,
                "{:04}-{:02}-{:02}",
                d.year(),
                u8::from(d.month()),
                d.day()
            ),
            Value::DateTime(t) => write!(f, "{t}"),
            Value::Duration { value, unit } | Value::Quantity { value, unit } => {
                write!(f, "{value} {unit}")
            }
            Value::Enum { member, .. } => write!(f, "{member}"),
            Value::Ref(r) => write!(f, "{}", r.value_ref),
            Value::List(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
        }
    }
}

// ============================================================================ declared results

/// A declared calculation result (`Calculation.result_type` and `Calculation.unit`): its type
/// and, for `Decimal(n)`, the scale `n`. The scale applies equally to a scalar Decimal result
/// and to a unit-bearing Quantity result (H0.10, architecture §33.11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredResult {
    pub ty: Ty,
    pub scale: Option<u8>,
}

impl DeclaredResult {
    /// The declared result of a canonical pilot value type and optional unit, mapped exactly as
    /// [`map_pilot_value_type`] maps it.
    pub fn from_value_type(
        value_type: &str,
        unit: Option<&str>,
    ) -> Result<DeclaredResult, TypeBindingError> {
        let ty = map_pilot_value_type(value_type, unit, None)?;
        let scale = match map_pilot_value_type(value_type, None, None) {
            Ok(Ty::Decimal(scale)) => Some(scale),
            _ => None,
        };
        Ok(DeclaredResult { ty, scale })
    }
}

/// Why an evaluated value does not satisfy its declared result.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResultCheckError {
    #[error("rounding: {0}")]
    Rounding(RoundingError),
    #[error("rounding applies only to Decimal and Quantity results")]
    RoundingNotApplicable,
    #[error("result scale {scale} exceeds the declared scale {declared}")]
    ScaleExceeded { scale: u32, declared: u8 },
    #[error("result unit {actual:?} does not match the declared unit {expected:?}")]
    UnitMismatch {
        expected: Option<Unit>,
        actual: Option<Unit>,
    },
    #[error("result does not match the declared type {expected:?}")]
    TypeMismatch { expected: Ty },
}

/// Checks an evaluated calculation value against its declared result in the frozen order
/// (after parse, type check and evaluation): round per the declared rounding, then check the
/// declared `Decimal(n)` scale, then the unit, then the remaining type. An Int value promotes
/// to a declared Decimal first. The scale is the value's own (trailing zeros removed), so a
/// division followed by the declared business rounding can satisfy its declared scale.
pub fn check_result(
    value: Value,
    declared: &DeclaredResult,
    rounding: Option<&RoundingSpec>,
) -> Result<Value, ResultCheckError> {
    let value = match (value, &declared.ty) {
        (Value::Int(i), Ty::Decimal(_)) => Value::Decimal(Decimal::from(i)),
        (v, _) => v,
    };
    let round = |d: Decimal, spec: &RoundingSpec| {
        round_decimal(d, spec).map_err(ResultCheckError::Rounding)
    };
    let value = match (rounding, value) {
        (None, v) => v,
        (Some(spec), Value::Decimal(d)) => Value::Decimal(round(d, spec)?),
        (Some(spec), Value::Quantity { value, unit }) => Value::Quantity {
            value: round(value, spec)?,
            unit,
        },
        (Some(_), _) => return Err(ResultCheckError::RoundingNotApplicable),
    };
    if let (Some(declared_scale), Value::Decimal(d) | Value::Quantity { value: d, .. }) =
        (declared.scale, &value)
    {
        let scale = d.normalize().scale();
        if scale > u32::from(declared_scale) {
            return Err(ResultCheckError::ScaleExceeded {
                scale,
                declared: declared_scale,
            });
        }
    }
    let expected_unit = match &declared.ty {
        Ty::Quantity(u) => Some(u.clone()),
        _ => None,
    };
    let actual_unit = match &value {
        Value::Quantity { unit, .. } => Some(unit.clone()),
        _ => None,
    };
    let numeric = matches!(
        value,
        Value::Int(_) | Value::Decimal(_) | Value::Quantity { .. }
    );
    if numeric && expected_unit != actual_unit {
        return Err(ResultCheckError::UnitMismatch {
            expected: expected_unit,
            actual: actual_unit,
        });
    }
    let matches = matches!(
        (&value, &declared.ty),
        (Value::Int(_), Ty::Int)
            | (Value::Decimal(_), Ty::Decimal(_))
            | (Value::Quantity { .. }, Ty::Quantity(_))
            | (Value::Bool(_), Ty::Bool)
            | (Value::String(_), Ty::String)
            | (Value::Date(_), Ty::Date)
            | (Value::DateTime(_), Ty::DateTime)
    );
    if !matches {
        return Err(ResultCheckError::TypeMismatch {
            expected: declared.ty.clone(),
        });
    }
    Ok(value)
}

// ============================================================================ rounding

/// A deterministic rounding rule; its canonical string is the `Calculation.rounding` form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundingSpec {
    /// Nearest at `scale`; midpoints away from zero.
    HalfUp { scale: u8 },
    /// Nearest at `scale`; midpoints to the even final digit.
    HalfEven { scale: u8 },
    /// Toward negative infinity at `scale`.
    Floor { scale: u8 },
    /// Toward positive infinity at `scale`.
    Ceil { scale: u8 },
    /// Nearest multiple of `step`; ties away from zero; result at the step's scale.
    ToStep { step: Decimal },
}

/// Why a rounding rule or value is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RoundingError {
    #[error("rounding scale {0} exceeds {MAX_DECIMAL_SCALE}")]
    InvalidScale(u32),
    #[error("rounding step must be a positive exact decimal")]
    InvalidStep,
    #[error("invalid rounding spec {0:?}")]
    InvalidFormat(String),
    #[error("rounding overflow")]
    Overflow,
}

/// Rounds `value` by `spec` with `rust_decimal` only.
pub fn round_decimal(value: Decimal, spec: &RoundingSpec) -> Result<Decimal, RoundingError> {
    let at_scale = |scale: u8, strategy: RoundingStrategy| {
        if scale > MAX_DECIMAL_SCALE {
            return Err(RoundingError::InvalidScale(u32::from(scale)));
        }
        let mut rounded = value.round_dp_with_strategy(u32::from(scale), strategy);
        rescaled_exactly(&mut rounded, u32::from(scale))?;
        Ok(rounded)
    };
    match spec {
        RoundingSpec::HalfUp { scale } => at_scale(*scale, RoundingStrategy::MidpointAwayFromZero),
        RoundingSpec::HalfEven { scale } => at_scale(*scale, RoundingStrategy::MidpointNearestEven),
        RoundingSpec::Floor { scale } => at_scale(*scale, RoundingStrategy::ToNegativeInfinity),
        RoundingSpec::Ceil { scale } => at_scale(*scale, RoundingStrategy::ToPositiveInfinity),
        RoundingSpec::ToStep { step } => {
            if *step <= Decimal::ZERO {
                return Err(RoundingError::InvalidStep);
            }
            let multiples = value
                .checked_div(*step)
                .ok_or(RoundingError::Overflow)?
                .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero);
            let mut rounded = multiples
                .checked_mul(*step)
                .ok_or(RoundingError::Overflow)?;
            rescaled_exactly(&mut rounded, step.scale())?;
            Ok(rounded)
        }
    }
}

/// `rescale` saturates when the value cannot carry `scale` fractional digits in 96 bits; that
/// is an overflow, never a silently shorter scale (Fable #148).
fn rescaled_exactly(value: &mut Decimal, scale: u32) -> Result<(), RoundingError> {
    value.rescale(scale);
    if value.scale() == scale {
        Ok(())
    } else {
        Err(RoundingError::Overflow)
    }
}

impl fmt::Display for RoundingSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RoundingSpec::HalfUp { scale } => write!(f, "half_up({scale})"),
            RoundingSpec::HalfEven { scale } => write!(f, "half_even({scale})"),
            RoundingSpec::Floor { scale } => write!(f, "floor({scale})"),
            RoundingSpec::Ceil { scale } => write!(f, "ceil({scale})"),
            RoundingSpec::ToStep { step } => write!(f, "to_step({step})"),
        }
    }
}

impl FromStr for RoundingSpec {
    type Err = RoundingError;

    /// Exactly `half_up(n)`, `half_even(n)`, `floor(n)`, `ceil(n)` or `to_step(d)`, case-
    /// sensitive, without whitespace.
    fn from_str(s: &str) -> Result<RoundingSpec, RoundingError> {
        let invalid = || RoundingError::InvalidFormat(s.to_owned());
        let (name, rest) = s.split_once('(').ok_or_else(invalid)?;
        let argument = rest.strip_suffix(')').ok_or_else(invalid)?;
        let canonical_integer = |a: &str| {
            a == "0"
                || (!a.is_empty() && !a.starts_with('0') && a.bytes().all(|b| b.is_ascii_digit()))
        };
        let scale = || -> Result<u8, RoundingError> {
            if !canonical_integer(argument) {
                return Err(invalid());
            }
            let value: u32 = argument.parse().map_err(|_| invalid())?;
            if value > u32::from(MAX_DECIMAL_SCALE) {
                return Err(RoundingError::InvalidScale(value));
            }
            u8::try_from(value).map_err(|_| RoundingError::InvalidScale(value))
        };
        match name {
            "half_up" => Ok(RoundingSpec::HalfUp { scale: scale()? }),
            "half_even" => Ok(RoundingSpec::HalfEven { scale: scale()? }),
            "floor" => Ok(RoundingSpec::Floor { scale: scale()? }),
            "ceil" => Ok(RoundingSpec::Ceil { scale: scale()? }),
            "to_step" => {
                let (integer, fraction) = match argument.split_once('.') {
                    Some((i, f)) => (i, Some(f)),
                    None => (argument, None),
                };
                let shaped = canonical_integer(integer)
                    && fraction
                        .is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()));
                let step = shaped
                    .then(|| Decimal::from_str(argument).ok())
                    .flatten()
                    .filter(|d| d.to_string() == argument && *d > Decimal::ZERO)
                    .ok_or(RoundingError::InvalidStep)?;
                Ok(RoundingSpec::ToStep { step })
            }
            _ => Err(invalid()),
        }
    }
}
