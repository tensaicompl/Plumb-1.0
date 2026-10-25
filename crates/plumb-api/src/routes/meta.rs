//! Meta routes (plan A0.2): identity, model version and the forward-only design-event stream;
//! A0.3 (Hotfix 065) adds lexical search over the existing index of one loaded revision.

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{RawQuery, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use plumb_intake::{LexicalIndex, RetrievalDocumentKind};
use tokio::sync::broadcast;

use crate::error::ApiError;
use crate::openapi::{
    DesignEvent, MeResponse, ModelVersionResponse, SearchHit, SearchPage, SearchScope,
};
use crate::state::{
    paginate_ordered, parse_wire, read_json, wire, ApiState, PageRequest, QueryParams,
};

/// The longest search snippet, in Unicode scalar values.
const SNIPPET_SCALARS: usize = 240;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/me", get(get_me))
        .route("/api/model/version", get(get_model_version))
        .route("/api/events", get(events_websocket))
        .route("/api/search", get(search_model))
}

/// The retrieval family of a search scope.
fn scope_kind(scope: SearchScope) -> RetrievalDocumentKind {
    match scope {
        SearchScope::Requirement => RetrievalDocumentKind::Requirement,
        SearchScope::Concept => RetrievalDocumentKind::Concept,
        SearchScope::Rule => RetrievalDocumentKind::Rule,
        SearchScope::Calculation => RetrievalDocumentKind::Calculation,
        SearchScope::Scenario => RetrievalDocumentKind::Scenario,
        SearchScope::Evidence => RetrievalDocumentKind::Evidence,
    }
}

/// `search_model`: every positive-score hit of the existing BM25 index of the loaded revision
/// (score descending, then ID ascending), filtered by scope and paged in that ranked order. The
/// snippet is the exact prefix of the indexed text; a detail path exists only where a route does.
async fn search_model(
    State(state): State<ApiState>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &["q", "scope"], true)?;
    let q = match query.get("q") {
        Some(q) if !q.trim().is_empty() => q.to_owned(),
        _ => return Err(ApiError::invalid("q is required and must not be blank")),
    };
    let scope = query
        .get("scope")
        .map(|s| parse_wire::<SearchScope>(s, "scope"))
        .transpose()?
        .map(scope_kind);
    let loaded = state.load_head()?;
    let index =
        LexicalIndex::build(&loaded.graph).map_err(|e| ApiError::internal_cause("search", &e))?;
    let hits = index
        .search(&q, usize::MAX)
        .map_err(|e| ApiError::internal_cause("search", &e))?;
    let mut items = Vec::new();
    for hit in hits
        .into_iter()
        .filter(|h| scope.is_none_or(|k| h.kind == k))
    {
        let text = index
            .indexed_text(&hit.id)
            .ok_or_else(|| ApiError::internal("search"))?;
        let path = match hit.kind {
            RetrievalDocumentKind::Requirement => Some(format!("/api/requirements/{}", hit.id)),
            RetrievalDocumentKind::Rule => Some(format!("/api/model/rules/{}", hit.id)),
            _ => None,
        };
        items.push((
            hit.id.to_string(),
            SearchHit {
                id: hit.id.to_string(),
                kind: wire(&hit.kind)?,
                path,
                score: hit.score,
                snippet: text.chars().take(SNIPPET_SCALARS).collect(),
            },
        ));
    }
    let request = PageRequest {
        operation_id: "search_model",
        path: Default::default(),
        query: &query,
    };
    let (items, next_cursor) = paginate_ordered(items, &request, &loaded)?;
    Ok(read_json(&loaded, SearchPage { items, next_cursor }))
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
            inference_available: state.inference_available(),
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

/// The WebSocket close code of a subscriber that lagged behind the event stream: events were
/// missed, so the client must refetch its state (Fable #237).
pub const EVENTS_MISSED_CLOSE_CODE: u16 = 4001;

/// The close frame of an ended subscription: a lag is the distinct "events missed" close.
fn closing_frame(lagged: bool) -> Option<CloseFrame> {
    lagged.then(|| CloseFrame {
        code: EVENTS_MISSED_CLOSE_CODE,
        reason: "events_missed".into(),
    })
}

/// Forwards committed events while reading the socket concurrently, so a Ping is answered and a
/// client Close or disconnect ends the subscription promptly (Fable #237).
async fn forward_events(mut socket: WebSocket, mut receiver: broadcast::Receiver<DesignEvent>) {
    let lagged = loop {
        tokio::select! {
            event = receiver.recv() => {
                let event = match event {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(_)) => break true,
                    Err(broadcast::error::RecvError::Closed) => break false,
                };
                let Ok(text) = serde_json::to_string(&event) else {
                    break false;
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => match incoming {
                // Pings are answered by the protocol layer while reading; other client messages
                // carry no meaning on this server-to-client stream.
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => {}
            },
        }
    };
    let _ = socket.send(Message::Close(closing_frame(lagged))).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fable #237: a lagged subscriber is closed with the distinct events-missed code.
    #[test]
    fn a_lagged_subscription_closes_as_events_missed() {
        let frame = closing_frame(true).unwrap();
        assert_eq!(frame.code, EVENTS_MISSED_CLOSE_CODE);
        assert_eq!(frame.reason.as_str(), "events_missed");
        assert!(closing_frame(false).is_none());
    }
}
