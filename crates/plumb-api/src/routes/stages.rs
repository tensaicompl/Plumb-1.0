//! Stage and job routes (plan A0.4, Hotfix 067): `run_stage` captures or reuses an application
//! StageRecipeJob and queues it; `get_job` reads it. Orchestration, gate evaluation and
//! persistence live in the stage service, never here.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::ApiError;
use crate::openapi::{StageName, StageRunRequest};
use crate::stage::Recipe;
use crate::state::{parse_body, parse_wire, read_json, ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/stages/{stage}/run", post(run_stage))
        .route("/api/jobs/{id}", get(get_job))
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
    let if_match = headers
        .get("if-match")
        .ok_or_else(|| ApiError::invalid("the If-Match header is required"))?
        .to_str()
        .map_err(|_| ApiError::invalid("the If-Match header is not valid text"))?
        .to_owned();
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
    .map_err(|_| ApiError::internal("stage_task"))??;
    Ok(Json(started.response).into_response())
}

/// `get_job`: the recipe job with the current semantic hash as ETag.
async fn get_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let (job, loaded) = tokio::task::spawn_blocking(move || state.get_job(&id))
        .await
        .map_err(|_| ApiError::internal("stage_task"))??;
    Ok(read_json(&loaded, job))
}
