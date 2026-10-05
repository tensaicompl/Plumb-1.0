//! The PlumbExpr parser (plan S2.4, Hotfix 037).
//!
//! `grammar.pest` decides syntactic acceptance; this module converts its pairs into the AST
//! and validates literal values (i64 range, decimals, dates, timestamps, units and string
//! escapes). Parsing is pure: it never evaluates, resolves identifiers or calls functions.
//! Errors are a stable typed contract with zero-based UTF-8 byte offsets; pest errors are
//! translated and never exposed.

use pest::iterators::Pair;
use pest::Parser as _;
use plumb_core::Timestamp;
use rust_decimal::Decimal;
use thiserror::Error;

use crate::ast::{unparse, Ast, BinaryOp, Identifier, Literal, Symbol, UnaryOp};
use crate::types::Unit;

#[derive(pest_derive::Parser)]
#[grammar = "grammar.pest"]
struct PlumbExprGrammar;

/// Why an input is not a PlumbExpr v1 expression. Offsets are zero-based UTF-8 byte offsets
/// into the input; diagnostic prose is not part of the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ParseError {
    #[error("empty expression")]
    Empty,
    #[error("syntax error at byte {offset}")]
    Syntax { offset: usize },
    #[error("invalid integer literal at byte {offset}")]
    InvalidInteger { offset: usize },
    #[error("invalid decimal literal at byte {offset}")]
    InvalidDecimal { offset: usize },
    #[error("invalid string literal at byte {offset}")]
    InvalidString { offset: usize },
    #[error("invalid date literal at byte {offset}")]
    InvalidDate { offset: usize },
    #[error("invalid datetime literal at byte {offset}")]
    InvalidDateTime { offset: usize },
    #[error("invalid unit at byte {offset}")]
    InvalidUnit { offset: usize },
}

impl ParseError {
    /// The byte offset, when the error has one.
    pub fn offset(&self) -> Option<usize> {
        match self {
            ParseError::Empty => None,
            ParseError::Syntax { offset }
            | ParseError::InvalidInteger { offset }
            | ParseError::InvalidDecimal { offset }
            | ParseError::InvalidString { offset }
            | ParseError::InvalidDate { offset }
            | ParseError::InvalidDateTime { offset }
            | ParseError::InvalidUnit { offset } => Some(*offset),
        }
    }
}

/// Parses one PlumbExpr v1 expression.
pub fn parse(input: &str) -> Result<Ast, ParseError> {
    if input.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')) {
        return Err(ParseError::Empty);
    }
    let mut pairs = PlumbExprGrammar::parse(Rule::program, input).map_err(|e| {
        let offset = match e.location {
            pest::error::InputLocation::Pos(p) => p,
            pest::error::InputLocation::Span((start, _)) => start,
        };
        ParseError::Syntax { offset }
    })?;
    let program = pairs.next().ok_or(ParseError::Syntax { offset: 0 })?;
    let expr = program
        .into_inner()
        .find(|p| p.as_rule() == Rule::expr)
        .ok_or(ParseError::Syntax { offset: 0 })?;
    build(expr)
}

/// `unparse(parse(input))`: the canonical source text of an accepted input.
pub fn canonicalize(input: &str) -> Result<String, ParseError> {
    parse(input).map(|ast| unparse(&ast))
}

fn syntax(pair: &Pair<'_, Rule>) -> ParseError {
    ParseError::Syntax {
        offset: pair.as_span().start(),
    }
}

fn is_token(rule: Rule) -> bool {
    matches!(
        rule,
        Rule::kw_if
            | Rule::kw_then
            | Rule::kw_else
            | Rule::kw_or
            | Rule::kw_and
            | Rule::kw_date
            | Rule::kw_datetime
            | Rule::kw_duration
            | Rule::kw_quantity
    )
}

/// The inner pairs without keyword tokens.
fn children<'i>(pair: Pair<'i, Rule>) -> Vec<Pair<'i, Rule>> {
    pair.into_inner()
        .filter(|p| !is_token(p.as_rule()))
        .collect()
}

