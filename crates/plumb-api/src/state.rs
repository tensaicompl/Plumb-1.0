//! Runtime state and shared request plumbing of the pilot HTTP API (plan A0.2, Hotfix 063).
//!
//! `ApiState` owns one explicit branch, the SQLite revision and artifact stores behind
//! synchronous mutexes (never held across an await), the injected identity and clock, the
//! design-event broadcaster, the orchestration-registered gate reports and the validation
//! profile. There is no global graph: every request loads the branch head once and answers or
//! writes against exactly that revision. Writes go through the existing intake, the rationale
//! guard and the store's compare-and-swap commit; nothing mutates a graph in place.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use plumb_artifacts::SqliteArtifactStore;
use plumb_core::{to_canonical_json, Clock, GateId, Hash, HashKind, Id, Timestamp};
use plumb_intake::{
    intake, validate_required_rationale, IntakeAcceptanceError, IntakeError, IntakeValidationInput,
};
use plumb_patch::Proposal;
use plumb_store::{
    BranchName, GraphRevision, LoadedRevision, RevisionId, RevisionWriteMeta, SqliteRevisionStore,
    StoreError,
};
use plumb_validation::{GateReport, ValidationProfile};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::broadcast;

use crate::error::ApiError;
use crate::openapi::{DesignEvent, WriteResult};

/// Capacity of the design-event channel; a slower subscriber is disconnected, never skipped.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Why runtime state cannot be constructed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApiStateError {
    #[error("identity user must not be blank")]
    BlankUser,
    #[error("identity mode must not be blank")]
    BlankMode,
    #[error("identity roles must be sorted and unique")]
    UnsortedRoles,
    #[error("profile hashes cannot be computed: {0}")]
    Profile(String),
}

/// The injected API identity. `actor_ref` is provenance only and never returned by `/api/me`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiIdentity {
    user: String,
    actor_ref: Id,
    roles: Vec<String>,
    mode: String,
}

impl ApiIdentity {
    /// Requires a non-blank user and mode and sorted, unique roles.
    pub fn new(
        user: String,
        actor_ref: Id,
        roles: Vec<String>,
        mode: String,
    ) -> Result<ApiIdentity, ApiStateError> {
        if user.trim().is_empty() {
            return Err(ApiStateError::BlankUser);
        }
        if mode.trim().is_empty() {
            return Err(ApiStateError::BlankMode);
        }
        if roles.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ApiStateError::UnsortedRoles);
        }
        Ok(ApiIdentity {
            user,
            actor_ref,
            roles,
            mode,
        })
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn actor_ref(&self) -> &Id {
        &self.actor_ref
    }

    pub fn roles(&self) -> &[String] {
        &self.roles
    }

    pub fn mode(&self) -> &str {
        &self.mode
    }
}

struct Inner {
    branch: BranchName,
    revision_store: Mutex<SqliteRevisionStore>,
    artifact_store: Mutex<SqliteArtifactStore>,
    identity: ApiIdentity,
    clock: Arc<dyn Clock + Send + Sync>,
    events: broadcast::Sender<DesignEvent>,
    gate_reports: Vec<GateReport>,
    profile: ValidationProfile,
    profile_hash: Hash,
    rule_pack_hash: Hash,
}

/// Cheaply cloneable shared state of the pilot HTTP API.
#[derive(Clone)]
pub struct ApiState {
    inner: Arc<Inner>,
}

