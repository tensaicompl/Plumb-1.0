//! Durable workflow state of the pilot API (Hotfix 065): pending proposals, sessions and session
//! turns in the workflow tables (store schema v2; v3 adds the `stage` origin, Hotfix 067).
//!
//! The store keeps canonical opaque JSON (RFC 8785 text) together with the workflow metadata and
//! owns identity, state transitions, canonical bytes, lookup, durability and uniqueness. Typed
//! decoding of the stored proposal, intake report and session belongs to the layer above; the
//! store never interprets them. Workflow rows never participate in a revision, its semantic or
//! evidence hash or its identity.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use plumb_core::{to_canonical_json, Id, Timestamp};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction, TransactionBehavior};
use serde_json::Value;

use crate::revisions::{revision_exists, RevisionId};
use crate::schema::open_connection;
use crate::StoreError;

/// Declares a closed workflow vocabulary stored as its exact lowercase string.
macro_rules! workflow_vocabulary {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// The exact stored string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($text => Ok($name::$variant),)+
                    other => Err(other.to_owned()),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

workflow_vocabulary!(
    /// The lifecycle state of a workflow proposal.
    WorkflowProposalState {
        Prepared => "prepared",
        Pending => "pending",
        Accepted => "accepted",
        Rejected => "rejected",
        Failed => "failed",
    }
);

workflow_vocabulary!(
    /// Where a workflow proposal came from.
    WorkflowOrigin {
        Intake => "intake",
        Question => "question",
        Session => "session",
        Stage => "stage",
    }
);

workflow_vocabulary!(
    /// The workflow action class of a proposal (the API `ProposalKind`).
    WorkflowProposalKind {
        Link => "link",
        Merge => "merge",
        Supersede => "supersede",
        Create => "create",
        Accept => "accept",
    }
);

workflow_vocabulary!(
    /// The lifecycle state of a session turn.
    WorkflowTurnState {
        Prepared => "prepared",
        Committed => "committed",
        Failed => "failed",
    }
);

/// A proposal to persist. `proposal_json` and `intake_json` are stored as their RFC 8785 text.
#[derive(Debug, Clone, PartialEq)]
pub struct NewWorkflowProposal {
    pub proposal_id: Id,
    pub base_revision_id: RevisionId,
    pub proposal_json: Value,
    pub intake_json: Value,
    pub proposal_kind: WorkflowProposalKind,
    pub origin_kind: WorkflowOrigin,
    pub origin_ref: Option<Id>,
    pub created_at: Timestamp,
}

/// A persisted workflow proposal. The JSON fields are the exact stored canonical text.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowProposal {
    pub proposal_id: Id,
    pub base_revision_id: RevisionId,
    pub proposal_json: String,
    pub intake_json: String,
    pub proposal_kind: WorkflowProposalKind,
    pub state: WorkflowProposalState,
    pub origin_kind: WorkflowOrigin,
    pub origin_ref: Option<Id>,
    pub created_at: Timestamp,
    pub resolved_at: Option<Timestamp>,
    pub resolved_revision_id: Option<RevisionId>,
}

/// The result of inserting a proposal or session whose ID may already be stored.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowInsert<T> {
    /// The record was inserted.
    Inserted(T),
    /// A record with the same ID already exists; it is returned unchanged.
    Existing(T),
}

/// A session to persist; `session_json` is stored as its RFC 8785 text.
#[derive(Debug, Clone, PartialEq)]
pub struct NewWorkflowSession {
    pub session_id: Id,
    pub session_json: Value,
    pub owner_actor_ref: Id,
    pub created_at: Timestamp,
}

/// A persisted session.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowSession {
    pub session_id: Id,
    pub session_json: String,
    pub owner_actor_ref: Id,
    pub created_at: Timestamp,
}

/// A prepared session turn to persist together with its generated proposals.
#[derive(Debug, Clone, PartialEq)]
pub struct NewWorkflowTurn {
    pub session_id: Id,
    pub base_revision_id: RevisionId,
    pub stage: String,
    pub expected_node_refs: Vec<Id>,
    pub proposal_ids: Vec<Id>,
    pub requires_reason_count: u32,
    pub created_at: Timestamp,
}

/// A persisted session turn.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowTurn {
    pub session_id: Id,
    pub turn_index: u32,
    pub state: WorkflowTurnState,
    pub base_revision_id: RevisionId,
    pub stage: String,
    pub expected_node_refs: Vec<Id>,
    pub proposal_ids: Vec<Id>,
    pub requires_reason_count: u32,
    pub committed_revision_id: Option<RevisionId>,
    pub created_at: Timestamp,
    pub failure: Option<String>,
}

