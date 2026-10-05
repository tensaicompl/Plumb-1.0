//! PlumbExpr type environment and type checker (plan S2.5, Hotfix 038).
//!
//! `TypeEnv` is an explicit, deterministic input: root bindings, Ref field schemas, enum
//! members and the calendar ref type. Bare names resolve only through root bindings or, where
//! the context expects `Enum(E)`, as a registered member of E. The checker is pure and
//! PSG-agnostic; it never resolves anything by searching names.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use rust_decimal::Decimal;
use thiserror::Error;

use crate::ast::{Ast, BinaryOp, Identifier, Literal, Symbol, UnaryOp};
use crate::types::{Ty, TypeError, Unit, MAX_DECIMAL_SCALE};

/// The unit of calendar-day durations in date arithmetic.
pub const DAY_UNIT: &str = "day";

/// The unit of `working_days` results.
pub const WORKING_DAY_UNIT: &str = "working_day";

// ============================================================================ errors

/// Why a TypeEnv binding cannot be added.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TypeEnvError {
    #[error("root {name} is already bound to another type")]
    BindingConflict { name: Symbol },
    #[error("field {field} of {ref_type} is already bound to another type")]
    FieldConflict { ref_type: Id, field: Symbol },
    #[error("enum {enum_ref} is already defined with other members")]
    EnumConflict { enum_ref: Id },
    #[error("the calendar ref type is already {existing}")]
    CalendarTypeConflict { existing: Id },
    #[error("invalid type: {0}")]
    InvalidType(#[from] TypeError),
}

/// Why a pilot PSG value type cannot be mapped to a `Ty`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TypeBindingError {
    #[error("unsupported value type {0:?}")]
    UnsupportedValueType(String),
    #[error("an Enum value type needs an explicit enum identity")]
    MissingEnumIdentity,
    #[error("unit on non-numeric value type {0:?}")]
    UnitOnNonNumericType(String),
    #[error("invalid unit {0:?}")]
    InvalidUnit(String),
}

/// Why an expression is not well-typed. Diagnostic prose is not part of the contract.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TypeCheckError {
    #[error("unknown identifier {name}")]
    UnknownIdentifier { name: String },
    #[error("{ref_type} has no field {field}")]
    UnknownRefField { ref_type: Id, field: String },
    #[error("cannot navigate {path}: not a Ref")]
    CannotNavigateNonRef { path: String },
    #[error("{member} is not a member of enum {enum_ref}")]
    UnknownEnumMember { enum_ref: Id, member: String },
    #[error("unknown function {name}")]
    UnknownFunction { name: String },
    #[error("function {name} is deferred")]
    DeferredFunction { name: String },
    #[error("{function} takes {expected} arguments, got {found}")]
    WrongArity {
        function: String,
        expected: usize,
        found: usize,
    },
    #[error("type mismatch in {context}: found {found:?}")]
    TypeMismatch { context: String, found: Ty },
    #[error("unit mismatch: {left} and {right}")]
    UnitMismatch { left: Unit, right: Unit },
    #[error("invalid operands for {op}: {left:?} and {right:?}")]
    InvalidOperand {
        op: String,
        left: Ty,
        right: Option<Ty>,
    },
    #[error("the element type of [] is unknown")]
    EmptyListTypeUnknown,
    #[error("incompatible list elements {first:?} and {other:?}")]
    IncompatibleListElements { first: Ty, other: Ty },
    #[error("incompatible conditional branches {then_ty:?} and {else_ty:?}")]
    IncompatibleConditionalBranches { then_ty: Ty, else_ty: Ty },
    #[error("no calendar ref type is configured")]
    MissingCalendarType,
    #[error("invalid working_days period")]
    InvalidPeriodType,
}

// ============================================================================ TypeEnv

/// The explicit type environment of an expression.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeEnv {
    roots: BTreeMap<Symbol, Ty>,
    ref_fields: BTreeMap<Id, BTreeMap<Symbol, Ty>>,
    enum_members: BTreeMap<Id, BTreeSet<Symbol>>,
    calendar_ref_type: Option<Id>,
}

