//! S1.4 contract tests for the deterministic lint pack and the benchmark framework.
//!
//! Every corpus here is obviously synthetic (corpus_id `synthetic-test`, annotators `test-a`
//! and `test-b`) and lives in this file; nothing reads fixtures/lint-corpus. Expected counts
//! were derived by hand from the synthetic labels, never from the helpers under test, and no
//! result here is evidence of real-world lint precision.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use plumb_lint::*;
use serde_json::{json, Value};

const HR_PROFILE: &str = include_str!("../../../fixtures/hr-leave/profile.yaml");

fn id(s: &str) -> Id {
    s.parse().unwrap()
}

fn input(statement: &str) -> LintInput {
    LintInput {
        requirement_ref: id("req:0000000000000001"),
        statement: statement.to_owned(),
        evidence_refs: Vec::new(),
        source_anchor: None,
        term_context: None,
    }
}

fn lint(statement: &str) -> LintResult {
    lint_requirement(&input(statement), &LintPolicy::default()).unwrap()
}

/// The matched statement slices of `rule`.
fn hits(statement: &str, rule: LintRuleId) -> Vec<String> {
    lint(statement)
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == rule)
        .map(|d| {
            statement[d.statement_range.start as usize..d.statement_range.end as usize].to_owned()
        })
        .collect()
}

fn messages(statement: &str, rule: LintRuleId) -> Vec<String> {
    lint(statement)
        .diagnostics
        .into_iter()
        .filter(|d| d.rule_id == rule)
        .map(|d| d.message)
        .collect()
}

use LintRuleId::*;

// ---------------------------------------------------------------------------- registry

#[test]
fn rules_registry_snapshot() {
    assert_eq!(LintRuleId::ALL.len(), 15);
    let unique: BTreeSet<&str> = LintRuleId::ALL.iter().map(|r| r.as_str()).collect();
    assert_eq!(unique.len(), 15);
    let snapshot: Value = serde_json::to_value(lint_registry()).unwrap();
    assert_eq!(
        snapshot,
        json!([
            {"id": "PLUMB.LINT.REQ.VAGUE_TERM", "description": "A vague term does not define an objective requirement.", "default_severity": "warn"},
            {"id": "PLUMB.LINT.REQ.PASSIVE_NO_ACTOR", "description": "A passive normative construction does not identify the responsible actor.", "default_severity": "warn"},
            {"id": "PLUMB.LINT.REQ.UNMEASURABLE_QUALIFIER", "description": "A qualifier is not objectively measurable.", "default_severity": "error"},
            {"id": "PLUMB.LINT.REQ.COMPOUND_MODAL", "description": "More than one \"shall\" may indicate more than one obligation.", "default_severity": "warn"},
            {"id": "PLUMB.LINT.REQ.NEGATION_STACK", "description": "Multiple negations in one clause may make the obligation ambiguous.", "default_severity": "warn"},
            {"id": "PLUMB.LINT.REQ.ESCAPE_CLAUSE", "description": "An escape clause weakens the requirement without an explicit condition.", "default_severity": "warn"},
            {"id": "PLUMB.LINT.REQ.OPEN_LIST", "description": "An open-ended list marker makes the requirement scope incomplete.", "default_severity": "error"},
            {"id": "PLUMB.LINT.REQ.PRONOUN_NO_ANTECEDENT", "description": "A pronoun in subject position has no explicit antecedent.", "default_severity": "warn"},
            {"id": "PLUMB.LINT.REQ.UI_PHRASED", "description": "The requirement is phrased as a user-interface interaction.", "default_severity": "warn"},
            {"id": "PLUMB.LINT.REQ.UNDEFINED_TERM", "description": "A supplied term mention has no defined vocabulary entry.", "default_severity": "error"},
            {"id": "PLUMB.LINT.REQ.EARS_ORDER", "description": "An EARS trigger, precondition or \"then\" is out of order.", "default_severity": "warn"},
            {"id": "PLUMB.LINT.REQ.NUMBER_NO_UNIT", "description": "A numeric value has no explicit unit or count noun.", "default_severity": "error"},
            {"id": "PLUMB.LINT.REQ.RELATIVE_TIME_NO_ANCHOR", "description": "A relative time constraint lacks an explicit anchor event.", "default_severity": "error"},
            {"id": "PLUMB.LINT.REQ.MISSING_ACTOR", "description": "A normative obligation has no explicit actor before the modal.", "default_severity": "error"},
            {"id": "PLUMB.LINT.REQ.AMBIGUOUS_QUANTIFIER", "description": "A quantifier does not define a precise quantity or frequency.", "default_severity": "warn"},
        ])
    );
    assert!("PLUMB.LINT.REQ.UNKNOWN".parse::<LintRuleId>().is_err());
    assert!(serde_json::from_value::<LintSeverity>(json!("blocker")).is_err());
    assert!(serde_json::from_value::<LintSeverity>(json!("Warn")).is_err());
}

