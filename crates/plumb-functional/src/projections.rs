//! Functional pilot projections and reports (plan S4.7, Hotfix 056).
//!
//! [`project_functional_pilot`] materializes exactly six deterministic artifacts from one graph,
//! its validation profile and a supplied, current F4 GateReport: functional.yaml (the unchanged
//! F0.14 projection), glossary.md, scenarios.yaml, decisions.jsonl, model-report.md and
//! gate-report.json. Each accompanies the source semantic hash and the hash of its exact bytes.
//! Only Accepted semantic truth is projected; a failed gate is exported as failed. Nothing here
//! evaluates a gate, computes readiness, writes a file, reads time or mutates the graph.

use plumb_core::{to_canonical_json, CoreError, GateId, Hash, Id, Timestamp};
use plumb_psg::{ElementStatus, Graph, NodePayload, ScenarioKind};
use plumb_validation::{EvaluationError, GateReport, ValidationProfile};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::model::ProjectionMetadata;
use crate::project::{project_functional_v2, ProjectionError};
use crate::report::{render_glossary, render_model_report, ModelReportInput};

/// Version of the scenarios.yaml document.
pub const SCENARIO_PROJECTION_VERSION: u32 = 1;

/// The six functional pilot artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionArtifactKind {
    FunctionalYaml,
    GlossaryMarkdown,
    ScenariosYaml,
    DecisionsJsonl,
    ModelReportMarkdown,
    GateReportJson,
}

impl ProjectionArtifactKind {
    /// Every kind, in the fixed artifact order.
    pub const ALL: [ProjectionArtifactKind; 6] = [
        ProjectionArtifactKind::FunctionalYaml,
        ProjectionArtifactKind::GlossaryMarkdown,
        ProjectionArtifactKind::ScenariosYaml,
        ProjectionArtifactKind::DecisionsJsonl,
        ProjectionArtifactKind::ModelReportMarkdown,
        ProjectionArtifactKind::GateReportJson,
    ];

    /// The stable file name (a label only; nothing is written).
    pub fn file_name(self) -> &'static str {
        match self {
            ProjectionArtifactKind::FunctionalYaml => "functional.yaml",
            ProjectionArtifactKind::GlossaryMarkdown => "glossary.md",
            ProjectionArtifactKind::ScenariosYaml => "scenarios.yaml",
            ProjectionArtifactKind::DecisionsJsonl => "decisions.jsonl",
            ProjectionArtifactKind::ModelReportMarkdown => "model-report.md",
            ProjectionArtifactKind::GateReportJson => "gate-report.json",
        }
    }

    /// The stable media type.
    pub fn media_type(self) -> &'static str {
        match self {
            ProjectionArtifactKind::FunctionalYaml | ProjectionArtifactKind::ScenariosYaml => {
                "application/yaml"
            }
            ProjectionArtifactKind::GlossaryMarkdown
            | ProjectionArtifactKind::ModelReportMarkdown => "text/markdown; charset=utf-8",
            ProjectionArtifactKind::DecisionsJsonl => "application/x-ndjson",
            ProjectionArtifactKind::GateReportJson => "application/json",
        }
    }
}

/// One artifact: its exact bytes, the source semantic hash and the SHA-256 of the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionArtifact {
    pub kind: ProjectionArtifactKind,
    pub bytes: Vec<u8>,
    pub source_semantic_hash: Hash,
    pub content_hash: Hash,
}

impl ProjectionArtifact {
    fn new(kind: ProjectionArtifactKind, bytes: Vec<u8>, source: &Hash) -> ProjectionArtifact {
        ProjectionArtifact {
            kind,
            content_hash: Hash::content_sha256(&bytes),
            bytes,
            source_semantic_hash: source.clone(),
        }
    }
}

/// The six functional pilot artifacts and the F0.14 companion metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionalPilotOutputs {
    pub functional_yaml: ProjectionArtifact,
    pub glossary_md: ProjectionArtifact,
    pub scenarios_yaml: ProjectionArtifact,
    pub decisions_jsonl: ProjectionArtifact,
    pub model_report_md: ProjectionArtifact,
    pub gate_report_json: ProjectionArtifact,
    pub functional_metadata: ProjectionMetadata,
}

