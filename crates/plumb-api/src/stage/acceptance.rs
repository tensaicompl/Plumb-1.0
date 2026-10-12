//! Output settlement and stage-proposal acceptance of A0.4 (Hotfix 067): output Cases A-D,
//! the plain X1.5 automatic commit, the governed acceptance of stage-linked proposals through
//! the unchanged A0.3 governance path and the X1.5 governed recorder, and crash reconciliation
//! that never re-commits.

use plumb_artifacts::ArtifactStore;
use plumb_compiler::{CompileRun, StageEvaluation, StageJob, StageJobError, StageJobStatus};
use plumb_core::{to_canonical_json, Hash, Id, Timestamp};
use plumb_patch::{apply_patch, AcceptancePolicy, PatchSet, Proposal};
use plumb_store::{
    CommitResult, LoadedRevision, NewWorkflowProposal, RevisionId, StageProposalLink, StageStatus,
    StoreError, WorkflowOrigin, WorkflowProposalState,
};
use serde_json::json;

use crate::error::ApiError;
use crate::state::{run_intake, ApiState, Committed};
use crate::workflow::{acceptance_decisions, classify, decode_proposal, store_kind, stored_json};

/// How a recipe step ended.
pub(crate) enum StepEnd {
    /// The step can no longer complete against its base (child and parent become stale).
    Stale,
    /// An operational failure with its frozen code.
    Failed(ApiError),
}

impl From<ApiError> for StepEnd {
    fn from(error: ApiError) -> StepEnd {
        StepEnd::Failed(error)
    }
}

pub(crate) fn internal(subsystem: &str) -> ApiError {
    ApiError::internal(subsystem)
}

/// The stored wire status of a compiler job.
pub(crate) fn status_of(job: &StageJob) -> StageStatus {
    match job.status() {
        StageJobStatus::Queued => StageStatus::Queued,
        StageJobStatus::Acquiring => StageStatus::Acquiring,
        StageJobStatus::Evaluating => StageStatus::Evaluating,
        StageJobStatus::Ready => StageStatus::Ready,
        StageJobStatus::Stale => StageStatus::Stale,
        StageJobStatus::Failed => StageStatus::Failed,
        StageJobStatus::Committed => StageStatus::Committed,
    }
}

/// Persists the child job's current state over its stored state `from`, keeping its stored
/// CompileRun artifact ref.
pub(crate) fn persist_child(
    state: &ApiState,
    job: &StageJob,
    from: StageStatus,
    at: Timestamp,
) -> Result<(), ApiError> {
    let stored = state
        .with_stage_store(|store| store.stage_job(job.id()))?
        .map_err(crate::stage::store_error)?
        .and_then(|r| r.compile_run_ref);
    persist_child_with_run(state, job, from, stored.as_ref(), at)
}

/// Persists the child job's current state with `run_ref`, the persisted CompileRun artifact
/// (the stored column references the artifact, not the run identity).
pub(crate) fn persist_child_with_run(
    state: &ApiState,
    job: &StageJob,
    from: StageStatus,
    run_ref: Option<&Hash>,
    at: Timestamp,
) -> Result<(), ApiError> {
    let value = serde_json::to_value(job).map_err(|_| internal("stage_job"))?;
    state
        .with_stage_store(|store| {
            store.update_stage_job(job.id(), from, &value, status_of(job), run_ref, at)
        })?
        .map_err(crate::stage::store_error)?;
    Ok(())
}

/// Decodes a stored compiler job: strict serde, canonical bytes, matching ID.
pub(crate) fn decode_child(text: &str, id: &Hash) -> Result<StageJob, ApiError> {
    let corrupt = || ApiError::workflow_corrupt(&format!("stage job {id}"));
    let job: StageJob = serde_json::from_str(text).map_err(|_| corrupt())?;
    if to_canonical_json(&job).map_err(|_| corrupt())? != text.as_bytes() || job.id() != id {
        return Err(corrupt());
    }
    Ok(job)
}