impl ApiState {
    /// Builds the state for one explicit branch. `gate_reports` are the orchestration-supplied
    /// reports; only those matching the loaded head are ever presented as current.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        branch: BranchName,
        revision_store: SqliteRevisionStore,
        artifact_store: SqliteArtifactStore,
        identity: ApiIdentity,
        clock: Arc<dyn Clock + Send + Sync>,
        gate_reports: Vec<GateReport>,
        profile: ValidationProfile,
    ) -> Result<ApiState, ApiStateError> {
        let profile_hash = profile
            .profile_hash()
            .map_err(|e| ApiStateError::Profile(e.to_string()))?;
        let rule_pack_hash = profile
            .rule_pack_hash()
            .map_err(|e| ApiStateError::Profile(e.to_string()))?;
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Ok(ApiState {
            inner: Arc::new(Inner {
                branch,
                revision_store: Mutex::new(revision_store),
                artifact_store: Mutex::new(artifact_store),
                identity,
                clock,
                events,
                gate_reports,
                profile,
                profile_hash,
                rule_pack_hash,
            }),
        })
    }

    pub fn identity(&self) -> &ApiIdentity {
        &self.inner.identity
    }

    /// A new subscription to future design events.
    pub fn subscribe(&self) -> broadcast::Receiver<DesignEvent> {
        self.inner.events.subscribe()
    }

    pub(crate) fn profile(&self) -> &ValidationProfile {
        &self.inner.profile
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.inner.clock.now()
    }

    pub(crate) fn with_artifact_store<R>(
        &self,
        f: impl FnOnce(&mut SqliteArtifactStore) -> R,
    ) -> Result<R, ApiError> {
        let mut store = self
            .inner
            .artifact_store
            .lock()
            .map_err(|_| ApiError::internal("artifact_store"))?;
        Ok(f(&mut store))
    }

    fn with_revision_store<R>(
        &self,
        f: impl FnOnce(&mut SqliteRevisionStore) -> R,
    ) -> Result<R, ApiError> {
        let mut store = self
            .inner
            .revision_store
            .lock()
            .map_err(|_| ApiError::internal("revision_store"))?;
        Ok(f(&mut store))
    }

    /// Loads the branch head once: the only revision a request reads or writes against.
    pub(crate) fn load_head(&self) -> Result<LoadedRevision, ApiError> {
        self.with_revision_store(|store| {
            let head = store
                .head(&self.inner.branch)
                .map_err(|_| ApiError::internal("revision_store"))?
                .ok_or_else(|| ApiError::internal("branch_head"))?;
            store
                .load_revision(&head)
                .map_err(|_| ApiError::internal("revision_store"))
        })?
    }

    /// Loads an immutable revision by ID; an unknown ID is 404.
    pub(crate) fn load_revision(&self, id: &RevisionId) -> Result<LoadedRevision, ApiError> {
        self.with_revision_store(|store| match store.load_revision(id) {
            Ok(loaded) => Ok(loaded),
            Err(StoreError::RevisionNotFound(_)) => {
                Err(ApiError::not_found(format!("revision {id} not found")))
            }
            Err(_) => Err(ApiError::internal("revision_store")),
        })?
    }

    /// The registered gate reports that are current for `loaded`, in I0, F1, F2, F3, F4 order.
    pub(crate) fn current_gate_reports(&self, loaded: &LoadedRevision) -> Vec<&GateReport> {
        let inner = &self.inner;
        let mut current: Vec<&GateReport> = inner
            .gate_reports
            .iter()
            .filter(|r| {
                r.validate().is_ok()
                    && r.baseline_semantic_hash == loaded.revision.semantic_hash
                    && r.baseline_evidence_hash == loaded.revision.evidence_hash
                    && r.profile_id == inner.profile.profile_id
                    && r.profile_hash == inner.profile_hash
                    && r.rule_pack_hash == inner.rule_pack_hash
            })
            .collect();
        current.sort_by_key(|r| GateId::ALL.iter().position(|g| *g == r.gate));
        current.dedup_by_key(|r| r.gate);
        current
    }

    /// The current report of `gate`, or 409 `E_GATE_REPORT_UNAVAILABLE`.
    pub(crate) fn current_gate_report(
        &self,
        loaded: &LoadedRevision,
        gate: GateId,
    ) -> Result<&GateReport, ApiError> {
        self.current_gate_reports(loaded)
            .into_iter()
            .find(|r| r.gate == gate)
            .ok_or_else(|| {
                ApiError::conflict(
                    "E_GATE_REPORT_UNAVAILABLE",
                    format!("no current {gate} gate report for the branch head"),
                )
                .with_details(json!({ "gate": gate.to_string() }))
            })
    }

    /// Runs the existing intake and rationale guard on `proposal` against `loaded`, then
    /// commits it with the loaded revision as expected head and broadcasts one design event.
    /// A moved head is 412 built from the reloaded head; nothing is retried or rebased.
    pub(crate) fn commit(
        &self,
        loaded: &LoadedRevision,
        proposal: &Proposal,
        created_at: Timestamp,
        operation_id: &'static str,
    ) -> Result<WriteResult, ApiError> {
        let report = match intake(&loaded.graph, proposal, &IntakeValidationInput::empty()) {
            Ok(report) => report,
            Err(IntakeError::Patch(error)) => {
                return Err(ApiError::unprocessable(
                    "E_PATCH_REJECTED",
                    "the proposed change cannot be applied to the current graph",
                )
                .with_details(json!({ "patch_error": error.to_string() })))
            }
            Err(_) => return Err(ApiError::internal("intake")),
        };
        match validate_required_rationale(proposal, &report, None) {
            Ok(()) => {}
            Err(IntakeAcceptanceError::ReasonRequired(_)) => {
                let codes: Vec<String> =
                    report.reason_codes.iter().map(|c| c.to_string()).collect();
                return Err(ApiError::conflict(
                    "E_INTAKE_REASON_REQUIRED",
                    "intake requires a recorded reason before this change can be accepted",
                )
                .with_details(json!({
                    "proposal_ref": proposal.id,
                    "reason_codes": codes,
                })));
            }
            Err(_) => return Err(ApiError::internal("intake_rationale")),
        }
        let meta = RevisionWriteMeta {
            decision_refs: Vec::new(),
            created_by: self.inner.identity.actor_ref.clone(),
            created_at,
        };
        let committed = self.with_revision_store(|store| {
            store.commit(
                &self.inner.branch,
                &loaded.revision.id,
                &proposal.patch_set,
                meta,
            )
        })?;
        let result = match committed {
            Ok(result) => result,
            Err(StoreError::StaleBase { .. }) => {
                let current = self.load_head()?;
                return Err(precondition_failed(&current.revision));
            }
            Err(StoreError::Patch(error)) => {
                return Err(ApiError::unprocessable(
                    "E_PATCH_REJECTED",
                    "the proposed change cannot be applied to the current graph",
                )
                .with_details(json!({ "patch_error": error.to_string() })))
            }
            Err(_) => return Err(ApiError::internal("revision_store")),
        };
        let delta = &result.delta;
        let mut ids: Vec<String> = delta
            .added_nodes
            .iter()
            .chain(&delta.removed_nodes)
            .chain(&delta.modified_nodes)
            .chain(&delta.added_edges)
            .chain(&delta.removed_edges)
            .chain(&delta.modified_edges)
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        ids.dedup();
        let revision = result.revision.id.to_string();
        let event = DesignEvent {
            kind: operation_id.to_owned(),
            ids,
            version: revision.clone(),
        };
        // No subscriber is not an error: the event stream is forward-only.
        let _ = self.inner.events.send(event.clone());
        Ok(WriteResult {
            version: revision.clone(),
            revision,
            semantic_hash: result.revision.semantic_hash.to_string(),
            events: vec![event],
        })
    }
}

