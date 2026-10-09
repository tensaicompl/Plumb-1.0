//! Hotfix 044 contract tests for the relocated calculation kernel: qualification under explicit
//! bindings and the reserved calendar, origin replay and legacy overrides, dependencies and
//! cycles, and identity with the S2.6 public paths. All graphs are synthetic.

mod calculation_analysis_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{Id, Timestamp};
    use plumb_expr::{Ty, Unit};
    use plumb_psg::{
        Attribute, AuditMeta, Calculation, Calendar, Edge, ElementStatus, Entity, Graph, Node,
        NodePayload, RelationKind, RelationProperties,
    };
    use plumb_validation::calculation_analysis::*;
    use plumb_validation::expression_scope::{ExpressionBindingExposure, ExpressionScopeBinding};
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

    fn root(node_ref: &str, symbol: &str) -> CalculationScopeBinding {
        ExpressionScopeBinding {
            node_ref: id(node_ref),
            symbol: symbol.into(),
            exposure: ExpressionBindingExposure::Root,
        }
    }

    fn calc(
        node_id: &str,
        expression: &str,
        result_type: &str,
        unit: Option<&str>,
        calendar: Option<&str>,
        origin: Option<Vec<CalculationScopeBinding>>,
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
                json!({"name_range": range, "expression_evidence": [range], "used_bindings": bindings}),
            );
        }
        n
    }

    fn graph(extra: Vec<Node>) -> Graph {
        let mut nodes = vec![
            node(
                "entity:req",
                Accepted,
                NodePayload::Entity(Entity {
                    name: "Req".into(),
                    description: None,
                    aggregate_root: None,
                }),
            ),
            node(
                "attr:start",
                Accepted,
                NodePayload::Attribute(Attribute {
                    name: "start".into(),
                    value_type: "Date".into(),
                    nullable: false,
                    unit: None,
                    precision: None,
                    enum_values: None,
                    data_classification: None,
                }),
            ),
            node(
                "attr:fraction",
                Accepted,
                NodePayload::Attribute(Attribute {
                    name: "fraction".into(),
                    value_type: "Decimal(2)".into(),
                    nullable: false,
                    unit: None,
                    precision: None,
                    enum_values: None,
                    data_classification: None,
                }),
            ),
            node(
                "cal:office",
                Accepted,
                NodePayload::Calendar(Calendar {
                    time_zone: "Europe/Warsaw".into(),
                    week_pattern: json!([]),
                    region: None,
                    holiday_source: None,
                }),
            ),
            node(
                "cal:draft",
                Proposed,
                NodePayload::Calendar(Calendar {
                    time_zone: "UTC".into(),
                    week_pattern: json!([]),
                    region: None,
                    holiday_source: None,
                }),
            ),
        ];
        nodes.extend(extra);
        let edges = ["attr:start", "attr:fraction"]
            .iter()
            .map(|a| Edge {
                id: id(&format!("rel:own-{}", &a[5..])),
                revision: 1,
                status: Accepted,
                kind: RelationKind::HasAttribute,
                from: id("entity:req"),
                to: id(a),
                properties: RelationProperties::None,
                evidence: Vec::new(),
                derivations: Vec::new(),
                standards: Vec::new(),
                audit: meta(),
            })
            .collect();
        Graph::new(
            id("project:pilot"),
            id("profile:plumb-software-2026.1"),
            nodes,
            edges,
        )
        .unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn draft<'a>(
        expression: &'a str,
        result_type: &'a str,
        unit: Option<&'a str>,
        calendar: Option<&'a Id>,
    ) -> CalculationDraft<'a> {
        CalculationDraft {
            expression,
            result_type,
            unit,
            rounding: None,
            calendar_ref: calendar,
        }
    }

    #[test]
    fn calculation_kernel_qualification() {
        let g = graph(vec![]);
        let scope = || {
            validate_calculation_bindings(
                &g,
                &[
                    root("attr:start", "start"),
                    root("attr:fraction", "fraction"),
                ],
            )
        };
        let cal = id("cal:office");
        let q = qualify_calculation(
            &g,
            &draft(
                "working_days([start, start], calendar, true) * fraction",
                "Decimal(2)",
                Some("working_day"),
                Some(&cal),
            ),
            scope(),
        );
        assert!(q.is_qualified(), "{:?}", q.issues);
        assert!(q.requires_calendar);
        assert_eq!(
            q.actual_ty,
            Some(Ty::Quantity(Unit::new("working_day").unwrap()))
        );
        assert_eq!(
            q.used_bindings,
            vec![
                root("attr:fraction", "fraction"),
                root("attr:start", "start")
            ]
        );
        let prepared = q.prepared.unwrap();
        assert_eq!(prepared.type_env.calendar_ref_type(), Some(&cal));
        let missing = qualify_calculation(
            &g,
            &draft(
                "working_days([start, start], calendar, true)",
                "Decimal(0)",
                Some("working_day"),
                None,
            ),
            scope(),
        );
        assert!(missing.issues.contains(&CalculationIssue::CalendarRequired));
        let draft_cal = id("cal:draft");
        let unresolved = qualify_calculation(
            &g,
            &draft("fraction", "Decimal(2)", None, Some(&draft_cal)),
            scope(),
        );
        assert!(unresolved
            .issues
            .contains(&CalculationIssue::CalendarUnresolved {
                calendar_ref: draft_cal.clone()
            }));
        let undefined = qualify_calculation(&g, &draft("ghost + 1", "Int", None, None), scope());
        assert!(undefined
            .issues
            .contains(&CalculationIssue::UndefinedInput {
                identifier: "ghost".into()
            }));
        assert_eq!(
            qualify_calculation(&g, &draft("fraction +", "Decimal(2)", None, None), scope())
                .issues
                .len(),
            1
        );
        assert_eq!(
            validate_calculation_bindings(&g, &[root("attr:start", "calendar")]).unwrap_err(),
            CalculationIssue::ReservedSymbol {
                node_ref: id("attr:start")
            }
        );
        assert!(accepted_calendar(&g, &cal) && !accepted_calendar(&g, &draft_cal));
        assert!(matches!(
            validate_calculation_scope(
                &g,
                &CalculationScope {
                    bindings: vec![],
                    calendar_refs: vec![draft_cal]
                }
            ),
            Err(CalculationIssue::CalendarUnresolved { .. })
        ));
    }

    #[test]
    fn calculation_kernel_accepted_and_cycles() {
        let g = graph(vec![
            calc(
                "calculation:double",
                "fraction * 2",
                "Decimal(2)",
                None,
                None,
                Some(vec![root("attr:fraction", "fraction")]),
            ),
            calc(
                "calculation:legacy",
                "fraction * 3",
                "Decimal(2)",
                None,
                None,
                None,
            ),
            calc(
                "calculation:a",
                "b + 1",
                "Int",
                None,
                None,
                Some(vec![root("calculation:b", "b")]),
            ),
            calc(
                "calculation:b",
                "a + 1",
                "Int",
                None,
                None,
                Some(vec![root("calculation:a", "a")]),
            ),
        ]);
        let replay = qualify_accepted_calculation(&g, &id("calculation:double"), None).unwrap();
        assert!(replay.from_origin && replay.qualification.is_qualified());
        assert!(calculation_origin_of(g.node(&id("calculation:double")).unwrap()).is_some());
        assert_eq!(
            qualify_accepted_calculation(&g, &id("calculation:legacy"), None)
                .unwrap()
                .qualification
                .issues,
            vec![CalculationIssue::ScopeUnavailable {
                calculation_ref: id("calculation:legacy")
            }]
        );
        let scope = CalculationScope {
            bindings: vec![root("attr:fraction", "fraction")],
            calendar_refs: vec![],
        };
        assert!(
            qualify_accepted_calculation(&g, &id("calculation:legacy"), Some(&scope))
                .unwrap()
                .qualification
                .is_qualified()
        );
        assert!(matches!(
            qualify_accepted_calculation(&g, &id("calculation:double"), Some(&scope))
                .unwrap()
                .qualification
                .issues
                .as_slice(),
            [CalculationIssue::UnexpectedScopeOverride { .. }]
        ));
        assert!(matches!(
            qualify_accepted_calculation(&g, &id("attr:start"), None),
            Err(CalculationAnalysisError::InvalidInput { .. })
        ));
        let mut overrides = BTreeMap::new();
        overrides.insert(id("calculation:legacy"), scope);
        let cycles = analyze_calculation_cycles(&g, &overrides).unwrap();
        assert_eq!(
            cycles.cycles,
            vec![vec![id("calculation:a"), id("calculation:b")]]
        );
        assert!(cycles.incomplete.is_empty());
        let a = cycles
            .qualifications
            .iter()
            .find(|q| q.calculation_ref.as_str() == "calculation:a")
            .unwrap();
        assert!(a.in_cycle);
        assert_eq!(a.qualification.dependencies, vec![id("calculation:b")]);
        let no_override = analyze_calculation_cycles(&g, &BTreeMap::new()).unwrap();
        assert_eq!(no_override.incomplete, vec![id("calculation:legacy")]);
    }

    #[test]
    fn calculation_kernel_is_the_s2_6_kernel() {
        // The S2.6 public paths resolve to this kernel's types and constants.
        let issue: CalculationIssue =
            plumb_functional::calculation::CalculationIssue::CalendarRequired;
        assert_eq!(issue, CalculationIssue::CalendarRequired);
        let scope: CalculationScope = plumb_functional::calculation::CalculationScope::default();
        assert_eq!(scope, CalculationScope::default());
        assert_eq!(
            plumb_functional::CALCULATION_CALENDAR_SYMBOL,
            CALCULATION_CALENDAR_SYMBOL
        );
        assert_eq!(
            plumb_functional::CALCULATION_ORIGIN_EXTENSION,
            CALCULATION_ORIGIN_EXTENSION
        );
        let g = graph(vec![calc(
            "calculation:double",
            "fraction * 2",
            "Decimal(2)",
            None,
            None,
            Some(vec![root("attr:fraction", "fraction")]),
        )]);
        let via_functional =
            plumb_functional::qualify_accepted_calculation(&g, &id("calculation:double"), None)
                .unwrap();
        let via_kernel = qualify_accepted_calculation(&g, &id("calculation:double"), None).unwrap();
        assert_eq!(format!("{via_functional:?}"), format!("{via_kernel:?}"));
    }
}
