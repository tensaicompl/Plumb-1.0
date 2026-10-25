//! Finding materialization (B2): `materialize_findings` is the CLI `findings materialize` over
//! HTTP, so governed actions (waivers, questions) can address current gate findings as nodes.
//! The finding reads and waiver stay in `model`.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::error::ApiError;
use crate::openapi::{MaterializeFindingsRequest, MaterializeFindingsResponse};
use crate::state::{check_if_match, parse_body, ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new().route("/api/findings/materialize", post(materialize_findings))
}

/// `materialize_findings`: one non-semantic revision adding every actionable current gate
/// finding absent from the graph and closing stale open ones; nothing changes (no revision, no
/// event, no IDs) when there is nothing to do.
async fn materialize_findings(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let MaterializeFindingsRequest {} = parse_body(&body)?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let response = tokio::task::spawn_blocking(move || {
        let (write, added) = state.materialize_gate_findings_detailed(&loaded)?;
        let finding_ids = added.iter().map(ToString::to_string).collect();
        Ok::<_, ApiError>(match write {
            Some(write) => MaterializeFindingsResponse {
                version: write.version,
                revision: write.revision,
                semantic_hash: write.semantic_hash,
                events: write.events,
                finding_ids,
            },
            None => {
                let revision = loaded.revision.id.to_string();
                MaterializeFindingsResponse {
                    version: revision.clone(),
                    revision,
                    semantic_hash: loaded.revision.semantic_hash.to_string(),
                    events: Vec::new(),
                    finding_ids,
                }
            }
        })
    })
    .await
    .map_err(|e| ApiError::internal_cause("findings_task", &e))??;
    Ok(Json(response).into_response())
}
