//! Grounded calculations over explicit PlumbExpr scopes (plan S2.6; compiler architecture §8).
//!
//! A supplied, already-acquired `calculation_analysis` inference names a calculation by a
//! grounded requirement range, proposes a PlumbExpr expression with grounded evidence and
//! declares its result type, unit, rounding and calendar. Plumb resolves the expression only
//! through an explicit [`CalculationScope`]: every input symbol is an explicit binding of an
//! Accepted Entity, Attribute or Calculation, PSG names are never rewritten into identifiers,
//! and the root symbol `calendar` is reserved for `Calculation.calendar_ref`. The S2.4 parser
//! and S2.5 type checker, evaluator and rounding are reused; nothing here emits validation
//! findings, mutates a graph, calls a provider, reads a clock or persists anything, and
//! `apply_patch` is used solely to dry-validate proposals in memory.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_expr::{
    eval, map_pilot_value_type, parse, round_decimal, typecheck, unparse, Ast, CalendarProvider,
    EvalContext, EvalError, Identifier, PredicateProvider, RefValue, RoundingSpec, Symbol, Ty,
    TypeEnv, Value, ValueEnv,
};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AuditMeta, Calculation, DerivationRef, ElementStatus, EvidenceRef, ExtensionKey, Graph, Node,
    NodePayload, NodeType, RelationKind,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

/// The reserved root symbol bound only from `Calculation.calendar_ref`.
pub const CALCULATION_CALENDAR_SYMBOL: &str = "calendar";

/// Version of [`CalculationContext`].
pub const CALCULATION_CONTEXT_VERSION: u32 = 1;

/// Version of the calculation inference output.
pub const CALCULATION_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of calculation analysis.
pub const CALCULATION_TASK_KIND: &str = "calculation_analysis";

/// Extension key of the provenance [`CalculationOrigin`].
pub const CALCULATION_ORIGIN_EXTENSION: &str = "plumb_functional:calculation_origin";

const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s2-calculation.md");
const SCHEMA_SOURCE: &str = include_str!("../../../schemas/inference/s2-calculation.schema.json");
const CALCULATION_STAGE: StageId = StageId::S2;
const WORKING_DAYS: &str = "working_days";

// ============================================================================ errors and issues

/// Why calculation analysis could not run. Malformed supplied inference and invalid
/// requests are errors; semantic problems of a candidate are [`CalculationIssue`]s.
#[derive(Debug, Error)]
pub enum CalculationError {
    #[error("invalid calculation input: {reason}")]
    InvalidInput { reason: String },
    #[error("invalid calculation scope: {0:?}")]
    Scope(CalculationIssue),
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    #[error("invalid calculation inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error("calculation schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    #[error("calculation output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("invalid grounding {requirement_ref} {start}..{end}: {reason}")]
    InvalidGrounding {
        requirement_ref: Id,
        start: u64,
        end: u64,
        reason: String,
    },
    #[error("calendar {calendar_ref} is not an allowed calendar of the request")]
    CalendarNotAllowed { calendar_ref: Id },
    #[error("duplicate calculation candidate {calculation_ref}")]
    DuplicateCandidate { calculation_ref: Id },
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_input(reason: impl Into<String>) -> CalculationError {
    CalculationError::InvalidInput {
        reason: reason.into(),
    }
}

fn invalid_proposal(e: impl ToString) -> CalculationError {
    CalculationError::InvalidProposal {
        reason: e.to_string(),
    }
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

// ============================================================================ scope

/// How a binding exposes its node to a PlumbExpr expression.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalculationBindingExposure {
    /// A top-level root symbol.
    Root,
    /// A field of one Entity's Ref type.
    Field { owner_ref: Id },
}

/// One explicit PSG-to-PlumbExpr binding. The symbol is validated through the exact S2.4
/// Symbol constructor; it is never derived by rewriting a PSG name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationScopeBinding {
    pub node_ref: Id,
    pub symbol: String,
    pub exposure: CalculationBindingExposure,
}

impl CalculationScopeBinding {
    /// The canonical order: Root before Field, then owner, symbol and node.
    fn order_key(&self) -> (u8, Option<&Id>, &str, &Id) {
        match &self.exposure {
            CalculationBindingExposure::Root => (0, None, &self.symbol, &self.node_ref),
            CalculationBindingExposure::Field { owner_ref } => {
                (1, Some(owner_ref), &self.symbol, &self.node_ref)
            }
        }
    }
}