impl FunctionalPilotOutputs {
    /// The six artifacts in the fixed order.
    pub fn artifacts(&self) -> [&ProjectionArtifact; 6] {
        [
            &self.functional_yaml,
            &self.glossary_md,
            &self.scenarios_yaml,
            &self.decisions_jsonl,
            &self.model_report_md,
            &self.gate_report_json,
        ]
    }
}

/// One Accepted Scenario: its node ID and its exact payload fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioProjectionEntry {
    pub id: Id,
    pub name: String,
    pub scenario_kind: ScenarioKind,
    pub given: Vec<Value>,
    pub when: Value,
    pub then: Vec<Value>,
    pub requirement_refs: Option<Vec<Id>>,
    pub derived_from_refs: Option<Vec<Id>>,
    pub confirmation_status: Option<String>,
}

/// scenarios.yaml.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioProjectionDocument {
    pub version: u32,
    pub model_hash: Hash,
    pub scenarios: Vec<ScenarioProjectionEntry>,
}

/// One decisions.jsonl line: an Accepted ResolutionDecision's node ID and exact payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionProjectionEntry {
    pub id: Id,
    pub question_ref: Option<Id>,
    pub proposal_ref: Option<Id>,
    pub answer: Value,
    pub decided_by: Id,
    pub decided_at: Timestamp,
    pub patch_ref: Hash,
    pub rationale: Option<String>,
    pub supersedes: Option<Id>,
}

/// Which GateReport identity binding failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateReportBinding {
    SemanticHash,
    EvidenceHash,
    ProfileId,
    ProfileHash,
    RulePackHash,
}

/// Why the functional pilot artifacts could not be produced. A failed gate is not an error.
#[derive(Debug, Error)]
pub enum PilotProjectionError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("functional projection: {0}")]
    FunctionalProjection(#[from] ProjectionError),
    #[error("invalid gate report: {0}")]
    GateReport(EvaluationError),
    #[error("gate report is for {actual}, not F4")]
    WrongGate { actual: GateId },
    #[error("gate report {binding:?} is not the current baseline")]
    StaleGateReport { binding: GateReportBinding },
    #[error("gate report {binding:?} does not match the profile")]
    ProfileMismatch { binding: GateReportBinding },
    #[error("{artifact:?} serialization: {reason}")]
    Serialization {
        artifact: ProjectionArtifactKind,
        reason: String,
    },
}

/// Requires a valid F4 report bound to the current graph and profile identity.
fn bind_gate_report(
    graph: &Graph,
    profile: &ValidationProfile,
    report: &GateReport,
    semantic_hash: &Hash,
    evidence_hash: &Hash,
) -> Result<(), PilotProjectionError> {
    if report.gate != GateId::F4 {
        return Err(PilotProjectionError::WrongGate {
            actual: report.gate,
        });
    }
    report
        .validate()
        .map_err(PilotProjectionError::GateReport)?;
    let stale = |binding| PilotProjectionError::StaleGateReport { binding };
    let mismatch = |binding| PilotProjectionError::ProfileMismatch { binding };
    if &report.baseline_semantic_hash != semantic_hash {
        return Err(stale(GateReportBinding::SemanticHash));
    }
    if &report.baseline_evidence_hash != evidence_hash {
        return Err(stale(GateReportBinding::EvidenceHash));
    }
    if report.profile_id != profile.profile_id || graph.profile_id() != &profile.profile_id {
        return Err(mismatch(GateReportBinding::ProfileId));
    }
    if report.profile_hash != profile.profile_hash()? {
        return Err(mismatch(GateReportBinding::ProfileHash));
    }
    if report.rule_pack_hash != profile.rule_pack_hash()? {
        return Err(mismatch(GateReportBinding::RulePackHash));
    }
    Ok(())
}

fn sorted_unique(refs: &Option<Vec<Id>>) -> Option<Vec<Id>> {
    refs.as_ref().map(|r| {
        let mut r = r.clone();
        r.sort();
        r.dedup();
        r
    })
}

