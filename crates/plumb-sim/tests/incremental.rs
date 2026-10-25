//! S4.4 contract tests for dirty-set scenario re-execution and S4 execution findings
//! (Hotfix 053).
//!
//! Every test lives in `incremental_contract` (`cargo test -p plumb-sim --test incremental`).
//! Dirty sets are supplied as finished F0.11 material; graphs, Scenarios, calendars and
//! predicates are built in memory. The finding key goldens were computed independently in Python.

mod incremental_contract {
    use std::collections::{BTreeMap, BTreeSet};
    use std::str::FromStr;

    use plumb_core::{Hash, Id, Timestamp};
    use plumb_expr::{
        CalendarDefinition, PredicateError, PredicateProvider, RefValue, StaticCalendarProvider,
    };
    use plumb_functional::{
        CalculationBindingExposure, CalculationScopeBinding, ScenarioDependency, ScenarioGiven,
        ScenarioLiteral, ScenarioThen, ScenarioValue, ScenarioWhen, CALCULATION_ORIGIN_EXTENSION,
        HOLIDAY_DATE_KEY,
    };
    use plumb_patch::DirtySet;
    use plumb_psg::{
        Attribute, AuditMeta, Calculation, Calendar, DecisionTable, Edge, ElementStatus, Entity,
        Event, ExtensionKey, Graph, Node, NodePayload, Operation, OperationKind, Outcome,
        OutcomeKind, RelationKind, RelationProperties, Rule, RuleKind, Scenario, ScenarioKind,
        State, Transition,
    };
    use plumb_psg::{
        EvidenceFragment, EvidenceLocator, Finding, FindingSeverity, Modality, Permission,
        Principal, Requirement, RequirementKind, RequirementLevel, ResourceScope, SecurityRole,
        SourceArtifact, Viewpoint,
    };
    use plumb_sim::{
        build_execution_model, ExecutionExpressionScopeInput, ExecutionInputs, ExecutionRuntime,
        RuntimeSemanticValue, ScenarioVerdict, SimulationError,
    };
    use plumb_sim::{
        rerun_dirty_scenarios, scenario_run_currency, select_dirty_scenarios, IncrementalError,
        IncrementalScenarioPolicy, IncrementalScenarioResult, IncrementalScenarioRuntime,
        ScenarioRunCurrency, SCENARIO_FAIL_CODE, SCENARIO_FAIL_CONDITION, SCENARIO_FINDING_FAMILY,
        SCENARIO_UNDECIDABLE_CODE, SCENARIO_UNDECIDABLE_CONDITION,
    };
    use plumb_validation::{finding_id, finding_key, GeneratedFinding};
    use serde_json::{json, Value};

    use ElementStatus::Accepted;

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";

    // HR calculation fixture (built in memory; no fixture file is read).
    const CAL: &str = "cal:office";
    const CALC: &str = "calculation:requested-working-days";
    const LEAVE: &str = "entity:leave";
    const START: &str = "attr:start-date";
    const END: &str = "attr:end-date";
    const INCLUSIVE: &str = "attr:inclusive-end";
    const FRACTION: &str = "attr:day-fraction";
    const HALF_DAY: &str = "half_day_fraction";

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

    const INCREMENTAL_SRC: &str = include_str!("../src/incremental.rs");

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

    const CALC_A: &str = "calculation:a";

    // ------------------------------------------------------------------ incremental fixture

    const LEAVE_BALANCE: &str = CALC;
    const CALC_UNRELATED: &str = "calculation:unrelated";
    const REQ: &str = "requirement:r-001";
    const REQ_OTHER: &str = "requirement:r-002";
    const PERMISSION: &str = "permission:submit";
    const PERMISSION_OTHER: &str = "permission:other";
    const CLERK: &str = "security-role:clerk";
    const SENIOR: &str = "security-role:senior";
    const ROLE_OTHER: &str = "security-role:other";
    const ALICE: &str = "principal:alice";
    const OTHER_OP: &str = "op:other";
    const EVENT_SUBMITTED: &str = "event:submitted";
    const EVENT_APPROVED: &str = "event:approved";
    const EVENT_OTHER: &str = "event:other";
    const TR_CLOSE: &str = "transition:close";
    const VIEWPOINT: &str = "viewpoint:overview";
    const VIEW: &str = "view:overview";

    const S_SUBMIT: &str = "scenario:submit";
    const S_HR: &str = "scenario:hr";
    const S_HOLIDAY: &str = "scenario:hr-holiday";
    const S_TABLE: &str = "scenario:table";
    const S_EVENT: &str = "scenario:event";
    const S_TRANSITION: &str = "scenario:transition";
    const S_RULE: &str = "scenario:rule";
    const S_DERIVED: &str = "scenario:derived";

    fn source_ref() -> Id {
        plumb_psg::source_artifact_id(&Hash::content_sha256(b"requirements")).unwrap()
    }

