//! A0.3 (Hotfix 065) contract tests for store schema v2 and the durable workflow records.
//!
//! Every test lives in `workflow` so that `cargo test -p plumb-store workflow` selects all of
//! them.

mod workflow {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use plumb_core::{to_canonical_json, Hash, Id, Timestamp};
    use plumb_patch::{PatchSet, SemanticPatch};
    use plumb_psg::*;
    use plumb_store::*;
    use rusqlite::Connection;
    use serde_json::{json, Value};
    use tempfile::TempDir;

    const T1: &str = "2026-09-29T08:00:00.000000000Z";
    const T2: &str = "2026-09-29T09:00:00.000000000Z";
    const PROJECT: &str = "project:leave-management";
    const PROFILE: &str = "profile:plumb-software-2026.1";

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn generic(c: &str) -> Hash {
        format!("sha256:{}", c.repeat(64)).parse().unwrap()
    }

    fn requirement(node_id: &str, status: &str) -> Node {
        serde_json::from_value(json!({
            "id": node_id, "revision": 1, "status": status,
            "payload": {"type": "Requirement", "data": {
                "statement": "Employees shall submit leave requests.",
                "requirement_kind": "functional", "level": "system", "modality": "shall",
                "title": null, "rationale": null, "priority": null, "source_identifier": null,
                "verification_method": null, "owner_refs": null, "stakeholder_refs": null
            }},
            "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
            "audit": {"created_by": "actor:analyst", "created_at": T1, "updated_by": null, "updated_at": null}
        }))
        .unwrap()
    }

    fn graph() -> Graph {
        Graph::new(
            id(PROJECT),
            id(PROFILE),
            vec![requirement("req:a", "Accepted")],
            vec![],
        )
        .unwrap()
    }

    fn meta(at: &str) -> RevisionWriteMeta {
        RevisionWriteMeta {
            decision_refs: vec![],
            created_by: id("actor:analyst"),
            created_at: ts(at),
        }
    }

    struct Db {
        _dir: TempDir,
        path: PathBuf,
    }

    impl Db {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("plumb.sqlite");
            Db { _dir: dir, path }
        }

        fn raw(&self) -> Connection {
            Connection::open(&self.path).unwrap()
        }

        fn exec(&self, sql: &str) {
            self.raw().execute_batch(sql).unwrap();
        }

        fn version(&self) -> Vec<i64> {
            let conn = self.raw();
            let mut stmt = conn.prepare("SELECT version FROM schema_meta").unwrap();
            let rows = stmt
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        }

        fn tables(&self) -> BTreeSet<String> {
            let conn = self.raw();
            let mut stmt = conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
                .unwrap();
            let names = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<Result<BTreeSet<_>, _>>()
                .unwrap();
            names
        }

        /// Every revision and artifact row, for the never-rewritten check.
        fn graph_rows(&self) -> Vec<String> {
            let conn = self.raw();
            let mut out = Vec::new();
            for sql in [
                "SELECT id || version || semantic_hash || decision_refs_json FROM graph_revisions ORDER BY id",
                "SELECT revision_id || node_id || node_json FROM revision_nodes ORDER BY revision_id, node_id",
                "SELECT name || revision_id || updated_at FROM branch_heads ORDER BY name",
                "SELECT hash || kind || media_type || hex(bytes) || created_at FROM artifacts ORDER BY hash",
            ] {
                let mut stmt = conn.prepare(sql).unwrap();
                out.extend(
                    stmt.query_map([], |r| r.get::<_, String>(0))
                        .unwrap()
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap(),
                );
            }
            out
        }

