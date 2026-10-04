//! S1.5 contract tests for span-grounded vocabulary analysis: normalization, the vocabulary
//! request, inference validation, statistics, the plumb-lint bridge, accepted-vocabulary
//! reconciliation, conflicts and F1 finding material, Term/Concept proposals and the HR path.
//!
//! Every test lives in `vocabulary_contract` so that `cargo test -p plumb-functional
//! vocabulary` selects them. Golden values were computed independently with Python
//! `hashlib` over RFC 8785 JSON, never with the helpers under test.

mod vocabulary_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use jsonschema::{Draft, JSONSchema};
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_functional::*;
    use plumb_import::{
        build_segmentation_request, evaluate_segmentation, import_markdown, import_plain_text,
        ImportAudit, ImportedSource, SegmentCandidate, SegmentationAudit,
    };
    use plumb_inference::{
        InferenceArtifact, InferenceProvider, InferenceRequest, MockProvider, ProviderExecution,
        ProviderPolicy,
    };
    use plumb_lint::{lint_requirement, LintInput, LintPolicy, LintRuleId, LintTextRange};
    use plumb_patch::{
        apply_patch, AcceptancePolicy, Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        AuditMeta, Concept, ConceptKind, DerivationRef, ElementStatus, EvidenceRef, Graph, Node,
        NodePayload, Term,
    };
    use serde_json::{json, Value};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s1-vocabulary.md");
    const SCHEMA: &str = include_str!("../../../schemas/inference/s1-vocabulary.schema.json");
    const HR_MD: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.md");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:5aa872f42a4cbb0a896bc11a6432c7fdad4339f91864414061d79caec36b86fb";
    const SCHEMA_HASH: &str =
        "sha256:611ce010870a1642d6c213b12783ac6e38b17424b0db557d60bcef495936a78c";
    const TERM_ID: &str = "term:ac50ff756b24a007";
    const CONCEPT_ID: &str = "concept:40553d4718d90342";
    const GOLDEN_REQUIREMENT: &str = "req:2e93dd96fb35eb2d";
    const GOLDEN_FRAGMENT: &str = "evd:f95315d056164f00";
    const GOLDEN_CONTEXT_HASH: &str =
        "sha256:629abc4db318b963b7ec633721dd9f7c957060bcbd3323737b40928ee5ff0374";
    const GOLDEN_REQUEST_ID: &str =
        "sha256:9acfe1b9f39db91694be61f0e961fdfb72f4bc8235cfdff86c9e0c3e598e96a5";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";

    use ConceptKind::{ObjectType, Role};
    use ElementStatus::{Accepted, Proposed};

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn policy() -> VocabularyPolicy {
        VocabularyPolicy {
            language: "en".to_owned(),
        }
    }

    fn provider(name: &str) -> ProviderPolicy {
        ProviderPolicy {
            provider: name.to_owned(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn derivation() -> DerivationRef {
        DerivationRef::from(id("drv:00000000000000f5"))
    }

    fn audit_at(at: &str) -> VocabularyAudit {
        VocabularyAudit {
            created_by: id("agent:vocabulary"),
            created_at: ts(at),
        }
    }

    fn audit() -> VocabularyAudit {
        audit_at(AT)
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

    fn semantic(node_id: &str, status: ElementStatus, payload: NodePayload) -> Node {
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

    fn term_node(
        node_id: &str,
        status: ElementStatus,
        term: &str,
        aliases: Option<Vec<&str>>,
    ) -> Node {
        semantic(
            node_id,
            status,
            NodePayload::Term(Term {
                term: term.to_owned(),
                language: "en".to_owned(),
                definition_ref: None,
                aliases: aliases.map(|a| a.into_iter().map(str::to_owned).collect()),
                status: None,
            }),
        )
    }

    fn concept_node(node_id: &str, status: ElementStatus, name: &str, kind: ConceptKind) -> Node {
        semantic(
            node_id,
            status,
            NodePayload::Concept(Concept {
                name: name.to_owned(),
                definition: format!("Accepted definition of {name}."),
                concept_kind: kind,
            }),
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

    fn with_status(graph: &Graph, targets: &[Id], status: ElementStatus) -> Graph {
        let nodes = graph
            .nodes()
            .values()
            .cloned()
            .map(|mut n| {
                if targets.contains(&n.id) {
                    n.status = status;
                }
                n
            })
            .collect();
        rebuild(graph, nodes)
    }

    /// Imported evidence, S0.4 whole-fragment candidates and S1.1 requirements (mock
    /// classifier: functional / system) applied in test code; requirement IDs in input order.
    fn requirements_graph(imported: ImportedSource, extra: Vec<Node>) -> (Graph, Vec<Id>) {
        let fragments = imported.fragments.clone();
        let mut nodes = vec![imported.source];
        nodes.extend(imported.fragments);
        nodes.extend(extra);
        let mut graph = Graph::new(id(PROJECT), id(PROFILE), nodes, Vec::new()).unwrap();
        let segmentation = build_segmentation_request(&fragments, provider("mock")).unwrap();
        let segmentation_audit = SegmentationAudit {
            created_by: id("agent:segmenter"),
            created_at: ts(AT),
        };
        let candidates: Vec<SegmentCandidate> =
            evaluate_segmentation(&segmentation, &fragments, None, &segmentation_audit)
                .unwrap()
                .candidates;
        let classification =
            build_requirement_classification_request(&graph, &candidates, provider("mock"))
                .unwrap();
        let entries: Vec<Value> = classification
            .context
            .candidates
            .iter()
            .map(|c| json!({"candidate_ref": c.candidate_ref, "requirement_kind": "functional", "level": "system"}))
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
                created_by: id("agent:requirements"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        let mut by_fragment = BTreeMap::new();
        for proposal in &compiled.proposals {
            graph = apply_patch(&graph, &proposal.patch_set).unwrap().graph;
            if let SemanticPatch::AddNode { node } = &proposal.patch_set.patch {
                by_fragment.insert(node.evidence[0].as_id().clone(), node.id.clone());
            }
        }
        let ids = fragments
            .iter()
            .filter_map(|f| by_fragment.get(&f.id).cloned())
            .collect();
        (graph, ids)
    }

    /// One requirement per paragraph (each must contain shall so S0.4 fallback selects it).
    fn setup_with(paragraphs: &[&str], extra: Vec<Node>) -> (Graph, Vec<Id>) {
        let audit = ImportAudit {
            created_by: id("actor:importer"),
            created_at: ts(AT),
        };
        let imported =
            import_plain_text("synthetic.txt", paragraphs.join("\n\n").as_bytes(), &audit).unwrap();
        let (graph, ids) = requirements_graph(imported, extra);
        assert_eq!(ids.len(), paragraphs.len());
        (graph, ids)
    }

    fn setup(paragraphs: &[&str]) -> (Graph, Vec<Id>) {
        setup_with(paragraphs, Vec::new())
    }

    fn statement(graph: &Graph, req: &Id) -> String {
        match &graph.node(req).unwrap().payload {
            NodePayload::Requirement(r) => r.statement.clone(),
            other => panic!("{other:?}"),
        }
    }

    fn request(graph: &Graph, targets: &[Id]) -> VocabularyRequest {
        build_vocabulary_request(graph, targets, &policy(), provider("mock")).unwrap()
    }

    /// A mention of the `nth` occurrence of `part` in the requirement statement.
    fn mention_nth(
        graph: &Graph,
        req: &Id,
        part: &str,
        nth: usize,
        kind: Value,
        definition: Value,
    ) -> Value {
        let s = statement(graph, req);
        let start = s
            .match_indices(part)
            .nth(nth)
            .unwrap_or_else(|| panic!("{part:?} in {s:?}"))
            .0;
        json!({"requirement_ref": req, "start": start, "end": start + part.len(), "concept_kind": kind, "definition": definition})
    }

    fn mention(graph: &Graph, req: &Id, part: &str) -> Value {
        mention_nth(graph, req, part, 0, Value::Null, Value::Null)
    }

    fn typed(graph: &Graph, req: &Id, part: &str, kind: &str, definition: Value) -> Value {
        mention_nth(graph, req, part, 0, json!(kind), definition)
    }

    fn grounding(graph: &Graph, req: &Id, part: &str) -> Value {
        let s = statement(graph, req);
        let start = s.find(part).unwrap();
        json!({"requirement_ref": req, "start": start, "end": start + part.len()})
    }

    fn output(mentions: Vec<Value>) -> Value {
        json!({"version": 1, "mentions": mentions})
    }

    fn analyze_with(
        graph: &Graph,
        req: &VocabularyRequest,
        artifact: &InferenceArtifact,
        audit: &VocabularyAudit,
    ) -> Result<VocabularyAnalysisResult, VocabularyError> {
        analyze_vocabulary(
            graph,
            req,
            &policy(),
            Some(VocabularyInference {
                artifact,
                derivation_ref: derivation(),
            }),
            audit,
        )
    }

    fn analyze(
        graph: &Graph,
        req: &VocabularyRequest,
        mentions: Vec<Value>,
    ) -> Result<VocabularyAnalysisResult, VocabularyError> {
        let artifact = execution(&req.request, output(mentions)).artifact;
        analyze_with(graph, req, &artifact, &audit())
    }

    fn added(proposal: &Proposal) -> &Node {
        match &proposal.patch_set.patch {
            SemanticPatch::AddNode { node } => node,
            other => panic!("{other:?}"),
        }
    }

    fn terms(result: &VocabularyAnalysisResult) -> Vec<(&Node, &Term)> {
        result
            .proposals
            .iter()
            .map(added)
            .filter_map(|n| match &n.payload {
                NodePayload::Term(t) => Some((n, t)),
                _ => None,
            })
            .collect()
    }

    fn concepts(result: &VocabularyAnalysisResult) -> Vec<(&Node, &Concept)> {
        result
            .proposals
            .iter()
            .map(added)
            .filter_map(|n| match &n.payload {
                NodePayload::Concept(c) => Some((n, c)),
                _ => None,
            })
            .collect()
    }

    fn keys(result: &VocabularyAnalysisResult) -> Vec<&str> {
        result
            .candidates
            .iter()
            .map(|c| c.normalized_key.as_str())
            .collect()
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

    const REQ: &str = "req:0123456789abcdef";

    fn raw(kind: Value, definition: Value) -> Value {
        json!({"requirement_ref": REQ, "start": 0, "end": 5, "concept_kind": kind, "definition": definition})
    }

    #[test]
    fn vocabulary_schema_validity() {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(
            schema["$schema"],
            json!("https://json-schema.org/draft/2020-12/schema")
        );
        assert!(!SCHEMA.contains("$ref"));
        for field in [
            "\"term\"",
            "\"surface\"",
            "\"normalized_key\"",
            "\"name\"",
            "\"alias",
            "\"frequency\"",
            "\"cooccurrence\"",
        ] {
            assert!(!SCHEMA.contains(field), "{field}");
        }
        let compiled = compiled_schema();
        let range = json!({"requirement_ref": REQ, "start": 0, "end": 3});
        let mut valid = vec![output(vec![]), output(vec![raw(Value::Null, Value::Null)])];
        for kind in ["object_type", "fact_type", "value_type", "role", "other"] {
            valid.push(output(vec![raw(json!(kind), Value::Null)]));
            valid.push(output(vec![raw(json!(kind), range.clone())]));
        }
        for v in valid {
            assert!(compiled.is_valid(&v), "{v}");
        }
        let mut invalid: Vec<Value> = [
            "entity",
            "attribute",
            "actor",
            "value",
            "system",
            "unknown",
            "Object_Type",
        ]
        .iter()
        .map(|k| output(vec![raw(json!(k), Value::Null)]))
        .collect();
        let with = |f: &dyn Fn(&mut Value)| {
            let mut m = raw(json!("role"), range.clone());
            f(&mut m);
            output(vec![m])
        };
        invalid.extend([
            json!({"version": 1, "mentions": [], "extra": 1}),
            json!({"version": 2, "mentions": []}),
            with(&|m| m["term"] = json!("employee")),
            with(&|m| m["normalized_key"] = json!("employee")),
            with(&|m| m["definition"] = json!("A person employed.")),
            with(&|m| m["definition_text"] = json!("A person employed.")),
            with(&|m| m["definition"]["extra"] = json!(1)),
            with(&|m| {
                m.as_object_mut().unwrap().remove("definition");
            }),
            output(vec![raw(Value::Null, range.clone())]),
        ]);
        for v in invalid {
            assert!(!compiled.is_valid(&v), "{v}");
        }
    }

    #[test]
    fn vocabulary_prompt_and_schema_hashes() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
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

    // ------------------------------------------------------------------ normalization

    #[test]
    fn vocabulary_normalization_table() {
        let n = |s: &str| normalize_vocabulary_term(s);
        for (input, expected) in [
            ("The Employees", "employee"),
            ("leave-requests", "leave request"),
            ("leave request", "leave request"),
            ("leave/request", "leave request"),
            ("Policies", "policy"),
            ("categories", "category"),
            ("Boxes", "box"),
            ("classes", "class"),
            ("churches", "church"),
            ("wishes", "wish"),
            ("quizzes", "quizz"),
            ("Statuses", "status"),
            ("Business", "business"),
            ("Analysis", "analysis"),
            ("status", "status"),
            ("Data", "data"),
            ("Café", "café"),
            ("requests", "request"),
            ("cases", "case"),
            ("a leave request", "leave request"),
            ("an employee", "employee"),
            ("the manager", "manager"),
            ("the head of the departments", "head of the department"),
            ("the the requests", "the request"),
            ("EMPLOYEE", "employee"),
            ("employees' records", "employees record"),
            ("2 days", "2 day"),
            ("ies", "ie"),
        ] {
            assert_eq!(n(input).as_deref(), Some(expected), "{input}");
        }
        // NFKC: composed and decomposed é, and the ﬁ ligature.
        assert_eq!(n("Cafe\u{301}"), n("Caf\u{e9}"));
        assert_eq!(n("ﬁle"), Some("file".to_owned()));
        for empty in ["---", "the", " ", "A", "..."] {
            assert_eq!(n(empty), None, "{empty:?}");
        }
    }

    #[test]
    fn vocabulary_exception_tables() {
        for (plural, singular) in [
            ("analyses", "analysis"),
            ("buses", "bus"),
            ("children", "child"),
            ("feet", "foot"),
            ("geese", "goose"),
            ("indices", "index"),
            ("matrices", "matrix"),
            ("men", "man"),
            ("mice", "mouse"),
            ("people", "person"),
            ("statuses", "status"),
            ("teeth", "tooth"),
            ("vertices", "vertex"),
            ("women", "woman"),
        ] {
            assert_eq!(
                normalize_vocabulary_term(plural).as_deref(),
                Some(singular),
                "{plural}"
            );
        }
        for invariant in [
            "data",
            "equipment",
            "gas",
            "information",
            "metadata",
            "news",
            "series",
            "software",
            "species",
            "staff",
        ] {
            assert_eq!(
                normalize_vocabulary_term(invariant).as_deref(),
                Some(invariant)
            );
        }
        // Only the final token is singularized.
        assert_eq!(
            normalize_vocabulary_term("men requests").as_deref(),
            Some("men request")
        );
        assert_eq!(
            normalize_vocabulary_term("request men").as_deref(),
            Some("request man")
        );
        let table = include_str!("../src/vocabulary_exceptions.rs");
        assert_eq!(table.matches("(\"").count(), 14);
    }

    // ------------------------------------------------------------------ request

    #[test]
    fn vocabulary_request_golden() {
        let (graph, ids) = setup(&["The employee shall submit a leave request."]);
        let req = request(&graph, &ids);
        let fragment = graph.node(&ids[0]).unwrap().evidence[0].as_id().clone();
        assert_eq!(ids[0].as_str(), GOLDEN_REQUIREMENT);
        assert_eq!(fragment.as_str(), GOLDEN_FRAGMENT);
        assert_eq!(
            serde_json::to_value(&req.context).unwrap(),
            json!({"version": 1, "normalization_version": 1, "project_id": PROJECT, "language": "en",
                   "requirements": [{"requirement_ref": GOLDEN_REQUIREMENT, "status": "Proposed",
                                     "statement": "The employee shall submit a leave request."}]})
        );
        let inner = &req.request;
        assert_eq!(inner.stage, StageId::S1);
        assert_eq!(inner.task_kind, "vocabulary_analysis");
        assert_eq!(VOCABULARY_TASK_KIND, "vocabulary_analysis");
        assert_eq!(
            (
                VOCABULARY_CONTEXT_VERSION,
                VOCABULARY_OUTPUT_VERSION,
                VOCABULARY_NORMALIZATION_VERSION
            ),
            (1, 1, 1)
        );
        assert_eq!(PILOT_VOCABULARY_LANGUAGE, "en");
        assert_eq!(inner.input_refs, ids);
        assert_eq!(inner.evidence_refs, vec![fragment]);
        assert_eq!(inner.prompt_template_hash.as_str(), PROMPT_HASH);
        assert_eq!(inner.schema_hash.as_str(), SCHEMA_HASH);
        assert_eq!(inner.context_hash.as_str(), GOLDEN_CONTEXT_HASH);
        assert_eq!(inner.id.as_str(), GOLDEN_REQUEST_ID);
    }

    #[test]
    fn vocabulary_request_targets_and_policy() {
        let (graph, ids) = setup(&[
            "The employee shall submit a leave request.",
            "The manager shall approve a leave request.",
        ]);
        let forward = request(&graph, &ids);
        let mut reversed = ids.clone();
        reversed.reverse();
        assert_eq!(request(&graph, &reversed), forward);
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(forward.request.input_refs, sorted);
        assert_eq!(
            forward
                .context
                .requirements
                .iter()
                .map(|r| r.requirement_ref.clone())
                .collect::<Vec<_>>(),
            sorted
        );
        let other =
            build_vocabulary_request(&graph, &ids, &policy(), provider("other-provider")).unwrap();
        assert_ne!(other.request.id, forward.request.id);
        assert_eq!(other.context, forward.context);
        let invalid = |g: &Graph, targets: &[Id]| {
            assert!(
                matches!(
                    build_vocabulary_request(g, targets, &policy(), provider("mock")),
                    Err(VocabularyError::InvalidInput { .. })
                ),
                "{targets:?}"
            );
        };
        invalid(&graph, &[ids[0].clone(), ids[0].clone()]);
        invalid(&graph, &[]);
        invalid(&graph, &[id("req:00000000000000ff")]);
        invalid(
            &graph,
            &[graph.node(&ids[0]).unwrap().evidence[0].as_id().clone()],
        );
        for status in [
            ElementStatus::Rejected,
            ElementStatus::Suspect,
            ElementStatus::Superseded,
            ElementStatus::Deprecated,
        ] {
            invalid(&with_status(&graph, &ids[..1], status), &ids[..1]);
        }
        assert!(build_vocabulary_request(
            &with_status(&graph, &ids, Accepted),
            &ids,
            &policy(),
            provider("mock")
        )
        .is_ok());
        let german = VocabularyPolicy {
            language: "de".to_owned(),
        };
        assert!(matches!(
            build_vocabulary_request(&graph, &ids, &german, provider("mock")),
            Err(VocabularyError::UnsupportedLanguage { .. })
        ));
        assert!(
            serde_json::from_value::<VocabularyPolicy>(json!({"language": "en", "extra": 1}))
                .is_err()
        );
    }

    #[test]
    fn vocabulary_request_tampering() {
        let (graph, ids) = setup(&["The employee shall submit a leave request."]);
        let req = request(&graph, &ids);
        let rebuild_id = |f: &dyn Fn(&mut VocabularyRequest)| {
            let mut t = req.clone();
            f(&mut t);
            t.request.id = t.request.recompute_id().unwrap();
            t
        };
        let cases = [
            rebuild_id(&|t| t.request.stage = StageId::S0),
            rebuild_id(&|t| t.request.task_kind = "ears_normalization".to_owned()),
            rebuild_id(&|t| t.context.language = "de".to_owned()),
            rebuild_id(&|t| t.context.normalization_version = 2),
            rebuild_id(&|t| t.request.context_hash = Hash::content_sha256(b"x")),
            rebuild_id(&|t| t.request.input_refs = vec![]),
            rebuild_id(&|t| t.request.evidence_refs = vec![id("evd:00000000000000ff")]),
            rebuild_id(&|t| t.request.prompt_template_hash = Hash::content_sha256(b"x")),
            rebuild_id(&|t| t.request.schema_hash = Hash::content_sha256(b"x")),
        ];
        for (i, bad) in cases.into_iter().enumerate() {
            // Evidence refs depend on the graph, so only evaluation can check them (case 6).
            assert_eq!(bad.validate().is_err(), i != 6, "case {i}");
            assert!(
                analyze_vocabulary(&graph, &bad, &policy(), None, &audit()).is_err(),
                "case {i}"
            );
        }
        // A changed statement or status makes the request stale.
        let accepted = with_status(&graph, &ids, Accepted);
        assert!(matches!(
            analyze_vocabulary(&accepted, &req, &policy(), None, &audit()),
            Err(VocabularyError::InvalidInput { .. })
        ));
    }

    // ------------------------------------------------------------------ inference validation

    #[test]
    fn vocabulary_absent_and_invalid_inference() {
        let (graph, ids) = setup(&["The employee shall submit a leave request."]);
        let req = request(&graph, &ids);
        let absent = analyze_vocabulary(&graph, &req, &policy(), None, &audit()).unwrap();
        assert_eq!(absent.issues, vec![VocabularyIssue::InferenceUnavailable]);
        assert!(
            absent.candidates.is_empty()
                && absent.cooccurrences.is_empty()
                && absent.lint_contexts.is_empty()
        );
        assert!(
            absent.conflicts.is_empty()
                && absent.findings.is_empty()
                && absent.proposals.is_empty()
        );
        let good = execution(
            &req.request,
            output(vec![mention(&graph, &ids[0], "employee")]),
        )
        .artifact;
        let mut wrong_request = good.clone();
        wrong_request.request_hash = Hash::content_sha256(b"other");
        let mut wrong_provider = good.clone();
        wrong_provider.provider = "other-provider".to_owned();
        let mut wrong_hash = good.clone();
        wrong_hash.validated_output_hash = Hash::content_sha256(b"other");
        for bad in [wrong_request, wrong_provider, wrong_hash] {
            assert!(matches!(
                analyze_with(&graph, &req, &bad, &audit()),
                Err(VocabularyError::InvalidInferenceArtifact { .. })
            ));
        }
        assert!(matches!(
            analyze(&graph, &req, vec![raw(json!("entity"), Value::Null)]),
            Err(VocabularyError::SchemaInvalid { .. })
        ));
    }

    #[test]
    fn vocabulary_mention_validation() {
        let s = "Zażółć employee shall submit a leave request.";
        let (graph, ids) = setup(&[s]);
        let req = request(&graph, &ids);
        let r = &ids[0];
        let at = |start: usize, end: usize| json!({"requirement_ref": r, "start": start, "end": end, "concept_kind": null, "definition": null});
        let unknown = json!({"requirement_ref": "req:00000000000000ff", "start": 0, "end": 3, "concept_kind": null, "definition": null});
        assert!(matches!(
            analyze(&graph, &req, vec![unknown]),
            Err(VocabularyError::UnknownRequirement { .. })
        ));
        assert!(
            matches!(
                analyze(&graph, &req, vec![at(0, 3)]),
                Err(VocabularyError::InvalidMention { .. })
            ),
            "cuts ż"
        );
        assert!(matches!(
            analyze(&graph, &req, vec![at(5, 5 + 999)]),
            Err(VocabularyError::InvalidMention { .. })
        ));
        let dash = s.find(' ').unwrap();
        assert!(
            matches!(
                analyze(&graph, &req, vec![at(dash, dash + 1)]),
                Err(VocabularyError::InvalidMention { .. })
            ),
            "whitespace only"
        );
        let e = s.find("employee").unwrap();
        assert!(matches!(
            analyze(&graph, &req, vec![at(e, e + 8), at(e, e + 8)]),
            Err(VocabularyError::DuplicateMention { .. })
        ));
        assert!(matches!(
            analyze(&graph, &req, vec![at(e, e + 8), at(e + 4, e + 12)]),
            Err(VocabularyError::OverlappingMention { .. })
        ));
        let ok = analyze(&graph, &req, vec![at(e, e + 4), at(e + 4, e + 8)]).unwrap();
        assert_eq!(ok.candidates.len(), 2, "adjacent ranges are allowed");
        let zazolc = s.find(' ').unwrap();
        let multibyte = analyze(&graph, &req, vec![at(0, zazolc)]).unwrap();
        assert_eq!(multibyte.candidates[0].surface_forms, ["Zażółć"]);
    }

    #[test]
    fn vocabulary_grounded_concept() {
        let s = "A contractor is a worker engaged under a non-employee contract and shall sign the agreement.";
        let (graph, ids) = setup(&[s]);
        let req = request(&graph, &ids);
        let definition = grounding(
            &graph,
            &ids[0],
            "a worker engaged under a non-employee contract",
        );
        let result = analyze(
            &graph,
            &req,
            vec![typed(
                &graph,
                &ids[0],
                "contractor",
                "object_type",
                definition.clone(),
            )],
        )
        .unwrap();
        let c = concepts(&result);
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0].1,
            &Concept {
                name: "contractor".to_owned(),
                definition: "a worker engaged under a non-employee contract".to_owned(),
                concept_kind: ObjectType
            }
        );
        assert_eq!(terms(&result).len(), 1);
        // Kind without definition and null kind: Term only, no fabricated definition.
        for m in [
            typed(&graph, &ids[0], "contractor", "object_type", Value::Null),
            mention(&graph, &ids[0], "contractor"),
        ] {
            let result = analyze(&graph, &req, vec![m]).unwrap();
            assert_eq!(terms(&result).len(), 1);
            assert!(concepts(&result).is_empty());
        }
        // Padded or empty definition ranges are rejected, never trimmed.
        let padded = grounding(&graph, &ids[0], " a worker");
        assert!(matches!(
            analyze(
                &graph,
                &req,
                vec![typed(&graph, &ids[0], "contractor", "role", padded)]
            ),
            Err(VocabularyError::InvalidDefinitionGrounding { .. })
        ));
        let foreign = json!({"requirement_ref": "req:00000000000000ff", "start": 0, "end": 3});
        assert!(analyze(
            &graph,
            &req,
            vec![typed(&graph, &ids[0], "contractor", "role", foreign)]
        )
        .is_err());
    }

    // ------------------------------------------------------------------ statistics

    #[test]
    fn vocabulary_statistics() {
        let (graph, ids) = setup(&[
            "The employee shall tell another employee about the leave request.",
            "The employee shall send the leave request to the manager.",
        ]);
        let req = request(&graph, &ids);
        let (a, b) = (&ids[0], &ids[1]);
        let mentions = vec![
            mention_nth(&graph, a, "employee", 0, Value::Null, Value::Null),
            mention_nth(&graph, a, "employee", 1, Value::Null, Value::Null),
            mention(&graph, a, "leave request"),
            mention(&graph, b, "employee"),
            mention(&graph, b, "leave request"),
            mention(&graph, b, "manager"),
        ];
        let result = analyze(&graph, &req, mentions.clone()).unwrap();
        assert_eq!(keys(&result), ["employee", "leave request", "manager"]);
        let stats: Vec<(u64, u64)> = result
            .candidates
            .iter()
            .map(|c| (c.frequency, c.requirement_count))
            .collect();
        assert_eq!(stats, [(3, 2), (2, 2), (1, 1)]);
        let pairs: Vec<(&str, &str, u64)> = result
            .cooccurrences
            .iter()
            .map(|c| {
                (
                    c.left_key.as_str(),
                    c.right_key.as_str(),
                    c.requirement_count,
                )
            })
            .collect();
        assert_eq!(
            pairs,
            [
                ("employee", "leave request", 2),
                ("employee", "manager", 1),
                ("leave request", "manager", 1)
            ]
        );
        let employee = &result.candidates[0];
        let order: Vec<(&Id, u64)> = employee
            .mentions
            .iter()
            .map(|m| (&m.requirement_ref, m.range.start))
            .collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
        // Output order does not matter; audit time changes no IDs or statistics.
        let mut reversed = mentions;
        reversed.reverse();
        let again = analyze(&graph, &req, reversed).unwrap();
        assert_eq!(again, result);
        let artifact = execution(&req.request, output(again.candidates.iter().flat_map(|c| c.mentions.iter()).map(|m| json!({"requirement_ref": m.requirement_ref, "start": m.range.start, "end": m.range.end, "concept_kind": null, "definition": null})).collect())).artifact;
        let later = analyze_with(
            &graph,
            &req,
            &artifact,
            &audit_at("2026-07-01T00:00:00.000000000Z"),
        )
        .unwrap();
        assert_eq!(later.candidates, result.candidates);
        let node_ids = |r: &VocabularyAnalysisResult| {
            r.proposals
                .iter()
                .map(|p| added(p).id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(node_ids(&later), node_ids(&result));
        assert_ne!(later.proposals[0].id, result.proposals[0].id);
    }

    // ------------------------------------------------------------------ proposals

    #[test]
    fn vocabulary_term_proposal() {
        let (graph, ids) = setup(&[
            "The Leave Request shall be stored.",
            "All leave requests shall be audited.",
            "The leave request shall be signed.",
        ]);
        let req = request(&graph, &ids);
        let mentions = vec![
            mention(&graph, &ids[0], "Leave Request"),
            mention(&graph, &ids[1], "leave requests"),
            mention(&graph, &ids[2], "leave request"),
        ];
        let result = analyze(&graph, &req, mentions).unwrap();
        let t = terms(&result);
        assert_eq!(t.len(), 1);
        let (node, term) = t[0];
        assert_eq!(node.id.as_str(), TERM_ID);
        assert_eq!(
            node.id.as_str(),
            sha_id(
                "term",
                &json!({"project_id": PROJECT, "language": "en", "normalized_key": "leave request", "node_type": "term"})
            )
        );
        assert_eq!(
            term,
            &Term {
                term: "leave request".to_owned(),
                language: "en".to_owned(),
                definition_ref: None,
                aliases: Some(vec![
                    "Leave Request".to_owned(),
                    "leave requests".to_owned()
                ]),
                status: None,
            }
        );
        assert_eq!(node.status, Proposed);
        assert_eq!(node.revision, 1);
        let mut evidence: Vec<EvidenceRef> = ids
            .iter()
            .map(|r| graph.node(r).unwrap().evidence[0].clone())
            .collect();
        evidence.sort();
        assert_eq!(evidence.len(), 3);
        assert_eq!(node.evidence, evidence);
        assert!(node.derivations.is_empty() && node.standards.is_empty() && node.tags.is_empty());
        assert_eq!(node.audit.created_by, id("agent:vocabulary"));
        let origin = &node.extensions[&VOCABULARY_ORIGIN_EXTENSION.parse().unwrap()];
        let mut expected_mentions: Vec<Value> = ids
            .iter()
            .zip(["Leave Request", "leave requests", "leave request"])
            .map(|(r, part)| {
                let start = statement(&graph, r).find(part).unwrap();
                json!({"requirement_ref": r, "start": start, "end": start + part.len()})
            })
            .collect();
        expected_mentions.sort_by_key(|m| m["requirement_ref"].as_str().unwrap().to_owned());
        assert_eq!(
            origin,
            &json!({"language": "en", "normalized_key": "leave request", "mentions": expected_mentions, "definition": null})
        );
        assert_eq!(
            VOCABULARY_ORIGIN_EXTENSION,
            "plumb_functional:vocabulary_origin"
        );
        let proposal = &result.proposals[0];
        assert_eq!(proposal.stage, StageId::S1);
        assert_eq!(proposal.materiality, ProposalMateriality::Semantic);
        assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanConfirm);
        assert_eq!(proposal.confidence, None);
        assert_eq!(proposal.evidence_refs, evidence);
        assert_eq!(proposal.derivation_refs, vec![derivation()]);
        assert_eq!(
            proposal.patch_set.base_semantic_hash,
            graph.semantic_hash().unwrap()
        );
        let applied = apply_patch(&graph, &proposal.patch_set).unwrap().graph;
        applied.validate().unwrap();
        // Re-analysis with the identical node pending emits no duplicate proposal.
        let req2 = request(&applied, &ids);
        let again = analyze(
            &applied,
            &req2,
            vec![
                mention(&applied, &ids[0], "Leave Request"),
                mention(&applied, &ids[1], "leave requests"),
                mention(&applied, &ids[2], "leave request"),
            ],
        )
        .unwrap();
        assert!(again.proposals.is_empty());
        // A different node at the deterministic ID is a conflict.
        let squatter = with_nodes(
            &graph,
            vec![concept_node(TERM_ID, Proposed, "squatter", Role)],
        );
        let req3 = request(&squatter, &ids);
        assert!(matches!(
            analyze(
                &squatter,
                &req3,
                vec![mention(&squatter, &ids[2], "leave request")]
            ),
            Err(VocabularyError::ExistingVocabularyCandidateConflict { .. })
        ));
        // A canonical surface is not repeated as an alias.
        let single = analyze(
            &graph,
            &req,
            vec![mention(&graph, &ids[2], "leave request")],
        )
        .unwrap();
        assert_eq!(terms(&single)[0].1.aliases, None);
    }

    #[test]
    fn vocabulary_concept_proposal_and_evidence() {
        let (graph, ids) = setup(&[
            "The leave request shall be stored.",
            "Each leave request shall be audited.",
            "A leave request is a formal absence application and shall be numbered.",
        ]);
        let req = request(&graph, &ids);
        let def = grounding(&graph, &ids[2], "a formal absence application");
        let mentions = vec![
            typed(&graph, &ids[0], "leave request", "object_type", def.clone()),
            typed(&graph, &ids[1], "leave request", "object_type", Value::Null),
        ];
        let result = analyze(&graph, &req, mentions).unwrap();
        let c = concepts(&result);
        assert_eq!(c.len(), 1);
        let (node, concept) = c[0];
        assert_eq!(node.id.as_str(), CONCEPT_ID);
        assert_eq!(concept.definition, "a formal absence application");
        assert_eq!(concept.concept_kind, ObjectType);
        let mut evidence: Vec<EvidenceRef> = ids
            .iter()
            .map(|r| graph.node(r).unwrap().evidence[0].clone())
            .collect();
        evidence.sort();
        assert_eq!(
            node.evidence, evidence,
            "mention and definition requirements"
        );
        let origin = &node.extensions[&VOCABULARY_ORIGIN_EXTENSION.parse().unwrap()];
        assert_eq!(origin["definition"], def);
        for field in ["concept_kind", "kind", "frequency", "confidence"] {
            assert!(origin.get(field).is_none(), "{field}");
        }
        assert_eq!(
            result.proposals.len(),
            2,
            "one proposal per node, no compound"
        );
        let (term_node, term) = terms(&result)[0];
        assert_eq!(
            term.definition_ref, None,
            "never points at a newly proposed Concept"
        );
        assert_eq!(term_node.id.as_str(), TERM_ID);
        let applied = apply_patch(
            &graph,
            &result
                .proposals
                .iter()
                .find(|p| added(p).id.as_str() == CONCEPT_ID)
                .unwrap()
                .patch_set,
        )
        .unwrap();
        applied.graph.validate().unwrap();
        let ids_sorted: Vec<&Id> = result.proposals.iter().map(|p| &added(p).id).collect();
        let mut s = ids_sorted.clone();
        s.sort();
        assert_eq!(ids_sorted, s);
    }

    #[test]
    fn vocabulary_consensus_and_conflicts() {
        let (graph, ids) = setup(&[
            "The employee shall file a request.",
            "Every employee shall sign the form.",
            "An employee is a person under contract and shall be registered.",
            "An employee is a staff member and shall be insured.",
        ]);
        let req = request(&graph, &ids);
        let d1 = grounding(&graph, &ids[2], "a person under contract");
        let d2 = grounding(&graph, &ids[3], "a staff member");
        // Conflicting kinds.
        let kinds = analyze(
            &graph,
            &req,
            vec![
                typed(&graph, &ids[0], "employee", "object_type", d1.clone()),
                typed(&graph, &ids[1], "employee", "role", d1.clone()),
            ],
        )
        .unwrap();
        assert!(
            matches!(&kinds.conflicts[..], [VocabularyConflict::ConceptKindConflict { normalized_key, concept_kinds }] if normalized_key == "employee" && concept_kinds == &vec![ObjectType, Role])
        );
        assert!(concepts(&kinds).is_empty());
        assert_eq!(terms(&kinds).len(), 1);
        assert!(kinds.findings.is_empty(), "Proposed requirements only");
        // Conflicting definitions.
        let defs = analyze(
            &graph,
            &req,
            vec![
                typed(&graph, &ids[0], "employee", "object_type", d1.clone()),
                typed(&graph, &ids[1], "employee", "object_type", d2.clone()),
            ],
        )
        .unwrap();
        assert!(
            matches!(&defs.conflicts[..], [VocabularyConflict::ConceptDefinitionConflict { definitions, .. }] if definitions.len() == 2)
        );
        assert!(concepts(&defs).is_empty());
        // Identical text from two ranges is no conflict; the first range is the origin.
        let (g2, ids2) = setup(&[
            "An employee is a person under contract and shall be registered.",
            "Each employee is a person under contract and shall be insured.",
        ]);
        let req2 = request(&g2, &ids2);
        let first = grounding(&g2, &ids2[0], "a person under contract");
        let second = grounding(&g2, &ids2[1], "a person under contract");
        let same = analyze(
            &g2,
            &req2,
            vec![
                typed(&g2, &ids2[0], "employee", "object_type", second.clone()),
                typed(&g2, &ids2[1], "employee", "object_type", first.clone()),
            ],
        )
        .unwrap();
        assert!(same.conflicts.is_empty());
        let c = concepts(&same);
        assert_eq!(c.len(), 1);
        let origin = &c[0].0.extensions[&VOCABULARY_ORIGIN_EXTENSION.parse().unwrap()];
        let expected = if first["requirement_ref"].as_str() < second["requirement_ref"].as_str() {
            first
        } else {
            second
        };
        assert_eq!(origin["definition"], expected);
    }

    #[test]
    fn vocabulary_accepted_reconciliation() {
        let base = [
            "The employee shall file a request.",
            "The staff members shall sign the form.",
        ];
        let run = |extra: Vec<Node>, mentions: &dyn Fn(&Graph, &[Id]) -> Vec<Value>| {
            let (graph, ids) = setup_with(&base, extra);
            let req = request(&graph, &ids);
            analyze(&graph, &req, mentions(&graph, &ids)).unwrap()
        };
        let employee = |g: &Graph, ids: &[Id]| vec![mention(g, &ids[0], "employee")];
        // Accepted canonical Term and alias: no duplicate Term.
        assert!(terms(&run(
            vec![term_node(
                "term:00000000000000a1",
                Accepted,
                "Employees",
                None
            )],
            &employee
        ))
        .is_empty());
        let alias = run(
            vec![term_node(
                "term:00000000000000a1",
                Accepted,
                "employee",
                Some(vec!["staff member"]),
            )],
            &|g, ids| vec![mention(g, &ids[1], "staff members")],
        );
        assert!(terms(&alias).is_empty());
        assert_eq!(keys(&alias), ["staff member"]);
        // A Proposed Term does not resolve anything.
        assert_eq!(
            terms(&run(
                vec![term_node(
                    "term:00000000000000a1",
                    Proposed,
                    "employee",
                    None
                )],
                &employee
            ))
            .len(),
            1
        );
        // Two Accepted Terms for one key.
        let ambiguous = run(
            vec![
                term_node("term:00000000000000a1", Accepted, "employee", None),
                term_node("term:00000000000000a2", Accepted, "Employee", None),
            ],
            &employee,
        );
        assert!(
            matches!(&ambiguous.conflicts[..], [VocabularyConflict::AcceptedTermKeyConflict { term_refs, .. }] if term_refs.len() == 2)
        );
        assert!(terms(&ambiguous).is_empty());
        // Accepted Concept same kind: no new Concept, no conflict; Term gets definition_ref.
        let concept = vec![concept_node(
            "concept:00000000000000c1",
            Accepted,
            "Employee",
            ObjectType,
        )];
        let same = run(concept.clone(), &|g, ids| {
            vec![typed(
                g,
                &ids[0],
                "employee",
                "object_type",
                grounding(g, &ids[1], "staff members"),
            )]
        });
        assert!(same.conflicts.is_empty());
        assert!(concepts(&same).is_empty());
        assert_eq!(
            terms(&same)[0].1.definition_ref,
            Some(id("concept:00000000000000c1"))
        );
        // Accepted Concept conflicting kind: reported, never overwritten.
        let conflicting = run(concept, &|g, ids| {
            vec![typed(g, &ids[0], "employee", "role", Value::Null)]
        });
        assert!(matches!(
            &conflicting.conflicts[..],
            [VocabularyConflict::AcceptedConceptKindConflict { concept_ref, accepted_kind: ObjectType, proposed_kinds, .. }]
                if concept_ref.as_str() == "concept:00000000000000c1" && proposed_kinds == &vec![Role]
        ));
        assert!(conflicting
            .proposals
            .iter()
            .all(|p| matches!(p.patch_set.patch, SemanticPatch::AddNode { .. })));
        assert!(concepts(&conflicting).is_empty());
        // Two Accepted Concepts for one key.
        let two = run(
            vec![
                concept_node("concept:00000000000000c1", Accepted, "employee", ObjectType),
                concept_node("concept:00000000000000c2", Accepted, "employees", Role),
            ],
            &|g, ids| {
                vec![typed(
                    g,
                    &ids[0],
                    "employee",
                    "object_type",
                    grounding(g, &ids[1], "staff members"),
                )]
            },
        );
        assert!(two.conflicts.iter().any(|c| matches!(c, VocabularyConflict::AcceptedConceptKeyConflict { concept_refs, .. } if concept_refs.len() == 2)));
        assert!(concepts(&two).is_empty());
        assert_eq!(terms(&two)[0].1.definition_ref, None);
    }

    // ------------------------------------------------------------------ lint bridge

    #[test]
    fn vocabulary_lint_bridge() {
        let s = "The employee shall submit a leave request.";
        let lint_for = |extra: Vec<Node>| {
            let (graph, ids) = setup_with(&[s], extra);
            let req = request(&graph, &ids);
            let result = analyze(
                &graph,
                &req,
                vec![
                    mention(&graph, &ids[0], "employee"),
                    mention(&graph, &ids[0], "leave request"),
                ],
            )
            .unwrap();
            assert_eq!(result.lint_contexts.len(), 1);
            let ctx = result.lint_contexts[0].context.clone();
            for m in &ctx.mentions {
                let slice = &s[m.range.start as usize..m.range.end as usize];
                assert_eq!(
                    normalize_vocabulary_term(slice).as_deref(),
                    Some(m.normalized_key.as_str())
                );
            }
            let input = LintInput {
                requirement_ref: ids[0].clone(),
                statement: s.to_owned(),
                evidence_refs: vec![],
                source_anchor: None,
                term_context: Some(ctx),
            };
            lint_requirement(&input, &LintPolicy::default())
                .unwrap()
                .diagnostics
                .into_iter()
                .filter(|d| d.rule_id == LintRuleId::UndefinedTerm)
                .map(|d| d.statement_range)
                .collect::<Vec<LintTextRange>>()
        };
        let e = s.find("employee").unwrap() as u64;
        let l = s.find("leave request").unwrap() as u64;
        assert_eq!(
            lint_for(vec![]),
            [
                LintTextRange {
                    start: e,
                    end: e + 8
                },
                LintTextRange {
                    start: l,
                    end: l + 13
                }
            ]
        );
        assert_eq!(
            lint_for(vec![term_node(
                "term:00000000000000a1",
                Accepted,
                "employee",
                None
            )]),
            [LintTextRange {
                start: l,
                end: l + 13
            }]
        );
        assert_eq!(
            lint_for(vec![concept_node(
                "concept:00000000000000c1",
                Accepted,
                "Leave Requests",
                ObjectType
            )]),
            [LintTextRange {
                start: e,
                end: e + 8
            }]
        );
        assert_eq!(
            lint_for(vec![term_node(
                "term:00000000000000a1",
                Proposed,
                "employee",
                None
            )])
            .len(),
            2,
            "Proposed is not defined"
        );
    }

    // ------------------------------------------------------------------ findings

    #[test]
    fn vocabulary_conflict_findings() {
        let (graph, ids) = setup(&[
            "The employee shall file a request.",
            "Every employee shall sign the form.",
        ]);
        let accepted = with_status(&graph, &ids, Accepted);
        let req = request(&accepted, &ids);
        let result = analyze(
            &accepted,
            &req,
            vec![
                typed(&accepted, &ids[0], "employee", "object_type", Value::Null),
                typed(&accepted, &ids[1], "employee", "role", Value::Null),
            ],
        )
        .unwrap();
        assert_eq!(result.findings.len(), 1);
        let f = &result.findings[0];
        let profile = plumb_validation::load_builtin_software_profile().unwrap();
        let rule = profile
            .rules
            .iter()
            .find(|r| r.id == "PLUMB.F1.REQ.TERMS_RESOLVED")
            .unwrap();
        assert_eq!(f.payload.code, rule.id);
        assert_eq!(f.payload.family, "F1");
        assert_eq!(f.payload.status, rule.finding_status_on_fail);
        assert_eq!(
            f.semantic_condition_key,
            "vocabulary_concept_kind_conflict:employee"
        );
        assert_eq!(
            f.payload.message,
            "Vocabulary term \"employee\" has conflicting concept-kind proposals."
        );
        assert_eq!(
            f.payload.suggested_resolution.as_deref(),
            Some("Review the vocabulary evidence and establish one governed vocabulary interpretation before treating the term as resolved.")
        );
        let mut targets = ids.clone();
        targets.sort();
        assert_eq!(f.payload.affected_refs, targets);
        // Independent finding identity: rule 0x00 targets 0x00 condition.
        let mut bytes = rule.id.as_bytes().to_vec();
        bytes.push(0);
        for t in &targets {
            bytes.extend_from_slice(t.as_str().as_bytes());
            bytes.push(0);
        }
        bytes.extend_from_slice(b"vocabulary_concept_kind_conflict:employee");
        assert_eq!(f.key, Hash::content_sha256(&bytes));
        assert_eq!(f.id.as_str(), format!("fnd:{}", &f.key.as_str()[7..23]));
        // Every family's condition key and message.
        let families = [
            (
                vec![
                    typed(&accepted, &ids[0], "employee", "object_type", Value::Null),
                    typed(&accepted, &ids[1], "employee", "object_type", Value::Null),
                ],
                vec![concept_node(
                    "concept:00000000000000c1",
                    Accepted,
                    "employee",
                    Role,
                )],
                "vocabulary_accepted_concept_kind_conflict:employee",
                "Vocabulary term \"employee\" conflicts with the kind of an accepted Concept.",
            ),
            (
                vec![mention(&accepted, &ids[0], "employee")],
                vec![
                    term_node("term:00000000000000a1", Accepted, "employee", None),
                    term_node("term:00000000000000a2", Accepted, "employees", None),
                ],
                "vocabulary_accepted_term_key_conflict:employee",
                "Vocabulary key \"employee\" resolves to more than one accepted Term.",
            ),
            (
                vec![mention(&accepted, &ids[0], "employee")],
                vec![
                    concept_node("concept:00000000000000c1", Accepted, "employee", Role),
                    concept_node("concept:00000000000000c2", Accepted, "Employee", Role),
                ],
                "vocabulary_accepted_concept_key_conflict:employee",
                "Vocabulary key \"employee\" resolves to more than one accepted Concept.",
            ),
        ];
        for (mentions, extra, condition, message) in families {
            let g = with_nodes(&accepted, extra);
            let r = request(&g, &ids);
            let result = analyze(&g, &r, mentions).unwrap();
            let f = result
                .findings
                .iter()
                .find(|f| f.semantic_condition_key == condition)
                .unwrap_or_else(|| panic!("{condition}"));
            assert_eq!(f.payload.message, message);
        }
        let (g3, ids3) = setup(&[
            "An employee is a person and shall work.",
            "An employee is a worker and shall rest.",
        ]);
        let g3 = with_status(&g3, &ids3, Accepted);
        let r3 = request(&g3, &ids3);
        let result = analyze(
            &g3,
            &r3,
            vec![
                typed(
                    &g3,
                    &ids3[0],
                    "employee",
                    "object_type",
                    grounding(&g3, &ids3[0], "a person"),
                ),
                typed(
                    &g3,
                    &ids3[1],
                    "employee",
                    "object_type",
                    grounding(&g3, &ids3[1], "a worker"),
                ),
            ],
        )
        .unwrap();
        assert_eq!(
            result.findings[0].semantic_condition_key,
            "vocabulary_concept_definition_conflict:employee"
        );
        assert_eq!(
            result.findings[0].payload.message,
            "Vocabulary term \"employee\" has conflicting grounded Concept definitions."
        );
        // A graph with another profile cannot produce F1 material.
        let other = Graph::new(
            id(PROJECT),
            id("profile:other"),
            accepted.nodes().values().cloned().collect(),
            Vec::new(),
        )
        .unwrap();
        let ro = request(&other, &ids);
        assert!(matches!(
            analyze(
                &other,
                &ro,
                vec![
                    typed(&other, &ids[0], "employee", "object_type", Value::Null),
                    typed(&other, &ids[1], "employee", "role", Value::Null)
                ]
            ),
            Err(VocabularyError::InvalidInput { .. })
        ));
    }

    // ------------------------------------------------------------------ guard

    #[test]
    fn vocabulary_production_source_guard() {
        for (name, source) in [
            ("vocabulary.rs", include_str!("../src/vocabulary.rs")),
            (
                "vocabulary_exceptions.rs",
                include_str!("../src/vocabulary_exceptions.rs"),
            ),
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
                "apply_patch",
                "persist_inference_bundle",
                "materialize_derivation_record",
                "commit(",
                "unsafe",
                "HR-0",
                "leave request",
                "employee",
                "manager",
                "Europe/Warsaw",
                "fixtures/",
                "I_TERM",
                "NodePayload::Question",
                "NodePayload::Entity",
                "NodePayload::Actor",
                "glossary",
            ] {
                assert!(!source.contains(token), "{name} contains {token}");
            }
        }
        assert!(!include_str!("../src/lib.rs").contains("Proposal"));
    }

    // ------------------------------------------------------------------ HR integration

    /// Mock vocabulary spans over the real current HR statements, null kinds and definitions
    /// (the HR fixture carries no v3 ConceptKind oracle).
    fn hr_mentions(graph: &Graph, ids: &[Id]) -> Vec<Value> {
        let phrases = [
            "annual-leave balance",
            "leave balance",
            "leave requests",
            "leave request",
            "leave type",
            "start date",
            "end date",
            "approval records",
            "approval record",
            "audit record",
            "time zone",
            "employees",
            "employee",
            "managers",
            "manager",
        ];
        let mut out = Vec::new();
        for req in ids {
            // ASCII lowercase keeps byte offsets, so matching is case-insensitive.
            let s = statement(graph, req).to_ascii_lowercase();
            let mut taken: Vec<(usize, usize)> = Vec::new();
            for phrase in phrases {
                for (start, _) in s.match_indices(phrase) {
                    let end = start + phrase.len();
                    let word_bounded = (start == 0
                        || !s.as_bytes()[start - 1].is_ascii_alphanumeric())
                        && (end == s.len() || !s.as_bytes()[end].is_ascii_alphanumeric());
                    if word_bounded && taken.iter().all(|(a, b)| end <= *a || start >= *b) {
                        taken.push((start, end));
                        out.push(json!({"requirement_ref": req, "start": start, "end": end, "concept_kind": null, "definition": null}));
                    }
                }
            }
        }
        out
    }

    #[test]
    fn vocabulary_hr_integration() {
        let run = || {
            let audit = ImportAudit {
                created_by: id("actor:importer"),
                created_at: ts(AT),
            };
            let imported = import_markdown("requirements.md", HR_MD, &audit).unwrap();
            let (graph, ids) = requirements_graph(imported, Vec::new());
            assert_eq!(ids.len(), 32);
            let req = request(&graph, &ids);
            let mentions = hr_mentions(&graph, &ids);
            let mut provider_fixture = MockProvider::new();
            provider_fixture
                .register(
                    req.request.id.clone(),
                    execution(&req.request, output(mentions)),
                )
                .unwrap();
            let executed = provider_fixture.execute(&req.request).unwrap();
            let result = analyze_with(&graph, &req, &executed.artifact, &audit_at(AT)).unwrap();
            (graph, result)
        };
        let (graph, result) = run();
        let observed: BTreeSet<&str> = keys(&result).into_iter().collect();
        for key in [
            "employee",
            "leave request",
            "leave type",
            "start date",
            "end date",
            "manager",
            "leave balance",
            "annual leave balance",
            "approval record",
            "audit record",
            "time zone",
        ] {
            assert!(observed.contains(key), "{key}: {observed:?}");
        }
        let leave = result
            .candidates
            .iter()
            .find(|c| c.normalized_key == "leave request")
            .unwrap();
        assert!(leave.surface_forms.contains(&"leave request".to_owned()));
        assert!(leave.surface_forms.iter().any(|s| s == "leave requests") || leave.frequency > 1);
        let approval = result
            .candidates
            .iter()
            .find(|c| c.normalized_key == "approval record")
            .unwrap();
        assert!(approval
            .surface_forms
            .iter()
            .all(|s| normalize_vocabulary_term(s).as_deref() == Some("approval record")));
        for ctx in &result.lint_contexts {
            let s = statement(&graph, &ctx.requirement_ref);
            for m in &ctx.context.mentions {
                assert_eq!(
                    normalize_vocabulary_term(&s[m.range.start as usize..m.range.end as usize])
                        .as_deref(),
                    Some(m.normalized_key.as_str())
                );
            }
        }
        assert!(
            concepts(&result).is_empty(),
            "no ConceptKind oracle, no typing claimed"
        );
        assert_eq!(terms(&result).len(), result.candidates.len());
        for p in &result.proposals {
            assert_eq!(added(p).status, Proposed);
            assert_eq!(
                (p.materiality, p.acceptance_policy),
                (
                    ProposalMateriality::Semantic,
                    AcceptancePolicy::HumanConfirm
                )
            );
        }
        let (_, replay) = run();
        assert_eq!(replay, result);
        assert_eq!(
            serde_json::to_vec(&replay).unwrap(),
            serde_json::to_vec(&result).unwrap()
        );
    }
}
