//! Immutable graph revisions: identifiers, metadata, full-snapshot persistence and
//! integrity-verified loading (compiler architecture §4.1; plan §6).

use std::fmt;
use std::str::FromStr;

use plumb_artifacts::ArtifactKind;
use plumb_core::{to_canonical_json, CoreError, Hash, HashKind, Id, Timestamp};
use plumb_patch::PatchSet;
use plumb_psg::{is_baseline, Edge, Graph, Node, NodeType};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::schema::PSG_SCHEMA_VERSION;
use crate::StoreError;

const REVISION_PREFIX: &str = "rev:";
const REVISION_DIGEST_LEN: usize = 16;

/// The media type of every accepted Patch artifact.
pub(crate) const PATCH_MEDIA_TYPE: &str = "application/json";

/// A string or input that does not form a canonical revision ID.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid revision id {0:?}")]
pub struct InvalidRevisionId(pub String);

/// `rev:<positive decimal version>:<first 16 hex digits of the semantic hash digest>`.
///
/// Serialized as the bare string. The version has no leading zeros and the digest is lowercase.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RevisionId {
    text: String,
    version: u64,
}

impl RevisionId {
    /// Builds the revision ID for `version` and a Semantic (`psg:sha256:`) `semantic_hash`.
    pub fn new(version: u64, semantic_hash: &Hash) -> Result<RevisionId, InvalidRevisionId> {
        let digest = semantic_hash
            .as_str()
            .strip_prefix("psg:sha256:")
            .filter(|_| semantic_hash.kind() == HashKind::Semantic);
        match digest {
            Some(digest) if version > 0 => Ok(RevisionId {
                text: format!(
                    "{REVISION_PREFIX}{version}:{}",
                    &digest[..REVISION_DIGEST_LEN]
                ),
                version,
            }),
            _ => Err(InvalidRevisionId(format!(
                "{REVISION_PREFIX}{version}:{semantic_hash}"
            ))),
        }
    }

    /// The global revision version encoded in the ID.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Returns the exact canonical ID string.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

fn parse_revision_id(s: &str) -> Option<u64> {
    let (version, digest) = s.strip_prefix(REVISION_PREFIX)?.split_once(':')?;
    let canonical_version = version
        .bytes()
        .next()
        .is_some_and(|b| matches!(b, b'1'..=b'9'))
        && version.bytes().all(|b| b.is_ascii_digit());
    let canonical_digest = digest.len() == REVISION_DIGEST_LEN
        && digest
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if canonical_version && canonical_digest {
        version.parse().ok()
    } else {
        None
    }
}

impl TryFrom<String> for RevisionId {
    type Error = InvalidRevisionId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match parse_revision_id(&value) {
            Some(version) => Ok(RevisionId {
                text: value,
                version,
            }),
            None => Err(InvalidRevisionId(value)),
        }
    }
}

impl FromStr for RevisionId {
    type Err = InvalidRevisionId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.to_owned().try_into()
    }
}

impl fmt::Display for RevisionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Serialize for RevisionId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for RevisionId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// An immutable accepted graph revision (compiler architecture §4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphRevision {
    pub id: RevisionId,
    pub version: u64,
    pub parent: Option<RevisionId>,

    pub project_id: Id,
    pub psg_schema_version: u32,

    pub semantic_hash: Hash,
    pub evidence_hash: Hash,

    pub profile_ref: Id,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,

    pub accepted_patch_ref: Option<Hash>,
    pub decision_refs: Vec<Id>,

    pub created_by: Id,
    pub created_at: Timestamp,
}

/// Caller-supplied provenance for a new revision. The store never reads a clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionWriteMeta {
    /// Baseline `ResolutionDecision` nodes of the resulting graph; unique, persisted sorted.
    pub decision_refs: Vec<Id>,
    /// Provenance only: any valid `Id`, never used for authorization.
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// A revision together with its reconstructed, validated graph.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedRevision {
    pub revision: GraphRevision,
    pub graph: Graph,
}

/// Validates decision refs against `graph` and returns them sorted.
pub(crate) fn validate_decision_refs(graph: &Graph, refs: &[Id]) -> Result<Vec<Id>, StoreError> {
    let mut sorted = refs.to_vec();
    sorted.sort();
    if let Some(pair) = sorted.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(StoreError::DuplicateDecisionRef(pair[0].clone()));
    }
    for id in &sorted {
        let node = graph
            .node(id)
            .ok_or_else(|| StoreError::DecisionRefMissing(id.clone()))?;
        let node_type = node.payload.node_type();
        if node_type != NodeType::ResolutionDecision {
            return Err(StoreError::DecisionRefWrongType {
                id: id.clone(),
                node_type,
            });
        }
        if !is_baseline(node.status) {
            return Err(StoreError::DecisionRefNotBaseline {
                id: id.clone(),
                status: node.status,
            });
        }
    }
    Ok(sorted)
}

