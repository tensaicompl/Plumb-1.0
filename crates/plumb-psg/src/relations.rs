//! Relation kinds and typed relation properties (metamodel §§17, 17.8).

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::extensions::{ExtensionKey, InvalidExtensionKey};

/// Why a string is not a [`RelationKind`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RelationKindError {
    /// An unqualified name that is not one of the 51 core relations.
    #[error("unknown core relation {0:?}")]
    UnknownCoreRelation(String),
    /// A namespaced name that is not a valid `ExtensionKey`.
    #[error("invalid extension relation: {0}")]
    InvalidExtension(#[from] InvalidExtensionKey),
}

/// Declares the core `RelationKind` variants and their exact wire names from one list.
macro_rules! relation_kinds {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// A PSG relation kind: one of the 51 closed core relations of metamodel §17, or a
        /// namespaced extension relation. Serialized as a single JSON string.
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum RelationKind {
            $($variant,)+
            /// A namespaced extension relation such as `acme:depends_on`.
            Extension(ExtensionKey),
        }

        impl RelationKind {
            /// Every core relation, in metamodel §17 order.
            pub const CORE: &'static [RelationKind] = &[$(RelationKind::$variant),+];

            /// The exact wire string: the snake_case core name or the extension key.
            pub fn as_str(&self) -> &str {
                match self {
                    $(RelationKind::$variant => $name,)+
                    RelationKind::Extension(key) => key.as_str(),
                }
            }

            fn core_from_name(name: &str) -> Option<RelationKind> {
                match name {
                    $($name => Some(RelationKind::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

relation_kinds! {
    // Evidence and governance
    EvidencedBy => "evidenced_by",
    DerivedFrom => "derived_from",
    Supersedes => "supersedes",
    ConflictsWith => "conflicts_with",
    Resolves => "resolves",
    Raises => "raises",
    // Intent and traceability
    Addresses => "addresses",
    Refines => "refines",
    DecomposesTo => "decomposes_to",
    SpecifiedBy => "specified_by",
    ConstrainedBy => "constrained_by",
    SatisfiedBy => "satisfied_by",
    // Domain and function
    HasAttribute => "has_attribute",
    HasState => "has_state",
    TransitionsVia => "transitions_via",
    PerformedBy => "performed_by",
    Reads => "reads",
    Writes => "writes",
    Produces => "produces",
    Consumes => "consumes",
    GovernedBy => "governed_by",
    UsesCalculation => "uses_calculation",
    Next => "next",
    // Authorization
    AssignedRole => "assigned_role",
    InheritsRole => "inherits_role",
    Grants => "grants",
    Permits => "permits",
    ScopedTo => "scoped_to",
    ConditionedBy => "conditioned_by",
    // Quality and architecture
    CharacterizedBy => "characterized_by",
    MeasuredBy => "measured_by",
    Drives => "drives",
    AllocatedTo => "allocated_to",
    DependsOn => "depends_on",
    Exposes => "exposes",
    StoresIn => "stores_in",
    DeployedTo => "deployed_to",
    UsesTechnology => "uses_technology",
    JustifiedBy => "justified_by",
    // Contracts
    ExposedBy => "exposed_by",
    Publishes => "publishes",
    SubscribesTo => "subscribes_to",
    SchemaFor => "schema_for",
    WorkflowStep => "workflow_step",
    // Delivery and proof
    ImplementedBy => "implemented_by",
    Contains => "contains",
    DependsOnSlice => "depends_on_slice",
    VerifiedBy => "verified_by",
    ImplementedAs => "implemented_as",
    ProducesReceipt => "produces_receipt",
    BoundToCode => "bound_to_code",
}

impl RelationKind {
    /// Whether this is one of the 51 core relations (not an extension).
    pub fn is_core(&self) -> bool {
        !matches!(self, RelationKind::Extension(_))
    }
}

impl FromStr for RelationKind {
    type Err = RelationKindError;

    /// A known core name gives the core variant; a string containing `:` must be a valid
    /// `ExtensionKey`; any other unqualified name is rejected.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(core) = RelationKind::core_from_name(s) {
            return Ok(core);
        }
        if s.contains(':') {
            return Ok(RelationKind::Extension(s.parse()?));
        }
        Err(RelationKindError::UnknownCoreRelation(s.to_owned()))
    }
}

impl fmt::Display for RelationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for RelationKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RelationKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// What a `schema_for` DataSchema describes about its target (metamodel §17.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaBindingRole {
    Attribute,
    MessagePayload,
    MessageHeaders,
    ApiRequest,
    ApiResponse,
    ApiError,
}

/// Properties of a core `schema_for` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaForProperties {
    pub role: SchemaBindingRole,
}

/// Typed edge properties. Serialized as a JSON object: `{}` for `None`, `{"role": ...}` for
/// `SchemaFor`, and the namespaced keys for `Extension`.
#[derive(Debug, Clone, PartialEq)]
pub enum RelationProperties {
    /// No properties; required for every core relation except `schema_for`.
    None,
    /// Properties of a core `schema_for` relation.
    SchemaFor(SchemaForProperties),
    /// Namespaced properties of an extension relation (possibly empty).
    Extension(BTreeMap<ExtensionKey, Value>),
}

/// Why properties are invalid for a relation kind.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RelationPropertiesError {
    /// A core relation other than `schema_for` carries properties.
    #[error("core relation {kind} must have no properties")]
    UnexpectedProperties { kind: RelationKind },
    /// `schema_for` properties are missing or malformed.
    #[error("invalid schema_for properties: {0}")]
    InvalidSchemaFor(String),
    /// An extension relation has a property key that is not an `ExtensionKey`.
    #[error("extension relation {kind} has invalid property key {key:?}")]
    InvalidExtensionPropertyKey { kind: RelationKind, key: String },
    /// The properties variant does not match the relation kind.
    #[error("relation {kind} does not accept {found} properties")]
    KindMismatch {
        kind: RelationKind,
        found: &'static str,
    },
}

impl RelationProperties {
    fn variant_name(&self) -> &'static str {
        match self {
            RelationProperties::None => "none",
            RelationProperties::SchemaFor(_) => "schema_for",
            RelationProperties::Extension(_) => "extension",
        }
    }

    /// Checks that this properties variant is the one required by `kind`.
    pub fn check_compatible(&self, kind: &RelationKind) -> Result<(), RelationPropertiesError> {
        let ok = match (kind, self) {
            (RelationKind::Extension(_), RelationProperties::Extension(_)) => true,
            (RelationKind::SchemaFor, RelationProperties::SchemaFor(_)) => true,
            (RelationKind::Extension(_) | RelationKind::SchemaFor, _) => false,
            (_, RelationProperties::None) => true,
            (_, _) => false,
        };
        if ok {
            Ok(())
        } else {
            Err(RelationPropertiesError::KindMismatch {
                kind: kind.clone(),
                found: self.variant_name(),
            })
        }
    }

    /// Interprets a JSON properties object for `kind`.
    pub fn from_json(
        kind: &RelationKind,
        object: Map<String, Value>,
    ) -> Result<Self, RelationPropertiesError> {
        match kind {
            RelationKind::SchemaFor => serde_json::from_value(Value::Object(object))
                .map(RelationProperties::SchemaFor)
                .map_err(|e| RelationPropertiesError::InvalidSchemaFor(e.to_string())),
            RelationKind::Extension(_) => object
                .into_iter()
                .map(|(key, value)| match key.parse::<ExtensionKey>() {
                    Ok(key) => Ok((key, value)),
                    Err(_) => Err(RelationPropertiesError::InvalidExtensionPropertyKey {
                        kind: kind.clone(),
                        key,
                    }),
                })
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map(RelationProperties::Extension),
            _ if object.is_empty() => Ok(RelationProperties::None),
            _ => Err(RelationPropertiesError::UnexpectedProperties { kind: kind.clone() }),
        }
    }
}

impl Serialize for RelationProperties {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            RelationProperties::None => serializer.serialize_map(Some(0))?.end(),
            RelationProperties::SchemaFor(properties) => properties.serialize(serializer),
            RelationProperties::Extension(map) => map.serialize(serializer),
        }
    }
}
