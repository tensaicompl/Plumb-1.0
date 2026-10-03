//! S1.2 contract tests for grounded EARS normalization: original-statement recovery, the EARS
//! request, grounding, the deterministic renderer, ReplacePayload proposals and the end-to-end
//! S0.4 -> S1.1 -> S1.2 path.
//!
//! Every test lives in `ears_contract` so that `cargo test -p plumb-functional ears` selects
//! them. Golden values were computed independently with Python `hashlib` over RFC 8785 JSON,
//! never with the helpers under test.

mod ears_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use jsonschema::{Draft, JSONSchema};
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_functional::*;
    use plumb_import::{
        build_segmentation_request, evaluate_segmentation, import_plain_text, ImportAudit,
        SegmentCandidate, SegmentationAudit,
    };
    use plumb_inference::{
        InferenceArtifact, InferenceProvider, InferenceRequest, MockProvider, ProviderExecution,
        ProviderPolicy,
    };
    use plumb_patch::{
        apply_patch, AcceptancePolicy, PatchError, Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        node_element_hash, node_element_projection, Actor, ActorKind, AuditMeta, DerivationRef,
        ElementStatus, Graph, Modality, Node, NodePayload, Requirement,
    };
    use serde_json::{json, Value};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s1-ears.md");
    const SCHEMA: &str = include_str!("../../../schemas/inference/s1-ears.schema.json");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:c10da0fb1b71c1296c7dee1a7b6340e4a3ef0aadaa049efab1a744ac9392d98a";
    const SCHEMA_HASH: &str =
        "sha256:0b56132d51dd3f048fd06af0bdc7fa66e54d367f9ffe72866db146fcad154c86";
    const GOLDEN_CONTEXT_HASH: &str =
        "sha256:bceae30f6c786c54c6912bf0df6d5bfa9d265726006aaa74a35b94b58376f48f";
    const GOLDEN_REQUEST_ID: &str =
        "sha256:4f746ebe8e584c8009b9d945749c6355e91f726436befa3e62969caa27860122";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const EXPORT: &str = "The system shall export the report.";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn policy(provider: &str) -> ProviderPolicy {
        ProviderPolicy {
            provider: provider.to_owned(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn derivation() -> DerivationRef {
        DerivationRef::from(id("drv:00000000000000e1"))
    }

    fn execution(request: &InferenceRequest, output: Value) -> ProviderExecution {
        let raw_response = to_canonical_json(&output).unwrap();
        let validated_output = CanonicalJson::new(output);
        ProviderExecution {
            artifact: InferenceArtifact {
                request_hash: request.id.clone(),
                provider: request.provider_policy.provider.clone(),
                model: "mock-model".to_owned(),
                parameters: CanonicalJson::new(json!({})),
                raw_response_hash: Hash::content_sha256(&raw_response),
                validated_output_hash: validated_output.content_hash().unwrap(),
                validated_output,
            },
            raw_response_media_type: "application/json".to_owned(),
            raw_response,
        }
    }

    fn semantic_node(node_id: &str, payload: NodePayload, status: ElementStatus) -> Node {
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

    fn actor(node_id: &str, name: &str, status: ElementStatus) -> Node {
        semantic_node(
            node_id,
            NodePayload::Actor(Actor {
                name: name.to_owned(),
                actor_kind: ActorKind::Human,
            }),
            status,
        )
    }

    fn rebuild(graph: &Graph, nodes: Vec<Node>) -> Graph {
        Graph::new(
            graph.project_id().clone(),
            graph.profile_id().clone(),
            nodes,
            graph.edges().values().cloned().collect(),
        )
        .unwrap()
    }

    fn with_nodes(graph: &Graph, extra: Vec<Node>) -> Graph {
        let mut nodes: Vec<Node> = graph.nodes().values().cloned().collect();
        nodes.extend(extra);
        rebuild(graph, nodes)
    }

    fn modify(graph: &Graph, target: &Id, f: impl Fn(&mut Node)) -> Graph {
        let nodes = graph
            .nodes()
            .values()
            .cloned()
            .map(|mut node| {
                if &node.id == target {
                    f(&mut node);
                }
                node
            })
            .collect();
        rebuild(graph, nodes)
    }

    /// The end-to-end pilot path: plain-text import, S0.4 whole-fragment requirement
    /// candidates, S1.1 classification (mock: functional / system) and every S1.1 Requirement
    /// AddNode applied in test code. Returns the graph and the requirement IDs in paragraph
    /// order.
    fn pipeline(paragraphs: &[&str], extra: Vec<Node>) -> (Graph, Vec<Id>) {
        let text = paragraphs.join("\n\n");
        let audit = ImportAudit {
            created_by: id("actor:importer"),
            created_at: ts(AT),
        };
        let imported = import_plain_text("synthetic.txt", text.as_bytes(), &audit).unwrap();
        assert_eq!(imported.fragments.len(), paragraphs.len());
        let mut nodes = vec![imported.source];
        nodes.extend(imported.fragments.clone());
        nodes.extend(extra);
        let mut graph = Graph::new(id(PROJECT), id(PROFILE), nodes, Vec::new()).unwrap();

        let fragments = imported.fragments;
        let segmentation = build_segmentation_request(&fragments, policy("mock")).unwrap();
        let segments: Vec<Value> = segmentation
            .context
            .fragments
            .iter()
            .map(|f| {
                json!({"fragment_ref": f.fragment_ref, "start": 0, "end": f.text.len(),
                       "classification": "requirement_candidate"})
            })
            .collect();
        let artifact = execution(
            &segmentation.request,
            json!({"version": 1, "segments": segments}),
        )
        .artifact;
        let segmentation_audit = SegmentationAudit {
            created_by: id("agent:segmenter"),
            created_at: ts(AT),
        };
        let candidates: Vec<SegmentCandidate> = evaluate_segmentation(
            &segmentation,
            &fragments,
            Some(&artifact),
            &segmentation_audit,
        )
        .unwrap()
        .candidates;

        let classification =
            build_requirement_classification_request(&graph, &candidates, policy("mock")).unwrap();
        let entries: Vec<Value> = classification
            .context
            .candidates
            .iter()
            .map(|c| {
                json!({"candidate_ref": c.candidate_ref, "requirement_kind": "functional",
                       "level": "system"})
            })
            .collect();
        let artifact = execution(
            &classification.request,
            json!({"version": 1, "classifications": entries, "intents": []}),
        )
        .artifact;
        let compiled = compile_requirement_candidates(
            &graph,
            &candidates,
            &classification,
            Some(RequirementClassificationInference {
                artifact: &artifact,
                derivation_ref: DerivationRef::from(id("drv:00000000000000c1")),
            }),
            &RequirementCompilationAudit {
                created_by: id("agent:requirements-compiler"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        assert!(compiled.unresolved.is_empty(), "{:?}", compiled.unresolved);
        let mut by_fragment = BTreeMap::new();
        for proposal in &compiled.proposals {
            graph = apply_patch(&graph, &proposal.patch_set).unwrap().graph;
            if let SemanticPatch::AddNode { node } = &proposal.patch_set.patch {
                by_fragment.insert(node.evidence[0].as_id().clone(), node.id.clone());
            }
        }
        let ids = fragments
            .iter()
            .map(|f| by_fragment[&f.id].clone())
            .collect();
        (graph, ids)
    }

    fn single(text: &str) -> (Graph, Id) {
        let (graph, ids) = pipeline(&[text], Vec::new());
        (graph, ids[0].clone())
    }

    fn request(graph: &Graph, targets: &[Id]) -> EarsRequest {
        build_ears_request(graph, targets, policy("mock")).unwrap()
    }

    fn range(start: usize, end: usize) -> Value {
        json!({"kind": "source_range", "start": start, "end": end})
    }

    /// The source range of `part` in `source`.
    fn find(source: &str, part: &str) -> Value {
        let start = source
            .find(part)
            .unwrap_or_else(|| panic!("{part:?} not in {source:?}"));
        range(start, start + part.len())
    }

    fn accepted(semantic_ref: &str, text: &str) -> Value {
        json!({"kind": "accepted_semantic", "semantic_ref": semantic_ref, "text": text})
    }

    fn suggestion(
        pattern: &str,
        actor: Value,
        trigger: Value,
        condition: Value,
        response: Value,
    ) -> Value {
        json!({"pattern": pattern, "actor": actor, "trigger": trigger, "condition": condition,
               "response": response})
    }

    fn entry(requirement_ref: &Id, suggestion: Value) -> Value {
        json!({"requirement_ref": requirement_ref, "suggestion": suggestion})
    }

    fn output(entries: Vec<Value>) -> Value {
        json!({"version": 1, "requirements": entries})
    }

    fn evaluate(
        graph: &Graph,
        ears: &EarsRequest,
        out: Value,
    ) -> Result<EarsNormalizationResult, EarsError> {
        let artifact = execution(&ears.request, out).artifact;
        evaluate_with(graph, ears, &artifact)
    }

    fn evaluate_with(
        graph: &Graph,
        ears: &EarsRequest,
        artifact: &InferenceArtifact,
    ) -> Result<EarsNormalizationResult, EarsError> {
        evaluate_ears_normalization(
            graph,
            ears,
            Some(EarsInference {
                artifact,
                derivation_ref: derivation(),
            }),
        )
    }

    /// Normalizes the single requirement of `text` with `suggestion` and returns the statement.
    fn normalize(text: &str, make: impl Fn(&str) -> Value) -> Result<String, EarsError> {
        let (graph, req) = single(text);
        let ears = request(&graph, std::slice::from_ref(&req));
        let source = ears.context.requirements[0].source_statement.clone();
        let result = evaluate(&graph, &ears, output(vec![entry(&req, make(&source))]))?;
        assert_eq!(result.proposals.len(), 1, "{:?}", result.issues);
        Ok(replaced(&result.proposals[0]).statement.clone())
    }

    fn replaced(proposal: &Proposal) -> &Requirement {
        match &proposal.patch_set.patch {
            SemanticPatch::ReplacePayload {
                payload: NodePayload::Requirement(r),
                ..
            } => r,
            other => panic!("expected ReplacePayload, got {other:?}"),
        }
    }

    fn requirement_of(graph: &Graph, req: &Id) -> Requirement {
        match &graph.node(req).unwrap().payload {
            NodePayload::Requirement(r) => r.clone(),
            other => panic!("expected Requirement, got {other:?}"),
        }
    }

    fn ubiquitous(source: &str, actor: &str, response: &str) -> Value {
        suggestion(
            "ubiquitous",
            find(source, actor),
            Value::Null,
            Value::Null,
            find(source, response),
        )
    }

    // ------------------------------------------------------------------ schema and assets

    fn compiled_schema() -> JSONSchema {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&schema)
            .unwrap()
    }

    const REQ: &str = "req:0123456789abcdef";

    fn schema_valid_cases() -> Vec<Value> {
        let r = range(0, 3);
        let a = accepted("actor:admin", "Administrator");
        vec![
            suggestion("ubiquitous", r.clone(), Value::Null, Value::Null, r.clone()),
            suggestion("event_driven", a.clone(), r.clone(), Value::Null, r.clone()),
            suggestion("state_driven", r.clone(), Value::Null, r.clone(), r.clone()),
            suggestion(
                "optional_feature",
                r.clone(),
                Value::Null,
                a.clone(),
                r.clone(),
            ),
            suggestion("unwanted_behavior", r.clone(), Value::Null, r.clone(), a),
            Value::Null,
        ]
    }

    #[test]
    fn ears_schema_is_draft_2020_12_and_compiles() {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(
            schema["$schema"],
            json!("https://json-schema.org/draft/2020-12/schema")
        );
        assert!(!SCHEMA.contains("$ref"));
        for field in [
            "normalized_statement",
            "normalized_text",
            "ears_text",
            "modality",
            "confidence",
        ] {
            assert!(!SCHEMA.contains(field), "{field}");
        }
        let compiled = compiled_schema();
        for case in schema_valid_cases() {
            assert!(
                compiled.is_valid(&output(vec![entry(&id(REQ), case.clone())])),
                "{case}"
            );
        }
    }

    #[test]
    fn ears_schema_rejects_invalid_outputs() {
        let compiled = compiled_schema();
        let base = schema_valid_cases()[1].clone();
        let mutate = |f: &dyn Fn(&mut Value)| {
            let mut s = base.clone();
            f(&mut s);
            output(vec![entry(&id(REQ), s)])
        };
        let invalid = [
            mutate(&|s| s["normalized_statement"] = json!("When 2 seconds pass, it shall act.")),
            mutate(&|s| s["pattern"] = json!("complex")),
            mutate(&|s| s["pattern"] = json!("Ubiquitous")),
            mutate(&|s| s["actor"] = json!({"kind": "free_text", "text": "Administrator"})),
            mutate(&|s| {
                s.as_object_mut().unwrap().remove("response");
            }),
            mutate(&|s| s["response"] = json!({"kind": "source_range", "start": "0", "end": 3})),
            mutate(&|s| {
                s["actor"] = json!({"kind": "accepted_semantic", "semantic_ref": "actor:admin"})
            }),
            mutate(&|s| s["actor"]["extra"] = json!(1)),
            mutate(&|s| s["modality"] = json!("shall")),
            mutate(&|s| s["requirement_kind"] = json!("functional")),
            mutate(&|s| s["condition"] = s["trigger"].clone()),
            mutate(&|s| s["trigger"] = Value::Null),
            mutate(&|s| s["pattern"] = json!("ubiquitous")),
            json!({"version": 1, "requirements": [], "extra": true}),
            json!({"version": 2, "requirements": []}),
            json!({"version": 1, "requirements": [{"requirement_ref": REQ}]}),
            json!({"version": 1, "requirements": [{"requirement_ref": REQ, "suggestion": null, "x": 1}]}),
        ];
        for value in invalid {
            assert!(!compiled.is_valid(&value), "{value}");
        }
    }

    #[test]
    fn ears_prompt_and_schema_hashes_are_exact_file_bytes() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
        let (graph, req) = single(EXPORT);
        let ears = request(&graph, &[req]);
        assert_eq!(ears.request.prompt_template_hash.as_str(), PROMPT_HASH);
        assert_eq!(ears.request.schema_hash.as_str(), SCHEMA_HASH);
    }

    #[test]
    fn ears_assets_have_no_placeholders() {
        for content in [
            String::from_utf8(PROMPT.to_vec()).unwrap(),
            SCHEMA.to_owned(),
        ] {
            let lower = content.to_lowercase();
            for marker in ["todo", "placeholder", "fixme"] {
                assert!(!lower.contains(marker), "{marker}");
            }
        }
    }

    #[test]
    fn ears_production_source_guard() {
        let source = include_str!("../src/ears.rs");
        for token in [
            "std::fs",
            "File::open",
            "ArtifactStore",
            "rusqlite",
            "SystemClock",
            "Clock::now",
            "Utc::now",
            "Instant::now",
            "reqwest",
            "InferenceProvider",
            "::execute",
            "MockProvider",
            "apply_patch",
            "persist_inference_bundle",
            "materialize_derivation_record",
            "ResolutionDecision {",
            "NodePayload::Question",
            "NodePayload::Finding",
            "AddNode",
            "commit(",
            "unsafe",
            "\"mock\"",
            "normalized_statement",
        ] {
            assert!(!source.contains(token), "{token}");
        }
    }

    // ------------------------------------------------------------------ request and context

    #[test]
    fn ears_request_golden() {
        let admin = actor("actor:admin", "Administrator", ElementStatus::Accepted);
        let (graph, ids) = pipeline(&[EXPORT], vec![admin]);
        let ears = request(&graph, &ids);
        let context = &ears.context;
        assert_eq!(context.version, EARS_CONTEXT_VERSION);
        assert_eq!(context.project_id, id(PROJECT));
        let r = &context.requirements[0];
        assert_eq!(r.requirement_ref, ids[0]);
        assert_eq!(r.status, ElementStatus::Proposed);
        assert_eq!(r.current_statement, EXPORT);
        assert_eq!(r.source_statement, EXPORT);
        assert_eq!(r.modality, Modality::Shall);
        assert_eq!(r.evidence_refs, vec![r.segment_origin.fragment_ref.clone()]);
        assert_eq!(
            context.accepted_semantics,
            vec![EarsAcceptedSemantic {
                semantic_ref: id("actor:admin"),
                values: vec![
                    "Actor".to_owned(),
                    "Administrator".to_owned(),
                    "human".to_owned()
                ],
            }]
        );
        let inner = &ears.request;
        assert_eq!(inner.stage, StageId::S1);
        assert_eq!(inner.task_kind, "ears_normalization");
        assert_eq!(EARS_TASK_KIND, "ears_normalization");
        assert_eq!((EARS_CONTEXT_VERSION, EARS_OUTPUT_VERSION), (1, 1));
        let mut input_refs = vec![ids[0].clone(), id("actor:admin")];
        input_refs.sort();
        assert_eq!(inner.input_refs, input_refs);
        assert_eq!(inner.evidence_refs, r.evidence_refs);
        assert_eq!(
            serde_json::to_value(context).unwrap(),
            json!({
                "version": 1,
                "project_id": PROJECT,
                "requirements": [{
                    "requirement_ref": "req:c0821fde077de3f0",
                    "status": "Proposed",
                    "current_statement": EXPORT,
                    "source_statement": EXPORT,
                    "requirement_kind": "functional",
                    "level": "system",
                    "modality": "shall",
                    "source_identifier": null,
                    "evidence_refs": ["evd:fc7990d3f9bd336b"],
                    "segment_origin": {
                        "candidate_ref": "seg:c6853889ffcb4346",
                        "fragment_ref": "evd:fc7990d3f9bd336b",
                        "start": 0,
                        "end": 35,
                    },
                }],
                "accepted_semantics": [{
                    "semantic_ref": "actor:admin",
                    "values": ["Actor", "Administrator", "human"],
                }],
            })
        );
        assert_eq!(inner.context_hash.as_str(), GOLDEN_CONTEXT_HASH);
        assert_eq!(inner.id.as_str(), GOLDEN_REQUEST_ID);
    }

    #[test]
    fn ears_accepted_semantic_catalog() {
        let extra = vec![
            actor("actor:admin", "Administrator", ElementStatus::Accepted),
            actor("actor:dup", "Administrator", ElementStatus::Accepted),
            actor("actor:proposed", "Auditor", ElementStatus::Proposed),
            actor("actor:suspect", "Clerk", ElementStatus::Suspect),
        ];
        let (graph, ids) = pipeline(&[EXPORT, "The system shall archive the report."], extra);
        // An Accepted Requirement target is excluded from its own catalog; the other
        // Accepted Requirement is a catalog entry.
        let graph = modify(&graph, &ids[0], |n| n.status = ElementStatus::Accepted);
        let graph = modify(&graph, &ids[1], |n| n.status = ElementStatus::Accepted);
        let ears = request(&graph, std::slice::from_ref(&ids[0]));
        let refs: Vec<&Id> = ears
            .context
            .accepted_semantics
            .iter()
            .map(|s| &s.semantic_ref)
            .collect();
        let mut expected = [id("actor:admin"), id("actor:dup"), ids[1].clone()];
        expected.sort();
        assert_eq!(refs, expected.iter().collect::<Vec<_>>());
        for entry in &ears.context.accepted_semantics {
            assert!(entry.values.windows(2).all(|p| p[0] < p[1]));
            assert!(entry.values.iter().all(|v| !v.is_empty()));
        }
        let archive = ears
            .context
            .accepted_semantics
            .iter()
            .find(|s| s.semantic_ref == ids[1])
            .unwrap();
        assert!(archive
            .values
            .contains(&"The system shall archive the report.".to_owned()));
        assert!(
            !archive.values.iter().any(|v| v == "statement"),
            "keys are not values"
        );
        // EvidenceFragment and SourceArtifact never contribute to the semantic hash.
        assert!(refs
            .iter()
            .all(|r| !r.as_str().starts_with("evd:") && !r.as_str().starts_with("src:")));
    }

    #[test]
    fn ears_request_is_order_independent_and_policy_sensitive() {
        let extra = vec![
            actor("actor:b", "Bookkeeper", ElementStatus::Accepted),
            actor("actor:a", "Administrator", ElementStatus::Accepted),
        ];
        let (graph, ids) = pipeline(&[EXPORT, "The system shall archive the report."], extra);
        let forward = request(&graph, &ids);
        let mut reversed_ids = ids.clone();
        reversed_ids.reverse();
        let mut nodes: Vec<Node> = graph.nodes().values().cloned().collect();
        nodes.reverse();
        let reversed_graph = rebuild(&graph, nodes);
        let reversed = request(&reversed_graph, &reversed_ids);
        assert_eq!(forward, reversed);
        let other = build_ears_request(&graph, &ids, policy("other-provider")).unwrap();
        assert_ne!(other.request.id, forward.request.id);
        assert_eq!(other.context, forward.context);
    }

    #[test]
    fn ears_request_rejects_invalid_targets() {
        let (graph, req) = single(EXPORT);
        let invalid = |graph: &Graph, targets: &[Id]| {
            assert!(
                matches!(
                    build_ears_request(graph, targets, policy("mock")),
                    Err(EarsError::InvalidInput { .. })
                ),
                "{targets:?}"
            );
        };
        invalid(&graph, &[]);
        invalid(&graph, &[req.clone(), req.clone()]);
        invalid(&graph, &[id("req:00000000000000ff")]);
        let fragment = graph.node(&req).unwrap().evidence[0].as_id().clone();
        invalid(&graph, std::slice::from_ref(&fragment));
        for status in [
            ElementStatus::Rejected,
            ElementStatus::Superseded,
            ElementStatus::Deprecated,
            ElementStatus::Suspect,
        ] {
            invalid(
                &modify(&graph, &req, |n| n.status = status),
                std::slice::from_ref(&req),
            );
        }
        let accepted = modify(&graph, &req, |n| n.status = ElementStatus::Accepted);
        assert!(build_ears_request(&accepted, std::slice::from_ref(&req), policy("mock")).is_ok());
        invalid(
            &modify(&graph, &req, |n| n.extensions.clear()),
            std::slice::from_ref(&req),
        );
        let key = SEGMENT_ORIGIN_EXTENSION.parse().unwrap();
        let origin = graph.node(&req).unwrap().extensions[&key].clone();
        let broken = |f: &dyn Fn(&mut Value)| {
            let mut value = origin.clone();
            f(&mut value);
            modify(&graph, &req, |n| {
                n.extensions
                    .insert(SEGMENT_ORIGIN_EXTENSION.parse().unwrap(), value.clone());
            })
        };
        invalid(
            &broken(&|v| v["extra"] = json!(1)),
            std::slice::from_ref(&req),
        );
        invalid(
            &broken(&|v| v["end"] = json!(999)),
            std::slice::from_ref(&req),
        );
        invalid(
            &broken(&|v| v["start"] = v["end"].clone()),
            std::slice::from_ref(&req),
        );
        invalid(
            &broken(&|v| v["fragment_ref"] = json!("evd:00000000000000ff")),
            std::slice::from_ref(&req),
        );
        // An origin fragment outside Node.evidence (graph validation already forbids an
        // evidence ref to a non-EvidenceFragment node).
        let graph_with_actor =
            with_nodes(&graph, vec![actor("actor:x", "X", ElementStatus::Accepted)]);
        let outside = modify(&graph_with_actor, &req, |n| {
            let mut value = origin.clone();
            value["fragment_ref"] = json!("actor:x");
            n.extensions
                .insert(SEGMENT_ORIGIN_EXTENSION.parse().unwrap(), value);
        });
        invalid(&outside, std::slice::from_ref(&req));
    }

    #[test]
    fn ears_source_statement_recovery_strips_s1_prefix() {
        let (graph, req) =
            single("12. **REQ-123** [functional] The system shall export the report.");
        let ears = request(&graph, std::slice::from_ref(&req));
        let r = &ears.context.requirements[0];
        assert_eq!(r.source_statement, EXPORT);
        assert_eq!(r.source_identifier.as_deref(), Some("REQ-123"));
    }

    #[test]
    fn ears_request_tampering_is_rejected() {
        let (graph, req) = single(EXPORT);
        let ears = request(&graph, &[req]);
        let rebuild_id = |f: &dyn Fn(&mut InferenceRequest)| {
            let mut tampered = ears.clone();
            f(&mut tampered.request);
            tampered.request.id = tampered.request.recompute_id().unwrap();
            tampered
        };
        let mut cases = vec![
            rebuild_id(&|r| r.stage = StageId::S0),
            rebuild_id(&|r| r.task_kind = "requirement_classification".to_owned()),
            rebuild_id(&|r| r.context_hash = Hash::content_sha256(b"x")),
            rebuild_id(&|r| r.prompt_template_hash = Hash::content_sha256(b"x")),
            rebuild_id(&|r| r.schema_hash = Hash::content_sha256(b"x")),
            rebuild_id(&|r| r.input_refs = vec![]),
            rebuild_id(&|r| r.evidence_refs = vec![id("evd:00000000000000ff")]),
        ];
        let mut policy_only = ears.clone();
        policy_only.request.provider_policy = policy("other-provider");
        cases.push(policy_only);
        let mut context = ears.clone();
        context.context.requirements[0].current_statement.push('!');
        cases.push(context);
        for bad in cases {
            assert!(bad.validate().is_err());
            assert!(matches!(
                evaluate_ears_normalization(&graph, &bad, None),
                Err(EarsError::InvalidInput { .. })
            ));
        }
    }

    #[test]
    fn ears_stale_request_is_rejected() {
        let (graph, req) = single(EXPORT);
        let ears = request(&graph, std::slice::from_ref(&req));
        let changed = modify(&graph, &req, |n| n.status = ElementStatus::Accepted);
        assert!(matches!(
            evaluate_ears_normalization(&changed, &ears, None),
            Err(EarsError::InvalidInput { .. })
        ));
        let with_semantic =
            with_nodes(&graph, vec![actor("actor:a", "A", ElementStatus::Accepted)]);
        assert!(matches!(
            evaluate_ears_normalization(&with_semantic, &ears, None),
            Err(EarsError::InvalidInput { .. })
        ));
    }

    // ------------------------------------------------------------------ grounding and rendering

    #[test]
    fn ears_source_range_exact() {
        let (graph, req) = single(EXPORT);
        let ears = request(&graph, std::slice::from_ref(&req));
        let out = output(vec![entry(
            &req,
            suggestion(
                "ubiquitous",
                range(0, 10),
                Value::Null,
                Value::Null,
                range(17, 34),
            ),
        )]);
        assert_eq!(&EXPORT[0..10], "The system");
        assert_eq!(&EXPORT[17..34], "export the report");
        let result = evaluate(&graph, &ears, out).unwrap();
        // The rendering equals the current statement: no no-op patch.
        assert_eq!(
            result.issues,
            vec![EarsIssue::AlreadyNormalized {
                requirement_ref: req.clone()
            }]
        );
        assert!(result.proposals.is_empty());
    }

    #[test]
    fn ears_renderer_patterns() {
        let text = "When an order arrives while the shop is open, the clerk shall pack the order.";
        let event = normalize(text, |s| {
            suggestion(
                "event_driven",
                find(s, "the clerk"),
                find(s, "an order arrives"),
                Value::Null,
                find(s, "pack the order"),
            )
        });
        assert_eq!(
            event.unwrap(),
            "When an order arrives, the clerk shall pack the order."
        );
        for (pattern, lead) in [("state_driven", "While"), ("optional_feature", "Where")] {
            let rendered = normalize(text, |s| {
                suggestion(
                    pattern,
                    find(s, "the clerk"),
                    Value::Null,
                    find(s, "the shop is open"),
                    find(s, "pack the order"),
                )
            });
            assert_eq!(
                rendered.unwrap(),
                format!("{lead} the shop is open, the clerk shall pack the order.")
            );
        }
        let unwanted = normalize(text, |s| {
            suggestion(
                "unwanted_behavior",
                find(s, "the clerk"),
                Value::Null,
                find(s, "an order arrives"),
                find(s, "pack the order"),
            )
        });
        assert_eq!(
            unwanted.unwrap(),
            "If an order arrives, then the clerk shall pack the order."
        );
        let ubiquitous_text = normalize(text, |s| ubiquitous(s, "the clerk", "pack the order"));
        assert_eq!(ubiquitous_text.unwrap(), "the clerk shall pack the order.");
    }

    #[test]
    fn ears_modality_preservation() {
        for (text, modal) in [
            ("Always, the clerk shall pack the order.", "shall"),
            ("Always, the clerk shall not pack the order.", "shall not"),
            ("Always, the clerk should pack the order.", "should"),
            ("Always, the clerk may pack the order.", "may"),
        ] {
            for pattern in ["ubiquitous", "state_driven", "unwanted_behavior"] {
                let (graph, req) = single(text);
                let before = requirement_of(&graph, &req);
                let ears = request(&graph, std::slice::from_ref(&req));
                let s = ears.context.requirements[0].source_statement.clone();
                let condition = if pattern == "ubiquitous" {
                    Value::Null
                } else {
                    find(&s, "Always")
                };
                let out = output(vec![entry(
                    &req,
                    suggestion(
                        pattern,
                        find(&s, "the clerk"),
                        Value::Null,
                        condition,
                        find(&s, "pack the order"),
                    ),
                )]);
                let result = evaluate(&graph, &ears, out).unwrap();
                let after = replaced(&result.proposals[0]);
                assert!(
                    after
                        .statement
                        .contains(&format!("the clerk {modal} pack the order.")),
                    "{}",
                    after.statement
                );
                assert_eq!(after.modality, before.modality);
            }
        }
    }

    #[test]
    fn ears_utf8_ranges_are_byte_offsets() {
        let text = "Zażółć pracownik shall złożyć żądanie teraz.";
        let cut = normalize(text, |_| {
            suggestion(
                "ubiquitous",
                range(0, 3),
                Value::Null,
                Value::Null,
                range(25, 30),
            )
        });
        assert!(matches!(
            cut,
            Err(EarsError::InvalidGrounding {
                component: "actor",
                ..
            })
        ));
        let ok = normalize(text, |s| {
            ubiquitous(s, "Zażółć pracownik", "złożyć żądanie")
        });
        assert_eq!(ok.unwrap(), "Zażółć pracownik shall złożyć żądanie.");
        let beyond = normalize(text, |s| {
            suggestion(
                "ubiquitous",
                find(s, "Zażółć pracownik"),
                Value::Null,
                Value::Null,
                range(10, s.chars().count() + 20),
            )
        });
        assert!(matches!(
            beyond,
            Err(EarsError::InvalidGrounding {
                component: "response",
                ..
            })
        ));
    }

    #[test]
    fn ears_component_boundaries() {
        let text = "Now the system shall export the report, daily.";
        let with_period = normalize(text, |s| {
            ubiquitous(s, "the system", "export the report, daily.")
        });
        assert!(matches!(
            with_period,
            Err(EarsError::InvalidComponent {
                component: "response",
                ..
            })
        ));
        let fixed = normalize(text, |s| {
            ubiquitous(s, "the system", "export the report, daily")
        });
        assert_eq!(fixed.unwrap(), "the system shall export the report, daily.");
        let spaced = normalize(text, |s| ubiquitous(s, "the system ", "export the report"));
        assert!(matches!(
            spaced,
            Err(EarsError::InvalidComponent {
                component: "actor",
                ..
            })
        ));
        let actor_comma = normalize("Daily, the clerk shall pack.", |s| {
            ubiquitous(s, "Daily,", "pack")
        });
        assert!(matches!(
            actor_comma,
            Err(EarsError::InvalidComponent {
                component: "actor",
                ..
            })
        ));
        let condition_comma = normalize("Daily, the clerk shall pack.", |s| {
            suggestion(
                "state_driven",
                find(s, "the clerk"),
                Value::Null,
                find(s, "Daily,"),
                find(s, "pack"),
            )
        });
        assert!(matches!(
            condition_comma,
            Err(EarsError::InvalidComponent {
                component: "condition",
                ..
            })
        ));
        let trigger_comma = normalize("Daily, the clerk shall pack.", |s| {
            suggestion(
                "event_driven",
                find(s, "the clerk"),
                find(s, ", the"),
                Value::Null,
                find(s, "pack"),
            )
        });
        assert!(matches!(
            trigger_comma,
            Err(EarsError::InvalidComponent {
                component: "trigger",
                ..
            })
        ));
    }

    #[test]
    fn ears_pattern_shape_is_rechecked() {
        // Shape violations that slip past a schema still fail deterministically.
        let (graph, req) = single(EXPORT);
        let ears = request(&graph, std::slice::from_ref(&req));
        let r = range(0, 10);
        let bad = output(vec![entry(
            &req,
            suggestion("event_driven", r.clone(), r.clone(), r.clone(), r),
        )]);
        assert!(matches!(
            evaluate(&graph, &ears, bad),
            Err(EarsError::SchemaInvalid { .. })
        ));
    }

    // ------------------------------------------------------------------ hallucination

    #[test]
    fn ears_numeric_invention_is_rejected() {
        let text = "The system shall respond to the user.";
        let threshold = actor(
            "actor:sla",
            "respond to the user quickly",
            ElementStatus::Accepted,
        );
        let (graph, ids) = pipeline(&[text], vec![threshold]);
        let ears = request(&graph, &ids);
        let s = ears.context.requirements[0].source_statement.clone();
        let invented = output(vec![entry(
            &ids[0],
            suggestion(
                "ubiquitous",
                find(&s, "The system"),
                Value::Null,
                Value::Null,
                accepted("actor:sla", "respond within 2 seconds"),
            ),
        )]);
        assert!(matches!(
            evaluate(&graph, &ears, invented),
            Err(EarsError::InvalidGrounding {
                component: "response",
                ..
            })
        ));
        let free_text = output(vec![json!({"requirement_ref": ids[0], "suggestion": null,
            "normalized_statement": "The system shall respond within 2 seconds."})]);
        assert!(matches!(
            evaluate(&graph, &ears, free_text),
            Err(EarsError::SchemaInvalid { .. })
        ));
    }

    #[test]
    fn ears_grounded_numbers_are_accepted() {
        let source_grounded =
            normalize("Always, the system shall respond within 2 seconds.", |s| {
                suggestion(
                    "state_driven",
                    find(s, "the system"),
                    Value::Null,
                    find(s, "Always"),
                    find(s, "respond within 2 seconds"),
                )
            });
        assert_eq!(
            source_grounded.unwrap(),
            "While Always, the system shall respond within 2 seconds."
        );
        let sla = actor(
            "actor:sla",
            "respond within 2 seconds",
            ElementStatus::Accepted,
        );
        let (graph, ids) = pipeline(&["The system shall respond to the user."], vec![sla]);
        let ears = request(&graph, &ids);
        let s = ears.context.requirements[0].source_statement.clone();
        let out = output(vec![entry(
            &ids[0],
            suggestion(
                "ubiquitous",
                find(&s, "The system"),
                Value::Null,
                Value::Null,
                accepted("actor:sla", "respond within 2 seconds"),
            ),
        )]);
        let result = evaluate(&graph, &ears, out).unwrap();
        assert_eq!(
            replaced(&result.proposals[0]).statement,
            "The system shall respond within 2 seconds."
        );
    }

    #[test]
    fn ears_actor_grounding() {
        let text = "A leave request shall be retained.";
        let run = |extra: Vec<Node>, grounding: Value| {
            let (graph, ids) = pipeline(&[text], extra);
            let ears = request(&graph, &ids);
            let s = ears.context.requirements[0].source_statement.clone();
            evaluate(
                &graph,
                &ears,
                output(vec![entry(
                    &ids[0],
                    suggestion(
                        "ubiquitous",
                        grounding,
                        Value::Null,
                        Value::Null,
                        find(&s, "be retained"),
                    ),
                )]),
            )
        };
        let invented = run(vec![], accepted("actor:admin", "Administrator"));
        assert!(matches!(
            invented,
            Err(EarsError::InvalidGrounding {
                component: "actor",
                ..
            })
        ));
        let proposed = run(
            vec![actor(
                "actor:admin",
                "Administrator",
                ElementStatus::Proposed,
            )],
            accepted("actor:admin", "Administrator"),
        );
        assert!(matches!(proposed, Err(EarsError::InvalidGrounding { .. })));
        let suspect = run(
            vec![actor(
                "actor:admin",
                "Administrator",
                ElementStatus::Suspect,
            )],
            accepted("actor:admin", "Administrator"),
        );
        assert!(matches!(suspect, Err(EarsError::InvalidGrounding { .. })));
        let fuzzy = run(
            vec![actor("actor:admin", "Employee", ElementStatus::Accepted)],
            accepted("actor:admin", "employee"),
        );
        assert!(matches!(fuzzy, Err(EarsError::InvalidGrounding { .. })));
        let grounded = run(
            vec![actor(
                "actor:admin",
                "Administrator",
                ElementStatus::Accepted,
            )],
            accepted("actor:admin", "Administrator"),
        )
        .unwrap();
        assert_eq!(
            replaced(&grounded.proposals[0]).statement,
            "Administrator shall be retained."
        );
    }

    #[test]
    fn ears_trigger_condition_response_invention_is_rejected() {
        let text = "When an order arrives, the clerk shall pack the order.";
        let (graph, ids) = pipeline(
            &[text],
            vec![actor("actor:clerk", "Clerk", ElementStatus::Accepted)],
        );
        let ears = request(&graph, &ids);
        let s = ears.context.requirements[0].source_statement.clone();
        let invent = accepted("actor:clerk", "an invented event");
        for (pattern, trigger, condition, response, component) in [
            (
                "event_driven",
                invent.clone(),
                Value::Null,
                find(&s, "pack the order"),
                "trigger",
            ),
            (
                "state_driven",
                Value::Null,
                invent.clone(),
                find(&s, "pack the order"),
                "condition",
            ),
            (
                "event_driven",
                find(&s, "an order arrives"),
                Value::Null,
                invent.clone(),
                "response",
            ),
            (
                "event_driven",
                find(&s, "an order arrives"),
                Value::Null,
                accepted("actor:unknown", "pack"),
                "response",
            ),
        ] {
            let out = output(vec![entry(
                &ids[0],
                suggestion(pattern, find(&s, "the clerk"), trigger, condition, response),
            )]);
            assert!(
                matches!(evaluate(&graph, &ears, out), Err(EarsError::InvalidGrounding { component: c, .. }) if c == component),
                "{component}"
            );
        }
        let ok = output(vec![entry(
            &ids[0],
            suggestion(
                "event_driven",
                accepted("actor:clerk", "Clerk"),
                find(&s, "an order arrives"),
                Value::Null,
                find(&s, "pack the order"),
            ),
        )]);
        let result = evaluate(&graph, &ears, ok).unwrap();
        assert_eq!(
            replaced(&result.proposals[0]).statement,
            "When an order arrives, Clerk shall pack the order."
        );
    }

    // ------------------------------------------------------------------ outcomes

    #[test]
    fn ears_absent_null_and_invalid_inference() {
        let (graph, ids) = pipeline(&[EXPORT, "The system shall archive the report."], vec![]);
        let ears = request(&graph, &ids);
        let absent = evaluate_ears_normalization(&graph, &ears, None).unwrap();
        assert!(absent.proposals.is_empty());
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(
            absent.issues,
            sorted
                .iter()
                .map(|r| EarsIssue::InferenceUnavailable {
                    requirement_ref: r.clone()
                })
                .collect::<Vec<_>>()
        );
        let nulls = evaluate(
            &graph,
            &ears,
            output(ids.iter().map(|r| entry(r, Value::Null)).collect()),
        )
        .unwrap();
        assert!(nulls.proposals.is_empty());
        assert_eq!(
            nulls.issues.iter().map(|i| i.as_str()).collect::<Vec<_>>(),
            ["no_suggestion", "no_suggestion"]
        );
        let good = execution(
            &ears.request,
            output(ids.iter().map(|r| entry(r, Value::Null)).collect()),
        )
        .artifact;
        let mut wrong_request = good.clone();
        wrong_request.request_hash = Hash::content_sha256(b"another request");
        let mut wrong_provider = good.clone();
        wrong_provider.provider = "other-provider".to_owned();
        let mut wrong_hash = good.clone();
        wrong_hash.validated_output_hash = Hash::content_sha256(b"other");
        for bad in [wrong_request, wrong_provider, wrong_hash] {
            assert!(matches!(
                evaluate_with(&graph, &ears, &bad),
                Err(EarsError::InvalidInferenceArtifact { .. })
            ));
        }
    }

    #[test]
    fn ears_output_coverage() {
        let (graph, ids) = pipeline(
            &[
                EXPORT,
                "The system shall archive the report.",
                "The system shall print the report.",
            ],
            vec![],
        );
        let ears = request(&graph, &ids);
        let null = |r: &Id| entry(r, Value::Null);
        assert!(matches!(
            evaluate(&graph, &ears, output(vec![null(&ids[0]), null(&ids[1])])),
            Err(EarsError::OutputCoverage { requirement_ref }) if requirement_ref == ids[2]
        ));
        assert!(matches!(
            evaluate(
                &graph,
                &ears,
                output(vec![
                    null(&ids[0]),
                    null(&ids[1]),
                    null(&ids[2]),
                    null(&ids[2])
                ])
            ),
            Err(EarsError::DuplicateRequirementOutput { .. })
        ));
        // An unknown target is reported before a malformed component of it.
        let unknown = id(REQ);
        let bad_component = entry(
            &unknown,
            suggestion(
                "ubiquitous",
                range(0, 999),
                Value::Null,
                Value::Null,
                range(0, 3),
            ),
        );
        assert!(matches!(
            evaluate(&graph, &ears, output(vec![null(&ids[0]), null(&ids[1]), null(&ids[2]), bad_component])),
            Err(EarsError::UnknownRequirement { requirement_ref }) if requirement_ref == unknown
        ));
    }

    #[test]
    fn ears_mixed_batch_and_ordering() {
        let a = "Always, the clerk shall pack the order.";
        let b = "The system shall archive the report.";
        let (graph, ids) = pipeline(&[a, b, EXPORT], vec![]);
        let ears = request(&graph, &ids);
        let entries = vec![
            entry(
                &ids[0],
                suggestion(
                    "state_driven",
                    find(a, "the clerk"),
                    Value::Null,
                    find(a, "Always"),
                    find(a, "pack the order"),
                ),
            ),
            entry(&ids[1], Value::Null),
            entry(
                &ids[2],
                ubiquitous(EXPORT, "The system", "export the report"),
            ),
        ];
        let result = evaluate(&graph, &ears, output(entries.clone())).unwrap();
        assert_eq!(result.proposals.len(), 1);
        assert_eq!(
            replaced(&result.proposals[0]).statement,
            "While Always, the clerk shall pack the order."
        );
        let mut expected = vec![
            EarsIssue::NoSuggestion {
                requirement_ref: ids[1].clone(),
            },
            EarsIssue::AlreadyNormalized {
                requirement_ref: ids[2].clone(),
            },
        ];
        expected.sort_by(|x, y| {
            (x.requirement_ref(), x.as_str()).cmp(&(y.requirement_ref(), y.as_str()))
        });
        assert_eq!(result.issues, expected);
        let mut reversed = entries;
        reversed.reverse();
        let again = evaluate(&graph, &ears, output(reversed)).unwrap();
        assert_eq!(again, result);
        assert_eq!(
            serde_json::to_vec(&again).unwrap(),
            serde_json::to_vec(&result).unwrap()
        );
    }

    #[test]
    fn ears_one_proposal_per_requirement_sorted() {
        let texts = [
            "Always, the clerk shall pack the order.",
            "Always, the clerk shall ship the order.",
            "Always, the clerk shall bill the order.",
        ];
        let (graph, ids) = pipeline(&texts, vec![]);
        let ears = request(&graph, &ids);
        let entries: Vec<Value> = ids
            .iter()
            .zip(texts)
            .map(|(r, t)| {
                let verb = t.split_whitespace().nth(4).unwrap();
                entry(
                    r,
                    suggestion(
                        "state_driven",
                        find(t, "the clerk"),
                        Value::Null,
                        find(t, "Always"),
                        find(t, &format!("{verb} the order")),
                    ),
                )
            })
            .collect();
        let result = evaluate(&graph, &ears, output(entries)).unwrap();
        assert_eq!(result.proposals.len(), 3);
        let targets: Vec<&Id> = result
            .proposals
            .iter()
            .map(|p| match &p.patch_set.patch {
                SemanticPatch::ReplacePayload { target, .. } => &target.id,
                other => panic!("{other:?}"),
            })
            .collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(targets, sorted.iter().collect::<Vec<_>>());
    }

    // ------------------------------------------------------------------ proposals and patches

    fn populated(graph: &Graph, req: &Id) -> Graph {
        modify(graph, req, |n| {
            if let NodePayload::Requirement(r) = &mut n.payload {
                r.title = Some("Pack".to_owned());
                r.rationale = Some("Orders ship packed.".to_owned());
                r.priority = Some("high".to_owned());
                r.source_identifier = Some("REQ-9".to_owned());
                r.verification_method = Some("test".to_owned());
                r.owner_refs = Some(vec![id("actor:owner")]);
                r.stakeholder_refs = Some(vec![id("stakeholder:clerk")]);
            }
            n.tags.insert("pilot".to_owned());
        })
    }

    #[test]
    fn ears_proposal_and_patch() {
        let text = "Always, the clerk shall pack the order.";
        for status in [ElementStatus::Proposed, ElementStatus::Accepted] {
            let (graph, req) = single(text);
            let graph = populated(&modify(&graph, &req, |n| n.status = status), &req);
            let before_node = graph.node(&req).unwrap().clone();
            let before = requirement_of(&graph, &req);
            let ears = request(&graph, std::slice::from_ref(&req));
            let out = output(vec![entry(
                &req,
                suggestion(
                    "state_driven",
                    find(text, "the clerk"),
                    Value::Null,
                    find(text, "Always"),
                    find(text, "pack the order"),
                ),
            )]);
            let mut provider = MockProvider::new();
            provider
                .register(ears.request.id.clone(), execution(&ears.request, out))
                .unwrap();
            let executed = provider.execute(&ears.request).unwrap();
            let result = evaluate_with(&graph, &ears, &executed.artifact).unwrap();
            assert_eq!(result.proposals.len(), 1);
            let proposal = &result.proposals[0];
            assert_eq!(proposal.stage, StageId::S1);
            assert_eq!(proposal.materiality, ProposalMateriality::Semantic);
            assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanConfirm);
            assert_eq!(proposal.confidence, None);
            assert_eq!(proposal.evidence_refs, before_node.evidence);
            assert_eq!(proposal.derivation_refs, vec![derivation()]);
            assert_eq!(
                proposal.patch_set.base_semantic_hash,
                graph.semantic_hash().unwrap()
            );
            proposal.validate().unwrap();
            let SemanticPatch::ReplacePayload { target, payload } = &proposal.patch_set.patch
            else {
                panic!("expected ReplacePayload");
            };
            assert_eq!(target.id, req);
            assert_eq!(
                target.expected_hash,
                node_element_hash(&before_node).unwrap()
            );
            assert_eq!(
                target.expected_hash,
                Hash::content_sha256(
                    &to_canonical_json(&node_element_projection(&before_node)).unwrap()
                )
            );
            let NodePayload::Requirement(after) = payload else {
                panic!()
            };
            assert_eq!(
                after.statement,
                "While Always, the clerk shall pack the order."
            );
            assert_eq!(
                Requirement {
                    statement: before.statement.clone(),
                    ..after.clone()
                },
                before
            );
            let serialized = serde_json::to_string(proposal).unwrap();
            for word in [
                "ResolutionDecision",
                "Question",
                "Finding",
                "resolves",
                "raises",
            ] {
                assert!(!serialized.contains(word), "{word}");
            }

            // Test-only application by the canonical patch engine.
            let applied = apply_patch(&graph, &proposal.patch_set).unwrap();
            applied.graph.validate().unwrap();
            let node = applied.graph.node(&req).unwrap();
            assert_eq!(node.status, status);
            assert_eq!(node.evidence, before_node.evidence);
            assert_eq!(node.derivations, before_node.derivations);
            assert_eq!(node.standards, before_node.standards);
            assert_eq!(node.tags, before_node.tags);
            assert_eq!(node.extensions, before_node.extensions);
            assert_eq!(node.audit, before_node.audit);
            assert_eq!(
                node.revision,
                before_node.revision + 1,
                "patch engine finalizes the revision"
            );
            assert_eq!(
                requirement_of(&applied.graph, &req).statement,
                after.statement
            );
            // The original is recovered from the preserved evidence and segment origin.
            let recovered = request(&applied.graph, std::slice::from_ref(&req));
            let context = &recovered.context.requirements[0];
            assert_eq!(context.current_statement, after.statement);
            assert_eq!(context.source_statement, text);
            assert_eq!(
                context.source_statement,
                ears.context.requirements[0].source_statement
            );
        }
    }

    #[test]
    fn ears_stale_patches_are_rejected() {
        let text = "Always, the clerk shall pack the order.";
        let (graph, req) = single(text);
        let ears = request(&graph, std::slice::from_ref(&req));
        let out = output(vec![entry(
            &req,
            suggestion(
                "state_driven",
                find(text, "the clerk"),
                Value::Null,
                find(text, "Always"),
                find(text, "pack the order"),
            ),
        )]);
        let proposal = evaluate(&graph, &ears, out.clone())
            .unwrap()
            .proposals
            .remove(0);
        // Proposed target: the semantic hash ignores it, the element precondition does not.
        let changed = modify(&graph, &req, |n| {
            if let NodePayload::Requirement(r) = &mut n.payload {
                r.statement = "Changed meanwhile.".to_owned();
            }
        });
        assert_eq!(
            changed.semantic_hash().unwrap(),
            graph.semantic_hash().unwrap()
        );
        assert!(apply_patch(&changed, &proposal.patch_set).is_err());
        // Accepted target: a changed accepted semantic graph is a stale base.
        let accepted = modify(&graph, &req, |n| n.status = ElementStatus::Accepted);
        let ears = request(&accepted, std::slice::from_ref(&req));
        let proposal = evaluate(&accepted, &ears, out).unwrap().proposals.remove(0);
        let grown = with_nodes(
            &accepted,
            vec![actor("actor:new", "New", ElementStatus::Accepted)],
        );
        assert!(matches!(
            apply_patch(&grown, &proposal.patch_set),
            Err(PatchError::StaleBase { .. })
        ));
        assert!(apply_patch(&accepted, &proposal.patch_set).is_ok());
    }

    #[test]
    fn ears_current_and_source_statements_diverge() {
        let text = "Always, the clerk shall pack the order.";
        let (graph, req) = single(text);
        let normalized = "While Always, the clerk shall pack the order.";
        let graph = modify(&graph, &req, |n| {
            if let NodePayload::Requirement(r) = &mut n.payload {
                r.statement = normalized.to_owned();
            }
        });
        let ears = request(&graph, std::slice::from_ref(&req));
        let r = &ears.context.requirements[0];
        assert_eq!(r.current_statement, normalized);
        assert_eq!(r.source_statement, text);
        // Ranges resolve against the source statement, not the current statement.
        let again = output(vec![entry(
            &req,
            suggestion(
                "state_driven",
                find(text, "the clerk"),
                Value::Null,
                find(text, "Always"),
                find(text, "pack the order"),
            ),
        )]);
        let result = evaluate(&graph, &ears, again).unwrap();
        assert_eq!(
            result.issues,
            vec![EarsIssue::AlreadyNormalized {
                requirement_ref: req.clone()
            }]
        );
        let back = output(vec![entry(
            &req,
            ubiquitous(text, "the clerk", "pack the order"),
        )]);
        let result = evaluate(&graph, &ears, back).unwrap();
        assert_eq!(
            replaced(&result.proposals[0]).statement,
            "the clerk shall pack the order."
        );
    }

    #[test]
    fn ears_vocabulary_and_issue_wire() {
        assert_eq!(
            EarsPattern::ALL.map(|p| p.as_str()),
            [
                "ubiquitous",
                "event_driven",
                "state_driven",
                "optional_feature",
                "unwanted_behavior"
            ]
        );
        assert!(serde_json::from_value::<EarsPattern>(json!("Ubiquitous")).is_err());
        assert_eq!(
            serde_json::to_value(EarsGrounding::SourceRange { start: 0, end: 3 }).unwrap(),
            json!({"kind": "source_range", "start": 0, "end": 3})
        );
        assert!(
            serde_json::from_value::<EarsGrounding>(json!({"kind": "free_text", "text": "x"}))
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(EarsIssue::NoSuggestion {
                requirement_ref: id(REQ)
            })
            .unwrap(),
            json!({"issue": "no_suggestion", "requirement_ref": REQ})
        );
    }
}
