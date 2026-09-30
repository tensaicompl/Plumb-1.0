//! The one-way PSG → functional.yaml v2 compatibility projection (compiler architecture §30,
//! v2 plan §4.1).
//!
//! Only Accepted nodes and edges contribute, and associations are read only from typed
//! payload fields and typed relations. Whatever the legacy shape cannot carry is omitted or
//! collapsed deterministically and recorded as a notice; nothing is fabricated, the graph is
//! never changed and nothing is written back.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{CoreError, Hash, HashKind, Id};
use plumb_psg::{
    ActorKind, ElementStatus, Graph, Node, NodePayload, NodeType, OutcomeKind, ProcessNodeKind,
    RelationKind, RelationProperties, SchemaBindingRole,
};
use plumb_validation::ValidationProfile;
use serde_json::Value;
use thiserror::Error;

use crate::model::codes::*;
use crate::model::*;

/// The machine schema every projected document is validated against.
pub const FUNCTIONAL_V2_SCHEMA: &str = include_str!("../../../schemas/functional-v2.schema.json");

/// The JSON Schema dialect `FUNCTIONAL_V2_SCHEMA` declares and is validated with.
pub const FUNCTIONAL_V2_SCHEMA_DRAFT: Draft = Draft::Draft202012;

/// Why a projection could not be produced.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProjectionError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("YAML serialization failed: {0}")]
    Yaml(String),
    #[error("functional-v2 schema: {0}")]
    Schema(String),
    #[error("graph profile {graph} is not the validation profile {profile}")]
    ProfileMismatch { graph: Id, profile: Id },
    #[error("conflicting legacy nfr.{key} values for {target}")]
    ConflictingLegacyNfr { target: Id, key: String },
    #[error("invalid projection notice: {0}")]
    InvalidProjectionNotice(String),
    /// An accepted element no listed notice code can carry.
    #[error("{id} cannot be represented in functional.yaml v2: {reason}")]
    UnrepresentableElement { id: Id, reason: String },
}

/// Projects `graph` into a functional.yaml v2 document with its YAML bytes and metadata.
pub fn project_functional_v2(
    graph: &Graph,
    profile: &ValidationProfile,
) -> Result<FunctionalProjection, ProjectionError> {
    if graph.profile_id() != &profile.profile_id {
        return Err(ProjectionError::ProfileMismatch {
            graph: graph.profile_id().clone(),
            profile: profile.profile_id.clone(),
        });
    }
    let source_semantic_hash = graph.semantic_hash()?;
    let mut projector = Projector {
        graph,
        warnings: BTreeSet::new(),
        lossy: BTreeSet::new(),
        nfr: BTreeMap::new(),
    };
    projector.non_accepted_baseline();
    let glossary = projector.glossary();
    let actors = projector.actors();
    let roles = projector.roles();
    let operations = projector.operations();
    let rules = projector.rules();
    let calendars = projector.calendars();
    let calculations = projector.calculations();
    let events = projector.events();
    let requirements = projector.requirements();
    let processes = projector.processes();
    let scenarios = projector.scenarios()?;
    let assumptions = projector.assumptions();
    projector.quality_scenarios();
    projector.idempotency();
    let mut entities = projector.entities();
    let mut operations = operations;
    projector.apply_nfr(&mut entities, &mut operations)?;

    let document = FunctionalV2 {
        version: FUNCTIONAL_V2_VERSION,
        model_hash: source_semantic_hash.clone(),
        glossary,
        actors,
        roles,
        entities,
        calendars,
        calculations,
        rules,
        operations,
        processes,
        events,
        requirements,
        scenarios,
        assumptions,
    };
    validate_against_schema(&document)?;
    let yaml = serde_yaml::to_string(&document)
        .map_err(|e| ProjectionError::Yaml(e.to_string()))?
        .into_bytes();
    let metadata = ProjectionMetadata {
        source_semantic_hash,
        profile_hash: profile.profile_hash()?,
        projection_version: FUNCTIONAL_V2_PROJECTION_VERSION,
        content_hash: Hash::content_sha256(&yaml),
        warnings: projector.warnings.into_iter().collect(),
        lossy_mappings: projector.lossy.into_iter().collect(),
    };
    validate_metadata(&metadata)?;
    Ok(FunctionalProjection {
        document,
        yaml,
        metadata,
    })
}

/// Validates `document` against [`FUNCTIONAL_V2_SCHEMA`].
///
/// The schema declares JSON Schema draft 2020-12 and is evaluated with the draft 2020-12
/// validator. No reference is resolved over the network.
pub fn validate_against_schema<T: serde::Serialize>(document: &T) -> Result<(), ProjectionError> {
    let schema: Value = serde_json::from_str(FUNCTIONAL_V2_SCHEMA)
        .map_err(|e| ProjectionError::Schema(format!("schema is not JSON: {e}")))?;
    let compiled = JSONSchema::options()
        .with_draft(FUNCTIONAL_V2_SCHEMA_DRAFT)
        .compile(&schema)
        .map_err(|e| ProjectionError::Schema(format!("schema does not compile: {e}")))?;
    let instance = serde_json::to_value(document)
        .map_err(|e| ProjectionError::Schema(format!("document is not JSON: {e}")))?;
    let result = compiled.validate(&instance);
    if let Err(errors) = result {
        let mut messages: Vec<String> = errors
            .map(|e| format!("{} at {}", e, e.instance_path))
            .collect();
        messages.sort();
        return Err(ProjectionError::Schema(messages.join("; ")));
    }
    Ok(())
}

