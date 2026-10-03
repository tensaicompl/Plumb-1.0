//! S1.3 contract tests for deterministic duplicate detection, F1 finding material, lossless
//! merge proposals and governed supersession proposals.
//!
//! Every test lives in `duplicates_contract` so that `cargo test -p plumb-functional
//! duplicates` selects them. Golden values were computed independently with Python `hashlib`
//! (finding key bytes and RFC 8785 JSON), never with the helpers under test.

mod duplicates_contract {
    use std::collections::{BTreeMap, BTreeSet};
    use std::str::FromStr;

    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_functional::duplicates::DuplicateResolutionProposal;
    use plumb_functional::*;
    use plumb_import::{
        build_segmentation_request, evaluate_segmentation, import_docx, import_markdown,
        import_plain_text, ImportAudit, SegmentationAudit,
    };
    use plumb_inference::{InferenceArtifact, InferenceRequest, ProviderPolicy};
    use plumb_patch::{
        apply_patch, AcceptancePolicy, MergePolicy, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        node_element_hash, AuditMeta, DerivationRef, Edge, ElementStatus, EvidenceRef, Finding,
        FindingSeverity, Graph, Modality, Node, NodePayload, RelationKind, RelationProperties,
        Requirement, RequirementKind, RequirementLevel, ResolutionDecision,
    };
    use plumb_validation::{load_builtin_software_profile, GeneratedFinding};
    use rust_decimal::Decimal;
    use serde_json::{json, Value};

    const HR_MD: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.md");
    const HR_DOCX: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.docx");
    const HR_PROFILE: &str = include_str!("../../../fixtures/hr-leave/profile.yaml");

    // Independently computed goldens (Python hashlib).
    const FINDING_KEY: &str =
        "sha256:f258f6b5df92a11560d48c2f16175d0e6852228f5499a04b5eebf9f6632e8a2c";
    const FINDING_ID: &str = "fnd:f258f6b5df92a115";
    const SUPERSEDES_EDGE_ID: &str = "rel:cd6dc99e8e4aa767";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const A: &str = "req:0000000000000a01";
    const B: &str = "req:0000000000000a02";
    const C: &str = "req:0000000000000a03";
    const STATEMENT: &str = "The system shall export the report.";

    use ElementStatus::{Accepted, Proposed};

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn audit() -> AuditMeta {
        AuditMeta::new(id("actor:human"), ts(AT), None, None).unwrap()
    }

    fn policy(threshold: &str) -> DuplicatePolicy {
        DuplicatePolicy {
            jaccard_threshold: Decimal::from_str(threshold).unwrap(),
        }
    }

    /// The duplicate threshold resolved from the HR profile configuration (test code only).
    fn hr_policy() -> DuplicatePolicy {
        let profile: serde_yaml::Value = serde_yaml::from_str(HR_PROFILE).unwrap();
        let threshold = match &profile["duplicate_jaccard"] {
            serde_yaml::Value::Number(n) => n.to_string(),
            other => panic!("unexpected duplicate_jaccard {other:?}"),
        };
        assert_eq!(threshold, "0.8");
        policy(&threshold)
    }

    fn requirement(statement: &str) -> Requirement {
        Requirement {
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
        }
    }

