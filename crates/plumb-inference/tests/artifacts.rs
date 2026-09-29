//! F0.9 contract tests: inference requests and artifacts, providers, acquisition persistence
//! and deterministic DerivationRecord materialization.
//!
//! Golden values were calculated independently from the literal canonical JSON bytes below.

use plumb_artifacts::{ArtifactKind, ArtifactStore, SqliteArtifactStore};
use plumb_core::{to_canonical_json, CanonicalJson, Hash, HashKind, Id, StageId, Timestamp};
use plumb_inference::*;
use serde_json::{json, Value};

const H1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const H2: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const H3: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const T1: &str = "2026-09-29T12:00:00.000000000Z";
const T2: &str = "2026-09-29T13:00:00.000000000Z";

const GOLDEN_PROJECTION: &str = r#"{"context_hash":"sha256:1111111111111111111111111111111111111111111111111111111111111111","evidence_refs":["evd:0123456789abcdef"],"input_refs":["req:HR-001","src:0123456789abcdef"],"prompt_template_hash":"sha256:2222222222222222222222222222222222222222222222222222222222222222","provider_policy":{"config":{"max_tokens":4096,"temperature":0},"provider":"anthropic"},"schema_hash":"sha256:3333333333333333333333333333333333333333333333333333333333333333","stage":"S2","task_kind":"extract-requirements"}"#;
const GOLDEN_REQUEST_ID: &str =
    "sha256:b122087c03c50cb6f4ae9814bcdac2cd43ff9c8e419ac191a0230f25a521f274";
const GOLDEN_REQUEST_JSON: &str = r#"{"context_hash":"sha256:1111111111111111111111111111111111111111111111111111111111111111","evidence_refs":["evd:0123456789abcdef"],"id":"sha256:b122087c03c50cb6f4ae9814bcdac2cd43ff9c8e419ac191a0230f25a521f274","input_refs":["req:HR-001","src:0123456789abcdef"],"prompt_template_hash":"sha256:2222222222222222222222222222222222222222222222222222222222222222","provider_policy":{"config":{"max_tokens":4096,"temperature":0},"provider":"anthropic"},"schema_hash":"sha256:3333333333333333333333333333333333333333333333333333333333333333","stage":"S2","task_kind":"extract-requirements"}"#;
const GOLDEN_REQUEST_REF: &str =
    "sha256:d63507c2581d78d86f845a5f6bf10abd6c36783e3a782a5f30c4021265d6a5b7";
const RAW_RESPONSE: &[u8] = br#"{"id":"msg_01","content":[{"type":"text","text":"ok"}]}"#;
const GOLDEN_RAW_HASH: &str =
    "sha256:25a8da9bfa43244bd47a40b7026057fb63c62e088a838507cf9662f628898cd3";
const GOLDEN_OUTPUT_HASH: &str =
    "sha256:999749231adb93766b104854ad9e37c9fd852fbf55163f4528f206ebd0a6cd27";
const GOLDEN_ARTIFACT_JSON: &str = r#"{"model":"claude-opus-5-5","parameters":{"max_tokens":4096,"temperature":0},"provider":"anthropic","raw_response_hash":"sha256:25a8da9bfa43244bd47a40b7026057fb63c62e088a838507cf9662f628898cd3","request_hash":"sha256:b122087c03c50cb6f4ae9814bcdac2cd43ff9c8e419ac191a0230f25a521f274","validated_output":{"requirements":[{"id":"req:HR-001","statement":"Employees shall submit leave requests."}]},"validated_output_hash":"sha256:999749231adb93766b104854ad9e37c9fd852fbf55163f4528f206ebd0a6cd27"}"#;
const GOLDEN_VALIDATED_REF: &str =
    "sha256:bb1d811ba68148e9195aad5612f5b81dd6a825f6ebe87851884bbc492f4031e8";
const GOLDEN_DRV_ID: &str = "drv:48d777dc5cc38d3c";

// ---------------------------------------------------------------------------- builders

fn id(s: &str) -> Id {
    s.parse().unwrap()
}

fn hash(s: &str) -> Hash {
    s.parse().unwrap()
}

