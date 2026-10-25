//! Typed decision-table analysis (plan S2.6; compiler architecture §8).
//!
//! The one implementation lives in `plumb_validation::decision_table_analysis` (Hotfix 044), so
//! S2.10 validation reuses it without a crate cycle; this module keeps the S2.6 public paths.

pub use plumb_validation::decision_table_analysis::*;

use plumb_psg::DecisionTable;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

/// Which part of a persisted DecisionTable is malformed (Hotfix 065).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecisionTableEncodingError {
    #[error("input column {index}: {reason}")]
    InputColumn { index: usize, reason: String },
    #[error("output column {index}: {reason}")]
    OutputColumn { index: usize, reason: String },
    #[error("row {index}: {reason}")]
    Row { index: usize, reason: String },
    #[error("default output is not part of the pilot PSG encoding")]
    DefaultOutput,
}

/// Why one output cell cannot be set (Hotfix 065).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecisionCellError {
    #[error("row {row} is out of range")]
    RowOutOfRange { row: usize },
    #[error("output column {column} is out of range")]
    ColumnOutOfRange { column: usize },
    #[error("the cell does not fit output column {column}")]
    InvalidOutputCell { column: usize },
    #[error("the cell already has this outcome")]
    NoChange,
    #[error("uncovered point {witness} does not exist")]
    WitnessOutOfRange { witness: usize },
    #[error("only a single-output table can cover a point with one cell")]
    NotSingleOutput,
}

/// Decodes one persisted element and requires it to be exactly the serde encoding of the typed
/// value: anything else (unknown fields, another number spelling, a missing field) is malformed.
fn strict<T: DeserializeOwned + Serialize>(value: &Value) -> Result<T, String> {
    let typed: T = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    let encoded = serde_json::to_value(&typed).map_err(|e| e.to_string())?;
    if &encoded == value {
        Ok(typed)
    } else {
        Err("not the exact typed encoding".to_owned())
    }
}

/// The one strict adapter from the pilot PSG DecisionTable encoding to the typed
/// [`DecisionTableSpec`] (Hotfix 065): `hit_policy` as stored, `inputs[i]`, `outputs[i]` and
/// `rows[i]` the serde values of [`DecisionColumn`] and [`DecisionRow`], and no default output.
/// Malformed material is an error, never skipped or emptied.
pub fn decode_decision_table(
    table: &DecisionTable,
) -> Result<DecisionTableSpec, DecisionTableEncodingError> {
    let inputs = table
        .inputs
        .iter()
        .enumerate()
        .map(|(index, v)| {
            strict(v).map_err(|reason| DecisionTableEncodingError::InputColumn { index, reason })
        })
        .collect::<Result<Vec<DecisionColumn>, _>>()?;
    let outputs = table
        .outputs
        .iter()
        .enumerate()
        .map(|(index, v)| {
            strict(v).map_err(|reason| DecisionTableEncodingError::OutputColumn { index, reason })
        })
        .collect::<Result<Vec<DecisionColumn>, _>>()?;
    let rows = table
        .rows
        .iter()
        .enumerate()
        .map(|(index, v)| {
            strict(v).map_err(|reason| DecisionTableEncodingError::Row { index, reason })
        })
        .collect::<Result<Vec<DecisionRow>, _>>()?;
    Ok(DecisionTableSpec {
        hit_policy: table.hit_policy.clone(),
        inputs,
        outputs,
        rows,
        default_output: None,
    })
}

