//! Typed references from PSG elements to evidence and derivations (metamodel §4.5), and the
//! census of graph-bound payload references (architecture §33.11).
//!
//! `EvidenceRef` and `DerivationRef` are transparent wrappers over a validated [`Id`]. They
//! impose no prefix or namespace restriction: whether the referenced ID resolves to evidence or
//! to a `DerivationRecord` is checked by graph-semantic validation, not here.

use plumb_core::Id;
use serde::{Deserialize, Serialize};

use crate::payload::{NodePayload, NodeType};

/// Reference to an evidence element. Serializes as the underlying ID string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EvidenceRef(Id);

impl EvidenceRef {
    /// The referenced ID.
    pub fn as_id(&self) -> &Id {
        &self.0
    }

    /// The exact referenced ID string.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Id> for EvidenceRef {
    fn from(id: Id) -> Self {
        Self(id)
    }
}

/// Reference to a derivation record. Serializes as the underlying ID string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DerivationRef(Id);

impl DerivationRef {
    /// The referenced ID.
    pub fn as_id(&self) -> &Id {
        &self.0
    }

    /// The exact referenced ID string.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<Id> for DerivationRef {
    fn from(id: Id) -> Self {
        Self(id)
    }
}

// ============================================================================ payload reference census

/// What a graph-bound payload reference must resolve to (architecture §33.11 census).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefTarget {
    /// A node of one of these types.
    Types(&'static [NodeType]),
    /// Any node.
    AnyNode,
    /// Any existing node or edge.
    AnyElement,
}

impl RefTarget {
    /// Whether a node of `node_type` satisfies the expectation.
    pub fn allows(self, node_type: NodeType) -> bool {
        match self {
            RefTarget::Types(types) => types.contains(&node_type),
            RefTarget::AnyNode | RefTarget::AnyElement => true,
        }
    }
}

/// Which target statuses a graph-bound payload reference allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefStatusPolicy {
    /// A baseline-participating owner needs a baseline-participating target, a Proposed owner
    /// may also reference a Proposed target, and a Rejected target is never valid.
    Participation,
    /// The target must be Accepted (the governing human Agent of a decision).
    AcceptedOnly,
    /// Only existence (and the type, if one is named) is checked.
    ExistenceOnly,
}

/// One graph-bound reference carried by a payload field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadRef<'a> {
    /// The payload field, as serialized.
    pub field: &'static str,
    pub target: &'a Id,
    pub expected: RefTarget,
    pub policy: RefStatusPolicy,
}

const AGENT: &[NodeType] = &[NodeType::Agent];
const AGENT_OR_STAKEHOLDER: &[NodeType] = &[NodeType::Agent, NodeType::Stakeholder];
const REQUIREMENT_OWNER: &[NodeType] = &[NodeType::Agent, NodeType::Stakeholder, NodeType::Actor];
const STAKEHOLDER: &[NodeType] = &[NodeType::Stakeholder];
const FINDING: &[NodeType] = &[NodeType::Finding];
const DECISION: &[NodeType] = &[NodeType::ResolutionDecision];
const GOAL: &[NodeType] = &[NodeType::Goal];
const SCENARIO: &[NodeType] = &[NodeType::Scenario];
const CONCEPT: &[NodeType] = &[NodeType::Concept];
const ENTITY: &[NodeType] = &[NodeType::Entity];
const STATE: &[NodeType] = &[NodeType::State];
const EVENT_OR_OPERATION: &[NodeType] = &[NodeType::Event, NodeType::Operation];
const INVARIANT_SCOPE: &[NodeType] = &[
    NodeType::Entity,
    NodeType::Attribute,
    NodeType::Process,
    NodeType::Operation,
];
const PROCESS: &[NodeType] = &[NodeType::Process];
const OPERATION: &[NodeType] = &[NodeType::Operation];
const EVENT: &[NodeType] = &[NodeType::Event];
const CALENDAR: &[NodeType] = &[NodeType::Calendar];
const REQUIREMENT: &[NodeType] = &[NodeType::Requirement];
/// The pilot resource subset (plan S2.8, Hotfix 042): an Operation, Entity or Attribute.
const RESOURCE: &[NodeType] = &[NodeType::Operation, NodeType::Entity, NodeType::Attribute];
const ROLE: &[NodeType] = &[NodeType::BusinessRole, NodeType::SecurityRole];

