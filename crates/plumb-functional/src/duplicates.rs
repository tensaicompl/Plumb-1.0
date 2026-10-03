//! Deterministic duplicate detection and governed resolution proposals (plan S1.3; compiler
//! architecture §7).
//!
//! Same-scope Proposed/Accepted Requirement pairs are compared by the exact Jaccard fraction of
//! their normalized current statements against a caller-supplied threshold. Accepted/Accepted
//! matches yield F1 finding material through the existing `GeneratedFinding` identity. A merge
//! is proposed only for a lossless exact duplicate that the canonical patch engine accepts, and
//! a supersession only from a governed Accepted human `ResolutionDecision`. Both are
//! MaterialDecision / HUMAN_DECISION proposals; nothing is applied, committed or persisted.
//! `apply_patch` is used solely to dry-validate a candidate proposal in memory.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId};
use plumb_patch::{
    apply_patch, AcceptancePolicy, ElementPrecondition, MergePolicy, PatchError, PatchSet,
    Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    node_element_hash, AuditMeta, Edge, ElementStatus, EvidenceRef, Graph, Node, NodePayload,
    NodeType, RelationKind, RelationProperties, Requirement,
};
use plumb_validation::{
    load_builtin_software_profile, GeneratedFinding, RuleMetadata, ViolationFacts,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

/// The F1 rule whose finding material Accepted/Accepted duplicates produce.
const DUPLICATE_RULE_ID: &str = "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED";

/// The semantic condition key of every duplicate finding.
const DUPLICATE_CONDITION_KEY: &str = "active_requirement_duplicate";

const SUGGESTED_RESOLUTION: &str = "Review the pair and either merge a lossless exact duplicate, \
supersede a replaced requirement through a governed decision, or resolve/waive the finding as \
intentionally distinct.";

/// The `ResolutionDecision.answer` kind that directs a requirement supersession.
const SUPERSESSION_KIND: &str = "requirement_supersession";

// ============================================================================ errors

/// Why duplicate analysis could not run. An unavailable merge is not an error.
#[derive(Debug, Error)]
pub enum DuplicateError {
    /// The duplicate policy is invalid.
    #[error("invalid duplicate policy: {reason}")]
    InvalidPolicy { reason: String },
    /// The graph is not valid analysis input.
    #[error("invalid duplicate analysis input: {reason}")]
    InvalidInput { reason: String },
    /// The built-in validation profile or its duplicate rule is unavailable.
    #[error("validation profile: {reason}")]
    ValidationProfile { reason: String },
    /// Finding material could not be generated.
    #[error("generated finding: {reason}")]
    GeneratedFinding { reason: String },
    /// An Accepted supersession decision is malformed, ungoverned or not applicable.
    #[error("invalid supersession decision {decision_ref}: {reason}")]
    InvalidSupersessionDecision { decision_ref: Id, reason: String },
    /// Several Accepted decisions direct the supersession of the same requirement.
    #[error("ambiguous supersession of {old_ref}: decisions {decision_refs:?}")]
    AmbiguousSupersessionDecision { old_ref: Id, decision_refs: Vec<Id> },
    /// A proposal could not be built.
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    /// Canonicalization or hashing failed.
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn invalid_proposal(e: impl ToString) -> DuplicateError {
    DuplicateError::InvalidProposal {
        reason: e.to_string(),
    }
}

// ============================================================================ policy and score

/// The profile-resolved duplicate policy, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicatePolicy {
    pub jaccard_threshold: Decimal,
}

impl DuplicatePolicy {
    /// Requires `0 < jaccard_threshold <= 1`.
    pub fn validate(&self) -> Result<(), DuplicateError> {
        if self.jaccard_threshold > Decimal::ZERO && self.jaccard_threshold <= Decimal::ONE {
            Ok(())
        } else {
            Err(DuplicateError::InvalidPolicy {
                reason: format!(
                    "jaccard_threshold {} is not in (0, 1]",
                    self.jaccard_threshold
                ),
            })
        }
    }
}

/// The exact Jaccard fraction of two token sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JaccardScore {
    pub intersection: u32,
    pub union: u32,
}

impl JaccardScore {
    /// `intersection / union` for presentation; matching never uses it.
    pub fn as_decimal(&self) -> Option<Decimal> {
        (self.union > 0).then(|| Decimal::from(self.intersection) / Decimal::from(self.union))
    }

    /// `union > 0` and `intersection / union >= threshold`, compared exactly as
    /// `intersection >= threshold * union`.
    fn meets(&self, threshold: Decimal) -> bool {
        self.union > 0 && Decimal::from(self.intersection) >= threshold * Decimal::from(self.union)
    }
}