/// 412 for `revision`.
pub(crate) fn precondition_failed(revision: &GraphRevision) -> ApiError {
    ApiError::precondition_failed(revision.id.to_string(), revision.semantic_hash.to_string())
}

/// Compares `If-Match` literally with the loaded head's semantic hash.
pub(crate) fn check_if_match(headers: &HeaderMap, loaded: &LoadedRevision) -> Result<(), ApiError> {
    let value = headers
        .get("if-match")
        .ok_or_else(|| ApiError::invalid("the If-Match header is required"))?
        .to_str()
        .map_err(|_| ApiError::invalid("the If-Match header is not valid text"))?;
    if value != loaded.revision.semantic_hash.as_str() {
        return Err(precondition_failed(&loaded.revision));
    }
    Ok(())
}

/// A JSON read response carrying `ETag` = the loaded revision's semantic hash.
pub(crate) fn read_json<T: Serialize>(loaded: &LoadedRevision, body: T) -> Response {
    with_etag(loaded, Json(body).into_response())
}

/// Adds `ETag` = the loaded revision's semantic hash.
pub(crate) fn with_etag(loaded: &LoadedRevision, mut response: Response) -> Response {
    if let Ok(value) = HeaderValue::from_str(loaded.revision.semantic_hash.as_str()) {
        response.headers_mut().insert("etag", value);
    }
    response
}

