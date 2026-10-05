//! S2.6 contract tests for typed decision-table analysis: the Bool/Enum/Int/Decimal pilot
//! subset, UNIQUE and FIRST overlap semantics, structural validation and deterministic
//! coverage where analyzable.
//!
//! Every table here is a synthetic generic-engine fixture; no HR prose rule is turned into a
//! decision table.

mod decision_table_contract {
    use std::str::FromStr;

    use plumb_functional::decision_table::*;
    use rust_decimal::Decimal;

    // ------------------------------------------------------------------ builders

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn column(name: &str, ty: DecisionType) -> DecisionColumn {
        DecisionColumn {
            name: name.into(),
            ty,
            domain: None,
        }
    }

    fn bool_col(name: &str) -> DecisionColumn {
        column(name, DecisionType::Bool)
    }

    fn enum_col(name: &str, members: &[&str]) -> DecisionColumn {
        column(
            name,
            DecisionType::Enum {
                members: members.iter().map(|m| (*m).to_owned()).collect(),
            },
        )
    }

    fn int_col(name: &str, domain: Option<(&str, &str)>) -> DecisionColumn {
        DecisionColumn {
            domain: domain.map(|(l, u)| NumericDomain {
                lower: dec(l),
                upper: dec(u),
            }),
            ..column(name, DecisionType::Int)
        }
    }

    fn dec_col(name: &str, scale: u32, domain: Option<(&str, &str)>) -> DecisionColumn {
        DecisionColumn {
            domain: domain.map(|(l, u)| NumericDomain {
                lower: dec(l),
                upper: dec(u),
            }),
            ..column(name, DecisionType::Decimal { scale })
        }
    }

    fn any() -> DecisionInputCell {
        DecisionInputCell::Any
    }

    fn b(value: bool) -> DecisionInputCell {
        DecisionInputCell::Bool { value }
    }

    fn set(members: &[&str]) -> DecisionInputCell {
        DecisionInputCell::EnumSet {
            members: members.iter().map(|m| (*m).to_owned()).collect(),
        }
    }

    fn bnd(value: &str, inclusive: bool) -> Option<NumericBound> {
        Some(NumericBound {
            value: dec(value),
            inclusive,
        })
    }

    fn iv(lower: Option<NumericBound>, upper: Option<NumericBound>) -> NumericInterval {
        NumericInterval { lower, upper }
    }

    fn cell(lower: Option<NumericBound>, upper: Option<NumericBound>) -> DecisionInputCell {
        DecisionInputCell::Interval {
            interval: iv(lower, upper),
        }
    }

    fn out(value: bool) -> DecisionOutputCell {
        DecisionOutputCell::Bool { value }
    }

    fn row(inputs: Vec<DecisionInputCell>) -> DecisionRow {
        DecisionRow {
            inputs,
            outputs: vec![out(true)],
        }
    }

    fn table(
        hit_policy: &str,
        inputs: Vec<DecisionColumn>,
        rows: Vec<DecisionRow>,
    ) -> DecisionTableSpec {
        DecisionTableSpec {
            hit_policy: hit_policy.into(),
            inputs,
            outputs: vec![bool_col("result")],
            rows,
            default_output: None,
        }
    }

    fn pb(value: bool) -> DecisionPointValue {
        DecisionPointValue::Bool { value }
    }

    fn pe(member: &str) -> DecisionPointValue {
        DecisionPointValue::Enum {
            member: member.into(),
        }
    }

    fn overlap(left_row: usize, right_row: usize) -> DecisionOverlap {
        DecisionOverlap {
            left_row,
            right_row,
        }
    }

    fn analyze(spec: &DecisionTableSpec) -> DecisionTableAnalysis {
        analyze_decision_table(spec)
    }

    fn structure(spec: &DecisionTableSpec) -> Vec<DecisionStructureIssue> {
        let analysis = analyze(spec);
        assert!(analysis.overlaps.is_empty() && analysis.illegal_overlaps.is_empty());
        analysis.structure
    }

