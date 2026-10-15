//! The asynchronous stage job lifecycle and base-revision protection (plan X1.5, Hotfix 061).
//!
//! A job records the branch head it observed and the immutable inputs of one stage run. It is a
//! deterministic state model, not a runner: nothing here executes, schedules, persists, reads a
//! clock or touches a store. Freshness is decided by revision-ID equality with the observed
//! branch head alone. The commit guards prove only revision, run and output freshness; intake,
//! reasons and authorization stay with acceptance, and the revision store's expected-head check
//! stays the final race authority.

use std::fmt;

use plumb_core::{to_canonical_json, CoreError, Hash, HashKind, StageId};
use plumb_patch::{PatchSet, Proposal, ProposalError, SemanticPatch};
use plumb_psg::{ElementStatus, NodePayload};
use plumb_store::{BranchName, CommitResult, LoadedRevision, RevisionId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::context::{CompileContext, Scope};
use crate::run::CompileRun;
use crate::stage::{CompilerError, StagePlan};

/// Where a stage job is in its lifecycle. `Stale`, `Failed` and `Committed` are terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageJobStatus {
    Queued,
    Acquiring,
    Evaluating,
    Ready,
    Stale,
    Failed,
    Committed,
}

impl StageJobStatus {
    /// The exact wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            StageJobStatus::Queued => "queued",
            StageJobStatus::Acquiring => "acquiring",
            StageJobStatus::Evaluating => "evaluating",
            StageJobStatus::Ready => "ready",
            StageJobStatus::Stale => "stale",
            StageJobStatus::Failed => "failed",
            StageJobStatus::Committed => "committed",
        }
    }

    /// Whether no transition leaves this status.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            StageJobStatus::Stale | StageJobStatus::Failed | StageJobStatus::Committed
        )
    }
}

