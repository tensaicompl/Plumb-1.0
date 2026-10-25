//! B1 Inbox projections (UI-SPEC §5.2): the human-readable view of one pending workflow
//! proposal — its title, element kind, facts, evidence passages and model change.
//!
//! Everything here is a pure, deterministic function of the stored Proposal and the graphs it is
//! judged against: no model, clock, store or environment is read, and nothing is persisted.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use plumb_patch::{apply_patch, GraphDelta, Proposal, SemanticPatch};
use plumb_psg::{
    contributes_to_semantic_hash, is_proposal_acceptance_companion, Edge, ElementStatus, Graph,
    Node, NodePayload, NodeType, RelationKind,
};
use plumb_store::{LoadedRevision, RevisionId, WorkflowOrigin, WorkflowProposalState};
use serde_json::Value;

use crate::application::{is_discardable, not_pending, workflow_error, PendingProposalView};
use crate::error::{echo, ApiError};
use crate::openapi::{
    DiffItem, ElementLabel, PendingProposal, PendingProposalDetail, ProposalElementKind as Kind,
    ProposalEvidence, ProposalFact, ProposalKind, ProposalModelDiff, ProposalOrigin,
};
use crate::state::{parse_id, parse_wire, ApiState};
use crate::workflow::{decode_proposal, StoredProposal};

// ============================================================================ patch material

/// The leaves of `patch` in patch order.
pub(crate) fn leaves(patch: &SemanticPatch) -> Vec<&SemanticPatch> {
    fn walk<'a>(patch: &'a SemanticPatch, out: &mut Vec<&'a SemanticPatch>) {
        match patch {
            SemanticPatch::Compound { patches } => patches.iter().for_each(|p| walk(p, out)),
            leaf => out.push(leaf),
        }
    }
    let mut out = Vec::new();
    walk(patch, &mut out);
    out
}

/// The nodes and edges `proposal` adds, in patch order.
pub(crate) fn added_elements(proposal: &Proposal) -> (Vec<&Node>, Vec<&Edge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for leaf in leaves(&proposal.patch_set.patch) {
        match leaf {
            SemanticPatch::AddNode { node } => nodes.push(node),
            SemanticPatch::AddEdge { edge } => edges.push(edge),
            _ => {}
        }
    }
    (nodes, edges)
}

/// Whether a node is model content (not governance, provenance, workflow or proof material).
fn is_model_node(node: &Node) -> bool {
    let t = node.payload.node_type();
    t != NodeType::ResolutionDecision
        && contributes_to_semantic_hash(t)
        && !is_proposal_acceptance_companion(node)
}

/// Whether an edge is model content in `graph`: not `resolves`, between two model nodes.
fn is_model_edge(graph: &Graph, edge: &Edge) -> bool {
    edge.kind != RelationKind::Resolves
        && [&edge.from, &edge.to]
            .iter()
            .all(|end| graph.node(end).is_some_and(is_model_node))
}

// ============================================================================ names

/// Name lookup: the nodes the proposal adds, then the candidate graph, then the base graph.
pub(crate) struct Names<'a> {
    added: BTreeMap<&'a Id, &'a Node>,
    graphs: Vec<&'a Graph>,
}

impl<'a> Names<'a> {
    pub(crate) fn new(proposal: &'a Proposal, graphs: Vec<&'a Graph>) -> Names<'a> {
        let added = added_elements(proposal)
            .0
            .into_iter()
            .map(|n| (&n.id, n))
            .collect();
        Names { added, graphs }
    }

    fn node(&self, id: &Id) -> Option<&'a Node> {
        self.added
            .get(id)
            .copied()
            .or_else(|| self.graphs.iter().find_map(|g| g.node(id)))
    }

    fn edge(&self, id: &Id) -> Option<&'a Edge> {
        self.graphs.iter().find_map(|g| g.edge(id))
    }

    /// The plain-language label of an element ID (its ID when unknown).
    fn label(&self, id: &Id) -> String {
        match self.node(id) {
            Some(node) => self.node_label(node),
            None => match self.edge(id) {
                Some(edge) => self.edge_label(edge),
                None => id.to_string(),
            },
        }
    }

    fn node_label(&self, node: &Node) -> String {
        match &node.payload {
            NodePayload::Requirement(r) => r
                .source_identifier
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| r.statement.clone()),
            NodePayload::Term(t) => t.term.clone(),
            NodePayload::Transition(t) => {
                format!(
                    "{} → {}",
                    self.label(&t.from_state),
                    self.label(&t.to_state)
                )
            }
            NodePayload::Question(q) => q.prompt.clone(),
            payload => {
                let data = payload_data(payload);
                ["name", "statement", "title", "prompt"]
                    .iter()
                    .find_map(|k| data.get(*k).and_then(Value::as_str))
                    .filter(|s| !s.trim().is_empty())
                    .map(humanize)
                    .unwrap_or_else(|| node.id.to_string())
            }
        }
    }

    fn edge_label(&self, edge: &Edge) -> String {
        format!(
            "{} {} {}",
            self.label(&edge.from),
            relation_words(&edge.kind),
            self.label(&edge.to)
        )
    }
}

