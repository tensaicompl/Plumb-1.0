//! Pure deterministic Process graph analysis (plan S2.9; compiler architecture §8).
//!
//! This is a dependency-neutral analysis kernel, not a rule evaluator: it produces typed
//! structural material about one Process and creates no finding. Membership is exactly
//! `ProcessNode.process_ref == Process.id`; control flow is `next` between members. The pilot
//! executable subset is start (manual, Event or timer), end, operation-backed human and service
//! tasks, operation-less human activities with performer and outcome, Event and timer waits and
//! simple structured parallel split/join. Exclusive gateways, error events, subprocesses and any
//! node condition are recorded as unqualified semantics, never reinterpreted.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use plumb_core::Id;
use plumb_psg::{
    ElementStatus, Graph, Node, NodePayload, NodeType, ProcessNode, ProcessNodeKind, RelationKind,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Which elements participate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessAnalysisMode {
    /// Accepted Process, ProcessNodes and relations only (baseline validation).
    AcceptedOnly,
    /// Proposed and Accepted elements (proposal qualification on a dry-run graph).
    ActiveOverlay,
}

impl ProcessAnalysisMode {
    fn includes(self, status: ElementStatus) -> bool {
        match self {
            ProcessAnalysisMode::AcceptedOnly => status == ElementStatus::Accepted,
            ProcessAnalysisMode::ActiveOverlay => {
                matches!(status, ElementStatus::Proposed | ElementStatus::Accepted)
            }
        }
    }
}

/// Why a Process cannot be analyzed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProcessAnalysisError {
    #[error("process {0} does not exist")]
    NotFound(Id),
    #[error("{0} is not a Process")]
    NotAProcess(Id),
    #[error("process {0} does not have a status included by the analysis mode")]
    StatusNotIncluded(Id),
}

/// Why an executable task does not resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskResolutionIssue {
    /// A service task, or an operation-backed human task, without an Accepted Operation.
    UnresolvedServiceTask,
    /// An operation-less human task without an Accepted Actor/BusinessRole performer.
    UnresolvedHumanResponsibility,
    /// An operation-less human task without an Accepted produced Outcome/Event.
    UnresolvedHumanOutcome,
}

/// One unresolved executable task.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnresolvedTask {
    pub node_ref: Id,
    pub issue: TaskResolutionIssue,
}

/// Semantics the pilot cannot qualify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedSemantics {
    ExclusiveGateway,
    ErrorEvent,
    Subprocess,
    ConditionExpression,
}

/// One node carrying unqualified executable semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnsupportedNode {
    pub node_ref: Id,
    pub semantics: UnsupportedSemantics,
}

/// A `next` edge leaving or entering the Process.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BoundaryViolation {
    pub edge_ref: Id,
    pub from: Id,
    pub to: Id,
}

/// A violated node-shape requirement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum ShapeViolation {
    Indegree { expected: String, actual: usize },
    Outdegree { expected: String, actual: usize },
    UnexpectedField { field: String },
    MissingField { field: String },
    AmbiguousStartTrigger,
    InvalidTimerExpression,
    InvalidMessageRef,
    MissingConsumes,
    UnexpectedRelation { relation: String },
}

/// One node-shape violation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeShapeViolation {
    pub node_ref: Id,
    pub violation: ShapeViolation,
}

/// A parallel-structure problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParallelIssueKind {
    ParallelCyclicFlow,
    ParallelJoinMissing,
    ParallelJoinAmbiguous,
    ParallelBranchDoesNotJoin,
    ParallelBranchesMergeEarly,
    NestedParallelUnsupported,
    ParallelJoinUnmatched,
}

/// One parallel issue with its affected nodes (sorted).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ParallelIssue {
    pub kind: ParallelIssueKind,
    pub split_ref: Option<Id>,
    pub affected_refs: Vec<Id>,
}

/// The pairing material of one split.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ParallelSplitAnalysis {
    pub split_ref: Id,
    pub matched_join_ref: Option<Id>,
    pub branch_root_refs: Vec<Id>,
}

/// Parallel structure of a Process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "parallel", rename_all = "snake_case")]
pub enum ParallelAnalysis {
    NotApplicable,
    Analyzed {
        splits: Vec<ParallelSplitAnalysis>,
        issues: Vec<ParallelIssue>,
    },
}

