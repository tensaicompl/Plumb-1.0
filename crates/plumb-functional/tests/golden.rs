//! F0.14 contract tests for the one-way functional.yaml v2 compatibility projection.
//!
//! The graph is a hand-built HR PSG fixture, not the full compiled HR corpus (that export
//! belongs to S4.7). The expected YAML below is hand-authored from the mapping contract.
//! Every test lives in `golden_contract` so that `cargo test -p plumb-functional golden`
//! selects them.

mod golden_contract {
    use std::collections::BTreeSet;

    use plumb_core::{Hash, HashKind, Id};
    use plumb_functional::*;
    use plumb_psg::{Edge, Graph, Node};
    use plumb_validation::{load_builtin_software_profile, ValidationProfile};
    use serde_json::{json, Value};

    const T1: &str = "2026-09-29T08:00:00.000000000Z";
    const PROJECT: &str = "project:leave-management";
    const PROFILE: &str = "profile:plumb-software-2026.1";

    /// Semantic hash of `hr_graph()`, produced by plumb-psg.
    const HR_SEMANTIC_HASH: &str =
        "psg:sha256:b1497582882784f9ee21638c289d6017ae9b28ee69a458955f02c769b5df235f";

    /// The hand-authored functional.yaml of `hr_graph()`.
    const GOLDEN_YAML: &str = r#"version: 2
model_hash: psg:sha256:b1497582882784f9ee21638c289d6017ae9b28ee69a458955f02c769b5df235f
glossary:
- id: concept:working-day
  name: Working day
  definition: A day on the HR calendar that is not a holiday.
  aliases: []
- id: term:carry-over
  name: Carry-over
  definition: ''
  aliases: []
- id: term:leave-request
  name: Leave request
  definition: A request by an employee for paid time off.
  aliases:
  - absence request
  - time-off request
actors:
- id: actor:employee
  kind: human
- id: actor:hr-system
  kind: system
- id: actor:line-manager
  kind: human
- id: actor:payroll-provider
  kind: external
roles:
- id: brole:manager
  actor: brole:manager
  operations:
  - op:approve-leave
- id: srole:hr-admin
  actor: srole:hr-admin
  operations: []
- id: srole:leave-approver
  actor: actor:line-manager
  operations:
  - op:approve-leave
entities:
- id: ent:employee
  attributes: []
  relationships: []
  states: []
  transitions: []
  invariants: []
- id: ent:leave-request
  attributes:
  - id: attr:leave-days
    type: decimal
    class: none
    unit: day
    precision: 1
  - id: attr:leave-status
    type: enum
    class: confidential
    enum:
    - pending
    - approved
  relationships:
  - id: rel:request-employee
    to: ent:employee
    card_from: many
    card_to: one
    snapshot: true
  states:
  - id: state:approved
  - id: state:expired
  - id: state:pending
  transitions:
  - from: state:pending
    to: state:approved
    operation: op:approve-leave
    actor: brole:manager
    precondition: leave_days > 0
  invariants:
  - id: inv:positive-days
    expr: leave_days > 0
  nfr:
    retention: P7Y
calendars:
- id: cal:hr-pl
  region: PL
  tz: Europe/Warsaw
  week_pattern:
  - mon
  - tue
  - wed
  - thu
  - fri
calculations: []
rules:
- id: dt:approval-routing
  conditions:
  - name: leave_days
    type: decimal
  rows:
  - then:
    - manager
    when:
    - <= 5
operations:
- id: op:approve-leave
  kind: command
  actor: brole:manager
  reads:
  - attr:leave-days
  writes:
  - attr:leave-status
  pre:
  - leave_status == pending
  post:
  - leave_status == approved
  outcomes:
  - id: out:approved
    kind: success
  - id: out:rejected
    kind: failure
  governed_by:
  - dt:approval-routing
  nfr:
    latency_ms: 2000
    idempotent: true
processes:
- id: proc:leave-approval
  trigger:
    kind: event
    ref: msg:leave-submitted
  steps:
  - id: pn:approve
    actor: brole:manager
    operation: op:approve-leave
    emits:
    - evt:leave-approved
    next:
    - to: pn:done
  outcomes:
  - out:approved
events:
- id: evt:leave-approved
  payload:
  - attr:leave-status
- id: evt:request-expired
  payload: []
requirements:
- id: req:HR-001
  text: The system shall let a manager approve a pending leave request.
  class: functional
  criteria: []
  operations:
  - op:approve-leave
  status: Confirmed
scenarios:
- id: scn:approve-happy
  requirement: req:HR-001
  operation: op:approve-leave
  given:
  - entity: ent:leave-request
    values:
      leave_days: 2
  when:
    actor: brole:manager
    inputs: {}
    operation: op:approve-leave
  then:
    outcome: out:approved
    states:
    - state:approved
  status: Confirmed
assumptions:
- id: asm:carry-over
  finding: fnd:0123456789abcdef
  default: 5
  owner: actor:line-manager
  expires: 2027-01-01T00:00:00.000000000Z
"#;

    /// SHA-256 of `GOLDEN_YAML`, calculated outside the implementation.
    const GOLDEN_CONTENT_HASH: &str =
        "sha256:3a4a3d756d0821ced3cfd3f9e842975247a66d5cba8ed15ec47968e34588016a";

    const PROFILE_HASH: &str =
        "sha256:08b5b1535eedf401dd002c26ffc7301cfc35a119680f75b664de691b41c3a52f";

    /// The expected warnings of `hr_graph()`: (code, source refs).
    const GOLDEN_WARNINGS: [(&str, &[&str]); 13] = [
        ("V2_ACCEPTANCE_CRITERION_UNMAPPED", &["ac:HR-001-a"]),
        ("V2_ASSUMPTION_UNREPRESENTABLE", &["asm:unbounded"]),
        (
            "V2_CALCULATION_TARGET_UNREPRESENTABLE",
            &["calc:leave-days"],
        ),
        (
            "V2_ENTITY_LEVEL_READ_WRITE_OMITTED",
            &["ent:employee", "op:approve-leave"],
        ),
        (
            "V2_EVENT_TRIGGER_TRANSITION_OMITTED",
            &["evt:request-expired", "tr:expire"],
        ),
        ("V2_GLOSSARY_DEFINITION_MISSING", &["term:carry-over"]),
        ("V2_NON_ACCEPTED_BASELINE_OMITTED", &["actor:legacy-clerk"]),
        ("V2_OPERATION_PERFORMER_MISSING", &["op:submit-leave"]),
        ("V2_PROCESS_STEP_OPERATION_MISSING", &["pn:route"]),
        ("V2_PROCESS_TRIGGER_UNREPRESENTABLE", &["proc:escalation"]),
        ("V2_QUALITY_SCENARIO_OMITTED", &["qs:throughput"]),
        ("V2_RULE_TABLE_UNREPRESENTABLE", &["rule:max-consecutive"]),
        ("V2_SCENARIO_OPERATION_MISSING", &["scn:no-operation"]),
    ];