/// The plain-language labels of the `ids` present in `graph` (S4 `resolve_labels`), each once,
/// sorted by ID: a node's name and lower snake_case type, or a relation's words and kind.
pub(crate) fn element_labels(graph: &Graph, ids: &[Id]) -> Vec<ElementLabel> {
    let names = Names {
        added: BTreeMap::new(),
        graphs: vec![graph],
    };
    ids.iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| {
            if let Some(node) = graph.node(id) {
                return Some(ElementLabel {
                    id: id.to_string(),
                    kind: type_snake(node.payload.node_type()),
                    label: names.node_label(node),
                });
            }
            graph.edge(id).map(|edge| ElementLabel {
                id: id.to_string(),
                kind: edge.kind.as_str().to_owned(),
                label: names.edge_label(edge),
            })
        })
        .collect()
}

/// The `data` object of a payload (`{"type", "data"}` on the wire).
fn payload_data(payload: &NodePayload) -> Value {
    serde_json::to_value(payload)
        .ok()
        .and_then(|v| v.get("data").cloned())
        .unwrap_or(Value::Null)
}

/// The words of a relation kind: `performed_by` -> "performed by".
fn relation_words(kind: &RelationKind) -> String {
    if kind.is_core() {
        kind.as_str().replace('_', " ")
    } else {
        kind.as_str().to_owned()
    }
}

