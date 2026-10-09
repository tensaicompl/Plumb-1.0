//! Hotfix 044 contract tests for the shared expression-scope adapter and the F2 supplemental
//! inputs. The adapter keeps the exact S2.6 binding semantics; the inputs are canonical,
//! content-hashed and validated against the graph, and stale or malformed inputs are context
//! errors. All graphs are synthetic.

mod expression_scope_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{Hash, Id, Timestamp};
    use plumb_expr::{Symbol, Ty, Unit};
    use plumb_psg::{
        Attribute, AuditMeta, Calculation, DecisionTable, Edge, ElementStatus, Entity, Finding,
        FindingSeverity, Graph, Invariant, Node, NodePayload, RelationKind, RelationProperties,
    };
    use plumb_validation::calculation_analysis::{CalculationScope, CALCULATION_ORIGIN_EXTENSION};
    use plumb_validation::decision_table_analysis::{
        DecisionColumn, DecisionTableSpec, DecisionType,
    };
    use plumb_validation::expression_scope::*;
    use plumb_validation::{
        finding_id, finding_key, EvaluationError, F2CalculationScopeOverride, F2DecisionTableInput,
        F2InvariantInput, F2ValidationInputs, GeneratedFinding,
    };
    use serde_json::json;

    use ElementStatus::{Accepted, Proposed};

    const AT: &str = "2026-01-01T00:00:00.000000000Z";

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

    fn attribute(
        node_id: &str,
        name: &str,
        value_type: &str,
        unit: Option<&str>,
        members: Option<&[&str]>,
    ) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Attribute(Attribute {
                name: name.into(),
                value_type: value_type.into(),
                nullable: false,
                unit: unit.map(Into::into),
                precision: None,
                enum_values: members.map(|m| m.iter().map(|s| (*s).to_owned()).collect()),
                data_classification: None,
            }),
        )
    }

    fn calculation(node_id: &str, expression: &str, result_type: &str, with_origin: bool) -> Node {
        let mut n = node(
            node_id,
            Accepted,
            NodePayload::Calculation(Calculation {
                name: node_id.into(),
                expression: expression.into(),
                result_type: result_type.into(),
                unit: None,
                rounding: None,
                calendar_ref: None,
                examples: None,
            }),
        );
        if with_origin {
            let range = json!({"requirement_ref": "req:r1", "start": 0, "end": 1});
            n.extensions.insert(
                CALCULATION_ORIGIN_EXTENSION.parse().unwrap(),
                json!({"name_range": range, "expression_evidence": [range], "used_bindings": []}),
            );
        }
        n
    }

    fn edge(kind: RelationKind, from: &str, to: &str) -> Edge {
        Edge {
            id: id(&format!(
                "rel:{}-{}",
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

    fn nodes() -> Vec<Node> {
        vec![
            node(
                "entity:order",
                Accepted,
                NodePayload::Entity(Entity {
                    name: "Order".into(),
                    description: None,
                    aggregate_root: None,
                }),
            ),
            attribute("attr:amount", "amount", "Decimal(2)", None, None),
            attribute("attr:hours", "hours", "Decimal(1)", Some("hour"), None),
            attribute("attr:kind", "kind", "Enum", None, Some(&["Annual", "Sick"])),
            attribute("attr:empty-enum", "e", "Enum", None, None),
            attribute("attr:text", "t", "Text", None, None),
            calculation("calculation:total", "amount * 2", "Decimal(2)", true),
            calculation("calculation:legacy", "1", "Int", false),
            node(
                "dt:discount",
                Accepted,
                NodePayload::DecisionTable(DecisionTable {
                    name: "Discount".into(),
                    hit_policy: "UNIQUE".into(),
                    inputs: vec![],
                    outputs: vec![],
                    rows: vec![],
                }),
            ),
            node(
                "dt:draft",
                Proposed,
                NodePayload::DecisionTable(DecisionTable {
                    name: "Draft".into(),
                    hit_policy: "FIRST".into(),
                    inputs: vec![],
                    outputs: vec![],
                    rows: vec![],
                }),
            ),
            node(
                "inv:positive",
                Accepted,
                NodePayload::Invariant(Invariant {
                    scope_ref: id("entity:order"),
                    expression: "amount >= 0".into(),
                }),
            ),
        ]
    }

    fn graph_with(extra: Vec<Node>) -> Graph {
        let mut n = nodes();
        n.extend(extra);
        let edges = [
            "attr:amount",
            "attr:hours",
            "attr:kind",
            "attr:empty-enum",
            "attr:text",
        ]
        .iter()
        .map(|a| edge(RelationKind::HasAttribute, "entity:order", a))
        .collect();
        Graph::new(
            id("project:pilot"),
            id("profile:plumb-software-2026.1"),
            n,
            edges,
        )
        .unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn graph() -> Graph {
        graph_with(vec![])
    }

    fn root(node_ref: &str, symbol: &str) -> ExpressionScopeBinding {
        ExpressionScopeBinding {
            node_ref: id(node_ref),
            symbol: symbol.into(),
            exposure: ExpressionBindingExposure::Root,
        }
    }

    fn field(node_ref: &str, owner: &str, symbol: &str) -> ExpressionScopeBinding {
        ExpressionScopeBinding {
            node_ref: id(node_ref),
            symbol: symbol.into(),
            exposure: ExpressionBindingExposure::Field {
                owner_ref: id(owner),
            },
        }
    }

    // ------------------------------------------------------------------ adapter

    #[test]
    fn expression_scope_mappings() {
        let g = graph();
        let s = validate_expression_bindings(
            &g,
            &[
                root("attr:kind", "kind"),
                root("entity:order", "Order"),
                field("attr:amount", "entity:order", "amount"),
                root("attr:amount", "amount"),
                root("attr:hours", "hours"),
                root("calculation:total", "total"),
            ],
            &[],
        )
        .unwrap();
        let sym = |s: &str| Symbol::new(s).unwrap();
        assert_eq!(
            s.env.root(&sym("Order")),
            Some(&Ty::Ref(id("entity:order")))
        );
        assert_eq!(s.env.root(&sym("amount")), Some(&Ty::Decimal(2)));
        assert_eq!(
            s.env.root(&sym("hours")),
            Some(&Ty::Quantity(Unit::new("hour").unwrap()))
        );
        assert_eq!(s.env.root(&sym("total")), Some(&Ty::Decimal(2)));
        assert_eq!(s.env.root(&sym("kind")), Some(&Ty::Enum(id("attr:kind"))));
        assert_eq!(s.env.enum_members(&id("attr:kind")).unwrap().len(), 2);
        assert_eq!(
            s.env.field(&id("entity:order"), &sym("amount")),
            Some(&Ty::Decimal(2))
        );
        // Canonical order: Root before Field.
        assert_eq!(
            s.bindings.last().unwrap(),
            &field("attr:amount", "entity:order", "amount")
        );
        assert_eq!(
            canonical_attribute_owner(&g, &id("attr:amount")),
            Some(&id("entity:order"))
        );
    }

    #[test]
    fn expression_scope_rejections() {
        let g = graph();
        let err = |b: Vec<ExpressionScopeBinding>, reserved: &[&str]| {
            validate_expression_bindings(&g, &b, reserved).unwrap_err()
        };
        assert!(matches!(
            err(vec![root("attr:amount", "Amount Value")], &[]),
            ExpressionScopeIssue::InvalidScope { .. }
        ));
        assert_eq!(
            err(vec![root("attr:amount", "calendar")], &["calendar"]),
            ExpressionScopeIssue::ReservedSymbol {
                node_ref: id("attr:amount")
            }
        );
        assert!(validate_expression_bindings(&g, &[root("attr:amount", "calendar")], &[]).is_ok());
        assert!(matches!(
            err(vec![root("attr:amount", "x"), root("attr:hours", "x")], &[]),
            ExpressionScopeIssue::ScopeBindingConflict {
                owner_ref: None,
                ..
            }
        ));
        assert_eq!(
            err(vec![root("attr:ghost", "x")], &[]),
            ExpressionScopeIssue::UnknownInputRef {
                node_ref: id("attr:ghost")
            }
        );
        assert!(matches!(
            err(vec![root("attr:empty-enum", "e")], &[]),
            ExpressionScopeIssue::UnsupportedInputType { .. }
        ));
        assert!(matches!(
            err(vec![root("attr:text", "t")], &[]),
            ExpressionScopeIssue::UnsupportedInputType { .. }
        ));
        assert!(matches!(
            err(vec![field("attr:amount", "entity:other", "a")], &[]),
            ExpressionScopeIssue::InvalidScope { .. }
        ));
        assert!(matches!(
            err(vec![field("calculation:total", "entity:order", "t")], &[]),
            ExpressionScopeIssue::InvalidScope { .. }
        ));
        assert!(matches!(
            err(vec![root("dt:discount", "d")], &[]),
            ExpressionScopeIssue::InvalidScope { .. }
        ));
        assert_eq!(
            declared_result_type("Enum", None),
            Err(ResultTypeIssue::UnsupportedResultType {
                result_type: "Enum".into()
            })
        );
        assert!(matches!(
            declared_result_type("Bool", Some("hour")),
            Err(ResultTypeIssue::UnitMismatch { .. })
        ));
        assert_eq!(
            declared_result_type("Decimal(2)", Some("hour")),
            Ok(Ty::Quantity(Unit::new("hour").unwrap()))
        );
    }

    #[test]
    fn expression_scope_is_the_s2_6_binding() {
        // The S2.6 names are the shared types, not copies.
        let shared: ExpressionScopeBinding =
            plumb_functional::calculation::CalculationScopeBinding {
                node_ref: id("attr:amount"),
                symbol: "amount".into(),
                exposure: plumb_functional::calculation::CalculationBindingExposure::Root,
            };
        assert_eq!(shared, root("attr:amount", "amount"));
        assert_eq!(
            serde_json::to_value(&shared).unwrap(),
            json!({"node_ref": "attr:amount", "symbol": "amount", "exposure": {"kind": "root"}})
        );
    }

    // ------------------------------------------------------------------ F2 supplemental inputs

    const CARDINALITY: &str = "domain_relationship_cardinality_unresolved:entity:order";
    const TRIGGER: &str = "state_transition_trigger_unresolved:entity:order";

    fn f2_finding(code: &str, condition: &str, targets: &[&str], family: &str) -> GeneratedFinding {
        let affected: Vec<Id> = targets.iter().map(|t| id(t)).collect();
        let key = finding_key(code, &affected, condition).unwrap();
        GeneratedFinding {
            id: finding_id(&key).unwrap(),
            key,
            semantic_condition_key: condition.into(),
            payload: Finding {
                code: code.into(),
                family: family.into(),
                severity: FindingSeverity::Error,
                message: "unresolved".into(),
                status: "Open".into(),
                affected_refs: affected,
                standard_rule_ref: None,
                suggested_resolution: None,
                waiver_ref: None,
            },
        }
    }

    fn domain_finding() -> GeneratedFinding {
        f2_finding(
            "PLUMB.F2.DOMAIN.RELATION_TYPED",
            CARDINALITY,
            &["entity:order"],
            "F2",
        )
    }

    fn state_finding() -> GeneratedFinding {
        f2_finding(
            "PLUMB.F2.STATE.TRANSITION_COMPLETE",
            TRIGGER,
            &["entity:order"],
            "F2",
        )
    }

    fn table_for(table_ref: &str, hit_policy: &str) -> F2DecisionTableInput {
        F2DecisionTableInput {
            decision_table_ref: id(table_ref),
            spec: DecisionTableSpec {
                hit_policy: hit_policy.into(),
                inputs: vec![DecisionColumn {
                    name: "flag".into(),
                    ty: DecisionType::Bool,
                    domain: None,
                }],
                outputs: vec![DecisionColumn {
                    name: "out".into(),
                    ty: DecisionType::Bool,
                    domain: None,
                }],
                rows: vec![],
                default_output: None,
            },
        }
    }

    fn table() -> F2DecisionTableInput {
        table_for("dt:discount", "UNIQUE")
    }

    fn override_for(calculation_ref: &str, scope: CalculationScope) -> F2CalculationScopeOverride {
        F2CalculationScopeOverride {
            calculation_ref: id(calculation_ref),
            scope,
        }
    }

    fn legacy_override() -> F2CalculationScopeOverride {
        override_for("calculation:legacy", CalculationScope::default())
    }

    fn invariant_for(
        invariant_ref: &str,
        bindings: Vec<ExpressionScopeBinding>,
    ) -> F2InvariantInput {
        F2InvariantInput {
            invariant_ref: id(invariant_ref),
            bindings,
        }
    }

    fn invariant() -> F2InvariantInput {
        invariant_for("inv:positive", vec![root("attr:amount", "amount")])
    }

    fn inputs(findings: Vec<GeneratedFinding>) -> F2ValidationInputs {
        F2ValidationInputs::new(
            findings,
            vec![legacy_override()],
            vec![table()],
            vec![invariant()],
        )
    }

    fn with(
        overrides: Vec<F2CalculationScopeOverride>,
        tables: Vec<F2DecisionTableInput>,
        invariants: Vec<F2InvariantInput>,
    ) -> F2ValidationInputs {
        F2ValidationInputs::new(vec![], overrides, tables, invariants)
    }

    fn context_error(g: &Graph, i: &F2ValidationInputs) -> bool {
        matches!(i.validate_for(g), Err(EvaluationError::InvalidContext(_)))
    }

    /// A second legacy Calculation, Accepted DecisionTable and Invariant, so every collection
    /// has more than one member.
    fn wide_graph() -> Graph {
        graph_with(vec![
            calculation("calculation:legacy-b", "2", "Int", false),
            node(
                "dt:surcharge",
                Accepted,
                NodePayload::DecisionTable(DecisionTable {
                    name: "Surcharge".into(),
                    hit_policy: "FIRST".into(),
                    inputs: vec![],
                    outputs: vec![],
                    rows: vec![],
                }),
            ),
            node(
                "inv:bounded",
                Accepted,
                NodePayload::Invariant(Invariant {
                    scope_ref: id("entity:order"),
                    expression: "hours >= 0 hour".into(),
                }),
            ),
        ])
    }

    fn wide_parts() -> (
        Vec<GeneratedFinding>,
        Vec<F2CalculationScopeOverride>,
        Vec<F2DecisionTableInput>,
        Vec<F2InvariantInput>,
    ) {
        let scope = CalculationScope {
            bindings: vec![root("attr:hours", "hours"), root("attr:amount", "amount")],
            calendar_refs: vec![],
        };
        (
            vec![domain_finding(), state_finding()],
            vec![
                legacy_override(),
                override_for("calculation:legacy-b", scope),
            ],
            vec![table(), table_for("dt:surcharge", "FIRST")],
            vec![
                invariant(),
                invariant_for(
                    "inv:bounded",
                    vec![root("attr:hours", "hours"), root("attr:amount", "amount")],
                ),
            ],
        )
    }

    #[test]
    fn f2_inputs_canonical_ordering() {
        let (mut f, mut o, mut t, mut i) = wide_parts();
        f.reverse();
        o.reverse();
        t.reverse();
        i.reverse();
        let canonical = F2ValidationInputs::new(f, o, t, i);
        let finding_ids: Vec<&Id> = canonical.analysis_findings.iter().map(|f| &f.id).collect();
        assert!(finding_ids.windows(2).all(|p| p[0] < p[1]));
        let override_refs: Vec<&Id> = canonical
            .calculation_scope_overrides
            .iter()
            .map(|o| &o.calculation_ref)
            .collect();
        assert_eq!(
            override_refs,
            [&id("calculation:legacy"), &id("calculation:legacy-b")]
        );
        let table_refs: Vec<&Id> = canonical
            .decision_tables
            .iter()
            .map(|t| &t.decision_table_ref)
            .collect();
        assert_eq!(table_refs, [&id("dt:discount"), &id("dt:surcharge")]);
        let invariant_refs: Vec<&Id> = canonical
            .invariants
            .iter()
            .map(|i| &i.invariant_ref)
            .collect();
        assert_eq!(invariant_refs, [&id("inv:bounded"), &id("inv:positive")]);
        let sorted = vec![root("attr:amount", "amount"), root("attr:hours", "hours")];
        assert_eq!(
            canonical.calculation_scope_overrides[1].scope.bindings,
            sorted
        );
        assert_eq!(canonical.invariants[0].bindings, sorted);
        canonical.validate_for(&wide_graph()).unwrap();
        // Inputs that bypass the canonicalizing constructor are rejected, not reordered.
        let mut unsorted = canonical.clone();
        unsorted.decision_tables.reverse();
        assert!(context_error(&wide_graph(), &unsorted));
        let mut unsorted_scope = canonical.clone();
        unsorted_scope.calculation_scope_overrides[1]
            .scope
            .bindings
            .reverse();
        assert!(context_error(&wide_graph(), &unsorted_scope));
        let mut unsorted_bindings = canonical;
        unsorted_bindings.invariants[0].bindings.reverse();
        assert!(context_error(&wide_graph(), &unsorted_bindings));
    }

    #[test]
    fn f2_inputs_content_hash_determinism() {
        let a = inputs(vec![domain_finding(), state_finding()]);
        let b = inputs(vec![domain_finding(), state_finding()]);
        assert_eq!(a.content_hash().unwrap(), b.content_hash().unwrap());
        assert_eq!(a.content_hash().unwrap(), a.clone().content_hash().unwrap());
        let hash: Hash = a.content_hash().unwrap();
        assert!(hash.as_str().starts_with("sha256:"));
        // The hash covers content.
        assert_ne!(
            a.content_hash().unwrap(),
            inputs(vec![domain_finding()]).content_hash().unwrap()
        );
        let mut retyped = table();
        retyped.spec.inputs[0].name = "other".into();
        assert_ne!(
            inputs(vec![]).content_hash().unwrap(),
            F2ValidationInputs::new(
                vec![],
                vec![legacy_override()],
                vec![retyped],
                vec![invariant()]
            )
            .content_hash()
            .unwrap()
        );
        // The context serializes the inputs only when present.
        let value = serde_json::to_value(&a).unwrap();
        assert_eq!(
            value.as_object().unwrap().keys().collect::<Vec<_>>(),
            [
                "analysis_findings",
                "calculation_scope_overrides",
                "decision_tables",
                "invariants"
            ]
        );
    }

    #[test]
    fn f2_inputs_valid_s2_1_supplemental_finding() {
        inputs(vec![domain_finding()])
            .validate_for(&graph())
            .unwrap();
    }

    #[test]
    fn f2_inputs_bad_cardinality_condition_prefix_rejected() {
        let wrong = f2_finding(
            "PLUMB.F2.DOMAIN.RELATION_TYPED",
            TRIGGER,
            &["entity:order"],
            "F2",
        );
        let missing = f2_finding(
            "PLUMB.F2.DOMAIN.RELATION_TYPED",
            "entity:order",
            &["entity:order"],
            "F2",
        );
        assert!(context_error(&graph(), &inputs(vec![wrong])));
        assert!(context_error(&graph(), &inputs(vec![missing])));
    }

    #[test]
    fn f2_inputs_valid_s2_2_supplemental_finding() {
        inputs(vec![state_finding()])
            .validate_for(&graph())
            .unwrap();
    }

    #[test]
    fn f2_inputs_bad_transition_condition_prefix_rejected() {
        let wrong = f2_finding(
            "PLUMB.F2.STATE.TRANSITION_COMPLETE",
            CARDINALITY,
            &["entity:order"],
            "F2",
        );
        assert!(context_error(&graph(), &inputs(vec![wrong])));
    }

    #[test]
    fn f2_inputs_unknown_finding_code_rejected() {
        for code in [
            "PLUMB.F2.CALC.TYPECHECK",
            "PLUMB.F2.NO_UNRESOLVED_SEMANTIC_BLOCKER",
            "PLUMB.F1.REQ.ATOMIC",
        ] {
            let smuggled = f2_finding(code, CARDINALITY, &["entity:order"], "F2");
            assert!(context_error(&graph(), &inputs(vec![smuggled])), "{code}");
        }
    }

    #[test]
    fn f2_inputs_bad_finding_identity_rejected() {
        let g = graph();
        let mut forged = domain_finding();
        forged.semantic_condition_key = format!("{CARDINALITY}:other");
        assert!(context_error(&g, &inputs(vec![forged])));
        let mut retargeted = domain_finding();
        retargeted.payload.affected_refs = vec![id("attr:amount")];
        assert!(context_error(&g, &inputs(vec![retargeted])));
        // Wrong family and missing affected refs fail even with a self-consistent identity.
        let wrong_family = f2_finding(
            "PLUMB.F2.DOMAIN.RELATION_TYPED",
            CARDINALITY,
            &["entity:order"],
            "F1",
        );
        assert!(context_error(&g, &inputs(vec![wrong_family])));
        let ghost = f2_finding(
            "PLUMB.F2.DOMAIN.RELATION_TYPED",
            CARDINALITY,
            &["entity:ghost"],
            "F2",
        );
        assert!(context_error(&g, &inputs(vec![ghost])));
        // A duplicate is not a canonical set.
        let duplicate = F2ValidationInputs {
            analysis_findings: vec![domain_finding(), domain_finding()],
            ..inputs(vec![])
        };
        assert!(context_error(&g, &duplicate));
    }

    #[test]
    fn f2_inputs_legacy_calculation_override_accepted() {
        let g = graph();
        let i = inputs(vec![]);
        i.validate_for(&g).unwrap();
        assert_eq!(
            i.calculation_scope_override(&id("calculation:legacy")),
            Some(&CalculationScope::default())
        );
        // The origin-bearing Calculation has none.
        assert_eq!(i.calculation_scope_override(&id("calculation:total")), None);
    }

    #[test]
    fn f2_inputs_missing_required_legacy_override_rejected() {
        assert!(context_error(
            &graph(),
            &with(vec![], vec![table()], vec![invariant()])
        ));
        // Each legacy Calculation needs its own override.
        let (_, _, tables, invariants) = wide_parts();
        assert!(context_error(
            &wide_graph(),
            &with(vec![legacy_override()], tables, invariants)
        ));
    }

    #[test]
    fn f2_inputs_override_for_origin_bearing_calculation_rejected() {
        let g = graph_with(vec![node(
            "calculation:proposed",
            Proposed,
            NodePayload::Calculation(Calculation {
                name: "proposed".into(),
                expression: "1".into(),
                result_type: "Int".into(),
                unit: None,
                rounding: None,
                calendar_ref: None,
                examples: None,
            }),
        )]);
        for target in [
            "calculation:total",
            "calculation:proposed",
            "attr:amount",
            "calculation:ghost",
        ] {
            let extra = override_for(target, CalculationScope::default());
            assert!(
                context_error(
                    &g,
                    &with(
                        vec![legacy_override(), extra],
                        vec![table()],
                        vec![invariant()]
                    )
                ),
                "{target}"
            );
        }
    }

    #[test]
    fn f2_inputs_decision_table_exact_accepted_coverage() {
        let i = inputs(vec![]);
        // The Proposed dt:draft needs no input.
        i.validate_for(&graph()).unwrap();
        assert_eq!(
            i.decision_table(&id("dt:discount")).unwrap().hit_policy,
            "UNIQUE"
        );
        assert_eq!(i.decision_table(&id("dt:draft")), None);
    }

    #[test]
    fn f2_inputs_decision_table_wrong_hit_policy_rejected() {
        assert!(context_error(
            &graph(),
            &with(
                vec![legacy_override()],
                vec![table_for("dt:discount", "FIRST")],
                vec![invariant()]
            )
        ));
    }

    #[test]
    fn f2_inputs_missing_decision_table_spec_rejected() {
        assert!(context_error(
            &graph(),
            &with(vec![legacy_override()], vec![], vec![invariant()])
        ));
    }

    #[test]
    fn f2_inputs_extra_decision_table_spec_rejected() {
        for extra in [
            table_for("dt:draft", "FIRST"),
            table_for("attr:amount", "UNIQUE"),
            table_for("dt:ghost", "UNIQUE"),
        ] {
            assert!(context_error(
                &graph(),
                &with(
                    vec![legacy_override()],
                    vec![table(), extra],
                    vec![invariant()]
                )
            ));
        }
    }

    #[test]
    fn f2_inputs_invariant_exact_accepted_coverage() {
        let g = graph();
        let i = inputs(vec![]);
        i.validate_for(&g).unwrap();
        assert_eq!(i.invariant_bindings(&id("inv:positive")).unwrap().len(), 1);
        // Empty bindings are a valid scope.
        with(
            vec![legacy_override()],
            vec![table()],
            vec![invariant_for("inv:positive", vec![])],
        )
        .validate_for(&g)
        .unwrap();
        // A missing Invariant input is rejected.
        assert!(context_error(
            &g,
            &with(vec![legacy_override()], vec![table()], vec![])
        ));
    }

    #[test]
    fn f2_inputs_invalid_invariant_ref_rejected() {
        for target in ["attr:amount", "inv:ghost"] {
            assert!(context_error(
                &graph(),
                &with(
                    vec![legacy_override()],
                    vec![table()],
                    vec![invariant(), invariant_for(target, vec![])]
                )
            ));
        }
        // Invariant.scope_ref must resolve to an Accepted node.
        let dangling = graph_with(vec![node(
            "inv:dangling",
            Accepted,
            NodePayload::Invariant(Invariant {
                scope_ref: id("entity:ghost"),
                expression: "1 = 1".into(),
            }),
        )]);
        assert!(context_error(
            &dangling,
            &with(
                vec![legacy_override()],
                vec![table()],
                vec![invariant(), invariant_for("inv:dangling", vec![])]
            )
        ));
    }

    #[test]
    fn f2_inputs_invalid_expression_binding_rejected() {
        for binding in [
            root("attr:text", "t"),
            root("attr:empty-enum", "e"),
            root("attr:amount", "not a symbol"),
            root("attr:ghost", "ghost"),
            field("attr:amount", "attr:hours", "amount"),
        ] {
            assert!(
                context_error(
                    &graph(),
                    &with(
                        vec![legacy_override()],
                        vec![table()],
                        vec![invariant_for("inv:positive", vec![binding.clone()])]
                    )
                ),
                "{binding:?}"
            );
        }
    }

    #[test]
    fn f2_inputs_reversed_caller_order_canonicalizes_identically() {
        let (f, o, t, i) = wide_parts();
        let forward = F2ValidationInputs::new(f.clone(), o.clone(), t.clone(), i.clone());
        let reversed = F2ValidationInputs::new(
            f.into_iter().rev().collect(),
            o.into_iter().rev().collect(),
            t.into_iter().rev().collect(),
            i.into_iter()
                .rev()
                .map(|mut i| {
                    i.bindings.reverse();
                    i
                })
                .collect(),
        );
        assert_eq!(forward, reversed);
        assert_eq!(
            forward.content_hash().unwrap(),
            reversed.content_hash().unwrap()
        );
        let g = wide_graph();
        forward.validate_for(&g).unwrap();
        reversed.validate_for(&g).unwrap();
    }
}
