//! Output settlement and stage-proposal acceptance of A0.4 (Hotfix 067): output Cases A-D,
//! the plain X1.5 automatic commit, the governed acceptance of stage-linked proposals through
//! the unchanged A0.3 governance path and the X1.5 governed recorder, and crash reconciliation
//! that never re-commits.

use plumb_artifacts::ArtifactStore;
use plumb_compiler::{CompileRun, StageEvaluation, StageJob, StageJobError, StageJobStatus};
use plumb_core::{to_canonical_json, Hash, Id, Timestamp};
use plumb_patch::{apply_patch, AcceptancePolicy, PatchSet, Proposal, SemanticPatch};
use plumb_psg::NodePayload;
use plumb_store::{
    CommitResult, LoadedRevision, NewWorkflowProposal, RevisionId, StageProposalLink, StageStatus,
    StoreError, WorkflowOrigin, WorkflowProposalState,
};
use serde_json::json;

use crate::error::ApiError;
use crate::state::{run_intake, ApiState, Committed};
use crate::workflow::{acceptance_decisions, classify, decode_proposal, store_kind, stored_json};
use plumb_functional::resolution::ACCEPTANCE_DECISION_KIND;

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
    let artifact = state.get_artifact(run_ref)?.map_err(|_| corrupt())?;
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

/// The effect of a patch independent of its baseline, audit and element-hash preconditions:
/// the canonical JSON of each leaf's operation, target ID and new payload or status.
fn effect_key(patch: &plumb_patch::SemanticPatch) -> Result<String, StepEnd> {
    use plumb_patch::SemanticPatch as P;
    fn leaves(patch: &P, out: &mut Vec<serde_json::Value>) {
        match patch {
            P::Compound { patches } => patches.iter().for_each(|p| leaves(p, out)),
            P::AddNode { node } => out.push(json!({"op": "add_node", "id": node.id,
                "status": node.status, "payload": node.payload})),
            P::AddEdge { edge } => out.push(json!({"op": "add_edge", "id": edge.id,
                "status": edge.status, "kind": edge.kind, "from": edge.from, "to": edge.to})),
            other => out.push(serde_json::to_value(other).unwrap_or(serde_json::Value::Null)),
        }
    }
    let mut out = Vec::new();
    leaves(patch, &mut out);
    serde_json::to_string(&out).map_err(|_| StepEnd::Failed(internal("effect_key")))
}

/// The effects of every stage proposal the analyst rejected on the revision it was built on
/// (a content decision). Rejecting an already stale proposal is clean-up and is not remembered.
fn rejected_effects(state: &ApiState) -> Result<std::collections::BTreeSet<String>, StepEnd> {
    let rejected = state
        .with_workflow_store(|store| store.rejected_proposals(WorkflowOrigin::Stage))?
        .map_err(|_| StepEnd::Failed(internal("workflow_store")))?;
    let mut effects = std::collections::BTreeSet::new();
    for record in rejected {
        if record.resolved_revision_id.as_ref() != Some(&record.base_revision_id) {
            continue;
        }
        let proposal: Proposal = serde_json::from_str(&record.proposal_json)
            .map_err(|_| StepEnd::Failed(internal("workflow_proposal")))?;
        effects.insert(effect_key(&proposal.patch_set.patch)?);
    }
    Ok(effects)
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
    let remembered = rejected_effects(state)?;
    let mut records = Vec::with_capacity(proposals.len());
    for proposal in proposals {
        // Rejection memory (H0.10): the analyst already rejected exactly this change.
        if remembered.contains(&effect_key(&proposal.patch_set.patch)?) {
            continue;
        }
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

/// The stage link of the occurrence of `proposal` on `base`, if it is stage-produced (A0.4
/// runtime present).
pub(crate) fn link_of(
    state: &ApiState,
    proposal: &Id,
    base: &RevisionId,
) -> Result<Option<StageProposalLink>, ApiError> {
    if state.stage_runtime("stage_store").is_err() {
        return Ok(None);
    }
    state
        .with_stage_store(|store| store.proposal_link(proposal, base))?
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
            .get_artifact(&patch_ref)?
            .map_err(|_| ApiError::workflow_corrupt("stage acceptance patch"))?;
        serde_json::from_slice(&artifact.bytes)
            .map_err(|_| ApiError::workflow_corrupt("stage acceptance patch"))?
    };
    if set_members(&patch_set).is_some_and(|ids| ids.len() > 1) {
        // R0.3: a proposal-set acceptance records every batch-accepted output of every
        // affected child at once (never one sibling at a time).
        return reconcile_set(state, loaded, &revision, &patch_set);
    }
    let base_graph = state.load_revision(&base)?.graph;
    let delta = apply_patch(&base_graph, &patch_set)
        .map_err(|_| ApiError::workflow_corrupt("stage acceptance patch"))?
        .delta;
    let record = state
        .with_workflow_store(|store| store.proposal(&link.proposal_id, &link.base_revision_id))?
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
        // An acceptance always moves the head: an occurrence on the head has none to reconcile.
        if link.base_revision_id == loaded.revision.id {
            continue;
        }
        let decisions = acceptance_decisions(&loaded.graph, &link.proposal_id);
        if let [(decision_ref, decided_at)] = decisions.as_slice() {
            // A set reconciliation of an earlier link may already have resolved this row.
            if is_pending(state, &link.proposal_id, &link.base_revision_id)? {
                // Fable #196: resolved with the revision that committed the decision.
                let accepted = decision_revision(state, &loaded.revision, decision_ref)?;
                state
                    .with_workflow_store(|store| {
                        store.resolve_proposal(
                            &link.proposal_id,
                            &link.base_revision_id,
                            WorkflowProposalState::Accepted,
                            &accepted,
                            *decided_at,
                        )
                    })?
                    .map_err(|_| internal("workflow_store"))?;
            }
            reconcile(state, &loaded, &link)?;
        }
    }
    Ok(())
}

