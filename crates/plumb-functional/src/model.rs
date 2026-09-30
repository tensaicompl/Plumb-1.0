//! The functional.yaml v2 compatibility document and its companion projection metadata
//! (v2 plan §4.1; compiler architecture §30).
//!
//! These types describe projection output only. The PSG stays canonical: nothing here is
//! read back into a graph.

use std::collections::BTreeMap;

use plumb_core::{Hash, Id, Timestamp};
use serde::Serialize;
use serde_json::Value;

/// The `version` of the functional.yaml v2 document.
pub const FUNCTIONAL_V2_VERSION: u32 = 2;

/// The `projection_version` of the functional.yaml v2 projection metadata.
pub const FUNCTIONAL_V2_PROJECTION_VERSION: u32 = 2;

/// The functional.yaml v2 document: exactly `version`, `model_hash` and thirteen collections.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionalV2 {
    pub version: u32,
    pub model_hash: Hash,

    pub glossary: Vec<GlossaryEntry>,
    pub actors: Vec<ActorEntry>,
    pub roles: Vec<RoleEntry>,
    pub entities: Vec<EntityEntry>,
    pub calendars: Vec<CalendarEntry>,
    pub calculations: Vec<CalculationEntry>,
    pub rules: Vec<RuleEntry>,
    pub operations: Vec<OperationEntry>,
    pub processes: Vec<ProcessEntry>,
    pub events: Vec<EventEntry>,
    pub requirements: Vec<RequirementEntry>,
    pub scenarios: Vec<ScenarioEntry>,
    pub assumptions: Vec<AssumptionEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GlossaryEntry {
    pub id: Id,
    pub name: String,
    pub definition: String,
    pub aliases: Vec<String>,
}

