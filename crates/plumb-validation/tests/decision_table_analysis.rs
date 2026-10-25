//! Hotfix 044 contract tests for the relocated decision-table kernel: overlap and coverage keep
//! their S2.6 semantics, and the S2.6 public path is the same implementation.

mod decision_table_analysis_contract {
    use plumb_validation::decision_table_analysis::*;

    fn bool_col(name: &str) -> DecisionColumn {
        DecisionColumn {
            name: name.into(),
            ty: DecisionType::Bool,
            domain: None,
        }
    }

    fn row(value: Option<bool>) -> DecisionRow {
        DecisionRow {
            inputs: vec![
                value.map_or(DecisionInputCell::Any, |value| DecisionInputCell::Bool {
                    value,
                }),
            ],
            outputs: vec![DecisionOutputCell::Bool { value: true }],
        }
    }

    fn table(policy: &str, rows: Vec<DecisionRow>, default: bool) -> DecisionTableSpec {
        DecisionTableSpec {
            hit_policy: policy.into(),
            inputs: vec![bool_col("flag")],
            outputs: vec![bool_col("out")],
            rows,
            default_output: default.then(|| vec![DecisionOutputCell::Bool { value: false }]),
        }
    }

    #[test]
    fn decision_table_kernel_semantics() {
        assert_eq!(
            (MAX_FINITE_DECISION_POINTS, MAX_COVERAGE_WITNESSES),
            (4096, 32)
        );
        let complete = analyze_decision_table(&table(
            "UNIQUE",
            vec![row(Some(true)), row(Some(false))],
            false,
        ));
        assert!(complete.overlaps.is_empty() && complete.structure.is_empty());
        assert_eq!(complete.coverage, DecisionCoverage::Complete);
        let overlap =
            analyze_decision_table(&table("UNIQUE", vec![row(Some(true)), row(None)], false));
        assert_eq!(
            overlap.illegal_overlaps,
            vec![DecisionOverlap {
                left_row: 0,
                right_row: 1
            }]
        );
        let first =
            analyze_decision_table(&table("FIRST", vec![row(Some(true)), row(None)], false));
        assert!(first.illegal_overlaps.is_empty() && !first.overlaps.is_empty());
        let incomplete = analyze_decision_table(&table("UNIQUE", vec![row(Some(true))], false));
        assert_eq!(
            incomplete.coverage,
            DecisionCoverage::Incomplete {
                witnesses: vec![vec![DecisionPointValue::Bool { value: false }]]
            }
        );
        assert_eq!(
            analyze_decision_table(&table("UNIQUE", vec![], true)).coverage,
            DecisionCoverage::CompleteByDefault
        );
        // Fable #164: overlap-permitting DMN hit policies are analysed (coverage too), and
        // their overlaps are never illegal; an unknown policy stays structurally invalid.
        for policy in OVERLAP_PERMITTING_HIT_POLICIES {
            let a = analyze_decision_table(&table(policy, vec![row(Some(true)), row(None)], false));
            assert!(a.structure.is_empty(), "{policy}");
            assert!(
                a.illegal_overlaps.is_empty() && !a.overlaps.is_empty(),
                "{policy}"
            );
            assert_eq!(a.coverage, DecisionCoverage::Complete, "{policy}");
            assert!(permits_overlap(policy));
        }
        assert!(!permits_overlap("UNIQUE") && !permits_overlap("any"));
        assert_eq!(
            analyze_decision_table(&table("SOMETIMES", vec![], false)).coverage,
            DecisionCoverage::StructureInvalid
        );
    }

    fn decimal_table(scale: u32, output: &str, bound: &str) -> DecisionTableSpec {
        let decimal = |s: &str| s.parse::<rust_decimal::Decimal>().unwrap();
        let column = |name: &str| DecisionColumn {
            name: name.into(),
            ty: DecisionType::Decimal { scale },
            domain: None,
        };
        DecisionTableSpec {
            hit_policy: "UNIQUE".into(),
            inputs: vec![column("amount")],
            outputs: vec![column("rate")],
            rows: vec![DecisionRow {
                inputs: vec![DecisionInputCell::Interval {
                    interval: NumericInterval {
                        lower: Some(NumericBound {
                            value: decimal(bound),
                            inclusive: true,
                        }),
                        upper: None,
                    },
                }],
                outputs: vec![DecisionOutputCell::Decimal {
                    value: decimal(output),
                }],
            }],
            default_output: Some(vec![DecisionOutputCell::Decimal {
                value: decimal(output),
            }]),
        }
    }

    /// Fable #162: Decimal output/default literals and Decimal interval bounds are
    /// scale-checked after normalization (trailing zeros removed).
    #[test]
    fn decision_table_decimal_scale_is_checked_after_normalization() {
        let structure = |spec: &DecisionTableSpec| analyze_decision_table(spec).structure;
        assert_eq!(structure(&decimal_table(1, "1.50", "10.00")), vec![]);
        assert_eq!(structure(&decimal_table(0, "2.000", "3.0")), vec![]);
        assert_eq!(
            structure(&decimal_table(1, "1.55", "10.0")),
            vec![
                DecisionStructureIssue::InvalidOutputCell {
                    row: None,
                    column: 0
                },
                DecisionStructureIssue::InvalidOutputCell {
                    row: Some(0),
                    column: 0
                },
            ]
        );
        assert_eq!(
            structure(&decimal_table(1, "1.5", "10.25")),
            vec![DecisionStructureIssue::InvalidInterval { row: 0, column: 0 }]
        );
    }

    #[test]
    fn decision_table_kernel_is_the_s2_6_kernel() {
        let spec: DecisionTableSpec = plumb_functional::decision_table::DecisionTableSpec {
            hit_policy: "UNIQUE".into(),
            inputs: vec![bool_col("flag")],
            outputs: vec![bool_col("out")],
            rows: vec![row(Some(true))],
            default_output: None,
        };
        let via_functional: DecisionTableAnalysis =
            plumb_functional::decision_table::analyze_decision_table(&spec);
        assert_eq!(via_functional, analyze_decision_table(&spec));
        let source = include_str!("../../plumb-functional/src/decision_table.rs");
        assert!(!source.contains("fn analyze_decision_table") && !source.contains("fn coverage"));
    }
}