/// Plain stage commit reconciliation (H0.10, §33.11), run on startup before any in-flight job
/// is interrupted: every Ready child of an in-flight recipe job whose plain (automatic or
/// derivation) output was already committed is settled to that revision — found by walking the
/// branch from the head to the revision whose parent is the child base and whose
/// `accepted_patch_ref` is the output PatchSet — and its parent becomes Committed with the
/// last settled revision. Nothing is committed again; committed work is never failed.
pub(crate) fn settle_plain_commits(state: &ApiState) -> Result<(), ApiError> {
    let head = state.load_head()?;
    let parents = state
        .with_stage_store(|store| store.recipe_jobs())?
        .map_err(crate::stage::store_error)?;
    for parent in parents.into_iter().filter(|r| r.status.is_in_flight()) {
        let steps = state
            .with_stage_store(|store| store.steps(&parent.recipe_job_id))?
            .map_err(crate::stage::store_error)?;
        let mut settled = None;
        for step in &steps {
            let Some(child_id) = &step.stage_job_id else {
                continue;
            };
            if let Some(revision) = settle_plain_child(state, &head, child_id)? {
                state
                    .with_stage_store(|store| {
                        store.update_step(
                            &parent.recipe_job_id,
                            step.ordinal,
                            None,
                            None,
                            None,
                            None,
                            Some(&revision),
                        )
                    })?
                    .map_err(crate::stage::store_error)?;
                settled = Some(revision);
            }
        }
        if let Some(revision) = settled {
            state
                .with_stage_store(|store| {
                    store.transition_recipe_job(
                        &parent.recipe_job_id,
                        &[
                            StageStatus::Queued,
                            StageStatus::Acquiring,
                            StageStatus::Evaluating,
                        ],
                        StageStatus::Committed,
                        Some(&revision),
                        None,
                        state.now(),
                    )
                })?
                .map_err(crate::stage::store_error)?;
        }
    }
    Ok(())
}

