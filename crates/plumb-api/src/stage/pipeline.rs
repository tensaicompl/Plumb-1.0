//! The UI pipeline projection (B2, UI-SPEC §5.1): the state of every stage recipe in pipeline
//! order, derived only from the stage store (the latest recipe job of each recipe, in insertion
//! order) and the pending workflow proposals. It is a read: nothing is observed, reconciled or
//! written (`get_job` keeps the only legal Ready -> Stale observation).

use std::collections::BTreeSet;

use plumb_core::Id;
use plumb_psg::NodeType;
use plumb_store::{LoadedRevision, RevisionId, StageRecipeJobRecord, StageStatus};
use serde_json::Value;

use crate::application::workflow_error;
use crate::error::ApiError;
use crate::openapi::{
    PipelineError, PipelineResponse, PipelineStep, PipelineStepStatus, QuestionStatus, StageName,
};
use crate::stage::{progress, store_error, Phase, Recipe};
use crate::state::ApiState;

/// The pipeline order of the Overview stepper and the toolbar run control.
pub const PIPELINE: [Recipe; 11] = [
    Recipe::Extract,
    Recipe::Consolidate,
    Recipe::Lint,
    Recipe::Directory,
    Recipe::Analyse,
    Recipe::Flows,
    Recipe::Calc,
    Recipe::Rules,
    Recipe::Scenarios,
    Recipe::Tags,
    Recipe::Run,
];

/// The public stage name of a recipe.
fn stage_name(recipe: Recipe) -> StageName {
    match recipe {
        Recipe::Intake => StageName::Intake,
        Recipe::Lint => StageName::Lint,
        Recipe::Extract => StageName::Extract,
        Recipe::Consolidate => StageName::Consolidate,
        Recipe::Calc => StageName::Calc,
        Recipe::Rules => StageName::Rules,
        Recipe::Analyse => StageName::Analyse,
        Recipe::Flows => StageName::Flows,
        Recipe::Scenarios => StageName::Scenarios,
        Recipe::Tags => StageName::Tags,
        Recipe::Run => StageName::Run,
        Recipe::Directory => StageName::Directory,
    }
}

/// One pending workflow proposal occurrence.
type Occurrence = (Id, RevisionId);

/// The state of a step from its latest job and the pending proposals of that job.
fn status_of(
    record: &StageRecipeJobRecord,
    pending: usize,
    head: &RevisionId,
) -> PipelineStepStatus {
    match record.status {
        StageStatus::Queued => PipelineStepStatus::Queued,
        StageStatus::Acquiring | StageStatus::Evaluating => PipelineStepStatus::Running,
        StageStatus::Failed => PipelineStepStatus::Failed,
        StageStatus::Stale => PipelineStepStatus::Stale,
        StageStatus::Committed => PipelineStepStatus::Done,
        StageStatus::Ready if pending == 0 => PipelineStepStatus::Done,
        // Proposals are never rebased: built on another revision they can only be rejected.
        StageStatus::Ready if record.result_revision_id.as_ref() == Some(head) => {
            PipelineStepStatus::NeedsReview
        }
        StageStatus::Ready => PipelineStepStatus::Stale,
    }
}

impl ApiState {
    /// The pending proposal occurrences a directory job persisted: the elements recorded by its
    /// `directory` step, on the job's base revision.
    pub(crate) fn directory_proposals(
        &self,
        record: &StageRecipeJobRecord,
    ) -> Result<Vec<Occurrence>, ApiError> {
        let steps = self
            .with_stage_store(|store| store.steps(&record.recipe_job_id))?
            .map_err(store_error)?;
        let mut out = Vec::new();
        for step in steps
            .iter()
            .filter(|s| s.phase == Phase::Directory.as_str())
        {
            let Some(hash) = &step.intermediate_artifact_ref else {
                continue;
            };
            let corrupt = || ApiError::workflow_corrupt(&format!("directory step {hash}"));
            let artifact = self.get_artifact(hash)?.map_err(|_| corrupt())?;
            let value: Value = serde_json::from_slice(&artifact.bytes).map_err(|_| corrupt())?;
            for element in value["elements"].as_array().ok_or_else(corrupt)? {
                if let Some(proposal) = element["proposal_id"].as_str() {
                    let id = proposal.parse().map_err(|_| corrupt())?;
                    out.push((id, record.base_revision_id.clone()));
                }
            }
        }
        Ok(out)
    }