// ============================================================================ wire vocabulary

/// The serde wire string of a closed enum value.
pub(crate) fn wire<T: Serialize>(value: &T) -> Result<String, ApiError> {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => Ok(s),
        _ => Err(ApiError::internal("wire_vocabulary")),
    }
}

/// Parses a closed enum from its exact serde wire string.
pub(crate) fn parse_wire<T: serde::de::DeserializeOwned>(
    text: &str,
    what: &str,
) -> Result<T, ApiError> {
    serde_json::from_value(Value::String(text.to_owned()))
        .map_err(|_| ApiError::invalid(format!("{what} {text:?} is not a valid value")))
}

/// Non-empty text without surrounding whitespace or control characters.
pub(crate) fn is_clean_text(text: &str) -> bool {
    !text.is_empty() && text.trim() == text && !text.chars().any(char::is_control)
}

/// Parses a JSON request body, mapping any failure to 400.
pub(crate) fn parse_body<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body)
        .map_err(|e| ApiError::invalid(format!("invalid request body: {e}")))
}

// ============================================================================ query and cursor

/// The parsed query string of a request: distinct keys only.
pub(crate) struct QueryParams {
    values: BTreeMap<String, String>,
}

impl QueryParams {
    /// Parses `query`, rejecting repeated keys and keys outside `allowed` (plus `cursor` and
    /// `limit` when `pageable`).
    pub(crate) fn parse(
        query: Option<&str>,
        allowed: &[&str],
        pageable: bool,
    ) -> Result<QueryParams, ApiError> {
        let pairs: Vec<(String, String)> = match query {
            None | Some("") => Vec::new(),
            Some(q) => {
                axum::extract::Query::<Vec<(String, String)>>::try_from_uri(
                    &format!("/?{q}")
                        .parse()
                        .map_err(|_| ApiError::invalid("malformed query string"))?,
                )
                .map_err(|_| ApiError::invalid("malformed query string"))?
                .0
            }
        };
        let mut values = BTreeMap::new();
        for (key, value) in pairs {
            let known = allowed.contains(&key.as_str())
                || (pageable && (key == "cursor" || key == "limit"));
            if !known {
                return Err(ApiError::invalid(format!(
                    "unknown query parameter {key:?}"
                )));
            }
            if values.insert(key.clone(), value).is_some() {
                return Err(ApiError::invalid(format!(
                    "repeated query parameter {key:?}"
                )));
            }
        }
        Ok(QueryParams { values })
    }

    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// The present non-pagination parameters: the request-hash query object.
    fn filters(&self) -> BTreeMap<&str, &str> {
        self.values
            .iter()
            .filter(|(k, _)| k.as_str() != "cursor" && k.as_str() != "limit")
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

/// Cursor v2: exactly these four fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    after_id: String,
    request_hash: Hash,
    revision: RevisionId,
    semantic_hash: Hash,
}

/// Generic SHA-256 of RFC 8785 `{operation_id, path, query}`.
fn request_hash(
    operation_id: &str,
    path: &BTreeMap<&str, &str>,
    query: &BTreeMap<&str, &str>,
) -> Result<Hash, ApiError> {
    let bytes = to_canonical_json(&json!({
        "operation_id": operation_id,
        "path": path,
        "query": query,
    }))
    .map_err(|_| ApiError::internal("request_hash"))?;
    Ok(Hash::content_sha256(&bytes))
}

fn encode_cursor(cursor: &Cursor) -> Result<String, ApiError> {
    let bytes = to_canonical_json(cursor).map_err(|_| ApiError::internal("cursor"))?;
    Ok(hex::encode(bytes))
}

/// Decodes a cursor, accepting only lowercase canonical encodings of the four-field object.
fn decode_cursor(text: &str) -> Result<Cursor, ApiError> {
    let invalid = || ApiError::cursor_invalid("the cursor is malformed");
    if text.is_empty()
        || !text.len().is_multiple_of(2)
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let bytes = hex::decode(text).map_err(|_| invalid())?;
    let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let canonical = to_canonical_json(&cursor).map_err(|_| invalid())?;
    if canonical != bytes
        || cursor.request_hash.kind() != HashKind::Generic
        || cursor.semantic_hash.kind() != HashKind::Semantic
        || cursor.after_id.parse::<Id>().is_err()
    {
        return Err(invalid());
    }
    Ok(cursor)
}

/// One pageable request: its operation, path parameters and query.
pub(crate) struct PageRequest<'a> {
    pub(crate) operation_id: &'static str,
    pub(crate) path: BTreeMap<&'a str, &'a str>,
    pub(crate) query: &'a QueryParams,
}

