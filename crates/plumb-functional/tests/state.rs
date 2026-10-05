//! S2.2 contract tests for grounded lifecycle extraction: the schema and prompt, the
//! lifecycle request, inference validation, grounded States, triggered Transitions, the
//! missing-trigger finding material, governed trigger decisions, unqualified Invariant
//! proposals, reconciliation, determinism and the real HR diagnostic.
//!
//! Every test is named `s2_state_*` so that `cargo test -p plumb-functional s2_state_` selects
//! exactly them. Golden values were computed independently with Python `hashlib` over RFC 8785
//! JSON (and the finding-key bytes), never with the helpers under test. The synthetic lifecycle
//! is a structural conformance fixture, not an extraction-accuracy benchmark. No PlumbExpr
//! parser or typechecker runs anywhere in S2.2: invariant expressions are only proposed.

mod lifecycle_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use jsonschema::{Draft, JSONSchema};
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_functional::state::*;
    use plumb_functional::{
        analyze_domain, build_domain_request, build_requirement_classification_request,
        compile_requirement_candidates, DomainAudit, DomainInference, InvariantOrigin,
        RequirementClassificationInference, RequirementCompilationAudit,
        INVARIANT_ORIGIN_EXTENSION,
    };
    use plumb_import::{
        build_segmentation_request, evaluate_segmentation, import_markdown, import_plain_text,
        ImportAudit, SegmentationAudit,
    };
    use plumb_inference::{InferenceArtifact, InferenceRequest, ProviderPolicy};
    use plumb_patch::{
        apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        Agent, AgentKind, Attribute, AuditMeta, DerivationRef, Edge, ElementStatus, Entity, Event,
        EvidenceRef, FindingSeverity, Graph, Modality, Node, NodePayload, NodeType, Operation,
        OperationKind, Process, Question, QuestionKind, RelationKind, RelationProperties,
        Requirement, RequirementKind, RequirementLevel, ResolutionDecision, State,
    };
    use plumb_validation::GeneratedFinding;
    use serde_json::{json, Value};

    use ElementStatus::{Accepted, Proposed, Rejected};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s2-lifecycle.md");
    const SCHEMA: &str = include_str!("../../../schemas/inference/s2-lifecycle.schema.json");
    const HR_MD: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.md");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:7599e88a965d4b3cb82b2add45f3102b79ab2f9d80f1ca20bbba572f0af55232";
    const SCHEMA_HASH: &str =
        "sha256:dedd8865046ee32af5fd55954027f5f037e4a3e80b702884c783906cf6265298";
    const GOLDEN_CONTEXT_HASH: &str =
        "sha256:2a674e2fb28ffadb2bcae0a2a01be14265c7bf8f4643a9b8a68aa4d18a248b81";
    const GOLDEN_REQUEST_ID: &str =
        "sha256:5748d571a8860c7d45c5c2aa6f7611a874506e65c4e1c4d24098c26975432773";
    const SUBMITTED: &str = "state:34fea847d6cbbed1";
    const APPROVED: &str = "state:1af3ab50cd177533";
    const TRANSITION: &str = "transition:1824db448b1cb9d6";
    const INVARIANT: &str = "invariant:0515bbed12c15b49";
    const HAS_STATE_EDGE: &str = "rel:bfd316ad63286a15";
    const TRANSITIONS_VIA_EDGE: &str = "rel:7c2de2a38fffb295";
    const TRIGGER_FINDING_KEY: &str =
        "sha256:fda9fe0fa7452b0a7235e7ab8cfbb2fefdf3836da5218b876e7a20b831c8539c";
    const TRIGGER_FINDING_ID: &str = "fnd:fda9fe0fa7452b0a";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const TRANSITION_COMPLETE: &str = "PLUMB.F2.STATE.TRANSITION_COMPLETE";

    // Synthetic test data only; never in production source.
    const R1: &str = "A request moves from submitted to approved when approval is recorded.";
    const R2: &str = "The approved amount must never exceed the requested amount.";
    const OWNER: &str = "entity:request";
    const OPERATION: &str = "operation:approve";
    const EVENT: &str = "event:approval-recorded";
    const EXPRESSION: &str = "approved_amount <= requested_amount";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ids(v: &[&str]) -> Vec<Id> {
        v.iter().map(|s| id(s)).collect()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn provider(name: &str) -> ProviderPolicy {
        ProviderPolicy {
            provider: name.to_owned(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn derivation() -> DerivationRef {
        DerivationRef::from(id("drv:00000000000000e2"))
    }

    fn audit_at(at: &str) -> LifecycleAudit {
        LifecycleAudit {
            created_by: id("agent:lifecycle"),
            created_at: ts(at),
        }
    }

    fn audit() -> LifecycleAudit {
        audit_at(AT)
    }

    fn artifact_for(request: &InferenceRequest, output: Value) -> InferenceArtifact {
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
            audit: AuditMeta::new(id("actor:human"), ts(AT), None, None).unwrap(),
        }
    }

    fn requirement_node(node_id: &str, status: ElementStatus, statement: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::Requirement(Requirement {
                statement: statement.to_owned(),
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

    fn entity_node(node_id: &str, status: ElementStatus, name: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::Entity(Entity {
                name: name.into(),
                description: None,
                aggregate_root: None,
            }),
        )
    }

    fn operation_node(node_id: &str, status: ElementStatus, name: &str) -> Node {
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

    fn event_node(node_id: &str, status: ElementStatus, name: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::Event(Event {
                name: name.into(),
                payload_schema_ref: None,
                semantic_type: None,
            }),
        )
    }

    fn new_graph(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    /// The golden graph: two Accepted Requirements, one Proposed Entity owner and one
    /// Proposed Operation trigger, no evidence.
    fn golden_graph() -> Graph {
        new_graph(
            vec![
                requirement_node("req:r1", Accepted, R1),
                requirement_node("req:r2", Accepted, R2),
                entity_node(OWNER, Proposed, "Request"),
                operation_node(OPERATION, Proposed, "ApproveRequest"),
            ],
            vec![],
        )
    }

    /// The golden graph with imported evidence on both Requirements and the owner.
    fn base_graph() -> Graph {
        let imported = import_plain_text(
            "synthetic.txt",
            [R1, R2].join("\n\n").as_bytes(),
            &ImportAudit {
                created_by: id("actor:importer"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        let refs: Vec<EvidenceRef> = imported
            .fragments
            .iter()
            .map(|f| EvidenceRef::from(f.id.clone()))
            .collect();
        let mut nodes: Vec<Node> = golden_graph().nodes().values().cloned().collect();
        for n in &mut nodes {
            match n.id.as_str() {
                "req:r1" => n.evidence = vec![refs[0].clone()],
                "req:r2" | OWNER => n.evidence = vec![refs[1].clone()],
                _ => {}
            }
        }
        nodes.push(imported.source);
        nodes.extend(imported.fragments);
        new_graph(nodes, vec![])
    }

    fn extend(graph: &Graph, nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        let mut all: Vec<Node> = graph.nodes().values().cloned().collect();
        all.extend(nodes);
        let mut all_edges: Vec<Edge> = graph.edges().values().cloned().collect();
        all_edges.extend(edges);
        Graph::new(
            graph.project_id().clone(),
            graph.profile_id().clone(),
            all,
            all_edges,
        )
        .unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn map_nodes(graph: &Graph, f: impl Fn(&mut Node)) -> Graph {
        let nodes = graph
            .nodes()
            .values()
            .cloned()
            .map(|mut n| {
                f(&mut n);
                n
            })
            .collect();
        Graph::new(
            graph.project_id().clone(),
            graph.profile_id().clone(),
            nodes,
            graph.edges().values().cloned().collect(),
        )
        .unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn ground(text: &str, requirement: &str, needle: &str, nth: usize) -> Value {
        let start = text
            .match_indices(needle)
            .nth(nth)
            .unwrap_or_else(|| panic!("{needle:?} #{nth} not in {text:?}"))
            .0;
        json!({"requirement_ref": requirement, "start": start, "end": start + needle.len()})
    }

    fn r1(needle: &str) -> Value {
        ground(R1, "req:r1", needle, 0)
    }

    fn state_json(owner: &str, grounding: Value) -> Value {
        json!({"stateful_ref": owner, "grounding": grounding})
    }

    fn transition_json(trigger: Value) -> Value {
        json!({
            "stateful_ref": OWNER, "grounding": r1("moves from submitted to approved"),
            "from_state": r1("submitted"), "to_state": r1("approved"), "trigger_ref": trigger,
        })
    }

    fn invariant_json(expression: &str) -> Value {
        json!({"scope_ref": OWNER,
               "grounding": {"requirement_ref": "req:r2", "start": 0, "end": R2.len()},
               "expression": expression})
    }

    fn states() -> Vec<Value> {
        vec![
            state_json(OWNER, r1("submitted")),
            state_json(OWNER, r1("approved")),
        ]
    }

    fn output(states: Vec<Value>, transitions: Vec<Value>, invariants: Vec<Value>) -> Value {
        json!({"version": 1, "states": states, "transitions": transitions, "invariants": invariants})
    }

    fn empty_output() -> Value {
        output(vec![], vec![], vec![])
    }

    fn full_output(trigger: Value) -> Value {
        output(
            states(),
            vec![transition_json(trigger)],
            vec![invariant_json(EXPRESSION)],
        )
    }

    fn request(graph: &Graph) -> LifecycleRequest {
        build_lifecycle_request(graph, provider("mock")).unwrap()
    }

    fn analyze_with(
        graph: &Graph,
        request: &LifecycleRequest,
        artifact: &InferenceArtifact,
    ) -> Result<LifecycleAnalysisResult, LifecycleError> {
        analyze_lifecycle(
            graph,
            request,
            Some(LifecycleInference {
                artifact,
                derivation_ref: derivation(),
            }),
            &audit(),
        )
    }

    fn analyze(graph: &Graph, output: Value) -> Result<LifecycleAnalysisResult, LifecycleError> {
        let request = request(graph);
        let artifact = artifact_for(&request.request, output);
        analyze_with(graph, &request, &artifact)
    }

    /// Applies proposals in TEST CODE, rebasing each patch onto the current graph.
    fn apply(graph: &Graph, proposals: &[Proposal]) -> Graph {
        let mut g = graph.clone();
        for p in proposals {
            let patch_set = PatchSet {
                base_semantic_hash: g.semantic_hash().unwrap(),
                patch: p.patch_set.patch.clone(),
            };
            g = apply_patch(&g, &patch_set).unwrap().graph;
        }
        g.validate().unwrap();
        g
    }

    fn added(proposal: &Proposal) -> (&Node, Vec<&Edge>) {
        match &proposal.patch_set.patch {
            SemanticPatch::AddNode { node } => (node, vec![]),
            SemanticPatch::Compound { patches } => {
                let SemanticPatch::AddNode { node } = &patches[0] else {
                    panic!("first leaf is not AddNode");
                };
                let edges = patches[1..]
                    .iter()
                    .map(|p| match p {
                        SemanticPatch::AddEdge { edge } => edge,
                        other => panic!("unexpected leaf {other:?}"),
                    })
                    .collect();
                (node, edges)
            }
            other => panic!("unexpected patch {other:?}"),
        }
    }

    fn of_type(result: &LifecycleAnalysisResult, node_type: NodeType) -> Vec<&Proposal> {
        result
            .proposals
            .iter()
            .filter(|p| added(p).0.payload.node_type() == node_type)
            .collect()
    }

    fn origin(node: &Node, key: &str) -> Value {
        node.extensions
            .iter()
            .find(|(k, _)| k.as_str() == key)
            .map(|(_, v)| v.clone())
            .unwrap()
    }

    fn evidence_of(graph: &Graph, nodes: &[&str]) -> Vec<EvidenceRef> {
        let refs: BTreeSet<EvidenceRef> = nodes
            .iter()
            .flat_map(|r| graph.node(&id(r)).unwrap().evidence.clone())
            .collect();
        refs.into_iter().collect()
    }

    /// The base graph after the two State proposals were applied in test code.
    fn with_states() -> Graph {
        let g = base_graph();
        let first = analyze(&g, output(states(), vec![], vec![])).unwrap();
        assert_eq!(first.proposals.len(), 2);
        apply(&g, &first.proposals)
    }

    // ------------------------------------------------------------------ schema and prompt

    #[test]
    fn s2_state_schema_contract() {
        let value: Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(
            value["$schema"],
            json!("https://json-schema.org/draft/2020-12/schema")
        );
        let schema = JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&value)
            .unwrap();
        let valid = |v: &Value| schema.is_valid(v);
        assert!(valid(&empty_output()));
        assert!(valid(&full_output(json!(OPERATION))));
        assert!(valid(&full_output(Value::Null)));
        let mut root = empty_output();
        root["extra"] = json!(1);
        assert!(!valid(&root));
        for missing in ["version", "states", "transitions", "invariants"] {
            let mut v = empty_output();
            v.as_object_mut().unwrap().remove(missing);
            assert!(!valid(&v), "{missing}");
        }
        let with = |section: &str, key: &str, value: Value| {
            let mut v = full_output(json!(OPERATION));
            v[section][0][key] = value;
            v
        };
        let without = |section: &str, key: &str| {
            let mut v = full_output(json!(OPERATION));
            v[section][0].as_object_mut().unwrap().remove(key);
            v
        };
        // Unknown fields and free names.
        for (section, key) in [
            ("states", "extra"),
            ("states", "name"),
            ("states", "state_name"),
            ("transitions", "extra"),
            ("transitions", "from_state_name"),
            ("transitions", "guard_expr"),
            ("invariants", "extra"),
            ("invariants", "parsed"),
        ] {
            assert!(!valid(&with(section, key, json!("x"))), "{section}.{key}");
        }
        // Bad grounding shapes and free from/to names.
        assert!(!valid(&with(
            "states",
            "grounding",
            json!({"requirement_ref": "req:r1", "start": 0})
        )));
        assert!(!valid(&with(
            "states",
            "grounding",
            json!({"requirement_ref": "req:r1", "start": 0, "end": 1, "x": 1})
        )));
        assert!(!valid(&with(
            "transitions",
            "from_state",
            json!("submitted")
        )));
        assert!(!valid(&with("transitions", "to_state", json!("approved"))));
        // trigger_ref is required but nullable, and must be an ID.
        assert!(!valid(&without("transitions", "trigger_ref")));
        assert!(!valid(&with("transitions", "trigger_ref", json!(7))));
        assert!(!valid(&with(
            "transitions",
            "trigger_ref",
            json!("Approve Request")
        )));
        // Invariants need scope, grounding and a non-empty expression.
        for key in ["scope_ref", "grounding", "expression"] {
            assert!(!valid(&without("invariants", key)), "{key}");
        }
        assert!(!valid(&with("invariants", "expression", json!(""))));
        assert!(!valid(&with("invariants", "expression", Value::Null)));
        fn walk(v: &Value) {
            match v {
                Value::Object(map) => {
                    if let Some(r) = map.get("$ref") {
                        assert!(r.as_str().unwrap().starts_with("#/$defs/"), "{r}");
                    }
                    if map.get("type") == Some(&json!("object")) {
                        assert_eq!(map.get("additionalProperties"), Some(&json!(false)));
                    }
                    map.values().for_each(walk);
                }
                Value::Array(items) => items.iter().for_each(walk),
                _ => {}
            }
        }
        walk(&value);
    }

    #[test]
    fn s2_state_prompt_and_schema_hashes() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
        let prompt = std::str::from_utf8(PROMPT).unwrap();
        for required in [
            "Use only the supplied Accepted Requirement text",
            "Do not invent owners",
            "Capitalization or a field-like name",
            "UTF-8 byte offsets",
            "Do not return state names",
            "Use trigger_ref only from the supplied Operation/Event trigger list",
            "return null",
            "Do not invent a trigger",
            "Do not return guard or effect expressions",
            "proposed PlumbExpr expression",
            "Do not claim that the",
            "Return only JSON",
        ] {
            assert!(prompt.contains(required), "{required}");
        }
        for text in [prompt, SCHEMA] {
            for forbidden in ["G_STATE", "I_STATE", "TODO", "FIXME", "{{", "state_name"] {
                assert!(!text.contains(forbidden), "{forbidden}");
            }
        }
    }

    // ------------------------------------------------------------------ request

    #[test]
    fn s2_state_request_golden_and_context() {
        let g = golden_graph();
        let r = request(&g);
        assert_eq!(r.request.id.as_str(), GOLDEN_REQUEST_ID);
        assert_eq!(r.request.context_hash.as_str(), GOLDEN_CONTEXT_HASH);
        assert_eq!(r.request.stage, StageId::S2);
        assert_eq!(r.request.task_kind, "lifecycle_extraction");
        assert_eq!(r.request.prompt_template_hash.as_str(), PROMPT_HASH);
        assert_eq!(r.request.schema_hash.as_str(), SCHEMA_HASH);
        assert_eq!(
            r.request.input_refs,
            ids(&[OWNER, OPERATION, "req:r1", "req:r2"])
        );
        assert!(r.request.evidence_refs.is_empty());
        assert_eq!(
            serde_json::to_value(&r.context.owners).unwrap(),
            json!([{"stateful_ref": OWNER, "node_type": "Entity", "name": "Request"}])
        );
        assert_eq!(
            serde_json::to_value(&r.context.triggers).unwrap(),
            json!([{"trigger_ref": OPERATION, "node_type": "Operation", "name": "ApproveRequest"}])
        );
        assert_eq!(LIFECYCLE_CONTEXT_VERSION, 1);
        assert_eq!(LIFECYCLE_OUTPUT_VERSION, 1);
        assert_eq!(LIFECYCLE_TASK_KIND, "lifecycle_extraction");
        assert_eq!(STATE_ORIGIN_EXTENSION, "plumb_functional:state_origin");
        assert_eq!(
            INVARIANT_ORIGIN_EXTENSION,
            "plumb_functional:invariant_origin"
        );
        // Proposed Requirements, inactive owners and triggers, and lifecycle nodes are never
        // context; Process owners are StateOwners.
        let g2 = extend(
            &g,
            vec![
                requirement_node("req:r0", Proposed, "A draft shall exist."),
                entity_node("entity:gone", Rejected, "Gone"),
                operation_node("operation:gone", Rejected, "Gone"),
                event_node("event:old", ElementStatus::Superseded, "Old"),
                node(
                    "state:hand",
                    Proposed,
                    NodePayload::State(State {
                        name: "draft".into(),
                    }),
                ),
                node(
                    "invariant:hand",
                    Proposed,
                    NodePayload::Invariant(plumb_psg::Invariant {
                        scope_ref: id(OWNER),
                        expression: "x".into(),
                    }),
                ),
            ],
            vec![],
        );
        assert_eq!(request(&g2), r);
        let g3 = extend(
            &g,
            vec![
                node(
                    "process:handle",
                    Accepted,
                    NodePayload::Process(Process {
                        name: "Handle Request".into(),
                        description: None,
                        process_kind: None,
                    }),
                ),
                event_node(EVENT, Accepted, "ApprovalRecorded"),
            ],
            vec![],
        );
        let r3 = request(&g3);
        assert_eq!(
            r3.context
                .owners
                .iter()
                .map(|o| (o.stateful_ref.as_str(), o.node_type))
                .collect::<Vec<_>>(),
            [
                (OWNER, NodeType::Entity),
                ("process:handle", NodeType::Process)
            ]
        );
        assert_eq!(
            r3.context
                .triggers
                .iter()
                .map(|t| (t.trigger_ref.as_str(), t.node_type))
                .collect::<Vec<_>>(),
            [(EVENT, NodeType::Event), (OPERATION, NodeType::Operation)]
        );
        // A StateOwner payload with an unusable name is an invalid owner.
        let unnamed = extend(
            &g,
            vec![entity_node("entity:unnamed", Proposed, " padded")],
            vec![],
        );
        assert!(matches!(
            build_lifecycle_request(&unnamed, provider("mock")),
            Err(LifecycleError::InvalidOwner { .. })
        ));
    }

    #[test]
    fn s2_state_request_evidence_and_provider_policy() {
        let g = base_graph();
        let r = request(&g);
        let expected: BTreeSet<Id> = evidence_of(&g, &["req:r1", "req:r2", OWNER])
            .into_iter()
            .map(|e| e.as_id().clone())
            .collect();
        assert_eq!(expected.len(), 2);
        assert_eq!(
            r.request.evidence_refs,
            expected.into_iter().collect::<Vec<_>>()
        );
        let other = build_lifecycle_request(&g, provider("other")).unwrap();
        assert_ne!(other.request.id, r.request.id);
        assert_eq!(other.context, r.context);
        let ids_of = |res: &LifecycleAnalysisResult| -> Vec<Id> {
            res.proposals
                .iter()
                .map(|p| added(p).0.id.clone())
                .collect()
        };
        let a = analyze_with(
            &g,
            &r,
            &artifact_for(&r.request, full_output(json!(OPERATION))),
        )
        .unwrap();
        let b = analyze_with(
            &g,
            &other,
            &artifact_for(&other.request, full_output(json!(OPERATION))),
        )
        .unwrap();
        assert_eq!(ids_of(&a), ids_of(&b));
    }

    #[test]
    fn s2_state_request_staleness() {
        let g = base_graph();
        let r = request(&g);
        let artifact = artifact_for(&r.request, empty_output());
        let stale = |changed: Graph| {
            assert!(matches!(
                analyze_with(&changed, &r, &artifact),
                Err(LifecycleError::InvalidInput { .. })
            ));
        };
        let edit = |target: &'static str, f: fn(&mut Node)| {
            map_nodes(&g, move |n| {
                if n.id.as_str() == target {
                    f(n);
                }
            })
        };
        stale(edit("req:r2", |n| {
            if let NodePayload::Requirement(r) = &mut n.payload {
                r.statement.push_str(" Always.");
            }
        }));
        stale(edit("req:r2", |n| n.status = Proposed));
        stale(edit(OWNER, |n| {
            if let NodePayload::Entity(e) = &mut n.payload {
                e.name = "Claim".into();
            }
        }));
        stale(edit(OWNER, |n| {
            n.payload = NodePayload::Process(Process {
                name: "Request".into(),
                description: None,
                process_kind: None,
            })
        }));
        stale(edit(OWNER, |n| n.status = Rejected));
        stale(edit(OPERATION, |n| {
            if let NodePayload::Operation(o) = &mut n.payload {
                o.name = "ApproveClaim".into();
            }
        }));
        stale(edit(OPERATION, |n| {
            n.payload = NodePayload::Event(Event {
                name: "ApproveRequest".into(),
                payload_schema_ref: None,
                semantic_type: None,
            })
        }));
        stale(edit(OPERATION, |n| n.status = Rejected));
        stale(extend(
            &g,
            vec![event_node(EVENT, Proposed, "ApprovalRecorded")],
            vec![],
        ));
        stale(
            Graph::new(
                id("project:other"),
                g.profile_id().clone(),
                g.nodes().values().cloned().collect(),
                vec![],
            )
            .unwrap(),
        );
        // Applied State proposals do not stale the request (the two-pass basis).
        let applied = with_states();
        assert_eq!(request(&applied), r);
        analyze_with(&applied, &r, &artifact).unwrap();
    }

    #[test]
    fn s2_state_inference_absent_and_owner_unavailable() {
        let g = base_graph();
        let r = request(&g);
        let absent = analyze_lifecycle(&g, &r, None, &audit()).unwrap();
        assert_eq!(absent.issues, vec![LifecycleIssue::InferenceUnavailable]);
        assert!(absent.proposals.is_empty() && absent.findings.is_empty());
        assert!(absent.conflicts.is_empty() && absent.existing.is_empty());
        let no_owner = new_graph(
            vec![
                requirement_node("req:r1", Accepted, R1),
                requirement_node("req:r2", Accepted, R2),
            ],
            vec![],
        );
        let result = analyze(&no_owner, empty_output()).unwrap();
        assert_eq!(result.issues, vec![LifecycleIssue::StateOwnerUnavailable]);
        assert!(result.proposals.is_empty() && result.findings.is_empty());
        // Naming an owner outside the context is malformed inference, not a missing owner.
        assert!(matches!(
            analyze(
                &no_owner,
                output(vec![state_json(OWNER, r1("submitted"))], vec![], vec![])
            ),
            Err(LifecycleError::UnknownOwnerRef { .. })
        ));
    }

    // ------------------------------------------------------------------ inference validation

    #[test]
    fn s2_state_inference_validation() {
        let g = extend(
            &base_graph(),
            vec![entity_node("entity:gone", Rejected, "Gone")],
            vec![],
        );
        let r = request(&g);
        let good = artifact_for(&r.request, full_output(json!(OPERATION)));
        analyze_with(&g, &r, &good).unwrap();
        let mut wrong_request = good.clone();
        wrong_request.request_hash = Hash::content_sha256(b"other request");
        let mut wrong_provider = good.clone();
        wrong_provider.provider = "other".into();
        let mut wrong_hash = good.clone();
        wrong_hash.validated_output_hash = Hash::content_sha256(b"other output");
        for artifact in [wrong_request, wrong_provider, wrong_hash] {
            assert!(matches!(
                analyze_with(&g, &r, &artifact),
                Err(LifecycleError::InvalidInferenceArtifact { .. })
            ));
        }
        let run = |v: Value| analyze_with(&g, &r, &artifact_for(&r.request, v));
        assert!(matches!(
            run(json!({"version": 1, "states": []})),
            Err(LifecycleError::SchemaInvalid { .. })
        ));
        // Owners: unknown, inactive, wrong category.
        for owner in ["entity:ghost", "entity:gone"] {
            assert!(matches!(
                run(output(
                    vec![state_json(owner, r1("submitted"))],
                    vec![],
                    vec![]
                )),
                Err(LifecycleError::UnknownOwnerRef { .. })
            ));
        }
        for owner in ["req:r1", OPERATION] {
            assert!(matches!(
                run(output(
                    vec![state_json(owner, r1("submitted"))],
                    vec![],
                    vec![]
                )),
                Err(LifecycleError::InvalidOwner { .. })
            ));
        }
        let mut inv = invariant_json(EXPRESSION);
        inv["scope_ref"] = json!("req:r2");
        assert!(matches!(
            run(output(vec![], vec![], vec![inv])),
            Err(LifecycleError::InvalidOwner { .. })
        ));
        // Triggers: unknown and wrong type are malformed inference.
        for trigger in ["operation:ghost", OWNER] {
            assert!(matches!(
                run(full_output(json!(trigger))),
                Err(LifecycleError::InvalidTriggerRef { .. })
            ));
        }
        // Groundings.
        for grounding in [
            json!({"requirement_ref": "req:r9", "start": 0, "end": 3}),
            json!({"requirement_ref": "req:r1", "start": 4, "end": 4}),
            json!({"requirement_ref": "req:r1", "start": 0, "end": R1.len() + 1}),
            ground(R1, "req:r1", " submitted", 0),
            ground(R1, "req:r1", "submitted ", 0),
        ] {
            assert!(matches!(
                run(output(vec![state_json(OWNER, grounding)], vec![], vec![])),
                Err(LifecycleError::InvalidGrounding { .. })
            ));
        }
        // Expressions: clean text only (empty is schema-invalid).
        for expression in [" x > 1", "x > 1 ", "x >\t1", "x\n> 1"] {
            assert!(matches!(
                run(output(vec![], vec![], vec![invariant_json(expression)])),
                Err(LifecycleError::InvalidExpression { .. })
            ));
        }
        // Duplicate candidates.
        for duplicated in [
            output(
                vec![
                    state_json(OWNER, r1("submitted")),
                    state_json(OWNER, r1("submitted")),
                ],
                vec![],
                vec![],
            ),
            output(
                states(),
                vec![
                    transition_json(Value::Null),
                    transition_json(json!(OPERATION)),
                ],
                vec![],
            ),
            output(
                vec![],
                vec![],
                vec![invariant_json("a"), invariant_json("b")],
            ),
        ] {
            assert!(matches!(
                run(duplicated),
                Err(LifecycleError::DuplicateCandidate { .. })
            ));
        }
        // An endpoint that is not a state candidate is never invented.
        let mut t = transition_json(json!(OPERATION));
        t["to_state"] = r1("approval");
        assert!(matches!(
            run(output(states(), vec![t], vec![])),
            Err(LifecycleError::UnknownStateCandidate { state_key, .. }) if state_key == "approval"
        ));
        assert!(matches!(
            run(output(
                vec![],
                vec![transition_json(json!(OPERATION))],
                vec![]
            )),
            Err(LifecycleError::UnknownStateCandidate { .. })
        ));
    }

    #[test]
    fn s2_state_utf8_split_grounding() {
        let accented = "A request moves from soumis to approuvé when approval is recorded.";
        let g = map_nodes(&golden_graph(), |n| {
            if let NodePayload::Requirement(r) = &mut n.payload {
                if n.id.as_str() == "req:r1" {
                    r.statement = accented.into();
                }
            }
        });
        let start = accented.find("approuvé").unwrap();
        let split = json!({"requirement_ref": "req:r1", "start": start, "end": start + "approuv".len() + 1});
        assert!(matches!(
            analyze(&g, output(vec![state_json(OWNER, split)], vec![], vec![])),
            Err(LifecycleError::InvalidGrounding { .. })
        ));
        let whole =
            json!({"requirement_ref": "req:r1", "start": start, "end": start + "approuvé".len()});
        let result = analyze(&g, output(vec![state_json(OWNER, whole)], vec![], vec![])).unwrap();
        assert_eq!(
            added(&result.proposals[0]).0.payload,
            NodePayload::State(State {
                name: "approuvé".into()
            })
        );
    }

    // ------------------------------------------------------------------ states

    #[test]
    fn s2_state_state_proposal_structure() {
        let g = base_graph();
        let result = analyze(&g, output(states(), vec![], vec![])).unwrap();
        assert_eq!(result.proposals.len(), 2);
        let approved = result
            .proposals
            .iter()
            .find(|p| added(p).0.id == id(APPROVED))
            .unwrap();
        assert!(result
            .proposals
            .iter()
            .any(|p| added(p).0.id == id(SUBMITTED)));
        let (node, edges) = added(approved);
        assert_eq!(
            node.payload,
            NodePayload::State(State {
                name: "approved".into()
            })
        );
        assert_eq!(node.status, Proposed);
        assert_eq!(node.revision, 1);
        assert_eq!(node.evidence, evidence_of(&g, &["req:r1"]));
        assert!(node.derivations.is_empty() && node.standards.is_empty() && node.tags.is_empty());
        assert_eq!(node.extensions.len(), 1);
        assert_eq!(
            origin(node, STATE_ORIGIN_EXTENSION),
            json!({"kind": "state", "stateful_ref": OWNER, "state_key": "approved", "grounding": r1("approved")})
        );
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].id, id(HAS_STATE_EDGE));
        assert_eq!(
            (&edges[0].kind, edges[0].from.as_str(), edges[0].to.as_str()),
            (&RelationKind::HasState, OWNER, APPROVED)
        );
        assert_eq!(edges[0].status, Proposed);
        assert_eq!(edges[0].properties, RelationProperties::None);
        assert_eq!(edges[0].evidence, node.evidence);
        assert_eq!(approved.stage, StageId::S2);
        assert_eq!(approved.materiality, ProposalMateriality::Semantic);
        assert_eq!(approved.acceptance_policy, AcceptancePolicy::HumanConfirm);
        assert_eq!(approved.confidence, None);
        assert_eq!(approved.derivation_refs, vec![derivation()]);
        assert_eq!(approved.evidence_refs, node.evidence);
        assert_eq!(
            approved.patch_set.base_semantic_hash,
            g.semantic_hash().unwrap()
        );
        // Applied in test code: exactly one active has_state owner.
        let applied = apply(&g, &result.proposals);
        for state in [SUBMITTED, APPROVED] {
            let owners: Vec<&Id> = applied
                .incoming_edge_ids(&id(state))
                .iter()
                .map(|e| applied.edge(e).unwrap())
                .filter(|e| e.kind == RelationKind::HasState)
                .map(|e| &e.from)
                .collect();
            assert_eq!(owners, vec![&id(OWNER)]);
        }
        let order: Vec<&Id> = result.proposals.iter().map(|p| &p.id).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
    }

    #[test]
    fn s2_state_normalization_and_owner_identity() {
        let shouting = "A request moves from SUBMITTED to Approved when approval is recorded.";
        let g = extend(
            &map_nodes(&golden_graph(), |n| {
                if let NodePayload::Requirement(r) = &mut n.payload {
                    if n.id.as_str() == "req:r2" {
                        r.statement = shouting.into();
                    }
                }
            }),
            vec![entity_node("entity:claim", Proposed, "Claim")],
            vec![],
        );
        let upper =
            |owner: &str, needle: &str| state_json(owner, ground(shouting, "req:r2", needle, 0));
        // Capitalization does not change the key or the ID.
        let a = analyze(
            &g,
            output(
                vec![upper(OWNER, "SUBMITTED"), upper(OWNER, "Approved")],
                vec![],
                vec![],
            ),
        )
        .unwrap();
        let state_ids: BTreeSet<Id> = a.proposals.iter().map(|p| added(p).0.id.clone()).collect();
        assert_eq!(state_ids, BTreeSet::from([id(SUBMITTED), id(APPROVED)]));
        // The same key under another owner is another State.
        let b = analyze(
            &g,
            output(vec![upper("entity:claim", "SUBMITTED")], vec![], vec![]),
        )
        .unwrap();
        assert_ne!(added(&b.proposals[0]).0.id, id(SUBMITTED));
        assert_eq!(
            added(&b.proposals[0]).0.payload,
            NodePayload::State(State {
                name: "submitted".into()
            })
        );
        // Plural/determiner normalization is the S1.5 one.
        let c = analyze(
            &g,
            output(vec![state_json(OWNER, r1("A request"))], vec![], vec![]),
        )
        .unwrap();
        assert_eq!(
            added(&c.proposals[0]).0.payload,
            NodePayload::State(State {
                name: "request".into()
            })
        );
    }

    #[test]
    fn s2_state_capitalization_and_field_name_negative() {
        // Capitalized words and an Attribute named Status: without explicit grounded state
        // candidates nothing is created.
        let g = extend(
            &map_nodes(&base_graph(), |n| {
                if let NodePayload::Requirement(r) = &mut n.payload {
                    if n.id.as_str() == "req:r2" {
                        r.statement = "The Status shall be Pending, Approved or Rejected.".into();
                    }
                }
            }),
            vec![node(
                "attr:status",
                Proposed,
                NodePayload::Attribute(Attribute {
                    name: "Status".into(),
                    value_type: "Text".into(),
                    nullable: false,
                    unit: None,
                    precision: None,
                    enum_values: Some(vec!["Pending".into(), "Approved".into()]),
                    data_classification: None,
                }),
            )],
            vec![Edge {
                id: id("rel:owner-status"),
                revision: 1,
                status: Proposed,
                kind: RelationKind::HasAttribute,
                from: id(OWNER),
                to: id("attr:status"),
                properties: RelationProperties::None,
                evidence: Vec::new(),
                derivations: Vec::new(),
                standards: Vec::new(),
                audit: AuditMeta::new(id("agent:domain"), ts(AT), None, None).unwrap(),
            }],
        );
        let result = analyze(&g, empty_output()).unwrap();
        assert!(result.proposals.is_empty());
        assert!(result.issues.is_empty());
        assert!(analyze_lifecycle(&g, &request(&g), None, &audit())
            .unwrap()
            .proposals
            .is_empty());
    }

    // ------------------------------------------------------------------ transitions

    #[test]
    fn s2_state_two_pass_transition() {
        let g = base_graph();
        let r = request(&g);
        let artifact = artifact_for(&r.request, full_output(json!(OPERATION)));
        // Pass 1: States and the Invariant; the Transition waits for its States.
        let first = analyze_with(&g, &r, &artifact).unwrap();
        assert_eq!(of_type(&first, NodeType::State).len(), 2);
        assert!(of_type(&first, NodeType::Transition).is_empty());
        assert_eq!(of_type(&first, NodeType::Invariant).len(), 1);
        assert!(first.findings.is_empty());
        assert_eq!(
            first.issues,
            vec![LifecycleIssue::StateDependencyPending {
                candidate_ref: id(TRANSITION),
                state_refs: ids(&[APPROVED, SUBMITTED]),
            }]
        );
        // Apply in test code; replay the SAME request and artifact.
        let g2 = apply(&g, &first.proposals);
        assert_eq!(request(&g2), r);
        let second = analyze_with(&g2, &r, &artifact).unwrap();
        assert!(of_type(&second, NodeType::State).is_empty());
        assert!(of_type(&second, NodeType::Invariant).is_empty());
        let transitions = of_type(&second, NodeType::Transition);
        assert_eq!(transitions.len(), 1);
        assert!(second.issues.is_empty() && second.findings.is_empty());
        assert_eq!(
            second.existing,
            vec![
                LifecycleExisting {
                    candidate_ref: id(INVARIANT),
                    node_ref: id(INVARIANT)
                },
                LifecycleExisting {
                    candidate_ref: id(APPROVED),
                    node_ref: id(APPROVED)
                },
                LifecycleExisting {
                    candidate_ref: id(SUBMITTED),
                    node_ref: id(SUBMITTED)
                },
            ]
        );
        let p = transitions[0];
        let (node, edges) = added(p);
        assert_eq!(node.id, id(TRANSITION));
        assert_eq!(
            node.payload,
            NodePayload::Transition(plumb_psg::Transition {
                stateful_ref: id(OWNER),
                from_state: id(SUBMITTED),
                to_state: id(APPROVED),
                guard_expr: None,
                effect_refs: None,
            })
        );
        assert_eq!(node.status, Proposed);
        assert_eq!(node.evidence, evidence_of(&g, &["req:r1"]));
        assert_eq!(
            origin(node, STATE_ORIGIN_EXTENSION),
            json!({
                "kind": "transition", "stateful_ref": OWNER,
                "grounding": r1("moves from submitted to approved"),
                "from_state_grounding": r1("submitted"), "to_state_grounding": r1("approved"),
                "trigger_ref": OPERATION, "trigger_decision_ref": null,
            })
        );
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].id, id(TRANSITIONS_VIA_EDGE));
        assert_eq!(
            (&edges[0].kind, edges[0].from.as_str(), edges[0].to.as_str()),
            (&RelationKind::TransitionsVia, TRANSITION, OPERATION)
        );
        assert_eq!(p.materiality, ProposalMateriality::Semantic);
        assert_eq!(p.acceptance_policy, AcceptancePolicy::HumanConfirm);
        // Applied: exactly one trigger edge, and the graph validates.
        let g3 = apply(&g2, &second.proposals);
        let triggers: Vec<&Edge> = g3
            .outgoing_edge_ids(&id(TRANSITION))
            .iter()
            .map(|e| g3.edge(e).unwrap())
            .filter(|e| e.kind == RelationKind::TransitionsVia)
            .collect();
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].to, id(OPERATION));
        // Replay after application: nothing new.
        let replay = analyze_with(&g3, &r, &artifact).unwrap();
        assert!(
            replay.proposals.is_empty() && replay.findings.is_empty() && replay.issues.is_empty()
        );
        assert_eq!(replay.existing.len(), 4);
    }

    #[test]
    fn s2_state_missing_trigger_finding() {
        let g = with_states();
        let result = analyze(
            &g,
            output(vec![], vec![transition_json(Value::Null)], vec![]),
        )
        .unwrap();
        assert!(result.proposals.is_empty(), "no incomplete Transition node");
        assert_eq!(result.findings.len(), 1);
        let f: &GeneratedFinding = &result.findings[0];
        assert_eq!(f.key.as_str(), TRIGGER_FINDING_KEY);
        assert_eq!(f.id.as_str(), TRIGGER_FINDING_ID);
        assert_eq!(
            f.semantic_condition_key,
            format!("state_transition_trigger_unresolved:{TRANSITION}")
        );
        assert_eq!(f.payload.code, TRANSITION_COMPLETE);
        assert_eq!(f.payload.family, "F2");
        assert_eq!(f.payload.severity, FindingSeverity::Blocker);
        assert_eq!(f.payload.status, "Open");
        assert_eq!(f.payload.affected_refs, ids(&["req:r1"]));
        assert_eq!(
            f.payload.message,
            format!(
                "State transition candidate {TRANSITION} has no resolved Operation/Event trigger."
            )
        );
        assert_eq!(
            f.payload.suggested_resolution.as_deref(),
            Some("Provide or select the Operation/Event that triggers this state transition.")
        );
        // Targets cover the transition and both endpoint groundings.
        let mut spread = transition_json(Value::Null);
        spread["from_state"] = ground(R2, "req:r2", "approved", 0);
        spread["to_state"] = r1("approved");
        let result = analyze(&g, output(vec![], vec![spread], vec![])).unwrap();
        assert_eq!(
            result.findings[0].payload.affected_refs,
            ids(&["req:r1", "req:r2"])
        );
        // While endpoint States are pending there is no finding yet.
        let pending = analyze(
            &base_graph(),
            output(states(), vec![transition_json(Value::Null)], vec![]),
        )
        .unwrap();
        assert!(pending.findings.is_empty());
        // The profile must match only when finding material is built.
        let other = Graph::new(
            g.project_id().clone(),
            id("profile:other"),
            g.nodes().values().cloned().collect(),
            g.edges().values().cloned().collect(),
        )
        .unwrap();
        assert!(matches!(
            analyze(
                &other,
                output(vec![], vec![transition_json(Value::Null)], vec![])
            ),
            Err(LifecycleError::ValidationProfile { .. })
        ));
        analyze(
            &other,
            output(vec![], vec![transition_json(json!(OPERATION))], vec![]),
        )
        .unwrap();
    }

    #[test]
    fn s2_state_self_transition_and_owner_mismatch() {
        let g = with_states();
        let mut selfish = transition_json(json!(OPERATION));
        selfish["grounding"] = r1("submitted to approved");
        selfish["to_state"] = r1("submitted");
        let result = analyze(&g, output(vec![], vec![selfish], vec![])).unwrap();
        let (node, _) = added(of_type(&result, NodeType::Transition)[0]);
        let NodePayload::Transition(t) = &node.payload else {
            panic!()
        };
        assert_eq!(t.from_state, t.to_state);
        apply(&g, &result.proposals);

        // A State at the derived ID whose only owner is another owner.
        let foreign = |state: &str, key: &str, needle: &str| {
            let mut n = node_with_state_origin(state, key, needle);
            n.status = Proposed;
            n
        };
        let g = extend(
            &base_graph(),
            vec![
                entity_node("entity:other", Proposed, "Other"),
                foreign(SUBMITTED, "submitted", "submitted"),
                foreign(APPROVED, "approved", "approved"),
            ],
            vec![
                has_state("rel:foreign-a", "entity:other", SUBMITTED),
                has_state("rel:foreign-b", OWNER, APPROVED),
            ],
        );
        assert!(matches!(
            analyze(&g, output(vec![], vec![transition_json(json!(OPERATION))], vec![])),
            Err(LifecycleError::StateOwnerMismatch { state_ref, .. }) if state_ref == id(SUBMITTED)
        ));
    }

    fn node_with_state_origin(state: &str, key: &str, needle: &str) -> Node {
        let mut n = node(
            state,
            Proposed,
            NodePayload::State(State { name: key.into() }),
        );
        n.extensions.insert(
            STATE_ORIGIN_EXTENSION.parse().unwrap(),
            json!({"kind": "state", "stateful_ref": OWNER, "state_key": key, "grounding": r1(needle)}),
        );
        n
    }

    fn has_state(edge_id: &str, from: &str, to: &str) -> Edge {
        Edge {
            id: id(edge_id),
            revision: 1,
            status: Proposed,
            kind: RelationKind::HasState,
            from: id(from),
            to: id(to),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: AuditMeta::new(id("agent:lifecycle"), ts(AT), None, None).unwrap(),
        }
    }

    // ------------------------------------------------------------------ governed triggers

    const HUMAN: &str = "agent:analyst";
    const QUESTION: &str = "question:trigger";
    const DECISION: &str = "decision:trigger";

    fn marker(candidate: &str, trigger: &str) -> Value {
        json!({"kind": "state_transition_trigger", "candidate_ref": candidate, "trigger_ref": trigger})
    }

    fn decision_node(
        node_id: &str,
        status: ElementStatus,
        answer: Value,
        rationale: Option<&str>,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::ResolutionDecision(ResolutionDecision {
                question_ref: Some(id(QUESTION)),
                proposal_ref: None,
                answer,
                decided_by: id(HUMAN),
                decided_at: ts(AT),
                patch_ref: Hash::content_sha256(b"trigger patch"),
                rationale: rationale.map(str::to_owned),
                supersedes: None,
            }),
        )
    }

    fn resolves(edge_id: &str, from: &str, to: &str, status: ElementStatus) -> Edge {
        Edge {
            id: id(edge_id),
            revision: 1,
            status,
            kind: RelationKind::Resolves,
            from: id(from),
            to: id(to),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: AuditMeta::new(id(HUMAN), ts(AT), None, None).unwrap(),
        }
    }

    /// States applied, an Event added, and the materialized trigger finding with an Agent,
    /// a Question and the given decisions/edges.
    fn governed(decisions: Vec<Node>, edges: Vec<Edge>, agent_kind: AgentKind) -> Graph {
        let g = extend(
            &with_states(),
            vec![event_node(EVENT, Proposed, "ApprovalRecorded")],
            vec![],
        );
        let finding = analyze(
            &g,
            output(vec![], vec![transition_json(Value::Null)], vec![]),
        )
        .unwrap()
        .findings[0]
            .clone();
        let mut nodes = vec![
            node(
                finding.id.as_str(),
                Accepted,
                NodePayload::Finding(finding.payload.clone()),
            ),
            node(HUMAN, Accepted, NodePayload::Agent(Agent { agent_kind })),
            node(
                QUESTION,
                Accepted,
                NodePayload::Question(Question {
                    finding_ref: finding.id.clone(),
                    question_kind: QuestionKind::PickOne,
                    prompt: "Which Operation or Event triggers this transition?".into(),
                    status: "Answered".into(),
                    answer_schema: None,
                    stakeholder_ref: None,
                    priority: None,
                    round_ref: None,
                    context_refs: None,
                }),
            ),
        ];
        nodes.extend(decisions);
        extend(&g, nodes, edges)
    }

    fn decided_transition(result: &LifecycleAnalysisResult) -> (Id, Value) {
        let (node, edges) = added(of_type(result, NodeType::Transition)[0]);
        assert_eq!(edges.len(), 1);
        (edges[0].to.clone(), origin(node, STATE_ORIGIN_EXTENSION))
    }

    #[test]
    fn s2_state_trigger_decision_resolves_candidate() {
        for (via_question, target) in [(true, QUESTION), (false, TRIGGER_FINDING_ID)] {
            let _ = via_question;
            let g = governed(
                vec![decision_node(
                    DECISION,
                    Accepted,
                    marker(TRANSITION, EVENT),
                    Some("Confirmed."),
                )],
                vec![resolves("rel:resolves-1", DECISION, target, Accepted)],
                AgentKind::Human,
            );
            let result = analyze(
                &g,
                output(vec![], vec![transition_json(Value::Null)], vec![]),
            )
            .unwrap();
            assert!(result.findings.is_empty());
            let (trigger, origin) = decided_transition(&result);
            assert_eq!(trigger, id(EVENT));
            assert_eq!(origin["trigger_ref"], json!(EVENT));
            assert_eq!(origin["trigger_decision_ref"], json!(DECISION));
            apply(&g, &result.proposals);
        }
        // Human governance outranks the model trigger; no conflict.
        let g = governed(
            vec![decision_node(
                DECISION,
                Accepted,
                marker(TRANSITION, EVENT),
                Some("Confirmed."),
            )],
            vec![resolves("rel:resolves-1", DECISION, QUESTION, Accepted)],
            AgentKind::Human,
        );
        let result = analyze(
            &g,
            output(vec![], vec![transition_json(json!(OPERATION))], vec![]),
        )
        .unwrap();
        let (trigger, _) = decided_transition(&result);
        assert_eq!(trigger, id(EVENT));
        assert!(result.conflicts.is_empty() && result.findings.is_empty());
    }

    #[test]
    fn s2_state_invalid_trigger_decisions() {
        let run = |decision: Node, agent_kind: AgentKind, target: &str| {
            let status = decision.status;
            let g = governed(
                vec![decision],
                vec![resolves("rel:resolves-1", DECISION, target, status)],
                agent_kind,
            );
            analyze(
                &g,
                output(vec![], vec![transition_json(Value::Null)], vec![]),
            )
        };
        let good = |answer: Value| decision_node(DECISION, Accepted, answer, Some("Confirmed."));
        // Unknown and wrong-type triggers.
        for trigger in ["operation:ghost", OWNER] {
            assert!(matches!(
                run(
                    good(marker(TRANSITION, trigger)),
                    AgentKind::Human,
                    QUESTION
                ),
                Err(LifecycleError::InvalidTriggerRef { .. })
            ));
        }
        // Non-human decider, missing or unclean rationale, extra or missing answer fields.
        assert!(matches!(
            run(
                good(marker(TRANSITION, EVENT)),
                AgentKind::LlmModel,
                QUESTION
            ),
            Err(LifecycleError::InvalidGovernedDecision { .. })
        ));
        for rationale in [None, Some(""), Some("padded ")] {
            assert!(matches!(
                run(
                    decision_node(DECISION, Accepted, marker(TRANSITION, EVENT), rationale),
                    AgentKind::Human,
                    QUESTION
                ),
                Err(LifecycleError::InvalidGovernedDecision { .. })
            ));
        }
        let mut extra = marker(TRANSITION, EVENT);
        extra["note"] = json!("x");
        assert!(matches!(
            run(good(extra), AgentKind::Human, QUESTION),
            Err(LifecycleError::InvalidGovernedDecision { .. })
        ));
        let mut partial = marker(TRANSITION, EVENT);
        partial.as_object_mut().unwrap().remove("trigger_ref");
        assert!(matches!(
            run(good(partial), AgentKind::Human, QUESTION),
            Err(LifecycleError::InvalidGovernedDecision { .. })
        ));
        // A decision resolving a Finding of another rule is ungoverned.
        let g = governed(vec![], vec![], AgentKind::Human);
        let mut wrong = g.node(&id(TRIGGER_FINDING_ID)).unwrap().clone();
        wrong.id = id("fnd:00000000000000ff");
        if let NodePayload::Finding(f) = &mut wrong.payload {
            f.code = "PLUMB.F2.DOMAIN.RELATION_TYPED".into();
        }
        let g = extend(
            &g,
            vec![wrong, good(marker(TRANSITION, EVENT))],
            vec![resolves(
                "rel:resolves-1",
                DECISION,
                "fnd:00000000000000ff",
                Accepted,
            )],
        );
        assert!(matches!(
            analyze(
                &g,
                output(vec![], vec![transition_json(Value::Null)], vec![])
            ),
            Err(LifecycleError::InvalidGovernedDecision { .. })
        ));
        // A non-Accepted decision and decisions for non-current candidates are ignored.
        let proposed = run(
            decision_node(
                DECISION,
                Proposed,
                marker(TRANSITION, EVENT),
                Some("Draft."),
            ),
            AgentKind::Human,
            QUESTION,
        )
        .unwrap();
        assert_eq!(proposed.findings.len(), 1);
        for other in ["transition:0000000000000000", OWNER] {
            let ignored = run(
                good(marker(other, "event:ghost")),
                AgentKind::Human,
                QUESTION,
            )
            .unwrap();
            assert_eq!(ignored.findings.len(), 1);
            assert!(ignored.proposals.is_empty());
        }
    }

    #[test]
    fn s2_state_cross_candidate_and_unrelated_decisions() {
        // A second current candidate (another grounded occurrence) for the cross check.
        let mut second = transition_json(Value::Null);
        second["grounding"] = r1("submitted to approved");
        let g0 = extend(
            &with_states(),
            vec![event_node(EVENT, Proposed, "ApprovalRecorded")],
            vec![],
        );
        let second_id = {
            let res = analyze(&g0, output(vec![], vec![second.clone()], vec![])).unwrap();
            let key = res.findings[0].semantic_condition_key.clone();
            id(key
                .strip_prefix("state_transition_trigger_unresolved:")
                .unwrap())
        };
        let g = governed(
            vec![decision_node(
                DECISION,
                Accepted,
                marker(second_id.as_str(), EVENT),
                Some("Confirmed."),
            )],
            vec![resolves("rel:resolves-1", DECISION, QUESTION, Accepted)],
            AgentKind::Human,
        );
        assert!(matches!(
            analyze(
                &g,
                output(vec![], vec![transition_json(Value::Null), second], vec![])
            ),
            Err(LifecycleError::InvalidGovernedDecision { .. })
        ));
        // An Accepted, malformed decision for a candidate outside the inference is ignored.
        let stale = node(
            "decision:stale",
            Accepted,
            NodePayload::ResolutionDecision(ResolutionDecision {
                question_ref: None,
                proposal_ref: Some(id("prop:00000000000000bb")),
                answer: json!({"kind": "state_transition_trigger", "candidate_ref": "transition:00000000000000aa", "trigger_ref": 7}),
                decided_by: id("agent:nobody"),
                decided_at: ts(AT),
                patch_ref: Hash::content_sha256(b"stale"),
                rationale: None,
                supersedes: None,
            }),
        );
        let g = governed(vec![], vec![], AgentKind::Human);
        let g = extend(
            &g,
            vec![stale],
            vec![resolves(
                "rel:stale",
                "decision:stale",
                TRIGGER_FINDING_ID,
                Accepted,
            )],
        );
        let result = analyze(
            &g,
            output(vec![], vec![transition_json(json!(OPERATION))], vec![]),
        )
        .unwrap();
        assert_eq!(of_type(&result, NodeType::Transition).len(), 1);
    }

    #[test]
    fn s2_state_ambiguous_trigger_decisions() {
        let g = governed(
            vec![
                decision_node(
                    DECISION,
                    Accepted,
                    marker(TRANSITION, EVENT),
                    Some("First."),
                ),
                decision_node(
                    "decision:trigger-2",
                    Accepted,
                    marker(TRANSITION, OPERATION),
                    Some("Second."),
                ),
            ],
            vec![
                resolves("rel:resolves-1", DECISION, QUESTION, Accepted),
                resolves("rel:resolves-2", "decision:trigger-2", QUESTION, Accepted),
            ],
            AgentKind::Human,
        );
        match analyze(
            &g,
            output(vec![], vec![transition_json(Value::Null)], vec![]),
        ) {
            Err(LifecycleError::AmbiguousTriggerDecision {
                candidate_ref,
                decision_refs,
            }) => {
                assert_eq!(candidate_ref, id(TRANSITION));
                assert_eq!(decision_refs, ids(&[DECISION, "decision:trigger-2"]));
            }
            other => panic!("{other:?}"),
        }
    }

    // ------------------------------------------------------------------ invariants

    #[test]
    fn s2_state_invariant_proposal() {
        let g = base_graph();
        // Invariant proposal creation is tested; PlumbExpr parse/typecheck is NOT run in S2.2
        // because those capabilities belong to S2.4/S2.5.
        for expression in [EXPRESSION, "(( >= approved"] {
            let result =
                analyze(&g, output(vec![], vec![], vec![invariant_json(expression)])).unwrap();
            assert_eq!(result.proposals.len(), 1);
            let p = &result.proposals[0];
            assert!(matches!(p.patch_set.patch, SemanticPatch::AddNode { .. }));
            let (node, edges) = added(p);
            assert!(edges.is_empty());
            assert_eq!(node.id, id(INVARIANT));
            assert_eq!(
                node.payload,
                NodePayload::Invariant(plumb_psg::Invariant {
                    scope_ref: id(OWNER),
                    expression: expression.into(),
                })
            );
            assert_eq!(node.status, Proposed);
            assert_eq!(node.evidence, evidence_of(&g, &["req:r2"]));
            let origin = origin(node, INVARIANT_ORIGIN_EXTENSION);
            assert_eq!(
                origin,
                json!({"scope_ref": OWNER, "grounding": {"requirement_ref": "req:r2", "start": 0, "end": R2.len()}})
            );
            let decoded: InvariantOrigin = serde_json::from_value(origin).unwrap();
            assert_eq!(decoded.scope_ref, id(OWNER));
            assert_eq!(p.materiality, ProposalMateriality::Semantic);
            assert_eq!(p.acceptance_policy, AcceptancePolicy::HumanConfirm);
            assert_eq!(p.stage, StageId::S2);
            assert_eq!(p.confidence, None);
            let applied = apply(&g, &result.proposals);
            assert_eq!(applied.node(&id(INVARIANT)).unwrap().status, Proposed);
            // Idempotent replay.
            assert!(analyze(
                &applied,
                output(vec![], vec![], vec![invariant_json(expression)])
            )
            .unwrap()
            .proposals
            .is_empty());
        }
        // The identity excludes the expression: a different expression for the same grounded
        // invariant conflicts with the existing node instead of being duplicated.
        let applied = apply(
            &g,
            &analyze(&g, output(vec![], vec![], vec![invariant_json(EXPRESSION)]))
                .unwrap()
                .proposals,
        );
        assert!(matches!(
            analyze(&applied, output(vec![], vec![], vec![invariant_json("approved_amount < requested_amount")])),
            Err(LifecycleError::ExistingCandidateConflict { node_ref }) if node_ref == id(INVARIANT)
        ));
    }

    // ------------------------------------------------------------------ reconciliation

    #[test]
    fn s2_state_existing_candidate_conflicts() {
        let g = base_graph();
        // A different payload at the deterministic State ID.
        let occupied = extend(
            &g,
            vec![node(
                APPROVED,
                Proposed,
                NodePayload::State(State {
                    name: "accepted".into(),
                }),
            )],
            vec![],
        );
        assert!(matches!(
            analyze(&occupied, output(states(), vec![], vec![])),
            Err(LifecycleError::ExistingCandidateConflict { node_ref }) if node_ref == id(APPROVED)
        ));
        // Another node type at the deterministic Transition ID.
        let occupied = extend(
            &with_states(),
            vec![entity_node(TRANSITION, Proposed, "Occupier")],
            vec![],
        );
        assert!(matches!(
            analyze(&occupied, output(vec![], vec![transition_json(json!(OPERATION))], vec![])),
            Err(LifecycleError::ExistingCandidateConflict { node_ref }) if node_ref == id(TRANSITION)
        ));
        // A State whose has_state owner is missing is not reused.
        let ownerless = extend(
            &g,
            vec![node_with_state_origin(APPROVED, "approved", "approved")],
            vec![],
        );
        let result = analyze(&ownerless, output(states(), vec![], vec![])).unwrap();
        assert!(result
            .conflicts
            .contains(&LifecycleConflict::ExistingStateOriginConflict {
                candidate_ref: id(APPROVED),
                node_refs: ids(&[APPROVED]),
            }));
        assert_eq!(of_type(&result, NodeType::State).len(), 1);
    }

    #[test]
    fn s2_state_duplicate_state_origin() {
        let g = extend(
            &base_graph(),
            vec![
                node_with_state_origin("state:dup-a", "approved", "approved"),
                node_with_state_origin("state:dup-b", "approved", "approved"),
                node_with_state_origin(SUBMITTED, "submitted", "submitted"),
            ],
            vec![
                has_state("rel:dup-a", OWNER, "state:dup-a"),
                has_state("rel:dup-b", OWNER, "state:dup-b"),
                has_state("rel:sub", OWNER, SUBMITTED),
            ],
        );
        let result = analyze(&g, full_output(json!(OPERATION))).unwrap();
        assert!(result
            .conflicts
            .contains(&LifecycleConflict::DuplicateStateOrigin {
                candidate_ref: id(APPROVED),
                node_refs: ids(&["state:dup-a", "state:dup-b"]),
            }));
        assert!(of_type(&result, NodeType::State).is_empty());
        assert!(of_type(&result, NodeType::Transition).is_empty());
        assert!(result.findings.is_empty());
        assert!(result
            .issues
            .contains(&LifecycleIssue::StateDependencyConflicted {
                candidate_ref: id(TRANSITION),
                state_refs: ids(&[APPROVED]),
            }));
        for p in &result.proposals {
            assert!(!matches!(
                p.patch_set.patch,
                SemanticPatch::MergeNodes { .. }
            ));
            let wire = serde_json::to_string(&p.patch_set).unwrap();
            assert!(!wire.contains("MergeNodes") && !wire.contains("RemoveEdge"));
        }
    }

    // ------------------------------------------------------------------ determinism

    #[test]
    fn s2_state_input_order_and_audit_independence() {
        let g = with_states();
        let reversed = Graph::new(
            g.project_id().clone(),
            g.profile_id().clone(),
            g.nodes().values().rev().cloned().collect(),
            g.edges().values().rev().cloned().collect(),
        )
        .unwrap();
        let mut second = transition_json(Value::Null);
        second["grounding"] = r1("submitted to approved");
        let forward = output(
            states(),
            vec![transition_json(json!(OPERATION)), second.clone()],
            vec![invariant_json(EXPRESSION)],
        );
        let backward = output(
            states().into_iter().rev().collect(),
            vec![second, transition_json(json!(OPERATION))],
            vec![invariant_json(EXPRESSION)],
        );
        let a = analyze(&g, forward.clone()).unwrap();
        let b = analyze(&reversed, backward).unwrap();
        assert_eq!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&b).unwrap()
        );
        // The audit timestamp changes proposal identity only.
        let r = request(&g);
        let artifact = artifact_for(&r.request, forward);
        let run = |at: &str| {
            analyze_lifecycle(
                &g,
                &r,
                Some(LifecycleInference {
                    artifact: &artifact,
                    derivation_ref: derivation(),
                }),
                &audit_at(at),
            )
            .unwrap()
        };
        let x = run(AT);
        let y = run("2031-06-01T12:00:00.000000000Z");
        let node_ids = |res: &LifecycleAnalysisResult| -> BTreeSet<Id> {
            res.proposals
                .iter()
                .map(|p| added(p).0.id.clone())
                .collect()
        };
        assert_eq!(node_ids(&x), node_ids(&y));
        assert_eq!(x.findings, y.findings);
        assert_ne!(x.proposals[0].id, y.proposals[0].id);
    }

    // ------------------------------------------------------------------ real HR diagnostic

    /// The real single-source HR graph through S0.1, S0.4, S1.1 (mock classifier; all
    /// Requirements Accepted in test code) and S2.1 with no Accepted vocabulary.
    fn hr_graph() -> Graph {
        let imported = import_markdown(
            "requirements.md",
            HR_MD,
            &ImportAudit {
                created_by: id("actor:importer"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        let fragments = imported.fragments.clone();
        let mut nodes = vec![imported.source];
        nodes.extend(imported.fragments);
        let mut g = new_graph(nodes, vec![]);
        let segmentation = build_segmentation_request(&fragments, provider("mock")).unwrap();
        let candidates = evaluate_segmentation(
            &segmentation,
            &fragments,
            None,
            &SegmentationAudit {
                created_by: id("agent:segmenter"),
                created_at: ts(AT),
            },
        )
        .unwrap()
        .candidates;
        let classification =
            build_requirement_classification_request(&g, &candidates, provider("mock")).unwrap();
        let entries: Vec<Value> = classification
            .context
            .candidates
            .iter()
            .map(|c| json!({"candidate_ref": c.candidate_ref, "requirement_kind": null, "level": "software"}))
            .collect();
        let artifact = artifact_for(
            &classification.request,
            json!({"version": 1, "classifications": entries, "intents": []}),
        );
        let compiled = compile_requirement_candidates(
            &g,
            &candidates,
            &classification,
            Some(RequirementClassificationInference {
                artifact: &artifact,
                derivation_ref: DerivationRef::from(id("drv:00000000000000c1")),
            }),
            &RequirementCompilationAudit {
                created_by: id("agent:requirements"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        for p in &compiled.proposals {
            g = apply_patch(&g, &p.patch_set).unwrap().graph;
        }
        let g = map_nodes(&g, |n| {
            if matches!(n.payload, NodePayload::Requirement(_)) {
                n.status = Accepted;
            }
        });
        // S2.1 with the current truthful vocabulary state: no Accepted Concept, no Entity.
        let domain = build_domain_request(&g, provider("mock")).unwrap();
        assert!(domain.context.concepts.is_empty());
        let domain_artifact = artifact_for(
            &domain.request,
            json!({"version": 1, "entities": [], "attributes": [], "relationships": []}),
        );
        let entities = analyze_domain(
            &g,
            &domain,
            Some(DomainInference {
                artifact: &domain_artifact,
                derivation_ref: DerivationRef::from(id("drv:00000000000000c3")),
            }),
            &DomainAudit {
                created_by: id("agent:domain"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        assert!(entities.proposals.is_empty());
        g
    }

    #[test]
    fn s2_state_real_hr_diagnostic() {
        let g = hr_graph();
        let owners = g
            .nodes()
            .values()
            .filter(|n| {
                matches!(n.status, Proposed | Accepted)
                    && plumb_psg::NodeCategory::StateOwner.contains(n.payload.node_type())
            })
            .count();
        // Stop and inspect rather than force this if upstream ever produces an owner.
        assert_eq!(owners, 0);
        let r = request(&g);
        assert_eq!(r.context.requirements.len(), 32);
        assert!(r.context.owners.is_empty());
        let result = analyze_with(&g, &r, &artifact_for(&r.request, empty_output())).unwrap();
        assert_eq!(result.issues, vec![LifecycleIssue::StateOwnerUnavailable]);
        assert!(of_type(&result, NodeType::State).is_empty());
        assert!(of_type(&result, NodeType::Transition).is_empty());
        assert!(of_type(&result, NodeType::Invariant).is_empty());
        assert!(result.findings.is_empty());
        // No state-machine accuracy, survival, precision or recall is computed or claimed.
    }

    // ------------------------------------------------------------------ guards

    #[test]
    fn s2_state_production_source_guard() {
        let state = include_str!("../src/state.rs");
        let invariant = include_str!("../src/invariant.rs");
        for (name, source) in [("state.rs", state), ("invariant.rs", invariant)] {
            for forbidden in [
                "std::fs",
                "File::open",
                "reqwest",
                ".execute(",
                "MockProvider",
                "ArtifactStore",
                "RevisionStore",
                "rusqlite",
                "SystemClock",
                "Clock::now",
                "Utc::now",
                "Instant::now",
                "commit(",
                "Commit",
                "unsafe",
                "persist_inference_bundle",
                "plumb_expr",
                "parse_expr",
                "typecheck(",
                "Regex",
                "is_uppercase",
                "is_ascii_uppercase",
                "to_uppercase",
                "NodePayload::Attribute",
                "NodePayload::Operation(Operation",
                "NodePayload::Question(Question",
                "Question {",
                "Operation {",
                "Event {",
                "Process {",
                "initial_state",
                "is_initial",
                "terminal_state",
                "is_terminal",
                "externally_entered",
                "reachable(",
                "fn reachab",
                "VecDeque",
                "MergeNodes",
                "trigger_ref: Option<Id>,\n    pub",
                "G_STATE",
                "I_STATE",
                "G_TRANSITION",
                "request moves",
                "submitted",
                "approved",
            ] {
                assert!(!source.contains(forbidden), "{name} contains {forbidden}");
            }
        }
        // The F0.14 root guard and the module wiring.
        let lib = include_str!("../src/lib.rs");
        assert!(!lib.contains("Proposal"));
        assert!(lib.contains("pub mod state;") && lib.contains("pub mod invariant;"));
        assert!(state.contains("apply_patch"));
        // No local PlumbExpr clone is defined.
        for source in [state, invariant] {
            for clone in [
                "enum Expr",
                "struct Expr",
                "enum Ty",
                "struct Ast",
                "enum Ast",
                "PlumbExpr {",
            ] {
                assert!(!source.contains(clone), "{clone}");
            }
        }
    }
}
