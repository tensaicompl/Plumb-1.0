//! Typed decision-table analysis (plan S2.6, relocated by Hotfix 044; compiler architecture §8).
//!
//! The canonical `DecisionTable` payload keeps generic `inputs`, `outputs` and `rows`, and no
//! encoding of them is frozen, so this module defines no wire format and produces no PSG
//! proposal. It analyzes a typed [`DecisionTableSpec`]: Bool, Enum, Int and Decimal columns,
//! `UNIQUE` and `FIRST` hit policies, exact overlap detection and coverage where the pilot
//! subset is analyzable (finite Bool/Enum products and one numeric column over a declared
//! finite domain). Int intervals are discrete, Decimal intervals continuous and exact; no
//! floats, solver, approximation, findings, mutation or provider are involved.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// The largest finite Bool/Enum input product enumerated for coverage.
pub const MAX_FINITE_DECISION_POINTS: u64 = 4096;

/// The largest number of uncovered witnesses reported.
pub const MAX_COVERAGE_WITNESSES: usize = 32;

// ============================================================================ model

/// The supported hit policies: `UNIQUE` (no overlap) and `FIRST` (row order decides).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitPolicy {
    #[serde(rename = "UNIQUE")]
    Unique,
    #[serde(rename = "FIRST")]
    First,
}

impl HitPolicy {
    /// Exactly `UNIQUE` or `FIRST`, case-sensitive.
    pub fn parse(value: &str) -> Option<HitPolicy> {
        match value {
            "UNIQUE" => Some(HitPolicy::Unique),
            "FIRST" => Some(HitPolicy::First),
            _ => None,
        }
    }
}

/// A column type. `Unsupported` names a domain outside the pilot subset (String, Date,
/// DateTime, Quantity, Duration, Ref, List, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionType {
    Bool,
    Enum { members: Vec<String> },
    Int,
    Decimal { scale: u32 },
    Unsupported { name: String },
}

/// A finite closed numeric analysis domain `[lower, upper]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumericDomain {
    pub lower: Decimal,
    pub upper: Decimal,
}

/// A table column; numeric input columns may declare a finite analysis domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionColumn {
    pub name: String,
    pub ty: DecisionType,
    pub domain: Option<NumericDomain>,
}

/// An interval endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumericBound {
    pub value: Decimal,
    pub inclusive: bool,
}

/// A numeric interval; a missing bound is unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumericInterval {
    pub lower: Option<NumericBound>,
    pub upper: Option<NumericBound>,
}

/// An input cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cell", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionInputCell {
    Any,
    Bool { value: bool },
    EnumSet { members: Vec<String> },
    Interval { interval: NumericInterval },
}

/// An output cell: exactly one literal of the column type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cell", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionOutputCell {
    Bool { value: bool },
    Enum { member: String },
    Int { value: i64 },
    Decimal { value: Decimal },
}

/// One rule row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionRow {
    pub inputs: Vec<DecisionInputCell>,
    pub outputs: Vec<DecisionOutputCell>,
}

/// A typed decision table (an analysis structure, not a PSG payload).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionTableSpec {
    pub hit_policy: String,
    pub inputs: Vec<DecisionColumn>,
    pub outputs: Vec<DecisionColumn>,
    pub rows: Vec<DecisionRow>,
    pub default_output: Option<Vec<DecisionOutputCell>>,
}

// ============================================================================ results

/// A structural problem; any one makes overlap and coverage unanalyzed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionStructureIssue {
    UnsupportedHitPolicy {
        hit_policy: String,
    },
    InvalidEnumDomain {
        column: usize,
    },
    InvalidNumericDomain {
        column: usize,
    },
    UnsupportedInputDomain {
        column: usize,
    },
    UnsupportedOutputDomain {
        column: usize,
    },
    RowArityMismatch {
        row: usize,
    },
    DefaultArityMismatch,
    InvalidCell {
        row: usize,
        column: usize,
    },
    UnknownEnumMember {
        row: usize,
        column: usize,
        member: String,
    },
    EmptyEnumSet {
        row: usize,
        column: usize,
    },
    InvalidInterval {
        row: usize,
        column: usize,
    },
    EmptyInterval {
        row: usize,
        column: usize,
    },
    InvalidOutputCell {
        row: Option<usize>,
        column: usize,
    },
}