// ============================================================================ vocabulary

/// Whether the normalized token sequences are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateMatchKind {
    Exact,
    Near,
}

/// Whether and why a duplicate pair can be resolved by a proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case", deny_unknown_fields)]
pub enum MergeDisposition {
    /// A near match needs human review before any consolidation.
    ReviewOnly,
    /// An exact match whose payloads differ cannot be merged losslessly.
    UnavailablePayloadDifference,
    /// The canonical patch engine rejected the merge on this extension key.
    UnavailableExtensionConflict { key: String },
    /// The canonical patch engine rejected the merge; `reason` is diagnostic only.
    UnavailablePatchConstraint { reason: String },
    /// A dry-validated MergeNodes proposal was produced.
    MergeProposed { proposal_ref: Id },
    /// A governed supersession decision directs this pair instead of a merge.
    SupersessionDirected { decision_ref: Id, proposal_ref: Id },
}

/// One active duplicate pair, `left_ref < right_ref`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicateMatch {
    pub left_ref: Id,
    pub right_ref: Id,
    pub match_kind: DuplicateMatchKind,
    pub score: JaccardScore,
    pub merge_disposition: MergeDisposition,
}

/// A merge or supersession proposal with the basis it was built from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "resolution", rename_all = "snake_case")]
pub enum DuplicateResolutionProposal {
    Merge {
        left_ref: Id,
        right_ref: Id,
        proposal: Proposal,
    },
    Supersede {
        old_ref: Id,
        new_ref: Id,
        decision_ref: Id,
        proposal: Proposal,
    },
}

impl DuplicateResolutionProposal {
    /// Merges by (left, right) first, then supersessions by (old, new, decision).
    fn order_key(&self) -> (u8, &Id, &Id, Option<&Id>) {
        match self {
            DuplicateResolutionProposal::Merge {
                left_ref,
                right_ref,
                ..
            } => (0, left_ref, right_ref, None),
            DuplicateResolutionProposal::Supersede {
                old_ref,
                new_ref,
                decision_ref,
                ..
            } => (1, old_ref, new_ref, Some(decision_ref)),
        }
    }
}

/// Matches sorted by pair, findings sorted unique by ID, proposals in canonical order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DuplicateAnalysisResult {
    pub matches: Vec<DuplicateMatch>,
    pub findings: Vec<GeneratedFinding>,
    pub proposals: Vec<DuplicateResolutionProposal>,
}

// ============================================================================ normalization

/// NFKC, Unicode lowercase, maximal runs of alphanumeric characters; nothing else is removed.
fn tokens(statement: &str) -> Vec<String> {
    let normalized: String = statement.nfkc().collect::<String>().to_lowercase();
    normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

/// One active Requirement as analysed.
struct Active<'g> {
    node: &'g Node,
    requirement: &'g Requirement,
    sequence: Vec<String>,
    set: BTreeSet<String>,
}

fn scope_set(refs: &Option<Vec<Id>>) -> BTreeSet<&Id> {
    refs.iter().flatten().collect()
}

/// Equal kind, level and modality, and equal owner/stakeholder sets (None == Some([])).
fn same_scope(a: &Requirement, b: &Requirement) -> bool {
    a.requirement_kind == b.requirement_kind
        && a.level == b.level
        && a.modality == b.modality
        && scope_set(&a.owner_refs) == scope_set(&b.owner_refs)
        && scope_set(&a.stakeholder_refs) == scope_set(&b.stakeholder_refs)
}

fn score(a: &BTreeSet<String>, b: &BTreeSet<String>) -> JaccardScore {
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    JaccardScore {
        intersection: u32::try_from(intersection).unwrap_or(u32::MAX),
        union: u32::try_from(union).unwrap_or(u32::MAX),
    }
}

fn active_requirements(graph: &Graph) -> Vec<Active<'_>> {
    graph
        .node_ids_by_type(NodeType::Requirement)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|node| {
            matches!(
                node.status,
                ElementStatus::Proposed | ElementStatus::Accepted
            )
        })
        .filter_map(|node| match &node.payload {
            NodePayload::Requirement(requirement) => {
                let sequence = tokens(&requirement.statement);
                let set = sequence.iter().cloned().collect();
                Some(Active {
                    node,
                    requirement,
                    sequence,
                    set,
                })
            }
            _ => None,
        })
        .collect()
}

// ============================================================================ findings

