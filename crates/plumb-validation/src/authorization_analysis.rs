//! Pure authorization analysis: SecurityRole hierarchy cycles, effective roles and static
//! separation of duty (plan S2.8, relocated by Hotfix 044).
//!
//! `A inherits_role B` means A inherits B, so effective roles follow outgoing inheritance. A
//! subject violates a static separation-of-duty constraint with roles R iff at least two roles
//! of R are effective for it; dynamic separation of duty, mutual exclusion and required
//! combination are reported as not evaluated. S2.8 compilation and S2.10 validation share this
//! one implementation.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use plumb_psg::{
    ElementStatus, Graph, NodePayload, NodeType, RelationKind, SeparationConstraint,
    SeparationConstraintKind,
};
use serde::{Deserialize, Serialize};

// ============================================================================ analyzers

/// Authorization relations over which hierarchy and separation are analyzed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuthorizationFacts {
    /// `(subject, security_role)` assigned_role pairs.
    pub assignments: Vec<(Id, Id)>,
    /// `(role, inherited_role)` inherits_role pairs.
    pub inheritances: Vec<(Id, Id)>,
    /// SeparationConstraints by ID.
    pub constraints: Vec<(Id, SeparationConstraint)>,
}

/// One subject holding at least two roles of one static separation-of-duty constraint.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaticSodViolation {
    pub constraint_ref: Id,
    pub subject_ref: Id,
    /// Sorted.
    pub conflicting_role_refs: Vec<Id>,
}

/// Static separation-of-duty analysis; the other kinds are not evaluated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeparationAnalysis {
    /// Sorted by constraint and subject.
    pub static_sod_violations: Vec<StaticSodViolation>,
    /// Constraints of kinds without executable semantics, sorted.
    pub not_evaluated: Vec<Id>,
}

fn is_active(status: ElementStatus) -> bool {
    matches!(status, ElementStatus::Proposed | ElementStatus::Accepted)
}

/// Tarjan bookkeeping: next index, (index, lowlink) marks, the stack and its members.
#[derive(Default)]
struct TarjanState {
    next: usize,
    marks: BTreeMap<Id, (usize, usize)>,
    stack: Vec<Id>,
    on_stack: BTreeSet<Id>,
}

fn tarjan(
    v: &Id,
    adjacency: &BTreeMap<Id, BTreeSet<Id>>,
    state: &mut TarjanState,
    out: &mut Vec<Vec<Id>>,
) {
    let index = state.next;
    state.next += 1;
    state.marks.insert(v.clone(), (index, index));
    state.stack.push(v.clone());
    state.on_stack.insert(v.clone());
    for w in adjacency.get(v).into_iter().flatten() {
        if !state.marks.contains_key(w) {
            tarjan(w, adjacency, state, out);
            let low_w = state.marks[w].1;
            let entry = state.marks.get_mut(v).expect("visited");
            entry.1 = entry.1.min(low_w);
        } else if state.on_stack.contains(w) {
            let index_w = state.marks[w].0;
            let entry = state.marks.get_mut(v).expect("visited");
            entry.1 = entry.1.min(index_w);
        }
    }
    let (index_v, low_v) = state.marks[v];
    if index_v == low_v {
        let mut component = Vec::new();
        while let Some(w) = state.stack.pop() {
            state.on_stack.remove(&w);
            let done = &w == v;
            component.push(w);
            if done {
                break;
            }
        }
        let self_loop = adjacency.get(v).is_some_and(|s| s.contains(v));
        if component.len() > 1 || self_loop {
            component.sort();
            out.push(component);
        }
    }
}

/// Self-cycles and multi-role cycles (strongly connected components) of `inherits_role` pairs,
/// each sorted, in sorted order.
pub fn role_hierarchy_cycles(inheritances: &[(Id, Id)]) -> Vec<Vec<Id>> {
    let mut adjacency: BTreeMap<Id, BTreeSet<Id>> = BTreeMap::new();
    for (from, to) in inheritances {
        adjacency
            .entry(from.clone())
            .or_default()
            .insert(to.clone());
        adjacency.entry(to.clone()).or_default();
    }
    let mut state = TarjanState::default();
    let mut out = Vec::new();
    for v in adjacency.keys() {
        if !state.marks.contains_key(v) {
            tarjan(v, &adjacency, &mut state, &mut out);
        }
    }
    out.sort();
    out
}

