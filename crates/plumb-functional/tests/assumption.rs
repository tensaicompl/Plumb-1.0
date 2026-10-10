//! S3.4 contract tests for governed assumptions and their expiry (Hotfix 048).
//!
//! Every test lives in `assumption_contract` (`cargo test -p plumb-functional --test
//! assumption`). Graphs are synthetic. Golden IDs were computed independently with Python
//! `hashlib` over RFC 8785 JSON (and the finding-key bytes), never with the helpers under test.

mod assumption_contract {
    use std::cell::Cell;
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, Clock, FixedClock, Hash, Id, StageId, Timestamp};
    use plumb_functional::assumption::*;
    use plumb_patch::{apply_patch, AcceptancePolicy, ProposalMateriality, SemanticPatch};
    use plumb_psg::{
        Agent, AgentKind, Assumption, AuditMeta, ElementStatus, Entity, Finding, FindingSeverity,
        Graph, Node, NodePayload, Stakeholder,
    };
    use serde_json::{json, Value as JsonValue};

    use ElementStatus::{Accepted, Proposed, Suspect};

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const EXPIRES: &str = "2026-06-30T12:00:00.000000000Z";
    const HUMAN: &str = "agent:human";
    const BLOCKER: &str = "fnd:00000000000000b1";
    const ERROR: &str = "fnd:00000000000000e1";

    // Independently computed goldens (Python hashlib, RFC 8785).
    const GOLDEN_ASSUMPTION: &str = "assumption:7014a41fe8932fe2";
    const GOLDEN_EXPIRY_KEY: &str =
        "sha256:87db33e4dbee8dfc1a8d852404fb97b5ddc46e82d1f1ba3623819c1838a1dd3a";
    const GOLDEN_EXPIRY_FINDING: &str = "fnd:87db33e4dbee8dfc";

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
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

    fn agent(node_id: &str, status: ElementStatus, kind: AgentKind) -> Node {
        node(
            node_id,
            status,
            NodePayload::Agent(Agent { agent_kind: kind }),
        )
    }

    fn stakeholder(node_id: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            status,
            NodePayload::Stakeholder(Stakeholder {
                name: node_id.into(),
                stakeholder_kind: "internal".into(),
                organization: None,
                responsibilities: None,
                contact_ref: None,
            }),
        )
    }

    fn finding(node_id: &str, status: ElementStatus, severity: FindingSeverity) -> Node {
        node(
            node_id,
            status,
            NodePayload::Finding(Finding {
                code: "PLUMB.F2.CALC.TYPECHECK".into(),
                family: "F2".into(),
                severity,
                message: "open".into(),
                status: "Open".into(),
                affected_refs: vec![id("entity:order")],
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            }),
        )
    }

    fn base_nodes() -> Vec<Node> {
        vec![
            agent(HUMAN, Accepted, AgentKind::Human),
            agent("agent:service", Accepted, AgentKind::SoftwareService),
            agent("agent:llm", Accepted, AgentKind::LlmModel),
            agent("agent:intern", Proposed, AgentKind::Human),
            stakeholder("stakeholder:hr", Accepted),
            stakeholder("stakeholder:old", Suspect),
            node(
                "entity:order",
                Accepted,
                NodePayload::Entity(Entity {
                    name: "Order".into(),
                    description: None,
                    aggregate_root: None,
                }),
            ),
            finding(BLOCKER, Accepted, FindingSeverity::Blocker),
            finding(ERROR, Accepted, FindingSeverity::Error),
            finding("fnd:00000000000000d1", Proposed, FindingSeverity::Error),
        ]
    }

    fn graph_of(nodes: Vec<Node>) -> Graph {
        Graph::new(
            id("project:pilot"),
            id("profile:plumb-software-2026.1"),
            nodes,
            vec![],
        )
        .unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn graph() -> Graph {
        graph_of(base_nodes())
    }

    fn input() -> AssumptionInput {
        AssumptionInput {
            statement: "Leave is counted in working days.".into(),
            owner_ref: id("stakeholder:hr"),
            finding_ref: Some(id(BLOCKER)),
            default_value: Some(json!({"unit": "working_day", "values": [1, 0.5]})),
            expires_at: Some(ts(EXPIRES)),
            risk_ref: None,
        }
    }

    fn audit() -> AssumptionAudit {
        AssumptionAudit {
            created_by: id(HUMAN),
            created_at: ts("2026-02-01T08:00:00.000000000Z"),
        }
    }

    fn create(
        graph: &Graph,
        input: &AssumptionInput,
    ) -> Result<AssumptionCreation, AssumptionError> {
        create_assumption(graph, input, &audit())
    }

    fn new_of(
        result: Result<AssumptionCreation, AssumptionError>,
    ) -> (Id, Assumption, plumb_patch::Proposal) {
        match result.unwrap() {
            AssumptionCreation::New {
                assumption_ref,
                assumption,
                proposal,
            } => (assumption_ref, assumption, *proposal),
            other => panic!("expected New, got {other:?}"),
        }
    }

    fn assumption_node(
        node_id: &str,
        status: ElementStatus,
        payload_status: &str,
        expires_at: Option<&str>,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Assumption(Assumption {
                statement: format!("Assumption {node_id}."),
                owner_ref: id("stakeholder:hr"),
                status: payload_status.into(),
                finding_ref: None,
                default_value: None,
                expires_at: expires_at.map(ts),
                risk_ref: None,
            }),
        )
    }

    // ------------------------------------------------------------------ creation validation

    #[test]
    fn statement_rules() {
        let g = graph();
        assert!(create(&g, &input()).is_ok());
        for bad in ["", " padded", "padded ", "line\nbreak", "tab\tinside"] {
            let i = AssumptionInput {
                statement: bad.into(),
                ..input()
            };
            assert_eq!(
                create(&g, &i),
                Err(AssumptionError::InvalidStatement),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn owner_rules() {
        let g = graph();
        for owner in [HUMAN, "stakeholder:hr"] {
            assert!(create(
                &g,
                &AssumptionInput {
                    owner_ref: id(owner),
                    ..input()
                }
            )
            .is_ok());
        }
        assert_eq!(
            create(
                &g,
                &AssumptionInput {
                    owner_ref: id("agent:ghost"),
                    ..input()
                }
            ),
            Err(AssumptionError::OwnerMissing(id("agent:ghost")))
        );
        assert_eq!(
            create(
                &g,
                &AssumptionInput {
                    owner_ref: id("entity:order"),
                    ..input()
                }
            ),
            Err(AssumptionError::OwnerWrongType(id("entity:order")))
        );
        assert_eq!(
            create(
                &g,
                &AssumptionInput {
                    owner_ref: id("stakeholder:old"),
                    ..input()
                }
            ),
            Err(AssumptionError::OwnerNotAccepted(id("stakeholder:old")))
        );
        assert_eq!(
            create(
                &g,
                &AssumptionInput {
                    owner_ref: id("agent:intern"),
                    ..input()
                }
            ),
            Err(AssumptionError::OwnerNotAccepted(id("agent:intern")))
        );
    }

    #[test]
    fn acceptance_actor_rules() {
        let g = graph();
        for actor in [
            "agent:service",
            "agent:llm",
            "agent:intern",
            "agent:ghost",
            "stakeholder:hr",
        ] {
            let a = AssumptionAudit {
                created_by: id(actor),
                ..audit()
            };
            assert_eq!(
                create_assumption(&g, &input(), &a),
                Err(AssumptionError::InvalidAcceptanceActor(id(actor))),
                "{actor}"
            );
        }
        // Owner and accepting actor may differ.
        assert!(create(
            &g,
            &AssumptionInput {
                owner_ref: id("stakeholder:hr"),
                ..input()
            }
        )
        .is_ok());
    }

    #[test]
    fn finding_link_and_blocking_expiry() {
        let g = graph();
        assert!(create(
            &g,
            &AssumptionInput {
                finding_ref: None,
                expires_at: None,
                ..input()
            }
        )
        .is_ok());
        assert!(create(
            &g,
            &AssumptionInput {
                finding_ref: Some(id(ERROR)),
                expires_at: None,
                ..input()
            }
        )
        .is_ok());
        assert_eq!(
            create(
                &g,
                &AssumptionInput {
                    finding_ref: Some(id(BLOCKER)),
                    expires_at: None,
                    ..input()
                }
            ),
            Err(AssumptionError::BlockingAssumptionRequiresExpiry {
                finding_ref: id(BLOCKER)
            })
        );
        assert!(create(
            &g,
            &AssumptionInput {
                finding_ref: Some(id(BLOCKER)),
                ..input()
            }
        )
        .is_ok());
        assert_eq!(
            create(
                &g,
                &AssumptionInput {
                    finding_ref: Some(id("fnd:0000000000000000")),
                    ..input()
                }
            ),
            Err(AssumptionError::FindingMissing(id("fnd:0000000000000000")))
        );
        assert_eq!(
            create(
                &g,
                &AssumptionInput {
                    finding_ref: Some(id("entity:order")),
                    ..input()
                }
            ),
            Err(AssumptionError::FindingWrongType(id("entity:order")))
        );
        assert_eq!(
            create(
                &g,
                &AssumptionInput {
                    finding_ref: Some(id("fnd:00000000000000d1")),
                    ..input()
                }
            ),
            Err(AssumptionError::FindingNotAccepted(id(
                "fnd:00000000000000d1"
            )))
        );
    }

    #[test]
    fn risk_ref_is_unsupported() {
        let g = graph();
        let (_, assumption, _) = new_of(create(&g, &input()));
        assert_eq!(assumption.risk_ref, None);
        // Independent of whether the supplied ID exists or what it names.
        for risk in ["risk:any-valid-id", "entity:order", "risk:missing"] {
            let i = AssumptionInput {
                risk_ref: Some(id(risk)),
                ..input()
            };
            assert_eq!(
                create(&g, &i),
                Err(AssumptionError::RiskReferenceUnsupported { risk_ref: id(risk) }),
                "{risk}"
            );
        }
    }

    #[test]
    fn default_value_and_status_are_exact() {
        let g = graph();
        let (_, assumption, proposal) = new_of(create(&g, &input()));
        assert_eq!(assumption.default_value, input().default_value);
        assert_eq!(assumption.status, "Accepted");
        let SemanticPatch::AddNode { node } = &proposal.patch_set.patch else {
            panic!()
        };
        let NodePayload::Assumption(persisted) = &node.payload else {
            panic!()
        };
        assert_eq!(
            to_canonical_json(&persisted.default_value).unwrap(),
            to_canonical_json(&input().default_value).unwrap()
        );
    }

    // ------------------------------------------------------------------ identity

    #[test]
    fn assumption_identity() {
        let project = id("project:pilot");
        let base = assumption_id(&project, &input()).unwrap();
        assert_eq!(base.to_string(), GOLDEN_ASSUMPTION);
        let (created, _, _) = new_of(create(&graph(), &input()));
        assert_eq!(created, base);
        let variants = [
            AssumptionInput {
                statement: "Other.".into(),
                ..input()
            },
            AssumptionInput {
                owner_ref: id(HUMAN),
                ..input()
            },
            AssumptionInput {
                finding_ref: Some(id(ERROR)),
                ..input()
            },
            AssumptionInput {
                default_value: Some(json!(1)),
                ..input()
            },
            AssumptionInput {
                default_value: None,
                ..input()
            },
            AssumptionInput {
                expires_at: Some(ts("2026-07-01T00:00:00.000000000Z")),
                ..input()
            },
        ];
        for v in variants {
            assert_ne!(assumption_id(&project, &v).unwrap(), base);
        }
        assert_ne!(assumption_id(&id("project:other"), &input()).unwrap(), base);
        // Audit is not identity.
        let later = AssumptionAudit {
            created_by: id(HUMAN),
            created_at: ts("2026-05-05T00:00:00.000000000Z"),
        };
        let (other, _, _) = new_of(create_assumption(&graph(), &input(), &later));
        assert_eq!(other, base);
        let human2 = graph_of({
            let mut n = base_nodes();
            n.push(agent("agent:other-human", Accepted, AgentKind::Human));
            n
        });
        let (other, _, _) = new_of(create_assumption(
            &human2,
            &input(),
            &AssumptionAudit {
                created_by: id("agent:other-human"),
                ..audit()
            },
        ));
        assert_eq!(other, base);
        // Insertion order does not matter.
        let mut reversed = base_nodes();
        reversed.reverse();
        let (r, _, _) = new_of(create(&graph_of(reversed), &input()));
        assert_eq!(r, base);
    }

    // ------------------------------------------------------------------ reconciliation and proposal

    #[test]
    fn existing_assumption_reconciliation() {
        let g = graph();
        let (aid, assumption, proposal) = new_of(create(&g, &input()));
        let applied = apply_patch(&g, &proposal.patch_set).unwrap().graph;
        assert_eq!(
            create(&applied, &input()),
            Ok(AssumptionCreation::Existing {
                assumption_ref: aid.clone()
            })
        );
        let with = |n: Node| {
            graph_of({
                let mut v = base_nodes();
                v.push(n);
                v
            })
        };
        let mut different = assumption.clone();
        different.statement = "Tampered.".into();
        let collisions = [
            node(aid.as_str(), Accepted, NodePayload::Assumption(different)),
            node(
                aid.as_str(),
                Proposed,
                NodePayload::Assumption(assumption.clone()),
            ),
            stakeholder(aid.as_str(), Accepted),
        ];
        for c in collisions {
            assert_eq!(
                create(&with(c), &input()),
                Err(AssumptionError::AssumptionIdCollision(aid.clone()))
            );
        }
    }

    #[test]
    fn assumption_proposal_shape() {
        let g = graph();
        let (aid, _, proposal) = new_of(create(&g, &input()));
        assert_eq!(proposal.stage, StageId::S3);
        assert_eq!(proposal.materiality, ProposalMateriality::MaterialDecision);
        assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanDecision);
        assert_eq!(proposal.confidence, None);
        assert_eq!(
            proposal.patch_set.base_semantic_hash,
            g.semantic_hash().unwrap()
        );
        let SemanticPatch::AddNode { node } = &proposal.patch_set.patch else {
            panic!("not a single AddNode")
        };
        assert_eq!(node.id, aid);
        assert_eq!((node.status, node.revision), (Accepted, 1));
        assert!(
            node.evidence.is_empty() && node.derivations.is_empty() && node.standards.is_empty()
        );
        assert!(node.tags.is_empty() && node.extensions.is_empty());
        assert_eq!(
            node.audit,
            AuditMeta::new(id(HUMAN), ts("2026-02-01T08:00:00.000000000Z"), None, None).unwrap()
        );
        let applied = apply_patch(&g, &proposal.patch_set).unwrap().graph;
        assert_ne!(applied.semantic_hash().unwrap(), g.semantic_hash().unwrap());
    }

    // ------------------------------------------------------------------ expiry

    struct CountingClock {
        times: Vec<Timestamp>,
        calls: Cell<usize>,
    }

    impl Clock for CountingClock {
        fn now(&self) -> Timestamp {
            let i = self.calls.get();
            self.calls.set(i + 1);
            self.times[i.min(self.times.len() - 1)]
        }
    }

    fn expiry_graph(extra: Vec<Node>) -> Graph {
        let mut n = base_nodes();
        n.extend(extra);
        graph_of(n)
    }

    fn findings_at(graph: &Graph, now: &str) -> Vec<plumb_validation::GeneratedFinding> {
        analyze_assumption_expiry(graph, &FixedClock::new(ts(now)))
            .unwrap()
            .findings
    }

    #[test]
    fn expiry_boundary() {
        let g = expiry_graph(vec![
            assumption_node(
                "assumption:00000000000000a1",
                Accepted,
                "Accepted",
                Some(EXPIRES),
            ),
            assumption_node("assumption:00000000000000a2", Accepted, "Accepted", None),
        ]);
        assert!(findings_at(&g, "2026-06-30T11:59:59.999999999Z").is_empty());
        let at = findings_at(&g, EXPIRES);
        assert_eq!(at.len(), 1);
        assert_eq!(
            at[0].payload.affected_refs,
            vec![id("assumption:00000000000000a1")]
        );
        assert_eq!(findings_at(&g, "2027-01-01T00:00:00.000000000Z"), at);
    }

    #[test]
    fn expiry_scope() {
        let mut nodes = Vec::new();
        for (i, status) in ["Open", "Resolved", "Expired", "Superseded"]
            .iter()
            .enumerate()
        {
            nodes.push(assumption_node(
                &format!("assumption:00000000000000b{i}"),
                Accepted,
                status,
                Some(AT),
            ));
        }
        nodes.push(assumption_node(
            "assumption:00000000000000c1",
            Proposed,
            "Accepted",
            Some(AT),
        ));
        nodes.push(assumption_node(
            "assumption:00000000000000c2",
            Suspect,
            "Accepted",
            Some(AT),
        ));
        let g = expiry_graph(nodes);
        assert!(findings_at(&g, EXPIRES).is_empty());
        let bad = expiry_graph(vec![assumption_node(
            "assumption:00000000000000d9",
            Accepted,
            "accepted",
            Some(AT),
        )]);
        assert_eq!(
            analyze_assumption_expiry(&bad, &FixedClock::new(ts(EXPIRES))),
            Err(AssumptionError::InvalidAssumptionStatus {
                assumption_ref: id("assumption:00000000000000d9"),
                status: "accepted".into()
            })
        );
    }

    #[test]
    fn expiry_finding_material() {
        let aref = id("assumption:00000000000000a1");
        let g = expiry_graph(vec![assumption_node(
            aref.as_str(),
            Accepted,
            "Accepted",
            Some(EXPIRES),
        )]);
        let f = findings_at(&g, "2026-12-01T00:00:00.000000000Z").remove(0);
        assert_eq!(f.payload.code, "PLUMB.S3.ASSUMPTION.EXPIRED");
        assert_eq!(f.payload.family, "S3");
        assert_eq!(f.payload.severity, FindingSeverity::Blocker);
        assert_eq!(f.payload.status, "Open");
        assert_eq!(f.payload.affected_refs, vec![aref.clone()]);
        assert_eq!(f.payload.standard_rule_ref, None);
        assert_eq!(f.payload.waiver_ref, None);
        assert_eq!(
            f.payload.suggested_resolution.as_deref(),
            Some("Review, resolve, supersede or explicitly replace the expired assumption.")
        );
        assert_eq!(
            f.payload.message,
            format!("Accepted assumption {aref} expired at {EXPIRES}.")
        );
        assert_eq!(
            f.semantic_condition_key,
            format!("assumption_expired:{aref}:{EXPIRES}")
        );
        assert_eq!(f.key.as_str(), GOLDEN_EXPIRY_KEY);
        assert_eq!(f.id.as_str(), GOLDEN_EXPIRY_FINDING);
        // `now` is not identity.
        let later = findings_at(&g, "2030-01-01T00:00:00.000000000Z").remove(0);
        assert_eq!((later.key, later.id), (f.key.clone(), f.id.clone()));
        // Analysis never mutates the assumption (it is a pure read of the graph).
        let NodePayload::Assumption(a) = &g.node(&aref).unwrap().payload else {
            panic!()
        };
        assert_eq!(a.status, "Accepted");
    }

    #[test]
    fn expiry_ordering_determinism_and_single_clock_read() {
        let nodes = vec![
            assumption_node(
                "assumption:00000000000000a1",
                Accepted,
                "Accepted",
                Some(AT),
            ),
            assumption_node(
                "assumption:00000000000000a2",
                Accepted,
                "Accepted",
                Some(EXPIRES),
            ),
            assumption_node(
                "assumption:00000000000000a3",
                Accepted,
                "Accepted",
                Some("2026-03-01T00:00:00.000000000Z"),
            ),
        ];
        let g = expiry_graph(nodes.clone());
        let found = findings_at(&g, "2026-12-31T00:00:00.000000000Z");
        assert_eq!(found.len(), 3);
        assert!(found.windows(2).all(|w| w[0].id < w[1].id));
        let mut reversed = base_nodes();
        reversed.extend(nodes);
        reversed.reverse();
        assert_eq!(
            findings_at(&graph_of(reversed), "2026-12-31T00:00:00.000000000Z"),
            found
        );
        // A clock answering differently on each call is read exactly once: the first instant
        // (before every expiry) decides for all assumptions.
        let clock = CountingClock {
            times: vec![
                ts("2025-01-01T00:00:00.000000000Z"),
                ts("2030-01-01T00:00:00.000000000Z"),
            ],
            calls: Cell::new(0),
        };
        let result = analyze_assumption_expiry(&g, &clock).unwrap();
        assert_eq!(clock.calls.get(), 1);
        assert!(result.findings.is_empty());
    }

    #[test]
    fn expiry_finding_helper_matches_analysis() {
        let aref = id("assumption:00000000000000a1");
        let direct = expiry_finding(&aref, &ts(EXPIRES)).unwrap();
        assert_eq!(
            direct.semantic_condition_key,
            expiry_condition_key(&aref, &ts(EXPIRES))
        );
        let _: Hash = direct.key.clone();
        let _: JsonValue = json!(null);
    }

    #[test]
    fn production_source_guard() {
        let source = include_str!("../src/assumption.rs");
        for token in [
            "std::fs",
            "File::open",
            "reqwest",
            "std::net",
            "SystemTime",
            "Utc::now",
            "Instant::now",
            "SystemClock",
            "rand::",
            "uuid",
            "ulid",
            "Provider",
            "plumb_inference",
            "include_str!",
            "fixtures/",
            ".name ==",
            "prompt",
            "unsafe",
            "NodeType::Constraint",
            "NodeType::ArchitectureDecision",
            "NodeType::Extension",
            "extension_type",
        ] {
            assert!(!source.contains(token), "assumption.rs contains {token}");
        }
    }
}
