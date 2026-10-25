//! A0.4 (Hotfix 067) contract tests for the store schema v3 stage-orchestration records.
//!
//! Every test lives in `stages` so that `cargo test -p plumb-store stages` selects all of them.

mod stages {
    use std::path::PathBuf;

    use plumb_artifacts::{ArtifactKind, ArtifactStore, SqliteArtifactStore};
    use plumb_core::{Hash, Id, Timestamp};
    use plumb_patch::{PatchSet, SemanticPatch};
    use plumb_psg::*;
    use plumb_store::*;
    use rusqlite::Connection;
    use serde_json::{json, Value};
    use tempfile::TempDir;

    const T1: &str = "2026-09-29T08:00:00.000000000Z";
    const T2: &str = "2026-09-29T09:00:00.000000000Z";
    const T3: &str = "2026-09-29T10:00:00.000000000Z";

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
        r1: RevisionId,
        r2: RevisionId,
        run: Hash,
    }

    impl Db {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("plumb.sqlite");
            let mut store = SqliteRevisionStore::open(&path).unwrap();
            let g = Graph::new(
                id("project:leave-management"),
                id("profile:plumb-software-2026.1"),
                vec![requirement("req:a", "Accepted")],
                vec![],
            )
            .unwrap();
            let r1 = store
                .create_initial_revision(&g, generic("a"), generic("b"), meta(T1))
                .unwrap()
                .id;
            let r2 = store
                .commit(
                    &BranchName::main(),
                    &r1,
                    &PatchSet {
                        base_semantic_hash: g.semantic_hash().unwrap(),
                        patch: SemanticPatch::AddNode {
                            node: requirement("req:b", "Proposed"),
                        },
                    },
                    meta(T2),
                )
                .unwrap()
                .revision
                .id;
            let run = SqliteArtifactStore::open(&path)
                .unwrap()
                .put(ArtifactKind::CompileRun, "application/json", b"{}", ts(T1))
                .unwrap();
            Db {
                _dir: dir,
                path,
                r1,
                r2,
                run,
            }
        }

        fn store(&self) -> SqliteStageStore {
            SqliteStageStore::open(&self.path).unwrap()
        }

        fn raw(&self) -> Connection {
            let conn = Connection::open(&self.path).unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
            conn
        }

        fn exec_err(&self, sql: &str) -> bool {
            self.raw().execute_batch(sql).is_err()
        }

        fn recipe(&self, job: &str) -> NewStageRecipeJob {
            NewStageRecipeJob {
                recipe_job_id: generic(job),
                // Opaque material: canonicalized, never interpreted.
                recipe_job_json: json!({"z": 1, "recipe": "extract", "a": [2, 1]}),
                config_json: json!({"version": 1, "inputs": {}}),
                recipe: "extract".to_owned(),
                branch: BranchName::main(),
                base_revision_id: self.r1.clone(),
                created_at: ts(T1),
                lease: None,
            }
        }

        fn stage_proposal(&self, proposal_id: &str) -> NewWorkflowProposal {
            NewWorkflowProposal {
                proposal_id: id(proposal_id),
                base_revision_id: self.r1.clone(),
                proposal_json: json!({"id": proposal_id}),
                intake_json: json!({"requires_reason": false}),
                proposal_kind: WorkflowProposalKind::Create,
                origin_kind: WorkflowOrigin::Stage,
                origin_ref: None,
                created_at: ts(T1),
            }
        }
    }

    fn inserted<T: std::fmt::Debug>(r: WorkflowInsert<T>) -> T {
        match r {
            WorkflowInsert::Inserted(t) => t,
            other => panic!("{other:?}"),
        }
    }

    // ------------------------------------------------------------------ recipe jobs

    #[test]
    fn stages_recipe_jobs_are_canonical_unique_and_durable() {
        let db = Db::new();
        let mut store = db.store();
        let job = db.recipe("1");
        let stored = inserted(store.insert_recipe_job(&job).unwrap());
        assert_eq!(stored.status, StageStatus::Queued);
        assert_eq!(
            stored.recipe_job_json,
            r#"{"a":[2,1],"recipe":"extract","z":1}"#
        );
        assert_eq!(stored.result_revision_id, None);
        assert_eq!(stored.failure_code, None);
        // The same ID returns the stored job unchanged, whatever the new material says.
        let mut again = db.recipe("1");
        again.recipe_job_json = json!({"other": true});
        assert!(matches!(
            store.insert_recipe_job(&again).unwrap(),
            WorkflowInsert::Existing(e) if e == stored
        ));
        // Durable across a reopen.
        drop(store);
        assert_eq!(db.store().recipe_job(&generic("1")).unwrap(), Some(stored));
        // An unknown base revision is rejected.
        let mut orphan = db.recipe("2");
        orphan.base_revision_id = "rev:9:0000000000000000".parse().unwrap();
        assert!(db.store().insert_recipe_job(&orphan).is_err());
        assert_eq!(db.store().recipe_job(&generic("2")).unwrap(), None);
    }

    #[test]
    fn stages_recipe_transitions_are_conditional_and_checked() {
        let db = Db::new();
        let mut store = db.store();
        inserted(store.insert_recipe_job(&db.recipe("1")).unwrap());
        // Claim happens exactly once.
        assert!(store.claim_recipe_job(&generic("1"), ts(T2)).unwrap());
        assert!(!store.claim_recipe_job(&generic("1"), ts(T2)).unwrap());
        let job = store.recipe_job(&generic("1")).unwrap().unwrap();
        assert_eq!(job.status, StageStatus::Acquiring);
        assert_eq!(job.updated_at, ts(T2));
        // A transition from another state is a conflict and changes nothing.
        assert!(matches!(
            store.transition_recipe_job(
                &generic("1"),
                &[StageStatus::Queued],
                StageStatus::Evaluating,
                None,
                None,
                ts(T3)
            ),
            Err(StoreError::WorkflowStateConflict { .. })
        ));
        store
            .transition_recipe_job(
                &generic("1"),
                &[StageStatus::Acquiring],
                StageStatus::Evaluating,
                None,
                None,
                ts(T3),
            )
            .unwrap();
        // Ready requires a result revision; failed requires a code and message.
        for (to, result, failure) in [
            (StageStatus::Ready, None, None),
            (StageStatus::Committed, None, None),
            (StageStatus::Failed, None, None),
            (StageStatus::Stale, Some(&db.r1), None),
            (StageStatus::Ready, Some(&db.r1), Some(("E_X", "x"))),
        ] {
            assert!(
                store
                    .transition_recipe_job(
                        &generic("1"),
                        &[StageStatus::Evaluating],
                        to,
                        result,
                        failure,
                        ts(T3)
                    )
                    .is_err(),
                "{to}"
            );
        }
        let ready = store
            .transition_recipe_job(
                &generic("1"),
                &[StageStatus::Evaluating],
                StageStatus::Ready,
                Some(&db.r2),
                None,
                ts(T3),
            )
            .unwrap();
        assert_eq!(ready.result_revision_id, Some(db.r2.clone()));
        // Raw CHECKs hold as well.
        assert!(db
            .exec_err("UPDATE stage_recipe_jobs SET status = 'failed', result_revision_id = NULL"));
        assert!(db.exec_err("UPDATE stage_recipe_jobs SET recipe = 'ready'"));
        assert!(db.exec_err("UPDATE stage_recipe_jobs SET status = 'done'"));
    }

    #[test]
    fn stages_restart_sweep_fails_in_flight_parents_only() {
        let db = Db::new();
        let mut store = db.store();
        for job in ["1", "2", "3", "4"] {
            inserted(store.insert_recipe_job(&db.recipe(job)).unwrap());
        }
        assert!(store.claim_recipe_job(&generic("2"), ts(T2)).unwrap());
        assert!(store.claim_recipe_job(&generic("3"), ts(T2)).unwrap());
        store
            .transition_recipe_job(
                &generic("3"),
                &[StageStatus::Acquiring],
                StageStatus::Ready,
                Some(&db.r1),
                None,
                ts(T2),
            )
            .unwrap();
        assert!(store.claim_recipe_job(&generic("4"), ts(T2)).unwrap());
        store
            .transition_recipe_job(
                &generic("4"),
                &[StageStatus::Acquiring],
                StageStatus::Evaluating,
                None,
                None,
                ts(T2),
            )
            .unwrap();
        drop(store);
        let mut store = db.store();
        let failed = store
            .interrupt_recipe_jobs("E_JOB_INTERRUPTED", "interrupted", ts(T3))
            .unwrap();
        assert_eq!(failed, vec![generic("1"), generic("2"), generic("4")]);
        for job in ["1", "2", "4"] {
            let r = store.recipe_job(&generic(job)).unwrap().unwrap();
            assert_eq!(r.status, StageStatus::Failed);
            assert_eq!(r.failure_code.as_deref(), Some("E_JOB_INTERRUPTED"));
        }
        assert_eq!(
            store.recipe_job(&generic("3")).unwrap().unwrap().status,
            StageStatus::Ready
        );
        // Failed is never revived by a claim.
        assert!(!store.claim_recipe_job(&generic("1"), ts(T3)).unwrap());
    }

    /// H0.3 (§33.3): only a Failed recipe job is re-queued for a new attempt under the same ID;
    /// its failure, result and the steps of the failed attempt are cleared atomically.
    #[test]
    fn stages_retry_requeues_only_failed_jobs() {
        let db = Db::new();
        let mut store = db.store();
        inserted(store.insert_recipe_job(&db.recipe("1")).unwrap());
        assert!(store.claim_recipe_job(&generic("1"), ts(T2)).unwrap());
        store
            .insert_step(&NewStageStep {
                recipe_job_id: generic("1"),
                ordinal: 0,
                phase: "tags".to_owned(),
                compiler_stage: "S1".to_owned(),
                input_revision_id: db.r1.clone(),
            })
            .unwrap();
        store
            .interrupt_recipe_jobs("E_JOB_INTERRUPTED", "interrupted", ts(T2))
            .unwrap();
        inserted(store.insert_recipe_job(&db.recipe("2")).unwrap());
        let retried = store.retry_recipe_job(&generic("1"), ts(T3)).unwrap();
        assert_eq!(retried.status, StageStatus::Queued);
        assert_eq!(retried.failure_code, None);
        assert_eq!(retried.failure_message, None);
        assert_eq!(retried.result_revision_id, None);
        assert_eq!(retried.updated_at, ts(T3));
        assert!(store.steps(&generic("1")).unwrap().is_empty());
        assert!(store.claim_recipe_job(&generic("1"), ts(T3)).unwrap());
        // Anything but Failed is a conflict and changes nothing.
        for job in ["1", "2"] {
            let before = store.recipe_job(&generic(job)).unwrap();
            assert!(matches!(
                store.retry_recipe_job(&generic(job), ts(T3)),
                Err(StoreError::WorkflowStateConflict { .. })
            ));
            assert_eq!(store.recipe_job(&generic(job)).unwrap(), before);
        }
        assert!(store.retry_recipe_job(&generic("9"), ts(T3)).is_err());
    }

    // ------------------------------------------------------------------ attempts and leases

    fn lease(owner: &str, at: &str) -> StageLease {
        StageLease {
            owner_id: owner.to_owned(),
            expires_at: ts(at),
        }
    }

    /// H0.3 (§33.3): a v5 store records every attempt of a recipe job under its unchanged ID,
    /// with the owner that claimed it and how it ended; a claim leases the job to its owner.
    #[test]
    fn stages_attempts_are_durable_and_leased() {
        let db = Db::new();
        let mut store = db.store();
        assert_eq!(STORE_SCHEMA_VERSION, 7);
        let mut job = db.recipe("1");
        job.lease = Some(lease("own:aaaaaaaaaaaaaaaa", T2));
        let queued = inserted(store.insert_recipe_job(&job).unwrap());
        assert_eq!(queued.attempt, 1);
        assert_eq!(queued.owner_id.as_deref(), Some("own:aaaaaaaaaaaaaaaa"));
        assert_eq!(queued.lease_expires_at, Some(ts(T2)));
        assert!(store.recipe_job_attempts(&generic("1")).unwrap().is_empty());
        // A claim leases the job to its claimer and starts attempt 1.
        assert!(store
            .claim_recipe_job_leased(&generic("1"), &lease("own:bbbbbbbbbbbbbbbb", T3), ts(T1))
            .unwrap());
        let claimed = store.recipe_job(&generic("1")).unwrap().unwrap();
        assert_eq!(claimed.owner_id.as_deref(), Some("own:bbbbbbbbbbbbbbbb"));
        assert_eq!(claimed.lease_expires_at, Some(ts(T3)));
        // Renewal touches only the owner's in-flight jobs.
        assert_eq!(
            store.renew_leases("own:cccccccccccccccc", ts(T3)).unwrap(),
            0
        );
        let renewed = "2026-09-29T11:00:00.000000000Z";
        assert_eq!(
            store
                .renew_leases("own:bbbbbbbbbbbbbbbb", ts(renewed))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .recipe_job(&generic("1"))
                .unwrap()
                .unwrap()
                .lease_expires_at,
            Some(ts(renewed))
        );
        let failed = store
            .transition_recipe_job(
                &generic("1"),
                &[StageStatus::Acquiring],
                StageStatus::Failed,
                None,
                Some(("E_INFERENCE_FAILED", "provider down")),
                ts(T2),
            )
            .unwrap();
        // A terminal job holds no lease; its last owner stays recorded.
        assert_eq!(failed.lease_expires_at, None);
        assert_eq!(failed.owner_id.as_deref(), Some("own:bbbbbbbbbbbbbbbb"));
        assert_eq!(
            store
                .renew_leases("own:bbbbbbbbbbbbbbbb", ts(renewed))
                .unwrap(),
            0
        );
        let first = StageAttemptRecord {
            recipe_job_id: generic("1"),
            attempt: 1,
            owner_id: Some("own:bbbbbbbbbbbbbbbb".to_owned()),
            started_at: ts(T1),
            ended_at: Some(ts(T2)),
            outcome: Some(StageStatus::Failed),
            error: Some(r#"{"code":"E_INFERENCE_FAILED","message":"provider down"}"#.to_owned()),
        };
        assert_eq!(
            store.recipe_job_attempts(&generic("1")).unwrap(),
            vec![first.clone()]
        );
        // A retry is attempt 2 under the same ID; attempt 1 is kept.
        let retried = store
            .retry_recipe_job_leased(
                &generic("1"),
                Some(&lease("own:dddddddddddddddd", T3)),
                ts(T2),
            )
            .unwrap();
        assert_eq!(retried.attempt, 2);
        assert_eq!(retried.owner_id.as_deref(), Some("own:dddddddddddddddd"));
        assert!(store
            .claim_recipe_job_leased(&generic("1"), &lease("own:dddddddddddddddd", T3), ts(T3))
            .unwrap());
        store
            .transition_recipe_job(
                &generic("1"),
                &[StageStatus::Acquiring],
                StageStatus::Ready,
                Some(&db.r1),
                None,
                ts(T3),
            )
            .unwrap();
        // Later transitions of a settled job do not rewrite its attempt.
        store
            .transition_recipe_job(
                &generic("1"),
                &[StageStatus::Ready],
                StageStatus::Committed,
                Some(&db.r2),
                None,
                ts(T3),
            )
            .unwrap();
        drop(store);
        let attempts = db.store().recipe_job_attempts(&generic("1")).unwrap();
        assert_eq!(
            attempts,
            vec![
                first,
                StageAttemptRecord {
                    recipe_job_id: generic("1"),
                    attempt: 2,
                    owner_id: Some("own:dddddddddddddddd".to_owned()),
                    started_at: ts(T3),
                    ended_at: Some(ts(T3)),
                    outcome: Some(StageStatus::Ready),
                    error: None,
                }
            ]
        );
        // Raw CHECKs: attempts start at 1; an attempt that ended has an outcome.
        assert!(db.exec_err("UPDATE stage_recipe_jobs SET attempt = 0"));
        assert!(db.exec_err("UPDATE stage_recipe_job_attempts SET outcome = NULL"));
        assert!(db.exec_err("UPDATE stage_recipe_job_attempts SET outcome = 'queued'"));
    }

    /// H0.3 (§33.3): startup recovery may interrupt only in-flight jobs whose lease expired or
    /// that have no owner; an interruption ends the job's current attempt.
    #[test]
    fn stages_only_expired_or_ownerless_jobs_are_abandoned() {
        let db = Db::new();
        let mut store = db.store();
        for (job, owner) in [
            ("1", None),
            ("2", Some(lease("own:aaaaaaaaaaaaaaaa", T1))),
            ("3", Some(lease("own:aaaaaaaaaaaaaaaa", T3))),
            ("4", Some(lease("own:aaaaaaaaaaaaaaaa", T1))),
        ] {
            let mut new = db.recipe(job);
            new.lease = owner;
            inserted(store.insert_recipe_job(&new).unwrap());
        }
        assert!(store
            .claim_recipe_job_leased(&generic("4"), &lease("own:aaaaaaaaaaaaaaaa", T1), ts(T1))
            .unwrap());
        // A terminal job is never abandoned, whatever its lease.
        inserted(store.insert_recipe_job(&db.recipe("5")).unwrap());
        store
            .interrupt_recipe_job(&generic("5"), "E_X", "x", ts(T1))
            .unwrap();
        assert_eq!(
            store.abandoned_recipe_jobs(ts(T2)).unwrap(),
            vec![generic("1"), generic("2"), generic("4")]
        );
        // Interrupting is conditional on the job still being in flight.
        assert!(store
            .interrupt_recipe_job(&generic("4"), "E_JOB_INTERRUPTED", "lease expired", ts(T2))
            .unwrap());
        assert!(!store
            .interrupt_recipe_job(&generic("4"), "E_JOB_INTERRUPTED", "again", ts(T3))
            .unwrap());
        let record = store.recipe_job(&generic("4")).unwrap().unwrap();
        assert_eq!(record.status, StageStatus::Failed);
        assert_eq!(record.failure_code.as_deref(), Some("E_JOB_INTERRUPTED"));
        let attempts = store.recipe_job_attempts(&generic("4")).unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].started_at, ts(T1));
        assert_eq!(attempts[0].ended_at, Some(ts(T2)));
        assert_eq!(attempts[0].outcome, Some(StageStatus::Failed));
        // A queued job that never started still records its interrupted attempt.
        store
            .interrupt_recipe_job(&generic("1"), "E_JOB_INTERRUPTED", "no owner", ts(T2))
            .unwrap();
        let attempts = store.recipe_job_attempts(&generic("1")).unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            (attempts[0].started_at, attempts[0].ended_at),
            (ts(T2), Some(ts(T2)))
        );
        assert_eq!(
            store.abandoned_recipe_jobs(ts(T2)).unwrap(),
            vec![generic("2")]
        );
        // An unreadable lease counts as expired (per-row fault tolerance).
        db.raw()
            .execute(
                "UPDATE stage_recipe_jobs SET lease_expires_at = 'garbage' \
                 WHERE recipe_job_id = ?1",
                [generic("3").as_str()],
            )
            .unwrap();
        assert_eq!(
            store.abandoned_recipe_jobs(ts(T2)).unwrap(),
            vec![generic("2"), generic("3")]
        );
    }

    /// H0.3 (§33.3): a v4 store (pre H0.11) and a v5 store (H0.11 `abandoned`, pre H0.3)
    /// migrate in place to v6; every recipe job keeps its row as attempt 1 and each started
    /// job gets its attempt record.
    #[test]
    fn stages_v4_and_v5_stores_migrate_to_v6_attempts() {
        for from in [4, 5] {
            let db = Db::new();
            let mut store = db.store();
            for job in ["1", "2", "3"] {
                inserted(store.insert_recipe_job(&db.recipe(job)).unwrap());
            }
            assert!(store.claim_recipe_job(&generic("2"), ts(T2)).unwrap());
            assert!(store.claim_recipe_job(&generic("3"), ts(T2)).unwrap());
            store
                .transition_recipe_job(
                    &generic("3"),
                    &[StageStatus::Acquiring],
                    StageStatus::Failed,
                    None,
                    Some(("E_INFERENCE_FAILED", "provider \"down\"")),
                    ts(T3),
                )
                .unwrap();
            drop(store);
            // Rebuild the exact v4/v5 shape: no attempt table, no attempt or lease columns (a
            // fresh store already carries the v5 `abandoned` proposal state; a v4 store is rebuilt
            // by the v4 step either way).
            db.raw()
                .execute_batch(&format!(
                    "DROP TABLE stage_recipe_job_attempts; \
                     ALTER TABLE stage_recipe_jobs DROP COLUMN lease_expires_at; \
                     ALTER TABLE stage_recipe_jobs DROP COLUMN owner_id; \
                     ALTER TABLE stage_recipe_jobs DROP COLUMN attempt; \
                     UPDATE schema_meta SET version = {from};",
                ))
                .unwrap();
            let before: Vec<(String, String, String)> = {
                let conn = db.raw();
                let mut stmt = conn
                    .prepare(
                        "SELECT recipe_job_id, status, updated_at FROM stage_recipe_jobs \
                         ORDER BY recipe_job_id",
                    )
                    .unwrap();
                let rows = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                rows
            };
            let store = db.store();
            let version: i64 = db
                .raw()
                .query_row("SELECT version FROM schema_meta", [], |r| r.get(0))
                .unwrap();
            assert_eq!(version, STORE_SCHEMA_VERSION);
            let jobs = store.recipe_jobs().unwrap();
            assert_eq!(
                jobs.iter()
                    .map(|j| (
                        j.recipe_job_id.to_string(),
                        j.status.as_str().to_owned(),
                        j.updated_at.to_string()
                    ))
                    .collect::<Vec<_>>(),
                before
            );
            assert!(jobs
                .iter()
                .all(|j| j.attempt == 1 && j.owner_id.is_none() && j.lease_expires_at.is_none()));
            assert!(store.recipe_job_attempts(&generic("1")).unwrap().is_empty());
            let running = store.recipe_job_attempts(&generic("2")).unwrap();
            assert_eq!(running.len(), 1);
            assert_eq!((running[0].ended_at, running[0].outcome), (None, None));
            let failed = store.recipe_job_attempts(&generic("3")).unwrap();
            assert_eq!(failed[0].outcome, Some(StageStatus::Failed));
            assert_eq!(failed[0].ended_at, Some(ts(T3)));
            assert_eq!(
                failed[0].error.as_deref(),
                Some(r#"{"code":"E_INFERENCE_FAILED","message":"provider \"down\""}"#)
            );
            // Pre-H0.3 rows are ownerless: recovery may interrupt every in-flight one.
            assert_eq!(
                store.abandoned_recipe_jobs(ts(T1)).unwrap(),
                vec![generic("1"), generic("2")]
            );
        }
    }

    // ------------------------------------------------------------------ stage jobs and steps

    #[test]
    fn stages_child_jobs_and_ordered_steps() {
        let db = Db::new();
        let mut store = db.store();
        inserted(store.insert_recipe_job(&db.recipe("1")).unwrap());
        let child = generic("c");
        let queued = inserted(
            store
                .insert_stage_job(
                    &child,
                    &json!({"b": 1, "a": 2}),
                    StageStatus::Queued,
                    ts(T1),
                )
                .unwrap(),
        );
        assert_eq!(queued.stage_job_json, r#"{"a":2,"b":1}"#);
        assert!(matches!(
            store
                .insert_stage_job(&child, &json!({}), StageStatus::Ready, ts(T2))
                .unwrap(),
            WorkflowInsert::Existing(e) if e == queued
        ));
        assert_eq!(store.in_flight_stage_jobs().unwrap(), vec![queued.clone()]);
        assert!(store
            .update_stage_job(
                &child,
                StageStatus::Ready,
                &json!({}),
                StageStatus::Stale,
                None,
                ts(T2)
            )
            .is_err());
        let ready = store
            .update_stage_job(
                &child,
                StageStatus::Queued,
                &json!({"s": "ready"}),
                StageStatus::Ready,
                Some(&db.run),
                ts(T2),
            )
            .unwrap();
        assert_eq!(ready.compile_run_ref, Some(db.run.clone()));
        assert!(store.in_flight_stage_jobs().unwrap().is_empty());
        // A compile run ref must be a stored artifact.
        assert!(store
            .update_stage_job(
                &child,
                StageStatus::Ready,
                &json!({}),
                StageStatus::Ready,
                Some(&generic("9")),
                ts(T2)
            )
            .is_err());

        for (ordinal, phase) in [(1, "classification"), (0, "segmentation")] {
            store
                .insert_step(&NewStageStep {
                    recipe_job_id: generic("1"),
                    ordinal,
                    phase: phase.to_owned(),
                    compiler_stage: "S1".to_owned(),
                    input_revision_id: db.r1.clone(),
                })
                .unwrap();
        }
        // Duplicate ordinal or phase, unknown phase or stage: rejected.
        for (ordinal, phase, stage) in [
            (0, "tags", "S2"),
            (2, "segmentation", "S1"),
            (3, "lint", "S1"),
            (4, "tags", "S9"),
        ] {
            assert!(
                store
                    .insert_step(&NewStageStep {
                        recipe_job_id: generic("1"),
                        ordinal,
                        phase: phase.to_owned(),
                        compiler_stage: stage.to_owned(),
                        input_revision_id: db.r1.clone(),
                    })
                    .is_err(),
                "{ordinal} {phase} {stage}"
            );
        }
        let updated = store
            .update_step(
                &generic("1"),
                0,
                Some(&child),
                Some(&db.run),
                Some(&db.run),
                None,
                Some(&db.r1),
            )
            .unwrap();
        assert_eq!(updated.stage_job_id, Some(child.clone()));
        assert_eq!(updated.intermediate_artifact_ref, Some(db.run.clone()));
        // None leaves fields unchanged.
        let again = store
            .update_step(&generic("1"), 0, None, None, None, None, None)
            .unwrap();
        assert_eq!(again, updated);
        assert!(store
            .update_step(&generic("1"), 0, None, None, None, Some("X9"), None)
            .is_err());
        let steps = db.store().steps(&generic("1")).unwrap();
        assert_eq!(
            steps.iter().map(|s| s.phase.as_str()).collect::<Vec<_>>(),
            ["segmentation", "classification"]
        );
        // An unknown child is a foreign-key violation.
        assert!(store
            .update_step(
                &generic("1"),
                1,
                Some(&generic("8")),
                None,
                None,
                None,
                None
            )
            .is_err());
    }

    // ------------------------------------------------------------------ proposal links

    #[test]
    fn stages_proposals_are_linked_atomically() {
        let db = Db::new();
        let mut store = db.store();
        inserted(store.insert_recipe_job(&db.recipe("1")).unwrap());
        let child = generic("c");
        inserted(
            store
                .insert_stage_job(&child, &json!({}), StageStatus::Ready, ts(T1))
                .unwrap(),
        );
        let proposals = [
            db.stage_proposal("prop:0000000000000001"),
            db.stage_proposal("prop:0000000000000002"),
        ];
        let stored = store
            .insert_stage_proposals(&generic("1"), &child, &db.run, &proposals)
            .unwrap();
        assert_eq!(stored.len(), 2);
        assert!(stored
            .iter()
            .all(|p| p.state == WorkflowProposalState::Pending
                && p.origin_kind == WorkflowOrigin::Stage
                && p.origin_ref.is_none()));
        let link = store
            .proposal_link(&id("prop:0000000000000001"), &db.r1)
            .unwrap()
            .unwrap();
        assert_eq!(
            link,
            StageProposalLink {
                proposal_id: id("prop:0000000000000001"),
                base_revision_id: db.r1.clone(),
                recipe_job_id: generic("1"),
                stage_job_id: child.clone(),
                compile_run_ref: db.run.clone(),
            }
        );
        assert_eq!(store.pending_proposal_links().unwrap().len(), 2);
        // A second parent reusing the child keeps the first links.
        inserted(store.insert_recipe_job(&db.recipe("2")).unwrap());
        store
            .insert_stage_proposals(&generic("2"), &child, &db.run, &proposals)
            .unwrap();
        assert_eq!(
            store
                .proposal_link(&id("prop:0000000000000002"), &db.r1)
                .unwrap()
                .unwrap()
                .recipe_job_id,
            generic("1")
        );
        // A non-stage origin, or an ID stored by another origin, writes nothing.
        let mut wf = SqliteWorkflowStore::open(&db.path).unwrap();
        let mut intake = db.stage_proposal("prop:0000000000000003");
        intake.origin_kind = WorkflowOrigin::Intake;
        wf.insert_proposal(&intake, WorkflowProposalState::Pending)
            .unwrap();
        let batch = [
            db.stage_proposal("prop:0000000000000004"),
            db.stage_proposal("prop:0000000000000003"),
        ];
        assert!(matches!(
            store.insert_stage_proposals(&generic("1"), &child, &db.run, &batch),
            Err(StoreError::StageProposalConflict { .. })
        ));
        assert!(wf
            .proposal(&id("prop:0000000000000004"), &db.r1)
            .unwrap()
            .is_none());
        assert!(matches!(
            store.insert_stage_proposals(&generic("1"), &child, &db.run, &[intake]),
            Err(StoreError::WorkflowInvalidOrigin { .. })
        ));
        // Resolving a proposal removes it from the pending links.
        wf.resolve_proposal(
            &id("prop:0000000000000001"),
            &db.r1,
            WorkflowProposalState::Accepted,
            &db.r2,
            ts(T2),
        )
        .unwrap();
        let pending: Vec<Id> = store
            .pending_proposal_links()
            .unwrap()
            .into_iter()
            .map(|l| l.proposal_id)
            .collect();
        assert_eq!(pending, vec![id("prop:0000000000000002")]);
        // H0.10: the identical stage proposal on a later baseline is a new occurrence with its
        // own link; the accepted r1 occurrence and its link are unchanged.
        let mut later = db.stage_proposal("prop:0000000000000001");
        later.base_revision_id = db.r2.clone();
        inserted(store.insert_recipe_job(&db.recipe("3")).unwrap());
        let stored = store
            .insert_stage_proposals(&generic("3"), &child, &db.run, &[later])
            .unwrap();
        assert_eq!(stored[0].base_revision_id, db.r2);
        assert_eq!(stored[0].state, WorkflowProposalState::Pending);
        assert_eq!(
            store
                .proposal_link(&id("prop:0000000000000001"), &db.r2)
                .unwrap()
                .unwrap()
                .recipe_job_id,
            generic("3")
        );
        assert_eq!(
            store
                .proposal_link(&id("prop:0000000000000001"), &db.r1)
                .unwrap()
                .unwrap()
                .recipe_job_id,
            generic("1")
        );
        let pending: Vec<(Id, RevisionId)> = store
            .pending_proposal_links()
            .unwrap()
            .into_iter()
            .map(|l| (l.proposal_id, l.base_revision_id))
            .collect();
        assert_eq!(
            pending,
            vec![
                (id("prop:0000000000000001"), db.r2.clone()),
                (id("prop:0000000000000002"), db.r1.clone())
            ]
        );
    }

    #[test]
    fn stages_v3_links_migrate_to_their_occurrence() {
        let db = Db::new();
        let mut store = db.store();
        inserted(store.insert_recipe_job(&db.recipe("1")).unwrap());
        let child = generic("c");
        inserted(
            store
                .insert_stage_job(&child, &json!({}), StageStatus::Ready, ts(T1))
                .unwrap(),
        );
        let mut on_r2 = db.stage_proposal("prop:0000000000000002");
        on_r2.base_revision_id = db.r2.clone();
        store
            .insert_stage_proposals(
                &generic("1"),
                &child,
                &db.run,
                &[db.stage_proposal("prop:0000000000000001"), on_r2],
            )
            .unwrap();
        let links = store.pending_proposal_links().unwrap();
        drop(store);
        // Rebuild the exact v3 shapes of both tables.
        let cols = "proposal_id, base_revision_id, proposal_json, intake_json, proposal_kind, \
                    state, origin_kind, origin_ref, created_at, resolved_at, resolved_revision_id";
        db.raw()
            .execute_batch(&format!(
                "DROP TABLE stage_recipe_job_attempts; \
                 ALTER TABLE stage_recipe_jobs DROP COLUMN lease_expires_at; \
                 ALTER TABLE stage_recipe_jobs DROP COLUMN owner_id; \
                 ALTER TABLE stage_recipe_jobs DROP COLUMN attempt; \
                 CREATE TABLE sl AS SELECT proposal_id, recipe_job_id, stage_job_id, \
                   compile_run_ref FROM stage_job_proposals; DROP TABLE stage_job_proposals; \
                 CREATE TABLE wp AS SELECT {cols} FROM workflow_proposals; \
                 DROP TABLE workflow_proposals; \
                 CREATE TABLE workflow_proposals (proposal_id TEXT PRIMARY KEY, \
                   base_revision_id TEXT NOT NULL, proposal_json TEXT NOT NULL, \
                   intake_json TEXT NOT NULL, proposal_kind TEXT NOT NULL, state TEXT NOT NULL, \
                   origin_kind TEXT NOT NULL, origin_ref TEXT NULL, created_at TEXT NOT NULL, \
                   resolved_at TEXT NULL, resolved_revision_id TEXT NULL, \
                   FOREIGN KEY (base_revision_id) REFERENCES graph_revisions(id), \
                   FOREIGN KEY (resolved_revision_id) REFERENCES graph_revisions(id)); \
                 INSERT INTO workflow_proposals SELECT * FROM wp; DROP TABLE wp; \
                 CREATE TABLE stage_job_proposals (proposal_id TEXT PRIMARY KEY, \
                   recipe_job_id TEXT NOT NULL, stage_job_id TEXT NOT NULL, \
                   compile_run_ref TEXT NOT NULL, \
                   FOREIGN KEY (proposal_id) REFERENCES workflow_proposals(proposal_id), \
                   FOREIGN KEY (recipe_job_id) REFERENCES stage_recipe_jobs(recipe_job_id), \
                   FOREIGN KEY (stage_job_id) REFERENCES stage_jobs(stage_job_id), \
                   FOREIGN KEY (compile_run_ref) REFERENCES artifacts(hash)); \
                 INSERT INTO stage_job_proposals SELECT * FROM sl; DROP TABLE sl; \
                 UPDATE schema_meta SET version = 3;"
            ))
            .unwrap();
        let store = db.store();
        assert_eq!(store.pending_proposal_links().unwrap(), links);
        assert_eq!(links[1].base_revision_id, db.r2);
        let version: i64 = db
            .raw()
            .query_row("SELECT version FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, STORE_SCHEMA_VERSION);
    }

    // ------------------------------------------------------------------ gate reports

    #[test]
    fn stages_gate_reports_upsert_per_revision_and_gate() {
        let db = Db::new();
        let mut store = db.store();
        let report = |gate: &str, body: Value, at: &str| NewGateReport {
            revision_id: db.r1.clone(),
            gate: gate.to_owned(),
            report_hash: generic("e"),
            report_json: body,
            supplemental_input_hash: None,
            evaluation_at: (gate == "F3").then(|| ts(at)),
            recipe_job_id: None,
            created_at: ts(at),
        };
        let first = store
            .upsert_gate_report(&report("F1", json!({"v": 1}), T1))
            .unwrap();
        assert_eq!(first.report_json, r#"{"v":1}"#);
        let second = store
            .upsert_gate_report(&report("F1", json!({"v": 2}), T2))
            .unwrap();
        assert_eq!(second.report_json, r#"{"v":2}"#);
        store
            .upsert_gate_report(&report("F3", json!({"v": 3}), T2))
            .unwrap();
        let all = db.store().gate_reports().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].created_at, ts(T2));
        assert_eq!(all[1].evaluation_at, Some(ts(T2)));
        // F3 alone carries evaluation_at; unknown gates and revisions are rejected.
        let mut bad = report("F2", json!({}), T1);
        bad.evaluation_at = Some(ts(T1));
        assert!(store.upsert_gate_report(&bad).is_err());
        let mut bad = report("F3", json!({}), T1);
        bad.evaluation_at = None;
        assert!(store.upsert_gate_report(&bad).is_err());
        assert!(store
            .upsert_gate_report(&report("Q1", json!({}), T1))
            .is_err());
        let mut bad = report("I0", json!({}), T1);
        bad.revision_id = "rev:9:0000000000000000".parse().unwrap();
        assert!(store.upsert_gate_report(&bad).is_err());
    }

    // ------------------------------------------------------------------ corruption

    #[test]
    fn stages_corrupt_json_is_an_error_never_repaired() {
        let db = Db::new();
        let mut store = db.store();
        inserted(store.insert_recipe_job(&db.recipe("1")).unwrap());
        inserted(
            store
                .insert_stage_job(&generic("c"), &json!({"a": 1}), StageStatus::Queued, ts(T1))
                .unwrap(),
        );
        store
            .upsert_gate_report(&NewGateReport {
                revision_id: db.r1.clone(),
                gate: "I0".to_owned(),
                report_hash: generic("e"),
                report_json: json!({"a": 1}),
                supplemental_input_hash: None,
                evaluation_at: None,
                recipe_job_id: Some(generic("1")),
                created_at: ts(T1),
            })
            .unwrap();
        let conn = db.raw();
        for sql in [
            "UPDATE stage_recipe_jobs SET recipe_job_json = '{\"z\":1, \"a\":2}'",
            "UPDATE stage_jobs SET stage_job_json = 'not json'",
            "UPDATE gate_reports SET report_json = '{\"b\":1,\"a\":2}'",
        ] {
            conn.execute_batch(sql).unwrap();
        }
        let store = db.store();
        assert!(matches!(
            store.recipe_job(&generic("1")),
            Err(StoreError::CorruptWorkflowRecord { .. })
        ));
        assert!(matches!(
            store.stage_job(&generic("c")),
            Err(StoreError::CorruptWorkflowRecord { .. })
        ));
        assert!(matches!(
            store.gate_reports(),
            Err(StoreError::CorruptWorkflowRecord { .. })
        ));
        // Still corrupt after reading: nothing was repaired.
        let text: String = conn
            .query_row("SELECT stage_job_json FROM stage_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(text, "not json");
    }

    #[test]
    fn stages_status_vocabulary() {
        let wire: Vec<&str> = StageStatus::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            wire,
            [
                "queued",
                "acquiring",
                "evaluating",
                "ready",
                "stale",
                "failed",
                "committed"
            ]
        );
        for s in StageStatus::ALL {
            assert_eq!(s.as_str().parse::<StageStatus>().unwrap(), s);
        }
        assert!("done".parse::<StageStatus>().is_err());
        assert!(StageStatus::Queued.is_in_flight() && !StageStatus::Ready.is_in_flight());
    }

    /// The `directory` recipe job and step of a store-schema-v7 store.
    fn directory_job(db: &Db, job: &str) -> NewStageRecipeJob {
        NewStageRecipeJob {
            recipe: "directory".to_owned(),
            recipe_job_json: json!({"recipe": "directory"}),
            ..db.recipe(job)
        }
    }

    fn directory_step(db: &Db, job: &str) -> NewStageStep {
        NewStageStep {
            recipe_job_id: generic(job),
            ordinal: 0,
            phase: "directory".to_owned(),
            compiler_stage: "S2".to_owned(),
            input_revision_id: db.r1.clone(),
        }
    }

    /// The pipeline (UI backend gap): store schema v7 admits the `directory` recipe and phase,
    /// so the configured directory runs as an ordinary recipe job.
    #[test]
    fn stages_v7_admits_the_directory_recipe_and_phase() {
        assert_eq!(STORE_SCHEMA_VERSION, 7);
        let db = Db::new();
        let mut store = db.store();
        let job = inserted(store.insert_recipe_job(&directory_job(&db, "7")).unwrap());
        assert_eq!(job.recipe, "directory");
        store.insert_step(&directory_step(&db, "7")).unwrap();
        assert_eq!(store.steps(&generic("7")).unwrap()[0].phase, "directory");
        // Anything else is still refused.
        let mut other = directory_job(&db, "8");
        other.recipe = "nope".to_owned();
        assert!(store.insert_recipe_job(&other).is_err());
    }

    /// A v6 store (no `directory` recipe or phase) migrates in place to v7: every recipe job and
    /// step keeps its row, the schema equals a fresh one and `directory` is admitted.
    #[test]
    fn stages_v6_store_migrates_to_v7_admitting_directory() {
        let fresh = Db::new();
        let db = Db::new();
        let mut store = db.store();
        for job in ["1", "2"] {
            inserted(store.insert_recipe_job(&db.recipe(job)).unwrap());
        }
        assert!(store.claim_recipe_job(&generic("2"), ts(T2)).unwrap());
        store
            .insert_step(&NewStageStep {
                recipe_job_id: generic("2"),
                ordinal: 0,
                phase: "segmentation".to_owned(),
                compiler_stage: "S1".to_owned(),
                input_revision_id: db.r1.clone(),
            })
            .unwrap();
        drop(store);
        let schema = |db: &Db| -> Vec<String> {
            let conn = db.raw();
            let mut stmt = conn
                .prepare(
                    "SELECT type || name || COALESCE(sql, '') FROM sqlite_master ORDER BY name",
                )
                .unwrap();
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows.into_iter().map(|r| r.replace('"', "")).collect()
        };
        let rows = |db: &Db, sql: &str| -> Vec<String> {
            let conn = db.raw();
            let mut stmt = conn.prepare(sql).unwrap();
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        // Rebuild the exact v6 shape: the recipe and phase CHECKs without `directory`.
        let conn = db.raw();
        let ddl = |table: &str| -> String {
            conn.query_row(
                "SELECT sql FROM sqlite_master WHERE name = ?1",
                [table],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
            .replace(", 'directory'", "")
        };
        let (jobs, steps) = (ddl("stage_recipe_jobs"), ddl("stage_recipe_job_steps"));
        assert!(!jobs.contains("directory") && !steps.contains("directory"));
        conn.execute_batch(&format!(
            "PRAGMA foreign_keys = OFF; \
             CREATE TABLE jobs_v6 AS SELECT * FROM stage_recipe_jobs; \
             CREATE TABLE steps_v6 AS SELECT * FROM stage_recipe_job_steps; \
             DROP TABLE stage_recipe_job_steps; DROP TABLE stage_recipe_jobs; \
             {jobs}; {steps}; \
             INSERT INTO stage_recipe_jobs SELECT * FROM jobs_v6; \
             INSERT INTO stage_recipe_job_steps SELECT * FROM steps_v6; \
             DROP TABLE jobs_v6; DROP TABLE steps_v6; \
             UPDATE schema_meta SET version = 6;"
        ))
        .unwrap();
        drop(conn);
        assert!(db.exec_err(
            "UPDATE stage_recipe_jobs SET recipe = 'directory' WHERE recipe_job_id LIKE 'sha256:1%'"
        ));
        let jobs_before = rows(
            &db,
            "SELECT recipe_job_id || recipe || status || attempt FROM stage_recipe_jobs \
             ORDER BY rowid",
        );
        let steps_before = rows(
            &db,
            "SELECT recipe_job_id || phase FROM stage_recipe_job_steps",
        );
        let mut store = db.store();
        let version: i64 = db
            .raw()
            .query_row("SELECT version FROM schema_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, STORE_SCHEMA_VERSION);
        assert_eq!(
            rows(
                &db,
                "SELECT recipe_job_id || recipe || status || attempt FROM stage_recipe_jobs \
                 ORDER BY rowid",
            ),
            jobs_before
        );
        assert_eq!(
            rows(
                &db,
                "SELECT recipe_job_id || phase FROM stage_recipe_job_steps"
            ),
            steps_before
        );
        assert_eq!(store.recipe_job_attempts(&generic("2")).unwrap().len(), 1);
        drop(fresh.store());
        assert_eq!(schema(&db), schema(&fresh));
        inserted(store.insert_recipe_job(&directory_job(&db, "7")).unwrap());
        store.insert_step(&directory_step(&db, "7")).unwrap();
    }
}