impl ParallelAnalysis {
    pub fn issues(&self) -> &[ParallelIssue] {
        match self {
            ParallelAnalysis::NotApplicable => &[],
            ParallelAnalysis::Analyzed { issues, .. } => issues,
        }
    }
}

/// Deterministic structural material about one Process; every list is sorted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessAnalysis {
    pub process_ref: Id,
    pub mode: ProcessAnalysisMode,
    pub node_refs: Vec<Id>,
    pub start_refs: Vec<Id>,
    pub end_refs: Vec<Id>,
    pub unreachable_refs: Vec<Id>,
    pub invalid_terminal_refs: Vec<Id>,
    pub unresolved_task_refs: Vec<UnresolvedTask>,
    pub unsupported_nodes: Vec<UnsupportedNode>,
    pub boundary_violations: Vec<BoundaryViolation>,
    pub node_shape_violations: Vec<NodeShapeViolation>,
    pub parallel_analysis: ParallelAnalysis,
}

impl ProcessAnalysis {
    pub fn missing_start(&self) -> bool {
        self.start_refs.is_empty()
    }

    pub fn missing_end(&self) -> bool {
        self.end_refs.is_empty()
    }

    /// Whether the Process is fully qualified in the pilot executable subset.
    pub fn is_qualified(&self) -> bool {
        !self.missing_start()
            && !self.missing_end()
            && self.unreachable_refs.is_empty()
            && self.invalid_terminal_refs.is_empty()
            && self.unresolved_task_refs.is_empty()
            && self.unsupported_nodes.is_empty()
            && self.boundary_violations.is_empty()
            && self.node_shape_violations.is_empty()
            && self.parallel_analysis.issues().is_empty()
    }
}

struct Ctx<'g> {
    graph: &'g Graph,
    mode: ProcessAnalysisMode,
}

impl<'g> Ctx<'g> {
    /// Included relation targets of `kind` from `node`.
    fn targets(&self, node: &Id, kind: RelationKind) -> Vec<&'g Id> {
        let graph = self.graph;
        graph
            .outgoing_edge_ids(node)
            .iter()
            .filter_map(|e| graph.edge(e))
            .filter(|e| e.kind == kind && self.mode.includes(e.status))
            .map(|e| &e.to)
            .collect()
    }

    fn accepted_of(&self, id: &Id, types: &[NodeType]) -> bool {
        self.graph.node(id).is_some_and(|n| {
            n.status == ElementStatus::Accepted && types.contains(&n.payload.node_type())
        })
    }
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

fn reach(adjacency: &BTreeMap<Id, BTreeSet<Id>>, from: &Id, stop: Option<&Id>) -> BTreeSet<Id> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([from.clone()]);
    while let Some(v) = queue.pop_front() {
        if Some(&v) == stop || !seen.insert(v.clone()) {
            continue;
        }
        for w in adjacency.get(&v).into_iter().flatten() {
            queue.push_back(w.clone());
        }
    }
    seen
}

fn has_cycle(adjacency: &BTreeMap<Id, BTreeSet<Id>>) -> bool {
    let mut indegree: BTreeMap<&Id, usize> = adjacency.keys().map(|k| (k, 0)).collect();
    for targets in adjacency.values() {
        for t in targets {
            *indegree.entry(t).or_default() += 1;
        }
    }
    let mut queue: VecDeque<&Id> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(k, _)| *k)
        .collect();
    let mut visited = 0;
    while let Some(v) = queue.pop_front() {
        visited += 1;
        for w in adjacency.get(v).into_iter().flatten() {
            let d = indegree.get_mut(w).expect("node");
            *d -= 1;
            if *d == 0 {
                queue.push_back(w);
            }
        }
    }
    visited != indegree.len()
}

