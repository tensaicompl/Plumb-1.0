//! The fifteen deterministic pilot requirement lint rules (plan S1.4; compiler architecture
//! §7).
//!
//! Rules evaluate the current `Requirement.statement` and report exact UTF-8 byte ranges of it.
//! An EvidenceFragment sub-range is reported only when a validated anchor shows that the
//! statement is a byte-identical slice of the fragment text; no evidence offset is ever guessed
//! for transformed text. Diagnostics are plain analysis material, not PSG Findings. The rule
//! vocabularies are exactly those of the plan and are never expanded or tuned at runtime.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use plumb_core::{CoreError, Id};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

// ============================================================================ errors

/// Why linting or benchmarking could not run.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LintError {
    /// The lint input is invalid (empty statement, unsorted evidence, duplicate requirement).
    #[error("invalid lint input: {reason}")]
    InvalidInput { reason: String },
    /// The lint policy is invalid.
    #[error("invalid lint policy: {reason}")]
    InvalidPolicy { reason: String },
    /// The source anchor does not map the statement byte-identically.
    #[error("invalid source anchor: {reason}")]
    InvalidSourceAnchor { reason: String },
    /// The supplied term context is invalid.
    #[error("invalid term context: {reason}")]
    InvalidTermContext { reason: String },
    /// A corpus line is not valid JSON of the corpus format.
    #[error("corpus line {line}: {reason}")]
    CorpusParse { line: usize, reason: String },
    /// The corpus violates the corpus contract.
    #[error("invalid corpus: {reason}")]
    CorpusValidation { reason: String },
    /// A benchmark could not be computed or compared.
    #[error("benchmark: {reason}")]
    Benchmark { reason: String },
    /// An identifier or canonical value could not be built.
    #[error(transparent)]
    Core(#[from] CoreError),
}

pub(crate) fn invalid_input(reason: impl Into<String>) -> LintError {
    LintError::InvalidInput {
        reason: reason.into(),
    }
}

// ============================================================================ vocabularies

/// A string that is not a value of a closed lint vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown {vocabulary} value {value:?}")]
pub struct UnknownLintValue {
    pub vocabulary: &'static str,
    pub value: String,
}

macro_rules! closed_vocabulary {
    ($(#[$meta:meta])* $name:ident, $count:literal { $($variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every value, in specification order.
            pub const ALL: [$name; $count] = [$($name::$variant),+];

            /// The exact wire string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $wire),+
                }
            }
        }

        impl FromStr for $name {
            type Err = UnknownLintValue;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                $name::ALL
                    .into_iter()
                    .find(|value| value.as_str() == s)
                    .ok_or_else(|| UnknownLintValue {
                        vocabulary: stringify!($name),
                        value: s.to_owned(),
                    })
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                value.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

closed_vocabulary! {
    /// The fifteen pilot lint rule IDs. These are lint codes, not validation-rule metadata.
    LintRuleId, 15 {
        VagueTerm => "PLUMB.LINT.REQ.VAGUE_TERM",
        PassiveNoActor => "PLUMB.LINT.REQ.PASSIVE_NO_ACTOR",
        UnmeasurableQualifier => "PLUMB.LINT.REQ.UNMEASURABLE_QUALIFIER",
        CompoundModal => "PLUMB.LINT.REQ.COMPOUND_MODAL",
        NegationStack => "PLUMB.LINT.REQ.NEGATION_STACK",
        EscapeClause => "PLUMB.LINT.REQ.ESCAPE_CLAUSE",
        OpenList => "PLUMB.LINT.REQ.OPEN_LIST",
        PronounNoAntecedent => "PLUMB.LINT.REQ.PRONOUN_NO_ANTECEDENT",
        UiPhrased => "PLUMB.LINT.REQ.UI_PHRASED",
        UndefinedTerm => "PLUMB.LINT.REQ.UNDEFINED_TERM",
        EarsOrder => "PLUMB.LINT.REQ.EARS_ORDER",
        NumberNoUnit => "PLUMB.LINT.REQ.NUMBER_NO_UNIT",
        RelativeTimeNoAnchor => "PLUMB.LINT.REQ.RELATIVE_TIME_NO_ANCHOR",
        MissingActor => "PLUMB.LINT.REQ.MISSING_ACTOR",
        AmbiguousQuantifier => "PLUMB.LINT.REQ.AMBIGUOUS_QUANTIFIER",
    }
}

closed_vocabulary! {
    /// Lint severity. Gate severities (blocker) belong to validation, not to lint.
    LintSeverity, 3 {
        Info => "info",
        Warn => "warn",
        Error => "error",
    }
}

/// One immutable registry entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LintRuleDefinition {
    pub id: LintRuleId,
    pub description: &'static str,
    pub default_severity: LintSeverity,
}

