//! The configured semantic directory (R0.6, architecture §2 "Configured semantic directory").
//!
//! Stakeholder, Actor, BusinessRole, Calendar, Rule and DecisionTable material that no compiler
//! can extract is authored explicitly in configuration as a typed [`DirectoryDraft`] with
//! explicit IDs. [`produce_directory`] turns each draft element into one `Proposed` node in its
//! own `Semantic` / `HUMAN_CONFIRM` proposal; nothing is committed, accepted or inferred here.
//!
//! - Node evidence is the sorted union of the evidence of the `Accepted` Requirements a draft
//!   element names as grounding, and is empty when it names none.
//! - A Calendar draft names a configured runtime `calendar_ref`; its `week_pattern` is exactly
//!   `{"weekend": <the runtime weekend weekday names in configured order>}` from the supplied
//!   [`DirectoryCalendarBinding`], so the PSG Calendar and the runtime provider cannot disagree.
//! - A DecisionTable draft is a typed [`DecisionTableSpec`] encoded only through
//!   [`encode_decision_table`].
//! - The proposal stage is the stage whose gate is the element type's earliest gate
//!   (architecture §2 stage table: S1 -> F1, S2 -> F2; [`earliest_gate`]).
//! - Event and State dispositions (H0.7, architecture §33.7) are not elements: each changed
//!   disposition of an existing Proposed or Accepted Event/State is one Semantic /
//!   HUMAN_CONFIRM `SetNodeExtension` proposal of the `plumb_functional:event_disposition` /
//!   `plumb_functional:state_disposition` extension (wire form owned by
//!   `plumb_validation::disposition`), outcomes after the elements in target-ID order.
//! - Outcome selection drafts (H0.4, architecture §33.4) group by Outcome into one
//!   [`OUTCOME_SELECTION_EXTENSION`] value set on the existing Accepted Outcome by its own
//!   `SetNodeExtension` proposal.
//!
//! No clock, configuration file or global is read: the caller supplies the draft, the calendar
//! bindings and the audit.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{CoreError, GateId, Id, StageId, Timestamp};
use plumb_patch::{
    apply_patch, earliest_gate, AcceptancePolicy, ElementPrecondition, PatchSet, Proposal,
    ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    node_element_hash, Actor, AuditMeta, BusinessRole, Calendar, ElementStatus, EvidenceRef,
    ExtensionKey, Graph, Node, NodePayload, NodeType, Rule, Stakeholder,
};
use plumb_validation::disposition::{
    TerminalPurpose, DISPOSITION_VERSION, EVENT_DISPOSITION_EXTENSION, STATE_DISPOSITION_EXTENSION,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::decision_table::{encode_decision_table, DecisionTableSpec};
use crate::outcome_selection::{
    BusinessFailureKind, OutcomeSelection, OutcomeSelectionEntry, OUTCOME_SELECTION_EXTENSION,
    OUTCOME_SELECTION_VERSION,
};

// ============================================================================ draft

/// One directory element: its explicit PSG ID, its complete typed payload and the optional
/// Accepted Requirements that ground it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryElement<T> {
    pub id: Id,
    pub payload: T,
    /// Accepted Requirement IDs whose evidence grounds the element; absent means none.
    #[serde(default)]
    pub grounding_refs: Vec<Id>,
}

/// A Calendar draft: the PSG element ID, the configured runtime calendar it describes and the
/// calendar facts the runtime does not carry. `region` and `holiday_source` are required keys
/// (each may be `null`); nothing is defaulted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryCalendarDraft {
    pub id: Id,
    pub calendar_ref: Id,
    pub time_zone: String,
    #[serde(deserialize_with = "present")]
    pub region: Option<String>,
    #[serde(deserialize_with = "present")]
    pub holiday_source: Option<String>,
    #[serde(default)]
    pub grounding_refs: Vec<Id>,
}

/// A DecisionTable draft: the PSG element ID, the table name and its typed specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryDecisionTableDraft {
    pub id: Id,
    pub name: String,
    pub spec: DecisionTableSpec,
    #[serde(default)]
    pub grounding_refs: Vec<Id>,
}