    fn not_analyzable(reason: NotAnalyzableReason) -> DecisionCoverage {
        DecisionCoverage::NotAnalyzable { reason }
    }

    // ------------------------------------------------------------------ finite coverage

    #[test]
    fn decision_table_bool_and_enum_coverage() {
        assert_eq!(MAX_FINITE_DECISION_POINTS, 4096);
        assert_eq!(MAX_COVERAGE_WITNESSES, 32);
        let complete = table(
            "UNIQUE",
            vec![bool_col("flag")],
            vec![row(vec![b(false)]), row(vec![b(true)])],
        );
        let analysis = analyze(&complete);
        assert!(analysis.structure.is_empty());
        assert_eq!(analysis.coverage, DecisionCoverage::Complete);
        let incomplete = table("UNIQUE", vec![bool_col("flag")], vec![row(vec![b(true)])]);
        assert_eq!(
            analyze(&incomplete).coverage,
            DecisionCoverage::Incomplete {
                witnesses: vec![vec![pb(false)]]
            }
        );

        let members = ["Alpha", "Beta", "Gamma"];
        let complete = table(
            "UNIQUE",
            vec![enum_col("kind", &members)],
            vec![
                row(vec![set(&["Alpha", "Gamma"])]),
                row(vec![set(&["Beta"])]),
            ],
        );
        assert_eq!(analyze(&complete).coverage, DecisionCoverage::Complete);
        let incomplete = table(
            "UNIQUE",
            vec![enum_col("kind", &members)],
            vec![row(vec![set(&["Beta"])])],
        );
        assert_eq!(
            analyze(&incomplete).coverage,
            DecisionCoverage::Incomplete {
                witnesses: vec![vec![pe("Alpha")], vec![pe("Gamma")]]
            }
        );
    }

    #[test]
    fn decision_table_cartesian_coverage() {
        let inputs = || vec![bool_col("flag"), enum_col("kind", &["Alpha", "Beta"])];
        let complete = table(
            "UNIQUE",
            inputs(),
            vec![
                row(vec![b(true), any()]),
                row(vec![b(false), set(&["Alpha"])]),
                row(vec![b(false), set(&["Beta"])]),
            ],
        );
        let analysis = analyze(&complete);
        assert!(analysis.overlaps.is_empty());
        assert_eq!(analysis.coverage, DecisionCoverage::Complete);
        let incomplete = table("UNIQUE", inputs(), vec![row(vec![b(true), set(&["Beta"])])]);
        // Canonical order: false < true, enum members in declared (sorted) order, last axis fastest.
        assert_eq!(
            analyze(&incomplete).coverage,
            DecisionCoverage::Incomplete {
                witnesses: vec![
                    vec![pb(false), pe("Alpha")],
                    vec![pb(false), pe("Beta")],
                    vec![pb(true), pe("Alpha")]
                ]
            }
        );
    }

    #[test]
    fn decision_table_finite_domain_limit_and_witness_cap() {
        // 2^12 = 4096 points is analyzed; 2^13 = 8192 is not.
        let at_limit = table(
            "FIRST",
            (0..12).map(|i| bool_col(&format!("f{i}"))).collect(),
            vec![],
        );
        let DecisionCoverage::Incomplete { witnesses } = analyze(&at_limit).coverage else {
            panic!()
        };
        assert_eq!(witnesses.len(), MAX_COVERAGE_WITNESSES);
        assert_eq!(witnesses[0], vec![pb(false); 12]);
        let mut second = vec![pb(false); 12];
        second[11] = pb(true);
        assert_eq!(witnesses[1], second);
        let over = table(
            "FIRST",
            (0..13).map(|i| bool_col(&format!("f{i}"))).collect(),
            vec![],
        );
        assert_eq!(
            analyze(&over).coverage,
            not_analyzable(NotAnalyzableReason::FiniteDomainTooLarge)
        );
        // 65 * 65 = 4225 > 4096.
        let names: Vec<String> = (0..65).map(|i| format!("M{i:02}")).collect();
        let members: Vec<&str> = names.iter().map(String::as_str).collect();
        let wide = table(
            "FIRST",
            vec![enum_col("a", &members), enum_col("b", &members)],
            vec![],
        );
        assert_eq!(
            analyze(&wide).coverage,
            not_analyzable(NotAnalyzableReason::FiniteDomainTooLarge)
        );
    }

