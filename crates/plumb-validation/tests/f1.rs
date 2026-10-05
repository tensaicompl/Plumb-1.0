//! S1.6 contract tests for the F1 supplemental inputs and the eleven F1 evaluators.
//!
//! Every test lives in `f1_contract` so that `cargo test -p plumb-validation f1` selects them.
//! Golden values were computed independently with Python `hashlib` (RFC 8785 JSON and the
//! finding-key byte formula), never with the helpers under test. The HR graph is compiled in
//! test code through the existing S0.1, S0.4, S1.1 and S1.5 contracts (test-only
//! dependencies); production validation never depends on plumb-functional.

mod f1_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, GateId, Hash, Id, Timestamp};
    use plumb_import::{
        build_segmentation_request, evaluate_segmentation, import_markdown, import_plain_text,
        ImportAudit, SegmentationAudit,
    };
    use plumb_lint::{LintInput, LintPolicy, LintRuleId, LintSeverity, LintTextRange};
    use plumb_psg::{
        AcceptanceCriterion, Agent, AgentKind, AuditMeta, Concept, ConceptKind, Concern,
        DerivationRef, Edge, ElementStatus, Finding, FindingSeverity, Goal, Graph, Modality, Need,
        Node, NodePayload, Operation, OperationKind, Process, RelationKind, RelationProperties,
        Requirement, RequirementKind, RequirementLevel, ResolutionDecision, Stakeholder, Term,
    };
    use plumb_validation::*;
    use serde_json::{json, Value};

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const HR_MD: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.md");

    const STATEMENT_PRESENT: &str = "ISO29148.F1.REQ.STATEMENT_PRESENT";
    const GROUNDED: &str = "PLUMB.F1.REQ.GROUNDED";
    const LINEAGE: &str = "ISO29148.F1.REQ.LINEAGE";
    const TYPE_KNOWN: &str = "PLUMB.F1.REQ.TYPE_KNOWN";
    const NO_DUPLICATE: &str = "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED";
    const NO_CONTRADICTION: &str = "PLUMB.F1.REQ.NO_CONTRADICTION";
    const TERMS: &str = "PLUMB.F1.REQ.TERMS_RESOLVED";
    const MODALITY: &str = "PLUMB.F1.REQ.MODALITY_EXPLICIT";
    const CRITERIA: &str = "PLUMB.F1.REQ.CRITERIA_FOR_BEHAVIOR";
    const QUALITY: &str = "PLUMB.F1.REQ.QUALITY_FINDINGS_CLEAR";
    const SUPERSEDED: &str = "PLUMB.F1.REQ.SUPERSEDED_EXCLUDED";

    // Independently computed goldens (Python hashlib).
    const GOLDEN_INPUT_HASH: &str =
        "sha256:c3a92447963cd8ce8574558be4e0999c7dd32172b80552989bb8def7100250aa";
    const GOLDEN_STATEMENT_KEY: &str =
        "sha256:13f8045c10d0b351dfe619c1dcf90711d1c4dfac7108c3154089188f2ef0fdcb";
    const GOLDEN_STATEMENT_FINDING: &str = "fnd:13f8045c10d0b351";

    const HUMAN: &str = "agent:alice";
    const BOT: &str = "agent:bot";
    const ANCHOR: &str = "fnd:00000000000000a0";
    const A: &str = "req:0000000000000a01";
    const B: &str = "req:0000000000000a02";
    const C: &str = "req:0000000000000a03";

    use ElementStatus::{Accepted, Proposed};

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn audit() -> AuditMeta {
        AuditMeta::new(id(HUMAN), ts(AT), None, None).unwrap()
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
            audit: audit(),
        }
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

    fn requirement(
        statement: &str,
        kind: RequirementKind,
        level: RequirementLevel,
        modality: Modality,
    ) -> Requirement {
        Requirement {
            statement: statement.to_owned(),
            requirement_kind: kind,
            level,
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

    /// An Accepted operational stakeholder shall requirement with one evidence ref: passes F1.
    fn req_node(node_id: &str, statement: &str, evidence: &[&Id]) -> Node {
        req_with(
            node_id,
            requirement(
                statement,
                RequirementKind::Operational,
                RequirementLevel::Stakeholder,
                Modality::Shall,
            ),
            evidence,
        )
    }

    fn req_with(node_id: &str, r: Requirement, evidence: &[&Id]) -> Node {
        let mut n = node(node_id, Accepted, NodePayload::Requirement(r));
        n.evidence = evidence.iter().map(|e| (*e).clone().into()).collect();
        n
    }

    fn agent(node_id: &str, kind: AgentKind) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Agent(Agent { agent_kind: kind }),
        )
    }

    fn finding_node(
        node_id: &str,
        status: ElementStatus,
        code: &str,
        open: bool,
        affected: &[&str],
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Finding(Finding {
                code: code.to_owned(),
                family: "F1".to_owned(),
                severity: FindingSeverity::Blocker,
                message: "Synthetic finding.".to_owned(),
                status: if open { "Open" } else { "Resolved" }.to_owned(),
                affected_refs: affected.iter().map(|a| id(a)).collect(),
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            }),
        )
    }

    fn decision(
        node_id: &str,
        status: ElementStatus,
        answer: Value,
        rationale: Option<&str>,
        decided_by: &str,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::ResolutionDecision(ResolutionDecision {
                question_ref: None,
                proposal_ref: Some(id("prop:0000000000000001")),
                answer,
                decided_by: id(decided_by),
                decided_at: ts(AT),
                patch_ref: Hash::content_sha256(b"patch"),
                rationale: rationale.map(str::to_owned),
                supersedes: None,
            }),
        )
    }

    /// Base nodes: imported evidence fragments, a human and a software Agent, and the
    /// resolved anchor finding governed decisions resolve.
    fn base() -> (Vec<Node>, Vec<Id>) {
        let audit = ImportAudit {
            created_by: id(HUMAN),
            created_at: ts(AT),
        };
        let text = (0..4)
            .map(|i| format!("Evidence paragraph {i}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let imported = import_plain_text("evidence.txt", text.as_bytes(), &audit).unwrap();
        let fragments = imported.fragments.iter().map(|f| f.id.clone()).collect();
        let mut nodes = vec![imported.source];
        nodes.extend(imported.fragments);
        nodes.push(agent(HUMAN, AgentKind::Human));
        nodes.push(agent(BOT, AgentKind::SoftwareService));
        nodes.push(finding_node(ANCHOR, Accepted, GROUNDED, false, &[]));
        (nodes, fragments)
    }

    /// A governed decision node plus its resolves edge to the anchor finding.
    fn governed(node_id: &str, answer: Value) -> (Node, Edge) {
        governed_with(
            node_id,
            Accepted,
            answer,
            Some("Governed by the product owner."),
            HUMAN,
        )
    }

    fn governed_with(
        node_id: &str,
        status: ElementStatus,
        answer: Value,
        rationale: Option<&str>,
        by: &str,
    ) -> (Node, Edge) {
        let edge_status = if plumb_psg::is_baseline(status) {
            Accepted
        } else {
            Proposed
        };
        (
            decision(node_id, status, answer, rationale, by),
            edge(
                &format!("rel:{}", &node_id[4..]),
                RelationKind::Resolves,
                node_id,
                ANCHOR,
                edge_status,
            ),
        )
    }

    fn graph(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn statement_of(graph: &Graph, req: &Id) -> String {
        match &graph.node(req).unwrap().payload {
            NodePayload::Requirement(r) => r.statement.clone(),
            other => panic!("{other:?}"),
        }
    }

    fn lint_input(graph: &Graph, req: &Id) -> LintInput {
        let node = graph.node(req).unwrap();
        let evidence: BTreeSet<Id> = node.evidence.iter().map(|e| e.as_id().clone()).collect();
        LintInput {
            requirement_ref: req.clone(),
            statement: statement_of(graph, req),
            evidence_refs: evidence.into_iter().collect(),
            source_anchor: None,
            term_context: None,
        }
    }

    fn accepted_requirements(graph: &Graph) -> Vec<Id> {
        graph
            .nodes()
            .values()
            .filter(|n| n.status == Accepted && matches!(n.payload, NodePayload::Requirement(_)))
            .map(|n| n.id.clone())
            .collect()
    }

    /// Canonical F1 inputs for every Accepted Requirement of `graph`.
    fn inputs_with(
        graph: &Graph,
        deps: BTreeMap<Id, Vec<F1VocabularyDependency>>,
        findings: Vec<GeneratedFinding>,
        policy: LintPolicy,
    ) -> F1ValidationInputs {
        let requirements = accepted_requirements(graph)
            .into_iter()
            .map(|r| F1RequirementValidationInput {
                vocabulary_dependencies: deps.get(&r).cloned().unwrap_or_default(),
                lint_input: lint_input(graph, &r),
                requirement_ref: r,
            })
            .collect();
        F1ValidationInputs::new(requirements, findings, policy)
    }

    fn inputs(graph: &Graph) -> F1ValidationInputs {
        inputs_with(graph, BTreeMap::new(), Vec::new(), LintPolicy::default())
    }

    fn metadata() -> ValidationRegistry {
        ValidationRegistry::new(load_builtin_software_profile().unwrap()).unwrap()
    }

    fn registry() -> EvaluatorRegistry {
        let mut registry = EvaluatorRegistry::new(metadata());
        rules::register_i0_evaluators(&mut registry).unwrap();
        rules::register_f1_evaluators(&mut registry).unwrap();
        registry
    }

    fn base_context(graph: &Graph, policy: ValidationPolicy) -> ValidationContext {
        ValidationContext::new(
            graph,
            &metadata(),
            policy,
            graph.evidence_hash().unwrap(),
            vec![],
            vec![],
        )
        .unwrap()
    }

    fn context(
        graph: &Graph,
        inputs: F1ValidationInputs,
    ) -> Result<ValidationContext, EvaluationError> {
        base_context(graph, ValidationPolicy::default()).with_f1_inputs(graph, inputs)
    }

    fn evaluate_with(
        graph: &Graph,
        inputs: F1ValidationInputs,
        policy: ValidationPolicy,
    ) -> Result<GateReport, EvaluationError> {
        let ctx = base_context(graph, policy).with_f1_inputs(graph, inputs)?;
        registry().evaluate_gate(GateId::F1, graph, &ctx)
    }

    fn evaluate(graph: &Graph, inputs: F1ValidationInputs) -> GateReport {
        evaluate_with(graph, inputs, ValidationPolicy::default()).unwrap()
    }

    fn rule<'a>(report: &'a GateReport, rule_id: &str) -> &'a RuleResult {
        report.rules.iter().find(|r| r.rule_id == rule_id).unwrap()
    }

    fn state(report: &GateReport, rule_id: &str) -> RuleResultState {
        rule(report, rule_id).state
    }

    fn targets(report: &GateReport, rule_id: &str) -> Vec<String> {
        rule(report, rule_id)
            .targets
            .iter()
            .map(Id::to_string)
            .collect()
    }

    use RuleResultState::{Fail, NotApplicable, Pass};

    /// A graph of base nodes plus `extra` nodes/edges and its default F1 report.
    fn run(extra: Vec<Node>, edges: Vec<Edge>) -> (Graph, GateReport) {
        let (mut nodes, _) = base();
        nodes.extend(extra);
        let g = graph(nodes, edges);
        let report = evaluate(&g, inputs(&g));
        (g, report)
    }

    fn fragments() -> Vec<Id> {
        base().1
    }

    fn generated(code: &str, affected: &[&str], condition: &str) -> GeneratedFinding {
        let profile = load_builtin_software_profile().unwrap();
        let rule = profile.rules.iter().find(|r| r.id == code).unwrap();
        let targets: Vec<Id> = affected.iter().map(|a| id(a)).collect();
        GeneratedFinding::for_violation(
            rule,
            rule.severity,
            &ViolationFacts {
                targets: &targets,
                semantic_condition_key: condition,
                message: "Upstream finding.",
                suggested_resolution: None,
            },
            None,
        )
        .unwrap()
    }

    // ------------------------------------------------------------------ context extension

    #[test]
    fn f1_requires_supplemental_inputs() {
        let ev = fragments();
        let (mut nodes, _) = base();
        nodes.push(req_node(A, "The operator shall retain records.", &[&ev[0]]));
        let g = graph(nodes, vec![]);
        let ctx = base_context(&g, ValidationPolicy::default());
        assert_eq!(ctx.f1_inputs, None);
        assert_eq!(
            registry().evaluate_gate(GateId::F1, &g, &ctx),
            Err(EvaluationError::InvalidContext(
                "F1 validation inputs are required".into()
            ))
        );
        let with = ctx.clone().with_f1_inputs(&g, inputs(&g)).unwrap();
        assert!(with.f1_inputs.is_some());
        // The context's wire form omits absent F1 inputs.
        assert!(serde_json::to_value(&ctx)
            .unwrap()
            .get("f1_inputs")
            .is_none());
    }

    #[test]
    fn f1_synthetic_fully_resolved_graph_passes() {
        let ev = fragments();
        let (_, report) = run(
            vec![req_node(A, "The operator shall retain records.", &[&ev[0]])],
            vec![],
        );
        let expected = [
            (STATEMENT_PRESENT, Pass),
            (GROUNDED, Pass),
            (LINEAGE, NotApplicable),
            (TYPE_KNOWN, Pass),
            (NO_DUPLICATE, Pass),
            (NO_CONTRADICTION, Pass),
            (TERMS, Pass),
            (MODALITY, Pass),
            (CRITERIA, NotApplicable),
            (QUALITY, Pass),
            (SUPERSEDED, Pass),
        ];
        for (rule_id, s) in expected {
            assert_eq!(state(&report, rule_id), s, "{rule_id}");
        }
        assert_eq!(report.result, GateResult::Pass);
        assert_eq!(report.gate, GateId::F1);
        assert!(report.findings.is_empty());
        let ids: Vec<&str> = report.rules.iter().map(|r| r.rule_id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "lexicographic rule order");
        assert_eq!(ids.len(), 11);
        assert_eq!(
            rule(&report, LINEAGE).applicability,
            Applicability::NotApplicable {
                reason:
                    "No accepted non-stakeholder requirement requires intent-lineage validation."
                        .into()
            }
        );
        assert_eq!(
            rule(&report, CRITERIA).applicability,
            Applicability::NotApplicable {
                reason: "No accepted behavioral requirement is in the F1 implementation scope."
                    .into()
            }
        );
    }

    #[test]
    fn f1_empty_requirement_set() {
        let (_, report) = run(vec![], vec![]);
        for rule_id in [
            STATEMENT_PRESENT,
            GROUNDED,
            TYPE_KNOWN,
            NO_DUPLICATE,
            NO_CONTRADICTION,
            TERMS,
            MODALITY,
            QUALITY,
            SUPERSEDED,
        ] {
            assert_eq!(state(&report, rule_id), Pass, "{rule_id}");
            assert!(rule(&report, rule_id).targets.is_empty());
        }
        assert_eq!(state(&report, LINEAGE), NotApplicable);
        assert_eq!(state(&report, CRITERIA), NotApplicable);
    }

    #[test]
    fn f1_input_hash_golden_and_report_binding() {
        let ev = fragments();
        let (mut nodes, _) = base();
        nodes.push(req_node(A, "The operator shall retain records.", &[&ev[0]]));
        let g = graph(nodes, vec![]);
        let i = inputs(&g);
        assert_eq!(
            i.content_hash().unwrap(),
            Hash::content_sha256(&to_canonical_json(&serde_json::to_value(&i).unwrap()).unwrap())
        );
        assert_eq!(
            serde_json::to_value(&i).unwrap(),
            json!({"requirements": [{"requirement_ref": A, "vocabulary_dependencies": [],
                "lint_input": {"requirement_ref": A, "statement": "The operator shall retain records.",
                    "evidence_refs": [ev[0]], "source_anchor": null, "term_context": null}}],
                "analysis_findings": [], "lint_policy": {"severity_overrides": {}}})
        );
        assert_eq!(ev[0].as_str(), "evd:57b7485b8b58db90");
        assert_eq!(i.content_hash().unwrap().as_str(), GOLDEN_INPUT_HASH);
        let report = evaluate(&g, i.clone());
        assert_eq!(report.f1_input_hash, Some(i.content_hash().unwrap()));
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["f1_input_hash"], json!(GOLDEN_INPUT_HASH));
        let round: GateReport = serde_json::from_value(value).unwrap();
        assert_eq!(round, report);
        // Sensitivity: one dependency, one policy override, one analysis finding.
        let start = statement_of(&g, &id(A)).find("records").unwrap() as u64;
        let dep = F1VocabularyDependency {
            normalized_key: "record".into(),
            range: LintTextRange {
                start,
                end: start + 7,
            },
            resolution: F1VocabularyResolution::Unresolved,
        };
        let mut policy = LintPolicy::default();
        policy
            .severity_overrides
            .insert(LintRuleId::NumberNoUnit, LintSeverity::Warn);
        let variants = [
            inputs_with(
                &g,
                BTreeMap::from([(id(A), vec![dep])]),
                vec![],
                LintPolicy::default(),
            ),
            inputs_with(&g, BTreeMap::new(), vec![], policy),
            inputs_with(
                &g,
                BTreeMap::new(),
                vec![generated(NO_CONTRADICTION, &[A], "synthetic")],
                LintPolicy::default(),
            ),
        ];
        for v in variants {
            assert_ne!(v.content_hash().unwrap(), i.content_hash().unwrap());
            let r = evaluate(&g, v.clone());
            assert_eq!(r.baseline_semantic_hash, report.baseline_semantic_hash);
            assert_ne!(r.content_hash().unwrap(), report.content_hash().unwrap());
        }
        // A non-F1 report with an F1 hash, or an F1 report without one, is invalid.
        let mut tampered = serde_json::to_value(&report).unwrap();
        tampered.as_object_mut().unwrap().remove("f1_input_hash");
        assert!(serde_json::from_value::<GateReport>(tampered).is_err());
    }

    #[test]
    fn f1_inputs_do_not_affect_i0() {
        let ev = fragments();
        let (mut nodes, _) = base();
        nodes.push(req_node(A, "The operator shall retain records.", &[&ev[0]]));
        let g = graph(nodes, vec![]);
        let plain = base_context(&g, ValidationPolicy::default());
        let with = plain.clone().with_f1_inputs(&g, inputs(&g)).unwrap();
        let a = registry().evaluate_gate(GateId::I0, &g, &plain).unwrap();
        let b = registry().evaluate_gate(GateId::I0, &g, &with).unwrap();
        assert_eq!(a.f1_input_hash, None);
        assert_eq!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&b).unwrap()
        );
        assert!(serde_json::to_value(&a)
            .unwrap()
            .get("f1_input_hash")
            .is_none());
    }

    // ------------------------------------------------------------------ input integrity

    fn invalid(g: &Graph, i: F1ValidationInputs) {
        assert!(matches!(
            context(g, i),
            Err(EvaluationError::InvalidContext(_))
        ));
    }

    #[test]
    fn f1_input_integrity() {
        let ev = fragments();
        let (mut nodes, _) = base();
        nodes.push(req_node(A, "The operator shall retain records.", &[&ev[0]]));
        nodes.push(req_node(
            B,
            "The operator shall archive records.",
            &[&ev[1], &ev[2]],
        ));
        let mut old = req_node(C, "The operator shall file records.", &[&ev[3]]);
        old.status = ElementStatus::Superseded;
        nodes.push(old);
        let g = graph(nodes, vec![]);
        let good = inputs(&g);
        assert!(context(&g, good.clone()).is_ok());
        // Missing and extra entries.
        let mut missing = good.clone();
        missing.requirements.pop();
        invalid(&g, missing);
        let mut extra = good.clone();
        extra.requirements.push(F1RequirementValidationInput {
            requirement_ref: id(C),
            vocabulary_dependencies: vec![],
            lint_input: lint_input(&g, &id(C)),
        });
        invalid(&g, extra);
        // Unsorted wire input is rejected; the constructor canonicalizes.
        let mut unsorted = good.clone();
        unsorted.requirements.reverse();
        invalid(&g, unsorted.clone());
        assert_eq!(
            F1ValidationInputs::new(unsorted.requirements, vec![], LintPolicy::default()),
            good
        );
        // Stale statement, stale evidence, term context, foreign lint ref.
        let mutate = |f: &dyn Fn(&mut LintInput)| {
            let mut i = good.clone();
            f(&mut i.requirements[0].lint_input);
            i
        };
        invalid(&g, mutate(&|l| l.statement.push('!')));
        invalid(&g, mutate(&|l| l.evidence_refs.pop().map(|_| ()).unwrap()));
        invalid(&g, mutate(&|l| l.evidence_refs = vec![ev[3].clone()]));
        invalid(&g, mutate(&|l| l.requirement_ref = id(B)));
        invalid(
            &g,
            mutate(&|l| {
                l.term_context = Some(plumb_lint::TermLintContext {
                    defined_term_keys: BTreeSet::new(),
                    mentions: vec![],
                })
            }),
        );
        // Dependency ranges and keys.
        let s = statement_of(&g, &id(A));
        let dep = |key: &str, start: usize, end: usize| F1VocabularyDependency {
            normalized_key: key.into(),
            range: LintTextRange {
                start: start as u64,
                end: end as u64,
            },
            resolution: F1VocabularyResolution::Unresolved,
        };
        let r = s.find("records").unwrap();
        let o = s.find("operator").unwrap();
        for deps in [
            vec![dep("record", r, r + 7), dep("operator", o, o + 8)],
            vec![dep("record", r, r + 7), dep("record", r, r + 7)],
            vec![
                dep("operator", o, o + 8),
                dep("operator role", o + 2, o + 10),
            ],
            vec![dep(" record", r, r + 7)],
            vec![dep("record", r, r + 99)],
        ] {
            let mut i = good.clone();
            i.requirements[0].vocabulary_dependencies = deps;
            invalid(&g, i);
        }
        let mut adjacent = good.clone();
        adjacent.requirements[0].vocabulary_dependencies =
            vec![dep("operator", o, o + 4), dep("ator", o + 4, o + 8)];
        assert!(context(&g, adjacent).is_ok());
    }

    #[test]
    fn f1_analysis_finding_integrity() {
        let ev = fragments();
        let (mut nodes, _) = base();
        nodes.push(req_node(A, "The operator shall retain records.", &[&ev[0]]));
        nodes.push(req_node(
            B,
            "The operator shall archive records.",
            &[&ev[1]],
        ));
        let g = graph(nodes, vec![]);
        let f = generated(NO_DUPLICATE, &[A, B], "active_requirement_duplicate");
        assert!(context(
            &g,
            inputs_with(&g, BTreeMap::new(), vec![f.clone()], LintPolicy::default())
        )
        .is_ok());
        let tamper = |m: &dyn Fn(&mut GeneratedFinding)| {
            let mut t = f.clone();
            m(&mut t);
            inputs_with(&g, BTreeMap::new(), vec![t], LintPolicy::default())
        };
        invalid(&g, tamper(&|t| t.id = id("fnd:0000000000000001")));
        invalid(&g, tamper(&|t| t.key = Hash::content_sha256(b"other")));
        invalid(&g, tamper(&|t| t.semantic_condition_key = "other".into()));
        invalid(&g, tamper(&|t| t.payload.affected_refs = vec![id(A)]));
        invalid(
            &g,
            tamper(&|t| t.payload.affected_refs = vec![id(B), id(A)]),
        );
        invalid(&g, tamper(&|t| t.payload.family = "F2".into()));
        let lint_code = {
            let mut x = generated(QUALITY, &[A], "lint");
            x.payload.code = QUALITY.into();
            x
        };
        invalid(
            &g,
            inputs_with(&g, BTreeMap::new(), vec![lint_code], LintPolicy::default()),
        );
        invalid(
            &g,
            inputs_with(
                &g,
                BTreeMap::new(),
                vec![generated(NO_DUPLICATE, &[A, HUMAN], "x")],
                LintPolicy::default(),
            ),
        );
        let mut unsorted = inputs_with(
            &g,
            BTreeMap::new(),
            vec![f.clone(), generated(NO_CONTRADICTION, &[A], "c")],
            LintPolicy::default(),
        );
        unsorted.analysis_findings.reverse();
        invalid(&g, unsorted);
    }

    // ------------------------------------------------------------------ STATEMENT_PRESENT

    #[test]
    fn f1_statement_present() {
        let ev = fragments();
        let (_, report) = run(
            vec![req_node(A, "The system shall log.", &[&ev[0]])],
            vec![],
        );
        assert_eq!(state(&report, STATEMENT_PRESENT), Pass);
        assert_eq!(targets(&report, STATEMENT_PRESENT), [A]);
        let (_, report) = run(
            vec![
                req_node(A, "The operator shall retain records.", &[&ev[0]]),
                req_node(B, "   \t\n", &[&ev[1]]),
            ],
            vec![],
        );
        let r = rule(&report, STATEMENT_PRESENT);
        assert_eq!(r.state, Fail);
        assert_eq!(targets(&report, STATEMENT_PRESENT), [B]);
        assert_eq!(
            r.semantic_condition_key.as_deref(),
            Some("accepted_requirement_statement_missing")
        );
        let f = report
            .findings
            .iter()
            .find(|f| Some(&f.id) == r.finding_ref.as_ref())
            .unwrap();
        assert_eq!(
            f.payload.message,
            "Accepted requirements must have a non-empty statement."
        );
        assert_eq!(
            f.payload.suggested_resolution.as_deref(),
            Some("Provide an explicit requirement statement grounded in authoritative evidence.")
        );
        // Independent finding identity: rule 0x00 target 0x00 condition.
        assert_eq!(f.key.as_str(), GOLDEN_STATEMENT_KEY);
        assert_eq!(f.id.as_str(), GOLDEN_STATEMENT_FINDING);
    }

    // ------------------------------------------------------------------ GROUNDED

    #[test]
    fn f1_grounded() {
        let ev = fragments();
        let ungrounded = || req_node(A, "The operator shall retain records.", &[]);
        // A. Node.evidence alone.
        let (_, r) = run(
            vec![req_node(A, "The operator shall retain records.", &[&ev[0]])],
            vec![],
        );
        assert_eq!(state(&r, GROUNDED), Pass);
        // B. evidenced_by edge.
        let (_, r) = run(
            vec![ungrounded()],
            vec![edge(
                "rel:00000000000000e1",
                RelationKind::EvidencedBy,
                A,
                ev[0].as_str(),
                Accepted,
            )],
        );
        assert_eq!(state(&r, GROUNDED), Pass);
        // C. derived_from path through a Need.
        let need = node(
            "need:00000000000000n1",
            Accepted,
            NodePayload::Need(Need {
                statement: "Records are needed.".into(),
                stakeholder_refs: vec![id("stakeholder:ops")],
                goal_refs: None,
                context: None,
            }),
        );
        let stakeholder = node(
            "stakeholder:ops",
            Accepted,
            NodePayload::Stakeholder(Stakeholder {
                name: "Operations".into(),
                stakeholder_kind: "role".into(),
                organization: None,
                responsibilities: None,
                contact_ref: None,
            }),
        );
        let mut need_with_evidence = need.clone();
        need_with_evidence.evidence = vec![ev[1].clone().into()];
        let (_, r) = run(
            vec![
                ungrounded(),
                need_with_evidence.clone(),
                stakeholder.clone(),
            ],
            vec![
                edge(
                    "rel:00000000000000e2",
                    RelationKind::DerivedFrom,
                    A,
                    "need:00000000000000n1",
                    Accepted,
                ),
                edge(
                    "rel:00000000000000e3",
                    RelationKind::DerivedFrom,
                    "need:00000000000000n1",
                    ev[1].as_str(),
                    Accepted,
                ),
            ],
        );
        assert_eq!(state(&r, GROUNDED), Pass);
        // D. governed human-origin decision.
        let (d, e) = governed(
            "dec:00000000000000d1",
            json!({"kind": "human_requirement_origin", "requirement_ref": A}),
        );
        let (_, r) = run(vec![ungrounded(), d], vec![e]);
        assert_eq!(state(&r, GROUNDED), Pass);
        // FAIL: nothing; a Proposed edge does not count.
        let (_, r) = run(vec![ungrounded()], vec![]);
        assert_eq!(state(&r, GROUNDED), Fail);
        assert_eq!(targets(&r, GROUNDED), [A]);
        assert_eq!(
            rule(&r, GROUNDED).semantic_condition_key.as_deref(),
            Some("accepted_requirement_grounding_missing")
        );
    }

    #[test]
    fn f1_governed_decision_checks() {
        let ev = fragments();
        let ungrounded = || req_node(A, "The operator shall retain records.", &[]);
        let marker = || json!({"kind": "human_requirement_origin", "requirement_ref": A});
        let try_graph = |d: Node, e: Edge| {
            let (mut nodes, _) = base();
            nodes.push(ungrounded());
            nodes.push(req_node(
                B,
                "The operator shall archive records.",
                &[&ev[0]],
            ));
            nodes.push(d);
            let g = graph(nodes, vec![e]);
            context(&g, inputs(&g))
                .map(|ctx| registry().evaluate_gate(GateId::F1, &g, &ctx).unwrap())
        };
        // Claimed but ungoverned or malformed markers are context errors.
        for (d, e) in [
            governed_with("dec:00000000000000d1", Accepted, marker(), None, HUMAN),
            governed_with(
                "dec:00000000000000d1",
                Accepted,
                marker(),
                Some("  "),
                HUMAN,
            ),
            governed_with("dec:00000000000000d1", Accepted, marker(), Some("Ok."), BOT),
            governed_with(
                "dec:00000000000000d1",
                Accepted,
                json!({"kind": "human_requirement_origin", "requirement_ref": A, "extra": 1}),
                Some("Ok."),
                HUMAN,
            ),
            governed_with(
                "dec:00000000000000d1",
                Accepted,
                json!({"kind": "deferred_verification"}),
                Some("Ok."),
                HUMAN,
            ),
        ] {
            assert!(matches!(
                try_graph(d, e),
                Err(EvaluationError::InvalidContext(_))
            ));
        }
        // Non-Accepted decisions, wrong targets and unrelated kinds do not qualify.
        for (d, e) in [
            governed_with(
                "dec:00000000000000d1",
                Proposed,
                marker(),
                Some("Ok."),
                HUMAN,
            ),
            governed_with(
                "dec:00000000000000d1",
                Accepted,
                json!({"kind": "human_requirement_origin", "requirement_ref": B}),
                Some("Ok."),
                HUMAN,
            ),
            governed_with(
                "dec:00000000000000d1",
                Accepted,
                json!({"kind": "Human_Requirement_Origin", "requirement_ref": A}),
                Some("Ok."),
                HUMAN,
            ),
            governed_with(
                "dec:00000000000000d1",
                Accepted,
                json!({"kind": "cardinality"}),
                Some("Ok."),
                HUMAN,
            ),
        ] {
            let report = try_graph(d, e).unwrap();
            assert_eq!(state(&report, GROUNDED), Fail);
        }
    }

    // ------------------------------------------------------------------ LINEAGE

    fn software(node_id: &str, statement: &str, ev: &Id) -> Node {
        req_with(
            node_id,
            requirement(
                statement,
                RequirementKind::Operational,
                RequirementLevel::Software,
                Modality::Shall,
            ),
            &[ev],
        )
    }

    #[test]
    fn f1_lineage() {
        let ev = fragments();
        let goal = node(
            "goal:00000000000000g1",
            Accepted,
            NodePayload::Goal(Goal {
                statement: "Keep records.".into(),
                success_measures: None,
                priority: None,
            }),
        );
        let concern = node(
            "concern:000000000000c1",
            Accepted,
            NodePayload::Concern(Concern {
                name: "Records".into(),
                description: "Records matter.".into(),
            }),
        );
        let stakeholder = node(
            "stakeholder:ops",
            Accepted,
            NodePayload::Stakeholder(Stakeholder {
                name: "Operations".into(),
                stakeholder_kind: "role".into(),
                organization: None,
                responsibilities: None,
                contact_ref: None,
            }),
        );
        let need = node(
            "need:00000000000000n1",
            Accepted,
            NodePayload::Need(Need {
                statement: "Records are needed.".into(),
                stakeholder_refs: vec![id("stakeholder:ops")],
                goal_refs: None,
                context: None,
            }),
        );
        let child = || software(A, "The system shall retain records.", &ev[0]);
        let parent = || software(B, "The system shall manage records.", &ev[1]);
        let lineage_of = |extra: Vec<Node>, edges: Vec<Edge>| {
            let (_, r) = run(extra, edges);
            (state(&r, LINEAGE), targets(&r, LINEAGE))
        };
        assert_eq!(
            lineage_of(
                vec![child(), goal.clone()],
                vec![edge(
                    "rel:00000000000000l1",
                    RelationKind::Addresses,
                    A,
                    "goal:00000000000000g1",
                    Accepted
                )]
            )
            .0,
            Pass
        );
        assert_eq!(
            lineage_of(
                vec![child(), concern.clone()],
                vec![edge(
                    "rel:00000000000000l1",
                    RelationKind::Addresses,
                    A,
                    "concern:000000000000c1",
                    Accepted
                )]
            )
            .0,
            Pass
        );
        let mut stakeholder_parent = parent();
        if let NodePayload::Requirement(r) = &mut stakeholder_parent.payload {
            r.level = RequirementLevel::Stakeholder;
        }
        assert_eq!(
            lineage_of(
                vec![child(), stakeholder_parent.clone()],
                vec![edge(
                    "rel:00000000000000l1",
                    RelationKind::Refines,
                    A,
                    B,
                    Accepted
                )]
            )
            .0,
            Pass
        );
        assert_eq!(
            lineage_of(
                vec![child(), stakeholder_parent.clone()],
                vec![edge(
                    "rel:00000000000000l1",
                    RelationKind::DecomposesTo,
                    B,
                    A,
                    Accepted
                )]
            )
            .0,
            Pass
        );
        assert_eq!(
            lineage_of(
                vec![child(), need.clone(), stakeholder.clone()],
                vec![edge(
                    "rel:00000000000000l1",
                    RelationKind::DerivedFrom,
                    A,
                    "need:00000000000000n1",
                    Accepted
                )]
            )
            .0,
            Pass
        );
        let (d, e) = governed(
            "dec:00000000000000d1",
            json!({"kind": "human_requirement_origin", "requirement_ref": A}),
        );
        assert_eq!(lineage_of(vec![child(), d], vec![e]).0, Pass);
        // segment_origin is provenance, not intent lineage.
        let mut with_origin = child();
        with_origin.extensions.insert(
            "plumb_functional:segment_origin".parse().unwrap(),
            json!({"candidate_ref": "seg:0000000000000001", "fragment_ref": ev[0], "start": 0, "end": 5}),
        );
        let (s, t) = lineage_of(vec![with_origin], vec![]);
        assert_eq!((s, t), (Fail, vec![A.to_owned()]));
        let (_, r) = run(vec![child()], vec![]);
        assert_eq!(
            rule(&r, LINEAGE).semantic_condition_key.as_deref(),
            Some("accepted_requirement_lineage_missing")
        );
        // A Proposed goal does not count.
        let mut proposed_goal = goal.clone();
        proposed_goal.status = Proposed;
        assert_eq!(
            lineage_of(
                vec![child(), proposed_goal],
                vec![edge(
                    "rel:00000000000000l1",
                    RelationKind::Addresses,
                    A,
                    "goal:00000000000000g1",
                    Proposed
                )]
            )
            .0,
            Fail
        );
        // Stakeholder roots alone are not applicable.
        assert_eq!(
            lineage_of(vec![stakeholder_parent], vec![]).0,
            NotApplicable
        );
    }

    // ------------------------------------------------------------------ TYPE_KNOWN

    #[test]
    fn f1_type_known() {
        let ev = fragments();
        let (_, r) = run(
            vec![
                req_node(A, "The operator shall retain records.", &[&ev[0]]),
                software(B, "The system shall log.", &ev[1]),
            ],
            vec![],
        );
        assert_eq!(state(&r, TYPE_KNOWN), Pass);
        assert_eq!(targets(&r, TYPE_KNOWN), [A, B]);
        // Unknown values cannot enter a typed Requirement, so no valid graph can violate it.
        let raw = json!({"statement": "x shall y.", "requirement_kind": "business_requirement", "level": "system", "modality": "shall",
            "title": null, "rationale": null, "priority": null, "source_identifier": null, "verification_method": null, "owner_refs": null, "stakeholder_refs": null});
        assert!(serde_json::from_value::<Requirement>(raw.clone()).is_err());
        let mut bad_level = raw;
        bad_level["requirement_kind"] = json!("functional");
        bad_level["level"] = json!("application");
        assert!(serde_json::from_value::<Requirement>(bad_level.clone()).is_err());
        bad_level["level"] = json!("system");
        assert!(serde_json::from_value::<Requirement>(bad_level).is_ok());
    }

    // ------------------------------------------------------------------ finding-state rules

    #[test]
    fn f1_duplicate_and_contradiction_findings() {
        let ev = fragments();
        let (mut nodes, _) = base();
        nodes.push(req_node(A, "The operator shall retain records.", &[&ev[0]]));
        nodes.push(req_node(B, "The operator shall keep records.", &[&ev[1]]));
        let mut old = req_node(C, "The operator shall store records.", &[&ev[2]]);
        old.status = ElementStatus::Superseded;
        nodes.push(old);
        let dup = generated(NO_DUPLICATE, &[A, B], "active_requirement_duplicate");
        let g = graph(nodes.clone(), vec![]);
        // Transient analysis finding.
        let r = evaluate(
            &g,
            inputs_with(
                &g,
                BTreeMap::new(),
                vec![dup.clone()],
                LintPolicy::default(),
            ),
        );
        assert_eq!(state(&r, NO_DUPLICATE), Fail);
        assert_eq!(targets(&r, NO_DUPLICATE), [A, B]);
        assert_eq!(rule(&r, NO_DUPLICATE).evidence, vec![dup.id.to_string()]);
        assert_eq!(
            rule(&r, NO_DUPLICATE).semantic_condition_key.as_deref(),
            Some("accepted_requirement_duplicates_open")
        );
        // Materialized Accepted Open Finding node with the same ID; counted once.
        let mut materialized = nodes.clone();
        materialized.push(finding_node(
            dup.id.as_str(),
            Accepted,
            NO_DUPLICATE,
            true,
            &[A, B],
        ));
        let gm = graph(materialized, vec![]);
        let r = evaluate(&gm, inputs(&gm));
        assert_eq!(state(&r, NO_DUPLICATE), Fail);
        let r = evaluate(
            &gm,
            inputs_with(
                &gm,
                BTreeMap::new(),
                vec![dup.clone()],
                LintPolicy::default(),
            ),
        );
        assert_eq!(rule(&r, NO_DUPLICATE).evidence, vec![dup.id.to_string()]);
        // A resolved or Proposed materialized finding does not block.
        for (status, open) in [(Accepted, false), (Proposed, true)] {
            let mut n = nodes.clone();
            n.push(finding_node(
                dup.id.as_str(),
                status,
                NO_DUPLICATE,
                open,
                &[A, B],
            ));
            let gx = graph(n, vec![]);
            assert_eq!(state(&evaluate(&gx, inputs(&gx)), NO_DUPLICATE), Pass);
        }
        // A finding naming a now Superseded requirement is stale.
        let stale = generated(NO_DUPLICATE, &[A, C], "active_requirement_duplicate");
        let r = evaluate(
            &g,
            inputs_with(&g, BTreeMap::new(), vec![stale], LintPolicy::default()),
        );
        assert_eq!(state(&r, NO_DUPLICATE), Pass);
        // Contradictions: finding state only.
        let contradiction = generated(NO_CONTRADICTION, &[A, B], "synthetic_contradiction");
        let r = evaluate(
            &g,
            inputs_with(
                &g,
                BTreeMap::new(),
                vec![contradiction.clone()],
                LintPolicy::default(),
            ),
        );
        assert_eq!(state(&r, NO_CONTRADICTION), Fail);
        assert_eq!(
            rule(&r, NO_CONTRADICTION).semantic_condition_key.as_deref(),
            Some("accepted_requirement_contradictions_open")
        );
        assert_eq!(
            rule(&r, NO_CONTRADICTION).evidence,
            vec![contradiction.id.to_string()]
        );
        // Superficially opposite statements without finding material pass.
        let (_, r) = run(
            vec![
                req_node(A, "The operator shall retain records.", &[&ev[0]]),
                req_node(B, "The operator shall not retain records.", &[&ev[1]]),
            ],
            vec![],
        );
        assert_eq!(state(&r, NO_CONTRADICTION), Pass);
    }

    // ------------------------------------------------------------------ TERMS_RESOLVED

    #[test]
    fn f1_terms_resolved() {
        let ev = fragments();
        let statement = "The operator shall retain the IBAN records.";
        let term = node(
            "term:00000000000000t1",
            Accepted,
            NodePayload::Term(Term {
                term: "record".into(),
                language: "en".into(),
                definition_ref: None,
                aliases: None,
                status: None,
            }),
        );
        let concept = node(
            "concept:000000000000c1",
            Accepted,
            NodePayload::Concept(Concept {
                name: "record".into(),
                definition: "A kept entry.".into(),
                concept_kind: ConceptKind::ObjectType,
            }),
        );
        let run_terms = |extra: Vec<Node>,
                         edges: Vec<Edge>,
                         resolution: F1VocabularyResolution,
                         key: &str,
                         part: &str| {
            let (mut nodes, _) = base();
            nodes.push(req_node(A, statement, &[&ev[0]]));
            nodes.extend(extra);
            let g = graph(nodes, edges);
            let start = statement.find(part).unwrap() as u64;
            let dep = F1VocabularyDependency {
                normalized_key: key.into(),
                range: LintTextRange {
                    start,
                    end: start + part.len() as u64,
                },
                resolution,
            };
            let i = inputs_with(
                &g,
                BTreeMap::from([(id(A), vec![dep])]),
                vec![],
                LintPolicy::default(),
            );
            context(&g, i.clone())
                .map(|ctx| registry().evaluate_gate(GateId::F1, &g, &ctx).unwrap())
        };
        let r = run_terms(
            vec![],
            vec![],
            F1VocabularyResolution::Unresolved,
            "record",
            "records",
        )
        .unwrap();
        assert_eq!(state(&r, TERMS), Fail);
        assert_eq!(targets(&r, TERMS), [A]);
        let start = statement.find("records").unwrap();
        assert_eq!(
            rule(&r, TERMS).evidence,
            vec![format!("term:{A}:{start}:{}:record", start + 7)]
        );
        assert_eq!(
            rule(&r, TERMS).semantic_condition_key.as_deref(),
            Some("accepted_requirement_terms_unresolved")
        );
        assert_eq!(
            state(
                &run_terms(
                    vec![term.clone()],
                    vec![],
                    F1VocabularyResolution::AcceptedTerm {
                        term_ref: id("term:00000000000000t1")
                    },
                    "record",
                    "records"
                )
                .unwrap(),
                TERMS
            ),
            Pass
        );
        assert_eq!(
            state(
                &run_terms(
                    vec![concept.clone()],
                    vec![],
                    F1VocabularyResolution::AcceptedConcept {
                        concept_ref: id("concept:000000000000c1")
                    },
                    "record",
                    "records"
                )
                .unwrap(),
                TERMS
            ),
            Pass
        );
        // Stale or wrong-type claims are context errors.
        let mut proposed_term = term.clone();
        proposed_term.status = Proposed;
        assert!(matches!(
            run_terms(
                vec![proposed_term],
                vec![],
                F1VocabularyResolution::AcceptedTerm {
                    term_ref: id("term:00000000000000t1")
                },
                "record",
                "records"
            ),
            Err(EvaluationError::InvalidContext(_))
        ));
        assert!(matches!(
            run_terms(
                vec![concept.clone()],
                vec![],
                F1VocabularyResolution::AcceptedTerm {
                    term_ref: id("concept:000000000000c1")
                },
                "record",
                "records"
            ),
            Err(EvaluationError::InvalidContext(_))
        ));
        // Governed external identifier.
        let ext = |key: &str, identifier: &str, req: &str, by: &str| {
            governed_with(
                "dec:00000000000000x1",
                Accepted,
                json!({"kind": "external_vocabulary_identifier", "requirement_ref": req, "normalized_key": key, "identifier": identifier}),
                Some("Use the standard identifier."),
                by,
            )
        };
        let resolution = || F1VocabularyResolution::ExternalIdentifier {
            decision_ref: id("dec:00000000000000x1"),
            identifier: "ISO 13616 IBAN".into(),
        };
        let (d, e) = ext("iban", "ISO 13616 IBAN", A, HUMAN);
        assert_eq!(
            state(
                &run_terms(vec![d], vec![e], resolution(), "iban", "IBAN").unwrap(),
                TERMS
            ),
            Pass
        );
        for (key, identifier, req, by) in [
            ("ibans", "ISO 13616 IBAN", A, HUMAN),
            ("iban", "IBAN", A, HUMAN),
            ("iban", "ISO 13616 IBAN", B, HUMAN),
        ] {
            let (d, e) = ext(key, identifier, req, by);
            assert!(
                matches!(
                    run_terms(vec![d], vec![e], resolution(), "iban", "IBAN"),
                    Err(EvaluationError::InvalidContext(_))
                ),
                "{key} {identifier} {req}"
            );
        }
        let (d, e) = ext("iban", "ISO 13616 IBAN", A, BOT);
        assert!(matches!(
            run_terms(vec![d], vec![e], resolution(), "iban", "IBAN"),
            Err(EvaluationError::InvalidContext(_))
        ));
        // Resolved dependencies but an active TERMS conflict finding still fail.
        let (mut nodes, _) = base();
        nodes.push(req_node(A, statement, &[&ev[0]]));
        nodes.push(term);
        let g = graph(nodes, vec![]);
        let start = statement.find("records").unwrap() as u64;
        let dep = F1VocabularyDependency {
            normalized_key: "record".into(),
            range: LintTextRange {
                start,
                end: start + 7,
            },
            resolution: F1VocabularyResolution::AcceptedTerm {
                term_ref: id("term:00000000000000t1"),
            },
        };
        let conflict = generated(TERMS, &[A], "vocabulary_concept_kind_conflict:record");
        let r = evaluate(
            &g,
            inputs_with(
                &g,
                BTreeMap::from([(id(A), vec![dep])]),
                vec![conflict.clone()],
                LintPolicy::default(),
            ),
        );
        assert_eq!(state(&r, TERMS), Fail);
        assert_eq!(rule(&r, TERMS).evidence, vec![conflict.id.to_string()]);
    }

    // ------------------------------------------------------------------ MODALITY_EXPLICIT

    #[test]
    fn f1_modality_explicit() {
        let ev = fragments();
        let modality = |statement: &str, typed: Modality| {
            let (_, r) = run(
                vec![req_with(
                    A,
                    requirement(
                        statement,
                        RequirementKind::Operational,
                        RequirementLevel::Stakeholder,
                        typed,
                    ),
                    &[&ev[0]],
                )],
                vec![],
            );
            state(&r, MODALITY)
        };
        assert_eq!(modality("The system shall log.", Modality::Shall), Pass);
        assert_eq!(modality("The system should log.", Modality::Shall), Fail);
        assert_eq!(
            modality("The system shall not log.", Modality::ShallNot),
            Pass
        );
        assert_eq!(
            modality("The system SHALL\tNOT log.", Modality::ShallNot),
            Pass
        );
        assert_eq!(modality("The system shall not log.", Modality::Shall), Fail);
        assert_eq!(modality("The system should log.", Modality::Should), Pass);
        assert_eq!(modality("The system may log.", Modality::May), Pass);
        for typed in [
            Modality::Shall,
            Modality::ShallNot,
            Modality::Should,
            Modality::May,
        ] {
            for s in [
                "The system should not log.",
                "The system may not export.",
                "The system must log.",
                "The system must not log.",
                "The system can log.",
                "The system can not log.",
                // Hotfix 032: the first candidate controls, so a later supported word
                // cannot rescue an unsupported controlling reading.
                "The system can ensure that the request shall be stored.",
                "The system must ensure that the request shall be stored.",
                // No controlling modal: `cannot` is not a candidate token.
                "The system logs the request.",
                "The system cannot export.",
            ] {
                assert_eq!(modality(s, typed), Fail, "{s} {typed:?}");
            }
        }
        // Hotfix 032: the immutable HR-010 statement has controlling Shall; the
        // subordinate `can` after it does not participate.
        assert_eq!(
            modality(
                "A submitted leave request shall require a manager decision before it can become Approved or Rejected.",
                Modality::Shall
            ),
            Pass
        );
        assert_eq!(
            modality(
                "The system shall ensure the request can become Approved.",
                Modality::Shall
            ),
            Pass
        );
        assert_eq!(
            modality(
                "The system shall ensure the user cannot export restricted data.",
                Modality::Shall
            ),
            Pass
        );
        // Only the primary controlling modal is the pilot contract; secondary-clause
        // deontic contradictions are detected upstream and reach F1 only as governed
        // NO_CONTRADICTION finding material, so the later `should` is out of scope.
        assert_eq!(
            modality(
                "The system shall validate and should store.",
                Modality::Shall
            ),
            Pass
        );
        // A `not` separated by anything other than space, tab, CR or LF does not negate.
        assert_eq!(
            modality("The system shall, not log.", Modality::ShallNot),
            Fail
        );
        assert_eq!(
            modality("The system shall, not log.", Modality::Shall),
            Pass
        );
        assert_eq!(
            modality("The system shall\r\n not log.", Modality::ShallNot),
            Pass
        );
        assert_eq!(
            modality(
                "The system shall validate and shall store.",
                Modality::Shall
            ),
            Pass
        );
        let (_, r) = run(
            vec![req_with(
                A,
                requirement(
                    "The system must log.",
                    RequirementKind::Operational,
                    RequirementLevel::Stakeholder,
                    Modality::Shall,
                ),
                &[&ev[0]],
            )],
            vec![],
        );
        assert_eq!(
            rule(&r, MODALITY).semantic_condition_key.as_deref(),
            Some("accepted_requirement_modality_inconsistent")
        );
    }

    // ------------------------------------------------------------------ CRITERIA_FOR_BEHAVIOR

    #[test]
    fn f1_criteria_for_behavior() {
        let ev = fragments();
        let functional = || {
            req_with(
                A,
                requirement(
                    "The system shall log.",
                    RequirementKind::Functional,
                    RequirementLevel::Stakeholder,
                    Modality::Shall,
                ),
                &[&ev[0]],
            )
        };
        let criterion = node(
            "criterion:00000000001",
            Accepted,
            NodePayload::AcceptanceCriterion(AcceptanceCriterion {
                statement: "A log entry exists.".into(),
                criterion_kind: "example".into(),
                verification_method: None,
                measure_ref: None,
                scenario_ref: None,
            }),
        );
        let operation = node(
            "operation:000000000001",
            Accepted,
            NodePayload::Operation(Operation {
                name: "Log".into(),
                operation_kind: OperationKind::Command,
                input_schema_ref: None,
                output_schema_ref: None,
                preconditions: None,
                postconditions: None,
                idempotency: None,
                transaction_semantics: None,
            }),
        );
        let process = node(
            "process:00000000000001",
            Accepted,
            NodePayload::Process(Process {
                name: "Logging".into(),
                description: None,
                process_kind: None,
            }),
        );
        let criteria = |extra: Vec<Node>, edges: Vec<Edge>| {
            let (_, r) = run(extra, edges);
            (state(&r, CRITERIA), targets(&r, CRITERIA))
        };
        assert_eq!(
            criteria(
                vec![functional(), criterion.clone()],
                vec![edge(
                    "rel:00000000000000k1",
                    RelationKind::DerivedFrom,
                    "criterion:00000000001",
                    A,
                    Accepted
                )]
            )
            .0,
            Pass
        );
        assert_eq!(
            criteria(
                vec![functional(), operation],
                vec![edge(
                    "rel:00000000000000k1",
                    RelationKind::SpecifiedBy,
                    A,
                    "operation:000000000001",
                    Accepted
                )]
            )
            .0,
            Pass
        );
        assert_eq!(
            criteria(
                vec![functional(), process],
                vec![edge(
                    "rel:00000000000000k1",
                    RelationKind::SpecifiedBy,
                    A,
                    "process:00000000000001",
                    Accepted
                )]
            )
            .0,
            Pass
        );
        let (d, e) = governed(
            "dec:00000000000000v1",
            json!({"kind": "deferred_verification", "requirement_ref": A}),
        );
        assert_eq!(criteria(vec![functional(), d], vec![e]).0, Pass);
        assert_eq!(
            criteria(vec![functional()], vec![]),
            (Fail, vec![A.to_owned()])
        );
        let mut proposed = criterion;
        proposed.status = Proposed;
        assert_eq!(
            criteria(
                vec![functional(), proposed],
                vec![edge(
                    "rel:00000000000000k1",
                    RelationKind::DerivedFrom,
                    "criterion:00000000001",
                    A,
                    Proposed
                )]
            )
            .0,
            Fail
        );
        let data = req_with(
            B,
            requirement(
                "The system shall keep data.",
                RequirementKind::Data,
                RequirementLevel::Stakeholder,
                Modality::Shall,
            ),
            &[&ev[1]],
        );
        assert_eq!(criteria(vec![data], vec![]).0, NotApplicable);
        let (_, r) = run(vec![functional()], vec![]);
        assert_eq!(
            rule(&r, CRITERIA).semantic_condition_key.as_deref(),
            Some("behavior_requirement_criteria_missing")
        );
    }

    // ------------------------------------------------------------------ QUALITY_FINDINGS_CLEAR

    #[test]
    fn f1_quality_findings_clear() {
        let ev = fragments();
        let (mut nodes, _) = base();
        nodes.push(req_node(
            A,
            "The operator shall respond within 2 days.",
            &[&ev[0]],
        ));
        nodes.push(req_node(
            B,
            "The operator shall retain several records.",
            &[&ev[1]],
        ));
        let g = graph(nodes, vec![]);
        let r = evaluate(&g, inputs(&g));
        assert_eq!(state(&r, QUALITY), Fail);
        assert_eq!(
            targets(&r, QUALITY),
            [A],
            "warn-only AMBIGUOUS_QUANTIFIER does not block"
        );
        let s = statement_of(&g, &id(A));
        let start = s.find("within").unwrap();
        assert_eq!(
            rule(&r, QUALITY).evidence,
            vec![format!(
                "lint:PLUMB.LINT.REQ.RELATIVE_TIME_NO_ANCHOR:{A}:{start}:{}",
                start + "within 2 days".len()
            )]
        );
        assert_eq!(
            rule(&r, QUALITY).semantic_condition_key.as_deref(),
            Some("accepted_requirement_quality_errors_open")
        );
        let mut policy = LintPolicy::default();
        policy
            .severity_overrides
            .insert(LintRuleId::RelativeTimeNoAnchor, LintSeverity::Warn);
        let r = evaluate(&g, inputs_with(&g, BTreeMap::new(), vec![], policy));
        assert_eq!(state(&r, QUALITY), Pass);
    }

    #[test]
    fn f1_quality_excludes_undefined_term() {
        // Unresolved vocabulary fails TERMS_RESOLVED only; UNDEFINED_TERM is not evaluated.
        let ev = fragments();
        let statement = "The operator shall retain records.";
        let (mut nodes, _) = base();
        nodes.push(req_node(A, statement, &[&ev[0]]));
        let g = graph(nodes, vec![]);
        let start = statement.find("records").unwrap() as u64;
        let dep = F1VocabularyDependency {
            normalized_key: "record".into(),
            range: LintTextRange {
                start,
                end: start + 7,
            },
            resolution: F1VocabularyResolution::Unresolved,
        };
        let r = evaluate(
            &g,
            inputs_with(
                &g,
                BTreeMap::from([(id(A), vec![dep])]),
                vec![],
                LintPolicy::default(),
            ),
        );
        assert_eq!(state(&r, TERMS), Fail);
        assert_eq!(state(&r, QUALITY), Pass);
        let lint =
            plumb_lint::lint_requirement(&lint_input(&g, &id(A)), &LintPolicy::default()).unwrap();
        let undefined = lint
            .evaluations
            .iter()
            .find(|e| e.rule_id == LintRuleId::UndefinedTerm)
            .unwrap();
        assert!(matches!(
            undefined.applicability,
            plumb_lint::LintApplicability::NotEvaluated { .. }
        ));
    }

    // ------------------------------------------------------------------ SUPERSEDED_EXCLUDED

    #[test]
    fn f1_superseded_excluded() {
        let ev = fragments();
        let new = req_node(B, "The operator shall archive records.", &[&ev[1]]);
        let mut old = req_node(A, "The operator shall retain records.", &[&ev[0]]);
        let link = || {
            edge(
                "rel:00000000000000s1",
                RelationKind::Supersedes,
                B,
                A,
                Accepted,
            )
        };
        let (_, r) = run(vec![old.clone(), new.clone()], vec![link()]);
        assert_eq!(state(&r, SUPERSEDED), Fail);
        assert_eq!(targets(&r, SUPERSEDED), [A]);
        assert_eq!(
            rule(&r, SUPERSEDED).semantic_condition_key.as_deref(),
            Some("superseded_requirement_still_active")
        );
        old.status = ElementStatus::Superseded;
        let (_, r) = run(vec![old, new], vec![link()]);
        assert_eq!(state(&r, SUPERSEDED), Pass);
        assert_eq!(targets(&r, SUPERSEDED), [B]);
    }

    // ------------------------------------------------------------------ waivers and registration

    #[test]
    fn f1_aggregate_waivers() {
        let ev = fragments();
        let ungrounded = req_node(A, "The operator shall retain records.", &[]);
        let (_, first) = run(vec![ungrounded.clone()], vec![]);
        let result = rule(&first, GROUNDED);
        let finding = first
            .findings
            .iter()
            .find(|f| Some(&f.id) == result.finding_ref.as_ref())
            .unwrap()
            .clone();
        let waive = |rule_id: &str,
                     finding: &GeneratedFinding,
                     extra: Vec<Node>,
                     policy: ValidationPolicy| {
            let (mut nodes, _) = base();
            nodes.push(ungrounded.clone());
            nodes.extend(extra);
            let mut f = finding_node(finding.id.as_str(), Accepted, rule_id, true, &[]);
            if let NodePayload::Finding(p) = &mut f.payload {
                *p = finding.payload.clone();
            }
            nodes.push(f);
            nodes.push(decision("dec:00000000000000w1", Accepted, json!({"kind": "waiver", "rule_id": rule_id, "finding_key": finding.key, "finding_ref": finding.id}), Some("Accepted risk."), HUMAN));
            let g = graph(
                nodes,
                vec![edge(
                    "rel:00000000000000w1",
                    RelationKind::Resolves,
                    "dec:00000000000000w1",
                    finding.id.as_str(),
                    Accepted,
                )],
            );
            evaluate_with(&g, inputs(&g), policy)
        };
        let waived = waive(GROUNDED, &finding, vec![], ValidationPolicy::default()).unwrap();
        assert_eq!(state(&waived, GROUNDED), RuleResultState::Waived);
        // profile_allow LINEAGE needs policy enablement.
        let (_, lineage_report) = run(
            vec![software(A, "The system shall retain records.", &ev[0])],
            vec![],
        );
        let lineage_finding = lineage_report
            .findings
            .iter()
            .find(|f| f.payload.code == LINEAGE)
            .unwrap()
            .clone();
        let with_lineage = |policy: ValidationPolicy| {
            let (mut nodes, _) = base();
            nodes.push(software(A, "The system shall retain records.", &ev[0]));
            let mut f = finding_node(lineage_finding.id.as_str(), Accepted, LINEAGE, true, &[]);
            if let NodePayload::Finding(p) = &mut f.payload {
                *p = lineage_finding.payload.clone();
            }
            nodes.push(f);
            nodes.push(decision("dec:00000000000000w2", Accepted, json!({"kind": "waiver", "rule_id": LINEAGE, "finding_key": lineage_finding.key, "finding_ref": lineage_finding.id}), Some("Accepted risk."), HUMAN));
            let g = graph(
                nodes,
                vec![edge(
                    "rel:00000000000000w2",
                    RelationKind::Resolves,
                    "dec:00000000000000w2",
                    lineage_finding.id.as_str(),
                    Accepted,
                )],
            );
            evaluate_with(&g, inputs(&g), policy)
        };
        assert!(matches!(
            with_lineage(ValidationPolicy::default()),
            Err(EvaluationError::ProfileWaiverNotEnabled { .. })
        ));
        let enabled = ValidationPolicy {
            promoted_to_blocker_rule_ids: BTreeSet::new(),
            profile_allow_waiver_rule_ids: BTreeSet::from([LINEAGE.to_owned()]),
        };
        assert_eq!(
            state(&with_lineage(enabled).unwrap(), LINEAGE),
            RuleResultState::Waived
        );
        // forbidden STATEMENT_PRESENT.
        let (_, blank) = run(vec![req_node(B, "  ", &[&ev[1]])], vec![]);
        let blank_finding = blank
            .findings
            .iter()
            .find(|f| f.payload.code == STATEMENT_PRESENT)
            .unwrap()
            .clone();
        let (mut nodes, _) = base();
        nodes.push(req_node(B, "  ", &[&ev[1]]));
        let mut f = finding_node(
            blank_finding.id.as_str(),
            Accepted,
            STATEMENT_PRESENT,
            true,
            &[],
        );
        if let NodePayload::Finding(p) = &mut f.payload {
            *p = blank_finding.payload.clone();
        }
        nodes.push(f);
        nodes.push(decision("dec:00000000000000w3", Accepted, json!({"kind": "waiver", "rule_id": STATEMENT_PRESENT, "finding_key": blank_finding.key, "finding_ref": blank_finding.id}), Some("No."), HUMAN));
        let g = graph(
            nodes,
            vec![edge(
                "rel:00000000000000w3",
                RelationKind::Resolves,
                "dec:00000000000000w3",
                blank_finding.id.as_str(),
                Accepted,
            )],
        );
        assert!(matches!(
            evaluate_with(&g, inputs(&g), ValidationPolicy::default()),
            Err(EvaluationError::ForbiddenWaiver { .. })
        ));
    }

    #[test]
    fn f1_registration() {
        let mut registry = EvaluatorRegistry::new(metadata());
        assert_eq!(registry.missing_for_gate(GateId::I0).len(), 6);
        assert_eq!(registry.missing_for_gate(GateId::F1).len(), 11);
        rules::register_i0_evaluators(&mut registry).unwrap();
        assert!(registry.missing_for_gate(GateId::I0).is_empty());
        assert_eq!(registry.missing_for_gate(GateId::F1).len(), 11);
        rules::register_f1_evaluators(&mut registry).unwrap();
        assert!(registry.missing_for_gate(GateId::F1).is_empty());
        for gate in [GateId::F2, GateId::F3, GateId::F4] {
            assert_eq!(
                registry.missing_for_gate(gate).len(),
                registry.metadata().gate(gate).rule_ids.len()
            );
        }
        let f1: Vec<String> = registry.metadata().gate(GateId::F1).rule_ids.clone();
        assert_eq!(f1.len(), 11);
        assert!(f1.iter().all(|r| registry.has_evaluator(r)));
        assert!(matches!(
            rules::register_f1_evaluators(&mut registry),
            Err(EvaluationError::DuplicateEvaluator(_))
        ));
    }

    #[test]
    fn f1_production_source_guard() {
        let source = include_str!("../src/rules/f1.rs");
        for token in [
            "std::fs",
            "File::open",
            "reqwest",
            "InferenceProvider",
            "MockProvider",
            "ArtifactStore",
            "RevisionStore",
            "rusqlite",
            "SystemClock",
            "Utc::now",
            "Instant::now",
            "SemanticPatch",
            "Proposal",
            "apply_patch",
            "commit(",
            "unsafe",
            "jaccard",
            "Jaccard",
            "nfkc",
            "singular",
            "plumb_functional",
            "\"appropriate\"",
            "\"within\"",
            "profile.yaml",
            "requirements.md",
        ] {
            assert!(!source.contains(token), "{token}");
        }
        let mod_rs = include_str!("../src/rules/mod.rs");
        assert!(mod_rs.contains("mod f1;") && !mod_rs.contains("pub mod f1;"));
        assert!(mod_rs.contains("pub use f1::register_f1_evaluators;"));
        assert!(
            !mod_rs.contains("fn ") && !mod_rs.contains("PLUMB.") && !mod_rs.contains("ISO29148.")
        );
    }

    // ------------------------------------------------------------------ HR baseline

    /// The test-only mock provider policy, decoded into whatever policy type the API takes
    /// (plumb-validation names no inference type, even in tests).
    fn provider<T: serde::de::DeserializeOwned>() -> T {
        serde_json::from_value(json!({"provider": "mock", "config": {}})).unwrap()
    }

    /// A mock inference artifact for request `request_id`, decoded into the artifact type the
    /// API takes; its deserialization validates the hashes.
    fn artifact<T: serde::de::DeserializeOwned>(request_id: &Hash, output: Value) -> T {
        let canonical = to_canonical_json(&output).unwrap();
        serde_json::from_value(json!({
            "request_hash": request_id, "provider": "mock", "model": "mock-model", "parameters": {},
            "raw_response_hash": Hash::content_sha256(&canonical),
            "validated_output": output, "validated_output_hash": Hash::content_sha256(&canonical),
        }))
        .unwrap()
    }

    /// The single-source HR graph: requirements.md through S0.1, S0.4 fallback and S1.1 (mock
    /// classifier output: null kinds, fixed level software; not an authoritative HR level), all
    /// 32 Requirements Accepted in test code, plus mock S1.5 vocabulary dependencies.
    fn hr() -> (Graph, F1ValidationInputs) {
        use plumb_functional::*;
        let audit = ImportAudit {
            created_by: id(HUMAN),
            created_at: ts(AT),
        };
        let imported = import_markdown("requirements.md", HR_MD, &audit).unwrap();
        let fragments = imported.fragments.clone();
        let mut nodes = vec![imported.source];
        nodes.extend(imported.fragments);
        let mut g = graph(nodes, vec![]);
        let segmentation = build_segmentation_request(&fragments, provider()).unwrap();
        let seg_audit = SegmentationAudit {
            created_by: id(HUMAN),
            created_at: ts(AT),
        };
        let candidates = evaluate_segmentation(&segmentation, &fragments, None, &seg_audit)
            .unwrap()
            .candidates;
        let request =
            build_requirement_classification_request(&g, &candidates, provider()).unwrap();
        let entries: Vec<Value> = request.context.candidates.iter().map(|c| json!({"candidate_ref": c.candidate_ref, "requirement_kind": null, "level": "software"})).collect();
        let classification = artifact(
            &request.request.id,
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
                created_by: id(HUMAN),
                created_at: ts(AT),
            },
        )
        .unwrap();
        assert_eq!(compiled.proposals.len(), 32);
        for p in &compiled.proposals {
            g = plumb_patch::apply_patch(&g, &p.patch_set).unwrap().graph;
        }
        let accepted: Vec<Node> = g
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
        let g = graph(accepted, vec![]);
        let reqs = accepted_requirements(&g);
        assert_eq!(reqs.len(), 32);
        // S1.5 vocabulary analysis with mock spans; new Terms are only Proposed, so every
        // dependency is unresolved.
        let policy = VocabularyPolicy {
            language: "en".into(),
        };
        let vocab = build_vocabulary_request(&g, &reqs, &policy, provider()).unwrap();
        let phrases = [
            "annual-leave balance",
            "leave balance",
            "leave requests",
            "leave request",
            "leave type",
            "start date",
            "end date",
            "approval records",
            "audit record",
            "time zone",
            "employees",
            "employee",
            "managers",
            "manager",
        ];
        let mut mentions = Vec::new();
        for r in &reqs {
            let s = statement_of(&g, r).to_ascii_lowercase();
            let mut taken: Vec<(usize, usize)> = Vec::new();
            for phrase in phrases {
                for (start, _) in s.match_indices(phrase) {
                    let end = start + phrase.len();
                    let bounded = (start == 0 || !s.as_bytes()[start - 1].is_ascii_alphanumeric())
                        && (end == s.len() || !s.as_bytes()[end].is_ascii_alphanumeric());
                    if bounded && taken.iter().all(|(a, b)| end <= *a || start >= *b) {
                        taken.push((start, end));
                        mentions.push(json!({"requirement_ref": r, "start": start, "end": end, "concept_kind": null, "definition": null}));
                    }
                }
            }
        }
        let vocab_artifact = artifact(
            &vocab.request.id,
            json!({"version": 1, "mentions": mentions}),
        );
        let analysis = analyze_vocabulary(
            &g,
            &vocab,
            &policy,
            Some(VocabularyInference {
                artifact: &vocab_artifact,
                derivation_ref: DerivationRef::from(id("drv:00000000000000c2")),
            }),
            &VocabularyAudit {
                created_by: id(HUMAN),
                created_at: ts(AT),
            },
        )
        .unwrap();
        let mut deps: BTreeMap<Id, Vec<F1VocabularyDependency>> = BTreeMap::new();
        for ctx in &analysis.lint_contexts {
            deps.insert(
                ctx.requirement_ref.clone(),
                ctx.context
                    .mentions
                    .iter()
                    .map(|m| F1VocabularyDependency {
                        normalized_key: m.normalized_key.clone(),
                        range: m.range,
                        resolution: F1VocabularyResolution::Unresolved,
                    })
                    .collect(),
            );
        }
        let inputs = inputs_with(&g, deps, analysis.findings.clone(), LintPolicy::default());
        (g, inputs)
    }

    fn source_identifier(g: &Graph, r: &str) -> String {
        match &g.node(&id(r)).unwrap().payload {
            NodePayload::Requirement(req) => req.source_identifier.clone().unwrap(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn f1_hr_baseline() {
        let (g, i) = hr();
        let report = evaluate(&g, i.clone());
        let all = accepted_requirements(&g);
        for (rule_id, expected) in [
            (STATEMENT_PRESENT, Pass),
            (GROUNDED, Pass),
            (TYPE_KNOWN, Pass),
        ] {
            assert_eq!(state(&report, rule_id), expected, "{rule_id}");
            assert_eq!(rule(&report, rule_id).targets, all, "{rule_id}");
        }
        assert_eq!(state(&report, NO_DUPLICATE), Pass);
        assert_eq!(state(&report, NO_CONTRADICTION), Pass);
        assert_eq!(state(&report, SUPERSEDED), Pass);
        assert_eq!(state(&report, LINEAGE), Fail);
        assert_eq!(rule(&report, LINEAGE).targets, all);
        // Behavioral scope verified against the compiled payloads.
        let behavioral: Vec<Id> = all
            .iter()
            .filter(|r| match &g.node(r).unwrap().payload {
                NodePayload::Requirement(req) => matches!(
                    req.requirement_kind,
                    RequirementKind::Functional
                        | RequirementKind::Interface
                        | RequirementKind::Security
                ),
                _ => false,
            })
            .cloned()
            .collect();
        assert_eq!(behavioral.len(), 29);
        assert_eq!(state(&report, CRITERIA), Fail);
        assert_eq!(rule(&report, CRITERIA).targets, behavioral);
        assert_eq!(state(&report, TERMS), Fail);
        // Hotfix 031 §161 as revised by Hotfix 032: all 32 HR statements have a
        // controlling modal matching their typed modality, including HR-010.
        assert_eq!(rule(&report, MODALITY).targets, all);
        let hr_010 = all
            .iter()
            .find(|t| source_identifier(&g, t.as_str()) == "HR-010")
            .expect("HR-010 is an Accepted requirement");
        match &g.node(hr_010).unwrap().payload {
            NodePayload::Requirement(req) => assert_eq!(req.modality, Modality::Shall),
            _ => unreachable!(),
        }
        // HR-010 is not a MODALITY violation target: no finding is generated.
        assert_eq!(rule(&report, MODALITY).finding_ref, None);
        assert!(!report
            .findings
            .iter()
            .any(|f| f.payload.code == MODALITY && f.payload.affected_refs.contains(hr_010)));
        assert_eq!(
            state(&report, MODALITY),
            Pass,
            "HR modality: {:?}",
            rule(&report, MODALITY)
                .targets
                .iter()
                .map(|t| source_identifier(&g, t.as_str()))
                .collect::<Vec<_>>()
        );
        assert_eq!(state(&report, QUALITY), Fail);
        let quality: Vec<String> = rule(&report, QUALITY)
            .targets
            .iter()
            .map(|t| source_identifier(&g, t.as_str()))
            .collect();
        // The complete deterministic quality target set under the frozen S1.4 rules.
        assert_eq!(quality, ["HR-030"]);
        assert_eq!(rule(&report, QUALITY).evidence.len(), 1);
        assert!(rule(&report, QUALITY).evidence[0]
            .starts_with("lint:PLUMB.LINT.REQ.RELATIVE_TIME_NO_ANCHOR:"));
        assert_eq!(report.result, GateResult::Fail);
        // Replay determinism.
        let (g2, i2) = hr();
        assert_eq!(
            serde_json::to_vec(&evaluate(&g2, i2)).unwrap(),
            serde_json::to_vec(&report).unwrap()
        );
    }
}
