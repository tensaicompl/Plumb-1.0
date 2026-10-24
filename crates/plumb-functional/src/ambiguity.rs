//! Deterministic detection of the open calculation semantics that only a domain expert can
//! settle (the S3 calculation answer families of Hotfix 083): an unresolved `working_days`
//! inclusive end, an unconfirmed holiday policy and an ungoverned fractional input.
//!
//! Each open semantic yields one per-candidate `GeneratedFinding` whose semantic condition key
//! is the frozen family prefix plus the exact Calculation ID, which is the explicit upstream
//! analysis material the question templates match. A family stops reporting as soon as its
//! governed answer exists in the graph: the rewritten expression, the holiday decision marker or
//! the governed fraction example. Nothing here mutates a graph or calls a provider.

use plumb_core::Id;
use plumb_expr::{parse, Ast, Ty};
use plumb_psg::{ElementStatus, Graph, Node, NodePayload, NodeType};
use plumb_validation::calculation_analysis::{
    calculation_origin_of, has_unresolved_inclusive_end, CalculationBindingExposure,
};
use plumb_validation::expression_scope::attribute_expression_type;
use plumb_validation::{
    load_builtin_software_profile, GeneratedFinding, RuleMetadata, ViolationFacts,
};
use serde_json::Value;

use crate::calculation::calculation_fraction_semantic_key;
use crate::question_templates::{
    CALCULATION_FRACTION_CONDITION_PREFIX, CALCULATION_HOLIDAY_POLICY_CONDITION_PREFIX,
    CALCULATION_INCLUSIVE_END_CONDITION_PREFIX,
};

const TYPECHECK_RULE: &str = "PLUMB.F2.CALC.TYPECHECK";
const CALENDAR_RULE: &str = "PLUMB.F2.TIME.CALENDAR_DEFINED";
const HOLIDAY_MARKER: &str = "calculation_holiday_policy";

/// The open-semantics findings of every Accepted Calculation of `graph`, sorted by ID.
pub fn analyze_calculation_ambiguities(graph: &Graph) -> Result<Vec<GeneratedFinding>, String> {
    let profile = load_builtin_software_profile().map_err(|e| e.to_string())?;
    let rule = |id: &str| -> Result<RuleMetadata, String> {
        profile
            .rules
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| format!("profile has no rule {id}"))
    };
    let typecheck = rule(TYPECHECK_RULE)?;
    let calendar = rule(CALENDAR_RULE)?;
    let mut findings = Vec::new();
    for id in graph.node_ids_by_type(NodeType::Calculation) {
        let Some(node) = graph
            .node(id)
            .filter(|n| n.status == ElementStatus::Accepted)
        else {
            continue;
        };
        let NodePayload::Calculation(calculation) = &node.payload else {
            continue;
        };
        let Ok(ast) = parse(&calculation.expression) else {
            continue; // an unparsable expression is a CALC.TYPECHECK rule failure instead
        };
        let mut push = |rule: &RuleMetadata, prefix: &str, targets: Vec<Id>, message: String| {
            let condition = format!("{prefix}{id}");
            GeneratedFinding::for_violation(
                rule,
                rule.severity,
                &ViolationFacts {
                    targets: &targets,
                    semantic_condition_key: &condition,
                    message: &message,
                    suggested_resolution: Some("Answer the routed expert question."),
                },
                None,
            )
            .map(|f| findings.push(f))
            .map_err(|e| e.to_string())
        };
        if has_unresolved_inclusive_end(&ast) {
            push(
                &typecheck,
                CALCULATION_INCLUSIVE_END_CONDITION_PREFIX,
                vec![id.clone()],
                format!("Calculation {id} does not say whether the period end date is included."),
            )?;
        }
        if calls(&ast, "working_days") && !holiday_decided(graph, id) {
            push(
                &calendar,
                CALCULATION_HOLIDAY_POLICY_CONDITION_PREFIX,
                vec![id.clone()],
                format!(
                    "Calculation {id} counts working days but the treatment of holidays inside \
                     the period and its calendar are not confirmed."
                ),
            )?;
        }
        for attribute in fractional_inputs(graph, node) {
            let key =
                calculation_fraction_semantic_key(id, &attribute.id).map_err(|e| e.to_string())?;
            let governed = calculation
                .examples
                .iter()
                .flatten()
                .any(|e| e.get("key").and_then(Value::as_str) == Some(key.as_str()));
            if !governed {
                push(
                    &typecheck,
                    CALCULATION_FRACTION_CONDITION_PREFIX,
                    vec![id.clone(), attribute.id.clone()],
                    format!(
                        "Calculation {id} uses the fractional input {} whose value per case \
                         (e.g. a half day) is not defined.",
                        attribute.id
                    ),
                )?;
            }
        }
    }
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    findings.dedup_by(|a, b| a.id == b.id);
    Ok(findings)
}

/// Whether an Accepted governed holiday-policy decision exists for `calculation`.
fn holiday_decided(graph: &Graph, calculation: &Id) -> bool {
    graph
        .node_ids_by_type(NodeType::ResolutionDecision)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| n.status == ElementStatus::Accepted)
        .any(|n| match &n.payload {
            NodePayload::ResolutionDecision(d) => {
                d.answer.get("kind").and_then(Value::as_str) == Some(HOLIDAY_MARKER)
                    && d.answer.get("calculation_ref").and_then(Value::as_str)
                        == Some(calculation.as_str())
            }
            _ => false,
        })
}

/// The Accepted unitless Decimal Attributes bound as roots by the calculation's origin: the
/// inputs whose per-case value (a fraction such as a half day) an expert must define.
fn fractional_inputs<'g>(graph: &'g Graph, node: &Node) -> Vec<&'g Node> {
    let Some(origin) = calculation_origin_of(node) else {
        return Vec::new();
    };
    origin
        .used_bindings
        .iter()
        .filter(|b| b.exposure == CalculationBindingExposure::Root)
        .filter_map(|b| graph.node(&b.node_ref))
        .filter(|n| n.status == ElementStatus::Accepted)
        .filter(|n| matches!(attribute_expression_type(n), Ok((Ty::Decimal(_), None))))
        .collect()
}

fn calls(ast: &Ast, name: &str) -> bool {
    match ast {
        Ast::Literal(_) | Ast::Identifier(_) => false,
        Ast::Call { function, args } => {
            function.as_str() == name || args.iter().any(|a| calls(a, name))
        }
        Ast::List(items) => items.iter().any(|a| calls(a, name)),
        Ast::Unary { expr, .. } => calls(expr, name),
        Ast::Binary { left, right, .. } => calls(left, name) || calls(right, name),
        Ast::Conditional {
            condition,
            then_expr,
            else_expr,
        } => calls(condition, name) || calls(then_expr, name) || calls(else_expr, name),
    }
}