fn canonical_text<T: Serialize>(value: &T) -> Result<String, StoreError> {
    let bytes = to_canonical_json(value)?;
    String::from_utf8(bytes).map_err(|e| CoreError::Canonicalization(e.to_string()).into())
}

/// The next database-global revision version.
pub(crate) fn next_version(conn: &Connection) -> Result<u64, StoreError> {
    let max: Option<i64> =
        conn.query_row("SELECT MAX(version) FROM graph_revisions", [], |row| {
            row.get(0)
        })?;
    let next = max
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(StoreError::VersionOverflow)?;
    u64::try_from(next).map_err(|_| StoreError::VersionOverflow)
}

/// Inserts `revision` and the full node/edge snapshot of `graph`.
pub(crate) fn insert_revision(
    conn: &Connection,
    revision: &GraphRevision,
    graph: &Graph,
) -> Result<(), StoreError> {
    let version = i64::try_from(revision.version).map_err(|_| StoreError::VersionOverflow)?;
    conn.execute(
        "INSERT INTO graph_revisions (id, version, parent_id, project_id, psg_schema_version, \
         semantic_hash, evidence_hash, profile_ref, profile_hash, rule_pack_hash, \
         patch_artifact_hash, decision_refs_json, created_by, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            revision.id.as_str(),
            version,
            revision.parent.as_ref().map(RevisionId::as_str),
            revision.project_id.as_str(),
            revision.psg_schema_version,
            revision.semantic_hash.as_str(),
            revision.evidence_hash.as_str(),
            revision.profile_ref.as_str(),
            revision.profile_hash.as_str(),
            revision.rule_pack_hash.as_str(),
            revision.accepted_patch_ref.as_ref().map(Hash::as_str),
            serde_json::to_string(&revision.decision_refs)?,
            revision.created_by.as_str(),
            revision.created_at.to_string(),
        ],
    )?;
    let mut node_stmt = conn.prepare(
        "INSERT INTO revision_nodes (revision_id, node_id, node_json) VALUES (?1, ?2, ?3)",
    )?;
    for (id, node) in graph.nodes() {
        node_stmt.execute(params![
            revision.id.as_str(),
            id.as_str(),
            canonical_text(node)?
        ])?;
    }
    let mut edge_stmt = conn.prepare(
        "INSERT INTO revision_edges (revision_id, edge_id, edge_json) VALUES (?1, ?2, ?3)",
    )?;
    for (id, edge) in graph.edges() {
        edge_stmt.execute(params![
            revision.id.as_str(),
            id.as_str(),
            canonical_text(edge)?
        ])?;
    }
    Ok(())
}

pub(crate) fn revision_exists(conn: &Connection, id: &RevisionId) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM graph_revisions WHERE id = ?1)",
        [id.as_str()],
        |row| row.get(0),
    )?)
}

/// The raw persisted `graph_revisions` row.
struct RevisionRow {
    id: String,
    version: i64,
    parent_id: Option<String>,
    project_id: String,
    psg_schema_version: i64,
    semantic_hash: String,
    evidence_hash: String,
    profile_ref: String,
    profile_hash: String,
    rule_pack_hash: String,
    patch_artifact_hash: Option<String>,
    decision_refs_json: String,
    created_by: String,
    created_at: String,
}

fn read_row(conn: &Connection, id: &str) -> Result<Option<RevisionRow>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT id, version, parent_id, project_id, psg_schema_version, semantic_hash, \
             evidence_hash, profile_ref, profile_hash, rule_pack_hash, patch_artifact_hash, \
             decision_refs_json, created_by, created_at FROM graph_revisions WHERE id = ?1",
            [id],
            |r| {
                Ok(RevisionRow {
                    id: r.get(0)?,
                    version: r.get(1)?,
                    parent_id: r.get(2)?,
                    project_id: r.get(3)?,
                    psg_schema_version: r.get(4)?,
                    semantic_hash: r.get(5)?,
                    evidence_hash: r.get(6)?,
                    profile_ref: r.get(7)?,
                    profile_hash: r.get(8)?,
                    rule_pack_hash: r.get(9)?,
                    patch_artifact_hash: r.get(10)?,
                    decision_refs_json: r.get(11)?,
                    created_by: r.get(12)?,
                    created_at: r.get(13)?,
                })
            },
        )
        .optional()?)
}