/// Settles one Ready child whose plain output is already committed on the branch; returns the
/// committed revision.
fn settle_plain_child(
    state: &ApiState,
    head: &LoadedRevision,
    child_id: &Hash,
) -> Result<Option<RevisionId>, ApiError> {
    let record = state
        .with_stage_store(|store| store.stage_job(child_id))?
        .map_err(crate::stage::store_error)?
        .ok_or_else(|| ApiError::workflow_corrupt(&format!("stage job {child_id}")))?;
    let (StageStatus::Ready, Some(run_ref)) = (record.status, &record.compile_run_ref) else {
        return Ok(None);
    };
    let mut job = decode_child(&record.stage_job_json, child_id)?;
    let run = load_run(state, run_ref)?;
    let corrupt = || ApiError::workflow_corrupt(&format!("stage job output {child_id}"));
    let output = if let Some(patch_ref) = &run.output_derivation_patch_ref {
        let artifact = state
            .with_artifact_store(|store| store.get(patch_ref))?
            .map_err(|_| corrupt())?;
        let patch_set: PatchSet = serde_json::from_slice(&artifact.bytes).map_err(|_| corrupt())?;
        PlainOutput::Derivation(patch_set)
    } else if let [proposal_ref] = run.output_proposal_refs.as_slice() {
        let artifact = state
            .with_artifact_store(|store| store.get(proposal_ref))?
            .map_err(|_| corrupt())?;
        let proposal: Proposal = serde_json::from_slice(&artifact.bytes).map_err(|_| corrupt())?;
        if !is_auto(&proposal) {
            return Ok(None);
        }
        PlainOutput::Proposal(proposal)
    } else {
        return Ok(None);
    };
    let patch_set = match &output {
        PlainOutput::Proposal(p) => &p.patch_set,
        PlainOutput::Derivation(d) => d,
    };
    let accepted = Hash::content_sha256(&to_canonical_json(patch_set).map_err(|_| corrupt())?);
    let base = job.base_revision().clone();
    let Some(revision) = state.find_on_branch(&head.revision, Some(&base), |r| {
        r.parent.as_ref() == Some(&base) && r.accepted_patch_ref.as_ref() == Some(&accepted)
    })?
    else {
        return Ok(None);
    };
    let delta = apply_patch(&state.load_revision(&base)?.graph, patch_set)
        .map_err(|_| corrupt())?
        .delta;
    let commit = CommitResult {
        revision: revision.clone(),
        delta,
    };
    match &output {
        PlainOutput::Proposal(p) => job.record_proposal_commit(&run, p, &commit),
        PlainOutput::Derivation(d) => job.record_derivation_commit(&run, d, &commit),
    }
    .map_err(|_| corrupt())?;
    persist_child(state, &job, StageStatus::Ready, state.now())?;
    Ok(Some(revision.id))
}

/// A decoded plain output of a persisted run.
enum PlainOutput {
    Proposal(Proposal),
    Derivation(PatchSet),
}

/// Fable #196: the revision on the branch lineage of `head` whose decision refs contain the
/// acceptance decision `decision_ref` (the revision that committed the acceptance).
pub(crate) fn decision_revision(
    state: &ApiState,
    head: &plumb_store::GraphRevision,
    decision_ref: &Id,
) -> Result<RevisionId, ApiError> {
    state
        .find_on_branch(head, None, |r| r.decision_refs.contains(decision_ref))?
        .map(|r| r.id)
        .ok_or_else(|| ApiError::workflow_corrupt(&format!("acceptance decision {decision_ref}")))
}

// ============================================================================ proposal sets (R0.3)

/// Whether the occurrence of workflow proposal `id` on `base` is still pending.
fn is_pending(state: &ApiState, id: &Id, base: &RevisionId) -> Result<bool, ApiError> {
    let record = state
        .with_workflow_store(|store| store.proposal(id, base))?
        .map_err(|_| ApiError::workflow_corrupt("stage proposal"))?
        .ok_or_else(|| ApiError::workflow_corrupt("stage proposal"))?;
    Ok(record.state == WorkflowProposalState::Pending)
}

