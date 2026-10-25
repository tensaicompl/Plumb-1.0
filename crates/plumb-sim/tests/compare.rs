//! S4.3 contract tests for Then comparison and scenario trace material (Hotfix 052).
//!
//! Every test lives in `compare_contract` (`cargo test -p plumb-sim --test compare`). Graphs,
//! Scenarios, calendars and predicates are built in memory and executed through the S4.2 run();
//! no fixture file, store, clock, network or provider other than the injected deterministic test
//! doubles is used. The artifact hash golden was computed independently in Python.

mod compare_contract {
    use std::collections::{BTreeMap, BTreeSet};
    use std::str::FromStr;

    use plumb_core::{Hash, Id, Timestamp};
    use plumb_expr::{
        CalendarDefinition, PredicateError, PredicateProvider, RefValue, StaticCalendarProvider,
    };
    use plumb_functional::decision_table::{
        DecisionColumn, DecisionInputCell, DecisionOutputCell, DecisionRow, DecisionTableSpec,
        DecisionType,
    };
    use plumb_functional::{
        CalculationBindingExposure, CalculationScopeBinding, ScenarioDecisionTableInput,
        ScenarioDependency, ScenarioGiven, ScenarioLiteral, ScenarioThen, ScenarioValue,
        ScenarioWhen, CALCULATION_ORIGIN_EXTENSION, HOLIDAY_DATE_KEY, WORKING_DAYS_INCLUSIVE_KEY,
    };
    use plumb_psg::{
        Attribute, AuditMeta, Calculation, Calendar, DecisionTable, Edge, ElementStatus, Entity,
        Event, ExtensionKey, Graph, Node, NodePayload, Operation, OperationKind, Outcome,
        OutcomeKind, RelationKind, RelationProperties, Rule, RuleKind, Scenario, ScenarioKind,
        State, Transition,
    };
    use plumb_psg::{ScenarioRun, ScenarioRunResult};
    use plumb_sim::{
        build_execution_model, run, ExecutionExpressionScopeInput, ExecutionFailure,
        ExecutionInputs, ExecutionResult, ExecutionRun, ExecutionRuntime, ExecutionTraceStep,
        MissingSemanticKind, ResolutionSource, RuntimeSemanticValue, SimulationError,
    };
    use plumb_sim::{
        compare_run, ComparisonError, ComparisonTraceStep, DiffLocation, ScenarioComparison,
        ScenarioDiff, ScenarioTraceArtifact, ScenarioTraceArtifactBody, ScenarioTraceStep,
        ScenarioVerdict, SCENARIO_TRACE_VERSION,
    };
    use serde_json::{json, Value};

    use ElementStatus::Accepted;

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const SCN: &str = "scenario:under-test";

    // HR calculation fixture (built in memory; no fixture file is read).
    const CAL: &str = "cal:office";
    const CALC: &str = "calculation:requested-working-days";
    const LEAVE: &str = "entity:leave";
    const START: &str = "attr:start-date";
    const END: &str = "attr:end-date";
    const INCLUSIVE: &str = "attr:inclusive-end";
    const FRACTION: &str = "attr:day-fraction";
    const HALF_DAY: &str = "half_day_fraction";
    const HR_EXPRESSION: &str =
        "working_days([start_date, end_date], calendar, inclusive_end) * day_fraction";

    // Operation fixture.
    const REQUEST: &str = "entity:request";
    const AMOUNT: &str = "attr:amount";
    const PRIORITY: &str = "attr:priority";
    const APPROVED: &str = "attr:approved";
    const DUE: &str = "attr:due";
    const FEE: &str = "attr:fee";
    const HOURS: &str = "attr:hours";
    const NOTE: &str = "attr:note";
    const STAMP: &str = "attr:stamp";
    const DRAFT: &str = "state:draft";
    const SUBMITTED: &str = "state:submitted";
    const CLOSED: &str = "state:closed";
    const SUBMIT: &str = "op:submit";
    const TR_SUBMIT: &str = "transition:submit";
    const RULE: &str = "rule:large";
    const TABLE: &str = "decision-table:eligibility";

    const COMPARE_SRC: &str = include_str!("../src/compare.rs");
    const TRACE_SRC: &str = include_str!("../src/trace.rs");

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

    /// An in-memory graph under construction.
    #[derive(Clone)]
    struct World {
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    }

    impl World {
        fn empty() -> World {
            World {
                nodes: Vec::new(),
                edges: Vec::new(),
            }
        }

        fn add(&mut self, n: Node) -> &mut World {
            self.nodes.retain(|existing| existing.id != n.id);
            self.nodes.push(n);
            self
        }

        fn link(&mut self, kind: RelationKind, from: &str, to: &str) -> &mut World {
            self.link_with(kind, from, to, Accepted)
        }

        fn link_with(
            &mut self,
            kind: RelationKind,
            from: &str,
            to: &str,
            status: ElementStatus,
        ) -> &mut World {
            let edge_id = format!("edge:e{}", self.edges.len());
            self.edges.push(Edge {
                id: id(&edge_id),
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
            });
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

        /// Adds the Accepted, Confirmed Scenario under test with Then expectations.
        fn scenario_then(
            &mut self,
            given: ScenarioGiven<ScenarioValue>,
            when: ScenarioWhen,
            then: ScenarioThen<ScenarioValue>,
        ) -> &mut World {
            self.scenario_with(given, when, then, Accepted, Some("Confirmed"))
        }

        fn scenario_with(
            &mut self,
            given: ScenarioGiven<ScenarioValue>,
            when: ScenarioWhen,
            then: ScenarioThen<ScenarioValue>,
            status: ElementStatus,
            confirmation: Option<&str>,
        ) -> &mut World {
            let encode = |v: &ScenarioValue| serde_json::to_value(v).unwrap();
            let when_value = when.to_value();
            let anchor = when_value.as_object().unwrap().values().next().unwrap();
            let anchor = id(anchor.as_str().unwrap());
            self.add(node(
                SCN,
                status,
                NodePayload::Scenario(Scenario {
                    name: "Scenario under test".into(),
                    scenario_kind: ScenarioKind::Boundary,
                    given: given.to_values(encode),
                    when: when_value,
                    then: then.to_values(encode),
                    requirement_refs: None,
                    derived_from_refs: Some(vec![anchor]),
                    confirmation_status: confirmation.map(Into::into),
                }),
            ))
        }
    }

    fn entity(node_id: &str, name: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Entity(Entity {
                name: name.into(),
                description: None,
                aggregate_root: None,
            }),
        )
    }

