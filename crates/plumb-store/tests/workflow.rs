//! A0.3 (Hotfix 065) contract tests for the durable workflow records and their store schema
//! (v2, migrated to v3 by A0.4, Hotfix 067, and to the v4 proposal occurrences by H0.10).
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

        /// Runs `sql` and returns its error, if any.
        fn exec_err(&self, sql: &str) -> Option<rusqlite::Error> {
            self.raw().execute_batch(sql).err()
        }

        /// Every row of `sql` as one `|`-joined text per row.
        fn rows(&self, sql: &str) -> Vec<String> {
            let conn = self.raw();
            let mut stmt = conn.prepare(sql).unwrap();
            let n = stmt.column_count();
            let rows = stmt
                .query_map([], |r| {
                    (0..n)
                        .map(|i| {
                            r.get::<_, Option<String>>(i)
                                .map(|v| v.unwrap_or_else(|| "NULL".to_owned()))
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map(|v| v.join("|"))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        }

        /// The full schema SQL, for unchanged-schema checks.
        fn schema(&self) -> Vec<String> {
            self.rows("SELECT type || name || COALESCE(sql, '') FROM sqlite_master ORDER BY name")
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

        /// Drops the store schema v3 stage tables.
        fn drop_stage_tables(&self) {
            self.exec(
                "DROP TABLE stage_recipe_job_attempts; DROP TABLE stage_job_proposals; \
                 DROP TABLE stage_recipe_job_steps; \
                 DROP TABLE gate_reports; DROP TABLE stage_jobs; DROP TABLE stage_recipe_jobs;",
            );
        }

        /// Turns a fresh v3 store with revisions into the equivalent v1 store.
        fn downgrade_to_v1(&self) {
            self.drop_stage_tables();
            self.exec(
                "DROP TABLE workflow_session_turns; DROP TABLE workflow_sessions; \
                 DROP TABLE workflow_proposals; UPDATE schema_meta SET version = 1;",
            );
        }

        /// Rebuilds the exact store schema v2: no stage tables and the v2 origin CHECK.
        fn downgrade_to_v2(&self) {
            self.drop_stage_tables();
            self.exec(&format!(
                "CREATE TABLE wp_v2 AS SELECT {V3_COLUMNS} FROM workflow_proposals; \
                 DROP TABLE workflow_proposals; {V2_WORKFLOW_PROPOSALS} \
                 INSERT INTO workflow_proposals SELECT * FROM wp_v2; DROP TABLE wp_v2; \
                 UPDATE schema_meta SET version = 2;"
            ));
        }

        /// Rebuilds the exact store schema v3 proposal table (one row per proposal ID, no
        /// occurrence key); the stage links are rebuilt in their v3 form too.
        fn downgrade_to_v3(&self) {
            self.exec(&format!(
                "{V6_ADDITIONS} CREATE TABLE sl_v3 AS SELECT proposal_id, recipe_job_id, stage_job_id, \
                   compile_run_ref FROM stage_job_proposals; DROP TABLE stage_job_proposals; \
                 CREATE TABLE wp_v3 AS SELECT {V3_COLUMNS} FROM workflow_proposals; \
                 DROP TABLE workflow_proposals; {V3_WORKFLOW_PROPOSALS} \
                 INSERT INTO workflow_proposals SELECT * FROM wp_v3; DROP TABLE wp_v3; \
                 {V3_STAGE_JOB_PROPOSALS} \
                 INSERT INTO stage_job_proposals SELECT * FROM sl_v3; DROP TABLE sl_v3; \
                 UPDATE schema_meta SET version = 3;"
            ));
        }

        /// Rebuilds the exact store schema v4 proposal table (H0.10: occurrence keys, no
        /// `abandoned` state); the stage links keep referencing it by name.
        fn downgrade_to_v4(&self) {
            self.exec(&format!(
                "{V6_ADDITIONS} \
                 CREATE TABLE wp_v4 AS SELECT * FROM workflow_proposals; \
                 DROP TABLE workflow_proposals; {V4_WORKFLOW_PROPOSALS} \
                 INSERT INTO workflow_proposals SELECT * FROM wp_v4; DROP TABLE wp_v4; \
                 UPDATE schema_meta SET version = 4;"
            ));
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

    /// The exact store schema v2 definition of `workflow_proposals` (Hotfix 065).
    const V2_WORKFLOW_PROPOSALS: &str = "CREATE TABLE workflow_proposals (
  proposal_id TEXT PRIMARY KEY,
  base_revision_id TEXT NOT NULL,
  proposal_json TEXT NOT NULL,
  intake_json TEXT NOT NULL,
  proposal_kind TEXT NOT NULL
    CHECK (proposal_kind IN ('link', 'merge', 'supersede', 'create', 'accept')),
  state TEXT NOT NULL
    CHECK (state IN ('prepared', 'pending', 'accepted', 'rejected', 'failed')),
  origin_kind TEXT NOT NULL CHECK (origin_kind IN ('intake', 'question', 'session')),
  origin_ref TEXT NULL,
  created_at TEXT NOT NULL,
  resolved_at TEXT NULL,
  resolved_revision_id TEXT NULL,

  FOREIGN KEY (base_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (resolved_revision_id) REFERENCES graph_revisions(id)
);";

    /// Removes what store schema v6 (H0.3) added, giving the v4/v5 stage tables.
    const V6_ADDITIONS: &str = "DROP TABLE stage_recipe_job_attempts; \
        ALTER TABLE stage_recipe_jobs DROP COLUMN lease_expires_at; \
        ALTER TABLE stage_recipe_jobs DROP COLUMN owner_id; \
        ALTER TABLE stage_recipe_jobs DROP COLUMN attempt;";

    /// The columns of a store schema v2/v3 workflow proposal row.
    const V3_COLUMNS: &str = "proposal_id, base_revision_id, proposal_json, intake_json, \
        proposal_kind, state, origin_kind, origin_ref, created_at, resolved_at, \
        resolved_revision_id";

    /// The exact store schema v3 definition of `workflow_proposals` (Hotfix 067).
    const V3_WORKFLOW_PROPOSALS: &str = "CREATE TABLE workflow_proposals (
  proposal_id TEXT PRIMARY KEY,
  base_revision_id TEXT NOT NULL,
  proposal_json TEXT NOT NULL,
  intake_json TEXT NOT NULL,
  proposal_kind TEXT NOT NULL
    CHECK (proposal_kind IN ('link', 'merge', 'supersede', 'create', 'accept')),
  state TEXT NOT NULL
    CHECK (state IN ('prepared', 'pending', 'accepted', 'rejected', 'failed')),
  origin_kind TEXT NOT NULL CHECK (origin_kind IN ('intake', 'question', 'session', 'stage')),
  origin_ref TEXT NULL,
  created_at TEXT NOT NULL,
  resolved_at TEXT NULL,
  resolved_revision_id TEXT NULL,

  CHECK (origin_kind <> 'stage' OR origin_ref IS NULL),
  FOREIGN KEY (base_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (resolved_revision_id) REFERENCES graph_revisions(id)
);";

    /// The exact store schema v3 definition of `stage_job_proposals` (Hotfix 067).
    const V3_STAGE_JOB_PROPOSALS: &str = "CREATE TABLE stage_job_proposals (
  proposal_id TEXT PRIMARY KEY,
  recipe_job_id TEXT NOT NULL,
  stage_job_id TEXT NOT NULL,
  compile_run_ref TEXT NOT NULL,
  FOREIGN KEY (proposal_id) REFERENCES workflow_proposals(proposal_id),
  FOREIGN KEY (recipe_job_id) REFERENCES stage_recipe_jobs(recipe_job_id),
  FOREIGN KEY (stage_job_id) REFERENCES stage_jobs(stage_job_id),
  FOREIGN KEY (compile_run_ref) REFERENCES artifacts(hash)
);";

    /// The exact store schema v4 definition of `workflow_proposals` (H0.10).
    const V4_WORKFLOW_PROPOSALS: &str = "CREATE TABLE workflow_proposals (
  proposal_id TEXT NOT NULL,
  base_revision_id TEXT NOT NULL,
  proposal_json TEXT NOT NULL,
  intake_json TEXT NOT NULL,
  proposal_kind TEXT NOT NULL
    CHECK (proposal_kind IN ('link', 'merge', 'supersede', 'create', 'accept')),
  state TEXT NOT NULL
    CHECK (state IN ('prepared', 'pending', 'accepted', 'rejected', 'failed')),
  origin_kind TEXT NOT NULL CHECK (origin_kind IN ('intake', 'question', 'session', 'stage')),
  origin_ref TEXT NULL,
  created_at TEXT NOT NULL,
  resolved_at TEXT NULL,
  resolved_revision_id TEXT NULL,
  occurrence_id TEXT NOT NULL UNIQUE,

  PRIMARY KEY (proposal_id, base_revision_id),
  CHECK (origin_kind <> 'stage' OR origin_ref IS NULL),
  FOREIGN KEY (base_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (resolved_revision_id) REFERENCES graph_revisions(id)
);";

    const STAGE_TABLES: [&str; 5] = [
        "stage_recipe_jobs",
        "stage_jobs",
        "stage_recipe_job_steps",
        "stage_job_proposals",
        "gate_reports",
    ];

    // ------------------------------------------------------------------ schema v4

    #[test]
    fn workflow_fresh_database_is_created_at_v8() {
        let db = Db::new();
        drop(db.workflow());
        assert_eq!(db.version(), vec![STORE_SCHEMA_VERSION]);
        assert_eq!(STORE_SCHEMA_VERSION, 8);
        for t in [
            "workflow_proposals",
            "workflow_sessions",
            "workflow_session_turns",
        ]
        .into_iter()
        .chain(STAGE_TABLES)
        {
            assert!(db.tables().contains(t), "{t}");
        }
        // A v5 store is verified and reopened unchanged.
        let before = db.schema();
        drop(db.workflow());
        drop(SqliteRevisionStore::open(&db.path).unwrap());
        assert_eq!(db.schema(), before);
        assert_eq!(db.version(), vec![STORE_SCHEMA_VERSION]);
    }

    #[test]
    fn workflow_valid_v2_store_migrates_to_v4_preserving_proposals() {
        let db = Db::new();
        let (r1, r2) = db.with_revisions();
        {
            let mut wf = db.workflow();
            for (id, origin, state) in [
                (
                    "prop:0000000000000001",
                    WorkflowOrigin::Intake,
                    WorkflowProposalState::Pending,
                ),
                (
                    "prop:0000000000000002",
                    WorkflowOrigin::Question,
                    WorkflowProposalState::Prepared,
                ),
            ] {
                wf.insert_proposal(&new_proposal(id, &r1, origin), state)
                    .unwrap();
            }
            wf.resolve_proposal(
                &id("prop:0000000000000001"),
                &r1,
                WorkflowProposalState::Accepted,
                &r2,
                ts(T1),
            )
            .unwrap();
        }
        let v3_rows = format!("SELECT {V3_COLUMNS} FROM workflow_proposals ORDER BY proposal_id");
        let proposals_before = db.rows(&v3_rows);
        db.downgrade_to_v2();
        assert_eq!(db.version(), vec![2]);
        for t in STAGE_TABLES {
            assert!(!db.tables().contains(t), "{t}");
        }
        assert!(db
            .exec_err("INSERT INTO workflow_proposals SELECT 'prop:0000000000000009', base_revision_id, \
                 proposal_json, intake_json, proposal_kind, state, 'stage', NULL, created_at, NULL, \
                 NULL FROM workflow_proposals LIMIT 1;")
            .is_some());
        let rows = db.graph_rows();
        drop(SqliteRevisionStore::open(&db.path).unwrap());
        assert_eq!(db.version(), vec![STORE_SCHEMA_VERSION]);
        assert_eq!(db.graph_rows(), rows);
        assert_eq!(db.rows(&v3_rows), proposals_before);
        // Every migrated row is its own occurrence.
        let wf = db.workflow();
        for (proposal, base) in [
            ("prop:0000000000000001", &r1),
            ("prop:0000000000000002", &r1),
        ] {
            let stored = wf.proposal(&id(proposal), base).unwrap().unwrap();
            assert_eq!(
                stored.occurrence_id,
                occurrence_id(&id(proposal), base).unwrap()
            );
        }
        drop(wf);
        for t in STAGE_TABLES {
            assert!(db.tables().contains(t), "{t}");
        }
        // The rebuilt table admits the stage origin, only without a ref.
        let mut wf = db.workflow();
        let mut stage = new_proposal("prop:0000000000000003", &r1, WorkflowOrigin::Stage);
        stage.origin_ref = Some(id("job:x"));
        assert!(matches!(
            wf.insert_proposal(&stage, WorkflowProposalState::Pending),
            Err(StoreError::WorkflowInvalidOrigin { .. })
        ));
        assert!(db
            .exec_err("INSERT INTO workflow_proposals SELECT 'prop:0000000000000009', base_revision_id, \
                 proposal_json, intake_json, proposal_kind, state, 'stage', 'job:x', created_at, NULL, \
                 NULL, 'occ:0000000000000009' FROM workflow_proposals LIMIT 1;")
            .is_some());
        stage.origin_ref = None;
        assert!(matches!(
            wf.insert_proposal(&stage, WorkflowProposalState::Pending).unwrap(),
            WorkflowInsert::Inserted(p) if p.origin_kind == WorkflowOrigin::Stage && p.origin_ref.is_none()
        ));
    }

    #[test]
    fn workflow_failed_v2_migration_rolls_back_completely() {
        let db = Db::new();
        db.with_revisions();
        db.downgrade_to_v2();
        // A conflicting object makes the stage-table creation fail after the rebuild.
        db.exec("CREATE TABLE gate_reports (x INTEGER);");
        let before = db.schema();
        let rows = db.graph_rows();
        assert!(SqliteWorkflowStore::open(&db.path).is_err());
        assert_eq!(db.version(), vec![2]);
        assert_eq!(db.schema(), before);
        assert_eq!(db.graph_rows(), rows);
        db.exec("DROP TABLE gate_reports;");
        drop(db.workflow());
        assert_eq!(db.version(), vec![STORE_SCHEMA_VERSION]);
    }

    #[test]
    fn workflow_valid_v3_store_migrates_to_v4_keeping_every_row_as_its_occurrence() {
        let db = Db::new();
        let (r1, r2) = db.with_revisions();
        {
            let mut wf = db.workflow();
            session(&mut wf);
            for (proposal, base, state) in [
                ("prop:0000000000000001", &r1, WorkflowProposalState::Pending),
                ("prop:0000000000000002", &r2, WorkflowProposalState::Pending),
                (
                    "prop:0000000000000003",
                    &r1,
                    WorkflowProposalState::Prepared,
                ),
            ] {
                wf.insert_proposal(
                    &new_proposal(proposal, base, WorkflowOrigin::Session),
                    state,
                )
                .unwrap();
            }
            wf.resolve_proposal(
                &id("prop:0000000000000001"),
                &r1,
                WorkflowProposalState::Rejected,
                &r1,
                ts(T2),
            )
            .unwrap();
        }
        let v3_rows = format!("SELECT {V3_COLUMNS} FROM workflow_proposals ORDER BY proposal_id");
        let before = db.rows(&v3_rows);
        db.downgrade_to_v3();
        assert_eq!(db.version(), vec![3]);
        let graph = db.graph_rows();
        let wf = db.workflow();
        assert_eq!(db.version(), vec![STORE_SCHEMA_VERSION]);
        assert_eq!(db.rows(&v3_rows), before);
        assert_eq!(db.graph_rows(), graph);
        for (proposal, base) in [
            ("prop:0000000000000001", &r1),
            ("prop:0000000000000002", &r2),
            ("prop:0000000000000003", &r1),
        ] {
            let stored = wf.proposal(&id(proposal), base).unwrap().unwrap();
            assert_eq!(
                stored.occurrence_id,
                occurrence_id(&id(proposal), base).unwrap()
            );
        }
        // The migrated schema is exactly the fresh v4 schema (SQLite quotes renamed tables).
        let fresh = Db::new();
        drop(fresh.workflow());
        let unquoted = |rows: Vec<String>| -> Vec<String> {
            rows.into_iter().map(|r| r.replace('"', "")).collect()
        };
        assert_eq!(unquoted(db.schema()), unquoted(fresh.schema()));
    }

    #[test]
    fn workflow_valid_v1_store_migrates_to_v4_without_rewriting_rows() {
        let db = Db::new();
        let (_, r2) = db.with_revisions();
        db.downgrade_to_v1();
        assert_eq!(db.version(), vec![1]);
        let rows = db.graph_rows();
        // Opening through either store migrates transactionally.
        let store = SqliteRevisionStore::open(&db.path).unwrap();
        assert_eq!(db.version(), vec![STORE_SCHEMA_VERSION]);
        assert!(db.tables().contains("workflow_session_turns"));
        for t in STAGE_TABLES {
            assert!(db.tables().contains(t), "{t}");
        }
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

    /// H0.11 (architecture §33.12): store schema v5 (and so v6) admits the terminal `abandoned` proposal
    /// state. A valid v4 store is migrated in place: every row, revision and stage link is kept
    /// unchanged and the rebuilt schema is exactly the fresh one.
    #[test]
    fn workflow_valid_v4_store_migrates_to_v6_admitting_abandoned() {
        let db = Db::new();
        let (r1, r2) = db.with_revisions();
        {
            let mut wf = db.workflow();
            for (proposal, base) in [
                ("prop:0000000000000001", &r1),
                ("prop:0000000000000002", &r1),
                ("prop:0000000000000001", &r2),
            ] {
                wf.insert_proposal(
                    &new_proposal(proposal, base, WorkflowOrigin::Intake),
                    WorkflowProposalState::Pending,
                )
                .unwrap();
            }
            wf.resolve_proposal(
                &id("prop:0000000000000002"),
                &r1,
                WorkflowProposalState::Rejected,
                &r2,
                ts(T2),
            )
            .unwrap();
        }
        let all = "SELECT * FROM workflow_proposals ORDER BY proposal_id, base_revision_id";
        let before = db.rows(all);
        db.downgrade_to_v4();
        assert_eq!(db.version(), vec![4]);
        // The v4 table refuses the new state.
        assert!(db
            .exec_err(
                "UPDATE workflow_proposals SET state = 'abandoned' \
                 WHERE proposal_id = 'prop:0000000000000001'"
            )
            .is_some());
        let graph = db.graph_rows();
        let mut wf = db.workflow();
        assert_eq!(db.version(), vec![STORE_SCHEMA_VERSION]);
        assert_eq!(db.rows(all), before);
        assert_eq!(db.graph_rows(), graph);
        let fresh = Db::new();
        drop(fresh.workflow());
        let unquoted = |rows: Vec<String>| -> Vec<String> {
            rows.into_iter().map(|r| r.replace('"', "")).collect()
        };
        assert_eq!(unquoted(db.schema()), unquoted(fresh.schema()));
        // The stage links still reference the rebuilt proposal table.
        let parents = db.rows(
            "SELECT \"table\" FROM pragma_foreign_key_list('stage_job_proposals') \
             WHERE \"from\" = 'proposal_id'",
        );
        assert_eq!(parents, ["workflow_proposals"]);
        let abandoned = wf
            .resolve_proposal(
                &id("prop:0000000000000001"),
                &r1,
                WorkflowProposalState::Abandoned,
                &r2,
                ts(T2),
            )
            .unwrap();
        assert_eq!(abandoned.state, WorkflowProposalState::Abandoned);
    }

    #[test]
    fn workflow_failed_v4_migration_rolls_back_completely() {
        let db = Db::new();
        db.with_revisions();
        db.downgrade_to_v4();
        // A conflicting object makes the rebuild fail.
        db.exec("CREATE TABLE workflow_proposals_v5 (x INTEGER);");
        let before = db.schema();
        let rows = db.graph_rows();
        assert!(SqliteWorkflowStore::open(&db.path).is_err());
        assert_eq!(db.version(), vec![4]);
        assert_eq!(db.schema(), before);
        assert_eq!(db.graph_rows(), rows);
        db.exec("DROP TABLE workflow_proposals_v5;");
        drop(db.workflow());
        assert_eq!(db.version(), vec![STORE_SCHEMA_VERSION]);
    }

    /// H0.11: a discard moves exactly the pending occurrence to the terminal state `abandoned`,
    /// recorded on the given revision (no descent requirement: nothing is committed). The record
    /// is kept, is no longer pending, is not rejection memory and is never resolved again.
    #[test]
    fn workflow_pending_occurrence_is_abandoned_and_kept() {
        let db = Db::new();
        let (r1, r2) = db.with_revisions();
        let mut wf = db.workflow();
        for base in [&r1, &r2] {
            wf.insert_proposal(
                &new_proposal("prop:0000000000000001", base, WorkflowOrigin::Question),
                WorkflowProposalState::Pending,
            )
            .unwrap();
        }
        // Recorded on the base revision itself: an abandonment needs no newer revision.
        let abandoned = wf
            .resolve_proposal(
                &id("prop:0000000000000001"),
                &r2,
                WorkflowProposalState::Abandoned,
                &r2,
                ts(T2),
            )
            .unwrap();
        assert_eq!(abandoned.state, WorkflowProposalState::Abandoned);
        assert_eq!(abandoned.state.as_str(), "abandoned");
        assert_eq!(abandoned.resolved_revision_id, Some(r2.clone()));
        assert_eq!(abandoned.resolved_at, Some(ts(T2)));
        // Only that occurrence moved; the history is kept.
        assert_eq!(
            wf.proposal(&id("prop:0000000000000001"), &r1)
                .unwrap()
                .unwrap()
                .state,
            WorkflowProposalState::Pending
        );
        assert_eq!(
            wf.proposal(&id("prop:0000000000000001"), &r2).unwrap(),
            Some(abandoned)
        );
        let pending = wf.pending_proposals().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].base_revision_id, r1);
        assert!(wf
            .rejected_proposals(WorkflowOrigin::Question)
            .unwrap()
            .is_empty());
        for to in [
            WorkflowProposalState::Accepted,
            WorkflowProposalState::Rejected,
            WorkflowProposalState::Abandoned,
        ] {
            assert!(matches!(
                wf.resolve_proposal(&id("prop:0000000000000001"), &r2, to, &r2, ts(T2)),
                Err(StoreError::WorkflowStateConflict { .. })
            ));
        }
        drop(wf);
        let wf = db.workflow();
        assert_eq!(
            wf.proposal_occurrences(&id("prop:0000000000000001"))
                .unwrap()
                .len(),
            2
        );
    }

    /// R0.6: the pending-proposal enumeration returns every and only pending record, complete,
    /// in proposal-ID order whatever the insertion order, and changes nothing.
    #[test]
    fn pending_proposals_are_sorted_and_pending_only() {
        let db = Db::new();
        let (r1, r2) = db.with_revisions();
        let mut wf = db.workflow();
        session(&mut wf);
        let mut stage = new_proposal("prop:00000000000000c3", &r2, WorkflowOrigin::Stage);
        stage.origin_ref = None;
        let mut question = new_proposal("prop:00000000000000a1", &r1, WorkflowOrigin::Question);
        question.origin_ref = Some(id("q:0123456789abcdef"));
        question.proposal_kind = WorkflowProposalKind::Accept;
        let session_origin = new_proposal("prop:00000000000000b2", &r2, WorkflowOrigin::Session);
        // Deliberately unsorted insertion order; pending and non-pending rows interleaved.
        let pending = [
            (stage, WorkflowProposalState::Pending),
            (
                new_proposal("prop:00000000000000d4", &r1, WorkflowOrigin::Intake),
                WorkflowProposalState::Pending,
            ),
            (question, WorkflowProposalState::Pending),
            (session_origin, WorkflowProposalState::Pending),
        ];
        let others = [
            (
                new_proposal("prop:0000000000000099", &r1, WorkflowOrigin::Intake),
                WorkflowProposalState::Prepared,
            ),
            (
                new_proposal("prop:00000000000000a0", &r1, WorkflowOrigin::Intake),
                WorkflowProposalState::Pending,
            ),
            (
                new_proposal("prop:00000000000000e5", &r1, WorkflowOrigin::Intake),
                WorkflowProposalState::Pending,
            ),
        ];
        let mut expected = Vec::new();
        for (record, state) in pending.iter().chain(others.iter()) {
            let WorkflowInsert::Inserted(stored) = wf.insert_proposal(record, *state).unwrap()
            else {
                panic!("fresh insert");
            };
            if pending
                .iter()
                .any(|(p, _)| p.proposal_id == record.proposal_id)
            {
                expected.push(stored);
            }
        }
        // Resolve two of the "others": one accepted, one rejected.
        wf.resolve_proposal(
            &id("prop:00000000000000a0"),
            &r1,
            WorkflowProposalState::Accepted,
            &r2,
            ts(T2),
        )
        .unwrap();
        wf.resolve_proposal(
            &id("prop:00000000000000e5"),
            &r1,
            WorkflowProposalState::Rejected,
            &r2,
            ts(T2),
        )
        .unwrap();
        expected.sort_by(|a, b| a.proposal_id.cmp(&b.proposal_id));

        let before = db.rows("SELECT * FROM workflow_proposals ORDER BY proposal_id");
        let schema = db.schema();
        let listed = wf.pending_proposals().unwrap();
        // Only pending, ID ascending, complete records equal to what proposal(id) returns.
        assert_eq!(
            listed
                .iter()
                .map(|p| p.proposal_id.to_string())
                .collect::<Vec<_>>(),
            [
                "prop:00000000000000a1",
                "prop:00000000000000b2",
                "prop:00000000000000c3",
                "prop:00000000000000d4"
            ]
        );
        assert_eq!(listed, expected);
        for p in &listed {
            assert_eq!(p.state, WorkflowProposalState::Pending);
            assert_eq!(
                &wf.proposal(&p.proposal_id, &p.base_revision_id)
                    .unwrap()
                    .unwrap(),
                p
            );
        }
        let origins: BTreeSet<&str> = listed.iter().map(|p| p.origin_kind.as_str()).collect();
        assert_eq!(
            origins,
            BTreeSet::from(["intake", "question", "session", "stage"])
        );
        let q = listed
            .iter()
            .find(|p| p.origin_kind == WorkflowOrigin::Question)
            .unwrap();
        assert_eq!(q.origin_ref, Some(id("q:0123456789abcdef")));
        assert_eq!(q.base_revision_id, r1);
        assert_eq!(q.proposal_kind, WorkflowProposalKind::Accept);
        // Read-only: a repeated read is identical and no row or schema changed.
        assert_eq!(wf.pending_proposals().unwrap(), listed);
        assert_eq!(
            db.rows("SELECT * FROM workflow_proposals ORDER BY proposal_id"),
            before
        );
        assert_eq!(db.schema(), schema);
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
        assert_eq!(db.version(), vec![STORE_SCHEMA_VERSION]);
    }

    #[test]
    fn workflow_unknown_versions_and_unversioned_tables_are_rejected() {
        let db = Db::new();
        drop(db.workflow());
        db.exec("UPDATE schema_meta SET version = 9;");
        assert!(matches!(
            SqliteWorkflowStore::open(&db.path).unwrap_err(),
            StoreError::UnsupportedStoreSchemaVersion { found: 9 }
        ));
        assert_eq!(db.version(), vec![9]);

        let db = Db::new();
        db.exec("CREATE TABLE workflow_proposals (proposal_id TEXT PRIMARY KEY);");
        assert!(matches!(
            SqliteWorkflowStore::open(&db.path).unwrap_err(),
            StoreError::UnversionedStore
        ));
        assert!(!db.tables().contains("schema_meta"));

        // A v4 store missing a workflow or stage table is corrupt and never healed.
        let db = Db::new();
        drop(db.workflow());
        db.exec("DROP TABLE workflow_sessions;");
        assert!(matches!(
            SqliteWorkflowStore::open(&db.path).unwrap_err(),
            StoreError::CorruptStoreSchema { reason } if reason.contains("workflow_sessions")
        ));
        assert!(!db.tables().contains("workflow_sessions"));
        let db = Db::new();
        drop(db.workflow());
        db.exec("DROP TABLE stage_job_proposals;");
        assert!(matches!(
            SqliteWorkflowStore::open(&db.path).unwrap_err(),
            StoreError::CorruptStoreSchema { reason } if reason.contains("stage_job_proposals")
        ));
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
        assert_eq!(
            stored.occurrence_id,
            occurrence_id(&p.proposal_id, &r1).unwrap()
        );
        assert!(stored.occurrence_id.as_str().starts_with("occ:"));
        assert_eq!(stored.occurrence_id.as_str().len(), "occ:".len() + 16);
        // A second insert of the same occurrence returns the stored record unchanged.
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
        assert_eq!(wf.proposal(&p.proposal_id, &r1).unwrap(), Some(stored));
        assert_eq!(wf.proposal(&p.proposal_id, &r2).unwrap(), None);
        assert_eq!(
            wf.proposal(&id("prop:ffffffffffffffff"), &r1).unwrap(),
            None
        );
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
        assert!(wf
            .proposal_occurrences(&id("prop:0000000000000002"))
            .unwrap()
            .is_empty());
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
                &r1,
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
                &r1,
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
                wf.resolve_proposal(&id(target), &r1, to, &r2, ts(T2)),
                Err(StoreError::WorkflowStateConflict { .. })
            ));
        }
        assert!(matches!(
            wf.resolve_proposal(
                &id("prop:0000000000000001"),
                &r1,
                WorkflowProposalState::Pending,
                &r2,
                ts(T2)
            ),
            Err(StoreError::WorkflowInvalidTransition { .. })
        ));
        assert!(matches!(
            wf.resolve_proposal(
                &id("prop:ffffffffffffffff"),
                &r1,
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
            let stored = wf.proposal(&p.proposal_id, &r1).unwrap().unwrap();
            assert_eq!(stored.state, WorkflowProposalState::Prepared);
            assert_eq!(stored.origin_kind, WorkflowOrigin::Session);
        }
        // Prepared proposals cannot be resolved.
        assert!(matches!(
            wf.resolve_proposal(
                &p1.proposal_id,
                &r1,
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
            let stored = wf.proposal(&p.proposal_id, &r2).unwrap().unwrap();
            assert_eq!(stored.state, WorkflowProposalState::Pending);
            assert_eq!(stored.base_revision_id, r2);
            assert_eq!(
                stored.occurrence_id,
                occurrence_id(&p.proposal_id, &r2).unwrap()
            );
            assert_eq!(wf.proposal(&p.proposal_id, &r1).unwrap(), None);
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
            wf.proposal(&p3.proposal_id, &r2).unwrap().unwrap().state,
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
        assert_eq!(wf.proposal(&p1.proposal_id, &r1).unwrap(), None);
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
            &r1,
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
            wf.proposal(&id("prop:0000000000000001"), &r1),
            Err(StoreError::CorruptWorkflowRecord { .. })
        ));
        // An occurrence key that does not match its row is corruption too.
        db.exec(&format!(
            "UPDATE workflow_proposals SET created_at = '{T1}', \
             occurrence_id = 'occ:0000000000000000';"
        ));
        assert!(matches!(
            wf.proposal(&id("prop:0000000000000001"), &r1),
            Err(StoreError::CorruptWorkflowRecord { .. })
        ));
    }

    // ------------------------------------------------------------------ occurrences (H0.10)

    /// Revisions 1..=3 on main, each adding one Proposed requirement.
    fn three_revisions(db: &Db) -> (RevisionId, RevisionId, RevisionId) {
        let (r1, r2) = db.with_revisions();
        let mut store = SqliteRevisionStore::open(&db.path).unwrap();
        let g2 = store.load_revision(&r2).unwrap().graph;
        let r3 = store
            .commit(
                &BranchName::main(),
                &r2,
                &PatchSet {
                    base_semantic_hash: g2.semantic_hash().unwrap(),
                    patch: SemanticPatch::AddNode {
                        node: requirement("req:c", "Proposed"),
                    },
                },
                meta(T2),
            )
            .unwrap()
            .revision
            .id;
        (r1, r2, r3)
    }

    #[test]
    fn identical_proposal_on_later_baseline_is_a_new_occurrence() {
        let db = Db::new();
        let (r1, r2, r3) = three_revisions(&db);
        let mut wf = db.workflow();
        let on =
            |base: &RevisionId| new_proposal("prop:00000000000000aa", base, WorkflowOrigin::Intake);
        let pid = id("prop:00000000000000aa");

        // Occurrence 1 on r1 is rejected on its own revision.
        let WorkflowInsert::Inserted(first) = wf
            .insert_proposal(&on(&r1), WorkflowProposalState::Pending)
            .unwrap()
        else {
            panic!("fresh insert");
        };
        wf.resolve_proposal(&pid, &r1, WorkflowProposalState::Rejected, &r1, ts(T2))
            .unwrap();

        // The identical proposal on r2 is a new, pending occurrence: never Existing.
        let second = match wf
            .insert_proposal(&on(&r2), WorkflowProposalState::Pending)
            .unwrap()
        {
            WorkflowInsert::Inserted(p) => p,
            other => panic!("expected a new occurrence, got {other:?}"),
        };
        assert_eq!(second.proposal_id, first.proposal_id);
        assert_eq!(second.base_revision_id, r2);
        assert_eq!(second.state, WorkflowProposalState::Pending);
        assert_ne!(second.occurrence_id, first.occurrence_id);
        assert_eq!(second.occurrence_id, occurrence_id(&pid, &r2).unwrap());

        // Historical occurrences are immutable.
        let historical = wf.proposal(&pid, &r1).unwrap().unwrap();
        assert_eq!(historical.state, WorkflowProposalState::Rejected);
        assert_eq!(historical.resolved_revision_id, Some(r1.clone()));

        // A lookup on the loaded head resolves exactly its pending occurrence.
        assert_eq!(
            wf.current_proposal(&pid, &r2).unwrap(),
            Some(second.clone())
        );
        // On another head there is no pending occurrence: the newest one is reported, whose
        // base is not the head (E_PROPOSAL_BASELINE_CHANGED above the store).
        let other = wf.current_proposal(&pid, &r3).unwrap().unwrap();
        assert_eq!(other.base_revision_id, r2);
        assert_eq!(
            wf.current_proposal(&id("prop:ffffffffffffffff"), &r3)
                .unwrap(),
            None
        );
        assert_eq!(
            wf.proposal_occurrences(&pid)
                .unwrap()
                .iter()
                .map(|p| p.base_revision_id.clone())
                .collect::<Vec<_>>(),
            [r1.clone(), r2.clone()]
        );
        assert_eq!(wf.pending_proposals().unwrap(), vec![second.clone()]);

        // The r2 occurrence goes stale and the identical proposal is governed again on r3.
        let third = match wf
            .insert_proposal(&on(&r3), WorkflowProposalState::Pending)
            .unwrap()
        {
            WorkflowInsert::Inserted(p) => p,
            other => panic!("expected a new occurrence, got {other:?}"),
        };
        assert_eq!(wf.current_proposal(&pid, &r3).unwrap(), Some(third.clone()));
        assert_eq!(
            wf.pending_proposals().unwrap(),
            vec![second.clone(), third.clone()]
        );
        // Resolving one occurrence never touches another.
        wf.resolve_proposal(&pid, &r2, WorkflowProposalState::Rejected, &r3, ts(T2))
            .unwrap();
        assert_eq!(
            wf.proposal(&pid, &r3).unwrap().unwrap().state,
            WorkflowProposalState::Pending
        );
        assert_eq!(wf.pending_proposals().unwrap(), vec![third]);
    }

    #[test]
    fn accepted_resolution_must_descend_from_the_base() {
        let db = Db::new();
        let (r1, r2, r3) = three_revisions(&db);
        // A branch from r1 whose head is not on main's lineage after r2.
        let mut store = SqliteRevisionStore::open(&db.path).unwrap();
        store
            .create_branch("side".parse().unwrap(), r1.clone(), ts(T2))
            .unwrap();
        let g1 = store.load_revision(&r1).unwrap().graph;
        let side = store
            .commit(
                &"side".parse().unwrap(),
                &r1,
                &PatchSet {
                    base_semantic_hash: g1.semantic_hash().unwrap(),
                    patch: SemanticPatch::AddNode {
                        node: requirement("req:side", "Proposed"),
                    },
                },
                meta(T2),
            )
            .unwrap()
            .revision
            .id;
        let mut wf = db.workflow();
        let pid = id("prop:00000000000000bb");
        wf.insert_proposal(
            &new_proposal("prop:00000000000000bb", &r2, WorkflowOrigin::Intake),
            WorkflowProposalState::Pending,
        )
        .unwrap();
        let before = db.rows("SELECT * FROM workflow_proposals ORDER BY proposal_id");
        // The base itself, an older revision and a revision of another lineage are refused.
        for resolved in [&r2, &r1, &side] {
            assert!(
                matches!(
                    wf.resolve_proposal(
                        &pid,
                        &r2,
                        WorkflowProposalState::Accepted,
                        resolved,
                        ts(T2)
                    ),
                    Err(StoreError::WorkflowResolutionNotDescendant { .. })
                ),
                "{resolved}"
            );
        }
        assert_eq!(
            db.rows("SELECT * FROM workflow_proposals ORDER BY proposal_id"),
            before
        );
        // A rejection may be recorded on the base itself.
        // A descendant with a greater version on the lineage is accepted.
        let accepted = wf
            .resolve_proposal(&pid, &r2, WorkflowProposalState::Accepted, &r3, ts(T2))
            .unwrap();
        assert_eq!(accepted.resolved_revision_id, Some(r3));
    }
}