/// Two rows whose input domains intersect, `left_row < right_row`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionOverlap {
    pub left_row: usize,
    pub right_row: usize,
}

/// One value of an uncovered finite input point.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionPointValue {
    Bool { value: bool },
    Enum { member: String },
}

/// Why coverage is not analyzed in the pilot subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotAnalyzableReason {
    FiniteDomainTooLarge,
    UnboundedNumericDomain,
    MultiDimensionalNumericDomain,
    MixedNumericDomain,
    UnsupportedDomain,
}

/// The coverage verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "coverage", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionCoverage {
    Complete,
    CompleteByDefault,
    /// Uncovered finite points (at most [`MAX_COVERAGE_WITNESSES`], canonical order).
    Incomplete {
        witnesses: Vec<Vec<DecisionPointValue>>,
    },
    /// Uncovered numeric gaps of the single numeric column (inclusive integer ranges for Int).
    IncompleteNumeric {
        gaps: Vec<NumericInterval>,
    },
    NotAnalyzable {
        reason: NotAnalyzableReason,
    },
    /// The table is structurally invalid.
    StructureInvalid,
}

/// The deterministic analysis of one table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionTableAnalysis {
    pub structure: Vec<DecisionStructureIssue>,
    pub overlaps: Vec<DecisionOverlap>,
    /// Overlaps that the hit policy forbids (UNIQUE); empty under FIRST.
    pub illegal_overlaps: Vec<DecisionOverlap>,
    pub coverage: DecisionCoverage,
}

// ============================================================================ structure

fn is_numeric(ty: &DecisionType) -> bool {
    matches!(ty, DecisionType::Int | DecisionType::Decimal { .. })
}

fn is_finite(ty: &DecisionType) -> bool {
    matches!(ty, DecisionType::Bool | DecisionType::Enum { .. })
}

fn sorted_unique(values: &[String]) -> bool {
    values.windows(2).all(|p| p[0] < p[1])
}

fn check_output(column: &DecisionColumn, cell: &DecisionOutputCell) -> bool {
    match (&column.ty, cell) {
        (DecisionType::Bool, DecisionOutputCell::Bool { .. })
        | (DecisionType::Int, DecisionOutputCell::Int { .. }) => true,
        (DecisionType::Enum { members }, DecisionOutputCell::Enum { member }) => {
            members.contains(member)
        }
        (DecisionType::Decimal { scale }, DecisionOutputCell::Decimal { value }) => {
            value.scale() <= *scale
        }
        _ => false,
    }
}

