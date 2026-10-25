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

    /// The frozen Hotfix 062 inventory (01-72) with the two U2.1 semantic change previews
    /// appended by Hotfix 073 (73-74), written independently of the production table.
    const INVENTORY: [(&str, &str, &str); 74] = [
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
        (
            "POST",
            "/api/requirements/{id}/split/preview",
            "preview_requirement_split",
        ),
        (
            "POST",
            "/api/glossary/{id}/merge/preview",
            "preview_glossary_merge",
        ),
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
        assert_eq!(doc["x-plumb-contract-version"], json!(3));
        assert_eq!(CONTRACT_VERSION, 3);
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
        assert_eq!(ops.len(), 74);
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
        assert_eq!(ids.len(), 74);
        // The production table is the frozen order.
        let production: Vec<(&str, &str, &str)> = OPERATIONS
            .iter()
            .map(|s| (s.method, s.path, s.operation_id))
            .collect();
        assert_eq!(production, INVENTORY.to_vec());
        assert_eq!(
            OPERATIONS.iter().map(|s| s.number).collect::<Vec<_>>(),
            (1..=74).collect::<Vec<_>>()
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
        assert_eq!(doc["paths"].as_object().unwrap().len(), 74);
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
        let step_ops: Vec<Value> = schema(&doc, "ProcessStepRequest")["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["properties"]["op"]["enum"].clone())
            .collect();
        // Hotfix 079 adds move_lane (snake_case, never movelane).
        assert_eq!(
            step_ops,
            [
                json!(["add"]),
                json!(["move"]),
                json!(["remove"]),
                json!(["move_lane"])
            ]
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
            // H0.2 (§33.2): the token is the head revision ID, never the semantic hash.
            let description = if_match[0]["description"].as_str().unwrap();
            assert!(description.contains("revision ID"), "{path}");
            assert!(description.contains("weak"), "{path}");
            assert!(!description.contains("semantic_hash"), "{path}");
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
        assert_eq!(posts, 39);
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
            set(&["proposal_id", "action", "intake", "patch_preview"])
        );
        // Hotfix 076: the answer carries the authoritative ProposalKind of its pending Proposal.
        let answer = schema(&doc, "AnswerResponse");
        let fields = set(&[
            "version",
            "revision",
            "semantic_hash",
            "events",
            "proposal_id",
            "action",
            "intake",
            "patch_preview",
        ]);
        assert_eq!(properties(&doc, "AnswerResponse"), fields);
        assert_eq!(required(&doc, "AnswerResponse"), fields);
        assert_eq!(answer["required"].as_array().unwrap().len(), 8);
        assert_eq!(answer["additionalProperties"], json!(false));
        assert_eq!(
            answer["properties"]["action"]["$ref"],
            component_ref("ProposalKind")["$ref"]
        );
        // Hotfix 077: a session proposal card carries its authoritative ProposalKind.
        let card = schema(&doc, "SessionProposalView");
        let fields = set(&["id", "kind", "payload", "intake"]);
        assert_eq!(properties(&doc, "SessionProposalView"), fields);
        assert_eq!(required(&doc, "SessionProposalView"), fields);
        assert_eq!(card["required"].as_array().unwrap().len(), 4);
        assert_eq!(card["additionalProperties"], json!(false));
        assert_eq!(
            card["properties"]["kind"]["$ref"],
            component_ref("ProposalKind")["$ref"]
        );
        assert_eq!(extra("CreateSessionResponse"), set(&["id"]));
        assert_eq!(
            extra("SessionTurnResponse"),
            set(&["reply", "proposals", "asked_question"])
        );
        assert_eq!(extra("IntakeResponse"), set(&["intake"]));
        assert_eq!(
            extra("SemanticChangePreviewResponse"),
            set(&["proposal_id", "action", "intake", "diff"])
        );
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
                let description = response["headers"]["ETag"]["description"].as_str().unwrap();
                assert!(description.contains("strong entity-tag"), "{path}");
                assert!(description.contains("revision ID"), "{path}");
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
                "ears_proposal_ref",
                "linked",
            ],
        );
        // R0.6: the pending S1.2 EARS proposal reference is detail-only and nullable.
        let detail = schema(&doc, "RequirementDetail");
        assert!(detail["properties"]["ears_proposal_ref"]["type"]
            .as_array()
            .is_some_and(|t| t.contains(&json!("string")) && t.contains(&json!("null"))));
        assert!(!properties(&doc, "RequirementSummary").contains("ears_proposal_ref"));
        has(
            "RequirementLinks",
            &["operations", "processes", "scenarios"],
        );
        // R0.4: lint_current is a required boolean of both requirement read schemas.
        for name in ["RequirementSummary", "RequirementDetail"] {
            let s = schema(&doc, name);
            assert_eq!(s["properties"]["lint_current"]["type"], "boolean", "{name}");
            assert!(
                s["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("lint_current")),
                "{name} must require lint_current"
            );
        }
        assert_eq!(operations(&doc).len(), 74);
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
                "kind",
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
                "confirmation_status",
                "derived_from",
                "run",
            ],
        );
        has("ScenarioRun", &["result", "diff", "missing", "trace"]);
        assert_eq!(
            properties(&doc, "Question"),
            set(&[
                "id",
                "kind",
                "text",
                "status",
                "answer_schema",
                "stakeholder_ref",
                "priority",
                "round_ref",
                "context_refs"
            ])
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
                "rule_class",
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
        // Answer (Hotfix 065): a JSON object validated against the Question's answer_schema.
        let answer = schema(&doc, "AnswerRequest");
        assert_eq!(answer["required"], json!(["answer"]));
        assert_eq!(answer["additionalProperties"], false);
        for (example, ok) in [
            (json!({"answer": {"cardinality_from": "one"}}), true),
            (
                json!({"answer": {}, "rationale": "Agreed.", "supersedes": "dec:0000000000000001"}),
                true,
            ),
            (
                json!({"answer": {}, "rationale": null, "supersedes": null}),
                true,
            ),
            (json!({"answer": 3}), false),
            (json!({"value": 3}), false),
            (json!({"text": "x"}), false),
            (json!({}), false),
        ] {
            assert_eq!(
                validate_example(&doc, &component_ref("AnswerRequest"), &example, "a").is_ok(),
                ok,
                "{example}"
            );
        }
        // ScalarValue remains the assumption default projection.
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
            "ClassifyRequest",
        ] {
            assert_eq!(schema(&doc, name)["additionalProperties"], false, "{name}");
        }
        // The tagged process-step union rejects unknown fields in every branch.
        for branch in schema(&doc, "ProcessStepRequest")["oneOf"]
            .as_array()
            .unwrap()
        {
            assert_eq!(branch["additionalProperties"], false, "{branch}");
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
                "AnswerRequest.answer",
                "AnswerResponse.patch_preview",
                "CursorConflictResponse.details",
                "IntakeRequest.payload",
                "PreconditionFailedResponse.details",
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
            "Contract version 3",
            "74 operations",
            "A0.1 defines the pilot contract only; A0.2, A0.3 and A0.4 provide the runtime handlers of operations 1-72 and U2.1 those of operations 73-74.",
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
            "- A0.3: 40 operations (27 POST, 13 GET).",
            "- A0.4: 2 operations (1 POST, 1 GET).",
            "- U2.1: 2 operations (2 POST, 0 GET).",
            "- Hotfix 073 appends operations 73 and 74, the U2.1 semantic change previews",
            "`candidate_semantic_hash` is not a prediction of the committed `semantic_hash`",
            "- Hotfix 065 froze the deferred A0.3 compatibility semantics (contract version 3).",
            "- A0.4 owns `run_stage` and `get_job` and must not start before its separate stage-orchestration contract freeze.",
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
        assert_eq!(rows.len(), 74);
        for ((row, (method, path, id)), spec) in rows.iter().zip(INVENTORY).zip(OPERATIONS.iter()) {
            assert!(
                row.contains(&format!("| {method} | `{path}` | `{id}` |")),
                "{row}"
            );
            assert!(row.ends_with(&format!("| {} |", spec.owner)), "{row}");
        }
    }

    // ------------------------------------------------------------------ Hotfix 063

    /// The 40 A0.3 operations (Hotfix 065), written independently of the production table.
    const A03: [&str; 40] = [
        "search_model",
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

    /// The stage/job operations Hotfix 065 moved to A0.4.
    const A04: [&str; 2] = ["run_stage", "get_job"];

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
            (40, 27, 13)
        );
        assert_eq!(
            (
                count("A0.4", None),
                count("A0.4", Some("POST")),
                count("A0.4", Some("GET"))
            ),
            (2, 1, 1)
        );
        assert!(OPERATIONS
            .iter()
            .all(|s| ["A0.2", "A0.3", "A0.4", "U2.1"].contains(&s.owner)));
        assert_eq!(
            (
                count("U2.1", None),
                count("U2.1", Some("POST")),
                count("U2.1", Some("GET"))
            ),
            (2, 2, 0)
        );
        // Hotfix 073/074: U2.1 owns exactly the two appended semantic change previews.
        let u21: Vec<(u32, &str)> = OPERATIONS
            .iter()
            .filter(|s| s.owner == "U2.1")
            .map(|s| (s.number, s.operation_id))
            .collect();
        assert_eq!(
            u21,
            vec![
                (73, "preview_requirement_split"),
                (74, "preview_glossary_merge")
            ]
        );
        let a04: BTreeSet<&str> = OPERATIONS
            .iter()
            .filter(|s| s.owner == "A0.4")
            .map(|s| s.operation_id)
            .collect();
        assert_eq!(a04, A04.iter().copied().collect());
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
        // Only Hotfix 073 added operations (73 and 74); none was removed.
        let doc = doc();
        assert_eq!(operations(&doc).len(), 74);
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

    // ------------------------------------------------------------------ Hotfix 065

    fn required(doc: &Value, name: &str) -> BTreeSet<String> {
        schema(doc, name)["required"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    }

    fn enum_of(doc: &Value, name: &str) -> Value {
        schema(doc, name)["enum"].clone()
    }

    fn query_param(doc: &Value, method: &str, path: &str, name: &str) -> Value {
        operation(doc, method, path)["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("{path} has no {name}"))
            .clone()
    }

    /// The stage/job and round DTOs exactly as frozen by contract version 2.
    const UNCHANGED_V2: [(&str, &str); 7] = [
        (
            "StageName",
            r#"{"description":"Compatibility stage names of `POST /api/stages/{stage}/run`.","enum":["intake","lint","extract","consolidate","calc","rules","analyse","flows","scenarios","tags","run"],"type":"string"}"#,
        ),
        (
            "StageRunRequest",
            r#"{"additionalProperties":false,"properties":{"scope":{"items":{"type":"string"},"type":["array","null"]}},"type":"object"}"#,
        ),
        (
            "StageRunResponse",
            r##"{"additionalProperties":false,"properties":{"events":{"items":{"$ref":"#/components/schemas/DesignEvent"},"type":"array"},"job_id":{"type":"string"},"revision":{"type":"string"},"semantic_hash":{"type":"string"},"version":{"type":"string"}},"required":["version","revision","semantic_hash","events","job_id"],"type":"object"}"##,
        ),
        (
            "JobStatus",
            r#"{"description":"Stage job status (X1.5); `stale` is a status, not a flag.","enum":["queued","acquiring","evaluating","ready","stale","failed","committed"],"type":"string"}"#,
        ),
        (
            "JobResponse",
            r##"{"additionalProperties":false,"properties":{"base_revision":{"description":"The branch revision the job captured.","type":"string"},"error":{"type":["string","null"]},"progress":{"format":"double","maximum":1,"minimum":0,"type":"number"},"result_version":{"type":["string","null"]},"status":{"$ref":"#/components/schemas/JobStatus"}},"required":["status","progress","base_revision"],"type":"object"}"##,
        ),
        (
            "CreateRoundRequest",
            r#"{"additionalProperties":false,"properties":{"max":{"format":"int64","minimum":0,"type":"integer"},"stakeholder":{"type":"string"}},"required":["stakeholder","max"],"type":"object"}"#,
        ),
        (
            "Round",
            r#"{"additionalProperties":false,"properties":{"id":{"type":"string"},"questions":{"items":{"type":"string"},"type":"array"},"stakeholder":{"type":"string"}},"required":["id","stakeholder","questions"],"type":"object"}"#,
        ),
    ];

    #[test]
    fn openapi_hotfix_065_contract_v3() {
        let doc = doc();
        assert_eq!(doc["x-plumb-contract-version"], json!(3));
        // Inventory and examples.
        let ops = operations(&doc);
        assert_eq!(
            (
                ops.len(),
                ops.iter().filter(|o| o.0 == "POST").count(),
                ops.iter().filter(|o| o.0 == "GET").count()
            ),
            (74, 39, 35)
        );
        let (_, counts) = example_locations(&doc);
        assert_eq!(
            (
                counts.parameters,
                counts.request_bodies,
                counts.responses,
                counts.headers,
                counts.websocket
            ),
            // Hotfix 073 adds two path IDs, two If-Match headers, two request bodies and two
            // 200 plus two 412 responses.
            (139, 34, 126, 34, 1)
        );
        let schemas = doc["components"]["schemas"].as_object().unwrap();
        // Hotfix 073 adds SemanticElementDiff and SemanticChangePreviewResponse; Hotfix 079 adds
        // ProcessNodeView, ProcessNodeViewKind and ProcessFlowView; Hotfix 080 adds
        // FindingRuleClass; Hotfix 082 adds GateRuleState, GateRuleApplicabilityState,
        // GateRuleApplicability, GateRuleError, GateRuleResult, GateWaiver and GateSummaryView;
        // H0.11 adds GateEvaluationState and GateStateReason.
        assert_eq!(schemas.len(), 130);
        for added in [
            "CalculationValue",
            "CreateRoundResponse",
            "ProposalKind",
            "QuestionKind",
            "QuestionStatus",
            "RuleViewKind",
            "ScenarioConfirmationStatus",
            "ScenarioStatus",
            "SearchScope",
            "SentenceSlotPatch",
            "SentenceSlots",
            "SessionStage",
        ] {
            assert!(schemas.contains_key(added), "{added}");
        }
        for removed in [
            "AnswerByText",
            "AnswerByValue",
            "ProcessStepOp",
            "QuestionContext",
        ] {
            assert!(!schemas.contains_key(removed), "{removed}");
        }
        for (name, frozen) in UNCHANGED_V2 {
            let frozen: Value = serde_json::from_str(frozen).unwrap();
            assert_eq!(schema(&doc, name), frozen, "{name}");
        }

        // Finding (Hotfix 080): rule_class is required, nullable and a closed FindingRuleClass;
        // family stays the gate/family string.
        let finding = schema(&doc, "Finding");
        assert_eq!(
            finding["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            set(&[
                "id",
                "code",
                "family",
                "rule_class",
                "severity",
                "status",
                "message",
                "affected_refs",
                "waiver_ref"
            ])
        );
        assert!(required(&doc, "Finding").contains("rule_class"));
        assert_eq!(finding["properties"]["family"]["type"], json!("string"));
        let rule_class = finding["properties"]["rule_class"]["oneOf"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(rule_class.len(), 2);
        assert!(rule_class.contains(&json!({"type": "null"})));
        assert_eq!(
            rule_class
                .iter()
                .filter_map(|branch| branch.get("$ref"))
                .collect::<Vec<_>>(),
            vec!["#/components/schemas/FindingRuleClass"]
        );
        assert_eq!(
            enum_of(&doc, "FindingRuleClass"),
            json!([
                "external_standard",
                "external_interop",
                "plumb_core",
                "profile_policy",
                "organization_policy"
            ])
        );

        // GateStatus (Hotfix 082): the additive projection of current GateReport material.
        let keys = |name: &str| {
            schema(&doc, name)["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
        };
        // H0.11: every NOW gate carries its evaluation state and, when not current, the reason.
        let gate_fields = [
            "gate", "state", "reason", "passed", "findings", "rules", "waivers", "summary",
        ];
        assert_eq!(keys("GateStatus"), set(&gate_fields));
        assert_eq!(required(&doc, "GateStatus"), set(&gate_fields));
        assert_eq!(
            enum_of(&doc, "GateEvaluationState"),
            json!(["not_evaluated", "current", "stale"])
        );
        assert_eq!(
            enum_of(&doc, "GateRuleState"),
            json!(["PASS", "FAIL", "WARN", "NOT_APPLICABLE", "WAIVED", "ERROR"])
        );
        assert_eq!(
            enum_of(&doc, "GateRuleApplicabilityState"),
            json!(["APPLICABLE", "NOT_APPLICABLE"])
        );
        assert_eq!(keys("GateRuleApplicability"), set(&["state", "reason"]));
        assert_eq!(
            required(&doc, "GateRuleApplicability"),
            set(&["state", "reason"])
        );
        assert_eq!(
            schema(&doc, "GateRuleApplicability")["properties"]["reason"]["type"],
            json!(["string", "null"])
        );
        let rule_fields = [
            "rule_id",
            "state",
            "declared_severity",
            "effective_severity",
            "applicability",
            "targets",
            "evidence",
            "semantic_condition_key",
            "finding_ref",
            "waiver_ref",
            "error",
        ];
        assert_eq!(keys("GateRuleResult"), set(&rule_fields));
        assert_eq!(required(&doc, "GateRuleResult"), set(&rule_fields));
        let rule = schema(&doc, "GateRuleResult");
        for nullable in ["semantic_condition_key", "finding_ref", "waiver_ref"] {
            assert_eq!(
                rule["properties"][nullable]["type"],
                json!(["string", "null"]),
                "{nullable}"
            );
        }
        let error = rule["properties"]["error"]["oneOf"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(error.len(), 2);
        assert!(error.contains(&json!({"type": "null"})));
        assert_eq!(
            error
                .iter()
                .filter_map(|branch| branch.get("$ref"))
                .collect::<Vec<_>>(),
            vec!["#/components/schemas/GateRuleError"]
        );
        let error_fields = ["code", "message", "targets", "evidence"];
        assert_eq!(keys("GateRuleError"), set(&error_fields));
        assert_eq!(required(&doc, "GateRuleError"), set(&error_fields));
        let waiver_fields = ["rule_id", "finding_ref", "finding_key", "decision_ref"];
        assert_eq!(keys("GateWaiver"), set(&waiver_fields));
        assert_eq!(required(&doc, "GateWaiver"), set(&waiver_fields));
        let summary_fields = ["blocker_failed", "blocker_waived", "warnings"];
        assert_eq!(keys("GateSummaryView"), set(&summary_fields));
        assert_eq!(required(&doc, "GateSummaryView"), set(&summary_fields));

        // Search.
        let hit = schema(&doc, "SearchHit");
        assert_eq!(hit["properties"]["path"]["type"], json!(["string", "null"]));
        assert_eq!(
            required(&doc, "SearchHit"),
            set(&["id", "kind", "path", "score", "snippet"])
        );
        assert_eq!(
            enum_of(&doc, "SearchScope"),
            json!([
                "requirement",
                "concept",
                "rule",
                "calculation",
                "scenario",
                "evidence"
            ])
        );
        assert_eq!(
            query_param(&doc, "GET", "/api/search", "scope")["schema"],
            component_ref("SearchScope")
        );

        // Required reasons.
        for (name, fields) in [
            ("SplitRequest", &["parts", "reason"][..]),
            ("MarkDuplicateRequest", &["of", "reason"][..]),
            ("GlossaryMergeRequest", &["into", "reason"][..]),
            ("RuleCellRequest", &["cell", "outcome", "reason"][..]),
        ] {
            assert_eq!(required(&doc, name), set(fields), "{name}");
            assert_eq!(properties(&doc, name), set(fields), "{name}");
            assert_eq!(schema(&doc, name)["properties"]["reason"]["type"], "string");
        }
        let resolve = operation(&doc, "POST", "/api/assumptions/{id}/resolve");
        assert_eq!(resolve["requestBody"]["required"], true);
        assert_eq!(
            resolve["requestBody"]["content"]["application/json"]["schema"],
            component_ref("ReasonRequest")
        );

        // Sentences.
        assert_eq!(
            schema(&doc, "Sentence")["properties"]["slots"],
            component_ref("SentenceSlots")
        );
        assert_eq!(
            required(&doc, "SentenceSlots"),
            set(&["class", "level", "modality"])
        );
        assert_eq!(
            schema(&doc, "SentenceEditRequest")["properties"]["slots"],
            component_ref("SentenceSlotPatch")
        );
        assert_eq!(
            properties(&doc, "SentenceSlotPatch"),
            set(&["class", "level", "modality"])
        );
        assert!(required(&doc, "SentenceSlotPatch").is_empty());
        assert_eq!(
            schema(&doc, "SentenceSlotPatch")["additionalProperties"],
            false
        );

        // CRUD.
        let crud = schema(&doc, "CrudCell");
        assert_eq!(
            required(&doc, "CrudCell"),
            set(&["op", "ent", "c", "r", "u", "d"])
        );
        for nullable in ["c", "u", "d"] {
            assert_eq!(
                crud["properties"][nullable]["type"],
                json!(["boolean", "null"])
            );
        }
        assert_eq!(crud["properties"]["r"]["type"], "boolean");

        // Calculation values: exact closed non-recursive variants.
        assert_eq!(
            schema(&doc, "CalculationExample")["properties"]["value"],
            component_ref("CalculationValue")
        );
        let variants: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> =
            schema(&doc, "CalculationValue")["oneOf"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| {
                    assert_eq!(b["additionalProperties"], false);
                    assert!(refs_of(b).is_empty(), "recursive {b}");
                    let names = |v: &Value| -> BTreeSet<String> {
                        v.as_object()
                            .map(|o| o.keys().cloned().collect())
                            .or_else(|| {
                                v.as_array().map(|a| {
                                    a.iter().map(|x| x.as_str().unwrap().to_owned()).collect()
                                })
                            })
                            .unwrap()
                    };
                    (
                        b["properties"]["type"]["enum"][0]
                            .as_str()
                            .unwrap()
                            .to_owned(),
                        (names(&b["properties"]), names(&b["required"])),
                    )
                })
                .collect();
        let both = |fields: &[&str]| (set(fields), set(fields));
        assert_eq!(
            variants,
            BTreeMap::from([
                ("integer".to_owned(), both(&["type", "value"])),
                ("decimal".to_owned(), both(&["type", "value"])),
                ("boolean".to_owned(), both(&["type", "value"])),
                ("string".to_owned(), both(&["type", "value"])),
                ("date".to_owned(), both(&["type", "value"])),
                ("date_time".to_owned(), both(&["type", "value"])),
                ("duration".to_owned(), both(&["type", "value", "unit"])),
                ("quantity".to_owned(), both(&["type", "value", "unit"])),
                ("enum".to_owned(), both(&["type", "enum_ref", "member"])),
            ])
        );
        for (example, ok) in [
            (json!({"type": "decimal", "value": "0.5"}), true),
            (
                json!({"type": "quantity", "value": "2", "unit": "day"}),
                true,
            ),
            (
                json!({"type": "enum", "enum_ref": "enum:x", "member": "a"}),
                true,
            ),
            (json!({"type": "decimal", "value": 0.5}), false),
            (json!({"type": "integer", "value": 1, "unit": "x"}), false),
            (json!({"type": "list", "value": []}), false),
        ] {
            assert_eq!(
                validate_example(&doc, &component_ref("CalculationValue"), &example, "v").is_ok(),
                ok,
                "{example}"
            );
        }

        // Rules.
        assert_eq!(
            enum_of(&doc, "RuleViewKind"),
            json!(["rule", "decision_table"])
        );
        let rule = schema(&doc, "Rule");
        assert_eq!(rule["properties"]["kind"], component_ref("RuleViewKind"));
        assert_eq!(
            required(&doc, "Rule"),
            set(&[
                "id",
                "name",
                "kind",
                "conditions",
                "rows",
                "uncovered_cells",
                "overlaps"
            ])
        );
        for nullable in ["rows", "uncovered_cells", "overlaps"] {
            assert_eq!(
                rule["properties"][nullable]["type"],
                json!(["array", "null"])
            );
        }

        // Process steps: exactly add, move, remove and (Hotfix 079) move_lane; reason optional.
        let steps: BTreeMap<String, BTreeSet<String>> = schema(&doc, "ProcessStepRequest")["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| {
                (
                    b["properties"]["op"]["enum"][0]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    b["required"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x.as_str().unwrap().to_owned())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            steps,
            BTreeMap::from([
                ("add".to_owned(), set(&["op", "operation_ref", "position"])),
                ("move".to_owned(), set(&["op", "node_ref", "position"])),
                ("remove".to_owned(), set(&["op", "node_ref"])),
                (
                    "move_lane".to_owned(),
                    set(&["op", "node_ref", "performer_ref"])
                ),
            ])
        );
        assert!(validate_example(
            &doc,
            &component_ref("ProcessStepRequest"),
            &json!({"op": "remove", "node_ref": "process_node:1", "position": 0}),
            "s"
        )
        .is_err());

        // Proposals.
        assert_eq!(
            enum_of(&doc, "ProposalKind"),
            json!(["link", "merge", "supersede", "create", "accept"])
        );
        assert_eq!(
            schema(&doc, "IntakeRequest")["properties"]["kind"],
            component_ref("ProposalKind")
        );
        assert_eq!(
            required(&doc, "IntakeRequest"),
            set(&["kind", "payload", "evidence"])
        );
        assert_eq!(required(&doc, "AnswerRequest"), set(&["answer"]));
        assert_eq!(
            properties(&doc, "AnswerRequest"),
            set(&["answer", "rationale", "supersedes"])
        );

        // Sessions.
        let stages: Vec<String> = (0..=12).map(|i| format!("S{i}")).collect();
        assert_eq!(enum_of(&doc, "SessionStage"), json!(stages));
        assert_eq!(
            required(&doc, "SessionTurnRequest"),
            set(&["text", "stage"])
        );
        assert_eq!(
            schema(&doc, "SessionTurnRequest")["properties"]["stage"],
            component_ref("SessionStage")
        );

        // Questions: the persisted projection.
        let question = schema(&doc, "Question");
        let fields = [
            "id",
            "kind",
            "text",
            "status",
            "answer_schema",
            "stakeholder_ref",
            "priority",
            "round_ref",
            "context_refs",
        ];
        assert_eq!(properties(&doc, "Question"), set(&fields));
        assert_eq!(required(&doc, "Question"), set(&fields));
        assert_eq!(
            question["properties"]["kind"],
            component_ref("QuestionKind")
        );
        assert_eq!(
            question["properties"]["status"],
            component_ref("QuestionStatus")
        );
        assert_eq!(
            question["properties"]["answer_schema"]["type"],
            json!(["object", "null"])
        );
        for nullable in ["stakeholder_ref", "round_ref"] {
            assert_eq!(
                question["properties"][nullable]["type"],
                json!(["string", "null"])
            );
        }
        assert_eq!(
            question["properties"]["priority"]["type"],
            json!(["integer", "null"])
        );
        assert_eq!(question["properties"]["priority"]["minimum"], 0);
        assert_eq!(question["properties"]["context_refs"]["type"], "array");
        assert_eq!(
            enum_of(&doc, "QuestionKind"),
            json!([
                "YesNo",
                "PickOne",
                "PickMany",
                "Number",
                "Text",
                "Cardinality",
                "Unit",
                "Precision",
                "Rounding",
                "FormulaConfirm",
                "RuleCell",
                "Calendar",
                "RoleAssignment",
                "Permission",
                "QualityThreshold",
                "ArchitectureChoice",
                "TechnologyChoice",
                "InterfaceChoice",
                "VerificationMethod"
            ])
        );
        assert_eq!(
            enum_of(&doc, "QuestionStatus"),
            json!(["Open", "Answered", "Closed", "Superseded"])
        );
        assert_eq!(
            query_param(&doc, "GET", "/api/questions", "status")["schema"],
            component_ref("QuestionStatus")
        );

        // Rounds.
        let round = operation(&doc, "POST", "/api/rounds");
        assert_eq!(
            round["responses"]["200"]["content"]["application/json"]["schema"],
            component_ref("CreateRoundResponse")
        );
        let round_fields = ["version", "revision", "semantic_hash", "events", "round"];
        assert_eq!(properties(&doc, "CreateRoundResponse"), set(&round_fields));
        assert_eq!(required(&doc, "CreateRoundResponse"), set(&round_fields));
        assert_eq!(
            schema(&doc, "CreateRoundResponse")["properties"]["round"],
            component_ref("Round")
        );

        // Scenarios.
        let scenario = schema(&doc, "Scenario");
        assert_eq!(
            scenario["properties"]["status"],
            component_ref("ScenarioStatus")
        );
        assert_eq!(
            scenario["properties"]["confirmation_status"],
            component_ref("ScenarioConfirmationStatus")
        );
        assert_eq!(scenario["properties"]["derived_from"]["type"], "array");
        assert!(required(&doc, "Scenario").contains("confirmation_status"));
        assert!(required(&doc, "Scenario").contains("derived_from"));
        assert!(required(&doc, "Scenario").contains("run"));
        assert_eq!(
            enum_of(&doc, "ScenarioStatus"),
            json!([
                "Proposed",
                "Accepted",
                "Rejected",
                "Superseded",
                "Deprecated",
                "Suspect"
            ])
        );
        assert_eq!(
            enum_of(&doc, "ScenarioConfirmationStatus"),
            json!(["Derived", "Confirmed"])
        );
        assert_eq!(
            query_param(&doc, "GET", "/api/scenarios", "status")["schema"],
            component_ref("ScenarioStatus")
        );
    }

    // ------------------------------------------------------------------ Hotfix 073

    #[test]
    fn openapi_hotfix_073_semantic_change_previews() {
        let doc = doc();
        assert_eq!(doc["x-plumb-contract-version"], json!(3));
        // Operations 73 and 74 are appended; 01-72 keep their frozen order.
        let tail: Vec<_> = OPERATIONS[72..]
            .iter()
            .map(|s| {
                (
                    s.number,
                    s.method,
                    s.path,
                    s.operation_id,
                    s.tag,
                    s.pageable,
                    s.owner,
                )
            })
            .collect();
        assert_eq!(
            tail,
            vec![
                (
                    73,
                    "POST",
                    "/api/requirements/{id}/split/preview",
                    "preview_requirement_split",
                    "requirements",
                    false,
                    "U2.1"
                ),
                (
                    74,
                    "POST",
                    "/api/glossary/{id}/merge/preview",
                    "preview_glossary_merge",
                    "glossary",
                    false,
                    "U2.1"
                ),
            ]
        );
        for (path, request, tag) in [
            (
                "/api/requirements/{id}/split/preview",
                "SplitRequest",
                "requirements",
            ),
            (
                "/api/glossary/{id}/merge/preview",
                "GlossaryMergeRequest",
                "glossary",
            ),
        ] {
            let op = operation(&doc, "POST", path);
            assert_eq!(op["tags"], json!([tag]), "{path}");
            assert_eq!(
                op["requestBody"]["content"]["application/json"]["schema"],
                component_ref(request),
                "{path}"
            );
            assert_eq!(op["requestBody"]["required"], json!(true), "{path}");
            assert_eq!(
                op["responses"]["200"]["content"]["application/json"]["schema"],
                component_ref("SemanticChangePreviewResponse"),
                "{path}"
            );
            assert_eq!(
                op["responses"]["412"]["content"]["application/json"]["schema"],
                component_ref("PreconditionFailedResponse"),
                "{path}"
            );
            let params = op["parameters"].as_array().unwrap();
            let names: Vec<(&str, &str, bool)> = params
                .iter()
                .map(|p| {
                    (
                        p["name"].as_str().unwrap(),
                        p["in"].as_str().unwrap(),
                        p["required"].as_bool().unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                names,
                vec![("id", "path", true), ("If-Match", "header", true)],
                "{path}"
            );
            assert_eq!(
                op["responses"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>(),
                vec!["200", "412"],
                "{path}"
            );
            assert!(op["responses"]["200"].get("headers").is_none(), "{path}");
        }
        let diff = schema(&doc, "SemanticElementDiff");
        assert_eq!(diff["type"], "object");
        assert_eq!(diff["additionalProperties"], json!(false));
        assert_eq!(
            required(&doc, "SemanticElementDiff"),
            set(&[
                "base_semantic_hash",
                "candidate_semantic_hash",
                "added",
                "removed",
                "modified"
            ])
        );
        assert_eq!(
            properties(&doc, "SemanticElementDiff"),
            required(&doc, "SemanticElementDiff")
        );
        for field in ["base_semantic_hash", "candidate_semantic_hash"] {
            assert_eq!(
                diff["properties"][field],
                json!({"type": "string"}),
                "{field}"
            );
        }
        for field in ["added", "removed", "modified"] {
            assert_eq!(
                diff["properties"][field],
                json!({"type": "array", "items": {"type": "string"}}),
                "{field}"
            );
        }
        let preview = schema(&doc, "SemanticChangePreviewResponse");
        assert_eq!(preview["additionalProperties"], json!(false));
        let fields = set(&[
            "version",
            "revision",
            "semantic_hash",
            "events",
            "proposal_id",
            "action",
            "intake",
            "diff",
        ]);
        assert_eq!(required(&doc, "SemanticChangePreviewResponse"), fields);
        assert_eq!(properties(&doc, "SemanticChangePreviewResponse"), fields);
        assert_eq!(
            preview["properties"]["action"],
            component_ref("ProposalKind")
        );
        assert_eq!(
            preview["properties"]["intake"],
            component_ref("IntakeReportView")
        );
        assert_eq!(
            preview["properties"]["diff"],
            component_ref("SemanticElementDiff")
        );
        assert_eq!(
            preview["properties"]["events"],
            json!({"type": "array", "items": component_ref("DesignEvent")})
        );
        // No final-hash field: the candidate hash is not a revision identity.
        assert!(preview["properties"].get("result_semantic_hash").is_none());
        assert!(diff["properties"].get("result_semantic_hash").is_none());
        // The direct compatibility operations are unchanged write results.
        for path in ["/api/requirements/{id}/split", "/api/glossary/{id}/merge"] {
            assert_eq!(
                operation(&doc, "POST", path)["responses"]["200"]["content"]["application/json"]
                    ["schema"],
                component_ref("WriteResult"),
                "{path}"
            );
        }
    }

    // ------------------------------------------------------------------ Hotfix 079

    #[test]
    fn openapi_hotfix_079_process_projection_and_lane_move() {
        let doc = doc();
        assert_eq!(operations(&doc).len(), 74);
        assert_eq!(doc["x-plumb-contract-version"], json!(3));
        // Process keeps its summary fields and gains the structured projection.
        let process_fields = set(&[
            "id",
            "name",
            "lanes",
            "steps",
            "branches",
            "validation",
            "nodes",
            "flows",
        ]);
        assert_eq!(properties(&doc, "Process"), process_fields);
        assert_eq!(required(&doc, "Process"), process_fields);
        let process = schema(&doc, "Process");
        assert_eq!(
            process["properties"]["nodes"]["items"],
            component_ref("ProcessNodeView")
        );
        assert_eq!(
            process["properties"]["flows"]["items"],
            component_ref("ProcessFlowView")
        );
        let steps = process["properties"]["steps"]["description"]
            .as_str()
            .unwrap();
        assert!(steps.contains("Sorted ProcessNode IDs"), "{steps}");
        assert!(steps.contains("not semantic execution order"), "{steps}");
        assert!(!steps.contains("operation names"), "{steps}");
        // ProcessNodeView: nine required properties, four of them nullable.
        let node = schema(&doc, "ProcessNodeView");
        let node_fields = set(&[
            "id",
            "kind",
            "operation_ref",
            "performer_refs",
            "consumes_refs",
            "produces_refs",
            "condition_expr",
            "message_ref",
            "timer_expr",
        ]);
        assert_eq!(node["additionalProperties"], json!(false));
        assert_eq!(properties(&doc, "ProcessNodeView"), node_fields);
        assert_eq!(required(&doc, "ProcessNodeView"), node_fields);
        for nullable in [
            "operation_ref",
            "condition_expr",
            "message_ref",
            "timer_expr",
        ] {
            assert_eq!(
                node["properties"][nullable]["type"],
                json!(["string", "null"]),
                "{nullable}"
            );
        }
        for list in ["performer_refs", "consumes_refs", "produces_refs"] {
            assert_eq!(node["properties"][list]["type"], json!("array"), "{list}");
        }
        assert_eq!(
            node["properties"]["kind"]["$ref"],
            component_ref("ProcessNodeViewKind")["$ref"]
        );
        assert_eq!(
            enum_of(&doc, "ProcessNodeViewKind"),
            json!([
                "start",
                "end",
                "human_task",
                "service_task",
                "exclusive_gateway",
                "parallel_split",
                "parallel_join",
                "message_event",
                "timer_event",
                "error_event",
                "subprocess"
            ])
        );
        // ProcessFlowView: exactly from and to.
        assert_eq!(
            schema(&doc, "ProcessFlowView")["additionalProperties"],
            json!(false)
        );
        assert_eq!(properties(&doc, "ProcessFlowView"), set(&["from", "to"]));
        assert_eq!(required(&doc, "ProcessFlowView"), set(&["from", "to"]));
        // Every step request variant takes an optional, nullable reason.
        for branch in schema(&doc, "ProcessStepRequest")["oneOf"]
            .as_array()
            .unwrap()
        {
            assert_eq!(
                branch["properties"]["reason"]["type"],
                json!(["string", "null"]),
                "{branch}"
            );
            assert!(!branch["required"]
                .as_array()
                .unwrap()
                .contains(&json!("reason")));
        }
        assert!(validate_example(
            &doc,
            &component_ref("ProcessStepRequest"),
            &json!({"op": "move_lane", "node_ref": "process_node:1", "performer_ref": "actor:1", "reason": null}),
            "move_lane"
        )
        .is_ok());
        assert!(validate_example(
            &doc,
            &component_ref("ProcessStepRequest"),
            &json!({"op": "movelane", "node_ref": "process_node:1", "performer_ref": "actor:1"}),
            "movelane"
        )
        .is_err());
        assert!(!doc.to_string().contains("movelane"));
        // No new process operation or interchange format.
        assert_eq!(schema(&doc, "ProcessFormat")["enum"], json!(["mermaid"]));
        let process_ops: BTreeSet<String> = OPERATIONS
            .iter()
            .filter(|s| s.path.starts_with("/api/processes"))
            .map(|s| s.operation_id.to_owned())
            .collect();
        assert_eq!(
            process_ops,
            set(&[
                "list_processes",
                "get_process",
                "mutate_process_steps",
                "import_process",
                "export_process"
            ])
        );
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
