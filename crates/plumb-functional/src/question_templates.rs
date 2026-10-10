//! The closed deterministic Question template registry (plan S3.1, Hotfix 045).
//!
//! Each template maps exactly one finding code and semantic-condition contract to a typed
//! [`QuestionKind`], a routing key, canonical slots, a fixed prompt and a closed JSON answer
//! schema. Choice lists are the sorted IDs of Accepted nodes of exactly the stated types. There
//! are no model-written templates and no runtime registration; every other finding is unmapped.

use plumb_core::Id;
use plumb_psg::{ElementStatus, Graph, NodeType, QuestionKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as JsonValue};

/// The four frozen DomainRelationship cardinalities.
pub const CARDINALITY_VALUES: [&str; 4] = ["0..1", "1", "0..*", "1..*"];

/// How a template matches a finding's semantic condition key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionMatch {
    /// The key starts with this prefix; the rest is the candidate ID.
    Prefix(&'static str),
    /// The key equals this condition exactly.
    Exact(&'static str),
}

/// What one matched finding expands into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expansion {
    /// One Question about the candidate named by the condition suffix.
    Candidate,
    /// One Question per affected Accepted node of this type.
    PerTarget(NodeType),
}

/// The answer choices a template offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceSpec {
    /// No choice list (the answer is a closed vocabulary or text).
    None,
    /// One choice list of the Accepted nodes of these types (slot `choice_refs`).
    Single(&'static [NodeType]),
    /// Accepted Operations and Accepted ResourceScopes (slots `operation_refs`,
    /// `resource_scope_refs`).
    OperationAndScope,
}

/// One closed template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestionTemplate {
    pub template_id: &'static str,
    pub code: &'static str,
    pub condition: ConditionMatch,
    pub kind: QuestionKind,
    pub routing_key: &'static str,
    pub expansion: Expansion,
    pub choices: ChoiceSpec,
}

pub const CARDINALITY_TEMPLATE: &str = "f2.domain.relationship.cardinality.v1";
pub const TRIGGER_TEMPLATE: &str = "f2.state.transition.trigger.v1";
pub const PERFORMER_TEMPLATE: &str = "f2.operation.performer.v1";
pub const CALENDAR_TEMPLATE: &str = "f2.time.calendar.v1";
pub const PERMISSION_TEMPLATE: &str = "f2.permission.concrete.v1";
pub const INVARIANT_TEMPLATE: &str = "f2.invariant.formula.v1";

/// The six templates, in template ID order.
pub const QUESTION_TEMPLATES: [QuestionTemplate; 6] = [
    QuestionTemplate {
        template_id: CARDINALITY_TEMPLATE,
        code: "PLUMB.F2.DOMAIN.RELATION_TYPED",
        condition: ConditionMatch::Prefix("domain_relationship_cardinality_unresolved:"),
        kind: QuestionKind::Cardinality,
        routing_key: "default",
        expansion: Expansion::Candidate,
        choices: ChoiceSpec::None,
    },
    QuestionTemplate {
        template_id: INVARIANT_TEMPLATE,
        code: "PLUMB.F2.INVARIANT.EXPRESSIBLE",
        condition: ConditionMatch::Exact("invariant_not_expressible"),
        kind: QuestionKind::FormulaConfirm,
        routing_key: "formula",
        expansion: Expansion::PerTarget(NodeType::Invariant),
        choices: ChoiceSpec::None,
    },
    QuestionTemplate {
        template_id: PERFORMER_TEMPLATE,
        code: "PLUMB.F2.OPERATION.PERFORMER",
        condition: ConditionMatch::Exact("operation_performer_missing"),
        kind: QuestionKind::RoleAssignment,
        routing_key: "default",
        expansion: Expansion::PerTarget(NodeType::Operation),
        choices: ChoiceSpec::Single(&[NodeType::Actor, NodeType::BusinessRole]),
    },
    QuestionTemplate {
        template_id: PERMISSION_TEMPLATE,
        code: "RBAC.F2.PERMISSION.CONCRETE",
        condition: ConditionMatch::Exact("permission_not_concrete"),
        kind: QuestionKind::Permission,
        routing_key: "authorization",
        expansion: Expansion::PerTarget(NodeType::Permission),
        choices: ChoiceSpec::OperationAndScope,
    },
    QuestionTemplate {
        template_id: TRIGGER_TEMPLATE,
        code: "PLUMB.F2.STATE.TRANSITION_COMPLETE",
        condition: ConditionMatch::Prefix("state_transition_trigger_unresolved:"),
        kind: QuestionKind::PickOne,
        routing_key: "default",
        expansion: Expansion::Candidate,
        choices: ChoiceSpec::Single(&[NodeType::Event, NodeType::Operation]),
    },
    QuestionTemplate {
        template_id: CALENDAR_TEMPLATE,
        code: "PLUMB.F2.TIME.CALENDAR_DEFINED",
        condition: ConditionMatch::Exact("business_time_calendar_missing"),
        kind: QuestionKind::Calendar,
        routing_key: "calendar",
        expansion: Expansion::PerTarget(NodeType::Calculation),
        choices: ChoiceSpec::Single(&[NodeType::Calendar]),
    },
];