fn binary(op: BinaryOp, left: Ast, right: Ast) -> Ast {
    Ast::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn operator(pair: &Pair<'_, Rule>) -> Result<BinaryOp, ParseError> {
    Ok(match pair.as_str() {
        "->" => BinaryOp::Implies,
        "==" => BinaryOp::Equal,
        "!=" => BinaryOp::NotEqual,
        "<" => BinaryOp::LessThan,
        "<=" => BinaryOp::LessThanOrEqual,
        ">" => BinaryOp::GreaterThan,
        ">=" => BinaryOp::GreaterThanOrEqual,
        "in" => BinaryOp::In,
        "+" => BinaryOp::Add,
        "-" => BinaryOp::Subtract,
        "*" => BinaryOp::Multiply,
        "/" => BinaryOp::Divide,
        "%" => BinaryOp::Remainder,
        _ => return Err(syntax(pair)),
    })
}

/// Folds `operand (op operand)*` left-associatively; `fixed` names the operator of keyword
/// levels (`or`, `and`) whose tokens are filtered out.
fn fold_left(pair: Pair<'_, Rule>, fixed: Option<BinaryOp>) -> Result<Ast, ParseError> {
    let mut items = children(pair).into_iter();
    let first = items.next().ok_or(ParseError::Syntax { offset: 0 })?;
    let mut acc = build(first)?;
    while let Some(next) = items.next() {
        let (op, operand) = match fixed {
            Some(op) => (op, next),
            None => {
                let op = operator(&next)?;
                let operand = items.next().ok_or_else(|| syntax(&next))?;
                (op, operand)
            }
        };
        acc = binary(op, acc, build(operand)?);
    }
    Ok(acc)
}

fn build(pair: Pair<'_, Rule>) -> Result<Ast, ParseError> {
    match pair.as_rule() {
        Rule::expr | Rule::primary => {
            let inner = pair
                .clone()
                .into_inner()
                .next()
                .ok_or_else(|| syntax(&pair))?;
            build(inner)
        }
        Rule::conditional => {
            let parts = children(pair.clone());
            let [condition, then_expr, else_expr]: [Pair<'_, Rule>; 3] =
                parts.try_into().map_err(|_| syntax(&pair))?;
            Ok(Ast::Conditional {
                condition: Box::new(build(condition)?),
                then_expr: Box::new(build(then_expr)?),
                else_expr: Box::new(build(else_expr)?),
            })
        }
        Rule::implication => {
            let mut parts = children(pair.clone()).into_iter();
            let left = build(parts.next().ok_or_else(|| syntax(&pair))?)?;
            match (parts.next(), parts.next()) {
                (Some(op), Some(right)) => Ok(binary(operator(&op)?, left, build(right)?)),
                _ => Ok(left),
            }
        }
        Rule::disjunction => fold_left(pair, Some(BinaryOp::Or)),
        Rule::conjunction => fold_left(pair, Some(BinaryOp::And)),
        Rule::comparison | Rule::additive | Rule::multiplicative => fold_left(pair, None),
        Rule::unary => {
            let mut ops = Vec::new();
            let mut operand = None;
            for inner in pair.clone().into_inner() {
                match inner.as_rule() {
                    Rule::unary_op => ops.push(if inner.as_str() == "-" {
                        UnaryOp::Negate
                    } else {
                        UnaryOp::Not
                    }),
                    _ => operand = Some(build(inner)?),
                }
            }
            let mut acc = operand.ok_or_else(|| syntax(&pair))?;
            for op in ops.into_iter().rev() {
                acc = Ast::Unary {
                    op,
                    expr: Box::new(acc),
                };
            }
            Ok(acc)
        }
        Rule::call => {
            let mut parts = children(pair.clone()).into_iter();
            let name = parts.next().ok_or_else(|| syntax(&pair))?;
            let function = Symbol::new(name.as_str()).map_err(|_| syntax(&name))?;
            let args = parts.map(build).collect::<Result<_, _>>()?;
            Ok(Ast::Call { function, args })
        }
        Rule::list => Ok(Ast::List(
            children(pair)
                .into_iter()
                .map(build)
                .collect::<Result<_, _>>()?,
        )),
        Rule::identifier => {
            let segments = pair
                .clone()
                .into_inner()
                .map(|s| Symbol::new(s.as_str()).map_err(|_| syntax(&s)))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Ast::Identifier(
                Identifier::new(segments).map_err(|_| syntax(&pair))?,
            ))
        }
        Rule::bool_lit => Ok(Ast::Literal(Literal::Bool(pair.as_str() == "true"))),
        Rule::number => number(&pair),
        Rule::string => Ok(Ast::Literal(Literal::String(string(&pair)?))),
        Rule::date_lit => {
            let text_pair = single(pair.clone())?;
            let text = string(&text_pair)?;
            let offset = text_pair.as_span().start();
            Ok(Ast::Literal(Literal::Date(
                date(&text).ok_or(ParseError::InvalidDate { offset })?,
            )))
        }
        Rule::datetime_lit => {
            let text_pair = single(pair.clone())?;
            let text = string(&text_pair)?;
            let offset = text_pair.as_span().start();
            let timestamp: Timestamp = text
                .parse()
                .map_err(|_| ParseError::InvalidDateTime { offset })?;
            Ok(Ast::Literal(Literal::DateTime(timestamp)))
        }
        Rule::duration_lit | Rule::quantity_lit => {
            let is_duration = pair.as_rule() == Rule::duration_lit;
            let parts = children(pair.clone());
            let [number_pair, unit_pair]: [Pair<'_, Rule>; 2] =
                parts.try_into().map_err(|_| syntax(&pair))?;
            let value = match number(&number_pair)? {
                Ast::Literal(Literal::Int(i)) => Decimal::from(i),
                Ast::Literal(Literal::Decimal(d)) => d,
                _ => return Err(syntax(&number_pair)),
            };
            let unit = Unit::new(unit_pair.as_str()).map_err(|_| ParseError::InvalidUnit {
                offset: unit_pair.as_span().start(),
            })?;
            Ok(Ast::Literal(if is_duration {
                Literal::Duration { value, unit }
            } else {
                Literal::Quantity { value, unit }
            }))
        }
        _ => Err(syntax(&pair)),
    }
}

fn single(pair: Pair<'_, Rule>) -> Result<Pair<'_, Rule>, ParseError> {
    let parts = children(pair.clone());
    let [only]: [Pair<'_, Rule>; 1] = parts.try_into().map_err(|_| syntax(&pair))?;
    Ok(only)
}

/// An integer `0 | [1-9][0-9]*` within i64, or a decimal `0.[0-9]+ | [1-9][0-9]*.[0-9]+`
/// represented exactly with its scale.
fn number(pair: &Pair<'_, Rule>) -> Result<Ast, ParseError> {
    let text = pair.as_str();
    let offset = pair.as_span().start();
    let leading_zero = |integer: &str| integer.len() > 1 && integer.starts_with('0');
    match text.split_once('.') {
        None => {
            if leading_zero(text) {
                return Err(ParseError::InvalidInteger { offset });
            }
            text.parse::<i64>()
                .map(|v| Ast::Literal(Literal::Int(v)))
                .map_err(|_| ParseError::InvalidInteger { offset })
        }
        Some((integer, fraction)) => {
            if leading_zero(integer) || fraction.is_empty() {
                return Err(ParseError::InvalidDecimal { offset });
            }
            // Exact representation only: no silent rounding of scale or magnitude.
            match text.parse::<Decimal>() {
                Ok(value) if value.to_string() == text => Ok(Ast::Literal(Literal::Decimal(value))),
                _ => Err(ParseError::InvalidDecimal { offset }),
            }
        }
    }
}

/// The unescaped content of a string literal pair.
fn string(pair: &Pair<'_, Rule>) -> Result<String, ParseError> {
    let raw = pair.as_str();
    let base = pair.as_span().start() + 1;
    let body = &raw[1..raw.len() - 1];
    let mut out = String::with_capacity(body.len());
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        let invalid = ParseError::InvalidString { offset: base + i };
        match c {
            '\\' => match chars.next().map(|(_, e)| e) {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                _ => return Err(invalid),
            },
            c if c.is_ascii_control() => return Err(invalid),
            c => out.push(c),
        }
    }
    Ok(out)
}

/// A real Gregorian date from exactly `YYYY-MM-DD`.
fn date(text: &str) -> Option<time::Date> {
    let bytes = text.as_bytes();
    let shape = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|&i| bytes[i].is_ascii_digit());
    if !shape {
        return None;
    }
    let year: i32 = text[0..4].parse().ok()?;
    let month: u8 = text[5..7].parse().ok()?;
    let day: u8 = text[8..10].parse().ok()?;
    time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()
}
