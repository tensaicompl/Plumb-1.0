//! Pure Calculation qualification and dependency analysis (plan S2.6, relocated by Hotfix 044).
//!
//! Qualifies a calculation expression through the S2.4 parser and S2.5 type checker under an
//! explicit binding scope (the shared [`crate::expression_scope`] adapter) and the reserved root
//! `calendar`, replays the scope of Accepted Calculations from their S2.6 origin (or an explicit
//! override for legacy ones), checks declared result type, unit and rounding, and extracts the
//! dependency graph and its cycles. S2.6 proposal compilation and S2.10 validation both use this
//! one implementation; nothing here infers, mutates or evaluates runtime values.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use plumb_expr::{parse, typecheck, unparse, Ast, Identifier, RoundingSpec, Symbol, Ty, TypeEnv};
use plumb_psg::{ElementStatus, Graph, Node, NodePayload, NodeType};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::expression_scope::{
    declared_result_type, validate_expression_bindings, ExpressionBindingExposure,
    ExpressionScopeBinding, ExpressionScopeIssue, ResultTypeIssue, ValidatedExpressionScope,
};

/// The reserved root symbol bound only from `Calculation.calendar_ref`.
pub const CALCULATION_CALENDAR_SYMBOL: &str = "calendar";

/// Extension key of the provenance [`CalculationOrigin`].
pub const CALCULATION_ORIGIN_EXTENSION: &str = "plumb_functional:calculation_origin";

const WORKING_DAYS: &str = "working_days";

/// The S2.6 name of the shared binding exposure.
pub type CalculationBindingExposure = ExpressionBindingExposure;

/// The S2.6 name of the shared binding.
pub type CalculationScopeBinding = ExpressionScopeBinding;

/// Why calculation analysis could not run (as opposed to a qualification issue).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CalculationAnalysisError {
    #[error("invalid calculation input: {reason}")]
    InvalidInput { reason: String },
}

/// A deterministic qualification problem. These are analysis categories, not rule IDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalculationIssue {
    InvalidGrounding {
        reason: String,
    },
    InvalidName {
        reason: String,
    },
    InvalidScope {
        reason: String,
    },
    ReservedSymbol {
        node_ref: Id,
    },
    UnbindablePayloadName {
        node_ref: Id,
        name: String,
    },
    ScopeBindingConflict {
        symbol: String,
        owner_ref: Option<Id>,
    },
    ScopeUnavailable {
        calculation_ref: Id,
    },
    UnexpectedScopeOverride {
        calculation_ref: Id,
    },
    StaleScopeBinding {
        node_ref: Id,
        reason: String,
    },
    UnknownInputRef {
        node_ref: Id,
    },
    UnsupportedInputType {
        node_ref: Id,
        reason: String,
    },
    UnsupportedResultType {
        result_type: String,
    },
    UndefinedInput {
        identifier: String,
    },
    ParseError {
        offset: Option<usize>,
    },
    TypeCheckError {
        reason: String,
    },
    ResultTypeMismatch {
        expected: String,
        actual: String,
    },
    UnitMismatch {
        expected: String,
        actual: String,
    },
    InvalidRounding {
        rounding: String,
    },
    RoundingNotApplicable {
        rounding: String,
    },
    CalendarRequired,
    CalendarUnresolved {
        calendar_ref: Id,
    },
    ExistingCalculationConflict {
        node_ref: Id,
    },
    InferenceUnavailable,
}

impl From<ExpressionScopeIssue> for CalculationIssue {
    fn from(issue: ExpressionScopeIssue) -> CalculationIssue {
        match issue {
            ExpressionScopeIssue::InvalidScope { reason } => {
                CalculationIssue::InvalidScope { reason }
            }
            ExpressionScopeIssue::ReservedSymbol { node_ref } => {
                CalculationIssue::ReservedSymbol { node_ref }
            }
            ExpressionScopeIssue::ScopeBindingConflict { symbol, owner_ref } => {
                CalculationIssue::ScopeBindingConflict { symbol, owner_ref }
            }
            ExpressionScopeIssue::UnknownInputRef { node_ref } => {
                CalculationIssue::UnknownInputRef { node_ref }
            }
            ExpressionScopeIssue::UnsupportedInputType { node_ref, reason } => {
                CalculationIssue::UnsupportedInputType { node_ref, reason }
            }
        }
    }
}

