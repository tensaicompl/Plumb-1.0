//! S4.7 contract tests for the functional pilot projections and reports (Hotfix 056).
//!
//! Every test lives in `projections_contract` (`cargo test -p plumb-functional --test
//! projections`). The representative fixture is a hand-built typed PSG; the HR test reads only
//! the immutable fixture contract and is an HR contract-scale projection fixture, not the real
//! compiled HR PSG (owned by E0.1). Content-hash goldens were computed independently in Python.

mod projections_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, GateId, Hash, Id, Timestamp};
    use plumb_functional::{
        project_functional_pilot, project_functional_v2, relation_id, verification_obligation_id,
        DecisionProjectionEntry, FunctionalPilotOutputs, GateReportBinding, PilotProjectionError,
        ProjectionArtifactKind, ScenarioProjectionDocument,
    };
    use plumb_psg::{
        Agent, AgentKind, ArchitectureDescription, Assumption, AuditMeta, BusinessRole, Concept,
        ConceptKind, Edge, ElementStatus, Finding, FindingSeverity, Graph, Modality, Node,
        NodePayload, Operation, OperationKind, RelationKind, RelationProperties, Requirement,
        RequirementKind, RequirementLevel, ResolutionDecision, Scenario, ScenarioKind, Term,
        VerificationKind, VerificationObligation, View, Viewpoint,
    };
    use plumb_validation::{
        load_builtin_software_profile, rules, EvaluatorRegistry, F4ApplicabilityInput,
        F4ValidationInputs, GateReport, GateResult, ValidationContext, ValidationPolicy,
        ValidationProfile, ValidationRegistry,
    };
    use serde_json::{json, Value};

    use ElementStatus::{Accepted, Proposed, Suspect};

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const FIXTURE_CONTRACT: &str = include_str!("../../../fixtures/hr-leave/fixture-contract.yaml");
    const REPORT_SRC: &str = include_str!("../src/report.rs");
    const PROJECTIONS_SRC: &str = include_str!("../src/projections.rs");

    const GOLDEN_GLOSSARY: &str = r##"# Glossary

## `term:balance` — Balance

- Language: `en`
- Status: `unspecified`

_No accepted definition._

## `term:leave` — Leave \# request

- Language: `en`
- Status: `preferred`
- Aliases: Absence, PTO

A \[period\] of \*approved\* absence.
Paid or unpaid.
"##;
    const GOLDEN_SCENARIOS: &str = r##"version: 1
model_hash: psg:sha256:238ffc0323ffc9b12c7598ff5d865239d1dd22f41e4d35b9285664bad3208d36
scenarios:
- id: scenario:a-derived
  name: Scenario scenario:a-derived
  scenario_kind: acceptance
  given:
  - states_active: []
  - attribute_values: {}
  when:
    operation: operation:submit
  then: []
  requirement_refs:
  - requirement:r1
  derived_from_refs:
  - operation:submit
  confirmation_status: Derived
- id: scenario:b-confirmed
  name: Scenario scenario:b-confirmed
  scenario_kind: acceptance
  given:
  - states_active: []
  - attribute_values: {}
  when:
    operation: operation:submit
  then: []
  requirement_refs:
  - requirement:r1
  derived_from_refs:
  - operation:submit
  confirmation_status: Confirmed
- id: scenario:c-plain
  name: Scenario scenario:c-plain
  scenario_kind: acceptance
  given:
  - states_active: []
  - attribute_values: {}
  when:
    operation: operation:submit
  then: []
  requirement_refs:
  - requirement:r1
  derived_from_refs:
  - operation:submit
  confirmation_status: null
"##;
    const GOLDEN_DECISIONS: &str = r##"{"answer":{"days":5,"kind":"carry_over"},"decided_at":"2026-03-01T00:00:00.000000000Z","decided_by":"agent:analyst","id":"dec:a","patch_ref":"sha256:102d1cd658beb9c2a06b0458173fdebcb331f65addfb3a70a3d77783e540c325","proposal_ref":null,"question_ref":"question:carry-over","rationale":"Policy <b>section</b> 4.2","supersedes":null}
{"answer":{"days":5,"kind":"carry_over"},"decided_at":"2026-02-01T00:00:00.000000000Z","decided_by":"agent:analyst","id":"dec:b","patch_ref":"sha256:55ca51bdb9295598c2770b3bdc72c5a5cc9ebcd0a39f4e79321b9c59136de013","proposal_ref":null,"question_ref":"question:carry-over","rationale":"Policy <b>section</b> 4.2","supersedes":null}
"##;
    const GOLDEN_MODEL_REPORT: &str = r##"# Model Report

