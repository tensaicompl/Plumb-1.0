//! The PlumbExpr syntax tree and canonical unparser (plan S2.4, Hotfix 037).
//!
//! The AST is made of explicit typed syntax variants; it carries no semantic types and no
//! resolution. A call is a static symbol plus argument expressions and is never executed
//! here. The canonical source text, not the AST, is the persisted form.

use std::fmt::{self, Write as _};
use std::str::FromStr;

use plumb_core::Timestamp;
use rust_decimal::Decimal;
use thiserror::Error;

use crate::types::Unit;

/// Language version of the grammar, precedence and canonical printing.
pub const PLUMB_EXPR_LANGUAGE_VERSION: u32 = 1;

/// The exact lowercase reserved words of language v1.
pub const RESERVED_WORDS: [&str; 13] = [
    "true", "false", "and", "or", "not", "in", "if", "then", "else", "date", "datetime",
    "duration", "quantity",
];

/// Why a symbol or identifier is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NameError {
    #[error("invalid symbol {0:?}")]
    InvalidSymbol(String),
    #[error("reserved word {0:?} cannot be a symbol")]
    ReservedWord(String),
    #[error("invalid identifier {0:?}")]
    InvalidIdentifier(String),
}

/// One simple, non-reserved identifier `[A-Za-z_][A-Za-z0-9_]*`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(String);

impl Symbol {
    /// Validates the symbol syntax and rejects reserved words.
    pub fn new(symbol: &str) -> Result<Symbol, NameError> {
        let mut chars = symbol.chars();
        let valid = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(NameError::InvalidSymbol(symbol.to_owned()));
        }
        if RESERVED_WORDS.contains(&symbol) {
            return Err(NameError::ReservedWord(symbol.to_owned()));
        }
        Ok(Symbol(symbol.to_owned()))
    }

    /// The symbol text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Symbol {
    type Err = NameError;

    fn from_str(s: &str) -> Result<Symbol, NameError> {
        Symbol::new(s)
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A static dotted path of one or more symbols.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identifier {
    segments: Vec<Symbol>,
}

impl Identifier {
    /// An identifier from at least one segment.
    pub fn new(segments: Vec<Symbol>) -> Result<Identifier, NameError> {
        if segments.is_empty() {
            return Err(NameError::InvalidIdentifier(String::new()));
        }
        Ok(Identifier { segments })
    }

    /// The path segments, in order.
    pub fn segments(&self) -> &[Symbol] {
        &self.segments
    }
}

impl FromStr for Identifier {
    type Err = NameError;

    /// Parses the exact dotted form without whitespace.
    fn from_str(s: &str) -> Result<Identifier, NameError> {
        let segments = s
            .split('.')
            .map(Symbol::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| NameError::InvalidIdentifier(s.to_owned()))?;
        Identifier::new(segments)
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.segments.iter().enumerate() {
            if i > 0 {
                f.write_char('.')?;
            }
            f.write_str(segment.as_str())?;
        }
        Ok(())
    }
}

/// A literal value. Negative numbers are `Unary { op: Negate, .. }` around a literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    Int(i64),
    Decimal(Decimal),
    Bool(bool),
    String(String),
    Date(time::Date),
    DateTime(Timestamp),
    Duration { value: Decimal, unit: Unit },
    Quantity { value: Decimal, unit: Unit },
}

/// A prefix operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Negate,
    Not,
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    In,
    And,
    Or,
    Implies,
}

impl BinaryOp {
    /// Every binary operator.
    pub const ALL: [BinaryOp; 15] = [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Remainder,
        BinaryOp::Equal,
        BinaryOp::NotEqual,
        BinaryOp::LessThan,
        BinaryOp::LessThanOrEqual,
        BinaryOp::GreaterThan,
        BinaryOp::GreaterThanOrEqual,
        BinaryOp::In,
        BinaryOp::And,
        BinaryOp::Or,
        BinaryOp::Implies,
    ];

    /// The exact source token.
    pub fn token(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Subtract => "-",
            BinaryOp::Multiply => "*",
            BinaryOp::Divide => "/",
            BinaryOp::Remainder => "%",
            BinaryOp::Equal => "==",
            BinaryOp::NotEqual => "!=",
            BinaryOp::LessThan => "<",
            BinaryOp::LessThanOrEqual => "<=",
            BinaryOp::GreaterThan => ">",
            BinaryOp::GreaterThanOrEqual => ">=",
            BinaryOp::In => "in",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
            BinaryOp::Implies => "->",
        }
    }

    /// The normative precedence level (1 lowest).
    pub fn precedence(self) -> u8 {
        match self {
            BinaryOp::Implies => 2,
            BinaryOp::Or => 3,
            BinaryOp::And => 4,
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::LessThan
            | BinaryOp::LessThanOrEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterThanOrEqual
            | BinaryOp::In => 5,
            BinaryOp::Add | BinaryOp::Subtract => 6,
            BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Remainder => 7,
        }
    }
}