impl PartialOrd for CalculationScopeBinding {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CalculationScopeBinding {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.order_key().cmp(&other.order_key())
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

fn payload_name(node: &Node) -> Option<&str> {
    match &node.payload {
        NodePayload::Entity(e) => Some(&e.name),
        NodePayload::Attribute(a) => Some(&a.name),
        NodePayload::Calculation(c) => Some(&c.name),
        _ => None,
    }
}

/// The exact payload name as a binding symbol, when it already is a valid non-reserved Symbol.
fn default_symbol(graph: &Graph, node_ref: &Id) -> Result<String, CalculationIssue> {
    let node = graph
        .node(node_ref)
        .ok_or_else(|| CalculationIssue::UnknownInputRef {
            node_ref: node_ref.clone(),
        })?;
    let name = payload_name(node).ok_or_else(|| CalculationIssue::InvalidScope {
        reason: format!("{node_ref} has no bindable payload name"),
    })?;
    match Symbol::new(name) {
        Ok(_) if name != CALCULATION_CALENDAR_SYMBOL => Ok(name.to_owned()),
        _ => Err(CalculationIssue::UnbindablePayloadName {
            node_ref: node_ref.clone(),
            name: name.to_owned(),
        }),
    }
}

/// A Root binding named by the exact payload name, only when it is already a valid Symbol.
pub fn default_root_binding(
    graph: &Graph,
    node_ref: &Id,
) -> Result<CalculationScopeBinding, CalculationIssue> {
    Ok(CalculationScopeBinding {
        node_ref: node_ref.clone(),
        symbol: default_symbol(graph, node_ref)?,
        exposure: CalculationBindingExposure::Root,
    })
}

/// A Field binding named by the exact Attribute name, only when it is already a valid Symbol.
pub fn default_field_binding(
    graph: &Graph,
    attribute_ref: &Id,
    owner_ref: &Id,
) -> Result<CalculationScopeBinding, CalculationIssue> {
    Ok(CalculationScopeBinding {
        node_ref: attribute_ref.clone(),
        symbol: default_symbol(graph, attribute_ref)?,
        exposure: CalculationBindingExposure::Field {
            owner_ref: owner_ref.clone(),
        },
    })
}

/// The canonical Accepted Entity owning an Attribute through an Accepted `has_attribute`.
fn accepted_owner<'g>(graph: &'g Graph, attribute: &Id) -> Option<&'g Id> {
    let owners: Vec<&Id> = graph
        .incoming_edge_ids(attribute)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == RelationKind::HasAttribute && e.status == ElementStatus::Accepted)
        .map(|e| &e.from)
        .filter(|from| {
            graph
                .node(from)
                .is_some_and(|n| is_accepted(n) && matches!(n.payload, NodePayload::Entity(_)))
        })
        .collect();
    match owners.as_slice() {
        [only] => Some(only),
        _ => None,
    }
}

