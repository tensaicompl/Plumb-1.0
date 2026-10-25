//! R0.7 customer-zero horizontal regression flow: a second, independent pilot (an equipment loan
//! desk, fixtures/customer-zero) driven only through the `plumb-cli` product surface, from an
//! empty directory to an exported model, replaying committed inference executions.
//!
//! `customer_zero_horizontal_flow` replays fixtures/customer-zero/replay (network-free, no model
//! in the loop) twice and asserts the R0.7 proofs and the frozen expected results in
//! fixtures/customer-zero/expected.yaml. `regenerate_customer_zero_replay` (ignored) re-records
//! the replay files from the scripted model below; it is the only writer of those files.

mod support;

use std::collections::BTreeSet;
use std::path::PathBuf;

use plumb_store::{BranchName, SqliteRevisionStore};
use serde_json::{json, Value};
use support::*;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/customer-zero")
        .join(name)
}

fn config() -> Value {
    serde_json::from_slice(&std::fs::read(fixture("config.json")).unwrap()).unwrap()
}

// ---------------------------------------------------------------------- scripted model

/// The bracketed requirement kind of a source line, if it is tagged.
fn tag_kind(text: &str) -> Option<&'static str> {
    [
        "functional",
        "quality",
        "security",
        "constraint",
        "operational",
    ]
    .into_iter()
    .find(|kind| text.contains(&format!("[{kind}]")))
}

/// The loan-desk vocabulary and its Concept kinds (longest phrases first).
const VOCABULARY: [(&str, &str); 11] = [
    ("public holidays", "other"),
    ("equipment item", "object_type"),
    ("working days", "value_type"),
    ("pickup date", "value_type"),
    ("return date", "value_type"),
    ("loan length", "value_type"),
    ("reservation", "object_type"),
    ("technician", "role"),
    ("half-day", "value_type"),
    ("student", "object_type"),
    ("visitor", "role"),
];

/// Non-overlapping case-insensitive whole-word occurrences of the vocabulary.
fn mentions(statement: &str) -> Vec<(usize, usize, &'static str)> {
    let lower = statement.to_lowercase();
    let mut taken: Vec<(usize, usize, &'static str)> = Vec::new();
    for (phrase, kind) in VOCABULARY {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(phrase) {
            let start = from + pos;
            let mut end = start + phrase.len();
            from = end;
            let before = start == 0 || !lower.as_bytes()[start - 1].is_ascii_alphanumeric();
            if lower.as_bytes().get(end) == Some(&b's') {
                end += 1;
            }
            let after = lower
                .as_bytes()
                .get(end)
                .is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'-');
            if before && after && !taken.iter().any(|(s, e, _)| start < *e && *s < end) {
                taken.push((start, end, kind));
            }
        }
    }
    taken.sort();
    taken
}

fn requirements(context: &Value) -> Vec<Value> {
    context["requirements"].as_array().unwrap().clone()
}