// ---------------------------------------------------------------------------- rules

#[test]
fn rules_vague_term() {
    let s = "The system shall use an appropriate timeout.";
    assert_eq!(hits(s, VagueTerm), ["appropriate"]);
    assert_eq!(
        messages(s, VagueTerm),
        ["Vague term \"appropriate\" does not define an objective requirement."]
    );
    assert!(hits("The system shall use a 30 second timeout.", VagueTerm).is_empty());
    assert!(hits("Some quickly reliable abnormal output.", VagueTerm).is_empty());
}

#[test]
fn rules_passive_no_actor() {
    assert_eq!(
        hits("The request shall be stored.", PassiveNoActor),
        ["be stored"]
    );
    assert_eq!(
        hits("The request shall not be written.", PassiveNoActor),
        ["be written"]
    );
    assert!(hits(
        "The request shall be stored by the audit service.",
        PassiveNoActor
    )
    .is_empty());
    assert!(hits("The audit service shall store the request.", PassiveNoActor).is_empty());
    assert_eq!(
        messages("The request shall be stored.", PassiveNoActor),
        ["Passive requirement wording does not identify the responsible actor."]
    );
}

#[test]
fn rules_unmeasurable_qualifier() {
    assert_eq!(
        hits("The system shall respond quickly.", UnmeasurableQualifier),
        ["quickly"]
    );
    assert_eq!(
        hits("It must be User-Friendly.", UnmeasurableQualifier),
        ["User-Friendly"]
    );
    assert!(hits(
        "The system shall respond within 2 seconds of submission.",
        UnmeasurableQualifier
    )
    .is_empty());
    assert_eq!(
        messages("The system shall respond quickly.", UnmeasurableQualifier),
        ["Qualifier \"quickly\" is not objectively measurable."]
    );
}

#[test]
fn rules_compound_modal() {
    let s = "The system shall validate the request and shall store it.";
    let result = lint(s);
    let d: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == CompoundModal)
        .collect();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].statement_range, LintTextRange { start: 42, end: 47 });
    assert_eq!(&s[42..47], "shall");
    assert!(hits("The system shall validate the request.", CompoundModal).is_empty());
}

#[test]
fn rules_negation_stack() {
    assert_eq!(
        hits(
            "The system shall not process requests without approval.",
            NegationStack
        ),
        ["without"]
    );
    assert!(hits("The system shall not process the request.", NegationStack).is_empty());
    assert!(hits("It shall not stop. It shall never fail.", NegationStack).is_empty());
}

#[test]
fn rules_escape_clause() {
    let s = "The system shall notify the user where possible.";
    assert_eq!(hits(s, EscapeClause), ["where possible"]);
    assert_eq!(
        messages(s, EscapeClause),
        ["Escape clause \"where possible\" weakens the requirement without an explicit condition."]
    );
    assert!(hits(
        "The system shall notify the user when delivery fails.",
        EscapeClause
    )
    .is_empty());
}

#[test]
fn rules_open_list() {
    let s = "The record shall contain name, email, etc.";
    assert_eq!(hits(s, OpenList), ["etc."]);
    assert_eq!(
        hits("Fields: name, etc and so on", OpenList),
        ["etc", "and so on"]
    );
    assert!(hits("The record shall contain name and email.", OpenList).is_empty());
}

#[test]
fn rules_pronoun_no_antecedent() {
    assert_eq!(
        hits("It shall store the request.", PronounNoAntecedent),
        ["It"]
    );
    assert_eq!(
        hits(
            "When a request arrives, it shall be stored.",
            PronounNoAntecedent
        ),
        ["it"]
    );
    assert_eq!(
        hits(
            "If a request arrives, then they shall store it.",
            PronounNoAntecedent
        ),
        ["they"]
    );
    assert!(hits("The system shall store the request.", PronounNoAntecedent).is_empty());
    assert!(hits("This shall store the request.", PronounNoAntecedent).is_empty());
    assert_eq!(
        messages("It shall store the request.", PronounNoAntecedent),
        ["Pronoun \"It\" has no explicit antecedent in the requirement subject position."]
    );
}

#[test]
fn rules_ui_phrased() {
    let s = "The user shall click the Submit button.";
    assert_eq!(hits(s, UiPhrased), ["click the Submit button"]);
    assert_eq!(
        hits("On the screen the user shall double-click.", UiPhrased),
        ["screen the user shall double-click"]
    );
    assert!(hits("The system shall submit the request.", UiPhrased).is_empty());
    assert!(hits("The user shall click to continue.", UiPhrased).is_empty());
}

