//! The SQLite-backed revision store: initial revision, CAS commit, branches and restore.

use std::path::Path;

use plumb_artifacts::{put_artifact_in_transaction, ArtifactKind};
use plumb_core::{to_canonical_json, Hash, HashKind, Timestamp};
use plumb_patch::{apply_patch, PatchSet};
use plumb_psg::Graph;
use rusqlite::{params, Connection, TransactionBehavior};

use crate::branches::{cas_head, read_head, BranchName};
use crate::revisions::{
    insert_revision, load_revision_on, next_version, revision_exists, validate_decision_refs,
    GraphRevision, LoadedRevision, RevisionId, RevisionWriteMeta, PATCH_MEDIA_TYPE,
};
use crate::schema::{open_connection, PSG_SCHEMA_VERSION};
use crate::StoreError;

/// Immutable graph revisions and mutable branch heads in the pilot SQLite database.
///
/// Owns its connection. Every write runs in one `BEGIN IMMEDIATE` transaction, so a failed
/// operation leaves the database unchanged and concurrent writers serialize.
#[derive(Debug)]
pub struct SqliteRevisionStore {
    conn: Connection,
}

impl SqliteRevisionStore {
    /// Opens the store at `path`, initializing store schema v1 in a fresh database (an existing
    /// F0.2 `artifacts` table is kept) or verifying an existing v1 store without modifying it.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let mut conn = Connection::open(path)?;
        open_connection(&mut conn)?;
        Ok(Self { conn })
    }

    /// Persists `graph` as revision 1 and creates branch `main` pointing to it.
    pub fn create_initial_revision(
        &mut self,
        graph: &Graph,
        profile_hash: Hash,
        rule_pack_hash: Hash,
        meta: RevisionWriteMeta,
    ) -> Result<GraphRevision, StoreError> {
        if profile_hash.kind() != HashKind::Generic {
            return Err(StoreError::InvalidProfileHashKind(profile_hash));
        }
        if rule_pack_hash.kind() != HashKind::Generic {
            return Err(StoreError::InvalidRulePackHashKind(rule_pack_hash));
        }
        let decision_refs = validate_decision_refs(graph, &meta.decision_refs)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let initialized: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM graph_revisions) OR EXISTS(SELECT 1 FROM branch_heads)",
            [],
            |row| row.get(0),
        )?;
        if initialized {
            return Err(StoreError::AlreadyInitialized);
        }
        let semantic_hash = graph.semantic_hash()?;
        let revision = GraphRevision {
            id: RevisionId::new(1, &semantic_hash)?,
            version: 1,
            parent: None,
            project_id: graph.project_id().clone(),
            psg_schema_version: PSG_SCHEMA_VERSION,
            semantic_hash,
            evidence_hash: graph.evidence_hash()?,
            profile_ref: graph.profile_id().clone(),
            profile_hash,
            rule_pack_hash,
            accepted_patch_ref: None,
            decision_refs,
            created_by: meta.created_by,
            created_at: meta.created_at,
        };
        insert_revision(&tx, &revision, graph)?;
        tx.execute(
            "INSERT INTO branch_heads (name, revision_id, updated_at) VALUES (?1, ?2, ?3)",
            params![
                BranchName::main().as_str(),
                revision.id.as_str(),
                revision.created_at.to_string()
            ],
        )?;
        tx.commit()?;
        Ok(revision)
    }

    /// Applies `patch_set` to the head of `branch` and advances the branch to the new revision,
    /// provided the head is still `expected_head`.
    ///
    /// The accepted Patch artifact, the revision, its snapshot and the head move are written in
    /// one transaction. A moved head is `StaleBase` and a semantic-hash mismatch is the
    /// underlying `PatchError`; neither is ever rebased or retried.
    pub fn commit(
        &mut self,
        branch: &BranchName,
        expected_head: &RevisionId,
        patch_set: &PatchSet,
        meta: RevisionWriteMeta,
    ) -> Result<GraphRevision, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual =
            read_head(&tx, branch)?.ok_or_else(|| StoreError::BranchNotFound(branch.clone()))?;
        if actual != *expected_head {
            return Err(StoreError::StaleBase {
                branch: branch.clone(),
                expected: expected_head.clone(),
                actual,
            });
        }
        let head = load_revision_on(&tx, expected_head)?;
        let applied = apply_patch(&head.graph, patch_set)?;
        let decision_refs = validate_decision_refs(&applied.graph, &meta.decision_refs)?;
        let patch_bytes = to_canonical_json(patch_set)?;
        let patch_ref = put_artifact_in_transaction(
            &tx,
            ArtifactKind::Patch,
            PATCH_MEDIA_TYPE,
            &patch_bytes,
            meta.created_at,
        )?;
        let version = next_version(&tx)?;
        let semantic_hash = applied.graph.semantic_hash()?;
        let revision = GraphRevision {
            id: RevisionId::new(version, &semantic_hash)?,
            version,
            parent: Some(expected_head.clone()),
            project_id: head.revision.project_id,
            psg_schema_version: PSG_SCHEMA_VERSION,
            semantic_hash,
            evidence_hash: applied.graph.evidence_hash()?,
            profile_ref: head.revision.profile_ref,
            profile_hash: head.revision.profile_hash,
            rule_pack_hash: head.revision.rule_pack_hash,
            accepted_patch_ref: Some(patch_ref),
            decision_refs,
            created_by: meta.created_by,
            created_at: meta.created_at,
        };
        insert_revision(&tx, &revision, &applied.graph)?;
        cas_head(
            &tx,
            branch,
            expected_head,
            &revision.id,
            &revision.created_at.to_string(),
        )?;
        tx.commit()?;
        Ok(revision)
    }

    /// Loads revision `id` and verifies its metadata, snapshot, hashes and Patch artifact.
    pub fn load_revision(&self, id: &RevisionId) -> Result<LoadedRevision, StoreError> {
        load_revision_on(&self.conn, id)
    }

    /// The head of `branch`, or `None` if the branch does not exist.
    pub fn head(&self, branch: &BranchName) -> Result<Option<RevisionId>, StoreError> {
        read_head(&self.conn, branch)
    }

    /// Creates branch `name` pointing at the existing revision `from`. No revision is created.
    pub fn create_branch(
        &mut self,
        name: BranchName,
        from: RevisionId,
        updated_at: Timestamp,
    ) -> Result<(), StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !revision_exists(&tx, &from)? {
            return Err(StoreError::RevisionNotFound(from));
        }
        if read_head(&tx, &name)?.is_some() {
            return Err(StoreError::BranchAlreadyExists(name));
        }
        tx.execute(
            "INSERT INTO branch_heads (name, revision_id, updated_at) VALUES (?1, ?2, ?3)",
            params![name.as_str(), from.as_str(), updated_at.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Moves `branch` from `expected_head` to the existing revision `target` (explicit restore).
    /// Creates no revision and deletes nothing.
    pub fn move_head(
        &mut self,
        branch: &BranchName,
        expected_head: &RevisionId,
        target: &RevisionId,
        updated_at: Timestamp,
    ) -> Result<(), StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual =
            read_head(&tx, branch)?.ok_or_else(|| StoreError::BranchNotFound(branch.clone()))?;
        if !revision_exists(&tx, target)? {
            return Err(StoreError::RevisionNotFound(target.clone()));
        }
        if actual != *expected_head {
            return Err(StoreError::StaleBase {
                branch: branch.clone(),
                expected: expected_head.clone(),
                actual,
            });
        }
        cas_head(&tx, branch, expected_head, target, &updated_at.to_string())?;
        tx.commit()?;
        Ok(())
    }
}