/// Why a finding is not mapped to a Question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnmappedReason {
    /// The finding code has no question template.
    NoQuestionTemplate,
    /// The code has a template, but this aggregate condition does not say what to decide.
    ConditionNotQuestionResolvable,
}

/// The template matching a finding code and condition key, or why there is none.
pub fn match_template(
    code: &str,
    condition: &str,
) -> Result<&'static QuestionTemplate, UnmappedReason> {
    let mut known_code = false;
    for template in &QUESTION_TEMPLATES {
        if template.code != code {
            continue;
        }
        known_code = true;
        let matched = match template.condition {
            ConditionMatch::Prefix(prefix) => condition.starts_with(prefix),
            ConditionMatch::Exact(exact) => condition == exact,
        };
        if matched {
            return Ok(template);
        }
    }
    Err(if known_code {
        UnmappedReason::ConditionNotQuestionResolvable
    } else {
        UnmappedReason::NoQuestionTemplate
    })
}

/// The sorted IDs of the Accepted nodes of `types`.
pub fn accepted_choices(graph: &Graph, types: &[NodeType]) -> Vec<Id> {
    let mut ids: Vec<Id> = types
        .iter()
        .flat_map(|t| graph.node_ids_by_type(*t))
        .filter(|id| {
            graph
                .node(id)
                .is_some_and(|n| n.status == ElementStatus::Accepted)
        })
        .cloned()
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The concrete answer choices of one template instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choices {
    None,
    Single(Vec<Id>),
    OperationAndScope {
        operation_refs: Vec<Id>,
        resource_scope_refs: Vec<Id>,
    },
}

impl Choices {
    /// The choices of `spec` over the current Accepted graph.
    pub fn of(graph: &Graph, spec: ChoiceSpec) -> Choices {
        match spec {
            ChoiceSpec::None => Choices::None,
            ChoiceSpec::Single(types) => Choices::Single(accepted_choices(graph, types)),
            ChoiceSpec::OperationAndScope => Choices::OperationAndScope {
                operation_refs: accepted_choices(graph, &[NodeType::Operation]),
                resource_scope_refs: accepted_choices(graph, &[NodeType::ResourceScope]),
            },
        }
    }

    /// Whether a required choice list is empty.
    pub fn is_unanswerable(&self) -> bool {
        match self {
            Choices::None => false,
            Choices::Single(ids) => ids.is_empty(),
            Choices::OperationAndScope {
                operation_refs,
                resource_scope_refs,
            } => operation_refs.is_empty() || resource_scope_refs.is_empty(),
        }
    }
}

fn id_array(ids: &[Id]) -> JsonValue {
    JsonValue::Array(
        ids.iter()
            .map(|i| JsonValue::String(i.to_string()))
            .collect(),
    )
}

fn enum_property(ids: &[Id]) -> JsonValue {
    json!({"type": "string", "enum": id_array(ids)})
}

/// A closed object schema with every property required.
fn object_schema(properties: Vec<(&str, JsonValue)>) -> JsonValue {
    let required: Vec<JsonValue> = properties
        .iter()
        .map(|(name, _)| JsonValue::String((*name).to_owned()))
        .collect();
    let properties: Map<String, JsonValue> = properties
        .into_iter()
        .map(|(name, schema)| (name.to_owned(), schema))
        .collect();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties,
    })
}