/// A PlumbExpr syntax tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ast {
    Literal(Literal),
    Identifier(Identifier),
    List(Vec<Ast>),
    Call {
        function: Symbol,
        args: Vec<Ast>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Ast>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Ast>,
        right: Box<Ast>,
    },
    Conditional {
        condition: Box<Ast>,
        then_expr: Box<Ast>,
        else_expr: Box<Ast>,
    },
}

// ============================================================================ canonical unparse

const CONDITIONAL_LEVEL: u8 = 1;
const UNARY_LEVEL: u8 = 8;
const PRIMARY_LEVEL: u8 = 9;

fn level(ast: &Ast) -> u8 {
    match ast {
        Ast::Conditional { .. } => CONDITIONAL_LEVEL,
        Ast::Binary { op, .. } => op.precedence(),
        Ast::Unary { .. } => UNARY_LEVEL,
        _ => PRIMARY_LEVEL,
    }
}

/// Renders the canonical source text of `ast`; for a parser-produced AST,
/// `parse(&unparse(ast)) == Ok(ast)`.
pub fn unparse(ast: &Ast) -> String {
    let mut out = String::new();
    write_at(&mut out, ast, CONDITIONAL_LEVEL);
    out
}

/// Writes `ast`, parenthesized when its level is below `min`.
fn write_at(out: &mut String, ast: &Ast, min: u8) {
    if level(ast) < min {
        out.push('(');
        write_ast(out, ast);
        out.push(')');
    } else {
        write_ast(out, ast);
    }
}

fn write_list(out: &mut String, items: &[Ast]) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_at(out, item, CONDITIONAL_LEVEL);
    }
}

fn write_ast(out: &mut String, ast: &Ast) {
    match ast {
        Ast::Literal(literal) => write_literal(out, literal),
        Ast::Identifier(identifier) => {
            let _ = write!(out, "{identifier}");
        }
        Ast::List(items) => {
            out.push('[');
            write_list(out, items);
            out.push(']');
        }
        Ast::Call { function, args } => {
            out.push_str(function.as_str());
            out.push('(');
            write_list(out, args);
            out.push(')');
        }
        Ast::Unary { op, expr } => {
            out.push_str(match op {
                UnaryOp::Negate => "-",
                UnaryOp::Not => "not ",
            });
            write_at(out, expr, UNARY_LEVEL);
        }
        Ast::Binary { op, left, right } => {
            let p = op.precedence();
            let (left_min, right_min) = match op {
                // Right-associative.
                BinaryOp::Implies => (p + 1, p),
                // Non-associative comparisons.
                _ if p == 5 => (p + 1, p + 1),
                // Left-associative.
                _ => (p, p + 1),
            };
            write_at(out, left, left_min);
            out.push(' ');
            out.push_str(op.token());
            out.push(' ');
            write_at(out, right, right_min);
        }
        Ast::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            out.push_str("if ");
            write_at(out, condition, CONDITIONAL_LEVEL);
            out.push_str(" then ");
            write_at(out, then_expr, CONDITIONAL_LEVEL);
            out.push_str(" else ");
            write_at(out, else_expr, CONDITIONAL_LEVEL);
        }
    }
}

fn write_literal(out: &mut String, literal: &Literal) {
    let _ = match literal {
        Literal::Int(value) => write!(out, "{value}"),
        Literal::Decimal(value) => write!(out, "{value}"),
        Literal::Bool(value) => write!(out, "{value}"),
        Literal::String(value) => {
            write_string(out, value);
            Ok(())
        }
        Literal::Date(date) => write!(
            out,
            "date(\"{:04}-{:02}-{:02}\")",
            date.year(),
            u8::from(date.month()),
            date.day()
        ),
        Literal::DateTime(timestamp) => write!(out, "datetime(\"{timestamp}\")"),
        Literal::Duration { value, unit } => write!(out, "duration({value}, {unit})"),
        Literal::Quantity { value, unit } => write!(out, "quantity({value}, {unit})"),
    };
}

/// A double-quoted string with exactly the language escapes.
fn write_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            other => out.push(other),
        }
    }
    out.push('"');
}
