//! Governed waivers: recognizing the `ResolutionDecision` that waives one deterministic
//! finding and applying the rule's waiver policy (metamodel §6.3, rulebook §2.3).
//!
//! No clock is consulted. `ResolutionDecision` has no review or expiry field, so a waiver is
//! validated for owner, exact finding scope, rationale and accepted governance state only.
//!
//! S3.4 (Hotfix 048) adds the pure creation side: [`prepare_waiver_material`] derives the
//! finding identity, checks the rule's waiver policy before anything is built and constructs the
//! canonical waiver decision, its resolves edge and the content-addressed artifact, self-checked
//! against this module's consumer. It has no mutation or proposal capability; plumb-functional
//! wraps the material in a proposal.

use plumb_core::{to_canonical_json, CoreError, Hash, HashKind, Id, Timestamp};
use plumb_psg::{
    AgentKind, AuditMeta, Edge, ElementStatus, Finding, Graph, Node, NodePayload, RelationKind,
    RelationProperties, ResolutionDecision,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

use crate::evaluator::{EvaluationError, ValidationPolicy};
use crate::finding::is_clean_text;
use crate::model::{RuleMetadata, WaiverPolicy};

/// The `kind` that marks a `ResolutionDecision.answer` as a waiver.
pub const WAIVER_KIND: &str = "waiver";

/// The exact `ResolutionDecision.answer` of a waiver decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaiverDecisionMarker {
    pub kind: String,
    pub rule_id: String,
    pub finding_key: Hash,
    pub finding_ref: Id,
}

/// A waiver that satisfied a violated rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedWaiver {
    pub rule_id: String,
    pub finding_ref: Id,
    pub finding_key: Hash,
    pub decision_ref: Id,
}

/// Looks in `graph` for the governed waiver of the finding `finding_ref` of `rule`, whose
/// violation targets are `targets` (sorted).
///
/// Returns `None` when no accepted waiver decision resolves the accepted finding. A decision
/// that claims to be a waiver but is malformed, forbidden, not enabled or ambiguous is an
/// error, never silently ignored, and so is a waiver claim on a node at the deterministic
/// Finding ID whose payload is not this finding.
pub fn governed_waiver(
    graph: &Graph,
    rule: &RuleMetadata,
    policy: &ValidationPolicy,
    finding_key: &Hash,
    finding_ref: &Id,
    targets: &[Id],
) -> Result<Option<AppliedWaiver>, EvaluationError> {
    let Some(finding_node) = graph.node(finding_ref) else {
        return Ok(None);
    };
    let NodePayload::Finding(finding) = &finding_node.payload else {
        return Ok(None);
    };
    if finding_node.status != ElementStatus::Accepted {
        return Ok(None);
    }

    let mut qualifying: Vec<Id> = Vec::new();
    for edge_id in graph.incoming_edge_ids(finding_ref) {
        let Some(edge) = graph.edge(edge_id) else {
            continue;
        };
        if edge.kind != RelationKind::Resolves || edge.status != ElementStatus::Accepted {
            continue;
        }
        let Some(decision_node) = graph.node(&edge.from) else {
            continue;
        };
        let NodePayload::ResolutionDecision(decision) = &decision_node.payload else {
            continue;
        };
        if decision_node.status != ElementStatus::Accepted || !claims_waiver(decision) {
            continue;
        }
        check_finding_identity(finding_ref, finding, rule, targets)?;
        check_waiver_decision(&decision_node.id, decision, rule, finding_key, finding_ref)?;
        qualifying.push(decision_node.id.clone());
    }

    // Two edges from one decision to the same finding are still one decision.
    qualifying.sort();
    qualifying.dedup();
    let decision_ref = match qualifying.as_slice() {
        [] => return Ok(None),
        [decision] => decision.clone(),
        _ => {
            return Err(EvaluationError::AmbiguousWaiver {
                rule_id: rule.id.clone(),
                finding_ref: finding_ref.clone(),
                decisions: qualifying,
            })
        }
    };

    match rule.waiver_policy {
        WaiverPolicy::Forbidden => Err(EvaluationError::ForbiddenWaiver {
            rule_id: rule.id.clone(),
            decision: decision_ref,
        }),
        WaiverPolicy::ProfileAllow if !policy.profile_allow_waiver_rule_ids.contains(&rule.id) => {
            Err(EvaluationError::ProfileWaiverNotEnabled {
                rule_id: rule.id.clone(),
                decision: decision_ref,
            })
        }
        WaiverPolicy::DecisionRequired | WaiverPolicy::ProfileAllow => Ok(Some(AppliedWaiver {
            rule_id: rule.id.clone(),
            finding_ref: finding_ref.clone(),
            finding_key: finding_key.clone(),
            decision_ref,
        })),
    }
}

