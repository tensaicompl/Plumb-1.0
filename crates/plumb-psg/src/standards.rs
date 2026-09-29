//! Standards mapping vocabulary and `StandardMapping` (metamodel §§2, 16.2).

use serde::{Deserialize, Serialize};

/// What an external standard is used for (metamodel §2). Closed vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingRole {
    SemanticAlignment,
    Taxonomy,
    Interchange,
    ValidationReference,
    PresentationConvention,
}

/// How strongly Plumb claims correspondence with an external concept (metamodel §2).
/// Closed vocabulary; `taxonomy` is a role, not a strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingStrength {
    Exact,
    Compatible,
    Subset,
    Extension,
    InspiredBy,
}

/// A mapping from a PSG element to an external standard concept (metamodel §16.2).
///
/// Only structure and enum vocabulary are validated here; whether the standard, clause,
/// concept or validator rule exists is not checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardMapping {
    pub standard_id: String,
    pub version: String,
    pub concept: String,
    pub clause_ref: Option<String>,
    pub mapping_role: MappingRole,
    pub mapping_strength: MappingStrength,
    pub validator_rules: Vec<String>,
}