    fn node(node_id: &str, status: ElementStatus, payload: NodePayload, evidence: &[&Id]) -> Node {
        Node {
            id: id(node_id),
            revision: 1,
            status,
            payload,
            evidence: evidence
                .iter()
                .map(|e| EvidenceRef::from((*e).clone()))
                .collect(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::new(),
            audit: audit(),
        }
    }

    fn req_node(node_id: &str, status: ElementStatus, r: Requirement, evidence: &[&Id]) -> Node {
        node(node_id, status, NodePayload::Requirement(r), evidence)
    }

    /// Imported plain-text evidence: the source and fragment nodes and the fragment IDs.
    fn evidence(count: usize) -> (Vec<Node>, Vec<Id>) {
        let text: Vec<String> = (0..count)
            .map(|i| format!("Evidence paragraph {i}."))
            .collect();
        let audit = ImportAudit {
            created_by: id("actor:importer"),
            created_at: ts(AT),
        };
        let imported =
            import_plain_text("evidence.txt", text.join("\n\n").as_bytes(), &audit).unwrap();
        let ids = imported.fragments.iter().map(|f| f.id.clone()).collect();
        let mut nodes = vec![imported.source];
        nodes.extend(imported.fragments);
        (nodes, ids)
    }

    fn graph(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap()
    }

    /// A graph of evidence plus requirements built by `f` from the fragment IDs.
    fn with_requirements(f: impl Fn(&[Id]) -> Vec<Node>) -> Graph {
        let (mut nodes, fragments) = evidence(4);
        nodes.extend(f(&fragments));
        graph(nodes, Vec::new())
    }

    fn analyze(graph: &Graph) -> DuplicateAnalysisResult {
        analyze_requirement_duplicates(graph, &hr_policy()).unwrap()
    }

    fn pair(
        a: &str,
        a_status: ElementStatus,
        b: &str,
        b_status: ElementStatus,
        sa: &str,
        sb: &str,
    ) -> Graph {
        with_requirements(|f| {
            vec![
                req_node(a, a_status, requirement(sa), &[&f[0]]),
                req_node(b, b_status, requirement(sb), &[&f[1]]),
            ]
        })
    }

    fn only_match(result: &DuplicateAnalysisResult) -> &DuplicateMatch {
        assert_eq!(result.matches.len(), 1, "{:?}", result.matches);
        &result.matches[0]
    }

    fn edge(
        edge_id: &str,
        kind: RelationKind,
        from: &str,
        to: &str,
        status: ElementStatus,
    ) -> Edge {
        Edge {
            id: id(edge_id),
            revision: 1,
            status,
            kind,
            from: id(from),
            to: id(to),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: audit(),
        }
    }

    // ------------------------------------------------------------------ policy and tokens

    #[test]
    fn duplicates_policy_validation() {
        let g = pair(A, Accepted, B, Accepted, STATEMENT, STATEMENT);
        for bad in ["0", "-0.1", "1.01", "2"] {
            assert!(
                matches!(
                    analyze_requirement_duplicates(&g, &policy(bad)),
                    Err(DuplicateError::InvalidPolicy { .. })
                ),
                "{bad}"
            );
        }
        for good in ["0.0001", "0.5", "1"] {
            assert!(
                analyze_requirement_duplicates(&g, &policy(good)).is_ok(),
                "{good}"
            );
        }
        assert_eq!(hr_policy(), policy("0.8"));
    }

    #[test]
    fn duplicates_profile_identity_is_checked() {
        let (mut nodes, f) = evidence(2);
        nodes.push(req_node(A, Accepted, requirement(STATEMENT), &[&f[0]]));
        let other = Graph::new(id(PROJECT), id("profile:other"), nodes, Vec::new()).unwrap();
        assert!(matches!(
            analyze_requirement_duplicates(&other, &hr_policy()),
            Err(DuplicateError::InvalidInput { .. })
        ));
        assert_eq!(
            load_builtin_software_profile().unwrap().profile_id,
            id(PROFILE)
        );
    }

    /// Exact matches of `a` and `b` under the HR policy, or None.
    fn kind_of(a: &str, b: &str) -> Option<(DuplicateMatchKind, JaccardScore)> {
        let result = analyze(&pair(A, Proposed, B, Proposed, a, b));
        result.matches.first().map(|m| (m.match_kind, m.score))
    }

    #[test]
    fn duplicates_normalization() {
        // Golden tokens: NFKC (ﬁ ligature, decomposed é), lowercase, alphanumeric runs.
        let messy = "Employee SHALL Submit the ﬁrst-request: 2 days, Cafe\u{301}!";
        let clean = "employee shall submit the first request 2 days café";
        assert_eq!(
            kind_of(messy, clean),
            Some((
                DuplicateMatchKind::Exact,
                JaccardScore {
                    intersection: 9,
                    union: 9
                }
            ))
        );
        assert_eq!(
            kind_of("Employee SHALL Submit", "employee shall submit")
                .unwrap()
                .0,
            DuplicateMatchKind::Exact
        );
        assert_eq!(
            kind_of("submit-request now", "submit request now")
                .unwrap()
                .0,
            DuplicateMatchKind::Exact
        );
        assert_eq!(
            kind_of("Caf\u{e9} shall open", "Cafe\u{301} shall open")
                .unwrap()
                .0,
            DuplicateMatchKind::Exact
        );
        // Numbers stay tokens: 2 days vs 3 days is 4/6 < 0.8.
        assert_eq!(kind_of("leave of 2 days now", "leave of 3 days now"), None);
        // Stopwords stay tokens: removing "the" and "to" changes the score.
        assert_eq!(
            kind_of("send the report to the manager", "send report manager"),
            None,
            "3/5 with stopwords kept"
        );
        // Repeated tokens: equal sets but different sequences are Near with score 1.
        assert_eq!(
            kind_of("shall shall submit", "shall submit"),
            Some((
                DuplicateMatchKind::Near,
                JaccardScore {
                    intersection: 2,
                    union: 2
                }
            ))
        );
        // Punctuation-only statements match nothing.
        assert_eq!(kind_of("...", "?!"), None);
        assert_eq!(kind_of("...", "..."), None);
    }

    #[test]
    fn duplicates_jaccard_boundary_and_threshold_sensitivity() {
        let a = "alpha beta gamma delta";
        let b = "alpha beta gamma delta epsilon";
        let g = pair(A, Proposed, B, Proposed, a, b);
        // Golden: 4 shared tokens of 5.
        let at = analyze_requirement_duplicates(&g, &policy("0.8")).unwrap();
        let m = only_match(&at);
        assert_eq!(
            m.score,
            JaccardScore {
                intersection: 4,
                union: 5
            }
        );
        assert_eq!(
            m.score.as_decimal(),
            Some(Decimal::from_str("0.8").unwrap())
        );
        assert_eq!(m.match_kind, DuplicateMatchKind::Near);
        assert_eq!(m.merge_disposition, MergeDisposition::ReviewOnly);
        assert!(analyze_requirement_duplicates(&g, &policy("0.8000001"))
            .unwrap()
            .matches
            .is_empty());
        assert_eq!(
            analyze_requirement_duplicates(&g, &policy("0.5"))
                .unwrap()
                .matches
                .len(),
            1
        );
        // Below threshold: nothing at all.
        let below = analyze(&pair(
            A,
            Accepted,
            B,
            Accepted,
            "alpha beta gamma",
            "alpha beta delta",
        ));
        assert!(
            below.matches.is_empty() && below.findings.is_empty() && below.proposals.is_empty()
        );
    }

    #[test]
    fn duplicates_scope_compatibility() {
        let mutate: [&dyn Fn(&mut Requirement); 5] = [
            &|r| r.requirement_kind = RequirementKind::Security,
            &|r| r.level = RequirementLevel::Software,
            &|r| r.modality = Modality::Should,
            &|r| r.owner_refs = Some(vec![id("actor:owner")]),
            &|r| r.stakeholder_refs = Some(vec![id("stakeholder:x")]),
        ];
        for f in mutate {
            let g = with_requirements(|ev| {
                let mut b = requirement(STATEMENT);
                f(&mut b);
                vec![
                    req_node(A, Accepted, requirement(STATEMENT), &[&ev[0]]),
                    req_node(B, Accepted, b, &[&ev[1]]),
                ]
            });
            assert!(analyze(&g).matches.is_empty());
        }
        // None and Some([]) are both empty; order and repetition are ignored.
        let g = with_requirements(|ev| {
            let mut a = requirement(STATEMENT);
            a.owner_refs = Some(vec![]);
            a.stakeholder_refs = Some(vec![id("stakeholder:y"), id("stakeholder:x")]);
            let mut b = requirement(STATEMENT);
            b.stakeholder_refs = Some(vec![
                id("stakeholder:x"),
                id("stakeholder:y"),
                id("stakeholder:x"),
            ]);
            vec![
                req_node(A, Accepted, a, &[&ev[0]]),
                req_node(B, Accepted, b, &[&ev[1]]),
            ]
        });
        let result = analyze(&g);
        assert_eq!(only_match(&result).match_kind, DuplicateMatchKind::Exact);
        assert_eq!(
            only_match(&result).merge_disposition,
            MergeDisposition::UnavailablePayloadDifference
        );
    }

    #[test]
    fn duplicates_metadata_does_not_prevent_detection() {
        let g = with_requirements(|ev| {
            let mut a = requirement(STATEMENT);
            a.source_identifier = Some("REQ-1".to_owned());
            a.title = Some("Export".to_owned());
            let mut b = requirement(STATEMENT);
            b.source_identifier = Some("REQ-2".to_owned());
            b.priority = Some("high".to_owned());
            vec![
                req_node(A, Accepted, a, &[&ev[0]]),
                req_node(B, Accepted, b, &[&ev[1]]),
            ]
        });
        let result = analyze(&g);
        let m = only_match(&result);
        assert_eq!(m.match_kind, DuplicateMatchKind::Exact);
        assert_eq!(
            m.merge_disposition,
            MergeDisposition::UnavailablePayloadDifference
        );
        assert!(result.proposals.is_empty());
        assert_eq!(result.findings.len(), 1);
    }

    #[test]
    fn duplicates_status_pairs() {
        for (sa, sb, matched, finding) in [
            (Proposed, Proposed, true, false),
            (Proposed, Accepted, true, false),
            (Accepted, Accepted, true, true),
            (Accepted, ElementStatus::Superseded, false, false),
            (Accepted, ElementStatus::Suspect, false, false),
            (Accepted, ElementStatus::Rejected, false, false),
            (Accepted, ElementStatus::Deprecated, false, false),
        ] {
            let result = analyze(&pair(A, sa, B, sb, STATEMENT, STATEMENT));
            assert_eq!(result.matches.len(), usize::from(matched), "{sa:?}/{sb:?}");
            assert_eq!(result.findings.len(), usize::from(finding), "{sa:?}/{sb:?}");
        }
    }

    #[test]
    fn duplicates_current_statement_is_compared() {
        // Different source evidence, equal current (e.g. accepted EARS) statements.
        let (mut nodes, _) = evidence(1);
        let audit_import = ImportAudit {
            created_by: id("actor:importer"),
            created_at: ts(AT),
        };
        let other = import_plain_text(
            "other.txt",
            b"Totally different source wording.\n\nAnother unrelated paragraph.",
            &audit_import,
        )
        .unwrap();
        let ev: Vec<Id> = other.fragments.iter().map(|f| f.id.clone()).collect();
        nodes.push(other.source);
        nodes.extend(other.fragments);
        nodes.push(req_node(
            A,
            Proposed,
            requirement("When an order arrives, the clerk shall pack it."),
            &[&ev[0]],
        ));
        nodes.push(req_node(
            B,
            Proposed,
            requirement("When an order arrives, the clerk shall pack it."),
            &[&ev[1]],
        ));
        let result = analyze(&graph(nodes, Vec::new()));
        assert_eq!(only_match(&result).match_kind, DuplicateMatchKind::Exact);
    }

    // ------------------------------------------------------------------ findings

    #[test]
    fn duplicates_finding_golden_and_metadata() {
        let result = analyze(&pair(A, Accepted, B, Accepted, STATEMENT, STATEMENT));
        assert_eq!(result.findings.len(), 1);
        let finding: &GeneratedFinding = &result.findings[0];
        assert_eq!(finding.key.as_str(), FINDING_KEY);
        assert_eq!(finding.id.as_str(), FINDING_ID);
        assert_eq!(
            finding.semantic_condition_key,
            "active_requirement_duplicate"
        );
        let profile = load_builtin_software_profile().unwrap();
        let rule = profile
            .rules
            .iter()
            .find(|r| r.id == "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED")
            .unwrap();
        assert_eq!(
            finding.payload,
            Finding {
                code: rule.id.clone(),
                family: rule.gate.as_str().to_owned(),
                severity: FindingSeverity::Blocker,
                message: format!("Accepted requirements {A} and {B} have identical normalized obligation text."),
                status: rule.finding_status_on_fail.clone(),
                affected_refs: vec![id(A), id(B)],
                standard_rule_ref: None,
                suggested_resolution: Some(
                    "Review the pair and either merge a lossless exact duplicate, supersede a replaced requirement through a governed decision, or resolve/waive the finding as intentionally distinct."
                        .to_owned()
                ),
                waiver_ref: None,
            }
        );
        assert_eq!(finding.payload.family, "F1");
        assert_eq!(finding.payload.status, "Open");
        let near = analyze(&pair(
            A,
            Accepted,
            B,
            Accepted,
            "alpha beta gamma delta",
            "alpha beta gamma delta epsilon",
        ));
        assert_eq!(
            near.findings[0].payload.message,
            format!("Accepted requirements {A} and {B} meet the configured duplicate Jaccard threshold (4/5).")
        );
        assert_eq!(
            near.findings[0].id.as_str(),
            FINDING_ID,
            "same pair, same condition key"
        );
    }

    // ------------------------------------------------------------------ merge

    #[test]
    fn duplicates_exact_merge_fixture() {
        // Synthetic exact duplicate fixture: identical payloads, different evidence, no
        // extensions, edges or references.
        let g = with_requirements(|f| {
            vec![
                req_node(A, Accepted, requirement(STATEMENT), &[&f[0], &f[2]]),
                req_node(B, Accepted, requirement(STATEMENT), &[&f[1], &f[2]]),
            ]
        });
        let result = analyze(&g);
        let m = only_match(&result);
        assert_eq!((m.left_ref.as_str(), m.right_ref.as_str()), (A, B));
        assert_eq!(m.match_kind, DuplicateMatchKind::Exact);
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.proposals.len(), 1);
        let DuplicateResolutionProposal::Merge {
            left_ref,
            right_ref,
            proposal,
        } = &result.proposals[0]
        else {
            panic!("expected merge");
        };
        assert_eq!((left_ref.as_str(), right_ref.as_str()), (A, B));
        assert_eq!(
            m.merge_disposition,
            MergeDisposition::MergeProposed {
                proposal_ref: proposal.id.clone()
            }
        );
        assert_eq!(proposal.stage, StageId::S1);
        assert_eq!(proposal.materiality, ProposalMateriality::MaterialDecision);
        assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanDecision);
        assert_eq!(proposal.confidence, None);
        assert!(proposal.derivation_refs.is_empty());
        assert_eq!(
            proposal.patch_set.base_semantic_hash,
            g.semantic_hash().unwrap()
        );
        let a = g.node(&id(A)).unwrap();
        let b = g.node(&id(B)).unwrap();
        let mut union: Vec<EvidenceRef> = a
            .evidence
            .iter()
            .chain(&b.evidence)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        union.sort();
        assert_eq!(union.len(), 3);
        assert_eq!(proposal.evidence_refs, union);
        let SemanticPatch::MergeNodes {
            keep,
            merge,
            field_policy,
        } = &proposal.patch_set.patch
        else {
            panic!("expected MergeNodes");
        };
        assert_eq!(
            (keep.id.as_str(), keep.expected_hash.clone()),
            (A, node_element_hash(a).unwrap())
        );
        assert_eq!(merge.len(), 1);
        assert_eq!(
            (merge[0].id.as_str(), merge[0].expected_hash.clone()),
            (B, node_element_hash(b).unwrap())
        );
        assert_eq!(*field_policy, MergePolicy::KeepPayloadUnionMetadata);
        // Test-only acceptance: one Requirement remains with the keep payload and all evidence.
        let merged = apply_patch(&g, &proposal.patch_set).unwrap().graph;
        assert!(merged.node(&id(B)).is_none());
        let kept = merged.node(&id(A)).unwrap();
        assert_eq!(kept.payload, a.payload);
        assert_eq!(kept.evidence, union);
        let after = analyze(&merged);
        assert!(after.matches.is_empty() && after.findings.is_empty());
    }

    #[test]
    fn duplicates_keep_node_policy() {
        // Accepted is kept even when its ID is larger.
        let result = analyze(&pair(A, Proposed, B, Accepted, STATEMENT, STATEMENT));
        let DuplicateResolutionProposal::Merge { proposal, .. } = &result.proposals[0] else {
            panic!()
        };
        let SemanticPatch::MergeNodes { keep, merge, .. } = &proposal.patch_set.patch else {
            panic!()
        };
        assert_eq!((keep.id.as_str(), merge[0].id.as_str()), (B, A));
        assert!(result.findings.is_empty());
        // Same status: the smaller ID is kept.
        for status in [Proposed, Accepted] {
            let result = analyze(&pair(A, status, B, status, STATEMENT, STATEMENT));
            let DuplicateResolutionProposal::Merge { proposal, .. } = &result.proposals[0] else {
                panic!()
            };
            let SemanticPatch::MergeNodes { keep, .. } = &proposal.patch_set.patch else {
                panic!()
            };
            assert_eq!(keep.id.as_str(), A);
            assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanDecision);
        }
    }

    #[test]
    fn duplicates_segment_origin_conflict_blocks_merge() {
        let origin = |f: &Id, end: u64| json!({"candidate_ref": "seg:0000000000000001", "fragment_ref": f, "start": 0, "end": end});
        let g = with_requirements(|f| {
            let mut a = req_node(A, Accepted, requirement(STATEMENT), &[&f[0]]);
            let mut b = req_node(B, Accepted, requirement(STATEMENT), &[&f[1]]);
            a.extensions
                .insert(SEGMENT_ORIGIN_EXTENSION.parse().unwrap(), origin(&f[0], 5));
            b.extensions
                .insert(SEGMENT_ORIGIN_EXTENSION.parse().unwrap(), origin(&f[1], 6));
            vec![a, b]
        });
        let before: Vec<Node> = g.nodes().values().cloned().collect();
        let result = analyze(&g);
        let m = only_match(&result);
        assert_eq!(m.match_kind, DuplicateMatchKind::Exact);
        assert_eq!(
            m.merge_disposition,
            MergeDisposition::UnavailableExtensionConflict {
                key: "plumb_functional:segment_origin".to_owned()
            }
        );
        assert!(result.proposals.is_empty());
        assert_eq!(result.findings.len(), 1);
        // Provenance untouched.
        assert_eq!(g.nodes().values().cloned().collect::<Vec<_>>(), before);
    }

    #[test]
    fn duplicates_merge_blocked_by_edges_and_references() {
        // B (the merge target) has an incident refines edge.
        let (mut nodes, f) = evidence(4);
        nodes.push(req_node(A, Accepted, requirement(STATEMENT), &[&f[0]]));
        nodes.push(req_node(B, Accepted, requirement(STATEMENT), &[&f[1]]));
        nodes.push(req_node(
            C,
            Accepted,
            requirement("An unrelated obligation entirely."),
            &[&f[2]],
        ));
        let g = graph(
            nodes.clone(),
            vec![edge(
                "rel:00000000000000e1",
                RelationKind::Refines,
                B,
                C,
                Accepted,
            )],
        );
        let result = analyze(&g);
        assert!(matches!(
            only_match(&result).merge_disposition,
            MergeDisposition::UnavailablePatchConstraint { .. }
        ));
        assert!(result.proposals.is_empty());
        assert_eq!(g.edges().len(), 1, "no rewiring");
        // Another node references B by ID.
        let mut referencing = nodes;
        referencing.push(node(
            "fnd:00000000000000f1",
            Proposed,
            NodePayload::Finding(Finding {
                code: "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED".to_owned(),
                family: "F1".to_owned(),
                severity: FindingSeverity::Blocker,
                message: "m".to_owned(),
                status: "Open".to_owned(),
                affected_refs: vec![id(B)],
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            }),
            &[],
        ));
        let result = analyze(&graph(referencing, Vec::new()));
        assert!(matches!(
            only_match(&result).merge_disposition,
            MergeDisposition::UnavailablePatchConstraint { .. }
        ));
        assert!(result.proposals.is_empty());
    }

    #[test]
    fn duplicates_near_match_is_review_only() {
        let result = analyze(&pair(
            A,
            Accepted,
            B,
            Accepted,
            "alpha beta gamma delta",
            "alpha beta gamma delta epsilon",
        ));
        assert_eq!(
            only_match(&result).merge_disposition,
            MergeDisposition::ReviewOnly
        );
        assert!(result.proposals.is_empty());
        assert_eq!(result.findings.len(), 1);
    }

    // ------------------------------------------------------------------ supersession

    const OLD: &str = "req:0000000000000b01";
    const NEW: &str = "req:0000000000000b02";
    const DECISION: &str = "dec:00000000000000d1";
    const FINDING: &str = "fnd:00000000000000f9";

    struct Governance {
        decision_status: ElementStatus,
        answer: Value,
        rationale: Option<&'static str>,
        edge_status: ElementStatus,
        target: NodePayload,
        target_status: ElementStatus,
    }

    impl Default for Governance {
        fn default() -> Self {
            Governance {
                decision_status: Accepted,
                answer: json!({"kind": "requirement_supersession", "old_ref": OLD, "new_ref": NEW}),
                rationale: Some("Policy v2 replaces the export obligation."),
                edge_status: Accepted,
                target: NodePayload::Finding(Finding {
                    code: "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED".to_owned(),
                    family: "F1".to_owned(),
                    severity: FindingSeverity::Blocker,
                    message: "Possible replacement.".to_owned(),
                    status: "Open".to_owned(),
                    affected_refs: vec![id(OLD), id(NEW)],
                    standard_rule_ref: None,
                    suggested_resolution: None,
                    waiver_ref: None,
                }),
                target_status: Accepted,
            }
        }
    }

    fn decision_node(
        node_id: &str,
        status: ElementStatus,
        answer: Value,
        rationale: Option<&str>,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::ResolutionDecision(ResolutionDecision {
                question_ref: None,
                proposal_ref: Some(id("prop:0000000000000001")),
                answer,
                decided_by: id("actor:architect"),
                decided_at: ts("2026-03-04T05:06:07.000000000Z"),
                patch_ref: Hash::content_sha256(b"patch"),
                rationale: rationale.map(str::to_owned),
                supersedes: None,
            }),
            &[],
        )
    }

    /// Accepted OLD/NEW requirements with dissimilar wording plus a governed decision.
    fn supersession_graph(
        g: Governance,
        old_status: ElementStatus,
        new_status: ElementStatus,
        extra_edges: Vec<Edge>,
    ) -> Graph {
        let (mut nodes, f) = evidence(4);
        nodes.push(req_node(
            OLD,
            old_status,
            requirement("The system shall export the report as CSV."),
            &[&f[0]],
        ));
        nodes.push(req_node(
            NEW,
            new_status,
            requirement("Monthly statements must be delivered as signed PDF archives."),
            &[&f[1], &f[0]],
        ));
        nodes.push(node(FINDING, g.target_status, g.target, &[]));
        nodes.push(decision_node(
            DECISION,
            g.decision_status,
            g.answer,
            g.rationale,
        ));
        let mut edges = vec![edge(
            "rel:00000000000000c1",
            RelationKind::Resolves,
            DECISION,
            FINDING,
            g.edge_status,
        )];
        edges.extend(extra_edges);
        graph(nodes, edges)
    }

    fn supersede(g: &Graph) -> Result<DuplicateAnalysisResult, DuplicateError> {
        analyze_requirement_duplicates(g, &hr_policy())
    }

    #[test]
    fn duplicates_supersession_proposal() {
        let g = supersession_graph(Governance::default(), Accepted, Accepted, vec![]);
        let result = supersede(&g).unwrap();
        assert!(
            result.matches.is_empty(),
            "lexically dissimilar pair is not a duplicate"
        );
        assert_eq!(result.proposals.len(), 1);
        let DuplicateResolutionProposal::Supersede {
            old_ref,
            new_ref,
            decision_ref,
            proposal,
        } = &result.proposals[0]
        else {
            panic!("expected supersede");
        };
        assert_eq!(
            (old_ref.as_str(), new_ref.as_str(), decision_ref.as_str()),
            (OLD, NEW, DECISION)
        );
        assert_eq!(proposal.materiality, ProposalMateriality::MaterialDecision);
        assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanDecision);
        assert_eq!(proposal.confidence, None);
        assert!(proposal.derivation_refs.is_empty());
        assert_eq!(
            proposal.patch_set.base_semantic_hash,
            g.semantic_hash().unwrap()
        );
        let old = g.node(&id(OLD)).unwrap();
        let new = g.node(&id(NEW)).unwrap();
        let union: Vec<EvidenceRef> = old
            .evidence
            .iter()
            .chain(&new.evidence)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(union.len(), 2);
        assert_eq!(proposal.evidence_refs, union);
        let SemanticPatch::Supersede {
            old: po,
            new: pn,
            edge,
        } = &proposal.patch_set.patch
        else {
            panic!("expected Supersede");
        };
        assert_eq!(
            (po.id.as_str(), po.expected_hash.clone()),
            (OLD, node_element_hash(old).unwrap())
        );
        assert_eq!(
            (pn.id.as_str(), pn.expected_hash.clone()),
            (NEW, node_element_hash(new).unwrap())
        );
        assert_eq!(edge.id.as_str(), SUPERSEDES_EDGE_ID);
        assert_eq!(edge.id.as_str(), {
            let digest = Hash::content_sha256(
                &to_canonical_json(&json!({"kind": "supersedes", "from": NEW, "to": OLD})).unwrap(),
            );
            format!("rel:{}", &digest.as_str()[7..23])
        });
        assert_eq!(edge.revision, 1);
        assert_eq!(edge.status, Accepted);
        assert_eq!(edge.kind, RelationKind::Supersedes);
        assert_eq!((edge.from.as_str(), edge.to.as_str()), (NEW, OLD));
        assert_eq!(edge.properties, RelationProperties::None);
        assert_eq!(edge.evidence, union);
        assert!(edge.derivations.is_empty() && edge.standards.is_empty());
        assert_eq!(
            edge.audit,
            AuditMeta::new(
                id("actor:architect"),
                ts("2026-03-04T05:06:07.000000000Z"),
                None,
                None
            )
            .unwrap()
        );
        // Test-only application: old Superseded, new unchanged, edge added.
        let applied = apply_patch(&g, &proposal.patch_set).unwrap().graph;
        assert_eq!(
            applied.node(&id(OLD)).unwrap().status,
            ElementStatus::Superseded
        );
        assert_eq!(applied.node(&id(NEW)).unwrap(), new);
        assert_eq!(applied.edge(&edge.id), Some(edge));
        // The carried-out decision produces nothing further.
        assert!(supersede(&applied).unwrap().proposals.is_empty());
    }

    #[test]
    fn duplicates_supersession_takes_precedence_over_merge() {
        let (mut nodes, f) = evidence(4);
        nodes.push(req_node(OLD, Accepted, requirement(STATEMENT), &[&f[0]]));
        nodes.push(req_node(NEW, Accepted, requirement(STATEMENT), &[&f[1]]));
        let gov = Governance::default();
        nodes.push(node(FINDING, Accepted, gov.target, &[]));
        nodes.push(decision_node(DECISION, Accepted, gov.answer, gov.rationale));
        let g = graph(
            nodes,
            vec![edge(
                "rel:00000000000000c1",
                RelationKind::Resolves,
                DECISION,
                FINDING,
                Accepted,
            )],
        );
        let result = supersede(&g).unwrap();
        assert_eq!(result.proposals.len(), 1);
        let DuplicateResolutionProposal::Supersede { proposal, .. } = &result.proposals[0] else {
            panic!()
        };
        assert_eq!(
            only_match(&result).merge_disposition,
            MergeDisposition::SupersessionDirected {
                decision_ref: id(DECISION),
                proposal_ref: proposal.id.clone()
            }
        );
        assert_eq!(result.findings.len(), 1);
    }

    #[test]
    fn duplicates_supersession_question_governance() {
        let question = NodePayload::Question(plumb_psg::Question {
            finding_ref: id(FINDING),
            question_kind: serde_json::from_value(json!("PickOne")).unwrap(),
            prompt: "Which requirement replaces the other?".to_owned(),
            status: "Answered".to_owned(),
            answer_schema: None,
            stakeholder_ref: None,
            priority: None,
            round_ref: None,
            context_refs: None,
        });
        let g = supersession_graph(
            Governance {
                target: question,
                ..Governance::default()
            },
            Accepted,
            Accepted,
            vec![],
        );
        assert_eq!(supersede(&g).unwrap().proposals.len(), 1);
    }

    fn invalid(g: &Graph) -> bool {
        matches!(
            supersede(g),
            Err(DuplicateError::InvalidSupersessionDecision { .. })
        )
    }

    #[test]
    fn duplicates_supersession_marker_validation() {
        let with = |answer: Value| {
            supersession_graph(
                Governance {
                    answer,
                    ..Governance::default()
                },
                Accepted,
                Accepted,
                vec![],
            )
        };
        assert!(invalid(&with(
            json!({"kind": "requirement_supersession", "old_ref": OLD})
        )));
        assert!(invalid(&with(
            json!({"kind": "requirement_supersession", "new_ref": NEW})
        )));
        assert!(invalid(&with(
            json!({"kind": "requirement_supersession", "old_ref": OLD, "new_ref": NEW, "extra": 1})
        )));
        assert!(invalid(&with(
            json!({"kind": "requirement_supersession", "old_ref": OLD, "new_ref": OLD})
        )));
        assert!(invalid(&with(
            json!({"kind": "requirement_supersession", "old_ref": "not an id", "new_ref": NEW})
        )));
        // Unrelated or wrong-case kinds are ignored.
        for answer in [
            json!({"kind": "Requirement_Supersession", "old_ref": OLD, "new_ref": NEW}),
            json!({"kind": "waiver", "rule_id": "x"}),
            json!({"kind": "cardinality"}),
            json!("free text"),
        ] {
            let result = supersede(&with(answer)).unwrap();
            assert!(result.proposals.is_empty());
        }
    }

    #[test]
    fn duplicates_supersession_governance_requirements() {
        for rationale in [
            None,
            Some(""),
            Some("   "),
            Some(" padded"),
            Some("line\nbreak"),
        ] {
            let g = supersession_graph(
                Governance {
                    rationale,
                    ..Governance::default()
                },
                Accepted,
                Accepted,
                vec![],
            );
            assert!(invalid(&g), "{rationale:?}");
        }
        // An Accepted decision without an Accepted resolves edge is structurally invalid
        // (resolves outgoing cardinality 1..*), so the reachable ungoverned case is an edge
        // to a baseline but non-Accepted (Suspect) Finding.
        let ungoverned_target = supersession_graph(
            Governance {
                target_status: ElementStatus::Suspect,
                ..Governance::default()
            },
            Accepted,
            Accepted,
            vec![],
        );
        assert!(invalid(&ungoverned_target));
        for status in [
            Proposed,
            ElementStatus::Suspect,
            ElementStatus::Rejected,
            ElementStatus::Superseded,
            ElementStatus::Deprecated,
        ] {
            let g = supersession_graph(
                Governance {
                    decision_status: status,
                    answer: json!({"kind": "requirement_supersession", "broken": true}),
                    // A baseline edge needs baseline endpoints.
                    edge_status: if plumb_psg::is_baseline(status) {
                        Accepted
                    } else {
                        Proposed
                    },
                    ..Governance::default()
                },
                Accepted,
                Accepted,
                vec![],
            );
            assert!(supersede(&g).unwrap().proposals.is_empty(), "{status:?}");
        }
        assert!(invalid(&supersession_graph(
            Governance::default(),
            Accepted,
            Proposed,
            vec![]
        )));
        assert!(invalid(&supersession_graph(
            Governance::default(),
            Proposed,
            Accepted,
            vec![]
        )));
        // Already carried out: no new proposal.
        let done = supersession_graph(
            Governance::default(),
            ElementStatus::Superseded,
            Accepted,
            vec![],
        );
        assert!(supersede(&done).unwrap().proposals.is_empty());
    }

    #[test]
    fn duplicates_supersession_ambiguity() {
        let (mut nodes, f) = evidence(4);
        let third = "req:0000000000000b03";
        nodes.push(req_node(
            OLD,
            Accepted,
            requirement("Old wording here."),
            &[&f[0]],
        ));
        nodes.push(req_node(
            NEW,
            Accepted,
            requirement("New wording entirely."),
            &[&f[1]],
        ));
        nodes.push(req_node(
            third,
            Accepted,
            requirement("Third wording variant."),
            &[&f[2]],
        ));
        let gov = Governance::default();
        nodes.push(node(FINDING, Accepted, gov.target, &[]));
        let rationale = Some("Replacement decided.");
        let second = "dec:00000000000000d2";
        let build = |nodes: &Vec<Node>, other_new: &str| {
            let mut nodes = nodes.clone();
            nodes.push(decision_node(
                DECISION,
                Accepted,
                json!({"kind": "requirement_supersession", "old_ref": OLD, "new_ref": NEW}),
                rationale,
            ));
            nodes.push(decision_node(
                second,
                Accepted,
                json!({"kind": "requirement_supersession", "old_ref": OLD, "new_ref": other_new}),
                rationale,
            ));
            graph(
                nodes,
                vec![
                    edge(
                        "rel:00000000000000c1",
                        RelationKind::Resolves,
                        DECISION,
                        FINDING,
                        Accepted,
                    ),
                    edge(
                        "rel:00000000000000c2",
                        RelationKind::Resolves,
                        second,
                        FINDING,
                        Accepted,
                    ),
                ],
            )
        };
        for other_new in [third, NEW] {
            assert!(matches!(
                supersede(&build(&nodes, other_new)),
                Err(DuplicateError::AmbiguousSupersessionDecision { .. })
            ));
        }
    }

    #[test]
    fn duplicates_supersession_cycle_is_invalid() {
        let existing = edge(
            "rel:00000000000000c9",
            RelationKind::Supersedes,
            OLD,
            NEW,
            Accepted,
        );
        let g = supersession_graph(Governance::default(), Accepted, Accepted, vec![existing]);
        assert!(invalid(&g));
    }

    // ------------------------------------------------------------------ determinism

    #[test]
    fn duplicates_result_is_order_independent() {
        let build = |reverse: bool| {
            let (mut nodes, f) = evidence(4);
            nodes.push(req_node(A, Accepted, requirement(STATEMENT), &[&f[0]]));
            nodes.push(req_node(B, Accepted, requirement(STATEMENT), &[&f[1]]));
            nodes.push(req_node(
                C,
                Proposed,
                requirement("The system shall export the report now."),
                &[&f[2]],
            ));
            if reverse {
                nodes.reverse();
            }
            graph(nodes, Vec::new())
        };
        let forward = analyze(&build(false));
        let reversed = analyze(&build(true));
        assert_eq!(forward, reversed);
        assert_eq!(
            serde_json::to_vec(&forward).unwrap(),
            serde_json::to_vec(&reversed).unwrap()
        );
        assert!(forward
            .matches
            .windows(2)
            .all(|p| (&p[0].left_ref, &p[0].right_ref) < (&p[1].left_ref, &p[1].right_ref)));
        assert!(forward.matches.iter().all(|m| m.left_ref < m.right_ref));
        assert!(forward.findings.windows(2).all(|p| p[0].id < p[1].id));
    }

    #[test]
    fn duplicates_production_source_guard() {
        let source = include_str!("../src/duplicates.rs");
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
            "InferenceRequest",
            "MockProvider",
            "RevisionStore",
            "commit(",
            "unsafe",
            "HR-0",
            "leave",
            "0.8",
            "requirements.md",
            "requirements.docx",
            "profile.yaml",
        ] {
            assert!(!source.contains(token), "{token}");
        }
        assert_eq!(
            source.matches("apply_patch(").count(),
            2,
            "dry-validation only"
        );
    }

    // ------------------------------------------------------------------ HR integration

    fn policy_mock() -> ProviderPolicy {
        ProviderPolicy {
            provider: "mock".to_owned(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn artifact(request: &InferenceRequest, output: Value) -> InferenceArtifact {
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

    /// Both HR sources through S0.4 fallback segmentation and S1.1 compilation (mock
    /// classifier: null kinds, fixed level `software`, which is mock output and not an
    /// authoritative HR level), every Requirement AddNode applied in test code.
    fn hr_graph() -> Graph {
        let audit = ImportAudit {
            created_by: id("actor:importer"),
            created_at: ts(AT),
        };
        let md = import_markdown("requirements.md", HR_MD, &audit).unwrap();
        let docx = import_docx("requirements.docx", HR_DOCX, &audit).unwrap();
        let mut nodes = Vec::new();
        let mut candidates = Vec::new();
        for imported in [md, docx] {
            let fragments = imported.fragments.clone();
            let request = build_segmentation_request(&fragments, policy_mock()).unwrap();
            let segmentation_audit = SegmentationAudit {
                created_by: id("agent:segmenter"),
                created_at: ts(AT),
            };
            candidates.extend(
                evaluate_segmentation(&request, &fragments, None, &segmentation_audit)
                    .unwrap()
                    .candidates,
            );
            nodes.push(imported.source);
            nodes.extend(imported.fragments);
        }
        let mut g = graph(nodes, Vec::new());
        let request =
            build_requirement_classification_request(&g, &candidates, policy_mock()).unwrap();
        assert_eq!(request.context.candidates.len(), 64);
        let entries: Vec<Value> = request
            .context
            .candidates
            .iter()
            .map(|c| json!({"candidate_ref": c.candidate_ref, "requirement_kind": null, "level": "software"}))
            .collect();
        let classification = artifact(
            &request.request,
            json!({"version": 1, "classifications": entries, "intents": []}),
        );
        let compiled = compile_requirement_candidates(
            &g,
            &candidates,
            &request,
            Some(RequirementClassificationInference {
                artifact: &classification,
                derivation_ref: DerivationRef::from(id("drv:00000000000000c1")),
            }),
            &RequirementCompilationAudit {
                created_by: id("agent:requirements-compiler"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        assert_eq!(compiled.proposals.len(), 64);
        for proposal in &compiled.proposals {
            g = apply_patch(&g, &proposal.patch_set).unwrap().graph;
        }
        g
    }

    /// The (Markdown, DOCX) Requirement IDs per HR source identifier.
    fn hr_pairs(g: &Graph) -> BTreeMap<String, BTreeSet<Id>> {
        let mut pairs: BTreeMap<String, BTreeSet<Id>> = BTreeMap::new();
        for node in g.nodes().values() {
            if let NodePayload::Requirement(r) = &node.payload {
                pairs
                    .entry(r.source_identifier.clone().unwrap())
                    .or_default()
                    .insert(node.id.clone());
            }
        }
        pairs
    }

    #[test]
    fn duplicates_hr_cross_format_pairs() {
        let g = hr_graph();
        let pairs = hr_pairs(&g);
        assert_eq!(pairs.len(), 32);
        let check = |g: &Graph, accepted: bool| {
            let result = analyze(g);
            let mut detected = 0;
            for ids in pairs.values() {
                let ids: Vec<&Id> = ids.iter().collect();
                assert_eq!(ids.len(), 2);
                let m = result
                    .matches
                    .iter()
                    .find(|m| &m.left_ref == ids[0] && &m.right_ref == ids[1])
                    .expect("corresponding pair detected");
                assert_eq!(m.match_kind, DuplicateMatchKind::Exact);
                assert_eq!(
                    m.merge_disposition,
                    MergeDisposition::UnavailableExtensionConflict {
                        key: "plumb_functional:segment_origin".to_owned()
                    }
                );
                let has_finding = result
                    .findings
                    .iter()
                    .any(|f| f.payload.affected_refs == vec![ids[0].clone(), ids[1].clone()]);
                assert_eq!(has_finding, accepted);
                detected += 1;
            }
            assert!(
                result.proposals.is_empty(),
                "no merge proposals for distinct origins"
            );
            if !accepted {
                assert!(result.findings.is_empty());
            }
            detected
        };
        assert_eq!(check(&g, false), 32);
        let accepted_nodes: Vec<Node> = g
            .nodes()
            .values()
            .cloned()
            .map(|mut n| {
                if matches!(n.payload, NodePayload::Requirement(_)) {
                    n.status = Accepted;
                }
                n
            })
            .collect();
        let accepted = graph(accepted_nodes, Vec::new());
        assert_eq!(check(&accepted, true), 32);
    }
}