/// Encodes `spec` as the PSG DecisionTable payload named `name`; the inverse of
/// [`decode_decision_table`]. A default output has no PSG encoding.
pub fn encode_decision_table(
    name: &str,
    spec: &DecisionTableSpec,
) -> Result<DecisionTable, DecisionTableEncodingError> {
    if spec.default_output.is_some() {
        return Err(DecisionTableEncodingError::DefaultOutput);
    }
    let value = |v: Result<Value, serde_json::Error>,
                 wrap: &dyn Fn(String) -> DecisionTableEncodingError| {
        v.map_err(|e| wrap(e.to_string()))
    };
    let mut inputs = Vec::with_capacity(spec.inputs.len());
    for (index, column) in spec.inputs.iter().enumerate() {
        inputs.push(value(serde_json::to_value(column), &|reason| {
            DecisionTableEncodingError::InputColumn { index, reason }
        })?);
    }
    let mut outputs = Vec::with_capacity(spec.outputs.len());
    for (index, column) in spec.outputs.iter().enumerate() {
        outputs.push(value(serde_json::to_value(column), &|reason| {
            DecisionTableEncodingError::OutputColumn { index, reason }
        })?);
    }
    let mut rows = Vec::with_capacity(spec.rows.len());
    for (index, row) in spec.rows.iter().enumerate() {
        rows.push(value(serde_json::to_value(row), &|reason| {
            DecisionTableEncodingError::Row { index, reason }
        })?);
    }
    Ok(DecisionTable {
        name: name.to_owned(),
        hit_policy: spec.hit_policy.clone(),
        inputs,
        outputs,
        rows,
    })
}

/// Replaces exactly the output cell `(row, column)` (zero-based) with `cell`; no row, input cell
/// or column changes. The cell must fit its output column by the existing structural analysis.
pub fn set_output_cell(
    spec: &DecisionTableSpec,
    row: usize,
    column: usize,
    cell: DecisionOutputCell,
) -> Result<DecisionTableSpec, DecisionCellError> {
    let current = spec
        .rows
        .get(row)
        .ok_or(DecisionCellError::RowOutOfRange { row })?;
    if column >= spec.outputs.len() || column >= current.outputs.len() {
        return Err(DecisionCellError::ColumnOutOfRange { column });
    }
    if current.outputs[column] == cell {
        return Err(DecisionCellError::NoChange);
    }
    let mut candidate = spec.clone();
    candidate.rows[row].outputs[column] = cell;
    let analysis = analyze_decision_table(&candidate);
    if analysis.structure.iter().any(|issue| {
        matches!(issue, DecisionStructureIssue::InvalidOutputCell { row: Some(r), column: c }
            if *r == row && *c == column)
    }) {
        return Err(DecisionCellError::InvalidOutputCell { column });
    }
    Ok(candidate)
}

/// Covers the uncovered finite point `witness` (zero-based, in the canonical order of
/// [`DecisionCoverage::Incomplete`]) of a single-output table with one new row: its inputs are
/// exactly the point's values (an enum member as a one-member set) and its output is `cell`.
/// Existing rows never change.
pub fn cover_uncovered_point(
    spec: &DecisionTableSpec,
    witness: usize,
    column: usize,
    cell: DecisionOutputCell,
) -> Result<DecisionTableSpec, DecisionCellError> {
    if spec.outputs.len() != 1 {
        return Err(DecisionCellError::NotSingleOutput);
    }
    if column != 0 {
        return Err(DecisionCellError::ColumnOutOfRange { column });
    }
    let point = match analyze_decision_table(spec).coverage {
        DecisionCoverage::Incomplete { witnesses } => witnesses.into_iter().nth(witness),
        _ => None,
    }
    .ok_or(DecisionCellError::WitnessOutOfRange { witness })?;
    let inputs = point
        .into_iter()
        .map(|value| match value {
            DecisionPointValue::Bool { value } => DecisionInputCell::Bool { value },
            DecisionPointValue::Enum { member } => DecisionInputCell::EnumSet {
                members: vec![member],
            },
        })
        .collect();
    let mut candidate = spec.clone();
    candidate.rows.push(DecisionRow {
        inputs,
        outputs: vec![cell],
    });
    let row = candidate.rows.len() - 1;
    let analysis = analyze_decision_table(&candidate);
    if analysis.structure.iter().any(|issue| {
        matches!(issue, DecisionStructureIssue::InvalidOutputCell { row: Some(r), column: 0 }
            if *r == row)
    }) {
        return Err(DecisionCellError::InvalidOutputCell { column: 0 });
    }
    Ok(candidate)
}
