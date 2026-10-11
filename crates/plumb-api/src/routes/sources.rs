//! Source routes (plan A0.2): evidence import and source/span reads.
//!
//! An upload imports evidence only: the importer chosen by the exact part media type produces the
//! source and fragment nodes and two content-addressed artifacts. The artifacts are persisted
//! before the graph commit and stay (immutable, possibly orphaned) if the commit loses its race.
//! No segmentation, model call or Requirement creation happens here.

use axum::extract::multipart::MultipartRejection;
use axum::extract::{Multipart, Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plumb_artifacts::ArtifactStore;
use plumb_core::{Id, StageId};
use plumb_import::{import_docx, import_markdown, import_plain_text, ImportAudit};
use plumb_patch::{AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch};
use plumb_psg::{EvidenceLocator, NodePayload, NodeType};
use serde_json::json;

use crate::error::ApiError;
use crate::openapi::{Source, SourceUploadResponse, Span, SpanPage};
use crate::state::{check_if_match, paginate, read_json, ApiState, PageRequest, QueryParams};

const MARKDOWN: &str = "text/markdown";
const PLAIN_TEXT: &str = "text/plain";
const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/sources", post(create_source))
        .route("/api/sources/{id}", get(get_source))
        .route("/api/sources/{id}/spans", get(get_source_spans))
}

struct Upload {
    display_name: String,
    media_type: String,
    bytes: Vec<u8>,
}

/// Reads the complete multipart body before any store is touched.
async fn read_upload(multipart: Result<Multipart, MultipartRejection>) -> Result<Upload, ApiError> {
    let mut multipart =
        multipart.map_err(|_| ApiError::invalid("a multipart/form-data body is required"))?;
    let mut upload = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::invalid("malformed multipart body"))?
    {
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
        let bytes = field
            .bytes()
            .await
            .map_err(|_| ApiError::invalid("malformed multipart body"))?
            .to_vec();
        upload = Some(Upload {
            display_name,
            media_type,
            bytes,
        });
    }
    upload.ok_or_else(|| ApiError::invalid("the multipart part `file` is required"))
}

/// `create_source`.
async fn create_source(
    State(state): State<ApiState>,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Response, ApiError> {
    let upload = read_upload(multipart).await?;
    let importer = match upload.media_type.as_str() {
        MARKDOWN => import_markdown,
        PLAIN_TEXT => import_plain_text,
        DOCX => import_docx,
        other => return Err(ApiError::unsupported_media_type(other)),
    };
    let loaded = state.load_head()?;
    check_if_match(&headers, &loaded)?;
    let created_at = state.now();
    let imported = importer(
        &upload.display_name,
        &upload.bytes,
        &ImportAudit {
            created_by: state.identity().actor_ref().clone(),
            created_at,
        },
    )
    .map_err(|e| {
        ApiError::invalid("the uploaded file cannot be imported as its declared media type")
            .with_details(json!({ "import_error": e.to_string() }))
    })?;
    if let Some(existing) = loaded.graph.node(&imported.source.id) {
        let code = if existing == &imported.source {
            "E_SOURCE_EXISTS"
        } else {
            "E_SOURCE_CONFLICT"
        };
        return Err(
            ApiError::conflict(code, "a source with this content already exists")
                .with_details(json!({ "id": imported.source.id })),
        );
    }
    for artifact in [&imported.original_artifact, &imported.extracted_artifact] {
        let stored = state.with_artifact_store(|store| {
            store.put(
                artifact.kind,
                &artifact.media_type,
                &artifact.bytes,
                artifact.created_at,
            )
        })?;
        match stored {
            Ok(hash) if hash == artifact.hash => {}
            _ => return Err(ApiError::internal("artifact_store")),
        }
    }
    let mut patches = vec![SemanticPatch::AddNode {
        node: imported.source.clone(),
    }];
    patches.extend(
        imported
            .fragments
            .iter()
            .map(|node| SemanticPatch::AddNode { node: node.clone() }),
    );
    let proposal = Proposal::new(
        StageId::S0,
        PatchSet {
            base_semantic_hash: loaded.revision.semantic_hash.clone(),
            patch: SemanticPatch::Compound { patches },
        },
        Vec::new(),
        Vec::new(),
        ProposalMateriality::NonSemantic,
        AcceptancePolicy::AutoNonSemantic,
        None,
    )
    .map_err(|_| ApiError::internal("proposal"))?;
    let write = state.commit(&loaded, &proposal, created_at, "create_source")?;
    let NodePayload::SourceArtifact(source) = &imported.source.payload else {
        return Err(ApiError::internal("importer"));
    };
    let body = SourceUploadResponse {
        version: write.version,
        revision: write.revision,
        semantic_hash: write.semantic_hash,
        events: write.events,
        id: imported.source.id.to_string(),
        hash: source.content_hash.to_string(),
        spans_count: imported.fragments.len() as u64,
    };
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
            Err(_) => return Err(ApiError::internal("locator")),
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