const REGISTRY: [LintRuleDefinition; 15] = [
    LintRuleDefinition {
        id: LintRuleId::VagueTerm,
        description: "A vague term does not define an objective requirement.",
        default_severity: LintSeverity::Warn,
    },
    LintRuleDefinition {
        id: LintRuleId::PassiveNoActor,
        description: "A passive normative construction does not identify the responsible actor.",
        default_severity: LintSeverity::Warn,
    },
    LintRuleDefinition {
        id: LintRuleId::UnmeasurableQualifier,
        description: "A qualifier is not objectively measurable.",
        default_severity: LintSeverity::Error,
    },
    LintRuleDefinition {
        id: LintRuleId::CompoundModal,
        description: "More than one \"shall\" may indicate more than one obligation.",
        default_severity: LintSeverity::Warn,
    },
    LintRuleDefinition {
        id: LintRuleId::NegationStack,
        description: "Multiple negations in one clause may make the obligation ambiguous.",
        default_severity: LintSeverity::Warn,
    },
    LintRuleDefinition {
        id: LintRuleId::EscapeClause,
        description: "An escape clause weakens the requirement without an explicit condition.",
        default_severity: LintSeverity::Warn,
    },
    LintRuleDefinition {
        id: LintRuleId::OpenList,
        description: "An open-ended list marker makes the requirement scope incomplete.",
        default_severity: LintSeverity::Error,
    },
    LintRuleDefinition {
        id: LintRuleId::PronounNoAntecedent,
        description: "A pronoun in subject position has no explicit antecedent.",
        default_severity: LintSeverity::Warn,
    },
    LintRuleDefinition {
        id: LintRuleId::UiPhrased,
        description: "The requirement is phrased as a user-interface interaction.",
        default_severity: LintSeverity::Warn,
    },
    LintRuleDefinition {
        id: LintRuleId::UndefinedTerm,
        description: "A supplied term mention has no defined vocabulary entry.",
        default_severity: LintSeverity::Error,
    },
    LintRuleDefinition {
        id: LintRuleId::EarsOrder,
        description: "An EARS trigger, precondition or \"then\" is out of order.",
        default_severity: LintSeverity::Warn,
    },
    LintRuleDefinition {
        id: LintRuleId::NumberNoUnit,
        description: "A numeric value has no explicit unit or count noun.",
        default_severity: LintSeverity::Error,
    },
    LintRuleDefinition {
        id: LintRuleId::RelativeTimeNoAnchor,
        description: "A relative time constraint lacks an explicit anchor event.",
        default_severity: LintSeverity::Error,
    },
    LintRuleDefinition {
        id: LintRuleId::MissingActor,
        description: "A normative obligation has no explicit actor before the modal.",
        default_severity: LintSeverity::Error,
    },
    LintRuleDefinition {
        id: LintRuleId::AmbiguousQuantifier,
        description: "A quantifier does not define a precise quantity or frequency.",
        default_severity: LintSeverity::Warn,
    },
];

impl LintRuleId {
    /// The registry entry of this rule.
    pub fn definition(self) -> &'static LintRuleDefinition {
        &REGISTRY[self as usize]
    }
}

/// Every registry entry, in specification order.
pub fn lint_registry() -> &'static [LintRuleDefinition; 15] {
    &REGISTRY
}

// ============================================================================ input

/// Caller-supplied severity overrides; the default policy has none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintPolicy {
    pub severity_overrides: BTreeMap<LintRuleId, LintSeverity>,
}

