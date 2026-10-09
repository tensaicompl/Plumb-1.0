//! Deterministic authorization compilation and static separation-of-duty analysis (plan S2.8;
//! compiler architecture §8).
//!
//! S2.8 runs no inference. It compiles an explicitly supplied, structured
//! [`AuthorizationDraft`] into one HumanConfirm proposal of Principal, SecurityRole,
//! Permission, ResourceScope, PolicyCondition and SeparationConstraint nodes with the canonical
//! `assigned_role`, `inherits_role`, `grants`, `permits`, `scoped_to` and `conditioned_by`
//! relations. Every reference is an explicit ID; nothing is matched by name, and security
//! membership exists only as `assigned_role` from a Principal or Actor to a SecurityRole.
//! `A inherits_role B` means A inherits B, so effective roles follow outgoing inheritance.
//! The role-hierarchy and static separation-of-duty analyzers are pure and reusable; the other
//! separation kinds are reported as not evaluated. Scope kinds and condition expressions are
//! preserved verbatim and never evaluated. `apply_patch` is used solely to dry-validate the
//! proposal in memory; nothing is persisted and no finding is created.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AuditMeta, Edge, ElementStatus, EvidenceRef, Graph, Node, NodePayload, NodeType, Permission,
    PolicyCondition, Principal, RelationKind, RelationProperties, ResourceScope, SecurityRole,
    SeparationConstraint, SeparationConstraintKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

const AUTHORIZATION_STAGE: StageId = StageId::S2;

// ============================================================================ draft

/// A SecurityRole to declare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityRoleDraft {
    pub name: String,
    pub description: Option<String>,
    pub evidence: Vec<EvidenceRef>,
}

/// A Principal to declare; `principal_kind` is the metamodel's open String.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrincipalDraft {
    pub name: String,
    pub principal_kind: String,
    pub evidence: Vec<EvidenceRef>,
}

/// An opaque policy condition; the expression is never parsed or evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConditionDraft {
    pub expression: String,
    pub evidence: Vec<EvidenceRef>,
}

/// One permission row: the owning SecurityRole grants a Permission that permits one Accepted
/// Operation, is scoped to one explicit resource and may carry conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDraft {
    pub security_role_ref: Id,
    pub operation_ref: Id,
    pub resource_ref: Id,
    pub scope_kind: String,
    pub conditions: Vec<PolicyConditionDraft>,
    pub evidence: Vec<EvidenceRef>,
}

/// `subject -assigned_role-> SecurityRole` for a Principal or Actor subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleAssignmentDraft {
    pub subject_ref: Id,
    pub security_role_ref: Id,
    pub evidence: Vec<EvidenceRef>,
}

/// `role -inherits_role-> inherited_role`: `role` inherits `inherited_role`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleInheritanceDraft {
    pub role_ref: Id,
    pub inherited_role_ref: Id,
    pub evidence: Vec<EvidenceRef>,
}

/// A separation constraint over at least two SecurityRoles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationConstraintDraft {
    pub constraint_kind: SeparationConstraintKind,
    pub role_refs: Vec<Id>,
    pub evidence: Vec<EvidenceRef>,
}

/// Explicitly supplied, structured authorization semantics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationDraft {
    pub security_roles: Vec<SecurityRoleDraft>,
    pub principals: Vec<PrincipalDraft>,
    pub permissions: Vec<PermissionDraft>,
    pub assignments: Vec<RoleAssignmentDraft>,
    pub inheritances: Vec<RoleInheritanceDraft>,
    pub separation_constraints: Vec<SeparationConstraintDraft>,
}

/// Who owns the Proposed elements and when, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

// ============================================================================ issues

