//! The lint benchmark framework (plan S1.4): a strict parser for an externally supplied,
//! human-labelled two-annotator corpus, exact agreement counts, exact per-rule confusion
//! counts, precision/recall as rational values, the precision severity recommendation and the
//! precision-regression check.
//!
//! No corpus is bundled and no measured precision is claimed. The framework recommends warn
//! severity when measured precision on an externally labelled qualification corpus is below
//! the threshold; it never mutates the rule registry or any severity, and it reads and writes
//! no file: the caller supplies the corpus text and persists any returned baseline.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::Id;
use serde::{Deserialize, Serialize};

use crate::rules::{
    lint_requirement, LintApplicability, LintError, LintInput, LintPolicy, LintRuleId, LintSeverity,
};

/// The only supported corpus and baseline version.
pub const CORPUS_VERSION: u32 = 1;

/// The sentence count of a pilot qualification corpus.
pub const PILOT_QUALIFICATION_SENTENCES: usize = 200;

/// The default precision qualification threshold, 85/100.
pub const DEFAULT_PRECISION_THRESHOLD: Ratio = Ratio {
    numerator: 85,
    denominator: 100,
};

/// The precision drop that fails a regression, 3/100 (an exactly equal drop passes).
pub const REGRESSION_TOLERANCE: Ratio = Ratio {
    numerator: 3,
    denominator: 100,
};

/// An exact non-negative rational value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ratio {
    pub numerator: u32,
    pub denominator: u32,
}

impl Ratio {
    /// `self < other`, by u128 cross-multiplication.
    fn lt(self, other: Ratio) -> bool {
        u128::from(self.numerator) * u128::from(other.denominator)
            < u128::from(other.numerator) * u128::from(self.denominator)
    }
}

fn corpus_error(reason: impl Into<String>) -> LintError {
    LintError::CorpusValidation {
        reason: reason.into(),
    }
}

fn benchmark_error(reason: impl Into<String>) -> LintError {
    LintError::Benchmark {
        reason: reason.into(),
    }
}

fn is_clean(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

// ============================================================================ corpus

/// The corpus metadata record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusMetadata {
    pub version: u32,
    pub corpus_id: String,
    pub annotators: Vec<String>,
}

/// One annotator's positive rule labels for a sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusAnnotation {
    pub annotator: String,
    pub positive_rule_ids: Vec<LintRuleId>,
}

/// One labelled sentence; the adjudicated labels are the human-approved benchmark truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusSentence {
    pub id: String,
    pub domain: String,
    pub text: String,
    pub annotations: Vec<CorpusAnnotation>,
    pub adjudicated_positive_rule_ids: Vec<LintRuleId>,
}

/// A parsed, validated corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintCorpus {
    pub metadata: CorpusMetadata,
    pub sentences: Vec<CorpusSentence>,
}

#[derive(Deserialize)]
#[serde(tag = "record_type", rename_all = "snake_case", deny_unknown_fields)]
enum CorpusRecord {
    Metadata {
        version: u32,
        corpus_id: String,
        annotators: Vec<String>,
    },
    Sentence {
        id: String,
        domain: String,
        text: String,
        annotations: Vec<CorpusAnnotation>,
        adjudicated_positive_rule_ids: Vec<LintRuleId>,
    },
}

/// Labels must be strictly sorted by wire value (sorted and unique).
fn sorted_labels(labels: &[LintRuleId]) -> bool {
    labels.windows(2).all(|p| p[0].as_str() < p[1].as_str())
}

fn validate_corpus(corpus: &LintCorpus) -> Result<(), LintError> {
    let metadata = &corpus.metadata;
    if metadata.version != CORPUS_VERSION {
        return Err(corpus_error(format!(
            "unsupported corpus version {}",
            metadata.version
        )));
    }
    if !is_clean(&metadata.corpus_id) {
        return Err(corpus_error("corpus_id is empty or not clean"));
    }
    if metadata.annotators.len() != 2
        || metadata.annotators[0] == metadata.annotators[1]
        || !metadata.annotators.iter().all(|a| is_clean(a))
    {
        return Err(corpus_error(
            "metadata must name exactly two distinct clean annotators",
        ));
    }
    if corpus.sentences.is_empty() {
        return Err(corpus_error("corpus has no sentence records"));
    }
    let expected: BTreeSet<&String> = metadata.annotators.iter().collect();
    let mut ids = BTreeSet::new();
    for sentence in &corpus.sentences {
        let at = |reason: &str| corpus_error(format!("sentence {:?}: {reason}", sentence.id));
        if !is_clean(&sentence.id) {
            return Err(at("id is empty or not clean"));
        }
        if !ids.insert(&sentence.id) {
            return Err(at("duplicate sentence id"));
        }
        if !is_clean(&sentence.domain) {
            return Err(at("domain is empty or not clean"));
        }
        if sentence.text.chars().all(char::is_whitespace) {
            return Err(at("text is empty"));
        }
        let annotators: BTreeSet<&String> =
            sentence.annotations.iter().map(|a| &a.annotator).collect();
        if sentence.annotations.len() != 2 || annotators != expected {
            return Err(at(
                "needs exactly one annotation from each metadata annotator",
            ));
        }
        if sentence
            .annotations
            .iter()
            .any(|a| !sorted_labels(&a.positive_rule_ids))
            || !sorted_labels(&sentence.adjudicated_positive_rule_ids)
        {
            return Err(at("rule labels are not sorted and unique"));
        }
    }
    Ok(())
}