fn check_interval(interval: &NumericInterval) -> Option<bool> {
    // None: valid; Some(true): lower > upper; Some(false): empty equal bounds.
    match (interval.lower, interval.upper) {
        (Some(l), Some(u)) => match l.value.cmp(&u.value) {
            Ordering::Greater => Some(true),
            Ordering::Equal if !(l.inclusive && u.inclusive) => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn structure(spec: &DecisionTableSpec) -> Vec<DecisionStructureIssue> {
    use DecisionStructureIssue as I;
    let mut issues = Vec::new();
    if HitPolicy::parse(&spec.hit_policy).is_none() {
        issues.push(I::UnsupportedHitPolicy {
            hit_policy: spec.hit_policy.clone(),
        });
    }
    for (c, column) in spec.inputs.iter().enumerate() {
        match &column.ty {
            DecisionType::Unsupported { .. } => {
                issues.push(I::UnsupportedInputDomain { column: c })
            }
            DecisionType::Enum { members } if members.is_empty() || !sorted_unique(members) => {
                issues.push(I::InvalidEnumDomain { column: c })
            }
            _ => {}
        }
        match (&column.domain, is_numeric(&column.ty)) {
            (Some(d), true) if d.lower > d.upper => {
                issues.push(I::InvalidNumericDomain { column: c })
            }
            (Some(_), false) => issues.push(I::InvalidNumericDomain { column: c }),
            _ => {}
        }
    }
    for (c, column) in spec.outputs.iter().enumerate() {
        match &column.ty {
            DecisionType::Unsupported { .. } => {
                issues.push(I::UnsupportedOutputDomain { column: c })
            }
            DecisionType::Enum { members } if members.is_empty() || !sorted_unique(members) => {
                issues.push(I::InvalidEnumDomain { column: c })
            }
            _ => {}
        }
    }
    for (r, row) in spec.rows.iter().enumerate() {
        if row.inputs.len() != spec.inputs.len() || row.outputs.len() != spec.outputs.len() {
            issues.push(I::RowArityMismatch { row: r });
            continue;
        }
        for (c, (cell, column)) in row.inputs.iter().zip(&spec.inputs).enumerate() {
            match (cell, &column.ty) {
                (DecisionInputCell::Any, _)
                | (DecisionInputCell::Bool { .. }, DecisionType::Bool) => {}
                (
                    DecisionInputCell::EnumSet { members },
                    DecisionType::Enum { members: domain },
                ) => {
                    if members.is_empty() {
                        issues.push(I::EmptyEnumSet { row: r, column: c });
                    } else if !sorted_unique(members) {
                        issues.push(I::InvalidCell { row: r, column: c });
                    }
                    for member in members {
                        if !domain.contains(member) {
                            issues.push(I::UnknownEnumMember {
                                row: r,
                                column: c,
                                member: member.clone(),
                            });
                        }
                    }
                }
                (DecisionInputCell::Interval { interval }, ty) if is_numeric(ty) => {
                    match check_interval(interval) {
                        Some(true) => issues.push(I::InvalidInterval { row: r, column: c }),
                        Some(false) => issues.push(I::EmptyInterval { row: r, column: c }),
                        None => {}
                    }
                }
                (_, DecisionType::Unsupported { .. }) => {}
                _ => issues.push(I::InvalidCell { row: r, column: c }),
            }
        }
        for (c, (cell, column)) in row.outputs.iter().zip(&spec.outputs).enumerate() {
            if !check_output(column, cell) {
                issues.push(I::InvalidOutputCell {
                    row: Some(r),
                    column: c,
                });
            }
        }
    }
    if let Some(default) = &spec.default_output {
        if default.len() != spec.outputs.len() {
            issues.push(I::DefaultArityMismatch);
        } else {
            for (c, (cell, column)) in default.iter().zip(&spec.outputs).enumerate() {
                if !check_output(column, cell) {
                    issues.push(I::InvalidOutputCell {
                        row: None,
                        column: c,
                    });
                }
            }
        }
    }
    issues.sort();
    issues.dedup();
    issues
}

// ============================================================================ intersections

/// A closed integer range; `None` is unbounded.
type IntRange = (Option<i128>, Option<i128>);

fn to_i128(d: Decimal) -> i128 {
    d.mantissa() / 10i128.pow(d.scale())
}

/// The integers of an interval (optionally clipped to a domain) as a closed range.
fn int_range(cell: &DecisionInputCell, domain: Option<&NumericDomain>) -> Option<IntRange> {
    let (mut lo, mut hi): IntRange = match cell {
        DecisionInputCell::Any => (None, None),
        DecisionInputCell::Interval { interval } => (
            interval.lower.map(|b| {
                let c = to_i128(b.value.ceil());
                if b.inclusive || b.value.fract() != Decimal::ZERO {
                    c
                } else {
                    c + 1
                }
            }),
            interval.upper.map(|b| {
                let f = to_i128(b.value.floor());
                if b.inclusive || b.value.fract() != Decimal::ZERO {
                    f
                } else {
                    f - 1
                }
            }),
        ),
        _ => return None,
    };
    if let Some(d) = domain {
        let (dl, dh) = (to_i128(d.lower.ceil()), to_i128(d.upper.floor()));
        lo = Some(lo.map_or(dl, |l| l.max(dl)));
        hi = Some(hi.map_or(dh, |h| h.min(dh)));
    }
    match (lo, hi) {
        (Some(l), Some(h)) if l > h => None,
        _ => Some((lo, hi)),
    }
}

fn int_intersect(a: IntRange, b: IntRange) -> bool {
    let lo = match (a.0, b.0) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, None) => x,
        (None, y) => y,
    };
    let hi = match (a.1, b.1) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    };
    !matches!((lo, hi), (Some(l), Some(h)) if l > h)
}

