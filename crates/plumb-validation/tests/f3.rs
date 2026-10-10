//! S3.5 contract tests for the six F3 evaluators and the F3 supplemental inputs (Hotfix 049).
//!
//! Every test lives in `f3_contract` (`cargo test -p plumb-validation --test f3`). The governed
//! fixture is produced by the real S3.1-S3.4 engines (plumb-functional is a dev-dependency only):
//! questions, answers, waiver and assumption, with the S2.1/S2.2 compiled relationship and
//! transition added as the persisted postconditions of the cardinality and trigger decisions.
//! The legacy fixtures/hr-leave/expert-answers.yaml is never applied. Golden values were
//! computed independently with Python `hashlib`.

mod f3_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{FixedClock, GateId, Hash, Id, Timestamp};
    use plumb_functional::assumption::{
        analyze_assumption_expiry, create_assumption, AssumptionAudit, AssumptionCreation,
        AssumptionInput,
    };
    use plumb_functional::question::{
        generate_questions, QuestionAudit, QuestionGenerationInput, StakeholderRoutingConfig,
    };
    use plumb_functional::resolution::{resolve_question, ResolutionContext, ResolutionInput};
    use plumb_functional::waiver::create_waiver;
    use plumb_patch::{apply_patch, Proposal};
    use plumb_psg::{
        Actor, ActorKind, Agent, AgentKind, Assumption, Attribute, AuditMeta, Calculation,
        Calendar, DomainRelationship, Edge, ElementStatus, Entity, Event, Finding, FindingSeverity,
        Graph, Invariant, Modality, Node, NodePayload, Operation, OperationKind, Permission,
        Question, QuestionKind, RelationKind, RelationProperties, Requirement, RequirementKind,
        RequirementLevel, ResolutionDecision, ResourceScope, Stakeholder, State, Transition,
    };
    use plumb_validation::evaluator::{F3DecisionArtifactInput, F3ValidationInputs};
    use plumb_validation::expression_scope::{ExpressionBindingExposure, ExpressionScopeBinding};
    use plumb_validation::waiver::WaiverRequest;
    use plumb_validation::*;
    use serde_json::json;

    use ElementStatus::{Accepted, Proposed, Superseded, Suspect};

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const DECIDED_AT: &str = "2026-03-01T09:30:00.000000000Z";
    const HR_STAKEHOLDERS: &str = include_str!("../../../fixtures/hr-leave/stakeholders.yaml");
    const HR_EXPERT_ANSWERS: &str = include_str!("../../../fixtures/hr-leave/expert-answers.yaml");
    const HUMAN: &str = "agent:human";
    const CARDINALITY_CANDIDATE: &str = "domainrel:order-customer";
    const TRIGGER_CANDIDATE: &str = "transition:approve";
    const WAIVED_RULE: &str = "PLUMB.F2.DOMAIN.ATTRIBUTE_OWNER";

    const NO_BLOCKING_OPEN: &str = "PLUMB.F3.QUESTION.NO_BLOCKING_OPEN";
    const PATCH_APPLIED: &str = "PLUMB.F3.DECISION.PATCH_APPLIED";
    const DECISION_EVIDENCE: &str = "PLUMB.F3.DECISION.EVIDENCE";
    const ASSUMPTION_OWNER: &str = "PLUMB.F3.ASSUMPTION.OWNER";
    const ASSUMPTION_NOT_EXPIRED: &str = "PLUMB.F3.ASSUMPTION.NOT_EXPIRED";
    const WAIVER_DECISION: &str = "PLUMB.F3.WAIVER.DECISION";
    const F3_RULES: [&str; 6] = [
        ASSUMPTION_NOT_EXPIRED,
        ASSUMPTION_OWNER,
        PATCH_APPLIED,
        DECISION_EVIDENCE,
        NO_BLOCKING_OPEN,
        WAIVER_DECISION,
    ];

    // Independently computed goldens (Python hashlib).
    const GOLDEN_F3_KEY: &str =
        "sha256:6a496765f60cc1a1bcfba9da10742f945b0c92fa20eb326a583fa894d171ce0d";
    const GOLDEN_F3_FINDING: &str = "fnd:6a496765f60cc1a1";

    use RuleResultState::{Error as ERROR, Fail as FAIL, NotApplicable as NA, Pass as PASS};

    // ------------------------------------------------------------------ builders

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

    fn edge(
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

    fn entity(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Entity(Entity {
                name: node_id.into(),
                description: None,
                aggregate_root: None,
            }),
        )
    }

    fn scope(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::ResourceScope(ResourceScope {
                resource_ref: id("entity:order"),
                scope_kind: "entity".into(),
            }),
        )
    }

    fn base_nodes() -> Vec<Node> {
        vec![
            agent(HUMAN, Accepted, AgentKind::Human),
            agent("agent:compiler", Accepted, AgentKind::CompilerStage),
            agent("agent:service", Accepted, AgentKind::SoftwareService),
            agent("agent:intern", Proposed, AgentKind::Human),
            node(
                "stakeholder:hr",
                Accepted,
                NodePayload::Stakeholder(Stakeholder {
                    name: "HR Policy Owner".into(),
                    stakeholder_kind: "internal".into(),
                    organization: None,
                    responsibilities: None,
                    contact_ref: None,
                }),
            ),
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
            entity("entity:order"),
            entity("entity:customer"),
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
            node(
                "actor:clerk",
                Accepted,
                NodePayload::Actor(Actor {
                    name: "Clerk".into(),
                    actor_kind: ActorKind::Human,
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
            node(
                "calendar:nl",
                Accepted,
                NodePayload::Calendar(Calendar {
                    time_zone: "Europe/Amsterdam".into(),
                    week_pattern: json!(["Mon", "Tue", "Wed", "Thu", "Fri"]),
                    region: None,
                    holiday_source: None,
                }),
            ),
            node(
                "permission:approve",
                Accepted,
                NodePayload::Permission(Permission {
                    name: "Approve".into(),
                }),
            ),
            scope("scope:orders"),
            node(
                "inv:positive",
                Accepted,
                NodePayload::Invariant(Invariant {
                    scope_ref: id("entity:order"),
                    expression: "amount".into(),
                }),
            ),
            node(
                "state:open",
                Accepted,
                NodePayload::State(State {
                    name: "open".into(),
                }),
            ),
            node(
                "state:approved",
                Accepted,
                NodePayload::State(State {
                    name: "approved".into(),
                }),
            ),
        ]
    }

    fn base_edges() -> Vec<Edge> {
        vec![
            edge(
                "rel:has-amount",
                RelationKind::HasAttribute,
                "entity:order",
                "attr:amount",
                Accepted,
            ),
            edge(
                "rel:permits",
                RelationKind::Permits,
                "permission:approve",
                "operation:old",
                Accepted,
            ),
            edge(
                "rel:scoped",
                RelationKind::ScopedTo,
                "permission:approve",
                "scope:orders",
                Accepted,
            ),
            edge(
                "rel:has-open",
                RelationKind::HasState,
                "entity:order",
                "state:open",
                Accepted,
            ),
            edge(
                "rel:has-approved",
                RelationKind::HasState,
                "entity:order",
                "state:approved",
                Accepted,
            ),
        ]
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

    fn rebuild(graph: &Graph, f: impl FnOnce(&mut Vec<Node>, &mut Vec<Edge>)) -> Graph {
        let mut nodes: Vec<Node> = graph.nodes().values().cloned().collect();
        let mut edges: Vec<Edge> = graph.edges().values().cloned().collect();
        f(&mut nodes, &mut edges);
        graph_of(nodes, edges)
    }

    fn edit_node(graph: &Graph, node_id: &str, f: impl FnOnce(&mut Node)) -> Graph {
        rebuild(graph, |nodes, _| {
            f(nodes.iter_mut().find(|n| n.id == id(node_id)).unwrap())
        })
    }

    fn apply(graph: &Graph, proposal: &Proposal) -> Graph {
        apply_patch(graph, &proposal.patch_set).unwrap().graph
    }

    fn gf(
        code: &str,
        condition: &str,
        targets: &[&str],
        severity: FindingSeverity,
    ) -> GeneratedFinding {
        let mut affected: Vec<Id> = targets.iter().map(|t| id(t)).collect();
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

    fn question_findings() -> Vec<GeneratedFinding> {
        let b = FindingSeverity::Blocker;
        vec![
            gf(
                "PLUMB.F2.DOMAIN.RELATION_TYPED",
                &format!("domain_relationship_cardinality_unresolved:{CARDINALITY_CANDIDATE}"),
                &["req:r1"],
                b,
            ),
            gf(
                "PLUMB.F2.STATE.TRANSITION_COMPLETE",
                &format!("state_transition_trigger_unresolved:{TRIGGER_CANDIDATE}"),
                &["req:r1"],
                b,
            ),
            gf(
                "PLUMB.F2.OPERATION.PERFORMER",
                "operation_performer_missing",
                &["operation:approve"],
                b,
            ),
            gf(
                "PLUMB.F2.TIME.CALENDAR_DEFINED",
                "business_time_calendar_missing",
                &["calculation:days"],
                b,
            ),
            gf(
                "RBAC.F2.PERMISSION.CONCRETE",
                "permission_not_concrete",
                &["permission:approve"],
                b,
            ),
            gf(
                "PLUMB.F2.INVARIANT.EXPRESSIBLE",
                "invariant_not_expressible",
                &["inv:positive"],
                b,
            ),
        ]
    }

    fn waived_finding() -> GeneratedFinding {
        gf(
            WAIVED_RULE,
            "attribute-owner-condition",
            &["attr:amount"],
            FindingSeverity::Blocker,
        )
    }

    /// The fully governed fixture and its F3 inputs.
    struct Governed {
        graph: Graph,
        artifacts: Vec<F3DecisionArtifactInput>,
        decisions: BTreeMap<String, Id>,
        assumption: Id,
    }

    fn question_about(graph: &Graph, subject: &str) -> Id {
        graph
            .nodes()
            .values()
            .find(|n| {
                matches!(&n.payload, NodePayload::Question(q)
                if q.context_refs.as_ref().is_some_and(|c| c[0] == id(subject)))
            })
            .unwrap()
            .id
            .clone()
    }

    /// Runs S3.1 (questions), S3.3 (six answers), S3.4 (waiver, assumption) and adds the
    /// compiled relationship and transition the S2.1/S2.2 compilers derive from the decisions.
    fn governed() -> Governed {
        let g0 = graph_of(base_nodes(), base_edges());
        let generated = generate_questions(
            &g0,
            &QuestionGenerationInput {
                findings: question_findings(),
                routing: StakeholderRoutingConfig::from_yaml(HR_STAKEHOLDERS).unwrap(),
            },
            &QuestionAudit {
                created_by: id("agent:compiler"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        assert!(generated.issues.is_empty(), "{:?}", generated.issues);
        let mut g = apply(&g0, generated.proposal.as_ref().unwrap());
        let answers = [
            (
                CARDINALITY_CANDIDATE,
                "domain_relationship_cardinality",
                json!({"cardinality_from": "0..*", "cardinality_to": "1"}),
                Some("Confirmed with HR."),
            ),
            (
                TRIGGER_CANDIDATE,
                "state_transition_trigger",
                json!({"trigger_ref": "operation:approve"}),
                Some("Approval triggers it."),
            ),
            (
                "operation:approve",
                "operation_performer",
                json!({"performer_ref": "actor:clerk"}),
                None,
            ),
            (
                "calculation:days",
                "calculation_calendar",
                json!({"calendar_ref": "calendar:nl"}),
                None,
            ),
            (
                "permission:approve",
                "permission_binding",
                json!({"operation_ref": "operation:approve", "resource_scope_ref": "scope:orders"}),
                None,
            ),
            (
                "inv:positive",
                "invariant_formula",
                json!({"expression": "amount >= 0"}),
                None,
            ),
        ];
        let context = ResolutionContext {
            invariant_bindings: BTreeMap::from([(
                id("inv:positive"),
                vec![ExpressionScopeBinding {
                    node_ref: id("attr:amount"),
                    symbol: "amount".into(),
                    exposure: ExpressionBindingExposure::Root,
                }],
            )]),
        };
        let mut artifacts = Vec::new();
        let mut decisions = BTreeMap::new();
        for (subject, kind, answer, rationale) in answers {
            let result = resolve_question(
                &g,
                &ResolutionInput {
                    question_ref: question_about(&g, subject),
                    answer,
                    decided_by: id(HUMAN),
                    decided_at: ts(DECIDED_AT),
                    rationale: rationale.map(Into::into),
                    supersedes: None,
                },
                &context,
            )
            .unwrap_or_else(|e| panic!("{kind}: {e}"));
            g = apply(&g, &result.proposal);
            artifacts.push(F3DecisionArtifactInput {
                decision_ref: result.decision_ref.clone(),
                hash: result.decision.patch_ref.clone(),
                bytes: result.patch_artifact_bytes.clone(),
            });
            decisions.insert(kind.to_owned(), result.decision_ref);
        }
        // The S2.1/S2.2 compiled results of the cardinality and trigger decisions, accepted.
        g = rebuild(&g, |nodes, edges| {
            nodes.push(node(
                CARDINALITY_CANDIDATE,
                Accepted,
                NodePayload::DomainRelationship(DomainRelationship {
                    from_entity: id("entity:order"),
                    to_entity: id("entity:customer"),
                    relationship_kind: "association".into(),
                    cardinality_from: "0..*".into(),
                    cardinality_to: "1".into(),
                    name: None,
                    snapshot_semantics: None,
                    ownership: None,
                }),
            ));
            nodes.push(node(
                TRIGGER_CANDIDATE,
                Accepted,
                NodePayload::Transition(Transition {
                    stateful_ref: id("entity:order"),
                    from_state: id("state:open"),
                    to_state: id("state:approved"),
                    guard_expr: None,
                    effect_refs: None,
                }),
            ));
            edges.push(edge(
                "rel:via-approve",
                RelationKind::TransitionsVia,
                TRIGGER_CANDIDATE,
                "operation:approve",
                Accepted,
            ));
        });
        // S3.4: a governed waiver of an F2 blocker finding.
        let wf = waived_finding();
        g = rebuild(&g, |nodes, _| {
            nodes.push(node(
                wf.id.as_str(),
                Accepted,
                NodePayload::Finding(wf.payload.clone()),
            ))
        });
        let metadata = ValidationRegistry::new(load_builtin_software_profile().unwrap()).unwrap();
        let rule = metadata.rule(WAIVED_RULE).unwrap().clone();
        let targets = wf.payload.affected_refs.clone();
        let human = id(HUMAN);
        let waiver = create_waiver(
            &g,
            &WaiverRequest {
                rule: &rule,
                targets: &targets,
                semantic_condition_key: "attribute-owner-condition",
                decided_by: &human,
                decided_at: ts(DECIDED_AT),
                rationale: "The attribute is owned by a legacy system.",
            },
            &ValidationPolicy::default(),
        )
        .unwrap();
        g = apply(&g, &waiver.proposal);
        artifacts.push(F3DecisionArtifactInput {
            decision_ref: waiver.decision_ref.clone(),
            hash: waiver.decision.patch_ref.clone(),
            bytes: waiver.artifact_bytes.clone(),
        });
        decisions.insert("waiver".into(), waiver.decision_ref);
        // S3.4: an Accepted assumption linked to a blocker finding, expiring in the future.
        let creation = create_assumption(
            &g,
            &AssumptionInput {
                statement: "Leave days are whole working days.".into(),
                owner_ref: id("stakeholder:hr"),
                finding_ref: Some(wf.id.clone()),
                default_value: None,
                expires_at: Some(ts("2026-12-31T00:00:00.000000000Z")),
                risk_ref: None,
            },
            &AssumptionAudit {
                created_by: id(HUMAN),
                created_at: ts(DECIDED_AT),
            },
        )
        .unwrap();
        let AssumptionCreation::New {
            assumption_ref,
            proposal,
            ..
        } = creation
        else {
            panic!()
        };
        g = apply(&g, &proposal);
        Governed {
            graph: g,
            artifacts,
            decisions,
            assumption: assumption_ref,
        }
    }

    fn expiry_at(graph: &Graph, now: &str) -> Vec<GeneratedFinding> {
        analyze_assumption_expiry(graph, &FixedClock::new(ts(now)))
            .unwrap()
            .findings
    }

    fn metadata() -> ValidationRegistry {
        ValidationRegistry::new(load_builtin_software_profile().unwrap()).unwrap()
    }

    fn registry() -> EvaluatorRegistry {
        let mut registry = EvaluatorRegistry::new(metadata());
        rules::register_f3_evaluators(&mut registry).unwrap();
        registry
    }

    fn base_context(graph: &Graph, policy: ValidationPolicy) -> ValidationContext {
        ValidationContext::new(
            graph,
            &metadata(),
            policy,
            graph.evidence_hash().unwrap(),
            vec![],
            vec![],
        )
        .unwrap()
    }

    fn evaluate_with(
        graph: &Graph,
        inputs: F3ValidationInputs,
        policy: ValidationPolicy,
    ) -> GateReport {
        let ctx = base_context(graph, policy)
            .with_f3_inputs(graph, inputs)
            .unwrap();
        registry().evaluate_gate(GateId::F3, graph, &ctx).unwrap()
    }

    fn evaluate(graph: &Graph, inputs: F3ValidationInputs) -> GateReport {
        evaluate_with(graph, inputs, ValidationPolicy::default())
    }

    fn inputs_of(g: &Governed) -> F3ValidationInputs {
        F3ValidationInputs::new(
            g.artifacts.clone(),
            expiry_at(&g.graph, "2026-06-01T00:00:00.000000000Z"),
        )
    }

    fn rule<'a>(report: &'a GateReport, rule_id: &str) -> &'a RuleResult {
        report.rules.iter().find(|r| r.rule_id == rule_id).unwrap()
    }

    fn state(report: &GateReport, rule_id: &str) -> RuleResultState {
        rule(report, rule_id).state
    }

    fn targets(report: &GateReport, rule_id: &str) -> Vec<Id> {
        rule(report, rule_id).targets.clone()
    }

    fn context_error(graph: &Graph, inputs: F3ValidationInputs) -> bool {
        matches!(
            base_context(graph, ValidationPolicy::default()).with_f3_inputs(graph, inputs),
            Err(EvaluationError::InvalidContext(_))
        )
    }

    // ------------------------------------------------------------------ full gate and HR

    #[test]
    fn governed_fixture_passes_the_f3_gate() {
        let g = governed();
        let report = evaluate(&g.graph, inputs_of(&g));
        for r in F3_RULES {
            assert_eq!(state(&report, r), PASS, "{r} {:?}", rule(&report, r));
        }
        assert_eq!(report.result, GateResult::Pass);
        assert_eq!(targets(&report, PATCH_APPLIED).len(), 7);
        assert_eq!(
            targets(&report, ASSUMPTION_NOT_EXPIRED),
            vec![g.assumption.clone()]
        );
    }

    /// The current-pipeline HR governance fixture: the real HR routing configuration routes the
    /// questions, the S3.1-S3.4 engines govern them. The legacy expert answers are not used.
    #[test]
    fn hr_governance_fixture_passes_and_fails_with_an_open_blocker() {
        for legacy in ["half_day", "holiday_inside_period", "inclusive_end"] {
            assert!(HR_EXPERT_ANSWERS.contains(legacy), "{legacy}");
        }
        let g = governed();
        assert!(g.graph.nodes().values().all(
            |n| !matches!(&n.payload, NodePayload::Question(q) if q.prompt.contains("half_day"))
        ));
        assert_eq!(evaluate(&g.graph, inputs_of(&g)).result, GateResult::Pass);
        // Reopen one blocker question: the gate fails.
        let q = question_about(&g.graph, "operation:approve");
        let reopened = edit_node(&g.graph, q.as_str(), |n| {
            if let NodePayload::Question(p) = &mut n.payload {
                p.status = "Open".into();
            }
        });
        let report = evaluate(&reopened, inputs_of(&g));
        assert_eq!(state(&report, NO_BLOCKING_OPEN), FAIL);
        assert_eq!(report.result, GateResult::Fail);
    }

    #[test]
    fn blocker_rule_failures_fail_the_gate_and_error_rules_need_promotion() {
        let g = governed();
        // ASSUMPTION.NOT_EXPIRED (blocker).
        let expired = F3ValidationInputs::new(
            g.artifacts.clone(),
            expiry_at(&g.graph, "2027-01-01T00:00:00.000000000Z"),
        );
        let report = evaluate(&g.graph, expired);
        assert_eq!(state(&report, ASSUMPTION_NOT_EXPIRED), FAIL);
        assert_eq!(report.result, GateResult::Fail);
        // DECISION.PATCH_APPLIED (blocker): the performer edge is gone.
        let unapplied = rebuild(&g.graph, |_, edges| {
            edges.retain(|e| e.kind != RelationKind::PerformedBy)
        });
        let report = evaluate(&unapplied, inputs_of(&g));
        assert_eq!(state(&report, PATCH_APPLIED), FAIL);
        assert_eq!(report.result, GateResult::Fail);
        // DECISION.EVIDENCE (error): a waiver with an unclean rationale fails the rule only.
        let w = g.decisions["waiver"].clone();
        let unclean = edit_node(&g.graph, w.as_str(), |n| {
            if let NodePayload::ResolutionDecision(d) = &mut n.payload {
                d.rationale = Some(" padded".into());
            }
        });
        let artifacts = g.artifacts.clone();
        let report = evaluate(&unclean, F3ValidationInputs::new(artifacts.clone(), vec![]));
        assert_eq!(state(&report, DECISION_EVIDENCE), FAIL);
        assert_eq!(state(&report, WAIVER_DECISION), FAIL);
        // ASSUMPTION.OWNER (error): owner made Suspect fails only that rule.
        let ownerless = edit_node(&g.graph, "stakeholder:hr", |n| n.status = Suspect);
        let report = evaluate(
            &ownerless,
            F3ValidationInputs::new(artifacts.clone(), vec![]),
        );
        assert_eq!(state(&report, ASSUMPTION_OWNER), FAIL);
        assert_eq!(report.result, GateResult::Pass);
        let promoted = ValidationPolicy {
            promoted_to_blocker_rule_ids: BTreeSet::from([ASSUMPTION_OWNER.to_owned()]),
            profile_allow_waiver_rule_ids: BTreeSet::new(),
        };
        assert_eq!(
            evaluate_with(
                &ownerless,
                F3ValidationInputs::new(artifacts, vec![]),
                promoted
            )
            .result,
            GateResult::Fail
        );
    }

    // ------------------------------------------------------------------ registration and inputs

    #[test]
    fn f3_registration_is_incremental() {
        let mut registry = EvaluatorRegistry::new(metadata());
        assert_eq!(registry.missing_for_gate(GateId::F3).len(), 6);
        rules::register_i0_evaluators(&mut registry).unwrap();
        rules::register_f1_evaluators(&mut registry).unwrap();
        rules::register_f2_evaluators(&mut registry).unwrap();
        for gate in [GateId::I0, GateId::F1, GateId::F2] {
            assert!(registry.missing_for_gate(gate).is_empty());
        }
        assert_eq!(registry.missing_for_gate(GateId::F3).len(), 6);
        rules::register_f3_evaluators(&mut registry).unwrap();
        assert!(registry.missing_for_gate(GateId::F3).is_empty());
        let gate: BTreeSet<&str> = registry
            .metadata()
            .gate(GateId::F3)
            .rule_ids
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(gate, F3_RULES.into_iter().collect());
        assert_eq!(
            registry.missing_for_gate(GateId::F4).len(),
            registry.metadata().gate(GateId::F4).rule_ids.len()
        );
        assert!(matches!(
            rules::register_f3_evaluators(&mut registry),
            Err(EvaluationError::DuplicateEvaluator(_))
        ));
    }

    #[test]
    fn missing_f3_inputs_are_errors_not_passes() {
        let g = governed();
        let report = registry()
            .evaluate_gate(
                GateId::F3,
                &g.graph,
                &base_context(&g.graph, ValidationPolicy::default()),
            )
            .unwrap();
        for r in F3_RULES {
            assert_eq!(state(&report, r), ERROR, "{r}");
            assert_eq!(
                rule(&report, r).error.as_ref().unwrap().code,
                "F3_INPUTS_MISSING"
            );
        }
        assert_eq!(report.result, GateResult::Fail);
    }

    #[test]
    fn f3_inputs_canonicalize_and_bind_to_the_context() {
        let g = governed();
        let forward = inputs_of(&g);
        let mut reversed_artifacts = g.artifacts.clone();
        reversed_artifacts.reverse();
        let reversed = F3ValidationInputs::new(reversed_artifacts, vec![]);
        assert_eq!(forward, reversed);
        assert_eq!(
            forward.content_hash().unwrap(),
            reversed.content_hash().unwrap()
        );
        assert!(forward
            .decision_artifacts
            .windows(2)
            .all(|w| w[0].decision_ref < w[1].decision_ref));
        let base = base_context(&g.graph, ValidationPolicy::default());
        let with = base
            .clone()
            .with_f3_inputs(&g.graph, forward.clone())
            .unwrap();
        assert_eq!(with.f3_inputs, Some(forward.clone()));
        assert_eq!(with.baseline_semantic_hash, base.baseline_semantic_hash);
        assert_eq!(with.baseline_evidence_hash, base.baseline_evidence_hash);
        assert_eq!(with.profile_id, base.profile_id);
        assert_eq!(with.policy, base.policy);
        assert_eq!(with.f1_inputs, base.f1_inputs);
        assert_eq!(with.f2_inputs, base.f2_inputs);
        assert_eq!(with.evidence_artifacts, base.evidence_artifacts);
        assert_eq!(
            with,
            base.clone().with_f3_inputs(&g.graph, reversed).unwrap()
        );
    }

    #[test]
    fn decision_artifact_inputs_are_validated() {
        let g = governed();
        let first = g.artifacts[0].clone();
        let with = |a: F3DecisionArtifactInput| {
            let mut artifacts = g.artifacts.clone();
            artifacts.retain(|x| x.decision_ref != a.decision_ref);
            artifacts.push(a);
            F3ValidationInputs::new(artifacts, vec![])
        };
        // Duplicate decision artifact.
        let mut dup = g.artifacts.clone();
        dup.push(first.clone());
        assert!(context_error(
            &g.graph,
            F3ValidationInputs::new(dup, vec![])
        ));
        // Missing, wrong type, non-Accepted decision.
        assert!(context_error(
            &g.graph,
            with(F3DecisionArtifactInput {
                decision_ref: id("dec:0000000000000000"),
                ..first.clone()
            })
        ));
        assert!(context_error(
            &g.graph,
            with(F3DecisionArtifactInput {
                decision_ref: id("operation:approve"),
                ..first.clone()
            })
        ));
        let proposed = edit_node(&g.graph, first.decision_ref.as_str(), |n| {
            n.status = Superseded
        });
        assert!(context_error(
            &proposed,
            F3ValidationInputs::new(g.artifacts.clone(), vec![])
        ));
        // Non-Generic hash, bytes mismatch, hash != patch_ref.
        let semantic = Hash::semantic_sha256(&first.bytes);
        assert!(context_error(
            &g.graph,
            with(F3DecisionArtifactInput {
                hash: semantic,
                ..first.clone()
            })
        ));
        let mut tampered = first.clone();
        tampered.bytes.push(b' ');
        assert!(context_error(&g.graph, with(tampered.clone())));
        tampered.hash = Hash::content_sha256(&tampered.bytes);
        assert!(context_error(&g.graph, with(tampered)));
        // A waiver artifact must match its marker.
        let w = g
            .artifacts
            .iter()
            .find(|a| a.decision_ref == g.decisions["waiver"])
            .unwrap()
            .clone();
        let wrong = br#"{"finding_key":"sha256:0000000000000000000000000000000000000000000000000000000000000000","finding_ref":"fnd:0000000000000000","rule_id":"X","version":1}"#.to_vec();
        let graph = edit_node(&g.graph, w.decision_ref.as_str(), |n| {
            if let NodePayload::ResolutionDecision(d) = &mut n.payload {
                d.patch_ref = Hash::content_sha256(&wrong);
            }
        });
        let mut artifacts = g.artifacts.clone();
        artifacts.retain(|x| x.decision_ref != w.decision_ref);
        artifacts.push(F3DecisionArtifactInput {
            decision_ref: w.decision_ref.clone(),
            hash: Hash::content_sha256(&wrong),
            bytes: wrong,
        });
        assert!(context_error(
            &graph,
            F3ValidationInputs::new(artifacts, vec![])
        ));
    }

    #[test]
    fn expiry_finding_inputs_are_validated() {
        let g = governed();
        let valid = expiry_at(&g.graph, "2027-01-01T00:00:00.000000000Z");
        assert_eq!(valid.len(), 1);
        assert!(!context_error(
            &g.graph,
            F3ValidationInputs::new(g.artifacts.clone(), valid.clone())
        ));
        let mutate = |f: fn(&mut GeneratedFinding)| {
            let mut m = valid[0].clone();
            f(&mut m);
            F3ValidationInputs::new(g.artifacts.clone(), vec![m])
        };
        for bad in [
            mutate(|f| f.payload.code = "PLUMB.S3.OTHER".into()),
            mutate(|f| f.payload.family = "F3".into()),
            mutate(|f| f.payload.severity = FindingSeverity::Error),
            mutate(|f| f.payload.status = "Closed".into()),
            mutate(|f| {
                f.payload
                    .affected_refs
                    .push("assumption:zzzzzzzzzzzzzzzz".parse().unwrap())
            }),
            mutate(|f| {
                f.payload.affected_refs = vec!["assumption:0000000000000000".parse().unwrap()]
            }),
            mutate(|f| f.payload.affected_refs = vec!["operation:approve".parse().unwrap()]),
            mutate(|f| {
                f.semantic_condition_key = f
                    .semantic_condition_key
                    .replace("assumption_expired", "assumption_late")
            }),
            mutate(|f| {
                f.semantic_condition_key =
                    f.semantic_condition_key.replace("2026-12-31", "2026-12-30")
            }),
            mutate(|f| f.key = Hash::content_sha256(b"wrong")),
            mutate(|f| f.id = "fnd:0000000000000000".parse().unwrap()),
        ] {
            assert!(context_error(&g.graph, bad));
        }
        // Inactive assumption.
        let resolved = edit_node(&g.graph, g.assumption.as_str(), |n| {
            if let NodePayload::Assumption(a) = &mut n.payload {
                a.status = "Resolved".into();
            }
        });
        assert!(context_error(
            &resolved,
            F3ValidationInputs::new(g.artifacts.clone(), valid.clone())
        ));
        let suspect = edit_node(&g.graph, g.assumption.as_str(), |n| n.status = Suspect);
        assert!(context_error(
            &suspect,
            F3ValidationInputs::new(g.artifacts.clone(), valid)
        ));
    }

    // ------------------------------------------------------------------ QUESTION.NO_BLOCKING_OPEN

    fn question_graph(questions: Vec<(&str, &str, &str)>) -> Graph {
        // (question id, finding id, status)
        let mut nodes = base_nodes();
        nodes.push(node(
            "fnd:00000000000000b1",
            Accepted,
            NodePayload::Finding(
                gf(
                    "PLUMB.F2.CALC.TYPECHECK",
                    "x",
                    &["calculation:days"],
                    FindingSeverity::Blocker,
                )
                .payload,
            ),
        ));
        nodes.push(node(
            "fnd:00000000000000e1",
            Accepted,
            NodePayload::Finding(
                gf(
                    "PLUMB.F2.CALC.TYPECHECK",
                    "y",
                    &["calculation:days"],
                    FindingSeverity::Error,
                )
                .payload,
            ),
        ));
        for (qid, fid, status) in questions {
            nodes.push(node(
                qid,
                Accepted,
                NodePayload::Question(Question {
                    finding_ref: id(fid),
                    question_kind: QuestionKind::Calendar,
                    prompt: "?".into(),
                    status: status.into(),
                    answer_schema: None,
                    stakeholder_ref: None,
                    priority: None,
                    round_ref: if qid.ends_with('2') {
                        Some(id("rnd:0000000000000001"))
                    } else {
                        None
                    },
                    context_refs: None,
                }),
            ));
        }
        graph_of(nodes, base_edges())
    }

    fn empty_inputs() -> F3ValidationInputs {
        F3ValidationInputs::new(vec![], vec![])
    }

    #[test]
    fn no_blocking_open_rule() {
        let r = |g: &Graph| evaluate(g, empty_inputs());
        let none = question_graph(vec![]);
        assert_eq!(
            (
                state(&r(&none), NO_BLOCKING_OPEN),
                targets(&r(&none), NO_BLOCKING_OPEN)
            ),
            (PASS, vec![])
        );
        let non_blocker =
            question_graph(vec![("q:00000000000000a1", "fnd:00000000000000e1", "Open")]);
        assert_eq!(state(&r(&non_blocker), NO_BLOCKING_OPEN), PASS);
        for status in ["Answered", "Closed", "Superseded"] {
            let g = question_graph(vec![
                ("q:00000000000000a1", "fnd:00000000000000b1", status),
                ("q:00000000000000a2", "fnd:00000000000000b1", status),
            ]);
            assert_eq!(state(&r(&g), NO_BLOCKING_OPEN), PASS, "{status}");
            assert_eq!(
                targets(&r(&g), NO_BLOCKING_OPEN),
                vec![id("q:00000000000000a1"), id("q:00000000000000a2")]
            );
        }
        let open = question_graph(vec![
            ("q:00000000000000a2", "fnd:00000000000000b1", "Open"),
            ("q:00000000000000a1", "fnd:00000000000000b1", "Open"),
            ("q:00000000000000a3", "fnd:00000000000000b1", "Answered"),
        ]);
        let report = r(&open);
        assert_eq!(state(&report, NO_BLOCKING_OPEN), FAIL);
        assert_eq!(
            targets(&report, NO_BLOCKING_OPEN),
            vec![id("q:00000000000000a1"), id("q:00000000000000a2")]
        );
        assert_eq!(
            rule(&report, NO_BLOCKING_OPEN)
                .semantic_condition_key
                .as_deref(),
            Some("blocking_functional_question_open")
        );
        // Unknown status and broken links are errors.
        let unknown = question_graph(vec![(
            "q:00000000000000a1",
            "fnd:00000000000000b1",
            "Pending",
        )]);
        assert_eq!(
            rule(&r(&unknown), NO_BLOCKING_OPEN)
                .error
                .as_ref()
                .unwrap()
                .code,
            "F3_QUESTION_STATUS"
        );
        let broken = question_graph(vec![("q:00000000000000a1", "operation:approve", "Open")]);
        assert_eq!(
            rule(&r(&broken), NO_BLOCKING_OPEN)
                .error
                .as_ref()
                .unwrap()
                .code,
            "F3_QUESTION_LINK"
        );
        // A missing or inactive Finding leaves the question outside the applicable set.
        let missing = question_graph(vec![("q:00000000000000a1", "fnd:00000000000000ff", "Open")]);
        assert_eq!(state(&r(&missing), NO_BLOCKING_OPEN), PASS);
        // Determinism.
        let reversed = rebuild(&open, |nodes, edges| {
            nodes.reverse();
            edges.reverse();
        });
        assert_eq!(
            rule(&r(&reversed), NO_BLOCKING_OPEN),
            rule(&r(&open), NO_BLOCKING_OPEN)
        );
    }

    #[test]
    fn f3_finding_identity_golden() {
        let g = question_graph(vec![("q:00000000000000f1", "fnd:00000000000000b1", "Open")]);
        let report = evaluate(&g, empty_inputs());
        let finding = report
            .findings
            .iter()
            .find(|f| f.payload.code == NO_BLOCKING_OPEN)
            .unwrap();
        assert_eq!(finding.key.as_str(), GOLDEN_F3_KEY);
        assert_eq!(finding.id.as_str(), GOLDEN_F3_FINDING);
    }

    #[test]
    fn f3_violation_can_be_waived_through_the_generic_mechanism() {
        let g = question_graph(vec![("q:00000000000000f1", "fnd:00000000000000b1", "Open")]);
        let report = evaluate(&g, empty_inputs());
        assert_eq!(state(&report, NO_BLOCKING_OPEN), FAIL);
        let finding = report
            .findings
            .iter()
            .find(|f| f.payload.code == NO_BLOCKING_OPEN)
            .unwrap()
            .clone();
        let g = rebuild(&g, |nodes, _| {
            nodes.push(node(
                finding.id.as_str(),
                Accepted,
                NodePayload::Finding(finding.payload.clone()),
            ))
        });
        let rule_meta = metadata().rule(NO_BLOCKING_OPEN).unwrap().clone();
        let targets_ = vec![id("q:00000000000000f1")];
        let human = id(HUMAN);
        let waiver = create_waiver(
            &g,
            &WaiverRequest {
                rule: &rule_meta,
                targets: &targets_,
                semantic_condition_key: "blocking_functional_question_open",
                decided_by: &human,
                decided_at: ts(DECIDED_AT),
                rationale: "Deferred to the next release.",
            },
            &ValidationPolicy::default(),
        )
        .unwrap();
        let waived = apply(&g, &waiver.proposal);
        let artifacts = vec![F3DecisionArtifactInput {
            decision_ref: waiver.decision_ref.clone(),
            hash: waiver.decision.patch_ref.clone(),
            bytes: waiver.artifact_bytes.clone(),
        }];
        let report = evaluate(&waived, F3ValidationInputs::new(artifacts, vec![]));
        assert_eq!(state(&report, NO_BLOCKING_OPEN), RuleResultState::Waived);
        assert_eq!(report.waivers[0].decision_ref, waiver.decision_ref);
        assert_eq!(state(&report, WAIVER_DECISION), PASS);
    }

    // ------------------------------------------------------------------ DECISION.PATCH_APPLIED

    #[test]
    fn patch_applied_per_family() {
        let g = governed();
        let inputs = inputs_of(&g);
        assert_eq!(
            state(&evaluate(&g.graph, inputs.clone()), PATCH_APPLIED),
            PASS
        );
        type Break = Box<dyn Fn(&Graph) -> Graph>;
        let breaks: Vec<(&str, Break)> = vec![
            (
                "domain_relationship_cardinality",
                Box::new(|g: &Graph| {
                    edit_node(g, CARDINALITY_CANDIDATE, |n| {
                        if let NodePayload::DomainRelationship(r) = &mut n.payload {
                            r.cardinality_to = "0..1".into();
                        }
                    })
                }),
            ),
            (
                "state_transition_trigger",
                Box::new(|g: &Graph| {
                    rebuild(g, |_, edges| {
                        edges
                            .iter_mut()
                            .find(|e| e.id == id("rel:via-approve"))
                            .unwrap()
                            .to = id("event:submitted");
                    })
                }),
            ),
            (
                "operation_performer",
                Box::new(|g: &Graph| {
                    rebuild(g, |_, edges| {
                        edges.retain(|e| e.kind != RelationKind::PerformedBy)
                    })
                }),
            ),
            (
                "calculation_calendar",
                Box::new(|g: &Graph| {
                    edit_node(g, "calculation:days", |n| {
                        if let NodePayload::Calculation(c) = &mut n.payload {
                            c.calendar_ref = None;
                        }
                    })
                }),
            ),
            (
                "permission_binding",
                Box::new(|g: &Graph| {
                    rebuild(g, |_, edges| {
                        edges
                            .iter_mut()
                            .find(|e| e.id == id("rel:permits"))
                            .unwrap()
                            .to = id("operation:submit");
                    })
                }),
            ),
            (
                "invariant_formula",
                Box::new(|g: &Graph| {
                    edit_node(g, "inv:positive", |n| {
                        if let NodePayload::Invariant(i) = &mut n.payload {
                            i.expression = "amount > 0".into();
                        }
                    })
                }),
            ),
            (
                "waiver",
                Box::new(|g: &Graph| {
                    rebuild(g, |_, edges| {
                        // Retarget the waiver's resolves edge away from its marker's Finding.
                        let w = edges.iter_mut().find(|e| e.kind == RelationKind::Resolves && g.node(&e.to).is_some_and(|n| matches!(&n.payload, NodePayload::Finding(f) if f.code == WAIVED_RULE))).unwrap();
                        w.to = question_findings()[2].id.clone();
                    })
                }),
            ),
        ];
        for (family, broken) in breaks {
            let report = evaluate(&broken(&g.graph), inputs.clone());
            assert_eq!(state(&report, PATCH_APPLIED), FAIL, "{family}");
            assert_eq!(
                targets(&report, PATCH_APPLIED),
                vec![g.decisions[family].clone()],
                "{family}"
            );
        }
        // A missing artifact fails; a permission scope change fails too.
        let mut without = g.artifacts.clone();
        without.retain(|a| a.decision_ref != g.decisions["calculation_calendar"]);
        assert_eq!(
            targets(
                &evaluate(&g.graph, F3ValidationInputs::new(without, vec![])),
                PATCH_APPLIED
            ),
            vec![g.decisions["calculation_calendar"].clone()]
        );
        let rescoped = rebuild(&g.graph, |nodes, edges| {
            nodes.push(scope("scope:hr"));
            edges
                .iter_mut()
                .find(|e| e.id == id("rel:scoped"))
                .unwrap()
                .to = id("scope:hr");
        });
        assert_eq!(
            state(&evaluate(&rescoped, inputs.clone()), PATCH_APPLIED),
            FAIL
        );
        // Superseded and non-S3 decisions are not applicable.
        let other = rebuild(&g.graph, |nodes, edges| {
            edges.push(edge(
                "rel:other-decision",
                RelationKind::Resolves,
                "dec:00000000000000c1",
                waived_finding().id.as_str(),
                Accepted,
            ));
            nodes.push(node(
                "dec:00000000000000c1",
                Accepted,
                NodePayload::ResolutionDecision(ResolutionDecision {
                    question_ref: Some(id("q:0000000000000001")),
                    proposal_ref: None,
                    answer: json!({"kind": "requirement_supersession"}),
                    decided_by: id(HUMAN),
                    decided_at: ts(AT),
                    patch_ref: Hash::content_sha256(b"other"),
                    rationale: None,
                    supersedes: None,
                }),
            ));
        });
        assert_eq!(
            state(&evaluate(&other, inputs.clone()), PATCH_APPLIED),
            PASS
        );
        let mut no_calendar = g.artifacts.clone();
        no_calendar.retain(|a| a.decision_ref != g.decisions["calculation_calendar"]);
        let retired = edit_node(
            &g.graph,
            g.decisions["calculation_calendar"].as_str(),
            |n| n.status = Superseded,
        );
        let report = evaluate(&retired, F3ValidationInputs::new(no_calendar, vec![]));
        assert_eq!(state(&report, PATCH_APPLIED), PASS);
        assert!(!targets(&report, PATCH_APPLIED).contains(&g.decisions["calculation_calendar"]));
    }

    // ------------------------------------------------------------------ DECISION.EVIDENCE

    fn edit_decision(g: &Governed, family: &str, f: impl FnOnce(&mut ResolutionDecision)) -> Graph {
        edit_node(&g.graph, g.decisions[family].as_str(), |n| {
            if let NodePayload::ResolutionDecision(d) = &mut n.payload {
                f(d);
            }
        })
    }

    #[test]
    fn decision_evidence_rule() {
        let g = governed();
        let inputs = inputs_of(&g);
        // Optional S3.3 rationale stays optional: the governed fixture passes with four None.
        let report = evaluate(&g.graph, inputs.clone());
        assert_eq!(state(&report, DECISION_EVIDENCE), PASS);
        assert_eq!(targets(&report, DECISION_EVIDENCE).len(), 7);
        let fails = |graph: Graph, family: &str| {
            let report = evaluate(&graph, inputs.clone());
            assert_eq!(state(&report, DECISION_EVIDENCE), FAIL, "{family}");
            assert!(
                targets(&report, DECISION_EVIDENCE).contains(&g.decisions[family]),
                "{family}"
            );
        };
        fails(
            edit_decision(&g, "operation_performer", |d| {
                d.decided_by = id("agent:ghost")
            }),
            "operation_performer",
        );
        fails(
            edit_decision(&g, "operation_performer", |d| {
                d.decided_by = id("agent:service")
            }),
            "operation_performer",
        );
        fails(
            edit_decision(&g, "operation_performer", |d| {
                d.decided_by = id("agent:intern")
            }),
            "operation_performer",
        );
        fails(
            edit_decision(&g, "domain_relationship_cardinality", |d| {
                d.rationale = None
            }),
            "domain_relationship_cardinality",
        );
        fails(
            edit_decision(&g, "state_transition_trigger", |d| d.rationale = None),
            "state_transition_trigger",
        );
        fails(
            edit_decision(&g, "calculation_calendar", |d| {
                d.rationale = Some("bad\trationale".into())
            }),
            "calculation_calendar",
        );
        fails(
            edit_decision(&g, "invariant_formula", |d| {
                d.supersedes = Some(id("dec:0000000000000001"))
            }),
            "invariant_formula",
        );
        fails(
            edit_decision(&g, "invariant_formula", |d| d.answer["extra"] = json!("x")),
            "invariant_formula",
        );
        // Missing governance target and missing artifact.
        let d = g.decisions["calculation_calendar"].clone();
        // Retarget its resolves edges to another Question's Finding.
        let elsewhere = question_findings()[2].id.clone();
        let untargeted = rebuild(&g.graph, |_, edges| {
            for e in edges
                .iter_mut()
                .filter(|e| e.kind == RelationKind::Resolves && e.from == d)
            {
                e.to = elsewhere.clone();
            }
            let mut seen = BTreeSet::new();
            edges.retain(|e| {
                !(e.kind == RelationKind::Resolves && e.from == d) || seen.insert(e.to.clone())
            });
        });
        fails(untargeted, "calculation_calendar");
        let mut without = g.artifacts.clone();
        without.retain(|a| a.decision_ref != d);
        let report = evaluate(&g.graph, F3ValidationInputs::new(without, vec![]));
        assert!(targets(&report, DECISION_EVIDENCE).contains(&d));
        // A superseding decision with a clean rationale passes.
        let superseding = edit_decision(&g, "invariant_formula", |d| {
            d.supersedes = Some(id("dec:0000000000000001"));
            d.rationale = Some("Policy changed.".into());
        });
        assert_eq!(
            state(&evaluate(&superseding, inputs.clone()), DECISION_EVIDENCE),
            PASS
        );
    }

    // ------------------------------------------------------------------ assumptions

    #[test]
    fn assumption_owner_rule() {
        let g = governed();
        let inputs = || F3ValidationInputs::new(g.artifacts.clone(), vec![]);
        assert_eq!(state(&evaluate(&g.graph, inputs()), ASSUMPTION_OWNER), PASS);
        let edit = |f: fn(&mut Assumption)| {
            edit_node(&g.graph, g.assumption.as_str(), |n| {
                if let NodePayload::Assumption(a) = &mut n.payload {
                    f(a);
                }
            })
        };
        // Agent owner passes.
        assert_eq!(
            state(
                &evaluate(
                    &edit(|a| a.owner_ref = "agent:human".parse().unwrap()),
                    inputs()
                ),
                ASSUMPTION_OWNER
            ),
            PASS
        );
        for bad in [
            edit(|a| a.owner_ref = "agent:ghost".parse().unwrap()),
            edit(|a| a.owner_ref = "operation:approve".parse().unwrap()),
            edit(|a| a.owner_ref = "agent:intern".parse().unwrap()),
            edit(|a| a.expires_at = None),
            edit(|a| a.risk_ref = Some("risk:any".parse().unwrap())),
        ] {
            let report = evaluate(&bad, inputs());
            assert_eq!(state(&report, ASSUMPTION_OWNER), FAIL);
            assert_eq!(
                targets(&report, ASSUMPTION_OWNER),
                vec![g.assumption.clone()]
            );
        }
        // Not linked to a blocker finding: not applicable.
        let unlinked = edit(|a| a.finding_ref = None);
        assert_eq!(state(&evaluate(&unlinked, inputs()), ASSUMPTION_OWNER), NA);
        assert_eq!(
            state(
                &evaluate(&question_graph(vec![]), empty_inputs()),
                ASSUMPTION_OWNER
            ),
            NA
        );
    }

    #[test]
    fn assumption_not_expired_rule() {
        let g = governed();
        assert_eq!(
            state(&evaluate(&g.graph, inputs_of(&g)), ASSUMPTION_NOT_EXPIRED),
            PASS
        );
        let expired = expiry_at(&g.graph, "2026-12-31T00:00:00.000000000Z");
        let report = evaluate(
            &g.graph,
            F3ValidationInputs::new(g.artifacts.clone(), expired.clone()),
        );
        assert_eq!(state(&report, ASSUMPTION_NOT_EXPIRED), FAIL);
        assert_eq!(
            targets(&report, ASSUMPTION_NOT_EXPIRED),
            vec![g.assumption.clone()]
        );
        // Expiry of an assumption outside the blocking scope is valid input but does not count.
        let second = create_assumption(
            &g.graph,
            &AssumptionInput {
                statement: "Unrelated assumption.".into(),
                owner_ref: id("stakeholder:hr"),
                finding_ref: None,
                default_value: None,
                expires_at: Some(ts("2026-02-01T00:00:00.000000000Z")),
                risk_ref: None,
            },
            &AssumptionAudit {
                created_by: id(HUMAN),
                created_at: ts(AT),
            },
        )
        .unwrap();
        let AssumptionCreation::New { proposal, .. } = second else {
            panic!()
        };
        let g2 = apply(&g.graph, &proposal);
        let outside = expiry_at(&g2, "2026-06-01T00:00:00.000000000Z");
        assert_eq!(outside.len(), 1);
        assert_eq!(
            state(
                &evaluate(&g2, F3ValidationInputs::new(g.artifacts.clone(), outside)),
                ASSUMPTION_NOT_EXPIRED
            ),
            PASS
        );
        let both = expiry_at(&g2, "2027-01-01T00:00:00.000000000Z");
        let report = evaluate(&g2, F3ValidationInputs::new(g.artifacts.clone(), both));
        assert_eq!(
            targets(&report, ASSUMPTION_NOT_EXPIRED),
            vec![g.assumption.clone()]
        );
        assert_eq!(
            state(
                &evaluate(&question_graph(vec![]), empty_inputs()),
                ASSUMPTION_NOT_EXPIRED
            ),
            NA
        );
    }

    // ------------------------------------------------------------------ WAIVER.DECISION

    #[test]
    fn waiver_decision_rule() {
        let g = governed();
        let inputs = inputs_of(&g);
        assert_eq!(
            state(&evaluate(&g.graph, inputs.clone()), WAIVER_DECISION),
            PASS
        );
        assert_eq!(
            state(
                &evaluate(&question_graph(vec![]), empty_inputs()),
                WAIVER_DECISION
            ),
            NA
        );
        let w = g.decisions["waiver"].clone();
        // Warn/info waivers are outside the rule.
        let wf = waived_finding().id;
        let warn = edit_node(&g.graph, wf.as_str(), |n| {
            if let NodePayload::Finding(f) = &mut n.payload {
                f.severity = FindingSeverity::Warn;
            }
        });
        assert_eq!(state(&evaluate(&warn, inputs.clone()), WAIVER_DECISION), NA);
        let error = edit_node(&g.graph, wf.as_str(), |n| {
            if let NodePayload::Finding(f) = &mut n.payload {
                f.severity = FindingSeverity::Error;
            }
        });
        assert_eq!(
            state(&evaluate(&error, inputs.clone()), WAIVER_DECISION),
            PASS
        );
        let fails = |graph: Graph, inputs: F3ValidationInputs| {
            let report = evaluate(&graph, inputs);
            assert_eq!(state(&report, WAIVER_DECISION), FAIL);
            assert!(targets(&report, WAIVER_DECISION).contains(&w));
        };
        fails(
            edit_decision(&g, "waiver", |d| d.decided_by = id("agent:service")),
            inputs.clone(),
        );
        fails(
            edit_decision(&g, "waiver", |d| d.decided_by = id("agent:intern")),
            inputs.clone(),
        );
        fails(
            edit_decision(&g, "waiver", |d| d.rationale = None),
            inputs.clone(),
        );
        fails(
            edit_decision(&g, "waiver", |d| d.rationale = Some("tab\there".into())),
            inputs.clone(),
        );
        let mut without = g.artifacts.clone();
        without.retain(|a| a.decision_ref != w);
        fails(g.graph.clone(), F3ValidationInputs::new(without, vec![]));
        // A second Accepted waiver of the same Finding makes both invalid.
        let duplicate = rebuild(&g.graph, |nodes, edges| {
            let original = nodes.iter().find(|n| n.id == w).unwrap().clone();
            let mut copy = original.clone();
            copy.id = id("dec:00000000000000d2");
            nodes.push(copy);
            edges.push(edge(
                "rel:dup-waiver",
                RelationKind::Resolves,
                "dec:00000000000000d2",
                wf.as_str(),
                Accepted,
            ));
        });
        let report = evaluate(&duplicate, inputs.clone());
        assert_eq!(state(&report, WAIVER_DECISION), FAIL);
        assert!(targets(&report, WAIVER_DECISION).contains(&w));
    }

    #[test]
    fn production_source_guard() {
        let source = include_str!("../src/rules/f3.rs");
        for token in [
            "std::fs",
            "File::open",
            "ArtifactStore",
            "SqliteArtifactStore",
            "SqliteRevisionStore",
            "rusqlite",
            "Clock",
            "SystemClock",
            "Utc::now",
            "Instant::now",
            "SystemTime::now",
            "OffsetDateTime::now",
            "reqwest",
            "InferenceProvider",
            "MockProvider",
            "SemanticPatch",
            "PatchSet",
            "Proposal",
            "apply_patch",
            "plumb_functional",
            "plumb_patch",
            "commit(",
            "branch_head",
            "unsafe",
            "WaiverPolicy",
        ] {
            assert!(!source.contains(token), "f3.rs contains {token}");
        }
        let mod_rs = include_str!("../src/rules/mod.rs");
        assert!(
            mod_rs.contains("mod f3;") && mod_rs.contains("pub use f3::register_f3_evaluators;")
        );
    }
}
