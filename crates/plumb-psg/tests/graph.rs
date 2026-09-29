//! F0.6 contract tests for `Graph`, its indexes, and the semantic/evidence hashes.
//!
//! Every test lives in `graph_contract` so that the task command
//! `cargo test -p plumb-psg graph` selects all of them.

mod graph_contract {
    use std::collections::BTreeSet;

    use plumb_core::{to_canonical_json, Hash, Id};
    use plumb_psg::*;
    use serde_json::{json, Value};

    const T1: &str = "2026-09-29T08:00:00.000000000Z";
    const T2: &str = "2026-09-30T08:00:00.000000000Z";
    const H1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const PROJECT: &str = "project:leave-management";
    const PROFILE: &str = "profile:plumb-software-2026.1";

    // Golden fixtures: literal RFC 8785 bytes and SHA-256 values computed independently
    // (Python json.dumps(sort_keys=True, separators=(",", ":")) + hashlib.sha256).
    const GOLDEN_SEMANTIC_JCS: &str = r#"{"edges":[{"evidence":[],"from":"req:HR-001","id":"rel:hr:1","kind":"constrained_by","properties":{},"standards":[],"status":"Accepted","to":"constraint:hr:eu"}],"nodes":[{"evidence":[],"extensions":{},"id":"constraint:hr:eu","payload":{"data":{"constraint_category":"regulatory","statement":"Leave data must stay in the EU.","strength":"mandatory"},"type":"Constraint"},"standards":[],"status":"Accepted","tags":[]},{"evidence":[],"extensions":{"acme:priority":2},"id":"req:HR-001","payload":{"data":{"level":"system","modality":"shall","owner_refs":null,"priority":null,"rationale":null,"requirement_kind":"functional","source_identifier":"HR-001","stakeholder_refs":null,"statement":"The system shall let an employee submit a leave request.","title":null,"verification_method":null},"type":"Requirement"},"standards":[{"clause_ref":null,"concept":"requirement","mapping_role":"semantic_alignment","mapping_strength":"compatible","standard_id":"ISO/IEC/IEEE 29148","validator_rules":["ISO29148.F1.A","ISO29148.F1.B"],"version":"2018"}],"status":"Accepted","tags":["hr"]}],"profile_id":"profile:plumb-software-2026.1","project_id":"project:leave-management"}"#;
    const GOLDEN_SEMANTIC_HASH: &str =
        "psg:sha256:175379d3099cda5734eabce1c61899b337868f8bd18964b13dfb103d3bde2b6d";
    const GOLDEN_EVIDENCE_JCS: &str = r#"{"fragments":[{"content_hash":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","id":"evd:d4432335574078c1","locator":{"data":{"end":42,"start":0},"kind":"TextRange"},"source_ref":"src:abababababababab"}],"sources":[{"content_hash":"sha256:abababababababababababababababababababababababababababababababab","id":"src:abababababababab"}]}"#;
    const GOLDEN_EVIDENCE_HASH: &str =
        "ev:sha256:fcc225b4db72674bd22afff04ddaf5378342beeba8ba88be1975cd03776fa4c8";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn node_value(node_id: &str, status: &str, tag: &str, data: Value) -> Value {
        json!({
            "id": node_id,
            "revision": 1,
            "status": status,
            "payload": {"type": tag, "data": data},
            "evidence": [],
            "derivations": [],
            "standards": [],
            "tags": [],
            "extensions": {},
            "audit": {"created_by": "actor:analyst", "created_at": T1, "updated_by": null, "updated_at": null}
        })
    }

    fn node(node_id: &str, status: &str, tag: &str, data: Value) -> Node {
        from_json(node_value(node_id, status, tag, data))
    }

    fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{e}: {value}"))
    }

    fn edge_with(
        edge_id: &str,
        status: &str,
        kind: &str,
        from: &str,
        to: &str,
        properties: Value,
    ) -> Edge {
        from_json(json!({
            "id": edge_id, "revision": 1, "status": status, "kind": kind, "from": from, "to": to,
            "properties": properties, "evidence": [], "derivations": [], "standards": [],
            "audit": {"created_by": "actor:analyst", "created_at": T1, "updated_by": null, "updated_at": null}
        }))
    }

    fn edge(edge_id: &str, status: &str, kind: &str, from: &str, to: &str) -> Edge {
        edge_with(edge_id, status, kind, from, to, json!({}))
    }

    fn requirement(node_id: &str, status: &str, statement: &str) -> Node {
        node(
            node_id,
            status,
            "Requirement",
            json!({
                "statement": statement, "requirement_kind": "functional", "level": "system", "modality": "shall"
            }),
        )
    }

    fn entity(node_id: &str, status: &str) -> Node {
        node(node_id, status, "Entity", json!({"name": "LeaveRequest"}))
    }

    fn attribute(node_id: &str, status: &str) -> Node {
        node(
            node_id,
            status,
            "Attribute",
            json!({"name": "days", "value_type": "decimal", "nullable": false}),
        )
    }

    fn operation(node_id: &str, status: &str) -> Node {
        node(
            node_id,
            status,
            "Operation",
            json!({"name": "Approve", "operation_kind": "command"}),
        )
    }

    fn constraint(node_id: &str, status: &str) -> Node {
        node(
            node_id,
            status,
            "Constraint",
            json!({
                "statement": "Leave data must stay in the EU.", "constraint_category": "regulatory", "strength": "mandatory"
            }),
        )
    }

    fn digest(hex_char: &str) -> String {
        format!("sha256:{}", hex_char.repeat(64))
    }

    /// `src:<first 16 hex of content hash>` derived independently from the graph code.
    fn src_id(content_hash: &str) -> String {
        format!(
            "src:{}",
            &content_hash["sha256:".len().."sha256:".len() + 16]
        )
    }

    /// `evd:<first 16 hex of SHA-256(source_ref | JCS(locator))>` derived independently.
    fn evd_id(source_ref: &str, locator: &Value) -> String {
        let mut bytes = format!("{source_ref}|").into_bytes();
        bytes.extend(to_canonical_json(locator).unwrap());
        let h = Hash::content_sha256(&bytes);
        format!("evd:{}", &h.as_str()["sha256:".len().."sha256:".len() + 16])
    }

    fn source(status: &str, content_hash: &str) -> Node {
        node(
            &src_id(content_hash),
            status,
            "SourceArtifact",
            json!({
                "source_kind": "markdown", "display_name": "requirements.md",
                "content_hash": content_hash, "media_type": "text/markdown"
            }),
        )
    }

    fn locator(start: u64, end: u64) -> Value {
        json!({"kind": "TextRange", "data": {"start": start, "end": end}})
    }

    fn fragment(status: &str, source_ref: &str, loc: Value, content_hash: &str) -> Node {
        node(
            &evd_id(source_ref, &loc),
            status,
            "EvidenceFragment",
            json!({
                "source_ref": source_ref, "locator": loc, "content_hash": content_hash,
                "extracted_text": "Employees shall submit leave requests."
            }),
        )
    }

    fn derivation(node_id: &str, status: &str) -> Node {
        node(
            node_id,
            status,
            "DerivationRecord",
            json!({
                "id": node_id, "kind": "parser", "stage": "S0.import", "input_refs": [], "output_refs": [], "created_at": T1
            }),
        )
    }

    fn graph(nodes: Vec<Node>, edges: Vec<Edge>) -> Result<Graph, Vec<GraphViolation>> {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges)
    }

    fn violations(nodes: Vec<Node>, edges: Vec<Edge>) -> Vec<GraphViolation> {
        graph(nodes, edges).err().unwrap_or_default()
    }

    fn semantic(nodes: Vec<Node>, edges: Vec<Edge>) -> Hash {
        graph(nodes, edges).unwrap().semantic_hash().unwrap()
    }

    fn evidence(nodes: Vec<Node>) -> Hash {
        graph(nodes, vec![]).unwrap().evidence_hash().unwrap()
    }

    fn with_evidence(mut n: Node, refs: &[&str]) -> Node {
        n.evidence = refs.iter().map(|r| EvidenceRef::from(id(r))).collect();
        n
    }

    // ------------------------------------------------------------------ construction and IDs

    #[test]
    fn duplicate_and_colliding_ids_are_rejected_without_overwriting() {
        let v = violations(
            vec![
                entity("ent:a", "Accepted"),
                entity("ent:a", "Proposed"),
                entity("x:shared", "Proposed"),
            ],
            vec![
                edge("rel:1", "Proposed", "reads", "ent:a", "ent:a"),
                edge("rel:1", "Proposed", "writes", "ent:a", "ent:a"),
                edge("x:shared", "Proposed", "reads", "ent:a", "ent:a"),
            ],
        );
        assert_eq!(
            v,
            vec![
                GraphViolation::DuplicateNodeId(id("ent:a")),
                GraphViolation::DuplicateEdgeId(id("rel:1")),
                GraphViolation::NodeEdgeIdCollision(id("x:shared")),
            ]
        );
    }

    #[test]
    fn source_and_fragment_ids_must_be_derived_exactly() {
        let hash = digest("a");
        let src = src_id(&hash);
        let good = vec![
            source("Accepted", &hash),
            fragment("Accepted", &src, locator(0, 10), &digest("b")),
        ];
        assert!(graph(good.clone(), vec![]).is_ok());

        let mut wrong_source = source("Accepted", &hash);
        wrong_source.id = id("src:0000000000000000");
        let v = violations(vec![wrong_source], vec![]);
        assert!(
            v.contains(&GraphViolation::SourceArtifactIdMismatch {
                node: id("src:0000000000000000"),
                expected: src.clone()
            }),
            "{v:?}"
        );

        let mut wrong_fragment = good[1].clone();
        wrong_fragment.id = id("evd:0000000000000000");
        let v = violations(vec![good[0].clone(), wrong_fragment], vec![]);
        assert!(v.iter().any(|x| matches!(x, GraphViolation::EvidenceFragmentIdMismatch { node, .. } if node == &id("evd:0000000000000000"))), "{v:?}");
    }

    #[test]
    fn evidence_content_hashes_must_be_generic() {
        let hash = digest("a");
        let mut src = source("Accepted", &hash);
        if let NodePayload::SourceArtifact(s) = &mut src.payload {
            s.content_hash = format!("psg:{hash}").parse().unwrap();
        }
        let v = violations(vec![src], vec![]);
        assert!(
            v.iter()
                .any(|x| matches!(x, GraphViolation::NonGenericContentHash { .. })),
            "{v:?}"
        );
    }

    // ------------------------------------------------------------------ status behavior

    fn cardinality(v: &[GraphViolation]) -> bool {
        v.iter().any(|x| {
            matches!(
                x,
                GraphViolation::Relation(RelationViolation::IncomingCardinality { .. })
            )
        })
    }

    #[test]
    fn non_baseline_attributes_do_not_need_an_owner() {
        for status in ["Proposed", "Rejected"] {
            assert!(
                graph(vec![attribute("attr:x", status)], vec![]).is_ok(),
                "{status}"
            );
        }
    }

    #[test]
    fn baseline_attributes_participate_in_cardinality() {
        for status in ["Accepted", "Suspect", "Superseded", "Deprecated"] {
            assert!(
                cardinality(&violations(vec![attribute("attr:x", status)], vec![])),
                "{status}"
            );
            let owned = graph(
                vec![entity("ent:a", status), attribute("attr:x", status)],
                vec![edge("rel:1", status, "has_attribute", "ent:a", "attr:x")],
            );
            assert!(owned.is_ok(), "{status}: {owned:?}");
        }
    }

    #[test]
    fn non_baseline_edges_do_not_count_toward_cardinality() {
        for edge_status in ["Proposed", "Rejected"] {
            let v = violations(
                vec![entity("ent:a", "Accepted"), attribute("attr:x", "Accepted")],
                vec![edge(
                    "rel:1",
                    edge_status,
                    "has_attribute",
                    "ent:a",
                    "attr:x",
                )],
            );
            assert!(cardinality(&v), "{edge_status}: {v:?}");
        }
    }

    #[test]
    fn baseline_edges_cannot_reference_non_baseline_nodes() {
        for node_status in ["Proposed", "Rejected"] {
            let v = violations(
                vec![
                    entity("ent:a", node_status),
                    attribute("attr:x", "Accepted"),
                ],
                vec![edge(
                    "rel:1",
                    "Accepted",
                    "has_attribute",
                    "ent:a",
                    "attr:x",
                )],
            );
            assert!(
                v.iter().any(|x| matches!(x, GraphViolation::BaselineEdgeEndpointNotBaseline { node, .. } if node == &id("ent:a"))),
                "{node_status}: {v:?}"
            );
        }
        // A Proposed edge may point to Proposed or baseline nodes.
        assert!(graph(
            vec![operation("op:a", "Accepted"), entity("ent:p", "Proposed")],
            vec![edge("rel:1", "Proposed", "reads", "op:a", "ent:p")],
        )
        .is_ok());
    }

    // ------------------------------------------------------------------ relation shape

    #[test]
    fn malformed_non_baseline_relations_still_fail_shape_validation() {
        for status in ["Proposed", "Rejected"] {
            let v = violations(
                vec![
                    operation("op:a", "Proposed"),
                    requirement("req:a", "Proposed", "x"),
                ],
                vec![edge("rel:1", status, "reads", "op:a", "req:a")],
            );
            assert!(
                v.iter().any(|x| matches!(
                    x,
                    GraphViolation::Relation(RelationViolation::TargetTypeNotAllowed { .. })
                )),
                "{status}: {v:?}"
            );
        }
    }

    #[test]
    fn every_edge_requires_existing_endpoints() {
        for status in ["Proposed", "Rejected", "Accepted"] {
            let v = violations(
                vec![operation("op:a", "Accepted")],
                vec![edge("rel:1", status, "reads", "op:a", "attr:missing")],
            );
            assert!(
                v.contains(&GraphViolation::Relation(
                    RelationViolation::MissingEndpoint {
                        edge: id("rel:1"),
                        node: id("attr:missing")
                    }
                )),
                "{status}: {v:?}"
            );
        }
    }

    // ------------------------------------------------------------------ evidence and provenance

    fn evidence_base(status: &str) -> (Node, Node, String) {
        let hash = digest("a");
        let src = source(status, &hash);
        let frag = fragment(status, src.id.as_str(), locator(0, 10), &digest("b"));
        let frag_id = frag.id.to_string();
        (src, frag, frag_id)
    }

    #[test]
    fn evidence_refs_resolve_to_evidence_fragments() {
        let (src, frag, frag_id) = evidence_base("Accepted");
        let req = with_evidence(requirement("req:a", "Accepted", "x"), &[&frag_id]);
        assert!(graph(vec![src.clone(), frag.clone(), req], vec![]).is_ok());

        let missing = with_evidence(
            requirement("req:a", "Accepted", "x"),
            &["evd:ffffffffffffffff"],
        );
        let v = violations(vec![src.clone(), frag.clone(), missing], vec![]);
        assert!(
            v.contains(&GraphViolation::EvidenceRefMissing {
                owner: id("req:a"),
                target: id("evd:ffffffffffffffff")
            }),
            "{v:?}"
        );

        let wrong = with_evidence(requirement("req:a", "Accepted", "x"), &["req:b"]);
        let v = violations(
            vec![src, frag, wrong, requirement("req:b", "Accepted", "y")],
            vec![],
        );
        assert!(
            v.contains(&GraphViolation::EvidenceRefWrongType {
                owner: id("req:a"),
                target: id("req:b"),
                node_type: NodeType::Requirement
            }),
            "{v:?}"
        );
    }

    #[test]
    fn derivation_refs_resolve_to_derivation_records() {
        let mut req = requirement("req:a", "Accepted", "x");
        req.derivations = vec![DerivationRef::from(id("drv:s0:1"))];
        assert!(graph(
            vec![req.clone(), derivation("drv:s0:1", "Accepted")],
            vec![]
        )
        .is_ok());
        let v = violations(vec![req.clone()], vec![]);
        assert!(
            v.contains(&GraphViolation::DerivationRefMissing {
                owner: id("req:a"),
                target: id("drv:s0:1")
            }),
            "{v:?}"
        );
        let v = violations(
            vec![req, requirement("drv:s0:1", "Accepted", "not a derivation")],
            vec![],
        );
        assert!(
            v.contains(&GraphViolation::DerivationRefWrongType {
                owner: id("req:a"),
                target: id("drv:s0:1"),
                node_type: NodeType::Requirement
            }),
            "{v:?}"
        );
    }

    #[test]
    fn edge_provenance_refs_are_resolved_too() {
        let mut e = edge(
            "rel:1",
            "Accepted",
            "constrained_by",
            "req:a",
            "constraint:a",
        );
        e.derivations = vec![DerivationRef::from(id("drv:missing"))];
        let v = violations(
            vec![
                requirement("req:a", "Accepted", "x"),
                constraint("constraint:a", "Accepted"),
            ],
            vec![e],
        );
        assert!(
            v.contains(&GraphViolation::DerivationRefMissing {
                owner: id("rel:1"),
                target: id("drv:missing")
            }),
            "{v:?}"
        );
    }

    #[test]
    fn evidence_fragment_source_must_be_an_existing_source_artifact() {
        let loc = locator(0, 10);
        let missing = fragment(
            "Accepted",
            "src:abababababababab",
            loc.clone(),
            &digest("b"),
        );
        let v = violations(vec![missing], vec![]);
        assert!(
            v.iter()
                .any(|x| matches!(x, GraphViolation::EvidenceSourceMissing { .. })),
            "{v:?}"
        );
        let wrong = fragment("Accepted", "req:a", loc, &digest("b"));
        let v = violations(vec![wrong, requirement("req:a", "Accepted", "x")], vec![]);
        assert!(
            v.iter().any(|x| matches!(
                x,
                GraphViolation::EvidenceSourceWrongType {
                    node_type: NodeType::Requirement,
                    ..
                }
            )),
            "{v:?}"
        );
    }

    #[test]
    fn baseline_provenance_cannot_point_to_non_baseline_provenance() {
        for status in ["Proposed", "Rejected"] {
            let (src, frag, frag_id) = evidence_base(status);
            let req = with_evidence(requirement("req:a", "Accepted", "x"), &[&frag_id]);
            let v = violations(vec![src.clone(), frag, req], vec![]);
            assert!(
                v.iter()
                    .any(|x| matches!(x, GraphViolation::EvidenceRefNotBaseline { .. })),
                "{status}: {v:?}"
            );

            let mut req = requirement("req:a", "Accepted", "x");
            req.derivations = vec![DerivationRef::from(id("drv:s0:1"))];
            let v = violations(vec![req, derivation("drv:s0:1", status)], vec![]);
            assert!(
                v.iter()
                    .any(|x| matches!(x, GraphViolation::DerivationRefNotBaseline { .. })),
                "{status}: {v:?}"
            );

            let accepted_fragment =
                fragment("Accepted", src.id.as_str(), locator(0, 10), &digest("b"));
            let v = violations(vec![src, accepted_fragment], vec![]);
            assert!(
                v.iter()
                    .any(|x| matches!(x, GraphViolation::EvidenceSourceNotBaseline { .. })),
                "{status}: {v:?}"
            );
        }
        // A Proposed owner may reference Proposed provenance.
        let (src, frag, frag_id) = evidence_base("Proposed");
        let req = with_evidence(requirement("req:a", "Proposed", "x"), &[&frag_id]);
        assert!(graph(vec![src, frag, req], vec![]).is_ok());
    }

    // ------------------------------------------------------------------ duplicate semantic edges

    fn duplicates(v: &[GraphViolation]) -> Vec<&GraphViolation> {
        v.iter()
            .filter(|x| matches!(x, GraphViolation::DuplicateSemanticEdge { .. }))
            .collect()
    }

    fn op_attr_nodes() -> Vec<Node> {
        vec![
            operation("op:a", "Accepted"),
            entity("ent:a", "Accepted"),
            attribute("attr:x", "Accepted"),
        ]
    }

    fn owner_edge() -> Edge {
        edge("rel:own", "Accepted", "has_attribute", "ent:a", "attr:x")
    }

    #[test]
    fn duplicate_baseline_ordinary_relations_are_rejected() {
        let v = violations(
            op_attr_nodes(),
            vec![
                owner_edge(),
                edge("rel:1", "Accepted", "reads", "op:a", "attr:x"),
                edge("rel:2", "Deprecated", "reads", "op:a", "attr:x"),
            ],
        );
        assert_eq!(
            duplicates(&v),
            vec![&GraphViolation::DuplicateSemanticEdge {
                edge: id("rel:2"),
                duplicate_of: id("rel:1")
            }]
        );
    }

    #[test]
    fn schema_for_duplicates_are_keyed_by_role() {
        let nodes = vec![
            node(
                "schema:a",
                "Accepted",
                "DataSchema",
                json!({"name": "S", "schema_kind": "json_schema"}),
            ),
            node("msg:a", "Accepted", "Message", json!({"name": "M"})),
        ];
        let same = violations(
            nodes.clone(),
            vec![
                edge_with(
                    "rel:1",
                    "Accepted",
                    "schema_for",
                    "schema:a",
                    "msg:a",
                    json!({"role": "message_payload"}),
                ),
                edge_with(
                    "rel:2",
                    "Accepted",
                    "schema_for",
                    "schema:a",
                    "msg:a",
                    json!({"role": "message_payload"}),
                ),
            ],
        );
        assert_eq!(duplicates(&same).len(), 1);
        assert!(graph(
            nodes,
            vec![
                edge_with(
                    "rel:1",
                    "Accepted",
                    "schema_for",
                    "schema:a",
                    "msg:a",
                    json!({"role": "message_payload"})
                ),
                edge_with(
                    "rel:2",
                    "Accepted",
                    "schema_for",
                    "schema:a",
                    "msg:a",
                    json!({"role": "message_headers"})
                ),
            ]
        )
        .is_ok());
    }

    #[test]
    fn extension_duplicates_are_keyed_by_canonical_properties() {
        let nodes = vec![
            requirement("req:a", "Accepted", "x"),
            requirement("req:b", "Accepted", "y"),
        ];
        let same = violations(
            nodes.clone(),
            vec![
                edge_with(
                    "rel:1",
                    "Accepted",
                    "acme:traces",
                    "req:a",
                    "req:b",
                    json!({"acme:w": 1, "acme:v": "x"}),
                ),
                edge_with(
                    "rel:2",
                    "Accepted",
                    "acme:traces",
                    "req:a",
                    "req:b",
                    json!({"acme:v": "x", "acme:w": 1}),
                ),
            ],
        );
        assert_eq!(duplicates(&same).len(), 1);
        assert!(graph(
            nodes,
            vec![
                edge_with(
                    "rel:1",
                    "Accepted",
                    "acme:traces",
                    "req:a",
                    "req:b",
                    json!({"acme:w": 1})
                ),
                edge_with(
                    "rel:2",
                    "Accepted",
                    "acme:traces",
                    "req:a",
                    "req:b",
                    json!({"acme:w": 2})
                ),
            ]
        )
        .is_ok());
    }

    #[test]
    fn proposed_duplicates_do_not_invalidate_the_baseline() {
        assert!(graph(
            op_attr_nodes(),
            vec![
                owner_edge(),
                edge("rel:1", "Accepted", "reads", "op:a", "attr:x"),
                edge("rel:2", "Proposed", "reads", "op:a", "attr:x"),
                edge("rel:3", "Rejected", "reads", "op:a", "attr:x"),
            ]
        )
        .is_ok());
    }

    // ------------------------------------------------------------------ symmetry

    #[test]
    fn conflicts_with_uses_canonical_orientation_in_graphs() {
        let nodes = vec![
            requirement("req:a", "Accepted", "x"),
            requirement("req:b", "Accepted", "y"),
        ];
        assert!(graph(
            nodes.clone(),
            vec![edge(
                "rel:1",
                "Accepted",
                "conflicts_with",
                "req:a",
                "req:b"
            )]
        )
        .is_ok());
        let mut reverse = edge("rel:1", "Accepted", "conflicts_with", "req:a", "req:b");
        reverse.from = id("req:b");
        reverse.to = id("req:a");
        let v = violations(nodes.clone(), vec![reverse]);
        assert!(
            v.iter().any(|x| matches!(
                x,
                GraphViolation::InvalidEdge {
                    error: EdgeError::NonCanonicalConflictOrientation { .. },
                    ..
                }
            )),
            "{v:?}"
        );
        let mut self_conflict = edge("rel:1", "Accepted", "conflicts_with", "req:a", "req:b");
        self_conflict.to = id("req:a");
        assert!(!violations(nodes, vec![self_conflict]).is_empty());
    }

    // ------------------------------------------------------------------ indexes

    #[test]
    fn indexes_are_exact_and_include_every_status() {
        let g = graph(
            vec![
                operation("op:a", "Accepted"),
                operation("op:p", "Proposed"),
                entity("ent:a", "Accepted"),
                entity("ent:r", "Rejected"),
                attribute("attr:x", "Accepted"),
            ],
            vec![
                owner_edge(),
                edge("rel:1", "Accepted", "reads", "op:a", "attr:x"),
                edge("rel:2", "Proposed", "writes", "op:p", "ent:a"),
                edge("rel:3", "Rejected", "reads", "op:a", "ent:r"),
            ],
        )
        .unwrap();
        let ids = |v: &[&str]| v.iter().map(|s| id(s)).collect::<BTreeSet<_>>();
        assert_eq!(
            g.node_ids_by_type(NodeType::Operation),
            &ids(&["op:a", "op:p"])
        );
        assert_eq!(
            g.node_ids_by_type(NodeType::Entity),
            &ids(&["ent:a", "ent:r"])
        );
        assert!(g.node_ids_by_type(NodeType::Requirement).is_empty());
        assert_eq!(
            g.edge_ids_by_kind(&RelationKind::Reads),
            &ids(&["rel:1", "rel:3"])
        );
        assert_eq!(g.edge_ids_by_kind(&RelationKind::Writes), &ids(&["rel:2"]));
        assert_eq!(g.outgoing_edge_ids(&id("op:a")), &ids(&["rel:1", "rel:3"]));
        assert_eq!(g.incoming_edge_ids(&id("ent:a")), &ids(&["rel:2"]));
        assert_eq!(
            g.incoming_edge_ids(&id("attr:x")),
            &ids(&["rel:1", "rel:own"])
        );
        assert!(g.outgoing_edge_ids(&id("attr:x")).is_empty());
        assert_eq!(g.project_id(), &id(PROJECT));
        assert_eq!(g.profile_id(), &id(PROFILE));
        assert!(g.node(&id("ent:r")).is_some() && g.edge(&id("rel:3")).is_some());
        assert_eq!(g.nodes().len(), 5);
        assert_eq!(g.edges().len(), 4);
        assert_eq!(g.validate(), Ok(()));
    }

    // ------------------------------------------------------------------ semantic hash

    fn golden_nodes() -> Vec<Node> {
        let mut req = node(
            "req:HR-001",
            "Accepted",
            "Requirement",
            json!({
                "statement": "The system shall let an employee submit a leave request.",
                "requirement_kind": "functional", "level": "system", "modality": "shall",
                "source_identifier": "HR-001"
            }),
        );
        req.tags.insert("hr".into());
        req.extensions
            .insert("acme:priority".parse().unwrap(), json!(2));
        req.standards = vec![from_json(json!({
            "standard_id": "ISO/IEC/IEEE 29148", "version": "2018", "concept": "requirement", "clause_ref": null,
            "mapping_role": "semantic_alignment", "mapping_strength": "compatible",
            "validator_rules": ["ISO29148.F1.B", "ISO29148.F1.A"]
        }))];
        vec![
            req,
            constraint("constraint:hr:eu", "Accepted"),
            node(
                "fnd:0123456789abcdef",
                "Accepted",
                "Finding",
                json!({
                    "code": "PLUMB.F1.X", "family": "requirements", "severity": "info", "message": "m",
                    "status": "Open", "affected_refs": ["req:HR-001"]
                }),
            ),
            requirement("req:HR-002", "Proposed", "Proposed requirement."),
        ]
    }

    fn golden_edges() -> Vec<Edge> {
        vec![edge(
            "rel:hr:1",
            "Accepted",
            "constrained_by",
            "req:HR-001",
            "constraint:hr:eu",
        )]
    }

    #[test]
    fn golden_semantic_hash_matches_fixed_bytes_and_value() {
        let g = graph(golden_nodes(), golden_edges()).unwrap();
        let bytes = to_canonical_json(&semantic_projection(&g)).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), GOLDEN_SEMANTIC_JCS);
        assert_eq!(
            Hash::semantic_sha256(GOLDEN_SEMANTIC_JCS.as_bytes()).as_str(),
            GOLDEN_SEMANTIC_HASH
        );
        assert_eq!(g.semantic_hash().unwrap().as_str(), GOLDEN_SEMANTIC_HASH);
    }

    fn base_hash() -> Hash {
        semantic(golden_nodes(), golden_edges())
    }

    fn hash_with(modify: impl FnOnce(&mut Vec<Node>, &mut Vec<Edge>)) -> Hash {
        let (mut nodes, mut edges) = (golden_nodes(), golden_edges());
        modify(&mut nodes, &mut edges);
        semantic(nodes, edges)
    }

    #[test]
    fn semantic_hash_ignores_non_semantic_changes() {
        let base = base_hash();
        let mut reversed = golden_nodes();
        reversed.reverse();
        assert_eq!(semantic(reversed, golden_edges()), base, "insertion order");
        assert_eq!(
            hash_with(|n, _| n.retain(|x| x.id.as_str() != "req:HR-002")),
            base,
            "the Proposed requirement does not contribute"
        );
        assert_eq!(
            hash_with(|n, _| n.retain(|x| x.id.as_str() != "fnd:0123456789abcdef")),
            base,
            "the Finding does not contribute"
        );
        assert_eq!(
            hash_with(|n, e| {
                n[0].audit.updated_by = Some(id("actor:reviewer"));
                n[0].audit.updated_at = Some(T2.parse().unwrap());
                e[0].audit.created_by = id("actor:other");
            }),
            base,
            "audit"
        );
        assert_eq!(
            hash_with(|n, e| {
                n[0].revision = 7;
                e[0].revision = 3;
            }),
            base,
            "revision"
        );
        assert_eq!(
            hash_with(|n, _| {
                n[0].standards[0].validator_rules.reverse();
            }),
            base,
            "validator_rules order"
        );
        assert_eq!(
            hash_with(|n, _| {
                n.push(requirement(
                    "req:HR-003",
                    "Rejected",
                    "Rejected requirement.",
                ));
            }),
            base,
            "rejected node"
        );
        assert_eq!(
            hash_with(|n, _| {
                if let NodePayload::Finding(f) = &mut n[2].payload {
                    f.message = "changed".into();
                }
            }),
            base,
            "finding content"
        );
    }

    #[test]
    fn derivation_refs_do_not_change_semantic_hash() {
        let with_drv = hash_with(|n, _| {
            n[0].derivations = vec![DerivationRef::from(id("drv:s0:1"))];
            n.push(derivation("drv:s0:1", "Accepted"));
        });
        let with_drv_only = hash_with(|n, _| n.push(derivation("drv:s0:1", "Accepted")));
        assert_eq!(with_drv, with_drv_only);
        assert_eq!(
            with_drv_only,
            base_hash(),
            "DerivationRecord nodes are excluded"
        );
    }

    #[test]
    fn evidence_and_standards_order_do_not_change_semantic_hash_but_content_does() {
        let (src, frag_a, a) = evidence_base("Accepted");
        let frag_b = fragment("Accepted", src.id.as_str(), locator(10, 20), &digest("c"));
        let b = frag_b.id.to_string();
        let order1 = hash_with(|n, _| {
            n[0] = with_evidence(n[0].clone(), &[&a, &b]);
            n.extend([src.clone(), frag_a.clone(), frag_b.clone()]);
        });
        let order2 = hash_with(|n, _| {
            n[0] = with_evidence(n[0].clone(), &[&b, &a]);
            n.extend([src.clone(), frag_a.clone(), frag_b.clone()]);
        });
        assert_eq!(order1, order2, "evidence order");
        let only_a = hash_with(|n, _| {
            n[0] = with_evidence(n[0].clone(), &[&a]);
            n.extend([src.clone(), frag_a.clone(), frag_b.clone()]);
        });
        assert_ne!(order1, only_a, "attached evidence changes the hash");
        let second: StandardMapping = from_json(json!({
            "standard_id": "ISO/IEC 25010", "version": "2023", "concept": "quality", "clause_ref": null,
            "mapping_role": "taxonomy", "mapping_strength": "compatible", "validator_rules": []
        }));
        let s1 = hash_with(|n, _| n[0].standards.push(second.clone()));
        let s2 = hash_with(|n, _| n[0].standards.insert(0, second.clone()));
        assert_eq!(s1, s2, "standards order");
        assert_ne!(s1, base_hash());
    }

    #[test]
    fn semantic_changes_change_semantic_hash() {
        let base = base_hash();
        assert_ne!(
            hash_with(|n, _| {
                if let NodePayload::Requirement(r) = &mut n[0].payload {
                    r.statement = "Changed.".into();
                }
            }),
            base,
            "requirement statement"
        );
        assert_ne!(
            hash_with(|n, _| n[0].status = ElementStatus::Suspect),
            base,
            "status"
        );
        assert_ne!(
            hash_with(|n, e| {
                n.push(constraint("constraint:hr:gdpr", "Accepted"));
                e.push(edge(
                    "rel:hr:2",
                    "Accepted",
                    "constrained_by",
                    "req:HR-001",
                    "constraint:hr:gdpr",
                ));
            }),
            hash_with(|n, _| n.push(constraint("constraint:hr:gdpr", "Accepted"))),
            "semantic edge"
        );
        assert_ne!(
            hash_with(|n, _| {
                n[0].tags.insert("x".into());
            }),
            base,
            "tags"
        );
    }

    fn view(layout: Value, style: Value, rules: Value) -> Node {
        node(
            "view:hr:1",
            "Accepted",
            "View",
            json!({
                "name": "Leave", "viewpoint_ref": "vp:hr:functional", "architecture_description_ref": "ad:hr:leave",
                "projection_rules": rules, "layout_ref": layout, "style_ref": style
            }),
        )
    }

    #[test]
    fn view_layout_and_style_refs_are_omitted_from_semantic_hash() {
        let a =
            hash_with(|n, _| n.push(view(json!(null), json!(null), json!(["include Process"]))));
        let b = hash_with(|n, _| n.push(view(json!(H1), json!(H1), json!(["include Process"]))));
        assert_eq!(a, b);
        let c = hash_with(|n, _| n.push(view(json!(null), json!(null), json!(["include Entity"]))));
        assert_ne!(a, c, "projection_rules is semantic");
        let g = graph(vec![view(json!(H1), json!(null), json!([]))], vec![]).unwrap();
        let data = &semantic_projection(&g)["nodes"][0]["payload"]["data"];
        assert!(
            data.get("layout_ref").is_none() && data.get("style_ref").is_none(),
            "{data}"
        );
        assert!(data.get("viewpoint_ref").is_some());
    }

    #[test]
    fn excluded_node_types_do_not_change_semantic_hash() {
        let base = base_hash();
        let excluded = [
            node(
                "q:0123456789abcdef",
                "Accepted",
                "Question",
                json!({"finding_ref": "fnd:0123456789abcdef", "question_kind": "YesNo", "prompt": "p", "status": "open"}),
            ),
            node(
                "run:a",
                "Accepted",
                "ScenarioRun",
                json!({"scenario_ref": "scn:a", "specification_hash": H1, "result": "pass", "trace": []}),
            ),
            node(
                "exec:a",
                "Accepted",
                "TestExecution",
                json!({"test_case_ref": "test:a", "result": "passed", "started_at": T1, "finished_at": T2}),
            ),
            node(
                "rcpt:a",
                "Accepted",
                "TestReceipt",
                json!({"verification_obligation_ref": "verify:a", "specification_hash": H1, "code_revision": "a", "test_revision": "b", "environment_ref": "env:ci", "result": "passed", "executed_at": T2}),
            ),
            node(
                "code:a",
                "Accepted",
                "CodeBinding",
                json!({"repository_ref": "repo", "code_locator": "src/a.rs", "code_revision": "a"}),
            ),
            node(
                "check:a",
                "Accepted",
                "ArchitectureCheck",
                json!({"rule_code": "R", "architecture_hash": H1, "result": "pass", "affected_refs": []}),
            ),
            node(
                "cov:a",
                "Accepted",
                "CoverageRecord",
                json!({"coverage_kind": "k", "source_refs": [], "target_refs": [], "status": "covered"}),
            ),
            node(
                "agent:a",
                "Accepted",
                "Agent",
                json!({"agent_kind": "human"}),
            ),
        ];
        for n in excluded {
            let tag = n.payload.node_type();
            assert_eq!(hash_with(|nodes, _| nodes.push(n.clone())), base, "{tag:?}");
        }
        assert_eq!(SEMANTIC_HASH_EXCLUDED_NODE_TYPES.len(), 12);
        assert!(!contributes_to_semantic_hash(NodeType::Finding));
        assert!(contributes_to_semantic_hash(NodeType::TestCase));
    }

    #[test]
    fn edge_participation_follows_endpoint_participation() {
        // bound_to_code targets an excluded CodeBinding: no change.
        let code = node(
            "code:a",
            "Accepted",
            "CodeBinding",
            json!({"repository_ref": "r", "code_locator": "l", "code_revision": "c"}),
        );
        let with_code = hash_with(|n, _| n.push(code.clone()));
        let with_binding = hash_with(|n, e| {
            n.push(code.clone());
            e.push(edge(
                "rel:bind",
                "Accepted",
                "bound_to_code",
                "req:HR-001",
                "code:a",
            ));
        });
        assert_eq!(with_code, with_binding, "bound_to_code excluded");
        // implemented_as to TestCase is semantic, to ArchitectureCheck is not.
        let vo = node(
            "verify:a",
            "Accepted",
            "VerificationObligation",
            json!({"name": "V", "verification_kind": "test"}),
        );
        let tc = node(
            "test:a",
            "Accepted",
            "TestCase",
            json!({"name": "T", "steps": [], "expected": []}),
        );
        let ac = node(
            "check:a",
            "Accepted",
            "ArchitectureCheck",
            json!({"rule_code": "R", "architecture_hash": H1, "result": "pass", "affected_refs": []}),
        );
        let nodes_only = hash_with(|n, _| n.extend([vo.clone(), tc.clone(), ac.clone()]));
        let to_test_case = hash_with(|n, e| {
            n.extend([vo.clone(), tc.clone(), ac.clone()]);
            e.push(edge(
                "rel:impl1",
                "Accepted",
                "implemented_as",
                "verify:a",
                "test:a",
            ));
        });
        let to_check = hash_with(|n, e| {
            n.extend([vo.clone(), tc.clone(), ac.clone()]);
            e.push(edge(
                "rel:impl2",
                "Accepted",
                "implemented_as",
                "verify:a",
                "check:a",
            ));
        });
        assert_ne!(
            nodes_only, to_test_case,
            "implemented_as -> TestCase included"
        );
        assert_eq!(
            nodes_only, to_check,
            "implemented_as -> ArchitectureCheck excluded"
        );
        // evidenced_by targets an excluded EvidenceFragment.
        let (src, frag, frag_id) = evidence_base("Accepted");
        let ev_nodes = hash_with(|n, _| n.extend([src.clone(), frag.clone()]));
        let ev_edge = hash_with(|n, e| {
            n.extend([src.clone(), frag.clone()]);
            e.push(edge(
                "rel:ev",
                "Accepted",
                "evidenced_by",
                "req:HR-001",
                &frag_id,
            ));
        });
        assert_eq!(ev_nodes, ev_edge, "evidenced_by excluded");
        // Proposed semantic edges do not contribute.
        assert_eq!(
            hash_with(|n, e| {
                n.push(constraint("constraint:hr:gdpr", "Accepted"));
                e.push(edge(
                    "rel:hr:2",
                    "Proposed",
                    "constrained_by",
                    "req:HR-001",
                    "constraint:hr:gdpr",
                ));
            }),
            hash_with(|n, _| n.push(constraint("constraint:hr:gdpr", "Accepted")))
        );
    }

    // ------------------------------------------------------------------ evidence hash

    fn golden_evidence_nodes() -> Vec<Node> {
        let src_hash = format!("sha256:{}", "ab".repeat(32));
        let src = source("Accepted", &src_hash);
        let frag = fragment(
            "Accepted",
            "src:abababababababab",
            locator(0, 42),
            &format!("sha256:{}", "cd".repeat(32)),
        );
        vec![src, frag]
    }

    #[test]
    fn golden_evidence_hash_matches_fixed_bytes_and_value() {
        let nodes = golden_evidence_nodes();
        assert_eq!(nodes[0].id.as_str(), "src:abababababababab");
        assert_eq!(nodes[1].id.as_str(), "evd:d4432335574078c1");
        let g = graph(nodes, vec![]).unwrap();
        let bytes = to_canonical_json(&evidence_projection_object(&g)).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), GOLDEN_EVIDENCE_JCS);
        assert_eq!(
            Hash::evidence_sha256(GOLDEN_EVIDENCE_JCS.as_bytes()).as_str(),
            GOLDEN_EVIDENCE_HASH
        );
        assert_eq!(g.evidence_hash().unwrap().as_str(), GOLDEN_EVIDENCE_HASH);
    }

    fn evidence_with(modify: impl FnOnce(&mut Vec<Node>)) -> Hash {
        let mut nodes = golden_evidence_nodes();
        modify(&mut nodes);
        evidence(nodes)
    }

    #[test]
    fn evidence_hash_ignores_non_identity_fields() {
        let base = evidence(golden_evidence_nodes());
        let mut reversed = golden_evidence_nodes();
        reversed.reverse();
        assert_eq!(evidence(reversed), base, "insertion order");
        assert_eq!(
            evidence_with(|n| {
                if let NodePayload::SourceArtifact(s) = &mut n[0].payload {
                    s.display_name = "renamed.md".into();
                    s.media_type = "text/plain".into();
                }
                if let NodePayload::EvidenceFragment(f) = &mut n[1].payload {
                    f.extracted_text = Some("changed".into());
                    f.speaker = Some("alice".into());
                    f.source_timestamp = Some(T2.parse().unwrap());
                }
                n[1].revision = 9;
                n[1].tags.insert("x".into());
            }),
            base
        );
        assert_eq!(
            evidence_with(|n| n.push(requirement("req:a", "Accepted", "x"))),
            base,
            "non-evidence nodes"
        );
    }

    #[test]
    fn evidence_identity_changes_change_evidence_hash() {
        let base = evidence(golden_evidence_nodes());
        // content_hash (fragment IDs do not include it)
        assert_ne!(
            evidence_with(|n| {
                if let NodePayload::EvidenceFragment(f) = &mut n[1].payload {
                    f.content_hash = digest("e").parse().unwrap();
                }
            }),
            base
        );
        // locator and source_ref change the (re-derived) fragment identity
        let other_locator = evidence_with(|n| {
            n[1] = fragment(
                "Accepted",
                "src:abababababababab",
                locator(0, 43),
                &format!("sha256:{}", "cd".repeat(32)),
            )
        });
        assert_ne!(other_locator, base);
        let other_source = evidence(vec![
            source("Accepted", &digest("f")),
            fragment(
                "Accepted",
                &src_id(&digest("f")),
                locator(0, 42),
                &format!("sha256:{}", "cd".repeat(32)),
            ),
        ]);
        assert_ne!(other_source, base);
    }

    #[test]
    fn only_baseline_evidence_contributes() {
        let empty = evidence(vec![]);
        for status in ["Proposed", "Rejected"] {
            let n = golden_evidence_nodes()
                .into_iter()
                .map(|mut x| {
                    x.status = status.parse_status();
                    x
                })
                .collect();
            assert_eq!(evidence(n), empty, "{status}");
        }
        let accepted = evidence(golden_evidence_nodes());
        for status in ["Suspect", "Superseded", "Deprecated"] {
            let n: Vec<Node> = golden_evidence_nodes()
                .into_iter()
                .map(|mut x| {
                    x.status = status.parse_status();
                    x
                })
                .collect();
            assert_eq!(evidence(n), accepted, "{status} contributes like Accepted");
        }
    }

    trait ParseStatus {
        fn parse_status(&self) -> ElementStatus;
    }

    impl ParseStatus for &str {
        fn parse_status(&self) -> ElementStatus {
            serde_json::from_value(json!(self)).unwrap()
        }
    }
}
