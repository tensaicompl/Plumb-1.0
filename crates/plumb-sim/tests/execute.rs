//! S4.2 contract tests for the pure functional interpreter (Hotfix 051).
//!
//! Every test lives in `execute_contract` (`cargo test -p plumb-sim --test execute`). Graphs,
//! Scenarios, calendars and predicates are built in memory; no fixture file, store, clock,
//! network or provider other than the injected deterministic test doubles is used. Then
//! comparison is S4.3's and is never exercised here.

mod execute_contract {
    use std::collections::{BTreeMap, BTreeSet};
    use std::str::FromStr;

    use plumb_core::{Hash, Id, Timestamp};
    use plumb_expr::{
        CalendarDefinition, PredicateError, PredicateProvider, RefValue, StaticCalendarProvider,
    };
    use plumb_functional::decision_table::{
        DecisionColumn, DecisionInputCell, DecisionOutputCell, DecisionRow, DecisionTableSpec,
        DecisionType, NumericBound, NumericDomain, NumericInterval,
    };
    use plumb_functional::{
        CalculationBindingExposure, CalculationScope, CalculationScopeBinding,
        ScenarioDecisionTableInput, ScenarioDependency, ScenarioGiven, ScenarioLiteral,
        ScenarioThen, ScenarioValue, ScenarioWhen, CALCULATION_ORIGIN_EXTENSION, HOLIDAY_DATE_KEY,
        WORKING_DAYS_INCLUSIVE_KEY,
    };
    use plumb_psg::{
        ActorKind, Assumption, Attribute, AuditMeta, BusinessRole, Calculation, Calendar,
        DecisionTable, Edge, ElementStatus, Entity, Event, ExtensionKey, Graph, Invariant, Node,
        NodePayload, Operation, OperationKind, Outcome, OutcomeKind, Permission, PolicyCondition,
        Principal, RelationKind, RelationProperties, ResourceScope, Rule, RuleKind, Scenario,
        ScenarioKind, SecurityRole, State, Transition,
    };
    use plumb_sim::{
        build_execution_model, run, AuthorizationDecision, EvaluationFailureKind,
        ExecutionCalculationScopeOverride, ExecutionExpressionScopeInput, ExecutionFailure,
        ExecutionInputKind, ExecutionInputs, ExecutionResult, ExecutionRun, ExecutionRuntime,
        ExecutionTraceStep, ExpressionSite, MissingSemanticKind, ResolutionSource,
        RuntimeSemanticValue, SimulationError,
    };
    use serde_json::{json, Value};

    use ElementStatus::{Accepted, Proposed};

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
    const TR_CLOSE: &str = "transition:close";
    const APPROVED_EVENT: &str = "event:approved";
    const RULE: &str = "rule:large";
    const TABLE: &str = "decision-table:eligibility";

    const SOURCES: [&str; 3] = [
        "../src/model.rs",
        "../src/materialize.rs",
        "../src/execute.rs",
    ];
    const PRODUCTION: [&str; 4] = [
        include_str!("../src/lib.rs"),
        include_str!("../src/model.rs"),
        include_str!("../src/materialize.rs"),
        include_str!("../src/execute.rs"),
    ];

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

        /// Adds the Scenario under test.
        fn scenario(
            &mut self,
            given: ScenarioGiven<ScenarioValue>,
            when: ScenarioWhen,
        ) -> &mut World {
            self.scenario_with(given, when, Accepted, Some("Confirmed"))
        }

