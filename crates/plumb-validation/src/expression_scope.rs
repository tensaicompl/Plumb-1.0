//! The generic deterministic PSG-node -> explicit PlumbExpr symbol binding -> TypeEnv adapter
//! (plan S2.6, relocated by Hotfix 044).
//!
//! A binding names an Accepted Entity (Root: `Ref(entity id)`), Attribute (Root: its pilot
//! mapped type, enums identified by the Attribute ID with their exact members, numeric values
//! with a unit as `Quantity(unit)`; Field: a field of its canonical Accepted owning Entity) or
//! Calculation (Root: its declared result type) under an exact S2.4 Symbol. PSG names are never
//! normalized, searched or rewritten into symbols. Calculations (S2.6) and Invariants (F2)
//! share this one adapter.

use std::collections::BTreeMap;

use plumb_core::Id;
use plumb_expr::{map_pilot_value_type, Symbol, Ty, TypeBindingError, TypeEnv};
use plumb_psg::{ElementStatus, Graph, Node, NodePayload, RelationKind};
use serde::{Deserialize, Serialize};

/// Why a set of bindings is not a valid expression scope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpressionScopeIssue {
    InvalidScope {
        reason: String,
    },
    ReservedSymbol {
        node_ref: Id,
    },
    ScopeBindingConflict {
        symbol: String,
        owner_ref: Option<Id>,
    },
    UnknownInputRef {
        node_ref: Id,
    },
    UnsupportedInputType {
        node_ref: Id,
        reason: String,
    },
}

/// Why a declared result type cannot be mapped.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResultTypeIssue {
    UnsupportedResultType { result_type: String },
    UnitMismatch { expected: String, actual: String },
}

/// How a binding exposes its node to a PlumbExpr expression.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpressionBindingExposure {
    /// A top-level root symbol.
    Root,
    /// A field of one Entity's Ref type.
    Field { owner_ref: Id },
}

/// One explicit PSG-to-PlumbExpr binding. The symbol is validated through the exact S2.4
/// Symbol constructor; it is never derived by rewriting a PSG name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionScopeBinding {
    pub node_ref: Id,
    pub symbol: String,
    pub exposure: ExpressionBindingExposure,
}

impl ExpressionScopeBinding {
    /// The canonical order: Root before Field, then owner, symbol and node.
    fn order_key(&self) -> (u8, Option<&Id>, &str, &Id) {
        match &self.exposure {
            ExpressionBindingExposure::Root => (0, None, &self.symbol, &self.node_ref),
            ExpressionBindingExposure::Field { owner_ref } => {
                (1, Some(owner_ref), &self.symbol, &self.node_ref)
            }
        }
    }
}

impl PartialOrd for ExpressionScopeBinding {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ExpressionScopeBinding {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.order_key().cmp(&other.order_key())
    }
}

fn is_accepted(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
}