fn duplicate_rule() -> Result<(plumb_core::Id, RuleMetadata), DuplicateError> {
    let profile =
        load_builtin_software_profile().map_err(|e| DuplicateError::ValidationProfile {
            reason: e.to_string(),
        })?;
    let rule = profile
        .rules
        .into_iter()
        .find(|rule| rule.id == DUPLICATE_RULE_ID)
        .ok_or_else(|| DuplicateError::ValidationProfile {
            reason: format!("profile has no rule {DUPLICATE_RULE_ID}"),
        })?;
    Ok((profile.profile_id, rule))
}

fn duplicate_finding(
    rule: &RuleMetadata,
    left: &Id,
    right: &Id,
    kind: DuplicateMatchKind,
    score: JaccardScore,
) -> Result<GeneratedFinding, DuplicateError> {
    let message = match kind {
        DuplicateMatchKind::Exact => format!(
            "Accepted requirements {left} and {right} have identical normalized obligation text."
        ),
        DuplicateMatchKind::Near => format!(
            "Accepted requirements {left} and {right} meet the configured duplicate Jaccard \
             threshold ({}/{}).",
            score.intersection, score.union
        ),
    };
    let targets = [left.clone(), right.clone()];
    GeneratedFinding::for_violation(
        rule,
        rule.severity,
        &ViolationFacts {
            targets: &targets,
            semantic_condition_key: DUPLICATE_CONDITION_KEY,
            message: &message,
            suggested_resolution: Some(SUGGESTED_RESOLUTION),
        },
        None,
    )
    .map_err(|e| DuplicateError::GeneratedFinding {
        reason: e.to_string(),
    })
}

// ============================================================================ proposals

fn precondition(node: &Node) -> Result<ElementPrecondition, DuplicateError> {
    Ok(ElementPrecondition {
        id: node.id.clone(),
        expected_hash: node_element_hash(node)?,
    })
}

fn evidence_union(a: &Node, b: &Node) -> Vec<EvidenceRef> {
    let refs: BTreeSet<&EvidenceRef> = a.evidence.iter().chain(&b.evidence).collect();
    refs.into_iter().cloned().collect()
}

fn material_proposal(
    patch_set: PatchSet,
    evidence_refs: Vec<EvidenceRef>,
) -> Result<Proposal, DuplicateError> {
    Proposal::new(
        StageId::S1,
        patch_set,
        evidence_refs,
        Vec::new(),
        ProposalMateriality::MaterialDecision,
        AcceptancePolicy::HumanDecision,
        None,
    )
    .map_err(invalid_proposal)
}

/// The MergeNodes proposal for a payload-identical exact pair, or why none is available.
fn merge_proposal(
    graph: &Graph,
    base: &Hash,
    a: &Active<'_>,
    b: &Active<'_>,
) -> Result<Result<Proposal, MergeDisposition>, DuplicateError> {
    if a.requirement != b.requirement {
        return Ok(Err(MergeDisposition::UnavailablePayloadDifference));
    }
    let (keep, other) = match (a.node.status, b.node.status) {
        (ElementStatus::Accepted, ElementStatus::Proposed) => (a.node, b.node),
        (ElementStatus::Proposed, ElementStatus::Accepted) => (b.node, a.node),
        _ if a.node.id < b.node.id => (a.node, b.node),
        _ => (b.node, a.node),
    };
    let patch_set = PatchSet {
        base_semantic_hash: base.clone(),
        patch: SemanticPatch::MergeNodes {
            keep: precondition(keep)?,
            merge: vec![precondition(other)?],
            field_policy: MergePolicy::KeepPayloadUnionMetadata,
        },
    };
    let proposal = material_proposal(patch_set, evidence_union(keep, other))?;
    // In-memory dry validation by the canonical engine; the candidate graph is discarded.
    Ok(match apply_patch(graph, &proposal.patch_set) {
        Ok(_) => Ok(proposal),
        Err(PatchError::MergeExtensionConflict { key }) => {
            Err(MergeDisposition::UnavailableExtensionConflict { key })
        }
        Err(e) => Err(MergeDisposition::UnavailablePatchConstraint {
            reason: e.to_string(),
        }),
    })
}

/// The exact supersession marker of a `ResolutionDecision.answer`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupersessionMarker {
    kind: String,
    old_ref: Id,
    new_ref: Id,
}

/// A qualifying Accepted supersession decision.
struct Supersession<'g> {
    decision: &'g Node,
    old: &'g Node,
    new: &'g Node,
}

