//! Grounded Invariant proposals (plan S2.2; compiler architecture §8).
//!
//! A supplied lifecycle inference may propose a PlumbExpr expression string for a business rule
//! grounded in an Accepted Requirement and scoped to an active state owner. S2.2 checks only
//! that the string is clean text and proposes a Proposed Invariant for human review; the
//! expression is not parsed, typechecked or evaluated here. Qualification belongs to the
//! PlumbExpr grammar (S2.4), typechecker (S2.5) and `PLUMB.F2.INVARIANT.EXPRESSIBLE`.

use plumb_core::{CoreError, Id};
use plumb_psg::{Invariant, NodePayload};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::state::{short_id, LifecycleError, LifecycleGrounding};

/// Extension key of the provenance-only [`InvariantOrigin`].
pub const INVARIANT_ORIGIN_EXTENSION: &str = "plumb_functional:invariant_origin";

/// Provenance only (`plumb_functional:invariant_origin`): the scope and the grounding the
/// proposed expression was derived from. It carries no qualification claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvariantOrigin {
    pub scope_ref: Id,
    pub grounding: LifecycleGrounding,
}

/// `invariant:<16 hex of SHA-256(RFC 8785 {project_id, scope_ref, grounding, node_type:
/// invariant})>`; the expression text is not part of the identity.
pub(crate) fn invariant_id(
    project_id: &Id,
    scope_ref: &Id,
    grounding: &LifecycleGrounding,
) -> Result<Id, CoreError> {
    short_id(
        "invariant",
        &json!({
            "project_id": project_id,
            "scope_ref": scope_ref,
            "grounding": {
                "requirement_ref": grounding.requirement_ref,
                "start": grounding.start,
                "end": grounding.end,
            },
            "node_type": "invariant",
        }),
    )
}

/// The only S2.2 expression check: non-empty, unpadded text without control characters.
/// Syntax and types are deliberately not examined.
pub(crate) fn validate_expression(expression: &str) -> Result<(), LifecycleError> {
    if expression.is_empty()
        || expression.trim() != expression
        || expression.chars().any(char::is_control)
    {
        return Err(LifecycleError::InvalidExpression {
            expression: expression.to_owned(),
        });
    }
    Ok(())
}

/// The payload of a proposed Invariant: exactly the scope and the supplied expression.
pub(crate) fn invariant_payload(scope_ref: &Id, expression: &str) -> NodePayload {
    NodePayload::Invariant(Invariant {
        scope_ref: scope_ref.clone(),
        expression: expression.to_owned(),
    })
}
