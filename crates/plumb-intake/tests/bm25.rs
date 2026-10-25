//! X1.2 contract tests for the deterministic lexical BM25 retrieval index (Hotfix 058).
//!
//! Every test lives in `bm25` (`cargo test -p plumb-intake bm25`). Fixtures are hand-built
//! typed PSGs. The corpus-hash and BM25 goldens were computed independently in Python
//! (RFC 8785 via sorted-key compact `json.dumps`, `hashlib.sha256`, `math.log`).

mod bm25 {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{canonical_hash, Hash, HashKind, Id, Timestamp};
    use plumb_intake::{
        tokenize, IndexError, IndexUpdateSummary, LexicalIndex, RetrievalDocumentKind,
        RetrievalHit, BM25_B, BM25_K1, RETRIEVAL_INDEX_VERSION,
    };
    use plumb_psg::{
        evidence_fragment_id, payload_refs, source_artifact_id, Agent, AgentKind, AuditMeta,
        Calculation, Calendar, Concept, ConceptKind, DecisionTable, ElementStatus, Entity,
        EvidenceFragment, EvidenceLocator, Finding, FindingSeverity, Graph, Modality, Node,
        NodePayload, Operation, OperationKind, Question, QuestionKind, Requirement,
        RequirementKind, RequirementLevel, Rule, RuleKind, Scenario, ScenarioKind, SourceArtifact,
        Stakeholder, Term,
    };
    use proptest::prelude::*;
    use serde_json::{json, Value};

    use ElementStatus::{Accepted, Deprecated, Proposed, Rejected, Superseded, Suspect};

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const TOLERANCE: f64 = 1e-12;
    const BM25_SRC: &str = include_str!("../src/bm25.rs");
    const INDEX_SRC: &str = include_str!("../src/index.rs");
    const LIB_SRC: &str = include_str!("../src/lib.rs");