impl From<ResultTypeIssue> for CalculationIssue {
    fn from(issue: ResultTypeIssue) -> CalculationIssue {
        match issue {
            ResultTypeIssue::UnsupportedResultType { result_type } => {
                CalculationIssue::UnsupportedResultType { result_type }
            }
            ResultTypeIssue::UnitMismatch { expected, actual } => {
                CalculationIssue::UnitMismatch { expected, actual }
            }
        }
    }
}

/// An explicit calculation scope: the input bindings and the Accepted Calendars inference may
/// select (which are not symbols).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationScope {
    pub bindings: Vec<CalculationScopeBinding>,
    pub calendar_refs: Vec<Id>,
}

fn is_accepted(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
}

/// The declared expression type of a Calculation's result.
fn calculation_result_type(result_type: &str, unit: Option<&str>) -> Result<Ty, CalculationIssue> {
    declared_result_type(result_type, unit).map_err(CalculationIssue::from)
}

/// Validates calculation bindings (the root `calendar` is reserved) and builds their TypeEnv.
pub fn validate_calculation_bindings(
    graph: &Graph,
    bindings: &[CalculationScopeBinding],
) -> Result<ValidatedExpressionScope, CalculationIssue> {
    validate_expression_bindings(graph, bindings, &[CALCULATION_CALENDAR_SYMBOL])
        .map_err(CalculationIssue::from)
}

/// Whether `calendar_ref` names an Accepted Calendar.
pub fn accepted_calendar(graph: &Graph, calendar_ref: &Id) -> bool {
    graph
        .node(calendar_ref)
        .is_some_and(|n| is_accepted(n) && matches!(n.payload, NodePayload::Calendar(_)))
}

/// Validates a full scope: its bindings and its allowed Accepted Calendars.
pub fn validate_calculation_scope(
    graph: &Graph,
    scope: &CalculationScope,
) -> Result<ValidatedExpressionScope, CalculationIssue> {
    let validated = validate_calculation_bindings(graph, &scope.bindings)?;
    let unique: BTreeSet<&Id> = scope.calendar_refs.iter().collect();
    if unique.len() != scope.calendar_refs.len() {
        return Err(CalculationIssue::InvalidScope {
            reason: "duplicate calendar reference".into(),
        });
    }
    for calendar in &scope.calendar_refs {
        if !accepted_calendar(graph, calendar) {
            return Err(CalculationIssue::CalendarUnresolved {
                calendar_ref: calendar.clone(),
            });
        }
    }
    Ok(validated)
}

// ============================================================================ qualification

/// An expression ready for deterministic evaluation.
#[derive(Debug, Clone)]
pub struct PreparedCalculation {
    pub ast: Ast,
    pub type_env: TypeEnv,
    pub calendar_ref: Option<Id>,
    pub expected_ty: Ty,
    pub rounding: Option<RoundingSpec>,
}

/// Deterministic qualification material of one calculation, reusable by F2 validation.
#[derive(Debug, Clone)]
pub struct CalculationQualification {
    /// The canonical expression `unparse(parse(expression))`, when it parses.
    pub canonical_expression: Option<String>,
    pub expected_ty: Option<Ty>,
    pub actual_ty: Option<Ty>,
    /// The canonical scope bindings the expression actually uses.
    pub used_bindings: Vec<CalculationScopeBinding>,
    pub undefined_inputs: Vec<String>,
    /// Accepted Calculations used through Root bindings, sorted.
    pub dependencies: Vec<Id>,
    /// Whether the expression needs a business calendar (`working_days`).
    pub requires_calendar: bool,
    pub issues: Vec<CalculationIssue>,
    pub prepared: Option<PreparedCalculation>,
}

impl CalculationQualification {
    /// Qualified without issues.
    pub fn is_qualified(&self) -> bool {
        self.issues.is_empty() && self.prepared.is_some()
    }

