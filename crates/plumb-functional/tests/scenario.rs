//! S4.1 contract tests for deterministic Scenario derivation (Hotfix 050).
//!
//! Every test lives in `scenario_contract` (`cargo test -p plumb-functional --test scenario`).
//! The HR fixture files are read only as test oracles and to build explicit boundary inputs;
//! production code never reads them. Golden values were computed independently in Python
//! (hashlib + RFC 8785 canonical JSON).

mod scenario_contract {
    use std::collections::{BTreeMap, BTreeSet};
    use std::str::FromStr;

    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_functional::decision_table::{
        DecisionColumn, DecisionInputCell, DecisionOutputCell, DecisionRow, DecisionTableSpec,
        DecisionType, NumericBound, NumericDomain, NumericInterval,
    };
    use plumb_functional::{
        analyze_scenarios, build_scenario_request, derive_scenarios, project_functional_v2,
        validate_scenario, CalculationExampleV1, ScenarioAudit, ScenarioBoundaryInput,
        ScenarioBoundaryKind, ScenarioDecisionTableInput, ScenarioDependency,
        ScenarioDerivationInputs, ScenarioError, ScenarioFamily, ScenarioGiven, ScenarioInference,
        ScenarioLiteral, ScenarioNumericBound, ScenarioNumericConstraint, ScenarioRequest,
        ScenarioSemantics, ScenarioSkeleton, ScenarioThen, ScenarioValue, ScenarioValueContract,
        ScenarioWhen, SkeletonValue, CALCULATION_ORIGIN_EXTENSION, HOLIDAY_DATE_KEY,
        SCENARIO_CONTEXT_VERSION, SCENARIO_OUTPUT_VERSION, SCENARIO_TASK_KIND,
        WORKING_DAYS_INCLUSIVE_KEY,
    };
    use plumb_inference::{InferenceArtifact, ProviderPolicy};
    use plumb_patch::{AcceptancePolicy, ProposalMateriality, SemanticPatch};
    use plumb_psg::{
        Attribute, AuditMeta, Calculation, Calendar, DecisionTable, DerivationRef, Edge,
        ElementStatus, Event, EvidenceFragment, EvidenceLocator, ExtensionKey, Graph, Modality,
        Node, NodePayload, Operation, OperationKind, RelationKind, RelationProperties, Requirement,
        RequirementKind, RequirementLevel, Scenario, ScenarioKind, SourceArtifact, State,
        Transition,
    };
    use plumb_validation::load_builtin_software_profile;
    use serde_json::{json, Value};

    use ElementStatus::{Accepted, Proposed};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s4-scenario.md");
    const SCHEMA: &str = include_str!("../../../schemas/inference/s4-scenario.schema.json");
    const SCENARIO_SRC: &str = include_str!("../src/scenario.rs");
    const FIXTURE_CONTRACT: &str = include_str!("../../../fixtures/hr-leave/fixture-contract.yaml");
    const EXPERT_ANSWERS: &str = include_str!("../../../fixtures/hr-leave/expert-answers.yaml");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:794abed9f2b7cc7b62038612b539bffbe2afd9f74a68be459860f3432568bf01";
    const SCHEMA_HASH: &str =
        "sha256:95cd2e1cbb0330d28fcdcab4f9a4c55d4fbcdb1eb595d2f326783cded0c2653a";
    const GOLDEN_TRANSITION_ID: &str = "scenario:a7cc25f1fbc10cfb";
    const GOLDEN_SKELETON_HASH: &str =
        "sha256:144d88650f402546e8e2b533d23b38223113b3f17b7d3429fbcbc40a81d27f99";
    const GOLDEN_CONTEXT_HASH: &str =
        "sha256:d54155783c1e9f731952b1e18fdf22315a00756d08e7413561d063b46ed3a523";
    const GOLDEN_REQUEST_ID: &str =
        "sha256:bbeb93f2a3309b8265270520efddbfb68c1b0cfd1efe421a99d5b26fdf6e6cf5";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const CAL: &str = "cal:PL-Office";
    const CALC: &str = "calculation:requested-working-days";
    const START: &str = "attr:start-date";
    const END: &str = "attr:end-date";
    const INCLUSIVE: &str = "attr:inclusive-end";
    const FRACTION: &str = "attr:day-fraction";
    const HR_EXPRESSION: &str =
        "working_days([start_date, end_date], calendar, inclusive_end) * day_fraction";

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
        status: ElementStatus,
        kind: RelationKind,
        from: &str,
        to: &str,
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

    fn graph(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap()
    }

    fn requirement(node_id: &str, statement: &str) -> Node {
        let mut n = node(
            node_id,
            Accepted,
            NodePayload::Requirement(Requirement {
                statement: statement.into(),
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
        );
        n.evidence = vec![fragment_id(node_id).into()];
        n
    }

    fn source_ref() -> Id {
        plumb_psg::source_artifact_id(&Hash::content_sha256(b"hr requirements")).unwrap()
    }

    fn locator(requirement_ref: &str) -> EvidenceLocator {
        let n: u64 = requirement_ref.rsplit('-').next().unwrap().parse().unwrap();
        EvidenceLocator::TextRange {
            start: n * 100,
            end: n * 100 + 50,
        }
    }

    fn fragment_id(requirement_ref: &str) -> Id {
        plumb_psg::evidence_fragment_id(&source_ref(), &locator(requirement_ref)).unwrap()
    }

    /// The SourceArtifact and one EvidenceFragment per requirement.
    fn evidence_nodes(requirement_refs: &[&str]) -> Vec<Node> {
        let mut nodes = vec![node(
            source_ref().as_str(),
            Accepted,
            NodePayload::SourceArtifact(SourceArtifact {
                source_kind: "document".into(),
                display_name: "requirements.md".into(),
                content_hash: Hash::content_sha256(b"hr requirements"),
                media_type: "text/markdown".into(),
                external_uri: None,
                external_version: None,
                producer: None,
                created_at_source: None,
                language: None,
                classification: None,
            }),
        )];
        for r in requirement_refs {
            nodes.push(node(
                fragment_id(r).as_str(),
                Accepted,
                NodePayload::EvidenceFragment(EvidenceFragment {
                    source_ref: source_ref(),
                    locator: locator(r),
                    content_hash: Hash::content_sha256(r.as_bytes()),
                    extracted_text: None,
                    speaker: None,
                    source_timestamp: None,
                }),
            ));
        }
        nodes
    }

    fn hr_edges() -> Vec<Edge> {
        [START, END, INCLUSIVE, FRACTION]
            .iter()
            .map(|a| {
                let name = format!("edge:has-{}", &a["attr:".len()..]);
                edge(
                    &name,
                    Accepted,
                    RelationKind::HasAttribute,
                    "entity:leave",
                    a,
                )
            })
            .collect()
    }

    fn attribute(node_id: &str, name: &str, value_type: &str, members: Option<&[&str]>) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Attribute(Attribute {
                name: name.into(),
                value_type: value_type.into(),
                nullable: false,
                unit: None,
                precision: None,
                enum_values: members.map(|m| m.iter().map(|s| s.to_string()).collect()),
                data_classification: None,
            }),
        )
    }

    fn root(node_ref: &str, symbol: &str) -> Value {
        json!({"node_ref": node_ref, "symbol": symbol, "exposure": {"kind": "root"}})
    }

    /// An Accepted HR Calculation with a valid S2.6 origin.
    fn calculation(
        expression: &str,
        symbols: &[(&str, &str)],
        examples: Option<Vec<Value>>,
    ) -> Node {
        let mut n = node(
            CALC,
            Accepted,
            NodePayload::Calculation(Calculation {
                name: "Requested Working Days".into(),
                expression: expression.into(),
                result_type: "Decimal(2)".into(),
                unit: Some("working_day".into()),
                rounding: None,
                calendar_ref: Some(id(CAL)),
                examples,
            }),
        );
        let mut bindings: Vec<Value> = symbols.iter().map(|(r, s)| root(r, s)).collect();
        bindings.sort_by_key(|b| b["symbol"].as_str().unwrap().to_owned());
        let range = json!({"requirement_ref": "requirement:hr-004", "start": 0, "end": 9});
        n.extensions.insert(
            ExtensionKey::from_str(CALCULATION_ORIGIN_EXTENSION).unwrap(),
            json!({"name_range": range, "expression_evidence": [range], "used_bindings": bindings}),
        );
        n
    }

    fn calendar() -> Node {
        node(
            CAL,
            Accepted,
            NodePayload::Calendar(Calendar {
                time_zone: "Europe/Warsaw".into(),
                week_pattern: json!(["Mon", "Tue", "Wed", "Thu", "Fri"]),
                region: Some("PL".into()),
                holiday_source: Some("PL public holidays".into()),
            }),
        )
    }

    fn hr_symbols() -> Vec<(&'static str, &'static str)> {
        vec![
            (START, "start_date"),
            (END, "end_date"),
            (INCLUSIVE, "inclusive_end"),
            (FRACTION, "day_fraction"),
        ]
    }

    fn hr_nodes(expression: &str, examples: Option<Vec<Value>>) -> Vec<Node> {
        let symbols: Vec<(&str, &str)> = hr_symbols()
            .into_iter()
            .filter(|(_, s)| expression.contains(s))
            .collect();
        let mut nodes = evidence_nodes(&[
            "requirement:hr-004",
            "requirement:hr-006",
            "requirement:hr-007",
        ]);
        nodes.push(node(
            "entity:leave",
            Accepted,
            NodePayload::Entity(plumb_psg::Entity {
                name: "Leave".into(),
                description: None,
                aggregate_root: None,
            }),
        ));
        nodes.extend([
            requirement("requirement:hr-004", "The system shall calculate the requested leave duration in working days from the start date to the end date."),
            requirement("requirement:hr-006", "The system shall exclude public holidays from requested leave duration."),
            requirement("requirement:hr-007", "The system shall support a half-day leave request."),
            attribute(START, "start_date", "Date", None),
            attribute(END, "end_date", "Date", None),
            attribute(INCLUSIVE, "inclusive_end", "Bool", None),
            attribute(FRACTION, "day_fraction", "Decimal(2)", None),
            calendar(),
            calculation(expression, &symbols, examples),
        ]);
        nodes
    }

    fn hr_graph(expression: &str, examples: Option<Vec<Value>>) -> Graph {
        graph(hr_nodes(expression, examples), hr_edges())
    }

    fn decimal_contract(scale: u32) -> ScenarioValueContract {
        ScenarioValueContract::Decimal {
            scale,
            interval: None,
            unit: None,
        }
    }

    /// Test-only: the explicit HR boundary inputs built from the fixture contract's ambiguity
    /// keys and requirement lists (the fixture answers are never used here).
    fn hr_boundaries() -> Vec<ScenarioBoundaryInput> {
        let fixture: serde_yaml::Value = serde_yaml::from_str(FIXTURE_CONTRACT).unwrap();
        let ambiguities = &fixture["ambiguities"];
        let refs = |entry: &serde_yaml::Value| -> Vec<Id> {
            let mut out: Vec<Id> = match (&entry["requirement"], &entry["requirements"]) {
                (serde_yaml::Value::String(r), _) => vec![r.clone()],
                (_, serde_yaml::Value::Sequence(rs)) => {
                    rs.iter().map(|r| r.as_str().unwrap().to_owned()).collect()
                }
                _ => panic!("ambiguity without requirements"),
            }
            .into_iter()
            .map(|r| id(&format!("requirement:{}", r.to_lowercase())))
            .collect();
            out.sort();
            out
        };
        vec![
            ScenarioBoundaryInput {
                boundary_key: "half_day".into(),
                source_refs: refs(&ambiguities["half_day"]),
                subject_ref: id(CALC),
                kind: ScenarioBoundaryKind::CalculationSemantic {
                    calculation_ref: id(CALC),
                    semantic_key: "half_day_fraction".into(),
                    input_symbol: "day_fraction".into(),
                    value_contract: decimal_contract(2),
                },
            },
            ScenarioBoundaryInput {
                boundary_key: "holiday_inside_period".into(),
                source_refs: refs(&ambiguities["holiday_inside_period"]),
                subject_ref: id(CALC),
                kind: ScenarioBoundaryKind::CalendarHoliday {
                    calculation_ref: id(CALC),
                    calendar_ref: id(CAL),
                },
            },
            ScenarioBoundaryInput {
                boundary_key: "inclusive_end".into(),
                source_refs: refs(&ambiguities["inclusive_end"]),
                subject_ref: id(CALC),
                kind: ScenarioBoundaryKind::InclusiveEnd {
                    calculation_ref: id(CALC),
                },
            },
        ]
    }