    // ------------------------------------------------------------------ overlaps

    #[test]
    fn decision_table_hit_policy_overlaps() {
        let inputs = || {
            vec![
                bool_col("flag"),
                enum_col("kind", &["Alpha", "Beta", "Gamma"]),
            ]
        };
        let rows = || {
            vec![
                row(vec![any(), set(&["Alpha", "Beta"])]),
                row(vec![b(true), set(&["Beta", "Gamma"])]),
                row(vec![b(false), set(&["Gamma"])]),
                row(vec![b(true), set(&["Alpha"])]),
            ]
        };
        let unique = analyze(&table("UNIQUE", inputs(), rows()));
        assert_eq!(unique.overlaps, vec![overlap(0, 1), overlap(0, 3)]);
        assert_eq!(unique.illegal_overlaps, unique.overlaps);
        let first = analyze(&table("FIRST", inputs(), rows()));
        assert_eq!(first.overlaps, vec![overlap(0, 1), overlap(0, 3)]);
        assert!(first.illegal_overlaps.is_empty());
        assert_eq!(first.coverage, unique.coverage);
        let disjoint = analyze(&table(
            "UNIQUE",
            inputs(),
            vec![row(vec![b(true), any()]), row(vec![b(false), any()])],
        ));
        assert!(disjoint.overlaps.is_empty() && disjoint.illegal_overlaps.is_empty());
        assert_eq!(disjoint.coverage, DecisionCoverage::Complete);
    }

    #[test]
    fn decision_table_numeric_overlaps() {
        // Int intervals are discrete: (1, 2) contains no integer, so it overlaps nothing.
        let int = || vec![int_col("n", Some(("0", "10")))];
        let a = analyze(&table(
            "UNIQUE",
            int(),
            vec![
                row(vec![cell(bnd("0", true), bnd("5", false))]),
                row(vec![cell(bnd("4", false), bnd("5", true))]),
                row(vec![cell(bnd("5", true), bnd("10", true))]),
            ],
        ));
        // [0,5) = 0..4, (4,5] = {5}, [5,10] = 5..10: only rows 1 and 2 share 5.
        assert_eq!(a.overlaps, vec![overlap(1, 2)]);
        // Decimal intervals are continuous: [0,5) and (4.99,5] share (4.99, 5).
        let decimal = || vec![dec_col("x", 2, Some(("0", "10")))];
        let d = analyze(&table(
            "UNIQUE",
            decimal(),
            vec![
                row(vec![cell(bnd("0", true), bnd("5", false))]),
                row(vec![cell(bnd("4.99", false), bnd("5", true))]),
                row(vec![cell(bnd("5", false), bnd("10", true))]),
            ],
        ));
        assert_eq!(d.overlaps, vec![overlap(0, 1)]);
        assert_eq!(d.illegal_overlaps, vec![overlap(0, 1)]);
        // Open/closed boundaries at the same value.
        let touch = |left_inclusive: bool, right_inclusive: bool| {
            analyze(&table(
                "UNIQUE",
                decimal(),
                vec![
                    row(vec![cell(bnd("0", true), bnd("5", left_inclusive))]),
                    row(vec![cell(bnd("5", right_inclusive), bnd("10", true))]),
                ],
            ))
            .overlaps
        };
        assert_eq!(touch(true, true), vec![overlap(0, 1)]);
        assert!(touch(true, false).is_empty());
        assert!(touch(false, true).is_empty());
        assert!(touch(false, false).is_empty());
    }

    // ------------------------------------------------------------------ numeric coverage

