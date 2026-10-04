//! Deterministic requirement lint (plan S1.4; compiler architecture §7): fifteen closed lint
//! rules over the current `Requirement.statement` with exact statement byte ranges, and a
//! benchmark framework for an externally supplied human-labelled corpus.
//!
//! The crate depends on neither plumb-functional nor plumb-validation, produces plain
//! diagnostics (no PSG Findings, patches or proposals), reads no file and calls no model.
//! No lint corpus is bundled and no measured precision is claimed.

mod benchmark;
mod rules;

pub use benchmark::{
    compare_to_baseline, parse_corpus_jsonl, recommend_severities, run_benchmark,
    validate_pilot_qualification_corpus, AgreementReport, BenchmarkApplicability,
    BenchmarkBaseline, BenchmarkRegression, BenchmarkReport, CorpusAnnotation, CorpusMetadata,
    CorpusSentence, LintCorpus, Ratio, RegressionOutcome, RuleAgreement, RuleBenchmarkMetrics,
    RuleRegression, SeverityRecommendation, CORPUS_VERSION, DEFAULT_PRECISION_THRESHOLD,
    PILOT_QUALIFICATION_SENTENCES, REGRESSION_TOLERANCE,
};
pub use rules::{
    lint_registry, lint_requirement, lint_requirements, EvidenceStatementAnchor, LintApplicability,
    LintDiagnostic, LintError, LintEvidenceRange, LintInput, LintPolicy, LintResult,
    LintRuleDefinition, LintRuleEvaluation, LintRuleId, LintSeverity, LintTextRange,
    TermLintContext, TermMention, UnknownLintValue,
};