/// Why authorization compilation could not run at all.
#[derive(Debug, Error)]
pub enum AuthorizationError {
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_proposal(e: impl ToString) -> AuthorizationError {
    AuthorizationError::InvalidProposal {
        reason: e.to_string(),
    }
}

/// A deterministic compilation problem. These are analysis categories, not rule IDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthorizationIssue {
    InvalidName {
        field: String,
        reason: String,
    },
    InvalidPrincipalKind {
        reason: String,
    },
    InvalidScopeKind {
        reason: String,
    },
    InvalidConditionExpression {
        reason: String,
    },
    InvalidEvidence {
        reference: Id,
        reason: String,
    },
    UnknownReference {
        field: String,
        reference: Id,
        reason: String,
    },
    WrongTargetType {
        field: String,
        reference: Id,
        node_type: String,
    },
    NonAcceptedOperation {
        reference: Id,
    },
    NonAcceptedResource {
        reference: Id,
    },
    NonAcceptedSubject {
        reference: Id,
    },
    DuplicateDraftIdentity {
        node_ref: Id,
    },
    ExistingPrincipalConflict {
        node_ref: Id,
    },
    ExistingSecurityRoleConflict {
        node_ref: Id,
    },
    ExistingSecurityRoleNameConflict {
        node_ref: Id,
        existing_ref: Id,
    },
    ExistingPermissionConflict {
        node_ref: Id,
    },
    ExistingResourceScopeConflict {
        node_ref: Id,
    },
    ExistingPolicyConditionConflict {
        node_ref: Id,
    },
    ExistingSeparationConstraintConflict {
        node_ref: Id,
    },
    RoleHierarchyCycle {
        role_refs: Vec<Id>,
    },
    InvalidSeparationConstraint {
        reason: String,
    },
    InvalidProposal {
        reason: String,
    },
}

// ============================================================================ identities

fn short_id(prefix: &str, body: &JsonValue) -> Result<Id, CoreError> {
    let digest = Hash::content_sha256(&to_canonical_json(body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("{prefix}:{}", &hex[..16]).parse()
}

/// `security_role:<16 hex of SHA-256(RFC 8785 {project_id, name, node_type})>`.
pub fn security_role_id(project_id: &Id, name: &str) -> Result<Id, CoreError> {
    short_id(
        "security_role",
        &json!({"project_id": project_id, "name": name, "node_type": "security_role"}),
    )
}

/// `principal:<16 hex of SHA-256(RFC 8785 {project_id, name, principal_kind, node_type})>`.
pub fn principal_id(project_id: &Id, name: &str, principal_kind: &str) -> Result<Id, CoreError> {
    short_id(
        "principal",
        &json!({
            "project_id": project_id,
            "name": name,
            "principal_kind": principal_kind,
            "node_type": "principal"
        }),
    )
}

/// `resource_scope:<16 hex of SHA-256(RFC 8785 {project_id, resource_ref, scope_kind,
/// node_type})>`.
pub fn resource_scope_id(
    project_id: &Id,
    resource_ref: &Id,
    scope_kind: &str,
) -> Result<Id, CoreError> {
    short_id(
        "resource_scope",
        &json!({
            "project_id": project_id,
            "resource_ref": resource_ref,
            "scope_kind": scope_kind,
            "node_type": "resource_scope"
        }),
    )
}

/// `policy_condition:<16 hex of SHA-256(RFC 8785 {project_id, expression, node_type})>`.
pub fn policy_condition_id(project_id: &Id, expression: &str) -> Result<Id, CoreError> {
    short_id(
        "policy_condition",
        &json!({"project_id": project_id, "expression": expression, "node_type": "policy_condition"}),
    )
}

/// `permission:<16 hex of SHA-256(RFC 8785 {project_id, security_role_ref, operation_ref,
/// resource_scope_ref, node_type})>`; conditions are not part of the identity.
pub fn permission_id(
    project_id: &Id,
    security_role_ref: &Id,
    operation_ref: &Id,
    resource_scope_ref: &Id,
) -> Result<Id, CoreError> {
    short_id(
        "permission",
        &json!({
            "project_id": project_id,
            "security_role_ref": security_role_ref,
            "operation_ref": operation_ref,
            "resource_scope_ref": resource_scope_ref,
            "node_type": "permission"
        }),
    )
}

/// `separation_constraint:<16 hex of SHA-256(RFC 8785 {project_id, constraint_kind, role_refs
/// (sorted), node_type})>`.
pub fn separation_constraint_id(
    project_id: &Id,
    constraint_kind: SeparationConstraintKind,
    role_refs: &[Id],
) -> Result<Id, CoreError> {
    let mut roles = role_refs.to_vec();
    roles.sort();
    short_id(
        "separation_constraint",
        &json!({
            "project_id": project_id,
            "constraint_kind": constraint_kind,
            "role_refs": roles,
            "node_type": "separation_constraint"
        }),
    )
}

/// `rel:<16 hex of SHA-256(RFC 8785 {kind, from, to})>`.
fn edge_id(kind: &RelationKind, from: &Id, to: &Id) -> Result<Id, CoreError> {
    short_id("rel", &json!({"kind": kind, "from": from, "to": to}))
}

/// The deterministic Permission name `<SecurityRole.name> -> <Operation.name> [<scope_kind>]`.
pub fn permission_name(security_role_name: &str, operation_name: &str, scope_kind: &str) -> String {
    format!("{security_role_name} -> {operation_name} [{scope_kind}]")
}

// ============================================================================ analyzers

// The pure analyzers live in plumb-validation (Hotfix 044); these are their S2.8 public names.
pub use plumb_validation::authorization_analysis::{
    accepted_role_hierarchy_cycles, accepted_separation_analysis, authorization_facts,
    effective_security_roles, role_hierarchy_cycles, separation_analysis, AuthorizationFacts,
    SeparationAnalysis, StaticSodViolation,
};

fn is_active(status: ElementStatus) -> bool {
    matches!(status, ElementStatus::Proposed | ElementStatus::Accepted)
}

// ============================================================================ compilation

/// What happened to the draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationDisposition {
    /// One compound proposal holds every missing element.
    Proposed { proposal_ref: Id },
    /// Every element already exists identically; nothing to propose.
    AllExisting,
    /// Issues prevent a proposal.
    Rejected,
}

/// Everything one compilation produced; nothing has been applied or persisted.
#[derive(Debug, Clone)]
pub struct AuthorizationAnalysisResult {
    pub proposal: Option<Proposal>,
    pub disposition: AuthorizationDisposition,
    /// Candidate-overlay hierarchy cycles (blocking).
    pub hierarchy_cycles: Vec<Vec<Id>>,
    /// Candidate-overlay static SOD violations (analysis material, not blocking).
    pub static_sod_violations: Vec<StaticSodViolation>,
    /// Candidate-overlay constraints of kinds that are not evaluated.
    pub unevaluated_separation_constraints: Vec<Id>,
    pub issues: Vec<AuthorizationIssue>,
}

fn text_problem(value: &str) -> Option<&'static str> {
    if value.trim().is_empty() {
        Some("empty")
    } else if value.trim() != value {
        Some("not trimmed")
    } else if value.chars().any(char::is_control) {
        Some("contains control characters")
    } else {
        None
    }
}

