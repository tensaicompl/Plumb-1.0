//! A0.1 contract tests for the generated pilot OpenAPI document (Hotfix 062).
//!
//! The contract tests live in `openapi` (`cargo test -p plumb-api openapi`). Examples are
//! validated with jsonschema 0.18 (Draft 2020-12). `regenerate_contract_files` is ignored and is
//! the only test that writes the committed files.

use std::path::PathBuf;

fn repo_file(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// Rewrites the committed generated files from the Rust contract. Run only with
/// `cargo test -p plumb-api --test openapi regenerate_contract_files -- --ignored --exact`.
#[test]
#[ignore]
fn regenerate_contract_files() {
    let openapi = plumb_api::render_openapi_json().unwrap();
    let markdown = plumb_api::render_api_markdown().unwrap();
    std::fs::create_dir_all(repo_file("api")).unwrap();
    std::fs::write(repo_file("api/modeller.openapi.json"), openapi).unwrap();
    std::fs::write(repo_file("docs/api.md"), markdown).unwrap();
}

mod openapi {
    use std::collections::{BTreeMap, BTreeSet};

    use jsonschema::{Draft, JSONSchema};
    use plumb_api::openapi::{
        fixture_seeds, ExampleGenerator, FixtureSeed, SeedFamily, CONTRACT_VERSION, OPERATIONS,
    };
    use plumb_api::{
        openapi_document, render_api_markdown, render_openapi_json, OpenApiContractError,
    };
    use serde_json::{json, Map, Value};

    use super::repo_file;

    const LIB_SRC: &str = include_str!("../src/lib.rs");
    const OPENAPI_SRC: &str = include_str!("../src/openapi.rs");

    /// The frozen Hotfix 062 inventory, written independently of the production table.
    const INVENTORY: [(&str, &str, &str); 72] = [
        ("GET", "/api/me", "get_me"),
        ("GET", "/api/model/version", "get_model_version"),
        ("GET", "/api/gates", "get_gates"),
        ("GET", "/api/search", "search_model"),
        ("GET", "/api/events", "events_websocket"),
        ("POST", "/api/sources", "create_source"),
        ("GET", "/api/sources/{id}", "get_source"),
        ("GET", "/api/sources/{id}/spans", "get_source_spans"),
        ("POST", "/api/stages/{stage}/run", "run_stage"),
        ("GET", "/api/jobs/{id}", "get_job"),
        ("GET", "/api/requirements", "list_requirements"),
        ("GET", "/api/requirements/{id}", "get_requirement"),
        (
            "POST",
            "/api/requirements/{id}/classify",
            "classify_requirement",
        ),
        (
            "POST",
            "/api/requirements/{id}/accept-ears",
            "accept_requirement_ears",
        ),
        ("POST", "/api/requirements/{id}/split", "split_requirement"),
        (
            "POST",
            "/api/requirements/{id}/waive-finding",
            "waive_requirement_finding",
        ),
        (
            "POST",
            "/api/requirements/{id}/mark-duplicate",
            "mark_requirement_duplicate",
        ),
        ("GET", "/api/glossary", "list_glossary"),
        ("POST", "/api/glossary/{id}/confirm", "confirm_glossary"),
        ("POST", "/api/glossary/{id}/edit", "edit_glossary"),
        ("POST", "/api/glossary/{id}/merge", "merge_glossary"),
        ("GET", "/api/model/sentences", "list_sentences"),
        (
            "POST",
            "/api/model/sentences/{id}/accept",
            "accept_sentence",
        ),
        ("POST", "/api/model/sentences/{id}/edit", "edit_sentence"),
        (
            "POST",
            "/api/model/sentences/{id}/reject",
            "reject_sentence",
        ),
        ("GET", "/api/model/entities", "list_entities"),
        ("GET", "/api/model/entities/{id}", "get_entity"),
        ("GET", "/api/model/crud", "get_crud_matrix"),
        (
            "POST",
            "/api/model/attributes/{id}/type",
            "set_attribute_type",
        ),
        (
            "POST",
            "/api/model/attributes/{id}/unit",
            "set_attribute_unit",
        ),
        (
            "POST",
            "/api/model/attributes/{id}/class",
            "set_attribute_class",
        ),
        ("GET", "/api/model/calculations", "list_calculations"),
        (
            "GET",
            "/api/model/calculations/{id}/example",
            "get_calculation_example",
        ),
        (
            "POST",
            "/api/model/calculations/{id}/confirm",
            "confirm_calculation",
        ),
        ("GET", "/api/model/rules", "list_rules"),
        ("GET", "/api/model/rules/{id}", "get_rule"),
        ("POST", "/api/model/rules/{id}/cells", "set_rule_cell"),
        ("GET", "/api/processes", "list_processes"),
        ("GET", "/api/processes/{id}", "get_process"),
        ("POST", "/api/processes/{id}/steps", "mutate_process_steps"),
        ("POST", "/api/processes/import", "import_process"),
        ("GET", "/api/processes/{id}/export", "export_process"),
        ("GET", "/api/scenarios", "list_scenarios"),
        ("POST", "/api/scenarios/{id}/confirm", "confirm_scenario"),
        ("POST", "/api/scenarios/{id}/correct", "correct_scenario"),
        (
            "POST",
            "/api/scenarios/confirm-derived",
            "confirm_derived_scenarios",
        ),
        ("POST", "/api/scenarios/run", "run_scenarios"),
        ("GET", "/api/scenarios/{id}/run", "get_scenario_run"),
        ("GET", "/api/findings", "list_findings"),
        ("POST", "/api/findings/{id}/waive", "waive_finding"),
        ("GET", "/api/assumptions", "list_assumptions"),
        (
            "POST",
            "/api/assumptions/{id}/resolve",
            "resolve_assumption",
        ),
        ("GET", "/api/questions", "list_questions"),
        ("POST", "/api/questions/{id}/answer", "answer_question"),
        ("POST", "/api/rounds", "create_round"),
        ("GET", "/api/rounds/{id}", "get_round"),
        ("POST", "/api/proposals/intake", "intake_proposal"),
        ("POST", "/api/proposals/{id}/link", "link_proposal"),
        ("POST", "/api/proposals/{id}/merge", "merge_proposal"),
        (
            "POST",
            "/api/proposals/{id}/supersede",
            "supersede_proposal",
        ),
        ("POST", "/api/proposals/{id}/create", "create_proposal"),
        ("POST", "/api/proposals/{id}/reject", "reject_proposal"),
        ("POST", "/api/proposals/{id}/accept", "accept_proposal"),
        ("POST", "/api/sessions", "create_session"),
        ("POST", "/api/sessions/{id}/turns", "create_session_turn"),
        ("GET", "/api/sessions/{id}", "get_session"),
        ("GET", "/api/sessions/{id}/metrics", "get_session_metrics"),
        ("GET", "/api/decisions", "list_decisions"),
        (
            "GET",
            "/api/requirements/{id}/readiness",
            "get_requirement_readiness",
        ),
        ("GET", "/api/readiness", "get_readiness"),
        ("GET", "/api/model/export", "export_model"),
        ("GET", "/api/model/diff", "get_model_diff"),
    ];

    const PAGEABLE: [&str; 14] = [
        "/api/search",
        "/api/sources/{id}/spans",
        "/api/requirements",
        "/api/glossary",
        "/api/model/sentences",
        "/api/model/entities",
        "/api/model/calculations",
        "/api/model/rules",
        "/api/processes",
        "/api/scenarios",
        "/api/findings",
        "/api/assumptions",
        "/api/questions",
        "/api/decisions",
    ];

    // ------------------------------------------------------------------ helpers

    fn doc() -> Value {
        openapi_document().unwrap_or_else(|e| panic!("{e}"))
    }

    /// (method, path, operation) for every operation of the document.
    fn operations(doc: &Value) -> Vec<(String, String, Value)> {
        let mut out = Vec::new();
        for (path, item) in doc["paths"].as_object().unwrap() {
            for (method, operation) in item.as_object().unwrap() {
                out.push((method.to_uppercase(), path.clone(), operation.clone()));
            }
        }
        out
    }

    fn operation(doc: &Value, method: &str, path: &str) -> Value {
        doc["paths"][path][method.to_lowercase()].clone()
    }

    fn schema(doc: &Value, name: &str) -> Value {
        doc["components"]["schemas"][name].clone()
    }

    fn properties(doc: &Value, name: &str) -> BTreeSet<String> {
        schema(doc, name)["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{name} has no properties"))
            .keys()
            .cloned()
            .collect()
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn component_ref(name: &str) -> Value {
        json!({ "$ref": format!("#/components/schemas/{name}") })
    }

    /// A Draft 2020-12 validator for `schema` that keeps `components.schemas` reachable.
    fn validator(doc: &Value, schema: &Value) -> JSONSchema {
        let mut wrapper = schema.as_object().cloned().unwrap_or_default();
        wrapper.insert("components".to_owned(), doc["components"].clone());
        JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .should_validate_formats(true)
            .should_ignore_unknown_formats(true)
            .compile(&Value::Object(wrapper))
            .unwrap_or_else(|e| panic!("schema does not compile: {e}"))
    }

    /// The typed blocked-contract outcome of validating one example.
    fn validate_example(
        doc: &Value,
        schema: &Value,
        example: &Value,
        location: &str,
    ) -> Result<(), OpenApiContractError> {
        let compiled = validator(doc, schema);
        let result =
            compiled
                .validate(example)
                .map_err(|errors| OpenApiContractError::ExampleValidation {
                    location: location.to_owned(),
                    reason: errors.map(|e| e.to_string()).collect::<Vec<_>>().join("; "),
                });
        result
    }

    #[derive(Debug, Default)]
    struct ExampleCounts {
        parameters: usize,
        request_bodies: usize,
        responses: usize,
        headers: usize,
        websocket: usize,
    }

    /// Every schema-bearing example location: (location, schema, example).
    fn example_locations(doc: &Value) -> (Vec<(String, Value, Option<Value>)>, ExampleCounts) {
        let mut out = Vec::new();
        let mut counts = ExampleCounts::default();
        for (method, path, op) in operations(doc) {
            let id = op["operationId"].as_str().unwrap().to_owned();
            for parameter in op["parameters"].as_array().into_iter().flatten() {
                if let Some(schema) = parameter.get("schema") {
                    counts.parameters += 1;
                    out.push((
                        format!("{id} {} parameter {}", parameter["in"], parameter["name"]),
                        schema.clone(),
                        parameter.get("example").cloned(),
                    ));
                }
            }
            for (media, content) in op["requestBody"]["content"]
                .as_object()
                .into_iter()
                .flatten()
            {
                if let Some(schema) = content.get("schema") {
                    counts.request_bodies += 1;
                    out.push((
                        format!("{id} request {media}"),
                        schema.clone(),
                        content.get("example").cloned(),
                    ));
                }
            }
            for (status, response) in op["responses"].as_object().unwrap() {
                for (media, content) in response["content"].as_object().into_iter().flatten() {
                    if let Some(schema) = content.get("schema") {
                        counts.responses += 1;
                        out.push((
                            format!("{id} response {status} {media}"),
                            schema.clone(),
                            content.get("example").cloned(),
                        ));
                    }
                }
                for (name, header) in response["headers"].as_object().into_iter().flatten() {
                    if let Some(schema) = header.get("schema") {
                        counts.headers += 1;
                        out.push((
                            format!("{id} response {status} header {name}"),
                            schema.clone(),
                            header.get("example").cloned(),
                        ));
                    }
                }
            }
            if let Some(ws) = op.get("x-plumb-websocket") {
                counts.websocket += 1;
                out.push((
                    format!("{method} {path} websocket message"),
                    ws["message_schema"].clone(),
                    ws.get("message_example").cloned(),
                ));
            }
        }
        (out, counts)
    }

    // ------------------------------------------------------------------ identity and inventory

    #[test]
    fn openapi_document_identity() {
        let doc = doc();
        assert!(doc["openapi"].as_str().unwrap().starts_with("3.1"));
        assert_eq!(doc["info"]["title"], "Plumb Modeller API");
        assert_eq!(doc["info"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(doc["x-plumb-contract-version"], json!(2));
        assert_eq!(CONTRACT_VERSION, 2);
        assert!(doc.get("servers").is_none());
        let tags: BTreeSet<String> = doc["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            tags,
            set(&[
                "meta",
                "sources",
                "stages",
                "requirements",
                "glossary",
                "sentences",
                "entities",
                "calculations",
                "rules",
                "processes",
                "scenarios",
                "findings",
                "assumptions",
                "questions",
                "proposals",
                "sessions",
                "decisions",
                "readiness",
                "export"
            ])
        );
    }

    #[test]
    fn openapi_inventory_is_exact() {
        let doc = doc();
        let ops = operations(&doc);
        assert_eq!(ops.len(), 72);
        let found: BTreeSet<(String, String, String)> = ops
            .iter()
            .map(|(m, p, o)| {
                (
                    m.clone(),
                    p.clone(),
                    o["operationId"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let expected: BTreeSet<(String, String, String)> = INVENTORY
            .iter()
            .map(|(m, p, o)| ((*m).to_owned(), (*p).to_owned(), (*o).to_owned()))
            .collect();
        assert_eq!(found, expected);
        let ids: BTreeSet<&str> = INVENTORY.iter().map(|i| i.2).collect();
        assert_eq!(ids.len(), 72);
        // The production table is the frozen order.
        let production: Vec<(&str, &str, &str)> = OPERATIONS
            .iter()
            .map(|s| (s.method, s.path, s.operation_id))
            .collect();
        assert_eq!(production, INVENTORY.to_vec());
        assert_eq!(
            OPERATIONS.iter().map(|s| s.number).collect::<Vec<_>>(),
            (1..=72).collect::<Vec<_>>()
        );
        // Tags match the production table and every operation has exactly one.
        for spec in &OPERATIONS {
            let op = operation(&doc, spec.method, spec.path);
            assert_eq!(op["tags"], json!([spec.tag]), "{}", spec.operation_id);
        }
        assert_eq!(
            operation(&doc, "GET", "/api/events")["tags"],
            json!(["meta"])
        );
        // Paths shared by several methods: none in A0.1.
        let shared: Vec<&String> = doc["paths"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, item)| item.as_object().unwrap().len() > 1)
            .map(|(p, _)| p)
            .collect();
        assert!(shared.is_empty(), "{shared:?}");
        assert_eq!(doc["paths"].as_object().unwrap().len(), 72);
    }

    #[test]
    fn openapi_exclusions_and_proposal_accept() {
        let doc = doc();
        let paths = doc["paths"].as_object().unwrap();
        assert!(!paths.contains_key("/api/model/calculations/{id}/edit"));
        for absent in [
            "/api/baselines",
            "/api/gates/{gate}",
            "/api/provenance/{id}",
            "/api/impact/{id}",
        ] {
            assert!(!paths.contains_key(absent), "{absent}");
        }
        assert!(paths.keys().all(|p| !p.starts_with("/api/tests")));
        assert_eq!(
            schema(&doc, "ProcessStepOp")["enum"],
            json!(["add", "move", "remove"])
        );
        assert_eq!(schema(&doc, "ProcessFormat")["enum"], json!(["mermaid"]));
        assert!(!doc.to_string().contains("\"svg\""));
        assert!(!doc.to_string().contains("\"branch\""));
        let export = operation(&doc, "GET", "/api/processes/{id}/export");
        assert!(export["responses"]["200"]["content"]["text/plain"].is_object());
        let format = export["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "format")
            .unwrap()
            .clone();
        assert_eq!(format["schema"], component_ref("ProcessFormat"));
        let import = schema(&doc, "ProcessImportRequest");
        assert_eq!(
            import["properties"]["format"],
            component_ref("ProcessFormat")
        );
        // Proposal accept: If-Match, 412, write result, optional reason.
        let accept = operation(&doc, "POST", "/api/proposals/{id}/accept");
        assert_eq!(accept["operationId"], "accept_proposal");
        assert!(accept["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "If-Match" && p["required"] == true));
        assert!(accept["responses"]["412"].is_object());
        assert_eq!(
            accept["responses"]["200"]["content"]["application/json"]["schema"],
            component_ref("WriteResult")
        );
        assert_eq!(
            accept["requestBody"]["content"]["application/json"]["schema"],
            component_ref("ProposalActionRequest")
        );
        assert_eq!(properties(&doc, "ProposalActionRequest"), set(&["reason"]));
        assert!(schema(&doc, "ProposalActionRequest")
            .get("required")
            .is_none());
        // Export formats and stage names.
        assert_eq!(
            schema(&doc, "ExportFormat")["enum"],
            json!(["functional", "report", "decisions", "glossary"])
        );
        assert_eq!(
            schema(&doc, "StageName")["enum"],
            json!([
                "intake",
                "lint",
                "extract",
                "consolidate",
                "calc",
                "rules",
                "analyse",
                "flows",
                "scenarios",
                "tags",
                "run"
            ])
        );
        let model_export = operation(&doc, "GET", "/api/model/export");
        assert_eq!(
            model_export["responses"]["200"]["content"]["application/octet-stream"]["schema"],
            component_ref("BinaryFile")
        );
        assert_eq!(
            schema(&doc, "BinaryFile"),
            json!({"description": "A binary file body.", "format": "binary", "type": "string"})
        );
        let upload = operation(&doc, "POST", "/api/sources");
        let multipart = &upload["requestBody"]["content"]["multipart/form-data"];
        assert_eq!(multipart["schema"], component_ref("SourceUpload"));
        assert_eq!(schema(&doc, "SourceUpload")["required"], json!(["file"]));
        assert_eq!(
            schema(&doc, "SourceUpload")["properties"]["file"]["format"],
            "binary"
        );
    }

    // ------------------------------------------------------------------ HTTP concurrency contract

    #[test]
    fn openapi_every_post_requires_if_match_and_documents_412() {
        let doc = doc();
        let mut posts = 0;
        for (method, path, op) in operations(&doc) {
            if method != "POST" {
                continue;
            }
            posts += 1;
            let if_match: Vec<&Value> = op["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["name"] == "If-Match")
                .collect();
            assert_eq!(if_match.len(), 1, "{path}");
            assert_eq!(if_match[0]["in"], "header");
            assert_eq!(if_match[0]["required"], true);
            assert_eq!(if_match[0]["schema"], json!({"type": "string"}));
            assert_eq!(
                op["responses"]["412"]["content"]["application/json"]["schema"],
                component_ref("PreconditionFailedResponse"),
                "{path}"
            );
            // Every success is a flat write result carrying the four metadata fields.
            let success = &op["responses"]["200"]["content"]["application/json"]["schema"]["$ref"];
            let name = success.as_str().unwrap().rsplit('/').next().unwrap();
            let fields = properties(&doc, name);
            for field in ["version", "revision", "semantic_hash", "events"] {
                assert!(fields.contains(field), "{path}: {name} lacks {field}");
            }
            assert!(schema(&doc, name).get("allOf").is_none());
        }
        assert_eq!(posts, 37);
        assert_eq!(
            properties(&doc, "PreconditionFailedResponse"),
            set(&["code", "message", "details", "revision", "semantic_hash"])
        );
        assert_eq!(
            properties(&doc, "WriteResult"),
            set(&["version", "revision", "semantic_hash", "events"])
        );
        assert_eq!(
            properties(&doc, "DesignEvent"),
            set(&["kind", "ids", "version"])
        );
        let extra = |name: &str| -> BTreeSet<String> {
            properties(&doc, name)
                .difference(&set(&["version", "revision", "semantic_hash", "events"]))
                .cloned()
                .collect()
        };
        assert_eq!(
            extra("SourceUploadResponse"),
            set(&["id", "hash", "spans_count"])
        );
        assert_eq!(extra("StageRunResponse"), set(&["job_id"]));
        assert_eq!(
            extra("AnswerResponse"),
            set(&["proposal_id", "intake", "patch_preview"])
        );
        assert_eq!(extra("CreateSessionResponse"), set(&["id"]));
        assert_eq!(
            extra("SessionTurnResponse"),
            set(&["reply", "proposals", "asked_question"])
        );
        assert_eq!(extra("IntakeResponse"), set(&["intake"]));
    }

    #[test]
    fn openapi_every_ordinary_get_has_an_etag() {
        let doc = doc();
        let mut gets = 0;
        for (method, path, op) in operations(&doc) {
            if method != "GET" {
                continue;
            }
            let responses = op["responses"].as_object().unwrap();
            if path == "/api/events" {
                assert!(responses.values().all(|r| r.get("headers").is_none()));
                continue;
            }
            gets += 1;
            let successes: Vec<&Value> = responses
                .iter()
                .filter(|(s, _)| s.starts_with('2'))
                .map(|(_, r)| r)
                .collect();
            assert!(!successes.is_empty(), "{path}");
            for response in successes {
                assert_eq!(
                    response["headers"]["ETag"]["schema"],
                    json!({"type": "string"}),
                    "{path}"
                );
            }
            assert!(op["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .all(|p| p["name"] != "If-Match"));
        }
        assert_eq!(gets, 34);
    }

    #[test]
    fn openapi_pagination_contract() {
        let doc = doc();
        let mut pageable = BTreeSet::new();
        for (method, path, op) in operations(&doc) {
            let response = &op["responses"]["200"]["content"]["application/json"]["schema"]["$ref"];
            let is_page = response.as_str().is_some_and(|r| r.ends_with("Page"));
            let has_409 = op["responses"].get("409").is_some();
            assert_eq!(is_page, has_409, "{path}");
            if !is_page {
                continue;
            }
            assert_eq!(method, "GET");
            pageable.insert(path.clone());
            let name = response.as_str().unwrap().rsplit('/').next().unwrap();
            assert_eq!(properties(&doc, name), set(&["items", "next_cursor"]));
            assert_eq!(schema(&doc, name)["required"], json!(["items"]));
            assert_eq!(
                schema(&doc, name)["properties"]["next_cursor"]["type"],
                json!(["string", "null"])
            );
            let params: BTreeMap<String, Value> = op["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p["name"].as_str().unwrap().to_owned(), p.clone()))
                .collect();
            assert_eq!(params["cursor"]["schema"], json!({"type": "string"}));
            assert_eq!(params["cursor"]["required"], false);
            assert_eq!(
                params["limit"]["schema"],
                json!({"type": "integer", "minimum": 1})
            );
            assert_eq!(params["limit"]["required"], false);
            assert_eq!(
                op["responses"]["409"]["content"]["application/json"]["schema"],
                component_ref("CursorConflictResponse")
            );
        }
        assert_eq!(pageable, set(&PAGEABLE));
        assert!(properties(&doc, "CursorConflictResponse")
            .is_superset(&set(&["code", "message", "details"])));
        assert_eq!(
            schema(&doc, "CursorErrorCode")["enum"],
            json!(["E_CURSOR_BASELINE_CHANGED"])
        );
        // v2 filters are preserved.
        let names = |path: &str| -> BTreeSet<String> {
            operation(&doc, "GET", path)["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["name"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(
            names("/api/search"),
            set(&["q", "scope", "cursor", "limit"])
        );
        assert_eq!(
            names("/api/requirements"),
            set(&["status", "class", "q", "cursor", "limit"])
        );
        assert_eq!(
            names("/api/model/sentences"),
            set(&["entity", "status", "cursor", "limit"])
        );
        assert_eq!(
            names("/api/scenarios"),
            set(&["requirement", "status", "derived", "cursor", "limit"])
        );
        assert_eq!(
            names("/api/findings"),
            set(&["status", "severity", "code", "family", "cursor", "limit"])
        );
        assert_eq!(
            names("/api/questions"),
            set(&["stakeholder", "round", "status", "cursor", "limit"])
        );
        assert_eq!(
            names("/api/decisions"),
            set(&["by", "kind", "since", "cursor", "limit"])
        );
        assert_eq!(names("/api/readiness"), set(&["group"]));
        assert_eq!(names("/api/model/diff"), set(&["from", "to"]));
    }

    // ------------------------------------------------------------------ v3 overrides

    #[test]
    fn openapi_gates_model_version_and_jobs() {
        let doc = doc();
        let gates = operation(&doc, "GET", "/api/gates");
        let body = &gates["responses"]["200"]["content"]["application/json"]["schema"];
        assert_eq!(body["type"], "array");
        assert_eq!(body["items"], component_ref("GateStatus"));
        assert!(gates["parameters"].is_null());
        assert_eq!(
            gates["x-plumb-gate-order"],
            json!(["I0", "F1", "F2", "F3", "F4"])
        );
        assert_eq!(
            schema(&doc, "GateName")["enum"],
            json!(["I0", "F1", "F2", "F3", "F4"])
        );
        assert!(properties(&doc, "GateStatus").is_superset(&set(&["gate", "passed", "findings"])));
        // Model version: exactly the five fields, aliases equal in the example.
        assert_eq!(
            properties(&doc, "ModelVersionResponse"),
            set(&[
                "version",
                "hash",
                "revision",
                "semantic_hash",
                "evidence_hash"
            ])
        );
        let example = &operation(&doc, "GET", "/api/model/version")["responses"]["200"]["content"]
            ["application/json"]["example"];
        assert_eq!(example["version"], example["revision"]);
        assert_eq!(example["hash"], example["semantic_hash"]);
        // Jobs.
        let job = schema(&doc, "JobResponse");
        assert!(properties(&doc, "JobResponse").is_superset(&set(&[
            "status",
            "progress",
            "base_revision",
            "result_version",
            "error"
        ])));
        for flag in ["is_stale", "stale"] {
            assert!(job["properties"].get(flag).is_none());
        }
        assert_eq!(job["properties"]["status"], component_ref("JobStatus"));
        assert_eq!(
            schema(&doc, "JobStatus")["enum"],
            json!([
                "queued",
                "acquiring",
                "evaluating",
                "ready",
                "stale",
                "failed",
                "committed"
            ])
        );
        assert_eq!(job["properties"]["progress"]["minimum"], json!(0));
        assert_eq!(job["properties"]["progress"]["maximum"], json!(1));
        assert_eq!(
            job["properties"]["result_version"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(
            job["properties"]["error"]["type"],
            json!(["string", "null"])
        );
        let job_example = &operation(&doc, "GET", "/api/jobs/{id}")["responses"]["200"]["content"]
            ["application/json"]["example"];
        assert_eq!(job_example["progress"], json!(0.0));
        assert_eq!(job_example["status"], "acquiring");
        // Readiness: advisory nullable composite and the five sub-scores.
        let readiness = schema(&doc, "Readiness");
        assert_eq!(
            readiness["properties"]["overall"]["type"],
            json!(["number", "null"])
        );
        assert_eq!(
            properties(&doc, "ReadinessSubScores"),
            set(&[
                "clarity",
                "completeness",
                "consistency",
                "verifiability",
                "confirmation"
            ])
        );
    }

    #[test]
    fn openapi_websocket_compatibility_operation() {
        let doc = doc();
        let events = operation(&doc, "GET", "/api/events");
        assert_eq!(events["operationId"], "events_websocket");
        let responses = events["responses"].as_object().unwrap();
        assert_eq!(responses.keys().collect::<Vec<_>>(), ["101"]);
        assert!(responses["101"].get("content").is_none());
        let ws = &events["x-plumb-websocket"];
        assert_eq!(ws["direction"], "server");
        assert_eq!(ws["message_schema"], component_ref("DesignEvent"));
        assert_eq!(
            ws["message_example"],
            json!({"ids": ["example"], "kind": "example", "version": "example"})
        );
        assert!(
            validate_example(&doc, &ws["message_schema"], &ws["message_example"], "ws").is_ok()
        );
    }

    #[test]
    fn openapi_domain_dtos_keep_v2_essentials() {
        let doc = doc();
        let has = |name: &str, fields: &[&str]| {
            let props = properties(&doc, name);
            for f in fields {
                assert!(props.contains(*f), "{name} lacks {f}");
            }
        };
        has(
            "RequirementSummary",
            &[
                "id",
                "text",
                "class",
                "status",
                "smells",
                "coverage",
                "readiness",
            ],
        );
        has("RequirementCoverage", &["operations", "scenarios"]);
        has(
            "RequirementDetail",
            &[
                "id",
                "text",
                "class",
                "status",
                "smells",
                "coverage",
                "readiness",
                "spans",
                "lint_findings",
                "ears_suggestion",
                "linked",
            ],
        );
        has(
            "RequirementLinks",
            &["operations", "processes", "scenarios"],
        );
        has(
            "Sentence",
            &["id", "text", "slots", "status", "evidence", "unknowns"],
        );
        has("SentenceUnknown", &["slot", "question_id"]);
        has(
            "Entity",
            &[
                "id",
                "name",
                "attributes",
                "relationships",
                "states",
                "transitions",
                "tags",
            ],
        );
        has("CrudMatrix", &["operations", "entities", "cells"]);
        has("CrudCell", &["op", "ent", "c", "r", "u", "d"]);
        has("Calculation", &["id", "name", "expr"]);
        has("CalculationExample", &["expr", "rendered_example", "value"]);
        has(
            "Rule",
            &[
                "id",
                "name",
                "conditions",
                "rows",
                "uncovered_cells",
                "overlaps",
            ],
        );
        has(
            "Process",
            &["id", "name", "lanes", "steps", "branches", "validation"],
        );
        has(
            "Scenario",
            &[
                "id",
                "given",
                "when",
                "then",
                "status",
                "derived_from",
                "run",
            ],
        );
        has("ScenarioRun", &["result", "diff", "missing", "trace"]);
        has(
            "Question",
            &["id", "kind", "text", "slots", "context", "priority"],
        );
        has(
            "QuestionContext",
            &["sentence", "flow_fragment", "scenario"],
        );
        has("IntakeRequest", &["kind", "payload", "evidence"]);
        has(
            "IntakeReportView",
            &[
                "covered_by",
                "conflicts_with",
                "refines",
                "new_terms",
                "novelty",
                "duplicate_candidates",
                "validation_findings",
                "reason_codes",
                "requires_reason",
            ],
        );
        has("SessionProposalView", &["id", "kind", "payload", "intake"]);
        has(
            "Decision",
            &[
                "id",
                "proposal_ref",
                "question_ref",
                "decided_by",
                "decided_at",
                "rationale",
            ],
        );
        has(
            "Finding",
            &[
                "id",
                "code",
                "family",
                "severity",
                "status",
                "message",
                "affected_refs",
                "waiver_ref",
            ],
        );
        has("Source", &["id", "hash", "media_type", "source_kind"]);
        has("SearchHit", &["id", "kind", "path", "score", "snippet"]);
        has("ModelDiff", &["from", "to", "added", "removed", "modified"]);
        assert_eq!(
            properties(&doc, "SessionDetail"),
            set(&["id", "scope", "assistant_actor_ref"])
        );
        assert_eq!(
            properties(&doc, "SessionMetrics"),
            set(&["turns", "proposals", "requires_reason"])
        );
        assert_eq!(
            schema(&doc, "ScenarioRunResult")["enum"],
            json!(["pass", "fail", "undecidable"])
        );
        assert_eq!(
            schema(&doc, "IntakeReasonCode")["enum"],
            json!([
                "material_decision",
                "human_decision",
                "exact_equivalent",
                "declared_conflict",
                "validation_conflict",
                "ambiguous_term"
            ])
        );
        // Answer: exactly {value} or {text}, never both.
        let answer = schema(&doc, "AnswerRequest");
        assert_eq!(
            answer["oneOf"],
            json!([
                component_ref("AnswerByValue"),
                component_ref("AnswerByText")
            ])
        );
        assert_eq!(schema(&doc, "AnswerByValue")["additionalProperties"], false);
        assert_eq!(schema(&doc, "AnswerByText")["additionalProperties"], false);
        assert!(validate_example(
            &doc,
            &component_ref("AnswerRequest"),
            &json!({"value": 3}),
            "a"
        )
        .is_ok());
        assert!(validate_example(
            &doc,
            &component_ref("AnswerRequest"),
            &json!({"text": "x"}),
            "a"
        )
        .is_ok());
        assert!(validate_example(
            &doc,
            &component_ref("AnswerRequest"),
            &json!({"value": 3, "text": "x"}),
            "a"
        )
        .is_err());
        assert!(validate_example(
            &doc,
            &component_ref("AnswerRequest"),
            &json!({"value": {"x": 1}}),
            "a"
        )
        .is_err());
        let value_branches = schema(&doc, "ScalarValue")["oneOf"].clone();
        let types: Vec<&str> = value_branches
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["string", "number", "boolean"]);
        // Fixed request DTOs reject unknown fields.
        for name in [
            "ProposalActionRequest",
            "IntakeRequest",
            "SessionTurnRequest",
            "ProcessStepRequest",
            "ClassifyRequest",
        ] {
            assert_eq!(schema(&doc, name)["additionalProperties"], false, "{name}");
        }
    }

    // ------------------------------------------------------------------ examples

    #[test]
    fn openapi_every_example_exists_and_validates() {
        let doc = doc();
        let (locations, counts) = example_locations(&doc);
        let mut invalid = Vec::new();
        for (location, schema, example) in &locations {
            let example = example
                .as_ref()
                .unwrap_or_else(|| panic!("{location} has no example"));
            if let Err(e) = validate_example(&doc, schema, example, location) {
                invalid.push(e.to_string());
            }
        }
        assert!(invalid.is_empty(), "{invalid:#?}");
        eprintln!(
            "examples validated: {} (parameters {}, request bodies {}, responses {}, response headers {}, websocket {}) invalid 0",
            locations.len(),
            counts.parameters,
            counts.request_bodies,
            counts.responses,
            counts.headers,
            counts.websocket
        );
        assert_eq!(counts.websocket, 1);
        // No fabricated request for zero-input operations.
        for path in ["/api/me", "/api/model/version", "/api/gates"] {
            let op = operation(&doc, "GET", path);
            assert!(
                op.get("requestBody").is_none() && op.get("parameters").is_none(),
                "{path}"
            );
        }
    }

    fn example_at(doc: &Value, method: &str, path: &str) -> Value {
        operation(doc, method, path)["responses"]["200"]["content"]["application/json"]["example"]
            .clone()
    }

    fn path_id_example(doc: &Value, method: &str, path: &str) -> Value {
        operation(doc, method, path)["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "id")
            .unwrap()["example"]
            .clone()
    }

    #[test]
    fn openapi_fixture_seeds_drive_examples() {
        let seeds = fixture_seeds().unwrap();
        let ids: Vec<(SeedFamily, &str)> =
            seeds.iter().map(|s| (s.family, s.id.as_str())).collect();
        assert_eq!(
            ids,
            [
                (SeedFamily::Requirement, "HR-001"),
                (SeedFamily::Entity, "ApprovalRecord"),
                (SeedFamily::Calculation, "AnnualBalanceDelta"),
                (SeedFamily::Rule, "AnnualLeaveEligibility"),
                (SeedFamily::Process, "CancelApprovedLeave"),
                (SeedFamily::Operation, "ApproveLeaveRequest"),
            ]
        );
        let operation_seed: &FixtureSeed = &seeds[5];
        assert_eq!(operation_seed.fields["kind"], "command");
        assert_eq!(operation_seed.fields["performer"], "ManagerRole");
        let doc = doc();
        let requirement = example_at(&doc, "GET", "/api/requirements/{id}");
        assert_eq!(requirement["id"], "HR-001");
        assert_eq!(
            requirement["text"],
            "The system shall allow an employee to create a draft leave request."
        );
        assert_eq!(requirement["class"], "functional");
        assert_eq!(
            example_at(&doc, "GET", "/api/requirements")["items"][0]["id"],
            "HR-001"
        );
        let entity = example_at(&doc, "GET", "/api/model/entities/{id}");
        assert_eq!(
            (entity["id"].as_str(), entity["name"].as_str()),
            (Some("ApprovalRecord"), Some("ApprovalRecord"))
        );
        let calculation = example_at(&doc, "GET", "/api/model/calculations");
        assert_eq!(calculation["items"][0]["id"], "AnnualBalanceDelta");
        assert_eq!(
            calculation["items"][0]["expr"],
            "if leave_type == Annual then RequestedWorkingDays else 0"
        );
        assert_eq!(
            example_at(&doc, "GET", "/api/model/calculations/{id}/example")["expr"],
            "if leave_type == Annual then RequestedWorkingDays else 0"
        );
        let rule = example_at(&doc, "GET", "/api/model/rules/{id}");
        assert_eq!(rule["id"], "AnnualLeaveEligibility");
        assert_eq!(rule["name"], "AnnualLeaveEligibility");
        let process = example_at(&doc, "GET", "/api/processes/{id}");
        assert_eq!(process["id"], "CancelApprovedLeave");
        assert_eq!(
            process["steps"],
            json!(["CancelLeaveRequest", "RestoreLeaveBalance"])
        );
        // Optional properties appear only when seeded.
        assert!(requirement.get("ears_suggestion").is_none());
        assert!(requirement.get("readiness").is_none());
        // Path IDs of the seeded families.
        for (path, id) in [
            ("/api/requirements/{id}", "HR-001"),
            ("/api/requirements/{id}/readiness", "HR-001"),
            ("/api/model/entities/{id}", "ApprovalRecord"),
            ("/api/model/calculations/{id}/example", "AnnualBalanceDelta"),
            ("/api/model/rules/{id}", "AnnualLeaveEligibility"),
            ("/api/processes/{id}", "CancelApprovedLeave"),
        ] {
            assert_eq!(path_id_example(&doc, "GET", path), json!(id), "{path}");
        }
        assert_eq!(
            path_id_example(&doc, "POST", "/api/model/calculations/{id}/confirm"),
            json!("AnnualBalanceDelta")
        );
        assert_eq!(
            path_id_example(&doc, "GET", "/api/jobs/{id}"),
            json!("example")
        );
        assert_eq!(
            path_id_example(&doc, "POST", "/api/model/attributes/{id}/type"),
            json!("example")
        );
        // The fixture itself is the YAML file, read only at compile time.
        assert!(OPENAPI_SRC
            .contains("include_str!(\"../../../fixtures/hr-leave/fixture-contract.yaml\")"));
    }

    // ------------------------------------------------------------------ fallback engine

    fn components(schemas: Value) -> Map<String, Value> {
        schemas.as_object().cloned().unwrap()
    }

    fn fallback(schema: Value) -> Result<Value, OpenApiContractError> {
        fallback_with(json!({}), schema)
    }

    fn fallback_with(schemas: Value, schema: Value) -> Result<Value, OpenApiContractError> {
        let schemas = components(schemas);
        ExampleGenerator::new(&schemas, &[]).example(&schema, "unit")
    }

    #[test]
    fn openapi_fallback_rules() {
        assert_eq!(
            fallback(json!({"enum": ["b", "a", "c"]})).unwrap(),
            json!("a")
        );
        // Serialized JSON order: `"zeta"` (starting with a quote) < `10` < `9`.
        assert_eq!(
            fallback(json!({"enum": ["zeta", 10, 9]})).unwrap(),
            json!("zeta")
        );
        assert_eq!(fallback(json!({"enum": [9, 10]})).unwrap(), json!(10));
        assert_eq!(fallback(json!({"type": "boolean"})).unwrap(), json!(true));
        assert_eq!(fallback(json!({"type": "integer"})).unwrap(), json!(1));
        assert_eq!(
            fallback(json!({"type": "integer", "minimum": 5})).unwrap(),
            json!(5)
        );
        assert_eq!(fallback(json!({"type": "number"})).unwrap(), json!(1.0));
        assert_eq!(
            fallback(json!({"type": "number", "minimum": 5})).unwrap(),
            json!(5.0)
        );
        assert_eq!(
            fallback(json!({"type": "string", "format": "date"})).unwrap(),
            json!("2026-01-15")
        );
        assert_eq!(
            fallback(json!({"type": "string", "format": "date-time"})).unwrap(),
            json!("2026-01-15T12:00:00Z")
        );
        assert_eq!(
            fallback(json!({"type": "string", "format": "email"})).unwrap(),
            json!("example")
        );
        assert_eq!(
            fallback(json!({"type": "string", "default": "x", "example": "y"})).unwrap(),
            json!("example")
        );
        assert_eq!(
            fallback(json!({"type": "array", "items": {"type": "integer"}})).unwrap(),
            json!([1])
        );
        assert_eq!(
            fallback(json!({
                "type": "object",
                "required": ["b", "a"],
                "properties": {"a": {"type": "string"}, "b": {"type": "boolean"}, "c": {"type": "integer"}}
            }))
            .unwrap(),
            json!({"a": "example", "b": true})
        );
        // oneOf/anyOf: smallest RFC 8785 canonical branch, compared before dereferencing.
        assert_eq!(
            fallback(json!({"oneOf": [{"type": "string"}, {"type": "boolean"}]})).unwrap(),
            json!(true)
        );
        // `{"minLength":1,"type":"string"}` < `{"type":"integer"}` canonically.
        assert_eq!(
            fallback(json!({"anyOf": [{"type": "string", "minLength": 1}, {"type": "integer"}]}))
                .unwrap(),
            json!("example")
        );
        assert_eq!(
            fallback(json!({"anyOf": [{"type": "string"}, {"type": "integer"}]})).unwrap(),
            json!(1)
        );
        assert_eq!(
            fallback_with(
                json!({"Zed": {"type": "boolean"}}),
                json!({"anyOf": [{"type": "string"}, {"$ref": "#/components/schemas/Zed"}]})
            )
            .unwrap(),
            json!(true)
        );
        // Nullable: the non-null alternative.
        assert_eq!(
            fallback(json!({"type": ["null", "string"]})).unwrap(),
            json!("example")
        );
        assert_eq!(
            fallback(json!({"oneOf": [{"type": "null"}, {"type": "integer"}]})).unwrap(),
            json!(1)
        );
        // Local $ref.
        assert_eq!(
            fallback_with(
                json!({"Thing": {"type": "object", "required": ["id"], "properties": {"id": {"type": "string"}}}}),
                json!({"$ref": "#/components/schemas/Thing"})
            )
            .unwrap(),
            json!({"id": "example"})
        );
    }

    #[test]
    fn openapi_fallback_blocks_instead_of_inventing() {
        // A pattern that the fallback string does not satisfy is a typed blocked contract.
        let schema = json!({"type": "string", "pattern": "^[0-9a-f]{64}$"});
        let example = fallback(schema.clone()).unwrap();
        assert_eq!(example, json!("example"));
        let doc = json!({"components": {"schemas": {}}});
        let err = validate_example(&doc, &schema, &example, "unit pattern").unwrap_err();
        assert!(matches!(
            err,
            OpenApiContractError::ExampleValidation { .. }
        ));
        assert!(err.to_string().starts_with("BLOCKED-TEST-CONTRACT"));
        // Recursive cycles, unsupported keywords and dangling refs are typed errors.
        let cyclic = json!({"Node": {"type": "object", "required": ["next"], "properties": {"next": {"$ref": "#/components/schemas/Node"}}}});
        assert!(matches!(
            fallback_with(cyclic, json!({"$ref": "#/components/schemas/Node"})),
            Err(OpenApiContractError::UnsupportedFallback { .. })
        ));
        for construct in [
            json!({"allOf": [{"type": "string"}]}),
            json!({"not": {"type": "string"}}),
            json!({"type": "object", "patternProperties": {"^x": {"type": "string"}}}),
            json!({"const": 1}),
            json!({}),
            json!({"type": ["string", "integer"]}),
        ] {
            assert!(
                matches!(
                    fallback(construct.clone()),
                    Err(OpenApiContractError::UnsupportedFallback { .. })
                ),
                "{construct}"
            );
        }
        assert!(matches!(
            fallback(json!({"$ref": "#/components/schemas/Missing"})),
            Err(OpenApiContractError::MissingSchema(_))
        ));
        let err = fallback(json!({"allOf": []})).unwrap_err();
        assert!(err.to_string().contains("BLOCKED-TEST-CONTRACT"));
    }

    // ------------------------------------------------------------------ schema hygiene

    /// Every schema-level key and value; the keys of `properties` maps are property names,
    /// not keywords, and are skipped (their schemas are still visited), as are example values.
    fn walk<'v>(value: &'v Value, out: &mut Vec<(&'v str, &'v Value)>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    out.push((k.as_str(), v));
                    if k == "example" || k == "message_example" {
                        continue;
                    }
                    if k == "properties" {
                        for property in v.as_object().into_iter().flat_map(|p| p.values()) {
                            walk(property, out);
                        }
                    } else {
                        walk(v, out);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }

    fn refs_of(value: &Value) -> BTreeSet<String> {
        let mut entries = Vec::new();
        walk(value, &mut entries);
        entries
            .into_iter()
            .filter(|(k, _)| *k == "$ref")
            .map(|(_, v)| v.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn openapi_schema_hygiene() {
        let doc = doc();
        let mut entries = Vec::new();
        walk(&doc, &mut entries);
        for forbidden in [
            "allOf",
            "not",
            "if",
            "then",
            "else",
            "dependentSchemas",
            "patternProperties",
        ] {
            assert!(!entries.iter().any(|(k, _)| *k == forbidden), "{forbidden}");
        }
        let schemas = doc["components"]["schemas"].as_object().unwrap();
        let all_refs = refs_of(&doc);
        for reference in &all_refs {
            let name = reference
                .strip_prefix("#/components/schemas/")
                .unwrap_or_else(|| panic!("non-local ref {reference}"));
            assert!(schemas.contains_key(name), "dangling {reference}");
        }
        // No $ref cycle among components.
        let edges: BTreeMap<&str, BTreeSet<String>> = schemas
            .iter()
            .map(|(name, s)| {
                (
                    name.as_str(),
                    refs_of(s)
                        .into_iter()
                        .map(|r| r.trim_start_matches("#/components/schemas/").to_owned())
                        .collect(),
                )
            })
            .collect();
        fn visit(name: &str, edges: &BTreeMap<&str, BTreeSet<String>>, path: &mut Vec<String>) {
            assert!(!path.iter().any(|p| p == name), "cycle {path:?} -> {name}");
            path.push(name.to_owned());
            for next in &edges[name] {
                visit(next, edges, path);
            }
            path.pop();
        }
        for name in edges.keys() {
            visit(name, &edges, &mut Vec::new());
        }
        // Component names are unique case-insensitively as well.
        let lower: BTreeSet<String> = schemas.keys().map(|k| k.to_lowercase()).collect();
        assert_eq!(lower.len(), schemas.len());
        // Open objects only for the v2 open fields.
        let mut open = Vec::new();
        for (name, s) in schemas {
            for (property, p) in s["properties"].as_object().into_iter().flatten() {
                if p["type"] == "object" && p.get("properties").is_none() {
                    open.push(format!("{name}.{property}"));
                }
            }
        }
        assert_eq!(
            open,
            [
                "AnswerResponse.patch_preview",
                "CursorConflictResponse.details",
                "IntakeRequest.payload",
                "PreconditionFailedResponse.details",
                "Question.slots",
                "Sentence.slots",
                "SentenceEditRequest.slots",
                "SessionProposalView.payload",
            ]
        );
        eprintln!("components: {}", schemas.len());
    }

    // ------------------------------------------------------------------ rendering and drift

    #[test]
    fn openapi_rendering_is_repeatable() {
        let a = render_openapi_json().unwrap();
        assert_eq!(a, render_openapi_json().unwrap());
        assert!(a.ends_with("}\n") && !a.ends_with("\n\n"));
        let m = render_api_markdown().unwrap();
        assert_eq!(m, render_api_markdown().unwrap());
        assert!(m.ends_with("|\n") && !m.ends_with("\n\n"));
        // Keys are sorted lexicographically in the rendered document.
        let parsed: Value = serde_json::from_str(&a).unwrap();
        let paths: Vec<&String> = parsed["paths"].as_object().unwrap().keys().collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted);
        assert_eq!(parsed, doc());
    }

    #[test]
    fn committed_openapi_matches_rust_generation() {
        let committed = std::fs::read_to_string(repo_file("api/modeller.openapi.json")).unwrap();
        assert_eq!(committed, render_openapi_json().unwrap());
        let parsed: Value = serde_json::from_str(&committed).unwrap();
        assert_eq!(parsed, openapi_document().unwrap());
    }

    #[test]
    fn committed_openapi_api_markdown_matches_rust_generation() {
        let committed = std::fs::read_to_string(repo_file("docs/api.md")).unwrap();
        assert_eq!(committed, render_api_markdown().unwrap());
    }

    #[test]
    fn openapi_markdown_contents() {
        let m = render_api_markdown().unwrap();
        assert!(m.starts_with(
            "# Plumb Pilot API\n\n> Generated from the Rust OpenAPI contract. Do not edit by hand.\n"
        ));
        for needle in [
            "Contract version 2",
            "72 operations",
            "A0.1 defines the pilot contract only; A0.2 and A0.3 provide the runtime handlers.",
            "ETag",
            "If-Match",
            "412 Precondition Failed",
            "E_CURSOR_BASELINE_CHANGED",
            "`/api/events` is a WebSocket upgrade endpoint; the OpenAPI GET operation exists for compatibility/documentation and must not be treated as an ordinary JSON GET call.",
            "I0, F1, F2, F3 and F4",
            "`base_revision`",
            "the composite is uncalibrated and non-authoritative in the pilot",
            "`null` means not calculated by the current pilot runtime, not zero",
            "Cursor v2",
            "E_CURSOR_REQUEST_MISMATCH",
            "E_CURSOR_INVALID",
            "- A0.2: 30 operations (9 POST, 21 GET).",
            "- A0.3: 42 operations (28 POST, 14 GET).",
            "A0.3 must not start directly after A0.2",
            "`version` == `revision` and `hash` == `semantic_hash`",
            "D test endpoints are not in the pilot API.",
            "POST calculation edit is not in NOW P-R scope",
            "Process branch mutation is not in NOW P-R scope",
            "Process SVG export is not in NOW P-R scope",
            "The additional generic v3 engineering endpoints are introduced gradually and are not part of A0.1 except proposal accept.",
        ] {
            assert!(m.contains(needle), "missing {needle:?}");
        }
        // The table follows the frozen inventory order.
        let rows: Vec<&str> = m
            .lines()
            .filter(|l| l.starts_with("| ") && !l.starts_with("| #"))
            .collect();
        assert_eq!(rows.len(), 72);
        for ((row, (method, path, id)), spec) in rows.iter().zip(INVENTORY).zip(OPERATIONS.iter()) {
            assert!(
                row.contains(&format!("| {method} | `{path}` | `{id}` |")),
                "{row}"
            );
            assert!(row.ends_with(&format!("| {} |", spec.owner)), "{row}");
        }
    }

    // ------------------------------------------------------------------ Hotfix 063

    /// The 42 operations deferred to A0.3, written independently of the production table.
    const A03: [&str; 42] = [
        "search_model",
        "run_stage",
        "get_job",
        "list_sentences",
        "accept_sentence",
        "edit_sentence",
        "reject_sentence",
        "split_requirement",
        "waive_requirement_finding",
        "mark_requirement_duplicate",
        "merge_glossary",
        "get_crud_matrix",
        "get_calculation_example",
        "list_rules",
        "get_rule",
        "set_rule_cell",
        "mutate_process_steps",
        "import_process",
        "export_process",
        "list_scenarios",
        "confirm_scenario",
        "correct_scenario",
        "confirm_derived_scenarios",
        "run_scenarios",
        "get_scenario_run",
        "waive_finding",
        "resolve_assumption",
        "list_questions",
        "answer_question",
        "create_round",
        "get_round",
        "intake_proposal",
        "link_proposal",
        "merge_proposal",
        "supersede_proposal",
        "create_proposal",
        "reject_proposal",
        "accept_proposal",
        "create_session",
        "create_session_turn",
        "get_session",
        "get_session_metrics",
    ];

    #[test]
    fn openapi_hotfix_063_ownership() {
        let count = |owner: &str, method: Option<&str>| {
            OPERATIONS
                .iter()
                .filter(|s| s.owner == owner && method.is_none_or(|m| s.method == m))
                .count()
        };
        assert_eq!(
            (
                count("A0.2", None),
                count("A0.2", Some("POST")),
                count("A0.2", Some("GET"))
            ),
            (30, 9, 21)
        );
        assert_eq!(
            (
                count("A0.3", None),
                count("A0.3", Some("POST")),
                count("A0.3", Some("GET"))
            ),
            (42, 28, 14)
        );
        assert!(OPERATIONS
            .iter()
            .all(|s| s.owner == "A0.2" || s.owner == "A0.3"));
        let a03: BTreeSet<&str> = OPERATIONS
            .iter()
            .filter(|s| s.owner == "A0.3")
            .map(|s| s.operation_id)
            .collect();
        assert_eq!(a03, A03.iter().copied().collect());
        let a02_posts: BTreeSet<&str> = OPERATIONS
            .iter()
            .filter(|s| s.owner == "A0.2" && s.method == "POST")
            .map(|s| s.operation_id)
            .collect();
        assert_eq!(
            a02_posts,
            [
                "create_source",
                "classify_requirement",
                "accept_requirement_ears",
                "confirm_glossary",
                "edit_glossary",
                "set_attribute_type",
                "set_attribute_unit",
                "set_attribute_class",
                "confirm_calculation"
            ]
            .into_iter()
            .collect()
        );
        let a02_pages: BTreeSet<&str> = OPERATIONS
            .iter()
            .filter(|s| s.owner == "A0.2" && s.pageable)
            .map(|s| s.operation_id)
            .collect();
        assert_eq!(
            a02_pages,
            [
                "get_source_spans",
                "list_requirements",
                "list_glossary",
                "list_entities",
                "list_calculations",
                "list_processes",
                "list_findings",
                "list_assumptions",
                "list_decisions"
            ]
            .into_iter()
            .collect()
        );
        // No operation was added or removed.
        let doc = doc();
        assert_eq!(operations(&doc).len(), 72);
    }

    #[test]
    fn openapi_hotfix_063_cursor_v2() {
        let doc = doc();
        let mut pageable = 0;
        for (_, path, op) in operations(&doc) {
            let Some(cursor) = op["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|p| p["name"] == "cursor")
            else {
                continue;
            };
            pageable += 1;
            let description = cursor["description"].as_str().unwrap();
            assert!(
                description.contains(
                    "{\"after_id\":\"<last-returned-id>\",\"request_hash\":\"sha256:...\",\"revision\":\"rev:...\",\"semantic_hash\":\"psg:sha256:...\"}"
                ),
                "{path}"
            );
            assert!(description.contains("exactly these four fields"), "{path}");
            assert!(
                description.contains("E_CURSOR_INVALID")
                    && description.contains("E_CURSOR_REQUEST_MISMATCH")
            );
            // The superseded two-field cursor is gone.
            assert!(!description.contains("\"after_id\":\"<last-returned-id>\",\"semantic_hash\""));
            assert!(op["responses"]["409"]["description"]
                .as_str()
                .unwrap()
                .contains("revision or semantic_hash"));
        }
        assert_eq!(pageable, 14);
        assert!(!doc
            .to_string()
            .contains("\\\"after_id\\\":\\\"<last-returned-id>\\\",\\\"semantic_hash"));
    }

    #[test]
    fn openapi_hotfix_063_wire_corrections() {
        let doc = doc();
        let ears = schema(&doc, "AcceptEarsRequest");
        assert_eq!(ears["required"], json!(["text"]));
        assert_eq!(ears["properties"]["text"]["type"], "string");
        assert_eq!(ears["additionalProperties"], false);
        let sub = schema(&doc, "ReadinessSubScores");
        let fields = [
            "clarity",
            "completeness",
            "consistency",
            "verifiability",
            "confirmation",
        ];
        assert_eq!(properties(&doc, "ReadinessSubScores"), set(&fields));
        for field in fields {
            assert_eq!(
                sub["properties"][field]["type"],
                json!(["number", "null"]),
                "{field}"
            );
            assert!(validate_example(
                &doc,
                &component_ref("ReadinessSubScores"),
                &json!({
                    "clarity": null, "completeness": null, "consistency": null,
                    "verifiability": null, "confirmation": null
                }),
                "readiness"
            )
            .is_ok());
        }
        let mut required: Vec<&str> = sub["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        required.sort();
        let mut expected = fields.to_vec();
        expected.sort();
        assert_eq!(required, expected);
        let readiness = schema(&doc, "Readiness");
        assert_eq!(
            properties(&doc, "Readiness"),
            set(&["overall", "sub", "contributing"])
        );
        assert_eq!(
            readiness["properties"]["overall"]["type"],
            json!(["number", "null"])
        );
        // The all-null runtime readiness validates.
        assert!(validate_example(&doc, &component_ref("Readiness"), &json!({
            "overall": null,
            "sub": {"clarity": null, "completeness": null, "consistency": null, "verifiability": null, "confirmation": null},
            "contributing": []
        }), "readiness").is_ok());
        assert!(validate_example(
            &doc,
            &component_ref("AcceptEarsRequest"),
            &json!({}),
            "ears"
        )
        .is_err());
    }

    // ------------------------------------------------------------------ source guards

    #[test]
    fn openapi_production_source_guards() {
        let code: String = [LIB_SRC, OPENAPI_SRC]
            .iter()
            .flat_map(|s| s.lines())
            .map(|line| line.split("//").next().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");
        for token in [
            "Router::new",
            "axum::serve",
            "TcpListener",
            "State<",
            "SqliteRevisionStore",
            "GraphStore",
            "RevisionStore",
            "store.commit",
            "apply_patch",
            "intake(",
            "prepare_turn(",
            "complete_turn(",
            "provider.execute",
            ".execute(",
            "std::fs",
            "File::open",
            "read_to_string",
            "PathBuf",
            "Clock",
            "SystemTime",
            "Utc::now",
            "reqwest",
            "std::net",
            "tokio",
            "Extension<",
            "Multipart",
            "WebSocketUpgrade",
            "Json<",
            "std::env::var",
            "OnceLock",
            "lazy_static",
            "static mut",
        ] {
            assert!(!code.contains(token), "production code contains {token:?}");
        }
    }
}