        /// Revision 1 and revision 2 (a Proposed requirement added).
        fn with_revisions(&self) -> (RevisionId, RevisionId) {
            let mut store = SqliteRevisionStore::open(&self.path).unwrap();
            let g = graph();
            let r1 = store
                .create_initial_revision(&g, generic("a"), generic("b"), meta(T1))
                .unwrap();
            let r2 = store
                .commit(
                    &BranchName::main(),
                    &r1.id,
                    &PatchSet {
                        base_semantic_hash: g.semantic_hash().unwrap(),
                        patch: SemanticPatch::AddNode {
                            node: requirement("req:b", "Proposed"),
                        },
                    },
                    meta(T2),
                )
                .unwrap()
                .revision;
            (r1.id, r2.id)
        }

        /// Turns a fresh v2 store with revisions into the equivalent v1 store.
        fn downgrade_to_v1(&self) {
            self.exec(
                "DROP TABLE workflow_session_turns; DROP TABLE workflow_sessions; \
                 DROP TABLE workflow_proposals; UPDATE schema_meta SET version = 1;",
            );
        }

        fn workflow(&self) -> SqliteWorkflowStore {
            SqliteWorkflowStore::open(&self.path).unwrap()
        }
    }

    fn new_proposal(
        proposal_id: &str,
        base: &RevisionId,
        origin: WorkflowOrigin,
    ) -> NewWorkflowProposal {
        NewWorkflowProposal {
            proposal_id: id(proposal_id),
            base_revision_id: base.clone(),
            // Opaque material: the store never interprets it, only canonicalizes it.
            proposal_json: json!({"z": 1, "id": proposal_id, "a": [3, 2, 1]}),
            intake_json: json!({"requires_reason": false, "reason_codes": []}),
            proposal_kind: WorkflowProposalKind::Create,
            origin_kind: origin,
            origin_ref: match origin {
                WorkflowOrigin::Intake => None,
                _ => Some(id("ses:0123456789abcdef")),
            },
            created_at: ts(T1),
        }
    }

    fn canonical(value: &Value) -> String {
        String::from_utf8(to_canonical_json(value).unwrap()).unwrap()
    }

    fn session(store: &mut SqliteWorkflowStore) -> WorkflowSession {
        match store
            .insert_session(&NewWorkflowSession {
                session_id: id("ses:0123456789abcdef"),
                session_json: json!({"scope_refs": ["req:a"], "id": "ses:0123456789abcdef"}),
                owner_actor_ref: id("actor:analyst"),
                created_at: ts(T1),
            })
            .unwrap()
        {
            WorkflowInsert::Inserted(s) => s,
            other => panic!("{other:?}"),
        }
    }

    // ------------------------------------------------------------------ schema v2

    #[test]
    fn workflow_fresh_database_is_created_at_v2() {
        let db = Db::new();
        drop(db.workflow());
        assert_eq!(db.version(), vec![STORE_SCHEMA_VERSION]);
        assert_eq!(STORE_SCHEMA_VERSION, 2);
        for t in [
            "workflow_proposals",
            "workflow_sessions",
            "workflow_session_turns",
        ] {
            assert!(db.tables().contains(t), "{t}");
        }
        // A v2 store is verified and reopened unchanged.
        let before = db.tables();
        drop(db.workflow());
        drop(SqliteRevisionStore::open(&db.path).unwrap());
        assert_eq!(db.tables(), before);
        assert_eq!(db.version(), vec![2]);
    }

    #[test]
    fn workflow_valid_v1_store_migrates_to_v2_without_rewriting_rows() {
        let db = Db::new();
        let (_, r2) = db.with_revisions();
        db.downgrade_to_v1();
        assert_eq!(db.version(), vec![1]);
        let rows = db.graph_rows();
        // Opening through either store migrates transactionally.
        let store = SqliteRevisionStore::open(&db.path).unwrap();
        assert_eq!(db.version(), vec![2]);
        assert!(db.tables().contains("workflow_session_turns"));
        assert_eq!(db.graph_rows(), rows);
        assert_eq!(store.head(&BranchName::main()).unwrap(), Some(r2.clone()));
        let loaded = store.load_revision(&r2).unwrap();
        assert_eq!(loaded.revision.id, r2);
        // The migrated store accepts workflow records.
        let mut wf = db.workflow();
        assert!(matches!(
            wf.insert_proposal(
                &new_proposal("prop:0000000000000001", &r2, WorkflowOrigin::Intake),
                WorkflowProposalState::Pending
            )
            .unwrap(),
            WorkflowInsert::Inserted(_)
        ));
    }

    #[test]
    fn workflow_failed_migration_rolls_back_completely() {
        let db = Db::new();
        db.with_revisions();
        db.downgrade_to_v1();
        // A conflicting object makes the third CREATE TABLE fail mid-migration.
        db.exec("CREATE TABLE workflow_session_turns (x INTEGER);");
        let before_tables = db.tables();
        let rows = db.graph_rows();
        assert!(SqliteWorkflowStore::open(&db.path).is_err());
        assert_eq!(db.version(), vec![1]);
        assert_eq!(db.tables(), before_tables);
        assert!(!db.tables().contains("workflow_proposals"));
        assert!(!db.tables().contains("workflow_sessions"));
        assert_eq!(db.graph_rows(), rows);
        // Removing the conflict lets the migration complete.
        db.exec("DROP TABLE workflow_session_turns;");
        drop(db.workflow());
        assert_eq!(db.version(), vec![2]);
    }

    #[test]
    fn workflow_unknown_versions_and_unversioned_tables_are_rejected() {
        let db = Db::new();
        drop(db.workflow());
        db.exec("UPDATE schema_meta SET version = 3;");
        assert!(matches!(
            SqliteWorkflowStore::open(&db.path).unwrap_err(),
            StoreError::UnsupportedStoreSchemaVersion { found: 3 }
        ));
        assert_eq!(db.version(), vec![3]);

        let db = Db::new();
        db.exec("CREATE TABLE workflow_proposals (proposal_id TEXT PRIMARY KEY);");
        assert!(matches!(
            SqliteWorkflowStore::open(&db.path).unwrap_err(),
            StoreError::UnversionedStore
        ));
        assert!(!db.tables().contains("schema_meta"));

        // A v2 store missing a workflow table is corrupt and never healed.
        let db = Db::new();
        drop(db.workflow());
        db.exec("DROP TABLE workflow_sessions;");
        assert!(matches!(
            SqliteWorkflowStore::open(&db.path).unwrap_err(),
            StoreError::CorruptStoreSchema { reason } if reason.contains("workflow_sessions")
        ));
        assert!(!db.tables().contains("workflow_sessions"));
    }

    // ------------------------------------------------------------------ proposals

    #[test]
    fn workflow_proposals_are_canonical_durable_and_unique() {
        let db = Db::new();
        let (r1, r2) = db.with_revisions();
        let mut wf = db.workflow();
        let p = new_proposal("prop:0000000000000001", &r1, WorkflowOrigin::Intake);
        let stored = match wf
            .insert_proposal(&p, WorkflowProposalState::Pending)
            .unwrap()
        {
            WorkflowInsert::Inserted(stored) => stored,
            other => panic!("{other:?}"),
        };
        assert_eq!(stored.proposal_json, canonical(&p.proposal_json));
        assert_eq!(
            stored.proposal_json,
            r#"{"a":[3,2,1],"id":"prop:0000000000000001","z":1}"#
        );
        assert_eq!(stored.intake_json, canonical(&p.intake_json));
        assert_eq!(stored.state, WorkflowProposalState::Pending);
        assert_eq!(stored.base_revision_id, r1);
        assert_eq!(stored.origin_ref, None);
        assert_eq!(stored.resolved_at, None);
        // A second insert of the same ID returns the stored record unchanged.
        let mut other = p.clone();
        other.proposal_json = json!({"different": true});
        assert_eq!(
            wf.insert_proposal(&other, WorkflowProposalState::Pending)
                .unwrap(),
            WorkflowInsert::Existing(stored.clone())
        );
        // Durable across reopening.
        drop(wf);
        let wf = db.workflow();
        assert_eq!(wf.proposal(&p.proposal_id).unwrap(), Some(stored));
        assert_eq!(wf.proposal(&id("prop:ffffffffffffffff")).unwrap(), None);
        // Only prepared or pending are initial states, and the base revision must exist.
        let mut wf = db.workflow();
        assert!(matches!(
            wf.insert_proposal(
                &new_proposal("prop:0000000000000002", &r2, WorkflowOrigin::Intake),
                WorkflowProposalState::Accepted
            ),
            Err(StoreError::WorkflowInvalidTransition { .. })
        ));
        let ghost: RevisionId = "rev:9:0123456789abcdef".parse().unwrap();
        assert!(matches!(
            wf.insert_proposal(
                &new_proposal("prop:0000000000000002", &ghost, WorkflowOrigin::Intake),
                WorkflowProposalState::Pending
            ),
            Err(StoreError::RevisionNotFound(_))
        ));
        assert_eq!(wf.proposal(&id("prop:0000000000000002")).unwrap(), None);
    }

    #[test]
    fn workflow_proposal_transitions_are_exact() {
        let db = Db::new();
        let (r1, r2) = db.with_revisions();
        let mut wf = db.workflow();
        for name in ["prop:0000000000000001", "prop:0000000000000002"] {
            wf.insert_proposal(
                &new_proposal(name, &r1, WorkflowOrigin::Question),
                WorkflowProposalState::Pending,
            )
            .unwrap();
        }
        let accepted = wf
            .resolve_proposal(
                &id("prop:0000000000000001"),
                WorkflowProposalState::Accepted,
                &r2,
                ts(T2),
            )
            .unwrap();
        assert_eq!(accepted.state, WorkflowProposalState::Accepted);
        assert_eq!(accepted.resolved_revision_id, Some(r2.clone()));
        assert_eq!(accepted.resolved_at, Some(ts(T2)));
        assert_eq!(accepted.base_revision_id, r1);
        let rejected = wf
            .resolve_proposal(
                &id("prop:0000000000000002"),
                WorkflowProposalState::Rejected,
                &r1,
                ts(T2),
            )
            .unwrap();
        assert_eq!(rejected.state, WorkflowProposalState::Rejected);
        // Resolved proposals are never reopened or resolved twice.
        for (target, to) in [
            ("prop:0000000000000001", WorkflowProposalState::Rejected),
            ("prop:0000000000000002", WorkflowProposalState::Accepted),
        ] {
            assert!(matches!(
                wf.resolve_proposal(&id(target), to, &r2, ts(T2)),
                Err(StoreError::WorkflowStateConflict { .. })
            ));
        }
        assert!(matches!(
            wf.resolve_proposal(
                &id("prop:0000000000000001"),
                WorkflowProposalState::Pending,
                &r2,
                ts(T2)
            ),
            Err(StoreError::WorkflowInvalidTransition { .. })
        ));
        assert!(matches!(
            wf.resolve_proposal(
                &id("prop:ffffffffffffffff"),
                WorkflowProposalState::Accepted,
                &r2,
                ts(T2)
            ),
            Err(StoreError::WorkflowRecordNotFound { .. })
        ));
    }

    // ------------------------------------------------------------------ sessions and turns

    #[test]
    fn workflow_sessions_are_canonical_and_unique() {
        let db = Db::new();
        db.with_revisions();
        let mut wf = db.workflow();
        let stored = session(&mut wf);
        assert_eq!(
            stored.session_json,
            r#"{"id":"ses:0123456789abcdef","scope_refs":["req:a"]}"#
        );
        assert_eq!(stored.owner_actor_ref, id("actor:analyst"));
        let again = wf
            .insert_session(&NewWorkflowSession {
                session_id: id("ses:0123456789abcdef"),
                session_json: json!({}),
                owner_actor_ref: id("actor:other"),
                created_at: ts(T2),
            })
            .unwrap();
        assert_eq!(again, WorkflowInsert::Existing(stored.clone()));
        drop(wf);
        assert_eq!(
            db.workflow().session(&id("ses:0123456789abcdef")).unwrap(),
            Some(stored)
        );
    }

    fn new_turn(r: &RevisionId, proposals: &[&NewWorkflowProposal]) -> NewWorkflowTurn {
        NewWorkflowTurn {
            session_id: id("ses:0123456789abcdef"),
            base_revision_id: r.clone(),
            stage: "S1".into(),
            expected_node_refs: vec![id("evd:0000000000000001"), id("src:0000000000000001")],
            proposal_ids: proposals.iter().map(|p| p.proposal_id.clone()).collect(),
            requires_reason_count: 1,
            created_at: ts(T1),
        }
    }

    #[test]
    fn workflow_turn_prepare_commit_and_fail_are_atomic() {
        let db = Db::new();
        let (r1, r2) = db.with_revisions();
        let mut wf = db.workflow();
        session(&mut wf);
        let p1 = new_proposal("prop:0000000000000001", &r1, WorkflowOrigin::Session);
        let p2 = new_proposal("prop:0000000000000002", &r1, WorkflowOrigin::Session);
        let turn = wf
            .prepare_turn(&new_turn(&r1, &[&p1, &p2]), &[p1.clone(), p2.clone()])
            .unwrap();
        assert_eq!(turn.turn_index, 0);
        assert_eq!(turn.state, WorkflowTurnState::Prepared);
        assert_eq!(turn.committed_revision_id, None);
        assert_eq!(turn.requires_reason_count, 1);
        assert_eq!(turn.stage, "S1");
        for p in [&p1, &p2] {
            let stored = wf.proposal(&p.proposal_id).unwrap().unwrap();
            assert_eq!(stored.state, WorkflowProposalState::Prepared);
            assert_eq!(stored.origin_kind, WorkflowOrigin::Session);
        }
        // Prepared proposals cannot be resolved.
        assert!(matches!(
            wf.resolve_proposal(
                &p1.proposal_id,
                WorkflowProposalState::Accepted,
                &r2,
                ts(T2)
            ),
            Err(StoreError::WorkflowStateConflict { .. })
        ));
        let committed = wf.commit_turn(&id("ses:0123456789abcdef"), 0, &r2).unwrap();
        assert_eq!(committed.state, WorkflowTurnState::Committed);
        assert_eq!(committed.committed_revision_id, Some(r2.clone()));
        for p in [&p1, &p2] {
            let stored = wf.proposal(&p.proposal_id).unwrap().unwrap();
            assert_eq!(stored.state, WorkflowProposalState::Pending);
            assert_eq!(stored.base_revision_id, r2);
        }
        // A committed turn is never committed or failed again.
        assert!(matches!(
            wf.commit_turn(&id("ses:0123456789abcdef"), 0, &r2),
            Err(StoreError::WorkflowStateConflict { .. })
        ));
        assert!(matches!(
            wf.fail_turn(&id("ses:0123456789abcdef"), 0, "E_X"),
            Err(StoreError::WorkflowStateConflict { .. })
        ));

        // The next turn takes the next index and can fail with its proposals.
        let p3 = new_proposal("prop:0000000000000003", &r2, WorkflowOrigin::Session);
        let turn = wf
            .prepare_turn(&new_turn(&r2, &[&p3]), std::slice::from_ref(&p3))
            .unwrap();
        assert_eq!(turn.turn_index, 1);
        let failed = wf
            .fail_turn(&id("ses:0123456789abcdef"), 1, "STALE_BASE")
            .unwrap();
        assert_eq!(failed.state, WorkflowTurnState::Failed);
        assert_eq!(failed.failure.as_deref(), Some("STALE_BASE"));
        assert_eq!(
            wf.proposal(&p3.proposal_id).unwrap().unwrap().state,
            WorkflowProposalState::Failed
        );
        drop(wf);
        let turns = db.workflow().turns(&id("ses:0123456789abcdef")).unwrap();
        assert_eq!(
            turns.iter().map(|t| t.state).collect::<Vec<_>>(),
            [WorkflowTurnState::Committed, WorkflowTurnState::Failed]
        );
        assert_eq!(turns[0].proposal_ids, [p1.proposal_id, p2.proposal_id]);
        assert_eq!(
            turns[0].expected_node_refs,
            [id("evd:0000000000000001"), id("src:0000000000000001")]
        );
    }

    #[test]
    fn workflow_turn_preparation_is_all_or_nothing() {
        let db = Db::new();
        let (r1, _) = db.with_revisions();
        let mut wf = db.workflow();
        // Unknown session.
        let p1 = new_proposal("prop:0000000000000001", &r1, WorkflowOrigin::Session);
        assert!(matches!(
            wf.prepare_turn(&new_turn(&r1, &[&p1]), std::slice::from_ref(&p1)),
            Err(StoreError::WorkflowRecordNotFound {
                kind: "session",
                ..
            })
        ));
        session(&mut wf);
        // A proposal ID that already exists aborts the whole turn.
        wf.insert_proposal(
            &new_proposal("prop:0000000000000002", &r1, WorkflowOrigin::Intake),
            WorkflowProposalState::Pending,
        )
        .unwrap();
        let p2 = new_proposal("prop:0000000000000002", &r1, WorkflowOrigin::Session);
        assert!(matches!(
            wf.prepare_turn(&new_turn(&r1, &[&p1, &p2]), &[p1.clone(), p2.clone()]),
            Err(StoreError::WorkflowRecordExists { .. })
        ));
        assert_eq!(wf.proposal(&p1.proposal_id).unwrap(), None);
        assert!(wf.turns(&id("ses:0123456789abcdef")).unwrap().is_empty());
        // The turn's proposal list must name exactly its proposals.
        assert!(matches!(
            wf.prepare_turn(&new_turn(&r1, &[]), std::slice::from_ref(&p1)),
            Err(StoreError::CorruptWorkflowRecord { .. })
        ));
    }

    #[test]
    fn workflow_rows_do_not_change_revisions() {
        let db = Db::new();
        let (r1, r2) = db.with_revisions();
        let rows = db.graph_rows();
        let mut wf = db.workflow();
        session(&mut wf);
        wf.insert_proposal(
            &new_proposal("prop:0000000000000001", &r1, WorkflowOrigin::Intake),
            WorkflowProposalState::Pending,
        )
        .unwrap();
        wf.resolve_proposal(
            &id("prop:0000000000000001"),
            WorkflowProposalState::Accepted,
            &r2,
            ts(T2),
        )
        .unwrap();
        assert_eq!(db.graph_rows(), rows);
    }

    #[test]
    fn workflow_corrupt_rows_are_reported_never_repaired() {
        let db = Db::new();
        let (r1, _) = db.with_revisions();
        let mut wf = db.workflow();
        session(&mut wf);
        let p = new_proposal("prop:0000000000000001", &r1, WorkflowOrigin::Session);
        wf.prepare_turn(&new_turn(&r1, &[&p]), std::slice::from_ref(&p))
            .unwrap();
        db.exec(
            "UPDATE workflow_session_turns SET proposal_ids_json = '[ \"prop:0000000000000001\"]';",
        );
        assert!(matches!(
            wf.turns(&id("ses:0123456789abcdef")),
            Err(StoreError::CorruptWorkflowRecord { .. })
        ));
        db.exec("UPDATE workflow_proposals SET created_at = 'yesterday';");
        assert!(matches!(
            wf.proposal(&id("prop:0000000000000001")),
            Err(StoreError::CorruptWorkflowRecord { .. })
        ));
    }
}
