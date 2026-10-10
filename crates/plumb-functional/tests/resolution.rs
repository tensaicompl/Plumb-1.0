//! S3.3 contract tests for ResolutionDecision and answer-to-patch application (Hotfix 047).
//!
//! Every test lives in `resolution_contract` (`cargo test -p plumb-functional --test
//! resolution`). Questions are produced by the real S3.1 engine over synthetic current-v3
//! graphs and the real HR routing fixture. The domain and lifecycle compatibility modules run the
//! existing S2.1/S2.2 analyses as oracles. Golden decision IDs were computed independently with
//! Python `hashlib` over RFC 8785 JSON, never with the helper under test.

mod resolution_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, Hash, Id, StageId, Timestamp};
    use plumb_functional::question::{
        generate_questions, QuestionAudit, QuestionGenerationInput, StakeholderRoutingConfig,
    };
    use plumb_functional::resolution::*;
    use plumb_patch::{
        apply_patch, AcceptancePolicy, PatchError, PatchSet, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        edge_element_hash, node_element_hash, Actor, ActorKind, Agent, AgentKind, Attribute,
        AuditMeta, BusinessRole, Calculation, Calendar, Edge, ElementStatus, Entity, Event,
        Finding, FindingSeverity, Graph, Invariant, Modality, Node, NodePayload, Operation,
        OperationKind, Permission, Question, QuestionKind, RelationKind, RelationProperties,
        Requirement, RequirementKind, RequirementLevel, ResourceScope, SecurityRole, Stakeholder,
    };
    use plumb_validation::expression_scope::{ExpressionBindingExposure, ExpressionScopeBinding};
    use plumb_validation::{finding_id, finding_key, GeneratedFinding};
    use serde_json::{json, Value as JsonValue};

    use ElementStatus::{Accepted, Deprecated, Proposed, Rejected, Superseded, Suspect};

    pub const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const DECIDED_AT: &str = "2026-03-01T09:30:00.000000000Z";
    const HR_STAKEHOLDERS: &str = include_str!("../../../fixtures/hr-leave/stakeholders.yaml");

    const HUMAN: &str = "agent:human";
    const CARDINALITY_CANDIDATE: &str = "domainrel:leave-employee";
    const TRIGGER_CANDIDATE: &str = "transition:approve";

    // Independently computed goldens (Python hashlib, RFC 8785).
    const GOLDEN_FIXED_CARDINALITY_DECISION: &str = "dec:4beb342f87c87936";
    const GOLDEN_FIXED_CALENDAR_DECISION: &str = "dec:bc3ea12a54014953";
    const GOLDEN_CARDINALITY_DECISION: &str = "dec:f1c317b958ec3028";
    const GOLDEN_CALENDAR_DECISION: &str = "dec:006bc21bba6e6598";
    const GOLDEN_PERFORMER_EDGE: &str = "rel:097fef862fa377e2";

    // ------------------------------------------------------------------ builders

    pub fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ids(list: &[&str]) -> Vec<Id> {
        list.iter().map(|s| id(s)).collect()
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

    fn edge_as(
        edge_id: &str,
        kind: RelationKind,
        from: &str,
        to: &str,
        status: ElementStatus,
    ) -> Edge {
        Edge {
            id: id(edge_id),
            revision: 1,
            status,
            kind,
            from: id(from),
            to: id(to),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: meta(),
        }
    }

    fn agent(node_id: &str, status: ElementStatus, kind: AgentKind) -> Node {
        node(
            node_id,
            status,
            NodePayload::Agent(Agent { agent_kind: kind }),
        )
    }

    fn operation(node_id: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            status,
            NodePayload::Operation(Operation {
                name: node_id.into(),
                operation_kind: OperationKind::Command,
                input_schema_ref: None,
                output_schema_ref: None,
                preconditions: None,
                postconditions: None,
                idempotency: None,
                transaction_semantics: None,
            }),
        )
    }

    fn actor(node_id: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            status,
            NodePayload::Actor(Actor {
                name: node_id.into(),
                actor_kind: ActorKind::Human,
            }),
        )
    }

    fn calendar(node_id: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            status,
            NodePayload::Calendar(Calendar {
                time_zone: "Europe/Amsterdam".into(),
                week_pattern: json!(["Mon", "Tue", "Wed", "Thu", "Fri"]),
                region: None,
                holiday_source: None,
            }),
        )
    }

    fn scope(node_id: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            status,
            NodePayload::ResourceScope(ResourceScope {
                resource_ref: id("entity:order"),
                scope_kind: "entity".into(),
            }),
        )
    }

    fn stakeholder(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Stakeholder(Stakeholder {
                name: node_id.into(),
                stakeholder_kind: "internal".into(),
                organization: None,
                responsibilities: None,
                contact_ref: None,
            }),
        )
    }

    /// The synthetic current-v3 graph every template can be asked against.
    #[derive(Clone)]
    struct Fx {
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    }

    impl Fx {
        fn new() -> Fx {
            Fx {
                nodes: vec![
                    agent(HUMAN, Accepted, AgentKind::Human),
                    agent("agent:compiler", Accepted, AgentKind::CompilerStage),
                    agent("agent:intern", Proposed, AgentKind::Human),
                    stakeholder("stakeholder:hr"),
                    stakeholder("stakeholder:architect"),
                    node(
                        "req:r1",
                        Accepted,
                        NodePayload::Requirement(Requirement {
                            statement: "The system shall record leave.".into(),
                            requirement_kind: RequirementKind::Functional,
                            level: RequirementLevel::System,
                            modality: Modality::Shall,
                            title: None,
                            rationale: None,
                            priority: None,
                            source_identifier: None,
                            verification_method: None,
                            owner_refs: None,
                            stakeholder_refs: None,
                        }),
                    ),
                    node(
                        "entity:order",
                        Accepted,
                        NodePayload::Entity(Entity {
                            name: "Order".into(),
                            description: None,
                            aggregate_root: None,
                        }),
                    ),
                    node(
                        "attr:amount",
                        Accepted,
                        NodePayload::Attribute(Attribute {
                            name: "amount".into(),
                            value_type: "Decimal(2)".into(),
                            nullable: false,
                            unit: None,
                            precision: None,
                            enum_values: None,
                            data_classification: None,
                        }),
                    ),
                    operation("operation:approve", Accepted),
                    operation("operation:submit", Accepted),
                    operation("operation:old", Suspect),
                    operation("operation:draft", Proposed),
                    actor("actor:clerk", Accepted),
                    actor("actor:draft", Proposed),
                    node(
                        "brole:manager",
                        Accepted,
                        NodePayload::BusinessRole(BusinessRole {
                            name: "Manager".into(),
                        }),
                    ),
                    node(
                        "secrole:approver",
                        Accepted,
                        NodePayload::SecurityRole(SecurityRole {
                            name: "Approver".into(),
                            description: None,
                        }),
                    ),
                    node(
                        "event:submitted",
                        Accepted,
                        NodePayload::Event(Event {
                            name: "Submitted".into(),
                            payload_schema_ref: None,
                            semantic_type: None,
                        }),
                    ),
                    node(
                        "calculation:days",
                        Accepted,
                        NodePayload::Calculation(Calculation {
                            name: "days".into(),
                            expression: "1".into(),
                            result_type: "Int".into(),
                            unit: None,
                            rounding: Some("HALF_UP(0)".into()),
                            calendar_ref: None,
                            examples: None,
                        }),
                    ),
                    calendar("calendar:nl", Accepted),
                    calendar("calendar:de", Accepted),
                    calendar("calendar:draft", Proposed),
                    node(
                        "permission:approve",
                        Accepted,
                        NodePayload::Permission(Permission {
                            name: "Approve".into(),
                        }),
                    ),
                    scope("scope:orders", Accepted),
                    scope("scope:hr", Accepted),
                    scope("scope:old", Suspect),
                    node(
                        "inv:positive",
                        Accepted,
                        NodePayload::Invariant(Invariant {
                            scope_ref: id("entity:order"),
                            expression: "amount".into(),
                        }),
                    ),
                ],
                edges: vec![
                    edge_as(
                        "rel:has-amount",
                        RelationKind::HasAttribute,
                        "entity:order",
                        "attr:amount",
                        Accepted,
                    ),
                    // Non-concrete permission: permits targets a Suspect Operation.
                    edge_as(
                        "rel:permits",
                        RelationKind::Permits,
                        "permission:approve",
                        "operation:old",
                        Accepted,
                    ),
                    edge_as(
                        "rel:scoped",
                        RelationKind::ScopedTo,
                        "permission:approve",
                        "scope:orders",
                        Accepted,
                    ),
                ],
            }
        }

        fn edit_edge(mut self, edge_id: &str, f: impl FnOnce(&mut Edge)) -> Fx {
            let e = self.edges.iter_mut().find(|e| e.id == id(edge_id)).unwrap();
            f(e);
            self
        }

        fn graph(&self) -> Graph {
            Graph::new(
                id("project:pilot"),
                id("profile:plumb-software-2026.1"),
                self.nodes.clone(),
                self.edges.clone(),
            )
            .unwrap_or_else(|v| panic!("{v:?}"))
        }
    }

    fn gf(
        code: &str,
        condition: &str,
        targets: &[&str],
        severity: FindingSeverity,
    ) -> GeneratedFinding {
        let mut affected = ids(targets);
        affected.sort();
        let key = finding_key(code, &affected, condition).unwrap();
        GeneratedFinding {
            id: finding_id(&key).unwrap(),
            key,
            semantic_condition_key: condition.into(),
            payload: Finding {
                code: code.into(),
                family: "F2".into(),
                severity,
                message: format!("{code} violated"),
                status: "Open".into(),
                affected_refs: affected,
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            },
        }
    }

    fn cardinality_finding() -> GeneratedFinding {
        gf(
            "PLUMB.F2.DOMAIN.RELATION_TYPED",
            &format!("domain_relationship_cardinality_unresolved:{CARDINALITY_CANDIDATE}"),
            &["req:r1"],
            FindingSeverity::Error,
        )
    }
    fn trigger_finding() -> GeneratedFinding {
        gf(
            "PLUMB.F2.STATE.TRANSITION_COMPLETE",
            &format!("state_transition_trigger_unresolved:{TRIGGER_CANDIDATE}"),
            &["req:r1"],
            FindingSeverity::Blocker,
        )
    }
    fn performer_finding() -> GeneratedFinding {
        gf(
            "PLUMB.F2.OPERATION.PERFORMER",
            "operation_performer_missing",
            &["operation:approve", "operation:submit"],
            FindingSeverity::Blocker,
        )
    }
    fn calendar_finding() -> GeneratedFinding {
        gf(
            "PLUMB.F2.TIME.CALENDAR_DEFINED",
            "business_time_calendar_missing",
            &["calculation:days"],
            FindingSeverity::Blocker,
        )
    }
    fn permission_finding() -> GeneratedFinding {
        gf(
            "RBAC.F2.PERMISSION.CONCRETE",
            "permission_not_concrete",
            &["permission:approve"],
            FindingSeverity::Blocker,
        )
    }
    fn invariant_finding() -> GeneratedFinding {
        gf(
            "PLUMB.F2.INVARIANT.EXPRESSIBLE",
            "invariant_not_expressible",
            &["inv:positive"],
            FindingSeverity::Error,
        )
    }

    /// Runs the real S3.1 engine and applies its proposal: the Findings and Questions exist.
    fn ask(graph: &Graph, findings: Vec<GeneratedFinding>) -> Graph {
        let generated = generate_questions(
            graph,
            &QuestionGenerationInput {
                findings,
                routing: StakeholderRoutingConfig::from_yaml(HR_STAKEHOLDERS).unwrap(),
            },
            &QuestionAudit {
                created_by: id("agent:compiler"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        assert!(generated.issues.is_empty(), "{:?}", generated.issues);
        apply_patch(graph, &generated.proposal.unwrap().patch_set)
            .unwrap()
            .graph
    }

    fn all_findings() -> Vec<GeneratedFinding> {
        vec![
            cardinality_finding(),
            trigger_finding(),
            performer_finding(),
            calendar_finding(),
            permission_finding(),
            invariant_finding(),
        ]
    }

    fn asked() -> Graph {
        ask(&Fx::new().graph(), all_findings())
    }

    /// The Question about `subject` (context_refs[0]).
    fn question_about(graph: &Graph, subject: &str) -> Id {
        graph
            .nodes()
            .values()
            .find(|n| {
                matches!(&n.payload, NodePayload::Question(q)
                    if q.context_refs.as_ref().is_some_and(|c| c[0] == id(subject)))
            })
            .unwrap_or_else(|| panic!("no question about {subject}"))
            .id
            .clone()
    }

    fn context() -> ResolutionContext {
        ResolutionContext {
            invariant_bindings: BTreeMap::from([(
                id("inv:positive"),
                vec![ExpressionScopeBinding {
                    node_ref: id("attr:amount"),
                    symbol: "amount".into(),
                    exposure: ExpressionBindingExposure::Root,
                }],
            )]),
        }
    }

    fn input(question: Id, answer: JsonValue, rationale: Option<&str>) -> ResolutionInput {
        ResolutionInput {
            question_ref: question,
            answer,
            decided_by: id(HUMAN),
            decided_at: ts(DECIDED_AT),
            rationale: rationale.map(Into::into),
            supersedes: None,
        }
    }

    fn resolve(
        graph: &Graph,
        input: &ResolutionInput,
    ) -> Result<ResolutionResult, ResolutionError> {
        resolve_question(graph, input, &context())
    }

    /// The answer of each template on the asked graph, with a valid rationale.
    fn good(graph: &Graph, template: ResolutionTemplate) -> ResolutionInput {
        use ResolutionTemplate::*;
        let (subject, answer, rationale) = match template {
            DomainRelationshipCardinality => (
                CARDINALITY_CANDIDATE,
                json!({"cardinality_from": "1", "cardinality_to": "0..*"}),
                Some("Confirmed with HR."),
            ),
            StateTransitionTrigger => (
                TRIGGER_CANDIDATE,
                json!({"trigger_ref": "operation:approve"}),
                Some("The approval operation triggers it."),
            ),
            OperationPerformer => (
                "operation:approve",
                json!({"performer_ref": "actor:clerk"}),
                None,
            ),
            CalculationCalendar => (
                "calculation:days",
                json!({"calendar_ref": "calendar:nl"}),
                None,
            ),
            PermissionBinding => (
                "permission:approve",
                json!({"operation_ref": "operation:approve", "resource_scope_ref": "scope:orders"}),
                None,
            ),
            InvariantFormula => ("inv:positive", json!({"expression": "amount >= 0"}), None),
        };
        input(question_about(graph, subject), answer, rationale)
    }

    const TEMPLATES: [ResolutionTemplate; 6] = [
        ResolutionTemplate::DomainRelationshipCardinality,
        ResolutionTemplate::StateTransitionTrigger,
        ResolutionTemplate::OperationPerformer,
        ResolutionTemplate::CalculationCalendar,
        ResolutionTemplate::PermissionBinding,
        ResolutionTemplate::InvariantFormula,
    ];

    fn leaves(result: &ResolutionResult) -> Vec<SemanticPatch> {
        let SemanticPatch::Compound { patches } = &result.proposal.patch_set.patch else {
            panic!("not a Compound")
        };
        patches.clone()
    }

    fn flat(patch: &SemanticPatch) -> Vec<SemanticPatch> {
        match patch {
            SemanticPatch::Compound { patches } => patches.iter().flat_map(flat).collect(),
            other => vec![other.clone()],
        }
    }

    fn applied(graph: &Graph, result: &ResolutionResult) -> Graph {
        apply_patch(graph, &result.proposal.patch_set)
            .unwrap()
            .graph
    }

    fn question_payload(graph: &Graph, question: &Id) -> Question {
        match &graph.node(question).unwrap().payload {
            NodePayload::Question(q) => q.clone(),
            _ => unreachable!(),
        }
    }

    fn finding_status(graph: &Graph, finding: &Id) -> String {
        match &graph.node(finding).unwrap().payload {
            NodePayload::Finding(f) => f.status.clone(),
            _ => unreachable!(),
        }
    }

    /// Rebuilds a graph with edited nodes and edges.
    fn rebuild(graph: &Graph, f: impl FnOnce(&mut Vec<Node>, &mut Vec<Edge>)) -> Graph {
        let mut nodes: Vec<Node> = graph.nodes().values().cloned().collect();
        let mut edges: Vec<Edge> = graph.edges().values().cloned().collect();
        f(&mut nodes, &mut edges);
        Graph::new(
            graph.project_id().clone(),
            graph.profile_id().clone(),
            nodes,
            edges,
        )
        .unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn edit_node(graph: &Graph, node_id: &Id, f: impl FnOnce(&mut Node)) -> Graph {
        rebuild(graph, |nodes, _| {
            f(nodes.iter_mut().find(|n| &n.id == node_id).unwrap());
        })
    }

    fn edit_question(graph: &Graph, question: &Id, f: impl FnOnce(&mut Question)) -> Graph {
        edit_node(graph, question, |n| {
            if let NodePayload::Question(q) = &mut n.payload {
                f(q);
            }
        })
    }

    // ------------------------------------------------------------------ template identity

    #[test]
    fn template_identity_is_the_exact_code_kind_pair() {
        use ResolutionTemplate::*;
        let pairs = [
            (
                "PLUMB.F2.DOMAIN.RELATION_TYPED",
                QuestionKind::Cardinality,
                DomainRelationshipCardinality,
            ),
            (
                "PLUMB.F2.STATE.TRANSITION_COMPLETE",
                QuestionKind::PickOne,
                StateTransitionTrigger,
            ),
            (
                "PLUMB.F2.OPERATION.PERFORMER",
                QuestionKind::RoleAssignment,
                OperationPerformer,
            ),
            (
                "PLUMB.F2.TIME.CALENDAR_DEFINED",
                QuestionKind::Calendar,
                CalculationCalendar,
            ),
            (
                "RBAC.F2.PERMISSION.CONCRETE",
                QuestionKind::Permission,
                PermissionBinding,
            ),
            (
                "PLUMB.F2.INVARIANT.EXPRESSIBLE",
                QuestionKind::FormulaConfirm,
                InvariantFormula,
            ),
        ];
        for (code, kind, template) in pairs {
            assert_eq!(ResolutionTemplate::of(code, kind), Some(template));
        }
        assert_eq!(
            ResolutionTemplate::of("PLUMB.F2.DOMAIN.RELATION_TYPED", QuestionKind::PickOne),
            None
        );
        assert_eq!(
            ResolutionTemplate::of("PLUMB.F2.CALC.TYPECHECK", QuestionKind::FormulaConfirm),
            None
        );
        let g = asked();
        let q = question_about(&g, "inv:positive");
        let g = edit_question(&g, &q, |p| p.question_kind = QuestionKind::Text);
        assert!(matches!(
            resolve(&g, &input(q, json!({"expression": "amount >= 0"}), None)),
            Err(ResolutionError::UnsupportedResolutionTemplate { .. })
        ));
    }

    // ------------------------------------------------------------------ eligibility

    #[test]
    fn every_template_resolves_on_the_asked_graph() {
        let g = asked();
        for template in TEMPLATES {
            let result =
                resolve(&g, &good(&g, template)).unwrap_or_else(|e| panic!("{template:?}: {e}"));
            assert_eq!(result.template, template);
            applied(&g, &result).validate().unwrap();
        }
    }

    #[test]
    fn question_eligibility_errors() {
        let g = asked();
        let q = question_about(&g, "inv:positive");
        let answer = || input(q.clone(), json!({"expression": "amount >= 0"}), None);
        // A round_ref is preserved and does not matter.
        let rounded = edit_question(&g, &q, |p| p.round_ref = Some(id("rnd:00000000000000aa")));
        let r = resolve(&rounded, &answer()).unwrap();
        assert_eq!(
            question_payload(&applied(&rounded, &r), &q).round_ref,
            Some(id("rnd:00000000000000aa"))
        );
        let missing = ResolutionInput {
            question_ref: id("q:0000000000000000"),
            ..answer()
        };
        assert!(matches!(
            resolve(&g, &missing),
            Err(ResolutionError::QuestionMissing(_))
        ));
        let wrong = ResolutionInput {
            question_ref: id("operation:approve"),
            ..answer()
        };
        assert!(matches!(
            resolve(&g, &wrong),
            Err(ResolutionError::NotAQuestion(_))
        ));
        let proposed = edit_node(&g, &q, |n| n.status = Proposed);
        assert!(matches!(
            resolve(&proposed, &answer()),
            Err(ResolutionError::QuestionNotAccepted(_))
        ));
        for status in ["Closed", "Superseded", "Answered"] {
            let g = edit_question(&g, &q, |p| p.status = status.into());
            assert!(
                matches!(
                    resolve(&g, &answer()),
                    Err(ResolutionError::QuestionNotOpen { .. })
                ),
                "{status}"
            );
        }
        let none = edit_question(&g, &q, |p| p.context_refs = None);
        assert!(matches!(
            resolve(&none, &answer()),
            Err(ResolutionError::MissingQuestionContext(_))
        ));
        let empty = edit_question(&g, &q, |p| p.context_refs = Some(vec![]));
        assert!(matches!(
            resolve(&empty, &answer()),
            Err(ResolutionError::MissingQuestionContext(_))
        ));
        let no_schema = edit_question(&g, &q, |p| p.answer_schema = None);
        assert!(matches!(
            resolve(&no_schema, &answer()),
            Err(ResolutionError::InvalidQuestionContract { .. })
        ));
    }

    #[test]
    fn finding_currentness_errors() {
        let g = asked();
        let q = question_about(&g, "inv:positive");
        let f = invariant_finding().id;
        let answer = input(q.clone(), json!({"expression": "amount >= 0"}), None);
        let edit_finding = |edit: fn(&mut Finding)| {
            edit_node(&g, &f, |n| {
                if let NodePayload::Finding(p) = &mut n.payload {
                    edit(p);
                }
            })
        };
        assert!(matches!(
            resolve(&edit_finding(|p| p.status = "Closed".into()), &answer),
            Err(ResolutionError::FindingNotOpen { .. })
        ));
        assert!(matches!(
            resolve(
                &edit_finding(|p| p.waiver_ref = Some("dec:waiver".parse().unwrap())),
                &answer
            ),
            Err(ResolutionError::FindingWaived(_))
        ));
        assert!(matches!(
            resolve(&edit_node(&g, &f, |n| n.status = Suspect), &answer),
            Err(ResolutionError::FindingNotAccepted(_))
        ));
        assert!(matches!(
            resolve(
                &edit_node(&g, &id("inv:positive"), |n| n.status = Suspect),
                &answer
            ),
            Err(ResolutionError::AffectedRefNotAccepted(_))
        ));
        let no_finding = rebuild(&g, |nodes, _| nodes.retain(|n| n.id != f));
        assert!(matches!(
            resolve(&no_finding, &answer),
            Err(ResolutionError::FindingMissing(_))
        ));
        let wrong = rebuild(&g, |nodes, _| {
            let n = nodes.iter_mut().find(|n| n.id == f).unwrap();
            n.payload = stakeholder("stakeholder:x").payload;
        });
        assert!(matches!(
            resolve(&wrong, &answer),
            Err(ResolutionError::NotAFinding(_))
        ));
    }

    // ------------------------------------------------------------------ actor and rationale

    #[test]
    fn actor_and_rationale_rules() {
        let g = asked();
        let base = good(&g, ResolutionTemplate::CalculationCalendar);
        assert!(resolve(&g, &base).is_ok());
        for actor in [
            "agent:ghost",
            "stakeholder:hr",
            "agent:intern",
            "agent:compiler",
        ] {
            let i = ResolutionInput {
                decided_by: id(actor),
                ..base.clone()
            };
            assert!(
                matches!(
                    resolve(&g, &i),
                    Err(ResolutionError::InvalidDecisionActor(_))
                ),
                "{actor}"
            );
        }
        for template in [
            ResolutionTemplate::DomainRelationshipCardinality,
            ResolutionTemplate::StateTransitionTrigger,
        ] {
            let ok = good(&g, template);
            for bad in [
                None,
                Some(""),
                Some(" padded"),
                Some("padded "),
                Some("tab\there"),
            ] {
                let i = ResolutionInput {
                    rationale: bad.map(Into::into),
                    ..ok.clone()
                };
                assert!(
                    matches!(resolve(&g, &i), Err(ResolutionError::InvalidRationale)),
                    "{template:?} {bad:?}"
                );
            }
        }
        // Optional for the four direct-effect initial answers, but clean when present.
        for template in [
            ResolutionTemplate::OperationPerformer,
            ResolutionTemplate::PermissionBinding,
            ResolutionTemplate::InvariantFormula,
        ] {
            assert!(resolve(&g, &good(&g, template)).is_ok());
            let i = ResolutionInput {
                rationale: Some(" x".into()),
                ..good(&g, template)
            };
            assert!(matches!(
                resolve(&g, &i),
                Err(ResolutionError::InvalidRationale)
            ));
        }
    }

    // ------------------------------------------------------------------ answer schema and references

    #[test]
    fn answers_are_validated_against_the_persisted_schema() {
        let g = asked();
        for template in TEMPLATES {
            let ok = good(&g, template);
            let object = ok.answer.as_object().unwrap().clone();
            let first = object.keys().next().unwrap().clone();
            let mut missing = object.clone();
            missing.remove(&first);
            let mut unknown = object.clone();
            unknown.insert("extra".into(), json!("x"));
            let mut wrong_type = object.clone();
            wrong_type.insert(first.clone(), json!(7));
            for answer in [
                JsonValue::Object(missing),
                JsonValue::Object(unknown),
                JsonValue::Object(wrong_type),
                json!("x"),
            ] {
                let i = ResolutionInput {
                    answer,
                    ..ok.clone()
                };
                assert!(
                    matches!(
                        resolve(&g, &i),
                        Err(ResolutionError::AnswerSchemaInvalid { .. })
                    ),
                    "{template:?}"
                );
            }
        }
        // Not in the persisted enum.
        let i = ResolutionInput {
            answer: json!({"performer_ref": "secrole:approver"}),
            ..good(&g, ResolutionTemplate::OperationPerformer)
        };
        assert!(matches!(
            resolve(&g, &i),
            Err(ResolutionError::AnswerSchemaInvalid { .. })
        ));
        let i = ResolutionInput {
            answer: json!({"cardinality_from": "many", "cardinality_to": "1"}),
            ..good(&g, ResolutionTemplate::DomainRelationshipCardinality)
        };
        assert!(matches!(
            resolve(&g, &i),
            Err(ResolutionError::AnswerSchemaInvalid { .. })
        ));
    }

    #[test]
    fn schema_valid_references_are_rechecked_against_the_graph() {
        let g = asked();
        // A listed choice that has since become non-Accepted.
        let later = edit_node(&g, &id("actor:clerk"), |n| n.status = Suspect);
        assert!(matches!(
            resolve(
                &later,
                &good(&later, ResolutionTemplate::OperationPerformer)
            ),
            Err(ResolutionError::AnswerReferenceNotAccepted(_))
        ));
        let later = edit_node(&g, &id("calendar:nl"), |n| n.status = Suspect);
        assert!(matches!(
            resolve(
                &later,
                &good(&later, ResolutionTemplate::CalculationCalendar)
            ),
            Err(ResolutionError::AnswerReferenceNotAccepted(_))
        ));
        // A persisted schema that (wrongly) lists another type is still checked by the mapper.
        let q = question_about(&g, "operation:approve");
        let tampered = edit_question(&g, &q, |p| {
            p.answer_schema = Some(
                json!({"type": "object", "additionalProperties": false, "required": ["performer_ref"],
                "properties": {"performer_ref": {"type": "string", "enum": ["secrole:approver", "actor:ghost"]}}}),
            );
        });
        let i = input(
            q.clone(),
            json!({"performer_ref": "secrole:approver"}),
            None,
        );
        assert!(matches!(
            resolve(&tampered, &i),
            Err(ResolutionError::AnswerReferenceWrongType { .. })
        ));
        let i = input(q, json!({"performer_ref": "actor:ghost"}), None);
        assert!(matches!(
            resolve(&tampered, &i),
            Err(ResolutionError::AnswerReferenceMissing(_))
        ));
    }

    // ------------------------------------------------------------------ cardinality and trigger

    #[test]
    fn cardinality_decision_marker_and_no_effect() {
        let g = asked();
        for (from, to) in [
            ("0..1", "1"),
            ("1", "0..*"),
            ("0..*", "1..*"),
            ("1..*", "0..1"),
        ] {
            let i = ResolutionInput {
                answer: json!({"cardinality_from": from, "cardinality_to": to}),
                ..good(&g, ResolutionTemplate::DomainRelationshipCardinality)
            };
            let r = resolve(&g, &i).unwrap();
            assert_eq!(
                r.decision.answer,
                json!({"kind": "domain_relationship_cardinality", "candidate_ref": CARDINALITY_CANDIDATE,
                       "cardinality_from": from, "cardinality_to": to})
            );
            assert_eq!(r.patch_artifact.effect_patch, None);
            let after = applied(&g, &r);
            assert!(after
                .node_ids_by_type(plumb_psg::NodeType::DomainRelationship)
                .is_empty());
        }
    }

    #[test]
    fn trigger_decision_marker_and_no_effect() {
        let g = asked();
        for trigger in ["operation:approve", "event:submitted"] {
            let i = ResolutionInput {
                answer: json!({"trigger_ref": trigger}),
                ..good(&g, ResolutionTemplate::StateTransitionTrigger)
            };
            let r = resolve(&g, &i).unwrap();
            assert_eq!(
                r.decision.answer,
                json!({"kind": "state_transition_trigger", "candidate_ref": TRIGGER_CANDIDATE, "trigger_ref": trigger})
            );
            assert_eq!(r.patch_artifact.effect_patch, None);
            let after = applied(&g, &r);
            assert!(after
                .node_ids_by_type(plumb_psg::NodeType::Transition)
                .is_empty());
            assert!(after
                .edge_ids_by_kind(&RelationKind::TransitionsVia)
                .is_empty());
        }
        // A trigger that is no longer Accepted is rejected by the mapper.
        let later = edit_node(&g, &id("event:submitted"), |n| n.status = Suspect);
        let i = ResolutionInput {
            answer: json!({"trigger_ref": "event:submitted"}),
            ..good(&later, ResolutionTemplate::StateTransitionTrigger)
        };
        assert!(matches!(
            resolve(&later, &i),
            Err(ResolutionError::AnswerReferenceNotAccepted(_))
        ));
    }

    // ------------------------------------------------------------------ performer

    #[test]
    fn performer_answers_and_staleness() {
        let g = asked();
        for performer in ["actor:clerk", "brole:manager"] {
            let i = ResolutionInput {
                answer: json!({"performer_ref": performer}),
                ..good(&g, ResolutionTemplate::OperationPerformer)
            };
            let r = resolve(&g, &i).unwrap();
            let SemanticPatch::AddEdge { edge } = r.patch_artifact.effect_patch.clone().unwrap()
            else {
                panic!()
            };
            assert_eq!(
                (
                    edge.kind.clone(),
                    edge.from.clone(),
                    edge.to.clone(),
                    edge.status
                ),
                (
                    RelationKind::PerformedBy,
                    id("operation:approve"),
                    id(performer),
                    Accepted
                )
            );
            assert_eq!(
                edge.id,
                relation_id(
                    &RelationKind::PerformedBy,
                    &id("operation:approve"),
                    &id(performer)
                )
                .unwrap()
            );
        }
        let r = resolve(&g, &good(&g, ResolutionTemplate::OperationPerformer)).unwrap();
        let SemanticPatch::AddEdge { edge } = r.patch_artifact.effect_patch.unwrap() else {
            panic!()
        };
        assert_eq!(edge.id.to_string(), GOLDEN_PERFORMER_EDGE);
        // An Accepted performer already present makes the initial Question stale.
        let performed = rebuild(&g, |_, edges| {
            edges.push(edge_as(
                "rel:existing",
                RelationKind::PerformedBy,
                "operation:approve",
                "brole:manager",
                Accepted,
            ))
        });
        assert!(matches!(
            resolve(
                &performed,
                &good(&performed, ResolutionTemplate::OperationPerformer)
            ),
            Err(ResolutionError::StaleResolutionQuestion { .. })
        ));
    }

    #[test]
    fn multi_question_finding_closes_after_the_last_answer() {
        let g = asked();
        let f = performer_finding().id;
        let qa = question_about(&g, "operation:approve");
        let qs = question_about(&g, "operation:submit");
        let first = resolve(
            &g,
            &input(qa.clone(), json!({"performer_ref": "actor:clerk"}), None),
        )
        .unwrap();
        assert!(leaves(&first)
            .iter()
            .all(|p| !matches!(p, SemanticPatch::ReplacePayload { target, .. } if target.id == f)));
        let g1 = applied(&g, &first);
        assert_eq!(question_payload(&g1, &qa).status, "Answered");
        assert_eq!(question_payload(&g1, &qs).status, "Open");
        assert_eq!(finding_status(&g1, &f), "Open");
        let second = resolve(
            &g1,
            &input(qs.clone(), json!({"performer_ref": "brole:manager"}), None),
        )
        .unwrap();
        let g2 = applied(&g1, &second);
        assert_eq!(question_payload(&g2, &qs).status, "Answered");
        assert_eq!(finding_status(&g2, &f), "Closed");
        // The same order reversed gives the same terminal state.
        let other = resolve(
            &g,
            &input(qs.clone(), json!({"performer_ref": "brole:manager"}), None),
        )
        .unwrap();
        let h1 = applied(&g, &other);
        assert_eq!(finding_status(&h1, &f), "Open");
        let last = resolve(
            &h1,
            &input(qa, json!({"performer_ref": "actor:clerk"}), None),
        )
        .unwrap();
        assert_eq!(finding_status(&applied(&h1, &last), &f), "Closed");
    }

    // ------------------------------------------------------------------ calendar

    #[test]
    fn calendar_changes_only_calendar_ref() {
        let g = asked();
        let r = resolve(&g, &good(&g, ResolutionTemplate::CalculationCalendar)).unwrap();
        let SemanticPatch::ReplacePayload {
            target,
            payload: NodePayload::Calculation(after),
        } = r.patch_artifact.effect_patch.clone().unwrap()
        else {
            panic!()
        };
        let node = g.node(&id("calculation:days")).unwrap();
        assert_eq!(target.expected_hash, node_element_hash(node).unwrap());
        let NodePayload::Calculation(before) = &node.payload else {
            unreachable!()
        };
        let mut restored = after.clone();
        assert_eq!(restored.calendar_ref, Some(id("calendar:nl")));
        restored.calendar_ref = None;
        assert_eq!(&restored, before);
        // Wrong type and non-Accepted calendars are rejected; an existing calendar is stale.
        let tampered = edit_question(&g, &question_about(&g, "calculation:days"), |p| {
            p.answer_schema = Some(
                json!({"type": "object", "additionalProperties": false, "required": ["calendar_ref"],
                "properties": {"calendar_ref": {"type": "string"}}}),
            );
        });
        let i = ResolutionInput {
            answer: json!({"calendar_ref": "operation:approve"}),
            ..good(&tampered, ResolutionTemplate::CalculationCalendar)
        };
        assert!(matches!(
            resolve(&tampered, &i),
            Err(ResolutionError::AnswerReferenceWrongType { .. })
        ));
        let i = ResolutionInput {
            answer: json!({"calendar_ref": "calendar:draft"}),
            ..good(&tampered, ResolutionTemplate::CalculationCalendar)
        };
        assert!(matches!(
            resolve(&tampered, &i),
            Err(ResolutionError::AnswerReferenceNotAccepted(_))
        ));
        let has_calendar = edit_node(&g, &id("calculation:days"), |n| {
            if let NodePayload::Calculation(c) = &mut n.payload {
                c.calendar_ref = Some(id("calendar:de"));
            }
        });
        assert!(matches!(
            resolve(
                &has_calendar,
                &good(&has_calendar, ResolutionTemplate::CalculationCalendar)
            ),
            Err(ResolutionError::StaleResolutionQuestion { .. })
        ));
    }

    // ------------------------------------------------------------------ permission

    fn permission_answer(graph: &Graph, operation: &str, scope: &str) -> ResolutionInput {
        ResolutionInput {
            answer: json!({"operation_ref": operation, "resource_scope_ref": scope}),
            ..good(graph, ResolutionTemplate::PermissionBinding)
        }
    }

    fn effect_leaves(result: &ResolutionResult) -> Vec<SemanticPatch> {
        result
            .patch_artifact
            .effect_patch
            .as_ref()
            .map(flat)
            .unwrap_or_default()
    }

    /// The permission graph with permits/scoped_to set to (status, target).
    fn permission_graph(permits: (ElementStatus, &str), scoped: (ElementStatus, &str)) -> Graph {
        let fx = Fx::new()
            .edit_edge("rel:permits", |e| {
                e.status = permits.0;
                e.to = id(permits.1);
            })
            .edit_edge("rel:scoped", |e| {
                e.status = scoped.0;
                e.to = id(scoped.1);
            });
        ask(&fx.graph(), vec![permission_finding()])
    }

    fn assert_final_permission(graph: &Graph, operation: &str, scope: &str) {
        for (kind, target) in [
            (RelationKind::Permits, operation),
            (RelationKind::ScopedTo, scope),
        ] {
            let baseline: Vec<&Edge> = graph
                .edges()
                .values()
                .filter(|e| {
                    e.kind == kind
                        && e.from == id("permission:approve")
                        && plumb_psg::is_baseline(e.status)
                })
                .collect();
            assert_eq!(baseline.len(), 1, "{kind:?}");
            assert_eq!(
                (baseline[0].status, baseline[0].to.clone()),
                (Accepted, id(target))
            );
        }
        assert!(
            graph.edge(&id("rel:permits")).is_some() && graph.edge(&id("rel:scoped")).is_some()
        );
    }

    #[test]
    fn permission_structure_and_staleness() {
        // Both concrete: stale.
        let g = permission_graph((Accepted, "operation:approve"), (Accepted, "scope:orders"));
        assert!(matches!(
            resolve(
                &g,
                &permission_answer(&g, "operation:approve", "scope:orders")
            ),
            Err(ResolutionError::StaleResolutionQuestion { .. })
        ));
        // Superseded/Deprecated authoritative edges are not repaired.
        for status in [Superseded, Deprecated] {
            let g = permission_graph((status, "operation:approve"), (Accepted, "scope:orders"));
            assert!(
                matches!(
                    resolve(
                        &g,
                        &permission_answer(&g, "operation:approve", "scope:orders")
                    ),
                    Err(ResolutionError::UnsupportedPermissionEdgeLifecycle { .. })
                ),
                "{status:?}"
            );
        }
        // Answer types are checked.
        let g = permission_graph((Accepted, "operation:old"), (Accepted, "scope:orders"));
        let tampered = edit_question(&g, &question_about(&g, "permission:approve"), |p| {
            p.answer_schema = Some(
                json!({"type": "object", "additionalProperties": false, "required": ["operation_ref", "resource_scope_ref"],
                "properties": {"operation_ref": {"type": "string"}, "resource_scope_ref": {"type": "string"}}}),
            );
        });
        assert!(matches!(
            resolve(
                &tampered,
                &permission_answer(&tampered, "scope:hr", "scope:orders")
            ),
            Err(ResolutionError::AnswerReferenceWrongType { .. })
        ));
        assert!(matches!(
            resolve(
                &tampered,
                &permission_answer(&tampered, "operation:approve", "operation:submit")
            ),
            Err(ResolutionError::AnswerReferenceWrongType { .. })
        ));
        assert!(matches!(
            resolve(
                &tampered,
                &permission_answer(&tampered, "operation:draft", "scope:orders")
            ),
            Err(ResolutionError::AnswerReferenceNotAccepted(_))
        ));
    }

    #[test]
    fn permission_accepted_edge_is_retargeted_in_place() {
        // Accepted permits -> Suspect Operation, valid scoped_to.
        let g = permission_graph((Accepted, "operation:old"), (Accepted, "scope:orders"));
        let r = resolve(
            &g,
            &permission_answer(&g, "operation:approve", "scope:orders"),
        )
        .unwrap();
        let effect = effect_leaves(&r);
        assert_eq!(effect.len(), 1);
        let SemanticPatch::ReplaceEdge {
            target,
            kind,
            from,
            to,
            properties,
        } = &effect[0]
        else {
            panic!("{effect:?}")
        };
        assert_eq!(target.id, id("rel:permits"));
        assert_eq!(
            target.expected_hash,
            edge_element_hash(g.edge(&id("rel:permits")).unwrap()).unwrap()
        );
        assert_eq!(
            (kind, from, to, properties),
            (
                &RelationKind::Permits,
                &id("permission:approve"),
                &id("operation:approve"),
                &RelationProperties::None
            )
        );
        let after = applied(&g, &r);
        assert_final_permission(&after, "operation:approve", "scope:orders");
        // The human may also retarget the currently valid sibling.
        let r = resolve(&g, &permission_answer(&g, "operation:approve", "scope:hr")).unwrap();
        assert_eq!(effect_leaves(&r).len(), 2);
        assert_final_permission(&applied(&g, &r), "operation:approve", "scope:hr");
    }

    #[test]
    fn permission_suspect_edges_are_promoted_with_intermediate_cas() {
        // Suspect permits on the answered target: SetStatus only.
        let g = permission_graph((Suspect, "operation:approve"), (Accepted, "scope:orders"));
        let r = resolve(
            &g,
            &permission_answer(&g, "operation:approve", "scope:orders"),
        )
        .unwrap();
        let effect = effect_leaves(&r);
        assert!(matches!(
            &effect[..],
            [SemanticPatch::SetStatus {
                from: Suspect,
                to: Accepted,
                ..
            }]
        ));
        assert_final_permission(&applied(&g, &r), "operation:approve", "scope:orders");
        // Suspect permits -> Suspect Operation, answered with another: ReplaceEdge then SetStatus.
        let g = permission_graph((Suspect, "operation:old"), (Accepted, "scope:orders"));
        let r = resolve(
            &g,
            &permission_answer(&g, "operation:approve", "scope:orders"),
        )
        .unwrap();
        let effect = effect_leaves(&r);
        let [SemanticPatch::ReplaceEdge { target: first, .. }, SemanticPatch::SetStatus {
            target: second,
            from: Suspect,
            to: Accepted,
        }] = &effect[..]
        else {
            panic!("{effect:?}")
        };
        let original = g.edge(&id("rel:permits")).unwrap();
        assert_eq!(first.expected_hash, edge_element_hash(original).unwrap());
        let mut intermediate = original.clone();
        intermediate.to = id("operation:approve");
        assert_eq!(
            second.expected_hash,
            edge_element_hash(&intermediate).unwrap()
        );
        assert_ne!(first.expected_hash, second.expected_hash);
        assert_final_permission(&applied(&g, &r), "operation:approve", "scope:orders");
        // The same for scoped_to.
        let g = permission_graph((Accepted, "operation:approve"), (Suspect, "scope:old"));
        let r = resolve(&g, &permission_answer(&g, "operation:approve", "scope:hr")).unwrap();
        assert!(matches!(
            &effect_leaves(&r)[..],
            [
                SemanticPatch::ReplaceEdge { .. },
                SemanticPatch::SetStatus { .. }
            ]
        ));
        assert_final_permission(&applied(&g, &r), "operation:approve", "scope:hr");
        // A concurrent change to the edge makes the whole proposal fail CAS.
        let g = permission_graph((Suspect, "operation:old"), (Accepted, "scope:orders"));
        let r = resolve(
            &g,
            &permission_answer(&g, "operation:approve", "scope:orders"),
        )
        .unwrap();
        let changed = rebuild(&g, |_, edges| {
            edges
                .iter_mut()
                .find(|e| e.id == id("rel:permits"))
                .unwrap()
                .to = id("operation:submit");
        });
        let rebased = PatchSet {
            base_semantic_hash: changed.semantic_hash().unwrap(),
            patch: r.proposal.patch_set.patch.clone(),
        };
        assert!(matches!(
            apply_patch(&changed, &rebased),
            Err(PatchError::ElementHashMismatch { .. })
        ));
    }

    #[test]
    fn permission_never_adds_or_removes_relations_and_ignores_alternatives() {
        let g = permission_graph((Suspect, "operation:old"), (Suspect, "scope:old"));
        let g = rebuild(&g, |_, edges| {
            edges.push(edge_as(
                "rel:alt-permits",
                RelationKind::Permits,
                "permission:approve",
                "operation:submit",
                Proposed,
            ));
            edges.push(edge_as(
                "rel:alt-scope",
                RelationKind::ScopedTo,
                "permission:approve",
                "scope:hr",
                Rejected,
            ));
        });
        let r = resolve(
            &g,
            &permission_answer(&g, "operation:approve", "scope:orders"),
        )
        .unwrap();
        for leaf in effect_leaves(&r) {
            assert!(
                !matches!(
                    leaf,
                    SemanticPatch::AddEdge { .. } | SemanticPatch::RemoveEdge { .. }
                ),
                "{leaf:?}"
            );
        }
        assert_eq!(effect_leaves(&r).len(), 4);
        let after = applied(&g, &r);
        assert_final_permission(&after, "operation:approve", "scope:orders");
        assert_eq!(
            after.edge(&id("rel:alt-permits")),
            g.edge(&id("rel:alt-permits"))
        );
        assert_eq!(
            after.edge(&id("rel:alt-scope")),
            g.edge(&id("rel:alt-scope"))
        );
    }

    // ------------------------------------------------------------------ invariant

    #[test]
    fn invariant_expression_is_checked_before_any_patch() {
        let g = asked();
        let r = resolve(&g, &good(&g, ResolutionTemplate::InvariantFormula)).unwrap();
        let SemanticPatch::ReplacePayload {
            payload: NodePayload::Invariant(after),
            ..
        } = r.patch_artifact.effect_patch.clone().unwrap()
        else {
            panic!()
        };
        assert_eq!(
            after,
            Invariant {
                scope_ref: id("entity:order"),
                expression: "amount >= 0".into()
            }
        );
        let with = |expression: &str| ResolutionInput {
            answer: json!({"expression": expression}),
            ..good(&g, ResolutionTemplate::InvariantFormula)
        };
        assert!(matches!(
            resolve(&g, &with("amount >=")),
            Err(ResolutionError::InvalidInvariantExpression { .. })
        ));
        assert!(matches!(
            resolve(&g, &with("amount and true")),
            Err(ResolutionError::InvalidInvariantExpression { .. })
        ));
        assert!(matches!(
            resolve(&g, &with("amount + 1")),
            Err(ResolutionError::InvariantNotBoolean { .. })
        ));
        assert!(matches!(
            resolve(&g, &with("missing >= 0")),
            Err(ResolutionError::InvalidInvariantExpression { .. })
        ));
        assert!(matches!(
            resolve_question(
                &g,
                &good(&g, ResolutionTemplate::InvariantFormula),
                &ResolutionContext::default()
            ),
            Err(ResolutionError::InvariantScopeUnavailable(_))
        ));
        // A self-contained expression is accepted under an explicitly empty scope.
        let empty = ResolutionContext {
            invariant_bindings: BTreeMap::from([(id("inv:positive"), vec![])]),
        };
        assert!(resolve_question(&g, &with("1 == 1"), &empty).is_ok());
    }

    // ------------------------------------------------------------------ decision node, ID and artifact

    #[test]
    fn decision_node_and_governance_edges() {
        let g = asked();
        let i = good(&g, ResolutionTemplate::CalculationCalendar);
        let r = resolve(&g, &i).unwrap();
        let after = applied(&g, &r);
        let node = after.node(&r.decision_ref).unwrap();
        assert_eq!((node.status, node.revision), (Accepted, 1));
        assert!(node.evidence.is_empty() && node.derivations.is_empty());
        assert_eq!(node.audit.created_by, id(HUMAN));
        assert_eq!(node.audit.created_at, ts(DECIDED_AT));
        assert_eq!(node.audit.updated_by, None);
        let NodePayload::ResolutionDecision(d) = &node.payload else {
            panic!()
        };
        assert_eq!(d, &r.decision);
        assert_eq!(d.question_ref, Some(i.question_ref.clone()));
        assert_eq!(d.proposal_ref, None);
        assert_eq!(d.decided_by, id(HUMAN));
        assert_eq!(d.decided_at, ts(DECIDED_AT));
        assert_eq!(d.rationale, None);
        assert_eq!(d.supersedes, None);
        assert_eq!(
            d.answer,
            json!({"kind": "calculation_calendar", "calculation_ref": "calculation:days", "calendar_ref": "calendar:nl"})
        );
        let resolves: Vec<&Edge> = after
            .outgoing_edge_ids(&r.decision_ref)
            .iter()
            .filter_map(|e| after.edge(e))
            .collect();
        assert_eq!(resolves.len(), 2);
        let targets: BTreeSet<Id> = resolves.iter().map(|e| e.to.clone()).collect();
        assert_eq!(
            targets,
            BTreeSet::from([i.question_ref.clone(), calendar_finding().id])
        );
        for e in resolves {
            assert_eq!(
                (e.kind.clone(), e.status, e.properties.clone()),
                (RelationKind::Resolves, Accepted, RelationProperties::None)
            );
            assert_eq!(
                e.id,
                relation_id(&RelationKind::Resolves, &r.decision_ref, &e.to).unwrap()
            );
            assert_eq!(e.audit.created_by, id(HUMAN));
        }
    }

    #[test]
    fn decision_id_goldens_and_sensitivity() {
        let project = id("project:pilot");
        let patch_ref = Hash::content_sha256(b"artifact");
        let marker = json!({"kind": "domain_relationship_cardinality", "candidate_ref": CARDINALITY_CANDIDATE,
                            "cardinality_from": "1", "cardinality_to": "0..*"});
        let q = id("q:00000000000000c1");
        let base = decision_id(
            &project,
            &q,
            &marker,
            &id(HUMAN),
            &ts(DECIDED_AT),
            &patch_ref,
            None,
        )
        .unwrap();
        assert_eq!(base.to_string(), GOLDEN_FIXED_CARDINALITY_DECISION);
        let calendar = json!({"kind": "calculation_calendar", "calculation_ref": "calculation:days", "calendar_ref": "calendar:nl"});
        assert_eq!(
            decision_id(
                &project,
                &q,
                &calendar,
                &id(HUMAN),
                &ts(DECIDED_AT),
                &patch_ref,
                None
            )
            .unwrap()
            .to_string(),
            GOLDEN_FIXED_CALENDAR_DECISION
        );
        let variants = [
            decision_id(
                &project,
                &q,
                &calendar,
                &id(HUMAN),
                &ts(DECIDED_AT),
                &patch_ref,
                None,
            ),
            decision_id(
                &project,
                &id("q:00000000000000c2"),
                &marker,
                &id(HUMAN),
                &ts(DECIDED_AT),
                &patch_ref,
                None,
            ),
            decision_id(
                &project,
                &q,
                &marker,
                &id("agent:other"),
                &ts(DECIDED_AT),
                &patch_ref,
                None,
            ),
            decision_id(&project, &q, &marker, &id(HUMAN), &ts(AT), &patch_ref, None),
            decision_id(
                &project,
                &q,
                &marker,
                &id(HUMAN),
                &ts(DECIDED_AT),
                &Hash::content_sha256(b"other"),
                None,
            ),
            decision_id(
                &project,
                &q,
                &marker,
                &id(HUMAN),
                &ts(DECIDED_AT),
                &patch_ref,
                Some(&id("dec:0000000000000001")),
            ),
            decision_id(
                &id("project:other"),
                &q,
                &marker,
                &id(HUMAN),
                &ts(DECIDED_AT),
                &patch_ref,
                None,
            ),
        ];
        for v in variants {
            assert_ne!(v.unwrap(), base);
        }
        // End to end: the real decisions match independently computed IDs; rationale is not identity.
        let g = asked();
        let card = resolve(
            &g,
            &good(&g, ResolutionTemplate::DomainRelationshipCardinality),
        )
        .unwrap();
        assert_eq!(card.decision_ref.to_string(), GOLDEN_CARDINALITY_DECISION);
        let other_rationale = ResolutionInput {
            rationale: Some("Another reason.".into()),
            ..good(&g, ResolutionTemplate::DomainRelationshipCardinality)
        };
        assert_eq!(
            resolve(&g, &other_rationale).unwrap().decision_ref,
            card.decision_ref
        );
        let cal = resolve(&g, &good(&g, ResolutionTemplate::CalculationCalendar)).unwrap();
        assert_eq!(cal.decision_ref.to_string(), GOLDEN_CALENDAR_DECISION);
    }

    #[test]
    fn patch_artifact_is_canonical_and_not_circular() {
        let g = asked();
        let i = good(&g, ResolutionTemplate::CalculationCalendar);
        let r = resolve(&g, &i).unwrap();
        let a = &r.patch_artifact;
        assert_eq!(a.version, 1);
        assert_eq!(a.question_ref, i.question_ref);
        assert_eq!(a.base_semantic_hash, g.semantic_hash().unwrap());
        assert_eq!(r.patch_artifact_bytes, to_canonical_json(a).unwrap());
        assert_eq!(
            Hash::content_sha256(&r.patch_artifact_bytes),
            r.decision.patch_ref
        );
        assert_eq!(a.content_hash().unwrap(), r.decision.patch_ref);
        // The artifact is computable before (and independently of) the decision node.
        let independent = ResolutionPatchArtifact {
            version: 1,
            question_ref: i.question_ref.clone(),
            base_semantic_hash: g.semantic_hash().unwrap(),
            effect_patch: a.effect_patch.clone(),
        };
        assert_eq!(independent.content_hash().unwrap(), r.decision.patch_ref);
        let text = String::from_utf8(r.patch_artifact_bytes.clone()).unwrap();
        assert!(!text.contains(r.decision_ref.as_str()));
        assert!(!text.contains("resolves") && !text.contains("Answered"));
        // Byte-identical and insertion-order independent.
        let again = resolve(&g, &i).unwrap();
        assert_eq!(again.patch_artifact_bytes, r.patch_artifact_bytes);
        let reversed = rebuild(&g, |nodes, edges| {
            nodes.reverse();
            edges.reverse();
        });
        assert_eq!(
            resolve(&reversed, &i).unwrap().patch_artifact_bytes,
            r.patch_artifact_bytes
        );
    }

    // ------------------------------------------------------------------ lifecycle and proposal

    #[test]
    fn single_question_lifecycle_and_round_ref() {
        let g = asked();
        let q = question_about(&g, "calculation:days");
        let g = edit_question(&g, &q, |p| p.round_ref = Some(id("rnd:00000000000000bb")));
        let before = question_payload(&g, &q);
        let r = resolve(&g, &good(&g, ResolutionTemplate::CalculationCalendar)).unwrap();
        let after = applied(&g, &r);
        let mut expected = before.clone();
        expected.status = "Answered".into();
        assert_eq!(question_payload(&after, &q), expected);
        assert_eq!(after.node(&q).unwrap().status, Accepted);
        assert_eq!(finding_status(&after, &calendar_finding().id), "Closed");
        // A replay of the initial answer is AlreadyResolved and creates nothing.
        assert!(matches!(
            resolve(
                &after,
                &good(&after, ResolutionTemplate::CalculationCalendar)
            ),
            Err(ResolutionError::AlreadyResolved { .. })
        ));
    }

    #[test]
    fn proposal_metadata_order_and_cas() {
        let g = asked();
        let i = good(&g, ResolutionTemplate::CalculationCalendar);
        let r = resolve(&g, &i).unwrap();
        let p = &r.proposal;
        assert_eq!(p.stage, StageId::S3);
        assert_eq!(p.materiality, ProposalMateriality::MaterialDecision);
        assert_eq!(p.acceptance_policy, AcceptancePolicy::HumanDecision);
        assert_eq!(
            serde_json::to_value(p.acceptance_policy).unwrap(),
            json!("HUMAN_DECISION")
        );
        assert_eq!(p.confidence, None);
        assert_eq!(p.patch_set.base_semantic_hash, g.semantic_hash().unwrap());
        let l = leaves(&r);
        assert_eq!(l.len(), 6);
        assert!(matches!(
            &l[0],
            SemanticPatch::ReplacePayload {
                payload: NodePayload::Calculation(_),
                ..
            }
        ));
        assert!(matches!(&l[1], SemanticPatch::AddNode { node } if node.id == r.decision_ref));
        assert!(matches!(&l[2], SemanticPatch::AddEdge { edge } if edge.to == i.question_ref));
        assert!(
            matches!(&l[3], SemanticPatch::AddEdge { edge } if edge.to == calendar_finding().id)
        );
        assert!(
            matches!(&l[4], SemanticPatch::ReplacePayload { target, payload: NodePayload::Question(_) } if target.id == i.question_ref)
        );
        assert!(
            matches!(&l[5], SemanticPatch::ReplacePayload { target, payload: NodePayload::Finding(_) } if target.id == calendar_finding().id)
        );
        for leaf in &l {
            if let SemanticPatch::ReplacePayload { target, .. } = leaf {
                assert_eq!(
                    target.expected_hash,
                    node_element_hash(g.node(&target.id).unwrap()).unwrap()
                );
            }
        }
        let after = applied(&g, &r);
        assert_ne!(after.semantic_hash().unwrap(), g.semantic_hash().unwrap());
        // Cardinality has no direct effect but its decision still changes semantic_hash.
        let card = resolve(
            &g,
            &good(&g, ResolutionTemplate::DomainRelationshipCardinality),
        )
        .unwrap();
        assert!(matches!(&leaves(&card)[0], SemanticPatch::AddNode { .. }));
        assert_ne!(
            applied(&g, &card).semantic_hash().unwrap(),
            g.semantic_hash().unwrap()
        );
        // A stale graph is rejected by ordinary CAS.
        let changed = edit_node(&g, &id("calculation:days"), |n| {
            if let NodePayload::Calculation(c) = &mut n.payload {
                c.expression = "2".into();
            }
        });
        let rebased = PatchSet {
            base_semantic_hash: changed.semantic_hash().unwrap(),
            patch: p.patch_set.patch.clone(),
        };
        assert!(matches!(
            apply_patch(&changed, &rebased),
            Err(PatchError::ElementHashMismatch { .. })
        ));
        assert!(apply_patch(&changed, &p.patch_set).is_err());
    }

    // ------------------------------------------------------------------ supersession

    fn supersede(_graph: &Graph, old: &ResolutionResult, answer: JsonValue) -> ResolutionInput {
        ResolutionInput {
            question_ref: old.decision.question_ref.clone().unwrap(),
            answer,
            decided_by: id(HUMAN),
            decided_at: ts("2026-04-01T10:00:00.000000000Z"),
            rationale: Some("Policy changed.".into()),
            supersedes: Some(old.decision_ref.clone()),
        }
    }

    /// Answers a template, applies it, supersedes it with `answer` and checks the common rules.
    fn superseded(
        template: ResolutionTemplate,
        initial: ResolutionInput,
        answer: JsonValue,
        start: Graph,
    ) -> (Graph, ResolutionResult, ResolutionResult, Graph) {
        let first = resolve(&start, &initial).unwrap_or_else(|e| panic!("{template:?}: {e}"));
        let g1 = applied(&start, &first);
        let q = initial.question_ref.clone();
        let round = question_payload(&g1, &q).round_ref;
        let i = supersede(&g1, &first, answer);
        let second = resolve(&g1, &i).unwrap_or_else(|e| panic!("{template:?}: {e}"));
        assert_eq!(second.decision.supersedes, Some(first.decision_ref.clone()));
        let g2 = applied(&g1, &second);
        assert_eq!(g2.node(&first.decision_ref).unwrap().status, Superseded);
        assert_eq!(g2.node(&second.decision_ref).unwrap().status, Accepted);
        assert_eq!(
            g2.node(&first.decision_ref).unwrap().payload,
            g1.node(&first.decision_ref).unwrap().payload
        );
        let supersedes: Vec<&Edge> = g2
            .outgoing_edge_ids(&second.decision_ref)
            .iter()
            .filter_map(|e| g2.edge(e))
            .filter(|e| e.kind == RelationKind::Supersedes)
            .collect();
        assert_eq!(supersedes.len(), 1);
        assert_eq!(
            (supersedes[0].to.clone(), supersedes[0].status),
            (first.decision_ref.clone(), Accepted)
        );
        let old_resolves = g2
            .outgoing_edge_ids(&first.decision_ref)
            .iter()
            .filter_map(|e| g2.edge(e))
            .filter(|e| e.kind == RelationKind::Resolves)
            .count();
        assert_eq!(old_resolves, 2);
        assert_eq!(question_payload(&g2, &q).status, "Answered");
        assert_eq!(question_payload(&g2, &q).round_ref, round);
        assert!(!leaves(&second).iter().any(|p| matches!(
            p,
            SemanticPatch::ReplacePayload {
                payload: NodePayload::Question(_),
                ..
            }
        )));
        // Replaying the applied supersession creates nothing.
        assert!(matches!(
            resolve(&g2, &i),
            Err(ResolutionError::AlreadyResolved { .. })
        ));
        (g1, first, second, g2)
    }

    #[test]
    fn supersession_of_each_family() {
        let g = asked();
        use ResolutionTemplate::*;
        let (_, _, second, g2) = superseded(
            DomainRelationshipCardinality,
            good(&g, DomainRelationshipCardinality),
            json!({"cardinality_from": "0..1", "cardinality_to": "1"}),
            g.clone(),
        );
        assert_eq!(second.patch_artifact.effect_patch, None);
        let active: Vec<&Node> = g2.nodes().values().filter(|n| n.status == Accepted && matches!(&n.payload, NodePayload::ResolutionDecision(d) if d.answer["kind"] == "domain_relationship_cardinality")).collect();
        assert_eq!(active.len(), 1);
        let (_, _, second, _) = superseded(
            StateTransitionTrigger,
            good(&g, StateTransitionTrigger),
            json!({"trigger_ref": "event:submitted"}),
            g.clone(),
        );
        assert_eq!(second.patch_artifact.effect_patch, None);
        let (_, _, second, g2) = superseded(
            CalculationCalendar,
            good(&g, CalculationCalendar),
            json!({"calendar_ref": "calendar:de"}),
            g.clone(),
        );
        assert!(matches!(
            second.patch_artifact.effect_patch,
            Some(SemanticPatch::ReplacePayload { .. })
        ));
        let NodePayload::Calculation(c) = &g2.node(&id("calculation:days")).unwrap().payload else {
            panic!()
        };
        assert_eq!(c.calendar_ref, Some(id("calendar:de")));
        let (_, _, _, g2) = superseded(
            InvariantFormula,
            good(&g, InvariantFormula),
            json!({"expression": "amount > 0"}),
            g.clone(),
        );
        let NodePayload::Invariant(inv) = &g2.node(&id("inv:positive")).unwrap().payload else {
            panic!()
        };
        assert_eq!(inv.expression, "amount > 0");
        let (_, _, second, g2) = superseded(
            OperationPerformer,
            good(&g, OperationPerformer),
            json!({"performer_ref": "brole:manager"}),
            g.clone(),
        );
        assert!(matches!(
            second.patch_artifact.effect_patch,
            Some(SemanticPatch::ReplaceEdge { .. })
        ));
        let performers: Vec<Id> = g2
            .edges()
            .values()
            .filter(|e| e.kind == RelationKind::PerformedBy && e.from == id("operation:approve"))
            .map(|e| e.to.clone())
            .collect();
        assert_eq!(performers, ids(&["brole:manager"]));
        let pg = permission_graph((Accepted, "operation:old"), (Accepted, "scope:orders"));
        let (_, _, second, g2) = superseded(
            PermissionBinding,
            permission_answer(&pg, "operation:approve", "scope:orders"),
            json!({"operation_ref": "operation:submit", "resource_scope_ref": "scope:orders"}),
            pg.clone(),
        );
        let effect = effect_leaves(&second);
        assert!(
            matches!(&effect[..], [SemanticPatch::ReplaceEdge { target, .. }] if target.id == id("rel:permits"))
        );
        assert_final_permission(&g2, "operation:submit", "scope:orders");
    }

    #[test]
    fn supersession_errors() {
        let g = asked();
        let first = resolve(&g, &good(&g, ResolutionTemplate::CalculationCalendar)).unwrap();
        let g1 = applied(&g, &first);
        let base = supersede(&g1, &first, json!({"calendar_ref": "calendar:de"}));
        // Rationale is mandatory for supersession.
        assert!(matches!(
            resolve(
                &g1,
                &ResolutionInput {
                    rationale: None,
                    ..base.clone()
                }
            ),
            Err(ResolutionError::InvalidRationale)
        ));
        // An Open Question cannot be superseded.
        assert!(matches!(
            resolve(&g, &ResolutionInput { ..base.clone() }),
            Err(ResolutionError::QuestionNotAnsweredForSupersession { .. })
        ));
        // An old decision of another Question.
        let other = resolve(&g1, &good(&g1, ResolutionTemplate::InvariantFormula)).unwrap();
        let g2 = applied(&g1, &other);
        let foreign = ResolutionInput {
            supersedes: Some(other.decision_ref.clone()),
            ..base.clone()
        };
        assert!(matches!(
            resolve(&g2, &foreign),
            Err(ResolutionError::InvalidSupersededDecision { .. })
        ));
        // Not a decision at all.
        let nonsense = ResolutionInput {
            supersedes: Some(id("operation:approve")),
            ..base.clone()
        };
        assert!(matches!(
            resolve(&g1, &nonsense),
            Err(ResolutionError::InvalidSupersededDecision { .. })
        ));
        // The graph no longer matches the old answer.
        let drifted = edit_node(&g1, &id("calculation:days"), |n| {
            if let NodePayload::Calculation(c) = &mut n.payload {
                c.calendar_ref = Some(id("calendar:de"));
            }
        });
        assert!(matches!(
            resolve(&drifted, &base),
            Err(ResolutionError::SupersessionStateMismatch { .. })
        ));
        // An already Superseded old decision without an Accepted successor.
        let retired = edit_node(&g1, &first.decision_ref, |n| n.status = Superseded);
        assert!(matches!(
            resolve(&retired, &base),
            Err(ResolutionError::InvalidSupersededDecision { .. })
        ));
        // Permission supersession requires the current relations to equal the old marker.
        let pg = permission_graph((Accepted, "operation:old"), (Accepted, "scope:orders"));
        let pfirst = resolve(
            &pg,
            &permission_answer(&pg, "operation:approve", "scope:orders"),
        )
        .unwrap();
        let pg1 = applied(&pg, &pfirst);
        let moved = rebuild(&pg1, |_, edges| {
            edges
                .iter_mut()
                .find(|e| e.id == id("rel:scoped"))
                .unwrap()
                .to = id("scope:hr");
        });
        let i = supersede(
            &moved,
            &pfirst,
            json!({"operation_ref": "operation:submit", "resource_scope_ref": "scope:orders"}),
        );
        assert!(matches!(
            resolve(&moved, &i),
            Err(ResolutionError::SupersessionStateMismatch { .. })
        ));
    }

    // ------------------------------------------------------------------ determinism and guards

    #[test]
    fn resolution_is_deterministic_under_reordering() {
        let g = asked();
        let reversed = rebuild(&g, |nodes, edges| {
            nodes.reverse();
            edges.reverse();
        });
        for template in TEMPLATES {
            let a = resolve(&g, &good(&g, template)).unwrap();
            let b = resolve(&reversed, &good(&reversed, template)).unwrap();
            assert_eq!(a.decision_ref, b.decision_ref);
            assert_eq!(a.decision, b.decision);
            assert_eq!(a.patch_artifact_bytes, b.patch_artifact_bytes);
            assert_eq!(
                to_canonical_json(&a.proposal).unwrap(),
                to_canonical_json(&b.proposal).unwrap()
            );
        }
        // Sibling Question insertion order does not change the performer lifecycle.
        let qa = question_about(&g, "operation:approve");
        let a = resolve(
            &g,
            &input(qa.clone(), json!({"performer_ref": "actor:clerk"}), None),
        )
        .unwrap();
        let b = resolve(
            &reversed,
            &input(qa, json!({"performer_ref": "actor:clerk"}), None),
        )
        .unwrap();
        assert_eq!(leaves(&a).len(), leaves(&b).len());
    }

    #[test]
    fn production_source_guard() {
        let source = include_str!("../src/resolution.rs");
        for token in [
            "std::fs",
            "File::open",
            "include_str!",
            "fixtures/",
            "reqwest",
            "std::net",
            "SystemClock",
            "Utc::now",
            "Instant::now",
            "now()",
            "plumb_inference",
            "InferenceRequest",
            "rand",
            "Uuid",
            "uuid",
            "ulid",
            "Ulid",
            ".prompt",
            "Regex",
            "regex",
            ".name ==",
            "route_question",
            "generate_questions",
            "impact_reachable_nodes",
            "register_f2",
            "inverse(",
            "ArtifactStore",
            "plumb_artifacts",
            "f64",
        ] {
            assert!(!source.contains(token), "resolution.rs contains {token}");
        }
        let lib = include_str!("../src/lib.rs");
        assert!(lib.contains("pub mod resolution;"));
        assert!(!lib.contains("Proposal"));
    }
}

/// Shared builders of the compatibility modules: the real S3.1 engine and S3.3 resolution
/// between two runs of an existing S2 compiler.
mod compat_support {
    use plumb_core::{Id, Timestamp};
    use plumb_functional::question::{
        generate_questions, QuestionAudit, QuestionGenerationInput, StakeholderRoutingConfig,
    };
    use plumb_functional::resolution::{
        resolve_question, ResolutionContext, ResolutionInput, ResolutionResult,
    };
    use plumb_patch::apply_patch;
    use plumb_psg::{
        Agent, AgentKind, AuditMeta, ElementStatus, Graph, Node, NodePayload, Stakeholder,
    };
    use plumb_validation::GeneratedFinding;
    use serde_json::Value;

    pub const HUMAN: &str = "agent:human";
    const HR_STAKEHOLDERS: &str = include_str!("../../../fixtures/hr-leave/stakeholders.yaml");

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn governance_node(node_id: &str, payload: NodePayload) -> Node {
        Node {
            id: id(node_id),
            revision: 1,
            status: ElementStatus::Accepted,
            payload,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: Default::default(),
            extensions: Default::default(),
            audit: AuditMeta::new(
                id("agent:analyst"),
                ts("2026-01-01T00:00:00.000000000Z"),
                None,
                None,
            )
            .unwrap(),
        }
    }

    /// The human Agent and the routed HR Stakeholder.
    pub fn governance_nodes() -> Vec<Node> {
        vec![
            governance_node(
                HUMAN,
                NodePayload::Agent(Agent {
                    agent_kind: AgentKind::Human,
                }),
            ),
            governance_node(
                "stakeholder:hr",
                NodePayload::Stakeholder(Stakeholder {
                    name: "HR".into(),
                    stakeholder_kind: "internal".into(),
                    organization: None,
                    responsibilities: None,
                    contact_ref: None,
                }),
            ),
        ]
    }

    /// S3.1 asks one question for `finding`; S3.3 answers it; both proposals are applied.
    pub fn ask_and_answer(
        graph: &Graph,
        finding: GeneratedFinding,
        answer: Value,
    ) -> (Graph, ResolutionResult) {
        let generated = generate_questions(
            graph,
            &QuestionGenerationInput {
                findings: vec![finding],
                routing: StakeholderRoutingConfig::from_yaml(HR_STAKEHOLDERS).unwrap(),
            },
            &QuestionAudit {
                created_by: id("agent:compiler"),
                created_at: ts("2026-02-01T00:00:00.000000000Z"),
            },
        )
        .unwrap();
        assert_eq!(generated.questions.len(), 1, "{:?}", generated.issues);
        let question_ref = generated.questions[0].question_ref.clone();
        let asked = apply_patch(graph, &generated.proposal.unwrap().patch_set)
            .unwrap()
            .graph;
        let result = resolve_question(
            &asked,
            &ResolutionInput {
                question_ref,
                answer,
                decided_by: id(HUMAN),
                decided_at: ts("2026-03-01T09:30:00.000000000Z"),
                rationale: Some("Confirmed with the policy owner.".into()),
                supersedes: None,
            },
            &ResolutionContext::default(),
        )
        .unwrap();
        let resolved = apply_patch(&asked, &result.proposal.patch_set)
            .unwrap()
            .graph;
        (resolved, result)
    }
}

/// The S3.3 cardinality decision is consumed unchanged by the existing domain.rs reader.
mod domain_compatibility {
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, Timestamp};
    use plumb_functional::domain::*;
    use plumb_import::{import_plain_text, ImportAudit};
    use plumb_inference::{InferenceArtifact, InferenceRequest, ProviderPolicy};
    use plumb_patch::{apply_patch, PatchSet, Proposal, SemanticPatch};
    use plumb_psg::{
        AuditMeta, Concept, ConceptKind, DerivationRef, ElementStatus, EvidenceRef, Graph,
        Modality, Node, NodePayload, NodeType, Requirement, RequirementKind, RequirementLevel,
    };
    use serde_json::{json, Value};

    use super::compat_support::{ask_and_answer, governance_nodes};

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const R1: &str = "Every Employee submits Leave Request records; each Leave Request belongs to \
exactly one Employee, and an Employee may hold zero or more of them.";
    const R2: &str = "Each Leave Request shall have a mandatory Start Date of type Date.";

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn node(node_id: &str, payload: NodePayload) -> Node {
        Node {
            id: id(node_id),
            revision: 1,
            status: ElementStatus::Accepted,
            payload,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: Default::default(),
            extensions: Default::default(),
            audit: AuditMeta::new(id("agent:analyst"), ts(AT), None, None).unwrap(),
        }
    }

    fn requirement(node_id: &str, statement: &str) -> Node {
        node(
            node_id,
            NodePayload::Requirement(Requirement {
                statement: statement.into(),
                requirement_kind: RequirementKind::Functional,
                level: RequirementLevel::System,
                modality: Modality::Shall,
                title: None,
                rationale: None,
                priority: None,
                source_identifier: None,
                verification_method: None,
                owner_refs: None,
                stakeholder_refs: None,
            }),
        )
    }

    fn concept(node_id: &str, name: &str, kind: ConceptKind) -> Node {
        node(
            node_id,
            NodePayload::Concept(Concept {
                name: name.into(),
                definition: format!("The {name}."),
                concept_kind: kind,
            }),
        )
    }

    fn base_graph() -> Graph {
        let imported = import_plain_text(
            "synthetic.txt",
            [R1, R2].join("\n\n").as_bytes(),
            &ImportAudit {
                created_by: id("actor:importer"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        let refs: Vec<EvidenceRef> = imported
            .fragments
            .iter()
            .map(|f| EvidenceRef::from(f.id.clone()))
            .collect();
        let mut nodes = vec![requirement("req:r1", R1), requirement("req:r2", R2)];
        nodes[0].evidence = vec![refs[0].clone()];
        nodes[1].evidence = vec![refs[1].clone()];
        nodes.extend([
            concept("concept:date", "Date", ConceptKind::ValueType),
            concept("concept:employee", "Employee", ConceptKind::ObjectType),
            concept(
                "concept:leave-request",
                "Leave Request",
                ConceptKind::ObjectType,
            ),
            concept("concept:start-date", "Start Date", ConceptKind::Other),
            concept(
                "concept:submits",
                "Employee submits Leave Request",
                ConceptKind::FactType,
            ),
        ]);
        nodes.push(imported.source);
        nodes.extend(imported.fragments);
        nodes.extend(governance_nodes());
        Graph::new(
            id("project:pilot"),
            id("profile:plumb-software-2026.1"),
            nodes,
            vec![],
        )
        .unwrap()
    }

    fn r1(needle: &str) -> Value {
        let start = R1.find(needle).unwrap();
        json!({"requirement_ref": "req:r1", "start": start, "end": start + needle.len()})
    }

    fn relationship(from: Value, to: Value) -> Value {
        json!({
            "concept_ref": "concept:submits", "grounding": r1("Employee submits Leave Request"),
            "from_concept_ref": "concept:employee", "from_grounding": r1("Employee"),
            "to_concept_ref": "concept:leave-request", "to_grounding": r1("Leave Request"),
            "cardinality_from": from, "cardinality_to": to,
        })
    }

    fn output(entities: Vec<Value>, relationships: Vec<Value>) -> Value {
        json!({"version": 1, "entities": entities, "attributes": [], "relationships": relationships})
    }

    fn analyze(graph: &Graph, output: Value) -> DomainAnalysisResult {
        let request = build_domain_request(
            graph,
            ProviderPolicy {
                provider: "mock".into(),
                config: CanonicalJson::new(json!({})),
            },
        )
        .unwrap();
        let raw = to_canonical_json(&output).unwrap();
        let validated_output = CanonicalJson::new(output);
        let artifact = InferenceArtifact {
            request_hash: request.request.id.clone(),
            provider: "mock".into(),
            model: "mock-model".into(),
            parameters: CanonicalJson::new(json!({})),
            raw_response_hash: Hash::content_sha256(&raw),
            validated_output_hash: validated_output.content_hash().unwrap(),
            validated_output,
        };
        let _: &InferenceRequest = &request.request;
        analyze_domain(
            graph,
            &request,
            Some(DomainInference {
                artifact: &artifact,
                derivation_ref: DerivationRef::from(id("drv:00000000000000d2")),
            }),
            &DomainAudit {
                created_by: id("agent:domain"),
                created_at: ts(AT),
            },
        )
        .unwrap()
    }

    fn apply(graph: &Graph, proposals: &[Proposal]) -> Graph {
        let mut g = graph.clone();
        for p in proposals {
            let patch_set = PatchSet {
                base_semantic_hash: g.semantic_hash().unwrap(),
                patch: p.patch_set.patch.clone(),
            };
            g = apply_patch(&g, &patch_set).unwrap().graph;
        }
        g
    }

    #[test]
    fn s3_3_cardinality_decision_is_consumed_by_domain_analysis() {
        let g = base_graph();
        let entities = vec![
            json!({"concept_ref": "concept:employee", "grounding": r1("Employee")}),
            json!({"concept_ref": "concept:leave-request", "grounding": r1("Leave Request")}),
        ];
        let first = analyze(&g, output(entities, vec![]));
        let with_entities = apply(&g, &first.proposals);
        let unresolved = analyze(
            &with_entities,
            output(vec![], vec![relationship(Value::Null, Value::Null)]),
        );
        assert_eq!(unresolved.findings.len(), 1);
        let finding = unresolved.findings[0].clone();
        let candidate = finding
            .semantic_condition_key
            .split_once(':')
            .unwrap()
            .1
            .to_owned();
        let (resolved, result) = ask_and_answer(
            &with_entities,
            finding,
            json!({"cardinality_from": "1", "cardinality_to": "0..*"}),
        );
        assert_eq!(
            result.decision.answer,
            json!({"kind": "domain_relationship_cardinality", "candidate_ref": candidate,
                   "cardinality_from": "1", "cardinality_to": "0..*"})
        );
        assert_eq!(result.patch_artifact.effect_patch, None);
        // The existing reader consumes it: no unresolved finding, a decided relationship.
        let decided = analyze(
            &resolved,
            output(vec![], vec![relationship(Value::Null, Value::Null)]),
        );
        assert!(decided.findings.is_empty(), "{:?}", decided.findings);
        let relationship_proposal = decided
            .proposals
            .iter()
            .find_map(|p| match &p.patch_set.patch {
                SemanticPatch::Compound { patches } => patches.iter().find_map(|leaf| match leaf {
                    SemanticPatch::AddNode { node }
                        if node.payload.node_type() == NodeType::DomainRelationship =>
                    {
                        Some(node.clone())
                    }
                    _ => None,
                }),
                _ => None,
            })
            .expect("a DomainRelationship proposal");
        let NodePayload::DomainRelationship(r) = &relationship_proposal.payload else {
            unreachable!()
        };
        assert_eq!(
            (r.cardinality_from.as_str(), r.cardinality_to.as_str()),
            ("1", "0..*")
        );
        assert_eq!(relationship_proposal.id.to_string(), candidate);
        let origin = relationship_proposal
            .extensions
            .iter()
            .find(|(k, _)| k.as_str() == DOMAIN_ORIGIN_EXTENSION)
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(
            origin["cardinality_decision_ref"],
            json!(result.decision_ref.to_string())
        );
    }
}

/// The S3.3 trigger decision is consumed unchanged by the existing state.rs reader.
mod lifecycle_compatibility {
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, Timestamp};
    use plumb_functional::state::*;
    use plumb_import::{import_plain_text, ImportAudit};
    use plumb_inference::{InferenceArtifact, ProviderPolicy};
    use plumb_patch::{apply_patch, PatchSet, Proposal, SemanticPatch};
    use plumb_psg::{
        AuditMeta, DerivationRef, ElementStatus, Entity, EvidenceRef, Graph, Modality, Node,
        NodePayload, NodeType, Operation, OperationKind, RelationKind, Requirement,
        RequirementKind, RequirementLevel,
    };
    use serde_json::{json, Value};

    use super::compat_support::{ask_and_answer, governance_nodes};

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const R1: &str = "A request moves from submitted to approved when approval is recorded.";
    const R2: &str = "The approved amount must never exceed the requested amount.";
    const OWNER: &str = "entity:request";
    const OPERATION: &str = "operation:approve";

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn node(node_id: &str, payload: NodePayload) -> Node {
        Node {
            id: id(node_id),
            revision: 1,
            status: ElementStatus::Accepted,
            payload,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            tags: Default::default(),
            extensions: Default::default(),
            audit: AuditMeta::new(id("agent:analyst"), ts(AT), None, None).unwrap(),
        }
    }

    fn requirement(node_id: &str, statement: &str) -> Node {
        node(
            node_id,
            NodePayload::Requirement(Requirement {
                statement: statement.into(),
                requirement_kind: RequirementKind::Functional,
                level: RequirementLevel::System,
                modality: Modality::Shall,
                title: None,
                rationale: None,
                priority: None,
                source_identifier: None,
                verification_method: None,
                owner_refs: None,
                stakeholder_refs: None,
            }),
        )
    }

    fn base_graph() -> Graph {
        let imported = import_plain_text(
            "synthetic.txt",
            [R1, R2].join("\n\n").as_bytes(),
            &ImportAudit {
                created_by: id("actor:importer"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        let refs: Vec<EvidenceRef> = imported
            .fragments
            .iter()
            .map(|f| EvidenceRef::from(f.id.clone()))
            .collect();
        let mut nodes = vec![requirement("req:r1", R1), requirement("req:r2", R2)];
        nodes[0].evidence = vec![refs[0].clone()];
        nodes[1].evidence = vec![refs[1].clone()];
        nodes.push(node(
            OWNER,
            NodePayload::Entity(Entity {
                name: "Request".into(),
                description: None,
                aggregate_root: None,
            }),
        ));
        nodes.push(node(
            OPERATION,
            NodePayload::Operation(Operation {
                name: "ApproveRequest".into(),
                operation_kind: OperationKind::Command,
                input_schema_ref: None,
                output_schema_ref: None,
                preconditions: None,
                postconditions: None,
                idempotency: None,
                transaction_semantics: None,
            }),
        ));
        nodes.push(imported.source);
        nodes.extend(imported.fragments);
        nodes.extend(governance_nodes());
        Graph::new(
            id("project:pilot"),
            id("profile:plumb-software-2026.1"),
            nodes,
            vec![],
        )
        .unwrap()
    }

    fn r1(needle: &str) -> Value {
        let start = R1.find(needle).unwrap();
        json!({"requirement_ref": "req:r1", "start": start, "end": start + needle.len()})
    }

    fn states() -> Vec<Value> {
        vec![
            json!({"stateful_ref": OWNER, "grounding": r1("submitted")}),
            json!({"stateful_ref": OWNER, "grounding": r1("approved")}),
        ]
    }

    fn transition(trigger: Value) -> Value {
        json!({"stateful_ref": OWNER, "grounding": r1("moves from submitted to approved"),
               "from_state": r1("submitted"), "to_state": r1("approved"), "trigger_ref": trigger})
    }

    fn output(states: Vec<Value>, transitions: Vec<Value>) -> Value {
        json!({"version": 1, "states": states, "transitions": transitions, "invariants": []})
    }

    fn analyze(graph: &Graph, output: Value) -> LifecycleAnalysisResult {
        let request = build_lifecycle_request(
            graph,
            ProviderPolicy {
                provider: "mock".into(),
                config: CanonicalJson::new(json!({})),
            },
        )
        .unwrap();
        let raw = to_canonical_json(&output).unwrap();
        let validated_output = CanonicalJson::new(output);
        let artifact = InferenceArtifact {
            request_hash: request.request.id.clone(),
            provider: "mock".into(),
            model: "mock-model".into(),
            parameters: CanonicalJson::new(json!({})),
            raw_response_hash: Hash::content_sha256(&raw),
            validated_output_hash: validated_output.content_hash().unwrap(),
            validated_output,
        };
        analyze_lifecycle(
            graph,
            &request,
            Some(LifecycleInference {
                artifact: &artifact,
                derivation_ref: DerivationRef::from(id("drv:00000000000000e2")),
            }),
            &LifecycleAudit {
                created_by: id("agent:lifecycle"),
                created_at: ts(AT),
            },
        )
        .unwrap()
    }

    fn apply(graph: &Graph, proposals: &[Proposal]) -> Graph {
        let mut g = graph.clone();
        for p in proposals {
            let patch_set = PatchSet {
                base_semantic_hash: g.semantic_hash().unwrap(),
                patch: p.patch_set.patch.clone(),
            };
            g = apply_patch(&g, &patch_set).unwrap().graph;
        }
        g
    }

    #[test]
    fn s3_3_trigger_decision_is_consumed_by_lifecycle_analysis() {
        let g = base_graph();
        let with_states = apply(&g, &analyze(&g, output(states(), vec![])).proposals);
        let unresolved = analyze(&with_states, output(vec![], vec![transition(Value::Null)]));
        assert_eq!(unresolved.findings.len(), 1);
        let finding = unresolved.findings[0].clone();
        let candidate = finding
            .semantic_condition_key
            .split_once(':')
            .unwrap()
            .1
            .to_owned();
        assert!(candidate.starts_with("transition:"));
        let (resolved, result) =
            ask_and_answer(&with_states, finding, json!({"trigger_ref": OPERATION}));
        // Exactly the strict TriggerMarker of state.rs.
        assert_eq!(
            result.decision.answer,
            json!({"kind": "state_transition_trigger", "candidate_ref": candidate, "trigger_ref": OPERATION})
        );
        assert_eq!(result.patch_artifact.effect_patch, None);
        assert!(resolved
            .edge_ids_by_kind(&RelationKind::TransitionsVia)
            .is_empty());
        // The existing reader consumes it: no unresolved finding, a triggered Transition.
        let decided = analyze(&resolved, output(vec![], vec![transition(Value::Null)]));
        assert!(decided.findings.is_empty(), "{:?}", decided.findings);
        let (transition_node, trigger_edge) = decided
            .proposals
            .iter()
            .find_map(|p| match &p.patch_set.patch {
                SemanticPatch::Compound { patches } => match &patches[..] {
                    [SemanticPatch::AddNode { node }, SemanticPatch::AddEdge { edge }]
                        if node.payload.node_type() == NodeType::Transition =>
                    {
                        Some((node.clone(), edge.clone()))
                    }
                    _ => None,
                },
                _ => None,
            })
            .expect("a Transition proposal");
        assert_eq!(transition_node.id.to_string(), candidate);
        assert_eq!(
            (trigger_edge.kind, trigger_edge.from, trigger_edge.to),
            (
                RelationKind::TransitionsVia,
                transition_node.id.clone(),
                id(OPERATION)
            )
        );
        let after = apply(&resolved, &decided.proposals);
        after.validate().unwrap();
    }
}