/// Collects the references of one payload in field order.
struct Census<'a>(Vec<PayloadRef<'a>>);

impl<'a> Census<'a> {
    fn all(
        &mut self,
        field: &'static str,
        targets: impl IntoIterator<Item = &'a Id>,
        expected: RefTarget,
        policy: RefStatusPolicy,
    ) -> &mut Self {
        self.0.extend(targets.into_iter().map(|target| PayloadRef {
            field,
            target,
            expected,
            policy,
        }));
        self
    }

    /// A reference with a type and participation contract.
    fn typed(
        &mut self,
        field: &'static str,
        targets: impl IntoIterator<Item = &'a Id>,
        types: &'static [NodeType],
    ) -> &mut Self {
        self.all(
            field,
            targets,
            RefTarget::Types(types),
            RefStatusPolicy::Participation,
        )
    }

    /// A post-pilot (or explicitly existence-only) reference to any node or edge.
    fn exists(
        &mut self,
        field: &'static str,
        targets: impl IntoIterator<Item = &'a Id>,
    ) -> &mut Self {
        self.all(
            field,
            targets,
            RefTarget::AnyElement,
            RefStatusPolicy::ExistenceOnly,
        )
    }
}

/// Every graph-bound reference of `payload`, in field order (architecture §33.11 census).
///
/// External or opaque identifiers are never returned: `Question.round_ref`,
/// `Question.context_refs` (subject-first, and the subject may be a candidate ID that a
/// compiler builds only once the question is answered, metamodel Hotfix 047),
/// `ResolutionDecision.proposal_ref`, `Operation.input_schema_ref`/`output_schema_ref`,
/// `Event.payload_schema_ref`, `EvidenceLocator::ConversationTurn.turn_id`, and the node's own
/// identity in `DerivationRecord.id`/`StandardsProfile.id`. `EvidenceFragment.source_ref` and
/// `ResolutionDecision.question_ref` are graph-bound but have dedicated graph rules with their
/// own violations, so they are not listed here. Post-pilot payload references are checked for
/// existence only until their stages define type contracts.
pub fn payload_refs(payload: &NodePayload) -> Vec<PayloadRef<'_>> {
    use RefStatusPolicy::{AcceptedOnly, ExistenceOnly, Participation};
    let mut c = Census(Vec::new());
    match payload {
        NodePayload::Finding(p) => {
            c.exists("affected_refs", &p.affected_refs).typed(
                "waiver_ref",
                &p.waiver_ref,
                DECISION,
            );
        }
        NodePayload::Question(p) => {
            c.typed("finding_ref", [&p.finding_ref], FINDING).typed(
                "stakeholder_ref",
                &p.stakeholder_ref,
                STAKEHOLDER,
            );
        }
        NodePayload::ResolutionDecision(p) => {
            c.all(
                "decided_by",
                [&p.decided_by],
                RefTarget::Types(AGENT),
                AcceptedOnly,
            )
            .typed("supersedes", &p.supersedes, DECISION);
        }
        NodePayload::Assumption(p) => {
            c.typed("owner_ref", [&p.owner_ref], AGENT_OR_STAKEHOLDER)
                .typed("finding_ref", &p.finding_ref, FINDING)
                .all("risk_ref", &p.risk_ref, RefTarget::AnyNode, ExistenceOnly);
        }
        NodePayload::Need(p) => {
            c.typed("stakeholder_refs", &p.stakeholder_refs, STAKEHOLDER)
                .typed("goal_refs", p.goal_refs.iter().flatten(), GOAL);
        }
        NodePayload::Requirement(p) => {
            c.typed(
                "owner_refs",
                p.owner_refs.iter().flatten(),
                REQUIREMENT_OWNER,
            )
            .typed(
                "stakeholder_refs",
                p.stakeholder_refs.iter().flatten(),
                STAKEHOLDER,
            );
        }
        NodePayload::AcceptanceCriterion(p) => {
            c.all(
                "measure_ref",
                &p.measure_ref,
                RefTarget::AnyNode,
                ExistenceOnly,
            )
            .typed("scenario_ref", &p.scenario_ref, SCENARIO);
        }
        NodePayload::Term(p) => {
            c.typed("definition_ref", &p.definition_ref, CONCEPT);
        }
        NodePayload::DomainRelationship(p) => {
            c.typed("from_entity", [&p.from_entity], ENTITY).typed(
                "to_entity",
                [&p.to_entity],
                ENTITY,
            );
        }
        NodePayload::Transition(p) => {
            c.typed("stateful_ref", [&p.stateful_ref], ENTITY)
                .typed("from_state", [&p.from_state], STATE)
                .typed("to_state", [&p.to_state], STATE)
                .typed(
                    "effect_refs",
                    p.effect_refs.iter().flatten(),
                    EVENT_OR_OPERATION,
                );
        }
        NodePayload::Invariant(p) => {
            c.typed("scope_ref", [&p.scope_ref], INVARIANT_SCOPE);
        }
        NodePayload::ProcessNode(p) => {
            c.typed("process_ref", [&p.process_ref], PROCESS)
                .typed("operation_ref", &p.operation_ref, OPERATION)
                .typed("message_ref", &p.message_ref, EVENT);
        }
        NodePayload::Calculation(p) => {
            c.typed("calendar_ref", &p.calendar_ref, CALENDAR);
        }
        NodePayload::Scenario(p) => {
            c.typed(
                "requirement_refs",
                p.requirement_refs.iter().flatten(),
                REQUIREMENT,
            )
            .all(
                "derived_from_refs",
                p.derived_from_refs.iter().flatten(),
                RefTarget::AnyNode,
                Participation,
            );
        }
        NodePayload::ResourceScope(p) => {
            c.typed("resource_ref", [&p.resource_ref], RESOURCE);
        }
        NodePayload::SeparationConstraint(p) => {
            c.typed("role_refs", &p.role_refs, ROLE);
        }
        NodePayload::ScenarioRun(p) => {
            c.typed("scenario_ref", [&p.scenario_ref], SCENARIO);
        }
        // Post-pilot payloads: existence only.
        NodePayload::QualityScenario(p) => {
            c.exists("source_ref", &p.source_ref)
                .exists("affected_refs", p.affected_refs.iter().flatten());
        }
        NodePayload::ArchitectureDescription(p) => {
            c.exists("system_of_interest_ref", [&p.system_of_interest_ref])
                .exists("architecture_candidate_ref", &p.architecture_candidate_ref);
        }
        NodePayload::Viewpoint(p) => {
            c.exists("concern_refs", &p.concern_refs)
                .exists("stakeholder_refs", &p.stakeholder_refs)
                .exists("model_kind_refs", &p.model_kind_refs);
        }
        NodePayload::View(p) => {
            c.exists("viewpoint_ref", [&p.viewpoint_ref])
                .exists(
                    "architecture_description_ref",
                    [&p.architecture_description_ref],
                )
                .exists("root_refs", p.root_refs.iter().flatten());
        }
        NodePayload::SoftwareSystem(p)
        | NodePayload::Container(p)
        | NodePayload::Component(p)
        | NodePayload::Module(p)
        | NodePayload::Interface(p)
        | NodePayload::DataStore(p)
        | NodePayload::ExternalSystem(p)
        | NodePayload::DeploymentNode(p)
        | NodePayload::RuntimeEnvironment(p)
        | NodePayload::NetworkZone(p) => {
            c.exists("owner_ref", &p.owner_ref);
        }
        NodePayload::ArchitectureDecision(p) => {
            c.exists("affected_refs", p.affected_refs.iter().flatten())
                .exists("supersedes", &p.supersedes);
        }
        NodePayload::TechnologySelection(p) => {
            c.exists("technology_ref", [&p.technology_ref])
                .exists("alternatives", p.alternatives.iter().flatten())
                .exists("architecture_decision_ref", &p.architecture_decision_ref);
        }
        NodePayload::ApiOperation(p) => {
            c.exists("api_contract_ref", [&p.api_contract_ref])
                .exists("security_refs", p.security_refs.iter().flatten());
        }
        NodePayload::Message(p) => {
            c.exists("correlation_ref", &p.correlation_ref);
        }
        NodePayload::Capability(p) => {
            c.exists("goal_refs", p.goal_refs.iter().flatten())
                .exists("requirement_refs", p.requirement_refs.iter().flatten());
        }
        NodePayload::WorkPackage(p) => {
            c.exists("owner_ref", &p.owner_ref)
                .exists("target_release_ref", &p.target_release_ref);
        }
        NodePayload::TaskContract(p) => {
            c.exists("allowed_change_refs", &p.allowed_change_refs)
                .exists("forbidden_change_refs", &p.forbidden_change_refs)
                .exists("implementation_slice_ref", &p.implementation_slice_ref)
                .exists("decision_refs", p.decision_refs.iter().flatten())
                .exists("verification_refs", p.verification_refs.iter().flatten());
        }
        NodePayload::Migration(p) => {
            c.exists("source_ref", [&p.source_ref])
                .exists("target_ref", [&p.target_ref]);
        }
        NodePayload::TestExecution(p) => {
            c.exists("test_case_ref", [&p.test_case_ref]);
        }
        NodePayload::TestReceipt(p) => {
            c.exists(
                "verification_obligation_ref",
                [&p.verification_obligation_ref],
            )
            .exists("environment_ref", [&p.environment_ref]);
        }
        NodePayload::ArchitectureCheck(p) => {
            c.exists("affected_refs", &p.affected_refs);
        }
        NodePayload::CoverageRecord(p) => {
            c.exists("source_refs", &p.source_refs)
                .exists("target_refs", &p.target_refs);
        }
        // No graph-bound payload reference (EvidenceFragment.source_ref has its own rule).
        NodePayload::SourceArtifact(_)
        | NodePayload::EvidenceFragment(_)
        | NodePayload::DerivationRecord(_)
        | NodePayload::Agent(_)
        | NodePayload::Stakeholder(_)
        | NodePayload::Concern(_)
        | NodePayload::Goal(_)
        | NodePayload::Constraint(_)
        | NodePayload::Concept(_)
        | NodePayload::Actor(_)
        | NodePayload::BusinessRole(_)
        | NodePayload::Entity(_)
        | NodePayload::Attribute(_)
        | NodePayload::State(_)
        | NodePayload::Operation(_)
        | NodePayload::Outcome(_)
        | NodePayload::Event(_)
        | NodePayload::Process(_)
        | NodePayload::Rule(_)
        | NodePayload::DecisionTable(_)
        | NodePayload::Calendar(_)
        | NodePayload::Principal(_)
        | NodePayload::SecurityRole(_)
        | NodePayload::Permission(_)
        | NodePayload::PolicyCondition(_)
        | NodePayload::QualityCharacteristic(_)
        | NodePayload::Measure(_)
        | NodePayload::SystemOfInterest(_)
        | NodePayload::ArchitectureCandidate(_)
        | NodePayload::ModelKind(_)
        | NodePayload::Technology(_)
        | NodePayload::ApiContract(_)
        | NodePayload::EventContract(_)
        | NodePayload::Channel(_)
        | NodePayload::DataSchema(_)
        | NodePayload::TechnicalWorkflow(_)
        | NodePayload::ImplementationSlice(_)
        | NodePayload::Release(_)
        | NodePayload::VerificationObligation(_)
        | NodePayload::TestCase(_)
        | NodePayload::CodeBinding(_)
        | NodePayload::StandardsProfile(_)
        | NodePayload::Extension(_) => {}
    }
    c.0
}
