//! F0.13 contract tests for the gate evaluator framework: policy, context, the rule evaluation
//! contract, finding identity, governed waivers, gate aggregation, registration and the
//! deterministic gate report.
//!
//! The rule metadata is the real supplied profile; every evaluator here is a test-only fixture.
//! Every test lives in `evaluator_contract` so that `cargo test -p plumb-validation evaluator`
//! selects them.

mod evaluator_contract {
    use std::collections::BTreeSet;

    use plumb_core::{to_canonical_json, CanonicalJson, GateId, Hash, HashKind, Id};
    use plumb_psg::{Edge, FindingSeverity, Graph, Node};
    use plumb_validation::*;
    use serde_json::{json, Value};

    const SUPPLIED: &str =
        include_str!("../../../config/profiles/plumb-software-2026.1-rules.yaml");

    const T1: &str = "2026-09-29T08:00:00.000000000Z";
    const T2: &str = "2031-01-01T00:00:00.000000000Z";
    const PROJECT: &str = "project:leave-management";
    const PROFILE: &str = "profile:plumb-software-2026.1";

    const PROFILE_HASH: &str =
        "sha256:08b5b1535eedf401dd002c26ffc7301cfc35a119680f75b664de691b41c3a52f";
    const RULE_PACK_HASH: &str =
        "sha256:6677e02dd7740bb294533513a4d5294576c62c65eb8606973ca998b565f1b2e7";

    // The six I0 rules of the supplied profile, in rule ID order.
    const HASHABLE: &str = "PLUMB.I0.BASELINE.HASHABLE";
    const HASH_MATCH: &str = "PLUMB.I0.EVIDENCE.HASH_MATCH";
    const LOCATABLE: &str = "PLUMB.I0.EVIDENCE.LOCATABLE";
    /// blocker, waiver forbidden
    const CONTENT_ADDRESSED: &str = "PLUMB.I0.SOURCE.CONTENT_ADDRESSED";
    /// blocker, waiver decision_required
    const PARSE_STATUS: &str = "PLUMB.I0.SOURCE.PARSE_STATUS";
    /// error, waiver profile_allow, has a standard reference
    const AGENT_IDENTIFIED: &str = "PPMN.I0.PROVENANCE.AGENT_IDENTIFIED";
    const I0_RULES: [&str; 6] = [
        HASHABLE,
        HASH_MATCH,
        LOCATABLE,
        CONTENT_ADDRESSED,
        PARSE_STATUS,
        AGENT_IDENTIFIED,
    ];

    // ---- Goldens calculated outside this crate (Python: hashlib.sha256 over the literal bytes
    // ---- and over json.dumps(sort_keys=True, separators=(",", ":")) canonical JSON).

    /// `finding_key("PLUMB.F1.REQ.GROUNDED", [req:HR-001, req:HR-002], "missing-evidence")`.
    const GOLDEN_KEY_BYTES: &[u8] =
        b"PLUMB.F1.REQ.GROUNDED\x00req:HR-001\x00req:HR-002\x00missing-evidence";
    const GOLDEN_KEY: &str =
        "sha256:16a2811655c60d0c4391c92f6e7a1f23ad79d050231792ee2f01f078cea21457";
    const GOLDEN_FINDING_ID: &str = "fnd:16a2811655c60d0c";

    const EMPTY_POLICY_JSON: &str =
        r#"{"profile_allow_waiver_rule_ids":[],"promoted_to_blocker_rule_ids":[]}"#;
    const EMPTY_POLICY_HASH: &str =
        "sha256:c4c5e711282810d61c3fe57b8024ae2f7537bb3f7f25ac710e7988fe97790952";
    const GOLDEN_POLICY_HASH: &str =
        "sha256:1fe3ac45f9434c572b10fe238b8118f59a331479c1e2e1352aa659cfd925f6c9";

    /// The `req-lint` / `lint` / no inputs / `{}` request answered with `{"ok":true}`.
    const GOLDEN_ARTIFACT_REF: &str =
        "sha256:3f55607117f194b351c7dcf035c17dc52b7883953eb70dd47b4a59f5cc970f8a";

    /// The PARSE_STATUS violation of the golden scenario: target `src:hr-policy`, condition
    /// `parse-failed`.
    const GOLDEN_WAIVED_KEY: &str =
        "sha256:b175e0ef856728ae1e8e4cb5466ee7d9a2edb836359710969ad4aa5c802e28a3";
    const GOLDEN_WAIVED_FINDING: &str = "fnd:b175e0ef856728ae";

    /// Semantic hash of `golden_graph()`, produced by plumb-psg.
    const GOLDEN_BASELINE: &str =
        "psg:sha256:25787df602640a7532f8d51e223c73cd27df6d9b0cdd7e20e46235ff0ab751b9";
    const GOLDEN_REPORT: &str = r#"{"baseline_semantic_hash":"psg:sha256:25787df602640a7532f8d51e223c73cd27df6d9b0cdd7e20e46235ff0ab751b9","findings":[{"id":"fnd:6326de72ad5dfba1","key":"sha256:6326de72ad5dfba1735f6064ffccd44ae82643c5e1f64a5b171dad0a982b9742","payload":{"affected_refs":["src:hr-handbook","src:hr-policy"],"code":"PLUMB.I0.SOURCE.CONTENT_ADDRESSED","family":"I0","message":"Source artifacts lack a content hash.","severity":"blocker","standard_rule_ref":null,"status":"Open","suggested_resolution":null,"waiver_ref":null},"semantic_condition_key":"content-hash-missing"},{"id":"fnd:81baae8cb47adb41","key":"sha256:81baae8cb47adb410e1cc5b5bc6d08f07333e9be9dbdb1432bf9edd0b1f25b41","payload":{"affected_refs":["drv:import-1"],"code":"PPMN.I0.PROVENANCE.AGENT_IDENTIFIED","family":"I0","message":"Derivation has no agent.","severity":"blocker","standard_rule_ref":"PPMN.I0.PROVENANCE.AGENT_IDENTIFIED","status":"Open","suggested_resolution":null,"waiver_ref":null},"semantic_condition_key":"agent-missing"},{"id":"fnd:b175e0ef856728ae","key":"sha256:b175e0ef856728ae1e8e4cb5466ee7d9a2edb836359710969ad4aa5c802e28a3","payload":{"affected_refs":["src:hr-policy"],"code":"PLUMB.I0.SOURCE.PARSE_STATUS","family":"I0","message":"Source could not be parsed.","severity":"blocker","standard_rule_ref":null,"status":"Open","suggested_resolution":"Re-import the source.","waiver_ref":"dec:waive-parse"},"semantic_condition_key":"parse-failed"}],"gate":"I0","policy":{"profile_allow_waiver_rule_ids":["PPMN.I0.PROVENANCE.AGENT_IDENTIFIED"],"promoted_to_blocker_rule_ids":["PPMN.I0.PROVENANCE.AGENT_IDENTIFIED"]},"profile_hash":"sha256:08b5b1535eedf401dd002c26ffc7301cfc35a119680f75b664de691b41c3a52f","profile_id":"profile:plumb-software-2026.1","result":"FAIL","rule_pack_hash":"sha256:6677e02dd7740bb294533513a4d5294576c62c65eb8606973ca998b565f1b2e7","rules":[{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":["ev:manifest"],"finding_ref":null,"rule_id":"PLUMB.I0.BASELINE.HASHABLE","semantic_condition_key":null,"state":"PASS","targets":["src:hr-policy"],"waiver_ref":null},{"applicability":{"reason":"no evidence fragments in scope","state":"NOT_APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":[],"finding_ref":null,"rule_id":"PLUMB.I0.EVIDENCE.HASH_MATCH","semantic_condition_key":null,"state":"NOT_APPLICABLE","targets":[],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":{"code":"E_LOCATOR","evidence":[],"message":"locator index unavailable","targets":["frag:a1"]},"evidence":[],"finding_ref":null,"rule_id":"PLUMB.I0.EVIDENCE.LOCATABLE","semantic_condition_key":null,"state":"ERROR","targets":["frag:a1"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":["ev:manifest"],"finding_ref":"fnd:6326de72ad5dfba1","rule_id":"PLUMB.I0.SOURCE.CONTENT_ADDRESSED","semantic_condition_key":"content-hash-missing","state":"FAIL","targets":["src:hr-handbook","src:hr-policy"],"waiver_ref":null},{"applicability":{"state":"APPLICABLE"},"declared_severity":"blocker","effective_severity":"blocker","error":null,"evidence":[],"finding_ref":"fnd:b175e0ef856728ae","rule_id":"PLUMB.I0.SOURCE.PARSE_STATUS","semantic_condition_key":"parse-failed","state":"WAIVED","targets":["src:hr-policy"],"waiver_ref":"dec:waive-parse"},{"applicability":{"state":"APPLICABLE"},"declared_severity":"error","effective_severity":"blocker","error":null,"evidence":[],"finding_ref":"fnd:81baae8cb47adb41","rule_id":"PPMN.I0.PROVENANCE.AGENT_IDENTIFIED","semantic_condition_key":"agent-missing","state":"FAIL","targets":["drv:import-1"],"waiver_ref":null}],"summary":{"blocker_failed":3,"blocker_waived":1,"warnings":0},"validation_artifact_refs":["sha256:3f55607117f194b351c7dcf035c17dc52b7883953eb70dd47b4a59f5cc970f8a"],"waivers":[{"decision_ref":"dec:waive-parse","finding_key":"sha256:b175e0ef856728ae1e8e4cb5466ee7d9a2edb836359710969ad4aa5c802e28a3","finding_ref":"fnd:b175e0ef856728ae","rule_id":"PLUMB.I0.SOURCE.PARSE_STATUS"}]}"#;
    const GOLDEN_REPORT_HASH: &str =
        "sha256:2de2b756d60b5f4fac49118f48798d75a2dbcac12a9740eb783a4cfce05e579b";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ids(v: &[&str]) -> Vec<Id> {
        v.iter().map(|s| id(s)).collect()
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn canonical<T: serde::Serialize>(value: &T) -> String {
        String::from_utf8(to_canonical_json(value).unwrap()).unwrap()
    }

    fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{e}: {value}"))
    }

    fn audit() -> Value {
        json!({"created_by": "actor:analyst", "created_at": T1, "updated_by": null, "updated_at": null})
    }

    fn node(node_id: &str, status: &str, tag: &str, data: Value) -> Node {
        from_json(json!({
            "id": node_id, "revision": 1, "status": status,
            "payload": {"type": tag, "data": data},
            "evidence": [], "derivations": [], "standards": [], "tags": [], "extensions": {},
            "audit": audit()
        }))
    }

    fn requirement(node_id: &str) -> Node {
        node(
            node_id,
            "Accepted",
            "Requirement",
            json!({
                "statement": format!("The system shall {node_id}."), "requirement_kind": "functional",
                "level": "system", "modality": "shall"
            }),
        )
    }

