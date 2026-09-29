//! The typed core relation registry and relation validation (metamodel §17.8).
//!
//! Compatibility is declared explicitly per relation over `NodeType`s and named node
//! categories; it is never inferred from type names.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use thiserror::Error;

use crate::edge::{Edge, EdgeError};
use crate::payload::NodeType;
use crate::relations::{RelationKind, RelationProperties, SchemaBindingRole};

// ============================================================================ node categories

/// Named node-type categories used by relation predicates (metamodel §17.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeCategory {
    /// Every node type.
    Any,
    /// SourceArtifact, EvidenceFragment.
    Evidence,
    /// DerivationRecord, Agent.
    Provenance,
    /// Every node type except Evidence and Provenance types.
    SemanticNode,
    /// Every node type except Provenance types.
    SemanticOrEvidenceNode,
    /// The ten architecture element types.
    ArchitectureElement,
    /// The seven technical-contract types.
    TechnicalContract,
    /// Entity, Process and every architecture element type.
    StateOwner,
    /// SoftwareSystem, Container, Component, Module, DataStore.
    DeployableArchitectureElement,
}

const EVIDENCE: &[NodeType] = &[NodeType::SourceArtifact, NodeType::EvidenceFragment];
const PROVENANCE: &[NodeType] = &[NodeType::DerivationRecord, NodeType::Agent];
const ARCHITECTURE_ELEMENTS: &[NodeType] = &[
    NodeType::SoftwareSystem,
    NodeType::Container,
    NodeType::Component,
    NodeType::Module,
    NodeType::Interface,
    NodeType::DataStore,
    NodeType::ExternalSystem,
    NodeType::DeploymentNode,
    NodeType::RuntimeEnvironment,
    NodeType::NetworkZone,
];
const TECHNICAL_CONTRACTS: &[NodeType] = &[
    NodeType::ApiContract,
    NodeType::ApiOperation,
    NodeType::EventContract,
    NodeType::Channel,
    NodeType::Message,
    NodeType::DataSchema,
    NodeType::TechnicalWorkflow,
];
const DEPLOYABLE_ARCHITECTURE_ELEMENTS: &[NodeType] = &[
    NodeType::SoftwareSystem,
    NodeType::Container,
    NodeType::Component,
    NodeType::Module,
    NodeType::DataStore,
];

impl NodeCategory {
    /// Whether `node_type` belongs to this category.
    pub fn contains(self, node_type: NodeType) -> bool {
        match self {
            NodeCategory::Any => true,
            NodeCategory::Evidence => EVIDENCE.contains(&node_type),
            NodeCategory::Provenance => PROVENANCE.contains(&node_type),
            NodeCategory::SemanticNode => {
                !EVIDENCE.contains(&node_type) && !PROVENANCE.contains(&node_type)
            }
            NodeCategory::SemanticOrEvidenceNode => !PROVENANCE.contains(&node_type),
            NodeCategory::ArchitectureElement => ARCHITECTURE_ELEMENTS.contains(&node_type),
            NodeCategory::TechnicalContract => TECHNICAL_CONTRACTS.contains(&node_type),
            NodeCategory::StateOwner => {
                matches!(node_type, NodeType::Entity | NodeType::Process)
                    || ARCHITECTURE_ELEMENTS.contains(&node_type)
            }
            NodeCategory::DeployableArchitectureElement => {
                DEPLOYABLE_ARCHITECTURE_ELEMENTS.contains(&node_type)
            }
        }
    }

    /// Every member of this category, in `NodeType::ALL` order.
    pub fn members(self) -> Vec<NodeType> {
        NodeType::ALL
            .iter()
            .copied()
            .filter(|t| self.contains(*t))
            .collect()
    }
}

/// One alternative in a type predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeTerm {
    Type(NodeType),
    Category(NodeCategory),
}

/// A union of explicit node types and categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypePredicate(pub &'static [TypeTerm]);

impl TypePredicate {
    /// Whether `node_type` satisfies any term of the predicate.
    pub fn matches(&self, node_type: NodeType) -> bool {
        self.0.iter().any(|term| match term {
            TypeTerm::Type(t) => *t == node_type,
            TypeTerm::Category(c) => c.contains(node_type),
        })
    }
}

// ============================================================================ relation definitions

/// Structural cardinality: `max == None` means unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cardinality {
    pub min: u32,
    pub max: Option<u32>,
}

