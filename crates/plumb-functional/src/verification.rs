//! Minimal VerificationObligation applicability for F4 (plan S4.5, Hotfix 054).
//!
//! S4.5 proposes deterministic VerificationObligation nodes with `verified_by` and
//! `implemented_as` links so F4 can distinguish executable sources from sources that are
//! verifiable by other means. Scope, executability and blocking status are explicit stage input;
//! a requirement's kind never chooses a method. A non-executable requirement uses the method
//! already recorded in its Accepted payload; missing, unknown, deferred or incompatible methods
//! are reported as dispositions. The only automatically linked implementations are existing
//! Accepted, Confirmed Scenarios. Nothing here plans D2 verification, creates test, check,
//! scenario, question or decision nodes, rewrites accepted material, reads a clock or persists
//! anything; `apply_patch` only dry-validates proposals in memory.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AuditMeta, Edge, ElementStatus, EvidenceRef, Graph, Node, NodePayload, NodeType, RelationKind,
    RelationProperties, RequirementKind,
};
pub use plumb_psg::{VerificationKind, VerificationObligation};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

use crate::resolution::relation_id;
use crate::scenario::{validate_scenario, ScenarioWhen, SCENARIO_CONFIRMED};

const VERIFICATION_STAGE: StageId = StageId::S4;

// ============================================================================ inputs

/// How a source is verified in the current F4/profile evaluation. Stage input only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationSourceMode {
    ExecutableScenario,
    NonExecutable,
}

/// One implementation-scoped Requirement or Operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationSourceInput {
    pub source_ref: Id,
    pub mode: VerificationSourceMode,
    /// Blocking under the current resolved profile context; never persisted.
    pub blocking: bool,
}

/// The explicit applicability input of one invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationApplicabilityInputs {
    pub sources: Vec<VerificationSourceInput>,
}

/// Caller-supplied audit of proposed material; outside every identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

// ============================================================================ results

/// The outcome of one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum VerificationDisposition {
    Proposed { proposal_ref: Id },
    Existing,
    MethodDecisionRequired,
    VerificationKindDeferred,
    IncompatibleVerificationMethod,
    UnsupportedVerificationMethod,
}

/// The analysis of one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerificationObligationAnalysis {
    pub source_ref: Id,
    pub mode: VerificationSourceMode,
    pub obligation_ref: Option<Id>,
    pub verification_kind: Option<VerificationKind>,
    /// Covering Accepted, Confirmed Scenarios, sorted.
    pub scenario_refs: Vec<Id>,
    pub blocking: bool,
    pub disposition: VerificationDisposition,
}

/// Read-only F4 applicability material; not PSG and not persisted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct VerificationApplicabilitySnapshot {
    pub source_refs: BTreeSet<Id>,
    pub executable_source_refs: BTreeSet<Id>,
    pub blocking_obligation_refs: BTreeSet<Id>,
    /// The value orchestration may hand to S4.4 as its blocking scenario policy.
    pub blocking_scenario_refs: BTreeSet<Id>,
}

/// Every analysis, the per-source proposals and the applicability snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct VerificationAnalysisResult {
    /// source_ref order.
    pub items: Vec<VerificationObligationAnalysis>,
    /// source_ref order.
    pub proposals: Vec<Proposal>,
    pub applicability: VerificationApplicabilitySnapshot,
}

/// Malformed input or graph material; method gaps are dispositions, not errors.
#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("duplicate verification source {source_ref}")]
    DuplicateSource { source_ref: Id },
    #[error("invalid verification source {source_ref}: {reason}")]
    InvalidSource { source_ref: Id, reason: String },
    #[error("invalid applicability of {source_ref}: {reason}")]
    InvalidApplicability { source_ref: Id, reason: String },
    #[error("verification obligation {obligation_ref} collides: {reason}")]
    VerificationIdCollision { obligation_ref: Id, reason: String },
    #[error("verification relation {edge_ref} collides: {reason}")]
    VerificationEdgeCollision { edge_ref: Id, reason: String },
    #[error("invalid scenario {scenario_ref}: {reason}")]
    InvalidScenario { scenario_ref: Id, reason: String },
    #[error("invalid audit: {reason}")]
    InvalidAudit { reason: String },
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

// ============================================================================ identity

fn kind_text(kind: VerificationKind) -> String {
    match serde_json::to_value(kind) {
        Ok(JsonValue::String(s)) => s,
        _ => unreachable!("VerificationKind serializes to its wire string"),
    }
}

