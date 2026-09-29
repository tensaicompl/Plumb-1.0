//! F0.10 contract tests: scope/context, external validation, acquired artifact inputs, stage
//! plans and evaluations, a deterministic fixture stage, and compile-run records.
//!
//! Golden hashes were calculated independently from the literal canonical JSON below.

use std::collections::BTreeSet;

use plumb_artifacts::{Artifact, ArtifactKind, ArtifactStore, SqliteArtifactStore};
use plumb_compiler::*;
use plumb_core::{to_canonical_json, CanonicalJson, Hash, HashKind, Id, StageId, Timestamp};
use plumb_inference::{InferenceArtifact, InferenceRequest, ProviderPolicy};
use plumb_patch::{
    AcceptancePolicy, ElementPrecondition, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{node_element_hash, Edge, ElementStatus, Graph, Node, NodeType};
use plumb_store::{GraphRevision, LoadedRevision, RevisionId, PSG_SCHEMA_VERSION};
use serde_json::{json, Value};

const T1: &str = "2026-09-29T12:00:00.000000000Z";
const T2: &str = "2026-09-30T12:00:00.000000000Z";
const H2: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const H3: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const PROJECT: &str = "project:leave-management";
const PROFILE: &str = "profile:plumb-software-2026.1";

const GOLDEN_CONFIG_HASH: &str =
    "sha256:5359f0a81f1e876e87ffe35cffc12b819a044a962a9c542f5c975ca9c0e72e19";
const GOLDEN_EVR_JSON: &str = r#"{"config":{},"id":"sha256:2dbd3ea3b1c8233a76edf9e42cccc62fa2b46c79eacd47695d2d2a20075e9832","input_artifact_refs":["sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"],"task_kind":"lint","validator":"req-lint"}"#;
const GOLDEN_EVR_ID: &str =
    "sha256:2dbd3ea3b1c8233a76edf9e42cccc62fa2b46c79eacd47695d2d2a20075e9832";
const GOLDEN_EVA_JSON: &str = r#"{"request_hash":"sha256:2dbd3ea3b1c8233a76edf9e42cccc62fa2b46c79eacd47695d2d2a20075e9832","validated_output":{"ok":true},"validated_output_hash":"sha256:4062edaf750fb8074e7e83e0c9028c94e32468a8b6f1614774328ef045150f93","validator":"req-lint"}"#;
const GOLDEN_PLAN_JSON: &str = r#"{"deterministic_artifacts":[{"bytes":[114,101,113,58,72,82,45,48,48,49,10,114,101,113,58,72,82,45,48,48,50],"kind":"projection","media_type":"text/plain"}],"external_validation_requests":[{"config":{},"id":"sha256:a9c573dbd7e8f1e520ffc4495127cee9b11ac1980c881667eb7714738e5504e4","input_artifact_refs":["sha256:5e3f2173ac22d5887e5a98e35497943052ce23aacc841b941c576eaf6108d2dc"],"task_kind":"lint","validator":"req-lint"}],"inference_requests":[{"context_hash":"sha256:5359f0a81f1e876e87ffe35cffc12b819a044a962a9c542f5c975ca9c0e72e19","evidence_refs":[],"id":"sha256:be95c0d694a87c8ec11d63ec39ae3b9ff4703d61169d970cf2bf89023b5a9ac8","input_refs":["req:HR-001","req:HR-002"],"prompt_template_hash":"sha256:2222222222222222222222222222222222222222222222222222222222222222","provider_policy":{"config":{},"provider":"anthropic"},"schema_hash":"sha256:3333333333333333333333333333333333333333333333333333333333333333","stage":"S1","task_kind":"extract-requirements"}],"scope":{"kind":"elements","refs":["req:HR-001","req:HR-002"]}}"#;
const GOLDEN_EVALUATION_JSON: &str = r#"{"derivation_patch_set":{"base_semantic_hash":"psg:sha256:f0626043428dce3e421bf2d11665e55153c7aa9fc440b3e9bffc6feff72f5a19","patch":{"node":{"audit":{"created_at":"2026-09-29T12:00:00.000000000Z","created_by":"actor:analyst","updated_at":null,"updated_by":null},"derivations":[],"evidence":[],"extensions":{},"id":"req:derived-1","payload":{"data":{"level":"system","modality":"shall","owner_refs":null,"priority":null,"rationale":null,"requirement_kind":"functional","source_identifier":null,"stakeholder_refs":null,"statement":"The system shall list pending leave requests.","title":null,"verification_method":null},"type":"Requirement"},"revision":1,"standards":[],"status":"Proposed","tags":[]},"op":"AddNode"}},"proposals":[{"acceptance_policy":"HUMAN_CONFIRM","confidence":0.5,"derivation_refs":[],"evidence_refs":[],"id":"prop:0d5d15377e36d268","materiality":"semantic","patch_set":{"base_semantic_hash":"psg:sha256:f0626043428dce3e421bf2d11665e55153c7aa9fc440b3e9bffc6feff72f5a19","patch":{"from":"Proposed","op":"SetStatus","target":{"expected_hash":"sha256:90f45d530c518d64d026bf4403d66a2960cc948dba0cf87236a3f81c9adac9dc","id":"req:HR-002"},"to":"Accepted"}},"stage":"S1"}]}"#;
const GOLDEN_OUTPUT_HASH: &str =
    "sha256:8bb7f125d05d5f73f07ad68e35776326f66eeb63c7f6f8b2ffcd42fc47057800";
const GOLDEN_PATCH_REF: &str =
    "sha256:8a295052a8c014fcc4a9e22b6a7dc85ccbab918beb619576d8a950406b277948";
const GOLDEN_PROPOSAL_REF: &str =
    "sha256:eb3766ea6a0cb9134ecbaf9d640820c5a3d550fb5382a498b485ac6aa2aec9b2";
const GOLDEN_RUN_JSON: &str = r#"{"compiler_version":"0.1.0","config_hash":"sha256:5359f0a81f1e876e87ffe35cffc12b819a044a962a9c542f5c975ca9c0e72e19","deterministic_artifacts":["sha256:5e3f2173ac22d5887e5a98e35497943052ce23aacc841b941c576eaf6108d2dc"],"id":"sha256:b637565847dc075546408e622a5a275f906f9aff37d6c4260844a1d7f706d79e","inference_artifacts":["sha256:8ef935059bf2a60b8511a711b7bd45de2596c9ea14dfbf3af105b0812842cb75"],"input_revision":"rev:1:f0626043428dce3e","input_semantic_hash":"psg:sha256:f0626043428dce3e421bf2d11665e55153c7aa9fc440b3e9bffc6feff72f5a19","output_derivation_patch_ref":"sha256:8a295052a8c014fcc4a9e22b6a7dc85ccbab918beb619576d8a950406b277948","output_hash":"sha256:8bb7f125d05d5f73f07ad68e35776326f66eeb63c7f6f8b2ffcd42fc47057800","output_proposal_refs":["sha256:eb3766ea6a0cb9134ecbaf9d640820c5a3d550fb5382a498b485ac6aa2aec9b2"],"profile_hash":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","profile_ref":"profile:plumb-software-2026.1","rule_pack_hash":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","stage":"S1","validation_artifacts":["sha256:e91b8f1e2d1ad54a7033c3a34182a72183d1a9544829674d69d67e2ff0fdd34f"]}"#;
const GOLDEN_RUN_ID: &str =
    "sha256:b637565847dc075546408e622a5a275f906f9aff37d6c4260844a1d7f706d79e";
const GOLDEN_RUN_REF: &str =
    "sha256:ed296500cab8d84c5ef4a52109c57680fc0e1c6f6121fda77021629b8dd65d7b";

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

fn generic(c: char) -> Hash {
    hash(&format!("sha256:{}", c.to_string().repeat(64)))
}

fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{e}: {value}"))
}