impl Cardinality {
    /// Whether `count` lies within the bounds.
    pub fn allows(&self, count: u32) -> bool {
        count >= self.min && self.max.is_none_or(|max| count <= max)
    }
}

/// Whether one edge represents an ordered or an unordered pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Directionality {
    Directed,
    Symmetric,
}

/// Whether edges of a relation may form cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CyclePolicy {
    Allowed,
    Acyclic,
}

/// Which typed properties a core relation carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationPropertySchema {
    None,
    SchemaFor,
}

/// The registry entry of one core relation.
#[derive(Debug, Clone, PartialEq)]
pub struct RelationDef {
    /// Always a core relation.
    pub kind: RelationKind,
    pub from: TypePredicate,
    pub to: TypePredicate,
    pub outgoing: Cardinality,
    pub incoming: Cardinality,
    pub directionality: Directionality,
    pub cycle_policy: CyclePolicy,
    /// Source and target must have the same `NodeType`.
    pub same_node_type: bool,
    pub property_schema: RelationPropertySchema,
}

const ANY_COUNT: Cardinality = Cardinality { min: 0, max: None };
const AT_MOST_ONE: Cardinality = Cardinality {
    min: 0,
    max: Some(1),
};
const EXACTLY_ONE: Cardinality = Cardinality {
    min: 1,
    max: Some(1),
};
const AT_LEAST_ONE: Cardinality = Cardinality { min: 1, max: None };

use NodeCategory as C;
use NodeType as N;
use TypeTerm::{Category as Cat, Type as Ty};

/// A directed, unconstrained, cycle-permitting relation without properties.
const fn rel(
    kind: RelationKind,
    from: &'static [TypeTerm],
    to: &'static [TypeTerm],
) -> RelationDef {
    RelationDef {
        kind,
        from: TypePredicate(from),
        to: TypePredicate(to),
        outgoing: ANY_COUNT,
        incoming: ANY_COUNT,
        directionality: Directionality::Directed,
        cycle_policy: CyclePolicy::Allowed,
        same_node_type: false,
        property_schema: RelationPropertySchema::None,
    }
}

const fn outgoing(mut def: RelationDef, cardinality: Cardinality) -> RelationDef {
    def.outgoing = cardinality;
    def
}

const fn acyclic(mut def: RelationDef) -> RelationDef {
    def.cycle_policy = CyclePolicy::Acyclic;
    def
}

const SEMANTIC: &[TypeTerm] = &[Cat(C::SemanticNode)];
const ARCH: &[TypeTerm] = &[Cat(C::ArchitectureElement)];
const OPERATION_OR_PROCESS_NODE: &[TypeTerm] = &[Ty(N::Operation), Ty(N::ProcessNode)];
const OPERATION_OR_COMPONENT: &[TypeTerm] = &[Ty(N::Operation), Ty(N::Component)];
const ATTRIBUTE_OR_ENTITY: &[TypeTerm] = &[Ty(N::Attribute), Ty(N::Entity)];
const REQUIREMENT: &[TypeTerm] = &[Ty(N::Requirement)];
const SECURITY_ROLE: &[TypeTerm] = &[Ty(N::SecurityRole)];
const PERMISSION: &[TypeTerm] = &[Ty(N::Permission)];
const QUALITY_SCENARIO: &[TypeTerm] = &[Ty(N::QualityScenario)];
const PROCESS_NODE: &[TypeTerm] = &[Ty(N::ProcessNode)];
const IMPLEMENTATION_SLICE: &[TypeTerm] = &[Ty(N::ImplementationSlice)];