    pub fn failed(issue: CalculationIssue) -> CalculationQualification {
        CalculationQualification {
            canonical_expression: None,
            expected_ty: None,
            actual_ty: None,
            used_bindings: Vec::new(),
            undefined_inputs: Vec::new(),
            dependencies: Vec::new(),
            requires_calendar: false,
            issues: vec![issue],
            prepared: None,
        }
    }
}

fn identifiers<'a>(ast: &'a Ast, out: &mut Vec<&'a Identifier>) {
    match ast {
        Ast::Literal(_) => {}
        Ast::Identifier(identifier) => out.push(identifier),
        Ast::List(items) | Ast::Call { args: items, .. } => {
            items.iter().for_each(|item| identifiers(item, out))
        }
        Ast::Unary { expr, .. } => identifiers(expr, out),
        Ast::Binary { left, right, .. } => {
            identifiers(left, out);
            identifiers(right, out);
        }
        Ast::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            identifiers(condition, out);
            identifiers(then_expr, out);
            identifiers(else_expr, out);
        }
    }
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

/// Resolves every identifier against the explicit scope, the reserved calendar and the
/// registered enum members; returns the used bindings and the undefined identifiers.
fn used_bindings(
    graph: &Graph,
    ast: &Ast,
    scope: &ValidatedExpressionScope,
    calendar_bound: bool,
) -> (Vec<CalculationScopeBinding>, Vec<String>) {
    let mut found = Vec::new();
    identifiers(ast, &mut found);
    let enum_members: BTreeSet<&Symbol> = scope
        .bindings
        .iter()
        .filter_map(|b| scope.env.enum_members(&b.node_ref))
        .flatten()
        .collect();
    let mut used = BTreeSet::new();
    let mut undefined = BTreeSet::new();
    for identifier in found {
        let segments = identifier.segments();
        let root = segments[0].as_str();
        if segments.len() == 1 && root == CALCULATION_CALENDAR_SYMBOL && calendar_bound {
            continue;
        }
        let Some(root_binding) = scope.roots.get(root) else {
            if !(segments.len() == 1 && enum_members.contains(&segments[0])) {
                undefined.insert(identifier.to_string());
            }
            continue;
        };
        used.insert(root_binding.clone());
        if let Some(field) = segments.get(1) {
            let is_entity = graph
                .node(&root_binding.node_ref)
                .is_some_and(|n| matches!(n.payload, NodePayload::Entity(_)));
            match scope
                .fields
                .get(&(root_binding.node_ref.clone(), field.as_str().to_owned()))
            {
                Some(field_binding) if is_entity => {
                    used.insert(field_binding.clone());
                }
                _ => {
                    undefined.insert(identifier.to_string());
                }
            }
        }
    }
    (used.into_iter().collect(), undefined.into_iter().collect())
}

/// Whether the actual type satisfies the declared result type and unit.
fn result_compatibility(expected: &Ty, actual: &Ty) -> Option<CalculationIssue> {
    let mismatch = || CalculationIssue::ResultTypeMismatch {
        expected: format!("{expected:?}"),
        actual: format!("{actual:?}"),
    };
    let unit_mismatch = || CalculationIssue::UnitMismatch {
        expected: format!("{expected:?}"),
        actual: format!("{actual:?}"),
    };
    match (expected, actual) {
        (Ty::Quantity(u), Ty::Quantity(v)) if u == v => None,
        (Ty::Quantity(_), _) | (_, Ty::Quantity(_)) | (_, Ty::Duration(_)) => Some(unit_mismatch()),
        (Ty::Int, Ty::Int) => None,
        (Ty::Decimal(_), Ty::Int) => None,
        (Ty::Decimal(n), Ty::Decimal(m)) if m <= n => None,
        (Ty::String, Ty::String)
        | (Ty::Bool, Ty::Bool)
        | (Ty::Date, Ty::Date)
        | (Ty::DateTime, Ty::DateTime) => None,
        _ => Some(mismatch()),
    }
}

/// The semantic content of a calculation to qualify.
#[derive(Debug, Clone, Copy)]
pub struct CalculationDraft<'a> {
    pub expression: &'a str,
    pub result_type: &'a str,
    pub unit: Option<&'a str>,
    pub rounding: Option<&'a str>,
    pub calendar_ref: Option<&'a Id>,
}