/// Parses a corpus from caller-supplied JSONL text. Completely empty lines are ignored; the
/// first record must be the metadata record and every later record a sentence.
pub fn parse_corpus_jsonl(input: &str) -> Result<LintCorpus, LintError> {
    let mut metadata = None;
    let mut sentences = Vec::new();
    for (index, line) in input.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let record: CorpusRecord =
            serde_json::from_str(line).map_err(|e| LintError::CorpusParse {
                line: index + 1,
                reason: e.to_string(),
            })?;
        match record {
            CorpusRecord::Metadata {
                version,
                corpus_id,
                annotators,
            } => {
                if metadata.is_some() {
                    return Err(corpus_error("second metadata record"));
                }
                if !sentences.is_empty() {
                    return Err(corpus_error("metadata record after sentence records"));
                }
                metadata = Some(CorpusMetadata {
                    version,
                    corpus_id,
                    annotators,
                });
            }
            CorpusRecord::Sentence {
                id,
                domain,
                text,
                annotations,
                adjudicated_positive_rule_ids,
            } => {
                if metadata.is_none() {
                    return Err(corpus_error("the first record is not the metadata record"));
                }
                sentences.push(CorpusSentence {
                    id,
                    domain,
                    text,
                    annotations,
                    adjudicated_positive_rule_ids,
                });
            }
        }
    }
    let metadata = metadata.ok_or_else(|| corpus_error("missing metadata record"))?;
    let corpus = LintCorpus {
        metadata,
        sentences,
    };
    validate_corpus(&corpus)?;
    Ok(corpus)
}

/// The pilot qualification contract: exactly 200 sentences, two annotators and at least two
/// domains. Rule IDs are closed by type. No agreement threshold is defined.
pub fn validate_pilot_qualification_corpus(corpus: &LintCorpus) -> Result<(), LintError> {
    validate_corpus(corpus)?;
    if corpus.sentences.len() != PILOT_QUALIFICATION_SENTENCES {
        return Err(corpus_error(format!(
            "a pilot qualification corpus has exactly {PILOT_QUALIFICATION_SENTENCES} sentences, got {}",
            corpus.sentences.len()
        )));
    }
    let domains: BTreeSet<&String> = corpus.sentences.iter().map(|s| &s.domain).collect();
    if domains.len() < 2 {
        return Err(corpus_error(
            "a pilot qualification corpus spans at least two domains",
        ));
    }
    Ok(())
}

// ============================================================================ metrics

/// Whether a rule was automatically evaluated over the corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkApplicability {
    Evaluated,
    NotEvaluated,
}

/// Exact confusion counts of one rule against the adjudicated labels. A not-evaluated rule has
/// all-zero counts that are never read as measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleBenchmarkMetrics {
    pub rule_id: LintRuleId,
    pub applicability: BenchmarkApplicability,
    pub true_positive: u32,
    pub false_positive: u32,
    pub false_negative: u32,
    pub true_negative: u32,
}

impl RuleBenchmarkMetrics {
    /// TP / (TP + FP), undefined when not evaluated or without predicted positives.
    pub fn precision(&self) -> Option<Ratio> {
        self.ratio(self.true_positive + self.false_positive)
    }

    /// TP / (TP + FN), undefined when not evaluated or without actual positives.
    pub fn recall(&self) -> Option<Ratio> {
        self.ratio(self.true_positive + self.false_negative)
    }

    fn ratio(&self, denominator: u32) -> Option<Ratio> {
        (self.applicability == BenchmarkApplicability::Evaluated && denominator > 0).then_some(
            Ratio {
                numerator: self.true_positive,
                denominator,
            },
        )
    }
}

/// Exact binary agreement of the two annotators for one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleAgreement {
    pub rule_id: LintRuleId,
    pub agreements: u32,
    pub disagreements: u32,
}

/// Exact agreement counts over every sentence and rule; no kappa or alpha.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgreementReport {
    pub agreements: u32,
    pub disagreements: u32,
    pub per_rule: Vec<RuleAgreement>,
}

