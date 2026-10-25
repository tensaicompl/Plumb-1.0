//! E0.4 performance release gate, representative-scale workload (driven by
//! scripts/benchmark-representative-scale.sh, which times every `plumb-cli` invocation).
//!
//! `representative_scale_workload` (ignored; it is a benchmark, not a unit test) builds, through
//! the product CLI only, the audit's measured shapes:
//!
//! * a 50,000-paragraph Markdown import. One 50,000-paragraph file is above the importer's
//!   frozen `SOURCE_MAX_FRAGMENTS` limit (H0.1, never unbounded), so it must be rejected
//!   promptly with the typed limit error; the same 50,000 paragraphs are then imported as three
//!   sources within the limit.
//! * a 10,000-node graph: every fifth paragraph is a tagged requirement, so extraction proposes
//!   10,000 Requirements, which are accepted as one set and confirmed.
//! * the read commands (lint, ready, findings, export, report) over that head.
//!
//! The scripted model answers from the source text only. Set `SIM_DIR` to keep the database
//! (the benchmark serves it afterwards for the concurrent version reads). `BENCH_PARAGRAPHS`
//! (default 50,000, the frozen shape) scales the workload down for scaling probes; every fifth
//! paragraph stays a requirement.

mod support;

use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::{json, Value};
use support::*;

const PARAGRAPHS: usize = 50_000;
const REQUIREMENT_EVERY: usize = 5;
/// The most paragraphs of one importable source (it also has one heading fragment; the
/// importer's frozen limit is 20,000 fragments).
const PER_SOURCE: usize = 19_999;

fn paragraphs() -> usize {
    std::env::var("BENCH_PARAGRAPHS")
        .ok()
        .map_or(PARAGRAPHS, |v| v.parse().unwrap())
}

/// Wall time of a step the CLI does not time (the in-process stage recording).
fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = std::time::Instant::now();
    let value = f();
    println!("BENCH {label}: {:.2} s", start.elapsed().as_secs_f64());
    value
}

fn paragraph(i: usize) -> String {
    if i.is_multiple_of(REQUIREMENT_EVERY) {
        format!(
            "REQ-{i:05} The leave service shall record leave request {i} for the requesting \
             employee. [functional]"
        )
    } else {
        format!("Background note {i} describes the context of the leave policy.")
    }
}

fn markdown(title: &str, range: std::ops::Range<usize>) -> String {
    let mut text = format!("# {title}\n\n");
    for i in range {
        writeln!(text, "{}\n", paragraph(i)).unwrap();
    }
    text
}

fn scale_model(task_kind: &str, context: &Value) -> Option<Value> {
    Some(match task_kind {
        "requirement_segmentation" => {
            let segments: Vec<Value> = context["fragments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| {
                    let text = f["text"].as_str().unwrap();
                    let class = if text.contains("[functional]") {
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
                           "requirement_kind": "functional", "level": "system"})
                })
                .collect();
            json!({"version": 1, "classifications": classes, "intents": []})
        }
        "vocabulary_analysis" => json!({"version": 1, "mentions": []}),
        "ears_normalization" => {
            let none: Vec<Value> = context["requirements"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| json!({"requirement_ref": r["requirement_ref"], "suggestion": null}))
                .collect();
            json!({"version": 1, "requirements": none})
        }
        _ => return None,
    })
}

fn config() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/customer-zero/config.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn import(sim: &Sim, name: &str, text: &str) -> (i32, String) {
    import_with(sim, name, text, &[])
}

fn import_with(sim: &Sim, name: &str, text: &str, extra: &[&str]) -> (i32, String) {
    let path = sim.dir.join(name);
    std::fs::write(&path, text).unwrap();
    sim.cli(
        &[
            "import",
            path.to_str().unwrap(),
            "--media-type",
            "text/markdown",
            "--name",
            name,
        ],
        extra,
    )
}

#[test]
#[ignore = "representative-scale benchmark; run by scripts/benchmark-representative-scale.sh"]
fn representative_scale_workload() {
    let sim = Sim::new(config(), scale_model);
    assert_eq!(sim.cli(&["init", "--project", "project:scale"], &[]).0, 0);

    let total = paragraphs();
    // One document of every paragraph: above the limit, a prompt, typed, bounded rejection (H0.1).
    if total > PER_SOURCE {
        let (code, out) = import_with(&sim, "whole.md", &markdown("Whole", 0..total), &["--json"]);
        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("E_INVALID_REQUEST") && out.contains("above the limit 20000"),
            "{out}"
        );
    }

    // The same paragraphs as sources within the limit (19,999 + 19,999 + 10,002 at full scale).
    let mut start = 0;
    let mut n = 0;
    while start < total {
        n += 1;
        let count = PER_SOURCE.min(total - start);
        let name = format!("part-{n}.md");
        let (code, out) = import(&sim, &name, &markdown(&name, start..start + count));
        assert_eq!(code, 0, "{out}");
        start += count;
    }

    // Every fifth paragraph a Requirement: extracted, accepted as one set and confirmed.
    let (code, out) = timed("extract (in-process recording + CLI replay)", || {
        sim.stage("extract")
    });
    assert_eq!(code, 0, "{out}");
    sim.accept_all();
    sim.confirm_all(&["Requirement"]);
    let requirements = sim.accepted("Requirement").len();
    let nodes = sim.nodes().len();
    println!("BENCH graph: {nodes} nodes, {requirements} Accepted Requirements");
    assert_eq!(requirements, total.div_ceil(REQUIREMENT_EVERY));

    // Read commands over that head (their verdicts are not the subject here).
    sim.cli(&["lint"], &[]);
    sim.cli(&["ready"], &[]);
    sim.cli(&["findings", "list"], &[]);
    sim.cli(&["export", "--format", "functional"], &[]);
    let out = sim.dir.join("report");
    sim.cli(&["report", "--out", out.to_str().unwrap()], &[]);
}
