//! Deterministic change impact of an accepted patch: changed, dirty, affected projections and
//! affected gate namespaces (compiler architecture §§5.6, 22).
//!
//! The input is the base graph, the result graph and the exact `GraphDelta` produced by patch
//! application (after a commit, `CommitResult.delta`). The patch is never reapplied and this
//! module has no revision-store dependency.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::str::FromStr;

use plumb_core::{CoreError, GateId, Hash, Id};
use plumb_psg::{
    edge_element_hash, is_baseline, node_element_hash, Edge, Graph, Node, NodeType, RelationKind,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::diff::GraphDelta;

/// Why an impact could not be computed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImpactError {
    #[error(transparent)]
    Core(#[from] CoreError),
    /// `delta.base_semantic_hash` is not the base graph's semantic hash.
    #[error("delta base semantic hash {delta} differs from base graph hash {graph}")]
    BaseHashMismatch { delta: Hash, graph: Hash },
    /// `delta.result_semantic_hash` is not the result graph's semantic hash.
    #[error("delta result semantic hash {delta} differs from result graph hash {graph}")]
    ResultHashMismatch { delta: Hash, graph: Hash },
    #[error("base project {base} differs from result project {result}")]
    ProjectMismatch { base: Id, result: Id },
    #[error("base profile {base} differs from result profile {result}")]
    ProfileMismatch { base: Id, result: Id },
    /// A net added/removed/modified set of the delta differs from the graphs' difference.
    #[error("delta {set} does not match the base/result graphs")]
    DeltaGraphMismatch { set: &'static str },
    #[error("element {0} is missing from the base graph")]
    MissingBaseElement(Id),
    #[error("element {0} is missing from the result graph")]
    MissingResultElement(Id),
}

/// Elements whose element-hash projection changed between base and result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangedSet {
    pub node_ids: BTreeSet<Id>,
    pub edge_ids: BTreeSet<Id>,
}

/// Baseline work invalidated by the change: directly changed baseline elements and every node
/// reached through the directed impact relations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirtySet {
    pub node_ids: BTreeSet<Id>,
    pub edge_ids: BTreeSet<Id>,
}

/// A generated projection family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProjectionKind {
    FunctionalYaml,
    RequirementsYaml,
    ArchitectureYaml,
    OpenApi,
    AsyncApi,
    Arazzo,
    Bpmn,
    Dmn,
    Adrs,
    ImplementationPlan,
    TaskContracts,
    VerificationMatrix,
    StandardsConformanceReport,
    Markdown,
    Diagrams,
}

/// A string that is not one of the 15 projection kinds.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown projection kind {0:?}")]
pub struct UnknownProjectionKind(pub String);

impl ProjectionKind {
    pub const ALL: [ProjectionKind; 15] = [
        ProjectionKind::FunctionalYaml,
        ProjectionKind::RequirementsYaml,
        ProjectionKind::ArchitectureYaml,
        ProjectionKind::OpenApi,
        ProjectionKind::AsyncApi,
        ProjectionKind::Arazzo,
        ProjectionKind::Bpmn,
        ProjectionKind::Dmn,
        ProjectionKind::Adrs,
        ProjectionKind::ImplementationPlan,
        ProjectionKind::TaskContracts,
        ProjectionKind::VerificationMatrix,
        ProjectionKind::StandardsConformanceReport,
        ProjectionKind::Markdown,
        ProjectionKind::Diagrams,
    ];

    /// The exact wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectionKind::FunctionalYaml => "functional.yaml",
            ProjectionKind::RequirementsYaml => "requirements.yaml",
            ProjectionKind::ArchitectureYaml => "architecture.yaml",
            ProjectionKind::OpenApi => "openapi",
            ProjectionKind::AsyncApi => "asyncapi",
            ProjectionKind::Arazzo => "arazzo",
            ProjectionKind::Bpmn => "bpmn",
            ProjectionKind::Dmn => "dmn",
            ProjectionKind::Adrs => "adrs",
            ProjectionKind::ImplementationPlan => "implementation-plan",
            ProjectionKind::TaskContracts => "task-contracts",
            ProjectionKind::VerificationMatrix => "verification-matrix",
            ProjectionKind::StandardsConformanceReport => "standards-conformance-report",
            ProjectionKind::Markdown => "markdown",
            ProjectionKind::Diagrams => "diagrams",
        }
    }
}