/// A recommended configuration override; never applied by the benchmark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeverityRecommendation {
    pub rule_id: LintRuleId,
    pub recommended_severity: LintSeverity,
    pub reason: String,
}

/// The result of one benchmark run, in rule order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkReport {
    pub corpus_id: String,
    pub sentence_count: u32,
    pub metrics: Vec<RuleBenchmarkMetrics>,
    pub agreement: AgreementReport,
    pub severity_recommendations: Vec<SeverityRecommendation>,
}

/// A benchmark baseline as returned to the caller for external persistence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkBaseline {
    pub version: u32,
    pub corpus_id: String,
    pub corpus_sentence_count: u32,
    pub per_rule: Vec<RuleBenchmarkMetrics>,
    pub agreement: AgreementReport,
}

impl BenchmarkReport {
    /// The baseline of this run.
    pub fn baseline(&self) -> BenchmarkBaseline {
        BenchmarkBaseline {
            version: CORPUS_VERSION,
            corpus_id: self.corpus_id.clone(),
            corpus_sentence_count: self.sentence_count,
            per_rule: self.metrics.clone(),
            agreement: self.agreement.clone(),
        }
    }
}

fn count(n: usize) -> Result<u32, LintError> {
    u32::try_from(n).map_err(|_| benchmark_error("count exceeds u32"))
}

fn agreement(corpus: &LintCorpus) -> AgreementReport {
    let [first, second] = [
        &corpus.metadata.annotators[0],
        &corpus.metadata.annotators[1],
    ];
    let mut per_rule: BTreeMap<LintRuleId, (u32, u32)> =
        LintRuleId::ALL.iter().map(|r| (*r, (0, 0))).collect();
    for sentence in &corpus.sentences {
        let labels = |annotator: &String| -> BTreeSet<LintRuleId> {
            sentence
                .annotations
                .iter()
                .filter(|a| &a.annotator == annotator)
                .flat_map(|a| a.positive_rule_ids.iter().copied())
                .collect()
        };
        let (a, b) = (labels(first), labels(second));
        for rule in LintRuleId::ALL {
            let entry = per_rule.entry(rule).or_default();
            if a.contains(&rule) == b.contains(&rule) {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
    }
    let per_rule: Vec<RuleAgreement> = LintRuleId::ALL
        .iter()
        .map(|rule| {
            let (agreements, disagreements) = per_rule[rule];
            RuleAgreement {
                rule_id: *rule,
                agreements,
                disagreements,
            }
        })
        .collect();
    AgreementReport {
        agreements: per_rule.iter().map(|r| r.agreements).sum(),
        disagreements: per_rule.iter().map(|r| r.disagreements).sum(),
        per_rule,
    }
}

/// Lints every corpus sentence with the default policy (no term context, so UNDEFINED_TERM
/// is not evaluated) and compares predictions with the adjudicated labels. A rule whose
/// defined precision is below `threshold` and whose default severity is above warn gets a
/// warn recommendation; nothing is applied.
pub fn run_benchmark(corpus: &LintCorpus, threshold: Ratio) -> Result<BenchmarkReport, LintError> {
    validate_corpus(corpus)?;
    if threshold.denominator == 0 || threshold.numerator > threshold.denominator {
        return Err(benchmark_error("threshold must be a ratio in [0, 1]"));
    }
    let policy = LintPolicy::default();
    let mut counts: BTreeMap<LintRuleId, [u32; 4]> = BTreeMap::new();
    let mut evaluated: BTreeMap<LintRuleId, bool> =
        LintRuleId::ALL.iter().map(|r| (*r, true)).collect();
    for (index, sentence) in corpus.sentences.iter().enumerate() {
        let input = LintInput {
            requirement_ref: format!("corpus:{index}").parse::<Id>()?,
            statement: sentence.text.clone(),
            evidence_refs: Vec::new(),
            source_anchor: None,
            term_context: None,
        };
        let result = lint_requirement(&input, &policy)?;
        for evaluation in &result.evaluations {
            let rule = evaluation.rule_id;
            if evaluation.applicability != LintApplicability::Evaluated {
                evaluated.insert(rule, false);
                continue;
            }
            let predicted = !evaluation.diagnostics.is_empty();
            let actual = sentence.adjudicated_positive_rule_ids.contains(&rule);
            let cell = match (predicted, actual) {
                (true, true) => 0,
                (true, false) => 1,
                (false, true) => 2,
                (false, false) => 3,
            };
            counts.entry(rule).or_default()[cell] += 1;
        }
    }
    let metrics: Vec<RuleBenchmarkMetrics> = LintRuleId::ALL
        .iter()
        .map(|rule| {
            let [tp, fp, fn_, tn] = if evaluated[rule] {
                counts.get(rule).copied().unwrap_or_default()
            } else {
                [0; 4]
            };
            RuleBenchmarkMetrics {
                rule_id: *rule,
                applicability: if evaluated[rule] {
                    BenchmarkApplicability::Evaluated
                } else {
                    BenchmarkApplicability::NotEvaluated
                },
                true_positive: tp,
                false_positive: fp,
                false_negative: fn_,
                true_negative: tn,
            }
        })
        .collect();
    let severity_recommendations = recommend_severities(&metrics, threshold);
    Ok(BenchmarkReport {
        corpus_id: corpus.metadata.corpus_id.clone(),
        sentence_count: count(corpus.sentences.len())?,
        metrics,
        agreement: agreement(corpus),
        severity_recommendations,
    })
}

/// Warn recommendations for evaluated rules with defined precision below `threshold` whose
/// default severity is above warn; a recommendation never raises a severity.
pub fn recommend_severities(
    metrics: &[RuleBenchmarkMetrics],
    threshold: Ratio,
) -> Vec<SeverityRecommendation> {
    let mut out: Vec<SeverityRecommendation> = metrics
        .iter()
        .filter_map(|m| {
            let precision = m.precision()?;
            (precision.lt(threshold)
                && m.rule_id.definition().default_severity > LintSeverity::Warn)
                .then(|| SeverityRecommendation {
                    rule_id: m.rule_id,
                    recommended_severity: LintSeverity::Warn,
                    reason: format!(
                        "measured precision {}/{} is below {}/{}",
                        precision.numerator,
                        precision.denominator,
                        threshold.numerator,
                        threshold.denominator
                    ),
                })
        })
        .collect();
    out.sort_by_key(|r| r.rule_id);
    out
}

// ============================================================================ regression

/// The regression outcome of one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionOutcome {
    /// Precision did not drop by more than the tolerance.
    Pass,
    /// Precision dropped by more than the tolerance.
    Fail,
    /// Precision is undefined or the rule was not evaluated on one side.
    NotComparable,
}

/// The regression of one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleRegression {
    pub rule_id: LintRuleId,
    pub outcome: RegressionOutcome,
}

