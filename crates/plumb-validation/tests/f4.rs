//! S4.6 contract tests for the seven F4 evaluators (Hotfix 055).
//!
//! Every test lives in `f4_contract` (`cargo test -p plumb-validation --test f4`). Graphs and F4
//! supplemental inputs are built in memory; Scenario type checks come from the real S4.1
//! `plumb_functional::validate_scenario` (dev-dependency) and ScenarioRun traces follow the
//! frozen S4.2/S4.3 wire. Goldens were computed independently in Python (hashlib + RFC 8785).

mod f4_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, GateId, Hash, Id, Timestamp};
    use plumb_functional::{
        relation_id, validate_scenario, verification_obligation_id, ScenarioGiven, ScenarioThen,
        ScenarioValue, ScenarioWhen,
    };
    use plumb_psg::{
        Agent, AgentKind, Assumption, AuditMeta, Calculation, Calendar, DecisionTable, Edge,
        ElementStatus, Entity, Event, Finding, FindingSeverity, Graph, Modality, Node, NodePayload,
        Operation, OperationKind, RelationKind, RelationProperties, Requirement, RequirementKind,
        RequirementLevel, ResolutionDecision, Scenario, ScenarioKind, ScenarioRun,
        ScenarioRunResult, State, Transition, VerificationKind, VerificationObligation,
    };
    use plumb_validation::*;
    use serde_json::{json, Value};

    use ElementStatus::Accepted;
    use RuleResultState::{Fail as FAIL, NotApplicable as NA, Pass as PASS, Waived as WAIVED};

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const SRC: &str = include_str!("../src/rules/f4.rs");

    const VERIFICATION_LINK: &str = "PLUMB.F4.FUNCTION.VERIFICATION_LINK";
    const FOR_EXECUTABLE_BEHAVIOR: &str = "PLUMB.F4.SCENARIO.FOR_EXECUTABLE_BEHAVIOR";
    const GROUNDED: &str = "PLUMB.F4.SCENARIO.GROUNDED";
    const TYPED: &str = "PLUMB.F4.SCENARIO.TYPED";
    const NO_BLOCKING_UNDECIDABLE: &str = "PLUMB.F4.RUN.NO_BLOCKING_UNDECIDABLE";
    const TRACE_COMPLETE: &str = "PLUMB.F4.RUN.TRACE_COMPLETE";
    const ASSUMPTIONS_VISIBLE: &str = "PLUMB.F4.ASSUMPTIONS.VISIBLE";
    const F4_RULES: [&str; 7] = [
        ASSUMPTIONS_VISIBLE,
        VERIFICATION_LINK,
        TRACE_COMPLETE,
        NO_BLOCKING_UNDECIDABLE,
        FOR_EXECUTABLE_BEHAVIOR,
        GROUNDED,
        TYPED,
    ];

    const R_EXEC: &str = "requirement:exec";
    const R_QUALITY: &str = "requirement:quality";
    const OP: &str = "operation:submit";
    const DT: &str = "decision-table:eligibility";
    const TR: &str = "transition:approve";
    const CALC: &str = "calculation:days";
    const CAL: &str = "calendar:office";
    const EV: &str = "event:submitted";
    const S_OP: &str = "scenario:op";
    const S_BOUNDARY: &str = "scenario:boundary";
    const S_RR: &str = "scenario:rule-row";
    const S_TR: &str = "scenario:transition";
    const S_CALC: &str = "scenario:calculation";
    const S_CAL: &str = "scenario:calendar";
    const S_EV: &str = "scenario:event";
    const ASSUMPTION: &str = "assumption:carry-over";
    const FINDING: &str = "fnd:00000000000000a1";

    // Independent Python goldens.
    const GOLDEN_SPEC_HASH: &str =
        "sha256:665dd8646f9763d9ea6628d7919178bb03a3051e1e6b8d7d2cc0737d2d7be19f";
    const GOLDEN_ARTIFACT_HASH: &str =
        "sha256:54372155336ebdb42af8cccd94ada3f5cfde479d1ccfa6417a9fc020263fec79";
    const GOLDEN_INPUT_HASH: &str =
        "sha256:47864b5b483471af71294daf86dc62473223d71b93c95f5ffdcb79dfea4ec11a";

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

    fn node(node_id: &str, payload: NodePayload) -> Node {
        Node {
            id: id(node_id),
            revision: 1,
            status: Accepted,
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

        fn link(&mut self, kind: RelationKind, from: &str, to: &str) -> &mut World {
            let edge_id = relation_id(&kind, &id(from), &id(to)).unwrap();
            self.edges.retain(|e| e.id != edge_id);
            self.edges.push(Edge {
                id: edge_id,
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
            });
            self
        }

        fn unlink(&mut self, kind: RelationKind, from: &str, to: &str) -> &mut World {
            let edge_id = relation_id(&kind, &id(from), &id(to)).unwrap();
            self.edges.retain(|e| e.id != edge_id);
            self
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

        fn edit_scenario(&mut self, scenario_ref: &str, f: impl Fn(&mut Scenario)) {
            let n = self
                .nodes
                .iter_mut()
                .find(|n| n.id == id(scenario_ref))
                .unwrap();
            if let NodePayload::Scenario(s) = &mut n.payload {
                f(s);
            }
        }
    }

    fn requirement(node_id: &str, kind: RequirementKind, method: Option<&str>) -> Node {
        node(
            node_id,
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
        )
    }

    fn scenario(
        scenario_ref: &str,
        kind: ScenarioKind,
        when: ScenarioWhen,
        requirement_refs: &[&str],
        derived_from: &[&str],
        confirmation: &str,
    ) -> Node {
        let encode = |v: &ScenarioValue| serde_json::to_value(v).unwrap();
        node(
            scenario_ref,
            NodePayload::Scenario(Scenario {
                name: format!("Scenario {scenario_ref}"),
                scenario_kind: kind,
                given: ScenarioGiven::<ScenarioValue>::default().to_values(encode),
                when: when.to_value(),
                then: ScenarioThen::<ScenarioValue>::default().to_values(encode),
                requirement_refs: (!requirement_refs.is_empty())
                    .then(|| requirement_refs.iter().map(|r| id(r)).collect()),
                derived_from_refs: (!derived_from.is_empty())
                    .then(|| derived_from.iter().map(|r| id(r)).collect()),
                confirmation_status: Some(confirmation.into()),
            }),
        )
    }

    fn obligation_ref(source: &str, kind: VerificationKind) -> Id {
        verification_obligation_id(&id(PROJECT), &id(source), kind).unwrap()
    }

    /// An Accepted obligation of `kind` verifying `source`, implemented by `scenarios`.
    fn verify(w: &mut World, source: &str, kind: VerificationKind, scenarios: &[&str]) -> Id {
        let o = obligation_ref(source, kind);
        let kind_text = serde_json::to_value(kind).unwrap();
        w.add(node(
            o.as_str(),
            NodePayload::VerificationObligation(VerificationObligation {
                name: format!("Verify {source} via {}", kind_text.as_str().unwrap()),
                verification_kind: kind,
                method: None,
                acceptance_condition: None,
                required_evidence_kind: None,
            }),
        ))
        .link(RelationKind::VerifiedBy, source, o.as_str());
        for s in scenarios {
            w.link(RelationKind::ImplementedAs, o.as_str(), s);
        }
        o
    }

    fn assumption(assumption_ref: &str, status: &str, finding: Option<&str>) -> Node {
        node(
            assumption_ref,
            NodePayload::Assumption(Assumption {
                statement: "Carry-over days default to five.".into(),
                owner_ref: id("agent:analyst"),
                status: status.into(),
                finding_ref: finding.map(id),
                default_value: None,
                expires_at: None,
                risk_ref: None,
            }),
        )
    }

    fn finding(finding_ref: &str, affected: &[&str]) -> Node {
        node(
            finding_ref,
            NodePayload::Finding(Finding {
                code: "PLUMB.S3.AMBIGUITY".into(),
                family: "S3".into(),
                severity: FindingSeverity::Warn,
                message: "Carry-over is ambiguous.".into(),
                status: "Open".into(),
                affected_refs: affected.iter().map(|a| id(a)).collect(),
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            }),
        )
    }

    /// Every grounding origin, an executable and a non-executable source and a visible
    /// assumption.
    fn world() -> World {
        let mut w = World {
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        w.add(node(
            "agent:analyst",
            NodePayload::Agent(Agent {
                agent_kind: AgentKind::Human,
            }),
        ))
        .add(requirement(R_EXEC, RequirementKind::Functional, None))
        .add(requirement(
            R_QUALITY,
            RequirementKind::Quality,
            Some("analysis"),
        ))
        .add(node(
            OP,
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
        ))
        .add(node(
            "entity:leave",
            NodePayload::Entity(Entity {
                name: "Leave".into(),
                description: None,
                aggregate_root: None,
            }),
        ))
        .add(node(
            "state:draft",
            NodePayload::State(State {
                name: "Draft".into(),
            }),
        ))
        .add(node(
            "state:approved",
            NodePayload::State(State {
                name: "Approved".into(),
            }),
        ))
        .add(node(
            TR,
            NodePayload::Transition(Transition {
                stateful_ref: id("entity:leave"),
                from_state: id("state:draft"),
                to_state: id("state:approved"),
                guard_expr: None,
                effect_refs: None,
            }),
        ))
        .add(node(
            DT,
            NodePayload::DecisionTable(DecisionTable {
                name: "Eligibility".into(),
                hit_policy: "FIRST".into(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                rows: Vec::new(),
            }),
        ))
        .add(node(
            CAL,
            NodePayload::Calendar(Calendar {
                time_zone: "Europe/Warsaw".into(),
                week_pattern: json!(["Mon", "Tue", "Wed", "Thu", "Fri"]),
                region: None,
                holiday_source: None,
            }),
        ))
        .add(node(
            CALC,
            NodePayload::Calculation(Calculation {
                name: "Days".into(),
                expression: "1".into(),
                result_type: "Int".into(),
                unit: None,
                rounding: None,
                calendar_ref: None,
                examples: None,
            }),
        ))
        .add(node(
            EV,
            NodePayload::Event(Event {
                name: "Submitted".into(),
                payload_schema_ref: None,
                semantic_type: None,
            }),
        ))
        .link(RelationKind::HasState, "entity:leave", "state:draft")
        .link(RelationKind::HasState, "entity:leave", "state:approved")
        .link(RelationKind::TransitionsVia, TR, OP)
        .add(scenario(
            S_OP,
            ScenarioKind::Acceptance,
            ScenarioWhen::Operation(id(OP)),
            &[R_EXEC],
            &[OP],
            "Confirmed",
        ))
        .add(scenario(
            S_BOUNDARY,
            ScenarioKind::Boundary,
            ScenarioWhen::Operation(id(OP)),
            &[],
            &[OP],
            "Confirmed",
        ))
        .add(scenario(
            S_RR,
            ScenarioKind::RuleRow,
            ScenarioWhen::DecisionTable(id(DT)),
            &[],
            &[DT],
            "Confirmed",
        ))
        .add(scenario(
            S_TR,
            ScenarioKind::StateTransition,
            ScenarioWhen::Transition(id(TR)),
            &[],
            &[TR],
            "Confirmed",
        ))
        .add(scenario(
            S_CALC,
            ScenarioKind::Boundary,
            ScenarioWhen::Calculation(id(CALC)),
            &[],
            &[CALC],
            "Confirmed",
        ))
        .add(scenario(
            S_CAL,
            ScenarioKind::Boundary,
            ScenarioWhen::Calculation(id(CALC)),
            &[],
            &[CAL],
            "Confirmed",
        ))
        .add(scenario(
            S_EV,
            ScenarioKind::Boundary,
            ScenarioWhen::Event(id(EV)),
            &[],
            &[EV],
            "Confirmed",
        ))
        .add(finding(FINDING, &[R_EXEC]))
        .add(assumption(ASSUMPTION, "Accepted", Some(FINDING)));
        verify(
            &mut w,
            R_EXEC,
            VerificationKind::Scenario,
            &[S_OP, S_BOUNDARY],
        );
        verify(&mut w, R_QUALITY, VerificationKind::Analysis, &[]);
        w
    }

    const ALL_SCENARIOS: [&str; 7] = [S_BOUNDARY, S_CALC, S_CAL, S_EV, S_OP, S_RR, S_TR];

    // ------------------------------------------------------------------ traces and runs

    fn exec(step: &str, fields: Value) -> Value {
        let mut s = fields;
        s["step"] = json!(step);
        json!({"phase": "execution", "step": s})
    }

    fn cmp(step: &str, fields: Value) -> Value {
        let mut s = fields;
        s["step"] = json!(step);
        json!({"phase": "comparison", "step": s})
    }

    fn result_text(result: ScenarioRunResult) -> Value {
        serde_json::to_value(result).unwrap()
    }

    /// A complete S4.2/S4.3 wire trace with business-semantic evidence.
    fn complete_trace(scenario_ref: &str, result: ScenarioRunResult) -> Vec<Value> {
        vec![
            exec(
                "materialization_started",
                json!({"scenario_ref": scenario_ref}),
            ),
            exec(
                "given_materialized",
                json!({"attribute_refs": [], "entity_refs": [], "state_refs": []}),
            ),
            exec(
                "footprint_declared",
                json!({"operation_ref": OP, "reads": [], "writes": []}),
            ),
            exec(
                "transition_applied",
                json!({"transition_ref": TR, "from_state": "state:draft", "to_state": "state:approved"}),
            ),
            exec("execution_completed", json!({})),
            cmp("comparison_started", json!({"scenario_ref": scenario_ref})),
            cmp(
                "comparison_completed",
                json!({"result": result_text(result), "diff_count": 0}),
            ),
        ]
    }

    /// An undecidable trace stopping at missing semantics.
    fn undecidable_trace(scenario_ref: &str) -> Vec<Value> {
        vec![
            exec(
                "materialization_started",
                json!({"scenario_ref": scenario_ref}),
            ),
            exec(
                "semantic_missing",
                json!({"missing": [{"owner_ref": CALC, "key": "half_day_fraction", "kind": "semantic_ref"}]}),
            ),
            cmp("comparison_started", json!({"scenario_ref": scenario_ref})),
            cmp(
                "comparison_completed",
                json!({"result": "undecidable", "diff_count": 0}),
            ),
        ]
    }

    fn spec_hash(graph: &Graph, scenario_ref: &str) -> Hash {
        let Some(NodePayload::Scenario(s)) = graph.node(&id(scenario_ref)).map(|n| &n.payload)
        else {
            panic!("no scenario {scenario_ref}");
        };
        scenario_specification_hash(&id(scenario_ref), s).unwrap()
    }

    fn artifact_body(graph: &Graph, run: &ScenarioRun) -> Value {
        json!({
            "version": 1,
            "model_semantic_hash": graph.semantic_hash().unwrap(),
            "scenario_ref": run.scenario_ref,
            "specification_hash": run.specification_hash,
            "result": run.result,
            "trace": run.trace,
            "diffs": [],
        })
    }

    fn with_body(run: ScenarioRun, body: &Value) -> F4ScenarioRunInput {
        let bytes = to_canonical_json(body).unwrap();
        F4ScenarioRunInput {
            run,
            trace_artifact_hash: Hash::content_sha256(&bytes),
            trace_artifact_bytes: bytes,
        }
    }

    fn run_input(
        graph: &Graph,
        scenario_ref: &str,
        result: ScenarioRunResult,
        trace: Vec<Value>,
    ) -> F4ScenarioRunInput {
        let run = ScenarioRun {
            scenario_ref: id(scenario_ref),
            specification_hash: spec_hash(graph, scenario_ref),
            result,
            trace,
        };
        let body = artifact_body(graph, &run);
        with_body(run, &body)
    }

    fn type_checks(graph: &Graph, scenarios: &[&str]) -> Vec<F4ScenarioTypeCheckInput> {
        scenarios
            .iter()
            .map(|s| {
                let Some(NodePayload::Scenario(payload)) = graph.node(&id(s)).map(|n| &n.payload)
                else {
                    panic!("no scenario {s}");
                };
                let status = match validate_scenario(graph, payload) {
                    Ok(_) => F4ScenarioTypeStatus::Valid,
                    Err(e) => F4ScenarioTypeStatus::Invalid {
                        issues: vec![e.to_string()],
                    },
                };
                F4ScenarioTypeCheckInput {
                    scenario_ref: id(s),
                    specification_hash: spec_hash(graph, s),
                    status,
                }
            })
            .collect()
    }

    struct Setup {
        sources: Vec<&'static str>,
        executable: Vec<&'static str>,
        scenarios: Vec<&'static str>,
        blocking: Vec<&'static str>,
        kinds: Vec<(&'static str, Vec<ScenarioKind>)>,
        runs: Vec<(&'static str, ScenarioRunResult, Vec<Value>)>,
        visible: Vec<&'static str>,
    }

    fn setup() -> Setup {
        Setup {
            sources: vec![R_EXEC, R_QUALITY],
            executable: vec![R_EXEC],
            scenarios: ALL_SCENARIOS.to_vec(),
            blocking: vec![S_OP],
            kinds: vec![],
            runs: vec![(
                S_OP,
                ScenarioRunResult::Pass,
                complete_trace(S_OP, ScenarioRunResult::Pass),
            )],
            visible: vec![ASSUMPTION],
        }
    }

    fn inputs(graph: &Graph, s: &Setup) -> F4ValidationInputs {
        let ids = |v: &[&str]| v.iter().map(|r| id(r)).collect::<Vec<_>>();
        let coverage = s
            .executable
            .iter()
            .map(|e| F4ExecutableCoverageInput {
                source_ref: id(e),
                required_scenario_kinds: s
                    .kinds
                    .iter()
                    .find(|(k, _)| k == e)
                    .map(|(_, kinds)| kinds.clone())
                    .unwrap_or_default(),
            })
            .collect();
        F4ValidationInputs::new(
            F4ApplicabilityInput {
                source_refs: ids(&s.sources),
                executable_source_refs: ids(&s.executable),
                in_scope_scenario_refs: ids(&s.scenarios),
                blocking_scenario_refs: ids(&s.blocking),
                executable_coverage: coverage,
            },
            type_checks(graph, &s.scenarios),
            s.runs
                .iter()
                .map(|(r, result, trace)| run_input(graph, r, *result, trace.clone()))
                .collect(),
            ids(&s.visible),
        )
    }

    fn metadata() -> ValidationRegistry {
        ValidationRegistry::new(load_builtin_software_profile().unwrap()).unwrap()
    }

    fn registry() -> EvaluatorRegistry {
        let mut registry = EvaluatorRegistry::new(metadata());
        rules::register_f4_evaluators(&mut registry).unwrap();
        registry
    }

    fn base_context(graph: &Graph) -> ValidationContext {
        ValidationContext::new(
            graph,
            &metadata(),
            ValidationPolicy::default(),
            graph.evidence_hash().unwrap(),
            vec![],
            vec![],
        )
        .unwrap()
    }

    fn try_evaluate(
        graph: &Graph,
        inputs: F4ValidationInputs,
    ) -> Result<GateReport, EvaluationError> {
        let ctx = base_context(graph).with_f4_inputs(graph, inputs)?;
        registry().evaluate_gate(GateId::F4, graph, &ctx)
    }

    fn evaluate(w: &World, s: &Setup) -> GateReport {
        let graph = w.graph();
        try_evaluate(&graph, inputs(&graph, s)).unwrap()
    }

    fn rule<'a>(report: &'a GateReport, rule_id: &str) -> &'a RuleResult {
        report.rules.iter().find(|r| r.rule_id == rule_id).unwrap()
    }

    fn state(report: &GateReport, rule_id: &str) -> RuleResultState {
        rule(report, rule_id).state
    }

    fn ids(items: &[&str]) -> Vec<Id> {
        items.iter().map(|i| id(i)).collect()
    }

    fn strings(items: &[String]) -> Vec<&str> {
        items.iter().map(String::as_str).collect()
    }

    /// Asserts one isolated violation with its exact targets, key and stable finding.
    fn assert_violation(report: &GateReport, rule_id: &str, targets: &[&str], key: &str) {
        let r = rule(report, rule_id);
        assert_eq!(r.state, FAIL, "{rule_id}");
        assert_eq!(r.targets, ids(targets), "{rule_id}");
        assert_eq!(r.semantic_condition_key.as_deref(), Some(key), "{rule_id}");
        let expected_key = finding_key(rule_id, &ids(targets), key).unwrap();
        let expected_id = finding_id(&expected_key).unwrap();
        assert_eq!(r.finding_ref.as_ref(), Some(&expected_id), "{rule_id}");
        let finding = report
            .findings
            .iter()
            .find(|f| f.id == expected_id)
            .unwrap();
        assert_eq!(finding.key, expected_key);
        assert_eq!(finding.payload.code, rule_id);
        assert_eq!(finding.payload.family, "F4");
        let declared = metadata().rule(rule_id).unwrap().severity;
        assert_eq!(finding.payload.severity, finding_severity(declared));
        for other in F4_RULES.iter().filter(|r| **r != rule_id) {
            assert_ne!(state(report, other), FAIL, "{other} also failed");
        }
    }

    fn context_error(graph: &Graph, inputs: F4ValidationInputs) -> bool {
        matches!(
            base_context(graph).with_f4_inputs(graph, inputs),
            Err(EvaluationError::InvalidContext(_))
        )
    }

    // ------------------------------------------------------------------ full gate

    #[test]
    fn full_f4_fixture_passes_every_rule() {
        let report = evaluate(&world(), &setup());
        for rule_id in F4_RULES {
            assert_eq!(state(&report, rule_id), PASS, "{rule_id}");
        }
        assert_eq!(report.result, GateResult::Pass);
        assert!(report.findings.is_empty());
        assert_eq!(
            rule(&report, VERIFICATION_LINK).targets,
            ids(&[R_EXEC, R_QUALITY])
        );
        assert_eq!(
            rule(&report, FOR_EXECUTABLE_BEHAVIOR).targets,
            ids(&[R_EXEC])
        );
        assert_eq!(rule(&report, GROUNDED).targets, ids(&ALL_SCENARIOS));
        assert_eq!(rule(&report, TYPED).targets, ids(&ALL_SCENARIOS));
        assert_eq!(rule(&report, NO_BLOCKING_UNDECIDABLE).targets, ids(&[S_OP]));
        assert_eq!(rule(&report, TRACE_COMPLETE).targets, ids(&[S_OP]));
        let visible = rule(&report, ASSUMPTIONS_VISIBLE);
        assert_eq!(visible.targets, ids(&[ASSUMPTION]));
        assert_eq!(
            strings(&visible.evidence),
            vec![format!("visible_assumption:{ASSUMPTION}").as_str()]
        );
    }

    #[test]
    fn f4_requires_its_supplemental_inputs() {
        let graph = world().graph();
        let err = registry()
            .evaluate_gate(GateId::F4, &graph, &base_context(&graph))
            .unwrap_err();
        assert_eq!(
            err,
            EvaluationError::InvalidContext("F4 validation inputs are required".into())
        );
    }

    // ------------------------------------------------------------------ verification

    #[test]
    fn non_executable_source_verifies_without_scenarios() {
        let mut s = setup();
        s.sources = vec![R_QUALITY];
        s.executable = vec![];
        s.visible = vec![];
        s.scenarios = vec![];
        s.blocking = vec![];
        s.runs = vec![];
        let report = evaluate(&world(), &s);
        assert_eq!(state(&report, VERIFICATION_LINK), PASS);
        assert_eq!(rule(&report, VERIFICATION_LINK).targets, ids(&[R_QUALITY]));
        for na in [
            FOR_EXECUTABLE_BEHAVIOR,
            GROUNDED,
            TYPED,
            NO_BLOCKING_UNDECIDABLE,
            TRACE_COMPLETE,
            ASSUMPTIONS_VISIBLE,
        ] {
            assert_eq!(state(&report, na), NA, "{na}");
        }
        assert_eq!(report.result, GateResult::Pass);
    }

    /// E0.4 mutation gap: only an Accepted verified_by edge verifies.
    #[test]
    fn non_accepted_verification_links_do_not_verify() {
        let o = obligation_ref(R_QUALITY, VerificationKind::Analysis);
        let mut proposed_edge = world();
        for e in &mut proposed_edge.edges {
            if e.kind == RelationKind::VerifiedBy && e.to == o {
                e.status = ElementStatus::Proposed;
            }
        }
        // (An Accepted edge to a Proposed obligation is rejected by graph validation itself.)
        let report = evaluate(&proposed_edge, &setup());
        assert_violation(
            &report,
            VERIFICATION_LINK,
            &[R_QUALITY],
            "implementation_scoped_source_unverified",
        );
    }

    #[test]
    fn unverified_source_violates_verification_link() {
        let mut w = world();
        let o = obligation_ref(R_QUALITY, VerificationKind::Analysis);
        w.unlink(RelationKind::VerifiedBy, R_QUALITY, o.as_str());
        let report = evaluate(&w, &setup());
        assert_violation(
            &report,
            VERIFICATION_LINK,
            &[R_QUALITY],
            "implementation_scoped_source_unverified",
        );
        let mut s = setup();
        s.sources = vec![];
        s.executable = vec![];
        assert_eq!(state(&evaluate(&world(), &s), VERIFICATION_LINK), NA);
    }

    #[test]
    fn executable_sources_need_scenario_coverage() {
        // A non-scenario obligation does not cover executable behaviour.
        let mut w = world();
        let o = obligation_ref(R_EXEC, VerificationKind::Scenario);
        w.unlink(RelationKind::VerifiedBy, R_EXEC, o.as_str());
        verify(&mut w, R_EXEC, VerificationKind::Review, &[]);
        let report = evaluate(&w, &setup());
        assert_violation(
            &report,
            FOR_EXECUTABLE_BEHAVIOR,
            &[R_EXEC],
            "executable_source_scenario_coverage_incomplete",
        );
        assert_eq!(
            strings(&rule(&report, FOR_EXECUTABLE_BEHAVIOR).evidence),
            vec![format!("missing_scenario:{R_EXEC}").as_str()]
        );
        // Missing implemented_as.
        let mut w = world();
        let o = obligation_ref(R_EXEC, VerificationKind::Scenario);
        w.unlink(RelationKind::ImplementedAs, o.as_str(), S_OP)
            .unlink(RelationKind::ImplementedAs, o.as_str(), S_BOUNDARY);
        let report = evaluate(&w, &setup());
        assert_violation(
            &report,
            FOR_EXECUTABLE_BEHAVIOR,
            &[R_EXEC],
            "executable_source_scenario_coverage_incomplete",
        );
        // Implementations outside the F4 scope do not count.
        let mut s = setup();
        s.scenarios = vec![S_RR];
        s.blocking = vec![];
        s.runs = vec![];
        let report = evaluate(&world(), &s);
        assert_eq!(state(&report, FOR_EXECUTABLE_BEHAVIOR), FAIL);
    }

    #[test]
    fn required_scenario_kinds_are_explicit() {
        let mut s = setup();
        s.kinds = vec![(
            R_EXEC,
            vec![ScenarioKind::Acceptance, ScenarioKind::Boundary],
        )];
        assert_eq!(
            state(&evaluate(&world(), &s), FOR_EXECUTABLE_BEHAVIOR),
            PASS
        );
        let mut w = world();
        let o = obligation_ref(R_EXEC, VerificationKind::Scenario);
        w.unlink(RelationKind::ImplementedAs, o.as_str(), S_BOUNDARY);
        let report = evaluate(&w, &s);
        assert_eq!(state(&report, FOR_EXECUTABLE_BEHAVIOR), FAIL);
        assert_eq!(
            strings(&rule(&report, FOR_EXECUTABLE_BEHAVIOR).evidence),
            vec![format!("missing_kind:{R_EXEC}:boundary").as_str()]
        );
    }

    #[test]
    fn executable_and_non_executable_sources_coexist() {
        let report = evaluate(&world(), &setup());
        assert_eq!(
            rule(&report, VERIFICATION_LINK).targets,
            ids(&[R_EXEC, R_QUALITY])
        );
        assert_eq!(
            rule(&report, FOR_EXECUTABLE_BEHAVIOR).targets,
            ids(&[R_EXEC])
        );
        assert_eq!(report.result, GateResult::Pass);
    }

    // ------------------------------------------------------------------ scenarios

    #[test]
    fn every_s41_origin_grounds_and_others_do_not() {
        let report = evaluate(&world(), &setup());
        assert_eq!(state(&report, GROUNDED), PASS);
        let mut w = world();
        w.add(node(
            "attr:days",
            NodePayload::Attribute(plumb_psg::Attribute {
                name: "days".into(),
                value_type: "Int".into(),
                nullable: false,
                unit: None,
                precision: None,
                enum_values: None,
                data_classification: None,
            }),
        ))
        .link(RelationKind::HasAttribute, "entity:leave", "attr:days")
        .add(node(
            "process:leave",
            NodePayload::Process(plumb_psg::Process {
                name: "Leave".into(),
                description: None,
                process_kind: None,
            }),
        ))
        .add(scenario(
            "scenario:ungrounded",
            ScenarioKind::Boundary,
            ScenarioWhen::Calculation(id(CALC)),
            &[],
            &["attr:days", "process:leave"],
            "Confirmed",
        ));
        let mut s = setup();
        s.scenarios.push("scenario:ungrounded");
        let report = evaluate(&w, &s);
        assert_violation(
            &report,
            GROUNDED,
            &["scenario:ungrounded"],
            "scenario_not_grounded",
        );
    }

    #[test]
    fn typed_status_comes_from_the_real_s41_validator() {
        let mut w = world();
        // states_active names an Operation: the real S4.1 validator rejects it.
        let encode = |v: &ScenarioValue| serde_json::to_value(v).unwrap();
        let given: ScenarioGiven<ScenarioValue> = ScenarioGiven {
            states_active: vec![id(OP)],
            ..ScenarioGiven::default()
        };
        let mut bad = scenario(
            "scenario:mistyped",
            ScenarioKind::Boundary,
            ScenarioWhen::Operation(id(OP)),
            &[],
            &[OP],
            "Confirmed",
        );
        if let NodePayload::Scenario(s) = &mut bad.payload {
            s.given = given.to_values(encode);
        }
        w.add(bad);
        let mut s = setup();
        s.scenarios.push("scenario:mistyped");
        let graph = w.graph();
        let checks = type_checks(&graph, &["scenario:mistyped"]);
        let F4ScenarioTypeStatus::Invalid { issues } = &checks[0].status else {
            panic!("the real validator accepted the mistyped scenario");
        };
        let report = evaluate(&w, &s);
        assert_violation(&report, TYPED, &["scenario:mistyped"], "scenario_not_typed");
        assert_eq!(
            strings(&rule(&report, TYPED).evidence),
            vec![format!("type_issue:scenario:mistyped:{}", issues[0]).as_str()]
        );
        // A type check made before the payload changed is stale context.
        let before = inputs(&world().graph(), &setup());
        let mut changed = world();
        changed.edit_scenario(S_OP, |s| s.name = "Renamed".into());
        assert!(context_error(&changed.graph(), before));
    }

    // ------------------------------------------------------------------ runs

    #[test]
    fn blocking_run_matrix() {
        let cases = [
            (Some(ScenarioRunResult::Pass), None),
            (Some(ScenarioRunResult::Fail), Some("run_fail")),
            (
                Some(ScenarioRunResult::Undecidable),
                Some("run_undecidable"),
            ),
            (None, Some("missing_run")),
        ];
        for (result, evidence) in cases {
            let mut s = setup();
            s.runs = match result {
                Some(ScenarioRunResult::Undecidable) => vec![(
                    S_OP,
                    ScenarioRunResult::Undecidable,
                    undecidable_trace(S_OP),
                )],
                Some(r) => vec![(S_OP, r, complete_trace(S_OP, r))],
                None => vec![],
            };
            let report = evaluate(&world(), &s);
            match evidence {
                None => assert_eq!(state(&report, NO_BLOCKING_UNDECIDABLE), PASS),
                Some(e) => {
                    assert_violation(
                        &report,
                        NO_BLOCKING_UNDECIDABLE,
                        &[S_OP],
                        "blocking_scenario_run_not_acceptable",
                    );
                    assert_eq!(
                        strings(&rule(&report, NO_BLOCKING_UNDECIDABLE).evidence),
                        vec![format!("{e}:{S_OP}").as_str()]
                    );
                }
            }
        }
    }

    #[test]
    fn non_blocking_undecidable_runs_are_ignored() {
        let mut s = setup();
        s.runs.push((
            S_CALC,
            ScenarioRunResult::Undecidable,
            undecidable_trace(S_CALC),
        ));
        let report = evaluate(&world(), &s);
        assert_eq!(state(&report, NO_BLOCKING_UNDECIDABLE), PASS);
        assert_eq!(state(&report, TRACE_COMPLETE), PASS);
        s.blocking = vec![];
        assert_eq!(state(&evaluate(&world(), &s), NO_BLOCKING_UNDECIDABLE), NA);
    }

    #[test]
    fn stale_or_unbound_run_evidence_is_invalid_context() {
        let w = world();
        let graph = w.graph();
        let good = inputs(&graph, &setup());
        let with_run = |run: F4ScenarioRunInput| {
            let mut i = good.clone();
            i.scenario_runs = vec![run];
            i
        };
        let base = good.scenario_runs[0].clone();
        // Stale specification: the Scenario changed after the run.
        let mut changed = w.clone();
        changed.edit_scenario(S_OP, |s| s.name = "Renamed".into());
        let changed_graph = changed.graph();
        let mut stale = inputs(&changed_graph, &setup());
        stale.scenario_runs = vec![base.clone()];
        assert!(context_error(&changed_graph, stale));
        // Wrong model baseline.
        let mut body = artifact_body(&graph, &base.run);
        body["model_semantic_hash"] = json!(changed_graph.semantic_hash().unwrap());
        assert!(context_error(
            &graph,
            with_run(with_body(base.run.clone(), &body))
        ));
        // One mutated byte.
        let mut mutated = base.clone();
        let last = mutated.trace_artifact_bytes.len() - 2;
        mutated.trace_artifact_bytes[last] = b' ';
        assert!(context_error(&graph, with_run(mutated)));
        // Noncanonical but semantically equal bytes.
        let mut pretty = base.clone();
        pretty.trace_artifact_bytes =
            serde_json::to_vec_pretty(&artifact_body(&graph, &base.run)).unwrap();
        pretty.trace_artifact_hash = Hash::content_sha256(&pretty.trace_artifact_bytes);
        assert!(context_error(&graph, with_run(pretty)));
        // Artifact and run disagree.
        for field in ["scenario_ref", "specification_hash", "result", "trace"] {
            let mut body = artifact_body(&graph, &base.run);
            body[field] = match field {
                "scenario_ref" => json!(S_BOUNDARY),
                "specification_hash" => json!(spec_hash(&graph, S_BOUNDARY)),
                "result" => json!("fail"),
                _ => json!([]),
            };
            assert!(
                context_error(&graph, with_run(with_body(base.run.clone(), &body))),
                "{field}"
            );
        }
        // Extra envelope field, out-of-scope run and duplicate runs.
        let mut body = artifact_body(&graph, &base.run);
        body["extra"] = json!(1);
        assert!(context_error(
            &graph,
            with_run(with_body(base.run.clone(), &body))
        ));
        let mut narrow = good.clone();
        narrow.applicability.in_scope_scenario_refs = ids(&[S_OP]);
        narrow
            .scenario_type_checks
            .retain(|c| c.scenario_ref == id(S_OP));
        narrow.scenario_runs.push(run_input(
            &graph,
            S_CALC,
            ScenarioRunResult::Pass,
            complete_trace(S_CALC, ScenarioRunResult::Pass),
        ));
        assert!(context_error(&graph, narrow));
        let mut twice = good.clone();
        twice.scenario_runs.push(base);
        assert!(context_error(&graph, twice));
        assert!(!context_error(&graph, good));
    }

    #[test]
    fn trace_completeness_is_checked_on_the_wire() {
        let check = |trace: Vec<Value>, result: ScenarioRunResult| {
            let mut s = setup();
            s.blocking = vec![];
            s.runs = vec![(S_OP, result, trace)];
            evaluate(&world(), &s)
        };
        assert_eq!(
            state(
                &check(
                    complete_trace(S_OP, ScenarioRunResult::Pass),
                    ScenarioRunResult::Pass
                ),
                TRACE_COMPLETE
            ),
            PASS
        );
        assert_eq!(
            state(
                &check(
                    complete_trace(S_OP, ScenarioRunResult::Fail),
                    ScenarioRunResult::Fail
                ),
                TRACE_COMPLETE
            ),
            PASS
        );
        let mut unknown = complete_trace(S_OP, ScenarioRunResult::Pass);
        unknown.insert(2, exec("execution_failed", json!({})));
        let mut bad_order = complete_trace(S_OP, ScenarioRunResult::Pass);
        let moved = bad_order.remove(3);
        bad_order.insert(5, moved);
        let lifecycle = vec![
            exec("materialization_started", json!({"scenario_ref": S_OP})),
            exec("given_materialized", json!({})),
            cmp("comparison_started", json!({"scenario_ref": S_OP})),
            cmp(
                "comparison_completed",
                json!({"result": "pass", "diff_count": 0}),
            ),
        ];
        // Fable #165: the execution phase ends with its S4.2 terminal step.
        let without_terminal = |result| {
            let mut t = complete_trace(S_OP, result);
            t.retain(|e| e["step"]["step"] != "execution_completed");
            t
        };
        // A stopped (business-failed) execution has no execution_completed and its comparison
        // records no expectation steps: complete for a Fail run.
        assert_eq!(
            state(
                &check(
                    without_terminal(ScenarioRunResult::Fail),
                    ScenarioRunResult::Fail
                ),
                TRACE_COMPLETE
            ),
            PASS
        );
        let mut compared_without_terminal = without_terminal(ScenarioRunResult::Fail);
        compared_without_terminal.insert(
            5,
            cmp(
                "attribute_compared",
                json!({"attribute_ref": "attr:days", "matched": false}),
            ),
        );
        let mut missing_after_completion = complete_trace(S_OP, ScenarioRunResult::Pass);
        missing_after_completion.insert(
            5,
            exec(
                "semantic_missing",
                json!({"missing": [{"owner_ref": CALC, "key": "half_day_fraction", "kind": "semantic_ref"}]}),
            ),
        );
        let cases = [
            (
                without_terminal(ScenarioRunResult::Pass),
                ScenarioRunResult::Pass,
                "trace_execution_incomplete",
            ),
            (
                compared_without_terminal,
                ScenarioRunResult::Fail,
                "trace_execution_incomplete",
            ),
            (
                missing_after_completion,
                ScenarioRunResult::Pass,
                "trace_execution_incomplete",
            ),
            (unknown, ScenarioRunResult::Pass, "trace_unknown_step"),
            (bad_order, ScenarioRunResult::Pass, "trace_bad_order"),
            (
                complete_trace(S_OP, ScenarioRunResult::Fail),
                ScenarioRunResult::Pass,
                "trace_result_mismatch",
            ),
            (
                lifecycle,
                ScenarioRunResult::Pass,
                "trace_no_semantic_evidence",
            ),
            (
                vec![],
                ScenarioRunResult::Pass,
                "trace_missing_execution_phase",
            ),
        ];
        for (trace, result, reason) in cases {
            let report = check(trace, result);
            assert_violation(
                &report,
                TRACE_COMPLETE,
                &[S_OP],
                "scenario_trace_incomplete",
            );
            assert_eq!(
                strings(&rule(&report, TRACE_COMPLETE).evidence),
                vec![format!("{reason}:{S_OP}").as_str()],
                "{reason}"
            );
        }
    }

    #[test]
    fn undecidable_trace_with_missing_semantics_is_complete_but_blocking_fails() {
        let mut s = setup();
        s.runs = vec![(
            S_OP,
            ScenarioRunResult::Undecidable,
            undecidable_trace(S_OP),
        )];
        let report = evaluate(&world(), &s);
        assert_eq!(state(&report, TRACE_COMPLETE), PASS);
        assert_eq!(state(&report, NO_BLOCKING_UNDECIDABLE), FAIL);
        assert_eq!(state(&report, TYPED), PASS);
    }

    // ------------------------------------------------------------------ assumptions

    #[test]
    fn assumption_visibility_is_exact() {
        // Missing.
        let mut s = setup();
        s.visible = vec![];
        let report = evaluate(&world(), &s);
        assert_violation(
            &report,
            ASSUMPTIONS_VISIBLE,
            &[ASSUMPTION],
            "execution_assumptions_not_visible",
        );
        assert_eq!(
            strings(&rule(&report, ASSUMPTIONS_VISIBLE).evidence),
            vec![format!("expected_assumption:{ASSUMPTION}").as_str()]
        );
        // Unexpected: an active assumption whose finding touches nothing covered.
        let mut w = world();
        w.add(finding("fnd:00000000000000b2", &["entity:leave"]))
            .add(assumption(
                "assumption:unrelated",
                "Accepted",
                Some("fnd:00000000000000b2"),
            ));
        let mut s = setup();
        s.visible = vec![ASSUMPTION, "assumption:unrelated"];
        let report = evaluate(&w, &s);
        assert_violation(
            &report,
            ASSUMPTIONS_VISIBLE,
            &["assumption:unrelated"],
            "execution_assumptions_not_visible",
        );
        // Inactive and unlinked assumptions are never expected.
        let mut w = world();
        w.add(assumption(ASSUMPTION, "Open", Some(FINDING)))
            .add(assumption("assumption:unlinked", "Accepted", None));
        let mut s = setup();
        s.visible = vec![];
        assert_eq!(state(&evaluate(&w, &s), ASSUMPTIONS_VISIBLE), NA);
        // An inactive visible assumption is malformed context.
        s.visible = vec![ASSUMPTION];
        let graph = w.graph();
        assert!(context_error(&graph, inputs(&graph, &s)));
    }

    // ------------------------------------------------------------------ context

    #[test]
    fn malformed_applicability_is_invalid_context() {
        let graph = world().graph();
        let good = inputs(&graph, &setup());
        let mutate = |f: &dyn Fn(&mut F4ValidationInputs)| {
            let mut i = good.clone();
            f(&mut i);
            context_error(&graph, i)
        };
        assert!(mutate(&|i| i.applicability.source_refs.push(id(S_OP))));
        assert!(mutate(&|i| i
            .applicability
            .executable_source_refs
            .push(id(OP))));
        assert!(mutate(&|i| i.applicability.executable_coverage.clear()));
        assert!(mutate(
            &|i| i.applicability.blocking_scenario_refs = ids(&["scenario:unknown"])
        ));
        assert!(mutate(&|i| {
            i.scenario_type_checks.pop();
        }));
        assert!(mutate(&|i| i.applicability.source_refs.push(id(R_EXEC))));
        assert!(mutate(&|i| i.visible_assumption_refs.push(id(FINDING))));
        // A Derived Scenario is not in scope.
        let mut w = world();
        w.add(scenario(
            "scenario:derived",
            ScenarioKind::Boundary,
            ScenarioWhen::Operation(id(OP)),
            &[],
            &[OP],
            "Derived",
        ));
        let mut s = setup();
        s.scenarios.push("scenario:derived");
        let graph = w.graph();
        assert!(context_error(&graph, inputs(&graph, &s)));
    }

    // ------------------------------------------------------------------ waivers

    fn waive(w: &mut World, rule_id: &str, targets: &[&str], key: &str) {
        let k = finding_key(rule_id, &ids(targets), key).unwrap();
        let f = finding_id(&k).unwrap();
        w.add(node(
            f.as_str(),
            NodePayload::Finding(Finding {
                code: rule_id.into(),
                family: "F4".into(),
                severity: FindingSeverity::Blocker,
                message: "waived".into(),
                status: "Open".into(),
                affected_refs: ids(targets),
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            }),
        ))
        .add(node(
            "dec:00000000000000c1",
            NodePayload::ResolutionDecision(ResolutionDecision {
                question_ref: None,
                proposal_ref: Some(id("prop:00000000000000c1")),
                answer: json!({"kind": WAIVER_KIND, "rule_id": rule_id, "finding_key": k, "finding_ref": f}),
                decided_by: id("agent:analyst"),
                decided_at: ts(AT),
                patch_ref: Hash::content_sha256(b"waiver"),
                rationale: Some("Accepted for the pilot.".into()),
                supersedes: None,
            }),
        ))
        .link(RelationKind::Resolves, "dec:00000000000000c1", f.as_str());
    }

    #[test]
    fn waivers_follow_rule_metadata() {
        let mut w = world();
        let o = obligation_ref(R_QUALITY, VerificationKind::Analysis);
        w.unlink(RelationKind::VerifiedBy, R_QUALITY, o.as_str());
        waive(
            &mut w,
            VERIFICATION_LINK,
            &[R_QUALITY],
            "implementation_scoped_source_unverified",
        );
        let report = evaluate(&w, &setup());
        assert_eq!(state(&report, VERIFICATION_LINK), WAIVED);
        assert_eq!(report.result, GateResult::Pass);

        let mut w = world();
        let encode = |v: &ScenarioValue| serde_json::to_value(v).unwrap();
        let given: ScenarioGiven<ScenarioValue> = ScenarioGiven {
            states_active: vec![id(OP)],
            ..ScenarioGiven::default()
        };
        let mut bad = scenario(
            "scenario:mistyped",
            ScenarioKind::Boundary,
            ScenarioWhen::Operation(id(OP)),
            &[],
            &[OP],
            "Confirmed",
        );
        if let NodePayload::Scenario(s) = &mut bad.payload {
            s.given = given.to_values(encode);
        }
        w.add(bad);
        waive(&mut w, TYPED, &["scenario:mistyped"], "scenario_not_typed");
        let mut s = setup();
        s.scenarios.push("scenario:mistyped");
        let graph = w.graph();
        assert!(matches!(
            try_evaluate(&graph, inputs(&graph, &s)),
            Err(EvaluationError::ForbiddenWaiver { .. })
        ));

        let mut w = world();
        waive(
            &mut w,
            ASSUMPTIONS_VISIBLE,
            &[ASSUMPTION],
            "execution_assumptions_not_visible",
        );
        let mut s = setup();
        s.visible = vec![];
        let graph = w.graph();
        assert!(matches!(
            try_evaluate(&graph, inputs(&graph, &s)),
            Err(EvaluationError::ForbiddenWaiver { .. })
        ));
    }

    // ------------------------------------------------------------------ registry

    #[test]
    fn registration_binds_exactly_seven_and_completes_the_now_set() {
        let mut registry = EvaluatorRegistry::new(metadata());
        assert_eq!(registry.missing_for_gate(GateId::F4).len(), 7);
        rules::register_f4_evaluators(&mut registry).unwrap();
        assert!(registry.missing_for_gate(GateId::F4).is_empty());
        let gate: BTreeSet<&str> = registry
            .metadata()
            .gate(GateId::F4)
            .rule_ids
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(gate, F4_RULES.into_iter().collect());
        assert!(matches!(
            rules::register_f4_evaluators(&mut registry),
            Err(EvaluationError::DuplicateEvaluator(_))
        ));

        let mut all = EvaluatorRegistry::new(metadata());
        rules::register_i0_evaluators(&mut all).unwrap();
        rules::register_f1_evaluators(&mut all).unwrap();
        rules::register_f2_evaluators(&mut all).unwrap();
        rules::register_f3_evaluators(&mut all).unwrap();
        rules::register_f4_evaluators(&mut all).unwrap();
        let required: BTreeSet<String> = all
            .metadata()
            .required_now_rule_ids()
            .into_iter()
            .map(|r| r.to_string())
            .collect();
        assert_eq!(required.len(), EXPECTED_NOW_RULE_COUNT);
        assert_eq!(EXPECTED_NOW_RULE_COUNT, 54);
        for gate in NOW_GATES {
            assert!(all.missing_for_gate(gate).is_empty(), "{gate}");
        }
        let bound: BTreeSet<String> = required
            .iter()
            .filter(|r| all.has_evaluator(r))
            .cloned()
            .collect();
        assert_eq!(bound, required);
        for code in ["I_SCENARIO_FAIL", "G_SCENARIO_UNDECIDABLE"] {
            assert!(all.metadata().rule(code).is_none());
            assert!(!all.has_evaluator(code));
        }
        // Metadata stays in the profile: gate, severity and waiver policy are not shadowed.
        for rule_id in F4_RULES {
            let rule = all.metadata().rule(rule_id).unwrap();
            assert_eq!(rule.gate, GateId::F4);
            assert!(!SRC.contains("Severity::") && !SRC.contains("WaiverPolicy::"));
        }
        assert_eq!(
            all.metadata().rule(TYPED).unwrap().waiver_policy,
            WaiverPolicy::Forbidden
        );
        assert_eq!(
            all.metadata()
                .rule(ASSUMPTIONS_VISIBLE)
                .unwrap()
                .waiver_policy,
            WaiverPolicy::Forbidden
        );
    }

    // ------------------------------------------------------------------ HR sequence

    /// The HR half-day boundary as a blocking executable Scenario (fixture answers are only the
    /// test's expectation of what the accepted semantics produce).
    fn hr_world() -> World {
        let mut w = world();
        w.add(requirement(
            "requirement:hr-007",
            RequirementKind::Functional,
            None,
        ))
        .add(scenario(
            "scenario:hr-half-day",
            ScenarioKind::Boundary,
            ScenarioWhen::Calculation(id(CALC)),
            &["requirement:hr-007"],
            &[CALC],
            "Confirmed",
        ));
        verify(
            &mut w,
            "requirement:hr-007",
            VerificationKind::Scenario,
            &["scenario:hr-half-day"],
        );
        w
    }

    fn hr_setup(result: ScenarioRunResult) -> Setup {
        let mut s = setup();
        s.sources = vec![R_EXEC, R_QUALITY, "requirement:hr-007"];
        s.executable = vec![R_EXEC, "requirement:hr-007"];
        s.scenarios.push("scenario:hr-half-day");
        s.blocking = vec![S_OP, "scenario:hr-half-day"];
        let trace = match result {
            ScenarioRunResult::Undecidable => undecidable_trace("scenario:hr-half-day"),
            r => vec![
                exec(
                    "materialization_started",
                    json!({"scenario_ref": "scenario:hr-half-day"}),
                ),
                exec(
                    "semantic_resolved",
                    json!({"owner_ref": CALC, "key": "half_day_fraction", "source": "calculation_example"}),
                ),
                exec(
                    "calculation_evaluated",
                    json!({"calculation_ref": CALC, "value": {"kind": "decimal", "value": "2.5", "unit": "working_day"}}),
                ),
                exec("execution_completed", json!({})),
                cmp(
                    "comparison_started",
                    json!({"scenario_ref": "scenario:hr-half-day"}),
                ),
                cmp(
                    "calculation_compared",
                    json!({"calculation_ref": CALC, "matched": true}),
                ),
                cmp(
                    "comparison_completed",
                    json!({"result": result_text(r), "diff_count": 0}),
                ),
            ],
        };
        s.runs.push(("scenario:hr-half-day", result, trace));
        s
    }

    #[test]
    fn hr_fails_before_answers_and_passes_after() {
        let w = hr_world();
        let pre = evaluate(&w, &hr_setup(ScenarioRunResult::Undecidable));
        assert_violation(
            &pre,
            NO_BLOCKING_UNDECIDABLE,
            &["scenario:hr-half-day"],
            "blocking_scenario_run_not_acceptable",
        );
        assert_eq!(state(&pre, TYPED), PASS);
        assert_eq!(state(&pre, TRACE_COMPLETE), PASS);
        assert_eq!(pre.result, GateResult::Fail);
        let post = evaluate(&w, &hr_setup(ScenarioRunResult::Pass));
        for rule_id in F4_RULES {
            assert_eq!(state(&post, rule_id), PASS, "{rule_id}");
        }
        assert_eq!(post.result, GateResult::Pass);
    }

    // ------------------------------------------------------------------ determinism and goldens

    #[test]
    fn inputs_and_results_are_order_independent() {
        let w = world();
        let forward_graph = w.graph();
        let reverse_graph = w.reversed_graph();
        let mut s = setup();
        s.runs.push((
            S_CALC,
            ScenarioRunResult::Undecidable,
            undecidable_trace(S_CALC),
        ));
        let forward = inputs(&forward_graph, &s);
        let mut reversed = s;
        reversed.sources.reverse();
        reversed.scenarios.reverse();
        reversed.runs.reverse();
        let reverse = inputs(&reverse_graph, &reversed);
        assert_eq!(forward, reverse);
        assert_eq!(
            forward.content_hash().unwrap(),
            reverse.content_hash().unwrap()
        );
        let a = try_evaluate(&forward_graph, forward).unwrap();
        let b = try_evaluate(&reverse_graph, reverse).unwrap();
        assert_eq!(a, b);
    }

    fn golden_scenario() -> Scenario {
        Scenario {
            name: "Golden".into(),
            scenario_kind: ScenarioKind::Boundary,
            given: vec![],
            when: json!({"operation": "operation:submit"}),
            then: vec![],
            requirement_refs: None,
            derived_from_refs: Some(vec![id("operation:submit")]),
            confirmation_status: Some("Confirmed".into()),
        }
    }

    #[test]
    fn identity_helpers_match_independent_goldens() {
        let spec = scenario_specification_hash(&id("scenario:golden"), &golden_scenario()).unwrap();
        assert_eq!(spec.to_string(), GOLDEN_SPEC_HASH);
        let artifact = br#"{"diffs":[],"model_semantic_hash":"psg:sha256:0000000000000000000000000000000000000000000000000000000000000000","result":"pass","scenario_ref":"scenario:golden","specification_hash":"sha256:1111111111111111111111111111111111111111111111111111111111111111","trace":[],"version":1}"#;
        assert_eq!(
            Hash::content_sha256(artifact).to_string(),
            GOLDEN_ARTIFACT_HASH
        );
        let inputs = F4ValidationInputs::new(
            F4ApplicabilityInput {
                source_refs: ids(&["requirement:b", "requirement:a"]),
                executable_source_refs: ids(&["requirement:a"]),
                in_scope_scenario_refs: ids(&["scenario:golden"]),
                blocking_scenario_refs: ids(&["scenario:golden"]),
                executable_coverage: vec![F4ExecutableCoverageInput {
                    source_ref: id("requirement:a"),
                    required_scenario_kinds: vec![ScenarioKind::Boundary, ScenarioKind::Acceptance],
                }],
            },
            vec![F4ScenarioTypeCheckInput {
                scenario_ref: id("scenario:golden"),
                specification_hash: spec,
                status: F4ScenarioTypeStatus::Valid,
            }],
            vec![],
            ids(&["assumption:x"]),
        );
        assert_eq!(
            inputs.content_hash().unwrap().to_string(),
            GOLDEN_INPUT_HASH
        );
    }

    #[test]
    fn production_sources_are_pure() {
        let evaluator = include_str!("../src/evaluator.rs");
        let f4_inputs = &evaluator[evaluator.find("F4 supplemental inputs").unwrap()..];
        for source in [SRC, f4_inputs] {
            for token in [
                "plumb_functional",
                "plumb_sim",
                "InferenceRequest",
                "InferenceArtifact",
                "InferenceProvider",
                "MockProvider",
                "std::fs",
                "File::open",
                "reqwest",
                "std::net",
                "rusqlite",
                "ArtifactStore",
                "RevisionStore",
                "Clock",
                "Utc::now",
                "Instant::now",
                "SystemTime::now",
                "OffsetDateTime::now",
                "SemanticPatch",
                "Proposal",
                "apply_patch",
                "branch_head",
                "compare_run",
                "evaluate_calculation",
                "fixture-contract",
                "expert-answers",
                "fixtures/",
                "half_day_fraction",
                "holiday_consumes_working_day",
                "unsafe",
                "rand::",
                "f32",
                "f64",
                "ScenarioSemantics",
                "validate_scenario",
            ] {
                assert!(!source.contains(token), "production contains {token}");
            }
        }
        let rules_mod = include_str!("../src/rules/mod.rs");
        assert!(rules_mod.contains("mod f4;") && !rules_mod.contains("pub mod f4;"));
        assert!(rules_mod.contains("pub use f4::register_f4_evaluators;"));
    }
}
