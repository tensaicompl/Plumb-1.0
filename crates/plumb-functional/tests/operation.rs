//! S2.7 contract tests for AI-assisted Operations: the closed operation scope, the inference
//! request and artifact contract, typed relations, Outcomes, Events and their payload
//! references, query writes, performers, the S2.6 calculation-read completeness check,
//! reconciliation, deterministic proposals and the HR operation_specs reference.
//!
//! Golden values were computed independently with Python `hashlib` over RFC 8785 JSON, never
//! with the helpers under test. The generic order fixture and the HR reference graph are
//! synthetic Accepted semantic graphs; they do not claim the real HR graph contains these
//! nodes, and the HR test checks compiler conformance given a mock artifact, not extraction
//! accuracy.

mod operation_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_functional::operation::*;
    use plumb_functional::{EventOrigin, EVENT_ORIGIN_EXTENSION};
    use plumb_inference::{InferenceArtifact, InferenceRequest, ProviderPolicy};
    use plumb_patch::{
        apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        AcceptanceCriterion, Actor, ActorKind, Attribute, AuditMeta, BusinessRole, Calculation,
        DataSchema, DecisionTable, DerivationRef, Edge, ElementStatus, Entity, Event, Graph,
        Invariant, Modality, Node, NodePayload, NodeType, Operation, OperationKind, Outcome,
        OutcomeKind, RelationKind, RelationProperties, Requirement, RequirementKind,
        RequirementLevel, Rule, RuleKind, SecurityRole,
    };
    use serde_json::{json, Value as Json};

    use ElementStatus::{Accepted, Proposed};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s2-operation.md");
    const SCHEMA: &str = include_str!("../../../schemas/inference/s2-operation.schema.json");
    const HR_CONTRACT: &str = include_str!("../../../fixtures/hr-leave/fixture-contract.yaml");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:4d4cb78949fd76ac39cc1ee1a41262caab478bea2ceb31f4231b8135b961f829";
    const SCHEMA_HASH: &str =
        "sha256:29ca4a2de45850a2c97d9b4119b97f9074c76619865cfcc1ccdd385a2edef6d0";
    const GOLDEN_CONTEXT_HASH: &str =
        "sha256:68a5a788bc509400759ea64d7301ac0111a726432daae9bc0a9b322c6bd75728";
    const GOLDEN_REQUEST_ID: &str =
        "sha256:3ba5a629e873454b413cad031c32da9a76695e48f30bd98be2ae41e5b3400082";
    const GOLDEN_OPERATION_ID: &str = "operation:8e5694fe9fe51240";
    const GOLDEN_OUTCOME_ID: &str = "outcome:512acf4c13a0e637";
    const GOLDEN_EVENT_ID: &str = "event:71f955142012d071";
    const GOLDEN_PERFORMED_BY_ID: &str = "rel:094407b5ce7f531b";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    // Synthetic generic test data only; never in production source.
    const R1: &str = "The clerk shall approve a submitted order by reading the order and its customer, writing the order status and recording that the order was approved.";
    const CLERK: &str = "role:clerk";
    const ORDER: &str = "entity:order";
    const CUSTOMER: &str = "entity:customer";
    const STATUS: &str = "attr:order-status";
    const AMOUNT: &str = "attr:amount";
    const VIP: &str = "attr:vip";
    const ORDER_SCHEMA: &str = "schema:order";

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

    fn audit_meta() -> AuditMeta {
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
            audit: audit_meta(),
        }
    }

    fn edge(kind: RelationKind, from: &str, to: &str, status: ElementStatus) -> Edge {
        Edge {
            id: id(&format!(
                "rel:fx-{}-{}",
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
            audit: audit_meta(),
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

    fn entity(node_id: &str, name: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Entity(Entity {
                name: name.into(),
                description: None,
                aggregate_root: None,
            }),
        )
    }

    fn attribute(node_id: &str, name: &str, value_type: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Attribute(Attribute {
                name: name.into(),
                value_type: value_type.into(),
                nullable: false,
                unit: None,
                precision: None,
                enum_values: None,
                data_classification: None,
            }),
        )
    }

    fn data_schema(node_id: &str, status: ElementStatus, name: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::DataSchema(DataSchema {
                name: name.into(),
                schema_kind: "json_schema".into(),
                external_ref: None,
                inline_schema: None,
            }),
        )
    }

    fn event_node(
        node_id: &str,
        name: &str,
        payload: Option<&str>,
        semantic_type: Option<&str>,
    ) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Event(Event {
                name: name.into(),
                payload_schema_ref: payload.map(id),
                semantic_type: semantic_type.map(Into::into),
            }),
        )
    }

    /// An Accepted Calculation carrying a hand-written S2.6 origin with the given used bindings
    /// (`(node_ref, symbol)` Root bindings or `(node_ref, owner, symbol)` Field bindings).
    fn calculation(
        node_id: &str,
        expression: &str,
        result_type: &str,
        roots: &[(&str, &str)],
        fields: &[(&str, &str, &str)],
    ) -> Node {
        let mut n = node(
            node_id,
            Accepted,
            NodePayload::Calculation(Calculation {
                name: node_id.to_owned(),
                expression: expression.into(),
                result_type: result_type.into(),
                unit: None,
                rounding: None,
                calendar_ref: None,
                examples: None,
            }),
        );
        let mut bindings: Vec<Json> = roots
            .iter()
            .map(|(r, s)| json!({"node_ref": r, "symbol": s, "exposure": {"kind": "root"}}))
            .collect();
        bindings.extend(fields.iter().map(|(r, o, s)| json!({"node_ref": r, "symbol": s, "exposure": {"kind": "field", "owner_ref": o}})));
        let range = json!({"requirement_ref": "req:r1", "start": 0, "end": 3});
        n.extensions.insert(
            "plumb_functional:calculation_origin".parse().unwrap(),
            json!({"name_range": range, "expression_evidence": [range], "used_bindings": bindings}),
        );
        n
    }

    fn nodes() -> Vec<Node> {
        let mut legacy = calculation("calculation:legacy", "1", "Int", &[], &[]);
        legacy.extensions.clear();
        vec![
            requirement("req:r1", R1),
            node(
                CLERK,
                Accepted,
                NodePayload::BusinessRole(BusinessRole {
                    name: "Clerk".into(),
                }),
            ),
            node(
                "actor:system",
                Accepted,
                NodePayload::Actor(Actor {
                    name: "Scheduler".into(),
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
            entity(ORDER, "Order"),
            entity(CUSTOMER, "Customer"),
            attribute(STATUS, "status", "String"),
            attribute(AMOUNT, "amount", "Decimal(2)"),
            attribute(VIP, "vip", "Bool"),
            data_schema(ORDER_SCHEMA, Accepted, "OrderSchema"),
            data_schema("schema:draft", Proposed, "DraftSchema"),
            node(
                "rule:approval",
                Accepted,
                NodePayload::Rule(Rule {
                    name: "Approval".into(),
                    rule_kind: RuleKind::Business,
                    condition_expr: None,
                    action_expr: None,
                }),
            ),
            node(
                "dt:discount",
                Accepted,
                NodePayload::DecisionTable(DecisionTable {
                    name: "Discount".into(),
                    hit_policy: "UNIQUE".into(),
                    inputs: vec![],
                    outputs: vec![],
                    rows: vec![],
                }),
            ),
            node(
                "inv:positive",
                Accepted,
                NodePayload::Invariant(Invariant {
                    scope_ref: id(ORDER),
                    expression: "amount >= 0".into(),
                }),
            ),
            node(
                "ac:approve",
                Accepted,
                NodePayload::AcceptanceCriterion(AcceptanceCriterion {
                    statement: "An approved order is recorded.".into(),
                    criterion_kind: "functional".into(),
                    verification_method: None,
                    measure_ref: None,
                    scenario_ref: None,
                }),
            ),
            event_node("event:shipped", "Order Shipped", None, Some("domain_event")),
            calculation(
                "calculation:total",
                "amount * 2",
                "Decimal(2)",
                &[(AMOUNT, "amount")],
                &[],
            ),
            calculation(
                "calculation:vip",
                "Customer.vip",
                "Bool",
                &[(CUSTOMER, "Customer")],
                &[(VIP, CUSTOMER, "vip")],
            ),
            calculation(
                "calculation:double",
                "total + 1",
                "Decimal(2)",
                &[("calculation:total", "total")],
                &[],
            ),
            calculation("calculation:broken", "missing + 1", "Int", &[], &[]),
            calculation(
                "calculation:uses-broken",
                "broken + 1",
                "Int",
                &[("calculation:broken", "broken")],
                &[],
            ),
            calculation(
                "calculation:cyc-a",
                "cb + 1",
                "Int",
                &[("calculation:cyc-b", "cb")],
                &[],
            ),
            calculation(
                "calculation:cyc-b",
                "ca + 1",
                "Int",
                &[("calculation:cyc-a", "ca")],
                &[],
            ),
            legacy,
        ]
    }

    fn edges() -> Vec<Edge> {
        vec![
            edge(RelationKind::HasAttribute, ORDER, STATUS, Accepted),
            edge(RelationKind::HasAttribute, ORDER, AMOUNT, Accepted),
            edge(RelationKind::HasAttribute, CUSTOMER, VIP, Accepted),
        ]
    }

    fn graph_of(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn base_graph() -> Graph {
        graph_of(nodes(), edges())
    }

    fn with_nodes(extra: Vec<Node>) -> Graph {
        let mut n = nodes();
        n.extend(extra);
        graph_of(n, edges())
    }

    fn golden_scope() -> OperationScope {
        OperationScope {
            performer_refs: ids(&[CLERK]),
            domain_refs: ids(&[ORDER, CUSTOMER, STATUS]),
            data_schema_refs: ids(&[ORDER_SCHEMA]),
            event_payload_attribute_refs: ids(&[STATUS]),
            ..OperationScope::default()
        }
    }

    fn full_scope() -> OperationScope {
        OperationScope {
            performer_refs: ids(&["actor:system", CLERK]),
            domain_refs: ids(&[ORDER, CUSTOMER, STATUS, AMOUNT, VIP]),
            calculation_refs: ids(&[
                "calculation:total",
                "calculation:vip",
                "calculation:double",
                "calculation:broken",
                "calculation:uses-broken",
                "calculation:cyc-a",
                "calculation:cyc-b",
            ]),
            governor_refs: ids(&["rule:approval", "dt:discount"]),
            event_refs: ids(&["event:shipped"]),
            data_schema_refs: ids(&[ORDER_SCHEMA]),
            event_payload_attribute_refs: ids(&[STATUS]),
            support_refs: ids(&["ac:approve", "inv:positive", "rule:approval"]),
        }
    }

    fn provider() -> ProviderPolicy {
        ProviderPolicy {
            provider: "mock".into(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn derivation() -> DerivationRef {
        DerivationRef::from(id("drv:00000000000000c7"))
    }

    fn audit() -> OperationAudit {
        OperationAudit {
            created_by: id("agent:operation"),
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
        let start = R1.find(needle).unwrap_or_else(|| panic!("{needle}"));
        json!({"requirement_ref": "req:r1", "start": start, "end": start + needle.len()})
    }

    fn gref(target: &str, needle: &str) -> Json {
        json!({"target_ref": target, "evidence": [range(needle)]})
    }

    /// A minimal valid command candidate performed by the clerk.
    fn candidate(name: &str, kind: &str) -> Json {
        json!({
            "name": name,
            "operation_kind": kind,
            "evidence": [range("approve a submitted order")],
            "performers": [gref(CLERK, "clerk")],
            "reads": [],
            "writes": [],
            "governed_by": [],
            "uses_calculation": [],
            "input_schema": null,
            "output_schema": null,
            "outcomes": [],
            "new_produced_events": [],
            "produced_event_refs": [],
            "consumed_event_refs": [],
        })
    }

    /// The golden candidate: reads the order and customer, writes the status, one Outcome and
    /// one new Event with an Attribute payload.
    fn golden_candidate() -> Json {
        let mut c = candidate("Approve Order", "command");
        c["reads"] = json!([
            gref(ORDER, "reading the order"),
            gref(CUSTOMER, "its customer")
        ]);
        c["writes"] = json!([gref(STATUS, "writing the order status")]);
        c["outcomes"] = json!([{"name": "Approved", "outcome_kind": "success", "evidence": [range("the order was approved")]}]);
        c["new_produced_events"] = json!([{"name": "Order Approved", "payload_schema_ref": STATUS, "evidence": [range("recording that the order was approved")]}]);
        c
    }

    fn output(candidates: Vec<Json>) -> Json {
        json!({"version": 1, "operations": candidates})
    }

    fn request(graph: &Graph, scope: &OperationScope) -> OperationRequest {
        build_operation_request(graph, &[id("req:r1")], scope, provider()).unwrap()
    }

    fn run_with(
        graph: &Graph,
        scope: &OperationScope,
        out: Json,
    ) -> Result<OperationAnalysisResult, OperationError> {
        let r = request(graph, scope);
        let artifact = artifact_for(&r.request, out);
        analyze_operations(
            graph,
            &r,
            Some(OperationInference {
                artifact: &artifact,
                derivation_ref: derivation(),
            }),
            &audit(),
        )
    }

    fn run(
        graph: &Graph,
        scope: &OperationScope,
        candidates: Vec<Json>,
    ) -> Result<OperationAnalysisResult, OperationError> {
        run_with(graph, scope, output(candidates))
    }

    fn one(
        graph: &Graph,
        scope: &OperationScope,
        c: Json,
    ) -> (OperationCandidateAnalysis, Vec<Proposal>) {
        let result = run(graph, scope, vec![c]).unwrap();
        assert_eq!(result.candidates.len(), 1);
        (result.candidates[0].clone(), result.proposals)
    }

    fn issues(graph: &Graph, c: Json) -> Vec<OperationIssue> {
        one(graph, &full_scope(), c).0.issues
    }

    fn patches(p: &Proposal) -> &[SemanticPatch] {
        let SemanticPatch::Compound { patches } = &p.patch_set.patch else {
            panic!("not compound")
        };
        patches
    }

    fn added_nodes(p: &Proposal) -> Vec<&Node> {
        patches(p)
            .iter()
            .filter_map(|s| {
                if let SemanticPatch::AddNode { node } = s {
                    Some(node)
                } else {
                    None
                }
            })
            .collect()
    }

    fn added_edges(p: &Proposal) -> Vec<&Edge> {
        patches(p)
            .iter()
            .filter_map(|s| {
                if let SemanticPatch::AddEdge { edge } = s {
                    Some(edge)
                } else {
                    None
                }
            })
            .collect()
    }

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

    fn accept_all(graph: &Graph) -> Graph {
        let nodes = graph
            .nodes()
            .values()
            .cloned()
            .map(|mut n| {
                n.status = Accepted;
                n
            })
            .collect::<Vec<_>>();
        let nodes = nodes
            .into_iter()
            .map(|mut n| {
                if n.id.as_str() == "schema:draft" {
                    n.status = Proposed;
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

    fn rel(kind: &str, from: &str, to: &str) -> (String, String, String) {
        (kind.to_owned(), from.to_owned(), to.to_owned())
    }

    fn edge_set(p: &Proposal) -> BTreeSet<(String, String, String)> {
        added_edges(p)
            .iter()
            .map(|e| {
                (
                    serde_json::to_value(&e.kind)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    e.from.to_string(),
                    e.to.to_string(),
                )
            })
            .collect()
    }

    // ------------------------------------------------------------------ goldens and proposals

    #[test]
    fn operation_request_goldens() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
        assert_eq!(OPERATION_CONTEXT_VERSION, 1);
        assert_eq!(OPERATION_OUTPUT_VERSION, 1);
        assert_eq!(OPERATION_TASK_KIND, "operation_analysis");
        assert_eq!(
            OPERATION_ORIGIN_EXTENSION,
            "plumb_functional:operation_origin"
        );
        assert_eq!(OUTCOME_ORIGIN_EXTENSION, "plumb_functional:outcome_origin");
        assert_eq!(EVENT_ORIGIN_EXTENSION, "plumb_functional:event_origin");
        let g = base_graph();
        let r = request(&g, &golden_scope());
        assert_eq!(r.request.context_hash.as_str(), GOLDEN_CONTEXT_HASH);
        assert_eq!(r.request.id.as_str(), GOLDEN_REQUEST_ID);
        assert_eq!(r.request.stage, StageId::S2);
        assert_eq!(r.request.task_kind, "operation_analysis");
        assert_eq!(
            r.request.input_refs,
            ids(&[STATUS, CUSTOMER, ORDER, "req:r1", CLERK, ORDER_SCHEMA])
        );
        assert!(r.request.evidence_refs.is_empty());
        // Canonical sorted scope lists.
        assert_eq!(r.scope.domain_refs, ids(&[STATUS, CUSTOMER, ORDER]));
        // Calculation context is a summary only.
        let full = request(&g, &full_scope());
        let calc = serde_json::to_value(&full.context.calculations[0]).unwrap();
        assert_eq!(
            calc,
            json!({"calculation_ref": "calculation:broken", "name": "calculation:broken", "result_type": "Int", "unit": null, "qualified": false, "dependencies": []})
        );
        let double = full
            .context
            .calculations
            .iter()
            .find(|c| c.calculation_ref.as_str() == "calculation:double")
            .unwrap();
        assert!(double.qualified);
        assert_eq!(double.dependencies, ids(&["calculation:total"]));
        assert!(build_operation_request(&g, &[], &golden_scope(), provider()).is_err());
    }

    #[test]
    fn operation_golden_proposal() {
        let g = base_graph();
        let result = run(&g, &golden_scope(), vec![golden_candidate()]).unwrap();
        assert!(result.issues.is_empty());
        let c = &result.candidates[0];
        assert!(c.issues.is_empty(), "{:?}", c.issues);
        assert_eq!(c.operation_ref.as_str(), GOLDEN_OPERATION_ID);
        assert_eq!(c.outcomes[0].outcome_ref.as_str(), GOLDEN_OUTCOME_ID);
        assert_eq!(c.new_events[0].event_ref.as_str(), GOLDEN_EVENT_ID);
        assert_eq!(c.specified_by, ids(&["req:r1"]));
        assert_eq!(result.proposals.len(), 1);
        let p = &result.proposals[0];
        assert!(
            matches!(&c.disposition, OperationDisposition::Proposed { proposal_ref } if *proposal_ref == p.id)
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
        // Operation first, then the children by ID; every node Proposed revision 1.
        let nodes = added_nodes(p);
        let node_ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(
            node_ids,
            [GOLDEN_OPERATION_ID, GOLDEN_EVENT_ID, GOLDEN_OUTCOME_ID]
        );
        assert!(nodes
            .iter()
            .all(|n| n.status == Proposed && n.revision == 1 && n.derivations.is_empty()));
        assert_eq!(
            nodes[0].payload,
            NodePayload::Operation(Operation {
                name: "Approve Order".into(),
                operation_kind: OperationKind::Command,
                input_schema_ref: None,
                output_schema_ref: None,
                preconditions: None,
                postconditions: None,
                idempotency: None,
                transaction_semantics: None,
            })
        );
        assert_eq!(
            nodes[1].payload,
            NodePayload::Event(Event {
                name: "Order Approved".into(),
                payload_schema_ref: Some(id(STATUS)),
                semantic_type: None
            })
        );
        assert_eq!(
            nodes[2].payload,
            NodePayload::Outcome(Outcome {
                name: "Approved".into(),
                outcome_kind: OutcomeKind::Success
            })
        );
        let origin = |n: &Node, key: &str| {
            n.extensions
                .iter()
                .find(|(k, _)| k.as_str() == key)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        let op_origin: OperationOrigin =
            serde_json::from_value(origin(nodes[0], OPERATION_ORIGIN_EXTENSION)).unwrap();
        assert_eq!(
            serde_json::to_value(&op_origin.evidence).unwrap(),
            json!([range("approve a submitted order")])
        );
        let outcome_origin: OutcomeOrigin =
            serde_json::from_value(origin(nodes[2], OUTCOME_ORIGIN_EXTENSION)).unwrap();
        assert_eq!(outcome_origin.operation_ref.as_str(), GOLDEN_OPERATION_ID);
        let event_origin: EventOrigin =
            serde_json::from_value(origin(nodes[1], EVENT_ORIGIN_EXTENSION)).unwrap();
        assert_eq!(event_origin.evidence.len(), 1);
        // Edges: canonical typed relations plus the specified_by trace, sorted by ID.
        let op = GOLDEN_OPERATION_ID;
        assert_eq!(
            edge_set(p),
            BTreeSet::from([
                rel("performed_by", op, CLERK),
                rel("reads", op, ORDER),
                rel("reads", op, CUSTOMER),
                rel("writes", op, STATUS),
                rel("produces", op, GOLDEN_OUTCOME_ID),
                rel("produces", op, GOLDEN_EVENT_ID),
                rel("specified_by", "req:r1", op),
            ])
        );
        let edges = added_edges(p);
        assert!(edges.windows(2).all(|w| w[0].id < w[1].id));
        assert!(edges
            .iter()
            .all(|e| e.status == Proposed && e.revision == 1 && e.derivations.is_empty()));
        let performed = edges
            .iter()
            .find(|e| e.kind == RelationKind::PerformedBy)
            .unwrap();
        assert_eq!(performed.id.as_str(), GOLDEN_PERFORMED_BY_ID);
        // Dry-run: the registry accepts every endpoint type.
        let applied = apply(&g, &result.proposals);
        assert_eq!(
            applied.node(&id(op)).unwrap().payload.node_type(),
            NodeType::Operation
        );
    }

    // ------------------------------------------------------------------ scope

    #[test]
    fn operation_scope_validation() {
        let g = base_graph();
        let err = |scope: OperationScope| match build_operation_request(
            &g,
            &[id("req:r1")],
            &scope,
            provider(),
        ) {
            Err(OperationError::Scope { field, .. }) => field,
            other => panic!("{other:?}"),
        };
        let with = |f: fn(&mut OperationScope)| {
            let mut s = golden_scope();
            f(&mut s);
            s
        };
        assert_eq!(
            err(with(|s| s.performer_refs.push(id(CLERK)))),
            "performer_refs"
        );
        assert_eq!(
            err(with(|s| s.performer_refs.push(id("secrole:clerk")))),
            "performer_refs"
        );
        assert_eq!(
            err(with(|s| s.performer_refs.push(id(ORDER)))),
            "performer_refs"
        );
        assert_eq!(err(with(|s| s.domain_refs.push(id(CLERK)))), "domain_refs");
        assert_eq!(
            err(with(|s| s.domain_refs.push(id("attr:ghost")))),
            "domain_refs"
        );
        assert_eq!(
            err(with(|s| s.calculation_refs.push(id("calculation:legacy")))),
            "calculation_refs"
        );
        assert_eq!(
            err(with(|s| s.governor_refs.push(id("inv:positive")))),
            "governor_refs"
        );
        assert_eq!(err(with(|s| s.event_refs.push(id(ORDER)))), "event_refs");
        assert_eq!(
            err(with(|s| s.data_schema_refs.push(id("schema:draft")))),
            "data_schema_refs"
        );
        assert_eq!(
            err(with(|s| s.event_payload_attribute_refs.push(id(ORDER)))),
            "event_payload_attribute_refs"
        );
        assert_eq!(
            err(with(|s| s.support_refs.push(id(CLERK)))),
            "support_refs"
        );
        // Support nodes are context only: they never create relations.
        let (c, proposals) = one(&g, &full_scope(), candidate("Approve Order", "command"));
        assert!(c.governed_by.is_empty() && c.reads.is_empty());
        assert_eq!(
            edge_set(&proposals[0]),
            BTreeSet::from([
                rel("performed_by", c.operation_ref.as_str(), CLERK),
                rel("specified_by", "req:r1", c.operation_ref.as_str())
            ])
        );
    }

    // ------------------------------------------------------------------ artifact errors

    #[test]
    fn operation_hard_artifact_errors() {
        let g = base_graph();
        let s = full_scope();
        let err = |c: Json| run(&g, &s, vec![c]).unwrap_err();
        let set = |field: &str, value: Json| {
            let mut c = candidate("Approve Order", "command");
            c[field] = value;
            c
        };
        assert!(matches!(
            err(set("reads", json!([gref("attr:ghost", "order")]))),
            OperationError::UnknownSemanticRef { .. }
        ));
        assert!(matches!(
            err(set("performers", json!([gref("secrole:clerk", "clerk")]))),
            OperationError::WrongTargetType { .. }
        ));
        assert!(matches!(
            err(set("reads", json!([gref(CLERK, "clerk")]))),
            OperationError::WrongTargetType { .. }
        ));
        assert!(matches!(
            err(set("writes", json!([gref("rule:approval", "order")]))),
            OperationError::WrongTargetType { .. }
        ));
        assert!(matches!(
            err(set("governed_by", json!([gref("inv:positive", "order")]))),
            OperationError::WrongTargetType { .. }
        ));
        assert!(matches!(
            err(set(
                "uses_calculation",
                json!([gref("calculation:legacy", "order")])
            )),
            OperationError::OutOfScopeRef { .. }
        ));
        assert!(matches!(
            run(
                &g,
                &golden_scope(),
                vec![set("reads", json!([gref(AMOUNT, "order")]))]
            )
            .unwrap_err(),
            OperationError::OutOfScopeRef { .. }
        ));
        assert!(matches!(
            err(set(
                "reads",
                json!([gref(ORDER, "order"), gref(ORDER, "reading")])
            )),
            OperationError::DuplicateRelationTarget { .. }
        ));
        assert!(matches!(
            err(set("input_schema", gref(ORDER, "order"))),
            OperationError::WrongTargetType { .. }
        ));
        assert!(matches!(
            err(set("output_schema", gref("schema:draft", "order"))),
            OperationError::OutOfScopeRef { .. }
        ));
        assert!(matches!(
            err(set(
                "produced_event_refs",
                json!([gref(GOLDEN_OUTCOME_ID, "order")])
            )),
            OperationError::UnknownSemanticRef { .. }
        ));
        let event = |payload: &str| {
            set(
                "new_produced_events",
                json!([{"name": "Order Approved", "payload_schema_ref": payload, "evidence": [range("approved")]}]),
            )
        };
        assert!(matches!(
            err(event(ORDER)),
            OperationError::WrongTargetType { .. }
        ));
        assert!(matches!(
            err(event("schema:draft")),
            OperationError::OutOfScopeRef { .. }
        ));
        assert!(matches!(
            err(event(AMOUNT)),
            OperationError::OutOfScopeRef { .. }
        ));
        // Grounding.
        let bad = |r: Json| set("evidence", json!([r]));
        assert!(matches!(
            err(bad(
                json!({"requirement_ref": "req:r9", "start": 0, "end": 3})
            )),
            OperationError::InvalidGrounding { .. }
        ));
        assert!(matches!(
            err(bad(
                json!({"requirement_ref": "req:r1", "start": 0, "end": 9999})
            )),
            OperationError::InvalidGrounding { .. }
        ));
        assert!(matches!(
            err(bad(
                json!({"requirement_ref": "req:r1", "start": 5, "end": 5})
            )),
            OperationError::InvalidGrounding { .. }
        ));
        assert!(matches!(
            err(set("evidence", json!([range("clerk"), range("clerk")]))),
            OperationError::InvalidGrounding { .. }
        ));
        // Identities.
        assert!(matches!(
            run(
                &g,
                &s,
                vec![
                    candidate("Approve Order", "command"),
                    candidate("Approve Order", "query")
                ]
            )
            .unwrap_err(),
            OperationError::DuplicateCandidate { .. }
        ));
        let outcome =
            json!({"name": "Done", "outcome_kind": "success", "evidence": [range("approved")]});
        assert!(matches!(
            err(set("outcomes", json!([outcome, outcome]))),
            OperationError::DuplicateCandidate { .. }
        ));
        // The same Event both selected and newly proposed is a duplicate produces target.
        let mut both = set(
            "produced_event_refs",
            json!([gref(GOLDEN_EVENT_ID, "order")]),
        );
        both["new_produced_events"] = json!([{"name": "Order Approved", "payload_schema_ref": null, "evidence": [range("order")]}]);
        let with_event = with_nodes(vec![event_node(
            GOLDEN_EVENT_ID,
            "Order Approved",
            None,
            None,
        )]);
        let mut event_scope = full_scope();
        event_scope.event_refs.push(id(GOLDEN_EVENT_ID));
        assert!(matches!(
            run(&with_event, &event_scope, vec![both]).unwrap_err(),
            OperationError::DuplicateRelationTarget { .. }
        ));
        assert_eq!(
            plumb_functional::event_id(&id(PROJECT), "Order Approved")
                .unwrap()
                .as_str(),
            GOLDEN_EVENT_ID
        );
        // Schema-level: kinds, unknown fields, bare IDs, version.
        let schema_invalid = |c: Json| matches!(err(c), OperationError::SchemaInvalid { .. });
        assert!(schema_invalid(set("operation_kind", json!("event"))));
        assert!(schema_invalid(set(
            "outcomes",
            json!([{"name": "X", "outcome_kind": "failure", "evidence": [range("order")]}])
        )));
        assert!(schema_invalid(set("preconditions", json!(["x"]))));
        assert!(schema_invalid(set("reads", json!([ORDER]))));
        assert!(schema_invalid(set(
            "performers",
            json!([{"target_ref": CLERK, "evidence": []}])
        )));
        assert!(matches!(
            run_with(&g, &s, json!({"version": 2, "operations": []})).unwrap_err(),
            OperationError::SchemaInvalid { .. }
        ));
        assert!(matches!(
            run_with(&g, &s, json!({"version": 1, "operations": [], "extra": 1})).unwrap_err(),
            OperationError::SchemaInvalid { .. }
        ));
        // Artifact bound to another request.
        let r = request(&g, &s);
        let mut artifact = artifact_for(&r.request, output(vec![]));
        artifact.request_hash = Hash::content_sha256(b"other");
        assert!(matches!(
            analyze_operations(
                &g,
                &r,
                Some(OperationInference {
                    artifact: &artifact,
                    derivation_ref: derivation()
                }),
                &audit()
            ),
            Err(OperationError::InvalidInferenceArtifact { .. })
        ));
    }

    #[test]
    fn operation_inference_unavailable() {
        let g = base_graph();
        let r = request(&g, &full_scope());
        let result = analyze_operations(&g, &r, None, &audit()).unwrap();
        assert_eq!(result.issues, vec![OperationIssue::InferenceUnavailable]);
        assert!(result.candidates.is_empty() && result.proposals.is_empty());
        // An unrelated node does not change the closed context; a changed scope node does.
        let unrelated = with_nodes(vec![entity("entity:extra", "Extra")]);
        assert!(analyze_operations(&unrelated, &r, None, &audit()).is_ok());
        let renamed: Vec<Node> = nodes()
            .into_iter()
            .map(|mut n| {
                if n.id.as_str() == CLERK {
                    n.payload = NodePayload::BusinessRole(BusinessRole {
                        name: "Senior Clerk".into(),
                    });
                }
                n
            })
            .collect();
        assert!(analyze_operations(&graph_of(renamed, edges()), &r, None, &audit()).is_err());
    }

    // ------------------------------------------------------------------ semantic issues

    #[test]
    fn operation_semantic_issues() {
        let g = base_graph();
        let mut query = candidate("Find Order", "query");
        query["reads"] = json!([gref(ORDER, "order")]);
        let (q, proposals) = one(&g, &full_scope(), query.clone());
        assert!(q.issues.is_empty() && q.writes.is_empty());
        assert_eq!(q.operation_kind, OperationKind::Query);
        assert_eq!(proposals.len(), 1);
        query["writes"] = json!([gref(STATUS, "status")]);
        let (q, proposals) = one(&g, &full_scope(), query);
        assert_eq!(
            q.issues,
            vec![OperationIssue::QueryWritesState {
                operation_ref: q.operation_ref.clone()
            }]
        );
        assert_eq!(q.operation_kind, OperationKind::Query);
        assert!(proposals.is_empty());
        assert_eq!(q.disposition, OperationDisposition::Ineligible);
        let mut nobody = candidate("Approve Order", "command");
        nobody["performers"] = json!([]);
        assert!(matches!(
            issues(&g, nobody).as_slice(),
            [OperationIssue::MissingPerformer { .. }]
        ));
        // An Actor is a valid performer too.
        let mut system = candidate("Approve Order", "command");
        system["performers"] = json!([gref("actor:system", "order")]);
        assert!(issues(&g, system).is_empty());
        // Names are never transformed: untrimmed or control characters are invalid.
        assert!(matches!(
            issues(&g, candidate(" Approve Order", "command")).as_slice(),
            [OperationIssue::InvalidName { .. }]
        ));
        let mut outcome = candidate("Approve Order", "command");
        outcome["outcomes"] = json!([{"name": "Ap\u{7}proved", "outcome_kind": "success", "evidence": [range("approved")]}]);
        assert!(matches!(
            issues(&g, outcome).as_slice(),
            [OperationIssue::InvalidName { .. }]
        ));
        let (c, _) = one(
            &g,
            &full_scope(),
            candidate("approve_order (v2)", "command"),
        );
        assert_eq!(c.name, "approve_order (v2)");
    }

    // ------------------------------------------------------------------ relations

    #[test]
    fn operation_relations_schemas_outcomes_and_events() {
        let g = base_graph();
        let mut c = candidate("Approve Order", "command");
        c["governed_by"] = json!([
            gref("rule:approval", "approve"),
            gref("dt:discount", "approve")
        ]);
        c["uses_calculation"] = json!([gref("calculation:total", "order")]);
        c["reads"] = json!([gref(AMOUNT, "order")]);
        c["input_schema"] = gref(ORDER_SCHEMA, "submitted order");
        c["output_schema"] = gref(ORDER_SCHEMA, "order status");
        c["produced_event_refs"] = json!([gref("event:shipped", "recording")]);
        c["consumed_event_refs"] = json!([gref("event:shipped", "submitted")]);
        c["outcomes"] = json!([
            {"name": "Approved", "outcome_kind": "success", "evidence": [range("approved")]},
            {"name": "Rejected", "outcome_kind": "business_failure", "evidence": [range("approve")]},
            {"name": "Unavailable", "outcome_kind": "technical_failure", "evidence": [range("order")]},
            {"name": "Partly Approved", "outcome_kind": "partial", "evidence": [range("customer")]},
        ]);
        c["new_produced_events"] = json!([
            {"name": "Order Approved", "payload_schema_ref": ORDER_SCHEMA, "evidence": [range("approved")]},
            {"name": "Order Audited", "payload_schema_ref": null, "evidence": [range("recording")]},
        ]);
        let (a, proposals) = one(&g, &full_scope(), c);
        assert!(a.issues.is_empty(), "{:?}", a.issues);
        assert_eq!(a.input_schema_ref, Some(id(ORDER_SCHEMA)));
        assert_eq!(a.output_schema_ref, Some(id(ORDER_SCHEMA)));
        let kinds: BTreeSet<OutcomeKind> = a.outcomes.iter().map(|o| o.outcome_kind).collect();
        assert_eq!(kinds.len(), 4);
        assert_eq!(a.consumed_events, ids(&["event:shipped"]));
        assert_eq!(a.produced_events.len(), 3);
        let p = &proposals[0];
        let op = a.operation_ref.as_str();
        let set = edge_set(p);
        for expected in [
            rel("governed_by", op, "rule:approval"),
            rel("governed_by", op, "dt:discount"),
            rel("uses_calculation", op, "calculation:total"),
            rel("produces", op, "event:shipped"),
            rel("consumes", op, "event:shipped"),
        ] {
            assert!(set.contains(&expected), "{expected:?}");
        }
        assert_eq!(set.iter().filter(|r| r.0 == "produces").count(), 4 + 3);
        // Selecting a Calculation adds no write and no read.
        assert!(!set.iter().any(|r| r.0 == "writes"));
        assert_eq!(set.iter().filter(|r| r.0 == "reads").count(), 1);
        // The existing Event is not mutated or re-added.
        assert!(!added_nodes(p)
            .iter()
            .any(|n| n.id.as_str() == "event:shipped"));
        assert_eq!(added_nodes(p).len(), 1 + 4 + 2);
        let applied = apply(&g, &proposals);
        assert_eq!(
            applied.node(&id("event:shipped")).unwrap().payload,
            g.node(&id("event:shipped")).unwrap().payload
        );
        // Outcome identity is scoped to the Operation: two Operations' "Approved" differ.
        let mut other = candidate("Approve Invoice", "command");
        other["outcomes"] = json!([{"name": "Approved", "outcome_kind": "success", "evidence": [range("approved")]}]);
        let (b, _) = one(&g, &full_scope(), other);
        assert_ne!(
            b.outcomes[0].outcome_ref,
            a.outcomes
                .iter()
                .find(|o| o.name == "Approved")
                .unwrap()
                .outcome_ref
        );
    }

    // ------------------------------------------------------------------ calculation reads

    #[test]
    fn operation_calculation_read_completeness() {
        let g = base_graph();
        let uses = |calc: &str, reads: &[&str]| {
            let mut c = candidate("Approve Order", "command");
            c["uses_calculation"] = json!([gref(calc, "order")]);
            c["reads"] = Json::Array(reads.iter().map(|r| gref(r, "order")).collect());
            one(&g, &full_scope(), c).0
        };
        // Covered by the exact Attribute or by its owning Entity.
        let by_attr = uses("calculation:total", &[AMOUNT]);
        assert!(by_attr.issues.is_empty(), "{:?}", by_attr.issues);
        assert_eq!(by_attr.calculation_reads[0].required_reads, ids(&[AMOUNT]));
        assert!(uses("calculation:total", &[ORDER]).issues.is_empty());
        let missing = uses("calculation:total", &[CUSTOMER]);
        assert_eq!(
            missing.issues,
            vec![OperationIssue::MissingCalculationRead {
                calculation_ref: id("calculation:total"),
                semantic_ref: id(AMOUNT)
            }]
        );
        assert_eq!(missing.disposition, OperationDisposition::Ineligible);
        // Transitive dependencies are traversed; the Calculation itself is not a read.
        let double = uses("calculation:double", &[]);
        assert_eq!(
            double.issues,
            vec![OperationIssue::MissingCalculationRead {
                calculation_ref: id("calculation:double"),
                semantic_ref: id(AMOUNT)
            }]
        );
        assert!(uses("calculation:double", &[ORDER]).issues.is_empty());
        // A dotted use needs the Entity itself; an Attribute does not imply the whole Entity.
        assert!(uses("calculation:vip", &[CUSTOMER]).issues.is_empty());
        let only_attr = uses("calculation:vip", &[VIP]);
        assert_eq!(
            only_attr.issues,
            vec![OperationIssue::MissingCalculationRead {
                calculation_ref: id("calculation:vip"),
                semantic_ref: id(CUSTOMER)
            }]
        );
        // Unqualified, unqualified dependency and cyclic Calculations are unavailable.
        for calc in [
            "calculation:broken",
            "calculation:uses-broken",
            "calculation:cyc-a",
        ] {
            let c = uses(calc, &[ORDER, CUSTOMER]);
            assert!(
                matches!(c.issues.as_slice(), [OperationIssue::CalculationReadAnalysisUnavailable { calculation_ref, .. }] if calculation_ref.as_str() == calc),
                "{calc}: {:?}",
                c.issues
            );
            assert_eq!(c.disposition, OperationDisposition::Ineligible);
        }
    }

    // ------------------------------------------------------------------ reconciliation

    #[test]
    fn operation_reconciliation() {
        let g = base_graph();
        let first = run(&g, &golden_scope(), vec![golden_candidate()]).unwrap();
        let applied = apply(&g, &first.proposals);
        let replay = run(&applied, &golden_scope(), vec![golden_candidate()]).unwrap();
        assert_eq!(
            replay.candidates[0].disposition,
            OperationDisposition::IdempotentReplay
        );
        assert!(replay.proposals.is_empty());
        let accepted = accept_all(&applied);
        let equivalent = run(&accepted, &golden_scope(), vec![golden_candidate()]).unwrap();
        assert_eq!(
            equivalent.candidates[0].disposition,
            OperationDisposition::ExistingEquivalent
        );
        assert!(equivalent.proposals.is_empty());
        // A different relation set or payload at the same ID is a conflict, never repaired.
        let mut fewer = golden_candidate();
        fewer["reads"] = json!([gref(ORDER, "order")]);
        let conflict = run(&accepted, &golden_scope(), vec![fewer]).unwrap();
        assert_eq!(
            conflict.candidates[0].disposition,
            OperationDisposition::ExistingConflict
        );
        assert_eq!(
            conflict.candidates[0].issues,
            vec![OperationIssue::ExistingOperationConflict {
                operation_ref: id(GOLDEN_OPERATION_ID)
            }]
        );
        let mut as_query = golden_candidate();
        as_query["operation_kind"] = json!("query");
        as_query["writes"] = json!([]);
        assert_eq!(
            run(&applied, &golden_scope(), vec![as_query])
                .unwrap()
                .candidates[0]
                .disposition,
            OperationDisposition::ExistingConflict
        );
        // An Outcome at the deterministic ID with another kind conflicts.
        let clash = node(
            GOLDEN_OUTCOME_ID,
            Accepted,
            NodePayload::Outcome(Outcome {
                name: "Approved".into(),
                outcome_kind: OutcomeKind::Partial,
            }),
        );
        let c = run(
            &with_nodes(vec![clash]),
            &golden_scope(),
            vec![golden_candidate()],
        )
        .unwrap();
        assert_eq!(
            c.candidates[0].issues,
            vec![OperationIssue::ExistingOutcomeConflict {
                outcome_ref: id(GOLDEN_OUTCOME_ID)
            }]
        );
        assert_eq!(
            c.candidates[0].disposition,
            OperationDisposition::ExistingConflict
        );
        assert!(c.proposals.is_empty());
        // An identical existing Outcome is reused, not re-added.
        let same = node(
            GOLDEN_OUTCOME_ID,
            Accepted,
            NodePayload::Outcome(Outcome {
                name: "Approved".into(),
                outcome_kind: OutcomeKind::Success,
            }),
        );
        let reuse = run(
            &with_nodes(vec![same]),
            &golden_scope(),
            vec![golden_candidate()],
        )
        .unwrap();
        assert!(!added_nodes(&reuse.proposals[0])
            .iter()
            .any(|n| n.id.as_str() == GOLDEN_OUTCOME_ID));
        assert!(edge_set(&reuse.proposals[0]).contains(&rel(
            "produces",
            GOLDEN_OPERATION_ID,
            GOLDEN_OUTCOME_ID
        )));
    }

    #[test]
    fn operation_event_reconciliation() {
        let event = |payload: Option<&str>| {
            let mut c = candidate("Approve Order", "command");
            c["new_produced_events"] = json!([{"name": "Order Approved", "payload_schema_ref": payload, "evidence": [range("approved")]}]);
            c
        };
        let existing = |payload: Option<&str>, semantic: Option<&str>| {
            with_nodes(vec![event_node(
                GOLDEN_EVENT_ID,
                "Order Approved",
                payload,
                semantic,
            )])
        };
        let check = |graph: Graph, payload: Option<&str>| {
            let (c, proposals) = one(&graph, &full_scope(), event(payload));
            (c, proposals)
        };
        // New Event.
        let (c, p) = check(base_graph(), None);
        assert!(!c.new_events[0].reused);
        assert!(added_nodes(&p[0])
            .iter()
            .any(|n| n.id.as_str() == GOLDEN_EVENT_ID));
        // Identical existing Event, a richer one with semantic_type, payload None reusing Some.
        for (graph, payload) in [
            (existing(None, None), None),
            (existing(None, Some("domain_event")), None),
            (existing(Some(ORDER_SCHEMA), Some("domain_event")), None),
            (existing(Some(ORDER_SCHEMA), None), Some(ORDER_SCHEMA)),
        ] {
            let (c, p) = check(graph.clone(), payload);
            assert!(c.issues.is_empty(), "{:?}", c.issues);
            assert!(c.new_events[0].reused);
            assert!(!added_nodes(&p[0])
                .iter()
                .any(|n| n.id.as_str() == GOLDEN_EVENT_ID));
            let applied = apply(&graph, &p);
            assert_eq!(
                applied.node(&id(GOLDEN_EVENT_ID)).unwrap().payload,
                graph.node(&id(GOLDEN_EVENT_ID)).unwrap().payload
            );
        }
        // Different non-null payload, or a payload where the existing Event has none.
        for (graph, payload) in [
            (existing(Some(ORDER_SCHEMA), None), Some(STATUS)),
            (existing(None, None), Some(STATUS)),
        ] {
            let (c, p) = check(graph, payload);
            assert_eq!(
                c.issues,
                vec![OperationIssue::ExistingEventConflict {
                    event_ref: id(GOLDEN_EVENT_ID)
                }]
            );
            assert!(p.is_empty());
        }
        // A legacy Event with the same name under another ID.
        let legacy = with_nodes(vec![event_node(
            "event:legacy-approved",
            "Order Approved",
            None,
            None,
        )]);
        let (c, p) = check(legacy, None);
        assert_eq!(
            c.issues,
            vec![OperationIssue::ExistingEventNameConflict {
                event_ref: id(GOLDEN_EVENT_ID),
                existing_ref: id("event:legacy-approved")
            }]
        );
        assert_eq!(c.disposition, OperationDisposition::ExistingConflict);
        assert!(p.is_empty());
    }

    // ------------------------------------------------------------------ determinism

    #[test]
    fn operation_deterministic_ordering() {
        let forward = run(
            &base_graph(),
            &full_scope(),
            vec![golden_candidate(), candidate("Find Order", "query")],
        )
        .unwrap();
        let mut reversed_nodes = nodes();
        reversed_nodes.reverse();
        let mut reversed_edges = edges();
        reversed_edges.reverse();
        let g2 = graph_of(reversed_nodes, reversed_edges);
        let mut scope = full_scope();
        for list in [
            &mut scope.performer_refs,
            &mut scope.domain_refs,
            &mut scope.calculation_refs,
            &mut scope.governor_refs,
            &mut scope.event_refs,
            &mut scope.data_schema_refs,
            &mut scope.event_payload_attribute_refs,
            &mut scope.support_refs,
        ] {
            list.reverse();
        }
        let mut c = golden_candidate();
        for field in ["reads", "evidence"] {
            c[field].as_array_mut().unwrap().reverse();
        }
        c["outcomes"] = json!([
            {"name": "Rejected", "outcome_kind": "business_failure", "evidence": [range("approve")]},
            {"name": "Approved", "outcome_kind": "success", "evidence": [range("the order was approved")]},
        ]);
        c["new_produced_events"] = json!([
            {"name": "Order Audited", "payload_schema_ref": null, "evidence": [range("recording")]},
            {"name": "Order Approved", "payload_schema_ref": STATUS, "evidence": [range("recording that the order was approved")]},
        ]);
        let mut c_forward = golden_candidate();
        c_forward["outcomes"] = json!([
            {"name": "Approved", "outcome_kind": "success", "evidence": [range("the order was approved")]},
            {"name": "Rejected", "outcome_kind": "business_failure", "evidence": [range("approve")]},
        ]);
        c_forward["new_produced_events"] = json!([
            {"name": "Order Approved", "payload_schema_ref": STATUS, "evidence": [range("recording that the order was approved")]},
            {"name": "Order Audited", "payload_schema_ref": null, "evidence": [range("recording")]},
        ]);
        let a = run(
            &base_graph(),
            &full_scope(),
            vec![c_forward, candidate("Find Order", "query")],
        )
        .unwrap();
        let b = run(&g2, &scope, vec![candidate("Find Order", "query"), c]).unwrap();
        assert_eq!(a.proposals, b.proposals);
        assert_eq!(format!("{:?}", a.candidates), format!("{:?}", b.candidates));
        assert!(a
            .candidates
            .windows(2)
            .all(|w| w[0].operation_ref < w[1].operation_ref));
        assert!(a.proposals.windows(2).all(|w| w[0].id < w[1].id));
        // Outcome and Event IDs do not depend on their order.
        assert_eq!(
            forward
                .candidates
                .iter()
                .find(|c| c.name == "Approve Order")
                .unwrap()
                .outcomes[0]
                .outcome_ref
                .as_str(),
            GOLDEN_OUTCOME_ID
        );
        let golden = a
            .candidates
            .iter()
            .find(|c| c.name == "Approve Order")
            .unwrap();
        assert!(golden
            .outcomes
            .iter()
            .any(|o| o.outcome_ref.as_str() == GOLDEN_OUTCOME_ID));
        assert!(golden
            .new_events
            .iter()
            .any(|e| e.event_ref.as_str() == GOLDEN_EVENT_ID));
    }

    // ------------------------------------------------------------------ HR operation_specs reference

    fn yaml_list(v: &serde_yaml::Value) -> Vec<String> {
        v.as_sequence()
            .map(|s| s.iter().map(|x| x.as_str().unwrap().to_owned()).collect())
            .unwrap_or_default()
    }

    /// Fixture operation_specs conformance: given a mock artifact carrying the fixture relations
    /// over a synthetic Accepted graph with fixture names, the canonical candidate material
    /// equals the fixture. The mock OutcomeKind is not fixture-grounded and is not compared.
    #[test]
    fn operation_hr_operation_specs_reference() {
        let contract: serde_yaml::Value = serde_yaml::from_str(HR_CONTRACT).unwrap();
        let specs = contract["operation_specs"].as_mapping().unwrap();
        let entities = yaml_list(&contract["entities"]);
        let business_roles = yaml_list(&contract["role_specs"]["business_roles"]);
        assert_eq!(specs.len(), 13);
        assert_eq!(entities.len(), 9);
        assert!(!business_roles.contains(&"ManagerBusinessRole".to_owned()));
        const STATEMENT: &str =
            "Synthetic grounding statement for the HR operation_specs conformance test.";
        let mut graph_nodes = vec![requirement("req:hr", STATEMENT)];
        let mut performer_ids = BTreeMap::new();
        for role in &business_roles {
            let node_id = format!("role:{role}");
            graph_nodes.push(node(
                &node_id,
                Accepted,
                NodePayload::BusinessRole(BusinessRole { name: role.clone() }),
            ));
            performer_ids.insert(role.clone(), node_id);
        }
        // `SystemActor` is a fixture performer not listed in role_specs; the synthetic graph
        // represents it as an Actor, a legal performer type.
        graph_nodes.push(node(
            "actor:SystemActor",
            Accepted,
            NodePayload::Actor(Actor {
                name: "SystemActor".into(),
                actor_kind: ActorKind::System,
            }),
        ));
        performer_ids.insert("SystemActor".into(), "actor:SystemActor".into());
        let mut entity_ids = BTreeMap::new();
        for e in &entities {
            let node_id = format!("entity:{e}");
            graph_nodes.push(entity(&node_id, e));
            entity_ids.insert(e.clone(), node_id);
        }
        let g = graph_of(graph_nodes, vec![]);
        let names: BTreeMap<Id, String> = g
            .nodes()
            .values()
            .filter_map(|n| match &n.payload {
                NodePayload::Entity(e) => Some((n.id.clone(), e.name.clone())),
                NodePayload::BusinessRole(r) => Some((n.id.clone(), r.name.clone())),
                NodePayload::Actor(a) => Some((n.id.clone(), a.name.clone())),
                _ => None,
            })
            .collect();
        let scope = OperationScope {
            performer_refs: performer_ids.values().map(|s| id(s)).collect(),
            domain_refs: entity_ids.values().map(|s| id(s)).collect(),
            ..OperationScope::default()
        };
        let r = json!({"requirement_ref": "req:hr", "start": 0, "end": STATEMENT.len()});
        let grounded = |target: &str| json!({"target_ref": target, "evidence": [r]});
        let mut candidates = Vec::new();
        for (name, spec) in specs {
            let name = name.as_str().unwrap();
            let performer = spec["performer"].as_str().unwrap();
            candidates.push(json!({
                "name": name,
                "operation_kind": spec["kind"].as_str().unwrap(),
                "evidence": [r],
                "performers": [grounded(&performer_ids[performer])],
                "reads": yaml_list(&spec["reads"]).iter().map(|e| grounded(&entity_ids[e])).collect::<Vec<_>>(),
                "writes": yaml_list(&spec["writes"]).iter().map(|e| grounded(&entity_ids[e])).collect::<Vec<_>>(),
                "governed_by": [],
                "uses_calculation": [],
                "input_schema": null,
                "output_schema": null,
                "outcomes": yaml_list(&spec["outcomes"]).iter().map(|o| json!({"name": o, "outcome_kind": "success", "evidence": [r]})).collect::<Vec<_>>(),
                "new_produced_events": yaml_list(&spec["events"]).iter().map(|e| json!({"name": e, "payload_schema_ref": null, "evidence": [r]})).collect::<Vec<_>>(),
                "produced_event_refs": [],
                "consumed_event_refs": [],
            }));
        }
        let request = build_operation_request(&g, &[id("req:hr")], &scope, provider()).unwrap();
        let artifact = artifact_for(&request.request, output(candidates));
        let result = analyze_operations(
            &g,
            &request,
            Some(OperationInference {
                artifact: &artifact,
                derivation_ref: derivation(),
            }),
            &audit(),
        )
        .unwrap();
        assert_eq!(result.candidates.len(), 13);
        assert_eq!(result.proposals.len(), 13);
        let named = |list: &[Id]| {
            list.iter()
                .map(|i| names[i].clone())
                .collect::<BTreeSet<_>>()
        };
        let as_set = |v: Vec<String>| v.into_iter().collect::<BTreeSet<_>>();
        for (name, spec) in specs {
            let name = name.as_str().unwrap();
            let c = result
                .candidates
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("{name}"));
            assert!(c.issues.is_empty(), "{name}: {:?}", c.issues);
            assert!(matches!(
                c.disposition,
                OperationDisposition::Proposed { .. }
            ));
            let kind = match spec["kind"].as_str().unwrap() {
                "command" => OperationKind::Command,
                "query" => OperationKind::Query,
                other => panic!("{other}"),
            };
            assert_eq!(c.operation_kind, kind, "{name}");
            assert_eq!(
                named(&c.performers),
                BTreeSet::from([spec["performer"].as_str().unwrap().to_owned()]),
                "{name}"
            );
            assert_eq!(named(&c.reads), as_set(yaml_list(&spec["reads"])), "{name}");
            assert_eq!(
                named(&c.writes),
                as_set(yaml_list(&spec["writes"])),
                "{name}"
            );
            assert_eq!(
                c.outcomes
                    .iter()
                    .map(|o| o.name.clone())
                    .collect::<BTreeSet<_>>(),
                as_set(yaml_list(&spec["outcomes"])),
                "{name}"
            );
            assert_eq!(
                c.new_events
                    .iter()
                    .map(|e| e.name.clone())
                    .collect::<BTreeSet<_>>(),
                as_set(yaml_list(&spec["events"])),
                "{name}"
            );
            if kind == OperationKind::Query {
                assert!(
                    yaml_list(&spec["writes"]).is_empty() && c.writes.is_empty(),
                    "{name}"
                );
            }
        }
        for p in &result.proposals {
            apply(&g, std::slice::from_ref(p));
        }
        // ApproveLeaveRequest explicit regression: ManagerRole and the exact entity sets.
        let approve = result
            .candidates
            .iter()
            .find(|c| c.name == "ApproveLeaveRequest")
            .unwrap();
        assert_eq!(approve.performers, ids(&["role:ManagerRole"]));
        assert_eq!(
            named(&approve.reads),
            as_set(
                [
                    "LeaveRequest",
                    "ManagerAssignment",
                    "EmploymentContract",
                    "LeaveBalance",
                    "LeaveType"
                ]
                .map(String::from)
                .to_vec()
            )
        );
        assert_eq!(
            named(&approve.writes),
            as_set(
                [
                    "LeaveRequest",
                    "ApprovalRecord",
                    "LeaveBalance",
                    "AuditRecord"
                ]
                .map(String::from)
                .to_vec()
            )
        );
        assert!(approve
            .reads
            .iter()
            .all(|r| g.node(r).unwrap().payload.node_type() == NodeType::Entity));
    }

    // ------------------------------------------------------------------ guards

    #[test]
    fn operation_source_guard() {
        for (file, source) in [
            ("operation.rs", include_str!("../src/operation.rs")),
            ("event.rs", include_str!("../src/event.rs")),
        ] {
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
                "Sqlite",
                ".execute(",
                "MockProvider",
                "rand::",
                "f64",
                "f32",
                "unsafe",
                "fixtures/",
                "hr-leave",
                "LeaveRequest",
                "ApproveLeaveRequest",
                "ManagerRole",
                "SystemActor",
                "PLUMB.F2",
                "GeneratedFinding",
                "Employee.manager",
                "Contract.type",
                "Permission",
                "NodePayload::SecurityRole",
                "NodePayload::Principal",
                "NodePayload::Process",
                "RelationKind::Next",
                "NodePayload::DataSchema(",
                "commit(",
            ] {
                assert!(!source.contains(forbidden), "{file} contains {forbidden}");
            }
        }
        let lib = include_str!("../src/lib.rs");
        assert!(!lib.contains("Proposal"));
        assert!(lib.contains("pub mod operation;") && lib.contains("pub mod event;"));
        let tests = include_str!("operation.rs");
        let legacy = ["Employee", ".manager"].concat();
        assert_eq!(
            tests.matches(legacy.as_str()).count(),
            1,
            "legacy attribute only in this guard"
        );
    }
}
