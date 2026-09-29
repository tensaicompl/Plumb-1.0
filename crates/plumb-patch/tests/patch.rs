//! F0.7 contract tests for the `SemanticPatch` AST, `apply_patch` and `GraphDelta`.
//!
//! Every test lives in `patch_contract` so that `cargo test -p plumb-patch` and name filters
//! containing `patch` select all of them.

mod patch_contract {
    use std::collections::BTreeSet;

    use plumb_core::{to_canonical_json, Hash, HashKind, Id};
    use plumb_patch::*;
    use plumb_psg::*;
    use proptest::prelude::*;
    use serde_json::{json, Value};

    const T1: &str = "2026-09-29T08:00:00.000000000Z";
    const PROJECT: &str = "project:leave-management";
    const PROFILE: &str = "profile:plumb-software-2026.1";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{e}: {value}"))
    }

    fn audit() -> Value {
        json!({"created_by": "actor:analyst", "created_at": T1, "updated_by": null, "updated_at": null})
    }

    fn node_value(node_id: &str, status: &str, tag: &str, data: Value) -> Value {
        json!({
            "id": node_id, "revision": 1, "status": status,
            "payload": {"type": tag, "data": data},
            "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
            "audit": audit()
        })
    }

    fn node(node_id: &str, status: &str, tag: &str, data: Value) -> Node {
        from_json(node_value(node_id, status, tag, data))
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
            "audit": audit()
        }))
    }

    fn edge(edge_id: &str, status: &str, kind: &str, from: &str, to: &str) -> Edge {
        edge_with(edge_id, status, kind, from, to, json!({}))
    }

    fn requirement_data(statement: &str) -> Value {
        json!({
            "statement": statement, "requirement_kind": "functional", "level": "system",
            "modality": "shall", "title": null, "rationale": null, "priority": null,
            "source_identifier": null, "verification_method": null, "owner_refs": null,
            "stakeholder_refs": null
        })
    }

    fn requirement(node_id: &str, status: &str, statement: &str) -> Node {
        node(node_id, status, "Requirement", requirement_data(statement))
    }

    fn requirement_payload(statement: &str) -> NodePayload {
        from_json(json!({"type": "Requirement", "data": requirement_data(statement)}))
    }

    fn term(node_id: &str, status: &str, word: &str) -> Node {
        node(
            node_id,
            status,
            "Term",
            json!({"term": word, "language": "en", "definition_ref": null, "aliases": null, "status": null}),
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

    fn view(node_id: &str, layout: Option<&str>) -> Node {
        node(
            node_id,
            "Proposed",
            "View",
            json!({
                "name": "Leave process", "viewpoint_ref": "vp:hr:functional",
                "architecture_description_ref": "ad:hr:leave", "root_refs": null, "filter": null,
                "projection_rules": [], "layout_ref": layout, "style_ref": null
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

    fn digest(hex_char: &str) -> String {
        format!("sha256:{}", hex_char.repeat(64))
    }

    fn src_id(content_hash: &str) -> String {
        format!(
            "src:{}",
            &content_hash["sha256:".len().."sha256:".len() + 16]
        )
    }

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

    fn fragment(status: &str, source_ref: &str, loc: Value) -> Node {
        node(
            &evd_id(source_ref, &loc),
            status,
            "EvidenceFragment",
            json!({
                "source_ref": source_ref, "locator": loc, "content_hash": digest("c"),
                "extracted_text": "Employees shall submit leave requests."
            }),
        )
    }

    fn mapping(concept: &str) -> StandardMapping {
        from_json(json!({
            "standard_id": "ISO/IEC/IEEE 29148", "version": "2018", "concept": concept,
            "clause_ref": null, "mapping_role": "semantic_alignment",
            "mapping_strength": "compatible", "validator_rules": ["ISO29148.F1.A"]
        }))
    }

    fn ext(key: &str) -> ExtensionKey {
        key.parse().unwrap()
    }

    fn graph(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn pre_node(n: &Node) -> ElementPrecondition {
        ElementPrecondition {
            id: n.id.clone(),
            expected_hash: node_element_hash(n).unwrap(),
        }
    }

    fn pre_edge(e: &Edge) -> ElementPrecondition {
        ElementPrecondition {
            id: e.id.clone(),
            expected_hash: edge_element_hash(e).unwrap(),
        }
    }

    fn pre_n(g: &Graph, node_id: &str) -> ElementPrecondition {
        pre_node(g.node(&id(node_id)).unwrap())
    }

    fn pre_e(g: &Graph, edge_id: &str) -> ElementPrecondition {
        pre_edge(g.edge(&id(edge_id)).unwrap())
    }

    fn patch_set(base: &Graph, patch: SemanticPatch) -> PatchSet {
        PatchSet {
            base_semantic_hash: base.semantic_hash().unwrap(),
            patch,
        }
    }

    fn apply(base: &Graph, patch: SemanticPatch) -> Result<ApplyResult, PatchError> {
        apply_patch(base, &patch_set(base, patch))
    }

    /// Applies and asserts failure, checking that the base graph is unchanged.
    fn apply_err(base: &Graph, patch: SemanticPatch) -> PatchError {
        let before = base.clone();
        let err = apply(base, patch).expect_err("patch must fail");
        assert_eq!(base, &before);
        err
    }

    fn apply_ok(base: &Graph, patch: SemanticPatch) -> ApplyResult {
        apply(base, patch).unwrap_or_else(|e| panic!("{e:?}"))
    }

    fn compound(patches: Vec<SemanticPatch>) -> SemanticPatch {
        SemanticPatch::Compound { patches }
    }

    fn set_status(pre: ElementPrecondition, from: &str, to: &str) -> SemanticPatch {
        SemanticPatch::SetStatus {
            target: pre,
            from: from_json(json!(from)),
            to: from_json(json!(to)),
        }
    }

    fn with_status(n: &Node, status: ElementStatus) -> Node {
        let mut n = n.clone();
        n.status = status;
        n
    }

    fn ids(list: &[&str]) -> BTreeSet<Id> {
        list.iter().map(|s| id(s)).collect()
    }

    fn final_violations(err: PatchError) -> Vec<GraphViolation> {
        match err {
            PatchError::FinalGraphInvalid(v) => v,
            other => panic!("expected FinalGraphInvalid, got {other:?}"),
        }
    }

    fn base_requirements() -> Graph {
        graph(
            vec![
                requirement(
                    "req:a",
                    "Accepted",
                    "Employees shall submit leave requests.",
                ),
                requirement(
                    "req:b",
                    "Proposed",
                    "Managers shall approve leave requests.",
                ),
                requirement("req:r", "Rejected", "The system shall fax requests."),
                constraint("con:eu", "Accepted"),
            ],
            vec![
                edge("rel:a-eu", "Accepted", "constrained_by", "req:a", "con:eu"),
                edge("rel:b-eu", "Proposed", "constrained_by", "req:b", "con:eu"),
            ],
        )
    }

    // ------------------------------------------------------------------ wire contract (§39)

    const BASE_H: &str =
        "psg:sha256:175379d3099cda5734eabce1c61899b337868f8bd18964b13dfb103d3bde2b6d";
    const EL_H1: &str = "sha256:0b9c65577e297b0c98959ca2902712f39c7dae2885b8ecf9dfaf4a856943b2ca";
    const EL_H2: &str = "sha256:0e45b678a1d77504e09cefe40f3d1fc64cde2dd76274e33bd06ccabd739200e2";

    fn wire_pre(element_id: &str, hash: &str) -> Value {
        json!({"id": element_id, "expected_hash": hash})
    }

    fn wire_node() -> Value {
        json!({
            "id": "req:HR-001", "revision": 1, "status": "Proposed",
            "payload": {"type": "Requirement", "data": {
                "statement": "The system shall let an employee submit a leave request.",
                "requirement_kind": "functional", "level": "system", "modality": "shall",
                "title": null, "rationale": null, "priority": null, "source_identifier": "HR-001",
                "verification_method": null, "owner_refs": null, "stakeholder_refs": null
            }},
            "evidence": [], "derivations": [], "standards": [], "tags": ["hr"],
            "extensions": {"acme:priority": 2},
            "audit": {"created_by": "actor:analyst", "created_at": "2026-09-29T08:00:00.000000000Z", "updated_by": null, "updated_at": null}
        })
    }

    fn wire_edge(kind: &str, from: &str, to: &str, status: &str) -> Value {
        json!({
            "id": "rel:hr:1", "revision": 1, "status": status, "kind": kind, "from": from, "to": to,
            "properties": {}, "evidence": [], "derivations": [], "standards": [],
            "audit": {"created_by": "actor:analyst", "created_at": "2026-09-29T08:00:00.000000000Z", "updated_by": null, "updated_at": null}
        })
    }

    fn wire_mapping() -> Value {
        json!({
            "standard_id": "ISO/IEC/IEEE 29148", "version": "2018", "concept": "requirement",
            "clause_ref": "5.2.5", "mapping_role": "semantic_alignment",
            "mapping_strength": "compatible", "validator_rules": ["ISO29148.F1.A"]
        })
    }

    /// Hand-authored wire fixtures, one per variant, in the exact serialized form.
    fn variant_fixtures() -> Vec<(&'static str, Value)> {
        vec![
            ("AddNode", json!({"op": "AddNode", "node": wire_node()})),
            (
                "RemoveNode",
                json!({"op": "RemoveNode", "target": wire_pre("req:HR-001", EL_H1)}),
            ),
            (
                "ReplacePayload",
                json!({"op": "ReplacePayload", "target": wire_pre("req:HR-001", EL_H1),
                       "payload": wire_node()["payload"].clone()}),
            ),
            (
                "SetStatus",
                json!({"op": "SetStatus", "target": wire_pre("req:HR-001", EL_H1),
                       "from": "Proposed", "to": "Accepted"}),
            ),
            (
                "AddEdge",
                json!({"op": "AddEdge", "edge": wire_edge("constrained_by", "req:HR-001", "constraint:hr:eu", "Proposed")}),
            ),
            (
                "ReplaceEdge",
                json!({"op": "ReplaceEdge", "target": wire_pre("rel:hr:1", EL_H2),
                       "kind": "schema_for", "from": "schema:leave", "to": "attr:days",
                       "properties": {"role": "attribute"}}),
            ),
            (
                "RemoveEdge",
                json!({"op": "RemoveEdge", "target": wire_pre("rel:hr:1", EL_H2)}),
            ),
            (
                "MergeNodes",
                json!({"op": "MergeNodes", "keep": wire_pre("req:HR-001", EL_H1),
                       "merge": [wire_pre("req:HR-002", EL_H2)],
                       "field_policy": "keep_payload_union_metadata"}),
            ),
            (
                "Supersede",
                json!({"op": "Supersede", "old": wire_pre("req:HR-001", EL_H1),
                       "new": wire_pre("req:HR-002", EL_H2),
                       "edge": wire_edge("supersedes", "req:HR-002", "req:HR-001", "Accepted")}),
            ),
            (
                "AttachEvidence",
                json!({"op": "AttachEvidence", "target": wire_pre("req:HR-001", EL_H1),
                       "evidence": "evd:0123456789abcdef"}),
            ),
            (
                "AttachStandardMapping",
                json!({"op": "AttachStandardMapping", "target": wire_pre("rel:hr:1", EL_H2),
                       "mapping": wire_mapping()}),
            ),
            (
                "Compound",
                json!({"op": "Compound", "patches": [
                    {"op": "RemoveEdge", "target": wire_pre("rel:hr:1", EL_H2)},
                    {"op": "Compound", "patches": [
                        {"op": "RemoveNode", "target": wire_pre("req:HR-001", EL_H1)}
                    ]}
                ]}),
            ),
        ]
    }

    /// Exhaustive (no wildcard): adding or removing a variant breaks compilation.
    fn op_name(patch: &SemanticPatch) -> &'static str {
        match patch {
            SemanticPatch::AddNode { .. } => "AddNode",
            SemanticPatch::RemoveNode { .. } => "RemoveNode",
            SemanticPatch::ReplacePayload { .. } => "ReplacePayload",
            SemanticPatch::SetStatus { .. } => "SetStatus",
            SemanticPatch::AddEdge { .. } => "AddEdge",
            SemanticPatch::ReplaceEdge { .. } => "ReplaceEdge",
            SemanticPatch::RemoveEdge { .. } => "RemoveEdge",
            SemanticPatch::MergeNodes { .. } => "MergeNodes",
            SemanticPatch::Supersede { .. } => "Supersede",
            SemanticPatch::AttachEvidence { .. } => "AttachEvidence",
            SemanticPatch::AttachStandardMapping { .. } => "AttachStandardMapping",
            SemanticPatch::Compound { .. } => "Compound",
        }
    }

    #[test]
    fn all_twelve_variants_round_trip_through_fixed_fixtures() {
        let fixtures = variant_fixtures();
        assert_eq!(fixtures.len(), 12);
        let names: BTreeSet<&str> = fixtures.iter().map(|(n, _)| *n).collect();
        assert_eq!(names.len(), 12);
        for (name, wire) in fixtures {
            let patch: SemanticPatch = from_json(wire.clone());
            assert_eq!(op_name(&patch), name);
            assert_eq!(wire["op"], json!(name));
            assert_eq!(serde_json::to_value(&patch).unwrap(), wire, "{name}");
            let reparsed: SemanticPatch =
                serde_json::from_slice(&serde_json::to_vec(&patch).unwrap()).unwrap();
            assert_eq!(reparsed, patch, "{name}");
        }
    }

    #[test]
    fn typed_fields_of_wire_fixtures_are_exact() {
        let fixtures = variant_fixtures();
        let replace: SemanticPatch = from_json(fixtures[5].1.clone());
        assert_eq!(
            replace,
            SemanticPatch::ReplaceEdge {
                target: ElementPrecondition {
                    id: id("rel:hr:1"),
                    expected_hash: EL_H2.parse().unwrap()
                },
                kind: RelationKind::SchemaFor,
                from: id("schema:leave"),
                to: id("attr:days"),
                properties: RelationProperties::SchemaFor(SchemaForProperties {
                    role: SchemaBindingRole::Attribute
                }),
            }
        );
        let merge: SemanticPatch = from_json(fixtures[7].1.clone());
        assert!(matches!(
            merge,
            SemanticPatch::MergeNodes {
                field_policy: MergePolicy::KeepPayloadUnionMetadata,
                ..
            }
        ));
    }

    #[test]
    fn patch_set_precondition_and_policy_round_trip() {
        let wire = json!({
            "base_semantic_hash": BASE_H,
            "patch": {"op": "RemoveNode", "target": wire_pre("req:HR-001", EL_H1)}
        });
        let set: PatchSet = from_json(wire.clone());
        assert_eq!(set.base_semantic_hash.kind(), HashKind::Semantic);
        assert_eq!(set.base_semantic_hash.as_str(), BASE_H);
        assert_eq!(serde_json::to_value(&set).unwrap(), wire);

        let pre_wire = wire_pre("req:HR-001", EL_H1);
        let pre: ElementPrecondition = from_json(pre_wire.clone());
        assert_eq!(
            pre,
            ElementPrecondition {
                id: id("req:HR-001"),
                expected_hash: EL_H1.parse().unwrap()
            }
        );
        assert_eq!(serde_json::to_value(&pre).unwrap(), pre_wire);

        let policy: MergePolicy = from_json(json!("keep_payload_union_metadata"));
        assert_eq!(policy, MergePolicy::KeepPayloadUnionMetadata);
        assert_eq!(
            serde_json::to_string(&policy).unwrap(),
            r#""keep_payload_union_metadata""#
        );
    }

    #[test]
    fn unknown_ops_fields_and_policies_are_rejected() {
        let target = wire_pre("req:HR-001", EL_H1);
        let rejected = [
            json!({"op": "Frobnicate", "target": target}),
            json!({"op": "Generic", "path": "/payload", "value": 1}),
            json!({"op": "removeNode", "target": target}),
            json!({"target": target}),
            json!({"op": "RemoveNode", "target": target, "extra": 1}),
            json!({"op": "RemoveNode", "target": {"id": "req:HR-001", "expected_hash": EL_H1, "extra": 1}}),
            json!({"op": "RemoveNode", "target": {"id": "req:HR-001"}}),
            json!({"op": "MergeNodes", "keep": target, "merge": [], "field_policy": "keep_all"}),
            json!({"op": "ReplaceEdge", "target": target, "kind": "constrained_by", "from": "req:a",
                   "to": "con:b", "properties": {"role": "attribute"}}),
            json!({"op": "ReplaceEdge", "target": target, "kind": "schema_for", "from": "schema:a",
                   "to": "attr:b", "properties": {}}),
            json!({"op": "Compound", "patches": [], "extra": true}),
        ];
        for wire in rejected {
            assert!(
                serde_json::from_value::<SemanticPatch>(wire.clone()).is_err(),
                "{wire}"
            );
        }
        let extra_set = json!({"base_semantic_hash": BASE_H, "patch": {"op": "RemoveNode", "target": target}, "x": 1});
        assert!(serde_json::from_value::<PatchSet>(extra_set).is_err());
        assert!(serde_json::from_value::<MergePolicy>(json!("KeepPayloadUnionMetadata")).is_err());
    }

    #[test]
    fn wrong_hash_kinds_are_rejected_on_the_wire() {
        let generic_base = json!({"base_semantic_hash": EL_H1, "patch": {"op": "RemoveNode", "target": wire_pre("req:HR-001", EL_H1)}});
        assert!(serde_json::from_value::<PatchSet>(generic_base).is_err());
        let evidence_base = json!({"base_semantic_hash": format!("ev:{EL_H1}"), "patch": {"op": "RemoveNode", "target": wire_pre("req:HR-001", EL_H1)}});
        assert!(serde_json::from_value::<PatchSet>(evidence_base).is_err());
        for bad in [BASE_H.to_string(), format!("ev:{EL_H1}")] {
            assert!(
                serde_json::from_value::<ElementPrecondition>(wire_pre("req:HR-001", &bad))
                    .is_err()
            );
            let patch = json!({"op": "RemoveNode", "target": wire_pre("req:HR-001", &bad)});
            assert!(serde_json::from_value::<SemanticPatch>(patch).is_err());
        }
    }

    // ------------------------------------------------------------------ CAS / preconditions (§40)

    #[test]
    fn correct_base_is_accepted_and_stale_base_rejected() {
        let base = base_requirements();
        let result = apply_ok(
            &base,
            set_status(pre_n(&base, "req:b"), "Proposed", "Accepted"),
        );
        assert_eq!(
            result.delta.base_semantic_hash,
            base.semantic_hash().unwrap()
        );

        let stale: Hash = BASE_H.parse().unwrap();
        let set = PatchSet {
            base_semantic_hash: stale.clone(),
            patch: set_status(pre_n(&base, "req:b"), "Proposed", "Accepted"),
        };
        let before = base.clone();
        assert_eq!(
            apply_patch(&base, &set),
            Err(PatchError::StaleBase {
                expected: stale,
                actual: base.semantic_hash().unwrap()
            })
        );
        assert_eq!(base, before);

        // Programmatically built wrong kinds are rejected by apply as well.
        let set = PatchSet {
            base_semantic_hash: EL_H1.parse().unwrap(),
            patch: set_status(pre_n(&base, "req:b"), "Proposed", "Accepted"),
        };
        assert_eq!(
            apply_patch(&base, &set),
            Err(PatchError::InvalidBaseHashKind(HashKind::Generic))
        );
        let mut pre = pre_n(&base, "req:b");
        pre.expected_hash = BASE_H.parse().unwrap();
        assert_eq!(
            apply_err(&base, set_status(pre, "Proposed", "Accepted")),
            PatchError::InvalidExpectedHashKind {
                id: id("req:b"),
                kind: HashKind::Semantic
            }
        );
    }

    #[test]
    fn element_hash_mismatch_is_rejected_for_nodes_and_edges() {
        let base = base_requirements();
        let mut pre = pre_n(&base, "req:b");
        pre.expected_hash = EL_H1.parse().unwrap();
        assert!(matches!(
            apply_err(&base, set_status(pre, "Proposed", "Accepted")),
            PatchError::ElementHashMismatch { id: i, .. } if i == id("req:b")
        ));
        let mut pre = pre_e(&base, "rel:b-eu");
        pre.expected_hash = EL_H1.parse().unwrap();
        assert!(matches!(
            apply_err(&base, SemanticPatch::RemoveEdge { target: pre }),
            PatchError::ElementHashMismatch { id: i, .. } if i == id("rel:b-eu")
        ));
        let missing = ElementPrecondition {
            id: id("req:missing"),
            expected_hash: EL_H1.parse().unwrap(),
        };
        assert_eq!(
            apply_err(&base, SemanticPatch::RemoveNode { target: missing }),
            PatchError::ElementNotFound(id("req:missing"))
        );
        assert_eq!(
            apply_err(
                &base,
                SemanticPatch::RemoveNode {
                    target: pre_e(&base, "rel:b-eu")
                }
            ),
            PatchError::ExpectedNode(id("rel:b-eu"))
        );
        assert_eq!(
            apply_err(
                &base,
                SemanticPatch::RemoveEdge {
                    target: pre_n(&base, "req:b")
                }
            ),
            PatchError::ExpectedEdge(id("req:b"))
        );
    }

    #[test]
    fn proposed_and_rejected_element_hashes_work() {
        let base = base_requirements();
        let result = apply_ok(
            &base,
            compound(vec![
                set_status(pre_n(&base, "req:r"), "Rejected", "Proposed"),
                set_status(pre_e(&base, "rel:b-eu"), "Proposed", "Rejected"),
            ]),
        );
        assert_eq!(
            result.graph.node(&id("req:r")).unwrap().status,
            ElementStatus::Proposed
        );
        assert_eq!(
            result.graph.edge(&id("rel:b-eu")).unwrap().status,
            ElementStatus::Rejected
        );
        // Proposed/Rejected elements are outside semantic_hash but inside element hashes.
        assert_eq!(
            result.delta.result_semantic_hash,
            base.semantic_hash().unwrap()
        );
    }

    #[test]
    fn later_child_sees_state_of_earlier_child() {
        let base = base_requirements();
        let b = base.node(&id("req:b")).unwrap();
        let accepted = with_status(b, ElementStatus::Accepted);
        let result = apply_ok(
            &base,
            compound(vec![
                set_status(pre_n(&base, "req:b"), "Proposed", "Accepted"),
                SemanticPatch::AttachStandardMapping {
                    target: pre_node(&accepted),
                    mapping: mapping("requirement"),
                },
            ]),
        );
        let final_b = result.graph.node(&id("req:b")).unwrap();
        assert_eq!(final_b.standards, vec![mapping("requirement")]);
        assert_eq!(final_b.revision, 2);

        // The base hash no longer describes the element after the first child.
        let err = apply_err(
            &base,
            compound(vec![
                set_status(pre_n(&base, "req:b"), "Proposed", "Accepted"),
                SemanticPatch::AttachStandardMapping {
                    target: pre_n(&base, "req:b"),
                    mapping: mapping("requirement"),
                },
            ]),
        );
        assert_eq!(
            err,
            PatchError::ElementHashMismatch {
                id: id("req:b"),
                expected: node_element_hash(b).unwrap(),
                actual: node_element_hash(&accepted).unwrap(),
            }
        );
    }

    #[test]
    fn set_status_requires_matching_from_and_a_change() {
        let base = base_requirements();
        assert_eq!(
            apply_err(
                &base,
                set_status(pre_n(&base, "req:b"), "Accepted", "Rejected")
            ),
            PatchError::StatusMismatch {
                id: id("req:b"),
                expected: ElementStatus::Accepted,
                actual: ElementStatus::Proposed
            }
        );
        assert_eq!(
            apply_err(
                &base,
                set_status(pre_n(&base, "req:b"), "Proposed", "Proposed")
            ),
            PatchError::StatusNoOp(id("req:b"))
        );
        // Status change never creates relations; illegal baseline participation fails finally.
        let err = apply_err(
            &base,
            set_status(pre_e(&base, "rel:b-eu"), "Proposed", "Accepted"),
        );
        assert!(final_violations(err)
            .iter()
            .any(|v| matches!(v, GraphViolation::BaselineEdgeEndpointNotBaseline { .. })));
    }

    // ------------------------------------------------------------------ atomic Compound (§41)

    fn entity_base() -> Graph {
        graph(vec![entity("ent:leave", "Accepted")], vec![])
    }

    #[test]
    fn add_attribute_alone_fails_final_validation() {
        let base = entity_base();
        let err = apply_err(
            &base,
            SemanticPatch::AddNode {
                node: attribute("attr:days", "Accepted"),
            },
        );
        assert!(final_violations(err).iter().any(|v| matches!(
            v,
            GraphViolation::Relation(RelationViolation::IncomingCardinality { node, .. }) if node == &id("attr:days")
        )));
    }

    #[test]
    fn attribute_with_has_attribute_succeeds_in_either_order() {
        let base = entity_base();
        let add_attr = SemanticPatch::AddNode {
            node: attribute("attr:days", "Accepted"),
        };
        let add_edge = SemanticPatch::AddEdge {
            edge: edge(
                "rel:has-days",
                "Accepted",
                "has_attribute",
                "ent:leave",
                "attr:days",
            ),
        };
        let forward = apply_ok(&base, compound(vec![add_attr.clone(), add_edge.clone()]));
        let backward = apply_ok(&base, compound(vec![add_edge, add_attr]));
        assert_eq!(forward.graph, backward.graph);
        assert_eq!(forward.delta.added_nodes, ids(&["attr:days"]));
        assert_eq!(forward.delta.added_edges, ids(&["rel:has-days"]));
        assert_ne!(
            forward.delta.result_semantic_hash,
            forward.delta.base_semantic_hash
        );
    }

    #[test]
    fn child_failure_and_final_failure_return_no_candidate() {
        let base = entity_base();
        // A later child fails: no partial result from the successful first child.
        let err = apply_err(
            &base,
            compound(vec![
                SemanticPatch::AddNode {
                    node: attribute("attr:days", "Proposed"),
                },
                SemanticPatch::AddNode {
                    node: attribute("attr:days", "Proposed"),
                },
            ]),
        );
        assert_eq!(err, PatchError::IdAlreadyExists(id("attr:days")));
        // Final failure: edge to a missing endpoint.
        let err = apply_err(
            &base,
            SemanticPatch::AddEdge {
                edge: edge(
                    "rel:x",
                    "Proposed",
                    "has_attribute",
                    "ent:leave",
                    "attr:none",
                ),
            },
        );
        assert!(final_violations(err).iter().any(|v| matches!(
            v,
            GraphViolation::Relation(RelationViolation::MissingEndpoint { .. })
        )));
        assert_eq!(
            apply_err(&base, compound(vec![])),
            PatchError::EmptyCompound
        );
        assert_eq!(
            apply_err(
                &base,
                compound(vec![
                    SemanticPatch::AddNode {
                        node: attribute("attr:days", "Proposed")
                    },
                    compound(vec![])
                ])
            ),
            PatchError::EmptyCompound
        );
    }

    #[test]
    fn nested_compound_uses_depth_first_leaf_order() {
        let base = graph(vec![], vec![]);
        let add = |n: &str| SemanticPatch::AddNode {
            node: requirement(n, "Proposed", "The system shall log."),
        };
        let result = apply_ok(
            &base,
            compound(vec![
                add("req:3"),
                compound(vec![add("req:1"), compound(vec![add("req:4")])]),
                add("req:2"),
            ]),
        );
        let ordinals: Vec<(String, u32)> = result
            .delta
            .diff
            .iter()
            .map(|d| (d.id.to_string(), d.operation_ordinal))
            .collect();
        assert_eq!(
            ordinals,
            vec![
                ("req:1".to_string(), 1),
                ("req:2".to_string(), 3),
                ("req:3".to_string(), 0),
                ("req:4".to_string(), 2),
            ]
        );
    }

    // ------------------------------------------------------------------ revisions (§42)

    #[test]
    fn new_elements_must_have_revision_one() {
        let base = entity_base();
        let mut n = attribute("attr:days", "Proposed");
        n.revision = 2;
        assert_eq!(
            apply_err(&base, SemanticPatch::AddNode { node: n }),
            PatchError::NewElementRevisionNotOne {
                id: id("attr:days"),
                revision: 2
            }
        );
        let mut e = edge(
            "rel:x",
            "Proposed",
            "has_attribute",
            "ent:leave",
            "attr:days",
        );
        e.revision = 3;
        assert_eq!(
            apply_err(&base, SemanticPatch::AddEdge { edge: e }),
            PatchError::NewElementRevisionNotOne {
                id: id("rel:x"),
                revision: 3
            }
        );
    }

    #[test]
    fn revisions_follow_the_element_hash_rule() {
        let base = base_requirements();
        let b = base.node(&id("req:b")).unwrap().clone();
        let accepted = with_status(&b, ElementStatus::Accepted);

        let one = apply_ok(&base, set_status(pre_node(&b), "Proposed", "Accepted"));
        assert_eq!(one.graph.node(&id("req:b")).unwrap().revision, 2);

        let two = apply_ok(
            &base,
            compound(vec![
                set_status(pre_node(&b), "Proposed", "Accepted"),
                SemanticPatch::AttachStandardMapping {
                    target: pre_node(&accepted),
                    mapping: mapping("requirement"),
                },
            ]),
        );
        assert_eq!(two.graph.node(&id("req:b")).unwrap().revision, 2);

        let reversal = apply_ok(
            &base,
            compound(vec![
                set_status(pre_node(&b), "Proposed", "Rejected"),
                set_status(
                    pre_node(&with_status(&b, ElementStatus::Rejected)),
                    "Rejected",
                    "Proposed",
                ),
            ]),
        );
        assert_eq!(reversal.graph.node(&id("req:b")).unwrap(), &b);
        assert_eq!(reversal.graph, base);

        let added = requirement("req:new", "Proposed", "The system shall export.");
        let added_then_modified = apply_ok(
            &base,
            compound(vec![
                SemanticPatch::AddNode {
                    node: added.clone(),
                },
                set_status(pre_node(&added), "Proposed", "Rejected"),
            ]),
        );
        let n = added_then_modified.graph.node(&id("req:new")).unwrap();
        assert_eq!((n.revision, n.status), (1, ElementStatus::Rejected));
    }

    #[test]
    fn view_layout_only_change_keeps_revision() {
        let base = graph(vec![view("view:leave", Some(&digest("4")))], vec![]);
        let v = base.node(&id("view:leave")).unwrap();
        let mut payload = v.payload.clone();
        if let NodePayload::View(data) = &mut payload {
            data.layout_ref = Some(digest("5").parse().unwrap());
            data.style_ref = Some(digest("6").parse().unwrap());
        }
        let result = apply_ok(
            &base,
            SemanticPatch::ReplacePayload {
                target: pre_node(v),
                payload: payload.clone(),
            },
        );
        let after = result.graph.node(&id("view:leave")).unwrap();
        assert_eq!(after.revision, 1);
        assert_eq!(after.payload, payload);
        // Persisted value changed, element hash did not.
        assert_eq!(result.delta.modified_nodes, ids(&["view:leave"]));
        let entry = &result.delta.diff[0];
        assert_eq!(entry.change, DiffChange::Modified);
        assert_eq!(entry.before_hash, entry.after_hash);
    }

    #[test]
    fn change_at_revision_max_fails_the_patch_set() {
        let mut b = requirement("req:max", "Proposed", "The system shall count.");
        b.revision = u32::MAX;
        let base = graph(vec![b.clone()], vec![]);
        assert_eq!(
            apply_err(&base, set_status(pre_node(&b), "Proposed", "Rejected")),
            PatchError::RevisionOverflow(id("req:max"))
        );
        // An unchanged element at u32::MAX is fine.
        let result = apply_ok(
            &base,
            SemanticPatch::AddNode {
                node: requirement("req:other", "Proposed", "The system shall sum."),
            },
        );
        assert_eq!(
            result.graph.node(&id("req:max")).unwrap().revision,
            u32::MAX
        );
    }

    #[test]
    fn derivation_only_change_keeps_revision() {
        // Derivations are excluded from the element hash: merging in a derivation-only
        // difference is a persisted modification without a revision increment.
        let mut keep = requirement("req:keep", "Accepted", "The system shall store.");
        keep.derivations = vec![DerivationRef::from(id("drv:1"))];
        let mut merged = requirement("req:dup", "Accepted", "The system shall store.");
        merged.derivations = vec![DerivationRef::from(id("drv:2"))];
        let base = graph(
            vec![
                keep.clone(),
                merged.clone(),
                derivation("drv:1", "Accepted"),
                derivation("drv:2", "Accepted"),
            ],
            vec![],
        );
        let result = apply_ok(
            &base,
            SemanticPatch::MergeNodes {
                keep: pre_node(&keep),
                merge: vec![pre_node(&merged)],
                field_policy: MergePolicy::KeepPayloadUnionMetadata,
            },
        );
        let k = result.graph.node(&id("req:keep")).unwrap();
        assert_eq!(k.revision, 1);
        assert_eq!(k.derivations.len(), 2);
        assert!(result.delta.modified_nodes.contains(&id("req:keep")));
        let entry = result
            .delta
            .diff
            .iter()
            .find(|d| d.id == id("req:keep"))
            .unwrap();
        assert_eq!(entry.before_hash, entry.after_hash);
    }

    // ------------------------------------------------------------------ ReplacePayload (§43)

    #[test]
    fn replace_payload_same_type_succeeds_and_type_change_fails() {
        let base = base_requirements();
        let payload = requirement_payload("Managers shall approve or reject leave requests.");
        let result = apply_ok(
            &base,
            SemanticPatch::ReplacePayload {
                target: pre_n(&base, "req:b"),
                payload: payload.clone(),
            },
        );
        let b = result.graph.node(&id("req:b")).unwrap();
        let old = base.node(&id("req:b")).unwrap();
        assert_eq!(b.payload, payload);
        assert_eq!(
            (&b.id, b.status, &b.audit, &b.tags, b.revision),
            (&old.id, old.status, &old.audit, &old.tags, 2)
        );
        assert_eq!(
            apply_err(
                &base,
                SemanticPatch::ReplacePayload {
                    target: pre_n(&base, "req:b"),
                    payload: constraint("con:x", "Proposed").payload,
                }
            ),
            PatchError::NodeTypeChange {
                id: id("req:b"),
                from: NodeType::Requirement,
                to: NodeType::Constraint
            }
        );
    }

    fn evidence_base() -> (Graph, String, String) {
        let hash = digest("a");
        let src = src_id(&hash);
        let frag = fragment("Accepted", &src, locator(0, 10));
        let frag_id = frag.id.to_string();
        (
            graph(vec![source("Accepted", &hash), frag], vec![]),
            src,
            frag_id,
        )
    }

    #[test]
    fn replace_payload_protects_derived_identity() {
        let (base, src, frag) = evidence_base();
        let s = base.node(&id(&src)).unwrap();
        let mut p = s.payload.clone();
        if let NodePayload::SourceArtifact(data) = &mut p {
            data.content_hash = digest("b").parse().unwrap();
        }
        assert_eq!(
            apply_err(
                &base,
                SemanticPatch::ReplacePayload {
                    target: pre_node(s),
                    payload: p
                }
            ),
            PatchError::DerivedIdentityChange {
                id: id(&src),
                field: "content_hash"
            }
        );
        let f = base.node(&id(&frag)).unwrap();
        let edit = |change: &dyn Fn(&mut EvidenceFragment)| {
            let mut p = f.payload.clone();
            if let NodePayload::EvidenceFragment(data) = &mut p {
                change(data);
            }
            SemanticPatch::ReplacePayload {
                target: pre_node(f),
                payload: p,
            }
        };
        assert_eq!(
            apply_err(&base, edit(&|d| d.source_ref = id("src:0000000000000000"))),
            PatchError::DerivedIdentityChange {
                id: id(&frag),
                field: "source_ref"
            }
        );
        assert_eq!(
            apply_err(&base, edit(&|d| d.locator = from_json(locator(0, 11)))),
            PatchError::DerivedIdentityChange {
                id: id(&frag),
                field: "locator"
            }
        );
        let result = apply_ok(
            &base,
            edit(&|d| d.extracted_text = Some("Employees submit requests.".into())),
        );
        assert_eq!(result.graph.node(&id(&frag)).unwrap().revision, 2);
    }

    #[test]
    fn replace_payload_runs_programmatic_payload_validation() {
        let base = graph(
            vec![
                derivation("drv:1", "Proposed"),
                node(
                    "decision:1",
                    "Proposed",
                    "ResolutionDecision",
                    json!({
                        "question_ref": "q:0123456789abcdef", "proposal_ref": null,
                        "answer": {"cardinality": "one_to_many"}, "decided_by": "actor:hr-lead",
                        "decided_at": T1, "patch_ref": digest("3"), "rationale": null,
                        "supersedes": null
                    }),
                ),
            ],
            vec![],
        );
        let d = base.node(&id("drv:1")).unwrap();
        let mut p = d.payload.clone();
        if let NodePayload::DerivationRecord(r) = &mut p {
            r.kind = DerivationKind::LlmInference;
        }
        assert!(matches!(
            apply_err(
                &base,
                SemanticPatch::ReplacePayload {
                    target: pre_node(d),
                    payload: p
                }
            ),
            PatchError::InvalidLocalNode {
                error: NodeError::InvalidPayload(NodePayloadError::InvalidDerivationRecord(
                    DerivationRecordError::MissingLlmField("provider")
                )),
                ..
            }
        ));
        let r = base.node(&id("decision:1")).unwrap();
        let mut p = r.payload.clone();
        if let NodePayload::ResolutionDecision(data) = &mut p {
            data.question_ref = None;
        }
        assert!(matches!(
            apply_err(
                &base,
                SemanticPatch::ReplacePayload {
                    target: pre_node(r),
                    payload: p
                }
            ),
            PatchError::InvalidLocalNode {
                error: NodeError::InvalidPayload(NodePayloadError::InvalidResolutionDecision(
                    ResolutionDecisionError::MissingQuestionOrProposal
                )),
                ..
            }
        ));
        // Locator changes are identity changes under ReplacePayload, so an invalid
        // programmatic locator is exercised through AddNode's local validation.
        let (ev_base, src, _) = evidence_base();
        let mut bad = fragment("Proposed", &src, locator(20, 30));
        if let NodePayload::EvidenceFragment(data) = &mut bad.payload {
            data.locator = EvidenceLocator::PageRegion {
                page: 1,
                x: Some(f64::INFINITY),
                y: None,
                width: None,
                height: None,
            };
        }
        assert!(matches!(
            apply_err(&ev_base, SemanticPatch::AddNode { node: bad }),
            PatchError::InvalidLocalNode {
                error: NodeError::InvalidPayload(NodePayloadError::InvalidEvidenceLocator(
                    EvidenceLocatorError::NonFiniteCoordinate("x")
                )),
                ..
            }
        ));
    }

    // ------------------------------------------------------------------ ReplaceEdge (§44)

    fn replace_edge(
        pre: ElementPrecondition,
        kind: &str,
        from: &str,
        to: &str,
        properties: RelationProperties,
    ) -> SemanticPatch {
        SemanticPatch::ReplaceEdge {
            target: pre,
            kind: from_json(json!(kind)),
            from: id(from),
            to: id(to),
            properties,
        }
    }

    fn edge_base() -> Graph {
        let mut e = edge("rel:a-eu", "Accepted", "constrained_by", "req:a", "con:eu");
        e.evidence = vec![];
        e.standards = vec![mapping("constraint")];
        e.derivations = vec![DerivationRef::from(id("drv:1"))];
        graph(
            vec![
                requirement(
                    "req:a",
                    "Accepted",
                    "Employees shall submit leave requests.",
                ),
                requirement("req:c", "Accepted", "Managers shall see balances."),
                constraint("con:eu", "Accepted"),
                constraint("con:gdpr", "Accepted"),
                derivation("drv:1", "Accepted"),
            ],
            vec![e],
        )
    }

    #[test]
    fn replace_edge_retargets_and_preserves_envelope() {
        let base = edge_base();
        let old = base.edge(&id("rel:a-eu")).unwrap();
        let result = apply_ok(
            &base,
            replace_edge(
                pre_edge(old),
                "constrained_by",
                "req:c",
                "con:gdpr",
                RelationProperties::None,
            ),
        );
        let e = result.graph.edge(&id("rel:a-eu")).unwrap();
        assert_eq!((&e.from, &e.to), (&id("req:c"), &id("con:gdpr")));
        assert_eq!(
            (
                &e.id,
                e.status,
                &e.evidence,
                &e.derivations,
                &e.standards,
                &e.audit
            ),
            (
                &old.id,
                old.status,
                &old.evidence,
                &old.derivations,
                &old.standards,
                &old.audit
            )
        );
        assert_eq!(e.revision, 2);
        assert_eq!(result.delta.modified_edges, ids(&["rel:a-eu"]));

        let ext_props = RelationProperties::from_json(
            &from_json(json!("acme:traces")),
            from_json(json!({"acme:weight": 2})),
        )
        .unwrap();
        let result = apply_ok(
            &base,
            replace_edge(
                pre_edge(old),
                "acme:traces",
                "req:a",
                "con:eu",
                ext_props.clone(),
            ),
        );
        let e = result.graph.edge(&id("rel:a-eu")).unwrap();
        assert_eq!(e.kind.to_string(), "acme:traces");
        assert_eq!(e.properties, ext_props);
        assert_eq!(e.id, id("rel:a-eu"));
    }

    #[test]
    fn replace_edge_rejects_locally_and_finally_invalid_edges() {
        let base = edge_base();
        let pre = pre_e(&base, "rel:a-eu");
        let err = apply_err(
            &base,
            replace_edge(
                pre.clone(),
                "conflicts_with",
                "req:c",
                "req:a",
                RelationProperties::None,
            ),
        );
        assert!(matches!(
            err,
            PatchError::InvalidLocalEdge {
                error: EdgeError::NonCanonicalConflictOrientation { .. },
                ..
            }
        ));
        let err = apply_err(
            &base,
            replace_edge(
                pre.clone(),
                "constrained_by",
                "req:a",
                "con:eu",
                RelationProperties::SchemaFor(SchemaForProperties {
                    role: SchemaBindingRole::Attribute,
                }),
            ),
        );
        assert!(matches!(
            err,
            PatchError::InvalidLocalEdge {
                error: EdgeError::InvalidProperties(_),
                ..
            }
        ));
        let err = apply_err(
            &base,
            replace_edge(
                pre,
                "constrained_by",
                "req:a",
                "req:c",
                RelationProperties::None,
            ),
        );
        assert!(final_violations(err).iter().any(|v| matches!(
            v,
            GraphViolation::Relation(RelationViolation::TargetTypeNotAllowed { .. })
        )));
    }

    // ------------------------------------------------------------------ removal (§45)

    #[test]
    fn remove_node_never_cascades() {
        let base = base_requirements();
        assert_eq!(
            apply_err(
                &base,
                SemanticPatch::RemoveNode {
                    target: pre_n(&base, "req:b")
                }
            ),
            PatchError::IncidentEdgesPreventRemoval {
                node: id("req:b"),
                edges: vec![id("rel:b-eu")]
            }
        );
        let result = apply_ok(
            &base,
            compound(vec![
                SemanticPatch::RemoveEdge {
                    target: pre_e(&base, "rel:b-eu"),
                },
                SemanticPatch::RemoveNode {
                    target: pre_n(&base, "req:b"),
                },
            ]),
        );
        assert_eq!(result.delta.removed_nodes, ids(&["req:b"]));
        assert_eq!(result.delta.removed_edges, ids(&["rel:b-eu"]));
        assert!(result.graph.node(&id("con:eu")).is_some());
    }

    #[test]
    fn reference_guard_rejects_typed_and_open_json_references() {
        let mut typed = requirement("req:owner", "Proposed", "The system shall notify.");
        if let NodePayload::Requirement(r) = &mut typed.payload {
            r.owner_refs = Some(vec![id("ent:leave")]);
        }
        let mut open = requirement("req:open", "Proposed", "The system shall archive.");
        open.extensions
            .insert(ext("acme:note"), json!({"see": ["ent:leave", "rel:x"]}));
        let base = graph(
            vec![
                entity("ent:leave", "Proposed"),
                entity("ent:other", "Proposed"),
                typed.clone(),
            ],
            vec![],
        );
        assert_eq!(
            apply_err(
                &base,
                SemanticPatch::RemoveNode {
                    target: pre_n(&base, "ent:leave")
                }
            ),
            PatchError::ReferencedElement {
                removed: id("ent:leave"),
                referenced_by: id("req:owner")
            }
        );
        let base = graph(
            vec![
                entity("ent:leave", "Proposed"),
                entity("ent:other", "Proposed"),
                open.clone(),
            ],
            vec![edge(
                "rel:x",
                "Proposed",
                "acme:links",
                "ent:leave",
                "ent:other",
            )],
        );
        assert_eq!(
            apply_err(
                &base,
                compound(vec![
                    SemanticPatch::RemoveEdge {
                        target: pre_e(&base, "rel:x")
                    },
                    SemanticPatch::RemoveNode {
                        target: pre_n(&base, "ent:leave")
                    },
                ])
            ),
            PatchError::ReferencedElement {
                removed: id("rel:x"),
                referenced_by: id("req:open")
            }
        );
        // Removing the referencing node in a prior child allows both deletions.
        let result = apply_ok(
            &base,
            compound(vec![
                SemanticPatch::RemoveNode {
                    target: pre_node(&open),
                },
                SemanticPatch::AddNode {
                    node: requirement("req:open2", "Proposed", "The system shall archive."),
                },
                SemanticPatch::RemoveEdge {
                    target: pre_e(&base, "rel:x"),
                },
                SemanticPatch::RemoveNode {
                    target: pre_n(&base, "ent:leave"),
                },
            ]),
        );
        assert_eq!(result.delta.removed_nodes, ids(&["ent:leave", "req:open"]));
    }

    #[test]
    fn reference_guard_inspects_json_values_not_object_keys() {
        let mut keyed = requirement("req:keyed", "Proposed", "The system shall archive.");
        keyed
            .extensions
            .insert(ext("acme:note"), json!({"ent:leave": "unrelated"}));
        let base = graph(vec![entity("ent:leave", "Proposed"), keyed], vec![]);
        let result = apply_ok(
            &base,
            SemanticPatch::RemoveNode {
                target: pre_n(&base, "ent:leave"),
            },
        );
        assert_eq!(result.delta.removed_nodes, ids(&["ent:leave"]));

        let mut valued = requirement("req:valued", "Proposed", "The system shall archive.");
        valued
            .extensions
            .insert(ext("acme:note"), json!({"target": "ent:leave"}));
        let base = graph(vec![entity("ent:leave", "Proposed"), valued], vec![]);
        assert_eq!(
            apply_err(
                &base,
                SemanticPatch::RemoveNode {
                    target: pre_n(&base, "ent:leave")
                }
            ),
            PatchError::ReferencedElement {
                removed: id("ent:leave"),
                referenced_by: id("req:valued")
            }
        );
    }

    #[test]
    fn reference_replaced_by_prior_child_allows_deletion() {
        let mut typed = requirement("req:owner", "Proposed", "The system shall notify.");
        if let NodePayload::Requirement(r) = &mut typed.payload {
            r.owner_refs = Some(vec![id("ent:leave")]);
        }
        let base = graph(vec![entity("ent:leave", "Proposed"), typed.clone()], vec![]);
        let result = apply_ok(
            &base,
            compound(vec![
                SemanticPatch::ReplacePayload {
                    target: pre_node(&typed),
                    payload: requirement_payload("The system shall notify."),
                },
                SemanticPatch::RemoveNode {
                    target: pre_n(&base, "ent:leave"),
                },
            ]),
        );
        assert_eq!(result.delta.removed_nodes, ids(&["ent:leave"]));
        assert_eq!(result.delta.modified_nodes, ids(&["req:owner"]));
    }

    #[test]
    fn removed_and_used_ids_cannot_be_reused() {
        let base = base_requirements();
        let r = base.node(&id("req:r")).unwrap().clone();
        assert_eq!(
            apply_err(
                &base,
                compound(vec![
                    SemanticPatch::RemoveNode {
                        target: pre_node(&r)
                    },
                    SemanticPatch::AddNode { node: r.clone() },
                ])
            ),
            PatchError::IdReused(id("req:r"))
        );
        let e = base.edge(&id("rel:b-eu")).unwrap().clone();
        assert_eq!(
            apply_err(
                &base,
                compound(vec![
                    SemanticPatch::RemoveEdge {
                        target: pre_edge(&e)
                    },
                    SemanticPatch::AddEdge { edge: e.clone() },
                ])
            ),
            PatchError::IdReused(id("rel:b-eu"))
        );
        let x = requirement("req:x", "Proposed", "The system shall print.");
        assert_eq!(
            apply_err(
                &base,
                compound(vec![
                    SemanticPatch::AddNode { node: x.clone() },
                    SemanticPatch::RemoveNode {
                        target: pre_node(&x)
                    },
                    SemanticPatch::AddNode { node: x.clone() },
                ])
            ),
            PatchError::IdReused(id("req:x"))
        );
        // Node and edge IDs share one namespace.
        assert_eq!(
            apply_err(
                &base,
                SemanticPatch::AddNode {
                    node: requirement("rel:b-eu", "Proposed", "Collides.")
                }
            ),
            PatchError::IdAlreadyExists(id("rel:b-eu"))
        );
    }

    // ------------------------------------------------------------------ MergeNodes (§46)

    fn merge(keep: ElementPrecondition, merged: Vec<ElementPrecondition>) -> SemanticPatch {
        SemanticPatch::MergeNodes {
            keep,
            merge: merged,
            field_policy: MergePolicy::KeepPayloadUnionMetadata,
        }
    }

    #[test]
    fn merge_requirements_unions_metadata() {
        let hash = digest("a");
        let src = src_id(&hash);
        let f1 = fragment("Accepted", &src, locator(0, 10));
        let f2 = fragment("Accepted", &src, locator(10, 20));
        let (e1, e2) = (
            EvidenceRef::from(f1.id.clone()),
            EvidenceRef::from(f2.id.clone()),
        );
        let mut keep = requirement("req:keep", "Accepted", "Employees shall submit leave.");
        keep.evidence = vec![e1.clone()];
        keep.derivations = vec![DerivationRef::from(id("drv:2"))];
        keep.standards = vec![mapping("zeta")];
        keep.tags = ["hr".to_string()].into();
        keep.extensions.insert(ext("acme:x"), json!(1));
        let mut dup = requirement("req:dup", "Proposed", "Employees submit leave.");
        dup.evidence = vec![e2.clone(), e1.clone()];
        dup.derivations = vec![DerivationRef::from(id("drv:1"))];
        dup.standards = vec![mapping("alpha"), mapping("zeta")];
        dup.tags = ["leave".to_string(), "hr".to_string()].into();
        dup.extensions.insert(ext("acme:x"), json!(1));
        dup.extensions.insert(ext("acme:y"), json!("two"));
        let base = graph(
            vec![
                source("Accepted", &hash),
                f1,
                f2,
                derivation("drv:1", "Accepted"),
                derivation("drv:2", "Accepted"),
                keep.clone(),
                dup.clone(),
            ],
            vec![],
        );
        let result = apply_ok(&base, merge(pre_node(&keep), vec![pre_node(&dup)]));
        let k = result.graph.node(&id("req:keep")).unwrap();
        let mut expected_evidence = vec![e1, e2];
        expected_evidence.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        assert_eq!(k.evidence, expected_evidence);
        assert_eq!(
            k.derivations,
            vec![
                DerivationRef::from(id("drv:1")),
                DerivationRef::from(id("drv:2"))
            ]
        );
        let mut expected_standards = vec![mapping("zeta"), mapping("alpha")];
        expected_standards.sort_by_key(|m| to_canonical_json(m).unwrap());
        assert_eq!(k.standards, expected_standards);
        assert_eq!(k.tags, ["hr".to_string(), "leave".to_string()].into());
        assert_eq!(k.extensions.len(), 2);
        assert_eq!(k.extensions[&ext("acme:y")], json!("two"));
        assert_eq!(
            (&k.payload, k.status, &k.audit),
            (&keep.payload, keep.status, &keep.audit)
        );
        assert_eq!(k.revision, 2);
        assert!(result.graph.node(&id("req:dup")).is_none());
        assert_eq!(result.delta.removed_nodes, ids(&["req:dup"]));
        assert_eq!(result.delta.modified_nodes, ids(&["req:keep"]));
        // All automatic changes of the one leaf share its ordinal.
        assert!(result.delta.diff.iter().all(|d| d.operation_ordinal == 0));
        assert_eq!(result.delta.diff.len(), 2);
    }

    #[test]
    fn merge_terms_and_extension_conflicts() {
        let keep = term("term:leave", "Accepted", "leave");
        let dup = term("term:absence", "Accepted", "absence");
        let base = graph(vec![keep.clone(), dup.clone()], vec![]);
        let result = apply_ok(&base, merge(pre_node(&keep), vec![pre_node(&dup)]));
        assert_eq!(result.graph.nodes().len(), 1);
        // Nothing unioned changed the element hash of the keep node.
        assert_eq!(result.graph.node(&id("term:leave")).unwrap(), &keep);

        let mut a = term("term:a", "Accepted", "a");
        a.extensions.insert(ext("acme:x"), json!(1));
        let mut b = term("term:b", "Accepted", "b");
        b.extensions.insert(ext("acme:x"), json!(2));
        let base = graph(vec![a.clone(), b.clone()], vec![]);
        assert_eq!(
            apply_err(&base, merge(pre_node(&a), vec![pre_node(&b)])),
            PatchError::MergeExtensionConflict {
                key: "acme:x".into()
            }
        );
    }

    #[test]
    fn merge_preconditions_are_enforced() {
        let a = term("term:a", "Accepted", "a");
        let b = term("term:b", "Accepted", "b");
        let r = requirement("req:r", "Accepted", "The system shall merge.");
        let e1 = entity("ent:1", "Proposed");
        let e2 = entity("ent:2", "Proposed");
        let base = graph(
            vec![a.clone(), b.clone(), r.clone(), e1.clone(), e2.clone()],
            vec![],
        );
        assert_eq!(
            apply_err(&base, merge(pre_node(&a), vec![])),
            PatchError::EmptyMerge
        );
        assert_eq!(
            apply_err(&base, merge(pre_node(&a), vec![pre_node(&b), pre_node(&b)])),
            PatchError::DuplicateMergeTarget(id("term:b"))
        );
        assert_eq!(
            apply_err(&base, merge(pre_node(&a), vec![pre_node(&a)])),
            PatchError::MergeContainsKeep(id("term:a"))
        );
        assert_eq!(
            apply_err(&base, merge(pre_node(&a), vec![pre_node(&r)])),
            PatchError::MergeTypeMismatch {
                id: id("req:r"),
                expected: NodeType::Term,
                found: NodeType::Requirement
            }
        );
        assert_eq!(
            apply_err(&base, merge(pre_node(&e1), vec![pre_node(&e2)])),
            PatchError::MergeTypeUnsupported(NodeType::Entity)
        );
        let mut stale = pre_node(&b);
        stale.expected_hash = EL_H1.parse().unwrap();
        assert!(matches!(
            apply_err(&base, merge(pre_node(&a), vec![stale])),
            PatchError::ElementHashMismatch { .. }
        ));
    }

    #[test]
    fn merge_rejects_incident_edges_and_references_until_rewired() {
        let keep = requirement("req:keep", "Accepted", "Employees shall submit leave.");
        let dup = requirement("req:dup", "Accepted", "Employees submit leave.");
        let mut referrer = requirement("req:ref", "Proposed", "Refers to the duplicate.");
        if let NodePayload::Requirement(r) = &mut referrer.payload {
            r.stakeholder_refs = Some(vec![id("req:dup")]);
        }
        let base = graph(
            vec![keep.clone(), dup.clone(), constraint("con:eu", "Accepted")],
            vec![edge(
                "rel:dup-eu",
                "Accepted",
                "constrained_by",
                "req:dup",
                "con:eu",
            )],
        );
        assert_eq!(
            apply_err(&base, merge(pre_node(&keep), vec![pre_node(&dup)])),
            PatchError::IncidentEdgesPreventRemoval {
                node: id("req:dup"),
                edges: vec![id("rel:dup-eu")]
            }
        );
        let result = apply_ok(
            &base,
            compound(vec![
                replace_edge(
                    pre_e(&base, "rel:dup-eu"),
                    "constrained_by",
                    "req:keep",
                    "con:eu",
                    RelationProperties::None,
                ),
                merge(pre_node(&keep), vec![pre_node(&dup)]),
            ]),
        );
        assert_eq!(
            result.graph.edge(&id("rel:dup-eu")).unwrap().from,
            id("req:keep")
        );
        assert_eq!(result.delta.removed_nodes, ids(&["req:dup"]));
        // Unchanged keep: revision stays, but the node was touched.
        assert_eq!(result.graph.node(&id("req:keep")).unwrap().revision, 1);
        assert!(result.delta.touched_nodes.contains(&id("req:keep")));

        let base = graph(vec![keep.clone(), dup.clone(), referrer.clone()], vec![]);
        assert_eq!(
            apply_err(&base, merge(pre_node(&keep), vec![pre_node(&dup)])),
            PatchError::ReferencedElement {
                removed: id("req:dup"),
                referenced_by: id("req:ref")
            }
        );
        let result = apply_ok(
            &base,
            compound(vec![
                SemanticPatch::RemoveNode {
                    target: pre_node(&referrer),
                },
                merge(pre_node(&keep), vec![pre_node(&dup)]),
            ]),
        );
        assert_eq!(result.delta.removed_nodes, ids(&["req:dup", "req:ref"]));
        // A merged ID cannot be recycled later in the same patch set.
        assert_eq!(
            apply_err(
                &base,
                compound(vec![
                    SemanticPatch::RemoveNode {
                        target: pre_node(&referrer)
                    },
                    merge(pre_node(&keep), vec![pre_node(&dup)]),
                    SemanticPatch::AddNode { node: dup.clone() },
                ])
            ),
            PatchError::IdReused(id("req:dup"))
        );
    }

    // ------------------------------------------------------------------ Supersede (§47)

    fn supersede_base() -> Graph {
        graph(
            vec![
                requirement("req:old", "Accepted", "Employees shall fax leave requests."),
                requirement(
                    "req:new",
                    "Accepted",
                    "Employees shall submit leave online.",
                ),
                constraint("con:eu", "Accepted"),
            ],
            vec![],
        )
    }

    fn supersede(base: &Graph, e: Edge) -> SemanticPatch {
        SemanticPatch::Supersede {
            old: pre_n(base, "req:old"),
            new: pre_n(base, "req:new"),
            edge: e,
        }
    }

    #[test]
    fn supersede_marks_old_and_adds_exactly_the_supplied_edge() {
        let base = supersede_base();
        let e = edge("rel:sup", "Accepted", "supersedes", "req:new", "req:old");
        let result = apply_ok(&base, supersede(&base, e.clone()));
        let old = result.graph.node(&id("req:old")).unwrap();
        assert_eq!((old.status, old.revision), (ElementStatus::Superseded, 2));
        assert_eq!(
            result.graph.node(&id("req:new")).unwrap(),
            base.node(&id("req:new")).unwrap()
        );
        assert_eq!(result.graph.edge(&id("rel:sup")).unwrap(), &e);
        assert_eq!(result.graph.edges().len(), 1);
        assert_eq!(result.graph.nodes().len(), 3);
        assert_eq!(result.delta.added_edges, ids(&["rel:sup"]));
        assert_eq!(result.delta.touched_nodes, ids(&["req:new", "req:old"]));
        assert_eq!(result.delta.modified_nodes, ids(&["req:old"]));
    }

    #[test]
    fn supersede_rejects_malformed_edges() {
        let base = supersede_base();
        let invalid = |e: Edge| apply_err(&base, supersede(&base, e));
        assert!(matches!(
            invalid(edge("rel:sup", "Accepted", "refines", "req:new", "req:old")),
            PatchError::InvalidSupersede(_)
        ));
        assert!(matches!(
            invalid(edge(
                "rel:sup",
                "Accepted",
                "supersedes",
                "req:old",
                "req:new"
            )),
            PatchError::InvalidSupersede(_)
        ));
        assert!(matches!(
            invalid(edge(
                "rel:sup",
                "Accepted",
                "supersedes",
                "req:new",
                "con:eu"
            )),
            PatchError::InvalidSupersede(_)
        ));
        assert!(matches!(
            invalid(edge(
                "rel:sup",
                "Proposed",
                "supersedes",
                "req:new",
                "req:old"
            )),
            PatchError::InvalidSupersede(_)
        ));
        let mut e = edge("rel:sup", "Accepted", "supersedes", "req:new", "req:old");
        e.revision = 2;
        assert_eq!(
            invalid(e),
            PatchError::NewElementRevisionNotOne {
                id: id("rel:sup"),
                revision: 2
            }
        );
        assert!(matches!(
            invalid(edge(
                "req:new",
                "Accepted",
                "supersedes",
                "req:new",
                "req:old"
            )),
            PatchError::IdAlreadyExists(_)
        ));
        let same = SemanticPatch::Supersede {
            old: pre_n(&base, "req:old"),
            new: pre_n(&base, "req:old"),
            edge: edge("rel:sup", "Accepted", "supersedes", "req:old", "req:old"),
        };
        assert!(matches!(
            apply_err(&base, same),
            PatchError::InvalidSupersede(_)
        ));
        let mut stale = pre_n(&base, "req:old");
        stale.expected_hash = EL_H1.parse().unwrap();
        let p = SemanticPatch::Supersede {
            old: stale,
            new: pre_n(&base, "req:new"),
            edge: edge("rel:sup", "Accepted", "supersedes", "req:new", "req:old"),
        };
        assert!(matches!(
            apply_err(&base, p),
            PatchError::ElementHashMismatch { .. }
        ));
    }

    #[test]
    fn supersede_cycle_is_rejected_by_final_graph() {
        let base = graph(
            vec![
                requirement("req:old", "Accepted", "Employees shall fax leave requests."),
                requirement(
                    "req:new",
                    "Accepted",
                    "Employees shall submit leave online.",
                ),
            ],
            vec![edge(
                "rel:prior",
                "Accepted",
                "supersedes",
                "req:old",
                "req:new",
            )],
        );
        let err = apply_err(
            &base,
            supersede(
                &base,
                edge("rel:sup", "Accepted", "supersedes", "req:new", "req:old"),
            ),
        );
        assert!(final_violations(err)
            .iter()
            .any(|v| matches!(v, GraphViolation::Relation(RelationViolation::Cycle { .. }))));
    }

    // ------------------------------------------------------------------ attach (§48)

    #[test]
    fn attach_evidence_to_nodes_and_edges() {
        let hash = digest("a");
        let src = src_id(&hash);
        let f = fragment("Accepted", &src, locator(0, 10));
        let ev = EvidenceRef::from(f.id.clone());
        let base = graph(
            vec![
                source("Accepted", &hash),
                f,
                requirement(
                    "req:a",
                    "Accepted",
                    "Employees shall submit leave requests.",
                ),
                constraint("con:eu", "Accepted"),
            ],
            vec![edge(
                "rel:a-eu",
                "Accepted",
                "constrained_by",
                "req:a",
                "con:eu",
            )],
        );
        let result = apply_ok(
            &base,
            compound(vec![
                SemanticPatch::AttachEvidence {
                    target: pre_n(&base, "req:a"),
                    evidence: ev.clone(),
                },
                SemanticPatch::AttachEvidence {
                    target: pre_e(&base, "rel:a-eu"),
                    evidence: ev.clone(),
                },
            ]),
        );
        assert_eq!(
            result.graph.node(&id("req:a")).unwrap().evidence,
            vec![ev.clone()]
        );
        assert_eq!(
            result.graph.edge(&id("rel:a-eu")).unwrap().evidence,
            vec![ev.clone()]
        );
        // No evidenced_by edge is created.
        assert_eq!(result.graph.edges().len(), 1);

        let with_ev = result.graph;
        assert_eq!(
            apply_err(
                &with_ev,
                SemanticPatch::AttachEvidence {
                    target: pre_n(&with_ev, "req:a"),
                    evidence: ev.clone()
                }
            ),
            PatchError::DuplicateEvidenceAttachment {
                target: id("req:a"),
                evidence: ev.as_id().clone()
            }
        );
        let err = apply_err(
            &base,
            SemanticPatch::AttachEvidence {
                target: pre_n(&base, "req:a"),
                evidence: EvidenceRef::from(id("evd:0000000000000000")),
            },
        );
        assert!(final_violations(err)
            .iter()
            .any(|v| matches!(v, GraphViolation::EvidenceRefMissing { .. })));
        let mut stale = pre_n(&base, "req:a");
        stale.expected_hash = EL_H1.parse().unwrap();
        assert!(matches!(
            apply_err(
                &base,
                SemanticPatch::AttachEvidence {
                    target: stale,
                    evidence: ev
                }
            ),
            PatchError::ElementHashMismatch { .. }
        ));
    }

    #[test]
    fn attach_standard_mapping_to_nodes_and_edges() {
        let base = base_requirements();
        let m = mapping("requirement");
        let result = apply_ok(
            &base,
            compound(vec![
                SemanticPatch::AttachStandardMapping {
                    target: pre_n(&base, "req:a"),
                    mapping: m.clone(),
                },
                SemanticPatch::AttachStandardMapping {
                    target: pre_e(&base, "rel:a-eu"),
                    mapping: m.clone(),
                },
            ]),
        );
        assert_eq!(
            result.graph.node(&id("req:a")).unwrap().standards,
            vec![m.clone()]
        );
        assert_eq!(
            result.graph.edge(&id("rel:a-eu")).unwrap().standards,
            vec![m.clone()]
        );
        let g = result.graph;
        assert_eq!(
            apply_err(
                &g,
                SemanticPatch::AttachStandardMapping {
                    target: pre_e(&g, "rel:a-eu"),
                    mapping: m.clone()
                }
            ),
            PatchError::DuplicateStandardMappingAttachment(id("rel:a-eu"))
        );
        // A different mapping is appended, never merged with the existing one.
        let mut other = m.clone();
        other.mapping_strength = from_json(json!("exact"));
        let r = apply_ok(
            &g,
            SemanticPatch::AttachStandardMapping {
                target: pre_n(&g, "req:a"),
                mapping: other.clone(),
            },
        );
        assert_eq!(
            r.graph.node(&id("req:a")).unwrap().standards,
            vec![m, other]
        );
        let mut stale = pre_n(&g, "req:a");
        stale.expected_hash = EL_H2.parse().unwrap();
        assert!(matches!(
            apply_err(
                &g,
                SemanticPatch::AttachStandardMapping {
                    target: stale,
                    mapping: mapping("x")
                }
            ),
            PatchError::ElementHashMismatch { .. }
        ));
    }

    // ------------------------------------------------------------------ GraphDelta / diff (§49)

    #[test]
    fn delta_sets_and_diff_are_exact() {
        let base = base_requirements();
        let a = base.node(&id("req:a")).unwrap();
        let b = base.node(&id("req:b")).unwrap();
        let rejected_b = with_status(b, ElementStatus::Rejected);
        let added = requirement("req:0", "Proposed", "The system shall log.");
        let result = apply_ok(
            &base,
            compound(vec![
                set_status(pre_node(b), "Proposed", "Rejected"),
                compound(vec![
                    SemanticPatch::AttachStandardMapping {
                        target: pre_node(a),
                        mapping: mapping("requirement"),
                    },
                    SemanticPatch::AddNode {
                        node: added.clone(),
                    },
                ]),
                set_status(pre_node(&rejected_b), "Rejected", "Proposed"),
                SemanticPatch::RemoveNode {
                    target: pre_n(&base, "req:r"),
                },
            ]),
        );
        let d = &result.delta;
        assert_eq!(d.touched_nodes, ids(&["req:0", "req:a", "req:b", "req:r"]));
        assert!(d.touched_edges.is_empty());
        assert_eq!(d.added_nodes, ids(&["req:0"]));
        assert_eq!(d.removed_nodes, ids(&["req:r"]));
        assert_eq!(d.modified_nodes, ids(&["req:a"]));
        assert!(
            d.added_edges.is_empty() && d.removed_edges.is_empty() && d.modified_edges.is_empty()
        );
        assert_eq!(
            d.result_semantic_hash,
            result.graph.semantic_hash().unwrap()
        );

        let mut a_after = a.clone();
        a_after.standards = vec![mapping("requirement")];
        let h = |n: &Node| Some(node_element_hash(n).unwrap());
        let entry = |i: &str, ord, change, before, after| DiffEntry {
            element_kind: DiffElementKind::Node,
            id: id(i),
            operation_ordinal: ord,
            change,
            before_hash: before,
            after_hash: after,
        };
        assert_eq!(
            d.diff,
            vec![
                entry("req:0", 2, DiffChange::Added, None, h(&added)),
                entry("req:a", 1, DiffChange::Modified, h(a), h(&a_after)),
                entry("req:b", 0, DiffChange::Modified, h(b), h(&rejected_b)),
                entry("req:b", 3, DiffChange::Modified, h(&rejected_b), h(b)),
                entry(
                    "req:r",
                    4,
                    DiffChange::Removed,
                    h(base.node(&id("req:r")).unwrap()),
                    None
                ),
            ]
        );
        let wire = serde_json::to_value(d).unwrap();
        assert_eq!(wire["diff"][0]["element_kind"], json!("Node"));
        assert_eq!(wire["diff"][0]["change"], json!("Added"));
        assert_eq!(wire["diff"][0]["before_hash"], Value::Null);
        assert_eq!(wire["added_nodes"], json!(["req:0"]));
    }

    #[test]
    fn diff_orders_edges_and_nodes_by_id() {
        let base = base_requirements();
        let result = apply_ok(
            &base,
            compound(vec![
                SemanticPatch::RemoveEdge {
                    target: pre_e(&base, "rel:b-eu"),
                },
                SemanticPatch::AddEdge {
                    edge: edge("rel:0", "Proposed", "constrained_by", "req:b", "con:eu"),
                },
                set_status(pre_n(&base, "req:b"), "Proposed", "Rejected"),
            ]),
        );
        let order: Vec<(String, DiffElementKind, u32)> = result
            .delta
            .diff
            .iter()
            .map(|d| (d.id.to_string(), d.element_kind, d.operation_ordinal))
            .collect();
        assert_eq!(
            order,
            vec![
                ("rel:0".into(), DiffElementKind::Edge, 1),
                ("rel:b-eu".into(), DiffElementKind::Edge, 0),
                ("req:b".into(), DiffElementKind::Node, 2),
            ]
        );
        assert_eq!(result.delta.touched_edges, ids(&["rel:0", "rel:b-eu"]));
    }

    // ------------------------------------------------------------------ inverse (§50)

    #[test]
    fn add_inverses_are_exact_and_round_trip() {
        let base = base_requirements();
        let n = requirement("req:new", "Proposed", "The system shall export.");
        let e = edge(
            "rel:new-eu",
            "Proposed",
            "constrained_by",
            "req:new",
            "con:eu",
        );
        let add_node = SemanticPatch::AddNode { node: n.clone() };
        let add_edge = SemanticPatch::AddEdge { edge: e.clone() };
        assert_eq!(
            add_node.inverse().unwrap(),
            Some(SemanticPatch::RemoveNode {
                target: pre_node(&n)
            })
        );
        assert_eq!(
            add_edge.inverse().unwrap(),
            Some(SemanticPatch::RemoveEdge {
                target: pre_edge(&e)
            })
        );
        let forward = compound(vec![add_node, add_edge]);
        let inverse = forward.inverse().unwrap().unwrap();
        assert_eq!(
            inverse,
            compound(vec![
                SemanticPatch::RemoveEdge {
                    target: pre_edge(&e)
                },
                SemanticPatch::RemoveNode {
                    target: pre_node(&n)
                },
            ])
        );
        let applied = apply_ok(&base, forward);
        let restored = apply_ok(&applied.graph, inverse);
        assert_eq!(restored.graph, base);
        assert_eq!(
            restored.delta.result_semantic_hash,
            base.semantic_hash().unwrap()
        );
    }

    #[test]
    fn non_invertible_variants_return_none() {
        let fixtures = variant_fixtures();
        for (name, wire) in fixtures {
            let patch: SemanticPatch = from_json(wire);
            let inverse = patch.inverse().unwrap();
            match name {
                "AddNode" | "AddEdge" => assert!(inverse.is_some(), "{name}"),
                _ => assert_eq!(inverse, None, "{name}"),
            }
        }
        let mixed = compound(vec![
            SemanticPatch::AddNode {
                node: requirement("req:x", "Proposed", "x"),
            },
            SemanticPatch::RemoveNode {
                target: ElementPrecondition {
                    id: id("req:y"),
                    expected_hash: EL_H1.parse().unwrap(),
                },
            },
        ]);
        assert_eq!(mixed.inverse().unwrap(), None);
    }

    // ------------------------------------------------------------------ determinism (§51)

    #[derive(Debug, Clone)]
    struct Gen {
        accepted: bool,
        with_edge: bool,
        order_key: u8,
    }

    fn gen_strategy() -> impl Strategy<Value = Vec<Gen>> {
        prop::collection::vec(
            (any::<bool>(), any::<bool>(), any::<u8>()).prop_map(
                |(accepted, with_edge, order_key)| Gen {
                    accepted,
                    with_edge,
                    order_key,
                },
            ),
            1..6,
        )
    }

    /// Generated additions in a shuffled order; with `nodes_first` every AddNode precedes every
    /// AddEdge so that the reversed inverse removes edges before their endpoints.
    fn generated_leaves(gens: &[Gen], nodes_first: bool) -> Vec<SemanticPatch> {
        let mut keyed = Vec::new();
        for (i, g) in gens.iter().enumerate() {
            let status = if g.accepted { "Accepted" } else { "Proposed" };
            let req_id = format!("req:gen-{i}");
            keyed.push((
                g.order_key,
                2 * i,
                SemanticPatch::AddNode {
                    node: requirement(&req_id, status, &format!("Generated requirement {i}.")),
                },
            ));
            if g.with_edge {
                keyed.push((
                    g.order_key.wrapping_add(7),
                    2 * i + 1,
                    SemanticPatch::AddEdge {
                        edge: edge(
                            &format!("rel:gen-{i}"),
                            status,
                            "constrained_by",
                            &req_id,
                            "con:eu",
                        ),
                    },
                ));
            }
        }
        keyed.sort_by_key(|(k, i, _)| (nodes_first && i % 2 == 1, *k, *i));
        keyed.into_iter().map(|(_, _, p)| p).collect()
    }

    proptest! {
        // No regression files: they are outside the task allowlist.
        #![proptest_config(ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn patch_application_is_deterministic(gens in gen_strategy()) {
            let base = base_requirements();
            let leaves = generated_leaves(&gens, false);
            let flat = compound(leaves.clone());
            let first = apply_ok(&base, flat.clone());
            let second = apply_ok(&base.clone(), flat.clone());
            prop_assert_eq!(&first.graph, &second.graph);
            prop_assert_eq!(
                serde_json::to_vec(&first.delta).unwrap(),
                serde_json::to_vec(&second.delta).unwrap()
            );
            // Nesting preserves depth-first order and therefore the whole result.
            let mut nested = Vec::new();
            for chunk in leaves.chunks(2) {
                nested.push(compound(chunk.to_vec()));
            }
            let nested = apply_ok(&base, compound(nested));
            prop_assert_eq!(&nested.graph, &first.graph);
            prop_assert_eq!(
                serde_json::to_vec(&nested.delta).unwrap(),
                serde_json::to_vec(&first.delta).unwrap()
            );
        }

        #[test]
        fn add_patch_inverse_round_trips(gens in gen_strategy()) {
            let base = base_requirements();
            let forward = compound(generated_leaves(&gens, true));
            let applied = apply_ok(&base, forward.clone());
            let inverse = forward.inverse().unwrap().unwrap();
            let restored = apply_ok(&applied.graph, inverse);
            prop_assert_eq!(&restored.graph, &base);
        }
    }
}
