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

    /// The standard context and prompt with another output schema.
    #[cfg(feature = "anthropic")]
    pub(crate) fn with_schema(
        provider: &str,
        schema: &str,
    ) -> (InferenceRequest, InferenceRequestMaterial) {
        let context = context_bytes();
        let request = request_for(
            policy(provider, anthropic_config()),
            &context,
            PROMPT.as_bytes(),
            schema.as_bytes(),
        );
        let material = material_for(&request, &context, PROMPT.as_bytes(), schema.as_bytes());
        (request, material)
    }

    /// A second, distinct request and its own material (another context).
    pub(crate) fn second(provider: &str) -> (InferenceRequest, InferenceRequestMaterial) {
        let context =
            to_canonical_json(&json!({"question": "Is the claim paid?", "version": 2})).unwrap();
        let request = request_for(
            policy(provider, anthropic_config()),
            &context,
            PROMPT.as_bytes(),
            SCHEMA.as_bytes(),
        );
        let material =
            InferenceRequestMaterial::new(&request, context, PROMPT.into(), SCHEMA.into()).unwrap();
        (request, material)
    }

    // ------------------------------------------------------------------ material

    #[test]
    fn material_new_builds_only_validated_material() {
        let (request, material) = standard("anthropic");
        let built =
            InferenceRequestMaterial::new(&request, context_bytes(), PROMPT.into(), SCHEMA.into())
                .unwrap();
        assert_eq!(built, material);
        assert_eq!(built.request_hash, request.id);
        // Wrong bytes for this request are rejected at construction.
        assert!(matches!(
            InferenceRequestMaterial::new(&request, context_bytes(), b"other\n".to_vec(), SCHEMA.into()),
            Err(ProviderError::MaterialHashMismatch { field }) if field == "prompt_template"
        ));
        let noncanonical = br#"{"version": 1, "question": "Is the leave approved?"}"#.to_vec();
        let r = request_for(
            policy("anthropic", anthropic_config()),
            &noncanonical,
            PROMPT.as_bytes(),
            SCHEMA.as_bytes(),
        );
        assert!(matches!(
            InferenceRequestMaterial::new(&r, noncanonical, PROMPT.into(), SCHEMA.into()),
            Err(ProviderError::NonCanonicalContext)
        ));
    }

    #[test]
    fn material_serde_is_strict_and_roundtrips() {
        let (request, material) = standard("anthropic");
        let text = serde_json::to_string(&material).unwrap();
        let back: InferenceRequestMaterial = serde_json::from_str(&text).unwrap();
        assert_eq!(back, material);
        back.validate_for(&request).unwrap();
        let mut value: Value = serde_json::from_str(&text).unwrap();
        value["extra"] = json!(1);
        assert!(serde_json::from_value::<InferenceRequestMaterial>(value).is_err());
        let mut value: Value = serde_json::from_str(&text).unwrap();
        value.as_object_mut().unwrap().remove("schema_json");
        assert!(serde_json::from_value::<InferenceRequestMaterial>(value).is_err());
    }

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
        let (request, material) = standard("mock");
        assert!(matches!(
            NullProvider.execute(&request, &material),
            Err(ProviderError::ProviderDisabled)
        ));
        let execution = mock_execution(&request);
        let mock = MockProvider::with_fixtures([(request.id.clone(), execution.clone())]).unwrap();
        assert_eq!(mock.execute(&request, &material).unwrap(), execution);
        // Replay is deterministic.
        assert_eq!(mock.execute(&request, &material).unwrap(), execution);
        let (other, other_material) = standard("anthropic");
        assert!(matches!(
            MockProvider::new().execute(&other, &other_material),
            Err(ProviderError::FixtureNotFound(_))
        ));
    }

    #[test]
    fn null_and_mock_validate_the_binding_first() {
        let (request, material) = standard("mock");
        let (_, foreign) = second("mock");
        let mut corrupt = material.clone();
        corrupt.context_json.push(b' ');
        let mock =
            MockProvider::with_fixtures([(request.id.clone(), mock_execution(&request))]).unwrap();
        for bad in [&foreign, &corrupt] {
            let null = NullProvider.execute(&request, bad).unwrap_err();
            assert!(!matches!(null, ProviderError::ProviderDisabled), "{null:?}");
            let m = mock.execute(&request, bad).unwrap_err();
            assert!(
                matches!(
                    m,
                    ProviderError::MaterialRequestMismatch { .. }
                        | ProviderError::MaterialHashMismatch { .. }
                ),
                "{m:?}"
            );
        }
    }

    /// H0.8 (architecture §33.8): a replay fixture is never a schema bypass. The fixture's
    /// validated output is checked against the request material's compiled task output schema,
    /// exactly as the live provider checks model output before admission.
    #[test]
    fn replay_fixture_must_satisfy_schema() {
        let (request, material) = standard("mock");
        let mut invalid = mock_execution(&request);
        let raw = br#"{"answer":"maybe"}"#.to_vec();
        let output = CanonicalJson::new(json!({"answer": "maybe"}));
        invalid.artifact.raw_response_hash = Hash::content_sha256(&raw);
        invalid.artifact.validated_output_hash = output.content_hash().unwrap();
        invalid.artifact.validated_output = output;
        invalid.raw_response = raw;
        // The fixture is structurally valid, so registration (which has no schema) accepts it.
        let mock = MockProvider::with_fixtures([(request.id.clone(), invalid)]).unwrap();
        let err = mock.execute(&request, &material).unwrap_err();
        assert!(
            matches!(&err, ProviderError::SchemaInvalidOutput { errors } if !errors.is_empty()),
            "{err:?}"
        );
        // An extra property the closed schema forbids is rejected the same way.
        let mut extra = mock_execution(&request);
        let raw = br#"{"answer":"yes","note":"x"}"#.to_vec();
        let output = CanonicalJson::new(json!({"answer": "yes", "note": "x"}));
        extra.artifact.raw_response_hash = Hash::content_sha256(&raw);
        extra.artifact.validated_output_hash = output.content_hash().unwrap();
        extra.artifact.validated_output = output;
        extra.raw_response = raw;
        let mock = MockProvider::with_fixtures([(request.id.clone(), extra)]).unwrap();
        assert!(matches!(
            mock.execute(&request, &material),
            Err(ProviderError::SchemaInvalidOutput { .. })
        ));
        // A schema-valid fixture still replays unchanged.
        let valid = mock_execution(&request);
        let mock = MockProvider::with_fixtures([(request.id.clone(), valid.clone())]).unwrap();
        assert_eq!(mock.execute(&request, &material).unwrap(), valid);
    }

    #[test]
    fn provider_execution_serde_is_strict_and_validated() {
        let (request, _) = standard("mock");
        let execution = mock_execution(&request);
        let text = serde_json::to_string(&execution).unwrap();
        let back: ProviderExecution = serde_json::from_str(&text).unwrap();
        assert_eq!(back, execution);
        let original: Value = serde_json::from_str(&text).unwrap();
        // Unknown fields are rejected.
        let mut value = original.clone();
        value["extra"] = json!(true);
        assert!(serde_json::from_value::<ProviderExecution>(value).is_err());
        // Raw bytes that do not match raw_response_hash are rejected on decode.
        let mut value = original.clone();
        value["raw_response"] = serde_json::to_value(b"{\"answer\":\"no\"}".to_vec()).unwrap();
        assert!(serde_json::from_value::<ProviderExecution>(value).is_err());
        let mut value = original;
        value["artifact"]["raw_response_hash"] =
            serde_json::to_value(Hash::content_sha256(b"other")).unwrap();
        assert!(serde_json::from_value::<ProviderExecution>(value).is_err());
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
        use std::time::{Duration, Instant};

        use plumb_core::{to_canonical_json, CanonicalJson, Hash};
        use plumb_inference::anthropic::RetryPolicy;
        use plumb_inference::anthropic::{
            default_request_timeout_ms, ANTHROPIC_DEFAULT_CONNECT_TIMEOUT_MS,
            ANTHROPIC_MAX_REQUEST_TIMEOUT_MS, ANTHROPIC_MAX_TOKENS_LIMIT,
        };
        use plumb_inference::{AnthropicProvider, InferenceProvider, ProviderError};
        use serde_json::{json, Value};

        use super::{
            anthropic_config, context_bytes, material_for, policy, request_for, second, standard,
            PROMPT, SCHEMA,
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
            serve_sequence(&[(status, extra_headers, body)])
        }

        /// One HTTP/1.1 reply.
        fn reply_bytes(status: &str, extra_headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
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
            reply
        }

        /// One scripted reply: status line, extra headers, body.
        type Reply<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a [u8]);

        /// A local responder answering the n-th connection with the n-th reply (the last one
        /// repeats).
        fn serve_sequence(replies: &[Reply<'_>]) -> Server {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let connections = Arc::new(AtomicUsize::new(0));
            let captured = Arc::new(Mutex::new(Vec::new()));
            let (c, cap) = (connections.clone(), captured.clone());
            let replies: Vec<Vec<u8>> = replies
                .iter()
                .map(|(s, h, b)| reply_bytes(s, h, b))
                .collect();
            thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    let reply = &replies[n.min(replies.len() - 1)];
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
                    stream.write_all(reply).unwrap();
                    stream.flush().unwrap();
                }
            });
            Server {
                endpoint: format!("http://127.0.0.1:{port}/v1/messages"),
                connections,
                captured,
            }
        }

        /// The exact header set of every Messages API request (H0.8, architecture §33.8):
        /// x-api-key authentication, the configured anthropic-version, the JSON content type
        /// and the transport headers; never `authorization`.
        const EXACT_HEADERS: [&str; 6] = [
            "accept",
            "anthropic-version",
            "content-length",
            "content-type",
            "host",
            "x-api-key",
        ];

        fn header_names(captured: &Captured) -> Vec<&str> {
            captured.headers.keys().map(String::as_str).collect()
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

        fn provider(server: &Server) -> AnthropicProvider {
            AnthropicProvider::with_endpoint(KEY, &server.endpoint).unwrap()
        }

        /// The provider of `server` retrying transient statuses without waiting.
        fn eager(server: &Server) -> AnthropicProvider {
            provider(server).with_retry_policy(RetryPolicy {
                base_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
                ..RetryPolicy::default()
            })
        }

        fn run_with(
            body: &[u8],
        ) -> (
            Result<plumb_inference::ProviderExecution, ProviderError>,
            Server,
        ) {
            let server = serve("200 OK", &[("content-type", "application/json")], body);
            let (request, material) = standard("anthropic");
            let result = provider(&server).execute(&request, &material);
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
            let p = provider(&server);
            let execution = p.execute(&request, &material).unwrap();
            execution.validate_for(&request).unwrap();
            assert_eq!(execution.raw_response, body);
            assert_eq!(execution.raw_response_media_type, "application/json");
            let a = &execution.artifact;
            assert_eq!(a.raw_response_hash, Hash::content_sha256(&body));
            assert_eq!(a.request_hash, request.id);
            assert_eq!(a.provider, "anthropic");
            assert_eq!(a.model, "test-model-returned");
            // Fable #66: the recorded parameters are exactly the generation parameters of the
            // request body actually sent (model and max_tokens; no sampling parameters).
            assert_eq!(
                a.parameters.as_value(),
                &json!({"model": "test-model-configured", "max_tokens": 256})
            );
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
            assert_eq!(sent.headers["x-api-key"], KEY);
            assert!(!sent.headers.contains_key("authorization"));
            assert_eq!(sent.headers["anthropic-version"], "2023-06-01");
            assert_eq!(sent.headers["content-type"], "application/json");
            assert!(!sent.headers.contains_key("anthropic-workspace-id"));
            assert_eq!(header_names(&sent), EXACT_HEADERS.to_vec());
            let body: Value = serde_json::from_slice(&sent.body).unwrap();
            // The persisted parameters are exactly the body's generation parameters.
            let mut sent_parameters = body.as_object().unwrap().clone();
            sent_parameters.remove("system");
            sent_parameters.remove("messages");
            sent_parameters.remove("output_config");
            assert_eq!(&Value::Object(sent_parameters), a.parameters.as_value());
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
                    // Structured outputs: the bound output schema (already part of the request
                    // identity) constrains decoding; it is not a generation parameter.
                    "output_config": {"format": {"type": "json_schema", "schema": schema}},
                })
            );
            // The secret is confined to the x-api-key header.
            let sent_text = String::from_utf8_lossy(&sent.body);
            assert!(!sent_text.contains(KEY));
            assert!(!format!("{p:?}").contains(KEY));
            assert!(!format!("{request:?}").contains(KEY));
            assert!(!format!("{execution:?}").contains(KEY));
            assert_eq!(sent.headers.values().filter(|v| v.contains(KEY)).count(), 1);
        }

        /// The API accepts a JSON Schema subset for structured outputs (measured against the
        /// live API): numeric bounds, `maxItems`, `uniqueItems` and `minItems` above 1 are
        /// rejected, `oneOf` is rejected (`anyOf` is accepted), and every object must close
        /// its properties. The constraint sent to the API drops exactly what it rejects; the
        /// full schema still validates every answer locally.
        #[test]
        fn output_schema_sent_to_the_api_is_the_supported_subset() {
            use plumb_inference::anthropic::api_output_schema;
            let full = json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "additionalProperties": false,
                "required": ["n", "names", "pick", "one"],
                "properties": {
                    "n": {"type": "integer", "minimum": 0, "maximum": 9,
                          "exclusiveMinimum": -1, "exclusiveMaximum": 10, "multipleOf": 1},
                    "names": {"type": "array", "items": {"type": "string", "minLength": 1,
                              "pattern": "^[a-z]+$"},
                              "minItems": 2, "maxItems": 5, "uniqueItems": true},
                    "one": {"type": "array", "items": {"$ref": "#/$defs/x"}, "minItems": 1},
                    "pick": {"oneOf": [{"type": "string"}, {"$ref": "#/$defs/x"}]}
                },
                "$defs": {"x": {"type": "object", "additionalProperties": false,
                                "required": ["v"], "properties": {"v": {"type": "number",
                                "minimum": 0}}}}
            });
            assert_eq!(
                api_output_schema(&full),
                Some(json!({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["n", "names", "pick", "one"],
                    "properties": {
                        "n": {"type": "integer"},
                        "names": {"type": "array", "items": {"type": "string", "minLength": 1,
                                  "pattern": "^[a-z]+$"}},
                        "one": {"type": "array", "items": {"$ref": "#/$defs/x"}, "minItems": 1},
                        "pick": {"anyOf": [{"type": "string"}, {"$ref": "#/$defs/x"}]}
                    },
                    "$defs": {"x": {"type": "object", "additionalProperties": false,
                                    "required": ["v"], "properties": {"v": {"type": "number"}}}}
                }))
            );
            // Grammar size (measured: the EARS schema's per-pattern variants were "too large"):
            // object variants that all require exactly the same keys merge into one object whose
            // properties take the union of the variants' schemas; variants with different keys,
            // and non-object branches, stay alternatives.
            let variants = json!({"anyOf": [
                {"type": "null"},
                {"type": "object", "additionalProperties": false, "required": ["p", "a"],
                 "properties": {"p": {"const": "one"}, "a": {"type": "null"}}},
                {"type": "object", "additionalProperties": false, "required": ["p", "a"],
                 "properties": {"p": {"const": "two"}, "a": {"anyOf": [{"type": "string"},
                                                                        {"type": "null"}]}}}
            ]});
            assert_eq!(
                api_output_schema(&variants),
                Some(json!({"anyOf": [
                    {"type": "null"},
                    {"type": "object", "additionalProperties": false, "required": ["p", "a"],
                     "properties": {
                         "a": {"anyOf": [{"type": "null"}, {"type": "string"}]},
                         "p": {"anyOf": [{"const": "one"}, {"const": "two"}]}}}
                ]}))
            );
            let distinct = json!({"anyOf": [
                {"type": "object", "additionalProperties": false, "required": ["g"],
                 "properties": {"g": {"const": 1}}},
                {"type": "object", "additionalProperties": false, "required": ["n", "s"],
                 "properties": {"n": {"const": 2}, "s": {"type": "string"}}}
            ]});
            assert_eq!(api_output_schema(&distinct), Some(distinct.clone()));
            // Property *names* that look like keywords are data, never dropped.
            let named = json!({"type": "object", "additionalProperties": false,
                "required": ["minimum"], "properties": {"minimum": {"type": "string"}}});
            assert_eq!(api_output_schema(&named), Some(named.clone()));
            // An open object cannot be expressed: no constraint is sent for that schema.
            for open in [
                json!({"type": "object", "properties": {"p": {"type": "object"}},
                       "required": ["p"], "additionalProperties": false}),
                json!({"type": "object", "required": [], "properties": {}}),
                json!({"type": "object", "additionalProperties": true}),
            ] {
                assert_eq!(api_output_schema(&open), None, "{open}");
            }
        }

        /// Constrained decoding follows the schema's property order, so the constraint keeps
        /// the authored order of the committed schema bytes (real-model run: with sorted keys a
        /// variant whose first sorted key differs from the model's natural first key was ruled
        /// out mid-object, which forced filler such as "x:y" into the other variant).
        #[test]
        fn output_constraint_keeps_the_authored_property_order() {
            let body = message(
                "m",
                "end_turn",
                json!([text(r#"{"zeta":"z","alpha":"a"}"#)]),
            );
            let server = serve("200 OK", &[("content-type", "application/json")], &body);
            let schema = r#"{"type":"object","additionalProperties":false,"required":["zeta","alpha"],
                "properties":{"zeta":{"type":"string"},"alpha":{"type":"string"}}}"#;
            let (request, material) = super::with_schema("anthropic", schema);
            provider(&server)
                .execute(&request, &material)
                .unwrap()
                .validate_for(&request)
                .unwrap();
            let sent = server.captured.lock().unwrap()[0].clone();
            let raw = String::from_utf8(sent.body).unwrap();
            let constraint = &raw[raw.find(r#""output_config""#).unwrap()..];
            let zeta = constraint.find(r#""zeta":{"#).unwrap();
            let alpha = constraint.find(r#""alpha":{"#).unwrap();
            assert!(zeta < alpha, "{constraint}");
            let required = constraint.find(r#""required":["zeta","alpha"]"#);
            assert!(required.is_some(), "{constraint}");
        }

        /// A schema the API subset cannot express is sent without `output_config`; the answer
        /// is still validated locally against the full schema.
        #[test]
        fn inexpressible_output_schema_is_sent_without_a_constraint() {
            let body = message("m", "end_turn", json!([text(r#"{"answer":{"free":1}}"#)]));
            let server = serve("200 OK", &[("content-type", "application/json")], &body);
            let (request, material) = super::with_schema(
                "anthropic",
                r#"{"type":"object","additionalProperties":false,"required":["answer"],
                    "properties":{"answer":{"type":"object"}}}"#,
            );
            let p = provider(&server);
            p.execute(&request, &material)
                .unwrap()
                .validate_for(&request)
                .unwrap();
            let sent = server.captured.lock().unwrap()[0].clone();
            let body: Value = serde_json::from_slice(&sent.body).unwrap();
            assert!(body.get("output_config").is_none(), "{body}");
        }

        /// Optional effort (real-model run: at the API default the model skipped all
        /// thinking on the 32-candidate classification and emitted an invented reference).
        /// It is a generation parameter: sent in `output_config` next to the format, part of
        /// the configuration identity, and recorded with the other parameters.
        #[test]
        fn configured_effort_is_sent_and_recorded() {
            use plumb_inference::anthropic::validate_config;
            let body = message("m", "end_turn", json!([text(r#"{"answer":"yes"}"#)]));
            let server = serve("200 OK", &[("content-type", "application/json")], &body);
            let mut config = anthropic_config();
            config["effort"] = json!("high");
            let context = context_bytes();
            let request = request_for(
                policy("anthropic", config.clone()),
                &context,
                PROMPT.as_bytes(),
                SCHEMA.as_bytes(),
            );
            let material = material_for(&request, &context, PROMPT.as_bytes(), SCHEMA.as_bytes());
            let execution = provider(&server).execute(&request, &material).unwrap();
            let sent = server.captured.lock().unwrap()[0].clone();
            let sent: Value = serde_json::from_slice(&sent.body).unwrap();
            let schema: Value = serde_json::from_str(SCHEMA).unwrap();
            assert_eq!(
                sent["output_config"],
                json!({"effort": "high", "format": {"type": "json_schema", "schema": schema}})
            );
            assert_eq!(
                execution.artifact.parameters.as_value(),
                &json!({"model": "test-model-configured", "max_tokens": 256,
                        "output_config": {"effort": "high"}})
            );
            for level in ["low", "medium", "high", "xhigh", "max"] {
                config["effort"] = json!(level);
                validate_config(&config).unwrap();
            }
            for bad in [json!("extreme"), json!(""), json!(3), Value::Null] {
                config["effort"] = bad.clone();
                assert!(validate_config(&config).is_err(), "{bad}");
            }
        }

        #[test]
        fn one_instance_executes_distinct_requests_with_their_own_material() {
            let body = message("m", "end_turn", json!([text(r#"{"answer":"yes"}"#)]));
            let server = serve("200 OK", &[], &body);
            let p = provider(&server);
            let (a, a_material) = standard("anthropic");
            let (b, b_material) = second("anthropic");
            assert_ne!(a.id, b.id);
            p.execute(&a, &a_material)
                .unwrap()
                .validate_for(&a)
                .unwrap();
            p.execute(&b, &b_material)
                .unwrap()
                .validate_for(&b)
                .unwrap();
            let sent = server.captured.lock().unwrap().clone();
            assert_eq!(sent.len(), 2);
            let user = |m: &super::InferenceRequestMaterial| {
                let context: Value = serde_json::from_slice(&m.context_json).unwrap();
                let schema: Value = serde_json::from_slice(&m.schema_json).unwrap();
                String::from_utf8(
                    to_canonical_json(&json!({"context": context, "output_schema": schema}))
                        .unwrap(),
                )
                .unwrap()
            };
            for (captured, material) in sent.iter().zip([&a_material, &b_material]) {
                let body: Value = serde_json::from_slice(&captured.body).unwrap();
                assert_eq!(body["messages"][0]["content"], user(material));
            }
            // Swapped material is rejected before any further connection.
            assert!(matches!(
                p.execute(&a, &b_material),
                Err(ProviderError::MaterialRequestMismatch { .. })
            ));
            assert_eq!(
                server.connections.load(std::sync::atomic::Ordering::SeqCst),
                2
            );
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
            let execution = provider(&server).execute(&request, &material).unwrap();
            let sent = server.captured.lock().unwrap()[0].clone();
            assert_eq!(sent.headers["anthropic-workspace-id"], "wrkspc_local");
            assert_eq!(sent.headers["x-api-key"], KEY);
            let mut expected = EXACT_HEADERS.to_vec();
            expected.insert(2, "anthropic-workspace-id");
            assert_eq!(header_names(&sent), expected);
            // The workspace is routing configuration, not a generation parameter.
            assert_eq!(
                execution.artifact.parameters.as_value(),
                &json!({"model": "test-model-configured", "max_tokens": 256})
            );
        }

        #[test]
        fn timeouts_are_validated_identity_bound_config() {
            let body = message("m", "end_turn", json!([text(r#"{"answer":"yes"}"#)]));
            let server = serve("200 OK", &[], &body);
            let context = context_bytes();
            let attempt = |config: Value| {
                let request = request_for(
                    policy("anthropic", config),
                    &context,
                    PROMPT.as_bytes(),
                    SCHEMA.as_bytes(),
                );
                let material =
                    material_for(&request, &context, PROMPT.as_bytes(), SCHEMA.as_bytes());
                (
                    request.id.clone(),
                    provider(&server).execute(&request, &material),
                )
            };
            for (key, value) in [
                ("request_timeout_ms", json!(0)),
                ("request_timeout_ms", json!(-1)),
                ("request_timeout_ms", json!(1.5)),
                ("request_timeout_ms", json!("60000")),
                (
                    "request_timeout_ms",
                    json!(ANTHROPIC_MAX_REQUEST_TIMEOUT_MS + 1),
                ),
                ("request_timeout_ms", Value::Null),
                ("connect_timeout_ms", json!(0)),
                ("connect_timeout_ms", Value::Null),
                // The connect timeout cannot exceed the total timeout.
                (
                    "connect_timeout_ms",
                    json!(ANTHROPIC_MAX_REQUEST_TIMEOUT_MS),
                ),
            ] {
                let mut c = anthropic_config();
                c[key] = value.clone();
                assert!(
                    matches!(
                        attempt(c).1,
                        Err(ProviderError::InvalidProviderConfig { .. })
                    ),
                    "{key}={value}"
                );
            }
            assert_eq!(
                server.connections.load(std::sync::atomic::Ordering::SeqCst),
                0
            );
            // Explicit timeouts are accepted and bound into the request identity.
            let mut explicit = anthropic_config();
            explicit["request_timeout_ms"] = json!(30_000);
            explicit["connect_timeout_ms"] = json!(5_000);
            let (explicit_id, result) = attempt(explicit);
            result.unwrap();
            let (default_id, result) = attempt(anthropic_config());
            result.unwrap();
            assert_ne!(explicit_id, default_id);
        }

        #[test]
        fn default_timeout_is_sized_for_max_tokens() {
            let small = default_request_timeout_ms(256);
            let large = default_request_timeout_ms(ANTHROPIC_MAX_TOKENS_LIMIT);
            assert!(small >= 120_000, "{small}");
            assert!(large > small);
            // A full-size non-streaming generation is not cut off by the old fixed 120 s.
            assert!(large >= 3_600_000, "{large}");
            assert!(large <= ANTHROPIC_MAX_REQUEST_TIMEOUT_MS);
            assert!(ANTHROPIC_DEFAULT_CONNECT_TIMEOUT_MS < small);
        }

        #[test]
        fn a_timeout_is_typed_and_distinct_from_transport_failure() {
            // A local server that accepts the connection and never answers.
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            thread::spawn(move || {
                let mut held = Vec::new();
                for stream in listener.incoming() {
                    held.push(stream);
                }
            });
            let mut config = anthropic_config();
            config["request_timeout_ms"] = json!(300);
            config["connect_timeout_ms"] = json!(200);
            let context = context_bytes();
            let request = request_for(
                policy("anthropic", config),
                &context,
                PROMPT.as_bytes(),
                SCHEMA.as_bytes(),
            );
            let material = material_for(&request, &context, PROMPT.as_bytes(), SCHEMA.as_bytes());
            let p = AnthropicProvider::with_endpoint(
                KEY,
                &format!("http://127.0.0.1:{port}/v1/messages"),
            )
            .unwrap();
            let started = std::time::Instant::now();
            let err = p.execute(&request, &material).unwrap_err();
            assert!(matches!(err, ProviderError::Timeout { .. }), "{err:?}");
            assert!(started.elapsed() < std::time::Duration::from_secs(10));
            assert!(!format!("{err} {err:?}").contains(KEY));
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
                provider(&server).execute(&request, &material)
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
            // Corrupted and foreign material.
            let (request, material) = standard("anthropic");
            let mut corrupt = material.clone();
            corrupt.schema_json.push(b'\n');
            assert!(matches!(
                provider(&server).execute(&request, &corrupt),
                Err(ProviderError::MaterialHashMismatch { .. })
            ));
            let (other_request, other_material) = second("anthropic");
            assert_ne!(other_request.id, request.id);
            assert!(matches!(
                provider(&server).execute(&request, &other_material),
                Err(ProviderError::MaterialRequestMismatch { .. })
            ));
            assert_eq!(
                server.connections.load(std::sync::atomic::Ordering::SeqCst),
                0
            );
        }

        #[test]
        fn secrets_and_endpoints_are_validated() {
            for key in ["", " key", "key ", "ke\ny"] {
                let err = AnthropicProvider::new(key).unwrap_err();
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
                        AnthropicProvider::with_endpoint(KEY, endpoint),
                        Err(ProviderError::InvalidEndpoint { .. })
                    ),
                    "{endpoint}"
                );
            }
            let p = AnthropicProvider::new(KEY).unwrap();
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

        /// H0.3 (Fable #62): 429, 529 and other 5xx statuses are retried with bounded backoff;
        /// every other status is final at once. The last answer stays a typed remote error.
        #[test]
        fn remote_statuses_are_typed_and_only_transient_ones_retried() {
            assert_eq!(RetryPolicy::default().max_attempts, 3);
            for (status, attempts) in [
                ("400 Bad Request", 1),
                ("401 Unauthorized", 1),
                ("403 Forbidden", 1),
                ("404 Not Found", 1),
                ("429 Too Many Requests", 3),
                ("500 Internal Server Error", 3),
                ("529 Overloaded", 3),
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
                let err = eager(&server).execute(&request, &material).unwrap_err();
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
                    attempts,
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
            let err = eager(&server).execute(&request, &material).unwrap_err();
            assert!(matches!(
                &err,
                ProviderError::Remote { status: 500, error_type: None, message: None, request_id: Some(r) } if r == "req_header_2"
            ));
            assert!(!format!("{err}").contains("upstream exploded"));
        }

        /// H0.3 (Fable #62): a transient status is retried after the provider's `retry-after`
        /// and the later success is the execution, unchanged (no output repair); a
        /// `retry-after` beyond the bound is not waited for.
        #[test]
        fn transient_status_is_retried_honoring_retry_after() {
            let body = message("m", "end_turn", json!([text(r#"{"answer":"yes"}"#)]));
            let overloaded =
                br#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
            let server = serve_sequence(&[
                ("529 Overloaded", &[("retry-after", "1")], overloaded),
                ("200 OK", &[("content-type", "application/json")], &body),
            ]);
            let (request, material) = standard("anthropic");
            let started = Instant::now();
            let execution = provider(&server).execute(&request, &material).unwrap();
            assert!(started.elapsed() >= Duration::from_secs(1));
            assert_eq!(execution.raw_response, body);
            assert_eq!(
                server.connections.load(std::sync::atomic::Ordering::SeqCst),
                2
            );
            // The same request body is sent on every attempt.
            let sent = server.captured.lock().unwrap().clone();
            assert_eq!(sent[0].body, sent[1].body);

            let server = serve("429 Too Many Requests", &[("retry-after", "3600")], b"{}");
            let started = Instant::now();
            let err = provider(&server).execute(&request, &material).unwrap_err();
            assert!(matches!(err, ProviderError::Remote { status: 429, .. }));
            assert!(started.elapsed() < Duration::from_secs(5));
            assert_eq!(
                server.connections.load(std::sync::atomic::Ordering::SeqCst),
                1
            );
        }

        /// H0.3 (Fable #253): an answer the adapter rejects keeps its exact raw bytes for the
        /// caller to store; `execute` keeps returning the same typed error.
        #[test]
        fn rejected_answers_carry_their_raw_response() {
            let body = message("m", "end_turn", json!([text(r#"{"answer":1}"#)]));
            let server = serve("200 OK", &[("content-type", "application/json")], &body);
            let (request, material) = standard("anthropic");
            let failure = provider(&server)
                .execute_detailed(&request, &material)
                .unwrap_err();
            assert!(matches!(
                failure.error,
                ProviderError::SchemaInvalidOutput { .. }
            ));
            let rejected = failure.rejected_response.unwrap();
            assert_eq!(rejected.bytes, body);
            assert_eq!(rejected.media_type, "application/json");
            let err = provider(&server).execute(&request, &material).unwrap_err();
            assert!(matches!(err, ProviderError::SchemaInvalidOutput { .. }));
            // A non-success status is a typed remote error without a stored answer.
            let server = serve("400 Bad Request", &[], b"{}");
            let failure = provider(&server)
                .execute_detailed(&request, &material)
                .unwrap_err();
            assert!(matches!(
                failure.error,
                ProviderError::Remote { status: 400, .. }
            ));
            assert!(failure.rejected_response.is_none());
        }

        #[test]
        fn redirects_are_not_followed() {
            let server = serve(
                "302 Found",
                &[("location", "http://127.0.0.1:9/elsewhere")],
                b"",
            );
            let (request, material) = standard("anthropic");
            let err = provider(&server).execute(&request, &material).unwrap_err();
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
            )
            .unwrap();
            let err = p.execute(&request, &material).unwrap_err();
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
            let p = AnthropicProvider::new(key).unwrap();
            let execution = p.execute(&request, &material).unwrap();
            assert_eq!(
                execution.artifact.validated_output.as_value(),
                &json!({"ok": true})
            );
            execution.validate_for(&request).unwrap();
            let _ = Hash::content_sha256(b"");
        }
    }
}