fn degree_check(
    out: &mut Vec<NodeShapeViolation>,
    node: &Id,
    indegree: usize,
    outdegree: usize,
    expected_in: (usize, Option<usize>),
    expected_out: (usize, Option<usize>),
) {
    let fmt = |(lo, hi): (usize, Option<usize>)| match hi {
        Some(h) if h == lo => format!("{lo}"),
        Some(h) => format!("{lo}..={h}"),
        None => format!(">={lo}"),
    };
    let ok = |v: usize, (lo, hi): (usize, Option<usize>)| v >= lo && hi.is_none_or(|h| v <= h);
    if !ok(indegree, expected_in) {
        out.push(NodeShapeViolation {
            node_ref: node.clone(),
            violation: ShapeViolation::Indegree {
                expected: fmt(expected_in),
                actual: indegree,
            },
        });
    }
    if !ok(outdegree, expected_out) {
        out.push(NodeShapeViolation {
            node_ref: node.clone(),
            violation: ShapeViolation::Outdegree {
                expected: fmt(expected_out),
                actual: outdegree,
            },
        });
    }
}

/// Analyzes one Process in the given mode.
pub fn analyze_process(
    graph: &Graph,
    process_ref: &Id,
    mode: ProcessAnalysisMode,
) -> Result<ProcessAnalysis, ProcessAnalysisError> {
    let process = graph
        .node(process_ref)
        .ok_or_else(|| ProcessAnalysisError::NotFound(process_ref.clone()))?;
    if !matches!(process.payload, NodePayload::Process(_)) {
        return Err(ProcessAnalysisError::NotAProcess(process_ref.clone()));
    }
    if !mode.includes(process.status) {
        return Err(ProcessAnalysisError::StatusNotIncluded(process_ref.clone()));
    }
    let ctx = Ctx { graph, mode };
    // Members.
    let members: BTreeMap<Id, (&Node, &ProcessNode)> = graph
        .node_ids_by_type(NodeType::ProcessNode)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| mode.includes(n.status))
        .filter_map(|n| match &n.payload {
            NodePayload::ProcessNode(p) if &p.process_ref == process_ref => {
                Some((n.id.clone(), (n, p)))
            }
            _ => None,
        })
        .collect();
    // In-process flow and boundary violations.
    let mut adjacency: BTreeMap<Id, BTreeSet<Id>> = members
        .keys()
        .map(|k| (k.clone(), BTreeSet::new()))
        .collect();
    let mut indegree: BTreeMap<Id, usize> = members.keys().map(|k| (k.clone(), 0)).collect();
    let mut boundary_violations = Vec::new();
    for edge_id in graph.edge_ids_by_kind(&RelationKind::Next) {
        let Some(e) = graph.edge(edge_id) else {
            continue;
        };
        if !mode.includes(e.status) {
            continue;
        }
        let (from_in, to_in) = (members.contains_key(&e.from), members.contains_key(&e.to));
        match (from_in, to_in) {
            (true, true) => {
                if adjacency
                    .get_mut(&e.from)
                    .expect("member")
                    .insert(e.to.clone())
                {
                    *indegree.get_mut(&e.to).expect("member") += 1;
                }
            }
            (false, false) => {}
            _ => {
                let endpoint_active =
                    |id: &Id| graph.node(id).is_some_and(|n| mode.includes(n.status));
                if endpoint_active(&e.from) && endpoint_active(&e.to) {
                    boundary_violations.push(BoundaryViolation {
                        edge_ref: e.id.clone(),
                        from: e.from.clone(),
                        to: e.to.clone(),
                    });
                }
            }
        }
    }
    let outdegree = |id: &Id| adjacency.get(id).map_or(0, BTreeSet::len);

    let mut start_refs = Vec::new();
    let mut end_refs = Vec::new();
    let mut unresolved_task_refs = Vec::new();
    let mut unsupported_nodes = Vec::new();
    let mut shapes = Vec::new();
    let mut splits = Vec::new();
    let mut joins = Vec::new();
    for (id, (_, p)) in &members {
        let (i, o) = (indegree[id], outdegree(id));
        let field = |name: &str| ShapeViolation::UnexpectedField {
            field: name.to_owned(),
        };
        let forbid =
            |shapes: &mut Vec<NodeShapeViolation>, operation: bool, message: bool, timer: bool| {
                for (present, name) in [
                    (operation && p.operation_ref.is_some(), "operation_ref"),
                    (message && p.message_ref.is_some(), "message_ref"),
                    (timer && p.timer_expr.is_some(), "timer_expr"),
                ] {
                    if present {
                        shapes.push(NodeShapeViolation {
                            node_ref: id.clone(),
                            violation: field(name),
                        });
                    }
                }
            };
        if p.condition_expr.is_some() {
            unsupported_nodes.push(UnsupportedNode {
                node_ref: id.clone(),
                semantics: UnsupportedSemantics::ConditionExpression,
            });
        }
        let performers = ctx.targets(id, RelationKind::PerformedBy);
        let produces = ctx.targets(id, RelationKind::Produces);
        let consumes = ctx.targets(id, RelationKind::Consumes);
        let no_relation = |shapes: &mut Vec<NodeShapeViolation>, kinds: &[(&[&Id], &str)]| {
            for (targets, name) in kinds {
                if !targets.is_empty() {
                    shapes.push(NodeShapeViolation {
                        node_ref: id.clone(),
                        violation: ShapeViolation::UnexpectedRelation {
                            relation: (*name).to_owned(),
                        },
                    });
                }
            }
        };
        // A message_ref names an Accepted Event that the node consumes.
        let message = |shapes: &mut Vec<NodeShapeViolation>| match &p.message_ref {
            None => {}
            Some(event) => {
                if !ctx.accepted_of(event, &[NodeType::Event]) {
                    shapes.push(NodeShapeViolation {
                        node_ref: id.clone(),
                        violation: ShapeViolation::InvalidMessageRef,
                    });
                } else if !consumes.contains(&event) {
                    shapes.push(NodeShapeViolation {
                        node_ref: id.clone(),
                        violation: ShapeViolation::MissingConsumes,
                    });
                }
            }
        };
        let timer = |shapes: &mut Vec<NodeShapeViolation>| {
            if let Some(t) = &p.timer_expr {
                if !valid_text(t) {
                    shapes.push(NodeShapeViolation {
                        node_ref: id.clone(),
                        violation: ShapeViolation::InvalidTimerExpression,
                    });
                }
            }
        };
        let operation_resolves = p
            .operation_ref
            .as_ref()
            .is_some_and(|op| ctx.accepted_of(op, &[NodeType::Operation]));
        match p.node_kind {
            ProcessNodeKind::Start => {
                start_refs.push(id.clone());
                forbid(&mut shapes, true, false, false);
                if p.message_ref.is_some() && p.timer_expr.is_some() {
                    shapes.push(NodeShapeViolation {
                        node_ref: id.clone(),
                        violation: ShapeViolation::AmbiguousStartTrigger,
                    });
                }
                message(&mut shapes);
                timer(&mut shapes);
                no_relation(
                    &mut shapes,
                    &[(&performers, "performed_by"), (&produces, "produces")],
                );
                degree_check(&mut shapes, id, i, o, (0, Some(0)), (1, Some(1)));
            }
            ProcessNodeKind::End => {
                end_refs.push(id.clone());
                forbid(&mut shapes, true, true, true);
                degree_check(&mut shapes, id, i, o, (1, None), (0, Some(0)));
            }
            ProcessNodeKind::ServiceTask => {
                forbid(&mut shapes, false, true, true);
                if !operation_resolves {
                    unresolved_task_refs.push(UnresolvedTask {
                        node_ref: id.clone(),
                        issue: TaskResolutionIssue::UnresolvedServiceTask,
                    });
                }
                no_relation(
                    &mut shapes,
                    &[(&performers, "performed_by"), (&produces, "produces")],
                );
                degree_check(&mut shapes, id, i, o, (1, Some(1)), (1, Some(1)));
            }
            ProcessNodeKind::HumanTask => {
                forbid(&mut shapes, false, true, true);
                if p.operation_ref.is_some() {
                    if !operation_resolves {
                        unresolved_task_refs.push(UnresolvedTask {
                            node_ref: id.clone(),
                            issue: TaskResolutionIssue::UnresolvedServiceTask,
                        });
                    }
                    no_relation(
                        &mut shapes,
                        &[(&performers, "performed_by"), (&produces, "produces")],
                    );
                } else {
                    if !performers
                        .iter()
                        .any(|t| ctx.accepted_of(t, &[NodeType::Actor, NodeType::BusinessRole]))
                    {
                        unresolved_task_refs.push(UnresolvedTask {
                            node_ref: id.clone(),
                            issue: TaskResolutionIssue::UnresolvedHumanResponsibility,
                        });
                    }
                    if !produces
                        .iter()
                        .any(|t| ctx.accepted_of(t, &[NodeType::Outcome, NodeType::Event]))
                    {
                        unresolved_task_refs.push(UnresolvedTask {
                            node_ref: id.clone(),
                            issue: TaskResolutionIssue::UnresolvedHumanOutcome,
                        });
                    }
                }
                degree_check(&mut shapes, id, i, o, (1, Some(1)), (1, Some(1)));
            }
            ProcessNodeKind::MessageEvent => {
                forbid(&mut shapes, true, false, true);
                if p.message_ref.is_none() {
                    shapes.push(NodeShapeViolation {
                        node_ref: id.clone(),
                        violation: ShapeViolation::MissingField {
                            field: "message_ref".into(),
                        },
                    });
                }
                message(&mut shapes);
                no_relation(
                    &mut shapes,
                    &[(&performers, "performed_by"), (&produces, "produces")],
                );
                degree_check(&mut shapes, id, i, o, (1, Some(1)), (1, Some(1)));
            }
            ProcessNodeKind::TimerEvent => {
                forbid(&mut shapes, true, true, false);
                if p.timer_expr.is_none() {
                    shapes.push(NodeShapeViolation {
                        node_ref: id.clone(),
                        violation: ShapeViolation::MissingField {
                            field: "timer_expr".into(),
                        },
                    });
                }
                timer(&mut shapes);
                no_relation(
                    &mut shapes,
                    &[(&performers, "performed_by"), (&produces, "produces")],
                );
                degree_check(&mut shapes, id, i, o, (1, Some(1)), (1, Some(1)));
            }
            ProcessNodeKind::ParallelSplit => {
                splits.push(id.clone());
                forbid(&mut shapes, true, true, true);
                degree_check(&mut shapes, id, i, o, (1, Some(1)), (2, None));
            }
            ProcessNodeKind::ParallelJoin => {
                joins.push(id.clone());
                forbid(&mut shapes, true, true, true);
                degree_check(&mut shapes, id, i, o, (2, None), (1, Some(1)));
            }
            ProcessNodeKind::ExclusiveGateway => unsupported_nodes.push(UnsupportedNode {
                node_ref: id.clone(),
                semantics: UnsupportedSemantics::ExclusiveGateway,
            }),
            ProcessNodeKind::ErrorEvent => unsupported_nodes.push(UnsupportedNode {
                node_ref: id.clone(),
                semantics: UnsupportedSemantics::ErrorEvent,
            }),
            ProcessNodeKind::Subprocess => unsupported_nodes.push(UnsupportedNode {
                node_ref: id.clone(),
                semantics: UnsupportedSemantics::Subprocess,
            }),
        }
    }

    // Reachability from the union of Starts; terminals must be Ends.
    let mut reachable = BTreeSet::new();
    for s in &start_refs {
        reachable.extend(reach(&adjacency, s, None));
    }
    let unreachable_refs: Vec<Id> = members
        .keys()
        .filter(|id| !reachable.contains(*id))
        .cloned()
        .collect();
    let invalid_terminal_refs: Vec<Id> = members
        .iter()
        .filter(|(id, (_, p))| outdegree(id) == 0 && p.node_kind != ProcessNodeKind::End)
        .map(|(id, _)| id.clone())
        .collect();

    let parallel_analysis = parallel(&adjacency, &members, &splits, &joins);

    let mut analysis = ProcessAnalysis {
        process_ref: process_ref.clone(),
        mode,
        node_refs: members.keys().cloned().collect(),
        start_refs,
        end_refs,
        unreachable_refs,
        invalid_terminal_refs,
        unresolved_task_refs,
        unsupported_nodes,
        boundary_violations,
        node_shape_violations: shapes,
        parallel_analysis,
    };
    analysis.unresolved_task_refs.sort();
    analysis.unsupported_nodes.sort();
    analysis.boundary_violations.sort();
    analysis.node_shape_violations.sort();
    analysis.node_shape_violations.dedup();
    Ok(analysis)
}