/// A continuous interval clipped to an optional closed domain; `None` when empty.
fn decimal_interval(
    cell: &DecisionInputCell,
    domain: Option<&NumericDomain>,
) -> Option<NumericInterval> {
    let mut interval = match cell {
        DecisionInputCell::Any => NumericInterval {
            lower: None,
            upper: None,
        },
        DecisionInputCell::Interval { interval } => *interval,
        _ => return None,
    };
    if let Some(d) = domain {
        let clip = NumericInterval {
            lower: Some(NumericBound {
                value: d.lower,
                inclusive: true,
            }),
            upper: Some(NumericBound {
                value: d.upper,
                inclusive: true,
            }),
        };
        interval = decimal_meet(&interval, &clip)?;
    }
    nonempty(interval)
}

fn nonempty(interval: NumericInterval) -> Option<NumericInterval> {
    match (interval.lower, interval.upper) {
        (Some(l), Some(u)) => match l.value.cmp(&u.value) {
            Ordering::Less => Some(interval),
            Ordering::Equal if l.inclusive && u.inclusive => Some(interval),
            _ => None,
        },
        _ => Some(interval),
    }
}

fn decimal_meet(a: &NumericInterval, b: &NumericInterval) -> Option<NumericInterval> {
    let lower = match (a.lower, b.lower) {
        (Some(x), Some(y)) => Some(match x.value.cmp(&y.value) {
            Ordering::Greater => x,
            Ordering::Less => y,
            Ordering::Equal => NumericBound {
                value: x.value,
                inclusive: x.inclusive && y.inclusive,
            },
        }),
        (x, None) => x,
        (None, y) => y,
    };
    let upper = match (a.upper, b.upper) {
        (Some(x), Some(y)) => Some(match x.value.cmp(&y.value) {
            Ordering::Less => x,
            Ordering::Greater => y,
            Ordering::Equal => NumericBound {
                value: x.value,
                inclusive: x.inclusive && y.inclusive,
            },
        }),
        (x, None) => x,
        (None, y) => y,
    };
    nonempty(NumericInterval { lower, upper })
}

fn enum_set<'a>(cell: &'a DecisionInputCell, domain: &'a [String]) -> BTreeSet<&'a String> {
    match cell {
        DecisionInputCell::EnumSet { members } => members.iter().collect(),
        _ => domain.iter().collect(),
    }
}

/// Whether two cells of one column share at least one input value.
fn cells_intersect(column: &DecisionColumn, a: &DecisionInputCell, b: &DecisionInputCell) -> bool {
    match &column.ty {
        DecisionType::Bool => match (a, b) {
            (DecisionInputCell::Bool { value: x }, DecisionInputCell::Bool { value: y }) => x == y,
            _ => true,
        },
        DecisionType::Enum { members } => {
            let (x, y) = (enum_set(a, members), enum_set(b, members));
            x.intersection(&y).next().is_some()
        }
        DecisionType::Int => {
            match (
                int_range(a, column.domain.as_ref()),
                int_range(b, column.domain.as_ref()),
            ) {
                (Some(x), Some(y)) => int_intersect(x, y),
                _ => false,
            }
        }
        DecisionType::Decimal { .. } => match (
            decimal_interval(a, column.domain.as_ref()),
            decimal_interval(b, column.domain.as_ref()),
        ) {
            (Some(x), Some(y)) => decimal_meet(&x, &y).is_some(),
            _ => false,
        },
        DecisionType::Unsupported { .. } => true,
    }
}

// ============================================================================ coverage