fn ts(s: &str) -> Timestamp {
    s.parse().unwrap()
}

fn policy(provider: &str) -> ProviderPolicy {
    ProviderPolicy {
        provider: provider.to_owned(),
        config: CanonicalJson::new(json!({"temperature": 0, "max_tokens": 4096})),
    }
}

/// The golden request, built with refs in non-canonical insertion order.
fn request() -> InferenceRequest {
    InferenceRequest::new(
        StageId::S2,
        "extract-requirements".to_owned(),
        vec![id("src:0123456789abcdef"), id("req:HR-001")],
        vec![id("evd:0123456789abcdef")],
        hash(H1),
        hash(H2),
        hash(H3),
        policy("anthropic"),
    )
    .unwrap()
}

fn output() -> CanonicalJson {
    CanonicalJson::new(json!({
        "requirements": [{"id": "req:HR-001", "statement": "Employees shall submit leave requests."}]
    }))
}

fn execution(request: &InferenceRequest) -> ProviderExecution {
    ProviderExecution {
        artifact: InferenceArtifact {
            request_hash: request.id.clone(),
            provider: "anthropic".to_owned(),
            model: "claude-opus-5-5".to_owned(),
            parameters: CanonicalJson::new(json!({"temperature": 0, "max_tokens": 4096})),
            raw_response_hash: Hash::content_sha256(RAW_RESPONSE),
            validated_output: output(),
            validated_output_hash: output().content_hash().unwrap(),
        },
        raw_response_media_type: "application/json".to_owned(),
        raw_response: RAW_RESPONSE.to_vec(),
    }
}

fn request_value() -> Value {
    serde_json::from_str(GOLDEN_REQUEST_JSON).unwrap()
}

fn artifact_value() -> Value {
    serde_json::from_str(GOLDEN_ARTIFACT_JSON).unwrap()
}

fn persisted() -> PersistedInferenceRefs {
    PersistedInferenceRefs {
        request_artifact_ref: hash(GOLDEN_REQUEST_REF),
        raw_response_ref: hash(GOLDEN_RAW_HASH),
        validated_inference_ref: hash(GOLDEN_VALIDATED_REF),
    }
}

fn outputs() -> Vec<String> {
    vec!["prop:hr-002".to_owned(), "prop:hr-001".to_owned()]
}

/// A request rebuilt from the golden one with `edit` applied to its identity fields.
fn rebuilt(edit: impl FnOnce(&mut InferenceRequest)) -> InferenceRequest {
    let mut r = request();
    edit(&mut r);
    InferenceRequest::new(
        r.stage,
        r.task_kind,
        r.input_refs,
        r.evidence_refs,
        r.context_hash,
        r.prompt_template_hash,
        r.schema_hash,
        r.provider_policy,
    )
    .unwrap()
}

// ---------------------------------------------------------------------------- requests

#[test]
fn golden_request_projection_id_and_json() {
    let r = request();
    assert_eq!(
        to_canonical_json(&r.identity_projection()).unwrap(),
        GOLDEN_PROJECTION.as_bytes()
    );
    assert_eq!(r.id.as_str(), GOLDEN_REQUEST_ID);
    assert_eq!(r.recompute_id().unwrap(), r.id);
    assert_eq!(r.id.kind(), HashKind::Generic);
    assert_eq!(
        to_canonical_json(&r).unwrap(),
        GOLDEN_REQUEST_JSON.as_bytes()
    );
    assert_eq!(
        r.input_refs,
        vec![id("req:HR-001"), id("src:0123456789abcdef")]
    );
    let parsed: InferenceRequest = serde_json::from_str(GOLDEN_REQUEST_JSON).unwrap();
    assert_eq!(parsed, r);
    assert_eq!(r.validate().ok(), Some(()));
}