/// The core relation registry: exactly one entry per core relation, in metamodel §17 order.
pub static RELATION_REGISTRY: [RelationDef; 51] = [
    // Evidence and governance
    rel(
        RelationKind::EvidencedBy,
        SEMANTIC,
        &[Ty(N::EvidenceFragment)],
    ),
    rel(
        RelationKind::DerivedFrom,
        SEMANTIC,
        &[Cat(C::SemanticOrEvidenceNode)],
    ),
    {
        let mut def = acyclic(outgoing(
            rel(RelationKind::Supersedes, &[Cat(C::Any)], &[Cat(C::Any)]),
            AT_MOST_ONE,
        ));
        def.same_node_type = true;
        def
    },
    {
        let mut def = rel(RelationKind::ConflictsWith, SEMANTIC, SEMANTIC);
        def.directionality = Directionality::Symmetric;
        def
    },
    outgoing(
        rel(
            RelationKind::Resolves,
            &[Ty(N::ResolutionDecision)],
            &[Ty(N::Question), Ty(N::Finding)],
        ),
        AT_LEAST_ONE,
    ),
    rel(RelationKind::Raises, &[Ty(N::Finding)], &[Ty(N::Question)]),
    // Intent and traceability
    rel(
        RelationKind::Addresses,
        &[Ty(N::Requirement), Cat(C::ArchitectureElement), Ty(N::View)],
        &[Ty(N::Concern), Ty(N::Goal)],
    ),
    acyclic(rel(RelationKind::Refines, REQUIREMENT, REQUIREMENT)),
    acyclic(rel(
        RelationKind::DecomposesTo,
        &[Ty(N::Requirement), Ty(N::Capability)],
        &[Ty(N::Requirement), Ty(N::Capability)],
    )),
    rel(
        RelationKind::SpecifiedBy,
        REQUIREMENT,
        &[
            Ty(N::Operation),
            Ty(N::Process),
            Ty(N::Rule),
            Ty(N::QualityScenario),
        ],
    ),
    rel(RelationKind::ConstrainedBy, SEMANTIC, &[Ty(N::Constraint)]),
    rel(
        RelationKind::SatisfiedBy,
        REQUIREMENT,
        &[Cat(C::ArchitectureElement), Cat(C::TechnicalContract)],
    ),
    // Domain and function
    {
        let mut def = rel(
            RelationKind::HasAttribute,
            &[Ty(N::Entity)],
            &[Ty(N::Attribute)],
        );
        def.incoming = EXACTLY_ONE;
        def
    },
    rel(
        RelationKind::HasState,
        &[Cat(C::StateOwner)],
        &[Ty(N::State)],
    ),
    outgoing(
        rel(
            RelationKind::TransitionsVia,
            &[Ty(N::Transition)],
            &[Ty(N::Operation), Ty(N::Event)],
        ),
        EXACTLY_ONE,
    ),
    rel(
        RelationKind::PerformedBy,
        OPERATION_OR_PROCESS_NODE,
        &[Ty(N::Actor), Ty(N::BusinessRole)],
    ),
    rel(
        RelationKind::Reads,
        &[Ty(N::Operation)],
        ATTRIBUTE_OR_ENTITY,
    ),
    rel(
        RelationKind::Writes,
        &[Ty(N::Operation)],
        ATTRIBUTE_OR_ENTITY,
    ),
    rel(
        RelationKind::Produces,
        OPERATION_OR_PROCESS_NODE,
        &[Ty(N::Outcome), Ty(N::Event)],
    ),
    rel(
        RelationKind::Consumes,
        OPERATION_OR_PROCESS_NODE,
        &[Ty(N::Event)],
    ),
    rel(
        RelationKind::GovernedBy,
        OPERATION_OR_PROCESS_NODE,
        &[Ty(N::Rule), Ty(N::DecisionTable)],
    ),
    rel(
        RelationKind::UsesCalculation,
        &[Ty(N::Operation), Ty(N::Rule)],
        &[Ty(N::Calculation)],
    ),
    rel(RelationKind::Next, PROCESS_NODE, PROCESS_NODE),
    // Authorization
    rel(
        RelationKind::AssignedRole,
        &[Ty(N::Principal), Ty(N::Actor)],
        SECURITY_ROLE,
    ),
    acyclic(rel(
        RelationKind::InheritsRole,
        SECURITY_ROLE,
        SECURITY_ROLE,
    )),
    rel(RelationKind::Grants, SECURITY_ROLE, PERMISSION),
    outgoing(
        rel(RelationKind::Permits, PERMISSION, &[Ty(N::Operation)]),
        EXACTLY_ONE,
    ),
    outgoing(
        rel(RelationKind::ScopedTo, PERMISSION, &[Ty(N::ResourceScope)]),
        EXACTLY_ONE,
    ),
    rel(
        RelationKind::ConditionedBy,
        PERMISSION,
        &[Ty(N::PolicyCondition)],
    ),
    // Quality and architecture
    outgoing(
        rel(
            RelationKind::CharacterizedBy,
            QUALITY_SCENARIO,
            &[Ty(N::QualityCharacteristic)],
        ),
        EXACTLY_ONE,
    ),
    outgoing(
        rel(
            RelationKind::MeasuredBy,
            QUALITY_SCENARIO,
            &[Ty(N::Measure)],
        ),
        EXACTLY_ONE,
    ),
    rel(
        RelationKind::Drives,
        &[Ty(N::QualityScenario), Ty(N::Constraint)],
        &[Ty(N::ArchitectureDecision), Ty(N::ArchitectureCandidate)],
    ),
    rel(
        RelationKind::AllocatedTo,
        &[Ty(N::Operation), Ty(N::Process), Ty(N::Entity)],
        ARCH,
    ),
    rel(RelationKind::DependsOn, ARCH, ARCH),
    rel(
        RelationKind::Exposes,
        ARCH,
        &[Ty(N::Interface), Ty(N::ApiOperation), Ty(N::Channel)],
    ),
    rel(
        RelationKind::StoresIn,
        &[Ty(N::Component), Ty(N::Container)],
        &[Ty(N::DataStore)],
    ),
    rel(
        RelationKind::DeployedTo,
        &[Cat(C::DeployableArchitectureElement)],
        &[Ty(N::DeploymentNode), Ty(N::RuntimeEnvironment)],
    ),
    rel(
        RelationKind::UsesTechnology,
        ARCH,
        &[Ty(N::TechnologySelection)],
    ),
    rel(
        RelationKind::JustifiedBy,
        &[Ty(N::ArchitectureDecision), Ty(N::TechnologySelection)],
        &[
            Ty(N::Requirement),
            Ty(N::QualityScenario),
            Ty(N::Constraint),
        ],
    ),
    // Contracts
    rel(
        RelationKind::ExposedBy,
        &[Ty(N::Operation)],
        &[Ty(N::ApiOperation)],
    ),
    rel(
        RelationKind::Publishes,
        OPERATION_OR_COMPONENT,
        &[Ty(N::Message), Ty(N::Event)],
    ),
    rel(
        RelationKind::SubscribesTo,
        OPERATION_OR_COMPONENT,
        &[Ty(N::Message), Ty(N::Channel)],
    ),
    {
        let mut def = rel(
            RelationKind::SchemaFor,
            &[Ty(N::DataSchema)],
            &[Ty(N::Attribute), Ty(N::Message), Ty(N::ApiOperation)],
        );
        def.property_schema = RelationPropertySchema::SchemaFor;
        def
    },
    outgoing(
        rel(
            RelationKind::WorkflowStep,
            &[Ty(N::TechnicalWorkflow)],
            &[Ty(N::ApiOperation)],
        ),
        AT_LEAST_ONE,
    ),
    // Delivery and proof
    rel(
        RelationKind::ImplementedBy,
        &[
            Ty(N::Requirement),
            Ty(N::Operation),
            Cat(C::ArchitectureElement),
        ],
        IMPLEMENTATION_SLICE,
    ),
    rel(
        RelationKind::Contains,
        &[Ty(N::WorkPackage), Ty(N::Release)],
        IMPLEMENTATION_SLICE,
    ),
    acyclic(rel(
        RelationKind::DependsOnSlice,
        IMPLEMENTATION_SLICE,
        IMPLEMENTATION_SLICE,
    )),
    rel(
        RelationKind::VerifiedBy,
        &[
            Ty(N::Requirement),
            Ty(N::Operation),
            Cat(C::ArchitectureElement),
            Cat(C::TechnicalContract),
        ],
        &[Ty(N::VerificationObligation)],
    ),
    rel(
        RelationKind::ImplementedAs,
        &[Ty(N::VerificationObligation)],
        &[Ty(N::TestCase), Ty(N::Scenario), Ty(N::ArchitectureCheck)],
    ),
    outgoing(
        rel(
            RelationKind::ProducesReceipt,
            &[Ty(N::TestExecution), Ty(N::ScenarioRun)],
            &[Ty(N::TestReceipt)],
        ),
        AT_MOST_ONE,
    ),
    rel(RelationKind::BoundToCode, SEMANTIC, &[Ty(N::CodeBinding)]),
];