impl TypeEnv {
    /// An empty environment.
    pub fn new() -> TypeEnv {
        TypeEnv::default()
    }

    /// Binds a root symbol; rebinding to the identical type is idempotent.
    pub fn bind_root(&mut self, name: Symbol, ty: Ty) -> Result<(), TypeEnvError> {
        ty.validate()?;
        match self.roots.get(&name) {
            Some(existing) if *existing != ty => Err(TypeEnvError::BindingConflict { name }),
            _ => {
                self.roots.insert(name, ty);
                Ok(())
            }
        }
    }

    /// Declares a field of a Ref type; rebinding to the identical type is idempotent.
    pub fn bind_field(&mut self, ref_type: Id, field: Symbol, ty: Ty) -> Result<(), TypeEnvError> {
        ty.validate()?;
        let fields = self.ref_fields.entry(ref_type.clone()).or_default();
        match fields.get(&field) {
            Some(existing) if *existing != ty => {
                Err(TypeEnvError::FieldConflict { ref_type, field })
            }
            _ => {
                fields.insert(field, ty);
                Ok(())
            }
        }
    }

    /// Defines the members of an enum; redefinition with the identical set is idempotent.
    pub fn define_enum(
        &mut self,
        enum_ref: Id,
        members: impl IntoIterator<Item = Symbol>,
    ) -> Result<(), TypeEnvError> {
        let members: BTreeSet<Symbol> = members.into_iter().collect();
        match self.enum_members.get(&enum_ref) {
            Some(existing) if *existing != members => Err(TypeEnvError::EnumConflict { enum_ref }),
            _ => {
                self.enum_members.insert(enum_ref, members);
                Ok(())
            }
        }
    }

    /// Sets the Ref type accepted as a calendar by `working_days`.
    pub fn set_calendar_ref_type(&mut self, ref_type: Id) -> Result<(), TypeEnvError> {
        match &self.calendar_ref_type {
            Some(existing) if *existing != ref_type => Err(TypeEnvError::CalendarTypeConflict {
                existing: existing.clone(),
            }),
            _ => {
                self.calendar_ref_type = Some(ref_type);
                Ok(())
            }
        }
    }

    /// The type of a root symbol.
    pub fn root(&self, name: &Symbol) -> Option<&Ty> {
        self.roots.get(name)
    }

    /// The type of a field of a Ref type.
    pub fn field(&self, ref_type: &Id, field: &Symbol) -> Option<&Ty> {
        self.ref_fields.get(ref_type).and_then(|f| f.get(field))
    }

    /// The members of an enum.
    pub fn enum_members(&self, enum_ref: &Id) -> Option<&BTreeSet<Symbol>> {
        self.enum_members.get(enum_ref)
    }

    /// The calendar Ref type, when configured.
    pub fn calendar_ref_type(&self) -> Option<&Id> {
        self.calendar_ref_type.as_ref()
    }
}

/// Maps a canonical pilot PSG value type to a `Ty`: exactly `String`, `Int`, `Bool`, `Date`,
/// `DateTime`, `Decimal(n)` (n 0..=28) and `Enum` (identity mandatory). A numeric type with a
/// unit maps to `Quantity(unit)`. Nothing else is guessed.
pub fn map_pilot_value_type(
    value_type: &str,
    unit: Option<&str>,
    enum_ref: Option<&Id>,
) -> Result<Ty, TypeBindingError> {
    let unsupported = || TypeBindingError::UnsupportedValueType(value_type.to_owned());
    let base = match value_type {
        "String" => Ty::String,
        "Int" => Ty::Int,
        "Bool" => Ty::Bool,
        "Date" => Ty::Date,
        "DateTime" => Ty::DateTime,
        "Enum" => Ty::Enum(
            enum_ref
                .cloned()
                .ok_or(TypeBindingError::MissingEnumIdentity)?,
        ),
        other => {
            let digits = other
                .strip_prefix("Decimal(")
                .and_then(|rest| rest.strip_suffix(')'))
                .ok_or_else(unsupported)?;
            let canonical = digits == "0"
                || (!digits.is_empty()
                    && !digits.starts_with('0')
                    && digits.bytes().all(|b| b.is_ascii_digit()));
            let scale: u8 = digits
                .parse()
                .ok()
                .filter(|_| canonical)
                .ok_or_else(unsupported)?;
            if scale > MAX_DECIMAL_SCALE {
                return Err(unsupported());
            }
            Ty::Decimal(scale)
        }
    };
    match unit {
        None => Ok(base),
        Some(symbol) => match base {
            Ty::Int | Ty::Decimal(_) => {
                Ok(Ty::Quantity(Unit::new(symbol).map_err(|_| {
                    TypeBindingError::InvalidUnit(symbol.to_owned())
                })?))
            }
            _ => Err(TypeBindingError::UnitOnNonNumericType(
                value_type.to_owned(),
            )),
        },
    }
}