#[test]
fn every_identity_field_changes_the_id_but_ref_order_does_not() {
    let base = request().id;
    let variants = [
        rebuilt(|r| r.stage = StageId::S3),
        rebuilt(|r| r.task_kind = "extract-terms".to_owned()),
        rebuilt(|r| r.input_refs.push(id("req:HR-002"))),
        rebuilt(|r| r.evidence_refs.clear()),
        rebuilt(|r| r.context_hash = hash(H2)),
        rebuilt(|r| r.prompt_template_hash = hash(H3)),
        rebuilt(|r| r.schema_hash = hash(H1)),
        rebuilt(|r| r.provider_policy.provider = "openai".to_owned()),
        rebuilt(|r| r.provider_policy.config = CanonicalJson::new(json!({"temperature": 1}))),
        rebuilt(|r| r.provider_policy.config = CanonicalJson::new(json!({}))),
    ];
    let mut ids: Vec<Hash> = variants.iter().map(|r| r.id.clone()).collect();
    ids.push(base.clone());
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());

    let reordered = InferenceRequest::new(
        StageId::S2,
        "extract-requirements".to_owned(),
        vec![id("req:HR-001"), id("src:0123456789abcdef")],
        vec![id("evd:0123456789abcdef")],
        hash(H1),
        hash(H2),
        hash(H3),
        policy("anthropic"),
    )
    .unwrap();
    assert_eq!(reordered.id, base);
}

#[test]
fn every_stage_is_usable() {
    let ids: std::collections::BTreeSet<Hash> = StageId::ALL
        .into_iter()
        .map(|stage| {
            let r = rebuilt(|r| r.stage = stage);
            assert_eq!(r.stage, stage);
            let back: InferenceRequest =
                serde_json::from_slice(&to_canonical_json(&r).unwrap()).unwrap();
            assert_eq!(back, r);
            r.id
        })
        .collect();
    assert_eq!(ids.len(), 13);
}

#[test]
fn duplicate_refs_are_rejected() {
    let dup = |inputs: Vec<Id>, evidence: Vec<Id>| {
        InferenceRequest::new(
            StageId::S2,
            "extract-requirements".to_owned(),
            inputs,
            evidence,
            hash(H1),
            hash(H2),
            hash(H3),
            policy("anthropic"),
        )
        .unwrap_err()
    };
    assert!(matches!(
        dup(vec![id("req:HR-001"), id("req:HR-001")], vec![]),
        InferenceError::DuplicateRef {
            field: "input_refs",
            ..
        }
    ));
    assert!(matches!(
        dup(vec![], vec![id("evd:a"), id("evd:a")]),
        InferenceError::DuplicateRef {
            field: "evidence_refs",
            ..
        }
    ));
    let mut wire = request_value();
    wire["input_refs"] = json!(["req:HR-001", "req:HR-001"]);
    assert!(serde_json::from_value::<InferenceRequest>(wire).is_err());
    let mut wire = request_value();
    wire["input_refs"] = json!(["src:0123456789abcdef", "req:HR-001"]);
    assert!(serde_json::from_value::<InferenceRequest>(wire).is_err());
}

#[test]
fn request_hashes_must_be_generic() {
    let semantic = format!("psg:sha256:{}", "a".repeat(64));
    let evidence = format!("ev:sha256:{}", "a".repeat(64));
    for field in ["context_hash", "prompt_template_hash", "schema_hash"] {
        for bad in [&semantic, &evidence] {
            let mut r = request();
            match field {
                "context_hash" => r.context_hash = hash(bad),
                "prompt_template_hash" => r.prompt_template_hash = hash(bad),
                _ => r.schema_hash = hash(bad),
            }
            assert!(
                matches!(r.validate(), Err(InferenceError::NonGenericHash { field: f, .. }) if f == field),
                "{field}"
            );
            let mut wire = request_value();
            wire[field] = json!(bad);
            assert!(serde_json::from_value::<InferenceRequest>(wire).is_err());
        }
    }
    let mut r = request();
    r.id = hash(&semantic);
    assert!(matches!(
        r.validate(),
        Err(InferenceError::NonGenericHash { field: "id", .. })
    ));
}