fn domain(context: &Value) -> Value {
    let reqs = requirements(context);
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
            let g = ground(&reqs, c["name"].as_str().unwrap(), None)?;
            Some(json!({"concept_ref": c["concept_ref"], "grounding": g}))
        })
        .collect();
    let cz02 = "shall identify the student, the equipment item";
    let cz07 = "support a half-day loan";
    let mut attributes = Vec::new();
    if let Some(owner) = concept("reservation") {
        for (name, surface, anchor, nullable) in [
            ("pickup date", "pickup date", cz02, "shall identify"),
            ("return date", "return date", cz02, "shall identify"),
            ("half day", "half-day", cz07, "shall support"),
        ] {
            let (Some(value), Some(g), Some(og), Some(ng)) = (
                concept(name),
                ground(&reqs, surface, Some(anchor)),
                ground(&reqs, "reservation", Some(cz02)),
                ground(&reqs, nullable, Some(anchor)),
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

fn lifecycle(context: &Value) -> Value {
    let reqs = requirements(context);
    let Some(owner) = context["owners"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == "reservation")
        .map(|o| o["stateful_ref"].clone())
    else {
        return json!({"version": 1, "states": [], "transitions": [], "invariants": []});
    };
    let r = |within: &str, needle: &str| at(&reqs, within, needle).unwrap();
    let cz08 = "from Requested to Confirmed";
    let cz09 = "hand out a Confirmed reservation";
    let cz10 = "change its status to Loaned";
    let cz11 = "cancel a Requested reservation";
    let cz12 = "change its status to Cancelled";
    let cz13 = "reservation status to Returned";
    let states: Vec<Value> = [
        r(cz08, "Requested"),
        r(cz08, "Confirmed"),
        r(cz10, "Loaned"),
        r(cz12, "Cancelled"),
        r(cz13, "Returned"),
    ]
    .into_iter()
    .map(|g| json!({"stateful_ref": owner, "grounding": g}))
    .collect();
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
            r(cz08, "change its status"),
            r(cz08, "Requested"),
            r(cz08, "Confirmed"),
            "ConfirmReservation",
        ),
        transition(
            r(cz10, "change its status"),
            r(cz09, "Confirmed"),
            r(cz10, "Loaned"),
            "HandOutReservation",
        ),
        transition(
            r(cz12, "change its status"),
            r(cz11, "Requested"),
            r(cz12, "Cancelled"),
            "CancelReservation",
        ),
        transition(
            r(cz13, "change the reservation status"),
            r(cz10, "Loaned"),
            r(cz13, "Returned"),
            "ReturnEquipment",
        ),
    ];
    json!({"version": 1, "states": states, "transitions": transitions, "invariants": []})
}

fn operations(context: &Value) -> Value {
    let reqs = requirements(context);
    let r = |within: &str, needle: &str| at(&reqs, within, needle).unwrap();
    let Some(reservation) = named(context, "domain", "reservation") else {
        return json!({"version": 1, "operations": []});
    };
    let grounded = |t: &Value, e: Value| json!({"target_ref": t, "evidence": [e]});
    let student = named(context, "performers", "Student").unwrap();
    let technician = named(context, "performers", "Technician").unwrap();
    let desk = named(context, "performers", "Loan Desk").unwrap();
    let eligibility = named(context, "governors", "BorrowerEligibility").unwrap();
    let cz02 = "A reservation shall identify";
    // (name, anchor, verb, performer, performer evidence, writes evidence, event)
    type Ev<'a> = Option<(&'a str, &'a str, &'a str)>;
    let specs: [(&str, &str, &str, &Value, Value, Value, Ev); 5] = [
        (
            "ReserveEquipment",
            "able to reserve an equipment item",
            "reserve",
            &student,
            r("able to reserve", "student"),
            r(cz02, "reservation"),
            None,
        ),
        (
            "ConfirmReservation",
            "Confirming a valid reservation",
            "Confirming",
            &desk,
            r("The desk shall reject", "desk"),
            r("Confirming a valid reservation", "reservation"),
            Some((
                "ReservationConfirmed",
                "from Requested to Confirmed",
                "Confirmed",
            )),
        ),
        (
            "HandOutReservation",
            "hand out a Confirmed reservation",
            "hand out",
            &technician,
            r("hand out a Confirmed", "technician"),
            r("hand out a Confirmed", "reservation"),
            Some(("ReservationLoaned", "change its status to Loaned", "Loaned")),
        ),
        (
            "CancelReservation",
            "cancel a Requested reservation",
            "cancel",
            &student,
            r("cancel a Requested", "student"),
            r("cancel a Requested", "reservation"),
            Some((
                "ReservationCancelled",
                "change its status to Cancelled",
                "Cancelled",
            )),
        ),
        (
            "ReturnEquipment",
            "Returning an equipment item",
            "Returning",
            &technician,
            r("hand out a Confirmed", "technician"),
            r("reservation status to Returned", "reservation"),
            Some((
                "EquipmentReturned",
                "reservation status to Returned",
                "Returned",
            )),
        ),
    ];
    let operations: Vec<Value> = specs
        .iter()
        .map(|(name, anchor, verb, performer, pe, we, event)| {
            let mut outcomes = vec![json!({"name": "Succeeded", "outcome_kind": "success",
                                           "evidence": [r(anchor, verb)]})];
            let mut governed_by = vec![];
            if *name == "ReserveEquipment" {
                outcomes.push(json!({"name": "BorrowerNotEligible",
                    "outcome_kind": "business_failure",
                    "evidence": [r("visitor shall not be eligible", "not be eligible")]}));
                governed_by.push(grounded(
                    &eligibility,
                    r("visitor shall not be eligible", "eligible"),
                ));
            }
            let events: Vec<Value> = event
                .iter()
                .map(|(e, within, needle)| {
                    json!({"name": e, "payload_schema_ref": null, "evidence": [r(within, needle)]})
                })
                .collect();
            json!({
                "name": name, "operation_kind": "command", "evidence": [r(anchor, verb)],
                "performers": [grounded(performer, pe.clone())],
                "reads": [], "writes": [grounded(&reservation, we.clone())],
                "governed_by": governed_by, "uses_calculation": [],
                "input_schema": null, "output_schema": null, "outcomes": outcomes,
                "new_produced_events": events, "produced_event_refs": [], "consumed_event_refs": [],
            })
        })
        .collect();
    json!({"version": 1, "operations": operations})
}