    fn locator(n: u64) -> EvidenceLocator {
        EvidenceLocator::TextRange {
            start: n * 100,
            end: n * 100 + 50,
        }
    }

    fn requirement_nodes(w: &mut World, requirements: &[(&str, u64)]) {
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
        for (r, n) in requirements {
            let fragment = plumb_psg::evidence_fragment_id(&source_ref(), &locator(*n)).unwrap();
            w.add(node(
                fragment.as_str(),
                Accepted,
                NodePayload::EvidenceFragment(EvidenceFragment {
                    source_ref: source_ref(),
                    locator: locator(*n),
                    content_hash: Hash::content_sha256(r.as_bytes()),
                    extracted_text: None,
                    speaker: None,
                    source_timestamp: None,
                }),
            ));
            let mut req = node(
                r,
                Accepted,
                NodePayload::Requirement(Requirement {
                    statement: format!("The system shall satisfy {r}."),
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
            req.evidence = vec![fragment.into()];
            w.add(req);
        }
    }

    /// Adds a Scenario with an explicit ID, status, confirmation and traceability.
    fn add_scenario(
        w: &mut World,
        scenario_ref: &str,
        given: ScenarioGiven<ScenarioValue>,
        when: ScenarioWhen,
        then: ScenarioThen<ScenarioValue>,
        confirmation: &str,
        requirement_refs: &[&str],
    ) {
        let encode = |v: &ScenarioValue| serde_json::to_value(v).unwrap();
        let when_value = when.to_value();
        let anchor = when_value.as_object().unwrap().values().next().unwrap();
        let anchor = id(anchor.as_str().unwrap());
        w.add(node(
            scenario_ref,
            Accepted,
            NodePayload::Scenario(Scenario {
                name: format!("Scenario {scenario_ref}"),
                scenario_kind: ScenarioKind::Boundary,
                given: given.to_values(encode),
                when: when_value,
                then: then.to_values(encode),
                requirement_refs: (!requirement_refs.is_empty())
                    .then(|| requirement_refs.iter().map(|r| id(r)).collect()),
                derived_from_refs: Some(vec![anchor]),
                confirmation_status: Some(confirmation.into()),
            }),
        ));
    }

    fn security_role(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::SecurityRole(SecurityRole {
                name: node_id.into(),
                description: None,
            }),
        )
    }

    fn permission(w: &mut World, permission_ref: &str, operation_ref: &str, role: &str) {
        let scope_ref = format!("resource-scope:{}", &permission_ref["permission:".len()..]);
        w.add(node(
            permission_ref,
            Accepted,
            NodePayload::Permission(Permission {
                name: permission_ref.into(),
            }),
        ))
        .add(node(
            &scope_ref,
            Accepted,
            NodePayload::ResourceScope(ResourceScope {
                resource_ref: id(REQUEST),
                scope_kind: "own".into(),
            }),
        ))
        .add(security_role(role))
        .link(RelationKind::Permits, permission_ref, operation_ref)
        .link(RelationKind::ScopedTo, permission_ref, &scope_ref)
        .link(RelationKind::Grants, role, permission_ref);
    }

    fn table_node() -> Node {
        node(
            TABLE,
            Accepted,
            NodePayload::DecisionTable(DecisionTable {
                name: "Eligibility".into(),
                hit_policy: "FIRST".into(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                rows: Vec::new(),
            }),
        )
    }

    /// One graph holding every selection case.
    fn selection_world() -> World {
        let mut w = submit_world(&["amount > 0"], &[], None);
        // The HR LeaveBalance calculation lives beside the request model.
        for n in hr_world("working_days([start_date, end_date], calendar, true)", None).nodes {
            w.add(n);
        }
        for a in [START, END, INCLUSIVE, FRACTION] {
            w.link(RelationKind::HasAttribute, LEAVE, a);
        }
        requirement_nodes(&mut w, &[(REQ, 1), (REQ_OTHER, 2)]);
        w.add(event(EVENT_SUBMITTED))
            .link(RelationKind::Produces, SUBMIT, EVENT_SUBMITTED)
            .add(outcome("outcome:accepted"))
            .link(RelationKind::Produces, SUBMIT, "outcome:accepted")
            .add(rule(RULE, Some("amount > 10"), None))
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
            .add(calculation(
                CALC_UNRELATED,
                "fee + 1",
                ("Decimal(2)", None),
                None,
                &[(FEE, "fee")],
                None,
                true,
            ))
            .add(operation(OTHER_OP, &[], &[]))
            .add(event(EVENT_APPROVED))
            .add(event(EVENT_OTHER))
            .add(transition(TR_CLOSE, SUBMITTED, CLOSED, None))
            .link(RelationKind::TransitionsVia, TR_CLOSE, EVENT_APPROVED)
            .add(table_node())
            .add(node(
                ALICE,
                Accepted,
                NodePayload::Principal(Principal {
                    name: "Alice".into(),
                    principal_kind: "user".into(),
                }),
            ))
            .add(node(
                VIEWPOINT,
                Accepted,
                NodePayload::Viewpoint(Viewpoint {
                    name: "Overview".into(),
                    concern_refs: Vec::new(),
                    stakeholder_refs: Vec::new(),
                    model_kind_refs: Vec::new(),
                    purpose: None,
                    conventions: None,
                }),
            ));
        permission(&mut w, PERMISSION, SUBMIT, CLERK);
        permission(&mut w, PERMISSION_OTHER, OTHER_OP, ROLE_OTHER);
        w.add(security_role(SENIOR))
            .link(RelationKind::InheritsRole, SENIOR, CLERK)
            .link(RelationKind::AssignedRole, ALICE, SENIOR);

        let mut t = then();
        t.states_active = vec![id(SUBMITTED)];
        t.events = vec![id(EVENT_SUBMITTED)];
        add_scenario(
            &mut w,
            S_SUBMIT,
            given_states(&[(AMOUNT, lit(int(20)))], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
            t,
            "Confirmed",
            &[REQ],
        );
        let mut t = then();
        t.calculation_results
            .insert(id(LEAVE_BALANCE), lit(decimal("5", Some("working_day"))));
        add_scenario(
            &mut w,
            S_HR,
            given(&[
                (START, lit(date("2026-01-05"))),
                (END, lit(date("2026-01-09"))),
            ]),
            ScenarioWhen::Calculation(id(LEAVE_BALANCE)),
            t,
            "Confirmed",
            &[],
        );
        add_scenario(
            &mut w,
            S_HOLIDAY,
            given(&[
                (START, sem(CAL, HOLIDAY_DATE_KEY)),
                (END, sem(CAL, HOLIDAY_DATE_KEY)),
            ]),
            ScenarioWhen::Calculation(id(LEAVE_BALANCE)),
            then(),
            "Confirmed",
            &[],
        );
        let mut g = ScenarioGiven::default();
        g.decision_inputs.insert(0, lit(boolean(true)));
        add_scenario(
            &mut w,
            S_TABLE,
            g,
            ScenarioWhen::DecisionTable(id(TABLE)),
            then(),
            "Confirmed",
            &[],
        );
        add_scenario(
            &mut w,
            S_EVENT,
            given_states(&[], &[SUBMITTED]),
            ScenarioWhen::Event(id(EVENT_APPROVED)),
            then(),
            "Confirmed",
            &[],
        );
        add_scenario(
            &mut w,
            S_TRANSITION,
            given_states(&[(AMOUNT, lit(int(1)))], &[DRAFT]),
            ScenarioWhen::Transition(id(TR_SUBMIT)),
            then(),
            "Confirmed",
            &[],
        );
        add_scenario(
            &mut w,
            S_RULE,
            given(&[(FEE, lit(decimal("1.00", None)))]),
            ScenarioWhen::Rule(id(RULE)),
            then(),
            "Confirmed",
            &[],
        );
        add_scenario(
            &mut w,
            S_DERIVED,
            given_states(&[(AMOUNT, lit(int(20)))], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
            then(),
            "Derived",
            &[REQ_OTHER],
        );
        w
    }

    fn dirty(ids: &[&str]) -> DirtySet {
        DirtySet {
            node_ids: ids.iter().map(|i| id(i)).collect(),
            edge_ids: BTreeSet::new(),
        }
    }

    fn selected(
        w: &World,
        ids: &[&str],
        runtimes: &[IncrementalScenarioRuntime<'_>],
    ) -> Vec<String> {
        let graph = w.graph();
        let model = build_execution_model(&graph, ExecutionInputs::default()).unwrap();
        select_dirty_scenarios(&model, &dirty(ids), runtimes)
            .unwrap()
            .iter()
            .map(|i| i.to_string())
            .collect()
    }

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// A predicate provider that must never be called.
    struct Untouchable;

    impl PredicateProvider for Untouchable {
        fn direct_manager(&self, _: &RefValue, _: &RefValue) -> Result<bool, PredicateError> {
            panic!("the predicate provider was called")
        }
    }

    // ------------------------------------------------------------------ selection

    #[test]
    fn selection_follows_exact_scenario_dependencies() {
        let w = selection_world();
        let cases: Vec<(&[&str], Vec<&str>)> = vec![
            (&[S_SUBMIT], vec![S_SUBMIT]),
            (&[REQ], vec![S_SUBMIT]),
            (&[REQ_OTHER], vec![]),
            (&[CALC_A], vec![S_SUBMIT]),
            (&[LEAVE_BALANCE], vec![S_HR, S_HOLIDAY]),
            (&[CALC_UNRELATED], vec![]),
            (&[RULE], vec![S_RULE, S_SUBMIT]),
            (&[TABLE], vec![S_TABLE]),
            (&[SUBMITTED], vec![S_EVENT, S_SUBMIT, S_TRANSITION]),
            (&[CLOSED], vec![S_EVENT]),
            (&[TR_SUBMIT], vec![S_SUBMIT, S_TRANSITION]),
            (&[TR_CLOSE], vec![S_EVENT]),
            (&[EVENT_APPROVED], vec![S_EVENT]),
            (&[EVENT_SUBMITTED], vec![S_SUBMIT]),
            (&[EVENT_OTHER], vec![]),
            (&[PERMISSION], vec![S_SUBMIT]),
            (&[PERMISSION_OTHER], vec![]),
            (&[CLERK], vec![S_SUBMIT]),
            (&[SENIOR], vec![S_SUBMIT]),
            (&[ROLE_OTHER], vec![]),
            (&[AMOUNT], vec![S_SUBMIT, S_TRANSITION]),
            (&[FEE], vec![S_RULE]),
            (&[CAL], vec![S_HR, S_HOLIDAY]),
            (&["outcome:accepted"], vec![S_SUBMIT]),
            (&[VIEW], vec![]),
            (&[VIEWPOINT], vec![]),
            (&[], vec![]),
            (&[ALICE], vec![]),
            (
                &[TABLE, LEAVE_BALANCE, CALC_UNRELATED],
                vec![S_HR, S_HOLIDAY, S_TABLE],
            ),
            (&[S_DERIVED, OTHER_OP], vec![]),
        ];
        for (dirty_ids, expected) in cases {
            assert_eq!(
                selected(&w, dirty_ids, &[]),
                names(&expected),
                "{dirty_ids:?}"
            );
        }
    }

    fn invariant(node_id: &str, scope_ref: &str, expression: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Invariant(plumb_psg::Invariant {
                scope_ref: id(scope_ref),
                expression: expression.into(),
            }),
        )
    }

    #[test]
    fn invariant_edit_marks_scenario_dirty() {
        // H0.10 (architecture §33.11): the dependency set holds the Accepted Invariants scoped
        // to the footprint and the footprint Entities and Attributes.
        let mut w = selection_world();
        w.add(invariant("invariant:limit", AMOUNT, "amount < 100"))
            .add(invariant("invariant:request", REQUEST, "amount < 1000"))
            .add(invariant("invariant:fee", FEE, "fee > 0"))
            .add(attribute(NOTE, "note", "String", None, None))
            .link(RelationKind::Writes, SUBMIT, AMOUNT)
            .link(RelationKind::Reads, SUBMIT, NOTE);
        // Transitions on the request apply its invariants to every Scenario that moves it.
        assert_eq!(
            selected(&w, &["invariant:request"], &[]),
            names(&[S_EVENT, S_SUBMIT, S_TRANSITION])
        );
        assert_eq!(selected(&w, &["invariant:limit"], &[]), names(&[S_SUBMIT]));
        assert!(selected(&w, &["invariant:fee"], &[]).is_empty());
        assert_eq!(selected(&w, &[NOTE], &[]), names(&[S_SUBMIT]));

        // Editing the invariant re-executes the dependent Scenario deterministically and its
        // earlier run is stale.
        let cal = calendars(&[]);
        let inputs = || {
            scopes(vec![
                scope(SUBMIT, &[(AMOUNT, "amount")]),
                scope(RULE, &[(AMOUNT, "amount")]),
                scope("invariant:limit", &[(AMOUNT, "amount")]),
                scope("invariant:request", &[(AMOUNT, "amount")]),
                scope("invariant:fee", &[(FEE, "fee")]),
            ])
        };
        let runtimes = [IncrementalScenarioRuntime {
            scenario_ref: id(S_SUBMIT),
            runtime: runtime(&cal, &NO_MANAGERS, Some(ALICE), Vec::new()),
        }];
        let before = rerun(&w, inputs(), &["invariant:limit"], &runtimes, &[]).unwrap();
        assert_eq!(before.selected_scenario_refs, vec![id(S_SUBMIT)]);
        assert_eq!(before.comparisons[0].result, ScenarioVerdict::Pass);
        let mut edited = w.clone();
        edited.add(invariant("invariant:limit", AMOUNT, "amount < 10"));
        let after = rerun(&edited, inputs(), &["invariant:limit"], &runtimes, &[]).unwrap();
        assert_eq!(after.comparisons[0].result, ScenarioVerdict::Fail);
        let again = rerun(&edited, inputs(), &["invariant:limit"], &runtimes, &[]).unwrap();
        assert_eq!(after, again);

        let old = &before.comparisons[0].trace_artifact.body;
        let graph = edited.graph();
        let model = build_execution_model(&graph, inputs()).unwrap();
        assert_eq!(
            scenario_run_currency(
                &model,
                &id(S_SUBMIT),
                &old.specification_hash,
                &old.model_semantic_hash
            )
            .unwrap(),
            ScenarioRunCurrency::StaleDependencies
        );
        let new = &after.comparisons[0].trace_artifact.body;
        assert_eq!(
            scenario_run_currency(
                &model,
                &id(S_SUBMIT),
                &new.specification_hash,
                &new.model_semantic_hash
            )
            .unwrap(),
            ScenarioRunCurrency::Current
        );
        assert_eq!(
            scenario_run_currency(
                &model,
                &id(S_SUBMIT),
                &plumb_core::Hash::content_sha256(b"older specification"),
                &new.model_semantic_hash
            )
            .unwrap(),
            ScenarioRunCurrency::StaleSpecification
        );
    }

    #[test]
    fn runtime_actor_is_a_dependency_of_its_scenario() {
        let w = selection_world();
        let cal = calendars(&[]);
        let runtimes = [IncrementalScenarioRuntime {
            scenario_ref: id(S_SUBMIT),
            runtime: runtime(&cal, &NO_MANAGERS, Some(ALICE), Vec::new()),
        }];
        assert_eq!(selected(&w, &[ALICE], &runtimes), names(&[S_SUBMIT]));
        assert!(selected(&w, &[ALICE], &[]).is_empty());
    }

    // ------------------------------------------------------------------ reruns

    fn rerun(
        w: &World,
        inputs: ExecutionInputs,
        ids: &[&str],
        runtimes: &[IncrementalScenarioRuntime<'_>],
        blocking: &[&str],
    ) -> Result<IncrementalScenarioResult, IncrementalError> {
        let graph = w.graph();
        let model = build_execution_model(&graph, inputs)?;
        let policy = IncrementalScenarioPolicy {
            blocking_scenario_refs: blocking.iter().map(|b| id(b)).collect(),
        };
        rerun_dirty_scenarios(&model, &dirty(ids), runtimes, &policy)
    }

    fn runtimes_for<'a>(
        scenarios: &[&str],
        cal: &'a StaticCalendarProvider,
    ) -> Vec<IncrementalScenarioRuntime<'a>> {
        scenarios
            .iter()
            .map(|s| IncrementalScenarioRuntime {
                scenario_ref: id(s),
                runtime: runtime(cal, &NO_MANAGERS, None, Vec::new()),
            })
            .collect()
    }

    #[test]
    fn hr_leave_balance_change_reruns_only_dependent_scenarios() {
        let w = selection_world();
        let cal = calendars(&[]);
        let all = [
            S_SUBMIT,
            S_HR,
            S_HOLIDAY,
            S_TABLE,
            S_EVENT,
            S_TRANSITION,
            S_RULE,
        ];
        let runtimes = runtimes_for(&all, &cal);
        let result = rerun(
            &w,
            ExecutionInputs::default(),
            &[LEAVE_BALANCE],
            &runtimes,
            &[],
        )
        .unwrap();
        assert_eq!(result.selected_scenario_refs, vec![id(S_HR), id(S_HOLIDAY)]);
        let verdicts: Vec<(String, ScenarioVerdict)> = result
            .comparisons
            .iter()
            .map(|c| (c.scenario_ref.to_string(), c.result))
            .collect();
        // The holiday date is not supplied at runtime, so that scenario stays undecidable.
        assert_eq!(
            verdicts,
            vec![
                (S_HR.to_string(), ScenarioVerdict::Pass),
                (S_HOLIDAY.to_string(), ScenarioVerdict::Undecidable),
            ]
        );
        assert!(result.findings.is_empty());
        for (dirty_ids, _) in [
            (&[CALC_UNRELATED][..], ()),
            (&[VIEW][..], ()),
            (&[VIEWPOINT][..], ()),
            (&[][..], ()),
        ] {
            let result = rerun(&w, ExecutionInputs::default(), dirty_ids, &runtimes, &[]).unwrap();
            assert!(result.selected_scenario_refs.is_empty(), "{dirty_ids:?}");
            assert!(result.comparisons.is_empty() && result.findings.is_empty());
        }
    }

    #[test]
    fn runtimes_are_required_unique_and_only_used_when_selected() {
        let w = selection_world();
        let cal = calendars(&[]);
        let err = rerun(
            &w,
            ExecutionInputs::default(),
            &[LEAVE_BALANCE],
            &runtimes_for(&[S_HR], &cal),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            err,
            IncrementalError::MissingRuntime {
                scenario_ref: id(S_HOLIDAY)
            }
        );
        let err = rerun(
            &w,
            ExecutionInputs::default(),
            &[],
            &runtimes_for(&[S_HR, S_HR], &cal),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            err,
            IncrementalError::DuplicateRuntime {
                scenario_ref: id(S_HR)
            }
        );
        // Providers of unselected scenarios, and of any scenario when nothing is selected, are
        // never called.
        let untouchable = Untouchable;
        let mut runtimes = runtimes_for(&[S_HR, S_HOLIDAY], &cal);
        runtimes.push(IncrementalScenarioRuntime {
            scenario_ref: id(S_SUBMIT),
            runtime: ExecutionRuntime {
                actor_ref: None,
                semantic_values: Vec::new(),
                calendars: &cal,
                predicates: &untouchable,
            },
        });
        let result = rerun(
            &w,
            ExecutionInputs::default(),
            &[LEAVE_BALANCE],
            &runtimes,
            &[],
        )
        .unwrap();
        assert_eq!(result.selected_scenario_refs.len(), 2);
        let result = rerun(&w, ExecutionInputs::default(), &[VIEW], &runtimes, &[]).unwrap();
        assert!(result.selected_scenario_refs.is_empty());
    }

    #[test]
    fn untouchable_provider_panics_when_actually_called() {
        let mut w = submit_world(&["direct_manager(request, request)"], &[], None);
        add_scenario(
            &mut w,
            S_SUBMIT,
            given_states(&[(AMOUNT, lit(int(1)))], &[DRAFT]),
            ScenarioWhen::Operation(id(SUBMIT)),
            then(),
            "Confirmed",
            &[],
        );
        let cal = calendars(&[]);
        let untouchable = Untouchable;
        let runtimes = [IncrementalScenarioRuntime {
            scenario_ref: id(S_SUBMIT),
            runtime: ExecutionRuntime {
                actor_ref: None,
                semantic_values: Vec::new(),
                calendars: &cal,
                predicates: &untouchable,
            },
        }];
        let inputs = || scopes(vec![scope(SUBMIT, &[(REQUEST, "request")])]);
        // Not selected: no call.
        assert!(rerun(&w, inputs(), &[VIEW], &runtimes, &[])
            .unwrap()
            .selected_scenario_refs
            .is_empty());
        // Selected: the provider is reached (proving the guard above is meaningful).
        let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = rerun(&w, inputs(), &[SUBMIT], &runtimes, &[]);
        }));
        assert!(called.is_err());
    }

    // ------------------------------------------------------------------ findings

    const S_PASS: &str = "scenario:a-pass";
    const S_FAIL: &str = "scenario:b-fail";
    const S_BLOCKING: &str = "scenario:c-blocking";
    const S_OPEN: &str = "scenario:d-open";

    /// Four rule scenarios sharing one dirty Rule: Pass, Fail, and two Undecidable.
    fn verdict_world(fail_amount: i64, undecidable_given: &[(&str, ScenarioValue)]) -> World {
        let mut w = request_world();
        w.add(rule(RULE, Some("amount > 10 and approved"), None));
        let mut t = then();
        t.rules_satisfied = vec![id(RULE)];
        let approved = (APPROVED, lit(boolean(true)));
        add_scenario(
            &mut w,
            S_PASS,
            given(&[(AMOUNT, lit(int(20))), approved.clone()]),
            ScenarioWhen::Rule(id(RULE)),
            t.clone(),
            "Confirmed",
            &[],
        );
        add_scenario(
            &mut w,
            S_FAIL,
            given(&[(AMOUNT, lit(int(fail_amount))), approved]),
            ScenarioWhen::Rule(id(RULE)),
            t.clone(),
            "Confirmed",
            &[],
        );
        for s in [S_BLOCKING, S_OPEN] {
            add_scenario(
                &mut w,
                s,
                given(undecidable_given),
                ScenarioWhen::Rule(id(RULE)),
                t.clone(),
                "Confirmed",
                &[],
            );
        }
        w
    }

    fn rule_inputs() -> ExecutionInputs {
        scopes(vec![scope(
            RULE,
            &[(AMOUNT, "amount"), (APPROVED, "approved")],
        )])
    }

    fn verdict_run(w: &World, blocking: &[&str]) -> IncrementalScenarioResult {
        let cal = calendars(&[]);
        let runtimes = runtimes_for(&[S_PASS, S_FAIL, S_BLOCKING, S_OPEN], &cal);
        rerun(w, rule_inputs(), &[RULE], &runtimes, blocking).unwrap()
    }

    // Independently computed in Python: SHA-256 of code 0x00 scenario_ref 0x00 condition.
    const GOLDEN_FAIL_KEY: &str =
        "sha256:b74213b0405828fabcbbe1e15deb0e34b44a29cccde1cf196ebd8fe77926b949";
    const GOLDEN_FAIL_ID: &str = "fnd:b74213b0405828fa";
    const GOLDEN_UNDECIDABLE_KEY: &str =
        "sha256:98a2bf30641d069964e4f8ae55488b3c1d2747f7ccd057cc5308f2d34f987f4f";
    const GOLDEN_UNDECIDABLE_ID: &str = "fnd:98a2bf30641d0699";

    fn expected_fail() -> GeneratedFinding {
        GeneratedFinding {
            key: GOLDEN_FAIL_KEY.parse().unwrap(),
            id: id(GOLDEN_FAIL_ID),
            semantic_condition_key: "scenario_fail".into(),
            payload: Finding {
                code: "I_SCENARIO_FAIL".into(),
                family: "S4".into(),
                severity: FindingSeverity::Blocker,
                message: format!(
                    "Confirmed scenario {S_FAIL} failed against its expected semantics."
                ),
                status: "Open".into(),
                affected_refs: vec![id(S_FAIL)],
                standard_rule_ref: None,
                suggested_resolution: Some(
                    "Review the scenario trace and resolve the model or confirmed expectation mismatch."
                        .into(),
                ),
                waiver_ref: None,
            },
        }
    }

    fn expected_undecidable() -> GeneratedFinding {
        GeneratedFinding {
            key: GOLDEN_UNDECIDABLE_KEY.parse().unwrap(),
            id: id(GOLDEN_UNDECIDABLE_ID),
            semantic_condition_key: "blocking_scenario_undecidable".into(),
            payload: Finding {
                code: "G_SCENARIO_UNDECIDABLE".into(),
                family: "S4".into(),
                severity: FindingSeverity::Blocker,
                message: format!(
                    "Blocking confirmed scenario {S_BLOCKING} is undecidable because required semantics are missing."
                ),
                status: "Open".into(),
                affected_refs: vec![id(S_BLOCKING)],
                standard_rule_ref: None,
                suggested_resolution: Some(
                    "Resolve the missing semantics and re-execute the scenario.".into(),
                ),
                waiver_ref: None,
            },
        }
    }

    #[test]
    fn mixed_verdicts_produce_exact_findings() {
        let w = verdict_world(5, &[(APPROVED, lit(boolean(true)))]);
        let result = verdict_run(&w, &[S_BLOCKING]);
        assert_eq!(
            result.selected_scenario_refs,
            vec![id(S_PASS), id(S_FAIL), id(S_BLOCKING), id(S_OPEN)]
        );
        let verdicts: Vec<ScenarioVerdict> = result.comparisons.iter().map(|c| c.result).collect();
        assert_eq!(
            verdicts,
            vec![
                ScenarioVerdict::Pass,
                ScenarioVerdict::Fail,
                ScenarioVerdict::Undecidable,
                ScenarioVerdict::Undecidable,
            ]
        );
        let refs: Vec<&Id> = result.comparisons.iter().map(|c| &c.scenario_ref).collect();
        let mut sorted = refs.clone();
        sorted.sort();
        assert_eq!(refs, sorted);
        let mut expected = vec![expected_fail(), expected_undecidable()];
        expected.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(result.findings, expected);
        // Without the blocking designation the undecidable scenarios produce nothing.
        let result = verdict_run(&w, &[]);
        assert_eq!(result.findings, vec![expected_fail()]);
        assert_eq!(result.comparisons.len(), 4);
    }

    #[test]
    fn finding_identity_is_stable_across_details() {
        let a = verdict_run(
            &verdict_world(5, &[(APPROVED, lit(boolean(true)))]),
            &[S_BLOCKING],
        );
        let b = verdict_run(&verdict_world(1, &[(AMOUNT, lit(int(50)))]), &[S_BLOCKING]);
        // Different diffs and different missing semantics, identical findings.
        assert_ne!(
            a.comparisons[1].trace_artifact.hash,
            b.comparisons[1].trace_artifact.hash
        );
        assert_ne!(
            a.comparisons[2].trace_artifact.hash,
            b.comparisons[2].trace_artifact.hash
        );
        assert_eq!(a.findings, b.findings);
        assert_eq!(a.findings.len(), 2);
    }

    #[test]
    fn finding_keys_match_the_independent_goldens() {
        let fail = finding_key("I_SCENARIO_FAIL", &[id(S_FAIL)], "scenario_fail").unwrap();
        assert_eq!(fail.to_string(), GOLDEN_FAIL_KEY);
        assert_eq!(finding_id(&fail).unwrap(), id(GOLDEN_FAIL_ID));
        let undecidable = finding_key(
            "G_SCENARIO_UNDECIDABLE",
            &[id(S_BLOCKING)],
            "blocking_scenario_undecidable",
        )
        .unwrap();
        assert_eq!(undecidable.to_string(), GOLDEN_UNDECIDABLE_KEY);
        assert_eq!(finding_id(&undecidable).unwrap(), id(GOLDEN_UNDECIDABLE_ID));
        assert_eq!(SCENARIO_FAIL_CODE, "I_SCENARIO_FAIL");
        assert_eq!(SCENARIO_UNDECIDABLE_CODE, "G_SCENARIO_UNDECIDABLE");
        assert_eq!(SCENARIO_FAIL_CONDITION, "scenario_fail");
        assert_eq!(
            SCENARIO_UNDECIDABLE_CONDITION,
            "blocking_scenario_undecidable"
        );
        assert_eq!(SCENARIO_FINDING_FAMILY, "S4");
    }

    #[test]
    fn execution_codes_are_not_validation_rules() {
        let rules = include_str!("../../../config/profiles/plumb-software-2026.1-rules.yaml");
        let rulebook = include_str!("../../../docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md");
        for code in ["I_SCENARIO_FAIL", "G_SCENARIO_UNDECIDABLE"] {
            assert!(!rules.contains(code) && !rulebook.contains(code), "{code}");
        }
    }

    #[test]
    fn blocking_policy_must_name_confirmed_scenarios() {
        let w = selection_world();
        let cal = calendars(&[]);
        for bad in [S_DERIVED, "scenario:unknown", SUBMIT] {
            let err = rerun(
                &w,
                ExecutionInputs::default(),
                &[],
                &runtimes_for(&[], &cal),
                &[bad],
            )
            .unwrap_err();
            assert_eq!(
                err,
                IncrementalError::InvalidBlockingScenario {
                    scenario_ref: id(bad)
                }
            );
        }
    }

    #[test]
    fn errors_propagate_and_never_become_findings() {
        // A runtime value that would answer business semantics is a SimulationError.
        let w = selection_world();
        let cal = calendars(&[]);
        let runtimes = [
            IncrementalScenarioRuntime {
                scenario_ref: id(S_HR),
                runtime: runtime(
                    &cal,
                    &NO_MANAGERS,
                    None,
                    vec![RuntimeSemanticValue {
                        owner_ref: id(LEAVE_BALANCE),
                        key: HALF_DAY.into(),
                        value: decimal("0.5", None),
                    }],
                ),
            },
            IncrementalScenarioRuntime {
                scenario_ref: id(S_HOLIDAY),
                runtime: runtime(&cal, &NO_MANAGERS, None, Vec::new()),
            },
        ];
        let err = rerun(
            &w,
            ExecutionInputs::default(),
            &[LEAVE_BALANCE],
            &runtimes,
            &[],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            IncrementalError::Simulation(SimulationError::DisallowedRuntimeValue { .. })
        ));
    }