#[test]
fn wrong_supplied_id_and_unknown_fields_are_rejected() {
    let mut wire = request_value();
    wire["id"] = json!(H1);
    let err = serde_json::from_value::<InferenceRequest>(wire).unwrap_err();
    assert!(
        err.to_string().contains("does not match recomputed id"),
        "{err}"
    );
    let mut r = request();
    r.task_kind = "extract-terms".to_owned();
    assert!(matches!(
        r.validate(),
        Err(InferenceError::RequestIdMismatch { .. })
    ));
    let mut wire = request_value();
    wire["priority"] = json!("high");
    assert!(serde_json::from_value::<InferenceRequest>(wire).is_err());
    let mut wire = request_value();
    wire["provider_policy"]["region"] = json!("eu");
    assert!(serde_json::from_value::<InferenceRequest>(wire).is_err());
    let mut wire = request_value();
    wire["stage"] = json!("s2");
    assert!(serde_json::from_value::<InferenceRequest>(wire).is_err());
}

#[test]
fn task_kind_rules() {
    for bad in ["", " extract", "extract ", "ex\ntract", "ex\u{7}tract"] {
        let err = InferenceRequest::new(
            StageId::S2,
            bad.to_owned(),
            vec![],
            vec![],
            hash(H1),
            hash(H2),
            hash(H3),
            policy("anthropic"),
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                InferenceError::InvalidText {
                    field: "task_kind",
                    ..
                }
            ),
            "{bad:?}"
        );
    }
    let exact = rebuilt(|r| r.task_kind = "Extract Requirements (v2)".to_owned());
    assert_eq!(exact.task_kind, "Extract Requirements (v2)");
}

#[test]
fn provider_policy_contract() {
    for good in [
        "anthropic",
        "openai",
        "azure-openai",
        "bedrock",
        "vertex",
        "openai-compatible",
        "local",
        "null",
        "mock",
        "a.b_c-1",
    ] {
        let p = policy(good);
        assert!(p.validate().is_ok(), "{good}");
        let wire = json!({"provider": good, "config": {}});
        assert_eq!(
            serde_json::from_value::<ProviderPolicy>(wire)
                .unwrap()
                .provider,
            good
        );
    }
    for bad in ["", "Anthropic", "1local", "-x", "open ai", "openai/", "é"] {
        assert!(matches!(
            policy(bad).validate(),
            Err(InferenceError::InvalidProvider(_))
        ));
        assert!(
            serde_json::from_value::<ProviderPolicy>(json!({"provider": bad, "config": {}}))
                .is_err()
        );
    }
    for config in [json!([]), json!(null), json!("x"), json!(1)] {
        let p = ProviderPolicy {
            provider: "local".to_owned(),
            config: CanonicalJson::new(config.clone()),
        };
        assert!(matches!(
            p.validate(),
            Err(InferenceError::ProviderConfigNotObject)
        ));
        assert!(serde_json::from_value::<ProviderPolicy>(
            json!({"provider": "local", "config": config})
        )
        .is_err());
    }
    assert!(serde_json::from_value::<ProviderPolicy>(json!({"provider": "local"})).is_err());
    assert!(InferenceRequest::new(
        StageId::S0,
        "segment".to_owned(),
        vec![],
        vec![],
        hash(H1),
        hash(H2),
        hash(H3),
        policy("Bad"),
    )
    .is_err());
}

// ---------------------------------------------------------------------------- artifacts

#[test]
fn inference_artifact_fixed_json_round_trip() {
    let r = request();
    let artifact = execution(&r).artifact;
    assert_eq!(artifact.raw_response_hash.as_str(), GOLDEN_RAW_HASH);
    assert_eq!(artifact.validated_output_hash.as_str(), GOLDEN_OUTPUT_HASH);
    assert_eq!(
        to_canonical_json(&artifact).unwrap(),
        GOLDEN_ARTIFACT_JSON.as_bytes()
    );
    let parsed: InferenceArtifact = serde_json::from_str(GOLDEN_ARTIFACT_JSON).unwrap();
    assert_eq!(parsed, artifact);
    assert_eq!(parsed.validate_for(&r).ok(), Some(()));
}