impl LintPolicy {
    /// The override if present, else the registry default.
    pub fn effective_severity(&self, rule: LintRuleId) -> LintSeverity {
        self.severity_overrides
            .get(&rule)
            .copied()
            .unwrap_or(rule.definition().default_severity)
    }
}

/// A zero-based, end-exclusive UTF-8 byte range of the current statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintTextRange {
    pub start: u64,
    pub end: u64,
}

impl LintTextRange {
    fn new(start: usize, end: usize) -> LintTextRange {
        LintTextRange {
            start: start as u64,
            end: end as u64,
        }
    }

    /// The range as `usize` bounds when it is a valid UTF-8 range of `text`.
    fn bounds_in(&self, text: &str) -> Option<(usize, usize)> {
        let start = usize::try_from(self.start).ok()?;
        let end = usize::try_from(self.end).ok()?;
        (start < end
            && end <= text.len()
            && text.is_char_boundary(start)
            && text.is_char_boundary(end))
        .then_some((start, end))
    }
}

/// The exact EvidenceFragment byte range a diagnostic corresponds to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintEvidenceRange {
    pub fragment_ref: Id,
    pub start: u64,
    pub end: u64,
}

/// Evidence that the statement is exactly `fragment_text[statement_start..statement_end]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceStatementAnchor {
    pub fragment_ref: Id,
    pub fragment_text: String,
    pub statement_start: u64,
    pub statement_end: u64,
}

/// One vocabulary mention supplied by later vocabulary analysis.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermMention {
    pub range: LintTextRange,
    pub normalized_key: String,
}

/// The defined vocabulary keys and the statement's term mentions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermLintContext {
    pub defined_term_keys: BTreeSet<String>,
    pub mentions: Vec<TermMention>,
}

/// The neutral lint input for one requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintInput {
    pub requirement_ref: Id,
    pub statement: String,
    pub evidence_refs: Vec<Id>,
    pub source_anchor: Option<EvidenceStatementAnchor>,
    pub term_context: Option<TermLintContext>,
}

fn is_clean(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

fn validate_input(input: &LintInput) -> Result<(), LintError> {
    let statement = &input.statement;
    if statement.chars().all(char::is_whitespace) {
        return Err(invalid_input(format!(
            "{}: statement is empty or whitespace-only",
            input.requirement_ref
        )));
    }
    if input.evidence_refs.windows(2).any(|p| p[0] >= p[1]) {
        return Err(invalid_input(format!(
            "{}: evidence_refs are not strictly sorted",
            input.requirement_ref
        )));
    }
    if let Some(anchor) = &input.source_anchor {
        let invalid = |reason: &str| LintError::InvalidSourceAnchor {
            reason: format!("{}: {reason}", input.requirement_ref),
        };
        let range = LintTextRange {
            start: anchor.statement_start,
            end: anchor.statement_end,
        };
        let (start, end) = range
            .bounds_in(&anchor.fragment_text)
            .ok_or_else(|| invalid("statement range is not a valid UTF-8 range of the fragment"))?;
        if &anchor.fragment_text[start..end] != statement {
            return Err(invalid(
                "fragment slice is not byte-identical to the statement",
            ));
        }
        if !input.evidence_refs.contains(&anchor.fragment_ref) {
            return Err(invalid("anchor fragment is not among the evidence refs"));
        }
    }
    if let Some(context) = &input.term_context {
        let invalid = |reason: &str| LintError::InvalidTermContext {
            reason: format!("{}: {reason}", input.requirement_ref),
        };
        for mention in &context.mentions {
            if mention.range.bounds_in(statement).is_none() {
                return Err(invalid("mention range is not a valid UTF-8 range"));
            }
            if !is_clean(&mention.normalized_key) {
                return Err(invalid("mention key is empty or not clean"));
            }
        }
        if context.mentions.windows(2).any(|p| p[0] >= p[1]) {
            return Err(invalid("mentions are not strictly sorted and unique"));
        }
    }
    Ok(())
}

// ============================================================================ results

/// Whether a rule was evaluated for a requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "applicability", rename_all = "snake_case", deny_unknown_fields)]
pub enum LintApplicability {
    Evaluated,
    NotEvaluated { reason: String },
}

/// One reported lint occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintDiagnostic {
    pub rule_id: LintRuleId,
    pub default_severity: LintSeverity,
    pub effective_severity: LintSeverity,
    pub requirement_ref: Id,
    pub statement_range: LintTextRange,
    pub evidence_range: Option<LintEvidenceRange>,
    pub message: String,
}