/// Durable workflow records in the pilot SQLite database (store schema v2).
///
/// Owns its own connection to the same database as the revision store. Every write runs in one
/// `BEGIN IMMEDIATE` transaction, so a failed operation leaves the database unchanged.
#[derive(Debug)]
pub struct SqliteWorkflowStore {
    conn: Connection,
}

const PROPOSAL_COLUMNS: &str = "proposal_id, base_revision_id, proposal_json, intake_json, \
     proposal_kind, state, origin_kind, origin_ref, created_at, resolved_at, resolved_revision_id";

const TURN_COLUMNS: &str = "session_id, turn_index, state, base_revision_id, stage, \
     expected_node_refs_json, proposal_ids_json, requires_reason_count, committed_revision_id, \
     created_at, failure";

impl SqliteWorkflowStore {
    /// Opens the workflow store at `path`, initializing or migrating the database to store
    /// schema v2 exactly as [`crate::SqliteRevisionStore::open`] does.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let mut conn = Connection::open(path)?;
        open_connection(&mut conn)?;
        Ok(Self { conn })
    }

    // ------------------------------------------------------------------ proposals

    /// Inserts `proposal` in `state` (only `prepared` or `pending` are initial states). An
    /// existing row with the same ID is returned unchanged as [`WorkflowInsert::Existing`]; the
    /// caller decides whether it is the identical record.
    pub fn insert_proposal(
        &mut self,
        proposal: &NewWorkflowProposal,
        state: WorkflowProposalState,
    ) -> Result<WorkflowInsert<WorkflowProposal>, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = proposal_on(&tx, &proposal.proposal_id)? {
            return Ok(WorkflowInsert::Existing(existing));
        }
        insert_proposal_on(&tx, proposal, state)?;
        let inserted = proposal_on(&tx, &proposal.proposal_id)?.ok_or_else(|| {
            StoreError::WorkflowRecordNotFound {
                kind: "proposal",
                id: proposal.proposal_id.to_string(),
            }
        })?;
        tx.commit()?;
        Ok(WorkflowInsert::Inserted(inserted))
    }

    /// The proposal with `id`, if stored.
    pub fn proposal(&self, id: &Id) -> Result<Option<WorkflowProposal>, StoreError> {
        proposal_on(&self.conn, id)
    }

    /// Moves a `pending` proposal to `accepted` or `rejected` with its resolution revision and
    /// time. Any other current state is [`StoreError::WorkflowStateConflict`].
    pub fn resolve_proposal(
        &mut self,
        id: &Id,
        to: WorkflowProposalState,
        resolved_revision_id: &RevisionId,
        resolved_at: Timestamp,
    ) -> Result<WorkflowProposal, StoreError> {
        if !matches!(
            to,
            WorkflowProposalState::Accepted | WorkflowProposalState::Rejected
        ) {
            return Err(StoreError::WorkflowInvalidTransition {
                kind: "proposal",
                id: id.to_string(),
                to: to.as_str(),
            });
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = proposal_on(&tx, id)?.ok_or_else(|| StoreError::WorkflowRecordNotFound {
            kind: "proposal",
            id: id.to_string(),
        })?;
        expect_state(
            "proposal",
            id,
            current.state.as_str(),
            WorkflowProposalState::Pending.as_str(),
        )?;
        require_revision(&tx, resolved_revision_id)?;
        tx.execute(
            "UPDATE workflow_proposals SET state = ?1, resolved_at = ?2, \
             resolved_revision_id = ?3 WHERE proposal_id = ?4 AND state = 'pending'",
            params![
                to.as_str(),
                resolved_at.to_string(),
                resolved_revision_id.as_str(),
                id.as_str()
            ],
        )?;
        let updated = proposal_on(&tx, id)?.ok_or_else(|| StoreError::WorkflowRecordNotFound {
            kind: "proposal",
            id: id.to_string(),
        })?;
        tx.commit()?;
        Ok(updated)
    }

    // ------------------------------------------------------------------ sessions

    /// Inserts `session`; an existing row with the same ID is returned unchanged.
    pub fn insert_session(
        &mut self,
        session: &NewWorkflowSession,
    ) -> Result<WorkflowInsert<WorkflowSession>, StoreError> {
        let json = canonical_text(&session.session_json)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = session_on(&tx, &session.session_id)? {
            return Ok(WorkflowInsert::Existing(existing));
        }
        tx.execute(
            "INSERT INTO workflow_sessions (session_id, session_json, owner_actor_ref, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                session.session_id.as_str(),
                json,
                session.owner_actor_ref.as_str(),
                session.created_at.to_string()
            ],
        )?;
        let inserted = session_on(&tx, &session.session_id)?.ok_or_else(|| {
            StoreError::WorkflowRecordNotFound {
                kind: "session",
                id: session.session_id.to_string(),
            }
        })?;
        tx.commit()?;
        Ok(WorkflowInsert::Inserted(inserted))
    }

    /// The session with `id`, if stored.
    pub fn session(&self, id: &Id) -> Result<Option<WorkflowSession>, StoreError> {
        session_on(&self.conn, id)
    }

    // ------------------------------------------------------------------ session turns

    /// Persists a prepared turn (the next turn index of its session) and its generated
    /// proposals in state `prepared`, atomically. Every proposal must be listed in
    /// `turn.proposal_ids`, in that order, and none may already exist.
    pub fn prepare_turn(
        &mut self,
        turn: &NewWorkflowTurn,
        proposals: &[NewWorkflowProposal],
    ) -> Result<WorkflowTurn, StoreError> {
        let listed: Vec<&Id> = proposals.iter().map(|p| &p.proposal_id).collect();
        if listed != turn.proposal_ids.iter().collect::<Vec<_>>() {
            return Err(StoreError::CorruptWorkflowRecord {
                kind: "turn",
                id: turn.session_id.to_string(),
                reason: "the turn's proposal_ids do not list its proposals".to_owned(),
            });
        }
        let expected_json = canonical_text(&turn.expected_node_refs)?;
        let proposals_json = canonical_text(&turn.proposal_ids)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if session_on(&tx, &turn.session_id)?.is_none() {
            return Err(StoreError::WorkflowRecordNotFound {
                kind: "session",
                id: turn.session_id.to_string(),
            });
        }
        let next: i64 = tx.query_row(
            "SELECT COALESCE(MAX(turn_index) + 1, 0) FROM workflow_session_turns \
             WHERE session_id = ?1",
            [turn.session_id.as_str()],
            |row| row.get(0),
        )?;
        let turn_index = u32::try_from(next).map_err(|_| StoreError::CorruptWorkflowRecord {
            kind: "turn",
            id: turn.session_id.to_string(),
            reason: "turn index overflow".to_owned(),
        })?;
        for proposal in proposals {
            if proposal_on(&tx, &proposal.proposal_id)?.is_some() {
                return Err(StoreError::WorkflowRecordExists {
                    kind: "proposal",
                    id: proposal.proposal_id.to_string(),
                });
            }
            insert_proposal_on(&tx, proposal, WorkflowProposalState::Prepared)?;
        }
        require_revision(&tx, &turn.base_revision_id)?;
        tx.execute(
            "INSERT INTO workflow_session_turns (session_id, turn_index, state, base_revision_id, \
             stage, expected_node_refs_json, proposal_ids_json, requires_reason_count, \
             committed_revision_id, created_at, failure) \
             VALUES (?1, ?2, 'prepared', ?3, ?4, ?5, ?6, ?7, NULL, ?8, NULL)",
            params![
                turn.session_id.as_str(),
                i64::from(turn_index),
                turn.base_revision_id.as_str(),
                turn.stage,
                expected_json,
                proposals_json,
                i64::from(turn.requires_reason_count),
                turn.created_at.to_string()
            ],
        )?;
        let stored = turn_on(&tx, &turn.session_id, turn_index)?.ok_or_else(|| {
            StoreError::WorkflowRecordNotFound {
                kind: "turn",
                id: turn.session_id.to_string(),
            }
        })?;
        tx.commit()?;
        Ok(stored)
    }

    /// Moves a `prepared` turn to `committed` (recording `committed_revision_id`) and every one of
    /// its `prepared` proposals to `pending`, bound to `committed_revision_id`, atomically.
    pub fn commit_turn(
        &mut self,
        session_id: &Id,
        turn_index: u32,
        committed_revision_id: &RevisionId,
    ) -> Result<WorkflowTurn, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let turn = prepared_turn(&tx, session_id, turn_index)?;
        require_revision(&tx, committed_revision_id)?;
        for proposal_id in &turn.proposal_ids {
            let proposal = proposal_on(&tx, proposal_id)?.ok_or_else(|| {
                StoreError::CorruptWorkflowRecord {
                    kind: "turn",
                    id: session_id.to_string(),
                    reason: format!("listed proposal {proposal_id} is missing"),
                }
            })?;
            expect_state(
                "proposal",
                proposal_id,
                proposal.state.as_str(),
                WorkflowProposalState::Prepared.as_str(),
            )?;
            tx.execute(
                "UPDATE workflow_proposals SET state = 'pending', base_revision_id = ?1 \
                 WHERE proposal_id = ?2 AND state = 'prepared'",
                params![committed_revision_id.as_str(), proposal_id.as_str()],
            )?;
        }
        tx.execute(
            "UPDATE workflow_session_turns SET state = 'committed', committed_revision_id = ?1 \
             WHERE session_id = ?2 AND turn_index = ?3 AND state = 'prepared'",
            params![
                committed_revision_id.as_str(),
                session_id.as_str(),
                i64::from(turn_index)
            ],
        )?;
        let stored = turn_on(&tx, session_id, turn_index)?.ok_or_else(|| {
            StoreError::WorkflowRecordNotFound {
                kind: "turn",
                id: session_id.to_string(),
            }
        })?;
        tx.commit()?;
        Ok(stored)
    }

    /// Moves a `prepared` turn and every one of its `prepared` proposals to `failed`,
    /// atomically, recording the stable `failure` name on the turn.
    pub fn fail_turn(
        &mut self,
        session_id: &Id,
        turn_index: u32,
        failure: &str,
    ) -> Result<WorkflowTurn, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let turn = prepared_turn(&tx, session_id, turn_index)?;
        for proposal_id in &turn.proposal_ids {
            tx.execute(
                "UPDATE workflow_proposals SET state = 'failed' \
                 WHERE proposal_id = ?1 AND state = 'prepared'",
                [proposal_id.as_str()],
            )?;
        }
        tx.execute(
            "UPDATE workflow_session_turns SET state = 'failed', failure = ?1 \
             WHERE session_id = ?2 AND turn_index = ?3 AND state = 'prepared'",
            params![failure, session_id.as_str(), i64::from(turn_index)],
        )?;
        let stored = turn_on(&tx, session_id, turn_index)?.ok_or_else(|| {
            StoreError::WorkflowRecordNotFound {
                kind: "turn",
                id: session_id.to_string(),
            }
        })?;
        tx.commit()?;
        Ok(stored)
    }

    /// Every turn of `session_id` in turn-index order.
    pub fn turns(&self, session_id: &Id) -> Result<Vec<WorkflowTurn>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TURN_COLUMNS} FROM workflow_session_turns WHERE session_id = ?1 \
             ORDER BY turn_index"
        ))?;
        let rows = stmt
            .query_map([session_id.as_str()], raw_turn)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(RawTurn::decode).collect()
    }
}