/// Builds a `CorruptRevision` error for the revision `id`.
fn corrupt(id: &str, reason: impl fmt::Display) -> StoreError {
    StoreError::CorruptRevision {
        revision: id.to_owned(),
        reason: reason.to_string(),
    }
}

fn parse_field<T: FromStr>(id: &str, field: &str, value: &str) -> Result<T, StoreError>
where
    T::Err: fmt::Display,
{
    value
        .parse()
        .map_err(|e: T::Err| corrupt(id, format!("{field}: {e}")))
}

fn parse_hash(id: &str, field: &str, value: &str, kind: HashKind) -> Result<Hash, StoreError> {
    let hash: Hash = parse_field(id, field, value)?;
    if hash.kind() != kind {
        return Err(corrupt(
            id,
            format!("{field} {hash} is not a {kind:?} hash"),
        ));
    }
    Ok(hash)
}

/// Verifies every metadata-only integrity rule of a stored row (no graph, no patch artifact).
fn parse_metadata(row: RevisionRow) -> Result<GraphRevision, StoreError> {
    let id_text = row.id.clone();
    let rid = id_text.as_str();
    let id: RevisionId = parse_field(rid, "id", rid)?;
    if row.version <= 0 {
        return Err(corrupt(
            rid,
            format!("version {} is not positive", row.version),
        ));
    }
    let version = row.version as u64;
    let semantic_hash = parse_hash(rid, "semantic_hash", &row.semantic_hash, HashKind::Semantic)?;
    let evidence_hash = parse_hash(rid, "evidence_hash", &row.evidence_hash, HashKind::Evidence)?;
    let derived = RevisionId::new(version, &semantic_hash)?;
    if derived != id {
        return Err(corrupt(
            rid,
            format!("id does not match derived id {derived}"),
        ));
    }
    let profile_hash = parse_hash(rid, "profile_hash", &row.profile_hash, HashKind::Generic)?;
    let rule_pack_hash = parse_hash(
        rid,
        "rule_pack_hash",
        &row.rule_pack_hash,
        HashKind::Generic,
    )?;
    let accepted_patch_ref = match row.patch_artifact_hash {
        None => None,
        Some(text) => {
            let hash: Hash = parse_field(rid, "patch_artifact_hash", &text)?;
            if hash.kind() != HashKind::Generic {
                return Err(StoreError::InvalidPatchArtifactHashKind {
                    revision: row.id,
                    hash: text,
                });
            }
            Some(hash)
        }
    };
    let parent = row
        .parent_id
        .as_deref()
        .map(|p| parse_field::<RevisionId>(rid, "parent_id", p))
        .transpose()?;
    let project_id: Id = parse_field(rid, "project_id", &row.project_id)?;
    let profile_ref: Id = parse_field(rid, "profile_ref", &row.profile_ref)?;
    let created_by: Id = parse_field(rid, "created_by", &row.created_by)?;
    let created_at: Timestamp = parse_field(rid, "created_at", &row.created_at)?;
    if created_at.to_string() != row.created_at {
        return Err(corrupt(rid, "created_at is not canonical"));
    }
    let decision_refs: Vec<Id> = serde_json::from_str(&row.decision_refs_json)
        .map_err(|e| corrupt(rid, format!("decision_refs_json: {e}")))?;
    if decision_refs.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(corrupt(rid, "decision_refs_json is not unique and sorted"));
    }
    if row.psg_schema_version != i64::from(PSG_SCHEMA_VERSION) {
        return Err(StoreError::UnsupportedPsgSchemaVersion {
            revision: row.id,
            found: row.psg_schema_version,
        });
    }
    match (&parent, &accepted_patch_ref) {
        (None, None) if version == 1 => {}
        (Some(_), Some(_)) if version > 1 => {}
        _ => {
            return Err(corrupt(
                rid,
                "initial revisions need version 1 and no parent or patch; \
                 committed revisions need a parent and a patch",
            ))
        }
    }
    Ok(GraphRevision {
        id,
        version,
        parent,
        project_id,
        psg_schema_version: PSG_SCHEMA_VERSION,
        semantic_hash,
        evidence_hash,
        profile_ref,
        profile_hash,
        rule_pack_hash,
        accepted_patch_ref,
        decision_refs,
        created_by,
        created_at,
    })
}