/// The expression type of an Accepted Attribute and, for an enum, its members.
fn attribute_type(node: &Node) -> Result<(Ty, Option<Vec<Symbol>>), CalculationIssue> {
    let NodePayload::Attribute(a) = &node.payload else {
        return Err(CalculationIssue::InvalidScope {
            reason: format!("{} is not an Attribute", node.id),
        });
    };
    let unsupported = |reason: String| CalculationIssue::UnsupportedInputType {
        node_ref: node.id.clone(),
        reason,
    };
    let is_enum = a.value_type == "Enum";
    let ty = map_pilot_value_type(
        &a.value_type,
        a.unit.as_deref(),
        is_enum.then_some(&node.id),
    )
    .map_err(|e| unsupported(e.to_string()))?;
    if !is_enum {
        return Ok((ty, None));
    }
    let members = a
        .enum_values
        .as_ref()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| unsupported("Enum attribute without enum_values".into()))?;
    let symbols = members
        .iter()
        .map(|m| {
            Symbol::new(m).map_err(|_| unsupported(format!("enum member {m:?} is not a symbol")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((ty, Some(symbols)))
}

/// The declared expression type of a Calculation's result.
fn calculation_result_type(result_type: &str, unit: Option<&str>) -> Result<Ty, CalculationIssue> {
    if result_type == "Enum" {
        return Err(CalculationIssue::UnsupportedResultType {
            result_type: result_type.to_owned(),
        });
    }
    match map_pilot_value_type(result_type, unit, None) {
        Ok(ty) => Ok(ty),
        Err(plumb_expr::TypeBindingError::UnitOnNonNumericType(_))
        | Err(plumb_expr::TypeBindingError::InvalidUnit(_)) => {
            Err(CalculationIssue::UnitMismatch {
                expected: result_type.to_owned(),
                actual: unit.unwrap_or_default().to_owned(),
            })
        }
        Err(_) => Err(CalculationIssue::UnsupportedResultType {
            result_type: result_type.to_owned(),
        }),
    }
}

/// A validated scope: canonical bindings, the root and field lookups and the base TypeEnv.
#[derive(Debug, Clone)]
struct ValidatedScope {
    bindings: Vec<CalculationScopeBinding>,
    roots: BTreeMap<String, CalculationScopeBinding>,
    fields: BTreeMap<(Id, String), CalculationScopeBinding>,
    env: TypeEnv,
}

fn conflict(binding: &CalculationScopeBinding) -> CalculationIssue {
    CalculationIssue::ScopeBindingConflict {
        symbol: binding.symbol.clone(),
        owner_ref: match &binding.exposure {
            CalculationBindingExposure::Root => None,
            CalculationBindingExposure::Field { owner_ref } => Some(owner_ref.clone()),
        },
    }
}

/// Validates bindings against the graph and builds their TypeEnv (no calendar).
fn validate_bindings(
    graph: &Graph,
    bindings: &[CalculationScopeBinding],
) -> Result<ValidatedScope, CalculationIssue> {
    let mut sorted = bindings.to_vec();
    sorted.sort();
    let mut scope = ValidatedScope {
        bindings: Vec::new(),
        roots: BTreeMap::new(),
        fields: BTreeMap::new(),
        env: TypeEnv::new(),
    };
    let invalid = |reason: String| CalculationIssue::InvalidScope { reason };
    for binding in sorted {
        if scope.bindings.contains(&binding) {
            return Err(conflict(&binding));
        }
        let symbol = Symbol::new(&binding.symbol)
            .map_err(|_| invalid(format!("{:?} is not a PlumbExpr symbol", binding.symbol)))?;
        let node =
            graph
                .node(&binding.node_ref)
                .ok_or_else(|| CalculationIssue::UnknownInputRef {
                    node_ref: binding.node_ref.clone(),
                })?;
        if !is_accepted(node) {
            return Err(invalid(format!("{} is not Accepted", node.id)));
        }
        match &binding.exposure {
            CalculationBindingExposure::Root => {
                if binding.symbol == CALCULATION_CALENDAR_SYMBOL {
                    return Err(CalculationIssue::ReservedSymbol {
                        node_ref: binding.node_ref.clone(),
                    });
                }
                if scope.roots.contains_key(&binding.symbol) {
                    return Err(conflict(&binding));
                }
                let ty = match &node.payload {
                    NodePayload::Entity(_) => Ty::Ref(node.id.clone()),
                    NodePayload::Attribute(_) => {
                        let (ty, members) = attribute_type(node)?;
                        if let Some(members) = members {
                            scope
                                .env
                                .define_enum(node.id.clone(), members)
                                .map_err(|e| invalid(e.to_string()))?;
                        }
                        ty
                    }
                    NodePayload::Calculation(c) => {
                        calculation_result_type(&c.result_type, c.unit.as_deref()).map_err(|e| {
                            CalculationIssue::UnsupportedInputType {
                                node_ref: node.id.clone(),
                                reason: format!("{e:?}"),
                            }
                        })?
                    }
                    _ => {
                        return Err(invalid(format!(
                            "{} cannot be a Root binding",
                            node.payload.node_type().as_str()
                        )))
                    }
                };
                scope
                    .env
                    .bind_root(symbol, ty)
                    .map_err(|_| conflict(&binding))?;
                scope.roots.insert(binding.symbol.clone(), binding.clone());
            }
            CalculationBindingExposure::Field { owner_ref } => {
                if !matches!(node.payload, NodePayload::Attribute(_)) {
                    return Err(invalid(format!(
                        "{} cannot be a Field binding",
                        node.payload.node_type().as_str()
                    )));
                }
                if accepted_owner(graph, &node.id) != Some(owner_ref) {
                    return Err(invalid(format!(
                        "{owner_ref} is not the canonical owner of {}",
                        node.id
                    )));
                }
                let key = (owner_ref.clone(), binding.symbol.clone());
                if scope.fields.contains_key(&key) {
                    return Err(conflict(&binding));
                }
                let (ty, members) = attribute_type(node)?;
                if let Some(members) = members {
                    scope
                        .env
                        .define_enum(node.id.clone(), members)
                        .map_err(|e| invalid(e.to_string()))?;
                }
                scope
                    .env
                    .bind_field(owner_ref.clone(), symbol, ty)
                    .map_err(|_| conflict(&binding))?;
                scope.fields.insert(key, binding.clone());
            }
        }
        scope.bindings.push(binding);
    }
    Ok(scope)
}

/// Whether `calendar_ref` names an Accepted Calendar.
fn accepted_calendar(graph: &Graph, calendar_ref: &Id) -> bool {
    graph
        .node(calendar_ref)
        .is_some_and(|n| is_accepted(n) && matches!(n.payload, NodePayload::Calendar(_)))
}

/// Validates a full scope: its bindings and its allowed Accepted Calendars.
fn validate_scope(
    graph: &Graph,
    scope: &CalculationScope,
) -> Result<ValidatedScope, CalculationIssue> {
    let validated = validate_bindings(graph, &scope.bindings)?;
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

    fn failed(issue: CalculationIssue) -> CalculationQualification {
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
    scope: &ValidatedScope,
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
struct CalculationDraft<'a> {
    expression: &'a str,
    result_type: &'a str,
    unit: Option<&'a str>,
    rounding: Option<&'a str>,
    calendar_ref: Option<&'a Id>,
}

/// Qualifies an expression under explicit bindings and a calendar reference.
fn qualify(
    graph: &Graph,
    draft: &CalculationDraft<'_>,
    scope: Result<ValidatedScope, CalculationIssue>,
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
        .filter(|b| b.exposure == CalculationBindingExposure::Root)
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

fn origin_of(node: &Node) -> Option<CalculationOrigin> {
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
) -> Result<AcceptedCalculationQualification, CalculationError> {
    let node = graph
        .node(calculation_ref)
        .filter(|n| is_accepted(n))
        .ok_or_else(|| invalid_input(format!("{calculation_ref} is not an Accepted node")))?;
    let NodePayload::Calculation(calculation) = &node.payload else {
        return Err(invalid_input(format!(
            "{calculation_ref} is not a Calculation"
        )));
    };
    let origin = origin_of(node);
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
        (Some(origin), None) => validate_bindings(graph, &origin.used_bindings).map_err(|issue| {
            CalculationIssue::StaleScopeBinding {
                node_ref: match &issue {
                    CalculationIssue::UnknownInputRef { node_ref }
                    | CalculationIssue::UnsupportedInputType { node_ref, .. }
                    | CalculationIssue::ReservedSymbol { node_ref } => node_ref.clone(),
                    _ => calculation_ref.clone(),
                },
                reason: format!("{issue:?}"),
            }
        }),
        (None, Some(scope)) => validate_bindings(graph, &scope.bindings),
    };
    let draft = CalculationDraft {
        expression: &calculation.expression,
        result_type: &calculation.result_type,
        unit: calculation.unit.as_deref(),
        rounding: calculation.rounding.as_deref(),
        calendar_ref: calculation.calendar_ref.as_ref(),
    };
    Ok(done(qualify(graph, &draft, scope)))
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
) -> Result<CalculationCycleAnalysis, CalculationError> {
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

// ============================================================================ evaluation

/// Why a prepared calculation could not be evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CalculationEvalError {
    #[error("evaluation: {0}")]
    Eval(#[from] EvalError),
    #[error("the caller's value environment already binds the reserved root calendar")]
    ReservedBindingConflict,
    #[error("rounding: {reason}")]
    Rounding { reason: String },
    #[error("result scale {scale} exceeds the declared Decimal({declared})")]
    ResultScaleExceeded { scale: u32, declared: u8 },
}

/// Evaluates a prepared calculation with the S2.5 evaluator, injecting the reserved calendar
/// Ref (`type_ref` and `value_ref` = calendar_ref) and applying only final rounding.
pub fn evaluate_calculation(
    prepared: &PreparedCalculation,
    values: &ValueEnv,
    calendars: &dyn CalendarProvider,
    predicates: &dyn PredicateProvider,
) -> Result<Value, CalculationEvalError> {
    let mut values = values.clone();
    if let Some(calendar) = &prepared.calendar_ref {
        let reserved = Symbol::new(CALCULATION_CALENDAR_SYMBOL)
            .unwrap_or_else(|_| unreachable!("calendar is a valid symbol"));
        let value = Value::Ref(RefValue {
            type_ref: calendar.clone(),
            value_ref: calendar.clone(),
            fields: BTreeMap::new(),
        });
        values
            .bind(reserved, value)
            .map_err(|_| CalculationEvalError::ReservedBindingConflict)?;
    }
    let value = eval(
        &prepared.ast,
        &EvalContext {
            types: &prepared.type_env,
            values: &values,
            calendars,
            predicates,
        },
    )?;
    let value = match (value, &prepared.expected_ty) {
        (Value::Int(i), Ty::Decimal(_)) => Value::Decimal(Decimal::from(i)),
        (v, _) => v,
    };
    let rounded = match (&prepared.rounding, value) {
        (None, v) => v,
        (Some(spec), Value::Decimal(d)) => {
            Value::Decimal(
                round_decimal(d, spec).map_err(|e| CalculationEvalError::Rounding {
                    reason: e.to_string(),
                })?,
            )
        }
        (Some(spec), Value::Quantity { value, unit }) => Value::Quantity {
            value: round_decimal(value, spec).map_err(|e| CalculationEvalError::Rounding {
                reason: e.to_string(),
            })?,
            unit,
        },
        (Some(_), _) => {
            return Err(CalculationEvalError::Rounding {
                reason: "rounding applies only to Decimal and Quantity results".into(),
            })
        }
    };
    if let (Value::Decimal(d), Ty::Decimal(declared)) = (&rounded, &prepared.expected_ty) {
        if d.scale() > u32::from(*declared) {
            return Err(CalculationEvalError::ResultScaleExceeded {
                scale: d.scale(),
                declared: *declared,
            });
        }
    }
    Ok(rounded)
}

// ============================================================================ request

/// One target Requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationRequirementContext {
    pub requirement_ref: Id,
    pub status: ElementStatus,
    pub statement: String,
}

/// One available PlumbExpr input symbol with its type metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationInputContext {
    pub node_ref: Id,
    pub symbol: String,
    pub exposure: CalculationBindingExposure,
    pub node_type: String,
    pub value_type: Option<String>,
    pub unit: Option<String>,
    pub enum_values: Option<Vec<String>>,
}

/// One Accepted Calendar inference may select; it is never a symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationCalendarContext {
    pub calendar_ref: Id,
    pub time_zone: String,
    pub region: Option<String>,
}

/// The exact calculation inference context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationContext {
    pub version: u32,
    pub project_id: Id,
    pub requirements: Vec<CalculationRequirementContext>,
    pub available_inputs: Vec<CalculationInputContext>,
    pub calendars: Vec<CalculationCalendarContext>,
}

impl CalculationContext {
    /// Generic SHA-256 of the RFC 8785 canonical JSON of the context.
    pub fn content_hash(&self) -> Result<Hash, CalculationError> {
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn requirement(&self, id: &Id) -> Option<&CalculationRequirementContext> {
        self.requirements
            .binary_search_by(|r| r.requirement_ref.cmp(id))
            .ok()
            .map(|i| &self.requirements[i])
    }

    fn input_refs(&self) -> Vec<Id> {
        let refs: BTreeSet<&Id> = self
            .requirements
            .iter()
            .map(|r| &r.requirement_ref)
            .chain(self.available_inputs.iter().map(|i| &i.node_ref))
            .chain(self.calendars.iter().map(|c| &c.calendar_ref))
            .collect();
        refs.into_iter().cloned().collect()
    }
}

/// A calculation `InferenceRequest` with its exact context and validated scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationRequest {
    pub request: InferenceRequest,
    pub context: CalculationContext,
    pub scope: CalculationScope,
}

fn context_of(
    graph: &Graph,
    target_requirement_refs: &[Id],
    scope: &CalculationScope,
) -> Result<(CalculationContext, CalculationScope), CalculationError> {
    let validated = validate_scope(graph, scope).map_err(CalculationError::Scope)?;
    let mut targets = BTreeSet::new();
    let mut requirements = Vec::new();
    for id in target_requirement_refs {
        if !targets.insert(id) {
            return Err(invalid_input(format!("duplicate target requirement {id}")));
        }
    }
    if targets.is_empty() {
        return Err(invalid_input("no target requirement"));
    }
    for id in targets {
        let node = graph
            .node(id)
            .ok_or_else(|| invalid_input(format!("requirement {id} does not exist")))?;
        let NodePayload::Requirement(r) = &node.payload else {
            return Err(invalid_input(format!("{id} is not a Requirement")));
        };
        if !matches!(
            node.status,
            ElementStatus::Proposed | ElementStatus::Accepted
        ) {
            return Err(invalid_input(format!(
                "requirement {id} is not Proposed or Accepted"
            )));
        }
        requirements.push(CalculationRequirementContext {
            requirement_ref: id.clone(),
            status: node.status,
            statement: r.statement.clone(),
        });
    }
    let mut available_inputs = Vec::new();
    for binding in &validated.bindings {
        let node = graph
            .node(&binding.node_ref)
            .ok_or_else(|| invalid_input(format!("{} does not exist", binding.node_ref)))?;
        let (value_type, unit, enum_values) = match &node.payload {
            NodePayload::Attribute(a) => (
                Some(a.value_type.clone()),
                a.unit.clone(),
                a.enum_values.clone(),
            ),
            NodePayload::Calculation(c) => (Some(c.result_type.clone()), c.unit.clone(), None),
            _ => (None, None, None),
        };
        available_inputs.push(CalculationInputContext {
            node_ref: binding.node_ref.clone(),
            symbol: binding.symbol.clone(),
            exposure: binding.exposure.clone(),
            node_type: node.payload.node_type().as_str().to_owned(),
            value_type,
            unit,
            enum_values,
        });
    }
    let mut calendar_refs = scope.calendar_refs.clone();
    calendar_refs.sort();
    let mut calendars = Vec::new();
    for calendar_ref in &calendar_refs {
        let Some(NodePayload::Calendar(c)) = graph.node(calendar_ref).map(|n| &n.payload) else {
            return Err(invalid_input(format!("{calendar_ref} is not a Calendar")));
        };
        calendars.push(CalculationCalendarContext {
            calendar_ref: calendar_ref.clone(),
            time_zone: c.time_zone.clone(),
            region: c.region.clone(),
        });
    }
    let context = CalculationContext {
        version: CALCULATION_CONTEXT_VERSION,
        project_id: graph.project_id().clone(),
        requirements,
        available_inputs,
        calendars,
    };
    let canonical = CalculationScope {
        bindings: validated.bindings,
        calendar_refs,
    };
    Ok((context, canonical))
}

fn context_evidence(graph: &Graph, context: &CalculationContext) -> Vec<Id> {
    let refs: BTreeSet<Id> = context
        .input_refs()
        .iter()
        .filter_map(|id| graph.node(id))
        .flat_map(|n| n.evidence.iter().map(|e| e.as_id().clone()))
        .collect();
    refs.into_iter().collect()
}

/// Builds the S2 calculation request over explicit target Requirements and an explicit scope.
pub fn build_calculation_request(
    graph: &Graph,
    target_requirement_refs: &[Id],
    scope: &CalculationScope,
    provider_policy: ProviderPolicy,
) -> Result<CalculationRequest, CalculationError> {
    let (context, scope) = context_of(graph, target_requirement_refs, scope)?;
    let request = InferenceRequest::new(
        CALCULATION_STAGE,
        CALCULATION_TASK_KIND.to_owned(),
        context.input_refs(),
        context_evidence(graph, &context),
        context.content_hash()?,
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    Ok(CalculationRequest {
        request,
        context,
        scope,
    })
}

impl CalculationRequest {
    /// The request is bound to its context, the committed prompt and schema.
    pub fn validate(&self) -> Result<(), CalculationError> {
        let r = &self.request;
        r.validate()
            .map_err(|e| invalid_input(format!("invalid inference request: {e}")))?;
        let ok = r.stage == CALCULATION_STAGE
            && r.task_kind == CALCULATION_TASK_KIND
            && r.input_refs == self.context.input_refs()
            && r.context_hash == self.context.content_hash()?
            && r.prompt_template_hash == Hash::content_sha256(PROMPT_TEMPLATE)
            && r.schema_hash == Hash::content_sha256(SCHEMA_SOURCE.as_bytes())
            && self.context.version == CALCULATION_CONTEXT_VERSION;
        if ok {
            Ok(())
        } else {
            Err(invalid_input(
                "request does not match its context, prompt or schema",
            ))
        }
    }
}

// ============================================================================ analysis

/// An already-acquired calculation artifact and the caller's provenance ref for it.
#[derive(Debug, Clone)]
pub struct CalculationInference<'a> {
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

/// Who owns the Proposed Calculation nodes and when, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalculationAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// What happened to one inferred candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalculationDisposition {
    Proposed { proposal_ref: Id },
    Ineligible,
    IdempotentReplay,
    ExistingEquivalent,
    ExistingConflict,
}

/// One inferred candidate with its deterministic identity and qualification.
#[derive(Debug, Clone)]
pub struct CalculationCandidateAnalysis {
    pub calculation_ref: Id,
    pub name: String,
    pub qualification: CalculationQualification,
    pub disposition: CalculationDisposition,
}

/// Everything one analysis produced; nothing has been applied or persisted.
#[derive(Debug, Clone)]
pub struct CalculationAnalysisResult {
    pub candidates: Vec<CalculationCandidateAnalysis>,
    pub proposals: Vec<Proposal>,
    pub issues: Vec<CalculationIssue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CalculationOutput {
    version: u32,
    calculations: Vec<OutputCalculation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputCalculation {
    name_range: CalculationGroundedRange,
    expression: String,
    expression_evidence: Vec<CalculationGroundedRange>,
    result_type: String,
    unit: Option<String>,
    rounding: Option<String>,
    calendar_ref: Option<Id>,
}

fn compile_schema() -> Result<JSONSchema, CalculationError> {
    let schema: JsonValue =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| CalculationError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| CalculationError::SchemaCompilation {
            reason: e.to_string(),
        })
}

fn grounded<'c>(
    context: &'c CalculationContext,
    g: &CalculationGroundedRange,
) -> Result<&'c str, CalculationError> {
    let invalid = |reason: &str| CalculationError::InvalidGrounding {
        requirement_ref: g.requirement_ref.clone(),
        start: g.start,
        end: g.end,
        reason: reason.to_owned(),
    };
    let requirement = context
        .requirement(&g.requirement_ref)
        .ok_or_else(|| invalid("not a target requirement"))?;
    let text = &requirement.statement;
    let (s, e) = match (usize::try_from(g.start), usize::try_from(g.end)) {
        (Ok(s), Ok(e)) if s < e && e <= text.len() => (s, e),
        _ => return Err(invalid("not a non-empty range of the statement")),
    };
    if !text.is_char_boundary(s) || !text.is_char_boundary(e) {
        return Err(invalid("not on UTF-8 character boundaries"));
    }
    Ok(&text[s..e])
}