// ============================================================================ helpers

pub(crate) fn unit(symbol: &str) -> Unit {
    Unit::new(symbol).unwrap_or_else(|_| unreachable!("{symbol} is a valid unit"))
}

pub(crate) fn is_scalar(ty: &Ty) -> bool {
    matches!(ty, Ty::Int | Ty::Decimal(_))
}

fn is_unit_bearing(ty: &Ty) -> bool {
    matches!(ty, Ty::Quantity(_) | Ty::Duration(_))
}

/// The exact numeric literal zero (`0`, `0.0`, ...), never a computed or negated zero.
pub(crate) fn is_zero_literal(ast: &Ast) -> bool {
    match ast {
        Ast::Literal(Literal::Int(0)) => true,
        Ast::Literal(Literal::Decimal(d)) => d.is_zero(),
        _ => false,
    }
}

fn bare_symbol(ast: &Ast) -> bool {
    matches!(ast, Ast::Identifier(id) if id.segments().len() == 1)
}

fn empty_list(ast: &Ast) -> bool {
    matches!(ast, Ast::List(items) if items.is_empty())
}

/// The scalar common type of addition, subtraction, comparison and list typing.
fn scalar_common(a: &Ty, b: &Ty) -> Option<Ty> {
    match (a, b) {
        (Ty::Int, Ty::Int) => Some(Ty::Int),
        (Ty::Int, Ty::Decimal(s)) | (Ty::Decimal(s), Ty::Int) => Some(Ty::Decimal(*s)),
        (Ty::Decimal(x), Ty::Decimal(y)) => Some(Ty::Decimal(*x.max(y))),
        _ => None,
    }
}

/// The common type of two types: identical, scalar-compatible or recursively list-compatible.
pub(crate) fn common(a: &Ty, b: &Ty) -> Option<Ty> {
    if a == b {
        return Some(a.clone());
    }
    if let Some(t) = scalar_common(a, b) {
        return Some(t);
    }
    match (a, b) {
        (Ty::List(x), Ty::List(y)) => common(x, y).map(|t| Ty::List(Box::new(t))),
        _ => None,
    }
}

fn mismatch(context: &str, found: Ty) -> TypeCheckError {
    TypeCheckError::TypeMismatch {
        context: context.to_owned(),
        found,
    }
}

fn invalid(op: BinaryOp, left: &Ty, right: &Ty) -> TypeCheckError {
    match (left, right) {
        (Ty::Quantity(u), Ty::Quantity(v)) | (Ty::Duration(u), Ty::Duration(v)) if u != v => {
            TypeCheckError::UnitMismatch {
                left: u.clone(),
                right: v.clone(),
            }
        }
        _ => TypeCheckError::InvalidOperand {
            op: op.token().to_owned(),
            left: left.clone(),
            right: Some(right.clone()),
        },
    }
}

// ============================================================================ checker

/// Type-checks an expression against an explicit environment.
pub fn typecheck(ast: &Ast, env: &TypeEnv) -> Result<Ty, TypeCheckError> {
    check(env, ast, None)
}

/// How an identifier resolves.
pub(crate) enum Resolved {
    /// A root binding, possibly navigated through Ref fields.
    Bound(Ty),
    /// A contextual enum member.
    EnumMember(Id, Symbol),
}