    /// `get_pipeline`: every recipe of [`PIPELINE`] with its latest job, its pending proposals
    /// and its missing configuration, plus the source and open-question counts and the next
    /// step (the first not `done` whose configuration is complete).
    pub fn pipeline(&self) -> Result<(PipelineResponse, LoadedRevision), ApiError> {
        let a04 = self.stage_runtime("pipeline")?;
        self.runtime("pipeline")?;
        let loaded = self.load_head()?;
        let head = &loaded.revision.id;
        let jobs = self
            .with_stage_store(|store| store.recipe_jobs_in_creation_order())?
            .map_err(store_error)?;
        let links = self
            .with_stage_store(|store| store.pending_proposal_links())?
            .map_err(store_error)?;
        let pending: BTreeSet<Occurrence> = self
            .with_workflow_store(|store| store.pending_proposals())?
            .map_err(workflow_error)?
            .into_iter()
            .map(|p| (p.proposal_id, p.base_revision_id))
            .collect();
        // The pending proposals each job produced.
        let produced = |record: &StageRecipeJobRecord| -> Result<BTreeSet<Occurrence>, ApiError> {
            // A configured directory records its proposals on its step; an inferred one links
            // them like every child.
            let mut occurrences: Vec<Occurrence> = links
                .iter()
                .filter(|l| l.recipe_job_id == record.recipe_job_id)
                .map(|l| (l.proposal_id.clone(), l.base_revision_id.clone()))
                .collect();
            if record.recipe == Recipe::Directory.as_str() {
                occurrences.extend(self.directory_proposals(record)?);
            }
            Ok(occurrences
                .into_iter()
                .filter(|o| pending.contains(o))
                .collect())
        };
        let mut steps = Vec::with_capacity(PIPELINE.len());
        for recipe in PIPELINE {
            let mut of_recipe = BTreeSet::new();
            let mut latest = None;
            for record in jobs.iter().filter(|j| j.recipe == recipe.as_str()) {
                let mine = produced(record)?;
                of_recipe.extend(mine.iter().cloned());
                latest = Some((record, mine.len()));
            }
            let missing_config = recipe
                .missing_config(a04)
                .into_iter()
                .map(str::to_owned)
                .collect();
            let step = match latest {
                None => PipelineStep {
                    recipe: stage_name(recipe),
                    status: PipelineStepStatus::NotStarted,
                    job_id: None,
                    progress: None,
                    pending_proposals: of_recipe.len() as u64,
                    error: None,
                    updated_at: None,
                    missing_config,
                },
                Some((record, mine)) => PipelineStep {
                    recipe: stage_name(recipe),
                    status: status_of(record, mine, head),
                    job_id: Some(record.recipe_job_id.to_string()),
                    progress: Some(progress(record.status)),
                    pending_proposals: of_recipe.len() as u64,
                    error: match (record.status, &record.failure_code, &record.failure_message) {
                        (StageStatus::Failed, Some(code), Some(message)) => Some(PipelineError {
                            code: code.clone(),
                            message: message.clone(),
                        }),
                        _ => None,
                    },
                    updated_at: Some(record.updated_at.to_string()),
                    missing_config,
                },
            };
            steps.push(step);
        }
        let next = steps
            .iter()
            .find(|s| s.status != PipelineStepStatus::Done && s.missing_config.is_empty())
            .map(|s| s.recipe);
        let sources = loaded
            .graph
            .node_ids_by_type(NodeType::SourceArtifact)
            .len() as u64;
        let open_questions = self
            .question_items(&loaded)
            .into_iter()
            .filter(|(_, q)| q.as_ref().is_ok_and(|q| q.status == QuestionStatus::Open))
            .count() as u64;
        Ok((
            PipelineResponse {
                steps,
                sources,
                open_questions,
                next,
            },
            loaded,
        ))
    }
}
