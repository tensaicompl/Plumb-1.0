//! X1.3 contract tests for patch-centric intake analysis and the acceptance-rationale guard
//! (Hotfix 059, Hotfix 059a).
//!
//! Every test lives in `intake` (`cargo test -p plumb-intake intake`). Fixtures are hand-built
//! typed PSGs; the validation regression runs the real F1 and I0 evaluators over the dry-run
//! candidate graph.

mod intake {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{canonical_hash, GateId, Hash, HashKind, Id, StageId, Timestamp};
    use plumb_intake::{
        intake, tokenize, validate_required_rationale, CoverageMatch, DeclaredConflict,
        DuplicateCandidate, IntakeAcceptanceError, IntakeError, IntakeReasonCode, IntakeReport,
        IntakeReportError, IntakeValidationInput, LexicalIndex, RefinementLink,
        RetrievalDocumentKind, SubjectNovelty, TermResolution, TermResolutionStatus,
        INTAKE_DUPLICATE_CANDIDATE_LIMIT,
    };
    use plumb_patch::{
        apply_patch, compute_impact, AcceptancePolicy, ElementPrecondition, PatchError, PatchSet,
        Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        edge_element_hash, node_element_hash, AuditMeta, Concept, ConceptKind, Edge, ElementStatus,
        Finding, FindingSeverity, Graph, Modality, Node, NodePayload, RelationKind,
        RelationProperties, Requirement, RequirementKind, RequirementLevel, ResolutionDecision,
        Term,
    };
    use plumb_psg::{
        Calculation, Invariant, PolicyCondition, ProcessNode, ProcessNodeKind, Rule, RuleKind,
        Transition,
    };
    use plumb_validation::{
        load_builtin_software_profile, rules, EvaluatorRegistry, F1RequirementValidationInput,
        F1ValidationInputs, GateReport, GeneratedFinding, ValidationContext, ValidationPolicy,
        ValidationRegistry,
    };
    use serde_json::json;

    use ElementStatus::{Accepted, Deprecated, Proposed, Rejected, Superseded, Suspect};
    use IntakeReasonCode::{
        AmbiguousTerm, DeclaredConflict as DeclaredConflictReason, ExactEquivalent, HumanDecision,
        MaterialDecision, ValidationConflict,
    };

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const NO_CONTRADICTION: &str = "PLUMB.F1.REQ.NO_CONTRADICTION";
    const INTAKE_SRC: &str = include_str!("../src/intake.rs");
    const REPORT_SRC: &str = include_str!("../src/report.rs");
    const LIB_SRC: &str = include_str!("../src/lib.rs");
    const INDEX_SRC: &str = include_str!("../src/index.rs");

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

    fn requirement_payload(statement: &str, modality: Modality) -> Requirement {
        Requirement {
            statement: statement.into(),
            requirement_kind: RequirementKind::Functional,
            level: RequirementLevel::System,
            modality,
            title: None,
            rationale: None,
            priority: None,
            source_identifier: None,
            verification_method: None,
            owner_refs: None,
            stakeholder_refs: None,
        }
    }

    fn req_with(node_id: &str, status: ElementStatus, r: Requirement) -> Node {
        node(node_id, status, NodePayload::Requirement(r))
    }