fn parallel(
    adjacency: &BTreeMap<Id, BTreeSet<Id>>,
    members: &BTreeMap<Id, (&Node, &ProcessNode)>,
    splits: &[Id],
    joins: &[Id],
) -> ParallelAnalysis {
    if splits.is_empty() && joins.is_empty() {
        return ParallelAnalysis::NotApplicable;
    }
    let mut issues = Vec::new();
    let mut analyses = Vec::new();
    if has_cycle(adjacency) {
        issues.push(ParallelIssue {
            kind: ParallelIssueKind::ParallelCyclicFlow,
            split_ref: None,
            affected_refs: splits
                .iter()
                .chain(joins)
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        });
        return ParallelAnalysis::Analyzed {
            splits: analyses,
            issues,
        };
    }
    let reach_from: BTreeMap<&Id, BTreeSet<Id>> = adjacency
        .keys()
        .map(|k| (k, reach(adjacency, k, None)))
        .collect();
    let is_parallel = |id: &Id| {
        members.get(id).is_some_and(|(_, p)| {
            matches!(
                p.node_kind,
                ProcessNodeKind::ParallelSplit | ProcessNodeKind::ParallelJoin
            )
        })
    };
    let mut join_matches: BTreeMap<&Id, usize> = joins.iter().map(|j| (j, 0)).collect();
    for split in splits {
        let roots: Vec<Id> = adjacency
            .get(split)
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        let issue = |kind, affected: Vec<Id>| ParallelIssue {
            kind,
            split_ref: Some(split.clone()),
            affected_refs: affected
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        };
        let candidates: Vec<&Id> = joins
            .iter()
            .filter(|j| !roots.is_empty() && roots.iter().all(|r| reach_from[r].contains(*j)))
            .collect();
        let nearest: Vec<&Id> = candidates
            .iter()
            .filter(|j| {
                !candidates
                    .iter()
                    .any(|k| k != *j && reach_from[*k].contains(**j))
            })
            .copied()
            .collect();
        let matched = match nearest.as_slice() {
            [] => {
                issues.push(issue(
                    ParallelIssueKind::ParallelJoinMissing,
                    vec![split.clone()],
                ));
                None
            }
            [one] => Some((*one).clone()),
            many => {
                issues.push(issue(
                    ParallelIssueKind::ParallelJoinAmbiguous,
                    many.iter().map(|j| (*j).clone()).collect(),
                ));
                None
            }
        };
        if let Some(join) = &matched {
            *join_matches.get_mut(join).expect("join") += 1;
            let regions: Vec<BTreeSet<Id>> = roots
                .iter()
                .map(|r| reach(adjacency, r, Some(join)))
                .collect();
            for region in &regions {
                let stuck: Vec<Id> = region
                    .iter()
                    .filter(|n| !reach_from[*n].contains(join))
                    .cloned()
                    .collect();
                if !stuck.is_empty() {
                    issues.push(issue(ParallelIssueKind::ParallelBranchDoesNotJoin, stuck));
                }
                let nested: Vec<Id> = region.iter().filter(|n| is_parallel(n)).cloned().collect();
                if !nested.is_empty() {
                    issues.push(issue(ParallelIssueKind::NestedParallelUnsupported, nested));
                }
            }
            let mut shared = BTreeSet::new();
            for (a, ra) in regions.iter().enumerate() {
                for rb in &regions[a + 1..] {
                    shared.extend(ra.intersection(rb).cloned());
                }
            }
            if !shared.is_empty() {
                issues.push(issue(
                    ParallelIssueKind::ParallelBranchesMergeEarly,
                    shared.into_iter().collect(),
                ));
            }
        }
        analyses.push(ParallelSplitAnalysis {
            split_ref: split.clone(),
            matched_join_ref: matched,
            branch_root_refs: roots,
        });
    }
    for (join, count) in join_matches {
        if count != 1 {
            issues.push(ParallelIssue {
                kind: ParallelIssueKind::ParallelJoinUnmatched,
                split_ref: None,
                affected_refs: vec![join.clone()],
            });
        }
    }
    issues.sort();
    issues.dedup();
    analyses.sort();
    ParallelAnalysis::Analyzed {
        splits: analyses,
        issues,
    }
}
