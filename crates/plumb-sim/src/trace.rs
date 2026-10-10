//! Scenario trace vocabulary, ScenarioRun material and canonical scenario-trace artifact
//! material (plan S4.3, Hotfix 052).
//!
//! The composite trace keeps every S4.2 execution step unchanged and appends typed comparison
//! steps. The artifact body is serialized as RFC 8785 canonical JSON and hashed with SHA-256;
//! persisting the ScenarioRun or the artifact is the caller's concern, so nothing here performs
//! storage or I/O.

use plumb_core::{to_canonical_json, Hash, Id};
use plumb_psg::{ScenarioRun, ScenarioRunResult};
use serde::{Deserialize, Serialize};

use crate::compare::{ComparisonError, ScenarioDiff};
use crate::model::{ExecutionTraceStep, MissingSemantic, ResolutionSource};

/// Version of the scenario-trace artifact body.
pub const SCENARIO_TRACE_VERSION: u32 = 1;

/// The final verdict of one scenario comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioVerdict {
    Pass,
    Fail,
    Undecidable,
}

impl ScenarioVerdict {
    /// The exact PSG ScenarioRun result.
    pub fn run_result(self) -> ScenarioRunResult {
        match self {
            ScenarioVerdict::Pass => ScenarioRunResult::Pass,
            ScenarioVerdict::Fail => ScenarioRunResult::Fail,
            ScenarioVerdict::Undecidable => ScenarioRunResult::Undecidable,
        }
    }
}

/// One typed comparison step, in comparison order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum ComparisonTraceStep {
    ComparisonStarted {
        scenario_ref: Id,
    },
    ExpectedValueResolved {
        owner_ref: Id,
        key: String,
        source: ResolutionSource,
    },
    ExpectedSemanticMissing {
        missing: Vec<MissingSemantic>,
    },
    AttributeCompared {
        attribute_ref: Id,
        matched: bool,
    },
    StateCompared {
        state_ref: Id,
        matched: bool,
    },
    OutcomesCompared {
        anchor_ref: Id,
        matched: bool,
    },
    EventsCompared {
        anchor_ref: Id,
        matched: bool,
    },
    RuleCompared {
        rule_ref: Id,
        matched: bool,
    },
    DecisionOutputCompared {
        decision_table_ref: Id,
        column: u32,
        matched: bool,
    },
    CalculationCompared {
        calculation_ref: Id,
        matched: bool,
    },
    ComparisonCompleted {
        result: ScenarioVerdict,
        diff_count: u32,
    },
}

/// One step of the composite scenario trace: an unchanged execution step or a comparison step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "phase", content = "step", rename_all = "snake_case")]
pub enum ScenarioTraceStep {
    Execution(ExecutionTraceStep),
    Comparison(ComparisonTraceStep),
}

/// The versioned scenario-trace artifact body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScenarioTraceArtifactBody {
    pub version: u32,
    pub model_semantic_hash: Hash,
    pub scenario_ref: Id,
    pub specification_hash: Hash,
    pub result: ScenarioRunResult,
    pub trace: Vec<ScenarioTraceStep>,
    pub diffs: Vec<ScenarioDiff>,
}

/// The artifact material: the body, its canonical bytes and the SHA-256 of exactly those bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioTraceArtifact {
    pub body: ScenarioTraceArtifactBody,
    pub bytes: Vec<u8>,
    pub hash: Hash,
}

fn serialization(e: impl ToString) -> ComparisonError {
    ComparisonError::TraceSerialization {
        reason: e.to_string(),
    }
}

impl ScenarioTraceArtifact {
    /// Canonical RFC 8785 bytes of the body and their Generic SHA-256; the hash is not part of
    /// the hashed body.
    pub fn from_body(body: ScenarioTraceArtifactBody) -> Result<Self, ComparisonError> {
        let bytes = to_canonical_json(&body).map_err(serialization)?;
        let hash = Hash::content_sha256(&bytes);
        Ok(ScenarioTraceArtifact { body, bytes, hash })
    }
}

/// The existing PSG ScenarioRun payload of an artifact body: each trace step serialized to its
/// closed JSON object. No node, ID or clock value is created.
pub fn scenario_run(body: &ScenarioTraceArtifactBody) -> Result<ScenarioRun, ComparisonError> {
    let trace = body
        .trace
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(serialization)?;
    Ok(ScenarioRun {
        scenario_ref: body.scenario_ref.clone(),
        specification_hash: body.specification_hash.clone(),
        result: body.result,
        trace,
    })
}
