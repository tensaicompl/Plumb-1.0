//! S2.10 contract tests for the twenty-four F2 evaluators (Hotfix 044).
//!
//! Every test lives in `f2_contract` (`cargo test -p plumb-validation --test f2`). Graphs are
//! synthetic and hand-built; supplemental inputs are typed and validated by the context. Each
//! rule is exercised for every result class its frozen contract allows.

mod f2_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{GateId, Id, Timestamp};
    use plumb_psg::{
        Actor, ActorKind, Attribute, AuditMeta, BusinessRole, Calculation, Calendar, DataSchema,
        DecisionTable, DomainRelationship, Edge, ElementStatus, Entity, Event, Finding,
        FindingSeverity, Graph, Invariant, Node, NodePayload, Operation, OperationKind, Permission,
        Principal, Process, ProcessNode, ProcessNodeKind, RelationKind, RelationProperties,
        ResourceScope, SecurityRole, SeparationConstraint, SeparationConstraintKind, State,
        Transition,
    };
    use plumb_validation::calculation_analysis::{CalculationScope, CALCULATION_ORIGIN_EXTENSION};
    use plumb_validation::decision_table_analysis::{
        DecisionColumn, DecisionInputCell, DecisionOutputCell, DecisionRow, DecisionTableSpec,
        DecisionType,
    };
    use plumb_validation::expression_scope::{ExpressionBindingExposure, ExpressionScopeBinding};
    use plumb_validation::*;
    use serde_json::json;

    use ElementStatus::{Accepted, Proposed, Suspect};
    use ProcessNodeKind as K;

    const AT: &str = "2026-01-01T00:00:00.000000000Z";

    const ATTRIBUTE_OWNER: &str = "PLUMB.F2.DOMAIN.ATTRIBUTE_OWNER";
    const RELATION_TYPED: &str = "PLUMB.F2.DOMAIN.RELATION_TYPED";
    const TRANSITION_COMPLETE: &str = "PLUMB.F2.STATE.TRANSITION_COMPLETE";
    const REACHABILITY: &str = "PLUMB.F2.STATE.REACHABILITY";
    const PERFORMER: &str = "PLUMB.F2.OPERATION.PERFORMER";
    const IO_TYPED: &str = "PLUMB.F2.OPERATION.IO_TYPED";
    const CALC_TYPECHECK: &str = "PLUMB.F2.CALC.TYPECHECK";
    const CALC_NO_CYCLE: &str = "PLUMB.F2.CALC.NO_CYCLE";
    const CALENDAR_DEFINED: &str = "PLUMB.F2.TIME.CALENDAR_DEFINED";
    const TABLE_NO_OVERLAP: &str = "DMN.F2.TABLE.NO_OVERLAP";
    const TABLE_COVERAGE: &str = "DMN.F2.TABLE.COVERAGE";
    const PROCESS_START_END: &str = "BPMN.F2.PROCESS.START_END";
    const PROCESS_REACHABLE: &str = "BPMN.F2.PROCESS.REACHABLE";
    const TASK_RESOLVES: &str = "PLUMB.F2.PROCESS.TASK_RESOLVES";
    const GATEWAY_BALANCED: &str = "PLUMB.F2.PROCESS.GATEWAY_BALANCED";
    const PAYLOAD_TYPED: &str = "PLUMB.F2.EVENT.PAYLOAD_TYPED";
    const EVENT_PRODUCER: &str = "PLUMB.F2.EVENT.PRODUCER";
    const EVENT_CONSUMER: &str = "PLUMB.F2.EVENT.CONSUMER";
    const PERMISSION_CONCRETE: &str = "RBAC.F2.PERMISSION.CONCRETE";
    const HIERARCHY_ACYCLIC: &str = "RBAC.F2.ROLE.HIERARCHY_ACYCLIC";
    const ROLE_SEPARATION: &str = "PLUMB.F2.ROLE.SEPARATION";
    const SOD_NO_VIOLATION: &str = "PLUMB.F2.SOD.NO_VIOLATION";
    const INVARIANT_EXPRESSIBLE: &str = "PLUMB.F2.INVARIANT.EXPRESSIBLE";
    const NO_SEMANTIC_BLOCKER: &str = "PLUMB.F2.NO_UNRESOLVED_SEMANTIC_BLOCKER";

    const ALL_RULES: [&str; 24] = [
        ATTRIBUTE_OWNER,
        RELATION_TYPED,
        TRANSITION_COMPLETE,
        REACHABILITY,
        PERFORMER,
        IO_TYPED,
        CALC_TYPECHECK,
        CALC_NO_CYCLE,
        CALENDAR_DEFINED,
        TABLE_NO_OVERLAP,
        TABLE_COVERAGE,
        PROCESS_START_END,
        PROCESS_REACHABLE,
        TASK_RESOLVES,
        GATEWAY_BALANCED,
        PAYLOAD_TYPED,
        EVENT_PRODUCER,
        EVENT_CONSUMER,
        PERMISSION_CONCRETE,
        HIERARCHY_ACYCLIC,
        ROLE_SEPARATION,
        SOD_NO_VIOLATION,
        INVARIANT_EXPRESSIBLE,
        NO_SEMANTIC_BLOCKER,
    ];

    /// The rules that read F2ValidationInputs.
    const INPUT_RULES: [&str; 9] = [
        RELATION_TYPED,
        TRANSITION_COMPLETE,
        CALC_TYPECHECK,
        CALC_NO_CYCLE,
        CALENDAR_DEFINED,
        TABLE_NO_OVERLAP,
        TABLE_COVERAGE,
        INVARIANT_EXPRESSIBLE,
        NO_SEMANTIC_BLOCKER,
    ];

    use RuleResultState::{Error as ERROR, Fail as FAIL, NotApplicable as NA, Pass as PASS};

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn meta() -> AuditMeta {
        AuditMeta::new(
            id("actor:human"),
            AT.parse::<Timestamp>().unwrap(),
            None,
            None,
        )
        .unwrap()
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

    fn edge(kind: RelationKind, from: &str, to: &str, status: ElementStatus) -> Edge {
        Edge {
            id: id(&format!(
                "rel:{}-{}-{}",
                kind.as_str().replace('_', "-"),
                from.replace(':', "-"),
                to.replace(':', "-")
            )),
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

    fn attribute(node_id: &str, value_type: &str, unit: Option<&str>) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Attribute(Attribute {
                name: node_id.into(),
                value_type: value_type.into(),
                nullable: false,
                unit: unit.map(Into::into),
                precision: None,
                enum_values: None,
                data_classification: None,
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

    fn event(node_id: &str, payload: Option<&str>) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Event(Event {
                name: node_id.into(),
                payload_schema_ref: payload.map(id),
                semantic_type: None,
            }),
        )
    }

    fn schema(node_id: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            status,
            NodePayload::DataSchema(DataSchema {
                name: node_id.into(),
                schema_kind: "json_schema".into(),
                external_ref: None,
                inline_schema: None,
            }),
        )
    }

    fn security_role(node_id: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            status,
            NodePayload::SecurityRole(SecurityRole {
                name: node_id.into(),
                description: None,
            }),
        )
    }

    fn principal(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Principal(Principal {
                name: node_id.into(),
                principal_kind: "user".into(),
            }),
        )
    }

    fn binding(node_ref: &str, symbol: &str) -> ExpressionScopeBinding {
        ExpressionScopeBinding {
            node_ref: id(node_ref),
            symbol: symbol.into(),
            exposure: ExpressionBindingExposure::Root,
        }
    }

    /// An Accepted Calculation; `origin` gives it an S2.6 origin with those used bindings.
    fn calculation(
        node_id: &str,
        expression: &str,
        result_type: &str,
        unit: Option<&str>,
        calendar: Option<&str>,
        origin: Option<Vec<ExpressionScopeBinding>>,
    ) -> Node {
        let mut n = node(
            node_id,
            Accepted,
            NodePayload::Calculation(Calculation {
                name: node_id.into(),
                expression: expression.into(),
                result_type: result_type.into(),
                unit: unit.map(Into::into),
                rounding: None,
                calendar_ref: calendar.map(id),
                examples: None,
            }),
        );
        if let Some(bindings) = origin {
            let range = json!({"requirement_ref": "req:r1", "start": 0, "end": 1});
            n.extensions.insert(
                CALCULATION_ORIGIN_EXTENSION.parse().unwrap(),
                json!({"name_range": range, "expression_evidence": [range],
                       "used_bindings": serde_json::to_value(bindings).unwrap()}),
            );
        }
        n
    }

    fn decision_table(node_id: &str, hit_policy: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::DecisionTable(DecisionTable {
                name: node_id.into(),
                hit_policy: hit_policy.into(),
                inputs: vec![],
                outputs: vec![],
                rows: vec![],
            }),
        )
    }

    fn bool_column(name: &str) -> DecisionColumn {
        DecisionColumn {
            name: name.into(),
            ty: DecisionType::Bool,
            domain: None,
        }
    }

    fn row(input: DecisionInputCell, output: bool) -> DecisionRow {
        DecisionRow {
            inputs: vec![input],
            outputs: vec![DecisionOutputCell::Bool { value: output }],
        }
    }

    fn flag(value: bool) -> DecisionInputCell {
        DecisionInputCell::Bool { value }
    }

    fn spec(table_ref: &str, hit_policy: &str, rows: Vec<DecisionRow>) -> F2DecisionTableInput {
        F2DecisionTableInput {
            decision_table_ref: id(table_ref),
            spec: DecisionTableSpec {
                hit_policy: hit_policy.into(),
                inputs: vec![bool_column("flag")],
                outputs: vec![bool_column("out")],
                rows,
                default_output: None,
            },
        }
    }

    fn complete_spec(table_ref: &str) -> F2DecisionTableInput {
        spec(
            table_ref,
            "UNIQUE",
            vec![row(flag(true), true), row(flag(false), false)],
        )
    }

    fn finding(
        code: &str,
        condition: &str,
        targets: &[&str],
        severity: FindingSeverity,
    ) -> GeneratedFinding {
        let affected: Vec<Id> = targets.iter().map(|t| id(t)).collect();
        let key = finding_key(code, &affected, condition).unwrap();
        GeneratedFinding {
            id: finding_id(&key).unwrap(),
            key,
            semantic_condition_key: condition.into(),
            payload: Finding {
                code: code.into(),
                family: "F2".into(),
                severity,
                message: "unresolved".into(),
                status: "Open".into(),
                affected_refs: affected,
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            },
        }
    }

    /// A synthetic F2 fixture: graph elements plus supplemental inputs.
    #[derive(Clone)]
    struct Fx {
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        findings: Vec<GeneratedFinding>,
        overrides: Vec<F2CalculationScopeOverride>,
        tables: Vec<F2DecisionTableInput>,
        invariants: Vec<F2InvariantInput>,
    }

    impl Fx {
        /// A clean baseline: every graph-shaped rule passes or is not applicable.
        fn clean() -> Fx {
            Fx {
                nodes: vec![
                    entity("entity:order"),
                    entity("entity:customer"),
                    attribute("attr:amount", "Decimal(2)", None),
                    attribute("attr:text", "Text", None),
                    attribute("attr:start", "Date", None),
                    attribute("attr:end", "Date", None),
                    node(
                        "domrel:order-customer",
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
                    ),
                    operation("operation:submit", Accepted),
                    operation("operation:approve", Accepted),
                    node(
                        "brole:clerk",
                        Accepted,
                        NodePayload::BusinessRole(BusinessRole {
                            name: "Clerk".into(),
                        }),
                    ),
                    node(
                        "actor:bot",
                        Accepted,
                        NodePayload::Actor(Actor {
                            name: "Bot".into(),
                            actor_kind: ActorKind::System,
                        }),
                    ),
                    event("event:submitted", None),
                    calculation(
                        "calculation:total",
                        "amount * 2",
                        "Decimal(2)",
                        None,
                        None,
                        Some(vec![binding("attr:amount", "amount")]),
                    ),
                    decision_table("dt:discount", "UNIQUE"),
                    node(
                        "permission:approve",
                        Accepted,
                        NodePayload::Permission(Permission {
                            name: "Approve".into(),
                        }),
                    ),
                    node(
                        "scope:orders",
                        Accepted,
                        NodePayload::ResourceScope(ResourceScope {
                            resource_ref: id("entity:order"),
                            scope_kind: "entity".into(),
                        }),
                    ),
                    security_role("secrole:approver", Accepted),
                    security_role("secrole:requester", Accepted),
                    principal("principal:alice"),
                    node(
                        "inv:positive",
                        Accepted,
                        NodePayload::Invariant(Invariant {
                            scope_ref: id("entity:order"),
                            expression: "amount >= 0".into(),
                        }),
                    ),
                ],
                edges: vec![],
                findings: vec![],
                overrides: vec![],
                tables: vec![complete_spec("dt:discount")],
                invariants: vec![F2InvariantInput {
                    invariant_ref: id("inv:positive"),
                    bindings: vec![binding("attr:amount", "amount")],
                }],
            }
            .rel(RelationKind::HasAttribute, "entity:order", "attr:amount")
            .rel(RelationKind::HasAttribute, "entity:order", "attr:text")
            .rel(RelationKind::HasAttribute, "entity:order", "attr:start")
            .rel(RelationKind::HasAttribute, "entity:order", "attr:end")
            .rel(RelationKind::PerformedBy, "operation:submit", "brole:clerk")
            .rel(RelationKind::PerformedBy, "operation:approve", "actor:bot")
            .rel(RelationKind::Reads, "operation:submit", "entity:order")
            .rel(RelationKind::Writes, "operation:submit", "attr:amount")
            .rel(
                RelationKind::Produces,
                "operation:submit",
                "event:submitted",
            )
            .rel(
                RelationKind::Consumes,
                "operation:approve",
                "event:submitted",
            )
            .rel(
                RelationKind::Permits,
                "permission:approve",
                "operation:approve",
            )
            .rel(RelationKind::ScopedTo, "permission:approve", "scope:orders")
            .rel(
                RelationKind::AssignedRole,
                "principal:alice",
                "secrole:approver",
            )
        }

        fn with(mut self, n: Node) -> Fx {
            self.nodes.push(n);
            self
        }

        fn without(mut self, node_id: &str) -> Fx {
            let target = id(node_id);
            self.nodes.retain(|n| n.id != target);
            self.edges.retain(|e| e.from != target && e.to != target);
            self
        }

        fn rel(self, kind: RelationKind, from: &str, to: &str) -> Fx {
            self.rel_as(kind, from, to, Accepted)
        }

        fn rel_as(mut self, kind: RelationKind, from: &str, to: &str, status: ElementStatus) -> Fx {
            self.edges.push(edge(kind, from, to, status));
            self
        }

        fn unrel(mut self, kind: RelationKind, from: &str, to: &str) -> Fx {
            self.edges
                .retain(|e| !(e.kind == kind && e.from == id(from) && e.to == id(to)));
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

        fn table(mut self, input: F2DecisionTableInput) -> Fx {
            self.tables.push(input);
            self
        }

        fn tables(mut self, tables: Vec<F2DecisionTableInput>) -> Fx {
            self.tables = tables;
            self
        }

        fn finding(mut self, f: GeneratedFinding) -> Fx {
            self.findings.push(f);
            self
        }

        fn scope_override(mut self, calculation_ref: &str, scope: CalculationScope) -> Fx {
            self.overrides.push(F2CalculationScopeOverride {
                calculation_ref: id(calculation_ref),
                scope,
            });
            self
        }

        fn invariant(
            mut self,
            node_id: &str,
            expression: &str,
            bindings: Vec<ExpressionScopeBinding>,
        ) -> Fx {
            self.nodes.push(node(
                node_id,
                Accepted,
                NodePayload::Invariant(Invariant {
                    scope_ref: id("entity:order"),
                    expression: expression.into(),
                }),
            ));
            self.invariants.push(F2InvariantInput {
                invariant_ref: id(node_id),
                bindings,
            });
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

        fn inputs(&self) -> F2ValidationInputs {
            F2ValidationInputs::new(
                self.findings.clone(),
                self.overrides.clone(),
                self.tables.clone(),
                self.invariants.clone(),
            )
        }

        fn report(&self) -> GateReport {
            let g = self.graph();
            let ctx = base_context(&g).with_f2_inputs(&g, self.inputs()).unwrap();
            registry().evaluate_gate(GateId::F2, &g, &ctx).unwrap()
        }

        fn state(&self, rule_id: &str) -> RuleResultState {
            rule(&self.report(), rule_id).state
        }
    }

    fn metadata() -> ValidationRegistry {
        ValidationRegistry::new(load_builtin_software_profile().unwrap()).unwrap()
    }

    fn registry() -> EvaluatorRegistry {
        let mut registry = EvaluatorRegistry::new(metadata());
        rules::register_f2_evaluators(&mut registry).unwrap();
        registry
    }

    fn base_context(graph: &Graph) -> ValidationContext {
        ValidationContext::new(
            graph,
            &metadata(),
            ValidationPolicy::default(),
            graph.evidence_hash().unwrap(),
            vec![],
            vec![],
        )
        .unwrap()
    }

    fn rule<'a>(report: &'a GateReport, rule_id: &str) -> &'a RuleResult {
        report
            .rules
            .iter()
            .find(|r| r.rule_id == rule_id)
            .unwrap_or_else(|| panic!("{rule_id} missing"))
    }

    fn error_code(fx: &Fx, rule_id: &str) -> String {
        let report = fx.report();
        let result = rule(&report, rule_id);
        assert_eq!(result.state, ERROR, "{rule_id}");
        result.error.as_ref().unwrap().code.clone()
    }

    fn targets(fx: &Fx, rule_id: &str) -> Vec<String> {
        rule(&fx.report(), rule_id)
            .targets
            .iter()
            .map(Id::to_string)
            .collect()
    }

    // ------------------------------------------------------------------ registration and inputs

    #[test]
    fn f2_registration_is_complete() {
        let mut registry = EvaluatorRegistry::new(metadata());
        assert_eq!(registry.missing_for_gate(GateId::F2).len(), 24);
        rules::register_f2_evaluators(&mut registry).unwrap();
        assert!(registry.missing_for_gate(GateId::F2).is_empty());
        let gate: BTreeSet<&str> = registry
            .metadata()
            .gate(GateId::F2)
            .rule_ids
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(gate, ALL_RULES.into_iter().collect());
        assert!(ALL_RULES.iter().all(|r| registry.has_evaluator(r)));
        for gate in [GateId::I0, GateId::F1, GateId::F3, GateId::F4] {
            assert_eq!(
                registry.missing_for_gate(gate).len(),
                registry.metadata().gate(gate).rule_ids.len()
            );
        }
        assert!(matches!(
            rules::register_f2_evaluators(&mut registry),
            Err(EvaluationError::DuplicateEvaluator(_))
        ));
        let report = Fx::clean().report();
        assert_eq!(report.rules.len(), 24);
    }

    #[test]
    fn f2_inputs_missing_gate_cannot_pass() {
        let fx = Fx::clean();
        let g = fx.graph();
        let report = registry()
            .evaluate_gate(GateId::F2, &g, &base_context(&g))
            .unwrap();
        assert_eq!(report.result, GateResult::Fail);
        for rule_id in INPUT_RULES {
            let r = rule(&report, rule_id);
            assert_eq!(r.state, ERROR, "{rule_id}");
            assert_eq!(r.error.as_ref().unwrap().code, "F2_INPUTS_MISSING");
        }
        // Graph-only rules still evaluate independently.
        assert_eq!(rule(&report, ATTRIBUTE_OWNER).state, PASS);
        assert_eq!(rule(&report, PERFORMER).state, PASS);
        assert_eq!(rule(&report, REACHABILITY).state, NA);
        // With inputs, the clean fixture passes every rule that applies.
        let clean = fx.report();
        assert_eq!(clean.result, GateResult::Pass);
        for r in &clean.rules {
            assert!(matches!(r.state, PASS | NA), "{} {:?}", r.rule_id, r.state);
        }
    }

    #[test]
    fn f2_stale_inputs_are_context_errors() {
        let fx = Fx::clean().tables(vec![]);
        let g = fx.graph();
        assert!(matches!(
            base_context(&g).with_f2_inputs(&g, fx.inputs()),
            Err(EvaluationError::InvalidContext(_))
        ));
    }

    // ------------------------------------------------------------------ domain and state

    #[test]
    fn f2_attribute_owner_pass_fail() {
        assert_eq!(Fx::clean().state(ATTRIBUTE_OWNER), PASS);
        // The only owner edge is Suspect: the Accepted Attribute has no Accepted owner.
        let fx = Fx::clean()
            .with(attribute("attr:loose", "Int", None))
            .rel_as(
                RelationKind::HasAttribute,
                "entity:order",
                "attr:loose",
                Suspect,
            );
        assert_eq!(fx.state(ATTRIBUTE_OWNER), FAIL);
        assert_eq!(targets(&fx, ATTRIBUTE_OWNER), ["attr:loose"]);
        // A Suspect owning Entity does not count either.
        let fx = Fx::clean()
            .with(node(
                "entity:old",
                Suspect,
                NodePayload::Entity(Entity {
                    name: "Old".into(),
                    description: None,
                    aggregate_root: None,
                }),
            ))
            .with(attribute("attr:legacy", "Int", None))
            .rel(RelationKind::HasAttribute, "entity:old", "attr:legacy");
        assert_eq!(fx.state(ATTRIBUTE_OWNER), FAIL);
        // No Accepted Attribute: PASS with zero targets.
        let mut empty = Fx::clean()
            .without("attr:amount")
            .without("attr:text")
            .without("attr:start")
            .without("attr:end")
            .without("calculation:total")
            .without("inv:positive");
        empty.invariants.clear();
        let report = empty.report();
        assert_eq!(rule(&report, ATTRIBUTE_OWNER).state, PASS);
        assert!(rule(&report, ATTRIBUTE_OWNER).targets.is_empty());
    }

    fn relationship(node_id: &str, to: &str, from_card: &str, to_card: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::DomainRelationship(DomainRelationship {
                from_entity: id("entity:order"),
                to_entity: id(to),
                relationship_kind: "association".into(),
                cardinality_from: from_card.into(),
                cardinality_to: to_card.into(),
                name: None,
                snapshot_semantics: None,
                ownership: None,
            }),
        )
    }

    #[test]
    fn f2_domain_relation_typed() {
        assert_eq!(Fx::clean().state(RELATION_TYPED), PASS);
        for card in ["0..1", "1", "0..*", "1..*"] {
            let fx = Fx::clean().with(relationship("domrel:x", "entity:customer", card, card));
            assert_eq!(fx.state(RELATION_TYPED), PASS, "{card}");
        }
        let bad_card = Fx::clean().with(relationship("domrel:x", "entity:customer", "many", "1"));
        assert_eq!(bad_card.state(RELATION_TYPED), FAIL);
        assert_eq!(targets(&bad_card, RELATION_TYPED), ["domrel:x"]);
        let bad_endpoint = Fx::clean().with(relationship("domrel:x", "attr:amount", "1", "1"));
        assert_eq!(bad_endpoint.state(RELATION_TYPED), FAIL);
        let dangling = Fx::clean().with(relationship("domrel:x", "entity:ghost", "1", "1"));
        assert_eq!(dangling.state(RELATION_TYPED), FAIL);
    }

    #[test]
    fn f2_domain_relation_unresolved_cardinality_finding() {
        let fx = Fx::clean().finding(finding(
            RELATION_TYPED,
            "domain_relationship_cardinality_unresolved:candidate-1",
            &["entity:order"],
            FindingSeverity::Error,
        ));
        let report = fx.report();
        let r = rule(&report, RELATION_TYPED);
        assert_eq!(r.state, FAIL);
        assert_eq!(r.targets, [id("entity:order")]);
        assert_eq!(r.evidence, [fx.findings[0].id.to_string()]);
    }

    /// An Entity state machine with one transition triggered by `trigger`.
    fn lifecycle(trigger: Option<&str>) -> Fx {
        let state = |nid: &str| {
            node(
                nid,
                Accepted,
                NodePayload::State(State { name: nid.into() }),
            )
        };
        let fx = Fx::clean()
            .with(state("state:open"))
            .with(state("state:closed"))
            .with(node(
                "transition:close",
                Accepted,
                NodePayload::Transition(Transition {
                    stateful_ref: id("entity:order"),
                    from_state: id("state:open"),
                    to_state: id("state:closed"),
                    guard_expr: None,
                    effect_refs: None,
                }),
            ))
            .rel(RelationKind::HasState, "entity:order", "state:open")
            .rel(RelationKind::HasState, "entity:order", "state:closed");
        match trigger {
            Some(t) => fx.rel(RelationKind::TransitionsVia, "transition:close", t),
            None => fx,
        }
    }

    #[test]
    fn f2_transition_complete() {
        assert_eq!(
            lifecycle(Some("operation:approve")).state(TRANSITION_COMPLETE),
            PASS
        );
        assert_eq!(
            lifecycle(Some("event:submitted")).state(TRANSITION_COMPLETE),
            PASS
        );
        // A Suspect trigger is not an Accepted Operation/Event.
        let fx = lifecycle(Some("operation:approve")).status("operation:approve", Suspect);
        assert_eq!(fx.state(TRANSITION_COMPLETE), FAIL);
        assert_eq!(targets(&fx, TRANSITION_COMPLETE), ["transition:close"]);
        // A non-Accepted state endpoint.
        let fx = lifecycle(Some("operation:approve")).status("state:closed", Suspect);
        assert_eq!(fx.state(TRANSITION_COMPLETE), FAIL);
    }

    #[test]
    fn f2_transition_missing_trigger_finding() {
        let fx = lifecycle(Some("operation:approve")).finding(finding(
            TRANSITION_COMPLETE,
            "state_transition_trigger_unresolved:candidate-2",
            &["entity:customer"],
            FindingSeverity::Blocker,
        ));
        assert_eq!(fx.state(TRANSITION_COMPLETE), FAIL);
        assert_eq!(targets(&fx, TRANSITION_COMPLETE), ["entity:customer"]);
    }

    #[test]
    fn f2_state_reachability_representability() {
        assert_eq!(Fx::clean().state(REACHABILITY), NA);
        let fx = lifecycle(Some("operation:approve"));
        assert_eq!(
            error_code(&fx, REACHABILITY),
            "STATE_REACHABILITY_UNREPRESENTABLE"
        );
        assert_eq!(targets(&fx, REACHABILITY), ["state:closed", "state:open"]);
    }

    // ------------------------------------------------------------------ operations

    #[test]
    fn f2_operation_performer_pass_fail() {
        assert_eq!(Fx::clean().state(PERFORMER), PASS);
        let fx = Fx::clean().unrel(RelationKind::PerformedBy, "operation:approve", "actor:bot");
        assert_eq!(fx.state(PERFORMER), FAIL);
        assert_eq!(targets(&fx, PERFORMER), ["operation:approve"]);
        // A Suspect performer does not count.
        let fx = Fx::clean().status("actor:bot", Suspect);
        assert_eq!(fx.state(PERFORMER), FAIL);
    }

    #[test]
    fn f2_operation_io_pass_fail() {
        let with_schema = |status| {
            let mut fx = Fx::clean().with(schema("schema:req", status));
            for n in &mut fx.nodes {
                if let NodePayload::Operation(op) = &mut n.payload {
                    if n.id == id("operation:submit") {
                        op.input_schema_ref = Some(id("schema:req"));
                    }
                }
            }
            fx
        };
        assert_eq!(Fx::clean().state(IO_TYPED), PASS);
        assert_eq!(with_schema(Accepted).state(IO_TYPED), PASS);
        let fx = with_schema(Proposed);
        assert_eq!(fx.state(IO_TYPED), FAIL);
        assert_eq!(targets(&fx, IO_TYPED), ["operation:submit"]);
        // A reads edge to a Suspect Entity.
        let fx = Fx::clean().status("entity:customer", Suspect).rel(
            RelationKind::Reads,
            "operation:approve",
            "entity:customer",
        );
        let fx = Fx {
            nodes: fx
                .nodes
                .into_iter()
                .filter(|n| n.id != id("domrel:order-customer"))
                .collect(),
            ..fx
        };
        assert_eq!(fx.state(IO_TYPED), FAIL);
        assert_eq!(targets(&fx, IO_TYPED), ["operation:approve"]);
    }

    // ------------------------------------------------------------------ calculations

    #[test]
    fn f2_calculation_qualified_and_type_failure() {
        assert_eq!(Fx::clean().state(CALC_TYPECHECK), PASS);
        assert_eq!(Fx::clean().state(CALC_NO_CYCLE), PASS);
        let fx = Fx::clean().with(calculation(
            "calculation:bad",
            "amount + true",
            "Decimal(2)",
            None,
            None,
            Some(vec![binding("attr:amount", "amount")]),
        ));
        assert_eq!(fx.state(CALC_TYPECHECK), FAIL);
        assert_eq!(targets(&fx, CALC_TYPECHECK), ["calculation:bad"]);
        // Result type mismatch and undefined input also fail.
        let fx = Fx::clean().with(calculation(
            "calculation:text",
            "amount",
            "Bool",
            None,
            None,
            Some(vec![binding("attr:amount", "amount")]),
        ));
        assert_eq!(fx.state(CALC_TYPECHECK), FAIL);
        let fx = Fx::clean().with(calculation(
            "calculation:undefined",
            "missing * 2",
            "Decimal(2)",
            None,
            None,
            Some(vec![]),
        ));
        assert_eq!(fx.state(CALC_TYPECHECK), FAIL);
        // A stale origin binding fails rather than being repaired.
        let fx = Fx::clean().with(calculation(
            "calculation:stale",
            "gone * 2",
            "Decimal(2)",
            None,
            None,
            Some(vec![binding("attr:gone", "gone")]),
        ));
        assert_eq!(fx.state(CALC_TYPECHECK), FAIL);
    }

    #[test]
    fn f2_calculation_cycle() {
        let fx = Fx::clean()
            .with(calculation(
                "calculation:a",
                "b + 1",
                "Int",
                None,
                None,
                Some(vec![binding("calculation:b", "b")]),
            ))
            .with(calculation(
                "calculation:b",
                "a + 1",
                "Int",
                None,
                None,
                Some(vec![binding("calculation:a", "a")]),
            ))
            .with(calculation(
                "calculation:self",
                "me + 1",
                "Int",
                None,
                None,
                Some(vec![binding("calculation:self", "me")]),
            ));
        assert_eq!(fx.state(CALC_NO_CYCLE), FAIL);
        assert_eq!(
            targets(&fx, CALC_NO_CYCLE),
            ["calculation:a", "calculation:b", "calculation:self"]
        );
        // The cycle members still type-check individually.
        assert_eq!(fx.state(CALC_TYPECHECK), PASS);
    }

    #[test]
    fn f2_calculation_legacy_override() {
        let legacy = Fx::clean().with(calculation(
            "calculation:legacy",
            "amount * 3",
            "Decimal(2)",
            None,
            None,
            None,
        ));
        let fx = legacy.clone().scope_override(
            "calculation:legacy",
            CalculationScope {
                bindings: vec![binding("attr:amount", "amount")],
                calendar_refs: vec![],
            },
        );
        assert_eq!(fx.state(CALC_TYPECHECK), PASS);
        assert_eq!(fx.state(CALC_NO_CYCLE), PASS);
        // An empty explicit scope is honoured, not widened.
        let fx = legacy
            .clone()
            .scope_override("calculation:legacy", CalculationScope::default());
        assert_eq!(fx.state(CALC_TYPECHECK), FAIL);
        // Without its override the context is invalid.
        let g = legacy.graph();
        assert!(matches!(
            base_context(&g).with_f2_inputs(&g, legacy.inputs()),
            Err(EvaluationError::InvalidContext(_))
        ));
    }

    fn calendar_node(node_id: &str, time_zone: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Calendar(Calendar {
                time_zone: time_zone.into(),
                week_pattern: json!(["Mon", "Tue", "Wed", "Thu", "Fri"]),
                region: None,
                holiday_source: None,
            }),
        )
    }

    fn working_days(calendar: Option<&str>) -> Fx {
        Fx::clean().with(calculation(
            "calculation:days",
            "working_days([start, end], calendar, true)",
            "Int",
            Some("working_day"),
            calendar,
            Some(vec![
                binding("attr:end", "end"),
                binding("attr:start", "start"),
            ]),
        ))
    }

    #[test]
    fn f2_calendar_defined_and_missing() {
        assert_eq!(Fx::clean().state(CALENDAR_DEFINED), NA);
        let defined = working_days(Some("calendar:nl"))
            .with(calendar_node("calendar:nl", "Europe/Amsterdam"));
        assert_eq!(defined.state(CALENDAR_DEFINED), PASS);
        assert_eq!(defined.state(CALC_TYPECHECK), PASS);
        let missing = working_days(None);
        assert_eq!(missing.state(CALENDAR_DEFINED), FAIL);
        assert_eq!(targets(&missing, CALENDAR_DEFINED), ["calculation:days"]);
        let unresolved = working_days(Some("calendar:ghost"));
        assert_eq!(unresolved.state(CALENDAR_DEFINED), FAIL);
        let blank_zone = working_days(Some("calendar:nl")).with(calendar_node("calendar:nl", " "));
        assert_eq!(blank_zone.state(CALENDAR_DEFINED), FAIL);
    }

    // ------------------------------------------------------------------ decision tables

    #[test]
    fn f2_decision_table_unique_clean_and_overlap() {
        assert_eq!(Fx::clean().state(TABLE_NO_OVERLAP), PASS);
        let overlap = Fx::clean().tables(vec![spec(
            "dt:discount",
            "UNIQUE",
            vec![row(DecisionInputCell::Any, true), row(flag(false), false)],
        )]);
        assert_eq!(overlap.state(TABLE_NO_OVERLAP), FAIL);
        assert_eq!(targets(&overlap, TABLE_NO_OVERLAP), ["dt:discount"]);
    }

    #[test]
    fn f2_decision_table_first_overlap_not_applicable() {
        let fx = Fx::clean()
            .without("dt:discount")
            .with(decision_table("dt:priority", "FIRST"))
            .tables(vec![spec(
                "dt:priority",
                "FIRST",
                vec![row(flag(true), true), row(DecisionInputCell::Any, false)],
            )]);
        assert_eq!(fx.state(TABLE_NO_OVERLAP), NA);
        assert_eq!(fx.state(TABLE_COVERAGE), PASS);
        // With a UNIQUE table beside it, only the UNIQUE table is checked.
        let both = fx
            .with(decision_table("dt:discount", "UNIQUE"))
            .table(complete_spec("dt:discount"));
        assert_eq!(both.state(TABLE_NO_OVERLAP), PASS);
        assert_eq!(targets(&both, TABLE_NO_OVERLAP), ["dt:discount"]);
    }

    #[test]
    fn f2_decision_table_coverage() {
        assert_eq!(Fx::clean().state(TABLE_COVERAGE), PASS);
        let by_default = Fx::clean().tables(vec![{
            let mut s = spec("dt:discount", "UNIQUE", vec![row(flag(true), true)]);
            s.spec.default_output = Some(vec![DecisionOutputCell::Bool { value: false }]);
            s
        }]);
        assert_eq!(by_default.state(TABLE_COVERAGE), PASS);
        let incomplete = Fx::clean().tables(vec![spec(
            "dt:discount",
            "UNIQUE",
            vec![row(flag(true), true)],
        )]);
        assert_eq!(incomplete.state(TABLE_COVERAGE), FAIL);
        assert_eq!(incomplete.state(TABLE_NO_OVERLAP), PASS);
        // An unbounded numeric column is not analyzable: ERROR, never PASS.
        let numeric = Fx::clean().tables(vec![F2DecisionTableInput {
            decision_table_ref: id("dt:discount"),
            spec: DecisionTableSpec {
                hit_policy: "UNIQUE".into(),
                inputs: vec![DecisionColumn {
                    name: "n".into(),
                    ty: DecisionType::Int,
                    domain: None,
                }],
                outputs: vec![bool_column("out")],
                rows: vec![row(DecisionInputCell::Any, true)],
                default_output: None,
            },
        }]);
        assert_eq!(
            error_code(&numeric, TABLE_COVERAGE),
            "DECISION_TABLE_ANALYSIS_UNAVAILABLE"
        );
        // Invalid structure cannot be trusted for overlap either.
        let invalid = Fx::clean().tables(vec![spec(
            "dt:discount",
            "UNIQUE",
            vec![DecisionRow {
                inputs: vec![],
                outputs: vec![DecisionOutputCell::Bool { value: true }],
            }],
        )]);
        assert_eq!(
            error_code(&invalid, TABLE_NO_OVERLAP),
            "DECISION_TABLE_ANALYSIS_UNAVAILABLE"
        );
        assert_eq!(
            error_code(&invalid, TABLE_COVERAGE),
            "DECISION_TABLE_ANALYSIS_UNAVAILABLE"
        );
        // No Accepted DecisionTable.
        let none = Fx::clean().without("dt:discount").tables(vec![]);
        assert_eq!(none.state(TABLE_NO_OVERLAP), NA);
        assert_eq!(none.state(TABLE_COVERAGE), NA);
    }

    // ------------------------------------------------------------------ processes

    const P: &str = "process:p";

    fn process_node(key: &str, kind: K, operation: Option<&str>) -> Node {
        node(
            &format!("pn:{key}"),
            Accepted,
            NodePayload::ProcessNode(ProcessNode {
                process_ref: id(P),
                node_kind: kind,
                operation_ref: operation.map(id),
                condition_expr: None,
                message_ref: None,
                timer_expr: None,
            }),
        )
    }

    fn next(fx: Fx, from: &str, to: &str) -> Fx {
        fx.rel(
            RelationKind::Next,
            &format!("pn:{from}"),
            &format!("pn:{to}"),
        )
    }

    /// start -> submit (service task) -> end.
    fn process() -> Fx {
        let fx = Fx::clean()
            .with(node(
                P,
                Accepted,
                NodePayload::Process(Process {
                    name: "P".into(),
                    description: None,
                    process_kind: None,
                }),
            ))
            .with(process_node("start", K::Start, None))
            .with(process_node(
                "submit",
                K::ServiceTask,
                Some("operation:submit"),
            ))
            .with(process_node("end", K::End, None));
        next(next(fx, "start", "submit"), "submit", "end")
    }

    #[test]
    fn f2_process_start_end_pass_fail() {
        assert_eq!(Fx::clean().state(PROCESS_START_END), NA);
        assert_eq!(process().state(PROCESS_START_END), PASS);
        let no_end = process().without("pn:end").with(process_node(
            "approve",
            K::ServiceTask,
            Some("operation:approve"),
        ));
        let no_end = next(no_end, "submit", "approve");
        assert_eq!(no_end.state(PROCESS_START_END), FAIL);
        let no_start = process().without("pn:start");
        assert_eq!(no_start.state(PROCESS_START_END), FAIL);
    }

    #[test]
    fn f2_process_reachability_pass_fail() {
        assert_eq!(process().state(PROCESS_REACHABLE), PASS);
        let orphan = process().with(process_node(
            "orphan",
            K::ServiceTask,
            Some("operation:approve"),
        ));
        let orphan = next(orphan, "orphan", "end");
        assert_eq!(orphan.state(PROCESS_REACHABLE), FAIL);
        assert!(targets(&orphan, PROCESS_REACHABLE).contains(&"pn:orphan".to_owned()));
    }

    #[test]
    fn f2_process_task_resolution_pass_fail() {
        assert_eq!(process().state(TASK_RESOLVES), PASS);
        let unresolved =
            process()
                .without("pn:submit")
                .with(process_node("submit", K::ServiceTask, None));
        let unresolved = next(next(unresolved, "start", "submit"), "submit", "end");
        assert_eq!(unresolved.state(TASK_RESOLVES), FAIL);
        assert_eq!(targets(&unresolved, TASK_RESOLVES), ["pn:submit"]);
    }

    fn parallel(balanced: bool) -> Fx {
        let fx = Fx::clean()
            .with(node(
                P,
                Accepted,
                NodePayload::Process(Process {
                    name: "P".into(),
                    description: None,
                    process_kind: None,
                }),
            ))
            .with(process_node("start", K::Start, None))
            .with(process_node("split", K::ParallelSplit, None))
            .with(process_node("a", K::ServiceTask, Some("operation:submit")))
            .with(process_node("b", K::ServiceTask, Some("operation:approve")))
            .with(process_node("end", K::End, None));
        let fx = next(next(next(fx, "start", "split"), "split", "a"), "split", "b");
        if balanced {
            let fx = fx.with(process_node("join", K::ParallelJoin, None));
            next(next(next(fx, "a", "join"), "b", "join"), "join", "end")
        } else {
            let fx = fx.with(process_node("end2", K::End, None));
            next(next(fx, "a", "end"), "b", "end2")
        }
    }

    #[test]
    fn f2_process_parallel_balanced_unbalanced() {
        assert_eq!(process().state(GATEWAY_BALANCED), NA);
        assert_eq!(parallel(true).state(GATEWAY_BALANCED), PASS);
        assert_eq!(parallel(true).state(PROCESS_REACHABLE), PASS);
        let unbalanced = parallel(false);
        assert_eq!(unbalanced.state(GATEWAY_BALANCED), FAIL);
        assert!(targets(&unbalanced, GATEWAY_BALANCED).contains(&P.to_owned()));
    }

    #[test]
    fn f2_process_unsupported_semantics_error() {
        let fx = process().with(process_node("choice", K::ExclusiveGateway, None));
        let fx = next(fx, "choice", "end");
        for rule_id in [PROCESS_START_END, PROCESS_REACHABLE, TASK_RESOLVES] {
            assert_eq!(error_code(&fx, rule_id), "PROCESS_UNSUPPORTED_SEMANTICS");
        }
        assert_eq!(targets(&fx, PROCESS_REACHABLE), ["pn:choice", P]);
        let parallel = parallel(true).with(process_node("choice", K::ExclusiveGateway, None));
        let parallel = next(parallel, "choice", "end");
        assert_eq!(
            error_code(&parallel, GATEWAY_BALANCED),
            "PROCESS_UNSUPPORTED_SEMANTICS"
        );
    }

    // ------------------------------------------------------------------ events

    #[test]
    fn f2_event_payload() {
        assert_eq!(Fx::clean().state(PAYLOAD_TYPED), NA);
        let attribute = Fx::clean().with(event("event:paid", Some("attr:amount")));
        assert_eq!(attribute.state(PAYLOAD_TYPED), PASS);
        let data_schema = Fx::clean()
            .with(schema("schema:paid", Accepted))
            .with(event("event:paid", Some("schema:paid")));
        assert_eq!(data_schema.state(PAYLOAD_TYPED), PASS);
        for bad in ["attr:text", "entity:order", "attr:ghost"] {
            let fx = Fx::clean().with(event("event:paid", Some(bad)));
            assert_eq!(fx.state(PAYLOAD_TYPED), FAIL, "{bad}");
            assert_eq!(targets(&fx, PAYLOAD_TYPED), ["event:paid"]);
        }
        let draft = Fx::clean()
            .with(schema("schema:paid", Proposed))
            .with(event("event:paid", Some("schema:paid")));
        assert_eq!(draft.state(PAYLOAD_TYPED), FAIL);
    }

    #[test]
    fn f2_event_producer() {
        assert_eq!(Fx::clean().state(EVENT_PRODUCER), PASS);
        let none = Fx::clean().unrel(
            RelationKind::Consumes,
            "operation:approve",
            "event:submitted",
        );
        assert_eq!(none.state(EVENT_PRODUCER), NA);
        let external = Fx::clean().with(event("event:external", None)).rel(
            RelationKind::Consumes,
            "operation:approve",
            "event:external",
        );
        assert_eq!(
            error_code(&external, EVENT_PRODUCER),
            "EVENT_SOURCE_CLASSIFICATION_UNAVAILABLE"
        );
        assert_eq!(targets(&external, EVENT_PRODUCER), ["event:external"]);
    }

    #[test]
    fn f2_event_consumer() {
        assert_eq!(Fx::clean().state(EVENT_CONSUMER), PASS);
        let none = Fx::clean().unrel(
            RelationKind::Produces,
            "operation:submit",
            "event:submitted",
        );
        assert_eq!(none.state(EVENT_CONSUMER), NA);
        let audit = Fx::clean().with(event("event:audit", None)).rel(
            RelationKind::Produces,
            "operation:submit",
            "event:audit",
        );
        assert_eq!(
            error_code(&audit, EVENT_CONSUMER),
            "EVENT_PURPOSE_CLASSIFICATION_UNAVAILABLE"
        );
        assert_eq!(targets(&audit, EVENT_CONSUMER), ["event:audit"]);
    }

    // ------------------------------------------------------------------ authorization

    #[test]
    fn f2_permission_concrete_pass_fail() {
        assert_eq!(Fx::clean().state(PERMISSION_CONCRETE), PASS);
        let suspect_scope = Fx::clean().status("scope:orders", Suspect);
        assert_eq!(suspect_scope.state(PERMISSION_CONCRETE), FAIL);
        assert_eq!(
            targets(&suspect_scope, PERMISSION_CONCRETE),
            ["permission:approve"]
        );
        let suspect_edge = Fx::clean()
            .unrel(
                RelationKind::Permits,
                "permission:approve",
                "operation:approve",
            )
            .rel_as(
                RelationKind::Permits,
                "permission:approve",
                "operation:approve",
                Suspect,
            );
        assert_eq!(suspect_edge.state(PERMISSION_CONCRETE), FAIL);
    }

    #[test]
    fn f2_role_hierarchy_pass_and_cycle() {
        let fx = Fx::clean()
            .with(security_role("secrole:base", Accepted))
            .rel(
                RelationKind::InheritsRole,
                "secrole:approver",
                "secrole:base",
            );
        assert_eq!(fx.state(HIERARCHY_ACYCLIC), PASS);
        // A Proposed edge closing a cycle is not Accepted hierarchy.
        let proposed = fx.clone().rel_as(
            RelationKind::InheritsRole,
            "secrole:base",
            "secrole:approver",
            Proposed,
        );
        assert_eq!(proposed.state(HIERARCHY_ACYCLIC), PASS);
        // An Accepted cycle is already rejected by PSG baseline validation, so the
        // evaluator's FAIL path is reached only through the shared kernel.
        let cyclic = fx.rel(
            RelationKind::InheritsRole,
            "secrole:base",
            "secrole:approver",
        );
        assert!(Graph::new(
            id("project:pilot"),
            id("profile:plumb-software-2026.1"),
            cyclic.nodes,
            cyclic.edges
        )
        .is_err());
        assert_eq!(
            plumb_validation::authorization_analysis::role_hierarchy_cycles(&[
                (id("secrole:approver"), id("secrole:base")),
                (id("secrole:base"), id("secrole:approver")),
            ]),
            vec![vec![id("secrole:approver"), id("secrole:base")]]
        );
    }

    #[test]
    fn f2_role_separation_pass_bad_assigned_role() {
        assert_eq!(Fx::clean().state(ROLE_SEPARATION), PASS);
        let bad = Fx::clean().with(security_role("secrole:old", Suspect)).rel(
            RelationKind::AssignedRole,
            "principal:alice",
            "secrole:old",
        );
        assert_eq!(bad.state(ROLE_SEPARATION), FAIL);
        assert_eq!(
            targets(&bad, ROLE_SEPARATION),
            ["principal:alice", "secrole:old"]
        );
    }

    fn separation(kind: SeparationConstraintKind) -> Node {
        node(
            "sod:approve-request",
            Accepted,
            NodePayload::SeparationConstraint(SeparationConstraint {
                constraint_kind: kind,
                role_refs: vec![id("secrole:approver"), id("secrole:requester")],
            }),
        )
    }

    #[test]
    fn f2_static_sod_pass_fail() {
        assert_eq!(Fx::clean().state(SOD_NO_VIOLATION), NA);
        let fx = Fx::clean().with(separation(SeparationConstraintKind::StaticSeparationOfDuty));
        assert_eq!(fx.state(SOD_NO_VIOLATION), PASS);
        let violated = fx.rel(
            RelationKind::AssignedRole,
            "principal:alice",
            "secrole:requester",
        );
        assert_eq!(violated.state(SOD_NO_VIOLATION), FAIL);
        assert_eq!(
            targets(&violated, SOD_NO_VIOLATION),
            ["principal:alice", "sod:approve-request"]
        );
    }

    #[test]
    fn f2_non_static_sod_only_not_applicable() {
        for kind in [
            SeparationConstraintKind::DynamicSeparationOfDuty,
            SeparationConstraintKind::MutualExclusion,
            SeparationConstraintKind::RequiredCombination,
        ] {
            let fx = Fx::clean().with(separation(kind)).rel(
                RelationKind::AssignedRole,
                "principal:alice",
                "secrole:requester",
            );
            assert_eq!(fx.state(SOD_NO_VIOLATION), NA);
        }
    }

    // ------------------------------------------------------------------ invariants and blockers

    #[test]
    fn f2_invariant_valid_syntax_and_type_failures() {
        assert_eq!(Fx::clean().state(INVARIANT_EXPRESSIBLE), PASS);
        let empty_scope = Fx::clean().invariant("inv:literal", "1 == 1", vec![]);
        assert_eq!(empty_scope.state(INVARIANT_EXPRESSIBLE), PASS);
        let syntax = Fx::clean().invariant(
            "inv:broken",
            "amount >=",
            vec![binding("attr:amount", "amount")],
        );
        assert_eq!(syntax.state(INVARIANT_EXPRESSIBLE), FAIL);
        assert_eq!(targets(&syntax, INVARIANT_EXPRESSIBLE), ["inv:broken"]);
        let types = Fx::clean().invariant(
            "inv:typed",
            "amount and true",
            vec![binding("attr:amount", "amount")],
        );
        assert_eq!(types.state(INVARIANT_EXPRESSIBLE), FAIL);
        // An unbound symbol is a type failure: no name inference.
        let unbound = Fx::clean().invariant("inv:unbound", "amount >= 0", vec![]);
        assert_eq!(unbound.state(INVARIANT_EXPRESSIBLE), FAIL);
        // A non-boolean expression is not a predicate.
        let numeric = Fx::clean().invariant(
            "inv:numeric",
            "amount + 1",
            vec![binding("attr:amount", "amount")],
        );
        assert_eq!(numeric.state(INVARIANT_EXPRESSIBLE), FAIL);
    }

    fn finding_node(
        node_id: &str,
        code: &str,
        family: &str,
        severity: FindingSeverity,
        status: &str,
    ) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Finding(Finding {
                code: code.into(),
                family: family.into(),
                severity,
                message: "open".into(),
                status: status.into(),
                affected_refs: vec![id("calculation:total")],
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            }),
        )
    }

    #[test]
    fn f2_semantic_blocker_clear_and_present() {
        let clear = Fx::clean()
            .with(finding_node(
                "fnd:00000000000000b1",
                CALC_TYPECHECK,
                "F2",
                FindingSeverity::Error,
                "Open",
            ))
            .with(finding_node(
                "fnd:00000000000000b2",
                CALC_TYPECHECK,
                "F2",
                FindingSeverity::Blocker,
                "Resolved",
            ))
            .with(finding_node(
                "fnd:00000000000000b3",
                "PLUMB.F1.REQ.GROUNDED",
                "F1",
                FindingSeverity::Blocker,
                "Open",
            ))
            .with(finding_node(
                "fnd:00000000000000b4",
                NO_SEMANTIC_BLOCKER,
                "F2",
                FindingSeverity::Blocker,
                "Open",
            ))
            .finding(finding(
                RELATION_TYPED,
                "domain_relationship_cardinality_unresolved:x",
                &["entity:order"],
                FindingSeverity::Error,
            ));
        assert_eq!(clear.state(NO_SEMANTIC_BLOCKER), PASS);
        let node_blocker = Fx::clean().with(finding_node(
            "fnd:00000000000000b5",
            CALC_TYPECHECK,
            "F2",
            FindingSeverity::Blocker,
            "Open",
        ));
        assert_eq!(node_blocker.state(NO_SEMANTIC_BLOCKER), FAIL);
        assert_eq!(
            targets(&node_blocker, NO_SEMANTIC_BLOCKER),
            ["fnd:00000000000000b5"]
        );
        let supplemental = Fx::clean().finding(finding(
            TRANSITION_COMPLETE,
            "state_transition_trigger_unresolved:y",
            &["entity:customer"],
            FindingSeverity::Blocker,
        ));
        assert_eq!(supplemental.state(NO_SEMANTIC_BLOCKER), FAIL);
        assert_eq!(
            targets(&supplemental, NO_SEMANTIC_BLOCKER),
            ["entity:customer"]
        );
    }

    // ------------------------------------------------------------------ determinism and purity

    /// Several members in every unordered collection.
    fn rich() -> Fx {
        let mut fx = parallel(true)
            .with(calculation(
                "calculation:legacy",
                "amount * 3",
                "Decimal(2)",
                None,
                None,
                None,
            ))
            .with(calculation(
                "calculation:legacy-b",
                "amount + amount",
                "Decimal(2)",
                None,
                None,
                None,
            ))
            .scope_override(
                "calculation:legacy",
                CalculationScope {
                    bindings: vec![binding("attr:amount", "amount")],
                    calendar_refs: vec![],
                },
            )
            .scope_override(
                "calculation:legacy-b",
                CalculationScope {
                    bindings: vec![binding("attr:amount", "amount")],
                    calendar_refs: vec![],
                },
            )
            .with(decision_table("dt:priority", "FIRST"))
            .table(spec("dt:priority", "FIRST", vec![row(flag(true), true)]))
            .invariant("inv:literal", "1 == 1", vec![])
            .with(separation(SeparationConstraintKind::StaticSeparationOfDuty))
            .rel(
                RelationKind::AssignedRole,
                "principal:alice",
                "secrole:requester",
            )
            .finding(finding(
                RELATION_TYPED,
                "domain_relationship_cardinality_unresolved:x",
                &["entity:order"],
                FindingSeverity::Error,
            ))
            .finding(finding(
                TRANSITION_COMPLETE,
                "state_transition_trigger_unresolved:y",
                &["entity:customer"],
                FindingSeverity::Blocker,
            ));
        fx = fx.with(event("event:audit", None)).rel(
            RelationKind::Produces,
            "operation:approve",
            "event:audit",
        );
        fx
    }

    #[test]
    fn f2_determinism_under_reordering() {
        let forward = rich();
        let mut reversed = rich();
        reversed.nodes.reverse();
        reversed.edges.reverse();
        reversed.findings.reverse();
        reversed.overrides.reverse();
        reversed.tables.reverse();
        reversed.invariants.reverse();
        for i in &mut reversed.invariants {
            i.bindings.reverse();
        }
        let a = forward.report();
        let b = reversed.report();
        assert_eq!(a, b);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        // The rich fixture exercises every result class.
        let states: BTreeSet<String> = a.rules.iter().map(|r| format!("{:?}", r.state)).collect();
        for s in ["Pass", "Fail", "NotApplicable", "Error"] {
            assert!(states.contains(s), "{s}");
        }
        assert_eq!(forward.report(), a);
    }

    #[test]
    fn f2_production_source_guard() {
        let source = include_str!("../src/rules/f2.rs");
        for token in [
            "apply_patch",
            "SemanticPatch",
            "Proposal",
            "InferenceRequest",
            "InferenceArtifact",
            "InferenceProvider",
            "MockProvider",
            "plumb_functional",
            "plumb_inference",
            "std::fs",
            "File::open",
            "reqwest",
            "std::net",
            "SystemClock",
            "Utc::now",
            "Instant::now",
            "Timestamp",
            "ArtifactStore",
            "RevisionStore",
            "rusqlite",
            "commit(",
            "rand",
            "f64",
            "f32",
            "unsafe",
            // No second analysis algorithm or TypeEnv builder.
            "fn tarjan",
            "strongly_connected",
            "VecDeque",
            "TypeEnv::new",
            "bind_root",
            "fn analyze_",
            "fn cells_intersect",
            ".name ==",
            "to_lowercase",
            "to_ascii_lowercase",
        ] {
            assert!(!source.contains(token), "{token}");
        }
        let mod_rs = include_str!("../src/rules/mod.rs");
        assert!(mod_rs.contains("mod f2;") && !mod_rs.contains("pub mod f2;"));
        assert!(mod_rs.contains("pub use f2::register_f2_evaluators;"));
        assert!(!mod_rs.contains("fn ") && !mod_rs.contains("F2."));
    }
}