impl LintDiagnostic {
    fn order_key(&self) -> (&'static str, LintTextRange, &str) {
        (self.rule_id.as_str(), self.statement_range, &self.message)
    }
}

/// The evaluation of one rule for one requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintRuleEvaluation {
    pub rule_id: LintRuleId,
    pub applicability: LintApplicability,
    pub diagnostics: Vec<LintDiagnostic>,
}

/// Fifteen evaluations in registry order and every diagnostic in canonical order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LintResult {
    pub requirement_ref: Id,
    pub evaluations: Vec<LintRuleEvaluation>,
    pub diagnostics: Vec<LintDiagnostic>,
}

// ============================================================================ lexical helpers

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_clause_boundary(b: u8) -> bool {
    matches!(b, b';' | b'.' | b'?' | b'!')
}

/// A maximal `[A-Za-z0-9_]` run with exact byte offsets and its ASCII-lowercase text.
struct Token {
    start: usize,
    end: usize,
    lower: String,
}

fn tokens(statement: &str) -> Vec<Token> {
    let bytes = statement.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if is_word(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_word(bytes[i]) {
                i += 1;
            }
            out.push(Token {
                start,
                end: i,
                lower: statement[start..i].to_ascii_lowercase(),
            });
        } else {
            i += 1;
        }
    }
    out
}

/// Every ASCII case-insensitive occurrence of `phrase` bounded by non-word bytes.
fn phrase_matches(statement: &str, phrase: &str) -> Vec<(usize, usize)> {
    let bytes = statement.as_bytes();
    let p = phrase.as_bytes();
    let mut out = Vec::new();
    if p.is_empty() || p.len() > bytes.len() {
        return out;
    }
    for start in 0..=bytes.len() - p.len() {
        let end = start + p.len();
        if bytes[start..end].eq_ignore_ascii_case(p)
            && (start == 0 || !is_word(bytes[start - 1]))
            && (end == bytes.len() || !is_word(bytes[end]))
        {
            out.push((start, end));
        }
    }
    out
}

/// Matches of every phrase, keeping only the longest match at each start offset.
fn longest_matches(statement: &str, phrases: &[&str]) -> Vec<(usize, usize)> {
    let mut by_start: BTreeMap<usize, usize> = BTreeMap::new();
    for phrase in phrases {
        for (start, end) in phrase_matches(statement, phrase) {
            let entry = by_start.entry(start).or_insert(end);
            *entry = (*entry).max(end);
        }
    }
    by_start.into_iter().collect()
}

const MODALS: [&str; 5] = ["shall", "should", "may", "must", "can"];

fn is_modal(token: &Token) -> bool {
    MODALS.contains(&token.lower.as_str())
}

fn first_modal(tokens: &[Token]) -> Option<usize> {
    tokens.iter().position(is_modal)
}

/// The clause index of each byte: the number of clause boundaries before it.
fn clause_of(statement: &str, offset: usize) -> usize {
    statement.as_bytes()[..offset]
        .iter()
        .filter(|b| is_clause_boundary(**b))
        .count()
}

// ============================================================================ rules

/// A raw rule hit before severity and evidence resolution.
struct Hit {
    start: usize,
    end: usize,
    message: String,
}

fn hit(start: usize, end: usize, message: impl Into<String>) -> Hit {
    Hit {
        start,
        end,
        message: message.into(),
    }
}

fn dictionary(statement: &str, phrases: &[&str], message: impl Fn(&str) -> String) -> Vec<Hit> {
    longest_matches(statement, phrases)
        .into_iter()
        .map(|(start, end)| hit(start, end, message(&statement[start..end])))
        .collect()
}