/// The effective SecurityRoles of a subject: its direct assignments plus the transitive closure
/// of outgoing inheritance (`A inherits_role B` gives B to holders of A), sorted.
pub fn effective_security_roles(facts: &AuthorizationFacts, subject: &Id) -> Vec<Id> {
    let mut inherits: BTreeMap<&Id, BTreeSet<&Id>> = BTreeMap::new();
    for (from, to) in &facts.inheritances {
        inherits.entry(from).or_default().insert(to);
    }
    let mut seen: BTreeSet<&Id> = BTreeSet::new();
    let mut stack: Vec<&Id> = facts
        .assignments
        .iter()
        .filter(|(s, _)| s == subject)
        .map(|(_, r)| r)
        .collect();
    while let Some(role) = stack.pop() {
        if seen.insert(role) {
            stack.extend(inherits.get(role).into_iter().flatten().copied());
        }
    }
    seen.into_iter().cloned().collect()
}

/// Static separation of duty: a subject violates a constraint with roles R iff at least two
/// roles of R are effective for it. Other kinds are not evaluated.
pub fn separation_analysis(facts: &AuthorizationFacts) -> SeparationAnalysis {
    let subjects: BTreeSet<&Id> = facts.assignments.iter().map(|(s, _)| s).collect();
    let effective: BTreeMap<&Id, BTreeSet<Id>> = subjects
        .into_iter()
        .map(|s| (s, effective_security_roles(facts, s).into_iter().collect()))
        .collect();
    let mut analysis = SeparationAnalysis::default();
    for (constraint_ref, constraint) in &facts.constraints {
        if constraint.constraint_kind != SeparationConstraintKind::StaticSeparationOfDuty {
            analysis.not_evaluated.push(constraint_ref.clone());
            continue;
        }
        let governed: BTreeSet<&Id> = constraint.role_refs.iter().collect();
        for (subject, roles) in &effective {
            let conflicting: Vec<Id> = roles
                .iter()
                .filter(|r| governed.contains(r))
                .cloned()
                .collect();
            if conflicting.len() >= 2 {
                analysis.static_sod_violations.push(StaticSodViolation {
                    constraint_ref: constraint_ref.clone(),
                    subject_ref: (*subject).clone(),
                    conflicting_role_refs: conflicting,
                });
            }
        }
    }
    analysis.static_sod_violations.sort();
    analysis.static_sod_violations.dedup();
    analysis.not_evaluated.sort();
    analysis.not_evaluated.dedup();
    analysis
}

/// The authorization facts of a graph: Accepted-only (baseline) or active (Proposed and
/// Accepted) elements.
pub fn authorization_facts(graph: &Graph, accepted_only: bool) -> AuthorizationFacts {
    let included = |status: ElementStatus| {
        if accepted_only {
            status == ElementStatus::Accepted
        } else {
            is_active(status)
        }
    };
    let node_ok = |id: &Id| graph.node(id).is_some_and(|n| included(n.status));
    let pairs = |kind: RelationKind| -> Vec<(Id, Id)> {
        let mut out: Vec<(Id, Id)> = graph
            .edge_ids_by_kind(&kind)
            .iter()
            .filter_map(|e| graph.edge(e))
            .filter(|e| included(e.status) && node_ok(&e.from) && node_ok(&e.to))
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        out.sort();
        out.dedup();
        out
    };
    let constraints = graph
        .node_ids_by_type(NodeType::SeparationConstraint)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| included(n.status))
        .filter_map(|n| match &n.payload {
            NodePayload::SeparationConstraint(c) => Some((n.id.clone(), c.clone())),
            _ => None,
        })
        .collect();
    AuthorizationFacts {
        assignments: pairs(RelationKind::AssignedRole),
        inheritances: pairs(RelationKind::InheritsRole),
        constraints,
    }
}

/// Accepted-only SecurityRole hierarchy cycles (the baseline analyzer).
pub fn accepted_role_hierarchy_cycles(graph: &Graph) -> Vec<Vec<Id>> {
    role_hierarchy_cycles(&authorization_facts(graph, true).inheritances)
}

/// Accepted-only static separation-of-duty analysis (the baseline analyzer).
pub fn accepted_separation_analysis(graph: &Graph) -> SeparationAnalysis {
    separation_analysis(&authorization_facts(graph, true))
}