    /// Python: RFC 8785 canonical JSON of the representative corpus projection, SHA-256.
    const GOLDEN_CORPUS_HASH: &str =
        "sha256:8a54b796f4efb288641354fa351750d9261a8bc3b8e7657fbdd2a0df0d891f57";
    /// Python: `"src:" + sha256(b"leave-policy-v1")[:16]` and the derived fragment ID.
    const SOURCE_ID: &str = "src:42d25a85ad17bbff";
    const FRAGMENT_ID: &str = "evd:2a6d670afe6bbb03";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
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
            audit: AuditMeta::new(id("agent:analyst"), ts(AT), None, None).unwrap(),
        }
    }

    fn operation(node_id: &str, name: &str) -> Node {
        node(
            node_id,
            Accepted,
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

    /// The Accepted, never-indexed nodes the fixtures' graph-bound payload references name
    /// (architecture §33.11), added when a fixture references one that is not present.
    fn reference_targets(nodes: &[Node]) -> Vec<Node> {
        let targets = [
            node(
                "agent:zebra",
                Accepted,
                NodePayload::Agent(Agent {
                    agent_kind: AgentKind::Human,
                }),
            ),
            node(
                "stakeholder:zebra",
                Accepted,
                NodePayload::Stakeholder(Stakeholder {
                    name: "Zebra".into(),
                    stakeholder_kind: "business".into(),
                    organization: None,
                    responsibilities: None,
                    contact_ref: None,
                }),
            ),
            node(
                "calendar:zebra",
                Accepted,
                NodePayload::Calendar(Calendar {
                    time_zone: "UTC".into(),
                    week_pattern: json!({}),
                    region: None,
                    holiday_source: None,
                }),
            ),
            operation("operation:quokka", "Quokka"),
        ];
        let present: BTreeSet<&Id> = nodes.iter().map(|n| &n.id).collect();
        let referenced: BTreeSet<&Id> = nodes
            .iter()
            .flat_map(|n| payload_refs(&n.payload))
            .map(|r| r.target)
            .collect();
        targets
            .into_iter()
            .filter(|t| referenced.contains(&t.id) && !present.contains(&t.id))
            .collect()
    }

    fn graph_in(project: &str, profile: &str, mut nodes: Vec<Node>) -> Graph {
        nodes.extend(reference_targets(&nodes));
        Graph::new(id(project), id(profile), nodes, Vec::new()).unwrap()
    }

    fn graph(nodes: Vec<Node>) -> Graph {
        graph_in(PROJECT, PROFILE, nodes)
    }

    fn requirement(node_id: &str, status: ElementStatus, statement: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::Requirement(Requirement {
                statement: statement.into(),
                requirement_kind: RequirementKind::Functional,
                level: RequirementLevel::System,
                modality: Modality::Shall,
                title: Some("Zebra title".into()),
                rationale: Some("Zebra rationale".into()),
                priority: Some("zebrapriority".into()),
                source_identifier: Some("ZEBRA-1".into()),
                verification_method: Some("zebratest".into()),
                owner_refs: Some(vec![id("agent:zebra")]),
                stakeholder_refs: Some(vec![id("stakeholder:zebra")]),
            }),
        )
    }

    fn concept(node_id: &str, status: ElementStatus, name: &str, definition: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::Concept(Concept {
                name: name.into(),
                definition: definition.into(),
                concept_kind: ConceptKind::ObjectType,
            }),
        )
    }

    fn rule(
        node_id: &str,
        status: ElementStatus,
        name: &str,
        condition: Option<&str>,
        action: Option<&str>,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Rule(Rule {
                name: name.into(),
                rule_kind: RuleKind::Business,
                condition_expr: condition.map(Into::into),
                action_expr: action.map(Into::into),
            }),
        )
    }

    fn calculation(
        node_id: &str,
        status: ElementStatus,
        unit: Option<&str>,
        rounding: Option<&str>,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Calculation(Calculation {
                name: "Remaining balance".into(),
                expression: "entitlement - taken".into(),
                result_type: "Decimal".into(),
                unit: unit.map(Into::into),
                rounding: rounding.map(Into::into),
                calendar_ref: Some(id("calendar:zebra")),
                examples: Some(vec![json!({"zebra": 1})]),
            }),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn scenario(
        node_id: &str,
        status: ElementStatus,
        name: &str,
        kind: ScenarioKind,
        given: Vec<Value>,
        when: Value,
        then: Vec<Value>,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Scenario(Scenario {
                name: name.into(),
                scenario_kind: kind,
                given,
                when,
                then,
                requirement_refs: None,
                derived_from_refs: Some(vec![id("operation:quokka")]),
                confirmation_status: Some("Confirmed".into()),
            }),
        )
    }

    fn source() -> Node {
        let content_hash = Hash::content_sha256(b"leave-policy-v1");
        let source_id = source_artifact_id(&content_hash).unwrap();
        node(
            source_id.as_str(),
            Accepted,
            NodePayload::SourceArtifact(SourceArtifact {
                source_kind: "document".into(),
                display_name: "Leave policy".into(),
                content_hash,
                media_type: "text/plain".into(),
                external_uri: None,
                external_version: None,
                producer: None,
                created_at_source: None,
                language: Some("en".into()),
                classification: None,
            }),
        )
    }

    fn fragment_with(status: ElementStatus, end: u64, text: Option<&str>) -> Node {
        let locator = EvidenceLocator::TextRange { start: 0, end };
        let fragment_id = evidence_fragment_id(&id(SOURCE_ID), &locator).unwrap();
        node(
            fragment_id.as_str(),
            status,
            NodePayload::EvidenceFragment(EvidenceFragment {
                source_ref: id(SOURCE_ID),
                locator,
                content_hash: Hash::content_sha256(b"fragment-v1"),
                extracted_text: text.map(Into::into),
                speaker: Some("Zebra".into()),
                source_timestamp: None,
            }),
        )
    }

    fn fragment(status: ElementStatus, text: Option<&str>) -> Node {
        fragment_with(status, 42, text)
    }

    fn entity(node_id: &str, name: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Entity(Entity {
                name: name.into(),
                description: Some("Leave request entity".into()),
                aggregate_root: None,
            }),
        )
    }

    fn unsupported_nodes() -> Vec<Node> {
        vec![
            entity("entity:leave-request", "Leave request"),
            node(
                "operation:submit-leave",
                Accepted,
                NodePayload::Operation(Operation {
                    name: "Submit leave request".into(),
                    operation_kind: OperationKind::Command,
                    input_schema_ref: None,
                    output_schema_ref: None,
                    preconditions: Some(vec!["leave request open".into()]),
                    postconditions: None,
                    idempotency: None,
                    transaction_semantics: None,
                }),
            ),
            node(
                "decision_table:approval",
                Accepted,
                NodePayload::DecisionTable(DecisionTable {
                    name: "Leave approval table".into(),
                    hit_policy: "unique".into(),
                    inputs: vec![json!({"name": "leave request days"})],
                    outputs: vec![json!({"name": "approval"})],
                    rows: vec![json!({"when": ["leave"], "then": ["approve"]})],
                }),
            ),
            node(
                "term:leave",
                Accepted,
                NodePayload::Term(Term {
                    term: "leave request".into(),
                    language: "en".into(),
                    definition_ref: None,
                    aliases: Some(vec!["leave".into()]),
                    status: None,
                }),
            ),
            node(
                "finding:leave",
                Accepted,
                NodePayload::Finding(Finding {
                    code: "LEAVE".into(),
                    family: "leave".into(),
                    severity: FindingSeverity::Info,
                    message: "leave request finding".into(),
                    status: "open".into(),
                    affected_refs: vec![id("entity:leave-request")],
                    standard_rule_ref: None,
                    suggested_resolution: None,
                    waiver_ref: None,
                }),
            ),
            node(
                "question:leave",
                Accepted,
                NodePayload::Question(Question {
                    finding_ref: id("finding:leave"),
                    question_kind: QuestionKind::Text,
                    prompt: "What is a leave request?".into(),
                    status: "open".into(),
                    answer_schema: None,
                    stakeholder_ref: None,
                    priority: None,
                    round_ref: None,
                    context_refs: None,
                }),
            ),
        ]
    }

    /// The representative corpus: 2 Requirements, 1 Concept, 1 Rule, 1 Calculation, 2 Scenarios
    /// and 1 EvidenceFragment, plus a Proposed and a token-empty Requirement, an unsupported
    /// SourceArtifact and the six unsupported controls.
    fn representative_nodes() -> Vec<Node> {
        let mut nodes = vec![
            requirement(
                "requirement:r1",
                Accepted,
                "The employee shall submit a leave request.",
            ),
            requirement(
                "requirement:r2",
                Accepted,
                "A manager must approve each leave request within 2 days.",
            ),
            requirement(
                "requirement:r3",
                Proposed,
                "Contractors shall request leave.",
            ),
            requirement("requirement:r4", Accepted, "— … !!"),
            concept(
                "concept:leave",
                Accepted,
                "Leave",
                "An approved absence from work.",
            ),
            rule(
                "rule:approval",
                Accepted,
                "Manager approval",
                Some("request.days > 0"),
                Some("route_to(manager)"),
            ),
            calculation(
                "calculation:balance",
                Accepted,
                Some("day"),
                Some("half_up"),
            ),
            scenario(
                "scenario:s1",
                Accepted,
                "Submit request",
                ScenarioKind::Acceptance,
                vec![json!({"employee": "active"})],
                json!({"operation": "submit"}),
                vec![json!({"status": "pending"})],
            ),
            scenario(
                "scenario:s2",
                Accepted,
                "Żółć boundary",
                ScenarioKind::Boundary,
                vec![],
                json!({"days": 2}),
                vec![json!({"result": "rejected", "reason": "Zu früh"})],
            ),
            source(),
            fragment(Accepted, Some("Employees request leave in half-day units.")),
        ];
        nodes.extend(unsupported_nodes());
        nodes
    }

    fn representative() -> Graph {
        graph(representative_nodes())
    }

    fn build(g: &Graph) -> LexicalIndex {
        LexicalIndex::build(g).unwrap()
    }

    fn search(index: &LexicalIndex, query: &str, top_k: usize) -> Vec<RetrievalHit> {
        index.search(query, top_k).unwrap()
    }

    fn ids(hits: &[RetrievalHit]) -> Vec<&str> {
        hits.iter().map(|h| h.id.as_str()).collect()
    }

    /// The independent expected corpus hash of `documents` (id, kind, text) in the fixture
    /// project and profile.
    fn expected_corpus_hash(documents: &[(&str, &str, &str)]) -> Hash {
        let mut documents = documents.to_vec();
        documents.sort();
        let entries: Vec<Value> = documents
            .iter()
            .map(|(i, k, t)| json!({"id": i, "kind": k, "text": t}))
            .collect();
        canonical_hash(
            HashKind::Generic,
            &json!({"version": 1, "project_id": PROJECT, "profile_id": PROFILE, "documents": entries}),
        )
        .unwrap()
    }

    /// Asserts the single-document corpus of `n` has exactly this kind and retrieval text.
    fn assert_projection(n: Node, kind: &str, text: &str) {
        let node_id = n.id.as_str().to_owned();
        let index = build(&graph(vec![n]));
        assert_eq!(index.document_count(), 1);
        assert_eq!(
            index.corpus_hash(),
            &expected_corpus_hash(&[(&node_id, kind, text)]),
            "projection of {node_id}"
        );
    }

    #[test]
    fn indexed_text_is_the_exact_read_only_projection() {
        // Hotfix 065: search snippets are prefixes of exactly the indexed text.
        let g = representative();
        let index = build(&g);
        assert_eq!(
            index.indexed_text(&id("requirement:r1")),
            Some("The employee shall submit a leave request.")
        );
        assert_eq!(
            index.indexed_text(&id("concept:leave")),
            Some("Leave\nAn approved absence from work.")
        );
        assert_eq!(
            index.indexed_text(&id(FRAGMENT_ID)),
            Some("Employees request leave in half-day units.")
        );
        // Proposed, token-empty and unsupported nodes are not indexed.
        for absent in ["requirement:r3", "requirement:r4", SOURCE_ID, "entity:x"] {
            assert_eq!(index.indexed_text(&id(absent)), None, "{absent}");
        }
        // Every hit of a search has indexed text, and the accessor never changes the index.
        let before = index.clone();
        for hit in search(&index, "leave", usize::MAX) {
            assert!(index.indexed_text(&hit.id).is_some());
        }
        assert_eq!(index, before);
    }

    const REGRESSION_QUERIES: &[&str] = &[
        "leave",
        "leave request",
        "leave leave request",
        "manager approval",
        "żółć",
        "2",
        "half day units",
        "nonexistent",
        "",
    ];

    fn assert_same_hits(a: &[RetrievalHit], b: &[RetrievalHit]) {
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            assert_eq!(x.id, y.id);
            assert_eq!(x.kind, y.kind);
            assert!((x.score - y.score).abs() <= TOLERANCE, "{x:?} vs {y:?}");
        }
    }

    /// Updates a clone of `old` to `new`, requires full equality with a rebuild of `new` and
    /// identical regression rankings, and returns the summary.
    fn update_equals_rebuild(old: &Graph, new: &Graph) -> IndexUpdateSummary {
        let mut updated = build(old);
        let summary = updated.update(new).unwrap();
        let rebuilt = build(new);
        assert_eq!(updated, rebuilt, "incremental index must equal rebuild");
        assert_eq!(updated.corpus_hash(), rebuilt.corpus_hash());
        assert_eq!(updated.semantic_hash(), &new.semantic_hash().unwrap());
        assert_eq!(updated.evidence_hash(), &new.evidence_hash().unwrap());
        assert_eq!(updated.document_count(), rebuilt.document_count());
        for query in REGRESSION_QUERIES {
            assert_same_hits(&search(&updated, query, 50), &search(&rebuilt, query, 50));
        }
        summary
    }

    fn set(items: &[&str]) -> BTreeSet<Id> {
        items.iter().map(|s| id(s)).collect()
    }

    fn replace(nodes: &mut [Node], node_id: &str, with: Node) {
        let slot = nodes.iter_mut().find(|n| n.id.as_str() == node_id).unwrap();
        *slot = with;
    }

    // ------------------------------------------------------------------ constants and wire

    #[test]
    fn frozen_constants() {
        assert_eq!(RETRIEVAL_INDEX_VERSION, 1);
        assert_eq!(BM25_K1, 1.2);
        assert_eq!(BM25_B, 0.75);
    }

    #[test]
    fn document_kind_wire_strings() {
        let kinds = [
            (RetrievalDocumentKind::Requirement, "requirement"),
            (RetrievalDocumentKind::Concept, "concept"),
            (RetrievalDocumentKind::Rule, "rule"),
            (RetrievalDocumentKind::Calculation, "calculation"),
            (RetrievalDocumentKind::Scenario, "scenario"),
            (RetrievalDocumentKind::Evidence, "evidence"),
        ];
        for (kind, wire) in kinds {
            assert_eq!(kind.as_str(), wire);
            assert_eq!(kind.to_string(), wire);
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(wire));
            assert_eq!(
                serde_json::from_value::<RetrievalDocumentKind>(json!(wire)).unwrap(),
                kind
            );
        }
        assert!(serde_json::from_value::<RetrievalDocumentKind>(json!("Requirement")).is_err());
    }

    // ------------------------------------------------------------------ tokenizer

    #[test]
    fn tokenizer_exact_vectors() {
        let cases: &[(&str, &[&str])] = &[
            ("Leave Request", &["leave", "request"]),
            ("LEAVE-request", &["leave", "request"]),
            ("half-day", &["half", "day"]),
            ("0.5 day", &["0", "5", "day"]),
            ("Employee.manager", &["employee", "manager"]),
            ("leave_request", &["leave", "request"]),
            ("żółć ŁÓDŹ", &["żółć", "łódź"]),
            ("Straße STRASSE ẞ", &["straße", "strasse", "ß"]),
            ("٣ apples Ⅻ", &["٣", "apples", "ⅻ"]),
            // to_lowercase('İ') is 'i' + U+0307 COMBINING DOT ABOVE, which is not alphanumeric.
            ("İstanbul", &["i", "stanbul"]),
            ("  leave\t\nrequest\r\n ", &["leave", "request"]),
            ("a1b2 3c", &["a1b2", "3c"]),
            (
                "the shall must and or",
                &["the", "shall", "must", "and", "or"],
            ),
            ("requests approved", &["requests", "approved"]),
            ("", &[]),
            ("   \t\n", &[]),
            ("!?-_.,;:()[]{}\"'/\\—…", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(&tokenize(input), expected, "tokenize({input:?})");
        }
    }

    // ------------------------------------------------------------------ projection

    #[test]
    fn requirement_projection_is_statement_only() {
        assert_projection(
            requirement(
                "requirement:x",
                Accepted,
                "The employee shall submit a leave request.",
            ),
            "requirement",
            "The employee shall submit a leave request.",
        );
        let index = build(&graph(vec![requirement(
            "requirement:x",
            Accepted,
            "Leave",
        )]));
        assert!(search(&index, "zebra zebrapriority zebratest", 10).is_empty());
    }

    #[test]
    fn concept_projection() {
        assert_projection(
            concept("concept:x", Accepted, "Leave", "An approved absence."),
            "concept",
            "Leave\nAn approved absence.",
        );
    }

    #[test]
    fn rule_projection_line_combinations() {
        let cases = [
            (None, None, "Manager approval"),
            (Some("days > 0"), None, "Manager approval\ndays > 0"),
            (
                None,
                Some("route(manager)"),
                "Manager approval\nroute(manager)",
            ),
            (
                Some("days > 0"),
                Some("route(manager)"),
                "Manager approval\ndays > 0\nroute(manager)",
            ),
        ];
        for (condition, action, text) in cases {
            assert_projection(
                rule("rule:x", Accepted, "Manager approval", condition, action),
                "rule",
                text,
            );
        }
    }

    #[test]
    fn calculation_projection_line_combinations() {
        let base = "Remaining balance\nentitlement - taken\nDecimal";
        let cases = [
            (None, None, base.to_owned()),
            (Some("day"), None, format!("{base}\nday")),
            (None, Some("half_up"), format!("{base}\nhalf_up")),
            (
                Some("day"),
                Some("half_up"),
                format!("{base}\nday\nhalf_up"),
            ),
        ];
        for (unit, rounding, text) in cases {
            assert_projection(
                calculation("calculation:x", Accepted, unit, rounding),
                "calculation",
                &text,
            );
        }
        let index = build(&graph(vec![calculation(
            "calculation:x",
            Accepted,
            None,
            None,
        )]));
        assert!(search(&index, "zebra calendar", 10).is_empty());
    }

    #[test]
    fn scenario_projection_is_name_and_canonical_json() {
        assert_projection(
            scenario(
                "scenario:x",
                Accepted,
                "Submit request",
                ScenarioKind::StateTransition,
                vec![json!({"z": 1, "a": [true, null]})],
                json!({"operation": "submit"}),
                vec![json!({"status": "pending"}), json!("done")],
            ),
            "scenario",
            "Submit request\n{\"given\":[{\"a\":[true,null],\"z\":1}],\"scenario_kind\":\"state_transition\",\"then\":[{\"status\":\"pending\"},\"done\"],\"when\":{\"operation\":\"submit\"}}",
        );
    }

    #[test]
    fn scenario_refs_and_confirmation_are_not_searchable() {
        let index = build(&graph(vec![scenario(
            "scenario:x",
            Accepted,
            "Submit request",
            ScenarioKind::Acceptance,
            vec![],
            json!({}),
            vec![],
        )]));
        assert_eq!(index.document_count(), 1);
        assert!(search(&index, "zebra", 10).is_empty());
        assert!(search(&index, "quokka", 10).is_empty());
        assert!(search(&index, "confirmed", 10).is_empty());
        // The canonical JSON keys are tokenized normally.
        assert_eq!(ids(&search(&index, "given", 10)), ["scenario:x"]);
        assert_eq!(ids(&search(&index, "scenario kind", 10)), ["scenario:x"]);
    }

    #[test]
    fn evidence_projection_is_extracted_text_only() {
        let f = fragment(Accepted, Some("Employees request leave."));
        let fragment_id = f.id.as_str().to_owned();
        let index = build(&graph(vec![source(), f]));
        assert_eq!(index.document_count(), 1);
        assert_eq!(
            index.corpus_hash(),
            &expected_corpus_hash(&[(&fragment_id, "evidence", "Employees request leave.")])
        );
        assert!(search(&index, "zebra", 10).is_empty());
        assert!(search(&index, "policy document", 10).is_empty());
    }

    #[test]
    fn evidence_without_extracted_text_is_absent() {
        let index = build(&graph(vec![source(), fragment(Accepted, None)]));
        assert_eq!(index.document_count(), 0);
        assert_eq!(index.corpus_hash(), &expected_corpus_hash(&[]));
    }

    #[test]
    fn unsupported_node_types_are_not_indexed() {
        let mut nodes = unsupported_nodes();
        nodes.push(source());
        let index = build(&graph(nodes));
        assert_eq!(index.document_count(), 0);
        assert_eq!(index.corpus_hash(), &expected_corpus_hash(&[]));
        assert!(search(&index, "leave request approval", 10).is_empty());
    }

    #[test]
    fn decision_table_is_not_indexed() {
        let table = unsupported_nodes()
            .into_iter()
            .find(|n| matches!(n.payload, NodePayload::DecisionTable(_)))
            .unwrap();
        let index = build(&graph(vec![
            table,
            requirement("requirement:r1", Accepted, "Unrelated text"),
        ]));
        assert_eq!(index.document_count(), 1);
        assert!(search(&index, "leave approval table", 10).is_empty());
        assert!(search(&index, "approve", 10).is_empty());
    }

    #[test]
    fn status_matrix_admits_only_accepted() {
        type Family = fn(ElementStatus) -> Vec<Node>;
        let families: [(&str, Family); 6] = [
            ("requirement", |s| {
                vec![requirement("requirement:x", s, "Leave")]
            }),
            ("concept", |s| {
                vec![concept("concept:x", s, "Leave", "Absence")]
            }),
            ("rule", |s| {
                vec![rule("rule:x", s, "Leave rule", None, None)]
            }),
            ("calculation", |s| {
                vec![calculation("calculation:x", s, None, None)]
            }),
            ("scenario", |s| {
                vec![scenario(
                    "scenario:x",
                    s,
                    "Leave",
                    ScenarioKind::Acceptance,
                    vec![],
                    json!({}),
                    vec![],
                )]
            }),
            ("evidence", |s| {
                vec![source(), fragment(s, Some("Leave text"))]
            }),
        ];
        let statuses = [
            (Accepted, 1),
            (Proposed, 0),
            (Suspect, 0),
            (Superseded, 0),
            (Deprecated, 0),
            (Rejected, 0),
        ];
        for (family, make) in families {
            for (status, expected) in statuses {
                let index = build(&graph(make(status)));
                assert_eq!(index.document_count(), expected, "{family} {status:?}");
                let hits = search(&index, "leave remaining", 10);
                assert_eq!(hits.len(), expected, "{family} {status:?}");
                if let Some(hit) = hits.first() {
                    assert_eq!(hit.kind.as_str(), family);
                }
            }
        }
    }

    #[test]
    fn token_empty_documents_are_omitted_from_statistics() {
        let base = vec![
            requirement("requirement:a", Accepted, "leave request"),
            requirement("requirement:b", Accepted, "manager approval"),
        ];
        let mut with_empty = base.clone();
        with_empty.push(requirement("requirement:empty", Accepted, " -- !! … "));
        with_empty.push(concept("concept:empty", Accepted, "—", "."));
        let plain = build(&graph(base));
        let index = build(&graph(with_empty));
        assert_eq!(index.document_count(), 2);
        assert_eq!(index.corpus_hash(), plain.corpus_hash());
        for query in ["leave", "manager request"] {
            assert_eq!(search(&index, query, 10), search(&plain, query, 10));
        }
    }

    // ------------------------------------------------------------------ representative corpus

    #[test]
    fn representative_corpus_hash_matches_python_golden() {
        let g = representative();
        assert_eq!(source().id.as_str(), SOURCE_ID);
        assert_eq!(fragment(Accepted, None).id.as_str(), FRAGMENT_ID);
        let index = build(&g);
        assert_eq!(index.document_count(), 8);
        assert_eq!(index.corpus_hash().as_str(), GOLDEN_CORPUS_HASH);
        assert_eq!(index.corpus_hash().kind(), HashKind::Generic);
        assert_eq!(index.project_id().as_str(), PROJECT);
        assert_eq!(index.profile_id().as_str(), PROFILE);
        // Rust-side cross-check of the exact projected texts.
        let expected = expected_corpus_hash(&[
            ("requirement:r1", "requirement", "The employee shall submit a leave request."),
            (
                "requirement:r2",
                "requirement",
                "A manager must approve each leave request within 2 days.",
            ),
            ("concept:leave", "concept", "Leave\nAn approved absence from work."),
            (
                "rule:approval",
                "rule",
                "Manager approval\nrequest.days > 0\nroute_to(manager)",
            ),
            (
                "calculation:balance",
                "calculation",
                "Remaining balance\nentitlement - taken\nDecimal\nday\nhalf_up",
            ),
            (
                "scenario:s1",
                "scenario",
                "Submit request\n{\"given\":[{\"employee\":\"active\"}],\"scenario_kind\":\"acceptance\",\"then\":[{\"status\":\"pending\"}],\"when\":{\"operation\":\"submit\"}}",
            ),
            (
                "scenario:s2",
                "scenario",
                "Żółć boundary\n{\"given\":[],\"scenario_kind\":\"boundary\",\"then\":[{\"reason\":\"Zu früh\",\"result\":\"rejected\"}],\"when\":{\"days\":2}}",
            ),
            (FRAGMENT_ID, "evidence", "Employees request leave in half-day units."),
        ]);
        assert_eq!(index.corpus_hash(), &expected);
    }

    #[test]
    fn representative_search_kinds_and_controls() {
        let index = build(&representative());
        let hits = search(&index, "leave", 50);
        let found: BTreeSet<&str> = ids(&hits).into_iter().collect();
        assert_eq!(
            found,
            BTreeSet::from([
                "concept:leave",
                FRAGMENT_ID,
                "requirement:r1",
                "requirement:r2"
            ])
        );
        let kinds: BTreeMap<&str, RetrievalDocumentKind> =
            hits.iter().map(|h| (h.id.as_str(), h.kind)).collect();
        assert_eq!(kinds["concept:leave"], RetrievalDocumentKind::Concept);
        assert_eq!(kinds[FRAGMENT_ID], RetrievalDocumentKind::Evidence);
        assert_eq!(kinds["requirement:r1"], RetrievalDocumentKind::Requirement);
        assert_eq!(
            search(&index, "żółć", 5)[0].kind,
            RetrievalDocumentKind::Scenario
        );
        assert_eq!(
            search(&index, "half_up", 5)[0].kind,
            RetrievalDocumentKind::Calculation
        );
        assert_eq!(
            search(&index, "route", 5)[0].kind,
            RetrievalDocumentKind::Rule
        );
        // The Proposed Requirement and the controls never match.
        assert!(search(&index, "contractors", 5).is_empty());
        assert!(search(&index, "table aliases stanbul", 5).is_empty());
        for hit in hits {
            assert!(hit.score.is_finite() && hit.score > 0.0);
        }
    }

    // ------------------------------------------------------------------ BM25

    fn tiny() -> LexicalIndex {
        build(&graph(vec![
            requirement("requirement:a", Accepted, "leave request leave"),
            requirement(
                "requirement:b",
                Accepted,
                "Leave approval request for the manager",
            ),
            requirement("requirement:c", Accepted, "manager approval"),
            requirement("requirement:d", Accepted, "holiday calendar"),
        ]))
    }

    fn assert_golden(hits: &[RetrievalHit], expected: &[(&str, f64)]) {
        assert_eq!(hits.len(), expected.len(), "{hits:?}");
        for (hit, (expected_id, expected_score)) in hits.iter().zip(expected) {
            assert_eq!(hit.id.as_str(), *expected_id);
            assert_eq!(hit.kind, RetrievalDocumentKind::Requirement);
            assert!(
                (hit.score - expected_score).abs() <= TOLERANCE,
                "{}: {} vs {}",
                expected_id,
                hit.score,
                expected_score
            );
        }
    }

    /// Python: N = 4, avgdl = 3.25, k1 = 1.2, b = 0.75, natural-log IDF.
    #[test]
    fn bm25_matches_python_golden() {
        let index = tiny();
        assert_golden(
            &search(&index, "leave manager", 10),
            &[
                ("requirement:b", 1.0298186682604902),
                ("requirement:a", 0.9741527943004636),
                ("requirement:c", 0.8225730026562006),
            ],
        );
        assert_golden(
            &search(&index, "approval", 10),
            &[
                ("requirement:c", 0.8225730026562006),
                ("requirement:b", 0.5149093341302451),
            ],
        );
        assert_golden(
            &search(&index, "calendar request", 10),
            &[
                ("requirement:d", 1.428781004303808),
                ("requirement:a", 0.7156682080871637),
                ("requirement:b", 0.5149093341302451),
            ],
        );
    }

    #[test]
    fn bm25_formula_by_hand() {
        let index = tiny();
        let (k1, b, n, avgdl) = (1.2_f64, 0.75_f64, 4.0_f64, 3.25_f64);
        let idf = |df: f64| (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
        let term = |tf: f64, df: f64, dl: f64| {
            idf(df) * tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * dl / avgdl))
        };
        // Single term: "holiday" occurs once in d (dl 2) and nowhere else.
        let hits = search(&index, "holiday", 10);
        assert_eq!(ids(&hits), ["requirement:d"]);
        assert!((hits[0].score - term(1.0, 1.0, 2.0)).abs() <= TOLERANCE);
        // Multi term: the sum of the per-term contributions.
        let hits = search(&index, "leave manager", 10);
        let b_score = term(1.0, 2.0, 6.0) + term(1.0, 2.0, 6.0);
        assert!((hits[0].score - b_score).abs() <= TOLERANCE);
    }

    #[test]
    fn term_frequency_raises_score() {
        let index = build(&graph(vec![
            requirement("requirement:a", Accepted, "leave leave x"),
            requirement("requirement:b", Accepted, "leave y z"),
            requirement("requirement:c", Accepted, "other words here"),
        ]));
        let hits = search(&index, "leave", 10);
        assert_eq!(ids(&hits), ["requirement:a", "requirement:b"]);
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn longer_documents_are_length_normalized() {
        let index = build(&graph(vec![
            requirement(
                "requirement:a",
                Accepted,
                "leave one two three four five six",
            ),
            requirement("requirement:b", Accepted, "leave one"),
            requirement("requirement:c", Accepted, "other words"),
        ]));
        let hits = search(&index, "leave", 10);
        assert_eq!(ids(&hits), ["requirement:b", "requirement:a"]);
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn rarer_terms_have_higher_idf() {
        let index = build(&graph(vec![
            requirement("requirement:a", Accepted, "common rare"),
            requirement("requirement:b", Accepted, "common other"),
            requirement("requirement:c", Accepted, "common third"),
        ]));
        let rare = search(&index, "rare", 10);
        let common = search(&index, "common", 10);
        assert_eq!(ids(&rare), ["requirement:a"]);
        assert_eq!(common.len(), 3);
        assert!(rare[0].score > common[0].score);
    }

    #[test]
    fn repeated_query_terms_count_once() {
        let index = build(&representative());
        let once = search(&index, "leave request", 20);
        let twice = search(&index, "leave leave request", 20);
        assert!(!once.is_empty());
        assert_eq!(once, twice);
        assert_eq!(search(&index, "LEAVE Request request", 20), once);
    }

    #[test]
    fn no_stemming_and_no_stop_words() {
        let index = build(&graph(vec![
            requirement("requirement:a", Accepted, "The request was approved"),
            requirement(
                "requirement:b",
                Accepted,
                "The employee shall submit requests",
            ),
        ]));
        assert!(search(&index, "approve", 10).is_empty());
        assert!(search(&index, "approval", 10).is_empty());
        assert_eq!(ids(&search(&index, "approved", 10)), ["requirement:a"]);
        assert_eq!(ids(&search(&index, "requests", 10)), ["requirement:b"]);
        assert_eq!(ids(&search(&index, "shall", 10)), ["requirement:b"]);
        assert_eq!(
            ids(&search(&index, "the", 10)),
            ["requirement:a", "requirement:b"]
        );
    }

    #[test]
    fn equal_scores_tie_break_by_id() {
        let index = build(&graph(vec![
            requirement("requirement:b", Accepted, "leave request"),
            requirement("requirement:a", Accepted, "leave request"),
            requirement("requirement:c", Accepted, "other text"),
        ]));
        let hits = search(&index, "leave", 10);
        assert_eq!(ids(&hits), ["requirement:a", "requirement:b"]);
        assert_eq!(hits[0].score.to_bits(), hits[1].score.to_bits());
    }

    #[test]
    fn empty_corpus_returns_nothing() {
        let index = build(&graph(vec![
            requirement("requirement:p", Proposed, "leave"),
            requirement("requirement:e", Accepted, "!!"),
        ]));
        assert_eq!(index.document_count(), 0);
        for query in ["leave", "", "anything at all", "!!"] {
            assert!(search(&index, query, 10).is_empty());
        }
    }

    #[test]
    fn empty_query_zero_k_and_no_match() {
        let index = build(&representative());
        assert!(search(&index, "", 10).is_empty());
        assert!(search(&index, " -- !? … ", 10).is_empty());
        assert!(search(&index, "leave", 0).is_empty());
        assert!(search(&index, "quokka platypus", 10).is_empty());
        let before = index.clone();
        let _ = search(&index, "leave", 0);
        let _ = search(&index, "leave request", 3);
        assert_eq!(index, before);
    }

    #[test]
    fn top_k_truncates_after_sorting() {
        let index = build(&representative());
        let all = search(&index, "leave request", 100);
        assert!(all.len() >= 3, "{all:?}");
        for k in 1..=all.len() + 2 {
            let top = search(&index, "leave request", k);
            assert_eq!(top, all[..k.min(all.len())].to_vec());
        }
        for pair in all.windows(2) {
            assert!(
                pair[0].score > pair[1].score
                    || (pair[0].score == pair[1].score && pair[0].id < pair[1].id)
            );
        }
    }

    #[test]
    fn repeated_searches_are_identical() {
        let index = build(&representative());
        for query in REGRESSION_QUERIES {
            let first = search(&index, query, 10);
            for _ in 0..3 {
                assert_eq!(search(&index, query, 10), first);
            }
        }
    }

    // ------------------------------------------------------------------ hashes

    #[test]
    fn companion_hashes_are_recorded_and_distinct() {
        let g = representative();
        let index = build(&g);
        assert_eq!(index.semantic_hash(), &g.semantic_hash().unwrap());
        assert_eq!(index.evidence_hash(), &g.evidence_hash().unwrap());
        assert_eq!(index.semantic_hash().kind(), HashKind::Semantic);
        assert_eq!(index.evidence_hash().kind(), HashKind::Evidence);
        assert!(index.corpus_hash().as_str().starts_with("sha256:"));
        assert_ne!(index.corpus_hash(), index.semantic_hash());
        assert_ne!(index.corpus_hash(), index.evidence_hash());
    }

    #[test]
    fn evidence_text_change_moves_corpus_hash_but_not_evidence_hash() {
        let mut nodes = representative_nodes();
        let old = graph(nodes.clone());
        replace(
            &mut nodes,
            FRAGMENT_ID,
            fragment(Accepted, Some("Employees book vacation in hours.")),
        );
        let new = graph(nodes);
        let (a, b) = (build(&old), build(&new));
        assert_eq!(a.evidence_hash(), b.evidence_hash());
        assert_ne!(a.corpus_hash(), b.corpus_hash());
        let summary = update_equals_rebuild(&old, &new);
        assert_eq!(summary.changed, set(&[FRAGMENT_ID]));
        assert!(summary.added.is_empty() && summary.removed.is_empty());
        assert_eq!(summary.unchanged, 7);
        assert_eq!(ids(&search(&b, "vacation", 5)), [FRAGMENT_ID]);
    }

    #[test]
    fn corpus_hash_binds_project_and_profile() {
        let nodes = vec![requirement("requirement:a", Accepted, "leave")];
        let base = build(&graph(nodes.clone()));
        let other_project = build(&graph_in("project:other", PROFILE, nodes.clone()));
        let other_profile = build(&graph_in(PROJECT, "profile:other", nodes));
        assert_ne!(base.corpus_hash(), other_project.corpus_hash());
        assert_ne!(base.corpus_hash(), other_profile.corpus_hash());
    }

    // ------------------------------------------------------------------ insertion order

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn node_order_does_not_matter(
            nodes in Just(representative_nodes()).prop_shuffle()
        ) {
            let reference = build(&representative());
            let index = build(&graph(nodes));
            prop_assert_eq!(&index, &reference);
            prop_assert_eq!(index.corpus_hash().as_str(), GOLDEN_CORPUS_HASH);
            for query in REGRESSION_QUERIES {
                prop_assert_eq!(search(&index, query, 20), search(&reference, query, 20));
            }
        }
    }

    // ------------------------------------------------------------------ incremental lifecycle

    #[test]
    fn update_unchanged_graph() {
        let g = representative();
        let summary = update_equals_rebuild(&g, &g);
        assert_eq!(
            summary,
            IndexUpdateSummary {
                unchanged: 8,
                ..IndexUpdateSummary::default()
            }
        );
    }

    #[test]
    fn update_add() {
        let old = representative();
        let mut nodes = representative_nodes();
        nodes.push(requirement(
            "requirement:r5",
            Accepted,
            "Leave requests are audited.",
        ));
        let new = graph(nodes);
        let summary = update_equals_rebuild(&old, &new);
        assert_eq!(summary.added, set(&["requirement:r5"]));
        assert!(summary.changed.is_empty() && summary.removed.is_empty());
        assert_eq!(summary.unchanged, 8);
    }

    #[test]
    fn update_change_requirement_text() {
        let old = representative();
        let mut nodes = representative_nodes();
        replace(
            &mut nodes,
            "requirement:r1",
            requirement(
                "requirement:r1",
                Accepted,
                "The employee shall cancel a leave request.",
            ),
        );
        let new = graph(nodes);
        let summary = update_equals_rebuild(&old, &new);
        assert_eq!(summary.changed, set(&["requirement:r1"]));
        assert!(summary.added.is_empty() && summary.removed.is_empty());
        assert_eq!(summary.unchanged, 7);
        let index = build(&new);
        assert_eq!(ids(&search(&index, "cancel", 5)), ["requirement:r1"]);
        assert!(search(&index, "submit", 5)
            .iter()
            .all(|h| h.id.as_str() != "requirement:r1"));
    }

    #[test]
    fn update_remove() {
        let old = representative();
        let nodes: Vec<Node> = representative_nodes()
            .into_iter()
            .filter(|n| n.id.as_str() != "rule:approval")
            .collect();
        let new = graph(nodes);
        let summary = update_equals_rebuild(&old, &new);
        assert_eq!(summary.removed, set(&["rule:approval"]));
        assert!(summary.added.is_empty() && summary.changed.is_empty());
        assert_eq!(summary.unchanged, 7);
    }

    #[test]
    fn update_accepted_to_proposed_removes() {
        let old = representative();
        let mut nodes = representative_nodes();
        replace(
            &mut nodes,
            "concept:leave",
            concept(
                "concept:leave",
                Proposed,
                "Leave",
                "An approved absence from work.",
            ),
        );
        let summary = update_equals_rebuild(&old, &graph(nodes));
        assert_eq!(summary.removed, set(&["concept:leave"]));
        assert!(summary.added.is_empty() && summary.changed.is_empty());
    }

    #[test]
    fn update_proposed_to_accepted_adds() {
        let old = representative();
        let mut nodes = representative_nodes();
        replace(
            &mut nodes,
            "requirement:r3",
            requirement(
                "requirement:r3",
                Accepted,
                "Contractors shall request leave.",
            ),
        );
        let new = graph(nodes);
        let summary = update_equals_rebuild(&old, &new);
        assert_eq!(summary.added, set(&["requirement:r3"]));
        assert!(summary.removed.is_empty() && summary.changed.is_empty());
        assert_eq!(
            ids(&search(&build(&new), "contractors", 5)),
            ["requirement:r3"]
        );
    }

    #[test]
    fn update_text_becoming_token_empty_or_absent_removes() {
        let old = representative();
        let mut nodes = representative_nodes();
        replace(
            &mut nodes,
            "requirement:r2",
            requirement("requirement:r2", Accepted, "…"),
        );
        replace(&mut nodes, FRAGMENT_ID, fragment(Accepted, None));
        let summary = update_equals_rebuild(&old, &graph(nodes));
        assert_eq!(summary.removed, set(&["requirement:r2", FRAGMENT_ID]));
        assert!(summary.added.is_empty() && summary.changed.is_empty());
    }

    #[test]
    fn update_unsupported_only_change_keeps_corpus() {
        let old = representative();
        let mut nodes = representative_nodes();
        replace(
            &mut nodes,
            "entity:leave-request",
            entity("entity:leave-request", "Absence request"),
        );
        let new = graph(nodes);
        assert_ne!(old.semantic_hash().unwrap(), new.semantic_hash().unwrap());
        let summary = update_equals_rebuild(&old, &new);
        assert_eq!(
            summary,
            IndexUpdateSummary {
                unchanged: 8,
                ..IndexUpdateSummary::default()
            }
        );
        let (a, b) = (build(&old), build(&new));
        assert_eq!(a.corpus_hash(), b.corpus_hash());
        assert_ne!(a.semantic_hash(), b.semantic_hash());
        for query in REGRESSION_QUERIES {
            assert_eq!(search(&a, query, 20), search(&b, query, 20));
        }
    }

    #[test]
    fn update_envelope_only_change_keeps_corpus() {
        let old = representative();
        let mut nodes = representative_nodes();
        for n in nodes.iter_mut() {
            if n.id.as_str() == "requirement:r1" || n.id.as_str() == "scenario:s1" {
                n.revision = 7;
                n.tags.insert("zebra".into());
                n.audit = AuditMeta::new(
                    id("agent:analyst"),
                    ts(AT),
                    Some(id("agent:reviewer")),
                    Some(ts("2026-02-01T00:00:00.000000000Z")),
                )
                .unwrap();
            }
        }
        let new = graph(nodes);
        let summary = update_equals_rebuild(&old, &new);
        assert_eq!(
            summary,
            IndexUpdateSummary {
                unchanged: 8,
                ..IndexUpdateSummary::default()
            }
        );
        let (a, b) = (build(&old), build(&new));
        assert_eq!(a.corpus_hash(), b.corpus_hash());
        assert!(search(&b, "zebra", 5).is_empty());
        for query in REGRESSION_QUERIES {
            assert_eq!(search(&a, query, 20), search(&b, query, 20));
        }
    }

    #[test]
    fn update_multi_change_batch() {
        let old = representative();
        let mut nodes: Vec<Node> = representative_nodes()
            .into_iter()
            .filter(|n| n.id.as_str() != "scenario:s2")
            .collect();
        nodes.push(concept(
            "concept:manager",
            Accepted,
            "Manager",
            "Approves leave.",
        ));
        replace(
            &mut nodes,
            "calculation:balance",
            calculation("calculation:balance", Accepted, Some("hour"), None),
        );
        let summary = update_equals_rebuild(&old, &graph(nodes));
        assert_eq!(
            summary,
            IndexUpdateSummary {
                added: set(&["concept:manager"]),
                changed: set(&["calculation:balance"]),
                removed: set(&["scenario:s2"]),
                unchanged: 6,
            }
        );
    }

    #[test]
    fn update_kind_change_under_same_id_is_change() {
        let old = graph(vec![requirement("x:doc", Accepted, "leave")]);
        let new = graph(vec![concept("x:doc", Accepted, "leave", "absence")]);
        let summary = update_equals_rebuild(&old, &new);
        assert_eq!(summary.changed, set(&["x:doc"]));
        let replaced = graph(vec![entity("x:doc", "leave")]);
        let summary = update_equals_rebuild(&old, &replaced);
        assert_eq!(summary.removed, set(&["x:doc"]));
    }

    #[test]
    fn update_from_and_to_empty_corpus() {
        let empty = graph(vec![]);
        let full = representative();
        let summary = update_equals_rebuild(&empty, &full);
        assert_eq!(summary.added.len(), 8);
        let summary = update_equals_rebuild(&full, &empty);
        assert_eq!(summary.removed.len(), 8);
    }

    #[test]
    fn mismatched_project_or_profile_leaves_index_unchanged() {
        let g = representative();
        let mut index = build(&g);
        let before = index.clone();
        let hits_before = search(&index, "leave request", 10);
        let nodes = representative_nodes();
        let error = index
            .update(&graph_in("project:other", PROFILE, nodes.clone()))
            .unwrap_err();
        assert_eq!(
            error,
            IndexError::ProjectMismatch {
                expected: id(PROJECT),
                actual: id("project:other"),
            }
        );
        assert_eq!(index, before);
        let error = index
            .update(&graph_in(PROJECT, "profile:other", nodes))
            .unwrap_err();
        assert_eq!(
            error,
            IndexError::ProfileMismatch {
                expected: id(PROFILE),
                actual: id("profile:other"),
            }
        );
        assert_eq!(index, before);
        assert_eq!(index.corpus_hash(), before.corpus_hash());
        assert_eq!(search(&index, "leave request", 10), hits_before);
        // Still usable for a valid update afterwards.
        index.update(&g).unwrap();
        assert_eq!(index, before);
    }

    /// A bounded family of valid nodes: each slot is absent, Proposed or Accepted with one of
    /// three texts.
    fn slot_node(slot: usize, state: u8) -> Option<Node> {
        let texts = [
            "leave request",
            "manager approval of leave",
            "half-day unit 2",
        ];
        let status = if state >= 4 { Proposed } else { Accepted };
        let text = texts[(state % 3) as usize];
        Some(match (slot, state) {
            (_, 0) => return None,
            (0, _) => requirement("requirement:a", status, text),
            (1, _) => requirement("requirement:b", status, text),
            (2, _) => concept("concept:c", status, text, "definition"),
            (3, _) => rule("rule:d", status, text, Some("days > 0"), None),
            (4, _) => calculation("calculation:e", status, Some(text), None),
            (5, _) => scenario(
                "scenario:f",
                status,
                text,
                ScenarioKind::Acceptance,
                vec![],
                json!({"days": 2}),
                vec![],
            ),
            _ => fragment(status, Some(text)),
        })
    }

    fn slot_graph(states: &[u8]) -> Graph {
        let mut nodes = vec![source()];
        nodes.extend(
            states
                .iter()
                .enumerate()
                .filter_map(|(slot, state)| slot_node(slot, *state)),
        );
        graph(nodes)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn update_equals_rebuild_for_bounded_mutations(
            old in proptest::collection::vec(0u8..6, 7),
            new in proptest::collection::vec(0u8..6, 7),
        ) {
            let (old, new) = (slot_graph(&old), slot_graph(&new));
            let mut updated = build(&old);
            let summary = updated.update(&new).unwrap();
            let rebuilt = build(&new);
            prop_assert_eq!(&updated, &rebuilt);
            prop_assert_eq!(
                summary.added.len() + summary.changed.len() + summary.unchanged,
                rebuilt.document_count()
            );
            for query in REGRESSION_QUERIES {
                let (a, b) = (search(&updated, query, 10), search(&rebuilt, query, 10));
                prop_assert_eq!(a, b);
            }
        }
    }

    // ------------------------------------------------------------------ source guards

    fn assert_absent(source: &str, name: &str, forbidden: &[&str]) {
        for token in forbidden {
            assert!(!source.contains(token), "{name} must not contain {token:?}");
        }
    }

    #[test]
    fn source_guards() {
        let forbidden_capabilities = [
            "InferenceProvider",
            "AnthropicProvider",
            "InferenceRequest",
            "InferenceArtifact",
            "ProviderExecution",
            "ArtifactStore",
            "BlobStore",
            "rusqlite",
            "std::fs",
            "std::net",
            "reqwest",
            "tokio",
            "Clock",
            "SystemTime",
            "Instant",
            "Utc::now",
            "Embedding",
            "embedding",
            "vector",
            "cosine",
            "OpenAI",
            "Anthropic",
            "tantivy",
            "lucene",
            "rand",
            "shuffle",
            "HashMap",
            "File",
            "Path",
            "unicode_normalization",
            "regex",
        ];
        let forbidden_intake_semantics = [
            "IntakeReport",
            "requires_reason",
            "covered_by",
            "conflicts_with",
            "novelty",
            "ResolutionDecision",
            "jaccard",
            "Jaccard",
            "threshold",
        ];
        for (name, src) in [
            ("bm25.rs", BM25_SRC),
            ("index.rs", INDEX_SRC),
            ("lib.rs", LIB_SRC),
        ] {
            assert_absent(src, name, &forbidden_capabilities);
        }
        for (name, src) in [("bm25.rs", BM25_SRC), ("index.rs", INDEX_SRC)] {
            assert_absent(src, name, &forbidden_intake_semantics);
        }
        assert_absent(
            BM25_SRC,
            "bm25.rs",
            &["plumb_psg", "Graph", "NodePayload", "ElementStatus"],
        );
        assert_absent(
            INDEX_SRC,
            "index.rs",
            &[
                "SemanticPatch",
                "Proposal",
                "apply_patch",
                "DirtySet",
                "is_baseline",
            ],
        );
    }
}