/// `verification_obligation:<first 16 hex of SHA-256(RFC 8785 {project_id, source_ref,
/// verification_kind, node_type: verification_obligation})>`.
pub fn verification_obligation_id(
    project_id: &Id,
    source_ref: &Id,
    verification_kind: VerificationKind,
) -> Result<Id, CoreError> {
    let body = json!({
        "project_id": project_id,
        "source_ref": source_ref,
        "verification_kind": kind_text(verification_kind),
        "node_type": "verification_obligation",
    });
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("verification_obligation:{}", &hex[..16]).parse()
}

/// The exact non-executable method vocabulary of the pilot.
fn non_executable_kind(method: &str) -> Option<VerificationKind> {
    Some(match method {
        "analysis" => VerificationKind::Analysis,
        "inspection" => VerificationKind::Inspection,
        "review" => VerificationKind::Review,
        "demonstration" => VerificationKind::Demonstration,
        "formal_check" => VerificationKind::FormalCheck,
        "architecture_check" => VerificationKind::ArchitectureCheck,
        "security_check" => VerificationKind::SecurityCheck,
        _ => return None,
    })
}

// ============================================================================ coverage

fn active(status: ElementStatus) -> bool {
    matches!(status, ElementStatus::Proposed | ElementStatus::Accepted)
}

/// One Accepted, Confirmed Scenario: its When anchor and traceability.
struct CoverageCandidate {
    scenario_ref: Id,
    operation_anchor: Option<Id>,
    requirement_refs: BTreeSet<Id>,
    derived_from_refs: BTreeSet<Id>,
    evidence: Vec<EvidenceRef>,
}

fn confirmed_scenarios(graph: &Graph) -> Result<Vec<CoverageCandidate>, VerificationError> {
    let mut out = Vec::new();
    for id in graph.node_ids_by_type(NodeType::Scenario) {
        let Some(node) = graph.node(id) else { continue };
        let NodePayload::Scenario(scenario) = &node.payload else {
            continue;
        };
        if node.status != ElementStatus::Accepted
            || scenario.confirmation_status.as_deref() != Some(SCENARIO_CONFIRMED)
        {
            continue;
        }
        let semantics =
            validate_scenario(graph, scenario).map_err(|e| VerificationError::InvalidScenario {
                scenario_ref: id.clone(),
                reason: e.to_string(),
            })?;
        out.push(CoverageCandidate {
            scenario_ref: id.clone(),
            operation_anchor: match semantics.when {
                ScenarioWhen::Operation(o) => Some(o),
                _ => None,
            },
            requirement_refs: scenario
                .requirement_refs
                .iter()
                .flatten()
                .cloned()
                .collect(),
            derived_from_refs: scenario
                .derived_from_refs
                .iter()
                .flatten()
                .cloned()
                .collect(),
            evidence: node.evidence.clone(),
        });
    }
    Ok(out)
}

fn covers_operation(s: &CoverageCandidate, operation_ref: &Id) -> bool {
    s.operation_anchor.as_ref() == Some(operation_ref)
        || s.derived_from_refs.contains(operation_ref)
}

/// Accepted Operations an Accepted Requirement is specified by through Accepted `specified_by`.
fn specifying_operations(graph: &Graph, requirement_ref: &Id) -> BTreeSet<Id> {
    graph
        .outgoing_edge_ids(requirement_ref)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == RelationKind::SpecifiedBy && e.status == ElementStatus::Accepted)
        .filter(|e| {
            graph.node(&e.to).is_some_and(|n| {
                n.status == ElementStatus::Accepted && n.payload.node_type() == NodeType::Operation
            })
        })
        .map(|e| e.to.clone())
        .collect()
}

// ============================================================================ compilation

/// A validated source: its node and its resolved verification kind or method gap.
enum Resolution {
    Kind(VerificationKind, Option<String>),
    Gap(VerificationDisposition),
}