/// Loads and decodes the stored child `id`.
pub(crate) fn load_child(state: &ApiState, id: &Hash) -> Result<StageJob, ApiError> {
    let record = state
        .with_stage_store(|store| store.stage_job(id))?
        .map_err(|_| ApiError::workflow_corrupt(&format!("stage job {id}")))?
        .ok_or_else(|| ApiError::workflow_corrupt(&format!("stage job {id}")))?;
    decode_child(&record.stage_job_json, id)
}

/// Loads a persisted CompileRun by its artifact ref.
pub(crate) fn load_run(state: &ApiState, run_ref: &Hash) -> Result<CompileRun, ApiError> {
    let corrupt = || ApiError::workflow_corrupt(&format!("compile run {run_ref}"));
    let artifact = state
        .with_artifact_store(|store| store.get(run_ref))?
        .map_err(|_| corrupt())?;
    let run: CompileRun = serde_json::from_slice(&artifact.bytes).map_err(|_| corrupt())?;
    if to_canonical_json(&run).map_err(|_| corrupt())? != artifact.bytes {
        return Err(corrupt());
    }
    Ok(run)
}

fn is_auto(proposal: &Proposal) -> bool {
    matches!(
        proposal.acceptance_policy,
        AcceptancePolicy::AutoNonSemantic | AcceptancePolicy::AutoDerivation
    )
}

/// 409 `E_STAGE_COMMIT_FAILED` wrapping a non-staleness commit failure.
fn commit_failed(error: &ApiError) -> ApiError {
    ApiError::conflict(
        "E_STAGE_COMMIT_FAILED",
        format!(
            "the stage output could not be committed: {}",
            error.body().message
        ),
    )
    .with_details(json!({ "error": error.body().code }))
}

/// Settles one Ready child's evaluation (Cases A-D and derivation-only). Returns the committed
/// revision when an automatic commit advanced the branch.
#[allow(clippy::too_many_arguments)]
pub(crate) fn settle(
    state: &ApiState,
    recipe_job_id: &Hash,
    job: &mut StageJob,
    run_ref: &Hash,
    run: &CompileRun,
    evaluation: &StageEvaluation,
    base: &LoadedRevision,
    operation_id: &'static str,
) -> Result<Option<RevisionId>, StepEnd> {
    let at = state.now();
    let proposals = &evaluation.proposals;
    if evaluation.derivation_patch_set.is_some() && !proposals.is_empty() {
        return Err(StepEnd::Failed(ApiError::unprocessable(
            "E_STAGE_MIXED_OUTPUT",
            "the stage evaluation has both a derivation patch set and proposals",
        )));
    }
    if let Some(patch_set) = &evaluation.derivation_patch_set {
        return commit_plain(
            state,
            job,
            run,
            base,
            at,
            operation_id,
            Plain::Derivation(patch_set),
        );
    }
    match proposals.as_slice() {
        [] => Ok(None),
        [only] if is_auto(only) => commit_plain(
            state,
            job,
            run,
            base,
            at,
            operation_id,
            Plain::Proposal(only),
        ),
        _ => {
            persist_pending(state, recipe_job_id, job.id(), run_ref, proposals, base, at)?;
            Ok(None)
        }
    }
}