/// Qualifies an expression under explicit bindings and a calendar reference.
pub fn qualify_calculation(
    graph: &Graph,
    draft: &CalculationDraft<'_>,
    scope: Result<ValidatedExpressionScope, CalculationIssue>,
) -> CalculationQualification {
    let ast = match parse(draft.expression) {
        Ok(ast) => ast,
        Err(e) => {
            return CalculationQualification::failed(CalculationIssue::ParseError {
                offset: e.offset(),
            })
        }
    };
    let mut qualification = CalculationQualification {
        canonical_expression: None,
        expected_ty: None,
        actual_ty: None,
        used_bindings: Vec::new(),
        undefined_inputs: Vec::new(),
        dependencies: Vec::new(),
        requires_calendar: false,
        issues: Vec::new(),
        prepared: None,
    };
    qualification.canonical_expression = Some(unparse(&ast));
    qualification.requires_calendar = calls(&ast, WORKING_DAYS);
    let mut scope = match scope {
        Ok(scope) => scope,
        Err(issue) => {
            qualification.issues.push(issue);
            return qualification;
        }
    };
    let mut issues = Vec::new();
    let mut calendar_bound = false;
    if let Some(calendar) = draft.calendar_ref {
        if accepted_calendar(graph, calendar) {
            let reserved = Symbol::new(CALCULATION_CALENDAR_SYMBOL)
                .unwrap_or_else(|_| unreachable!("calendar is a valid symbol"));
            match scope
                .env
                .set_calendar_ref_type(calendar.clone())
                .and_then(|()| scope.env.bind_root(reserved, Ty::Ref(calendar.clone())))
            {
                Ok(()) => calendar_bound = true,
                Err(e) => issues.push(CalculationIssue::InvalidScope {
                    reason: e.to_string(),
                }),
            }
        } else {
            issues.push(CalculationIssue::CalendarUnresolved {
                calendar_ref: calendar.clone(),
            });
        }
    } else if qualification.requires_calendar {
        issues.push(CalculationIssue::CalendarRequired);
    }
    let (used, undefined) = used_bindings(graph, &ast, &scope, calendar_bound);
    qualification.dependencies = used
        .iter()
        .filter(|b| b.exposure == ExpressionBindingExposure::Root)
        .filter(|b| {
            graph
                .node(&b.node_ref)
                .is_some_and(|n| matches!(n.payload, NodePayload::Calculation(_)))
        })
        .map(|b| b.node_ref.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    qualification.used_bindings = used;
    for identifier in &undefined {
        issues.push(CalculationIssue::UndefinedInput {
            identifier: identifier.clone(),
        });
    }
    qualification.undefined_inputs = undefined;
    let expected = match calculation_result_type(draft.result_type, draft.unit) {
        Ok(ty) => Some(ty),
        Err(issue) => {
            issues.push(issue);
            None
        }
    };
    qualification.expected_ty = expected.clone();
    if qualification.undefined_inputs.is_empty() {
        match typecheck(&ast, &scope.env) {
            Ok(actual) => {
                if let Some(issue) = expected
                    .as_ref()
                    .and_then(|e| result_compatibility(e, &actual))
                {
                    issues.push(issue);
                }
                qualification.actual_ty = Some(actual);
            }
            Err(e) => issues.push(CalculationIssue::TypeCheckError {
                reason: e.to_string(),
            }),
        }
    }
    let rounding = match draft.rounding {
        None => None,
        Some(text) => match text.parse::<RoundingSpec>() {
            Err(_) => {
                issues.push(CalculationIssue::InvalidRounding {
                    rounding: text.to_owned(),
                });
                None
            }
            Ok(spec) => {
                if !matches!(expected, Some(Ty::Decimal(_) | Ty::Quantity(_))) {
                    issues.push(CalculationIssue::RoundingNotApplicable {
                        rounding: text.to_owned(),
                    });
                }
                Some(spec)
            }
        },
    };
    issues.sort();
    issues.dedup();
    qualification.issues = issues;
    if qualification.issues.is_empty() {
        if let Some(expected_ty) = expected {
            qualification.prepared = Some(PreparedCalculation {
                ast,
                type_env: scope.env,
                calendar_ref: draft.calendar_ref.cloned(),
                expected_ty,
                rounding,
            });
        }
    }
    qualification
}

// ============================================================================ origin and accepted

/// A zero-based, end-exclusive UTF-8 byte range of a target Requirement statement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationGroundedRange {
    pub requirement_ref: Id,
    pub start: u64,
    pub end: u64,
}