    #[test]
    fn decision_table_int_coverage() {
        let int = || vec![int_col("n", Some(("0", "10")))];
        let complete = table(
            "UNIQUE",
            int(),
            vec![
                row(vec![cell(bnd("0", true), bnd("5", false))]),
                row(vec![cell(bnd("5", true), None)]),
            ],
        );
        assert_eq!(analyze(&complete).coverage, DecisionCoverage::Complete);
        // An open Int interval (4, 6) is exactly {5}.
        let discrete = table(
            "UNIQUE",
            int(),
            vec![
                row(vec![cell(None, bnd("4", true))]),
                row(vec![cell(bnd("4", false), bnd("6", false))]),
                row(vec![cell(bnd("6", true), None)]),
            ],
        );
        let analysis = analyze(&discrete);
        assert!(analysis.overlaps.is_empty());
        assert_eq!(analysis.coverage, DecisionCoverage::Complete);
        let gap = table(
            "UNIQUE",
            int(),
            vec![
                row(vec![cell(bnd("0", true), bnd("3", true))]),
                row(vec![cell(bnd("5", false), bnd("8", true))]),
            ],
        );
        assert_eq!(
            analyze(&gap).coverage,
            DecisionCoverage::IncompleteNumeric {
                gaps: vec![
                    iv(bnd("4", true), bnd("5", true)),
                    iv(bnd("9", true), bnd("10", true))
                ]
            }
        );
    }

    #[test]
    fn decision_table_decimal_coverage() {
        let decimal = || vec![dec_col("x", 2, Some(("0", "10")))];
        let complete = table(
            "UNIQUE",
            decimal(),
            vec![
                row(vec![cell(None, bnd("2.5", false))]),
                row(vec![cell(bnd("2.5", true), None)]),
            ],
        );
        assert_eq!(analyze(&complete).coverage, DecisionCoverage::Complete);
        // Both rows exclude 2.5: the single point is the gap.
        let point_gap = table(
            "UNIQUE",
            decimal(),
            vec![
                row(vec![cell(None, bnd("2.5", false))]),
                row(vec![cell(bnd("2.5", false), None)]),
            ],
        );
        assert_eq!(
            analyze(&point_gap).coverage,
            DecisionCoverage::IncompleteNumeric {
                gaps: vec![iv(bnd("2.5", true), bnd("2.5", true))]
            }
        );
        let gaps = table(
            "UNIQUE",
            decimal(),
            vec![
                row(vec![cell(bnd("0", false), bnd("3", true))]),
                row(vec![cell(bnd("4", true), bnd("9.5", false))]),
            ],
        );
        assert_eq!(
            analyze(&gaps).coverage,
            DecisionCoverage::IncompleteNumeric {
                gaps: vec![
                    iv(bnd("0", true), bnd("0", true)),
                    iv(bnd("3", false), bnd("4", false)),
                    iv(bnd("9.5", true), bnd("10", true)),
                ]
            }
        );
    }

    // ------------------------------------------------------------------ structure