#[test]
fn inference_artifact_invariants() {
    let r = request();
    let good = execution(&r).artifact;

    let mut a = good.clone();
    a.request_hash = hash(H1);
    assert!(matches!(
        a.validate_for(&r),
        Err(InferenceError::RequestHashMismatch { .. })
    ));
    let mut a = good.clone();
    a.provider = "openai".to_owned();
    assert!(matches!(
        a.validate_for(&r),
        Err(InferenceError::ProviderMismatch { .. })
    ));
    let mut a = good.clone();
    a.provider = "Anthropic".to_owned();
    assert!(matches!(
        a.validate(),
        Err(InferenceError::InvalidProvider(_))
    ));
    for bad in ["", " claude", "claude ", "claude\nopus"] {
        let mut a = good.clone();
        a.model = bad.to_owned();
        assert!(matches!(
            a.validate(),
            Err(InferenceError::InvalidText { field: "model", .. })
        ));
    }
    let mut a = good.clone();
    a.validated_output_hash = hash(H1);
    assert!(matches!(
        a.validate(),
        Err(InferenceError::ValidatedOutputHashMismatch { .. })
    ));
    for field in ["request_hash", "raw_response_hash", "validated_output_hash"] {
        let mut wire = artifact_value();
        wire[field] = json!(format!("ev:sha256:{}", "a".repeat(64)));
        assert!(
            serde_json::from_value::<InferenceArtifact>(wire).is_err(),
            "{field}"
        );
        let mut a = good.clone();
        let bad = hash(&format!("psg:sha256:{}", "a".repeat(64)));
        match field {
            "request_hash" => a.request_hash = bad,
            "raw_response_hash" => a.raw_response_hash = bad,
            _ => a.validated_output_hash = bad,
        }
        assert!(matches!(
            a.validate(),
            Err(InferenceError::NonGenericHash { field: f, .. }) if f == field
        ));
    }
    let mut wire = artifact_value();
    wire["validated_output_hash"] = json!(H1);
    assert!(serde_json::from_value::<InferenceArtifact>(wire).is_err());
    let mut wire = artifact_value();
    wire["confidence"] = json!(0.9);
    assert!(serde_json::from_value::<InferenceArtifact>(wire).is_err());
    let mut wire = artifact_value();
    wire["model"] = json!(" claude");
    assert!(serde_json::from_value::<InferenceArtifact>(wire).is_err());
}

#[test]
fn provider_execution_contract() {
    let r = request();
    let good = execution(&r);
    assert_eq!(good.validate_for(&r).ok(), Some(()));
    let mut e = good.clone();
    e.raw_response.push(b' ');
    assert!(matches!(
        e.validate(),
        Err(InferenceError::RawResponseHashMismatch { .. })
    ));
    for bad in ["", "application/\njson"] {
        let mut e = good.clone();
        e.raw_response_media_type = bad.to_owned();
        assert!(matches!(
            e.validate(),
            Err(InferenceError::InvalidText {
                field: "raw_response_media_type",
                ..
            })
        ));
    }
    let mut e = good.clone();
    e.artifact.provider = "openai".to_owned();
    assert!(matches!(
        e.validate_for(&r),
        Err(InferenceError::ProviderMismatch { .. })
    ));
}

// ---------------------------------------------------------------------------- providers

#[test]
fn null_provider_is_always_disabled() {
    for provider in ["anthropic", "null", "mock", "local"] {
        let r = rebuilt(|r| r.provider_policy = policy(provider));
        assert!(matches!(
            NullProvider.execute(&r),
            Err(ProviderError::ProviderDisabled)
        ));
    }
    // Even an invalid request is not inspected.
    let mut r = request();
    r.task_kind = String::new();
    assert!(matches!(
        NullProvider.execute(&r),
        Err(ProviderError::ProviderDisabled)
    ));
}