    fn finding_node(node_id: &Id, status: &str) -> Node {
        node(
            node_id.as_str(),
            status,
            "Finding",
            json!({
                "code": PARSE_STATUS, "family": "I0", "severity": "blocker",
                "message": "Source could not be parsed.", "status": "Open",
                "affected_refs": ["src:hr-policy"], "standard_rule_ref": null,
                "suggested_resolution": null, "waiver_ref": null
            }),
        )
    }

    fn decision_at(node_id: &str, status: &str, answer: Value, rationale: Value, at: &str) -> Node {
        node(
            node_id,
            status,
            "ResolutionDecision",
            json!({
                "question_ref": null, "proposal_ref": "prop:0123456789abcdef", "answer": answer,
                "decided_by": "actor:compliance-owner", "decided_at": at,
                "patch_ref": format!("sha256:{}", "a".repeat(64)),
                "rationale": rationale, "supersedes": null
            }),
        )
    }

    fn decision(node_id: &str, status: &str, answer: Value) -> Node {
        decision_at(
            node_id,
            status,
            answer,
            json!("Legacy scan accepted until re-import."),
            T1,
        )
    }

    fn resolves(edge_id: &str, status: &str, from: &str, to: &Id) -> Edge {
        from_json(json!({
            "id": edge_id, "revision": 1, "status": status, "kind": "resolves",
            "from": from, "to": to,
            "properties": {}, "evidence": [], "derivations": [], "standards": [], "audit": audit()
        }))
    }

