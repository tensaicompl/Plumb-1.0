//! S2.9 contract tests for the pure Process graph-analysis kernel: membership, start/end and
//! terminal semantics, reachability, task resolution, Event and timer waits, unsupported
//! semantics, node shapes, structured parallel split/join and analysis modes. Every graph is
//! synthetic and hand-built; the kernel is not an evaluator and creates no finding.

mod process_analysis_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{Id, Timestamp};
    use plumb_psg::{
        Actor, ActorKind, AuditMeta, BusinessRole, Edge, ElementStatus, Event, Graph, Node,
        NodePayload, Operation, OperationKind, Outcome, OutcomeKind, Process, ProcessNode,
        ProcessNodeKind, RelationKind, RelationProperties,
    };
    use plumb_validation::process_analysis::*;

    use ElementStatus::{Accepted, Proposed};
    use ProcessNodeKind as K;

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const P: &str = "process:p";

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ids(list: &[&str]) -> Vec<Id> {
        list.iter().map(|s| id(s)).collect()
    }

    fn meta() -> AuditMeta {
        AuditMeta::new(
            id("actor:human"),
            AT.parse::<Timestamp>().unwrap(),
            None,
            None,
        )
        .unwrap()
    }

    fn node(node_id: &str, status: ElementStatus, payload: NodePayload) -> Node {
        Node {
            id: id(node_id),
            revision: 1,
            status,
            payload,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::new(),
            audit: meta(),
        }
    }

    /// A synthetic process fixture builder.
    #[derive(Clone)]
    struct Fx {
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        status: ElementStatus,
    }

    impl Fx {
        fn new() -> Fx {
            let op = |nid: &str, name: &str, status| {
                node(
                    nid,
                    status,
                    NodePayload::Operation(Operation {
                        name: name.into(),
                        operation_kind: OperationKind::Command,
                        input_schema_ref: None,
                        output_schema_ref: None,
                        preconditions: None,
                        postconditions: None,
                        idempotency: None,
                        transaction_semantics: None,
                    }),
                )
            };
            Fx {
                nodes: vec![
                    node(
                        P,
                        Accepted,
                        NodePayload::Process(Process {
                            name: "P".into(),
                            description: None,
                            process_kind: None,
                        }),
                    ),
                    node(
                        "process:q",
                        Accepted,
                        NodePayload::Process(Process {
                            name: "Q".into(),
                            description: None,
                            process_kind: None,
                        }),
                    ),
                    op("operation:a", "A", Accepted),
                    op("operation:b", "B", Accepted),
                    op("operation:draft", "Draft", Proposed),
                    node(
                        "event:e",
                        Accepted,
                        NodePayload::Event(Event {
                            name: "E".into(),
                            payload_schema_ref: None,
                            semantic_type: None,
                        }),
                    ),
                    node(
                        "event:draft",
                        Proposed,
                        NodePayload::Event(Event {
                            name: "D".into(),
                            payload_schema_ref: None,
                            semantic_type: None,
                        }),
                    ),
                    node(
                        "outcome:done",
                        Accepted,
                        NodePayload::Outcome(Outcome {
                            name: "Done".into(),
                            outcome_kind: OutcomeKind::Success,
                        }),
                    ),
                    node(
                        "brole:clerk",
                        Accepted,
                        NodePayload::BusinessRole(BusinessRole {
                            name: "Clerk".into(),
                        }),
                    ),
                    node(
                        "actor:bot",
                        Accepted,
                        NodePayload::Actor(Actor {
                            name: "Bot".into(),
                            actor_kind: ActorKind::System,
                        }),
                    ),
                ],
                edges: Vec::new(),
                status: Accepted,
            }
        }

        fn pn_in(
            mut self,
            process: &str,
            key: &str,
            kind: K,
            f: impl FnOnce(&mut ProcessNode),
        ) -> Fx {
            let mut p = ProcessNode {
                process_ref: id(process),
                node_kind: kind,
                operation_ref: None,
                condition_expr: None,
                message_ref: None,
                timer_expr: None,
            };
            f(&mut p);
            self.nodes.push(node(
                &format!("pn:{key}"),
                self.status,
                NodePayload::ProcessNode(p),
            ));
            self
        }

        fn pn(self, key: &str, kind: K) -> Fx {
            self.pn_in(P, key, kind, |_| {})
        }

        fn task(self, key: &str, kind: K, op: &str) -> Fx {
            self.pn_in(P, key, kind, |p| p.operation_ref = Some(id(op)))
        }

        fn rel(mut self, kind: RelationKind, from: &str, to: &str) -> Fx {
            let status = self.status;
            self.edges.push(Edge {
                id: id(&format!(
                    "rel:{}-{}-{}",
                    kind.as_str().replace('_', "-"),
                    from.replace(':', "-"),
                    to.replace(':', "-")
                )),
                revision: 1,
                status,
                kind,
                from: id(from),
                to: id(to),
                properties: RelationProperties::None,
                evidence: Vec::new(),
                derivations: Vec::new(),
                standards: Vec::new(),
                audit: meta(),
            });
            self
        }

        /// `next` along a chain of node keys.
        fn chain(mut self, keys: &[&str]) -> Fx {
            for w in keys.windows(2) {
                self = self.rel(
                    RelationKind::Next,
                    &format!("pn:{}", w[0]),
                    &format!("pn:{}", w[1]),
                );
            }
            self
        }

        fn proposed(mut self) -> Fx {
            self.status = Proposed;
            self
        }

        fn graph(&self) -> Graph {
            Graph::new(
                id("project:pilot"),
                id("profile:plumb-software-2026.1"),
                self.nodes.clone(),
                self.edges.clone(),
            )
            .unwrap_or_else(|v| panic!("{v:?}"))
        }

        fn analyze(&self) -> ProcessAnalysis {
            analyze_process(&self.graph(), &id(P), ProcessAnalysisMode::AcceptedOnly).unwrap()
        }
    }

    /// start -> a -> end with a service task on operation:a.
    fn linear() -> Fx {
        Fx::new()
            .pn("start", K::Start)
            .task("a", K::ServiceTask, "operation:a")
            .pn("end", K::End)
            .chain(&["start", "a", "end"])
    }

    fn shapes(a: &ProcessAnalysis) -> Vec<(String, ShapeViolation)> {
        a.node_shape_violations
            .iter()
            .map(|v| (v.node_ref.to_string(), v.violation.clone()))
            .collect()
    }

    fn has_shape(a: &ProcessAnalysis, node: &str, f: fn(&ShapeViolation) -> bool) -> bool {
        a.node_shape_violations
            .iter()
            .any(|v| v.node_ref.as_str() == node && f(&v.violation))
    }

    fn parallel_kinds(a: &ProcessAnalysis) -> Vec<ParallelIssueKind> {
        a.parallel_analysis
            .issues()
            .iter()
            .map(|i| i.kind)
            .collect()
    }

    // ------------------------------------------------------------------ basics

    #[test]
    fn process_analysis_linear_and_errors() {
        let a = linear().analyze();
        assert!(a.is_qualified(), "{a:?}");
        assert_eq!(a.node_refs, ids(&["pn:a", "pn:end", "pn:start"]));
        assert_eq!(a.start_refs, ids(&["pn:start"]));
        assert_eq!(a.end_refs, ids(&["pn:end"]));
        assert_eq!(a.parallel_analysis, ParallelAnalysis::NotApplicable);
        let g = linear().graph();
        assert_eq!(
            analyze_process(&g, &id("process:none"), ProcessAnalysisMode::AcceptedOnly),
            Err(ProcessAnalysisError::NotFound(id("process:none")))
        );
        assert_eq!(
            analyze_process(&g, &id("operation:a"), ProcessAnalysisMode::AcceptedOnly),
            Err(ProcessAnalysisError::NotAProcess(id("operation:a")))
        );
        // An empty Process misses both Start and End.
        let empty =
            analyze_process(&g, &id("process:q"), ProcessAnalysisMode::AcceptedOnly).unwrap();
        assert!(empty.missing_start() && empty.missing_end() && !empty.is_qualified());
    }

    #[test]
    fn process_analysis_modes() {
        // Proposed nodes and edges are invisible to AcceptedOnly and visible to ActiveOverlay.
        let mut fx = linear();
        fx = fx.proposed().task("b", K::ServiceTask, "operation:b");
        fx.edges
            .retain(|e| !(e.from.as_str() == "pn:a" && e.to.as_str() == "pn:end"));
        let fx = fx.chain(&["a", "b", "end"]);
        let g = fx.graph();
        let accepted = analyze_process(&g, &id(P), ProcessAnalysisMode::AcceptedOnly).unwrap();
        assert_eq!(accepted.node_refs, ids(&["pn:a", "pn:end", "pn:start"]));
        assert_eq!(accepted.invalid_terminal_refs, ids(&["pn:a"]));
        assert!(has_shape(&accepted, "pn:end", |v| matches!(
            v,
            ShapeViolation::Indegree { .. }
        )));
        let overlay = analyze_process(&g, &id(P), ProcessAnalysisMode::ActiveOverlay).unwrap();
        assert_eq!(
            overlay.node_refs,
            ids(&["pn:a", "pn:b", "pn:end", "pn:start"])
        );
        assert!(overlay.is_qualified(), "{overlay:?}");
        // A Proposed Process is not analyzable AcceptedOnly.
        let mut proposed = linear();
        proposed.nodes[0].status = Proposed;
        let g = proposed.graph();
        assert_eq!(
            analyze_process(&g, &id(P), ProcessAnalysisMode::AcceptedOnly),
            Err(ProcessAnalysisError::StatusNotIncluded(id(P)))
        );
        assert!(
            analyze_process(&g, &id(P), ProcessAnalysisMode::ActiveOverlay)
                .unwrap()
                .is_qualified()
        );
    }

    // ------------------------------------------------------------------ starts and ends

    #[test]
    fn process_analysis_starts() {
        let start = |f: fn(&mut ProcessNode)| {
            Fx::new()
                .pn_in(P, "start", K::Start, f)
                .task("a", K::ServiceTask, "operation:a")
                .pn("end", K::End)
                .chain(&["start", "a", "end"])
        };
        // Event start consumes its Accepted Event.
        let event = start(|p| p.message_ref = Some(id("event:e")))
            .rel(RelationKind::Consumes, "pn:start", "event:e")
            .analyze();
        assert!(event.is_qualified(), "{event:?}");
        let missing_consumes = start(|p| p.message_ref = Some(id("event:e"))).analyze();
        assert!(has_shape(&missing_consumes, "pn:start", |v| *v
            == ShapeViolation::MissingConsumes));
        let bad_event = start(|p| p.message_ref = Some(id("event:draft"))).analyze();
        assert!(has_shape(&bad_event, "pn:start", |v| *v
            == ShapeViolation::InvalidMessageRef));
        let not_event = start(|p| p.message_ref = Some(id("operation:a"))).analyze();
        assert!(has_shape(&not_event, "pn:start", |v| *v
            == ShapeViolation::InvalidMessageRef));
        // Timer start keeps opaque text.
        assert!(start(|p| p.timer_expr = Some("every weekday 09:00".into()))
            .analyze()
            .is_qualified());
        assert!(has_shape(
            &start(|p| p.timer_expr = Some(" ".into())).analyze(),
            "pn:start",
            |v| *v == ShapeViolation::InvalidTimerExpression
        ));
        // Both triggers.
        let both = start(|p| {
            p.message_ref = Some(id("event:e"));
            p.timer_expr = Some("daily".into());
        })
        .rel(RelationKind::Consumes, "pn:start", "event:e")
        .analyze();
        assert_eq!(
            shapes(&both),
            vec![("pn:start".to_owned(), ShapeViolation::AmbiguousStartTrigger)]
        );
        // Operation on a Start, incoming next, two outgoing.
        assert!(has_shape(
            &start(|p| p.operation_ref = Some(id("operation:a"))).analyze(),
            "pn:start",
            |v| matches!(v, ShapeViolation::UnexpectedField { .. })
        ));
        let incoming = linear()
            .pn("start2", K::Start)
            .chain(&["start2", "start"])
            .analyze();
        assert!(has_shape(&incoming, "pn:start", |v| matches!(
            v,
            ShapeViolation::Indegree { .. }
        )));
        let two_out = linear()
            .task("b", K::ServiceTask, "operation:b")
            .pn("end2", K::End)
            .chain(&["start", "b", "end2"])
            .analyze();
        assert!(has_shape(&two_out, "pn:start", |v| matches!(
            v,
            ShapeViolation::Outdegree { .. }
        )));
        // A Start with performer or produces relations.
        let performer = linear()
            .rel(RelationKind::PerformedBy, "pn:start", "brole:clerk")
            .analyze();
        assert!(has_shape(&performer, "pn:start", |v| matches!(
            v,
            ShapeViolation::UnexpectedRelation { .. }
        )));
        // Multiple Starts and Ends.
        let multi = linear()
            .pn("start2", K::Start)
            .task("b", K::ServiceTask, "operation:b")
            .pn("end2", K::End)
            .chain(&["start2", "b", "end2"])
            .analyze();
        assert!(multi.is_qualified(), "{multi:?}");
        assert_eq!(multi.start_refs, ids(&["pn:start", "pn:start2"]));
        assert_eq!(multi.end_refs, ids(&["pn:end", "pn:end2"]));
    }

    #[test]
    fn process_analysis_ends_terminals_and_reachability() {
        // Missing End: the task is a non-End terminal.
        let no_end = Fx::new()
            .pn("start", K::Start)
            .task("a", K::ServiceTask, "operation:a")
            .chain(&["start", "a"])
            .analyze();
        assert!(no_end.missing_end());
        assert_eq!(no_end.invalid_terminal_refs, ids(&["pn:a"]));
        // Missing Start: everything is unreachable.
        let no_start = Fx::new()
            .task("a", K::ServiceTask, "operation:a")
            .pn("end", K::End)
            .chain(&["a", "end"])
            .analyze();
        assert!(no_start.missing_start());
        assert_eq!(no_start.unreachable_refs, ids(&["pn:a", "pn:end"]));
        // End with an outgoing flow and field.
        let end_out = linear()
            .task("b", K::ServiceTask, "operation:b")
            .pn("end2", K::End)
            .chain(&["end", "b", "end2"])
            .analyze();
        assert!(has_shape(&end_out, "pn:end", |v| matches!(
            v,
            ShapeViolation::Outdegree { .. }
        )));
        let end_field = Fx::new()
            .pn("start", K::Start)
            .task("a", K::ServiceTask, "operation:a")
            .pn_in(P, "end", K::End, |p| p.timer_expr = Some("x".into()))
            .chain(&["start", "a", "end"])
            .analyze();
        assert!(has_shape(&end_field, "pn:end", |v| matches!(
            v,
            ShapeViolation::UnexpectedField { .. }
        )));
        // Unreachable End and unreachable task.
        let unreachable = linear()
            .pn("end2", K::End)
            .task("b", K::ServiceTask, "operation:b")
            .chain(&["b", "end2"])
            .analyze();
        assert_eq!(unreachable.unreachable_refs, ids(&["pn:b", "pn:end2"]));
        assert!(has_shape(&unreachable, "pn:b", |v| matches!(
            v,
            ShapeViolation::Indegree { .. }
        )));
        // Cross-process next is a boundary violation and not part of the flow.
        let cross = linear()
            .pn_in("process:q", "q1", K::End, |_| {})
            .rel(RelationKind::Next, "pn:a", "pn:q1")
            .analyze();
        assert_eq!(cross.boundary_violations.len(), 1);
        assert_eq!(
            (
                cross.boundary_violations[0].from.as_str(),
                cross.boundary_violations[0].to.as_str()
            ),
            ("pn:a", "pn:q1")
        );
        assert!(!cross.node_refs.contains(&id("pn:q1")));
        assert!(!cross.is_qualified());
    }

    // ------------------------------------------------------------------ tasks and waits

    #[test]
    fn process_analysis_tasks() {
        let with = |kind: K, f: fn(&mut ProcessNode)| {
            Fx::new()
                .pn("start", K::Start)
                .pn_in(P, "t", kind, f)
                .pn("end", K::End)
                .chain(&["start", "t", "end"])
        };
        let unresolved = |a: &ProcessAnalysis| {
            a.unresolved_task_refs
                .iter()
                .map(|u| u.issue)
                .collect::<Vec<_>>()
        };
        assert!(with(K::ServiceTask, |p| p.operation_ref =
            Some(id("operation:a")))
        .analyze()
        .is_qualified());
        assert_eq!(
            unresolved(&with(K::ServiceTask, |_| {}).analyze()),
            [TaskResolutionIssue::UnresolvedServiceTask]
        );
        assert_eq!(
            unresolved(
                &with(K::ServiceTask, |p| p.operation_ref =
                    Some(id("operation:draft")))
                .analyze()
            ),
            [TaskResolutionIssue::UnresolvedServiceTask]
        );
        assert_eq!(
            unresolved(&with(K::ServiceTask, |p| p.operation_ref = Some(id("event:e"))).analyze()),
            [TaskResolutionIssue::UnresolvedServiceTask]
        );
        // Operation-backed human task; node-level performer/produces are not allowed there.
        assert!(
            with(K::HumanTask, |p| p.operation_ref = Some(id("operation:a")))
                .analyze()
                .is_qualified()
        );
        let extra = with(K::HumanTask, |p| p.operation_ref = Some(id("operation:a")))
            .rel(RelationKind::PerformedBy, "pn:t", "brole:clerk")
            .analyze();
        assert!(has_shape(&extra, "pn:t", |v| matches!(
            v,
            ShapeViolation::UnexpectedRelation { .. }
        )));
        let service_extra = with(K::ServiceTask, |p| {
            p.operation_ref = Some(id("operation:a"))
        })
        .rel(RelationKind::Produces, "pn:t", "outcome:done")
        .analyze();
        assert!(has_shape(&service_extra, "pn:t", |v| matches!(
            v,
            ShapeViolation::UnexpectedRelation { .. }
        )));
        // Explicit human activity: performer and produced Outcome or Event.
        let human = || with(K::HumanTask, |_| {});
        assert!(human()
            .rel(RelationKind::PerformedBy, "pn:t", "brole:clerk")
            .rel(RelationKind::Produces, "pn:t", "outcome:done")
            .analyze()
            .is_qualified());
        assert!(human()
            .rel(RelationKind::PerformedBy, "pn:t", "actor:bot")
            .rel(RelationKind::Produces, "pn:t", "event:e")
            .analyze()
            .is_qualified());
        assert_eq!(
            unresolved(
                &human()
                    .rel(RelationKind::Produces, "pn:t", "outcome:done")
                    .analyze()
            ),
            [TaskResolutionIssue::UnresolvedHumanResponsibility]
        );
        assert_eq!(
            unresolved(
                &human()
                    .rel(RelationKind::PerformedBy, "pn:t", "brole:clerk")
                    .analyze()
            ),
            [TaskResolutionIssue::UnresolvedHumanOutcome]
        );
        assert_eq!(
            unresolved(&human().analyze()),
            [
                TaskResolutionIssue::UnresolvedHumanResponsibility,
                TaskResolutionIssue::UnresolvedHumanOutcome
            ]
        );
        // Tasks must have exactly one incoming and one outgoing flow.
        let branchy = linear().pn("end2", K::End).chain(&["a", "end2"]).analyze();
        assert!(has_shape(&branchy, "pn:a", |v| matches!(
            v,
            ShapeViolation::Outdegree { .. }
        )));
        let mergy = linear()
            .pn("start2", K::Start)
            .chain(&["start2", "a"])
            .analyze();
        assert!(has_shape(&mergy, "pn:a", |v| matches!(
            v,
            ShapeViolation::Indegree { .. }
        )));
    }

    #[test]
    fn process_analysis_waits() {
        let with = |kind: K, f: fn(&mut ProcessNode)| {
            Fx::new()
                .pn("start", K::Start)
                .pn_in(P, "w", kind, f)
                .pn("end", K::End)
                .chain(&["start", "w", "end"])
        };
        assert!(
            with(K::MessageEvent, |p| p.message_ref = Some(id("event:e")))
                .rel(RelationKind::Consumes, "pn:w", "event:e")
                .analyze()
                .is_qualified()
        );
        assert!(has_shape(
            &with(K::MessageEvent, |p| p.message_ref =
                Some(id("outcome:done")))
            .analyze(),
            "pn:w",
            |v| *v == ShapeViolation::InvalidMessageRef
        ));
        assert!(has_shape(
            &with(K::MessageEvent, |_| {}).analyze(),
            "pn:w",
            |v| matches!(v, ShapeViolation::MissingField { .. })
        ));
        let wrong_degree = with(K::MessageEvent, |p| p.message_ref = Some(id("event:e")))
            .rel(RelationKind::Consumes, "pn:w", "event:e")
            .pn("end2", K::End)
            .chain(&["w", "end2"])
            .analyze();
        assert!(has_shape(&wrong_degree, "pn:w", |v| matches!(
            v,
            ShapeViolation::Outdegree { .. }
        )));
        assert!(with(K::TimerEvent, |p| p.timer_expr = Some("P2D".into()))
            .analyze()
            .is_qualified());
        assert!(has_shape(
            &with(K::TimerEvent, |p| p.timer_expr = Some(String::new())).analyze(),
            "pn:w",
            |v| *v == ShapeViolation::InvalidTimerExpression
        ));
        assert!(has_shape(
            &with(K::TimerEvent, |_| {}).analyze(),
            "pn:w",
            |v| matches!(v, ShapeViolation::MissingField { .. })
        ));
        let timer_degree = with(K::TimerEvent, |p| p.timer_expr = Some("P2D".into()))
            .pn("start2", K::Start)
            .chain(&["start2", "w"])
            .analyze();
        assert!(has_shape(&timer_degree, "pn:w", |v| matches!(
            v,
            ShapeViolation::Indegree { .. }
        )));
    }

    #[test]
    fn process_analysis_unsupported_semantics() {
        for (kind, semantics) in [
            (K::ExclusiveGateway, UnsupportedSemantics::ExclusiveGateway),
            (K::ErrorEvent, UnsupportedSemantics::ErrorEvent),
            (K::Subprocess, UnsupportedSemantics::Subprocess),
        ] {
            let a = Fx::new()
                .pn("start", K::Start)
                .pn("x", kind)
                .pn("end", K::End)
                .chain(&["start", "x", "end"])
                .analyze();
            assert_eq!(
                a.unsupported_nodes,
                vec![UnsupportedNode {
                    node_ref: id("pn:x"),
                    semantics
                }]
            );
            assert!(!a.is_qualified());
        }
        let condition = Fx::new()
            .pn("start", K::Start)
            .pn_in(P, "a", K::ServiceTask, |p| {
                p.operation_ref = Some(id("operation:a"));
                p.condition_expr = Some("amount > 10".into());
            })
            .pn("end", K::End)
            .chain(&["start", "a", "end"])
            .analyze();
        assert_eq!(
            condition.unsupported_nodes,
            vec![UnsupportedNode {
                node_ref: id("pn:a"),
                semantics: UnsupportedSemantics::ConditionExpression
            }]
        );
        assert!(!condition.is_qualified());
    }

    // ------------------------------------------------------------------ parallel

    /// start -> split -> {branches} -> join -> end, each branch a chain of service tasks.
    fn parallel(branches: &[&[&str]]) -> Fx {
        let mut fx = Fx::new()
            .pn("start", K::Start)
            .pn("split", K::ParallelSplit)
            .pn("join", K::ParallelJoin)
            .pn("end", K::End)
            .chain(&["start", "split"])
            .chain(&["join", "end"]);
        for branch in branches {
            for key in *branch {
                fx = fx.task(key, K::ServiceTask, "operation:a");
            }
            let mut chain = vec!["split"];
            chain.extend(branch.iter());
            chain.push("join");
            fx = fx.chain(&chain);
        }
        fx
    }

    #[test]
    fn process_analysis_parallel_balanced() {
        let two = parallel(&[&["a"], &["b"]]).analyze();
        assert!(two.is_qualified(), "{two:?}");
        assert_eq!(
            two.parallel_analysis,
            ParallelAnalysis::Analyzed {
                splits: vec![ParallelSplitAnalysis {
                    split_ref: id("pn:split"),
                    matched_join_ref: Some(id("pn:join")),
                    branch_root_refs: ids(&["pn:a", "pn:b"])
                }],
                issues: vec![]
            }
        );
        let three = parallel(&[&["a", "a2"], &["b"], &["c"]]).analyze();
        assert!(three.is_qualified(), "{three:?}");
        // Branch-order reversal gives identical analysis.
        assert_eq!(parallel(&[&["c"], &["b"], &["a", "a2"]]).analyze(), three);
        let mut reversed = parallel(&[&["a", "a2"], &["b"], &["c"]]);
        reversed.nodes.reverse();
        reversed.edges.reverse();
        assert_eq!(reversed.analyze(), three);
    }

    #[test]
    fn process_analysis_parallel_failures() {
        // Missing join: branches end separately.
        let missing = Fx::new()
            .pn("start", K::Start)
            .pn("split", K::ParallelSplit)
            .task("a", K::ServiceTask, "operation:a")
            .task("b", K::ServiceTask, "operation:b")
            .pn("e1", K::End)
            .pn("e2", K::End)
            .chain(&["start", "split", "a", "e1"])
            .chain(&["split", "b", "e2"])
            .analyze();
        assert_eq!(
            parallel_kinds(&missing),
            [ParallelIssueKind::ParallelJoinMissing]
        );
        // Ambiguous: two incomparable common joins.
        let ambiguous = Fx::new()
            .pn("start", K::Start)
            .pn("split", K::ParallelSplit)
            .pn("s2", K::ParallelSplit)
            .pn("s3", K::ParallelSplit)
            .pn("j1", K::ParallelJoin)
            .pn("j2", K::ParallelJoin)
            .pn("e1", K::End)
            .pn("e2", K::End)
            .chain(&["start", "split", "s2", "j1", "e1"])
            .chain(&["split", "s3", "j2", "e2"])
            .chain(&["s2", "j2"])
            .chain(&["s3", "j1"])
            .analyze();
        assert!(
            parallel_kinds(&ambiguous).contains(&ParallelIssueKind::ParallelJoinAmbiguous),
            "{ambiguous:?}"
        );
        // A branch that terminates before the join leaves no join common to all branches.
        let stops = parallel(&[&["a"], &["b"]])
            .pn("e2", K::End)
            .task("c", K::ServiceTask, "operation:a")
            .chain(&["split", "c", "e2"])
            .analyze();
        assert_eq!(
            parallel_kinds(&stops),
            [
                ParallelIssueKind::ParallelJoinMissing,
                ParallelIssueKind::ParallelJoinUnmatched
            ]
        );
        // A branch region node that cannot reach the matched join.
        let leaks = Fx::new()
            .pn("start", K::Start)
            .pn("split", K::ParallelSplit)
            .pn("join", K::ParallelJoin)
            .pn("end", K::End)
            .pn("e2", K::End)
            .pn("inner", K::ParallelSplit)
            .task("a", K::ServiceTask, "operation:a")
            .task("b", K::ServiceTask, "operation:b")
            .chain(&["start", "split", "inner", "a", "join", "end"])
            .chain(&["inner", "e2"])
            .chain(&["split", "b", "join"])
            .analyze();
        let kinds = parallel_kinds(&leaks);
        assert!(
            kinds.contains(&ParallelIssueKind::ParallelBranchDoesNotJoin),
            "{kinds:?}"
        );
        assert!(
            kinds.contains(&ParallelIssueKind::NestedParallelUnsupported),
            "{kinds:?}"
        );
        // Branches merge before the join.
        let early = parallel(&[&["a", "m"], &["b"]]);
        let early = early.chain(&["b", "m"]).analyze();
        assert!(
            parallel_kinds(&early).contains(&ParallelIssueKind::ParallelBranchesMergeEarly),
            "{early:?}"
        );
        // Unmatched join.
        let unmatched = linear().pn("j", K::ParallelJoin).analyze();
        assert!(parallel_kinds(&unmatched).contains(&ParallelIssueKind::ParallelJoinUnmatched));
        // Split and join degrees.
        let one_branch = parallel(&[&["a"]]).analyze();
        assert!(has_shape(&one_branch, "pn:split", |v| matches!(
            v,
            ShapeViolation::Outdegree { .. }
        )));
        assert!(has_shape(&one_branch, "pn:join", |v| matches!(
            v,
            ShapeViolation::Indegree { .. }
        )));
        let split_in = parallel(&[&["a"], &["b"]])
            .pn("start2", K::Start)
            .chain(&["start2", "split"])
            .analyze();
        assert!(has_shape(&split_in, "pn:split", |v| matches!(
            v,
            ShapeViolation::Indegree { .. }
        )));
        let join_out = parallel(&[&["a"], &["b"]])
            .pn("end2", K::End)
            .chain(&["join", "end2"])
            .analyze();
        assert!(has_shape(&join_out, "pn:join", |v| matches!(
            v,
            ShapeViolation::Outdegree { .. }
        )));
        // Nested parallel.
        let nested = Fx::new()
            .pn("start", K::Start)
            .pn("split", K::ParallelSplit)
            .pn("join", K::ParallelJoin)
            .pn("s2", K::ParallelSplit)
            .pn("j2", K::ParallelJoin)
            .pn("end", K::End)
            .task("a", K::ServiceTask, "operation:a")
            .task("b", K::ServiceTask, "operation:b")
            .task("c", K::ServiceTask, "operation:a")
            .chain(&["start", "split", "s2", "a", "j2", "join", "end"])
            .chain(&["s2", "b", "j2"])
            .chain(&["split", "c", "join"])
            .analyze();
        assert!(
            parallel_kinds(&nested).contains(&ParallelIssueKind::NestedParallelUnsupported),
            "{nested:?}"
        );
        // Parallel with a next cycle.
        let cyclic = parallel(&[&["a"], &["b", "c"]])
            .chain(&["c", "b"])
            .analyze();
        assert_eq!(
            parallel_kinds(&cyclic),
            [ParallelIssueKind::ParallelCyclicFlow]
        );
    }
}
