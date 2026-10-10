//! S4.5 contract tests for VerificationObligation applicability (Hotfix 054).
//!
//! Every test lives in `verification_contract` (`cargo test -p plumb-functional --test
//! verification`). Graphs are built in memory; the obligation and relation ID goldens were
//! computed independently in Python (hashlib + RFC 8785 canonical JSON).

mod verification_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{Hash, Id, Timestamp};
    use plumb_functional::{
        compile_verification_obligations, relation_id, verification_obligation_id, ScenarioGiven,
        ScenarioThen, ScenarioValue, ScenarioWhen, VerificationAnalysisResult,
        VerificationApplicabilityInputs, VerificationAudit, VerificationDisposition,
        VerificationError, VerificationKind, VerificationObligation, VerificationSourceInput,
        VerificationSourceMode,
    };
    use plumb_patch::{apply_patch, AcceptancePolicy, ProposalMateriality, SemanticPatch};
    use plumb_psg::{
        AuditMeta, Edge, ElementStatus, EvidenceFragment, EvidenceLocator, Graph, Measure,
        Modality, Node, NodePayload, Operation, OperationKind, Process, QualityCharacteristic,
        QualityScenario, RelationKind, RelationProperties, Requirement, RequirementKind,
        RequirementLevel, Rule, RuleKind, Scenario, ScenarioKind, SourceArtifact,
    };
    use serde_json::json;

    use ElementStatus::{Accepted, Proposed};
    use VerificationSourceMode::{ExecutableScenario, NonExecutable};

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const SRC: &str = include_str!("../src/verification.rs");

    const SUBMIT: &str = "operation:submit-leave";
    const APPROVE: &str = "operation:approve-leave";
    const R_LEAVE: &str = "requirement:leave-balance";
    const R_DIRECT: &str = "requirement:direct";
    const R_QUALITY: &str = "requirement:quality-001";

    // Independent Python goldens.
    const GOLDEN_SCENARIO_OBLIGATION: &str = "verification_obligation:bb5a8f8a6d8bb0a1";
    const GOLDEN_ANALYSIS_OBLIGATION: &str = "verification_obligation:480bf2a7e5049076";
    const GOLDEN_VERIFIED_BY: &str = "rel:13b4bda839b47cd3";
    const GOLDEN_IMPLEMENTED_AS: &str = "rel:97aeb5cccd7997b2";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn meta() -> AuditMeta {
        AuditMeta::new(id("actor:human"), ts(AT), None, None).unwrap()
    }

    fn audit() -> VerificationAudit {
        VerificationAudit {
            created_by: id("actor:human"),
            created_at: ts(AT),
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
            audit: meta(),
        }
    }

    #[derive(Clone)]
    struct World {
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    }

    impl World {
        fn add(&mut self, n: Node) -> &mut World {
            self.nodes.retain(|e| e.id != n.id);
            self.nodes.push(n);
            self
        }

        fn link_as(
            &mut self,
            edge_id: Id,
            kind: RelationKind,
            from: &str,
            to: &str,
            status: ElementStatus,
        ) -> &mut World {
            self.edges.retain(|e| e.id != edge_id);
            self.edges.push(Edge {
                id: edge_id,
                revision: 1,
                status,
                kind,
                from: id(from),
                to: id(to),
                properties: RelationProperties::None,
                evidence: Vec::new(),
                derivations: Vec::new(),
                standards: Vec::new(),
                audit: meta(),
            });
            self
        }

        fn link(&mut self, kind: RelationKind, from: &str, to: &str) -> &mut World {
            let edge_id = relation_id(&kind, &id(from), &id(to)).unwrap();
            self.link_as(edge_id, kind, from, to, Accepted)
        }

        fn graph(&self) -> Graph {
            Graph::new(
                id(PROJECT),
                id(PROFILE),
                self.nodes.clone(),
                self.edges.clone(),
            )
            .unwrap()
        }

        fn reversed_graph(&self) -> Graph {
            let mut nodes = self.nodes.clone();
            let mut edges = self.edges.clone();
            nodes.reverse();
            edges.reverse();
            Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap()
        }
    }

    fn source_ref() -> Id {
        plumb_psg::source_artifact_id(&Hash::content_sha256(b"requirements")).unwrap()
    }

    fn fragment(n: u64) -> Id {
        plumb_psg::evidence_fragment_id(&source_ref(), &locator(n)).unwrap()
    }

    fn locator(n: u64) -> EvidenceLocator {
        EvidenceLocator::TextRange {
            start: n * 100,
            end: n * 100 + 50,
        }
    }

    fn requirement(
        w: &mut World,
        node_id: &str,
        n: u64,
        kind: RequirementKind,
        method: Option<&str>,
    ) {
        w.add(node(
            fragment(n).as_str(),
            Accepted,
            NodePayload::EvidenceFragment(EvidenceFragment {
                source_ref: source_ref(),
                locator: locator(n),
                content_hash: Hash::content_sha256(node_id.as_bytes()),
                extracted_text: None,
                speaker: None,
                source_timestamp: None,
            }),
        ));
        let mut r = node(
            node_id,
            Accepted,
            NodePayload::Requirement(Requirement {
                statement: format!("The system shall satisfy {node_id}."),
                requirement_kind: kind,
                level: RequirementLevel::System,
                modality: Modality::Shall,
                title: None,
                rationale: None,
                priority: None,
                source_identifier: None,
                verification_method: method.map(Into::into),
                owner_refs: None,
                stakeholder_refs: None,
            }),
        );
        r.evidence = vec![fragment(n).into()];
        w.add(r);
    }

    fn operation(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Operation(Operation {
                name: node_id.into(),
                operation_kind: OperationKind::Command,
                input_schema_ref: None,
                output_schema_ref: None,
                preconditions: None,
                postconditions: None,
                idempotency: None,
                transaction_semantics: None,
            }),
        )
    }

    fn scenario(
        w: &mut World,
        scenario_ref: &str,
        when: ScenarioWhen,
        requirement_refs: &[&str],
        status: ElementStatus,
        confirmation: &str,
        evidence: Option<u64>,
    ) {
        let encode = |v: &ScenarioValue| serde_json::to_value(v).unwrap();
        let when_value = when.to_value();
        let anchor = id(when_value
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .as_str()
            .unwrap());
        let mut n = node(
            scenario_ref,
            status,
            NodePayload::Scenario(Scenario {
                name: format!("Scenario {scenario_ref}"),
                scenario_kind: ScenarioKind::Acceptance,
                given: ScenarioGiven::<ScenarioValue>::default().to_values(encode),
                when: when_value,
                then: ScenarioThen::<ScenarioValue>::default().to_values(encode),
                requirement_refs: (!requirement_refs.is_empty())
                    .then(|| requirement_refs.iter().map(|r| id(r)).collect()),
                derived_from_refs: Some(vec![anchor]),
                confirmation_status: Some(confirmation.into()),
            }),
        );
        if let Some(e) = evidence {
            n.evidence = vec![fragment(e).into()];
        }
        w.add(n);
    }

    /// Requirements, operations and confirmed scenarios of the base fixture.
    fn base_world() -> World {
        let mut w = World {
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        w.add(node(
            source_ref().as_str(),
            Accepted,
            NodePayload::SourceArtifact(SourceArtifact {
                source_kind: "document".into(),
                display_name: "requirements.md".into(),
                content_hash: Hash::content_sha256(b"requirements"),
                media_type: "text/markdown".into(),
                external_uri: None,
                external_version: None,
                producer: None,
                created_at_source: None,
                language: None,
                classification: None,
            }),
        ));
        requirement(&mut w, R_LEAVE, 1, RequirementKind::Functional, None);
        requirement(&mut w, R_DIRECT, 2, RequirementKind::Functional, None);
        requirement(
            &mut w,
            R_QUALITY,
            3,
            RequirementKind::Quality,
            Some("analysis"),
        );
        w.add(operation(SUBMIT)).add(operation(APPROVE));
        w.link(RelationKind::SpecifiedBy, R_LEAVE, SUBMIT);
        scenario(
            &mut w,
            "scenario:s1",
            ScenarioWhen::Operation(id(SUBMIT)),
            &[],
            Accepted,
            "Confirmed",
            Some(1),
        );
        scenario(
            &mut w,
            "scenario:s2",
            ScenarioWhen::Operation(id(APPROVE)),
            &[R_DIRECT],
            Accepted,
            "Confirmed",
            None,
        );
        w
    }

    fn source(r: &str, mode: VerificationSourceMode, blocking: bool) -> VerificationSourceInput {
        VerificationSourceInput {
            source_ref: id(r),
            mode,
            blocking,
        }
    }

    fn compile(w: &World, sources: Vec<VerificationSourceInput>) -> VerificationAnalysisResult {
        compile_verification_obligations(
            &w.graph(),
            &VerificationApplicabilityInputs { sources },
            &audit(),
        )
        .unwrap()
    }

    fn compile_err(w: &World, sources: Vec<VerificationSourceInput>) -> VerificationError {
        compile_verification_obligations(
            &w.graph(),
            &VerificationApplicabilityInputs { sources },
            &audit(),
        )
        .unwrap_err()
    }

    fn obligation(r: &str, kind: VerificationKind) -> Id {
        verification_obligation_id(&id(PROJECT), &id(r), kind).unwrap()
    }

    fn rel(kind: RelationKind, from: &Id, to: &Id) -> Id {
        relation_id(&kind, from, to).unwrap()
    }

    /// (patch kind, element id) of each proposal patch.
    fn patch_summary(result: &VerificationAnalysisResult, index: usize) -> Vec<(String, Id)> {
        let SemanticPatch::Compound { patches } = &result.proposals[index].patch_set.patch else {
            panic!("expected a Compound patch");
        };
        patches
            .iter()
            .map(|p| match p {
                SemanticPatch::AddNode { node } => ("add_node".to_owned(), node.id.clone()),
                SemanticPatch::AddEdge { edge } => {
                    (format!("add_edge:{}", edge.kind.as_str()), edge.id.clone())
                }
                other => panic!("unexpected patch {other:?}"),
            })
            .collect()
    }

    fn added_obligation(
        result: &VerificationAnalysisResult,
        index: usize,
    ) -> VerificationObligation {
        let SemanticPatch::Compound { patches } = &result.proposals[index].patch_set.patch else {
            panic!("expected a Compound patch");
        };
        patches
            .iter()
            .find_map(|p| match p {
                SemanticPatch::AddNode { node } => match &node.payload {
                    NodePayload::VerificationObligation(o) => Some(o.clone()),
                    _ => None,
                },
                _ => None,
            })
            .unwrap()
    }

    fn ids(items: &[&str]) -> BTreeSet<Id> {
        items.iter().map(|i| id(i)).collect()
    }

    // ------------------------------------------------------------------ non-executable

    #[test]
    fn quality_requirement_gets_an_analysis_obligation_without_scenarios() {
        let w = base_world();
        let result = compile(&w, vec![source(R_QUALITY, NonExecutable, true)]);
        let o = obligation(R_QUALITY, VerificationKind::Analysis);
        assert_eq!(o.to_string(), GOLDEN_ANALYSIS_OBLIGATION);
        assert_eq!(
            patch_summary(&result, 0),
            vec![
                ("add_node".to_owned(), o.clone()),
                (
                    "add_edge:verified_by".to_owned(),
                    rel(RelationKind::VerifiedBy, &id(R_QUALITY), &o)
                ),
            ]
        );
        assert_eq!(
            added_obligation(&result, 0),
            VerificationObligation {
                name: format!("Verify {R_QUALITY} via analysis"),
                verification_kind: VerificationKind::Analysis,
                method: Some("analysis".into()),
                acceptance_condition: None,
                required_evidence_kind: None,
            }
        );
        let item = &result.items[0];
        assert!(item.scenario_refs.is_empty());
        assert_eq!(item.verification_kind, Some(VerificationKind::Analysis));
        let snapshot = &result.applicability;
        assert_eq!(snapshot.source_refs, ids(&[R_QUALITY]));
        assert!(snapshot.executable_source_refs.is_empty());
        assert_eq!(snapshot.blocking_obligation_refs, BTreeSet::from([o]));
        assert!(snapshot.blocking_scenario_refs.is_empty());
        let proposal = &result.proposals[0];
        assert_eq!(proposal.stage, plumb_core::StageId::S4);
        assert_eq!(proposal.materiality, ProposalMateriality::Semantic);
        assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanConfirm);
        assert_eq!(proposal.confidence, None);
        assert!(proposal.derivation_refs.is_empty());
        assert_eq!(proposal.evidence_refs, vec![fragment(3).into()]);
    }

    #[test]
    fn every_explicit_method_maps_exactly_and_kind_never_decides() {
        let cases = [
            (
                "analysis",
                VerificationKind::Analysis,
                RequirementKind::Quality,
            ),
            (
                "inspection",
                VerificationKind::Inspection,
                RequirementKind::Data,
            ),
            (
                "review",
                VerificationKind::Review,
                RequirementKind::Compliance,
            ),
            (
                "demonstration",
                VerificationKind::Demonstration,
                RequirementKind::Operational,
            ),
            (
                "formal_check",
                VerificationKind::FormalCheck,
                RequirementKind::Constraint,
            ),
            (
                "architecture_check",
                VerificationKind::ArchitectureCheck,
                RequirementKind::Interface,
            ),
            (
                "security_check",
                VerificationKind::SecurityCheck,
                RequirementKind::Security,
            ),
            (
                "review",
                VerificationKind::Review,
                RequirementKind::Functional,
            ),
            (
                "analysis",
                VerificationKind::Analysis,
                RequirementKind::Security,
            ),
        ];
        for (method, kind, requirement_kind) in cases {
            let mut w = base_world();
            requirement(&mut w, "requirement:x", 9, requirement_kind, Some(method));
            let result = compile(&w, vec![source("requirement:x", NonExecutable, false)]);
            assert_eq!(result.items[0].verification_kind, Some(kind), "{method}");
            let o = added_obligation(&result, 0);
            assert_eq!(o.method.as_deref(), Some(method));
            assert_eq!(o.verification_kind, kind);
            assert!(patch_summary(&result, 0)
                .iter()
                .all(|(k, _)| k != "add_edge:implemented_as"));
        }
    }

    #[test]
    fn method_gaps_are_dispositions_without_proposals() {
        let cases = [
            (None, VerificationDisposition::MethodDecisionRequired),
            (
                Some("document review"),
                VerificationDisposition::UnsupportedVerificationMethod,
            ),
            (
                Some("Analysis"),
                VerificationDisposition::UnsupportedVerificationMethod,
            ),
            (
                Some("test"),
                VerificationDisposition::VerificationKindDeferred,
            ),
            (
                Some("scenario"),
                VerificationDisposition::IncompatibleVerificationMethod,
            ),
        ];
        for (method, disposition) in cases {
            let mut w = base_world();
            requirement(&mut w, "requirement:x", 9, RequirementKind::Quality, method);
            let result = compile(&w, vec![source("requirement:x", NonExecutable, true)]);
            let item = &result.items[0];
            assert_eq!(item.disposition, disposition, "{method:?}");
            assert_eq!(item.obligation_ref, None);
            assert_eq!(item.verification_kind, None);
            assert!(result.proposals.is_empty());
            assert_eq!(result.applicability.source_refs, ids(&["requirement:x"]));
            assert!(result.applicability.blocking_obligation_refs.is_empty());
        }
    }

    #[test]
    fn no_universal_scenario_regression() {
        let mut w = base_world();
        requirement(
            &mut w,
            "requirement:compliance",
            7,
            RequirementKind::Compliance,
            Some("review"),
        );
        requirement(
            &mut w,
            "requirement:functional-review",
            8,
            RequirementKind::Functional,
            Some("inspection"),
        );
        let result = compile(
            &w,
            vec![
                source(R_QUALITY, NonExecutable, false),
                source("requirement:compliance", NonExecutable, false),
                source("requirement:functional-review", NonExecutable, false),
            ],
        );
        assert_eq!(result.items.len(), 3);
        for item in &result.items {
            assert_ne!(item.verification_kind, Some(VerificationKind::Scenario));
            assert!(item.scenario_refs.is_empty());
        }
        assert!(result.applicability.executable_source_refs.is_empty());
    }

    // ------------------------------------------------------------------ executable

    #[test]
    fn executable_operation_links_its_covering_scenario() {
        let w = base_world();
        let result = compile(&w, vec![source(SUBMIT, ExecutableScenario, false)]);
        let o = obligation(SUBMIT, VerificationKind::Scenario);
        assert_eq!(
            patch_summary(&result, 0),
            vec![
                ("add_node".to_owned(), o.clone()),
                (
                    "add_edge:verified_by".to_owned(),
                    rel(RelationKind::VerifiedBy, &id(SUBMIT), &o)
                ),
                (
                    "add_edge:implemented_as".to_owned(),
                    rel(RelationKind::ImplementedAs, &o, &id("scenario:s1"))
                ),
            ]
        );
        assert_eq!(
            added_obligation(&result, 0),
            VerificationObligation {
                name: format!("Verify {SUBMIT} via scenario"),
                verification_kind: VerificationKind::Scenario,
                method: None,
                acceptance_condition: None,
                required_evidence_kind: None,
            }
        );
    }

    #[test]
    fn executable_requirement_links_through_specified_by_operation() {
        let w = base_world();
        let result = compile(&w, vec![source(R_LEAVE, ExecutableScenario, true)]);
        let o = obligation(R_LEAVE, VerificationKind::Scenario);
        assert_eq!(o.to_string(), GOLDEN_SCENARIO_OBLIGATION);
        let verified = rel(RelationKind::VerifiedBy, &id(R_LEAVE), &o);
        let implemented = rel(RelationKind::ImplementedAs, &o, &id("scenario:s1"));
        assert_eq!(verified.to_string(), GOLDEN_VERIFIED_BY);
        assert_eq!(implemented.to_string(), GOLDEN_IMPLEMENTED_AS);
        assert_eq!(
            patch_summary(&result, 0),
            vec![
                ("add_node".to_owned(), o.clone()),
                ("add_edge:verified_by".to_owned(), verified),
                ("add_edge:implemented_as".to_owned(), implemented),
            ]
        );
        assert_eq!(result.items[0].scenario_refs, vec![id("scenario:s1")]);
        assert_eq!(
            result.applicability.blocking_scenario_refs,
            ids(&["scenario:s1"])
        );
        // Exact edge directions and evidence (source evidence plus scenario evidence, deduplicated).
        let SemanticPatch::Compound { patches } = &result.proposals[0].patch_set.patch else {
            unreachable!()
        };
        let SemanticPatch::AddEdge { edge } = &patches[1] else {
            unreachable!()
        };
        assert_eq!(
            (edge.from.clone(), edge.to.clone(), edge.status),
            (id(R_LEAVE), o.clone(), Proposed)
        );
        let SemanticPatch::AddEdge { edge } = &patches[2] else {
            unreachable!()
        };
        assert_eq!((edge.from.clone(), edge.to.clone()), (o, id("scenario:s1")));
        assert_eq!(edge.evidence, vec![fragment(1).into()]);
    }

    #[test]
    fn executable_requirement_links_direct_requirement_refs() {
        let w = base_world();
        let result = compile(&w, vec![source(R_DIRECT, ExecutableScenario, false)]);
        assert_eq!(result.items[0].scenario_refs, vec![id("scenario:s2")]);
        assert!(result.applicability.blocking_scenario_refs.is_empty());
    }

    #[test]
    fn executable_source_without_scenario_still_gets_its_obligation() {
        let mut w = base_world();
        requirement(
            &mut w,
            "requirement:uncovered",
            9,
            RequirementKind::Functional,
            None,
        );
        w.add(operation("operation:uncovered"));
        let result = compile(
            &w,
            vec![
                source("requirement:uncovered", ExecutableScenario, true),
                source("operation:uncovered", ExecutableScenario, false),
            ],
        );
        for (i, r) in ["operation:uncovered", "requirement:uncovered"]
            .iter()
            .enumerate()
        {
            let o = obligation(r, VerificationKind::Scenario);
            assert_eq!(result.items[i].source_ref, id(r));
            assert!(result.items[i].scenario_refs.is_empty());
            assert_eq!(
                patch_summary(&result, i),
                vec![
                    ("add_node".to_owned(), o.clone()),
                    (
                        "add_edge:verified_by".to_owned(),
                        rel(RelationKind::VerifiedBy, &id(r), &o)
                    ),
                ]
            );
        }
        assert_eq!(
            result.applicability.executable_source_refs,
            ids(&["operation:uncovered", "requirement:uncovered"])
        );
        assert!(result.applicability.blocking_scenario_refs.is_empty());
    }

    #[test]
    fn multiple_covering_scenarios_are_all_linked_in_id_order() {
        let mut w = base_world();
        scenario(
            &mut w,
            "scenario:s4",
            ScenarioWhen::Operation(id(SUBMIT)),
            &[],
            Accepted,
            "Confirmed",
            None,
        );
        scenario(
            &mut w,
            "scenario:s3",
            ScenarioWhen::Operation(id(APPROVE)),
            &[R_LEAVE],
            Accepted,
            "Confirmed",
            None,
        );
        let result = compile(&w, vec![source(R_LEAVE, ExecutableScenario, true)]);
        let expected = vec![id("scenario:s1"), id("scenario:s3"), id("scenario:s4")];
        assert_eq!(result.items[0].scenario_refs, expected);
        let o = obligation(R_LEAVE, VerificationKind::Scenario);
        let links: Vec<Id> = patch_summary(&result, 0)
            .into_iter()
            .filter(|(k, _)| k == "add_edge:implemented_as")
            .map(|(_, e)| e)
            .collect();
        assert_eq!(
            links,
            expected
                .iter()
                .map(|s| rel(RelationKind::ImplementedAs, &o, s))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            result.applicability.blocking_scenario_refs,
            expected.into_iter().collect()
        );
    }

    #[test]
    fn only_accepted_confirmed_scenarios_implement() {
        let mut w = base_world();
        scenario(
            &mut w,
            "scenario:derived",
            ScenarioWhen::Operation(id(SUBMIT)),
            &[],
            Accepted,
            "Derived",
            None,
        );
        scenario(
            &mut w,
            "scenario:proposed",
            ScenarioWhen::Operation(id(SUBMIT)),
            &[],
            Proposed,
            "Confirmed",
            None,
        );
        scenario(
            &mut w,
            "scenario:suspect",
            ScenarioWhen::Operation(id(SUBMIT)),
            &[],
            ElementStatus::Suspect,
            "Confirmed",
            None,
        );
        scenario(
            &mut w,
            "scenario:deprecated",
            ScenarioWhen::Operation(id(SUBMIT)),
            &[],
            ElementStatus::Deprecated,
            "Confirmed",
            None,
        );
        let result = compile(&w, vec![source(SUBMIT, ExecutableScenario, false)]);
        assert_eq!(result.items[0].scenario_refs, vec![id("scenario:s1")]);
    }

    #[test]
    fn non_operation_specified_by_is_never_promoted() {
        let mut w = base_world();
        requirement(
            &mut w,
            "requirement:process",
            9,
            RequirementKind::Functional,
            None,
        );
        w.add(node(
            "process:leave",
            Accepted,
            NodePayload::Process(Process {
                name: "Leave".into(),
                description: None,
                process_kind: None,
            }),
        ))
        .add(node(
            "rule:limit",
            Accepted,
            NodePayload::Rule(Rule {
                name: "Limit".into(),
                rule_kind: RuleKind::Constraint,
                condition_expr: None,
                action_expr: None,
            }),
        ))
        .add(node(
            "quality-characteristic:latency",
            Accepted,
            NodePayload::QualityCharacteristic(QualityCharacteristic {
                name: "Latency".into(),
                scheme: "ISO 25010".into(),
            }),
        ))
        .add(node(
            "measure:p95",
            Accepted,
            NodePayload::Measure(Measure {
                name: "p95".into(),
                measure_type: "latency".into(),
                unit: "ms".into(),
                aggregation: None,
                sampling_window: None,
                measurement_method: None,
            }),
        ))
        .add(node(
            "quality-scenario:fast",
            Accepted,
            NodePayload::QualityScenario(QualityScenario {
                stimulus: "submit".into(),
                response: "ack".into(),
                threshold: json!(200),
                source_ref: None,
                environment_condition: None,
                affected_refs: None,
                priority: None,
            }),
        ))
        .link(
            RelationKind::CharacterizedBy,
            "quality-scenario:fast",
            "quality-characteristic:latency",
        )
        .link(
            RelationKind::MeasuredBy,
            "quality-scenario:fast",
            "measure:p95",
        )
        .link(
            RelationKind::SpecifiedBy,
            "requirement:process",
            "process:leave",
        )
        .link(
            RelationKind::SpecifiedBy,
            "requirement:process",
            "rule:limit",
        )
        .link(
            RelationKind::SpecifiedBy,
            "requirement:process",
            "quality-scenario:fast",
        );
        scenario(
            &mut w,
            "scenario:rule",
            ScenarioWhen::Rule(id("rule:limit")),
            &[],
            Accepted,
            "Confirmed",
            None,
        );
        let result = compile(
            &w,
            vec![source("requirement:process", ExecutableScenario, true)],
        );
        assert!(result.items[0].scenario_refs.is_empty());
        // Direct requirement_refs still link.
        scenario(
            &mut w,
            "scenario:rule",
            ScenarioWhen::Rule(id("rule:limit")),
            &["requirement:process"],
            Accepted,
            "Confirmed",
            None,
        );
        let result = compile(
            &w,
            vec![source("requirement:process", ExecutableScenario, true)],
        );
        assert_eq!(result.items[0].scenario_refs, vec![id("scenario:rule")]);
    }

    // ------------------------------------------------------------------ invalid input

    #[test]
    fn invalid_sources_modes_and_duplicates_are_errors() {
        let mut w = base_world();
        w.add(node(
            "requirement:proposed",
            Proposed,
            NodePayload::Requirement(Requirement {
                statement: "The system shall be proposed.".into(),
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
        ));
        let check = |sources: Vec<VerificationSourceInput>,
                     expect: &dyn Fn(&VerificationError) -> bool| {
            let err = compile_err(&w, sources);
            assert!(expect(&err), "{err:?}");
        };
        check(
            vec![source("requirement:unknown", ExecutableScenario, false)],
            &|e| matches!(e, VerificationError::InvalidSource { .. }),
        );
        check(
            vec![source("requirement:proposed", NonExecutable, false)],
            &|e| matches!(e, VerificationError::InvalidSource { .. }),
        );
        check(
            vec![source("scenario:s1", ExecutableScenario, false)],
            &|e| matches!(e, VerificationError::InvalidSource { .. }),
        );
        check(vec![source(SUBMIT, NonExecutable, false)], &|e| {
            matches!(e, VerificationError::InvalidApplicability { .. })
        });
        check(vec![source(R_QUALITY, ExecutableScenario, false)], &|e| {
            matches!(e, VerificationError::InvalidApplicability { .. })
        });
        check(
            vec![
                source(R_LEAVE, ExecutableScenario, false),
                source(R_LEAVE, NonExecutable, true),
            ],
            &|e| matches!(e, VerificationError::DuplicateSource { .. }),
        );
    }

    // ------------------------------------------------------------------ reconciliation

    fn existing_obligation(
        w: &mut World,
        r: &str,
        kind: VerificationKind,
        method: Option<&str>,
        status: ElementStatus,
    ) -> Id {
        let o = obligation(r, kind);
        w.add(node(
            o.as_str(),
            status,
            NodePayload::VerificationObligation(VerificationObligation {
                name: format!(
                    "Verify {r} via {}",
                    serde_json::to_value(kind).unwrap().as_str().unwrap()
                ),
                verification_kind: kind,
                method: method.map(Into::into),
                acceptance_condition: None,
                required_evidence_kind: None,
            }),
        ));
        o
    }

    #[test]
    fn existing_material_is_reused_and_only_missing_links_are_proposed() {
        let mut w = base_world();
        let o = existing_obligation(&mut w, R_LEAVE, VerificationKind::Scenario, None, Accepted);
        let result = compile(&w, vec![source(R_LEAVE, ExecutableScenario, false)]);
        assert_eq!(
            patch_summary(&result, 0),
            vec![
                (
                    "add_edge:verified_by".to_owned(),
                    rel(RelationKind::VerifiedBy, &id(R_LEAVE), &o)
                ),
                (
                    "add_edge:implemented_as".to_owned(),
                    rel(RelationKind::ImplementedAs, &o, &id("scenario:s1"))
                ),
            ]
        );
        w.link(RelationKind::VerifiedBy, R_LEAVE, o.as_str());
        let result = compile(&w, vec![source(R_LEAVE, ExecutableScenario, false)]);
        assert_eq!(
            patch_summary(&result, 0),
            vec![(
                "add_edge:implemented_as".to_owned(),
                rel(RelationKind::ImplementedAs, &o, &id("scenario:s1"))
            )]
        );
        w.link(RelationKind::ImplementedAs, o.as_str(), "scenario:s1");
        let result = compile(&w, vec![source(R_LEAVE, ExecutableScenario, false)]);
        assert!(result.proposals.is_empty());
        assert_eq!(
            result.items[0].disposition,
            VerificationDisposition::Existing
        );
        assert_eq!(result.items[0].obligation_ref, Some(o.clone()));
        // A newly confirmed Scenario adds only its link; the Accepted obligation is untouched.
        scenario(
            &mut w,
            "scenario:s5",
            ScenarioWhen::Operation(id(SUBMIT)),
            &[],
            Accepted,
            "Confirmed",
            None,
        );
        let result = compile(&w, vec![source(R_LEAVE, ExecutableScenario, false)]);
        assert_eq!(
            patch_summary(&result, 0),
            vec![(
                "add_edge:implemented_as".to_owned(),
                rel(RelationKind::ImplementedAs, &o, &id("scenario:s5"))
            )]
        );
    }

    #[test]
    fn collisions_are_hard_errors() {
        let mut w = base_world();
        existing_obligation(
            &mut w,
            R_QUALITY,
            VerificationKind::Analysis,
            Some("review"),
            Proposed,
        );
        assert!(matches!(
            compile_err(&w, vec![source(R_QUALITY, NonExecutable, false)]),
            VerificationError::VerificationIdCollision { .. }
        ));
        let mut w = base_world();
        let o = obligation(R_QUALITY, VerificationKind::Analysis);
        w.add(operation(o.as_str()));
        assert!(matches!(
            compile_err(&w, vec![source(R_QUALITY, NonExecutable, false)]),
            VerificationError::VerificationIdCollision { .. }
        ));
        // A different relation at the deterministic verified_by ID.
        let mut w = base_world();
        let o = existing_obligation(&mut w, R_LEAVE, VerificationKind::Scenario, None, Accepted);
        w.link_as(
            rel(RelationKind::VerifiedBy, &id(R_LEAVE), &o),
            RelationKind::ImplementedAs,
            o.as_str(),
            "scenario:s1",
            Accepted,
        );
        assert!(matches!(
            compile_err(&w, vec![source(R_LEAVE, ExecutableScenario, false)]),
            VerificationError::VerificationEdgeCollision { .. }
        ));
    }

    #[test]
    fn another_kind_for_the_same_source_coexists() {
        let mut w = base_world();
        existing_obligation(
            &mut w,
            R_QUALITY,
            VerificationKind::Review,
            Some("review"),
            Accepted,
        );
        let result = compile(&w, vec![source(R_QUALITY, NonExecutable, false)]);
        assert_eq!(
            result.items[0].obligation_ref,
            Some(obligation(R_QUALITY, VerificationKind::Analysis))
        );
        assert_eq!(result.proposals.len(), 1);
    }

    // ------------------------------------------------------------------ applicability

    #[test]
    fn blocking_unions_are_exact() {
        let mut w = base_world();
        requirement(
            &mut w,
            "requirement:b",
            9,
            RequirementKind::Functional,
            None,
        );
        scenario(
            &mut w,
            "scenario:s3",
            ScenarioWhen::Operation(id(APPROVE)),
            &["requirement:b"],
            Accepted,
            "Confirmed",
            None,
        );
        // s2 covers R_DIRECT (non-blocking) via requirement_refs and is shared with APPROVE (blocking).
        let result = compile(
            &w,
            vec![
                source(R_LEAVE, ExecutableScenario, true),
                source(APPROVE, ExecutableScenario, true),
                source(R_DIRECT, ExecutableScenario, false),
                source(R_QUALITY, NonExecutable, true),
            ],
        );
        let snapshot = &result.applicability;
        assert_eq!(
            snapshot.source_refs,
            ids(&[APPROVE, R_DIRECT, R_LEAVE, R_QUALITY])
        );
        assert_eq!(
            snapshot.executable_source_refs,
            ids(&[APPROVE, R_DIRECT, R_LEAVE])
        );
        assert_eq!(
            snapshot.blocking_obligation_refs,
            BTreeSet::from([
                obligation(R_LEAVE, VerificationKind::Scenario),
                obligation(APPROVE, VerificationKind::Scenario),
                obligation(R_QUALITY, VerificationKind::Analysis),
            ])
        );
        assert_eq!(
            snapshot.blocking_scenario_refs,
            ids(&["scenario:s1", "scenario:s2", "scenario:s3"])
        );
        let items: Vec<&Id> = result.items.iter().map(|i| &i.source_ref).collect();
        let mut sorted = items.clone();
        sorted.sort();
        assert_eq!(items, sorted);
    }

    // ------------------------------------------------------------------ identity

    #[test]
    fn identity_is_stable_and_proposals_dry_run() {
        let w = base_world();
        let blocking = compile(&w, vec![source(R_LEAVE, ExecutableScenario, true)]);
        let open = compile(&w, vec![source(R_LEAVE, ExecutableScenario, false)]);
        assert_eq!(
            blocking.items[0].obligation_ref,
            open.items[0].obligation_ref
        );
        assert_eq!(blocking.proposals, open.proposals);
        let mut more = w.clone();
        scenario(
            &mut more,
            "scenario:s9",
            ScenarioWhen::Operation(id(SUBMIT)),
            &[],
            Accepted,
            "Confirmed",
            None,
        );
        let other_audit = compile_verification_obligations(
            &more.graph(),
            &VerificationApplicabilityInputs {
                sources: vec![source(R_LEAVE, ExecutableScenario, false)],
            },
            &VerificationAudit {
                created_by: id("actor:other"),
                created_at: ts("2026-02-02T00:00:00Z"),
            },
        )
        .unwrap();
        assert_eq!(
            other_audit.items[0].obligation_ref,
            open.items[0].obligation_ref
        );
        assert_ne!(
            obligation(R_LEAVE, VerificationKind::Scenario),
            obligation(R_LEAVE, VerificationKind::Review)
        );
        assert_ne!(
            obligation(R_LEAVE, VerificationKind::Scenario),
            obligation(R_DIRECT, VerificationKind::Scenario)
        );
        for result in [&blocking, &other_audit] {
            for proposal in &result.proposals {
                assert!(
                    apply_patch(&w.graph(), &proposal.patch_set).is_ok()
                        || apply_patch(&more.graph(), &proposal.patch_set).is_ok()
                );
            }
        }
    }

    #[test]
    fn results_are_deterministic_under_reverse_order() {
        let mut w = base_world();
        scenario(
            &mut w,
            "scenario:s3",
            ScenarioWhen::Operation(id(SUBMIT)),
            &[R_LEAVE],
            Accepted,
            "Confirmed",
            Some(2),
        );
        let sources = vec![
            source(R_LEAVE, ExecutableScenario, true),
            source(SUBMIT, ExecutableScenario, false),
            source(R_QUALITY, NonExecutable, true),
        ];
        let forward = compile(&w, sources.clone());
        let mut reversed_sources = sources;
        reversed_sources.reverse();
        let reverse = compile_verification_obligations(
            &w.reversed_graph(),
            &VerificationApplicabilityInputs {
                sources: reversed_sources,
            },
            &audit(),
        )
        .unwrap();
        assert_eq!(forward, reverse);
        let proposal_ids: Vec<&Id> = forward.proposals.iter().map(|p| &p.id).collect();
        let reverse_ids: Vec<&Id> = reverse.proposals.iter().map(|p| &p.id).collect();
        assert_eq!(proposal_ids, reverse_ids);
    }

    // ------------------------------------------------------------------ source guards

    #[test]
    fn production_source_creates_no_tests_checks_scenarios_or_defaults() {
        let forbidden = [
            "NodePayload::TestCase",
            "TestCase {",
            "ArchitectureCheck {",
            "(Scenario {",
            "= Scenario {",
            "NodePayload::ArchitectureCheck",
            "NodePayload::Question",
            "ResolutionDecision",
            "ReplacePayload",
            "RemoveNode",
            "RemoveEdge",
            "SetStatus",
            "RequirementKind::Quality",
            "RequirementKind::Security",
            "RequirementKind::Compliance",
            ".statement",
            ".contains(\"",
            "to_lowercase",
            "InferenceProvider",
            "MockProvider",
            "Clock",
            "Utc::now",
            "Instant::now",
            "SystemTime::now",
            "std::fs",
            "File::open",
            "rusqlite",
            "ArtifactStore",
            "GraphStore",
            "RevisionStore",
            "reqwest",
            "commit",
            "branch_head",
            "unsafe",
            "acceptance_condition: Some",
            "required_evidence_kind: Some",
        ];
        for token in forbidden {
            assert!(!SRC.contains(token), "verification.rs contains {token}");
        }
        assert!(SRC.contains("NodePayload::VerificationObligation"));
    }
}