    fn build_graph(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        let mut all = vec![requirement("req:HR-001")];
        all.extend(nodes);
        Graph::new(id(PROJECT), id(PROFILE), all, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn plain_graph() -> Graph {
        build_graph(vec![], vec![])
    }

    fn marker(rule_id: &str, key: &Hash, finding: &Id) -> Value {
        json!({"kind": "waiver", "rule_id": rule_id, "finding_key": key, "finding_ref": finding})
    }

    fn metadata() -> ValidationRegistry {
        ValidationRegistry::new(load_profile_yaml(SUPPLIED).unwrap()).unwrap()
    }

    /// The supplied profile with one rule's declared severity changed.
    fn metadata_with_severity(rule_id: &str, severity: &str) -> ValidationRegistry {
        let mut value: serde_yaml::Value = serde_yaml::from_str(SUPPLIED).unwrap();
        let rule = value["rules"]
            .as_sequence_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["id"].as_str() == Some(rule_id))
            .unwrap();
        rule["severity"] = serde_yaml::Value::from(severity);
        let yaml = serde_yaml::to_string(&value).unwrap();
        ValidationRegistry::new(load_profile_yaml(&yaml).unwrap()).unwrap()
    }

    fn policy(promoted: &[&str], profile_allow: &[&str]) -> ValidationPolicy {
        ValidationPolicy {
            promoted_to_blocker_rule_ids: promoted.iter().map(|s| (*s).to_owned()).collect(),
            profile_allow_waiver_rule_ids: profile_allow.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn external_artifact(output: Value) -> ExternalValidationArtifact {
        let request = ExternalValidationRequest::new(
            "req-lint".to_owned(),
            "lint".to_owned(),
            vec![],
            CanonicalJson::new(json!({})),
        )
        .unwrap();
        let output = CanonicalJson::new(output);
        ExternalValidationArtifact {
            request_hash: request.id,
            validator: request.validator,
            validated_output_hash: output.content_hash().unwrap(),
            validated_output: output,
        }
    }

    fn context(
        graph: &Graph,
        registry: &ValidationRegistry,
        policy: ValidationPolicy,
    ) -> ValidationContext {
        ValidationContext::new(graph, registry, policy, vec![]).unwrap()
    }

    // ------------------------------------------------------------------ test-only fixture evaluators

    type Outcome = Result<RuleEvaluation, EvaluatorFailure>;

    fn ev_pass(_: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
        Ok(RuleEvaluation::pass(ids(&["src:hr-policy"]), strings(&["ev:manifest"])).unwrap())
    }

    fn ev_not_applicable(_: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
        Ok(RuleEvaluation::not_applicable("no evidence fragments in scope".to_owned()).unwrap())
    }

    const FIXTURE_CONDITION: &str = "fixture-condition";

    /// Violates the rule for `src:hr-policy` under the condition `fixture-condition`.
    fn ev_violation(_: &Graph, _: &ValidationContext, rule: &RuleMetadata) -> Outcome {
        Ok(RuleEvaluation::violation(
            ids(&["src:hr-policy"]),
            vec![],
            FIXTURE_CONDITION.to_owned(),
            format!("{} is violated.", rule.id),
            None,
        )
        .unwrap())
    }

    /// The same violation as [`ev_violation`] with a different message and resolution.
    fn ev_violation_reworded(_: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
        Ok(RuleEvaluation::violation(
            ids(&["src:hr-policy"]),
            strings(&["ev:manifest"]),
            FIXTURE_CONDITION.to_owned(),
            "Reworded.".to_owned(),
            Some("Fix the source.".to_owned()),
        )
        .unwrap())
    }

    fn ev_failure(_: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
        Err(EvaluatorFailure::new(
            "E_LOCATOR".to_owned(),
            "locator index unavailable".to_owned(),
            ids(&["frag:a1"]),
            vec![],
        )
        .unwrap())
    }

    fn ev_unsorted_targets(_: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
        Ok(RuleEvaluation::Pass {
            targets: ids(&["src:b", "src:a"]),
            evidence: vec![],
        })
    }

    fn ev_duplicate_evidence(_: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
        Ok(RuleEvaluation::Pass {
            targets: vec![],
            evidence: strings(&["ev:a", "ev:a"]),
        })
    }

    fn ev_padded_condition_key(_: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
        Ok(RuleEvaluation::Violation {
            targets: vec![],
            evidence: vec![],
            semantic_condition_key: " padded".to_owned(),
            message: "m".to_owned(),
            suggested_resolution: None,
        })
    }

    fn ev_empty_reason(_: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
        Ok(RuleEvaluation::NotApplicable {
            reason: String::new(),
        })
    }

    fn ev_malformed_failure(_: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
        Err(EvaluatorFailure {
            code: String::new(),
            message: "m".to_owned(),
            targets: vec![],
            evidence: vec![],
        })
    }

    /// The scripted evaluator of the golden scenario: one distinct outcome per I0 rule.
    fn ev_golden(graph: &Graph, ctx: &ValidationContext, rule: &RuleMetadata) -> Outcome {
        match rule.id.as_str() {
            HASHABLE => ev_pass(graph, ctx, rule),
            HASH_MATCH => Ok(RuleEvaluation::not_applicable(
                "no evidence fragments in scope".to_owned(),
            )
            .unwrap()),
            LOCATABLE => Err(EvaluatorFailure::new(
                "E_LOCATOR".to_owned(),
                "locator index unavailable".to_owned(),
                ids(&["frag:a1"]),
                vec![],
            )
            .unwrap()),
            CONTENT_ADDRESSED => Ok(RuleEvaluation::violation(
                ids(&["src:hr-policy", "src:hr-handbook"]),
                strings(&["ev:manifest"]),
                "content-hash-missing".to_owned(),
                "Source artifacts lack a content hash.".to_owned(),
                None,
            )
            .unwrap()),
            PARSE_STATUS => Ok(RuleEvaluation::violation(
                ids(&["src:hr-policy"]),
                vec![],
                "parse-failed".to_owned(),
                "Source could not be parsed.".to_owned(),
                Some("Re-import the source.".to_owned()),
            )
            .unwrap()),
            AGENT_IDENTIFIED => Ok(RuleEvaluation::violation(
                ids(&["drv:import-1"]),
                vec![],
                "agent-missing".to_owned(),
                "Derivation has no agent.".to_owned(),
                None,
            )
            .unwrap()),
            other => panic!("no golden outcome for {other}"),
        }
    }

    /// Binds `default` to every rule of `gate`, except the listed overrides.
    fn bind_gate(
        registry: &mut EvaluatorRegistry,
        gate: GateId,
        default: RuleEvaluator,
        overrides: &[(&str, RuleEvaluator)],
    ) {
        let rule_ids: Vec<String> = registry.metadata().gate(gate).rule_ids.clone();
        for rule_id in rule_ids {
            let evaluator = overrides
                .iter()
                .find(|(id, _)| *id == rule_id)
                .map_or(default, |(_, evaluator)| *evaluator);
            registry.register(&rule_id, evaluator).unwrap();
        }
    }

    /// I0 with every rule passing except the listed overrides.
    fn i0_registry(
        metadata: ValidationRegistry,
        overrides: &[(&str, RuleEvaluator)],
    ) -> EvaluatorRegistry {
        let mut registry = EvaluatorRegistry::new(metadata);
        bind_gate(&mut registry, GateId::I0, ev_pass, overrides);
        registry
    }

    /// Evaluates I0 over `graph` with every rule passing except the listed overrides.
    fn evaluate_i0(
        graph: &Graph,
        policy: ValidationPolicy,
        overrides: &[(&str, RuleEvaluator)],
    ) -> Result<GateReport, EvaluationError> {
        let registry = i0_registry(metadata(), overrides);
        let ctx = context(graph, registry.metadata(), policy);
        registry.evaluate_gate(GateId::I0, graph, &ctx)
    }

    fn rule_result<'a>(report: &'a GateReport, rule_id: &str) -> &'a RuleResult {
        report
            .rules
            .iter()
            .find(|r| r.rule_id == rule_id)
            .unwrap_or_else(|| panic!("no result for {rule_id}"))
    }

    /// The deterministic finding of [`ev_violation`] for `rule_id`.
    fn fixture_finding(rule_id: &str) -> (Hash, Id) {
        let key = finding_key(rule_id, &ids(&["src:hr-policy"]), FIXTURE_CONDITION).unwrap();
        let finding = finding_id(&key).unwrap();
        (key, finding)
    }

    /// A graph in which `decision_nodes` resolve the accepted fixture finding of `rule_id`.
    fn waiver_graph(rule_id: &str, decision_nodes: Vec<Node>) -> Graph {
        let (_, finding) = fixture_finding(rule_id);
        let edges = decision_nodes
            .iter()
            .enumerate()
            .map(|(i, d)| {
                resolves(
                    &format!("edge:resolves-{i}"),
                    "Accepted",
                    d.id.as_str(),
                    &finding,
                )
            })
            .collect();
        let mut nodes = vec![finding_node(&finding, "Accepted")];
        nodes.extend(decision_nodes);
        build_graph(nodes, edges)
    }

    fn valid_waiver(rule_id: &str, decision_id: &str) -> Node {
        let (key, finding) = fixture_finding(rule_id);
        decision(decision_id, "Accepted", marker(rule_id, &key, &finding))
    }

    // ------------------------------------------------------------------ ValidationPolicy (§50)

    #[test]
    fn empty_policy_is_valid_with_fixed_canonical_json_and_hash() {
        let empty = ValidationPolicy::default();
        assert!(empty.promoted_to_blocker_rule_ids.is_empty());
        assert!(empty.profile_allow_waiver_rule_ids.is_empty());
        assert_eq!(empty.validate(&metadata()), Ok(()));
        assert_eq!(canonical(&empty), EMPTY_POLICY_JSON);
        let hash = empty.policy_hash().unwrap();
        assert_eq!(hash.kind(), HashKind::Generic);
        assert_eq!(hash.as_str(), EMPTY_POLICY_HASH);
        assert_eq!(
            serde_json::from_str::<ValidationPolicy>(EMPTY_POLICY_JSON).unwrap(),
            empty
        );
        let golden = policy(&[AGENT_IDENTIFIED], &[AGENT_IDENTIFIED]);
        assert_eq!(golden.policy_hash().unwrap().as_str(), GOLDEN_POLICY_HASH);
        assert!(serde_json::from_value::<ValidationPolicy>(json!({
            "promoted_to_blocker_rule_ids": [], "profile_allow_waiver_rule_ids": [], "extra": 1
        }))
        .is_err());
    }

    #[test]
    fn policy_rejects_unknown_and_non_profile_allow_rules() {
        let registry = metadata();
        let is_invalid = |p: ValidationPolicy| {
            matches!(
                p.validate(&registry),
                Err(EvaluationError::InvalidPolicy(_))
            )
        };
        assert!(is_invalid(policy(&["ORG.I0.UNKNOWN"], &[])));
        assert!(is_invalid(policy(&[], &["ORG.I0.UNKNOWN"])));
        // decision_required and forbidden rules cannot be profile-waiver enabled.
        assert!(is_invalid(policy(&[], &[PARSE_STATUS])));
        assert!(is_invalid(policy(&[], &[CONTENT_ADDRESSED])));
        assert_eq!(policy(&[], &[AGENT_IDENTIFIED]).validate(&registry), Ok(()));
        // Any existing rule may be promoted, including a deferred one and a blocker.
        assert_eq!(
            policy(
                &[AGENT_IDENTIFIED, PARSE_STATUS, "ISO15289.D1.INFO.ITEMS"],
                &[]
            )
            .validate(&registry),
            Ok(())
        );
        // An invalid policy cannot enter a context.
        assert!(matches!(
            ValidationContext::new(
                &plain_graph(),
                &registry,
                policy(&["ORG.I0.UNKNOWN"], &[]),
                vec![]
            ),
            Err(EvaluationError::InvalidPolicy(_))
        ));
    }

    #[test]
    fn policy_promotion_raises_to_blocker_and_never_demotes() {
        let promote = policy(&[PARSE_STATUS], &[]);
        for (declared, wire) in [
            (Severity::Error, "error"),
            (Severity::Warn, "warn"),
            (Severity::Info, "info"),
            (Severity::Blocker, "blocker"),
        ] {
            let registry = metadata_with_severity(PARSE_STATUS, wire);
            let rule = registry.rule(PARSE_STATUS).unwrap();
            assert_eq!(rule.severity, declared);
            assert_eq!(
                ValidationPolicy::default().effective_severity(rule),
                declared
            );
            assert_eq!(promote.effective_severity(rule), Severity::Blocker);
        }
        // A policy naming other rules leaves this rule's severity alone.
        let registry = metadata();
        let other = policy(&[AGENT_IDENTIFIED], &[]);
        assert_eq!(
            other.effective_severity(registry.rule(PARSE_STATUS).unwrap()),
            Severity::Blocker
        );
        assert_eq!(
            other.effective_severity(registry.rule(HASHABLE).unwrap()),
            Severity::Blocker
        );
    }

    #[test]
    fn policy_sets_are_deterministically_ordered() {
        let a = policy(&[PARSE_STATUS, AGENT_IDENTIFIED, HASHABLE], &[]);
        let b = policy(&[HASHABLE, PARSE_STATUS, AGENT_IDENTIFIED], &[]);
        assert_eq!(a, b);
        assert_eq!(canonical(&a), canonical(&b));
        assert_eq!(a.policy_hash().unwrap(), b.policy_hash().unwrap());
        assert_eq!(
            canonical(&a),
            format!(
                r#"{{"profile_allow_waiver_rule_ids":[],"promoted_to_blocker_rule_ids":["{HASHABLE}","{PARSE_STATUS}","{AGENT_IDENTIFIED}"]}}"#
            )
        );
        assert_ne!(a.policy_hash().unwrap().as_str(), EMPTY_POLICY_HASH);
    }

    // ------------------------------------------------------------------ ValidationContext (§51)

    #[test]
    fn context_binds_the_exact_graph_profile_and_rule_pack() {
        let graph = plain_graph();
        let registry = metadata();
        let ctx = context(&graph, &registry, ValidationPolicy::default());
        assert_eq!(ctx.baseline_semantic_hash, graph.semantic_hash().unwrap());
        assert_eq!(ctx.baseline_semantic_hash.kind(), HashKind::Semantic);
        assert_eq!(ctx.profile_id.as_str(), PROFILE);
        assert_eq!(ctx.profile_hash.as_str(), PROFILE_HASH);
        assert_eq!(ctx.rule_pack_hash.as_str(), RULE_PACK_HASH);
        assert_eq!(ctx.policy, ValidationPolicy::default());
        assert!(ctx.external_validation_artifacts.is_empty());
        assert_eq!(ctx.validate_for(&graph, &registry), Ok(()));
    }

    #[test]
    fn context_has_no_timestamp_or_capability() {
        let ctx = context(&plain_graph(), &metadata(), ValidationPolicy::default());
        let value = serde_json::to_value(&ctx).unwrap();
        let keys: BTreeSet<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "baseline_semantic_hash",
                "external_validation_artifacts",
                "policy",
                "profile_hash",
                "profile_id",
                "rule_pack_hash"
            ])
        );
        // Equal inputs give an equal context: nothing operational is observed.
        assert_eq!(
            ctx,
            context(&plain_graph(), &metadata(), ValidationPolicy::default())
        );
    }

    #[test]
    fn context_orders_external_artifacts_by_artifact_hash() {
        let graph = plain_graph();
        let registry = metadata();
        let artifacts: Vec<ExternalValidationArtifact> = (0..4)
            .map(|n| external_artifact(json!({"ok": true, "n": n})))
            .collect();
        let forward = ValidationContext::new(
            &graph,
            &registry,
            ValidationPolicy::default(),
            artifacts.clone(),
        )
        .unwrap();
        let mut reversed_input = artifacts.clone();
        reversed_input.reverse();
        let reversed = ValidationContext::new(
            &graph,
            &registry,
            ValidationPolicy::default(),
            reversed_input,
        )
        .unwrap();
        assert_eq!(forward, reversed);
        let refs = forward.validation_artifact_refs().unwrap();
        assert_eq!(refs.len(), 4);
        assert!(refs.windows(2).all(|w| w[0] < w[1]));
        assert!(refs.iter().all(|h| h.kind() == HashKind::Generic));
        let expected: BTreeSet<Hash> = artifacts
            .iter()
            .map(|a| a.artifact_hash().unwrap())
            .collect();
        assert_eq!(refs.iter().cloned().collect::<BTreeSet<_>>(), expected);
        for (artifact, hash) in forward.external_validation_artifacts.iter().zip(&refs) {
            assert_eq!(&artifact.artifact_hash().unwrap(), hash);
        }
    }

    #[test]
    fn context_rejects_duplicate_and_invalid_external_artifacts() {
        let graph = plain_graph();
        let registry = metadata();
        let artifact = external_artifact(json!({"ok": true}));
        assert!(matches!(
            ValidationContext::new(
                &graph,
                &registry,
                ValidationPolicy::default(),
                vec![artifact.clone(), artifact.clone()]
            ),
            Err(EvaluationError::InvalidContext(_))
        ));
        let mut invalid = artifact.clone();
        invalid.validated_output = CanonicalJson::new(json!({"ok": false}));
        assert!(matches!(
            ValidationContext::new(
                &graph,
                &registry,
                ValidationPolicy::default(),
                vec![invalid.clone()]
            ),
            Err(EvaluationError::ExternalValidation(
                ExternalValidationError::InvalidArtifact(_)
            ))
        ));

        // A context tampered with after construction is rejected before evaluation.
        let evaluators = i0_registry(metadata(), &[]);
        let mut ctx = context(&graph, &registry, ValidationPolicy::default());
        ctx.external_validation_artifacts = vec![invalid];
        assert!(matches!(
            evaluators.evaluate_gate(GateId::I0, &graph, &ctx),
            Err(EvaluationError::ExternalValidation(_))
        ));
        let mut ctx = context(&graph, &registry, ValidationPolicy::default());
        ctx.external_validation_artifacts = vec![artifact.clone(), artifact];
        assert!(matches!(
            evaluators.evaluate_gate(GateId::I0, &graph, &ctx),
            Err(EvaluationError::InvalidContext(_))
        ));
    }

    #[test]
    fn context_for_another_graph_or_profile_is_rejected() {
        let graph = plain_graph();
        let evaluators = i0_registry(metadata(), &[]);
        let is_invalid = |r: Result<GateReport, EvaluationError>| {
            matches!(r, Err(EvaluationError::InvalidContext(_)))
        };

        let other_graph = build_graph(vec![requirement("req:HR-002")], vec![]);
        assert_ne!(
            graph.semantic_hash().unwrap(),
            other_graph.semantic_hash().unwrap()
        );
        let ctx = context(
            &other_graph,
            evaluators.metadata(),
            ValidationPolicy::default(),
        );
        assert!(is_invalid(evaluators.evaluate_gate(
            GateId::I0,
            &graph,
            &ctx
        )));

        // A registry over a different profile: another rule pack and profile hash.
        let other_registry = metadata_with_severity(PARSE_STATUS, "error");
        let ctx = context(&graph, &other_registry, ValidationPolicy::default());
        assert_ne!(ctx.profile_hash.as_str(), PROFILE_HASH);
        assert!(is_invalid(evaluators.evaluate_gate(
            GateId::I0,
            &graph,
            &ctx
        )));

        for tamper in [
            (|c: &mut ValidationContext| c.profile_id = "profile:other".parse().unwrap())
                as fn(&mut ValidationContext),
            |c: &mut ValidationContext| c.profile_hash = Hash::content_sha256(b"other"),
            |c: &mut ValidationContext| c.rule_pack_hash = Hash::content_sha256(b"other"),
        ] {
            let mut ctx = context(&graph, evaluators.metadata(), ValidationPolicy::default());
            tamper(&mut ctx);
            assert!(is_invalid(evaluators.evaluate_gate(
                GateId::I0,
                &graph,
                &ctx
            )));
        }
        let mut ctx = context(&graph, evaluators.metadata(), ValidationPolicy::default());
        ctx.policy = policy(&["ORG.I0.UNKNOWN"], &[]);
        assert!(matches!(
            evaluators.evaluate_gate(GateId::I0, &graph, &ctx),
            Err(EvaluationError::InvalidPolicy(_))
        ));
    }

    // ------------------------------------------------------------------ RuleEvaluation (§52)

    #[test]
    fn applicability_has_the_exact_wire_forms() {
        assert_eq!(
            canonical(&Applicability::Applicable),
            r#"{"state":"APPLICABLE"}"#
        );
        let na =
            Applicability::not_applicable("no evidence fragments in scope".to_owned()).unwrap();
        assert_eq!(
            canonical(&na),
            r#"{"reason":"no evidence fragments in scope","state":"NOT_APPLICABLE"}"#
        );
        assert_eq!(
            serde_json::from_str::<Applicability>(r#"{"state":"APPLICABLE"}"#).unwrap(),
            Applicability::Applicable
        );
        assert_eq!(
            serde_json::from_str::<Applicability>(&canonical(&na)).unwrap(),
            na
        );
        for bad in [
            r#"{"state":"applicable"}"#,
            r#"{"state":"UNKNOWN"}"#,
            r#"{"state":"NOT_APPLICABLE"}"#,
            r#"{"state":"NOT_APPLICABLE","reason":"r","extra":1}"#,
            r#"{"state":"APPLICABLE","reason":"r"}"#,
            r#""APPLICABLE""#,
        ] {
            assert!(serde_json::from_str::<Applicability>(bad).is_err(), "{bad}");
        }
        for reason in ["", " padded", "padded ", "a\tb", "two\nlines"] {
            assert!(
                matches!(
                    Applicability::not_applicable(reason.to_owned()),
                    Err(EvaluationError::InvalidEvaluatorOutput(_))
                ),
                "{reason:?}"
            );
        }
    }

    #[test]
    fn rule_evaluation_constructors_sort_and_reject_duplicates() {
        let pass =
            RuleEvaluation::pass(ids(&["src:b", "src:a"]), strings(&["ev:2", "ev:1"])).unwrap();
        assert_eq!(
            pass,
            RuleEvaluation::Pass {
                targets: ids(&["src:a", "src:b"]),
                evidence: strings(&["ev:1", "ev:2"]),
            }
        );
        let violation = RuleEvaluation::violation(
            ids(&["src:b", "src:a"]),
            strings(&["sha256:artifact", "ev:1"]),
            "condition".to_owned(),
            "Message.".to_owned(),
            Some("Resolve.".to_owned()),
        )
        .unwrap();
        assert_eq!(
            violation,
            RuleEvaluation::Violation {
                targets: ids(&["src:a", "src:b"]),
                evidence: strings(&["ev:1", "sha256:artifact"]),
                semantic_condition_key: "condition".to_owned(),
                message: "Message.".to_owned(),
                suggested_resolution: Some("Resolve.".to_owned()),
            }
        );
        assert_eq!(
            RuleEvaluation::not_applicable("out of scope".to_owned()).unwrap(),
            RuleEvaluation::NotApplicable {
                reason: "out of scope".to_owned()
            }
        );
        let failure = EvaluatorFailure::new(
            "E_FIXTURE".to_owned(),
            "cannot evaluate".to_owned(),
            ids(&["src:b", "src:a"]),
            strings(&["ev:2", "ev:1"]),
        )
        .unwrap();
        assert_eq!(failure.targets, ids(&["src:a", "src:b"]));
        assert_eq!(failure.evidence, strings(&["ev:1", "ev:2"]));

        let invalid = |r: Result<RuleEvaluation, EvaluationError>| {
            matches!(r, Err(EvaluationError::InvalidEvaluatorOutput(_)))
        };
        assert!(invalid(RuleEvaluation::pass(
            ids(&["src:a", "src:a"]),
            vec![]
        )));
        assert!(invalid(RuleEvaluation::pass(
            vec![],
            strings(&["ev:1", "ev:1"])
        )));
        for evidence in ["", " ev:1", "ev:1 ", "ev\t1"] {
            assert!(
                invalid(RuleEvaluation::pass(vec![], strings(&[evidence]))),
                "{evidence:?}"
            );
        }
        assert!(invalid(RuleEvaluation::not_applicable(String::new())));
        assert!(invalid(RuleEvaluation::violation(
            ids(&["src:a", "src:a"]),
            vec![],
            "condition".to_owned(),
            "Message.".to_owned(),
            None
        )));
        assert!(invalid(RuleEvaluation::violation(
            vec![],
            vec![],
            "condition".to_owned(),
            String::new(),
            None
        )));
        for key in ["", " padded", "padded ", "a\tb", "a\nb", "a\u{0}b"] {
            assert!(
                matches!(
                    RuleEvaluation::violation(
                        vec![],
                        vec![],
                        key.to_owned(),
                        "Message.".to_owned(),
                        None
                    ),
                    Err(EvaluationError::InvalidSemanticConditionKey(_))
                ),
                "{key:?}"
            );
        }
        let failure = |code: &str, message: &str, targets: &[&str], evidence: &[&str]| {
            EvaluatorFailure::new(
                code.to_owned(),
                message.to_owned(),
                ids(targets),
                strings(evidence),
            )
        };
        for bad in [
            failure("", "m", &[], &[]),
            failure(" E", "m", &[], &[]),
            failure("E", "", &[], &[]),
            failure("E", "m", &["src:a", "src:a"], &[]),
            failure("E", "m", &[], &["ev:1", "ev:1"]),
        ] {
            assert!(matches!(
                bad,
                Err(EvaluationError::InvalidEvaluatorOutput(_))
            ));
        }
    }

    #[test]
    fn engine_detects_malformed_evaluator_output() {
        let graph = plain_graph();
        for evaluator in [
            ev_unsorted_targets as RuleEvaluator,
            ev_duplicate_evidence,
            ev_empty_reason,
            ev_malformed_failure,
        ] {
            let result = evaluate_i0(
                &graph,
                ValidationPolicy::default(),
                &[(HASHABLE, evaluator)],
            );
            assert!(
                matches!(&result, Err(EvaluationError::InvalidEvaluatorOutput(reason)) if reason.contains(HASHABLE)),
                "{result:?}"
            );
        }
        assert_eq!(
            evaluate_i0(
                &graph,
                ValidationPolicy::default(),
                &[(HASHABLE, ev_padded_condition_key)]
            ),
            Err(EvaluationError::InvalidSemanticConditionKey(
                " padded".to_owned()
            ))
        );
    }

    // ------------------------------------------------------------------ finding key (§53)

    #[test]
    fn finding_key_and_id_match_the_independent_golden() {
        // The exact hashed bytes: rule, NUL, each sorted target and NUL, condition key.
        assert_eq!(Hash::content_sha256(GOLDEN_KEY_BYTES).as_str(), GOLDEN_KEY);
        let key = finding_key(
            "PLUMB.F1.REQ.GROUNDED",
            &ids(&["req:HR-001", "req:HR-002"]),
            "missing-evidence",
        )
        .unwrap();
        assert_eq!(key.as_str(), GOLDEN_KEY);
        assert_eq!(key.kind(), HashKind::Generic);
        assert_eq!(finding_id(&key).unwrap().as_str(), GOLDEN_FINDING_ID);

        // Target insertion order does not matter.
        let reordered = finding_key(
            "PLUMB.F1.REQ.GROUNDED",
            &ids(&["req:HR-002", "req:HR-001"]),
            "missing-evidence",
        )
        .unwrap();
        assert_eq!(reordered, key);

        // No targets: rule, NUL, condition key.
        assert_eq!(
            finding_key("PLUMB.F1.REQ.GROUNDED", &[], "missing-evidence").unwrap(),
            Hash::content_sha256(b"PLUMB.F1.REQ.GROUNDED\x00missing-evidence")
        );
    }

    #[test]
    fn finding_key_changes_with_rule_target_and_condition() {
        let targets = ids(&["req:HR-001", "req:HR-002"]);
        let base = finding_key("PLUMB.F1.REQ.GROUNDED", &targets, "missing-evidence").unwrap();
        let variants = [
            finding_key("PLUMB.F1.REQ.TYPE_KNOWN", &targets, "missing-evidence").unwrap(),
            finding_key(
                "PLUMB.F1.REQ.GROUNDED",
                &ids(&["req:HR-001"]),
                "missing-evidence",
            )
            .unwrap(),
            finding_key(
                "PLUMB.F1.REQ.GROUNDED",
                &ids(&["req:HR-001", "req:HR-003"]),
                "missing-evidence",
            )
            .unwrap(),
            finding_key("PLUMB.F1.REQ.GROUNDED", &targets, "missing-origin").unwrap(),
        ];
        let keys: BTreeSet<&Hash> = variants.iter().chain([&base]).collect();
        assert_eq!(keys.len(), 5);
        let finding_ids: BTreeSet<Id> = keys.iter().map(|k| finding_id(k).unwrap()).collect();
        assert_eq!(finding_ids.len(), 5);
    }

    #[test]
    fn finding_key_and_id_reject_invalid_input() {
        let targets = ids(&["req:HR-001"]);
        for key in ["", " k", "k ", "a\u{0}b", "a\nb"] {
            assert!(
                matches!(
                    finding_key("PLUMB.F1.REQ.GROUNDED", &targets, key),
                    Err(EvaluationError::InvalidSemanticConditionKey(_))
                ),
                "{key:?}"
            );
        }
        for rule_id in ["", "a\u{0}b", " PLUMB"] {
            assert!(matches!(
                finding_key(rule_id, &targets, "k"),
                Err(EvaluationError::InvalidFindingKey(_))
            ));
        }
        assert!(matches!(
            finding_key(
                "PLUMB.F1.REQ.GROUNDED",
                &ids(&["req:HR-001", "req:HR-001"]),
                "k"
            ),
            Err(EvaluationError::InvalidFindingKey(_))
        ));
        for other_kind in [Hash::semantic_sha256(b"x"), Hash::evidence_sha256(b"x")] {
            assert!(matches!(
                finding_id(&other_kind),
                Err(EvaluationError::InvalidFindingId(_))
            ));
        }
    }

    // ------------------------------------------------------------------ generated finding (§54)

    #[test]
    fn severity_maps_one_to_one_onto_finding_severity() {
        assert_eq!(
            finding_severity(Severity::Blocker),
            FindingSeverity::Blocker
        );
        assert_eq!(finding_severity(Severity::Error), FindingSeverity::Error);
        assert_eq!(finding_severity(Severity::Warn), FindingSeverity::Warn);
        assert_eq!(finding_severity(Severity::Info), FindingSeverity::Info);
    }

    #[test]
    fn generated_finding_maps_rule_metadata_and_violation_exactly() {
        let report = evaluate_i0(
            &plain_graph(),
            ValidationPolicy::default(),
            &[
                (AGENT_IDENTIFIED, ev_violation_reworded),
                (PARSE_STATUS, ev_violation),
            ],
        )
        .unwrap();
        assert_eq!(report.findings.len(), 2);

        let (key, finding) = fixture_finding(AGENT_IDENTIFIED);
        let generated = report.findings.iter().find(|f| f.id == finding).unwrap();
        assert_eq!(generated.key, key);
        assert_eq!(generated.semantic_condition_key, FIXTURE_CONDITION);
        let payload = &generated.payload;
        assert_eq!(payload.code, AGENT_IDENTIFIED);
        assert_eq!(payload.family, "I0");
        assert_eq!(payload.severity, FindingSeverity::Error);
        assert_eq!(payload.message, "Reworded.");
        assert_eq!(payload.status, "Open");
        assert_eq!(payload.affected_refs, ids(&["src:hr-policy"]));
        // The rule has a standard reference, so the finding points back at the rule.
        assert_eq!(payload.standard_rule_ref.as_deref(), Some(AGENT_IDENTIFIED));
        assert_eq!(
            payload.suggested_resolution.as_deref(),
            Some("Fix the source.")
        );
        assert_eq!(payload.waiver_ref, None);
        assert_eq!(
            rule_result(&report, AGENT_IDENTIFIED).finding_ref.as_ref(),
            Some(&finding)
        );

        let (_, finding) = fixture_finding(PARSE_STATUS);
        let payload = &report
            .findings
            .iter()
            .find(|f| f.id == finding)
            .unwrap()
            .payload;
        assert_eq!(payload.code, PARSE_STATUS);
        assert_eq!(payload.severity, FindingSeverity::Blocker);
        assert_eq!(payload.message, format!("{PARSE_STATUS} is violated."));
        // No standard reference on this rule.
        assert_eq!(payload.standard_rule_ref, None);
        assert_eq!(payload.suggested_resolution, None);
    }

    #[test]
    fn finding_identity_ignores_severity_message_policy_and_waiver() {
        let graph = plain_graph();
        let (key, finding) = fixture_finding(AGENT_IDENTIFIED);
        let finding_of = |report: &GateReport| report.findings[0].clone();

        let plain = finding_of(
            &evaluate_i0(
                &graph,
                ValidationPolicy::default(),
                &[(AGENT_IDENTIFIED, ev_violation)],
            )
            .unwrap(),
        );
        // Promotion changes the generated severity, not the identity.
        let promoted = finding_of(
            &evaluate_i0(
                &graph,
                policy(&[AGENT_IDENTIFIED], &[AGENT_IDENTIFIED]),
                &[(AGENT_IDENTIFIED, ev_violation)],
            )
            .unwrap(),
        );
        assert_eq!(plain.payload.severity, FindingSeverity::Error);
        assert_eq!(promoted.payload.severity, FindingSeverity::Blocker);
        // Another message and resolution: same identity.
        let reworded = finding_of(
            &evaluate_i0(
                &graph,
                ValidationPolicy::default(),
                &[(AGENT_IDENTIFIED, ev_violation_reworded)],
            )
            .unwrap(),
        );
        assert_ne!(reworded.payload.message, plain.payload.message);
        // A waiver sets waiver_ref and nothing else about the identity.
        let waived_graph = waiver_graph(
            AGENT_IDENTIFIED,
            vec![valid_waiver(AGENT_IDENTIFIED, "dec:waive-1")],
        );
        let waived = finding_of(
            &evaluate_i0(
                &waived_graph,
                policy(&[], &[AGENT_IDENTIFIED]),
                &[(AGENT_IDENTIFIED, ev_violation)],
            )
            .unwrap(),
        );
        assert_eq!(waived.payload.waiver_ref, Some(id("dec:waive-1")));
        for generated in [&plain, &promoted, &reworded, &waived] {
            assert_eq!(generated.key, key);
            assert_eq!(generated.id, finding);
        }
    }

    // ------------------------------------------------------------------ waivers (§55)

    /// Evaluates I0 with `rule_id` violated over `graph`.
    fn evaluate_violation(
        graph: &Graph,
        rule_id: &str,
        policy: ValidationPolicy,
    ) -> Result<GateReport, EvaluationError> {
        evaluate_i0(graph, policy, &[(rule_id, ev_violation)])
    }

    #[test]
    fn violation_without_a_decision_is_not_waived() {
        let (_, finding) = fixture_finding(PARSE_STATUS);
        for graph in [
            plain_graph(),
            // The finding exists but nothing resolves it.
            build_graph(vec![finding_node(&finding, "Accepted")], vec![]),
        ] {
            let report =
                evaluate_violation(&graph, PARSE_STATUS, ValidationPolicy::default()).unwrap();
            let result = rule_result(&report, PARSE_STATUS);
            assert_eq!(result.state, RuleResultState::Fail);
            assert_eq!(result.waiver_ref, None);
            assert!(report.waivers.is_empty());
            assert_eq!(report.result, GateResult::Fail);
        }
    }

    #[test]
    fn accepted_decision_required_waiver_is_applied() {
        let (key, finding) = fixture_finding(PARSE_STATUS);
        let graph = waiver_graph(
            PARSE_STATUS,
            vec![valid_waiver(PARSE_STATUS, "dec:waive-1")],
        );
        let report = evaluate_violation(&graph, PARSE_STATUS, ValidationPolicy::default()).unwrap();
        let result = rule_result(&report, PARSE_STATUS);
        assert_eq!(result.state, RuleResultState::Waived);
        assert_eq!(result.applicability, Applicability::Applicable);
        assert_eq!(result.finding_ref, Some(finding.clone()));
        assert_eq!(result.waiver_ref, Some(id("dec:waive-1")));
        assert_eq!(
            result.semantic_condition_key.as_deref(),
            Some(FIXTURE_CONDITION)
        );
        assert_eq!(result.error, None);
        assert_eq!(
            report.waivers,
            vec![AppliedWaiver {
                rule_id: PARSE_STATUS.to_owned(),
                finding_ref: finding,
                finding_key: key,
                decision_ref: id("dec:waive-1"),
            }]
        );
        assert_eq!(
            report.findings[0].payload.waiver_ref,
            Some(id("dec:waive-1"))
        );
        assert_eq!(report.summary.blocker_waived, 1);
        assert_eq!(report.result, GateResult::Pass);
    }

    #[test]
    fn waiver_requires_accepted_finding_edge_and_decision() {
        let (key, finding) = fixture_finding(PARSE_STATUS);
        let other = id("fnd:0000000000000000");
        let waiver =
            |status: &str| decision("dec:waive-1", status, marker(PARSE_STATUS, &key, &finding));
        let graphs = [
            // decision Proposed (its edge cannot be baseline either)
            build_graph(
                vec![finding_node(&finding, "Accepted"), waiver("Proposed")],
                vec![resolves("edge:r1", "Proposed", "dec:waive-1", &finding)],
            ),
            // decision baseline but not Accepted
            build_graph(
                vec![finding_node(&finding, "Accepted"), waiver("Suspect")],
                vec![resolves("edge:r1", "Accepted", "dec:waive-1", &finding)],
            ),
            // resolves edge Proposed; the accepted decision resolves another finding
            build_graph(
                vec![
                    finding_node(&finding, "Accepted"),
                    finding_node(&other, "Accepted"),
                    waiver("Accepted"),
                ],
                vec![
                    resolves("edge:r1", "Proposed", "dec:waive-1", &finding),
                    resolves("edge:r2", "Accepted", "dec:waive-1", &other),
                ],
            ),
            // resolves edge baseline but not Accepted
            build_graph(
                vec![finding_node(&finding, "Accepted"), waiver("Accepted")],
                vec![resolves("edge:r1", "Suspect", "dec:waive-1", &finding)],
            ),
            // finding node not Accepted
            build_graph(
                vec![finding_node(&finding, "Suspect"), waiver("Accepted")],
                vec![resolves("edge:r1", "Accepted", "dec:waive-1", &finding)],
            ),
            // the decision resolves a different finding only
            build_graph(
                vec![finding_node(&other, "Accepted"), waiver("Accepted")],
                vec![resolves("edge:r1", "Accepted", "dec:waive-1", &other)],
            ),
        ];
        for (index, graph) in graphs.iter().enumerate() {
            let report =
                evaluate_violation(graph, PARSE_STATUS, ValidationPolicy::default()).unwrap();
            let result = rule_result(&report, PARSE_STATUS);
            assert_eq!(result.state, RuleResultState::Fail, "graph {index}");
            assert_eq!(result.waiver_ref, None, "graph {index}");
            assert!(report.waivers.is_empty(), "graph {index}");
        }
    }

    #[test]
    fn malformed_waiver_decisions_are_framework_errors() {
        let (key, finding) = fixture_finding(PARSE_STATUS);
        let (other_key, other_finding) = fixture_finding(CONTENT_ADDRESSED);
        let good = marker(PARSE_STATUS, &key, &finding);
        let with = |field: &str, value: Value| {
            let mut answer = good.clone();
            answer[field] = value;
            answer
        };
        let mut missing_field = good.clone();
        missing_field.as_object_mut().unwrap().remove("finding_ref");
        let answers = [
            // malformed marker
            missing_field,
            with("expires_at", json!(T2)),
            with("finding_key", json!("not-a-hash")),
            with("finding_ref", json!(17)),
            with("finding_key", json!(Hash::semantic_sha256(b"x"))),
            // wrong rule, key, finding
            with("rule_id", json!(CONTENT_ADDRESSED)),
            with("finding_key", json!(other_key)),
            with("finding_ref", json!(other_finding)),
        ];
        for answer in answers {
            let graph = waiver_graph(
                PARSE_STATUS,
                vec![decision("dec:waive-1", "Accepted", answer.clone())],
            );
            let result = evaluate_violation(&graph, PARSE_STATUS, ValidationPolicy::default());
            assert!(
                matches!(
                    &result,
                    Err(EvaluationError::MalformedWaiverDecision { decision, .. })
                        if decision.as_str() == "dec:waive-1"
                ),
                "{answer}: {result:?}"
            );
        }
        // Missing or unclean rationale.
        for rationale in [json!(null), json!(""), json!(" padded"), json!("a\tb")] {
            let graph = waiver_graph(
                PARSE_STATUS,
                vec![decision_at(
                    "dec:waive-1",
                    "Accepted",
                    good.clone(),
                    rationale.clone(),
                    T1,
                )],
            );
            assert!(
                matches!(
                    evaluate_violation(&graph, PARSE_STATUS, ValidationPolicy::default()),
                    Err(EvaluationError::MalformedWaiverDecision { .. })
                ),
                "{rationale}"
            );
        }
    }

    #[test]
    fn forbidden_waiver_attempt_is_an_error() {
        let graph = waiver_graph(
            CONTENT_ADDRESSED,
            vec![valid_waiver(CONTENT_ADDRESSED, "dec:waive-1")],
        );
        assert_eq!(
            evaluate_violation(&graph, CONTENT_ADDRESSED, ValidationPolicy::default()),
            Err(EvaluationError::ForbiddenWaiver {
                rule_id: CONTENT_ADDRESSED.to_owned(),
                decision: id("dec:waive-1"),
            })
        );
    }

    #[test]
    fn profile_allow_waiver_needs_policy_enablement() {
        let (_, finding) = fixture_finding(AGENT_IDENTIFIED);
        let graph = waiver_graph(
            AGENT_IDENTIFIED,
            vec![valid_waiver(AGENT_IDENTIFIED, "dec:waive-1")],
        );
        assert_eq!(
            evaluate_violation(&graph, AGENT_IDENTIFIED, ValidationPolicy::default()),
            Err(EvaluationError::ProfileWaiverNotEnabled {
                rule_id: AGENT_IDENTIFIED.to_owned(),
                decision: id("dec:waive-1"),
            })
        );
        // Promotion alone does not enable the waiver.
        assert!(matches!(
            evaluate_violation(&graph, AGENT_IDENTIFIED, policy(&[AGENT_IDENTIFIED], &[])),
            Err(EvaluationError::ProfileWaiverNotEnabled { .. })
        ));
        let report =
            evaluate_violation(&graph, AGENT_IDENTIFIED, policy(&[], &[AGENT_IDENTIFIED])).unwrap();
        let result = rule_result(&report, AGENT_IDENTIFIED);
        assert_eq!(result.state, RuleResultState::Waived);
        assert_eq!(result.finding_ref, Some(finding));
        assert_eq!(result.waiver_ref, Some(id("dec:waive-1")));
        // A waived non-blocker is not a blocker waiver.
        assert_eq!(report.summary, GateSummary::default());
        let promoted = evaluate_violation(
            &graph,
            AGENT_IDENTIFIED,
            policy(&[AGENT_IDENTIFIED], &[AGENT_IDENTIFIED]),
        )
        .unwrap();
        assert_eq!(promoted.summary.blocker_waived, 1);
        assert_eq!(promoted.result, GateResult::Pass);
    }

    #[test]
    fn two_qualifying_waiver_decisions_are_ambiguous() {
        let (_, finding) = fixture_finding(PARSE_STATUS);
        let graph = waiver_graph(
            PARSE_STATUS,
            vec![
                valid_waiver(PARSE_STATUS, "dec:waive-2"),
                valid_waiver(PARSE_STATUS, "dec:waive-1"),
            ],
        );
        assert_eq!(
            evaluate_violation(&graph, PARSE_STATUS, ValidationPolicy::default()),
            Err(EvaluationError::AmbiguousWaiver {
                rule_id: PARSE_STATUS.to_owned(),
                finding_ref: finding,
                decisions: ids(&["dec:waive-1", "dec:waive-2"]),
            })
        );
    }

    #[test]
    fn unrelated_resolution_decisions_are_ignored() {
        let unrelated = || {
            [
                decision("dec:answer-1", "Accepted", json!({"choice": "re-import"})),
                decision("dec:answer-2", "Accepted", json!("accepted as is")),
                decision("dec:answer-3", "Accepted", json!({"kind": "clarification"})),
            ]
        };
        let graph = waiver_graph(PARSE_STATUS, unrelated().to_vec());
        let report = evaluate_violation(&graph, PARSE_STATUS, ValidationPolicy::default()).unwrap();
        assert_eq!(
            rule_result(&report, PARSE_STATUS).state,
            RuleResultState::Fail
        );
        assert!(report.waivers.is_empty());

        // Next to one real waiver they do not make it ambiguous.
        let mut decisions = unrelated().to_vec();
        decisions.push(valid_waiver(PARSE_STATUS, "dec:waive-1"));
        let graph = waiver_graph(PARSE_STATUS, decisions);
        let report = evaluate_violation(&graph, PARSE_STATUS, ValidationPolicy::default()).unwrap();
        assert_eq!(
            rule_result(&report, PARSE_STATUS).waiver_ref,
            Some(id("dec:waive-1"))
        );
    }

    #[test]
    fn waiver_validity_does_not_depend_on_time() {
        let (key, finding) = fixture_finding(PARSE_STATUS);
        let report_for = |decided_at: &str| {
            let waiver = decision_at(
                "dec:waive-1",
                "Accepted",
                marker(PARSE_STATUS, &key, &finding),
                json!("Accepted risk."),
                decided_at,
            );
            let graph = waiver_graph(PARSE_STATUS, vec![waiver]);
            evaluate_violation(&graph, PARSE_STATUS, ValidationPolicy::default()).unwrap()
        };
        // Decisions years apart: both waive, and the reports differ in nothing else.
        let early = report_for(T1);
        let late = report_for(T2);
        assert_eq!(
            rule_result(&early, PARSE_STATUS).state,
            RuleResultState::Waived
        );
        assert_eq!(early.rules, late.rules);
        assert_eq!(early.waivers, late.waivers);
        assert_eq!(early.findings, late.findings);
    }

    // ------------------------------------------------------------------ state mapping (§56)

    #[test]
    fn engine_derives_the_rule_result_state() {
        let graph = plain_graph();
        let report = evaluate_i0(
            &graph,
            ValidationPolicy::default(),
            &[
                (HASH_MATCH, ev_not_applicable),
                (LOCATABLE, ev_failure),
                (CONTENT_ADDRESSED, ev_violation),
            ],
        )
        .unwrap();

        let pass = rule_result(&report, HASHABLE);
        assert_eq!(pass.state, RuleResultState::Pass);
        assert_eq!(pass.applicability, Applicability::Applicable);
        assert_eq!(pass.targets, ids(&["src:hr-policy"]));
        assert_eq!(pass.evidence, strings(&["ev:manifest"]));
        assert_eq!(
            (
                &pass.semantic_condition_key,
                &pass.finding_ref,
                &pass.waiver_ref,
                &pass.error
            ),
            (&None, &None, &None, &None)
        );

        let na = rule_result(&report, HASH_MATCH);
        assert_eq!(na.state, RuleResultState::NotApplicable);
        assert_eq!(
            na.applicability,
            Applicability::NotApplicable {
                reason: "no evidence fragments in scope".to_owned()
            }
        );
        assert!(na.targets.is_empty() && na.evidence.is_empty());
        assert_eq!(
            (
                &na.semantic_condition_key,
                &na.finding_ref,
                &na.waiver_ref,
                &na.error
            ),
            (&None, &None, &None, &None)
        );

        let error = rule_result(&report, LOCATABLE);
        assert_eq!(error.state, RuleResultState::Error);
        assert_eq!(error.applicability, Applicability::Applicable);
        assert_eq!(error.targets, ids(&["frag:a1"]));
        assert_eq!(
            (
                &error.semantic_condition_key,
                &error.finding_ref,
                &error.waiver_ref
            ),
            (&None, &None, &None)
        );
        let failure = error.error.as_ref().unwrap();
        assert_eq!(failure.code, "E_LOCATOR");
        assert_eq!(failure.message, "locator index unavailable");

        let fail = rule_result(&report, CONTENT_ADDRESSED);
        assert_eq!(fail.state, RuleResultState::Fail);
        assert_eq!(fail.applicability, Applicability::Applicable);
        assert_eq!(
            fail.semantic_condition_key.as_deref(),
            Some(FIXTURE_CONDITION)
        );
        assert_eq!(fail.finding_ref, Some(fixture_finding(CONTENT_ADDRESSED).1));
        assert_eq!((&fail.waiver_ref, &fail.error), (&None, &None));

        // One rule erroring did not stop the others: all six have a result.
        assert_eq!(report.rules.len(), 6);
    }

    #[test]
    fn violation_state_follows_effective_severity() {
        assert_eq!(violation_state(Severity::Blocker), RuleResultState::Fail);
        assert_eq!(violation_state(Severity::Error), RuleResultState::Fail);
        assert_eq!(violation_state(Severity::Warn), RuleResultState::Warn);
        assert_eq!(violation_state(Severity::Info), RuleResultState::Warn);

        let graph = plain_graph();
        for (declared, wire, state) in [
            (Severity::Blocker, "blocker", RuleResultState::Fail),
            (Severity::Error, "error", RuleResultState::Fail),
            (Severity::Warn, "warn", RuleResultState::Warn),
            (Severity::Info, "info", RuleResultState::Warn),
        ] {
            for promote in [false, true] {
                let registry = i0_registry(
                    metadata_with_severity(PARSE_STATUS, wire),
                    &[(PARSE_STATUS, ev_violation)],
                );
                let chosen = if promote {
                    policy(&[PARSE_STATUS], &[])
                } else {
                    ValidationPolicy::default()
                };
                let ctx = context(&graph, registry.metadata(), chosen);
                let report = registry.evaluate_gate(GateId::I0, &graph, &ctx).unwrap();
                let result = rule_result(&report, PARSE_STATUS);
                assert_eq!(result.declared_severity, declared);
                if promote {
                    // Promoted warn/error/info violations fail as blockers.
                    assert_eq!(result.effective_severity, Severity::Blocker);
                    assert_eq!(result.state, RuleResultState::Fail);
                    assert_eq!(
                        report.findings[0].payload.severity,
                        FindingSeverity::Blocker
                    );
                } else {
                    assert_eq!(result.effective_severity, declared);
                    assert_eq!(result.state, state, "{wire}");
                    assert_eq!(
                        report.findings[0].payload.severity,
                        finding_severity(declared)
                    );
                }
            }
        }
    }

    #[test]
    fn evaluators_cannot_emit_waived() {
        // Exhaustive without a wildcard: an evaluator has exactly these three outcomes.
        let describe = |evaluation: &RuleEvaluation| match evaluation {
            RuleEvaluation::Pass { .. } => "pass",
            RuleEvaluation::Violation { .. } => "violation",
            RuleEvaluation::NotApplicable { .. } => "not-applicable",
        };
        assert_eq!(
            describe(&RuleEvaluation::pass(vec![], vec![]).unwrap()),
            "pass"
        );
        let source = include_str!("../src/evaluator.rs");
        let start = source.find("pub enum RuleEvaluation {").unwrap();
        let body = &source[start..start + source[start..].find("\n}\n").unwrap()];
        assert!(!body.contains("Waived"));
        assert!(!body.contains("RuleResultState"));
    }

    // ------------------------------------------------------------------ GateSummary / pass (§57)

    fn summary(blocker_failed: u32, blocker_waived: u32, warnings: u32) -> GateSummary {
        GateSummary {
            blocker_failed,
            blocker_waived,
            warnings,
        }
    }

    #[test]
    fn gate_result_has_exactly_two_wire_values() {
        assert_eq!(GateResult::ALL, [GateResult::Pass, GateResult::Fail]);
        for (value, wire) in [(GateResult::Pass, "PASS"), (GateResult::Fail, "FAIL")] {
            assert_eq!(value.as_str(), wire);
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format!("\"{wire}\"")
            );
            assert_eq!(
                serde_json::from_str::<GateResult>(&format!("\"{wire}\"")).unwrap(),
                value
            );
        }
        for bad in [
            "pass",
            "Pass",
            "WARN",
            "WAIVED",
            "NOT_APPLICABLE",
            "ERROR",
            "",
        ] {
            assert!(serde_json::from_str::<GateResult>(&format!("\"{bad}\"")).is_err());
        }
    }

    #[test]
    fn gate_passes_exactly_when_no_effective_blocker_failed() {
        let graph = plain_graph();
        let outcome = |policy: ValidationPolicy, overrides: &[(&str, RuleEvaluator)]| {
            let report = evaluate_i0(&graph, policy, overrides).unwrap();
            (report.result, report.summary)
        };
        let none = ValidationPolicy::default;

        // all blocking rules PASS
        assert_eq!(outcome(none(), &[]), (GateResult::Pass, summary(0, 0, 0)));
        // blocking NOT_APPLICABLE
        assert_eq!(
            outcome(none(), &[(HASHABLE, ev_not_applicable)]),
            (GateResult::Pass, summary(0, 0, 0))
        );
        // blocking FAIL
        assert_eq!(
            outcome(none(), &[(HASHABLE, ev_violation)]),
            (GateResult::Fail, summary(1, 0, 0))
        );
        // blocking ERROR
        assert_eq!(
            outcome(none(), &[(HASHABLE, ev_failure)]),
            (GateResult::Fail, summary(1, 0, 0))
        );
        // non-promoted Error rule: FAIL and ERROR do not block
        assert_eq!(
            outcome(none(), &[(AGENT_IDENTIFIED, ev_violation)]),
            (GateResult::Pass, summary(0, 0, 0))
        );
        assert_eq!(
            outcome(none(), &[(AGENT_IDENTIFIED, ev_failure)]),
            (GateResult::Pass, summary(0, 0, 0))
        );
        // promoted Error rule: violation and failure block
        let promoted = || policy(&[AGENT_IDENTIFIED], &[]);
        assert_eq!(
            outcome(promoted(), &[(AGENT_IDENTIFIED, ev_violation)]),
            (GateResult::Fail, summary(1, 0, 0))
        );
        assert_eq!(
            outcome(promoted(), &[(AGENT_IDENTIFIED, ev_failure)]),
            (GateResult::Fail, summary(1, 0, 0))
        );
        // a promoted rule that passes or does not apply blocks nothing
        assert_eq!(
            outcome(promoted(), &[]),
            (GateResult::Pass, summary(0, 0, 0))
        );
        // exact counts over several rules
        assert_eq!(
            outcome(
                promoted(),
                &[
                    (HASHABLE, ev_violation),
                    (HASH_MATCH, ev_failure),
                    (LOCATABLE, ev_not_applicable),
                    (AGENT_IDENTIFIED, ev_violation),
                ]
            ),
            (GateResult::Fail, summary(3, 0, 0))
        );
    }

    #[test]
    fn warnings_do_not_block_unless_promoted() {
        let graph = plain_graph();
        let outcome = |wire: &str, policy: ValidationPolicy| {
            let registry = i0_registry(
                metadata_with_severity(PARSE_STATUS, wire),
                &[(PARSE_STATUS, ev_violation)],
            );
            let ctx = context(&graph, registry.metadata(), policy);
            let report = registry.evaluate_gate(GateId::I0, &graph, &ctx).unwrap();
            (
                rule_result(&report, PARSE_STATUS).state,
                report.result,
                report.summary,
            )
        };
        for wire in ["warn", "info"] {
            assert_eq!(
                outcome(wire, ValidationPolicy::default()),
                (RuleResultState::Warn, GateResult::Pass, summary(0, 0, 1))
            );
            // Promoted: the violation is a blocker FAIL, not a warning.
            assert_eq!(
                outcome(wire, policy(&[PARSE_STATUS], &[])),
                (RuleResultState::Fail, GateResult::Fail, summary(1, 0, 0))
            );
        }
        assert_eq!(
            outcome("error", ValidationPolicy::default()),
            (RuleResultState::Fail, GateResult::Pass, summary(0, 0, 0))
        );
    }

    #[test]
    fn waived_blocker_satisfies_the_gate_and_is_counted() {
        let graph = waiver_graph(
            PARSE_STATUS,
            vec![valid_waiver(PARSE_STATUS, "dec:waive-1")],
        );
        let report = evaluate_i0(
            &graph,
            ValidationPolicy::default(),
            &[
                (PARSE_STATUS, ev_violation),
                (HASH_MATCH, ev_not_applicable),
            ],
        )
        .unwrap();
        assert_eq!(report.result, GateResult::Pass);
        assert_eq!(report.summary, summary(0, 1, 0));

        // The same waiver next to another blocking failure: counted, gate still fails.
        let report = evaluate_i0(
            &graph,
            ValidationPolicy::default(),
            &[(PARSE_STATUS, ev_violation), (HASHABLE, ev_failure)],
        )
        .unwrap();
        assert_eq!(report.result, GateResult::Fail);
        assert_eq!(report.summary, summary(1, 1, 0));
        assert_eq!(GateSummary::of(&report.rules), report.summary);
        assert_eq!(report.summary.result(), GateResult::Fail);
    }

    // ------------------------------------------------------------------ registration (§58)

    #[test]
    fn empty_evaluator_registry_is_valid_and_reports_missing_bindings() {
        let registry = EvaluatorRegistry::new(metadata());
        assert_eq!(registry.metadata(), &metadata());
        for rule in &registry.metadata().profile().rules {
            assert!(!registry.has_evaluator(&rule.id));
        }
        assert_eq!(registry.missing_for_gate(GateId::I0), I0_RULES);
        for gate in GateId::ALL {
            assert_eq!(
                registry.missing_for_gate(gate).len(),
                registry.metadata().gate(gate).rule_ids.len()
            );
        }
        let graph = plain_graph();
        let ctx = context(&graph, registry.metadata(), ValidationPolicy::default());
        assert_eq!(
            registry.evaluate_gate(GateId::I0, &graph, &ctx),
            Err(EvaluationError::MissingGateEvaluators {
                gate: GateId::I0,
                rule_ids: strings(&I0_RULES),
            })
        );
    }

    #[test]
    fn evaluator_registration_is_validated() {
        let mut registry = EvaluatorRegistry::new(metadata());
        assert_eq!(
            registry.register("ORG.I0.UNKNOWN", ev_pass),
            Err(EvaluationError::UnknownEvaluatorRule(
                "ORG.I0.UNKNOWN".to_owned()
            ))
        );
        // Every rule of every deferred gate is refused.
        let deferred: Vec<(String, GateId)> = registry
            .metadata()
            .deferred_rule_ids()
            .into_iter()
            .map(|id| (id.to_owned(), registry.metadata().rule(id).unwrap().gate))
            .collect();
        assert_eq!(deferred.len(), 79);
        for (rule_id, gate) in deferred {
            assert_eq!(
                registry.register(&rule_id, ev_pass),
                Err(EvaluationError::EvaluatorForDeferredRule { rule_id, gate })
            );
        }
        assert!(registry.missing_for_gate(GateId::Q1).len() == 8);

        assert_eq!(registry.register(HASHABLE, ev_pass), Ok(()));
        assert!(registry.has_evaluator(HASHABLE));
        assert_eq!(
            registry.register(HASHABLE, ev_violation),
            Err(EvaluationError::DuplicateEvaluator(HASHABLE.to_owned()))
        );
        // Every NOW rule can be bound exactly once.
        let now: Vec<String> = registry
            .metadata()
            .required_now_rule_ids()
            .into_iter()
            .filter(|id| *id != HASHABLE)
            .map(str::to_owned)
            .collect();
        for rule_id in &now {
            assert_eq!(registry.register(rule_id, ev_pass), Ok(()));
        }
        for gate in NOW_GATES {
            assert!(registry.missing_for_gate(gate).is_empty());
        }
    }

    #[test]
    fn partially_bound_gate_reports_exactly_the_remaining_rules() {
        let graph = plain_graph();
        let mut registry = EvaluatorRegistry::new(metadata());
        // Bound out of order; the missing list stays in rule ID order.
        for rule_id in [PARSE_STATUS, HASHABLE, LOCATABLE] {
            registry.register(rule_id, ev_pass).unwrap();
        }
        let remaining = [HASH_MATCH, CONTENT_ADDRESSED, AGENT_IDENTIFIED];
        assert_eq!(registry.missing_for_gate(GateId::I0), remaining);
        let ctx = context(&graph, registry.metadata(), ValidationPolicy::default());
        assert_eq!(
            registry.evaluate_gate(GateId::I0, &graph, &ctx),
            Err(EvaluationError::MissingGateEvaluators {
                gate: GateId::I0,
                rule_ids: strings(&remaining),
            })
        );
    }

    #[test]
    fn complete_gate_evaluates_while_other_gates_are_unbound() {
        let graph = plain_graph();
        let registry = i0_registry(metadata(), &[]);
        assert!(registry.missing_for_gate(GateId::I0).is_empty());
        for gate in [GateId::F1, GateId::F2, GateId::F3, GateId::F4] {
            assert_eq!(
                registry.missing_for_gate(gate).len(),
                registry.metadata().gate(gate).rule_ids.len()
            );
        }
        let ctx = context(&graph, registry.metadata(), ValidationPolicy::default());
        let report = registry.evaluate_gate(GateId::I0, &graph, &ctx).unwrap();
        assert_eq!(report.gate, GateId::I0);
        assert_eq!(report.result, GateResult::Pass);
        let evaluated: Vec<&str> = report.rules.iter().map(|r| r.rule_id.as_str()).collect();
        assert_eq!(evaluated, I0_RULES);

        // F1 is a NOW gate without bindings: exactly its eleven rules are missing.
        match registry.evaluate_gate(GateId::F1, &graph, &ctx) {
            Err(EvaluationError::MissingGateEvaluators { gate, rule_ids }) => {
                assert_eq!(gate, GateId::F1);
                assert_eq!(rule_ids, registry.metadata().gate(GateId::F1).rule_ids);
                assert_eq!(rule_ids.len(), 11);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn later_gates_are_not_implemented() {
        let graph = plain_graph();
        let registry = i0_registry(metadata(), &[]);
        let ctx = context(&graph, registry.metadata(), ValidationPolicy::default());
        for gate in GateId::ALL {
            if NOW_GATES.contains(&gate) {
                continue;
            }
            assert_eq!(
                registry.evaluate_gate(gate, &graph, &ctx),
                Err(EvaluationError::Metadata(
                    ValidationError::GateNotImplemented(gate)
                ))
            );
        }
    }

    #[test]
    fn no_production_rule_evaluator_exists() {
        let rules = include_str!("../src/rules/mod.rs");
        assert!(!rules.contains("fn "));
        assert!(!rules.contains("mod "));
        let registry = metadata();
        let prefixes: BTreeSet<&str> = registry
            .profile()
            .rules
            .iter()
            .map(|r| r.id.split('.').next().unwrap())
            .collect();
        for rule in &registry.profile().rules {
            assert!(!rules.contains(rule.id.as_str()));
        }
        for prefix in prefixes {
            assert!(!rules.contains(&format!("{prefix}.")), "{prefix}");
        }
        // The framework itself binds nothing.
        let fresh = EvaluatorRegistry::new(metadata());
        assert_eq!(fresh.metadata().required_now_rule_ids().len(), 54);
        assert!(fresh
            .metadata()
            .required_now_rule_ids()
            .iter()
            .all(|id| !fresh.has_evaluator(id)));
    }

    // ------------------------------------------------------------------ GateReport golden (§59)

    /// The fixed graph of the golden scenario: the waived PARSE_STATUS finding and its waiver.
    fn golden_graph() -> Graph {
        let finding = id(GOLDEN_WAIVED_FINDING);
        let key: Hash = GOLDEN_WAIVED_KEY.parse().unwrap();
        build_graph(
            vec![
                finding_node(&finding, "Accepted"),
                decision(
                    "dec:waive-parse",
                    "Accepted",
                    marker(PARSE_STATUS, &key, &finding),
                ),
            ],
            vec![resolves(
                "edge:waive-parse",
                "Accepted",
                "dec:waive-parse",
                &finding,
            )],
        )
    }

    fn golden_report() -> GateReport {
        let graph = golden_graph();
        let mut registry = EvaluatorRegistry::new(metadata());
        bind_gate(&mut registry, GateId::I0, ev_golden, &[]);
        let ctx = ValidationContext::new(
            &graph,
            registry.metadata(),
            policy(&[AGENT_IDENTIFIED], &[AGENT_IDENTIFIED]),
            vec![external_artifact(json!({"ok": true}))],
        )
        .unwrap();
        registry.evaluate_gate(GateId::I0, &graph, &ctx).unwrap()
    }

    #[test]
    fn gate_report_matches_the_hand_authored_golden() {
        assert_eq!(
            golden_graph().semantic_hash().unwrap().as_str(),
            GOLDEN_BASELINE
        );
        let report = golden_report();
        assert_eq!(canonical(&report), GOLDEN_REPORT);
        let hash = report.content_hash().unwrap();
        assert_eq!(hash.kind(), HashKind::Generic);
        assert_eq!(hash.as_str(), GOLDEN_REPORT_HASH);
        assert_eq!(hash, Hash::content_sha256(GOLDEN_REPORT.as_bytes()));

        // Repeated equal evaluation is byte-identical.
        let again = golden_report();
        assert_eq!(to_canonical_json(&again).unwrap(), GOLDEN_REPORT.as_bytes());
        assert_eq!(again.content_hash().unwrap(), hash);

        // The golden parses back into the same report.
        assert_eq!(
            serde_json::from_str::<GateReport>(GOLDEN_REPORT).unwrap(),
            report
        );

        assert_eq!(report.gate, GateId::I0);
        assert_eq!(report.profile_hash.as_str(), PROFILE_HASH);
        assert_eq!(report.rule_pack_hash.as_str(), RULE_PACK_HASH);
        assert_eq!(
            report.policy.policy_hash().unwrap().as_str(),
            GOLDEN_POLICY_HASH
        );
        assert_eq!(report.result, GateResult::Fail);
        assert_eq!(report.summary, summary(3, 1, 0));
        let states: Vec<RuleResultState> = report.rules.iter().map(|r| r.state).collect();
        assert_eq!(
            states,
            [
                RuleResultState::Pass,
                RuleResultState::NotApplicable,
                RuleResultState::Error,
                RuleResultState::Fail,
                RuleResultState::Waived,
                RuleResultState::Fail,
            ]
        );
    }

    #[test]
    fn gate_report_is_normalized_and_has_no_operational_fields() {
        let report = golden_report();
        assert!(report.rules.windows(2).all(|w| w[0].rule_id < w[1].rule_id));
        assert!(report.findings.windows(2).all(|w| w[0].id < w[1].id));
        assert_eq!(report.findings.len(), 3);
        assert_eq!(report.waivers.len(), 1);
        let value: Value = serde_json::from_str(GOLDEN_REPORT).unwrap();
        let keys: BTreeSet<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "baseline_semantic_hash",
                "findings",
                "gate",
                "policy",
                "profile_hash",
                "profile_id",
                "result",
                "rule_pack_hash",
                "rules",
                "summary",
                "validation_artifact_refs",
                "waivers"
            ])
        );
        for forbidden in [
            "evaluated_at",
            "timestamp",
            "readiness",
            "branch",
            "revision",
        ] {
            assert!(!GOLDEN_REPORT.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn gate_report_deserialization_rejects_inconsistent_reports() {
        let golden: Value = serde_json::from_str(GOLDEN_REPORT).unwrap();
        let rejects = |mutate: &dyn Fn(&mut Value)| {
            let mut value = golden.clone();
            mutate(&mut value);
            serde_json::from_value::<GateReport>(value).is_err()
        };
        assert!(rejects(&|v| v["evaluated_at"] = json!(T1)));
        assert!(rejects(&|v| v["result"] = json!("PASS")));
        assert!(rejects(&|v| v["result"] = json!("WARN")));
        assert!(rejects(&|v| v["summary"]["blocker_failed"] = json!(2)));
        assert!(rejects(&|v| v["summary"]["readiness"] = json!(0.9)));
        assert!(rejects(&|v| v["rules"].as_array_mut().unwrap().reverse()));
        assert!(rejects(&|v| v["findings"]
            .as_array_mut()
            .unwrap()
            .reverse()));
        assert!(rejects(&|v| {
            v["findings"].as_array_mut().unwrap().pop();
        }));
        assert!(rejects(&|v| v["waivers"] = json!([])));
        assert!(rejects(&|v| {
            let refs = v["validation_artifact_refs"].as_array_mut().unwrap();
            let first = refs[0].clone();
            refs.push(first);
        }));
        assert!(rejects(
            &|v| v["baseline_semantic_hash"] = json!(PROFILE_HASH)
        ));
        assert!(rejects(&|v| v["profile_hash"] = json!(GOLDEN_BASELINE)));
        // Rule-result shape: a PASS cannot carry a finding, an ERROR needs its error, ...
        assert!(rejects(
            &|v| v["rules"][0]["finding_ref"] = json!("fnd:0000000000000000")
        ));
        assert!(rejects(&|v| v["rules"][0]["extra"] = json!(1)));
        assert!(rejects(
            &|v| v["rules"][1]["targets"] = json!(["src:hr-policy"])
        ));
        assert!(rejects(&|v| v["rules"][2]["error"] = json!(null)));
        assert!(rejects(&|v| v["rules"][3]["state"] = json!("WARN")));
        assert!(rejects(
            &|v| v["rules"][3]["semantic_condition_key"] = json!(null)
        ));
        assert!(rejects(&|v| v["rules"][4]["waiver_ref"] = json!(null)));
        assert!(rejects(
            &|v| v["rules"][5]["effective_severity"] = json!("warn")
        ));
        assert!(rejects(&|v| v["rules"][0]["state"] = json!("WAIVED")));
    }

    // ------------------------------------------------------------------ external artifacts (§60)

    #[test]
    fn external_artifact_refs_are_deterministic_and_reported_in_order() {
        let same = external_artifact(json!({"ok": true}));
        assert_eq!(same.artifact_hash().unwrap().as_str(), GOLDEN_ARTIFACT_REF);
        assert_eq!(
            external_artifact(json!({"ok": true}))
                .artifact_hash()
                .unwrap(),
            same.artifact_hash().unwrap()
        );
        assert_eq!(
            same.artifact_hash().unwrap(),
            Hash::content_sha256(&to_canonical_json(&same).unwrap())
        );
        let changed = external_artifact(json!({"ok": false}));
        assert_ne!(
            changed.artifact_hash().unwrap(),
            same.artifact_hash().unwrap()
        );

        let graph = plain_graph();
        let registry = i0_registry(metadata(), &[]);
        let artifacts = vec![
            same.clone(),
            changed.clone(),
            external_artifact(json!({"ok": true, "warnings": 2})),
        ];
        let report_for = |input: Vec<ExternalValidationArtifact>| {
            let ctx = ValidationContext::new(
                &graph,
                registry.metadata(),
                ValidationPolicy::default(),
                input,
            )
            .unwrap();
            registry.evaluate_gate(GateId::I0, &graph, &ctx).unwrap()
        };
        let forward = report_for(artifacts.clone());
        let mut reversed = artifacts.clone();
        reversed.reverse();
        let backward = report_for(reversed);
        assert_eq!(canonical(&forward), canonical(&backward));
        assert_eq!(
            forward.content_hash().unwrap(),
            backward.content_hash().unwrap()
        );

        let mut expected: Vec<Hash> = artifacts
            .iter()
            .map(|a| a.artifact_hash().unwrap())
            .collect();
        expected.sort();
        assert_eq!(forward.validation_artifact_refs, expected);

        // The artifacts are inputs: they change the report only through their refs.
        let without = report_for(vec![]);
        assert!(without.validation_artifact_refs.is_empty());
        assert_eq!(without.rules, forward.rules);
        assert_ne!(
            without.content_hash().unwrap(),
            forward.content_hash().unwrap()
        );
    }

    // ------------------------------------------------------------------ source guards (§61)

    const FRAMEWORK_SOURCES: [(&str, &str); 6] = [
        ("lib.rs", include_str!("../src/lib.rs")),
        ("evaluator.rs", include_str!("../src/evaluator.rs")),
        ("finding.rs", include_str!("../src/finding.rs")),
        ("waiver.rs", include_str!("../src/waiver.rs")),
        ("rules/mod.rs", include_str!("../src/rules/mod.rs")),
        ("external.rs", include_str!("../src/external.rs")),
    ];

    #[test]
    fn evaluator_source_has_no_clock_network_store_or_compiler_capability() {
        for (name, source) in FRAMEWORK_SOURCES {
            for forbidden in [
                "SystemClock",
                "Utc::now",
                "Instant::now",
                "reqwest",
                "InferenceProvider",
                "ArtifactStore",
                "SqliteRevisionStore",
                "plumb_compiler",
                "readiness",
                "Clock",
                "Timestamp",
                "plumb_store",
                "plumb_inference",
            ] {
                assert!(!source.contains(forbidden), "{name} mentions {forbidden}");
            }
        }
        let manifest = include_str!("../Cargo.toml");
        for dependency in [
            "plumb-compiler",
            "plumb-store",
            "plumb-inference",
            "reqwest",
        ] {
            assert!(!manifest.contains(dependency), "{dependency}");
        }
    }
}