/// A governed Event disposition (H0.7, architecture §33.7): written to the existing Event as
/// the `plumb_functional:event_disposition` extension. `terminal_purpose` is a required key
/// (it may be `null`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryEventDisposition {
    pub event_ref: Id,
    pub external_source: bool,
    #[serde(deserialize_with = "present_purpose")]
    pub terminal_purpose: Option<TerminalPurpose>,
}

/// A governed State disposition (H0.7): written to the existing State as the
/// `plumb_functional:state_disposition` extension; it declares a state machine's initial
/// state(s) and the terminal or externally-entered states for STATE.REACHABILITY.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryStateDisposition {
    pub state_ref: Id,
    pub initial: bool,
    pub terminal: bool,
    pub externally_entered: bool,
}

/// One outcome selection draft (H0.4): the Accepted Outcome, the Accepted Operation and exactly
/// one of a `when` PlumbExpr source (success / partial Outcomes) or the business failure kinds
/// it is selected on (business_failure Outcomes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryOutcomeSelectionDraft {
    pub outcome_ref: Id,
    pub operation_ref: Id,
    #[serde(default)]
    pub when: Option<String>,
    #[serde(default)]
    pub on_failures: Option<Vec<BusinessFailureKind>>,
}

/// The typed directory draft. The six element families are required keys (possibly empty);
/// the H0.7 disposition families and the H0.4 outcome selection family default to empty so
/// existing directory configurations stay valid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryDraft {
    pub stakeholders: Vec<DirectoryElement<Stakeholder>>,
    pub actors: Vec<DirectoryElement<Actor>>,
    pub business_roles: Vec<DirectoryElement<BusinessRole>>,
    pub calendars: Vec<DirectoryCalendarDraft>,
    pub rules: Vec<DirectoryElement<Rule>>,
    pub decision_tables: Vec<DirectoryDecisionTableDraft>,
    #[serde(default)]
    pub event_dispositions: Vec<DirectoryEventDisposition>,
    #[serde(default)]
    pub state_dispositions: Vec<DirectoryStateDisposition>,
    #[serde(default)]
    pub outcome_selections: Vec<DirectoryOutcomeSelectionDraft>,
}

/// A required key whose value may be `null` (a missing key is an error, never `None`).
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}

fn present_purpose<'de, D: Deserializer<'de>>(d: D) -> Result<Option<TerminalPurpose>, D::Error> {
    Option::<TerminalPurpose>::deserialize(d)
}

/// The runtime weekend of one configured calendar, in configured order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCalendarBinding {
    pub calendar_ref: Id,
    pub weekend: Vec<String>,
}

/// The caller-supplied audit of every produced node; outside every identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

// ============================================================================ results

/// What happened to one draft element.
#[derive(Debug, Clone, PartialEq)]
pub enum DirectoryDisposition {
    /// A new Proposed node in its own Semantic / HUMAN_CONFIRM proposal.
    Proposed { proposal: Box<Proposal> },
    /// The identical node (type, payload and evidence) is already persisted; nothing proposed.
    Existing,
}

/// The outcome of one draft element.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryOutcome {
    pub element_ref: Id,
    pub node_type: NodeType,
    pub disposition: DirectoryDisposition,
}

/// Every element's outcome, in element-ID order, then every outcome selection in Outcome-ID
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryResult {
    pub outcomes: Vec<DirectoryOutcome>,
}