/// Provenance and replay scope (`plumb_functional:calculation_origin`): the grounding and the
/// exact canonical bindings the expression uses. The calendar is not a binding; it is
/// `Calculation.calendar_ref`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationOrigin {
    pub name_range: CalculationGroundedRange,
    pub expression_evidence: Vec<CalculationGroundedRange>,
    pub used_bindings: Vec<CalculationScopeBinding>,
}

/// The valid S2.6 calculation origin of a node, if any.
pub fn calculation_origin_of(node: &Node) -> Option<CalculationOrigin> {
    node.extensions
        .iter()
        .find(|(k, _)| k.as_str() == CALCULATION_ORIGIN_EXTENSION)
        .and_then(|(_, v)| serde_json::from_value(v.clone()).ok())
}

/// Qualification of one Accepted Calculation.
#[derive(Debug, Clone)]
pub struct AcceptedCalculationQualification {
    pub calculation_ref: Id,
    /// Whether the replay scope came from a valid S2.6 origin.
    pub from_origin: bool,
    pub qualification: CalculationQualification,
    /// Whether the calculation is a member of a dependency cycle (set by cycle analysis).
    pub in_cycle: bool,
}

/// Qualifies an Accepted Calculation. An S2.6 origin is the authoritative replay scope (an
/// override for such a node is rejected); a calculation without that origin needs an explicit
/// override, and without one its scope is unavailable rather than guessed.
pub fn qualify_accepted_calculation(
    graph: &Graph,
    calculation_ref: &Id,
    scope_override: Option<&CalculationScope>,
) -> Result<AcceptedCalculationQualification, CalculationAnalysisError> {
    let node = graph
        .node(calculation_ref)
        .filter(|n| is_accepted(n))
        .ok_or_else(|| CalculationAnalysisError::InvalidInput {
            reason: format!("{calculation_ref} is not an Accepted node"),
        })?;
    let NodePayload::Calculation(calculation) = &node.payload else {
        return Err(CalculationAnalysisError::InvalidInput {
            reason: format!("{calculation_ref} is not a Calculation"),
        });
    };
    let origin = calculation_origin_of(node);
    let from_origin = origin.is_some();
    let done = |qualification: CalculationQualification| AcceptedCalculationQualification {
        calculation_ref: calculation_ref.clone(),
        from_origin,
        qualification,
        in_cycle: false,
    };
    let scope = match (&origin, scope_override) {
        (Some(_), Some(_)) => {
            return Ok(done(CalculationQualification::failed(
                CalculationIssue::UnexpectedScopeOverride {
                    calculation_ref: calculation_ref.clone(),
                },
            )))
        }
        (None, None) => {
            return Ok(done(CalculationQualification::failed(
                CalculationIssue::ScopeUnavailable {
                    calculation_ref: calculation_ref.clone(),
                },
            )))
        }
        (Some(origin), None) => validate_calculation_bindings(graph, &origin.used_bindings)
            .map_err(|issue| CalculationIssue::StaleScopeBinding {
                node_ref: match &issue {
                    CalculationIssue::UnknownInputRef { node_ref }
                    | CalculationIssue::UnsupportedInputType { node_ref, .. }
                    | CalculationIssue::ReservedSymbol { node_ref } => node_ref.clone(),
                    _ => calculation_ref.clone(),
                },
                reason: format!("{issue:?}"),
            }),
        (None, Some(scope)) => validate_calculation_bindings(graph, &scope.bindings),
    };
    let draft = CalculationDraft {
        expression: &calculation.expression,
        result_type: &calculation.result_type,
        unit: calculation.unit.as_deref(),
        rounding: calculation.rounding.as_deref(),
        calendar_ref: calculation.calendar_ref.as_ref(),
    };
    Ok(done(qualify_calculation(graph, &draft, scope)))
}

