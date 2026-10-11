//! X1.5 contract tests for the asynchronous stage job lifecycle and base-revision protection
//! (Hotfix 061).
//!
//! Every test lives in `job` (`cargo test -p plumb-compiler job`). Revisions come from a real
//! in-memory `SqliteRevisionStore`; runs from the real `CompileContext`, `StagePlan`,
//! `ArtifactSet`, `StageEvaluation` and `CompileRun::new`. The job-ID golden was computed
//! independently in Python (sorted-key compact `json.dumps`, `hashlib.sha256`).

mod job {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_compiler::*;
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_patch::{
        AcceptancePolicy, ElementPrecondition, PatchSet, Proposal, ProposalMateriality,
        SemanticPatch,
    };
    use plumb_psg::{
        node_element_hash, source_artifact_id, AuditMeta, ElementStatus, Graph, Modality, Node,
        NodePayload, Requirement, RequirementKind, RequirementLevel, SourceArtifact,
    };
    use plumb_store::{
        BranchName, CommitResult, GraphRevision, LoadedRevision, RevisionId, RevisionWriteMeta,
        SqliteRevisionStore, StoreError,
    };
    use serde_json::{json, Value};

    use ElementStatus::{Accepted, Proposed};
    use StageJobStatus::{Acquiring, Committed, Evaluating, Failed, Queued, Ready, Stale};

    const PROJECT: &str = "project:leave-management";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-09-29T12:00:00.000000000Z";
    const JOB_SRC: &str = include_str!("../src/job.rs");

    /// Python: the identity projection below, RFC 8785, SHA-256.
    const GOLDEN_JOB_PROJECTION: &str = r#"{"base_revision":"rev:1:92524cd18e6953a2","base_semantic_hash":"psg:sha256:92524cd18e6953a2710d06bde1414f64e5281b46353872ea3286ed484a2f862c","branch":"main","config_hash":"sha256:5359f0a81f1e876e87ffe35cffc12b819a044a962a9c542f5c975ca9c0e72e19","profile_hash":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","scope":{"kind":"elements","refs":["req:a","req:b"]},"stage":"S1"}"#;
    const GOLDEN_JOB_ID: &str =
        "sha256:0a22f4b5dc404b787ba21d12d55f72ce847515cf17d88000104b45bbeee82c46";
    const GOLDEN_SEMANTIC_HASH: &str =
        "psg:sha256:92524cd18e6953a2710d06bde1414f64e5281b46353872ea3286ed484a2f862c";
    const GOLDEN_CONFIG_HASH: &str =
        "sha256:5359f0a81f1e876e87ffe35cffc12b819a044a962a9c542f5c975ca9c0e72e19";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn generic(c: char) -> Hash {
        format!("sha256:{}", c.to_string().repeat(64))
            .parse()
            .unwrap()
    }

    fn node(node_id: &str, status: ElementStatus, payload: NodePayload) -> Node {
        Node {
            id: id(node_id),
            revision: 1,
            status,
            payload,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: BTreeSet::new(),
            extensions: BTreeMap::new(),
            audit: AuditMeta::new(id("agent:analyst"), ts(AT), None, None).unwrap(),
        }
    }