fn audit() -> Value {
    json!({"created_by": "actor:analyst", "created_at": T1, "updated_by": null, "updated_at": null})
}

fn requirement(node_id: &str, status: &str, statement: &str) -> Node {
    from_json(json!({
        "id": node_id, "revision": 1, "status": status,
        "payload": {"type": "Requirement", "data": {
            "statement": statement, "requirement_kind": "functional", "level": "system",
            "modality": "shall", "title": null, "rationale": null, "priority": null,
            "source_identifier": null, "verification_method": null, "owner_refs": null,
            "stakeholder_refs": null
        }},
        "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
        "audit": audit()
    }))
}

fn constraint(node_id: &str) -> Node {
    from_json(json!({
        "id": node_id, "revision": 1, "status": "Accepted",
        "payload": {"type": "Constraint", "data": {
            "statement": "Leave data must stay in the EU.",
            "constraint_category": "regulatory", "strength": "mandatory"
        }},
        "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
        "audit": audit()
    }))
}

fn edge(edge_id: &str, from: &str, to: &str) -> Edge {
    from_json(json!({
        "id": edge_id, "revision": 1, "status": "Accepted", "kind": "constrained_by",
        "from": from, "to": to, "properties": {}, "evidence": [], "derivations": [],
        "standards": [], "audit": audit()
    }))
}

fn graph() -> Graph {
    Graph::new(
        id(PROJECT),
        id(PROFILE),
        vec![
            requirement(
                "req:HR-001",
                "Accepted",
                "Employees shall submit leave requests.",
            ),
            requirement(
                "req:HR-002",
                "Proposed",
                "Managers shall approve leave requests.",
            ),
            constraint("con:eu"),
        ],
        vec![edge("rel:hr-eu", "req:HR-001", "con:eu")],
    )
    .unwrap()
}

fn loaded() -> LoadedRevision {
    let graph = graph();
    let semantic_hash = graph.semantic_hash().unwrap();
    LoadedRevision {
        revision: GraphRevision {
            id: RevisionId::new(1, &semantic_hash).unwrap(),
            version: 1,
            parent: None,
            project_id: id(PROJECT),
            psg_schema_version: PSG_SCHEMA_VERSION,
            semantic_hash,
            evidence_hash: graph.evidence_hash().unwrap(),
            profile_ref: id(PROFILE),
            profile_hash: generic('a'),
            rule_pack_hash: generic('b'),
            accepted_patch_ref: None,
            decision_refs: vec![],
            created_by: id("actor:analyst"),
            created_at: ts(T1),
        },
        graph,
    }
}

fn ctx() -> CompileContext {
    CompileContext::from_loaded_revision(
        &loaded(),
        "0.1.0".to_owned(),
        CanonicalJson::new(json!({"mode": "strict"})),
    )
    .unwrap()
}

fn validation_request(refs: Vec<Hash>) -> ExternalValidationRequest {
    ExternalValidationRequest::new(
        "req-lint".to_owned(),
        "lint".to_owned(),
        refs,
        CanonicalJson::new(json!({})),
    )
    .unwrap()
}

fn validation_artifact(request: &ExternalValidationRequest) -> ExternalValidationArtifact {
    let output = CanonicalJson::new(json!({"ok": true}));
    ExternalValidationArtifact {
        request_hash: request.id.clone(),
        validator: request.validator.clone(),
        validated_output_hash: output.content_hash().unwrap(),
        validated_output: output,
    }
}

fn inference_artifact(request: &InferenceRequest) -> InferenceArtifact {
    let output = CanonicalJson::new(json!({"accept": ["req:HR-002"]}));
    InferenceArtifact {
        request_hash: request.id.clone(),
        provider: request.provider_policy.provider.clone(),
        model: "claude-opus-5-5".to_owned(),
        parameters: CanonicalJson::new(json!({})),
        raw_response_hash: Hash::content_sha256(b"raw"),
        validated_output_hash: output.content_hash().unwrap(),
        validated_output: output,
    }
}

fn input(kind: ArtifactKind, media_type: &str, bytes: Vec<u8>) -> ArtifactInput {
    ArtifactInput {
        hash: Hash::content_sha256(&bytes),
        kind,
        media_type: media_type.to_owned(),
        bytes,
    }
}

/// A test-only stage. PLAN derives everything from the graph and context; EVALUATE uses only
/// the graph, context, plan and acquired artifacts.
struct FixtureStage;

impl CompilerStage for FixtureStage {
    fn id(&self) -> StageId {
        StageId::S1
    }

    fn plan(&self, graph: &Graph, ctx: &CompileContext) -> Result<StagePlan, CompilerError> {
        ctx.validate_for_graph(graph)?;
        let requirements: Vec<Id> = graph
            .node_ids_by_type(NodeType::Requirement)
            .iter()
            .cloned()
            .collect();
        let listing = PlannedArtifact {
            kind: ArtifactKind::Projection,
            media_type: "text/plain".to_owned(),
            bytes: requirements
                .iter()
                .map(Id::as_str)
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
        };
        let inference = InferenceRequest::new(
            self.id(),
            "extract-requirements".to_owned(),
            requirements.clone(),
            vec![],
            ctx.config_hash()?,
            hash(H2),
            hash(H3),
            ProviderPolicy {
                provider: "anthropic".to_owned(),
                config: CanonicalJson::new(json!({})),
            },
        )
        .map_err(|e| CompilerError::StageFailure {
            stage: self.id(),
            code: "inference-request".to_owned(),
            message: e.to_string(),
        })?;
        let validation = validation_request(vec![listing.content_hash()]);
        StagePlan::new(
            Scope::elements(requirements)?,
            vec![listing],
            vec![inference],
            vec![validation],
        )
    }