## Baseline

- Project ID: `project:pilot`
- Profile ID: `profile:plumb-software-2026.1`
- Semantic hash: `psg:sha256:238ffc0323ffc9b12c7598ff5d865239d1dd22f41e4d35b9285664bad3208d36`
- Evidence hash: `ev:sha256:9eef75643b0d6c5593a7c70e41d5833530ca17796cbc1465e6a62b9578548558`
- Profile hash: `sha256:e6dc73ec3f1847e5e7fc5d71aef64b71c15a3e93838e64c7f246f275c65af055`
- Rule-pack hash: `sha256:b3a48aa0d0dc47b612697bbf35fa778499509343f74de43459fbcc145e21d33e`

## F4 Gate

- Result: `PASS`
- Gate-report content hash: `sha256:b1cc7547c0fbcb1b4ac5db35c3977c306ba30fd126a3837c85c612158b0e5c7e`
- Blocker failures: 0
- Blocker waivers: 0
- Warnings: 0
- Generated findings: 0
- Applied waivers: 0

## Accepted Model Counts

| Node type | Accepted |
| --- | ---: |
| Agent | 1 |
| Assumption | 1 |
| BusinessRole | 1 |
| Concept | 1 |
| Finding | 1 |
| Operation | 1 |
| Requirement | 1 |
| ResolutionDecision | 2 |
| Scenario | 3 |
| Term | 2 |
| VerificationObligation | 1 |

- Accepted edges: 3

## Functional Compatibility Projection

- functional.yaml content hash: `sha256:250e0a11b76a28b482fe7f3add6bb9c2165b9dd77a0daa4c70ad54a93c05cb56`
- Projection version: 2
- Warnings: 4
- Lossy mappings: 1

### Warnings

