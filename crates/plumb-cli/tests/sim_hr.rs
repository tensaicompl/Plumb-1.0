//! End-to-end simulation of the HR leave pilot through the real CLI (plumb-goal-approach).
//!
//! Each inference stage is first executed in-process over a copy of the database with a
//! scripted provider that answers the way a correct model would for the HR fixture; the
//! recorded executions are then replayed through the `plumb-cli` binary with
//! `--provider replay` over the real database. Deterministic stages run through the CLI
//! directly. Run with `cargo test -p plumb-cli --test sim_hr -- --nocapture`; set `SIM_DIR`
//! to keep the working directory (database, config, fixtures, request contexts).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use plumb_api::application::{open_application, ApplicationRuntime};
use plumb_api::{ApiIdentity, CalendarConfig, DirectManagerFact, DirectManagerPredicates, Recipe};
use plumb_core::{to_canonical_json, CanonicalJson, FixedClock, Hash, Id, Timestamp};
use plumb_functional::{ExprCalendarDefinition, ExprStaticCalendarProvider};
use plumb_inference::{
    validate_binding, InferenceArtifact, InferenceProvider, InferenceRequest,
    InferenceRequestMaterial, ProviderError, ProviderExecution,
};
use plumb_store::BranchName;
use plumb_validation::load_builtin_software_profile;
use serde_json::{json, Value};

const ACTOR: &str = "agent:cli";
const ASSISTANT: &str = "agent:assistant";
const STAGE_ACTOR: &str = "agent:compiler";
const T0: &str = "2026-01-01T00:00:00.000000000Z";

fn id(s: &str) -> Id {
    s.parse().unwrap()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/hr-leave")
        .join(name)
}

// ---------------------------------------------------------------------- configuration

fn config() -> Value {
    json!({
        "version": 1,
        "identity": {"user": "alice", "actor_ref": ACTOR, "roles": ["analyst"], "mode": "pilot"},
        "stage_actor_ref": STAGE_ACTOR,
        "assistant_actor_ref": ASSISTANT,
        "provider_policy": {"provider": "test", "config": {}},
        "session_turn": {"retrieval_top_k": 3, "max_open_questions": 3, "max_context_bytes": 65536},
        "validation_policy": {"promoted_to_blocker_rule_ids": [], "profile_allow_waiver_rule_ids": []},
        "calendars": [{"calendar_ref": "cal:office", "weekend": ["saturday", "sunday"], "holidays": ["2026-12-25"]}],
        "direct_managers": [{"manager_ref": "actor:manager", "employee_ref": "actor:employee"}],
        "expression_scopes": [],
        "calculation_scope_overrides": [],
        "runtime_semantic_values": [],
        "vocabulary_policy": {"language": "en"},
        // The fixture profile's threshold (fixtures/hr-leave/profile.yaml).
        "duplicate_policy": {"jaccard_threshold": "0.8"},
        "calculation_scope": null,
        "operation_scope": null,
        "process_scope": null,
        "authorization_draft": null,
        "scenario_derivation": {
            "decision_tables": [{"decision_table_ref": "dt:annual-eligibility", "spec": eligibility_table()}],
            "boundaries": [],
        },
        "verification_applicability": {"sources": []},
        "directory": directory(),
        "question_routing": {
            "stakeholders": [
                {"id": "stakeholder:employee", "name": "Employee", "roles": ["business"]},
                {"id": "stakeholder:manager", "name": "Manager", "roles": ["business"]},
                {"id": "stakeholder:hr", "name": "HR Policy Owner", "roles": ["business", "analyst"]},
                {"id": "stakeholder:architect", "name": "Solution Architect", "roles": ["architect"]},
                {"id": "stakeholder:auditor", "name": "Auditor", "roles": ["auditor"]}
            ],
            "routing": {
                "calendar": ["stakeholder:hr"], "rounding": ["stakeholder:hr"],
                "formula": ["stakeholder:hr"],
                "authorization": ["stakeholder:hr", "stakeholder:architect"],
                "default": ["stakeholder:hr"]
            }
        },
        "gates": {
            "f1": {"lint_policy": {"severity_overrides": {}}},
            "f4": {"in_scope_scenario_refs": [], "required_scenario_kinds": [], "visible_assumption_refs": []}
        },
    })
}

/// The HR directory: stakeholders, actors, business roles, the office calendar and rules.
fn directory() -> Value {
    let stakeholder = |id: &str, name: &str, kind: &str| {
        json!({"id": id, "payload": {"name": name, "stakeholder_kind": kind, "organization": null,
               "responsibilities": null, "contact_ref": null}})
    };
    let rule = |id: &str, name: &str| {
        json!({"id": id, "payload": {"name": name, "rule_kind": "business",
               "condition_expr": null, "action_expr": null}})
    };
    json!({
        "stakeholders": [
            stakeholder("stakeholder:employee", "Employee", "business"),
            stakeholder("stakeholder:manager", "Manager", "business"),
            stakeholder("stakeholder:hr", "HR Policy Owner", "business"),
        ],
        "actors": [
            {"id": "actor:employee", "payload": {"name": "Employee", "actor_kind": "human"}},
            {"id": "actor:manager", "payload": {"name": "Manager", "actor_kind": "human"}},
            {"id": "actor:system", "payload": {"name": "Leave System", "actor_kind": "system"}},
        ],
        "business_roles": [
            {"id": "brole:employee", "payload": {"name": "EmployeeRole"}},
            {"id": "brole:manager", "payload": {"name": "ManagerRole"}},
        ],
        "calendars": [
            {"id": "calendar:office", "calendar_ref": "cal:office", "time_zone": "Europe/Warsaw",
             "region": "PL", "holiday_source": null},
        ],
        "rules": [
            rule("rule:annual-leave-eligibility", "AnnualLeaveEligibility"),
            rule("rule:manager-decision-authorization", "ManagerDecisionAuthorization"),
            rule("rule:overlap", "OverlapRule"),
        ],
        "decision_tables": [{
            "id": "dt:annual-eligibility", "name": "AnnualLeaveEligibility",
            "spec": eligibility_table(),
        }],
    })
}

/// AnnualLeaveEligibility (HR-014): an Employee contract is eligible, a Contractor is not.
fn eligibility_table() -> Value {
    json!({
        "hit_policy": "UNIQUE",
        "inputs": [{"name": "contract_type", "ty": {"type": "enum", "members": ["Contractor", "Employee"]},
                    "domain": null}],
        "outputs": [{"name": "eligible", "ty": {"type": "bool"}, "domain": null}],
        "rows": [
            {"inputs": [{"cell": "enum_set", "members": ["Employee"]}],
             "outputs": [{"cell": "bool", "value": true}]},
            {"inputs": [{"cell": "enum_set", "members": ["Contractor"]}],
             "outputs": [{"cell": "bool", "value": false}]},
        ],
        "default_output": null,
    })
}

