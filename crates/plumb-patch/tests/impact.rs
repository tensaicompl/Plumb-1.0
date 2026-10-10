//! F0.11 contract tests for `compute_impact`: ChangedSet, DirtySet, directed traversal,
//! affected projections and affected gate namespaces.
//!
//! Every test lives in `impact_contract` so that `cargo test -p plumb-patch impact` selects them.

mod impact_contract {
    use std::collections::BTreeSet;

    use plumb_core::{to_canonical_json, GateId, Id};
    use plumb_patch::*;
    use plumb_psg::*;
    use serde_json::{json, Value};

    const T1: &str = "2026-09-29T08:00:00.000000000Z";
    const T2: &str = "2026-09-30T08:00:00.000000000Z";
    const PROJECT: &str = "project:leave-management";
    const PROFILE: &str = "profile:plumb-software-2026.1";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ids(v: &[&str]) -> BTreeSet<Id> {
        v.iter().map(|s| id(s)).collect()
    }

    fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{e}: {value}"))
    }

    fn audit() -> Value {
        json!({"created_by": "actor:analyst", "created_at": T1, "updated_by": null, "updated_at": null})
    }

    fn node(node_id: &str, status: &str, tag: &str, data: Value) -> Node {
        from_json(json!({
            "id": node_id, "revision": 1, "status": status,
            "payload": {"type": tag, "data": data},
            "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
            "audit": audit()
        }))
    }

    fn edge(edge_id: &str, status: &str, kind: &str, from: &str, to: &str) -> Edge {
        from_json(json!({
            "id": edge_id, "revision": 1, "status": status, "kind": kind, "from": from, "to": to,
            "properties": {}, "evidence": [], "derivations": [], "standards": [], "audit": audit()
        }))
    }

    fn arch(name: &str) -> Value {
        json!({"name": name})
    }

    /// A node of the given type with a minimal valid payload.
    fn typed(node_id: &str, status: &str, tag: &str) -> Node {
        let data = match tag {
            "Requirement" => json!({
                "statement": format!("The system shall {node_id}."), "requirement_kind": "functional",
                "level": "system", "modality": "shall"
            }),
            "Constraint" => json!({
                "statement": "Leave data must stay in the EU.",
                "constraint_category": "regulatory", "strength": "mandatory"
            }),
            "Operation" => json!({"name": "ApproveLeaveRequest", "operation_kind": "command"}),
            "Entity" => json!({"name": "LeaveRequest"}),
            "Attribute" => json!({"name": "days", "value_type": "decimal", "nullable": false}),
            "Rule" => json!({"name": "Maximum consecutive days", "rule_kind": "validation"}),
            "DecisionTable" => json!({
                "name": "Approval routing", "hit_policy": "UNIQUE",
                "inputs": [{"name": "days", "type": "decimal"}],
                "outputs": [{"name": "approver", "type": "text"}],
                "rows": [{"when": ["<= 5"], "then": ["manager"]}]
            }),
            "Calculation" => json!({
                "name": "LeaveDays", "expression": "working_days(period)", "result_type": "decimal"
            }),
            "Process" => json!({"name": "Leave approval", "process_kind": "approval"}),
            "Component" => arch("Approval service"),
            "ApiOperation" => json!({
                "api_contract_ref": "api:hr:leave", "operation_id": "approveLeave",
                "method": "POST", "path": "/leave/{id}/approve"
            }),
            "ImplementationSlice" => {
                json!({"name": "Slice 1", "intent": "Submit and approve leave"})
            }
            "VerificationObligation" => {
                json!({"name": "Verify approval", "verification_kind": "scenario"})
            }
            "TestCase" => json!({"name": "Approve happy path", "steps": [], "expected": []}),
            "Scenario" => json!({
                "name": "Half-day request", "scenario_kind": "boundary", "given": [],
                "when": {"operation": "op:approve"}, "then": []
            }),
            "View" => json!({
                "name": "Leave process", "viewpoint_ref": "vp:hr:functional",
                "architecture_description_ref": "ad:hr:leave", "root_refs": null, "filter": null,
                "projection_rules": [], "layout_ref": null, "style_ref": null
            }),
            "Extension" => {
                json!({"extension_type": "acme:risk_register", "data": {"risk": "high"}})
            }
            "DerivationRecord" => json!({
                "id": node_id, "kind": "parser", "stage": "S0.import",
                "input_refs": [], "output_refs": [], "created_at": T1
            }),
            other => panic!("no fixture for {other}"),
        };
        node(node_id, status, tag, data)
    }

    fn graph(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn pre(g: &Graph, element: &str) -> ElementPrecondition {
        let expected_hash = match g.node(&id(element)) {
            Some(n) => node_element_hash(n).unwrap(),
            None => edge_element_hash(g.edge(&id(element)).unwrap()).unwrap(),
        };
        ElementPrecondition {
            id: id(element),
            expected_hash,
        }
    }

    fn set_status(
        g: &Graph,
        element: &str,
        from: ElementStatus,
        to: ElementStatus,
    ) -> SemanticPatch {
        SemanticPatch::SetStatus {
            target: pre(g, element),
            from,
            to,
        }
    }

    /// Accepted -> Suspect: a semantic change that keeps the element baseline.
    fn suspect(g: &Graph, element: &str) -> SemanticPatch {
        set_status(g, element, ElementStatus::Accepted, ElementStatus::Suspect)
    }

    fn apply(base: &Graph, patch: SemanticPatch) -> (Graph, GraphDelta) {
        let result = apply_patch(
            base,
            &PatchSet {
                base_semantic_hash: base.semantic_hash().unwrap(),
                patch,
            },
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        (result.graph, result.delta)
    }

    fn impact(base: &Graph, patch: SemanticPatch) -> ImpactReport {
        let (result, delta) = apply(base, patch);
        compute_impact(base, &result, &delta).unwrap()
    }

    /// A hand-built delta for persisted edits that no patch operation can express.
    fn manual_delta(base: &Graph, result: &Graph) -> GraphDelta {
        fn net<T: PartialEq>(
            b: &std::collections::BTreeMap<Id, T>,
            r: &std::collections::BTreeMap<Id, T>,
        ) -> (BTreeSet<Id>, BTreeSet<Id>, BTreeSet<Id>) {
            (
                r.keys().filter(|k| !b.contains_key(*k)).cloned().collect(),
                b.keys().filter(|k| !r.contains_key(*k)).cloned().collect(),
                b.iter()
                    .filter(|(k, v)| r.get(*k).is_some_and(|w| w != *v))
                    .map(|(k, _)| k.clone())
                    .collect(),
            )
        }
        let (added_nodes, removed_nodes, modified_nodes) = net(base.nodes(), result.nodes());
        let (added_edges, removed_edges, modified_edges) = net(base.edges(), result.edges());
        GraphDelta {
            base_semantic_hash: base.semantic_hash().unwrap(),
            result_semantic_hash: result.semantic_hash().unwrap(),
            touched_nodes: modified_nodes.clone(),
            touched_edges: modified_edges.clone(),
            added_nodes,
            removed_nodes,
            modified_nodes,
            added_edges,
            removed_edges,
            modified_edges,
            diff: vec![],
        }
    }

    fn rebuild(g: &Graph, edit: impl FnOnce(&mut Vec<Node>, &mut Vec<Edge>)) -> Graph {
        let mut nodes: Vec<Node> = g.nodes().values().cloned().collect();
        let mut edges: Vec<Edge> = g.edges().values().cloned().collect();
        edit(&mut nodes, &mut edges);
        graph(nodes, edges)
    }

    fn projections(kinds: &[&str]) -> BTreeSet<ProjectionKind> {
        kinds.iter().map(|k| k.parse().unwrap()).collect()
    }

    fn gates_from(first: GateId) -> BTreeSet<GateId> {
        GateId::ALL.into_iter().skip(first.ordinal()).collect()
    }

    /// Calculation <-uses_calculation- Operation -verified_by-> VerificationObligation
    /// -implemented_as-> Scenario, plus an unrelated requirement.
    fn chain() -> Graph {
        graph(
            vec![
                typed("calc:leave-days", "Accepted", "Calculation"),
                typed("op:approve", "Accepted", "Operation"),
                typed("verify:approval", "Accepted", "VerificationObligation"),
                typed("scn:half-day", "Accepted", "Scenario"),
                typed("req:unrelated", "Accepted", "Requirement"),
            ],
            vec![
                edge(
                    "rel:uses",
                    "Accepted",
                    "uses_calculation",
                    "op:approve",
                    "calc:leave-days",
                ),
                edge(
                    "rel:verified",
                    "Accepted",
                    "verified_by",
                    "op:approve",
                    "verify:approval",
                ),
                edge(
                    "rel:impl",
                    "Accepted",
                    "implemented_as",
                    "verify:approval",
                    "scn:half-day",
                ),
            ],
        )
    }

    // ------------------------------------------------------------------ ChangedSet (§34)

    #[test]
    fn changed_set_from_real_patches() {
        let base = graph(
            vec![
                typed("req:a", "Accepted", "Requirement"),
                typed("con:eu", "Accepted", "Constraint"),
                typed("req:p", "Proposed", "Requirement"),
            ],
            vec![edge(
                "rel:a-eu",
                "Accepted",
                "constrained_by",
                "req:a",
                "con:eu",
            )],
        );
        // Add a baseline node.
        let r = impact(
            &base,
            SemanticPatch::AddNode {
                node: typed("req:new", "Accepted", "Requirement"),
            },
        );
        assert_eq!(r.changed.node_ids, ids(&["req:new"]));
        // Remove a baseline node (after its edge) and the edge itself.
        let r = impact(
            &base,
            SemanticPatch::Compound {
                patches: vec![
                    SemanticPatch::RemoveEdge {
                        target: pre(&base, "rel:a-eu"),
                    },
                    SemanticPatch::RemoveNode {
                        target: pre(&base, "req:a"),
                    },
                ],
            },
        );
        assert_eq!(r.changed.node_ids, ids(&["req:a"]));
        assert_eq!(r.changed.edge_ids, ids(&["rel:a-eu"]));
        // Semantic node modification.
        let r = impact(&base, suspect(&base, "req:a"));
        assert_eq!(r.changed.node_ids, ids(&["req:a"]));
        // Add an edge.
        let r = impact(
            &base,
            SemanticPatch::AddEdge {
                edge: edge("rel:p-eu", "Proposed", "constrained_by", "req:p", "con:eu"),
            },
        );
        assert_eq!(r.changed.edge_ids, ids(&["rel:p-eu"]));
        // Semantic edge modification.
        let r = impact(&base, suspect(&base, "rel:a-eu"));
        assert_eq!(r.changed.edge_ids, ids(&["rel:a-eu"]));
        // A Proposed semantic change is still changed.
        let r = impact(
            &base,
            SemanticPatch::ReplacePayload {
                target: pre(&base, "req:p"),
                payload: typed("req:p", "Proposed", "Requirement")
                    .payload
                    .clone()
                    .pipe_changed(),
            },
        );
        assert_eq!(r.changed.node_ids, ids(&["req:p"]));
        assert!(r.dirty.node_ids.is_empty());
    }

    /// An in-place edit of a graph's persisted nodes and edges.
    type Edit = dyn Fn(&mut Vec<Node>, &mut Vec<Edge>);

    trait PipeChanged {
        fn pipe_changed(self) -> Self;
    }

    impl PipeChanged for NodePayload {
        /// The same requirement payload with a different statement.
        fn pipe_changed(self) -> Self {
            let mut value = serde_json::to_value(&self).unwrap();
            value["data"]["statement"] = json!("The system shall do something else.");
            from_json(value)
        }
    }

    #[test]
    fn non_semantic_persisted_changes_are_not_changed() {
        let base = graph(
            vec![
                typed("req:a", "Accepted", "Requirement"),
                typed("con:eu", "Accepted", "Constraint"),
                typed("drv:parse", "Accepted", "DerivationRecord"),
            ],
            vec![edge(
                "rel:a-eu",
                "Accepted",
                "constrained_by",
                "req:a",
                "con:eu",
            )],
        );
        let edits: Vec<(&str, Box<Edit>)> = vec![
            (
                "node audit",
                Box::new(|n, _| {
                    let r = n.iter_mut().find(|x| x.id == id("req:a")).unwrap();
                    r.audit = from_json(
                        json!({"created_by": "actor:analyst", "created_at": T1, "updated_by": "actor:other", "updated_at": T2}),
                    );
                }),
            ),
            (
                "node derivations",
                Box::new(|n, _| {
                    let r = n.iter_mut().find(|x| x.id == id("req:a")).unwrap();
                    r.derivations = vec![DerivationRef::from(id("drv:parse"))];
                }),
            ),
            (
                "node revision",
                Box::new(|n, _| n.iter_mut().find(|x| x.id == id("req:a")).unwrap().revision = 7),
            ),
            (
                "edge audit",
                Box::new(|_, e| {
                    e[0].audit = from_json(
                        json!({"created_by": "actor:analyst", "created_at": T1, "updated_by": "actor:other", "updated_at": T2}),
                    );
                }),
            ),
            (
                "edge derivations",
                Box::new(|_, e| e[0].derivations = vec![DerivationRef::from(id("drv:parse"))]),
            ),
        ];
        for (name, edit) in edits {
            let result = rebuild(&base, |n, e| edit(n, e));
            let delta = manual_delta(&base, &result);
            assert_eq!(
                delta.modified_nodes.len() + delta.modified_edges.len(),
                1,
                "{name} is a persisted change"
            );
            let r = compute_impact(&base, &result, &delta).unwrap();
            assert_eq!(r, ImpactReport::default(), "{name}");
        }
    }

    #[test]
    fn view_layout_only_changes_nothing_but_semantic_view_change_does() {
        let base = graph(vec![typed("view:leave", "Accepted", "View")], vec![]);
        let view = base.node(&id("view:leave")).unwrap();
        let mut layout: Value = serde_json::to_value(&view.payload).unwrap();
        layout["data"]["layout_ref"] = json!(format!("sha256:{}", "4".repeat(64)));
        layout["data"]["style_ref"] = json!(format!("sha256:{}", "5".repeat(64)));
        let (result, delta) = apply(
            &base,
            SemanticPatch::ReplacePayload {
                target: pre(&base, "view:leave"),
                payload: from_json(layout),
            },
        );
        assert_eq!(delta.modified_nodes, ids(&["view:leave"]));
        let r = compute_impact(&base, &result, &delta).unwrap();
        assert!(r.changed.node_ids.is_empty());
        assert!(r.dirty.node_ids.is_empty());
        assert!(r.affected_projections.projections.is_empty());
        assert!(r.affected_gates.gates.is_empty());

        let mut named: Value = serde_json::to_value(&view.payload).unwrap();
        named["data"]["name"] = json!("Leave process v2");
        let r = impact(
            &base,
            SemanticPatch::ReplacePayload {
                target: pre(&base, "view:leave"),
                payload: from_json(named),
            },
        );
        assert_eq!(r.changed.node_ids, ids(&["view:leave"]));
        assert_eq!(r.dirty.node_ids, ids(&["view:leave"]));
        assert_eq!(
            r.affected_projections.projections,
            projections(&[
                "architecture.yaml",
                "diagrams",
                "markdown",
                "standards-conformance-report"
            ])
        );
        assert_eq!(r.affected_gates.gates, gates_from(GateId::A2));
    }

    #[test]
    fn modify_then_restore_is_not_changed() {
        let base = graph(vec![typed("req:a", "Accepted", "Requirement")], vec![]);
        let original = base.node(&id("req:a")).unwrap().payload.clone();
        let (mid, _) = apply(
            &base,
            SemanticPatch::ReplacePayload {
                target: pre(&base, "req:a"),
                payload: original.clone().pipe_changed(),
            },
        );
        let r = impact(
            &base,
            SemanticPatch::Compound {
                patches: vec![
                    SemanticPatch::ReplacePayload {
                        target: pre(&base, "req:a"),
                        payload: original.clone().pipe_changed(),
                    },
                    SemanticPatch::ReplacePayload {
                        target: pre(&mid, "req:a"),
                        payload: original,
                    },
                ],
            },
        );
        assert_eq!(r, ImpactReport::default());
    }

    #[test]
    fn inconsistent_inputs_are_rejected() {
        let base = chain();
        let (result, delta) = apply(&base, suspect(&base, "calc:leave-days"));
        let mut bad = delta.clone();
        bad.modified_nodes.clear();
        assert_eq!(
            compute_impact(&base, &result, &bad),
            Err(ImpactError::DeltaGraphMismatch {
                set: "modified_nodes"
            })
        );
        let mut bad = delta.clone();
        bad.added_edges.insert(id("rel:ghost"));
        assert_eq!(
            compute_impact(&base, &result, &bad),
            Err(ImpactError::DeltaGraphMismatch { set: "added_edges" })
        );
        assert!(matches!(
            compute_impact(&result, &result, &delta),
            Err(ImpactError::BaseHashMismatch { .. })
        ));
        assert!(matches!(
            compute_impact(&base, &base, &delta),
            Err(ImpactError::ResultHashMismatch { .. })
        ));
        let other_project = Graph::new(
            id("project:other"),
            id(PROFILE),
            result.nodes().values().cloned().collect(),
            result.edges().values().cloned().collect(),
        )
        .unwrap();
        let mut d = delta.clone();
        d.result_semantic_hash = other_project.semantic_hash().unwrap();
        assert!(matches!(
            compute_impact(&base, &other_project, &d),
            Err(ImpactError::ProjectMismatch { .. })
        ));
        let other_profile = Graph::new(
            id(PROJECT),
            id("profile:other"),
            result.nodes().values().cloned().collect(),
            result.edges().values().cloned().collect(),
        )
        .unwrap();
        let mut d = delta;
        d.result_semantic_hash = other_profile.semantic_hash().unwrap();
        assert!(matches!(
            compute_impact(&base, &other_profile, &d),
            Err(ImpactError::ProfileMismatch { .. })
        ));
    }

    // ------------------------------------------------------------------ DirtySet status (§35)

    #[test]
    fn baseline_statuses_seed_the_dirty_set() {
        for status in ["Accepted", "Suspect", "Superseded", "Deprecated"] {
            let base = graph(vec![typed("req:a", status, "Requirement")], vec![]);
            let payload = base
                .node(&id("req:a"))
                .unwrap()
                .payload
                .clone()
                .pipe_changed();
            let r = impact(
                &base,
                SemanticPatch::ReplacePayload {
                    target: pre(&base, "req:a"),
                    payload,
                },
            );
            assert_eq!(r.dirty.node_ids, ids(&["req:a"]), "{status}");
        }
        for status in ["Proposed", "Rejected"] {
            let base = graph(vec![typed("req:a", status, "Requirement")], vec![]);
            let payload = base
                .node(&id("req:a"))
                .unwrap()
                .payload
                .clone()
                .pipe_changed();
            let r = impact(
                &base,
                SemanticPatch::ReplacePayload {
                    target: pre(&base, "req:a"),
                    payload,
                },
            );
            assert_eq!(r.changed.node_ids, ids(&["req:a"]), "{status}");
            assert!(r.dirty.node_ids.is_empty(), "{status}");
            assert!(r.affected_gates.gates.is_empty(), "{status}");
        }
    }

    #[test]
    fn crossing_the_baseline_boundary_seeds_the_dirty_set() {
        use ElementStatus::*;
        for (from, to) in [
            (Proposed, Accepted),
            (Accepted, Rejected),
            (Rejected, Suspect),
        ] {
            let from_text = serde_json::to_value(from).unwrap();
            let base = graph(
                vec![typed("req:a", from_text.as_str().unwrap(), "Requirement")],
                vec![],
            );
            let r = impact(&base, set_status(&base, "req:a", from, to));
            assert_eq!(r.dirty.node_ids, ids(&["req:a"]), "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn changed_edges_seed_their_endpoints() {
        let base = graph(
            vec![
                typed("req:a", "Accepted", "Requirement"),
                typed("req:b", "Accepted", "Requirement"),
                typed("con:eu", "Accepted", "Constraint"),
                typed("req:p", "Proposed", "Requirement"),
            ],
            vec![
                edge("rel:a-eu", "Accepted", "constrained_by", "req:a", "con:eu"),
                edge("rel:p-eu", "Proposed", "constrained_by", "req:p", "con:eu"),
            ],
        );
        let r = impact(&base, suspect(&base, "rel:a-eu"));
        assert_eq!(r.dirty.edge_ids, ids(&["rel:a-eu"]));
        assert_eq!(r.dirty.node_ids, ids(&["con:eu", "req:a"]));
        // Retargeting: old and new endpoints are both seeds.
        let r = impact(
            &base,
            SemanticPatch::ReplaceEdge {
                target: pre(&base, "rel:a-eu"),
                kind: RelationKind::ConstrainedBy,
                from: id("req:b"),
                to: id("con:eu"),
                properties: RelationProperties::None,
            },
        );
        assert_eq!(r.dirty.node_ids, ids(&["con:eu", "req:a", "req:b"]));
        // A Proposed edge change seeds nothing.
        let r = impact(
            &base,
            set_status(
                &base,
                "rel:p-eu",
                ElementStatus::Proposed,
                ElementStatus::Rejected,
            ),
        );
        assert_eq!(r.changed.edge_ids, ids(&["rel:p-eu"]));
        assert_eq!(r.dirty, DirtySet::default());
    }

    #[test]
    fn removed_baseline_nodes_stay_dirty_and_dirty_survivors() {
        let base = chain();
        let (result, delta) = apply(
            &base,
            SemanticPatch::Compound {
                patches: vec![
                    SemanticPatch::RemoveEdge {
                        target: pre(&base, "rel:uses"),
                    },
                    SemanticPatch::RemoveNode {
                        target: pre(&base, "calc:leave-days"),
                    },
                ],
            },
        );
        assert!(result.node(&id("calc:leave-days")).is_none());
        let r = compute_impact(&base, &result, &delta).unwrap();
        assert_eq!(r.changed.node_ids, ids(&["calc:leave-days"]));
        assert_eq!(
            r.dirty.node_ids,
            ids(&[
                "calc:leave-days",
                "op:approve",
                "scn:half-day",
                "verify:approval"
            ])
        );
        assert_eq!(r.dirty.edge_ids, ids(&["rel:uses"]));
    }

    // ------------------------------------------------------------------ traversal (§36)

    /// `(relation, from type, to type, direction)` for all 13 impact relations.
    const RELATIONS: [(&str, &str, &str, ImpactDirection); 13] = [
        (
            "derived_from",
            "Requirement",
            "Requirement",
            ImpactDirection::ToFrom,
        ),
        (
            "specified_by",
            "Requirement",
            "Operation",
            ImpactDirection::FromTo,
        ),
        (
            "constrained_by",
            "Requirement",
            "Constraint",
            ImpactDirection::ToFrom,
        ),
        (
            "satisfied_by",
            "Requirement",
            "Component",
            ImpactDirection::FromTo,
        ),
        ("reads", "Operation", "Entity", ImpactDirection::ToFrom),
        ("writes", "Operation", "Entity", ImpactDirection::ToFrom),
        ("governed_by", "Operation", "Rule", ImpactDirection::ToFrom),
        (
            "uses_calculation",
            "Operation",
            "Calculation",
            ImpactDirection::ToFrom,
        ),
        (
            "allocated_to",
            "Operation",
            "Component",
            ImpactDirection::FromTo,
        ),
        (
            "exposed_by",
            "Operation",
            "ApiOperation",
            ImpactDirection::FromTo,
        ),
        (
            "implemented_by",
            "Requirement",
            "ImplementationSlice",
            ImpactDirection::FromTo,
        ),
        (
            "verified_by",
            "Requirement",
            "VerificationObligation",
            ImpactDirection::FromTo,
        ),
        (
            "implemented_as",
            "VerificationObligation",
            "TestCase",
            ImpactDirection::FromTo,
        ),
    ];

    #[test]
    fn each_impact_relation_propagates_in_exactly_one_direction() {
        for (relation, from_type, to_type, direction) in RELATIONS {
            let kind: RelationKind = relation.parse().unwrap();
            assert_eq!(impact_direction(&kind), Some(direction), "{relation}");
            let base = graph(
                vec![
                    typed("x:from", "Accepted", from_type),
                    typed("x:to", "Accepted", to_type),
                ],
                vec![edge("rel:x", "Accepted", relation, "x:from", "x:to")],
            );
            let from_change = impact(&base, suspect(&base, "x:from")).dirty.node_ids;
            let to_change = impact(&base, suspect(&base, "x:to")).dirty.node_ids;
            match direction {
                ImpactDirection::FromTo => {
                    assert_eq!(from_change, ids(&["x:from", "x:to"]), "{relation} forward");
                    assert_eq!(to_change, ids(&["x:to"]), "{relation} reverse");
                }
                ImpactDirection::ToFrom => {
                    assert_eq!(to_change, ids(&["x:from", "x:to"]), "{relation} forward");
                    assert_eq!(from_change, ids(&["x:from"]), "{relation} reverse");
                }
            }
        }
    }

    #[test]
    fn only_the_13_relations_are_traversed() {
        let listed: BTreeSet<&str> = RELATIONS.iter().map(|r| r.0).collect();
        assert_eq!(RELATION_REGISTRY.len(), 51);
        for kind in RELATION_REGISTRY.iter().map(|def| &def.kind) {
            assert_eq!(
                impact_direction(kind).is_some(),
                listed.contains(kind.as_str()),
                "{}",
                kind.as_str()
            );
        }
    }

    #[test]
    fn calculation_change_dirties_operation_obligation_and_scenario() {
        let base = chain();
        let r = impact(&base, suspect(&base, "calc:leave-days"));
        assert_eq!(r.changed.node_ids, ids(&["calc:leave-days"]));
        assert_eq!(
            r.dirty.node_ids,
            ids(&[
                "calc:leave-days",
                "op:approve",
                "scn:half-day",
                "verify:approval"
            ])
        );
        assert!(r.dirty.edge_ids.is_empty());
    }

    #[test]
    fn base_and_result_topology_both_count() {
        // A newly added dependency propagates through the result graph.
        let base = graph(
            vec![
                typed("calc:new", "Accepted", "Calculation"),
                typed("op:approve", "Accepted", "Operation"),
                typed("verify:approval", "Accepted", "VerificationObligation"),
            ],
            vec![edge(
                "rel:verified",
                "Accepted",
                "verified_by",
                "op:approve",
                "verify:approval",
            )],
        );
        let r = impact(
            &base,
            SemanticPatch::AddEdge {
                edge: edge(
                    "rel:uses",
                    "Accepted",
                    "uses_calculation",
                    "op:approve",
                    "calc:new",
                ),
            },
        );
        assert_eq!(
            r.dirty.node_ids,
            ids(&["calc:new", "op:approve", "verify:approval"])
        );
        // A dependency removed in the result is still considered from the base.
        let base = chain();
        let r = impact(
            &base,
            SemanticPatch::Compound {
                patches: vec![
                    SemanticPatch::RemoveEdge {
                        target: pre(&base, "rel:verified"),
                    },
                    suspect(&base, "op:approve"),
                ],
            },
        );
        assert!(r.dirty.node_ids.contains(&id("verify:approval")));
        assert!(r.dirty.node_ids.contains(&id("scn:half-day")));
    }

    #[test]
    fn proposed_and_rejected_dependencies_do_not_propagate() {
        for status in ["Proposed", "Rejected"] {
            let base = graph(
                vec![
                    typed("calc:leave-days", "Accepted", "Calculation"),
                    typed("op:approve", "Accepted", "Operation"),
                ],
                vec![edge(
                    "rel:uses",
                    status,
                    "uses_calculation",
                    "op:approve",
                    "calc:leave-days",
                )],
            );
            let r = impact(&base, suspect(&base, "calc:leave-days"));
            assert_eq!(r.dirty.node_ids, ids(&["calc:leave-days"]), "{status}");
        }
    }

    #[test]
    fn unlisted_relations_are_not_traversed_but_their_changes_dirty_endpoints() {
        let base = graph(
            vec![
                typed("ent:leave", "Accepted", "Entity"),
                typed("attr:days", "Accepted", "Attribute"),
            ],
            vec![edge(
                "rel:has",
                "Accepted",
                "has_attribute",
                "ent:leave",
                "attr:days",
            )],
        );
        assert_eq!(
            impact(&base, suspect(&base, "attr:days")).dirty.node_ids,
            ids(&["attr:days"])
        );
        assert_eq!(
            impact(&base, suspect(&base, "ent:leave")).dirty.node_ids,
            ids(&["ent:leave"])
        );
        let r = impact(&base, suspect(&base, "rel:has"));
        assert_eq!(r.dirty.edge_ids, ids(&["rel:has"]));
        assert_eq!(r.dirty.node_ids, ids(&["attr:days", "ent:leave"]));
    }

    // ------------------------------------------------------------------ cycles and determinism (§37)

    #[test]
    fn cycles_terminate_and_reports_are_deterministic() {
        // req:a -specified_by-> op (a -> op) and req:a -derived_from-> op (op -> a).
        let nodes = vec![
            typed("req:a", "Accepted", "Requirement"),
            typed("op:approve", "Accepted", "Operation"),
            typed("req:other", "Accepted", "Requirement"),
        ];
        let edges = vec![
            edge(
                "rel:spec",
                "Accepted",
                "specified_by",
                "req:a",
                "op:approve",
            ),
            edge(
                "rel:derived",
                "Accepted",
                "derived_from",
                "req:a",
                "op:approve",
            ),
        ];
        let base = graph(nodes.clone(), edges.clone());
        let r = impact(&base, suspect(&base, "req:a"));
        assert_eq!(r.dirty.node_ids, ids(&["op:approve", "req:a"]));
        let again = impact(&base, suspect(&base, "req:a"));
        assert_eq!(
            to_canonical_json(&r).unwrap(),
            to_canonical_json(&again).unwrap()
        );
        let reversed = graph(
            nodes.into_iter().rev().collect(),
            edges.into_iter().rev().collect(),
        );
        let from_reversed = impact(&reversed, suspect(&reversed, "req:a"));
        assert_eq!(
            serde_json::to_vec(&from_reversed).unwrap(),
            serde_json::to_vec(&r).unwrap()
        );
    }

    // ------------------------------------------------------------------ projections (§38)

    fn full_projections(node_type: NodeType) -> BTreeSet<ProjectionKind> {
        let mut set = projections(&["markdown", "standards-conformance-report"]);
        set.extend(projection_families(node_type));
        set
    }

    #[test]
    fn projection_kinds_are_exact() {
        let expected = [
            "functional.yaml",
            "requirements.yaml",
            "architecture.yaml",
            "openapi",
            "asyncapi",
            "arazzo",
            "bpmn",
            "dmn",
            "adrs",
            "implementation-plan",
            "task-contracts",
            "verification-matrix",
            "standards-conformance-report",
            "markdown",
            "diagrams",
        ];
        assert_eq!(ProjectionKind::ALL.map(ProjectionKind::as_str), expected);
        for (kind, text) in ProjectionKind::ALL.into_iter().zip(expected) {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(text));
            assert_eq!(
                serde_json::from_value::<ProjectionKind>(json!(text)).unwrap(),
                kind
            );
        }
        for bad in ["functional_yaml", "OpenApi", "json", ""] {
            assert!(bad.parse::<ProjectionKind>().is_err());
        }
    }

    #[test]
    fn projection_mappings_are_exact() {
        use NodeType as T;
        let cases: [(NodeType, &[&str]); 8] = [
            (T::Requirement, &["requirements.yaml", "functional.yaml"]),
            (T::Process, &["functional.yaml", "bpmn", "diagrams"]),
            (T::DecisionTable, &["functional.yaml", "dmn"]),
            (
                T::ApiOperation,
                &["architecture.yaml", "openapi", "arazzo", "diagrams"],
            ),
            (T::Message, &["architecture.yaml", "asyncapi", "diagrams"]),
            (T::ArchitectureDecision, &["architecture.yaml", "adrs"]),
            (
                T::ImplementationSlice,
                &["implementation-plan", "task-contracts"],
            ),
            (T::VerificationObligation, &["verification-matrix"]),
        ];
        for (node_type, families) in cases {
            let mut expected = projections(families);
            expected.extend(projections(&["markdown", "standards-conformance-report"]));
            assert_eq!(
                full_projections(node_type),
                expected,
                "{}",
                node_type.as_str()
            );
        }
        // One representative per remaining family.
        assert!(projection_families(T::EventContract).contains(&ProjectionKind::AsyncApi));
        assert!(projection_families(T::TechnicalWorkflow).contains(&ProjectionKind::Arazzo));
        assert!(projection_families(T::WorkPackage).contains(&ProjectionKind::TaskContracts));
        assert!(projection_families(T::Release).contains(&ProjectionKind::ImplementationPlan));
        assert!(projection_families(T::TestReceipt).contains(&ProjectionKind::VerificationMatrix));
        assert!(projection_families(T::Question).contains(&ProjectionKind::RequirementsYaml));
        assert!(projection_families(T::Technology).contains(&ProjectionKind::Adrs));
        for provenance in [
            T::SourceArtifact,
            T::EvidenceFragment,
            T::DerivationRecord,
            T::Agent,
        ] {
            assert!(projection_families(provenance).is_empty());
        }
        assert_eq!(
            projection_families(T::Extension)
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            ProjectionKind::ALL.into_iter().collect()
        );
    }

    #[test]
    fn end_to_end_projection_sets() {
        let base = graph(vec![typed("req:a", "Accepted", "Requirement")], vec![]);
        let r = impact(&base, suspect(&base, "req:a"));
        assert_eq!(
            r.affected_projections.projections,
            projections(&[
                "requirements.yaml",
                "functional.yaml",
                "markdown",
                "standards-conformance-report"
            ])
        );
        let base = graph(vec![typed("ext:risk", "Accepted", "Extension")], vec![]);
        let r = impact(&base, suspect(&base, "ext:risk"));
        assert_eq!(r.affected_projections.projections.len(), 15);
        assert_eq!(r.affected_gates.gates, GateId::ALL.into_iter().collect());
        let base = graph(vec![typed("req:p", "Proposed", "Requirement")], vec![]);
        let r = impact(
            &base,
            set_status(
                &base,
                "req:p",
                ElementStatus::Proposed,
                ElementStatus::Rejected,
            ),
        );
        assert!(r.dirty.node_ids.is_empty());
        assert!(r.affected_projections.projections.is_empty());
    }

    // ------------------------------------------------------------------ gates (§39)

    #[test]
    fn earliest_gates_and_downstream_expansion() {
        use NodeType as T;
        let cases = [
            (T::SourceArtifact, GateId::I0),
            (T::Requirement, GateId::F1),
            (T::Calculation, GateId::F2),
            (T::Question, GateId::F3),
            (T::Scenario, GateId::F4),
            (T::QualityScenario, GateId::Q1),
            (T::Component, GateId::A2),
            (T::ApiOperation, GateId::A3),
            (T::ArchitectureDecision, GateId::A4),
            (T::ImplementationSlice, GateId::D1),
            (T::VerificationObligation, GateId::D2),
            (T::TestReceipt, GateId::C1),
        ];
        for (node_type, gate) in cases {
            assert_eq!(earliest_gate(node_type), gate, "{}", node_type.as_str());
        }
        assert_eq!(gates_from(GateId::I0).len(), 13);
        assert_eq!(gates_from(GateId::A2).len(), 6);
        assert_eq!(gates_from(GateId::C1), BTreeSet::from([GateId::C1]));
        // End to end: Requirement -> F1..C1, Calculation -> F2..C1, Component -> A2..C1.
        for (tag, first) in [
            ("Requirement", GateId::F1),
            ("Calculation", GateId::F2),
            ("Component", GateId::A2),
            ("ApiOperation", GateId::A3),
            ("ImplementationSlice", GateId::D1),
            ("VerificationObligation", GateId::D2),
        ] {
            let base = graph(vec![typed("x:a", "Accepted", tag)], vec![]);
            let r = impact(&base, suspect(&base, "x:a"));
            assert_eq!(r.affected_gates.gates, gates_from(first), "{tag}");
        }
    }

    #[test]
    fn all_86_node_types_have_exactly_one_earliest_gate() {
        let table: [(GateId, &[&str]); 12] = [
            (
                GateId::I0,
                &[
                    "SourceArtifact",
                    "EvidenceFragment",
                    "DerivationRecord",
                    "Agent",
                    "Finding",
                    "StandardsProfile",
                    "Extension",
                ],
            ),
            (
                GateId::F1,
                &[
                    "Stakeholder",
                    "Concern",
                    "Goal",
                    "Need",
                    "Requirement",
                    "AcceptanceCriterion",
                    "Constraint",
                    "Term",
                    "Concept",
                ],
            ),
            (
                GateId::F2,
                &[
                    "Actor",
                    "BusinessRole",
                    "Entity",
                    "Attribute",
                    "DomainRelationship",
                    "State",
                    "Transition",
                    "Invariant",
                    "Operation",
                    "Outcome",
                    "Event",
                    "Process",
                    "ProcessNode",
                    "Rule",
                    "DecisionTable",
                    "Calculation",
                    "Calendar",
                    "Principal",
                    "SecurityRole",
                    "Permission",
                    "ResourceScope",
                    "PolicyCondition",
                    "SeparationConstraint",
                ],
            ),
            (
                GateId::F3,
                &["Question", "ResolutionDecision", "Assumption"],
            ),
            (GateId::F4, &["Scenario", "ScenarioRun"]),
            (
                GateId::Q1,
                &["QualityCharacteristic", "Measure", "QualityScenario"],
            ),
            (
                GateId::A2,
                &[
                    "SystemOfInterest",
                    "ArchitectureDescription",
                    "ArchitectureCandidate",
                    "Viewpoint",
                    "View",
                    "ModelKind",
                    "SoftwareSystem",
                    "Container",
                    "Component",
                    "Module",
                    "Interface",
                    "DataStore",
                    "ExternalSystem",
                    "DeploymentNode",
                    "RuntimeEnvironment",
                    "NetworkZone",
                    "Technology",
                    "TechnologySelection",
                ],
            ),
            (
                GateId::A3,
                &[
                    "ApiContract",
                    "ApiOperation",
                    "EventContract",
                    "Channel",
                    "Message",
                    "DataSchema",
                    "TechnicalWorkflow",
                ],
            ),
            (GateId::A4, &["ArchitectureDecision"]),
            (
                GateId::D1,
                &[
                    "Capability",
                    "ImplementationSlice",
                    "WorkPackage",
                    "TaskContract",
                    "Migration",
                    "Release",
                ],
            ),
            (GateId::D2, &["VerificationObligation", "TestCase"]),
            (
                GateId::C1,
                &[
                    "TestExecution",
                    "TestReceipt",
                    "CodeBinding",
                    "ArchitectureCheck",
                    "CoverageRecord",
                ],
            ),
        ];
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for (gate, types) in table {
            for name in types {
                assert!(seen.insert(name), "{name} listed twice");
                let node_type = NodeType::ALL.iter().find(|t| t.as_str() == *name).unwrap();
                assert_eq!(earliest_gate(*node_type), gate, "{name}");
            }
        }
        assert_eq!(NodeType::ALL.len(), 86);
        assert_eq!(seen.len(), 86);
        assert!(NodeType::ALL.iter().all(|t| seen.contains(t.as_str())));
        assert!(NodeType::ALL
            .iter()
            .all(|t| earliest_gate(*t) != GateId::A1));
    }

    // ------------------------------------------------------------------ golden report (§40)

    /// Hand-written from the rules, not from the implementation.
    const GOLDEN_REPORT: &str = r#"{"affected_gates":{"gates":["F2","F3","F4","Q1","A1","A2","A3","A4","D1","D2","C1"]},"affected_projections":{"projections":["functional.yaml","bpmn","dmn","verification-matrix","standards-conformance-report","markdown","diagrams"]},"changed":{"edge_ids":[],"node_ids":["calc:leave-days"]},"dirty":{"edge_ids":[],"node_ids":["calc:leave-days","op:approve","scn:half-day","verify:approval"]}}"#;

    #[test]
    fn golden_impact_report() {
        let base = chain();
        let r = impact(&base, suspect(&base, "calc:leave-days"));
        assert_eq!(
            String::from_utf8(to_canonical_json(&r).unwrap()).unwrap(),
            GOLDEN_REPORT
        );
        assert_eq!(
            serde_json::from_str::<ImpactReport>(GOLDEN_REPORT).unwrap(),
            r
        );
        let again = impact(&base, suspect(&base, "calc:leave-days"));
        assert_eq!(to_canonical_json(&again).unwrap(), GOLDEN_REPORT.as_bytes());
    }

    // ------------------------------------------------------------------ dependency guard (§41)

    #[test]
    fn impact_has_no_store_dependency() {
        // The raw, complete source: comments are part of the layering boundary.
        let source = include_str!("../src/impact.rs");
        for forbidden in [
            "plumb_store",
            "CommitResult",
            "SqliteRevisionStore",
            "RevisionId",
        ] {
            assert!(
                !source.contains(forbidden),
                "impact.rs mentions {forbidden}"
            );
        }
        let manifest = include_str!("../Cargo.toml");
        assert!(!manifest.contains("plumb-store"));
    }

    #[test]
    fn impact_does_not_consume_touched_sets() {
        // Documentation may contrast touched with changed; executable source must not read them.
        let code: String = include_str!("../src/impact.rs")
            .lines()
            .map(|l| l.split("//").next().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in ["touched_nodes", "touched_edges"] {
            assert!(!code.contains(forbidden), "impact.rs reads {forbidden}");
        }
    }

    // ------------------------------------------------------------------ reachable-node closure (Hotfix 045)

    fn reach(g: &Graph, seeds: &[&str]) -> BTreeSet<Id> {
        impact_reachable_nodes(g, &ids(seeds)).unwrap()
    }

    #[test]
    fn reachable_seed_without_edges_returns_itself() {
        assert_eq!(reach(&chain(), &["req:unrelated"]), ids(&["req:unrelated"]));
        assert_eq!(reach(&chain(), &[]), ids(&[]));
    }

    #[test]
    fn reachable_follows_to_from_and_from_to() {
        let g = chain();
        // uses_calculation is to -> from; verified_by and implemented_as are from -> to.
        assert_eq!(
            reach(&g, &["calc:leave-days"]),
            ids(&[
                "calc:leave-days",
                "op:approve",
                "scn:half-day",
                "verify:approval"
            ])
        );
        assert_eq!(
            reach(&g, &["op:approve"]),
            ids(&["op:approve", "scn:half-day", "verify:approval"])
        );
        assert_eq!(
            reach(&g, &["verify:approval"]),
            ids(&["scn:half-day", "verify:approval"])
        );
        assert_eq!(reach(&g, &["scn:half-day"]), ids(&["scn:half-day"]));
        for (relation, from_type, to_type, direction) in RELATIONS {
            let g = graph(
                vec![
                    typed("x:from", "Accepted", from_type),
                    typed("x:to", "Accepted", to_type),
                ],
                vec![edge("rel:x", "Accepted", relation, "x:from", "x:to")],
            );
            let (dependency, dependent) = match direction {
                ImpactDirection::ToFrom => ("x:to", "x:from"),
                ImpactDirection::FromTo => ("x:from", "x:to"),
            };
            assert_eq!(
                reach(&g, &[dependency]),
                ids(&["x:from", "x:to"]),
                "{relation}"
            );
            assert_eq!(reach(&g, &[dependent]), ids(&[dependent]), "{relation}");
        }
    }

    #[test]
    fn reachable_ignores_unsupported_and_non_baseline_relations() {
        let g = graph(
            vec![
                typed("ent:leave", "Accepted", "Entity"),
                typed("attr:days", "Accepted", "Attribute"),
            ],
            vec![edge(
                "rel:has",
                "Accepted",
                "has_attribute",
                "ent:leave",
                "attr:days",
            )],
        );
        assert_eq!(reach(&g, &["ent:leave"]), ids(&["ent:leave"]));
        assert_eq!(reach(&g, &["attr:days"]), ids(&["attr:days"]));
        for status in ["Proposed", "Rejected"] {
            let g = graph(
                vec![
                    typed("calc:leave-days", "Accepted", "Calculation"),
                    typed("op:approve", "Accepted", "Operation"),
                ],
                vec![edge(
                    "rel:uses",
                    status,
                    "uses_calculation",
                    "op:approve",
                    "calc:leave-days",
                )],
            );
            assert_eq!(reach(&g, &["calc:leave-days"]), ids(&["calc:leave-days"]));
        }
    }

    #[test]
    fn reachable_terminates_on_cycles_and_unions_seeds() {
        let nodes = vec![
            typed("req:a", "Accepted", "Requirement"),
            typed("op:approve", "Accepted", "Operation"),
            typed("req:other", "Accepted", "Requirement"),
        ];
        let edges = vec![
            edge(
                "rel:spec",
                "Accepted",
                "specified_by",
                "req:a",
                "op:approve",
            ),
            edge(
                "rel:derived",
                "Accepted",
                "derived_from",
                "req:a",
                "op:approve",
            ),
        ];
        let g = graph(nodes.clone(), edges.clone());
        assert_eq!(reach(&g, &["req:a"]), ids(&["op:approve", "req:a"]));
        assert_eq!(reach(&g, &["op:approve"]), ids(&["op:approve", "req:a"]));
        assert_eq!(
            reach(&g, &["req:a", "req:other"]),
            ids(&["op:approve", "req:a", "req:other"])
        );
        let reversed = graph(
            nodes.into_iter().rev().collect(),
            edges.into_iter().rev().collect(),
        );
        assert_eq!(
            reach(&reversed, &["req:other", "req:a"]),
            reach(&g, &["req:a", "req:other"])
        );
        // Overlapping closures count each node once.
        let c = chain();
        assert_eq!(
            reach(&c, &["calc:leave-days", "op:approve"]),
            reach(&c, &["calc:leave-days"])
        );
    }

    #[test]
    fn reachable_rejects_missing_and_non_baseline_seeds() {
        let g = graph(
            vec![
                typed("op:approve", "Accepted", "Operation"),
                typed("op:draft", "Proposed", "Operation"),
                typed("op:old", "Suspect", "Operation"),
            ],
            vec![],
        );
        assert_eq!(
            impact_reachable_nodes(&g, &ids(&["op:ghost"])),
            Err(ImpactError::UnknownSeed(id("op:ghost")))
        );
        assert_eq!(
            impact_reachable_nodes(&g, &ids(&["op:approve", "op:draft"])),
            Err(ImpactError::NonBaselineSeed(id("op:draft")))
        );
        assert_eq!(reach(&g, &["op:old"]), ids(&["op:old"]));
    }

    #[test]
    fn reachable_matches_compute_impact_closure() {
        // The closure of a changed node equals compute_impact's dirty nodes for that change.
        let g = chain();
        for seed in [
            "calc:leave-days",
            "op:approve",
            "verify:approval",
            "req:unrelated",
        ] {
            assert_eq!(
                reach(&g, &[seed]),
                impact(&g, suspect(&g, seed)).dirty.node_ids,
                "{seed}"
            );
        }
    }
}