impl FromStr for ProjectionKind {
    type Err = UnknownProjectionKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ProjectionKind::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| UnknownProjectionKind(s.to_owned()))
    }
}

impl fmt::Display for ProjectionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ProjectionKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProjectionKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Projection families that must be regenerated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AffectedProjectionSet {
    pub projections: BTreeSet<ProjectionKind>,
}

/// Gate namespaces that must be re-evaluated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AffectedGateNamespaceSet {
    pub gates: BTreeSet<GateId>,
}

/// The deterministic impact of one accepted patch. It carries no timestamp, revision ID,
/// staleness, readiness or confidence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactReport {
    pub changed: ChangedSet,
    pub dirty: DirtySet,
    pub affected_projections: AffectedProjectionSet,
    pub affected_gates: AffectedGateNamespaceSet,
}

/// Direction in which a dependency relation propagates impact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactDirection {
    /// A change of the edge's `to` element affects its `from` element.
    ToFrom,
    /// A change of the edge's `from` element affects its `to` element.
    FromTo,
}

/// The 13 dependency relations that propagate impact, and their direction. Every other
/// relation returns `None` and is never traversed.
pub fn impact_direction(kind: &RelationKind) -> Option<ImpactDirection> {
    use ImpactDirection::{FromTo, ToFrom};
    match kind {
        RelationKind::DerivedFrom
        | RelationKind::ConstrainedBy
        | RelationKind::Reads
        | RelationKind::Writes
        | RelationKind::GovernedBy
        | RelationKind::UsesCalculation => Some(ToFrom),
        RelationKind::SpecifiedBy
        | RelationKind::SatisfiedBy
        | RelationKind::AllocatedTo
        | RelationKind::ExposedBy
        | RelationKind::ImplementedBy
        | RelationKind::VerifiedBy
        | RelationKind::ImplementedAs => Some(FromTo),
        _ => None,
    }
}

/// The earliest gate that owns a node type (compiler architecture §22).
pub fn earliest_gate(node_type: NodeType) -> GateId {
    use NodeType as T;
    match node_type {
        T::SourceArtifact
        | T::EvidenceFragment
        | T::DerivationRecord
        | T::Agent
        | T::Finding
        | T::StandardsProfile
        | T::Extension => GateId::I0,
        T::Stakeholder
        | T::Concern
        | T::Goal
        | T::Need
        | T::Requirement
        | T::AcceptanceCriterion
        | T::Constraint
        | T::Term
        | T::Concept => GateId::F1,
        T::Actor
        | T::BusinessRole
        | T::Entity
        | T::Attribute
        | T::DomainRelationship
        | T::State
        | T::Transition
        | T::Invariant
        | T::Operation
        | T::Outcome
        | T::Event
        | T::Process
        | T::ProcessNode
        | T::Rule
        | T::DecisionTable
        | T::Calculation
        | T::Calendar
        | T::Principal
        | T::SecurityRole
        | T::Permission
        | T::ResourceScope
        | T::PolicyCondition
        | T::SeparationConstraint => GateId::F2,
        T::Question | T::ResolutionDecision | T::Assumption => GateId::F3,
        T::Scenario | T::ScenarioRun => GateId::F4,
        T::QualityCharacteristic | T::Measure | T::QualityScenario => GateId::Q1,
        T::SystemOfInterest
        | T::ArchitectureDescription
        | T::ArchitectureCandidate
        | T::Viewpoint
        | T::View
        | T::ModelKind
        | T::SoftwareSystem
        | T::Container
        | T::Component
        | T::Module
        | T::Interface
        | T::DataStore
        | T::ExternalSystem
        | T::DeploymentNode
        | T::RuntimeEnvironment
        | T::NetworkZone
        | T::Technology
        | T::TechnologySelection => GateId::A2,
        T::ApiContract
        | T::ApiOperation
        | T::EventContract
        | T::Channel
        | T::Message
        | T::DataSchema
        | T::TechnicalWorkflow => GateId::A3,
        T::ArchitectureDecision => GateId::A4,
        T::Capability
        | T::ImplementationSlice
        | T::WorkPackage
        | T::TaskContract
        | T::Migration
        | T::Release => GateId::D1,
        T::VerificationObligation | T::TestCase => GateId::D2,
        T::TestExecution
        | T::TestReceipt
        | T::CodeBinding
        | T::ArchitectureCheck
        | T::CoverageRecord => GateId::C1,
    }
}