/// The in-process runtime matching `config` with `provider` (mirrors the CLI's own build).
fn runtime(config: &Value, provider: Arc<Scripted>) -> ApplicationRuntime {
    let calendars: Vec<CalendarConfig> =
        serde_json::from_value(config["calendars"].clone()).unwrap();
    let managers: Vec<DirectManagerFact> =
        serde_json::from_value(config["direct_managers"].clone()).unwrap();
    let opt = |k: &str| {
        let v = &config[k];
        if v.is_null() {
            None
        } else {
            Some(v.clone())
        }
    };
    ApplicationRuntime {
        inference_provider: provider,
        provider_mode: plumb_api::ProviderMode::Replay {
            fixtures_hash: Hash::content_sha256(b"sim recorder"),
        },
        provider_policy: serde_json::from_value(config["provider_policy"].clone()).unwrap(),
        session_turn_config: serde_json::from_value(config["session_turn"].clone()).unwrap(),
        assistant_actor_ref: id(ASSISTANT),
        stage_actor_ref: id(STAGE_ACTOR),
        calendars: Arc::new(
            ExprStaticCalendarProvider::new(
                calendars
                    .iter()
                    .map(|c| ExprCalendarDefinition::new(c.calendar_ref.clone(), [], [])),
            )
            .unwrap(),
        ),
        predicates: Arc::new(DirectManagerPredicates::new(&managers).unwrap()),
        calendar_configs: calendars,
        direct_managers: managers,
        expression_scopes: serde_json::from_value(config["expression_scopes"].clone()).unwrap(),
        calculation_scope_overrides: serde_json::from_value(
            config["calculation_scope_overrides"].clone(),
        )
        .unwrap(),
        runtime_semantic_values: serde_json::from_value(config["runtime_semantic_values"].clone())
            .unwrap(),
        validation_policy: serde_json::from_value(config["validation_policy"].clone()).unwrap(),
        vocabulary_policy: serde_json::from_value(config["vocabulary_policy"].clone()).unwrap(),
        duplicate_policy: serde_json::from_value(config["duplicate_policy"].clone()).unwrap(),
        calculation_scope: opt("calculation_scope").map(|v| serde_json::from_value(v).unwrap()),
        operation_scope: opt("operation_scope").map(|v| serde_json::from_value(v).unwrap()),
        process_scope: opt("process_scope").map(|v| serde_json::from_value(v).unwrap()),
        authorization_draft: opt("authorization_draft").map(|v| serde_json::from_value(v).unwrap()),
        scenario_derivation: opt("scenario_derivation").map(|v| serde_json::from_value(v).unwrap()),
        verification_applicability: opt("verification_applicability")
            .map(|v| serde_json::from_value(v).unwrap()),
        gates: serde_json::from_value(config["gates"].clone()).unwrap(),
        directory: config
            .get("directory")
            .map(|d| serde_json::from_value(d.clone()).unwrap()),
        question_routing: config
            .get("question_routing")
            .map(|r| serde_json::from_value(r.clone()).unwrap()),
    }
}

// ---------------------------------------------------------------------- scripted model

/// Answers each task kind from the request context, like a correct model would for the HR
/// fixture; records every execution and dumps every context for inspection.
struct Scripted {
    dir: PathBuf,
    calls: Mutex<Vec<ProviderExecution>>,
}

fn execution(request: &InferenceRequest, output: Value) -> ProviderExecution {
    let raw_response = to_canonical_json(&output).unwrap();
    let validated_output = CanonicalJson::new(output);
    ProviderExecution {
        artifact: InferenceArtifact {
            request_hash: request.id.clone(),
            provider: request.provider_policy.provider.clone(),
            model: "scripted-hr".to_owned(),
            parameters: CanonicalJson::new(json!({})),
            raw_response_hash: Hash::content_sha256(&raw_response),
            validated_output_hash: validated_output.content_hash().unwrap(),
            validated_output,
        },
        raw_response_media_type: "application/json".to_owned(),
        raw_response,
    }
}

/// The bracketed requirement kind of an HR fixture line (`[functional]` etc.).
fn hr_kind(text: &str) -> Option<&'static str> {
    for kind in [
        "functional",
        "security",
        "constraint",
        "quality",
        "operational",
    ] {
        if text.contains(&format!("[{kind}]")) {
            return Some(kind);
        }
    }
    None
}

/// The HR domain vocabulary: phrase, Concept kind and the statement that would ground a
/// definition if the source had one (unused: the model returns no definitions; longest first).
const VOCABULARY: [(&str, Option<&str>, &str); 17] = [
    ("half-day", Some("value_type"), "The system shall support a half-day leave request."),
    ("annual-leave balance", Some("object_type"), "The employee's annual-leave balance shall never be negative."),
    ("leave request", Some("object_type"), "A leave request shall identify the employee, leave type, start date, and end date."),
    ("leave balance", Some("object_type"), "Rejecting a leave request shall not change the employee's leave balance."),
    ("leave type", Some("value_type"), "A leave request shall identify the employee, leave type, start date, and end date."),
    ("leave start date", Some("value_type"), "An employee shall be allowed to cancel an Approved leave request before the leave start date."),
    ("start date", Some("value_type"), "A leave request shall identify the employee, leave type, start date, and end date."),
    ("end date", Some("value_type"), "A leave request shall identify the employee, leave type, start date, and end date."),
    ("public holidays", Some("other"), "The system shall exclude public holidays from requested leave duration."),
    ("working days", Some("value_type"), "The system shall calculate the requested leave duration in working days from the start date to the end date."),
    ("audit record", Some("object_type"), "The system shall create an audit record for every approval, rejection, withdrawal, and cancellation decision."),
    ("rejection reason", Some("value_type"), "A manager shall be allowed to reject leave requests for employees who report directly to that manager and shall record a rejection reason."),
    ("decision timestamp", Some("value_type"), "An approval or rejection shall record the deciding manager and decision timestamp."),
    ("approval records", Some("object_type"), "Approval records shall be retained for five years."),
    ("employee", Some("object_type"), "An employee shall be able to view that employee's own leave requests."),
    ("manager", Some("role"), "A manager shall be allowed to approve leave requests for employees who report directly to that manager."),
    ("contractor", Some("role"), "A contractor shall not be eligible for paid annual leave."),
];

