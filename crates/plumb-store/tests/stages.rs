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
            .proposal_link(&id("prop:0000000000000001"))
            .unwrap()
            .unwrap();
        assert_eq!(
            link,
            StageProposalLink {
                proposal_id: id("prop:0000000000000001"),
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
                .proposal_link(&id("prop:0000000000000002"))
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
        assert!(wf.proposal(&id("prop:0000000000000004")).unwrap().is_none());
        assert!(matches!(
            store.insert_stage_proposals(&generic("1"), &child, &db.run, &[intake]),
            Err(StoreError::WorkflowInvalidOrigin { .. })
        ));
        // Resolving a proposal removes it from the pending links.
        wf.resolve_proposal(
            &id("prop:0000000000000001"),
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
}
