//! S3.4 contract tests for governed waiver creation (Hotfix 048).
//!
//! Every test lives in `waiver_contract` (`cargo test -p plumb-validation --test waiver`). The
//! pure material builder is plumb-validation's; the proposal wrapper is plumb-functional's (a
//! dev-dependency only). Real rules come from the built-in pack and the I0 gate is evaluated by
//! the unchanged F0.13 evaluator. Golden IDs were computed independently with Python `hashlib`
//! over RFC 8785 JSON, never with the helpers under test.

mod waiver_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, GateId, Hash, Id, Timestamp};
    use plumb_functional::waiver::{create_waiver, WaiverError};
    use plumb_patch::{apply_patch, AcceptancePolicy, ProposalMateriality, SemanticPatch};
    use plumb_psg::{
        Agent, AgentKind, AuditMeta, Edge, ElementStatus, Finding, FindingSeverity, Graph, Node,
        NodePayload, RelationKind, RelationProperties, ResolutionDecision,
    };
    use plumb_validation::waiver::*;
    use plumb_validation::*;
    use serde_json::json;

    use ElementStatus::{Accepted, Proposed, Superseded};

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const DECIDED_AT: &str = "2026-04-01T09:00:00.000000000Z";
    const HUMAN: &str = "agent:human";
    const CONDITION: &str = "fixture-condition";
    const TARGET: &str = "src:hr-policy";

    const DECISION_REQUIRED: &str = "PLUMB.I0.SOURCE.PARSE_STATUS";
    const FORBIDDEN: &str = "PLUMB.I0.SOURCE.CONTENT_ADDRESSED";
    const PROFILE_ALLOW: &str = "PPMN.I0.PROVENANCE.AGENT_IDENTIFIED";
    const RATIONALE: &str = "Parsing is not required for this archived source.";

    // Independently computed goldens (Python hashlib, RFC 8785).
    const GOLDEN_SUBJECT: &str = "waiver-subject:14a0d2dcf9b40037";
    const GOLDEN_DECISION: &str = "dec:88f9b1296cd3e92f";
    const GOLDEN_EDGE: &str = "rel:ba25526636af08f7";

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn meta() -> AuditMeta {
        AuditMeta::new(id("agent:analyst"), ts(AT), None, None).unwrap()
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
            audit: meta(),
        }
    }

    fn metadata() -> ValidationRegistry {
        ValidationRegistry::new(load_builtin_software_profile().unwrap()).unwrap()
    }

    fn rule(rule_id: &str) -> RuleMetadata {
        metadata().rule(rule_id).unwrap().clone()
    }

    fn policy(profile_allow: &[&str]) -> ValidationPolicy {
        ValidationPolicy {
            promoted_to_blocker_rule_ids: BTreeSet::new(),
            profile_allow_waiver_rule_ids: profile_allow.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn targets() -> Vec<Id> {
        vec![id(TARGET)]
    }

    /// The deterministic finding of the fixture violation of `rule_id`.
    fn fixture_finding(rule_id: &str) -> (Hash, Id) {
        let key = finding_key(rule_id, &targets(), CONDITION).unwrap();
        let fid = finding_id(&key).unwrap();
        (key, fid)
    }

    fn finding_node(rule_id: &str) -> Node {
        let (_, fid) = fixture_finding(rule_id);
        node(
            fid.as_str(),
            Accepted,
            NodePayload::Finding(Finding {
                code: rule_id.into(),
                family: "I0".into(),
                severity: FindingSeverity::Blocker,
                message: format!("{rule_id} is violated."),
                status: "Open".into(),
                affected_refs: targets(),
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            }),
        )
    }

    fn base_nodes(rule_ids: &[&str]) -> Vec<Node> {
        let mut nodes = vec![
            node(
                HUMAN,
                Accepted,
                NodePayload::Agent(Agent {
                    agent_kind: AgentKind::Human,
                }),
            ),
            node(
                "agent:service",
                Accepted,
                NodePayload::Agent(Agent {
                    agent_kind: AgentKind::SoftwareService,
                }),
            ),
            node(
                "agent:llm",
                Accepted,
                NodePayload::Agent(Agent {
                    agent_kind: AgentKind::LlmModel,
                }),
            ),
            node(
                "agent:intern",
                Proposed,
                NodePayload::Agent(Agent {
                    agent_kind: AgentKind::Human,
                }),
            ),
        ];
        nodes.extend(rule_ids.iter().map(|r| finding_node(r)));
        nodes
    }

    fn graph_of(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(
            id("project:pilot"),
            id("profile:plumb-software-2026.1"),
            nodes,
            edges,
        )
        .unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn graph(rule_ids: &[&str]) -> Graph {
        graph_of(base_nodes(rule_ids), vec![])
    }

    fn request<'a>(rule: &'a RuleMetadata, targets: &'a [Id]) -> WaiverRequest<'a> {
        WaiverRequest {
            rule,
            targets,
            semantic_condition_key: CONDITION,
            decided_by: &HUMAN_ID,
            decided_at: ts(DECIDED_AT),
            rationale: RATIONALE,
        }
    }

    static HUMAN_ID: std::sync::LazyLock<Id> = std::sync::LazyLock::new(|| id(HUMAN));

    // ------------------------------------------------------------------ real I0 evaluation

    type Outcome = Result<RuleEvaluation, EvaluatorFailure>;

    fn ev_pass(_: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
        Ok(RuleEvaluation::pass(vec![], vec![]).unwrap())
    }

    fn ev_violation(_: &Graph, _: &ValidationContext, rule: &RuleMetadata) -> Outcome {
        Ok(RuleEvaluation::violation(
            vec![id(TARGET)],
            vec![],
            CONDITION.to_owned(),
            format!("{} is violated.", rule.id),
            None,
        )
        .unwrap())
    }

    /// The I0 gate with `violated` violating and every other rule passing.
    fn evaluate(
        graph: &Graph,
        violated: &str,
        policy: ValidationPolicy,
    ) -> Result<GateReport, EvaluationError> {
        let mut registry = EvaluatorRegistry::new(metadata());
        for rule_id in registry.metadata().gate(GateId::I0).rule_ids.clone() {
            let evaluator: RuleEvaluator = if rule_id == violated {
                ev_violation
            } else {
                ev_pass
            };
            registry.register(&rule_id, evaluator).unwrap();
        }
        let ctx = ValidationContext::new(
            graph,
            registry.metadata(),
            policy,
            graph.evidence_hash().unwrap(),
            vec![],
            vec![],
        )
        .unwrap();
        registry.evaluate_gate(GateId::I0, graph, &ctx)
    }

    fn state(report: &GateReport, rule_id: &str) -> RuleResultState {
        report
            .rules
            .iter()
            .find(|r| r.rule_id == rule_id)
            .unwrap()
            .state
    }

    fn applied(graph: &Graph, proposal: &plumb_patch::Proposal) -> Graph {
        apply_patch(graph, &proposal.patch_set).unwrap().graph
    }

    #[test]
    fn decision_required_waiver_is_applied_by_the_real_evaluator() {
        let r = rule(DECISION_REQUIRED);
        assert_eq!(r.waiver_policy, WaiverPolicy::DecisionRequired);
        let g = graph(&[DECISION_REQUIRED]);
        let before = evaluate(&g, DECISION_REQUIRED, policy(&[])).unwrap();
        assert_eq!(state(&before, DECISION_REQUIRED), RuleResultState::Fail);
        let t = targets();
        let created = create_waiver(&g, &request(&r, &t), &policy(&[])).unwrap();
        let after_graph = applied(&g, &created.proposal);
        let after = evaluate(&after_graph, DECISION_REQUIRED, policy(&[])).unwrap();
        assert_eq!(state(&after, DECISION_REQUIRED), RuleResultState::Waived);
        assert_eq!(after.waivers.len(), 1);
        assert_eq!(after.waivers[0].decision_ref, created.decision_ref);
        let (_, fid) = fixture_finding(DECISION_REQUIRED);
        let generated = after.findings.iter().find(|f| f.id == fid).unwrap();
        assert_eq!(
            generated.payload.waiver_ref,
            Some(created.decision_ref.clone())
        );
        // The direct consumer agrees.
        let (key, _) = fixture_finding(DECISION_REQUIRED);
        let waiver = governed_waiver(&after_graph, &r, &policy(&[]), &key, &fid, &t)
            .unwrap()
            .unwrap();
        assert_eq!(waiver.decision_ref, created.decision_ref);
    }

    #[test]
    fn forbidden_waiver_fails_before_any_proposal() {
        let r = rule(FORBIDDEN);
        assert_eq!(r.waiver_policy, WaiverPolicy::Forbidden);
        let g = graph(&[FORBIDDEN]);
        let before = evaluate(&g, FORBIDDEN, policy(&[])).unwrap();
        assert_eq!(state(&before, FORBIDDEN), RuleResultState::Fail);
        let t = targets();
        assert_eq!(
            prepare_waiver_material(&g, &request(&r, &t), &policy(&[])),
            Err(WaiverCreationError::ForbiddenWaiver {
                rule_id: FORBIDDEN.into()
            })
        );
        assert!(matches!(
            create_waiver(&g, &request(&r, &t), &policy(&[])),
            Err(WaiverError::Material(
                WaiverCreationError::ForbiddenWaiver { .. }
            ))
        ));
        let again = evaluate(&g, FORBIDDEN, policy(&[])).unwrap();
        assert_eq!(state(&again, FORBIDDEN), RuleResultState::Fail);
        assert_eq!(again, before);
    }

    #[test]
    fn profile_allow_waiver_needs_the_enabled_policy() {
        let r = rule(PROFILE_ALLOW);
        assert_eq!(r.waiver_policy, WaiverPolicy::ProfileAllow);
        let g = graph(&[PROFILE_ALLOW]);
        let t = targets();
        assert_eq!(
            prepare_waiver_material(&g, &request(&r, &t), &policy(&[])),
            Err(WaiverCreationError::ProfileWaiverNotEnabled {
                rule_id: PROFILE_ALLOW.into()
            })
        );
        let disabled = evaluate(&g, PROFILE_ALLOW, policy(&[])).unwrap();
        assert_eq!(state(&disabled, PROFILE_ALLOW), RuleResultState::Fail);
        let enabled = policy(&[PROFILE_ALLOW]);
        let created = create_waiver(&g, &request(&r, &t), &enabled).unwrap();
        let after = evaluate(&applied(&g, &created.proposal), PROFILE_ALLOW, enabled).unwrap();
        assert_eq!(state(&after, PROFILE_ALLOW), RuleResultState::Waived);
        assert_eq!(after.waivers[0].decision_ref, created.decision_ref);
    }

    // ------------------------------------------------------------------ identity and request checks

    #[test]
    fn finding_identity_is_derived_and_checked() {
        let r = rule(DECISION_REQUIRED);
        let g = graph(&[DECISION_REQUIRED]);
        let t = targets();
        let material = prepare_waiver_material(&g, &request(&r, &t), &policy(&[])).unwrap();
        let (key, fid) = fixture_finding(DECISION_REQUIRED);
        assert_eq!(
            (
                material.artifact.finding_key.clone(),
                material.artifact.finding_ref.clone()
            ),
            (key, fid.clone())
        );
        // A different condition is a different (missing) finding.
        let other = WaiverRequest {
            semantic_condition_key: "other-condition",
            ..request(&r, &t)
        };
        assert!(matches!(
            prepare_waiver_material(&g, &other, &policy(&[])),
            Err(WaiverCreationError::FindingMissing(_))
        ));
        // Target order does not matter; duplicates are rejected by the finding-key contract.
        let two = vec![id("src:b"), id(TARGET)];
        let reversed = vec![id(TARGET), id("src:b")];
        assert_eq!(
            finding_key(DECISION_REQUIRED, &two, CONDITION).unwrap(),
            finding_key(DECISION_REQUIRED, &reversed, CONDITION).unwrap()
        );
        let duplicated = vec![id(TARGET), id(TARGET)];
        assert!(matches!(
            prepare_waiver_material(&g, &request(&r, &duplicated), &policy(&[])),
            Err(WaiverCreationError::InvalidFindingKey { .. })
        ));
        // Persisted Finding mismatches.
        let edit = |f: fn(&mut Finding)| {
            let mut nodes = base_nodes(&[]);
            let mut n = finding_node(DECISION_REQUIRED);
            if let NodePayload::Finding(p) = &mut n.payload {
                f(p);
            }
            nodes.push(n);
            graph_of(nodes, vec![])
        };
        for g in [
            edit(|p| p.code = "PLUMB.I0.EVIDENCE.LOCATABLE".into()),
            edit(|p| p.family = "F1".into()),
            edit(|p| p.affected_refs = vec!["src:other".parse().unwrap()]),
        ] {
            assert!(matches!(
                prepare_waiver_material(&g, &request(&r, &t), &policy(&[])),
                Err(WaiverCreationError::RuleFindingMismatch { .. })
            ));
        }
        let wrong_type = graph_of(
            {
                let mut n = base_nodes(&[]);
                n.push(node(
                    fid.as_str(),
                    Accepted,
                    NodePayload::Agent(Agent {
                        agent_kind: AgentKind::Human,
                    }),
                ));
                n
            },
            vec![],
        );
        assert!(matches!(
            prepare_waiver_material(&wrong_type, &request(&r, &t), &policy(&[])),
            Err(WaiverCreationError::FindingWrongType(_))
        ));
        let not_accepted = graph_of(
            {
                let mut n = base_nodes(&[]);
                let mut f = finding_node(DECISION_REQUIRED);
                f.status = Proposed;
                n.push(f);
                n
            },
            vec![],
        );
        assert!(matches!(
            prepare_waiver_material(&not_accepted, &request(&r, &t), &policy(&[])),
            Err(WaiverCreationError::FindingNotAccepted(_))
        ));
        assert!(matches!(
            prepare_waiver_material(&graph(&[]), &request(&r, &t), &policy(&[])),
            Err(WaiverCreationError::FindingMissing(_))
        ));
    }

    #[test]
    fn actor_and_rationale_rules() {
        let r = rule(DECISION_REQUIRED);
        let g = graph(&[DECISION_REQUIRED]);
        let t = targets();
        for actor in ["agent:service", "agent:llm", "agent:intern", "agent:ghost"] {
            let a = id(actor);
            let req = WaiverRequest {
                decided_by: &a,
                ..request(&r, &t)
            };
            assert_eq!(
                prepare_waiver_material(&g, &req, &policy(&[])),
                Err(WaiverCreationError::InvalidWaiverActor(a.clone()))
            );
        }
        for bad in ["", "   ", " padded", "padded ", "bell\u{7}"] {
            let req = WaiverRequest {
                rationale: bad,
                ..request(&r, &t)
            };
            assert_eq!(
                prepare_waiver_material(&g, &req, &policy(&[])),
                Err(WaiverCreationError::InvalidWaiverRationale),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn marker_subject_artifact_and_decision_goldens() {
        let r = rule(DECISION_REQUIRED);
        let g = graph(&[DECISION_REQUIRED]);
        let t = targets();
        let m = prepare_waiver_material(&g, &request(&r, &t), &policy(&[])).unwrap();
        let (key, fid) = fixture_finding(DECISION_REQUIRED);
        let expected_marker = json!({"kind": "waiver", "rule_id": DECISION_REQUIRED, "finding_key": key.as_str(), "finding_ref": fid.as_str()});
        assert_eq!(m.decision.answer, expected_marker);
        let parsed: WaiverDecisionMarker =
            serde_json::from_value(m.decision.answer.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), expected_marker);
        let mut extra = expected_marker.clone();
        extra["owner"] = json!("x");
        assert!(serde_json::from_value::<WaiverDecisionMarker>(extra).is_err());
        assert_eq!(m.waiver_subject_ref.to_string(), GOLDEN_SUBJECT);
        assert_eq!(
            waiver_subject_ref(&id("project:pilot"), &fid, &key, DECISION_REQUIRED).unwrap(),
            m.waiver_subject_ref
        );
        assert_ne!(
            waiver_subject_ref(&id("project:other"), &fid, &key, DECISION_REQUIRED).unwrap(),
            m.waiver_subject_ref
        );
        assert_ne!(
            waiver_subject_ref(
                &id("project:pilot"),
                &id("fnd:0000000000000001"),
                &key,
                DECISION_REQUIRED
            )
            .unwrap(),
            m.waiver_subject_ref
        );
        assert_ne!(
            waiver_subject_ref(&id("project:pilot"), &fid, &key, FORBIDDEN).unwrap(),
            m.waiver_subject_ref
        );
        assert!(g.node(&m.waiver_subject_ref).is_none());
        // Artifact.
        assert_eq!(
            m.artifact,
            WaiverPatchArtifact {
                version: 1,
                finding_ref: fid.clone(),
                finding_key: key.clone(),
                rule_id: DECISION_REQUIRED.into()
            }
        );
        assert_eq!(m.artifact_bytes, to_canonical_json(&m.artifact).unwrap());
        assert_eq!(
            Hash::content_sha256(&m.artifact_bytes),
            m.decision.patch_ref
        );
        let text = String::from_utf8(m.artifact_bytes.clone()).unwrap();
        assert!(!text.contains(m.decision_ref.as_str()));
        // Decision ID.
        assert_eq!(m.decision_ref.to_string(), GOLDEN_DECISION);
        let base = |subject: &Id, answer: &serde_json::Value, by: &Id, at: &str, patch: &Hash| {
            waiver_decision_id(&id("project:pilot"), subject, answer, by, &ts(at), patch).unwrap()
        };
        let human = id(HUMAN);
        assert_eq!(
            base(
                &m.waiver_subject_ref,
                &expected_marker,
                &human,
                DECIDED_AT,
                &m.decision.patch_ref
            ),
            m.decision_ref
        );
        let mut other_marker = expected_marker.clone();
        other_marker["rule_id"] = json!(FORBIDDEN);
        for other in [
            base(
                &id("waiver-subject:0000000000000001"),
                &expected_marker,
                &human,
                DECIDED_AT,
                &m.decision.patch_ref,
            ),
            base(
                &m.waiver_subject_ref,
                &other_marker,
                &human,
                DECIDED_AT,
                &m.decision.patch_ref,
            ),
            base(
                &m.waiver_subject_ref,
                &expected_marker,
                &id("agent:other"),
                DECIDED_AT,
                &m.decision.patch_ref,
            ),
            base(
                &m.waiver_subject_ref,
                &expected_marker,
                &human,
                AT,
                &m.decision.patch_ref,
            ),
            base(
                &m.waiver_subject_ref,
                &expected_marker,
                &human,
                DECIDED_AT,
                &Hash::content_sha256(b"x"),
            ),
        ] {
            assert_ne!(other, m.decision_ref);
        }
        let reworded = WaiverRequest {
            rationale: "Another clean reason.",
            ..request(&r, &t)
        };
        assert_eq!(
            prepare_waiver_material(&g, &reworded, &policy(&[]))
                .unwrap()
                .decision_ref,
            m.decision_ref
        );
        assert_eq!(m.resolves_edge.id.to_string(), GOLDEN_EDGE);
    }

    #[test]
    fn governance_material_and_layer_boundary() {
        let r = rule(DECISION_REQUIRED);
        let g = graph(&[DECISION_REQUIRED]);
        let t = targets();
        let m = prepare_waiver_material(&g, &request(&r, &t), &policy(&[])).unwrap();
        let (_, fid) = fixture_finding(DECISION_REQUIRED);
        let d: &ResolutionDecision = &m.decision;
        assert_eq!(d.question_ref, None);
        assert_eq!(d.proposal_ref, Some(m.waiver_subject_ref.clone()));
        assert_eq!(d.decided_by, id(HUMAN));
        assert_eq!(d.decided_at, ts(DECIDED_AT));
        assert_eq!(d.rationale.as_deref(), Some(RATIONALE));
        assert_eq!(d.supersedes, None);
        assert_eq!(
            (m.decision_node.status, m.decision_node.revision),
            (Accepted, 1)
        );
        assert_eq!(
            m.decision_node.payload,
            NodePayload::ResolutionDecision(d.clone())
        );
        assert_eq!(
            m.decision_node.audit,
            AuditMeta::new(id(HUMAN), ts(DECIDED_AT), None, None).unwrap()
        );
        assert!(
            m.decision_node.evidence.is_empty()
                && m.decision_node.derivations.is_empty()
                && m.decision_node.extensions.is_empty()
        );
        let e = &m.resolves_edge;
        assert_eq!(
            (
                e.kind.clone(),
                e.from.clone(),
                e.to.clone(),
                e.status,
                e.properties.clone()
            ),
            (
                RelationKind::Resolves,
                m.decision_ref.clone(),
                fid.clone(),
                Accepted,
                RelationProperties::None
            )
        );
        // The functional proposal holds exactly the prepared node and edge.
        let created = create_waiver(&g, &request(&r, &t), &policy(&[])).unwrap();
        let p = &created.proposal;
        assert_eq!(p.stage, plumb_core::StageId::S3);
        assert_eq!(p.materiality, ProposalMateriality::MaterialDecision);
        assert_eq!(p.acceptance_policy, AcceptancePolicy::HumanDecision);
        assert_eq!(p.confidence, None);
        let SemanticPatch::Compound { patches } = &p.patch_set.patch else {
            panic!()
        };
        assert_eq!(
            patches,
            &vec![
                SemanticPatch::AddNode {
                    node: m.decision_node.clone()
                },
                SemanticPatch::AddEdge {
                    edge: m.resolves_edge.clone()
                }
            ]
        );
        assert_eq!(created.decision_ref, m.decision_ref);
        assert_eq!(created.artifact_bytes, m.artifact_bytes);
        // Dry-run succeeds; the persisted Finding is unchanged; no Question is created.
        let after = applied(&g, p);
        assert_eq!(after.node(&fid), g.node(&fid));
        assert!(after
            .node_ids_by_type(plumb_psg::NodeType::Question)
            .is_empty());
        let incoming: Vec<&Edge> = after
            .incoming_edge_ids(&fid)
            .iter()
            .filter_map(|e| after.edge(e))
            .collect();
        assert_eq!(incoming.len(), 1);
        // Material is independent of graph insertion order.
        let mut nodes = base_nodes(&[DECISION_REQUIRED]);
        nodes.reverse();
        assert_eq!(
            prepare_waiver_material(&graph_of(nodes, vec![]), &request(&r, &t), &policy(&[]))
                .unwrap(),
            m
        );
    }

    // ------------------------------------------------------------------ ambiguity and isolation

    fn decision_node(node_id: &str, status: ElementStatus, answer: serde_json::Value) -> Node {
        node(
            node_id,
            status,
            NodePayload::ResolutionDecision(ResolutionDecision {
                question_ref: Some(id("q:0000000000000001")),
                proposal_ref: None,
                answer,
                decided_by: id(HUMAN),
                decided_at: ts(AT),
                patch_ref: Hash::content_sha256(b"other"),
                rationale: Some("Existing.".into()),
                supersedes: None,
            }),
        )
    }

    fn resolving(decision: Node, fid: &Id) -> Graph {
        let edge = Edge {
            id: id(&format!("rel:{}", &decision.id.as_str()[4..])),
            revision: 1,
            status: Accepted,
            kind: RelationKind::Resolves,
            from: decision.id.clone(),
            to: fid.clone(),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: meta(),
        };
        let mut nodes = base_nodes(&[DECISION_REQUIRED]);
        nodes.push(decision);
        graph_of(nodes, vec![edge])
    }

    #[test]
    fn existing_waivers_prevent_ambiguity() {
        let r = rule(DECISION_REQUIRED);
        let t = targets();
        let (key, fid) = fixture_finding(DECISION_REQUIRED);
        let marker = json!({"kind": "waiver", "rule_id": DECISION_REQUIRED, "finding_key": key.as_str(), "finding_ref": fid.as_str()});
        // An applied waiver blocks a second identical request.
        let g = graph(&[DECISION_REQUIRED]);
        let created = create_waiver(&g, &request(&r, &t), &policy(&[])).unwrap();
        let after = applied(&g, &created.proposal);
        let later = WaiverRequest {
            decided_at: ts("2026-05-01T00:00:00.000000000Z"),
            ..request(&r, &t)
        };
        assert_eq!(
            prepare_waiver_material(&after, &later, &policy(&[])),
            Err(WaiverCreationError::AlreadyWaived {
                finding_ref: fid.clone(),
                decision_ref: created.decision_ref.clone()
            })
        );
        assert!(matches!(
            prepare_waiver_material(
                &resolving(
                    decision_node("dec:00000000000000a1", Accepted, marker.clone()),
                    &fid
                ),
                &request(&r, &t),
                &policy(&[])
            ),
            Err(WaiverCreationError::AlreadyWaived { .. })
        ));
        // A different waiver marker conflicts.
        let mut different = marker.clone();
        different["finding_key"] = json!(Hash::content_sha256(b"other key").as_str());
        assert!(matches!(
            prepare_waiver_material(
                &resolving(
                    decision_node("dec:00000000000000a2", Accepted, different),
                    &fid
                ),
                &request(&r, &t),
                &policy(&[])
            ),
            Err(WaiverCreationError::ExistingWaiverConflict { .. })
        ));
        // Non-Accepted waiver decisions and S3.3 decisions do not count.
        assert!(prepare_waiver_material(
            &resolving(
                decision_node("dec:00000000000000a3", Superseded, marker.clone()),
                &fid
            ),
            &request(&r, &t),
            &policy(&[])
        )
        .is_ok());
        let s33 = json!({"kind": "domain_relationship_cardinality", "candidate_ref": "domainrel:x", "cardinality_from": "1", "cardinality_to": "1"});
        let g = resolving(decision_node("dec:00000000000000a4", Accepted, s33), &fid);
        assert!(prepare_waiver_material(&g, &request(&r, &t), &policy(&[])).is_ok());
    }

    #[test]
    fn s3_3_decisions_are_not_waivers() {
        let r = rule(DECISION_REQUIRED);
        let t = targets();
        let (key, fid) = fixture_finding(DECISION_REQUIRED);
        for answer in [
            json!({"kind": "domain_relationship_cardinality", "candidate_ref": "domainrel:x", "cardinality_from": "1", "cardinality_to": "1"}),
            json!({"kind": "state_transition_trigger", "candidate_ref": "transition:x", "trigger_ref": "operation:x"}),
            json!({"kind": "calculation_calendar", "calculation_ref": "calculation:x", "calendar_ref": "calendar:x"}),
        ] {
            let g = resolving(
                decision_node("dec:00000000000000b1", Accepted, answer),
                &fid,
            );
            assert_eq!(
                governed_waiver(&g, &r, &policy(&[]), &key, &fid, &t),
                Ok(None)
            );
            let report = evaluate(&g, DECISION_REQUIRED, policy(&[])).unwrap();
            assert_eq!(state(&report, DECISION_REQUIRED), RuleResultState::Fail);
            assert!(report.waivers.is_empty());
        }
    }

    #[test]
    fn production_source_guard() {
        let source = include_str!("../src/waiver.rs");
        for token in [
            "plumb_patch",
            "apply_patch",
            "SemanticPatch",
            "PatchSet",
            "Proposal::new",
            "std::fs",
            "File::open",
            "reqwest",
            "std::net",
            "SystemClock",
            "Utc::now",
            "Instant::now",
            "now()",
            "rand::",
            "uuid",
            "ulid",
            "plumb_inference",
            "include_str!",
            "fixtures/",
            "generate_questions",
            "resolve_question",
            "expires_at",
            "unsafe",
        ] {
            assert!(!source.contains(token), "waiver.rs contains {token}");
        }
        let functional = include_str!("../../plumb-functional/src/waiver.rs");
        for token in [
            "finding_key(",
            "finding_id(",
            "short_id",
            "sha256",
            "WaiverPolicy",
            "AgentKind",
            "is_clean_text",
        ] {
            assert!(
                !functional.contains(token),
                "functional waiver.rs recomputes {token}"
            );
        }
        let manifest = include_str!("../Cargo.toml");
        let production = manifest.split("[dev-dependencies]").next().unwrap();
        assert!(!production.contains("plumb-patch") && !production.contains("plumb-functional"));
    }
}