/// A page of `items` (already filtered) for `request` at `loaded`: sorted by ID, continued
/// exclusively after the cursor's `after_id`, then limited.
pub(crate) fn paginate<T>(
    mut items: Vec<(String, T)>,
    request: &PageRequest<'_>,
    loaded: &LoadedRevision,
) -> Result<(Vec<T>, Option<String>), ApiError> {
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let limit = match request.query.get("limit") {
        None => None,
        Some(text) => match text.parse::<u64>() {
            Ok(n) if n >= 1 => Some(usize::try_from(n).unwrap_or(usize::MAX)),
            _ => return Err(ApiError::invalid("limit must be an integer of at least 1")),
        },
    };
    let request_hash = request_hash(
        request.operation_id,
        &request.path,
        &request.query.filters(),
    )?;
    let mut start = 0;
    if let Some(text) = request.query.get("cursor") {
        let cursor = decode_cursor(text)?;
        if cursor.request_hash != request_hash {
            return Err(ApiError::cursor_request_mismatch());
        }
        if cursor.revision != loaded.revision.id
            || cursor.semantic_hash != loaded.revision.semantic_hash
        {
            return Err(ApiError::cursor_baseline_changed());
        }
        let position = items
            .iter()
            .position(|(id, _)| *id == cursor.after_id)
            .ok_or_else(|| {
                ApiError::cursor_invalid("the cursor's after_id is not in this collection")
            })?;
        start = position + 1;
    }
    let remaining = items.len() - start;
    let take = limit.map_or(remaining, |l| l.min(remaining));
    let end = start + take;
    let next_cursor = if end < items.len() && take > 0 {
        Some(encode_cursor(&Cursor {
            after_id: items[end - 1].0.clone(),
            request_hash,
            revision: loaded.revision.id.clone(),
            semantic_hash: loaded.revision.semantic_hash.clone(),
        })?)
    } else {
        None
    };
    let page = items
        .into_iter()
        .skip(start)
        .take(take)
        .map(|(_, item)| item)
        .collect();
    Ok((page, next_cursor))
}