fn is_clean_text(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

/// Every Accepted decision whose answer kind is `requirement_supersession`, validated; other
/// answers, non-Accepted decisions and decisions already carried out are ignored.
fn supersession_decisions(graph: &Graph) -> Result<Vec<Supersession<'_>>, DuplicateError> {
    let mut decisions = Vec::new();
    for id in graph.node_ids_by_type(NodeType::ResolutionDecision) {
        let Some(node) = graph.node(id) else {
            continue;
        };
        let NodePayload::ResolutionDecision(decision) = &node.payload else {
            continue;
        };
        if node.status != ElementStatus::Accepted
            || decision.answer.get("kind") != Some(&Value::from(SUPERSESSION_KIND))
        {
            continue;
        }
        let invalid = |reason: &str| DuplicateError::InvalidSupersessionDecision {
            decision_ref: id.clone(),
            reason: reason.to_owned(),
        };
        let marker: SupersessionMarker = serde_json::from_value(decision.answer.clone())
            .map_err(|e| invalid(&format!("malformed supersession marker: {e}")))?;
        debug_assert_eq!(marker.kind, SUPERSESSION_KIND);
        if marker.old_ref == marker.new_ref {
            return Err(invalid("old_ref equals new_ref"));
        }
        if !decision.rationale.as_deref().is_some_and(is_clean_text) {
            return Err(invalid("rationale is missing or not clean text"));
        }
        let governed = graph.outgoing_edge_ids(id).iter().any(|edge_id| {
            graph.edge(edge_id).is_some_and(|edge| {
                edge.kind == RelationKind::Resolves
                    && edge.status == ElementStatus::Accepted
                    && graph.node(&edge.to).is_some_and(|target| {
                        target.status == ElementStatus::Accepted
                            && matches!(
                                target.payload,
                                NodePayload::Finding(_) | NodePayload::Question(_)
                            )
                    })
            })
        });
        if !governed {
            return Err(invalid(
                "no Accepted resolves edge to an Accepted Finding or Question",
            ));
        }
        let accepted_requirement = |reference: &Id, role: &str| {
            graph
                .node(reference)
                .filter(|n| {
                    n.status == ElementStatus::Accepted
                        && matches!(n.payload, NodePayload::Requirement(_))
                })
                .ok_or_else(|| {
                    invalid(&format!(
                        "{role} {reference} is not an Accepted Requirement"
                    ))
                })
        };
        // A decision whose old requirement is already Superseded has been carried out; it is
        // no longer an eligible supersession action and produces nothing.
        if graph
            .node(&marker.old_ref)
            .is_some_and(|n| n.status == ElementStatus::Superseded)
        {
            continue;
        }
        let old = accepted_requirement(&marker.old_ref, "old_ref")?;
        let new = accepted_requirement(&marker.new_ref, "new_ref")?;
        decisions.push(Supersession {
            decision: node,
            old,
            new,
        });
    }
    let mut by_old: BTreeMap<&Id, Vec<&Id>> = BTreeMap::new();
    for s in &decisions {
        by_old.entry(&s.old.id).or_default().push(&s.decision.id);
    }
    if let Some((old_ref, decision_refs)) = by_old.into_iter().find(|(_, d)| d.len() > 1) {
        return Err(DuplicateError::AmbiguousSupersessionDecision {
            old_ref: old_ref.clone(),
            decision_refs: decision_refs.into_iter().cloned().collect(),
        });
    }
    Ok(decisions)
}

/// `rel:<first 16 hex of SHA-256(RFC 8785 {kind: supersedes, from: new, to: old})>`.
fn supersedes_edge_id(new_ref: &Id, old_ref: &Id) -> Result<Id, CoreError> {
    let body = json!({"kind": "supersedes", "from": new_ref, "to": old_ref});
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("rel:{}", &hex[..16]).parse()
}

/// The dry-validated Supersede proposal a governed decision directs.
fn supersede_proposal(
    graph: &Graph,
    base: &Hash,
    s: &Supersession<'_>,
) -> Result<Proposal, DuplicateError> {
    let NodePayload::ResolutionDecision(decision) = &s.decision.payload else {
        return Err(invalid_proposal("decision is not a ResolutionDecision"));
    };
    let invalid = |reason: String| DuplicateError::InvalidSupersessionDecision {
        decision_ref: s.decision.id.clone(),
        reason,
    };
    let evidence = evidence_union(s.old, s.new);
    let edge = Edge {
        id: supersedes_edge_id(&s.new.id, &s.old.id)?,
        revision: 1,
        status: ElementStatus::Accepted,
        kind: RelationKind::Supersedes,
        from: s.new.id.clone(),
        to: s.old.id.clone(),
        properties: RelationProperties::None,
        evidence: evidence.clone(),
        derivations: Vec::new(),
        standards: Vec::new(),
        audit: AuditMeta::new(decision.decided_by.clone(), decision.decided_at, None, None)
            .map_err(|e| invalid(e.to_string()))?,
    };
    let patch_set = PatchSet {
        base_semantic_hash: base.clone(),
        patch: SemanticPatch::Supersede {
            old: precondition(s.old)?,
            new: precondition(s.new)?,
            edge,
        },
    };
    let proposal = material_proposal(patch_set, evidence)?;
    // In-memory dry validation by the canonical engine; the candidate graph is discarded.
    apply_patch(graph, &proposal.patch_set)
        .map_err(|e| invalid(format!("supersession cannot be applied: {e}")))?;
    Ok(proposal)
}