/// The canonical Accepted Entity owning an Attribute through an Accepted `has_attribute`.
pub fn canonical_attribute_owner<'g>(graph: &'g Graph, attribute: &Id) -> Option<&'g Id> {
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
pub fn attribute_expression_type(
    node: &Node,
) -> Result<(Ty, Option<Vec<Symbol>>), ExpressionScopeIssue> {
    let NodePayload::Attribute(a) = &node.payload else {
        return Err(ExpressionScopeIssue::InvalidScope {
            reason: format!("{} is not an Attribute", node.id),
        });
    };
    let unsupported = |reason: String| ExpressionScopeIssue::UnsupportedInputType {
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
pub fn declared_result_type(result_type: &str, unit: Option<&str>) -> Result<Ty, ResultTypeIssue> {
    if result_type == "Enum" {
        return Err(ResultTypeIssue::UnsupportedResultType {
            result_type: result_type.to_owned(),
        });
    }
    match map_pilot_value_type(result_type, unit, None) {
        Ok(ty) => Ok(ty),
        Err(TypeBindingError::UnitOnNonNumericType(_)) | Err(TypeBindingError::InvalidUnit(_)) => {
            Err(ResultTypeIssue::UnitMismatch {
                expected: result_type.to_owned(),
                actual: unit.unwrap_or_default().to_owned(),
            })
        }
        Err(_) => Err(ResultTypeIssue::UnsupportedResultType {
            result_type: result_type.to_owned(),
        }),
    }
}

/// A validated scope: canonical bindings, the root and field lookups and the base TypeEnv.
#[derive(Debug, Clone)]
pub struct ValidatedExpressionScope {
    pub bindings: Vec<ExpressionScopeBinding>,
    pub roots: BTreeMap<String, ExpressionScopeBinding>,
    pub fields: BTreeMap<(Id, String), ExpressionScopeBinding>,
    pub env: TypeEnv,
}

fn conflict(binding: &ExpressionScopeBinding) -> ExpressionScopeIssue {
    ExpressionScopeIssue::ScopeBindingConflict {
        symbol: binding.symbol.clone(),
        owner_ref: match &binding.exposure {
            ExpressionBindingExposure::Root => None,
            ExpressionBindingExposure::Field { owner_ref } => Some(owner_ref.clone()),
        },
    }
}

/// Validates explicit bindings against the graph and builds their TypeEnv. Root bindings named
/// by one of `reserved_roots` are rejected as reserved.
pub fn validate_expression_bindings(
    graph: &Graph,
    bindings: &[ExpressionScopeBinding],
    reserved_roots: &[&str],
) -> Result<ValidatedExpressionScope, ExpressionScopeIssue> {
    let mut sorted = bindings.to_vec();
    sorted.sort();
    let mut scope = ValidatedExpressionScope {
        bindings: Vec::new(),
        roots: BTreeMap::new(),
        fields: BTreeMap::new(),
        env: TypeEnv::new(),
    };
    let invalid = |reason: String| ExpressionScopeIssue::InvalidScope { reason };
    for binding in sorted {
        if scope.bindings.contains(&binding) {
            return Err(conflict(&binding));
        }
        let symbol = Symbol::new(&binding.symbol)
            .map_err(|_| invalid(format!("{:?} is not a PlumbExpr symbol", binding.symbol)))?;
        let node =
            graph
                .node(&binding.node_ref)
                .ok_or_else(|| ExpressionScopeIssue::UnknownInputRef {
                    node_ref: binding.node_ref.clone(),
                })?;
        if !is_accepted(node) {
            return Err(invalid(format!("{} is not Accepted", node.id)));
        }
        match &binding.exposure {
            ExpressionBindingExposure::Root => {
                if reserved_roots.contains(&binding.symbol.as_str()) {
                    return Err(ExpressionScopeIssue::ReservedSymbol {
                        node_ref: binding.node_ref.clone(),
                    });
                }
                if scope.roots.contains_key(&binding.symbol) {
                    return Err(conflict(&binding));
                }
                let ty = match &node.payload {
                    NodePayload::Entity(_) => Ty::Ref(node.id.clone()),
                    NodePayload::Attribute(_) => {
                        let (ty, members) = attribute_expression_type(node)?;
                        if let Some(members) = members {
                            scope
                                .env
                                .define_enum(node.id.clone(), members)
                                .map_err(|e| invalid(e.to_string()))?;
                        }
                        ty
                    }
                    NodePayload::Calculation(c) => {
                        declared_result_type(&c.result_type, c.unit.as_deref()).map_err(|e| {
                            ExpressionScopeIssue::UnsupportedInputType {
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
            ExpressionBindingExposure::Field { owner_ref } => {
                if !matches!(node.payload, NodePayload::Attribute(_)) {
                    return Err(invalid(format!(
                        "{} cannot be a Field binding",
                        node.payload.node_type().as_str()
                    )));
                }
                if canonical_attribute_owner(graph, &node.id) != Some(owner_ref) {
                    return Err(invalid(format!(
                        "{owner_ref} is not the canonical owner of {}",
                        node.id
                    )));
                }
                let key = (owner_ref.clone(), binding.symbol.clone());
                if scope.fields.contains_key(&key) {
                    return Err(conflict(&binding));
                }
                let (ty, members) = attribute_expression_type(node)?;
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