#[test]
fn mock_provider_replays_exact_fixtures() {
    let r = request();
    let fixture = execution(&r);
    let mock = MockProvider::with_fixtures([(r.id.clone(), fixture.clone())]).unwrap();
    // The request names anthropic; the replay keeps the recorded provider and model.
    assert_eq!(r.provider_policy.provider, "anthropic");
    let first = mock.execute(&r).unwrap();
    assert_eq!(first, fixture);
    assert_eq!(first.artifact.provider, "anthropic");
    assert_eq!(first.artifact.model, "claude-opus-5-5");
    assert_eq!(first.raw_response, RAW_RESPONSE);
    assert_eq!(
        to_canonical_json(&first.artifact).unwrap(),
        GOLDEN_ARTIFACT_JSON.as_bytes()
    );
    assert_eq!(mock.execute(&r).unwrap(), first);

    let other = rebuilt(|r| r.task_kind = "extract-terms".to_owned());
    assert!(matches!(
        mock.execute(&other),
        Err(ProviderError::FixtureNotFound(h)) if h == other.id
    ));
    let mut invalid = request();
    invalid.task_kind = "changed".to_owned();
    assert!(matches!(
        mock.execute(&invalid),
        Err(ProviderError::InvalidRequest(_))
    ));
}

#[test]
fn mock_provider_registration_is_validated() {
    let r = request();
    let fixture = execution(&r);
    assert!(matches!(
        MockProvider::with_fixtures([(hash(H1), fixture.clone())]),
        Err(ProviderError::FixtureRequestMismatch { .. })
    ));
    let semantic = hash(&format!("psg:sha256:{}", "b".repeat(64)));
    assert!(matches!(
        MockProvider::with_fixtures([(semantic, fixture.clone())]),
        Err(ProviderError::FixtureKeyNotGeneric(_))
    ));
    let mut broken = fixture.clone();
    broken.raw_response = b"other".to_vec();
    assert!(matches!(
        MockProvider::with_fixtures([(r.id.clone(), broken)]),
        Err(ProviderError::InvalidFixture(_))
    ));
    let mut mock = MockProvider::new();
    mock.register(r.id.clone(), fixture.clone()).unwrap();
    assert!(matches!(
        mock.register(r.id.clone(), fixture),
        Err(ProviderError::DuplicateFixture(_))
    ));
}

// ---------------------------------------------------------------------------- persistence

#[test]
fn inference_bundle_persists_exact_artifacts_and_replays() {
    let r = request();
    let e = execution(&r);
    let mut store = SqliteArtifactStore::open_in_memory().unwrap();
    let refs = persist_inference_bundle(&mut store, &r, &e, ts(T1)).unwrap();
    assert_eq!(refs, persisted());
    assert_ne!(refs.request_artifact_ref, r.id);
    assert_ne!(
        refs.validated_inference_ref,
        e.artifact.validated_output_hash
    );

    let request_artifact = store.get(&refs.request_artifact_ref).unwrap();
    assert_eq!(request_artifact.kind, ArtifactKind::InferenceRequest);
    assert_eq!(request_artifact.media_type, "application/json");
    assert_eq!(request_artifact.bytes, GOLDEN_REQUEST_JSON.as_bytes());
    assert_eq!(request_artifact.created_at, ts(T1));

    let raw = store.get(&refs.raw_response_ref).unwrap();
    assert_eq!(raw.kind, ArtifactKind::InferenceResponse);
    assert_eq!(raw.media_type, "application/json");
    assert_eq!(raw.bytes, RAW_RESPONSE);
    assert_eq!(raw.hash, e.artifact.raw_response_hash);

    let validated = store.get(&refs.validated_inference_ref).unwrap();
    assert_eq!(validated.kind, ArtifactKind::ValidatedInference);
    assert_eq!(validated.media_type, "application/json");
    assert_eq!(validated.bytes, GOLDEN_ARTIFACT_JSON.as_bytes());

    // Replay from the persisted validated-inference artifact alone.
    let replayed: InferenceArtifact = serde_json::from_slice(&validated.bytes).unwrap();
    assert_eq!(replayed, e.artifact);
    assert_eq!(replayed.validated_output_hash.as_str(), GOLDEN_OUTPUT_HASH);
    assert_eq!(
        replayed.validated_output.content_hash().unwrap(),
        replayed.validated_output_hash
    );
    let persisted_request: InferenceRequest =
        serde_json::from_slice(&request_artifact.bytes).unwrap();
    let mock = MockProvider::with_fixtures([(
        persisted_request.id.clone(),
        ProviderExecution {
            artifact: replayed.clone(),
            raw_response_media_type: raw.media_type.clone(),
            raw_response: raw.bytes.clone(),
        },
    )])
    .unwrap();
    let again = mock.execute(&persisted_request).unwrap();
    assert_eq!(again.artifact, e.artifact);
    assert_eq!(again.artifact.validated_output, output());

    // Retrying the same bundle is idempotent.
    assert_eq!(
        persist_inference_bundle(&mut store, &r, &e, ts(T2)).unwrap(),
        refs
    );
    assert_eq!(
        store.get(&refs.request_artifact_ref).unwrap().created_at,
        ts(T1)
    );
}