/// A new node or edge with its accumulated evidence.
struct Planned<T> {
    item: T,
    evidence: BTreeSet<EvidenceRef>,
}

struct Compiler<'a> {
    graph: &'a Graph,
    issues: Vec<AuthorizationIssue>,
    /// New nodes by ID with their payloads and evidence.
    nodes: BTreeMap<Id, Planned<NodePayload>>,
    /// Planned edges by (kind, from, to) with evidence.
    edges: BTreeMap<(String, Id, Id), Planned<RelationKind>>,
    /// Draft-declared names of SecurityRoles.
    declared_roles: BTreeMap<Id, String>,
    declared_principals: BTreeSet<Id>,
}

impl<'a> Compiler<'a> {
    fn issue(&mut self, issue: AuthorizationIssue) {
        self.issues.push(issue);
    }

    /// Sorts evidence and checks that every ref names an EvidenceFragment, without duplicates.
    fn evidence(&mut self, refs: &[EvidenceRef]) -> BTreeSet<EvidenceRef> {
        let mut sorted = refs.to_vec();
        sorted.sort();
        for pair in sorted.windows(2) {
            if pair[0] == pair[1] {
                self.issue(AuthorizationIssue::InvalidEvidence {
                    reference: pair[0].as_id().clone(),
                    reason: "duplicate evidence reference".into(),
                });
            }
        }
        for r in &sorted {
            match self.graph.node(r.as_id()) {
                Some(n) if n.payload.node_type() == NodeType::EvidenceFragment => {}
                Some(_) => self.issue(AuthorizationIssue::InvalidEvidence {
                    reference: r.as_id().clone(),
                    reason: "not an EvidenceFragment".into(),
                }),
                None => self.issue(AuthorizationIssue::InvalidEvidence {
                    reference: r.as_id().clone(),
                    reason: "does not exist".into(),
                }),
            }
        }
        sorted.into_iter().collect()
    }