/// Splits an identifier-like name into words ("LeaveRequest" -> "Leave request",
/// "double_days" -> "double days"); a name that already has spaces is kept as it is.
fn humanize(name: &str) -> String {
    if name.chars().any(char::is_whitespace) || name.contains(':') {
        return name.to_owned();
    }
    let chars: Vec<char> = name.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '-' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let boundary = c.is_uppercase()
            && i > 0
            && (chars[i - 1].is_lowercase()
                || chars[i - 1].is_ascii_digit()
                || (chars[i - 1].is_uppercase()
                    && chars.get(i + 1).is_some_and(|n| n.is_lowercase())));
        if boundary && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() {
        words.push(current);
    }
    if words.is_empty() {
        return name.to_owned();
    }
    words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let acronym = w.chars().count() > 1 && w.chars().all(|c| !c.is_lowercase());
            if i == 0 || acronym {
                w.clone()
            } else {
                w.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The lower words of a node type: `DecisionTable` -> "decision table".
fn type_words(t: NodeType) -> String {
    let mut out = String::new();
    for (i, c) in t.as_str().chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// The lower snake_case of a node type: `DecisionTable` -> "decision_table".
fn type_snake(t: NodeType) -> String {
    type_words(t).replace(' ', "_")
}

/// The Inbox kind of a node type.
fn node_kind(t: NodeType) -> Kind {
    match t {
        NodeType::Operation => Kind::Operation,
        NodeType::Event => Kind::Event,
        NodeType::Outcome => Kind::Outcome,
        NodeType::Requirement => Kind::Requirement,
        NodeType::Term => Kind::GlossaryTerm,
        NodeType::Entity => Kind::Entity,
        NodeType::Attribute => Kind::Attribute,
        NodeType::State => Kind::State,
        NodeType::Transition => Kind::Transition,
        NodeType::Invariant => Kind::Invariant,
        NodeType::Calculation => Kind::Calculation,
        NodeType::Scenario => Kind::Scenario,
        NodeType::Rule => Kind::Rule,
        NodeType::DecisionTable => Kind::DecisionTable,
        NodeType::Process | NodeType::ProcessNode => Kind::Process,
        NodeType::Stakeholder | NodeType::Actor | NodeType::BusinessRole | NodeType::Calendar => {
            Kind::DirectoryElement
        }
        _ => Kind::Other,
    }
}

/// Which added node a create proposal is about: the first of the most specific type.
const PRIMARY_ORDER: [NodeType; 21] = [
    NodeType::Operation,
    NodeType::Process,
    NodeType::Calculation,
    NodeType::DecisionTable,
    NodeType::Rule,
    NodeType::Scenario,
    NodeType::Entity,
    NodeType::Requirement,
    NodeType::Term,
    NodeType::Event,
    NodeType::Outcome,
    NodeType::State,
    NodeType::Transition,
    NodeType::Attribute,
    NodeType::Invariant,
    NodeType::Actor,
    NodeType::BusinessRole,
    NodeType::Stakeholder,
    NodeType::Calendar,
    NodeType::Concept,
    NodeType::ProcessNode,
];

fn rank(t: NodeType) -> usize {
    PRIMARY_ORDER
        .iter()
        .position(|p| *p == t)
        .unwrap_or(PRIMARY_ORDER.len())
}

// ============================================================================ description

/// What a proposal is about: its subject and the shape of its change.
enum Subject<'a> {
    Answer {
        prompt: Option<String>,
        answer: Value,
    },
    Merge {
        keep: Id,
        merged: Vec<Id>,
    },
    Supersede {
        old: Id,
        new: Id,
    },
    Link {
        edge: &'a Edge,
    },
    Add {
        node: &'a Node,
        more: usize,
    },
    Change {
        verb: &'static str,
        target: Id,
    },
    Other,
}

/// The question-answering ResolutionDecision a proposal adds, if any.
fn answer_decision(proposal: &Proposal) -> Option<&plumb_psg::ResolutionDecision> {
    added_elements(proposal)
        .0
        .into_iter()
        .find_map(|n| match &n.payload {
            NodePayload::ResolutionDecision(d) if d.question_ref.is_some() => Some(d),
            _ => None,
        })
}

fn subject<'a>(names: &Names<'a>, proposal: &'a Proposal, origin: WorkflowOrigin) -> Subject<'a> {
    let leaves = leaves(&proposal.patch_set.patch);
    if origin == WorkflowOrigin::Question {
        let decision = answer_decision(proposal);
        let prompt = decision
            .and_then(|d| d.question_ref.as_ref())
            .and_then(|q| names.node(q))
            .and_then(|n| match &n.payload {
                NodePayload::Question(q) => Some(q.prompt.clone()),
                _ => None,
            });
        return Subject::Answer {
            prompt,
            answer: decision.map(|d| d.answer.clone()).unwrap_or(Value::Null),
        };
    }
    for leaf in &leaves {
        match leaf {
            SemanticPatch::MergeNodes { keep, merge, .. } => {
                return Subject::Merge {
                    keep: keep.id.clone(),
                    merged: merge.iter().map(|m| m.id.clone()).collect(),
                }
            }
            SemanticPatch::Supersede { old, new, .. } => {
                return Subject::Supersede {
                    old: old.id.clone(),
                    new: new.id.clone(),
                }
            }
            _ => {}
        }
    }
    let (nodes, edges) = added_elements(proposal);
    let model_nodes: Vec<&Node> = nodes.into_iter().filter(|n| is_model_node(n)).collect();
    if let Some(edge) = edges
        .iter()
        .find(|e| e.kind == RelationKind::Refines)
        .or_else(|| {
            // Only relations added between existing model elements: a link.
            if model_nodes.is_empty() {
                edges.iter().find(|e| {
                    e.kind != RelationKind::Resolves
                        && [&e.from, &e.to]
                            .iter()
                            .all(|end| names.node(end).is_some_and(is_model_node))
                })
            } else {
                None
            }
        })
    {
        return Subject::Link { edge };
    }
    if let Some(primary) = model_nodes
        .iter()
        .min_by_key(|n| rank(n.payload.node_type()))
        .copied()
    {
        let t = primary.payload.node_type();
        let more = model_nodes
            .iter()
            .filter(|n| n.payload.node_type() == t)
            .count()
            - 1;
        return Subject::Add {
            node: primary,
            more,
        };
    }
    let changed = |id: &Id| names.node(id).is_some_and(is_model_node);
    let change = leaves
        .iter()
        .filter_map(|leaf| match leaf {
            SemanticPatch::SetStatus { target, to, .. } => Some((
                match to {
                    ElementStatus::Accepted => "Accept",
                    ElementStatus::Rejected => "Reject",
                    ElementStatus::Deprecated => "Deprecate",
                    _ => "Change the status of",
                },
                &target.id,
            )),
            SemanticPatch::ReplacePayload { target, .. } => Some(("Change", &target.id)),
            SemanticPatch::SetNodeExtension { target, .. } => Some(("Update", &target.id)),
            SemanticPatch::RemoveNode { target } => Some(("Remove", &target.id)),
            SemanticPatch::AttachEvidence { target, .. } => Some(("Add evidence to", &target.id)),
            SemanticPatch::AttachStandardMapping { target, .. } => {
                Some(("Map a standard to", &target.id))
            }
            SemanticPatch::ReplaceEdge { target, .. } => Some(("Change", &target.id)),
            SemanticPatch::RemoveEdge { target } => Some(("Remove", &target.id)),
            _ => None,
        })
        .min_by_key(|(_, id)| !changed(id));
    match change {
        Some((verb, target)) => Subject::Change {
            verb,
            target: target.clone(),
        },
        None => Subject::Other,
    }
}

/// The title and kind of `proposal` (B1). `graphs` are searched in order for names: the
/// candidate graph (when known) and the graph the proposal was built on.
pub(crate) fn describe(
    proposal: &Proposal,
    origin: WorkflowOrigin,
    graphs: Vec<&Graph>,
) -> (String, Kind) {
    let names = Names::new(proposal, graphs);
    match subject(&names, proposal, origin) {
        Subject::Answer { prompt, .. } => (
            match prompt {
                Some(prompt) => format!("Apply the answer to: {prompt}"),
                None => "Apply an answer".to_owned(),
            },
            Kind::Answer,
        ),
        Subject::Merge { keep, merged } => {
            let what = names
                .node(&keep)
                .map(|n| format!("{} ", type_words(n.payload.node_type())))
                .unwrap_or_default();
            let merged = merged
                .iter()
                .map(|m| format!("'{}'", names.label(m)))
                .collect::<Vec<_>>()
                .join(", ");
            (
                format!("Merge {what}{merged} into '{}'", names.label(&keep)),
                Kind::Merge,
            )
        }
        Subject::Supersede { old, new } => (
            format!("Supersede {} with {}", names.label(&old), names.label(&new)),
            Kind::Supersede,
        ),
        Subject::Link { edge } => (
            format!(
                "Link {} to {}",
                names.label(&edge.from),
                names.label(&edge.to)
            ),
            Kind::Link,
        ),
        Subject::Add { node, more } => {
            let t = node.payload.node_type();
            let tail = if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            };
            (
                format!("Add {} {}{tail}", type_words(t), names.node_label(node)),
                node_kind(t),
            )
        }
        Subject::Change { verb, target } => match names.node(&target) {
            Some(node) => {
                let t = node.payload.node_type();
                (
                    format!("{verb} {} {}", type_words(t), names.node_label(node)),
                    node_kind(t),
                )
            }
            None => (format!("{verb} link {}", names.label(&target)), Kind::Link),
        },
        Subject::Other => ("Change the model".to_owned(), Kind::Other),
    }
}

// ============================================================================ facts

fn fact(label: &str, value: impl Into<String>) -> ProposalFact {
    ProposalFact {
        label: label.to_owned(),
        value: value.into(),
    }
}

/// The key payload fields of a node type, with their plain-language labels.
fn key_fields(t: NodeType) -> &'static [(&'static str, &'static str)] {
    match t {
        NodeType::Requirement => &[
            ("statement", "Statement"),
            ("requirement_kind", "Kind"),
            ("source_identifier", "Source"),
        ],
        NodeType::Term => &[
            ("term", "Term"),
            ("language", "Language"),
            ("aliases", "Aliases"),
        ],
        NodeType::Entity | NodeType::Process => &[("name", "Name"), ("description", "Description")],
        NodeType::Attribute => &[("name", "Name"), ("value_type", "Type"), ("unit", "Unit")],
        NodeType::Transition => &[
            ("stateful_ref", "Of"),
            ("from_state", "From"),
            ("to_state", "To"),
            ("guard_expr", "Guard"),
        ],
        NodeType::Outcome => &[("name", "Name"), ("outcome_kind", "Kind")],
        NodeType::Calculation => &[
            ("name", "Name"),
            ("expression", "Expression"),
            ("result_type", "Result type"),
            ("unit", "Unit"),
        ],
        NodeType::Rule => &[
            ("name", "Name"),
            ("condition_expr", "Condition"),
            ("action_expr", "Action"),
        ],
        NodeType::DecisionTable => &[("name", "Name"), ("hit_policy", "Hit policy")],
        NodeType::Scenario => &[
            ("name", "Name"),
            ("scenario_kind", "Kind"),
            ("confirmation_status", "Confirmation"),
        ],
        NodeType::Actor => &[("name", "Name"), ("actor_kind", "Kind")],
        NodeType::Stakeholder => &[
            ("name", "Name"),
            ("stakeholder_kind", "Kind"),
            ("organization", "Organization"),
        ],
        NodeType::Calendar => &[("time_zone", "Time zone"), ("region", "Region")],
        NodeType::Invariant => &[("name", "Name"), ("expression", "Expression")],
        _ => &[("name", "Name")],
    }
}