    #[test]
    fn decision_table_cell_validation() {
        let kind = || vec![enum_col("kind", &["Alpha", "Beta"])];
        assert_eq!(
            structure(&table("UNIQUE", kind(), vec![row(vec![set(&["Delta"])])])),
            vec![DecisionStructureIssue::UnknownEnumMember {
                row: 0,
                column: 0,
                member: "Delta".into()
            }]
        );
        assert_eq!(
            structure(&table("UNIQUE", kind(), vec![row(vec![set(&[])])])),
            vec![DecisionStructureIssue::EmptyEnumSet { row: 0, column: 0 }]
        );
        assert_eq!(
            analyze(&table("UNIQUE", kind(), vec![row(vec![set(&[])])])).coverage,
            DecisionCoverage::StructureInvalid
        );
        let int = || vec![int_col("n", Some(("0", "10")))];
        assert_eq!(
            structure(&table(
                "UNIQUE",
                int(),
                vec![row(vec![cell(bnd("5", true), bnd("4", true))])]
            )),
            vec![DecisionStructureIssue::InvalidInterval { row: 0, column: 0 }]
        );
        for (l, u) in [(true, false), (false, true), (false, false)] {
            assert_eq!(
                structure(&table(
                    "UNIQUE",
                    int(),
                    vec![row(vec![cell(bnd("5", l), bnd("5", u))])]
                )),
                vec![DecisionStructureIssue::EmptyInterval { row: 0, column: 0 }]
            );
        }
        // A closed equal interval is the single point and is valid.
        assert!(structure(&table(
            "UNIQUE",
            int(),
            vec![row(vec![cell(bnd("5", true), bnd("5", true))])]
        ))
        .is_empty());
        assert_eq!(
            structure(&table(
                "UNIQUE",
                vec![bool_col("flag")],
                vec![row(vec![set(&["Alpha"])])]
            )),
            vec![DecisionStructureIssue::InvalidCell { row: 0, column: 0 }]
        );
        assert_eq!(
            structure(&table(
                "UNIQUE",
                vec![enum_col("kind", &["Beta", "Alpha"])],
                vec![]
            )),
            vec![DecisionStructureIssue::InvalidEnumDomain { column: 0 }]
        );
    }

    #[test]
    fn decision_table_default_empty_rows_and_arity() {
        let inputs = || vec![bool_col("flag")];
        let empty = table("UNIQUE", inputs(), vec![]);
        let analysis = analyze(&empty);
        assert!(analysis.structure.is_empty() && analysis.overlaps.is_empty());
        assert_eq!(
            analysis.coverage,
            DecisionCoverage::Incomplete {
                witnesses: vec![vec![pb(false)], vec![pb(true)]]
            }
        );
        let defaulted = DecisionTableSpec {
            default_output: Some(vec![out(false)]),
            ..empty.clone()
        };
        assert_eq!(
            analyze(&defaulted).coverage,
            DecisionCoverage::CompleteByDefault
        );
        let bad_default = DecisionTableSpec {
            default_output: Some(vec![]),
            ..empty.clone()
        };
        assert_eq!(
            structure(&bad_default),
            vec![DecisionStructureIssue::DefaultArityMismatch]
        );
        let input_arity = table("UNIQUE", inputs(), vec![row(vec![b(true), b(false)])]);
        assert_eq!(
            structure(&input_arity),
            vec![DecisionStructureIssue::RowArityMismatch { row: 0 }]
        );
        let output_arity = table(
            "UNIQUE",
            inputs(),
            vec![DecisionRow {
                inputs: vec![b(true)],
                outputs: vec![],
            }],
        );
        assert_eq!(
            structure(&output_arity),
            vec![DecisionStructureIssue::RowArityMismatch { row: 0 }]
        );
        let wrong_output = table(
            "UNIQUE",
            inputs(),
            vec![DecisionRow {
                inputs: vec![b(true)],
                outputs: vec![DecisionOutputCell::Int { value: 1 }],
            }],
        );
        assert_eq!(
            structure(&wrong_output),
            vec![DecisionStructureIssue::InvalidOutputCell {
                row: Some(0),
                column: 0
            }]
        );
    }