/// The Accepted Scenarios in node-ID order.
fn scenario_document(graph: &Graph, model_hash: &Hash) -> ScenarioProjectionDocument {
    let scenarios = graph
        .nodes()
        .iter()
        .filter(|(_, n)| n.status == ElementStatus::Accepted)
        .filter_map(|(id, n)| match &n.payload {
            NodePayload::Scenario(s) => Some(ScenarioProjectionEntry {
                id: id.clone(),
                name: s.name.clone(),
                scenario_kind: s.scenario_kind,
                given: s.given.clone(),
                when: s.when.clone(),
                then: s.then.clone(),
                requirement_refs: sorted_unique(&s.requirement_refs),
                derived_from_refs: sorted_unique(&s.derived_from_refs),
                confirmation_status: s.confirmation_status.clone(),
            }),
            _ => None,
        })
        .collect();
    ScenarioProjectionDocument {
        version: SCENARIO_PROJECTION_VERSION,
        model_hash: model_hash.clone(),
        scenarios,
    }
}

/// The Accepted ResolutionDecisions in node-ID order.
fn decision_entries(graph: &Graph) -> Vec<DecisionProjectionEntry> {
    graph
        .nodes()
        .iter()
        .filter(|(_, n)| n.status == ElementStatus::Accepted)
        .filter_map(|(id, n)| match &n.payload {
            NodePayload::ResolutionDecision(d) => Some(DecisionProjectionEntry {
                id: id.clone(),
                question_ref: d.question_ref.clone(),
                proposal_ref: d.proposal_ref.clone(),
                answer: d.answer.clone(),
                decided_by: d.decided_by.clone(),
                decided_at: d.decided_at,
                patch_ref: d.patch_ref.clone(),
                rationale: d.rationale.clone(),
                supersedes: d.supersedes.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Materializes the six functional pilot artifacts for `graph` with its current F4 report.
pub fn project_functional_pilot(
    graph: &Graph,
    profile: &ValidationProfile,
    f4_gate_report: &GateReport,
) -> Result<FunctionalPilotOutputs, PilotProjectionError> {
    let semantic_hash = graph.semantic_hash()?;
    let evidence_hash = graph.evidence_hash()?;
    bind_gate_report(
        graph,
        profile,
        f4_gate_report,
        &semantic_hash,
        &evidence_hash,
    )?;
    let artifact = |kind, bytes: Vec<u8>| ProjectionArtifact::new(kind, bytes, &semantic_hash);
    let serialization = |artifact: ProjectionArtifactKind| {
        move |e: &dyn std::fmt::Display| PilotProjectionError::Serialization {
            artifact,
            reason: e.to_string(),
        }
    };

    let functional = project_functional_v2(graph, profile)?;

    let scenarios = serde_yaml::to_string(&scenario_document(graph, &semantic_hash))
        .map_err(|e| serialization(ProjectionArtifactKind::ScenariosYaml)(&e))?;

    let mut decisions = Vec::new();
    for entry in decision_entries(graph) {
        decisions.extend(to_canonical_json(&entry)?);
        decisions.push(b'\n');
    }

    let gate_bytes = to_canonical_json(f4_gate_report)?;
    let gate_report_json = artifact(ProjectionArtifactKind::GateReportJson, gate_bytes);
    let model_report = render_model_report(
        graph,
        &ModelReportInput {
            gate_report: f4_gate_report,
            gate_report_hash: &gate_report_json.content_hash,
            functional: &functional.metadata,
            evidence_hash: &evidence_hash,
            semantic_hash: &semantic_hash,
        },
    );

    Ok(FunctionalPilotOutputs {
        functional_yaml: artifact(ProjectionArtifactKind::FunctionalYaml, functional.yaml),
        glossary_md: artifact(
            ProjectionArtifactKind::GlossaryMarkdown,
            render_glossary(graph).into_bytes(),
        ),
        scenarios_yaml: artifact(
            ProjectionArtifactKind::ScenariosYaml,
            scenarios.into_bytes(),
        ),
        decisions_jsonl: artifact(ProjectionArtifactKind::DecisionsJsonl, decisions),
        model_report_md: artifact(
            ProjectionArtifactKind::ModelReportMarkdown,
            model_report.into_bytes(),
        ),
        gate_report_json,
        functional_metadata: functional.metadata,
    })
}