// ============================================================================ analysis

/// Detects active duplicate pairs and builds finding material and governed proposals.
pub fn analyze_requirement_duplicates(
    graph: &Graph,
    policy: &DuplicatePolicy,
) -> Result<DuplicateAnalysisResult, DuplicateError> {
    policy.validate()?;
    let (profile_id, rule) = duplicate_rule()?;
    if graph.profile_id() != &profile_id {
        return Err(DuplicateError::InvalidInput {
            reason: format!(
                "graph profile {} is not the built-in profile {profile_id}",
                graph.profile_id()
            ),
        });
    }
    let base = graph.semantic_hash()?;
    let mut proposals = Vec::new();

    let mut directed: BTreeMap<(&Id, &Id), (Id, Id)> = BTreeMap::new();
    let decisions = supersession_decisions(graph)?;
    for s in &decisions {
        let proposal = supersede_proposal(graph, &base, s)?;
        let pair = if s.old.id < s.new.id {
            (&s.old.id, &s.new.id)
        } else {
            (&s.new.id, &s.old.id)
        };
        directed.insert(pair, (s.decision.id.clone(), proposal.id.clone()));
        proposals.push(DuplicateResolutionProposal::Supersede {
            old_ref: s.old.id.clone(),
            new_ref: s.new.id.clone(),
            decision_ref: s.decision.id.clone(),
            proposal,
        });
    }

    let active = active_requirements(graph);
    let mut matches = Vec::new();
    let mut findings = BTreeMap::new();
    for (i, a) in active.iter().enumerate() {
        for b in &active[i + 1..] {
            // node_ids_by_type is sorted, so a.id < b.id.
            if a.set.is_empty() || b.set.is_empty() || !same_scope(a.requirement, b.requirement) {
                continue;
            }
            let score = score(&a.set, &b.set);
            if !score.meets(policy.jaccard_threshold) {
                continue;
            }
            let kind = if a.sequence == b.sequence {
                DuplicateMatchKind::Exact
            } else {
                DuplicateMatchKind::Near
            };
            let disposition = if let Some((decision_ref, proposal_ref)) =
                directed.get(&(&a.node.id, &b.node.id))
            {
                MergeDisposition::SupersessionDirected {
                    decision_ref: decision_ref.clone(),
                    proposal_ref: proposal_ref.clone(),
                }
            } else if kind == DuplicateMatchKind::Near {
                MergeDisposition::ReviewOnly
            } else {
                match merge_proposal(graph, &base, a, b)? {
                    Ok(proposal) => {
                        let proposal_ref = proposal.id.clone();
                        proposals.push(DuplicateResolutionProposal::Merge {
                            left_ref: a.node.id.clone(),
                            right_ref: b.node.id.clone(),
                            proposal,
                        });
                        MergeDisposition::MergeProposed { proposal_ref }
                    }
                    Err(disposition) => disposition,
                }
            };
            if a.node.status == ElementStatus::Accepted && b.node.status == ElementStatus::Accepted
            {
                let finding = duplicate_finding(&rule, &a.node.id, &b.node.id, kind, score)?;
                findings.insert(finding.id.clone(), finding);
            }
            matches.push(DuplicateMatch {
                left_ref: a.node.id.clone(),
                right_ref: b.node.id.clone(),
                match_kind: kind,
                score,
                merge_disposition: disposition,
            });
        }
    }
    matches.sort_by(|x, y| (&x.left_ref, &x.right_ref).cmp(&(&y.left_ref, &y.right_ref)));
    proposals.sort_by(|x, y| x.order_key().cmp(&y.order_key()));
    Ok(DuplicateAnalysisResult {
        matches,
        findings: findings.into_values().collect(),
        proposals,
    })
}
