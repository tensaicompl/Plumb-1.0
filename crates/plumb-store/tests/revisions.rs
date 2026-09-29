//! F0.8 contract tests for the SQLite revision store, branch heads and CAS commit.
//!
//! Every test lives in `revisions` so that `cargo test -p plumb-store revisions` and the
//! unfiltered `cargo test -p plumb-store` select all of them.

mod revisions {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Barrier};
    use std::thread;

    use plumb_artifacts::{ArtifactKind, ArtifactStore, SqliteArtifactStore};
    use plumb_core::{to_canonical_json, Hash, HashKind, Id, Timestamp};
    use plumb_patch::{PatchError, PatchSet, SemanticPatch};
    use plumb_psg::*;
    use plumb_store::*;
    use rusqlite::Connection;
    use serde_json::{json, Value};
    use tempfile::TempDir;

    const T1: &str = "2026-09-29T08:00:00.000000000Z";
    const T2: &str = "2026-09-29T09:00:00.000000000Z";
    const T3: &str = "2026-09-29T10:00:00.000000000Z";
    const T4: &str = "2026-09-29T11:00:00.000000000Z";
    const PROJECT: &str = "project:leave-management";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const JSON: &str = "application/json";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn branch(s: &str) -> BranchName {
        s.parse().unwrap()
    }

    fn generic(c: &str) -> Hash {
        format!("sha256:{}", c.repeat(64)).parse().unwrap()
    }

    fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{e}: {value}"))
    }

    fn node(node_id: &str, status: &str, tag: &str, data: Value) -> Node {
        from_json(json!({
            "id": node_id, "revision": 1, "status": status,
            "payload": {"type": tag, "data": data},
            "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
            "audit": {"created_by": "actor:analyst", "created_at": T1, "updated_by": null, "updated_at": null}
        }))
    }

    fn requirement_data(statement: &str) -> Value {
        json!({
            "statement": statement, "requirement_kind": "functional", "level": "system",
            "modality": "shall", "title": null, "rationale": null, "priority": null,
            "source_identifier": null, "verification_method": null, "owner_refs": null,
            "stakeholder_refs": null
        })
    }

    fn requirement(node_id: &str, status: &str, statement: &str) -> Node {
        node(node_id, status, "Requirement", requirement_data(statement))
    }

    fn constraint(node_id: &str) -> Node {
        node(
            node_id,
            "Accepted",
            "Constraint",
            json!({
                "statement": "Leave data must stay in the EU.",
                "constraint_category": "regulatory", "strength": "mandatory"
            }),
        )
    }

    fn decision(node_id: &str, status: &str) -> Node {
        node(
            node_id,
            status,
            "ResolutionDecision",
            json!({
                "question_ref": "q:carry-over", "proposal_ref": null, "answer": "5 days",
                "decided_by": "actor:hr-lead", "decided_at": T1,
                "patch_ref": format!("sha256:{}", "e".repeat(64)),
                "rationale": null, "supersedes": null
            }),
        )
    }

    fn edge(edge_id: &str, kind: &str, from: &str, to: &str) -> Edge {
        from_json(json!({
            "id": edge_id, "revision": 1, "status": "Accepted", "kind": kind, "from": from,
            "to": to, "properties": {}, "evidence": [], "derivations": [], "standards": [],
            "audit": {"created_by": "actor:analyst", "created_at": T1, "updated_by": null, "updated_at": null}
        }))
    }

    fn graph_in(project: &str, nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(project), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    /// Accepted and Proposed requirements, a constraint edge and two baseline decisions.
    fn base_graph() -> Graph {
        graph_in(
            PROJECT,
            vec![
                requirement(
                    "req:a",
                    "Accepted",
                    "Employees shall submit leave requests.",
                ),
                requirement(
                    "req:b",
                    "Proposed",
                    "Managers shall approve leave requests.",
                ),
                constraint("con:eu"),
                question("q:carry-over"),
                decision("dec:1", "Accepted"),
                decision("dec:2", "Accepted"),
                decision("dec:proposed", "Proposed"),
            ],
            vec![
                edge("rel:a-eu", "constrained_by", "req:a", "con:eu"),
                edge("rel:dec1-q", "resolves", "dec:1", "q:carry-over"),
                edge("rel:dec2-q", "resolves", "dec:2", "q:carry-over"),
            ],
        )
    }

    fn question(node_id: &str) -> Node {
        node(
            node_id,
            "Accepted",
            "Question",
            json!({
                "finding_ref": "finding:carry-over", "question_kind": "Cardinality",
                "prompt": "How many days carry over?", "status": "answered",
                "answer_schema": null, "stakeholder_ref": null, "priority": null,
                "round_ref": null, "context_refs": null
            }),
        )
    }

    fn meta(at: &str) -> RevisionWriteMeta {
        RevisionWriteMeta {
            decision_refs: vec![],
            created_by: id("actor:analyst"),
            created_at: ts(at),
        }
    }

    fn patch_set(base: &Graph, patch: SemanticPatch) -> PatchSet {
        PatchSet {
            base_semantic_hash: base.semantic_hash().unwrap(),
            patch,
        }
    }

    /// Adds an Accepted requirement: changes the semantic hash.
    fn add_accepted(base: &Graph, node_id: &str) -> PatchSet {
        patch_set(
            base,
            SemanticPatch::AddNode {
                node: requirement(node_id, "Accepted", "The system shall log requests."),
            },
        )
    }

    /// Adds a Proposed requirement: keeps the semantic hash.
    fn add_proposed(base: &Graph, node_id: &str) -> PatchSet {
        patch_set(
            base,
            SemanticPatch::AddNode {
                node: requirement(node_id, "Proposed", "The system shall archive requests."),
            },
        )
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

        fn store(&self) -> SqliteRevisionStore {
            SqliteRevisionStore::open(&self.path).unwrap()
        }

        fn raw(&self) -> Connection {
            Connection::open(&self.path).unwrap()
        }

        fn exec(&self, sql: &str) {
            self.raw().execute_batch(sql).unwrap();
        }

        fn count(&self, sql: &str) -> i64 {
            self.raw().query_row(sql, [], |r| r.get(0)).unwrap()
        }

        /// Opens a store and creates revision 1 from `base_graph()`.
        fn initialized(&self) -> (SqliteRevisionStore, GraphRevision) {
            let mut store = self.store();
            let rev = store
                .create_initial_revision(&base_graph(), generic("a"), generic("b"), meta(T1))
                .unwrap();
            (store, rev)
        }
    }

    fn tables(path: &Path) -> BTreeSet<String> {
        let conn = Connection::open(path).unwrap();
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

    fn schema_sql(path: &Path) -> Vec<(String, Option<String>)> {
        let conn = Connection::open(path).unwrap();
        let mut stmt = conn
            .prepare("SELECT name, sql FROM sqlite_master ORDER BY name")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    }

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
            ))
            .unwrap();
        let cols = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        cols
    }

    fn schema_meta_rows(path: &Path) -> Vec<i64> {
        let conn = Connection::open(path).unwrap();
        let mut stmt = conn.prepare("SELECT version FROM schema_meta").unwrap();
        let rows = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    }

    fn corrupt_reason(err: StoreError) -> String {
        match err {
            StoreError::CorruptRevision { reason, .. } => reason,
            other => panic!("expected CorruptRevision, got {other:?}"),
        }
    }

    // ------------------------------------------------------------------ schema

    #[test]
    fn fresh_database_initializes_exact_v1_schema() {
        let db = Db::new();
        let store = db.store();
        assert_eq!(STORE_SCHEMA_VERSION, 1);
        assert_eq!(PSG_SCHEMA_VERSION, 1);
        let expected: BTreeSet<String> = [
            "artifacts",
            "branch_heads",
            "graph_revisions",
            "ledger_events",
            "revision_edges",
            "revision_nodes",
            "schema_meta",
            "sqlite_sequence",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(tables(&db.path), expected);
        assert_eq!(schema_meta_rows(&db.path), vec![1]);
        let conn = db.raw();
        assert_eq!(
            columns(&conn, "graph_revisions"),
            [
                "id",
                "version",
                "parent_id",
                "project_id",
                "psg_schema_version",
                "semantic_hash",
                "evidence_hash",
                "profile_ref",
                "profile_hash",
                "rule_pack_hash",
                "patch_artifact_hash",
                "decision_refs_json",
                "created_by",
                "created_at"
            ]
        );
        assert_eq!(
            columns(&conn, "revision_nodes"),
            ["revision_id", "node_id", "node_json"]
        );
        assert_eq!(
            columns(&conn, "revision_edges"),
            ["revision_id", "edge_id", "edge_json"]
        );
        assert_eq!(
            columns(&conn, "branch_heads"),
            ["name", "revision_id", "updated_at"]
        );
        assert_eq!(
            columns(&conn, "ledger_events"),
            ["seq", "revision_id", "event_json"]
        );
        assert_eq!(
            columns(&conn, "artifacts"),
            ["hash", "kind", "media_type", "bytes", "created_at"]
        );
        let journal: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal, "wal");
        drop(store);
    }

    #[test]
    fn store_connection_enforces_foreign_keys_and_checks() {
        let db = Db::new();
        let (store, rev) = db.initialized();
        drop(store);
        // A branch pointing to a missing revision is rejected by the store's own pragmas.
        let mut store = db.store();
        let err = store
            .create_branch(
                branch("ghost"),
                "rev:9:0123456789abcdef".parse().unwrap(),
                ts(T2),
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::RevisionNotFound(_)));
        assert_eq!(store.head(&branch("main")).unwrap(), Some(rev.id));
        let conn = db.raw();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        assert!(conn
            .execute(
                "INSERT INTO branch_heads (name, revision_id, updated_at) VALUES ('x', 'rev:9:0123456789abcdef', ?1)",
                [T2],
            )
            .is_err());
        assert!(conn
            .execute("UPDATE graph_revisions SET version = 0", [],)
            .is_err());
    }

    #[test]
    fn artifact_only_database_is_adopted() {
        let db = Db::new();
        let hash = {
            let mut artifacts = SqliteArtifactStore::open(&db.path).unwrap();
            artifacts
                .put(
                    ArtifactKind::SourceOriginal,
                    "text/markdown",
                    b"# HR",
                    ts(T1),
                )
                .unwrap()
        };
        let before: (String, String, Vec<u8>, String) = db
            .raw()
            .query_row(
                "SELECT kind, media_type, bytes, created_at FROM artifacts WHERE hash = ?1",
                [hash.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        let (store, rev) = db.initialized();
        assert_eq!(rev.version, 1);
        let after: (String, String, Vec<u8>, String) = db
            .raw()
            .query_row(
                "SELECT kind, media_type, bytes, created_at FROM artifacts WHERE hash = ?1",
                [hash.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(after, before);
        assert_eq!(db.count("SELECT COUNT(*) FROM artifacts"), 1);
        assert_eq!(schema_meta_rows(&db.path), vec![1]);
        drop(store);
    }

    #[test]
    fn unsupported_store_schema_version_is_rejected_without_change() {
        let db = Db::new();
        drop(db.store());
        db.exec("UPDATE schema_meta SET version = 2;");
        let before = schema_sql(&db.path);
        assert!(matches!(
            SqliteRevisionStore::open(&db.path).unwrap_err(),
            StoreError::UnsupportedStoreSchemaVersion { found: 2 }
        ));
        assert_eq!(schema_sql(&db.path), before);
        assert_eq!(schema_meta_rows(&db.path), vec![2]);
    }

    #[test]
    fn malformed_schema_meta_and_missing_tables_are_corrupt() {
        let db = Db::new();
        drop(db.store());
        db.exec("INSERT INTO schema_meta (version) VALUES (1);");
        assert!(matches!(
            SqliteRevisionStore::open(&db.path).unwrap_err(),
            StoreError::CorruptStoreSchema { .. }
        ));
        db.exec("DELETE FROM schema_meta;");
        assert!(matches!(
            SqliteRevisionStore::open(&db.path).unwrap_err(),
            StoreError::CorruptStoreSchema { .. }
        ));

        let db = Db::new();
        drop(db.store());
        db.exec("DROP TABLE ledger_events;");
        let err = SqliteRevisionStore::open(&db.path).unwrap_err();
        assert!(
            matches!(&err, StoreError::CorruptStoreSchema { reason } if reason.contains("ledger_events")),
            "{err:?}"
        );
        // Never auto-healed.
        assert!(!tables(&db.path).contains("ledger_events"));
    }

    #[test]
    fn unversioned_graph_store_is_rejected() {
        let db = Db::new();
        db.exec("CREATE TABLE graph_revisions (id TEXT PRIMARY KEY);");
        let before = schema_sql(&db.path);
        assert!(matches!(
            SqliteRevisionStore::open(&db.path).unwrap_err(),
            StoreError::UnversionedStore
        ));
        assert_eq!(schema_sql(&db.path), before);
        assert!(!tables(&db.path).contains("schema_meta"));
    }

    // ------------------------------------------------------------------ identifiers

    #[test]
    fn revision_id_grammar() {
        for valid in ["rev:1:0123456789abcdef", "rev:42:abcdef0123456789"] {
            let rid: RevisionId = valid.parse().unwrap();
            assert_eq!(rid.as_str(), valid);
            assert_eq!(serde_json::to_value(&rid).unwrap(), json!(valid));
            assert_eq!(
                serde_json::from_value::<RevisionId>(json!(valid)).unwrap(),
                rid
            );
        }
        assert_eq!(
            "rev:42:abcdef0123456789"
                .parse::<RevisionId>()
                .unwrap()
                .version(),
            42
        );
        for invalid in [
            "rev:0:0123456789abcdef",
            "rev:01:0123456789abcdef",
            "Rev:1:0123456789abcdef",
            "rev:1:0123456789ABCDEF",
            "rev:1:0123456789abcde",
            "rev:1:0123456789abcdef0",
            "rev:1:0123456789abcdef:x",
            "rev:1:0123456789abcdef ",
            " rev:1:0123456789abcdef",
            "rev: 1:0123456789abcdef",
            "rev:-1:0123456789abcdef",
            "rev::0123456789abcdef",
            "rev:1",
            "",
        ] {
            assert!(invalid.parse::<RevisionId>().is_err(), "{invalid:?}");
            assert!(serde_json::from_value::<RevisionId>(json!(invalid)).is_err());
        }
        let semantic: Hash = format!("psg:sha256:{}", "0123456789abcdef".repeat(4))
            .parse()
            .unwrap();
        let rid = RevisionId::new(7, &semantic).unwrap();
        assert_eq!(rid.as_str(), "rev:7:0123456789abcdef");
        assert_eq!(rid.version(), 7);
        assert!(RevisionId::new(0, &semantic).is_err());
        assert!(RevisionId::new(1, &generic("a")).is_err());
        let evidence: Hash = format!("ev:sha256:{}", "a".repeat(64)).parse().unwrap();
        assert!(RevisionId::new(1, &evidence).is_err());
    }

    #[test]
    fn branch_name_grammar() {
        let max = format!("a{}", "b".repeat(254));
        for valid in [
            "main",
            "candidate:architecture-A",
            "candidate/team-A",
            "proposal:prop-123",
            "7.x_release",
            max.as_str(),
        ] {
            let name: BranchName = valid.parse().unwrap();
            assert_eq!(name.as_str(), valid);
            assert_eq!(serde_json::to_value(&name).unwrap(), json!(valid));
            assert_eq!(
                serde_json::from_value::<BranchName>(json!(valid)).unwrap(),
                name
            );
        }
        let too_long = format!("a{}", "b".repeat(255));
        for invalid in [
            "",
            "-main",
            ".main",
            ":main",
            "/main",
            "has space",
            "back\\slash",
            "tab\tname",
            "new\nline",
            "zażółć",
            too_long.as_str(),
        ] {
            assert!(invalid.parse::<BranchName>().is_err(), "{invalid:?}");
            assert!(serde_json::from_value::<BranchName>(json!(invalid)).is_err());
        }
    }

    // ------------------------------------------------------------------ initial revision

    #[test]
    fn initial_revision_persists_exact_metadata_and_main_branch() {
        let db = Db::new();
        let graph = base_graph();
        let (store, rev) = db.initialized();
        let semantic = graph.semantic_hash().unwrap();
        let digest = &semantic.as_str()["psg:sha256:".len()..][..16];
        assert_eq!(rev.id.as_str(), format!("rev:1:{digest}"));
        assert_eq!(rev.version, 1);
        assert_eq!(rev.parent, None);
        assert_eq!(rev.project_id, id(PROJECT));
        assert_eq!(rev.psg_schema_version, PSG_SCHEMA_VERSION);
        assert_eq!(rev.semantic_hash, semantic);
        assert_eq!(rev.evidence_hash, graph.evidence_hash().unwrap());
        assert_eq!(rev.profile_ref, id(PROFILE));
        assert_eq!(rev.profile_hash, generic("a"));
        assert_eq!(rev.rule_pack_hash, generic("b"));
        assert_eq!(rev.accepted_patch_ref, None);
        assert_eq!(rev.decision_refs, Vec::<Id>::new());
        assert_eq!(rev.created_by, id("actor:analyst"));
        assert_eq!(rev.created_at, ts(T1));
        assert_eq!(store.head(&branch("main")).unwrap(), Some(rev.id.clone()));
        let updated_at: String = db
            .raw()
            .query_row(
                "SELECT updated_at FROM branch_heads WHERE name = 'main'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(updated_at, T1);
        let loaded = store.load_revision(&rev.id).unwrap();
        assert_eq!(loaded.revision, rev);
        assert_eq!(loaded.graph, graph);
        assert_eq!(db.count("SELECT COUNT(*) FROM ledger_events"), 0);
        assert_eq!(db.count("SELECT COUNT(*) FROM artifacts"), 0);
    }

    #[test]
    fn second_initial_revision_is_already_initialized_even_for_another_project() {
        let db = Db::new();
        let (mut store, rev) = db.initialized();
        let other = graph_in("project:other", vec![constraint("con:x")], vec![]);
        assert!(matches!(
            store
                .create_initial_revision(&other, generic("a"), generic("b"), meta(T2))
                .unwrap_err(),
            StoreError::AlreadyInitialized
        ));
        assert!(matches!(
            store
                .create_initial_revision(&base_graph(), generic("a"), generic("b"), meta(T2))
                .unwrap_err(),
            StoreError::AlreadyInitialized
        ));
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 1);
        assert_eq!(db.count("SELECT COUNT(*) FROM branch_heads"), 1);
        assert_eq!(store.head(&branch("main")).unwrap(), Some(rev.id));
    }

    #[test]
    fn profile_and_rule_pack_hashes_must_be_generic() {
        let db = Db::new();
        let mut store = db.store();
        let semantic = base_graph().semantic_hash().unwrap();
        assert!(matches!(
            store
                .create_initial_revision(&base_graph(), semantic.clone(), generic("b"), meta(T1))
                .unwrap_err(),
            StoreError::InvalidProfileHashKind(h) if h == semantic
        ));
        let evidence = base_graph().evidence_hash().unwrap();
        assert!(matches!(
            store
                .create_initial_revision(&base_graph(), generic("a"), evidence.clone(), meta(T1))
                .unwrap_err(),
            StoreError::InvalidRulePackHashKind(h) if h == evidence
        ));
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 0);
        assert_eq!(store.head(&branch("main")).unwrap(), None);
    }

    #[test]
    fn decision_refs_are_validated_and_persisted_sorted() {
        let db = Db::new();
        let mut store = db.store();
        let graph = base_graph();
        let with = |refs: &[&str]| RevisionWriteMeta {
            decision_refs: refs.iter().map(|r| id(r)).collect(),
            ..meta(T1)
        };
        let mut attempt = |refs: &[&str]| {
            store
                .create_initial_revision(&graph, generic("a"), generic("b"), with(refs))
                .unwrap_err()
        };
        assert!(
            matches!(attempt(&["dec:1", "dec:1"]), StoreError::DuplicateDecisionRef(d) if d == id("dec:1"))
        );
        assert!(
            matches!(attempt(&["dec:missing"]), StoreError::DecisionRefMissing(d) if d == id("dec:missing"))
        );
        assert!(matches!(
            attempt(&["req:a"]),
            StoreError::DecisionRefWrongType { id: d, node_type: NodeType::Requirement } if d == id("req:a")
        ));
        assert!(matches!(
            attempt(&["dec:proposed"]),
            StoreError::DecisionRefNotBaseline { id: d, status: ElementStatus::Proposed } if d == id("dec:proposed")
        ));
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 0);

        let rev = store
            .create_initial_revision(
                &graph,
                generic("a"),
                generic("b"),
                with(&["dec:2", "dec:1"]),
            )
            .unwrap();
        assert_eq!(rev.decision_refs, vec![id("dec:1"), id("dec:2")]);
        let stored: String = db
            .raw()
            .query_row("SELECT decision_refs_json FROM graph_revisions", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stored, r#"["dec:1","dec:2"]"#);
        assert_eq!(
            store.load_revision(&rev.id).unwrap().revision.decision_refs,
            rev.decision_refs
        );

        // Commit validates decision refs against the resulting graph.
        let head = store.load_revision(&rev.id).unwrap().graph;
        let ps = add_proposed(&head, "req:new");
        let err = store
            .commit(
                &branch("main"),
                &rev.id,
                &ps,
                RevisionWriteMeta {
                    decision_refs: vec![id("req:new")],
                    ..meta(T2)
                },
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::DecisionRefWrongType { .. }));
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 1);
        assert_eq!(db.count("SELECT COUNT(*) FROM artifacts"), 0);
    }

    // ------------------------------------------------------------------ snapshots

    #[test]
    fn full_snapshot_round_trips_as_canonical_json() {
        let db = Db::new();
        let (store, rev) = db.initialized();
        let graph = base_graph();
        let conn = db.raw();
        let mut stmt = conn
            .prepare("SELECT node_id, node_json FROM revision_nodes WHERE revision_id = ?1 ORDER BY rowid")
            .unwrap();
        let nodes: Vec<(String, String)> = stmt
            .query_map([rev.id.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        // Every node, including Proposed ones, stored in element-ID order as canonical JSON.
        let expected: Vec<(String, String)> = graph
            .nodes()
            .iter()
            .map(|(k, n)| {
                (
                    k.as_str().to_owned(),
                    String::from_utf8(to_canonical_json(n).unwrap()).unwrap(),
                )
            })
            .collect();
        assert_eq!(nodes, expected);
        assert_eq!(nodes.len(), 7);
        let edge_json: String = conn
            .query_row(
                "SELECT edge_json FROM revision_edges WHERE revision_id = ?1 AND edge_id = 'rel:a-eu'",
                [rev.id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            edge_json.as_bytes(),
            to_canonical_json(graph.edge(&id("rel:a-eu")).unwrap())
                .unwrap()
                .as_slice()
        );
        let loaded = store.load_revision(&rev.id).unwrap();
        assert_eq!(loaded.graph, graph);
        assert_eq!(loaded.graph.semantic_hash().unwrap(), rev.semantic_hash);
    }

    // ------------------------------------------------------------------ commit

    #[test]
    fn commit_persists_exact_patch_artifact_and_parent_linkage() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        let base = base_graph();
        let ps = add_accepted(&base, "req:log");
        let r2 = store
            .commit(&branch("main"), &r1.id, &ps, meta(T2))
            .unwrap();

        let bytes = to_canonical_json(&ps).unwrap();
        let patch_ref = Hash::content_sha256(&bytes);
        assert_eq!(r2.accepted_patch_ref, Some(patch_ref.clone()));
        assert_eq!(r2.version, 2);
        assert_eq!(r2.parent, Some(r1.id.clone()));
        assert_eq!(r2.project_id, r1.project_id);
        assert_eq!(r2.profile_ref, r1.profile_ref);
        assert_eq!(r2.profile_hash, r1.profile_hash);
        assert_eq!(r2.rule_pack_hash, r1.rule_pack_hash);
        assert_eq!(r2.psg_schema_version, PSG_SCHEMA_VERSION);
        assert_eq!(r2.created_at, ts(T2));
        assert_ne!(r2.semantic_hash, r1.semantic_hash);
        assert_eq!(r2.id, RevisionId::new(2, &r2.semantic_hash).unwrap());
        assert_eq!(store.head(&branch("main")).unwrap(), Some(r2.id.clone()));

        let artifact = SqliteArtifactStore::open(&db.path)
            .unwrap()
            .get(&patch_ref)
            .unwrap();
        assert_eq!(artifact.bytes, bytes);
        assert_eq!(artifact.kind, ArtifactKind::Patch);
        assert_eq!(artifact.media_type, JSON);
        assert_eq!(artifact.created_at, ts(T2));
        assert_eq!(
            serde_json::from_slice::<PatchSet>(&artifact.bytes).unwrap(),
            ps
        );

        let loaded = store.load_revision(&r2.id).unwrap();
        assert_eq!(loaded.revision, r2);
        assert!(loaded.graph.node(&id("req:log")).is_some());
        let updated_at: String = db
            .raw()
            .query_row(
                "SELECT updated_at FROM branch_heads WHERE name = 'main'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(updated_at, T2);
        assert_eq!(db.count("SELECT COUNT(*) FROM ledger_events"), 0);
    }

    #[test]
    fn global_versions_and_branch_independence() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        let other = branch("candidate:architecture-A");
        store
            .create_branch(other.clone(), r1.id.clone(), ts(T1))
            .unwrap();
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 1);
        assert_eq!(store.head(&other).unwrap(), Some(r1.id.clone()));

        let base = base_graph();
        let r2 = store
            .commit(
                &branch("main"),
                &r1.id,
                &add_accepted(&base, "req:main"),
                meta(T2),
            )
            .unwrap();
        assert_eq!(r2.version, 2);
        assert_eq!(store.head(&other).unwrap(), Some(r1.id.clone()));

        let r3 = store
            .commit(&other, &r1.id, &add_accepted(&base, "req:other"), meta(T3))
            .unwrap();
        assert_eq!(r3.version, 3);
        assert_eq!(r3.parent, Some(r1.id.clone()));
        assert_eq!(store.head(&branch("main")).unwrap(), Some(r2.id.clone()));
        assert_eq!(store.head(&other).unwrap(), Some(r3.id.clone()));
        assert!(store
            .load_revision(&r3.id)
            .unwrap()
            .graph
            .node(&id("req:main"))
            .is_none());
        let versions: Vec<i64> = {
            let conn = db.raw();
            let mut stmt = conn
                .prepare("SELECT version FROM graph_revisions ORDER BY version")
                .unwrap();
            let v = stmt
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            v
        };
        assert_eq!(versions, vec![1, 2, 3]);
    }

    #[test]
    fn branch_operations_validate_inputs() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        assert_eq!(store.head(&branch("nope")).unwrap(), None);
        assert_eq!(db.count("SELECT COUNT(*) FROM branch_heads"), 1);
        assert!(matches!(
            store.create_branch(branch("main"), r1.id.clone(), ts(T2)).unwrap_err(),
            StoreError::BranchAlreadyExists(b) if b == branch("main")
        ));
        let missing: RevisionId = "rev:5:0123456789abcdef".parse().unwrap();
        assert!(matches!(
            store.create_branch(branch("x"), missing.clone(), ts(T2)).unwrap_err(),
            StoreError::RevisionNotFound(r) if r == missing
        ));
        let ps = add_accepted(&base_graph(), "req:x");
        assert!(matches!(
            store.commit(&branch("nope"), &r1.id, &ps, meta(T2)).unwrap_err(),
            StoreError::BranchNotFound(b) if b == branch("nope")
        ));
        assert!(matches!(
            store
                .move_head(&branch("nope"), &r1.id, &r1.id, ts(T2))
                .unwrap_err(),
            StoreError::BranchNotFound(_)
        ));
        assert!(matches!(
            store
                .move_head(&branch("main"), &r1.id, &missing, ts(T2))
                .unwrap_err(),
            StoreError::RevisionNotFound(_)
        ));
        store
            .create_branch(branch("proposal:prop-123"), r1.id.clone(), ts(T3))
            .unwrap();
        let updated_at: String = db
            .raw()
            .query_row(
                "SELECT updated_at FROM branch_heads WHERE name = 'proposal:prop-123'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(updated_at, T3);
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 1);
    }

    #[test]
    fn stale_branch_head_is_rejected_without_changes() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        let base = base_graph();
        let r2 = store
            .commit(
                &branch("main"),
                &r1.id,
                &add_accepted(&base, "req:one"),
                meta(T2),
            )
            .unwrap();
        let err = store
            .commit(
                &branch("main"),
                &r1.id,
                &add_accepted(&base, "req:two"),
                meta(T3),
            )
            .unwrap_err();
        match err {
            StoreError::StaleBase {
                branch: b,
                expected,
                actual,
            } => {
                assert_eq!(b, branch("main"));
                assert_eq!(expected, r1.id);
                assert_eq!(actual, r2.id);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 2);
        assert_eq!(db.count("SELECT COUNT(*) FROM artifacts"), 1);
        assert_eq!(store.head(&branch("main")).unwrap(), Some(r2.id));
    }

    #[test]
    fn patch_set_semantic_base_is_checked_even_when_head_matches() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        let other = graph_in(PROJECT, vec![constraint("con:x")], vec![]);
        let ps = add_accepted(&other, "req:x");
        let err = store
            .commit(&branch("main"), &r1.id, &ps, meta(T2))
            .unwrap_err();
        assert!(
            matches!(err, StoreError::Patch(PatchError::StaleBase { .. })),
            "{err:?}"
        );
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 1);
        assert_eq!(db.count("SELECT COUNT(*) FROM artifacts"), 0);
        assert_eq!(store.head(&branch("main")).unwrap(), Some(r1.id));
    }

    #[test]
    fn concurrent_commits_yield_one_success_and_one_stale_base() {
        let db = Db::new();
        let (store, r1) = db.initialized();
        drop(store);
        let base = base_graph();
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = ["req:left", "req:right"]
            .into_iter()
            .map(|node_id| {
                let path = db.path.clone();
                let expected = r1.id.clone();
                let ps = add_accepted(&base, node_id);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let mut store = SqliteRevisionStore::open(&path).unwrap();
                    barrier.wait();
                    (
                        ps.clone(),
                        store.commit(&branch("main"), &expected, &ps, meta(T2)),
                    )
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let winners: Vec<&GraphRevision> = results
            .iter()
            .filter_map(|(_, r)| r.as_ref().ok())
            .collect();
        assert_eq!(winners.len(), 1, "{results:?}");
        let winner = winners[0].clone();
        let losers: Vec<&StoreError> = results
            .iter()
            .filter_map(|(_, r)| r.as_ref().err())
            .collect();
        assert_eq!(losers.len(), 1);
        assert!(
            matches!(losers[0], StoreError::StaleBase { actual, .. } if *actual == winner.id),
            "{:?}",
            losers[0]
        );
        let store = db.store();
        assert_eq!(
            store.head(&branch("main")).unwrap(),
            Some(winner.id.clone())
        );
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 2);
        // Only the winning Patch artifact exists.
        assert_eq!(db.count("SELECT COUNT(*) FROM artifacts"), 1);
        let winning_ps = &results.iter().find(|(_, r)| r.is_ok()).unwrap().0;
        assert_eq!(
            winner.accepted_patch_ref,
            Some(Hash::content_sha256(
                &to_canonical_json(winning_ps).unwrap()
            ))
        );
        store.load_revision(&r1.id).unwrap();
        store.load_revision(&winner.id).unwrap();
    }

    #[test]
    fn failure_after_patch_artifact_insert_rolls_back_everything() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        let ps = add_accepted(&base_graph(), "req:x");
        let patch_ref = Hash::content_sha256(&to_canonical_json(&ps).unwrap());
        db.exec(
            "CREATE TRIGGER fail_revision BEFORE INSERT ON graph_revisions \
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        );
        let err = store
            .commit(&branch("main"), &r1.id, &ps, meta(T2))
            .unwrap_err();
        assert!(matches!(err, StoreError::Sqlite(_)), "{err:?}");
        db.exec("DROP TRIGGER fail_revision;");
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 1);
        assert_eq!(
            db.count("SELECT COUNT(*) FROM revision_nodes WHERE revision_id <> (SELECT id FROM graph_revisions)"),
            0
        );
        assert_eq!(
            db.count("SELECT COUNT(*) FROM revision_edges WHERE revision_id <> (SELECT id FROM graph_revisions)"),
            0
        );
        assert_eq!(store.head(&branch("main")).unwrap(), Some(r1.id.clone()));
        assert!(!SqliteArtifactStore::open(&db.path)
            .unwrap()
            .exists(&patch_ref)
            .unwrap());

        // A snapshot failure after the revision row is written also rolls back the artifact.
        db.exec(
            "CREATE TRIGGER fail_edges BEFORE INSERT ON revision_edges \
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        );
        assert!(store
            .commit(&branch("main"), &r1.id, &ps, meta(T2))
            .is_err());
        db.exec("DROP TRIGGER fail_edges;");
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 1);
        assert_eq!(db.count("SELECT COUNT(*) FROM artifacts"), 0);

        // The store remains usable and the same commit now succeeds.
        let r2 = store
            .commit(&branch("main"), &r1.id, &ps, meta(T2))
            .unwrap();
        assert_eq!(r2.version, 2);
        assert_eq!(r2.accepted_patch_ref, Some(patch_ref));
    }

    #[test]
    fn artifact_errors_are_typed_and_roll_back() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        let ps = add_accepted(&base_graph(), "req:x");
        let bytes = to_canonical_json(&ps).unwrap();
        SqliteArtifactStore::open(&db.path)
            .unwrap()
            .put(ArtifactKind::Proposal, JSON, &bytes, ts(T1))
            .unwrap();
        let err = store
            .commit(&branch("main"), &r1.id, &ps, meta(T2))
            .unwrap_err();
        assert!(
            matches!(
                err,
                StoreError::Artifact(
                    plumb_artifacts::ArtifactStoreError::ArtifactMetadataConflict { .. }
                )
            ),
            "{err:?}"
        );
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), 1);
        assert_eq!(store.head(&branch("main")).unwrap(), Some(r1.id));
    }

    #[test]
    fn version_overflow_is_explicit() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        db.exec(&format!(
            "INSERT INTO graph_revisions SELECT 'rev:{max}:0000000000000000', {max}, NULL, project_id, \
             psg_schema_version, semantic_hash, evidence_hash, profile_ref, profile_hash, \
             rule_pack_hash, NULL, decision_refs_json, created_by, created_at FROM graph_revisions;",
            max = i64::MAX
        ));
        let ps = add_accepted(&base_graph(), "req:x");
        assert!(matches!(
            store
                .commit(&branch("main"), &r1.id, &ps, meta(T2))
                .unwrap_err(),
            StoreError::VersionOverflow
        ));
        assert_eq!(db.count("SELECT COUNT(*) FROM artifacts"), 0);
    }

    // ------------------------------------------------------------------ restore

    #[test]
    fn move_head_restores_without_deleting_history() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        let r2 = store
            .commit(
                &branch("main"),
                &r1.id,
                &add_accepted(&base_graph(), "req:x"),
                meta(T2),
            )
            .unwrap();
        let revisions = db.count("SELECT COUNT(*) FROM graph_revisions");
        let nodes = db.count("SELECT COUNT(*) FROM revision_nodes");

        let err = store
            .move_head(&branch("main"), &r1.id, &r1.id, ts(T3))
            .unwrap_err();
        assert!(matches!(err, StoreError::StaleBase { actual, .. } if actual == r2.id));

        store
            .move_head(&branch("main"), &r2.id, &r1.id, ts(T3))
            .unwrap();
        assert_eq!(store.head(&branch("main")).unwrap(), Some(r1.id.clone()));
        let updated_at: String = db
            .raw()
            .query_row(
                "SELECT updated_at FROM branch_heads WHERE name = 'main'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(updated_at, T3);
        assert_eq!(db.count("SELECT COUNT(*) FROM graph_revisions"), revisions);
        assert_eq!(db.count("SELECT COUNT(*) FROM revision_nodes"), nodes);
        store.load_revision(&r1.id).unwrap();
        store.load_revision(&r2.id).unwrap();
        assert!(SqliteArtifactStore::open(&db.path)
            .unwrap()
            .exists(r2.accepted_patch_ref.as_ref().unwrap())
            .unwrap());

        // Moving forward again (sideways to any existing revision) is also pointer movement.
        store
            .move_head(&branch("main"), &r1.id, &r2.id, ts(T4))
            .unwrap();
        assert_eq!(store.head(&branch("main")).unwrap(), Some(r2.id));
    }

    // ------------------------------------------------------------------ revisions and hashes

    #[test]
    fn same_semantic_hash_revisions_are_distinct() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        let ps = add_proposed(&base_graph(), "req:draft");
        let r2 = store
            .commit(&branch("main"), &r1.id, &ps, meta(T2))
            .unwrap();
        assert_eq!(r2.semantic_hash, r1.semantic_hash);
        assert_ne!(r2.id, r1.id);
        assert_eq!(r2.version, 2);
        let loaded = store.load_revision(&r2.id).unwrap();
        assert!(loaded.graph.node(&id("req:draft")).is_some());
        // A head CAS still distinguishes the two revisions.
        assert!(matches!(
            store
                .commit(
                    &branch("main"),
                    &r1.id,
                    &add_proposed(&base_graph(), "req:z"),
                    meta(T3)
                )
                .unwrap_err(),
            StoreError::StaleBase { .. }
        ));
    }

    #[test]
    fn element_revisions_are_persisted_exactly_as_patch_application_produced() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        let base = base_graph();
        let req_a = base.node(&id("req:a")).unwrap();
        let payload: NodePayload = from_json(json!({
            "type": "Requirement",
            "data": requirement_data("Employees shall submit leave requests online.")
        }));
        let ps = patch_set(
            &base,
            SemanticPatch::Compound {
                patches: vec![
                    SemanticPatch::ReplacePayload {
                        target: plumb_patch::ElementPrecondition {
                            id: id("req:a"),
                            expected_hash: node_element_hash(req_a).unwrap(),
                        },
                        payload,
                    },
                    SemanticPatch::AddNode {
                        node: requirement("req:new", "Accepted", "The system shall log requests."),
                    },
                ],
            },
        );
        let expected = plumb_patch::apply_patch(&base, &ps).unwrap().graph;
        let r2 = store
            .commit(&branch("main"), &r1.id, &ps, meta(T2))
            .unwrap();
        let loaded = store.load_revision(&r2.id).unwrap().graph;
        assert_eq!(loaded, expected);
        let revision_of = |g: &Graph, n: &str| g.node(&id(n)).unwrap().revision;
        assert_eq!(revision_of(&loaded, "req:a"), 2);
        assert_eq!(revision_of(&loaded, "req:b"), 1);
        assert_eq!(revision_of(&loaded, "req:new"), 1);
        assert_eq!(loaded.edge(&id("rel:a-eu")).unwrap().revision, 1);
        let stored: String = db
            .raw()
            .query_row(
                "SELECT node_json FROM revision_nodes WHERE revision_id = ?1 AND node_id = 'req:a'",
                [r2.id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&stored).unwrap()["revision"],
            json!(2)
        );
        // Revision 1 is untouched.
        assert_eq!(
            revision_of(&store.load_revision(&r1.id).unwrap().graph, "req:a"),
            1
        );
    }

    #[test]
    fn reopen_preserves_heads_revisions_and_schema() {
        let db = Db::new();
        let (mut store, r1) = db.initialized();
        let other = branch("candidate/team-A");
        store
            .create_branch(other.clone(), r1.id.clone(), ts(T1))
            .unwrap();
        let r2 = store
            .commit(
                &branch("main"),
                &r1.id,
                &add_accepted(&base_graph(), "req:x"),
                meta(T2),
            )
            .unwrap();
        let r3 = store
            .commit(
                &other,
                &r1.id,
                &add_proposed(&base_graph(), "req:y"),
                meta(T3),
            )
            .unwrap();
        let loaded: Vec<LoadedRevision> = [&r1, &r2, &r3]
            .iter()
            .map(|r| store.load_revision(&r.id).unwrap())
            .collect();
        drop(store);
        let schema = schema_sql(&db.path);
        let heads = db.count("SELECT COUNT(*) FROM branch_heads");

        let store = db.store();
        assert_eq!(schema_sql(&db.path), schema);
        assert_eq!(schema_meta_rows(&db.path), vec![1]);
        assert_eq!(db.count("SELECT COUNT(*) FROM branch_heads"), heads);
        assert_eq!(store.head(&branch("main")).unwrap(), Some(r2.id.clone()));
        assert_eq!(store.head(&other).unwrap(), Some(r3.id.clone()));
        let artifacts = SqliteArtifactStore::open(&db.path).unwrap();
        for before in loaded {
            let after = store.load_revision(&before.revision.id).unwrap();
            assert_eq!(after, before);
            assert_eq!(
                after.graph.semantic_hash().unwrap(),
                after.revision.semantic_hash
            );
            assert_eq!(
                after.graph.evidence_hash().unwrap(),
                after.revision.evidence_hash
            );
            if let Some(patch_ref) = &after.revision.accepted_patch_ref {
                assert_eq!(artifacts.get(patch_ref).unwrap().kind, ArtifactKind::Patch);
            }
        }
    }

    // ------------------------------------------------------------------ corruption

    /// A store with revisions 1 and 2 on main.
    fn committed(db: &Db) -> (SqliteRevisionStore, GraphRevision, GraphRevision) {
        let (mut store, r1) = db.initialized();
        let r2 = store
            .commit(
                &branch("main"),
                &r1.id,
                &add_accepted(&base_graph(), "req:x"),
                meta(T2),
            )
            .unwrap();
        (store, r1, r2)
    }

    #[test]
    fn tampered_hashes_and_ids_are_corruption() {
        let db = Db::new();
        let (store, r1, _) = committed(&db);
        let original_evidence = r1.evidence_hash.as_str().to_owned();
        db.exec(&format!(
            "UPDATE graph_revisions SET evidence_hash = 'ev:sha256:{}' WHERE id = '{}';",
            "0".repeat(64),
            r1.id
        ));
        assert!(corrupt_reason(store.load_revision(&r1.id).unwrap_err()).contains("evidence_hash"));
        db.exec(&format!(
            "UPDATE graph_revisions SET evidence_hash = '{original_evidence}' WHERE id = '{}';",
            r1.id
        ));
        store.load_revision(&r1.id).unwrap();

        db.exec(&format!(
            "UPDATE graph_revisions SET semantic_hash = 'psg:sha256:{}' WHERE id = '{}';",
            "0".repeat(64),
            r1.id
        ));
        assert!(corrupt_reason(store.load_revision(&r1.id).unwrap_err()).contains("derived id"));
        db.exec(&format!(
            "UPDATE graph_revisions SET semantic_hash = '{}' WHERE id = '{}';",
            r1.semantic_hash, r1.id
        ));

        // Tampering a snapshot element so it no longer matches the stored semantic hash.
        let tampered = requirement("req:a", "Accepted", "Employees shall fax leave requests.");
        let text = String::from_utf8(to_canonical_json(&tampered).unwrap()).unwrap();
        db.raw()
            .execute(
                "UPDATE revision_nodes SET node_json = ?1 WHERE revision_id = ?2 AND node_id = 'req:a'",
                [text.as_str(), r1.id.as_str()],
            )
            .unwrap();
        assert!(corrupt_reason(store.load_revision(&r1.id).unwrap_err()).contains("semantic_hash"));

        // A stored ID that does not match version + semantic hash.
        let db = Db::new();
        let (store, r1) = db.initialized();
        drop(store);
        let wrong = "rev:1:ffffffffffffffff";
        db.exec(&format!(
            "PRAGMA foreign_keys = OFF; UPDATE graph_revisions SET id = '{wrong}' WHERE id = '{}';",
            r1.id
        ));
        let store = db.store();
        assert!(
            corrupt_reason(store.load_revision(&wrong.parse().unwrap()).unwrap_err())
                .contains("derived id")
        );
        assert!(matches!(
            store.load_revision(&r1.id).unwrap_err(),
            StoreError::RevisionNotFound(_)
        ));
    }

    #[test]
    fn malformed_snapshot_rows_and_metadata_are_corruption() {
        let db = Db::new();
        let (store, r1, _) = committed(&db);
        let cases = [
            (
                "UPDATE revision_nodes SET node_json = '{\"id\":' WHERE node_id = 'req:a'",
                "element req:a",
            ),
            (
                "UPDATE revision_edges SET edge_json = '{\"kind\":1}' WHERE edge_id = 'rel:a-eu'",
                "element rel:a-eu",
            ),
            (
                "UPDATE graph_revisions SET created_at = '2026-09-29T08:00:00Z'",
                "created_at",
            ),
            (
                "UPDATE graph_revisions SET created_by = 'Bad Id'",
                "created_by",
            ),
            ("UPDATE graph_revisions SET project_id = ''", "project_id"),
            (
                "UPDATE graph_revisions SET decision_refs_json = '[\"dec:2\",\"dec:1\"]'",
                "sorted",
            ),
            (
                "UPDATE graph_revisions SET decision_refs_json = '{}'",
                "decision_refs_json",
            ),
            (
                &format!(
                    "UPDATE graph_revisions SET profile_hash = 'psg:sha256:{}'",
                    "a".repeat(64)
                ),
                "profile_hash",
            ),
        ];
        for (sql, expected) in cases {
            let backup = TempDir::new().unwrap();
            let copy = backup.path().join("copy.sqlite");
            db.raw()
                .execute("VACUUM INTO ?1", [copy.to_str().unwrap()])
                .unwrap();
            let conn = Connection::open(&copy).unwrap();
            conn.execute_batch(sql).unwrap();
            drop(conn);
            let tampered = SqliteRevisionStore::open(&copy).unwrap();
            let reason = corrupt_reason(tampered.load_revision(&r1.id).unwrap_err());
            assert!(reason.contains(expected), "{sql}: {reason}");
        }
        store.load_revision(&r1.id).unwrap();
    }

    #[test]
    fn unsupported_psg_schema_version_is_rejected_before_graph_construction() {
        let db = Db::new();
        let (store, r1) = db.initialized();
        // Snapshot rows that no longer validate under the current PSG contract.
        db.exec(&format!(
            "UPDATE graph_revisions SET psg_schema_version = 2 WHERE id = '{}'; \
             UPDATE revision_nodes SET node_json = 'not json' WHERE revision_id = '{}';",
            r1.id, r1.id
        ));
        assert!(matches!(
            store.load_revision(&r1.id).unwrap_err(),
            StoreError::UnsupportedPsgSchemaVersion { found: 2, .. }
        ));
    }

    #[test]
    fn patch_artifact_corruption_is_detected() {
        let db = Db::new();
        let (store, _, r2) = committed(&db);
        let patch_ref = r2.accepted_patch_ref.clone().unwrap();
        let cases: Vec<(String, &str)> = vec![
            (
                format!("PRAGMA foreign_keys = OFF; DELETE FROM artifacts WHERE hash = '{patch_ref}';"),
                "missing",
            ),
            (
                format!("UPDATE artifacts SET kind = 'proposal' WHERE hash = '{patch_ref}';"),
                "proposal",
            ),
            (
                format!("UPDATE artifacts SET media_type = 'text/plain' WHERE hash = '{patch_ref}';"),
                "text/plain",
            ),
            (
                format!("UPDATE artifacts SET bytes = X'7b7d' WHERE hash = '{patch_ref}';"),
                "bytes do not match",
            ),
            (
                format!(
                    "PRAGMA foreign_keys = OFF; UPDATE graph_revisions SET patch_artifact_hash = NULL WHERE id = '{}';",
                    r2.id
                ),
                "committed revisions need",
            ),
        ];
        for (sql, expected) in cases {
            let backup = TempDir::new().unwrap();
            let copy = backup.path().join("copy.sqlite");
            db.raw()
                .execute("VACUUM INTO ?1", [copy.to_str().unwrap()])
                .unwrap();
            Connection::open(&copy)
                .unwrap()
                .execute_batch(&sql)
                .unwrap();
            let tampered = SqliteRevisionStore::open(&copy).unwrap();
            let reason = corrupt_reason(tampered.load_revision(&r2.id).unwrap_err());
            assert!(reason.contains(expected), "{sql}: {reason}");
        }

        // A patch artifact whose bytes are a valid PatchSet for a different base.
        let backup = TempDir::new().unwrap();
        let copy = backup.path().join("copy.sqlite");
        db.raw()
            .execute("VACUUM INTO ?1", [copy.to_str().unwrap()])
            .unwrap();
        let foreign = add_accepted(
            &graph_in(PROJECT, vec![constraint("con:x")], vec![]),
            "req:x",
        );
        let bytes = to_canonical_json(&foreign).unwrap();
        let foreign_ref = Hash::content_sha256(&bytes);
        let conn = Connection::open(&copy).unwrap();
        conn.execute(
            "INSERT INTO artifacts (hash, kind, media_type, bytes, created_at) VALUES (?1, 'patch', ?2, ?3, ?4)",
            rusqlite::params![foreign_ref.as_str(), JSON, bytes, T2],
        )
        .unwrap();
        conn.execute(
            "UPDATE graph_revisions SET patch_artifact_hash = ?1 WHERE id = ?2",
            [foreign_ref.as_str(), r2.id.as_str()],
        )
        .unwrap();
        drop(conn);
        let tampered = SqliteRevisionStore::open(&copy).unwrap();
        assert!(corrupt_reason(tampered.load_revision(&r2.id).unwrap_err())
            .contains("base_semantic_hash"));

        // A non-generic stored patch ref is a typed hash-kind error.
        let backup = TempDir::new().unwrap();
        let copy = backup.path().join("copy.sqlite");
        db.raw()
            .execute("VACUUM INTO ?1", [copy.to_str().unwrap()])
            .unwrap();
        let conn = Connection::open(&copy).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        conn.execute(
            "UPDATE graph_revisions SET patch_artifact_hash = ?1 WHERE id = ?2",
            [
                format!("ev:sha256:{}", "a".repeat(64)).as_str(),
                r2.id.as_str(),
            ],
        )
        .unwrap();
        let tampered = SqliteRevisionStore::open(&copy).unwrap();
        assert!(matches!(
            tampered.load_revision(&r2.id).unwrap_err(),
            StoreError::InvalidPatchArtifactHashKind { .. }
        ));
        store.load_revision(&r2.id).unwrap();
        assert_eq!(patch_ref.kind(), HashKind::Generic);
    }
}