    fn text(&mut self, value: &str, make: impl Fn(String) -> AuthorizationIssue) -> bool {
        match text_problem(value) {
            Some(reason) => {
                self.issue(make(reason.to_owned()));
                false
            }
            None => true,
        }
    }

    /// Reconciles a new node with an existing one at its deterministic ID: absent nodes are
    /// planned, identical active nodes reused, anything else is the given conflict.
    fn node(
        &mut self,
        id: Id,
        payload: NodePayload,
        evidence: BTreeSet<EvidenceRef>,
        conflict: impl Fn(Id) -> AuthorizationIssue,
    ) {
        match self.graph.node(&id) {
            Some(existing) if is_active(existing.status) && existing.payload == payload => {}
            Some(_) => self.issue(conflict(id)),
            None => match self.nodes.get_mut(&id) {
                Some(planned) if planned.item == payload => planned.evidence.extend(evidence),
                Some(_) => self.issue(conflict(id)),
                None => {
                    self.nodes.insert(
                        id,
                        Planned {
                            item: payload,
                            evidence,
                        },
                    );
                }
            },
        }
    }

    fn edge(&mut self, kind: RelationKind, from: &Id, to: &Id, evidence: BTreeSet<EvidenceRef>) {
        let key_kind = kind.as_str().to_owned();
        let entry = self
            .edges
            .entry((key_kind, from.clone(), to.clone()))
            .or_insert(Planned {
                item: kind,
                evidence: BTreeSet::new(),
            });
        entry.evidence.extend(evidence);
    }

    /// Resolves a SecurityRole reference: declared in the draft or an active SecurityRole.
    fn role(&mut self, field: &str, id: &Id) -> Option<String> {
        if let Some(name) = self.declared_roles.get(id) {
            return Some(name.clone());
        }
        match self.graph.node(id) {
            Some(n) => match &n.payload {
                NodePayload::SecurityRole(r) if is_active(n.status) => Some(r.name.clone()),
                NodePayload::SecurityRole(_) => {
                    self.issue(AuthorizationIssue::UnknownReference {
                        field: field.into(),
                        reference: id.clone(),
                        reason: "SecurityRole is not active".into(),
                    });
                    None
                }
                other => {
                    self.issue(AuthorizationIssue::WrongTargetType {
                        field: field.into(),
                        reference: id.clone(),
                        node_type: other.node_type().as_str().into(),
                    });
                    None
                }
            },
            None => {
                self.issue(AuthorizationIssue::UnknownReference {
                    field: field.into(),
                    reference: id.clone(),
                    reason: "neither declared in the draft nor in the graph".into(),
                });
                None
            }
        }
    }

    /// An Accepted node of one of `allowed`, reporting missing, wrongly typed or non-Accepted
    /// references with the given category.
    fn accepted(
        &mut self,
        field: &str,
        id: &Id,
        allowed: &[NodeType],
        non_accepted: impl Fn(Id) -> AuthorizationIssue,
    ) -> Option<&'a Node> {
        let graph = self.graph;
        match graph.node(id) {
            None => {
                self.issue(AuthorizationIssue::UnknownReference {
                    field: field.into(),
                    reference: id.clone(),
                    reason: "does not exist".into(),
                });
                None
            }
            Some(n) if !allowed.contains(&n.payload.node_type()) => {
                self.issue(AuthorizationIssue::WrongTargetType {
                    field: field.into(),
                    reference: id.clone(),
                    node_type: n.payload.node_type().as_str().into(),
                });
                None
            }
            Some(n) if n.status != ElementStatus::Accepted => {
                self.issue(non_accepted(id.clone()));
                None
            }
            Some(n) => Some(n),
        }
    }

    /// The active targets of `kind` edges from or to `id`.
    fn active_ends(&self, kind: RelationKind, id: &Id, outgoing: bool) -> BTreeSet<Id> {
        let ids = if outgoing {
            self.graph.outgoing_edge_ids(id)
        } else {
            self.graph.incoming_edge_ids(id)
        };
        ids.iter()
            .filter_map(|e| self.graph.edge(e))
            .filter(|e| e.kind == kind && is_active(e.status))
            .map(|e| {
                if outgoing {
                    e.to.clone()
                } else {
                    e.from.clone()
                }
            })
            .collect()
    }
}

