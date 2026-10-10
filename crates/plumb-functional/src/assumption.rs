//! Governed assumptions and their expiry (plan S3.4, Hotfix 048; metamodel §6.4).
//!
//! An Accepted Assumption is created only through a validated human decision with a
//! content-derived ID; `risk_ref` must be absent because the metamodel defines no Risk element.
//! Expiry analysis reads an injected Clock once and reports every active assumption whose
//! expiry has been reached as S3 governance finding material whose identity never depends on the
//! current time. Nothing here mutates the graph, reads files or an ambient clock.

use std::collections::BTreeMap;

use plumb_core::{to_canonical_json, Clock, CoreError, Hash, Id, StageId, Timestamp};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AgentKind, Assumption, AuditMeta, ElementStatus, Finding, FindingSeverity, Graph, Node,
    NodePayload, NodeType,
};
use plumb_validation::{finding_id, finding_key, GeneratedFinding};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

const ASSUMPTION_STAGE: StageId = StageId::S3;

/// The pilot Assumption.status vocabulary; only `Accepted` is active.
pub const ASSUMPTION_STATUSES: [&str; 5] =
    ["Open", "Accepted", "Resolved", "Expired", "Superseded"];

/// The active Assumption status, the only one S3.4 creates.
pub const ASSUMPTION_ACCEPTED: &str = "Accepted";

/// The S3 governance finding code of an expired assumption (not a registered validation rule).
pub const ASSUMPTION_EXPIRED_CODE: &str = "PLUMB.S3.ASSUMPTION.EXPIRED";

/// The family of S3 governance finding material.
pub const ASSUMPTION_EXPIRED_FAMILY: &str = "S3";

const EXPIRED_RESOLUTION: &str =
    "Review, resolve, supersede or explicitly replace the expired assumption.";

/// The semantic content of a new assumption.
#[derive(Debug, Clone, PartialEq)]
pub struct AssumptionInput {
    pub statement: String,
    pub owner_ref: Id,
    pub finding_ref: Option<Id>,
    pub default_value: Option<JsonValue>,
    pub expires_at: Option<Timestamp>,
    /// Must be None: the metamodel defines no Risk element.
    pub risk_ref: Option<Id>,
}

/// The accepting human actor and time (caller supplied; no clock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssumptionAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// Why an assumption cannot be created or analyzed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AssumptionError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("assumption statement is not clean text")]
    InvalidStatement,
    #[error("assumption {assumption_ref} has status {status:?} outside the pilot vocabulary")]
    InvalidAssumptionStatus { assumption_ref: Id, status: String },
    #[error("owner {0} does not exist")]
    OwnerMissing(Id),
    #[error("owner {0} is not Accepted")]
    OwnerNotAccepted(Id),
    #[error("owner {0} is not an Agent or Stakeholder")]
    OwnerWrongType(Id),
    #[error("{0} is not an Accepted human Agent")]
    InvalidAcceptanceActor(Id),
    #[error("finding {0} does not exist")]
    FindingMissing(Id),
    #[error("finding {0} is not Accepted")]
    FindingNotAccepted(Id),
    #[error("{0} is not a Finding")]
    FindingWrongType(Id),
    #[error("risk_ref {risk_ref} is unsupported: the metamodel defines no Risk element")]
    RiskReferenceUnsupported { risk_ref: Id },
    #[error("an assumption linked to blocker finding {finding_ref} needs expires_at")]
    BlockingAssumptionRequiresExpiry { finding_ref: Id },
    #[error("assumption ID {0} is already used by different material")]
    AssumptionIdCollision(Id),
    #[error("assumption proposal is invalid: {reason}")]
    InvalidAssumptionProposal { reason: String },
    #[error("expiry finding material is invalid: {reason}")]
    InvalidExpiryFinding { reason: String },
}

/// The creation outcome.
#[derive(Debug, Clone, PartialEq)]
pub enum AssumptionCreation {
    New {
        assumption_ref: Id,
        assumption: Assumption,
        proposal: Box<Proposal>,
    },
    /// The identical Accepted Assumption already exists; no proposal.
    Existing { assumption_ref: Id },
}