    #[test]
    fn decision_table_unsupported_and_not_analyzable() {
        for policy in ["unique", "ANY", "COLLECT", "PRIORITY", ""] {
            let spec = table(policy, vec![bool_col("flag")], vec![row(vec![b(true)])]);
            assert_eq!(
                structure(&spec),
                vec![DecisionStructureIssue::UnsupportedHitPolicy {
                    hit_policy: policy.into()
                }]
            );
            assert_eq!(analyze(&spec).coverage, DecisionCoverage::StructureInvalid);
        }
        assert_eq!(HitPolicy::parse("UNIQUE"), Some(HitPolicy::Unique));
        assert_eq!(HitPolicy::parse("FIRST"), Some(HitPolicy::First));
        let unsupported = table(
            "UNIQUE",
            vec![column(
                "when",
                DecisionType::Unsupported {
                    name: "Date".into(),
                },
            )],
            vec![row(vec![any()])],
        );
        let analysis = analyze(&unsupported);
        assert_eq!(
            analysis.structure,
            vec![DecisionStructureIssue::UnsupportedInputDomain { column: 0 }]
        );
        assert_eq!(
            analysis.coverage,
            not_analyzable(NotAnalyzableReason::UnsupportedDomain)
        );
        let multi = table(
            "UNIQUE",
            vec![
                int_col("n", Some(("0", "1"))),
                dec_col("x", 1, Some(("0", "1"))),
            ],
            vec![row(vec![any(), any()])],
        );
        assert_eq!(
            analyze(&multi).coverage,
            not_analyzable(NotAnalyzableReason::MultiDimensionalNumericDomain)
        );
        let mixed = table(
            "UNIQUE",
            vec![bool_col("flag"), int_col("n", Some(("0", "1")))],
            vec![row(vec![any(), any()])],
        );
        assert_eq!(
            analyze(&mixed).coverage,
            not_analyzable(NotAnalyzableReason::MixedNumericDomain)
        );
        let unbounded = table("UNIQUE", vec![int_col("n", None)], vec![row(vec![any()])]);
        assert_eq!(
            analyze(&unbounded).coverage,
            not_analyzable(NotAnalyzableReason::UnboundedNumericDomain)
        );
    }

    // ------------------------------------------------------------------ determinism

    #[test]
    fn decision_table_deterministic_ordering() {
        let inputs = vec![enum_col("kind", &["Alpha", "Beta", "Gamma", "Omega"])];
        let rows = vec![
            row(vec![set(&["Gamma"])]),
            row(vec![any()]),
            row(vec![set(&["Alpha", "Gamma"])]),
            row(vec![set(&["Alpha"])]),
        ];
        let spec = table("UNIQUE", inputs, rows);
        let first = analyze(&spec);
        assert_eq!(
            first.overlaps,
            vec![
                overlap(0, 1),
                overlap(0, 2),
                overlap(1, 2),
                overlap(1, 3),
                overlap(2, 3)
            ]
        );
        for _ in 0..8 {
            assert_eq!(analyze(&spec), first);
        }
        let gaps = table(
            "FIRST",
            vec![
                enum_col("kind", &["Alpha", "Beta", "Gamma", "Omega"]),
                bool_col("flag"),
            ],
            vec![row(vec![set(&["Beta"]), b(true)])],
        );
        let DecisionCoverage::Incomplete { witnesses } = analyze(&gaps).coverage else {
            panic!()
        };
        assert_eq!(witnesses.len(), 7);
        assert_eq!(witnesses[0], vec![pe("Alpha"), pb(false)]);
        assert_eq!(witnesses[2], vec![pe("Beta"), pb(false)]);
        assert_eq!(witnesses[6], vec![pe("Omega"), pb(true)]);
        let json = serde_json::to_value(analyze(&gaps)).unwrap();
        assert_eq!(json["coverage"]["coverage"], "incomplete");
        assert_eq!(
            json["coverage"]["witnesses"][0][0],
            serde_json::json!({"kind": "enum", "member": "Alpha"})
        );
    }

    #[test]
    fn decision_table_source_guard() {
        let source = include_str!("../src/decision_table.rs");
        for forbidden in [
            "std::fs",
            "reqwest",
            "rand::",
            "f64",
            "f32",
            "unsafe",
            "Proposal",
            "SemanticPatch",
            "Graph",
            "NodePayload",
            "PLUMB.F2",
            "DMN.F2",
            "GeneratedFinding",
            "G_",
            "Instant::now",
            "Utc::now",
            "AnnualLeaveEligibility",
            "ManagerDecisionAuthorization",
            "OverlapRule",
        ] {
            assert!(
                !source.contains(forbidden),
                "decision_table.rs contains {forbidden}"
            );
        }
        assert!(!std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../prompts/s2-decision-table.md"
        ))
        .exists());
        assert!(!std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/inference/s2-decision-table.schema.json"
        ))
        .exists());
    }
}