const VAGUE_TERMS: [&str; 7] = [
    "appropriate",
    "adequate",
    "reasonable",
    "suitable",
    "sufficient",
    "acceptable",
    "normal",
];

const UNMEASURABLE_QUALIFIERS: [&str; 9] = [
    "quickly",
    "promptly",
    "rapidly",
    "efficiently",
    "easily",
    "seamlessly",
    "reliably",
    "user-friendly",
    "intuitive",
];

const ESCAPE_CLAUSES: [&str; 10] = [
    "if possible",
    "where possible",
    "if practical",
    "where practical",
    "as applicable",
    "where applicable",
    "unless otherwise specified",
    "unless otherwise stated",
    "as needed",
    "as necessary",
];

const OPEN_LIST_MARKERS: [&str; 7] = [
    "etc",
    "etc.",
    "and so on",
    "and the like",
    "including but not limited to",
    "including without limitation",
    "among others",
];

const AMBIGUOUS_QUANTIFIERS: [&str; 12] = [
    "some",
    "several",
    "many",
    "few",
    "most",
    "various",
    "multiple",
    "numerous",
    "frequently",
    "occasionally",
    "regularly",
    "often",
];

const PARTICIPLES: [&str; 12] = [
    "built", "done", "given", "kept", "known", "made", "read", "sent", "set", "shown", "taken",
    "written",
];

fn is_participle(token: &Token) -> bool {
    token.lower.ends_with("ed")
        || token.lower.ends_with("en")
        || PARTICIPLES.contains(&token.lower.as_str())
}

fn passive_no_actor(statement: &str, tokens: &[Token]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for i in 0..tokens.len() {
        if !is_modal(&tokens[i]) {
            continue;
        }
        let mut j = i + 1;
        if tokens.get(j).is_some_and(|t| t.lower == "not") {
            j += 1;
        }
        let (Some(be), Some(participle)) = (tokens.get(j), tokens.get(j + 1)) else {
            continue;
        };
        if be.lower != "be" || !is_participle(participle) {
            continue;
        }
        let clause_end = statement.as_bytes()[participle.end..]
            .iter()
            .position(|b| is_clause_boundary(*b))
            .map_or(statement.len(), |p| participle.end + p);
        let by_actor = tokens
            .iter()
            .any(|t| t.start >= participle.end && t.end <= clause_end && t.lower == "by");
        if !by_actor {
            hits.push(hit(
                be.start,
                participle.end,
                "Passive requirement wording does not identify the responsible actor.",
            ));
        }
    }
    hits
}

fn compound_modal(tokens: &[Token]) -> Vec<Hit> {
    tokens
        .iter()
        .filter(|t| t.lower == "shall")
        .skip(1)
        .map(|t| {
            hit(
                t.start,
                t.end,
                "Additional \"shall\" may indicate more than one obligation in the requirement.",
            )
        })
        .collect()
}

fn negation_stack(statement: &str, tokens: &[Token]) -> Vec<Hit> {
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    let mut hits = Vec::new();
    for t in tokens {
        if !matches!(t.lower.as_str(), "not" | "no" | "never" | "without") {
            continue;
        }
        if !seen.insert(clause_of(statement, t.start)) {
            hits.push(hit(
                t.start,
                t.end,
                "Multiple negations in one clause may make the obligation ambiguous.",
            ));
        }
    }
    hits
}

fn pronoun_no_antecedent(statement: &str, tokens: &[Token]) -> Vec<Hit> {
    let Some(first) = tokens.first() else {
        return Vec::new();
    };
    let mut subject = Some(first);
    if matches!(first.lower.as_str(), "when" | "while" | "where" | "if") {
        let modal_start = first_modal(tokens).map(|i| tokens[i].start);
        let comma = statement.as_bytes()[first.end..]
            .iter()
            .position(|b| *b == b',')
            .map(|p| first.end + p)
            .filter(|c| modal_start.is_some_and(|m| *c < m));
        if let Some(comma) = comma {
            let mut after = tokens.iter().filter(|t| t.start > comma);
            subject = after.next();
            if subject.is_some_and(|t| t.lower == "then") {
                subject = after.next();
            }
        }
    }
    match subject {
        Some(t) if matches!(t.lower.as_str(), "it" | "they" | "them" | "he" | "she") => vec![hit(
            t.start,
            t.end,
            format!(
                "Pronoun \"{}\" has no explicit antecedent in the requirement subject position.",
                &statement[t.start..t.end]
            ),
        )],
        _ => Vec::new(),
    }
}

