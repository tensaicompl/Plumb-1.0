//! Deterministic plain-English example renderer (plan S2.5, Hotfix 038).
//!
//! One fixed English template documents an evaluation: the referenced runtime inputs with
//! their values, the canonical expression and its result. A runtime input is an identifier
//! whose root is an explicit TypeEnv root binding; a contextual enum member (no root binding,
//! resolved by typing as a member of the expected enum) is a constant and is never listed. It is documentation output, not
//! natural-language generation: no provider, translation or randomness is involved.

use std::collections::BTreeMap;

use crate::ast::{unparse, Ast, Identifier};
use crate::eval::{Value, ValueEnv};
use crate::typecheck::TypeEnv;

/// Every referenced identifier whose root is a TypeEnv root binding.
fn collect(ast: &Ast, types: &TypeEnv, out: &mut BTreeMap<String, Identifier>) {
    match ast {
        Ast::Literal(_) => {}
        Ast::Identifier(identifier) => {
            if types.root(&identifier.segments()[0]).is_some() {
                out.insert(identifier.to_string(), identifier.clone());
            }
        }
        Ast::List(items) | Ast::Call { args: items, .. } => {
            items.iter().for_each(|item| collect(item, types, out))
        }
        Ast::Unary { expr, .. } => collect(expr, types, out),
        Ast::Binary { left, right, .. } => {
            collect(left, types, out);
            collect(right, types, out);
        }
        Ast::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect(condition, types, out);
            collect(then_expr, types, out);
            collect(else_expr, types, out);
        }
    }
}

/// `Given <id> = <value>, ...; <canonical expression> evaluates to <result>.` with every
/// referenced runtime input once, sorted by dotted text (known inputs without a value as
/// `<missing>`), or `<canonical expression> evaluates to <result>.` when no input is
/// referenced. Contextual enum constants are never listed.
pub fn render_example(ast: &Ast, types: &TypeEnv, values: &ValueEnv, result: &Value) -> String {
    let mut identifiers = BTreeMap::new();
    collect(ast, types, &mut identifiers);
    let expression = unparse(ast);
    if identifiers.is_empty() {
        return format!("{expression} evaluates to {result}.");
    }
    let given: Vec<String> = identifiers
        .iter()
        .map(|(text, identifier)| match values.lookup(identifier) {
            Some(value) => format!("{text} = {value}"),
            None => format!("{text} = <missing>"),
        })
        .collect();
    format!(
        "Given {}; {expression} evaluates to {result}.",
        given.join(", ")
    )
}