/// Non-overlapping case-insensitive whole-word occurrences of the vocabulary in `statement`.
fn vocabulary_mentions(statement: &str) -> Vec<(usize, usize, usize)> {
    let lower = statement.to_lowercase();
    let mut taken: Vec<(usize, usize, usize)> = Vec::new();
    for (index, (phrase, _, _)) in VOCABULARY.iter().enumerate() {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(phrase) {
            let start = from + pos;
            let mut end = start + phrase.len();
            from = end;
            let before_ok = start == 0 || !lower.as_bytes()[start - 1].is_ascii_alphanumeric();
            // Allow a plural "s" after the phrase.
            if lower.as_bytes().get(end) == Some(&b's') {
                end += 1;
            }
            let after_ok = lower
                .as_bytes()
                .get(end)
                .is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'-');
            let overlaps = taken.iter().any(|(s, e, _)| start < *e && *s < end);
            if before_ok && after_ok && !overlaps {
                taken.push((start, end, index));
            }
        }
    }
    taken.sort();
    taken
}

/// The first exact grounding of `phrase` (case-insensitive; a space may also be a hyphen) in
/// the statements of `requirements`, preferring the statement containing `prefer`.
fn ground(requirements: &[Value], phrase: &str, prefer: Option<&str>) -> Option<Value> {
    let variants = [phrase.to_owned(), phrase.replacen(' ', "-", 1)];
    let mut ordered: Vec<&Value> = requirements.iter().collect();
    if let Some(prefer) = prefer {
        ordered.sort_by_key(|r| !r["statement"].as_str().unwrap().contains(prefer));
    }
    for r in ordered {
        let statement = r["statement"].as_str().unwrap();
        let lower = statement.to_lowercase();
        for v in &variants {
            if let Some(start) = lower.find(v.as_str()) {
                return Some(
                    json!({"requirement_ref": r["requirement_ref"], "start": start,
                                   "end": start + v.len()}),
                );
            }
        }
    }
    None
}

/// Entities from every object_type Concept and the HR-002 attributes of a leave request.
fn domain_output(context: &Value) -> Value {
    let requirements = context["requirements"].as_array().unwrap().clone();
    let concepts = context["concepts"].as_array().unwrap();
    let concept = |name: &str| {
        concepts
            .iter()
            .find(|c| c["name"] == name)
            .map(|c| c["concept_ref"].clone())
    };
    let entities: Vec<Value> = concepts
        .iter()
        .filter(|c| c["concept_kind"] == "object_type")
        .filter_map(|c| {
            let grounding = ground(&requirements, c["name"].as_str().unwrap(), None)?;
            Some(json!({"concept_ref": c["concept_ref"], "grounding": grounding}))
        })
        .collect();
    let hr002 = "shall identify the employee, leave type, start date, and end date";
    let mut attributes = Vec::new();
    if let Some(owner) = concept("leave request") {
        // (Concept name, statement anchor, nullable wording): HR-002 fields and HR-007 half day.
        let hr007 = "support a half-day leave request";
        for (name, anchor, nullable) in [
            ("leave type", hr002, "shall identify"),
            ("start date", hr002, "shall identify"),
            ("end date", hr002, "shall identify"),
            ("half day", hr007, "shall support"),
        ] {
            let surface = if name == "half day" { "half-day" } else { name };
            let (Some(value), Some(g), Some(og), Some(ng)) = (
                concept(name),
                ground(&requirements, surface, Some(anchor)),
                ground(&requirements, "leave request", Some(anchor)),
                ground(&requirements, nullable, Some(anchor)),
            ) else {
                continue;
            };
            attributes.push(json!({
                "concept_ref": value, "grounding": g,
                "owner_concept_ref": owner, "owner_grounding": og,
                "value_type": {"concept_ref": value, "grounding": g},
                "nullable": {"value": false, "grounding": ng},
            }));
        }
    }
    json!({"version": 1, "entities": entities, "attributes": attributes, "relationships": []})
}

/// The exact case-sensitive grounding of `needle` in the statement containing `within`.
fn at(requirements: &[Value], within: &str, needle: &str) -> Option<Value> {
    let r = requirements
        .iter()
        .find(|r| r["statement"].as_str().unwrap().contains(within))?;
    let start = r["statement"].as_str().unwrap().find(needle)?;
    Some(
        json!({"requirement_ref": r["requirement_ref"], "start": start, "end": start + needle.len()}),
    )
}

