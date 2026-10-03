//! S0.3 contract tests for the six I0 evaluators, through the real registration and
//! evaluate_gate path.
//!
//! Every test lives in `i0_contract` so that `cargo test -p plumb-validation i0` selects them.

mod i0_contract {
    use std::collections::BTreeMap;

    use plumb_artifacts::{Artifact, ArtifactKind};
    use plumb_core::{GateId, Hash, Id, Timestamp};
    use plumb_import::*;
    use plumb_psg::{
        evidence_fragment_id, source_artifact_id, EvidenceLocator, ExtensionKey, Graph, Node,
        NodePayload,
    };
    use plumb_validation::rules::register_i0_evaluators;
    use plumb_validation::*;
    use serde_json::{json, Value};

    const T1: &str = "2026-10-03T08:00:00.000000000Z";
    const T2: &str = "2031-01-01T00:00:00.000000000Z";
    const PROJECT: &str = "project:leave-management";
    const PROFILE: &str = "profile:plumb-software-2026.1";

    const CONTENT_ADDRESSED: &str = "PLUMB.I0.SOURCE.CONTENT_ADDRESSED";
    const LOCATABLE: &str = "PLUMB.I0.EVIDENCE.LOCATABLE";
    const HASH_MATCH: &str = "PLUMB.I0.EVIDENCE.HASH_MATCH";
    const PARSE_STATUS: &str = "PLUMB.I0.SOURCE.PARSE_STATUS";
    const AGENT_IDENTIFIED: &str = "PPMN.I0.PROVENANCE.AGENT_IDENTIFIED";
    const BASELINE_HASHABLE: &str = "PLUMB.I0.BASELINE.HASHABLE";
    const I0: [&str; 6] = [
        BASELINE_HASHABLE,
        HASH_MATCH,
        LOCATABLE,
        CONTENT_ADDRESSED,
        PARSE_STATUS,
        AGENT_IDENTIFIED,
    ];

    const HR_MD: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.md");
    const HR_DOCX: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.docx");

    use RuleResultState::{Fail, NotApplicable, Pass};

    // ------------------------------------------------------------------ fixtures

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn audit() -> ImportAudit {
        ImportAudit {
            created_by: id("actor:importer"),
            created_at: T1.parse().unwrap(),
        }
    }

    fn metadata() -> ValidationRegistry {
        ValidationRegistry::new(load_builtin_software_profile().unwrap()).unwrap()
    }

    fn evaluators() -> EvaluatorRegistry {
        let mut registry = EvaluatorRegistry::new(metadata());
        register_i0_evaluators(&mut registry).unwrap();
        registry
    }

    /// Nodes and acquired artifacts of an evaluation, before the manifest is added.
    #[derive(Clone)]
    struct Corpus {
        nodes: Vec<Node>,
        artifacts: Vec<Artifact>,
    }

    impl Corpus {
        fn empty() -> Corpus {
            Corpus {
                nodes: Vec::new(),
                artifacts: Vec::new(),
            }
        }

        fn of(imports: Vec<ImportedSource>) -> Corpus {
            let mut corpus = Corpus::empty();
            for imported in imports {
                corpus.nodes.push(imported.source);
                corpus.nodes.extend(imported.fragments);
                corpus.artifacts.push(imported.original_artifact);
                corpus.artifacts.push(imported.extracted_artifact);
            }
            corpus
        }

        fn graph(&self) -> Graph {
            Graph::new(id(PROJECT), id(PROFILE), self.nodes.clone(), vec![])
                .unwrap_or_else(|v| panic!("{v:#?}"))
        }

        /// The corpus artifacts plus the exact manifest artifact of its graph.
        fn with_manifest(&self) -> Vec<Artifact> {
            let manifest = build_evidence_manifest(&self.graph()).unwrap();
            let mut artifacts = self.artifacts.clone();
            artifacts.push(evidence_manifest_artifact(&manifest, T1.parse().unwrap()).unwrap());
            artifacts
        }

        fn node_mut(&mut self, node_id: &Id) -> &mut Node {
            self.nodes.iter_mut().find(|n| &n.id == node_id).unwrap()
        }
    }

    fn small_corpus() -> Corpus {
        Corpus::of(vec![
            import_markdown("a.md", b"# Title\n\n- item\n\nText line\n", &audit()).unwrap(),
            import_plain_text("b.txt", b"Plain paragraph.\n", &audit()).unwrap(),
        ])
    }