fn load_elements<T: serde::de::DeserializeOwned + Serialize>(
    conn: &Connection,
    rid: &str,
    sql: &str,
    id_of: impl Fn(&T) -> &Id,
) -> Result<Vec<T>, StoreError> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([rid], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(element_id, json)| {
            let element: T = serde_json::from_str(&json)
                .map_err(|e| corrupt(rid, format!("element {element_id}: {e}")))?;
            if id_of(&element).as_str() != element_id {
                return Err(corrupt(
                    rid,
                    format!("element row {element_id} holds another id"),
                ));
            }
            if canonical_text(&element)? != json {
                return Err(corrupt(
                    rid,
                    format!("element {element_id} is not canonical JSON"),
                ));
            }
            Ok(element)
        })
        .collect()
}

/// Verifies the accepted Patch artifact of a committed revision against its parent.
fn verify_patch_artifact(
    conn: &Connection,
    revision: &GraphRevision,
    patch_ref: &Hash,
    parent: &RevisionId,
) -> Result<(), StoreError> {
    let rid = revision.id.as_str();
    let artifact: Option<(String, String, Vec<u8>)> = conn
        .query_row(
            "SELECT kind, media_type, bytes FROM artifacts WHERE hash = ?1",
            [patch_ref.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (kind, media_type, bytes) =
        artifact.ok_or_else(|| corrupt(rid, format!("patch artifact {patch_ref} is missing")))?;
    if kind != ArtifactKind::Patch.as_str() || media_type != PATCH_MEDIA_TYPE {
        return Err(corrupt(
            rid,
            format!("patch artifact {patch_ref} is {kind} ({media_type})"),
        ));
    }
    if Hash::content_sha256(&bytes) != *patch_ref {
        return Err(corrupt(
            rid,
            format!("patch artifact {patch_ref} bytes do not match"),
        ));
    }
    let patch_set: PatchSet = serde_json::from_slice(&bytes)
        .map_err(|e| corrupt(rid, format!("patch artifact is not a PatchSet: {e}")))?;
    let parent_row = read_row(conn, parent.as_str())?
        .ok_or_else(|| corrupt(rid, format!("parent {parent} is missing")))?;
    let parent_meta = parse_metadata(parent_row)?;
    if patch_set.base_semantic_hash != parent_meta.semantic_hash {
        return Err(corrupt(
            rid,
            "patch base_semantic_hash differs from the parent",
        ));
    }
    let inherited = parent_meta.project_id == revision.project_id
        && parent_meta.profile_ref == revision.profile_ref
        && parent_meta.profile_hash == revision.profile_hash
        && parent_meta.rule_pack_hash == revision.rule_pack_hash;
    if !inherited {
        return Err(corrupt(
            rid,
            "project/profile metadata differs from the parent",
        ));
    }
    Ok(())
}

/// Loads and fully verifies the revision `id` on `conn` (plan F0.8 load-integrity checks).
pub(crate) fn load_revision_on(
    conn: &Connection,
    id: &RevisionId,
) -> Result<LoadedRevision, StoreError> {
    let row =
        read_row(conn, id.as_str())?.ok_or_else(|| StoreError::RevisionNotFound(id.clone()))?;
    // Metadata (including the PSG schema version) is verified before any Graph is built.
    let revision = parse_metadata(row)?;
    let rid = revision.id.as_str();
    let nodes: Vec<Node> = load_elements(
        conn,
        rid,
        "SELECT node_id, node_json FROM revision_nodes WHERE revision_id = ?1 ORDER BY node_id",
        |n: &Node| &n.id,
    )?;
    let edges: Vec<Edge> = load_elements(
        conn,
        rid,
        "SELECT edge_id, edge_json FROM revision_edges WHERE revision_id = ?1 ORDER BY edge_id",
        |e: &Edge| &e.id,
    )?;
    let graph = Graph::new(
        revision.project_id.clone(),
        revision.profile_ref.clone(),
        nodes,
        edges,
    )
    .map_err(|v| corrupt(rid, format!("graph is invalid: {v:?}")))?;
    if graph.semantic_hash()? != revision.semantic_hash {
        return Err(corrupt(rid, "semantic_hash does not match the snapshot"));
    }
    if graph.evidence_hash()? != revision.evidence_hash {
        return Err(corrupt(rid, "evidence_hash does not match the snapshot"));
    }
    validate_decision_refs(&graph, &revision.decision_refs)
        .map_err(|e| corrupt(rid, format!("decision refs: {e}")))?;
    if let (Some(patch_ref), Some(parent)) = (&revision.accepted_patch_ref, &revision.parent) {
        verify_patch_artifact(conn, &revision, patch_ref, parent)?;
    }
    Ok(LoadedRevision { revision, graph })
}