pub(crate) fn resolve(
    env: &TypeEnv,
    identifier: &Identifier,
    expected: Option<&Ty>,
) -> Result<Resolved, TypeCheckError> {
    let segments = identifier.segments();
    let root = &segments[0];
    let Some(mut ty) = env.root(root).cloned() else {
        if segments.len() == 1 {
            if let Some(Ty::Enum(enum_ref)) = expected {
                let known = env
                    .enum_members(enum_ref)
                    .is_some_and(|members| members.contains(root));
                return if known {
                    Ok(Resolved::EnumMember(enum_ref.clone(), root.clone()))
                } else {
                    Err(TypeCheckError::UnknownEnumMember {
                        enum_ref: enum_ref.clone(),
                        member: root.to_string(),
                    })
                };
            }
        }
        return Err(TypeCheckError::UnknownIdentifier {
            name: identifier.to_string(),
        });
    };
    for (i, field) in segments.iter().enumerate().skip(1) {
        let Ty::Ref(ref_type) = &ty else {
            let path: Vec<&str> = segments[..i].iter().map(Symbol::as_str).collect();
            return Err(TypeCheckError::CannotNavigateNonRef {
                path: path.join("."),
            });
        };
        ty =
            env.field(ref_type, field)
                .cloned()
                .ok_or_else(|| TypeCheckError::UnknownRefField {
                    ref_type: ref_type.clone(),
                    field: field.to_string(),
                })?;
    }
    Ok(Resolved::Bound(ty))
}

/// Types two operands that give each other context: the left first, unless it is a bare
/// unbound name or `[]` that needs the right operand's type.
pub(crate) fn check_pair(
    env: &TypeEnv,
    left: &Ast,
    right: &Ast,
) -> Result<(Ty, Ty), TypeCheckError> {
    match check(env, left, None) {
        Ok(lt) => {
            let rt = check(env, right, Some(&lt))?;
            Ok((lt, rt))
        }
        Err(
            e @ (TypeCheckError::UnknownIdentifier { .. } | TypeCheckError::EmptyListTypeUnknown),
        ) if bare_symbol(left) || empty_list(left) => {
            let rt = check(env, right, None).map_err(|_| e)?;
            let lt = check(env, left, Some(&rt))?;
            Ok((lt, rt))
        }
        Err(e) => Err(e),
    }
}

fn equality_compatible(lt: &Ty, rt: &Ty, left: &Ast, right: &Ast) -> bool {
    common(lt, rt).is_some()
        || (is_unit_bearing(lt) && is_zero_literal(right))
        || (is_unit_bearing(rt) && is_zero_literal(left))
}

fn ordering_compatible(lt: &Ty, rt: &Ty, left: &Ast, right: &Ast) -> bool {
    let orderable = |t: &Ty| {
        is_scalar(t)
            || matches!(
                t,
                Ty::Date | Ty::DateTime | Ty::Quantity(_) | Ty::Duration(_)
            )
    };
    if is_unit_bearing(lt) && is_zero_literal(right) || is_unit_bearing(rt) && is_zero_literal(left)
    {
        return true;
    }
    orderable(lt) && orderable(rt) && (scalar_common(lt, rt).is_some() || lt == rt)
}

/// The common type of conditional branches, including the literal-zero rule.
pub(crate) fn branch_common(
    then_ty: &Ty,
    else_ty: &Ty,
    then_expr: &Ast,
    else_expr: &Ast,
) -> Option<Ty> {
    if let Some(t) = common(then_ty, else_ty) {
        return Some(t);
    }
    if is_unit_bearing(then_ty) && is_zero_literal(else_expr) {
        return Some(then_ty.clone());
    }
    if is_unit_bearing(else_ty) && is_zero_literal(then_expr) {
        return Some(else_ty.clone());
    }
    None
}

fn literal_type(literal: &Literal) -> Ty {
    match literal {
        Literal::Int(_) => Ty::Int,
        Literal::Decimal(d) => Ty::Decimal(u8::try_from(d.scale()).unwrap_or(MAX_DECIMAL_SCALE)),
        Literal::Bool(_) => Ty::Bool,
        Literal::String(_) => Ty::String,
        Literal::Date(_) => Ty::Date,
        Literal::DateTime(_) => Ty::DateTime,
        Literal::Duration { unit, .. } => Ty::Duration(unit.clone()),
        Literal::Quantity { unit, .. } => Ty::Quantity(unit.clone()),
    }
}