/// The projection families a dirty node of this type affects, besides the `markdown` and
/// `standards-conformance-report` projections every non-empty DirtySet affects.
pub fn projection_families(node_type: NodeType) -> &'static [ProjectionKind] {
    use NodeType as T;
    use ProjectionKind as P;
    match node_type {
        // Core cannot know an extension's projection semantics: everything is affected.
        T::Extension => &ProjectionKind::ALL,
        T::SourceArtifact
        | T::EvidenceFragment
        | T::DerivationRecord
        | T::Agent
        | T::StandardsProfile => &[],
        T::Finding
        | T::Question
        | T::ResolutionDecision
        | T::Assumption
        | T::Stakeholder
        | T::Concern
        | T::Goal
        | T::Need => &[P::RequirementsYaml],
        T::Requirement | T::AcceptanceCriterion | T::Constraint | T::Term | T::Concept => {
            &[P::RequirementsYaml, P::FunctionalYaml]
        }
        T::Actor | T::BusinessRole => &[P::FunctionalYaml, P::Bpmn],
        T::Entity | T::DomainRelationship | T::State | T::Transition => {
            &[P::FunctionalYaml, P::Diagrams]
        }
        T::Attribute
        | T::Invariant
        | T::Outcome
        | T::Calendar
        | T::Principal
        | T::SecurityRole
        | T::Permission
        | T::ResourceScope
        | T::PolicyCondition
        | T::SeparationConstraint
        | T::QualityCharacteristic
        | T::Measure
        | T::QualityScenario => &[P::FunctionalYaml],
        T::Operation | T::Event | T::Process | T::ProcessNode => {
            &[P::FunctionalYaml, P::Bpmn, P::Diagrams]
        }
        T::Rule | T::DecisionTable | T::Calculation => &[P::FunctionalYaml, P::Dmn],
        T::Scenario => &[P::FunctionalYaml, P::VerificationMatrix],
        T::SystemOfInterest
        | T::ArchitectureDescription
        | T::ArchitectureCandidate
        | T::Viewpoint
        | T::View
        | T::SoftwareSystem
        | T::Container
        | T::Component
        | T::Module
        | T::Interface
        | T::DataStore
        | T::ExternalSystem
        | T::DeploymentNode
        | T::RuntimeEnvironment
        | T::NetworkZone => &[P::ArchitectureYaml, P::Diagrams],
        T::ModelKind => &[P::ArchitectureYaml],
        T::ArchitectureDecision | T::Technology | T::TechnologySelection => {
            &[P::ArchitectureYaml, P::Adrs]
        }
        T::ApiContract => &[P::ArchitectureYaml, P::OpenApi],
        T::ApiOperation => &[P::ArchitectureYaml, P::OpenApi, P::Arazzo, P::Diagrams],
        T::EventContract => &[P::ArchitectureYaml, P::AsyncApi],
        T::Channel | T::Message => &[P::ArchitectureYaml, P::AsyncApi, P::Diagrams],
        T::DataSchema => &[P::ArchitectureYaml, P::OpenApi, P::AsyncApi],
        T::TechnicalWorkflow => &[P::ArchitectureYaml, P::Arazzo],
        T::Capability | T::Migration | T::Release => &[P::ImplementationPlan],
        T::ImplementationSlice | T::WorkPackage => &[P::ImplementationPlan, P::TaskContracts],
        T::TaskContract => &[P::TaskContracts],
        T::VerificationObligation
        | T::TestCase
        | T::ScenarioRun
        | T::TestExecution
        | T::TestReceipt
        | T::CodeBinding
        | T::ArchitectureCheck
        | T::CoverageRecord => &[P::VerificationMatrix],
    }
}

