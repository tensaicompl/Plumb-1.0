//! S3.1 contract tests for deterministic Question generation and routing (Hotfix 045).
//!
//! Every test lives in `question_contract` (`cargo test -p plumb-functional --test question`).
//! Graphs and findings are synthetic current-v3 material; the routing configuration is the real
//! immutable `fixtures/hr-leave/stakeholders.yaml`. Golden Question IDs were computed
//! independently with Python `hashlib` over RFC 8785 JSON, never with the helper under test.

mod question_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, Id, StageId, Timestamp};
    use plumb_functional::question::*;
    use plumb_functional::question_templates::*;
    use plumb_patch::{apply_patch, AcceptancePolicy, ProposalMateriality, SemanticPatch};
    use plumb_psg::{
        Actor, ActorKind, AuditMeta, BusinessRole, Calculation, Calendar, Edge, ElementStatus,
        Entity, Event, Finding, FindingSeverity, Graph, Invariant, Modality, Node, NodePayload,
        Operation, OperationKind, Permission, Question, QuestionKind, RelationKind,
        RelationProperties, Requirement, RequirementKind, RequirementLevel, ResourceScope,
        SecurityRole, Stakeholder,
    };
    use plumb_validation::{finding_id, finding_key, GeneratedFinding};
    use serde_json::{json, Value as JsonValue};

    use ElementStatus::{Accepted, Proposed, Suspect};

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const STAKEHOLDERS_YAML: &str = include_str!("../../../fixtures/hr-leave/stakeholders.yaml");

    const RELATION_TYPED: &str = "PLUMB.F2.DOMAIN.RELATION_TYPED";
    const TRANSITION_COMPLETE: &str = "PLUMB.F2.STATE.TRANSITION_COMPLETE";
    const PERFORMER: &str = "PLUMB.F2.OPERATION.PERFORMER";
    const CALENDAR_DEFINED: &str = "PLUMB.F2.TIME.CALENDAR_DEFINED";
    const PERMISSION_CONCRETE: &str = "RBAC.F2.PERMISSION.CONCRETE";
    const INVARIANT_EXPRESSIBLE: &str = "PLUMB.F2.INVARIANT.EXPRESSIBLE";
    const CARDINALITY_CONDITION: &str =
        "domain_relationship_cardinality_unresolved:domainrel:leave-employee";
    const TRIGGER_CONDITION: &str = "state_transition_trigger_unresolved:transition:approve";

    // Independently computed goldens (Python hashlib, RFC 8785).
    const GOLDEN_CARDINALITY_FINDING: &str = "fnd:3593cbf36d315144";
    const GOLDEN_CARDINALITY_QUESTION: &str = "q:2a414cddc59ff77f";
    const GOLDEN_CALENDAR_QUESTION: &str = "q:6204ae40674353cc";
    const GOLDEN_FALLBACK_QUESTION: &str = "q:3e64d480866528f3";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ids(list: &[&str]) -> Vec<Id> {
        list.iter().map(|s| id(s)).collect()
    }

    fn meta() -> AuditMeta {
        AuditMeta::new(
            id("agent:analyst"),
            AT.parse::<Timestamp>().unwrap(),
            None,
            None,
        )
        .unwrap()
    }

    fn audit() -> QuestionAudit {
        QuestionAudit {
            created_by: id("agent:compiler"),
            created_at: "2026-02-01T00:00:00.000000000Z".parse().unwrap(),
        }
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

    fn edge(kind: RelationKind, from: &str, to: &str) -> Edge {
        Edge {
            id: id(&format!(
                "rel:{}-{}-{}",
                kind.as_str().replace('_', "-"),
                from.replace(':', "-"),
                to.replace(':', "-")
            )),
            revision: 1,
            status: Accepted,
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

    fn business_role(node_id: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            status,
            NodePayload::BusinessRole(BusinessRole {
                name: node_id.into(),
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

    /// A synthetic current-v3 graph: stakeholders of the HR routing directory, Accepted choice
    /// candidates of every template, Proposed and wrongly typed distractors.
    #[derive(Clone)]
    struct Fx {
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    }

    impl Fx {
        fn new() -> Fx {
            Fx {
                nodes: vec![
                    stakeholder("stakeholder:architect", Accepted),
                    stakeholder("stakeholder:auditor", Accepted),
                    stakeholder("stakeholder:employee", Accepted),
                    stakeholder("stakeholder:hr", Accepted),
                    stakeholder("stakeholder:manager", Accepted),
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
                    operation("operation:approve", Accepted),
                    operation("operation:submit", Accepted),
                    operation("operation:draft", Proposed),
                    node(
                        "actor:bot",
                        Accepted,
                        NodePayload::Actor(Actor {
                            name: "Bot".into(),
                            actor_kind: ActorKind::System,
                        }),
                    ),
                    business_role("brole:clerk", Accepted),
                    business_role("brole:draft", Proposed),
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
                            rounding: None,
                            calendar_ref: None,
                            examples: None,
                        }),
                    ),
                    calendar("calendar:nl", Accepted),
                    calendar("calendar:draft", Proposed),
                    node(
                        "permission:approve",
                        Accepted,
                        NodePayload::Permission(Permission {
                            name: "Approve".into(),
                        }),
                    ),
                    scope("scope:orders", Accepted),
                    scope("scope:draft", Proposed),
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
                    // calculation:days -> operation:approve (uses_calculation is to -> from).
                    edge(
                        RelationKind::UsesCalculation,
                        "operation:approve",
                        "calculation:days",
                    ),
                    edge(
                        RelationKind::Permits,
                        "permission:approve",
                        "operation:approve",
                    ),
                    edge(RelationKind::ScopedTo, "permission:approve", "scope:orders"),
                ],
            }
        }

        fn with(mut self, n: Node) -> Fx {
            self.nodes.push(n);
            self
        }

        fn rel(mut self, kind: RelationKind, from: &str, to: &str) -> Fx {
            self.edges.push(edge(kind, from, to));
            self
        }

        fn status(mut self, node_id: &str, status: ElementStatus) -> Fx {
            for n in &mut self.nodes {
                if n.id == id(node_id) {
                    n.status = status;
                }
            }
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

    /// A current GeneratedFinding; targets may be given in any order.
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

    fn cardinality() -> GeneratedFinding {
        gf(
            RELATION_TYPED,
            CARDINALITY_CONDITION,
            &["req:r1"],
            FindingSeverity::Error,
        )
    }
    fn trigger() -> GeneratedFinding {
        gf(
            TRANSITION_COMPLETE,
            TRIGGER_CONDITION,
            &["req:r1"],
            FindingSeverity::Blocker,
        )
    }
    fn performer() -> GeneratedFinding {
        gf(
            PERFORMER,
            "operation_performer_missing",
            &["operation:submit", "operation:approve"],
            FindingSeverity::Blocker,
        )
    }
    fn calendar_missing() -> GeneratedFinding {
        gf(
            CALENDAR_DEFINED,
            "business_time_calendar_missing",
            &["calculation:days"],
            FindingSeverity::Blocker,
        )
    }
    fn permission() -> GeneratedFinding {
        gf(
            PERMISSION_CONCRETE,
            "permission_not_concrete",
            &["permission:approve"],
            FindingSeverity::Blocker,
        )
    }
    fn invariant() -> GeneratedFinding {
        gf(
            INVARIANT_EXPRESSIBLE,
            "invariant_not_expressible",
            &["inv:positive"],
            FindingSeverity::Error,
        )
    }

    fn all_mapped() -> Vec<GeneratedFinding> {
        vec![
            cardinality(),
            trigger(),
            performer(),
            calendar_missing(),
            permission(),
            invariant(),
        ]
    }

    fn hr_routing() -> StakeholderRoutingConfig {
        StakeholderRoutingConfig::from_yaml(STAKEHOLDERS_YAML).unwrap()
    }

    fn run_with(
        fx: &Fx,
        findings: Vec<GeneratedFinding>,
        routing: StakeholderRoutingConfig,
    ) -> QuestionGenerationResult {
        generate_questions(
            &fx.graph(),
            &QuestionGenerationInput { findings, routing },
            &audit(),
        )
        .unwrap()
    }

    fn run(fx: &Fx, findings: Vec<GeneratedFinding>) -> QuestionGenerationResult {
        run_with(fx, findings, hr_routing())
    }

    fn only(result: &QuestionGenerationResult) -> &GeneratedQuestion {
        assert_eq!(result.questions.len(), 1, "{:?}", result.issues);
        &result.questions[0]
    }

    fn strings(ids: &[Id]) -> Vec<String> {
        ids.iter().map(Id::to_string).collect()
    }

    fn added_nodes(result: &QuestionGenerationResult) -> Vec<Node> {
        match &result.proposal.as_ref().expect("proposal").patch_set.patch {
            SemanticPatch::AddNode { node } => vec![node.clone()],
            SemanticPatch::Compound { patches } => patches
                .iter()
                .map(|p| match p {
                    SemanticPatch::AddNode { node } => node.clone(),
                    other => panic!("unexpected patch {other:?}"),
                })
                .collect(),
            other => panic!("unexpected patch {other:?}"),
        }
    }

    // ------------------------------------------------------------------ templates

    #[test]
    fn template_registry_is_closed_and_exact() {
        let registry: Vec<(&str, &str, QuestionKind, &str)> = QUESTION_TEMPLATES
            .iter()
            .map(|t| (t.template_id, t.code, t.kind, t.routing_key))
            .collect();
        assert_eq!(
            registry,
            [
                (
                    "f2.domain.relationship.cardinality.v1",
                    RELATION_TYPED,
                    QuestionKind::Cardinality,
                    "default"
                ),
                (
                    "f2.invariant.formula.v1",
                    INVARIANT_EXPRESSIBLE,
                    QuestionKind::FormulaConfirm,
                    "formula"
                ),
                (
                    "f2.operation.performer.v1",
                    PERFORMER,
                    QuestionKind::RoleAssignment,
                    "default"
                ),
                (
                    "f2.permission.concrete.v1",
                    PERMISSION_CONCRETE,
                    QuestionKind::Permission,
                    "authorization"
                ),
                (
                    "f2.state.transition.trigger.v1",
                    TRANSITION_COMPLETE,
                    QuestionKind::PickOne,
                    "default"
                ),
                (
                    "f2.time.calendar.v1",
                    CALENDAR_DEFINED,
                    QuestionKind::Calendar,
                    "calendar"
                ),
            ]
        );
    }

    #[test]
    fn template_matching_is_exact() {
        let matched =
            |code: &str, condition: &str| match_template(code, condition).map(|t| t.template_id);
        assert_eq!(
            matched(RELATION_TYPED, CARDINALITY_CONDITION),
            Ok(CARDINALITY_TEMPLATE)
        );
        assert_eq!(
            matched(RELATION_TYPED, "domain_relationship_untyped"),
            Err(UnmappedReason::ConditionNotQuestionResolvable)
        );
        assert_eq!(
            matched(
                RELATION_TYPED,
                "xdomain_relationship_cardinality_unresolved:a"
            ),
            Err(UnmappedReason::ConditionNotQuestionResolvable)
        );
        assert_eq!(
            matched(TRANSITION_COMPLETE, TRIGGER_CONDITION),
            Ok(TRIGGER_TEMPLATE)
        );
        assert_eq!(
            matched(TRANSITION_COMPLETE, "state_transition_incomplete"),
            Err(UnmappedReason::ConditionNotQuestionResolvable)
        );
        assert_eq!(
            matched(PERFORMER, "operation_performer_missing"),
            Ok(PERFORMER_TEMPLATE)
        );
        assert_eq!(
            matched(PERFORMER, "operation_performer_missing:x"),
            Err(UnmappedReason::ConditionNotQuestionResolvable)
        );
        assert_eq!(
            matched(CALENDAR_DEFINED, "business_time_calendar_missing"),
            Ok(CALENDAR_TEMPLATE)
        );
        assert_eq!(
            matched(PERMISSION_CONCRETE, "permission_not_concrete"),
            Ok(PERMISSION_TEMPLATE)
        );
        assert_eq!(
            matched(INVARIANT_EXPRESSIBLE, "invariant_not_expressible"),
            Ok(INVARIANT_TEMPLATE)
        );
        for code in [
            "PLUMB.F2.CALC.TYPECHECK",
            "DMN.F2.TABLE.NO_OVERLAP",
            "DMN.F2.TABLE.COVERAGE",
            "PLUMB.F2.DOMAIN.ATTRIBUTE_OWNER",
            "BPMN.F2.PROCESS.START_END",
            "PLUMB.F2.NO_UNRESOLVED_SEMANTIC_BLOCKER",
            "PLUMB.I0.SOURCE.CONTENT_ADDRESSED",
            "PLUMB.F1.REQ.GROUNDED",
            "ACME.UNKNOWN",
        ] {
            assert_eq!(
                matched(code, "anything"),
                Err(UnmappedReason::NoQuestionTemplate),
                "{code}"
            );
        }
    }

    #[test]
    fn unmapped_findings_are_reported_not_discarded() {
        let fx = Fx::new();
        let typecheck = gf(
            "PLUMB.F2.CALC.TYPECHECK",
            "calculation_not_qualified",
            &["calculation:days"],
            FindingSeverity::Blocker,
        );
        let table = gf(
            "DMN.F2.TABLE.COVERAGE",
            "decision_table_domain_uncovered",
            &["entity:order"],
            FindingSeverity::Error,
        );
        let generic = gf(
            RELATION_TYPED,
            "domain_relationship_untyped",
            &["req:r1"],
            FindingSeverity::Error,
        );
        let unknown = gf(
            "ACME.UNKNOWN",
            "whatever",
            &["req:r1"],
            FindingSeverity::Info,
        );
        let result = run(
            &fx,
            vec![
                unknown.clone(),
                typecheck.clone(),
                table.clone(),
                generic.clone(),
            ],
        );
        assert!(result.questions.is_empty());
        assert!(result.proposal.is_none());
        let mut expected = vec![
            UnmappedFinding {
                finding_ref: typecheck.id.clone(),
                code: "PLUMB.F2.CALC.TYPECHECK".into(),
                semantic_condition_key: "calculation_not_qualified".into(),
                reason: UnmappedReason::NoQuestionTemplate,
            },
            UnmappedFinding {
                finding_ref: table.id.clone(),
                code: "DMN.F2.TABLE.COVERAGE".into(),
                semantic_condition_key: "decision_table_domain_uncovered".into(),
                reason: UnmappedReason::NoQuestionTemplate,
            },
            UnmappedFinding {
                finding_ref: generic.id.clone(),
                code: RELATION_TYPED.into(),
                semantic_condition_key: "domain_relationship_untyped".into(),
                reason: UnmappedReason::ConditionNotQuestionResolvable,
            },
            UnmappedFinding {
                finding_ref: unknown.id.clone(),
                code: "ACME.UNKNOWN".into(),
                semantic_condition_key: "whatever".into(),
                reason: UnmappedReason::NoQuestionTemplate,
            },
        ];
        expected.sort();
        assert_eq!(result.unmapped, expected);
    }

    // ------------------------------------------------------------------ answer schemas

    #[test]
    fn answer_schemas_are_exact() {
        let fx = Fx::new();
        let cardinal = json!({"type": "string", "enum": ["0..1", "1", "0..*", "1..*"]});
        let q = only(&run(&fx, vec![cardinality()])).payload.clone();
        assert_eq!(q.question_kind, QuestionKind::Cardinality);
        assert_eq!(
            q.prompt,
            "What are the endpoint cardinalities for relationship candidate domainrel:leave-employee?"
        );
        assert_eq!(
            q.answer_schema.unwrap(),
            json!({"type": "object", "additionalProperties": false,
                   "required": ["cardinality_from", "cardinality_to"],
                   "properties": {"cardinality_from": cardinal, "cardinality_to": cardinal}})
        );
        let q = only(&run(&fx, vec![trigger()])).payload.clone();
        assert_eq!(q.question_kind, QuestionKind::PickOne);
        assert_eq!(
            q.prompt,
            "Which accepted operation or event triggers transition candidate transition:approve?"
        );
        assert_eq!(
            q.answer_schema.unwrap(),
            json!({"type": "object", "additionalProperties": false, "required": ["trigger_ref"],
                   "properties": {"trigger_ref": {"type": "string",
                       "enum": ["event:submitted", "operation:approve", "operation:submit"]}}})
        );
        let result = run(&fx, vec![performer()]);
        assert_eq!(result.questions.len(), 2);
        for q in &result.questions {
            assert_eq!(q.payload.question_kind, QuestionKind::RoleAssignment);
            assert_eq!(
                q.payload.answer_schema.clone().unwrap(),
                json!({"type": "object", "additionalProperties": false, "required": ["performer_ref"],
                       "properties": {"performer_ref": {"type": "string",
                           "enum": ["actor:bot", "brole:clerk"]}}})
            );
        }
        let prompts: BTreeSet<String> = result
            .questions
            .iter()
            .map(|q| q.payload.prompt.clone())
            .collect();
        assert_eq!(
            prompts,
            BTreeSet::from([
                "Who performs operation operation:approve?".to_owned(),
                "Who performs operation operation:submit?".to_owned(),
            ])
        );
        let q = only(&run(&fx, vec![calendar_missing()])).payload.clone();
        assert_eq!(q.question_kind, QuestionKind::Calendar);
        assert_eq!(
            q.prompt,
            "Which accepted calendar should calculation calculation:days use for business-time evaluation?"
        );
        assert_eq!(
            q.answer_schema.unwrap(),
            json!({"type": "object", "additionalProperties": false, "required": ["calendar_ref"],
                   "properties": {"calendar_ref": {"type": "string", "enum": ["calendar:nl"]}}})
        );
        let q = only(&run(&fx, vec![permission()])).payload.clone();
        assert_eq!(q.question_kind, QuestionKind::Permission);
        assert_eq!(
            q.prompt,
            "Which accepted operation and resource scope should permission permission:approve reference?"
        );
        assert_eq!(
            q.answer_schema.unwrap(),
            json!({"type": "object", "additionalProperties": false,
                   "required": ["operation_ref", "resource_scope_ref"],
                   "properties": {
                       "operation_ref": {"type": "string", "enum": ["operation:approve", "operation:submit"]},
                       "resource_scope_ref": {"type": "string", "enum": ["scope:orders"]}}})
        );
        let q = only(&run(&fx, vec![invariant()])).payload.clone();
        assert_eq!(q.question_kind, QuestionKind::FormulaConfirm);
        assert_eq!(
            q.prompt,
            "What boolean PlumbExpr predicate should invariant inv:positive use?"
        );
        assert_eq!(
            q.answer_schema.unwrap(),
            json!({"type": "object", "additionalProperties": false, "required": ["expression"],
                   "properties": {"expression": {"type": "string", "minLength": 1}}})
        );
    }

    #[test]
    fn choices_are_sorted_accepted_and_typed() {
        // Proposed candidates (operation:draft, brole:draft, calendar:draft, scope:draft) and
        // wrongly typed nodes (secrole:approver for performers) never appear.
        let graph = Fx::new().graph();
        assert_eq!(
            strings(&accepted_choices(
                &graph,
                &[
                    plumb_psg::NodeType::Actor,
                    plumb_psg::NodeType::BusinessRole
                ]
            )),
            ["actor:bot", "brole:clerk"]
        );
        // Insertion order of choice nodes does not matter.
        let mut fx = Fx::new()
            .with(business_role("brole:alpha", Accepted))
            .with(operation("operation:zeta", Accepted));
        fx.nodes.reverse();
        let result = run(&fx, vec![performer(), trigger()]);
        let performer_q = result
            .questions
            .iter()
            .find(|q| q.template_id == PERFORMER_TEMPLATE)
            .unwrap();
        assert_eq!(
            performer_q.slots["choice_refs"],
            json!(["actor:bot", "brole:alpha", "brole:clerk"])
        );
        let trigger_q = result
            .questions
            .iter()
            .find(|q| q.template_id == TRIGGER_TEMPLATE)
            .unwrap();
        assert_eq!(
            trigger_q.slots["choice_refs"],
            json!([
                "event:submitted",
                "operation:approve",
                "operation:submit",
                "operation:zeta"
            ])
        );
    }

    #[test]
    fn empty_choices_generate_no_question() {
        let fx = Fx::new().status("calendar:nl", Proposed);
        let result = run(&fx, vec![calendar_missing()]);
        assert!(result.questions.is_empty());
        assert!(result.proposal.is_none());
        assert_eq!(
            result.issues,
            [QuestionIssue::NoAnswerChoices {
                finding_ref: calendar_missing().id,
                template_id: CALENDAR_TEMPLATE.into(),
                subject_ref: id("calculation:days"),
            }]
        );
        // Permission needs both lists.
        let fx = Fx::new().status("scope:orders", Suspect);
        let result = run(&fx, vec![permission()]);
        assert!(result.questions.is_empty());
        assert!(matches!(
            result.issues[0],
            QuestionIssue::NoAnswerChoices { .. }
        ));
    }

    // ------------------------------------------------------------------ routing

    fn config(
        stakeholders: &[(&str, &[&str])],
        routing: &[(&str, &[&str])],
    ) -> StakeholderRoutingConfig {
        StakeholderRoutingConfig {
            stakeholders: stakeholders
                .iter()
                .map(|(sid, roles)| RoutingStakeholder {
                    id: id(sid),
                    name: format!("Name of {sid}"),
                    roles: roles.iter().map(|r| (*r).to_owned()).collect(),
                })
                .collect(),
            routing: routing
                .iter()
                .map(|(key, targets)| ((*key).to_owned(), ids(targets)))
                .collect(),
        }
    }

    #[test]
    fn routing_config_parses_and_validates() {
        let hr = hr_routing();
        assert_eq!(hr.stakeholders.len(), 5);
        assert_eq!(
            hr.routing["authorization"],
            ids(&["stakeholder:hr", "stakeholder:architect"])
        );
        let invalid = [
            config(&[("stakeholder:a", &["x"]), ("stakeholder:a", &["y"])], &[]),
            config(&[("stakeholder:a", &[])], &[]),
            config(&[("stakeholder:a", &["x", "x"])], &[]),
            config(&[("stakeholder:a", &[" x"])], &[]),
            config(
                &[("stakeholder:a", &["x"])],
                &[("default", &["stakeholder:ghost"])],
            ),
            config(
                &[("stakeholder:a", &["x"])],
                &[("default", &["stakeholder:a", "stakeholder:a"])],
            ),
            config(&[("stakeholder:a", &["x"])], &[("", &["stakeholder:a"])]),
        ];
        for c in invalid {
            assert!(
                matches!(
                    c.validate(),
                    Err(QuestionError::InvalidRoutingConfig { .. })
                ),
                "{c:?}"
            );
        }
        let mut blank = config(&[("stakeholder:a", &["x"])], &[]);
        blank.stakeholders[0].name = " ".into();
        assert!(blank.validate().is_err());
        assert!(
            StakeholderRoutingConfig::from_yaml("stakeholders: []\nrouting: {}\nextra: 1\n")
                .is_err()
        );
    }

    #[test]
    fn routing_uses_exact_key_then_default() {
        let graph = Fx::new().graph();
        let c = config(
            &[
                ("stakeholder:hr", &["business"]),
                ("stakeholder:architect", &["architect"]),
            ],
            &[
                ("calendar", &["stakeholder:architect"]),
                ("default", &["stakeholder:hr"]),
            ],
        );
        assert_eq!(
            route_question(&graph, &c, "calendar"),
            (
                Some(id("stakeholder:architect")),
                RouteDisposition::Configured {
                    routing_key: "calendar".into()
                }
            )
        );
        assert_eq!(
            route_question(&graph, &c, "formula"),
            (
                Some(id("stakeholder:hr")),
                RouteDisposition::Configured {
                    routing_key: "default".into()
                }
            )
        );
    }

    #[test]
    fn routing_walks_preference_order() {
        let hr = hr_routing();
        // Both Accepted: the declared first preference wins, never fan-out.
        let both = Fx::new().graph();
        assert_eq!(
            route_question(&both, &hr, "authorization").0,
            Some(id("stakeholder:hr"))
        );
        let result = run(&Fx::new(), vec![permission()]);
        assert_eq!(
            only(&result).payload.stakeholder_ref,
            Some(id("stakeholder:hr"))
        );
        // First missing, second Accepted.
        let second = Fx::new().status("stakeholder:hr", Proposed).graph();
        assert_eq!(
            route_question(&second, &hr, "authorization"),
            (
                Some(id("stakeholder:architect")),
                RouteDisposition::Configured {
                    routing_key: "authorization".into()
                }
            )
        );
    }

    #[test]
    fn routing_falls_back_to_analyst_or_none() {
        let c = config(
            &[
                ("stakeholder:hr", &["business"]),
                ("stakeholder:manager", &["analyst"]),
                ("stakeholder:auditor", &["analyst", "auditor"]),
                ("stakeholder:architect", &["analysts"]),
            ],
            &[("default", &["stakeholder:hr"])],
        );
        let fx = Fx::new().status("stakeholder:hr", Proposed);
        assert_eq!(
            route_question(&fx.graph(), &c, "formula"),
            (
                Some(id("stakeholder:auditor")),
                RouteDisposition::AnalystFallback
            )
        );
        // A Proposed analyst is skipped; the smallest Accepted one is chosen.
        let fx = fx.status("stakeholder:auditor", Proposed);
        assert_eq!(
            route_question(&fx.graph(), &c, "formula"),
            (
                Some(id("stakeholder:manager")),
                RouteDisposition::AnalystFallback
            )
        );
        // No Accepted analyst: unassigned; "analysts" is not "analyst".
        let fx = fx.status("stakeholder:manager", Proposed);
        assert_eq!(
            route_question(&fx.graph(), &c, "formula"),
            (None, RouteDisposition::AnalystRoleUnassigned)
        );
        // The HR fixture: the analyst is stakeholder:hr itself, so without it nobody is invented.
        let hr_gone = Fx::new().status("stakeholder:hr", Proposed);
        let result = run(&hr_gone, vec![invariant()]);
        let q = only(&result);
        assert_eq!(q.payload.stakeholder_ref, None);
        assert_eq!(q.route, RouteDisposition::AnalystRoleUnassigned);
        assert_eq!(q.slots["stakeholder_ref"], JsonValue::Null);
        // No route list at all and no analyst.
        let empty = config(&[("stakeholder:x", &["business"])], &[]);
        assert_eq!(
            route_question(&Fx::new().graph(), &empty, "default"),
            (None, RouteDisposition::AnalystRoleUnassigned)
        );
    }

    #[test]
    fn routing_never_invents_or_matches_names() {
        // A directory entry without an Accepted Stakeholder node is never used, even by name.
        let c = config(
            &[("stakeholder:ghost", &["analyst"])],
            &[("default", &["stakeholder:ghost"])],
        );
        let result = run_with(&Fx::new(), vec![invariant()], c);
        let q = only(&result);
        assert_eq!(q.payload.stakeholder_ref, None);
        let proposal_nodes = added_nodes(&result);
        assert!(proposal_nodes
            .iter()
            .all(|n| !matches!(n.payload, NodePayload::Stakeholder(_))));
    }

    // ------------------------------------------------------------------ priority

    #[test]
    fn priority_weights_and_blast_radius() {
        assert_eq!(severity_weight(FindingSeverity::Blocker), 4);
        assert_eq!(severity_weight(FindingSeverity::Error), 3);
        assert_eq!(severity_weight(FindingSeverity::Warn), 2);
        assert_eq!(severity_weight(FindingSeverity::Info), 1);
        let fx = Fx::new();
        // Seed only: radius 1.
        let q = only(&run(&fx, vec![invariant()])).clone();
        assert_eq!((q.blast_radius, q.priority), (1, 3));
        assert_eq!(q.payload.priority.as_deref(), Some("3"));
        // calculation:days reaches operation:approve through uses_calculation: radius 2.
        let q = only(&run(&fx, vec![calendar_missing()])).clone();
        assert_eq!((q.blast_radius, q.priority), (2, 8));
        for severity in [FindingSeverity::Warn, FindingSeverity::Info] {
            let f = gf(
                CALENDAR_DEFINED,
                "business_time_calendar_missing",
                &["calculation:days"],
                severity,
            );
            let q = only(&run(&fx, vec![f])).clone();
            assert_eq!(q.priority, severity_weight(severity) * 2);
        }
        // The relation direction matters: operation:approve does not reach calculation:days.
        let op = run(&fx, vec![performer()]);
        assert!(op
            .questions
            .iter()
            .all(|q| q.blast_radius == 2 && q.priority == 8));
        // A larger closure raises priority; reachable nodes are counted once.
        let wider = Fx::new()
            .rel(RelationKind::SpecifiedBy, "req:r1", "operation:approve")
            .rel(RelationKind::SpecifiedBy, "req:r1", "operation:submit");
        let q = only(&run(&wider, vec![cardinality()])).clone();
        assert_eq!((q.blast_radius, q.priority), (3, 9));
        let base = only(&run(&fx, vec![cardinality()])).clone();
        assert!(q.priority > base.priority);
        // Union of affected refs counts shared reachable nodes once.
        let shared = Fx::new().rel(RelationKind::SpecifiedBy, "req:r1", "operation:approve");
        let union = run(
            &shared,
            vec![gf(
                PERFORMER,
                "operation_performer_missing",
                &["operation:approve", "operation:submit"],
                FindingSeverity::Blocker,
            )],
        );
        assert!(union.questions.iter().all(|q| q.blast_radius == 2));
    }

    #[test]
    fn priority_is_canonical_and_shared_by_expanded_questions() {
        let result = run(&Fx::new(), vec![performer()]);
        assert_eq!(result.questions.len(), 2);
        let priorities: BTreeSet<Option<String>> = result
            .questions
            .iter()
            .map(|q| q.payload.priority.clone())
            .collect();
        assert_eq!(priorities, BTreeSet::from([Some("8".to_owned())]));
        for q in &result.questions {
            let p = q.payload.priority.as_deref().unwrap();
            assert!(p.chars().all(|c| c.is_ascii_digit()) && !p.starts_with('0'));
            assert_eq!(p.parse::<u64>().unwrap(), q.priority);
        }
    }

    // ------------------------------------------------------------------ identity

    #[test]
    fn question_id_goldens() {
        // Cardinality, routed to stakeholder:hr through `default`.
        let result = run(&Fx::new(), vec![cardinality()]);
        let q = only(&result);
        assert_eq!(cardinality().id.to_string(), GOLDEN_CARDINALITY_FINDING);
        assert_eq!(
            q.slots,
            json!({"affected_refs": ["req:r1"], "candidate_ref": "domainrel:leave-employee",
                   "finding_ref": GOLDEN_CARDINALITY_FINDING, "priority": "3",
                   "stakeholder_ref": "stakeholder:hr"})
        );
        assert_eq!(q.question_ref.to_string(), GOLDEN_CARDINALITY_QUESTION);
        // Calendar, routed through its own `calendar` key, with a choice list.
        let result = run(&Fx::new(), vec![calendar_missing()]);
        let q = only(&result);
        assert_eq!(
            q.route,
            RouteDisposition::Configured {
                routing_key: "calendar".into()
            }
        );
        assert_eq!(
            q.slots,
            json!({"affected_refs": ["calculation:days"], "choice_refs": ["calendar:nl"],
                   "finding_ref": calendar_missing().id.to_string(), "priority": "8",
                   "stakeholder_ref": "stakeholder:hr", "target_ref": "calculation:days"})
        );
        assert_eq!(q.question_ref.to_string(), GOLDEN_CALENDAR_QUESTION);
        // Invariant via the analyst fallback.
        let c = config(
            &[
                ("stakeholder:hr", &["business"]),
                ("stakeholder:auditor", &["analyst"]),
            ],
            &[("default", &["stakeholder:hr"])],
        );
        let result = run_with(
            &Fx::new().status("stakeholder:hr", Proposed),
            vec![invariant()],
            c,
        );
        let q = only(&result);
        assert_eq!(q.route, RouteDisposition::AnalystFallback);
        assert_eq!(
            q.slots,
            json!({"affected_refs": ["inv:positive"], "finding_ref": invariant().id.to_string(),
                   "priority": "3", "stakeholder_ref": "stakeholder:auditor",
                   "target_ref": "inv:positive"})
        );
        assert_eq!(q.question_ref.to_string(), GOLDEN_FALLBACK_QUESTION);
    }

    #[test]
    fn question_identity_follows_template_and_slots() {
        let project = id("project:pilot");
        let slots = json!({"finding_ref": "fnd:0000000000000001", "affected_refs": ["req:r1"],
                           "candidate_ref": "domainrel:x", "priority": "3", "stakeholder_ref": null});
        let a = question_id(&project, CARDINALITY_TEMPLATE, &slots).unwrap();
        assert_eq!(
            a,
            question_id(&project, CARDINALITY_TEMPLATE, &slots).unwrap()
        );
        assert_ne!(a, question_id(&project, TRIGGER_TEMPLATE, &slots).unwrap());
        assert_ne!(
            a,
            question_id(&id("project:other"), CARDINALITY_TEMPLATE, &slots).unwrap()
        );
        let base = run(&Fx::new(), vec![performer()]);
        let ids_of = |r: &QuestionGenerationResult| -> BTreeSet<Id> {
            r.questions.iter().map(|q| q.question_ref.clone()).collect()
        };
        // Slot change (another target set).
        let fewer = run(
            &Fx::new(),
            vec![gf(
                PERFORMER,
                "operation_performer_missing",
                &["operation:approve"],
                FindingSeverity::Blocker,
            )],
        );
        assert!(ids_of(&fewer).is_disjoint(&ids_of(&base)));
        // Route change.
        let rerouted = run(
            &Fx::new().status("stakeholder:hr", Proposed),
            vec![performer()],
        );
        assert!(ids_of(&rerouted).is_disjoint(&ids_of(&base)));
        // Priority change.
        let warn = run(
            &Fx::new(),
            vec![gf(
                PERFORMER,
                "operation_performer_missing",
                &["operation:approve", "operation:submit"],
                FindingSeverity::Warn,
            )],
        );
        assert!(ids_of(&warn).is_disjoint(&ids_of(&base)));
        // Choice-set change.
        let more = run(
            &Fx::new().with(business_role("brole:lead", Accepted)),
            vec![performer()],
        );
        assert!(ids_of(&more).is_disjoint(&ids_of(&base)));
    }

    #[test]
    fn duplicate_candidates_are_suppressed_by_template_and_slots() {
        let result = run(&Fx::new(), vec![invariant()]);
        let q = only(&result);
        let candidate = |finding: &str| QuestionCandidate {
            question_ref: q.question_ref.clone(),
            template_id: q.template_id.clone(),
            finding_ref: id(finding),
            slots: q.slots.clone(),
            payload: q.payload.clone(),
            priority: q.priority,
            blast_radius: q.blast_radius,
            route: q.route.clone(),
        };
        let (kept, suppressed) = suppress_duplicates(vec![
            candidate("fnd:00000000000000b2"),
            candidate("fnd:00000000000000b1"),
        ])
        .unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].finding_ref, id("fnd:00000000000000b1"));
        assert_eq!(
            suppressed,
            [QuestionDisposition::SuppressedDuplicate {
                question_ref: q.question_ref.clone(),
                template_id: INVARIANT_TEMPLATE.into(),
                finding_ref: id("fnd:00000000000000b2"),
            }]
        );
        // The same prompt under different slots is not a duplicate.
        let mut other = candidate("fnd:00000000000000b3");
        other.slots["priority"] = json!("4");
        let (kept, suppressed) =
            suppress_duplicates(vec![candidate("fnd:00000000000000b1"), other]).unwrap();
        assert_eq!((kept.len(), suppressed.len()), (2, 0));
    }

    // ------------------------------------------------------------------ finding validation

    #[test]
    fn invalid_finding_material_is_rejected() {
        let fx = Fx::new();
        let graph = fx.graph();
        let generate = |findings: Vec<GeneratedFinding>| {
            generate_questions(
                &graph,
                &QuestionGenerationInput {
                    findings,
                    routing: hr_routing(),
                },
                &audit(),
            )
        };
        assert!(matches!(
            generate(vec![invariant(), invariant()]),
            Err(QuestionError::DuplicateFinding(_))
        ));
        let mut forged = invariant();
        forged.semantic_condition_key = "invariant_not_expressible_x".into();
        assert!(matches!(
            generate(vec![forged]),
            Err(QuestionError::InvalidFinding { .. })
        ));
        let mut renamed = invariant();
        renamed.id = id("fnd:0000000000000000");
        assert!(matches!(
            generate(vec![renamed]),
            Err(QuestionError::InvalidFinding { .. })
        ));
        let ghost = gf(
            INVARIANT_EXPRESSIBLE,
            "invariant_not_expressible",
            &["inv:ghost"],
            FindingSeverity::Error,
        );
        assert!(matches!(
            generate(vec![ghost]),
            Err(QuestionError::InvalidFinding { .. })
        ));
        let mut closed = invariant();
        closed.payload.status = "Closed".into();
        assert!(matches!(
            generate(vec![closed]),
            Err(QuestionError::InvalidFinding { .. })
        ));
        let wrong_type = gf(
            PERFORMER,
            "operation_performer_missing",
            &["calculation:days"],
            FindingSeverity::Blocker,
        );
        assert!(matches!(
            generate(vec![wrong_type]),
            Err(QuestionError::InvalidFinding { .. })
        ));
        let mut unsorted = performer();
        unsorted.payload.affected_refs.reverse();
        assert!(matches!(
            generate(vec![unsorted]),
            Err(QuestionError::InvalidFinding { .. })
        ));
    }

    #[test]
    fn waived_and_stale_findings_produce_no_question() {
        let mut waived = invariant();
        waived.payload.waiver_ref = Some(id("dec:waive"));
        let result = run(&Fx::new(), vec![waived.clone()]);
        assert!(result.questions.is_empty() && result.proposal.is_none());
        assert_eq!(
            result.dispositions,
            [QuestionDisposition::WaivedFinding {
                finding_ref: waived.id
            }]
        );
        let stale = Fx::new().status("inv:positive", Suspect);
        let result = run(&stale, vec![invariant()]);
        assert!(result.questions.is_empty() && result.proposal.is_none());
        assert_eq!(
            result.dispositions,
            [QuestionDisposition::StaleFinding {
                finding_ref: invariant().id,
                stale_refs: ids(&["inv:positive"])
            }]
        );
    }

    #[test]
    fn invalid_candidate_suffix_is_an_issue() {
        let bad = gf(
            RELATION_TYPED,
            "domain_relationship_cardinality_unresolved:not an id",
            &["req:r1"],
            FindingSeverity::Error,
        );
        let result = run(&Fx::new(), vec![bad.clone()]);
        assert!(result.questions.is_empty());
        assert_eq!(
            result.issues,
            [QuestionIssue::InvalidCandidateRef {
                finding_ref: bad.id,
                template_id: CARDINALITY_TEMPLATE.into(),
                suffix: "not an id".into(),
            }]
        );
    }

    // ------------------------------------------------------------------ finding materialization

    fn persisted_finding(f: &GeneratedFinding, status: ElementStatus) -> Node {
        node(
            f.id.as_str(),
            status,
            NodePayload::Finding(f.payload.clone()),
        )
    }

    #[test]
    fn missing_finding_is_materialized_once() {
        let result = run(&Fx::new(), vec![performer()]);
        assert_eq!(result.questions.len(), 2);
        let nodes = added_nodes(&result);
        assert_eq!(nodes.len(), 3);
        let finding = &nodes[0];
        assert_eq!(finding.id, performer().id);
        assert_eq!(finding.status, Accepted);
        assert_eq!(finding.revision, 1);
        assert_eq!(finding.payload, NodePayload::Finding(performer().payload));
        assert!(finding.evidence.is_empty() && finding.derivations.is_empty());
        assert_eq!(finding.audit.created_by, id("agent:compiler"));
        assert!(nodes[1..]
            .iter()
            .all(|n| matches!(n.payload, NodePayload::Question(_))));
    }

    #[test]
    fn existing_exact_finding_is_reused() {
        let fx = Fx::new().with(persisted_finding(&invariant(), Accepted));
        let result = run(&fx, vec![invariant()]);
        let nodes = added_nodes(&result);
        assert_eq!(nodes.len(), 1);
        assert!(matches!(nodes[0].payload, NodePayload::Question(_)));
        assert!(result
            .dispositions
            .contains(&QuestionDisposition::ExistingFinding {
                finding_ref: invariant().id
            }));
    }

    #[test]
    fn conflicting_finding_material_blocks_its_questions() {
        let mut changed = invariant().payload;
        changed.message = "a different message".into();
        let conflicts = [
            node(
                invariant().id.as_str(),
                Accepted,
                NodePayload::Finding(changed),
            ),
            persisted_finding(&invariant(), Proposed),
            {
                let mut closed = invariant().payload;
                closed.status = "Closed".into();
                node(
                    invariant().id.as_str(),
                    Accepted,
                    NodePayload::Finding(closed),
                )
            },
            stakeholder(invariant().id.as_str(), Accepted),
        ];
        for conflict in conflicts {
            let fx = Fx::new().with(conflict);
            let result = run(&fx, vec![invariant(), calendar_missing()]);
            assert_eq!(
                result.questions.len(),
                1,
                "only the calendar question survives"
            );
            assert_eq!(result.questions[0].template_id, CALENDAR_TEMPLATE);
            assert!(matches!(
                &result.issues[..],
                [QuestionIssue::ExistingFindingConflict { finding_ref, .. }] if *finding_ref == invariant().id
            ));
            assert!(added_nodes(&result).iter().all(|n| n.id != invariant().id));
        }
    }

    // ------------------------------------------------------------------ question reconciliation

    fn persisted_question(q: &GeneratedQuestion, edit: impl FnOnce(&mut Question)) -> Node {
        let mut payload = q.payload.clone();
        edit(&mut payload);
        node(
            q.question_ref.as_str(),
            Accepted,
            NodePayload::Question(payload),
        )
    }

    #[test]
    fn existing_questions_are_idempotent_across_lifecycle() {
        let first = run(&Fx::new(), vec![invariant()]);
        let q = only(&first).clone();
        assert_eq!(q.materialization, QuestionMaterialization::New);
        let lifecycle: [fn(&mut Question); 3] = [
            |_| {},
            |p| p.round_ref = Some("round:1".parse().unwrap()),
            |p| p.status = "Answered".into(),
        ];
        for edit in lifecycle {
            let fx = Fx::new()
                .with(persisted_finding(&invariant(), Accepted))
                .with(persisted_question(&q, edit));
            let result = run(&fx, vec![invariant()]);
            assert!(result.proposal.is_none());
            assert!(result.issues.is_empty());
            let again = only(&result);
            assert_eq!(again.question_ref, q.question_ref);
            assert_eq!(again.materialization, QuestionMaterialization::Existing);
            assert!(result
                .dispositions
                .contains(&QuestionDisposition::ExistingQuestion {
                    question_ref: q.question_ref.clone()
                }));
        }
    }

    #[test]
    fn existing_question_conflicts_are_not_rewritten() {
        let q = only(&run(&Fx::new(), vec![invariant()])).clone();
        let conflicts = [
            persisted_question(&q, |p| p.prompt = "Something else?".into()),
            persisted_question(&q, |p| p.stakeholder_ref = None),
            persisted_question(&q, |p| p.context_refs = None),
            stakeholder(q.question_ref.as_str(), Accepted),
            {
                let mut n = persisted_question(&q, |_| {});
                n.status = Proposed;
                n
            },
        ];
        for conflict in conflicts {
            let fx = Fx::new()
                .with(persisted_finding(&invariant(), Accepted))
                .with(conflict);
            let result = run(&fx, vec![invariant()]);
            assert!(result.questions.is_empty());
            assert!(result.proposal.is_none());
            assert!(matches!(
                &result.issues[..],
                [QuestionIssue::ExistingQuestionConflict { .. }]
            ));
        }
    }

    // ------------------------------------------------------------------ proposal

    #[test]
    fn proposal_is_atomic_non_semantic_and_hash_neutral() {
        let fx = Fx::new();
        let graph = fx.graph();
        let result = run(&fx, all_mapped());
        let proposal = result.proposal.clone().unwrap();
        assert_eq!(proposal.stage, StageId::S3);
        assert_eq!(proposal.materiality, ProposalMateriality::NonSemantic);
        assert_eq!(
            proposal.acceptance_policy,
            AcceptancePolicy::AutoNonSemantic
        );
        assert_eq!(
            serde_json::to_value(proposal.acceptance_policy).unwrap(),
            json!("AUTO_NON_SEMANTIC")
        );
        assert_eq!(proposal.confidence, None);
        assert!(proposal.evidence_refs.is_empty() && proposal.derivation_refs.is_empty());
        let nodes = added_nodes(&result);
        let findings: Vec<Id> = nodes
            .iter()
            .filter(|n| matches!(n.payload, NodePayload::Finding(_)))
            .map(|n| n.id.clone())
            .collect();
        let questions: Vec<Id> = nodes
            .iter()
            .filter(|n| matches!(n.payload, NodePayload::Question(_)))
            .map(|n| n.id.clone())
            .collect();
        assert_eq!(findings.len(), 6);
        assert_eq!(questions.len(), 7);
        let mut sorted_findings = findings.clone();
        sorted_findings.sort();
        let mut sorted_questions = questions.clone();
        sorted_questions.sort();
        assert_eq!(
            nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            [sorted_findings, sorted_questions].concat()
        );
        assert_eq!(
            proposal.patch_set.base_semantic_hash,
            graph.semantic_hash().unwrap()
        );
        let applied = apply_patch(&graph, &proposal.patch_set).unwrap();
        assert_eq!(
            applied.graph.semantic_hash().unwrap(),
            graph.semantic_hash().unwrap()
        );
        // Every new Question node has the frozen envelope and payload.
        for n in nodes
            .iter()
            .filter(|n| matches!(n.payload, NodePayload::Question(_)))
        {
            assert_eq!((n.status, n.revision), (Accepted, 1));
            assert!(n.extensions.is_empty() && n.evidence.is_empty() && n.tags.is_empty());
            let NodePayload::Question(q) = &n.payload else {
                unreachable!()
            };
            assert_eq!(q.status, "Open");
            assert_eq!(q.round_ref, None);
            assert_eq!(q.context_refs, Some(expected_context(&result, &n.id)));
        }
        // After applying, generation is idempotent and proposes nothing.
        let again = generate_questions(
            &applied.graph,
            &QuestionGenerationInput {
                findings: all_mapped(),
                routing: hr_routing(),
            },
            &audit(),
        )
        .unwrap();
        assert!(again.proposal.is_none());
        assert!(again
            .questions
            .iter()
            .all(|q| q.materialization == QuestionMaterialization::Existing));
        assert_eq!(
            again
                .questions
                .iter()
                .map(|q| &q.question_ref)
                .collect::<Vec<_>>(),
            result
                .questions
                .iter()
                .map(|q| &q.question_ref)
                .collect::<Vec<_>>()
        );
    }

    /// Subject first (Hotfix 047), then the other affected refs in ID order, computed from the
    /// slots without the production helper.
    fn expected_context(result: &QuestionGenerationResult, question_ref: &Id) -> Vec<Id> {
        let q = result
            .questions
            .iter()
            .find(|q| &q.question_ref == question_ref)
            .unwrap();
        let subject = q
            .slots
            .get("target_ref")
            .or(q.slots.get("candidate_ref"))
            .unwrap();
        let subject = id(subject.as_str().unwrap());
        let mut context = vec![subject.clone()];
        for v in q.slots["affected_refs"].as_array().unwrap() {
            let r = id(v.as_str().unwrap());
            if r != subject {
                context.push(r);
            }
        }
        context
    }

    // ------------------------------------------------------------------ subject persistence (Hotfix 047)

    #[test]
    fn candidate_question_stores_candidate_subject_first() {
        let q = only(&run(&Fx::new(), vec![cardinality()])).clone();
        assert_eq!(
            q.payload.context_refs,
            Some(ids(&["domainrel:leave-employee", "req:r1"]))
        );
        let q = only(&run(&Fx::new(), vec![trigger()])).clone();
        assert_eq!(
            q.payload.context_refs,
            Some(ids(&["transition:approve", "req:r1"]))
        );
        // The Question ID is the accepted S3.1 golden.
        let q = only(&run(&Fx::new(), vec![cardinality()])).clone();
        assert_eq!(q.question_ref.to_string(), GOLDEN_CARDINALITY_QUESTION);
    }

    #[test]
    fn per_target_questions_store_their_own_subject_first() {
        let result = run(&Fx::new(), vec![performer()]);
        let mut contexts: Vec<Vec<Id>> = result
            .questions
            .iter()
            .map(|q| q.payload.context_refs.clone().unwrap())
            .collect();
        contexts.sort();
        assert_eq!(
            contexts,
            [
                ids(&["operation:approve", "operation:submit"]),
                ids(&["operation:submit", "operation:approve"]),
            ]
        );
        for q in &result.questions {
            let context = q.payload.context_refs.as_ref().unwrap();
            assert_eq!(context[0].as_str(), q.slots["target_ref"].as_str().unwrap());
            assert!(context[1..].windows(2).all(|w| w[0] < w[1]));
        }
        let subjects: BTreeSet<&Id> = result
            .questions
            .iter()
            .map(|q| &q.payload.context_refs.as_ref().unwrap()[0])
            .collect();
        assert_eq!(subjects.len(), 2);
        // A single-target Question's context is just its subject.
        let q = only(&run(&Fx::new(), vec![calendar_missing()])).clone();
        assert_eq!(q.payload.context_refs, Some(ids(&["calculation:days"])));
        assert_eq!(q.question_ref.to_string(), GOLDEN_CALENDAR_QUESTION);
    }

    #[test]
    fn subject_first_context_helper_is_canonical() {
        let affected = ids(&["a:1", "a:2", "a:3"]);
        assert_eq!(
            subject_first_context(&id("a:2"), &affected),
            ids(&["a:2", "a:1", "a:3"])
        );
        assert_eq!(
            subject_first_context(&id("c:9"), &affected),
            ids(&["c:9", "a:1", "a:2", "a:3"])
        );
    }

    #[test]
    fn single_add_is_not_wrapped_in_a_compound() {
        let fx = Fx::new().with(persisted_finding(&invariant(), Accepted));
        let result = run(&fx, vec![invariant()]);
        assert!(matches!(
            result.proposal.unwrap().patch_set.patch,
            SemanticPatch::AddNode { .. }
        ));
    }

    // ------------------------------------------------------------------ determinism

    #[test]
    fn generation_is_deterministic_under_reordering() {
        let forward = run(&Fx::new(), all_mapped());
        let mut fx = Fx::new();
        fx.nodes.reverse();
        fx.edges.reverse();
        let mut findings = all_mapped();
        findings.reverse();
        // Affected refs constructed in reverse order canonicalize identically.
        findings.push(gf(
            PERFORMER,
            "operation_performer_missing",
            &["operation:approve", "operation:submit"],
            FindingSeverity::Blocker,
        ));
        findings.retain({
            let mut seen = BTreeSet::new();
            move |f| seen.insert(f.id.clone())
        });
        let mut routing = hr_routing();
        routing.stakeholders.reverse();
        let reversed = run_with(&fx, findings, routing);
        assert_eq!(forward.questions, reversed.questions);
        assert_eq!(forward.dispositions, reversed.dispositions);
        assert_eq!(forward.unmapped, reversed.unmapped);
        assert_eq!(forward.issues, reversed.issues);
        assert_eq!(
            to_canonical_json(&forward.proposal).unwrap(),
            to_canonical_json(&reversed.proposal).unwrap()
        );
        // Routing preference order is semantic and is not sorted away.
        let mut swapped = hr_routing();
        swapped.routing.insert(
            "authorization".into(),
            ids(&["stakeholder:architect", "stakeholder:hr"]),
        );
        let q = only(&run_with(&Fx::new(), vec![permission()], swapped)).clone();
        assert_eq!(q.payload.stakeholder_ref, Some(id("stakeholder:architect")));
    }

    // ------------------------------------------------------------------ HR routing fixture adapter

    /// Routing/question-engine fixture adapter: the real HR stakeholders.yaml routes a synthetic
    /// current-v3 graph and synthetic current findings exercising the six frozen templates. This
    /// is NOT a claim that the current compiled HR graph already emits these findings, and no v2
    /// G_* finding is reproduced.
    #[test]
    fn hr_round_one_adapter_is_deterministic() {
        let result = run(&Fx::new(), all_mapped());
        let order: Vec<(String, &str, u64)> = result
            .questions
            .iter()
            .map(|q| {
                (
                    q.question_ref.to_string(),
                    q.template_id.as_str(),
                    q.priority,
                )
            })
            .collect();
        assert_eq!(
            order,
            [
                ("q:5871379a4480cf38".to_owned(), PERFORMER_TEMPLATE, 8),
                ("q:6204ae40674353cc".to_owned(), CALENDAR_TEMPLATE, 8),
                ("q:c028a0b06ecc6d87".to_owned(), PERFORMER_TEMPLATE, 8),
                ("q:0bb549ba61718eff".to_owned(), PERMISSION_TEMPLATE, 4),
                ("q:6f249ab4436ceb55".to_owned(), TRIGGER_TEMPLATE, 4),
                ("q:2a414cddc59ff77f".to_owned(), CARDINALITY_TEMPLATE, 3),
                ("q:8b4dc01ee3454b93".to_owned(), INVARIANT_TEMPLATE, 3),
            ]
        );
        assert_eq!(
            result.questions[0].payload.prompt,
            "Who performs operation operation:submit?"
        );
        // Priority descends, then Question IDs ascend.
        for w in result.questions.windows(2) {
            assert!(
                w[0].priority > w[1].priority
                    || (w[0].priority == w[1].priority && w[0].question_ref < w[1].question_ref)
            );
        }
        // Every question routes to the Accepted HR policy owner; nobody is fabricated.
        assert!(result
            .questions
            .iter()
            .all(|q| q.payload.stakeholder_ref == Some(id("stakeholder:hr"))));
        assert_eq!(run(&Fx::new(), all_mapped()), result);
    }

    // ------------------------------------------------------------------ source guards

    #[test]
    fn production_source_guard() {
        for (name, source) in [
            ("question.rs", include_str!("../src/question.rs")),
            (
                "question_templates.rs",
                include_str!("../src/question_templates.rs"),
            ),
        ] {
            for token in [
                "std::fs",
                "File::open",
                "include_str!",
                "include_bytes!",
                "fixtures/",
                "reqwest",
                "std::net",
                "SystemClock",
                "Utc::now",
                "Instant::now",
                "now()",
                "InferenceRequest",
                "InferenceArtifact",
                "InferenceProvider",
                "plumb_inference",
                "rand",
                "f32",
                "f64",
                "unsafe",
                "\"G_",
                ".name ==",
                ".name.",
                "organization",
                "to_lowercase",
                "contains(\"",
            ] {
                assert!(!source.contains(token), "{name} contains {token}");
            }
        }
        let lib = include_str!("../src/lib.rs");
        assert!(!lib.contains("Proposal"));
        assert!(lib.contains("pub mod question;") && lib.contains("pub mod question_templates;"));
    }
}