#[test]
fn rules_undefined_term() {
    let s = "The system shall compute the Entitlement.";
    let start = s.find("Entitlement").unwrap() as u64;
    let context = |defined: &[&str]| TermLintContext {
        defined_term_keys: defined.iter().map(|k| (*k).to_owned()).collect(),
        mentions: vec![TermMention {
            range: LintTextRange {
                start,
                end: start + 11,
            },
            normalized_key: "entitlement".to_owned(),
        }],
    };
    let with = |ctx: Option<TermLintContext>| {
        let mut i = input(s);
        i.term_context = ctx;
        lint_requirement(&i, &LintPolicy::default()).unwrap()
    };
    let undefined = with(Some(context(&[])));
    let d: Vec<_> = undefined
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == UndefinedTerm)
        .collect();
    assert_eq!(d.len(), 1);
    assert_eq!(
        d[0].statement_range,
        LintTextRange {
            start,
            end: start + 11
        }
    );
    assert!(with(Some(context(&["entitlement"])))
        .diagnostics
        .iter()
        .all(|d| d.rule_id != UndefinedTerm));
    let none = with(None);
    let evaluation = none
        .evaluations
        .iter()
        .find(|e| e.rule_id == UndefinedTerm)
        .unwrap();
    assert_eq!(
        evaluation.applicability,
        LintApplicability::NotEvaluated {
            reason: "term mention context is unavailable until vocabulary analysis".to_owned()
        }
    );
    assert!(evaluation.diagnostics.is_empty());
    // Invalid contexts.
    let mut bad = context(&[]);
    bad.mentions[0].normalized_key = " entitlement".to_owned();
    let mut i = input(s);
    i.term_context = Some(bad);
    assert!(matches!(
        lint_requirement(&i, &LintPolicy::default()),
        Err(LintError::InvalidTermContext { .. })
    ));
    let mut unsorted = context(&[]);
    unsorted.mentions.push(TermMention {
        range: LintTextRange { start: 0, end: 3 },
        normalized_key: "the".to_owned(),
    });
    i.term_context = Some(unsorted);
    assert!(matches!(
        lint_requirement(&i, &LintPolicy::default()),
        Err(LintError::InvalidTermContext { .. })
    ));
}

#[test]
fn rules_ears_order() {
    assert_eq!(
        hits(
            "The system shall notify the user when approval arrives.",
            EarsOrder
        ),
        ["when"]
    );
    assert!(hits(
        "When approval arrives, the system shall notify the user.",
        EarsOrder
    )
    .is_empty());
    assert_eq!(
        hits("Then the system shall notify the user.", EarsOrder),
        ["Then"]
    );
    assert!(hits(
        "If approval arrives, then the system shall notify the user.",
        EarsOrder
    )
    .is_empty());
    assert_eq!(
        messages("Then the system shall notify the user.", EarsOrder),
        ["EARS \"then\" appears without a preceding \"if\" condition."]
    );
}

#[test]
fn rules_number_no_unit() {
    assert_eq!(hits("The system shall retry 3.", NumberNoUnit), ["3"]);
    assert_eq!(hits("The ratio shall be 2.5.", NumberNoUnit), ["2.5"]);
    assert!(hits("The system shall retry 3 times.", NumberNoUnit).is_empty());
    assert!(hits("The threshold shall be 90%.", NumberNoUnit).is_empty());
    assert!(hits("The threshold shall be 90 %.", NumberNoUnit).is_empty());
    assert!(hits("The system shall support version 3.", NumberNoUnit).is_empty());
    assert!(hits("Store 10 MB and 5 requests over 2 days.", NumberNoUnit).is_empty());
    assert_eq!(
        messages("The system shall retry 3.", NumberNoUnit),
        ["Numeric value \"3\" has no explicit unit or count noun."]
    );
}

#[test]
fn rules_relative_time_no_anchor() {
    assert_eq!(
        hits(
            "The system shall respond within 2 days.",
            RelativeTimeNoAnchor
        ),
        ["within 2 days"]
    );
    assert_eq!(
        hits("Reply no later than 3 hours.", RelativeTimeNoAnchor),
        ["no later than 3 hours"]
    );
    assert!(hits(
        "The system shall respond within 2 days of submission.",
        RelativeTimeNoAnchor
    )
    .is_empty());
    assert!(hits(
        "The system shall respond within 2 days after approval.",
        RelativeTimeNoAnchor
    )
    .is_empty());
    assert!(hits(
        "The system shall respond within the budget.",
        RelativeTimeNoAnchor
    )
    .is_empty());
}