/// `calculation:<16 hex of SHA-256(RFC 8785 {project_id, name, node_type: calculation})>`.
fn calculation_id(project_id: &Id, name: &str) -> Result<Id, CoreError> {
    let body = json!({"project_id": project_id, "name": name, "node_type": "calculation"});
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("calculation:{}", &hex[..16]).parse()
}

/// Analyzes a supplied calculation inference against its request. Absent inference yields
/// only InferenceUnavailable; malformed inference is an error.
pub fn analyze_calculations(
    graph: &Graph,
    request: &CalculationRequest,
    inference: Option<CalculationInference<'_>>,
    audit: &CalculationAudit,
) -> Result<CalculationAnalysisResult, CalculationError> {
    request.validate()?;
    let targets: Vec<Id> = request
        .context
        .requirements
        .iter()
        .map(|r| r.requirement_ref.clone())
        .collect();
    let (context, scope) = context_of(graph, &targets, &request.scope)?;
    if context != request.context || scope != request.scope {
        return Err(invalid_input(
            "graph does not match the calculation request context",
        ));
    }
    if request.request.evidence_refs != context_evidence(graph, &context) {
        return Err(invalid_input(
            "evidence_refs differ from the context evidence",
        ));
    }
    let Some(inference) = inference else {
        return Ok(CalculationAnalysisResult {
            candidates: Vec::new(),
            proposals: Vec::new(),
            issues: vec![CalculationIssue::InferenceUnavailable],
        });
    };
    inference
        .artifact
        .validate_for(&request.request)
        .map_err(|e| CalculationError::InvalidInferenceArtifact {
            reason: e.to_string(),
        })?;
    let output = inference.artifact.validated_output.as_value();
    if let Err(errors) = compile_schema()?.validate(output) {
        let mut messages: Vec<String> = errors
            .map(|e| format!("{e} at {}", e.instance_path))
            .collect();
        messages.sort();
        return Err(CalculationError::SchemaInvalid {
            reason: messages.join("; "),
        });
    }
    let decoded: CalculationOutput =
        serde_json::from_value(output.clone()).map_err(|e| CalculationError::SchemaInvalid {
            reason: e.to_string(),
        })?;
    if decoded.version != CALCULATION_OUTPUT_VERSION {
        return Err(CalculationError::SchemaInvalid {
            reason: format!("output version {} is not 1", decoded.version),
        });
    }
    let base = graph.semantic_hash()?;
    let extension: ExtensionKey = CALCULATION_ORIGIN_EXTENSION
        .parse()
        .map_err(invalid_proposal)?;
    let mut candidates = Vec::new();
    let mut proposals = Vec::new();
    let mut seen = BTreeSet::new();
    for candidate in decoded.calculations {
        // Grounding and calendar selection are part of the artifact contract.
        let raw_name = grounded(&context, &candidate.name_range)?;
        let mut evidence = candidate.expression_evidence.clone();
        for range in &evidence {
            grounded(&context, range)?;
        }
        evidence.sort();
        if evidence.windows(2).any(|p| p[0] == p[1]) {
            return Err(CalculationError::InvalidGrounding {
                requirement_ref: evidence[0].requirement_ref.clone(),
                start: evidence[0].start,
                end: evidence[0].end,
                reason: "duplicate expression evidence range".into(),
            });
        }
        if let Some(calendar) = &candidate.calendar_ref {
            if !scope.calendar_refs.contains(calendar) {
                return Err(CalculationError::CalendarNotAllowed {
                    calendar_ref: calendar.clone(),
                });
            }
        }
        let name = raw_name.trim().to_owned();
        let calculation_ref = calculation_id(graph.project_id(), &name)?;
        if !seen.insert(calculation_ref.clone()) {
            return Err(CalculationError::DuplicateCandidate { calculation_ref });
        }
        let name_issue = if name.is_empty() {
            Some("the grounded name is empty")
        } else if name.chars().any(char::is_control) {
            Some("the grounded name contains control characters")
        } else {
            None
        };
        let draft = CalculationDraft {
            expression: &candidate.expression,
            result_type: &candidate.result_type,
            unit: candidate.unit.as_deref(),
            rounding: candidate.rounding.as_deref(),
            calendar_ref: candidate.calendar_ref.as_ref(),
        };
        let mut qualification = qualify(graph, &draft, validate_bindings(graph, &scope.bindings));
        if let Some(reason) = name_issue {
            qualification.issues.push(CalculationIssue::InvalidName {
                reason: reason.to_owned(),
            });
            qualification.prepared = None;
        }
        let mut disposition = CalculationDisposition::Ineligible;
        if qualification.is_qualified() {
            let payload = NodePayload::Calculation(Calculation {
                name: name.clone(),
                expression: qualification
                    .canonical_expression
                    .clone()
                    .unwrap_or_default(),
                result_type: candidate.result_type.clone(),
                unit: candidate.unit.clone(),
                rounding: candidate.rounding.clone(),
                calendar_ref: candidate.calendar_ref.clone(),
                examples: None,
            });
            let origin = serde_json::to_value(CalculationOrigin {
                name_range: candidate.name_range.clone(),
                expression_evidence: evidence.clone(),
                used_bindings: qualification.used_bindings.clone(),
            })
            .map_err(invalid_proposal)?;
            match graph.node(&calculation_ref) {
                Some(existing) => {
                    let same_payload = existing.payload == payload;
                    let same_origin = existing
                        .extensions
                        .iter()
                        .find(|(k, _)| k.as_str() == CALCULATION_ORIGIN_EXTENSION)
                        .is_some_and(|(_, v)| *v == origin);
                    disposition = match existing.status {
                        ElementStatus::Proposed if same_payload && same_origin => {
                            CalculationDisposition::IdempotentReplay
                        }
                        ElementStatus::Accepted if same_payload => {
                            CalculationDisposition::ExistingEquivalent
                        }
                        _ => {
                            qualification.issues.push(
                                CalculationIssue::ExistingCalculationConflict {
                                    node_ref: calculation_ref.clone(),
                                },
                            );
                            CalculationDisposition::ExistingConflict
                        }
                    };
                }
                None => {
                    let mut groundings = vec![&candidate.name_range];
                    groundings.extend(evidence.iter());
                    let node_evidence: Vec<EvidenceRef> = groundings
                        .iter()
                        .filter_map(|g| graph.node(&g.requirement_ref))
                        .flat_map(|n| n.evidence.iter().cloned())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    let node = Node {
                        id: calculation_ref.clone(),
                        revision: 1,
                        status: ElementStatus::Proposed,
                        payload,
                        evidence: node_evidence.clone(),
                        derivations: Vec::new(),
                        standards: Vec::new(),
                        tags: BTreeSet::new(),
                        extensions: BTreeMap::from([(extension.clone(), origin)]),
                        audit: AuditMeta::new(
                            audit.created_by.clone(),
                            audit.created_at,
                            None,
                            None,
                        )
                        .map_err(invalid_proposal)?,
                    };
                    node.validate().map_err(invalid_proposal)?;
                    let proposal = Proposal::new(
                        CALCULATION_STAGE,
                        PatchSet {
                            base_semantic_hash: base.clone(),
                            patch: SemanticPatch::AddNode { node },
                        },
                        node_evidence,
                        vec![inference.derivation_ref.clone()],
                        ProposalMateriality::Semantic,
                        AcceptancePolicy::HumanConfirm,
                        None,
                    )
                    .map_err(invalid_proposal)?;
                    // In-memory dry validation; the candidate graph is discarded.
                    apply_patch(graph, &proposal.patch_set)
                        .map_err(|e| invalid_proposal(format!("proposal does not apply: {e}")))?;
                    disposition = CalculationDisposition::Proposed {
                        proposal_ref: proposal.id.clone(),
                    };
                    proposals.push(proposal);
                }
            }
        }
        candidates.push(CalculationCandidateAnalysis {
            calculation_ref,
            name,
            qualification,
            disposition,
        });
    }
    candidates.sort_by(|a, b| a.calculation_ref.cmp(&b.calculation_ref));
    proposals.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(CalculationAnalysisResult {
        candidates,
        proposals,
        issues: Vec::new(),
    })
}