    fn attribute(
        node_id: &str,
        name: &str,
        value_type: &str,
        unit: Option<&str>,
        members: Option<&[&str]>,
    ) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Attribute(Attribute {
                name: name.into(),
                value_type: value_type.into(),
                nullable: false,
                unit: unit.map(Into::into),
                precision: None,
                enum_values: members.map(|m| m.iter().map(|s| s.to_string()).collect()),
                data_classification: None,
            }),
        )
    }

    fn state(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::State(State {
                name: node_id.into(),
            }),
        )
    }

    fn operation(node_id: &str, pre: &[&str], post: &[&str]) -> Node {
        let list = |items: &[&str]| {
            (!items.is_empty()).then(|| items.iter().map(|s| s.to_string()).collect())
        };
        node(
            node_id,
            Accepted,
            NodePayload::Operation(Operation {
                name: node_id.into(),
                operation_kind: OperationKind::Command,
                input_schema_ref: None,
                output_schema_ref: None,
                preconditions: list(pre),
                postconditions: list(post),
                idempotency: None,
                transaction_semantics: None,
            }),
        )
    }

    fn transition(node_id: &str, from: &str, to: &str, guard: Option<&str>) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Transition(Transition {
                stateful_ref: id(REQUEST),
                from_state: id(from),
                to_state: id(to),
                guard_expr: guard.map(Into::into),
                effect_refs: None,
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

    fn outcome(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Outcome(Outcome {
                name: node_id.into(),
                outcome_kind: OutcomeKind::Success,
            }),
        )
    }

    fn rule(node_id: &str, condition: Option<&str>, action: Option<&str>) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Rule(Rule {
                name: node_id.into(),
                rule_kind: RuleKind::Constraint,
                condition_expr: condition.map(Into::into),
                action_expr: action.map(Into::into),
            }),
        )
    }

    fn calendar(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Calendar(Calendar {
                time_zone: "Europe/Warsaw".into(),
                week_pattern: json!(["Mon", "Tue", "Wed", "Thu", "Fri"]),
                region: Some("PL".into()),
                holiday_source: Some("public holidays".into()),
            }),
        )
    }

    fn root_binding(node_ref: &str, symbol: &str) -> CalculationScopeBinding {
        CalculationScopeBinding {
            node_ref: id(node_ref),
            symbol: symbol.into(),
            exposure: CalculationBindingExposure::Root,
        }
    }

    /// An Accepted Calculation with a valid S2.6 origin (or none when `origin` is false).
    fn calculation(
        node_id: &str,
        expression: &str,
        result: (&str, Option<&str>),
        calendar_ref: Option<&str>,
        bindings: &[(&str, &str)],
        examples: Option<Vec<Value>>,
        origin: bool,
    ) -> Node {
        let mut n = node(
            node_id,
            Accepted,
            NodePayload::Calculation(Calculation {
                name: node_id.into(),
                expression: expression.into(),
                result_type: result.0.into(),
                unit: result.1.map(Into::into),
                rounding: None,
                calendar_ref: calendar_ref.map(id),
                examples,
            }),
        );
        if origin {
            let mut used: Vec<Value> = bindings
                .iter()
                .map(|(r, s)| json!({"node_ref": r, "symbol": s, "exposure": {"kind": "root"}}))
                .collect();
            used.sort_by_key(|b| b["symbol"].as_str().unwrap().to_owned());
            let range = json!({"requirement_ref": "requirement:r-001", "start": 0, "end": 4});
            n.extensions.insert(
                ExtensionKey::from_str(CALCULATION_ORIGIN_EXTENSION).unwrap(),
                json!({"name_range": range, "expression_evidence": [range], "used_bindings": used}),
            );
        }
        n
    }

    /// The request entity, its typed attributes and lifecycle states.
    fn request_world() -> World {
        let mut w = World::empty();
        w.add(entity(REQUEST, "Request"))
            .add(attribute(AMOUNT, "amount", "Int", None, None))
            .add(attribute(
                PRIORITY,
                "priority",
                "Enum",
                None,
                Some(&["High", "Low"]),
            ))
            .add(attribute(APPROVED, "approved", "Bool", None, None))
            .add(attribute(DUE, "due", "Date", None, None))
            .add(attribute(FEE, "fee", "Decimal(2)", None, None))
            .add(attribute(HOURS, "hours", "Decimal(1)", Some("hour"), None))
            .add(attribute(NOTE, "note", "String", None, None))
            .add(attribute(STAMP, "stamp", "DateTime", None, None))
            .add(state(DRAFT))
            .add(state(SUBMITTED))
            .add(state(CLOSED));
        for a in [AMOUNT, PRIORITY, APPROVED, DUE, FEE, HOURS, NOTE, STAMP] {
            w.link(RelationKind::HasAttribute, REQUEST, a);
        }
        for s in [DRAFT, SUBMITTED, CLOSED] {
            w.link(RelationKind::HasState, REQUEST, s);
        }
        w
    }

    /// The submit Operation with its draft -> submitted Transition.
    fn submit_world(pre: &[&str], post: &[&str], guard: Option<&str>) -> World {
        let mut w = request_world();
        w.add(operation(SUBMIT, pre, post))
            .add(transition(TR_SUBMIT, DRAFT, SUBMITTED, guard))
            .link(RelationKind::TransitionsVia, TR_SUBMIT, SUBMIT);
        w
    }

    fn hr_world(expression: &str, examples: Option<Vec<Value>>) -> World {
        let symbols = [
            (START, "start_date"),
            (END, "end_date"),
            (INCLUSIVE, "inclusive_end"),
            (FRACTION, "day_fraction"),
        ];
        let used: Vec<(&str, &str)> = symbols
            .into_iter()
            .filter(|(_, s)| expression.contains(s))
            .collect();
        let mut w = World::empty();
        w.add(entity(LEAVE, "Leave"))
            .add(attribute(START, "start_date", "Date", None, None))
            .add(attribute(END, "end_date", "Date", None, None))
            .add(attribute(INCLUSIVE, "inclusive_end", "Bool", None, None))
            .add(attribute(
                FRACTION,
                "day_fraction",
                "Decimal(2)",
                None,
                None,
            ))
            .add(calendar(CAL))
            .add(calculation(
                CALC,
                expression,
                ("Decimal(2)", Some("working_day")),
                Some(CAL),
                &used,
                examples,
                true,
            ));
        for a in [START, END, INCLUSIVE, FRACTION] {
            w.link(RelationKind::HasAttribute, LEAVE, a);
        }
        w
    }

    fn half_day_example(value: &str) -> Value {
        json!({
            "version": 1,
            "key": HALF_DAY,
            "inputs": [{"symbol": "day_fraction", "value": {"kind": "decimal", "value": value, "unit": null}}],
            "expected_result": null
        })
    }

    fn lit(literal: ScenarioLiteral) -> ScenarioValue {
        ScenarioValue::Literal(literal)
    }

    fn sem(owner: &str, key: &str) -> ScenarioValue {
        ScenarioValue::SemanticRef(ScenarioDependency {
            owner_ref: id(owner),
            key: key.into(),
        })
    }

    fn int(value: i64) -> ScenarioLiteral {
        ScenarioLiteral::Int { value }
    }

    fn boolean(value: bool) -> ScenarioLiteral {
        ScenarioLiteral::Bool { value }
    }

    fn date(value: &str) -> ScenarioLiteral {
        ScenarioLiteral::Date {
            value: value.into(),
        }
    }

    fn decimal(value: &str, unit: Option<&str>) -> ScenarioLiteral {
        ScenarioLiteral::Decimal {
            value: value.into(),
            unit: unit.map(Into::into),
        }
    }

    fn enumeration(value: &str) -> ScenarioLiteral {
        ScenarioLiteral::Enum {
            value: value.into(),
        }
    }

    fn given(attributes: &[(&str, ScenarioValue)]) -> ScenarioGiven<ScenarioValue> {
        let mut g = ScenarioGiven::default();
        for (a, v) in attributes {
            g.attribute_values.insert(id(a), v.clone());
        }
        g
    }

    fn given_states(
        attributes: &[(&str, ScenarioValue)],
        states: &[&str],
    ) -> ScenarioGiven<ScenarioValue> {
        let mut g = given(attributes);
        g.states_active = states.iter().map(|s| id(s)).collect();
        g
    }

    fn scope(owner: &str, bindings: &[(&str, &str)]) -> ExecutionExpressionScopeInput {
        ExecutionExpressionScopeInput {
            owner_ref: id(owner),
            bindings: bindings.iter().map(|(r, s)| root_binding(r, s)).collect(),
        }
    }

    fn scopes(items: Vec<ExecutionExpressionScopeInput>) -> ExecutionInputs {
        ExecutionInputs {
            expression_scopes: items,
            ..ExecutionInputs::default()
        }
    }

    /// Mon 2026-01-05 .. Sun 2026-01-11; weekend Saturday and Sunday.
    fn calendars(holidays: &[&str]) -> StaticCalendarProvider {
        let day = |s: &str| ts(&format!("{s}T00:00:00Z")).as_offset_datetime().date();
        let weekend = [day("2026-01-03").weekday(), day("2026-01-04").weekday()];
        StaticCalendarProvider::new([CalendarDefinition::new(
            id(CAL),
            weekend,
            holidays.iter().map(|h| day(h)),
        )])
        .unwrap()
    }

    struct Managers(bool);

    impl PredicateProvider for Managers {
        fn direct_manager(&self, _: &RefValue, _: &RefValue) -> Result<bool, PredicateError> {
            Ok(self.0)
        }
    }

    const NO_MANAGERS: Managers = Managers(false);

    fn runtime<'a>(
        calendars: &'a StaticCalendarProvider,
        predicates: &'a Managers,
        actor: Option<&str>,
        values: Vec<RuntimeSemanticValue>,
    ) -> ExecutionRuntime<'a> {
        ExecutionRuntime {
            actor_ref: actor.map(id),
            semantic_values: values,
            calendars,
            predicates,
        }
    }

    // ------------------------------------------------------------------ comparison helpers

    fn then() -> ScenarioThen<ScenarioValue> {
        ScenarioThen::default()
    }

    fn compare_with(
        world: &World,
        inputs: ExecutionInputs,
        rt: &ExecutionRuntime<'_>,
    ) -> Result<(ExecutionRun, ScenarioComparison), ComparisonError> {
        let graph = world.graph();
        let model = build_execution_model(&graph, inputs)?;
        let execution = run(&model, &id(SCN), rt)?;
        let comparison = compare_run(&model, &execution, rt)?;
        Ok((execution, comparison))
    }

    fn compare(world: &World, inputs: ExecutionInputs) -> ScenarioComparison {
        let cal = calendars(&[]);
        compare_with(
            world,
            inputs,
            &runtime(&cal, &NO_MANAGERS, None, Vec::new()),
        )
        .unwrap()
        .1
    }

    fn compare_err(world: &World, inputs: ExecutionInputs) -> ComparisonError {
        let cal = calendars(&[]);
        compare_with(
            world,
            inputs,
            &runtime(&cal, &NO_MANAGERS, None, Vec::new()),
        )
        .unwrap_err()
    }

    fn comparison_steps(c: &ScenarioComparison) -> Vec<&ComparisonTraceStep> {
        c.trace
            .iter()
            .filter_map(|s| match s {
                ScenarioTraceStep::Comparison(step) => Some(step),
                ScenarioTraceStep::Execution(_) => None,
            })
            .collect()
    }

    fn assert_consistent(c: &ScenarioComparison) {
        let body = &c.trace_artifact.body;
        assert_eq!(body.version, SCENARIO_TRACE_VERSION);
        assert_eq!(c.scenario_run.scenario_ref, body.scenario_ref);
        assert_eq!(c.scenario_run.specification_hash, body.specification_hash);
        assert_eq!(c.scenario_run.result, body.result);
        assert_eq!(c.scenario_run.result, c.result.run_result());
        let trace: Vec<Value> = body
            .trace
            .iter()
            .map(|s| serde_json::to_value(s).unwrap())
            .collect();
        assert_eq!(c.scenario_run.trace, trace);
        assert_eq!(body.trace, c.trace);
        assert_eq!(body.diffs, c.diffs);
        assert_eq!(
            c.trace_artifact.bytes,
            plumb_core::to_canonical_json(body).unwrap()
        );
        assert_eq!(
            c.trace_artifact.hash,
            Hash::content_sha256(&c.trace_artifact.bytes)
        );
    }

    /// Submit with a transition, produced event and outcome, a governing rule and a calculation.
    fn rich_world(rule_condition: &str, then: ScenarioThen<ScenarioValue>) -> World {
        let mut w = submit_world(&["amount > 0"], &[], None);
        w.add(event("event:submitted"))
            .link(RelationKind::Produces, SUBMIT, "event:submitted")
            .add(outcome("outcome:accepted"))
            .link(RelationKind::Produces, SUBMIT, "outcome:accepted")
            .add(rule(RULE, Some(rule_condition), None))
            .link(RelationKind::GovernedBy, SUBMIT, RULE)
            .add(calculation(
                CALC_A,
                "amount * 2",
                ("Int", None),
                None,
                &[(AMOUNT, "amount")],
                None,
                true,
            ))
            .link(RelationKind::UsesCalculation, SUBMIT, CALC_A);
        w.scenario_then(
            given_states(&[(AMOUNT, lit(int(20)))], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
            then,
        );
        w
    }

    fn amount_scope(owner: &str) -> ExecutionExpressionScopeInput {
        scope(owner, &[(AMOUNT, "amount"), (APPROVED, "approved")])
    }

    fn rich_inputs() -> ExecutionInputs {
        scopes(vec![amount_scope(SUBMIT), amount_scope(RULE)])
    }

    fn rich_then() -> ScenarioThen<ScenarioValue> {
        let mut t = then();
        t.attribute_values.insert(id(AMOUNT), lit(int(20)));
        t.states_active = vec![id(SUBMITTED)];
        t.outcomes = vec![id("outcome:accepted")];
        t.events = vec![id("event:submitted")];
        t.rules_satisfied = vec![id(RULE)];
        t.calculation_results.insert(id(CALC_A), lit(int(40)));
        t
    }

    const CALC_A: &str = "calculation:a";

    // ------------------------------------------------------------------ verdicts

    #[test]
    fn fully_matching_execution_passes() {
        let c = compare(&rich_world("amount > 10", rich_then()), rich_inputs());
        assert_eq!(c.result, ScenarioVerdict::Pass);
        assert!(c.diffs.is_empty());
        assert_eq!(c.scenario_run.result, ScenarioRunResult::Pass);
        assert_consistent(&c);
    }

    #[test]
    fn empty_then_passes_after_successful_execution() {
        let c = compare(&rich_world("amount > 10", then()), rich_inputs());
        assert_eq!(c.result, ScenarioVerdict::Pass);
        assert_eq!(
            comparison_steps(&c),
            vec![
                &ComparisonTraceStep::ComparisonStarted {
                    scenario_ref: id(SCN)
                },
                &ComparisonTraceStep::ComparisonCompleted {
                    result: ScenarioVerdict::Pass,
                    diff_count: 0
                },
            ]
        );
    }

    #[test]
    fn failed_execution_is_a_fail_with_one_execution_diff() {
        let mut w = rich_world("amount > 10", rich_then());
        w.scenario_then(
            given_states(&[(AMOUNT, lit(int(0)))], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
            rich_then(),
        );
        let c = compare(&w, rich_inputs());
        let failure = ExecutionFailure::PreconditionRejected {
            operation_ref: id(SUBMIT),
            index: 0,
        };
        assert_eq!(c.result, ScenarioVerdict::Fail);
        assert_eq!(
            c.diffs,
            vec![ScenarioDiff::ExecutionFailure {
                semantic_ref: id(SUBMIT),
                failure
            }]
        );
        assert_eq!(
            comparison_steps(&c),
            vec![
                &ComparisonTraceStep::ComparisonStarted {
                    scenario_ref: id(SCN)
                },
                &ComparisonTraceStep::ComparisonCompleted {
                    result: ScenarioVerdict::Fail,
                    diff_count: 1
                },
            ]
        );
        assert_eq!(c.scenario_run.result, ScenarioRunResult::Fail);
        assert_consistent(&c);
    }

    #[test]
    fn undecidable_execution_stays_undecidable_with_missing_retained() {
        let mut w = rich_world("amount > 10", rich_then());
        w.scenario_then(
            given_states(&[], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
            rich_then(),
        );
        let c = compare(&w, rich_inputs());
        assert_eq!(c.result, ScenarioVerdict::Undecidable);
        assert!(c.diffs.is_empty());
        assert!(c.trace.iter().any(|s| matches!(
            s,
            ScenarioTraceStep::Execution(ExecutionTraceStep::SemanticMissing { missing })
                if missing.iter().any(|m| m.owner_ref == id(AMOUNT))
        )));
        assert_eq!(comparison_steps(&c).len(), 2);
        assert_eq!(c.scenario_run.result, ScenarioRunResult::Undecidable);
        assert!(c.trace_artifact.body.diffs.is_empty());
        assert_consistent(&c);
    }

    // ------------------------------------------------------------------ Then SemanticRefs

    fn holiday_then_world(then: ScenarioThen<ScenarioValue>) -> World {
        let mut w = hr_world("working_days([start_date, end_date], calendar, true)", None);
        w.scenario_then(
            given(&[
                (START, sem(CAL, HOLIDAY_DATE_KEY)),
                (END, sem(CAL, HOLIDAY_DATE_KEY)),
            ]),
            ScenarioWhen::Calculation(id(CALC)),
            then,
        );
        w
    }

    fn holiday_runtime<'a>(cal: &'a StaticCalendarProvider) -> ExecutionRuntime<'a> {
        runtime(
            cal,
            &NO_MANAGERS,
            None,
            vec![RuntimeSemanticValue {
                owner_ref: id(CAL),
                key: HOLIDAY_DATE_KEY.into(),
                value: date("2026-01-06"),
            }],
        )
    }

    #[test]
    fn then_result_refs_resolve_to_the_execution_result_never_silently_undecidable() {
        // H0.10 (architecture §33.11): `result:<key>` of the named Calculation resolves to its
        // execution result in the trace (this was an Undecidable expected-semantic-missing
        // before the plan v3.2 hotfix).
        let mut t = then();
        t.calculation_results
            .insert(id(CALC), sem(CALC, "result:holiday_inside_period"));
        let w = holiday_then_world(t);
        let cal = calendars(&["2026-01-06"]);
        let (execution, c) =
            compare_with(&w, ExecutionInputs::default(), &holiday_runtime(&cal)).unwrap();
        assert!(matches!(execution.result, ExecutionResult::Executed { .. }));
        assert_eq!(c.result, ScenarioVerdict::Pass, "{:?}", c.diffs);
        let steps = comparison_steps(&c);
        assert!(
            steps.contains(&&ComparisonTraceStep::ExpectedValueResolved {
                owner_ref: id(CALC),
                key: "result:holiday_inside_period".into(),
                source: ResolutionSource::ExecutionResult,
            })
        );
        assert!(!steps
            .iter()
            .any(|s| matches!(s, ComparisonTraceStep::ExpectedSemanticMissing { .. })));
        // A Calculation the run never executed is a Fail with a typed diff.
        let mut t = then();
        t.calculation_results.insert(
            id("calculation:unused"),
            sem("calculation:unused", "result:x"),
        );
        let mut w = rich_world("amount > 10", t.clone());
        w.add(calculation(
            "calculation:unused",
            "amount * 3",
            ("Int", None),
            None,
            &[(AMOUNT, "amount")],
            None,
            true,
        ));
        w.scenario_then(
            given_states(&[(AMOUNT, lit(int(20)))], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
            t,
        );
        let c = compare(&w, rich_inputs());
        assert_eq!(c.result, ScenarioVerdict::Fail);
        assert_eq!(
            c.diffs,
            vec![ScenarioDiff::MissingExecutionResult {
                calculation_ref: id("calculation:unused"),
                key: "result:x".into(),
            }]
        );
    }

    #[test]
    fn resolved_then_semantic_refs_compare_typed() {
        let mut t = then();
        t.attribute_values
            .insert(id(START), sem(CAL, HOLIDAY_DATE_KEY));
        t.calculation_results
            .insert(id(CALC), lit(decimal("0", Some("working_day"))));
        let w = holiday_then_world(t);
        let cal = calendars(&["2026-01-06"]);
        let (_, c) = compare_with(&w, ExecutionInputs::default(), &holiday_runtime(&cal)).unwrap();
        assert_eq!(c.result, ScenarioVerdict::Pass, "{:?}", c.diffs);
        assert!(
            comparison_steps(&c).contains(&&ComparisonTraceStep::ExpectedValueResolved {
                owner_ref: id(CAL),
                key: HOLIDAY_DATE_KEY.into(),
                source: ResolutionSource::RuntimeValue,
            })
        );

        // The half-day value resolves from the Accepted example in an expectation too.
        let mut w = hr_world(HR_EXPRESSION, Some(vec![half_day_example("0.5")]));
        let mut t = then();
        t.attribute_values.insert(id(FRACTION), sem(CALC, HALF_DAY));
        t.calculation_results
            .insert(id(CALC), lit(decimal("2.50", Some("working_day"))));
        w.scenario_then(
            given(&[
                (START, lit(date("2026-01-05"))),
                (END, lit(date("2026-01-09"))),
                (INCLUSIVE, lit(boolean(true))),
                (FRACTION, sem(CALC, HALF_DAY)),
            ]),
            ScenarioWhen::Calculation(id(CALC)),
            t,
        );
        let c = compare(&w, ExecutionInputs::default());
        assert_eq!(c.result, ScenarioVerdict::Pass, "{:?}", c.diffs);
    }

    // ------------------------------------------------------------------ typed values

    fn attribute_world(
        given_values: &[(&str, ScenarioValue)],
        expected: &[(&str, ScenarioValue)],
    ) -> World {
        let mut w = request_world();
        w.add(rule(RULE, Some("amount > 0"), None));
        let mut t = then();
        for (a, v) in expected {
            t.attribute_values.insert(id(a), v.clone());
        }
        let mut g = given(given_values);
        g.attribute_values.insert(id(AMOUNT), lit(int(5)));
        w.scenario_then(g, ScenarioWhen::Rule(id(RULE)), t);
        w
    }

    fn attribute_verdict(
        given_values: &[(&str, ScenarioValue)],
        expected: &[(&str, ScenarioValue)],
    ) -> ScenarioComparison {
        compare(
            &attribute_world(given_values, expected),
            scopes(vec![amount_scope(RULE)]),
        )
    }

    #[test]
    fn attribute_values_compare_exactly_by_type() {
        let text = |v: &str| lit(ScenarioLiteral::String { value: v.into() });
        let pass = [
            (APPROVED, lit(boolean(true)), lit(boolean(true))),
            (NOTE, text("Urgent"), text("Urgent")),
            (PRIORITY, lit(enumeration("High")), lit(enumeration("High"))),
            (AMOUNT, lit(int(5)), lit(int(5))),
            (DUE, lit(date("2026-02-28")), lit(date("2026-02-28"))),
            (
                STAMP,
                lit(ScenarioLiteral::DateTime {
                    value: ts("2026-03-01T08:30:00Z"),
                }),
                lit(ScenarioLiteral::DateTime {
                    value: ts("2026-03-01T09:30:00+01:00"),
                }),
            ),
            (
                HOURS,
                lit(decimal("1.0", Some("hour"))),
                lit(decimal("1.00", Some("hour"))),
            ),
        ];
        for (attribute, actual, expected) in pass {
            let c = attribute_verdict(&[(attribute, actual)], &[(attribute, expected)]);
            assert_eq!(
                c.result,
                ScenarioVerdict::Pass,
                "{attribute}: {:?}",
                c.diffs
            );
        }
        let fail = [
            (APPROVED, lit(boolean(true)), lit(boolean(false))),
            (NOTE, text("Urgent"), text("urgent")),
            (PRIORITY, lit(enumeration("High")), lit(enumeration("Low"))),
            (DUE, lit(date("2026-02-28")), lit(date("2026-03-01"))),
            (
                STAMP,
                lit(ScenarioLiteral::DateTime {
                    value: ts("2026-03-01T08:30:00Z"),
                }),
                lit(ScenarioLiteral::DateTime {
                    value: ts("2026-03-01T08:30:00+01:00"),
                }),
            ),
            (
                HOURS,
                lit(decimal("1.0", Some("hour"))),
                lit(decimal("1.1", Some("hour"))),
            ),
        ];
        for (attribute, actual, expected) in fail {
            let c = attribute_verdict(
                &[(attribute, actual.clone())],
                &[(attribute, expected.clone())],
            );
            let (ScenarioValue::Literal(actual), ScenarioValue::Literal(expected)) =
                (actual, expected)
            else {
                unreachable!()
            };
            assert_eq!(
                c.diffs,
                vec![ScenarioDiff::ValueMismatch {
                    semantic_ref: id(attribute),
                    location: DiffLocation::AttributeValue,
                    expected,
                    actual,
                }],
                "{attribute}"
            );
            assert_eq!(c.result, ScenarioVerdict::Fail);
        }
    }

    #[test]
    fn missing_actual_attribute_is_a_diff_and_wrong_types_are_errors() {
        let c = attribute_verdict(&[], &[(APPROVED, lit(boolean(true)))]);
        assert_eq!(
            c.diffs,
            vec![ScenarioDiff::MissingActual {
                semantic_ref: id(APPROVED),
                location: DiffLocation::AttributeValue,
                expected: boolean(true),
            }]
        );
        for (attribute, wrong) in [
            (
                APPROVED,
                lit(ScenarioLiteral::String {
                    value: "true".into(),
                }),
            ),
            (AMOUNT, lit(decimal("5", None))),
            (PRIORITY, lit(enumeration("Medium"))),
            (HOURS, lit(decimal("1.0", Some("day")))),
            (HOURS, lit(decimal("1.0", None))),
        ] {
            let err = compare_err(
                &attribute_world(&[], &[(attribute, wrong)]),
                scopes(vec![amount_scope(RULE)]),
            );
            assert!(
                matches!(
                    err,
                    ComparisonError::Simulation(SimulationError::ValueTypeMismatch { .. })
                ),
                "{attribute}: {err:?}"
            );
        }
    }

    #[test]
    fn calculation_results_compare_numerically_with_exact_units() {
        let world = |expected: ScenarioLiteral| {
            let mut w = hr_world("working_days([start_date, end_date], calendar, true)", None);
            let mut t = then();
            t.calculation_results.insert(id(CALC), lit(expected));
            w.scenario_then(
                given(&[
                    (START, lit(date("2026-01-05"))),
                    (END, lit(date("2026-01-09"))),
                ]),
                ScenarioWhen::Calculation(id(CALC)),
                t,
            );
            w
        };
        for same in ["5", "5.0", "5.00"] {
            let c = compare(
                &world(decimal(same, Some("working_day"))),
                ExecutionInputs::default(),
            );
            assert_eq!(c.result, ScenarioVerdict::Pass, "{same}");
        }
        let c = compare(
            &world(decimal("4", Some("working_day"))),
            ExecutionInputs::default(),
        );
        assert_eq!(
            c.diffs,
            vec![ScenarioDiff::ValueMismatch {
                semantic_ref: id(CALC),
                location: DiffLocation::CalculationResult,
                expected: decimal("4", Some("working_day")),
                actual: decimal("5", Some("working_day")),
            }]
        );
        // A different unit, a scalar or another variant contradicts the declared result type.
        for wrong in [decimal("5", Some("day")), decimal("5", None), int(5)] {
            let err = compare_err(&world(wrong.clone()), ExecutionInputs::default());
            assert!(
                matches!(&err, ComparisonError::InvalidExpectedValue { semantic_ref, .. } if semantic_ref == &id(CALC)),
                "{wrong:?}: {err:?}"
            );
        }
    }

    #[test]
    fn calculation_results_missing_and_extra() {
        let mut t = rich_then();
        t.calculation_results.clear();
        t.calculation_results
            .insert(id("calculation:b"), lit(int(1)));
        let mut w = rich_world("amount > 10", t);
        w.add(calculation(
            "calculation:b",
            "amount + 1",
            ("Int", None),
            None,
            &[(AMOUNT, "amount")],
            None,
            true,
        ));
        let c = compare(&w, rich_inputs());
        assert_eq!(
            c.diffs,
            vec![ScenarioDiff::MissingActual {
                semantic_ref: id("calculation:b"),
                location: DiffLocation::CalculationResult,
                expected: int(1),
            }]
        );
        // calculation:a was evaluated but is not asserted: ignored.
        assert!(!comparison_steps(&c).iter().any(|s| matches!(
            s,
            ComparisonTraceStep::CalculationCompared { calculation_ref, .. } if calculation_ref == &id(CALC_A)
        )));
    }

    // ------------------------------------------------------------------ states, rules, sets

    #[test]
    fn states_are_a_positive_subset() {
        let mut t = then();
        t.states_active = vec![id(SUBMITTED)];
        let mut w = rich_world("amount > 10", t);
        let mut g = given_states(&[(AMOUNT, lit(int(20)))], &[DRAFT]);
        g.states_active.push(id("state:other"));
        w.add(state("state:other"));
        let mut t = then();
        t.states_active = vec![id(SUBMITTED)];
        w.scenario_then(g.clone(), ScenarioWhen::Operation(id(SUBMIT)), t);
        assert_eq!(compare(&w, rich_inputs()).result, ScenarioVerdict::Pass);

        let mut t = then();
        t.states_active = vec![id(CLOSED), id(SUBMITTED)];
        w.scenario_then(g, ScenarioWhen::Operation(id(SUBMIT)), t);
        assert_eq!(
            compare(&w, rich_inputs()).diffs,
            vec![ScenarioDiff::MissingExpectedState {
                state_ref: id(CLOSED)
            }]
        );
    }

    #[test]
    fn rules_satisfied_need_a_true_result() {
        let assert_rules = |condition: &str, rules: Vec<&str>, expected: Vec<ScenarioDiff>| {
            let mut t = then();
            t.rules_satisfied = rules.into_iter().map(id).collect();
            let mut w = rich_world(condition, t);
            w.add(rule("rule:other", Some("amount > 1"), None));
            assert_eq!(compare(&w, rich_inputs()).diffs, expected);
        };
        assert_rules("amount > 10", vec![RULE], vec![]);
        assert_rules(
            "amount > 100",
            vec![RULE],
            vec![ScenarioDiff::RuleNotSatisfied {
                rule_ref: id(RULE),
                actual: Some(false),
            }],
        );
        assert_rules(
            "amount > 10",
            vec!["rule:other"],
            vec![ScenarioDiff::RuleNotSatisfied {
                rule_ref: id("rule:other"),
                actual: None,
            }],
        );
    }

    #[test]
    fn outcomes_and_events_are_exact_sets_when_asserted() {
        // Absent categories assert nothing.
        let c = compare(&rich_world("amount > 10", then()), rich_inputs());
        assert_eq!(c.result, ScenarioVerdict::Pass);

        let mut w = rich_world("amount > 10", then());
        w.add(event("event:audit"))
            .link(RelationKind::Produces, SUBMIT, "event:audit");
        let check = |w: &World, events: Vec<&str>, outcomes: Vec<&str>| {
            let mut w = w.clone();
            let mut t = then();
            // S4.1 stores Then lists sorted; comparison is set-based after canonical sorting.
            let sorted = |refs: Vec<&str>| {
                let set: BTreeSet<Id> = refs.into_iter().map(id).collect();
                set.into_iter().collect::<Vec<Id>>()
            };
            t.events = sorted(events);
            t.outcomes = sorted(outcomes);
            w.scenario_then(
                given_states(&[(AMOUNT, lit(int(20)))], &[DRAFT]),
                ScenarioWhen::Operation(id(SUBMIT)),
                t,
            );
            compare(&w, rich_inputs()).diffs
        };
        assert!(check(
            &w,
            vec!["event:submitted", "event:audit"],
            vec!["outcome:accepted"]
        )
        .is_empty());
        assert_eq!(
            check(&w, vec!["event:submitted"], vec![]),
            vec![ScenarioDiff::EventSetMismatch {
                anchor_ref: id(SUBMIT),
                expected: vec![id("event:submitted")],
                actual: vec![id("event:audit"), id("event:submitted")],
                missing: vec![],
                unexpected: vec![id("event:audit")],
            }]
        );
        w.add(outcome("outcome:other"));
        assert_eq!(
            check(&w, vec![], vec!["outcome:accepted", "outcome:other"]),
            vec![ScenarioDiff::OutcomeSetMismatch {
                anchor_ref: id(SUBMIT),
                expected: vec![id("outcome:accepted"), id("outcome:other")],
                actual: vec![id("outcome:accepted")],
                missing: vec![id("outcome:other")],
                unexpected: vec![],
            }]
        );
    }

    // ------------------------------------------------------------------ decision outputs

    fn output_world(expected: &[(u32, ScenarioLiteral)]) -> World {
        let mut w = World::empty();
        w.add(node(
            TABLE,
            Accepted,
            NodePayload::DecisionTable(DecisionTable {
                name: "Eligibility".into(),
                hit_policy: "FIRST".into(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                rows: Vec::new(),
            }),
        ));
        let mut g = ScenarioGiven::default();
        g.decision_inputs.insert(0, lit(boolean(true)));
        let mut t = then();
        for (i, v) in expected {
            t.decision_outputs.insert(*i, lit(v.clone()));
        }
        w.scenario_then(g, ScenarioWhen::DecisionTable(id(TABLE)), t);
        w
    }

    fn output_inputs() -> ExecutionInputs {
        let column = |name: &str, ty: DecisionType| DecisionColumn {
            name: name.into(),
            ty,
            domain: None,
        };
        ExecutionInputs {
            decision_tables: vec![ScenarioDecisionTableInput {
                decision_table_ref: id(TABLE),
                spec: DecisionTableSpec {
                    hit_policy: "FIRST".into(),
                    inputs: vec![column("contractor", DecisionType::Bool)],
                    outputs: vec![
                        column("code", DecisionType::Int),
                        column("rate", DecisionType::Decimal { scale: 2 }),
                    ],
                    rows: vec![DecisionRow {
                        inputs: vec![DecisionInputCell::Any],
                        outputs: vec![
                            DecisionOutputCell::Int { value: 7 },
                            DecisionOutputCell::Decimal {
                                value: "0.50".parse().unwrap(),
                            },
                        ],
                    }],
                    default_output: None,
                },
            }],
            ..ExecutionInputs::default()
        }
    }

    #[test]
    fn decision_outputs_compare_named_columns_only() {
        let c = compare(&output_world(&[(0, int(7))]), output_inputs());
        assert_eq!(c.result, ScenarioVerdict::Pass);
        let c = compare(&output_world(&[(1, decimal("0.5", None))]), output_inputs());
        assert_eq!(c.result, ScenarioVerdict::Pass);
        let c = compare(&output_world(&[(0, int(8))]), output_inputs());
        assert_eq!(
            c.diffs,
            vec![ScenarioDiff::ValueMismatch {
                semantic_ref: id(TABLE),
                location: DiffLocation::DecisionOutput {
                    decision_table_ref: id(TABLE),
                    column: 0
                },
                expected: int(8),
                actual: int(7),
            }]
        );
        assert!(matches!(
            compare_err(&output_world(&[(2, int(1))]), output_inputs()),
            ComparisonError::InvalidExpectedValue { .. }
        ));
        assert!(matches!(
            compare_err(&output_world(&[(0, boolean(true))]), output_inputs()),
            ComparisonError::InvalidExpectedValue { .. }
        ));
        // A rule anchor has no decision-table source.
        let mut w = attribute_world(&[], &[]);
        let mut t = then();
        t.decision_outputs.insert(0, lit(int(7)));
        let mut g = ScenarioGiven::default();
        g.attribute_values.insert(id(AMOUNT), lit(int(5)));
        w.scenario_then(g, ScenarioWhen::Rule(id(RULE)), t);
        assert!(matches!(
            compare_err(&w, scopes(vec![amount_scope(RULE)])),
            ComparisonError::InvalidExpectedValue { .. }
        ));
    }

    // ------------------------------------------------------------------ multi-diff golden

    #[test]
    fn multiple_diffs_have_the_canonical_order() {
        let mut t = then();
        t.attribute_values.insert(id(AMOUNT), lit(int(21)));
        t.states_active = vec![id(CLOSED)];
        t.events = vec![id("event:other")];
        t.rules_satisfied = vec![id(RULE)];
        t.calculation_results.insert(id(CALC_A), lit(int(41)));
        let mut w = rich_world("amount > 100", t);
        w.add(event("event:other"));
        let c = compare(&w, rich_inputs());
        assert_eq!(c.result, ScenarioVerdict::Fail);
        assert_eq!(
            serde_json::to_value(&c.diffs).unwrap(),
            json!([
                {"diff": "value_mismatch", "semantic_ref": AMOUNT, "location": {"location": "attribute_value"},
                 "expected": {"kind": "int", "value": 21}, "actual": {"kind": "int", "value": 20}},
                {"diff": "missing_expected_state", "state_ref": CLOSED},
                {"diff": "rule_not_satisfied", "rule_ref": RULE, "actual": false},
                {"diff": "value_mismatch", "semantic_ref": CALC_A, "location": {"location": "calculation_result"},
                 "expected": {"kind": "int", "value": 41}, "actual": {"kind": "int", "value": 40}},
                {"diff": "event_set_mismatch", "anchor_ref": SUBMIT, "expected": ["event:other"],
                 "actual": ["event:submitted"], "missing": ["event:other"], "unexpected": ["event:submitted"]},
            ])
        );
        assert_consistent(&c);
    }

    // ------------------------------------------------------------------ trace and ScenarioRun

    #[test]
    fn trace_preserves_execution_and_follows_then_order() {
        let w = rich_world("amount > 10", rich_then());
        let cal = calendars(&[]);
        let (execution, c) = compare_with(
            &w,
            rich_inputs(),
            &runtime(&cal, &NO_MANAGERS, None, Vec::new()),
        )
        .unwrap();
        let ExecutionResult::Executed { trace, .. } = &execution.result else {
            panic!("expected Executed");
        };
        let n = trace.len();
        let executed: Vec<ScenarioTraceStep> = trace
            .iter()
            .cloned()
            .map(ScenarioTraceStep::Execution)
            .collect();
        assert_eq!(&c.trace[..n], executed.as_slice());
        let names: Vec<String> = c.trace[n..]
            .iter()
            .map(|s| {
                serde_json::to_value(s).unwrap()["step"]["step"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "comparison_started",
                "attribute_compared",
                "state_compared",
                "outcomes_compared",
                "events_compared",
                "rule_compared",
                "calculation_compared",
                "comparison_completed",
            ]
        );
        let wire = serde_json::to_value(&c.trace[n]).unwrap();
        assert_eq!(
            wire,
            json!({"phase": "comparison", "step": {"step": "comparison_started", "scenario_ref": SCN}})
        );
        assert_eq!(
            serde_json::to_value(&c.trace[0]).unwrap(),
            json!({"phase": "execution", "step": {"step": "materialization_started", "scenario_ref": SCN}})
        );
    }

    #[test]
    fn scenario_run_payload_is_exact_and_round_trips() {
        let c = compare(&rich_world("amount > 10", rich_then()), rich_inputs());
        let value = serde_json::to_value(&c.scenario_run).unwrap();
        let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 4);
        for key in ["scenario_ref", "specification_hash", "result", "trace"] {
            assert!(value.get(key).is_some(), "{key}");
        }
        assert_eq!(value["result"], "pass");
        let back: ScenarioRun = serde_json::from_value(value).unwrap();
        assert_eq!(back, c.scenario_run);
        assert_eq!(c.scenario_run.specification_hash, c.specification_hash);
        assert_eq!(c.scenario_run.trace.len(), c.trace.len());
        // Evidence material only: the graph and its semantic hash are unchanged.
        let w = rich_world("amount > 10", rich_then());
        let graph = w.graph();
        assert_eq!(
            c.trace_artifact.body.model_semantic_hash,
            plumb_psg::semantic_hash(&graph).unwrap()
        );
    }

    fn synthetic_body() -> ScenarioTraceArtifactBody {
        let working_days = |v: &str| decimal(v, Some("working_day"));
        ScenarioTraceArtifactBody {
            version: SCENARIO_TRACE_VERSION,
            model_semantic_hash: Hash::content_sha256(b"model"),
            scenario_ref: id(SCN),
            specification_hash: Hash::content_sha256(b"specification"),
            result: ScenarioRunResult::Fail,
            trace: vec![
                ScenarioTraceStep::Execution(ExecutionTraceStep::MaterializationStarted {
                    scenario_ref: id(SCN),
                }),
                ScenarioTraceStep::Execution(ExecutionTraceStep::CalculationEvaluated {
                    calculation_ref: id(CALC),
                    value: working_days("2.5"),
                }),
                ScenarioTraceStep::Execution(ExecutionTraceStep::ExecutionCompleted),
                ScenarioTraceStep::Comparison(ComparisonTraceStep::ComparisonStarted {
                    scenario_ref: id(SCN),
                }),
                ScenarioTraceStep::Comparison(ComparisonTraceStep::CalculationCompared {
                    calculation_ref: id(CALC),
                    matched: false,
                }),
                ScenarioTraceStep::Comparison(ComparisonTraceStep::ComparisonCompleted {
                    result: ScenarioVerdict::Fail,
                    diff_count: 1,
                }),
            ],
            diffs: vec![ScenarioDiff::ValueMismatch {
                semantic_ref: id(CALC),
                location: DiffLocation::CalculationResult,
                expected: working_days("3"),
                actual: working_days("2.5"),
            }],
        }
    }

    // Independently computed in Python (hashlib + RFC 8785 canonical JSON of the hand-written
    // artifact body).
    const GOLDEN_ARTIFACT_HASH: &str =
        "sha256:630edddab7019e630b4c87592de9508d6f4f0751a6603f6bc202bf8cf84606f5";

    #[test]
    fn artifact_bytes_and_hash_match_the_independent_golden() {
        let artifact = ScenarioTraceArtifact::from_body(synthetic_body()).unwrap();
        assert_eq!(artifact.hash.to_string(), GOLDEN_ARTIFACT_HASH);
        assert_eq!(artifact.hash, Hash::content_sha256(&artifact.bytes));
        let text = String::from_utf8(artifact.bytes.clone()).unwrap();
        assert!(text.starts_with("{\"diffs\":[{\"actual\":"));
        assert!(!text.contains(": ") && !text.contains('\n'));
        let run = plumb_sim::scenario_run(&artifact.body).unwrap();
        assert_eq!(run.result, ScenarioRunResult::Fail);
        assert_eq!(run.trace.len(), 6);
    }

    #[test]
    fn stale_execution_runs_are_rejected() {
        let cal = calendars(&[]);
        let rt = runtime(&cal, &NO_MANAGERS, None, Vec::new());
        let w = rich_world("amount > 10", rich_then());
        let graph = w.graph();
        let model = build_execution_model(&graph, rich_inputs()).unwrap();
        let execution = run(&model, &id(SCN), &rt).unwrap();

        let mut changed = w.clone();
        let mut t = rich_then();
        t.events.clear();
        changed.scenario_then(
            given_states(&[(AMOUNT, lit(int(20)))], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
            t,
        );
        let changed_graph = changed.graph();
        let changed_model = build_execution_model(&changed_graph, rich_inputs()).unwrap();
        assert!(matches!(
            compare_run(&changed_model, &execution, &rt).unwrap_err(),
            ComparisonError::StaleExecutionRun { .. }
        ));
        let mut forged = execution.clone();
        forged.specification_hash = Hash::content_sha256(b"other");
        assert!(matches!(
            compare_run(&model, &forged, &rt).unwrap_err(),
            ComparisonError::StaleExecutionRun { .. }
        ));
        assert!(compare_run(&model, &execution, &rt).is_ok());
    }

    #[test]
    fn comparison_is_deterministic_under_reverse_insertion() {
        let w = rich_world("amount > 100", rich_then());
        let cal = calendars(&[]);
        let rt = runtime(&cal, &NO_MANAGERS, None, Vec::new());
        let forward_graph = w.graph();
        let reverse_graph = w.reversed_graph();
        let mut reversed_inputs = rich_inputs();
        reversed_inputs.expression_scopes.reverse();
        let forward_model = build_execution_model(&forward_graph, rich_inputs()).unwrap();
        let reverse_model = build_execution_model(&reverse_graph, reversed_inputs).unwrap();
        let forward = compare_run(
            &forward_model,
            &run(&forward_model, &id(SCN), &rt).unwrap(),
            &rt,
        )
        .unwrap();
        let reverse = compare_run(
            &reverse_model,
            &run(&reverse_model, &id(SCN), &rt).unwrap(),
            &rt,
        )
        .unwrap();
        assert_eq!(forward, reverse);
        assert_eq!(forward.trace_artifact.bytes, reverse.trace_artifact.bytes);
        assert_eq!(forward.trace_artifact.hash, reverse.trace_artifact.hash);
        assert_eq!(forward.result, ScenarioVerdict::Fail);
        let again = compare_run(
            &forward_model,
            &run(&forward_model, &id(SCN), &rt).unwrap(),
            &rt,
        )
        .unwrap();
        assert_eq!(again.trace_artifact.bytes, forward.trace_artifact.bytes);
    }

    // ------------------------------------------------------------------ regressions

    #[test]
    fn expected_writes_are_not_synthesized() {
        let mut w = submit_world(&[], &[], None);
        w.link(RelationKind::Writes, SUBMIT, APPROVED);
        let mut t = then();
        t.attribute_values.insert(id(APPROVED), lit(boolean(true)));
        w.scenario_then(
            given_states(&[], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
            t.clone(),
        );
        let c = compare(&w, ExecutionInputs::default());
        assert_eq!(
            c.diffs,
            vec![ScenarioDiff::MissingActual {
                semantic_ref: id(APPROVED),
                location: DiffLocation::AttributeValue,
                expected: boolean(true),
            }]
        );
        w.scenario_then(
            given_states(&[(APPROVED, lit(boolean(false)))], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
            t,
        );
        let c = compare(&w, ExecutionInputs::default());
        assert!(matches!(
            c.diffs.as_slice(),
            [ScenarioDiff::ValueMismatch { .. }]
        ));
        assert_eq!(c.result, ScenarioVerdict::Fail);
    }

    fn hr_result_then() -> ScenarioThen<ScenarioValue> {
        let mut t = then();
        t.calculation_results
            .insert(id(CALC), sem(CALC, "result:boundary"));
        t
    }

    #[test]
    fn hr_pre_answer_scenarios_stay_undecidable() {
        let mut half_day = hr_world(HR_EXPRESSION, None);
        half_day.scenario_then(
            given(&[(FRACTION, sem(CALC, HALF_DAY))]),
            ScenarioWhen::Calculation(id(CALC)),
            hr_result_then(),
        );
        let holiday = {
            let mut w = hr_world(HR_EXPRESSION, None);
            w.scenario_then(
                given(&[
                    (START, sem(CAL, HOLIDAY_DATE_KEY)),
                    (END, sem(CAL, HOLIDAY_DATE_KEY)),
                ]),
                ScenarioWhen::Calculation(id(CALC)),
                hr_result_then(),
            );
            w
        };
        let mut inclusive = hr_world(
            "working_days([start_date, end_date], calendar, inclusive_end)",
            None,
        );
        let mut g = given(&[
            (START, lit(date("2026-01-05"))),
            (END, lit(date("2026-01-09"))),
        ]);
        g.calculation_semantics.insert(
            id(CALC),
            BTreeMap::from([(
                WORKING_DAYS_INCLUSIVE_KEY.to_owned(),
                sem(CALC, WORKING_DAYS_INCLUSIVE_KEY),
            )]),
        );
        inclusive.scenario_then(g, ScenarioWhen::Calculation(id(CALC)), hr_result_then());
        for (w, key) in [
            (half_day, HALF_DAY),
            (holiday, HOLIDAY_DATE_KEY),
            (inclusive, WORKING_DAYS_INCLUSIVE_KEY),
        ] {
            let c = compare(&w, ExecutionInputs::default());
            assert_eq!(c.result, ScenarioVerdict::Undecidable, "{key}");
            assert_eq!(c.scenario_run.result, ScenarioRunResult::Undecidable);
            assert!(c.diffs.is_empty());
            assert!(c.trace.iter().any(|s| matches!(
                s,
                ScenarioTraceStep::Execution(ExecutionTraceStep::SemanticMissing { missing })
                    if missing.iter().any(|m| m.key == key)
            )));
        }
    }

    #[test]
    fn hr_post_answer_scenarios_pass_where_represented() {
        let expect = |value: &str| {
            let mut t = then();
            t.calculation_results
                .insert(id(CALC), lit(decimal(value, Some("working_day"))));
            t
        };
        // Half-day: the accepted example resolves the fraction.
        let mut half_day = hr_world(HR_EXPRESSION, Some(vec![half_day_example("0.5")]));
        half_day.scenario_then(
            given(&[
                (START, lit(date("2026-01-05"))),
                (END, lit(date("2026-01-09"))),
                (INCLUSIVE, lit(boolean(true))),
                (FRACTION, sem(CALC, HALF_DAY)),
            ]),
            ScenarioWhen::Calculation(id(CALC)),
            expect("2.5"),
        );
        assert_eq!(
            compare(&half_day, ExecutionInputs::default()).result,
            ScenarioVerdict::Pass
        );
        // Inclusive end: the literal working_days argument resolves it.
        let mut inclusive = hr_world(
            "working_days([start_date, end_date], calendar, false)",
            None,
        );
        let mut g = given(&[
            (START, lit(date("2026-01-05"))),
            (END, lit(date("2026-01-09"))),
        ]);
        g.calculation_semantics.insert(
            id(CALC),
            BTreeMap::from([(
                WORKING_DAYS_INCLUSIVE_KEY.to_owned(),
                sem(CALC, WORKING_DAYS_INCLUSIVE_KEY),
            )]),
        );
        inclusive.scenario_then(g, ScenarioWhen::Calculation(id(CALC)), expect("4"));
        assert_eq!(
            compare(&inclusive, ExecutionInputs::default()).result,
            ScenarioVerdict::Pass
        );
        // Holiday: a runtime holiday date and an injected calendar that excludes it.
        let w = holiday_then_world(expect("0"));
        let cal = calendars(&["2026-01-06"]);
        let (_, c) = compare_with(&w, ExecutionInputs::default(), &holiday_runtime(&cal)).unwrap();
        assert_eq!(c.result, ScenarioVerdict::Pass);
    }

    // ------------------------------------------------------------------ source guards

    #[test]
    fn comparison_sources_are_pure_and_never_execute() {
        let forbidden = [
            "std::fs",
            "File::open",
            "rusqlite",
            "GraphStore",
            "ArtifactStore",
            "BlobStore",
            "reqwest",
            "InferenceProvider",
            "Clock",
            "Utc::now",
            "Instant::now",
            "SystemTime::now",
            "OffsetDateTime::now",
            "apply_patch",
            "SemanticPatch",
            "Proposal",
            "commit",
            "branch_head",
            "unsafe",
            "std::env",
            "Command",
            "HashMap",
            "thread_local",
            "static mut",
            "evaluate_calculation",
            "authorization_facts",
            "effective_security_roles",
            "eval(",
            "crate::execute",
            "bind_values",
            "Node {",
            "Graph::new",
            "timestamp",
            "half_day_fraction",
            "inclusive_end",
            "holiday_date",
        ];
        for (name, source) in [("compare.rs", COMPARE_SRC), ("trace.rs", TRACE_SRC)] {
            for token in forbidden {
                assert!(!source.contains(token), "{name} contains {token}");
            }
        }
        assert!(COMPARE_SRC.contains("materialize(model, scenario_ref, &probe, &values)"));
        assert!(COMPARE_SRC.contains("admit(model, scenario_ref)"));
    }

    // ------------------------------------------------------------------ stable run identity (Hotfix 065)

    #[test]
    fn scenario_run_identity_is_stable_per_project_and_scenario() {
        // Independent Python SHA-256 of RFC 8785 {node_type, project_id, scenario_ref}.
        let cases = [
            (
                "project:leave-management",
                "scenario:submit",
                "scenario_run:ac09d929fb3f9a47",
            ),
            (
                "project:leave-management",
                "scenario:other",
                "scenario_run:d12beb5a4947795e",
            ),
            (
                "project:other",
                "scenario:submit",
                "scenario_run:9a0d756f8628293a",
            ),
        ];
        for (project, scenario, expected) in cases {
            let a = plumb_sim::scenario_run_id(&id(project), &id(scenario)).unwrap();
            assert_eq!(a.as_str(), expected);
            assert_eq!(
                plumb_sim::scenario_run_id(&id(project), &id(scenario)).unwrap(),
                a
            );
        }
        assert_eq!(
            plumb_sim::SCENARIO_TRACE_LINK_EXTENSION,
            "plumb_sim:scenario_trace_artifact"
        );
        assert!(plumb_sim::SCENARIO_TRACE_LINK_EXTENSION
            .parse::<plumb_psg::ExtensionKey>()
            .is_ok());
    }

    // ------------------------------------------------------------------ H0.4 business failures

    // HR-shaped leave graph (architecture §33.4 regression shapes; built in memory).
    const LEAVE_REQUEST: &str = "entity:leave-request";
    const BALANCE: &str = "attr:annual-balance";
    const REQUESTED: &str = "attr:requested-days";
    const L_SUBMITTED: &str = "state:leave-submitted";
    const L_APPROVED: &str = "state:leave-approved";
    const L_WITHDRAWN: &str = "state:leave-withdrawn";
    const CHECK_BALANCE: &str = "op:check-leave-balance";
    const APPROVE: &str = "op:approve-leave-request";
    const WITHDRAW: &str = "op:withdraw-leave-request";
    const TR_APPROVE: &str = "transition:approve";
    const TR_WITHDRAW: &str = "transition:withdraw";
    const MANAGER_ROLE: &str = "secrole:manager";
    const APPROVE_PERMISSION: &str = "permission:approve-leave";
    const MANAGER: &str = "principal:m200";
    const EMPLOYEE: &str = "principal:e100";
    const INSUFFICIENT: &str = "outcome:insufficient-balance";
    const NOT_AUTHORIZED: &str = "outcome:not-authorized";
    const NOT_WITHDRAWABLE: &str = "outcome:not-withdrawable";
    const BALANCE_OK: &str = "outcome:balance-ok";
    const OUTCOME_SELECTION: &str = "plumb_sim:outcome_selection";

    fn outcome_of(node_id: &str, kind: OutcomeKind, selection: Option<Value>) -> Node {
        let mut n = node(
            node_id,
            Accepted,
            NodePayload::Outcome(Outcome {
                name: node_id.into(),
                outcome_kind: kind,
            }),
        );
        if let Some(selection) = selection {
            n.extensions.insert(
                ExtensionKey::from_str(OUTCOME_SELECTION).unwrap(),
                selection,
            );
        }
        n
    }

    fn on_failures(operation_ref: &str, kinds: &[&str]) -> Value {
        json!({"version": 1, "selections": [{"operation_ref": operation_ref, "on_failures": kinds}]})
    }

    /// The leave request with its balance attributes, lifecycle, the balance check, the
    /// manager-only approval and the withdrawal, each with its modeled business failure Outcome.
    fn hr_leave_world() -> World {
        let mut w = World::empty();
        w.add(entity(LEAVE_REQUEST, "LeaveRequest"))
            .add(attribute(
                BALANCE,
                "annual_balance",
                "Decimal(1)",
                None,
                None,
            ))
            .add(attribute(
                REQUESTED,
                "requested_days",
                "Decimal(1)",
                None,
                None,
            ))
            .add(state(L_SUBMITTED))
            .add(state(L_APPROVED))
            .add(state(L_WITHDRAWN))
            .add(operation(
                CHECK_BALANCE,
                &["requested_days <= annual_balance"],
                &[],
            ))
            .add(operation(APPROVE, &[], &[]))
            .add(operation(WITHDRAW, &[], &[]))
            .add(node(
                TR_APPROVE,
                Accepted,
                NodePayload::Transition(Transition {
                    stateful_ref: id(LEAVE_REQUEST),
                    from_state: id(L_SUBMITTED),
                    to_state: id(L_APPROVED),
                    guard_expr: None,
                    effect_refs: None,
                }),
            ))
            .add(node(
                TR_WITHDRAW,
                Accepted,
                NodePayload::Transition(Transition {
                    stateful_ref: id(LEAVE_REQUEST),
                    from_state: id(L_SUBMITTED),
                    to_state: id(L_WITHDRAWN),
                    guard_expr: None,
                    effect_refs: None,
                }),
            ))
            .add(node(
                MANAGER_ROLE,
                Accepted,
                NodePayload::SecurityRole(plumb_psg::SecurityRole {
                    name: "Manager".into(),
                    description: None,
                }),
            ))
            .add(node(
                APPROVE_PERMISSION,
                Accepted,
                NodePayload::Permission(plumb_psg::Permission {
                    name: "ApproveLeave".into(),
                }),
            ))
            .add(outcome_of(
                INSUFFICIENT,
                OutcomeKind::BusinessFailure,
                Some(on_failures(CHECK_BALANCE, &["precondition_rejected"])),
            ))
            .add(outcome_of(BALANCE_OK, OutcomeKind::Success, None))
            .add(node(
                "scope:leave-requests",
                Accepted,
                NodePayload::ResourceScope(plumb_psg::ResourceScope {
                    resource_ref: id(LEAVE_REQUEST),
                    scope_kind: "all".into(),
                }),
            ))
            .add(outcome_of(
                NOT_AUTHORIZED,
                OutcomeKind::BusinessFailure,
                Some(on_failures(APPROVE, &["authorization_denied"])),
            ))
            .add(outcome_of(
                NOT_WITHDRAWABLE,
                OutcomeKind::BusinessFailure,
                Some(on_failures(WITHDRAW, &["invalid_source_state"])),
            ))
            // Technical failures are never selected, whatever they would claim.
            .add(outcome_of(
                "outcome:storage-down",
                OutcomeKind::TechnicalFailure,
                None,
            ));
        for p in [MANAGER, EMPLOYEE] {
            w.add(node(
                p,
                Accepted,
                NodePayload::Principal(plumb_psg::Principal {
                    name: p.into(),
                    principal_kind: "user".into(),
                }),
            ));
        }
        for a in [BALANCE, REQUESTED] {
            w.link(RelationKind::HasAttribute, LEAVE_REQUEST, a);
        }
        for s in [L_SUBMITTED, L_APPROVED, L_WITHDRAWN] {
            w.link(RelationKind::HasState, LEAVE_REQUEST, s);
        }
        w.link(RelationKind::TransitionsVia, TR_APPROVE, APPROVE)
            .link(RelationKind::TransitionsVia, TR_WITHDRAW, WITHDRAW)
            .link(RelationKind::Grants, MANAGER_ROLE, APPROVE_PERMISSION)
            .link(RelationKind::Permits, APPROVE_PERMISSION, APPROVE)
            .link(
                RelationKind::ScopedTo,
                APPROVE_PERMISSION,
                "scope:leave-requests",
            )
            .link(RelationKind::AssignedRole, MANAGER, MANAGER_ROLE)
            .link(RelationKind::Produces, CHECK_BALANCE, INSUFFICIENT)
            .link(RelationKind::Produces, CHECK_BALANCE, BALANCE_OK)
            .link(
                RelationKind::Produces,
                CHECK_BALANCE,
                "outcome:storage-down",
            )
            .link(RelationKind::Produces, APPROVE, NOT_AUTHORIZED)
            .link(RelationKind::Produces, WITHDRAW, NOT_WITHDRAWABLE)
            .link(RelationKind::Produces, APPROVE, "event:leave-approved")
            .add(event("event:leave-approved"));
        w
    }

    fn balance_scope() -> ExecutionExpressionScopeInput {
        scope(
            CHECK_BALANCE,
            &[(BALANCE, "annual_balance"), (REQUESTED, "requested_days")],
        )
    }

    fn hr_compare(
        w: &World,
        actor: Option<&str>,
    ) -> Result<(ExecutionRun, ScenarioComparison), ComparisonError> {
        let cal = calendars(&[]);
        compare_with(
            w,
            scopes(vec![balance_scope()]),
            &runtime(&cal, &NO_MANAGERS, actor, Vec::new()),
        )
    }

    fn expect_outcome(outcome_ref: &str, states: &[&str]) -> ScenarioThen<ScenarioValue> {
        let mut t = then();
        t.outcomes = vec![id(outcome_ref)];
        t.states_active = states.iter().map(|s| id(s)).collect();
        t
    }

    fn balance_given(balance: &str, requested: &str) -> ScenarioGiven<ScenarioValue> {
        given_states(
            &[
                (BALANCE, lit(decimal(balance, None))),
                (REQUESTED, lit(decimal(requested, None))),
            ],
            &[L_SUBMITTED],
        )
    }

    #[test]
    fn expected_business_failure_passes() {
        // SCN-010 shape: insufficient balance (precondition_rejected).
        let mut w = hr_leave_world();
        w.scenario_then(
            balance_given("1.0", "2.0"),
            ScenarioWhen::Operation(id(CHECK_BALANCE)),
            expect_outcome(INSUFFICIENT, &[L_SUBMITTED]),
        );
        let (execution, c) = hr_compare(&w, None).unwrap();
        let ExecutionResult::Rejected {
            failure,
            outcome_ref,
            state,
            ..
        } = &execution.result
        else {
            panic!("expected Rejected, got {:?}", execution.result);
        };
        assert_eq!(
            failure,
            &ExecutionFailure::PreconditionRejected {
                operation_ref: id(CHECK_BALANCE),
                index: 0
            }
        );
        assert_eq!(outcome_ref, &id(INSUFFICIENT));
        // The pre-state is kept and no event is produced.
        assert_eq!(state.active_states, BTreeSet::from([id(L_SUBMITTED)]));
        assert_eq!(c.result, ScenarioVerdict::Pass, "{:?}", c.diffs);
        assert!(c.trace.contains(&ScenarioTraceStep::Execution(
            ExecutionTraceStep::OutcomeSelected {
                operation_ref: id(CHECK_BALANCE),
                outcome_ref: id(INSUFFICIENT),
            }
        )));
        assert_consistent(&c);

        // SCN-013 shape: a non-manager approval (authorization_denied); the state stays
        // Submitted and no LeaveApproved event is produced.
        let mut w = hr_leave_world();
        let mut t = expect_outcome(NOT_AUTHORIZED, &[L_SUBMITTED]);
        w.scenario_then(
            given_states(&[], &[L_SUBMITTED]),
            ScenarioWhen::Operation(id(APPROVE)),
            t.clone(),
        );
        let (_, c) = hr_compare(&w, Some(EMPLOYEE)).unwrap();
        assert_eq!(c.result, ScenarioVerdict::Pass, "{:?}", c.diffs);
        // The manager approves: the business failure expectation now fails.
        let (execution, c) = hr_compare(&w, Some(MANAGER)).unwrap();
        assert!(matches!(execution.result, ExecutionResult::Executed { .. }));
        assert_eq!(c.result, ScenarioVerdict::Fail);
        // A rejection never produces events.
        t.events = vec![id("event:leave-approved")];
        w.scenario_then(
            given_states(&[], &[L_SUBMITTED]),
            ScenarioWhen::Operation(id(APPROVE)),
            t,
        );
        let (_, c) = hr_compare(&w, Some(EMPLOYEE)).unwrap();
        assert_eq!(c.result, ScenarioVerdict::Fail);
        assert!(c
            .diffs
            .iter()
            .any(|d| matches!(d, ScenarioDiff::EventSetMismatch { .. })));

        // SCN-022 shape: withdrawing an Approved request (invalid_source_state of the
        // transition its Operation triggers).
        let mut w = hr_leave_world();
        w.scenario_then(
            given_states(&[], &[L_APPROVED]),
            ScenarioWhen::Transition(id(TR_WITHDRAW)),
            expect_outcome(NOT_WITHDRAWABLE, &[L_APPROVED]),
        );
        let (execution, c) = hr_compare(&w, None).unwrap();
        assert!(matches!(
            &execution.result,
            ExecutionResult::Rejected {
                failure: ExecutionFailure::InvalidSourceState { .. },
                ..
            }
        ));
        assert_eq!(c.result, ScenarioVerdict::Pass, "{:?}", c.diffs);
    }

    #[test]
    fn expected_business_failure_that_executes_successfully_fails() {
        let mut w = hr_leave_world();
        w.scenario_then(
            balance_given("5.0", "2.0"),
            ScenarioWhen::Operation(id(CHECK_BALANCE)),
            expect_outcome(INSUFFICIENT, &[]),
        );
        let (execution, c) = hr_compare(&w, None).unwrap();
        // The only success candidate is selected; the business failure never is.
        let ExecutionResult::Executed { outcome_refs, .. } = &execution.result else {
            panic!("expected Executed, got {:?}", execution.result);
        };
        assert_eq!(outcome_refs, &[id(BALANCE_OK)]);
        assert_eq!(c.result, ScenarioVerdict::Fail);
        assert_eq!(
            c.diffs,
            vec![ScenarioDiff::OutcomeSetMismatch {
                anchor_ref: id(CHECK_BALANCE),
                expected: vec![id(INSUFFICIENT)],
                actual: vec![id(BALANCE_OK)],
                missing: vec![id(INSUFFICIENT)],
                unexpected: vec![id(BALANCE_OK)],
            }]
        );
    }

    #[test]
    fn unexpected_rejections_and_model_failures_never_pass() {
        // No business_failure Outcome lists the kind: Fail with the execution-failure diff.
        let mut w = hr_leave_world();
        w.add(outcome_of(
            INSUFFICIENT,
            OutcomeKind::BusinessFailure,
            Some(on_failures(CHECK_BALANCE, &["authorization_denied"])),
        ));
        w.scenario_then(
            balance_given("1.0", "2.0"),
            ScenarioWhen::Operation(id(CHECK_BALANCE)),
            expect_outcome(INSUFFICIENT, &[]),
        );
        let (execution, c) = hr_compare(&w, None).unwrap();
        assert!(matches!(execution.result, ExecutionResult::Failed { .. }));
        assert_eq!(c.result, ScenarioVerdict::Fail);
        assert!(matches!(
            c.diffs.as_slice(),
            [ScenarioDiff::ExecutionFailure { .. }]
        ));
        // Two business_failure Outcomes listing the kind: Undecidable OUTCOME_SELECTION.
        let mut w = hr_leave_world();
        w.add(outcome_of(
            "outcome:also-insufficient",
            OutcomeKind::BusinessFailure,
            Some(on_failures(CHECK_BALANCE, &["precondition_rejected"])),
        ))
        .link(
            RelationKind::Produces,
            CHECK_BALANCE,
            "outcome:also-insufficient",
        );
        w.scenario_then(
            balance_given("1.0", "2.0"),
            ScenarioWhen::Operation(id(CHECK_BALANCE)),
            expect_outcome(INSUFFICIENT, &[]),
        );
        let (execution, c) = hr_compare(&w, None).unwrap();
        let ExecutionResult::Undecidable { missing, .. } = &execution.result else {
            panic!("expected Undecidable, got {:?}", execution.result);
        };
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].owner_ref, id(CHECK_BALANCE));
        assert_eq!(missing[0].key, "outcome_selection");
        assert_eq!(missing[0].kind, MissingSemanticKind::OutcomeSelection);
        assert_eq!(c.result, ScenarioVerdict::Undecidable);
        // A model failure (postcondition_violation) stays Fail whatever the Scenario expects.
        let mut w = hr_leave_world();
        w.add(operation(
            CHECK_BALANCE,
            &["requested_days <= annual_balance"],
            &["requested_days > annual_balance"],
        ));
        w.scenario_then(
            balance_given("5.0", "2.0"),
            ScenarioWhen::Operation(id(CHECK_BALANCE)),
            expect_outcome(INSUFFICIENT, &[]),
        );
        let (execution, c) = hr_compare(&w, None).unwrap();
        assert!(matches!(
            execution.result,
            ExecutionResult::Failed {
                failure: ExecutionFailure::PostconditionViolation { .. },
                ..
            }
        ));
        assert_eq!(c.result, ScenarioVerdict::Fail);
    }

    #[test]
    fn internal_error_never_passes() {
        // A simulator error is no verdict, never a Pass, even when the Scenario expects the
        // business failure Outcome.
        let mut w = hr_leave_world();
        w.scenario_then(
            given_states(&[], &[L_SUBMITTED]),
            ScenarioWhen::Operation(id(APPROVE)),
            expect_outcome(NOT_AUTHORIZED, &[L_SUBMITTED]),
        );
        let err = hr_compare(&w, Some("principal:unknown")).unwrap_err();
        assert!(matches!(
            err,
            ComparisonError::Simulation(SimulationError::InvalidRuntimeActor { .. })
        ));
        let cal = calendars(&[]);
        let disallowed = runtime(
            &cal,
            &NO_MANAGERS,
            Some(EMPLOYEE),
            vec![RuntimeSemanticValue {
                owner_ref: id(BALANCE),
                key: "value".into(),
                value: decimal("9.0", None),
            }],
        );
        let err = compare_with(&w, scopes(vec![balance_scope()]), &disallowed).unwrap_err();
        assert!(matches!(
            err,
            ComparisonError::Simulation(SimulationError::DisallowedRuntimeValue { .. })
        ));
        // A stale execution never yields a verdict either.
        let graph = w.graph();
        let model = build_execution_model(&graph, scopes(vec![balance_scope()])).unwrap();
        let rt = runtime(&cal, &NO_MANAGERS, Some(EMPLOYEE), Vec::new());
        let mut execution = run(&model, &id(SCN), &rt).unwrap();
        execution.specification_hash = Hash::content_sha256(b"older scenario");
        assert!(matches!(
            compare_run(&model, &execution, &rt),
            Err(ComparisonError::StaleExecutionRun { .. })
        ));
    }

    /// [Fable #118] The frozen interpreter verdict of every immutable HR fixture scenario whose
    /// Then asserts an attribute value the S4.2 interpreter never writes (it never invents
    /// attribute writes): Fail with a value diff, whatever `expected_post_answers` says, until
    /// the contract gains human-confirmed effect semantics. A write is a Then value of a Given
    /// state key that differs from its Given value.
    const FIXTURE_WRITE_VERDICTS: [(&str, ScenarioVerdict); 3] = [
        ("SCN-019", ScenarioVerdict::Fail),
        ("SCN-025", ScenarioVerdict::Fail),
        ("SCN-034", ScenarioVerdict::Fail),
    ];

    #[test]
    fn fixture_attribute_write_expectations_have_frozen_interpreter_verdicts() {
        // The fixture is read only as an oracle.
        let fixture: serde_yaml::Value =
            serde_yaml::from_str(include_str!("../../../fixtures/hr-leave/scenarios.yaml"))
                .unwrap();
        // Then keys that are lifecycle, event, outcome or calculation expectations.
        let not_writes = ["status", "event", "outcome", "working_days"];
        let mut writes = BTreeSet::new();
        for s in fixture["scenarios"].as_sequence().unwrap() {
            let given = &s["given"];
            let Some(then) = s["then"].as_mapping() else {
                continue;
            };
            for (key, value) in then {
                let key = key.as_str().unwrap();
                let written = given.get(key).is_some_and(|g| g != value);
                if !not_writes.contains(&key) && written {
                    writes.insert(s["id"].as_str().unwrap().to_owned());
                }
            }
        }
        let frozen: BTreeSet<String> = FIXTURE_WRITE_VERDICTS
            .iter()
            .map(|(id, _)| (*id).to_owned())
            .collect();
        assert_eq!(writes, frozen);
        // The verdict follows from the interpreter contract: the SCN-019 shape (approval
        // deducts the balance) executes, the balance is never written, so it is a Fail.
        let mut w = hr_leave_world();
        let mut t = then();
        t.states_active = vec![id(L_APPROVED)];
        t.attribute_values
            .insert(id(BALANCE), lit(decimal("18.0", None)));
        w.scenario_then(
            given_states(&[(BALANCE, lit(decimal("20.0", None)))], &[L_SUBMITTED]),
            ScenarioWhen::Operation(id(APPROVE)),
            t,
        );
        let (_, c) = hr_compare(&w, Some(MANAGER)).unwrap();
        assert_eq!(c.result, FIXTURE_WRITE_VERDICTS[0].1);
        assert_eq!(
            c.diffs,
            vec![ScenarioDiff::ValueMismatch {
                semantic_ref: id(BALANCE),
                location: DiffLocation::AttributeValue,
                expected: decimal("18.0", None),
                actual: decimal("20.0", None),
            }]
        );
    }

    #[test]
    fn then_calculation_result_refs_resolve_to_the_execution_result() {
        // H0.10: result:<key> resolves to the named Calculation's execution result in the trace.
        let mut t = then();
        t.calculation_results
            .insert(id(CALC_A), sem(CALC_A, "result:doubled"));
        let c = compare(&rich_world("amount > 10", t), rich_inputs());
        assert_eq!(c.result, ScenarioVerdict::Pass, "{:?}", c.diffs);
        let steps = comparison_steps(&c);
        assert!(
            steps.contains(&&ComparisonTraceStep::ExpectedValueResolved {
                owner_ref: id(CALC_A),
                key: "result:doubled".into(),
                source: ResolutionSource::ExecutionResult,
            })
        );
        assert!(steps.contains(&&ComparisonTraceStep::CalculationCompared {
            calculation_ref: id(CALC_A),
            matched: true,
        }));
    }
}
