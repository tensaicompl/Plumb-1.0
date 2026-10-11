//! X1.1 provider contract tests (Hotfix 057).
//!
//! Feature-independent tests cover the generic request material and the Null/Mock regressions.
//! Under the `anthropic` feature, every Anthropic test talks to a local `TcpListener` on
//! 127.0.0.1 (never api.anthropic.com); one ignored live smoke needs ANTHROPIC_API_KEY and
//! ANTHROPIC_MODEL and runs only through scripts/manual-llm-smoke.sh.

mod provider_contract {
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId};
    use plumb_inference::{
        InferenceArtifact, InferenceProvider, InferenceRequest, InferenceRequestMaterial,
        InferenceRequestMaterialSet, MockProvider, NullProvider, ProviderError, ProviderExecution,
        ProviderPolicy,
    };
    use serde_json::{json, Value};

    const PROMPT: &str = "Answer with the JSON object described by output_schema.\n";
    const SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "required": ["answer"],
  "properties": {"answer": {"type": "string", "pattern": "^(yes|no)$"}}
}
"#;

    pub(crate) fn context_bytes() -> Vec<u8> {
        to_canonical_json(&json!({"question": "Is the leave approved?", "version": 1})).unwrap()
    }

    pub(crate) fn policy(provider: &str, config: Value) -> ProviderPolicy {
        ProviderPolicy {
            provider: provider.to_owned(),
            config: CanonicalJson::new(config),
        }
    }

    pub(crate) fn anthropic_config() -> Value {
        json!({"adapter_version": 1, "model": "test-model-configured", "max_tokens": 256, "api_version": "2023-06-01"})
    }

    /// A request whose hashes bind the given material bytes.
    pub(crate) fn request_for(
        policy: ProviderPolicy,
        context: &[u8],
        prompt: &[u8],
        schema: &[u8],
    ) -> InferenceRequest {
        InferenceRequest::new(
            StageId::S4,
            "scenario_values".to_owned(),
            vec!["scenario:0000000000000001".parse::<Id>().unwrap()],
            vec![],
            Hash::content_sha256(context),
            Hash::content_sha256(prompt),
            Hash::content_sha256(schema),
            policy,
        )
        .unwrap()
    }

    pub(crate) fn material_for(
        request: &InferenceRequest,
        context: &[u8],
        prompt: &[u8],
        schema: &[u8],
    ) -> InferenceRequestMaterial {
        InferenceRequestMaterial {
            request_hash: request.id.clone(),
            context_json: context.to_vec(),
            prompt_template: prompt.to_vec(),
            schema_json: schema.to_vec(),
        }
    }

    pub(crate) fn standard(provider: &str) -> (InferenceRequest, InferenceRequestMaterial) {
        let context = context_bytes();
        let request = request_for(
            policy(provider, anthropic_config()),
            &context,
            PROMPT.as_bytes(),
            SCHEMA.as_bytes(),
        );
        let material = material_for(&request, &context, PROMPT.as_bytes(), SCHEMA.as_bytes());
        (request, material)
    }

    // ------------------------------------------------------------------ material

    #[test]
    fn material_validates_against_its_request() {
        let (request, material) = standard("anthropic");
        material.validate_for(&request).unwrap();
    }

    #[test]
    fn material_corruption_is_rejected() {
        let (request, material) = standard("anthropic");
        let mut other = material.clone();
        other.request_hash = Hash::content_sha256(b"another request");
        assert!(matches!(
            other.validate_for(&request),
            Err(ProviderError::MaterialRequestMismatch { .. })
        ));
        for (field, mutate) in [
            ("context_json", 0usize),
            ("prompt_template", 1),
            ("schema_json", 2),
        ] {
            let mut m = material.clone();
            match mutate {
                0 => m.context_json.push(b' '),
                1 => m.prompt_template.push(b' '),
                _ => m.schema_json.push(b' '),
            }
            assert!(
                matches!(m.validate_for(&request), Err(ProviderError::MaterialHashMismatch { field: f }) if f == field),
                "{field}"
            );
        }
    }

    #[test]
    fn noncanonical_context_bad_prompt_and_bad_schema_are_rejected() {
        let noncanonical = br#"{"version": 1, "question": "Is the leave approved?"}"#.to_vec();
        let request = request_for(
            policy("anthropic", anthropic_config()),
            &noncanonical,
            PROMPT.as_bytes(),
            SCHEMA.as_bytes(),
        );
        let m = material_for(
            &request,
            &noncanonical,
            PROMPT.as_bytes(),
            SCHEMA.as_bytes(),
        );
        assert!(matches!(
            m.validate_for(&request),
            Err(ProviderError::NonCanonicalContext)
        ));

        let context = context_bytes();
        let bad_prompt = vec![0xff, 0xfe, b'a'];
        let request = request_for(
            policy("anthropic", anthropic_config()),
            &context,
            &bad_prompt,
            SCHEMA.as_bytes(),
        );
        let m = material_for(&request, &context, &bad_prompt, SCHEMA.as_bytes());
        assert!(matches!(
            m.validate_for(&request),
            Err(ProviderError::InvalidPromptEncoding)
        ));

        for schema in [
            &b"{not json"[..],
            br#"{"$schema": "https://json-schema.org/draft/2020-12/schema", "type": 12}"#,
        ] {
            let request = request_for(
                policy("anthropic", anthropic_config()),
                &context,
                PROMPT.as_bytes(),
                schema,
            );
            let m = material_for(&request, &context, PROMPT.as_bytes(), schema);
            assert!(matches!(
                m.validate_for(&request),
                Err(ProviderError::InvalidSchema { .. })
            ));
        }
    }

    #[test]
    fn material_set_is_order_independent_and_rejects_duplicates() {
        let (a_request, a) = standard("anthropic");
        let (b_request, b) = standard("mock");
        let forward = InferenceRequestMaterialSet::from_materials([a.clone(), b.clone()]).unwrap();
        let reverse = InferenceRequestMaterialSet::from_materials([b.clone(), a.clone()]).unwrap();
        assert_eq!(forward, reverse);
        assert_eq!(forward.get(&a_request.id), Some(&a));
        assert_eq!(reverse.get(&b_request.id), Some(&b));
        assert!(forward.get(&Hash::content_sha256(b"none")).is_none());
        assert!(matches!(
            InferenceRequestMaterialSet::from_materials([a.clone(), a.clone()]),
            Err(ProviderError::DuplicateMaterial(h)) if h == a_request.id
        ));
    }

    // ------------------------------------------------------------------ null and mock

    fn mock_execution(request: &InferenceRequest) -> ProviderExecution {
        let raw = br#"{"answer":"yes"}"#.to_vec();
        let output = CanonicalJson::new(json!({"answer": "yes"}));
        ProviderExecution {
            artifact: InferenceArtifact {
                request_hash: request.id.clone(),
                provider: request.provider_policy.provider.clone(),
                model: "mock-model".to_owned(),
                parameters: request.provider_policy.config.clone(),
                raw_response_hash: Hash::content_sha256(&raw),
                validated_output_hash: output.content_hash().unwrap(),
                validated_output: output,
            },
            raw_response_media_type: "application/json".to_owned(),
            raw_response: raw,
        }
    }

    #[test]
    fn null_and_mock_providers_keep_their_contracts() {
        let (request, _) = standard("mock");
        assert!(matches!(
            NullProvider.execute(&request),
            Err(ProviderError::ProviderDisabled)
        ));
        let execution = mock_execution(&request);
        let mock = MockProvider::with_fixtures([(request.id.clone(), execution.clone())]).unwrap();
        assert_eq!(mock.execute(&request).unwrap(), execution);
        let (other, _) = standard("anthropic");
        assert!(matches!(
            MockProvider::new().execute(&other),
            Err(ProviderError::FixtureNotFound(_))
        ));
    }

    // ------------------------------------------------------------------ anthropic (local)

    #[cfg(feature = "anthropic")]
    mod anthropic {
        use std::collections::BTreeMap;
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};
        use std::thread;

        use plumb_core::{to_canonical_json, CanonicalJson, Hash};
        use plumb_inference::{
            AnthropicProvider, InferenceProvider, InferenceRequestMaterialSet, ProviderError,
        };
        use serde_json::{json, Value};

        use super::{
            anthropic_config, context_bytes, material_for, policy, request_for, standard, PROMPT,
            SCHEMA,
        };

        const KEY: &str = "test-api-key-not-a-real-secret";

        /// Whether an error is the expected one.
        type ErrorCheck = fn(&ProviderError) -> bool;

        #[derive(Debug, Clone)]
        struct Captured {
            request_line: String,
            headers: BTreeMap<String, String>,
            body: Vec<u8>,
        }

        struct Server {
            endpoint: String,
            connections: Arc<AtomicUsize>,
            captured: Arc<Mutex<Vec<Captured>>>,
        }

        /// A minimal local HTTP/1.1 responder answering every connection with one fixed reply.
        fn serve(status: &str, extra_headers: &[(&str, &str)], body: &[u8]) -> Server {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let connections = Arc::new(AtomicUsize::new(0));
            let captured = Arc::new(Mutex::new(Vec::new()));
            let (c, cap) = (connections.clone(), captured.clone());
            let mut reply = format!(
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n",
                body.len()
            )
            .into_bytes();
            for (k, v) in extra_headers {
                reply.extend(format!("{k}: {v}\r\n").bytes());
            }
            reply.extend(b"\r\n");
            reply.extend(body);
            thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    c.fetch_add(1, Ordering::SeqCst);
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    reader.read_line(&mut request_line).unwrap();
                    let mut headers = BTreeMap::new();
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        let line = line.trim_end_matches(['\r', '\n']);
                        if line.is_empty() {
                            break;
                        }
                        let (k, v) = line.split_once(':').unwrap();
                        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
                    }
                    let len: usize = headers
                        .get("content-length")
                        .map_or(0, |l| l.parse().unwrap());
                    let mut body = vec![0; len];
                    reader.read_exact(&mut body).unwrap();
                    cap.lock().unwrap().push(Captured {
                        request_line: request_line.trim_end().to_owned(),
                        headers,
                        body,
                    });
                    stream.write_all(&reply).unwrap();
                    stream.flush().unwrap();
                }
            });
            Server {
                endpoint: format!("http://127.0.0.1:{port}/v1/messages"),
                connections,
                captured,
            }
        }

        fn message(model: &str, stop_reason: &str, content: Value) -> Vec<u8> {
            serde_json::to_vec(&json!({
                "id": "msg_local_1",
                "type": "message",
                "role": "assistant",
                "model": model,
                "content": content,
                "stop_reason": stop_reason,
                "stop_sequence": null,
                "usage": {"input_tokens": 10, "output_tokens": 5},
            }))
            .unwrap()
        }

        fn text(t: &str) -> Value {
            json!({"type": "text", "text": t})
        }

        fn provider(
            server: &Server,
            material: super::InferenceRequestMaterial,
        ) -> AnthropicProvider {
            AnthropicProvider::with_endpoint(
                KEY,
                &server.endpoint,
                InferenceRequestMaterialSet::from_materials([material]).unwrap(),
            )
            .unwrap()
        }

        fn run_with(
            body: &[u8],
        ) -> (
            Result<plumb_inference::ProviderExecution, ProviderError>,
            Server,
        ) {
            let server = serve("200 OK", &[("content-type", "application/json")], body);
            let (request, material) = standard("anthropic");
            let result = provider(&server, material).execute(&request);
            (result, server)
        }

        #[test]
        fn successful_execution_builds_an_exact_artifact() {
            let body = message(
                "test-model-returned",
                "end_turn",
                json!([text(r#"{"answer":"yes"}"#)]),
            );
            let server = serve("200 OK", &[("content-type", "application/json")], &body);
            let (request, material) = standard("anthropic");
            let p = provider(&server, material);
            let execution = p.execute(&request).unwrap();
            execution.validate_for(&request).unwrap();
            assert_eq!(execution.raw_response, body);
            assert_eq!(execution.raw_response_media_type, "application/json");
            let a = &execution.artifact;
            assert_eq!(a.raw_response_hash, Hash::content_sha256(&body));
            assert_eq!(a.request_hash, request.id);
            assert_eq!(a.provider, "anthropic");
            assert_eq!(a.model, "test-model-returned");
            assert_eq!(a.parameters, request.provider_policy.config);
            assert_eq!(a.parameters.as_value()["model"], "test-model-configured");
            assert_eq!(a.validated_output.as_value(), &json!({"answer": "yes"}));
            assert_eq!(
                a.validated_output_hash,
                Hash::content_sha256(br#"{"answer":"yes"}"#)
            );
            assert_eq!(
                server.connections.load(std::sync::atomic::Ordering::SeqCst),
                1
            );

            // The exact outbound request.
            let sent = server.captured.lock().unwrap()[0].clone();
            assert_eq!(sent.request_line, "POST /v1/messages HTTP/1.1");
            assert_eq!(sent.headers["authorization"], format!("Bearer {KEY}"));
            assert_eq!(sent.headers["anthropic-version"], "2023-06-01");
            assert_eq!(sent.headers["content-type"], "application/json");
            assert!(!sent.headers.contains_key("anthropic-workspace-id"));
            let body: Value = serde_json::from_slice(&sent.body).unwrap();
            let schema: Value = serde_json::from_str(SCHEMA).unwrap();
            let context: Value = serde_json::from_slice(&context_bytes()).unwrap();
            let user = String::from_utf8(
                to_canonical_json(&json!({"context": context, "output_schema": schema})).unwrap(),
            )
            .unwrap();
            assert_eq!(
                body,
                json!({
                    "model": "test-model-configured",
                    "max_tokens": 256,
                    "system": PROMPT,
                    "messages": [{"role": "user", "content": user}],
                })
            );
            // The secret is confined to the Authorization header.
            let sent_text = String::from_utf8_lossy(&sent.body);
            assert!(!sent_text.contains(KEY));
            assert!(!format!("{p:?}").contains(KEY));
            assert!(!format!("{request:?}").contains(KEY));
            assert!(!format!("{execution:?}").contains(KEY));
            assert_eq!(sent.headers.values().filter(|v| v.contains(KEY)).count(), 1);
        }

        #[test]
        fn workspace_header_is_sent_only_when_configured() {
            let body = message("m", "end_turn", json!([text(r#"{"answer":"no"}"#)]));
            let server = serve("200 OK", &[], &body);
            let mut config = anthropic_config();
            config["workspace_id"] = json!("wrkspc_local");
            let context = context_bytes();
            let request = request_for(
                policy("anthropic", config),
                &context,
                PROMPT.as_bytes(),
                SCHEMA.as_bytes(),
            );
            let material = material_for(&request, &context, PROMPT.as_bytes(), SCHEMA.as_bytes());
            provider(&server, material).execute(&request).unwrap();
            assert_eq!(
                server.captured.lock().unwrap()[0].headers["anthropic-workspace-id"],
                "wrkspc_local"
            );
        }

        #[test]
        fn pre_network_failures_make_no_connection() {
            let body = message("m", "end_turn", json!([text(r#"{"answer":"yes"}"#)]));
            let server = serve("200 OK", &[], &body);
            let context = context_bytes();
            let attempt = |config: Value, provider_name: &str| {
                let request = request_for(
                    policy(provider_name, config),
                    &context,
                    PROMPT.as_bytes(),
                    SCHEMA.as_bytes(),
                );
                let material =
                    material_for(&request, &context, PROMPT.as_bytes(), SCHEMA.as_bytes());
                provider(&server, material).execute(&request)
            };
            assert!(matches!(
                attempt(anthropic_config(), "openai"),
                Err(ProviderError::UnsupportedProvider { .. })
            ));
            let mut bad_configs = Vec::new();
            for key in ["adapter_version", "model", "max_tokens", "api_version"] {
                let mut c = anthropic_config();
                c.as_object_mut().unwrap().remove(key);
                bad_configs.push(c);
            }
            for (key, value) in [
                ("temperature", json!(0.2)),
                ("adapter_version", json!(2)),
                ("max_tokens", json!(0)),
                ("max_tokens", json!(-1)),
                ("max_tokens", json!(1.5)),
                ("max_tokens", json!(131073)),
                ("model", json!("")),
                ("workspace_id", json!("")),
                ("workspace_id", json!("   ")),
                ("workspace_id", json!("bad\u{7}id")),
                ("workspace_id", Value::Null),
            ] {
                let mut c = anthropic_config();
                c[key] = value;
                bad_configs.push(c);
            }
            for c in bad_configs {
                assert!(
                    matches!(
                        attempt(c.clone(), "anthropic"),
                        Err(ProviderError::InvalidProviderConfig { .. })
                    ),
                    "{c}"
                );
            }
            // Missing and corrupted material.
            let (request, material) = standard("anthropic");
            let empty = AnthropicProvider::with_endpoint(
                KEY,
                &server.endpoint,
                InferenceRequestMaterialSet::new(),
            )
            .unwrap();
            assert!(matches!(
                empty.execute(&request),
                Err(ProviderError::MaterialNotFound(_))
            ));
            let mut corrupt = material.clone();
            corrupt.schema_json.push(b'\n');
            assert!(matches!(
                provider(&server, corrupt).execute(&request),
                Err(ProviderError::MaterialHashMismatch { .. })
            ));
            assert_eq!(
                server.connections.load(std::sync::atomic::Ordering::SeqCst),
                0
            );
        }

        #[test]
        fn secrets_and_endpoints_are_validated() {
            let set = InferenceRequestMaterialSet::new;
            for key in ["", " key", "key ", "ke\ny"] {
                let err = AnthropicProvider::new(key, set()).unwrap_err();
                assert!(matches!(err, ProviderError::InvalidSecret));
            }
            for endpoint in [
                "",
                "ftp://example.test/v1",
                "http://user:pass@127.0.0.1/v1",
                "not a url",
            ] {
                assert!(
                    matches!(
                        AnthropicProvider::with_endpoint(KEY, endpoint, set()),
                        Err(ProviderError::InvalidEndpoint { .. })
                    ),
                    "{endpoint}"
                );
            }
            let p = AnthropicProvider::new(KEY, set()).unwrap();
            assert!(format!("{p:?}").contains("api.anthropic.com/v1/messages"));
            assert!(!format!("{p:?}").contains(KEY));
        }

        #[test]
        fn output_must_be_strict_schema_valid_json() {
            let cases: Vec<(Vec<u8>, ErrorCheck)> = vec![
                (message("m", "end_turn", json!([text(r#"{"answer":"maybe"}"#)])), |e| matches!(e, ProviderError::SchemaInvalidOutput { errors } if !errors.is_empty())),
                (message("m", "end_turn", json!([text("not json")])), |e| matches!(e, ProviderError::InvalidOutputJson { .. })),
                (message("m", "end_turn", json!([text("```json\n{\"answer\":\"yes\"}\n```")])), |e| matches!(e, ProviderError::InvalidOutputJson { .. })),
                (message("m", "end_turn", json!([text(r#"Here: {"answer":"yes"}"#)])), |e| matches!(e, ProviderError::InvalidOutputJson { .. })),
                (message("m", "refusal", json!([text("I cannot help with that.")])), |e| matches!(e, ProviderError::Refusal)),
                (message("m", "max_tokens", json!([text(r#"{"answer":"y"#)])), |e| matches!(e, ProviderError::IncompleteOutput)),
                (message("m", "pause_turn", json!([text(r#"{"answer":"yes"}"#)])), |e| matches!(e, ProviderError::UnexpectedStopReason { stop_reason } if stop_reason == "pause_turn")),
                (message("m", "end_turn", json!([])), |e| matches!(e, ProviderError::MalformedResponse { .. })),
                (message("m", "end_turn", json!([text(r#"{"answer":"yes"}"#), text(r#"{"answer":"no"}"#)])), |e| matches!(e, ProviderError::MalformedResponse { .. })),
                (b"not json at all".to_vec(), |e| matches!(e, ProviderError::MalformedResponse { .. })),
                (serde_json::to_vec(&json!({"type": "message", "role": "user", "model": "m", "content": [], "stop_reason": "end_turn"})).unwrap(), |e| matches!(e, ProviderError::MalformedResponse { .. })),
            ];
            for (body, check) in cases {
                let (result, _) = run_with(&body);
                let err = result.unwrap_err();
                assert!(
                    check(&err),
                    "{err:?} for {}",
                    String::from_utf8_lossy(&body)
                );
                assert!(!format!("{err} {err:?}").contains(KEY));
            }
        }

        #[test]
        fn non_text_blocks_are_ignored_but_preserved() {
            let body = message(
                "m",
                "end_turn",
                json!([{"type": "thinking", "thinking": "..."}, text(r#"{"answer":"no"}"#), {"type": "server_tool_use", "id": "x"}]),
            );
            let (result, _) = run_with(&body);
            let execution = result.unwrap();
            assert_eq!(execution.raw_response, body);
            assert_eq!(
                execution.artifact.validated_output.as_value(),
                &json!({"answer": "no"})
            );
        }

        #[test]
        fn remote_statuses_are_typed_and_not_retried() {
            for status in [
                "400 Bad Request",
                "401 Unauthorized",
                "403 Forbidden",
                "404 Not Found",
                "429 Too Many Requests",
                "500 Internal Server Error",
                "529 Overloaded",
            ] {
                let code: u16 = status[..3].parse().unwrap();
                let body = serde_json::to_vec(&json!({
                    "type": "error",
                    "error": {"type": "overloaded_error", "message": "Overloaded"},
                    "request_id": "req_local_1",
                }))
                .unwrap();
                let server = serve(status, &[("request-id", "req_header_1")], &body);
                let (request, material) = standard("anthropic");
                let err = provider(&server, material).execute(&request).unwrap_err();
                match &err {
                    ProviderError::Remote {
                        status,
                        error_type,
                        message,
                        request_id,
                    } => {
                        assert_eq!(*status, code);
                        assert_eq!(error_type.as_deref(), Some("overloaded_error"));
                        assert_eq!(message.as_deref(), Some("Overloaded"));
                        assert_eq!(request_id.as_deref(), Some("req_local_1"));
                    }
                    other => panic!("{other:?}"),
                }
                assert!(!format!("{err} {err:?}").contains(KEY));
                assert_eq!(
                    server.connections.load(std::sync::atomic::Ordering::SeqCst),
                    1,
                    "{status}"
                );
            }
            // A plain-text error body keeps only the status (and the header request ID).
            let server = serve(
                "500 Internal Server Error",
                &[("request-id", "req_header_2")],
                b"upstream exploded",
            );
            let (request, material) = standard("anthropic");
            let err = provider(&server, material).execute(&request).unwrap_err();
            assert!(matches!(
                &err,
                ProviderError::Remote { status: 500, error_type: None, message: None, request_id: Some(r) } if r == "req_header_2"
            ));
            assert!(!format!("{err}").contains("upstream exploded"));
        }

        #[test]
        fn redirects_are_not_followed() {
            let server = serve(
                "302 Found",
                &[("location", "http://127.0.0.1:9/elsewhere")],
                b"",
            );
            let (request, material) = standard("anthropic");
            let err = provider(&server, material).execute(&request).unwrap_err();
            assert!(matches!(err, ProviderError::Remote { status: 302, .. }));
            assert_eq!(
                server.connections.load(std::sync::atomic::Ordering::SeqCst),
                1
            );
        }

        #[test]
        fn transport_failures_are_typed_without_secrets() {
            let port = {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                listener.local_addr().unwrap().port()
            };
            let (request, material) = standard("anthropic");
            let p = AnthropicProvider::with_endpoint(
                KEY,
                &format!("http://127.0.0.1:{port}/v1/messages"),
                InferenceRequestMaterialSet::from_materials([material]).unwrap(),
            )
            .unwrap();
            let err = p.execute(&request).unwrap_err();
            assert!(matches!(err, ProviderError::Transport { .. }));
            assert!(!format!("{err} {err:?}").contains(KEY));
        }

        // ------------------------------------------------------------------ live smoke

        const LIVE_PROMPT: &str =
            "Reply with exactly the JSON object {\"ok\":true} and nothing else.\n";
        const LIVE_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "required": ["ok"],
  "properties": {
    "ok": {
      "const": true
    }
  }
}
"#;

        #[test]
        #[ignore = "requires live Anthropic credentials"]
        fn live_anthropic_smoke() {
            let key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY is required");
            let model = std::env::var("ANTHROPIC_MODEL").expect("ANTHROPIC_MODEL is required");
            let mut config = json!({"adapter_version": 1, "model": model, "max_tokens": 128, "api_version": "2023-06-01"});
            if let Ok(workspace) = std::env::var("ANTHROPIC_WORKSPACE_ID") {
                config["workspace_id"] = json!(workspace);
            }
            let context = to_canonical_json(&json!({"smoke": true})).unwrap();
            let request = request_for(
                plumb_inference::ProviderPolicy {
                    provider: "anthropic".to_owned(),
                    config: CanonicalJson::new(config),
                },
                &context,
                LIVE_PROMPT.as_bytes(),
                LIVE_SCHEMA.as_bytes(),
            );
            let material = material_for(
                &request,
                &context,
                LIVE_PROMPT.as_bytes(),
                LIVE_SCHEMA.as_bytes(),
            );
            let p = AnthropicProvider::new(
                key,
                InferenceRequestMaterialSet::from_materials([material]).unwrap(),
            )
            .unwrap();
            let execution = p.execute(&request).unwrap();
            assert_eq!(
                execution.artifact.validated_output.as_value(),
                &json!({"ok": true})
            );
            execution.validate_for(&request).unwrap();
            let _ = Hash::content_sha256(b"");
        }
    }
}