/// The six LeaveRequest states and five transitions of HR-009/010/017/018/019/021.
fn lifecycle_output(context: &Value) -> Value {
    let requirements = context["requirements"].as_array().unwrap().clone();
    let Some(owner) = context["owners"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == "leave request")
        .map(|o| o["stateful_ref"].clone())
    else {
        return json!({"version": 1, "states": [], "transitions": [], "invariants": []});
    };
    let r = |within: &str, needle: &str| at(&requirements, within, needle).unwrap();
    let hr009 = "from Draft to Submitted";
    let hr010 = "before it can become Approved or Rejected";
    let hr017 = "withdraw a Submitted leave request";
    let hr018 = "change its status to Withdrawn";
    let hr019 = "cancel an Approved leave request";
    let hr021 = "change its status to Cancelled";
    let states = [
        r(hr009, "Draft"),
        r(hr009, "Submitted"),
        r(hr010, "Approved"),
        r(hr010, "Rejected"),
        r(hr018, "Withdrawn"),
        r(hr021, "Cancelled"),
    ]
    .into_iter()
    .map(|g| json!({"stateful_ref": owner, "grounding": g}))
    .collect::<Vec<_>>();
    // Triggers are the Operations of the flows stage once they exist (else null).
    let trigger = |name: &str| {
        context["triggers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .map_or(Value::Null, |t| t["trigger_ref"].clone())
    };
    let transition = |g: Value, from: Value, to: Value, op: &str| {
        json!({"stateful_ref": owner, "grounding": g, "from_state": from, "to_state": to,
               "trigger_ref": trigger(op)})
    };
    let transitions = vec![
        transition(
            r(hr009, "change its status"),
            r(hr009, "Draft"),
            r(hr009, "Submitted"),
            "SubmitLeaveRequest",
        ),
        transition(
            r(hr010, "become"),
            r(hr009, "Submitted"),
            r(hr010, "Approved"),
            "ApproveLeaveRequest",
        ),
        transition(
            r(hr010, "become"),
            r(hr009, "Submitted"),
            r(hr010, "Rejected"),
            "RejectLeaveRequest",
        ),
        transition(
            r(hr018, "change its status"),
            r(hr017, "Submitted"),
            r(hr018, "Withdrawn"),
            // No trigger on purpose: the expert supplies it through the question path.
            "-",
        ),
        transition(
            r(hr021, "change its status"),
            r(hr019, "Approved"),
            r(hr021, "Cancelled"),
            "CancelLeaveRequest",
        ),
    ];
    json!({"version": 1, "states": states, "transitions": transitions, "invariants": []})
}

/// The node_ref of the context `section` item whose payload name is `name`.
fn named(context: &Value, section: &str, name: &str) -> Option<Value> {
    context[section]
        .as_array()?
        .iter()
        .find(|i| i["payload"]["data"]["name"] == name)
        .map(|i| i["node_ref"].clone())
}

/// The six LeaveRequest commands of HR-001/009/011/012/017/019 with their outcomes and events.
fn operation_output(context: &Value) -> Value {
    let requirements = context["requirements"].as_array().unwrap().clone();
    let r = |within: &str, needle: &str| at(&requirements, within, needle).unwrap();
    let Some(leave_request) = named(context, "domain", "leave request") else {
        return json!({"version": 1, "operations": []});
    };
    let grounded =
        |target: &Value, evidence: Value| json!({"target_ref": target, "evidence": [evidence]});
    let employee = named(context, "performers", "Employee").unwrap();
    let manager = named(context, "performers", "Manager").unwrap();
    let authorization = named(context, "governors", "ManagerDecisionAuthorization").unwrap();
    // (name, statement anchor, verb, performer, performer mention, event, extra business
    //  failure); an event or failure is (name, statement anchor, evidence text).
    type Grounded<'a> = Option<(&'a str, &'a str, &'a str)>;
    let specs: [(&str, &str, &str, &Value, &str, Grounded, Grounded); 6] = [
        (
            "CreateLeaveRequest",
            "create a draft leave request",
            "create a draft",
            &employee,
            "employee",
            None,
            None,
        ),
        (
            "SubmitLeaveRequest",
            "from Draft to Submitted",
            "Submitting",
            &employee,
            "",
            Some(("LeaveSubmitted", "from Draft to Submitted", "Submitted")),
            Some((
                "InsufficientBalance",
                "insufficient annual-leave balance",
                "prevent submission",
            )),
        ),
        (
            "ApproveLeaveRequest",
            "allowed to approve leave requests",
            "approve",
            &manager,
            "manager",
            Some((
                "LeaveApproved",
                "when a leave request is Approved",
                "notify",
            )),
            None,
        ),
        (
            "RejectLeaveRequest",
            "allowed to reject leave requests",
            "reject",
            &manager,
            "manager",
            Some((
                "LeaveRejected",
                "when a leave request is Rejected",
                "notify",
            )),
            Some((
                "ReasonRequired",
                "allowed to reject leave requests",
                "record a rejection reason",
            )),
        ),
        (
            "WithdrawLeaveRequest",
            "withdraw a Submitted leave request",
            "withdraw",
            &employee,
            "employee",
            Some((
                "LeaveWithdrawn",
                "change its status to Withdrawn",
                "Withdrawn",
            )),
            None,
        ),
        (
            "CancelLeaveRequest",
            "cancel an Approved leave request",
            "cancel",
            &employee,
            "employee",
            Some((
                "LeaveCancelled",
                "change its status to Cancelled",
                "Cancelled",
            )),
            Some((
                "LeaveAlreadyStarted",
                "cancel an Approved leave request",
                "before the leave start date",
            )),
        ),
    ];
    let operations: Vec<Value> = specs
        .iter()
        .map(|(name, anchor, verb, performer, mention, event, failure)| {
            // HR-009 names no actor; HR-008 grounds the employee as the submitter.
            let performers = if mention.is_empty() {
                vec![grounded(performer, r("prevent submission when the employee", "employee"))]
            } else {
                vec![grounded(performer, r(anchor, mention))]
            };
            let mut outcomes = vec![json!({"name": "Succeeded", "outcome_kind": "success",
                                           "evidence": [r(anchor, verb)]})];
            if let Some((failure, within, needle)) = failure {
                outcomes.push(json!({"name": failure, "outcome_kind": "business_failure",
                                     "evidence": [r(within, needle)]}));
            }
            let events: Vec<Value> = event
                .iter()
                .map(|(event, within, needle)| {
                    json!({"name": event, "payload_schema_ref": null, "evidence": [r(within, needle)]})
                })
                .collect();
            let governed_by = if *name == "ApproveLeaveRequest" || *name == "RejectLeaveRequest" {
                vec![grounded(&authorization, r(anchor, "report directly to that manager"))]
            } else {
                vec![]
            };
            json!({
                "name": name, "operation_kind": "command", "evidence": [r(anchor, verb)],
                "performers": performers,
                "reads": [], "writes": [grounded(&leave_request, r(anchor, "leave request"))],
                "governed_by": governed_by, "uses_calculation": [],
                "input_schema": null, "output_schema": null, "outcomes": outcomes,
                "new_produced_events": events, "produced_event_refs": [], "consumed_event_refs": [],
            })
        })
        .collect();
    json!({"version": 1, "operations": operations})
}

/// RequestedWorkingDays of HR-004/005/006, inclusive end left unresolved for the expert.
fn calculation_output(context: &Value) -> Value {
    let requirements = context["requirements"].as_array().unwrap().clone();
    let r = |within: &str, needle: &str| at(&requirements, within, needle).unwrap();
    let hr004 = "calculate the requested leave duration in working days";
    let calendar = context["calendars"]
        .as_array()
        .and_then(|c| c.first())
        .map_or(Value::Null, |c| c["calendar_ref"].clone());
    json!({"version": 1, "calculations": [{
        "name_range": r(hr004, "requested leave duration"),
        // HR-004/007 leave the inclusive end and the half-day fraction open: the expert decides.
        "expression": "working_days([start_date, end_date], calendar, inclusive_end) * day_fraction",
        "expression_evidence": [
            r(hr004, "in working days from the start date to the end date"),
            r("exclude Saturdays and Sundays", "exclude Saturdays and Sundays"),
            r("exclude public holidays", "exclude public holidays"),
            r("support a half-day leave request", "half-day leave request"),
        ],
        "result_type": "Decimal(2)", "unit": "working_day", "rounding": null,
        "calendar_ref": calendar,
    }]})
}