fn point_values(column: &DecisionColumn) -> Vec<DecisionPointValue> {
    match &column.ty {
        DecisionType::Bool => vec![
            DecisionPointValue::Bool { value: false },
            DecisionPointValue::Bool { value: true },
        ],
        DecisionType::Enum { members } => members
            .iter()
            .map(|m| DecisionPointValue::Enum { member: m.clone() })
            .collect(),
        _ => Vec::new(),
    }
}

fn cell_matches(cell: &DecisionInputCell, value: &DecisionPointValue) -> bool {
    match (cell, value) {
        (DecisionInputCell::Any, _) => true,
        (DecisionInputCell::Bool { value: x }, DecisionPointValue::Bool { value: y }) => x == y,
        (DecisionInputCell::EnumSet { members }, DecisionPointValue::Enum { member }) => {
            members.contains(member)
        }
        _ => false,
    }
}

fn finite_coverage(spec: &DecisionTableSpec) -> DecisionCoverage {
    let axes: Vec<Vec<DecisionPointValue>> = spec.inputs.iter().map(point_values).collect();
    let mut size: u64 = 1;
    for axis in &axes {
        size = size.saturating_mul(axis.len() as u64);
        if size > MAX_FINITE_DECISION_POINTS {
            return DecisionCoverage::NotAnalyzable {
                reason: NotAnalyzableReason::FiniteDomainTooLarge,
            };
        }
    }
    let mut witnesses = Vec::new();
    let mut point = vec![0usize; axes.len()];
    for _ in 0..size {
        let values: Vec<DecisionPointValue> = point
            .iter()
            .zip(&axes)
            .map(|(&i, axis)| axis[i].clone())
            .collect();
        let covered = spec.rows.iter().any(|row| {
            row.inputs
                .iter()
                .zip(&values)
                .all(|(cell, value)| cell_matches(cell, value))
        });
        if !covered && witnesses.len() < MAX_COVERAGE_WITNESSES {
            witnesses.push(values);
        }
        // Advance the last axis first (canonical lexicographic order).
        for k in (0..point.len()).rev() {
            point[k] += 1;
            if point[k] < axes[k].len() {
                break;
            }
            point[k] = 0;
        }
    }
    let uncovered = !witnesses.is_empty();
    if uncovered {
        DecisionCoverage::Incomplete { witnesses }
    } else {
        DecisionCoverage::Complete
    }
}

fn bound(value: Decimal, inclusive: bool) -> Option<NumericBound> {
    Some(NumericBound { value, inclusive })
}

fn int_coverage(spec: &DecisionTableSpec, domain: &NumericDomain) -> DecisionCoverage {
    let column = &spec.inputs[0];
    let (lo, hi) = (to_i128(domain.lower.ceil()), to_i128(domain.upper.floor()));
    let mut ranges: Vec<(i128, i128)> = spec
        .rows
        .iter()
        .filter_map(|row| int_range(&row.inputs[0], column.domain.as_ref()))
        .map(|(l, h)| (l.unwrap_or(lo), h.unwrap_or(hi)))
        .collect();
    ranges.sort();
    let mut gaps = Vec::new();
    let mut next = lo;
    for (l, h) in ranges {
        if l > next {
            gaps.push((next, l - 1));
        }
        next = next.max(h + 1);
    }
    if next <= hi {
        gaps.push((next, hi));
    }
    if gaps.is_empty() {
        DecisionCoverage::Complete
    } else {
        DecisionCoverage::IncompleteNumeric {
            gaps: gaps
                .into_iter()
                .map(|(l, h)| NumericInterval {
                    lower: bound(Decimal::from_i128_with_scale(l, 0), true),
                    upper: bound(Decimal::from_i128_with_scale(h, 0), true),
                })
                .collect(),
        }
    }
}