fn resolve<'g>(
    graph: &'g Graph,
    input: &VerificationSourceInput,
) -> Result<(&'g Node, Resolution), VerificationError> {
    let source_ref = &input.source_ref;
    let invalid_source = |reason: &str| VerificationError::InvalidSource {
        source_ref: source_ref.clone(),
        reason: reason.to_owned(),
    };
    let invalid_applicability = |reason: &str| VerificationError::InvalidApplicability {
        source_ref: source_ref.clone(),
        reason: reason.to_owned(),
    };
    let node = graph
        .node(source_ref)
        .ok_or_else(|| invalid_source("unknown node"))?;
    if node.status != ElementStatus::Accepted {
        return Err(invalid_source("not Accepted"));
    }
    let resolution = match (&node.payload, input.mode) {
        (NodePayload::Operation(_), VerificationSourceMode::ExecutableScenario) => {
            Resolution::Kind(VerificationKind::Scenario, None)
        }
        (NodePayload::Operation(_), VerificationSourceMode::NonExecutable) => {
            return Err(invalid_applicability(
                "an Operation is verified only by executable scenarios in the pilot",
            ))
        }
        (NodePayload::Requirement(r), VerificationSourceMode::ExecutableScenario) => {
            if r.requirement_kind != RequirementKind::Functional {
                return Err(invalid_applicability(
                    "only a functional Requirement may be executable through scenarios",
                ));
            }
            Resolution::Kind(VerificationKind::Scenario, None)
        }
        (NodePayload::Requirement(r), VerificationSourceMode::NonExecutable) => {
            match r.verification_method.as_deref() {
                None => Resolution::Gap(VerificationDisposition::MethodDecisionRequired),
                Some("test") => Resolution::Gap(VerificationDisposition::VerificationKindDeferred),
                Some("scenario") => {
                    Resolution::Gap(VerificationDisposition::IncompatibleVerificationMethod)
                }
                Some(method) => match non_executable_kind(method) {
                    Some(kind) => Resolution::Kind(kind, Some(method.to_owned())),
                    None => Resolution::Gap(VerificationDisposition::UnsupportedVerificationMethod),
                },
            }
        }
        _ => return Err(invalid_source("not a Requirement or Operation")),
    };
    Ok((node, resolution))
}