#[test]
fn rules_missing_actor() {
    assert_eq!(hits("Shall store the request.", MissingActor), ["Shall"]);
    assert_eq!(
        hits("When submitted, shall store the request.", MissingActor),
        ["shall"]
    );
    assert!(hits("The system shall store the request.", MissingActor).is_empty());
    // A passive sentence has a syntactic subject: passive may fire, missing actor does not.
    let s = "A request shall be stored.";
    assert!(hits(s, MissingActor).is_empty());
    assert_eq!(hits(s, PassiveNoActor), ["be stored"]);
}

#[test]
fn rules_ambiguous_quantifier() {
    assert_eq!(
        hits(
            "The system shall retain several copies.",
            AmbiguousQuantifier
        ),
        ["several"]
    );
    assert!(hits(
        "The system shall retain exactly 3 copies.",
        AmbiguousQuantifier
    )
    .is_empty());
    assert_eq!(
        messages(
            "The system shall retain several copies.",
            AmbiguousQuantifier
        ),
        ["Quantifier \"several\" does not define a precise quantity or frequency."]
    );
}

#[test]
fn rules_overlapping_rules_are_both_reported() {
    let s = "The system shall respond quickly as needed with adequate logs.";
    let rules: BTreeSet<LintRuleId> = lint(s).diagnostics.iter().map(|d| d.rule_id).collect();
    assert!(
        rules.contains(&EscapeClause)
            && rules.contains(&VagueTerm)
            && rules.contains(&UnmeasurableQualifier)
    );
}

// ---------------------------------------------------------------------------- result shape

#[test]
fn rules_result_shape_and_order() {
    let clean = lint("The system shall store the request.");
    assert_eq!(clean.evaluations.len(), 15);
    assert_eq!(
        clean
            .evaluations
            .iter()
            .map(|e| e.rule_id)
            .collect::<Vec<_>>(),
        LintRuleId::ALL.to_vec()
    );
    for e in &clean.evaluations {
        if e.rule_id == UndefinedTerm {
            assert!(matches!(
                e.applicability,
                LintApplicability::NotEvaluated { .. }
            ));
        } else {
            assert_eq!(e.applicability, LintApplicability::Evaluated);
        }
        assert!(e.diagnostics.is_empty());
    }
    let messy = lint("It shall respond quickly, shall retry 3 and shall log several events.");
    let keys: Vec<(&str, u64, u64)> = messy
        .diagnostics
        .iter()
        .map(|d| {
            (
                d.rule_id.as_str(),
                d.statement_range.start,
                d.statement_range.end,
            )
        })
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    let unique: BTreeSet<_> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len());
    let total: usize = messy.evaluations.iter().map(|e| e.diagnostics.len()).sum();
    assert_eq!(total, messy.diagnostics.len());
    // Equal input, equal result.
    assert_eq!(
        messy,
        lint("It shall respond quickly, shall retry 3 and shall log several events.")
    );
}

#[test]
fn rules_input_validation() {
    for statement in ["", "   ", "\n\t"] {
        assert!(matches!(
            lint_requirement(&input(statement), &LintPolicy::default()),
            Err(LintError::InvalidInput { .. })
        ));
    }
    let mut unsorted = input("The system shall log.");
    unsorted.evidence_refs = vec![id("evd:0000000000000002"), id("evd:0000000000000001")];
    assert!(matches!(
        lint_requirement(&unsorted, &LintPolicy::default()),
        Err(LintError::InvalidInput { .. })
    ));
}

#[test]
fn rules_utf8_statement_ranges() {
    let s = "Zażółć: the system shall respond quickly.";
    let d: Vec<LintDiagnostic> = lint(s)
        .diagnostics
        .into_iter()
        .filter(|d| d.rule_id == UnmeasurableQualifier)
        .collect();
    let r = d[0].statement_range;
    assert_eq!(&s[r.start as usize..r.end as usize], "quickly");
    assert_eq!(r.start as usize, s.find("quickly").unwrap());
    assert_ne!(
        r.start as usize,
        s.chars().position(|c| c == 'q').unwrap(),
        "bytes, not chars"
    );
    assert_eq!(d[0].evidence_range, None);
}

fn anchored(statement: &str, fragment: &str) -> LintInput {
    let start = fragment.find(statement).unwrap() as u64;
    LintInput {
        requirement_ref: id("req:0000000000000001"),
        statement: statement.to_owned(),
        evidence_refs: vec![id("evd:00000000000000aa")],
        source_anchor: Some(EvidenceStatementAnchor {
            fragment_ref: id("evd:00000000000000aa"),
            fragment_text: fragment.to_owned(),
            statement_start: start,
            statement_end: start + statement.len() as u64,
        }),
        term_context: None,
    }
}