/// The v2 actor kind vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LegacyActorKind {
    Human,
    System,
    External,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActorEntry {
    pub id: Id,
    pub kind: LegacyActorKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoleEntry {
    pub id: Id,
    pub actor: Id,
    pub operations: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntityEntry {
    pub id: Id,
    pub attributes: Vec<AttributeEntry>,
    pub relationships: Vec<RelationshipEntry>,
    pub states: Vec<StateEntry>,
    pub transitions: Vec<TransitionEntry>,
    pub invariants: Vec<InvariantEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nfr: Option<EntityNfr>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AttributeEntry {
    pub id: Id,
    #[serde(rename = "type")]
    pub value_type: String,
    pub class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precision: Option<u32>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RelationshipEntry {
    pub id: Id,
    pub to: Id,
    pub card_from: String,
    pub card_to: String,
    pub snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateEntry {
    pub id: Id,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TransitionEntry {
    pub from: Id,
    pub to: Id,
    pub operation: Id,
    pub actor: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precondition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InvariantEntry {
    pub id: Id,
    pub expr: String,
}

/// Legacy entity NFR values, present only when a QualityScenario maps to them.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EntityNfr {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consistency: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volatility: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub residency: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalendarEntry {
    pub id: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    pub tz: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holidays_ref: Option<String>,
    pub week_pattern: Value,
}

/// The v2 calculation shape. The pilot PSG cannot supply its target, so none is projected.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalculationEntry {
    pub id: Id,
    pub target: Id,
    pub expr: String,
    pub unit: String,
    pub rounding: RoundingEntry,
    pub inputs: Vec<Id>,
    pub examples: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoundingEntry {
    pub mode: String,
    pub step: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuleEntry {
    pub id: Id,
    pub conditions: Vec<Value>,
    pub rows: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OperationEntry {
    pub id: Id,
    pub kind: String,
    pub actor: Id,
    pub reads: Vec<Id>,
    pub writes: Vec<Id>,
    pub pre: Vec<String>,
    pub post: Vec<String>,
    pub outcomes: Vec<OutcomeEntry>,
    pub governed_by: Vec<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nfr: Option<OperationNfr>,
}

/// The v2 outcome kind vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LegacyOutcomeKind {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutcomeEntry {
    pub id: Id,
    pub kind: LegacyOutcomeKind,
}

/// Legacy operation NFR values, present only when mapped.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct OperationNfr {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProcessEntry {
    pub id: Id,
    pub trigger: TriggerEntry,
    pub steps: Vec<StepEntry>,
    pub outcomes: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TriggerEntry {
    pub kind: String,
    #[serde(rename = "ref")]
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepEntry {
    pub id: Id,
    pub actor: Id,
    pub operation: Id,
    pub emits: Vec<Id>,
    pub next: Vec<NextEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NextEntry {
    pub to: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventEntry {
    pub id: Id,
    pub payload: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RequirementEntry {
    pub id: Id,
    pub text: String,
    pub class: String,
    pub criteria: Vec<Value>,
    pub operations: Vec<Id>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScenarioEntry {
    pub id: Id,
    pub requirement: Id,
    pub operation: String,
    pub given: Vec<Value>,
    pub when: Value,
    pub then: BTreeMap<String, Value>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssumptionEntry {
    pub id: Id,
    pub finding: Id,
    pub default: Value,
    pub owner: Id,
    pub expires: Timestamp,
}

/// One recorded omission or lossy collapse. It carries no timestamp.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ProjectionNotice {
    pub code: String,
    pub source_refs: Vec<Id>,
    pub message: String,
}

/// Companion metadata of a functional.yaml v2 projection. Never part of the YAML itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectionMetadata {
    pub source_semantic_hash: Hash,
    pub profile_hash: Hash,
    pub projection_version: u32,
    pub content_hash: Hash,

    pub warnings: Vec<ProjectionNotice>,
    pub lossy_mappings: Vec<ProjectionNotice>,
}

/// A complete projection: the typed document, its exact YAML bytes and its metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionalProjection {
    pub document: FunctionalV2,
    pub yaml: Vec<u8>,
    pub metadata: ProjectionMetadata,
}

/// The closed notice codes of the functional.yaml v2 projection.
pub mod codes {
    pub const V2_NON_ACCEPTED_BASELINE_OMITTED: &str = "V2_NON_ACCEPTED_BASELINE_OMITTED";
    pub const V2_GLOSSARY_DEFINITION_MISSING: &str = "V2_GLOSSARY_DEFINITION_MISSING";
    pub const V2_ORGANIZATION_ACTOR_COLLAPSE: &str = "V2_ORGANIZATION_ACTOR_COLLAPSE";
    pub const V2_BUSINESS_ROLE_COLLAPSE: &str = "V2_BUSINESS_ROLE_COLLAPSE";
    pub const V2_SECURITY_ROLE_COLLAPSE: &str = "V2_SECURITY_ROLE_COLLAPSE";
    pub const V2_PERMISSION_SCOPE_OMITTED: &str = "V2_PERMISSION_SCOPE_OMITTED";
    pub const V2_ATTRIBUTE_NULLABILITY_OMITTED: &str = "V2_ATTRIBUTE_NULLABILITY_OMITTED";
    pub const V2_RELATIONSHIP_DETAIL_OMITTED: &str = "V2_RELATIONSHIP_DETAIL_OMITTED";
    pub const V2_EVENT_TRIGGER_TRANSITION_OMITTED: &str = "V2_EVENT_TRIGGER_TRANSITION_OMITTED";
    pub const V2_OPERATION_PERFORMER_MISSING: &str = "V2_OPERATION_PERFORMER_MISSING";
    pub const V2_MULTIPLE_PERFORMERS_COLLAPSED: &str = "V2_MULTIPLE_PERFORMERS_COLLAPSED";
    pub const V2_ENTITY_LEVEL_READ_WRITE_OMITTED: &str = "V2_ENTITY_LEVEL_READ_WRITE_OMITTED";
    pub const V2_OUTCOME_KIND_COLLAPSE: &str = "V2_OUTCOME_KIND_COLLAPSE";
    pub const V2_IDEMPOTENCY_UNREPRESENTABLE: &str = "V2_IDEMPOTENCY_UNREPRESENTABLE";
    pub const V2_CALCULATION_TARGET_UNREPRESENTABLE: &str = "V2_CALCULATION_TARGET_UNREPRESENTABLE";
    pub const V2_RULE_TABLE_UNREPRESENTABLE: &str = "V2_RULE_TABLE_UNREPRESENTABLE";
    pub const V2_DECISION_TABLE_DETAIL_OMITTED: &str = "V2_DECISION_TABLE_DETAIL_OMITTED";
    pub const V2_EVENT_PAYLOAD_UNREPRESENTABLE: &str = "V2_EVENT_PAYLOAD_UNREPRESENTABLE";
    pub const V2_ACCEPTANCE_CRITERION_UNMAPPED: &str = "V2_ACCEPTANCE_CRITERION_UNMAPPED";
    pub const V2_PROCESS_TRIGGER_UNREPRESENTABLE: &str = "V2_PROCESS_TRIGGER_UNREPRESENTABLE";
    pub const V2_PROCESS_STEP_OPERATION_MISSING: &str = "V2_PROCESS_STEP_OPERATION_MISSING";
    pub const V2_QUALITY_SCENARIO_COLLAPSE: &str = "V2_QUALITY_SCENARIO_COLLAPSE";
    pub const V2_QUALITY_SCENARIO_OMITTED: &str = "V2_QUALITY_SCENARIO_OMITTED";
    pub const V2_SCENARIO_MULTI_REQUIREMENT_COLLAPSE: &str =
        "V2_SCENARIO_MULTI_REQUIREMENT_COLLAPSE";
    pub const V2_SCENARIO_REQUIREMENT_MISSING: &str = "V2_SCENARIO_REQUIREMENT_MISSING";
    pub const V2_SCENARIO_OPERATION_MISSING: &str = "V2_SCENARIO_OPERATION_MISSING";
    pub const V2_SCENARIO_THEN_CONFLICT: &str = "V2_SCENARIO_THEN_CONFLICT";
    pub const V2_ASSUMPTION_UNREPRESENTABLE: &str = "V2_ASSUMPTION_UNREPRESENTABLE";

    /// Every code, in contract order.
    pub const ALL: [&str; 28] = [
        V2_NON_ACCEPTED_BASELINE_OMITTED,
        V2_GLOSSARY_DEFINITION_MISSING,
        V2_ORGANIZATION_ACTOR_COLLAPSE,
        V2_BUSINESS_ROLE_COLLAPSE,
        V2_SECURITY_ROLE_COLLAPSE,
        V2_PERMISSION_SCOPE_OMITTED,
        V2_ATTRIBUTE_NULLABILITY_OMITTED,
        V2_RELATIONSHIP_DETAIL_OMITTED,
        V2_EVENT_TRIGGER_TRANSITION_OMITTED,
        V2_OPERATION_PERFORMER_MISSING,
        V2_MULTIPLE_PERFORMERS_COLLAPSED,
        V2_ENTITY_LEVEL_READ_WRITE_OMITTED,
        V2_OUTCOME_KIND_COLLAPSE,
        V2_IDEMPOTENCY_UNREPRESENTABLE,
        V2_CALCULATION_TARGET_UNREPRESENTABLE,
        V2_RULE_TABLE_UNREPRESENTABLE,
        V2_DECISION_TABLE_DETAIL_OMITTED,
        V2_EVENT_PAYLOAD_UNREPRESENTABLE,
        V2_ACCEPTANCE_CRITERION_UNMAPPED,
        V2_PROCESS_TRIGGER_UNREPRESENTABLE,
        V2_PROCESS_STEP_OPERATION_MISSING,
        V2_QUALITY_SCENARIO_COLLAPSE,
        V2_QUALITY_SCENARIO_OMITTED,
        V2_SCENARIO_MULTI_REQUIREMENT_COLLAPSE,
        V2_SCENARIO_REQUIREMENT_MISSING,
        V2_SCENARIO_OPERATION_MISSING,
        V2_SCENARIO_THEN_CONFLICT,
        V2_ASSUMPTION_UNREPRESENTABLE,
    ];
}
