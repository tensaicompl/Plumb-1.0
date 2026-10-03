//! S0.4 contract tests for S1 requirement segmentation: request construction, inference
//! consumption, post-checks, the deterministic fallback and its recovery provenance.
//!
//! Every test lives in `segment_contract` so that `cargo test -p plumb-import segment` selects
//! them. Golden values were computed independently with Python `hashlib` over RFC 8785 JSON,
//! never with the helpers under test.

mod segment_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use jsonschema::{Draft, JSONSchema};
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_import::*;
    use plumb_inference::{
        InferenceArtifact, InferenceProvider, MockProvider, ProviderExecution, ProviderPolicy,
    };
    use plumb_psg::{
        AuditMeta, DerivationKind, ElementStatus, EvidenceFragment, EvidenceLocator, Node,
        NodePayload,
    };
    use serde_json::{json, Value};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s1-segment-requirements.md");
    const SCHEMA: &str = include_str!("../../../schemas/inference/s1-segmentation.schema.json");
    const HR_MD: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.md");
    const HR_DOCX: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.docx");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:d7d6267608a65e754431056bed329885b7678380f46edc917f36b24caa6b7a70";
    const SCHEMA_HASH: &str =
        "sha256:ac038cd92a869a46205ec01d3a5cdedfec0b5a413f80ac002a2548c3785212d7";
    const CONTEXT_HASH: &str =
        "sha256:330dab252a7e50b0464183707ec478b881e525e422d53d3137e227a18f778ac6";
    const REQUEST_ID: &str =
        "sha256:52434ff8f22ef4078d2987def95e2045a5e9428f507d0d3210b8664d28d3897b";
    const SEG_A_REQ: &str = "seg:260f48b32b3c004b";
    const SEG_A_NON: &str = "seg:bd9fa27f867f85e9";
    const SEG_B_REQ: &str = "seg:4b37133ddf60126f";
    const DRV_ID: &str = "drv:7aa33fdb91ceeeb4";

    const A: &str = "evd:00000000000000a1";
    const B: &str = "evd:00000000000000b2";
    const TEXT_A: &str = "The system shall record leave. Weekends are excluded.";
    const TEXT_B: &str = "- Employee leave balance";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn fragment(fragment_id: &str, text: &str, kind: &str) -> Node {
        let mut metadata = json!({
            "kind": kind,
            "extracted_ref": Hash::content_sha256(b"extracted"),
        });
        if kind == "table_row" {
            metadata["header_cells"] = json!(["Column"]);
        }
        Node {
            id: id(fragment_id),
            revision: 1,
            status: ElementStatus::Accepted,
            payload: NodePayload::EvidenceFragment(EvidenceFragment {
                source_ref: id("src:0000000000000001"),
                locator: EvidenceLocator::TextRange {
                    start: 0,
                    end: text.len() as u64,
                },
                content_hash: Hash::content_sha256(text.as_bytes()),
                extracted_text: Some(text.to_owned()),
                speaker: None,
                source_timestamp: None,
            }),
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::from([(FRAGMENT_EXTENSION.parse().unwrap(), metadata)]),
            audit: AuditMeta::new(id("actor:importer"), ts(AT), None, None).unwrap(),
        }
    }

    fn fixed_fragments() -> Vec<Node> {
        vec![
            fragment(A, TEXT_A, "paragraph"),
            fragment(B, TEXT_B, "list_item"),
        ]
    }

    fn policy(provider: &str) -> ProviderPolicy {
        ProviderPolicy {
            provider: provider.to_owned(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn mock_policy() -> ProviderPolicy {
        policy("mock")
    }

    fn audit_at(at: &str) -> SegmentationAudit {
        SegmentationAudit {
            created_by: id("agent:segmenter"),
            created_at: ts(at),
        }
    }

    fn audit() -> SegmentationAudit {
        audit_at(AT)
    }

    fn request_for(fragments: &[Node]) -> SegmentationRequest {
        build_segmentation_request(fragments, mock_policy()).unwrap()
    }

    fn segment(fragment_ref: &str, start: u64, end: u64, classification: &str) -> Value {
        json!({
            "fragment_ref": fragment_ref,
            "start": start,
            "end": end,
            "classification": classification,
        })
    }

    fn output(segments: Vec<Value>) -> Value {
        json!({"version": 1, "segments": segments})
    }

    /// A ProviderExecution valid for `request` whose validated output is `output`.
    fn execution(request: &SegmentationRequest, output: Value) -> ProviderExecution {
        let raw_response = to_canonical_json(&output).unwrap();
        let validated_output = CanonicalJson::new(output);
        ProviderExecution {
            artifact: InferenceArtifact {
                request_hash: request.request.id.clone(),
                provider: request.request.provider_policy.provider.clone(),
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

    fn artifact(request: &SegmentationRequest, output: Value) -> InferenceArtifact {
        execution(request, output).artifact
    }

    fn evaluate(
        fragments: &[Node],
        inference: Option<&InferenceArtifact>,
    ) -> Result<SegmentationResult, SegmentationError> {
        evaluate_segmentation(&request_for(fragments), fragments, inference, &audit())
    }

    fn fallback(fragments: &[Node]) -> SegmentationResult {
        let result = evaluate(fragments, None).unwrap();
        assert_eq!(result.mode, SegmentationMode::Fallback);
        result
    }

    /// The fallback candidates of one fragment as (text, classification, flags).
    fn fallback_units(
        text: &str,
        kind: &str,
    ) -> Vec<(String, SegmentClassification, Vec<SegmentFlag>)> {
        let fragments = [fragment(A, text, kind)];
        fallback(&fragments)
            .candidates
            .into_iter()
            .map(|c| {
                (
                    text[c.start as usize..c.end as usize].to_owned(),
                    c.classification,
                    c.flags,
                )
            })
            .collect()
    }

    fn sha_hex16(prefix: &str, body: &Value) -> String {
        let digest = Hash::content_sha256(&to_canonical_json(body).unwrap());
        format!("{prefix}:{}", &digest.as_str()["sha256:".len()..][..16])
    }

    /// Full partition of every fragment, canonical order and recomputable IDs.
    fn assert_partition(result: &SegmentationResult, fragments: &[Node]) {
        let texts: BTreeMap<Id, String> = fragments
            .iter()
            .map(|node| match &node.payload {
                NodePayload::EvidenceFragment(f) => {
                    (node.id.clone(), f.extracted_text.clone().unwrap())
                }
                _ => unreachable!(),
            })
            .collect();
        let keys: Vec<_> = result
            .candidates
            .iter()
            .map(|c| {
                (
                    c.fragment_ref.clone(),
                    c.start,
                    c.end,
                    c.classification.as_str(),
                )
            })
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "canonical candidate order");
        for (fragment_ref, text) in &texts {
            let ranges: Vec<_> = result
                .candidates
                .iter()
                .filter(|c| &c.fragment_ref == fragment_ref)
                .collect();
            assert!(!ranges.is_empty());
            assert_eq!(ranges[0].start, 0);
            assert_eq!(ranges.last().unwrap().end, text.len() as u64);
            for pair in ranges.windows(2) {
                assert_eq!(pair[0].end, pair[1].start, "contiguous, no overlap or gap");
            }
            for c in &ranges {
                assert!(c.start < c.end);
                assert!(text.is_char_boundary(c.start as usize));
                assert!(text.is_char_boundary(c.end as usize));
            }
        }
        for c in &result.candidates {
            assert!(texts.contains_key(&c.fragment_ref));
            let expected = sha_hex16(
                "seg",
                &json!({
                    "fragment_ref": c.fragment_ref,
                    "start": c.start,
                    "end": c.end,
                    "classification": c.classification.as_str(),
                }),
            );
            assert_eq!(c.id.as_str(), expected);
        }
    }

    // ------------------------------------------------------------------ schema and assets

    fn compiled_schema() -> JSONSchema {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&schema)
            .unwrap()
    }

    #[test]
    fn segment_schema_is_draft_2020_12_and_compiles() {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(
            schema["$schema"],
            json!("https://json-schema.org/draft/2020-12/schema")
        );
        assert!(!SCHEMA.contains("$ref"));
        let compiled = compiled_schema();
        assert!(compiled.is_valid(&output(vec![segment(A, 0, 3, "requirement_candidate")])));
        assert!(compiled.is_valid(&output(vec![])));
    }

    #[test]
    fn segment_schema_rejects_invalid_outputs() {
        let compiled = compiled_schema();
        let mut unknown_top = output(vec![]);
        unknown_top["confidence"] = json!(1);
        let mut unknown_segment = segment(A, 0, 3, "non_requirement");
        unknown_segment["candidate_id"] = json!("seg:0000000000000000");
        let mut missing = segment(A, 0, 3, "non_requirement");
        missing.as_object_mut().unwrap().remove("end");
        let invalid = [
            unknown_top,
            output(vec![unknown_segment]),
            output(vec![missing]),
            json!({"version": 1}),
            json!({"segments": []}),
            json!({"version": 2, "segments": []}),
            output(vec![segment(A, 0, 3, "non_requirement")
                .as_object()
                .cloned()
                .map(|mut o| {
                    o.insert("start".into(), json!(-1));
                    Value::Object(o)
                })
                .unwrap()]),
            output(vec![segment(A, 0, 0, "non_requirement")]),
            output(vec![segment(A, 0, 3, "Requirement_Candidate")]),
            output(vec![segment(A, 0, 3, "requirement")]),
            output(vec![segment("EVD:bad", 0, 3, "non_requirement")]),
            output(vec![segment("evd", 0, 3, "non_requirement")]),
        ];
        for value in invalid {
            assert!(!compiled.is_valid(&value), "{value}");
        }
    }

    #[test]
    fn segment_prompt_and_schema_hashes_are_exact_file_bytes() {
        let request = request_for(&fixed_fragments());
        assert_eq!(request.request.prompt_template_hash.as_str(), PROMPT_HASH);
        assert_eq!(request.request.schema_hash.as_str(), SCHEMA_HASH);
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
    }

    #[test]
    fn segment_assets_have_no_placeholders() {
        for content in [
            String::from_utf8(PROMPT.to_vec()).unwrap(),
            SCHEMA.to_owned(),
        ] {
            let lower = content.to_lowercase();
            for marker in ["todo", "tbd", "example", "placeholder"] {
                assert!(!lower.contains(marker), "{marker}");
            }
        }
    }

    // ------------------------------------------------------------------ request

    #[test]
    fn segment_request_golden() {
        let request = request_for(&fixed_fragments());
        let inner = &request.request;
        assert_eq!(inner.stage, StageId::S1);
        assert_eq!(inner.task_kind, SEGMENTATION_TASK_KIND);
        assert_eq!(SEGMENTATION_TASK_KIND, "requirement_segmentation");
        assert!(inner.input_refs.is_empty());
        assert_eq!(inner.evidence_refs, vec![id(A), id(B)]);
        assert_eq!(inner.context_hash.as_str(), CONTEXT_HASH);
        assert_eq!(inner.provider_policy, mock_policy());
        assert_eq!(inner.id.as_str(), REQUEST_ID);
        assert_eq!(
            request.context,
            SegmentationContext {
                version: SEGMENTATION_CONTEXT_VERSION,
                fragments: vec![
                    SegmentationContextFragment {
                        fragment_ref: id(A),
                        text: TEXT_A.to_owned(),
                    },
                    SegmentationContextFragment {
                        fragment_ref: id(B),
                        text: TEXT_B.to_owned(),
                    },
                ],
            }
        );
        assert_eq!(
            serde_json::to_value(&request.context).unwrap(),
            json!({"version": 1, "fragments": [
                {"fragment_ref": A, "text": TEXT_A},
                {"fragment_ref": B, "text": TEXT_B},
            ]})
        );
        assert_eq!(SEGMENTATION_CONTEXT_VERSION, 1);
        assert_eq!(SEGMENTATION_OUTPUT_VERSION, 1);
        assert!(SegmentationRequest::new(request.request.clone(), request.context.clone()).is_ok());
    }

    #[test]
    fn segment_request_rejects_mismatched_context() {
        let request = request_for(&fixed_fragments());
        let mut context = request.context.clone();
        context.fragments[0].text.push('!');
        assert!(matches!(
            SegmentationRequest::new(request.request.clone(), context),
            Err(SegmentationError::InvalidInput { .. })
        ));
        let mut unknown = serde_json::to_value(&request.context).unwrap();
        unknown["kind"] = json!("paragraph");
        assert!(serde_json::from_value::<SegmentationContext>(unknown).is_err());
    }

    #[test]
    fn segment_request_policy_participates_in_identity() {
        let fragments = fixed_fragments();
        let mock = build_segmentation_request(&fragments, policy("mock")).unwrap();
        let other = build_segmentation_request(&fragments, policy("other-provider")).unwrap();
        let configured = build_segmentation_request(
            &fragments,
            ProviderPolicy {
                provider: "mock".to_owned(),
                config: CanonicalJson::new(json!({"temperature": 0})),
            },
        )
        .unwrap();
        assert_ne!(mock.request.id, other.request.id);
        assert_ne!(mock.request.id, configured.request.id);
        assert_eq!(mock.context, other.context);
        assert_eq!(mock.request.context_hash, other.request.context_hash);
    }

    #[test]
    fn segment_request_is_input_order_independent() {
        let mut fragments = fixed_fragments();
        let forward = request_for(&fragments);
        fragments.reverse();
        let reversed = request_for(&fragments);
        assert_eq!(forward, reversed);
        assert_eq!(
            forward.request.evidence_refs,
            reversed.request.evidence_refs
        );
        assert_eq!(forward.request.context_hash, reversed.request.context_hash);
    }

    #[test]
    fn segment_request_is_text_sensitive() {
        let base = request_for(&fixed_fragments());
        let changed = request_for(&[
            fragment(
                A,
                "The system shall record leave! Weekends are excluded.",
                "paragraph",
            ),
            fragment(B, TEXT_B, "list_item"),
        ]);
        assert_ne!(base.request.context_hash, changed.request.context_hash);
        assert_ne!(base.request.id, changed.request.id);
        assert_eq!(base.request.evidence_refs, changed.request.evidence_refs);
    }

    #[test]
    fn segment_request_excludes_fragment_kind() {
        let base = request_for(&fixed_fragments());
        let rekinded = request_for(&[
            fragment(A, TEXT_A, "heading"),
            fragment(B, TEXT_B, "paragraph"),
        ]);
        assert_eq!(base, rekinded);
    }

    #[test]
    fn segment_request_rejects_invalid_inputs() {
        let valid = fragment(A, TEXT_A, "paragraph");
        let reject = |node: Node| {
            assert!(matches!(
                build_segmentation_request(&[node], mock_policy()),
                Err(SegmentationError::InvalidInput { .. })
            ));
        };
        for status in [ElementStatus::Proposed, ElementStatus::Rejected] {
            let mut node = valid.clone();
            node.status = status;
            reject(node);
        }
        let mut no_text = valid.clone();
        if let NodePayload::EvidenceFragment(f) = &mut no_text.payload {
            f.extracted_text = None;
        }
        reject(no_text);
        reject(fragment(A, "", "paragraph"));
        let mut no_metadata = valid.clone();
        no_metadata.extensions.clear();
        reject(no_metadata);
        let mut malformed = valid.clone();
        malformed.extensions = BTreeMap::from([(
            FRAGMENT_EXTENSION.parse().unwrap(),
            json!({"kind": "sentence", "extracted_ref": Hash::content_sha256(b"x")}),
        )]);
        reject(malformed);
        let mut agent = valid.clone();
        agent.payload = NodePayload::Agent(plumb_psg::Agent {
            agent_kind: plumb_psg::AgentKind::CompilerStage,
        });
        reject(agent);
        assert!(matches!(
            build_segmentation_request(&[valid.clone(), valid], mock_policy()),
            Err(SegmentationError::InvalidInput { .. })
        ));
        assert!(matches!(
            build_segmentation_request(&[], mock_policy()),
            Err(SegmentationError::InvalidInput { .. })
        ));
        let suspect = {
            let mut node = fragment(A, TEXT_A, "paragraph");
            node.status = ElementStatus::Suspect;
            node
        };
        assert!(build_segmentation_request(&[suspect], mock_policy()).is_ok());
    }

    // ------------------------------------------------------------------ inference path

    fn valid_output() -> Value {
        output(vec![
            segment(B, 0, 24, "requirement_candidate"),
            segment(A, 31, 53, "non_requirement"),
            segment(A, 0, 31, "requirement_candidate"),
        ])
    }

    #[test]
    fn segment_mock_inference_path() {
        let fragments = fixed_fragments();
        let request = request_for(&fragments);
        let mut provider = MockProvider::new();
        provider
            .register(
                request.request.id.clone(),
                execution(&request, valid_output()),
            )
            .unwrap();
        let executed = provider.execute(&request.request).unwrap();
        let result =
            evaluate_segmentation(&request, &fragments, Some(&executed.artifact), &audit())
                .unwrap();
        assert_eq!(result.mode, SegmentationMode::Inference);
        assert_eq!(result.fallback_reason, None);
        assert_eq!(result.fallback_derivation, None);
        assert_partition(&result, &fragments);
        let ids: Vec<&str> = result.candidates.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, [SEG_A_REQ, SEG_A_NON, SEG_B_REQ]);
        assert!(result.candidates.iter().all(|c| c.flags.is_empty()));
        assert_eq!(
            result.candidates[1].classification,
            SegmentClassification::NonRequirement
        );
    }

    #[test]
    fn segment_mock_fixture_is_keyed_by_request_id() {
        let request = request_for(&fixed_fragments());
        let mut provider = MockProvider::new();
        assert!(provider
            .register(
                request.request.context_hash.clone(),
                execution(&request, valid_output())
            )
            .is_err());
        provider
            .register(
                request.request.id.clone(),
                execution(&request, valid_output()),
            )
            .unwrap();
        let executed = provider.execute(&request.request).unwrap();
        assert_eq!(executed.artifact.request_hash, request.request.id);
        assert_eq!(
            executed.artifact.provider,
            request.request.provider_policy.provider
        );
        executed.validate_for(&request.request).unwrap();
        let other = request_for(&[fragment(A, TEXT_A, "paragraph")]);
        assert!(provider.execute(&other.request).is_err());
    }

    #[test]
    fn segment_inference_output_order_does_not_control_result_order() {
        let fragments = fixed_fragments();
        let request = request_for(&fragments);
        let mut reordered = valid_output();
        reordered["segments"].as_array_mut().unwrap().reverse();
        let a = evaluate_segmentation(
            &request,
            &fragments,
            Some(&artifact(&request, valid_output())),
            &audit(),
        )
        .unwrap();
        let b = evaluate_segmentation(
            &request,
            &fragments,
            Some(&artifact(&request, reordered)),
            &audit(),
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn segment_missing_inference_uses_fallback() {
        let fragments = fixed_fragments();
        let result = evaluate(&fragments, None).unwrap();
        assert_eq!(result.mode, SegmentationMode::Fallback);
        assert_eq!(
            result.fallback_reason,
            Some(FallbackReason::MissingInference)
        );
        assert!(matches!(
            result.fallback_derivation.as_ref().map(|n| &n.payload),
            Some(NodePayload::DerivationRecord(r)) if r.kind == DerivationKind::Recovery
        ));
        assert_partition(&result, &fragments);
    }

    #[test]
    fn segment_invalid_artifact_uses_fallback() {
        let fragments = fixed_fragments();
        let request = request_for(&fragments);
        let mut wrong_request = artifact(&request, valid_output());
        wrong_request.request_hash = Hash::content_sha256(b"another request");
        let mut wrong_provider = artifact(&request, valid_output());
        wrong_provider.provider = "other-provider".to_owned();
        let mut wrong_output_hash = artifact(&request, valid_output());
        wrong_output_hash.validated_output_hash = Hash::content_sha256(b"other output");
        // Schema-invalid as well: validate_for fails first, so the reason is the artifact.
        let mut both = artifact(&request, json!({"nonsense": true}));
        both.request_hash = Hash::content_sha256(b"another request");
        for bad in [wrong_request, wrong_provider, wrong_output_hash, both] {
            let result = evaluate_segmentation(&request, &fragments, Some(&bad), &audit()).unwrap();
            assert_eq!(result.mode, SegmentationMode::Fallback);
            assert_eq!(
                result.fallback_reason,
                Some(FallbackReason::InvalidInferenceArtifact)
            );
            assert_partition(&result, &fragments);
        }
    }

    #[test]
    fn segment_schema_invalid_artifact_uses_fallback() {
        let fragments = fixed_fragments();
        let request = request_for(&fragments);
        let invalid_outputs = [
            json!({"nonsense": true}),
            json!({"version": 2, "segments": []}),
            output(vec![segment(A, 0, 0, "requirement_candidate")]),
            output(vec![segment(A, 0, 53, "maybe")]),
            // Schema-valid integer that does not decode as u64.
            output(vec![json!({
                "fragment_ref": A, "start": 0, "end": 1.5e30, "classification": "non_requirement"
            })]),
        ];
        for value in invalid_outputs {
            let result = evaluate_segmentation(
                &request,
                &fragments,
                Some(&artifact(&request, value.clone())),
                &audit(),
            )
            .unwrap();
            assert_eq!(result.mode, SegmentationMode::Fallback, "{value}");
            assert_eq!(result.fallback_reason, Some(FallbackReason::SchemaInvalid));
            assert_partition(&result, &fragments);
        }
    }

    fn inference_error(segments: Vec<Value>) -> SegmentationError {
        let fragments = fixed_fragments();
        let request = request_for(&fragments);
        let artifact = artifact(&request, output(segments));
        assert!(compiled_schema().is_valid(artifact.validated_output.as_value()));
        evaluate_segmentation(&request, &fragments, Some(&artifact), &audit()).unwrap_err()
    }

    #[test]
    fn segment_schema_valid_gap_is_intake_uncovered() {
        let error = inference_error(vec![
            segment(A, 0, 10, "requirement_candidate"),
            segment(A, 20, 31, "requirement_candidate"),
            segment(B, 0, 24, "requirement_candidate"),
        ]);
        assert_eq!(error.code(), Some("E_INTAKE_UNCOVERED"));
        assert_eq!(E_INTAKE_UNCOVERED, "E_INTAKE_UNCOVERED");
        let SegmentationError::IntakeUncovered { gaps } = error else {
            panic!("expected IntakeUncovered");
        };
        assert_eq!(
            gaps,
            vec![
                SegmentGap {
                    fragment_ref: id(A),
                    start: 10,
                    end: 20
                },
                SegmentGap {
                    fragment_ref: id(A),
                    start: 31,
                    end: 53
                },
            ]
        );
    }

    #[test]
    fn segment_gap_reports_uncovered_fragments_sorted() {
        let error = inference_error(vec![segment(A, 5, 53, "non_requirement")]);
        let SegmentationError::IntakeUncovered { gaps } = error else {
            panic!("expected IntakeUncovered");
        };
        assert_eq!(
            gaps,
            vec![
                SegmentGap {
                    fragment_ref: id(A),
                    start: 0,
                    end: 5
                },
                SegmentGap {
                    fragment_ref: id(B),
                    start: 0,
                    end: 24
                },
            ]
        );
        assert!(matches!(
            inference_error(vec![]),
            SegmentationError::IntakeUncovered { gaps } if gaps.len() == 2
        ));
    }

    #[test]
    fn segment_overlap_is_rejected() {
        let error = inference_error(vec![
            segment(A, 0, 31, "requirement_candidate"),
            segment(A, 30, 53, "non_requirement"),
            segment(B, 0, 24, "requirement_candidate"),
        ]);
        assert!(matches!(
            error,
            SegmentationError::Overlap {
                first_start: 0,
                first_end: 31,
                second_start: 30,
                second_end: 53,
                ..
            }
        ));
        assert_eq!(error.code(), None);
    }

    #[test]
    fn segment_duplicate_segments_are_overlap() {
        let error = inference_error(vec![
            segment(A, 0, 53, "requirement_candidate"),
            segment(A, 0, 53, "requirement_candidate"),
            segment(B, 0, 24, "requirement_candidate"),
        ]);
        assert!(matches!(error, SegmentationError::Overlap { .. }));
        let error = inference_error(vec![
            segment(A, 0, 53, "requirement_candidate"),
            segment(A, 0, 53, "non_requirement"),
            segment(B, 0, 24, "requirement_candidate"),
        ]);
        assert!(matches!(error, SegmentationError::Overlap { .. }));
    }

    #[test]
    fn segment_utf8_boundary_is_invalid_range() {
        // "Zażółć" is 10 bytes: Z a ż(2) ó(2) ł(2) ć(2); byte 3 is inside "ż".
        let text = "Zażółć żądanie shall pass.";
        let fragments = [fragment(A, text, "paragraph")];
        let request = request_for(&fragments);
        let cut = artifact(
            &request,
            output(vec![
                segment(A, 0, 3, "non_requirement"),
                segment(A, 3, text.len() as u64, "requirement_candidate"),
            ]),
        );
        assert!(matches!(
            evaluate_segmentation(&request, &fragments, Some(&cut), &audit()),
            Err(SegmentationError::InvalidRange {
                start: 0,
                end: 3,
                ..
            })
        ));
        // Character counts are not byte offsets: 26 characters, 32 bytes.
        assert_eq!((text.chars().count(), text.len()), (26, 32));
        let chars = artifact(
            &request,
            output(vec![segment(A, 0, 26, "requirement_candidate")]),
        );
        assert!(matches!(
            evaluate_segmentation(&request, &fragments, Some(&chars), &audit()),
            Err(SegmentationError::IntakeUncovered { gaps }) if gaps == vec![SegmentGap {
                fragment_ref: id(A),
                start: 26,
                end: 32,
            }]
        ));
        let whole = artifact(
            &request,
            output(vec![
                segment(A, 0, 4, "non_requirement"),
                segment(A, 4, text.len() as u64, "requirement_candidate"),
            ]),
        );
        let result = evaluate_segmentation(&request, &fragments, Some(&whole), &audit()).unwrap();
        assert_partition(&result, &fragments);
    }

    #[test]
    fn segment_out_of_bounds_and_empty_ranges_are_invalid() {
        assert!(matches!(
            inference_error(vec![
                segment(A, 0, 54, "requirement_candidate"),
                segment(B, 0, 24, "requirement_candidate"),
            ]),
            SegmentationError::InvalidRange { end: 54, .. }
        ));
        assert!(matches!(
            inference_error(vec![
                segment(A, 0, 53, "requirement_candidate"),
                segment(B, 5, 5, "requirement_candidate"),
                segment(B, 0, 24, "requirement_candidate"),
            ]),
            SegmentationError::InvalidRange {
                start: 5,
                end: 5,
                ..
            }
        ));
    }

    #[test]
    fn segment_unknown_fragment_is_rejected() {
        let error = inference_error(vec![
            segment(A, 0, 53, "requirement_candidate"),
            segment(B, 0, 24, "requirement_candidate"),
            segment("evd:00000000000000ff", 0, 3, "non_requirement"),
        ]);
        assert!(matches!(
            error,
            SegmentationError::UnknownFragment { fragment_ref } if fragment_ref == id("evd:00000000000000ff")
        ));
    }

    #[test]
    fn segment_post_check_precedence() {
        // Unknown fragment before an invalid range, invalid range before overlap,
        // overlap before gap.
        assert!(matches!(
            inference_error(vec![
                segment(A, 0, 99, "requirement_candidate"),
                segment("evd:00000000000000ff", 0, 3, "non_requirement"),
            ]),
            SegmentationError::UnknownFragment { .. }
        ));
        assert!(matches!(
            inference_error(vec![
                segment(A, 0, 31, "requirement_candidate"),
                segment(A, 30, 40, "non_requirement"),
                segment(B, 0, 99, "requirement_candidate"),
            ]),
            SegmentationError::InvalidRange { .. }
        ));
        assert!(matches!(
            inference_error(vec![
                segment(A, 0, 31, "requirement_candidate"),
                segment(A, 30, 40, "non_requirement"),
            ]),
            SegmentationError::Overlap { .. }
        ));
    }

    #[test]
    fn segment_evaluation_rejects_mismatched_fragments() {
        let fragments = fixed_fragments();
        let request = request_for(&fragments);
        let changed = [
            fragment(A, "The system shall record leave.", "paragraph"),
            fragment(B, TEXT_B, "list_item"),
        ];
        assert!(matches!(
            evaluate_segmentation(&request, &changed, None, &audit()),
            Err(SegmentationError::InvalidInput { .. })
        ));
        assert!(matches!(
            evaluate_segmentation(&request, &fragments[..1], None, &audit()),
            Err(SegmentationError::InvalidInput { .. })
        ));
        let mut tampered = request.clone();
        tampered.context.fragments[0].text = "Other text.".to_owned();
        assert!(matches!(
            evaluate_segmentation(&tampered, &fragments, None, &audit()),
            Err(SegmentationError::InvalidInput { .. })
        ));
    }

    // ------------------------------------------------------------------ fallback

    use SegmentClassification::{NonRequirement as Non, RequirementCandidate as Req};

    fn unassigned() -> Vec<SegmentFlag> {
        vec![SegmentFlag::UnassignedText]
    }

    #[test]
    fn segment_fallback_list_item_without_modal() {
        assert_eq!(
            fallback_units("- Employee leave balance", "list_item"),
            vec![("- Employee leave balance".to_owned(), Req, vec![])]
        );
        assert_eq!(
            fallback_units("No modal here. Second sentence.", "list_item"),
            vec![("No modal here. Second sentence.".to_owned(), Req, vec![])]
        );
    }

    #[test]
    fn segment_fallback_modal_tokens_match() {
        for text in [
            "The system shall log.",
            "The system SHALL log.",
            "Users Must log in.",
            "It should work.",
            "A user CAN leave.",
            "Can? Yes.",
            "It should-be done.",
            "Data shall not be lost.",
        ] {
            let units = fallback_units(text, "paragraph");
            assert_eq!(units[0].1, Req, "{text}");
            assert!(units[0].2.is_empty());
        }
    }

    #[test]
    fn segment_fallback_modal_non_matches() {
        for text in [
            "A user cannot leave.",
            "No scandal here.",
            "Use can2 now.",
            "Use _can_ now.",
            "Shallow musty shoulder.",
            "We may, will or need it; it is required.",
        ] {
            assert_eq!(
                fallback_units(text, "paragraph"),
                vec![(text.to_owned(), Non, unassigned())],
                "{text}"
            );
        }
    }

    #[test]
    fn segment_fallback_sentence_boundaries() {
        type Units = Vec<(&'static str, SegmentClassification)>;
        let cases: Vec<(&str, &str, Units)> = vec![
            (
                "period followed by space",
                "Users shall log in. Logs are kept",
                vec![("Users shall log in. ", Req), ("Logs are kept", Non)],
            ),
            (
                "question mark followed by LF",
                "Can users log in?\nYes",
                vec![("Can users log in?\n", Req), ("Yes", Non)],
            ),
            (
                "exclamation mark at end",
                "Stop! It must halt!",
                vec![("Stop! ", Non), ("It must halt!", Req)],
            ),
            (
                "period inside a token",
                "Version 2.5 of example.com must load.",
                vec![("Version 2.5 of example.com must load.", Req)],
            ),
            (
                "no terminal punctuation",
                "The system shall log",
                vec![("The system shall log", Req)],
            ),
            (
                "multiple spaces after punctuation",
                "One.   Two shall.",
                vec![("One.   ", Non), ("Two shall.", Req)],
            ),
            (
                "tab and CR after punctuation",
                "One.\t\r\nTwo.",
                vec![("One.\t\r\n", Non), ("Two.", Non)],
            ),
            (
                "leading whitespace belongs to the first sentence",
                "  Lead must.  Next",
                vec![("  Lead must.  ", Req), ("Next", Non)],
            ),
            (
                "multi-byte UTF-8 before and inside sentences",
                "Zażółć gęślą jaźń. Użytkownik must żądanie złożyć. Koniec…",
                vec![
                    ("Zażółć gęślą jaźń. ", Non),
                    ("Użytkownik must żądanie złożyć. ", Req),
                    ("Koniec…", Non),
                ],
            ),
            (
                "repeated terminators",
                "Really?! Yes... it can.",
                vec![("Really?! ", Non), ("Yes... ", Non), ("it can.", Req)],
            ),
            ("whitespace only", " \t\n", vec![(" \t\n", Non)]),
        ];
        for (name, text, expected) in cases {
            for kind in ["paragraph", "heading", "table_row"] {
                let units = fallback_units(text, kind);
                let actual: Vec<(&str, SegmentClassification)> =
                    units.iter().map(|(t, c, _)| (t.as_str(), *c)).collect();
                assert_eq!(actual, expected, "{name} ({kind})");
                assert_eq!(units.iter().map(|u| u.0.as_str()).collect::<String>(), text);
                for (_, classification, flags) in &units {
                    let expected_flags = if *classification == Non {
                        unassigned()
                    } else {
                        vec![]
                    };
                    assert_eq!(flags, &expected_flags, "{name}");
                }
            }
        }
    }

    #[test]
    fn segment_fallback_is_a_full_partition() {
        let fragments = [
            fragment(A, TEXT_A, "paragraph"),
            fragment(B, TEXT_B, "list_item"),
            fragment("evd:00000000000000c3", "Title must be shown", "heading"),
            fragment("evd:00000000000000d4", "HR-001\tshall\tx", "table_row"),
            fragment("evd:00000000000000e5", "ąę. żó must? ", "paragraph"),
        ];
        let result = fallback(&fragments);
        assert_partition(&result, &fragments);
        for c in &result.candidates {
            match c.classification {
                Req => assert!(c.flags.is_empty()),
                Non => assert_eq!(c.flags, unassigned()),
            }
        }
    }

    // ------------------------------------------------------------------ provenance

    #[test]
    fn segment_fallback_recovery_derivation() {
        let fragments = fixed_fragments();
        let request = request_for(&fragments);
        let result = evaluate_segmentation(&request, &fragments, None, &audit()).unwrap();
        let ids: Vec<&str> = result.candidates.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, [SEG_A_REQ, SEG_A_NON, SEG_B_REQ]);
        let node = result.fallback_derivation.clone().unwrap();
        let NodePayload::DerivationRecord(record) = &node.payload else {
            panic!("expected DerivationRecord");
        };
        assert_eq!(record.kind, DerivationKind::Recovery);
        assert_eq!(record.stage, "S1");
        assert_eq!(
            record.input_refs,
            vec![A.to_owned(), B.to_owned(), REQUEST_ID.to_owned()]
        );
        assert_eq!(
            record.output_refs,
            vec![
                SEG_A_REQ.to_owned(),
                SEG_B_REQ.to_owned(),
                SEG_A_NON.to_owned()
            ]
        );
        assert_eq!(record.created_at, ts(AT));
        assert_eq!(record.provider, None);
        assert_eq!(record.model, None);
        assert_eq!(record.prompt_template_hash, None);
        assert_eq!(record.schema_hash, None);
        assert_eq!(record.context_hash, None);
        assert_eq!(record.parameters, None);
        assert_eq!(record.raw_response_hash, None);
        assert_eq!(record.validated_output_hash, None);
        assert_eq!(record.id.as_str(), DRV_ID);
        assert_eq!(node.id, record.id);
        assert_eq!(node.revision, 1);
        assert_eq!(node.status, ElementStatus::Accepted);
        assert!(node.evidence.is_empty() && node.derivations.is_empty());
        assert!(node.standards.is_empty() && node.tags.is_empty() && node.extensions.is_empty());
        assert_eq!(node.audit.created_by, id("agent:segmenter"));
        assert_eq!(node.audit.created_at, ts(AT));
        assert_eq!(node.audit.updated_by, None);
        assert_eq!(node.audit.updated_at, None);
        node.validate().unwrap();
    }

    #[test]
    fn segment_recovery_derivation_id_is_the_record_body_hash() {
        let result = fallback(&fixed_fragments());
        let node = result.fallback_derivation.unwrap();
        let NodePayload::DerivationRecord(record) = &node.payload else {
            panic!("expected DerivationRecord");
        };
        let mut body = serde_json::to_value(record).unwrap();
        if let Value::Object(map) = &mut body {
            map.remove("id");
        }
        assert_eq!(sha_hex16("drv", &body), DRV_ID);
        assert_eq!(node.id.as_str(), DRV_ID);
        assert_eq!(
            body,
            json!({
                "kind": "recovery",
                "stage": "S1",
                "input_refs": [A, B, REQUEST_ID],
                "output_refs": [SEG_A_REQ, SEG_B_REQ, SEG_A_NON],
                "created_at": AT,
                "provider": null,
                "model": null,
                "prompt_template_hash": null,
                "schema_hash": null,
                "context_hash": null,
                "parameters": null,
                "raw_response_hash": null,
                "validated_output_hash": null,
            })
        );
    }

    #[test]
    fn segment_no_agent_is_created_and_created_by_is_preserved() {
        let fragments = fixed_fragments();
        let request = request_for(&fragments);
        let caller = SegmentationAudit {
            created_by: id("custom-ns:Any.Agent_1"),
            created_at: ts(AT),
        };
        let result = evaluate_segmentation(&request, &fragments, None, &caller).unwrap();
        let node = result.fallback_derivation.unwrap();
        assert_eq!(node.audit.created_by, caller.created_by);
        assert!(!matches!(node.payload, NodePayload::Agent(_)));
        let inference = evaluate_segmentation(
            &request,
            &fragments,
            Some(&artifact(&request, valid_output())),
            &caller,
        )
        .unwrap();
        assert_eq!(inference.fallback_derivation, None);
    }

    #[test]
    fn segment_created_at_changes_only_the_derivation() {
        let fragments = fixed_fragments();
        let request = request_for(&fragments);
        let first = evaluate_segmentation(&request, &fragments, None, &audit()).unwrap();
        let later = evaluate_segmentation(
            &request,
            &fragments,
            None,
            &audit_at("2026-06-30T12:00:00.000000000Z"),
        )
        .unwrap();
        assert_eq!(first.candidates, later.candidates);
        assert_ne!(first.fallback_derivation, later.fallback_derivation);
        assert_eq!(request_for(&fragments), request);
    }

    #[test]
    fn segment_vocabulary_wire_strings() {
        assert_eq!(
            SegmentClassification::ALL.map(|v| v.as_str()),
            ["requirement_candidate", "non_requirement"]
        );
        assert_eq!(SegmentFlag::ALL.map(|v| v.as_str()), ["unassigned_text"]);
        assert_eq!(
            SegmentationMode::ALL.map(|v| v.as_str()),
            ["inference", "fallback"]
        );
        assert_eq!(
            FallbackReason::ALL.map(|v| v.as_str()),
            [
                "missing_inference",
                "invalid_inference_artifact",
                "schema_invalid"
            ]
        );
        assert!("Requirement_Candidate"
            .parse::<SegmentClassification>()
            .is_err());
        assert!("UNASSIGNED_TEXT".parse::<SegmentFlag>().is_err());
        assert_eq!(
            serde_json::to_value(SegmentClassification::NonRequirement).unwrap(),
            json!("non_requirement")
        );
        let candidate = SegmentCandidate {
            id: id(SEG_A_REQ),
            fragment_ref: id(A),
            start: 0,
            end: 31,
            classification: SegmentClassification::RequirementCandidate,
            flags: vec![],
        };
        let mut value = serde_json::to_value(&candidate).unwrap();
        assert_eq!(
            value,
            json!({"id": SEG_A_REQ, "fragment_ref": A, "start": 0, "end": 31,
                   "classification": "requirement_candidate", "flags": []})
        );
        value["confidence"] = json!(1);
        assert!(serde_json::from_value::<SegmentCandidate>(value).is_err());
    }

    #[test]
    fn segment_production_source_guard() {
        let sources = [
            include_str!("../src/segment.rs"),
            include_str!("../src/fallback.rs"),
        ];
        let forbidden = [
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
            "::execute",
            "MockProvider",
            "persist_inference_bundle",
            "materialize_derivation_record",
            "SemanticPatch",
            "PatchSet",
            "Proposal",
            "Graph",
            "branch",
            "commit(",
            "unsafe",
            "openai",
            "anthropic",
            "\"mock\"",
        ];
        for source in sources {
            for token in forbidden {
                assert!(!source.contains(token), "{token}");
            }
        }
    }

    // ------------------------------------------------------------------ HR corpus

    fn import_audit() -> ImportAudit {
        ImportAudit {
            created_by: id("actor:importer"),
            created_at: ts(AT),
        }
    }

    fn text_of(node: &Node) -> &str {
        match &node.payload {
            NodePayload::EvidenceFragment(f) => f.extracted_text.as_deref().unwrap(),
            _ => unreachable!(),
        }
    }

    fn kind_of(node: &Node) -> FragmentKind {
        let value = &node.extensions[&FRAGMENT_EXTENSION.parse().unwrap()];
        serde_json::from_value::<FragmentMetadata>(value.clone())
            .unwrap()
            .kind
    }

    /// Every HR-NNN occurrence in fragments of `kind` lies inside a requirement candidate;
    /// returns how many of HR-001..HR-032 occur there.
    fn hr_ids_in_requirement_candidates(
        fragments: &[Node],
        result: &SegmentationResult,
        kind: FragmentKind,
    ) -> usize {
        let mut found = BTreeSet::new();
        for node in fragments.iter().filter(|n| kind_of(n) == kind) {
            let text = text_of(node);
            for n in 1..=32 {
                let hr = format!("HR-{n:03}");
                for (at, _) in text.match_indices(&hr) {
                    let (start, end) = (at as u64, (at + hr.len()) as u64);
                    let inside = result.candidates.iter().any(|c| {
                        c.fragment_ref == node.id
                            && c.classification == SegmentClassification::RequirementCandidate
                            && c.start <= start
                            && end <= c.end
                    });
                    assert!(inside, "{hr} in {}", node.id);
                    found.insert(hr.clone());
                }
            }
        }
        found.len()
    }

    #[test]
    fn segment_hr_markdown_fallback() {
        let imported = import_markdown("requirements.md", HR_MD, &import_audit()).unwrap();
        let fragments = imported.fragments;
        let result = fallback(&fragments);
        assert_eq!(
            result.fallback_reason,
            Some(FallbackReason::MissingInference)
        );
        assert_partition(&result, &fragments);
        for node in fragments
            .iter()
            .filter(|n| kind_of(n) == FragmentKind::ListItem)
        {
            let candidates: Vec<_> = result
                .candidates
                .iter()
                .filter(|c| c.fragment_ref == node.id)
                .collect();
            assert_eq!(candidates.len(), 1);
            assert_eq!(
                candidates[0].classification,
                SegmentClassification::RequirementCandidate
            );
        }
        assert_eq!(
            hr_ids_in_requirement_candidates(&fragments, &result, FragmentKind::ListItem),
            32
        );
        result.fallback_derivation.unwrap().validate().unwrap();
    }

    #[test]
    fn segment_hr_docx_fallback() {
        let imported = import_docx("requirements.docx", HR_DOCX, &import_audit()).unwrap();
        let fragments = imported.fragments;
        let result = fallback(&fragments);
        assert_partition(&result, &fragments);
        assert_eq!(
            hr_ids_in_requirement_candidates(&fragments, &result, FragmentKind::Paragraph),
            32
        );
        // Table rows follow the same sentence/modal rule; no HR-specific semantics.
        let rows: Vec<_> = fragments
            .iter()
            .filter(|n| kind_of(n) == FragmentKind::TableRow)
            .collect();
        assert!(!rows.is_empty());
        for row in rows {
            for c in result
                .candidates
                .iter()
                .filter(|c| c.fragment_ref == row.id)
            {
                let unit = &text_of(row)[c.start as usize..c.end as usize];
                let has_modal = unit
                    .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .any(|t| {
                        ["shall", "must", "should", "can"]
                            .iter()
                            .any(|m| t.eq_ignore_ascii_case(m))
                    });
                if has_modal {
                    assert_eq!(
                        c.classification,
                        SegmentClassification::RequirementCandidate
                    );
                    assert!(c.flags.is_empty());
                } else {
                    assert_eq!(c.classification, SegmentClassification::NonRequirement);
                    assert_eq!(c.flags, unassigned());
                }
            }
        }
    }

    #[test]
    fn segment_hr_fallback_is_deterministic() {
        for run in [
            || {
                import_markdown("requirements.md", HR_MD, &import_audit())
                    .unwrap()
                    .fragments
            },
            || {
                import_docx("requirements.docx", HR_DOCX, &import_audit())
                    .unwrap()
                    .fragments
            },
        ] {
            let first_fragments = run();
            let mut second_fragments = run();
            second_fragments.reverse();
            let first_request = request_for(&first_fragments);
            let second_request = request_for(&second_fragments);
            assert_eq!(first_request, second_request);
            let first =
                evaluate_segmentation(&first_request, &first_fragments, None, &audit()).unwrap();
            let second =
                evaluate_segmentation(&second_request, &second_fragments, None, &audit()).unwrap();
            assert_eq!(first, second);
        }
    }
}
