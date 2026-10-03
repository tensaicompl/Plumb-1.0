//! S1.1 contract tests for requirement and intent compilation: the classification request,
//! inference validation, the source-prefix parser, modality, proposals and the HR corpus.
//!
//! Every test lives in `requirements_contract` so that `cargo test -p plumb-functional
//! requirements` selects them. Golden values were computed independently with Python
//! `hashlib` over RFC 8785 JSON, never with the helpers under test.

mod requirements_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use jsonschema::{Draft, JSONSchema};
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_functional::*;
    use plumb_import::{
        build_segmentation_request, evaluate_segmentation, import_docx, import_markdown,
        import_plain_text, ImportAudit, SegmentCandidate, SegmentClassification, SegmentationAudit,
    };
    use plumb_inference::{
        InferenceArtifact, InferenceProvider, MockProvider, ProviderExecution, ProviderPolicy,
    };
    use plumb_patch::{
        apply_patch, AcceptancePolicy, Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        AuditMeta, ConstraintCategory, ConstraintStrength, DerivationRef, ElementStatus,
        EvidenceRef, Graph, Modality, Node, NodePayload, Requirement, RequirementKind,
        RequirementLevel, Stakeholder,
    };
    use serde_json::{json, Value};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s1-classify-requirement.md");
    const SCHEMA: &str =
        include_str!("../../../schemas/inference/s1-requirement-classification.schema.json");
    const HR_MD: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.md");
    const HR_DOCX: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.docx");
    const HR_CONTRACT: &str = include_str!("../../../fixtures/hr-leave/fixture-contract.yaml");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:ebf9de1ea95e89fb8892e1d7d06448efd65e8656d94d4e1090751896a619a0be";
    const SCHEMA_HASH: &str =
        "sha256:16abf2288daac76199316fd44c1db682347fcda50d0b24e6cce5345b3e869101";
    const GOLDEN_CANDIDATE: &str = "seg:31d8789f008efd04";
    const GOLDEN_FRAGMENT: &str = "evd:58386a9177fc2196";
    const GOLDEN_CONTEXT_HASH: &str =
        "sha256:fbe4e912ae613c1ec09b4f6c5f63c275a9bd88a1cdb99d5ba0bb9f7d282e9978";
    const GOLDEN_REQUEST_ID: &str =
        "sha256:4cb0493b723ecf5d671cbaccaef356237f5e15d4268c8204a93742bbb574cc52";
    const GOLDEN_REQ_ID: &str = "req:7a9cbb41f79dc0a6";
    const GOLDEN_CONSTRAINT_ID: &str = "constraint:db381bfb88247eed";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const STAKEHOLDER: &str = "stakeholder:employee";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn import_audit_at(at: &str) -> ImportAudit {
        ImportAudit {
            created_by: id("actor:importer"),
            created_at: ts(at),
        }
    }

    fn audit_at(at: &str) -> RequirementCompilationAudit {
        RequirementCompilationAudit {
            created_by: id("agent:requirements-compiler"),
            created_at: ts(at),
        }
    }

    fn audit() -> RequirementCompilationAudit {
        audit_at(AT)
    }

    fn policy(provider: &str) -> ProviderPolicy {
        ProviderPolicy {
            provider: provider.to_owned(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn derivation() -> DerivationRef {
        DerivationRef::from(id("drv:00000000000000c1"))
    }

    fn stakeholder_node(node_id: &str, name: &str, status: ElementStatus) -> Node {
        Node {
            id: id(node_id),
            revision: 1,
            status,
            payload: NodePayload::Stakeholder(Stakeholder {
                name: name.to_owned(),
                stakeholder_kind: "role".to_owned(),
                organization: None,
                responsibilities: None,
                contact_ref: None,
            }),
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::new(),
            audit: AuditMeta::new(id("actor:human"), ts(AT), None, None).unwrap(),
        }
    }

    fn graph_of(project: &str, imported: plumb_import::ImportedSource, extra: Vec<Node>) -> Graph {
        let mut nodes = vec![imported.source];
        nodes.extend(imported.fragments);
        nodes.extend(extra);
        Graph::new(id(project), id(PROFILE), nodes, Vec::new()).unwrap()
    }

    fn fragments_of(graph: &Graph) -> Vec<Node> {
        graph
            .nodes()
            .values()
            .filter(|n| matches!(n.payload, NodePayload::EvidenceFragment(_)))
            .cloned()
            .collect()
    }

    /// Segments every fragment of `graph` as one whole requirement candidate through a mock
    /// S0.4 segmentation artifact, so candidate IDs come from S0.4 itself.
    fn whole_candidates(graph: &Graph) -> Vec<SegmentCandidate> {
        let fragments = fragments_of(graph);
        let request = build_segmentation_request(&fragments, policy("mock")).unwrap();
        let segments: Vec<Value> = request
            .context
            .fragments
            .iter()
            .map(|f| {
                json!({"fragment_ref": f.fragment_ref, "start": 0, "end": f.text.len(),
                       "classification": "requirement_candidate"})
            })
            .collect();
        let artifact = execution(
            &request.request,
            json!({"version": 1, "segments": segments}),
        )
        .artifact;
        let audit = SegmentationAudit {
            created_by: id("agent:segmenter"),
            created_at: ts(AT),
        };
        evaluate_segmentation(&request, &fragments, Some(&artifact), &audit)
            .unwrap()
            .candidates
    }

    /// A graph with one plain-text paragraph per entry, its whole-fragment candidates and
    /// optional extra nodes.
    fn setup_with(paragraphs: &[&str], extra: Vec<Node>) -> (Graph, Vec<SegmentCandidate>) {
        let text = paragraphs.join("\n\n");
        let imported =
            import_plain_text("synthetic.txt", text.as_bytes(), &import_audit_at(AT)).unwrap();
        assert_eq!(imported.fragments.len(), paragraphs.len());
        let graph = graph_of(PROJECT, imported, extra);
        let candidates = whole_candidates(&graph);
        (graph, candidates)
    }

    fn setup(paragraphs: &[&str]) -> (Graph, Vec<SegmentCandidate>) {
        setup_with(paragraphs, Vec::new())
    }

    fn execution(request: &plumb_inference::InferenceRequest, output: Value) -> ProviderExecution {
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

    fn request_for(
        graph: &Graph,
        candidates: &[SegmentCandidate],
    ) -> RequirementClassificationRequest {
        build_requirement_classification_request(graph, candidates, policy("mock")).unwrap()
    }

    fn classification(candidate: &Id, kind: Value, level: Value) -> Value {
        json!({"candidate_ref": candidate, "requirement_kind": kind, "level": level})
    }

    fn output(classifications: Vec<Value>, intents: Vec<Value>) -> Value {
        json!({"version": 1, "classifications": classifications, "intents": intents})
    }

    /// Every candidate classified with `kind` / `level`, plus `intents`.
    fn uniform(
        request: &RequirementClassificationRequest,
        kind: Value,
        level: Value,
        intents: Vec<Value>,
    ) -> Value {
        output(
            request
                .context
                .candidates
                .iter()
                .map(|c| classification(&c.candidate_ref, kind.clone(), level.clone()))
                .collect(),
            intents,
        )
    }

    fn compile(
        graph: &Graph,
        candidates: &[SegmentCandidate],
        request: &RequirementClassificationRequest,
        output: Option<Value>,
    ) -> Result<RequirementCompilationResult, RequirementCompilationError> {
        let artifact = output.map(|o| execution(&request.request, o).artifact);
        compile_with(graph, candidates, request, artifact.as_ref(), &audit())
    }

    fn compile_with(
        graph: &Graph,
        candidates: &[SegmentCandidate],
        request: &RequirementClassificationRequest,
        artifact: Option<&InferenceArtifact>,
        audit: &RequirementCompilationAudit,
    ) -> Result<RequirementCompilationResult, RequirementCompilationError> {
        let inference = artifact.map(|artifact| RequirementClassificationInference {
            artifact,
            derivation_ref: derivation(),
        });
        compile_requirement_candidates(graph, candidates, request, inference, audit)
    }

    /// Compiles one synthetic paragraph classified with `kind` / `level`.
    fn compile_one(text: &str, kind: Value, level: Value) -> RequirementCompilationResult {
        let (graph, candidates) = setup(&[text]);
        let request = request_for(&graph, &candidates);
        let out = uniform(&request, kind, level, vec![]);
        compile(&graph, &candidates, &request, Some(out)).unwrap()
    }

    fn added_node(proposal: &Proposal) -> &Node {
        match &proposal.patch_set.patch {
            SemanticPatch::AddNode { node } => node,
            other => panic!("expected AddNode, got {other:?}"),
        }
    }

    fn requirements(result: &RequirementCompilationResult) -> Vec<(&Node, &Requirement)> {
        result
            .proposals
            .iter()
            .map(added_node)
            .filter_map(|node| match &node.payload {
                NodePayload::Requirement(r) => Some((node, r)),
                _ => None,
            })
            .collect()
    }

    fn only_requirement(result: &RequirementCompilationResult) -> Requirement {
        let reqs = requirements(result);
        assert_eq!(reqs.len(), 1, "{:?}", result.unresolved);
        reqs[0].1.clone()
    }

    fn issue_names(result: &RequirementCompilationResult) -> Vec<&'static str> {
        result.unresolved.iter().map(|i| i.as_str()).collect()
    }

    fn sha_id(prefix: &str, body: &Value) -> String {
        let digest = Hash::content_sha256(&to_canonical_json(body).unwrap());
        format!("{prefix}:{}", &digest.as_str()["sha256:".len()..][..16])
    }

    // ------------------------------------------------------------------ schema and assets

    fn compiled_schema() -> JSONSchema {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&schema)
            .unwrap()
    }

    const SEG: &str = "seg:0123456789abcdef";

    fn valid_schema_output() -> Value {
        output(
            vec![classification(
                &id(SEG),
                json!("functional"),
                json!("software"),
            )],
            vec![
                json!({"candidate_ref": SEG, "intent_kind": "goal"}),
                json!({"candidate_ref": SEG, "intent_kind": "need", "stakeholder_refs": [STAKEHOLDER]}),
                json!({"candidate_ref": SEG, "intent_kind": "concern", "name": "Privacy", "description": "Data privacy."}),
                json!({"candidate_ref": SEG, "intent_kind": "constraint", "constraint_category": "technical", "strength": "mandatory"}),
            ],
        )
    }

    #[test]
    fn requirements_schema_is_draft_2020_12_and_compiles() {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(
            schema["$schema"],
            json!("https://json-schema.org/draft/2020-12/schema")
        );
        assert!(!SCHEMA.contains("$ref"));
        let compiled = compiled_schema();
        assert!(compiled.is_valid(&valid_schema_output()));
        assert!(compiled.is_valid(&output(
            vec![classification(&id(SEG), Value::Null, Value::Null)],
            vec![]
        )));
        for field in ["confidence", "probability", "score", "modality"] {
            assert!(!SCHEMA.contains(field), "{field}");
        }
    }

    #[test]
    fn requirements_schema_rejects_invalid_outputs() {
        let compiled = compiled_schema();
        let mutate = |f: &dyn Fn(&mut Value)| {
            let mut value = valid_schema_output();
            f(&mut value);
            value
        };
        let invalid = [
            mutate(&|v| {
                v["classifications"][0]["requirement_kind"] = json!("business_requirement")
            }),
            mutate(&|v| v["classifications"][0]["level"] = json!("application")),
            mutate(&|v| v["classifications"][0]["requirement_kind"] = json!("Functional")),
            mutate(&|v| v["classifications"][0]["level"] = json!("SOFTWARE")),
            mutate(&|v| v["extra"] = json!(1)),
            mutate(&|v| v["classifications"][0]["confidence"] = json!(0.5)),
            mutate(&|v| v["intents"][0]["statement"] = json!("rewritten")),
            mutate(&|v| v["classifications"][0]["candidate_ref"] = json!("seg:0123")),
            mutate(&|v| v["classifications"][0]["candidate_ref"] = json!("seg:0123456789ABCDEF")),
            mutate(&|v| v["intents"][1]["stakeholder_refs"] = json!([])),
            mutate(&|v| v["intents"][1]["stakeholder_refs"] = json!([STAKEHOLDER, STAKEHOLDER])),
            mutate(&|v| v["intents"][3]["constraint_category"] = json!("physical")),
            mutate(&|v| v["intents"][3]["strength"] = json!("optional")),
            mutate(&|v| v["intents"][0]["intent_kind"] = json!("vision")),
            mutate(&|v| {
                v["intents"][2]
                    .as_object_mut()
                    .unwrap()
                    .remove("description")
                    .map(|_| ())
                    .unwrap()
            }),
            mutate(&|v| {
                v["classifications"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("level")
                    .map(|_| ())
                    .unwrap()
            }),
            mutate(&|v| v["version"] = json!(2)),
            mutate(&|v| {
                v.as_object_mut()
                    .unwrap()
                    .remove("intents")
                    .map(|_| ())
                    .unwrap()
            }),
        ];
        for value in invalid {
            assert!(!compiled.is_valid(&value), "{value}");
        }
    }

    #[test]
    fn requirements_prompt_and_schema_hashes_are_exact_file_bytes() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
        let (graph, candidates) = setup(&["The system shall log."]);
        let request = request_for(&graph, &candidates);
        assert_eq!(request.request.prompt_template_hash.as_str(), PROMPT_HASH);
        assert_eq!(request.request.schema_hash.as_str(), SCHEMA_HASH);
    }

    #[test]
    fn requirements_assets_have_no_placeholders() {
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
    fn requirements_production_source_guard() {
        for source in [
            include_str!("../src/requirements.rs"),
            include_str!("../src/intent.rs"),
        ] {
            for token in [
                "std::fs",
                "File::open",
                "ArtifactStore",
                "SqliteArtifactStore",
                "SqliteRevisionStore",
                "rusqlite",
                "SystemClock",
                "Clock::now",
                "Utc::now",
                "Instant::now",
                "reqwest",
                "InferenceProvider",
                "::execute",
                "MockProvider",
                "persist_inference_bundle",
                "materialize_derivation_record",
                "apply_patch",
                "ResolutionDecision",
                "commit(",
                "unsafe",
                "HR-0",
                "Europe/Warsaw",
                "leave request",
                "stakeholders.yaml",
                "\"mock\"",
            ] {
                assert!(!source.contains(token), "{token}");
            }
        }
    }

    // ------------------------------------------------------------------ request

    #[test]
    fn requirements_request_golden() {
        let (graph, candidates) = setup(&["The system shall log."]);
        let request = request_for(&graph, &candidates);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id.as_str(), GOLDEN_CANDIDATE);
        assert_eq!(candidates[0].fragment_ref.as_str(), GOLDEN_FRAGMENT);
        assert_eq!(
            serde_json::to_value(&request.context).unwrap(),
            json!({
                "version": 1,
                "project_id": PROJECT,
                "candidates": [{"candidate_ref": GOLDEN_CANDIDATE, "fragment_ref": GOLDEN_FRAGMENT,
                                "text": "The system shall log."}],
                "stakeholders": [],
            })
        );
        let inner = &request.request;
        assert_eq!(inner.stage, StageId::S1);
        assert_eq!(inner.task_kind, "requirement_classification");
        assert_eq!(
            REQUIREMENT_CLASSIFICATION_TASK_KIND,
            "requirement_classification"
        );
        assert_eq!(REQUIREMENT_CLASSIFICATION_CONTEXT_VERSION, 1);
        assert_eq!(REQUIREMENT_CLASSIFICATION_OUTPUT_VERSION, 1);
        assert_eq!(inner.input_refs, vec![id(GOLDEN_CANDIDATE)]);
        assert_eq!(inner.evidence_refs, vec![id(GOLDEN_FRAGMENT)]);
        assert_eq!(inner.context_hash.as_str(), GOLDEN_CONTEXT_HASH);
        assert_eq!(inner.provider_policy, policy("mock"));
        assert_eq!(inner.id.as_str(), GOLDEN_REQUEST_ID);
    }

    #[test]
    fn requirements_request_context_and_refs() {
        let extra = vec![
            stakeholder_node("stakeholder:manager", "Manager", ElementStatus::Accepted),
            stakeholder_node(STAKEHOLDER, "Employee", ElementStatus::Accepted),
            stakeholder_node("stakeholder:proposed", "Proposed", ElementStatus::Proposed),
            stakeholder_node("stakeholder:suspect", "Suspect", ElementStatus::Suspect),
        ];
        let (graph, mut candidates) = setup_with(
            &[
                "The system shall log.",
                "Audit records must be kept.",
                "Notes.",
            ],
            extra,
        );
        candidates[2].classification = SegmentClassification::NonRequirement;
        let request = request_for(&graph, &candidates);
        assert_eq!(
            request.context.candidates.len(),
            2,
            "non_requirement ignored"
        );
        assert_eq!(
            request.context.stakeholders,
            vec![
                RequirementClassificationStakeholder {
                    stakeholder_ref: id(STAKEHOLDER),
                    name: "Employee".to_owned(),
                },
                RequirementClassificationStakeholder {
                    stakeholder_ref: id("stakeholder:manager"),
                    name: "Manager".to_owned(),
                },
            ]
        );
        let mut expected_inputs: Vec<Id> = request
            .context
            .candidates
            .iter()
            .map(|c| c.candidate_ref.clone())
            .chain([id(STAKEHOLDER), id("stakeholder:manager")])
            .collect();
        expected_inputs.sort();
        assert_eq!(request.request.input_refs, expected_inputs);
        let mut fragments: Vec<Id> = request
            .context
            .candidates
            .iter()
            .map(|c| c.fragment_ref.clone())
            .collect();
        fragments.sort();
        assert_eq!(request.request.evidence_refs, fragments);
        assert!(request
            .context
            .candidates
            .windows(2)
            .all(|p| p[0].candidate_ref < p[1].candidate_ref));
    }

    #[test]
    fn requirements_request_is_order_independent_and_policy_sensitive() {
        let (graph, candidates) = setup(&["The system shall log.", "The system may export."]);
        let forward = request_for(&graph, &candidates);
        let mut reversed = candidates.clone();
        reversed.reverse();
        assert_eq!(request_for(&graph, &reversed), forward);
        let other =
            build_requirement_classification_request(&graph, &candidates, policy("other-provider"))
                .unwrap();
        assert_ne!(other.request.id, forward.request.id);
        assert_eq!(other.context, forward.context);
        assert_eq!(other.request.context_hash, forward.request.context_hash);
    }

    #[test]
    fn requirements_context_hash_sensitivity() {
        let (graph, candidates) = setup(&["The system shall log."]);
        let base = request_for(&graph, &candidates);
        let (other_text, other_candidates) = setup(&["The system shall log!"]);
        assert_ne!(
            request_for(&other_text, &other_candidates)
                .request
                .context_hash,
            base.request.context_hash
        );
        let imported = import_plain_text(
            "synthetic.txt",
            b"The system shall log.",
            &import_audit_at(AT),
        )
        .unwrap();
        let other_project = graph_of("project:other", imported, Vec::new());
        assert_ne!(
            request_for(&other_project, &candidates)
                .request
                .context_hash,
            base.request.context_hash
        );
        let imported = import_plain_text(
            "synthetic.txt",
            b"The system shall log.",
            &import_audit_at(AT),
        )
        .unwrap();
        let with_stakeholder = graph_of(
            PROJECT,
            imported,
            vec![stakeholder_node(
                STAKEHOLDER,
                "Employee",
                ElementStatus::Accepted,
            )],
        );
        assert_ne!(
            request_for(&with_stakeholder, &candidates)
                .request
                .context_hash,
            base.request.context_hash
        );
    }

    #[test]
    fn requirements_request_rejects_invalid_input() {
        let (graph, candidates) = setup(&["The system shall log."]);
        let invalid = |candidates: &[SegmentCandidate]| {
            assert!(matches!(
                build_requirement_classification_request(&graph, candidates, policy("mock")),
                Err(RequirementCompilationError::InvalidInput { .. })
            ));
        };
        invalid(&[]);
        let mut non = candidates.clone();
        non[0].classification = SegmentClassification::NonRequirement;
        invalid(&non);
        let mut unknown = candidates.clone();
        unknown[0].fragment_ref = id("evd:00000000000000ff");
        invalid(&unknown);
        let mut range = candidates.clone();
        range[0].end = 999;
        invalid(&range);
        invalid(&[candidates[0].clone(), candidates[0].clone()]);
    }

    #[test]
    fn requirements_request_tampering_is_rejected() {
        let (graph, candidates) = setup(&["The system shall log."]);
        let request = request_for(&graph, &candidates);
        let rebuild = |f: &dyn Fn(&mut plumb_inference::InferenceRequest)| {
            let mut tampered = request.clone();
            f(&mut tampered.request);
            tampered.request.id = tampered.request.recompute_id().unwrap();
            tampered
        };
        let tampered = [
            rebuild(&|r| r.stage = StageId::S0),
            rebuild(&|r| r.task_kind = "requirement_segmentation".to_owned()),
            rebuild(&|r| r.context_hash = Hash::content_sha256(b"other")),
            rebuild(&|r| r.prompt_template_hash = Hash::content_sha256(b"other")),
            rebuild(&|r| r.schema_hash = Hash::content_sha256(b"other")),
            rebuild(&|r| r.input_refs = vec![]),
            rebuild(&|r| r.evidence_refs = vec![id("evd:00000000000000ff")]),
        ];
        for bad in tampered {
            assert!(bad.validate().is_err());
            assert!(matches!(
                compile(&graph, &candidates, &bad, None),
                Err(RequirementCompilationError::InvalidInput { .. })
            ));
        }
        let mut stale_context = request.clone();
        stale_context.context.candidates[0].text.push('!');
        assert!(stale_context.validate().is_err());
        let (other_graph, other_candidates) = setup(&["The system shall log!"]);
        assert!(matches!(
            compile(&other_graph, &other_candidates, &request, None),
            Err(RequirementCompilationError::InvalidInput { .. })
        ));
    }

    // ------------------------------------------------------------------ parser and modality

    #[test]
    fn requirements_prefix_forms() {
        let cases: Vec<(&str, Option<&str>, Option<RequirementKind>, &str)> = vec![
            (
                "REQ-001 [security] The system shall protect data.",
                Some("REQ-001"),
                Some(RequirementKind::Security),
                "The system shall protect data.",
            ),
            (
                "4. **HR-001** [functional] The system shall log.",
                Some("HR-001"),
                Some(RequirementKind::Functional),
                "The system shall log.",
            ),
            (
                "12) A.b_c-1 [data]\tData shall be kept.  ",
                Some("A.b_c-1"),
                Some(RequirementKind::Data),
                "Data shall be kept.",
            ),
            (
                "- X1 [quality] It shall be fast.",
                Some("X1"),
                Some(RequirementKind::Quality),
                "It shall be fast.",
            ),
            (
                "* X2 [constraint] It shall use SQL.",
                Some("X2"),
                Some(RequirementKind::Constraint),
                "It shall use SQL.",
            ),
            (
                "+ X3 [transition] It shall migrate.",
                Some("X3"),
                Some(RequirementKind::Transition),
                "It shall migrate.",
            ),
            (
                "X4 [Functional] It shall work.",
                None,
                None,
                "X4 [Functional] It shall work.",
            ),
            (
                "4. The system shall log.",
                None,
                None,
                "4. The system shall log.",
            ),
            (
                "**X5** [unknown] It shall work.",
                None,
                None,
                "**X5** [unknown] It shall work.",
            ),
            (
                "1X [data] It shall work.",
                None,
                None,
                "1X [data] It shall work.",
            ),
            (
                "4.X6 [data] It shall work.",
                None,
                None,
                "4.X6 [data] It shall work.",
            ),
            (
                "  The system shall log.\t",
                None,
                None,
                "The system shall log.",
            ),
        ];
        for (text, source_identifier, explicit_kind, statement) in cases {
            let result = compile_one(text, json!("operational"), json!("system"));
            let requirement = only_requirement(&result);
            assert_eq!(
                requirement.source_identifier.as_deref(),
                source_identifier,
                "{text}"
            );
            assert_eq!(
                requirement.requirement_kind,
                explicit_kind.unwrap_or(RequirementKind::Operational),
                "{text}"
            );
            assert_eq!(requirement.statement, statement, "{text}");
        }
    }

    #[test]
    fn requirements_empty_statement() {
        let result = compile_one("REQ-9 [data] .", json!("data"), json!("system"));
        assert_eq!(only_requirement_issues(&result), ["missing_modality"]);
        let result = compile_one("REQ-9 [data] shall", json!(null), json!("system"));
        assert_eq!(only_requirement(&result).statement, "shall");
        // A whitespace-only candidate range has an empty clean statement.
        let imported = import_plain_text(
            "synthetic.txt",
            b"The system shall log.",
            &import_audit_at(AT),
        )
        .unwrap();
        let graph = graph_of(PROJECT, imported, Vec::new());
        let fragments = fragments_of(&graph);
        let request = build_segmentation_request(&fragments, policy("mock")).unwrap();
        let f = &request.context.fragments[0].fragment_ref;
        let segments: Vec<Value> = [(0, 3), (3, 4), (4, 21)]
            .iter()
            .map(|(start, end)| {
                json!({"fragment_ref": f, "start": start, "end": end,
                       "classification": "requirement_candidate"})
            })
            .collect();
        let artifact = execution(
            &request.request,
            json!({"version": 1, "segments": segments}),
        )
        .artifact;
        let segmentation_audit = SegmentationAudit {
            created_by: id("agent:segmenter"),
            created_at: ts(AT),
        };
        let candidates =
            evaluate_segmentation(&request, &fragments, Some(&artifact), &segmentation_audit)
                .unwrap()
                .candidates;
        let classification_request = request_for(&graph, &candidates);
        let blank = &candidates[1];
        assert_eq!((blank.start, blank.end), (3, 4));
        let result = compile(&graph, &candidates, &classification_request, None).unwrap();
        assert!(result
            .unresolved
            .contains(&RequirementCompilationIssue::EmptyStatement {
                candidate_ref: blank.id.clone(),
            }));
        assert_eq!(result.unresolved.len(), 3);
        let out = uniform(
            &classification_request,
            json!("data"),
            json!("system"),
            vec![],
        );
        let result = compile(&graph, &candidates, &classification_request, Some(out)).unwrap();
        assert!(result
            .unresolved
            .contains(&RequirementCompilationIssue::EmptyStatement {
                candidate_ref: blank.id.clone(),
            }));
        assert_eq!(
            requirements(&result).len(),
            1,
            "only \"system shall log.\" compiles"
        );
    }

    fn only_requirement_issues(result: &RequirementCompilationResult) -> Vec<&'static str> {
        assert!(requirements(result).is_empty());
        issue_names(result)
    }

    fn modality_of(text: &str) -> Result<Modality, Vec<RequirementCompilationIssue>> {
        let result = compile_one(text, json!("functional"), json!("software"));
        match requirements(&result).as_slice() {
            [(_, requirement)] => Ok(requirement.modality),
            _ => Err(result.unresolved.clone()),
        }
    }

    #[test]
    fn requirements_supported_modalities() {
        for (text, modality) in [
            ("The system shall log.", Modality::Shall),
            ("The system SHALL log.", Modality::Shall),
            ("The system shall not log.", Modality::ShallNot),
            ("The system Shall NOT log.", Modality::ShallNot),
            ("The system shall\n\tnot log.", Modality::ShallNot),
            ("The system should log.", Modality::Should),
            ("The system SHOULD log.", Modality::Should),
            ("The system may export the report.", Modality::May),
            ("The system MAY export the report.", Modality::May),
            ("A manager shall approve and shall record.", Modality::Shall),
            (
                "A request shall wait before it can be approved.",
                Modality::Shall,
            ),
            ("The system shall-not-log.", Modality::Shall),
            ("The system shall, not later, log.", Modality::Shall),
        ] {
            assert_eq!(modality_of(text), Ok(modality), "{text}");
        }
    }

    #[test]
    fn requirements_unsupported_and_missing_modalities() {
        for (text, token) in [
            ("The system must log.", "must"),
            ("The system MUST NOT log.", "must not"),
            ("The user can log in.", "can"),
            ("The system should not log.", "should not"),
            ("The system May Not log.", "may not"),
            ("Users must log in and shall be audited.", "must"),
        ] {
            let issues = modality_of(text).unwrap_err();
            assert_eq!(issues.len(), 1, "{text}");
            assert!(
                matches!(
                    &issues[0],
                    RequirementCompilationIssue::UnsupportedModality { token: t, .. } if t == token
                ),
                "{text}: {issues:?}"
            );
        }
        for text in ["The user cannot approve.", "Shallow scandal mayor musty."] {
            let issues = modality_of(text).unwrap_err();
            assert!(
                matches!(
                    issues.as_slice(),
                    [RequirementCompilationIssue::MissingModality { .. }]
                ),
                "{text}"
            );
        }
    }

    // ------------------------------------------------------------------ classification semantics

    #[test]
    fn requirements_explicit_kind_overrides_inference() {
        let result = compile_one(
            "REQ-001 [security] The system shall protect data.",
            json!("functional"),
            json!("software"),
        );
        let requirement = only_requirement(&result);
        assert_eq!(requirement.requirement_kind, RequirementKind::Security);
        assert_eq!(requirement.level, RequirementLevel::Software);
        assert_eq!(requirement.source_identifier.as_deref(), Some("REQ-001"));
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn requirements_no_prefix_uses_inference_kind() {
        let result = compile_one(
            "The system shall protect data.",
            json!("security"),
            json!("software"),
        );
        let requirement = only_requirement(&result);
        assert_eq!(
            requirement,
            Requirement {
                statement: "The system shall protect data.".to_owned(),
                requirement_kind: RequirementKind::Security,
                level: RequirementLevel::Software,
                modality: Modality::Shall,
                title: None,
                rationale: None,
                priority: None,
                source_identifier: None,
                verification_method: None,
                owner_refs: None,
                stakeholder_refs: None,
            }
        );
    }

    #[test]
    fn requirements_null_kind_and_level() {
        let result = compile_one(
            "The system shall protect data.",
            json!(null),
            json!("software"),
        );
        assert!(result.proposals.is_empty());
        assert_eq!(issue_names(&result), ["requirement_kind_unknown"]);
        let result = compile_one(
            "REQ-2 [functional] The system shall log.",
            json!(null),
            json!(null),
        );
        assert!(result.proposals.is_empty());
        assert_eq!(issue_names(&result), ["requirement_level_unknown"]);
        let result = compile_one("The user must log.", json!(null), json!(null));
        assert_eq!(
            issue_names(&result),
            [
                "requirement_kind_unknown",
                "requirement_level_unknown",
                "unsupported_modality"
            ]
        );
    }

    #[test]
    fn requirements_missing_inference_is_unresolved() {
        let (graph, candidates) = setup(&[
            "REQ-1 [functional] The system shall log.",
            "The user must log.",
        ]);
        let request = request_for(&graph, &candidates);
        let result = compile(&graph, &candidates, &request, None).unwrap();
        assert!(result.proposals.is_empty());
        assert_eq!(
            issue_names(&result),
            ["classification_unavailable", "classification_unavailable"]
        );
        let refs: Vec<&Id> = result
            .unresolved
            .iter()
            .map(|i| i.candidate_ref())
            .collect();
        let mut sorted = refs.clone();
        sorted.sort();
        assert_eq!(refs, sorted);
    }

    #[test]
    fn requirements_invalid_artifact_is_an_error() {
        let (graph, candidates) = setup(&["The system shall log."]);
        let request = request_for(&graph, &candidates);
        let good = execution(
            &request.request,
            uniform(&request, json!("data"), json!("system"), vec![]),
        )
        .artifact;
        let mut wrong_request = good.clone();
        wrong_request.request_hash = Hash::content_sha256(b"another request");
        let mut wrong_provider = good.clone();
        wrong_provider.provider = "other-provider".to_owned();
        let mut wrong_output_hash = good.clone();
        wrong_output_hash.validated_output_hash = Hash::content_sha256(b"other");
        for bad in [wrong_request, wrong_provider, wrong_output_hash] {
            assert!(matches!(
                compile_with(&graph, &candidates, &request, Some(&bad), &audit()),
                Err(RequirementCompilationError::InvalidInferenceArtifact { .. })
            ));
        }
    }

    #[test]
    fn requirements_schema_invalid_inference_is_an_error() {
        let (graph, candidates) = setup(&["The system shall log."]);
        let request = request_for(&graph, &candidates);
        for out in [
            uniform(
                &request,
                json!("business_requirement"),
                json!("system"),
                vec![],
            ),
            uniform(&request, json!("data"), json!("application"), vec![]),
            json!({"version": 1, "classifications": []}),
            json!({"version": 2, "classifications": [], "intents": []}),
        ] {
            assert!(matches!(
                compile(&graph, &candidates, &request, Some(out)),
                Err(RequirementCompilationError::SchemaInvalid { .. })
            ));
        }
    }

    #[test]
    fn requirements_classification_coverage() {
        let (graph, candidates) = setup(&["The system shall log.", "The system shall audit."]);
        let request = request_for(&graph, &candidates);
        let refs: Vec<Id> = request
            .context
            .candidates
            .iter()
            .map(|c| c.candidate_ref.clone())
            .collect();
        let entry = |r: &Id| classification(r, json!("data"), json!("system"));
        assert!(matches!(
            compile(&graph, &candidates, &request, Some(output(vec![entry(&refs[0])], vec![]))),
            Err(RequirementCompilationError::ClassificationCoverage { candidate_ref }) if candidate_ref == refs[1]
        ));
        assert!(matches!(
            compile(
                &graph,
                &candidates,
                &request,
                Some(output(
                    vec![entry(&refs[0]), entry(&refs[1]), entry(&refs[1])],
                    vec![]
                ))
            ),
            Err(RequirementCompilationError::DuplicateClassification { .. })
        ));
        assert!(matches!(
            compile(&graph, &candidates, &request, Some(output(vec![entry(&refs[0]), entry(&refs[1]), entry(&id(SEG))], vec![]))),
            Err(RequirementCompilationError::UnknownCandidate { candidate_ref }) if candidate_ref == id(SEG)
        ));
    }

    // ------------------------------------------------------------------ proposals

    #[test]
    fn requirements_proposal_shape_and_patch() {
        let (graph, candidates) = setup(&["REQ-001 [security] The system shall protect data."]);
        let request = request_for(&graph, &candidates);
        let out = uniform(&request, json!(null), json!("software"), vec![]);
        let mut provider = MockProvider::new();
        provider
            .register(request.request.id.clone(), execution(&request.request, out))
            .unwrap();
        let executed = provider.execute(&request.request).unwrap();
        let result = compile_with(
            &graph,
            &candidates,
            &request,
            Some(&executed.artifact),
            &audit(),
        )
        .unwrap();
        assert_eq!(result.proposals.len(), 1);
        let proposal = &result.proposals[0];
        let candidate = &candidates[0];
        assert_eq!(proposal.stage, StageId::S1);
        assert_eq!(proposal.materiality, ProposalMateriality::Semantic);
        assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanConfirm);
        assert_eq!(proposal.confidence, None);
        assert_eq!(
            proposal.evidence_refs,
            vec![EvidenceRef::from(candidate.fragment_ref.clone())]
        );
        assert_eq!(proposal.derivation_refs, vec![derivation()]);
        assert_eq!(
            proposal.patch_set.base_semantic_hash,
            graph.semantic_hash().unwrap()
        );
        proposal.validate().unwrap();
        let node = added_node(proposal);
        assert_eq!(node.status, ElementStatus::Proposed);
        assert_eq!(node.revision, 1);
        assert_eq!(
            node.evidence,
            vec![EvidenceRef::from(candidate.fragment_ref.clone())]
        );
        assert!(node.derivations.is_empty() && node.standards.is_empty() && node.tags.is_empty());
        assert_eq!(node.audit.created_by, id("agent:requirements-compiler"));
        assert_eq!(node.audit.created_at, ts(AT));
        assert_eq!(node.audit.updated_by, None);
        assert_eq!(node.audit.updated_at, None);
        let extensions: Value = serde_json::to_value(&node.extensions).unwrap();
        assert_eq!(
            extensions,
            json!({SEGMENT_ORIGIN_EXTENSION: {
                "candidate_ref": candidate.id,
                "fragment_ref": candidate.fragment_ref,
                "start": candidate.start,
                "end": candidate.end,
            }})
        );
        assert_eq!(SEGMENT_ORIGIN_EXTENSION, "plumb_functional:segment_origin");
        let origin: SegmentOrigin =
            serde_json::from_value(extensions[SEGMENT_ORIGIN_EXTENSION].clone()).unwrap();
        assert_eq!(origin.candidate_ref, candidate.id);
        // Test-only: the canonical patch engine consumes the generated AddNode.
        let applied = apply_patch(&graph, &proposal.patch_set).unwrap();
        applied.graph.validate().unwrap();
        assert_eq!(applied.graph.node(&node.id), Some(node));
    }

    #[test]
    fn requirements_requirement_id_golden() {
        let (graph, candidates) = setup(&["The system shall log."]);
        let request = request_for(&graph, &candidates);
        let constraint = json!({"candidate_ref": GOLDEN_CANDIDATE, "intent_kind": "constraint",
                                "constraint_category": "technical", "strength": "mandatory"});
        let result = compile(
            &graph,
            &candidates,
            &request,
            Some(uniform(
                &request,
                json!("data"),
                json!("system"),
                vec![constraint],
            )),
        )
        .unwrap();
        let ids: Vec<&str> = result
            .proposals
            .iter()
            .map(|p| added_node(p).id.as_str())
            .collect();
        assert_eq!(ids, [GOLDEN_CONSTRAINT_ID, GOLDEN_REQ_ID]);
        let node = added_node(&result.proposals[1]);
        assert_eq!(node.id.as_str(), GOLDEN_REQ_ID);
        assert_eq!(
            node.id.as_str(),
            sha_id(
                "req",
                &json!({"project_id": PROJECT, "candidate_ref": GOLDEN_CANDIDATE, "node_type": "requirement"})
            )
        );
        let imported = import_plain_text(
            "synthetic.txt",
            b"The system shall log.",
            &import_audit_at(AT),
        )
        .unwrap();
        let other = graph_of("project:other", imported, Vec::new());
        let other_request = request_for(&other, &candidates);
        let other_result = compile(
            &other,
            &candidates,
            &other_request,
            Some(uniform(
                &other_request,
                json!("data"),
                json!("system"),
                vec![],
            )),
        )
        .unwrap();
        assert_ne!(added_node(&other_result.proposals[0]).id, node.id);
    }

    fn intent_setup() -> (
        Graph,
        Vec<SegmentCandidate>,
        RequirementClassificationRequest,
    ) {
        let extra = vec![
            stakeholder_node(STAKEHOLDER, "Employee", ElementStatus::Accepted),
            stakeholder_node("stakeholder:manager", "Manager", ElementStatus::Accepted),
            stakeholder_node("stakeholder:proposed", "Proposed", ElementStatus::Proposed),
        ];
        let (graph, candidates) = setup_with(
            &["REQ-7 [constraint] The system shall use PostgreSQL."],
            extra,
        );
        let request = request_for(&graph, &candidates);
        (graph, candidates, request)
    }

    #[test]
    fn requirements_intent_proposals() {
        let (graph, candidates, request) = intent_setup();
        let c = &request.context.candidates[0].candidate_ref;
        let intents = vec![
            json!({"candidate_ref": c, "intent_kind": "constraint", "constraint_category": "technical", "strength": "mandatory"}),
            json!({"candidate_ref": c, "intent_kind": "need", "stakeholder_refs": ["stakeholder:manager", STAKEHOLDER]}),
            json!({"candidate_ref": c, "intent_kind": "goal"}),
            json!({"candidate_ref": c, "intent_kind": "concern", "name": "Database choice", "description": "The system uses PostgreSQL."}),
        ];
        let result = compile(
            &graph,
            &candidates,
            &request,
            Some(uniform(&request, json!(null), json!("system"), intents)),
        )
        .unwrap();
        assert_eq!(result.proposals.len(), 5);
        let ids: Vec<&Id> = result.proposals.iter().map(|p| &added_node(p).id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "proposals sorted by node ID");
        let statement = "The system shall use PostgreSQL.";
        let mut kinds = BTreeSet::new();
        for proposal in &result.proposals {
            let node = added_node(proposal);
            assert_eq!(node.status, ElementStatus::Proposed);
            assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanConfirm);
            assert_eq!(proposal.derivation_refs, vec![derivation()]);
            assert_eq!(
                node.evidence,
                vec![EvidenceRef::from(candidates[0].fragment_ref.clone())]
            );
            assert!(node
                .extensions
                .contains_key(&SEGMENT_ORIGIN_EXTENSION.parse().unwrap()));
            let (prefix, _) = node.id.as_str().split_once(':').unwrap();
            let node_type = if prefix == "req" {
                "requirement"
            } else {
                prefix
            };
            assert_eq!(
                node.id.as_str(),
                sha_id(
                    prefix,
                    &json!({"project_id": PROJECT, "candidate_ref": c, "node_type": node_type})
                )
            );
            match &node.payload {
                NodePayload::Requirement(r) => {
                    assert_eq!(r.requirement_kind, RequirementKind::Constraint);
                    kinds.insert("requirement");
                }
                NodePayload::Goal(g) => {
                    assert_eq!(g.statement, statement);
                    assert_eq!(
                        (g.success_measures.clone(), g.priority.clone()),
                        (None, None)
                    );
                    kinds.insert("goal");
                }
                NodePayload::Need(n) => {
                    assert_eq!(n.statement, statement);
                    assert_eq!(
                        n.stakeholder_refs,
                        vec![id(STAKEHOLDER), id("stakeholder:manager")]
                    );
                    assert_eq!((n.goal_refs.clone(), n.context.clone()), (None, None));
                    kinds.insert("need");
                }
                NodePayload::Concern(k) => {
                    assert_eq!(k.name, "Database choice");
                    assert_eq!(k.description, "The system uses PostgreSQL.");
                    kinds.insert("concern");
                }
                NodePayload::Constraint(k) => {
                    assert_eq!(k.statement, statement);
                    assert_eq!(k.constraint_category, ConstraintCategory::Technical);
                    assert_eq!(k.strength, ConstraintStrength::Mandatory);
                    kinds.insert("constraint");
                }
                other => panic!("unexpected payload {other:?}"),
            }
            let applied = apply_patch(&graph, &proposal.patch_set).unwrap();
            applied.graph.validate().unwrap();
        }
        assert_eq!(kinds.len(), 5);
    }

    #[test]
    fn requirements_constraint_label_creates_no_constraint_node() {
        let (graph, candidates, request) = intent_setup();
        let result = compile(
            &graph,
            &candidates,
            &request,
            Some(uniform(&request, json!(null), json!("system"), vec![])),
        )
        .unwrap();
        assert_eq!(result.proposals.len(), 1);
        assert!(matches!(
            added_node(&result.proposals[0]).payload,
            NodePayload::Requirement(_)
        ));
        // Requirement-like wording never creates Goal/Need/Concern on its own.
        let result = compile_one(
            "We want and need this concern resolved; the system shall act.",
            json!("data"),
            json!("system"),
        );
        assert_eq!(result.proposals.len(), 1);
    }

    #[test]
    fn requirements_invalid_intents_are_rejected() {
        let (graph, candidates, request) = intent_setup();
        let c = request.context.candidates[0].candidate_ref.to_string();
        let run = |intents: Vec<Value>| {
            compile(
                &graph,
                &candidates,
                &request,
                Some(uniform(&request, json!(null), json!("system"), intents)),
            )
        };
        let need = |refs: Value| json!({"candidate_ref": c, "intent_kind": "need", "stakeholder_refs": refs});
        for refs in [
            json!(["stakeholder:unknown"]),
            json!(["stakeholder:proposed"]),
            json!([candidates[0].fragment_ref]),
        ] {
            assert!(matches!(
                run(vec![need(refs)]),
                Err(RequirementCompilationError::InvalidIntentReference { .. })
            ));
        }
        assert!(matches!(
            run(vec![json!({"candidate_ref": SEG, "intent_kind": "goal"})]),
            Err(RequirementCompilationError::InvalidIntentReference { .. })
        ));
        assert!(matches!(
            run(vec![
                json!({"candidate_ref": c, "intent_kind": "goal"}),
                json!({"candidate_ref": c, "intent_kind": "goal"})
            ]),
            Err(RequirementCompilationError::DuplicateIntent { .. })
        ));
        for (name, description) in [
            (" Lead", "ok"),
            ("Trail ", "ok"),
            ("ok", "line\nbreak"),
            ("tab\there", "ok"),
            ("ok", "\u{7}"),
        ] {
            assert!(
                matches!(
                    run(vec![
                        json!({"candidate_ref": c, "intent_kind": "concern", "name": name, "description": description})
                    ]),
                    Err(RequirementCompilationError::InvalidIntent { .. })
                ),
                "{name:?} {description:?}"
            );
        }
        for bad in [
            json!({"candidate_ref": c, "intent_kind": "constraint", "constraint_category": "physical", "strength": "mandatory"}),
            json!({"candidate_ref": c, "intent_kind": "constraint", "constraint_category": "technical", "strength": "optional"}),
            json!({"candidate_ref": c, "intent_kind": "constraint", "constraint_category": "technical"}),
        ] {
            assert!(matches!(
                run(vec![bad]),
                Err(RequirementCompilationError::SchemaInvalid { .. })
            ));
        }
    }

    // ------------------------------------------------------------------ determinism

    #[test]
    fn requirements_order_and_audit_determinism() {
        let extra = vec![
            stakeholder_node("stakeholder:manager", "Manager", ElementStatus::Accepted),
            stakeholder_node(STAKEHOLDER, "Employee", ElementStatus::Accepted),
        ];
        let (graph, candidates) = setup_with(
            &[
                "The system shall log.",
                "REQ-2 [data] Data shall be kept.",
                "The user must log.",
            ],
            extra,
        );
        let request = request_for(&graph, &candidates);
        let refs: Vec<Id> = request
            .context
            .candidates
            .iter()
            .map(|c| c.candidate_ref.clone())
            .collect();
        let classifications: Vec<Value> = refs
            .iter()
            .map(|r| classification(r, json!("functional"), json!("system")))
            .collect();
        let intents = vec![
            json!({"candidate_ref": refs[0], "intent_kind": "goal"}),
            json!({"candidate_ref": refs[1], "intent_kind": "need", "stakeholder_refs": ["stakeholder:manager", STAKEHOLDER]}),
            json!({"candidate_ref": refs[0], "intent_kind": "constraint", "constraint_category": "data", "strength": "preferred"}),
        ];
        let forward = compile(
            &graph,
            &candidates,
            &request,
            Some(output(classifications.clone(), intents.clone())),
        )
        .unwrap();
        let mut reversed_candidates = candidates.clone();
        reversed_candidates.reverse();
        let mut rc = classifications.clone();
        rc.reverse();
        let mut ri = intents.clone();
        ri.reverse();
        let mut ri_need = ri.clone();
        ri_need[1]["stakeholder_refs"] = json!([STAKEHOLDER, "stakeholder:manager"]);
        let reversed = compile(
            &graph,
            &reversed_candidates,
            &request,
            Some(output(rc, ri_need)),
        )
        .unwrap();
        assert_eq!(forward, reversed);
        assert_eq!(
            serde_json::to_vec(&forward).unwrap(),
            serde_json::to_vec(
                &compile(
                    &graph,
                    &candidates,
                    &request,
                    Some(output(classifications.clone(), intents.clone()))
                )
                .unwrap()
            )
            .unwrap()
        );
        let artifact = execution(&request.request, output(classifications, intents)).artifact;
        let later = compile_with(
            &graph,
            &candidates,
            &request,
            Some(&artifact),
            &audit_at("2026-06-30T12:00:00.000000000Z"),
        )
        .unwrap();
        assert_eq!(later.unresolved, forward.unresolved);
        assert_eq!(later.proposals.len(), forward.proposals.len());
        for (a, b) in forward.proposals.iter().zip(&later.proposals) {
            let (na, nb) = (added_node(a), added_node(b));
            assert_eq!(na.id, nb.id);
            assert_eq!(na.payload, nb.payload);
            assert_eq!(na.extensions, nb.extensions);
            assert_ne!(na.audit, nb.audit);
            assert_ne!(a.id, b.id);
        }
    }

    // ------------------------------------------------------------------ HR corpus

    #[derive(serde::Deserialize)]
    struct Contract {
        requirements: Vec<ContractRequirement>,
    }

    #[derive(serde::Deserialize)]
    struct ContractRequirement {
        id: String,
        kind: String,
        statement: String,
    }

    fn contract() -> BTreeMap<String, (String, String)> {
        let contract: Contract = serde_yaml::from_str(HR_CONTRACT).unwrap();
        contract
            .requirements
            .into_iter()
            .map(|r| (r.id, (r.kind, r.statement)))
            .collect()
    }

    /// Real HR import, real S0.4 fallback segmentation, one graph per source.
    fn hr(source: &str, at: &str) -> (Graph, Vec<SegmentCandidate>) {
        let imported = match source {
            "md" => import_markdown("requirements.md", HR_MD, &import_audit_at(at)).unwrap(),
            _ => import_docx("requirements.docx", HR_DOCX, &import_audit_at(at)).unwrap(),
        };
        let fragments = imported.fragments.clone();
        let graph = graph_of("project:hr-leave", imported, Vec::new());
        let request = build_segmentation_request(&fragments, policy("mock")).unwrap();
        let audit = SegmentationAudit {
            created_by: id("agent:segmenter"),
            created_at: ts(at),
        };
        let candidates = evaluate_segmentation(&request, &fragments, None, &audit)
            .unwrap()
            .candidates;
        (graph, candidates)
    }

    /// Mock classifier output for S1.1 mechanics: null kinds (the explicit source labels are
    /// authoritative) and one fixed level. This is not an authoritative HR level; the fixture
    /// contract defines no requirement level.
    fn hr_compile(source: &str, at: &str) -> (RequirementCompilationResult, Vec<SegmentCandidate>) {
        let (graph, candidates) = hr(source, at);
        let request = request_for(&graph, &candidates);
        assert_eq!(request.context.candidates.len(), 32);
        assert!(request.context.stakeholders.is_empty());
        let out = uniform(&request, json!(null), json!("software"), vec![]);
        let mut provider = MockProvider::new();
        provider
            .register(request.request.id.clone(), execution(&request.request, out))
            .unwrap();
        let executed = provider.execute(&request.request).unwrap();
        let result = compile_with(
            &graph,
            &candidates,
            &request,
            Some(&executed.artifact),
            &audit_at(at),
        )
        .unwrap();
        (result, candidates)
    }

    fn hr_requirements(result: &RequirementCompilationResult) -> BTreeMap<String, Requirement> {
        requirements(result)
            .into_iter()
            .map(|(_, r)| (r.source_identifier.clone().unwrap(), r.clone()))
            .collect()
    }

    #[test]
    fn requirements_hr_sources_match_fixture_contract() {
        let contract = contract();
        assert_eq!(contract.len(), 32);
        let mut statements: Vec<BTreeMap<String, String>> = Vec::new();
        for source in ["md", "docx"] {
            let (result, candidates) = hr_compile(source, AT);
            assert!(
                result.unresolved.is_empty(),
                "{source}: {:?}",
                result.unresolved
            );
            assert_eq!(result.proposals.len(), 32, "{source}");
            let by_id = hr_requirements(&result);
            let expected_ids: Vec<String> = (1..=32).map(|n| format!("HR-{n:03}")).collect();
            assert_eq!(
                by_id.keys().cloned().collect::<Vec<_>>(),
                expected_ids,
                "{source}"
            );
            for (hr_id, requirement) in &by_id {
                let (kind, statement) = &contract[hr_id];
                assert_eq!(&requirement.statement, statement, "{source} {hr_id}");
                assert_eq!(
                    serde_json::to_value(requirement.requirement_kind).unwrap(),
                    json!(kind),
                    "{source} {hr_id}"
                );
                assert_eq!(requirement.level, RequirementLevel::Software, "mock level");
                let expected_modality = match hr_id.as_str() {
                    "HR-013" | "HR-014" | "HR-016" => Modality::ShallNot,
                    "HR-030" => Modality::Should,
                    _ => Modality::Shall,
                };
                assert_eq!(requirement.modality, expected_modality, "{source} {hr_id}");
            }
            for proposal in &result.proposals {
                assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanConfirm);
                assert_eq!(proposal.materiality, ProposalMateriality::Semantic);
                assert_eq!(proposal.derivation_refs, vec![derivation()]);
                let node = added_node(proposal);
                assert_eq!(node.status, ElementStatus::Proposed);
                let origin: SegmentOrigin = serde_json::from_value(
                    node.extensions[&SEGMENT_ORIGIN_EXTENSION.parse().unwrap()].clone(),
                )
                .unwrap();
                let candidate = candidates
                    .iter()
                    .find(|c| c.id == origin.candidate_ref)
                    .unwrap();
                assert_eq!(
                    candidate.classification,
                    SegmentClassification::RequirementCandidate
                );
                assert_eq!(
                    (origin.fragment_ref.clone(), origin.start, origin.end),
                    (
                        candidate.fragment_ref.clone(),
                        candidate.start,
                        candidate.end
                    )
                );
                assert_eq!(
                    node.evidence,
                    vec![EvidenceRef::from(candidate.fragment_ref.clone())]
                );
                assert_eq!(proposal.evidence_refs, node.evidence);
            }
            statements.push(by_id.into_iter().map(|(k, r)| (k, r.statement)).collect());
        }
        assert_eq!(
            statements[0], statements[1],
            "cross-format statements byte-identical"
        );
    }

    #[test]
    fn requirements_hr_reimport_preserves_ids() {
        for source in ["md", "docx"] {
            let (first, first_candidates) = hr_compile(source, AT);
            let (second, second_candidates) = hr_compile(source, "2026-09-15T08:30:00.000000000Z");
            assert_eq!(first_candidates, second_candidates);
            let ids = |r: &RequirementCompilationResult| -> Vec<Id> {
                r.proposals
                    .iter()
                    .map(|p| added_node(p).id.clone())
                    .collect()
            };
            assert_eq!(ids(&first), ids(&second), "{source}");
        }
        let (md, _) = hr_compile("md", AT);
        let (docx, _) = hr_compile("docx", AT);
        let md_ids: BTreeSet<Id> = md
            .proposals
            .iter()
            .map(|p| added_node(p).id.clone())
            .collect();
        let docx_ids: BTreeSet<Id> = docx
            .proposals
            .iter()
            .map(|p| added_node(p).id.clone())
            .collect();
        assert!(
            md_ids.is_disjoint(&docx_ids),
            "different source candidates, different IDs"
        );
    }
}
