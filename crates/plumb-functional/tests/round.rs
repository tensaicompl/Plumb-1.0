//! S3.2 contract tests for deterministic question-round composition (Hotfix 046).
//!
//! Every test lives in `round_contract` (`cargo test -p plumb-functional --test round`). Graphs,
//! Findings and Questions are synthetic current-v3 material; the HR adapter reuses the S3.1
//! engine with the real immutable `fixtures/hr-leave/stakeholders.yaml` and reads
//! `question_round_size` from `fixtures/hr-leave/profile.yaml`. Golden round IDs were computed
//! independently with Python `hashlib` over RFC 8785 JSON, never with the helper under test.

mod round_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, Id, StageId, Timestamp};
    use plumb_functional::question::{
        generate_questions, QuestionAudit, QuestionGenerationInput, StakeholderRoutingConfig,
    };
    use plumb_functional::round::*;
    use plumb_patch::{
        apply_patch, AcceptancePolicy, PatchError, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        Attribute, AuditMeta, Edge, ElementStatus, Entity, Finding, FindingSeverity, Graph, Node,
        NodePayload, Operation, OperationKind, Question, QuestionKind, RelationKind,
        RelationProperties, Stakeholder,
    };
    use plumb_validation::{finding_id, finding_key, GeneratedFinding};
    use serde::Deserialize;

    use ElementStatus::{Accepted, Proposed, Suspect};

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const HR_PROFILE: &str = include_str!("../../../fixtures/hr-leave/profile.yaml");
    const HR_STAKEHOLDERS: &str = include_str!("../../../fixtures/hr-leave/stakeholders.yaml");

    // Independently computed goldens (Python hashlib, RFC 8785).
    const GOLDEN_SINGLE_ROUND: &str = "rnd:efbb8f56f7e087a8";
    const GOLDEN_MULTI_ROUND: &str = "rnd:c1343f956d84fbc9";

    const HR: &str = "stakeholder:hr";
    const ARCHITECT: &str = "stakeholder:architect";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ids(list: &[&str]) -> Vec<Id> {
        list.iter().map(|s| id(s)).collect()
    }

    fn qid(n: usize) -> String {
        format!("q:{n:016x}")
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

    fn attribute(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
            NodePayload::Attribute(Attribute {
                name: node_id.into(),
                value_type: "Int".into(),
                nullable: false,
                unit: None,
                precision: None,
                enum_values: None,
                data_classification: None,
            }),
        )
    }

    fn operation(node_id: &str) -> Node {
        node(
            node_id,
            Accepted,
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

    fn finding_payload(severity: FindingSeverity, affected: &[&str]) -> Finding {
        let mut affected_refs = ids(affected);
        affected_refs.sort();
        Finding {
            code: "PLUMB.F2.OPERATION.PERFORMER".into(),
            family: "F2".into(),
            severity,
            message: "open".into(),
            status: "Open".into(),
            affected_refs,
            standard_rule_ref: None,
            suggested_resolution: None,
            waiver_ref: None,
        }
    }

    fn question_payload(
        finding_ref: &str,
        stakeholder: Option<&str>,
        priority: Option<&str>,
    ) -> Question {
        Question {
            finding_ref: id(finding_ref),
            question_kind: QuestionKind::RoleAssignment,
            prompt: format!("About {finding_ref}?"),
            status: "Open".into(),
            answer_schema: Some(serde_json::json!({"type": "object"})),
            stakeholder_ref: stakeholder.map(id),
            priority: priority.map(Into::into),
            round_ref: None,
            context_refs: None,
        }
    }

    /// A synthetic graph: stakeholders, Entities with owned and ownerless Attributes, an
    /// Operation, blocker and error Findings, and Questions added per test.
    #[derive(Clone)]
    struct Fx {
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    }

    impl Fx {
        fn new() -> Fx {
            Fx {
                nodes: vec![
                    stakeholder(HR, Accepted),
                    stakeholder(ARCHITECT, Accepted),
                    stakeholder("stakeholder:old", Suspect),
                    entity("entity:order"),
                    entity("entity:customer"),
                    attribute("attr:amount"),
                    attribute("attr:loose"),
                    node(
                        "entity:retired",
                        Suspect,
                        NodePayload::Entity(Entity {
                            name: "Retired".into(),
                            description: None,
                            aggregate_root: None,
                        }),
                    ),
                    operation("operation:approve"),
                    node(
                        "fnd:000000000000b10c",
                        Accepted,
                        NodePayload::Finding(finding_payload(
                            FindingSeverity::Blocker,
                            &["operation:approve"],
                        )),
                    ),
                    node(
                        "fnd:0000000000000e22",
                        Accepted,
                        NodePayload::Finding(finding_payload(
                            FindingSeverity::Error,
                            &["operation:approve"],
                        )),
                    ),
                ],
                edges: vec![
                    edge(
                        RelationKind::HasAttribute,
                        "entity:order",
                        "attr:amount",
                        Accepted,
                    ),
                    // The only owner edge of attr:loose is from a Suspect Entity.
                    edge(
                        RelationKind::HasAttribute,
                        "entity:retired",
                        "attr:loose",
                        Accepted,
                    ),
                ],
            }
        }

        fn with(mut self, n: Node) -> Fx {
            self.nodes.push(n);
            self
        }

        fn finding(self, node_id: &str, severity: FindingSeverity, affected: &[&str]) -> Fx {
            self.with(node(
                node_id,
                Accepted,
                NodePayload::Finding(finding_payload(severity, affected)),
            ))
        }

        /// An Accepted Open Question linked to `finding`.
        fn question(
            self,
            question_id: &str,
            finding: &str,
            stakeholder: Option<&str>,
            priority: &str,
        ) -> Fx {
            self.with(node(
                question_id,
                Accepted,
                NodePayload::Question(question_payload(finding, stakeholder, Some(priority))),
            ))
        }

        fn edit_question(mut self, question_id: &str, mut edit: impl FnMut(&mut Question)) -> Fx {
            for n in &mut self.nodes {
                if n.id == id(question_id) {
                    if let NodePayload::Question(q) = &mut n.payload {
                        edit(q);
                    }
                }
            }
            self
        }

        fn edit_node(mut self, node_id: &str, mut edit: impl FnMut(&mut Node)) -> Fx {
            for n in &mut self.nodes {
                if n.id == id(node_id) {
                    edit(n);
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

    const BLOCKER: &str = "fnd:000000000000b10c";
    const ERROR: &str = "fnd:0000000000000e22";

    fn size(n: usize) -> QuestionRoundConfig {
        QuestionRoundConfig {
            question_round_size: n,
        }
    }

    fn compose(fx: &Fx, n: usize) -> QuestionRoundResult {
        compose_question_rounds(&fx.graph(), &size(n)).unwrap()
    }

    fn round_of<'a>(result: &'a QuestionRoundResult, stakeholder: &str) -> &'a QuestionRound {
        result
            .rounds
            .iter()
            .find(|r| r.stakeholder_ref == id(stakeholder))
            .unwrap_or_else(|| panic!("no round for {stakeholder}"))
    }

    fn strings(list: &[Id]) -> Vec<String> {
        list.iter().map(Id::to_string).collect()
    }

    fn disposition<'a>(
        result: &'a QuestionRoundResult,
        question: &str,
    ) -> Vec<&'a QuestionRoundDisposition> {
        result
            .dispositions
            .iter()
            .filter(|d| d.question_ref() == &id(question))
            .collect()
    }

    fn replaced(result: &QuestionRoundResult) -> Vec<(Id, Question)> {
        let patch = &result.proposal.as_ref().expect("proposal").patch_set.patch;
        let one = |p: &SemanticPatch| match p {
            SemanticPatch::ReplacePayload {
                target,
                payload: NodePayload::Question(q),
            } => (target.id.clone(), q.clone()),
            other => panic!("unexpected patch {other:?}"),
        };
        match patch {
            SemanticPatch::Compound { patches } => patches.iter().map(one).collect(),
            single => vec![one(single)],
        }
    }

    /// `count` blocker Questions for `stakeholder` with priorities count..1.
    fn many(mut fx: Fx, stakeholder: &str, first: usize, count: usize) -> Fx {
        for i in 0..count {
            let priority = (count - i).to_string();
            fx = fx.question(&qid(first + i), BLOCKER, Some(stakeholder), &priority);
        }
        fx
    }

    // ------------------------------------------------------------------ configuration

    #[test]
    fn round_size_configuration() {
        for valid in [1, 15, 1000] {
            assert!(size(valid).validate().is_ok(), "{valid}");
        }
        for invalid in [0, 1001, usize::MAX] {
            assert!(matches!(
                size(invalid).validate(),
                Err(RoundError::InvalidConfig { .. })
            ));
            assert!(compose_question_rounds(&Fx::new().graph(), &size(invalid)).is_err());
        }
    }

    #[derive(Deserialize)]
    struct HrRoundProfile {
        question_round_size: usize,
    }

    #[test]
    fn hr_profile_fixture_round_size_is_15() {
        let profile: HrRoundProfile = serde_yaml::from_str(HR_PROFILE).unwrap();
        assert_eq!(profile.question_round_size, 15);
        assert!(size(profile.question_round_size).validate().is_ok());
    }

    // ------------------------------------------------------------------ eligibility

    #[test]
    fn eligibility_and_exclusions() {
        let fx = Fx::new()
            .question(&qid(1), BLOCKER, Some(HR), "5")
            .with(node(
                &qid(2),
                Proposed,
                NodePayload::Question(question_payload(BLOCKER, Some(HR), Some("5"))),
            ))
            .question(&qid(3), BLOCKER, Some(HR), "5")
            .edit_question(&qid(3), |q| q.status = "Answered".into())
            .question(&qid(4), BLOCKER, Some(HR), "5")
            .edit_question(&qid(4), |q| q.status = "Closed".into())
            .question(&qid(5), BLOCKER, Some(HR), "5")
            .edit_question(&qid(5), |q| q.status = "Superseded".into())
            .question(&qid(6), BLOCKER, Some(HR), "5")
            .edit_question(&qid(6), |q| q.round_ref = Some(id("rnd:00000000000000aa")))
            .question(&qid(7), BLOCKER, None, "5")
            .question(&qid(8), BLOCKER, Some("stakeholder:old"), "5")
            .question(&qid(9), "fnd:00000000000000ff", Some(HR), "5");
        let result = compose(&fx, 15);
        let round = round_of(&result, HR);
        assert_eq!(strings(&round.question_refs), [qid(1)]);
        assert!(
            disposition(&result, &qid(2)).is_empty(),
            "Proposed Questions are not read"
        );
        for (n, status) in [(3, "Answered"), (4, "Closed"), (5, "Superseded")] {
            assert_eq!(
                disposition(&result, &qid(n)),
                [&QuestionRoundDisposition::TerminalQuestion {
                    question_ref: id(&qid(n)),
                    status: status.into()
                }]
            );
        }
        assert_eq!(
            disposition(&result, &qid(6)),
            [&QuestionRoundDisposition::AlreadyAssigned {
                question_ref: id(&qid(6)),
                round_ref: id("rnd:00000000000000aa")
            }]
        );
        assert_eq!(
            disposition(&result, &qid(7)),
            [&QuestionRoundDisposition::UnassignedQuestion {
                question_ref: id(&qid(7))
            }]
        );
        assert_eq!(
            disposition(&result, &qid(8)),
            [&QuestionRoundDisposition::StaleQuestion {
                question_ref: id(&qid(8)),
                reason: StaleReason::StakeholderNotAccepted {
                    stakeholder_ref: id("stakeholder:old")
                },
            }]
        );
        assert_eq!(
            disposition(&result, &qid(9)),
            [&QuestionRoundDisposition::StaleQuestion {
                question_ref: id(&qid(9)),
                reason: StaleReason::FindingMissing {
                    finding_ref: id("fnd:00000000000000ff")
                },
            }]
        );
        assert_eq!(
            disposition(&result, &qid(1)),
            [&QuestionRoundDisposition::Assigned {
                question_ref: id(&qid(1)),
                round_ref: round.id.clone()
            }]
        );
    }

    #[test]
    fn stale_findings_exclude_questions() {
        let stale = |fx: Fx| -> StaleReason {
            let result = compose(
                &fx.question(&qid(1), "fnd:0000000000000aaa", Some(HR), "5"),
                15,
            );
            assert!(result.rounds.is_empty() && result.proposal.is_none());
            match &result.dispositions[..] {
                [QuestionRoundDisposition::StaleQuestion { reason, .. }] => reason.clone(),
                other => panic!("{other:?}"),
            }
        };
        let f = "fnd:0000000000000aaa";
        let base = || Fx::new().finding(f, FindingSeverity::Blocker, &["operation:approve"]);
        assert_eq!(
            stale(base().edit_node(f, |n| n.status = Suspect)),
            StaleReason::FindingNotAccepted { finding_ref: id(f) }
        );
        assert_eq!(
            stale(
                base().edit_node(f, |n| if let NodePayload::Finding(p) = &mut n.payload {
                    p.status = "Closed".into()
                })
            ),
            StaleReason::FindingNotOpen {
                finding_ref: id(f),
                status: "Closed".into()
            }
        );
        assert_eq!(
            stale(
                base().edit_node(f, |n| if let NodePayload::Finding(p) = &mut n.payload {
                    p.waiver_ref = Some(id("dec:waive"))
                })
            ),
            StaleReason::FindingWaived { finding_ref: id(f) }
        );
        assert_eq!(
            stale(Fx::new().finding(f, FindingSeverity::Blocker, &["entity:retired"])),
            StaleReason::AffectedRefNotAccepted {
                affected_ref: id("entity:retired")
            }
        );
        assert_eq!(
            stale(Fx::new().with(stakeholder(f, Accepted))),
            StaleReason::NotAFinding { finding_ref: id(f) }
        );
        // Stale Questions are never mutated.
        let result = compose(
            &base()
                .edit_node(f, |n| n.status = Suspect)
                .question(&qid(1), f, Some(HR), "5"),
            15,
        );
        assert!(result.proposal.is_none());
    }

    #[test]
    fn malformed_status_and_priority_are_errors() {
        let graph = Fx::new()
            .question(&qid(1), BLOCKER, Some(HR), "5")
            .edit_question(&qid(1), |q| q.status = "InRound".into())
            .graph();
        assert_eq!(
            compose_question_rounds(&graph, &size(15)),
            Err(RoundError::InvalidQuestionStatus {
                question_ref: id(&qid(1)),
                status: "InRound".into()
            })
        );
        for bad in [
            None,
            Some("01"),
            Some("+1"),
            Some("1.0"),
            Some("-1"),
            Some(" 1"),
            Some(""),
            Some("18446744073709551616"),
        ] {
            let graph = Fx::new()
                .question(&qid(1), BLOCKER, Some(HR), "5")
                .edit_question(&qid(1), |q| q.priority = bad.map(Into::into))
                .graph();
            assert_eq!(
                compose_question_rounds(&graph, &size(15)),
                Err(RoundError::InvalidQuestionPriority {
                    question_ref: id(&qid(1)),
                    priority: bad.map(Into::into)
                }),
                "{bad:?}"
            );
        }
        assert_eq!(parse_priority("0"), Some(0));
        assert_eq!(parse_priority("18446744073709551615"), Some(u64::MAX));
        assert_eq!(parse_priority("00"), None);
    }

    // ------------------------------------------------------------------ ordering

    #[test]
    fn ordering_blocking_then_priority_then_id() {
        let fx = Fx::new()
            .question(&qid(10), ERROR, Some(HR), "999")
            .question(&qid(11), BLOCKER, Some(HR), "1")
            .question(&qid(12), BLOCKER, Some(HR), "7")
            .question(&qid(13), ERROR, Some(HR), "40")
            .question(&qid(15), BLOCKER, Some(HR), "7")
            .question(&qid(14), BLOCKER, Some(HR), "7");
        let round = round_of(&compose(&fx, 15), HR).clone();
        assert_eq!(
            strings(&round.question_refs),
            [qid(12), qid(14), qid(15), qid(11), qid(10), qid(13)]
        );
        // The blocker class comes from Finding severity alone: the same Question under an
        // error Finding drops behind every blocker.
        let demoted = fx.edit_question(&qid(12), |q| q.finding_ref = id(ERROR));
        let round = round_of(&compose(&demoted, 15), HR).clone();
        assert_eq!(
            strings(&round.question_refs),
            [qid(14), qid(15), qid(11), qid(10), qid(13), qid(12)]
        );
    }

    // ------------------------------------------------------------------ capacity

    #[test]
    fn capacity_per_stakeholder() {
        let one = many(Fx::new(), HR, 1, 3);
        let result = compose(&one, 1);
        assert_eq!(strings(&round_of(&result, HR).question_refs), [qid(1)]);
        assert_eq!(
            result
                .dispositions
                .iter()
                .filter(|d| matches!(d, QuestionRoundDisposition::DeferredByCapacity { .. }))
                .count(),
            2
        );
        let exact = compose(&many(Fx::new(), HR, 1, 5), 5);
        assert_eq!(round_of(&exact, HR).question_refs.len(), 5);
        assert!(exact
            .dispositions
            .iter()
            .all(|d| matches!(d, QuestionRoundDisposition::Assigned { .. })));
        let plus_one = compose(&many(Fx::new(), HR, 1, 6), 5);
        assert_eq!(round_of(&plus_one, HR).question_refs.len(), 5);
        assert_eq!(
            disposition(&plus_one, &qid(6)),
            [&QuestionRoundDisposition::DeferredByCapacity {
                question_ref: id(&qid(6)),
                stakeholder_ref: id(HR)
            }]
        );
        // Independent allowances: 20 for HR, 8 for the architect, size 15 -> 15 + 8.
        let mixed = many(many(Fx::new(), HR, 1, 20), ARCHITECT, 100, 8);
        let result = compose(&mixed, 15);
        assert_eq!(result.rounds.len(), 2);
        assert_eq!(round_of(&result, ARCHITECT).question_refs.len(), 8);
        assert_eq!(round_of(&result, HR).question_refs.len(), 15);
        assert_eq!(replaced(&result).len(), 23);
        assert_eq!(
            strings(
                &result
                    .rounds
                    .iter()
                    .map(|r| r.stakeholder_ref.clone())
                    .collect::<Vec<_>>()
            ),
            [ARCHITECT, HR]
        );
    }

    #[test]
    fn thirty_two_questions_with_size_fifteen_and_next_round() {
        let fx = many(Fx::new(), HR, 1, 32);
        let graph = fx.graph();
        let first = compose_question_rounds(&graph, &size(15)).unwrap();
        let assigned = |r: &QuestionRoundResult| {
            r.dispositions
                .iter()
                .filter(|d| matches!(d, QuestionRoundDisposition::Assigned { .. }))
                .count()
        };
        let deferred = |r: &QuestionRoundResult| {
            r.dispositions
                .iter()
                .filter(|d| matches!(d, QuestionRoundDisposition::DeferredByCapacity { .. }))
                .count()
        };
        assert_eq!((assigned(&first), deferred(&first)), (15, 17));
        let first_round = round_of(&first, HR).clone();
        assert_eq!(
            strings(&first_round.question_refs),
            (1..=15).map(qid).collect::<Vec<_>>()
        );
        // Rerun after applying: the first 15 are AlreadyAssigned, the next 15 form a new round.
        let applied = apply_patch(&graph, &first.proposal.unwrap().patch_set)
            .unwrap()
            .graph;
        let second = compose_question_rounds(&applied, &size(15)).unwrap();
        for n in 1..=15 {
            assert_eq!(
                disposition(&second, &qid(n)),
                [&QuestionRoundDisposition::AlreadyAssigned {
                    question_ref: id(&qid(n)),
                    round_ref: first_round.id.clone()
                }]
            );
        }
        assert_eq!((assigned(&second), deferred(&second)), (15, 2));
        let second_round = round_of(&second, HR);
        assert_eq!(
            strings(&second_round.question_refs),
            (16..=30).map(qid).collect::<Vec<_>>()
        );
        assert_ne!(second_round.id, first_round.id);
        assert!(replaced(&second)
            .iter()
            .all(|(q, _)| !first_round.question_refs.contains(q)));
        let applied_twice = apply_patch(&applied, &second.proposal.unwrap().patch_set)
            .unwrap()
            .graph;
        for n in 1..=15 {
            let NodePayload::Question(q) = &applied_twice.node(&id(&qid(n))).unwrap().payload
            else {
                unreachable!()
            };
            assert_eq!(q.round_ref.as_ref(), Some(&first_round.id));
        }
        let third = compose_question_rounds(&applied_twice, &size(15)).unwrap();
        assert_eq!(round_of(&third, HR).question_refs.len(), 2);
    }

    #[test]
    fn no_eligible_question_is_an_empty_success() {
        let result = compose(&Fx::new().question(&qid(1), BLOCKER, None, "5"), 15);
        assert!(result.rounds.is_empty() && result.proposal.is_none());
        assert_eq!(result.dispositions.len(), 1);
        let empty = compose(&Fx::new(), 15);
        assert!(
            empty.rounds.is_empty() && empty.proposal.is_none() && empty.dispositions.is_empty()
        );
    }

    // ------------------------------------------------------------------ grouping

    #[test]
    fn entity_anchors() {
        let fx = Fx::new()
            .finding(
                "fnd:00000000000000e1",
                FindingSeverity::Blocker,
                &["entity:order"],
            )
            .finding(
                "fnd:00000000000000a1",
                FindingSeverity::Blocker,
                &["attr:amount"],
            )
            .finding(
                "fnd:00000000000000a2",
                FindingSeverity::Blocker,
                &["attr:loose"],
            )
            .finding(
                "fnd:00000000000000e2",
                FindingSeverity::Blocker,
                &["entity:customer", "entity:order"],
            )
            .finding(
                "fnd:00000000000000e3",
                FindingSeverity::Blocker,
                &["attr:amount", "entity:order"],
            );
        let graph = fx.graph();
        let anchor = |f: &str| {
            let NodePayload::Finding(finding) = &graph.node(&id(f)).unwrap().payload else {
                unreachable!()
            };
            entity_anchor(&graph, finding)
        };
        assert_eq!(anchor("fnd:00000000000000e1"), Some(id("entity:order")));
        assert_eq!(anchor("fnd:00000000000000a1"), Some(id("entity:order")));
        // attr:loose is owned only by a Suspect Entity: no unique Accepted owner.
        assert_eq!(anchor("fnd:00000000000000a2"), None);
        assert_eq!(anchor("fnd:00000000000000e2"), None);
        // The same Entity reached directly and through its attribute is one candidate.
        assert_eq!(anchor("fnd:00000000000000e3"), Some(id("entity:order")));
        assert_eq!(anchor(BLOCKER), None);
    }

    #[test]
    fn groups_are_contiguous_and_never_reorder() {
        let fx = Fx::new()
            .finding(
                "fnd:00000000000000e1",
                FindingSeverity::Blocker,
                &["entity:order"],
            )
            .finding(
                "fnd:00000000000000e4",
                FindingSeverity::Blocker,
                &["entity:customer"],
            )
            .question(&qid(1), "fnd:00000000000000e1", Some(HR), "9")
            .question(&qid(2), "fnd:00000000000000e1", Some(HR), "8")
            .question(&qid(3), BLOCKER, Some(HR), "7")
            .question(&qid(4), "fnd:00000000000000e1", Some(HR), "6")
            .question(&qid(5), "fnd:00000000000000e4", Some(HR), "5")
            .question(&qid(6), "fnd:00000000000000e4", Some(HR), "4");
        let round = round_of(&compose(&fx, 15), HR).clone();
        assert_eq!(
            strings(&round.question_refs),
            (1..=6).map(qid).collect::<Vec<_>>()
        );
        assert_eq!(
            round.groups,
            [
                QuestionRoundGroup {
                    entity_ref: Some(id("entity:order")),
                    question_refs: ids(&[&qid(1), &qid(2)])
                },
                QuestionRoundGroup {
                    entity_ref: None,
                    question_refs: ids(&[&qid(3)])
                },
                QuestionRoundGroup {
                    entity_ref: Some(id("entity:order")),
                    question_refs: ids(&[&qid(4)])
                },
                QuestionRoundGroup {
                    entity_ref: Some(id("entity:customer")),
                    question_refs: ids(&[&qid(5), &qid(6)])
                },
            ]
        );
        let flat: Vec<Id> = round
            .groups
            .iter()
            .flat_map(|g| g.question_refs.clone())
            .collect();
        assert_eq!(flat, round.question_refs);
    }

    // ------------------------------------------------------------------ round identity

    #[test]
    fn round_id_goldens_and_sensitivity() {
        let single = compose(&Fx::new().question(&qid(1), BLOCKER, Some(HR), "5"), 15);
        assert_eq!(round_of(&single, HR).id.to_string(), GOLDEN_SINGLE_ROUND);
        let multi = Fx::new()
            .question(&qid(1), ERROR, Some(HR), "9")
            .question(&qid(2), ERROR, Some(HR), "9")
            .question(&qid(3), BLOCKER, Some(HR), "1");
        let round = round_of(&compose(&multi, 15), HR).clone();
        assert_eq!(strings(&round.question_refs), [qid(3), qid(1), qid(2)]);
        assert_eq!(round.id.to_string(), GOLDEN_MULTI_ROUND);
        let project = id("project:pilot");
        let ordered = ids(&[&qid(3), &qid(1), &qid(2)]);
        assert_eq!(round_id(&project, &id(HR), &ordered).unwrap(), round.id);
        let reordered = ids(&[&qid(1), &qid(2), &qid(3)]);
        assert_ne!(round_id(&project, &id(HR), &reordered).unwrap(), round.id);
        assert_ne!(
            round_id(&project, &id(ARCHITECT), &ordered).unwrap(),
            round.id
        );
        assert_ne!(
            round_id(&id("project:other"), &id(HR), &ordered).unwrap(),
            round.id
        );
    }

    #[test]
    fn round_id_collision_blocks_that_round() {
        let fx = Fx::new()
            .question(&qid(1), BLOCKER, Some(HR), "5")
            .question(&qid(100), BLOCKER, Some(ARCHITECT), "5")
            .with(stakeholder(GOLDEN_SINGLE_ROUND, Accepted));
        let result = compose(&fx, 15);
        assert_eq!(result.rounds.len(), 1);
        assert_eq!(result.rounds[0].stakeholder_ref, id(ARCHITECT));
        assert_eq!(
            disposition(&result, &qid(1)),
            [&QuestionRoundDisposition::RoundIdCollision {
                question_ref: id(&qid(1)),
                round_ref: id(GOLDEN_SINGLE_ROUND)
            }]
        );
        assert_eq!(
            strings(
                &replaced(&result)
                    .into_iter()
                    .map(|(q, _)| q)
                    .collect::<Vec<_>>()
            ),
            [qid(100)]
        );
    }

    // ------------------------------------------------------------------ proposal

    #[test]
    fn proposal_changes_only_round_ref() {
        let fx = many(many(Fx::new(), HR, 1, 2), ARCHITECT, 100, 1);
        let graph = fx.graph();
        let result = compose_question_rounds(&graph, &size(15)).unwrap();
        let proposal = result.proposal.clone().unwrap();
        assert_eq!(proposal.stage, StageId::S3);
        assert_eq!(proposal.materiality, ProposalMateriality::NonSemantic);
        assert_eq!(
            proposal.acceptance_policy,
            AcceptancePolicy::AutoNonSemantic
        );
        assert_eq!(
            serde_json::to_value(proposal.acceptance_policy).unwrap(),
            serde_json::json!("AUTO_NON_SEMANTIC")
        );
        assert_eq!(proposal.confidence, None);
        assert!(matches!(
            proposal.patch_set.patch,
            SemanticPatch::Compound { .. }
        ));
        let changes = replaced(&result);
        assert_eq!(
            strings(&changes.iter().map(|(q, _)| q.clone()).collect::<Vec<_>>()),
            [qid(1), qid(2), qid(100)]
        );
        for (question_ref, payload) in &changes {
            let NodePayload::Question(before) = &graph.node(question_ref).unwrap().payload else {
                unreachable!()
            };
            let round = result
                .rounds
                .iter()
                .find(|r| r.question_refs.contains(question_ref))
                .unwrap();
            assert_eq!(payload.round_ref.as_ref(), Some(&round.id));
            assert_eq!(payload.status, "Open");
            let mut restored = payload.clone();
            restored.round_ref = None;
            assert_eq!(&restored, before);
            assert_eq!(
                to_canonical_json(&restored).unwrap(),
                to_canonical_json(before).unwrap()
            );
        }
        let applied = apply_patch(&graph, &proposal.patch_set).unwrap().graph;
        assert_eq!(
            applied.semantic_hash().unwrap(),
            graph.semantic_hash().unwrap()
        );
        assert_eq!(
            proposal.patch_set.base_semantic_hash,
            graph.semantic_hash().unwrap()
        );
        // A single assignment is a single ReplacePayload.
        let single = compose(&Fx::new().question(&qid(1), BLOCKER, Some(HR), "5"), 15);
        assert!(matches!(
            single.proposal.unwrap().patch_set.patch,
            SemanticPatch::ReplacePayload { .. }
        ));
    }

    #[test]
    fn cas_rejects_a_changed_question() {
        let fx = Fx::new().question(&qid(1), BLOCKER, Some(HR), "5");
        let result = compose(&fx, 15);
        let changed = fx
            .edit_question(&qid(1), |q| q.prompt = "Changed meanwhile?".into())
            .graph();
        assert!(matches!(
            apply_patch(&changed, &result.proposal.unwrap().patch_set),
            Err(PatchError::ElementHashMismatch { .. })
        ));
    }

    // ------------------------------------------------------------------ HR adapter

    fn hr_question_graph() -> Graph {
        // The S3.1 representative fixture, regenerated through the S3.1 engine and applied.
        use plumb_psg::{
            Actor, ActorKind, BusinessRole, Calculation, Calendar, Event, Invariant, Modality,
            Permission, Requirement, RequirementKind, RequirementLevel, ResourceScope,
        };
        let mut nodes = vec![
            stakeholder("stakeholder:architect", Accepted),
            stakeholder("stakeholder:auditor", Accepted),
            stakeholder("stakeholder:employee", Accepted),
            stakeholder("stakeholder:hr", Accepted),
            stakeholder("stakeholder:manager", Accepted),
            entity("entity:order"),
            operation("operation:approve"),
            operation("operation:submit"),
        ];
        nodes.push(node(
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
        ));
        nodes.push(node(
            "actor:bot",
            Accepted,
            NodePayload::Actor(Actor {
                name: "Bot".into(),
                actor_kind: ActorKind::System,
            }),
        ));
        nodes.push(node(
            "brole:clerk",
            Accepted,
            NodePayload::BusinessRole(BusinessRole {
                name: "Clerk".into(),
            }),
        ));
        nodes.push(node(
            "event:submitted",
            Accepted,
            NodePayload::Event(Event {
                name: "Submitted".into(),
                payload_schema_ref: None,
                semantic_type: None,
            }),
        ));
        nodes.push(node(
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
        ));
        nodes.push(node(
            "calendar:nl",
            Accepted,
            NodePayload::Calendar(Calendar {
                time_zone: "Europe/Amsterdam".into(),
                week_pattern: serde_json::json!(["Mon"]),
                region: None,
                holiday_source: None,
            }),
        ));
        nodes.push(node(
            "permission:approve",
            Accepted,
            NodePayload::Permission(Permission {
                name: "Approve".into(),
            }),
        ));
        nodes.push(node(
            "scope:orders",
            Accepted,
            NodePayload::ResourceScope(ResourceScope {
                resource_ref: id("entity:order"),
                scope_kind: "entity".into(),
            }),
        ));
        nodes.push(node(
            "inv:positive",
            Accepted,
            NodePayload::Invariant(Invariant {
                scope_ref: id("entity:order"),
                expression: "amount".into(),
            }),
        ));
        let edges = vec![
            edge(
                RelationKind::UsesCalculation,
                "operation:approve",
                "calculation:days",
                Accepted,
            ),
            edge(
                RelationKind::Permits,
                "permission:approve",
                "operation:approve",
                Accepted,
            ),
            edge(
                RelationKind::ScopedTo,
                "permission:approve",
                "scope:orders",
                Accepted,
            ),
        ];
        let graph = Graph::new(
            id("project:pilot"),
            id("profile:plumb-software-2026.1"),
            nodes,
            edges,
        )
        .unwrap();
        let gf = |code: &str, condition: &str, targets: &[&str], severity| {
            let affected = ids(targets);
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
        };
        let findings = vec![
            gf(
                "PLUMB.F2.DOMAIN.RELATION_TYPED",
                "domain_relationship_cardinality_unresolved:domainrel:leave-employee",
                &["req:r1"],
                FindingSeverity::Error,
            ),
            gf(
                "PLUMB.F2.STATE.TRANSITION_COMPLETE",
                "state_transition_trigger_unresolved:transition:approve",
                &["req:r1"],
                FindingSeverity::Blocker,
            ),
            gf(
                "PLUMB.F2.OPERATION.PERFORMER",
                "operation_performer_missing",
                &["operation:approve", "operation:submit"],
                FindingSeverity::Blocker,
            ),
            gf(
                "PLUMB.F2.TIME.CALENDAR_DEFINED",
                "business_time_calendar_missing",
                &["calculation:days"],
                FindingSeverity::Blocker,
            ),
            gf(
                "RBAC.F2.PERMISSION.CONCRETE",
                "permission_not_concrete",
                &["permission:approve"],
                FindingSeverity::Blocker,
            ),
            gf(
                "PLUMB.F2.INVARIANT.EXPRESSIBLE",
                "invariant_not_expressible",
                &["inv:positive"],
                FindingSeverity::Error,
            ),
        ];
        let generated = generate_questions(
            &graph,
            &QuestionGenerationInput {
                findings,
                routing: StakeholderRoutingConfig::from_yaml(HR_STAKEHOLDERS).unwrap(),
            },
            &QuestionAudit {
                created_by: id("agent:compiler"),
                created_at: AT.parse().unwrap(),
            },
        )
        .unwrap();
        apply_patch(&graph, &generated.proposal.unwrap().patch_set)
            .unwrap()
            .graph
    }

    /// Synthetic adapter over the S3.1 HR routing fixture: this does NOT claim the live compiled
    /// HR graph already contains these Questions.
    #[test]
    fn hr_round_of_seven_representative_questions() {
        let profile: HrRoundProfile = serde_yaml::from_str(HR_PROFILE).unwrap();
        let graph = hr_question_graph();
        let result = compose_question_rounds(&graph, &size(profile.question_round_size)).unwrap();
        assert_eq!(result.rounds.len(), 1);
        let round = &result.rounds[0];
        assert_eq!(round.stakeholder_ref, id(HR));
        // Blockers first (calendar, performers 8; permission, trigger 4), then errors (3).
        assert_eq!(
            strings(&round.question_refs),
            [
                "q:5871379a4480cf38",
                "q:6204ae40674353cc",
                "q:c028a0b06ecc6d87",
                "q:0bb549ba61718eff",
                "q:6f249ab4436ceb55",
                "q:2a414cddc59ff77f",
                "q:8b4dc01ee3454b93",
            ]
        );
        assert!(result
            .dispositions
            .iter()
            .all(|d| matches!(d, QuestionRoundDisposition::Assigned { .. })));
        assert_eq!(result, compose_question_rounds(&graph, &size(15)).unwrap());
    }

    #[test]
    fn rounds_preserve_subject_first_context() {
        let graph = hr_question_graph();
        let result = compose_question_rounds(&graph, &size(15)).unwrap();
        let changes = replaced(&result);
        assert_eq!(changes.len(), 7);
        for (question_ref, payload) in changes {
            let NodePayload::Question(before) = &graph.node(&question_ref).unwrap().payload else {
                unreachable!()
            };
            let context = before.context_refs.clone().unwrap();
            assert!(!context.is_empty());
            assert_eq!(payload.context_refs, Some(context));
        }
    }

    #[test]
    fn hr_synthetic_set_truncates_to_fifteen() {
        let profile: HrRoundProfile = serde_yaml::from_str(HR_PROFILE).unwrap();
        let result = compose(&many(Fx::new(), HR, 1, 20), profile.question_round_size);
        assert_eq!(round_of(&result, HR).question_refs.len(), 15);
        assert_eq!(
            result
                .dispositions
                .iter()
                .filter(|d| matches!(d, QuestionRoundDisposition::DeferredByCapacity { .. }))
                .count(),
            5
        );
    }

    // ------------------------------------------------------------------ determinism and guards

    #[test]
    fn composition_is_deterministic_under_reordering() {
        let fx = many(many(Fx::new(), HR, 1, 18), ARCHITECT, 100, 4)
            .finding(
                "fnd:00000000000000e1",
                FindingSeverity::Error,
                &["entity:order"],
            )
            .question(&qid(50), "fnd:00000000000000e1", Some(HR), "99")
            .question(&qid(51), BLOCKER, None, "3");
        let forward = compose(&fx, 15);
        let mut reversed = fx.clone();
        reversed.nodes.reverse();
        reversed.edges.reverse();
        let backward = compose(&reversed, 15);
        assert_eq!(forward.rounds, backward.rounds);
        assert_eq!(forward.dispositions, backward.dispositions);
        assert_eq!(
            to_canonical_json(&forward.proposal).unwrap(),
            to_canonical_json(&backward.proposal).unwrap()
        );
        // Dispositions are ordered by Question ID, then kind.
        let keys: Vec<&Id> = forward
            .dispositions
            .iter()
            .map(|d| d.question_ref())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn production_source_guard() {
        let source = include_str!("../src/round.rs");
        for token in [
            "std::fs",
            "File::open",
            "include_str!",
            "fixtures/",
            "profile.yaml",
            "reqwest",
            "std::net",
            "SystemClock",
            "Utc::now",
            "Instant::now",
            "now()",
            "plumb_inference",
            "InferenceRequest",
            "rand",
            "f32",
            "f64",
            "impact_reachable_nodes",
            "severity_weight",
            "route_question",
            ".name",
            "NodePayload::Round",
            "AddNode",
            "ValidationProfile",
            "unsafe",
        ] {
            assert!(!source.contains(token), "round.rs contains {token}");
        }
        let lib = include_str!("../src/lib.rs");
        assert!(lib.contains("pub mod round;"));
        assert!(!lib.contains("Proposal"));
    }
}