/// A payload value in plain words; `None` for null, empty and structured values. A string
/// naming a known element is that element's label.
fn plain(names: &Names, value: &Value) -> Option<String> {
    match value {
        Value::String(s) if s.trim().is_empty() => None,
        Value::String(s) => Some(
            s.parse::<Id>()
                .ok()
                .filter(|id| names.node(id).is_some())
                .map(|id| names.label(&id))
                .unwrap_or_else(|| s.clone()),
        ),
        Value::Bool(b) => Some(if *b { "yes" } else { "no" }.to_owned()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(|v| plain(names, v)).collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        }
        Value::Null | Value::Object(_) => None,
    }
}

/// The key payload fields of `node` in plain words.
fn node_facts(names: &Names, node: &Node) -> Vec<ProposalFact> {
    let data = payload_data(&node.payload);
    key_fields(node.payload.node_type())
        .iter()
        .filter_map(|(field, label)| {
            let value = data.get(*field)?;
            let text = if *field == "name" {
                value.as_str().map(humanize)
            } else {
                plain(names, value)
            }?;
            Some(fact(label, text))
        })
        .collect()
}

/// The operation facts of `operation`, from the relations the proposal adds: performers,
/// reads, writes (with the state a transition via this operation moves the written entity to),
/// produced and consumed events, governing rules and used calculations.
fn operation_facts(names: &Names, proposal: &Proposal, operation: &Id) -> Vec<ProposalFact> {
    const GROUPS: [(RelationKind, &str); 7] = [
        (RelationKind::PerformedBy, "Performed by"),
        (RelationKind::Reads, "Reads"),
        (RelationKind::Writes, "Writes"),
        (RelationKind::Produces, "Produces"),
        (RelationKind::Consumes, "Consumes"),
        (RelationKind::GovernedBy, "Governed by"),
        (RelationKind::UsesCalculation, "Uses calculation"),
    ];
    let (_, edges) = added_elements(proposal);
    // Transitions via this operation (added or existing), by stateful entity.
    let mut via: BTreeMap<Id, BTreeSet<Id>> = BTreeMap::new();
    let transitions = edges
        .iter()
        .copied()
        .chain(names.graphs.iter().flat_map(|g| g.edges().values()))
        .filter(|e| e.kind == RelationKind::TransitionsVia && &e.to == operation);
    for e in transitions {
        if let Some(NodePayload::Transition(t)) = names.node(&e.from).map(|n| &n.payload) {
            via.entry(t.stateful_ref.clone())
                .or_default()
                .insert(t.to_state.clone());
        }
    }
    let mut facts = Vec::new();
    for (kind, label) in GROUPS {
        let mut values: Vec<String> = edges
            .iter()
            .filter(|e| e.kind == kind && &e.from == operation)
            .map(|e| {
                let target = names.label(&e.to);
                match via.get(&e.to).filter(|_| kind == RelationKind::Writes) {
                    Some(states) => {
                        let states: Vec<String> = states.iter().map(|s| names.label(s)).collect();
                        format!("{target} → {}", states.join(", "))
                    }
                    None => target,
                }
            })
            .collect();
        values.sort();
        values.dedup();
        facts.extend(values.into_iter().map(|v| fact(label, v)));
    }
    facts
}