/// Why the directory draft cannot be produced.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DirectoryError {
    #[error("directory element {0} is defined more than once")]
    DuplicateElement(Id),
    #[error("directory element {element} names grounding requirement {requirement} twice")]
    DuplicateGrounding { element: Id, requirement: Id },
    #[error("directory element {element}: grounding {requirement} is not an Accepted Requirement")]
    InvalidGrounding { element: Id, requirement: Id },
    #[error("directory calendar {element}: runtime calendar {calendar_ref} is not configured")]
    UnknownCalendar { element: Id, calendar_ref: Id },
    #[error("directory calendar binding {0} is supplied twice")]
    DuplicateCalendarBinding(Id),
    #[error("directory decision table {element}: {reason}")]
    InvalidDecisionTable { element: Id, reason: String },
    #[error("directory element {element}: {reason}")]
    InvalidElement { element: Id, reason: String },
    #[error("directory element {element} conflicts with the persisted element: {reason}")]
    ElementConflict { element: Id, reason: String },
    #[error("directory disposition of {target}: {reason}")]
    InvalidDisposition { target: Id, reason: String },
    #[error("directory outcome selection of {outcome}: {reason}")]
    InvalidOutcomeSelection { outcome: Id, reason: String },
    #[error("invalid audit: {0}")]
    InvalidAudit(String),
    #[error(transparent)]
    Core(#[from] CoreError),
}

// ============================================================================ validation

/// One planned element before graph binding.
struct Planned {
    id: Id,
    grounding: Vec<Id>,
    payload: Planning,
}

enum Planning {
    Ready(NodePayload),
    Calendar(DirectoryCalendarDraft),
}

impl DirectoryDraft {
    /// The structural invariants that need no graph: unique element IDs across every family,
    /// unique grounding per element and encodable decision tables.
    pub fn validate(&self) -> Result<(), DirectoryError> {
        self.planned()?;
        self.dispositions().map(|_| ())
    }