impl InferenceProvider for Scripted {
    fn execute(
        &self,
        request: &InferenceRequest,
        material: &InferenceRequestMaterial,
    ) -> Result<ProviderExecution, ProviderError> {
        validate_binding(request, material)?;
        let context: Value = serde_json::from_slice(&material.context_json).unwrap();
        let ctx_dir = self.dir.join("contexts");
        std::fs::create_dir_all(&ctx_dir).unwrap();
        std::fs::write(
            ctx_dir.join(format!("{}.json", request.task_kind)),
            serde_json::to_vec_pretty(&context).unwrap(),
        )
        .unwrap();
        let output = match request.task_kind.as_str() {
            "requirement_segmentation" => {
                let segments: Vec<Value> = context["fragments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| {
                        let text = f["text"].as_str().unwrap();
                        let class = if hr_kind(text).is_some() {
                            "requirement_candidate"
                        } else {
                            "non_requirement"
                        };
                        json!({"fragment_ref": f["fragment_ref"], "start": 0,
                               "end": text.len(), "classification": class})
                    })
                    .collect();
                json!({"version": 1, "segments": segments})
            }
            "requirement_classification" => {
                let classes: Vec<Value> = context["candidates"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| {
                        let text = c["text"].as_str().unwrap_or("");
                        json!({"candidate_ref": c["candidate_ref"],
                               "requirement_kind": hr_kind(text), "level": "system"})
                    })
                    .collect();
                json!({"version": 1, "classifications": classes, "intents": []})
            }
            "vocabulary_analysis" => {
                let requirements = context["requirements"].as_array().unwrap();
                // Like a model following the prompt: HR requirements define nothing, so every
                // definition is null and Concepts are grounded in their using statements.
                let mentions: Vec<Value> = requirements
                    .iter()
                    .flat_map(|r| {
                        let statement = r["statement"].as_str().unwrap();
                        vocabulary_mentions(statement)
                            .into_iter()
                            .map(|(start, end, index)| {
                                let (_, kind, _) = VOCABULARY[index];
                                json!({"requirement_ref": r["requirement_ref"], "start": start,
                                       "end": end, "concept_kind": kind,
                                       "definition": null})
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                json!({"version": 1, "mentions": mentions})
            }
            "ears_normalization" => {
                let requirements: Vec<Value> = context["requirements"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| json!({"requirement_ref": r["requirement_ref"], "suggestion": null}))
                    .collect();
                json!({"version": 1, "requirements": requirements})
            }
            "domain_extraction" => domain_output(&context),
            "lifecycle_extraction" => lifecycle_output(&context),
            "operation_analysis" => operation_output(&context),
            "process_analysis" => json!({"version": 1, "processes": []}),
            "calculation_analysis" => calculation_output(&context),
            other => {
                eprintln!("SCRIPTED: no script for task kind {other}");
                return Err(ProviderError::ProviderDisabled);
            }
        };
        let executed = execution(request, output);
        self.calls.lock().unwrap().push(executed.clone());
        Ok(executed)
    }
}

// ---------------------------------------------------------------------- driver

struct Sim {
    dir: PathBuf,
    db: PathBuf,
    config_path: PathBuf,
    config: std::cell::RefCell<Value>,
    _tmp: Option<tempfile::TempDir>,
}

impl Sim {
    fn new() -> Sim {
        let (dir, tmp) = match std::env::var("SIM_DIR") {
            Ok(d) => {
                let d = PathBuf::from(d);
                let _ = std::fs::remove_dir_all(&d);
                std::fs::create_dir_all(&d).unwrap();
                (d, None)
            }
            Err(_) => {
                let t = tempfile::tempdir().unwrap();
                (t.path().to_owned(), Some(t))
            }
        };
        let config = config();
        let config_path = dir.join("plumb.json");
        std::fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        Sim {
            db: dir.join("plumb.sqlite"),
            dir,
            config_path,
            config: std::cell::RefCell::new(config),
            _tmp: tmp,
        }
    }

    /// Runs the CLI; returns (exit code, stdout+stderr) and prints both.
    fn cli(&self, args: &[&str], extra: &[&str]) -> (i32, String) {
        let out = Command::cargo_bin("plumb-cli")
            .unwrap()
            .arg("--db")
            .arg(&self.db)
            .arg("--config")
            .arg(&self.config_path)
            .args(extra)
            .args(args)
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let code = out.status.code().unwrap();
        println!(
            "$ plumb {} {}\n{}[exit {code}]\n",
            extra.join(" "),
            args.join(" "),
            text
        );
        (code, text)
    }

    fn json(&self, args: &[&str]) -> Value {
        let out = Command::cargo_bin("plumb-cli")
            .unwrap()
            .arg("--db")
            .arg(&self.db)
            .arg("--config")
            .arg(&self.config_path)
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
            panic!(
                "{args:?}: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
    }

    /// Runs `recipe`: records the scripted executions over a copy of the database, then
    /// replays them through the CLI over the real database.
    fn stage(&self, recipe: &str) -> (i32, String) {
        let copy = self.dir.join("recording.sqlite");
        std::fs::copy(&self.db, &copy).unwrap();
        let scripted = Arc::new(Scripted {
            dir: self.dir.clone(),
            calls: Mutex::new(Vec::new()),
        });
        let state = open_application(
            &copy,
            BranchName::main(),
            ApiIdentity::new(
                "alice".into(),
                id(ACTOR),
                vec!["analyst".into()],
                "pilot".into(),
            )
            .unwrap(),
            Arc::new(FixedClock::new(T0.parse::<Timestamp>().unwrap())),
            load_builtin_software_profile().unwrap(),
            runtime(&self.config.borrow(), scripted.clone()),
        )
        .unwrap();
        match state.run_stage(Recipe::parse(recipe).unwrap(), None, None) {
            Ok(run) => {
                if let Err(e) = state.execute_recipe_job(&run.job_id) {
                    println!("(recording {recipe}: {e:?})");
                }
            }
            Err(e) => println!("(recording {recipe} not started: {e:?})"),
        }
        let calls = scripted.calls.lock().unwrap().clone();
        drop(state);
        let _ = std::fs::remove_file(&copy);
        if calls.is_empty() {
            return self.cli(&[recipe], &[]);
        }
        let fixtures = self.dir.join(format!("replay-{recipe}.json"));
        std::fs::write(&fixtures, serde_json::to_vec(&calls).unwrap()).unwrap();
        self.cli(
            &[recipe],
            &[
                "--provider",
                "replay",
                "--replay-fixtures",
                fixtures.to_str().unwrap(),
            ],
        )
    }

    /// Replaces one top-level configuration value (what an analyst edits between runs).
    fn set(&self, key: &str, value: Value) {
        let mut config = self.config.borrow_mut();
        config[key] = value;
        std::fs::write(
            &self.config_path,
            serde_json::to_vec_pretty(&*config).unwrap(),
        )
        .unwrap();
        println!("(config {key} updated)\n");
    }

    /// The IDs of the Accepted nodes of `node_type`.
    fn accepted(&self, node_type: &str) -> Vec<String> {
        self.nodes()
            .into_iter()
            .filter(|n| n["status"] == "Accepted" && n["payload"]["type"] == node_type)
            .map(|n| n["id"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Every node of the head as JSON (read-only inspection).
    fn nodes(&self) -> Vec<Value> {
        let store = plumb_store::SqliteRevisionStore::open(&self.db).unwrap();
        let head = store.head(&BranchName::main()).unwrap().unwrap();
        let loaded = store.load_revision(&head).unwrap();
        loaded
            .graph
            .nodes()
            .values()
            .map(|n| serde_json::to_value(n).unwrap())
            .collect()
    }

    /// Prints node counts by type and status.
    fn census(&self) {
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        for n in self.nodes() {
            let key = format!(
                "{}/{}",
                n["payload"]["type"].as_str().unwrap_or("?"),
                n["status"].as_str().unwrap_or("?")
            );
            *counts.entry(key).or_default() += 1;
        }
        println!("census: {counts:?}\n");
    }

    /// Confirms every Proposed node of the given payload types through the CLI.
    fn confirm_all(&self, types: &[&str]) {
        let ids: Vec<String> = self
            .nodes()
            .into_iter()
            .filter(|n| {
                n["status"] == "Proposed"
                    && (types.is_empty()
                        || types.contains(&n["payload"]["type"].as_str().unwrap_or("")))
            })
            .map(|n| n["id"].as_str().unwrap().to_owned())
            .collect();
        if ids.is_empty() {
            println!("(nothing to confirm for {types:?})\n");
            return;
        }
        let mut args = vec!["confirm"];
        args.extend(ids.iter().map(String::as_str));
        args.extend(["--reason", "Reviewed in simulation."]);
        self.cli(&args, &[]);
    }

    /// Runs `recipe`, accepts and confirms everything it proposes, and reruns it until it
    /// proposes nothing new (what a reviewing analyst does after enrichment).
    fn settle(&self, recipe: &str) {
        for round in 1..=4 {
            println!("===== {recipe} (round {round})");
            let (code, _) = self.stage(recipe);
            let pending = self.json(&["proposals"]);
            let count = pending.as_array().map_or(0, Vec::len);
            if count == 0 {
                self.census();
                return;
            }
            self.accept_all();
            self.confirm_all(&[]);
            self.census();
            if code != 0 {
                return;
            }
        }
        println!("!!!!! {recipe} still proposing after 4 rounds");
    }

    /// Accepts every pending proposal of the head as one set (falling back to one by one).
    fn accept_all(&self) {
        let pending = self.json(&["proposals"]);
        let items = pending
            .as_array()
            .cloned()
            .unwrap_or_else(|| pending["proposals"].as_array().cloned().unwrap_or_default());
        if items.is_empty() {
            println!("(no pending proposals)\n");
            return;
        }
        let members: Vec<String> = items
            .iter()
            .map(|p| {
                format!(
                    "{}={}",
                    p["proposal_id"].as_str().unwrap(),
                    p["action"].as_str().unwrap()
                )
            })
            .collect();
        let mut args = vec!["accept-set"];
        args.extend(members.iter().map(String::as_str));
        args.extend(["--reason", "Reviewed in simulation."]);
        let (code, _) = self.cli(&args, &[]);
        if code != 0 {
            for p in &items {
                self.cli(
                    &[
                        "apply",
                        p["proposal_id"].as_str().unwrap(),
                        "--action",
                        p["action"].as_str().unwrap(),
                        "--reason",
                        "Reviewed in simulation.",
                    ],
                    &[],
                );
            }
        }
    }
}

#[test]
fn hr_pilot_simulation() {
    let sim = Sim::new();
    sim.cli(&["init", "--project", "project:hr"], &[]);
    let md = fixture("requirements.md");
    sim.cli(
        &[
            "import",
            md.to_str().unwrap(),
            "--media-type",
            "text/markdown",
            "--name",
            "requirements.md",
        ],
        &[],
    );
    for recipe in ["extract", "consolidate", "lint"] {
        sim.settle(recipe);
    }
    // The fixture's duplicate threshold (0.8) flags HR-023/HR-024 (notify on Approved vs
    // Rejected) as duplicates: an F1 blocker that is governed through a waiver (H0.6).
    let findings = sim.json(&["findings", "list"]);
    println!("findings: {findings:#}\n");
    let duplicate = findings
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["code"] == "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED")
        .expect("the HR-023/HR-024 duplicate is a current F1 finding")
        .clone();
    assert_eq!(duplicate["disposition"], "waivable", "{duplicate}");
    assert_eq!(duplicate["governed"], false, "{duplicate}");
    let (code, _) = sim.cli(&["findings", "materialize"], &[]);
    assert_eq!(code, 0);
    let (code, _) = sim.cli(
        &[
            "findings",
            "waive",
            duplicate["id"].as_str().unwrap(),
            "--reason",
            "HR-023 and HR-024 notify different outcomes; they are distinct requirements.",
        ],
        &[],
    );
    assert_eq!(code, 0);
    let (code, lint) = sim.cli(&["lint"], &[]);
    assert_eq!(code, 0, "F1 passes once the duplicate is waived: {lint}");
    let after = sim.json(&["findings", "list"]);
    assert!(
        after
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"] == duplicate["id"] && f["governed"] == true),
        "{after}"
    );
    sim.cli(&["ready"], &[]);
    sim.cli(&["directory"], &[]);
    sim.accept_all();
    sim.confirm_all(&[]);
    sim.census();
    sim.settle("analyse");
    let mut domain = sim.accepted("Entity");
    domain.extend(sim.accepted("Attribute"));
    let mut performers = sim.accepted("BusinessRole");
    performers.extend(sim.accepted("Actor"));
    sim.set(
        "operation_scope",
        json!({"performer_refs": performers, "domain_refs": domain, "calculation_refs": [],
               "governor_refs": sim.accepted("Rule"), "event_refs": [], "data_schema_refs": [],
               "event_payload_attribute_refs": [], "support_refs": []}),
    );
    sim.set(
        "process_scope",
        json!({"operation_refs": [], "performer_refs": performers, "event_refs": [],
               "outcome_refs": [], "transition_refs": []}),
    );
    sim.settle("flows");
    let attribute = |name: &str| {
        sim.nodes()
            .into_iter()
            .find(|n| n["payload"]["type"] == "Attribute" && n["payload"]["data"]["name"] == name)
            .map(|n| n["id"].clone())
            .unwrap()
    };
    for (name, value_type) in [
        ("start date", "Date"),
        ("end date", "Date"),
        ("leave type", "String"),
        ("half day", "Decimal(2)"),
    ] {
        let id = attribute(name);
        sim.cli(
            &[
                "set-attribute-type",
                id.as_str().unwrap(),
                "--type",
                value_type,
            ],
            &[],
        );
    }
    sim.set(
        "calculation_scope",
        json!({"bindings": [
                   {"node_ref": attribute("start date"), "symbol": "start_date", "exposure": {"kind": "root"}},
                   {"node_ref": attribute("end date"), "symbol": "end_date", "exposure": {"kind": "root"}},
                   {"node_ref": attribute("half day"), "symbol": "day_fraction", "exposure": {"kind": "root"}}],
               "calendar_refs": sim.accepted("Calendar")}),
    );
    for recipe in ["analyse", "calc", "rules", "scenarios", "tags", "run"] {
        sim.settle(recipe);
    }
    sim.cli(&["ask"], &[]);
    let questions = sim.json(&["questions"]);
    println!("questions: {questions:#}\n");
    sim.cli(
        &[
            "rounds",
            "create",
            "--stakeholder",
            "stakeholder:hr",
            "--max",
            "15",
        ],
        &[],
    );
    // The HR expert answers every routed question (fixtures/hr-leave/expert-answers.yaml plus
    // the missing Withdraw trigger), choosing the answer by the question's answer schema.
    let withdraw = sim
        .nodes()
        .into_iter()
        .find(|n| n["payload"]["data"]["name"] == "WithdrawLeaveRequest")
        .map(|n| n["id"].as_str().unwrap().to_owned())
        .unwrap();
    let calendar = sim.accepted("Calendar")[0].clone();
    for q in questions.as_array().unwrap() {
        let props = &q["answer_schema"]["properties"];
        let (answer, rationale) = if props.get("trigger_ref").is_some() {
            (
                json!({"trigger_ref": withdraw}),
                "HR-017 lets the employee withdraw a Submitted request.",
            )
        } else if props.get("inclusive_end").is_some() {
            (
                json!({"inclusive_end": true}),
                "Both start date and end date are included.",
            )
        } else if props.get("holiday_consumes_working_day").is_some() {
            (
                json!({"calendar_ref": calendar, "holiday_consumes_working_day": false}),
                "A configured public holiday consumes 0 working days.",
            )
        } else if props.get("decimal").is_some() {
            (
                json!({"decimal": "0.5", "unit": "working_day"}),
                "A half day consumes 0.5 working day.",
            )
        } else {
            panic!("unexpected question {q}");
        };
        let answer = answer.to_string();
        sim.cli(
            &[
                "answer",
                q["id"].as_str().unwrap(),
                "--answer",
                &answer,
                "--rationale",
                rationale,
            ],
            &[],
        );
    }
    // All answers were given on one head (as in a real round). Three of them change the same
    // calculation, so accepting one makes the others stale (never rebased). The stale answer
    // proposals are listed as stale and re-submitted from their recorded answers (H0.10).
    let pending = sim.json(&["proposals"]);
    let pending = pending.as_array().unwrap();
    assert_eq!(pending.len(), questions.as_array().unwrap().len());
    assert!(pending.iter().all(|p| p["stale"] == false), "{pending:?}");
    let first = pending[0]["proposal_id"].as_str().unwrap();
    let (code, _) = sim.cli(
        &[
            "apply",
            first,
            "--action",
            "accept",
            "--reason",
            "Expert answer.",
        ],
        &[],
    );
    assert_eq!(code, 0);
    let after = sim.json(&["proposals"]);
    let after = after.as_array().unwrap();
    assert_eq!(after.len(), pending.len() - 1);
    assert!(after.iter().all(|p| p["stale"] == true), "{after:?}");
    for _ in 0..pending.len() {
        let (code, _) = sim.cli(&["proposals", "--refresh"], &[]);
        assert_eq!(code, 0);
        let fresh = sim.json(&["proposals"]);
        let Some(next) = fresh
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["stale"] == false)
        else {
            break;
        };
        let (code, _) = sim.cli(
            &[
                "apply",
                next["proposal_id"].as_str().unwrap(),
                "--action",
                "accept",
                "--reason",
                "Expert answer.",
            ],
            &[],
        );
        assert_eq!(code, 0);
    }
    assert_eq!(
        sim.json(&["proposals"]).as_array().unwrap().len(),
        0,
        "every answer accepted"
    );
    sim.confirm_all(&[]);
    sim.settle("analyse");
    sim.settle("scenarios");
    sim.settle("run");
    sim.cli(&["vocab"], &[]);
    let out = sim.dir.join("report");
    sim.cli(&["report", "--out", out.to_str().unwrap()], &[]);
    let (code, functional) = sim.cli(&["export", "--format", "functional"], &[]);
    assert_eq!(code, 0, "{functional}");
    let (code, gates) = sim.cli(&["ready"], &[]);
    assert_eq!(
        code, 0,
        "every NOW gate passes at the end of the pilot: {gates}"
    );
    hr_oracle(&sim);
    hr_reference_outputs(&out);
}

/// E0.1: the deterministic pilot outputs are compared with the committed reference copies in
/// fixtures/hr-leave/derived (decisions.jsonl carries wall-clock decision times and is not
/// compared). Set PLUMB_REGENERATE_REFERENCE=1 to rewrite the references deliberately.
fn hr_reference_outputs(report: &Path) {
    let derived = fixture("derived");
    let regenerate = std::env::var("PLUMB_REGENERATE_REFERENCE").is_ok();
    if regenerate {
        std::fs::create_dir_all(&derived).unwrap();
    }
    for name in [
        "functional.yaml",
        "glossary.md",
        "scenarios.yaml",
        "model-report.md",
        "gate-report.json",
    ] {
        let produced = std::fs::read_to_string(report.join(name)).unwrap();
        if regenerate {
            std::fs::write(derived.join(name), &produced).unwrap();
            continue;
        }
        let reference = std::fs::read_to_string(derived.join(name))
            .unwrap_or_else(|_| panic!("missing reference fixtures/hr-leave/derived/{name}"));
        assert_eq!(produced, reference, "{name} differs from its reference");
    }
}

/// Normalizes a name for comparison with the fixture contract ("LeaveRequest", "leave request"
/// and "Leave Request" are the same name).
fn norm(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// E0.2 (HR semantic oracle): the production pipeline output is asserted against every section
/// of fixtures/hr-leave/fixture-contract.yaml the pipeline produces in this simulation; the
/// sections the product does not (yet) produce are printed as coverage, never hidden.
fn hr_oracle(sim: &Sim) {
    let contract: Value =
        serde_yaml::from_str(&std::fs::read_to_string(fixture("fixture-contract.yaml")).unwrap())
            .unwrap();
    let nodes = sim.nodes();
    let accepted = |t: &str| -> Vec<&Value> {
        nodes
            .iter()
            .filter(|n| n["status"] == "Accepted" && n["payload"]["type"] == t)
            .collect()
    };
    let names = |t: &str| -> std::collections::BTreeSet<String> {
        accepted(t)
            .iter()
            .filter_map(|n| n["payload"]["data"]["name"].as_str().map(norm))
            .collect()
    };
    let contract_names = |key: &str| -> std::collections::BTreeSet<String> {
        contract[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| norm(v.as_str().unwrap()))
            .collect()
    };
    let mut coverage = Vec::new();

    // Requirements: all 32, exact statements and kinds.
    let requirements = accepted("Requirement");
    assert_eq!(requirements.len(), 32);
    for r in contract["requirements"].as_array().unwrap() {
        let found = requirements
            .iter()
            .find(|n| n["payload"]["data"]["source_identifier"] == r["id"])
            .unwrap_or_else(|| panic!("{} missing", r["id"]));
        assert_eq!(
            found["payload"]["data"]["statement"], r["statement"],
            "{}",
            r["id"]
        );
        assert_eq!(
            found["payload"]["data"]["requirement_kind"], r["kind"],
            "{}",
            r["id"]
        );
    }
    coverage.push(("requirements", 32, 32));

    // States: exactly the contract's six lifecycle states.
    assert_eq!(names("State"), contract_names("states"));
    coverage.push(("states", 6, 6));

    // Operations and events: everything produced is in the contract. Entities: reported.
    for (section, node_type) in [
        ("operations", "Operation"),
        ("events", "Event"),
        ("entities", "Entity"),
    ] {
        let produced = names(node_type);
        let expected = contract_names(section);
        let unexpected: Vec<_> = produced.difference(&expected).collect();
        if section != "entities" {
            assert!(
                unexpected.is_empty(),
                "{section} not in the contract: {unexpected:?}"
            );
        }
        coverage.push((
            section,
            produced.intersection(&expected).count(),
            expected.len(),
        ));
    }

    // Rules: the three contract rules.
    let expected_rules: std::collections::BTreeSet<String> = contract["rules"]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| norm(k))
        .collect();
    assert_eq!(names("Rule"), expected_rules);
    coverage.push(("rules", 3, 3));

    // Ambiguities: the expert answers took effect through the question path.
    let calculations = accepted("Calculation");
    let calc = calculations[0];
    let expression = calc["payload"]["data"]["expression"].as_str().unwrap();
    assert!(
        expression.contains(", true)"),
        "inclusive end answered: {expression}"
    );
    assert_eq!(
        calc["payload"]["data"]["examples"][0]["expected_result"]["value"],
        contract["accepted_post_answer_semantics"]["half_day_fraction"],
        "half day answered"
    );
    let holiday = nodes.iter().any(|n| {
        n["payload"]["type"] == "ResolutionDecision"
            && n["payload"]["data"]["answer"]["kind"] == "calculation_holiday_policy"
            && n["payload"]["data"]["answer"]["holiday_consumes_working_day"] == false
    });
    assert!(holiday, "holiday policy answered");
    coverage.push(("ambiguities", 3, 3));
    coverage.push(("calculations", calculations.len(), 2));
    coverage.push(("processes", names("Process").len(), 6));
    coverage.push(("invariants", accepted("Invariant").len(), 5));
    coverage.push(("scenarios", accepted("Scenario").len(), 44));

    println!("HR oracle coverage (produced and in contract / contract):");
    for (section, produced, expected) in coverage {
        println!("  {section:<13} {produced:>3} / {expected}");
    }
}

/// H0.10 (rejection memory): a stage proposal the analyst rejected is not proposed again when
/// the stage reruns on a later revision with the same input.
#[test]
fn rejected_stage_proposal_is_not_reproposed() {
    let sim = Sim::new();
    sim.cli(&["init", "--project", "project:hr"], &[]);
    let md = fixture("requirements.md");
    sim.cli(
        &[
            "import",
            md.to_str().unwrap(),
            "--media-type",
            "text/markdown",
            "--name",
            "requirements.md",
        ],
        &[],
    );
    sim.stage("extract");
    let pending = sim.json(&["proposals"]);
    let pending = pending.as_array().unwrap();
    let rejected = pending[0]["proposal_id"].as_str().unwrap().to_owned();
    let (code, _) = sim.cli(
        &[
            "apply",
            &rejected,
            "--action",
            "reject",
            "--reason",
            "Out of scope.",
        ],
        &[],
    );
    assert_eq!(code, 0);
    sim.accept_all();
    sim.confirm_all(&[]);
    let accepted = sim.accepted("Requirement").len();
    assert_eq!(accepted, pending.len() - 1);
    // The head moved (accept + confirm); the rerun sees the same source.
    sim.stage("extract");
    let again = sim.json(&["proposals"]);
    assert_eq!(again.as_array().unwrap().len(), 0, "re-proposed: {again}");
    assert_eq!(sim.accepted("Requirement").len(), accepted);
}

/// H0.10 (rejection memory): rejecting a proposal that is already stale is clean-up, not a
/// content decision, so a rerun on the current revision proposes it again.
#[test]
fn rejecting_a_stale_proposal_is_not_remembered() {
    let sim = Sim::new();
    sim.cli(&["init", "--project", "project:hr"], &[]);
    let md = fixture("requirements.md");
    let import = |path: &str, name: &str| {
        sim.cli(
            &[
                "import",
                path,
                "--media-type",
                "text/markdown",
                "--name",
                name,
            ],
            &[],
        )
    };
    import(md.to_str().unwrap(), "requirements.md");
    sim.stage("extract");
    let first = sim.json(&["proposals"]);
    let target = first[0]["proposal_id"].as_str().unwrap().to_owned();
    // Another source moves the head: every pending extract proposal is now stale.
    let extra = sim.dir.join("extra.md");
    std::fs::write(
        &extra,
        "# Extra\n\n1. **HR-900** [functional] The system shall export reports.\n",
    )
    .unwrap();
    import(extra.to_str().unwrap(), "extra.md");
    let stale = sim.json(&["proposals"]);
    assert!(
        stale.as_array().unwrap().iter().all(|p| p["stale"] == true),
        "{stale}"
    );
    let (code, _) = sim.cli(
        &["apply", &target, "--action", "reject", "--reason", "Stale."],
        &[],
    );
    assert_eq!(code, 0);
    sim.stage("extract");
    let fresh = sim.json(&["proposals"]);
    let fresh_count = fresh
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["stale"] == false)
        .count();
    // All 32 fixture requirements (the stale-rejected one included) plus HR-900.
    assert_eq!(fresh_count, first.as_array().unwrap().len() + 1, "{fresh}");
}

/// H0.5 (analyzer issues): a candidate a stage cannot turn into a proposal is reported with
/// its reason in the stage result, never silently dropped.
#[test]
fn dropped_candidates_are_reported() {
    let sim = Sim::new();
    sim.cli(&["init", "--project", "project:hr"], &[]);
    let source = sim.dir.join("source.md");
    std::fs::write(
        &source,
        "# Requirements\n\n1. **X-001** [functional] The system shall store requests.\n\
         2. **X-002** [functional] The system might log every request.\n",
    )
    .unwrap();
    sim.cli(
        &[
            "import",
            source.to_str().unwrap(),
            "--media-type",
            "text/markdown",
            "--name",
            "source.md",
        ],
        &[],
    );
    let (code, out) = sim.stage("extract");
    assert_eq!(code, 0, "{out}");
    assert_eq!(sim.json(&["proposals"]).as_array().unwrap().len(), 1);
    assert!(
        out.contains("missing_modality"),
        "the dropped X-002 is reported: {out}"
    );
}