const UI_NOUNS: [&str; 11] = [
    "button", "link", "icon", "checkbox", "dropdown", "menu", "tab", "screen", "page", "dialog",
    "window",
];

fn ui_phrased(statement: &str) -> Vec<Hit> {
    let actions = longest_matches(
        statement,
        &["click", "tap", "press", "swipe", "double-click"],
    );
    // A word action inside a longer action (click in double-click) is that action.
    let actions: Vec<(usize, usize)> = actions
        .iter()
        .copied()
        .filter(|(s, e)| {
            !actions
                .iter()
                .any(|(s2, e2)| s2 <= s && e <= e2 && (s2, e2) != (s, e))
        })
        .collect();
    let nouns = longest_matches(statement, &UI_NOUNS);
    if actions.is_empty() || nouns.is_empty() {
        return Vec::new();
    }
    let message = "Requirement is phrased as a user-interface interaction rather than \
                   implementation-independent behavior.";
    actions
        .iter()
        .filter_map(|&(a_start, a_end)| {
            if let Some(&(_, n_end)) = nouns.iter().find(|(n_start, _)| *n_start >= a_end) {
                Some(hit(a_start, n_end, message))
            } else {
                nouns
                    .iter()
                    .rev()
                    .find(|(n_start, _)| *n_start < a_start)
                    .map(|&(n_start, _)| hit(n_start, a_end, message))
            }
        })
        .collect()
}

fn undefined_term(context: &TermLintContext) -> Vec<Hit> {
    context
        .mentions
        .iter()
        .filter(|m| !context.defined_term_keys.contains(&m.normalized_key))
        .map(|m| {
            hit(
                m.range.start as usize,
                m.range.end as usize,
                format!(
                    "Term \"{}\" has no defined vocabulary entry.",
                    m.normalized_key
                ),
            )
        })
        .collect()
}

fn ears_order(tokens: &[Token]) -> Vec<Hit> {
    let Some(m) = first_modal(tokens) else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for t in &tokens[m + 1..] {
        if matches!(t.lower.as_str(), "when" | "while" | "where" | "if") {
            hits.push(hit(
                t.start,
                t.end,
                "EARS trigger or precondition appears after the normative response.",
            ));
        }
    }
    for (i, t) in tokens[..m].iter().enumerate() {
        if t.lower == "then" && !tokens[..i].iter().any(|p| p.lower == "if") {
            hits.push(hit(
                t.start,
                t.end,
                "EARS \"then\" appears without a preceding \"if\" condition.",
            ));
        }
    }
    hits
}

/// Every `[0-9]+` or `[0-9]+.[0-9]+` number with ASCII token boundaries.
fn numbers(statement: &str) -> Vec<(usize, usize)> {
    let bytes = statement.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() && (i == 0 || !is_word(bytes[i - 1])) {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i == bytes.len() || !is_word(bytes[i]) {
                out.push((start, i));
            }
        } else {
            i += 1;
        }
    }
    out
}

fn number_no_unit(statement: &str, tokens: &[Token]) -> Vec<Hit> {
    let bytes = statement.as_bytes();
    let mut hits = Vec::new();
    for (start, end) in numbers(statement) {
        let after = &bytes[end..];
        let spaces = after.iter().take_while(|b| b.is_ascii_whitespace()).count();
        if after.get(spaces) == Some(&b'%') {
            continue;
        }
        let next_is_unit = spaces > 0 && after.get(spaces).is_some_and(|b| is_word(*b));
        if next_is_unit {
            continue;
        }
        let previous = tokens.iter().rev().find(|t| t.end <= start);
        if previous.is_some_and(|t| {
            matches!(
                t.lower.as_str(),
                "version" | "release" | "section" | "id" | "code" | "step" | "phase"
            )
        }) {
            continue;
        }
        hits.push(hit(
            start,
            end,
            format!(
                "Numeric value \"{}\" has no explicit unit or count noun.",
                &statement[start..end]
            ),
        ));
    }
    hits
}