fn decimal_coverage(spec: &DecisionTableSpec, domain: &NumericDomain) -> DecisionCoverage {
    let column = &spec.inputs[0];
    let mut intervals: Vec<NumericInterval> = spec
        .rows
        .iter()
        .filter_map(|row| decimal_interval(&row.inputs[0], column.domain.as_ref()))
        .collect();
    // Clipped to the closed domain, every interval is bounded.
    let low = |i: &NumericInterval| i.lower.map(|b| (b.value, !b.inclusive));
    intervals.sort_by_key(low);
    let mut gaps = Vec::new();
    let mut cur = domain.lower;
    let mut cur_covered = false;
    for interval in intervals {
        let (Some(a), Some(b)) = (interval.lower, interval.upper) else {
            continue;
        };
        let gap = a.value > cur || (a.value == cur && !a.inclusive && !cur_covered);
        if gap {
            gaps.push(NumericInterval {
                lower: bound(cur, !cur_covered),
                upper: bound(a.value, !a.inclusive),
            });
        }
        match b.value.cmp(&cur) {
            Ordering::Greater => {
                cur = b.value;
                cur_covered = b.inclusive;
            }
            Ordering::Equal => cur_covered = cur_covered || b.inclusive,
            Ordering::Less => {}
        }
    }
    if cur < domain.upper || !cur_covered {
        gaps.push(NumericInterval {
            lower: bound(cur, !cur_covered),
            upper: bound(domain.upper, true),
        });
    }
    if gaps.is_empty() {
        DecisionCoverage::Complete
    } else {
        DecisionCoverage::IncompleteNumeric { gaps }
    }
}

fn coverage(spec: &DecisionTableSpec) -> DecisionCoverage {
    if spec.default_output.is_some() {
        return DecisionCoverage::CompleteByDefault;
    }
    let numeric = spec.inputs.iter().filter(|c| is_numeric(&c.ty)).count();
    let finite = spec.inputs.iter().filter(|c| is_finite(&c.ty)).count();
    if numeric == 0 {
        return finite_coverage(spec);
    }
    if numeric > 1 {
        return DecisionCoverage::NotAnalyzable {
            reason: NotAnalyzableReason::MultiDimensionalNumericDomain,
        };
    }
    if finite > 0 {
        return DecisionCoverage::NotAnalyzable {
            reason: NotAnalyzableReason::MixedNumericDomain,
        };
    }
    let column = &spec.inputs[0];
    let Some(domain) = &column.domain else {
        return DecisionCoverage::NotAnalyzable {
            reason: NotAnalyzableReason::UnboundedNumericDomain,
        };
    };
    match column.ty {
        DecisionType::Int => int_coverage(spec, domain),
        _ => decimal_coverage(spec, domain),
    }
}

// ============================================================================ analysis

/// Analyzes a typed decision table: structure, overlaps (illegal under UNIQUE) and coverage.
pub fn analyze_decision_table(spec: &DecisionTableSpec) -> DecisionTableAnalysis {
    let issues = structure(spec);
    if !issues.is_empty() {
        let unsupported_only = issues.iter().all(|i| {
            matches!(
                i,
                DecisionStructureIssue::UnsupportedInputDomain { .. }
                    | DecisionStructureIssue::UnsupportedOutputDomain { .. }
            )
        });
        return DecisionTableAnalysis {
            structure: issues,
            overlaps: Vec::new(),
            illegal_overlaps: Vec::new(),
            coverage: if unsupported_only {
                DecisionCoverage::NotAnalyzable {
                    reason: NotAnalyzableReason::UnsupportedDomain,
                }
            } else {
                DecisionCoverage::StructureInvalid
            },
        };
    }
    let mut overlaps = Vec::new();
    for (i, a) in spec.rows.iter().enumerate() {
        for (j, b) in spec.rows.iter().enumerate().skip(i + 1) {
            let intersect = spec
                .inputs
                .iter()
                .enumerate()
                .all(|(c, column)| cells_intersect(column, &a.inputs[c], &b.inputs[c]));
            if intersect {
                overlaps.push(DecisionOverlap {
                    left_row: i,
                    right_row: j,
                });
            }
        }
    }
    let illegal_overlaps = match HitPolicy::parse(&spec.hit_policy) {
        Some(HitPolicy::Unique) => overlaps.clone(),
        _ => Vec::new(),
    };
    DecisionTableAnalysis {
        structure: Vec::new(),
        overlaps,
        illegal_overlaps,
        coverage: coverage(spec),
    }
}