/// Computes the impact of the change from `base` to `result` described by `delta`.
pub fn compute_impact(
    base: &Graph,
    result: &Graph,
    delta: &GraphDelta,
) -> Result<ImpactReport, ImpactError> {
    validate_inputs(base, result, delta)?;
    let changed = changed_set(base, result, delta)?;

    // Direct dirty elements and node seeds.
    let mut dirty = DirtySet::default();
    let mut seeds: BTreeSet<Id> = BTreeSet::new();
    for id in &changed.node_ids {
        if baseline_in(
            base.node(id).map(|n| n.status),
            result.node(id).map(|n| n.status),
        ) {
            seeds.insert(id.clone());
        }
    }
    for id in &changed.edge_ids {
        let (old, new) = (base.edge(id), result.edge(id));
        if baseline_in(old.map(|e| e.status), new.map(|e| e.status)) {
            dirty.edge_ids.insert(id.clone());
            for edge in old.into_iter().chain(new) {
                seeds.insert(edge.from.clone());
                seeds.insert(edge.to.clone());
            }
        }
    }

    // Deterministic traversal over the union of both graphs' baseline dependency arcs.
    let arcs = impact_arcs(base, result);
    let mut queue: VecDeque<Id> = seeds.iter().cloned().collect();
    let mut visited = seeds;
    while let Some(id) = queue.pop_front() {
        if let Some(next) = arcs.get(&id) {
            for dependent in next {
                if visited.insert(dependent.clone()) {
                    queue.push_back(dependent.clone());
                }
            }
        }
    }
    dirty.node_ids = visited;

    let mut affected_projections = AffectedProjectionSet::default();
    let mut affected_gates = AffectedGateNamespaceSet::default();
    if !dirty.node_ids.is_empty() {
        affected_projections.projections.extend([
            ProjectionKind::Markdown,
            ProjectionKind::StandardsConformanceReport,
        ]);
    }
    for id in &dirty.node_ids {
        let node = result
            .node(id)
            .or_else(|| base.node(id))
            .ok_or_else(|| ImpactError::MissingResultElement(id.clone()))?;
        let node_type = node.payload.node_type();
        affected_projections
            .projections
            .extend(projection_families(node_type));
        let earliest = earliest_gate(node_type);
        affected_gates
            .gates
            .extend(GateId::ALL.into_iter().filter(|g| *g >= earliest));
    }

    Ok(ImpactReport {
        changed,
        dirty,
        affected_projections,
        affected_gates,
    })
}

fn baseline_in(
    base: Option<plumb_psg::ElementStatus>,
    result: Option<plumb_psg::ElementStatus>,
) -> bool {
    base.is_some_and(is_baseline) || result.is_some_and(is_baseline)
}

/// `dependency -> dependents` over baseline edges of the 13 impact relations of both graphs.
fn impact_arcs(base: &Graph, result: &Graph) -> BTreeMap<Id, BTreeSet<Id>> {
    let mut arcs: BTreeMap<Id, BTreeSet<Id>> = BTreeMap::new();
    for graph in [base, result] {
        for edge in graph.edges().values() {
            if !is_baseline(edge.status) {
                continue;
            }
            let (from, to) = match impact_direction(&edge.kind) {
                Some(ImpactDirection::ToFrom) => (&edge.to, &edge.from),
                Some(ImpactDirection::FromTo) => (&edge.from, &edge.to),
                None => continue,
            };
            arcs.entry(from.clone()).or_default().insert(to.clone());
        }
    }
    arcs
}