    #[test]
    fn incremental_results_are_deterministic() {
        let w = verdict_world(5, &[(APPROVED, lit(boolean(true)))]);
        let cal = calendars(&[]);
        let forward_graph = w.graph();
        let reverse_graph = w.reversed_graph();
        let policy = |refs: &[&str]| IncrementalScenarioPolicy {
            blocking_scenario_refs: refs.iter().map(|r| id(r)).collect(),
        };
        let forward_model = build_execution_model(&forward_graph, rule_inputs()).unwrap();
        let reverse_model = build_execution_model(&reverse_graph, rule_inputs()).unwrap();
        let order = [S_PASS, S_FAIL, S_BLOCKING, S_OPEN];
        let mut reversed = order;
        reversed.reverse();
        let forward = rerun_dirty_scenarios(
            &forward_model,
            &dirty(&[RULE, AMOUNT]),
            &runtimes_for(&order, &cal),
            &policy(&[S_BLOCKING, S_OPEN]),
        )
        .unwrap();
        let reverse = rerun_dirty_scenarios(
            &reverse_model,
            &dirty(&[AMOUNT, RULE]),
            &runtimes_for(&reversed, &cal),
            &policy(&[S_OPEN, S_BLOCKING]),
        )
        .unwrap();
        assert_eq!(forward, reverse);
        for (a, b) in forward.comparisons.iter().zip(&reverse.comparisons) {
            assert_eq!(a.trace_artifact.bytes, b.trace_artifact.bytes);
        }
        assert_eq!(forward.findings.len(), 3);
        let ids: Vec<&Id> = forward.findings.iter().map(|f| &f.id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

    // ------------------------------------------------------------------ source guards

    #[test]
    fn incremental_source_is_pure_and_owns_no_impact_or_validation() {
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
            "compute_impact",
            "impact_reachable_nodes",
            "impact_direction",
            "GraphDelta",
            "ChangedSet",
            "ImpactReport",
            "dirty.edge_ids",
            "for_violation",
            "EvaluatorRegistry",
            "ValidationRegistry",
            "evaluate_gate",
            "violation_state",
            "governed_waiver",
            "evaluate_calculation",
            "effective_security_roles",
            "literals_equal",
            ".name",
            "Graph::new",
            "insert_node",
            "HashMap",
        ];
        for token in forbidden {
            assert!(
                !INCREMENTAL_SRC.contains(token),
                "incremental.rs contains {token}"
            );
        }
        assert!(INCREMENTAL_SRC.contains("run(model, scenario_ref, runtime)"));
        assert!(INCREMENTAL_SRC.contains("compare_run(model, &execution, runtime)"));
    }
}