/// Requires the accepted Finding at the deterministic ID to be the evaluated finding: same
/// rule code, gate family and sorted targets. Severity, message, resolution and waiver_ref
/// may legitimately differ.
fn check_finding_identity(
    finding_ref: &Id,
    finding: &Finding,
    rule: &RuleMetadata,
    targets: &[Id],
) -> Result<(), EvaluationError> {
    let mismatch = |reason: String| EvaluationError::FindingIdentityMismatch {
        finding_ref: finding_ref.clone(),
        reason,
    };
    if finding.code != rule.id {
        return Err(mismatch(format!(
            "code {:?} is not rule {:?}",
            finding.code, rule.id
        )));
    }
    if finding.family != rule.gate.as_str() {
        return Err(mismatch(format!(
            "family {:?} is not gate {}",
            finding.family, rule.gate
        )));
    }
    let mut expected = targets.to_vec();
    expected.sort();
    if finding.affected_refs != expected {
        return Err(mismatch(
            "affected_refs are not the violation targets".into(),
        ));
    }
    Ok(())
}

/// Whether the decision's answer is an object whose `kind` is the waiver kind.
fn claims_waiver(decision: &ResolutionDecision) -> bool {
    decision.answer.get("kind").and_then(|kind| kind.as_str()) == Some(WAIVER_KIND)
}

/// Requires a decision that claims to be a waiver to be exactly the waiver of this finding.
fn check_waiver_decision(
    decision_id: &Id,
    decision: &ResolutionDecision,
    rule: &RuleMetadata,
    finding_key: &Hash,
    finding_ref: &Id,
) -> Result<(), EvaluationError> {
    let malformed = |reason: String| EvaluationError::MalformedWaiverDecision {
        decision: decision_id.clone(),
        reason,
    };
    let marker: WaiverDecisionMarker = serde_json::from_value(decision.answer.clone())
        .map_err(|e| malformed(format!("answer is not a waiver marker: {e}")))?;
    if marker.finding_key.kind() != HashKind::Generic {
        return Err(malformed(format!(
            "finding_key {} is not a Generic hash",
            marker.finding_key
        )));
    }
    if marker.rule_id != rule.id {
        return Err(malformed(format!(
            "rule_id {:?} is not the evaluated rule {:?}",
            marker.rule_id, rule.id
        )));
    }
    if &marker.finding_key != finding_key {
        return Err(malformed(format!(
            "finding_key {} is not the deterministic key {finding_key}",
            marker.finding_key
        )));
    }
    if &marker.finding_ref != finding_ref {
        return Err(malformed(format!(
            "finding_ref {} is not the deterministic finding {finding_ref}",
            marker.finding_ref
        )));
    }
    if !decision.rationale.as_deref().is_some_and(is_clean_text) {
        return Err(malformed("rationale is missing or not clean text".into()));
    }
    Ok(())
}

// ============================================================================ creation material

/// The WaiverPatchArtifact format version.
pub const WAIVER_ARTIFACT_VERSION: u32 = 1;

/// One governed waiver request (plan S3.4, Hotfix 048). The finding identity is derived from the
/// rule, targets and condition; callers cannot choose an inconsistent key or reference.
#[derive(Debug, Clone, Copy)]
pub struct WaiverRequest<'a> {
    pub rule: &'a RuleMetadata,
    pub targets: &'a [Id],
    pub semantic_condition_key: &'a str,
    pub decided_by: &'a Id,
    pub decided_at: Timestamp,
    pub rationale: &'a str,
}