    fn req(node_id: &str, status: ElementStatus, statement: &str) -> Node {
        req_with(
            node_id,
            status,
            requirement_payload(statement, Modality::Shall),
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

    fn term(node_id: &str, status: ElementStatus, name: &str, aliases: &[&str]) -> Node {
        term_defined(node_id, status, name, aliases, None)
    }

    fn term_defined(
        node_id: &str,
        status: ElementStatus,
        name: &str,
        aliases: &[&str],
        definition_ref: Option<&str>,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Term(Term {
                term: name.into(),
                language: "en".into(),
                definition_ref: definition_ref.map(id),
                aliases: (!aliases.is_empty())
                    .then(|| aliases.iter().map(|a| (*a).to_owned()).collect()),
                status: None,
            }),
        )
    }

    fn finding_node(node_id: &str, code: &str, affected: &[&str]) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Finding(Finding {
                code: code.into(),
                family: "F1".into(),
                severity: FindingSeverity::Blocker,
                message: "Requirements contradict each other.".into(),
                status: "Open".into(),
                affected_refs: affected.iter().map(|a| id(a)).collect(),
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            }),
        )
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
            audit: AuditMeta::new(id("agent:analyst"), ts(AT), None, None).unwrap(),
        }
    }

    fn graph_with(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn graph(nodes: Vec<Node>) -> Graph {
        graph_with(nodes, Vec::new())
    }

    fn pre(g: &Graph, element: &str) -> ElementPrecondition {
        let expected_hash = match g.node(&id(element)) {
            Some(n) => node_element_hash(n).unwrap(),
            None => edge_element_hash(g.edge(&id(element)).unwrap()).unwrap(),
        };
        ElementPrecondition {
            id: id(element),
            expected_hash,
        }
    }

    fn add(n: Node) -> SemanticPatch {
        SemanticPatch::AddNode { node: n }
    }

    fn add_edge(e: Edge) -> SemanticPatch {
        SemanticPatch::AddEdge { edge: e }
    }

    fn compound(patches: Vec<SemanticPatch>) -> SemanticPatch {
        SemanticPatch::Compound { patches }
    }

    fn replace_payload(g: &Graph, target: &str, payload: NodePayload) -> SemanticPatch {
        SemanticPatch::ReplacePayload {
            target: pre(g, target),
            payload,
        }
    }

    fn set_status(
        g: &Graph,
        target: &str,
        from: ElementStatus,
        to: ElementStatus,
    ) -> SemanticPatch {
        SemanticPatch::SetStatus {
            target: pre(g, target),
            from,
            to,
        }
    }

    fn proposal_as(
        g: &Graph,
        patch: SemanticPatch,
        materiality: ProposalMateriality,
        policy: AcceptancePolicy,
    ) -> Proposal {
        Proposal::new(
            StageId::S1,
            PatchSet {
                base_semantic_hash: g.semantic_hash().unwrap(),
                patch,
            },
            Vec::new(),
            Vec::new(),
            materiality,
            policy,
            None,
        )
        .unwrap()
    }

    fn proposal(g: &Graph, patch: SemanticPatch) -> Proposal {
        proposal_as(
            g,
            patch,
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
        )
    }

    /// Checks the universal report invariants and returns the report.
    fn checked(report: IntakeReport) -> IntakeReport {
        report.validate().unwrap();
        assert_eq!(report.requires_reason, !report.reason_codes.is_empty());
        report
    }

    fn run_with(g: &Graph, p: &Proposal, validation: &IntakeValidationInput) -> IntakeReport {
        checked(intake(g, p, validation).unwrap_or_else(|e| panic!("{e}")))
    }

    fn run(g: &Graph, p: &Proposal) -> IntakeReport {
        run_with(g, p, &IntakeValidationInput::empty())
    }

    fn candidate(g: &Graph, p: &Proposal) -> Graph {
        apply_patch(g, &p.patch_set).unwrap().graph
    }

    fn codes(list: &[IntakeReasonCode]) -> BTreeSet<IntakeReasonCode> {
        list.iter().copied().collect()
    }

    fn subjects(report: &IntakeReport) -> Vec<String> {
        report
            .subject_novelty
            .iter()
            .map(|s| s.subject_ref.to_string())
            .collect()
    }

    fn candidates_of(report: &IntakeReport, subject: &str) -> Vec<String> {
        report
            .duplicate_candidates
            .iter()
            .filter(|c| c.subject_ref == id(subject))
            .map(|c| c.candidate_ref.to_string())
            .collect()
    }

    fn coverage(subject: &str, by: &str) -> CoverageMatch {
        CoverageMatch {
            subject_ref: id(subject),
            covered_by_ref: id(by),
            score: 1.0,
        }
    }

    // ------------------------------------------------------------------ validation fixtures

    fn metadata() -> ValidationRegistry {
        ValidationRegistry::new(load_builtin_software_profile().unwrap()).unwrap()
    }

    fn registry() -> EvaluatorRegistry {
        let mut registry = EvaluatorRegistry::new(metadata());
        rules::register_i0_evaluators(&mut registry).unwrap();
        rules::register_f1_evaluators(&mut registry).unwrap();
        registry
    }

    fn context(g: &Graph) -> ValidationContext {
        ValidationContext::new(
            g,
            &metadata(),
            ValidationPolicy::default(),
            g.evidence_hash().unwrap(),
            vec![],
            vec![],
        )
        .unwrap()
    }

    /// Canonical F1 inputs for every Accepted Requirement of `g`, built through the public wire
    /// form of the lint input.
    fn f1_inputs(g: &Graph) -> F1ValidationInputs {
        let requirements = g
            .nodes()
            .values()
            .filter(|n| n.status == Accepted)
            .filter_map(|n| match &n.payload {
                NodePayload::Requirement(r) => Some((n, r)),
                _ => None,
            })
            .map(|(n, r)| {
                let evidence: Vec<Id> = n.evidence.iter().map(|e| e.as_id().clone()).collect();
                F1RequirementValidationInput {
                    requirement_ref: n.id.clone(),
                    vocabulary_dependencies: Vec::new(),
                    lint_input: serde_json::from_value(json!({
                        "requirement_ref": n.id,
                        "statement": r.statement,
                        "evidence_refs": evidence,
                        "source_anchor": null,
                        "term_context": null,
                    }))
                    .unwrap(),
                }
            })
            .collect();
        F1ValidationInputs::new(requirements, Vec::new(), Default::default())
    }

    /// The real gate report of `gate` evaluated directly over `g`.
    fn gate_report(g: &Graph, gate: GateId) -> GateReport {
        let ctx = match gate {
            GateId::F1 => context(g).with_f1_inputs(g, f1_inputs(g)).unwrap(),
            _ => context(g),
        };
        let report = registry().evaluate_gate(gate, g, &ctx).unwrap();
        report.validate().unwrap();
        report
    }

    const A: &str = "requirement:a";
    const C: &str = "requirement:c";
    const CONTRADICTION: &str = "finding:contradiction";

    /// Base: Accepted Requirement A and an unrelated Accepted Requirement. The proposal adds an
    /// Accepted Requirement C and an Accepted Open contradiction Finding over A and C.
    fn contradiction_case() -> (Graph, Proposal, Graph) {
        let g = graph(vec![
            req(A, Accepted, "The operator shall retain records."),
            req(
                "requirement:z",
                Accepted,
                "The auditor shall review exports.",
            ),
        ]);
        let p = proposal(
            &g,
            compound(vec![
                add(req(C, Accepted, "The operator shall delete records.")),
                add(finding_node(CONTRADICTION, NO_CONTRADICTION, &[A, C])),
            ]),
        );
        let cand = candidate(&g, &p);
        (g, p, cand)
    }

    fn contradiction_finding(report: &GateReport) -> GeneratedFinding {
        report
            .findings
            .iter()
            .find(|f| f.payload.code == NO_CONTRADICTION)
            .cloned()
            .expect("the F1 contradiction rule fails over the candidate graph")
    }

    fn input(reports: Vec<GateReport>) -> IntakeValidationInput {
        IntakeValidationInput::new(reports).unwrap()
    }

    // ------------------------------------------------------------------ dry run

    #[test]
    fn dry_run_binds_hashes_impact_and_corpus() {
        let g = graph(vec![
            req("requirement:a", Accepted, "Managers shall approve leave."),
            req(
                "requirement:b",
                Accepted,
                "Employees shall submit expenses.",
            ),
        ]);
        let p = proposal(
            &g,
            add(req(
                "requirement:n",
                Proposed,
                "Managers shall approve leave quickly.",
            )),
        );
        let report = run(&g, &p);
        let applied = apply_patch(&g, &p.patch_set).unwrap();
        assert_eq!(report.proposal_ref, p.id);
        assert_eq!(report.base_semantic_hash, g.semantic_hash().unwrap());
        assert_eq!(
            report.candidate_semantic_hash,
            applied.graph.semantic_hash().unwrap()
        );
        assert_eq!(report.base_semantic_hash.kind(), HashKind::Semantic);
        assert_eq!(report.retrieval_corpus_hash.kind(), HashKind::Generic);
        // Impact is exactly the existing patch-engine impact of the dry run.
        let impact = compute_impact(&g, &applied.graph, &applied.delta).unwrap();
        assert_eq!(report.impact, impact);
        // The retrieval corpus is the Accepted base corpus; the Proposed subject is not in it.
        let base_index = LexicalIndex::build(&g).unwrap();
        assert_eq!(&report.retrieval_corpus_hash, base_index.corpus_hash());
        let candidate_index = LexicalIndex::build(&applied.graph).unwrap();
        assert_eq!(candidate_index.corpus_hash(), base_index.corpus_hash());
        assert_eq!(subjects(&report), ["requirement:n"]);
    }

    #[test]
    fn intake_mutates_neither_graph_nor_proposal_and_is_deterministic() {
        let g = graph(vec![
            req("requirement:a", Accepted, "Managers shall approve leave."),
            term("term:leave", Accepted, "leave", &["absence"]),
        ]);
        let p = proposal(
            &g,
            compound(vec![
                add(req(
                    "requirement:n",
                    Proposed,
                    "Managers shall approve leave.",
                )),
                add(term("term:new", Proposed, "Absence", &[])),
            ]),
        );
        let (g_before, p_before) = (g.clone(), p.clone());
        let first = run(&g, &p);
        assert_eq!(g, g_before);
        assert_eq!(p, p_before);
        assert_eq!(p.id, p_before.id);
        assert_eq!(p.recompute_id().unwrap(), p.id);
        let second = run(&g, &p);
        assert_eq!(first, second);
        // Graph construction order does not matter.
        let reversed = graph(vec![
            term("term:leave", Accepted, "leave", &["absence"]),
            req("requirement:a", Accepted, "Managers shall approve leave."),
        ]);
        assert_eq!(run(&reversed, &p), first);
        // Independent operations in reverse declaration order: the same analysis.
        let q = proposal(
            &g,
            compound(vec![
                add(term("term:new", Proposed, "Absence", &[])),
                add(req(
                    "requirement:n",
                    Proposed,
                    "Managers shall approve leave.",
                )),
            ]),
        );
        let mut reordered = run(&g, &q);
        assert_ne!(q.id, p.id);
        assert_eq!(reordered.proposal_ref, q.id);
        reordered.proposal_ref = p.id.clone();
        assert_eq!(reordered, first);
        // The report round-trips through its wire form.
        let wire = serde_json::to_value(&first).unwrap();
        let back: IntakeReport = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(back, first);
        let mut extra = wire;
        extra["unexpected"] = json!(1);
        assert!(serde_json::from_value::<IntakeReport>(extra).is_err());
    }

    #[test]
    fn stale_base_is_a_patch_error_without_report() {
        let g = graph(vec![req(
            "requirement:a",
            Accepted,
            "Managers approve leave.",
        )]);
        let p = proposal(&g, add(req("requirement:n", Proposed, "New leave rule.")));
        let other = graph(vec![req("requirement:a", Accepted, "Managers deny leave.")]);
        assert!(matches!(
            intake(&other, &p, &IntakeValidationInput::empty()),
            Err(IntakeError::Patch(PatchError::StaleBase { .. }))
        ));
    }

    #[test]
    fn precondition_failure_is_a_patch_error_without_report() {
        let g = graph(vec![req(
            "requirement:a",
            Accepted,
            "Managers approve leave.",
        )]);
        let other = graph(vec![req("requirement:x", Accepted, "Unrelated text.")]);
        // An element hash taken from another node of the same ID shape does not match.
        let mut target = pre(&g, "requirement:a");
        target.expected_hash = pre(&other, "requirement:x").expected_hash;
        let p = proposal(
            &g,
            SemanticPatch::ReplacePayload {
                target,
                payload: NodePayload::Requirement(requirement_payload(
                    "Managers deny leave.",
                    Modality::Shall,
                )),
            },
        );
        assert!(matches!(
            intake(&g, &p, &IntakeValidationInput::empty()),
            Err(IntakeError::Patch(PatchError::ElementHashMismatch { .. }))
        ));
    }

    #[test]
    fn structurally_invalid_candidate_is_a_patch_error_without_report() {
        let g = graph(vec![
            req("requirement:a", Accepted, "Managers approve leave."),
            req("requirement:b", Accepted, "Managers approve leave fast."),
        ]);
        // Accepted refines must stay acyclic: an Accepted two-node cycle is not a valid Graph.
        let p = proposal(
            &g,
            compound(vec![
                add_edge(edge(
                    "edge:ab",
                    RelationKind::Refines,
                    "requirement:a",
                    "requirement:b",
                    Accepted,
                )),
                add_edge(edge(
                    "edge:ba",
                    RelationKind::Refines,
                    "requirement:b",
                    "requirement:a",
                    Accepted,
                )),
            ]),
        );
        let err = intake(&g, &p, &IntakeValidationInput::empty()).unwrap_err();
        assert!(
            matches!(err, IntakeError::Patch(PatchError::FinalGraphInvalid(_))),
            "{err:?}"
        );
    }

    #[test]
    fn invalid_proposal_is_rejected_before_dry_run() {
        let g = graph(vec![req(
            "requirement:a",
            Accepted,
            "Managers approve leave.",
        )]);
        let mut p = proposal(&g, add(req("requirement:n", Proposed, "New rule.")));
        p.confidence = Some(0.5);
        assert!(matches!(
            intake(&g, &p, &IntakeValidationInput::empty()),
            Err(IntakeError::Proposal(_))
        ));
    }

    // ------------------------------------------------------------------ lexical subjects

    #[test]
    fn proposed_subject_is_queried_but_never_indexed() {
        let g = graph(vec![
            req(
                "requirement:a",
                Accepted,
                "Managers approve leave requests.",
            ),
            req("requirement:b", Accepted, "Payroll exports salaries."),
        ]);
        let p = proposal(
            &g,
            add(req(
                "requirement:n",
                Proposed,
                "Managers approve leave requests within two days.",
            )),
        );
        let report = run(&g, &p);
        assert_eq!(subjects(&report), ["requirement:n"]);
        assert_eq!(candidates_of(&report, "requirement:n"), ["requirement:a"]);
        assert_eq!(
            &report.retrieval_corpus_hash,
            LexicalIndex::build(&g).unwrap().corpus_hash()
        );
        // The candidate corpus (Accepted only) is unchanged by a Proposed addition.
        let cand = candidate(&g, &p);
        assert_eq!(
            LexicalIndex::build(&cand).unwrap().corpus_hash(),
            LexicalIndex::build(&g).unwrap().corpus_hash()
        );
    }

    #[test]
    fn accepted_payload_edit_is_queried_against_the_old_corpus() {
        let g = graph(vec![
            req("requirement:a", Accepted, "Managers approve leave."),
            req(
                "requirement:b",
                Accepted,
                "Payroll exports salaries monthly.",
            ),
        ]);
        let p = proposal(
            &g,
            replace_payload(
                &g,
                "requirement:a",
                NodePayload::Requirement(requirement_payload(
                    "Payroll exports salaries weekly.",
                    Modality::Shall,
                )),
            ),
        );
        let report = run(&g, &p);
        assert_eq!(subjects(&report), ["requirement:a"]);
        // The subject's own old document is excluded from candidates; B is found.
        assert_eq!(candidates_of(&report, "requirement:a"), ["requirement:b"]);
        assert_eq!(
            &report.retrieval_corpus_hash,
            LexicalIndex::build(&g).unwrap().corpus_hash()
        );
    }

    #[test]
    fn status_promotion_into_accepted_is_a_subject() {
        let g = graph(vec![
            req("requirement:a", Accepted, "Managers approve leave."),
            req("requirement:p", Proposed, "Managers approve leave."),
        ]);
        let p = proposal(&g, set_status(&g, "requirement:p", Proposed, Accepted));
        let report = run(&g, &p);
        assert_eq!(subjects(&report), ["requirement:p"]);
        assert_eq!(
            report.covered_by,
            [coverage("requirement:p", "requirement:a")]
        );
    }

    #[test]
    fn envelope_only_and_ineligible_changes_create_no_subject() {
        let g = graph(vec![
            req("requirement:a", Accepted, "Managers approve leave."),
            req("requirement:p", Proposed, "Payroll exports salaries."),
            req("requirement:q", Accepted, "Payroll exports bonuses."),
        ]);
        let mapping = serde_json::from_value(json!({
            "standard_id": "ISO/IEC/IEEE 29148", "version": "2018", "concept": "requirement",
            "clause_ref": "5.2.5", "mapping_role": "semantic_alignment",
            "mapping_strength": "compatible", "validator_rules": ["ISO29148.F1.A"]
        }))
        .unwrap();
        let envelope = proposal(
            &g,
            SemanticPatch::AttachStandardMapping {
                target: pre(&g, "requirement:a"),
                mapping,
            },
        );
        let report = run(&g, &envelope);
        assert!(report.subject_novelty.is_empty());
        assert!(report.duplicate_candidates.is_empty());
        assert!(report.covered_by.is_empty());
        assert_eq!(report.novelty, None);
        // Rejected / deprecated candidates are not subjects.
        let p = proposal(
            &g,
            compound(vec![
                set_status(&g, "requirement:p", Proposed, Rejected),
                set_status(&g, "requirement:q", Accepted, Deprecated),
                add(req("requirement:r", Rejected, "Managers approve leave.")),
            ]),
        );
        let report = run(&g, &p);
        assert!(report.subject_novelty.is_empty(), "{report:?}");
        assert!(report.covered_by.is_empty());
        // A non-searchable payload family is not a subject.
        let p = proposal(&g, add(term("term:leave", Proposed, "leave", &[])));
        assert!(run(&g, &p).subject_novelty.is_empty());
        // Status values outside Proposed/Accepted on added nodes are excluded as well.
        for status in [Suspect, Superseded] {
            let p = proposal(
                &g,
                add(req("requirement:s", status, "Managers approve leave.")),
            );
            if let Ok(report) = intake(&g, &p, &IntakeValidationInput::empty()) {
                assert!(checked(report).subject_novelty.is_empty());
            }
        }
    }

    // ------------------------------------------------------------------ coverage

    #[test]
    fn cross_id_exact_equivalent_coverage() {
        let a = requirement_payload("Managers shall approve leave.", Modality::Shall);
        let g = graph(vec![req_with("requirement:a", Accepted, a.clone())]);
        let p = proposal(&g, add(req_with("requirement:b", Proposed, a)));
        let report = run(&g, &p);
        assert_eq!(
            report.covered_by,
            [CoverageMatch {
                subject_ref: id("requirement:b"),
                covered_by_ref: id("requirement:a"),
                score: 1.0,
            }]
        );
        assert!(report.reason_codes.contains(&ExactEquivalent));
        assert_eq!(report.reason_codes, codes(&[ExactEquivalent]));
        assert!(report.requires_reason);
        // The exact match may also be an advisory lexical candidate.
        assert_eq!(candidates_of(&report, "requirement:b"), ["requirement:a"]);
    }

    #[test]
    fn same_statement_with_different_typed_payload_is_not_coverage() {
        let g = graph(vec![req_with(
            "requirement:a",
            Accepted,
            requirement_payload("Managers approve leave.", Modality::Shall),
        )]);
        let mut level = requirement_payload("Managers approve leave.", Modality::Shall);
        level.level = RequirementLevel::Stakeholder;
        let mut kind = requirement_payload("Managers approve leave.", Modality::Shall);
        kind.requirement_kind = RequirementKind::Operational;
        let variants = [
            requirement_payload("Managers approve leave.", Modality::Should),
            level,
            kind,
        ];
        for variant in variants {
            let p = proposal(&g, add(req_with("requirement:b", Proposed, variant)));
            let report = run(&g, &p);
            assert!(report.covered_by.is_empty());
            assert_eq!(candidates_of(&report, "requirement:b"), ["requirement:a"]);
            assert!(!report.reason_codes.contains(&ExactEquivalent));
            assert!(!report.requires_reason);
        }
    }

    #[test]
    fn coverage_requires_accepted_base_of_the_same_type() {
        let a = requirement_payload("Managers approve leave.", Modality::Shall);
        let g = graph(vec![
            req_with("requirement:p", Proposed, a.clone()),
            concept("concept:x", Accepted, "Managers approve leave.", "x"),
        ]);
        let p = proposal(&g, add(req_with("requirement:b", Proposed, a)));
        let report = run(&g, &p);
        assert!(report.covered_by.is_empty());
        assert!(!report.requires_reason);
    }

    #[test]
    fn coverage_lists_every_exact_match_in_order() {
        let a = requirement_payload("Managers approve leave.", Modality::Shall);
        let g = graph(vec![
            req_with("requirement:z", Accepted, a.clone()),
            req_with("requirement:a", Accepted, a.clone()),
        ]);
        let p = proposal(
            &g,
            compound(vec![
                add(req_with("requirement:y", Proposed, a.clone())),
                add(req_with("requirement:b", Proposed, a)),
            ]),
        );
        let report = run(&g, &p);
        assert_eq!(
            report.covered_by,
            [
                coverage("requirement:b", "requirement:a"),
                coverage("requirement:b", "requirement:z"),
                coverage("requirement:y", "requirement:a"),
                coverage("requirement:y", "requirement:z"),
            ]
        );
        // Several equivalents: one reason code.
        assert_eq!(report.reason_codes, codes(&[ExactEquivalent]));
    }

    // ------------------------------------------------------------------ duplicate candidates

    #[test]
    fn lexical_candidate_alone_requires_no_reason() {
        let g = graph(vec![
            req(
                "requirement:a",
                Accepted,
                "Managers approve leave requests.",
            ),
            concept(
                "concept:leave",
                Accepted,
                "Leave requests",
                "managers approve",
            ),
        ]);
        let p = proposal(
            &g,
            add(req(
                "requirement:n",
                Proposed,
                "Managers approve leave requests promptly.",
            )),
        );
        let report = run(&g, &p);
        // Same-kind filtering: the Concept matches lexically but is another kind.
        assert_eq!(candidates_of(&report, "requirement:n"), ["requirement:a"]);
        let candidate = &report.duplicate_candidates[0];
        assert!(candidate.score.is_finite() && candidate.score > 0.0);
        // The score is the advisory X1.2 BM25 score of the subject text, unchanged.
        let hits = LexicalIndex::build(&g)
            .unwrap()
            .search("Managers approve leave requests promptly.", 10)
            .unwrap();
        let hit = hits.iter().find(|h| h.id == id("requirement:a")).unwrap();
        assert_eq!(candidate.score, hit.score);
        assert!(report.covered_by.is_empty());
        assert!(report.reason_codes.is_empty());
        assert!(!report.requires_reason);
    }

    #[test]
    fn candidates_are_capped_at_twenty_best() {
        assert_eq!(INTAKE_DUPLICATE_CANDIDATE_LIMIT, 20);
        let mut nodes = Vec::new();
        for i in 0..25 {
            let padding = " filler".repeat(i);
            nodes.push(req(
                &format!("requirement:r{i:02}"),
                Accepted,
                &format!("leave approval{padding}"),
            ));
        }
        nodes.push(concept(
            "concept:leave",
            Accepted,
            "leave approval",
            "leave",
        ));
        let g = graph(nodes);
        let p = proposal(&g, add(req("requirement:n", Proposed, "leave approval")));
        let report = run(&g, &p);
        let expected: Vec<String> = LexicalIndex::build(&g)
            .unwrap()
            .search("leave approval", 100)
            .unwrap()
            .into_iter()
            .filter(|h| h.kind == RetrievalDocumentKind::Requirement)
            .take(20)
            .map(|h| h.id.to_string())
            .collect();
        let got = candidates_of(&report, "requirement:n");
        assert_eq!(got.len(), 20);
        assert_eq!(got, expected);
        // Shorter documents score higher: r00..r19 are the best twenty.
        let best: Vec<String> = (0..20).map(|i| format!("requirement:r{i:02}")).collect();
        assert_eq!(got, best);
    }

    #[test]
    fn equal_scores_order_by_id_and_self_is_excluded() {
        let g = graph(vec![
            req("requirement:c", Accepted, "leave approval rules"),
            req("requirement:a", Accepted, "leave approval rules"),
            req("requirement:b", Accepted, "leave approval rules"),
            req("requirement:x", Accepted, "payroll export"),
        ]);
        let p = proposal(
            &g,
            replace_payload(
                &g,
                "requirement:b",
                NodePayload::Requirement(requirement_payload(
                    "leave approval rules updated",
                    Modality::Shall,
                )),
            ),
        );
        let report = run(&g, &p);
        assert_eq!(
            candidates_of(&report, "requirement:b"),
            ["requirement:a", "requirement:c"]
        );
        let scores: Vec<f64> = report
            .duplicate_candidates
            .iter()
            .map(|c| c.score)
            .collect();
        assert_eq!(scores[0], scores[1]);
    }

    #[test]
    fn candidates_order_by_subject_then_score() {
        let g = graph(vec![
            req("requirement:a", Accepted, "leave approval"),
            req(
                "requirement:b",
                Accepted,
                "leave approval by managers and directors",
            ),
        ]);
        let p = proposal(
            &g,
            compound(vec![
                add(req("requirement:z", Proposed, "leave approval")),
                add(req("requirement:m", Proposed, "leave approval")),
            ]),
        );
        let report = run(&g, &p);
        let pairs: Vec<(String, String)> = report
            .duplicate_candidates
            .iter()
            .map(|c| (c.subject_ref.to_string(), c.candidate_ref.to_string()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("requirement:m".into(), "requirement:a".into()),
                ("requirement:m".into(), "requirement:b".into()),
                ("requirement:z".into(), "requirement:a".into()),
                ("requirement:z".into(), "requirement:b".into()),
            ]
        );
        assert!(report.duplicate_candidates[0].score > report.duplicate_candidates[1].score);
    }

    // ------------------------------------------------------------------ explicit relations

    #[test]
    fn explicit_refinement_is_reported_without_reason() {
        let g = graph(vec![
            req("requirement:a", Accepted, "Managers approve leave."),
            req(
                "requirement:b",
                Accepted,
                "Managers approve leave within two days.",
            ),
        ]);
        let p = proposal(
            &g,
            add_edge(edge(
                "edge:refines",
                RelationKind::Refines,
                "requirement:b",
                "requirement:a",
                Proposed,
            )),
        );
        let report = run(&g, &p);
        assert_eq!(
            report.refines,
            [RefinementLink {
                edge_ref: id("edge:refines"),
                from_ref: id("requirement:b"),
                to_ref: id("requirement:a"),
            }]
        );
        assert!(report.declared_conflicts.is_empty());
        assert!(!report.requires_reason);
        // Edge-only proposal: no lexical subjects.
        assert!(report.subject_novelty.is_empty());
        assert_eq!(report.novelty, None);
    }

    #[test]
    fn refinement_is_never_inferred_from_text() {
        let g = graph(vec![req(
            "requirement:a",
            Accepted,
            "Managers approve leave.",
        )]);
        let p = proposal(
            &g,
            add(req(
                "requirement:b",
                Proposed,
                "Managers approve leave within two working days of the request.",
            )),
        );
        let report = run(&g, &p);
        assert!(report.refines.is_empty());
        assert!(report.declared_conflicts.is_empty());
    }

    #[test]
    fn explicit_conflict_requires_reason() {
        let g = graph(vec![
            req("requirement:a", Accepted, "Managers approve leave."),
            req("requirement:b", Accepted, "Managers never approve leave."),
        ]);
        let p = proposal(
            &g,
            compound(vec![
                add_edge(edge(
                    "edge:c2",
                    RelationKind::ConflictsWith,
                    "requirement:a",
                    "requirement:b",
                    Accepted,
                )),
                add_edge(edge(
                    "edge:c1",
                    RelationKind::ConflictsWith,
                    "requirement:a",
                    "requirement:b",
                    Proposed,
                )),
            ]),
        );
        let report = run(&g, &p);
        let conflict = |e: &str| DeclaredConflict {
            edge_ref: id(e),
            from_ref: id("requirement:a"),
            to_ref: id("requirement:b"),
        };
        assert_eq!(
            report.declared_conflicts,
            [conflict("edge:c1"), conflict("edge:c2")]
        );
        assert_eq!(report.reason_codes, codes(&[DeclaredConflictReason]));
        assert!(report.requires_reason);
    }

    #[test]
    fn removed_or_rejected_conflict_edges_are_not_declared() {
        let g = graph_with(
            vec![
                req("requirement:a", Accepted, "Managers approve leave."),
                req("requirement:b", Accepted, "Managers never approve leave."),
            ],
            vec![
                edge(
                    "edge:c",
                    RelationKind::ConflictsWith,
                    "requirement:a",
                    "requirement:b",
                    Accepted,
                ),
                edge(
                    "edge:d",
                    RelationKind::ConflictsWith,
                    "requirement:a",
                    "requirement:b",
                    Proposed,
                ),
            ],
        );
        let p = proposal(
            &g,
            compound(vec![
                SemanticPatch::RemoveEdge {
                    target: pre(&g, "edge:c"),
                },
                set_status(&g, "edge:d", Proposed, Rejected),
            ]),
        );
        let report = run(&g, &p);
        assert!(report.declared_conflicts.is_empty());
        assert!(!report.requires_reason);
        // Text that merely sounds contradictory declares nothing.
        let p = proposal(
            &g,
            add(req(
                "requirement:n",
                Proposed,
                "Managers shall not approve leave.",
            )),
        );
        let report = run(&g, &p);
        assert!(report.declared_conflicts.is_empty());
        assert!(report.validation_findings.is_empty());
        assert!(!report.requires_reason);
    }

    // ------------------------------------------------------------------ terms

    fn resolution(report: &IntakeReport, term_ref: &str) -> TermResolution {
        report
            .term_resolutions
            .iter()
            .find(|t| t.term_ref == id(term_ref))
            .cloned()
            .unwrap()
    }

    #[test]
    fn term_resolution_statuses() {
        let g = graph(vec![
            term("term:leave", Accepted, "Leave request", &["absence"]),
            term("term:holiday", Accepted, "Holiday", &["day off"]),
            term("term:vacation", Accepted, "Vacation", &["Day-Off"]),
            term("term:proposed", Proposed, "Overtime", &[]),
        ]);
        let p = proposal(
            &g,
            compound(vec![
                add(term("term:t1", Proposed, "leave  REQUEST", &[])),
                add(term("term:t2", Proposed, "Absence", &[])),
                add(term("term:t3", Proposed, "Payroll", &["leave request"])),
                add(term("term:t4", Accepted, "day off", &[])),
                add(term("term:t5", Proposed, "?!", &[])),
                add(term("term:t6", Proposed, "overtime", &[])),
            ]),
        );
        let report = run(&g, &p);
        let r = |status, key: Option<&str>, matches: &[&str], t: &str| TermResolution {
            term_ref: id(t),
            key: key.map(str::to_owned),
            matched_term_refs: matches.iter().map(|m| id(m)).collect(),
            status,
        };
        use TermResolutionStatus::*;
        assert_eq!(
            report.term_resolutions,
            [
                r(Resolved, Some("leave request"), &["term:leave"], "term:t1"),
                r(Resolved, Some("absence"), &["term:leave"], "term:t2"),
                // Only the candidate's own term is keyed; its aliases are not looked up.
                r(New, Some("payroll"), &[], "term:t3"),
                r(
                    Ambiguous,
                    Some("day off"),
                    &["term:holiday", "term:vacation"],
                    "term:t4"
                ),
                r(Unsearchable, None, &[], "term:t5"),
                // A Proposed base Term is not in the dictionary.
                r(New, Some("overtime"), &[], "term:t6"),
            ]
        );
        assert_eq!(report.reason_codes, codes(&[AmbiguousTerm]));
        assert!(report.requires_reason);
        // Term keys come from the X1.2 tokenizer.
        assert_eq!(tokenize("leave  REQUEST").join(" "), "leave request");
    }

    #[test]
    fn new_term_requires_no_reason_and_self_resolution_is_allowed() {
        let g = graph(vec![
            term("term:leave", Accepted, "leave", &[]),
            term("term:p", Proposed, "Overtime", &[]),
        ]);
        let p = proposal(&g, add(term("term:new", Proposed, "Payroll", &[])));
        let report = run(&g, &p);
        assert_eq!(
            resolution(&report, "term:new").status,
            TermResolutionStatus::New
        );
        assert!(resolution(&report, "term:new").matched_term_refs.is_empty());
        assert!(!report.requires_reason);
        // An existing Accepted Term whose payload changes may resolve to itself.
        let mut payload = match &g.node(&id("term:leave")).unwrap().payload {
            NodePayload::Term(t) => t.clone(),
            _ => unreachable!(),
        };
        payload.aliases = Some(vec!["absence".into()]);
        let p = proposal(
            &g,
            replace_payload(&g, "term:leave", NodePayload::Term(payload)),
        );
        let report = run(&g, &p);
        let res = resolution(&report, "term:leave");
        assert_eq!(res.status, TermResolutionStatus::Resolved);
        assert_eq!(res.matched_term_refs, [id("term:leave")]);
        // Envelope-only Term changes are not resolved; nor are untouched Terms.
        let p = proposal(&g, set_status(&g, "term:p", Proposed, Rejected));
        assert!(run(&g, &p).term_resolutions.is_empty());
    }

    #[test]
    fn term_definitions_are_never_compared() {
        let g = graph(vec![
            concept("concept:a", Accepted, "Leave", "Paid absence."),
            concept("concept:b", Accepted, "Leave", "Unpaid absence."),
            term_defined("term:leave", Accepted, "leave", &[], Some("concept:a")),
        ]);
        let p = proposal(
            &g,
            add(term_defined(
                "term:other",
                Proposed,
                "Leave",
                &[],
                Some("concept:b"),
            )),
        );
        let report = run(&g, &p);
        assert_eq!(
            resolution(&report, "term:other").status,
            TermResolutionStatus::Resolved
        );
        assert!(report.validation_findings.is_empty());
        assert!(report.reason_codes.is_empty());
        assert!(!report.requires_reason);
    }

    // ------------------------------------------------------------------ novelty

    fn novelty_of(report: &IntakeReport, subject: &str) -> SubjectNovelty {
        report
            .subject_novelty
            .iter()
            .find(|s| s.subject_ref == id(subject))
            .cloned()
            .unwrap()
    }

    #[test]
    fn novelty_exact_disjoint_and_partial_jaccard() {
        let g = graph(vec![
            req("requirement:base", Accepted, "alpha beta delta epsilon"),
            concept("concept:x", Accepted, "alpha", "beta gamma"),
        ]);
        let p = proposal(
            &g,
            compound(vec![
                add(req(
                    "requirement:exact",
                    Proposed,
                    "Epsilon DELTA beta alpha alpha",
                )),
                add(req("requirement:disjoint", Proposed, "omega")),
                add(req("requirement:partial", Proposed, "alpha beta gamma")),
            ]),
        );
        let report = run(&g, &p);
        let exact = novelty_of(&report, "requirement:exact");
        assert_eq!(exact.kind, RetrievalDocumentKind::Requirement);
        assert_eq!(exact.nearest_ref, Some(id("requirement:base")));
        assert_eq!((exact.similarity, exact.novelty), (1.0, 0.0));
        let disjoint = novelty_of(&report, "requirement:disjoint");
        assert_eq!((disjoint.similarity, disjoint.novelty), (0.0, 1.0));
        // Same-kind only: the Concept shares "gamma" but is not compared.
        let partial = novelty_of(&report, "requirement:partial");
        assert_eq!(partial.similarity, 2.0 / 5.0);
        assert_eq!(partial.novelty, 1.0 - 2.0 / 5.0);
        assert_eq!(partial.nearest_ref, Some(id("requirement:base")));
        // Overall novelty is the minimum subject novelty.
        assert_eq!(report.novelty, Some(0.0));
        assert_eq!(
            subjects(&report),
            [
                "requirement:disjoint",
                "requirement:exact",
                "requirement:partial"
            ]
        );
        // Novelty 0.0 alone requires no reason (the typed payloads differ from the base).
        assert!(report.covered_by.is_empty());
        assert!(!report.requires_reason);
    }

    #[test]
    fn novelty_nearest_tie_and_missing_kind() {
        let g = graph(vec![
            req("requirement:b", Accepted, "alpha beta"),
            req("requirement:a", Accepted, "alpha gamma"),
        ]);
        let p = proposal(
            &g,
            compound(vec![
                add(req("requirement:n", Proposed, "alpha")),
                add(concept("concept:n", Proposed, "alpha", "beta")),
            ]),
        );
        let report = run(&g, &p);
        let n = novelty_of(&report, "requirement:n");
        assert_eq!(n.nearest_ref, Some(id("requirement:a")));
        assert_eq!(n.similarity, 0.5);
        let c = novelty_of(&report, "concept:n");
        assert_eq!(
            c,
            SubjectNovelty {
                subject_ref: id("concept:n"),
                kind: RetrievalDocumentKind::Concept,
                nearest_ref: None,
                similarity: 0.0,
                novelty: 1.0,
            }
        );
        assert_eq!(report.novelty, Some(0.5));
    }

    // ------------------------------------------------------------------ validation reuse

    #[test]
    fn real_f1_contradiction_finding_is_reused_exactly() {
        let (g, p, cand) = contradiction_case();
        let f1 = gate_report(&cand, GateId::F1);
        let direct = contradiction_finding(&f1);
        assert!(direct.payload.affected_refs.contains(&id(C)));
        let report = run_with(&g, &p, &input(vec![f1.clone()]));
        assert_eq!(report.validation_gates, [GateId::F1]);
        let surfaced = report
            .validation_findings
            .iter()
            .find(|f| f.id == direct.id)
            .unwrap();
        assert_eq!(surfaced, &direct);
        // Every surfaced finding is an exact copy from the gate report.
        for finding in &report.validation_findings {
            assert!(f1.findings.contains(finding));
        }
        assert!(report.reason_codes.contains(&ValidationConflict));
        assert!(report.requires_reason);
    }

    #[test]
    fn empty_validation_input_claims_nothing() {
        let (g, p, _) = contradiction_case();
        let report = run(&g, &p);
        assert!(report.validation_gates.is_empty());
        assert!(report.validation_findings.is_empty());
        assert!(!report.reason_codes.contains(&ValidationConflict));
        assert_eq!(
            IntakeValidationInput::empty(),
            IntakeValidationInput::default()
        );
        assert!(IntakeValidationInput::empty()
            .candidate_reports()
            .is_empty());
    }

    #[test]
    fn gate_reports_are_bound_to_the_candidate() {
        let (g, p, cand) = contradiction_case();
        // A report over the base graph is not evidence about the candidate.
        let stale = gate_report(&g, GateId::I0);
        assert_eq!(
            intake(&g, &p, &input(vec![stale])),
            Err(IntakeError::ValidationReportBaselineMismatch { gate: GateId::I0 })
        );
        let mut profile = gate_report(&cand, GateId::I0);
        profile.profile_id = id("profile:other");
        assert_eq!(
            intake(&g, &p, &input(vec![profile])),
            Err(IntakeError::ValidationReportProfileMismatch { gate: GateId::I0 })
        );
        let mut broken = gate_report(&cand, GateId::I0);
        broken.findings.reverse();
        broken
            .findings
            .push(contradiction_finding(&gate_report(&cand, GateId::F1)));
        assert!(matches!(
            intake(&g, &p, &input(vec![broken])),
            Err(IntakeError::Validation {
                gate: GateId::I0,
                ..
            })
        ));
    }

    #[test]
    fn duplicate_and_incoherent_gate_reports_are_rejected() {
        let (g, p, cand) = contradiction_case();
        let i0 = gate_report(&cand, GateId::I0);
        let f1 = gate_report(&cand, GateId::F1);
        assert_eq!(
            IntakeValidationInput::new(vec![i0.clone(), i0.clone()]),
            Err(IntakeError::DuplicateValidationGate { gate: GateId::I0 })
        );
        // Canonical GateId order regardless of supply order.
        let ordered = input(vec![f1.clone(), i0.clone()]);
        let gates: Vec<GateId> = ordered.candidate_reports().iter().map(|r| r.gate).collect();
        assert_eq!(gates, [GateId::I0, GateId::F1]);
        let report = run_with(&g, &p, &ordered);
        assert_eq!(report.validation_gates, [GateId::I0, GateId::F1]);
        // Shared context differs between gates.
        let mut other_pack = f1.clone();
        other_pack.rule_pack_hash = canonical_hash(HashKind::Generic, &json!("other")).unwrap();
        assert_eq!(
            intake(&g, &p, &input(vec![i0.clone(), other_pack])),
            Err(IntakeError::IncoherentValidationReports { gate: GateId::F1 })
        );
        let mut other_policy = f1.clone();
        other_policy
            .policy
            .promoted_to_blocker_rule_ids
            .insert(NO_CONTRADICTION.into());
        assert_eq!(
            intake(&g, &p, &input(vec![i0.clone(), other_policy])),
            Err(IntakeError::IncoherentValidationReports { gate: GateId::F1 })
        );
        let mut other_profile_hash = f1;
        other_profile_hash.profile_hash =
            canonical_hash(HashKind::Generic, &json!("profile")).unwrap();
        assert_eq!(
            intake(&g, &p, &input(vec![i0, other_profile_hash])),
            Err(IntakeError::IncoherentValidationReports { gate: GateId::F1 })
        );
    }

    #[test]
    fn failed_gate_report_is_accepted_evidence() {
        let (g, p, cand) = contradiction_case();
        let f1 = gate_report(&cand, GateId::F1);
        assert_eq!(serde_json::to_value(f1.result).unwrap(), json!("FAIL"));
        assert!(intake(&g, &p, &input(vec![f1])).is_ok());
    }

    #[test]
    fn unrelated_findings_are_not_surfaced() {
        let base = graph(vec![
            req(A, Accepted, "The operator shall retain records."),
            req(C, Accepted, "The operator shall delete records."),
            finding_node(CONTRADICTION, NO_CONTRADICTION, &[A, C]),
        ]);
        // The proposal touches only an unrelated Concept.
        let p = proposal(
            &base,
            add(concept(
                "concept:x",
                Proposed,
                "Archive",
                "Long-term storage.",
            )),
        );
        let cand = candidate(&base, &p);
        let f1 = gate_report(&cand, GateId::F1);
        let finding = contradiction_finding(&f1);
        let report = run_with(&base, &p, &input(vec![f1]));
        assert!(!report.validation_findings.contains(&finding));
        assert!(!report.reason_codes.contains(&ValidationConflict));
        assert!(!report.requires_reason);
    }

    /// The contradiction case's F1 report with every finding's severity and waiver replaced.
    fn reshaped(severity: FindingSeverity, waived: bool) -> (Graph, Proposal, GateReport) {
        let (g, p, cand) = contradiction_case();
        let mut f1 = gate_report(&cand, GateId::F1);
        for finding in &mut f1.findings {
            finding.payload.severity = severity;
            finding.payload.waiver_ref = waived.then(|| id("decision:waiver"));
        }
        (g, p, f1)
    }

    #[test]
    fn blocker_and_error_findings_require_reason_unless_waived() {
        for severity in [FindingSeverity::Blocker, FindingSeverity::Error] {
            let (g, p, f1) = reshaped(severity, false);
            let report = run_with(&g, &p, &input(vec![f1]));
            assert!(!report.validation_findings.is_empty());
            assert_eq!(report.reason_codes, codes(&[ValidationConflict]));
            let (g, p, f1) = reshaped(severity, true);
            let report = run_with(&g, &p, &input(vec![f1.clone()]));
            assert!(!report.validation_findings.is_empty());
            for finding in &report.validation_findings {
                assert!(f1.findings.contains(finding));
            }
            assert!(report.reason_codes.is_empty());
            assert!(!report.requires_reason);
        }
    }

    #[test]
    fn validation_conflict_predicate_is_exactly_the_reason_rule() {
        // Hotfix 065: the public predicate surfaced by the API projection agrees with the
        // reason code for every severity and waiver state.
        for severity in [
            FindingSeverity::Blocker,
            FindingSeverity::Error,
            FindingSeverity::Warn,
            FindingSeverity::Info,
        ] {
            for waived in [false, true] {
                let (g, p, f1) = reshaped(severity, waived);
                let report = run_with(&g, &p, &input(vec![f1]));
                assert!(!report.validation_findings.is_empty());
                let surfaced = report
                    .validation_findings
                    .iter()
                    .any(plumb_intake::is_validation_conflict);
                assert_eq!(
                    surfaced,
                    report.reason_codes.contains(&ValidationConflict),
                    "{severity:?} waived={waived}"
                );
            }
        }
    }

    #[test]
    fn warn_and_info_findings_are_reported_without_reason() {
        for severity in [FindingSeverity::Warn, FindingSeverity::Info] {
            let (g, p, f1) = reshaped(severity, false);
            let report = run_with(&g, &p, &input(vec![f1]));
            assert!(!report.validation_findings.is_empty());
            assert!(report
                .validation_findings
                .iter()
                .all(|f| f.payload.severity == severity));
            assert!(report.reason_codes.is_empty());
            assert!(!report.requires_reason);
        }
    }

    // ------------------------------------------------------------------ reasons

    #[test]
    fn materiality_and_policy_reasons() {
        let g = graph(vec![req(
            "requirement:a",
            Accepted,
            "Managers approve leave.",
        )]);
        let patch = || add(req("requirement:n", Proposed, "Payroll exports salaries."));
        let reasons = |materiality, policy| {
            run(&g, &proposal_as(&g, patch(), materiality, policy)).reason_codes
        };
        use AcceptancePolicy::*;
        use ProposalMateriality::*;
        assert_eq!(
            reasons(ProposalMateriality::MaterialDecision, HumanConfirm),
            codes(&[IntakeReasonCode::MaterialDecision])
        );
        assert_eq!(
            reasons(Semantic, AcceptancePolicy::HumanDecision),
            codes(&[IntakeReasonCode::HumanDecision])
        );
        assert_eq!(
            reasons(
                ProposalMateriality::MaterialDecision,
                AcceptancePolicy::HumanDecision
            ),
            codes(&[
                IntakeReasonCode::MaterialDecision,
                IntakeReasonCode::HumanDecision
            ])
        );
        for policy in [HumanConfirm, ProfilePolicy, AutoDerivation, AutoNonSemantic] {
            assert!(reasons(Semantic, policy).is_empty(), "{policy:?}");
        }
        assert!(reasons(NonSemantic, AutoNonSemantic).is_empty());
    }

    #[test]
    fn confidence_never_changes_analysis() {
        let g = graph(vec![req(
            "requirement:a",
            Accepted,
            "Managers approve leave.",
        )]);
        let make = |confidence| {
            Proposal::new(
                StageId::S1,
                PatchSet {
                    base_semantic_hash: g.semantic_hash().unwrap(),
                    patch: add(req("requirement:n", Proposed, "Managers approve leave.")),
                },
                Vec::new(),
                Vec::new(),
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
                confidence,
            )
            .unwrap()
        };
        let (low, high) = (make(Some(0.0)), make(Some(1.0)));
        let (mut a, b) = (run(&g, &low), run(&g, &high));
        a.proposal_ref = b.proposal_ref.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn every_reason_together_and_deduplicated() {
        let (g, _, _) = contradiction_case();
        let base = graph_with(
            {
                let mut nodes: Vec<Node> = g.nodes().values().cloned().collect();
                nodes.push(term("term:h", Accepted, "Holiday", &["day off"]));
                nodes.push(term("term:v", Accepted, "Vacation", &["day off"]));
                nodes
            },
            Vec::new(),
        );
        let statement = requirement_payload("The operator shall retain records.", Modality::Shall);
        let p = proposal_as(
            &base,
            compound(vec![
                add(req(C, Accepted, "The operator shall delete records.")),
                add(finding_node(CONTRADICTION, NO_CONTRADICTION, &[A, C])),
                add(req_with("requirement:copy", Proposed, statement.clone())),
                add(req_with("requirement:copy2", Proposed, statement)),
                add(term("term:x", Proposed, "Day off", &[])),
                add(term("term:y", Proposed, "day-off", &[])),
                add_edge(edge(
                    "edge:conflict",
                    RelationKind::ConflictsWith,
                    A,
                    C,
                    Proposed,
                )),
            ]),
            ProposalMateriality::MaterialDecision,
            AcceptancePolicy::HumanDecision,
        );
        let cand = candidate(&base, &p);
        let report = run_with(&base, &p, &input(vec![gate_report(&cand, GateId::F1)]));
        assert_eq!(
            report.reason_codes,
            codes(&[
                MaterialDecision,
                HumanDecision,
                ExactEquivalent,
                DeclaredConflictReason,
                ValidationConflict,
                AmbiguousTerm,
            ])
        );
        let wire: Vec<String> = report.reason_codes.iter().map(|c| c.to_string()).collect();
        assert_eq!(
            wire,
            [
                "material_decision",
                "human_decision",
                "exact_equivalent",
                "declared_conflict",
                "validation_conflict",
                "ambiguous_term"
            ]
        );
        assert_eq!(
            serde_json::to_value(&report.reason_codes).unwrap(),
            json!(wire)
        );
        assert!(report.requires_reason);
    }

    #[test]
    fn wire_strings_are_frozen() {
        use TermResolutionStatus::*;
        for (status, wire) in [
            (New, "new"),
            (Resolved, "resolved"),
            (Ambiguous, "ambiguous"),
            (Unsearchable, "unsearchable"),
        ] {
            assert_eq!(status.as_str(), wire);
            assert_eq!(serde_json::to_value(status).unwrap(), json!(wire));
        }
    }

    // ------------------------------------------------------------------ report validation

    #[test]
    fn report_validation_rejects_broken_reports() {
        let a = requirement_payload("Managers shall approve leave.", Modality::Shall);
        let g = graph(vec![
            req_with("requirement:a", Accepted, a.clone()),
            req(
                "requirement:b",
                Accepted,
                "Managers shall approve leave quickly.",
            ),
        ]);
        let p = proposal(&g, add(req_with("requirement:n", Proposed, a)));
        let good = run(&g, &p);
        assert!(good.duplicate_candidates.len() >= 2);
        let broken = |f: &dyn Fn(&mut IntakeReport)| {
            let mut r = good.clone();
            f(&mut r);
            r.validate().unwrap_err()
        };
        assert!(matches!(
            broken(&|r| r.base_semantic_hash = r.retrieval_corpus_hash.clone()),
            IntakeReportError::HashKind { .. }
        ));
        assert!(matches!(
            broken(&|r| r.retrieval_corpus_hash = r.base_semantic_hash.clone()),
            IntakeReportError::HashKind { .. }
        ));
        assert!(matches!(
            broken(&|r| r.covered_by[0].score = 0.99),
            IntakeReportError::CoverageScore { .. }
        ));
        assert!(matches!(
            broken(&|r| r.duplicate_candidates[0].score = f64::NAN),
            IntakeReportError::CandidateScore { .. }
        ));
        assert!(matches!(
            broken(&|r| r.duplicate_candidates[0].score = 0.0),
            IntakeReportError::CandidateScore { .. }
        ));
        assert!(matches!(
            broken(&|r| r.duplicate_candidates.swap(0, 1)),
            IntakeReportError::Unordered { .. }
        ));
        assert!(matches!(
            broken(&|r| r.duplicate_candidates[0].candidate_ref = id("requirement:n")),
            IntakeReportError::SelfCandidate(_)
        ));
        assert!(matches!(
            broken(&|r| {
                let extra = r.duplicate_candidates[0].clone();
                r.duplicate_candidates = (0..21)
                    .map(|i| DuplicateCandidate {
                        candidate_ref: id(&format!("requirement:x{i:02}")),
                        ..extra.clone()
                    })
                    .collect();
            }),
            IntakeReportError::CandidateLimit(_)
        ));
        assert!(matches!(
            broken(&|r| r.subject_novelty[0].novelty = 0.5),
            IntakeReportError::SubjectNovelty(_)
        ));
        assert!(matches!(
            broken(&|r| {
                r.subject_novelty[0].similarity = f64::INFINITY;
            }),
            IntakeReportError::SubjectNovelty(_)
        ));
        assert!(matches!(
            broken(&|r| r.novelty = Some(1.0)),
            IntakeReportError::Novelty
        ));
        assert!(matches!(
            broken(&|r| {
                r.reason_codes.clear();
                r.requires_reason = false;
            }),
            IntakeReportError::ReasonCode(ExactEquivalent)
        ));
        assert!(matches!(
            broken(&|r| r.requires_reason = false),
            IntakeReportError::RequiresReason
        ));
        assert!(matches!(
            broken(&|r| r.validation_gates = vec![GateId::F1, GateId::I0]),
            IntakeReportError::Unordered { .. }
        ));
        assert!(matches!(
            broken(&|r| r.validation_gates = vec![GateId::I0, GateId::I0]),
            IntakeReportError::Unordered { .. }
        ));
        assert!(matches!(
            broken(&|r| r.term_resolutions.push(TermResolution {
                term_ref: id("term:x"),
                key: None,
                matched_term_refs: vec![],
                status: TermResolutionStatus::New,
            })),
            IntakeReportError::TermResolution(_)
        ));
    }

    #[test]
    fn report_validation_permits_a_same_id_coverage_match_without_reason() {
        // Not reachable through the frozen subject rule (Hotfix 059a): the report contract
        // still permits it and it adds no exact_equivalent reason.
        let g = graph(vec![req(
            "requirement:a",
            Accepted,
            "Managers approve leave.",
        )]);
        let p = proposal(&g, add(req("requirement:n", Proposed, "Payroll exports.")));
        let mut report = run(&g, &p);
        report.covered_by = vec![coverage("requirement:a", "requirement:a")];
        report.validate().unwrap();
        assert!(!report.requires_reason);
    }

    // ------------------------------------------------------------------ rationale guard

    fn decision_node(
        status: ElementStatus,
        proposal_ref: Option<&Id>,
        rationale: Option<&str>,
    ) -> Node {
        node(
            "decision:d",
            status,
            NodePayload::ResolutionDecision(ResolutionDecision {
                question_ref: Some(id("question:q")),
                proposal_ref: proposal_ref.cloned(),
                answer: json!({"accept": true}),
                decided_by: id("agent:owner"),
                decided_at: ts(AT),
                patch_ref: Hash::content_sha256(b"patch"),
                rationale: rationale.map(str::to_owned),
                supersedes: None,
            }),
        )
    }

    #[test]
    fn rationale_guard_outcomes() {
        let g = graph(vec![req(
            "requirement:a",
            Accepted,
            "Managers approve leave.",
        )]);
        let plain = proposal(&g, add(req("requirement:n", Proposed, "Payroll exports.")));
        let plain_report = run(&g, &plain);
        assert!(!plain_report.requires_reason);
        // No reason: no decision needed.
        assert_eq!(
            validate_required_rationale(&plain, &plain_report, None),
            Ok(())
        );
        let p = proposal_as(
            &g,
            add(req("requirement:m", Proposed, "Payroll exports.")),
            ProposalMateriality::MaterialDecision,
            AcceptancePolicy::HumanDecision,
        );
        let report = run(&g, &p);
        assert!(report.requires_reason);
        // Report of another proposal.
        assert_eq!(
            validate_required_rationale(&p, &plain_report, None),
            Err(IntakeAcceptanceError::ReportProposalMismatch {
                proposal: p.id.clone(),
                report: plain.id.clone(),
            })
        );
        assert_eq!(
            validate_required_rationale(&p, &report, None),
            Err(IntakeAcceptanceError::ReasonRequired(p.id.clone()))
        );
        let ok = decision_node(Accepted, Some(&p.id), Some("Owner accepts the trade-off."));
        let decision = id("decision:d");
        assert_eq!(
            validate_required_rationale(
                &p,
                &report,
                Some(&decision_node(Proposed, Some(&p.id), Some("Fine.")))
            ),
            Err(IntakeAcceptanceError::DecisionNotAccepted(decision.clone()))
        );
        let mut wrong_type = ok.clone();
        wrong_type.payload =
            NodePayload::Requirement(requirement_payload("Not a decision.", Modality::Shall));
        assert_eq!(
            validate_required_rationale(&p, &report, Some(&wrong_type)),
            Err(IntakeAcceptanceError::DecisionWrongType(decision.clone()))
        );
        for bound in [None, Some(&plain.id)] {
            assert_eq!(
                validate_required_rationale(
                    &p,
                    &report,
                    Some(&decision_node(Accepted, bound, Some("Fine.")))
                ),
                Err(IntakeAcceptanceError::DecisionProposalMismatch {
                    decision: decision.clone(),
                    proposal: p.id.clone(),
                })
            );
        }
        for rationale in [None, Some(""), Some("  \n\t ")] {
            assert_eq!(
                validate_required_rationale(
                    &p,
                    &report,
                    Some(&decision_node(Accepted, Some(&p.id), rationale))
                ),
                Err(IntakeAcceptanceError::MissingRationale(decision.clone()))
            );
        }
        let before = ok.clone();
        assert_eq!(validate_required_rationale(&p, &report, Some(&ok)), Ok(()));
        assert_eq!(ok, before);
        // A decision is not needed, but tolerated, when no reason is required.
        let other = decision_node(Accepted, Some(&plain.id), None);
        assert_eq!(
            validate_required_rationale(&plain, &plain_report, Some(&other)),
            Ok(())
        );
    }

    // ------------------------------------------------------------------ source guards

    fn assert_absent(source: &str, name: &str, forbidden: &[&str]) {
        for token in forbidden {
            assert!(!source.contains(token), "{name} must not contain {token:?}");
        }
    }

    #[test]
    fn intake_source_guards() {
        let capabilities = [
            "GraphStore",
            "RevisionStore",
            "ArtifactStore",
            "BlobStore",
            "rusqlite",
            "std::fs",
            "std::net",
            "reqwest",
            "tokio",
            "InferenceProvider",
            "AnthropicProvider",
            "InferenceRequest",
            "Clock",
            "SystemTime",
            "Instant",
            "Utc::now",
            "Embedding",
            "embedding",
            "cosine",
            "rand",
            "HashMap",
            "commit",
            "Regex",
            "regex",
            ".confidence",
            "created_at",
            "evaluated_at",
            "intake_at",
        ];
        for (name, src) in [
            ("intake.rs", INTAKE_SRC),
            ("report.rs", REPORT_SRC),
            ("lib.rs", LIB_SRC),
        ] {
            assert_absent(src, name, &capabilities);
        }
        // No second validator: no rule IDs or rule-specific semantics in production intake.
        let rule_semantics = [
            "PLUMB.",
            "ISO29148.",
            "NO_CONTRADICTION",
            "NO_DUPLICATE",
            "I_NUMERIC_RANGE",
            "I_MODALITY",
            "I_CARD_CONFLICT",
            "evaluate_gate",
            "EvaluatorRegistry",
            "register_",
        ];
        // No textual semantic heuristics or alternate BM25, tokenizer or impact traversal.
        let heuristics = [
            "contains(\"",
            "\"not\"",
            "\"shall\"",
            "to_lowercase",
            "is_alphanumeric",
            "BM25_K1",
            "BM25_B",
            ".ln(",
            "CorpusStats",
            "TermCounts",
            "outgoing",
            "incoming",
            "edges_from",
            "edges_to",
            "DirtySet {",
            "is_subset",
            "is_superset",
            "starts_with",
        ];
        for (name, src) in [("intake.rs", INTAKE_SRC), ("report.rs", REPORT_SRC)] {
            assert_absent(src, name, &rule_semantics);
            assert_absent(src, name, &heuristics);
        }
        // The only X1.3 change to index.rs is the crate-visible projector.
        assert!(INDEX_SRC.contains("pub(crate) fn document_text("));
        assert_absent(
            INDEX_SRC,
            "index.rs",
            &[
                "pub fn document_text",
                "Proposal",
                "apply_patch",
                "IntakeReport",
            ],
        );
        // Crate-internal X1.2 types are not exported.
        assert_absent(
            LIB_SRC,
            "lib.rs",
            &[
                "document_text",
                "IndexedDocument",
                "CorpusStats",
                "TermCounts",
            ],
        );
        // Dry run only through the patch engine.
        assert!(INTAKE_SRC.contains("apply_patch(graph, &proposal.patch_set)"));
        assert!(INTAKE_SRC.contains("compute_impact(graph, candidate, delta)"));
    }

    // ------------------------------------------------------------------ H0.1 expression limits

    /// The eight expression-bearing fields (compiler architecture §33.1), each carrying
    /// `expression` while every other expression field stays absent or trivial.
    fn expression_payloads(expression: &str) -> Vec<(&'static str, NodePayload)> {
        let e = || expression.to_owned();
        let transition = |guard_expr| {
            NodePayload::Transition(Transition {
                stateful_ref: id("entity:leave"),
                from_state: id("state:draft"),
                to_state: id("state:submitted"),
                guard_expr,
                effect_refs: None,
            })
        };
        let process_node = |condition_expr, timer_expr| {
            NodePayload::ProcessNode(ProcessNode {
                process_ref: id("process:leave"),
                node_kind: ProcessNodeKind::ExclusiveGateway,
                operation_ref: None,
                condition_expr,
                message_ref: None,
                timer_expr,
            })
        };
        let rule = |condition_expr, action_expr| {
            NodePayload::Rule(Rule {
                name: "Leave rule".to_owned(),
                rule_kind: RuleKind::Business,
                condition_expr,
                action_expr,
            })
        };
        vec![
            ("Transition.guard_expr", transition(Some(e()))),
            (
                "Invariant.expression",
                NodePayload::Invariant(Invariant {
                    scope_ref: id("entity:leave"),
                    expression: e(),
                }),
            ),
            ("ProcessNode.condition_expr", process_node(Some(e()), None)),
            ("ProcessNode.timer_expr", process_node(None, Some(e()))),
            ("Rule.condition_expr", rule(Some(e()), None)),
            ("Rule.action_expr", rule(None, Some(e()))),
            (
                "Calculation.expression",
                NodePayload::Calculation(Calculation {
                    name: "Leave days".to_owned(),
                    expression: e(),
                    result_type: "integer".to_owned(),
                    unit: None,
                    rounding: None,
                    calendar_ref: None,
                    examples: None,
                }),
            ),
            (
                "PolicyCondition.expression",
                NodePayload::PolicyCondition(PolicyCondition { expression: e() }),
            ),
        ]
    }

    /// Over-limit expressions: one per limit, with the exact typed error each produces.
    fn over_limit_expressions() -> Vec<(String, IntakeError)> {
        let limit = |limit, actual, max| IntakeError::ExpressionLimit { limit, actual, max };
        vec![
            ("a".repeat(4097), limit("EXPR_MAX_SOURCE_BYTES", 4097, 4096)),
            (
                format!("{}a{}", "(".repeat(65), ")".repeat(65)),
                limit("EXPR_MAX_NESTING_DEPTH", 65, 64),
            ),
            (
                format!("{}a", "not ".repeat(65)),
                limit("EXPR_MAX_NESTING_DEPTH", 65, 64),
            ),
            (
                format!("a{}", " + a".repeat(512)),
                limit("EXPR_MAX_AST_NODES", 1025, 1024),
            ),
        ]
    }

    #[test]
    fn every_expression_field_is_limit_checked_on_add_and_replace() {
        let g = graph(vec![req("req:base", Accepted, "Employees request leave.")]);
        let fields: Vec<&str> = expression_payloads("a").iter().map(|(f, _)| *f).collect();
        assert_eq!(
            fields,
            [
                "Transition.guard_expr",
                "Invariant.expression",
                "ProcessNode.condition_expr",
                "ProcessNode.timer_expr",
                "Rule.condition_expr",
                "Rule.action_expr",
                "Calculation.expression",
                "PolicyCondition.expression",
            ]
        );
        for (expression, expected) in over_limit_expressions() {
            for (field, payload) in expression_payloads(&expression) {
                let ingress = [
                    ("AddNode", add(node("node:new", Proposed, payload.clone()))),
                    (
                        "ReplacePayload",
                        replace_payload(&g, "req:base", payload.clone()),
                    ),
                    (
                        "nested Compound",
                        compound(vec![
                            add(req("req:other", Proposed, "Managers approve leave.")),
                            compound(vec![add(node("node:new", Proposed, payload.clone()))]),
                        ]),
                    ),
                ];
                for (form, patch) in ingress {
                    let p = proposal(&g, patch);
                    let error = intake(&g, &p, &IntakeValidationInput::empty()).unwrap_err();
                    assert_eq!(error, expected, "{field} via {form}");
                    // The error never carries the expression source.
                    assert!(!error.to_string().contains("aaaa"), "{field} via {form}");
                    assert!(!error.to_string().contains("(((("), "{field} via {form}");
                }
            }
        }
        // The guard precedes the dry run: a stale base would otherwise be a patch error.
        let (expression, expected) = over_limit_expressions().remove(1);
        let stale = graph(vec![req("req:stale", Accepted, "Something else.")]);
        let (_, payload) = expression_payloads(&expression).remove(7);
        let p = proposal(&stale, add(node("node:new", Proposed, payload)));
        assert_eq!(
            intake(&g, &p, &IntakeValidationInput::empty()).unwrap_err(),
            expected
        );
    }

    #[test]
    fn below_limit_expressions_keep_existing_intake_behavior() {
        let g = graph(vec![req("req:base", Accepted, "Employees request leave.")]);
        // At-limit, valid, and below-limit syntax errors are not intake errors: intake still
        // owns no expression syntax policy.
        let at_limit_bytes = format!("\"{}\"", "x".repeat(4094));
        let at_limit_nodes = format!("a{}", " + a".repeat(511));
        for expression in [
            "requested_days <= 26",
            at_limit_bytes.as_str(),
            at_limit_nodes.as_str(),
            "a +",
            "",
            "((a",
            "1.",
        ] {
            for (payload_name, payload) in [
                (
                    "PolicyCondition",
                    NodePayload::PolicyCondition(PolicyCondition {
                        expression: expression.to_owned(),
                    }),
                ),
                (
                    "Rule",
                    NodePayload::Rule(Rule {
                        name: "Leave rule".to_owned(),
                        rule_kind: RuleKind::Business,
                        condition_expr: Some(expression.to_owned()),
                        action_expr: None,
                    }),
                ),
            ] {
                let p = proposal(&g, add(node("node:new", Proposed, payload)));
                let report = intake(&g, &p, &IntakeValidationInput::empty())
                    .unwrap_or_else(|e| panic!("{payload_name} {expression:?}: {e}"));
                assert_eq!(report, run(&g, &p), "{payload_name} {expression:?}");
            }
        }
        // Unrelated text fields are not expressions.
        let long = "word ".repeat(2000);
        let p = proposal(&g, add(req("req:long", Proposed, long.trim())));
        run(&g, &p);
    }
}