const TIME_UNITS: [&str; 23] = [
    "ms",
    "s",
    "sec",
    "second",
    "seconds",
    "min",
    "minute",
    "minutes",
    "h",
    "hr",
    "hrs",
    "hour",
    "hours",
    "day",
    "days",
    "week",
    "weeks",
    "month",
    "months",
    "year",
    "years",
    "millisecond",
    "milliseconds",
];

fn relative_time_no_anchor(statement: &str, tokens: &[Token]) -> Vec<Hit> {
    let numbers = numbers(statement);
    let mut hits = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        let starter_end_index = if t.lower == "within" {
            i
        } else if t.lower == "no"
            && tokens
                .get(i + 1)
                .is_some_and(|n| n.lower == "later" || n.lower == "earlier")
            && tokens.get(i + 2).is_some_and(|n| n.lower == "than")
        {
            i + 2
        } else {
            continue;
        };
        let Some(next) = tokens.get(starter_end_index + 1) else {
            continue;
        };
        let Some(&(_, number_end)) = numbers.iter().find(|(s, _)| *s == next.start) else {
            continue;
        };
        let Some(unit_index) = tokens.iter().position(|u| u.start >= number_end) else {
            continue;
        };
        let unit = &tokens[unit_index];
        if !TIME_UNITS.contains(&unit.lower.as_str()) {
            continue;
        }
        let anchored = tokens[unit_index + 1..].iter().take(3).any(|a| {
            matches!(
                a.lower.as_str(),
                "of" | "after" | "from" | "following" | "upon"
            )
        });
        if !anchored {
            hits.push(hit(
                t.start,
                unit.end,
                "Relative time constraint lacks an explicit anchor event.",
            ));
        }
    }
    hits
}

fn missing_actor(statement: &str, tokens: &[Token]) -> Vec<Hit> {
    let Some(m) = first_modal(tokens) else {
        return Vec::new();
    };
    let modal = &tokens[m];
    let clause_start = statement.as_bytes()[..modal.start]
        .iter()
        .rposition(|b| matches!(b, b',' | b';' | b':' | b'.' | b'?' | b'!'))
        .map_or(0, |p| p + 1);
    let has_actor = tokens[..m].iter().any(|t| {
        t.start >= clause_start
            && !matches!(
                t.lower.as_str(),
                "a" | "an" | "the" | "when" | "while" | "where" | "if" | "then"
            )
    });
    if has_actor {
        Vec::new()
    } else {
        vec![hit(
            modal.start,
            modal.end,
            "Normative obligation has no explicit actor before the modal.",
        )]
    }
}

fn evaluate_rule(
    rule: LintRuleId,
    input: &LintInput,
    tokens: &[Token],
) -> Result<Vec<Hit>, LintApplicability> {
    let s = input.statement.as_str();
    Ok(match rule {
        LintRuleId::VagueTerm => dictionary(s, &VAGUE_TERMS, |m| {
            format!("Vague term \"{m}\" does not define an objective requirement.")
        }),
        LintRuleId::PassiveNoActor => passive_no_actor(s, tokens),
        LintRuleId::UnmeasurableQualifier => dictionary(s, &UNMEASURABLE_QUALIFIERS, |m| {
            format!("Qualifier \"{m}\" is not objectively measurable.")
        }),
        LintRuleId::CompoundModal => compound_modal(tokens),
        LintRuleId::NegationStack => negation_stack(s, tokens),
        LintRuleId::EscapeClause => dictionary(s, &ESCAPE_CLAUSES, |m| {
            format!("Escape clause \"{m}\" weakens the requirement without an explicit condition.")
        }),
        LintRuleId::OpenList => dictionary(s, &OPEN_LIST_MARKERS, |m| {
            format!("Open-ended list marker \"{m}\" makes the requirement scope incomplete.")
        }),
        LintRuleId::PronounNoAntecedent => pronoun_no_antecedent(s, tokens),
        LintRuleId::UiPhrased => ui_phrased(s),
        LintRuleId::UndefinedTerm => match &input.term_context {
            Some(context) => undefined_term(context),
            None => {
                return Err(LintApplicability::NotEvaluated {
                    reason: "term mention context is unavailable until vocabulary analysis"
                        .to_owned(),
                })
            }
        },
        LintRuleId::EarsOrder => ears_order(tokens),
        LintRuleId::NumberNoUnit => number_no_unit(s, tokens),
        LintRuleId::RelativeTimeNoAnchor => relative_time_no_anchor(s, tokens),
        LintRuleId::MissingActor => missing_actor(s, tokens),
        LintRuleId::AmbiguousQuantifier => dictionary(s, &AMBIGUOUS_QUANTIFIERS, |m| {
            format!("Quantifier \"{m}\" does not define a precise quantity or frequency.")
        }),
    })
}

