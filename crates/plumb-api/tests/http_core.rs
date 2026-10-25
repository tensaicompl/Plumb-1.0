//! A0.2 contract tests for the core HTTP runtime (Hotfix 063, Hotfix 064), with the post-A0.3
//! routing and source-guard regression (Hotfix 065), extended to the two U2.1 semantic change
//! previews (Hotfix 073: 74 routed operations; Hotfix 074 route-guard exception).
//!
//! Every test lives in `http_core` (`cargo test -p plumb-api http_core`) and drives the real
//! Axum router with `tower::ServiceExt` over a temporary SQLite revision and artifact store.
//! Gate reports come from the real validation evaluators over the stored head graph.

mod http_core {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{header, Method, Request, StatusCode};
    use axum::Router;
    use plumb_api::openapi::{DesignEvent, OPERATIONS};
    use plumb_api::{router, router_with_ui, ApiIdentity, ApiState, ApiStateError};
    use plumb_artifacts::{ArtifactStore, SqliteArtifactStore};
    use plumb_core::{to_canonical_json, Clock, FixedClock, GateId, Hash, Id, Timestamp};
    use plumb_functional::waiver::create_waiver;
    use plumb_patch::apply_patch;
    use plumb_patch::{PatchSet, SemanticPatch};
    use plumb_psg::{source_artifact_id, Edge, Graph, Node};
    use plumb_store::{BranchName, RevisionWriteMeta, SqliteRevisionStore};
    use plumb_validation::model::WaiverPolicy;
    use plumb_validation::waiver::WaiverRequest;
    use plumb_validation::{
        load_builtin_software_profile, rules, EvaluatorRegistry, F1RequirementValidationInput,
        F1ValidationInputs, F4ApplicabilityInput, F4ValidationInputs, GateReport, RuleClass,
        ValidationContext, ValidationPolicy, ValidationProfile, ValidationRegistry,
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const T0: &str = "2026-01-01T00:00:00.000000000Z";
    const T1: &str = "2026-03-01T09:00:00.000000000Z";
    const T2: &str = "2026-04-01T09:00:00.000000000Z";
    const NOW: &str = "2026-05-01T12:00:00.000000000Z";
    const ACTOR: &str = "agent:api";
    const DOCX: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.docx");
    const DOCX_MIME: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    /// Every route module registered by `routes/mod.rs`.
    const ROUTE_SOURCES: [(&str, &str); 12] = [
        ("mod.rs", include_str!("../src/routes/mod.rs")),
        ("meta.rs", include_str!("../src/routes/meta.rs")),
        ("sources.rs", include_str!("../src/routes/sources.rs")),
        (
            "requirements.rs",
            include_str!("../src/routes/requirements.rs"),
        ),
        ("model.rs", include_str!("../src/routes/model.rs")),
        ("gates.rs", include_str!("../src/routes/gates.rs")),
        ("sentences.rs", include_str!("../src/routes/sentences.rs")),
        ("questions.rs", include_str!("../src/routes/questions.rs")),
        ("proposals.rs", include_str!("../src/routes/proposals.rs")),
        ("sessions.rs", include_str!("../src/routes/sessions.rs")),
        ("scenarios.rs", include_str!("../src/routes/scenarios.rs")),
        ("stages.rs", include_str!("../src/routes/stages.rs")),
    ];

    // ------------------------------------------------------------------ graph builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn audit() -> Value {
        json!({"created_by": "agent:analyst", "created_at": T0, "updated_by": null, "updated_at": null})
    }

    fn node(node_id: &str, status: &str, tag: &str, data: Value) -> Node {
        serde_json::from_value(json!({
            "id": node_id, "revision": 1, "status": status,
            "payload": {"type": tag, "data": data},
            "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
            "audit": audit()
        }))
        .unwrap_or_else(|e| panic!("{node_id}: {e}"))
    }

    fn edge(edge_id: &str, kind: &str, from: &str, to: &str) -> Edge {
        serde_json::from_value(json!({
            "id": edge_id, "revision": 1, "status": "Accepted", "kind": kind,
            "from": from, "to": to, "properties": {}, "evidence": [], "derivations": [],
            "standards": [], "audit": audit()
        }))
        .unwrap_or_else(|e| panic!("{edge_id}: {e}"))
    }

    fn requirement(node_id: &str, status: &str, kind: &str, statement: &str) -> Node {
        node(
            node_id,
            status,
            "Requirement",
            json!({
                "statement": statement, "requirement_kind": kind, "level": "system",
                "modality": "shall", "title": null, "rationale": null, "priority": null,
                "source_identifier": null, "verification_method": null, "owner_refs": null,
                "stakeholder_refs": null
            }),
        )
    }

    fn term(node_id: &str, status: &str, term: &str, aliases: Option<Vec<&str>>) -> Node {
        node(
            node_id,
            status,
            "Term",
            json!({"term": term, "language": "en", "definition_ref": null, "aliases": aliases, "status": null}),
        )
    }

    fn attribute(node_id: &str, name: &str, value_type: &str, class: Option<&str>) -> Node {
        node(
            node_id,
            "Accepted",
            "Attribute",
            json!({
                "name": name, "value_type": value_type, "nullable": false, "unit": null,
                "precision": null, "enum_values": null, "data_classification": class
            }),
        )
    }

    fn calculation(node_id: &str, status: &str, name: &str) -> Node {
        node(
            node_id,
            status,
            "Calculation",
            json!({
                "name": name, "expression": "1 + 1", "result_type": "Int", "unit": null,
                "rounding": null, "calendar_ref": null, "examples": null
            }),
        )
    }

    fn finding(
        node_id: &str,
        code: &str,
        family: &str,
        severity: &str,
        status: &str,
        affected: &[&str],
    ) -> Node {
        node(
            node_id,
            "Accepted",
            "Finding",
            json!({
                "code": code, "family": family, "severity": severity, "message": "Finding.",
                "status": status, "affected_refs": affected, "standard_rule_ref": null,
                "suggested_resolution": null, "waiver_ref": null
            }),
        )
    }

    fn assumption(node_id: &str, default_value: Value) -> Node {
        node(
            node_id,
            "Accepted",
            "Assumption",
            json!({
                "statement": "Half days count as 0.5.", "owner_ref": "agent:alice",
                "status": "Open", "finding_ref": null, "default_value": default_value,
                "expires_at": null, "risk_ref": null
            }),
        )
    }

    fn decision(node_id: &str, kind: &str, by: &str, at: &str) -> Node {
        node(
            node_id,
            "Accepted",
            "ResolutionDecision",
            json!({
                "question_ref": null, "proposal_ref": "prop:0000000000000001",
                "answer": {"kind": kind}, "decided_by": by, "decided_at": at,
                "patch_ref": Hash::content_sha256(node_id.as_bytes()), "rationale": "Agreed.",
                "supersedes": null
            }),
        )
    }

    /// The fixture PSG: requirements with coverage, terms, a domain, calculations, a process,
    /// findings, an assumption and decisions.
    fn fixture_parts() -> (Vec<Node>, Vec<Edge>) {
        let nodes = vec![
            requirement(
                "req:a",
                "Accepted",
                "functional",
                "The system shall export PDF, CSV, etc.",
            ),
            requirement(
                "req:b",
                "Accepted",
                "functional",
                "Managers shall approve leave requests.",
            ),
            requirement(
                "req:c",
                "Accepted",
                "operational",
                "Managers shall approve leave requests.",
            ),
            requirement(
                "req:p",
                "Proposed",
                "functional",
                "Employees shall cancel leave requests.",
            ),
            node(
                "op:submit",
                "Accepted",
                "Operation",
                json!({
                    "name": "SubmitLeave", "operation_kind": "command", "input_schema_ref": null,
                    "output_schema_ref": null, "preconditions": null, "postconditions": null,
                    "idempotency": null, "transaction_semantics": null
                }),
            ),
            node(
                "actor:employee",
                "Accepted",
                "Actor",
                json!({"name": "Employee", "actor_kind": "human"}),
            ),
            node(
                "proc:leave",
                "Accepted",
                "Process",
                json!({"name": "Leave", "description": null, "process_kind": null}),
            ),
            node(
                "pn:start",
                "Accepted",
                "ProcessNode",
                json!({
                    "process_ref": "proc:leave", "node_kind": "start", "operation_ref": null,
                    "condition_expr": null, "message_ref": null, "timer_expr": null
                }),
            ),
            node(
                "pn:task",
                "Accepted",
                "ProcessNode",
                json!({
                    "process_ref": "proc:leave", "node_kind": "human_task", "operation_ref": "op:submit",
                    "condition_expr": null, "message_ref": null, "timer_expr": null
                }),
            ),
            node(
                "pn:end",
                "Accepted",
                "ProcessNode",
                json!({
                    "process_ref": "proc:leave", "node_kind": "end", "operation_ref": null,
                    "condition_expr": null, "message_ref": null, "timer_expr": null
                }),
            ),
            node(
                "scn:a",
                "Accepted",
                "Scenario",
                json!({
                    "name": "Submit", "scenario_kind": "boundary", "given": [],
                    "when": {"operation": "op:submit"}, "then": [], "requirement_refs": ["req:a"],
                    "derived_from_refs": null, "confirmation_status": null
                }),
            ),
            term(
                "term:leave",
                "Accepted",
                "leave request",
                Some(vec!["absence"]),
            ),
            term("term:ot", "Proposed", "overtime", None),
            node(
                "concept:decimal",
                "Accepted",
                "Concept",
                json!({
                    "name": "Decimal", "definition": "A decimal number.", "concept_kind": "value_type"
                }),
            ),
            node(
                "entity:request",
                "Accepted",
                "Entity",
                json!({"name": "LeaveRequest", "description": null, "aggregate_root": null}),
            ),
            node(
                "entity:employee",
                "Accepted",
                "Entity",
                json!({"name": "Employee", "description": null, "aggregate_root": null}),
            ),
            attribute("attr:days", "days", "Integer", None),
            attribute("attr:salary", "salary", "Decimal", Some("financial")),
            node("state:draft", "Accepted", "State", json!({"name": "Draft"})),
            node(
                "state:submitted",
                "Accepted",
                "State",
                json!({"name": "Submitted"}),
            ),
            node(
                "trans:submit",
                "Accepted",
                "Transition",
                json!({
                    "stateful_ref": "entity:request", "from_state": "state:draft",
                    "to_state": "state:submitted", "guard_expr": null, "effect_refs": null
                }),
            ),
            node(
                "domainrel:req-emp",
                "Accepted",
                "DomainRelationship",
                json!({
                    "from_entity": "entity:request", "to_entity": "entity:employee",
                    "relationship_kind": "association", "cardinality_from": "*", "cardinality_to": "1",
                    "name": null, "snapshot_semantics": null, "ownership": null
                }),
            ),
            calculation("calc:days", "Accepted", "Days"),
            calculation("calc:p", "Proposed", "Pending"),
            finding(
                "finding:open",
                "PLUMB.F2.X",
                "F2",
                "error",
                "Open",
                &["req:a"],
            ),
            finding(
                "finding:closed",
                "PLUMB.F1.Y",
                "F1",
                "warn",
                "Resolved",
                &["req:b"],
            ),
            assumption("assumption:a", json!(5)),
            assumption("assumption:b", json!("manual")),
            node(
                "proc:broken",
                "Accepted",
                "Process",
                json!({"name": "Broken", "description": null, "process_kind": null}),
            ),
            node(
                "pn:orphan",
                "Accepted",
                "ProcessNode",
                json!({
                    "process_ref": "proc:broken", "node_kind": "human_task", "operation_ref": "op:submit",
                    "condition_expr": null, "message_ref": null, "timer_expr": null
                }),
            ),
            decision("decision:d1", "cardinality", "agent:alice", T1),
            decision("decision:d2", "trigger", "agent:bob", T2),
            // The decisions' and the assumptions' human Agents (graph-bound payload references,
            // architecture §33.11).
            node(
                "agent:alice",
                "Accepted",
                "Agent",
                json!({"agent_kind": "human"}),
            ),
            node(
                "agent:bob",
                "Accepted",
                "Agent",
                json!({"agent_kind": "human"}),
            ),
        ];
        let edges = vec![
            edge("rel:spec-op", "specified_by", "req:a", "op:submit"),
            edge("rel:spec-proc", "specified_by", "req:a", "proc:leave"),
            edge("rel:next-1", "next", "pn:start", "pn:task"),
            edge("rel:next-2", "next", "pn:task", "pn:end"),
            edge("rel:perf", "performed_by", "op:submit", "actor:employee"),
            edge("rel:perf-task", "performed_by", "pn:task", "actor:employee"),
            edge(
                "rel:attr-days",
                "has_attribute",
                "entity:request",
                "attr:days",
            ),
            edge(
                "rel:attr-salary",
                "has_attribute",
                "entity:request",
                "attr:salary",
            ),
            edge(
                "rel:state-draft",
                "has_state",
                "entity:request",
                "state:draft",
            ),
            edge(
                "rel:state-submitted",
                "has_state",
                "entity:request",
                "state:submitted",
            ),
            edge("rel:via", "transitions_via", "trans:submit", "op:submit"),
            edge("rel:res-1", "resolves", "decision:d1", "finding:closed"),
            edge("rel:res-2", "resolves", "decision:d2", "finding:closed"),
        ];
        (nodes, edges)
    }