fn require(context: &str, ty: Ty, expected: &Ty) -> Result<(), TypeCheckError> {
    if ty == *expected {
        Ok(())
    } else {
        Err(mismatch(context, ty))
    }
}

/// The static type of arithmetic.
pub(crate) fn arithmetic(op: BinaryOp, lt: &Ty, rt: &Ty) -> Result<Ty, TypeCheckError> {
    let day = Ty::Duration(unit(DAY_UNIT));
    let ok = match op {
        BinaryOp::Add | BinaryOp::Subtract => match (lt, rt) {
            (a, b) if is_scalar(a) && is_scalar(b) => scalar_common(a, b),
            (Ty::Quantity(u), Ty::Quantity(v)) | (Ty::Duration(u), Ty::Duration(v)) if u == v => {
                Some(lt.clone())
            }
            (Ty::Date, d) if *d == day => Some(Ty::Date),
            (Ty::Date, Ty::Date) if op == BinaryOp::Subtract => Some(day.clone()),
            _ => None,
        },
        BinaryOp::Multiply => match (lt, rt) {
            (Ty::Int, Ty::Int) => Some(Ty::Int),
            (Ty::Int, Ty::Decimal(s)) | (Ty::Decimal(s), Ty::Int) => Some(Ty::Decimal(*s)),
            (Ty::Decimal(a), Ty::Decimal(b)) => {
                Some(Ty::Decimal(a.saturating_add(*b).min(MAX_DECIMAL_SCALE)))
            }
            (s, q @ (Ty::Quantity(_) | Ty::Duration(_)))
            | (q @ (Ty::Quantity(_) | Ty::Duration(_)), s)
                if is_scalar(s) =>
            {
                Some(q.clone())
            }
            _ => None,
        },
        BinaryOp::Divide => match (lt, rt) {
            (a, b) if is_scalar(a) && is_scalar(b) => Some(Ty::Decimal(MAX_DECIMAL_SCALE)),
            (q @ (Ty::Quantity(_) | Ty::Duration(_)), s) if is_scalar(s) => Some(q.clone()),
            _ => None,
        },
        BinaryOp::Remainder => match (lt, rt) {
            (Ty::Int, Ty::Int) => Some(Ty::Int),
            (a, b) if is_scalar(a) && is_scalar(b) => scalar_common(a, b),
            _ => None,
        },
        _ => None,
    };
    ok.ok_or_else(|| invalid(op, lt, rt))
}

const FUNCTIONS: [&str; 10] = [
    "exists",
    "direct_manager",
    "working_days",
    "days_between",
    "sum",
    "min",
    "max",
    "count",
    "any",
    "all",
];

fn arity(function: &str, args: &[Ast], expected: usize) -> Result<(), TypeCheckError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(TypeCheckError::WrongArity {
            function: function.to_owned(),
            expected,
            found: args.len(),
        })
    }
}

