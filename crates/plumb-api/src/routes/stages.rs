//! Stage and job routes (plan A0.4, Hotfix 067): `run_stage` captures or reuses an application
//! StageRecipeJob and queues it; `get_job` reads it. B2 adds `get_pipeline` (the derived state of
//! every recipe) and `retry_job` (the H0.3 retry service, then the same start as `run_stage`).
//! Orchestration, gate evaluation and persistence live in the stage service, never here.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::ApiError;
use crate::openapi::{RetryJobRequest, StageName, StageRunRequest, StageRunResponse};
use crate::stage::Recipe;
use crate::state::{parse_body, parse_wire, read_json, ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/stages/{stage}/run", post(run_stage))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/pipeline", get(get_pipeline))
        .route("/api/jobs/{id}/retry", post(retry_job))
}

/// The required If-Match header text (400 when absent or not text).
fn if_match(headers: &HeaderMap) -> Result<String, ApiError> {
    Ok(headers
        .get("if-match")
        .ok_or_else(|| ApiError::invalid("the If-Match header is required"))?
        .to_str()
        .map_err(|_| ApiError::invalid("the If-Match header is not valid text"))?
        .to_owned())
}

/// `run_stage`: If-Match, scope and configuration checks, then a queued (or reused) recipe job;
/// no revision and no design event.
async fn run_stage(
    State(state): State<ApiState>,
    Path(stage): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let name: StageName = parse_wire(&stage, "stage")?;
    let if_match = if_match(&headers)?;
    let request: StageRunRequest = parse_body(&body)?;
    let started = tokio::task::spawn_blocking({
        let state = state.clone();
        move || {
            let run = state.run_stage(
                Recipe::from_stage_name(name),
                request.scope,
                Some(&if_match),
            )?;
            state.enqueue_recipe_job(&run.job_id)?;
            Ok::<_, ApiError>(run)
        }
    })
    .await
    .map_err(|e| ApiError::internal_cause("stage_task", &e))??;
    Ok(Json(started.response).into_response())
}

/// `get_job`: the recipe job with the current semantic hash as ETag.
async fn get_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let (job, loaded) = tokio::task::spawn_blocking(move || state.get_job(&id))
        .await
        .map_err(|e| ApiError::internal_cause("stage_task", &e))??;
    Ok(read_json(&loaded, job))
}

/// `get_pipeline`: the derived state of every stage recipe with the head as ETag.
async fn get_pipeline(State(state): State<ApiState>) -> Result<Response, ApiError> {
    let (pipeline, loaded) = tokio::task::spawn_blocking(move || state.pipeline())
        .await
        .map_err(|e| ApiError::internal_cause("stage_task", &e))??;
    Ok(read_json(&loaded, pipeline))
}

/// `retry_job`: If-Match names the head (which must still be the job's base), then the H0.3
/// retry service re-queues the Failed job as its next attempt and it is started like
/// `run_stage`; the response is the `run_stage` response. No revision and no design event.
async fn retry_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let tag = if_match(&headers)?;
    let RetryJobRequest {} = parse_body(&body)?;
    let response = tokio::task::spawn_blocking(move || {
        let loaded = state.load_head()?;
        if !crate::state::if_match_names(&tag, &loaded.revision) {
            return Err(crate::state::precondition_failed(&loaded.revision));
        }
        let retry = state.retry_job(&id)?;
        state.enqueue_recipe_job(&retry.job_id)?;
        let revision = loaded.revision.id.to_string();
        Ok::<_, ApiError>(StageRunResponse {
            version: revision.clone(),
            revision,
            semantic_hash: loaded.revision.semantic_hash.to_string(),
            events: Vec::new(),
            job_id: retry.job_id.to_string(),
        })
    })
    .await
    .map_err(|e| ApiError::internal_cause("stage_task", &e))??;
    Ok(Json(response).into_response())
}