    fn req(node_id: &str, status: ElementStatus, statement: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::Requirement(Requirement {
                statement: statement.into(),
                requirement_kind: RequirementKind::Functional,
                level: RequirementLevel::System,
                modality: Modality::Shall,
                title: None,
                rationale: None,
                priority: None,
                source_identifier: None,
                verification_method: None,
                owner_refs: None,
                stakeholder_refs: None,
            }),
        )
    }

    /// An Accepted source artifact: evidence material, excluded from the semantic hash.
    fn source(name: &str) -> Node {
        let content = Hash::content_sha256(name.as_bytes());
        node(
            source_artifact_id(&content).unwrap().as_str(),
            Accepted,
            NodePayload::SourceArtifact(SourceArtifact {
                source_kind: "markdown".into(),
                display_name: name.into(),
                content_hash: content,
                media_type: "text/markdown".into(),
                external_uri: None,
                external_version: None,
                producer: None,
                created_at_source: None,
                language: None,
                classification: None,
            }),
        )
    }

    fn meta() -> RevisionWriteMeta {
        RevisionWriteMeta {
            decision_refs: Vec::new(),
            created_by: id("agent:analyst"),
            created_at: ts(AT),
        }
    }

    fn base_graph() -> Graph {
        Graph::new(
            id(PROJECT),
            id(PROFILE),
            vec![
                req("req:a", Accepted, "Employees shall submit leave requests."),
                req("req:b", Proposed, "Managers shall approve leave requests."),
            ],
            Vec::new(),
        )
        .unwrap()
    }

    fn main() -> BranchName {
        BranchName::main()
    }

    fn config(value: Value) -> CanonicalJson {
        CanonicalJson::new(value)
    }

    /// A real store at revision 1 and everything a job captured there needs.
    struct World {
        store: SqliteRevisionStore,
        rev1: GraphRevision,
        loaded: LoadedRevision,
        ctx: CompileContext,
        scope: Scope,
    }

    fn world() -> World {
        let mut store = SqliteRevisionStore::open(":memory:").unwrap();
        let rev1 = store
            .create_initial_revision(&base_graph(), generic('a'), generic('b'), meta())
            .unwrap();
        let loaded = store.load_revision(&rev1.id).unwrap();
        let ctx = CompileContext::from_loaded_revision(
            &loaded,
            "0.1.0".into(),
            config(json!({"mode": "strict"})),
        )
        .unwrap();
        let scope = Scope::elements(vec![id("req:b"), id("req:a")]).unwrap();
        World {
            store,
            rev1,
            loaded,
            ctx,
            scope,
        }
    }

    impl World {
        fn head(&self) -> RevisionId {
            self.store.head(&main()).unwrap().unwrap()
        }

        /// A job captured when the head was observed at revision 1.
        fn capture(&self) -> StageJob {
            capture_stage_job(
                main(),
                &self.rev1.id,
                &self.loaded,
                StageId::S1,
                self.scope.clone(),
                &self.ctx,
            )
            .unwrap()
        }

        fn base(&self) -> Hash {
            self.rev1.semantic_hash.clone()
        }

        /// Accepts req:b: a semantic change.
        fn accept_b(&self) -> PatchSet {
            let b = self.loaded.graph.node(&id("req:b")).unwrap();
            PatchSet {
                base_semantic_hash: self.base(),
                patch: SemanticPatch::SetStatus {
                    target: ElementPrecondition {
                        id: id("req:b"),
                        expected_hash: node_element_hash(b).unwrap(),
                    },
                    from: Proposed,
                    to: Accepted,
                },
            }
        }

        fn add(&self, node: Node) -> PatchSet {
            PatchSet {
                base_semantic_hash: self.base(),
                patch: SemanticPatch::AddNode { node },
            }
        }

        fn proposal(&self, patch_set: PatchSet, confidence: Option<f32>) -> Proposal {
            Proposal::new(
                StageId::S1,
                patch_set,
                Vec::new(),
                Vec::new(),
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
                confidence,
            )
            .unwrap()
        }

        /// The stage outputs: two proposals and a derivation patch set.
        fn outputs(&self) -> Outputs {
            let p1 = self.proposal(self.accept_b(), Some(0.5));
            let p2 = self.proposal(
                self.add(req(
                    "req:c",
                    Proposed,
                    "Managers shall see pending requests.",
                )),
                None,
            );
            let derivation = self.add(req("req:d", Proposed, "The system shall list requests."));
            let evaluation =
                StageEvaluation::new(Some(derivation.clone()), vec![p1.clone(), p2.clone()])
                    .unwrap();
            let run = self.run(&self.ctx, StageId::S1, &evaluation);
            Outputs {
                p1,
                p2,
                derivation,
                plan: StagePlan::new(self.scope.clone(), vec![], vec![], vec![]).unwrap(),
                run,
            }
        }

        fn run(
            &self,
            ctx: &CompileContext,
            stage: StageId,
            evaluation: &StageEvaluation,
        ) -> CompileRun {
            let artifacts = ArtifactSet::new(vec![], vec![], vec![]).unwrap();
            CompileRun::new(stage, ctx, &artifacts, evaluation).unwrap()
        }

        /// A concurrent writer advances main from revision 1.
        fn concurrent(&mut self, patch_set: &PatchSet) -> CommitResult {
            let base = self.rev1.id.clone();
            self.store
                .commit(&main(), &base, patch_set, meta())
                .unwrap()
        }
    }

    struct Outputs {
        p1: Proposal,
        p2: Proposal,
        derivation: PatchSet,
        plan: StagePlan,
        run: CompileRun,
    }

    fn evaluating(w: &World) -> StageJob {
        let mut job = w.capture();
        job.start_evaluating().unwrap();
        job
    }

    fn ready(w: &World, o: &Outputs) -> StageJob {
        let mut job = evaluating(w);
        assert_eq!(job.complete(&o.plan, &o.run, &w.head()).unwrap(), Ready);
        job
    }

    fn proposal_ref(p: &Proposal) -> Hash {
        Hash::content_sha256(&to_canonical_json(p).unwrap())
    }

    fn patch_ref(p: &PatchSet) -> Hash {
        Hash::content_sha256(&to_canonical_json(p).unwrap())
    }

    // ------------------------------------------------------------------ capture

    #[test]
    fn capture_binds_every_field_and_matches_the_golden_id() {
        let w = world();
        let job = w.capture();
        assert_eq!(w.rev1.semantic_hash.as_str(), GOLDEN_SEMANTIC_HASH);
        assert_eq!(job.id().as_str(), GOLDEN_JOB_ID);
        assert_eq!(
            Hash::content_sha256(GOLDEN_JOB_PROJECTION.as_bytes()).as_str(),
            GOLDEN_JOB_ID
        );
        assert_eq!(
            to_canonical_json(&job.identity_projection()).unwrap(),
            GOLDEN_JOB_PROJECTION.as_bytes()
        );
        assert_eq!(job.branch(), &main());
        assert_eq!(job.base_revision(), &w.rev1.id);
        assert_eq!(job.base_revision().as_str(), "rev:1:92524cd18e6953a2");
        assert_eq!(job.base_semantic_hash(), &w.rev1.semantic_hash);
        assert_eq!(job.stage(), StageId::S1);
        assert_eq!(job.scope(), &w.scope);
        assert_eq!(job.profile_hash(), &w.rev1.profile_hash);
        assert_eq!(job.config_hash().as_str(), GOLDEN_CONFIG_HASH);
        assert_eq!(job.config_hash(), &w.ctx.config_hash().unwrap());
        assert_eq!(job.status(), Queued);
        assert_eq!(job.compile_run_ref(), None);
        assert_eq!(job.committed_revision(), None);
        assert_eq!(job.committed_output_ref(), None);
        job.validate().unwrap();
        assert_eq!(&job.recompute_id().unwrap(), job.id());
    }

    #[test]
    fn capture_identity_depends_on_capture_only() {
        let w = world();
        let job = w.capture();
        let reordered = capture_stage_job(
            main(),
            &w.head(),
            &w.loaded,
            StageId::S1,
            Scope::elements(vec![id("req:a"), id("req:b")]).unwrap(),
            &w.ctx,
        )
        .unwrap();
        assert_eq!(reordered, job);
        let other = |branch: BranchName, stage: StageId, scope: Scope| {
            capture_stage_job(branch, &w.head(), &w.loaded, stage, scope, &w.ctx)
                .unwrap()
                .id()
                .clone()
        };
        let feature: BranchName = serde_json::from_value(json!("feature/x")).unwrap();
        assert_ne!(other(feature, StageId::S1, w.scope.clone()), *job.id());
        assert_ne!(other(main(), StageId::S2, w.scope.clone()), *job.id());
        assert_ne!(other(main(), StageId::S1, Scope::Project), *job.id());
        // Config participates through context.config_hash().
        let ctx2 = CompileContext::from_loaded_revision(
            &w.loaded,
            "0.1.0".into(),
            config(json!({"mode": "lenient"})),
        )
        .unwrap();
        let job2 = capture_stage_job(
            main(),
            &w.head(),
            &w.loaded,
            StageId::S1,
            w.scope.clone(),
            &ctx2,
        )
        .unwrap();
        assert_ne!(job2.config_hash(), job.config_hash());
        assert_ne!(job2.id(), job.id());
        assert_eq!(job2.config_hash(), &ctx2.config_hash().unwrap());
        // The compiler version is not part of the job identity.
        let ctx3 = CompileContext::from_loaded_revision(
            &w.loaded,
            "0.2.0".into(),
            config(json!({"mode": "strict"})),
        )
        .unwrap();
        let job3 = capture_stage_job(
            main(),
            &w.head(),
            &w.loaded,
            StageId::S1,
            w.scope.clone(),
            &ctx3,
        )
        .unwrap();
        assert_eq!(job3.id(), job.id());
    }

    #[test]
    fn capture_rejects_a_stale_observation_and_mismatched_inputs() {
        let mut w = world();
        let source = w.add(source("other.md"));
        let rev2 = w.concurrent(&source).revision;
        let capture =
            |observed: &RevisionId, loaded: &LoadedRevision, scope: Scope, ctx: &CompileContext| {
                capture_stage_job(main(), observed, loaded, StageId::S1, scope, ctx)
            };
        assert!(matches!(
            capture(&rev2.id, &w.loaded, w.scope.clone(), &w.ctx),
            Err(StageJobError::ObservedHeadMismatch { .. })
        ));
        let check = |f: &dyn Fn(&mut CompileContext), field: &str| {
            let mut ctx = w.ctx.clone();
            f(&mut ctx);
            let err = capture(&w.rev1.id, &w.loaded, w.scope.clone(), &ctx).unwrap_err();
            assert!(
                matches!(err, StageJobError::ContextMismatch(f) if f == field),
                "{field}: {err:?}"
            );
        };
        check(&|c| c.input_revision = rev2.id.clone(), "input_revision");
        check(
            &|c| {
                c.input_semantic_hash =
                    "psg:sha256:1111111111111111111111111111111111111111111111111111111111111111"
                        .parse()
                        .unwrap()
            },
            "input_semantic_hash",
        );
        check(&|c| c.profile_ref = id("profile:other"), "profile_ref");
        check(&|c| c.profile_hash = generic('c'), "profile_hash");
        check(&|c| c.rule_pack_hash = generic('c'), "rule_pack_hash");
        // A loaded revision whose metadata disagrees with its graph.
        let mut loaded = w.loaded.clone();
        loaded.revision.semantic_hash = rev2.semantic_hash.clone();
        loaded.graph = Graph::new(
            id(PROJECT),
            id(PROFILE),
            vec![req("req:a", Accepted, "Changed.")],
            vec![],
        )
        .unwrap();
        assert!(matches!(
            capture(&w.rev1.id, &loaded, w.scope.clone(), &w.ctx),
            Err(StageJobError::LoadedRevisionMismatch("semantic_hash"))
        ));
        // A scope element that is not in the graph.
        let missing = Scope::elements(vec![id("req:missing")]).unwrap();
        assert!(matches!(
            capture(&w.rev1.id, &w.loaded, missing, &w.ctx),
            Err(StageJobError::Compiler(CompilerError::InvalidScope(_)))
        ));
    }

    // ------------------------------------------------------------------ wire

    #[test]
    fn status_wire_strings() {
        for (status, wire) in [
            (Queued, "queued"),
            (Acquiring, "acquiring"),
            (Evaluating, "evaluating"),
            (Ready, "ready"),
            (Stale, "stale"),
            (Failed, "failed"),
            (Committed, "committed"),
        ] {
            assert_eq!(status.as_str(), wire);
            assert_eq!(status.to_string(), wire);
            assert_eq!(serde_json::to_value(status).unwrap(), json!(wire));
            assert_eq!(
                serde_json::from_value::<StageJobStatus>(json!(wire)).unwrap(),
                status
            );
            assert_eq!(
                status.is_terminal(),
                matches!(status, Stale | Failed | Committed)
            );
        }
        for unknown in ["running", "completed", "cancelled", "Ready"] {
            assert!(serde_json::from_value::<StageJobStatus>(json!(unknown)).is_err());
        }
    }

    #[test]
    fn job_wire_is_validated() {
        let w = world();
        let o = w.outputs();
        let job = ready(&w, &o);
        let wire = serde_json::to_value(&job).unwrap();
        assert_eq!(
            wire.as_object().unwrap().keys().collect::<Vec<_>>(),
            [
                "base_revision",
                "base_semantic_hash",
                "branch",
                "committed_output_ref",
                "committed_revision",
                "compile_run_ref",
                "config_hash",
                "id",
                "profile_hash",
                "scope",
                "stage",
                "status"
            ]
        );
        assert_eq!(
            serde_json::from_value::<StageJob>(wire.clone()).unwrap(),
            job
        );
        let rejected = |f: &dyn Fn(&mut Value)| {
            let mut v = wire.clone();
            f(&mut v);
            serde_json::from_value::<StageJob>(v).is_err()
        };
        assert!(rejected(&|v| v["status"] = json!("queued")));
        assert!(rejected(&|v| v["compile_run_ref"] = Value::Null));
        assert!(rejected(&|v| v["committed_revision"] = json!(w.rev1.id)));
        assert!(rejected(&|v| v["status"] = json!("committed")));
        assert!(rejected(&|v| v["extra"] = json!(1)));
        assert!(rejected(&|v| v["stage"] = json!("S2")));
        assert!(rejected(&|v| v["base_semantic_hash"] = json!(generic('a'))));
        assert!(rejected(
            &|v| v["compile_run_ref"] = json!(GOLDEN_SEMANTIC_HASH)
        ));
        assert!(rejected(
            &|v| v["scope"] = json!({"kind": "elements", "refs": ["req:b", "req:a"]})
        ));
        // A queued wire with a run is impossible.
        let queued = serde_json::to_value(w.capture()).unwrap();
        let mut bad = queued.clone();
        bad["compile_run_ref"] = json!(o.run.id);
        assert!(serde_json::from_value::<StageJob>(bad).is_err());
        assert_eq!(
            serde_json::from_value::<StageJob>(queued).unwrap(),
            w.capture()
        );
    }

    // ------------------------------------------------------------------ lifecycle

    fn invalid(result: Result<(), StageJobError>, from: StageJobStatus) {
        assert!(
            matches!(result, Err(StageJobError::InvalidTransition { from: f, .. }) if f == from),
            "{result:?}"
        );
    }

    #[test]
    fn legal_transitions() {
        let w = world();
        let mut job = w.capture();
        job.start_acquiring().unwrap();
        assert_eq!(job.status(), Acquiring);
        job.start_evaluating().unwrap();
        assert_eq!(job.status(), Evaluating);
        let mut direct = w.capture();
        direct.start_evaluating().unwrap();
        assert_eq!(direct.status(), Evaluating);
        for status in [Queued, Acquiring, Evaluating] {
            let mut j = w.capture();
            if status != Queued {
                j.start_acquiring().unwrap();
            }
            if status == Evaluating {
                j.start_evaluating().unwrap();
            }
            assert_eq!(j.status(), status);
            j.mark_failed().unwrap();
            assert_eq!(j.status(), Failed);
            assert_eq!(j.compile_run_ref(), None);
            j.validate().unwrap();
        }
    }

    #[test]
    fn illegal_transitions_are_typed_errors() {
        let mut w = world();
        let o = w.outputs();
        let mut acquiring = w.capture();
        acquiring.start_acquiring().unwrap();
        invalid(acquiring.start_acquiring(), Acquiring);
        let mut evaluating = evaluating(&w);
        invalid(evaluating.start_acquiring(), Evaluating);
        invalid(evaluating.start_evaluating(), Evaluating);
        let mut queued = w.capture();
        assert!(matches!(
            queued.complete(&o.plan, &o.run, &w.head()),
            Err(StageJobError::InvalidTransition { from: Queued, .. })
        ));
        let mut ready = ready(&w, &o);
        invalid(ready.start_evaluating(), Ready);
        invalid(ready.start_acquiring(), Ready);
        invalid(ready.mark_failed(), Ready);
        assert!(matches!(
            ready.complete(&o.plan, &o.run, &w.head()),
            Err(StageJobError::InvalidTransition { from: Ready, .. })
        ));
        let mut failed = w.capture();
        failed.mark_failed().unwrap();
        invalid(failed.start_evaluating(), Failed);
        invalid(failed.mark_failed(), Failed);
        let rev2 = w.concurrent(&w.accept_b()).revision;
        let mut stale = ready.clone();
        assert_eq!(stale.observe_head(&rev2.id), Stale);
        invalid(stale.start_evaluating(), Stale);
        invalid(stale.mark_failed(), Stale);
        assert!(matches!(
            stale.complete(&o.plan, &o.run, &w.rev1.id),
            Err(StageJobError::InvalidTransition { from: Stale, .. })
        ));
        // Statuses are unchanged by every rejected request.
        assert_eq!(
            (
                acquiring.status(),
                evaluating.status(),
                ready.status(),
                failed.status(),
                stale.status()
            ),
            (Acquiring, Evaluating, Ready, Failed, Stale)
        );
    }

    // ------------------------------------------------------------------ completion

    #[test]
    fn completion_at_the_base_head_is_ready() {
        let w = world();
        let o = w.outputs();
        let job = ready(&w, &o);
        assert_eq!(job.status(), Ready);
        assert_eq!(job.compile_run_ref(), Some(&o.run.id));
        assert_eq!(job.committed_revision(), None);
        job.validate().unwrap();
        // The run is the real run of the evaluation.
        assert_eq!(o.run.output_proposal_refs.len(), 2);
        assert!(o.run.output_proposal_refs.contains(&proposal_ref(&o.p1)));
        assert_eq!(
            o.run.output_derivation_patch_ref,
            Some(patch_ref(&o.derivation))
        );
    }

    #[test]
    fn completion_binding_errors_leave_the_job_evaluating() {
        let w = world();
        let o = w.outputs();
        let empty = StageEvaluation::new(None, vec![]).unwrap();
        let with_ctx = |f: &dyn Fn(&mut CompileContext)| {
            let mut ctx = w.ctx.clone();
            f(&mut ctx);
            w.run(&ctx, StageId::S1, &empty)
        };
        let other_plan = StagePlan::new(
            Scope::elements(vec![id("req:a")]).unwrap(),
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        let mut job = evaluating(&w);
        assert!(matches!(
            job.complete(&other_plan, &o.run, &w.head()),
            Err(StageJobError::ScopeMismatch)
        ));
        let cases: Vec<(CompileRun, &str)> = vec![
            (w.run(&w.ctx, StageId::S2, &empty), "stage"),
            (
                with_ctx(&|c| c.input_revision = RevisionId::new(7, &w.base()).unwrap()),
                "input_revision",
            ),
            (
                with_ctx(&|c| {
                    c.input_semantic_hash =
                        "psg:sha256:1111111111111111111111111111111111111111111111111111111111111111"
                            .parse()
                            .unwrap()
                }),
                "input_semantic_hash",
            ),
            (with_ctx(&|c| c.profile_hash = generic('c')), "profile_hash"),
            (
                with_ctx(&|c| c.config = config(json!({"mode": "lenient"}))),
                "config_hash",
            ),
        ];
        for (run, field) in cases {
            let err = job.complete(&o.plan, &run, &w.head()).unwrap_err();
            assert!(
                matches!(err, StageJobError::RunMismatch(f) if f == field),
                "{field}: {err:?}"
            );
            assert_eq!(job.status(), Evaluating);
            assert_eq!(job.compile_run_ref(), None);
        }
        assert_eq!(job.complete(&o.plan, &o.run, &w.head()).unwrap(), Ready);
    }

    #[test]
    fn concurrent_semantic_commit_makes_completion_stale() {
        let mut w = world();
        let o = w.outputs();
        let mut job = evaluating(&w);
        let rev2 = w
            .concurrent(&w.add(req("req:x", Accepted, "Payroll shall export.")))
            .revision;
        assert_ne!(rev2.semantic_hash, w.rev1.semantic_hash);
        assert_eq!(w.head(), rev2.id);
        assert_eq!(job.complete(&o.plan, &o.run, &w.head()).unwrap(), Stale);
        assert_eq!(job.compile_run_ref(), Some(&o.run.id));
        job.validate().unwrap();
    }

    #[test]
    fn evidence_only_commit_with_equal_semantic_hash_makes_completion_stale() {
        let mut w = world();
        let o = w.outputs();
        let mut job = evaluating(&w);
        let rev1 = w.rev1.clone();
        let rev2 = w.concurrent(&w.add(source("policy.md"))).revision;
        eprintln!(
            "base revision {} semantic {} evidence {}\nnew revision {} semantic {} evidence {}",
            rev1.id.as_str(),
            rev1.semantic_hash,
            rev1.evidence_hash,
            rev2.id.as_str(),
            rev2.semantic_hash,
            rev2.evidence_hash
        );
        assert_eq!(rev1.semantic_hash, rev2.semantic_hash);
        assert_ne!(rev1.evidence_hash, rev2.evidence_hash);
        assert_ne!(rev1.id, rev2.id);
        assert_eq!(rev2.parent, Some(rev1.id.clone()));
        assert_eq!(w.head(), rev2.id);
        assert_eq!(job.complete(&o.plan, &o.run, &w.head()).unwrap(), Stale);
        assert_eq!(job.status(), Stale);
        // The stale outputs are not commit-eligible even though PatchSet CAS would accept them.
        assert_eq!(o.p1.patch_set.base_semantic_hash, rev2.semantic_hash);
        assert!(matches!(
            job.validate_proposal_commit(&w.head(), &o.run, &o.p1),
            Err(StageJobError::JobNotReady(Stale))
        ));
    }

    #[test]
    fn ready_job_becomes_stale_when_the_head_moves_and_never_revives() {
        let mut w = world();
        let o = w.outputs();
        let mut job = ready(&w, &o);
        assert_eq!(job.observe_head(&w.rev1.id), Ready);
        let rev2 = w.concurrent(&w.add(source("late.md"))).revision;
        assert_eq!(job.observe_head(&rev2.id), Stale);
        assert_eq!(job.observe_head(&w.rev1.id), Stale);
        assert_eq!(job.compile_run_ref(), Some(&o.run.id));
        // Observation does not move non-ready jobs either.
        let mut queued = w.capture();
        assert_eq!(queued.observe_head(&rev2.id), Queued);
    }

    #[test]
    fn re_evaluation_is_a_new_job() {
        for semantic in [true, false] {
            let mut w = world();
            let o = w.outputs();
            let mut old = ready(&w, &o);
            let patch = if semantic {
                w.accept_b()
            } else {
                w.add(source("reeval.md"))
            };
            let rev2 = w.concurrent(&patch).revision;
            assert_eq!(old.observe_head(&w.head()), Stale);
            let loaded2 = w.store.load_revision(&w.head()).unwrap();
            let ctx2 = CompileContext::from_loaded_revision(
                &loaded2,
                "0.1.0".into(),
                config(json!({"mode": "strict"})),
            )
            .unwrap();
            let new = capture_stage_job(
                main(),
                &w.head(),
                &loaded2,
                StageId::S1,
                w.scope.clone(),
                &ctx2,
            )
            .unwrap();
            assert_eq!(old.status(), Stale);
            assert_eq!(new.status(), Queued);
            assert_eq!(new.base_revision(), &rev2.id);
            assert_ne!(new.id(), old.id());
            assert_eq!(new.compile_run_ref(), None);
            assert_eq!(
                new.base_semantic_hash() == old.base_semantic_hash(),
                !semantic,
                "semantic change {semantic}"
            );
            // The old run is not a run of the new job.
            let mut new = new;
            new.start_evaluating().unwrap();
            assert!(matches!(
                new.complete(&o.plan, &o.run, &w.head()),
                Err(StageJobError::RunMismatch("input_revision"))
            ));
        }
    }

    // ------------------------------------------------------------------ commit guards

    #[test]
    fn proposal_guard_admits_exact_run_outputs_only() {
        let w = world();
        let o = w.outputs();
        let mut job = ready(&w, &o);
        let r1 = job
            .validate_proposal_commit(&w.head(), &o.run, &o.p1)
            .unwrap();
        assert_eq!(r1, proposal_ref(&o.p1));
        let r2 = job
            .validate_proposal_commit(&w.head(), &o.run, &o.p2)
            .unwrap();
        assert!(o.run.output_proposal_refs.contains(&r2));
        // Same stage, base and patch but another proposal (different confidence): rejected.
        let twin = w.proposal(w.accept_b(), Some(0.9));
        assert_eq!(twin.patch_set, o.p1.patch_set);
        assert!(matches!(
            job.validate_proposal_commit(&w.head(), &o.run, &twin),
            Err(StageJobError::ProposalMismatch("not an output of the run"))
        ));
        let foreign = w.proposal(w.add(req("req:z", Proposed, "Other.")), None);
        assert!(matches!(
            job.validate_proposal_commit(&w.head(), &o.run, &foreign),
            Err(StageJobError::ProposalMismatch("not an output of the run"))
        ));
        let wrong_stage = Proposal::new(
            StageId::S2,
            w.accept_b(),
            vec![],
            vec![],
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            Some(0.5),
        )
        .unwrap();
        assert!(matches!(
            job.validate_proposal_commit(&w.head(), &o.run, &wrong_stage),
            Err(StageJobError::ProposalMismatch("stage"))
        ));
        let mut wrong_base_set = w.accept_b();
        wrong_base_set.base_semantic_hash =
            "psg:sha256:1111111111111111111111111111111111111111111111111111111111111111"
                .parse()
                .unwrap();
        let wrong_base = w.proposal(wrong_base_set, Some(0.5));
        assert!(matches!(
            job.validate_proposal_commit(&w.head(), &o.run, &wrong_base),
            Err(StageJobError::ProposalMismatch("base_semantic_hash"))
        ));
        // A tampered proposal fails its own validation.
        let mut tampered = o.p1.clone();
        tampered.confidence = Some(0.1);
        assert!(matches!(
            job.validate_proposal_commit(&w.head(), &o.run, &tampered),
            Err(StageJobError::InvalidProposal(_))
        ));
        // Another (valid) run is not the job's completed run.
        let other_eval = StageEvaluation::new(None, vec![o.p1.clone()]).unwrap();
        let other_run = w.run(&w.ctx, StageId::S1, &other_eval);
        assert!(matches!(
            job.validate_proposal_commit(&w.head(), &other_run, &o.p1),
            Err(StageJobError::RunNotCompleted(_))
        ));
        assert_eq!(job.status(), Ready);
    }

    #[test]
    fn proposal_guard_marks_a_late_stale_job() {
        let mut w = world();
        let o = w.outputs();
        let mut job = ready(&w, &o);
        let rev2 = w.concurrent(&w.add(source("guard.md"))).revision;
        assert!(matches!(
            job.validate_proposal_commit(&rev2.id, &o.run, &o.p1),
            Err(StageJobError::Stale { .. })
        ));
        assert_eq!(job.status(), Stale);
        assert!(matches!(
            job.validate_proposal_commit(&w.rev1.id, &o.run, &o.p1),
            Err(StageJobError::JobNotReady(Stale))
        ));
        let mut queued = w.capture();
        assert!(matches!(
            queued.validate_proposal_commit(&w.rev1.id, &o.run, &o.p1),
            Err(StageJobError::JobNotReady(Queued))
        ));
    }

    #[test]
    fn derivation_guard() {
        let mut w = world();
        let o = w.outputs();
        let mut job = ready(&w, &o);
        assert_eq!(
            job.validate_derivation_commit(&w.head(), &o.run, &o.derivation)
                .unwrap(),
            patch_ref(&o.derivation)
        );
        assert_eq!(o.derivation.base_semantic_hash, *job.base_semantic_hash());
        let other = w.add(req(
            "req:d",
            Proposed,
            "The system shall list all requests.",
        ));
        assert!(matches!(
            job.validate_derivation_commit(&w.head(), &o.run, &other),
            Err(StageJobError::DerivationPatchMismatch(
                "not the run's derivation output"
            ))
        ));
        let mut wrong_base = o.derivation.clone();
        wrong_base.base_semantic_hash =
            "psg:sha256:1111111111111111111111111111111111111111111111111111111111111111"
                .parse()
                .unwrap();
        assert!(matches!(
            job.validate_derivation_commit(&w.head(), &o.run, &wrong_base),
            Err(StageJobError::DerivationPatchMismatch("base_semantic_hash"))
        ));
        // A run without derivation output admits no patch set.
        let no_derivation = StageEvaluation::new(None, vec![o.p1.clone()]).unwrap();
        let run = w.run(&w.ctx, StageId::S1, &no_derivation);
        let mut plain = evaluating(&w);
        plain.complete(&o.plan, &run, &w.head()).unwrap();
        assert!(matches!(
            plain.validate_derivation_commit(&w.head(), &run, &o.derivation),
            Err(StageJobError::DerivationPatchMismatch(
                "the run has no derivation output"
            ))
        ));
        // Head moved: stale.
        let rev2 = w.concurrent(&w.add(source("deriv.md"))).revision;
        assert!(matches!(
            job.validate_derivation_commit(&rev2.id, &o.run, &o.derivation),
            Err(StageJobError::Stale { .. })
        ));
        assert_eq!(job.status(), Stale);
    }

    // ------------------------------------------------------------------ store and recording

    #[test]
    fn guarded_store_commit_is_recorded() {
        let mut w = world();
        let o = w.outputs();
        let mut job = ready(&w, &o);
        let output = job
            .validate_proposal_commit(&w.head(), &o.run, &o.p1)
            .unwrap();
        let base = job.base_revision().clone();
        let result = w
            .store
            .commit(job.branch(), &base, &o.p1.patch_set, meta())
            .unwrap();
        // A result that is not a child of the base, or accepted another patch: rejected.
        let mut wrong_parent = result.clone();
        wrong_parent.revision.parent = Some(result.revision.id.clone());
        assert!(matches!(
            job.record_proposal_commit(&o.run, &o.p1, &wrong_parent),
            Err(StageJobError::CommitParentMismatch)
        ));
        let mut wrong_patch = result.clone();
        wrong_patch.revision.accepted_patch_ref = Some(patch_ref(&o.p2.patch_set));
        assert!(matches!(
            job.record_proposal_commit(&o.run, &o.p1, &wrong_patch),
            Err(StageJobError::CommitPatchMismatch)
        ));
        // The committed patch set must be the proposal's.
        assert!(matches!(
            job.record_proposal_commit(&o.run, &o.p2, &result),
            Err(StageJobError::CommitPatchMismatch)
        ));
        assert_eq!(job.status(), Ready);
        job.record_proposal_commit(&o.run, &o.p1, &result).unwrap();
        assert_eq!(job.status(), Committed);
        assert_eq!(job.committed_revision(), Some(&result.revision.id));
        assert_eq!(job.committed_output_ref(), Some(&output));
        assert_eq!(job.compile_run_ref(), Some(&o.run.id));
        job.validate().unwrap();
        let round =
            serde_json::from_value::<StageJob>(serde_json::to_value(&job).unwrap()).unwrap();
        assert_eq!(round, job);
        // Committed is terminal and admits nothing else, including the other proposal.
        invalid(job.start_evaluating(), Committed);
        invalid(job.mark_failed(), Committed);
        assert_eq!(job.observe_head(&result.revision.id), Committed);
        assert!(matches!(
            job.validate_proposal_commit(&result.revision.id, &o.run, &o.p2),
            Err(StageJobError::JobNotReady(Committed))
        ));
        assert!(matches!(
            job.record_proposal_commit(&o.run, &o.p2, &result),
            Err(StageJobError::JobNotReady(Committed))
        ));
    }

    #[test]
    fn guarded_derivation_commit_is_recorded() {
        let mut w = world();
        let o = w.outputs();
        let mut job = ready(&w, &o);
        job.validate_derivation_commit(&w.head(), &o.run, &o.derivation)
            .unwrap();
        let base = job.base_revision().clone();
        let result = w
            .store
            .commit(job.branch(), &base, &o.derivation, meta())
            .unwrap();
        assert_eq!(
            result.revision.accepted_patch_ref,
            Some(patch_ref(&o.derivation))
        );
        assert!(matches!(
            job.record_proposal_commit(&o.run, &o.p1, &result),
            Err(StageJobError::CommitPatchMismatch)
        ));
        job.record_derivation_commit(&o.run, &o.derivation, &result)
            .unwrap();
        assert_eq!(job.status(), Committed);
        assert_eq!(
            job.committed_output_ref(),
            o.run.output_derivation_patch_ref.as_ref()
        );
        assert_eq!(job.committed_revision(), Some(&result.revision.id));
    }

    #[test]
    fn store_cas_rejects_a_race_after_the_head_check() {
        let mut w = world();
        let o = w.outputs();
        let mut job = ready(&w, &o);
        // 1-2. The guard passes while the head is the base revision.
        job.validate_proposal_commit(&w.head(), &o.run, &o.p1)
            .unwrap();
        // 3. A concurrent writer advances the branch.
        let rev2 = w.concurrent(&w.add(source("race.md"))).revision;
        // 4. The old output is committed with the job's base as expected head.
        let base = job.base_revision().clone();
        let err = w
            .store
            .commit(job.branch(), &base, &o.p1.patch_set, meta())
            .unwrap_err();
        assert!(
            matches!(&err, StoreError::StaleBase { expected, actual, .. }
                if *expected == w.rev1.id && *actual == rev2.id),
            "{err:?}"
        );
        // No revision of the old output exists: the head is still the concurrent revision.
        assert_eq!(w.head(), rev2.id);
        let head = w.store.load_revision(&w.head()).unwrap();
        assert_ne!(
            head.revision.accepted_patch_ref,
            Some(patch_ref(&o.p1.patch_set))
        );
        assert!(head
            .graph
            .node(&id("req:b"))
            .is_some_and(|n| n.status == Proposed));
        // The job then observes the new head and refuses before any store write.
        assert_eq!(job.observe_head(&w.head()), Stale);
        assert!(matches!(
            job.validate_proposal_commit(&w.head(), &o.run, &o.p1),
            Err(StageJobError::JobNotReady(Stale))
        ));
    }

    // ------------------------------------------------------------------ unchanged wires

    #[test]
    fn proposal_and_compile_run_carry_no_job_metadata() {
        let w = world();
        let o = w.outputs();
        let wire = serde_json::to_value(&o.p1).unwrap();
        assert_eq!(
            wire.as_object().unwrap().keys().collect::<Vec<_>>(),
            [
                "acceptance_policy",
                "confidence",
                "derivation_refs",
                "evidence_refs",
                "id",
                "materiality",
                "patch_set",
                "stage"
            ]
        );
        for field in ["base_revision", "branch", "job_ref"] {
            assert!(wire.get(field).is_none());
            assert!(wire["patch_set"].get(field).is_none());
        }
        let run = serde_json::to_value(&o.run).unwrap();
        assert_eq!(
            run.as_object().unwrap().keys().collect::<Vec<_>>(),
            [
                "compiler_version",
                "config_hash",
                "deterministic_artifacts",
                "id",
                "inference_artifacts",
                "input_revision",
                "input_semantic_hash",
                "output_derivation_patch_ref",
                "output_hash",
                "output_proposal_refs",
                "profile_hash",
                "profile_ref",
                "rule_pack_hash",
                "stage",
                "validation_artifacts"
            ]
        );
    }

    // ------------------------------------------------------------------ source guards

    #[test]
    fn job_source_guards() {
        let code: String = JOB_SRC
            .lines()
            .map(|line| line.split("//").next().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");
        for token in [
            "SqliteRevisionStore",
            ".commit(",
            "move_head",
            "create_branch",
            "create_initial_revision",
            "rusqlite",
            "tokio",
            "spawn",
            "thread",
            "channel",
            "sleep",
            "Future",
            "async fn",
            "Clock",
            "SystemTime",
            "Instant",
            "rand",
            "Uuid",
            "reqwest",
            "std::net",
            "std::fs",
            "File",
            "PathBuf",
            "InferenceProvider",
            "AnthropicProvider",
            "ProviderExecution",
            "plumb_intake",
            "intake(",
            "coverage",
            "Jaccard",
            "reason_codes",
            "validate_required_rationale",
            "semantic_hash()? ==",
            "rebase",
        ] {
            assert!(!code.contains(token), "job.rs contains {token:?}");
        }
        // Staleness compares revision IDs, never semantic hashes.
        assert!(code.contains("current_head == &self.base_revision"));
        assert!(code.contains("current_head != &self.base_revision"));
    }
}