fn check_call(env: &TypeEnv, function: &Symbol, args: &[Ast]) -> Result<Ty, TypeCheckError> {
    let name = function.as_str();
    match name {
        "as_of" => {
            return Err(TypeCheckError::DeferredFunction {
                name: name.to_owned(),
            })
        }
        _ if !FUNCTIONS.contains(&name) => {
            return Err(TypeCheckError::UnknownFunction {
                name: name.to_owned(),
            })
        }
        _ => {}
    }
    match name {
        "exists" => {
            arity(name, args, 1)?;
            let Ast::Identifier(identifier) = &args[0] else {
                return Err(TypeCheckError::InvalidOperand {
                    op: name.to_owned(),
                    left: check(env, &args[0], None)?,
                    right: None,
                });
            };
            match resolve(env, identifier, None)? {
                Resolved::Bound(_) => Ok(Ty::Bool),
                Resolved::EnumMember(..) => Err(TypeCheckError::UnknownIdentifier {
                    name: identifier.to_string(),
                }),
            }
        }
        "direct_manager" => {
            arity(name, args, 2)?;
            for arg in args {
                let ty = check(env, arg, None)?;
                if !matches!(ty, Ty::Ref(_)) {
                    return Err(mismatch("direct_manager argument", ty));
                }
            }
            Ok(Ty::Bool)
        }
        "working_days" => {
            arity(name, args, 3)?;
            let calendar = env
                .calendar_ref_type()
                .cloned()
                .ok_or(TypeCheckError::MissingCalendarType)?;
            if matches!(&args[0], Ast::List(items) if items.len() != 2) {
                return Err(TypeCheckError::InvalidPeriodType);
            }
            let period = check(env, &args[0], Some(&Ty::List(Box::new(Ty::Date))))?;
            if period != Ty::List(Box::new(Ty::Date)) {
                return Err(TypeCheckError::InvalidPeriodType);
            }
            require(
                "working_days calendar",
                check(env, &args[1], None)?,
                &Ty::Ref(calendar),
            )?;
            require(
                "working_days inclusive_end",
                check(env, &args[2], None)?,
                &Ty::Bool,
            )?;
            Ok(Ty::Quantity(unit(WORKING_DAY_UNIT)))
        }
        "days_between" => {
            arity(name, args, 2)?;
            for arg in args {
                require("days_between argument", check(env, arg, None)?, &Ty::Date)?;
            }
            Ok(Ty::Int)
        }
        "count" | "any" | "all" => {
            arity(name, args, 1)?;
            let result = if name == "count" { Ty::Int } else { Ty::Bool };
            if empty_list(&args[0]) {
                return Ok(result);
            }
            let expected = (name != "count").then(|| Ty::List(Box::new(Ty::Bool)));
            let ty = check(env, &args[0], expected.as_ref())?;
            match (&ty, name) {
                (Ty::List(_), "count") => Ok(result),
                (Ty::List(inner), _) if **inner == Ty::Bool => Ok(result),
                _ => Err(mismatch(name, ty)),
            }
        }
        _ => {
            // sum, min, max
            arity(name, args, 1)?;
            let ty = check(env, &args[0], None)?;
            let Ty::List(inner) = &ty else {
                return Err(mismatch(name, ty));
            };
            let supported = match name {
                "sum" => is_scalar(inner) || is_unit_bearing(inner),
                _ => {
                    is_scalar(inner)
                        || is_unit_bearing(inner)
                        || matches!(**inner, Ty::Date | Ty::DateTime)
                }
            };
            if supported {
                Ok((**inner).clone())
            } else {
                Err(mismatch(name, ty))
            }
        }
    }
}

fn check_list(env: &TypeEnv, items: &[Ast], expected: Option<&Ty>) -> Result<Ty, TypeCheckError> {
    let element_expected = match expected {
        Some(Ty::List(inner)) => Some((**inner).clone()),
        _ => None,
    };
    if items.is_empty() {
        return element_expected
            .map(|t| Ty::List(Box::new(t)))
            .ok_or(TypeCheckError::EmptyListTypeUnknown);
    }
    let mut current: Option<Ty> = None;
    let mut deferred = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let context = current.clone().or_else(|| element_expected.clone());
        match check(env, item, context.as_ref()) {
            Ok(ty) => {
                current = Some(match &current {
                    None => ty,
                    Some(c) => {
                        common(c, &ty).ok_or_else(|| TypeCheckError::IncompatibleListElements {
                            first: c.clone(),
                            other: ty.clone(),
                        })?
                    }
                });
            }
            Err(e @ TypeCheckError::UnknownIdentifier { .. })
                if bare_symbol(item) && context.is_none() =>
            {
                deferred.push((i, e));
            }
            Err(e) => return Err(e),
        }
    }
    for (i, e) in deferred {
        let Some(context) = current.clone() else {
            return Err(e);
        };
        let ty = check(env, &items[i], Some(&context))?;
        current = Some(common(&context, &ty).ok_or_else(|| {
            TypeCheckError::IncompatibleListElements {
                first: context.clone(),
                other: ty.clone(),
            }
        })?);
    }
    let element = current.ok_or(TypeCheckError::EmptyListTypeUnknown)?;
    Ok(Ty::List(Box::new(element)))
}