/// Checks the notice and hash invariants of projection metadata.
pub fn validate_metadata(metadata: &ProjectionMetadata) -> Result<(), ProjectionError> {
    let invalid = ProjectionError::InvalidProjectionNotice;
    if metadata.source_semantic_hash.kind() != HashKind::Semantic
        || metadata.profile_hash.kind() != HashKind::Generic
        || metadata.content_hash.kind() != HashKind::Generic
        || metadata.projection_version != FUNCTIONAL_V2_PROJECTION_VERSION
    {
        return Err(invalid("metadata hash kinds or projection version".into()));
    }
    for list in [&metadata.warnings, &metadata.lossy_mappings] {
        for notice in list {
            if !is_clean(&notice.code) || !codes::ALL.contains(&notice.code.as_str()) {
                return Err(invalid(format!(
                    "unknown or unclean code {:?}",
                    notice.code
                )));
            }
            if !is_clean(&notice.message) {
                return Err(invalid(format!("unclean message for {}", notice.code)));
            }
            if notice.source_refs.windows(2).any(|w| w[0] >= w[1]) {
                return Err(invalid(format!("unsorted source_refs for {}", notice.code)));
            }
        }
        if list.windows(2).any(|w| w[0] >= w[1]) {
            return Err(invalid("notices are not sorted and unique".into()));
        }
    }
    Ok(())
}

fn is_clean(text: &str) -> bool {
    !text.is_empty() && text.trim() == text && !text.chars().any(char::is_control)
}

/// The fixed message of each notice code.
fn message(code: &str) -> &'static str {
    match code {
        V2_NON_ACCEPTED_BASELINE_OMITTED => {
            "Suspect, Superseded or Deprecated element omitted from the confirmed legacy view."
        }
        V2_GLOSSARY_DEFINITION_MISSING => "Term has no accepted concept definition.",
        V2_ORGANIZATION_ACTOR_COLLAPSE => "Organization actor projected as external.",
        V2_BUSINESS_ROLE_COLLAPSE => "Business role projected with itself as actor token.",
        V2_SECURITY_ROLE_COLLAPSE => {
            "Security role without a unique assigned actor projected with itself as actor token."
        }
        V2_PERMISSION_SCOPE_OMITTED => "Permission scope and conditions are not representable.",
        V2_ATTRIBUTE_NULLABILITY_OMITTED => "Attribute nullability is not representable.",
        V2_RELATIONSHIP_DETAIL_OMITTED => {
            "Relationship kind, name or ownership is not representable."
        }
        V2_EVENT_TRIGGER_TRANSITION_OMITTED => "Event-triggered transition omitted.",
        V2_OPERATION_PERFORMER_MISSING => "No performer; the legacy object is omitted.",
        V2_MULTIPLE_PERFORMERS_COLLAPSED => "Several performers collapsed to the smallest ID.",
        V2_ENTITY_LEVEL_READ_WRITE_OMITTED => "Entity-level read or write omitted.",
        V2_OUTCOME_KIND_COLLAPSE => "Outcome kind collapsed to failure.",
        V2_IDEMPOTENCY_UNREPRESENTABLE => "Idempotency value is not a legacy boolean.",
        V2_CALCULATION_TARGET_UNREPRESENTABLE => "Calculation target is not representable.",
        V2_RULE_TABLE_UNREPRESENTABLE => "Rule is not representable as a legacy table.",
        V2_DECISION_TABLE_DETAIL_OMITTED => "Decision table hit policy and outputs omitted.",
        V2_EVENT_PAYLOAD_UNREPRESENTABLE => "Event payload attributes are not representable.",
        V2_ACCEPTANCE_CRITERION_UNMAPPED => "Acceptance criterion has no legacy requirement.",
        V2_PROCESS_TRIGGER_UNREPRESENTABLE => "Process trigger is not representable.",
        V2_PROCESS_STEP_OPERATION_MISSING => "Process node without operation omitted.",
        V2_QUALITY_SCENARIO_COLLAPSE => "Quality scenario collapsed to a legacy NFR value.",
        V2_QUALITY_SCENARIO_OMITTED => "Quality scenario has no legacy NFR mapping.",
        V2_SCENARIO_MULTI_REQUIREMENT_COLLAPSE => {
            "Several requirements collapsed to the smallest ID."
        }
        V2_SCENARIO_REQUIREMENT_MISSING => "Scenario has no requirement; omitted.",
        V2_SCENARIO_OPERATION_MISSING => "Scenario has no when.operation; omitted.",
        V2_SCENARIO_THEN_CONFLICT => "Scenario then objects conflict; omitted.",
        V2_ASSUMPTION_UNREPRESENTABLE => "Assumption lacks legacy-required fields; omitted.",
        _ => "",
    }
}

/// Node types the legacy document reads.
const PROJECTED_NODE_TYPES: [NodeType; 30] = [
    NodeType::Term,
    NodeType::Concept,
    NodeType::Actor,
    NodeType::BusinessRole,
    NodeType::SecurityRole,
    NodeType::Permission,
    NodeType::ResourceScope,
    NodeType::PolicyCondition,
    NodeType::Entity,
    NodeType::Attribute,
    NodeType::DomainRelationship,
    NodeType::State,
    NodeType::Transition,
    NodeType::Invariant,
    NodeType::Operation,
    NodeType::Outcome,
    NodeType::Event,
    NodeType::Process,
    NodeType::ProcessNode,
    NodeType::Rule,
    NodeType::DecisionTable,
    NodeType::Calculation,
    NodeType::Calendar,
    NodeType::DataSchema,
    NodeType::Requirement,
    NodeType::AcceptanceCriterion,
    NodeType::QualityScenario,
    NodeType::QualityCharacteristic,
    NodeType::Scenario,
    NodeType::Assumption,
];