#[test]
fn rules_evidence_ranges() {
    let statement = "The system shall respond quickly.";
    let fragment = "PREFIX The system shall respond quickly. SUFFIX";
    let result = lint_requirement(&anchored(statement, fragment), &LintPolicy::default()).unwrap();
    let d = result
        .diagnostics
        .iter()
        .find(|d| d.rule_id == UnmeasurableQualifier)
        .unwrap();
    let e = d.evidence_range.clone().unwrap();
    assert_eq!(e.fragment_ref, id("evd:00000000000000aa"));
    assert_eq!(&fragment[e.start as usize..e.end as usize], "quickly");
    assert_eq!(e.start, 7 + d.statement_range.start);
    // Transformed statement: no anchor, statement range only.
    let plain = lint(statement);
    let d = plain
        .diagnostics
        .iter()
        .find(|d| d.rule_id == UnmeasurableQualifier)
        .unwrap();
    assert_eq!(d.evidence_range, None);
    // Byte mismatch, cut through a character, foreign fragment.
    let mut bad = anchored(statement, fragment);
    bad.source_anchor.as_mut().unwrap().statement_end -= 1;
    assert!(matches!(
        lint_requirement(&bad, &LintPolicy::default()),
        Err(LintError::InvalidSourceAnchor { .. })
    ));
    let utf8 = "żądanie shall be kept.";
    let mut cut = anchored(utf8, &format!("Ż {utf8}"));
    cut.source_anchor.as_mut().unwrap().statement_start = 1;
    assert!(matches!(
        lint_requirement(&cut, &LintPolicy::default()),
        Err(LintError::InvalidSourceAnchor { .. })
    ));
    let mut foreign = anchored(statement, fragment);
    foreign.evidence_refs = vec![id("evd:00000000000000bb")];
    assert!(matches!(
        lint_requirement(&foreign, &LintPolicy::default()),
        Err(LintError::InvalidSourceAnchor { .. })
    ));
}

#[test]
fn rules_severity_override() {
    let s = "The system shall retry 3.";
    let default = lint(s);
    let mut policy = LintPolicy::default();
    policy
        .severity_overrides
        .insert(NumberNoUnit, LintSeverity::Warn);
    let overridden = lint_requirement(&input(s), &policy).unwrap();
    let a = default
        .diagnostics
        .iter()
        .find(|d| d.rule_id == NumberNoUnit)
        .unwrap();
    let b = overridden
        .diagnostics
        .iter()
        .find(|d| d.rule_id == NumberNoUnit)
        .unwrap();
    assert_eq!(a.default_severity, LintSeverity::Error);
    assert_eq!(a.effective_severity, LintSeverity::Error);
    assert_eq!(b.effective_severity, LintSeverity::Warn);
    assert_eq!(
        (a.rule_id, a.statement_range, &a.message, a.default_severity),
        (b.rule_id, b.statement_range, &b.message, b.default_severity)
    );
    assert_eq!(default.evaluations.len(), overridden.evaluations.len());
    let json = json!({"severity_overrides": {"PLUMB.LINT.REQ.NUMBER_NO_UNIT": "warn"}});
    assert_eq!(serde_json::from_value::<LintPolicy>(json).unwrap(), policy);
    assert!(serde_json::from_value::<LintPolicy>(
        json!({"severity_overrides": {"PLUMB.LINT.REQ.OTHER": "warn"}})
    )
    .is_err());
    assert!(
        serde_json::from_value::<LintPolicy>(json!({"severity_overrides": {}, "extra": 1}))
            .is_err()
    );
}

#[test]
fn rules_batch() {
    let mut a = input("The system shall respond quickly.");
    a.requirement_ref = id("req:00000000000000b2");
    let mut b = input("It shall retry 3.");
    b.requirement_ref = id("req:00000000000000b1");
    let forward = lint_requirements(&[a.clone(), b.clone()], &LintPolicy::default()).unwrap();
    let reversed = lint_requirements(&[b.clone(), a.clone()], &LintPolicy::default()).unwrap();
    assert_eq!(forward, reversed);
    assert_eq!(forward[0].requirement_ref, id("req:00000000000000b1"));
    assert!(matches!(
        lint_requirements(&[a.clone(), a], &LintPolicy::default()),
        Err(LintError::InvalidInput { .. })
    ));
}

#[test]
fn rules_lint_input_is_strict() {
    let value = json!({"requirement_ref": "req:0000000000000001", "statement": "x", "evidence_refs": [],
                       "source_anchor": null, "term_context": null, "extra": true});
    assert!(serde_json::from_value::<LintInput>(value).is_err());
}

// ---------------------------------------------------------------------------- benchmark

fn metadata() -> Value {
    json!({"record_type": "metadata", "version": 1, "corpus_id": "synthetic-test", "annotators": ["test-a", "test-b"]})
}

