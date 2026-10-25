//! Source routes (plan A0.2): evidence import and source/span reads.
//!
//! An upload imports evidence only: the importer chosen by the exact part media type produces the
//! source and fragment nodes and two content-addressed artifacts. The artifacts are persisted
//! before the graph commit and stay (immutable, possibly orphaned) if the commit loses its race.
//! No segmentation, model call or Requirement creation happens here.

use axum::extract::multipart::{MultipartError, MultipartRejection};
use axum::extract::{DefaultBodyLimit, Multipart, Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plumb_core::Id;
use plumb_psg::{EvidenceLocator, NodePayload, NodeType};

use crate::application::require_importable;
use crate::error::ApiError;
use crate::openapi::{Source, Span, SpanPage};
use crate::routes::MAX_SOURCE_UPLOAD_BODY_BYTES;
use crate::state::{check_if_match, paginate, read_json, ApiState, PageRequest, QueryParams};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/sources",
            post(create_source).layer(DefaultBodyLimit::max(MAX_SOURCE_UPLOAD_BODY_BYTES)),
        )
        .route("/api/sources/{id}", get(get_source))
        .route("/api/sources/{id}/spans", get(get_source_spans))
}

struct Upload {
    display_name: String,
    media_type: String,
    bytes: Vec<u8>,
}

/// A multipart read failure: the body limit is 413, anything else stays a malformed body.
fn multipart_error(error: MultipartError) -> ApiError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::payload_too_large(MAX_SOURCE_UPLOAD_BODY_BYTES)
    } else {
        ApiError::invalid("malformed multipart body")
    }
}

/// Reads the complete multipart body, bounded by the route's body limit, before any store is
/// touched.
async fn read_upload(multipart: Result<Multipart, MultipartRejection>) -> Result<Upload, ApiError> {
    let mut multipart =
        multipart.map_err(|_| ApiError::invalid("a multipart/form-data body is required"))?;
    let mut upload = None;
    while let Some(field) = multipart.next_field().await.map_err(multipart_error)? {
        if field.name() != Some("file") {
            return Err(ApiError::invalid(
                "only the multipart part `file` is accepted",
            ));
        }
        if upload.is_some() {
            return Err(ApiError::invalid("exactly one `file` part is accepted"));
        }
        let display_name = field
            .file_name()
            .filter(|n| !n.trim().is_empty())
            .ok_or_else(|| ApiError::invalid("the `file` part needs a non-empty filename"))?
            .to_owned();
        let media_type = field.content_type().unwrap_or_default().to_owned();
        let bytes = field.bytes().await.map_err(multipart_error)?.to_vec();
        upload = Some(Upload {
            display_name,
            media_type,
            bytes,
        });
    }
    upload.ok_or_else(|| ApiError::invalid("the multipart part `file` is required"))
}

/// `create_source`: the transport of the shared import service (415 before If-Match).
async fn create_source(
    State(state): State<ApiState>,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Response, ApiError> {
    let upload = read_upload(multipart).await?;
    require_importable(&upload.media_type)?;
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let body = state.create_source(
        &loaded,
        &upload.display_name,
        &upload.media_type,
        &upload.bytes,
    )?;
    Ok(Json(body).into_response())
}

/// The SourceArtifact node `id`, or 404.
fn source_payload<'g>(
    graph: &'g plumb_psg::Graph,
    id: &str,
) -> Result<(&'g Id, &'g plumb_psg::SourceArtifact), ApiError> {
    let missing = || ApiError::not_found(format!("source {id} not found"));
    let id: Id = id.parse().map_err(|_| missing())?;
    let node = graph.node(&id).ok_or_else(missing)?;
    match &node.payload {
        NodePayload::SourceArtifact(source) => Ok((&node.id, source)),
        _ => Err(missing()),
    }
}

/// `get_source`.
async fn get_source(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let loaded = state.load_head()?;
    let (id, source) = source_payload(&loaded.graph, &id)?;
    let body = Source {
        id: id.to_string(),
        hash: source.content_hash.to_string(),
        media_type: source.media_type.clone(),
        source_kind: source.source_kind.clone(),
    };
    Ok(read_json(&loaded, body))
}

/// `get_source_spans`: the source's EvidenceFragments by ID.
async fn get_source_spans(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let query = QueryParams::parse(query.as_deref(), &[], true)?;
    let loaded = state.load_head()?;
    let (source_id, _) = source_payload(&loaded.graph, &id)?;
    let mut items = Vec::new();
    for fragment_id in loaded.graph.node_ids_by_type(NodeType::EvidenceFragment) {
        let Some(node) = loaded.graph.node(fragment_id) else {
            continue;
        };
        let NodePayload::EvidenceFragment(fragment) = &node.payload else {
            continue;
        };
        if &fragment.source_ref != source_id {
            continue;
        }
        let locator_kind = match serde_json::to_value(&fragment.locator) {
            Ok(value) => value["kind"].as_str().unwrap_or_default().to_owned(),
            Err(e) => return Err(ApiError::internal_cause("locator", &e)),
        };
        let (start, end) = match &fragment.locator {
            EvidenceLocator::TextRange { start, end } => (Some(*start), Some(*end)),
            _ => (None, None),
        };
        items.push((
            node.id.to_string(),
            Span {
                id: node.id.to_string(),
                source_ref: fragment.source_ref.to_string(),
                locator_kind,
                start,
                end,
                content_hash: fragment.content_hash.to_string(),
            },
        ));
    }
    let request = PageRequest {
        operation_id: "get_source_spans",
        path: [("id", id.as_str())].into_iter().collect(),
        query: &query,
    };
    let (items, next_cursor) = paginate(items, &request, &loaded)?;
    Ok(read_json(&loaded, SpanPage { items, next_cursor }))
}