/// Relations the legacy document reads.
const PROJECTED_RELATIONS: [RelationKind; 17] = [
    RelationKind::HasAttribute,
    RelationKind::HasState,
    RelationKind::TransitionsVia,
    RelationKind::PerformedBy,
    RelationKind::Reads,
    RelationKind::Writes,
    RelationKind::Produces,
    RelationKind::GovernedBy,
    RelationKind::Next,
    RelationKind::AssignedRole,
    RelationKind::Grants,
    RelationKind::Permits,
    RelationKind::ScopedTo,
    RelationKind::ConditionedBy,
    RelationKind::CharacterizedBy,
    RelationKind::SchemaFor,
    RelationKind::SpecifiedBy,
];

const ENTITY_NFR_KEYS: [&str; 5] = [
    "consistency",
    "availability",
    "volatility",
    "residency",
    "retention",
];
const OPERATION_NFR_KEYS: [&str; 3] = ["latency_ms", "audit", "idempotent"];

/// The performers of an operation or process node.
enum Performer {
    None,
    One(Id),
    /// Several performers: the smallest ID and all of them.
    Many(Id, Vec<Id>),
}

/// One candidate legacy NFR value and the element it came from.
struct NfrSource {
    value: Value,
}

struct Projector<'g> {
    graph: &'g Graph,
    warnings: BTreeSet<ProjectionNotice>,
    lossy: BTreeSet<ProjectionNotice>,
    /// (target, legacy key) -> candidate values.
    nfr: BTreeMap<(Id, &'static str), Vec<NfrSource>>,
}

fn notice(code: &str, refs: impl IntoIterator<Item = Id>, message: &str) -> ProjectionNotice {
    let refs: BTreeSet<Id> = refs.into_iter().collect();
    ProjectionNotice {
        code: code.to_owned(),
        source_refs: refs.into_iter().collect(),
        message: message.to_owned(),
    }
}

impl<'g> Projector<'g> {
    fn warn(&mut self, code: &str, refs: impl IntoIterator<Item = Id>) {
        self.warnings.insert(notice(code, refs, message(code)));
    }

    fn lossy(&mut self, code: &str, refs: impl IntoIterator<Item = Id>) {
        self.lossy.insert(notice(code, refs, message(code)));
    }

    // ------------------------------------------------------------------ graph access