- `V2_ASSUMPTION_UNREPRESENTABLE` (`assumption:carry-over`): Assumption lacks legacy-required fields; omitted.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:balance`): Term has no accepted concept definition.
- `V2_NON_ACCEPTED_BASELINE_OMITTED` (`requirement:suspect`): Suspect, Superseded or Deprecated element omitted from the confirmed legacy view.
- `V2_OPERATION_PERFORMER_MISSING` (`operation:submit`): No performer; the legacy object is omitted.

### Lossy mappings

- `V2_BUSINESS_ROLE_COLLAPSE` (`business-role:clerk`): Business role projected with itself as actor token.

## Scenarios

- Accepted Scenarios: 3
- Confirmed Scenarios: 1
- Derived Scenarios: 1
- Other/unspecified confirmation status: 1

## Decisions

- Accepted ResolutionDecisions: 2

## Readiness

Readiness is advisory only and is not calculated by S4.7. It does not alter the F4 gate result.
"##;
    // Independent Python SHA-256 of each representative artifact's exact bytes.
    const GOLDEN_HASHES: [(&str, &str); 6] = [
        (
            "functional.yaml",
            "sha256:250e0a11b76a28b482fe7f3add6bb9c2165b9dd77a0daa4c70ad54a93c05cb56",
        ),
        (
            "glossary.md",
            "sha256:af7a5cbfd9e33109aa6a451ca14b4240638e5512ff785a8b1174ff10c9d487f0",
        ),
        (
            "scenarios.yaml",
            "sha256:824f1c15bb3e4342ffdce6f8ce32ab78253e876ec8cffb947d1f8ff79cae3377",
        ),
        (
            "decisions.jsonl",
            "sha256:1f18f0501a4a8587f35ee808542b911edae014d13d42744517a0dea7c18c70a5",
        ),
        (
            "model-report.md",
            "sha256:4a7ab7b68cfa76b67400587f97fb0952ea89842b7e26694a6417fd664278100b",
        ),
        (
            "gate-report.json",
            "sha256:b1cc7547c0fbcb1b4ac5db35c3977c306ba30fd126a3837c85c612158b0e5c7e",
        ),
    ];

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn meta() -> AuditMeta {
        AuditMeta::new(id("agent:analyst"), ts(AT), None, None).unwrap()
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

    fn edge(kind: RelationKind, from: &str, to: &str) -> Edge {
        Edge {
            id: relation_id(&kind, &id(from), &id(to)).unwrap(),
            revision: 1,
            status: Accepted,
            kind,
            from: id(from),
            to: id(to),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: meta(),
        }
    }

    fn requirement(
        node_id: &str,
        status: ElementStatus,
        kind: RequirementKind,
        statement: &str,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Requirement(Requirement {
                statement: statement.into(),
                requirement_kind: kind,
                level: RequirementLevel::System,
                modality: Modality::Shall,
                title: None,
                rationale: None,
                priority: None,
                source_identifier: None,
                verification_method: Some("analysis".into()),
                owner_refs: None,
                stakeholder_refs: None,
            }),
        )
    }

    fn scenario(
        node_id: &str,
        status: ElementStatus,
        confirmation: Option<&str>,
        refs: &[&str],
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Scenario(Scenario {
                name: format!("Scenario {node_id}"),
                scenario_kind: ScenarioKind::Acceptance,
                given: vec![
                    json!({"states_active": []}),
                    json!({"attribute_values": {}}),
                ],
                when: json!({"operation": "operation:submit"}),
                then: vec![],
                requirement_refs: Some(refs.iter().map(|r| id(r)).collect()),
                derived_from_refs: Some(vec![id("operation:submit"), id("operation:submit")]),
                confirmation_status: confirmation.map(Into::into),
            }),
        )
    }

    fn decision(node_id: &str, status: ElementStatus, decided_at: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::ResolutionDecision(ResolutionDecision {
                question_ref: Some(id("question:carry-over")),
                proposal_ref: None,
                answer: json!({"kind": "carry_over", "days": 5}),
                decided_by: id("agent:analyst"),
                decided_at: ts(decided_at),
                patch_ref: Hash::content_sha256(node_id.as_bytes()),
                rationale: Some("Policy <b>section</b> 4.2".into()),
                supersedes: None,
            }),
        )
    }

    fn obligation_id() -> Id {
        verification_obligation_id(
            &id(PROJECT),
            &id("requirement:r1"),
            VerificationKind::Analysis,
        )
        .unwrap()
    }

    /// The representative typed fixture: every projected family, mixed statuses.
    fn representative() -> (Vec<Node>, Vec<Edge>) {
        let o = obligation_id();
        let nodes = vec![
            node(
                "agent:analyst",
                Accepted,
                NodePayload::Agent(Agent {
                    agent_kind: AgentKind::Human,
                }),
            ),
            node(
                "business-role:clerk",
                Accepted,
                NodePayload::BusinessRole(BusinessRole {
                    name: "Clerk".into(),
                }),
            ),
            node(
                "concept:leave",
                Accepted,
                NodePayload::Concept(Concept {
                    name: "Leave".into(),
                    definition: "A [period] of *approved* absence.\r\nPaid or unpaid.".into(),
                    concept_kind: ConceptKind::ObjectType,
                }),
            ),
            node(
                "term:leave",
                Accepted,
                NodePayload::Term(Term {
                    term: "Leave # request".into(),
                    language: "en".into(),
                    definition_ref: Some(id("concept:leave")),
                    aliases: Some(vec!["PTO".into(), "Absence".into(), "PTO".into()]),
                    status: Some("preferred".into()),
                }),
            ),
            node(
                "term:balance",
                Accepted,
                NodePayload::Term(Term {
                    term: "Balance".into(),
                    language: "en".into(),
                    definition_ref: None,
                    aliases: None,
                    status: None,
                }),
            ),
            node(
                "term:draft",
                Proposed,
                NodePayload::Term(Term {
                    term: "Draft".into(),
                    language: "en".into(),
                    definition_ref: None,
                    aliases: None,
                    status: None,
                }),
            ),
            requirement(
                "requirement:r1",
                Accepted,
                RequirementKind::Functional,
                "The system shall record leave.",
            ),
            requirement(
                "requirement:suspect",
                Suspect,
                RequirementKind::Functional,
                "The system shall be suspect.",
            ),
            node(
                "operation:submit",
                Accepted,
                NodePayload::Operation(Operation {
                    name: "Submit".into(),
                    operation_kind: OperationKind::Command,
                    input_schema_ref: None,
                    output_schema_ref: None,
                    preconditions: None,
                    postconditions: None,
                    idempotency: None,
                    transaction_semantics: None,
                }),
            ),
            scenario(
                "scenario:b-confirmed",
                Accepted,
                Some("Confirmed"),
                &["requirement:r1", "requirement:r1"],
            ),
            scenario(
                "scenario:a-derived",
                Accepted,
                Some("Derived"),
                &["requirement:r1"],
            ),
            scenario("scenario:c-plain", Accepted, None, &["requirement:r1"]),
            scenario(
                "scenario:d-proposed",
                Proposed,
                Some("Confirmed"),
                &["requirement:r1"],
            ),
            decision("dec:b", Accepted, "2026-02-01T00:00:00Z"),
            decision("dec:a", Accepted, "2026-03-01T00:00:00Z"),
            decision("dec:c", Proposed, "2026-01-15T00:00:00Z"),
            node(
                "fnd:00000000000000a1",
                Accepted,
                NodePayload::Finding(Finding {
                    code: "PLUMB.S3.AMBIGUITY".into(),
                    family: "S3".into(),
                    severity: FindingSeverity::Warn,
                    message: "Carry-over is ambiguous.".into(),
                    status: "Open".into(),
                    affected_refs: vec![id("requirement:r1")],
                    standard_rule_ref: None,
                    suggested_resolution: None,
                    waiver_ref: None,
                }),
            ),
            node(
                "assumption:carry-over",
                Accepted,
                NodePayload::Assumption(Assumption {
                    statement: "Carry-over defaults to five days.".into(),
                    owner_ref: id("agent:analyst"),
                    status: "Accepted".into(),
                    finding_ref: Some(id("fnd:00000000000000a1")),
                    default_value: None,
                    expires_at: None,
                    risk_ref: None,
                }),
            ),
            node(
                o.as_str(),
                Accepted,
                NodePayload::VerificationObligation(VerificationObligation {
                    name: "Verify requirement:r1 via analysis".into(),
                    verification_kind: VerificationKind::Analysis,
                    method: Some("analysis".into()),
                    acceptance_condition: None,
                    required_evidence_kind: None,
                }),
            ),
        ];
        let edges = vec![
            edge(RelationKind::VerifiedBy, "requirement:r1", o.as_str()),
            edge(RelationKind::Resolves, "dec:a", "fnd:00000000000000a1"),
            edge(RelationKind::Resolves, "dec:b", "fnd:00000000000000a1"),
            Edge {
                status: Proposed,
                ..edge(RelationKind::Resolves, "dec:c", "fnd:00000000000000a1")
            },
        ];
        (nodes, edges)
    }

    fn graph_of(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap()
    }

    fn graph() -> Graph {
        let (n, e) = representative();
        graph_of(n, e)
    }

    fn reversed_graph() -> Graph {
        let (mut n, mut e) = representative();
        n.reverse();
        e.reverse();
        graph_of(n, e)
    }

    fn profile() -> ValidationProfile {
        load_builtin_software_profile().unwrap()
    }

    /// A real F4 report from the S4.6 evaluators; `sources` choose PASS or FAIL.
    fn f4_report(graph: &Graph, sources: &[&str], visible: &[&str]) -> GateReport {
        let metadata = ValidationRegistry::new(profile()).unwrap();
        let mut registry = EvaluatorRegistry::new(metadata.clone());
        rules::register_f4_evaluators(&mut registry).unwrap();
        let inputs = F4ValidationInputs::new(
            F4ApplicabilityInput {
                source_refs: sources.iter().map(|s| id(s)).collect(),
                ..F4ApplicabilityInput::default()
            },
            vec![],
            vec![],
            visible.iter().map(|s| id(s)).collect(),
        );
        let ctx = ValidationContext::new(
            graph,
            &metadata,
            ValidationPolicy::default(),
            graph.evidence_hash().unwrap(),
            vec![],
            vec![],
        )
        .unwrap()
        .with_f4_inputs(graph, inputs)
        .unwrap();
        registry.evaluate_gate(GateId::F4, graph, &ctx).unwrap()
    }

    fn pass_report(graph: &Graph) -> GateReport {
        f4_report(graph, &["requirement:r1"], &["assumption:carry-over"])
    }

    fn outputs(graph: &Graph) -> FunctionalPilotOutputs {
        project_functional_pilot(graph, &profile(), &pass_report(graph)).unwrap()
    }

    fn text(bytes: &[u8]) -> &str {
        std::str::from_utf8(bytes).unwrap()
    }

    // ------------------------------------------------------------------ artifacts

    #[test]
    fn six_artifacts_carry_exact_companion_metadata() {
        let g = graph();
        let out = outputs(&g);
        let semantic = g.semantic_hash().unwrap();
        let expected = [
            ("functional.yaml", "application/yaml"),
            ("glossary.md", "text/markdown; charset=utf-8"),
            ("scenarios.yaml", "application/yaml"),
            ("decisions.jsonl", "application/x-ndjson"),
            ("model-report.md", "text/markdown; charset=utf-8"),
            ("gate-report.json", "application/json"),
        ];
        for ((artifact, kind), (file, media)) in out
            .artifacts()
            .iter()
            .zip(ProjectionArtifactKind::ALL)
            .zip(expected)
        {
            assert_eq!(artifact.kind, kind);
            assert_eq!(kind.file_name(), file);
            assert_eq!(kind.media_type(), media);
            assert_eq!(artifact.source_semantic_hash, semantic);
            assert_eq!(artifact.content_hash, Hash::content_sha256(&artifact.bytes));
        }
        for (artifact, (file, hash)) in out.artifacts().iter().zip(GOLDEN_HASHES) {
            assert_eq!(artifact.kind.file_name(), file);
            assert_eq!(artifact.content_hash.to_string(), hash, "{file}");
        }
    }

    #[test]
    fn functional_yaml_is_exactly_the_f014_projection() {
        let g = graph();
        let out = outputs(&g);
        let f014 = project_functional_v2(&g, &profile()).unwrap();
        assert_eq!(out.functional_yaml.bytes, f014.yaml);
        assert_eq!(out.functional_metadata, f014.metadata);
        assert_eq!(out.functional_yaml.content_hash, f014.metadata.content_hash);
        assert!(!out.functional_metadata.warnings.is_empty());
        assert!(!out.functional_metadata.lossy_mappings.is_empty());
    }

    #[test]
    fn gate_report_json_is_the_canonical_supplied_report() {
        let g = graph();
        let report = pass_report(&g);
        let out = project_functional_pilot(&g, &profile(), &report).unwrap();
        assert_eq!(
            out.gate_report_json.bytes,
            to_canonical_json(&report).unwrap()
        );
        assert_eq!(
            out.gate_report_json.content_hash,
            report.content_hash().unwrap()
        );
        assert_eq!(report.result, GateResult::Pass);
    }

    // ------------------------------------------------------------------ glossary

    #[test]
    fn glossary_projects_every_accepted_term() {
        let out = outputs(&graph());
        assert_eq!(text(&out.glossary_md.bytes), GOLDEN_GLOSSARY);
        let g = text(&out.glossary_md.bytes);
        assert!(g.contains("_No accepted definition._"));
        assert!(!g.contains("term:draft"));
        assert!(g.contains("- Aliases: Absence, PTO\n"));
        // Semantic Markdown is escaped, not interpreted.
        assert!(g.contains("Leave \\# request"));
        assert!(g.contains("A \\[period\\] of \\*approved\\* absence.\nPaid or unpaid."));
        assert!(!g.contains('\r'));
        assert!(g.ends_with("unpaid.\n") || g.ends_with(".\n"));
        assert!(!g.ends_with("\n\n"));
    }

    // ------------------------------------------------------------------ scenarios

    #[test]
    fn scenarios_project_accepted_payloads_in_id_order() {
        let g = graph();
        let out = outputs(&g);
        assert_eq!(text(&out.scenarios_yaml.bytes), GOLDEN_SCENARIOS);
        let doc: ScenarioProjectionDocument =
            serde_yaml::from_slice(&out.scenarios_yaml.bytes).unwrap();
        assert_eq!(doc.version, 1);
        assert_eq!(doc.model_hash, g.semantic_hash().unwrap());
        let ids: Vec<&str> = doc.scenarios.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "scenario:a-derived",
                "scenario:b-confirmed",
                "scenario:c-plain"
            ]
        );
        let confirmed = &doc.scenarios[1];
        assert_eq!(confirmed.requirement_refs, Some(vec![id("requirement:r1")]));
        assert_eq!(
            confirmed.derived_from_refs,
            Some(vec![id("operation:submit")])
        );
        assert_eq!(
            confirmed.given,
            vec![
                json!({"states_active": []}),
                json!({"attribute_values": {}})
            ]
        );
        assert_eq!(confirmed.confirmation_status.as_deref(), Some("Confirmed"));
        assert!(!text(&out.scenarios_yaml.bytes).contains("trace"));
    }

    // ------------------------------------------------------------------ decisions

    #[test]
    fn decisions_are_canonical_accepted_lines_in_id_order() {
        let out = outputs(&graph());
        let jsonl = text(&out.decisions_jsonl.bytes);
        assert_eq!(jsonl, GOLDEN_DECISIONS);
        let lines: Vec<&str> = jsonl.split_terminator('\n').collect();
        assert_eq!(lines.len(), 2);
        assert!(jsonl.ends_with('\n') && !jsonl.ends_with("\n\n"));
        let ids: Vec<String> = lines
            .iter()
            .map(|l| {
                let entry: DecisionProjectionEntry = serde_json::from_str(l).unwrap();
                assert_eq!(to_canonical_json(&entry).unwrap(), l.as_bytes());
                entry.id.to_string()
            })
            .collect();
        assert_eq!(ids, vec!["dec:a", "dec:b"]);
    }

    #[test]
    fn no_accepted_decisions_give_empty_bytes() {
        let (mut n, e) = representative();
        n.retain(|x| !matches!(x.payload, NodePayload::ResolutionDecision(_)));
        let e: Vec<Edge> = e
            .into_iter()
            .filter(|x| x.kind != RelationKind::Resolves)
            .collect();
        let g = graph_of(n, e);
        let out = outputs(&g);
        assert!(out.decisions_jsonl.bytes.is_empty());
        assert_eq!(out.decisions_jsonl.content_hash, Hash::content_sha256(b""));
    }

    // ------------------------------------------------------------------ model report

    #[test]
    fn model_report_summarizes_baseline_gate_counts_and_notices() {
        let g = graph();
        let out = outputs(&g);
        let report = text(&out.model_report_md.bytes);
        assert_eq!(report, GOLDEN_MODEL_REPORT);
        let profile = profile();
        for value in [
            PROJECT.to_owned(),
            PROFILE.to_owned(),
            g.semantic_hash().unwrap().to_string(),
            g.evidence_hash().unwrap().to_string(),
            profile.profile_hash().unwrap().to_string(),
            profile.rule_pack_hash().unwrap().to_string(),
            out.gate_report_json.content_hash.to_string(),
            out.functional_yaml.content_hash.to_string(),
        ] {
            assert!(report.contains(&format!("`{value}`")), "{value}");
        }
        assert!(report.contains("- Result: `PASS`"));
        assert!(report.contains("| Requirement | 1 |"));
        assert!(report.contains("| Term | 2 |"));
        assert!(report.contains("- Accepted Scenarios: 3\n- Confirmed Scenarios: 1\n- Derived Scenarios: 1\n- Other/unspecified confirmation status: 1"));
        assert!(report.contains("- Accepted ResolutionDecisions: 2"));
        for notice in out
            .functional_metadata
            .warnings
            .iter()
            .chain(&out.functional_metadata.lossy_mappings)
        {
            assert!(report.contains(&format!("`{}`", notice.code)));
        }
        assert!(report.ends_with(
            "## Readiness\n\nReadiness is advisory only and is not calculated by S4.7. It does not alter the F4 gate result.\n"
        ));
        let sections: Vec<&str> = report
            .lines()
            .filter(|l| l.starts_with("# ") || l.starts_with("## "))
            .collect();
        assert_eq!(
            sections,
            vec![
                "# Model Report",
                "## Baseline",
                "## F4 Gate",
                "## Accepted Model Counts",
                "## Functional Compatibility Projection",
                "## Scenarios",
                "## Decisions",
                "## Readiness",
            ]
        );
    }

    #[test]
    fn failed_gate_is_exported_as_failed() {
        let g = graph();
        let fail = f4_report(
            &g,
            &["operation:submit", "requirement:r1"],
            &["assumption:carry-over"],
        );
        assert_eq!(fail.result, GateResult::Fail);
        let before = to_canonical_json(&fail).unwrap();
        let out = project_functional_pilot(&g, &profile(), &fail).unwrap();
        assert_eq!(out.gate_report_json.bytes, before);
        assert_eq!(to_canonical_json(&fail).unwrap(), before);
        let report = text(&out.model_report_md.bytes);
        assert!(report.contains("- Result: `FAIL`"));
        assert!(report.contains("Readiness is advisory only"));
        // Only the gate-related material differs from the PASS outputs.
        let pass = outputs(&g);
        assert_eq!(out.functional_yaml, pass.functional_yaml);
        assert_eq!(out.glossary_md, pass.glossary_md);
        assert_eq!(out.scenarios_yaml, pass.scenarios_yaml);
        assert_eq!(out.decisions_jsonl, pass.decisions_jsonl);
        assert_ne!(out.gate_report_json, pass.gate_report_json);
        let differing: Vec<(&str, &str)> = text(&pass.model_report_md.bytes)
            .lines()
            .zip(report.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert!(!differing.is_empty());
        for (a, _) in differing {
            assert!(
                a.starts_with("- Result:")
                    || a.starts_with("- Gate-report content hash:")
                    || a.starts_with("- Blocker failures:")
                    || a.starts_with("- Generated findings:")
                    || a.starts_with("- Warnings:"),
                "{a}"
            );
        }
    }

    // ------------------------------------------------------------------ binding errors

    #[test]
    fn gate_report_must_be_a_current_valid_f4_report() {
        let g = graph();
        let p = profile();
        let report = pass_report(&g);
        let expect = |r: &GateReport, check: &dyn Fn(&PilotProjectionError) -> bool| {
            let err = project_functional_pilot(&g, &p, r).unwrap_err();
            assert!(check(&err), "{err:?}");
        };
        let mut wrong = report.clone();
        wrong.gate = GateId::F3;
        expect(&wrong, &|e| {
            matches!(e, PilotProjectionError::WrongGate { actual: GateId::F3 })
        });
        let other_graph = {
            let (mut n, e) = representative();
            n.retain(|x| x.id != id("term:balance"));
            graph_of(n, e)
        };
        let mut stale = report.clone();
        stale.baseline_semantic_hash = other_graph.semantic_hash().unwrap();
        expect(&stale, &|e| {
            matches!(
                e,
                PilotProjectionError::StaleGateReport {
                    binding: GateReportBinding::SemanticHash
                }
            )
        });
        let mut evidence = report.clone();
        evidence.baseline_evidence_hash = Hash::evidence_sha256(b"other");
        expect(&evidence, &|e| {
            matches!(
                e,
                PilotProjectionError::StaleGateReport {
                    binding: GateReportBinding::EvidenceHash
                }
            )
        });
        let mut profile_id = report.clone();
        profile_id.profile_id = id("profile:other");
        expect(&profile_id, &|e| {
            matches!(
                e,
                PilotProjectionError::ProfileMismatch {
                    binding: GateReportBinding::ProfileId
                }
            )
        });
        let mut profile_hash = report.clone();
        profile_hash.profile_hash = Hash::content_sha256(b"old profile");
        expect(&profile_hash, &|e| {
            matches!(
                e,
                PilotProjectionError::ProfileMismatch {
                    binding: GateReportBinding::ProfileHash
                }
            )
        });
        let mut pack = report.clone();
        pack.rule_pack_hash = Hash::content_sha256(b"old rule pack");
        expect(&pack, &|e| {
            matches!(
                e,
                PilotProjectionError::ProfileMismatch {
                    binding: GateReportBinding::RulePackHash
                }
            )
        });
        let mut invalid = report.clone();
        invalid.summary.blocker_failed += 1;
        expect(&invalid, &|e| {
            matches!(e, PilotProjectionError::GateReport(_))
        });
        // A changed semantic graph cannot reuse the old report.
        assert!(matches!(
            project_functional_pilot(&other_graph, &p, &report),
            Err(PilotProjectionError::StaleGateReport { .. })
        ));
    }

    // ------------------------------------------------------------------ determinism

    #[test]
    fn outputs_are_deterministic_and_insertion_order_independent() {
        let g = graph();
        let a = outputs(&g);
        let b = outputs(&g);
        assert_eq!(a, b);
        let r = reversed_graph();
        assert_eq!(r.semantic_hash().unwrap(), g.semantic_hash().unwrap());
        let c = project_functional_pilot(&r, &profile(), &pass_report(&g)).unwrap();
        assert_eq!(a, c);
    }

    fn with_view(layout: &[u8]) -> Graph {
        let (mut n, e) = representative();
        n.push(node(
            "architecture-description:ad",
            Accepted,
            NodePayload::ArchitectureDescription(ArchitectureDescription {
                system_of_interest_ref: id("operation:submit"),
                purpose: None,
                scope: None,
                architecture_candidate_ref: None,
            }),
        ));
        n.push(node(
            "viewpoint:functional",
            Accepted,
            NodePayload::Viewpoint(Viewpoint {
                name: "Functional".into(),
                concern_refs: vec![],
                stakeholder_refs: vec![],
                model_kind_refs: vec![],
                purpose: None,
                conventions: None,
            }),
        ));
        n.push(node(
            "view:functional",
            Accepted,
            NodePayload::View(View {
                name: "Functional".into(),
                viewpoint_ref: id("viewpoint:functional"),
                architecture_description_ref: id("architecture-description:ad"),
                root_refs: None,
                filter: None,
                projection_rules: vec![],
                layout_ref: Some(Hash::content_sha256(layout)),
                style_ref: Some(Hash::content_sha256(layout)),
            }),
        ));
        graph_of(n, e)
    }

    #[test]
    fn view_layout_and_style_do_not_change_outputs() {
        let a = with_view(b"layout-a");
        let b = with_view(b"layout-b");
        assert_eq!(a.semantic_hash().unwrap(), b.semantic_hash().unwrap());
        let report = pass_report(&a);
        let x = project_functional_pilot(&a, &profile(), &report).unwrap();
        let y = project_functional_pilot(&b, &profile(), &report).unwrap();
        assert_eq!(x, y);
    }

    #[test]
    fn semantic_changes_change_outputs() {
        let (mut n, e) = representative();
        n.retain(|x| x.id != id("term:balance"));
        let changed = graph_of(n, e);
        let a = outputs(&graph());
        let b = outputs(&changed);
        assert_ne!(
            a.glossary_md.source_semantic_hash,
            b.glossary_md.source_semantic_hash
        );
        assert_ne!(a.glossary_md.bytes, b.glossary_md.bytes);
        assert_ne!(
            a.functional_yaml.content_hash,
            b.functional_yaml.content_hash
        );
    }

    // ------------------------------------------------------------------ HR contract scale

    #[test]
    fn hr_fixture_contract_scale_projects_through_f014() {
        let contract: serde_yaml::Value = serde_yaml::from_str(FIXTURE_CONTRACT).unwrap();
        let counts = &contract["expected_counts"];
        for (key, expected) in [
            ("requirements", 32),
            ("entities", 9),
            ("operations", 13),
            ("processes", 6),
            ("invariants", 5),
            ("calculations", 2),
            ("rules", 3),
            ("events", 6),
            ("scenarios", 44),
        ] {
            assert_eq!(counts[key].as_u64(), Some(expected), "{key}");
        }
        let requirements = contract["requirements"].as_sequence().unwrap();
        assert_eq!(requirements.len(), 32);
        // An HR contract-scale projection fixture, not the real compiled HR PSG (E0.1).
        let nodes: Vec<Node> = requirements
            .iter()
            .map(|r| {
                let kind: RequirementKind = serde_yaml::from_value(r["kind"].clone()).unwrap();
                let reference = format!("requirement:{}", r["id"].as_str().unwrap().to_lowercase());
                requirement(&reference, Accepted, kind, r["statement"].as_str().unwrap())
            })
            .collect();
        let g = graph_of(nodes, vec![]);
        let report = f4_report(&g, &[], &[]);
        let out = project_functional_pilot(&g, &profile(), &report).unwrap();
        assert!(text(&out.model_report_md.bytes).contains("| Requirement | 32 |"));
        assert_eq!(
            out.functional_yaml.bytes,
            project_functional_v2(&g, &profile()).unwrap().yaml
        );
        assert_eq!(
            out,
            project_functional_pilot(&g, &profile(), &report).unwrap()
        );
    }

    // ------------------------------------------------------------------ source guards

    #[test]
    fn production_sources_are_pure() {
        for (name, source) in [
            ("report.rs", REPORT_SRC),
            ("projections.rs", PROJECTIONS_SRC),
        ] {
            for token in [
                "std::fs",
                "File::create",
                "File::write",
                "ArtifactStore",
                "BlobStore",
                "GraphStore",
                "RevisionStore",
                "Clock",
                "Utc::now",
                "Instant::now",
                "SystemTime",
                "InferenceProvider",
                "InferenceRequest",
                "MockProvider",
                "EvaluatorRegistry",
                "register_f4_evaluators",
                "evaluate_gate",
                "F4ValidationInputs",
                "ExecutionModel",
                "compare_run",
                "run(",
                "SemanticPatch",
                "Proposal",
                "apply_patch",
                "fixture-contract",
                "expert-answers",
                "hr-leave",
                "reference-psg",
                "unsafe",
                "rand::",
                "readiness_score",
                "generated_at",
                "created_at",
            ] {
                assert!(!source.contains(token), "{name} contains {token}");
            }
        }
        let value: Value = json!(null);
        assert!(value.is_null());
    }
}