impl fmt::Display for StageJobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a job cannot be captured, advanced, completed or used to commit.
#[derive(Debug, Error)]
pub enum StageJobError {
    #[error(transparent)]
    Compiler(#[from] CompilerError),
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("invalid stage job: {0}")]
    InvalidJob(&'static str),
    #[error("stage job cannot move from {from} to {to}")]
    InvalidTransition {
        from: StageJobStatus,
        to: StageJobStatus,
    },
    #[error("observed head {observed} is not the loaded revision {loaded}")]
    ObservedHeadMismatch {
        observed: RevisionId,
        loaded: RevisionId,
    },
    #[error("loaded revision metadata does not match its graph: {0}")]
    LoadedRevisionMismatch(&'static str),
    #[error("compile context does not match the loaded revision: {0}")]
    ContextMismatch(&'static str),
    #[error("stage plan scope is not the job scope")]
    ScopeMismatch,
    #[error("compile run does not match the job: {0}")]
    RunMismatch(&'static str),
    #[error("compile run {0} is not the job's completed run")]
    RunNotCompleted(Hash),
    #[error("stage job is {0}, not ready")]
    JobNotReady(StageJobStatus),
    #[error("stage job is stale: base {base}, current head {current}")]
    Stale {
        base: RevisionId,
        current: RevisionId,
    },
    #[error("proposal is invalid: {0}")]
    InvalidProposal(#[from] ProposalError),
    #[error("proposal is not an output of the job: {0}")]
    ProposalMismatch(&'static str),
    #[error("derivation patch set is not the job's output: {0}")]
    DerivationPatchMismatch(&'static str),
    #[error("committed revision is not a child of the job's base revision")]
    CommitParentMismatch,
    #[error("committed revision did not accept the selected output's patch set")]
    CommitPatchMismatch,
    /// The committed PatchSet is not the A0.3 governed-acceptance envelope of the selected
    /// output (Hotfix 067); the reason names the failing check.
    #[error("committed patch set is not the governed acceptance of the output: {0}")]
    GovernedCommitMismatch(&'static str),
}

/// One stage job: its immutable capture and its lifecycle state. Fields change only through
/// the lifecycle methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StageJobFields")]
pub struct StageJob {
    id: Hash,

    branch: BranchName,

    base_revision: RevisionId,
    base_semantic_hash: Hash,

    stage: StageId,
    scope: Scope,

    profile_hash: Hash,
    config_hash: Hash,

    status: StageJobStatus,

    compile_run_ref: Option<Hash>,
    committed_revision: Option<RevisionId>,
    committed_output_ref: Option<Hash>,
    /// R0.3: the sorted Proposal artifact refs of this job's outputs committed together by one
    /// governed proposal-set acceptance. Empty (and omitted from the wire) for every other
    /// status and for the single-output commit, which keeps using `committed_output_ref`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    committed_output_refs: Vec<Hash>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StageJobFields {
    id: Hash,
    branch: BranchName,
    base_revision: RevisionId,
    base_semantic_hash: Hash,
    stage: StageId,
    scope: Scope,
    profile_hash: Hash,
    config_hash: Hash,
    status: StageJobStatus,
    compile_run_ref: Option<Hash>,
    committed_revision: Option<RevisionId>,
    committed_output_ref: Option<Hash>,
    #[serde(default)]
    committed_output_refs: Vec<Hash>,
}

impl TryFrom<StageJobFields> for StageJob {
    type Error = StageJobError;

    fn try_from(f: StageJobFields) -> Result<Self, Self::Error> {
        let job = StageJob {
            id: f.id,
            branch: f.branch,
            base_revision: f.base_revision,
            base_semantic_hash: f.base_semantic_hash,
            stage: f.stage,
            scope: f.scope,
            profile_hash: f.profile_hash,
            config_hash: f.config_hash,
            status: f.status,
            compile_run_ref: f.compile_run_ref,
            committed_revision: f.committed_revision,
            committed_output_ref: f.committed_output_ref,
            committed_output_refs: f.committed_output_refs,
        };
        job.validate()?;
        Ok(job)
    }
}

/// The canonical artifact hash of a value: SHA-256 of its RFC 8785 JSON.
fn canonical_ref<T: Serialize>(value: &T) -> Result<Hash, CoreError> {
    Ok(Hash::content_sha256(&to_canonical_json(value)?))
}

/// Captures a queued job for `stage` and `scope` on `branch`, whose head was observed to be
/// `observed_head`, from the loaded head revision and the compile context built from it.
pub fn capture_stage_job(
    branch: BranchName,
    observed_head: &RevisionId,
    loaded: &LoadedRevision,
    stage: StageId,
    scope: Scope,
    context: &CompileContext,
) -> Result<StageJob, StageJobError> {
    let revision = &loaded.revision;
    if observed_head != &revision.id {
        return Err(StageJobError::ObservedHeadMismatch {
            observed: observed_head.clone(),
            loaded: revision.id.clone(),
        });
    }
    if loaded.graph.semantic_hash()? != revision.semantic_hash {
        return Err(StageJobError::LoadedRevisionMismatch("semantic_hash"));
    }
    if loaded.graph.profile_id() != &revision.profile_ref {
        return Err(StageJobError::LoadedRevisionMismatch("profile_ref"));
    }
    let mismatch = StageJobError::ContextMismatch;
    if context.input_revision != revision.id {
        return Err(mismatch("input_revision"));
    }
    if context.input_semantic_hash != revision.semantic_hash {
        return Err(mismatch("input_semantic_hash"));
    }
    if context.profile_ref != revision.profile_ref {
        return Err(mismatch("profile_ref"));
    }
    if context.profile_hash != revision.profile_hash {
        return Err(mismatch("profile_hash"));
    }
    if context.rule_pack_hash != revision.rule_pack_hash {
        return Err(mismatch("rule_pack_hash"));
    }
    context.validate_for_graph(&loaded.graph)?;
    scope.validate_for_graph(&loaded.graph)?;
    let mut job = StageJob {
        id: Hash::content_sha256(b""),
        branch,
        base_revision: revision.id.clone(),
        base_semantic_hash: revision.semantic_hash.clone(),
        stage,
        scope,
        profile_hash: revision.profile_hash.clone(),
        config_hash: context.config_hash()?,
        status: StageJobStatus::Queued,
        compile_run_ref: None,
        committed_revision: None,
        committed_output_ref: None,
        committed_output_refs: Vec::new(),
    };
    job.id = job.recompute_id()?;
    job.validate()?;
    Ok(job)
}

impl StageJob {
    pub fn id(&self) -> &Hash {
        &self.id
    }

    pub fn branch(&self) -> &BranchName {
        &self.branch
    }

    pub fn base_revision(&self) -> &RevisionId {
        &self.base_revision
    }

    pub fn base_semantic_hash(&self) -> &Hash {
        &self.base_semantic_hash
    }

    pub fn stage(&self) -> StageId {
        self.stage
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn profile_hash(&self) -> &Hash {
        &self.profile_hash
    }

    pub fn config_hash(&self) -> &Hash {
        &self.config_hash
    }

    pub fn status(&self) -> StageJobStatus {
        self.status
    }

    pub fn compile_run_ref(&self) -> Option<&Hash> {
        self.compile_run_ref.as_ref()
    }

    pub fn committed_revision(&self) -> Option<&RevisionId> {
        self.committed_revision.as_ref()
    }

    pub fn committed_output_ref(&self) -> Option<&Hash> {
        self.committed_output_ref.as_ref()
    }

    /// The outputs committed by one governed proposal-set acceptance (R0.3); empty otherwise.
    pub fn committed_output_refs(&self) -> &[Hash] {
        &self.committed_output_refs
    }

    /// Exactly the immutable capture fields that determine the job ID.
    pub fn identity_projection(&self) -> Value {
        json!({
            "branch": self.branch,
            "base_revision": self.base_revision,
            "base_semantic_hash": self.base_semantic_hash,
            "stage": self.stage,
            "scope": self.scope,
            "profile_hash": self.profile_hash,
            "config_hash": self.config_hash,
        })
    }

    /// Generic SHA-256 of the RFC 8785 canonical identity projection.
    pub fn recompute_id(&self) -> Result<Hash, CoreError> {
        canonical_ref(&self.identity_projection())
    }

    /// Checks the ID, hash kinds, scope and the fields each status requires.
    pub fn validate(&self) -> Result<(), StageJobError> {
        let invalid = StageJobError::InvalidJob;
        let generic = |hash: &Hash| hash.kind() == HashKind::Generic;
        if !generic(&self.id) {
            return Err(invalid("id is not a Generic hash"));
        }
        if self.base_semantic_hash.kind() != HashKind::Semantic {
            return Err(invalid("base_semantic_hash is not a Semantic hash"));
        }
        if !generic(&self.profile_hash) || !generic(&self.config_hash) {
            return Err(invalid(
                "profile_hash and config_hash must be Generic hashes",
            ));
        }
        if !self.compile_run_ref.as_ref().is_none_or(generic)
            || !self.committed_output_ref.as_ref().is_none_or(generic)
            || !self.committed_output_refs.iter().all(generic)
        {
            return Err(invalid("output refs must be Generic hashes"));
        }
        if self
            .committed_output_refs
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(invalid("committed output refs are not sorted and unique"));
        }
        self.scope.validate()?;
        let (run, revision, output, outputs) = (
            self.compile_run_ref.is_some(),
            self.committed_revision.is_some(),
            self.committed_output_ref.is_some(),
            !self.committed_output_refs.is_empty(),
        );
        let fields_ok = match self.status {
            StageJobStatus::Queued
            | StageJobStatus::Acquiring
            | StageJobStatus::Evaluating
            | StageJobStatus::Failed => !run && !revision && !output && !outputs,
            StageJobStatus::Ready | StageJobStatus::Stale => {
                run && !revision && !output && !outputs
            }
            // Exactly one commit mode: one output, or one proposal-set acceptance.
            StageJobStatus::Committed => run && revision && (output != outputs),
        };
        if !fields_ok {
            return Err(invalid("status does not match its run and commit fields"));
        }
        if self.recompute_id()? != self.id {
            return Err(invalid("id does not match the identity projection"));
        }
        Ok(())
    }

    fn transition(
        &mut self,
        from: &[StageJobStatus],
        to: StageJobStatus,
    ) -> Result<(), StageJobError> {
        if !from.contains(&self.status) {
            return Err(StageJobError::InvalidTransition {
                from: self.status,
                to,
            });
        }
        self.status = to;
        Ok(())
    }

    /// Queued -> Acquiring.
    pub fn start_acquiring(&mut self) -> Result<(), StageJobError> {
        self.transition(&[StageJobStatus::Queued], StageJobStatus::Acquiring)
    }

    /// Queued (no acquisition needed) or Acquiring -> Evaluating.
    pub fn start_evaluating(&mut self) -> Result<(), StageJobError> {
        self.transition(
            &[StageJobStatus::Queued, StageJobStatus::Acquiring],
            StageJobStatus::Evaluating,
        )
    }

    /// Queued, Acquiring or Evaluating -> Failed. A completed run is never Failed.
    pub fn mark_failed(&mut self) -> Result<(), StageJobError> {
        self.transition(
            &[
                StageJobStatus::Queued,
                StageJobStatus::Acquiring,
                StageJobStatus::Evaluating,
            ],
            StageJobStatus::Failed,
        )
    }

    /// Requires `run` to be a valid run of exactly this job's capture (and, once completed,
    /// the job's own run).
    fn check_run(&self, run: &CompileRun) -> Result<(), StageJobError> {
        run.validate()?;
        if let Some(completed) = &self.compile_run_ref {
            if completed != &run.id {
                return Err(StageJobError::RunNotCompleted(run.id.clone()));
            }
        }
        let mismatch = StageJobError::RunMismatch;
        if run.stage != self.stage {
            return Err(mismatch("stage"));
        }
        if run.input_revision != self.base_revision {
            return Err(mismatch("input_revision"));
        }
        if run.input_semantic_hash != self.base_semantic_hash {
            return Err(mismatch("input_semantic_hash"));
        }
        if run.profile_hash != self.profile_hash {
            return Err(mismatch("profile_hash"));
        }
        if run.config_hash != self.config_hash {
            return Err(mismatch("config_hash"));
        }
        Ok(())
    }

    /// Completes an Evaluating job with its plan and run: Ready when `current_head` is still
    /// the base revision, otherwise Stale. A binding error leaves the job Evaluating.
    pub fn complete(
        &mut self,
        plan: &StagePlan,
        run: &CompileRun,
        current_head: &RevisionId,
    ) -> Result<StageJobStatus, StageJobError> {
        if self.status != StageJobStatus::Evaluating {
            return Err(StageJobError::InvalidTransition {
                from: self.status,
                to: StageJobStatus::Ready,
            });
        }
        plan.validate_structure()?;
        if plan.scope != self.scope {
            return Err(StageJobError::ScopeMismatch);
        }
        self.check_run(run)?;
        self.compile_run_ref = Some(run.id.clone());
        self.status = if current_head == &self.base_revision {
            StageJobStatus::Ready
        } else {
            StageJobStatus::Stale
        };
        Ok(self.status)
    }

    /// Records the branch head now observed: a Ready job whose base revision is no longer the
    /// head becomes Stale. Nothing else changes; a Stale job is never revived.
    pub fn observe_head(&mut self, current_head: &RevisionId) -> StageJobStatus {
        if self.status == StageJobStatus::Ready && current_head != &self.base_revision {
            self.status = StageJobStatus::Stale;
        }
        self.status
    }

    /// The status, head and run checks shared by both commit guards. A moved head makes the
    /// job Stale.
    fn check_fresh(
        &mut self,
        current_head: &RevisionId,
        run: &CompileRun,
    ) -> Result<(), StageJobError> {
        if self.status != StageJobStatus::Ready {
            return Err(StageJobError::JobNotReady(self.status));
        }
        if current_head != &self.base_revision {
            self.status = StageJobStatus::Stale;
            return Err(StageJobError::Stale {
                base: self.base_revision.clone(),
                current: current_head.clone(),
            });
        }
        self.check_run(run)
    }

    /// The canonical Proposal artifact ref of `proposal` once it is shown to be an output of
    /// `run` for this job.
    fn proposal_output(
        &self,
        run: &CompileRun,
        proposal: &Proposal,
    ) -> Result<Hash, StageJobError> {
        proposal.validate()?;
        if proposal.stage != self.stage {
            return Err(StageJobError::ProposalMismatch("stage"));
        }
        if proposal.patch_set.base_semantic_hash != self.base_semantic_hash {
            return Err(StageJobError::ProposalMismatch("base_semantic_hash"));
        }
        let output_ref = canonical_ref(proposal)?;
        if !run.output_proposal_refs.contains(&output_ref) {
            return Err(StageJobError::ProposalMismatch("not an output of the run"));
        }
        Ok(output_ref)
    }

    /// The canonical PatchSet ref of `patch_set` once it is shown to be the derivation output
    /// of `run` for this job.
    fn derivation_output(
        &self,
        run: &CompileRun,
        patch_set: &PatchSet,
    ) -> Result<Hash, StageJobError> {
        let Some(expected) = &run.output_derivation_patch_ref else {
            return Err(StageJobError::DerivationPatchMismatch(
                "the run has no derivation output",
            ));
        };
        if patch_set.base_semantic_hash != self.base_semantic_hash {
            return Err(StageJobError::DerivationPatchMismatch("base_semantic_hash"));
        }
        let output_ref = canonical_ref(patch_set)?;
        if &output_ref != expected {
            return Err(StageJobError::DerivationPatchMismatch(
                "not the run's derivation output",
            ));
        }
        Ok(output_ref)
    }

    /// Proves that `proposal` is an exact output of this Ready job's run while `current_head`
    /// is still the base revision, and returns its canonical artifact ref. It proves nothing
    /// about intake, reasons or authorization; the caller must still commit with
    /// `expected_head` equal to the base revision.
    pub fn validate_proposal_commit(
        &mut self,
        current_head: &RevisionId,
        run: &CompileRun,
        proposal: &Proposal,
    ) -> Result<Hash, StageJobError> {
        self.check_fresh(current_head, run)?;
        self.proposal_output(run, proposal)
    }

    /// The derivation counterpart of [`StageJob::validate_proposal_commit`].
    pub fn validate_derivation_commit(
        &mut self,
        current_head: &RevisionId,
        run: &CompileRun,
        patch_set: &PatchSet,
    ) -> Result<Hash, StageJobError> {
        self.check_fresh(current_head, run)?;
        self.derivation_output(run, patch_set)
    }

    fn record_commit(
        &mut self,
        output_ref: Hash,
        patch_set: &PatchSet,
        commit: &CommitResult,
    ) -> Result<(), StageJobError> {
        if commit.revision.parent.as_ref() != Some(&self.base_revision) {
            return Err(StageJobError::CommitParentMismatch);
        }
        if commit.revision.accepted_patch_ref != Some(canonical_ref(patch_set)?) {
            return Err(StageJobError::CommitPatchMismatch);
        }
        self.committed_revision = Some(commit.revision.id.clone());
        self.committed_output_ref = Some(output_ref);
        self.status = StageJobStatus::Committed;
        Ok(())
    }

    /// Records that the store committed `proposal`, an output of this Ready job's run, from the
    /// base revision. Committed means this one selected output advanced the branch; other
    /// proposals of the run stay immutable run outputs.
    pub fn record_proposal_commit(
        &mut self,
        run: &CompileRun,
        proposal: &Proposal,
        commit: &CommitResult,
    ) -> Result<(), StageJobError> {
        if self.status != StageJobStatus::Ready {
            return Err(StageJobError::JobNotReady(self.status));
        }
        self.check_run(run)?;
        let output_ref = self.proposal_output(run, proposal)?;
        self.record_commit(output_ref, &proposal.patch_set, commit)
    }

    /// Records that the store committed `proposal`, an output of this Ready job's run, inside
    /// the A0.3 governed-acceptance envelope (Hotfix 067): `committed_patch_set` must be exactly
    /// `Compound [proposal.patch_set.patch, AddNode(Accepted ResolutionDecision)]` on the same
    /// base, the decision bound to this proposal and its original PatchSet, and the revision a
    /// child of the base that accepted exactly `committed_patch_set`. The committed output ref
    /// stays the original Proposal's canonical ref. Decision IDs, reasons, rationale,
    /// authorization and intake belong to the governance layer and are not re-validated here.
    pub fn record_governed_proposal_commit(
        &mut self,
        run: &CompileRun,
        proposal: &Proposal,
        committed_patch_set: &PatchSet,
        commit: &CommitResult,
    ) -> Result<(), StageJobError> {
        if self.status != StageJobStatus::Ready {
            return Err(StageJobError::JobNotReady(self.status));
        }
        self.check_run(run)?;
        let output_ref = self.proposal_output(run, proposal)?;
        let mismatch = StageJobError::GovernedCommitMismatch;
        if committed_patch_set.base_semantic_hash != proposal.patch_set.base_semantic_hash {
            return Err(mismatch("base_semantic_hash"));
        }
        let SemanticPatch::Compound { patches } = &committed_patch_set.patch else {
            return Err(mismatch("compound_shape"));
        };
        let [original, SemanticPatch::AddNode { node }] = patches.as_slice() else {
            return Err(mismatch("compound_shape"));
        };
        if original != &proposal.patch_set.patch {
            return Err(mismatch("original_patch"));
        }
        if node.validate().is_err() || node.status != ElementStatus::Accepted {
            return Err(mismatch("decision_status"));
        }
        let NodePayload::ResolutionDecision(decision) = &node.payload else {
            return Err(mismatch("decision_payload"));
        };
        if decision.question_ref.is_some() {
            return Err(mismatch("decision_question_ref"));
        }
        if decision.proposal_ref.as_ref() != Some(&proposal.id) {
            return Err(mismatch("decision_proposal_ref"));
        }
        if decision.patch_ref != canonical_ref(&proposal.patch_set)? {
            return Err(mismatch("decision_patch_ref"));
        }
        if decision.supersedes.is_some() {
            return Err(mismatch("decision_supersedes"));
        }
        if commit.revision.parent.as_ref() != Some(&self.base_revision) {
            return Err(StageJobError::CommitParentMismatch);
        }
        if commit.revision.accepted_patch_ref != Some(canonical_ref(committed_patch_set)?) {
            return Err(mismatch("accepted_patch_ref"));
        }
        self.committed_revision = Some(commit.revision.id.clone());
        self.committed_output_ref = Some(output_ref);
        self.status = StageJobStatus::Committed;
        Ok(())
    }

    /// Records that the store committed this job's `owned` outputs inside one governed
    /// proposal-set acceptance (R0.3, architecture §4.7). `members` is the complete accepted
    /// set in ascending proposal-ID order (possibly including proposals of other jobs or of no
    /// job) and `committed_patch_set` must be exactly `Compound [every member's original patch
    /// unchanged, in member order, then one Accepted proposal-acceptance ResolutionDecision
    /// AddNode per member in the same order]` on the shared base, each decision bound to its
    /// member and that member's original PatchSet. Every owned ID must be a member and an
    /// exact output of this Ready job's run; the revision must be a child of the base that
    /// accepted exactly `committed_patch_set`. The job records the sorted owned output refs.
    pub fn record_governed_proposal_set_commit(
        &mut self,
        run: &CompileRun,
        members: &[Proposal],
        owned: &[plumb_core::Id],
        committed_patch_set: &PatchSet,
        commit: &CommitResult,
    ) -> Result<(), StageJobError> {
        if self.status != StageJobStatus::Ready {
            return Err(StageJobError::JobNotReady(self.status));
        }
        self.check_run(run)?;
        let mismatch = StageJobError::GovernedCommitMismatch;
        if members.is_empty() || owned.is_empty() {
            return Err(mismatch("empty_set"));
        }
        if members.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err(mismatch("member_order"));
        }
        if committed_patch_set.base_semantic_hash != self.base_semantic_hash {
            return Err(mismatch("base_semantic_hash"));
        }
        let SemanticPatch::Compound { patches } = &committed_patch_set.patch else {
            return Err(mismatch("compound_shape"));
        };
        if patches.len() != 2 * members.len() {
            return Err(mismatch("compound_shape"));
        }
        let (originals, companions) = patches.split_at(members.len());
        for ((member, original), companion) in members.iter().zip(originals).zip(companions) {
            if member.patch_set.base_semantic_hash != self.base_semantic_hash {
                return Err(mismatch("member_base_semantic_hash"));
            }
            if original != &member.patch_set.patch {
                return Err(mismatch("original_patch"));
            }
            let SemanticPatch::AddNode { node } = companion else {
                return Err(mismatch("compound_shape"));
            };
            if node.validate().is_err() || node.status != ElementStatus::Accepted {
                return Err(mismatch("decision_status"));
            }
            let NodePayload::ResolutionDecision(decision) = &node.payload else {
                return Err(mismatch("decision_payload"));
            };
            if decision.question_ref.is_some() {
                return Err(mismatch("decision_question_ref"));
            }
            if decision.proposal_ref.as_ref() != Some(&member.id) {
                return Err(mismatch("decision_proposal_ref"));
            }
            if decision.patch_ref != canonical_ref(&member.patch_set)? {
                return Err(mismatch("decision_patch_ref"));
            }
            if decision.supersedes.is_some() {
                return Err(mismatch("decision_supersedes"));
            }
        }
        let mut output_refs = Vec::with_capacity(owned.len());
        for id in owned {
            let member = members
                .iter()
                .find(|m| &m.id == id)
                .ok_or(StageJobError::ProposalMismatch("not a member of the set"))?;
            output_refs.push(self.proposal_output(run, member)?);
        }
        output_refs.sort();
        if output_refs.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(mismatch("duplicate_output"));
        }
        if commit.revision.parent.as_ref() != Some(&self.base_revision) {
            return Err(StageJobError::CommitParentMismatch);
        }
        if commit.revision.accepted_patch_ref != Some(canonical_ref(committed_patch_set)?) {
            return Err(mismatch("accepted_patch_ref"));
        }
        self.committed_revision = Some(commit.revision.id.clone());
        self.committed_output_refs = output_refs;
        self.status = StageJobStatus::Committed;
        Ok(())
    }

    /// Records that the store committed the run's derivation `patch_set` from the base revision.
    pub fn record_derivation_commit(
        &mut self,
        run: &CompileRun,
        patch_set: &PatchSet,
        commit: &CommitResult,
    ) -> Result<(), StageJobError> {
        if self.status != StageJobStatus::Ready {
            return Err(StageJobError::JobNotReady(self.status));
        }
        self.check_run(run)?;
        let output_ref = self.derivation_output(run, patch_set)?;
        self.record_commit(output_ref, patch_set, commit)
    }
}