    /// Accepted nodes of `node_type`, in ID order.
    fn accepted(&self, node_type: NodeType) -> Vec<&'g Node> {
        self.graph
            .node_ids_by_type(node_type)
            .iter()
            .filter_map(|id| self.graph.node(id))
            .filter(|node| node.status == ElementStatus::Accepted)
            .collect()
    }

    fn accepted_node(&self, id: &Id) -> Option<&'g Node> {
        self.graph
            .node(id)
            .filter(|node| node.status == ElementStatus::Accepted)
    }

    fn is_accepted_type(&self, id: &Id, node_type: NodeType) -> bool {
        self.accepted_node(id)
            .is_some_and(|node| node.payload.node_type() == node_type)
    }

    /// Targets of Accepted `kind` edges leaving `from` that are Accepted nodes, sorted.
    fn targets(&self, from: &Id, kind: RelationKind) -> Vec<Id> {
        let mut targets: Vec<Id> = self
            .graph
            .outgoing_edge_ids(from)
            .iter()
            .filter_map(|id| self.graph.edge(id))
            .filter(|edge| edge.kind == kind && edge.status == ElementStatus::Accepted)
            .map(|edge| edge.to.clone())
            .filter(|to| self.accepted_node(to).is_some())
            .collect();
        targets.sort();
        targets.dedup();
        targets
    }

    /// Sources of Accepted `kind` edges entering `to` that are Accepted nodes, sorted.
    fn sources(&self, to: &Id, kind: RelationKind) -> Vec<Id> {
        let mut sources: Vec<Id> = self
            .graph
            .incoming_edge_ids(to)
            .iter()
            .filter_map(|id| self.graph.edge(id))
            .filter(|edge| edge.kind == kind && edge.status == ElementStatus::Accepted)
            .map(|edge| edge.from.clone())
            .filter(|from| self.accepted_node(from).is_some())
            .collect();
        sources.sort();
        sources.dedup();
        sources
    }

    fn typed_targets(&self, from: &Id, kind: RelationKind, node_type: NodeType) -> Vec<Id> {
        self.targets(from, kind)
            .into_iter()
            .filter(|id| self.is_accepted_type(id, node_type))
            .collect()
    }

    fn performer(&self, id: &Id) -> Performer {
        let performers = self.targets(id, RelationKind::PerformedBy);
        match performers.as_slice() {
            [] => Performer::None,
            [one] => Performer::One(one.clone()),
            [first, ..] => Performer::Many(first.clone(), performers.clone()),
        }
    }

    /// Resolves a performer, recording the collapse of several; `None` when there is none.
    fn resolve_performer(&mut self, owner: &Id, performer: Performer) -> Option<Id> {
        match performer {
            Performer::None => None,
            Performer::One(id) => Some(id),
            Performer::Many(first, all) => {
                self.lossy(
                    V2_MULTIPLE_PERFORMERS_COLLAPSED,
                    std::iter::once(owner.clone()).chain(all),
                );
                Some(first)
            }
        }
    }

    // ------------------------------------------------------------------ status

    fn non_accepted_baseline(&mut self) {
        let omitted = |status: ElementStatus| {
            matches!(
                status,
                ElementStatus::Suspect | ElementStatus::Superseded | ElementStatus::Deprecated
            )
        };
        let mut refs: Vec<Id> = Vec::new();
        for node_type in PROJECTED_NODE_TYPES {
            for id in self.graph.node_ids_by_type(node_type) {
                if self.graph.node(id).is_some_and(|n| omitted(n.status)) {
                    refs.push(id.clone());
                }
            }
        }
        for kind in PROJECTED_RELATIONS {
            for id in self.graph.edge_ids_by_kind(&kind) {
                if let Some(edge) = self.graph.edge(id).filter(|e| omitted(e.status)) {
                    // A transition whose only trigger edge is omitted is omitted with it.
                    let mut notice_refs = vec![id.clone()];
                    if kind == RelationKind::TransitionsVia
                        && self.is_accepted_type(&edge.from, NodeType::Transition)
                    {
                        notice_refs.push(edge.from.clone());
                    }
                    self.warn(V2_NON_ACCEPTED_BASELINE_OMITTED, notice_refs);
                }
            }
        }
        for id in refs {
            self.warn(V2_NON_ACCEPTED_BASELINE_OMITTED, [id]);
        }
    }

    // ------------------------------------------------------------------ glossary / actors / roles

    fn glossary(&mut self) -> Vec<GlossaryEntry> {
        let mut entries = Vec::new();
        let mut defined: BTreeSet<Id> = BTreeSet::new();
        for node in self.accepted(NodeType::Term) {
            let NodePayload::Term(term) = &node.payload else {
                continue;
            };
            let concept = term.definition_ref.as_ref().and_then(|id| {
                defined.insert(id.clone());
                match self.accepted_node(id).map(|n| &n.payload) {
                    Some(NodePayload::Concept(concept)) => Some(concept.definition.clone()),
                    _ => None,
                }
            });
            let definition = match concept {
                Some(definition) => definition,
                None => {
                    self.warn(V2_GLOSSARY_DEFINITION_MISSING, [node.id.clone()]);
                    String::new()
                }
            };
            let mut aliases = term.aliases.clone().unwrap_or_default();
            aliases.sort();
            entries.push(GlossaryEntry {
                id: node.id.clone(),
                name: term.term.clone(),
                definition,
                aliases,
            });
        }
        for node in self.accepted(NodeType::Concept) {
            let NodePayload::Concept(concept) = &node.payload else {
                continue;
            };
            if defined.contains(&node.id) {
                continue;
            }
            entries.push(GlossaryEntry {
                id: node.id.clone(),
                name: concept.name.clone(),
                definition: concept.definition.clone(),
                aliases: Vec::new(),
            });
        }
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        entries
    }

    fn actors(&mut self) -> Vec<ActorEntry> {
        let mut entries = Vec::new();
        for node in self.accepted(NodeType::Actor) {
            let NodePayload::Actor(actor) = &node.payload else {
                continue;
            };
            let kind = match actor.actor_kind {
                ActorKind::Human => LegacyActorKind::Human,
                ActorKind::System => LegacyActorKind::System,
                ActorKind::ExternalSystem => LegacyActorKind::External,
                ActorKind::Organization => {
                    self.lossy(V2_ORGANIZATION_ACTOR_COLLAPSE, [node.id.clone()]);
                    LegacyActorKind::External
                }
            };
            entries.push(ActorEntry {
                id: node.id.clone(),
                kind,
            });
        }
        entries
    }

    fn roles(&mut self) -> Vec<RoleEntry> {
        let mut entries = Vec::new();
        for node in self.accepted(NodeType::BusinessRole) {
            let operations = self
                .sources(&node.id, RelationKind::PerformedBy)
                .into_iter()
                .filter(|id| self.is_accepted_type(id, NodeType::Operation))
                .collect();
            self.lossy(V2_BUSINESS_ROLE_COLLAPSE, [node.id.clone()]);
            entries.push(RoleEntry {
                id: node.id.clone(),
                actor: node.id.clone(),
                operations,
            });
        }
        for node in self.accepted(NodeType::SecurityRole) {
            let mut operations: BTreeSet<Id> = BTreeSet::new();
            for permission in
                self.typed_targets(&node.id, RelationKind::Grants, NodeType::Permission)
            {
                operations.extend(self.typed_targets(
                    &permission,
                    RelationKind::Permits,
                    NodeType::Operation,
                ));
                let scopes = self.targets(&permission, RelationKind::ScopedTo);
                let conditions = self.targets(&permission, RelationKind::ConditionedBy);
                if !scopes.is_empty() || !conditions.is_empty() {
                    self.lossy(
                        V2_PERMISSION_SCOPE_OMITTED,
                        [node.id.clone(), permission.clone()]
                            .into_iter()
                            .chain(scopes)
                            .chain(conditions),
                    );
                }
            }
            let assigned: Vec<Id> = self
                .sources(&node.id, RelationKind::AssignedRole)
                .into_iter()
                .filter(|id| self.is_accepted_type(id, NodeType::Actor))
                .collect();
            let actor = match assigned.as_slice() {
                [actor] => actor.clone(),
                _ => {
                    self.lossy(
                        V2_SECURITY_ROLE_COLLAPSE,
                        std::iter::once(node.id.clone()).chain(assigned.iter().cloned()),
                    );
                    node.id.clone()
                }
            };
            entries.push(RoleEntry {
                id: node.id.clone(),
                actor,
                operations: operations.into_iter().collect(),
            });
        }
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        entries
    }

    // ------------------------------------------------------------------ entities

    fn entities(&mut self) -> Vec<EntityEntry> {
        let mut entries = Vec::new();
        for node in self.accepted(NodeType::Entity) {
            let entity = node.id.clone();
            let attributes = self.attributes(&entity);
            let relationships = self.relationships(&entity);
            let states = self
                .typed_targets(&entity, RelationKind::HasState, NodeType::State)
                .into_iter()
                .map(|id| StateEntry { id })
                .collect();
            let transitions = self.transitions(&entity);
            let invariants = self.invariants(&entity);
            entries.push(EntityEntry {
                id: entity,
                attributes,
                relationships,
                states,
                transitions,
                invariants,
                nfr: None,
            });
        }
        entries
    }

    fn attributes(&mut self, entity: &Id) -> Vec<AttributeEntry> {
        let mut entries = Vec::new();
        for id in self.typed_targets(entity, RelationKind::HasAttribute, NodeType::Attribute) {
            let Some(NodePayload::Attribute(attribute)) = self.graph.node(&id).map(|n| &n.payload)
            else {
                continue;
            };
            self.lossy(V2_ATTRIBUTE_NULLABILITY_OMITTED, [id.clone()]);
            entries.push(AttributeEntry {
                id,
                value_type: attribute.value_type.clone(),
                class: attribute
                    .data_classification
                    .clone()
                    .unwrap_or_else(|| "none".to_owned()),
                unit: attribute.unit.clone(),
                precision: attribute.precision,
                enum_values: attribute.enum_values.clone(),
            });
        }
        entries
    }

    fn relationships(&mut self, entity: &Id) -> Vec<RelationshipEntry> {
        let mut entries = Vec::new();
        for node in self.accepted(NodeType::DomainRelationship) {
            let NodePayload::DomainRelationship(relationship) = &node.payload else {
                continue;
            };
            if &relationship.from_entity != entity {
                continue;
            }
            if !relationship.relationship_kind.is_empty()
                || relationship.name.is_some()
                || relationship.ownership.is_some()
            {
                self.lossy(V2_RELATIONSHIP_DETAIL_OMITTED, [node.id.clone()]);
            }
            entries.push(RelationshipEntry {
                id: node.id.clone(),
                to: relationship.to_entity.clone(),
                card_from: relationship.cardinality_from.clone(),
                card_to: relationship.cardinality_to.clone(),
                snapshot: relationship.snapshot_semantics.unwrap_or(false),
            });
        }
        entries
    }

    fn transitions(&mut self, entity: &Id) -> Vec<TransitionEntry> {
        let mut entries = Vec::new();
        for node in self.accepted(NodeType::Transition) {
            let NodePayload::Transition(transition) = &node.payload else {
                continue;
            };
            if &transition.stateful_ref != entity {
                continue;
            }
            let triggers = self.targets(&node.id, RelationKind::TransitionsVia);
            // Without exactly one Accepted trigger the non-accepted edge was already recorded.
            let [trigger] = triggers.as_slice() else {
                continue;
            };
            if self.is_accepted_type(trigger, NodeType::Event) {
                self.warn(
                    V2_EVENT_TRIGGER_TRANSITION_OMITTED,
                    [node.id.clone(), trigger.clone()],
                );
                continue;
            }
            let performer = self.performer(trigger);
            let Some(actor) = self.resolve_performer(trigger, performer) else {
                self.warn(
                    V2_OPERATION_PERFORMER_MISSING,
                    [node.id.clone(), trigger.clone()],
                );
                continue;
            };
            entries.push((
                node.id.clone(),
                TransitionEntry {
                    from: transition.from_state.clone(),
                    to: transition.to_state.clone(),
                    operation: trigger.clone(),
                    actor,
                    precondition: transition.guard_expr.clone(),
                },
            ));
        }
        // Legacy transitions have no ID; they keep the order of their Transition IDs.
        entries.into_iter().map(|(_, entry)| entry).collect()
    }

    fn invariants(&mut self, entity: &Id) -> Vec<InvariantEntry> {
        self.accepted(NodeType::Invariant)
            .into_iter()
            .filter_map(|node| match &node.payload {
                NodePayload::Invariant(invariant) if &invariant.scope_ref == entity => {
                    Some(InvariantEntry {
                        id: node.id.clone(),
                        expr: invariant.expression.clone(),
                    })
                }
                _ => None,
            })
            .collect()
    }

    // ------------------------------------------------------------------ operations

    fn operations(&mut self) -> Vec<OperationEntry> {
        let mut entries = Vec::new();
        for node in self.accepted(NodeType::Operation) {
            let NodePayload::Operation(operation) = &node.payload else {
                continue;
            };
            let id = node.id.clone();
            let performer = self.performer(&id);
            let Some(actor) = self.resolve_performer(&id, performer) else {
                self.warn(V2_OPERATION_PERFORMER_MISSING, [id]);
                continue;
            };
            let reads = self.attribute_access(&id, RelationKind::Reads);
            let writes = self.attribute_access(&id, RelationKind::Writes);
            let mut outcomes = Vec::new();
            for outcome in self.typed_targets(&id, RelationKind::Produces, NodeType::Outcome) {
                let Some(NodePayload::Outcome(payload)) =
                    self.graph.node(&outcome).map(|n| &n.payload)
                else {
                    continue;
                };
                let kind = match payload.outcome_kind {
                    OutcomeKind::Success => LegacyOutcomeKind::Success,
                    OutcomeKind::BusinessFailure
                    | OutcomeKind::TechnicalFailure
                    | OutcomeKind::Partial => {
                        self.lossy(V2_OUTCOME_KIND_COLLAPSE, [outcome.clone()]);
                        LegacyOutcomeKind::Failure
                    }
                };
                outcomes.push(OutcomeEntry { id: outcome, kind });
            }
            let governed_by = self
                .targets(&id, RelationKind::GovernedBy)
                .into_iter()
                .filter(|target| {
                    self.is_accepted_type(target, NodeType::Rule)
                        || self.is_accepted_type(target, NodeType::DecisionTable)
                })
                .collect();
            entries.push(OperationEntry {
                id,
                kind: wire(operation.operation_kind),
                actor,
                reads,
                writes,
                pre: operation.preconditions.clone().unwrap_or_default(),
                post: operation.postconditions.clone().unwrap_or_default(),
                outcomes,
                governed_by,
                nfr: None,
            });
        }
        entries
    }

    /// Attribute targets of `kind`; Entity targets are recorded and omitted.
    fn attribute_access(&mut self, operation: &Id, kind: RelationKind) -> Vec<Id> {
        let targets = self.targets(operation, kind.clone());
        let entities: Vec<Id> = targets
            .iter()
            .filter(|id| self.is_accepted_type(id, NodeType::Entity))
            .cloned()
            .collect();
        if !entities.is_empty() {
            self.warn(
                V2_ENTITY_LEVEL_READ_WRITE_OMITTED,
                std::iter::once(operation.clone()).chain(entities),
            );
        }
        targets
            .into_iter()
            .filter(|id| self.is_accepted_type(id, NodeType::Attribute))
            .collect()
    }

    fn idempotency(&mut self) {
        for node in self.accepted(NodeType::Operation) {
            let NodePayload::Operation(operation) = &node.payload else {
                continue;
            };
            match operation.idempotency.as_deref() {
                None => {}
                Some("true") => self.add_nfr(&node.id, "idempotent", Value::Bool(true)),
                Some("false") => self.add_nfr(&node.id, "idempotent", Value::Bool(false)),
                Some(_) => self.warn(V2_IDEMPOTENCY_UNREPRESENTABLE, [node.id.clone()]),
            }
        }
    }

    // ------------------------------------------------------------------ calendars / calculations / rules / events

    fn calendars(&mut self) -> Vec<CalendarEntry> {
        self.accepted(NodeType::Calendar)
            .into_iter()
            .filter_map(|node| match &node.payload {
                NodePayload::Calendar(calendar) => Some(CalendarEntry {
                    id: node.id.clone(),
                    region: calendar.region.clone(),
                    tz: calendar.time_zone.clone(),
                    holidays_ref: calendar.holiday_source.clone(),
                    week_pattern: calendar.week_pattern.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    fn calculations(&mut self) -> Vec<CalculationEntry> {
        for node in self.accepted(NodeType::Calculation) {
            self.warn(V2_CALCULATION_TARGET_UNREPRESENTABLE, [node.id.clone()]);
        }
        Vec::new()
    }

    fn rules(&mut self) -> Vec<RuleEntry> {
        for node in self.accepted(NodeType::Rule) {
            self.warn(V2_RULE_TABLE_UNREPRESENTABLE, [node.id.clone()]);
        }
        let mut entries = Vec::new();
        for node in self.accepted(NodeType::DecisionTable) {
            let NodePayload::DecisionTable(table) = &node.payload else {
                continue;
            };
            if !table.inputs.iter().chain(&table.rows).all(Value::is_object) {
                self.warn(V2_RULE_TABLE_UNREPRESENTABLE, [node.id.clone()]);
                continue;
            }
            if !table.hit_policy.is_empty() || !table.outputs.is_empty() {
                self.lossy(V2_DECISION_TABLE_DETAIL_OMITTED, [node.id.clone()]);
            }
            entries.push(RuleEntry {
                id: node.id.clone(),
                conditions: table.inputs.clone(),
                rows: table.rows.clone(),
            });
        }
        entries
    }

    fn events(&mut self) -> Vec<EventEntry> {
        let mut entries = Vec::new();
        for node in self.accepted(NodeType::Event) {
            let NodePayload::Event(event) = &node.payload else {
                continue;
            };
            let mut payload = Vec::new();
            if let Some(schema) = &event.payload_schema_ref {
                if self.is_accepted_type(schema, NodeType::DataSchema) {
                    payload = self.schema_attributes(schema);
                }
                if payload.is_empty() {
                    self.warn(
                        V2_EVENT_PAYLOAD_UNREPRESENTABLE,
                        [node.id.clone(), schema.clone()],
                    );
                }
            }
            entries.push(EventEntry {
                id: node.id.clone(),
                payload,
            });
        }
        entries
    }

    /// Attributes bound by Accepted `schema_for` edges with the attribute role.
    fn schema_attributes(&self, schema: &Id) -> Vec<Id> {
        let mut attributes: Vec<Id> = self
            .graph
            .outgoing_edge_ids(schema)
            .iter()
            .filter_map(|id| self.graph.edge(id))
            .filter(|edge| {
                edge.kind == RelationKind::SchemaFor
                    && edge.status == ElementStatus::Accepted
                    && matches!(
                        &edge.properties,
                        RelationProperties::SchemaFor(p) if p.role == SchemaBindingRole::Attribute
                    )
            })
            .map(|edge| edge.to.clone())
            .filter(|to| self.is_accepted_type(to, NodeType::Attribute))
            .collect();
        attributes.sort();
        attributes.dedup();
        attributes
    }

    // ------------------------------------------------------------------ requirements

    fn requirements(&mut self) -> Vec<RequirementEntry> {
        for node in self.accepted(NodeType::AcceptanceCriterion) {
            self.warn(V2_ACCEPTANCE_CRITERION_UNMAPPED, [node.id.clone()]);
        }
        self.accepted(NodeType::Requirement)
            .into_iter()
            .filter_map(|node| match &node.payload {
                NodePayload::Requirement(requirement) => Some(RequirementEntry {
                    id: node.id.clone(),
                    text: requirement.statement.clone(),
                    class: wire(requirement.requirement_kind),
                    criteria: Vec::new(),
                    operations: self.typed_targets(
                        &node.id,
                        RelationKind::SpecifiedBy,
                        NodeType::Operation,
                    ),
                    status: "Confirmed".to_owned(),
                }),
                _ => None,
            })
            .collect()
    }

    // ------------------------------------------------------------------ processes

    fn processes(&mut self) -> Vec<ProcessEntry> {
        let mut entries = Vec::new();
        for process in self.accepted(NodeType::Process) {
            let nodes: Vec<&Node> = self
                .accepted(NodeType::ProcessNode)
                .into_iter()
                .filter(|node| {
                    matches!(&node.payload, NodePayload::ProcessNode(p) if p.process_ref == process.id)
                })
                .collect();
            let starts: Vec<&Node> = nodes
                .iter()
                .copied()
                .filter(|node| {
                    matches!(&node.payload, NodePayload::ProcessNode(p) if p.node_kind == ProcessNodeKind::Start)
                })
                .collect();
            let trigger = match starts.as_slice() {
                [start] => match &start.payload {
                    NodePayload::ProcessNode(p) => match (&p.message_ref, &p.timer_expr) {
                        (Some(message), None) => Some(TriggerEntry {
                            kind: "event".to_owned(),
                            reference: message.to_string(),
                        }),
                        (None, Some(timer)) => Some(TriggerEntry {
                            kind: "timer".to_owned(),
                            reference: timer.clone(),
                        }),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            };
            let Some(trigger) = trigger else {
                self.warn(
                    V2_PROCESS_TRIGGER_UNREPRESENTABLE,
                    std::iter::once(process.id.clone()).chain(starts.iter().map(|n| n.id.clone())),
                );
                continue;
            };

            let mut steps = Vec::new();
            let mut outcomes: BTreeSet<Id> = BTreeSet::new();
            for node in &nodes {
                let NodePayload::ProcessNode(payload) = &node.payload else {
                    continue;
                };
                outcomes.extend(self.typed_targets(
                    &node.id,
                    RelationKind::Produces,
                    NodeType::Outcome,
                ));
                if matches!(
                    payload.node_kind,
                    ProcessNodeKind::Start | ProcessNodeKind::End
                ) {
                    continue;
                }
                let Some(operation) = payload.operation_ref.clone() else {
                    self.warn(V2_PROCESS_STEP_OPERATION_MISSING, [node.id.clone()]);
                    continue;
                };
                let own = self.performer(&node.id);
                let actor = match own {
                    Performer::None => {
                        let fallback = self.performer(&operation);
                        self.resolve_performer(&operation, fallback)
                    }
                    other => self.resolve_performer(&node.id, other),
                };
                let Some(actor) = actor else {
                    self.warn(
                        V2_OPERATION_PERFORMER_MISSING,
                        [node.id.clone(), operation.clone()],
                    );
                    continue;
                };
                let emits = self.typed_targets(&node.id, RelationKind::Produces, NodeType::Event);
                let next = self
                    .targets(&node.id, RelationKind::Next)
                    .into_iter()
                    .map(|to| NextEntry {
                        to,
                        when: payload.condition_expr.clone(),
                    })
                    .collect();
                steps.push(StepEntry {
                    id: node.id.clone(),
                    actor,
                    operation,
                    emits,
                    next,
                });
            }
            entries.push(ProcessEntry {
                id: process.id.clone(),
                trigger,
                steps,
                outcomes: outcomes.into_iter().collect(),
            });
        }
        entries
    }

    // ------------------------------------------------------------------ quality scenarios

    fn quality_scenarios(&mut self) {
        for node in self.accepted(NodeType::QualityScenario) {
            let NodePayload::QualityScenario(scenario) = &node.payload else {
                continue;
            };
            match self.legacy_nfr_key(&node.id, scenario) {
                Some((target, key)) => {
                    self.lossy(
                        V2_QUALITY_SCENARIO_COLLAPSE,
                        [node.id.clone(), target.clone()],
                    );
                    self.add_nfr(&target, key, scenario.threshold.clone());
                }
                None => self.warn(V2_QUALITY_SCENARIO_OMITTED, [node.id.clone()]),
            }
        }
    }

    /// The legacy target and NFR key of an eligible quality scenario.
    fn legacy_nfr_key(
        &self,
        id: &Id,
        scenario: &plumb_psg::QualityScenario,
    ) -> Option<(Id, &'static str)> {
        let [characteristic] = self
            .typed_targets(
                id,
                RelationKind::CharacterizedBy,
                NodeType::QualityCharacteristic,
            )
            .try_into()
            .ok()?;
        let Some(NodePayload::QualityCharacteristic(characteristic)) =
            self.graph.node(&characteristic).map(|n| &n.payload)
        else {
            return None;
        };
        let [target] = scenario.affected_refs.as_deref()? else {
            return None;
        };
        if self.is_accepted_type(target, NodeType::Entity) {
            let key = ENTITY_NFR_KEYS
                .into_iter()
                .find(|key| *key == characteristic.name)?;
            return Some((target.clone(), key));
        }
        if self.is_accepted_type(target, NodeType::Operation) {
            let key = OPERATION_NFR_KEYS
                .into_iter()
                .find(|key| *key == characteristic.name)?;
            let typed = match key {
                "latency_ms" => scenario.threshold.is_number(),
                _ => scenario.threshold.is_boolean(),
            };
            return typed.then(|| (target.clone(), key));
        }
        None
    }

    fn add_nfr(&mut self, target: &Id, key: &'static str, value: Value) {
        self.nfr
            .entry((target.clone(), key))
            .or_default()
            .push(NfrSource { value });
    }

    /// Writes the collected NFR values; different values for one target and key conflict.
    fn apply_nfr(
        &self,
        entities: &mut [EntityEntry],
        operations: &mut [OperationEntry],
    ) -> Result<(), ProjectionError> {
        for ((target, key), sources) in &self.nfr {
            let value = &sources[0].value;
            if sources.iter().any(|source| &source.value != value) {
                return Err(ProjectionError::ConflictingLegacyNfr {
                    target: target.clone(),
                    key: (*key).to_owned(),
                });
            }
            if let Some(entity) = entities.iter_mut().find(|e| &e.id == target) {
                let nfr = entity.nfr.get_or_insert_with(EntityNfr::default);
                let slot = match *key {
                    "consistency" => &mut nfr.consistency,
                    "availability" => &mut nfr.availability,
                    "volatility" => &mut nfr.volatility,
                    "residency" => &mut nfr.residency,
                    _ => &mut nfr.retention,
                };
                *slot = Some(value.clone());
            } else if let Some(operation) = operations.iter_mut().find(|o| &o.id == target) {
                let nfr = operation.nfr.get_or_insert_with(OperationNfr::default);
                match *key {
                    "latency_ms" => nfr.latency_ms = Some(value.clone()),
                    "audit" => nfr.audit = value.as_bool(),
                    _ => nfr.idempotent = value.as_bool(),
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ scenarios / assumptions

    fn scenarios(&mut self) -> Result<Vec<ScenarioEntry>, ProjectionError> {
        let mut entries = Vec::new();
        for node in self.accepted(NodeType::Scenario) {
            let NodePayload::Scenario(scenario) = &node.payload else {
                continue;
            };
            let id = node.id.clone();
            if let Some(entry) = scenario
                .given
                .iter()
                .chain(&scenario.then)
                .find(|entry| !entry.is_object())
            {
                return Err(ProjectionError::UnrepresentableElement {
                    id,
                    reason: format!("given/then entry {entry} is not a JSON object"),
                });
            }
            let Some(operation) = scenario.when.get("operation").and_then(Value::as_str) else {
                self.warn(V2_SCENARIO_OPERATION_MISSING, [id]);
                continue;
            };
            let mut requirements = scenario.requirement_refs.clone().unwrap_or_default();
            requirements.sort();
            requirements.dedup();
            let requirement = match requirements.as_slice() {
                [] => {
                    self.warn(V2_SCENARIO_REQUIREMENT_MISSING, [id]);
                    continue;
                }
                [one] => one.clone(),
                [first, ..] => {
                    self.lossy(
                        V2_SCENARIO_MULTI_REQUIREMENT_COLLAPSE,
                        std::iter::once(id.clone()).chain(requirements.iter().cloned()),
                    );
                    first.clone()
                }
            };
            let mut then: BTreeMap<String, Value> = BTreeMap::new();
            let mut conflict = false;
            for object in scenario.then.iter().filter_map(Value::as_object) {
                for (key, value) in object {
                    conflict |= then.insert(key.clone(), value.clone()).is_some();
                }
            }
            if conflict {
                self.warn(V2_SCENARIO_THEN_CONFLICT, [id]);
                continue;
            }
            entries.push(ScenarioEntry {
                id,
                requirement,
                operation: operation.to_owned(),
                given: scenario.given.clone(),
                when: scenario.when.clone(),
                then,
                status: "Confirmed".to_owned(),
            });
        }
        Ok(entries)
    }

    fn assumptions(&mut self) -> Vec<AssumptionEntry> {
        let mut entries = Vec::new();
        for node in self.accepted(NodeType::Assumption) {
            let NodePayload::Assumption(assumption) = &node.payload else {
                continue;
            };
            match (
                &assumption.finding_ref,
                &assumption.default_value,
                &assumption.expires_at,
            ) {
                (Some(finding), Some(default), Some(expires)) => entries.push(AssumptionEntry {
                    id: node.id.clone(),
                    finding: finding.clone(),
                    default: default.clone(),
                    owner: assumption.owner_ref.clone(),
                    expires: *expires,
                }),
                _ => self.warn(V2_ASSUMPTION_UNREPRESENTABLE, [node.id.clone()]),
            }
        }
        entries
    }
}

/// The wire string of a PSG closed enum value.
fn wire(value: impl serde::Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        _ => String::new(),
    }
}