    /// Every disposition as `(target, node type, extension key, value)`, events then states,
    /// each in target-ID order; a target named twice in one family is a duplicate.
    fn dispositions(&self) -> Result<Vec<(Id, NodeType, &'static str, Value)>, DirectoryError> {
        // The exact wire form of plumb_validation::disposition::{EventDisposition,
        // StateDisposition}.
        let mut events: Vec<(Id, NodeType, &'static str, Value)> = self
            .event_dispositions
            .iter()
            .map(|d| {
                (
                    d.event_ref.clone(),
                    NodeType::Event,
                    EVENT_DISPOSITION_EXTENSION,
                    json!({
                        "version": DISPOSITION_VERSION,
                        "external_source": d.external_source,
                        "terminal_purpose": d.terminal_purpose,
                    }),
                )
            })
            .collect();
        let mut states: Vec<(Id, NodeType, &'static str, Value)> = self
            .state_dispositions
            .iter()
            .map(|d| {
                (
                    d.state_ref.clone(),
                    NodeType::State,
                    STATE_DISPOSITION_EXTENSION,
                    json!({
                        "version": DISPOSITION_VERSION,
                        "initial": d.initial,
                        "terminal": d.terminal,
                        "externally_entered": d.externally_entered,
                    }),
                )
            })
            .collect();
        for family in [&mut events, &mut states] {
            family.sort_by(|a, b| a.0.cmp(&b.0));
            if let Some(pair) = family.windows(2).find(|p| p[0].0 == p[1].0) {
                return Err(DirectoryError::DuplicateElement(pair[0].0.clone()));
            }
        }
        events.extend(states);
        Ok(events)
    }

    fn planned(&self) -> Result<Vec<Planned>, DirectoryError> {
        let mut planned = Vec::new();
        let ready = |id: &Id, grounding: &[Id], payload: NodePayload| Planned {
            id: id.clone(),
            grounding: grounding.to_vec(),
            payload: Planning::Ready(payload),
        };
        for e in &self.stakeholders {
            planned.push(ready(
                &e.id,
                &e.grounding_refs,
                NodePayload::Stakeholder(e.payload.clone()),
            ));
        }
        for e in &self.actors {
            planned.push(ready(
                &e.id,
                &e.grounding_refs,
                NodePayload::Actor(e.payload.clone()),
            ));
        }
        for e in &self.business_roles {
            planned.push(ready(
                &e.id,
                &e.grounding_refs,
                NodePayload::BusinessRole(e.payload.clone()),
            ));
        }
        for c in &self.calendars {
            planned.push(Planned {
                id: c.id.clone(),
                grounding: c.grounding_refs.clone(),
                payload: Planning::Calendar(c.clone()),
            });
        }
        for e in &self.rules {
            planned.push(ready(
                &e.id,
                &e.grounding_refs,
                NodePayload::Rule(e.payload.clone()),
            ));
        }
        for t in &self.decision_tables {
            let table = encode_decision_table(&t.name, &t.spec).map_err(|e| {
                DirectoryError::InvalidDecisionTable {
                    element: t.id.clone(),
                    reason: e.to_string(),
                }
            })?;
            planned.push(ready(
                &t.id,
                &t.grounding_refs,
                NodePayload::DecisionTable(table),
            ));
        }
        // Deterministic checks in element-ID order, independent of configuration order.
        planned.sort_by(|a, b| a.id.cmp(&b.id));
        for pair in planned.windows(2) {
            if pair[0].id == pair[1].id {
                return Err(DirectoryError::DuplicateElement(pair[0].id.clone()));
            }
        }
        for p in &planned {
            let mut seen = BTreeSet::new();
            let mut sorted = p.grounding.clone();
            sorted.sort();
            for r in sorted {
                if !seen.insert(r.clone()) {
                    return Err(DirectoryError::DuplicateGrounding {
                        element: p.id.clone(),
                        requirement: r,
                    });
                }
            }
        }
        Ok(planned)
    }
}

/// The stage of the gate that owns `node_type` (architecture §2: S1 -> F1, S2 -> F2).
fn directory_stage(node_type: NodeType) -> StageId {
    match earliest_gate(node_type) {
        GateId::F1 => StageId::S1,
        _ => StageId::S2,
    }
}

// ============================================================================ producer

/// Produces one `Proposed` node per draft element, each in its own `Semantic` /
/// `HUMAN_CONFIRM` proposal on `graph`, in element-ID order. Grounding must name `Accepted`
/// Requirements; each calendar must name a bound runtime calendar. An element whose ID already
/// holds the identical node is `Existing`; any other persisted element at that ID is a conflict.
pub fn produce_directory(
    graph: &Graph,
    draft: &DirectoryDraft,
    calendars: &[DirectoryCalendarBinding],
    audit: &DirectoryAudit,
) -> Result<DirectoryResult, DirectoryError> {
    let meta = AuditMeta::new(audit.created_by.clone(), audit.created_at, None, None)
        .map_err(|e| DirectoryError::InvalidAudit(e.to_string()))?;
    let mut bindings: BTreeMap<&Id, &DirectoryCalendarBinding> = BTreeMap::new();
    for binding in calendars {
        if bindings.insert(&binding.calendar_ref, binding).is_some() {
            return Err(DirectoryError::DuplicateCalendarBinding(
                binding.calendar_ref.clone(),
            ));
        }
    }
    let base_semantic_hash = graph.semantic_hash()?;
    let mut outcomes = Vec::new();
    for planned in draft.planned()? {
        let payload = match planned.payload {
            Planning::Ready(payload) => payload,
            Planning::Calendar(c) => {
                let binding = bindings.get(&c.calendar_ref).ok_or_else(|| {
                    DirectoryError::UnknownCalendar {
                        element: c.id.clone(),
                        calendar_ref: c.calendar_ref.clone(),
                    }
                })?;
                NodePayload::Calendar(Calendar {
                    time_zone: c.time_zone,
                    week_pattern: json!({ "weekend": binding.weekend }),
                    region: c.region,
                    holiday_source: c.holiday_source,
                })
            }
        };
        // Grounding: Accepted Requirements only; evidence is the sorted unique union.
        let mut evidence: BTreeSet<EvidenceRef> = BTreeSet::new();
        let mut grounding = planned.grounding.clone();
        grounding.sort();
        for requirement in &grounding {
            let node = graph
                .node(requirement)
                .filter(|n| {
                    n.status == ElementStatus::Accepted
                        && n.payload.node_type() == NodeType::Requirement
                })
                .ok_or_else(|| DirectoryError::InvalidGrounding {
                    element: planned.id.clone(),
                    requirement: requirement.clone(),
                })?;
            evidence.extend(node.evidence.iter().cloned());
        }
        let node_type = payload.node_type();
        let node = Node {
            id: planned.id.clone(),
            revision: 1,
            status: ElementStatus::Proposed,
            payload,
            evidence: evidence.iter().cloned().collect(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::new(),
            audit: meta.clone(),
        };
        let invalid = |reason: String| DirectoryError::InvalidElement {
            element: planned.id.clone(),
            reason,
        };
        node.validate().map_err(|e| invalid(e.to_string()))?;
        if let Some(existing) = graph.node(&planned.id) {
            if existing.payload == node.payload && existing.evidence == node.evidence {
                outcomes.push(DirectoryOutcome {
                    element_ref: planned.id,
                    node_type,
                    disposition: DirectoryDisposition::Existing,
                });
                continue;
            }
            return Err(DirectoryError::ElementConflict {
                element: planned.id.clone(),
                reason: format!(
                    "a different {} is persisted at this ID",
                    existing.payload.node_type().as_str()
                ),
            });
        }
        if graph.edge(&planned.id).is_some() {
            return Err(DirectoryError::ElementConflict {
                element: planned.id.clone(),
                reason: "an edge is persisted at this ID".into(),
            });
        }
        let proposal = Proposal::new(
            directory_stage(node_type),
            PatchSet {
                base_semantic_hash: base_semantic_hash.clone(),
                patch: SemanticPatch::AddNode { node },
            },
            evidence.into_iter().collect(),
            Vec::new(),
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .map_err(|e| invalid(e.to_string()))?;
        // In-memory dry validation by the canonical engine; the candidate graph is discarded.
        apply_patch(graph, &proposal.patch_set)
            .map_err(|e| invalid(format!("proposal does not apply: {e}")))?;
        outcomes.push(DirectoryOutcome {
            element_ref: planned.id,
            node_type,
            disposition: DirectoryDisposition::Proposed {
                proposal: Box::new(proposal),
            },
        });
    }
    // H0.7 dispositions: one SetNodeExtension proposal per changed disposition of an existing
    // Proposed or Accepted node of the right type; an equal value is Existing.
    for (target, node_type, key, value) in draft.dispositions()? {
        let invalid = |reason: String| DirectoryError::InvalidDisposition {
            target: target.clone(),
            reason,
        };
        let node = graph
            .node(&target)
            .filter(|n| {
                n.payload.node_type() == node_type
                    && matches!(n.status, ElementStatus::Proposed | ElementStatus::Accepted)
            })
            .ok_or_else(|| {
                invalid(format!(
                    "no Proposed or Accepted {} has this ID",
                    node_type.as_str()
                ))
            })?;
        let key: ExtensionKey = key.parse().map_err(|_| invalid("invalid key".into()))?;
        if node.extensions.get(&key) == Some(&value) {
            outcomes.push(DirectoryOutcome {
                element_ref: target,
                node_type,
                disposition: DirectoryDisposition::Existing,
            });
            continue;
        }
        let proposal = Proposal::new(
            directory_stage(node_type),
            PatchSet {
                base_semantic_hash: base_semantic_hash.clone(),
                patch: SemanticPatch::SetNodeExtension {
                    target: ElementPrecondition {
                        id: target.clone(),
                        expected_hash: node_element_hash(node)?,
                    },
                    key,
                    value,
                },
            },
            node.evidence.clone(),
            Vec::new(),
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .map_err(|e| invalid(e.to_string()))?;
        apply_patch(graph, &proposal.patch_set)
            .map_err(|e| invalid(format!("proposal does not apply: {e}")))?;
        outcomes.push(DirectoryOutcome {
            element_ref: target,
            node_type,
            disposition: DirectoryDisposition::Proposed {
                proposal: Box::new(proposal),
            },
        });
    }
    outcomes.extend(produce_outcome_selections(
        graph,
        &draft.outcome_selections,
        &base_semantic_hash,
    )?);
    Ok(DirectoryResult { outcomes })
}

// ============================================================================ outcome selections

/// One `SetNodeExtension` proposal per Outcome named by the drafts, in Outcome-ID order. The
/// drafts of one Outcome form its complete selection value; an Outcome already holding exactly
/// that value is `Existing`.
fn produce_outcome_selections(
    graph: &Graph,
    drafts: &[DirectoryOutcomeSelectionDraft],
    base_semantic_hash: &plumb_core::Hash,
) -> Result<Vec<DirectoryOutcome>, DirectoryError> {
    let mut by_outcome: BTreeMap<&Id, Vec<OutcomeSelectionEntry>> = BTreeMap::new();
    for d in drafts {
        let invalid = |reason: String| DirectoryError::InvalidOutcomeSelection {
            outcome: d.outcome_ref.clone(),
            reason,
        };
        let on_failures = match &d.on_failures {
            Some(kinds) => {
                let unique: BTreeSet<BusinessFailureKind> = kinds.iter().copied().collect();
                if unique.len() != kinds.len() {
                    return Err(invalid(format!(
                        "on_failures of {} lists a failure kind twice",
                        d.operation_ref
                    )));
                }
                Some(unique.into_iter().collect())
            }
            None => None,
        };
        let entries = by_outcome.entry(&d.outcome_ref).or_default();
        if entries.iter().any(|e| e.operation_ref == d.operation_ref) {
            return Err(invalid(format!(
                "{} is selected more than once",
                d.operation_ref
            )));
        }
        entries.push(OutcomeSelectionEntry {
            operation_ref: d.operation_ref.clone(),
            when: d.when.clone(),
            on_failures,
        });
    }
    let key: plumb_psg::ExtensionKey =
        OUTCOME_SELECTION_EXTENSION
            .parse()
            .map_err(
                |e: plumb_psg::InvalidExtensionKey| DirectoryError::InvalidElement {
                    element: graph.project_id().clone(),
                    reason: e.to_string(),
                },
            )?;
    let mut outcomes = Vec::new();
    for (outcome_ref, mut selections) in by_outcome {
        let invalid = |reason: String| DirectoryError::InvalidOutcomeSelection {
            outcome: outcome_ref.clone(),
            reason,
        };
        selections.sort_by(|a, b| a.operation_ref.cmp(&b.operation_ref));
        let selection = OutcomeSelection {
            version: OUTCOME_SELECTION_VERSION,
            selections,
        };
        let node = graph
            .node(outcome_ref)
            .filter(|n| {
                n.status == ElementStatus::Accepted && n.payload.node_type() == NodeType::Outcome
            })
            .ok_or_else(|| invalid("the target is not an Accepted Outcome".into()))?;
        selection
            .validate(graph, outcome_ref)
            .map_err(|e| invalid(e.to_string()))?;
        let value = selection.to_value();
        if node.extensions.get(&key) == Some(&value) {
            outcomes.push(DirectoryOutcome {
                element_ref: outcome_ref.clone(),
                node_type: NodeType::Outcome,
                disposition: DirectoryDisposition::Existing,
            });
            continue;
        }
        let proposal = Proposal::new(
            directory_stage(NodeType::Outcome),
            PatchSet {
                base_semantic_hash: base_semantic_hash.clone(),
                patch: SemanticPatch::SetNodeExtension {
                    target: plumb_patch::ElementPrecondition {
                        id: outcome_ref.clone(),
                        expected_hash: plumb_psg::node_element_hash(node)?,
                    },
                    key: key.clone(),
                    value,
                },
            },
            Vec::new(),
            Vec::new(),
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .map_err(|e| invalid(e.to_string()))?;
        apply_patch(graph, &proposal.patch_set)
            .map_err(|e| invalid(format!("proposal does not apply: {e}")))?;
        outcomes.push(DirectoryOutcome {
            element_ref: outcome_ref.clone(),
            node_type: NodeType::Outcome,
            disposition: DirectoryDisposition::Proposed {
                proposal: Box::new(proposal),
            },
        });
    }
    Ok(outcomes)
}