    fn graph_of(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn fixture_graph() -> Graph {
        let (nodes, edges) = fixture_parts();
        graph_of(nodes, edges)
    }

    // ------------------------------------------------------------------ gate reports

    fn profile() -> ValidationProfile {
        load_builtin_software_profile().unwrap()
    }

    fn base_context(graph: &Graph, metadata: &ValidationRegistry) -> ValidationContext {
        ValidationContext::new(
            graph,
            metadata,
            ValidationPolicy::default(),
            graph.evidence_hash().unwrap(),
            vec![],
            vec![],
        )
        .unwrap()
    }

    /// The real report of `gate` over `graph` (I0, F1 or F4).
    fn gate_report(graph: &Graph, gate: GateId) -> GateReport {
        let metadata = ValidationRegistry::new(profile()).unwrap();
        let mut registry = EvaluatorRegistry::new(metadata.clone());
        rules::register_i0_evaluators(&mut registry).unwrap();
        rules::register_f1_evaluators(&mut registry).unwrap();
        rules::register_f4_evaluators(&mut registry).unwrap();
        let ctx = base_context(graph, &metadata);
        let ctx = match gate {
            GateId::F1 => ctx.with_f1_inputs(graph, f1_inputs(graph)).unwrap(),
            GateId::F4 => ctx
                .with_f4_inputs(
                    graph,
                    F4ValidationInputs::new(
                        F4ApplicabilityInput::default(),
                        vec![],
                        vec![],
                        vec![],
                    ),
                )
                .unwrap(),
            _ => ctx,
        };
        let report = registry.evaluate_gate(gate, graph, &ctx).unwrap();
        report.validate().unwrap();
        report
    }

    fn f1_inputs(graph: &Graph) -> F1ValidationInputs {
        let requirements = graph
            .nodes()
            .values()
            .filter(|n| n.status == plumb_psg::ElementStatus::Accepted)
            .filter_map(|n| match &n.payload {
                plumb_psg::NodePayload::Requirement(r) => Some((n, r)),
                _ => None,
            })
            .map(|(n, r)| F1RequirementValidationInput {
                requirement_ref: n.id.clone(),
                vocabulary_dependencies: vec![],
                lint_input: serde_json::from_value(json!({
                    "requirement_ref": n.id, "statement": r.statement,
                    "evidence_refs": n.evidence.iter().map(|e| e.as_id().clone()).collect::<Vec<_>>(),
                    "source_anchor": null, "term_context": null
                }))
                .unwrap(),
            })
            .collect();
        F1ValidationInputs::new(requirements, vec![], Default::default())
    }

    // ------------------------------------------------------------------ app fixture

    struct App {
        _dir: tempfile::TempDir,
        path: PathBuf,
        state: ApiState,
        router: Router,
    }

    fn open_store(path: &Path) -> SqliteRevisionStore {
        SqliteRevisionStore::open(path).unwrap()
    }

    fn meta(at: &str) -> RevisionWriteMeta {
        RevisionWriteMeta {
            decision_refs: vec![],
            created_by: id("agent:other"),
            created_at: ts(at),
        }
    }

    /// Initializes a store with `graph` and builds the router; `gates` selects which real
    /// reports over the stored head are registered.
    fn app_with(graph: Graph, gates: &[GateId], clock: Arc<dyn Clock + Send + Sync>) -> App {
        app_with_profile(graph, gates, clock, profile())
    }

    /// [`app_with`] governed by one explicit `profile` (Hotfix 080): the same validated profile
    /// supplies the initial revision's profile and rule-pack hashes and the `ApiState` profile.
    fn app_with_profile(
        graph: Graph,
        gates: &[GateId],
        clock: Arc<dyn Clock + Send + Sync>,
        profile: ValidationProfile,
    ) -> App {
        profile.validate().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plumb.sqlite");
        let mut store = open_store(&path);
        let rev = store
            .create_initial_revision(
                &graph,
                profile.profile_hash().unwrap(),
                profile.rule_pack_hash().unwrap(),
                meta(T0),
            )
            .unwrap();
        let head = store.load_revision(&rev.id).unwrap();
        let reports = gates.iter().map(|g| gate_report(&head.graph, *g)).collect();
        let artifacts = SqliteArtifactStore::open(&path).unwrap();
        let state = state_with_profile(store, artifacts, clock, reports, profile);
        let router = router(state.clone());
        App {
            _dir: dir,
            path,
            state,
            router,
        }
    }

    fn state_with(
        store: SqliteRevisionStore,
        artifacts: SqliteArtifactStore,
        clock: Arc<dyn Clock + Send + Sync>,
        reports: Vec<GateReport>,
    ) -> ApiState {
        state_with_profile(store, artifacts, clock, reports, profile())
    }

    fn state_with_profile(
        store: SqliteRevisionStore,
        artifacts: SqliteArtifactStore,
        clock: Arc<dyn Clock + Send + Sync>,
        reports: Vec<GateReport>,
        profile: ValidationProfile,
    ) -> ApiState {
        let identity = ApiIdentity::new(
            "alice".into(),
            id(ACTOR),
            vec!["analyst".into(), "reviewer".into()],
            "pilot".into(),
        )
        .unwrap();
        ApiState::new(
            BranchName::main(),
            store,
            artifacts,
            identity,
            clock,
            reports,
            profile,
        )
        .unwrap()
    }

    fn app() -> App {
        app_with(
            fixture_graph(),
            &[GateId::I0, GateId::F1, GateId::F4],
            fixed(),
        )
    }

    /// Uploads a Markdown source through the API, then rebuilds the state over the same
    /// database with real I0, F1 and F4 reports for the new head, so gate-dependent reads
    /// stay current.
    async fn app_with_source(file: &str, body: &[u8]) -> (App, String) {
        let app = app();
        let r = upload(&app, Some(&etag(&app)), Some(file), "text/markdown", body).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        let source = r.json()["id"].as_str().unwrap().to_owned();
        let App { _dir, path, .. } = app;
        let store = open_store(&path);
        let head = store
            .load_revision(&store.head(&BranchName::main()).unwrap().unwrap())
            .unwrap();
        let reports = [GateId::I0, GateId::F1, GateId::F4]
            .iter()
            .map(|g| gate_report(&head.graph, *g))
            .collect();
        let artifacts = SqliteArtifactStore::open(&path).unwrap();
        let state = state_with(store, artifacts, fixed(), reports);
        let router = router(state.clone());
        (
            App {
                _dir,
                path,
                state,
                router,
            },
            source,
        )
    }

    fn fixed() -> Arc<dyn Clock + Send + Sync> {
        Arc::new(FixedClock::new(ts(NOW)))
    }

    struct Reply {
        status: StatusCode,
        headers: axum::http::HeaderMap,
        bytes: Vec<u8>,
    }

    impl Reply {
        fn json(&self) -> Value {
            serde_json::from_slice(&self.bytes)
                .unwrap_or_else(|e| panic!("{e}: {:?}", String::from_utf8_lossy(&self.bytes)))
        }

        fn code(&self) -> String {
            self.json()["code"].as_str().unwrap_or_default().to_owned()
        }

        fn etag(&self) -> Option<String> {
            self.headers
                .get("etag")
                .map(|v| v.to_str().unwrap().to_owned())
        }
    }

    /// Sends `request` as a loopback client does: a request without a `Host` header gets the
    /// loopback `Host` a real HTTP/1.1 client always sends (architecture §33.8).
    async fn send(router: &Router, mut request: Request<Body>) -> Reply {
        if !request.headers().contains_key(header::HOST) {
            request
                .headers_mut()
                .insert(header::HOST, "127.0.0.1:8080".parse().unwrap());
        }
        send_exact(router, request).await
    }

    /// Sends `request` with exactly its own headers.
    async fn send_exact(router: &Router, request: Request<Body>) -> Reply {
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        Reply {
            status,
            headers,
            bytes,
        }
    }

    async fn get(app: &App, uri: &str) -> Reply {
        send(&app.router, Request::get(uri).body(Body::empty()).unwrap()).await
    }

    async fn post(app: &App, uri: &str, if_match: Option<&str>, body: Option<Value>) -> Reply {
        let mut request = Request::post(uri);
        if let Some(value) = if_match {
            request = request.header("if-match", value);
        }
        let body = match body {
            Some(v) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(serde_json::to_vec(&v).unwrap())
            }
            None => Body::empty(),
        };
        send(&app.router, request.body(body).unwrap()).await
    }