fn validate_inputs(base: &Graph, result: &Graph, delta: &GraphDelta) -> Result<(), ImpactError> {
    let base_hash = base.semantic_hash()?;
    if delta.base_semantic_hash != base_hash {
        return Err(ImpactError::BaseHashMismatch {
            delta: delta.base_semantic_hash.clone(),
            graph: base_hash,
        });
    }
    let result_hash = result.semantic_hash()?;
    if delta.result_semantic_hash != result_hash {
        return Err(ImpactError::ResultHashMismatch {
            delta: delta.result_semantic_hash.clone(),
            graph: result_hash,
        });
    }
    if base.project_id() != result.project_id() {
        return Err(ImpactError::ProjectMismatch {
            base: base.project_id().clone(),
            result: result.project_id().clone(),
        });
    }
    if base.profile_id() != result.profile_id() {
        return Err(ImpactError::ProfileMismatch {
            base: base.profile_id().clone(),
            result: result.profile_id().clone(),
        });
    }
    let (added, removed, modified) = net(base.nodes(), result.nodes());
    let (added_e, removed_e, modified_e) = net(base.edges(), result.edges());
    for (set, expected, actual) in [
        ("added_nodes", &added, &delta.added_nodes),
        ("removed_nodes", &removed, &delta.removed_nodes),
        ("modified_nodes", &modified, &delta.modified_nodes),
        ("added_edges", &added_e, &delta.added_edges),
        ("removed_edges", &removed_e, &delta.removed_edges),
        ("modified_edges", &modified_e, &delta.modified_edges),
    ] {
        if expected != actual {
            return Err(ImpactError::DeltaGraphMismatch { set });
        }
    }
    Ok(())
}

/// Net added, removed and modified IDs by persisted equality.
fn net<T: PartialEq>(
    base: &BTreeMap<Id, T>,
    result: &BTreeMap<Id, T>,
) -> (BTreeSet<Id>, BTreeSet<Id>, BTreeSet<Id>) {
    let added = result
        .keys()
        .filter(|id| !base.contains_key(*id))
        .cloned()
        .collect();
    let removed = base
        .keys()
        .filter(|id| !result.contains_key(*id))
        .cloned()
        .collect();
    let modified = base
        .iter()
        .filter(|(id, old)| result.get(*id).is_some_and(|new| new != *old))
        .map(|(id, _)| id.clone())
        .collect();
    (added, removed, modified)
}

fn changed_set(
    base: &Graph,
    result: &Graph,
    delta: &GraphDelta,
) -> Result<ChangedSet, ImpactError> {
    let mut changed = ChangedSet::default();
    changed.node_ids.extend(
        delta
            .added_nodes
            .iter()
            .chain(&delta.removed_nodes)
            .cloned(),
    );
    for id in &delta.modified_nodes {
        let old: &Node = base
            .node(id)
            .ok_or_else(|| ImpactError::MissingBaseElement(id.clone()))?;
        let new: &Node = result
            .node(id)
            .ok_or_else(|| ImpactError::MissingResultElement(id.clone()))?;
        if node_element_hash(old)? != node_element_hash(new)? {
            changed.node_ids.insert(id.clone());
        }
    }
    changed.edge_ids.extend(
        delta
            .added_edges
            .iter()
            .chain(&delta.removed_edges)
            .cloned(),
    );
    for id in &delta.modified_edges {
        let old: &Edge = base
            .edge(id)
            .ok_or_else(|| ImpactError::MissingBaseElement(id.clone()))?;
        let new: &Edge = result
            .edge(id)
            .ok_or_else(|| ImpactError::MissingResultElement(id.clone()))?;
        if edge_element_hash(old)? != edge_element_hash(new)? {
            changed.edge_ids.insert(id.clone());
        }
    }
    Ok(changed)
}