/// The static type of `ast`; `expected` only supplies context for contextual enum members and
/// empty lists.
pub(crate) fn check(env: &TypeEnv, ast: &Ast, expected: Option<&Ty>) -> Result<Ty, TypeCheckError> {
    match ast {
        Ast::Literal(literal) => Ok(literal_type(literal)),
        Ast::Identifier(identifier) => match resolve(env, identifier, expected)? {
            Resolved::Bound(ty) => Ok(ty),
            Resolved::EnumMember(enum_ref, _) => Ok(Ty::Enum(enum_ref)),
        },
        Ast::List(items) => check_list(env, items, expected),
        Ast::Call { function, args } => check_call(env, function, args),
        Ast::Unary { op, expr } => {
            let ty = check(env, expr, None)?;
            match op {
                UnaryOp::Not if ty == Ty::Bool => Ok(Ty::Bool),
                UnaryOp::Not => Err(mismatch("not", ty)),
                UnaryOp::Negate if is_scalar(&ty) || is_unit_bearing(&ty) => Ok(ty),
                UnaryOp::Negate => Err(TypeCheckError::InvalidOperand {
                    op: "-".to_owned(),
                    left: ty,
                    right: None,
                }),
            }
        }
        Ast::Binary { op, left, right } => match op {
            BinaryOp::And | BinaryOp::Or | BinaryOp::Implies => {
                for side in [left, right] {
                    let ty = check(env, side, Some(&Ty::Bool))?;
                    if ty != Ty::Bool {
                        return Err(mismatch(op.token(), ty));
                    }
                }
                Ok(Ty::Bool)
            }
            BinaryOp::Equal | BinaryOp::NotEqual => {
                let (lt, rt) = check_pair(env, left, right)?;
                if equality_compatible(&lt, &rt, left, right) {
                    Ok(Ty::Bool)
                } else {
                    Err(invalid(*op, &lt, &rt))
                }
            }
            BinaryOp::LessThan
            | BinaryOp::LessThanOrEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterThanOrEqual => {
                let (lt, rt) = check_pair(env, left, right)?;
                if ordering_compatible(&lt, &rt, left, right) {
                    Ok(Ty::Bool)
                } else {
                    Err(invalid(*op, &lt, &rt))
                }
            }
            BinaryOp::In => {
                let (lt, rt) = match check(env, left, None) {
                    Ok(lt) => {
                        let rt = check(env, right, Some(&Ty::List(Box::new(lt.clone()))))?;
                        (lt, rt)
                    }
                    Err(e @ TypeCheckError::UnknownIdentifier { .. }) if bare_symbol(left) => {
                        let rt = check(env, right, None).map_err(|_| e)?;
                        let element = match &rt {
                            Ty::List(inner) => (**inner).clone(),
                            other => return Err(mismatch("in", other.clone())),
                        };
                        (check(env, left, Some(&element))?, rt)
                    }
                    Err(e) => return Err(e),
                };
                match &rt {
                    Ty::List(inner) if common(&lt, inner).is_some() => Ok(Ty::Bool),
                    Ty::List(inner) => Err(invalid(*op, &lt, inner)),
                    other => Err(mismatch("in", other.clone())),
                }
            }
            arithmetic_op => {
                let lt = check(env, left, None)?;
                let rt = check(env, right, None)?;
                arithmetic(*arithmetic_op, &lt, &rt)
            }
        },
        Ast::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            let ct = check(env, condition, Some(&Ty::Bool))?;
            if ct != Ty::Bool {
                return Err(mismatch("if condition", ct));
            }
            let (then_ty, else_ty) = match expected {
                Some(context) => (
                    check(env, then_expr, Some(context))?,
                    check(env, else_expr, Some(context))?,
                ),
                None => check_pair(env, then_expr, else_expr)?,
            };
            branch_common(&then_ty, &else_ty, then_expr, else_expr)
                .ok_or(TypeCheckError::IncompatibleConditionalBranches { then_ty, else_ty })
        }
    }
}

/// Whether `value` is an integral decimal.
pub(crate) fn is_integral(value: &Decimal) -> bool {
    value.fract().is_zero()
}