// ============================================================================ row access

pub(crate) fn canonical_text<T: serde::Serialize + ?Sized>(
    value: &T,
) -> Result<String, StoreError> {
    let bytes = to_canonical_json(value)?;
    String::from_utf8(bytes).map_err(|_| StoreError::CorruptWorkflowRecord {
        kind: "json",
        id: String::new(),
        reason: "canonical JSON is not UTF-8".to_owned(),
    })
}

pub(crate) fn require_revision(tx: &Transaction<'_>, id: &RevisionId) -> Result<(), StoreError> {
    if revision_exists(tx, id)? {
        Ok(())
    } else {
        Err(StoreError::RevisionNotFound(id.clone()))
    }
}

pub(crate) fn expect_state(
    kind: &'static str,
    id: &Id,
    actual: &str,
    expected: &str,
) -> Result<(), StoreError> {
    if actual == expected {
        Ok(())
    } else {
        Err(StoreError::WorkflowStateConflict {
            kind,
            id: id.to_string(),
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
}

pub(crate) fn insert_proposal_on(
    tx: &Transaction<'_>,
    proposal: &NewWorkflowProposal,
    state: WorkflowProposalState,
) -> Result<(), StoreError> {
    if !matches!(
        state,
        WorkflowProposalState::Prepared | WorkflowProposalState::Pending
    ) {
        return Err(StoreError::WorkflowInvalidTransition {
            kind: "proposal",
            id: proposal.proposal_id.to_string(),
            to: state.as_str(),
        });
    }
    if proposal.origin_kind == WorkflowOrigin::Stage && proposal.origin_ref.is_some() {
        return Err(StoreError::WorkflowInvalidOrigin {
            id: proposal.proposal_id.to_string(),
            reason: "a stage proposal has no origin_ref",
        });
    }
    require_revision(tx, &proposal.base_revision_id)?;
    tx.execute(
        &format!(
            "INSERT INTO workflow_proposals ({PROPOSAL_COLUMNS}) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL)"
        ),
        params![
            proposal.proposal_id.as_str(),
            proposal.base_revision_id.as_str(),
            canonical_text(&proposal.proposal_json)?,
            canonical_text(&proposal.intake_json)?,
            proposal.proposal_kind.as_str(),
            state.as_str(),
            proposal.origin_kind.as_str(),
            proposal.origin_ref.as_ref().map(Id::as_str),
            proposal.created_at.to_string()
        ],
    )?;
    Ok(())
}

pub(crate) fn corrupt(kind: &'static str, id: &str, reason: impl Into<String>) -> StoreError {
    StoreError::CorruptWorkflowRecord {
        kind,
        id: id.to_owned(),
        reason: reason.into(),
    }
}

pub(crate) fn parse_field<T: FromStr>(
    kind: &'static str,
    id: &str,
    field: &str,
    text: &str,
) -> Result<T, StoreError> {
    text.parse()
        .map_err(|_| corrupt(kind, id, format!("invalid {field}")))
}

pub(crate) fn parse_optional<T: FromStr>(
    kind: &'static str,
    id: &str,
    field: &str,
    text: Option<String>,
) -> Result<Option<T>, StoreError> {
    text.map(|t| parse_field(kind, id, field, &t)).transpose()
}

struct RawProposal {
    proposal_id: String,
    base_revision_id: String,
    proposal_json: String,
    intake_json: String,
    proposal_kind: String,
    state: String,
    origin_kind: String,
    origin_ref: Option<String>,
    created_at: String,
    resolved_at: Option<String>,
    resolved_revision_id: Option<String>,
}

fn raw_proposal(row: &Row<'_>) -> rusqlite::Result<RawProposal> {
    Ok(RawProposal {
        proposal_id: row.get(0)?,
        base_revision_id: row.get(1)?,
        proposal_json: row.get(2)?,
        intake_json: row.get(3)?,
        proposal_kind: row.get(4)?,
        state: row.get(5)?,
        origin_kind: row.get(6)?,
        origin_ref: row.get(7)?,
        created_at: row.get(8)?,
        resolved_at: row.get(9)?,
        resolved_revision_id: row.get(10)?,
    })
}

impl RawProposal {
    fn decode(self) -> Result<WorkflowProposal, StoreError> {
        const K: &str = "proposal";
        let id = self.proposal_id.clone();
        Ok(WorkflowProposal {
            proposal_id: parse_field(K, &id, "proposal_id", &self.proposal_id)?,
            base_revision_id: parse_field(K, &id, "base_revision_id", &self.base_revision_id)?,
            proposal_json: self.proposal_json,
            intake_json: self.intake_json,
            proposal_kind: parse_field(K, &id, "proposal_kind", &self.proposal_kind)?,
            state: parse_field(K, &id, "state", &self.state)?,
            origin_kind: parse_field(K, &id, "origin_kind", &self.origin_kind)?,
            origin_ref: parse_optional(K, &id, "origin_ref", self.origin_ref)?,
            created_at: parse_field(K, &id, "created_at", &self.created_at)?,
            resolved_at: parse_optional(K, &id, "resolved_at", self.resolved_at)?,
            resolved_revision_id: parse_optional(
                K,
                &id,
                "resolved_revision_id",
                self.resolved_revision_id,
            )?,
        })
    }
}

pub(crate) fn proposal_on(
    conn: &Connection,
    id: &Id,
) -> Result<Option<WorkflowProposal>, StoreError> {
    let raw = conn
        .query_row(
            &format!("SELECT {PROPOSAL_COLUMNS} FROM workflow_proposals WHERE proposal_id = ?1"),
            [id.as_str()],
            raw_proposal,
        )
        .optional()?;
    raw.map(RawProposal::decode).transpose()
}

fn session_on(conn: &Connection, id: &Id) -> Result<Option<WorkflowSession>, StoreError> {
    let raw = conn
        .query_row(
            "SELECT session_id, session_json, owner_actor_ref, created_at FROM workflow_sessions \
             WHERE session_id = ?1",
            [id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    raw.map(|(session_id, session_json, owner, created_at)| {
        const K: &str = "session";
        Ok(WorkflowSession {
            session_id: parse_field(K, &session_id, "session_id", &session_id)?,
            session_json,
            owner_actor_ref: parse_field(K, &session_id, "owner_actor_ref", &owner)?,
            created_at: parse_field(K, &session_id, "created_at", &created_at)?,
        })
    })
    .transpose()
}

struct RawTurn {
    session_id: String,
    turn_index: i64,
    state: String,
    base_revision_id: String,
    stage: String,
    expected_node_refs_json: String,
    proposal_ids_json: String,
    requires_reason_count: i64,
    committed_revision_id: Option<String>,
    created_at: String,
    failure: Option<String>,
}

fn raw_turn(row: &Row<'_>) -> rusqlite::Result<RawTurn> {
    Ok(RawTurn {
        session_id: row.get(0)?,
        turn_index: row.get(1)?,
        state: row.get(2)?,
        base_revision_id: row.get(3)?,
        stage: row.get(4)?,
        expected_node_refs_json: row.get(5)?,
        proposal_ids_json: row.get(6)?,
        requires_reason_count: row.get(7)?,
        committed_revision_id: row.get(8)?,
        created_at: row.get(9)?,
        failure: row.get(10)?,
    })
}

/// Decodes a stored canonical JSON array of IDs, requiring canonical bytes.
fn id_list(id: &str, field: &str, text: &str) -> Result<Vec<Id>, StoreError> {
    let ids: Vec<Id> =
        serde_json::from_str(text).map_err(|_| corrupt("turn", id, format!("invalid {field}")))?;
    if canonical_text(&ids)? != text {
        return Err(corrupt("turn", id, format!("non-canonical {field}")));
    }
    Ok(ids)
}

impl RawTurn {
    fn decode(self) -> Result<WorkflowTurn, StoreError> {
        const K: &str = "turn";
        let id = format!("{}#{}", self.session_id, self.turn_index);
        Ok(WorkflowTurn {
            session_id: parse_field(K, &id, "session_id", &self.session_id)?,
            turn_index: u32::try_from(self.turn_index)
                .map_err(|_| corrupt(K, &id, "invalid turn_index"))?,
            state: parse_field(K, &id, "state", &self.state)?,
            base_revision_id: parse_field(K, &id, "base_revision_id", &self.base_revision_id)?,
            stage: self.stage,
            expected_node_refs: id_list(&id, "expected_node_refs", &self.expected_node_refs_json)?,
            proposal_ids: id_list(&id, "proposal_ids", &self.proposal_ids_json)?,
            requires_reason_count: u32::try_from(self.requires_reason_count)
                .map_err(|_| corrupt(K, &id, "invalid requires_reason_count"))?,
            committed_revision_id: parse_optional(
                K,
                &id,
                "committed_revision_id",
                self.committed_revision_id,
            )?,
            created_at: parse_field(K, &id, "created_at", &self.created_at)?,
            failure: self.failure,
        })
    }
}

fn turn_on(
    conn: &Connection,
    session_id: &Id,
    turn_index: u32,
) -> Result<Option<WorkflowTurn>, StoreError> {
    let raw = conn
        .query_row(
            &format!(
                "SELECT {TURN_COLUMNS} FROM workflow_session_turns \
                 WHERE session_id = ?1 AND turn_index = ?2"
            ),
            params![session_id.as_str(), i64::from(turn_index)],
            raw_turn,
        )
        .optional()?;
    raw.map(RawTurn::decode).transpose()
}

fn prepared_turn(
    tx: &Transaction<'_>,
    session_id: &Id,
    turn_index: u32,
) -> Result<WorkflowTurn, StoreError> {
    let turn =
        turn_on(tx, session_id, turn_index)?.ok_or_else(|| StoreError::WorkflowRecordNotFound {
            kind: "turn",
            id: format!("{session_id}#{turn_index}"),
        })?;
    if turn.state != WorkflowTurnState::Prepared {
        return Err(StoreError::WorkflowStateConflict {
            kind: "turn",
            id: format!("{session_id}#{turn_index}"),
            expected: WorkflowTurnState::Prepared.as_str().to_owned(),
            actual: turn.state.as_str().to_owned(),
        });
    }
    Ok(turn)
}