/// Per-rule regression outcomes; `failed` lists every Fail rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkRegression {
    pub per_rule: Vec<RuleRegression>,
    pub failed: Vec<LintRuleId>,
}

/// `old - new > tolerance`, exactly: `(o_n*n_d - n_n*o_d) * t_d > t_n * o_d * n_d`.
fn drop_exceeds(old: Ratio, new: Ratio, tolerance: Ratio) -> bool {
    let (o_n, o_d) = (i128::from(old.numerator), i128::from(old.denominator));
    let (n_n, n_d) = (i128::from(new.numerator), i128::from(new.denominator));
    let (t_n, t_d) = (
        i128::from(tolerance.numerator),
        i128::from(tolerance.denominator),
    );
    (o_n * n_d - n_n * o_d) * t_d > t_n * o_d * n_d
}

/// Compares a run with a baseline of the same corpus and rule set; a rule fails when its
/// precision drops by more than 3/100.
pub fn compare_to_baseline(
    report: &BenchmarkReport,
    baseline: &BenchmarkBaseline,
) -> Result<BenchmarkRegression, LintError> {
    if baseline.version != CORPUS_VERSION {
        return Err(benchmark_error(format!(
            "unsupported baseline version {}",
            baseline.version
        )));
    }
    if report.corpus_id != baseline.corpus_id {
        return Err(benchmark_error("report and baseline corpora differ"));
    }
    let report_rules: Vec<LintRuleId> = report.metrics.iter().map(|m| m.rule_id).collect();
    let baseline_rules: Vec<LintRuleId> = baseline.per_rule.iter().map(|m| m.rule_id).collect();
    if report_rules != baseline_rules {
        return Err(benchmark_error("report and baseline rule sets differ"));
    }
    let per_rule: Vec<RuleRegression> = report
        .metrics
        .iter()
        .zip(&baseline.per_rule)
        .map(|(new, old)| RuleRegression {
            rule_id: new.rule_id,
            outcome: match (old.precision(), new.precision()) {
                (Some(o), Some(n)) if drop_exceeds(o, n, REGRESSION_TOLERANCE) => {
                    RegressionOutcome::Fail
                }
                (Some(_), Some(_)) => RegressionOutcome::Pass,
                _ => RegressionOutcome::NotComparable,
            },
        })
        .collect();
    let failed = per_rule
        .iter()
        .filter(|r| r.outcome == RegressionOutcome::Fail)
        .map(|r| r.rule_id)
        .collect();
    Ok(BenchmarkRegression { per_rule, failed })
}