/// The expiry findings of one analysis, by Finding ID.
#[derive(Debug, Clone, PartialEq)]
pub struct AssumptionExpiryResult {
    pub findings: Vec<GeneratedFinding>,
}

fn clean(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

fn accepted(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
}

/// `assumption:<16 hex of SHA-256(RFC 8785 {project_id, statement, owner_ref, finding_ref,
/// default_value, expires_at, risk_ref: null})>`; status and audit are not identity.
pub fn assumption_id(project_id: &Id, input: &AssumptionInput) -> Result<Id, CoreError> {
    let body = json!({
        "project_id": project_id.to_string(),
        "statement": input.statement,
        "owner_ref": input.owner_ref.to_string(),
        "finding_ref": input.finding_ref.as_ref().map(Id::to_string),
        "default_value": input.default_value,
        "expires_at": input.expires_at.map(|t| t.to_string()),
        "risk_ref": JsonValue::Null,
    });
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("assumption:{}", &hex[..16]).parse()
}

/// Validates a governed assumption and proposes it as an Accepted node.
pub fn create_assumption(
    graph: &Graph,
    input: &AssumptionInput,
    audit: &AssumptionAudit,
) -> Result<AssumptionCreation, AssumptionError> {
    if let Some(risk_ref) = &input.risk_ref {
        return Err(AssumptionError::RiskReferenceUnsupported {
            risk_ref: risk_ref.clone(),
        });
    }
    if !clean(&input.statement) {
        return Err(AssumptionError::InvalidStatement);
    }
    let owner = graph
        .node(&input.owner_ref)
        .ok_or_else(|| AssumptionError::OwnerMissing(input.owner_ref.clone()))?;
    if !matches!(
        owner.payload.node_type(),
        NodeType::Agent | NodeType::Stakeholder
    ) {
        return Err(AssumptionError::OwnerWrongType(input.owner_ref.clone()));
    }
    if !accepted(owner) {
        return Err(AssumptionError::OwnerNotAccepted(input.owner_ref.clone()));
    }
    let human = graph.node(&audit.created_by).is_some_and(|n| {
        accepted(n)
            && matches!(&n.payload, NodePayload::Agent(a) if a.agent_kind == AgentKind::Human)
    });
    if !human {
        return Err(AssumptionError::InvalidAcceptanceActor(
            audit.created_by.clone(),
        ));
    }
    if let Some(finding_ref) = &input.finding_ref {
        let node = graph
            .node(finding_ref)
            .ok_or_else(|| AssumptionError::FindingMissing(finding_ref.clone()))?;
        let NodePayload::Finding(finding) = &node.payload else {
            return Err(AssumptionError::FindingWrongType(finding_ref.clone()));
        };
        if !accepted(node) {
            return Err(AssumptionError::FindingNotAccepted(finding_ref.clone()));
        }
        if finding.severity == FindingSeverity::Blocker && input.expires_at.is_none() {
            return Err(AssumptionError::BlockingAssumptionRequiresExpiry {
                finding_ref: finding_ref.clone(),
            });
        }
    }

    let assumption_ref = assumption_id(graph.project_id(), input)?;
    let assumption = Assumption {
        statement: input.statement.clone(),
        owner_ref: input.owner_ref.clone(),
        status: ASSUMPTION_ACCEPTED.to_owned(),
        finding_ref: input.finding_ref.clone(),
        default_value: input.default_value.clone(),
        expires_at: input.expires_at,
        risk_ref: None,
    };
    if let Some(existing) = graph.node(&assumption_ref) {
        let exact = accepted(existing)
            && matches!(&existing.payload, NodePayload::Assumption(a) if *a == assumption);
        return if exact {
            Ok(AssumptionCreation::Existing { assumption_ref })
        } else {
            Err(AssumptionError::AssumptionIdCollision(assumption_ref))
        };
    }
    if graph.edge(&assumption_ref).is_some() {
        return Err(AssumptionError::AssumptionIdCollision(assumption_ref));
    }
    let invalid = |reason: String| AssumptionError::InvalidAssumptionProposal { reason };
    let meta = AuditMeta::new(audit.created_by.clone(), audit.created_at, None, None)
        .map_err(|e| invalid(e.to_string()))?;
    let node = Node {
        id: assumption_ref.clone(),
        revision: 1,
        status: ElementStatus::Accepted,
        payload: NodePayload::Assumption(assumption.clone()),
        evidence: Vec::new(),
        derivations: Vec::new(),
        standards: Vec::new(),
        tags: Default::default(),
        extensions: Default::default(),
        audit: meta,
    };
    let proposal = Proposal::new(
        ASSUMPTION_STAGE,
        PatchSet {
            base_semantic_hash: graph.semantic_hash()?,
            patch: SemanticPatch::AddNode { node },
        },
        Vec::new(),
        Vec::new(),
        ProposalMateriality::MaterialDecision,
        AcceptancePolicy::HumanDecision,
        None,
    )
    .map_err(|e| invalid(e.to_string()))?;
    apply_patch(graph, &proposal.patch_set)
        .map_err(|e| invalid(format!("proposal does not apply: {e}")))?;
    Ok(AssumptionCreation::New {
        assumption_ref,
        assumption,
        proposal: Box::new(proposal),
    })
}

/// `assumption_expired:<assumption_ref>:<expires_at>`.
pub fn expiry_condition_key(assumption_ref: &Id, expires_at: &Timestamp) -> String {
    format!("assumption_expired:{assumption_ref}:{expires_at}")
}

/// The deterministic expiry finding material of one Accepted assumption.
pub fn expiry_finding(
    assumption_ref: &Id,
    expires_at: &Timestamp,
) -> Result<GeneratedFinding, AssumptionError> {
    let invalid = |e: plumb_validation::EvaluationError| AssumptionError::InvalidExpiryFinding {
        reason: e.to_string(),
    };
    let condition = expiry_condition_key(assumption_ref, expires_at);
    let targets = vec![assumption_ref.clone()];
    let key = finding_key(ASSUMPTION_EXPIRED_CODE, &targets, &condition).map_err(invalid)?;
    let id = finding_id(&key).map_err(invalid)?;
    Ok(GeneratedFinding {
        key,
        id,
        semantic_condition_key: condition,
        payload: Finding {
            code: ASSUMPTION_EXPIRED_CODE.to_owned(),
            family: ASSUMPTION_EXPIRED_FAMILY.to_owned(),
            severity: FindingSeverity::Blocker,
            message: format!("Accepted assumption {assumption_ref} expired at {expires_at}."),
            status: "Open".to_owned(),
            affected_refs: targets,
            standard_rule_ref: None,
            suggested_resolution: Some(EXPIRED_RESOLUTION.to_owned()),
            waiver_ref: None,
        },
    })
}

/// Reports every Accepted assumption whose expiry has been reached (`now >= expires_at`) at one
/// clock instant, read exactly once. Pure: assumptions are never changed.
pub fn analyze_assumption_expiry(
    graph: &Graph,
    clock: &dyn Clock,
) -> Result<AssumptionExpiryResult, AssumptionError> {
    let now = clock.now();
    let mut findings = BTreeMap::new();
    for id in graph.node_ids_by_type(NodeType::Assumption) {
        let Some(node) = graph.node(id).filter(|n| accepted(n)) else {
            continue;
        };
        let NodePayload::Assumption(assumption) = &node.payload else {
            continue;
        };
        if !ASSUMPTION_STATUSES.contains(&assumption.status.as_str()) {
            return Err(AssumptionError::InvalidAssumptionStatus {
                assumption_ref: node.id.clone(),
                status: assumption.status.clone(),
            });
        }
        if assumption.status != ASSUMPTION_ACCEPTED {
            continue;
        }
        if let Some(expires_at) = &assumption.expires_at {
            if now >= *expires_at {
                let finding = expiry_finding(&node.id, expires_at)?;
                findings.insert(finding.id.clone(), finding);
            }
        }
    }
    Ok(AssumptionExpiryResult {
        findings: findings.into_values().collect(),
    })
}