    fn evaluate(
        &self,
        graph: &Graph,
        ctx: &CompileContext,
        plan: &StagePlan,
        artifacts: &ArtifactSet,
    ) -> Result<StageEvaluation, CompilerError> {
        ctx.validate_for_graph(graph)?;
        plan.validate(self.id(), graph)?;
        artifacts.validate_for_plan(plan)?;
        let inference: InferenceArtifact =
            serde_json::from_slice(&artifacts.inference_artifacts[0].bytes).unwrap();
        let base = ctx.input_semantic_hash.clone();
        let derivation = PatchSet {
            base_semantic_hash: base.clone(),
            patch: SemanticPatch::AddNode {
                node: requirement(
                    "req:derived-1",
                    "Proposed",
                    "The system shall list pending leave requests.",
                ),
            },
        };
        let proposals = inference.validated_output.as_value()["accept"]
            .as_array()
            .unwrap()
            .iter()
            .map(|target| {
                let node = graph.node(&id(target.as_str().unwrap())).unwrap();
                Proposal::new(
                    self.id(),
                    PatchSet {
                        base_semantic_hash: base.clone(),
                        patch: SemanticPatch::SetStatus {
                            target: ElementPrecondition {
                                id: node.id.clone(),
                                expected_hash: node_element_hash(node).unwrap(),
                            },
                            from: ElementStatus::Proposed,
                            to: ElementStatus::Accepted,
                        },
                    },
                    vec![],
                    vec![],
                    ProposalMateriality::Semantic,
                    AcceptancePolicy::HumanConfirm,
                    Some(0.5),
                )
                .unwrap()
            })
            .collect();
        StageEvaluation::new(Some(derivation), proposals)
    }
}

fn plan() -> StagePlan {
    FixtureStage.plan(&graph(), &ctx()).unwrap()
}

fn artifact_set(plan: &StagePlan) -> ArtifactSet {
    let listing = &plan.deterministic_artifacts[0];
    ArtifactSet::new(
        vec![input(
            listing.kind,
            &listing.media_type,
            listing.bytes.clone(),
        )],
        vec![input(
            ArtifactKind::ValidatedInference,
            JSON_MEDIA_TYPE,
            to_canonical_json(&inference_artifact(&plan.inference_requests[0])).unwrap(),
        )],
        vec![input(
            ArtifactKind::ExternalValidation,
            JSON_MEDIA_TYPE,
            to_canonical_json(&validation_artifact(&plan.external_validation_requests[0])).unwrap(),
        )],
    )
    .unwrap()
}

fn evaluation() -> StageEvaluation {
    let plan = plan();
    FixtureStage
        .evaluate(&graph(), &ctx(), &plan, &artifact_set(&plan))
        .unwrap()
}

fn run() -> CompileRun {
    let plan = plan();
    CompileRun::new(StageId::S1, &ctx(), &artifact_set(&plan), &evaluation()).unwrap()
}

fn canonical<T: serde::Serialize>(value: &T) -> String {
    String::from_utf8(to_canonical_json(value).unwrap()).unwrap()
}

fn invalid_kind(prefix: &str) -> Hash {
    hash(&format!("{prefix}{}", "e".repeat(64)))
}

// ---------------------------------------------------------------------------- Scope / context (§42)

