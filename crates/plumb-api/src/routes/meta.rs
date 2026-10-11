//! Meta routes (plan A0.2): identity, model version and the forward-only design-event stream.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio::sync::broadcast;

use crate::error::ApiError;
use crate::openapi::{DesignEvent, MeResponse, ModelVersionResponse};
use crate::state::{read_json, ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/me", get(get_me))
        .route("/api/model/version", get(get_model_version))
        .route("/api/events", get(events_websocket))
}

/// `get_me`: the injected identity without its provenance actor.
async fn get_me(State(state): State<ApiState>) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let identity = state.identity();
    Ok(read_json(
        &loaded,
        MeResponse {
            user: identity.user().to_owned(),
            roles: identity.roles().to_vec(),
            mode: identity.mode().to_owned(),
        },
    ))
}

/// `get_model_version`: the head revision and its hashes; `version` and `hash` are aliases.
async fn get_model_version(State(state): State<ApiState>) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let revision = loaded.revision.id.to_string();
    let semantic_hash = loaded.revision.semantic_hash.to_string();
    let body = ModelVersionResponse {
        version: revision.clone(),
        hash: semantic_hash.clone(),
        revision,
        semantic_hash,
        evidence_hash: loaded.revision.evidence_hash.to_string(),
    };
    Ok(read_json(&loaded, body))
}

/// `events_websocket`: a live server-to-client stream of design events committed after the
/// subscription. No history, snapshot or replay; a lagged or closed subscription ends the
/// socket instead of skipping events.
async fn events_websocket(
    State(state): State<ApiState>,
    upgrade: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    let Ok(upgrade) = upgrade else {
        return ApiError::invalid("/api/events requires a WebSocket upgrade").into_response();
    };
    let receiver = state.subscribe();
    upgrade.on_upgrade(move |socket| forward_events(socket, receiver))
}

async fn forward_events(mut socket: WebSocket, mut receiver: broadcast::Receiver<DesignEvent>) {
    loop {
        let event = match receiver.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(_))
            | Err(broadcast::error::RecvError::Closed) => break,
        };
        let Ok(text) = serde_json::to_string(&event) else {
            break;
        };
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}