fn duplicates(ids: &[Id]) -> Vec<Id> {
    let mut sorted = ids.to_vec();
    sorted.sort();
    let mut out: Vec<Id> = sorted
        .windows(2)
        .filter(|p| p[0] == p[1])
        .map(|p| p[0].clone())
        .collect();
    out.dedup();
    out
}

/// Compiles an explicit authorization draft into at most one dry-validated HumanConfirm
/// proposal, with candidate-overlay hierarchy and separation analysis.
pub fn analyze_authorization(
    graph: &Graph,
    draft: &AuthorizationDraft,
    audit: &AuthorizationAudit,
) -> Result<AuthorizationAnalysisResult, AuthorizationError> {
    use AuthorizationIssue as I;
    let project = graph.project_id();
    let mut c = Compiler {
        graph,
        issues: Vec::new(),
        nodes: BTreeMap::new(),
        edges: BTreeMap::new(),
        declared_roles: BTreeMap::new(),
        declared_principals: BTreeSet::new(),
    };

    // ------------------------------------------------ SecurityRoles
    let mut role_ids = Vec::new();
    for r in &draft.security_roles {
        let evidence = c.evidence(&r.evidence);
        let mut ok = c.text(&r.name, |reason| I::InvalidName {
            field: "security_role.name".into(),
            reason,
        });
        if let Some(d) = &r.description {
            ok &= c.text(d, |reason| I::InvalidName {
                field: "security_role.description".into(),
                reason,
            });
        }
        let id = security_role_id(project, &r.name)?;
        role_ids.push(id.clone());
        c.declared_roles.insert(id.clone(), r.name.clone());
        for other in graph.node_ids_by_type(NodeType::SecurityRole) {
            if other == &id {
                continue;
            }
            if let Some(n) = graph.node(other) {
                if let NodePayload::SecurityRole(existing) = &n.payload {
                    if is_active(n.status) && existing.name == r.name {
                        c.issue(I::ExistingSecurityRoleNameConflict {
                            node_ref: id.clone(),
                            existing_ref: other.clone(),
                        });
                    }
                }
            }
        }
        if ok {
            let payload = NodePayload::SecurityRole(SecurityRole {
                name: r.name.clone(),
                description: r.description.clone(),
            });
            c.node(id, payload, evidence, |node_ref| {
                I::ExistingSecurityRoleConflict { node_ref }
            });
        }
    }
    for id in duplicates(&role_ids) {
        c.issue(I::DuplicateDraftIdentity { node_ref: id });
    }

    // ------------------------------------------------ Principals
    let mut principal_ids = Vec::new();
    for p in &draft.principals {
        let evidence = c.evidence(&p.evidence);
        let ok_name = c.text(&p.name, |reason| I::InvalidName {
            field: "principal.name".into(),
            reason,
        });
        let ok_kind = c.text(&p.principal_kind, |reason| I::InvalidPrincipalKind {
            reason,
        });
        let id = principal_id(project, &p.name, &p.principal_kind)?;
        principal_ids.push(id.clone());
        c.declared_principals.insert(id.clone());
        if ok_name && ok_kind {
            let payload = NodePayload::Principal(Principal {
                name: p.name.clone(),
                principal_kind: p.principal_kind.clone(),
            });
            c.node(id, payload, evidence, |node_ref| {
                I::ExistingPrincipalConflict { node_ref }
            });
        }
    }
    for id in duplicates(&principal_ids) {
        c.issue(I::DuplicateDraftIdentity { node_ref: id });
    }

    // ------------------------------------------------ Permissions
    let mut permission_ids = Vec::new();
    for p in &draft.permissions {
        let evidence = c.evidence(&p.evidence);
        let role_name = c.role("permission.security_role_ref", &p.security_role_ref);
        let operation = c.accepted(
            "permission.operation_ref",
            &p.operation_ref,
            &[NodeType::Operation],
            |reference| I::NonAcceptedOperation { reference },
        );
        let operation_name = operation.and_then(|n| match &n.payload {
            NodePayload::Operation(o) => Some(o.name.clone()),
            _ => None,
        });
        let resource_ok = c
            .accepted(
                "permission.resource_ref",
                &p.resource_ref,
                &[NodeType::Operation, NodeType::Entity, NodeType::Attribute],
                |reference| I::NonAcceptedResource { reference },
            )
            .is_some();
        let scope_ok = c.text(&p.scope_kind, |reason| I::InvalidScopeKind { reason });
        let mut condition_ids = Vec::new();
        let mut conditions = Vec::new();
        for condition in &p.conditions {
            let condition_evidence = c.evidence(&condition.evidence);
            let ok = c.text(&condition.expression, |reason| {
                I::InvalidConditionExpression { reason }
            });
            let id = policy_condition_id(project, &condition.expression)?;
            condition_ids.push(id.clone());
            if ok {
                conditions.push((id, condition.expression.clone(), condition_evidence));
            }
        }
        for id in duplicates(&condition_ids) {
            c.issue(I::DuplicateDraftIdentity { node_ref: id });
        }
        let scope_ref = resource_scope_id(project, &p.resource_ref, &p.scope_kind)?;
        let permission_ref =
            permission_id(project, &p.security_role_ref, &p.operation_ref, &scope_ref)?;
        permission_ids.push(permission_ref.clone());
        let (Some(role_name), Some(operation_name), true, true) =
            (role_name, operation_name, resource_ok, scope_ok)
        else {
            continue;
        };
        c.node(
            scope_ref.clone(),
            NodePayload::ResourceScope(ResourceScope {
                resource_ref: p.resource_ref.clone(),
                scope_kind: p.scope_kind.clone(),
            }),
            evidence.clone(),
            |node_ref| I::ExistingResourceScopeConflict { node_ref },
        );
        for (id, expression, condition_evidence) in &conditions {
            c.node(
                id.clone(),
                NodePayload::PolicyCondition(PolicyCondition {
                    expression: expression.clone(),
                }),
                condition_evidence.clone(),
                |node_ref| I::ExistingPolicyConditionConflict { node_ref },
            );
        }
        let payload = NodePayload::Permission(Permission {
            name: permission_name(&role_name, &operation_name, &p.scope_kind),
        });
        let condition_set: BTreeSet<Id> = conditions.iter().map(|(id, _, _)| id.clone()).collect();
        if let Some(existing) = graph.node(&permission_ref) {
            let equivalent = is_active(existing.status)
                && existing.payload == payload
                && c.active_ends(RelationKind::Grants, &permission_ref, false)
                    == BTreeSet::from([p.security_role_ref.clone()])
                && c.active_ends(RelationKind::Permits, &permission_ref, true)
                    == BTreeSet::from([p.operation_ref.clone()])
                && c.active_ends(RelationKind::ScopedTo, &permission_ref, true)
                    == BTreeSet::from([scope_ref.clone()])
                && c.active_ends(RelationKind::ConditionedBy, &permission_ref, true)
                    == condition_set;
            if !equivalent {
                c.issue(I::ExistingPermissionConflict {
                    node_ref: permission_ref.clone(),
                });
                continue;
            }
        } else {
            c.node(
                permission_ref.clone(),
                payload,
                evidence.clone(),
                |node_ref| I::ExistingPermissionConflict { node_ref },
            );
        }
        c.edge(
            RelationKind::Grants,
            &p.security_role_ref,
            &permission_ref,
            evidence.clone(),
        );
        c.edge(
            RelationKind::Permits,
            &permission_ref,
            &p.operation_ref,
            evidence.clone(),
        );
        c.edge(
            RelationKind::ScopedTo,
            &permission_ref,
            &scope_ref,
            evidence,
        );
        for (id, _, condition_evidence) in conditions {
            c.edge(
                RelationKind::ConditionedBy,
                &permission_ref,
                &id,
                condition_evidence,
            );
        }
    }
    for id in duplicates(&permission_ids) {
        c.issue(I::DuplicateDraftIdentity { node_ref: id });
    }

    // ------------------------------------------------ assignments
    let mut assignment_keys = Vec::new();
    for a in &draft.assignments {
        let evidence = c.evidence(&a.evidence);
        let subject_ok = if c.declared_principals.contains(&a.subject_ref) {
            true
        } else {
            c.accepted(
                "assignment.subject_ref",
                &a.subject_ref,
                &[NodeType::Principal, NodeType::Actor],
                |reference| I::NonAcceptedSubject { reference },
            )
            .is_some()
        };
        let role_ok = c
            .role("assignment.security_role_ref", &a.security_role_ref)
            .is_some();
        assignment_keys.push(edge_id(
            &RelationKind::AssignedRole,
            &a.subject_ref,
            &a.security_role_ref,
        )?);
        if subject_ok && role_ok {
            c.edge(
                RelationKind::AssignedRole,
                &a.subject_ref,
                &a.security_role_ref,
                evidence,
            );
        }
    }
    for id in duplicates(&assignment_keys) {
        c.issue(I::DuplicateDraftIdentity { node_ref: id });
    }

    // ------------------------------------------------ inheritances
    let mut inheritance_keys = Vec::new();
    let mut draft_inheritances = Vec::new();
    for h in &draft.inheritances {
        let evidence = c.evidence(&h.evidence);
        let from_ok = c.role("inheritance.role_ref", &h.role_ref).is_some();
        let to_ok = c
            .role("inheritance.inherited_role_ref", &h.inherited_role_ref)
            .is_some();
        inheritance_keys.push(edge_id(
            &RelationKind::InheritsRole,
            &h.role_ref,
            &h.inherited_role_ref,
        )?);
        if from_ok && to_ok {
            draft_inheritances.push((h.role_ref.clone(), h.inherited_role_ref.clone()));
            c.edge(
                RelationKind::InheritsRole,
                &h.role_ref,
                &h.inherited_role_ref,
                evidence,
            );
        }
    }
    for id in duplicates(&inheritance_keys) {
        c.issue(I::DuplicateDraftIdentity { node_ref: id });
    }

    // ------------------------------------------------ separation constraints
    let mut constraint_ids = Vec::new();
    let mut draft_constraints = Vec::new();
    for s in &draft.separation_constraints {
        let evidence = c.evidence(&s.evidence);
        let mut roles = s.role_refs.clone();
        roles.sort();
        let id = separation_constraint_id(project, s.constraint_kind, &roles)?;
        constraint_ids.push(id.clone());
        let mut ok = true;
        if roles.windows(2).any(|p| p[0] == p[1]) {
            c.issue(I::InvalidSeparationConstraint {
                reason: format!("{id}: duplicate role reference"),
            });
            ok = false;
        }
        if roles.len() < 2 {
            c.issue(I::InvalidSeparationConstraint {
                reason: format!("{id}: fewer than two roles"),
            });
            ok = false;
        }
        for role in &roles {
            ok &= c.role("separation_constraint.role_refs", role).is_some();
        }
        if ok {
            let constraint = SeparationConstraint {
                constraint_kind: s.constraint_kind,
                role_refs: roles,
            };
            draft_constraints.push((id.clone(), constraint.clone()));
            c.node(
                id,
                NodePayload::SeparationConstraint(constraint),
                evidence,
                |node_ref| I::ExistingSeparationConstraintConflict { node_ref },
            );
        }
    }
    for id in duplicates(&constraint_ids) {
        c.issue(I::DuplicateDraftIdentity { node_ref: id });
    }

    // ------------------------------------------------ candidate hierarchy overlay
    let active = authorization_facts(graph, false);
    let mut overlay = active.clone();
    overlay.inheritances.extend(draft_inheritances);
    overlay.inheritances.sort();
    overlay.inheritances.dedup();
    let hierarchy_cycles = role_hierarchy_cycles(&overlay.inheritances);
    for cycle in &hierarchy_cycles {
        c.issue(I::RoleHierarchyCycle {
            role_refs: cycle.clone(),
        });
    }

    let mut result = AuthorizationAnalysisResult {
        proposal: None,
        disposition: AuthorizationDisposition::Rejected,
        hierarchy_cycles,
        static_sod_violations: Vec::new(),
        unevaluated_separation_constraints: Vec::new(),
        issues: Vec::new(),
    };
    if !c.issues.is_empty() {
        c.issues.sort();
        c.issues.dedup();
        result.issues = c.issues;
        return Ok(result);
    }

    // ------------------------------------------------ proposal
    let meta = AuditMeta::new(audit.created_by.clone(), audit.created_at, None, None)
        .map_err(invalid_proposal)?;
    let order = |payload: &NodePayload| match payload {
        NodePayload::Principal(_) => 0,
        NodePayload::SecurityRole(_) => 1,
        NodePayload::ResourceScope(_) => 2,
        NodePayload::PolicyCondition(_) => 3,
        NodePayload::Permission(_) => 4,
        _ => 5,
    };
    let mut new_nodes: Vec<Node> = Vec::new();
    for (id, planned) in &c.nodes {
        let node = Node {
            id: id.clone(),
            revision: 1,
            status: ElementStatus::Proposed,
            payload: planned.item.clone(),
            evidence: planned.evidence.iter().cloned().collect(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::new(),
            audit: meta.clone(),
        };
        node.validate().map_err(invalid_proposal)?;
        new_nodes.push(node);
    }
    new_nodes.sort_by(|a, b| (order(&a.payload), &a.id).cmp(&(order(&b.payload), &b.id)));
    let mut new_edges: Vec<Edge> = Vec::new();
    for ((_, from, to), planned) in &c.edges {
        let exists = graph
            .outgoing_edge_ids(from)
            .iter()
            .filter_map(|e| graph.edge(e))
            .any(|e| e.kind == planned.item && &e.to == to && is_active(e.status));
        if exists {
            continue;
        }
        new_edges.push(Edge {
            id: edge_id(&planned.item, from, to)?,
            revision: 1,
            status: ElementStatus::Proposed,
            kind: planned.item.clone(),
            from: from.clone(),
            to: to.clone(),
            properties: RelationProperties::None,
            evidence: planned.evidence.iter().cloned().collect(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: meta.clone(),
        });
    }
    new_edges.sort_by(|a, b| a.id.cmp(&b.id));
    if new_nodes.is_empty() && new_edges.is_empty() {
        result.disposition = AuthorizationDisposition::AllExisting;
    } else {
        let evidence: BTreeSet<EvidenceRef> = new_nodes
            .iter()
            .flat_map(|n| n.evidence.iter().cloned())
            .chain(new_edges.iter().flat_map(|e| e.evidence.iter().cloned()))
            .collect();
        let patches = new_nodes
            .into_iter()
            .map(|node| SemanticPatch::AddNode { node })
            .chain(
                new_edges
                    .into_iter()
                    .map(|edge| SemanticPatch::AddEdge { edge }),
            )
            .collect();
        let proposal = Proposal::new(
            AUTHORIZATION_STAGE,
            PatchSet {
                base_semantic_hash: graph.semantic_hash()?,
                patch: SemanticPatch::Compound { patches },
            },
            evidence.into_iter().collect(),
            Vec::new(),
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .map_err(invalid_proposal)?;
        // In-memory dry validation by the canonical engine; the candidate graph is discarded.
        if let Err(e) = apply_patch(graph, &proposal.patch_set) {
            result.issues = vec![I::InvalidProposal {
                reason: format!("proposal does not apply: {e}"),
            }];
            return Ok(result);
        }
        result.disposition = AuthorizationDisposition::Proposed {
            proposal_ref: proposal.id.clone(),
        };
        result.proposal = Some(proposal);
    }

    // ------------------------------------------------ candidate separation overlay
    let mut assignments = active.assignments;
    assignments.extend(
        c.edges
            .iter()
            .filter(|(_, p)| p.item == RelationKind::AssignedRole)
            .map(|((_, from, to), _)| (from.clone(), to.clone())),
    );
    assignments.sort();
    assignments.dedup();
    let mut constraints = active.constraints;
    for (id, constraint) in draft_constraints {
        if !constraints.iter().any(|(existing, _)| existing == &id) {
            constraints.push((id, constraint));
        }
    }
    constraints.sort_by(|a, b| a.0.cmp(&b.0));
    let separation = separation_analysis(&AuthorizationFacts {
        assignments,
        inheritances: overlay.inheritances,
        constraints,
    });
    result.static_sod_violations = separation.static_sod_violations;
    result.unevaluated_separation_constraints = separation.not_evaluated;
    Ok(result)
}