enum Plain<'a> {
    Proposal(&'a Proposal),
    Derivation(&'a PatchSet),
}

/// Validate, plain CAS from the child base, record (no companion, no rebase, no retry).
fn commit_plain(
    state: &ApiState,
    job: &mut StageJob,
    run: &CompileRun,
    base: &LoadedRevision,
    at: Timestamp,
    operation_id: &'static str,
    output: Plain<'_>,
) -> Result<Option<RevisionId>, StepEnd> {
    let head = state.load_head()?;
    let validated = match &output {
        Plain::Proposal(p) => job.validate_proposal_commit(&head.revision.id, run, p),
        Plain::Derivation(d) => job.validate_derivation_commit(&head.revision.id, run, d),
    };
    match validated {
        Ok(_) => {}
        Err(StageJobError::Stale { .. }) => {
            persist_child(state, job, StageStatus::Ready, at)?;
            return Err(StepEnd::Stale);
        }
        Err(e) => {
            return Err(StepEnd::Failed(ApiError::unprocessable(
                "E_STAGE_COMMIT_FAILED",
                e.to_string(),
            )))
        }
    }
    if let Plain::Proposal(proposal) = &output {
        let report = run_intake(base, proposal)?;
        if report.requires_reason {
            return Err(StepEnd::Failed(
                ApiError::conflict(
                    "E_AUTO_PROPOSAL_REQUIRES_REASON",
                    "an automatic stage proposal needs a recorded reason",
                )
                .with_details(json!({ "proposal_ref": proposal.id })),
            ));
        }
    }
    let patch_set = match &output {
        Plain::Proposal(p) => &p.patch_set,
        Plain::Derivation(d) => d,
    };
    let committed =
        match state.commit_patch_set_detailed(base, patch_set, Vec::new(), at, operation_id) {
            Ok((_, committed)) => committed,
            Err(error) if error.body().code == "E_PRECONDITION_FAILED" => {
                let head = state.load_head()?;
                job.observe_head(&head.revision.id);
                persist_child(state, job, StageStatus::Ready, at)?;
                return Err(StepEnd::Stale);
            }
            Err(error) => return Err(StepEnd::Failed(commit_failed(&error))),
        };
    let recorded = match &output {
        Plain::Proposal(p) => job.record_proposal_commit(run, p, &committed.commit),
        Plain::Derivation(d) => job.record_derivation_commit(run, d, &committed.commit),
    };
    recorded.map_err(|e| {
        StepEnd::Failed(ApiError::unprocessable(
            "E_STAGE_COMMIT_FAILED",
            e.to_string(),
        ))
    })?;
    persist_child(state, job, StageStatus::Ready, at)?;
    Ok(Some(committed.commit.revision.id))
}

/// Persists every proposal of the run pending, linked to its jobs, in one store transaction.
fn persist_pending(
    state: &ApiState,
    recipe_job_id: &Hash,
    stage_job_id: &Hash,
    run_ref: &Hash,
    proposals: &[Proposal],
    base: &LoadedRevision,
    at: Timestamp,
) -> Result<(), StepEnd> {
    let mut records = Vec::with_capacity(proposals.len());
    for proposal in proposals {
        let report = run_intake(base, proposal)?;
        let kind = classify(&base.graph, proposal)?;
        records.push(NewWorkflowProposal {
            proposal_id: proposal.id.clone(),
            base_revision_id: base.revision.id.clone(),
            proposal_json: stored_json(proposal)?,
            intake_json: stored_json(&report)?,
            proposal_kind: store_kind(kind),
            origin_kind: WorkflowOrigin::Stage,
            origin_ref: None,
            created_at: at,
        });
    }
    state
        .with_stage_store(|store| {
            store.insert_stage_proposals(recipe_job_id, stage_job_id, run_ref, &records)
        })?
        .map_err(|e| match e {
            StoreError::StageProposalConflict { id } => StepEnd::Failed(
                ApiError::conflict(
                    "E_STAGE_PROPOSAL_CONFLICT",
                    "a stage proposal ID is already stored by another origin",
                )
                .with_details(json!({ "proposal_ref": id })),
            ),
            _ => StepEnd::Failed(internal("stage_store")),
        })?;
    Ok(())
}

// ============================================================================ governed acceptance

/// A validated stage-linked acceptance in progress.
pub(crate) struct StageAcceptance {
    pub(crate) link: StageProposalLink,
    job: StageJob,
    run: CompileRun,
}

/// The stage link of `proposal`, if it is stage-produced (A0.4 runtime present).
pub(crate) fn link_of(
    state: &ApiState,
    proposal: &Id,
) -> Result<Option<StageProposalLink>, ApiError> {
    if state.stage_runtime("stage_store").is_err() {
        return Ok(None);
    }
    state
        .with_stage_store(|store| store.proposal_link(proposal))?
        .map_err(|_| ApiError::workflow_corrupt(&format!("stage proposal {proposal}")))
}

/// 409 `E_PROPOSAL_BASELINE_CHANGED` (the A0.3 code).
fn baseline_changed(proposal: &Id) -> ApiError {
    ApiError::conflict(
        "E_PROPOSAL_BASELINE_CHANGED",
        "the proposal was built on another revision; it is never rebased",
    )
    .with_details(json!({ "proposal_ref": proposal }))
}

/// Step 6 of the governed path: X1.5 freshness and exact-output validation before intake.
pub(crate) fn prepare(
    state: &ApiState,
    loaded: &LoadedRevision,
    link: StageProposalLink,
    proposal: &Proposal,
) -> Result<StageAcceptance, ApiError> {
    let mut job = load_child(state, &link.stage_job_id)?;
    let run = load_run(state, &link.compile_run_ref)?;
    let before = status_of(&job);
    match job.validate_proposal_commit(&loaded.revision.id, &run, proposal) {
        Ok(_) => Ok(StageAcceptance { link, job, run }),
        Err(StageJobError::Stale { .. }) => {
            persist_child(state, &job, before, state.now())?;
            mark_parent_stale(state, &link.recipe_job_id)?;
            Err(baseline_changed(&proposal.id))
        }
        Err(_) => Err(baseline_changed(&proposal.id)),
    }
}

/// Step 9-10: records the governed commit on the child and persists it Committed.
pub(crate) fn record(
    state: &ApiState,
    acceptance: &mut StageAcceptance,
    proposal: &Proposal,
    committed: &Committed,
) -> Result<(), ApiError> {
    acceptance
        .job
        .record_governed_proposal_commit(
            &acceptance.run,
            proposal,
            &committed.patch_set,
            &committed.commit,
        )
        .map_err(|_| internal("stage_governed_commit"))?;
    persist_child(state, &acceptance.job, StageStatus::Ready, state.now())
}

/// Step 12: the parent becomes committed with `revision` as its result.
pub(crate) fn finish_parent(
    state: &ApiState,
    recipe_job_id: &Hash,
    revision: &RevisionId,
) -> Result<(), ApiError> {
    let record = state
        .with_stage_store(|store| store.recipe_job(recipe_job_id))?
        .map_err(|_| ApiError::workflow_corrupt("stage recipe job"))?
        .ok_or_else(|| ApiError::workflow_corrupt("stage recipe job"))?;
    if !matches!(record.status, StageStatus::Ready | StageStatus::Committed) {
        return Ok(());
    }
    if record.status == StageStatus::Committed
        && record.result_revision_id.as_ref() == Some(revision)
    {
        return Ok(());
    }
    state
        .with_stage_store(|store| {
            store.transition_recipe_job(
                recipe_job_id,
                &[StageStatus::Ready, StageStatus::Committed],
                StageStatus::Committed,
                Some(revision),
                None,
                state.now(),
            )
        })?
        .map_err(crate::stage::store_error)?;
    Ok(())
}

/// A Ready parent becomes stale (its required base is gone).
pub(crate) fn mark_parent_stale(state: &ApiState, recipe_job_id: &Hash) -> Result<(), ApiError> {
    let changed = state.with_stage_store(|store| {
        store.transition_recipe_job(
            recipe_job_id,
            &[StageStatus::Ready],
            StageStatus::Stale,
            None,
            None,
            state.now(),
        )
    })?;
    match changed {
        Ok(_) | Err(StoreError::WorkflowStateConflict { .. }) => Ok(()),
        Err(_) => Err(internal("stage_store")),
    }
}

/// Reconciles an interrupted governed acceptance of a stage proposal without re-committing:
/// from the acceptance decision in `loaded`, finds the revision whose parent is the child base
/// and whose decision refs contain it, rebuilds the commit result deterministically and records
/// it; then updates the parent. Without such a revision a Ready child is observed stale.
pub(crate) fn reconcile(
    state: &ApiState,
    loaded: &LoadedRevision,
    link: &StageProposalLink,
) -> Result<(), ApiError> {
    let decisions = acceptance_decisions(&loaded.graph, &link.proposal_id);
    let [(decision_ref, _)] = decisions.as_slice() else {
        return Ok(());
    };
    let mut job = load_child(state, &link.stage_job_id)?;
    if job.status() == StageJobStatus::Committed {
        if let Some(revision) = job.committed_revision().cloned() {
            finish_parent(state, &link.recipe_job_id, &revision)?;
        }
        return Ok(());
    }
    if job.status() != StageJobStatus::Ready {
        return Ok(());
    }
    // Walk parents from the head to the revision committed from the child base.
    let base = job.base_revision().clone();
    let mut cursor = loaded.revision.clone();
    let found = loop {
        if cursor.parent.as_ref() == Some(&base) {
            break cursor
                .decision_refs
                .contains(decision_ref)
                .then_some(cursor);
        }
        if cursor.id == base {
            break None;
        }
        match cursor.parent.clone() {
            Some(parent) => cursor = state.load_revision(&parent)?.revision,
            None => break None,
        }
    };
    let Some(revision) = found else {
        job.observe_head(&loaded.revision.id);
        return persist_child(state, &job, StageStatus::Ready, state.now());
    };
    let patch_ref = revision
        .accepted_patch_ref
        .clone()
        .ok_or_else(|| ApiError::workflow_corrupt("stage acceptance revision"))?;
    let patch_set: PatchSet = {
        let artifact = state
            .with_artifact_store(|store| store.get(&patch_ref))?
            .map_err(|_| ApiError::workflow_corrupt("stage acceptance patch"))?;
        serde_json::from_slice(&artifact.bytes)
            .map_err(|_| ApiError::workflow_corrupt("stage acceptance patch"))?
    };
    let base_graph = state.load_revision(&base)?.graph;
    let delta = apply_patch(&base_graph, &patch_set)
        .map_err(|_| ApiError::workflow_corrupt("stage acceptance patch"))?
        .delta;
    let record = state
        .with_workflow_store(|store| store.proposal(&link.proposal_id))?
        .map_err(|_| ApiError::workflow_corrupt("stage proposal"))?
        .ok_or_else(|| ApiError::workflow_corrupt("stage proposal"))?;
    let proposal = decode_proposal(record)?.proposal;
    let run = load_run(state, &link.compile_run_ref)?;
    let commit = CommitResult {
        revision: revision.clone(),
        delta,
    };
    job.record_governed_proposal_commit(&run, &proposal, &patch_set, &commit)
        .map_err(|_| ApiError::workflow_corrupt("stage acceptance"))?;
    persist_child(state, &job, StageStatus::Ready, state.now())?;
    finish_parent(state, &link.recipe_job_id, &revision.id)
}

/// The startup reconciliation of every pending stage proposal: an acceptance already in the
/// graph resolves the workflow row and records the child, exactly as on first access.
pub(crate) fn reconcile_pending(state: &ApiState) -> Result<(), ApiError> {
    let loaded = state.load_head()?;
    let links = state
        .with_stage_store(|store| store.pending_proposal_links())?
        .map_err(crate::stage::store_error)?;
    for link in links {
        let decisions = acceptance_decisions(&loaded.graph, &link.proposal_id);
        if let [(_, decided_at)] = decisions.as_slice() {
            state
                .with_workflow_store(|store| {
                    store.resolve_proposal(
                        &link.proposal_id,
                        WorkflowProposalState::Accepted,
                        &loaded.revision.id,
                        *decided_at,
                    )
                })?
                .map_err(|_| internal("workflow_store"))?;
            reconcile(state, &loaded, &link)?;
        }
    }
    Ok(())
}
