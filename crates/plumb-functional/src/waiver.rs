//! Governed waiver proposals (plan S3.4, Hotfix 048; compiler architecture §9).
//!
//! Waiver governance has two deterministic layers: plumb-validation prepares and validates the
//! canonical waiver material (finding identity, policy, actor, rationale, existing waivers,
//! marker, subject, artifact, decision and resolves edge) without mutation or proposal
//! capabilities, and this module only wraps that exact material in a human-decision proposal and
//! dry-runs it through the patch engine. No waiver identity is recomputed here.

use plumb_core::{Id, StageId};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{Edge, Graph, ResolutionDecision};
use plumb_validation::evaluator::ValidationPolicy;
use plumb_validation::waiver::{
    prepare_waiver_material, WaiverCreationError, WaiverPatchArtifact, WaiverRequest,
};
use thiserror::Error;

const WAIVER_STAGE: StageId = StageId::S3;

/// Why a waiver proposal cannot be created.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WaiverError {
    /// The validation layer rejected the request; nothing was proposed.
    #[error(transparent)]
    Material(#[from] WaiverCreationError),
    #[error("waiver proposal is invalid: {reason}")]
    InvalidWaiverProposal { reason: String },
}

/// One proposed waiver; nothing has been applied.
#[derive(Debug, Clone, PartialEq)]
pub struct WaiverCreationResult {
    pub decision_ref: Id,
    pub decision: ResolutionDecision,
    pub resolves_edge: Edge,
    pub artifact: WaiverPatchArtifact,
    /// The canonical artifact bytes to persist; their hash is `decision.patch_ref`.
    pub artifact_bytes: Vec<u8>,
    pub proposal: Proposal,
}

/// Builds the human-decision proposal adding exactly the prepared waiver decision and its
/// resolves edge, dry-run through the patch engine.
pub fn create_waiver(
    graph: &Graph,
    request: &WaiverRequest<'_>,
    policy: &ValidationPolicy,
) -> Result<WaiverCreationResult, WaiverError> {
    let material = prepare_waiver_material(graph, request, policy)?;
    let invalid = |reason: String| WaiverError::InvalidWaiverProposal { reason };
    let proposal = Proposal::new(
        WAIVER_STAGE,
        PatchSet {
            base_semantic_hash: graph.semantic_hash().map_err(|e| invalid(e.to_string()))?,
            patch: SemanticPatch::Compound {
                patches: vec![
                    SemanticPatch::AddNode {
                        node: material.decision_node.clone(),
                    },
                    SemanticPatch::AddEdge {
                        edge: material.resolves_edge.clone(),
                    },
                ],
            },
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
    Ok(WaiverCreationResult {
        decision_ref: material.decision_ref,
        decision: material.decision,
        resolves_edge: material.resolves_edge,
        artifact: material.artifact,
        artifact_bytes: material.artifact_bytes,
        proposal,
    })
}