#[test]
fn invalid_bundles_are_rejected_before_persistence() {
    let r = request();
    let mut e = execution(&r);
    e.artifact.provider = "openai".to_owned();
    let mut store = SqliteArtifactStore::open_in_memory().unwrap();
    assert!(matches!(
        persist_inference_bundle(&mut store, &r, &e, ts(T1)),
        Err(InferenceError::ProviderMismatch { .. })
    ));
    assert!(!store.exists(&hash(GOLDEN_REQUEST_REF)).unwrap());
}

// ---------------------------------------------------------------------------- provenance

#[test]
fn derivation_record_materialization_is_deterministic_and_exact() {
    let r = request();
    let e = execution(&r);
    let record = materialize_derivation_record(&r, &e, &persisted(), outputs(), ts(T1)).unwrap();
    assert_eq!(record.id.as_str(), GOLDEN_DRV_ID);
    assert_eq!(record.kind, DerivationKind::LlmInference);
    assert_eq!(record.stage, "S2");
    let mut expected_inputs = vec![
        "req:HR-001",
        "src:0123456789abcdef",
        "evd:0123456789abcdef",
        GOLDEN_REQUEST_ID,
        GOLDEN_REQUEST_REF,
        GOLDEN_RAW_HASH,
        GOLDEN_VALIDATED_REF,
    ];
    expected_inputs.sort();
    assert_eq!(record.input_refs, expected_inputs);
    assert_eq!(record.output_refs, vec!["prop:hr-001", "prop:hr-002"]);
    assert_eq!(record.created_at, ts(T1));
    assert_eq!(record.provider.as_deref(), Some("anthropic"));
    assert_eq!(record.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(record.prompt_template_hash, Some(hash(H2)));
    assert_eq!(record.schema_hash, Some(hash(H3)));
    assert_eq!(record.context_hash, Some(hash(H1)));
    assert_eq!(
        record.parameters,
        Some(json!({"max_tokens": 4096, "temperature": 0}))
    );
    assert_eq!(record.raw_response_hash, Some(hash(GOLDEN_RAW_HASH)));
    assert_eq!(record.validated_output_hash, Some(hash(GOLDEN_OUTPUT_HASH)));
    assert_eq!(record.validate(), Ok(()));

    let again = materialize_derivation_record(&r, &e, &persisted(), outputs(), ts(T1)).unwrap();
    assert_eq!(
        to_canonical_json(&again).unwrap(),
        to_canonical_json(&record).unwrap()
    );
    let later = materialize_derivation_record(&r, &e, &persisted(), outputs(), ts(T2)).unwrap();
    assert_ne!(later.id, record.id);
    let other_output = materialize_derivation_record(
        &r,
        &e,
        &persisted(),
        vec!["prop:hr-001".to_owned(), "prop:hr-003".to_owned()],
        ts(T1),
    )
    .unwrap();
    assert_ne!(other_output.id, record.id);
    // The record round-trips as a PSG payload.
    let back: DerivationRecord =
        serde_json::from_slice(&to_canonical_json(&record).unwrap()).unwrap();
    assert_eq!(back, record);
}

#[test]
fn derivation_record_inputs_are_validated() {
    let r = request();
    let e = execution(&r);
    assert!(matches!(
        materialize_derivation_record(
            &r,
            &e,
            &persisted(),
            vec!["prop:hr-001".to_owned(), "prop:hr-001".to_owned()],
            ts(T1)
        ),
        Err(InferenceError::DuplicateOutputRef(v)) if v == "prop:hr-001"
    ));
    let mut refs = persisted();
    refs.raw_response_ref = hash(H1);
    assert!(matches!(
        materialize_derivation_record(&r, &e, &refs, outputs(), ts(T1)),
        Err(InferenceError::PersistedRawResponseMismatch { .. })
    ));
    let mut refs = persisted();
    refs.validated_inference_ref = hash(&format!("ev:sha256:{}", "c".repeat(64)));
    assert!(matches!(
        materialize_derivation_record(&r, &e, &refs, outputs(), ts(T1)),
        Err(InferenceError::NonGenericHash {
            field: "validated_inference_ref",
            ..
        })
    ));
    let mut wrong = e.clone();
    wrong.artifact.request_hash = hash(H1);
    assert!(matches!(
        materialize_derivation_record(&r, &wrong, &persisted(), outputs(), ts(T1)),
        Err(InferenceError::RequestHashMismatch { .. })
    ));
}

#[test]
fn psg_provenance_types_are_reused_not_duplicated() {
    let agent = Agent {
        agent_kind: AgentKind::LlmModel,
    };
    let psg_agent: plumb_psg::Agent = agent.clone();
    assert_eq!(psg_agent, agent);
    let record: plumb_psg::DerivationRecord = materialize_derivation_record(
        &request(),
        &execution(&request()),
        &persisted(),
        outputs(),
        ts(T1),
    )
    .unwrap();
    assert_eq!(record.kind, plumb_psg::DerivationKind::LlmInference);
    for (name, source) in [
        ("model.rs", include_str!("../src/model.rs")),
        ("provider.rs", include_str!("../src/provider.rs")),
        ("null.rs", include_str!("../src/null.rs")),
        ("mock.rs", include_str!("../src/mock.rs")),
        ("lib.rs", include_str!("../src/lib.rs")),
    ] {
        assert!(!source.contains("struct Agent"), "{name}");
        assert!(!source.contains("struct DerivationRecord"), "{name}");
        assert!(!source.contains("enum StageId"), "{name}");
        assert!(!source.contains("struct CanonicalJson"), "{name}");
    }
}

// ---------------------------------------------------------------------------- structure

#[test]
fn inference_crate_has_no_store_dependency_and_providers_no_capabilities() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("plumb-store"));
    assert!(!manifest.contains("plumb-patch"));
    for (name, source) in [
        ("provider.rs", include_str!("../src/provider.rs")),
        ("null.rs", include_str!("../src/null.rs")),
        ("mock.rs", include_str!("../src/mock.rs")),
    ] {
        let code: String = source
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "Graph",
            "SqliteRevisionStore",
            "BranchName",
            "PatchSet",
            "commit",
            "ArtifactStore",
            "plumb_store",
            "plumb_patch",
            "plumb_artifacts",
        ] {
            assert!(!code.contains(forbidden), "{name} mentions {forbidden}");
        }
    }
}

