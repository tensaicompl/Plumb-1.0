//! S2.1 contract tests for closed-vocabulary domain extraction: the schema and prompt, the
//! domain request, inference validation, grounding, ConceptKind rules, Entity / Attribute /
//! DomainRelationship proposals, the two-pass dependency path, missing-cardinality finding
//! material, governed cardinality decisions, reconciliation, consolidation, determinism and the
//! real HR diagnostic.
//!
//! Every test lives in `domain_contract` so that `cargo test -p plumb-functional domain`
//! selects them. Golden values were computed independently with Python `hashlib` over RFC 8785
//! JSON (and the finding-key bytes), never with the helpers under test. The synthetic domain is
//! a structural conformance fixture, not an extraction-accuracy benchmark.

mod domain_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use jsonschema::{Draft, JSONSchema};
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_functional::domain::*;
    use plumb_functional::{
        analyze_vocabulary, build_requirement_classification_request, build_vocabulary_request,
        compile_requirement_candidates, normalize_vocabulary_term,
        RequirementClassificationInference, RequirementCompilationAudit, VocabularyAudit,
        VocabularyInference, VocabularyPolicy,
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
        Agent, AgentKind, AuditMeta, Concept, ConceptKind, DerivationRef, Edge, ElementStatus,
        Entity, EvidenceRef, FindingSeverity, Graph, Modality, Node, NodePayload, NodeType,
        Question, QuestionKind, RelationKind, RelationProperties, Requirement, RequirementKind,
        RequirementLevel, ResolutionDecision,
    };
    use plumb_validation::GeneratedFinding;
    use serde_json::{json, Value};

    use ConceptKind::{FactType, ObjectType, Other, ValueType};
    use ElementStatus::{Accepted, Proposed};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s2-domain.md");
    const SCHEMA: &str = include_str!("../../../schemas/inference/s2-domain.schema.json");
    const HR_MD: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.md");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:67ed949c73620f5a26a4fd0419a51eeb7a05b9344334bad1cb54e64dfcc8c579";
    const SCHEMA_HASH: &str =
        "sha256:dce63d7466b24090a9e61e1033131538b12c808edb8a093a71c4e384297ab763";
    const GOLDEN_CONTEXT_HASH: &str =
        "sha256:b3ebb84070c0a58933d442b4ac883d447aa01c1d561fac25cbde9a93268e9ae6";
    const GOLDEN_REQUEST_ID: &str =
        "sha256:49b49ee52b573ce421d6b342b684f02c614ff9e4aa87e677acb96d62c39e22af";
    const EMPLOYEE_ENTITY: &str = "entity:c6ced1ff435f765a";
    const LEAVE_REQUEST_ENTITY: &str = "entity:148afe265d887d52";
    const START_DATE_ATTRIBUTE: &str = "attr:c2b91d3a99ff7cc5";
    const SUBMITS_RELATIONSHIP: &str = "domainrel:9cb2e27561560077";
    const HAS_ATTRIBUTE_EDGE: &str = "rel:659eed4f52a6eee6";
    const CARDINALITY_FINDING_KEY: &str =
        "sha256:9d62bce4279aa36e59bb08579b3f3aa4bd9c15b31fe6c86963c7d39095b302f1";
    const CARDINALITY_FINDING_ID: &str = "fnd:9d62bce4279aa36e";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const RELATION_TYPED: &str = "PLUMB.F2.DOMAIN.RELATION_TYPED";

    // Synthetic test vocabulary (test data only; never in production source).
    const R1: &str = "Every Employee submits Leave Request records; each Leave Request belongs to \
exactly one Employee, and an Employee may hold zero or more of them.";
    const R2: &str = "Each Leave Request shall have a mandatory Start Date of type Date.";
    const DATE: &str = "concept:date";
    const EMPLOYEE: &str = "concept:employee";
    const LEAVE_REQUEST: &str = "concept:leave-request";
    const START_DATE: &str = "concept:start-date";
    const SUBMITS: &str = "concept:submits";

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
        DerivationRef::from(id("drv:00000000000000d2"))
    }

    fn audit_at(at: &str) -> DomainAudit {
        DomainAudit {
            created_by: id("agent:domain"),
            created_at: ts(at),
        }
    }

    fn audit() -> DomainAudit {
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

    fn concept_node(
        node_id: &str,
        status: ElementStatus,
        name: &str,
        definition: &str,
        kind: ConceptKind,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Concept(Concept {
                name: name.to_owned(),
                definition: definition.to_owned(),
                concept_kind: kind,
            }),
        )
    }

    fn vocabulary() -> Vec<Node> {
        vec![
            concept_node(DATE, Accepted, "Date", "A calendar date.", ValueType),
            concept_node(
                EMPLOYEE,
                Accepted,
                "Employee",
                "A person employed by the organization.",
                ObjectType,
            ),
            concept_node(
                LEAVE_REQUEST,
                Accepted,
                "Leave Request",
                "A request for leave.",
                ObjectType,
            ),
            concept_node(
                START_DATE,
                Accepted,
                "Start Date",
                "The first day of leave.",
                Other,
            ),
            concept_node(
                SUBMITS,
                Accepted,
                "Employee submits Leave Request",
                "An employee submits a leave request.",
                FactType,
            ),
        ]
    }

    fn new_graph(project: &str, profile: &str, nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(project), id(profile), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    /// The synthetic graph: Accepted requirements r1/r2 and the Accepted vocabulary. With
    /// `evidence`, both statements are imported and r1, r2 and the Employee Concept carry
    /// EvidenceFragment evidence.
    fn graph_with(statements: [&str; 2], evidence: bool) -> Graph {
        let mut nodes = vec![
            requirement_node("req:r1", Accepted, statements[0]),
            requirement_node("req:r2", Accepted, statements[1]),
        ];
        nodes.extend(vocabulary());
        if evidence {
            let imported = import_plain_text(
                "synthetic.txt",
                statements.join("\n\n").as_bytes(),
                &ImportAudit {
                    created_by: id("actor:importer"),
                    created_at: ts(AT),
                },
            )
            .unwrap();
            assert_eq!(imported.fragments.len(), 2);
            let refs: Vec<EvidenceRef> = imported
                .fragments
                .iter()
                .map(|f| EvidenceRef::from(f.id.clone()))
                .collect();
            nodes[0].evidence = vec![refs[0].clone()];
            nodes[1].evidence = vec![refs[1].clone()];
            nodes[3].evidence = vec![refs[1].clone()];
            nodes.push(imported.source);
            nodes.extend(imported.fragments);
        }
        new_graph(PROJECT, PROFILE, nodes, vec![])
    }

    fn base_graph() -> Graph {
        graph_with([R1, R2], true)
    }

    fn extend(graph: &Graph, nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        let mut all_nodes: Vec<Node> = graph.nodes().values().cloned().collect();
        all_nodes.extend(nodes);
        let mut all_edges: Vec<Edge> = graph.edges().values().cloned().collect();
        all_edges.extend(edges);
        Graph::new(
            graph.project_id().clone(),
            graph.profile_id().clone(),
            all_nodes,
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

    fn statement(graph: &Graph, requirement: &str) -> String {
        match &graph.node(&id(requirement)).unwrap().payload {
            NodePayload::Requirement(r) => r.statement.clone(),
            _ => unreachable!(),
        }
    }

    /// The JSON grounding of the `nth` occurrence of `needle` in a requirement statement.
    fn ground(text: &str, requirement: &str, needle: &str, nth: usize) -> Value {
        let start = text
            .match_indices(needle)
            .nth(nth)
            .unwrap_or_else(|| panic!("{needle:?} #{nth} not in {text:?}"))
            .0;
        json!({"requirement_ref": requirement, "start": start, "end": start + needle.len()})
    }

    fn r1(needle: &str, nth: usize) -> Value {
        ground(R1, "req:r1", needle, nth)
    }

    fn r2(needle: &str, nth: usize) -> Value {
        ground(R2, "req:r2", needle, nth)
    }

    fn entity_json(concept: &str, grounding: Value) -> Value {
        json!({"concept_ref": concept, "grounding": grounding})
    }

    fn employee_json() -> Value {
        entity_json(EMPLOYEE, r1("Employee", 0))
    }

    fn leave_request_json() -> Value {
        entity_json(LEAVE_REQUEST, r1("Leave Request", 0))
    }

    fn attribute_json(nullable: bool) -> Value {
        json!({
            "concept_ref": START_DATE, "grounding": r2("Start Date", 0),
            "owner_concept_ref": LEAVE_REQUEST, "owner_grounding": r2("Leave Request", 0),
            "value_type": {"concept_ref": DATE, "grounding": r2("Date", 1)},
            "nullable": {"value": nullable, "grounding": r2("mandatory", 0)},
        })
    }

    fn cardinality(value: &str, needle: &str) -> Value {
        json!({"value": value, "grounding": r1(needle, 0)})
    }

    fn relationship_json(from: Value, to: Value) -> Value {
        json!({
            "concept_ref": SUBMITS, "grounding": r1("Employee submits Leave Request", 0),
            "from_concept_ref": EMPLOYEE, "from_grounding": r1("Employee", 0),
            "to_concept_ref": LEAVE_REQUEST, "to_grounding": r1("Leave Request", 0),
            "cardinality_from": from, "cardinality_to": to,
        })
    }

    fn grounded_relationship() -> Value {
        relationship_json(
            cardinality("1", "exactly one"),
            cardinality("0..*", "zero or more"),
        )
    }

    fn output(entities: Vec<Value>, attributes: Vec<Value>, relationships: Vec<Value>) -> Value {
        json!({"version": 1, "entities": entities, "attributes": attributes, "relationships": relationships})
    }

    fn full_output() -> Value {
        output(
            vec![employee_json(), leave_request_json()],
            vec![attribute_json(false)],
            vec![grounded_relationship()],
        )
    }

    fn empty_output() -> Value {
        output(vec![], vec![], vec![])
    }

    fn request(graph: &Graph) -> DomainRequest {
        build_domain_request(graph, provider("mock")).unwrap()
    }

    fn analyze_with(
        graph: &Graph,
        request: &DomainRequest,
        artifact: &InferenceArtifact,
    ) -> Result<DomainAnalysisResult, DomainError> {
        analyze_domain(
            graph,
            request,
            Some(DomainInference {
                artifact,
                derivation_ref: derivation(),
            }),
            &audit(),
        )
    }

    fn analyze(graph: &Graph, output: Value) -> Result<DomainAnalysisResult, DomainError> {
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
        let SemanticPatch::Compound { patches } = &proposal.patch_set.patch else {
            panic!("not a Compound: {:?}", proposal.patch_set.patch);
        };
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

    fn proposal_of_type(result: &DomainAnalysisResult, node_type: NodeType) -> Vec<&Proposal> {
        result
            .proposals
            .iter()
            .filter(|p| match &p.patch_set.patch {
                SemanticPatch::Compound { .. } => added(p).0.payload.node_type() == node_type,
                _ => false,
            })
            .collect()
    }

    fn origin(node: &Node) -> Value {
        node.extensions
            .iter()
            .find(|(k, _)| k.as_str() == DOMAIN_ORIGIN_EXTENSION)
            .map(|(_, v)| v.clone())
            .unwrap()
    }

    fn evidence_of(graph: &Graph, requirements: &[&str]) -> Vec<EvidenceRef> {
        let refs: BTreeSet<EvidenceRef> = requirements
            .iter()
            .flat_map(|r| graph.node(&id(r)).unwrap().evidence.clone())
            .collect();
        refs.into_iter().collect()
    }

    /// The synthetic graph after both Entity proposals were applied in test code.
    fn with_entities() -> Graph {
        let g = base_graph();
        let first = analyze(&g, full_output()).unwrap();
        apply(&g, &first.proposals)
    }

    // ------------------------------------------------------------------ schema and prompt

    fn schema() -> JSONSchema {
        let value: Value = serde_json::from_str(SCHEMA).unwrap();
        JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&value)
            .unwrap()
    }

    #[test]
    fn domain_schema_contract() {
        let value: Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(
            value["$schema"],
            json!("https://json-schema.org/draft/2020-12/schema")
        );
        let s = schema();
        let valid = |v: &Value| s.is_valid(v);
        assert!(valid(&empty_output()));
        assert!(valid(&full_output()));
        let with = |path: &[&str], key: &str, extra: Value| {
            let mut v = full_output();
            let mut target = &mut v;
            for p in path {
                target = match p.parse::<usize>() {
                    Ok(i) => &mut target[i],
                    Err(_) => &mut target[*p],
                };
            }
            target[key] = extra;
            v
        };
        // Unknown fields at every object level, including free model-owned names.
        for (path, key) in [
            (vec![], "extra"),
            (vec!["entities", "0"], "extra"),
            (vec!["entities", "0"], "entity_name"),
            (vec!["entities", "0", "grounding"], "extra"),
            (vec!["attributes", "0"], "extra"),
            (vec!["attributes", "0"], "attribute_name"),
            (vec!["attributes", "0"], "value_type_text"),
            (vec!["attributes", "0", "value_type"], "extra"),
            (vec!["attributes", "0", "nullable"], "extra"),
            (vec!["relationships", "0"], "extra"),
            (vec!["relationships", "0"], "relationship_name"),
            (vec!["relationships", "0"], "normalized_key"),
            (vec!["relationships", "0", "cardinality_to"], "extra"),
        ] {
            assert!(!valid(&with(&path, key, json!("x"))), "{path:?} {key}");
        }
        // A free value_type string instead of a grounded ValueType Concept.
        assert!(!valid(&with(
            &["attributes", "0"],
            "value_type",
            json!("string")
        )));
        // Incomplete Attributes are schema-invalid; the model must omit them instead.
        for field in [
            "owner_concept_ref",
            "owner_grounding",
            "value_type",
            "nullable",
        ] {
            let mut v = full_output();
            v["attributes"][0].as_object_mut().unwrap().remove(field);
            assert!(!valid(&v), "missing {field}");
            let mut v = full_output();
            v["attributes"][0][field] = Value::Null;
            assert!(!valid(&v), "null {field}");
        }
        assert!(!valid(&with(
            &["attributes", "0", "nullable"],
            "value",
            Value::Null
        )));
        assert!(!valid(&with(
            &["attributes", "0", "nullable"],
            "value",
            json!("false")
        )));
        // Cardinality keys are required but nullable.
        assert!(valid(&output(
            vec![],
            vec![],
            vec![relationship_json(Value::Null, Value::Null)]
        )));
        let mut v = full_output();
        v["relationships"][0]
            .as_object_mut()
            .unwrap()
            .remove("cardinality_from");
        assert!(!valid(&v));
        for value in ["0..1", "1", "0..*", "1..*"] {
            assert!(valid(&output(
                vec![],
                vec![],
                vec![relationship_json(
                    cardinality(value, "exactly one"),
                    Value::Null
                )]
            )));
        }
        for value in [
            "0",
            "*",
            "many",
            "unknown",
            "1..n",
            "?",
            "0..?",
            "unresolved",
            "",
        ] {
            assert!(!valid(&output(
                vec![],
                vec![],
                vec![relationship_json(
                    Value::Null,
                    cardinality(value, "exactly one")
                )]
            )));
        }
        assert!(!valid(&with(&[], "version", json!(2))));
        for root in ["version", "entities", "attributes", "relationships"] {
            let mut v = empty_output();
            v.as_object_mut().unwrap().remove(root);
            assert!(!valid(&v), "missing {root}");
        }
        // No external $ref and additionalProperties false on every object.
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
    fn domain_prompt_and_schema_hashes() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
        assert!(PROMPT.ends_with(b"\n"));
        let prompt = std::str::from_utf8(PROMPT).unwrap();
        for required in [
            "Use only supplied Accepted Concept IDs",
            "Do not invent vocabulary",
            "object_type",
            "fact_type",
            "value_type",
            "UTF-8 byte offsets",
            "Do not default nullable",
            "0..1, 1, 0..*, 1..*",
            "Use null when an endpoint cardinality is not stated",
            "Do not guess missing cardinality",
            "Do not infer aggregate roots, units, precision, enumeration values",
            "states, transitions or invariants",
            "Return only JSON",
        ] {
            assert!(prompt.contains(required), "{required}");
        }
        for text in [prompt, SCHEMA] {
            for forbidden in [
                "entity_name",
                "attribute_name",
                "relationship_name",
                "value_type_text",
                "normalized_key",
                "G_REL_NO_CARD",
                "TODO",
                "FIXME",
                "{{",
            ] {
                assert!(!text.contains(forbidden), "{forbidden}");
            }
        }
    }

    // ------------------------------------------------------------------ request

    #[test]
    fn domain_request_golden_and_context() {
        // Proposed Requirements and Concepts are not part of the closed vocabulary context.
        let g = extend(
            &graph_with([R1, R2], false),
            vec![
                requirement_node("req:r0", Proposed, "The system shall draft."),
                concept_node("concept:draft", Proposed, "Draft", "A draft.", ObjectType),
            ],
            vec![],
        );
        let r = request(&g);
        assert_eq!(r.request.id.as_str(), GOLDEN_REQUEST_ID);
        assert_eq!(r.request.context_hash.as_str(), GOLDEN_CONTEXT_HASH);
        assert_eq!(
            r.context.content_hash().unwrap().as_str(),
            GOLDEN_CONTEXT_HASH
        );
        assert_eq!(r.request.stage, StageId::S2);
        assert_eq!(r.request.task_kind, "domain_extraction");
        assert_eq!(r.request.prompt_template_hash.as_str(), PROMPT_HASH);
        assert_eq!(r.request.schema_hash.as_str(), SCHEMA_HASH);
        assert_eq!(r.context.version, 1);
        assert_eq!(r.context.project_id, id(PROJECT));
        assert_eq!(
            r.context
                .requirements
                .iter()
                .map(|x| x.requirement_ref.clone())
                .collect::<Vec<_>>(),
            ids(&["req:r1", "req:r2"])
        );
        assert_eq!(
            r.context
                .concepts
                .iter()
                .map(|x| x.concept_ref.clone())
                .collect::<Vec<_>>(),
            ids(&[DATE, EMPLOYEE, LEAVE_REQUEST, START_DATE, SUBMITS])
        );
        assert_eq!(r.context.concepts[1].concept_kind, ObjectType);
        assert_eq!(
            r.context.concepts[1].definition,
            "A person employed by the organization."
        );
        assert_eq!(
            r.request.input_refs,
            ids(&[
                DATE,
                EMPLOYEE,
                LEAVE_REQUEST,
                START_DATE,
                SUBMITS,
                "req:r1",
                "req:r2"
            ])
        );
        assert!(r.request.evidence_refs.is_empty());
        r.validate().unwrap();
        let wire: Value = serde_json::to_value(&r.context).unwrap();
        assert_eq!(
            wire.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["concepts", "project_id", "requirements", "version"]
        );
        assert_eq!(DOMAIN_CONTEXT_VERSION, 1);
        assert_eq!(DOMAIN_OUTPUT_VERSION, 1);
        assert_eq!(DOMAIN_TASK_KIND, "domain_extraction");
        assert_eq!(DOMAIN_ORIGIN_EXTENSION, "plumb_functional:domain_origin");
        // Tampered requests are rejected.
        let mut tampered = r.clone();
        tampered.context.concepts.pop();
        assert!(matches!(
            tampered.validate(),
            Err(DomainError::InvalidInput { .. })
        ));
    }

    #[test]
    fn domain_request_evidence_and_provider_policy() {
        let g = base_graph();
        let r = request(&g);
        let expected: BTreeSet<Id> = ["req:r1", "req:r2", EMPLOYEE]
            .iter()
            .flat_map(|n| {
                g.node(&id(n))
                    .unwrap()
                    .evidence
                    .iter()
                    .map(|e| e.as_id().clone())
            })
            .collect();
        assert_eq!(expected.len(), 2);
        assert_eq!(
            r.request.evidence_refs,
            expected.into_iter().collect::<Vec<_>>()
        );
        // Provider policy changes the request identity only.
        let other = build_domain_request(&g, provider("other")).unwrap();
        assert_ne!(other.request.id, r.request.id);
        assert_eq!(other.context, r.context);
        assert_eq!(other.request.context_hash, r.request.context_hash);
        let a = analyze_with(&g, &r, &artifact_for(&r.request, full_output())).unwrap();
        let b = analyze_with(&g, &other, &artifact_for(&other.request, full_output())).unwrap();
        let node_ids = |res: &DomainAnalysisResult| -> Vec<Id> {
            res.proposals
                .iter()
                .map(|p| added(p).0.id.clone())
                .collect()
        };
        assert_eq!(node_ids(&a), node_ids(&b));
        let mut entity_ids: Vec<Id> = node_ids(&a);
        entity_ids.sort();
        assert_eq!(
            entity_ids,
            ids(&[EMPLOYEE_ENTITY, LEAVE_REQUEST_ENTITY])
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn domain_request_stale_contexts() {
        let g = base_graph();
        let r = request(&g);
        let artifact = artifact_for(&r.request, full_output());
        let stale = |changed: Graph| {
            assert!(matches!(
                analyze_with(&changed, &r, &artifact),
                Err(DomainError::InvalidInput { .. })
            ));
        };
        let edit = |target: &'static str, f: fn(&mut NodePayload)| {
            map_nodes(&g, move |n| {
                if n.id.as_str() == target {
                    f(&mut n.payload);
                }
            })
        };
        stale(edit("req:r2", |p| {
            if let NodePayload::Requirement(r) = p {
                r.statement.push_str(" Always.");
            }
        }));
        stale(map_nodes(&g, |n| {
            if n.id.as_str() == "req:r2" {
                n.status = Proposed;
            }
        }));
        stale(map_nodes(&g, |n| {
            if n.id.as_str() == DATE {
                n.status = Proposed;
            }
        }));
        stale(edit(DATE, |p| {
            if let NodePayload::Concept(c) = p {
                c.name = "Calendar Date".into();
            }
        }));
        stale(edit(DATE, |p| {
            if let NodePayload::Concept(c) = p {
                c.definition = "A day.".into();
            }
        }));
        stale(edit(DATE, |p| {
            if let NodePayload::Concept(c) = p {
                c.concept_kind = Other;
            }
        }));
        stale(
            Graph::new(
                id("project:other"),
                g.profile_id().clone(),
                g.nodes().values().cloned().collect(),
                vec![],
            )
            .unwrap(),
        );
        // Evidence drift is also rejected.
        stale(map_nodes(&g, |n| {
            if n.id.as_str() == "req:r1" {
                n.evidence.clear();
            }
        }));
    }

    #[test]
    fn domain_request_survives_proposed_domain_nodes() {
        let g = base_graph();
        let r = request(&g);
        let artifact = artifact_for(&r.request, full_output());
        let first = analyze_with(&g, &r, &artifact).unwrap();
        let entity = proposal_of_type(&first, NodeType::Entity)[0];
        let g2 = apply(&g, std::slice::from_ref(entity));
        assert_eq!(request(&g2), r);
        r.validate().unwrap();
        analyze_with(&g2, &r, &artifact).unwrap();
    }

    #[test]
    fn domain_inference_absent_and_empty_vocabulary() {
        let g = base_graph();
        let r = request(&g);
        let absent = analyze_domain(&g, &r, None, &audit()).unwrap();
        assert_eq!(absent.issues, vec![DomainIssue::InferenceUnavailable]);
        assert!(absent.proposals.is_empty());
        assert!(absent.findings.is_empty());
        assert!(absent.conflicts.is_empty());
        assert!(absent.merge_dispositions.is_empty());

        // Accepted Requirements without any Accepted Concept.
        let no_vocab = new_graph(
            PROJECT,
            PROFILE,
            vec![
                requirement_node("req:r1", Accepted, R1),
                requirement_node("req:r2", Accepted, R2),
            ],
            vec![],
        );
        let r = request(&no_vocab);
        assert!(r.context.concepts.is_empty());
        let result = analyze(&no_vocab, empty_output()).unwrap();
        assert_eq!(
            result.issues,
            vec![DomainIssue::AcceptedVocabularyUnavailable]
        );
        assert!(result.proposals.is_empty());
        assert!(result.findings.is_empty());
        // Any candidate must reference an Accepted Concept, so none can exist.
        assert!(matches!(
            analyze(&no_vocab, output(vec![employee_json()], vec![], vec![])),
            Err(DomainError::UnknownConceptRef { .. })
        ));
    }

    // ------------------------------------------------------------------ inference validation

    #[test]
    fn domain_inference_artifact_validation() {
        let g = extend(
            &base_graph(),
            vec![concept_node(
                "concept:draft",
                Proposed,
                "Leave Request",
                "A proposed duplicate.",
                ObjectType,
            )],
            vec![],
        );
        let r = request(&g);
        let good = artifact_for(&r.request, full_output());
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
                Err(DomainError::InvalidInferenceArtifact { .. })
            ));
        }
        let run = |v: Value| analyze_with(&g, &r, &artifact_for(&r.request, v));
        assert!(matches!(
            run(json!({"version": 1, "entities": []})),
            Err(DomainError::SchemaInvalid { .. })
        ));
        // Unknown and Proposed Concepts.
        assert!(matches!(
            run(output(vec![entity_json("concept:ghost", r1("Employee", 0))], vec![], vec![])),
            Err(DomainError::UnknownConceptRef { concept_ref }) if concept_ref == id("concept:ghost")
        ));
        assert!(matches!(
            run(output(
                vec![entity_json("concept:draft", r1("Leave Request", 0))],
                vec![],
                vec![]
            )),
            Err(DomainError::ConceptNotAccepted {
                status: Proposed,
                ..
            })
        ));
        // Wrong ConceptKinds: ValueType as Entity, ObjectType as relationship, FactType as
        // value type, ValueType as owner and as relationship endpoint.
        let wrong_kind = |v: Value, concept: &str, expected: ConceptKind| match run(v) {
            Err(DomainError::WrongConceptKind {
                concept_ref,
                expected: e,
                ..
            }) => {
                assert_eq!(concept_ref, id(concept));
                assert_eq!(e, expected);
            }
            other => panic!("{other:?}"),
        };
        wrong_kind(
            output(vec![entity_json(DATE, r2("Date", 1))], vec![], vec![]),
            DATE,
            ObjectType,
        );
        let mut rel = grounded_relationship();
        rel["concept_ref"] = json!(EMPLOYEE);
        rel["grounding"] = r1("Employee", 0);
        wrong_kind(output(vec![], vec![], vec![rel]), EMPLOYEE, FactType);
        let mut attr = attribute_json(false);
        attr["value_type"] =
            json!({"concept_ref": SUBMITS, "grounding": r1("Employee submits Leave Request", 0)});
        wrong_kind(output(vec![], vec![attr], vec![]), SUBMITS, ValueType);
        let mut attr = attribute_json(false);
        attr["owner_concept_ref"] = json!(DATE);
        attr["owner_grounding"] = r2("Date", 1);
        wrong_kind(output(vec![], vec![attr], vec![]), DATE, ObjectType);
        let mut rel = grounded_relationship();
        rel["to_concept_ref"] = json!(DATE);
        rel["to_grounding"] = r2("Date", 1);
        wrong_kind(output(vec![], vec![], vec![rel]), DATE, ObjectType);
        // The attribute Concept may have any kind.
        let mut attr = attribute_json(false);
        attr["concept_ref"] = json!(DATE);
        attr["grounding"] = r2("Date", 1);
        run(output(vec![], vec![attr], vec![])).unwrap();

        // Grounding ranges.
        let bad_range = |grounding: Value| {
            assert!(matches!(
                run(output(
                    vec![entity_json(EMPLOYEE, grounding)],
                    vec![],
                    vec![]
                )),
                Err(DomainError::InvalidGrounding { .. })
            ));
        };
        bad_range(json!({"requirement_ref": "req:r9", "start": 0, "end": 5}));
        bad_range(json!({"requirement_ref": "req:r1", "start": 6, "end": 6}));
        bad_range(json!({"requirement_ref": "req:r1", "start": 7, "end": 6}));
        bad_range(json!({"requirement_ref": "req:r1", "start": 0, "end": R1.len() + 1}));
        // A grounding naming another Concept.
        assert!(matches!(
            run(output(vec![entity_json(EMPLOYEE, r1("Leave Request", 0))], vec![], vec![])),
            Err(DomainError::GroundingConceptMismatch { concept_ref, .. }) if concept_ref == id(EMPLOYEE)
        ));
        let mut attr = attribute_json(false);
        attr["value_type"]["grounding"] = r2("Start Date", 0);
        assert!(matches!(
            run(output(vec![], vec![attr], vec![])),
            Err(DomainError::GroundingConceptMismatch { concept_ref, .. }) if concept_ref == id(DATE)
        ));
        // A bad nullable or cardinality grounding is also rejected.
        let mut attr = attribute_json(false);
        attr["nullable"]["grounding"] =
            json!({"requirement_ref": "req:r2", "start": 0, "end": 999});
        assert!(matches!(
            run(output(vec![], vec![attr], vec![])),
            Err(DomainError::InvalidGrounding { .. })
        ));
        // Normalization follows S1.5 exactly: determiner and plural are normalized.
        let plural = "Every Employee submits Leave Request records; the employees agree.";
        assert_eq!(
            normalize_vocabulary_term("the employees"),
            normalize_vocabulary_term("Employee")
        );
        let g2 = graph_with([plural, R2], false);
        let r2_ = request(&g2);
        analyze_with(
            &g2,
            &r2_,
            &artifact_for(
                &r2_.request,
                output(
                    vec![entity_json(
                        EMPLOYEE,
                        ground(plural, "req:r1", "the employees", 0),
                    )],
                    vec![],
                    vec![],
                ),
            ),
        )
        .unwrap();

        // Duplicate semantic identities.
        for duplicated in [
            output(
                vec![employee_json(), entity_json(EMPLOYEE, r1("Employee", 1))],
                vec![],
                vec![],
            ),
            output(
                vec![],
                vec![attribute_json(false), attribute_json(true)],
                vec![],
            ),
            output(
                vec![],
                vec![],
                vec![
                    grounded_relationship(),
                    relationship_json(Value::Null, Value::Null),
                ],
            ),
        ] {
            assert!(matches!(
                run(duplicated),
                Err(DomainError::DuplicateCandidate { .. })
            ));
        }
        match run(output(
            vec![employee_json(), employee_json()],
            vec![],
            vec![],
        )) {
            Err(DomainError::DuplicateCandidate { candidate_ref }) => {
                assert_eq!(candidate_ref, id(EMPLOYEE_ENTITY))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn domain_utf8_split_grounding() {
        let accented = "Every Employée submits Leave Request records.";
        let g = graph_with([accented, R2], false);
        let r = request(&g);
        let e = accented.find('é').unwrap();
        let split = json!({"requirement_ref": "req:r1", "start": 6, "end": e + 1});
        assert!(matches!(
            analyze_with(
                &g,
                &r,
                &artifact_for(
                    &r.request,
                    output(vec![entity_json(EMPLOYEE, split)], vec![], vec![])
                )
            ),
            Err(DomainError::InvalidGrounding { .. })
        ));
    }

    // ------------------------------------------------------------------ proposals

    #[test]
    fn domain_entity_proposal_structure() {
        let g = base_graph();
        let result = analyze(&g, full_output()).unwrap();
        let entities = proposal_of_type(&result, NodeType::Entity);
        assert_eq!(entities.len(), 2);
        assert_eq!(result.proposals.len(), 2);
        let employee = entities
            .iter()
            .find(|p| added(p).0.id == id(EMPLOYEE_ENTITY))
            .unwrap();
        let (node, edges) = added(employee);
        assert_eq!(
            node.payload,
            NodePayload::Entity(Entity {
                name: "Employee".into(),
                description: None,
                aggregate_root: None,
            })
        );
        assert_eq!(node.status, Proposed);
        assert_eq!(node.revision, 1);
        assert_eq!(node.evidence, evidence_of(&g, &["req:r1"]));
        assert!(node.derivations.is_empty() && node.standards.is_empty() && node.tags.is_empty());
        assert_eq!(node.audit.created_by, id("agent:domain"));
        assert_eq!(node.extensions.len(), 1);
        assert_eq!(
            origin(node),
            json!({"kind": "entity", "concept_ref": EMPLOYEE, "grounding": r1("Employee", 0)})
        );
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, RelationKind::DerivedFrom);
        assert_eq!(edges[0].from, id(EMPLOYEE_ENTITY));
        assert_eq!(edges[0].to, id(EMPLOYEE));
        assert_eq!(edges[0].status, Proposed);
        assert_eq!(edges[0].properties, RelationProperties::None);
        assert_eq!(edges[0].evidence, node.evidence);
        assert_eq!(employee.stage, StageId::S2);
        assert_eq!(employee.materiality, ProposalMateriality::Semantic);
        assert_eq!(employee.acceptance_policy, AcceptancePolicy::HumanConfirm);
        assert_eq!(employee.confidence, None);
        assert_eq!(employee.derivation_refs, vec![derivation()]);
        assert_eq!(employee.evidence_refs, node.evidence);
        assert_eq!(
            employee.patch_set.base_semantic_hash,
            g.semantic_hash().unwrap()
        );
        employee.validate().unwrap();
        // Applied in test code: a Proposed Entity derived from the Accepted ObjectType Concept.
        let applied = apply(&g, std::slice::from_ref(*employee));
        assert_eq!(applied.node(&id(EMPLOYEE_ENTITY)).unwrap().status, Proposed);
        let edge = applied.edge(&edges[0].id).unwrap();
        assert_eq!(applied.node(&edge.to).unwrap().status, Accepted);
        // Proposals are canonically ordered by ID.
        let order: Vec<&Id> = result.proposals.iter().map(|p| &p.id).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
    }

    #[test]
    fn domain_two_pass_dependency_path() {
        let g = base_graph();
        let r = request(&g);
        let artifact = artifact_for(&r.request, full_output());

        // Pass 1: Entities only; dependents wait for their Entities.
        let first = analyze_with(&g, &r, &artifact).unwrap();
        assert_eq!(proposal_of_type(&first, NodeType::Entity).len(), 2);
        assert!(proposal_of_type(&first, NodeType::Attribute).is_empty());
        assert!(proposal_of_type(&first, NodeType::DomainRelationship).is_empty());
        assert!(first.findings.is_empty());
        assert_eq!(
            first.issues,
            vec![
                DomainIssue::EntityDependencyPending {
                    candidate_ref: id(START_DATE_ATTRIBUTE),
                    concept_refs: ids(&[LEAVE_REQUEST]),
                },
                DomainIssue::EntityDependencyPending {
                    candidate_ref: id(SUBMITS_RELATIONSHIP),
                    concept_refs: ids(&[EMPLOYEE, LEAVE_REQUEST]),
                },
            ]
        );
        // No Entity AddNode is copied into another proposal.
        for p in &first.proposals {
            let (node, _) = added(p);
            assert_eq!(node.payload.node_type(), NodeType::Entity);
        }

        // Apply the Entity proposals in test code; replay the SAME request and artifact.
        let g2 = apply(&g, &first.proposals);
        r.validate().unwrap();
        assert_eq!(request(&g2), r);
        let second = analyze_with(&g2, &r, &artifact).unwrap();
        assert!(proposal_of_type(&second, NodeType::Entity).is_empty());
        assert_eq!(proposal_of_type(&second, NodeType::Attribute).len(), 1);
        assert_eq!(
            proposal_of_type(&second, NodeType::DomainRelationship).len(),
            1
        );
        assert!(second.issues.is_empty());
        assert!(second.findings.is_empty());
        assert_eq!(
            second
                .merge_dispositions
                .iter()
                .map(|d| (d.candidate_ref.as_str(), &d.disposition))
                .collect::<Vec<_>>(),
            vec![
                (START_DATE_ATTRIBUTE, &DomainMergeDisposition::NotDuplicate),
                (SUBMITS_RELATIONSHIP, &DomainMergeDisposition::NotDuplicate),
                (
                    LEAVE_REQUEST_ENTITY,
                    &DomainMergeDisposition::ExistingEquivalent {
                        node_ref: id(LEAVE_REQUEST_ENTITY)
                    }
                ),
                (
                    EMPLOYEE_ENTITY,
                    &DomainMergeDisposition::ExistingEquivalent {
                        node_ref: id(EMPLOYEE_ENTITY)
                    }
                ),
            ]
            .into_iter()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect::<Vec<_>>()
        );
        // Apply the dependent proposals in test code; the final graph validates.
        let g3 = apply(&g2, &second.proposals);
        g3.validate().unwrap();
        assert_eq!(g3.node_ids_by_type(NodeType::Entity).len(), 2);
        assert_eq!(g3.node_ids_by_type(NodeType::Attribute).len(), 1);
        assert_eq!(g3.node_ids_by_type(NodeType::DomainRelationship).len(), 1);
    }

    #[test]
    fn domain_attribute_proposal_structure() {
        let g = with_entities();
        for nullable in [false, true] {
            let result = analyze(
                &g,
                output(
                    vec![employee_json(), leave_request_json()],
                    vec![attribute_json(nullable)],
                    vec![],
                ),
            )
            .unwrap();
            let attributes = proposal_of_type(&result, NodeType::Attribute);
            assert_eq!(attributes.len(), 1);
            let p = attributes[0];
            let (node, edges) = added(p);
            assert_eq!(node.id, id(START_DATE_ATTRIBUTE));
            let NodePayload::Attribute(a) = &node.payload else {
                panic!()
            };
            assert_eq!(a.name, "Start Date");
            // Exactly the Accepted ValueType Concept name: no case change, no dictionary.
            assert_eq!(a.value_type, "Date");
            assert_eq!(a.nullable, nullable);
            assert_eq!(a.unit, None);
            assert_eq!(a.precision, None);
            assert_eq!(a.enum_values, None);
            assert_eq!(a.data_classification, None);
            assert_eq!(node.status, Proposed);
            assert_eq!(node.evidence, evidence_of(&g, &["req:r2"]));
            assert_eq!(
                origin(node),
                json!({
                    "kind": "attribute",
                    "concept_ref": START_DATE, "grounding": r2("Start Date", 0),
                    "owner_concept_ref": LEAVE_REQUEST, "owner_grounding": r2("Leave Request", 0),
                    "value_type_concept_ref": DATE, "value_type_grounding": r2("Date", 1),
                    "nullable_grounding": r2("mandatory", 0),
                })
            );
            let edge_set: BTreeSet<(RelationKind, Id, Id)> = edges
                .iter()
                .map(|e| (e.kind.clone(), e.from.clone(), e.to.clone()))
                .collect();
            assert_eq!(
                edge_set,
                BTreeSet::from([
                    (
                        RelationKind::HasAttribute,
                        id(LEAVE_REQUEST_ENTITY),
                        id(START_DATE_ATTRIBUTE)
                    ),
                    (
                        RelationKind::DerivedFrom,
                        id(START_DATE_ATTRIBUTE),
                        id(START_DATE)
                    ),
                    (
                        RelationKind::DerivedFrom,
                        id(START_DATE_ATTRIBUTE),
                        id(DATE)
                    ),
                ])
            );
            let owner = edges
                .iter()
                .find(|e| e.kind == RelationKind::HasAttribute)
                .unwrap();
            assert_eq!(owner.id, id(HAS_ATTRIBUTE_EDGE));
            let edge_ids: Vec<&Id> = edges.iter().map(|e| &e.id).collect();
            let mut sorted = edge_ids.clone();
            sorted.sort();
            assert_eq!(edge_ids, sorted);
            assert_eq!(p.materiality, ProposalMateriality::Semantic);
            assert_eq!(p.acceptance_policy, AcceptancePolicy::HumanConfirm);
            assert_eq!(p.stage, StageId::S2);
            // Applied: exactly one incoming has_attribute from the active Entity.
            let applied = apply(&g, std::slice::from_ref(p));
            let incoming: Vec<&Edge> = applied
                .incoming_edge_ids(&id(START_DATE_ATTRIBUTE))
                .iter()
                .map(|e| applied.edge(e).unwrap())
                .filter(|e| e.kind == RelationKind::HasAttribute)
                .collect();
            assert_eq!(incoming.len(), 1);
            assert_eq!(incoming[0].from, id(LEAVE_REQUEST_ENTITY));
        }
        // An attribute whose own Concept is its value type has one derived_from edge.
        let mut same = attribute_json(false);
        same["concept_ref"] = json!(DATE);
        same["grounding"] = r2("Date", 1);
        let result = analyze(&g, output(vec![], vec![same], vec![])).unwrap();
        let (_, edges) = added(proposal_of_type(&result, NodeType::Attribute)[0]);
        assert_eq!(
            edges
                .iter()
                .filter(|e| e.kind == RelationKind::DerivedFrom)
                .count(),
            1
        );
    }

    #[test]
    fn domain_relationship_proposal_structure() {
        let g = with_entities();
        let result = analyze(&g, output(vec![], vec![], vec![grounded_relationship()])).unwrap();
        let relationships = proposal_of_type(&result, NodeType::DomainRelationship);
        assert_eq!(relationships.len(), 1);
        let p = relationships[0];
        let (node, edges) = added(p);
        assert_eq!(node.id, id(SUBMITS_RELATIONSHIP));
        let NodePayload::DomainRelationship(rel) = &node.payload else {
            panic!()
        };
        assert_eq!(rel.from_entity, id(EMPLOYEE_ENTITY));
        assert_eq!(rel.to_entity, id(LEAVE_REQUEST_ENTITY));
        assert_eq!(rel.relationship_kind, "Employee submits Leave Request");
        assert_eq!(rel.cardinality_from, "1");
        assert_eq!(rel.cardinality_to, "0..*");
        assert_eq!(rel.name, None);
        assert_eq!(rel.snapshot_semantics, None);
        assert_eq!(rel.ownership, None);
        assert_eq!(node.status, Proposed);
        assert_eq!(
            origin(node),
            json!({
                "kind": "domain_relationship",
                "concept_ref": SUBMITS, "grounding": r1("Employee submits Leave Request", 0),
                "from_concept_ref": EMPLOYEE, "from_grounding": r1("Employee", 0),
                "to_concept_ref": LEAVE_REQUEST, "to_grounding": r1("Leave Request", 0),
                "cardinality_from_grounding": r1("exactly one", 0),
                "cardinality_to_grounding": r1("zero or more", 0),
                "cardinality_decision_ref": null,
            })
        );
        assert_eq!(edges.len(), 1);
        assert_eq!(
            (&edges[0].kind, &edges[0].from, &edges[0].to),
            (
                &RelationKind::DerivedFrom,
                &id(SUBMITS_RELATIONSHIP),
                &id(SUBMITS)
            )
        );
        assert_eq!(p.materiality, ProposalMateriality::Semantic);
        assert_eq!(p.acceptance_policy, AcceptancePolicy::HumanConfirm);
        let applied = apply(&g, std::slice::from_ref(p));
        assert!(applied.node(&id(SUBMITS_RELATIONSHIP)).is_some());
        assert!(result.findings.is_empty());
    }

    // ------------------------------------------------------------------ cardinality

    fn missing_cardinality(g: &Graph, from: Value, to: Value) -> DomainAnalysisResult {
        analyze(g, output(vec![], vec![], vec![relationship_json(from, to)])).unwrap()
    }

    #[test]
    fn domain_missing_cardinality_finding() {
        let g = with_entities();
        let both = missing_cardinality(&g, Value::Null, Value::Null);
        assert!(proposal_of_type(&both, NodeType::DomainRelationship).is_empty());
        assert!(both.proposals.is_empty());
        assert_eq!(both.findings.len(), 1);
        let f: &GeneratedFinding = &both.findings[0];
        assert_eq!(f.key.as_str(), CARDINALITY_FINDING_KEY);
        assert_eq!(f.id.as_str(), CARDINALITY_FINDING_ID);
        assert_eq!(
            f.semantic_condition_key,
            format!("domain_relationship_cardinality_unresolved:{SUBMITS_RELATIONSHIP}")
        );
        assert_eq!(f.payload.code, RELATION_TYPED);
        assert_eq!(f.payload.family, "F2");
        assert_eq!(f.payload.severity, FindingSeverity::Error);
        assert_eq!(f.payload.status, "Open");
        assert_eq!(f.payload.affected_refs, ids(&["req:r1"]));
        assert_eq!(
            f.payload.message,
            format!(
                "Domain relationship candidate {SUBMITS_RELATIONSHIP} has unresolved endpoint cardinality."
            )
        );
        assert_eq!(
            f.payload.suggested_resolution.as_deref(),
            Some("Provide explicit cardinality evidence or record a governed domain relationship cardinality decision.")
        );
        assert_eq!(f.payload.waiver_ref, None);
        // One missing endpoint is the same unresolved condition; no half-complete node.
        for (from, to) in [
            (cardinality("1", "exactly one"), Value::Null),
            (Value::Null, cardinality("0..*", "zero or more")),
        ] {
            let one = missing_cardinality(&g, from, to);
            assert!(one.proposals.is_empty());
            assert_eq!(one.findings, both.findings);
        }
        // Targets are the sorted unique Requirements grounding the FactType and endpoints.
        let mut spread = relationship_json(Value::Null, Value::Null);
        spread["to_grounding"] = r2("Leave Request", 0);
        let result = analyze(&g, output(vec![], vec![], vec![spread])).unwrap();
        assert_eq!(
            result.findings[0].payload.affected_refs,
            ids(&["req:r1", "req:r2"])
        );
        // A pending endpoint produces no finding yet.
        let pending = missing_cardinality(&base_graph(), Value::Null, Value::Null);
        assert!(pending.findings.is_empty());
        assert_eq!(pending.issues.len(), 1);
    }

    #[test]
    fn domain_profile_must_match_for_finding_material() {
        let g = with_entities();
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
                output(
                    vec![],
                    vec![],
                    vec![relationship_json(Value::Null, Value::Null)]
                )
            ),
            Err(DomainError::ValidationProfile { .. })
        ));
    }

    #[test]
    fn domain_cardinality_vocabulary() {
        let all: Vec<&str> = DomainCardinality::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(all, ["0..1", "1", "0..*", "1..*"]);
        for c in DomainCardinality::ALL {
            assert_eq!(DomainCardinality::parse(c.as_str()).unwrap(), c);
            assert_eq!(serde_json::to_value(c).unwrap(), json!(c.as_str()));
        }
        for bad in [
            "0",
            "*",
            "many",
            "unknown",
            "1..n",
            "?",
            "unresolved",
            "0..?",
            " 1",
            "",
        ] {
            assert!(matches!(
                DomainCardinality::parse(bad),
                Err(DomainError::InvalidCardinality { .. })
            ));
            assert!(serde_json::from_value::<DomainCardinality>(json!(bad)).is_err());
        }
    }

    // ------------------------------------------------------------------ governed decisions

    const HUMAN: &str = "agent:analyst";
    const QUESTION: &str = "question:cardinality";
    const DECISION: &str = "decision:cardinality";

    fn marker(candidate: &str, from: &str, to: &str) -> Value {
        json!({"kind": "domain_relationship_cardinality", "candidate_ref": candidate,
               "cardinality_from": from, "cardinality_to": to})
    }

    fn decision_node(
        node_id: &str,
        status: ElementStatus,
        answer: Value,
        rationale: Option<&str>,
        by: &str,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::ResolutionDecision(ResolutionDecision {
                question_ref: Some(id(QUESTION)),
                proposal_ref: None,
                answer,
                decided_by: id(by),
                decided_at: ts(AT),
                patch_ref: Hash::content_sha256(b"cardinality patch"),
                rationale: rationale.map(str::to_owned),
                supersedes: None,
            }),
        )
    }

    fn resolves(edge_id: &str, from: &str, to: &str) -> Edge {
        Edge {
            id: id(edge_id),
            revision: 1,
            status: Accepted,
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

    /// The entity graph with the materialized finding, a human Agent, a Cardinality Question
    /// and the given decisions/edges. `via_question` routes the default decision's resolves
    /// edge through the Question instead of directly to the Finding.
    fn governed_graph(
        finding: &GeneratedFinding,
        decisions: Vec<Node>,
        edges: Vec<Edge>,
        agent_kind: AgentKind,
    ) -> Graph {
        let g = with_entities();
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
                    question_kind: QuestionKind::Cardinality,
                    prompt: "What are the endpoint cardinalities?".into(),
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

    fn the_finding() -> GeneratedFinding {
        missing_cardinality(&with_entities(), Value::Null, Value::Null).findings[0].clone()
    }

    fn decided_relationship(result: &DomainAnalysisResult) -> (String, String, Value) {
        let p = proposal_of_type(result, NodeType::DomainRelationship)[0];
        let (node, _) = added(p);
        let NodePayload::DomainRelationship(rel) = &node.payload else {
            panic!()
        };
        (
            rel.cardinality_from.clone(),
            rel.cardinality_to.clone(),
            origin(node),
        )
    }

    #[test]
    fn domain_cardinality_decision_resolves_candidate() {
        let finding = the_finding();
        for via_question in [true, false] {
            let target = if via_question {
                QUESTION
            } else {
                finding.id.as_str()
            };
            let g = governed_graph(
                &finding,
                vec![decision_node(
                    DECISION,
                    Accepted,
                    marker(SUBMITS_RELATIONSHIP, "1", "0..*"),
                    Some("Confirmed with HR."),
                    HUMAN,
                )],
                vec![resolves("rel:resolves-1", DECISION, target)],
                AgentKind::Human,
            );
            let result = missing_cardinality(&g, Value::Null, Value::Null);
            assert!(result.findings.is_empty());
            let (from, to, origin) = decided_relationship(&result);
            assert_eq!((from.as_str(), to.as_str()), ("1", "0..*"));
            assert_eq!(origin["cardinality_from_grounding"], Value::Null);
            assert_eq!(origin["cardinality_to_grounding"], Value::Null);
            assert_eq!(origin["cardinality_decision_ref"], json!(DECISION));
            let applied = apply(&g, &result.proposals);
            assert!(applied.node(&id(SUBMITS_RELATIONSHIP)).is_some());
        }
        // Accepted human governance outranks grounded inference; no conflict is raised.
        let g = governed_graph(
            &finding,
            vec![decision_node(
                DECISION,
                Accepted,
                marker(SUBMITS_RELATIONSHIP, "0..1", "1"),
                Some("Confirmed with HR."),
                HUMAN,
            )],
            vec![resolves("rel:resolves-1", DECISION, QUESTION)],
            AgentKind::Human,
        );
        let result = analyze(&g, output(vec![], vec![], vec![grounded_relationship()])).unwrap();
        let (from, to, origin) = decided_relationship(&result);
        assert_eq!((from.as_str(), to.as_str()), ("0..1", "1"));
        assert_eq!(origin["cardinality_decision_ref"], json!(DECISION));
        assert!(result.findings.is_empty());
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn domain_invalid_cardinality_decisions() {
        let finding = the_finding();
        // A Proposed decision may only carry a Proposed resolves edge.
        let run = |decision: Node, agent_kind: AgentKind, target: &str| {
            let mut edge = resolves("rel:resolves-1", DECISION, target);
            edge.status = decision.status;
            let g = governed_graph(&finding, vec![decision], vec![edge], agent_kind);
            missing_cardinality(&g, Value::Null, Value::Null)
        };
        let try_run = |decision: Node, agent_kind: AgentKind, target: &str| {
            let g = governed_graph(
                &finding,
                vec![decision],
                vec![resolves("rel:resolves-1", DECISION, target)],
                agent_kind,
            );
            analyze(
                &g,
                output(
                    vec![],
                    vec![],
                    vec![relationship_json(Value::Null, Value::Null)],
                ),
            )
        };
        let good =
            |answer: Value| decision_node(DECISION, Accepted, answer, Some("Confirmed."), HUMAN);
        assert!(matches!(
            try_run(
                good(marker(SUBMITS_RELATIONSHIP, "many", "1")),
                AgentKind::Human,
                QUESTION
            ),
            Err(DomainError::InvalidCardinality { .. })
        ));
        assert!(matches!(
            try_run(
                good(marker(SUBMITS_RELATIONSHIP, "1", "")),
                AgentKind::Human,
                QUESTION
            ),
            Err(DomainError::InvalidCardinality { .. })
        ));
        // Decisions naming a candidate that is not in the current inference are ignored, even
        // when they resolve a current candidate's finding: the finding simply remains.
        for other in ["domainrel:0000000000000000", EMPLOYEE_ENTITY] {
            let ignored = run(good(marker(other, "1", "1")), AgentKind::Human, QUESTION);
            assert_eq!(ignored.findings.len(), 1);
            assert!(ignored.proposals.is_empty());
        }
        // Non-human decider, missing or padded rationale, extra or missing answer fields.
        assert!(matches!(
            try_run(
                good(marker(SUBMITS_RELATIONSHIP, "1", "1")),
                AgentKind::LlmModel,
                QUESTION
            ),
            Err(DomainError::InvalidGovernedDecision { .. })
        ));
        for rationale in [None, Some(""), Some(" padded")] {
            assert!(matches!(
                try_run(
                    decision_node(
                        DECISION,
                        Accepted,
                        marker(SUBMITS_RELATIONSHIP, "1", "1"),
                        rationale,
                        HUMAN
                    ),
                    AgentKind::Human,
                    QUESTION
                ),
                Err(DomainError::InvalidGovernedDecision { .. })
            ));
        }
        let mut extra = marker(SUBMITS_RELATIONSHIP, "1", "1");
        extra["note"] = json!("x");
        assert!(matches!(
            try_run(good(extra), AgentKind::Human, QUESTION),
            Err(DomainError::InvalidGovernedDecision { .. })
        ));
        let mut partial = marker(SUBMITS_RELATIONSHIP, "1", "1");
        partial.as_object_mut().unwrap().remove("cardinality_to");
        assert!(matches!(
            try_run(good(partial), AgentKind::Human, QUESTION),
            Err(DomainError::InvalidGovernedDecision { .. })
        ));
        // A decision resolving a Finding of another code is ungoverned for this marker.
        let mut other_code = finding.clone();
        other_code.payload.code = "PLUMB.F1.REQ.TERMS_RESOLVED".into();
        let g = extend(
            &with_entities(),
            vec![
                node(
                    "fnd:00000000000000ff",
                    Accepted,
                    NodePayload::Finding(other_code.payload),
                ),
                node(
                    HUMAN,
                    Accepted,
                    NodePayload::Agent(Agent {
                        agent_kind: AgentKind::Human,
                    }),
                ),
                decision_node(
                    DECISION,
                    Accepted,
                    marker(SUBMITS_RELATIONSHIP, "1", "1"),
                    Some("Confirmed."),
                    HUMAN,
                ),
            ],
            vec![resolves("rel:resolves-1", DECISION, "fnd:00000000000000ff")],
        );
        let g = map_nodes(&g, |n| {
            if let NodePayload::ResolutionDecision(d) = &mut n.payload {
                d.question_ref = None;
                d.proposal_ref = Some(id("prop:00000000000000aa"));
            }
        });
        assert!(matches!(
            analyze(
                &g,
                output(
                    vec![],
                    vec![],
                    vec![relationship_json(Value::Null, Value::Null)]
                )
            ),
            Err(DomainError::InvalidGovernedDecision { .. })
        ));
        // A non-Accepted decision is not governance: the finding remains.
        let proposed = run(
            decision_node(
                DECISION,
                Proposed,
                marker(SUBMITS_RELATIONSHIP, "1", "1"),
                Some("Draft."),
                HUMAN,
            ),
            AgentKind::Human,
            QUESTION,
        );
        assert_eq!(proposed.findings.len(), 1);
        assert!(proposed.proposals.is_empty());
    }

    /// The reverse relationship candidate (Leave Request -> Employee) of the same FactType.
    fn reverse_relationship() -> Value {
        json!({
            "concept_ref": SUBMITS, "grounding": r1("Employee submits Leave Request", 0),
            "from_concept_ref": LEAVE_REQUEST, "from_grounding": r1("Leave Request", 0),
            "to_concept_ref": EMPLOYEE, "to_grounding": r1("Employee", 0),
            "cardinality_from": null, "cardinality_to": null,
        })
    }

    #[test]
    fn domain_cross_candidate_decision_is_rejected() {
        let finding = the_finding();
        // The reverse candidate's ID, read from its pending-cardinality finding (test data).
        let reverse = missing_cardinality_of(&with_entities(), reverse_relationship());
        // A decision naming the reverse (current) candidate but resolving the forward
        // candidate's finding: no cross-candidate resolution.
        let g = governed_graph(
            &finding,
            vec![decision_node(
                DECISION,
                Accepted,
                marker(reverse.as_str(), "1", "1"),
                Some("Confirmed."),
                HUMAN,
            )],
            vec![resolves("rel:resolves-1", DECISION, QUESTION)],
            AgentKind::Human,
        );
        assert!(matches!(
            analyze(
                &g,
                output(
                    vec![],
                    vec![],
                    vec![
                        relationship_json(Value::Null, Value::Null),
                        reverse_relationship()
                    ]
                )
            ),
            Err(DomainError::InvalidGovernedDecision { .. })
        ));
    }

    fn missing_cardinality_of(g: &Graph, relationship: Value) -> Id {
        let result = analyze(g, output(vec![], vec![], vec![relationship])).unwrap();
        let key = &result.findings[0].semantic_condition_key;
        id(key
            .strip_prefix("domain_relationship_cardinality_unresolved:")
            .unwrap())
    }

    #[test]
    fn domain_unrelated_cardinality_decision_is_ignored() {
        // An Accepted, malformed and ungoverned decision for a candidate X that is not in the
        // current inference: no rationale, non-canonical values, an unknown decider, and its
        // only resolves edge targets a Finding of another rule.
        let stale = node(
            "decision:stale",
            Accepted,
            NodePayload::ResolutionDecision(ResolutionDecision {
                question_ref: None,
                proposal_ref: Some(id("prop:00000000000000bb")),
                answer: marker("domainrel:00000000000000aa", "many", "?"),
                decided_by: id("agent:nobody"),
                decided_at: ts(AT),
                patch_ref: Hash::content_sha256(b"stale"),
                rationale: None,
                supersedes: None,
            }),
        );
        let mut unrelated = the_finding().payload;
        unrelated.code = "PLUMB.F1.REQ.TERMS_RESOLVED".into();
        let g = extend(
            &with_entities(),
            vec![
                stale,
                node(
                    "fnd:00000000000000cc",
                    Accepted,
                    NodePayload::Finding(unrelated),
                ),
            ],
            vec![resolves(
                "rel:stale-resolves",
                "decision:stale",
                "fnd:00000000000000cc",
            )],
        );
        // The current candidate Y is evaluated normally.
        let result = analyze(&g, output(vec![], vec![], vec![grounded_relationship()])).unwrap();
        assert_eq!(
            proposal_of_type(&result, NodeType::DomainRelationship).len(),
            1
        );
        let result = missing_cardinality(&g, Value::Null, Value::Null);
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].id.as_str(), CARDINALITY_FINDING_ID);
    }

    #[test]
    fn domain_ambiguous_cardinality_decisions() {
        let finding = the_finding();
        let g = governed_graph(
            &finding,
            vec![
                decision_node(
                    DECISION,
                    Accepted,
                    marker(SUBMITS_RELATIONSHIP, "1", "0..*"),
                    Some("First."),
                    HUMAN,
                ),
                decision_node(
                    "decision:cardinality-2",
                    Accepted,
                    marker(SUBMITS_RELATIONSHIP, "1", "1..*"),
                    Some("Second."),
                    HUMAN,
                ),
            ],
            vec![
                resolves("rel:resolves-1", DECISION, QUESTION),
                resolves("rel:resolves-2", "decision:cardinality-2", QUESTION),
            ],
            AgentKind::Human,
        );
        match analyze(&g, output(vec![], vec![], vec![grounded_relationship()])) {
            Err(DomainError::AmbiguousCardinalityDecision {
                candidate_ref,
                decision_refs,
            }) => {
                assert_eq!(candidate_ref, id(SUBMITS_RELATIONSHIP));
                assert_eq!(decision_refs, ids(&[DECISION, "decision:cardinality-2"]));
            }
            other => panic!("{other:?}"),
        }
    }

    // ------------------------------------------------------------------ reconciliation

    #[test]
    fn domain_idempotent_replay() {
        let g = base_graph();
        let r = request(&g);
        let artifact = artifact_for(&r.request, full_output());
        let g2 = apply(&g, &analyze_with(&g, &r, &artifact).unwrap().proposals);
        let g3 = apply(&g2, &analyze_with(&g2, &r, &artifact).unwrap().proposals);
        let replay = analyze_with(&g3, &r, &artifact).unwrap();
        assert!(replay.proposals.is_empty());
        assert!(replay.findings.is_empty());
        assert!(replay.issues.is_empty());
        assert!(replay.conflicts.is_empty());
        let reused: BTreeMap<Id, DomainMergeDisposition> = replay
            .merge_dispositions
            .iter()
            .map(|d| (d.candidate_ref.clone(), d.disposition.clone()))
            .collect();
        for candidate in [
            EMPLOYEE_ENTITY,
            LEAVE_REQUEST_ENTITY,
            START_DATE_ATTRIBUTE,
            SUBMITS_RELATIONSHIP,
        ] {
            assert_eq!(
                reused[&id(candidate)],
                DomainMergeDisposition::ExistingEquivalent {
                    node_ref: id(candidate)
                }
            );
        }
    }

    fn domain_node(
        node_id: &str,
        status: ElementStatus,
        payload: NodePayload,
        origin: Value,
    ) -> Node {
        let mut n = node(node_id, status, payload);
        n.extensions
            .insert(DOMAIN_ORIGIN_EXTENSION.parse().unwrap(), origin);
        n
    }

    fn employee_origin() -> Value {
        json!({"kind": "entity", "concept_ref": EMPLOYEE, "grounding": r1("Employee", 0)})
    }

    fn employee_entity(name: &str) -> NodePayload {
        NodePayload::Entity(Entity {
            name: name.into(),
            description: None,
            aggregate_root: None,
        })
    }

    #[test]
    fn domain_existing_candidate_conflicts() {
        let g = base_graph();
        // Different payload at the deterministic Entity ID.
        let occupied = extend(
            &g,
            vec![domain_node(
                EMPLOYEE_ENTITY,
                Proposed,
                employee_entity("Staff"),
                employee_origin(),
            )],
            vec![],
        );
        assert!(matches!(
            analyze(&occupied, output(vec![employee_json()], vec![], vec![])),
            Err(DomainError::ExistingCandidateConflict { node_ref }) if node_ref == id(EMPLOYEE_ENTITY)
        ));
        // Another node type or origin identity at the deterministic Attribute ID.
        let occupied = extend(
            &with_entities(),
            vec![domain_node(
                START_DATE_ATTRIBUTE,
                Proposed,
                employee_entity("Employee"),
                employee_origin(),
            )],
            vec![],
        );
        assert!(matches!(
            analyze(
                &occupied,
                output(vec![], vec![attribute_json(false)], vec![])
            ),
            Err(DomainError::ExistingCandidateConflict { .. })
        ));
        // A node without domain origin at the deterministic relationship ID.
        let occupied = extend(
            &with_entities(),
            vec![node(
                SUBMITS_RELATIONSHIP,
                Proposed,
                employee_entity("Employee"),
            )],
            vec![],
        );
        assert!(matches!(
            analyze(
                &occupied,
                output(vec![], vec![], vec![grounded_relationship()])
            ),
            Err(DomainError::ExistingCandidateConflict { .. })
        ));
        // Nothing was overwritten: the analysis returns an error, never a replacing patch.
    }

    const MERGE_UNAVAILABLE: &str = "MergeNodes is restricted to Requirement or Term nodes; domain-node consolidation is unavailable in S2.1.";

    fn patch_ops(p: &SemanticPatch, out: &mut Vec<String>) {
        match p {
            SemanticPatch::Compound { patches } => patches.iter().for_each(|x| patch_ops(x, out)),
            other => out.push(
                serde_json::to_value(other).unwrap()["op"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            ),
        }
    }

    /// The duplicate identity is reported, never merged, selected, deleted or rewired.
    fn assert_duplicate_unavailable(
        result: &DomainAnalysisResult,
        candidate: &str,
        node_refs: &[&str],
    ) {
        assert!(result
            .conflicts
            .contains(&DomainConflict::DuplicateDomainOrigin {
                candidate_ref: id(candidate),
                node_refs: ids(node_refs),
            }));
        let outcomes: Vec<&DomainMergeOutcome> = result
            .merge_dispositions
            .iter()
            .filter(|d| d.candidate_ref == id(candidate))
            .collect();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].node_refs, ids(node_refs));
        assert_eq!(
            outcomes[0].disposition,
            DomainMergeDisposition::UnavailablePatchConstraint {
                reason: MERGE_UNAVAILABLE.into()
            }
        );
        // No keeper is exposed in the wire form.
        let wire = serde_json::to_value(outcomes[0]).unwrap();
        assert_eq!(
            wire.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["candidate_ref", "disposition", "node_refs"]
        );
        assert_eq!(
            wire["disposition"],
            json!({"disposition": "unavailable_patch_constraint", "reason": MERGE_UNAVAILABLE})
        );
        for p in &result.proposals {
            let mut ops = Vec::new();
            patch_ops(&p.patch_set.patch, &mut ops);
            assert!(
                ops.iter().all(|op| op == "AddNode" || op == "AddEdge"),
                "{ops:?}"
            );
            let touched = serde_json::to_string(&p.patch_set).unwrap();
            for n in node_refs {
                assert!(!touched.contains(&format!("\"id\":\"{n}\"")), "{n}");
            }
        }
    }

    fn duplicate_entities(a: ElementStatus, b: ElementStatus, reversed: bool) -> Graph {
        let mut nodes = vec![
            domain_node(
                "entity:dup-a",
                a,
                employee_entity("Employee"),
                employee_origin(),
            ),
            domain_node(
                "entity:dup-b",
                b,
                employee_entity("Employee"),
                employee_origin(),
            ),
        ];
        let mut all: Vec<Node> = base_graph().nodes().values().cloned().collect();
        all.append(&mut nodes);
        if reversed {
            all.reverse();
        }
        new_graph(PROJECT, PROFILE, all, vec![])
    }

    #[test]
    fn domain_duplicate_origin_reports_merge_unavailable() {
        let g = duplicate_entities(Proposed, Proposed, false);
        let result = analyze(
            &g,
            output(vec![employee_json()], vec![], vec![grounded_relationship()]),
        )
        .unwrap();
        assert!(result.proposals.is_empty());
        assert!(result.findings.is_empty());
        assert_duplicate_unavailable(&result, EMPLOYEE_ENTITY, &["entity:dup-a", "entity:dup-b"]);
        // The ambiguous Entity blocks the dependent relationship; no cardinality finding.
        assert!(result
            .issues
            .contains(&DomainIssue::EntityDependencyConflicted {
                candidate_ref: id(SUBMITS_RELATIONSHIP),
                concept_refs: ids(&[EMPLOYEE]),
            }));
        let attribute_blocked = analyze(
            &duplicate_entities(Proposed, Proposed, false),
            output(vec![], vec![attribute_json(false)], vec![]),
        )
        .unwrap();
        assert!(attribute_blocked
            .issues
            .contains(&DomainIssue::EntityDependencyPending {
                candidate_ref: id(START_DATE_ATTRIBUTE),
                concept_refs: ids(&[LEAVE_REQUEST]),
            }));
        // Reversed statuses and insertion order: still no keeper and no proposal, and the
        // conflict output is identical in shape.
        for (a, b, reversed) in [
            (Accepted, Proposed, true),
            (Proposed, Accepted, false),
            (Accepted, Accepted, true),
        ] {
            let result = analyze(
                &duplicate_entities(a, b, reversed),
                output(vec![employee_json()], vec![], vec![]),
            )
            .unwrap();
            assert!(result.proposals.is_empty());
            assert_duplicate_unavailable(
                &result,
                EMPLOYEE_ENTITY,
                &["entity:dup-a", "entity:dup-b"],
            );
        }
        let x = analyze(
            &duplicate_entities(Accepted, Proposed, true),
            output(vec![employee_json()], vec![], vec![]),
        )
        .unwrap();
        let y = analyze(
            &duplicate_entities(Accepted, Proposed, false),
            output(vec![employee_json()], vec![], vec![]),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_vec(&x).unwrap(),
            serde_json::to_vec(&y).unwrap()
        );
    }

    fn has_attribute(edge_id: &str, from: &str, to: &str) -> Edge {
        Edge {
            id: id(edge_id),
            revision: 1,
            status: Proposed,
            kind: RelationKind::HasAttribute,
            from: id(from),
            to: id(to),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: AuditMeta::new(id("agent:domain"), ts(AT), None, None).unwrap(),
        }
    }

    fn start_date_payload() -> NodePayload {
        NodePayload::Attribute(plumb_psg::Attribute {
            name: "Start Date".into(),
            value_type: "Date".into(),
            nullable: false,
            unit: None,
            precision: None,
            enum_values: None,
            data_classification: None,
        })
    }

    fn start_date_origin() -> Value {
        json!({
            "kind": "attribute",
            "concept_ref": START_DATE, "grounding": r2("Start Date", 0),
            "owner_concept_ref": LEAVE_REQUEST, "owner_grounding": r2("Leave Request", 0),
            "value_type_concept_ref": DATE, "value_type_grounding": r2("Date", 1),
            "nullable_grounding": r2("mandatory", 0),
        })
    }

    #[test]
    fn domain_duplicate_attribute_origin() {
        let attributes = |second_owner: &str| {
            extend(
                &with_entities(),
                vec![
                    domain_node(
                        "attr:dup-a",
                        Proposed,
                        start_date_payload(),
                        start_date_origin(),
                    ),
                    domain_node(
                        "attr:dup-b",
                        Proposed,
                        start_date_payload(),
                        start_date_origin(),
                    ),
                ],
                vec![
                    has_attribute("rel:own-a", LEAVE_REQUEST_ENTITY, "attr:dup-a"),
                    has_attribute("rel:own-b", second_owner, "attr:dup-b"),
                ],
            )
        };
        // Same payload and same canonical owner: duplicate, merge unavailable, no proposal.
        let result = analyze(
            &attributes(LEAVE_REQUEST_ENTITY),
            output(vec![], vec![attribute_json(false)], vec![]),
        )
        .unwrap();
        assert!(result.proposals.is_empty());
        assert_duplicate_unavailable(&result, START_DATE_ATTRIBUTE, &["attr:dup-a", "attr:dup-b"]);
        assert!(!result
            .conflicts
            .iter()
            .any(|c| matches!(c, DomainConflict::ExistingOriginSemanticConflict { .. })));
        // A different owner is never masked as an equivalent duplicate.
        let result = analyze(
            &attributes(EMPLOYEE_ENTITY),
            output(vec![], vec![attribute_json(false)], vec![]),
        )
        .unwrap();
        assert!(result.proposals.is_empty());
        assert!(result
            .conflicts
            .contains(&DomainConflict::ExistingOriginSemanticConflict {
                candidate_ref: id(START_DATE_ATTRIBUTE),
                node_refs: ids(&["attr:dup-a", "attr:dup-b"]),
            }));
        let outcome = result
            .merge_dispositions
            .iter()
            .find(|d| d.candidate_ref == id(START_DATE_ATTRIBUTE))
            .unwrap();
        assert_eq!(
            outcome.disposition,
            DomainMergeDisposition::UnavailableOwnerDifference
        );
    }

    #[test]
    fn domain_duplicate_relationship_origin() {
        let payload = NodePayload::DomainRelationship(plumb_psg::DomainRelationship {
            from_entity: id(EMPLOYEE_ENTITY),
            to_entity: id(LEAVE_REQUEST_ENTITY),
            relationship_kind: "Employee submits Leave Request".into(),
            cardinality_from: "1".into(),
            cardinality_to: "0..*".into(),
            name: None,
            snapshot_semantics: None,
            ownership: None,
        });
        let origin = json!({
            "kind": "domain_relationship",
            "concept_ref": SUBMITS, "grounding": r1("Employee submits Leave Request", 0),
            "from_concept_ref": EMPLOYEE, "from_grounding": r1("Employee", 0),
            "to_concept_ref": LEAVE_REQUEST, "to_grounding": r1("Leave Request", 0),
            "cardinality_from_grounding": r1("exactly one", 0),
            "cardinality_to_grounding": r1("zero or more", 0),
            "cardinality_decision_ref": null,
        });
        let g = extend(
            &with_entities(),
            vec![
                domain_node("domainrel:dup-a", Proposed, payload.clone(), origin.clone()),
                domain_node("domainrel:dup-b", Accepted, payload, origin),
            ],
            vec![],
        );
        for out in [
            output(vec![], vec![], vec![grounded_relationship()]),
            output(
                vec![],
                vec![],
                vec![relationship_json(Value::Null, Value::Null)],
            ),
        ] {
            let result = analyze(&g, out).unwrap();
            assert!(result.proposals.is_empty());
            assert!(result.findings.is_empty());
            assert_duplicate_unavailable(
                &result,
                SUBMITS_RELATIONSHIP,
                &["domainrel:dup-a", "domainrel:dup-b"],
            );
        }
    }

    #[test]
    fn domain_payload_disagreement_is_a_conflict() {
        let g = extend(
            &base_graph(),
            vec![
                domain_node(
                    "entity:dup-a",
                    Proposed,
                    employee_entity("Employee"),
                    employee_origin(),
                ),
                domain_node(
                    "entity:dup-b",
                    Proposed,
                    employee_entity("Staff Member"),
                    employee_origin(),
                ),
            ],
            vec![],
        );
        let result = analyze(&g, output(vec![employee_json()], vec![], vec![])).unwrap();
        assert!(result
            .conflicts
            .contains(&DomainConflict::ExistingOriginSemanticConflict {
                candidate_ref: id(EMPLOYEE_ENTITY),
                node_refs: ids(&["entity:dup-a", "entity:dup-b"]),
            }));
        assert!(result.proposals.is_empty());
        assert!(result
            .merge_dispositions
            .iter()
            .any(|d| d.disposition == DomainMergeDisposition::UnavailablePayloadDifference));
    }

    #[test]
    fn domain_distinct_concepts_with_equal_names_stay_distinct() {
        let g = extend(
            &base_graph(),
            vec![concept_node(
                "concept:employee-b",
                Accepted,
                "Employee",
                "Another accepted meaning.",
                ObjectType,
            )],
            vec![],
        );
        let result = analyze(
            &g,
            output(
                vec![
                    employee_json(),
                    entity_json("concept:employee-b", r1("Employee", 1)),
                ],
                vec![],
                vec![],
            ),
        )
        .unwrap();
        let entities = proposal_of_type(&result, NodeType::Entity);
        assert_eq!(entities.len(), 2);
        assert_ne!(added(entities[0]).0.id, added(entities[1]).0.id);
        assert!(result.conflicts.is_empty());
        let applied = apply(&g, &result.proposals);
        let rerun = analyze(
            &applied,
            output(
                vec![
                    employee_json(),
                    entity_json("concept:employee-b", r1("Employee", 1)),
                ],
                vec![],
                vec![],
            ),
        )
        .unwrap();
        assert!(rerun.conflicts.is_empty());
        assert!(rerun.proposals.is_empty());
    }

    // ------------------------------------------------------------------ determinism

    #[test]
    fn domain_input_order_determinism() {
        let g = with_entities();
        let reversed = Graph::new(
            g.project_id().clone(),
            g.profile_id().clone(),
            g.nodes().values().rev().cloned().collect(),
            g.edges().values().rev().cloned().collect(),
        )
        .unwrap();
        let forward = full_output();
        let mut backward = full_output();
        for key in ["entities", "attributes", "relationships"] {
            backward[key].as_array_mut().unwrap().reverse();
        }
        let a = analyze(&g, forward).unwrap();
        let b = analyze(&reversed, backward).unwrap();
        assert_eq!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&b).unwrap()
        );
        assert_eq!(a, analyze(&g, full_output()).unwrap());
    }

    #[test]
    fn domain_audit_timestamp_independence() {
        let g = with_entities();
        let r = request(&g);
        let artifact = artifact_for(
            &r.request,
            output(
                vec![],
                vec![attribute_json(false)],
                vec![relationship_json(Value::Null, Value::Null)],
            ),
        );
        let run = |at: &str| {
            analyze_domain(
                &g,
                &r,
                Some(DomainInference {
                    artifact: &artifact,
                    derivation_ref: derivation(),
                }),
                &audit_at(at),
            )
            .unwrap()
        };
        let a = run(AT);
        let b = run("2031-06-01T12:00:00.000000000Z");
        let node_ids = |res: &DomainAnalysisResult| -> Vec<Id> {
            res.proposals
                .iter()
                .map(|p| added(p).0.id.clone())
                .collect()
        };
        assert_eq!(node_ids(&a), node_ids(&b));
        assert_eq!(a.findings, b.findings);
        assert_eq!(a.issues, b.issues);
        // Proposal identity covers the proposed element audit bytes.
        assert_ne!(a.proposals[0].id, b.proposals[0].id);
    }

    // ------------------------------------------------------------------ real HR diagnostic

    /// The real single-source HR graph: requirements.md through S0.1, S0.4 fallback and S1.1
    /// (mock classifier), all Requirements Accepted in test code, and S1.5 Concept proposals
    /// applied as Proposed. No Concept is accepted.
    fn hr_graph() -> Graph {
        let audit_meta = ImportAudit {
            created_by: id("actor:importer"),
            created_at: ts(AT),
        };
        let imported = import_markdown("requirements.md", HR_MD, &audit_meta).unwrap();
        let fragments = imported.fragments.clone();
        let mut nodes = vec![imported.source];
        nodes.extend(imported.fragments);
        let mut g = new_graph(PROJECT, PROFILE, nodes, vec![]);
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
        assert_eq!(compiled.proposals.len(), 32);
        for p in &compiled.proposals {
            g = apply_patch(&g, &p.patch_set).unwrap().graph;
        }
        let g = map_nodes(&g, |n| {
            if matches!(n.payload, NodePayload::Requirement(_)) {
                n.status = Accepted;
            }
        });
        let requirements: Vec<Id> = g
            .node_ids_by_type(NodeType::Requirement)
            .iter()
            .cloned()
            .collect();
        assert_eq!(requirements.len(), 32);
        // S1.5 with mock object_type spans and a grounded definition per key, so Concept
        // proposals exist; they are applied as Proposed only.
        let policy = VocabularyPolicy {
            language: "en".into(),
        };
        let vocab = build_vocabulary_request(&g, &requirements, &policy, provider("mock")).unwrap();
        let mut definitions: BTreeMap<String, Value> = BTreeMap::new();
        let mut mentions = Vec::new();
        for r in &requirements {
            let text = statement(&g, r.as_str());
            let lower = text.to_ascii_lowercase();
            let mut taken: Vec<(usize, usize)> = Vec::new();
            for phrase in ["leave requests", "leave request", "employees", "employee"] {
                for (start, _) in lower.match_indices(phrase) {
                    let end = start + phrase.len();
                    let bounded = (start == 0
                        || !lower.as_bytes()[start - 1].is_ascii_alphanumeric())
                        && (end == lower.len() || !lower.as_bytes()[end].is_ascii_alphanumeric());
                    if !bounded || taken.iter().any(|(a, b)| start < *b && end > *a) {
                        continue;
                    }
                    taken.push((start, end));
                    let key = normalize_vocabulary_term(&text[start..end]).unwrap();
                    let definition = definitions
                        .entry(key)
                        .or_insert_with(
                            || json!({"requirement_ref": r, "start": start, "end": end}),
                        )
                        .clone();
                    mentions.push(json!({"requirement_ref": r, "start": start, "end": end,
                        "concept_kind": "object_type", "definition": definition}));
                }
            }
        }
        let vocab_artifact =
            artifact_for(&vocab.request, json!({"version": 1, "mentions": mentions}));
        let analysis = analyze_vocabulary(
            &g,
            &vocab,
            &policy,
            Some(VocabularyInference {
                artifact: &vocab_artifact,
                derivation_ref: DerivationRef::from(id("drv:00000000000000c2")),
            }),
            &VocabularyAudit {
                created_by: id("agent:vocabulary"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        apply(&g, &analysis.proposals)
    }

    #[test]
    fn domain_real_hr_diagnostic() {
        let g = hr_graph();
        let concepts: Vec<&Node> = g
            .node_ids_by_type(NodeType::Concept)
            .iter()
            .map(|c| g.node(c).unwrap())
            .collect();
        assert!(!concepts.is_empty(), "S1.5 Concept proposals exist");
        assert!(concepts.iter().all(|c| c.status == Proposed));
        let accepted_concepts = concepts.iter().filter(|c| c.status == Accepted).count();
        // Stop and inspect rather than hard-code if upstream ever accepts a Concept.
        assert_eq!(accepted_concepts, 0);
        let r = request(&g);
        assert_eq!(r.context.requirements.len(), 32);
        assert_eq!(r.context.concepts.len(), 0);
        let result = analyze_with(&g, &r, &artifact_for(&r.request, empty_output())).unwrap();
        assert_eq!(
            result.issues,
            vec![DomainIssue::AcceptedVocabularyUnavailable]
        );
        assert_eq!(proposal_of_type(&result, NodeType::Entity).len(), 0);
        assert_eq!(proposal_of_type(&result, NodeType::Attribute).len(), 0);
        assert_eq!(
            proposal_of_type(&result, NodeType::DomainRelationship).len(),
            0
        );
        assert!(result.proposals.is_empty());
        assert!(result.findings.is_empty());
        // S1.5 Proposed Concepts are not accepted vocabulary.
        let proposed = concepts[0];
        let requirement = &r.context.requirements[0];
        let grounding =
            json!({"requirement_ref": requirement.requirement_ref, "start": 0, "end": 1});
        assert!(matches!(
            analyze_with(
                &g,
                &r,
                &artifact_for(
                    &r.request,
                    output(
                        vec![entity_json(proposed.id.as_str(), grounding)],
                        vec![],
                        vec![]
                    )
                )
            ),
            Err(DomainError::ConceptNotAccepted {
                status: Proposed,
                ..
            })
        ));
        // No entity survival percentage is computed or claimed: zero Entity proposals exist.
    }

    // ------------------------------------------------------------------ guards

    #[test]
    fn domain_production_source_guard() {
        let source = include_str!("../src/domain.rs");
        for forbidden in [
            "std::fs",
            "File::open",
            "ArtifactStore",
            "SqliteArtifactStore",
            "SqliteRevisionStore",
            "RevisionStore",
            "rusqlite",
            "SystemClock",
            "Clock::now",
            "Utc::now",
            "Instant::now",
            "reqwest",
            ".execute(",
            "MockProvider",
            "persist_inference_bundle",
            "unsafe",
            "commit(",
            "commit_",
            "Commit",
            "HR-001",
            "HR-032",
            "Employee",
            "LeaveRequest",
            "Leave Request",
            "Manager",
            "fixtures/",
            "85%",
            "0.85",
            "G_REL_NO_CARD",
            "G_UNKNOWN_TERM",
            "I_TERM",
            "NodePayload::State",
            "NodePayload::Transition",
            "NodePayload::Invariant",
            "NodePayload::Actor",
            "NodePayload::BusinessRole",
            "NodePayload::SecurityRole",
            "unit: Some",
            "precision: Some",
            "enum_values: Some",
            "data_classification: Some",
            "aggregate_root: Some",
            "entity_name",
            "attribute_name",
            "relationship_name",
            "value_type_text",
            "normalized_key",
            "unknown\"",
            "\"TBD\"",
            "MergeProposed",
            "SemanticPatch::MergeNodes",
            "MergePolicy",
            "keep_ref",
            "preferred_keep",
            "would_keep",
        ] {
            assert!(
                !source.contains(forbidden),
                "domain.rs contains {forbidden}"
            );
        }
        // The F0.14 root guard: lib.rs never spells the Proposal token.
        let lib = include_str!("../src/lib.rs");
        assert!(!lib.contains("Proposal"));
        assert!(lib.contains("pub mod domain;"));
        // apply_patch is only used for dry validation.
        assert!(source.contains("apply_patch"));
        assert!(!source.contains("RemoveEdge"));
        assert!(!source.contains("ReplaceEdge"));
        assert!(!source.contains("RemoveNode"));
    }
}
