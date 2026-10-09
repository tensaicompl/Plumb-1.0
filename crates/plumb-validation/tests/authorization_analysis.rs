//! Hotfix 044 contract tests for the relocated authorization kernel: hierarchy cycles,
//! effective-role closure and static separation of duty keep their S2.8 semantics, and the S2.8
//! public paths are the same implementation.

mod authorization_analysis_contract {
    use plumb_core::Id;
    use plumb_psg::{SeparationConstraint, SeparationConstraintKind as K};
    use plumb_validation::authorization_analysis::*;

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(Id, Id)> {
        list.iter().map(|(a, b)| (id(a), id(b))).collect()
    }

    fn constraint(c: &str, kind: K, roles: &[&str]) -> (Id, SeparationConstraint) {
        (
            id(c),
            SeparationConstraint {
                constraint_kind: kind,
                role_refs: roles.iter().map(|r| id(r)).collect(),
            },
        )
    }

    #[test]
    fn authorization_kernel_semantics() {
        assert_eq!(
            role_hierarchy_cycles(&pairs(&[("r:c", "r:c"), ("r:b", "r:a"), ("r:a", "r:b")])),
            vec![vec![id("r:a"), id("r:b")], vec![id("r:c")]]
        );
        assert!(role_hierarchy_cycles(&pairs(&[("r:a", "r:b"), ("r:b", "r:c")])).is_empty());
        let facts = AuthorizationFacts {
            assignments: pairs(&[("p:1", "r:a"), ("p:2", "r:c")]),
            inheritances: pairs(&[("r:a", "r:b")]),
            constraints: vec![
                constraint("sc:static", K::StaticSeparationOfDuty, &["r:a", "r:b"]),
                constraint("sc:dynamic", K::DynamicSeparationOfDuty, &["r:a", "r:b"]),
            ],
        };
        assert_eq!(
            effective_security_roles(&facts, &id("p:1")),
            vec![id("r:a"), id("r:b")]
        );
        let analysis = separation_analysis(&facts);
        assert_eq!(
            analysis.static_sod_violations,
            vec![StaticSodViolation {
                constraint_ref: id("sc:static"),
                subject_ref: id("p:1"),
                conflicting_role_refs: vec![id("r:a"), id("r:b")]
            }]
        );
        assert_eq!(analysis.not_evaluated, vec![id("sc:dynamic")]);
    }

    #[test]
    fn authorization_kernel_is_the_s2_8_kernel() {
        let facts: AuthorizationFacts = plumb_functional::authorization::AuthorizationFacts {
            assignments: pairs(&[("p:1", "r:a"), ("p:1", "r:b")]),
            inheritances: vec![],
            constraints: vec![constraint(
                "sc:1",
                K::StaticSeparationOfDuty,
                &["r:a", "r:b"],
            )],
        };
        assert_eq!(
            plumb_functional::authorization::separation_analysis(&facts),
            separation_analysis(&facts)
        );
        let source = include_str!("../../plumb-functional/src/authorization.rs");
        for algorithm in [
            "fn tarjan",
            "fn role_hierarchy_cycles",
            "fn effective_security_roles",
            "fn separation_analysis",
            "struct TarjanState",
        ] {
            assert!(
                !source.contains(algorithm),
                "{algorithm} duplicated in plumb-functional"
            );
        }
        let calculation = include_str!("../../plumb-functional/src/calculation.rs");
        for algorithm in [
            "fn qualify(",
            "fn qualify_calculation",
            "fn strongly_connected",
            "fn used_bindings",
            "fn validate_bindings",
            "fn result_compatibility",
        ] {
            assert!(
                !calculation.contains(algorithm),
                "{algorithm} duplicated in plumb-functional"
            );
        }
    }
}