/// The registry entry of a core relation; `None` for extension relations.
pub fn relation_def(kind: &RelationKind) -> Option<&'static RelationDef> {
    RELATION_REGISTRY.iter().find(|def| &def.kind == kind)
}

/// The roles `schema_for` allows for a target node type.
pub fn allowed_schema_roles(target: NodeType) -> &'static [SchemaBindingRole] {
    match target {
        NodeType::Attribute => &[SchemaBindingRole::Attribute],
        NodeType::Message => &[
            SchemaBindingRole::MessagePayload,
            SchemaBindingRole::MessageHeaders,
        ],
        NodeType::ApiOperation => &[
            SchemaBindingRole::ApiRequest,
            SchemaBindingRole::ApiResponse,
            SchemaBindingRole::ApiError,
        ],
        _ => &[],
    }
}

// ============================================================================ validation

/// A registry violation found by [`validate_relations`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RelationViolation {
    /// The edge fails its local invariants.
    #[error("edge {edge}: {error}")]
    InvalidEdge { edge: Id, error: EdgeError },
    /// An endpoint ID is not among the supplied nodes.
    #[error("edge {edge} references unknown node {node}")]
    MissingEndpoint { edge: Id, node: Id },
    /// The source node type is not allowed for the relation.
    #[error("edge {edge}: {kind} does not allow source type {node_type:?}")]
    SourceTypeNotAllowed {
        edge: Id,
        kind: RelationKind,
        node_type: NodeType,
    },
    /// The target node type is not allowed for the relation.
    #[error("edge {edge}: {kind} does not allow target type {node_type:?}")]
    TargetTypeNotAllowed {
        edge: Id,
        kind: RelationKind,
        node_type: NodeType,
    },
    /// The relation requires identical source and target node types.
    #[error("edge {edge}: {kind} requires the same node type, got {from_type:?} -> {to_type:?}")]
    NodeTypeMismatch {
        edge: Id,
        kind: RelationKind,
        from_type: NodeType,
        to_type: NodeType,
    },
    /// The `schema_for` role does not fit the target node type.
    #[error("edge {edge}: schema_for role {role:?} is not allowed for target type {target:?}")]
    SchemaRoleNotAllowed {
        edge: Id,
        role: SchemaBindingRole,
        target: NodeType,
    },
    /// A node has too few or too many outgoing edges of a relation.
    #[error("node {node}: {count} outgoing {kind} edges outside {min}..{max:?}")]
    OutgoingCardinality {
        node: Id,
        kind: RelationKind,
        count: u32,
        min: u32,
        max: Option<u32>,
    },
    /// A node has too few or too many incoming edges of a relation.
    #[error("node {node}: {count} incoming {kind} edges outside {min}..{max:?}")]
    IncomingCardinality {
        node: Id,
        kind: RelationKind,
        count: u32,
        min: u32,
        max: Option<u32>,
    },
    /// Edges of an acyclic relation form a cycle (listed in traversal order).
    #[error("{kind} edges form a cycle through {nodes:?}")]
    Cycle { kind: RelationKind, nodes: Vec<Id> },
}