fn sorted_unique(refs: impl IntoIterator<Item = EvidenceRef>) -> Vec<EvidenceRef> {
    refs.into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The deterministic edge at its relation ID: reused when it is the exact active relation,
/// proposed when absent, a collision otherwise.
fn plan_edge(
    graph: &Graph,
    kind: RelationKind,
    from: &Id,
    to: &Id,
    evidence: Vec<EvidenceRef>,
    meta: &AuditMeta,
) -> Result<Option<Edge>, VerificationError> {
    let id = relation_id(&kind, from, to)?;
    if let Some(existing) = graph.edge(&id) {
        if existing.kind == kind
            && &existing.from == from
            && &existing.to == to
            && active(existing.status)
        {
            return Ok(None);
        }
        return Err(VerificationError::VerificationEdgeCollision {
            edge_ref: id,
            reason: "a different or inactive relation holds the deterministic ID".to_owned(),
        });
    }
    Ok(Some(Edge {
        id,
        revision: 1,
        status: ElementStatus::Proposed,
        kind,
        from: from.clone(),
        to: to.clone(),
        properties: RelationProperties::None,
        evidence,
        derivations: Vec::new(),
        standards: Vec::new(),
        audit: meta.clone(),
    }))
}

/// Proposes the VerificationObligation material of explicitly scoped Requirements and
/// Operations. Pure: nothing is accepted, applied or persisted.
pub fn compile_verification_obligations(
    graph: &Graph,
    inputs: &VerificationApplicabilityInputs,
    audit: &VerificationAudit,
) -> Result<VerificationAnalysisResult, VerificationError> {
    let mut sources: BTreeMap<&Id, &VerificationSourceInput> = BTreeMap::new();
    for input in &inputs.sources {
        if sources.insert(&input.source_ref, input).is_some() {
            return Err(VerificationError::DuplicateSource {
                source_ref: input.source_ref.clone(),
            });
        }
    }
    let meta =
        AuditMeta::new(audit.created_by.clone(), audit.created_at, None, None).map_err(|e| {
            VerificationError::InvalidAudit {
                reason: e.to_string(),
            }
        })?;
    let scenarios = confirmed_scenarios(graph)?;
    let project_id = graph.project_id();
    let mut result = VerificationAnalysisResult {
        items: Vec::new(),
        proposals: Vec::new(),
        applicability: VerificationApplicabilitySnapshot::default(),
    };
    for (source_ref, input) in sources {
        let (node, resolution) = resolve(graph, input)?;
        result.applicability.source_refs.insert(source_ref.clone());
        if input.mode == VerificationSourceMode::ExecutableScenario {
            result
                .applicability
                .executable_source_refs
                .insert(source_ref.clone());
        }
        let mut item = VerificationObligationAnalysis {
            source_ref: source_ref.clone(),
            mode: input.mode,
            obligation_ref: None,
            verification_kind: None,
            scenario_refs: Vec::new(),
            blocking: input.blocking,
            disposition: VerificationDisposition::Existing,
        };
        let (kind, method) = match resolution {
            Resolution::Gap(disposition) => {
                item.disposition = disposition;
                result.items.push(item);
                continue;
            }
            Resolution::Kind(kind, method) => (kind, method),
        };
        let obligation_ref = verification_obligation_id(project_id, source_ref, kind)?;
        item.obligation_ref = Some(obligation_ref.clone());
        item.verification_kind = Some(kind);

        // Exact covering Scenarios of an executable source.
        let covering: Vec<&CoverageCandidate> = if kind == VerificationKind::Scenario {
            let operations = match &node.payload {
                NodePayload::Operation(_) => BTreeSet::from([source_ref.clone()]),
                _ => specifying_operations(graph, source_ref),
            };
            let is_requirement = matches!(node.payload, NodePayload::Requirement(_));
            scenarios
                .iter()
                .filter(|s| {
                    (is_requirement && s.requirement_refs.contains(source_ref))
                        || operations.iter().any(|o| covers_operation(s, o))
                })
                .collect()
        } else {
            Vec::new()
        };
        item.scenario_refs = covering.iter().map(|s| s.scenario_ref.clone()).collect();
        if input.blocking {
            result
                .applicability
                .blocking_obligation_refs
                .insert(obligation_ref.clone());
            result
                .applicability
                .blocking_scenario_refs
                .extend(item.scenario_refs.iter().cloned());
        }

        // Reconcile the obligation node.
        let source_evidence = sorted_unique(node.evidence.iter().cloned());
        let payload = VerificationObligation {
            name: format!("Verify {source_ref} via {}", kind_text(kind)),
            verification_kind: kind,
            method,
            acceptance_condition: None,
            required_evidence_kind: None,
        };
        let mut patches = Vec::new();
        match graph.node(&obligation_ref) {
            None => patches.push(SemanticPatch::AddNode {
                node: Node {
                    id: obligation_ref.clone(),
                    revision: 1,
                    status: ElementStatus::Proposed,
                    payload: NodePayload::VerificationObligation(payload),
                    evidence: source_evidence.clone(),
                    derivations: Vec::new(),
                    standards: Vec::new(),
                    tags: BTreeSet::new(),
                    extensions: BTreeMap::new(),
                    audit: meta.clone(),
                },
            }),
            Some(existing) => match &existing.payload {
                NodePayload::VerificationObligation(o)
                    if *o == payload && active(existing.status) => {}
                _ => {
                    return Err(VerificationError::VerificationIdCollision {
                        obligation_ref,
                        reason: "a different or inactive node holds the deterministic ID"
                            .to_owned(),
                    })
                }
            },
        }
        if let Some(edge) = plan_edge(
            graph,
            RelationKind::VerifiedBy,
            source_ref,
            &obligation_ref,
            source_evidence.clone(),
            &meta,
        )? {
            patches.push(SemanticPatch::AddEdge { edge });
        }
        for scenario in &covering {
            let evidence = sorted_unique(
                source_evidence
                    .iter()
                    .cloned()
                    .chain(scenario.evidence.iter().cloned()),
            );
            if let Some(edge) = plan_edge(
                graph,
                RelationKind::ImplementedAs,
                &obligation_ref,
                &scenario.scenario_ref,
                evidence,
                &meta,
            )? {
                patches.push(SemanticPatch::AddEdge { edge });
            }
        }
        if !patches.is_empty() {
            let evidence = sorted_unique(patches.iter().flat_map(|p| match p {
                SemanticPatch::AddNode { node } => node.evidence.clone(),
                SemanticPatch::AddEdge { edge } => edge.evidence.clone(),
                _ => Vec::new(),
            }));
            let proposal = Proposal::new(
                VERIFICATION_STAGE,
                PatchSet {
                    base_semantic_hash: graph.semantic_hash()?,
                    patch: SemanticPatch::Compound { patches },
                },
                evidence,
                Vec::new(),
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
                None,
            )
            .map_err(|e| VerificationError::InvalidProposal {
                reason: e.to_string(),
            })?;
            // In-memory dry validation by the canonical engine; the candidate is discarded.
            apply_patch(graph, &proposal.patch_set).map_err(|e| {
                VerificationError::InvalidProposal {
                    reason: format!("proposal does not apply: {e}"),
                }
            })?;
            item.disposition = VerificationDisposition::Proposed {
                proposal_ref: proposal.id.clone(),
            };
            result.proposals.push(proposal);
        }
        result.items.push(item);
    }
    Ok(result)
}