        fn scenario_with(
            &mut self,
            given: ScenarioGiven<ScenarioValue>,
            when: ScenarioWhen,
            status: ElementStatus,
            confirmation: Option<&str>,
        ) -> &mut World {
            let encode = |v: &ScenarioValue| serde_json::to_value(v).unwrap();
            let then: ScenarioThen<ScenarioValue> = ScenarioThen::default();
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

    fn invariant(node_id: &str, scope_ref: &str, expression: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Invariant(Invariant {
                scope_ref: id(scope_ref),
                expression: expression.into(),
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

    fn with_rounding(mut n: Node, rounding: &str) -> Node {
        if let NodePayload::Calculation(c) = &mut n.payload {
            c.rounding = Some(rounding.into());
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

    fn try_run(
        world: &World,
        inputs: ExecutionInputs,
        rt: &ExecutionRuntime<'_>,
    ) -> Result<ExecutionRun, SimulationError> {
        let graph = world.graph();
        let model = build_execution_model(&graph, inputs)?;
        run(&model, &id(SCN), rt)
    }

    fn execute(world: &World, inputs: ExecutionInputs) -> ExecutionResult {
        let cal = calendars(&[]);
        try_run(
            world,
            inputs,
            &runtime(&cal, &NO_MANAGERS, None, Vec::new()),
        )
        .unwrap()
        .result
    }

    fn missing_of(result: &ExecutionResult) -> Vec<(String, String)> {
        match result {
            ExecutionResult::Undecidable { missing, .. } => missing
                .iter()
                .map(|m| (m.owner_ref.to_string(), m.key.clone()))
                .collect(),
            other => panic!("expected Undecidable, got {other:?}"),
        }
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(o, k)| (o.to_string(), k.to_string()))
            .collect()
    }

    fn failure_of(result: &ExecutionResult) -> &ExecutionFailure {
        match result {
            ExecutionResult::Failed { failure, .. } => failure,
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    fn trace_of(result: &ExecutionResult) -> &[ExecutionTraceStep] {
        match result {
            ExecutionResult::Executed { trace, .. }
            | ExecutionResult::Failed { trace, .. }
            | ExecutionResult::Rejected { trace, .. }
            | ExecutionResult::Undecidable { trace, .. } => trace,
        }
    }

    fn executed(
        result: &ExecutionResult,
    ) -> (
        &plumb_sim::RuntimeState,
        &[plumb_core::Id],
        &[plumb_core::Id],
        &BTreeMap<Id, bool>,
    ) {
        match result {
            ExecutionResult::Executed {
                state,
                outcome_refs,
                event_refs,
                rule_results,
                ..
            } => (state, outcome_refs, event_refs, rule_results),
            other => panic!("expected Executed, got {other:?}"),
        }
    }

    fn calc_result(result: &ExecutionResult, calculation_ref: &str) -> ScenarioLiteral {
        executed(result).0.calculation_results[&id(calculation_ref)].clone()
    }

    // ------------------------------------------------------------------ materialization

    #[test]
    fn literals_materialize_exactly_without_floats() {
        let mut w = request_world();
        w.add(rule(RULE, Some("amount > 0"), None));
        let values = [
            (AMOUNT, int(7)),
            (PRIORITY, enumeration("High")),
            (APPROVED, boolean(true)),
            (DUE, date("2026-02-28")),
            (FEE, decimal("12.50", None)),
            (HOURS, decimal("1.5", Some("hour"))),
            (
                NOTE,
                ScenarioLiteral::String {
                    value: "Urgent".into(),
                },
            ),
            (
                STAMP,
                ScenarioLiteral::DateTime {
                    value: ts("2026-03-01T08:30:00Z"),
                },
            ),
        ];
        let g: Vec<(&str, ScenarioValue)> =
            values.iter().map(|(a, v)| (*a, lit(v.clone()))).collect();
        w.scenario(given(&g), ScenarioWhen::Rule(id(RULE)));
        let result = execute(&w, scopes(vec![scope(RULE, &[(AMOUNT, "amount")])]));
        let (state, _, _, rules) = executed(&result);
        let expected: BTreeMap<Id, ScenarioLiteral> =
            values.iter().map(|(a, v)| (id(a), v.clone())).collect();
        assert_eq!(state.attributes, expected);
        assert!(rules[&id(RULE)]);
        assert_eq!(state.attributes[&id(FEE)], decimal("12.50", None));
    }

    #[test]
    fn wrong_typed_given_literal_is_an_error() {
        // H0.4 [Fable #116]: Given literals are type-checked at scenario validation, so the
        // Scenario is never admitted (it is invalid, not a content-dependent run condition).
        let mut w = request_world();
        w.add(rule(RULE, Some("amount > 0"), None));
        w.scenario(
            given(&[(AMOUNT, lit(decimal("7.5", None)))]),
            ScenarioWhen::Rule(id(RULE)),
        );
        let cal = calendars(&[]);
        let err = try_run(
            &w,
            scopes(vec![scope(RULE, &[(AMOUNT, "amount")])]),
            &runtime(&cal, &NO_MANAGERS, None, Vec::new()),
        )
        .unwrap_err();
        assert!(
            matches!(&err, SimulationError::InvalidScenario { reason, .. } if reason.contains(AMOUNT)),
            "{err:?}"
        );
    }

    /// The typed model conditions of an Undecidable run (H0.4, Fable #116).
    fn conditions_of(result: &ExecutionResult) -> Vec<(String, String)> {
        match result {
            ExecutionResult::Undecidable { missing, .. } => missing
                .iter()
                .filter(|m| m.kind == MissingSemanticKind::ModelCondition)
                .map(|m| (m.owner_ref.to_string(), m.key.clone()))
                .collect(),
            other => panic!("expected Undecidable, got {other:?}"),
        }
    }

    // ------------------------------------------------------------------ HR semantic references

    #[test]
    fn hr_half_day_pre_answer_is_undecidable() {
        let mut w = hr_world(HR_EXPRESSION, None);
        w.scenario(
            given(&[(FRACTION, sem(CALC, HALF_DAY))]),
            ScenarioWhen::Calculation(id(CALC)),
        );
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(missing_of(&result), pairs(&[(CALC, HALF_DAY)]));
        let ExecutionResult::Undecidable { missing, trace } = &result else {
            unreachable!()
        };
        assert_eq!(missing[0].kind, MissingSemanticKind::SemanticRef);
        assert_eq!(
            trace,
            &vec![
                ExecutionTraceStep::MaterializationStarted {
                    scenario_ref: id(SCN)
                },
                ExecutionTraceStep::SemanticMissing {
                    missing: missing.clone()
                },
            ]
        );
    }

    #[test]
    fn hr_pre_answer_acceptance_names_every_missing_semantic() {
        let mut w = hr_world(HR_EXPRESSION, None);
        let mut g = given(&[
            (START, sem(CAL, HOLIDAY_DATE_KEY)),
            (END, sem(CAL, HOLIDAY_DATE_KEY)),
            (FRACTION, sem(CALC, HALF_DAY)),
        ]);
        g.calculation_semantics.insert(
            id(CALC),
            BTreeMap::from([(
                WORKING_DAYS_INCLUSIVE_KEY.to_owned(),
                sem(CALC, WORKING_DAYS_INCLUSIVE_KEY),
            )]),
        );
        w.scenario(g, ScenarioWhen::Calculation(id(CALC)));
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(
            missing_of(&result),
            pairs(&[
                (CAL, HOLIDAY_DATE_KEY),
                (CALC, HALF_DAY),
                (CALC, WORKING_DAYS_INCLUSIVE_KEY),
            ])
        );
        assert!(!trace_of(&result)
            .iter()
            .any(|s| matches!(s, ExecutionTraceStep::CalculationEvaluated { .. })));
    }

    #[test]
    fn hr_half_day_post_answer_resolves_from_the_accepted_example() {
        let mut w = hr_world(HR_EXPRESSION, Some(vec![half_day_example("0.5")]));
        w.scenario(
            given(&[(FRACTION, sem(CALC, HALF_DAY))]),
            ScenarioWhen::Calculation(id(CALC)),
        );
        let result = execute(&w, ExecutionInputs::default());
        let missing = missing_of(&result);
        assert!(!missing.contains(&(CALC.to_string(), HALF_DAY.to_string())));
        assert_eq!(
            missing,
            pairs(&[(END, "value"), (INCLUSIVE, "value"), (START, "value")])
        );
        assert!(
            trace_of(&result).contains(&ExecutionTraceStep::SemanticResolved {
                owner_ref: id(CALC),
                key: HALF_DAY.into(),
                source: ResolutionSource::CalculationExample,
            })
        );
    }

    #[test]
    fn hr_half_day_resolved_value_drives_the_calculation() {
        let mut w = hr_world(HR_EXPRESSION, Some(vec![half_day_example("0.5")]));
        w.scenario(
            given(&[
                (START, lit(date("2026-01-05"))),
                (END, lit(date("2026-01-09"))),
                (INCLUSIVE, lit(boolean(true))),
                (FRACTION, sem(CALC, HALF_DAY)),
            ]),
            ScenarioWhen::Calculation(id(CALC)),
        );
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(
            calc_result(&result, CALC),
            decimal("2.5", Some("working_day"))
        );
        assert_eq!(
            executed(&result).0.attributes[&id(FRACTION)],
            decimal("0.5", None)
        );
    }

    #[test]
    fn hr_ambiguous_or_malformed_examples_are_per_scenario_conditions() {
        // H0.4 [Fable #116]: bad Accepted examples make only the dependent Scenario
        // Undecidable with a typed condition; nothing aborts the batch.
        let cal = calendars(&[]);
        let rt = runtime(&cal, &NO_MANAGERS, None, Vec::new());
        let mut twice = hr_world(
            HR_EXPRESSION,
            Some(vec![half_day_example("0.5"), {
                let mut e = half_day_example("0.25");
                e["inputs"][0]["value"]["value"] = json!("0.25");
                e
            }]),
        );
        twice.scenario(
            given(&[(FRACTION, sem(CALC, HALF_DAY))]),
            ScenarioWhen::Calculation(id(CALC)),
        );
        assert_eq!(
            conditions_of(
                &try_run(&twice, ExecutionInputs::default(), &rt)
                    .unwrap()
                    .result
            ),
            pairs(&[(CALC, "calculation_example:half_day_fraction")])
        );
        let mut malformed = hr_world(HR_EXPRESSION, Some(vec![json!({"version": 2})]));
        malformed.scenario(
            given(&[(FRACTION, sem(CALC, HALF_DAY))]),
            ScenarioWhen::Calculation(id(CALC)),
        );
        assert_eq!(
            conditions_of(
                &try_run(&malformed, ExecutionInputs::default(), &rt)
                    .unwrap()
                    .result
            ),
            pairs(&[(CALC, "calculation_example")])
        );
    }

    fn inclusive_given() -> ScenarioGiven<ScenarioValue> {
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
        g
    }

    #[test]
    fn hr_inclusive_end_symbolic_is_undecidable() {
        let mut w = hr_world(
            "working_days([start_date, end_date], calendar, inclusive_end)",
            None,
        );
        w.scenario(inclusive_given(), ScenarioWhen::Calculation(id(CALC)));
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(
            missing_of(&result),
            pairs(&[(CALC, WORKING_DAYS_INCLUSIVE_KEY)])
        );
    }

    #[test]
    fn hr_inclusive_end_literal_resolves_and_evaluates() {
        for (literal, expected) in [("true", "5"), ("false", "4")] {
            let mut w = hr_world(
                &format!("working_days([start_date, end_date], calendar, {literal})"),
                None,
            );
            w.scenario(inclusive_given(), ScenarioWhen::Calculation(id(CALC)));
            let result = execute(&w, ExecutionInputs::default());
            assert_eq!(
                calc_result(&result, CALC),
                decimal(expected, Some("working_day"))
            );
            assert!(
                trace_of(&result).contains(&ExecutionTraceStep::SemanticResolved {
                    owner_ref: id(CALC),
                    key: WORKING_DAYS_INCLUSIVE_KEY.into(),
                    source: ResolutionSource::CalculationExpression,
                })
            );
        }
    }

    #[test]
    fn hr_inclusive_end_given_literal_must_match_the_expression() {
        let mut w = hr_world("working_days([start_date, end_date], calendar, true)", None);
        let mut g = inclusive_given();
        g.calculation_semantics.insert(
            id(CALC),
            BTreeMap::from([(WORKING_DAYS_INCLUSIVE_KEY.to_owned(), lit(boolean(false)))]),
        );
        w.scenario(g, ScenarioWhen::Calculation(id(CALC)));
        let cal = calendars(&[]);
        let result = try_run(
            &w,
            ExecutionInputs::default(),
            &runtime(&cal, &NO_MANAGERS, None, Vec::new()),
        )
        .unwrap()
        .result;
        assert_eq!(
            conditions_of(&result),
            pairs(&[(
                CALC,
                &format!("semantic_contradiction:{WORKING_DAYS_INCLUSIVE_KEY}")
            )])
        );
    }

    fn holiday_world() -> World {
        let mut w = hr_world("working_days([start_date, end_date], calendar, true)", None);
        w.scenario(
            given(&[
                (START, sem(CAL, HOLIDAY_DATE_KEY)),
                (END, sem(CAL, HOLIDAY_DATE_KEY)),
            ]),
            ScenarioWhen::Calculation(id(CALC)),
        );
        w
    }

    fn holiday_value(owner: &str, key: &str, value: ScenarioLiteral) -> RuntimeSemanticValue {
        RuntimeSemanticValue {
            owner_ref: id(owner),
            key: key.into(),
            value,
        }
    }

    #[test]
    fn hr_holiday_pre_answer_is_undecidable() {
        let result = execute(&holiday_world(), ExecutionInputs::default());
        assert_eq!(missing_of(&result), pairs(&[(CAL, HOLIDAY_DATE_KEY)]));
    }

    #[test]
    fn hr_holiday_runtime_value_materializes_and_the_calendar_excludes_it() {
        let w = holiday_world();
        let cal = calendars(&["2026-01-06"]);
        let rt = runtime(
            &cal,
            &NO_MANAGERS,
            None,
            vec![holiday_value(CAL, HOLIDAY_DATE_KEY, date("2026-01-06"))],
        );
        let result = try_run(&w, ExecutionInputs::default(), &rt).unwrap().result;
        let (state, ..) = executed(&result);
        assert_eq!(state.attributes[&id(START)], date("2026-01-06"));
        assert_eq!(state.attributes[&id(END)], date("2026-01-06"));
        assert_eq!(
            calc_result(&result, CALC),
            decimal("0", Some("working_day"))
        );
        assert!(
            trace_of(&result).contains(&ExecutionTraceStep::SemanticResolved {
                owner_ref: id(CAL),
                key: HOLIDAY_DATE_KEY.into(),
                source: ResolutionSource::RuntimeValue,
            })
        );
        // Without the holiday in the injected calendar the same date is a working day.
        let plain = calendars(&[]);
        let rt = runtime(
            &plain,
            &NO_MANAGERS,
            None,
            vec![holiday_value(CAL, HOLIDAY_DATE_KEY, date("2026-01-06"))],
        );
        let result = try_run(&w, ExecutionInputs::default(), &rt).unwrap().result;
        assert_eq!(
            calc_result(&result, CALC),
            decimal("1", Some("working_day"))
        );
    }

    #[test]
    fn runtime_values_cannot_answer_business_semantics() {
        let w = holiday_world();
        let cal = calendars(&[]);
        let attempt = |values: Vec<RuntimeSemanticValue>| {
            try_run(
                &w,
                ExecutionInputs::default(),
                &runtime(&cal, &NO_MANAGERS, None, values),
            )
        };
        for key in [HALF_DAY, WORKING_DAYS_INCLUSIVE_KEY, "result:half_day"] {
            let err = attempt(vec![holiday_value(CALC, key, decimal("0.5", None))]).unwrap_err();
            assert!(
                matches!(&err, SimulationError::DisallowedRuntimeValue { key: k, .. } if k == key),
                "{key}: {err:?}"
            );
        }
        let err = attempt(vec![holiday_value(
            LEAVE,
            HOLIDAY_DATE_KEY,
            date("2026-01-06"),
        )])
        .unwrap_err();
        assert!(matches!(
            err,
            SimulationError::DisallowedRuntimeValue { .. }
        ));
        let err = attempt(vec![holiday_value(CAL, HOLIDAY_DATE_KEY, boolean(true))]).unwrap_err();
        assert!(matches!(
            err,
            SimulationError::InvalidInput {
                input: ExecutionInputKind::RuntimeSemanticValue,
                ..
            }
        ));
        let twice = holiday_value(CAL, HOLIDAY_DATE_KEY, date("2026-01-06"));
        let err = attempt(vec![twice.clone(), twice]).unwrap_err();
        assert!(matches!(err, SimulationError::DuplicateInput { .. }));
    }

    // ------------------------------------------------------------------ decision tables

    fn column(name: &str, ty: DecisionType, domain: Option<(&str, &str)>) -> DecisionColumn {
        DecisionColumn {
            name: name.into(),
            ty,
            domain: domain.map(|(l, u)| NumericDomain {
                lower: l.parse().unwrap(),
                upper: u.parse().unwrap(),
            }),
        }
    }

    fn interval(lower: Option<(&str, bool)>, upper: Option<(&str, bool)>) -> DecisionInputCell {
        let bound = |(value, inclusive): (&str, bool)| NumericBound {
            value: value.parse().unwrap(),
            inclusive,
        };
        DecisionInputCell::Interval {
            interval: NumericInterval {
                lower: lower.map(bound),
                upper: upper.map(bound),
            },
        }
    }

    fn out_int(value: i64) -> DecisionOutputCell {
        DecisionOutputCell::Int { value }
    }

    /// Columns: contractor Bool, leave_type Enum, days Int, fraction Decimal(2).
    fn table_spec(hit_policy: &str, rows: Vec<(Vec<DecisionInputCell>, i64)>) -> DecisionTableSpec {
        DecisionTableSpec {
            hit_policy: hit_policy.into(),
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
            outputs: vec![column("code", DecisionType::Int, None)],
            rows: rows
                .into_iter()
                .map(|(inputs, code)| DecisionRow {
                    inputs,
                    outputs: vec![out_int(code)],
                })
                .collect(),
            default_output: None,
        }
    }

    fn table_world(hit_policy: &str, inputs: &[(u32, ScenarioLiteral)]) -> World {
        let mut w = World::empty();
        w.add(node(
            TABLE,
            Accepted,
            NodePayload::DecisionTable(DecisionTable {
                name: "Eligibility".into(),
                hit_policy: hit_policy.into(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                rows: Vec::new(),
            }),
        ));
        let mut g = ScenarioGiven::default();
        for (i, v) in inputs {
            g.decision_inputs.insert(*i, lit(v.clone()));
        }
        w.scenario(g, ScenarioWhen::DecisionTable(id(TABLE)));
        w
    }

    fn table_inputs(spec: DecisionTableSpec) -> ExecutionInputs {
        ExecutionInputs {
            decision_tables: vec![ScenarioDecisionTableInput {
                decision_table_ref: id(TABLE),
                spec,
            }],
            ..ExecutionInputs::default()
        }
    }

    fn any4() -> Vec<DecisionInputCell> {
        vec![DecisionInputCell::Any; 4]
    }

    fn row_inputs(
        contractor: bool,
        types: &[&str],
        days: DecisionInputCell,
        fraction: DecisionInputCell,
    ) -> Vec<DecisionInputCell> {
        vec![
            DecisionInputCell::Bool { value: contractor },
            DecisionInputCell::EnumSet {
                members: types.iter().map(|t| t.to_string()).collect(),
            },
            days,
            fraction,
        ]
    }

    fn standard_inputs(days: i64, fraction: &str) -> Vec<(u32, ScenarioLiteral)> {
        vec![
            (0, boolean(false)),
            (1, enumeration("Annual")),
            (2, int(days)),
            (3, decimal(fraction, None)),
        ]
    }

    fn table_output(result: &ExecutionResult) -> (i64, Option<u32>) {
        let (state, ..) = executed(result);
        let code = match &state.decision_outputs[&id(TABLE)][&0] {
            ScenarioLiteral::Int { value } => *value,
            other => panic!("{other:?}"),
        };
        let row = trace_of(result)
            .iter()
            .find_map(|s| match s {
                ExecutionTraceStep::DecisionTableEvaluated { row, .. } => Some(*row),
                _ => None,
            })
            .unwrap();
        (code, row)
    }

    #[test]
    fn decision_table_first_takes_the_first_matching_row_in_spec_order() {
        let spec = table_spec(
            "FIRST",
            vec![
                (
                    row_inputs(
                        true,
                        &["Annual"],
                        DecisionInputCell::Any,
                        DecisionInputCell::Any,
                    ),
                    9,
                ),
                (any4(), 1),
                (
                    row_inputs(
                        false,
                        &["Annual"],
                        DecisionInputCell::Any,
                        DecisionInputCell::Any,
                    ),
                    2,
                ),
            ],
        );
        let w = table_world("FIRST", &standard_inputs(3, "0.50"));
        let result = execute(&w, table_inputs(spec));
        assert_eq!(table_output(&result), (1, Some(1)));
    }

    #[test]
    fn decision_table_first_without_match_uses_the_default() {
        let mut spec = table_spec(
            "FIRST",
            vec![(
                row_inputs(
                    true,
                    &["Sick"],
                    DecisionInputCell::Any,
                    DecisionInputCell::Any,
                ),
                9,
            )],
        );
        spec.default_output = Some(vec![out_int(0)]);
        let w = table_world("FIRST", &standard_inputs(3, "0.50"));
        assert_eq!(table_output(&execute(&w, table_inputs(spec))), (0, None));
    }

    #[test]
    fn decision_table_unique_selects_one_fails_on_none_and_rejects_several() {
        let rows = vec![
            (
                row_inputs(
                    false,
                    &["Annual"],
                    DecisionInputCell::Any,
                    DecisionInputCell::Any,
                ),
                1,
            ),
            (
                row_inputs(
                    false,
                    &["Sick", "Unpaid"],
                    DecisionInputCell::Any,
                    DecisionInputCell::Any,
                ),
                2,
            ),
        ];
        let w = table_world("UNIQUE", &standard_inputs(3, "0.50"));
        let result = execute(&w, table_inputs(table_spec("UNIQUE", rows.clone())));
        assert_eq!(table_output(&result), (1, Some(0)));

        let mut none = standard_inputs(3, "0.50");
        none[0].1 = boolean(true);
        let result = execute(
            &table_world("UNIQUE", &none),
            table_inputs(table_spec("UNIQUE", rows.clone())),
        );
        assert_eq!(
            failure_of(&result),
            &ExecutionFailure::NoDecisionRow {
                decision_table_ref: id(TABLE)
            }
        );

        // H0.4 [Fable #116]: UNIQUE overlaps are rejected at model build, before any run.
        let mut overlapping = rows;
        overlapping.push((any4(), 3));
        let graph = w.graph();
        assert!(matches!(
            build_execution_model(&graph, table_inputs(table_spec("UNIQUE", overlapping)))
                .err()
                .unwrap(),
            SimulationError::InvalidDecisionTableSpec { .. }
        ));
    }

    #[test]
    fn decision_table_numeric_bounds_are_exact() {
        let rows = vec![
            (
                row_inputs(
                    false,
                    &["Annual"],
                    interval(Some(("5", false)), Some(("10", true))),
                    DecisionInputCell::Any,
                ),
                1,
            ),
            (
                row_inputs(
                    false,
                    &["Annual"],
                    interval(Some(("0", true)), Some(("5", true))),
                    interval(Some(("0.50", false)), None),
                ),
                2,
            ),
            (
                row_inputs(
                    false,
                    &["Annual"],
                    interval(Some(("0", true)), Some(("5", true))),
                    interval(None, Some(("0.50", true))),
                ),
                3,
            ),
            (
                row_inputs(
                    false,
                    &["Annual"],
                    interval(Some(("10", false)), Some(("30", true))),
                    DecisionInputCell::Any,
                ),
                4,
            ),
        ];
        for (days, fraction, code) in [
            (10, "0.50", 1),
            (6, "0.50", 1),
            (5, "0.51", 2),
            (5, "0.50", 3),
            (0, "0.25", 3),
            (11, "0.50", 4),
        ] {
            let w = table_world("FIRST", &standard_inputs(days, fraction));
            let result = execute(&w, table_inputs(table_spec("FIRST", rows.clone())));
            assert_eq!(table_output(&result).0, code, "{days} {fraction}");
        }
    }

    #[test]
    fn decision_table_inputs_are_typed_and_complete() {
        let spec = table_spec("FIRST", vec![(any4(), 1)]);
        let mut partial = standard_inputs(3, "0.50");
        partial.remove(2);
        let result = execute(&table_world("FIRST", &partial), table_inputs(spec.clone()));
        assert_eq!(missing_of(&result), pairs(&[(TABLE, "decision_input:2")]));

        let cal = calendars(&[]);
        let rt = runtime(&cal, &NO_MANAGERS, None, Vec::new());
        for bad in [
            (2, decimal("3.5", None)),
            (1, enumeration("Parental")),
            (3, decimal("0.505", None)),
        ] {
            let mut inputs = standard_inputs(3, "0.50");
            inputs.retain(|(i, _)| *i != bad.0);
            inputs.push(bad.clone());
            // H0.4 [Fable #116]: a per-scenario typed condition, never a batch abort.
            let result = try_run(
                &table_world("FIRST", &inputs),
                table_inputs(spec.clone()),
                &rt,
            )
            .unwrap()
            .result;
            assert_eq!(
                conditions_of(&result),
                pairs(&[(TABLE, &format!("decision_input_mismatch:{}", bad.0))]),
                "{bad:?}"
            );
        }
        let mut extra = standard_inputs(3, "0.50");
        extra.push((4, int(1)));
        let result = try_run(&table_world("FIRST", &extra), table_inputs(spec), &rt)
            .unwrap()
            .result;
        assert_eq!(
            conditions_of(&result),
            pairs(&[(TABLE, "decision_input_mismatch:4")])
        );
    }

    #[test]
    fn decision_table_without_spec_is_undecidable_and_inputs_are_validated() {
        let w = table_world("FIRST", &standard_inputs(3, "0.50"));
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(
            missing_of(&result),
            pairs(&[(TABLE, "decision_table_spec")])
        );

        let graph = w.graph();
        let spec = table_spec("FIRST", vec![(any4(), 1)]);
        let twice = ExecutionInputs {
            decision_tables: vec![
                ScenarioDecisionTableInput {
                    decision_table_ref: id(TABLE),
                    spec: spec.clone(),
                };
                2
            ],
            ..ExecutionInputs::default()
        };
        assert!(matches!(
            build_execution_model(&graph, twice).err().unwrap(),
            SimulationError::DuplicateInput { .. }
        ));
        let other_policy = table_inputs(table_spec("UNIQUE", vec![(any4(), 1)]));
        assert!(matches!(
            build_execution_model(&graph, other_policy).err().unwrap(),
            SimulationError::InvalidInput { .. }
        ));
        let stale = ExecutionInputs {
            decision_tables: vec![ScenarioDecisionTableInput {
                decision_table_ref: id("decision-table:unknown"),
                spec,
            }],
            ..ExecutionInputs::default()
        };
        assert!(matches!(
            build_execution_model(&graph, stale).err().unwrap(),
            SimulationError::InvalidInput { .. }
        ));
    }

    // ------------------------------------------------------------------ calculations

    const CALC_A: &str = "calculation:a";
    const CALC_B: &str = "calculation:b";

    fn calc_world(calcs: Vec<Node>, values: &[(&str, ScenarioValue)], anchor: &str) -> World {
        let mut w = request_world();
        for c in calcs {
            w.add(c);
        }
        w.scenario(given(values), ScenarioWhen::Calculation(id(anchor)));
        w
    }

    #[test]
    fn calculation_scalar_and_quantity_results() {
        let scalar = calculation(
            CALC_A,
            "amount * 2",
            ("Int", None),
            None,
            &[(AMOUNT, "amount")],
            None,
            true,
        );
        let w = calc_world(vec![scalar], &[(AMOUNT, lit(int(21)))], CALC_A);
        assert_eq!(
            calc_result(&execute(&w, ExecutionInputs::default()), CALC_A),
            int(42)
        );

        let quantity = calculation(
            CALC_A,
            "hours * 2",
            ("Decimal(1)", Some("hour")),
            None,
            &[(HOURS, "hours")],
            None,
            true,
        );
        let w = calc_world(
            vec![quantity],
            &[(HOURS, lit(decimal("1.5", Some("hour"))))],
            CALC_A,
        );
        assert_eq!(
            calc_result(&execute(&w, ExecutionInputs::default()), CALC_A),
            decimal("3.0", Some("hour"))
        );
    }

    #[test]
    fn calculation_dependencies_run_first_and_rounding_uses_the_existing_helper() {
        let b = calculation(
            CALC_B,
            "hours / 3",
            ("Decimal(1)", Some("hour")),
            None,
            &[(HOURS, "hours")],
            None,
            true,
        );
        let b = with_rounding(b, "half_up(1)");
        let a = calculation(
            CALC_A,
            "b * 2",
            ("Decimal(1)", Some("hour")),
            None,
            &[(CALC_B, "b")],
            None,
            true,
        );
        let w = calc_world(
            vec![a, b],
            &[(HOURS, lit(decimal("10.0", Some("hour"))))],
            CALC_A,
        );
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(calc_result(&result, CALC_B), decimal("3.3", Some("hour")));
        assert_eq!(calc_result(&result, CALC_A), decimal("6.6", Some("hour")));
        let order: Vec<&Id> = trace_of(&result)
            .iter()
            .filter_map(|s| match s {
                ExecutionTraceStep::CalculationEvaluated {
                    calculation_ref, ..
                } => Some(calculation_ref),
                _ => None,
            })
            .collect();
        assert_eq!(order, vec![&id(CALC_B), &id(CALC_A)]);
    }

    #[test]
    fn calculation_missing_input_is_undecidable() {
        let c = calculation(
            CALC_A,
            "amount * 2",
            ("Int", None),
            None,
            &[(AMOUNT, "amount")],
            None,
            true,
        );
        let w = calc_world(vec![c], &[], CALC_A);
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(missing_of(&result), pairs(&[(AMOUNT, "value")]));
        assert_eq!(
            match &result {
                ExecutionResult::Undecidable { missing, .. } => missing[0].kind,
                _ => unreachable!(),
            },
            MissingSemanticKind::AttributeValue
        );
    }

    #[test]
    fn calculation_cycles_are_rejected_and_legacy_scopes_need_an_override() {
        let a = calculation(
            CALC_A,
            "b + 1",
            ("Int", None),
            None,
            &[(CALC_B, "b")],
            None,
            true,
        );
        let b = calculation(
            CALC_B,
            "a + 1",
            ("Int", None),
            None,
            &[(CALC_A, "a")],
            None,
            true,
        );
        let w = calc_world(vec![a, b], &[], CALC_A);
        let graph = w.graph();
        assert!(matches!(
            build_execution_model(&graph, ExecutionInputs::default())
                .err()
                .unwrap(),
            SimulationError::CalculationCycle { .. }
        ));

        let legacy = calculation(
            CALC_A,
            "amount * 2",
            ("Int", None),
            None,
            &[(AMOUNT, "amount")],
            None,
            false,
        );
        let w = calc_world(vec![legacy], &[(AMOUNT, lit(int(4)))], CALC_A);
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(missing_of(&result), pairs(&[(CALC_A, "calculation_scope")]));
        let with_override = ExecutionInputs {
            calculation_scope_overrides: vec![ExecutionCalculationScopeOverride {
                calculation_ref: id(CALC_A),
                scope: CalculationScope {
                    bindings: vec![root_binding(AMOUNT, "amount")],
                    calendar_refs: Vec::new(),
                },
            }],
            ..ExecutionInputs::default()
        };
        assert_eq!(calc_result(&execute(&w, with_override), CALC_A), int(8));
    }

    #[test]
    fn working_days_follow_the_injected_calendar() {
        let w = |start: &str, end: &str| {
            let mut w = hr_world("working_days([start_date, end_date], calendar, true)", None);
            w.scenario(
                given(&[(START, lit(date(start))), (END, lit(date(end)))]),
                ScenarioWhen::Calculation(id(CALC)),
            );
            w
        };
        let cases = [
            ("2026-01-05", "2026-01-11", &[][..], "5"),
            ("2026-01-05", "2026-01-11", &["2026-01-07"][..], "4"),
            ("2026-01-10", "2026-01-11", &[][..], "0"),
            ("2026-01-07", "2026-01-07", &[][..], "1"),
        ];
        for (start, end, holidays, expected) in cases {
            let cal = calendars(holidays);
            let result = try_run(
                &w(start, end),
                ExecutionInputs::default(),
                &runtime(&cal, &NO_MANAGERS, None, Vec::new()),
            )
            .unwrap()
            .result;
            assert_eq!(
                calc_result(&result, CALC),
                decimal(expected, Some("working_day")),
                "{start}..{end} {holidays:?}"
            );
        }
        // A provider that does not know the calendar fails the evaluation.
        let unknown = StaticCalendarProvider::default();
        let result = try_run(
            &w("2026-01-05", "2026-01-09"),
            ExecutionInputs::default(),
            &runtime(&unknown, &NO_MANAGERS, None, Vec::new()),
        )
        .unwrap()
        .result;
        assert_eq!(
            failure_of(&result),
            &ExecutionFailure::EvaluationFailed {
                owner_ref: id(CALC),
                kind: EvaluationFailureKind::CalendarUnavailable
            }
        );
    }

    #[test]
    fn date_outside_calendar_coverage_makes_the_run_undecidable() {
        // §33.11: business-day evaluation outside the declared coverage is Undecidable, never a
        // Failed run and never an assumed working day.
        let mut w = hr_world("working_days([start_date, end_date], calendar, true)", None);
        w.scenario(
            given(&[
                (START, lit(date("2026-12-28"))),
                (END, lit(date("2027-01-08"))),
            ]),
            ScenarioWhen::Calculation(id(CALC)),
        );
        let day = |s: &str| ts(&format!("{s}T00:00:00Z")).as_offset_datetime().date();
        let weekend = [day("2026-01-03").weekday(), day("2026-01-04").weekday()];
        let covered = StaticCalendarProvider::new([
            CalendarDefinition::new(id(CAL), weekend, []).with_coverage([2026])
        ])
        .unwrap();
        let result = try_run(
            &w,
            ExecutionInputs::default(),
            &runtime(&covered, &NO_MANAGERS, None, Vec::new()),
        )
        .unwrap()
        .result;
        assert_eq!(
            missing_of(&result),
            pairs(&[(CAL, "calendar_coverage:2027")])
        );
        match &result {
            ExecutionResult::Undecidable { missing, .. } => assert!(missing
                .iter()
                .all(|m| m.kind == MissingSemanticKind::ModelCondition)),
            other => panic!("expected Undecidable, got {other:?}"),
        }
        // Covering both years answers the same scenario.
        let both = StaticCalendarProvider::new([
            CalendarDefinition::new(id(CAL), weekend, []).with_coverage([2026, 2027])
        ])
        .unwrap();
        let result = try_run(
            &w,
            ExecutionInputs::default(),
            &runtime(&both, &NO_MANAGERS, None, Vec::new()),
        )
        .unwrap()
        .result;
        assert_eq!(
            calc_result(&result, CALC),
            decimal("10", Some("working_day"))
        );
    }

    #[test]
    fn calculation_examples_are_not_generic_inputs() {
        // The example binds day_fraction, but a missing start date is not filled from it.
        let mut w = hr_world(HR_EXPRESSION, Some(vec![half_day_example("0.5")]));
        w.scenario(
            given(&[(FRACTION, lit(decimal("0.5", None)))]),
            ScenarioWhen::Calculation(id(CALC)),
        );
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(
            missing_of(&result),
            pairs(&[(END, "value"), (INCLUSIVE, "value"), (START, "value")])
        );
    }

    // ------------------------------------------------------------------ operations

    fn submit_scenario(w: &mut World, values: &[(&str, ScenarioValue)]) {
        w.scenario(
            given_states(values, &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
        );
    }

    fn amount_scope(owner: &str) -> ExecutionExpressionScopeInput {
        scope(owner, &[(AMOUNT, "amount"), (APPROVED, "approved")])
    }

    #[test]
    fn plain_operation_executes_with_a_typed_trace() {
        let mut w = request_world();
        w.add(operation(SUBMIT, &[], &[]));
        w.scenario(given(&[]), ScenarioWhen::Operation(id(SUBMIT)));
        let result = execute(&w, ExecutionInputs::default());
        let (state, outcomes, events, rules) = executed(&result);
        assert!(outcomes.is_empty() && events.is_empty() && rules.is_empty());
        assert_eq!(state, &plumb_sim::RuntimeState::default());
        assert_eq!(
            trace_of(&result),
            &[
                ExecutionTraceStep::MaterializationStarted {
                    scenario_ref: id(SCN)
                },
                ExecutionTraceStep::GivenMaterialized {
                    attribute_refs: vec![],
                    entity_refs: vec![],
                    state_refs: vec![]
                },
                ExecutionTraceStep::FootprintDeclared {
                    operation_ref: id(SUBMIT),
                    reads: vec![],
                    writes: vec![]
                },
                ExecutionTraceStep::PerformerRecorded {
                    operation_ref: id(SUBMIT),
                    performer_refs: vec![]
                },
                ExecutionTraceStep::AuthorizationChecked {
                    operation_ref: id(SUBMIT),
                    actor_ref: None,
                    decision: AuthorizationDecision::NotApplicable
                },
                ExecutionTraceStep::ExecutionCompleted,
            ]
        );
    }

    #[test]
    fn preconditions_pass_fail_or_are_undecidable() {
        let mut w = submit_world(&["amount > 0"], &[], None);
        submit_scenario(&mut w, &[(AMOUNT, lit(int(5)))]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        assert!(
            trace_of(&result).contains(&ExecutionTraceStep::PreconditionEvaluated {
                operation_ref: id(SUBMIT),
                index: 0,
                result: true
            })
        );
        assert_eq!(
            executed(&result).0.active_states,
            BTreeSet::from([id(SUBMITTED)])
        );

        submit_scenario(&mut w, &[(AMOUNT, lit(int(0)))]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        assert_eq!(
            failure_of(&result),
            &ExecutionFailure::PreconditionRejected {
                operation_ref: id(SUBMIT),
                index: 0
            }
        );
        assert!(!trace_of(&result)
            .iter()
            .any(|s| matches!(s, ExecutionTraceStep::TransitionApplied { .. })));

        submit_scenario(&mut w, &[]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        assert_eq!(missing_of(&result), pairs(&[(AMOUNT, "value")]));

        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(missing_of(&result), pairs(&[(SUBMIT, "expression_scope")]));
    }

    #[test]
    fn preconditions_are_a_three_valued_conjunction() {
        let mut w = submit_world(&["approved", "amount > 0"], &[], None);
        submit_scenario(&mut w, &[(AMOUNT, lit(int(0)))]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        assert_eq!(
            failure_of(&result),
            &ExecutionFailure::PreconditionRejected {
                operation_ref: id(SUBMIT),
                index: 1
            }
        );
    }

    #[test]
    fn prose_preconditions_are_undecidable_not_crashes() {
        let mut w = submit_world(&["the request must be complete", "amount > 0"], &[], None);
        submit_scenario(&mut w, &[(AMOUNT, lit(int(5)))]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        assert_eq!(missing_of(&result), pairs(&[(SUBMIT, "precondition:0")]));
        let ExecutionResult::Undecidable { missing, .. } = &result else {
            unreachable!()
        };
        assert_eq!(missing[0].kind, MissingSemanticKind::UnsupportedExpression);
    }

    #[test]
    fn injected_predicate_answers_direct_manager() {
        let mut w = submit_world(&["direct_manager(request, request)"], &[], None);
        w.scenario(
            given_states(&[(AMOUNT, lit(int(1)))], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
        );
        let inputs = || scopes(vec![scope(SUBMIT, &[(REQUEST, "request")])]);
        let cal = calendars(&[]);
        let yes = Managers(true);
        let result = try_run(&w, inputs(), &runtime(&cal, &yes, None, Vec::new()))
            .unwrap()
            .result;
        executed(&result);
        let result = try_run(&w, inputs(), &runtime(&cal, &NO_MANAGERS, None, Vec::new()))
            .unwrap()
            .result;
        assert!(matches!(
            failure_of(&result),
            ExecutionFailure::PreconditionRejected { .. }
        ));
    }

    #[test]
    fn outcomes_are_selected_only_when_deterministic() {
        let mut w = submit_world(&[], &[], None);
        w.add(outcome("outcome:accepted"))
            .link(RelationKind::Produces, SUBMIT, "outcome:accepted");
        submit_scenario(&mut w, &[]);
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(executed(&result).1, &[id("outcome:accepted")]);

        w.add(outcome("outcome:rejected"))
            .link(RelationKind::Produces, SUBMIT, "outcome:rejected");
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(missing_of(&result), pairs(&[(SUBMIT, "outcome_selection")]));
    }

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

    fn when(entries: &[(&str, &str)]) -> Value {
        let selections: Vec<Value> = entries
            .iter()
            .map(|(op, w)| json!({"operation_ref": op, "when": w}))
            .collect();
        json!({"version": 1, "selections": selections})
    }

    #[test]
    fn multiple_outcomes_select_deterministically() {
        // H0.4 (architecture §33.4): success/partial candidates; business and technical
        // failures are never selected on success.
        let mut w = submit_world(&[], &[], None);
        w.add(outcome_of("outcome:large", OutcomeKind::Success, None))
            .add(outcome_of(
                "outcome:declined",
                OutcomeKind::BusinessFailure,
                Some(json!({"version": 1, "selections": [
                    {"operation_ref": SUBMIT, "on_failures": ["precondition_rejected"]}]})),
            ))
            .add(outcome_of(
                "outcome:crash",
                OutcomeKind::TechnicalFailure,
                None,
            ));
        for o in ["outcome:large", "outcome:declined", "outcome:crash"] {
            w.link(RelationKind::Produces, SUBMIT, o);
        }
        submit_scenario(&mut w, &[(AMOUNT, lit(int(50)))]);
        let inputs = || scopes(vec![amount_scope(SUBMIT)]);
        // One candidate without a selection entry is selected as before.
        assert_eq!(executed(&execute(&w, inputs())).1, &[id("outcome:large")]);
        // Several candidates: each needs a `when` entry; exactly one true selects it.
        w.add(outcome_of(
            "outcome:large",
            OutcomeKind::Success,
            Some(when(&[(SUBMIT, "amount > 10")])),
        ))
        .add(outcome_of(
            "outcome:small",
            OutcomeKind::Partial,
            Some(when(&[(SUBMIT, "amount <= 10")])),
        ))
        .link(RelationKind::Produces, SUBMIT, "outcome:small");
        let result = execute(&w, inputs());
        assert_eq!(executed(&result).1, &[id("outcome:large")]);
        let selected = |r: &ExecutionResult| {
            trace_of(r)
                .iter()
                .filter(|s| matches!(s, ExecutionTraceStep::OutcomeSelected { .. }))
                .count()
        };
        assert_eq!(selected(&result), 1);
        submit_scenario(&mut w, &[(AMOUNT, lit(int(3)))]);
        assert_eq!(executed(&execute(&w, inputs())).1, &[id("outcome:small")]);
        // Identical under reverse insertion.
        let graph = w.reversed_graph();
        let model = build_execution_model(&graph, inputs()).unwrap();
        let cal = calendars(&[]);
        let rt = runtime(&cal, &NO_MANAGERS, None, Vec::new());
        assert_eq!(
            run(&model, &id(SCN), &rt).unwrap().result,
            execute(&w, inputs())
        );
        // Two true, or none true: Undecidable with OUTCOME_SELECTION.
        let mut both = w.clone();
        both.add(outcome_of(
            "outcome:small",
            OutcomeKind::Partial,
            Some(when(&[(SUBMIT, "amount > 0")])),
        ));
        submit_scenario(&mut both, &[(AMOUNT, lit(int(50)))]);
        assert_eq!(
            missing_of(&execute(&both, inputs())),
            pairs(&[(SUBMIT, "outcome_selection")])
        );
        let mut neither = w.clone();
        neither.add(outcome_of(
            "outcome:small",
            OutcomeKind::Partial,
            Some(when(&[(SUBMIT, "amount > 100")])),
        ));
        assert_eq!(
            missing_of(&execute(&neither, inputs())),
            pairs(&[(SUBMIT, "outcome_selection")])
        );
        // A missing entry for this Operation is Undecidable, never a guess.
        let mut unlisted = w.clone();
        unlisted.add(outcome_of(
            "outcome:small",
            OutcomeKind::Partial,
            Some(when(&[("op:other", "amount > 100")])),
        ));
        unlisted.add(operation("op:other", &[], &[]));
        assert_eq!(
            missing_of(&execute(&unlisted, inputs())),
            pairs(&[(SUBMIT, "outcome_selection")])
        );
        // Without the Operation's expression scope the conditions cannot be evaluated.
        assert_eq!(
            missing_of(&execute(&w, ExecutionInputs::default())),
            pairs(&[(SUBMIT, "expression_scope")])
        );
        // A `when` that does not type-check as Bool is a build error.
        let mut ill_typed = w.clone();
        ill_typed.add(outcome_of(
            "outcome:small",
            OutcomeKind::Partial,
            Some(when(&[(SUBMIT, "amount + 1")])),
        ));
        let graph = ill_typed.graph();
        assert!(matches!(
            build_execution_model(&graph, inputs()).err().unwrap(),
            SimulationError::InvalidOutcomeSelection { .. }
        ));
        // A malformed selection value never selects: Undecidable for the dependent run only.
        let mut malformed = w.clone();
        malformed.add(outcome_of(
            "outcome:small",
            OutcomeKind::Partial,
            Some(json!({"version": 2, "selections": []})),
        ));
        assert_eq!(
            missing_of(&execute(&malformed, inputs())),
            pairs(&[(SUBMIT, "outcome_selection")])
        );
    }

    #[test]
    fn business_rejections_select_their_modeled_outcome() {
        let mut w = submit_world(&["amount > 0"], &[], None);
        w.add(outcome_of(
            "outcome:declined",
            OutcomeKind::BusinessFailure,
            Some(json!({"version": 1, "selections": [
                {"operation_ref": SUBMIT, "on_failures": ["authorization_denied", "precondition_rejected"]}]})),
        ))
        .add(outcome_of("outcome:accepted", OutcomeKind::Success, None))
        .add(event("event:submitted"));
        for o in ["outcome:declined", "outcome:accepted", "event:submitted"] {
            w.link(RelationKind::Produces, SUBMIT, o);
        }
        submit_scenario(&mut w, &[(AMOUNT, lit(int(0)))]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        let ExecutionResult::Rejected {
            failure,
            outcome_ref,
            state,
            trace,
            ..
        } = &result
        else {
            panic!("expected Rejected, got {result:?}");
        };
        assert_eq!(
            failure,
            &ExecutionFailure::PreconditionRejected {
                operation_ref: id(SUBMIT),
                index: 0
            }
        );
        assert_eq!(outcome_ref, &id("outcome:declined"));
        assert_eq!(state.active_states, BTreeSet::from([id(DRAFT)]));
        assert_eq!(state.attributes[&id(AMOUNT)], int(0));
        assert!(!trace
            .iter()
            .any(|s| matches!(s, ExecutionTraceStep::EventEmitted { .. })));
        assert!(trace.contains(&ExecutionTraceStep::OutcomeSelected {
            operation_ref: id(SUBMIT),
            outcome_ref: id("outcome:declined"),
        }));
        // Model failures are never selected.
        let mut post = w.clone();
        post.add(operation(SUBMIT, &["amount > 0"], &["amount > 100"]));
        submit_scenario(&mut post, &[(AMOUNT, lit(int(5)))]);
        assert!(matches!(
            failure_of(&execute(&post, scopes(vec![amount_scope(SUBMIT)]))),
            ExecutionFailure::PostconditionViolation { .. }
        ));
    }

    #[test]
    fn unparseable_expressions_make_only_dependent_scenarios_undecidable() {
        // H0.4 [Fable #119]: prose at every site compiles to Unparseable under an explicit
        // scope; type errors in parseable expressions stay build errors.
        let mut w = submit_world(&[], &[], Some("the manager agrees"));
        w.add(rule(RULE, Some("the request is complete"), None))
            .add(invariant("invariant:limit", REQUEST, "amounts are bounded"))
            .add(entity("entity:other", "Other"))
            .add(operation("op:other", &[], &[]));
        w.scenario(
            given(&[(AMOUNT, lit(int(1)))]),
            ScenarioWhen::Rule(id(RULE)),
        );
        let inputs = || {
            scopes(vec![
                amount_scope(TR_SUBMIT),
                amount_scope(RULE),
                amount_scope("invariant:limit"),
            ])
        };
        let graph = w.graph();
        build_execution_model(&graph, inputs()).unwrap();
        let result = execute(&w, inputs());
        let ExecutionResult::Undecidable { missing, .. } = &result else {
            panic!("expected Undecidable, got {result:?}");
        };
        assert_eq!(
            missing
                .iter()
                .map(|m| (m.owner_ref.to_string(), m.key.clone(), m.kind))
                .collect::<Vec<_>>(),
            vec![(
                RULE.to_owned(),
                "rule_condition".to_owned(),
                MissingSemanticKind::UnsupportedExpression
            )]
        );
        submit_scenario(&mut w, &[]);
        assert_eq!(
            missing_of(&execute(&w, inputs())),
            pairs(&[(TR_SUBMIT, "transition_guard")])
        );
        // An independent Scenario still executes.
        w.scenario(given(&[]), ScenarioWhen::Operation(id("op:other")));
        executed(&execute(&w, inputs()));
        // The invariant applies to an affected request: only that Scenario is Undecidable.
        let mut inv = request_world();
        inv.add(operation(SUBMIT, &[], &[]))
            .add(invariant("invariant:limit", REQUEST, "amounts are bounded"))
            .link(RelationKind::Writes, SUBMIT, AMOUNT)
            .link(RelationKind::Writes, SUBMIT, REQUEST);
        inv.scenario(given(&[]), ScenarioWhen::Operation(id(SUBMIT)));
        assert_eq!(
            missing_of(&execute(
                &inv,
                scopes(vec![amount_scope("invariant:limit")])
            )),
            pairs(&[("invariant:limit", "invariant")])
        );
    }

    #[test]
    fn produced_events_are_emitted_sorted_and_only_on_success() {
        let mut w = submit_world(&["amount > 0"], &[], None);
        for e in ["event:z-submitted", "event:a-logged"] {
            w.add(event(e)).link(RelationKind::Produces, SUBMIT, e);
        }
        submit_scenario(&mut w, &[(AMOUNT, lit(int(1)))]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        assert_eq!(
            executed(&result).2,
            &[id("event:a-logged"), id("event:z-submitted")]
        );
        let tail: Vec<&ExecutionTraceStep> = trace_of(&result).iter().rev().take(3).collect();
        assert_eq!(
            tail,
            vec![
                &ExecutionTraceStep::ExecutionCompleted,
                &ExecutionTraceStep::EventEmitted {
                    event_ref: id("event:z-submitted")
                },
                &ExecutionTraceStep::EventEmitted {
                    event_ref: id("event:a-logged")
                },
            ]
        );
        submit_scenario(&mut w, &[(AMOUNT, lit(int(0)))]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        assert!(!trace_of(&result)
            .iter()
            .any(|s| matches!(s, ExecutionTraceStep::EventEmitted { .. })));
    }

    #[test]
    fn uses_calculation_runs_the_calculation() {
        let mut w = submit_world(&[], &[], None);
        w.add(calculation(
            CALC_A,
            "amount * 3",
            ("Int", None),
            None,
            &[(AMOUNT, "amount")],
            None,
            true,
        ))
        .link(RelationKind::UsesCalculation, SUBMIT, CALC_A);
        submit_scenario(&mut w, &[(AMOUNT, lit(int(2)))]);
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(calc_result(&result, CALC_A), int(6));
    }

    #[test]
    fn writes_edges_are_footprint_only() {
        let mut w = submit_world(&[], &[], None);
        w.link(RelationKind::Writes, SUBMIT, AMOUNT)
            .link(RelationKind::Writes, SUBMIT, APPROVED)
            .link(RelationKind::Reads, SUBMIT, FEE);
        submit_scenario(&mut w, &[(AMOUNT, lit(int(5)))]);
        let result = execute(&w, ExecutionInputs::default());
        let (state, ..) = executed(&result);
        assert_eq!(
            state.attributes,
            BTreeMap::from([(id(AMOUNT), int(5))]),
            "no attribute is written or invented"
        );
        assert!(
            trace_of(&result).contains(&ExecutionTraceStep::FootprintDeclared {
                operation_ref: id(SUBMIT),
                reads: vec![id(FEE)],
                writes: vec![id(AMOUNT), id(APPROVED)],
            })
        );
    }

    #[test]
    fn postconditions_assert_without_mutating() {
        let mut w = submit_world(&[], &["amount > 0"], None);
        w.add(event("event:submitted"))
            .link(RelationKind::Produces, SUBMIT, "event:submitted");
        submit_scenario(&mut w, &[(AMOUNT, lit(int(1)))]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        assert!(
            trace_of(&result).contains(&ExecutionTraceStep::PostconditionEvaluated {
                operation_ref: id(SUBMIT),
                index: 0,
                result: true
            })
        );
        let (state, _, events, _) = executed(&result);
        assert_eq!(events, &[id("event:submitted")]);
        assert_eq!(state.attributes, BTreeMap::from([(id(AMOUNT), int(1))]));

        submit_scenario(&mut w, &[(AMOUNT, lit(int(0)))]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        assert_eq!(
            failure_of(&result),
            &ExecutionFailure::PostconditionViolation {
                operation_ref: id(SUBMIT),
                index: 0
            }
        );
        assert!(!trace_of(&result)
            .iter()
            .any(|s| matches!(s, ExecutionTraceStep::EventEmitted { .. })));
    }

    // ------------------------------------------------------------------ rules

    #[test]
    fn rule_conditions_are_recorded() {
        for (amount, expected) in [(20, true), (5, false)] {
            let mut w = request_world();
            w.add(rule(RULE, Some("amount > 10"), None));
            w.scenario(
                given(&[(AMOUNT, lit(int(amount)))]),
                ScenarioWhen::Rule(id(RULE)),
            );
            let result = execute(&w, scopes(vec![amount_scope(RULE)]));
            assert_eq!(executed(&result).3, &BTreeMap::from([(id(RULE), expected)]));
        }
    }

    #[test]
    fn rules_without_scope_value_or_condition_are_undecidable() {
        let mut w = request_world();
        w.add(rule(RULE, Some("amount > 10"), None));
        w.scenario(given(&[]), ScenarioWhen::Rule(id(RULE)));
        assert_eq!(
            missing_of(&execute(&w, ExecutionInputs::default())),
            pairs(&[(RULE, "expression_scope")])
        );
        assert_eq!(
            missing_of(&execute(&w, scopes(vec![amount_scope(RULE)]))),
            pairs(&[(AMOUNT, "value")])
        );
        w.add(rule(RULE, None, Some("approved = true")));
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(missing_of(&result), pairs(&[(RULE, "condition_expr")]));
    }

    #[test]
    fn rule_actions_are_never_executed() {
        let mut w = request_world();
        w.add(rule(RULE, Some("amount > 10"), Some("amount = 0")));
        w.scenario(
            given(&[(AMOUNT, lit(int(20)))]),
            ScenarioWhen::Rule(id(RULE)),
        );
        let result = execute(&w, scopes(vec![amount_scope(RULE)]));
        let (state, ..) = executed(&result);
        assert_eq!(state.attributes[&id(AMOUNT)], int(20));
        assert!(trace_of(&result)
            .contains(&ExecutionTraceStep::RuleActionNotExecuted { rule_ref: id(RULE) }));
    }

    #[test]
    fn false_governing_rules_are_recorded_not_enforced() {
        let mut w = submit_world(&[], &[], None);
        w.add(rule(RULE, Some("amount > 10"), None))
            .link(RelationKind::GovernedBy, SUBMIT, RULE);
        submit_scenario(&mut w, &[(AMOUNT, lit(int(1)))]);
        let result = execute(&w, scopes(vec![amount_scope(RULE)]));
        let (state, _, _, rules) = executed(&result);
        assert_eq!(rules, &BTreeMap::from([(id(RULE), false)]));
        assert_eq!(state.active_states, BTreeSet::from([id(SUBMITTED)]));
    }

    #[test]
    fn governing_decision_table_uses_the_given_inputs() {
        let mut w = submit_world(&[], &[], None);
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
        ))
        .link(RelationKind::GovernedBy, SUBMIT, TABLE);
        let mut g = given_states(&[], &[DRAFT]);
        for (i, v) in standard_inputs(3, "0.50") {
            g.decision_inputs.insert(i, lit(v));
        }
        w.scenario(g, ScenarioWhen::Operation(id(SUBMIT)));
        let result = execute(&w, table_inputs(table_spec("FIRST", vec![(any4(), 7)])));
        assert_eq!(table_output(&result), (7, Some(0)));
    }

    // ------------------------------------------------------------------ transitions

    #[test]
    fn direct_transitions_check_source_and_guard() {
        let mut w = submit_world(&[], &[], Some("amount > 0"));
        let guard_scope = || scopes(vec![amount_scope(TR_SUBMIT)]);
        w.scenario(
            given_states(&[(AMOUNT, lit(int(1)))], &[DRAFT]),
            ScenarioWhen::Transition(id(TR_SUBMIT)),
        );
        let result = execute(&w, guard_scope());
        assert_eq!(
            executed(&result).0.active_states,
            BTreeSet::from([id(SUBMITTED)])
        );
        assert!(
            trace_of(&result).contains(&ExecutionTraceStep::TransitionTriggerRecorded {
                transition_ref: id(TR_SUBMIT),
                trigger_ref: Some(id(SUBMIT)),
            })
        );

        w.scenario(
            given_states(&[(AMOUNT, lit(int(0)))], &[DRAFT]),
            ScenarioWhen::Transition(id(TR_SUBMIT)),
        );
        assert_eq!(
            failure_of(&execute(&w, guard_scope())),
            &ExecutionFailure::TransitionGuardRejected {
                transition_ref: id(TR_SUBMIT)
            }
        );

        w.scenario(
            given_states(&[], &[DRAFT]),
            ScenarioWhen::Transition(id(TR_SUBMIT)),
        );
        assert_eq!(
            missing_of(&execute(&w, guard_scope())),
            pairs(&[(AMOUNT, "value")])
        );

        w.scenario(
            given_states(&[(AMOUNT, lit(int(1)))], &[SUBMITTED]),
            ScenarioWhen::Transition(id(TR_SUBMIT)),
        );
        assert_eq!(
            failure_of(&execute(&w, guard_scope())),
            &ExecutionFailure::InvalidSourceState {
                transition_ref: id(TR_SUBMIT),
                from_state: id(DRAFT)
            }
        );
    }

    #[test]
    fn operation_triggered_transition_runs_after_operation_semantics() {
        let mut w = submit_world(&["amount > 0"], &[], None);
        submit_scenario(&mut w, &[(AMOUNT, lit(int(1)))]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        let trace = trace_of(&result);
        let at = |pred: &dyn Fn(&ExecutionTraceStep) -> bool| trace.iter().position(pred).unwrap();
        assert!(
            at(&|s| matches!(s, ExecutionTraceStep::PreconditionEvaluated { .. }))
                < at(&|s| matches!(s, ExecutionTraceStep::TransitionApplied { .. }))
        );
        assert_eq!(
            executed(&result).0.active_states,
            BTreeSet::from([id(SUBMITTED)])
        );
    }

    #[test]
    fn event_anchor_runs_event_triggered_transitions() {
        let mut w = request_world();
        w.add(event(APPROVED_EVENT))
            .add(transition(TR_CLOSE, SUBMITTED, CLOSED, None))
            .link(RelationKind::TransitionsVia, TR_CLOSE, APPROVED_EVENT);
        w.scenario(
            given_states(&[], &[SUBMITTED]),
            ScenarioWhen::Event(id(APPROVED_EVENT)),
        );
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(
            executed(&result).0.active_states,
            BTreeSet::from([id(CLOSED)])
        );
        assert!(
            trace_of(&result).contains(&ExecutionTraceStep::EventOccurred {
                event_ref: id(APPROVED_EVENT)
            })
        );
        // An inactive source state means no transition and no invented effect.
        w.scenario(
            given_states(&[], &[DRAFT]),
            ScenarioWhen::Event(id(APPROVED_EVENT)),
        );
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(
            executed(&result).0.active_states,
            BTreeSet::from([id(DRAFT)])
        );
    }

    #[test]
    fn conflicting_transitions_make_only_that_scenario_undecidable() {
        // H0.4 [Fable #116]: a content-dependent model condition is a per-scenario typed
        // Undecidable; a Scenario whose state never reaches the conflict still executes.
        let mut w = submit_world(&[], &[], None);
        w.add(transition("transition:submit-close", DRAFT, CLOSED, None))
            .link(
                RelationKind::TransitionsVia,
                "transition:submit-close",
                SUBMIT,
            );
        submit_scenario(&mut w, &[]);
        let result = execute(&w, ExecutionInputs::default());
        assert_eq!(
            conditions_of(&result),
            pairs(&[(DRAFT, "conflicting_transitions")])
        );
        w.scenario(
            given_states(&[], &[SUBMITTED]),
            ScenarioWhen::Operation(id(SUBMIT)),
        );
        executed(&execute(&w, ExecutionInputs::default()));
    }

    #[test]
    fn several_governing_tables_route_inputs_or_report_a_typed_condition() {
        // H0.4 [Fable #117]: the Given decision inputs name no table, so with several
        // governing DecisionTables their routing is ambiguous: a dedicated typed condition,
        // never silently discarded inputs.
        let spec = table_spec("FIRST", vec![(any4(), 1)]);
        let mut w = table_world("FIRST", &standard_inputs(3, "0.50"));
        w.add(node(
            "decision-table:other",
            Accepted,
            NodePayload::DecisionTable(DecisionTable {
                name: "Other".into(),
                hit_policy: "FIRST".into(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                rows: Vec::new(),
            }),
        ))
        .add(operation(SUBMIT, &[], &[]))
        .link(RelationKind::GovernedBy, SUBMIT, TABLE)
        .link(RelationKind::GovernedBy, SUBMIT, "decision-table:other");
        let mut g = ScenarioGiven::default();
        for (index, literal) in standard_inputs(3, "0.50") {
            g.decision_inputs.insert(index, lit(literal));
        }
        w.scenario(g, ScenarioWhen::Operation(id(SUBMIT)));
        let inputs = ExecutionInputs {
            decision_tables: vec![
                ScenarioDecisionTableInput {
                    decision_table_ref: id(TABLE),
                    spec: spec.clone(),
                },
                ScenarioDecisionTableInput {
                    decision_table_ref: id("decision-table:other"),
                    spec: spec.clone(),
                },
            ],
            ..ExecutionInputs::default()
        };
        let result = execute(&w, inputs.clone());
        assert_eq!(
            conditions_of(&result),
            pairs(&[(SUBMIT, "decision_input_routing")])
        );
        // One governing table receives every input, as before.
        let mut single = w.clone();
        single.edges.retain(|e| e.to != id("decision-table:other"));
        let result = execute(&single, inputs);
        assert_eq!(executed(&result).0.decision_outputs[&id(TABLE)][&0], int(1));
    }

    // ------------------------------------------------------------------ invariants

    #[test]
    fn invariants_apply_to_affected_elements_only() {
        let mut w = submit_world(&[], &[], None);
        w.add(invariant("invariant:limit", REQUEST, "amount < 100"))
            .add(entity("entity:other", "Other"))
            .add(invariant("invariant:unrelated", "entity:other", "false"));
        let inputs = || {
            scopes(vec![
                amount_scope("invariant:limit"),
                scope("invariant:unrelated", &[]),
            ])
        };
        submit_scenario(&mut w, &[(AMOUNT, lit(int(5)))]);
        let result = execute(&w, inputs());
        executed(&result);
        assert!(
            trace_of(&result).contains(&ExecutionTraceStep::InvariantEvaluated {
                invariant_ref: id("invariant:limit"),
                result: true
            })
        );
        assert!(!trace_of(&result).iter().any(|s| matches!(
            s,
            ExecutionTraceStep::InvariantEvaluated { invariant_ref, .. } if invariant_ref == &id("invariant:unrelated")
        )));

        submit_scenario(&mut w, &[(AMOUNT, lit(int(150)))]);
        assert_eq!(
            failure_of(&execute(&w, inputs())),
            &ExecutionFailure::InvariantViolation {
                invariant_ref: id("invariant:limit")
            }
        );

        submit_scenario(&mut w, &[]);
        assert_eq!(
            missing_of(&execute(&w, inputs())),
            pairs(&[(AMOUNT, "value")])
        );
    }

    // ------------------------------------------------------------------ authorization

    const CLERK: &str = "security-role:clerk";
    const SENIOR: &str = "security-role:senior";
    const PERMISSION: &str = "permission:submit";
    const SCOPE: &str = "resource-scope:own";
    const ALICE: &str = "principal:alice";
    const BOB: &str = "principal:bob";

    fn authorization_world() -> World {
        let mut w = submit_world(&[], &[], None);
        w.add(node(
            CLERK,
            Accepted,
            NodePayload::SecurityRole(SecurityRole {
                name: "Clerk".into(),
                description: None,
            }),
        ))
        .add(node(
            SENIOR,
            Accepted,
            NodePayload::SecurityRole(SecurityRole {
                name: "Senior".into(),
                description: None,
            }),
        ))
        .add(node(
            PERMISSION,
            Accepted,
            NodePayload::Permission(Permission {
                name: "Submit".into(),
            }),
        ))
        .add(node(
            SCOPE,
            Accepted,
            NodePayload::ResourceScope(ResourceScope {
                resource_ref: id(REQUEST),
                scope_kind: "own".into(),
            }),
        ));
        for p in [ALICE, BOB] {
            w.add(node(
                p,
                Accepted,
                NodePayload::Principal(Principal {
                    name: p.into(),
                    principal_kind: "user".into(),
                }),
            ));
        }
        w.add(node(
            "business-role:clerk",
            Accepted,
            NodePayload::BusinessRole(BusinessRole {
                name: "Clerk".into(),
            }),
        ))
        .add(node(
            "actor:carol",
            Accepted,
            NodePayload::Actor(plumb_psg::Actor {
                name: "Carol".into(),
                actor_kind: ActorKind::Human,
            }),
        ))
        .link(RelationKind::Grants, CLERK, PERMISSION)
        .link(RelationKind::Permits, PERMISSION, SUBMIT)
        .link(RelationKind::ScopedTo, PERMISSION, SCOPE)
        .link(RelationKind::InheritsRole, SENIOR, CLERK)
        .link(RelationKind::AssignedRole, ALICE, SENIOR)
        .link(RelationKind::PerformedBy, SUBMIT, "business-role:clerk")
        .link(RelationKind::PerformedBy, SUBMIT, "actor:carol");
        submit_scenario(&mut w, &[]);
        w
    }

    fn as_actor(w: &World, actor: Option<&str>) -> Result<ExecutionResult, SimulationError> {
        let cal = calendars(&[]);
        try_run(
            w,
            ExecutionInputs::default(),
            &runtime(&cal, &NO_MANAGERS, actor, Vec::new()),
        )
        .map(|r| r.result)
    }

    #[test]
    fn authorization_allows_through_inherited_security_roles() {
        let result = as_actor(&authorization_world(), Some(ALICE)).unwrap();
        executed(&result);
        assert!(
            trace_of(&result).contains(&ExecutionTraceStep::AuthorizationChecked {
                operation_ref: id(SUBMIT),
                actor_ref: Some(id(ALICE)),
                decision: AuthorizationDecision::Allowed {
                    permission_refs: vec![id(PERMISSION)],
                    unevaluated_scope_refs: vec![id(SCOPE)],
                },
            })
        );
    }

    #[test]
    fn authorization_denies_and_needs_a_runtime_actor() {
        let w = authorization_world();
        assert_eq!(
            failure_of(&as_actor(&w, Some(BOB)).unwrap()),
            &ExecutionFailure::AuthorizationDenied {
                operation_ref: id(SUBMIT),
                actor_ref: id(BOB)
            }
        );
        assert_eq!(
            missing_of(&as_actor(&w, None).unwrap()),
            pairs(&[(SUBMIT, "runtime_actor")])
        );
    }

    #[test]
    fn performers_and_business_roles_are_not_security_authorization() {
        let w = authorization_world();
        // Carol performs the Operation but holds no SecurityRole.
        assert!(matches!(
            failure_of(&as_actor(&w, Some("actor:carol")).unwrap()),
            ExecutionFailure::AuthorizationDenied { .. }
        ));
        // A BusinessRole is not a runtime security principal.
        assert!(matches!(
            as_actor(&w, Some("business-role:clerk")).unwrap_err(),
            SimulationError::InvalidRuntimeActor { .. }
        ));
        let result = as_actor(&w, Some(ALICE)).unwrap();
        assert!(
            trace_of(&result).contains(&ExecutionTraceStep::PerformerRecorded {
                operation_ref: id(SUBMIT),
                performer_refs: vec![id("actor:carol"), id("business-role:clerk")],
            })
        );
    }

    #[test]
    fn conditioned_permissions_are_not_evaluated() {
        let mut w = authorization_world();
        w.add(node(
            "policy-condition:office-hours",
            Accepted,
            NodePayload::PolicyCondition(PolicyCondition {
                expression: "during office hours".into(),
            }),
        ))
        .link(
            RelationKind::ConditionedBy,
            PERMISSION,
            "policy-condition:office-hours",
        );
        assert_eq!(
            missing_of(&as_actor(&w, Some(ALICE)).unwrap()),
            pairs(&[(PERMISSION, "policy_condition")])
        );
    }

    // ------------------------------------------------------------------ assumptions

    #[test]
    fn assumption_default_values_are_never_used() {
        let mut w = submit_world(&["amount > 0"], &[], None);
        // Assumption.owner_ref names an Agent or Stakeholder (graph-bound, architecture §33.11).
        w.add(node(
            "agent:owner",
            Accepted,
            NodePayload::Agent(plumb_psg::Agent {
                agent_kind: plumb_psg::AgentKind::Human,
            }),
        ));
        w.add(node(
            "assumption:amount",
            Accepted,
            NodePayload::Assumption(Assumption {
                statement: "Amounts default to 123".into(),
                owner_ref: id("agent:owner"),
                status: "Accepted".into(),
                finding_ref: None,
                default_value: Some(json!(123)),
                expires_at: None,
                risk_ref: None,
            }),
        ));
        submit_scenario(&mut w, &[]);
        let result = execute(&w, scopes(vec![amount_scope(SUBMIT)]));
        assert_eq!(missing_of(&result), pairs(&[(AMOUNT, "value")]));
        for source in PRODUCTION {
            assert!(!source.contains("default_value"));
            assert!(!source.contains("NodePayload::Assumption"));
        }
    }

    // ------------------------------------------------------------------ admission

    #[test]
    fn only_accepted_confirmed_scenarios_run() {
        let cal = calendars(&[]);
        let rt = runtime(&cal, &NO_MANAGERS, None, Vec::new());
        let mut w = submit_world(&[], &[], None);
        let g = || given_states(&[], &[DRAFT]);
        let when = || ScenarioWhen::Operation(id(SUBMIT));
        w.scenario_with(g(), when(), Accepted, Some("Derived"));
        assert!(matches!(
            try_run(&w, ExecutionInputs::default(), &rt).unwrap_err(),
            SimulationError::ScenarioNotConfirmed { .. }
        ));
        w.scenario_with(g(), when(), Accepted, None);
        assert!(matches!(
            try_run(&w, ExecutionInputs::default(), &rt).unwrap_err(),
            SimulationError::ScenarioNotConfirmed { .. }
        ));
        w.scenario_with(g(), when(), Proposed, Some("Confirmed"));
        assert!(matches!(
            try_run(&w, ExecutionInputs::default(), &rt).unwrap_err(),
            SimulationError::ScenarioNotAccepted { .. }
        ));
        w.scenario_with(g(), when(), Accepted, Some("Confirmed"));
        let graph = w.graph();
        let model = build_execution_model(&graph, ExecutionInputs::default()).unwrap();
        assert!(matches!(
            run(&model, &id(SUBMIT), &rt).unwrap_err(),
            SimulationError::NotAScenario { .. }
        ));
        let first = run(&model, &id(SCN), &rt).unwrap();
        assert!(matches!(first.result, ExecutionResult::Executed { .. }));
        assert_eq!(model.project_id(), &id(PROJECT));
        assert_eq!(
            model.semantic_hash(),
            &plumb_psg::semantic_hash(&graph).unwrap()
        );
    }

    #[test]
    fn specification_hash_identifies_the_scenario_payload() {
        let cal = calendars(&[]);
        let rt = runtime(&cal, &NO_MANAGERS, None, Vec::new());
        let mut w = submit_world(&[], &[], None);
        submit_scenario(&mut w, &[(AMOUNT, lit(int(1)))]);
        let a = try_run(&w, ExecutionInputs::default(), &rt).unwrap();
        let b = try_run(&w, ExecutionInputs::default(), &rt).unwrap();
        assert_eq!(a.specification_hash, b.specification_hash);
        let actor_rt = runtime(&cal, &NO_MANAGERS, Some(ALICE), Vec::new());
        let with_actor = try_run(&w, ExecutionInputs::default(), &actor_rt).unwrap();
        assert_eq!(a.specification_hash, with_actor.specification_hash);
        submit_scenario(&mut w, &[(AMOUNT, lit(int(2)))]);
        let c = try_run(&w, ExecutionInputs::default(), &rt).unwrap();
        assert_ne!(a.specification_hash, c.specification_hash);
        assert!(a.specification_hash.to_string().starts_with("sha256:"));
        let _: Hash = c.specification_hash;
    }

    // ------------------------------------------------------------------ model validation

    #[test]
    fn expression_scopes_are_validated_at_build_time() {
        let mut w = submit_world(&[], &[], Some("amount"));
        w.add(invariant("invariant:limit", REQUEST, "amount + 1"));
        submit_scenario(&mut w, &[]);
        let graph = w.graph();
        let build = |items: Vec<ExecutionExpressionScopeInput>| {
            build_execution_model(&graph, scopes(items)).err()
        };
        assert!(matches!(
            build(vec![amount_scope(TR_SUBMIT)]).unwrap(),
            SimulationError::InvalidExpression {
                site: ExpressionSite::TransitionGuard,
                ..
            }
        ));
        assert!(matches!(
            build(vec![amount_scope("invariant:limit")]).unwrap(),
            SimulationError::InvalidExpression {
                site: ExpressionSite::Invariant,
                ..
            }
        ));
        assert!(matches!(
            build(vec![amount_scope(SUBMIT), amount_scope(SUBMIT)]).unwrap(),
            SimulationError::DuplicateInput { .. }
        ));
        assert!(matches!(
            build(vec![amount_scope(AMOUNT)]).unwrap(),
            SimulationError::InvalidInput { .. }
        ));
        assert!(matches!(
            build(vec![scope(SUBMIT, &[("attr:unknown", "x")])]).unwrap(),
            SimulationError::InvalidInput { .. }
        ));
        assert!(matches!(
            build(vec![scope(SUBMIT, &[(AMOUNT, "calendar")])]).unwrap(),
            SimulationError::InvalidInput { .. }
        ));
    }

    // ------------------------------------------------------------------ determinism

    fn rich_world() -> World {
        let mut w = submit_world(&["amount > 0"], &["amount > 0"], Some("approved"));
        w.add(event("event:submitted"))
            .link(RelationKind::Produces, SUBMIT, "event:submitted")
            .add(outcome("outcome:accepted"))
            .link(RelationKind::Produces, SUBMIT, "outcome:accepted")
            .add(rule(RULE, Some("amount > 10"), Some("approved = false")))
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
            .link(RelationKind::UsesCalculation, SUBMIT, CALC_A)
            .link(RelationKind::Writes, SUBMIT, APPROVED)
            .add(invariant("invariant:limit", REQUEST, "amount < 100"));
        submit_scenario(
            &mut w,
            &[(AMOUNT, lit(int(20))), (APPROVED, lit(boolean(true)))],
        );
        w
    }

    fn rich_inputs(reverse: bool) -> ExecutionInputs {
        let mut items = vec![
            amount_scope(SUBMIT),
            amount_scope(RULE),
            amount_scope(TR_SUBMIT),
            amount_scope("invariant:limit"),
        ];
        if reverse {
            items.reverse();
            for s in &mut items {
                s.bindings.reverse();
            }
        }
        scopes(items)
    }

    #[test]
    fn execution_is_deterministic_under_reverse_insertion() {
        let w = rich_world();
        let cal = calendars(&[]);
        let rt = runtime(&cal, &NO_MANAGERS, None, Vec::new());
        let forward_graph = w.graph();
        let reverse_graph = w.reversed_graph();
        let forward_model = build_execution_model(&forward_graph, rich_inputs(false)).unwrap();
        let reverse_model = build_execution_model(&reverse_graph, rich_inputs(true)).unwrap();
        let forward = run(&forward_model, &id(SCN), &rt).unwrap();
        let reverse = run(&reverse_model, &id(SCN), &rt).unwrap();
        assert_eq!(forward, reverse);
        assert_eq!(forward, run(&forward_model, &id(SCN), &rt).unwrap());
        let (state, outcomes, events, rules) = executed(&forward.result);
        assert_eq!(state.active_states, BTreeSet::from([id(SUBMITTED)]));
        assert_eq!(state.calculation_results[&id(CALC_A)], int(40));
        assert_eq!(state.attributes[&id(APPROVED)], boolean(true));
        assert_eq!(outcomes, &[id("outcome:accepted")]);
        assert_eq!(events, &[id("event:submitted")]);
        assert_eq!(rules, &BTreeMap::from([(id(RULE), true)]));
        // Pure: the graph is unchanged by execution.
        assert_eq!(forward_graph, w.graph());
        // Undecidable missing values are sorted the same way in both orders.
        let mut undecided = w.clone();
        undecided.scenario(
            given_states(&[], &[DRAFT]),
            ScenarioWhen::Calculation(id(CALC_A)),
        );
        let ug = undecided.graph();
        let urg = undecided.reversed_graph();
        let a = run(
            &build_execution_model(&ug, rich_inputs(false)).unwrap(),
            &id(SCN),
            &rt,
        )
        .unwrap();
        let b = run(
            &build_execution_model(&urg, rich_inputs(true)).unwrap(),
            &id(SCN),
            &rt,
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn execution_result_serializes_without_free_text() {
        let w = rich_world();
        let cal = calendars(&[]);
        let rt = runtime(&cal, &NO_MANAGERS, None, Vec::new());
        let graph = w.graph();
        let model = build_execution_model(&graph, rich_inputs(false)).unwrap();
        let run = run(&model, &id(SCN), &rt).unwrap();
        let value = serde_json::to_value(&run.result).unwrap();
        assert_eq!(value["result"], "executed");
        let steps: Vec<&str> = value["trace"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["step"].as_str().unwrap())
            .collect();
        assert_eq!(
            steps,
            vec![
                "materialization_started",
                "given_materialized",
                "footprint_declared",
                "performer_recorded",
                "authorization_checked",
                "precondition_evaluated",
                "rule_evaluated",
                "rule_action_not_executed",
                "calculation_evaluated",
                "outcome_selected",
                "guard_evaluated",
                "transition_applied",
                "postcondition_evaluated",
                "invariant_evaluated",
                "event_emitted",
                "execution_completed",
            ]
        );
    }

    // ------------------------------------------------------------------ source guards

    #[test]
    fn production_sources_are_pure() {
        let forbidden = [
            "std::fs",
            "File::open",
            "rusqlite",
            "GraphStore",
            "ArtifactStore",
            "BlobStore",
            "reqwest",
            "InferenceProvider",
            "MockProvider",
            "SystemClock",
            "Utc::now",
            "Instant::now",
            "SystemTime::now",
            "OffsetDateTime::now",
            "Command",
            "std::env",
            "env::var",
            "thread_local",
            "static mut",
            "lazy_static",
            "OnceLock",
            "unsafe",
            "fixtures",
            "hr-leave",
            "expert-answers",
            "apply_patch",
            "Proposal",
            "SemanticPatch",
            "Graph::new",
            "insert_node",
            "commit",
            "branch_head",
            "HashMap",
            "HashSet",
            "StaticCalendarProvider",
            "holiday_source",
            "default_value",
            "PL-Office",
            "half_day_fraction",
            "0.5",
        ];
        for (name, source) in SOURCES.iter().zip(&PRODUCTION[1..]) {
            for token in forbidden {
                assert!(!source.contains(token), "{name} contains {token}");
            }
        }
        for token in forbidden {
            assert!(!PRODUCTION[0].contains(token), "lib.rs contains {token}");
        }
    }

    #[test]
    fn production_reuses_s41_decoding_and_never_compares_then() {
        let categories = [
            "\"attribute_values\"",
            "\"entities_exist\"",
            "\"states_active\"",
            "\"decision_inputs\"",
            "\"calculation_semantics\"",
            "\"outcomes\"",
            "\"events\"",
            "\"rules_satisfied\"",
            "\"decision_outputs\"",
            "\"calculation_results\"",
            "\"semantic_ref\"",
        ];
        let comparison = [
            "ScenarioThen",
            "semantics.then",
            ".then.",
            "from_values",
            "Diff",
            "diff:",
            "fn compare",
            "Pass",
            "NodePayload::ScenarioRun",
            "ScenarioRun {",
            "ScenarioRunResult",
        ];
        for source in PRODUCTION {
            for token in categories.iter().chain(&comparison) {
                assert!(!source.contains(token), "production contains {token}");
            }
        }
        let materialize = PRODUCTION[2];
        assert!(materialize.contains("validate_scenario(model.graph, scenario)"));
        assert!(PRODUCTION[3].contains("evaluate_calculation("));
        assert!(PRODUCTION[3].contains("effective_security_roles("));
        assert!(
            PRODUCTION[1].contains("qualify_accepted_calculation")
                || PRODUCTION[1].contains("analyze_calculation_cycles(")
        );
    }
}