    fn multipart(filename: Option<&str>, media_type: &str, bytes: &[u8]) -> (String, Vec<u8>) {
        let boundary = "plumb-boundary-7f3a";
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(name) => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n")
                    .as_bytes(),
            ),
            None => body.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"\r\n"),
        }
        body.extend_from_slice(format!("Content-Type: {media_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        (format!("multipart/form-data; boundary={boundary}"), body)
    }

    async fn upload(
        app: &App,
        if_match: Option<&str>,
        filename: Option<&str>,
        media_type: &str,
        bytes: &[u8],
    ) -> Reply {
        let (content_type, body) = multipart(filename, media_type, bytes);
        let mut request = Request::post("/api/sources").header(header::CONTENT_TYPE, content_type);
        if let Some(value) = if_match {
            request = request.header("if-match", value);
        }
        send(&app.router, request.body(Body::from(body)).unwrap()).await
    }

    /// The current head via a separate store connection.
    fn head(app: &App) -> plumb_store::LoadedRevision {
        let store = open_store(&app.path);
        let head = store.head(&BranchName::main()).unwrap().unwrap();
        store.load_revision(&head).unwrap()
    }

    fn semantic(app: &App) -> String {
        head(app).revision.semantic_hash.to_string()
    }

    /// The current If-Match value: the strong entity-tag of the head revision ID (§33.2).
    fn etag(app: &App) -> String {
        format!("\"{}\"", head(app).revision.id)
    }

    fn revision_count(app: &App) -> u64 {
        head(app).revision.id.version()
    }

    fn artifact_exists(app: &App, hash: &Hash) -> bool {
        SqliteArtifactStore::open(&app.path)
            .unwrap()
            .exists(hash)
            .unwrap()
    }

    /// The importer output the API produces for `bytes` (same actor and clock).
    fn imported(media_type: &str, name: &str, bytes: &[u8]) -> plumb_import::ImportedSource {
        let audit = plumb_import::ImportAudit {
            created_by: id(ACTOR),
            created_at: ts(NOW),
        };
        match media_type {
            "text/markdown" => plumb_import::import_markdown(name, bytes, &audit),
            "text/plain" => plumb_import::import_plain_text(name, bytes, &audit),
            _ => plumb_import::import_docx(name, bytes, &audit),
        }
        .unwrap()
    }

    /// A store writer outside the API advancing the branch with `patch`.
    fn external_commit(app: &App, patch: SemanticPatch) -> plumb_store::GraphRevision {
        let mut store = open_store(&app.path);
        let head = store.head(&BranchName::main()).unwrap().unwrap();
        let loaded = store.load_revision(&head).unwrap();
        store
            .commit(
                &BranchName::main(),
                &head,
                &PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch,
                },
                meta(T1),
            )
            .unwrap()
            .revision
    }

    /// A clock whose first read lets another store writer commit first: a real CAS race.
    struct RacingClock {
        path: std::sync::OnceLock<PathBuf>,
        reads: std::sync::atomic::AtomicUsize,
        fired: AtomicBool,
    }

    impl Clock for RacingClock {
        fn now(&self) -> Timestamp {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if !self.fired.swap(true, Ordering::SeqCst) {
                let mut store = open_store(self.path.get().unwrap());
                let head = store.head(&BranchName::main()).unwrap().unwrap();
                let loaded = store.load_revision(&head).unwrap();
                let patch = PatchSet {
                    base_semantic_hash: loaded.revision.semantic_hash.clone(),
                    patch: SemanticPatch::AddNode {
                        node: finding(
                            "finding:race",
                            "PLUMB.F2.R",
                            "F2",
                            "info",
                            "Open",
                            &["req:b"],
                        ),
                    },
                };
                store
                    .commit(&BranchName::main(), &head, &patch, meta(T1))
                    .unwrap();
            }
            ts(NOW)
        }
    }

    fn racing_app() -> (App, Arc<RacingClock>) {
        let clock = Arc::new(RacingClock {
            path: std::sync::OnceLock::new(),
            reads: std::sync::atomic::AtomicUsize::new(0),
            fired: AtomicBool::new(false),
        });
        let app = app_with(fixture_graph(), &[GateId::F1], clock.clone());
        clock.path.set(app.path.clone()).unwrap();
        (app, clock)
    }

    /// Independent request hash: generic SHA-256 of RFC 8785 {operation_id, path, query}.
    fn request_hash(operation_id: &str, path: Value, query: Value) -> Hash {
        Hash::content_sha256(
            &to_canonical_json(
                &json!({"operation_id": operation_id, "path": path, "query": query}),
            )
            .unwrap(),
        )
    }

    fn cursor_for(
        after_id: &str,
        request_hash: &Hash,
        revision: &str,
        semantic_hash: &str,
    ) -> String {
        hex::encode(
            to_canonical_json(&json!({
                "after_id": after_id, "request_hash": request_hash,
                "revision": revision, "semantic_hash": semantic_hash
            }))
            .unwrap(),
        )
    }

    fn ids_of(page: &Value) -> Vec<String> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_str().unwrap().to_owned())
            .collect()
    }

    fn sample_path(path: &str) -> String {
        path.replace("{id}", "x:1").replace("{stage}", "lint")
    }

    // ------------------------------------------------------------------ routing

    /// The fixture plus findings whose codes are exactly the given profile rule IDs (or an
    /// unknown code), each under a family that does not match its rule class.
    fn rule_class_graph(codes: &[(&str, &str)]) -> Graph {
        let (mut nodes, edges) = fixture_parts();
        for (node_id, code) in codes {
            nodes.push(finding(node_id, code, "F3", "info", "Open", &["req:a"]));
        }
        graph_of(nodes, edges)
    }

    /// `rule_class` of every listed finding, by ID.
    fn rule_classes(page: &Value) -> BTreeMap<String, Value> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["id"].as_str().unwrap().to_owned(),
                    f["rule_class"].clone(),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn findings_project_rule_class_from_the_shipped_profile() {
        let codes = [
            ("finding:std", "ISO29148.F1.REQ.STATEMENT_PRESENT"),
            ("finding:interop", "PPMN.I0.PROVENANCE.AGENT_IDENTIFIED"),
            ("finding:core", "PLUMB.I0.SOURCE.CONTENT_ADDRESSED"),
        ];
        let app = app_with(rule_class_graph(&codes), &[], fixed());
        let page = get(&app, "/api/findings").await;
        assert_eq!(page.status, StatusCode::OK);
        let page = page.json();
        let classes = rule_classes(&page);
        // Expected classes are read from the shipped profile metadata, not from the codes.
        let shipped = profile();
        for (node_id, code) in codes {
            let rule = shipped.rules.iter().find(|r| r.id == code).unwrap();
            assert_eq!(classes[node_id], json!(rule.rule_class.as_str()), "{code}");
        }
        assert_eq!(classes["finding:std"], json!("external_standard"));
        assert_eq!(classes["finding:interop"], json!("external_interop"));
        assert_eq!(classes["finding:core"], json!("plumb_core"));
        // Unknown/custom codes stay listed with rule_class null; family is the stored family.
        assert_eq!(classes["finding:open"], Value::Null);
        assert_eq!(classes["finding:closed"], Value::Null);
        for f in page["items"].as_array().unwrap() {
            let family = match f["id"].as_str().unwrap() {
                "finding:open" => "F2",
                "finding:closed" => "F1",
                _ => "F3",
            };
            assert_eq!(f["family"], json!(family));
        }
        // Existing filters are unchanged; there is no rule_class filter.
        assert_eq!(
            ids_of(
                &get(&app, "/api/findings?code=PLUMB.I0.SOURCE.CONTENT_ADDRESSED")
                    .await
                    .json()
            ),
            ["finding:core"]
        );
        assert_eq!(
            ids_of(
                &get(&app, "/api/findings?family=F3&status=Open")
                    .await
                    .json()
            ),
            ["finding:core", "finding:interop", "finding:std"]
        );
        assert_eq!(
            get(&app, "/api/findings?rule_class=plumb_core")
                .await
                .status,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn findings_project_policy_rule_classes_from_a_test_local_profile() {
        // The shipped pilot profile has no profile_policy or organization_policy rule; a
        // test-local clone reclassifies two existing rules and governs revision and state alike.
        let mut local = profile();
        for (rule_id, class) in [
            ("PLUMB.I0.EVIDENCE.LOCATABLE", RuleClass::ProfilePolicy),
            (
                "PLUMB.I0.EVIDENCE.HASH_MATCH",
                RuleClass::OrganizationPolicy,
            ),
        ] {
            let rule = local.rules.iter_mut().find(|r| r.id == rule_id).unwrap();
            assert_eq!(rule.rule_class, RuleClass::PlumbCore);
            rule.rule_class = class;
        }
        local.validate().unwrap();
        assert_ne!(
            local.profile_hash().unwrap(),
            profile().profile_hash().unwrap()
        );
        let codes = [
            ("finding:profile", "PLUMB.I0.EVIDENCE.LOCATABLE"),
            ("finding:org", "PLUMB.I0.EVIDENCE.HASH_MATCH"),
            ("finding:core", "PLUMB.I0.SOURCE.CONTENT_ADDRESSED"),
        ];
        let app = app_with_profile(rule_class_graph(&codes), &[], fixed(), local.clone());
        let revision = head(&app).revision;
        assert_eq!(revision.profile_hash, local.profile_hash().unwrap());
        assert_eq!(revision.rule_pack_hash, local.rule_pack_hash().unwrap());
        let classes = rule_classes(&get(&app, "/api/findings").await.json());
        assert_eq!(classes["finding:profile"], json!("profile_policy"));
        assert_eq!(classes["finding:org"], json!("organization_policy"));
        assert_eq!(classes["finding:core"], json!("plumb_core"));
        assert_eq!(classes["finding:open"], Value::Null);
    }

    #[tokio::test]
    async fn http_core_routes_all_81_operations() {
        let app = app();
        let mut routed: Vec<(&str, &str, &str)> = Vec::new();
        for spec in OPERATIONS.iter() {
            let method = Method::from_bytes(spec.method.as_bytes()).unwrap();
            let request = Request::builder()
                .method(method)
                .uri(sample_path(spec.path))
                .body(Body::empty())
                .unwrap();
            let reply = send(&app.router, request).await;
            // An unregistered operation is a bare 404, or a bare 405 when a registered sibling
            // shares the path shape and the Allow header does not list its method.
            let allow = reply
                .headers
                .get("allow")
                .map(|v| v.to_str().unwrap().to_owned())
                .unwrap_or_default();
            let absent = reply.bytes.is_empty()
                && (reply.status == StatusCode::NOT_FOUND
                    || (reply.status == StatusCode::METHOD_NOT_ALLOWED
                        && !allow.split(',').any(|m| m.trim() == spec.method)));
            assert!(
                ["A0.2", "A0.3", "A0.4", "U2.1", "H0.11", "B1", "B2", "S2"].contains(&spec.owner),
                "{}",
                spec.operation_id
            );
            assert!(
                !absent && reply.status != StatusCode::METHOD_NOT_ALLOWED,
                "{} {} unrouted",
                spec.owner,
                spec.operation_id
            );
            assert!(
                reply.status.is_success() || !reply.code().is_empty(),
                "{} {:?} {:?}",
                spec.operation_id,
                reply.status,
                String::from_utf8_lossy(&reply.bytes)
            );
            assert_ne!(reply.status.as_u16(), 501, "{}", spec.operation_id);
            routed.push((spec.owner, spec.operation_id, spec.method));
        }
        let count = |list: &[(&str, &str, &str)], owner: &str, m: Option<&str>| {
            list.iter()
                .filter(|(o, _, x)| *o == owner && m.is_none_or(|m| *x == m))
                .count()
        };
        assert_eq!(
            (
                count(&routed, "A0.2", None),
                count(&routed, "A0.2", Some("POST")),
                count(&routed, "A0.2", Some("GET"))
            ),
            (30, 9, 21)
        );
        assert_eq!(
            (
                count(&routed, "A0.3", None),
                count(&routed, "A0.3", Some("POST")),
                count(&routed, "A0.3", Some("GET"))
            ),
            (40, 27, 13)
        );
        // A0.4 (Hotfix 067) routes exactly run_stage and get_job.
        assert_eq!(
            (
                count(&routed, "A0.4", None),
                count(&routed, "A0.4", Some("POST")),
                count(&routed, "A0.4", Some("GET"))
            ),
            (2, 1, 1)
        );
        // U2.1 (Hotfix 073) routes exactly the two semantic change previews.
        assert_eq!(
            (
                count(&routed, "U2.1", None),
                count(&routed, "U2.1", Some("POST")),
                count(&routed, "U2.1", Some("GET"))
            ),
            (2, 2, 0)
        );
        // H0.11 routes the proposal list and discard.
        assert_eq!(
            (
                count(&routed, "H0.11", None),
                count(&routed, "H0.11", Some("POST")),
                count(&routed, "H0.11", Some("GET"))
            ),
            (2, 1, 1)
        );
        // B1 routes the Inbox proposal detail, set acceptance and confirmation: B2 routes the five UI pipeline operations.
        assert_eq!(
            (
                count(&routed, "B1", None),
                count(&routed, "B1", Some("POST")),
                count(&routed, "B1", Some("GET"))
            ),
            (3, 2, 1)
        );

        assert_eq!(
            (
                count(&routed, "B2", None),
                count(&routed, "B2", Some("POST")),
                count(&routed, "B2", Some("GET"))
            ),
            (5, 3, 2)
        );
        // S2 routes the Inbox refresh of a stale answer.
        assert_eq!(
            (
                count(&routed, "S2", None),
                count(&routed, "S2", Some("POST")),
                count(&routed, "S2", Some("GET"))
            ),
            (1, 1, 0)
        );
        assert_eq!(routed.len(), 85);
        // An unregistered path is the typed 404 (H0.11 replaces the former empty body).
        let r = get(&app, "/api/nowhere").await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::NOT_FOUND, "E_ROUTE_NOT_FOUND")
        );
    }

    // ------------------------------------------------------------------ reads

    /// The 20 ordinary A0.2 GETs (all A0.2 GETs except the WebSocket upgrade).
    const ORDINARY_GETS: [&str; 20] = [
        "/api/me",
        "/api/model/version",
        "/api/gates",
        "/api/sources/SOURCE",
        "/api/sources/SOURCE/spans",
        "/api/requirements",
        "/api/requirements/req:a",
        "/api/glossary",
        "/api/model/entities",
        "/api/model/entities/entity:request",
        "/api/model/calculations",
        "/api/processes",
        "/api/processes/proc:leave",
        "/api/findings",
        "/api/assumptions",
        "/api/decisions",
        "/api/requirements/req:a/readiness",
        "/api/readiness",
        "/api/model/export?format=functional",
        "/api/model/diff",
    ];

    #[tokio::test]
    async fn http_core_every_ordinary_get_carries_the_loaded_etag() {
        let (app, source) = app_with_source(
            "policy.md",
            b"# Policy\n\nLeave needs approval.\n\nHalf days count.\n",
        )
        .await;
        let expected = etag(&app);
        for uri in ORDINARY_GETS {
            let uri = uri.replace("SOURCE", &source);
            let r = get(&app, &uri).await;
            assert_eq!(
                r.status,
                StatusCode::OK,
                "{uri}: {:?}",
                String::from_utf8_lossy(&r.bytes)
            );
            assert_eq!(r.etag().as_deref(), Some(expected.as_str()), "{uri}");
        }
        let a02_gets = OPERATIONS
            .iter()
            .filter(|s| {
                s.owner == "A0.2" && s.method == "GET" && s.operation_id != "events_websocket"
            })
            .count();
        assert_eq!(a02_gets, ORDINARY_GETS.len());
    }

    /// The expected public projection of `report` (Hotfix 082), built independently of the
    /// route: report order, exact wire values, summary copied from the report.
    fn gate_json(report: &GateReport) -> Value {
        let ids = |ids: &[Id]| ids.iter().map(Id::to_string).collect::<Vec<_>>();
        let mut findings: Vec<String> = report.findings.iter().map(|f| f.id.to_string()).collect();
        findings.sort();
        let rules: Vec<Value> = report
            .rules
            .iter()
            .map(|r| {
                let applicability = match &r.applicability {
                    plumb_validation::Applicability::Applicable => {
                        json!({"state": "APPLICABLE", "reason": null})
                    }
                    plumb_validation::Applicability::NotApplicable { reason } => {
                        json!({"state": "NOT_APPLICABLE", "reason": reason})
                    }
                };
                json!({
                    "rule_id": r.rule_id,
                    "state": r.state.as_str(),
                    "declared_severity": r.declared_severity.as_str(),
                    "effective_severity": r.effective_severity.as_str(),
                    "applicability": applicability,
                    "targets": ids(&r.targets),
                    "evidence": r.evidence,
                    "semantic_condition_key": r.semantic_condition_key,
                    "finding_ref": r.finding_ref.as_ref().map(Id::to_string),
                    "waiver_ref": r.waiver_ref.as_ref().map(Id::to_string),
                    "error": r.error.as_ref().map(|e| json!({
                        "code": e.code, "message": e.message,
                        "targets": ids(&e.targets), "evidence": e.evidence
                    })),
                })
            })
            .collect();
        let waivers: Vec<Value> = report
            .waivers
            .iter()
            .map(|w| {
                json!({
                    "rule_id": w.rule_id, "finding_ref": w.finding_ref.to_string(),
                    "finding_key": w.finding_key.to_string(), "decision_ref": w.decision_ref.to_string()
                })
            })
            .collect();
        json!({
            "gate": report.gate.as_str(),
            "state": "current",
            "reason": null,
            "passed": report.result == plumb_validation::GateResult::Pass,
            "findings": findings,
            "rules": rules,
            "waivers": waivers,
            "summary": {
                "blocker_failed": report.summary.blocker_failed,
                "blocker_waived": report.summary.blocker_waived,
                "warnings": report.summary.warnings
            },
        })
    }

    /// The entries of a GET /api/gates body whose gate is current (H0.11: every NOW gate is
    /// listed; the others are `not_evaluated` or `stale`).
    fn current_gates(gates: &Value) -> Value {
        let all = gates.as_array().unwrap();
        let names: Vec<&str> = all.iter().map(|g| g["gate"].as_str().unwrap()).collect();
        assert_eq!(names, ["I0", "F1", "F2", "F3", "F4"]);
        Value::Array(
            all.iter()
                .filter(|g| g["state"] == "current")
                .cloned()
                .collect(),
        )
    }

    /// `graph` plus the Accepted Finding nodes of `report` (findings do not change the semantic
    /// hash).
    fn with_findings(graph: &Graph, report: &GateReport) -> Graph {
        let mut nodes: Vec<Node> = graph.nodes().values().cloned().collect();
        for f in &report.findings {
            let mut n = finding(f.id.as_str(), "x", "x", "info", "Open", &[]);
            n.payload = plumb_psg::NodePayload::Finding(f.payload.clone());
            nodes.push(n);
        }
        graph_of(nodes, graph.edges().values().cloned().collect())
    }

    #[tokio::test]
    async fn http_core_gates_project_governed_waivers_and_summary() {
        // A real F1 failure of a decision-required rule, persisted as an Accepted Finding and
        // waived by a governed ResolutionDecision of an Accepted human Agent.
        let (mut nodes, edges) = fixture_parts();
        nodes.push(node(
            "agent:reviewer",
            "Accepted",
            "Agent",
            json!({"agent_kind": "human"}),
        ));
        let base = graph_of(nodes, edges);
        let before = gate_report(&base, GateId::F1);
        let base = with_findings(&base, &before);
        let shipped = profile();
        let failed = before
            .rules
            .iter()
            .find(|r| {
                r.state == plumb_validation::RuleResultState::Fail
                    && shipped.rules.iter().any(|m| {
                        m.id == r.rule_id && m.waiver_policy == WaiverPolicy::DecisionRequired
                    })
            })
            .unwrap()
            .clone();
        let metadata = shipped
            .rules
            .iter()
            .find(|m| m.id == failed.rule_id)
            .unwrap();
        let created = create_waiver(
            &base,
            &WaiverRequest {
                rule: metadata,
                targets: &failed.targets,
                semantic_condition_key: failed.semantic_condition_key.as_deref().unwrap(),
                decided_by: &id("agent:reviewer"),
                decided_at: ts(T0),
                rationale: "Accepted for the pilot.",
            },
            &ValidationPolicy::default(),
        )
        .unwrap();
        let waived_graph = apply_patch(&base, &created.proposal.patch_set)
            .unwrap()
            .graph;
        let report = gate_report(&waived_graph, GateId::F1);
        report.validate().unwrap();
        assert_eq!(report.waivers.len(), 1);
        assert!(
            report.summary.blocker_waived + report.summary.warnings + report.summary.blocker_failed
                > 0
        );

        let app = app_with(waived_graph, &[GateId::F1], fixed());
        let gates = current_gates(&get(&app, "/api/gates").await.json());
        assert_eq!(gates.as_array().unwrap().len(), 1);
        let f1 = &gates[0];
        assert_eq!(*f1, gate_json(&report));
        // The waived rule: WAIVED (never PASS) with its Finding and ResolutionDecision.
        let rule = f1["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["rule_id"] == json!(failed.rule_id))
            .unwrap();
        assert_eq!(rule["state"], "WAIVED");
        let finding_ref = failed.finding_ref.as_ref().unwrap().to_string();
        assert_eq!(rule["finding_ref"], json!(finding_ref));
        assert_eq!(rule["waiver_ref"], json!(created.decision_ref.to_string()));
        assert_eq!(
            f1["waivers"],
            json!([{
                "rule_id": failed.rule_id,
                "finding_ref": finding_ref,
                "finding_key": report.waivers[0].finding_key.to_string(),
                "decision_ref": created.decision_ref.to_string()
            }])
        );
        // The summary is the report's own.
        assert_eq!(
            f1["summary"],
            json!({
                "blocker_failed": report.summary.blocker_failed,
                "blocker_waived": report.summary.blocker_waived,
                "warnings": report.summary.warnings
            })
        );
        assert_eq!(
            f1["passed"],
            report.result == plumb_validation::GateResult::Pass
        );
    }

    /// `get_me.project` (S1, UI-SPEC §5.9): the head's project ID, a display name humanised from
    /// it, and the database file the server opened.
    #[tokio::test]
    async fn me_reports_the_project() {
        let app = app();
        let me = get(&app, "/api/me").await.json();
        let project = &me["project"];
        assert_eq!(project["id"], PROJECT);
        assert_eq!(project["name"], "Pilot");
        let database = PathBuf::from(project["database"].as_str().unwrap());
        assert_eq!(
            database.canonicalize().unwrap(),
            app.path.canonicalize().unwrap()
        );
        assert_eq!(
            project.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["database", "id", "name"]
        );
    }

    #[tokio::test]
    async fn http_core_meta_and_gates() {
        let app = app();
        let mut me = get(&app, "/api/me").await.json();
        // The project (S1) is asserted by `me_reports_the_project`.
        assert!(me.as_object_mut().unwrap().remove("project").is_some());
        assert_eq!(
            me,
            json!({"user": "alice", "roles": ["analyst", "reviewer"], "mode": "pilot", "inference_available": true, "inference_model": null})
        );
        let version = get(&app, "/api/model/version").await.json();
        let h = head(&app).revision;
        assert_eq!(version["version"], h.id.to_string());
        assert_eq!(version["revision"], h.id.to_string());
        assert_eq!(version["hash"], h.semantic_hash.to_string());
        assert_eq!(version["semantic_hash"], h.semantic_hash.to_string());
        assert_eq!(version["evidence_hash"], h.evidence_hash.to_string());
        // The head revision's provenance (UI: "Updated ... by ...").
        assert_eq!(version["created_at"], h.created_at.to_string());
        assert_eq!(version["created_at"], T0);
        assert_eq!(version["created_by"], "agent:other");
        // Current reports in gate order; the current I0 FAIL is still current.
        let gates = current_gates(&get(&app, "/api/gates").await.json());
        let names: Vec<&str> = gates
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["gate"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["I0", "F1", "F4"]);
        // Hotfix 082: each entry is the projection of its current report, including the rule
        // results, waivers and summary.
        let graph = head(&app).graph;
        for (index, gate) in [GateId::I0, GateId::F1, GateId::F4].into_iter().enumerate() {
            assert_eq!(
                gates[index],
                gate_json(&gate_report(&graph, gate)),
                "{gate:?}"
            );
        }
        assert_eq!(gates[0]["passed"], false);
        assert_eq!(gates[2]["passed"], true);
        // Real PASS and FAIL rule results with their exact material.
        let rules: Vec<&Value> = gates
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["rules"].as_array().unwrap())
            .collect();
        let pass = rules.iter().find(|r| r["state"] == "PASS").unwrap();
        assert_eq!(
            pass["applicability"],
            json!({"state": "APPLICABLE", "reason": null})
        );
        assert_eq!(
            (&pass["finding_ref"], &pass["waiver_ref"], &pass["error"]),
            (&Value::Null, &Value::Null, &Value::Null)
        );
        let fail = gates[0]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["state"] == "FAIL")
            .unwrap();
        assert!(fail["finding_ref"].is_string());
        assert!(gates[0]["findings"]
            .as_array()
            .unwrap()
            .contains(&fail["finding_ref"]));
        assert_eq!(fail["waiver_ref"], Value::Null);
        assert_eq!(fail["error"], Value::Null);
        assert!(gates[0]["summary"]["blocker_failed"].as_u64().unwrap() >= 1);
        // A real NOT_APPLICABLE result carries its exact deterministic reason.
        let not_applicable = rules
            .iter()
            .find(|r| r["state"] == "NOT_APPLICABLE")
            .unwrap();
        assert_eq!(not_applicable["applicability"]["state"], "NOT_APPLICABLE");
        assert!(!not_applicable["applicability"]["reason"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(not_applicable["finding_ref"], Value::Null);
        // Identity validation.
        assert_eq!(
            ApiIdentity::new(" ".into(), id(ACTOR), vec![], "m".into()),
            Err(ApiStateError::BlankUser)
        );
        assert_eq!(
            ApiIdentity::new("u".into(), id(ACTOR), vec![], "".into()),
            Err(ApiStateError::BlankMode)
        );
        assert_eq!(
            ApiIdentity::new(
                "u".into(),
                id(ACTOR),
                vec!["b".into(), "a".into()],
                "m".into()
            ),
            Err(ApiStateError::UnsortedRoles)
        );
        assert_eq!(
            ApiIdentity::new(
                "u".into(),
                id(ACTOR),
                vec!["a".into(), "a".into()],
                "m".into()
            ),
            Err(ApiStateError::UnsortedRoles)
        );
    }

    #[tokio::test]
    async fn http_core_gate_report_freshness() {
        // No current reports: no gates, and the gate-report-dependent export conflicts.
        let bare = app_with(fixture_graph(), &[], fixed());
        assert_eq!(
            current_gates(&get(&bare, "/api/gates").await.json()),
            json!([])
        );
        let r = get(&bare, "/api/model/export?format=report").await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "E_GATE_REPORT_UNAVAILABLE")
        );
        // R0.4: requirement reads never require a current F1 report; they state explicitly
        // that lint is not current, so the empty lint projection is not a clean lint result.
        let list = get(&bare, "/api/requirements").await;
        assert_eq!(list.status, StatusCode::OK);
        let list = list.json();
        assert!(!list["items"].as_array().unwrap().is_empty());
        for item in list["items"].as_array().unwrap() {
            assert_eq!(item["lint_current"], json!(false), "{item}");
            assert_eq!(item["smells"], json!([]), "{item}");
            assert_eq!(item["readiness"], Value::Null, "{item}");
        }
        let detail = get(&bare, "/api/requirements/req:a").await;
        assert_eq!(detail.status, StatusCode::OK);
        let detail = detail.json();
        assert_eq!(detail["lint_current"], json!(false));
        assert_eq!(detail["smells"], json!([]));
        assert_eq!(detail["lint_findings"], json!([]));
        assert_eq!(detail["readiness"], Value::Null);
        // Stale and mismatched reports are never presented.
        let graph = fixture_graph();
        let current = gate_report(&graph, GateId::I0);
        let (mut nodes, edges) = fixture_parts();
        nodes.push(requirement(
            "req:z",
            "Accepted",
            "functional",
            "Another requirement shall exist.",
        ));
        let stale_semantic = gate_report(&graph_of(nodes, edges.clone()), GateId::I0);
        let (mut nodes, _) = fixture_parts();
        let content = Hash::content_sha256(b"other");
        nodes.push(node(source_artifact_id(&content).unwrap().as_str(), "Accepted", "SourceArtifact", json!({
            "source_kind": "markdown", "display_name": "o.md", "content_hash": content,
            "media_type": "text/markdown", "external_uri": null, "external_version": null,
            "producer": null, "created_at_source": null, "language": null, "classification": null
        })));
        let evidence_graph = graph_of(nodes, edges);
        assert_eq!(
            evidence_graph.semantic_hash().unwrap(),
            graph.semantic_hash().unwrap()
        );
        let stale_evidence = gate_report(&evidence_graph, GateId::I0);
        let mut wrong_profile = current.clone();
        wrong_profile.profile_id = id("profile:other");
        let mut wrong_profile_hash = current.clone();
        wrong_profile_hash.profile_hash = Hash::content_sha256(b"p");
        let mut wrong_pack = current.clone();
        wrong_pack.rule_pack_hash = Hash::content_sha256(b"r");
        for (label, report) in [
            ("semantic", stale_semantic),
            ("evidence", stale_evidence),
            ("profile", wrong_profile),
            ("profile hash", wrong_profile_hash),
            ("rule pack", wrong_pack),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("db.sqlite");
            let mut store = open_store(&path);
            store
                .create_initial_revision(
                    &graph,
                    profile().profile_hash().unwrap(),
                    profile().rule_pack_hash().unwrap(),
                    meta(T0),
                )
                .unwrap();
            let state = state_with(
                store,
                SqliteArtifactStore::open(&path).unwrap(),
                fixed(),
                vec![report],
            );
            let r = send(
                &router(state),
                Request::get("/api/gates").body(Body::empty()).unwrap(),
            )
            .await;
            assert_eq!(current_gates(&r.json()), json!([]), "{label}");
        }
        // Registration order does not matter.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let mut store = open_store(&path);
        store
            .create_initial_revision(
                &graph,
                profile().profile_hash().unwrap(),
                profile().rule_pack_hash().unwrap(),
                meta(T0),
            )
            .unwrap();
        let reports = vec![
            gate_report(&graph, GateId::F4),
            gate_report(&graph, GateId::F1),
            current,
        ];
        let state = state_with(
            store,
            SqliteArtifactStore::open(&path).unwrap(),
            fixed(),
            reports,
        );
        let r = send(
            &router(state),
            Request::get("/api/gates").body(Body::empty()).unwrap(),
        )
        .await;
        let names: Vec<String> = current_gates(&r.json())
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["gate"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["I0", "F1", "F4"]);
    }

    #[tokio::test]
    async fn http_core_requirement_projection_and_filters() {
        let app = app();
        let f1 = gate_report(&head(&app).graph, GateId::F1);
        let detail = get(&app, "/api/requirements/req:a").await.json();
        let mut lint: Vec<String> = f1
            .findings
            .iter()
            .filter(|f| f.payload.affected_refs.contains(&id("req:a")))
            .map(|f| f.id.to_string())
            .collect();
        lint.sort();
        assert!(!lint.is_empty());
        assert_eq!(
            detail,
            json!({
                "id": "req:a", "text": "The system shall export PDF, CSV, etc.", "class": "functional",
                "status": "Accepted", "smells": ["PLUMB.LINT.REQ.OPEN_LIST"],
                "coverage": {"operations": ["op:submit"], "scenarios": ["scn:a"]},
                "readiness": null, "spans": [], "evidence": [], "lint_findings": lint,
                "ears_suggestion": null,
                "ears_proposal_ref": null,
                "linked": {"operations": ["op:submit"], "processes": ["proc:leave"], "scenarios": ["scn:a"]},
                "lint_current": true
            })
        );
        let list = get(&app, "/api/requirements").await.json();
        assert_eq!(ids_of(&list), ["req:a", "req:b", "req:c", "req:p"]);
        assert_eq!(list["items"][3]["smells"], json!([]));
        assert_eq!(
            ids_of(&get(&app, "/api/requirements?status=Proposed").await.json()),
            ["req:p"]
        );
        assert_eq!(
            ids_of(
                &get(&app, "/api/requirements?class=operational")
                    .await
                    .json()
            ),
            ["req:c"]
        );
        assert_eq!(
            ids_of(&get(&app, "/api/requirements?q=MANAGERS").await.json()),
            ["req:b", "req:c"]
        );
        assert_eq!(
            ids_of(
                &get(&app, "/api/requirements?q=managers&class=functional")
                    .await
                    .json()
            ),
            ["req:b"]
        );
        for bad in [
            "/api/requirements?q=%20",
            "/api/requirements?status=proposed",
            "/api/requirements?class=bogus",
            "/api/requirements?other=1",
            "/api/requirements?q=a&q=b",
        ] {
            let r = get(&app, bad).await;
            assert_eq!(
                (r.status, r.code().as_str()),
                (StatusCode::BAD_REQUEST, "E_INVALID_REQUEST"),
                "{bad}"
            );
        }
        let r = get(&app, "/api/requirements/term:leave").await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::NOT_FOUND, "E_NOT_FOUND")
        );
        // Readiness is uncalculated.
        let readiness = json!({
            "overall": null,
            "sub": {"clarity": null, "completeness": null, "consistency": null, "verifiability": null, "confirmation": null},
            "contributing": []
        });
        assert_eq!(
            get(&app, "/api/requirements/req:a/readiness").await.json(),
            readiness
        );
        assert_eq!(
            get(&app, "/api/readiness?group=project").await.json(),
            readiness
        );
        assert_eq!(get(&app, "/api/readiness").await.json(), readiness);
        assert_eq!(
            get(&app, "/api/readiness?group=team").await.status,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            get(&app, "/api/requirements/req:none/readiness")
                .await
                .status,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn http_core_model_projections() {
        let app = app();
        assert_eq!(
            get(&app, "/api/model/entities/entity:request").await.json(),
            json!({
                "id": "entity:request", "name": "LeaveRequest",
                "attributes": [
                    {"id": "attr:days", "name": "days", "type": "Integer", "nullable": false, "unit": null, "class": null},
                    {"id": "attr:salary", "name": "salary", "type": "Decimal", "nullable": false, "unit": null, "class": "financial"}
                ],
                "relationships": [{"id": "domainrel:req-emp", "target": "entity:employee", "kind": "association", "cardinality": "*:1"}],
                "states": ["state:draft", "state:submitted"],
                "transitions": [{"from": "state:draft", "to": "state:submitted", "operation": "op:submit"}],
                "tags": []
            })
        );
        assert_eq!(
            ids_of(&get(&app, "/api/model/entities").await.json()),
            ["entity:employee", "entity:request"]
        );
        assert_eq!(
            get(&app, "/api/glossary").await.json()["items"][0],
            json!({"id": "term:leave", "term": "leave request", "language": "en", "aliases": ["absence"], "definition_ref": null, "status": "Accepted"})
        );
        assert_eq!(
            get(&app, "/api/model/calculations").await.json()["items"][0],
            json!({"id": "calc:days", "name": "Days", "expr": "1 + 1"})
        );
        // Processes use the existing analyzer.
        let leave = get(&app, "/api/processes/proc:leave").await.json();
        assert_eq!(leave["lanes"], json!(["actor:employee"]));
        assert_eq!(leave["steps"], json!(["pn:end", "pn:start", "pn:task"]));
        assert_eq!(leave["branches"], json!([]));
        // Hotfix 079: steps are only the sorted node IDs; nodes and flows carry the semantics.
        assert_eq!(
            leave["nodes"],
            json!([
                {"id": "pn:end", "kind": "end", "operation_ref": null, "performer_refs": [],
                 "consumes_refs": [], "produces_refs": [], "condition_expr": null,
                 "message_ref": null, "timer_expr": null},
                {"id": "pn:start", "kind": "start", "operation_ref": null, "performer_refs": [],
                 "consumes_refs": [], "produces_refs": [], "condition_expr": null,
                 "message_ref": null, "timer_expr": null},
                {"id": "pn:task", "kind": "human_task", "operation_ref": "op:submit",
                 "performer_refs": ["actor:employee"], "consumes_refs": [], "produces_refs": [],
                 "condition_expr": null, "message_ref": null, "timer_expr": null}
            ])
        );
        assert_eq!(
            leave["flows"],
            json!([{"from": "pn:start", "to": "pn:task"}, {"from": "pn:task", "to": "pn:end"}])
        );
        let graph = head(&app).graph;
        let analysis = plumb_validation::analyze_process(
            &graph,
            &id("proc:broken"),
            plumb_validation::ProcessAnalysisMode::AcceptedOnly,
        )
        .unwrap();
        assert!(analysis.missing_start() && analysis.missing_end());
        let broken = get(&app, "/api/processes/proc:broken").await.json();
        let validation: Vec<&str> = broken["validation"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(
            validation.contains(&"missing_start") && validation.contains(&"missing_end"),
            "{validation:?}"
        );
        let mut sorted = validation.clone();
        sorted.sort();
        assert_eq!(sorted, validation);
        // Findings with exact filters.
        assert_eq!(
            ids_of(&get(&app, "/api/findings").await.json()),
            ["finding:closed", "finding:open"]
        );
        assert_eq!(
            ids_of(&get(&app, "/api/findings?status=Open").await.json()),
            ["finding:open"]
        );
        assert_eq!(
            ids_of(&get(&app, "/api/findings?severity=warn").await.json()),
            ["finding:closed"]
        );
        assert_eq!(
            ids_of(&get(&app, "/api/findings?code=PLUMB.F2.X").await.json()),
            ["finding:open"]
        );
        assert_eq!(
            ids_of(
                &get(&app, "/api/findings?family=F1&status=Open")
                    .await
                    .json()
            ),
            Vec::<String>::new()
        );
        // Assumptions with scalar defaults.
        let assumptions = get(&app, "/api/assumptions").await.json();
        assert_eq!(assumptions["items"][0]["default_value"], json!(5.0));
        assert_eq!(assumptions["items"][1]["default_value"], json!("manual"));
        // Decisions with exact filters.
        assert_eq!(
            ids_of(&get(&app, "/api/decisions").await.json()),
            ["decision:d1", "decision:d2"]
        );
        assert_eq!(
            ids_of(&get(&app, "/api/decisions?by=agent:bob").await.json()),
            ["decision:d2"]
        );
        assert_eq!(
            ids_of(&get(&app, "/api/decisions?kind=cardinality").await.json()),
            ["decision:d1"]
        );
        assert_eq!(
            ids_of(
                &get(&app, "/api/decisions?since=2026-03-15T00:00:00Z")
                    .await
                    .json()
            ),
            ["decision:d2"]
        );
        assert_eq!(
            ids_of(
                &get(&app, &format!("/api/decisions?since={T1}"))
                    .await
                    .json()
            ),
            ["decision:d1", "decision:d2"]
        );
        assert_eq!(
            get(&app, "/api/decisions?since=yesterday").await.status,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            get(&app, "/api/decisions").await.json()["items"][0],
            json!({"id": "decision:d1", "proposal_ref": "prop:0000000000000001", "question_ref": null, "decided_by": "agent:alice", "decided_at": T1, "rationale": "Agreed."})
        );
    }

    /// The fixture plus a process with a message event, a gateway, inputs, outputs and
    /// several or no performers (Hotfix 079).
    fn event_process_app() -> App {
        let (mut nodes, mut edges) = fixture_parts();
        let pnode = |node_id: &str,
                     kind: &str,
                     operation: Value,
                     message: Value,
                     timer: Value,
                     condition: Value| {
            node(
                node_id,
                "Accepted",
                "ProcessNode",
                json!({
                    "process_ref": "proc:events", "node_kind": kind, "operation_ref": operation,
                    "condition_expr": condition, "message_ref": message, "timer_expr": timer
                }),
            )
        };
        nodes.extend([
            node(
                "proc:events",
                "Accepted",
                "Process",
                json!({"name": "Events", "description": null, "process_kind": null}),
            ),
            node(
                "actor:manager",
                "Accepted",
                "Actor",
                json!({"name": "Manager", "actor_kind": "human"}),
            ),
            node(
                "event:requested",
                "Accepted",
                "Event",
                json!({"name": "LeaveRequested", "payload_schema_ref": null, "semantic_type": null}),
            ),
            node(
                "outcome:approved",
                "Accepted",
                "Outcome",
                json!({"name": "Approved", "outcome_kind": "success"}),
            ),
            pnode("pe:0start", "start", Value::Null, Value::Null, Value::Null, Value::Null),
            pnode("pe:1wait", "message_event", Value::Null, json!("event:requested"), Value::Null, Value::Null),
            pnode("pe:2decide", "exclusive_gateway", Value::Null, Value::Null, Value::Null, json!("days > 0")),
            pnode("pe:3approve", "human_task", json!("op:submit"), Value::Null, Value::Null, Value::Null),
            pnode("pe:4timer", "timer_event", Value::Null, Value::Null, json!("P1D"), Value::Null),
            pnode("pe:5end", "end", Value::Null, Value::Null, Value::Null, Value::Null),
        ]);
        edges.extend([
            edge("rel:pe-n1", "next", "pe:0start", "pe:1wait"),
            edge("rel:pe-n2", "next", "pe:1wait", "pe:2decide"),
            edge("rel:pe-n3", "next", "pe:2decide", "pe:3approve"),
            edge("rel:pe-n4", "next", "pe:2decide", "pe:4timer"),
            edge("rel:pe-n5", "next", "pe:3approve", "pe:5end"),
            edge("rel:pe-n6", "next", "pe:4timer", "pe:5end"),
            edge("rel:pe-consumes", "consumes", "pe:1wait", "event:requested"),
            edge(
                "rel:pe-produces",
                "produces",
                "pe:3approve",
                "outcome:approved",
            ),
            edge(
                "rel:pe-perf-1",
                "performed_by",
                "pe:3approve",
                "actor:manager",
            ),
            edge(
                "rel:pe-perf-2",
                "performed_by",
                "pe:3approve",
                "actor:employee",
            ),
        ]);
        app_with(graph_of(nodes, edges), &[], fixed())
    }

    #[tokio::test]
    async fn http_core_process_structured_projection() {
        let app = event_process_app();
        let r = get(&app, "/api/processes/proc:events").await;
        assert_eq!(
            r.status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&r.bytes)
        );
        let p = r.json();
        // Lanes are the sorted unique performers; steps only the sorted node IDs.
        assert_eq!(p["lanes"], json!(["actor:employee", "actor:manager"]));
        assert_eq!(
            p["steps"],
            json!([
                "pe:0start",
                "pe:1wait",
                "pe:2decide",
                "pe:3approve",
                "pe:4timer",
                "pe:5end"
            ])
        );
        let ids: Vec<&str> = p["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "pe:0start",
                "pe:1wait",
                "pe:2decide",
                "pe:3approve",
                "pe:4timer",
                "pe:5end"
            ]
        );
        let node = |id: &str| {
            p["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|n| n["id"] == id)
                .unwrap()
                .clone()
        };
        assert_eq!(
            node("pe:1wait"),
            json!({"id": "pe:1wait", "kind": "message_event", "operation_ref": null,
                   "performer_refs": [], "consumes_refs": ["event:requested"], "produces_refs": [],
                   "condition_expr": null, "message_ref": "event:requested", "timer_expr": null})
        );
        assert_eq!(
            node("pe:3approve"),
            json!({"id": "pe:3approve", "kind": "human_task", "operation_ref": "op:submit",
                   "performer_refs": ["actor:employee", "actor:manager"], "consumes_refs": [],
                   "produces_refs": ["outcome:approved"], "condition_expr": null,
                   "message_ref": null, "timer_expr": null})
        );
        assert_eq!(node("pe:4timer")["kind"], "timer_event");
        assert_eq!(node("pe:4timer")["timer_expr"], "P1D");
        assert_eq!(node("pe:2decide")["kind"], "exclusive_gateway");
        assert_eq!(node("pe:2decide")["condition_expr"], "days > 0");
        assert_eq!(p["branches"], json!(["pe:2decide"]));
        // The active next topology, sorted by from then to.
        assert_eq!(
            p["flows"],
            json!([
                {"from": "pe:0start", "to": "pe:1wait"},
                {"from": "pe:1wait", "to": "pe:2decide"},
                {"from": "pe:2decide", "to": "pe:3approve"},
                {"from": "pe:2decide", "to": "pe:4timer"},
                {"from": "pe:3approve", "to": "pe:5end"},
                {"from": "pe:4timer", "to": "pe:5end"}
            ])
        );
        // Deterministic across reads.
        assert_eq!(get(&app, "/api/processes/proc:events").await.json(), p);
    }

    #[tokio::test]
    async fn http_core_non_scalar_assumption_default_is_reported() {
        let (mut nodes, edges) = fixture_parts();
        nodes.push(assumption("assumption:z", json!({"days": 1})));
        let app = app_with(graph_of(nodes, edges), &[], fixed());
        // H0.11: reported as the item's typed health marker; the other items are returned.
        let r = get(&app, "/api/assumptions").await;
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.json()["health"][0]["id"], "assumption:z");
        assert_eq!(
            r.json()["health"][0]["code"],
            "E_UNREPRESENTABLE_ASSUMPTION_DEFAULT"
        );
    }

    #[tokio::test]
    async fn http_core_exports_are_the_real_projection() {
        let app = app();
        let graph = head(&app).graph;
        let f4 = gate_report(&graph, GateId::F4);
        let outputs = plumb_functional::project_functional_pilot(&graph, &profile(), &f4).unwrap();
        for (format, expected) in [
            ("functional", &outputs.functional_yaml.bytes),
            ("report", &outputs.model_report_md.bytes),
            ("decisions", &outputs.decisions_jsonl.bytes),
            ("glossary", &outputs.glossary_md.bytes),
            // Fable #307: every artifact `report --out` writes is exportable.
            ("scenarios", &outputs.scenarios_yaml.bytes),
            ("gate_report", &outputs.gate_report_json.bytes),
        ] {
            let r = get(&app, &format!("/api/model/export?format={format}")).await;
            assert_eq!(r.status, StatusCode::OK);
            assert_eq!(&r.bytes, expected, "{format}");
            assert_eq!(
                r.headers.get(header::CONTENT_TYPE).unwrap(),
                "application/octet-stream"
            );
        }
        assert_eq!(
            get(&app, "/api/model/export?format=svg").await.status,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            get(&app, "/api/model/export").await.status,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn http_core_model_diff_by_revision() {
        let app = app();
        let first = head(&app).revision;
        let r = get(&app, "/api/model/diff").await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::UNPROCESSABLE_ENTITY, "E_DIFF_NO_PARENT")
        );
        let w = post(
            &app,
            "/api/model/attributes/attr:days/unit",
            Some(&etag(&app)),
            Some(json!({"unit": "day"})),
        )
        .await;
        assert_eq!(w.status, StatusCode::OK);
        let second = head(&app).revision;
        let diff = get(&app, "/api/model/diff").await.json();
        assert_eq!(
            diff,
            json!({"from": first.id.to_string(), "to": second.id.to_string(), "added": [], "removed": [], "modified": ["attr:days"]})
        );
        let explicit = get(
            &app,
            &format!("/api/model/diff?from={}&to={}", second.id, first.id),
        )
        .await
        .json();
        assert_eq!(explicit["modified"], json!(["attr:days"]));
        assert_eq!(explicit["from"], second.id.to_string());
        let r = get(&app, "/api/model/diff?from=rev:9:0000000000000000").await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::NOT_FOUND, "E_NOT_FOUND")
        );
        assert_eq!(
            get(&app, "/api/model/diff?to=psg:sha256:00").await.status,
            StatusCode::BAD_REQUEST
        );
        // A semantic hash is not a revision.
        let r = get(
            &app,
            &format!("/api/model/diff?to={}", second.semantic_hash),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
    }

    // ------------------------------------------------------------------ sources

    #[tokio::test]
    async fn http_core_source_uploads() {
        let app = app();
        let mut events = app.state.subscribe();
        let md: &[u8] = b"# Leave\n\nEmployees request leave.\n\nManagers approve leave.\n";
        let txt: &[u8] = b"Leave requests need approval.\n\nHalf days count as 0.5.\n";
        for (name, media, bytes) in [
            ("leave.md", "text/markdown", md),
            ("leave.txt", "text/plain", txt),
            ("requirements.docx", DOCX_MIME, DOCX),
        ] {
            let before = head(&app).revision;
            let expected = imported(media, name, bytes);
            let r = upload(&app, Some(&etag(&app)), Some(name), media, bytes).await;
            assert_eq!(r.status, StatusCode::OK, "{name}: {:?}", r.json());
            let body = r.json();
            let after = head(&app).revision;
            assert_eq!(after.parent, Some(before.id.clone()));
            assert_eq!(body["id"], expected.source.id.to_string());
            assert_eq!(body["spans_count"], expected.fragments.len());
            let plumb_psg::NodePayload::SourceArtifact(source) = &expected.source.payload else {
                panic!()
            };
            assert_eq!(body["hash"], source.content_hash.to_string());
            assert_eq!(body["version"], after.id.to_string());
            assert_eq!(body["revision"], after.id.to_string());
            assert_eq!(body["semantic_hash"], after.semantic_hash.to_string());
            // Evidence only: the semantic hash is unchanged while the revision moves.
            assert_eq!(after.semantic_hash, before.semantic_hash);
            assert!(artifact_exists(&app, &expected.original_artifact.hash));
            assert!(artifact_exists(&app, &expected.extracted_artifact.hash));
            let event = events.try_recv().unwrap();
            let mut ids: Vec<String> = std::iter::once(expected.source.id.to_string())
                .chain(expected.fragments.iter().map(|f| f.id.to_string()))
                .collect();
            ids.sort();
            assert_eq!(
                event,
                DesignEvent {
                    kind: "create_source".into(),
                    ids,
                    version: after.id.to_string()
                }
            );
            assert_eq!(body["events"], json!([event]));
            assert!(events.try_recv().is_err());
            let graph = head(&app).graph;
            assert!(!graph
                .node_ids_by_type(plumb_psg::NodeType::Requirement)
                .iter()
                .any(|r| !fixture_graph().node(r).is_some()));
            // Source and spans reads.
            let source_json = get(&app, &format!("/api/sources/{}", expected.source.id))
                .await
                .json();
            assert_eq!(
                source_json,
                json!({"id": expected.source.id.to_string(), "hash": source.content_hash.to_string(), "media_type": source.media_type, "source_kind": source.source_kind})
            );
            let spans = get(&app, &format!("/api/sources/{}/spans", expected.source.id))
                .await
                .json();
            let mut fragment_ids: Vec<String> = expected
                .fragments
                .iter()
                .map(|f| f.id.to_string())
                .collect();
            fragment_ids.sort();
            assert_eq!(ids_of(&spans), fragment_ids);
            let first = &spans["items"][0];
            assert_eq!(first["source_ref"], expected.source.id.to_string());
            assert!(first["locator_kind"].is_string());
        }
        // The same content again: the source exists.
        let r = upload(
            &app,
            Some(&etag(&app)),
            Some("leave.md"),
            "text/markdown",
            md,
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "E_SOURCE_EXISTS")
        );
        // Unsupported media, missing or blank filename.
        let r = upload(
            &app,
            Some(&etag(&app)),
            Some("x.pdf"),
            "application/pdf",
            b"%PDF",
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "E_UNSUPPORTED_MEDIA_TYPE"
            )
        );
        let r = upload(
            &app,
            Some(&etag(&app)),
            Some("x.md"),
            "text/x-markdown",
            b"# x",
        )
        .await;
        assert_eq!(r.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let r = upload(&app, Some(&etag(&app)), None, "text/markdown", b"# x").await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::BAD_REQUEST, "E_INVALID_REQUEST")
        );
        let r = upload(&app, Some(&etag(&app)), Some(" "), "text/markdown", b"# x").await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        // A failed precondition writes no artifact.
        let fresh: &[u8] = b"# Fresh\n\nNever stored.\n";
        let r = upload(
            &app,
            Some("psg:sha256:stale"),
            Some("fresh.md"),
            "text/markdown",
            fresh,
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::PRECONDITION_FAILED, "E_PRECONDITION_FAILED")
        );
        let not_stored = imported("text/markdown", "fresh.md", fresh);
        assert!(!artifact_exists(&app, &not_stored.original_artifact.hash));
        assert!(events.try_recv().is_err());
    }

    /// Fable #368 (H0.10) changes the A0.2 contract: the import artifacts are stored in the
    /// revision-commit transaction, so a lost race leaves none behind.
    #[tokio::test]
    async fn http_core_source_artifacts_are_not_left_by_a_lost_race() {
        let (app, clock) = racing_app();
        let before = revision_count(&app);
        let bytes: &[u8] = b"# Raced\n\nThis upload loses its race.\n";
        let r = upload(
            &app,
            Some(&etag(&app)),
            Some("raced.md"),
            "text/markdown",
            bytes,
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::PRECONDITION_FAILED, "E_PRECONDITION_FAILED")
        );
        let current = head(&app).revision;
        assert_eq!(current.id.version(), before + 1);
        assert_eq!(r.json()["revision"], current.id.to_string());
        let expected = imported("text/markdown", "raced.md", bytes);
        assert!(!artifact_exists(&app, &expected.original_artifact.hash));
        assert!(!artifact_exists(&app, &expected.extracted_artifact.hash));
        assert!(head(&app).graph.node(&expected.source.id).is_none());
        assert_eq!(clock.reads.load(Ordering::SeqCst), 1);
    }

    // ------------------------------------------------------------------ writes

    /// The nine A0.2 writes with a valid body each.
    fn writes() -> Vec<(&'static str, &'static str, Option<Value>)> {
        vec![
            (
                "classify_requirement",
                "/api/requirements/req:p/classify",
                Some(json!({"class": "operational"})),
            ),
            (
                "accept_requirement_ears",
                "/api/requirements/req:p/accept-ears",
                Some(json!({"text": "When asked, the system shall cancel the leave request."})),
            ),
            ("confirm_glossary", "/api/glossary/term:ot/confirm", None),
            (
                "edit_glossary",
                "/api/glossary/term:leave/edit",
                Some(json!({"term": "leave request", "aliases": ["time off", "absence"]})),
            ),
            (
                "set_attribute_type",
                "/api/model/attributes/attr:days/type",
                Some(json!({"type": "Decimal"})),
            ),
            (
                "set_attribute_unit",
                "/api/model/attributes/attr:days/unit",
                Some(json!({"unit": "day"})),
            ),
            (
                "set_attribute_class",
                "/api/model/attributes/attr:days/class",
                Some(json!({"class": "pii"})),
            ),
            (
                "confirm_calculation",
                "/api/model/calculations/calc:p/confirm",
                None,
            ),
        ]
    }

    fn payload(app: &App, node_id: &str) -> Value {
        serde_json::to_value(&head(app).graph.node(&id(node_id)).unwrap().payload).unwrap()["data"]
            .clone()
    }

    fn status_of(app: &App, node_id: &str) -> String {
        format!("{:?}", head(app).graph.node(&id(node_id)).unwrap().status)
    }

    #[tokio::test]
    async fn http_core_nine_writes_commit_through_intake_and_cas() {
        let app = app();
        let mut events = app.state.subscribe();
        let start = head(&app);
        for (operation, uri, body) in writes() {
            let before = head(&app);
            let r = post(
                &app,
                uri,
                Some(&format!("\"{}\"", before.revision.id)),
                body,
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{operation}: {:?}", r.json());
            let after = head(&app);
            assert_eq!(
                after.revision.parent,
                Some(before.revision.id.clone()),
                "{operation}"
            );
            let body = r.json();
            assert_eq!(body["version"], after.revision.id.to_string());
            assert_eq!(body["revision"], after.revision.id.to_string());
            assert_eq!(
                body["semantic_hash"],
                after.revision.semantic_hash.to_string()
            );
            let event = events.try_recv().unwrap();
            assert_eq!(event.kind, operation);
            assert_eq!(event.version, after.revision.id.to_string());
            assert_eq!(body["events"], json!([event]));
            assert!(events.try_recv().is_err(), "{operation}: one event");
            // The committed patch is the accepted Patch artifact of the new revision.
            assert!(after.revision.accepted_patch_ref.is_some());
        }
        // Only the targeted fields changed.
        let p = payload(&app, "req:p");
        assert_eq!(p["requirement_kind"], "operational");
        assert_eq!(
            p["statement"],
            "When asked, the system shall cancel the leave request."
        );
        assert_eq!(p["modality"], "shall");
        assert_eq!(status_of(&app, "term:ot"), "Accepted");
        let leave = payload(&app, "term:leave");
        assert_eq!(leave["aliases"], json!(["absence", "time off"]));
        assert_eq!(leave["term"], "leave request");
        let days = payload(&app, "attr:days");
        assert_eq!(
            (
                days["value_type"].as_str(),
                days["unit"].as_str(),
                days["data_classification"].as_str()
            ),
            (Some("Decimal"), Some("day"), Some("pii"))
        );
        assert_eq!(days["name"], "days");
        assert_eq!(status_of(&app, "calc:p"), "Accepted");
        // Untargeted nodes are untouched.
        for untouched in ["req:a", "req:b", "attr:salary", "calc:days"] {
            assert_eq!(
                head(&app).graph.node(&id(untouched)),
                start.graph.node(&id(untouched)),
                "{untouched}"
            );
        }
    }

    #[tokio::test]
    async fn http_core_event_ids_are_the_commit_delta() {
        let app = app();
        let mut events = app.state.subscribe();
        let r = post(
            &app,
            "/api/model/calculations/calc:p/confirm",
            Some(&etag(&app)),
            None,
        )
        .await;
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(events.try_recv().unwrap().ids, ["calc:p"]);
        let r = post(
            &app,
            "/api/glossary/term:leave/edit",
            Some(&etag(&app)),
            Some(json!({"term": "leave request"})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        assert_eq!(events.try_recv().unwrap().ids, ["term:leave"]);
        assert_eq!(payload(&app, "term:leave")["aliases"], Value::Null);
    }

    #[tokio::test]
    async fn http_core_writes_require_if_match() {
        let app = app();
        let mut events = app.state.subscribe();
        let start = revision_count(&app);
        let current = head(&app).revision;
        let mut all = writes();
        all.push(("create_source", "/api/sources", None));
        assert_eq!(all.len(), 9, "the nine A0.2 POST operations");
        for (operation, uri, body) in &all {
            // The semantic hash is no longer a concurrency token (§33.2), and weak or wildcard
            // validators never match.
            for if_match in [
                None,
                Some("psg:sha256:0000"),
                Some(current.semantic_hash.as_str()),
                Some(&format!("\"{}\"", current.semantic_hash)[..]),
                Some(&format!("W/\"{}\"", current.id)[..]),
                Some("*"),
                Some(&format!("\"{}\", \"{}\"", current.id, current.id)[..]),
                Some(&format!("{} ", current.id)[..]),
            ] {
                let r = if *operation == "create_source" {
                    upload(&app, if_match, Some("x.md"), "text/markdown", b"# x\n").await
                } else {
                    post(&app, uri, if_match, body.clone()).await
                };
                match if_match {
                    None => assert_eq!(
                        (r.status, r.code().as_str()),
                        (StatusCode::BAD_REQUEST, "E_INVALID_REQUEST"),
                        "{operation}"
                    ),
                    Some(_) => {
                        assert_eq!(
                            (r.status, r.code().as_str()),
                            (StatusCode::PRECONDITION_FAILED, "E_PRECONDITION_FAILED"),
                            "{operation}"
                        );
                        assert_eq!(r.json()["revision"], current.id.to_string());
                        assert_eq!(r.json()["semantic_hash"], current.semantic_hash.to_string());
                    }
                }
            }
        }
        assert_eq!(revision_count(&app), start);
        assert!(events.try_recv().is_err());
        let posts = OPERATIONS
            .iter()
            .filter(|s| s.owner == "A0.2" && s.method == "POST")
            .count();
        assert_eq!(posts, 9);
    }

    /// Architecture §33.2: the ETag is the strong entity-tag of the loaded revision ID; If-Match
    /// accepts exactly that quoted form or the bare revision ID.
    #[tokio::test]
    async fn http_core_if_match_is_the_quoted_or_bare_revision_id() {
        let app = app();
        let reviewed = get(&app, "/api/requirements").await.etag().unwrap();
        let current = head(&app).revision;
        assert_eq!(reviewed, format!("\"{}\"", current.id));
        let r = post(
            &app,
            "/api/requirements/req:p/classify",
            Some(&reviewed),
            Some(json!({"class": "operational"})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        let bare = head(&app).revision.id.to_string();
        let r = post(
            &app,
            "/api/requirements/req:p/classify",
            Some(&bare),
            Some(json!({"class": "functional"})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        // The first tag now names an older revision.
        let r = post(
            &app,
            "/api/requirements/req:p/classify",
            Some(&reviewed),
            Some(json!({"class": "operational"})),
        )
        .await;
        assert_eq!(r.code(), "E_PRECONDITION_FAILED");
        assert_eq!(r.json()["revision"], head(&app).revision.id.to_string());
    }

    #[tokio::test]
    async fn http_core_cas_race_is_412_without_retry() {
        let (app, clock) = racing_app();
        let mut events = app.state.subscribe();
        let loaded = head(&app).revision;
        let r = post(
            &app,
            "/api/requirements/req:p/classify",
            Some(loaded.id.as_str()),
            Some(json!({"class": "operational"})),
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::PRECONDITION_FAILED, "E_PRECONDITION_FAILED")
        );
        let current = head(&app).revision;
        // Exactly the competing commit happened: no retry produced a further revision.
        assert_eq!(current.id.version(), loaded.id.version() + 1);
        assert_eq!(current.parent, Some(loaded.id.clone()));
        assert!(head(&app).graph.node(&id("finding:race")).is_some());
        assert_eq!(r.json()["revision"], current.id.to_string());
        assert_eq!(r.json()["semantic_hash"], current.semantic_hash.to_string());
        assert_eq!(payload(&app, "req:p")["requirement_kind"], "functional");
        assert_eq!(clock.reads.load(Ordering::SeqCst), 1);
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn http_core_intake_reason_required_blocks_the_write() {
        let app = app();
        let mut events = app.state.subscribe();
        let start = revision_count(&app);
        // req:c with req:b's class becomes an exact equivalent of Accepted req:b.
        let r = post(
            &app,
            "/api/requirements/req:c/classify",
            Some(&etag(&app)),
            Some(json!({"class": "functional"})),
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "E_INTAKE_REASON_REQUIRED")
        );
        let details = &r.json()["details"];
        assert!(details["proposal_ref"]
            .as_str()
            .unwrap()
            .starts_with("prop:"));
        assert_eq!(details["reason_codes"], json!(["exact_equivalent"]));
        assert_eq!(revision_count(&app), start);
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn http_core_write_validation_errors() {
        let app = app();
        let h = etag(&app);
        let cases: Vec<(&str, Option<Value>, StatusCode, &str)> = vec![
            (
                "/api/requirements/req:a/classify",
                Some(json!({"class": "functional"})),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_NO_CHANGE",
            ),
            (
                "/api/requirements/term:leave/classify",
                Some(json!({"class": "functional"})),
                StatusCode::NOT_FOUND,
                "E_NOT_FOUND",
            ),
            (
                "/api/requirements/req:a/classify",
                Some(json!({"class": "bogus"})),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/requirements/req:a/classify",
                Some(json!({"class": "functional", "x": 1})),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/requirements/req:a/accept-ears",
                Some(json!({})),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/requirements/req:a/accept-ears",
                Some(json!({"text": ""})),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/requirements/req:a/accept-ears",
                Some(json!({"text": " padded "})),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/requirements/req:a/accept-ears",
                Some(json!({"text": "line\nbreak"})),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/requirements/req:a/accept-ears",
                Some(json!({"text": "The system shall export PDF, CSV, etc."})),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_NO_CHANGE",
            ),
            (
                "/api/glossary/term:leave/confirm",
                None,
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_SEMANTIC_INVARIANT",
            ),
            (
                "/api/glossary/req:a/confirm",
                None,
                StatusCode::NOT_FOUND,
                "E_NOT_FOUND",
            ),
            (
                "/api/glossary/term:leave/edit",
                Some(json!({"term": "Leave Request", "aliases": []})),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_TERM_IDENTITY_CHANGE",
            ),
            (
                "/api/glossary/term:leave/edit",
                Some(json!({"term": "leave request", "aliases": ["absence"]})),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_NO_CHANGE",
            ),
            (
                "/api/glossary/term:leave/edit",
                Some(json!({"term": "leave request", "aliases": ["leave request"]})),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/glossary/term:leave/edit",
                Some(json!({"term": "leave request", "aliases": ["a", "a"]})),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/model/attributes/attr:days/type",
                Some(json!({"type": "Money"})),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_SEMANTIC_INVARIANT",
            ),
            (
                "/api/model/attributes/attr:days/type",
                Some(json!({"type": "Integer"})),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_SEMANTIC_INVARIANT",
            ),
            (
                "/api/model/attributes/entity:request/unit",
                Some(json!({"unit": "day"})),
                StatusCode::NOT_FOUND,
                "E_NOT_FOUND",
            ),
            (
                "/api/model/attributes/attr:days/unit",
                Some(json!({"unit": " day"})),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/model/attributes/attr:days/class",
                Some(json!({"class": "PII"})),
                StatusCode::BAD_REQUEST,
                "E_INVALID_REQUEST",
            ),
            (
                "/api/model/attributes/attr:salary/class",
                Some(json!({"class": "pii"})),
                StatusCode::CONFLICT,
                "E_ATTRIBUTE_ALREADY_CLASSIFIED",
            ),
            (
                "/api/model/attributes/attr:salary/class",
                Some(json!({"class": "financial"})),
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_NO_CHANGE",
            ),
            (
                "/api/model/calculations/calc:days/confirm",
                None,
                StatusCode::UNPROCESSABLE_ENTITY,
                "E_SEMANTIC_INVARIANT",
            ),
        ];
        let start = revision_count(&app);
        for (uri, body, status, code) in cases {
            let r = post(&app, uri, Some(&h), body.clone()).await;
            assert_eq!(
                (r.status, r.code()),
                (status, code.to_owned()),
                "{uri} {body:?}"
            );
        }
        assert_eq!(revision_count(&app), start);
        // A non-JSON body is a typed 400, not a framework text error.
        let r = send(
            &app.router,
            Request::post("/api/requirements/req:a/classify")
                .header("if-match", &h)
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::BAD_REQUEST, "E_INVALID_REQUEST")
        );
    }

    // ------------------------------------------------------------------ pagination

    /// The nine pageable A0.2 reads and a setup giving each at least two items.
    async fn pageable_app() -> (App, Vec<String>) {
        let (app, source) = app_with_source(
            "spans.md",
            b"# Spans\n\nFirst paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n",
        )
        .await;
        let uris = vec![
            format!("/api/sources/{source}/spans"),
            "/api/requirements".into(),
            "/api/glossary".into(),
            "/api/model/entities".into(),
            "/api/model/calculations".into(),
            "/api/processes".into(),
            "/api/findings".into(),
            "/api/assumptions".into(),
            "/api/decisions".into(),
        ];
        (app, uris)
    }

    fn with_query(uri: &str, query: &str) -> String {
        if uri.contains('?') {
            format!("{uri}&{query}")
        } else {
            format!("{uri}?{query}")
        }
    }

    #[tokio::test]
    async fn http_core_pagination_on_all_nine_lists() {
        let (app, uris) = pageable_app().await;
        let pageable = OPERATIONS
            .iter()
            .filter(|s| s.owner == "A0.2" && s.pageable)
            .count();
        assert_eq!(pageable, uris.len());
        for uri in &uris {
            let all = get(&app, uri).await.json();
            assert_eq!(all["next_cursor"], Value::Null, "{uri}");
            let all_ids = ids_of(&all);
            assert!(all_ids.len() >= 2, "{uri}");
            let mut sorted = all_ids.clone();
            sorted.sort();
            assert_eq!(all_ids, sorted, "{uri}: stable ID order");
            // limit=1 walk.
            let mut walked = Vec::new();
            let mut next = Some(with_query(uri, "limit=1"));
            while let Some(page_uri) = next {
                let page = get(&app, &page_uri).await;
                assert_eq!(page.status, StatusCode::OK, "{page_uri}: {:?}", page.json());
                let page = page.json();
                let ids = ids_of(&page);
                assert_eq!(ids.len(), 1);
                assert!(!walked.contains(&ids[0]), "{uri}: duplicate");
                walked.push(ids[0].clone());
                next = page["next_cursor"].as_str().map(|c| {
                    assert!(c
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
                    let decoded: Value = serde_json::from_slice(&hex::decode(c).unwrap()).unwrap();
                    assert_eq!(
                        decoded.as_object().unwrap().keys().collect::<Vec<_>>(),
                        ["after_id", "request_hash", "revision", "semantic_hash"]
                    );
                    assert_eq!(
                        to_canonical_json(&decoded).unwrap(),
                        hex::decode(c).unwrap()
                    );
                    with_query(uri, &format!("limit=1&cursor={c}"))
                });
            }
            assert_eq!(walked, all_ids, "{uri}");
            // A large limit returns everything with no cursor.
            let big = get(&app, &with_query(uri, "limit=1000")).await.json();
            assert_eq!(
                (ids_of(&big), big["next_cursor"].clone()),
                (all_ids.clone(), Value::Null)
            );
            for bad in ["limit=0", "limit=-1", "limit=x"] {
                let r = get(&app, &with_query(uri, bad)).await;
                assert_eq!(
                    (r.status, r.code().as_str()),
                    (StatusCode::BAD_REQUEST, "E_INVALID_REQUEST"),
                    "{uri} {bad}"
                );
            }
        }
    }

    #[tokio::test]
    async fn http_core_cursor_v2_contract() {
        let app = app();
        let h = head(&app).revision;
        let first = get(&app, "/api/requirements?status=Accepted&limit=1")
            .await
            .json();
        assert_eq!(ids_of(&first), ["req:a"]);
        let cursor = first["next_cursor"].as_str().unwrap().to_owned();
        // Exactly the frozen fields and an independently computed request hash.
        let rh = request_hash(
            "list_requirements",
            json!({}),
            json!({"status": "Accepted"}),
        );
        assert_eq!(
            cursor,
            cursor_for("req:a", &rh, &h.id.to_string(), h.semantic_hash.as_str())
        );
        // Same request continues; filtering happens before after_id.
        let second = get(
            &app,
            &format!("/api/requirements?status=Accepted&limit=1&cursor={cursor}"),
        )
        .await
        .json();
        assert_eq!(ids_of(&second), ["req:b"]);
        let reordered = get(
            &app,
            &format!("/api/requirements?cursor={cursor}&limit=1&status=Accepted"),
        )
        .await;
        assert_eq!(ids_of(&reordered.json()), ["req:b"]);
        // Another filter, an omitted filter, another endpoint: request mismatch.
        for uri in [
            format!("/api/requirements?status=Proposed&cursor={cursor}"),
            format!("/api/requirements?cursor={cursor}"),
            format!("/api/requirements?status=Accepted&class=functional&cursor={cursor}"),
            format!("/api/glossary?cursor={cursor}"),
            format!("/api/findings?status=Accepted&cursor={cursor}"),
        ] {
            let r = get(&app, &uri).await;
            assert_eq!(
                (r.status, r.code().as_str()),
                (StatusCode::BAD_REQUEST, "E_CURSOR_REQUEST_MISMATCH"),
                "{uri}"
            );
        }
        // An after_id outside the filtered collection is invalid.
        let foreign = cursor_for("req:p", &rh, &h.id.to_string(), h.semantic_hash.as_str());
        let r = get(
            &app,
            &format!("/api/requirements?status=Accepted&cursor={foreign}"),
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::BAD_REQUEST, "E_CURSOR_INVALID")
        );
        // Malformed cursors.
        let canonical = hex::decode(&cursor).unwrap();
        let mut pretty: Value = serde_json::from_slice(&canonical).unwrap();
        let pretty_bytes = serde_json::to_vec_pretty(&pretty).unwrap();
        pretty["extra"] = json!(1);
        let extra = hex::encode(to_canonical_json(&pretty).unwrap());
        let mut missing: Value = serde_json::from_slice(&canonical).unwrap();
        missing.as_object_mut().unwrap().remove("revision");
        let missing = hex::encode(to_canonical_json(&missing).unwrap());
        let wrong_kind = cursor_for(
            "req:a",
            &rh,
            &h.id.to_string(),
            Hash::content_sha256(b"x").as_str(),
        );
        let bad_revision = hex::encode(to_canonical_json(&json!({"after_id": "req:a", "request_hash": rh, "revision": "rev:x", "semantic_hash": h.semantic_hash})).unwrap());
        let bad_after = cursor_for(
            "not an id",
            &rh,
            &h.id.to_string(),
            h.semantic_hash.as_str(),
        );
        for bad in [
            cursor.to_uppercase(),
            cursor[1..].to_owned(),
            "zz".to_owned(),
            hex::encode(b"not json"),
            hex::encode(&pretty_bytes),
            extra,
            missing,
            wrong_kind,
            bad_revision,
            bad_after,
        ] {
            let r = get(
                &app,
                &format!("/api/requirements?status=Accepted&cursor={bad}"),
            )
            .await;
            assert_eq!(
                (r.status, r.code().as_str()),
                (StatusCode::BAD_REQUEST, "E_CURSOR_INVALID"),
                "{bad}"
            );
        }
        // A path parameter is part of the request identity.
        let a = upload(
            &app,
            Some(&etag(&app)),
            Some("a.md"),
            "text/markdown",
            b"# A\n\nOne.\n\nTwo.\n",
        )
        .await
        .json()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let b = upload(
            &app,
            Some(&etag(&app)),
            Some("b.md"),
            "text/markdown",
            b"# B\n\nThree.\n\nFour.\n",
        )
        .await
        .json()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let page = get(&app, &format!("/api/sources/{a}/spans?limit=1"))
            .await
            .json();
        let span_cursor = page["next_cursor"].as_str().unwrap().to_owned();
        let r = get(
            &app,
            &format!("/api/sources/{b}/spans?cursor={span_cursor}"),
        )
        .await;
        assert_eq!(r.code(), "E_CURSOR_REQUEST_MISMATCH");
        let r = get(
            &app,
            &format!("/api/sources/{a}/spans?cursor={span_cursor}"),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn http_core_cursor_baseline_changes() {
        let app = app();
        let page = get(&app, "/api/findings?limit=1").await.json();
        let cursor = page["next_cursor"].as_str().unwrap().to_owned();
        let before = head(&app).revision;
        // Evidence-only revision: a Finding is outside the semantic hash but visible on the page.
        let after = external_commit(
            &app,
            SemanticPatch::AddNode {
                node: finding(
                    "finding:new",
                    "PLUMB.F2.N",
                    "F2",
                    "info",
                    "Open",
                    &["req:b"],
                ),
            },
        );
        assert_eq!(after.semantic_hash, before.semantic_hash);
        assert_ne!(after.id, before.id);
        assert_eq!(ids_of(&get(&app, "/api/findings").await.json()).len(), 3);
        let r = get(&app, &format!("/api/findings?limit=1&cursor={cursor}")).await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "E_CURSOR_BASELINE_CHANGED")
        );
        // A semantic change is a baseline change too.
        let page = get(&app, "/api/findings?limit=1").await.json();
        let cursor = page["next_cursor"].as_str().unwrap().to_owned();
        let changed = external_commit(
            &app,
            SemanticPatch::AddNode {
                node: requirement(
                    "req:new",
                    "Accepted",
                    "functional",
                    "A new requirement shall exist.",
                ),
            },
        );
        assert_ne!(changed.semantic_hash, after.semantic_hash);
        let r = get(&app, &format!("/api/findings?limit=1&cursor={cursor}")).await;
        assert_eq!(r.code(), "E_CURSOR_BASELINE_CHANGED");
    }

    // ------------------------------------------------------------------ events

    async fn read_until(stream: &mut std::net::TcpStream, end: &[u8]) -> Vec<u8> {
        use std::io::Read;
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while !buf.ends_with(end) {
            stream.read_exact(&mut byte).unwrap();
            buf.push(byte[0]);
        }
        buf
    }

    fn read_frame(stream: &mut std::net::TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
        use std::io::Read;
        let mut head = [0u8; 2];
        stream.read_exact(&mut head)?;
        let mut len = (head[1] & 0x7f) as usize;
        if len == 126 {
            let mut ext = [0u8; 2];
            stream.read_exact(&mut ext)?;
            len = u16::from_be_bytes(ext) as usize;
        }
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload)?;
        Ok((head[0] & 0x0f, payload))
    }

    /// A masked client frame (RFC 6455) with `opcode` and a payload under 126 bytes.
    fn client_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mask = [0x11u8, 0x22, 0x33, 0x44];
        let mut frame = vec![0x80 | opcode, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        frame
    }

    /// Fable #237: the /api/events forwarder reads the socket while it waits for events, so a Ping
    /// is answered and a client Close is detected promptly.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn events_socket_answers_ping_and_close_promptly() {
        use std::io::Write;
        let app = app();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let server = app.router.clone();
        tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .write_all(
                b"GET /api/events HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                  Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            )
            .unwrap();
        let response = read_until(&mut stream, b"\r\n\r\n").await;
        assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 101"));
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        stream.write_all(&client_frame(0x9, b"hi")).unwrap();
        let (opcode, payload) = read_frame(&mut stream).expect("a pong without any event");
        assert_eq!((opcode, payload.as_slice()), (0xA, &b"hi"[..]));
        stream
            .write_all(&client_frame(0x8, &1000u16.to_be_bytes()))
            .unwrap();
        // The subscription ends promptly: a close frame or the end of the connection, never a
        // read timeout.
        match read_frame(&mut stream) {
            Ok((opcode, _)) => assert_eq!(opcode, 0x8),
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::UnexpectedEof, "{e}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http_core_events_websocket_is_forward_only() {
        use std::io::Write;
        let app = app();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let server = app.router.clone();
        tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
        // A committed event before the subscription is never replayed.
        let r = post(
            &app,
            "/api/model/calculations/calc:p/confirm",
            Some(&etag(&app)),
            None,
        )
        .await;
        assert_eq!(r.status, StatusCode::OK);
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .write_all(
                b"GET /api/events HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                  Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            )
            .unwrap();
        let response = read_until(&mut stream, b"\r\n\r\n").await;
        assert!(
            String::from_utf8_lossy(&response).starts_with("HTTP/1.1 101"),
            "{}",
            String::from_utf8_lossy(&response)
        );
        stream
            .set_read_timeout(Some(std::time::Duration::from_millis(300)))
            .unwrap();
        assert!(
            read_frame(&mut stream).is_err(),
            "no history or initial snapshot"
        );
        // A later commit arrives exactly as committed.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let r = post(
            &app,
            "/api/model/attributes/attr:days/unit",
            Some(&etag(&app)),
            Some(json!({"unit": "day"})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK);
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let (opcode, payload) = read_frame(&mut stream).unwrap();
        assert_eq!(opcode, 1);
        let event: Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(event, r.json()["events"][0]);
        // Failed writes emit nothing.
        let r = post(
            &app,
            "/api/model/attributes/attr:days/unit",
            Some("psg:sha256:stale"),
            Some(json!({"unit": "hour"})),
        )
        .await;
        assert_eq!(r.status, StatusCode::PRECONDITION_FAILED);
        stream
            .set_read_timeout(Some(std::time::Duration::from_millis(300)))
            .unwrap();
        assert!(read_frame(&mut stream).is_err());
        // Without an upgrade the route answers with a typed error.
        let r = get(&app, "/api/events").await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::BAD_REQUEST, "E_INVALID_REQUEST")
        );
    }

    // ------------------------------------------------------------------ H0.8 loopback-only serving

    fn with_headers(uri: &str, method: Method, headers: &[(&str, &str)]) -> Request<Body> {
        let mut request = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request.body(Body::empty()).unwrap()
    }

    fn assert_forbidden(r: &Reply, code: &str, case: &str) {
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{case}");
        assert_eq!(
            r.headers.get(header::CONTENT_TYPE).unwrap(),
            "application/json",
            "{case}"
        );
        assert_eq!(r.code(), code, "{case}");
        // The rejected value is never echoed.
        assert!(
            !String::from_utf8_lossy(&r.bytes).contains("evil"),
            "{case}"
        );
    }

    /// Architecture §33.8: every request carries a loopback `Host` (`127.0.0.1`, `[::1]` or
    /// `localhost`, any port), otherwise 403 `E_FORBIDDEN_HOST`; a present `Origin` is an `http`
    /// origin of one of those hosts (any port), otherwise 403 `E_FORBIDDEN_ORIGIN`. The check
    /// covers every route, including unrouted paths, before any handler runs.
    #[tokio::test]
    async fn loopback_host_and_origin_are_enforced() {
        let app = app();
        for host in [
            "127.0.0.1",
            "127.0.0.1:8080",
            "localhost",
            "localhost:5173",
            "LocalHost:80",
            "[::1]",
            "[::1]:8080",
        ] {
            let r = send_exact(
                &app.router,
                with_headers("/api/findings", Method::GET, &[("host", host)]),
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{host}");
        }
        // No Host at all is forbidden (a real HTTP/1.1 client always sends one).
        for uri in ["/api/findings", "/api/no-such-route", "/"] {
            let r = send_exact(&app.router, with_headers(uri, Method::GET, &[])).await;
            assert_forbidden(&r, "E_FORBIDDEN_HOST", uri);
        }
        for host in [
            "evil.example",
            "evil.example:8080",
            "localhost.evil.example",
            "127.0.0.1.evil.example",
            "127.0.0.2",
            "0.0.0.0:8080",
            "[::2]",
            "::1",
            "[::1",
            "localhost:",
            "localhost:evil",
            "localhost:8080:1",
            "user@localhost",
            "",
        ] {
            for uri in ["/api/findings", "/api/no-such-route"] {
                let r = send_exact(
                    &app.router,
                    with_headers(uri, Method::GET, &[("host", host)]),
                )
                .await;
                assert_forbidden(&r, "E_FORBIDDEN_HOST", &format!("{host} {uri}"));
            }
        }
        // A mutation with a foreign Host is refused before its handler (and its If-Match).
        let r = send_exact(
            &app.router,
            with_headers("/api/sources", Method::POST, &[("host", "evil.example")]),
        )
        .await;
        assert_forbidden(&r, "E_FORBIDDEN_HOST", "POST /api/sources");

        for origin in [
            "http://localhost",
            "http://localhost:5173",
            "http://127.0.0.1:8080",
            "http://[::1]:3000",
            "HTTP://LOCALHOST:5173",
        ] {
            let r = send(
                &app.router,
                with_headers("/api/findings", Method::GET, &[("origin", origin)]),
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{origin}");
        }
        for origin in [
            "https://localhost:5173",
            "http://evil.example",
            "http://localhost.evil.example",
            "http://127.0.0.1.evil.example:8080",
            "null",
            "file://",
            "http://user@localhost",
            "http://localhost/evil",
            "http://localhost:evil",
            "ws://localhost:5173",
            "",
        ] {
            let r = send(
                &app.router,
                with_headers("/api/findings", Method::GET, &[("origin", origin)]),
            )
            .await;
            assert_forbidden(&r, "E_FORBIDDEN_ORIGIN", origin);
        }
        // The Host check comes first.
        let r = send_exact(
            &app.router,
            with_headers(
                "/api/findings",
                Method::GET,
                &[("host", "evil.example"), ("origin", "http://evil.example")],
            ),
        )
        .await;
        assert_forbidden(&r, "E_FORBIDDEN_HOST", "host and origin");
    }

    // ------------------------------------------------------------------ H0.1 body and expression limits

    const MAX_JSON: usize = 4 * 1024 * 1024;
    const MAX_SOURCE: usize = 129 * 1024 * 1024;

    fn assert_payload_too_large(r: &Reply, limit_bytes: usize) {
        assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            r.headers.get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(
            r.json(),
            json!({
                "code": "E_PAYLOAD_TOO_LARGE",
                "message": "the request body exceeds the size limit",
                "details": {"limit_bytes": limit_bytes}
            })
        );
    }

    async fn post_raw(app: &App, uri: &str, content_type: &str, body: Vec<u8>) -> Reply {
        let request = Request::post(uri)
            .header(header::CONTENT_TYPE, content_type)
            .header("if-match", semantic(app))
            .body(Body::from(body))
            .unwrap();
        send(&app.router, request).await
    }

    #[test]
    fn http_core_body_limit_constants_are_exact() {
        assert_eq!(plumb_api::routes::MAX_JSON_BODY_BYTES, MAX_JSON);
        assert_eq!(plumb_api::routes::MAX_JSON_BODY_BYTES, 4_194_304);
        assert_eq!(plumb_api::routes::MAX_SOURCE_UPLOAD_BODY_BYTES, MAX_SOURCE);
        assert_eq!(plumb_api::routes::MAX_SOURCE_UPLOAD_BODY_BYTES, 135_266_304);
    }

    #[tokio::test]
    async fn json_body_above_4_mib_is_typed_413() {
        let app = app();
        let before = revision_count(&app);
        for uri in [
            "/api/requirements/req:p/classify",
            "/api/proposals/intake",
            "/api/glossary/term:leave/edit",
        ] {
            let r = post_raw(&app, uri, "application/json", vec![b' '; MAX_JSON + 1]).await;
            assert_payload_too_large(&r, MAX_JSON);
        }
        // Exactly 4 MiB is not a size failure: whitespace is malformed JSON, the existing 400.
        let r = post_raw(
            &app,
            "/api/requirements/req:p/classify",
            "application/json",
            vec![b' '; MAX_JSON],
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        assert_eq!(r.code(), "E_INVALID_REQUEST");
        // Malformed JSON below the limit keeps its existing typed 400.
        let r = post_raw(
            &app,
            "/api/requirements/req:p/classify",
            "application/json",
            b"{".to_vec(),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        assert_eq!(r.code(), "E_INVALID_REQUEST");
        assert_eq!(revision_count(&app), before);
    }

    #[tokio::test]
    async fn source_upload_has_its_own_129_mib_limit() {
        let app = app();
        let before = revision_count(&app);
        // Above 4 MiB and below 129 MiB: the source route's limit overrides the router-wide
        // 4 MiB default, so the upload imports.
        let text = vec![b'a'; 5 * 1024 * 1024];
        let r = upload(
            &app,
            Some(&etag(&app)),
            Some("big.md"),
            "text/markdown",
            &text,
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        assert_eq!(revision_count(&app), before + 1);
        // Above 129 MiB the body is rejected as a typed 413 with the source limit.
        let after_import = revision_count(&app);
        let (content_type, body) =
            multipart(Some("huge.md"), "text/markdown", &vec![b'a'; MAX_SOURCE]);
        assert!(body.len() > MAX_SOURCE);
        let r = post_raw(&app, "/api/sources", &content_type, body).await;
        assert_payload_too_large(&r, MAX_SOURCE);
        assert_eq!(revision_count(&app), after_import);
        // A malformed multipart body below the limit stays the existing 400.
        let r = post_raw(
            &app,
            "/api/sources",
            "multipart/form-data; boundary=plumb-boundary-7f3a",
            b"--plumb-boundary-7f3a\r\nnot a part".to_vec(),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        assert_eq!(r.code(), "E_INVALID_REQUEST");
        assert_eq!(revision_count(&app), after_import);
    }

    /// [`app`] with the A0.3 runtime, which proposal intake needs.
    fn app_with_runtime() -> App {
        let App {
            _dir, path, state, ..
        } = app();
        drop(state);
        let reports = {
            let store = open_store(&path);
            let head = store
                .load_revision(&store.head(&BranchName::main()).unwrap().unwrap())
                .unwrap();
            [GateId::I0, GateId::F1, GateId::F4]
                .iter()
                .map(|g| gate_report(&head.graph, *g))
                .collect()
        };
        let identity = ApiIdentity::new(
            "alice".into(),
            id(ACTOR),
            vec!["analyst".into(), "reviewer".into()],
            "pilot".into(),
        )
        .unwrap();
        let runtime = plumb_api::A03RuntimeConfig {
            workflow_store: plumb_store::SqliteWorkflowStore::open(&path).unwrap(),
            inference_provider: Arc::new(plumb_inference::NullProvider),
            provider_policy: plumb_inference::ProviderPolicy {
                provider: "null".into(),
                config: plumb_core::CanonicalJson::new(json!({})),
            },
            session_turn_config: plumb_session::SessionTurnConfig {
                retrieval_top_k: 3,
                max_open_questions: 3,
                max_context_bytes: 65536,
            },
            assistant_actor_ref: id("agent:assistant"),
            calendars: Arc::new(plumb_functional::ExprStaticCalendarProvider::new([]).unwrap()),
            predicates: Arc::new(plumb_api::DirectManagerPredicates::new(&[]).unwrap()),
            expression_scopes: Vec::new(),
            calculation_scope_overrides: Vec::new(),
            runtime_semantic_values: Vec::new(),
        };
        let state = ApiState::new_with_runtime(
            BranchName::main(),
            open_store(&path),
            SqliteArtifactStore::open(&path).unwrap(),
            identity,
            fixed(),
            reports,
            profile(),
            runtime,
        )
        .unwrap();
        let router = router(state.clone());
        App {
            _dir,
            path,
            state,
            router,
        }
    }

    #[tokio::test]
    async fn intake_expression_limit_is_422() {
        let app = app_with_runtime();
        let before = head(&app);
        let marker = "leave_days_marker";
        let expression = format!("{}{marker}{}", "(".repeat(65), ")".repeat(65));
        let calculation = node(
            "calc:oversized",
            "Proposed",
            "Calculation",
            json!({
                "name": "Oversized", "expression": expression, "result_type": "Int",
                "unit": null, "rounding": null, "calendar_ref": null, "examples": null
            }),
        );
        let proposal = plumb_patch::Proposal::new(
            plumb_core::StageId::S1,
            PatchSet {
                base_semantic_hash: before.revision.semantic_hash.clone(),
                patch: SemanticPatch::AddNode { node: calculation },
            },
            Vec::new(),
            Vec::new(),
            plumb_patch::ProposalMateriality::Semantic,
            plumb_patch::AcceptancePolicy::HumanConfirm,
            None,
        )
        .unwrap();
        let r = post(
            &app,
            "/api/proposals/intake",
            Some(&etag(&app)),
            Some(json!({"kind": "create", "payload": proposal, "evidence": []})),
        )
        .await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.json());
        assert_eq!(
            r.json(),
            json!({
                "code": "E_EXPRESSION_LIMIT",
                "message": "the proposal contains an expression exceeding a safety limit",
                "details": {"limit": "EXPR_MAX_NESTING_DEPTH", "actual": 65, "max": 64}
            })
        );
        let text = String::from_utf8_lossy(&r.bytes);
        assert!(!text.contains(marker));
        assert!(!text.contains("(("));
        // Nothing was committed or persisted.
        let after = head(&app);
        assert_eq!(after.revision.id, before.revision.id);
        assert_eq!(after.revision.semantic_hash, before.revision.semantic_hash);
        assert!(after.graph.node(&id("calc:oversized")).is_none());
        assert_eq!(
            plumb_store::SqliteWorkflowStore::open(&app.path)
                .unwrap()
                .latest_proposal(&proposal.id)
                .unwrap(),
            None
        );
        // The same proposal below the limit is admitted by the same path.
        let ok = node(
            "calc:oversized",
            "Proposed",
            "Calculation",
            json!({
                "name": "Oversized", "expression": format!("{}{marker}{}", "(".repeat(3), ")".repeat(3)),
                "result_type": "Int", "unit": null, "rounding": null, "calendar_ref": null,
                "examples": null
            }),
        );
        let proposal = plumb_patch::Proposal::new(
            plumb_core::StageId::S1,
            PatchSet {
                base_semantic_hash: before.revision.semantic_hash.clone(),
                patch: SemanticPatch::AddNode { node: ok },
            },
            Vec::new(),
            Vec::new(),
            plumb_patch::ProposalMateriality::Semantic,
            plumb_patch::AcceptancePolicy::HumanConfirm,
            None,
        )
        .unwrap();
        let r = post(
            &app,
            "/api/proposals/intake",
            Some(&etag(&app)),
            Some(json!({"kind": "create", "payload": proposal, "evidence": []})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        assert_eq!(head(&app).revision.id, before.revision.id);
    }

    // ------------------------------------------------------------------ source guards

    #[test]
    fn http_core_route_source_guards() {
        for (name, source) in ROUTE_SOURCES {
            let code: String = source
                .lines()
                .map(|l| l.split("//").next().unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n");
            for token in [
                "Graph::new",
                "&mut Graph",
                "nodes_mut",
                "edges_mut",
                "insert_node",
                "remove_node",
                "SqliteRevisionStore",
                "store.commit(",
                "revision_store",
                "std::fs",
                "SystemTime",
                "Utc::now",
                "std::env",
                "plumb_lint",
                "plumb_expr",
                "evaluate_gate",
                "EvaluatorRegistry",
                "501",
            ] {
                assert!(!code.contains(token), "{name} contains {token:?}");
            }
            // Hotfix 074: the one pure dry-run of the shared U2.1 semantic change preview is the
            // only route-layer apply_patch; every other route module neither imports nor calls it.
            if name == "model.rs" {
                assert_eq!(
                    code.matches("apply_patch(").count(),
                    1,
                    "one production call"
                );
                assert_eq!(
                    code.matches("apply_patch(&loaded.graph, &proposal.patch_set)")
                        .count(),
                    1,
                    "the call is the preview dry-run"
                );
                assert_eq!(
                    code.matches("apply_patch").count(),
                    2,
                    "the import and the call only"
                );
            } else {
                assert_eq!(
                    code.matches("apply_patch").count(),
                    0,
                    "{name} uses apply_patch"
                );
            }
        }
        // Writes reach the store only through the shared state pipeline; the unit and class
        // attribute writes share one call site (the type write is the shared
        // `set_attribute_type` application service, also used by the CLI).
        let model = ROUTE_SOURCES
            .iter()
            .find(|(n, _)| *n == "model.rs")
            .unwrap()
            .1;
        assert_eq!(
            model.matches("attribute_write(").count(),
            3,
            "one definition, two callers"
        );
        let lib = include_str!("../src/lib.rs");
        for token in ["fn main", "axum::serve", "TcpListener", "CorsLayer"] {
            assert!(!lib.contains(token), "lib.rs contains {token}");
        }
    }

    // ------------------------------------------------------------------ H0.9 typed store errors

    fn identity() -> ApiIdentity {
        ApiIdentity::new(
            "alice".into(),
            id(ACTOR),
            vec!["analyst".into(), "reviewer".into()],
            "pilot".into(),
        )
        .unwrap()
    }

    /// A router over `path` with the given stores.
    fn router_over(
        branch: &str,
        store: SqliteRevisionStore,
        artifacts: SqliteArtifactStore,
    ) -> Router {
        router(
            ApiState::new(
                branch.parse().unwrap(),
                store,
                artifacts,
                identity(),
                fixed(),
                vec![],
                profile(),
            )
            .unwrap(),
        )
    }

    async fn set_unit(router: &Router, if_match: &str) -> Reply {
        let request = Request::post("/api/model/attributes/attr:days/unit")
            .header("if-match", if_match)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({"unit": "day"})).unwrap(),
            ))
            .unwrap();
        send(router, request).await
    }

    #[tokio::test]
    async fn typed_store_errors() {
        // A branch without a head is 404 E_BRANCH_NOT_FOUND, never E_INTERNAL.
        let app = app();
        let tag = etag(&app);
        let ghost = router_over(
            "feature",
            open_store(&app.path),
            SqliteArtifactStore::open(&app.path).unwrap(),
        );
        let r = send(
            &ghost,
            Request::get("/api/requirements")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::NOT_FOUND, "E_BRANCH_NOT_FOUND")
        );

        // A write on a read-only database is 409 E_DATABASE_READ_ONLY and changes nothing.
        let read_only = router_over(
            "main",
            SqliteRevisionStore::open_read_only(&app.path).unwrap(),
            SqliteArtifactStore::open_read_only(&app.path).unwrap(),
        );
        let before = head(&app).revision.id;
        let r = set_unit(&read_only, &tag).await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "E_DATABASE_READ_ONLY"),
            "{:?}",
            r.json()
        );
        assert_eq!(head(&app).revision.id, before);

        // SQLite busy (another connection holds the write lock) is 503 E_DATABASE_BUSY.
        let blocker = rusqlite::Connection::open(&app.path).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();
        let store = open_store(&app.path);
        store.set_busy_timeout(std::time::Duration::ZERO).unwrap();
        let busy = router_over("main", store, SqliteArtifactStore::open(&app.path).unwrap());
        let r = set_unit(&busy, &tag).await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::SERVICE_UNAVAILABLE, "E_DATABASE_BUSY"),
            "{:?}",
            r.json()
        );
        blocker.execute_batch("ROLLBACK;").unwrap();
        drop(blocker);

        // A revision written under another PSG schema version is the typed unsupported-version
        // error (Fable #3), never E_STORE_CORRUPT.
        let raw = rusqlite::Connection::open(&app.path).unwrap();
        raw.execute(
            "UPDATE graph_revisions SET psg_schema_version = 2 WHERE id = ?1",
            [before.as_str()],
        )
        .unwrap();
        let r = get(&app, "/api/requirements").await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "E_STORE_VERSION_UNSUPPORTED")
        );
        raw.execute(
            "UPDATE graph_revisions SET psg_schema_version = 1 WHERE id = ?1",
            [before.as_str()],
        )
        .unwrap();
        assert_eq!(get(&app, "/api/requirements").await.status, StatusCode::OK);

        // Detected corruption is 500 E_STORE_CORRUPT.
        raw.execute(
            "UPDATE revision_nodes SET node_json = 'not json' WHERE revision_id = ?1 \
             AND node_id = 'req:a'",
            [before.as_str()],
        )
        .unwrap();
        let r = get(&app, "/api/requirements").await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::INTERNAL_SERVER_ERROR, "E_STORE_CORRUPT")
        );

        // The complete class table, including artifact corruption (H0.10).
        use plumb_api::error::ApiError;
        use plumb_store::StoreErrorClass as C;
        for (class, status, code) in [
            (
                C::BranchNotFound,
                StatusCode::NOT_FOUND,
                "E_BRANCH_NOT_FOUND",
            ),
            (
                C::NotInitialized,
                StatusCode::CONFLICT,
                "E_DATABASE_NOT_INITIALIZED",
            ),
            (C::Busy, StatusCode::SERVICE_UNAVAILABLE, "E_DATABASE_BUSY"),
            (C::ReadOnly, StatusCode::CONFLICT, "E_DATABASE_READ_ONLY"),
            (
                C::Corrupt,
                StatusCode::INTERNAL_SERVER_ERROR,
                "E_STORE_CORRUPT",
            ),
            (
                C::UnsupportedVersion,
                StatusCode::CONFLICT,
                "E_STORE_VERSION_UNSUPPORTED",
            ),
        ] {
            let error = ApiError::store_class(class).unwrap();
            assert_eq!((error.body().status, error.body().code), (status, code));
        }
        assert!(ApiError::store_class(C::Other).is_none());
        let corrupt = plumb_artifacts::ArtifactStoreError::Corrupt {
            hash: Hash::content_sha256(b"x"),
            reason: "the stored bytes do not hash to the artifact key",
        };
        assert_eq!(
            ApiError::artifact_store(&corrupt, "artifact_store")
                .body()
                .code,
            "E_STORE_CORRUPT"
        );
        assert_eq!(
            ApiError::store(&plumb_store::StoreError::DatabaseNotInitialized, "x")
                .body()
                .code,
            "E_DATABASE_NOT_INITIALIZED"
        );
    }

    // ------------------------------------------------------------------ H0.11 error envelope

    /// Asserts the typed `{code, message, details}` envelope with a catalogued `code` of `status`
    /// and no framework, Rust debug or SQLite text.
    fn assert_typed(r: &Reply, status: StatusCode, code: &str, what: &str) {
        assert_eq!(
            r.status,
            status,
            "{what}: {:?}",
            String::from_utf8_lossy(&r.bytes)
        );
        assert_eq!(
            r.headers.get(header::CONTENT_TYPE).unwrap(),
            "application/json",
            "{what}"
        );
        let body = r.json();
        let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["code", "details", "message"], "{what}");
        assert_eq!(body["code"], code, "{what}");
        let entry = plumb_api::error_catalogue::lookup(code)
            .unwrap_or_else(|| panic!("{what}: {code} is not catalogued"));
        assert_eq!(entry.status, status.as_u16(), "{what}");
        let message = body["message"].as_str().unwrap();
        for raw in [
            "Failed to",
            "axum",
            "serde",
            "sqlite",
            "SQLite",
            "{",
            "Error(",
        ] {
            assert!(!message.contains(raw), "{what}: raw text in {message:?}");
        }
    }

    /// H0.11 (architecture §33.12): every framework or extractor failure on the API boundary uses
    /// the typed envelope.
    #[tokio::test]
    async fn framework_errors_use_typed_envelope() {
        let app = app();
        let tag = etag(&app);
        // 404 unknown route, for any method and outside /api as well.
        for (method, uri) in [
            (Method::GET, "/api/nowhere"),
            (Method::POST, "/api/nowhere"),
            (Method::GET, "/nowhere"),
            (Method::GET, "/api/requirements/req:a/nothing"),
        ] {
            let r = send(
                &app.router,
                Request::builder()
                    .method(method.clone())
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_typed(&r, StatusCode::NOT_FOUND, "E_ROUTE_NOT_FOUND", uri);
        }
        // 405 a known route with another method.
        for (method, uri) in [
            (Method::DELETE, "/api/requirements"),
            (Method::PUT, "/api/requirements/req:a/classify"),
            (Method::POST, "/api/model/version"),
        ] {
            let r = send(
                &app.router,
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_typed(
                &r,
                StatusCode::METHOD_NOT_ALLOWED,
                "E_METHOD_NOT_ALLOWED",
                uri,
            );
        }
        // 400 malformed JSON, query and path.
        let r = post_raw(
            &app,
            "/api/requirements/req:p/classify",
            "application/json",
            b"{".to_vec(),
        )
        .await;
        assert_typed(
            &r,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
            "malformed JSON",
        );
        let r = get(&app, "/api/requirements?limit=x").await;
        assert_typed(
            &r,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
            "malformed query",
        );
        let r = get(&app, "/api/requirements/%FF").await;
        assert_typed(
            &r,
            StatusCode::BAD_REQUEST,
            "E_INVALID_REQUEST",
            "malformed path",
        );
        // A plain GET of the WebSocket endpoint without an upgrade.
        let r = get(&app, "/api/events").await;
        assert_eq!(
            r.headers.get(header::CONTENT_TYPE).unwrap(),
            "application/json",
            "events without upgrade"
        );
        assert!(plumb_api::error_catalogue::lookup(&r.code()).is_some());
        // 413 body limit.
        let r = post_raw(
            &app,
            "/api/requirements/req:p/classify",
            "application/json",
            vec![b' '; MAX_JSON + 1],
        )
        .await;
        assert_typed(
            &r,
            StatusCode::PAYLOAD_TOO_LARGE,
            "E_PAYLOAD_TOO_LARGE",
            "413",
        );
        // 415 an upload that is not multipart, and an unsupported source media type.
        let r = send(
            &app.router,
            Request::post("/api/sources")
                .header(header::CONTENT_TYPE, "text/plain")
                .header("if-match", &tag)
                .body(Body::from("x"))
                .unwrap(),
        )
        .await;
        assert_eq!(
            r.headers.get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert!(
            plumb_api::error_catalogue::lookup(&r.code()).is_some(),
            "{}",
            r.code()
        );
        let r = upload(
            &app,
            Some(&tag),
            Some("x.exe"),
            "application/x-msdownload",
            b"MZ",
        )
        .await;
        assert_typed(
            &r,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "E_UNSUPPORTED_MEDIA_TYPE",
            "415",
        );
    }

    /// Every `"E_..."` code literal in the plumb-api source is catalogued, and the catalogue is
    /// exactly the codes documented in OpenAPI (`x-plumb-error-codes`) with their statuses.
    #[test]
    fn every_error_code_is_catalogued() {
        fn scan(dir: &Path, codes: &mut std::collections::BTreeSet<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    scan(&path, codes);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && !path.ends_with("error_catalogue.rs")
                {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let bytes = text.as_bytes();
                    let mut i = 0;
                    while let Some(at) = text[i..].find("\"E_") {
                        let start = i + at + 1;
                        let end = start
                            + text[start..]
                                .find(|c: char| {
                                    !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                                })
                                .unwrap();
                        if bytes[end] == b'"' && end > start + 2 {
                            codes.insert(text[start..end].to_owned());
                        }
                        i = end;
                    }
                }
            }
        }
        let mut used = std::collections::BTreeSet::new();
        scan(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut used,
        );
        assert!(used.len() > 40, "{used:?}");
        let catalogue = plumb_api::error_catalogue::ERROR_CATALOGUE;
        let catalogued: std::collections::BTreeSet<String> =
            catalogue.iter().map(|e| e.code.to_owned()).collect();
        assert_eq!(
            catalogued.len(),
            catalogue.len(),
            "duplicate catalogue entries"
        );
        let missing: Vec<&String> = used.difference(&catalogued).collect();
        assert!(missing.is_empty(), "uncatalogued codes: {missing:?}");
        let unused: Vec<&String> = catalogued.difference(&used).collect();
        assert!(
            unused.is_empty(),
            "catalogued codes no source uses: {unused:?}"
        );
        for entry in catalogue {
            assert!(StatusCode::from_u16(entry.status).is_ok(), "{}", entry.code);
            assert!(!entry.meaning.is_empty(), "{}", entry.code);
        }
        let doc: Value = plumb_api::openapi_document().unwrap();
        let documented: Vec<Value> = doc["info"]["x-plumb-error-codes"]
            .as_array()
            .unwrap()
            .clone();
        let expected: Vec<Value> = catalogue
            .iter()
            .map(|e| json!({"code": e.code, "status": e.status, "meaning": e.meaning}))
            .collect();
        assert_eq!(documented, expected);
    }

    /// Fable #205: an error echoing client text truncates it to 64 characters plus an ellipsis.
    #[tokio::test]
    async fn echoed_client_text_is_truncated() {
        assert_eq!(plumb_api::error::echo("short"), "\"short\"");
        let long = "k".repeat(200);
        let expected = format!("{:?}…", "k".repeat(64));
        assert_eq!(plumb_api::error::echo(&long), expected);
        assert_eq!(
            plumb_api::error::echo(&"é".repeat(64)),
            format!("{:?}", "é".repeat(64))
        );
        let app = app();
        let r = get(&app, &format!("/api/requirements?{long}=1")).await;
        assert_eq!(r.code(), "E_INVALID_REQUEST");
        let message = r.json()["message"].as_str().unwrap().to_owned();
        assert!(message.contains(&expected), "{message}");
        assert!(!message.contains(&"k".repeat(65)), "{message}");
        let r = post(
            &app,
            "/api/requirements/req:p/classify",
            Some(&etag(&app)),
            Some(json!({"class": "x".repeat(500)})),
        )
        .await;
        assert_eq!(r.code(), "E_INVALID_REQUEST");
        let message = r.json()["message"].as_str().unwrap().to_owned();
        assert!(!message.contains(&"x".repeat(65)), "{message}");
    }

    /// A writer collecting formatted tracing output.
    #[derive(Clone, Default)]
    struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Fable #202: one tracing record per request (method, matched route, status, latency) and the
    /// underlying cause of an internal failure, which never reaches the response.
    #[tokio::test]
    async fn requests_and_internal_causes_are_traced() {
        // A process-wide subscriber: a scoped one can miss callsites first hit by other tests.
        static CAPTURED: std::sync::OnceLock<Captured> = std::sync::OnceLock::new();
        let captured = CAPTURED
            .get_or_init(|| {
                let captured = Captured::default();
                let writer = captured.clone();
                tracing::subscriber::set_global_default(
                    tracing_subscriber::fmt()
                        .with_writer(move || writer.clone())
                        .with_ansi(false)
                        .finish(),
                )
                .unwrap();
                captured
            })
            .clone();
        tracing::callsite::rebuild_interest_cache();
        let app = app();
        let r = get(&app, "/api/requirements/req:a").await;
        assert_eq!(r.status, StatusCode::OK);
        let r = get(&app, "/api/nowhere/secret-text").await;
        assert_eq!(r.status, StatusCode::NOT_FOUND);
        let error = plumb_api::error::ApiError::internal_cause("revision_store", &"disk I/O error");
        assert_eq!(error.body().details, json!({"subsystem": "revision_store"}));
        let text = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(text.contains("method=GET"), "{text}");
        assert!(text.contains("route=/api/requirements/{id}"), "{text}");
        assert!(text.contains("status=200"), "{text}");
        assert!(text.contains("latency_ms="), "{text}");
        assert!(
            text.contains("route=unmatched") && text.contains("status=404"),
            "{text}"
        );
        assert!(
            !text.contains("secret-text"),
            "raw paths are never logged: {text}"
        );
        assert!(
            text.contains("subsystem=\"revision_store\"")
                || text.contains("subsystem=revision_store"),
            "{text}"
        );
        assert!(text.contains("disk I/O error"), "{text}");
    }

    /// Fable #314: re-uploading the same source at another clock instant is E_SOURCE_EXISTS
    /// (audit metadata is ignored); only a different payload under the same identity conflicts.
    #[tokio::test]
    async fn source_reupload_exists_under_another_clock_instant() {
        let app = app();
        let md = b"# Leave\n\nEmployees request leave.\n";
        let r = upload(
            &app,
            Some(&etag(&app)),
            Some("leave.md"),
            "text/markdown",
            md,
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        let id = r.json()["id"].as_str().unwrap().to_owned();
        // The same database served by a state whose clock reads another instant.
        let later = state_with(
            open_store(&app.path),
            SqliteArtifactStore::open(&app.path).unwrap(),
            Arc::new(FixedClock::new(ts(T1))),
            Vec::new(),
        );
        let later = App {
            _dir: tempfile::tempdir().unwrap(),
            path: app.path.clone(),
            router: router(later.clone()),
            state: later,
        };
        let before = revision_count(&later);
        let r = upload(
            &later,
            Some(&etag(&later)),
            Some("leave.md"),
            "text/markdown",
            md,
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "E_SOURCE_EXISTS"),
            "{:?}",
            r.json()
        );
        assert_eq!(r.json()["details"]["id"], id);
        // Fable #235: the same content under another display name is the same identity with
        // another persisted payload: E_SOURCE_CONFLICT.
        let r = upload(
            &later,
            Some(&etag(&later)),
            Some("other.md"),
            "text/markdown",
            md,
        )
        .await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::CONFLICT, "E_SOURCE_CONFLICT"),
            "{:?}",
            r.json()
        );
        assert_eq!(revision_count(&later), before);
    }

    /// Fable #321: a source yielding zero EvidenceFragments is a typed 400 before any artifact or
    /// revision is written.
    #[tokio::test]
    async fn empty_source_is_rejected_before_any_write() {
        let app = app();
        let before = revision_count(&app);
        for (name, media, bytes) in [
            ("empty.md", "text/markdown", &b""[..]),
            ("blank.md", "text/markdown", &b"\n\n   \n"[..]),
            ("blank.txt", "text/plain", &b"   "[..]),
        ] {
            let r = upload(&app, Some(&etag(&app)), Some(name), media, bytes).await;
            assert_eq!(
                (r.status, r.code().as_str()),
                (StatusCode::BAD_REQUEST, "E_SOURCE_EMPTY"),
                "{name}: {:?}",
                r.json()
            );
            assert!(
                !artifact_exists(&app, &Hash::content_sha256(bytes)),
                "{name}"
            );
        }
        assert_eq!(revision_count(&app), before);
    }

    /// Local install (goal-approach): `serve --ui-dir` serves the built UI on the API's origin.
    /// Any non-API GET that names no file returns `index.html` (client-side routes), an API path
    /// keeps the typed envelope, and a path never escapes the UI directory.
    #[tokio::test]
    async fn the_built_ui_is_served_next_to_the_api() {
        let app = app();
        let ui = tempfile::tempdir().unwrap();
        std::fs::write(ui.path().join("index.html"), "<html>plumb-ui</html>").unwrap();
        std::fs::create_dir_all(ui.path().join("assets")).unwrap();
        std::fs::write(ui.path().join("assets/app.js"), "console.log(1)").unwrap();
        let served = router_with_ui(app.state.clone(), Some(ui.path()));
        let get = |uri: &str| Request::get(uri).body(Body::empty()).unwrap();
        for uri in ["/", "/requirements", "/requirements/req:a", "/gates?tab=f2"] {
            let r = send(&served, get(uri)).await;
            assert_eq!(r.status, StatusCode::OK, "{uri}");
            assert_eq!(
                String::from_utf8_lossy(&r.bytes),
                "<html>plumb-ui</html>",
                "{uri}"
            );
        }
        let r = send(&served, get("/assets/app.js")).await;
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(String::from_utf8_lossy(&r.bytes), "console.log(1)");
        assert!(r.headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("javascript"));
        // The API is unchanged, unknown API paths keep the typed 404.
        assert_eq!(
            send(&served, get("/api/requirements")).await.status,
            StatusCode::OK
        );
        let r = send(&served, get("/api/nowhere")).await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::NOT_FOUND, "E_ROUTE_NOT_FOUND")
        );
        // No traversal out of the UI directory.
        std::fs::write(ui.path().parent().unwrap().join("secret.txt"), "secret").ok();
        let r = send(&served, get("/../secret.txt")).await;
        assert!(!String::from_utf8_lossy(&r.bytes).contains("secret"));
        // Without a UI directory nothing changes.
        let r = send(&router(app.state.clone()), get("/")).await;
        assert_eq!(
            (r.status, r.code().as_str()),
            (StatusCode::NOT_FOUND, "E_ROUTE_NOT_FOUND")
        );
    }

    /// `list_sources`: every imported source with its name, import time and passage count, by
    /// ID; the same projection as `get_source` plus those three fields.
    #[tokio::test]
    async fn http_core_list_sources_reports_every_imported_source() {
        let app = app();
        let r = get(&app, "/api/sources").await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        assert_eq!(r.etag(), Some(etag(&app)));
        let existing = head(&app)
            .graph
            .node_ids_by_type(plumb_psg::NodeType::SourceArtifact)
            .len();
        assert_eq!(r.json()["items"].as_array().unwrap().len(), existing);
        let md: &[u8] = b"# Leave\n\nEmployees request leave.\n\nManagers approve leave.\n";
        let uploaded = upload(
            &app,
            Some(&etag(&app)),
            Some("leave.md"),
            "text/markdown",
            md,
        )
        .await;
        assert_eq!(uploaded.status, StatusCode::OK, "{:?}", uploaded.json());
        let source = uploaded.json()["id"].as_str().unwrap().to_owned();
        let listed = get(&app, "/api/sources").await.json();
        let items = listed["items"].as_array().unwrap();
        assert_eq!(items.len(), existing + 1);
        let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        let item = items.iter().find(|i| i["id"] == source.as_str()).unwrap();
        let single = get(&app, &format!("/api/sources/{source}")).await.json();
        for field in ["id", "hash", "media_type", "source_kind"] {
            assert_eq!(item[field], single[field], "{field}");
        }
        assert_eq!(item["name"], "leave.md");
        assert_eq!(item["imported_at"], NOW);
        let spans = get(&app, &format!("/api/sources/{source}/spans"))
            .await
            .json();
        assert_eq!(
            item["spans_count"],
            spans["items"].as_array().unwrap().len()
        );
        assert_eq!(item["spans_count"], uploaded.json()["spans_count"]);
        assert_eq!(
            item.as_object().unwrap().keys().collect::<Vec<_>>(),
            [
                "hash",
                "id",
                "imported_at",
                "media_type",
                "name",
                "source_kind",
                "spans_count"
            ]
        );
    }
}