fn sentence(
    id: &str,
    domain: &str,
    text: &str,
    a: &[&str],
    b: &[&str],
    adjudicated: &[&str],
) -> Value {
    json!({
        "record_type": "sentence", "id": id, "domain": domain, "text": text,
        "annotations": [
            {"annotator": "test-a", "positive_rule_ids": a},
            {"annotator": "test-b", "positive_rule_ids": b},
        ],
        "adjudicated_positive_rule_ids": adjudicated,
    })
}

fn jsonl(records: &[Value]) -> String {
    records
        .iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

const VAGUE: &str = "PLUMB.LINT.REQ.VAGUE_TERM";
const QUICK: &str = "PLUMB.LINT.REQ.UNMEASURABLE_QUALIFIER";

/// Six synthetic sentences. VAGUE_TERM predictions (by the dictionary): s1, s2, s3 fire;
/// s4, s5, s6 do not. Adjudicated VAGUE_TERM: s1, s2, s4. Hand-derived: TP 2 (s1, s2),
/// FP 1 (s3), FN 1 (s4), TN 2 (s5, s6).
fn synthetic_corpus() -> String {
    jsonl(&[
        metadata(),
        sentence(
            "s1",
            "synthetic-hr",
            "The system shall use an appropriate timeout.",
            &[VAGUE],
            &[VAGUE],
            &[VAGUE],
        ),
        sentence(
            "s2",
            "synthetic-hr",
            "The clerk shall keep adequate records.",
            &[VAGUE],
            &[],
            &[VAGUE],
        ),
        sentence(
            "s3",
            "synthetic-ops",
            "The normal flow shall be logged by the auditor.",
            &[],
            &[],
            &[],
        ),
        sentence(
            "s4",
            "synthetic-ops",
            "The system shall use a fitting timeout.",
            &[VAGUE],
            &[VAGUE],
            &[VAGUE],
        ),
        sentence(
            "s5",
            "synthetic-ops",
            "The system shall respond quickly.",
            &[QUICK],
            &[],
            &[QUICK],
        ),
        sentence(
            "s6",
            "synthetic-hr",
            "The system shall store the request.",
            &[],
            &[],
            &[],
        ),
    ])
}

#[test]
fn rules_benchmark_metrics_and_agreement() {
    let corpus = parse_corpus_jsonl(&synthetic_corpus()).unwrap();
    assert_eq!(corpus.sentences.len(), 6);
    let report = run_benchmark(&corpus, DEFAULT_PRECISION_THRESHOLD).unwrap();
    assert_eq!(report.corpus_id, "synthetic-test");
    assert_eq!(report.sentence_count, 6);
    assert_eq!(report.metrics.len(), 15);
    let vague = report
        .metrics
        .iter()
        .find(|m| m.rule_id == VagueTerm)
        .unwrap();
    assert_eq!(
        (
            vague.true_positive,
            vague.false_positive,
            vague.false_negative,
            vague.true_negative
        ),
        (2, 1, 1, 2)
    );
    assert_eq!(
        vague.precision(),
        Some(Ratio {
            numerator: 2,
            denominator: 3
        })
    );
    assert_eq!(
        vague.recall(),
        Some(Ratio {
            numerator: 2,
            denominator: 3
        })
    );
    let quick = report
        .metrics
        .iter()
        .find(|m| m.rule_id == UnmeasurableQualifier)
        .unwrap();
    assert_eq!(
        (
            quick.true_positive,
            quick.false_positive,
            quick.false_negative,
            quick.true_negative
        ),
        (1, 0, 0, 5)
    );
    // Undefined precision (no predicted positives) and recall (no actual positives).
    let compound = report
        .metrics
        .iter()
        .find(|m| m.rule_id == CompoundModal)
        .unwrap();
    assert_eq!(compound.precision(), None);
    assert_eq!(compound.recall(), None);
    // UNDEFINED_TERM is not evaluated without corpus term context and never counted.
    let term = report
        .metrics
        .iter()
        .find(|m| m.rule_id == UndefinedTerm)
        .unwrap();
    assert_eq!(term.applicability, BenchmarkApplicability::NotEvaluated);
    assert_eq!(
        (
            term.true_positive,
            term.false_positive,
            term.false_negative,
            term.true_negative
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(term.precision(), None);
    // Agreement, hand-derived: 6 sentences x 15 rules = 90 decisions; disagreements are
    // s2 VAGUE (a yes, b no) and s5 QUICK (a yes, b no).
    assert_eq!(report.agreement.agreements, 88);
    assert_eq!(report.agreement.disagreements, 2);
    let vague_agreement = report
        .agreement
        .per_rule
        .iter()
        .find(|r| r.rule_id == VagueTerm)
        .unwrap();
    assert_eq!(
        (vague_agreement.agreements, vague_agreement.disagreements),
        (5, 1)
    );
    // VAGUE_TERM default is warn, so 2/3 < 0.85 recommends nothing; no rule is upgraded.
    assert!(report.severity_recommendations.is_empty());
    // Deterministic and serializable.
    assert_eq!(
        report,
        run_benchmark(&corpus, DEFAULT_PRECISION_THRESHOLD).unwrap()
    );
    let baseline = report.baseline();
    let round: BenchmarkBaseline =
        serde_json::from_value(serde_json::to_value(&baseline).unwrap()).unwrap();
    assert_eq!(round, baseline);
}

fn metric(rule: LintRuleId, tp: u32, fp: u32) -> RuleBenchmarkMetrics {
    RuleBenchmarkMetrics {
        rule_id: rule,
        applicability: BenchmarkApplicability::Evaluated,
        true_positive: tp,
        false_positive: fp,
        false_negative: 0,
        true_negative: 0,
    }
}

#[test]
fn rules_precision_threshold_boundary() {
    // NUMBER_NO_UNIT defaults to error.
    assert!(
        recommend_severities(&[metric(NumberNoUnit, 17, 3)], DEFAULT_PRECISION_THRESHOLD)
            .is_empty(),
        "17/20 = 0.85 does not downgrade"
    );
    let below = recommend_severities(&[metric(NumberNoUnit, 16, 4)], DEFAULT_PRECISION_THRESHOLD);
    assert_eq!(below.len(), 1);
    assert_eq!(below[0].rule_id, NumberNoUnit);
    assert_eq!(below[0].recommended_severity, LintSeverity::Warn);
    assert!(
        recommend_severities(&[metric(NumberNoUnit, 0, 0)], DEFAULT_PRECISION_THRESHOLD).is_empty(),
        "undefined precision"
    );
    let mut not_evaluated = metric(UndefinedTerm, 0, 5);
    not_evaluated.applicability = BenchmarkApplicability::NotEvaluated;
    assert!(recommend_severities(&[not_evaluated], DEFAULT_PRECISION_THRESHOLD).is_empty());
    // The registry is untouched.
    assert_eq!(
        NumberNoUnit.definition().default_severity,
        LintSeverity::Error
    );
    // The HR fixture profile's configured minimum is the same threshold (test-only read).
    let configured = HR_PROFILE
        .lines()
        .find_map(|l| l.trim().strip_prefix("default_precision_min:"))
        .unwrap()
        .trim();
    assert_eq!(configured, "0.85");
    assert_eq!(
        DEFAULT_PRECISION_THRESHOLD,
        Ratio {
            numerator: 85,
            denominator: 100
        }
    );
}

fn report_with(metrics: Vec<RuleBenchmarkMetrics>) -> BenchmarkReport {
    BenchmarkReport {
        corpus_id: "synthetic-test".to_owned(),
        sentence_count: 100,
        metrics,
        agreement: AgreementReport {
            agreements: 0,
            disagreements: 0,
            per_rule: Vec::new(),
        },
        severity_recommendations: Vec::new(),
    }
}

#[test]
fn rules_regression_boundary() {
    let baseline = report_with(vec![
        metric(VagueTerm, 90, 10),
        metric(OpenList, 90, 10),
        metric(CompoundModal, 0, 0),
    ])
    .baseline();
    // 0.90 -> 0.87 drops exactly 0.03 and passes; 0.90 -> 0.86 fails.
    let exact = compare_to_baseline(
        &report_with(vec![
            metric(VagueTerm, 87, 13),
            metric(OpenList, 86, 14),
            metric(CompoundModal, 5, 0),
        ]),
        &baseline,
    )
    .unwrap();
    assert_eq!(
        exact.per_rule.iter().map(|r| r.outcome).collect::<Vec<_>>(),
        [
            RegressionOutcome::Pass,
            RegressionOutcome::Fail,
            RegressionOutcome::NotComparable
        ]
    );
    assert_eq!(exact.failed, [OpenList]);
    let mut other = report_with(vec![metric(VagueTerm, 1, 0)]);
    assert!(matches!(
        compare_to_baseline(&other, &baseline),
        Err(LintError::Benchmark { .. })
    ));
    other.corpus_id = "another".to_owned();
    assert!(matches!(
        compare_to_baseline(&other, &baseline),
        Err(LintError::Benchmark { .. })
    ));
}

#[test]
fn rules_corpus_parser_strictness() {
    let good = synthetic_corpus();
    assert!(parse_corpus_jsonl(&good).is_ok());
    assert!(
        parse_corpus_jsonl(&format!("\n{good}\n\n")).is_ok(),
        "empty lines ignored"
    );
    let s1 = || sentence("s1", "d", "The system shall log.", &[], &[], &[]);
    let with = |records: Vec<Value>| parse_corpus_jsonl(&jsonl(&records));
    let mutate_meta = |f: &dyn Fn(&mut Value)| {
        let mut m = metadata();
        f(&mut m);
        with(vec![m, s1()])
    };
    let mutate_sentence = |f: &dyn Fn(&mut Value)| {
        let mut s = s1();
        f(&mut s);
        with(vec![metadata(), s])
    };
    let failures = [
        mutate_meta(&|m| m["extra"] = json!(1)),
        mutate_meta(&|m| m["version"] = json!(2)),
        mutate_meta(&|m| m["annotators"] = json!(["test-a", "test-a"])),
        mutate_meta(&|m| m["annotators"] = json!(["test-a"])),
        mutate_meta(&|m| m["annotators"] = json!(["test-a", "test-b", "test-c"])),
        mutate_sentence(&|s| s["extra"] = json!(1)),
        mutate_sentence(&|s| s["annotations"][0]["extra"] = json!(1)),
        mutate_sentence(&|s| {
            s["annotations"][0]["positive_rule_ids"] = json!(["PLUMB.LINT.REQ.NOPE"])
        }),
        mutate_sentence(&|s| s["annotations"][1]["annotator"] = json!("test-a")),
        mutate_sentence(&|s| s["annotations"][1]["annotator"] = json!("test-z")),
        mutate_sentence(&|s| {
            s["annotations"]
                .as_array_mut()
                .unwrap()
                .push(json!({"annotator": "test-a", "positive_rule_ids": []}))
        }),
        mutate_sentence(&|s| {
            s.as_object_mut()
                .unwrap()
                .remove("adjudicated_positive_rule_ids");
        }),
        mutate_sentence(&|s| s["adjudicated_positive_rule_ids"] = json!([VAGUE, QUICK])),
        mutate_sentence(&|s| s["adjudicated_positive_rule_ids"] = json!([QUICK, QUICK])),
        mutate_sentence(&|s| s["id"] = json!("")),
        mutate_sentence(&|s| s["domain"] = json!(" d")),
        with(vec![metadata(), s1(), s1()]),
        with(vec![metadata(), s1(), metadata()]),
        with(vec![metadata(), metadata(), s1()]),
        with(vec![s1()]),
        with(vec![metadata()]),
        parse_corpus_jsonl("{not json"),
    ];
    for (i, failure) in failures.into_iter().enumerate() {
        assert!(
            matches!(
                failure,
                Err(LintError::CorpusParse { .. } | LintError::CorpusValidation { .. })
            ),
            "case {i}: {failure:?}"
        );
    }
}

#[test]
fn rules_pilot_qualification_contract() {
    let corpus = parse_corpus_jsonl(&synthetic_corpus()).unwrap();
    assert!(matches!(
        validate_pilot_qualification_corpus(&corpus),
        Err(LintError::CorpusValidation { .. })
    ));
    // synthetic_count_contract: structural-only in-memory corpora of 199 and 201 sentences
    // are rejected; a real 200-sentence qualification is left unclaimed.
    for n in [199, 201] {
        let mut records = vec![metadata()];
        for i in 0..n {
            records.push(sentence(
                &format!("c{i}"),
                if i % 2 == 0 {
                    "synthetic-a"
                } else {
                    "synthetic-b"
                },
                "The system shall log.",
                &[],
                &[],
                &[],
            ));
        }
        let corpus = parse_corpus_jsonl(&jsonl(&records)).unwrap();
        assert!(
            matches!(
                validate_pilot_qualification_corpus(&corpus),
                Err(LintError::CorpusValidation { .. })
            ),
            "{n}"
        );
    }
    assert_eq!(PILOT_QUALIFICATION_SENTENCES, 200);
}

#[test]
fn rules_no_corpus_directory_needed_and_no_precision_claims() {
    for (name, source) in [
        ("lib.rs", include_str!("../src/lib.rs")),
        ("rules.rs", include_str!("../src/rules.rs")),
        ("benchmark.rs", include_str!("../src/benchmark.rs")),
    ] {
        for token in [
            "std::fs",
            "File::open",
            "reqwest",
            "ArtifactStore",
            "rusqlite",
            "RevisionStore",
            "SystemClock",
            "Clock::now",
            "Utc::now",
            "Instant::now",
            "SemanticPatch",
            "Proposal",
            "Graph",
            "InferenceRequest",
            "plumb_functional",
            "plumb_validation",
            "lint-corpus",
            "unsafe",
            "precision is >=",
            "precision >= 0.85",
        ] {
            assert!(!source.contains(token), "{name} contains {token}");
        }
    }
    let _: BTreeMap<LintRuleId, LintSeverity> = BTreeMap::new();
}