    fn hr_corpus() -> Corpus {
        Corpus::of(vec![
            import_markdown("requirements.md", HR_MD, &audit()).unwrap(),
            import_docx("requirements.docx", HR_DOCX, &audit()).unwrap(),
        ])
    }

    fn evaluate_with(graph: &Graph, baseline: Hash, artifacts: &[Artifact]) -> GateReport {
        let registry = evaluators();
        let inputs = artifacts
            .iter()
            .map(ValidationArtifactInput::from)
            .collect();
        let ctx = ValidationContext::new(
            graph,
            registry.metadata(),
            ValidationPolicy::default(),
            baseline,
            inputs,
            vec![],
        )
        .unwrap();
        registry.evaluate_gate(GateId::I0, graph, &ctx).unwrap()
    }

    /// Evaluates the corpus with the given artifacts and the graph's own evidence hash.
    fn evaluate(corpus: &Corpus, artifacts: &[Artifact]) -> GateReport {
        let graph = corpus.graph();
        evaluate_with(&graph, graph.evidence_hash().unwrap(), artifacts)
    }

    fn evaluate_complete(corpus: &Corpus) -> GateReport {
        evaluate(corpus, &corpus.with_manifest())
    }

    fn result<'a>(report: &'a GateReport, rule_id: &str) -> &'a RuleResult {
        report.rules.iter().find(|r| r.rule_id == rule_id).unwrap()
    }

    fn state(report: &GateReport, rule_id: &str) -> RuleResultState {
        result(report, rule_id).state
    }

    fn targets(report: &GateReport, rule_id: &str) -> Vec<String> {
        result(report, rule_id)
            .targets
            .iter()
            .map(|t| t.to_string())
            .collect()
    }

    fn source_ids(corpus: &Corpus) -> Vec<String> {
        let mut ids: Vec<String> = corpus
            .nodes
            .iter()
            .filter(|n| matches!(n.payload, NodePayload::SourceArtifact(_)))
            .map(|n| n.id.to_string())
            .collect();
        ids.sort();
        ids
    }

    fn fragment_ids(corpus: &Corpus) -> Vec<String> {
        let mut ids: Vec<String> = corpus
            .nodes
            .iter()
            .filter(|n| matches!(n.payload, NodePayload::EvidenceFragment(_)))
            .map(|n| n.id.to_string())
            .collect();
        ids.sort();
        ids
    }

    fn first_source(corpus: &Corpus) -> Id {
        corpus
            .nodes
            .iter()
            .find(|n| matches!(n.payload, NodePayload::SourceArtifact(_)))
            .unwrap()
            .id
            .clone()
    }

    fn node_json(node_id: &str, status: &str, tag: &str, data: Value, created_by: &str) -> Node {
        serde_json::from_value(json!({
            "id": node_id, "revision": 1, "status": status,
            "payload": {"type": tag, "data": data},
            "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
            "audit": {"created_by": created_by, "created_at": T1, "updated_by": null, "updated_at": null}
        }))
        .unwrap()
    }

    /// A non-built-in source (kind `pdf`) with no importer extensions, its original artifact,
    /// and one TextRange fragment.
    fn foreign_source() -> (Node, Node, Artifact) {
        let bytes = b"%PDF-1.7 leave policy".to_vec();
        let hash = Hash::content_sha256(&bytes);
        let source_id = source_artifact_id(&hash).unwrap();
        let source = node_json(
            source_id.as_str(),
            "Accepted",
            "SourceArtifact",
            json!({"source_kind": "pdf", "display_name": "policy.pdf",
                   "content_hash": hash, "media_type": "application/pdf"}),
            "actor:importer",
        );
        let locator: EvidenceLocator =
            serde_json::from_value(json!({"kind": "TextRange", "data": {"start": 0, "end": 4}}))
                .unwrap();
        let fragment = node_json(
            evidence_fragment_id(&source_id, &locator).unwrap().as_str(),
            "Accepted",
            "EvidenceFragment",
            json!({"source_ref": source_id, "locator": locator,
                   "content_hash": Hash::content_sha256(b"%PDF"), "extracted_text": "%PDF"}),
            "actor:importer",
        );
        let artifact = Artifact {
            hash,
            kind: ArtifactKind::SourceOriginal,
            media_type: "application/pdf".to_owned(),
            bytes,
            created_at: T1.parse().unwrap(),
        };
        (source, fragment, artifact)
    }

    fn without(artifacts: &[Artifact], kind: ArtifactKind) -> Vec<Artifact> {
        artifacts
            .iter()
            .filter(|a| a.kind != kind)
            .cloned()
            .collect()
    }

    fn extension_key(key: &str) -> ExtensionKey {
        key.parse().unwrap()
    }

    // ------------------------------------------------------------------ registration

    #[test]
    fn exactly_the_six_i0_rules_are_registered() {
        let registry = evaluators();
        assert!(registry.missing_for_gate(GateId::I0).is_empty());
        for rule_id in I0 {
            assert!(registry.has_evaluator(rule_id), "{rule_id}");
        }
        let bound: usize = registry
            .metadata()
            .required_now_rule_ids()
            .iter()
            .filter(|id| registry.has_evaluator(id))
            .count();
        assert_eq!(bound, 6);
        assert_eq!(registry.missing_for_gate(GateId::F1).len(), 11);
        let mut again = evaluators();
        assert!(matches!(
            register_i0_evaluators(&mut again),
            Err(EvaluationError::DuplicateEvaluator(_))
        ));
        // F1 being unbound does not stop I0.
        let report = evaluate_complete(&small_corpus());
        assert_eq!(report.result, GateResult::Pass);
    }

    // ------------------------------------------------------------------ HR source corpus

    #[test]
    fn hr_source_corpus_passes_i0() {
        let corpus = hr_corpus();
        assert_eq!(corpus.artifacts.len(), 4);
        let artifacts = corpus.with_manifest();
        assert_eq!(artifacts.len(), 5);
        let report = evaluate(&corpus, &artifacts);
        assert_eq!(state(&report, CONTENT_ADDRESSED), Pass);
        assert_eq!(state(&report, LOCATABLE), Pass);
        assert_eq!(state(&report, HASH_MATCH), Pass);
        assert_eq!(state(&report, PARSE_STATUS), Pass);
        assert_eq!(state(&report, AGENT_IDENTIFIED), NotApplicable);
        assert_eq!(
            result(&report, AGENT_IDENTIFIED).applicability,
            Applicability::NotApplicable {
                reason:
                    "No derivation-producing semantic action is present in the evaluated baseline."
                        .to_owned()
            }
        );
        assert_eq!(state(&report, BASELINE_HASHABLE), Pass);
        assert_eq!(report.result, GateResult::Pass);
        assert_eq!(targets(&report, CONTENT_ADDRESSED), source_ids(&corpus));
        assert_eq!(targets(&report, LOCATABLE), fragment_ids(&corpus));
        assert_eq!(targets(&report, HASH_MATCH), fragment_ids(&corpus));
        assert_eq!(targets(&report, PARSE_STATUS), source_ids(&corpus));
        assert!(targets(&report, BASELINE_HASHABLE).is_empty());
        assert_eq!(report.evidence_artifact_refs.len(), 5);
    }

    // ------------------------------------------------------------------ CONTENT_ADDRESSED

    #[test]
    fn content_addressed_requires_the_original_artifact() {
        let corpus = small_corpus();
        let complete = corpus.with_manifest();
        let all = source_ids(&corpus);
        assert_eq!(
            state(&evaluate(&corpus, &complete), CONTENT_ADDRESSED),
            Pass
        );

        let missing = without(&complete, ArtifactKind::SourceOriginal);
        let report = evaluate(&corpus, &missing);
        assert_eq!(state(&report, CONTENT_ADDRESSED), Fail);
        assert_eq!(targets(&report, CONTENT_ADDRESSED), all);
        let finding = &report
            .findings
            .iter()
            .find(|f| f.payload.code == CONTENT_ADDRESSED)
            .unwrap();
        assert_eq!(finding.semantic_condition_key, "source-content-addressed");
        assert_eq!(
            finding.payload.message,
            "One or more SourceArtifact nodes are not backed by the required content-addressed source artifact."
        );

        // Wrong kind and wrong media type.
        for change in ["kind", "media"] {
            let mut artifacts = complete.clone();
            for artifact in artifacts
                .iter_mut()
                .filter(|a| a.kind == ArtifactKind::SourceOriginal)
            {
                if change == "kind" {
                    artifact.kind = ArtifactKind::SourceExtracted;
                } else {
                    artifact.media_type = "application/octet-stream".to_owned();
                }
            }
            let report = evaluate(&corpus, &artifacts);
            assert_eq!(state(&report, CONTENT_ADDRESSED), Fail, "{change}");
            assert_eq!(targets(&report, CONTENT_ADDRESSED), all, "{change}");
        }

        // A built-in source without (or with wrong) artifact links.
        let source = first_source(&corpus);
        let mut unlinked = corpus.clone();
        unlinked
            .node_mut(&source)
            .extensions
            .remove(&extension_key(SOURCE_ARTIFACTS_EXTENSION));
        let report = evaluate(&unlinked, &corpus.artifacts);
        assert_eq!(state(&report, CONTENT_ADDRESSED), Fail);
        assert_eq!(targets(&report, CONTENT_ADDRESSED), [source.to_string()]);
    }

    #[test]
    fn content_addressed_checks_non_built_in_sources_too() {
        let (source, _, artifact) = foreign_source();
        let mut corpus = small_corpus();
        corpus.nodes.push(source.clone());
        let mut expected = source_ids(&corpus);
        expected.sort();
        let mut artifacts = corpus.artifacts.clone();
        artifacts.push(artifact);
        let report = evaluate(&corpus, &artifacts);
        assert_eq!(state(&report, CONTENT_ADDRESSED), Pass);
        assert_eq!(targets(&report, CONTENT_ADDRESSED), expected);
        let report = evaluate(&corpus, &corpus.artifacts);
        assert_eq!(state(&report, CONTENT_ADDRESSED), Fail);
        assert_eq!(targets(&report, CONTENT_ADDRESSED), [source.id.to_string()]);
    }

    // ------------------------------------------------------------------ LOCATABLE / HASH_MATCH

    #[test]
    fn locatable_and_hash_match_resolve_the_extracted_text() {
        let corpus = small_corpus();
        let complete = corpus.with_manifest();
        let report = evaluate(&corpus, &complete);
        assert_eq!(state(&report, LOCATABLE), Pass);
        assert_eq!(state(&report, HASH_MATCH), Pass);

        // Missing or wrong-kind extracted artifacts make every fragment unlocatable.
        let missing = without(&complete, ArtifactKind::SourceExtracted);
        let report = evaluate(&corpus, &missing);
        assert_eq!(state(&report, LOCATABLE), Fail);
        assert_eq!(targets(&report, LOCATABLE), fragment_ids(&corpus));
        assert_eq!(state(&report, HASH_MATCH), Fail);
        let finding = report
            .findings
            .iter()
            .find(|f| f.payload.code == LOCATABLE)
            .unwrap();
        assert_eq!(finding.semantic_condition_key, "evidence-locatable");
        let mut wrong_kind = complete.clone();
        for artifact in wrong_kind
            .iter_mut()
            .filter(|a| a.kind == ArtifactKind::SourceExtracted)
        {
            artifact.kind = ArtifactKind::SourceOriginal;
        }
        let report = evaluate(&corpus, &wrong_kind);
        assert_eq!(state(&report, LOCATABLE), Fail);

        // A range outside the extracted text (fragment re-identified for graph validity).
        let mut bad_range = corpus.clone();
        let fragment = bad_range
            .nodes
            .iter_mut()
            .find(|n| matches!(n.payload, NodePayload::EvidenceFragment(_)))
            .unwrap();
        if let NodePayload::EvidenceFragment(f) = &mut fragment.payload {
            f.locator = EvidenceLocator::TextRange {
                start: 0,
                end: 100_000,
            };
            fragment.id = evidence_fragment_id(&f.source_ref, &f.locator).unwrap();
        }
        let bad_id = fragment.id.to_string();
        let report = evaluate(&bad_range, &bad_range.with_manifest());
        assert_eq!(state(&report, LOCATABLE), Fail);
        assert_eq!(targets(&report, LOCATABLE), std::slice::from_ref(&bad_id));
        assert_eq!(state(&report, HASH_MATCH), Fail);
        assert_eq!(targets(&report, HASH_MATCH), [bad_id]);
    }

    #[test]
    fn hash_match_detects_a_wrong_fragment_hash() {
        let mut corpus = small_corpus();
        let fragment = corpus
            .nodes
            .iter_mut()
            .find(|n| matches!(n.payload, NodePayload::EvidenceFragment(_)))
            .unwrap();
        if let NodePayload::EvidenceFragment(f) = &mut fragment.payload {
            f.content_hash = Hash::content_sha256(b"something else");
        }
        let wrong = fragment.id.to_string();
        let report = evaluate(&corpus, &corpus.with_manifest());
        assert_eq!(state(&report, LOCATABLE), Pass);
        assert_eq!(state(&report, HASH_MATCH), Fail);
        assert_eq!(targets(&report, HASH_MATCH), [wrong]);
        let finding = report
            .findings
            .iter()
            .find(|f| f.payload.code == HASH_MATCH)
            .unwrap();
        assert_eq!(finding.semantic_condition_key, "evidence-hash-match");
        assert_eq!(
            finding.payload.message,
            "One or more EvidenceFragment content hashes do not match the bytes resolved from the extracted source."
        );
    }

    #[test]
    fn non_built_in_fragments_are_ignored_by_locator_rules() {
        let (source, fragment, artifact) = foreign_source();
        let mut corpus = small_corpus();
        corpus.nodes.push(source);
        corpus.nodes.push(fragment.clone());
        let mut artifacts = corpus.artifacts.clone();
        artifacts.push(artifact);
        let report = evaluate(&corpus, &artifacts);
        assert_eq!(state(&report, LOCATABLE), Pass);
        assert_eq!(state(&report, HASH_MATCH), Pass);
        assert_eq!(state(&report, PARSE_STATUS), Pass);
        assert!(!targets(&report, LOCATABLE).contains(&fragment.id.to_string()));
        assert!(!targets(&report, HASH_MATCH).contains(&fragment.id.to_string()));
        // The foreign evidence lacks the pilot links, so no exact manifest exists for it.
        assert!(matches!(
            build_evidence_manifest(&corpus.graph()),
            Err(ManifestError::InvalidGraphEvidence { .. })
        ));
        assert_eq!(state(&report, BASELINE_HASHABLE), Fail);
    }

    // ------------------------------------------------------------------ PARSE_STATUS

    #[test]
    fn parse_status_requires_valid_metadata_on_built_in_sources() {
        let corpus = small_corpus();
        assert_eq!(state(&evaluate_complete(&corpus), PARSE_STATUS), Pass);
        let source = first_source(&corpus);
        for value in [
            None,
            Some(json!({"status": "complete"})),
            Some(json!({"status": "failed", "warnings": []})),
        ] {
            let mut broken = corpus.clone();
            let key = extension_key(PARSE_EXTENSION);
            match value {
                None => {
                    broken.node_mut(&source).extensions.remove(&key);
                }
                Some(v) => {
                    broken.node_mut(&source).extensions.insert(key, v);
                }
            }
            let report = evaluate(&broken, &broken.with_manifest());
            assert_eq!(state(&report, PARSE_STATUS), Fail);
            assert_eq!(targets(&report, PARSE_STATUS), [source.to_string()]);
            let finding = report
                .findings
                .iter()
                .find(|f| f.payload.code == PARSE_STATUS)
                .unwrap();
            assert_eq!(finding.semantic_condition_key, "source-parse-status");
        }
    }

    // ------------------------------------------------------------------ provenance

    fn with_derivation(corpus: &Corpus, created_by: &str, extra: Vec<Node>) -> Corpus {
        let mut corpus = corpus.clone();
        corpus.nodes.push(node_json(
            "drv:segment-1",
            "Accepted",
            "DerivationRecord",
            json!({"id": "drv:segment-1", "kind": "parser", "stage": "S0.import",
                   "input_refs": [], "output_refs": [], "created_at": T1}),
            created_by,
        ));
        corpus.nodes.extend(extra);
        corpus
    }

    #[test]
    fn provenance_requires_a_known_agent_creator() {
        let base = small_corpus();
        let agent = node_json(
            "agent:segmenter",
            "Accepted",
            "Agent",
            json!({"agent_kind": "software_service"}),
            "actor:importer",
        );
        let ok = with_derivation(&base, "agent:segmenter", vec![agent.clone()]);
        let report = evaluate_complete(&ok);
        assert_eq!(state(&report, AGENT_IDENTIFIED), Pass);
        assert_eq!(targets(&report, AGENT_IDENTIFIED), ["drv:segment-1"]);

        let missing = with_derivation(&base, "agent:absent", vec![]);
        let report = evaluate_complete(&missing);
        assert_eq!(state(&report, AGENT_IDENTIFIED), Fail);
        assert_eq!(targets(&report, AGENT_IDENTIFIED), ["drv:segment-1"]);
        let finding = report
            .findings
            .iter()
            .find(|f| f.payload.code == AGENT_IDENTIFIED)
            .unwrap();
        assert_eq!(
            finding.semantic_condition_key,
            "provenance-agent-identified"
        );

        let source = first_source(&base);
        let not_agent = with_derivation(&base, source.as_str(), vec![]);
        assert_eq!(
            state(&evaluate_complete(&not_agent), AGENT_IDENTIFIED),
            Fail
        );

        let deprecated_agent = node_json(
            "agent:segmenter",
            "Proposed",
            "Agent",
            json!({"agent_kind": "software_service"}),
            "actor:importer",
        );
        let proposed = with_derivation(&base, "agent:segmenter", vec![deprecated_agent]);
        assert_eq!(state(&evaluate_complete(&proposed), AGENT_IDENTIFIED), Fail);
    }

    // ------------------------------------------------------------------ BASELINE.HASHABLE

    #[test]
    fn baseline_hashable_requires_the_exact_hash_and_one_canonical_manifest() {
        let corpus = small_corpus();
        let graph = corpus.graph();
        let complete = corpus.with_manifest();
        let fails = |report: &GateReport| {
            let r = result(report, BASELINE_HASHABLE);
            r.state == Fail
                && r.targets.is_empty()
                && r.semantic_condition_key.as_deref() == Some("baseline-evidence-hash")
        };
        assert_eq!(
            state(&evaluate(&corpus, &complete), BASELINE_HASHABLE),
            Pass
        );

        // Wrong expected evidence hash: evaluated, not a context error.
        assert!(fails(&evaluate_with(
            &graph,
            Hash::evidence_sha256(b"other"),
            &complete
        )));

        // Zero and two manifests.
        assert!(fails(&evaluate(&corpus, &corpus.artifacts)));
        let mut other = corpus.clone();
        other.nodes.truncate(2);
        let stale = evidence_manifest_artifact(
            &build_evidence_manifest(&other.graph()).unwrap(),
            T1.parse().unwrap(),
        )
        .unwrap();
        let mut two = complete.clone();
        two.push(stale.clone());
        assert!(fails(&evaluate(&corpus, &two)));

        // Stale, wrong media, malformed and non-canonical manifests.
        let mut with_stale = corpus.artifacts.clone();
        with_stale.push(stale);
        assert!(fails(&evaluate(&corpus, &with_stale)));
        let manifest_artifact = complete.last().unwrap().clone();
        let replace = |bytes: Vec<u8>, media: &str| {
            let mut artifacts = corpus.artifacts.clone();
            artifacts.push(Artifact {
                hash: Hash::content_sha256(&bytes),
                kind: ArtifactKind::EvidenceManifest,
                media_type: media.to_owned(),
                bytes,
                created_at: T1.parse().unwrap(),
            });
            artifacts
        };
        assert!(fails(&evaluate(
            &corpus,
            &replace(manifest_artifact.bytes.clone(), "text/plain")
        )));
        assert!(fails(&evaluate(
            &corpus,
            &replace(b"{\"version\":1".to_vec(), "application/json")
        )));
        let value: Value = serde_json::from_slice(&manifest_artifact.bytes).unwrap();
        let pretty = serde_json::to_vec_pretty(&value).unwrap();
        assert!(fails(&evaluate(
            &corpus,
            &replace(pretty, "application/json")
        )));

        let report = evaluate(&corpus, &corpus.artifacts);
        let finding = report
            .findings
            .iter()
            .find(|f| f.payload.code == BASELINE_HASHABLE)
            .unwrap();
        assert!(finding.payload.affected_refs.is_empty());
        assert_eq!(
            finding.payload.message,
            "The evidence baseline cannot be reproduced from the evaluated graph and evidence manifest."
        );
        assert_eq!(report.result, GateResult::Fail);
    }

    // ------------------------------------------------------------------ empty sets

    #[test]
    fn empty_evidence_baseline_passes_universal_rules() {
        let corpus = Corpus::empty();
        let report = evaluate_complete(&corpus);
        for rule_id in [
            CONTENT_ADDRESSED,
            LOCATABLE,
            HASH_MATCH,
            PARSE_STATUS,
            BASELINE_HASHABLE,
        ] {
            assert_eq!(state(&report, rule_id), Pass, "{rule_id}");
            assert!(targets(&report, rule_id).is_empty(), "{rule_id}");
            assert!(result(&report, rule_id).evidence.is_empty(), "{rule_id}");
        }
        assert_eq!(state(&report, AGENT_IDENTIFIED), NotApplicable);
        assert_eq!(report.result, GateResult::Pass);
    }

    #[test]
    fn only_foreign_evidence_leaves_built_in_rules_empty() {
        let (source, fragment, artifact) = foreign_source();
        let corpus = Corpus {
            nodes: vec![source.clone(), fragment],
            artifacts: vec![artifact],
        };
        let report = evaluate(&corpus, &corpus.artifacts);
        for rule_id in [LOCATABLE, HASH_MATCH, PARSE_STATUS] {
            assert_eq!(state(&report, rule_id), Pass, "{rule_id}");
            assert!(targets(&report, rule_id).is_empty(), "{rule_id}");
        }
        assert_eq!(state(&report, CONTENT_ADDRESSED), Pass);
        assert_eq!(targets(&report, CONTENT_ADDRESSED), [source.id.to_string()]);
        assert_eq!(state(&report, BASELINE_HASHABLE), Fail);
    }

    // ------------------------------------------------------------------ determinism

    #[test]
    fn i0_reports_are_deterministic_and_time_independent() {
        let corpus = hr_corpus();
        let artifacts = corpus.with_manifest();
        let a = evaluate(&corpus, &artifacts);
        let b = evaluate(&corpus, &artifacts);
        assert_eq!(a.content_hash().unwrap(), b.content_hash().unwrap());

        let mut reordered = corpus.clone();
        reordered.nodes.reverse();
        let mut reversed_artifacts = artifacts.clone();
        reversed_artifacts.reverse();
        assert_eq!(
            evaluate(&reordered, &reversed_artifacts)
                .content_hash()
                .unwrap(),
            a.content_hash().unwrap()
        );

        // A manifest artifact acquired at another time is the same input.
        let manifest = build_evidence_manifest(&corpus.graph()).unwrap();
        let early = evidence_manifest_artifact(&manifest, T1.parse().unwrap()).unwrap();
        let late = evidence_manifest_artifact(&manifest, T2.parse::<Timestamp>().unwrap()).unwrap();
        assert_eq!((&early.bytes, &early.hash), (&late.bytes, &late.hash));
        assert_ne!(early.created_at, late.created_at);
        assert_eq!(
            ValidationArtifactInput::from(&early),
            ValidationArtifactInput::from(&late)
        );
        let mut later = corpus.artifacts.clone();
        later.push(late);
        assert_eq!(
            evaluate(&corpus, &later).content_hash().unwrap(),
            a.content_hash().unwrap()
        );
        let by_rule: BTreeMap<&str, RuleResultState> = a
            .rules
            .iter()
            .map(|r| (r.rule_id.as_str(), r.state))
            .collect();
        assert_eq!(by_rule.len(), 6);
    }

    // ------------------------------------------------------------------ source guard

    #[test]
    fn i0_and_manifest_sources_have_no_capability() {
        for (name, source) in [
            ("i0.rs", include_str!("../src/rules/i0.rs")),
            (
                "manifest.rs",
                include_str!("../../plumb-import/src/manifest.rs"),
            ),
        ] {
            let production = source.split("#[cfg(test)]").next().unwrap();
            for forbidden in [
                "ArtifactStore",
                "SqliteArtifactStore",
                "SqliteRevisionStore",
                "rusqlite",
                "std::fs",
                "File::open",
                "SystemClock",
                "Clock::now",
                "Utc::now",
                "Instant::now",
                "reqwest",
                "InferenceProvider",
                "LlmProvider",
                "plumb_compiler",
                "branch",
                "commit(",
                "SemanticPatch",
                "Proposal",
                "unsafe",
                "Graph::new",
            ] {
                assert!(
                    !production.contains(forbidden),
                    "{name} contains {forbidden}"
                );
            }
        }
    }
}