/// The content-addressed waiver artifact; its canonical hash is the decision's `patch_ref`. It
/// never contains the decision, so the hash is not circular.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaiverPatchArtifact {
    pub version: u32,
    pub finding_ref: Id,
    pub finding_key: Hash,
    pub rule_id: String,
}

impl WaiverPatchArtifact {
    /// RFC 8785 canonical bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CoreError> {
        to_canonical_json(self)
    }

    /// Generic SHA-256 of the canonical bytes.
    pub fn content_hash(&self) -> Result<Hash, CoreError> {
        Ok(Hash::content_sha256(&self.canonical_bytes()?))
    }
}

/// The single authoritative waiver governance material: what a proposal must add, unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct WaiverMaterial {
    pub decision_ref: Id,
    pub waiver_subject_ref: Id,
    pub decision: ResolutionDecision,
    pub decision_node: Node,
    pub resolves_edge: Edge,
    pub artifact: WaiverPatchArtifact,
    pub artifact_bytes: Vec<u8>,
}

/// Why waiver material cannot be prepared. Nothing is proposed or changed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WaiverCreationError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("invalid finding key: {reason}")]
    InvalidFindingKey { reason: String },
    #[error("finding {0} does not exist")]
    FindingMissing(Id),
    #[error("{0} is not a Finding")]
    FindingWrongType(Id),
    #[error("finding {0} is not Accepted")]
    FindingNotAccepted(Id),
    #[error("finding {finding_ref} is not the evaluated finding: {reason}")]
    RuleFindingMismatch { finding_ref: Id, reason: String },
    #[error("{0} is not an Accepted human Agent")]
    InvalidWaiverActor(Id),
    #[error("waiver rationale is not clean text")]
    InvalidWaiverRationale,
    #[error("rule {rule_id} forbids waivers")]
    ForbiddenWaiver { rule_id: String },
    #[error("the profile does not enable waivers of rule {rule_id}")]
    ProfileWaiverNotEnabled { rule_id: String },
    #[error("finding {finding_ref} is already waived by {decision_ref}")]
    AlreadyWaived { finding_ref: Id, decision_ref: Id },
    #[error("finding {finding_ref} is already waived differently by {decision_ref}")]
    ExistingWaiverConflict { finding_ref: Id, decision_ref: Id },
    #[error("waiver decision ID {0} is already used by different material")]
    WaiverDecisionIdCollision(Id),
    #[error("prepared waiver material violates the waiver contract: {reason}")]
    InvalidWaiverMaterial { reason: String },
}