impl QuestionTemplate {
    /// The subject of the prompt: the candidate or target ID.
    pub fn prompt(&self, subject: &Id) -> String {
        match self.template_id {
            CARDINALITY_TEMPLATE => format!(
                "What are the endpoint cardinalities for relationship candidate {subject}?"
            ),
            TRIGGER_TEMPLATE => format!(
                "Which accepted operation or event triggers transition candidate {subject}?"
            ),
            PERFORMER_TEMPLATE => format!("Who performs operation {subject}?"),
            CALENDAR_TEMPLATE => format!(
                "Which accepted calendar should calculation {subject} use for business-time evaluation?"
            ),
            PERMISSION_TEMPLATE => format!(
                "Which accepted operation and resource scope should permission {subject} reference?"
            ),
            INVARIANT_TEMPLATE => {
                format!("What boolean PlumbExpr predicate should invariant {subject} use?")
            }
            other => unreachable!("closed template registry has no {other}"),
        }
    }

    /// The exact closed answer schema.
    pub fn answer_schema(&self, choices: &Choices) -> JsonValue {
        let single = |name: &str| match choices {
            Choices::Single(ids) => object_schema(vec![(name, enum_property(ids))]),
            _ => unreachable!("{} has a single choice list", self.template_id),
        };
        match self.template_id {
            CARDINALITY_TEMPLATE => {
                let values: Vec<JsonValue> = CARDINALITY_VALUES
                    .iter()
                    .map(|v| JsonValue::String((*v).to_owned()))
                    .collect();
                let property = json!({"type": "string", "enum": values});
                object_schema(vec![
                    ("cardinality_from", property.clone()),
                    ("cardinality_to", property),
                ])
            }
            TRIGGER_TEMPLATE => single("trigger_ref"),
            PERFORMER_TEMPLATE => single("performer_ref"),
            CALENDAR_TEMPLATE => single("calendar_ref"),
            PERMISSION_TEMPLATE => match choices {
                Choices::OperationAndScope {
                    operation_refs,
                    resource_scope_refs,
                } => object_schema(vec![
                    ("operation_ref", enum_property(operation_refs)),
                    ("resource_scope_ref", enum_property(resource_scope_refs)),
                ]),
                _ => unreachable!("permission has operation and scope choices"),
            },
            INVARIANT_TEMPLATE => object_schema(vec![(
                "expression",
                json!({"type": "string", "minLength": 1}),
            )]),
            other => unreachable!("closed template registry has no {other}"),
        }
    }

    /// The template slots before priority and routing: `finding_ref`, `affected_refs`, the
    /// candidate or target, and the exact choice arrays.
    pub fn base_slots(
        &self,
        finding_ref: &Id,
        affected_refs: &[Id],
        subject: &Id,
        choices: &Choices,
    ) -> Map<String, JsonValue> {
        let mut slots = Map::new();
        slots.insert(
            "finding_ref".into(),
            JsonValue::String(finding_ref.to_string()),
        );
        slots.insert("affected_refs".into(), id_array(affected_refs));
        let subject_slot = match self.expansion {
            Expansion::Candidate => "candidate_ref",
            Expansion::PerTarget(_) => "target_ref",
        };
        slots.insert(subject_slot.into(), JsonValue::String(subject.to_string()));
        match choices {
            Choices::None => {}
            Choices::Single(ids) => {
                slots.insert("choice_refs".into(), id_array(ids));
            }
            Choices::OperationAndScope {
                operation_refs,
                resource_scope_refs,
            } => {
                slots.insert("operation_refs".into(), id_array(operation_refs));
                slots.insert("resource_scope_refs".into(), id_array(resource_scope_refs));
            }
        }
        slots
    }
}
