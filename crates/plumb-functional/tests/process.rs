//! S2.9 contract tests for AI-assisted Process proposals: the closed process scope and context,
//! the inference request and artifact contract, deterministic identities and origins, the
//! dry-run plus ActiveOverlay qualification through the plumb-validation process-analysis
//! kernel, safe reconciliation, requirement-order independence and the HR process_specs
//! reference.
//!
//! Golden values were computed independently with Python `hashlib` over RFC 8785 JSON, never
//! with the helpers under test. Graphs are synthetic; the HR test adapts the immutable fixture
//! process_specs (process names, operation membership and order) and claims nothing else.

mod process_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_functional::process::*;
    use plumb_inference::{InferenceArtifact, InferenceRequest, ProviderPolicy};
    use plumb_patch::{
        apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        Actor, ActorKind, AuditMeta, BusinessRole, DerivationRef, Edge, ElementStatus, Entity,
        Event, Graph, Modality, Node, NodePayload, Operation, OperationKind, Outcome, OutcomeKind,
        Process, ProcessNode, ProcessNodeKind, RelationKind, RelationProperties, Requirement,
        RequirementKind, RequirementLevel, SecurityRole, State, Transition,
    };
    use plumb_validation::process_analysis::{
        analyze_process, ParallelIssueKind, ProcessAnalysisMode,
    };
    use serde_json::{json, Value as Json};

    use ElementStatus::{Accepted, Proposed};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s2-process.md");
    const SCHEMA: &str = include_str!("../../../schemas/inference/s2-process.schema.json");
    const HR_CONTRACT: &str = include_str!("../../../fixtures/hr-leave/fixture-contract.yaml");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:a8dd9ca29824bcef9b59fbdacc2abee131fd2d3f4e1183f8eb8ec779a2d4419f";
    const SCHEMA_HASH: &str =
        "sha256:a4b0c6e1727e85d997c758e00fc86707935c26a683ce67d700ed10ec2afc39b3";
    const GOLDEN_CONTEXT_HASH: &str =
        "sha256:aa37d26fb9772847592ef6d3a10377e37245b864b1316b5e0e7b02236dc355ee";
    const GOLDEN_REQUEST_ID: &str =
        "sha256:b894cf52f9354e42cc11d10e04940136b6fd88c5f92348a5110dfc05b74828a6";
    const GOLDEN_PROCESS_ID: &str = "process:68053fbc051a5f16";
    const GOLDEN_START_ID: &str = "process_node:6f9da587bf8b2cc9";
    const GOLDEN_APPROVE_ID: &str = "process_node:6384d4d17f06eb04";
    const GOLDEN_END_ID: &str = "process_node:e9772c79be305801";
    const GOLDEN_NEXT_ID: &str = "rel:2078460de568e59e";
    const GOLDEN_CONSUMES_ID: &str = "rel:1df47652f0fc96cb";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    // Synthetic generic test data only; never in production source.
    const R1: &str = "When an order is received, the clerk approves the order and the process ends with the order approved.";
    const R2: &str = "Approved orders are shipped in parallel with invoicing.";
    const NAME: &str = "Order Approval";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ids(list: &[&str]) -> Vec<Id> {
        list.iter().map(|s| id(s)).collect()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn meta() -> AuditMeta {
        AuditMeta::new(id("actor:human"), ts(AT), None, None).unwrap()
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

    fn edge(kind: RelationKind, from: &str, to: &str) -> Edge {
        Edge {
            id: id(&format!(
                "rel:fx-{}-{}",
                from.replace(':', "-"),
                to.replace(':', "-")
            )),
            revision: 1,
            status: Accepted,
            kind,
            from: id(from),
            to: id(to),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: meta(),
        }
    }

    fn requirement(node_id: &str, statement: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Requirement(Requirement {
                statement: statement.into(),
                requirement_kind: RequirementKind::Functional,
                level: RequirementLevel::System,
                modality: Modality::Shall,
                title: None,
                rationale: None,
                priority: None,
                source_identifier: None,
                verification_method: None,
                owner_refs: None,
                stakeholder_refs: None,
            }),
        )
    }

    fn operation(node_id: &str, status: ElementStatus, name: &str) -> Node {
        node(
            node_id,
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
    }

    /// The golden graph (r1, approve, received) plus extra synthetic context nodes.
    fn nodes() -> Vec<Node> {
        vec![
            requirement("req:r1", R1),
            requirement("req:r2", R2),
            operation("operation:approve", Accepted, "ApproveOrder"),
            operation("operation:ship", Accepted, "ShipOrder"),
            operation("operation:invoice", Accepted, "InvoiceOrder"),
            operation("operation:draft", Proposed, "DraftOrder"),
            node(
                "event:received",
                Accepted,
                NodePayload::Event(Event {
                    name: "OrderReceived".into(),
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
            node(
                "secrole:clerk",
                Accepted,
                NodePayload::SecurityRole(SecurityRole {
                    name: "ClerkUser".into(),
                    description: None,
                }),
            ),
            node(
                "entity:order",
                Accepted,
                NodePayload::Entity(Entity {
                    name: "Order".into(),
                    description: None,
                    aggregate_root: None,
                }),
            ),
            node(
                "state:new",
                Accepted,
                NodePayload::State(State { name: "new".into() }),
            ),
            node(
                "state:approved",
                Accepted,
                NodePayload::State(State {
                    name: "approved".into(),
                }),
            ),
            node(
                "transition:approve",
                Accepted,
                NodePayload::Transition(Transition {
                    stateful_ref: id("entity:order"),
                    from_state: id("state:new"),
                    to_state: id("state:approved"),
                    guard_expr: None,
                    effect_refs: None,
                }),
            ),
        ]
    }

    fn edges() -> Vec<Edge> {
        vec![
            edge(RelationKind::HasState, "entity:order", "state:new"),
            edge(RelationKind::HasState, "entity:order", "state:approved"),
            edge(
                RelationKind::TransitionsVia,
                "transition:approve",
                "operation:approve",
            ),
            edge(
                RelationKind::PerformedBy,
                "operation:approve",
                "brole:clerk",
            ),
        ]
    }

    fn graph_of(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn golden_graph() -> Graph {
        let keep = ["req:r1", "operation:approve", "event:received"];
        graph_of(
            nodes()
                .into_iter()
                .filter(|n| keep.contains(&n.id.as_str()))
                .collect(),
            vec![],
        )
    }

    fn base_graph() -> Graph {
        graph_of(nodes(), edges())
    }

    fn with_nodes(graph: &Graph, extra: Vec<Node>, extra_edges: Vec<Edge>) -> Graph {
        let mut n: Vec<Node> = graph.nodes().values().cloned().collect();
        n.extend(extra);
        let mut e: Vec<Edge> = graph.edges().values().cloned().collect();
        e.extend(extra_edges);
        graph_of(n, e)
    }

    fn golden_scope() -> ProcessScope {
        ProcessScope {
            operation_refs: ids(&["operation:approve"]),
            event_refs: ids(&["event:received"]),
            ..ProcessScope::default()
        }
    }

    fn full_scope() -> ProcessScope {
        ProcessScope {
            operation_refs: ids(&["operation:invoice", "operation:approve", "operation:ship"]),
            performer_refs: ids(&["brole:clerk", "actor:bot"]),
            event_refs: ids(&["event:received"]),
            outcome_refs: ids(&["outcome:done"]),
            transition_refs: ids(&["transition:approve"]),
        }
    }

    fn provider() -> ProviderPolicy {
        ProviderPolicy {
            provider: "mock".into(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn derivation() -> DerivationRef {
        DerivationRef::from(id("drv:00000000000000c9"))
    }

    fn audit() -> ProcessAudit {
        ProcessAudit {
            created_by: id("agent:process"),
            created_at: ts(AT),
        }
    }

    fn artifact_for(request: &InferenceRequest, output: Json) -> InferenceArtifact {
        let raw = to_canonical_json(&output).unwrap();
        let validated_output = CanonicalJson::new(output);
        InferenceArtifact {
            request_hash: request.id.clone(),
            provider: request.provider_policy.provider.clone(),
            model: "mock-model".to_owned(),
            parameters: CanonicalJson::new(json!({})),
            raw_response_hash: Hash::content_sha256(&raw),
            validated_output_hash: validated_output.content_hash().unwrap(),
            validated_output,
        }
    }

    fn range(needle: &str) -> Json {
        let (req, text) = if R1.contains(needle) {
            ("req:r1", R1)
        } else {
            ("req:r2", R2)
        };
        let start = text.find(needle).unwrap_or_else(|| panic!("{needle}"));
        json!({"requirement_ref": req, "start": start, "end": start + needle.len()})
    }

    fn gref(target: &str, needle: &str) -> Json {
        json!({"target_ref": target, "evidence": [range(needle)]})
    }

    fn pnode(key: &str, kind: &str) -> Json {
        json!({
            "node_key": key,
            "node_kind": kind,
            "operation": null,
            "performers": [],
            "produces": [],
            "message_ref": null,
            "timer_expr": null,
            "evidence": [range("order")],
        })
    }

    fn task(key: &str, kind: &str, op: &str) -> Json {
        let mut n = pnode(key, kind);
        n["operation"] = gref(op, "approves the order");
        n
    }

    fn next(from: &str, to: &str) -> Json {
        json!({"from_key": from, "to_key": to, "evidence": [range("the clerk approves")]})
    }

    fn chain(keys: &[&str]) -> Vec<Json> {
        keys.windows(2).map(|w| next(w[0], w[1])).collect()
    }

    fn process(name: &str, nodes: Vec<Json>, next: Vec<Json>) -> Json {
        json!({"name": name, "evidence": [range("When an order is received")], "nodes": nodes, "next": next})
    }

    /// The golden candidate: Event start -> service task -> end.
    fn golden_process() -> Json {
        let mut start = pnode("start", "start");
        start["message_ref"] = gref("event:received", "an order is received");
        process(
            NAME,
            vec![
                start,
                task("approve", "service_task", "operation:approve"),
                pnode("end", "end"),
            ],
            chain(&["start", "approve", "end"]),
        )
    }

    fn linear_with(middle: Json) -> Json {
        let key = middle["node_key"].as_str().unwrap().to_owned();
        process(
            NAME,
            vec![pnode("start", "start"), middle, pnode("end", "end")],
            chain(&["start", &key, "end"]),
        )
    }

    fn output(processes: Vec<Json>) -> Json {
        json!({"version": 1, "processes": processes})
    }

    fn request(graph: &Graph, scope: &ProcessScope, targets: &[&str]) -> ProcessRequest {
        build_process_request(graph, &ids(targets), scope, provider()).unwrap()
    }

    fn run_out(
        graph: &Graph,
        scope: &ProcessScope,
        targets: &[&str],
        out: Json,
    ) -> Result<ProcessCompilationResult, ProcessError> {
        let r = request(graph, scope, targets);
        let artifact = artifact_for(&r.request, out);
        analyze_processes(
            graph,
            &r,
            Some(ProcessInference {
                artifact: &artifact,
                derivation_ref: derivation(),
            }),
            &audit(),
        )
    }

    fn run(graph: &Graph, processes: Vec<Json>) -> Result<ProcessCompilationResult, ProcessError> {
        run_out(
            graph,
            &full_scope(),
            &["req:r1", "req:r2"],
            output(processes),
        )
    }

    fn one(graph: &Graph, p: Json) -> (ProcessCandidateAnalysis, Vec<Proposal>) {
        let r = run(graph, vec![p]).unwrap();
        assert_eq!(r.candidates.len(), 1);
        (r.candidates[0].clone(), r.proposals)
    }

    fn issues(p: Json) -> Vec<ProcessIssue> {
        let (c, proposals) = one(&base_graph(), p);
        assert!(proposals.is_empty() || c.issues.is_empty());
        c.issues
    }

    fn patches(p: &Proposal) -> &[SemanticPatch] {
        let SemanticPatch::Compound { patches } = &p.patch_set.patch else {
            panic!("not compound")
        };
        patches
    }

    fn apply(graph: &Graph, p: &Proposal) -> Graph {
        let patch_set = PatchSet {
            base_semantic_hash: graph.semantic_hash().unwrap(),
            patch: p.patch_set.patch.clone(),
        };
        let g = apply_patch(graph, &patch_set).unwrap().graph;
        g.validate().unwrap();
        g
    }

    fn accept_all(graph: &Graph) -> Graph {
        let nodes = graph
            .nodes()
            .values()
            .cloned()
            .map(|mut n| {
                if n.status == Proposed && n.id.as_str() != "operation:draft" {
                    n.status = Accepted;
                }
                n
            })
            .collect();
        let edges = graph
            .edges()
            .values()
            .cloned()
            .map(|mut e| {
                e.status = Accepted;
                e
            })
            .collect();
        graph_of(nodes, edges)
    }

    // ------------------------------------------------------------------ goldens and proposals

    #[test]
    fn process_request_goldens() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
        assert_eq!(
            (
                PROCESS_CONTEXT_VERSION,
                PROCESS_OUTPUT_VERSION,
                PROCESS_TASK_KIND
            ),
            (1, 1, "process_analysis")
        );
        assert_eq!(PROCESS_ORIGIN_EXTENSION, "plumb_functional:process_origin");
        assert_eq!(
            PROCESS_NODE_ORIGIN_EXTENSION,
            "plumb_functional:process_node_origin"
        );
        let g = golden_graph();
        let r = request(&g, &golden_scope(), &["req:r1"]);
        assert_eq!(r.request.context_hash.as_str(), GOLDEN_CONTEXT_HASH);
        assert_eq!(r.request.id.as_str(), GOLDEN_REQUEST_ID);
        assert_eq!(r.request.stage, StageId::S2);
        assert_eq!(
            r.request.input_refs,
            ids(&["event:received", "operation:approve", "req:r1"])
        );
        // Context summaries; no document position anywhere.
        let full = request(&base_graph(), &full_scope(), &["req:r2", "req:r1"]);
        let json = serde_json::to_value(&full.context).unwrap();
        assert_eq!(json["operations"][0]["operation_ref"], "operation:approve");
        assert_eq!(
            json["operations"][0]["performer_refs"],
            json!(["brole:clerk"])
        );
        assert_eq!(
            json["transitions"],
            json!([{"transition_ref": "transition:approve", "stateful_ref": "entity:order", "from_state": "state:new", "to_state": "state:approved", "transitions_via_refs": ["operation:approve"]}])
        );
        let text = json.to_string();
        for forbidden in ["paragraph", "sentence", "ordinal", "position", "segment"] {
            assert!(!text.contains(forbidden), "{forbidden}");
        }
        // Scope validation.
        let bad = |s: ProcessScope| {
            matches!(
                build_process_request(&base_graph(), &ids(&["req:r1"]), &s, provider()),
                Err(ProcessError::Scope { .. })
            )
        };
        assert!(bad(ProcessScope {
            performer_refs: ids(&["secrole:clerk"]),
            ..ProcessScope::default()
        }));
        assert!(bad(ProcessScope {
            operation_refs: ids(&["operation:draft"]),
            ..ProcessScope::default()
        }));
        assert!(bad(ProcessScope {
            event_refs: ids(&["outcome:done"]),
            ..ProcessScope::default()
        }));
        assert!(bad(ProcessScope {
            operation_refs: ids(&["operation:ship", "operation:ship"]),
            ..ProcessScope::default()
        }));
    }

    #[test]
    fn process_golden_proposal() {
        let g = golden_graph();
        let r = run_out(
            &g,
            &golden_scope(),
            &["req:r1"],
            output(vec![golden_process()]),
        )
        .unwrap();
        let c = &r.candidates[0];
        assert!(c.issues.is_empty(), "{:?}", c.issues);
        assert_eq!(c.process_ref.as_str(), GOLDEN_PROCESS_ID);
        assert_eq!(c.node_refs["start"].as_str(), GOLDEN_START_ID);
        assert_eq!(c.node_refs["approve"].as_str(), GOLDEN_APPROVE_ID);
        assert_eq!(c.node_refs["end"].as_str(), GOLDEN_END_ID);
        assert!(c.analysis.as_ref().unwrap().is_qualified());
        let p = &r.proposals[0];
        assert!(
            matches!(&c.disposition, ProcessDisposition::Proposed { proposal_ref } if *proposal_ref == p.id)
        );
        assert_eq!(
            (p.stage, p.materiality, p.acceptance_policy, p.confidence),
            (
                StageId::S2,
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
                None
            )
        );
        assert_eq!(p.derivation_refs, vec![derivation()]);
        // Order: Process, ProcessNodes by ID, edges by ID.
        let order: Vec<&str> = patches(p)
            .iter()
            .map(|s| match s {
                SemanticPatch::AddNode { node } => node.id.as_str(),
                SemanticPatch::AddEdge { edge } => edge.id.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        let mut nodes = [GOLDEN_APPROVE_ID, GOLDEN_START_ID, GOLDEN_END_ID];
        nodes.sort();
        assert_eq!(order[0], GOLDEN_PROCESS_ID);
        assert_eq!(order[1..4], nodes[..]);
        assert!(order[4..].windows(2).all(|w| w[0] < w[1]));
        assert!(order[4..].contains(&GOLDEN_NEXT_ID) && order[4..].contains(&GOLDEN_CONSUMES_ID));
        assert_eq!(order.len(), 4 + 3);
        for s in patches(p) {
            match s {
                SemanticPatch::AddNode { node } => {
                    assert_eq!((node.status, node.revision), (Proposed, 1));
                    assert!(node.derivations.is_empty());
                }
                SemanticPatch::AddEdge { edge } => {
                    assert_eq!(
                        (edge.status, edge.revision, &edge.properties),
                        (Proposed, 1, &RelationProperties::None)
                    );
                    assert!(edge.derivations.is_empty());
                }
                _ => unreachable!(),
            }
        }
        let applied = apply(&g, p);
        assert_eq!(
            applied.node(&id(GOLDEN_PROCESS_ID)).unwrap().payload,
            NodePayload::Process(Process {
                name: NAME.into(),
                description: None,
                process_kind: None
            })
        );
        assert_eq!(
            applied.node(&id(GOLDEN_START_ID)).unwrap().payload,
            NodePayload::ProcessNode(ProcessNode {
                process_ref: id(GOLDEN_PROCESS_ID),
                node_kind: ProcessNodeKind::Start,
                operation_ref: None,
                condition_expr: None,
                message_ref: Some(id("event:received")),
                timer_expr: None,
            })
        );
        assert_eq!(
            applied.node(&id(GOLDEN_APPROVE_ID)).unwrap().payload,
            NodePayload::ProcessNode(ProcessNode {
                process_ref: id(GOLDEN_PROCESS_ID),
                node_kind: ProcessNodeKind::ServiceTask,
                operation_ref: Some(id("operation:approve")),
                condition_expr: None,
                message_ref: None,
                timer_expr: None,
            })
        );
        let origin = applied
            .node(&id(GOLDEN_APPROVE_ID))
            .unwrap()
            .extensions
            .iter()
            .find(|(k, _)| k.as_str() == PROCESS_NODE_ORIGIN_EXTENSION)
            .unwrap()
            .1
            .clone();
        let origin: ProcessNodeOrigin = serde_json::from_value(origin).unwrap();
        assert_eq!(origin.node_key, "approve");
        let process_origin = applied
            .node(&id(GOLDEN_PROCESS_ID))
            .unwrap()
            .extensions
            .iter()
            .find(|(k, _)| k.as_str() == PROCESS_ORIGIN_EXTENSION)
            .unwrap()
            .1
            .clone();
        let process_origin: ProcessOrigin = serde_json::from_value(process_origin).unwrap();
        assert_eq!(process_origin.evidence.len(), 1);
        let next = applied.edge(&id(GOLDEN_NEXT_ID)).unwrap();
        assert_eq!(
            (next.kind.clone(), next.from.as_str(), next.to.as_str()),
            (RelationKind::Next, GOLDEN_START_ID, GOLDEN_APPROVE_ID)
        );
        let consumes = applied.edge(&id(GOLDEN_CONSUMES_ID)).unwrap();
        assert_eq!(
            (
                consumes.kind.clone(),
                consumes.from.as_str(),
                consumes.to.as_str()
            ),
            (RelationKind::Consumes, GOLDEN_START_ID, "event:received")
        );
        // The accepted baseline analyzer sees the same Process once accepted.
        let accepted = accept_all(&applied);
        assert!(analyze_process(
            &accepted,
            &id(GOLDEN_PROCESS_ID),
            ProcessAnalysisMode::AcceptedOnly
        )
        .unwrap()
        .is_qualified());
    }

    // ------------------------------------------------------------------ semantics through inference

    #[test]
    fn process_starts_tasks_and_waits() {
        let g = base_graph();
        // Manual and timer starts.
        assert!(issues(process(
            NAME,
            vec![
                pnode("start", "start"),
                task("a", "service_task", "operation:approve"),
                pnode("end", "end")
            ],
            chain(&["start", "a", "end"])
        ))
        .is_empty());
        let mut timer = pnode("start", "start");
        timer["timer_expr"] = json!("every weekday at 09:00");
        assert!(issues(process(
            NAME,
            vec![
                timer.clone(),
                task("a", "service_task", "operation:approve"),
                pnode("end", "end")
            ],
            chain(&["start", "a", "end"])
        ))
        .is_empty());
        let mut both = timer;
        both["message_ref"] = gref("event:received", "received");
        assert!(issues(process(
            NAME,
            vec![
                both,
                task("a", "service_task", "operation:approve"),
                pnode("end", "end")
            ],
            chain(&["start", "a", "end"])
        ))
        .iter()
        .any(|i| matches!(i, ProcessIssue::AmbiguousStartTrigger { .. })));
        // Operation-backed human task and explicit human activity.
        assert!(issues(linear_with(task("a", "human_task", "operation:approve"))).is_empty());
        let mut human = pnode("review", "human_task");
        human["performers"] = json!([gref("brole:clerk", "clerk")]);
        human["produces"] = json!([gref("outcome:done", "approved")]);
        let (c, proposals) = one(&g, linear_with(human.clone()));
        assert!(c.issues.is_empty(), "{:?}", c.issues);
        let applied = apply(&g, &proposals[0]);
        let review = &c.node_refs["review"];
        let out: BTreeSet<(RelationKind, Id)> = applied
            .outgoing_edge_ids(review)
            .iter()
            .map(|e| applied.edge(e).unwrap())
            .map(|e| (e.kind.clone(), e.to.clone()))
            .collect();
        assert!(
            out.contains(&(RelationKind::PerformedBy, id("brole:clerk")))
                && out.contains(&(RelationKind::Produces, id("outcome:done")))
        );
        let mut by_event = human.clone();
        by_event["performers"] = json!([gref("actor:bot", "clerk")]);
        by_event["produces"] = json!([gref("event:received", "received")]);
        assert!(issues(linear_with(by_event)).is_empty());
        let mut no_performer = human.clone();
        no_performer["performers"] = json!([]);
        assert_eq!(
            issues(linear_with(no_performer)),
            vec![ProcessIssue::UnresolvedHumanResponsibility {
                node_ref: c.node_refs["review"].clone()
            }]
        );
        let mut no_outcome = human;
        no_outcome["produces"] = json!([]);
        assert_eq!(
            issues(linear_with(no_outcome)),
            vec![ProcessIssue::UnresolvedHumanOutcome {
                node_ref: c.node_refs["review"].clone()
            }]
        );
        // A service task without an Operation.
        assert!(issues(linear_with(pnode("a", "service_task")))
            .iter()
            .any(|i| matches!(i, ProcessIssue::UnresolvedServiceTask { .. })));
        // Event and timer waits.
        let mut wait = pnode("wait", "message_event");
        wait["message_ref"] = gref("event:received", "received");
        assert!(issues(linear_with(wait)).is_empty());
        let mut timer_wait = pnode("wait", "timer_event");
        timer_wait["timer_expr"] = json!("P2D");
        assert!(issues(linear_with(timer_wait.clone())).is_empty());
        timer_wait["timer_expr"] = json!("");
        assert!(issues(linear_with(timer_wait))
            .iter()
            .any(|i| matches!(i, ProcessIssue::InvalidNodeShape { .. })));
        // Unreachable node and a non-End terminal.
        let stray = process(
            NAME,
            vec![
                pnode("start", "start"),
                task("a", "service_task", "operation:approve"),
                pnode("end", "end"),
                task("b", "service_task", "operation:ship"),
            ],
            chain(&["start", "a", "end"]),
        );
        let i = issues(stray);
        assert!(i
            .iter()
            .any(|i| matches!(i, ProcessIssue::UnreachableNode { .. })));
        assert!(i
            .iter()
            .any(|i| matches!(i, ProcessIssue::InvalidTerminal { .. })));
        let no_end = process(
            NAME,
            vec![
                pnode("start", "start"),
                task("a", "service_task", "operation:approve"),
            ],
            chain(&["start", "a"]),
        );
        assert!(issues(no_end).contains(&ProcessIssue::MissingEnd));
        let no_start = process(
            NAME,
            vec![
                task("a", "service_task", "operation:approve"),
                pnode("end", "end"),
            ],
            chain(&["a", "end"]),
        );
        assert!(issues(no_start).contains(&ProcessIssue::MissingStart));
    }

    #[test]
    fn process_deferred_kinds_and_parallel() {
        for (kind, check) in [
            (
                "exclusive_gateway",
                (|i: &ProcessIssue| {
                    matches!(i, ProcessIssue::ExclusiveGatewayUnrepresentable { .. })
                }) as fn(&ProcessIssue) -> bool,
            ),
            ("error_event", |i| {
                matches!(i, ProcessIssue::ErrorEventUnrepresentable { .. })
            }),
            ("subprocess", |i| {
                matches!(i, ProcessIssue::SubprocessUnrepresentable { .. })
            }),
        ] {
            let (c, proposals) = one(&base_graph(), linear_with(pnode("x", kind)));
            assert!(c.issues.iter().any(check), "{kind}: {:?}", c.issues);
            assert!(proposals.is_empty());
            assert_eq!(c.disposition, ProcessDisposition::Ineligible);
        }
        // The schema never exposes condition_expr.
        let mut conditioned = pnode("a", "service_task");
        conditioned["condition_expr"] = json!("x > 1");
        assert!(matches!(
            run(&base_graph(), vec![linear_with(conditioned)]),
            Err(ProcessError::SchemaInvalid { .. })
        ));
        // Balanced parallel and an early merge.
        let parallel = |extra: Vec<Json>| {
            let nodes = vec![
                pnode("start", "start"),
                pnode("split", "parallel_split"),
                task("ship", "service_task", "operation:ship"),
                task("invoice", "service_task", "operation:invoice"),
                pnode("join", "parallel_join"),
                pnode("end", "end"),
            ];
            let mut next = chain(&["start", "split", "ship", "join", "end"]);
            next.extend(chain(&["split", "invoice", "join"]));
            next.extend(extra);
            process("Order Fulfilment", nodes, next)
        };
        let (c, proposals) = one(&base_graph(), parallel(vec![]));
        assert!(c.issues.is_empty(), "{:?}", c.issues);
        assert_eq!(proposals.len(), 1);
        let (c, _) = one(&base_graph(), parallel(vec![next("ship", "invoice")]));
        assert!(
            c.issues.iter().any(|i| matches!(
                i,
                ProcessIssue::Parallel {
                    kind: ParallelIssueKind::ParallelBranchesMergeEarly,
                    ..
                }
            )),
            "{:?}",
            c.issues
        );
    }

    // ------------------------------------------------------------------ artifact contract

    #[test]
    fn process_malformed_artifacts() {
        let g = base_graph();
        let err = |p: Json| run(&g, vec![p]).unwrap_err();
        let schema = |p: Json| matches!(err(p), ProcessError::SchemaInvalid { .. });
        assert!(matches!(
            run_out(
                &g,
                &full_scope(),
                &["req:r1"],
                json!({"version": 2, "processes": []})
            )
            .unwrap_err(),
            ProcessError::SchemaInvalid { .. }
        ));
        assert!(matches!(
            run_out(
                &g,
                &full_scope(),
                &["req:r1"],
                json!({"version": 1, "processes": [], "x": 1})
            )
            .unwrap_err(),
            ProcessError::SchemaInvalid { .. }
        ));
        let mut extra = golden_process();
        extra["description"] = json!("d");
        assert!(schema(extra));
        let mut bad_kind = golden_process();
        bad_kind["nodes"][1]["node_kind"] = json!("script_task");
        assert!(schema(bad_kind));
        let mut bad_key = golden_process();
        bad_key["nodes"][1]["node_key"] = json!("1approve");
        assert!(schema(bad_key));
        let mut bare = golden_process();
        bare["nodes"][1]["operation"] = json!("operation:approve");
        assert!(schema(bare));
        // Grounding.
        let mut unknown_req = golden_process();
        unknown_req["evidence"] = json!([{"requirement_ref": "req:r9", "start": 0, "end": 3}]);
        assert!(matches!(
            err(unknown_req),
            ProcessError::InvalidGrounding { .. }
        ));
        let mut out_of_range = golden_process();
        out_of_range["evidence"] = json!([{"requirement_ref": "req:r1", "start": 0, "end": 9999}]);
        assert!(matches!(
            err(out_of_range),
            ProcessError::InvalidGrounding { .. }
        ));
        // Semantic refs.
        let with_op = |op: &str| linear_with(task("a", "service_task", op));
        assert!(matches!(
            err(with_op("operation:ghost")),
            ProcessError::UnknownSemanticRef { .. }
        ));
        assert!(matches!(
            err(with_op("operation:draft")),
            ProcessError::NonAcceptedSemanticRef { .. }
        ));
        assert!(matches!(
            err(with_op("event:received")),
            ProcessError::WrongSemanticRefType { .. }
        ));
        assert!(matches!(
            run_out(
                &g,
                &golden_scope(),
                &["req:r1"],
                output(vec![with_op("operation:ship")])
            )
            .unwrap_err(),
            ProcessError::OutOfScopeRef { .. }
        ));
        let mut security = pnode("review", "human_task");
        security["performers"] = json!([gref("secrole:clerk", "clerk")]);
        assert!(matches!(
            err(linear_with(security)),
            ProcessError::WrongSemanticRefType { .. }
        ));
        // Identities and flows.
        assert!(matches!(
            run(&g, vec![golden_process(), golden_process()]).unwrap_err(),
            ProcessError::DuplicateProcess { .. }
        ));
        let dup_key = process(
            NAME,
            vec![pnode("start", "start"), pnode("start", "end")],
            vec![],
        );
        assert!(matches!(
            err(dup_key),
            ProcessError::DuplicateNodeKey { .. }
        ));
        let mut dup_next = golden_process();
        dup_next["next"]
            .as_array_mut()
            .unwrap()
            .push(next("start", "approve"));
        assert!(matches!(err(dup_next), ProcessError::DuplicateNext { .. }));
        let mut unknown_next = golden_process();
        unknown_next["next"]
            .as_array_mut()
            .unwrap()
            .push(next("start", "ghost"));
        assert!(matches!(
            err(unknown_next),
            ProcessError::UnknownNextKey { .. }
        ));
        let mut self_next = golden_process();
        self_next["next"]
            .as_array_mut()
            .unwrap()
            .push(next("approve", "approve"));
        assert!(matches!(err(self_next), ProcessError::SelfNext { .. }));
        // Artifact bound to another request.
        let r = request(&g, &full_scope(), &["req:r1"]);
        let mut artifact = artifact_for(&r.request, output(vec![]));
        artifact.request_hash = Hash::content_sha256(b"other");
        assert!(matches!(
            analyze_processes(
                &g,
                &r,
                Some(ProcessInference {
                    artifact: &artifact,
                    derivation_ref: derivation()
                }),
                &audit()
            ),
            Err(ProcessError::InvalidInferenceArtifact { .. })
        ));
        // No artifact.
        let none = analyze_processes(&g, &r, None, &audit()).unwrap();
        assert_eq!(none.issues, vec![ProcessIssue::InferenceUnavailable]);
        assert!(none.proposals.is_empty() && none.candidates.is_empty());
    }

    // ------------------------------------------------------------------ reconciliation

    #[test]
    fn process_reconciliation() {
        let g = base_graph();
        let (_, proposals) = one(&g, golden_process());
        let applied = apply(&g, &proposals[0]);
        let (c, p) = one(&applied, golden_process());
        assert_eq!(c.disposition, ProcessDisposition::IdempotentReplay);
        assert!(p.is_empty());
        let accepted = accept_all(&applied);
        let (c, p) = one(&accepted, golden_process());
        assert_eq!(c.disposition, ProcessDisposition::ExistingEquivalent);
        assert!(p.is_empty());
        // A different flow under the same Process is a conflict, never repaired.
        let different = process(
            NAME,
            vec![
                pnode("start", "start"),
                task("approve", "service_task", "operation:approve"),
                pnode("end", "end"),
            ],
            chain(&["start", "approve", "end"]),
        );
        let (c, p) = one(&accepted, different);
        assert_eq!(c.disposition, ProcessDisposition::ExistingConflict);
        assert!(c.issues.contains(&ProcessIssue::ExistingProcessConflict {
            process_ref: c.process_ref.clone()
        }));
        assert!(p.is_empty());
        // A different Process payload at the same ID.
        let pid = c.process_ref.clone();
        let described = with_nodes(
            &g,
            vec![node(
                pid.as_str(),
                Accepted,
                NodePayload::Process(Process {
                    name: NAME.into(),
                    description: Some("d".into()),
                    process_kind: None,
                }),
            )],
            vec![],
        );
        assert_eq!(
            one(&described, golden_process()).0.disposition,
            ProcessDisposition::ExistingConflict
        );
        // Same name under another ID.
        let legacy = with_nodes(
            &g,
            vec![node(
                "process:legacy",
                Accepted,
                NodePayload::Process(Process {
                    name: NAME.into(),
                    description: None,
                    process_kind: None,
                }),
            )],
            vec![],
        );
        let (c, _) = one(&legacy, golden_process());
        assert_eq!(
            c.issues,
            vec![ProcessIssue::ExistingProcessNameConflict {
                process_ref: pid.clone(),
                existing_ref: id("process:legacy")
            }]
        );
        // A ProcessNode already at a deterministic ID (under a different Process).
        let orphan_id = c.node_refs["approve"].clone();
        let orphan = with_nodes(
            &g,
            vec![
                node(
                    "process:other",
                    Accepted,
                    NodePayload::Process(Process {
                        name: "Other".into(),
                        description: None,
                        process_kind: None,
                    }),
                ),
                node(
                    orphan_id.as_str(),
                    Accepted,
                    NodePayload::ProcessNode(ProcessNode {
                        process_ref: id("process:other"),
                        node_kind: ProcessNodeKind::End,
                        operation_ref: None,
                        condition_expr: None,
                        message_ref: None,
                        timer_expr: None,
                    }),
                ),
            ],
            vec![],
        );
        let (c, _) = one(&orphan, golden_process());
        assert_eq!(
            c.issues,
            vec![ProcessIssue::ExistingProcessNodeConflict {
                node_ref: orphan_id
            }]
        );
    }

    // ------------------------------------------------------------------ order independence

    #[test]
    fn process_requirement_order_independence() {
        let g = base_graph();
        let forward = request(&g, &full_scope(), &["req:r1", "req:r2"]);
        let reversed = request(&g, &full_scope(), &["req:r2", "req:r1"]);
        assert_eq!(forward, reversed);
        let artifact = artifact_for(&forward.request, output(vec![golden_process()]));
        let a = analyze_processes(
            &g,
            &forward,
            Some(ProcessInference {
                artifact: &artifact,
                derivation_ref: derivation(),
            }),
            &audit(),
        )
        .unwrap();
        let b = analyze_processes(
            &g,
            &reversed,
            Some(ProcessInference {
                artifact: &artifact,
                derivation_ref: derivation(),
            }),
            &audit(),
        )
        .unwrap();
        assert_eq!(a.proposals, b.proposals);
        assert_eq!(format!("{:?}", a.candidates), format!("{:?}", b.candidates));
        // Reversed node, next and graph-insertion order give the same proposal.
        let mut p = golden_process();
        p["nodes"].as_array_mut().unwrap().reverse();
        p["next"].as_array_mut().unwrap().reverse();
        let mut n = nodes();
        n.reverse();
        let mut e = edges();
        e.reverse();
        let g2 = graph_of(n, e);
        assert_eq!(
            run(&g2, vec![p]).unwrap().proposals,
            run(&g, vec![golden_process()]).unwrap().proposals
        );
    }

    // ------------------------------------------------------------------ HR process_specs reference

    fn yaml_list(v: &serde_yaml::Value) -> Vec<String> {
        v.as_sequence()
            .map(|s| s.iter().map(|x| x.as_str().unwrap().to_owned()).collect())
            .unwrap_or_default()
    }

    /// Fixture process_specs conformance: each spec compiles to a linear Process (manual start,
    /// one operation-backed task per listed Operation in array order, end). Test-only node kind
    /// convention: Operations performed by a fixture BusinessRole become human_task, others
    /// service_task; node kinds are not a fixture oracle.
    #[test]
    fn process_hr_process_specs_reference() {
        let contract: serde_yaml::Value = serde_yaml::from_str(HR_CONTRACT).unwrap();
        let specs = contract["process_specs"].as_mapping().unwrap();
        let business_roles = yaml_list(&contract["role_specs"]["business_roles"]);
        let operations = yaml_list(&contract["operations"]);
        assert_eq!(specs.len(), 6);
        const STATEMENT: &str =
            "Synthetic grounding statement for the HR process_specs conformance test.";
        let mut graph_nodes = vec![requirement("req:hr", STATEMENT)];
        graph_nodes.extend(
            operations
                .iter()
                .map(|o| operation(&format!("operation:{o}"), Accepted, o)),
        );
        let g = graph_of(graph_nodes, vec![]);
        let scope = ProcessScope {
            operation_refs: operations
                .iter()
                .map(|o| id(&format!("operation:{o}")))
                .collect(),
            ..ProcessScope::default()
        };
        let r = json!({"requirement_ref": "req:hr", "start": 0, "end": STATEMENT.len()});
        let grounded = |target: &str| json!({"target_ref": target, "evidence": [r]});
        let plain = |key: &str, kind: &str| json!({"node_key": key, "node_kind": kind, "operation": null, "performers": [], "produces": [], "message_ref": null, "timer_expr": null, "evidence": [r]});
        let mut expected: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut candidates = Vec::new();
        for (name, ops) in specs {
            let name = name.as_str().unwrap().to_owned();
            let ops = yaml_list(ops);
            let mut nodes = vec![plain("start", "start")];
            for op in &ops {
                let performer = contract["operation_specs"][op.as_str()]["performer"]
                    .as_str()
                    .unwrap();
                let kind = if business_roles.iter().any(|b| b == performer) {
                    "human_task"
                } else {
                    "service_task"
                };
                let mut n = plain(op, kind);
                n["operation"] = grounded(&format!("operation:{op}"));
                nodes.push(n);
            }
            nodes.push(plain("end", "end"));
            let mut keys = vec!["start".to_owned()];
            keys.extend(ops.iter().cloned());
            keys.push("end".to_owned());
            let next: Vec<Json> = keys
                .windows(2)
                .map(|w| json!({"from_key": w[0], "to_key": w[1], "evidence": [r]}))
                .collect();
            candidates.push(json!({"name": name, "evidence": [r], "nodes": nodes, "next": next}));
            expected.insert(name, ops);
        }
        assert_eq!(
            expected.keys().cloned().collect::<Vec<_>>(),
            [
                "CancelApprovedLeave",
                "CreateAndSubmitLeave",
                "ManagerApproval",
                "ManagerRejection",
                "UpdateLeaveBalance",
                "WithdrawSubmittedLeave"
            ]
        );
        assert_eq!(
            expected["UpdateLeaveBalance"],
            ["ReserveLeaveBalance", "RestoreLeaveBalance"]
        );
        let request = build_process_request(&g, &[id("req:hr")], &scope, provider()).unwrap();
        let artifact = artifact_for(
            &request.request,
            json!({"version": 1, "processes": candidates}),
        );
        let result = analyze_processes(
            &g,
            &request,
            Some(ProcessInference {
                artifact: &artifact,
                derivation_ref: derivation(),
            }),
            &audit(),
        )
        .unwrap();
        assert_eq!(result.candidates.len(), 6);
        assert_eq!(result.proposals.len(), 6);
        for c in &result.candidates {
            assert!(c.issues.is_empty(), "{}: {:?}", c.name, c.issues);
            let analysis = c.analysis.as_ref().unwrap();
            assert!(analysis.is_qualified());
            assert_eq!((analysis.start_refs.len(), analysis.end_refs.len()), (1, 1));
            assert!(
                analysis.unreachable_refs.is_empty()
                    && analysis.invalid_terminal_refs.is_empty()
                    && analysis.unresolved_task_refs.is_empty()
                    && analysis.unsupported_nodes.is_empty()
            );
            assert!(analysis.parallel_analysis.issues().is_empty());
            // Walk the applied linear flow and compare the operation sequence.
            let p = result.proposals.iter().find(|p| matches!(&c.disposition, ProcessDisposition::Proposed { proposal_ref } if *proposal_ref == p.id)).unwrap();
            let applied = apply(&g, p);
            let mut current = c.node_refs["start"].clone();
            let mut sequence = Vec::new();
            loop {
                let nexts: Vec<Id> = applied
                    .outgoing_edge_ids(&current)
                    .iter()
                    .map(|e| applied.edge(e).unwrap())
                    .filter(|e| e.kind == RelationKind::Next)
                    .map(|e| e.to.clone())
                    .collect();
                let [following] = nexts.as_slice() else { break };
                current = following.clone();
                if let NodePayload::ProcessNode(pn) = &applied.node(&current).unwrap().payload {
                    if let Some(op) = &pn.operation_ref {
                        let NodePayload::Operation(o) = &applied.node(op).unwrap().payload else {
                            unreachable!()
                        };
                        sequence.push(o.name.clone());
                    }
                }
            }
            assert_eq!(sequence, expected[&c.name], "{}", c.name);
        }
    }

    // ------------------------------------------------------------------ guards

    #[test]
    fn process_source_guard() {
        let source = include_str!("../src/process.rs");
        for forbidden in [
            "std::fs",
            "File::open",
            "reqwest",
            "std::net",
            "SystemClock",
            "Clock::now",
            "Utc::now",
            "Instant::now",
            "ArtifactStore",
            "RevisionStore",
            "rusqlite",
            ".execute(",
            "MockProvider",
            "rand::",
            "f64",
            "f32",
            "unsafe",
            "segment_origin",
            "SEGMENT_ORIGIN",
            "paragraph_index",
            "sentence_index",
            "document_order",
            "source_order",
            "paragraph",
            "ordinal",
            "mermaid",
            "Mermaid",
            "bpmn",
            "BPMN",
            "RelationProperties::Extension",
            "condition_expr: Some",
            "fixtures/",
            "hr-leave",
            "LeaveRequest",
            "ManagerApproval",
            "UpdateLeaveBalance",
            "PLUMB.F2",
            "GeneratedFinding",
            "EvaluatorRegistry",
            "RuleEvaluator",
            "ReplacePayload",
            "MergeNodes",
            "Supersede",
            "SetStatus",
            "plumb_expr",
        ] {
            assert!(
                !source.contains(forbidden),
                "process.rs contains {forbidden}"
            );
        }
        let analysis = include_str!("../../plumb-validation/src/process_analysis.rs");
        for forbidden in [
            "RuleEvaluator",
            "GeneratedFinding",
            "EvaluatorRegistry",
            "GateReport",
            "std::fs",
            "f64",
            "plumb_functional",
            "PLUMB.F2",
            "BPMN.F2",
        ] {
            assert!(
                !analysis.contains(forbidden),
                "process_analysis.rs contains {forbidden}"
            );
        }
        let lib = include_str!("../src/lib.rs");
        assert!(!lib.contains("Proposal"));
        assert!(lib.contains("pub mod process;"));
    }
}