// ---------------------------------------------------------------------------- Hotfix 015

#[test]
fn persisted_inference_refs_deserialization_enforces_generic_hashes() {
    let wire = serde_json::to_value(persisted()).unwrap();
    assert_eq!(
        wire,
        json!({
            "request_artifact_ref": GOLDEN_REQUEST_REF,
            "raw_response_ref": GOLDEN_RAW_HASH,
            "validated_inference_ref": GOLDEN_VALIDATED_REF
        })
    );
    assert_eq!(
        serde_json::from_value::<PersistedInferenceRefs>(wire.clone()).unwrap(),
        persisted()
    );
    for field in [
        "request_artifact_ref",
        "raw_response_ref",
        "validated_inference_ref",
    ] {
        for prefix in ["psg:sha256:", "ev:sha256:"] {
            let mut bad = wire.clone();
            bad[field] = json!(format!("{prefix}{}", "a".repeat(64)));
            let err = serde_json::from_value::<PersistedInferenceRefs>(bad).unwrap_err();
            assert!(err.to_string().contains(field), "{field} {prefix}: {err}");
        }
    }
    let mut extra = wire;
    extra["request_id"] = json!(GOLDEN_REQUEST_ID);
    assert!(serde_json::from_value::<PersistedInferenceRefs>(extra).is_err());
}