/// The facts of `proposal` (B1).
pub(crate) fn facts(
    proposal: &Proposal,
    origin: WorkflowOrigin,
    graphs: Vec<&Graph>,
) -> Vec<ProposalFact> {
    let names = Names::new(proposal, graphs);
    match subject(&names, proposal, origin) {
        Subject::Answer { prompt, answer } => {
            let mut facts = Vec::new();
            if let Some(prompt) = prompt {
                facts.push(fact("Question", prompt));
            }
            let rendered = match &answer {
                Value::Object(map) => map
                    .iter()
                    .filter_map(|(k, v)| {
                        plain(&names, v)
                            .or_else(|| (!v.is_null()).then(|| v.to_string()))
                            .map(|v| format!("{}: {v}", k.replace('_', " ")))
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
                other => plain(&names, other).unwrap_or_else(|| other.to_string()),
            };
            if !rendered.is_empty() {
                facts.push(fact("Answer", rendered));
            }
            facts
        }
        Subject::Merge { keep, merged } => {
            let merged: Vec<String> = merged.iter().map(|m| names.label(m)).collect();
            vec![
                fact("Keep", names.label(&keep)),
                fact("Merge", merged.join(", ")),
            ]
        }
        Subject::Supersede { old, new } => {
            vec![
                fact("Old", names.label(&old)),
                fact("New", names.label(&new)),
            ]
        }
        Subject::Link { edge } => vec![
            fact("From", names.label(&edge.from)),
            fact("Relation", relation_words(&edge.kind)),
            fact("To", names.label(&edge.to)),
        ],
        Subject::Add { node, .. } if node.payload.node_type() == NodeType::Operation => {
            operation_facts(&names, proposal, &node.id)
        }
        Subject::Add { node, .. } => node_facts(&names, node),
        Subject::Change { target, .. } => match names.node(&target) {
            Some(node) if node.payload.node_type() == NodeType::Operation => {
                operation_facts(&names, proposal, &node.id)
            }
            Some(node) => node_facts(&names, node),
            None => Vec::new(),
        },
        Subject::Other => Vec::new(),
    }
}

// ============================================================================ evidence

/// A grounded byte range of a Requirement statement or an EvidenceFragment text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Grounding {
    Requirement { id: Id, start: u64, end: u64 },
    Fragment { id: Id, start: u64, end: u64 },
}

/// Every `{requirement_ref | fragment_ref, start, end}` object inside `value`.
fn groundings(value: &Value, out: &mut BTreeSet<Grounding>) {
    match value {
        Value::Object(map) => {
            let range = map
                .get("start")
                .and_then(Value::as_u64)
                .zip(map.get("end").and_then(Value::as_u64));
            let id = |key: &str| {
                map.get(key)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<Id>().ok())
            };
            if let Some((start, end)) = range {
                if let Some(id) = id("requirement_ref") {
                    out.insert(Grounding::Requirement { id, start, end });
                } else if let Some(id) = id("fragment_ref") {
                    out.insert(Grounding::Fragment { id, start, end });
                }
            }
            map.values().for_each(|v| groundings(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| groundings(v, out)),
        _ => {}
    }
}

/// The highlight of `[start, end)` in `text` when it is a valid range on char boundaries.
fn highlight(text: &str, start: u64, end: u64) -> (Option<u64>, Option<u64>) {
    match (usize::try_from(start), usize::try_from(end)) {
        (Ok(s), Ok(e))
            if s <= e
                && e <= text.len()
                && text.is_char_boundary(s)
                && text.is_char_boundary(e) =>
        {
            (Some(start), Some(end))
        }
        _ => (None, None),
    }
}

/// The evidence passages of `proposal` (B1): first every grounded byte range its added
/// material records (a Requirement statement range, then an EvidenceFragment range), then every
/// evidence reference of the proposal and its added elements that no passage already shows,
/// highlighted whole. Highlights are UTF-8 byte offsets into `text`.
pub(crate) fn evidence(proposal: &Proposal, graphs: Vec<&Graph>) -> Vec<ProposalEvidence> {
    let names = Names::new(proposal, graphs);
    let mut grounded = BTreeSet::new();
    let mut refs: BTreeSet<Id> = proposal
        .evidence_refs
        .iter()
        .map(|e| e.as_id().clone())
        .collect();
    for leaf in leaves(&proposal.patch_set.patch) {
        match leaf {
            SemanticPatch::AddNode { node } => {
                node.extensions
                    .values()
                    .for_each(|v| groundings(v, &mut grounded));
                refs.extend(node.evidence.iter().map(|e| e.as_id().clone()));
            }
            SemanticPatch::AddEdge { edge } => {
                refs.extend(edge.evidence.iter().map(|e| e.as_id().clone()));
            }
            SemanticPatch::SetNodeExtension { value, .. } => groundings(value, &mut grounded),
            SemanticPatch::AttachEvidence { evidence, .. } => {
                refs.insert(evidence.as_id().clone());
            }
            _ => {}
        }
    }
    let fragment = |id: &Id| match names.node(id).map(|n| &n.payload) {
        Some(NodePayload::EvidenceFragment(f)) => Some(f),
        _ => None,
    };
    // The requirement each fragment grounds (the first by ID with a source identifier).
    let mut owner: BTreeMap<Id, (Id, Option<String>)> = BTreeMap::new();
    for graph in names.graphs.iter().rev() {
        for id in graph.node_ids_by_type(NodeType::Requirement) {
            let Some(node) = graph.node(id) else { continue };
            let NodePayload::Requirement(r) = &node.payload else {
                continue;
            };
            for e in &node.evidence {
                let entry = owner
                    .entry(e.as_id().clone())
                    .or_insert_with(|| (node.id.clone(), r.source_identifier.clone()));
                if entry.1.is_none() && r.source_identifier.is_some() {
                    *entry = (node.id.clone(), r.source_identifier.clone());
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut shown_requirements = BTreeSet::new();
    let mut shown_fragments = BTreeSet::new();
    for g in &grounded {
        match g {
            Grounding::Requirement { id, start, end } => {
                let Some(node) = names.node(id) else { continue };
                let NodePayload::Requirement(r) = &node.payload else {
                    continue;
                };
                let source_ref = node
                    .evidence
                    .iter()
                    .find_map(|e| fragment(e.as_id()))
                    .map(|f| f.source_ref.to_string());
                let (highlight_start, highlight_end) = highlight(&r.statement, *start, *end);
                shown_requirements.insert(id.clone());
                out.push(ProposalEvidence {
                    id: id.to_string(),
                    source_ref,
                    text: Some(r.statement.clone()),
                    source_identifier: r.source_identifier.clone(),
                    highlight_start,
                    highlight_end,
                });
            }
            Grounding::Fragment { id, start, end } => {
                let Some(f) = fragment(id) else { continue };
                let text = f.extracted_text.clone();
                let (highlight_start, highlight_end) = match &text {
                    Some(text) => highlight(text, *start, *end),
                    None => (None, None),
                };
                shown_fragments.insert(id.clone());
                out.push(ProposalEvidence {
                    id: id.to_string(),
                    source_ref: Some(f.source_ref.to_string()),
                    text,
                    source_identifier: owner.get(id).and_then(|o| o.1.clone()),
                    highlight_start,
                    highlight_end,
                });
            }
        }
    }
    for id in &refs {
        if shown_fragments.contains(id)
            || owner
                .get(id)
                .is_some_and(|(req, _)| shown_requirements.contains(req))
        {
            continue;
        }
        let f = fragment(id);
        let text = f.and_then(|f| f.extracted_text.clone());
        let end = text.as_ref().map(|t| t.len() as u64);
        out.push(ProposalEvidence {
            id: id.to_string(),
            source_ref: f.map(|f| f.source_ref.to_string()),
            source_identifier: owner.get(id).and_then(|o| o.1.clone()),
            highlight_start: end.map(|_| 0),
            highlight_end: end,
            text,
        });
    }
    out
}

// ============================================================================ model diff

/// "What changes in the model" of one `apply_patch` result (B1): added elements present in
/// `candidate`, removed elements present in `base`, modified elements in either, each kept only
/// when it is model content whatever its lifecycle status. Node and edge items are merged and
/// sorted by ID.
pub(crate) fn model_diff(
    proposal: &Proposal,
    base: &Graph,
    candidate: &Graph,
    delta: &GraphDelta,
) -> ProposalModelDiff {
    let names_candidate = Names::new(proposal, vec![candidate, base]);
    let names_base = Names::new(proposal, vec![base, candidate]);
    let item = |graph: &Graph, names: &Names, id: &Id| -> Option<DiffItem> {
        if let Some(node) = graph.node(id) {
            return is_model_node(node).then(|| DiffItem {
                id: id.to_string(),
                kind: type_snake(node.payload.node_type()),
                label: names.node_label(node),
            });
        }
        let edge = graph.edge(id)?;
        is_model_edge(graph, edge).then(|| DiffItem {
            id: id.to_string(),
            kind: edge.kind.as_str().to_owned(),
            label: names.edge_label(edge),
        })
    };
    let collect = |ids: Vec<&Id>, primary: (&Graph, &Names), fallback: Option<(&Graph, &Names)>| {
        let mut items: Vec<DiffItem> = ids
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|id| {
                item(primary.0, primary.1, id)
                    .or_else(|| fallback.and_then(|(g, n)| item(g, n, id)))
            })
            .collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        items
    };
    ProposalModelDiff {
        added: collect(
            delta.added_nodes.iter().chain(&delta.added_edges).collect(),
            (candidate, &names_candidate),
            None,
        ),
        modified: collect(
            delta
                .modified_nodes
                .iter()
                .chain(&delta.modified_edges)
                .collect(),
            (candidate, &names_candidate),
            Some((base, &names_base)),
        ),
        removed: collect(
            delta
                .removed_nodes
                .iter()
                .chain(&delta.removed_edges)
                .collect(),
            (base, &names_base),
            None,
        ),
    }
}

// ============================================================================ services

impl ApiState {
    /// The graph a pending occurrence was built on: the head itself, or its base revision.
    fn base_of(
        &self,
        loaded: &LoadedRevision,
        stored: &StoredProposal,
        cache: &mut BTreeMap<RevisionId, LoadedRevision>,
    ) -> Result<Graph, ApiError> {
        let base = &stored.record.base_revision_id;
        if base == &loaded.revision.id {
            return Ok(loaded.graph.clone());
        }
        if !cache.contains_key(base) {
            cache.insert(base.clone(), self.load_revision(base)?);
        }
        Ok(cache[base].graph.clone())
    }

    /// The stage recipe of a stage-origin view (`flows`, ...); `None` for other origins.
    fn stage_of(&self, view: &PendingProposalView) -> Result<Option<String>, ApiError> {
        let Some(job) = &view.recipe_job_id else {
            return Ok(None);
        };
        let corrupt = || ApiError::workflow_corrupt(&format!("recipe job {job}"));
        let hash: plumb_core::Hash = job.parse().map_err(|_| corrupt())?;
        let record = self
            .with_stage_store(|store| store.recipe_job(&hash))?
            .map_err(|e| ApiError::store(&e, "stage_store"))?
            .ok_or_else(corrupt)?;
        Ok(Some(record.recipe))
    }

    /// One Inbox item: the discovery view with its title, kind, instant and stage.
    fn inbox_item(
        &self,
        view: PendingProposalView,
        stored: &StoredProposal,
        graphs: Vec<&Graph>,
    ) -> Result<PendingProposal, ApiError> {
        let corrupt = || ApiError::workflow_corrupt(&format!("proposal {}", view.proposal_id));
        let action: ProposalKind = parse_wire(&view.action, "action").map_err(|_| corrupt())?;
        let origin: ProposalOrigin = parse_wire(&view.origin, "origin").map_err(|_| corrupt())?;
        let (title, element_kind) = describe(&stored.proposal, stored.record.origin_kind, graphs);
        let stage = self.stage_of(&view)?;
        Ok(PendingProposal {
            proposal_id: view.proposal_id,
            action,
            origin,
            origin_ref: view.origin_ref,
            recipe_job_id: view.recipe_job_id,
            stage_job_id: view.stage_job_id,
            base_revision: view.base_revision,
            requires_reason: view.requires_reason,
            stale: view.stale,
            discardable: is_discardable(stored.record.origin_kind),
            title,
            element_kind,
            created_at: stored.record.created_at.to_string(),
            stage,
        })
    }

    /// Every pending proposal as an Inbox item (B1 `list_proposals`), judged against `loaded`
    /// and described against the graph it was built on; sorted by proposal ID then base.
    pub fn proposal_items(
        &self,
        loaded: &LoadedRevision,
    ) -> Result<Vec<PendingProposal>, ApiError> {
        let mut cache = BTreeMap::new();
        let mut items = Vec::new();
        for (view, stored) in self.pending_entries_on(loaded)? {
            let base = self.base_of(loaded, &stored, &mut cache)?;
            items.push(self.inbox_item(view, &stored, vec![&base, &loaded.graph])?);
        }
        items.sort_by(|a, b| {
            (&a.proposal_id, &a.base_revision).cmp(&(&b.proposal_id, &b.base_revision))
        });
        Ok(items)
    }

    /// The Inbox detail of the pending occurrence of `id` on `loaded` (B1 `get_proposal`): 404
    /// when unknown, 409 `E_PROPOSAL_NOT_PENDING` when the addressed occurrence is resolved.
    /// The model change dry-applies the patch to the head, or to its own base revision when it
    /// was built on another one. Read-only.
    pub fn proposal_detail(
        &self,
        loaded: &LoadedRevision,
        id: &str,
    ) -> Result<PendingProposalDetail, ApiError> {
        self.runtime("workflow_store")?;
        let missing = || ApiError::not_found(format!("proposal {} not found", echo(id)));
        let proposal_id = parse_id(id).ok_or_else(missing)?;
        let record = self
            .with_workflow_store(|store| store.current_proposal(&proposal_id, &loaded.revision.id))?
            .map_err(workflow_error)?
            .ok_or_else(missing)?;
        let stored = decode_proposal(record)?;
        if stored.record.state != WorkflowProposalState::Pending {
            return Err(not_pending(&stored.record));
        }
        let view = self.pending_view(&stored, &loaded.revision.id)?;
        let base = self.base_of(loaded, &stored, &mut BTreeMap::new())?;
        let applied = apply_patch(&base, &stored.proposal.patch_set).ok();
        let candidate = applied.as_ref().map(|a| &a.graph);
        let names_graphs: Vec<&Graph> = candidate
            .into_iter()
            .chain([&base, &loaded.graph])
            .collect();
        let origin = stored.record.origin_kind;
        // The title and kind are judged exactly as the list judges them.
        let item = self.inbox_item(view, &stored, vec![&base, &loaded.graph])?;
        let diff = match &applied {
            Some(applied) => model_diff(&stored.proposal, &base, &applied.graph, &applied.delta),
            None => ProposalModelDiff {
                added: Vec::new(),
                modified: Vec::new(),
                removed: Vec::new(),
            },
        };
        Ok(PendingProposalDetail {
            facts: facts(&stored.proposal, origin, names_graphs.clone()),
            evidence: evidence(&stored.proposal, names_graphs),
            diff,
            proposal_id: item.proposal_id,
            action: item.action,
            origin: item.origin,
            origin_ref: item.origin_ref,
            recipe_job_id: item.recipe_job_id,
            stage_job_id: item.stage_job_id,
            base_revision: item.base_revision,
            requires_reason: item.requires_reason,
            stale: item.stale,
            discardable: item.discardable,
            title: item.title,
            element_kind: item.element_kind,
            created_at: item.created_at,
            stage: item.stage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::humanize;

    #[test]
    fn humanize_splits_identifier_names_only() {
        assert_eq!(humanize("LeaveRequest"), "Leave request");
        assert_eq!(humanize("double_days"), "double days");
        assert_eq!(humanize("HRManager"), "HR manager");
        assert_eq!(humanize("Submit leave request"), "Submit leave request");
        assert_eq!(humanize("Draft"), "Draft");
        assert_eq!(humanize("op:submit"), "op:submit");
    }
}