fn short_id(prefix: &str, body: &serde_json::Value) -> Result<Id, CoreError> {
    let digest = Hash::content_sha256(&to_canonical_json(body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("{prefix}:{}", &hex[..16]).parse()
}

/// `waiver-subject:<16 hex of SHA-256(RFC 8785 {project_id, finding_ref, finding_key,
/// rule_id})>`: the reference token in a waiver decision's `proposal_ref`; no node exists at it.
pub fn waiver_subject_ref(
    project_id: &Id,
    finding_ref: &Id,
    finding_key: &Hash,
    rule_id: &str,
) -> Result<Id, CoreError> {
    short_id(
        "waiver-subject",
        &json!({
            "project_id": project_id.to_string(),
            "finding_ref": finding_ref.to_string(),
            "finding_key": finding_key.as_str(),
            "rule_id": rule_id,
        }),
    )
}

/// `dec:<16 hex of SHA-256(RFC 8785 {project_id, waiver_subject_ref, answer, decided_by,
/// decided_at, patch_ref})>`; the rationale is not part of decision identity.
pub fn waiver_decision_id(
    project_id: &Id,
    waiver_subject_ref: &Id,
    answer: &serde_json::Value,
    decided_by: &Id,
    decided_at: &Timestamp,
    patch_ref: &Hash,
) -> Result<Id, CoreError> {
    short_id(
        "dec",
        &json!({
            "project_id": project_id.to_string(),
            "waiver_subject_ref": waiver_subject_ref.to_string(),
            "answer": answer,
            "decided_by": decided_by.to_string(),
            "decided_at": decided_at.to_string(),
            "patch_ref": patch_ref.as_str(),
        }),
    )
}

/// `rel:<16 hex of SHA-256(RFC 8785 {kind: resolves, from, to})>`.
pub fn waiver_resolves_edge_id(from: &Id, to: &Id) -> Result<Id, CoreError> {
    short_id(
        "rel",
        &json!({"kind": RelationKind::Resolves.as_str(), "from": from.to_string(), "to": to.to_string()}),
    )
}

/// Prepares the canonical waiver governance material for one violation: derives the finding
/// identity, checks the persisted Finding, the rule's waiver policy, the human actor, the
/// rationale and existing waivers, builds the marker, subject, artifact, decision and resolves
/// edge, and self-checks them against the waiver consumer. Pure: no proposal, no mutation.
pub fn prepare_waiver_material(
    graph: &Graph,
    request: &WaiverRequest<'_>,
    policy: &ValidationPolicy,
) -> Result<WaiverMaterial, WaiverCreationError> {
    let rule = request.rule;
    let invalid_key = |e: EvaluationError| WaiverCreationError::InvalidFindingKey {
        reason: e.to_string(),
    };
    let finding_key =
        crate::finding::finding_key(&rule.id, request.targets, request.semantic_condition_key)
            .map_err(invalid_key)?;
    let finding_ref = crate::finding::finding_id(&finding_key).map_err(invalid_key)?;

    // The persisted, exact evaluated Finding.
    let finding_node = graph
        .node(&finding_ref)
        .ok_or_else(|| WaiverCreationError::FindingMissing(finding_ref.clone()))?;
    let NodePayload::Finding(finding) = &finding_node.payload else {
        return Err(WaiverCreationError::FindingWrongType(finding_ref.clone()));
    };
    if finding_node.status != ElementStatus::Accepted {
        return Err(WaiverCreationError::FindingNotAccepted(finding_ref.clone()));
    }
    check_finding_identity(&finding_ref, finding, rule, request.targets).map_err(|e| {
        WaiverCreationError::RuleFindingMismatch {
            finding_ref: finding_ref.clone(),
            reason: e.to_string(),
        }
    })?;

    // The rule's waiver policy, before anything is built.
    match rule.waiver_policy {
        WaiverPolicy::Forbidden => {
            return Err(WaiverCreationError::ForbiddenWaiver {
                rule_id: rule.id.clone(),
            })
        }
        WaiverPolicy::ProfileAllow if !policy.profile_allow_waiver_rule_ids.contains(&rule.id) => {
            return Err(WaiverCreationError::ProfileWaiverNotEnabled {
                rule_id: rule.id.clone(),
            })
        }
        WaiverPolicy::DecisionRequired | WaiverPolicy::ProfileAllow => {}
    }

    let human = graph.node(request.decided_by).is_some_and(|n| {
        n.status == ElementStatus::Accepted
            && matches!(&n.payload, NodePayload::Agent(a) if a.agent_kind == AgentKind::Human)
    });
    if !human {
        return Err(WaiverCreationError::InvalidWaiverActor(
            request.decided_by.clone(),
        ));
    }
    if !is_clean_text(request.rationale) {
        return Err(WaiverCreationError::InvalidWaiverRationale);
    }

    let marker = WaiverDecisionMarker {
        kind: WAIVER_KIND.to_owned(),
        rule_id: rule.id.clone(),
        finding_key: finding_key.clone(),
        finding_ref: finding_ref.clone(),
    };

    // An Accepted waiver of this Finding already exists: never a second one.
    for edge_id in graph.incoming_edge_ids(&finding_ref) {
        let Some(edge) = graph
            .edge(edge_id)
            .filter(|e| e.kind == RelationKind::Resolves && e.status == ElementStatus::Accepted)
        else {
            continue;
        };
        let Some(existing) = graph.node(&edge.from) else {
            continue;
        };
        let NodePayload::ResolutionDecision(decision) = &existing.payload else {
            continue;
        };
        if existing.status != ElementStatus::Accepted || !claims_waiver(decision) {
            continue;
        }
        let same = serde_json::from_value::<WaiverDecisionMarker>(decision.answer.clone())
            .is_ok_and(|m| m == marker);
        return Err(if same {
            WaiverCreationError::AlreadyWaived {
                finding_ref: finding_ref.clone(),
                decision_ref: existing.id.clone(),
            }
        } else {
            WaiverCreationError::ExistingWaiverConflict {
                finding_ref: finding_ref.clone(),
                decision_ref: existing.id.clone(),
            }
        });
    }

    let answer =
        serde_json::to_value(&marker).map_err(|e| CoreError::Canonicalization(e.to_string()))?;
    let waiver_subject =
        waiver_subject_ref(graph.project_id(), &finding_ref, &finding_key, &rule.id)?;
    let artifact = WaiverPatchArtifact {
        version: WAIVER_ARTIFACT_VERSION,
        finding_ref: finding_ref.clone(),
        finding_key: finding_key.clone(),
        rule_id: rule.id.clone(),
    };
    let artifact_bytes = artifact.canonical_bytes()?;
    let patch_ref = Hash::content_sha256(&artifact_bytes);
    let decision_ref = waiver_decision_id(
        graph.project_id(),
        &waiver_subject,
        &answer,
        request.decided_by,
        &request.decided_at,
        &patch_ref,
    )?;
    if graph.node(&decision_ref).is_some() || graph.edge(&decision_ref).is_some() {
        return Err(WaiverCreationError::WaiverDecisionIdCollision(decision_ref));
    }
    let decision = ResolutionDecision {
        question_ref: None,
        proposal_ref: Some(waiver_subject.clone()),
        answer,
        decided_by: request.decided_by.clone(),
        decided_at: request.decided_at,
        patch_ref,
        rationale: Some(request.rationale.to_owned()),
        supersedes: None,
    };
    let audit = AuditMeta::new(request.decided_by.clone(), request.decided_at, None, None)
        .map_err(|e| WaiverCreationError::InvalidWaiverMaterial {
            reason: e.to_string(),
        })?;
    let decision_node = Node {
        id: decision_ref.clone(),
        revision: 1,
        status: ElementStatus::Accepted,
        payload: NodePayload::ResolutionDecision(decision.clone()),
        evidence: Vec::new(),
        derivations: Vec::new(),
        standards: Vec::new(),
        tags: Default::default(),
        extensions: Default::default(),
        audit: audit.clone(),
    };
    let resolves_edge = Edge {
        id: waiver_resolves_edge_id(&decision_ref, &finding_ref)?,
        revision: 1,
        status: ElementStatus::Accepted,
        kind: RelationKind::Resolves,
        from: decision_ref.clone(),
        to: finding_ref.clone(),
        properties: RelationProperties::None,
        evidence: Vec::new(),
        derivations: Vec::new(),
        standards: Vec::new(),
        audit,
    };

    // Self-check against the authoritative consumer contract.
    let self_check = |reason: String| WaiverCreationError::InvalidWaiverMaterial { reason };
    if !claims_waiver(&decision) {
        return Err(self_check("the decision does not claim a waiver".into()));
    }
    check_waiver_decision(&decision_ref, &decision, rule, &finding_key, &finding_ref)
        .map_err(|e| self_check(e.to_string()))?;
    decision_node
        .validate()
        .map_err(|e| self_check(e.to_string()))?;
    resolves_edge
        .validate()
        .map_err(|e| self_check(e.to_string()))?;

    Ok(WaiverMaterial {
        decision_ref,
        waiver_subject_ref: waiver_subject,
        decision,
        decision_node,
        resolves_edge,
        artifact,
        artifact_bytes,
    })
}