#[test]
fn scope_wire_forms_and_rules() {
    assert_eq!(canonical(&Scope::Project), r#"{"kind":"project"}"#);
    let scope = Scope::elements(vec![id("req:2"), id("req:1")]).unwrap();
    assert_eq!(
        canonical(&scope),
        r#"{"kind":"elements","refs":["req:1","req:2"]}"#
    );
    assert_eq!(
        serde_json::from_str::<Scope>(r#"{"kind":"project"}"#).unwrap(),
        Scope::Project
    );
    assert_eq!(
        serde_json::from_str::<Scope>(r#"{"kind":"elements","refs":["req:1","req:2"]}"#).unwrap(),
        scope
    );
    assert!(matches!(
        Scope::elements(vec![id("req:1"), id("req:1")]),
        Err(CompilerError::InvalidScope(_))
    ));
    assert!(matches!(
        Scope::elements(vec![]),
        Err(CompilerError::InvalidScope(_))
    ));
    for bad in [
        r#"{"kind":"elements","refs":["req:2","req:1"]}"#,
        r#"{"kind":"elements","refs":["req:1","req:1"]}"#,
        r#"{"kind":"elements","refs":[]}"#,
        r#"{"kind":"Project"}"#,
        r#"{"kind":"project","refs":[]}"#,
        r#"{"kind":"elements"}"#,
        r#"{"kind":"graph"}"#,
    ] {
        assert!(serde_json::from_str::<Scope>(bad).is_err(), "{bad}");
    }
    let g = graph();
    Scope::elements(vec![id("req:HR-001"), id("rel:hr-eu")])
        .unwrap()
        .validate_for_graph(&g)
        .unwrap();
    Scope::Project.validate_for_graph(&g).unwrap();
    assert!(matches!(
        Scope::elements(vec![id("req:HR-999")])
            .unwrap()
            .validate_for_graph(&g),
        Err(CompilerError::InvalidScope(_))
    ));
}

#[test]
fn compile_context_comes_exactly_from_the_loaded_revision() {
    let l = loaded();
    let c = ctx();
    assert_eq!(c.input_revision, l.revision.id);
    assert_eq!(c.input_semantic_hash, l.revision.semantic_hash);
    assert_eq!(c.profile_ref, l.revision.profile_ref);
    assert_eq!(c.profile_hash, l.revision.profile_hash);
    assert_eq!(c.rule_pack_hash, l.revision.rule_pack_hash);
    assert_eq!(c.config_hash().unwrap().as_str(), GOLDEN_CONFIG_HASH);
    assert_eq!(
        c.config_hash().unwrap(),
        Hash::content_sha256(br#"{"mode":"strict"}"#)
    );
    c.validate_for_graph(&l.graph).unwrap();
    // Wire form has exactly the seven fields and no timestamp.
    let wire = serde_json::to_value(&c).unwrap();
    let keys: BTreeSet<&str> = wire
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "compiler_version",
            "config",
            "input_revision",
            "input_semantic_hash",
            "profile_hash",
            "profile_ref",
            "rule_pack_hash"
        ])
    );
    assert_eq!(
        serde_json::from_value::<CompileContext>(wire.clone()).unwrap(),
        c
    );
    let mut extra = wire;
    extra["created_at"] = json!(T1);
    assert!(serde_json::from_value::<CompileContext>(extra).is_err());
}

#[test]
fn compile_context_rejects_drift_and_invalid_fields() {
    let c = ctx();
    let other = Graph::new(id(PROJECT), id(PROFILE), vec![constraint("con:x")], vec![]).unwrap();
    assert!(matches!(
        c.validate_for_graph(&other),
        Err(CompilerError::InvalidContext(_))
    ));
    let other_profile = Graph::new(
        id(PROJECT),
        id("profile:other"),
        graph().nodes().values().cloned().collect(),
        graph().edges().values().cloned().collect(),
    )
    .unwrap();
    assert!(matches!(
        c.validate_for_graph(&other_profile),
        Err(CompilerError::InvalidContext(_))
    ));
    for config in [json!([]), json!("x"), json!(null)] {
        let err = CompileContext::from_loaded_revision(
            &loaded(),
            "0.1.0".to_owned(),
            CanonicalJson::new(config),
        )
        .unwrap_err();
        assert!(matches!(err, CompilerError::InvalidContext(_)));
    }
    for version in ["", " 0.1.0", "0.1.0 ", "0.1\n.0"] {
        let err = CompileContext::from_loaded_revision(
            &loaded(),
            version.to_owned(),
            CanonicalJson::new(json!({})),
        )
        .unwrap_err();
        assert!(
            matches!(err, CompilerError::InvalidContext(_)),
            "{version:?}"
        );
    }
    let mut bad = c.clone();
    bad.input_semantic_hash = generic('a');
    assert!(matches!(
        bad.validate(),
        Err(CompilerError::InvalidContext(_))
    ));
    let mut bad = c.clone();
    bad.profile_hash = invalid_kind("ev:sha256:");
    assert!(matches!(
        bad.validate(),
        Err(CompilerError::InvalidContext(_))
    ));
    let mut bad = c;
    bad.rule_pack_hash = invalid_kind("psg:sha256:");
    assert!(matches!(
        bad.validate(),
        Err(CompilerError::InvalidContext(_))
    ));
}

// ---------------------------------------------------------------------------- external validation (§43)

#[test]
fn external_validation_request_golden_and_identity() {
    let r = validation_request(vec![generic('d'), generic('c')]);
    assert_eq!(r.id.as_str(), GOLDEN_EVR_ID);
    assert_eq!(canonical(&r), GOLDEN_EVR_JSON);
    assert_eq!(r.input_artifact_refs, vec![generic('c'), generic('d')]);
    assert_eq!(
        serde_json::from_str::<ExternalValidationRequest>(GOLDEN_EVR_JSON).unwrap(),
        r
    );
    let projection = json!({
        "validator": "req-lint", "task_kind": "lint",
        "input_artifact_refs": [generic('c'), generic('d')], "config": {}
    });
    assert_eq!(
        r.recompute_id().unwrap(),
        Hash::content_sha256(&to_canonical_json(&projection).unwrap())
    );
    let refs = vec![generic('c'), generic('d')];
    let variants = [
        ExternalValidationRequest::new(
            "openapi-lint".into(),
            "lint".into(),
            refs.clone(),
            CanonicalJson::new(json!({})),
        ),
        ExternalValidationRequest::new(
            "req-lint".into(),
            "check".into(),
            refs.clone(),
            CanonicalJson::new(json!({})),
        ),
        ExternalValidationRequest::new(
            "req-lint".into(),
            "lint".into(),
            vec![generic('c')],
            CanonicalJson::new(json!({})),
        ),
        ExternalValidationRequest::new(
            "req-lint".into(),
            "lint".into(),
            refs,
            CanonicalJson::new(json!({"strict": true})),
        ),
    ];
    let ids: BTreeSet<Hash> = variants
        .into_iter()
        .map(|v| v.unwrap().id)
        .chain([r.id])
        .collect();
    assert_eq!(ids.len(), 5);
}

#[test]
fn external_validation_request_rejections() {
    let new = |validator: &str, task: &str, refs: Vec<Hash>, config: Value| {
        ExternalValidationRequest::new(
            validator.into(),
            task.into(),
            refs,
            CanonicalJson::new(config),
        )
    };
    let is_invalid = |r: Result<ExternalValidationRequest, CompilerError>| {
        matches!(r, Err(CompilerError::InvalidExternalValidationRequest(_)))
    };
    assert!(is_invalid(new(
        "req-lint",
        "lint",
        vec![generic('c'), generic('c')],
        json!({})
    )));
    assert!(is_invalid(new(
        "req-lint",
        "lint",
        vec![invalid_kind("psg:sha256:")],
        json!({})
    )));
    assert!(is_invalid(new("Req-Lint", "lint", vec![], json!({}))));
    assert!(is_invalid(new("req lint", "lint", vec![], json!({}))));
    assert!(is_invalid(new("req-lint", " lint", vec![], json!({}))));
    assert!(is_invalid(new("req-lint", "", vec![], json!({}))));
    assert!(is_invalid(new("req-lint", "lint", vec![], json!([]))));
    let wire: Value = serde_json::from_str(GOLDEN_EVR_JSON).unwrap();
    for (field, value) in [
        ("input_artifact_refs", json!([generic('d'), generic('c')])),
        ("input_artifact_refs", json!([generic('c'), generic('c')])),
        ("id", json!(generic('f'))),
        ("validator", json!("Req")),
        ("config", json!(1)),
        ("extra", json!(true)),
    ] {
        let mut w = wire.clone();
        w[field] = value;
        assert!(
            serde_json::from_value::<ExternalValidationRequest>(w).is_err(),
            "{field}"
        );
    }
}

#[test]
fn external_validation_artifact_golden_and_checks() {
    let r = validation_request(vec![generic('c'), generic('d')]);
    let a = validation_artifact(&r);
    assert_eq!(canonical(&a), GOLDEN_EVA_JSON);
    assert_eq!(
        serde_json::from_str::<ExternalValidationArtifact>(GOLDEN_EVA_JSON).unwrap(),
        a
    );
    a.validate_for(&r).unwrap();
    let invalid = |r: Result<(), CompilerError>| {
        matches!(r, Err(CompilerError::InvalidExternalValidationArtifact(_)))
    };
    let mut bad = a.clone();
    bad.validated_output_hash = generic('f');
    assert!(invalid(bad.validate()));
    let mut bad = a.clone();
    bad.request_hash = generic('f');
    assert!(invalid(bad.validate_for(&r)));
    let mut bad = a.clone();
    bad.validator = "other-lint".into();
    assert!(invalid(bad.validate_for(&r)));
    let mut bad = a.clone();
    bad.request_hash = invalid_kind("ev:sha256:");
    assert!(invalid(bad.validate()));
    let wire: Value = serde_json::from_str(GOLDEN_EVA_JSON).unwrap();
    for (field, value) in [
        ("validated_output_hash", json!(generic('f'))),
        ("validator", json!("Bad Name")),
        ("extra", json!(1)),
    ] {
        let mut w = wire.clone();
        w[field] = value;
        assert!(
            serde_json::from_value::<ExternalValidationArtifact>(w).is_err(),
            "{field}"
        );
    }
}

// ---------------------------------------------------------------------------- ArtifactInput / ArtifactSet (§44)

#[test]
fn artifact_input_drops_created_at() {
    let bytes = b"req:HR-001".to_vec();
    let stored = |at: &str| Artifact {
        hash: Hash::content_sha256(&bytes),
        kind: ArtifactKind::Projection,
        media_type: "text/plain".into(),
        bytes: bytes.clone(),
        created_at: ts(at),
    };
    let a = ArtifactInput::from(&stored(T1));
    let b = ArtifactInput::from(&stored(T2));
    assert_eq!(a, b);
    a.validate().unwrap();
    let mut bad = a.clone();
    bad.bytes.push(b'!');
    assert!(matches!(
        bad.validate(),
        Err(CompilerError::InvalidArtifactInput(_))
    ));
    let mut bad = a.clone();
    bad.hash = invalid_kind("psg:sha256:");
    assert!(matches!(
        bad.validate(),
        Err(CompilerError::InvalidArtifactInput(_))
    ));
    let mut bad = a;
    bad.media_type = String::new();
    assert!(matches!(
        bad.validate(),
        Err(CompilerError::InvalidArtifactInput(_))
    ));
}

#[test]
fn artifact_set_categories_sorting_and_uniqueness() {
    let p = plan();
    let set = artifact_set(&p);
    set.validate_for_plan(&p).unwrap();
    let x = input(ArtifactKind::Projection, "text/plain", b"x".to_vec());
    let y = input(ArtifactKind::Projection, "text/plain", b"y".to_vec());
    let sorted = ArtifactSet::new(vec![y.clone(), x.clone()], vec![], vec![]).unwrap();
    let mut expected = vec![x.clone(), y];
    expected.sort_by(|a, b| a.hash.cmp(&b.hash));
    assert_eq!(sorted.deterministic_artifacts, expected);
    assert!(matches!(
        ArtifactSet::new(vec![x.clone(), x.clone()], vec![], vec![]),
        Err(CompilerError::InvalidArtifactSet(_))
    ));
    // The same hash in two lists.
    let v = set.validation_artifacts[0].clone();
    let mut as_deterministic = v.clone();
    as_deterministic.kind = ArtifactKind::ExternalValidation;
    assert!(matches!(
        ArtifactSet::new(vec![as_deterministic], vec![], vec![v.clone()]),
        Err(CompilerError::InvalidArtifactSet(_))
    ));
    // Category kind/media rules.
    assert!(ArtifactSet::new(vec![], vec![x.clone()], vec![]).is_err());
    assert!(ArtifactSet::new(vec![], vec![], vec![x]).is_err());
    let mut wrong_media = set.inference_artifacts[0].clone();
    wrong_media.media_type = "text/plain".into();
    assert!(ArtifactSet::new(vec![], vec![wrong_media], vec![]).is_err());
    assert!(ArtifactSet::new(vec![], vec![v], vec![]).is_err());
    // Unsorted direct construction is rejected by validate.
    let raw = ArtifactSet {
        deterministic_artifacts: {
            let mut l = expected;
            l.reverse();
            l
        },
        inference_artifacts: vec![],
        validation_artifacts: vec![],
    };
    assert!(matches!(
        raw.validate(),
        Err(CompilerError::InvalidArtifactSet(_))
    ));
}

#[test]
fn artifact_set_must_match_the_plan_exactly() {
    let p = plan();
    let good = artifact_set(&p);
    let rejects = |s: ArtifactSet| {
        assert!(matches!(
            s.validate_for_plan(&p),
            Err(CompilerError::InvalidArtifactSet(_))
        ));
    };
    // Missing planned artifact.
    rejects(ArtifactSet {
        deterministic_artifacts: vec![],
        ..good.clone()
    });
    // Extra deterministic artifact.
    let extra = input(ArtifactKind::Projection, "text/plain", b"extra".to_vec());
    rejects(
        ArtifactSet::new(
            vec![good.deterministic_artifacts[0].clone(), extra],
            good.inference_artifacts.clone(),
            good.validation_artifacts.clone(),
        )
        .unwrap(),
    );
    // Same bytes, different kind or media type.
    let mut wrong_kind = good.deterministic_artifacts[0].clone();
    wrong_kind.kind = ArtifactKind::Diff;
    rejects(ArtifactSet {
        deterministic_artifacts: vec![wrong_kind],
        ..good.clone()
    });
    let mut wrong_media = good.deterministic_artifacts[0].clone();
    wrong_media.media_type = "text/markdown".into();
    rejects(ArtifactSet {
        deterministic_artifacts: vec![wrong_media],
        ..good.clone()
    });
    // Wrong inference result (another request's artifact).
    let other_request = InferenceRequest::new(
        StageId::S1,
        "other".into(),
        vec![],
        vec![],
        generic('1'),
        hash(H2),
        hash(H3),
        ProviderPolicy {
            provider: "anthropic".into(),
            config: CanonicalJson::new(json!({})),
        },
    )
    .unwrap();
    rejects(ArtifactSet {
        inference_artifacts: vec![input(
            ArtifactKind::ValidatedInference,
            JSON_MEDIA_TYPE,
            to_canonical_json(&inference_artifact(&other_request)).unwrap(),
        )],
        ..good.clone()
    });
    // Noncanonical inference bytes for the right request.
    let pretty = serde_json::to_vec_pretty(&inference_artifact(&p.inference_requests[0])).unwrap();
    rejects(ArtifactSet {
        inference_artifacts: vec![input(
            ArtifactKind::ValidatedInference,
            JSON_MEDIA_TYPE,
            pretty,
        )],
        ..good.clone()
    });
    // Missing inference result.
    rejects(ArtifactSet {
        inference_artifacts: vec![],
        ..good.clone()
    });
    // Wrong validation result.
    let other_validation = validation_request(vec![generic('c')]);
    rejects(ArtifactSet {
        validation_artifacts: vec![input(
            ArtifactKind::ExternalValidation,
            JSON_MEDIA_TYPE,
            to_canonical_json(&validation_artifact(&other_validation)).unwrap(),
        )],
        ..good.clone()
    });
    rejects(ArtifactSet {
        validation_artifacts: vec![],
        ..good.clone()
    });
    good.validate_for_plan(&p).unwrap();
}

// ---------------------------------------------------------------------------- StagePlan (§45)

#[test]
fn stage_plan_golden_ordering_and_rules() {
    let p = plan();
    assert_eq!(canonical(&p), GOLDEN_PLAN_JSON);
    assert_eq!(
        serde_json::from_str::<StagePlan>(GOLDEN_PLAN_JSON).unwrap(),
        p
    );
    p.validate(StageId::S1, &graph()).unwrap();
    let a = PlannedArtifact {
        kind: ArtifactKind::Projection,
        media_type: "text/plain".into(),
        bytes: b"a".to_vec(),
    };
    let b = PlannedArtifact {
        kind: ArtifactKind::Projection,
        media_type: "text/plain".into(),
        bytes: b"b".to_vec(),
    };
    let v1 = validation_request(vec![generic('c')]);
    let v2 = validation_request(vec![generic('d')]);
    let forward = StagePlan::new(
        Scope::Project,
        vec![a.clone(), b.clone()],
        p.inference_requests.clone(),
        vec![v1.clone(), v2.clone()],
    )
    .unwrap();
    let reversed = StagePlan::new(
        Scope::Project,
        vec![b, a.clone()],
        p.inference_requests.clone(),
        vec![v2, v1.clone()],
    )
    .unwrap();
    assert_eq!(canonical(&forward), canonical(&reversed));
    assert!(matches!(
        StagePlan::new(Scope::Project, vec![a.clone(), a.clone()], vec![], vec![]),
        Err(CompilerError::InvalidStagePlan(_))
    ));
    assert!(StagePlan::new(Scope::Project, vec![], vec![], vec![v1.clone(), v1]).is_err());
    let r = p.inference_requests[0].clone();
    assert!(StagePlan::new(Scope::Project, vec![], vec![r.clone(), r], vec![]).is_err());
    assert!(matches!(
        StagePlan::new(
            Scope::Project,
            vec![PlannedArtifact {
                media_type: String::new(),
                ..a
            }],
            vec![],
            vec![]
        ),
        Err(CompilerError::InvalidPlannedArtifact(_))
    ));
    // Inference request for another stage.
    assert!(matches!(
        p.validate(StageId::S2, &graph()),
        Err(CompilerError::InvalidStagePlan(_))
    ));
    // Unsorted wire input is rejected.
    let mut wire: Value = serde_json::to_value(&forward).unwrap();
    let list = wire["deterministic_artifacts"].as_array_mut().unwrap();
    list.reverse();
    assert!(serde_json::from_value::<StagePlan>(wire).is_err());
    // No timestamps and no capabilities in the wire form.
    for forbidden in ["created_at", "timestamp", "provider_client", "store"] {
        assert!(!GOLDEN_PLAN_JSON.contains(forbidden), "{forbidden}");
    }
}

// ---------------------------------------------------------------------------- StageEvaluation (§46)

#[test]
fn stage_evaluation_golden_and_rules() {
    let e = evaluation();
    assert_eq!(canonical(&e), GOLDEN_EVALUATION_JSON);
    assert_eq!(
        serde_json::from_str::<StageEvaluation>(GOLDEN_EVALUATION_JSON).unwrap(),
        e
    );
    let keys: BTreeSet<String> = serde_json::to_value(&e)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from(["derivation_patch_set".to_owned(), "proposals".to_owned()])
    );
    for forbidden in ["findings", "questions", "impact", "intake"] {
        assert!(!keys.contains(forbidden));
    }
    let c = ctx();
    e.validate(StageId::S1, &c.input_semantic_hash).unwrap();
    // Wrong base for the derivation patch set.
    let mut bad = e.clone();
    bad.derivation_patch_set
        .as_mut()
        .unwrap()
        .base_semantic_hash = hash(&format!("psg:sha256:{}", "9".repeat(64)));
    assert!(matches!(
        bad.validate(StageId::S1, &c.input_semantic_hash),
        Err(CompilerError::InvalidStageEvaluation(_))
    ));
    // Proposal for another stage.
    assert!(matches!(
        e.validate(StageId::S2, &c.input_semantic_hash),
        Err(CompilerError::InvalidStageEvaluation(_))
    ));
    // Proposal with another base.
    let p = &e.proposals[0];
    let moved = Proposal::new(
        StageId::S1,
        PatchSet {
            base_semantic_hash: hash(&format!("psg:sha256:{}", "9".repeat(64))),
            patch: p.patch_set.patch.clone(),
        },
        vec![],
        vec![],
        p.materiality,
        p.acceptance_policy,
        p.confidence,
    )
    .unwrap();
    let bad = StageEvaluation::new(None, vec![moved]).unwrap();
    assert!(matches!(
        bad.validate(StageId::S1, &c.input_semantic_hash),
        Err(CompilerError::InvalidStageEvaluation(_))
    ));
    // Sorting and duplicates.
    let second = Proposal::new(
        StageId::S1,
        p.patch_set.clone(),
        vec![],
        vec![],
        p.materiality,
        AcceptancePolicy::HumanDecision,
        None,
    )
    .unwrap();
    let sorted = StageEvaluation::new(None, vec![second.clone(), p.clone()]).unwrap();
    let mut ids = vec![second.id.clone(), p.id.clone()];
    ids.sort();
    assert_eq!(
        sorted
            .proposals
            .iter()
            .map(|x| x.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    assert!(matches!(
        StageEvaluation::new(None, vec![p.clone(), p.clone()]),
        Err(CompilerError::InvalidStageEvaluation(_))
    ));
    let mut wire = serde_json::to_value(&sorted).unwrap();
    wire["proposals"].as_array_mut().unwrap().reverse();
    assert!(serde_json::from_value::<StageEvaluation>(wire).is_err());
    let mut wire = serde_json::to_value(&e).unwrap();
    wire["findings"] = json!([]);
    assert!(serde_json::from_value::<StageEvaluation>(wire).is_err());
    assert_eq!(canonical(&evaluation()), canonical(&e));
}

// ---------------------------------------------------------------------------- fixture stage (§47, §50)

#[test]
fn fixture_stage_is_deterministic_and_ignores_acquisition_time() {
    let stage = FixtureStage;
    let (g, c) = (graph(), ctx());
    let p1 = stage.plan(&g, &c).unwrap();
    let p2 = stage.plan(&g, &c).unwrap();
    assert_eq!(
        to_canonical_json(&p1).unwrap(),
        to_canonical_json(&p2).unwrap()
    );

    // Acquire the same artifacts at two different times and convert them for evaluation.
    let acquired_at = |at: &str| {
        let mut store = SqliteArtifactStore::open_in_memory().unwrap();
        let set = artifact_set(&p1);
        let convert =
            |list: &[ArtifactInput], store: &mut SqliteArtifactStore| -> Vec<ArtifactInput> {
                list.iter()
                    .map(|a| {
                        let h = store.put(a.kind, &a.media_type, &a.bytes, ts(at)).unwrap();
                        let stored = store.get(&h).unwrap();
                        assert_eq!(stored.created_at, ts(at));
                        ArtifactInput::from(&stored)
                    })
                    .collect()
            };
        ArtifactSet::new(
            convert(&set.deterministic_artifacts, &mut store),
            convert(&set.inference_artifacts, &mut store),
            convert(&set.validation_artifacts, &mut store),
        )
        .unwrap()
    };
    let (early, late) = (acquired_at(T1), acquired_at(T2));
    assert_eq!(early, late);
    let e1 = stage.evaluate(&g, &c, &p1, &early).unwrap();
    let e2 = stage.evaluate(&g, &c, &p2, &late).unwrap();
    assert_eq!(
        to_canonical_json(&e1).unwrap(),
        to_canonical_json(&e2).unwrap()
    );
    assert_eq!(
        CompileRun::new(StageId::S1, &c, &early, &e1).unwrap(),
        CompileRun::new(StageId::S1, &c, &late, &e2).unwrap()
    );
}

#[test]
fn compiler_stage_trait_exposes_no_capabilities() {
    let source = include_str!("../src/stage.rs");
    let start = source.find("pub trait CompilerStage").unwrap();
    let end = start + source[start..].find("\n}\n").unwrap();
    let trait_src = &source[start..end];
    for forbidden in [
        "InferenceProvider",
        "ArtifactStore",
        "SqliteArtifactStore",
        "SqliteRevisionStore",
        "BranchName",
        "Clock",
        "reqwest",
        "Client",
        "http",
        "commit",
        "Timestamp",
    ] {
        assert!(!trait_src.contains(forbidden), "trait mentions {forbidden}");
    }
    for (name, src) in [
        ("stage.rs", source),
        ("context.rs", include_str!("../src/context.rs")),
    ] {
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // The error type `ArtifactStoreError` is allowed; a store capability is not.
        let code = code.replace("ArtifactStoreError", "");
        for forbidden in [
            "ArtifactStore",
            "SqliteRevisionStore",
            "InferenceProvider",
            "Clock",
            "reqwest",
            "Timestamp",
        ] {
            assert!(!code.contains(forbidden), "{name} mentions {forbidden}");
        }
    }
}

// ---------------------------------------------------------------------------- CompileRun (§48)

#[test]
fn compile_run_golden() {
    let r = run();
    let e = evaluation();
    let p = plan();
    let set = artifact_set(&p);
    let c = ctx();
    assert_eq!(canonical(&r), GOLDEN_RUN_JSON);
    assert_eq!(r.id.as_str(), GOLDEN_RUN_ID);
    assert_eq!(r.output_hash.as_str(), GOLDEN_OUTPUT_HASH);
    assert_eq!(
        r.output_hash,
        Hash::content_sha256(GOLDEN_EVALUATION_JSON.as_bytes())
    );
    assert_eq!(r.output_hash, e.output_hash().unwrap());
    assert_eq!(r.config_hash.as_str(), GOLDEN_CONFIG_HASH);
    assert_eq!(
        r.output_derivation_patch_ref.as_ref().unwrap().as_str(),
        GOLDEN_PATCH_REF
    );
    assert_eq!(r.output_proposal_refs, vec![hash(GOLDEN_PROPOSAL_REF)]);
    assert_eq!(
        hash(GOLDEN_PATCH_REF),
        Hash::content_sha256(&to_canonical_json(e.derivation_patch_set.as_ref().unwrap()).unwrap())
    );
    assert_eq!(
        hash(GOLDEN_PROPOSAL_REF),
        Hash::content_sha256(&to_canonical_json(&e.proposals[0]).unwrap())
    );
    assert_eq!(r.stage, StageId::S1);
    assert_eq!(r.input_revision, c.input_revision);
    assert_eq!(r.input_semantic_hash, c.input_semantic_hash);
    assert_eq!(r.profile_ref, c.profile_ref);
    assert_eq!(r.profile_hash, c.profile_hash);
    assert_eq!(r.rule_pack_hash, c.rule_pack_hash);
    assert_eq!(r.compiler_version, c.compiler_version);
    let hashes = |l: &[ArtifactInput]| l.iter().map(|a| a.hash.clone()).collect::<Vec<_>>();
    assert_eq!(
        r.deterministic_artifacts,
        hashes(&set.deterministic_artifacts)
    );
    assert_eq!(r.inference_artifacts, hashes(&set.inference_artifacts));
    assert_eq!(r.validation_artifacts, hashes(&set.validation_artifacts));
    assert_eq!(
        r.deterministic_artifacts,
        vec![Hash::content_sha256(b"req:HR-001\nreq:HR-002")]
    );
    assert_eq!(r.input_semantic_hash.kind(), HashKind::Semantic);
    for h in [
        &r.id,
        &r.profile_hash,
        &r.rule_pack_hash,
        &r.config_hash,
        &r.output_hash,
    ]
    .into_iter()
    .chain(&r.deterministic_artifacts)
    .chain(&r.inference_artifacts)
    .chain(&r.validation_artifacts)
    .chain(&r.output_proposal_refs)
    .chain(&r.output_derivation_patch_ref)
    {
        assert_eq!(h.kind(), HashKind::Generic);
    }
    let mut body: Value = serde_json::from_str(GOLDEN_RUN_JSON).unwrap();
    body.as_object_mut().unwrap().remove("id");
    assert_eq!(
        r.recompute_id().unwrap(),
        Hash::content_sha256(&to_canonical_json(&body).unwrap())
    );
    assert_eq!(
        serde_json::from_str::<CompileRun>(GOLDEN_RUN_JSON).unwrap(),
        r
    );
    assert!(!GOLDEN_RUN_JSON.contains("created_at"));
    assert!(!GOLDEN_RUN_JSON.contains("finding"));
    // run.id and the run artifact ref are distinct hashes.
    assert_eq!(
        Hash::content_sha256(GOLDEN_RUN_JSON.as_bytes()).as_str(),
        GOLDEN_RUN_REF
    );
    assert_ne!(GOLDEN_RUN_ID, GOLDEN_RUN_REF);
}

#[test]
fn compile_run_changes_and_rejections() {
    let base = run();
    let p = plan();
    let set = artifact_set(&p);
    let e = evaluation();
    // Changing the evaluation changes output_hash and id.
    let fewer = StageEvaluation::new(e.derivation_patch_set.clone(), vec![]).unwrap();
    let r2 = CompileRun::new(StageId::S1, &ctx(), &set, &fewer).unwrap();
    assert_ne!(r2.output_hash, base.output_hash);
    assert_ne!(r2.id, base.id);
    assert_eq!(r2.output_proposal_refs, Vec::<Hash>::new());
    let no_patch = StageEvaluation::new(None, e.proposals.clone()).unwrap();
    assert_eq!(
        CompileRun::new(StageId::S1, &ctx(), &set, &no_patch)
            .unwrap()
            .output_derivation_patch_ref,
        None
    );
    // Changing the config changes the id.
    let other_ctx = CompileContext::from_loaded_revision(
        &loaded(),
        "0.1.0".into(),
        CanonicalJson::new(json!({"mode": "lenient"})),
    )
    .unwrap();
    let r3 = CompileRun::new(StageId::S1, &other_ctx, &set, &e).unwrap();
    assert_ne!(r3.config_hash, base.config_hash);
    assert_ne!(r3.id, base.id);
    // Changing an input artifact ref changes the id.
    let extra = input(ArtifactKind::Projection, "text/plain", b"extra".to_vec());
    let more = ArtifactSet::new(
        vec![set.deterministic_artifacts[0].clone(), extra],
        set.inference_artifacts.clone(),
        set.validation_artifacts.clone(),
    )
    .unwrap();
    let r4 = CompileRun::new(StageId::S1, &ctx(), &more, &e).unwrap();
    assert_ne!(r4.id, base.id);
    assert_eq!(r4.deterministic_artifacts.len(), 2);
    // Evaluation for another stage is rejected.
    assert!(matches!(
        CompileRun::new(StageId::S2, &ctx(), &set, &e),
        Err(CompilerError::InvalidStageEvaluation(_))
    ));
    // Wrong supplied id, bad hash kinds, unsorted refs and unknown fields.
    let wire: Value = serde_json::from_str(GOLDEN_RUN_JSON).unwrap();
    for (field, value) in [
        ("id", json!(generic('f'))),
        ("output_hash", json!(generic('f'))),
        ("input_semantic_hash", json!(generic('f'))),
        (
            "config_hash",
            json!(format!("ev:sha256:{}", "a".repeat(64))),
        ),
        (
            "output_proposal_refs",
            json!([GOLDEN_PROPOSAL_REF, GOLDEN_PROPOSAL_REF]),
        ),
        ("output_finding_refs", json!([])),
        ("created_at", json!(T1)),
    ] {
        let mut w = wire.clone();
        w[field] = value;
        assert!(serde_json::from_value::<CompileRun>(w).is_err(), "{field}");
    }
    let mut bad = base.clone();
    bad.deterministic_artifacts = vec![generic('d'), generic('c')];
    assert!(matches!(
        bad.validate(),
        Err(CompilerError::InvalidCompileRun(_))
    ));
    let mut bad = base;
    bad.id = generic('f');
    assert!(matches!(
        bad.validate(),
        Err(CompilerError::InvalidCompileRun(_))
    ));
}

// ---------------------------------------------------------------------------- persistence (§49)

#[test]
fn compile_run_persistence() {
    let r = run();
    let e = evaluation();
    let mut store = SqliteArtifactStore::open_in_memory().unwrap();
    let persisted = persist_compile_run(&mut store, &r, &e, ts(T1)).unwrap();
    assert_eq!(persisted.run, r);
    assert_eq!(persisted.run_artifact_ref.as_str(), GOLDEN_RUN_REF);
    assert_ne!(persisted.run_artifact_ref, r.id);

    let patch = store.get(&hash(GOLDEN_PATCH_REF)).unwrap();
    assert_eq!(patch.kind, ArtifactKind::Patch);
    assert_eq!(patch.media_type, JSON_MEDIA_TYPE);
    assert_eq!(
        patch.bytes,
        to_canonical_json(e.derivation_patch_set.as_ref().unwrap()).unwrap()
    );
    let proposal = store.get(&hash(GOLDEN_PROPOSAL_REF)).unwrap();
    assert_eq!(proposal.kind, ArtifactKind::Proposal);
    assert_eq!(proposal.media_type, JSON_MEDIA_TYPE);
    assert_eq!(proposal.bytes, to_canonical_json(&e.proposals[0]).unwrap());
    assert!(r.output_proposal_refs.contains(&proposal.hash));
    let stored_run = store.get(&persisted.run_artifact_ref).unwrap();
    assert_eq!(stored_run.kind, ArtifactKind::CompileRun);
    assert_eq!(stored_run.media_type, JSON_MEDIA_TYPE);
    assert_eq!(stored_run.bytes, GOLDEN_RUN_JSON.as_bytes());
    let reloaded: CompileRun = serde_json::from_slice(&stored_run.bytes).unwrap();
    reloaded.validate().unwrap();
    assert_eq!(reloaded, r);

    // Retry is idempotent and keeps the first created_at; created_at changes no bytes or hash.
    let again = persist_compile_run(&mut store, &r, &e, ts(T2)).unwrap();
    assert_eq!(again, persisted);
    assert_eq!(
        store.get(&persisted.run_artifact_ref).unwrap().created_at,
        ts(T1)
    );
    let mut other = SqliteArtifactStore::open_in_memory().unwrap();
    let later = persist_compile_run(&mut other, &r, &e, ts(T2)).unwrap();
    assert_eq!(later.run_artifact_ref, persisted.run_artifact_ref);
    assert_eq!(
        other.get(&later.run_artifact_ref).unwrap().bytes,
        stored_run.bytes
    );
}

#[test]
fn compile_run_persistence_validates_before_writing() {
    let r = run();
    let e = evaluation();
    let mut store = SqliteArtifactStore::open_in_memory().unwrap();
    let fewer = StageEvaluation::new(e.derivation_patch_set.clone(), vec![]).unwrap();
    assert!(matches!(
        persist_compile_run(&mut store, &r, &fewer, ts(T1)),
        Err(CompilerError::InvalidCompileRun(_))
    ));
    let mut tampered = r.clone();
    tampered.output_proposal_refs = vec![];
    assert!(persist_compile_run(&mut store, &tampered, &e, ts(T1)).is_err());
    assert!(!store.exists(&hash(GOLDEN_PATCH_REF)).unwrap());
    assert!(!store.exists(&hash(GOLDEN_RUN_REF)).unwrap());
}