/// Validates `edges` against the registry for the given node types: endpoint existence and
/// types, `schema_for` roles, unconditional structural cardinality and acyclicity.
///
/// Violations are returned in a deterministic order (edge ID, then registry order).
/// Extension relations are checked only for their local invariants and endpoint existence.
pub fn validate_relations<'a>(
    node_types: &BTreeMap<Id, NodeType>,
    edges: impl IntoIterator<Item = &'a Edge>,
) -> Result<(), Vec<RelationViolation>> {
    let mut edges: Vec<&Edge> = edges.into_iter().collect();
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    let mut violations = Vec::new();

    for edge in &edges {
        check_edge(node_types, edge, &mut violations);
    }
    for def in RELATION_REGISTRY.iter() {
        check_cardinality(node_types, &edges, def, &mut violations);
        if def.cycle_policy == CyclePolicy::Acyclic {
            check_acyclic(&edges, &def.kind, &mut violations);
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

fn check_edge(
    node_types: &BTreeMap<Id, NodeType>,
    edge: &Edge,
    violations: &mut Vec<RelationViolation>,
) {
    if let Err(error) = edge.validate() {
        violations.push(RelationViolation::InvalidEdge {
            edge: edge.id.clone(),
            error,
        });
    }
    let endpoint = |node: &Id| node_types.get(node).copied();
    let (from_type, to_type) = (endpoint(&edge.from), endpoint(&edge.to));
    for (node, node_type) in [(&edge.from, from_type), (&edge.to, to_type)] {
        if node_type.is_none() {
            violations.push(RelationViolation::MissingEndpoint {
                edge: edge.id.clone(),
                node: node.clone(),
            });
        }
    }
    let (Some(def), Some(from_type), Some(to_type)) =
        (relation_def(&edge.kind), from_type, to_type)
    else {
        return;
    };
    if !def.from.matches(from_type) {
        violations.push(RelationViolation::SourceTypeNotAllowed {
            edge: edge.id.clone(),
            kind: edge.kind.clone(),
            node_type: from_type,
        });
    }
    if !def.to.matches(to_type) {
        violations.push(RelationViolation::TargetTypeNotAllowed {
            edge: edge.id.clone(),
            kind: edge.kind.clone(),
            node_type: to_type,
        });
    }
    if def.same_node_type && from_type != to_type {
        violations.push(RelationViolation::NodeTypeMismatch {
            edge: edge.id.clone(),
            kind: edge.kind.clone(),
            from_type,
            to_type,
        });
    }
    if let RelationProperties::SchemaFor(properties) = &edge.properties {
        if def.to.matches(to_type) && !allowed_schema_roles(to_type).contains(&properties.role) {
            violations.push(RelationViolation::SchemaRoleNotAllowed {
                edge: edge.id.clone(),
                role: properties.role,
                target: to_type,
            });
        }
    }
}

fn check_cardinality(
    node_types: &BTreeMap<Id, NodeType>,
    edges: &[&Edge],
    def: &RelationDef,
    violations: &mut Vec<RelationViolation>,
) {
    if def.outgoing == ANY_COUNT && def.incoming == ANY_COUNT {
        return;
    }
    let mut out_counts: BTreeMap<&Id, u32> = BTreeMap::new();
    let mut in_counts: BTreeMap<&Id, u32> = BTreeMap::new();
    for edge in edges.iter().filter(|e| e.kind == def.kind) {
        *out_counts.entry(&edge.from).or_default() += 1;
        *in_counts.entry(&edge.to).or_default() += 1;
    }
    for (node, node_type) in node_types {
        if def.from.matches(*node_type) {
            let count = out_counts.get(node).copied().unwrap_or(0);
            if !def.outgoing.allows(count) {
                violations.push(RelationViolation::OutgoingCardinality {
                    node: node.clone(),
                    kind: def.kind.clone(),
                    count,
                    min: def.outgoing.min,
                    max: def.outgoing.max,
                });
            }
        }
        if def.to.matches(*node_type) {
            let count = in_counts.get(node).copied().unwrap_or(0);
            if !def.incoming.allows(count) {
                violations.push(RelationViolation::IncomingCardinality {
                    node: node.clone(),
                    kind: def.kind.clone(),
                    count,
                    min: def.incoming.min,
                    max: def.incoming.max,
                });
            }
        }
    }
}

fn check_acyclic(edges: &[&Edge], kind: &RelationKind, violations: &mut Vec<RelationViolation>) {
    let mut adjacency: BTreeMap<&Id, BTreeSet<&Id>> = BTreeMap::new();
    for edge in edges.iter().filter(|e| &e.kind == kind) {
        adjacency.entry(&edge.from).or_default().insert(&edge.to);
    }
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Visiting,
        Done,
    }
    let mut marks: BTreeMap<&Id, Mark> = BTreeMap::new();
    let starts: Vec<&Id> = adjacency.keys().copied().collect();
    for start in starts {
        if marks.contains_key(start) {
            continue;
        }
        // Iterative DFS keeping the current path so a found cycle can be reported.
        let mut path: Vec<&Id> = vec![start];
        let mut stack: Vec<Vec<&Id>> = vec![successors(&adjacency, start)];
        marks.insert(start, Mark::Visiting);
        while let Some(frontier) = stack.last_mut() {
            match frontier.pop() {
                Some(next) => match marks.get(next) {
                    Some(Mark::Visiting) => {
                        let at = path.iter().position(|n| *n == next).unwrap_or(0);
                        violations.push(RelationViolation::Cycle {
                            kind: kind.clone(),
                            nodes: path[at..].iter().map(|n| (*n).clone()).collect(),
                        });
                        return;
                    }
                    Some(Mark::Done) => {}
                    None => {
                        marks.insert(next, Mark::Visiting);
                        path.push(next);
                        stack.push(successors(&adjacency, next));
                    }
                },
                None => {
                    stack.pop();
                    if let Some(done) = path.pop() {
                        marks.insert(done, Mark::Done);
                    }
                }
            }
        }
    }
}

/// Successors in reverse order so that popping visits them in ascending ID order.
fn successors<'a>(adjacency: &BTreeMap<&'a Id, BTreeSet<&'a Id>>, node: &Id) -> Vec<&'a Id> {
    adjacency
        .get(node)
        .map(|next| next.iter().rev().copied().collect())
        .unwrap_or_default()
}