    fn inputs(boundaries: Vec<ScenarioBoundaryInput>) -> ScenarioDerivationInputs {
        ScenarioDerivationInputs::new(Vec::new(), boundaries)
    }

    fn half_day_example(value: &str) -> Value {
        json!({
            "version": 1,
            "key": "half_day_fraction",
            "inputs": [{"symbol": "day_fraction", "value": {"kind": "decimal", "value": value, "unit": null}}],
            "expected_result": null
        })
    }

    fn skeleton_for<'a>(
        skeletons: &'a [ScenarioSkeleton],
        name_prefix: &str,
    ) -> &'a ScenarioSkeleton {
        let found: Vec<_> = skeletons
            .iter()
            .filter(|s| s.name.starts_with(name_prefix))
            .collect();
        assert_eq!(found.len(), 1, "{name_prefix}");
        found[0]
    }

    fn dec(s: &str) -> rust_decimal::Decimal {
        rust_decimal::Decimal::from_str(s).unwrap()
    }

    fn lit_dec(value: &str, unit: Option<&str>) -> ScenarioLiteral {
        ScenarioLiteral::Decimal {
            value: value.into(),
            unit: unit.map(Into::into),
        }
    }

    fn sem(owner: &str, key: &str) -> ScenarioDependency {
        ScenarioDependency {
            owner_ref: id(owner),
            key: key.into(),
        }
    }

    fn scenario_audit() -> ScenarioAudit {
        ScenarioAudit {
            created_by: id("actor:human"),
            created_at: ts(AT),
        }
    }

    fn provider(name: &str) -> ProviderPolicy {
        ProviderPolicy {
            provider: name.to_owned(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn derivation() -> DerivationRef {
        DerivationRef::from(id("derivation:s4-scenario"))
    }

    fn artifact_for(request: &ScenarioRequest, output: Value) -> InferenceArtifact {
        let raw = to_canonical_json(&output).unwrap();
        let validated_output = CanonicalJson::new(output);
        InferenceArtifact {
            request_hash: request.request.id.clone(),
            provider: request.request.provider_policy.provider.clone(),
            model: "mock-model".to_owned(),
            parameters: CanonicalJson::new(json!({})),
            raw_response_hash: Hash::content_sha256(&raw),
            validated_output_hash: validated_output.content_hash().unwrap(),
            validated_output,
        }
    }

    // ---------------------------------------------------------------- decision tables

    fn column(name: &str, ty: DecisionType, domain: Option<(&str, &str)>) -> DecisionColumn {
        DecisionColumn {
            name: name.into(),
            ty,
            domain: domain.map(|(l, u)| NumericDomain {
                lower: dec(l),
                upper: dec(u),
            }),
        }
    }

    fn bound(value: &str, inclusive: bool) -> NumericBound {
        NumericBound {
            value: dec(value),
            inclusive,
        }
    }

    fn interval(lower: Option<(&str, bool)>, upper: Option<(&str, bool)>) -> DecisionInputCell {
        DecisionInputCell::Interval {
            interval: NumericInterval {
                lower: lower.map(|(v, i)| bound(v, i)),
                upper: upper.map(|(v, i)| bound(v, i)),
            },
        }
    }

    const TABLE: &str = "decision-table:eligibility";

    fn eligibility_spec() -> DecisionTableSpec {
        DecisionTableSpec {
            hit_policy: "FIRST".into(),
            inputs: vec![
                column("contractor", DecisionType::Bool, None),
                column(
                    "leave_type",
                    DecisionType::Enum {
                        members: vec!["Annual".into(), "Sick".into(), "Unpaid".into()],
                    },
                    None,
                ),
                column("days", DecisionType::Int, Some(("0", "30"))),
                column("fraction", DecisionType::Decimal { scale: 2 }, None),
            ],
            outputs: vec![
                column("eligible", DecisionType::Bool, None),
                column("deduct", DecisionType::Decimal { scale: 2 }, None),
            ],
            rows: vec![
                DecisionRow {
                    inputs: vec![
                        DecisionInputCell::Bool { value: true },
                        DecisionInputCell::EnumSet {
                            members: vec!["Annual".into()],
                        },
                        interval(Some(("5", false)), Some(("10", true))),
                        interval(Some(("0.5", true)), Some(("0.5", true))),
                    ],
                    outputs: vec![
                        DecisionOutputCell::Bool { value: false },
                        DecisionOutputCell::Decimal { value: dec("0.00") },
                    ],
                },
                DecisionRow {
                    inputs: vec![
                        DecisionInputCell::Bool { value: false },
                        DecisionInputCell::EnumSet {
                            members: vec!["Sick".into(), "Unpaid".into()],
                        },
                        DecisionInputCell::Any,
                        DecisionInputCell::Any,
                    ],
                    outputs: vec![
                        DecisionOutputCell::Bool { value: true },
                        DecisionOutputCell::Decimal { value: dec("1.50") },
                    ],
                },
            ],
            default_output: None,
        }
    }

    fn table_node(node_id: &str, status: ElementStatus, hit_policy: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::DecisionTable(DecisionTable {
                name: "Eligibility".into(),
                hit_policy: hit_policy.into(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                rows: Vec::new(),
            }),
        )
    }

    fn table_inputs(spec: DecisionTableSpec) -> ScenarioDerivationInputs {
        ScenarioDerivationInputs::new(
            vec![ScenarioDecisionTableInput {
                decision_table_ref: id(TABLE),
                spec,
            }],
            Vec::new(),
        )
    }

    // ---------------------------------------------------------------- lifecycle

    fn state(node_id: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            status,
            NodePayload::State(State {
                name: node_id.into(),
            }),
        )
    }

    fn transition(node_id: &str, from: &str, to: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Transition(Transition {
                stateful_ref: id("entity:leave-request"),
                from_state: id(from),
                to_state: id(to),
                guard_expr: None,
                effect_refs: None,
            }),
        )
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

    fn event(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Event(Event {
                name: node_id.into(),
                payload_schema_ref: None,
                semantic_type: None,
            }),
        )
    }

    fn entity(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Entity(plumb_psg::Entity {
                name: "LeaveRequest".into(),
                description: None,
                aggregate_root: None,
            }),
        )
    }

    fn lifecycle_nodes() -> Vec<Node> {
        vec![
            entity("entity:leave-request"),
            state("state:draft", Accepted),
            state("state:submitted", Accepted),
            state("state:approved", Accepted),
            transition("transition:submit", "state:draft", "state:submitted"),
            transition("transition:approve", "state:submitted", "state:approved"),
            operation("operation:submit"),
            event("event:leave-approved"),
        ]
    }

    fn lifecycle_edges() -> Vec<Edge> {
        vec![
            edge(
                "edge:submit-via",
                Accepted,
                RelationKind::TransitionsVia,
                "transition:submit",
                "operation:submit",
            ),
            edge(
                "edge:approve-via",
                Accepted,
                RelationKind::TransitionsVia,
                "transition:approve",
                "event:leave-approved",
            ),
        ]
    }

    fn lifecycle_graph() -> Graph {
        graph(lifecycle_nodes(), lifecycle_edges())
    }

    fn empty_inputs() -> ScenarioDerivationInputs {
        ScenarioDerivationInputs::new(Vec::new(), Vec::new())
    }

    // ================================================================= wire shapes

    #[test]
    fn literal_and_value_wire_shapes_round_trip() {
        let cases = [
            json!({"kind": "bool", "value": true}),
            json!({"kind": "int", "value": 5}),
            json!({"kind": "decimal", "value": "0.5", "unit": null}),
            json!({"kind": "decimal", "value": "0.5", "unit": "working_day"}),
            json!({"kind": "string", "value": "ABC"}),
            json!({"kind": "date", "value": "2027-01-06"}),
            json!({"kind": "datetime", "value": "2027-01-06T08:00:00.000000000Z"}),
            json!({"kind": "enum", "value": "Annual"}),
            json!({"kind": "semantic_ref", "owner_ref": "calculation:x", "key": "half_day_fraction"}),
        ];
        for case in cases {
            let value: ScenarioValue = serde_json::from_value(case.clone()).unwrap();
            assert_eq!(serde_json::to_value(&value).unwrap(), case);
        }
        let semantic: ScenarioValue = serde_json::from_value(
            json!({"kind": "semantic_ref", "owner_ref": "calculation:x", "key": "k"}),
        )
        .unwrap();
        assert_eq!(
            semantic,
            ScenarioValue::SemanticRef(sem("calculation:x", "k"))
        );
        for bad in [
            json!({"kind": "float", "value": 0.5}),
            json!({"kind": "bool", "value": true, "extra": 1}),
            json!({"kind": "int", "value": 5.5}),
            json!({"kind": "int", "value": "5"}),
            json!({"kind": "decimal", "value": 0.5, "unit": null}),
            json!({"kind": "bool", "value": "true"}),
            json!({"kind": "semantic_ref", "owner_ref": "not an id", "key": "k"}),
            json!({"kind": "semantic_ref", "owner_ref": "calculation:x", "key": "k", "value": 1}),
            json!({"kind": "semantic_ref", "owner_ref": "calculation:x"}),
            json!({"value": true}),
        ] {
            assert!(
                serde_json::from_value::<ScenarioValue>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn literal_shapes_are_canonical() {
        for good in ["0.5", "12.00", "-3", "0", "1.50"] {
            assert!(lit_dec(good, None).validate().is_ok(), "{good}");
        }
        for bad in ["00.5", "+0.5", "5e-1", ".5", "0.5 ", "1,5", "NaN"] {
            assert!(lit_dec(bad, None).validate().is_err(), "{bad}");
        }
        assert!(lit_dec("1", Some("Working Day")).validate().is_err());
        for good in ["2027-01-06", "2024-02-29"] {
            assert!(ScenarioLiteral::Date { value: good.into() }
                .validate()
                .is_ok());
        }
        for bad in ["2027-02-30", "2027-1-6", "06.01.2027", "2027-01-06T00:00"] {
            assert!(
                ScenarioLiteral::Date { value: bad.into() }
                    .validate()
                    .is_err(),
                "{bad}"
            );
        }
        assert!(serde_json::from_value::<ScenarioLiteral>(
            json!({"kind": "datetime", "value": "2027-01-06 08:00"})
        )
        .is_err());
        assert!(ScenarioLiteral::String {
            value: " E-100".into()
        }
        .validate()
        .is_err());
        assert!(ScenarioLiteral::Enum {
            value: String::new()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn when_given_then_wire_is_closed_and_grouped() {
        for (when, expected) in [
            (
                json!({"operation": "operation:a"}),
                ScenarioWhen::Operation(id("operation:a")),
            ),
            (
                json!({"event": "event:a"}),
                ScenarioWhen::Event(id("event:a")),
            ),
            (
                json!({"transition": "transition:a"}),
                ScenarioWhen::Transition(id("transition:a")),
            ),
            (json!({"rule": "rule:a"}), ScenarioWhen::Rule(id("rule:a"))),
            (
                json!({"calculation": "calculation:a"}),
                ScenarioWhen::Calculation(id("calculation:a")),
            ),
            (
                json!({"decision_table": "decision-table:a"}),
                ScenarioWhen::DecisionTable(id("decision-table:a")),
            ),
        ] {
            let decoded = ScenarioWhen::from_value(&when).unwrap();
            assert_eq!(decoded, expected);
            assert_eq!(decoded.to_value(), when);
        }
        for bad in [
            json!({"kind": "operation", "operation_ref": "operation:a"}),
            json!({"operation": "operation:a", "event": "event:b"}),
            json!({"command": "operation:a"}),
            json!({"operation": "bad id"}),
            json!({}),
            json!("operation:a"),
        ] {
            assert!(ScenarioWhen::from_value(&bad).is_err(), "{bad}");
        }
        let given = vec![
            json!({"attribute_values": {"attr:a": {"kind": "int", "value": 1}}}),
            json!({"entities_exist": ["entity:a", "entity:b"]}),
            json!({"states_active": ["state:a"]}),
            json!({"decision_inputs": {"0": {"kind": "bool", "value": true}, "10": {"kind": "bool", "value": false}}}),
            json!({"calculation_semantics": {"calculation:a": {"inclusive_end": {"kind": "semantic_ref", "owner_ref": "calculation:a", "key": "inclusive_end"}}}}),
        ];
        let decoded = ScenarioGiven::from_values(&given).unwrap();
        assert_eq!(
            decoded.to_values(|v| serde_json::to_value(v).unwrap()),
            given
        );
        let then = vec![
            json!({"attribute_values": {"attr:a": {"kind": "int", "value": 1}}}),
            json!({"states_active": ["state:b"]}),
            json!({"outcomes": ["outcome:ok"]}),
            json!({"events": ["event:done"]}),
            json!({"rules_satisfied": ["rule:r"]}),
            json!({"decision_outputs": {"0": {"kind": "bool", "value": true}}}),
            json!({"calculation_results": {"calculation:a": {"kind": "decimal", "value": "1", "unit": "working_day"}}}),
        ];
        let decoded = ScenarioThen::from_values(&then).unwrap();
        assert_eq!(
            decoded.to_values(|v| serde_json::to_value(v).unwrap()),
            then
        );
        let bad_given = [
            vec![
                json!({"states_active": ["state:a"]}),
                json!({"attribute_values": {"attr:a": {"kind": "int", "value": 1}}}),
            ],
            vec![
                json!({"states_active": ["state:a"]}),
                json!({"states_active": ["state:b"]}),
            ],
            vec![json!({"states_active": ["state:b", "state:a"]})],
            vec![json!({"states_active": ["state:a", "state:a"]})],
            vec![json!({"states_active": []})],
            vec![json!({"attribute_values": {}})],
            vec![json!({"text": "given something"})],
            vec![json!({"states_active": ["state:a"], "entities_exist": ["entity:a"]})],
            vec![json!({"decision_inputs": {"01": {"kind": "bool", "value": true}}})],
            vec![
                json!({"attribute_values": {"attr:a": {"kind": "decimal", "value": "00.5", "unit": null}}}),
            ],
            vec![json!("states_active")],
        ];
        for bad in bad_given {
            assert!(ScenarioGiven::from_values(&bad).is_err(), "{bad:?}");
        }
        for bad in [
            vec![json!({"expected": {"x": 1}})],
            vec![
                json!({"decision_outputs": {"0": {"kind": "semantic_ref", "owner_ref": "calculation:a", "key": "k"}}}),
            ],
            vec![
                json!({"events": ["event:a"]}),
                json!({"outcomes": ["outcome:a"]}),
            ],
        ] {
            assert!(ScenarioThen::from_values(&bad).is_err(), "{bad:?}");
        }
    }

    // ================================================================= contracts

    fn constraint(
        lower: Option<(&str, bool)>,
        upper: Option<(&str, bool)>,
    ) -> ScenarioNumericConstraint {
        let b = |(v, i): (&str, bool)| ScenarioNumericBound {
            value: v.into(),
            inclusive: i,
        };
        ScenarioNumericConstraint {
            lower: lower.map(b),
            upper: upper.map(b),
        }
    }

    #[test]
    fn value_contracts_are_exact() {
        let int = |v: i64| ScenarioLiteral::Int { value: v };
        type Bound<'a> = Option<(&'a str, bool)>;
        let cases: [(Bound, Bound, &[i64], &[i64]); 6] = [
            (Some(("5", false)), Some(("10", false)), &[6, 9], &[5, 10]),
            (Some(("5", true)), Some(("10", false)), &[5, 9], &[4, 10]),
            (Some(("5", false)), Some(("10", true)), &[6, 10], &[5, 11]),
            (Some(("5", true)), Some(("10", true)), &[5, 10], &[4, 11]),
            (None, Some(("10", false)), &[-100, 9], &[10]),
            (Some(("5", false)), None, &[6, 1000], &[5]),
        ];
        for (lower, upper, accept, reject) in cases {
            let contract = ScenarioValueContract::Integer {
                interval: Some(constraint(lower, upper)),
            };
            contract.validate().unwrap();
            for v in accept {
                assert!(contract.check(&int(*v)).is_ok(), "{v} in {contract:?}");
            }
            for v in reject {
                assert!(contract.check(&int(*v)).is_err(), "{v} not in {contract:?}");
            }
        }
        // Decimal: scale, exclusive bound, unit exactness and no float.
        let contract = ScenarioValueContract::Decimal {
            scale: 2,
            interval: Some(constraint(Some(("0", false)), Some(("1", true)))),
            unit: Some("working_day".into()),
        };
        assert!(contract.check(&lit_dec("0.5", Some("working_day"))).is_ok());
        assert!(contract
            .check(&lit_dec("1.00", Some("working_day")))
            .is_ok());
        assert!(contract.check(&lit_dec("0", Some("working_day"))).is_err());
        assert!(contract
            .check(&lit_dec("0.125", Some("working_day")))
            .is_err());
        assert!(contract.check(&lit_dec("0.5", Some("day"))).is_err());
        assert!(contract.check(&lit_dec("0.5", None)).is_err());
        assert!(contract.check(&ScenarioLiteral::Int { value: 1 }).is_err());
        // Enum: exact, case-sensitive.
        let contract = ScenarioValueContract::Enum {
            values: vec!["Annual".into(), "Sick".into()],
        };
        assert!(contract
            .check(&ScenarioLiteral::Enum {
                value: "Annual".into()
            })
            .is_ok());
        assert!(contract
            .check(&ScenarioLiteral::Enum {
                value: "annual".into()
            })
            .is_err());
        assert!(contract
            .check(&ScenarioLiteral::Enum {
                value: "Unpaid".into()
            })
            .is_err());
        assert!(contract
            .check(&ScenarioLiteral::String {
                value: "Annual".into()
            })
            .is_err());
        // Bool, Date, DateTime, String.
        assert!(ScenarioValueContract::Bool
            .check(&ScenarioLiteral::Bool { value: false })
            .is_ok());
        assert!(ScenarioValueContract::Bool
            .check(&ScenarioLiteral::Int { value: 0 })
            .is_err());
        assert!(ScenarioValueContract::Date
            .check(&ScenarioLiteral::Date {
                value: "2027-01-06".into()
            })
            .is_ok());
        assert!(ScenarioValueContract::Date
            .check(&ScenarioLiteral::Date {
                value: "2027-13-01".into()
            })
            .is_err());
        assert!(ScenarioValueContract::DateTime
            .check(&ScenarioLiteral::DateTime { value: ts(AT) })
            .is_ok());
        let strings = ScenarioValueContract::String {
            values: Some(vec!["E-100".into(), "E-200".into()]),
        };
        assert!(strings
            .check(&ScenarioLiteral::String {
                value: "E-200".into()
            })
            .is_ok());
        assert!(strings
            .check(&ScenarioLiteral::String {
                value: "E-300".into()
            })
            .is_err());
        // Contract shape.
        for bad in [
            ScenarioValueContract::Enum { values: vec![] },
            ScenarioValueContract::Enum {
                values: vec!["b".into(), "a".into()],
            },
            ScenarioValueContract::Integer {
                interval: Some(constraint(Some(("10", true)), Some(("5", true)))),
            },
            ScenarioValueContract::Integer {
                interval: Some(constraint(Some(("5", false)), Some(("5", true)))),
            },
            ScenarioValueContract::Integer {
                interval: Some(constraint(Some(("5.0e1", true)), None)),
            },
            ScenarioValueContract::Decimal {
                scale: 29,
                interval: None,
                unit: None,
            },
            ScenarioValueContract::Decimal {
                scale: 2,
                interval: None,
                unit: Some("Day".into()),
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    // ================================================================= inputs

    #[test]
    fn derivation_inputs_canonicalize() {
        let mut boundaries = hr_boundaries();
        let forward = ScenarioDerivationInputs::new(Vec::new(), boundaries.clone());
        boundaries.reverse();
        let reversed = ScenarioDerivationInputs::new(Vec::new(), boundaries);
        assert_eq!(forward, reversed);
        assert_eq!(
            forward.content_hash().unwrap(),
            reversed.content_hash().unwrap()
        );
        let keys: Vec<&str> = forward
            .boundaries
            .iter()
            .map(|b| b.boundary_key.as_str())
            .collect();
        assert_eq!(keys, ["half_day", "holiday_inside_period", "inclusive_end"]);
        // Duplicate (boundary_key, subject_ref) is rejected.
        let mut dup = hr_boundaries();
        dup.push(hr_boundaries()[0].clone());
        let g = hr_graph(HR_EXPRESSION, None);
        assert!(matches!(
            analyze_scenarios(&g, &ScenarioDerivationInputs::new(Vec::new(), dup)),
            Err(ScenarioError::InvalidInput { .. })
        ));
        // Duplicate decision-table input.
        let t = || ScenarioDecisionTableInput {
            decision_table_ref: id(TABLE),
            spec: eligibility_spec(),
        };
        let g = graph(vec![table_node(TABLE, Accepted, "FIRST")], Vec::new());
        assert!(matches!(
            analyze_scenarios(
                &g,
                &ScenarioDerivationInputs::new(vec![t(), t()], Vec::new())
            ),
            Err(ScenarioError::InvalidInput { .. })
        ));
    }

    #[test]
    fn decision_table_inputs_are_validated() {
        let ok = graph(vec![table_node(TABLE, Accepted, "FIRST")], Vec::new());
        assert!(analyze_scenarios(&ok, &table_inputs(eligibility_spec())).is_ok());
        // Every Accepted DecisionTable needs exactly one spec.
        assert!(matches!(
            analyze_scenarios(&ok, &empty_inputs()),
            Err(ScenarioError::MissingDecisionTableInput { decision_table_ref }) if decision_table_ref == id(TABLE)
        ));
        // Missing, wrong type, non-Accepted, hit-policy mismatch.
        let missing = graph(Vec::new(), Vec::new());
        let wrong_type = graph(
            vec![node(
                TABLE,
                Accepted,
                NodePayload::State(State { name: "x".into() }),
            )],
            Vec::new(),
        );
        let proposed = graph(vec![table_node(TABLE, Proposed, "FIRST")], Vec::new());
        let unique = graph(vec![table_node(TABLE, Accepted, "UNIQUE")], Vec::new());
        for g in [missing, wrong_type, proposed, unique] {
            assert!(matches!(
                analyze_scenarios(&g, &table_inputs(eligibility_spec())),
                Err(ScenarioError::InvalidDecisionTableInput { .. })
            ));
        }
        // Structurally invalid spec.
        let mut spec = eligibility_spec();
        spec.rows[0].inputs.pop();
        assert!(matches!(
            analyze_scenarios(&ok, &table_inputs(spec)),
            Err(ScenarioError::InvalidDecisionTableSpec { .. })
        ));
        let mut spec = eligibility_spec();
        spec.rows[0].inputs[2] = interval(Some(("10", true)), Some(("5", true)));
        assert!(matches!(
            analyze_scenarios(&ok, &table_inputs(spec)),
            Err(ScenarioError::InvalidDecisionTableSpec { .. })
        ));
    }

    // ================================================================= rule rows

    #[test]
    fn rule_rows_derive_literals_slots_and_outputs() {
        let g = graph(vec![table_node(TABLE, Accepted, "FIRST")], Vec::new());
        let skeletons = analyze_scenarios(&g, &table_inputs(eligibility_spec())).unwrap();
        assert_eq!(skeletons.len(), 2);
        let row0 = skeleton_for(&skeletons, &format!("Rule row {TABLE} row 0"));
        let row1 = skeleton_for(&skeletons, &format!("Rule row {TABLE} row 1"));
        for s in [row0, row1] {
            assert_eq!(s.family, ScenarioFamily::RuleRow);
            assert_eq!(s.scenario_kind(), ScenarioKind::RuleRow);
            assert_eq!(s.when, ScenarioWhen::DecisionTable(id(TABLE)));
            assert_eq!(s.derived_from_refs, vec![id(TABLE)]);
            assert!(s.requirement_refs.is_empty());
            assert!(s.unresolved_dependencies.is_empty());
        }
        // Row 0: exact Bool, one-member EnumSet and point interval are literals; (5, 10] is a slot.
        let inputs0 = &row0.given.decision_inputs;
        assert_eq!(
            inputs0[&0],
            SkeletonValue::Literal(ScenarioLiteral::Bool { value: true })
        );
        assert_eq!(
            inputs0[&1],
            SkeletonValue::Literal(ScenarioLiteral::Enum {
                value: "Annual".into()
            })
        );
        assert_eq!(inputs0[&3], SkeletonValue::Literal(lit_dec("0.5", None)));
        let slot_id = format!("decision-input:{TABLE}:0:2");
        assert_eq!(inputs0[&2], SkeletonValue::Slot(slot_id.clone()));
        assert_eq!(row0.value_slots.len(), 1);
        assert_eq!(row0.value_slots[0].slot_id, slot_id);
        assert_eq!(
            row0.value_slots[0].contract,
            ScenarioValueContract::Integer {
                interval: Some(constraint(Some(("5", false)), Some(("10", true))))
            }
        );
        assert_eq!(
            row0.then
                .decision_outputs
                .values()
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                SkeletonValue::Literal(ScenarioLiteral::Bool { value: false }),
                SkeletonValue::Literal(lit_dec("0.00", None)),
            ]
        );
        // Row 1: multi-member EnumSet slot is exactly its members; Any uses the column type and
        // the numeric domain as an inclusive interval.
        let contracts: BTreeMap<String, ScenarioValueContract> = row1
            .value_slots
            .iter()
            .map(|s| (s.slot_id.clone(), s.contract.clone()))
            .collect();
        assert_eq!(
            contracts[&format!("decision-input:{TABLE}:1:1")],
            ScenarioValueContract::Enum {
                values: vec!["Sick".into(), "Unpaid".into()]
            }
        );
        assert_eq!(
            contracts[&format!("decision-input:{TABLE}:1:2")],
            ScenarioValueContract::Integer {
                interval: Some(constraint(Some(("0", true)), Some(("30", true))))
            }
        );
        assert_eq!(
            contracts[&format!("decision-input:{TABLE}:1:3")],
            decimal_contract(2)
        );
        assert_eq!(
            row1.given.decision_inputs[&0],
            SkeletonValue::Literal(ScenarioLiteral::Bool { value: false })
        );
        assert_eq!(row0.then.decision_outputs.len(), 2);
    }

    #[test]
    fn rule_row_identity_tracks_table_row_and_targets() {
        let g = graph(vec![table_node(TABLE, Accepted, "FIRST")], Vec::new());
        let ids = |g: &Graph, spec: DecisionTableSpec| -> Vec<Id> {
            analyze_scenarios(g, &table_inputs(spec))
                .unwrap()
                .into_iter()
                .map(|s| s.scenario_ref)
                .collect()
        };
        let base = ids(&g, eligibility_spec());
        assert_ne!(base[0], base[1]);
        // FIRST row order is semantic: swapping rows changes which obligation has which index.
        let mut swapped = eligibility_spec();
        swapped.rows.reverse();
        let swapped_ids: BTreeSet<Id> = ids(&g, swapped).into_iter().collect();
        assert!(swapped_ids.is_disjoint(&base.iter().cloned().collect()));
        // Another table: different IDs.
        let other = graph(
            vec![table_node("decision-table:other", Accepted, "FIRST")],
            Vec::new(),
        );
        let other_ids = analyze_scenarios(
            &other,
            &ScenarioDerivationInputs::new(
                vec![ScenarioDecisionTableInput {
                    decision_table_ref: id("decision-table:other"),
                    spec: eligibility_spec(),
                }],
                Vec::new(),
            ),
        )
        .unwrap();
        assert!(other_ids.iter().all(|s| !base.contains(&s.scenario_ref)));
        // Changing an asserted output changes the obligation.
        let mut changed = eligibility_spec();
        changed.rows[0].outputs[0] = DecisionOutputCell::Bool { value: true };
        assert!(!ids(&g, changed).contains(&base_row(&g, 0)));
    }

    fn base_row(g: &Graph, row: usize) -> Id {
        analyze_scenarios(g, &table_inputs(eligibility_spec()))
            .unwrap()
            .into_iter()
            .find(|s| s.name.ends_with(&format!("row {row}")))
            .unwrap()
            .scenario_ref
    }

    // ================================================================= transitions

    #[test]
    fn transitions_derive_operation_and_event_scenarios() {
        let skeletons = analyze_scenarios(&lifecycle_graph(), &empty_inputs()).unwrap();
        assert_eq!(skeletons.len(), 2);
        let submit = skeleton_for(&skeletons, "Transition transition:submit");
        assert_eq!(submit.when, ScenarioWhen::Operation(id("operation:submit")));
        assert_eq!(submit.given.states_active, vec![id("state:draft")]);
        assert_eq!(submit.then.states_active, vec![id("state:submitted")]);
        assert_eq!(submit.derived_from_refs, vec![id("transition:submit")]);
        assert_eq!(submit.scenario_kind(), ScenarioKind::StateTransition);
        assert!(submit.value_slots.is_empty());
        let approve = skeleton_for(&skeletons, "Transition transition:approve");
        assert_eq!(
            approve.when,
            ScenarioWhen::Event(id("event:leave-approved"))
        );
        assert_eq!(
            approve.when.to_value(),
            json!({"event": "event:leave-approved"})
        );
        assert_eq!(
            submit.when.to_value(),
            json!({"operation": "operation:submit"})
        );
    }

    #[test]
    fn transition_triggers_and_endpoints_are_required() {
        // Missing (only a Suspect baseline edge) and ambiguous triggers.
        let mut edges = lifecycle_edges();
        edges[0].status = ElementStatus::Suspect;
        let missing = Graph::new(id(PROJECT), id(PROFILE), lifecycle_nodes(), edges);
        {
            let g = missing.expect("a Suspect trigger edge is a valid baseline graph");
            assert!(matches!(
                analyze_scenarios(&g, &empty_inputs()),
                Err(ScenarioError::MissingTrigger { transition_ref }) if transition_ref == id("transition:submit")
            ));
        }
        let mut edges = lifecycle_edges();
        edges.push(edge(
            "edge:submit-via-2",
            Accepted,
            RelationKind::TransitionsVia,
            "transition:submit",
            "event:leave-approved",
        ));
        {
            // Two Accepted triggers can never form a graph: transitions_via is exactly one
            // baseline edge, so AmbiguousTrigger is defense in depth.
            assert!(Graph::new(id(PROJECT), id(PROFILE), lifecycle_nodes(), edges).is_err());
        }
        // Non-Accepted endpoint.
        let mut nodes = lifecycle_nodes();
        nodes[2] = state("state:submitted", Proposed);
        let g = graph(nodes, lifecycle_edges());
        assert!(matches!(
            analyze_scenarios(&g, &empty_inputs()),
            Err(ScenarioError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn only_the_three_frozen_kinds_are_created() {
        let mut nodes = hr_nodes(HR_EXPRESSION, None);
        nodes.extend(lifecycle_nodes());
        nodes.push(table_node(TABLE, Accepted, "FIRST"));
        let g = graph(nodes, [hr_edges(), lifecycle_edges()].concat());
        let all = ScenarioDerivationInputs::new(
            vec![ScenarioDecisionTableInput {
                decision_table_ref: id(TABLE),
                spec: eligibility_spec(),
            }],
            hr_boundaries(),
        );
        let skeletons = analyze_scenarios(&g, &all).unwrap();
        let kinds: BTreeSet<String> = skeletons
            .iter()
            .map(|s| {
                serde_json::to_value(s.scenario_kind())
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            kinds,
            BTreeSet::from([
                "boundary".into(),
                "rule_row".into(),
                "state_transition".into()
            ])
        );
        assert_eq!(skeletons.len(), 2 + 2 + 3);
        assert!(skeletons
            .windows(2)
            .all(|p| p[0].scenario_ref < p[1].scenario_ref));
    }

    // ================================================================= HR boundaries

    #[test]
    fn half_day_is_a_semantic_ref_before_the_answer_and_resolves_from_an_accepted_example() {
        let pre = hr_graph(HR_EXPRESSION, None);
        let skeletons = analyze_scenarios(&pre, &inputs(hr_boundaries())).unwrap();
        let half = skeleton_for(&skeletons, "Boundary half_day for");
        assert_eq!(half.scenario_kind(), ScenarioKind::Boundary);
        assert_eq!(half.when, ScenarioWhen::Calculation(id(CALC)));
        assert_eq!(half.requirement_refs, vec![id("requirement:hr-007")]);
        assert_eq!(
            half.derived_from_refs,
            vec![id(CALC), id("requirement:hr-007")]
        );
        assert!(
            half.value_slots.is_empty(),
            "the half-day value is never an inference slot"
        );
        assert_eq!(
            half.given.attribute_values[&id(FRACTION)],
            SkeletonValue::Semantic {
                dependency: sem(CALC, "half_day_fraction"),
                resolved: None
            }
        );
        assert_eq!(
            half.unresolved_dependencies,
            vec![sem(CALC, "half_day_fraction"), sem(CALC, "result:half_day")]
        );
        let scenario = half.materialize(&BTreeMap::new()).unwrap();
        let text = serde_json::to_string(&scenario.given).unwrap();
        assert!(
            !text.contains("0.5") && !text.contains("\"value\""),
            "{text}"
        );
        assert!(text.contains("semantic_ref"));

        // Post-answer: the accepted Calculation example carries the value.
        let post = hr_graph(HR_EXPRESSION, Some(vec![half_day_example("0.5")]));
        let after = analyze_scenarios(&post, &inputs(hr_boundaries())).unwrap();
        let resolved = skeleton_for(&after, "Boundary half_day for");
        assert_eq!(resolved.scenario_ref, half.scenario_ref);
        assert_eq!(
            resolved.given.attribute_values[&id(FRACTION)],
            SkeletonValue::Semantic {
                dependency: sem(CALC, "half_day_fraction"),
                resolved: Some(lit_dec("0.5", None))
            }
        );
        assert_eq!(
            resolved.unresolved_dependencies,
            vec![sem(CALC, "result:half_day")]
        );
        assert_ne!(
            resolved.content_hash().unwrap(),
            half.content_hash().unwrap()
        );
        let materialized = resolved.materialize(&BTreeMap::new()).unwrap();
        let given = ScenarioSemantics::from_scenario(&materialized)
            .unwrap()
            .given;
        assert_eq!(
            given.attribute_values[&id(FRACTION)],
            ScenarioValue::Literal(lit_dec("0.5", None))
        );
        // Oracle only: the fixture's expert answer is the value the accepted example carries.
        let answers: serde_yaml::Value = serde_yaml::from_str(EXPERT_ANSWERS).unwrap();
        let half_answer = answers["answers"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|a| a["ambiguity"] == "half_day")
            .unwrap();
        assert_eq!(half_answer["value"]["decimal"].as_str(), Some("0.5"));
        assert_eq!(half_answer["value"]["unit"].as_str(), Some("working_day"));
        // An example result resolves the expected result too, with the same ID.
        let mut with_result = half_day_example("0.5");
        with_result["expected_result"] =
            json!({"kind": "decimal", "value": "0.5", "unit": "working_day"});
        let g = hr_graph(HR_EXPRESSION, Some(vec![with_result]));
        let s = analyze_scenarios(&g, &inputs(hr_boundaries())).unwrap();
        let s = skeleton_for(&s, "Boundary half_day for");
        assert_eq!(s.scenario_ref, half.scenario_ref);
        assert!(s.unresolved_dependencies.is_empty());
    }

    #[test]
    fn holiday_date_stays_unresolved_even_with_an_accepted_calendar() {
        let g = hr_graph(HR_EXPRESSION, Some(vec![half_day_example("0.5")]));
        let skeletons = analyze_scenarios(&g, &inputs(hr_boundaries())).unwrap();
        let holiday = skeleton_for(&skeletons, "Boundary holiday_inside_period for");
        assert!(
            holiday.value_slots.is_empty(),
            "no inference slot for a holiday date"
        );
        for attr in [START, END] {
            assert_eq!(
                holiday.given.attribute_values[&id(attr)],
                SkeletonValue::Semantic {
                    dependency: sem(CAL, HOLIDAY_DATE_KEY),
                    resolved: None
                }
            );
        }
        assert_eq!(
            holiday.requirement_refs,
            vec![id("requirement:hr-004"), id("requirement:hr-006")]
        );
        let text =
            serde_json::to_string(&holiday.materialize(&BTreeMap::new()).unwrap().given).unwrap();
        let fixture: serde_yaml::Value = serde_yaml::from_str(FIXTURE_CONTRACT).unwrap();
        for date in fixture["calendar"]["test_holidays"].as_sequence().unwrap() {
            assert!(
                !text.contains(date.as_str().unwrap()),
                "fixture holiday imported"
            );
        }
        // The calendar must be the calculation's own Accepted calendar.
        let mut wrong = hr_boundaries();
        wrong[1].kind = ScenarioBoundaryKind::CalendarHoliday {
            calculation_ref: id(CALC),
            calendar_ref: id("cal:other"),
        };
        assert!(matches!(
            analyze_scenarios(&g, &inputs(wrong)),
            Err(ScenarioError::InvalidBoundaryInput { .. })
        ));
    }

    #[test]
    fn inclusive_end_resolves_only_from_a_literal_working_days_argument() {
        let expr = |third: &str| {
            format!("working_days([start_date, end_date], calendar, {third}) * day_fraction")
        };
        let mut ids = BTreeSet::new();
        for (third, expected) in [
            ("true", Some(ScenarioLiteral::Bool { value: true })),
            ("false", Some(ScenarioLiteral::Bool { value: false })),
            ("inclusive_end", None),
        ] {
            let g = hr_graph(&expr(third), None);
            let skeletons = analyze_scenarios(&g, &inputs(hr_boundaries())).unwrap();
            let s = skeleton_for(&skeletons, "Boundary inclusive_end for");
            assert_eq!(
                s.given.calculation_semantics[&id(CALC)][WORKING_DAYS_INCLUSIVE_KEY],
                SkeletonValue::Semantic {
                    dependency: sem(CALC, WORKING_DAYS_INCLUSIVE_KEY),
                    resolved: expected.clone()
                }
            );
            assert_eq!(
                s.unresolved_dependencies
                    .contains(&sem(CALC, WORKING_DAYS_INCLUSIVE_KEY)),
                expected.is_none()
            );
            // The period dates are realistic test-data slots; inclusivity never is.
            let slots: Vec<&str> = s.value_slots.iter().map(|v| v.slot_id.as_str()).collect();
            assert_eq!(
                slots,
                [
                    format!("given:attribute:{END}"),
                    format!("given:attribute:{START}")
                ]
            );
            assert!(s
                .value_slots
                .iter()
                .all(|v| v.contract == ScenarioValueContract::Date));
            ids.insert(s.scenario_ref.clone());
        }
        assert_eq!(
            ids.len(),
            1,
            "inclusive-end resolution keeps the obligation ID"
        );
        // No working_days call is an invalid boundary.
        let g = hr_graph("day_fraction", None);
        let only = vec![hr_boundaries().remove(2)];
        assert!(matches!(
            analyze_scenarios(&g, &inputs(only)),
            Err(ScenarioError::InvalidBoundaryInput { .. })
        ));
    }

    #[test]
    fn boundary_inputs_are_validated() {
        let g = hr_graph(HR_EXPRESSION, None);
        let base = || hr_boundaries().remove(0);
        let mut cases: Vec<ScenarioBoundaryInput> = Vec::new();
        let mut b = base();
        b.boundary_key = " half_day".into();
        cases.push(b);
        let mut b = base();
        b.source_refs = Vec::new();
        cases.push(b);
        let mut b = base();
        b.source_refs = vec![id("requirement:missing")];
        cases.push(b);
        let mut b = base();
        b.source_refs = vec![id(FRACTION)];
        cases.push(b);
        let mut b = base();
        b.subject_ref = id(CAL);
        cases.push(b);
        let mut b = base();
        b.kind = ScenarioBoundaryKind::CalculationSemantic {
            calculation_ref: id(CALC),
            semantic_key: "half_day_fraction".into(),
            input_symbol: "no_such_symbol".into(),
            value_contract: decimal_contract(2),
        };
        cases.push(b);
        let mut b = base();
        b.kind = ScenarioBoundaryKind::CalculationSemantic {
            calculation_ref: id(CALC),
            semantic_key: "half_day_fraction".into(),
            input_symbol: "day_fraction".into(),
            value_contract: decimal_contract(3),
        };
        cases.push(b);
        let mut b = base();
        b.kind = ScenarioBoundaryKind::Numeric {
            attribute_ref: id(START),
            value_contract: decimal_contract(2),
        };
        cases.push(b);
        for case in cases {
            assert!(
                matches!(
                    analyze_scenarios(&g, &inputs(vec![case.clone()])),
                    Err(ScenarioError::InvalidBoundaryInput { .. })
                ),
                "{case:?}"
            );
        }
        // A numeric boundary of a Decimal Attribute is a bounded slot.
        let numeric = ScenarioBoundaryInput {
            boundary_key: "fraction_range".into(),
            source_refs: vec![id("requirement:hr-007")],
            subject_ref: id(CALC),
            kind: ScenarioBoundaryKind::Numeric {
                attribute_ref: id(FRACTION),
                value_contract: ScenarioValueContract::Decimal {
                    scale: 2,
                    interval: Some(constraint(Some(("0", false)), Some(("1", true)))),
                    unit: None,
                },
            },
        };
        let skeletons = analyze_scenarios(&g, &inputs(vec![numeric])).unwrap();
        assert_eq!(
            skeletons[0].value_slots[0].slot_id,
            "boundary:fraction_range:value"
        );
        assert_eq!(
            skeletons[0].given.attribute_values[&id(FRACTION)],
            SkeletonValue::Slot("boundary:fraction_range:value".into())
        );
    }

    #[test]
    fn calculation_examples_are_strict() {
        let only_half = || vec![hr_boundaries().remove(0)];
        let run = |examples: Vec<Value>| {
            analyze_scenarios(
                &hr_graph(HR_EXPRESSION, Some(examples)),
                &inputs(only_half()),
            )
        };
        assert!(run(vec![half_day_example("0.5")]).is_ok());
        let decoded: CalculationExampleV1 =
            serde_json::from_value(half_day_example("0.5")).unwrap();
        assert_eq!(decoded.key, "half_day_fraction");
        let with = |f: &dyn Fn(&mut Value)| {
            let mut v = half_day_example("0.5");
            f(&mut v);
            v
        };
        let invalid = [
            with(&|v| v["version"] = json!(2)),
            with(&|v| v["note"] = json!("x")),
            with(&|v| v["key"] = json!(" half_day_fraction")),
            with(&|v| v["inputs"] = json!([])),
            with(&|v| {
                v["inputs"] = json!([
                {"symbol": "day_fraction", "value": {"kind": "decimal", "value": "0.5", "unit": null}},
                {"symbol": "day_fraction", "value": {"kind": "decimal", "value": "0.5", "unit": null}}])
            }),
            with(&|v| {
                v["inputs"] = json!([
                {"symbol": "start_date", "value": {"kind": "date", "value": "2027-01-04"}},
                {"symbol": "day_fraction", "value": {"kind": "decimal", "value": "0.5", "unit": null}}])
            }),
            with(&|v| v["inputs"][0]["symbol"] = json!("half_fraction")),
            with(&|v| v["inputs"][0]["value"] = json!({"kind": "int", "value": 1})),
            with(&|v| {
                v["inputs"][0]["value"] =
                    json!({"kind": "decimal", "value": "0.5", "unit": "working_day"})
            }),
            with(&|v| {
                v["inputs"][0]["value"] = json!({"kind": "decimal", "value": "0.125", "unit": null})
            }),
            with(&|v| {
                v["expected_result"] = json!({"kind": "decimal", "value": "0.5", "unit": null})
            }),
            with(&|v| v["expected_result"] = json!({"kind": "bool", "value": true})),
            with(&|v| {
                v["inputs"][0]["value"] = json!({"kind": "decimal", "value": 0.5, "unit": null})
            }),
        ];
        for example in invalid {
            assert!(
                matches!(
                    run(vec![example.clone()]),
                    Err(ScenarioError::InvalidCalculationExample { .. })
                ),
                "{example}"
            );
        }
        // Two examples with the boundary key are ambiguous, never first-wins.
        assert!(matches!(
            run(vec![half_day_example("0.5"), half_day_example("0.25")]),
            Err(ScenarioError::AmbiguousCalculationExample { .. })
        ));
        // A valid example that violates the boundary contract is a hard error.
        let mut bounded = hr_boundaries().remove(0);
        bounded.kind = ScenarioBoundaryKind::CalculationSemantic {
            calculation_ref: id(CALC),
            semantic_key: "half_day_fraction".into(),
            input_symbol: "day_fraction".into(),
            value_contract: ScenarioValueContract::Decimal {
                scale: 2,
                interval: Some(constraint(Some(("0.5", false)), Some(("1", true)))),
                unit: None,
            },
        };
        assert!(matches!(
            analyze_scenarios(
                &hr_graph(HR_EXPRESSION, Some(vec![half_day_example("0.5")])),
                &inputs(vec![bounded])
            ),
            Err(ScenarioError::ExampleContractMismatch { .. })
        ));
        // A matching example without the boundary's input symbol is a hard error.
        let other_symbol = json!({
            "version": 1, "key": "half_day_fraction",
            "inputs": [{"symbol": "start_date", "value": {"kind": "date", "value": "2027-01-04"}}],
            "expected_result": null
        });
        assert!(matches!(
            run(vec![other_symbol]),
            Err(ScenarioError::ExampleContractMismatch { .. })
        ));
        // Examples with other keys never resolve this boundary.
        let mut other_key = half_day_example("0.5");
        other_key["key"] = json!("other_semantic");
        let s = run(vec![other_key]).unwrap();
        assert_eq!(
            s[0].unresolved_dependencies[0],
            sem(CALC, "half_day_fraction")
        );
    }

    // ================================================================= identity

    #[test]
    fn scenario_id_matches_the_independent_golden() {
        let skeletons = analyze_scenarios(&lifecycle_graph(), &empty_inputs()).unwrap();
        let submit = skeleton_for(&skeletons, "Transition transition:submit");
        assert_eq!(submit.scenario_ref, id(GOLDEN_TRANSITION_ID));
        // Audit and confirmation never enter identity.
        let mut g2 = lifecycle_nodes();
        for n in &mut g2 {
            n.audit = AuditMeta::new(
                id("actor:other"),
                ts("2027-06-01T00:00:00.000000000Z"),
                None,
                None,
            )
            .unwrap();
        }
        let again = analyze_scenarios(&graph(g2, lifecycle_edges()), &empty_inputs()).unwrap();
        assert_eq!(
            skeleton_for(&again, "Transition transition:submit").scenario_ref,
            submit.scenario_ref
        );
        // Another source transition or another target changes the ID.
        let mut nodes = lifecycle_nodes();
        nodes[4] = transition("transition:submit", "state:draft", "state:approved");
        let changed = analyze_scenarios(&graph(nodes, lifecycle_edges()), &empty_inputs()).unwrap();
        assert_ne!(
            skeleton_for(&changed, "Transition transition:submit").scenario_ref,
            submit.scenario_ref
        );
        // Boundary key change gives a different ID.
        let g = hr_graph(HR_EXPRESSION, None);
        let mut renamed = hr_boundaries().remove(0);
        let original = analyze_scenarios(&g, &inputs(vec![renamed.clone()])).unwrap()[0]
            .scenario_ref
            .clone();
        renamed.boundary_key = "half_day_v2".into();
        assert_ne!(
            analyze_scenarios(&g, &inputs(vec![renamed])).unwrap()[0].scenario_ref,
            original
        );
    }

    // ================================================================= inference

    fn one_slot_table() -> (Graph, ScenarioDerivationInputs) {
        let spec = DecisionTableSpec {
            hit_policy: "UNIQUE".into(),
            inputs: vec![column("flag", DecisionType::Bool, None)],
            outputs: vec![column("ok", DecisionType::Bool, None)],
            rows: vec![DecisionRow {
                inputs: vec![DecisionInputCell::Any],
                outputs: vec![DecisionOutputCell::Bool { value: true }],
            }],
            default_output: None,
        };
        let g = graph(
            vec![table_node("decision-table:flag", Accepted, "UNIQUE")],
            Vec::new(),
        );
        let inputs = ScenarioDerivationInputs::new(
            vec![ScenarioDecisionTableInput {
                decision_table_ref: id("decision-table:flag"),
                spec,
            }],
            Vec::new(),
        );
        (g, inputs)
    }

    #[test]
    fn inference_request_matches_independent_goldens() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
        let (g, inputs) = one_slot_table();
        let skeleton = analyze_scenarios(&g, &inputs).unwrap().remove(0);
        assert_eq!(
            skeleton.content_hash().unwrap().as_str(),
            GOLDEN_SKELETON_HASH
        );
        let request =
            build_scenario_request(&g, &inputs, &skeleton.scenario_ref, provider("mock")).unwrap();
        assert_eq!(request.context.version, SCENARIO_CONTEXT_VERSION);
        assert_eq!(request.request.stage, StageId::S4);
        assert_eq!(request.request.task_kind, SCENARIO_TASK_KIND);
        assert_eq!(request.request.input_refs, vec![id("decision-table:flag")]);
        assert!(request.request.evidence_refs.is_empty());
        assert_eq!(request.request.context_hash.as_str(), GOLDEN_CONTEXT_HASH);
        assert_eq!(request.request.id.as_str(), GOLDEN_REQUEST_ID);
        assert_eq!(
            build_scenario_request(&g, &inputs, &skeleton.scenario_ref, provider("mock")).unwrap(),
            request
        );
        // Provider policy enters request identity only through the InferenceRequest contract.
        let other =
            build_scenario_request(&g, &inputs, &skeleton.scenario_ref, provider("other")).unwrap();
        assert_eq!(other.context, request.context);
        assert_eq!(other.request.context_hash, request.request.context_hash);
        assert_ne!(other.request.id, request.request.id);
        // No slots, unknown scenarios: no request.
        let lifecycle = lifecycle_graph();
        let transition_ref = analyze_scenarios(&lifecycle, &empty_inputs()).unwrap()[0]
            .scenario_ref
            .clone();
        assert!(matches!(
            build_scenario_request(
                &lifecycle,
                &empty_inputs(),
                &transition_ref,
                provider("mock")
            ),
            Err(ScenarioError::NoInferenceSlots { .. })
        ));
        assert!(matches!(
            build_scenario_request(
                &g,
                &inputs,
                &id("scenario:0000000000000000"),
                provider("mock")
            ),
            Err(ScenarioError::UnknownScenario { .. })
        ));
        // The context exposes only literal slots, never semantic references.
        let hr = hr_graph(HR_EXPRESSION, None);
        let hr_inputs = inputs_with(hr_boundaries());
        let inclusive = analyze_scenarios(&hr, &hr_inputs).unwrap();
        let inclusive = skeleton_for(&inclusive, "Boundary inclusive_end for");
        let r = build_scenario_request(&hr, &hr_inputs, &inclusive.scenario_ref, provider("mock"))
            .unwrap();
        let text = serde_json::to_string(&r.context).unwrap();
        assert!(!text.contains("semantic_ref") && !text.contains(WORKING_DAYS_INCLUSIVE_KEY));
        assert_eq!(r.context.slots.len(), 2);
        let mut evidence = r.request.evidence_refs.clone();
        evidence.sort();
        assert_eq!(evidence, vec![fragment_id("requirement:hr-004")]);
    }

    fn inputs_with(boundaries: Vec<ScenarioBoundaryInput>) -> ScenarioDerivationInputs {
        ScenarioDerivationInputs::new(Vec::new(), boundaries)
    }

    #[test]
    fn schema_is_closed_draft_2020_12() {
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert!(!SCHEMA.contains("\"additionalProperties\": true"));
        let compiled = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(&schema)
            .unwrap();
        let sr = "scenario:0123456789abcdef";
        let valid = [
            json!({"version": 1, "scenario_ref": sr, "values": []}),
            json!({"version": 1, "scenario_ref": sr, "values": [{"slot_id": "given:attribute:attr:a", "value": {"kind": "date", "value": "2027-01-04"}}]}),
            json!({"version": 1, "scenario_ref": sr, "values": [
                {"slot_id": "boundary:k:value", "value": {"kind": "decimal", "value": "0.5", "unit": null}},
                {"slot_id": "decision-input:t:0:1", "value": {"kind": "enum", "value": "Annual"}}]}),
        ];
        for v in valid {
            assert!(compiled.is_valid(&v), "{v}");
        }
        let invalid = [
            json!({"version": 1, "scenario_ref": sr, "values": [], "explanation": "because"}),
            json!({"version": 2, "scenario_ref": sr, "values": []}),
            json!({"version": 1, "scenario_ref": "scenario:XYZ", "values": []}),
            json!({"version": 1, "scenario_ref": sr, "values": [{"slot_id": "boundary:k:value", "value": {"kind": "bool", "value": true}, "note": "x"}]}),
            json!({"version": 1, "scenario_ref": sr, "values": [{"slot_id": "boundary:k:value", "value": {"kind": "semantic_ref", "owner_ref": "calculation:x", "key": "half_day_fraction"}}]}),
            json!({"version": 1, "scenario_ref": sr, "values": [{"slot_id": "boundary:k:value", "value": {"kind": "decimal", "value": 0.5, "unit": null}}]}),
            json!({"version": 1, "scenario_ref": sr, "values": [], "scenario_kind": "boundary"}),
            json!({"version": 1, "scenario_ref": sr, "values": [], "then": [{"outcomes": ["outcome:x"]}]}),
            json!({"version": 1, "scenario_ref": sr, "values": [], "semantic_ref": {}}),
            json!({"version": 1, "scenario_ref": sr, "values": [], "operation_ref": "operation:x"}),
            json!({"version": 1, "scenario_ref": sr}),
        ];
        for v in invalid {
            assert!(!compiled.is_valid(&v), "{v}");
        }
    }

    #[test]
    fn prompt_allows_bounded_literals_only() {
        let prompt = std::str::from_utf8(PROMPT).unwrap();
        assert!(!prompt.contains("{{") && !prompt.contains("}}") && !prompt.contains("TODO"));
        assert!(prompt.contains("You are filling bounded test-fixture literals only."));
        for forbidden in [
            "business rules",
            "half-day fractions",
            "holiday behavior",
            "inclusive-end behavior",
            "formulas",
            "rounding policy",
            "authorization",
            "expected business outcomes",
            "unresolved semantic references",
        ] {
            assert!(prompt.contains(&format!("- {forbidden}")), "{forbidden}");
        }
        assert!(prompt.contains("You must not decide or infer:"));
        assert!(prompt
            .contains("If a slot cannot be safely filled from its supplied contract, omit it."));
        assert!(prompt.contains("Return only JSON"));
    }

    fn rule_row_setup() -> (
        Graph,
        ScenarioDerivationInputs,
        ScenarioSkeleton,
        ScenarioRequest,
    ) {
        let g = graph(vec![table_node(TABLE, Accepted, "FIRST")], Vec::new());
        let inputs = table_inputs(eligibility_spec());
        let skeleton = analyze_scenarios(&g, &inputs)
            .unwrap()
            .into_iter()
            .find(|s| s.name.ends_with("row 1"))
            .unwrap();
        let request =
            build_scenario_request(&g, &inputs, &skeleton.scenario_ref, provider("mock")).unwrap();
        (g, inputs, skeleton, request)
    }

    fn row1_values(days: i64, fraction: &str, leave: &str) -> Value {
        json!([
            {"slot_id": format!("decision-input:{TABLE}:1:1"), "value": {"kind": "enum", "value": leave}},
            {"slot_id": format!("decision-input:{TABLE}:1:2"), "value": {"kind": "int", "value": days}},
            {"slot_id": format!("decision-input:{TABLE}:1:3"), "value": {"kind": "decimal", "value": fraction, "unit": null}},
        ])
    }

    #[test]
    fn inference_fills_rule_row_slots_with_one_human_confirm_proposal() {
        let (g, inputs, skeleton, request) = rule_row_setup();
        let artifact = artifact_for(
            &request,
            json!({"version": 1, "scenario_ref": skeleton.scenario_ref, "values": row1_values(7, "0.25", "Sick")}),
        );
        let inference = ScenarioInference {
            request: &request,
            artifact: &artifact,
            derivation_ref: derivation(),
        };
        let result = derive_scenarios(&g, &inputs, &scenario_audit(), &[inference]).unwrap();
        // Row 0 still has an open slot; row 1 is proposed.
        assert_eq!(result.unresolved.len(), 1);
        assert_eq!(result.proposals.len(), 1);
        let proposal = &result.proposals[0];
        assert_eq!(proposal.stage, StageId::S4);
        assert_eq!(proposal.materiality, ProposalMateriality::Semantic);
        assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanConfirm);
        assert_eq!(proposal.confidence, None);
        assert_eq!(proposal.derivation_refs, vec![derivation()]);
        let SemanticPatch::AddNode { node } = &proposal.patch_set.patch else {
            panic!("AddNode expected")
        };
        assert_eq!(node.id, skeleton.scenario_ref);
        assert_eq!(node.status, Proposed);
        let NodePayload::Scenario(scenario) = &node.payload else {
            panic!()
        };
        assert_eq!(scenario.confirmation_status.as_deref(), Some("Derived"));
        assert_eq!(scenario.scenario_kind, ScenarioKind::RuleRow);
        assert_eq!(scenario.when, json!({"decision_table": TABLE}));
        let semantics = ScenarioSemantics::from_scenario(scenario).unwrap();
        assert_eq!(
            semantics.given.decision_inputs[&2],
            ScenarioValue::Literal(ScenarioLiteral::Int { value: 7 })
        );
        // Another realistic literal keeps the obligation ID.
        let artifact2 = artifact_for(
            &request,
            json!({"version": 1, "scenario_ref": skeleton.scenario_ref, "values": row1_values(30, "1.5", "Unpaid")}),
        );
        let inference2 = ScenarioInference {
            request: &request,
            artifact: &artifact2,
            derivation_ref: derivation(),
        };
        let result2 = derive_scenarios(&g, &inputs, &scenario_audit(), &[inference2]).unwrap();
        let SemanticPatch::AddNode { node: node2 } = &result2.proposals[0].patch_set.patch else {
            panic!()
        };
        assert_eq!(node2.id, node.id);
        assert_ne!(node2.payload, node.payload);
        // Partial output leaves the skeleton unresolved, without a proposal.
        let partial = artifact_for(
            &request,
            json!({"version": 1, "scenario_ref": skeleton.scenario_ref, "values": [row1_values(7, "0.25", "Sick")[0].clone()]}),
        );
        let r = derive_scenarios(
            &g,
            &inputs,
            &scenario_audit(),
            &[ScenarioInference {
                request: &request,
                artifact: &partial,
                derivation_ref: derivation(),
            }],
        )
        .unwrap();
        assert!(r.proposals.is_empty());
        assert_eq!(r.unresolved.len(), 2);
    }

    #[test]
    fn inference_cannot_answer_semantics_or_leave_its_contracts() {
        let (g, inputs, skeleton, request) = rule_row_setup();
        let sr = skeleton.scenario_ref.to_string();
        let run = |values: Value| {
            let artifact = artifact_for(
                &request,
                json!({"version": 1, "scenario_ref": sr, "values": values}),
            );
            derive_scenarios(
                &g,
                &inputs,
                &scenario_audit(),
                &[ScenarioInference {
                    request: &request,
                    artifact: &artifact,
                    derivation_ref: derivation(),
                }],
            )
        };
        let one = |slot: &str, value: Value| json!([{"slot_id": slot, "value": value}]);
        let s1 = format!("decision-input:{TABLE}:1:1");
        let s2 = format!("decision-input:{TABLE}:1:2");
        let s3 = format!("decision-input:{TABLE}:1:3");
        type Check = fn(&ScenarioError) -> bool;
        let cases: Vec<(Value, Check)> = vec![
            // Unknown and added slots, including semantic-hole names.
            (
                one(
                    "decision-input:x:9:9",
                    json!({"kind": "bool", "value": true}),
                ),
                |e| matches!(e, ScenarioError::UnknownSlot { .. }),
            ),
            (
                one(
                    "semantic:half_day_fraction",
                    json!({"kind": "decimal", "value": "0.5", "unit": null}),
                ),
                |e| matches!(e, ScenarioError::UnknownSlot { .. }),
            ),
            (
                one(
                    "given:attribute:attr:start-date",
                    json!({"kind": "date", "value": "2027-01-06"}),
                ),
                |e| matches!(e, ScenarioError::UnknownSlot { .. }),
            ),
            (
                one(
                    "calculation:inclusive_end",
                    json!({"kind": "bool", "value": true}),
                ),
                |e| matches!(e, ScenarioError::UnknownSlot { .. }),
            ),
            // Contract violations.
            (one(&s1, json!({"kind": "enum", "value": "Annual"})), |e| {
                matches!(e, ScenarioError::SlotContractViolation { .. })
            }),
            (
                one(&s1, json!({"kind": "enum", "value": "Maternity"})),
                |e| matches!(e, ScenarioError::SlotContractViolation { .. }),
            ),
            (one(&s2, json!({"kind": "int", "value": 31})), |e| {
                matches!(e, ScenarioError::SlotContractViolation { .. })
            }),
            (
                one(&s2, json!({"kind": "decimal", "value": "5", "unit": null})),
                |e| matches!(e, ScenarioError::SlotContractViolation { .. }),
            ),
            (
                one(
                    &s3,
                    json!({"kind": "decimal", "value": "0.5", "unit": "working_day"}),
                ),
                |e| matches!(e, ScenarioError::SlotContractViolation { .. }),
            ),
            (
                one(
                    &s3,
                    json!({"kind": "decimal", "value": "0.125", "unit": null}),
                ),
                |e| matches!(e, ScenarioError::SlotContractViolation { .. }),
            ),
            // Duplicates and ordering.
            (
                json!([{"slot_id": s2, "value": {"kind": "int", "value": 1}}, {"slot_id": s2, "value": {"kind": "int", "value": 2}}]),
                |e| matches!(e, ScenarioError::DuplicateSlot { .. }),
            ),
            (
                json!([{"slot_id": s2, "value": {"kind": "int", "value": 1}}, {"slot_id": s1, "value": {"kind": "enum", "value": "Sick"}}]),
                |e| matches!(e, ScenarioError::DuplicateSlot { .. }),
            ),
            // Semantic answers and structure changes fail the closed schema.
            (
                one(
                    &s2,
                    json!({"kind": "semantic_ref", "owner_ref": CALC, "key": "half_day_fraction"}),
                ),
                |e| matches!(e, ScenarioError::SchemaInvalid { .. }),
            ),
            (
                one(
                    &s2,
                    json!({"kind": "int", "value": 7, "outcome_ref": "outcome:x"}),
                ),
                |e| matches!(e, ScenarioError::SchemaInvalid { .. }),
            ),
        ];
        for (values, check) in cases {
            let result = run(values.clone());
            assert!(
                matches!(&result, Err(e) if check(e)),
                "{values}: {result:?}"
            );
        }
        // Root-level changes: other scenario, when change, extra outcome, version.
        let root = |output: Value| {
            let artifact = artifact_for(&request, output);
            derive_scenarios(
                &g,
                &inputs,
                &scenario_audit(),
                &[ScenarioInference {
                    request: &request,
                    artifact: &artifact,
                    derivation_ref: derivation(),
                }],
            )
        };
        assert!(matches!(
            root(json!({"version": 1, "scenario_ref": "scenario:0000000000000000", "values": []})),
            Err(ScenarioError::ScenarioMismatch { .. })
        ));
        for extra in [
            json!({"when": {"operation": "operation:x"}}),
            json!({"then": [{"outcomes": ["outcome:x"]}]}),
            json!({"answers": {"inclusive_end": true}}),
        ] {
            let mut output = json!({"version": 1, "scenario_ref": sr, "values": []});
            output
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert!(matches!(
                root(output),
                Err(ScenarioError::SchemaInvalid { .. })
            ));
        }
        // A stale request (graph changed) and a foreign artifact are rejected.
        let other_g = graph(
            vec![
                table_node(TABLE, Accepted, "FIRST"),
                state("state:x", Accepted),
            ],
            Vec::new(),
        );
        let artifact = artifact_for(
            &request,
            json!({"version": 1, "scenario_ref": sr, "values": []}),
        );
        assert!(derive_scenarios(
            &other_g,
            &inputs,
            &scenario_audit(),
            &[ScenarioInference {
                request: &request,
                artifact: &artifact,
                derivation_ref: derivation()
            }]
        )
        .is_ok());
        let mut changed = eligibility_spec();
        changed.rows[1].inputs[2] = DecisionInputCell::Any;
        changed.inputs[2].domain = Some(NumericDomain {
            lower: dec("0"),
            upper: dec("31"),
        });
        assert!(matches!(
            derive_scenarios(
                &g,
                &table_inputs(changed),
                &scenario_audit(),
                &[ScenarioInference {
                    request: &request,
                    artifact: &artifact,
                    derivation_ref: derivation()
                }]
            ),
            Err(ScenarioError::StaleScenarioRequest { .. })
        ));
        let mut foreign = artifact.clone();
        foreign.request_hash = Hash::content_sha256(b"other");
        assert!(matches!(
            derive_scenarios(
                &g,
                &inputs,
                &scenario_audit(),
                &[ScenarioInference {
                    request: &request,
                    artifact: &foreign,
                    derivation_ref: derivation()
                }]
            ),
            Err(ScenarioError::InvalidInferenceArtifact { .. })
        ));
        assert!(matches!(
            derive_scenarios(
                &g,
                &inputs,
                &scenario_audit(),
                &[
                    ScenarioInference {
                        request: &request,
                        artifact: &artifact,
                        derivation_ref: derivation()
                    },
                    ScenarioInference {
                        request: &request,
                        artifact: &artifact,
                        derivation_ref: derivation()
                    },
                ]
            ),
            Err(ScenarioError::DuplicateInference { .. })
        ));
        // The HR semantic holes have no slot at all: inference cannot reach them.
        let hr = hr_graph(HR_EXPRESSION, None);
        let hr_inputs = inputs_with(hr_boundaries());
        for skeleton in analyze_scenarios(&hr, &hr_inputs).unwrap() {
            for slot in &skeleton.value_slots {
                assert!(
                    slot.slot_id.starts_with("given:attribute:"),
                    "{}",
                    slot.slot_id
                );
                assert_eq!(slot.contract, ScenarioValueContract::Date);
            }
        }
        let inclusive = skeleton_for(
            &analyze_scenarios(&hr, &hr_inputs).unwrap(),
            "Boundary inclusive_end for",
        )
        .clone();
        let req =
            build_scenario_request(&hr, &hr_inputs, &inclusive.scenario_ref, provider("mock"))
                .unwrap();
        let answer = |slot: &str, value: Value| {
            let artifact = artifact_for(
                &req,
                json!({"version": 1, "scenario_ref": inclusive.scenario_ref, "values": [{"slot_id": slot, "value": value}]}),
            );
            derive_scenarios(
                &hr,
                &hr_inputs,
                &scenario_audit(),
                &[ScenarioInference {
                    request: &req,
                    artifact: &artifact,
                    derivation_ref: derivation(),
                }],
            )
        };
        assert!(matches!(
            answer(
                &format!("given:attribute:{INCLUSIVE}"),
                json!({"kind": "bool", "value": true})
            ),
            Err(ScenarioError::UnknownSlot { .. })
        ));
        assert!(matches!(
            answer(
                &format!("given:attribute:{FRACTION}"),
                json!({"kind": "decimal", "value": "0.5", "unit": null})
            ),
            Err(ScenarioError::UnknownSlot { .. })
        ));
        assert!(matches!(
            answer(
                &format!("given:attribute:{START}"),
                json!({"kind": "bool", "value": true})
            ),
            Err(ScenarioError::SlotContractViolation { .. })
        ));
    }

    // ================================================================= proposals and reconciliation

    #[test]
    fn deterministic_and_semantic_ref_scenarios_are_proposed() {
        let mut nodes = hr_nodes(HR_EXPRESSION, None);
        nodes.extend(lifecycle_nodes());
        let g = graph(nodes, [hr_edges(), lifecycle_edges()].concat());
        let result =
            derive_scenarios(&g, &inputs_with(hr_boundaries()), &scenario_audit(), &[]).unwrap();
        // Two transitions, half-day and holiday are fully materialized (SemanticRefs allowed);
        // inclusive-end waits for its date slots.
        assert_eq!(result.proposals.len(), 4);
        assert_eq!(result.unresolved.len(), 1);
        for proposal in &result.proposals {
            assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanConfirm);
            assert!(proposal.derivation_refs.is_empty());
            let SemanticPatch::AddNode { node } = &proposal.patch_set.patch else {
                panic!()
            };
            assert_eq!(node.status, Proposed);
            assert_eq!(node.revision, 1);
            let NodePayload::Scenario(s) = &node.payload else {
                panic!()
            };
            assert_eq!(s.confirmation_status.as_deref(), Some("Derived"));
            assert!(validate_scenario(&g, s).is_ok());
            assert!(s.requirement_refs.is_some() || s.derived_from_refs.is_some());
        }
        let half = result
            .proposals
            .iter()
            .find_map(|p| match &p.patch_set.patch {
                SemanticPatch::AddNode { node } => match &node.payload {
                    NodePayload::Scenario(s) if s.name.starts_with("Boundary half_day") => {
                        Some(node.clone())
                    }
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        let mut evidence: Vec<Id> = half.evidence.iter().map(|e| e.as_id().clone()).collect();
        evidence.sort();
        assert_eq!(evidence, vec![fragment_id("requirement:hr-007")]);
    }

    fn scenario_node(
        skeleton: &ScenarioSkeleton,
        status: ElementStatus,
        values: &BTreeMap<String, ScenarioLiteral>,
    ) -> Node {
        let scenario = skeleton.materialize(values).unwrap();
        node(
            skeleton.scenario_ref.as_str(),
            status,
            NodePayload::Scenario(scenario),
        )
    }

    #[test]
    fn existing_scenarios_reconcile_and_never_overwrite() {
        let pre = hr_nodes(HR_EXPRESSION, None);
        let bounds = || inputs_with(vec![hr_boundaries().remove(0)]);
        let half = analyze_scenarios(&graph(pre.clone(), hr_edges()), &bounds())
            .unwrap()
            .remove(0);
        let none = BTreeMap::new();
        let with = |extra: Node, examples: Option<Vec<Value>>| {
            let mut nodes = hr_nodes(HR_EXPRESSION, examples);
            nodes.push(extra);
            derive_scenarios(&graph(nodes, hr_edges()), &bounds(), &scenario_audit(), &[]).unwrap()
        };
        // Exact Proposed and Accepted matches.
        let r = with(scenario_node(&half, Proposed, &none), None);
        assert!(r.proposals.is_empty() && r.collisions.is_empty());
        assert_eq!(
            (r.existing[0].accepted, r.existing[0].current),
            (false, true)
        );
        let mut confirmed = scenario_node(&half, Accepted, &none);
        if let NodePayload::Scenario(s) = &mut confirmed.payload {
            s.confirmation_status = Some("Confirmed".into());
        }
        let r = with(confirmed.clone(), None);
        assert_eq!(
            (r.existing[0].accepted, r.existing[0].current),
            (true, true)
        );
        // The answer resolves later: the Accepted Scenario is reported, not rewritten.
        let r = with(confirmed, Some(vec![half_day_example("0.5")]));
        assert!(r.proposals.is_empty() && r.collisions.is_empty());
        assert_eq!(
            (r.existing[0].accepted, r.existing[0].current),
            (true, false)
        );
        // Other material at the ID collides.
        let other_type = node(
            half.scenario_ref.as_str(),
            Accepted,
            NodePayload::State(State { name: "x".into() }),
        );
        let mut other_payload = scenario_node(&half, Proposed, &none);
        if let NodePayload::Scenario(s) = &mut other_payload.payload {
            s.name = "Something else".into();
        }
        let mut pending = scenario_node(&half, Proposed, &none);
        if let NodePayload::Scenario(s) = &mut pending.payload {
            s.confirmation_status = Some("Pending".into());
        }
        let mut rejected = scenario_node(&half, Proposed, &none);
        rejected.status = ElementStatus::Rejected;
        for n in [other_type, other_payload, pending, rejected] {
            let r = with(n, None);
            assert!(r.proposals.is_empty());
            assert_eq!(r.collisions.len(), 1);
            assert_eq!(r.collisions[0].scenario_ref, half.scenario_ref);
        }
        // A filled slot holding a contract-valid literal matches the slotted obligation.
        let g = graph(vec![table_node(TABLE, Accepted, "FIRST")], Vec::new());
        let row1 = analyze_scenarios(&g, &table_inputs(eligibility_spec()))
            .unwrap()
            .into_iter()
            .find(|s| s.name.ends_with("row 1"))
            .unwrap();
        let values: BTreeMap<String, ScenarioLiteral> = [
            (
                format!("decision-input:{TABLE}:1:1"),
                ScenarioLiteral::Enum {
                    value: "Sick".into(),
                },
            ),
            (
                format!("decision-input:{TABLE}:1:2"),
                ScenarioLiteral::Int { value: 3 },
            ),
            (format!("decision-input:{TABLE}:1:3"), lit_dec("1", None)),
        ]
        .into_iter()
        .collect();
        let g2 = graph(
            vec![
                table_node(TABLE, Accepted, "FIRST"),
                scenario_node(&row1, Proposed, &values),
            ],
            Vec::new(),
        );
        let r = derive_scenarios(
            &g2,
            &table_inputs(eligibility_spec()),
            &scenario_audit(),
            &[],
        )
        .unwrap();
        assert_eq!(r.existing.len(), 1);
        assert_eq!(r.existing[0].scenario_ref, row1.scenario_ref);
    }

    // ================================================================= F0.14 compatibility

    #[test]
    fn operation_scenarios_survive_the_unchanged_f014_projection() {
        let mut nodes = lifecycle_nodes();
        nodes.extend(evidence_nodes(&["requirement:hr-009"]));
        nodes.push(requirement(
            "requirement:hr-009",
            "Submitting a valid leave request shall change its status from Draft to Submitted.",
        ));
        let skeletons =
            analyze_scenarios(&graph(nodes.clone(), lifecycle_edges()), &empty_inputs()).unwrap();
        for s in &skeletons {
            let mut scenario = s.materialize(&BTreeMap::new()).unwrap();
            scenario.requirement_refs = Some(vec![id("requirement:hr-009")]);
            scenario.confirmation_status = Some("Confirmed".into());
            // A valid multi-category then: states and an event.
            if s.name.ends_with("transition:submit") {
                scenario
                    .then
                    .push(json!({"events": ["event:leave-approved"]}));
            }
            nodes.push(node(
                s.scenario_ref.as_str(),
                Accepted,
                NodePayload::Scenario(scenario),
            ));
        }
        let g = graph(nodes, lifecycle_edges());
        let profile = load_builtin_software_profile().unwrap();
        let projection = project_functional_v2(&g, &profile).unwrap();
        let submit = skeleton_for(&skeletons, "Transition transition:submit")
            .scenario_ref
            .clone();
        let approve = skeleton_for(&skeletons, "Transition transition:approve")
            .scenario_ref
            .clone();
        let entry = projection
            .document
            .scenarios
            .iter()
            .find(|e| e.id == submit)
            .expect("operation scenario projected");
        assert_eq!(entry.operation, "operation:submit");
        assert_eq!(
            entry.then.keys().cloned().collect::<Vec<_>>(),
            vec!["events".to_owned(), "states_active".to_owned()]
        );
        let notices: Vec<_> = projection
            .metadata
            .warnings
            .iter()
            .chain(&projection.metadata.lossy_mappings)
            .collect();
        let about = |scenario: &Id, code: &str| {
            notices
                .iter()
                .any(|n| n.code == code && n.source_refs.contains(scenario))
        };
        assert!(!about(&submit, "V2_SCENARIO_OPERATION_MISSING"));
        assert!(!about(&submit, "V2_SCENARIO_THEN_CONFLICT"));
        // Event-triggered scenarios stay intentionally lossy in the legacy projection.
        assert!(projection
            .document
            .scenarios
            .iter()
            .all(|e| e.id != approve));
        assert!(about(&approve, "V2_SCENARIO_OPERATION_MISSING"));
    }

    // ================================================================= determinism

    #[test]
    fn reverse_insertion_and_input_order_are_byte_identical() {
        let build = |reverse: bool| {
            let mut nodes = hr_nodes(HR_EXPRESSION, Some(vec![half_day_example("0.5")]));
            nodes.extend(lifecycle_nodes());
            nodes.push(table_node(TABLE, Accepted, "FIRST"));
            let mut edges = [hr_edges(), lifecycle_edges()].concat();
            let mut boundaries = hr_boundaries();
            if reverse {
                nodes.reverse();
                edges.reverse();
                boundaries.reverse();
            }
            let g = graph(nodes, edges);
            let inputs = ScenarioDerivationInputs::new(
                vec![ScenarioDecisionTableInput {
                    decision_table_ref: id(TABLE),
                    spec: eligibility_spec(),
                }],
                boundaries,
            );
            let result = derive_scenarios(&g, &inputs, &scenario_audit(), &[]).unwrap();
            let skeletons: Vec<(Id, String, Vec<String>, String)> = result
                .skeletons
                .iter()
                .map(|s| {
                    (
                        s.scenario_ref.clone(),
                        s.content_hash().unwrap().to_string(),
                        s.value_slots.iter().map(|v| v.slot_id.clone()).collect(),
                        format!("{:?}", s.unresolved_dependencies),
                    )
                })
                .collect();
            let requests: Vec<Hash> = result
                .skeletons
                .iter()
                .filter(|s| !s.value_slots.is_empty())
                .map(|s| {
                    build_scenario_request(&g, &inputs, &s.scenario_ref, provider("mock"))
                        .unwrap()
                        .request
                        .id
                })
                .collect();
            let proposals: Vec<(Id, Vec<u8>)> = result
                .proposals
                .iter()
                .map(|p| (p.id.clone(), to_canonical_json(&p.patch_set).unwrap()))
                .collect();
            (skeletons, requests, proposals)
        };
        assert_eq!(build(false), build(true));
    }

    // ================================================================= source guards

    #[test]
    fn production_source_has_no_capability_or_hr_constant() {
        for forbidden in [
            "std::fs",
            "File::open",
            "ArtifactStore",
            "SqliteArtifactStore",
            "SqliteRevisionStore",
            "rusqlite",
            "Clock",
            "SystemClock",
            "Utc::now",
            "Instant::now",
            "SystemTime::now",
            "OffsetDateTime::now",
            "reqwest",
            ".execute(",
            "MockProvider",
            "unsafe",
            ".commit(",
            "branch_head",
            "fixtures/",
            "hr-leave",
            "expert-answers",
            "fixture-contract",
            "HR-0",
            "half_day",
            "half-day",
            "0.5",
            "holiday_inside_period",
            "PL-Office",
            "holiday_consumes_working_day",
        ] {
            assert!(
                !SCENARIO_SRC.contains(forbidden),
                "scenario.rs contains {forbidden}"
            );
        }
        // The only inclusivity key is the generic named constant.
        assert_eq!(SCENARIO_SRC.matches("\"inclusive_end\"").count(), 1);
        assert!(SCENARIO_SRC
            .contains("pub const WORKING_DAYS_INCLUSIVE_KEY: &str = \"inclusive_end\";"));
        assert!(!include_str!("../src/lib.rs").contains("Proposal"));
        let _ = (SCENARIO_OUTPUT_VERSION, Edge::validate, Scenario::clone);
    }
}