/// Accepted-calculation qualification with deterministic dependency cycles.
#[derive(Debug, Clone)]
pub struct CalculationCycleAnalysis {
    pub qualifications: Vec<AcceptedCalculationQualification>,
    /// Self-cycles and multi-node strongly connected components, sorted.
    pub cycles: Vec<Vec<Id>>,
    /// Calculations whose scope is unavailable, so their dependencies are unknown.
    pub incomplete: Vec<Id>,
}

/// Qualifies every Accepted Calculation (with explicit overrides for legacy ones) and
/// reports the dependency cycles among them; unavailable scopes are never guessed.
pub fn analyze_calculation_cycles(
    graph: &Graph,
    overrides: &BTreeMap<Id, CalculationScope>,
) -> Result<CalculationCycleAnalysis, CalculationAnalysisError> {
    let mut qualifications = Vec::new();
    for id in graph.node_ids_by_type(NodeType::Calculation) {
        if graph.node(id).is_some_and(is_accepted) {
            qualifications.push(qualify_accepted_calculation(graph, id, overrides.get(id))?);
        }
    }
    let ids: Vec<Id> = qualifications
        .iter()
        .map(|q| q.calculation_ref.clone())
        .collect();
    let index: BTreeMap<&Id, usize> = ids.iter().enumerate().map(|(i, id)| (id, i)).collect();
    let edges: Vec<Vec<usize>> = qualifications
        .iter()
        .map(|q| {
            q.qualification
                .dependencies
                .iter()
                .filter_map(|d| index.get(d).copied())
                .collect()
        })
        .collect();
    let mut cycles: Vec<Vec<Id>> = strongly_connected(&edges)
        .into_iter()
        .filter(|component| component.len() > 1 || edges[component[0]].contains(&component[0]))
        .map(|component| {
            let mut members: Vec<Id> = component.into_iter().map(|i| ids[i].clone()).collect();
            members.sort();
            members
        })
        .collect();
    cycles.sort();
    let in_cycle: BTreeSet<&Id> = cycles.iter().flatten().collect();
    let incomplete = qualifications
        .iter()
        .filter(|q| {
            q.qualification
                .issues
                .iter()
                .any(|i| matches!(i, CalculationIssue::ScopeUnavailable { .. }))
        })
        .map(|q| q.calculation_ref.clone())
        .collect();
    let qualifications = qualifications
        .into_iter()
        .map(|mut q| {
            q.in_cycle = in_cycle.contains(&q.calculation_ref);
            q
        })
        .collect();
    Ok(CalculationCycleAnalysis {
        qualifications,
        cycles,
        incomplete,
    })
}

/// Tarjan's strongly connected components over node indices (deterministic order).
fn strongly_connected(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct State<'e> {
        edges: &'e [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        components: Vec<Vec<usize>>,
    }
    fn visit(s: &mut State<'_>, v: usize) {
        s.index[v] = Some(s.next);
        s.low[v] = s.next;
        s.next += 1;
        s.stack.push(v);
        s.on_stack[v] = true;
        for &w in &s.edges[v] {
            match s.index[w] {
                None => {
                    visit(s, w);
                    s.low[v] = s.low[v].min(s.low[w]);
                }
                Some(iw) if s.on_stack[w] => s.low[v] = s.low[v].min(iw),
                Some(_) => {}
            }
        }
        if Some(s.low[v]) == s.index[v] {
            let mut component = Vec::new();
            while let Some(w) = s.stack.pop() {
                s.on_stack[w] = false;
                component.push(w);
                if w == v {
                    break;
                }
            }
            s.components.push(component);
        }
    }
    let n = edges.len();
    let mut state = State {
        edges,
        index: vec![None; n],
        low: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        next: 0,
        components: Vec::new(),
    };
    for v in 0..n {
        if state.index[v].is_none() {
            visit(&mut state, v);
        }
    }
    state.components
}