/// The exact fragment range of a statement range under a validated anchor.
fn evidence_range(
    input: &LintInput,
    start: usize,
    end: usize,
) -> Result<Option<LintEvidenceRange>, LintError> {
    let Some(anchor) = &input.source_anchor else {
        return Ok(None);
    };
    let offset = anchor.statement_start as usize;
    let (e_start, e_end) = (offset + start, offset + end);
    if anchor.fragment_text.get(e_start..e_end) != Some(&input.statement[start..end]) {
        return Err(invalid_input(
            "evidence range does not map byte-identically",
        ));
    }
    Ok(Some(LintEvidenceRange {
        fragment_ref: anchor.fragment_ref.clone(),
        start: e_start as u64,
        end: e_end as u64,
    }))
}

/// Lints one requirement's current statement. All input is validated before any rule runs.
pub fn lint_requirement(input: &LintInput, policy: &LintPolicy) -> Result<LintResult, LintError> {
    validate_input(input)?;
    let tokens = tokens(&input.statement);
    let mut evaluations = Vec::with_capacity(LintRuleId::ALL.len());
    let mut all = Vec::new();
    for rule in LintRuleId::ALL {
        let (applicability, hits) = match evaluate_rule(rule, input, &tokens) {
            Ok(hits) => (LintApplicability::Evaluated, hits),
            Err(not_evaluated) => (not_evaluated, Vec::new()),
        };
        let mut unique: BTreeMap<(usize, usize, String), ()> = BTreeMap::new();
        for h in hits {
            unique.insert((h.start, h.end, h.message), ());
        }
        let mut diagnostics = Vec::new();
        for (start, end, message) in unique.into_keys() {
            diagnostics.push(LintDiagnostic {
                rule_id: rule,
                default_severity: rule.definition().default_severity,
                effective_severity: policy.effective_severity(rule),
                requirement_ref: input.requirement_ref.clone(),
                statement_range: LintTextRange::new(start, end),
                evidence_range: evidence_range(input, start, end)?,
                message,
            });
        }
        all.extend(diagnostics.iter().cloned());
        evaluations.push(LintRuleEvaluation {
            rule_id: rule,
            applicability,
            diagnostics,
        });
    }
    all.sort_by(|a, b| a.order_key().cmp(&b.order_key()));
    Ok(LintResult {
        requirement_ref: input.requirement_ref.clone(),
        evaluations,
        diagnostics: all,
    })
}

/// Lints several requirements; results are sorted by requirement_ref, which must be unique.
pub fn lint_requirements(
    inputs: &[LintInput],
    policy: &LintPolicy,
) -> Result<Vec<LintResult>, LintError> {
    let mut seen = BTreeSet::new();
    for input in inputs {
        if !seen.insert(&input.requirement_ref) {
            return Err(invalid_input(format!(
                "duplicate requirement_ref {}",
                input.requirement_ref
            )));
        }
    }
    let mut results = inputs
        .iter()
        .map(|input| lint_requirement(input, policy))
        .collect::<Result<Vec<_>, _>>()?;
    results.sort_by(|a, b| a.requirement_ref.cmp(&b.requirement_ref));
    Ok(results)
}