fn calculation(context: &Value) -> Value {
    let reqs = requirements(context);
    let r = |within: &str, needle: &str| at(&reqs, within, needle).unwrap();
    let cz05 = "calculate the loan length in working days";
    let calendar = context["calendars"]
        .as_array()
        .and_then(|c| c.first())
        .map_or(Value::Null, |c| c["calendar_ref"].clone());
    json!({"version": 1, "calculations": [{
        "name_range": r(cz05, "loan length"),
        "expression": "working_days([pickup_date, return_date], calendar, inclusive_end) * day_fraction",
        "expression_evidence": [
            r(cz05, "in working days from the pickup date to the return date"),
            r("exclude public holidays", "exclude public holidays"),
            r("support a half-day loan", "half-day loan"),
        ],
        "result_type": "Decimal(2)", "unit": "working_day", "rounding": null,
        "calendar_ref": calendar,
    }]})
}

/// The customer-zero scripted model (introduces no fact absent from source.md or config.json).
fn cz_model(task_kind: &str, context: &Value) -> Option<Value> {
    Some(match task_kind {
        "requirement_segmentation" => {
            let segments: Vec<Value> = context["fragments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| {
                    let text = f["text"].as_str().unwrap();
                    let class = if tag_kind(text).is_some() {
                        "requirement_candidate"
                    } else {
                        "non_requirement"
                    };
                    json!({"fragment_ref": f["fragment_ref"], "start": 0, "end": text.len(),
                           "classification": class})
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
                    json!({"candidate_ref": c["candidate_ref"],
                           "requirement_kind": tag_kind(c["text"].as_str().unwrap_or("")),
                           "level": "system"})
                })
                .collect();
            json!({"version": 1, "classifications": classes, "intents": []})
        }
        "vocabulary_analysis" => {
            let found: Vec<Value> = requirements(context)
                .iter()
                .flat_map(|r| {
                    mentions(r["statement"].as_str().unwrap())
                        .into_iter()
                        .map(|(start, end, kind)| {
                            json!({"requirement_ref": r["requirement_ref"], "start": start,
                                   "end": end, "concept_kind": kind, "definition": null})
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            json!({"version": 1, "mentions": found})
        }
        "ears_normalization" => {
            let none: Vec<Value> = requirements(context)
                .iter()
                .map(|r| json!({"requirement_ref": r["requirement_ref"], "suggestion": null}))
                .collect();
            json!({"version": 1, "requirements": none})
        }
        "domain_extraction" => domain(context),
        "lifecycle_extraction" => lifecycle(context),
        "operation_analysis" => operations(context),
        "process_analysis" => json!({"version": 1, "processes": []}),
        "calculation_analysis" => calculation(context),
        _ => return None,
    })
}

// ---------------------------------------------------------------------- flow

/// The final head of one complete customer-zero run.
struct Outcome {
    sim: Sim,
    revision: String,
    semantic_hash: String,
}

fn head(sim: &Sim) -> (String, String) {
    let ready = sim.json(&["ready"]);
    (
        ready["revision"].as_str().unwrap().to_owned(),
        ready["semantic_hash"].as_str().unwrap().to_owned(),
    )
}

/// One complete flow from an empty directory, through product operations only.
fn run(regenerate: bool) -> Outcome {
    let mut sim = Sim::new(config(), cz_model);
    let replay = fixture("replay");
    if regenerate {
        let _ = std::fs::remove_dir_all(&replay);
    }
    sim.replay = Some((replay, regenerate));
    let (code, _) = sim.cli(&["init", "--project", "project:loan-desk"], &[]);
    assert_eq!(code, 0);
    let source = fixture("source.md");
    let (code, _) = sim.cli(
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
    assert_eq!(code, 0);
    for recipe in ["extract", "consolidate"] {
        sim.settle(recipe);
    }
    // Lint lists its diagnostics with severities (R0.7); CZ-14 'quickly' blocks F1.
    let (code, lint) = sim.cli(&["lint"], &[]);
    assert_eq!(code, 2, "{lint}");
    assert!(
        lint.contains("PLUMB.LINT.REQ.UNMEASURABLE_QUALIFIER") && lint.contains("error"),
        "{lint}"
    );
    // The governed finding/waiver lifecycle (H0.6).
    let findings = sim.json(&["findings", "list"]);
    let quality = findings
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["code"] == "PLUMB.F1.REQ.QUALITY_FINDINGS_CLEAR")
        .expect("the CZ-14 quality finding is current")
        .clone();
    assert_eq!(quality["disposition"], "waivable");
    assert_eq!(sim.cli(&["findings", "materialize"], &[]).0, 0);
    let (code, _) = sim.cli(
        &[
            "findings",
            "waive",
            quality["id"].as_str().unwrap(),
            "--reason",
            "The desk accepts an unquantified screen response goal for the pilot.",
        ],
        &[],
    );
    assert_eq!(code, 0);
    assert_eq!(sim.cli(&["lint"], &[]).0, 0, "F1 passes once waived");
    // The configured directory.
    assert_eq!(sim.cli(&["directory"], &[]).0, 0);
    sim.accept_all();
    sim.confirm_all(&[]);
    sim.settle("analyse");
    // config.json names the generated elements of every stage scope by selector: the
    // configuration is never edited during the flow.
    sim.settle("flows");
    let attribute = |name: &str| {
        sim.nodes()
            .into_iter()
            .find(|n| n["payload"]["type"] == "Attribute" && n["payload"]["data"]["name"] == name)
            .map(|n| n["id"].as_str().unwrap().to_owned())
            .unwrap_or_else(|| panic!("attribute {name}"))
    };
    for (name, value_type) in [
        ("pickup date", "Date"),
        ("return date", "Date"),
        ("half day", "Decimal(2)"),
    ] {
        let (code, out) = sim.cli(
            &["set-attribute-type", &attribute(name), "--type", value_type],
            &[],
        );
        assert_eq!(code, 0, "{out}");
    }
    // Rerun of a semantic stage after downstream enrichment (attribute typing): no collision.
    for recipe in ["analyse", "calc", "rules", "scenarios", "tags", "run"] {
        sim.settle(recipe);
    }
    // Questions raised by the product, one round, the expert answers.
    assert_eq!(sim.cli(&["ask"], &[]).0, 0);
    let questions = sim.json(&["questions"]);
    let (code, round) = sim.cli(
        &[
            "rounds",
            "create",
            "--stakeholder",
            "stakeholder:desk-manager",
            "--max",
            "15",
        ],
        &[],
    );
    assert_eq!(code, 0, "{round}");
    let calendar = sim.accepted("Calendar")[0].clone();
    for q in questions.as_array().unwrap() {
        let props = &q["answer_schema"]["properties"];
        let (answer, why) = if props.get("inclusive_end").is_some() {
            (
                json!({"inclusive_end": false}),
                "The return date is not a loan day.",
            )
        } else if props.get("holiday_consumes_working_day").is_some() {
            (
                json!({"calendar_ref": calendar, "holiday_consumes_working_day": false}),
                "Holidays are never loan days.",
            )
        } else if props.get("decimal").is_some() {
            (
                json!({"decimal": "0.5", "unit": "working_day"}),
                "A half-day loan is half a day.",
            )
        } else {
            panic!("unexpected question {q}");
        };
        let answer = answer.to_string();
        let (code, out) = sim.cli(
            &[
                "answer",
                q["id"].as_str().unwrap(),
                "--answer",
                &answer,
                "--rationale",
                why,
            ],
            &[],
        );
        assert_eq!(code, 0, "{out}");
    }
    // Compatible answers in one round: accept one, refresh the stale siblings, never rebase.
    for _ in 0..questions.as_array().unwrap().len() {
        sim.cli(&["proposals", "--refresh"], &[]);
        let pending = sim.json(&["proposals"]);
        let Some(next) = pending
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["stale"] == false)
        else {
            break;
        };
        let (code, out) = sim.cli(
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
        assert_eq!(code, 0, "{out}");
    }
    assert!(sim.json(&["proposals"]).as_array().unwrap().is_empty());
    for recipe in ["analyse", "scenarios", "run"] {
        sim.settle(recipe);
    }
    let (code, gates) = sim.cli(&["ready"], &[]);
    assert_eq!(code, 0, "{gates}");
    let out = sim.dir.join("report");
    assert_eq!(
        sim.cli(&["report", "--out", out.to_str().unwrap()], &[]).0,
        0
    );
    assert_eq!(sim.cli(&["export", "--format", "functional"], &[]).0, 0);
    // Restart/reopen: read commands on the existing database create no revision.
    let before = head(&sim);
    for args in [
        &["vocab"][..],
        &["proposals"][..],
        &["findings", "list"][..],
    ] {
        assert_eq!(sim.cli(args, &[]).0, 0);
    }
    let after = head(&sim);
    assert_eq!(before, after, "reopening creates no revision");
    let questions_kinds = questions;
    oracle(&sim, &questions_kinds);
    proofs(&sim);
    Outcome {
        revision: after.0,
        semantic_hash: after.1,
        sim,
    }
}

/// R0.7 proofs: every semantic revision after the initial one has its patch artifact.
fn proofs(sim: &Sim) {
    let store = SqliteRevisionStore::open(&sim.db).unwrap();
    let mut current = store.head(&BranchName::main()).unwrap();
    let mut revisions = 0;
    while let Some(id) = current {
        let loaded = store.load_revision(&id).unwrap();
        let revision = serde_json::to_value(&loaded.revision).unwrap();
        if !revision["parent"].is_null() {
            assert!(
                !revision["accepted_patch_ref"].is_null(),
                "revision {id} has no patch artifact"
            );
        }
        current = serde_json::from_value(revision["parent"].clone()).unwrap();
        revisions += 1;
    }
    assert!(revisions > 10);
}

fn norm(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The customer-zero oracle: every section of fixtures/customer-zero/expected.yaml.
fn oracle(sim: &Sim, questions: &Value) {
    let expected: Value =
        serde_yaml::from_str(&std::fs::read_to_string(fixture("expected.yaml")).unwrap()).unwrap();
    let nodes = sim.nodes();
    let accepted = |t: &str| -> Vec<&Value> {
        nodes
            .iter()
            .filter(|n| n["status"] == "Accepted" && n["payload"]["type"] == t)
            .collect()
    };
    let names = |t: &str| -> BTreeSet<String> {
        accepted(t)
            .iter()
            .filter_map(|n| n["payload"]["data"]["name"].as_str().map(norm))
            .collect()
    };
    let list = |key: &str| -> BTreeSet<String> {
        expected[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| norm(v.as_str().unwrap()))
            .collect()
    };
    // Requirements: exact statements and kinds, nothing else.
    let reqs = accepted("Requirement");
    let expected_reqs = expected["requirements"].as_array().unwrap();
    assert_eq!(reqs.len(), expected_reqs.len());
    for r in expected_reqs {
        let found = reqs
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
    for text in expected["not_requirements"].as_array().unwrap() {
        let text = text.as_str().unwrap();
        assert!(
            !reqs.iter().any(|n| n["payload"]["data"]["statement"]
                .as_str()
                .unwrap()
                .contains(text)),
            "{text} became a requirement"
        );
    }
    assert_eq!(names("Entity"), list("entities"));
    let attributes: BTreeSet<String> = expected["attributes"]["reservation"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| norm(v.as_str().unwrap()))
        .collect();
    assert_eq!(names("Attribute"), attributes);
    assert_eq!(names("State"), list("states"));
    assert_eq!(
        accepted("Transition").len(),
        expected["transitions"].as_array().unwrap().len()
    );
    assert_eq!(names("Operation"), list("operations"));
    assert_eq!(names("Event"), list("events"));
    assert_eq!(names("Rule"), list("rules"));
    // The question templates raised.
    let raised: BTreeSet<String> = questions
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["text"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(raised.len(), list("questions").len(), "{raised:?}");
    // The calculation after the expert answers.
    let calc = accepted("Calculation");
    assert_eq!(calc.len(), 1);
    assert_eq!(
        calc[0]["payload"]["data"]["name"],
        expected["calculation"]["name"]
    );
    assert_eq!(
        calc[0]["payload"]["data"]["expression"],
        expected["calculation"]["expression"]
    );
    assert_eq!(
        calc[0]["payload"]["data"]["examples"][0]["expected_result"]["value"],
        expected["answers"]["half_day_fraction"]
    );
    // Waivers.
    let waivers = nodes
        .iter()
        .filter(|n| {
            n["payload"]["type"] == "ResolutionDecision"
                && n["payload"]["data"]["answer"]["kind"] == "waiver"
        })
        .count();
    assert_eq!(waivers, expected["waivers"].as_array().unwrap().len());
    // Scenarios: decision-table rows including the expected negative (Visitor) pass.
    let passing = nodes
        .iter()
        .filter(|n| {
            n["payload"]["type"] == "ScenarioRun" && n["payload"]["data"]["result"] == "pass"
        })
        .count() as u64;
    assert!(passing >= expected["scenarios"]["passing_at_least"].as_u64().unwrap());
}

/// R0.7: the whole flow replays the committed executions (no model, no network) and two runs
/// under different clocks reach the identical final semantic_hash.
#[test]
fn customer_zero_horizontal_flow() {
    let first = run(false);
    let second = run(false);
    assert_eq!(
        first.semantic_hash, second.semantic_hash,
        "deterministic replay"
    );
    assert_eq!(first.revision, second.revision);
    drop(first.sim);
}

/// Re-records fixtures/customer-zero/replay from the scripted model (the only writer).
#[test]
#[ignore]
fn regenerate_customer_zero_replay() {
    run(true);
}