/// The member proposal IDs of a governed acceptance envelope `Compound [originals...,
/// companions...]`, in member order: `None` unless the second half is exactly one Accepted
/// proposal-acceptance decision per original.
pub(crate) fn set_members(patch_set: &PatchSet) -> Option<Vec<Id>> {
    let SemanticPatch::Compound { patches } = &patch_set.patch else {
        return None;
    };
    if patches.is_empty() || patches.len() % 2 != 0 {
        return None;
    }
    patches[patches.len() / 2..]
        .iter()
        .map(|patch| match patch {
            SemanticPatch::AddNode { node } => match &node.payload {
                NodePayload::ResolutionDecision(d)
                    if d.question_ref.is_none()
                        && d.answer.get("kind").and_then(serde_json::Value::as_str)
                            == Some(ACCEPTANCE_DECISION_KIND) =>
                {
                    d.proposal_ref.clone()
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Read-only X1.5 freshness and exact-output validation of a stage-linked set member: the
/// child is checked on an in-memory copy and nothing is persisted, so a rejected set leaves no
/// child or parent transition (a stale child is observed later, independently).
pub(crate) fn validate_set_member(
    state: &ApiState,
    loaded: &LoadedRevision,
    link: &StageProposalLink,
    proposal: &Proposal,
) -> Result<(), ApiError> {
    let mut job = load_child(state, &link.stage_job_id)?;
    let run = load_run(state, &link.compile_run_ref)?;
    job.validate_proposal_commit(&loaded.revision.id, &run, proposal)
        .map(|_| ())
        .map_err(|_| baseline_changed(&proposal.id))
}

/// Records one child's share `owned` of a committed proposal set `members` (ascending ID
/// order) through the X1.5 set recorder and persists the child Committed.
pub(crate) fn record_set(
    state: &ApiState,
    link: &StageProposalLink,
    members: &[Proposal],
    owned: &[Id],
    committed: &Committed,
) -> Result<(), ApiError> {
    let mut job = load_child(state, &link.stage_job_id)?;
    let run = load_run(state, &link.compile_run_ref)?;
    let before = status_of(&job);
    job.record_governed_proposal_set_commit(
        &run,
        members,
        owned,
        &committed.patch_set,
        &committed.commit,
    )
    .map_err(|_| internal("stage_governed_commit"))?;
    persist_child(state, &job, before, state.now())
}

/// Reconciles an interrupted proposal-set acceptance from its committed `revision` without
/// re-committing: every still-pending member becomes accepted, every affected Ready child
/// records all of its batch-accepted outputs once, and every affected parent finishes.
/// Idempotent.
fn reconcile_set(
    state: &ApiState,
    loaded: &LoadedRevision,
    revision: &plumb_store::GraphRevision,
    patch_set: &PatchSet,
) -> Result<(), ApiError> {
    let corrupt = || ApiError::workflow_corrupt("stage acceptance set");
    let ids = set_members(patch_set).ok_or_else(corrupt)?;
    // Every member was built on the revision the set was committed on.
    let base = revision.parent.clone().ok_or_else(corrupt)?;
    let mut members = Vec::with_capacity(ids.len());
    let mut children: std::collections::BTreeMap<Hash, (StageProposalLink, Vec<Id>)> =
        std::collections::BTreeMap::new();
    for id in &ids {
        let record = state
            .with_workflow_store(|store| store.proposal(id, &base))?
            .map_err(|_| corrupt())?
            .ok_or_else(corrupt)?;
        if record.state == WorkflowProposalState::Pending {
            let decisions = acceptance_decisions(&loaded.graph, id);
            let [(_, decided_at)] = decisions.as_slice() else {
                return Err(corrupt());
            };
            state
                .with_workflow_store(|store| {
                    store.resolve_proposal(
                        id,
                        &base,
                        WorkflowProposalState::Accepted,
                        &revision.id,
                        *decided_at,
                    )
                })?
                .map_err(|_| internal("workflow_store"))?;
        }
        members.push(decode_proposal(record)?.proposal);
        if let Some(link) = link_of(state, id, &base)? {
            children
                .entry(link.stage_job_id.clone())
                .or_insert_with(|| (link.clone(), Vec::new()))
                .1
                .push(id.clone());
        }
    }
    let delta = apply_patch(&state.load_revision(&base)?.graph, patch_set)
        .map_err(|_| corrupt())?
        .delta;
    let committed = Committed {
        patch_set: patch_set.clone(),
        commit: CommitResult {
            revision: revision.clone(),
            delta,
        },
    };
    for (link, owned) in children.values() {
        let job = load_child(state, &link.stage_job_id)?;
        if job.status() == StageJobStatus::Ready {
            record_set(state, link, &members, owned, &committed)?;
        }
        finish_parent(state, &link.recipe_job_id, &revision.id)?;
    }
    Ok(())
}