    /// The expected lossy mappings of `hr_graph()`: (code, source refs).
    const GOLDEN_LOSSY: [(&str, &[&str]); 11] = [
        ("V2_ATTRIBUTE_NULLABILITY_OMITTED", &["attr:leave-days"]),
        ("V2_ATTRIBUTE_NULLABILITY_OMITTED", &["attr:leave-status"]),
        ("V2_BUSINESS_ROLE_COLLAPSE", &["brole:manager"]),
        ("V2_DECISION_TABLE_DETAIL_OMITTED", &["dt:approval-routing"]),
        (
            "V2_ORGANIZATION_ACTOR_COLLAPSE",
            &["actor:payroll-provider"],
        ),
        ("V2_OUTCOME_KIND_COLLAPSE", &["out:rejected"]),
        (
            "V2_PERMISSION_SCOPE_OMITTED",
            &[
                "perm:approve-leave",
                "scope:own-team",
                "srole:leave-approver",
            ],
        ),
        (
            "V2_QUALITY_SCENARIO_COLLAPSE",
            &["ent:leave-request", "qs:retention"],
        ),
        (
            "V2_QUALITY_SCENARIO_COLLAPSE",
            &["op:approve-leave", "qs:approval-latency"],
        ),
        ("V2_RELATIONSHIP_DETAIL_OMITTED", &["rel:request-employee"]),
        ("V2_SECURITY_ROLE_COLLAPSE", &["srole:hr-admin"]),
    ];

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{e}: {value}"))
    }

    fn audit() -> Value {
        json!({"created_by": "actor:analyst", "created_at": T1, "updated_by": null, "updated_at": null})
    }

    fn node(node_id: &str, status: &str, tag: &str, data: Value) -> Node {
        from_json(json!({
            "id": node_id, "revision": 1, "status": status,
            "payload": {"type": tag, "data": data},
            "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
            "audit": audit()
        }))
    }

    fn edge_with(
        edge_id: &str,
        status: &str,
        kind: &str,
        from: &str,
        to: &str,
        properties: Value,
    ) -> Edge {
        from_json(json!({
            "id": edge_id, "revision": 1, "status": status, "kind": kind, "from": from, "to": to,
            "properties": properties, "evidence": [], "derivations": [], "standards": [],
            "audit": audit()
        }))
    }

    /// A mutable set of nodes and edges, turned into a validated Graph on demand.
    #[derive(Clone)]
    struct Fixture {
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        edge_count: usize,
    }

    impl Fixture {
        fn new() -> Fixture {
            Fixture {
                nodes: Vec::new(),
                edges: Vec::new(),
                edge_count: 0,
            }
        }

        fn add(&mut self, node_id: &str, tag: &str, data: Value) -> &mut Self {
            self.add_with_status(node_id, "Accepted", tag, data)
        }

        fn add_with_status(
            &mut self,
            node_id: &str,
            status: &str,
            tag: &str,
            data: Value,
        ) -> &mut Self {
            self.nodes.push(node(node_id, status, tag, data));
            self
        }

        fn link(&mut self, kind: &str, from: &str, to: &str) -> &mut Self {
            self.link_with_status("Accepted", kind, from, to)
        }

        fn link_with_status(
            &mut self,
            status: &str,
            kind: &str,
            from: &str,
            to: &str,
        ) -> &mut Self {
            self.edge_count += 1;
            let properties = if kind == "schema_for" {
                json!({"role": "attribute"})
            } else {
                json!({})
            };
            self.edges.push(edge_with(
                &format!("edge:{:03}", self.edge_count),
                status,
                kind,
                from,
                to,
                properties,
            ));
            self
        }

        fn graph(&self) -> Graph {
            self.graph_for(PROFILE)
        }

        fn graph_for(&self, profile: &str) -> Graph {
            Graph::new(
                id(PROJECT),
                id(profile),
                self.nodes.clone(),
                self.edges.clone(),
            )
            .unwrap_or_else(|v| panic!("{v:#?}"))
        }
    }

    fn operation(f: &mut Fixture, op: &str, idempotency: Value) {
        f.add(
            op,
            "Operation",
            json!({"name": op, "operation_kind": "command", "idempotency": idempotency}),
        );
    }

    fn entity(f: &mut Fixture, ent: &str) {
        f.add(ent, "Entity", json!({"name": ent}));
    }

    fn attribute(f: &mut Fixture, owner: &str, attr: &str, extra: Value) {
        let mut data = json!({"name": attr, "value_type": "decimal", "nullable": false});
        for (k, v) in extra.as_object().unwrap() {
            data[k] = v.clone();
        }
        f.add(attr, "Attribute", data)
            .link("has_attribute", owner, attr);
    }

    fn quality(f: &mut Fixture, qs: &str, characteristic: &str, threshold: Value, target: &str) {
        let qc = format!("qc:{}", qs.trim_start_matches("qs:"));
        let msr = format!("msr:{}", qs.trim_start_matches("qs:"));
        f.add(
            qs,
            "QualityScenario",
            json!({"stimulus": "load", "response": "respond", "threshold": threshold,
                   "affected_refs": [target], "priority": "high"}),
        )
        .add(
            &qc,
            "QualityCharacteristic",
            json!({"name": characteristic, "scheme": "legacy"}),
        )
        .add(
            &msr,
            "Measure",
            json!({"name": characteristic, "measure_type": "gauge", "unit": "unit"}),
        )
        .link("characterized_by", qs, &qc)
        .link("measured_by", qs, &msr);
    }

    /// The hand-built HR PSG fixture of the golden projection.
    fn hr_fixture() -> Fixture {
        let mut f = Fixture::new();
        // glossary
        f.add(
            "term:leave-request",
            "Term",
            json!({"term": "Leave request", "language": "en", "definition_ref": "concept:leave-request",
                   "aliases": ["time-off request", "absence request"]}),
        )
        .add(
            "concept:leave-request",
            "Concept",
            json!({"name": "Leave request", "definition": "A request by an employee for paid time off.",
                   "concept_kind": "object_type"}),
        )
        .add(
            "concept:working-day",
            "Concept",
            json!({"name": "Working day", "definition": "A day on the HR calendar that is not a holiday.",
                   "concept_kind": "value_type"}),
        )
        .add("term:carry-over", "Term", json!({"term": "Carry-over", "language": "en"}));
        // actors and roles
        f.add(
            "actor:employee",
            "Actor",
            json!({"name": "Employee", "actor_kind": "human"}),
        )
        .add(
            "actor:hr-system",
            "Actor",
            json!({"name": "HR system", "actor_kind": "system"}),
        )
        .add(
            "actor:line-manager",
            "Actor",
            json!({"name": "Line manager", "actor_kind": "human"}),
        )
        .add(
            "actor:payroll-provider",
            "Actor",
            json!({"name": "Payroll provider", "actor_kind": "organization"}),
        )
        .add_with_status(
            "actor:legacy-clerk",
            "Deprecated",
            "Actor",
            json!({"name": "Clerk", "actor_kind": "human"}),
        )
        .add("brole:manager", "BusinessRole", json!({"name": "Manager"}))
        .add(
            "srole:leave-approver",
            "SecurityRole",
            json!({"name": "Leave approver"}),
        )
        .add(
            "srole:hr-admin",
            "SecurityRole",
            json!({"name": "HR admin"}),
        )
        .add(
            "perm:approve-leave",
            "Permission",
            json!({"name": "Approve leave"}),
        )
        .add(
            "scope:own-team",
            "ResourceScope",
            json!({"resource_ref": "ent:leave-request", "scope_kind": "team"}),
        )
        .link(
            "assigned_role",
            "actor:line-manager",
            "srole:leave-approver",
        )
        .link("grants", "srole:leave-approver", "perm:approve-leave")
        .link("permits", "perm:approve-leave", "op:approve-leave")
        .link("scoped_to", "perm:approve-leave", "scope:own-team");
        // domain
        entity(&mut f, "ent:employee");
        entity(&mut f, "ent:leave-request");
        attribute(
            &mut f,
            "ent:leave-request",
            "attr:leave-days",
            json!({"unit": "day", "precision": 1}),
        );
        attribute(
            &mut f,
            "ent:leave-request",
            "attr:leave-status",
            json!({"value_type": "enum", "enum_values": ["pending", "approved"],
                   "data_classification": "confidential"}),
        );
        f.add(
            "rel:request-employee",
            "DomainRelationship",
            json!({"from_entity": "ent:leave-request", "to_entity": "ent:employee",
                   "relationship_kind": "association", "cardinality_from": "many",
                   "cardinality_to": "one", "snapshot_semantics": true}),
        );
        for state in ["state:pending", "state:approved", "state:expired"] {
            f.add(state, "State", json!({"name": state})).link(
                "has_state",
                "ent:leave-request",
                state,
            );
        }
        f.add(
            "tr:approve",
            "Transition",
            json!({"stateful_ref": "ent:leave-request", "from_state": "state:pending",
                   "to_state": "state:approved", "guard_expr": "leave_days > 0"}),
        )
        .link("transitions_via", "tr:approve", "op:approve-leave")
        .add(
            "tr:expire",
            "Transition",
            json!({"stateful_ref": "ent:leave-request", "from_state": "state:pending",
                   "to_state": "state:expired"}),
        )
        .link("transitions_via", "tr:expire", "evt:request-expired")
        .add(
            "inv:positive-days",
            "Invariant",
            json!({"scope_ref": "ent:leave-request", "expression": "leave_days > 0"}),
        );
        // operations
        f.add(
            "op:approve-leave",
            "Operation",
            json!({"name": "ApproveLeaveRequest", "operation_kind": "command",
                   "preconditions": ["leave_status == pending"],
                   "postconditions": ["leave_status == approved"], "idempotency": "true"}),
        );
        operation(&mut f, "op:submit-leave", json!(null));
        f.add(
            "out:approved",
            "Outcome",
            json!({"name": "Approved", "outcome_kind": "success"}),
        )
        .add(
            "out:rejected",
            "Outcome",
            json!({"name": "Rejected", "outcome_kind": "business_failure"}),
        )
        .link("performed_by", "op:approve-leave", "brole:manager")
        .link("reads", "op:approve-leave", "attr:leave-days")
        .link("writes", "op:approve-leave", "attr:leave-status")
        .link("writes", "op:approve-leave", "ent:employee")
        .link("produces", "op:approve-leave", "out:approved")
        .link("produces", "op:approve-leave", "out:rejected")
        .link("governed_by", "op:approve-leave", "dt:approval-routing");
        // rules, calculations, calendars
        f.add(
            "dt:approval-routing",
            "DecisionTable",
            json!({"name": "Approval routing", "hit_policy": "UNIQUE",
                   "inputs": [{"name": "leave_days", "type": "decimal"}],
                   "outputs": [{"name": "approver", "type": "text"}],
                   "rows": [{"when": ["<= 5"], "then": ["manager"]}]}),
        )
        .add(
            "rule:max-consecutive",
            "Rule",
            json!({"name": "Maximum consecutive days", "rule_kind": "validation",
                   "condition_expr": "leave_days <= 20"}),
        )
        .add(
            "calc:leave-days",
            "Calculation",
            json!({"name": "LeaveDays", "expression": "working_days(period)", "result_type": "decimal"}),
        )
        .add(
            "cal:hr-pl",
            "Calendar",
            json!({"time_zone": "Europe/Warsaw", "week_pattern": ["mon", "tue", "wed", "thu", "fri"],
                   "region": "PL"}),
        );
        // events
        f.add(
            "evt:leave-approved",
            "Event",
            json!({"name": "LeaveApproved", "payload_schema_ref": "schema:leave-approved"}),
        )
        .add(
            "evt:request-expired",
            "Event",
            json!({"name": "RequestExpired"}),
        )
        .add(
            "schema:leave-approved",
            "DataSchema",
            json!({"name": "LeaveApproved", "schema_kind": "json_schema"}),
        )
        .link("schema_for", "schema:leave-approved", "attr:leave-status");
        // requirements
        f.add(
            "req:HR-001",
            "Requirement",
            json!({"statement": "The system shall let a manager approve a pending leave request.",
                   "requirement_kind": "functional", "level": "system", "modality": "shall"}),
        )
        .link("specified_by", "req:HR-001", "op:approve-leave")
        .add(
            "ac:HR-001-a",
            "AcceptanceCriterion",
            json!({"statement": "An approved request shows as approved.", "criterion_kind": "given_when_then"}),
        );
        // processes
        f.add(
            "proc:leave-approval",
            "Process",
            json!({"name": "Leave approval"}),
        )
        .add("proc:escalation", "Process", json!({"name": "Escalation"}))
        .add(
            "pn:submitted",
            "ProcessNode",
            json!({"process_ref": "proc:leave-approval", "node_kind": "start",
                       "message_ref": "msg:leave-submitted"}),
        )
        .add(
            "pn:route",
            "ProcessNode",
            json!({"process_ref": "proc:leave-approval", "node_kind": "exclusive_gateway"}),
        )
        .add(
            "pn:approve",
            "ProcessNode",
            json!({"process_ref": "proc:leave-approval", "node_kind": "human_task",
                       "operation_ref": "op:approve-leave"}),
        )
        .add(
            "pn:done",
            "ProcessNode",
            json!({"process_ref": "proc:leave-approval", "node_kind": "end"}),
        )
        .link("next", "pn:submitted", "pn:route")
        .link("next", "pn:route", "pn:approve")
        .link("next", "pn:approve", "pn:done")
        .link("produces", "pn:approve", "evt:leave-approved")
        .link("produces", "pn:approve", "out:approved");
        // quality
        quality(
            &mut f,
            "qs:approval-latency",
            "latency_ms",
            json!(2000),
            "op:approve-leave",
        );
        quality(
            &mut f,
            "qs:retention",
            "retention",
            json!("P7Y"),
            "ent:leave-request",
        );
        quality(
            &mut f,
            "qs:throughput",
            "throughput",
            json!(50),
            "op:approve-leave",
        );
        // scenarios and assumptions
        f.add(
            "scn:approve-happy",
            "Scenario",
            json!({"name": "Approve", "scenario_kind": "acceptance",
                   "given": [{"entity": "ent:leave-request", "values": {"leave_days": 2}}],
                   "when": {"operation": "op:approve-leave", "actor": "brole:manager", "inputs": {}},
                   "then": [{"states": ["state:approved"]}, {"outcome": "out:approved"}],
                   "requirement_refs": ["req:HR-001"]}),
        )
        .add(
            "scn:no-operation",
            "Scenario",
            json!({"name": "No operation", "scenario_kind": "acceptance", "given": [],
                   "when": {"actor": "actor:employee"}, "then": [], "requirement_refs": ["req:HR-001"]}),
        )
        .add(
            "asm:carry-over",
            "Assumption",
            json!({"statement": "Up to five days carry over.", "owner_ref": "actor:line-manager",
                   "status": "Open", "finding_ref": "fnd:0123456789abcdef", "default_value": 5,
                   "expires_at": "2027-01-01T00:00:00.000000000Z"}),
        )
        .add(
            "asm:unbounded",
            "Assumption",
            json!({"statement": "Managers answer within a week.", "owner_ref": "actor:line-manager",
                   "status": "Open"}),
        );
        f
    }

    fn hr_graph() -> Graph {
        hr_fixture().graph()
    }

    fn profile() -> ValidationProfile {
        load_builtin_software_profile().unwrap()
    }

    fn project(graph: &Graph) -> FunctionalProjection {
        project_functional_v2(graph, &profile()).unwrap()
    }

    fn project_fixture(f: &Fixture) -> FunctionalProjection {
        project(&f.graph())
    }

    fn codes_of(notices: &[ProjectionNotice]) -> Vec<(&str, Vec<&str>)> {
        notices
            .iter()
            .map(|n| {
                (
                    n.code.as_str(),
                    n.source_refs.iter().map(Id::as_str).collect(),
                )
            })
            .collect()
    }

    fn has_notice(notices: &[ProjectionNotice], code: &str, refs: &[&str]) -> bool {
        notices.iter().any(|n| {
            n.code == code && n.source_refs.iter().map(Id::as_str).collect::<Vec<_>>() == refs
        })
    }

    // ------------------------------------------------------------------ golden (§§46-48)

    #[test]
    fn golden_hr_projection_matches_the_hand_authored_yaml() {
        let graph = hr_graph();
        assert_eq!(graph.semantic_hash().unwrap().as_str(), HR_SEMANTIC_HASH);
        let projection = project(&graph);
        let yaml = String::from_utf8(projection.yaml.clone()).unwrap();
        assert_eq!(yaml, GOLDEN_YAML);
        assert_eq!(
            projection.metadata.content_hash.as_str(),
            GOLDEN_CONTENT_HASH
        );
        assert_eq!(
            projection.metadata.content_hash,
            Hash::content_sha256(GOLDEN_YAML.as_bytes())
        );
    }

    #[test]
    fn golden_hr_projection_records_exactly_the_expected_notices() {
        let metadata = project(&hr_graph()).metadata;
        let expected_warnings: Vec<(&str, Vec<&str>)> = GOLDEN_WARNINGS
            .iter()
            .map(|(c, r)| (*c, r.to_vec()))
            .collect();
        let expected_lossy: Vec<(&str, Vec<&str>)> =
            GOLDEN_LOSSY.iter().map(|(c, r)| (*c, r.to_vec())).collect();
        assert_eq!(codes_of(&metadata.warnings), expected_warnings);
        assert_eq!(codes_of(&metadata.lossy_mappings), expected_lossy);
        for notice in metadata.warnings.iter().chain(&metadata.lossy_mappings) {
            assert!(codes::ALL.contains(&notice.code.as_str()));
            assert!(notice.source_refs.windows(2).all(|w| w[0] < w[1]));
            assert!(!notice.message.is_empty());
        }
    }

    #[test]
    fn projection_metadata_binds_graph_profile_version_and_bytes() {
        let graph = hr_graph();
        let profile = profile();
        let projection = project_functional_v2(&graph, &profile).unwrap();
        let metadata = &projection.metadata;
        assert_eq!(
            metadata.source_semantic_hash,
            graph.semantic_hash().unwrap()
        );
        assert_eq!(metadata.source_semantic_hash.kind(), HashKind::Semantic);
        assert_eq!(metadata.profile_hash, profile.profile_hash().unwrap());
        assert_eq!(metadata.profile_hash.as_str(), PROFILE_HASH);
        assert_eq!(metadata.projection_version, 2);
        assert_eq!(FUNCTIONAL_V2_PROJECTION_VERSION, 2);
        assert_eq!(FUNCTIONAL_V2_VERSION, 2);
        assert_eq!(
            metadata.content_hash,
            Hash::content_sha256(&projection.yaml)
        );
        assert_eq!(metadata.content_hash.kind(), HashKind::Generic);
        assert_eq!(
            projection.document.model_hash,
            metadata.source_semantic_hash
        );
        assert_eq!(validate_metadata(metadata), Ok(()));

        // Repetition is byte-identical.
        let again = project_functional_v2(&graph, &profile).unwrap();
        assert_eq!(again.yaml, projection.yaml);
        assert_eq!(again.metadata, projection.metadata);
        assert_eq!(again.document, projection.document);
    }

    #[test]
    fn projection_requires_the_graph_profile() {
        let graph = hr_fixture().graph_for("profile:other-software");
        assert_eq!(
            project_functional_v2(&graph, &profile()),
            Err(ProjectionError::ProfileMismatch {
                graph: id("profile:other-software"),
                profile: id(PROFILE),
            })
        );
    }

    #[test]
    fn document_has_exactly_the_legacy_top_level_shape() {
        let projection = project(&hr_graph());
        let value: Value = serde_yaml::from_slice(&projection.yaml).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let expected: BTreeSet<&str> = [
            "version",
            "model_hash",
            "glossary",
            "actors",
            "roles",
            "entities",
            "calendars",
            "calculations",
            "rules",
            "operations",
            "processes",
            "events",
            "requirements",
            "scenarios",
            "assumptions",
        ]
        .into_iter()
        .collect();
        assert_eq!(keys.iter().copied().collect::<BTreeSet<_>>(), expected);
        assert_eq!(value["version"], json!(2));
        for field in [
            "profile_hash",
            "projection_version",
            "content_hash",
            "warnings",
            "lossy_mappings",
        ] {
            assert!(value.get(field).is_none(), "{field}");
        }
        // Emitted in contract order, empty collections included.
        let text = String::from_utf8(projection.yaml).unwrap();
        let order: Vec<usize> = expected_order()
            .iter()
            .map(|key| text.find(&format!("\n{key}:")).map_or(0, |i| i + 1))
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
        assert!(text.contains("\ncalculations: []\n"));

        let empty = project_fixture(&Fixture::new());
        let value: Value = serde_yaml::from_slice(&empty.yaml).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 15);
        for key in expected_order().iter().skip(2) {
            assert_eq!(value[key], json!([]), "{key}");
        }
    }

    fn expected_order() -> [&'static str; 15] {
        [
            "version",
            "model_hash",
            "glossary",
            "actors",
            "roles",
            "entities",
            "calendars",
            "calculations",
            "rules",
            "operations",
            "processes",
            "events",
            "requirements",
            "scenarios",
            "assumptions",
        ]
    }

    // ------------------------------------------------------------------ layout independence (§49)

    fn with_view(layout: char, style: char) -> Fixture {
        let mut f = hr_fixture();
        f.add(
            "view:leave-process",
            "View",
            json!({"name": "Leave process", "viewpoint_ref": "vp:hr:functional",
                   "architecture_description_ref": "ad:hr:leave", "projection_rules": [],
                   "layout_ref": format!("sha256:{}", layout.to_string().repeat(64)),
                   "style_ref": format!("sha256:{}", style.to_string().repeat(64))}),
        );
        f
    }

    #[test]
    fn view_layout_and_style_do_not_change_the_projection() {
        let a = with_view('a', 'b').graph();
        let b = with_view('c', 'd').graph();
        assert_eq!(a.semantic_hash().unwrap(), b.semantic_hash().unwrap());
        let pa = project(&a);
        let pb = project(&b);
        assert_eq!(pa.yaml, pb.yaml);
        assert_eq!(pa.metadata, pb.metadata);
        assert!(!String::from_utf8(pa.yaml).unwrap().contains("view:"));
    }

    // ------------------------------------------------------------------ status (§50)

    #[test]
    fn only_accepted_elements_are_projected() {
        for (status, warned) in [
            ("Proposed", false),
            ("Rejected", false),
            ("Suspect", true),
            ("Superseded", true),
            ("Deprecated", true),
        ] {
            let mut f = Fixture::new();
            f.add(
                "actor:accepted",
                "Actor",
                json!({"name": "A", "actor_kind": "human"}),
            )
            .add_with_status(
                "actor:other",
                status,
                "Actor",
                json!({"name": "B", "actor_kind": "human"}),
            )
            .add_with_status(
                "concept:other",
                status,
                "Concept",
                json!({"name": "Other", "definition": "d", "concept_kind": "other"}),
            );
            let projection = project_fixture(&f);
            let actors: Vec<&str> = projection
                .document
                .actors
                .iter()
                .map(|a| a.id.as_str())
                .collect();
            assert_eq!(actors, ["actor:accepted"], "{status}");
            assert!(projection.document.glossary.is_empty(), "{status}");
            let warnings = &projection.metadata.warnings;
            assert_eq!(
                has_notice(
                    warnings,
                    "V2_NON_ACCEPTED_BASELINE_OMITTED",
                    &["actor:other"]
                ),
                warned,
                "{status}"
            );
            assert_eq!(
                has_notice(
                    warnings,
                    "V2_NON_ACCEPTED_BASELINE_OMITTED",
                    &["concept:other"]
                ),
                warned,
                "{status}"
            );
            if !warned {
                assert!(warnings.is_empty(), "{status}");
            }
        }
    }

    #[test]
    fn only_accepted_edges_are_read() {
        let mut f = Fixture::new();
        operation(&mut f, "op:a", json!(null));
        f.add(
            "actor:x",
            "Actor",
            json!({"name": "X", "actor_kind": "human"}),
        )
        .add(
            "actor:y",
            "Actor",
            json!({"name": "Y", "actor_kind": "human"}),
        )
        .link("performed_by", "op:a", "actor:x")
        .link_with_status("Proposed", "performed_by", "op:a", "actor:y");
        let projection = project_fixture(&f);
        assert_eq!(projection.document.operations[0].actor, id("actor:x"));
        assert!(projection.metadata.lossy_mappings.is_empty());

        // A Suspect performer edge is not read and is recorded.
        let mut f = Fixture::new();
        operation(&mut f, "op:a", json!(null));
        f.add(
            "actor:x",
            "Actor",
            json!({"name": "X", "actor_kind": "human"}),
        )
        .link_with_status("Suspect", "performed_by", "op:a", "actor:x");
        let projection = project_fixture(&f);
        assert!(projection.document.operations.is_empty());
        let warnings = &projection.metadata.warnings;
        assert!(has_notice(
            warnings,
            "V2_NON_ACCEPTED_BASELINE_OMITTED",
            &["edge:001"]
        ));
        assert!(has_notice(
            warnings,
            "V2_OPERATION_PERFORMER_MISSING",
            &["op:a"]
        ));
    }

    // ------------------------------------------------------------------ roles / security (§51)

    #[test]
    fn roles_and_security_follow_the_typed_relations() {
        let graph = hr_graph();
        let projection = project(&graph);
        let doc = &projection.document;
        let kinds: Vec<(&str, LegacyActorKind)> =
            doc.actors.iter().map(|a| (a.id.as_str(), a.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("actor:employee", LegacyActorKind::Human),
                ("actor:hr-system", LegacyActorKind::System),
                ("actor:line-manager", LegacyActorKind::Human),
                ("actor:payroll-provider", LegacyActorKind::External),
            ]
        );
        let role = |role_id: &str| doc.roles.iter().find(|r| r.id.as_str() == role_id).unwrap();
        // BusinessRole: self-token actor, operations from performed_by.
        assert_eq!(role("brole:manager").actor, id("brole:manager"));
        assert_eq!(role("brole:manager").operations, [id("op:approve-leave")]);
        // SecurityRole with one assigned actor: the real actor; operations via grants/permits.
        assert_eq!(role("srole:leave-approver").actor, id("actor:line-manager"));
        assert_eq!(
            role("srole:leave-approver").operations,
            [id("op:approve-leave")]
        );
        // SecurityRole without a unique actor: self-token.
        assert_eq!(role("srole:hr-admin").actor, id("srole:hr-admin"));

        // Two assigned actors are never picked from.
        let mut f = hr_fixture();
        f.link("assigned_role", "actor:employee", "srole:leave-approver");
        let projection = project_fixture(&f);
        let approver = projection
            .document
            .roles
            .iter()
            .find(|r| r.id.as_str() == "srole:leave-approver")
            .unwrap();
        assert_eq!(approver.actor, id("srole:leave-approver"));
        assert!(has_notice(
            &projection.metadata.lossy_mappings,
            "V2_SECURITY_ROLE_COLLAPSE",
            &[
                "actor:employee",
                "actor:line-manager",
                "srole:leave-approver"
            ]
        ));

        // A permission condition is part of the recorded loss.
        let mut f = hr_fixture();
        f.add(
            "cond:own-team",
            "PolicyCondition",
            json!({"expression": "team == actor.team"}),
        )
        .link("conditioned_by", "perm:approve-leave", "cond:own-team");
        let projection = project_fixture(&f);
        assert!(has_notice(
            &projection.metadata.lossy_mappings,
            "V2_PERMISSION_SCOPE_OMITTED",
            &[
                "cond:own-team",
                "perm:approve-leave",
                "scope:own-team",
                "srole:leave-approver"
            ]
        ));
    }

    #[test]
    fn projection_neither_mutates_the_graph_nor_creates_actors() {
        let graph = hr_graph();
        let before = graph.clone();
        let projection = project(&graph);
        assert_eq!(graph, before);
        let actor_ids: BTreeSet<&str> = projection
            .document
            .actors
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        for role in &projection.document.roles {
            // Self-tokens are role IDs, never new actors.
            if !actor_ids.contains(role.actor.as_str()) {
                assert_eq!(role.actor, role.id);
                assert!(graph.node(&role.actor).is_some());
            }
        }
        assert_eq!(projection.document.actors.len(), 4);
    }

    // ------------------------------------------------------------------ NFR (§52)

    fn nfr_fixture(scenarios: &[(&str, &str, Value, &str)], idempotency: Value) -> Fixture {
        let mut f = Fixture::new();
        entity(&mut f, "ent:x");
        operation(&mut f, "op:x", idempotency);
        f.add(
            "actor:x",
            "Actor",
            json!({"name": "X", "actor_kind": "human"}),
        )
        .link("performed_by", "op:x", "actor:x");
        for (qs, characteristic, threshold, target) in scenarios {
            quality(&mut f, qs, characteristic, threshold.clone(), target);
        }
        f
    }

    #[test]
    fn exact_entity_characteristics_map_to_entity_nfr() {
        for key in [
            "consistency",
            "availability",
            "volatility",
            "residency",
            "retention",
        ] {
            let projection = project_fixture(&nfr_fixture(
                &[("qs:a", key, json!("strong"), "ent:x")],
                json!(null),
            ));
            let nfr = serde_json::to_value(projection.document.entities[0].nfr.as_ref().unwrap())
                .unwrap();
            assert_eq!(nfr, json!({key: "strong"}), "{key}");
            assert!(has_notice(
                &projection.metadata.lossy_mappings,
                "V2_QUALITY_SCENARIO_COLLAPSE",
                &["ent:x", "qs:a"]
            ));
        }
        // No case folding or fuzzy matching.
        for name in ["Retention", "retention ", "data_retention", "latency_ms"] {
            let projection = project_fixture(&nfr_fixture(
                &[("qs:a", name, json!("x"), "ent:x")],
                json!(null),
            ));
            assert_eq!(projection.document.entities[0].nfr, None, "{name}");
            assert!(has_notice(
                &projection.metadata.warnings,
                "V2_QUALITY_SCENARIO_OMITTED",
                &["qs:a"]
            ));
        }
    }

    #[test]
    fn operation_characteristics_require_typed_thresholds() {
        let op_nfr = |name: &str, threshold: Value| {
            project_fixture(&nfr_fixture(
                &[("qs:a", name, threshold, "op:x")],
                json!(null),
            ))
        };
        let mapped = op_nfr("latency_ms", json!(250));
        assert_eq!(
            mapped.document.operations[0]
                .nfr
                .as_ref()
                .unwrap()
                .latency_ms,
            Some(json!(250))
        );
        let mapped = op_nfr("audit", json!(true));
        assert_eq!(
            mapped.document.operations[0].nfr.as_ref().unwrap().audit,
            Some(true)
        );
        let mapped = op_nfr("idempotent", json!(false));
        assert_eq!(
            mapped.document.operations[0]
                .nfr
                .as_ref()
                .unwrap()
                .idempotent,
            Some(false)
        );
        for (name, threshold) in [
            ("latency_ms", json!("250")),
            ("audit", json!("yes")),
            ("idempotent", json!(1)),
            ("throughput", json!(10)),
        ] {
            let omitted = op_nfr(name, threshold);
            assert_eq!(omitted.document.operations[0].nfr, None, "{name}");
            assert!(has_notice(
                &omitted.metadata.warnings,
                "V2_QUALITY_SCENARIO_OMITTED",
                &["qs:a"]
            ));
        }
        // A scenario targeting a non-entity, non-operation or several targets is omitted.
        let mut f = nfr_fixture(&[], json!(null));
        f.add(
            "req:q",
            "Requirement",
            json!({"statement": "s", "requirement_kind": "quality",
                                             "level": "system", "modality": "shall"}),
        );
        quality(&mut f, "qs:a", "retention", json!("P1Y"), "req:q");
        assert!(has_notice(
            &project_fixture(&f).metadata.warnings,
            "V2_QUALITY_SCENARIO_OMITTED",
            &["qs:a"]
        ));
    }

    #[test]
    fn identical_nfr_values_merge_and_conflicting_ones_fail() {
        let same = project_fixture(&nfr_fixture(
            &[
                ("qs:a", "latency_ms", json!(250), "op:x"),
                ("qs:b", "latency_ms", json!(250), "op:x"),
            ],
            json!(null),
        ));
        assert_eq!(
            same.document.operations[0].nfr.as_ref().unwrap().latency_ms,
            Some(json!(250))
        );
        assert!(has_notice(
            &same.metadata.lossy_mappings,
            "V2_QUALITY_SCENARIO_COLLAPSE",
            &["op:x", "qs:a"]
        ));
        assert!(has_notice(
            &same.metadata.lossy_mappings,
            "V2_QUALITY_SCENARIO_COLLAPSE",
            &["op:x", "qs:b"]
        ));

        let conflict = nfr_fixture(
            &[
                ("qs:a", "latency_ms", json!(250), "op:x"),
                ("qs:b", "latency_ms", json!(300), "op:x"),
            ],
            json!(null),
        );
        assert_eq!(
            project_functional_v2(&conflict.graph(), &profile()),
            Err(ProjectionError::ConflictingLegacyNfr {
                target: id("op:x"),
                key: "latency_ms".to_owned()
            })
        );
        // The idempotency field and a quality scenario must agree too.
        let agree = project_fixture(&nfr_fixture(
            &[("qs:a", "idempotent", json!(true), "op:x")],
            json!("true"),
        ));
        assert_eq!(
            agree.document.operations[0]
                .nfr
                .as_ref()
                .unwrap()
                .idempotent,
            Some(true)
        );
        let disagree = nfr_fixture(
            &[("qs:a", "idempotent", json!(false), "op:x")],
            json!("true"),
        );
        assert!(matches!(
            project_functional_v2(&disagree.graph(), &profile()),
            Err(ProjectionError::ConflictingLegacyNfr { .. })
        ));
    }

    #[test]
    fn idempotency_maps_only_exact_booleans() {
        for (value, expected) in [(json!("true"), Some(true)), (json!("false"), Some(false))] {
            let projection = project_fixture(&nfr_fixture(&[], value));
            assert_eq!(
                projection.document.operations[0]
                    .nfr
                    .as_ref()
                    .unwrap()
                    .idempotent,
                expected
            );
        }
        for value in ["True", "yes", "at-least-once"] {
            let projection = project_fixture(&nfr_fixture(&[], json!(value)));
            assert_eq!(projection.document.operations[0].nfr, None, "{value}");
            assert!(has_notice(
                &projection.metadata.warnings,
                "V2_IDEMPOTENCY_UNREPRESENTABLE",
                &["op:x"]
            ));
        }
        assert_eq!(
            project_fixture(&nfr_fixture(&[], json!(null)))
                .document
                .operations[0]
                .nfr,
            None
        );
    }

    // ------------------------------------------------------------------ functional mappings (§53)

    #[test]
    fn functional_mappings_follow_typed_relations() {
        let doc = project(&hr_graph()).document;
        let entity = doc
            .entities
            .iter()
            .find(|e| e.id.as_str() == "ent:leave-request")
            .unwrap();
        let attributes: Vec<&str> = entity.attributes.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(attributes, ["attr:leave-days", "attr:leave-status"]);
        assert_eq!(entity.attributes[0].unit.as_deref(), Some("day"));
        assert_eq!(entity.attributes[0].precision, Some(1));
        assert_eq!(entity.attributes[0].class, "none");
        assert_eq!(entity.attributes[1].class, "confidential");
        let rel = &entity.relationships[0];
        assert_eq!(
            (
                rel.to.as_str(),
                rel.card_from.as_str(),
                rel.card_to.as_str(),
                rel.snapshot
            ),
            ("ent:employee", "many", "one", true)
        );
        // The relationship is nested under its from_entity only.
        let employee = doc
            .entities
            .iter()
            .find(|e| e.id.as_str() == "ent:employee")
            .unwrap();
        assert!(employee.relationships.is_empty());
        assert_eq!(entity.states.len(), 3);
        assert_eq!(entity.transitions.len(), 1);
        assert_eq!(entity.transitions[0].operation, id("op:approve-leave"));
        assert_eq!(entity.invariants[0].expr, "leave_days > 0");

        let op = &doc.operations[0];
        assert_eq!(op.id, id("op:approve-leave"));
        assert_eq!(op.actor, id("brole:manager"));
        assert_eq!(op.reads, [id("attr:leave-days")]);
        assert_eq!(op.writes, [id("attr:leave-status")]);
        assert_eq!(op.governed_by, [id("dt:approval-routing")]);
        let outcomes: Vec<(&str, LegacyOutcomeKind)> = op
            .outcomes
            .iter()
            .map(|o| (o.id.as_str(), o.kind))
            .collect();
        assert_eq!(
            outcomes,
            [
                ("out:approved", LegacyOutcomeKind::Success),
                ("out:rejected", LegacyOutcomeKind::Failure)
            ]
        );

        assert_eq!(doc.events[0].payload, [id("attr:leave-status")]);
        assert_eq!(doc.requirements[0].operations, [id("op:approve-leave")]);
        assert_eq!(
            doc.rules[0].conditions,
            [json!({"name": "leave_days", "type": "decimal"})]
        );
        let process = &doc.processes[0];
        assert_eq!(
            (
                process.trigger.kind.as_str(),
                process.trigger.reference.as_str()
            ),
            ("event", "msg:leave-submitted")
        );
        assert_eq!(process.steps[0].emits, [id("evt:leave-approved")]);
        assert_eq!(doc.scenarios[0].requirement, id("req:HR-001"));
        assert_eq!(doc.assumptions[0].default, json!(5));
    }

    #[test]
    fn performer_and_process_rules() {
        // Several performers: smallest ID, collapse recorded with all of them.
        let mut f = Fixture::new();
        operation(&mut f, "op:a", json!(null));
        f.add(
            "actor:z",
            "Actor",
            json!({"name": "Z", "actor_kind": "human"}),
        )
        .add("brole:b", "BusinessRole", json!({"name": "B"}))
        .link("performed_by", "op:a", "actor:z")
        .link("performed_by", "op:a", "brole:b");
        let projection = project_fixture(&f);
        assert_eq!(projection.document.operations[0].actor, id("actor:z"));
        assert!(has_notice(
            &projection.metadata.lossy_mappings,
            "V2_MULTIPLE_PERFORMERS_COLLAPSED",
            &["actor:z", "brole:b", "op:a"]
        ));

        // A process node's own performer wins over its operation's; next carries the condition.
        let mut f = hr_fixture();
        f.link("performed_by", "pn:approve", "actor:line-manager");
        let nodes = &mut f.nodes;
        let approve = nodes
            .iter_mut()
            .find(|n| n.id.as_str() == "pn:approve")
            .unwrap();
        *approve = node(
            "pn:approve",
            "Accepted",
            "ProcessNode",
            json!({"process_ref": "proc:leave-approval", "node_kind": "human_task",
                   "operation_ref": "op:approve-leave", "condition_expr": "approved"}),
        );
        let doc = project_fixture(&f).document;
        let step = &doc.processes[0].steps[0];
        assert_eq!(step.actor, id("actor:line-manager"));
        assert_eq!(step.next[0].when.as_deref(), Some("approved"));

        // A timer start gives a timer trigger; a start with both refs is not representable.
        let mut f = Fixture::new();
        f.add("proc:t", "Process", json!({"name": "T"})).add(
            "pn:s",
            "ProcessNode",
            json!({"process_ref": "proc:t", "node_kind": "start", "timer_expr": "P1D"}),
        );
        let doc = project_fixture(&f).document;
        assert_eq!(
            (
                doc.processes[0].trigger.kind.as_str(),
                doc.processes[0].trigger.reference.as_str()
            ),
            ("timer", "P1D")
        );
        let mut f = Fixture::new();
        f.add("proc:t", "Process", json!({"name": "T"})).add(
            "pn:s",
            "ProcessNode",
            json!({"process_ref": "proc:t", "node_kind": "start",
                                              "timer_expr": "P1D", "message_ref": "msg:m"}),
        );
        let projection = project_fixture(&f);
        assert!(projection.document.processes.is_empty());
        assert!(has_notice(
            &projection.metadata.warnings,
            "V2_PROCESS_TRIGGER_UNREPRESENTABLE",
            &["pn:s", "proc:t"]
        ));
    }

    #[test]
    fn scenario_requirement_and_then_rules() {
        let scenario = |refs: Value, then: Value| {
            let mut f = Fixture::new();
            f.add(
                "scn:s",
                "Scenario",
                json!({"name": "S", "scenario_kind": "acceptance", "given": [],
                "when": {"operation": "op:a"}, "then": then, "requirement_refs": refs}),
            );
            f
        };
        let multi = project_fixture(&scenario(json!(["req:b", "req:a"]), json!([])));
        assert_eq!(multi.document.scenarios[0].requirement, id("req:a"));
        assert!(multi.document.scenarios[0].then.is_empty());
        assert!(has_notice(
            &multi.metadata.lossy_mappings,
            "V2_SCENARIO_MULTI_REQUIREMENT_COLLAPSE",
            &["req:a", "req:b", "scn:s"]
        ));
        let none = project_fixture(&scenario(json!([]), json!([])));
        assert!(none.document.scenarios.is_empty());
        assert!(has_notice(
            &none.metadata.warnings,
            "V2_SCENARIO_REQUIREMENT_MISSING",
            &["scn:s"]
        ));
        assert!(none.metadata.lossy_mappings.is_empty());
        assert!(!none
            .metadata
            .warnings
            .iter()
            .any(|n| n.code == "V2_SCENARIO_MULTI_REQUIREMENT_COLLAPSE"));
        let conflict = project_fixture(&scenario(json!(["req:a"]), json!([{"x": 1}, {"x": 2}])));
        assert!(conflict.document.scenarios.is_empty());
        assert!(has_notice(
            &conflict.metadata.warnings,
            "V2_SCENARIO_THEN_CONFLICT",
            &["scn:s"]
        ));
        let merged = project_fixture(&scenario(json!(["req:a"]), json!([{"x": 1}, {"y": 2}])));
        assert_eq!(
            serde_json::to_value(&merged.document.scenarios[0].then).unwrap(),
            json!({"x": 1, "y": 2})
        );
        // A non-object then/given entry cannot be carried by any notice code: explicit error.
        assert!(matches!(
            project_functional_v2(
                &scenario(json!(["req:a"]), json!(["done"])).graph(),
                &profile()
            ),
            Err(ProjectionError::UnrepresentableElement { .. })
        ));
    }

    #[test]
    fn projection_is_deterministic_and_sorted() {
        let a = project(&hr_graph());
        // Same content inserted in reverse order.
        let mut f = hr_fixture();
        f.nodes.reverse();
        f.edges.reverse();
        let b = project_fixture(&f);
        assert_eq!(a.yaml, b.yaml);
        assert_eq!(a.metadata, b.metadata);
        let doc = &a.document;
        assert!(doc.glossary.windows(2).all(|w| w[0].id < w[1].id));
        assert!(doc.roles.windows(2).all(|w| w[0].id < w[1].id));
        assert!(doc.entities.windows(2).all(|w| w[0].id < w[1].id));
        assert!(doc.events.windows(2).all(|w| w[0].id < w[1].id));
    }

    // ------------------------------------------------------------------ unrepresentable (§54)

    #[test]
    fn unrepresentable_content_is_omitted_and_recorded() {
        let projection = project(&hr_graph());
        let doc = &projection.document;
        let warnings = &projection.metadata.warnings;
        assert!(doc.calculations.is_empty());
        assert!(has_notice(
            warnings,
            "V2_CALCULATION_TARGET_UNREPRESENTABLE",
            &["calc:leave-days"]
        ));
        assert!(doc
            .rules
            .iter()
            .all(|r| r.id.as_str() != "rule:max-consecutive"));
        assert!(has_notice(
            warnings,
            "V2_RULE_TABLE_UNREPRESENTABLE",
            &["rule:max-consecutive"]
        ));
        assert!(doc.requirements[0].criteria.is_empty());
        assert!(has_notice(
            warnings,
            "V2_ACCEPTANCE_CRITERION_UNMAPPED",
            &["ac:HR-001-a"]
        ));
        assert!(has_notice(
            warnings,
            "V2_EVENT_TRIGGER_TRANSITION_OMITTED",
            &["evt:request-expired", "tr:expire"]
        ));
        assert!(doc
            .processes
            .iter()
            .all(|p| p.id.as_str() != "proc:escalation"));
        assert!(has_notice(
            warnings,
            "V2_PROCESS_TRIGGER_UNREPRESENTABLE",
            &["proc:escalation"]
        ));
        assert!(doc
            .operations
            .iter()
            .all(|o| o.id.as_str() != "op:submit-leave"));
        assert!(has_notice(
            warnings,
            "V2_OPERATION_PERFORMER_MISSING",
            &["op:submit-leave"]
        ));
        assert!(doc
            .scenarios
            .iter()
            .all(|s| s.id.as_str() != "scn:no-operation"));
        assert!(has_notice(
            warnings,
            "V2_SCENARIO_OPERATION_MISSING",
            &["scn:no-operation"]
        ));
        assert!(doc
            .assumptions
            .iter()
            .all(|a| a.id.as_str() != "asm:unbounded"));
        assert!(has_notice(
            warnings,
            "V2_ASSUMPTION_UNREPRESENTABLE",
            &["asm:unbounded"]
        ));

        // A decision table with a non-object row is not a legacy rule table.
        let mut f = Fixture::new();
        f.add(
            "dt:bad",
            "DecisionTable",
            json!({"name": "Bad", "hit_policy": "", "inputs": [],
                                               "outputs": [], "rows": ["not an object"]}),
        );
        let projection = project_fixture(&f);
        assert!(projection.document.rules.is_empty());
        assert!(has_notice(
            &projection.metadata.warnings,
            "V2_RULE_TABLE_UNREPRESENTABLE",
            &["dt:bad"]
        ));
    }

    // ------------------------------------------------------------------ schema (§§43-44, 55)

    fn golden_json() -> Value {
        serde_json::to_value(&project(&hr_graph()).document).unwrap()
    }

    #[test]
    fn golden_document_validates_with_the_production_validator() {
        assert_eq!(validate_against_schema(&golden_json()), Ok(()));
        // The hand-authored golden itself is a valid document.
        let hand_authored: Value = serde_yaml::from_str(GOLDEN_YAML).unwrap();
        assert_eq!(validate_against_schema(&hand_authored), Ok(()));
        let from_yaml: Value = serde_yaml::from_slice(&project(&hr_graph()).yaml).unwrap();
        assert_eq!(validate_against_schema(&from_yaml), Ok(()));
    }

    #[test]
    fn schema_rejects_malformed_documents() {
        let rejects = |mutate: &dyn Fn(&mut Value)| {
            let mut value = golden_json();
            mutate(&mut value);
            matches!(
                validate_against_schema(&value),
                Err(ProjectionError::Schema(_))
            )
        };
        assert!(rejects(&|v| v["version"] = json!(3)));
        assert!(rejects(&|v| v["version"] = json!("2")));
        assert!(rejects(&|v| v["model_hash"] = json!("fm:abc")));
        assert!(rejects(
            &|v| v["model_hash"] = json!(format!("sha256:{}", "a".repeat(64)))
        ));
        assert!(rejects(&|v| {
            v.as_object_mut().unwrap().remove("calculations");
        }));
        assert!(rejects(&|v| v["warnings"] = json!([])));
        assert!(rejects(&|v| v["actors"][0]["kind"] = json!("organization")));
        assert!(rejects(&|v| v["operations"][0]["kind"] = json!("event")));
        assert!(rejects(&|v| {
            v["entities"][1]["attributes"][0]
                .as_object_mut()
                .unwrap()
                .remove("type");
        }));
        assert!(rejects(
            &|v| v["entities"][1]["attributes"][0]["class"] = json!("secret")
        ));
        assert!(rejects(&|v| v["roles"][0]["extra"] = json!(true)));
        assert!(rejects(
            &|v| v["processes"][0]["trigger"]["kind"] = json!("manual")
        ));
        assert!(rejects(&|v| v["scenarios"][0]["given"] = json!(["x"])));
        assert!(rejects(&|v| v["glossary"][0]["id"] = json!("Not An Id")));
        // Optional fields may be absent.
        let mut value = golden_json();
        value["entities"][1]["attributes"][0]
            .as_object_mut()
            .unwrap()
            .remove("unit");
        value["operations"][0]
            .as_object_mut()
            .unwrap()
            .remove("nfr");
        assert_eq!(validate_against_schema(&value), Ok(()));
    }

    #[test]
    fn schema_is_validated_as_draft_2020_12() {
        let schema: Value = serde_json::from_str(FUNCTIONAL_V2_SCHEMA).unwrap();
        assert_eq!(
            schema["$schema"],
            json!("https://json-schema.org/draft/2020-12/schema")
        );
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["properties"]["version"], json!({"const": 2}));
        assert_eq!(
            schema["properties"]["model_hash"]["pattern"],
            json!("^psg:sha256:[0-9a-f]{64}$")
        );
        assert_eq!(schema["required"].as_array().unwrap().len(), 15);
        assert!(!FUNCTIONAL_V2_SCHEMA.contains("warnings"));
        assert!(!FUNCTIONAL_V2_SCHEMA.contains("content_hash"));

        // The production validator uses the draft 2020-12 dialect ...
        assert_eq!(
            project::FUNCTIONAL_V2_SCHEMA_DRAFT,
            jsonschema::Draft::Draft202012
        );
        let source = include_str!("../src/project.rs");
        assert!(source.contains(".with_draft(FUNCTIONAL_V2_SCHEMA_DRAFT)"));
        assert!(!source.contains("Draft7"));
        // ... under which the schema compiles and decides the golden and a mutation.
        let compiled = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(&schema)
            .unwrap();
        assert!(compiled.is_valid(&golden_json()));
        let mut bad = golden_json();
        bad["version"] = json!(3);
        assert!(!compiled.is_valid(&bad));
        // A 2020-12-only keyword is enforced, which a draft 7 validator would ignore.
        let tuple = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(
                &json!({"$schema": "https://json-schema.org/draft/2020-12/schema",
                             "prefixItems": [{"type": "integer"}]}),
            )
            .unwrap();
        assert!(!tuple.is_valid(&json!(["not an integer"])));
    }

    // ------------------------------------------------------------------ output-only guard (§56)

    #[test]
    fn production_source_is_output_only() {
        for (name, source) in [
            ("lib.rs", include_str!("../src/lib.rs")),
            ("model.rs", include_str!("../src/model.rs")),
            ("project.rs", include_str!("../src/project.rs")),
        ] {
            for forbidden in [
                "import_functional_v2",
                "load_functional_into_graph",
                "apply_functional_yaml",
                "functional_to_psg",
                "serde_yaml::from",
                "Deserialize",
                "SemanticPatch",
                "PatchSet",
                "Proposal",
                "Graph::new",
                "SystemClock",
            ] {
                assert!(!source.contains(forbidden), "{name} contains {forbidden}");
            }
        }
    }
}
