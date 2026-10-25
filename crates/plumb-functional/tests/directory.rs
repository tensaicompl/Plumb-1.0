//! R0.6 configured semantic directory: typed draft -> one Proposed node per element in its own
//! Semantic / HUMAN_CONFIRM proposal, grounded evidence, runtime calendar binding and
//! DecisionTable encoding only through `encode_decision_table`.

mod directory_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{Hash, Id, StageId, Timestamp};
    use plumb_functional::directory::*;
    use plumb_functional::{encode_decision_table, DecisionTableSpec};
    use plumb_patch::{apply_patch, AcceptancePolicy, ProposalMateriality, SemanticPatch};
    use plumb_psg::{
        AuditMeta, ElementStatus, EvidenceFragment, EvidenceLocator, EvidenceRef, Graph, Modality,
        Node, NodePayload, NodeType, Requirement, RequirementKind, RequirementLevel,
        SourceArtifact,
    };
    use plumb_validation::decision_table_analysis::{
        DecisionColumn, DecisionInputCell, DecisionOutputCell, DecisionRow, DecisionType,
    };
    use serde_json::{json, Value};

    use ElementStatus::{Accepted, Proposed};

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const NOW: &str = "2026-05-01T12:00:00.000000000Z";
    const STAGE_ACTOR: &str = "agent:compiler";

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

    fn source_ref() -> Id {
        plumb_psg::source_artifact_id(&Hash::content_sha256(b"requirements")).unwrap()
    }

    fn locator(n: u64) -> EvidenceLocator {
        EvidenceLocator::TextRange {
            start: n * 100,
            end: n * 100 + 50,
        }
    }

    fn fragment(n: u64) -> Id {
        plumb_psg::evidence_fragment_id(&source_ref(), &locator(n)).unwrap()
    }

    fn fragment_node(n: u64) -> Node {
        node(
            fragment(n).as_str(),
            Accepted,
            NodePayload::EvidenceFragment(EvidenceFragment {
                source_ref: source_ref(),
                locator: locator(n),
                content_hash: Hash::content_sha256(format!("f{n}").as_bytes()),
                extracted_text: None,
                speaker: None,
                source_timestamp: None,
            }),
        )
    }

    fn requirement(node_id: &str, status: ElementStatus, fragments: &[u64]) -> Node {
        let mut n = node(
            node_id,
            status,
            NodePayload::Requirement(Requirement {
                statement: format!("The system shall satisfy {node_id}."),
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
        );
        n.evidence = fragments
            .iter()
            .map(|f| EvidenceRef::from(fragment(*f)))
            .collect();
        n
    }

    /// req:a (fragments 1, 2), req:b (fragments 2, 3), req:p Proposed and a Term.
    fn graph() -> Graph {
        let source = node(
            source_ref().as_str(),
            Accepted,
            NodePayload::SourceArtifact(SourceArtifact {
                source_kind: "document".into(),
                display_name: "requirements.md".into(),
                content_hash: Hash::content_sha256(b"requirements"),
                media_type: "text/markdown".into(),
                external_uri: None,
                external_version: None,
                producer: None,
                created_at_source: None,
                language: None,
                classification: None,
            }),
        );
        let mut nodes = vec![source, fragment_node(1), fragment_node(2), fragment_node(3)];
        nodes.extend([
            requirement("req:a", Accepted, &[2, 1]),
            requirement("req:b", Accepted, &[3, 2]),
            requirement("req:p", Proposed, &[1]),
            node(
                "term:leave",
                Accepted,
                NodePayload::Term(
                    serde_json::from_value(json!({
                        "term": "leave", "language": "en", "definition_ref": null, "aliases": null,
                        "status": null
                    }))
                    .unwrap(),
                ),
            ),
        ]);
        Graph::new(id(PROJECT), id(PROFILE), nodes, vec![]).unwrap()
    }

    fn spec() -> DecisionTableSpec {
        DecisionTableSpec {
            hit_policy: "UNIQUE".into(),
            inputs: vec![DecisionColumn {
                name: "eligible".into(),
                ty: DecisionType::Bool,
                domain: None,
            }],
            outputs: vec![DecisionColumn {
                name: "approve".into(),
                ty: DecisionType::Bool,
                domain: None,
            }],
            rows: vec![
                DecisionRow {
                    inputs: vec![DecisionInputCell::Bool { value: true }],
                    outputs: vec![DecisionOutputCell::Bool { value: true }],
                },
                DecisionRow {
                    inputs: vec![DecisionInputCell::Bool { value: false }],
                    outputs: vec![DecisionOutputCell::Bool { value: false }],
                },
            ],
            default_output: None,
        }
    }

    /// The complete configuration JSON of a six-family directory, deliberately not in ID order.
    fn draft_json() -> Value {
        json!({
            "stakeholders": [
                {"id": "stakeholder:hr", "grounding_refs": ["req:b", "req:a"],
                 "payload": {"name": "HR", "stakeholder_kind": "internal", "organization": null,
                             "responsibilities": null, "contact_ref": null}}
            ],
            "actors": [
                {"id": "actor:manager",
                 "payload": {"name": "Manager", "actor_kind": "human"}},
                {"id": "actor:employee", "grounding_refs": ["req:a"],
                 "payload": {"name": "Employee", "actor_kind": "human"}}
            ],
            "business_roles": [
                {"id": "brole:approver", "grounding_refs": [],
                 "payload": {"name": "Approver"}}
            ],
            "calendars": [
                {"id": "calendar:nl", "calendar_ref": "cal:office", "time_zone": "Europe/Amsterdam",
                 "region": "NL", "holiday_source": null, "grounding_refs": ["req:b"]}
            ],
            "rules": [
                {"id": "rule:balance",
                 "payload": {"name": "Balance", "rule_kind": "business",
                             "condition_expr": null, "action_expr": null}}
            ],
            "decision_tables": [
                {"id": "dt:approval", "name": "Approval", "spec": spec()}
            ]
        })
    }

    fn draft() -> DirectoryDraft {
        serde_json::from_value(draft_json()).unwrap()
    }

    fn bindings() -> Vec<DirectoryCalendarBinding> {
        vec![DirectoryCalendarBinding {
            calendar_ref: id("cal:office"),
            // Configured order, not sorted.
            weekend: vec!["sunday".into(), "saturday".into()],
        }]
    }

    fn audit() -> DirectoryAudit {
        DirectoryAudit {
            created_by: id(STAGE_ACTOR),
            created_at: ts(NOW),
        }
    }

    fn produce(g: &Graph, d: &DirectoryDraft) -> Result<DirectoryResult, DirectoryError> {
        produce_directory(g, d, &bindings(), &audit())
    }

    /// The single added node of a proposed outcome.
    fn added(outcome: &DirectoryOutcome) -> (&plumb_patch::Proposal, &Node) {
        let DirectoryDisposition::Proposed { proposal } = &outcome.disposition else {
            panic!("{} is not proposed", outcome.element_ref);
        };
        let SemanticPatch::AddNode { node } = &proposal.patch_set.patch else {
            panic!("not exactly one AddNode");
        };
        (proposal, node)
    }

    #[test]
    fn six_families_each_get_their_own_human_confirm_proposal() {
        let g = graph();
        let result = produce(&g, &draft()).unwrap();
        let ids: Vec<String> = result
            .outcomes
            .iter()
            .map(|o| o.element_ref.to_string())
            .collect();
        // Element-ID order, independent of configuration order.
        assert_eq!(
            ids,
            [
                "actor:employee",
                "actor:manager",
                "brole:approver",
                "calendar:nl",
                "dt:approval",
                "rule:balance",
                "stakeholder:hr"
            ]
        );
        let types: BTreeSet<NodeType> = result.outcomes.iter().map(|o| o.node_type).collect();
        assert_eq!(
            types,
            BTreeSet::from([
                NodeType::Stakeholder,
                NodeType::Actor,
                NodeType::BusinessRole,
                NodeType::Calendar,
                NodeType::Rule,
                NodeType::DecisionTable
            ])
        );
        let mut proposal_ids = BTreeSet::new();
        for outcome in &result.outcomes {
            let (proposal, node) = added(outcome);
            assert!(
                proposal_ids.insert(proposal.id.clone()),
                "one proposal each"
            );
            assert_eq!(node.id, outcome.element_ref);
            assert_eq!(node.status, Proposed);
            assert_eq!(node.payload.node_type(), outcome.node_type);
            assert_eq!(node.audit.created_by, id(STAGE_ACTOR));
            assert_eq!(node.audit.created_at, ts(NOW));
            assert_eq!(proposal.materiality, ProposalMateriality::Semantic);
            assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanConfirm);
            assert_eq!(
                proposal.patch_set.base_semantic_hash,
                g.semantic_hash().unwrap()
            );
            assert_eq!(proposal.evidence_refs, node.evidence);
            let expected_stage = if outcome.node_type == NodeType::Stakeholder {
                StageId::S1
            } else {
                StageId::S2
            };
            assert_eq!(proposal.stage, expected_stage);
            // Each proposal applies on its own and adds exactly that Proposed node.
            let applied = apply_patch(&g, &proposal.patch_set).unwrap().graph;
            assert_eq!(applied.node(&node.id), Some(node));
        }
        // Payloads are exactly the configured typed content.
        let by_id: BTreeMap<String, &Node> = result
            .outcomes
            .iter()
            .map(|o| (o.element_ref.to_string(), added(o).1))
            .collect();
        assert_eq!(
            serde_json::to_value(&by_id["rule:balance"].payload).unwrap()["data"],
            json!({"name": "Balance", "rule_kind": "business", "condition_expr": null, "action_expr": null})
        );
        assert_eq!(
            by_id["dt:approval"].payload,
            NodePayload::DecisionTable(encode_decision_table("Approval", &spec()).unwrap())
        );
    }

    #[test]
    fn evidence_is_the_sorted_union_of_accepted_grounding() {
        let result = produce(&graph(), &draft()).unwrap();
        let evidence = |element: &str| -> Vec<Id> {
            let outcome = result
                .outcomes
                .iter()
                .find(|o| o.element_ref == id(element))
                .unwrap();
            added(outcome)
                .1
                .evidence
                .iter()
                .map(|e| e.as_id().clone())
                .collect()
        };
        let mut union = vec![fragment(1), fragment(2), fragment(3)];
        union.sort();
        assert_eq!(evidence("stakeholder:hr"), union);
        let mut a = vec![fragment(1), fragment(2)];
        a.sort();
        assert_eq!(evidence("actor:employee"), a);
        assert!(evidence("actor:manager").is_empty());
        assert!(evidence("brole:approver").is_empty());
        let mut b = vec![fragment(2), fragment(3)];
        b.sort();
        assert_eq!(evidence("calendar:nl"), b);
    }

    #[test]
    fn calendar_week_pattern_comes_from_the_runtime_binding() {
        let result = produce(&graph(), &draft()).unwrap();
        let outcome = result
            .outcomes
            .iter()
            .find(|o| o.element_ref == id("calendar:nl"))
            .unwrap();
        let NodePayload::Calendar(calendar) = &added(outcome).1.payload else {
            panic!("not a calendar");
        };
        assert_eq!(
            calendar.week_pattern,
            json!({"weekend": ["sunday", "saturday"]})
        );
        assert_eq!(calendar.time_zone, "Europe/Amsterdam");
        assert_eq!(calendar.region.as_deref(), Some("NL"));
        assert_eq!(calendar.holiday_source, None);
        // An unknown runtime calendar is rejected, never guessed.
        let err = produce_directory(&graph(), &draft(), &[], &audit()).unwrap_err();
        assert_eq!(
            err,
            DirectoryError::UnknownCalendar {
                element: id("calendar:nl"),
                calendar_ref: id("cal:office"),
            }
        );
        let twice = [bindings(), bindings()].concat();
        assert!(matches!(
            produce_directory(&graph(), &draft(), &twice, &audit()),
            Err(DirectoryError::DuplicateCalendarBinding(_))
        ));
    }

    #[test]
    fn grounding_must_be_accepted_requirements() {
        for (grounding, label) in [
            ("req:missing", "missing"),
            ("term:leave", "wrong type"),
            ("req:p", "Proposed"),
        ] {
            let mut j = draft_json();
            j["actors"][0]["grounding_refs"] = json!([grounding]);
            let d: DirectoryDraft = serde_json::from_value(j).unwrap();
            assert_eq!(
                produce(&graph(), &d).unwrap_err(),
                DirectoryError::InvalidGrounding {
                    element: id("actor:manager"),
                    requirement: id(grounding),
                },
                "{label}"
            );
        }
    }

    #[test]
    fn structural_errors_are_rejected() {
        // Duplicate element IDs, also across families.
        let mut j = draft_json();
        j["rules"][0]["id"] = json!("actor:manager");
        let d: DirectoryDraft = serde_json::from_value(j).unwrap();
        assert_eq!(
            d.validate().unwrap_err(),
            DirectoryError::DuplicateElement(id("actor:manager"))
        );
        // Duplicate grounding.
        let mut j = draft_json();
        j["actors"][1]["grounding_refs"] = json!(["req:a", "req:a"]);
        let d: DirectoryDraft = serde_json::from_value(j).unwrap();
        assert_eq!(
            d.validate().unwrap_err(),
            DirectoryError::DuplicateGrounding {
                element: id("actor:employee"),
                requirement: id("req:a"),
            }
        );
        // A default output has no PSG encoding.
        let mut with_default = spec();
        with_default.default_output = Some(vec![DecisionOutputCell::Bool { value: false }]);
        let mut j = draft_json();
        j["decision_tables"][0]["spec"] = serde_json::to_value(&with_default).unwrap();
        let d: DirectoryDraft = serde_json::from_value(j).unwrap();
        assert!(matches!(
            d.validate().unwrap_err(),
            DirectoryError::InvalidDecisionTable { .. }
        ));
        // Strict wire: unknown and missing fields, malformed IDs and payload values.
        let mut invalid = Vec::new();
        let mut j = draft_json();
        j["extra"] = json!(1);
        invalid.push(j);
        let mut j = draft_json();
        j.as_object_mut().unwrap().remove("rules");
        invalid.push(j);
        let mut j = draft_json();
        j["actors"][0]["nickname"] = json!("x");
        invalid.push(j);
        let mut j = draft_json();
        j["actors"][0]["id"] = json!("Not An Id");
        invalid.push(j);
        let mut j = draft_json();
        j["actors"][0]["payload"]["actor_kind"] = json!("robot");
        invalid.push(j);
        let mut j = draft_json();
        j["calendars"][0].as_object_mut().unwrap().remove("region");
        invalid.push(j);
        let mut j = draft_json();
        j["decision_tables"][0]["spec"]["extra"] = json!(true);
        invalid.push(j);
        for j in invalid {
            assert!(
                serde_json::from_value::<DirectoryDraft>(j.clone()).is_err(),
                "{j}"
            );
        }
    }

    #[test]
    fn existing_elements_are_reported_and_conflicts_rejected() {
        let g = graph();
        let first = produce(&g, &draft()).unwrap();
        // Apply every proposal: re-producing reports each element as Existing.
        let mut applied = g.clone();
        for outcome in &first.outcomes {
            let (proposal, _) = added(outcome);
            let mut patch_set = proposal.patch_set.clone();
            patch_set.base_semantic_hash = applied.semantic_hash().unwrap();
            applied = apply_patch(&applied, &patch_set).unwrap().graph;
        }
        let again = produce(&applied, &draft()).unwrap();
        assert!(again
            .outcomes
            .iter()
            .all(|o| o.disposition == DirectoryDisposition::Existing));
        // A different persisted element at a configured ID is a conflict.
        let mut j = draft_json();
        j["actors"][0]["payload"]["name"] = json!("Line manager");
        let d: DirectoryDraft = serde_json::from_value(j).unwrap();
        assert!(matches!(
            produce(&applied, &d),
            Err(DirectoryError::ElementConflict { .. })
        ));
    }

    #[test]
    fn production_is_deterministic() {
        let g = graph();
        let a = produce(&g, &draft()).unwrap();
        // Reversed configuration arrays produce the identical result.
        let mut j = draft_json();
        j["actors"].as_array_mut().unwrap().reverse();
        let b = produce(&g, &serde_json::from_value(j).unwrap()).unwrap();
        assert_eq!(a, b);
        let c = produce(&g, &draft()).unwrap();
        assert_eq!(a, c);
        let bytes = |r: &DirectoryResult| -> Vec<Vec<u8>> {
            r.outcomes
                .iter()
                .map(|o| plumb_core::to_canonical_json(added(o).0).unwrap())
                .collect()
        };
        assert_eq!(bytes(&a), bytes(&c));
        // Audit is caller input: a different instant changes the node audit, not the payload.
        let later = produce_directory(
            &g,
            &draft(),
            &bindings(),
            &DirectoryAudit {
                created_by: id(STAGE_ACTOR),
                created_at: ts(AT),
            },
        )
        .unwrap();
        for (x, y) in a.outcomes.iter().zip(&later.outcomes) {
            assert_eq!(added(x).1.payload, added(y).1.payload);
        }
    }

    // ------------------------------------------------------------------ outcome selections

    /// The directory graph plus an Accepted Operation and Outcomes of every kind.
    fn outcome_graph() -> Graph {
        let g = graph();
        let mut nodes: Vec<Node> = g.nodes().values().cloned().collect();
        let operation = |node_id: &str, status: ElementStatus| {
            node(
                node_id,
                status,
                NodePayload::Operation(
                    serde_json::from_value(json!({
                        "name": node_id, "operation_kind": "command", "input_schema_ref": null,
                        "output_schema_ref": null, "preconditions": null, "postconditions": null,
                        "idempotency": null, "transaction_semantics": null
                    }))
                    .unwrap(),
                ),
            )
        };
        let outcome = |node_id: &str, kind: &str, status: ElementStatus| {
            node(
                node_id,
                status,
                NodePayload::Outcome(
                    serde_json::from_value(json!({"name": node_id, "outcome_kind": kind})).unwrap(),
                ),
            )
        };
        nodes.extend([
            operation("op:approve", Accepted),
            operation("op:submit", Accepted),
            operation("op:draft", Proposed),
            outcome("outcome:ok", "success", Accepted),
            outcome("outcome:partial", "partial", Accepted),
            outcome("outcome:low-balance", "business_failure", Accepted),
            outcome("outcome:crash", "technical_failure", Accepted),
            outcome("outcome:proposed", "success", Proposed),
        ]);
        Graph::new(id(PROJECT), id(PROFILE), nodes, vec![]).unwrap()
    }

    fn selection_draft(selections: Value) -> DirectoryDraft {
        let mut j = draft_json();
        for family in [
            "stakeholders",
            "actors",
            "business_roles",
            "calendars",
            "rules",
            "decision_tables",
        ] {
            j[family] = json!([]);
        }
        j["outcome_selections"] = selections;
        serde_json::from_value(j).unwrap()
    }

    fn extension_patch(outcome: &DirectoryOutcome) -> (&plumb_patch::Proposal, &Value) {
        let DirectoryDisposition::Proposed { proposal } = &outcome.disposition else {
            panic!("{} is not proposed", outcome.element_ref);
        };
        let SemanticPatch::SetNodeExtension { target, key, value } = &proposal.patch_set.patch
        else {
            panic!("not exactly one SetNodeExtension");
        };
        assert_eq!(target.id, outcome.element_ref);
        assert_eq!(key.as_str(), "plumb_sim:outcome_selection");
        (proposal, value)
    }

    #[test]
    fn outcome_selections_become_human_confirm_extension_proposals() {
        let g = outcome_graph();
        let d = selection_draft(json!([
            {"outcome_ref": "outcome:low-balance", "operation_ref": "op:submit",
             "on_failures": ["precondition_rejected", "authorization_denied"]},
            {"outcome_ref": "outcome:ok", "operation_ref": "op:submit", "when": "true"},
            {"outcome_ref": "outcome:ok", "operation_ref": "op:approve", "when": "1 > 0"},
        ]));
        let result = produce(&g, &d).unwrap();
        let ids: Vec<String> = result
            .outcomes
            .iter()
            .map(|o| o.element_ref.to_string())
            .collect();
        assert_eq!(ids, ["outcome:low-balance", "outcome:ok"]);
        let mut applied = g.clone();
        for outcome in &result.outcomes {
            assert_eq!(outcome.node_type, NodeType::Outcome);
            let (proposal, value) = extension_patch(outcome);
            assert_eq!(proposal.materiality, ProposalMateriality::Semantic);
            assert_eq!(proposal.acceptance_policy, AcceptancePolicy::HumanConfirm);
            assert_eq!(proposal.stage, StageId::S2);
            let expected = match outcome.element_ref.as_str() {
                "outcome:ok" => json!({"version": 1, "selections": [
                    {"operation_ref": "op:approve", "when": "1 > 0"},
                    {"operation_ref": "op:submit", "when": "true"}]}),
                _ => json!({"version": 1, "selections": [
                    {"operation_ref": "op:submit",
                     "on_failures": ["authorization_denied", "precondition_rejected"]}]}),
            };
            assert_eq!(value, &expected);
            // The material reaches the graph only through the governed patch.
            let mut patch_set = proposal.patch_set.clone();
            patch_set.base_semantic_hash = applied.semantic_hash().unwrap();
            applied = apply_patch(&applied, &patch_set).unwrap().graph;
            let selection = plumb_functional::OutcomeSelection::from_value(
                &outcome.element_ref,
                &applied.node(&outcome.element_ref).unwrap().extensions
                    [&"plumb_sim:outcome_selection".parse().unwrap()],
            )
            .unwrap();
            selection.validate(&applied, &outcome.element_ref).unwrap();
        }
        // Re-producing on the applied graph proposes nothing.
        let again = produce(&applied, &d).unwrap();
        assert!(again
            .outcomes
            .iter()
            .all(|o| o.disposition == DirectoryDisposition::Existing));
        // Configuration order never matters.
        let reversed = selection_draft(json!([
            {"outcome_ref": "outcome:ok", "operation_ref": "op:approve", "when": "1 > 0"},
            {"outcome_ref": "outcome:ok", "operation_ref": "op:submit", "when": "true"},
            {"outcome_ref": "outcome:low-balance", "operation_ref": "op:submit",
             "on_failures": ["authorization_denied", "precondition_rejected"]},
        ]));
        assert_eq!(produce(&g, &reversed).unwrap(), result);
        // A directory without the family is still a valid configuration.
        let mut j = draft_json();
        j.as_object_mut().unwrap().remove("outcome_selections");
        assert!(serde_json::from_value::<DirectoryDraft>(j).is_ok());
    }

    #[test]
    fn invalid_outcome_selections_are_rejected() {
        let g = outcome_graph();
        let cases = [
            // when only on success or partial Outcomes.
            json!([{"outcome_ref": "outcome:low-balance", "operation_ref": "op:submit", "when": "true"}]),
            // on_failures only on business_failure Outcomes.
            json!([{"outcome_ref": "outcome:partial", "operation_ref": "op:submit",
                    "on_failures": ["precondition_rejected"]}]),
            // technical_failure Outcomes are never selected.
            json!([{"outcome_ref": "outcome:crash", "operation_ref": "op:submit", "when": "true"}]),
            // Existing Accepted Outcomes and Operations only.
            json!([{"outcome_ref": "outcome:proposed", "operation_ref": "op:submit", "when": "true"}]),
            json!([{"outcome_ref": "outcome:none", "operation_ref": "op:submit", "when": "true"}]),
            json!([{"outcome_ref": "outcome:ok", "operation_ref": "op:draft", "when": "true"}]),
            // Exactly one of when and on_failures.
            json!([{"outcome_ref": "outcome:ok", "operation_ref": "op:submit"}]),
            json!([{"outcome_ref": "outcome:low-balance", "operation_ref": "op:submit",
                    "when": "true", "on_failures": ["precondition_rejected"]}]),
            // One entry per Operation and Outcome; unique, non-empty failure kinds.
            json!([{"outcome_ref": "outcome:ok", "operation_ref": "op:submit", "when": "true"},
                   {"outcome_ref": "outcome:ok", "operation_ref": "op:submit", "when": "false"}]),
            json!([{"outcome_ref": "outcome:low-balance", "operation_ref": "op:submit",
                    "on_failures": ["precondition_rejected", "precondition_rejected"]}]),
            json!([{"outcome_ref": "outcome:low-balance", "operation_ref": "op:submit",
                    "on_failures": []}]),
            // when is PlumbExpr source.
            json!([{"outcome_ref": "outcome:ok", "operation_ref": "op:submit", "when": "((("}]),
        ];
        for case in cases {
            let d = selection_draft(case.clone());
            assert!(
                matches!(
                    produce(&g, &d),
                    Err(DirectoryError::InvalidOutcomeSelection { .. })
                ),
                "{case}"
            );
        }
        // Unknown failure kinds and unknown keys are wire errors.
        for case in [
            json!([{"outcome_ref": "outcome:low-balance", "operation_ref": "op:submit",
                    "on_failures": ["postcondition_violation"]}]),
            json!([{"outcome_ref": "outcome:ok", "operation_ref": "op:submit", "when": "true",
                    "extra": 1}]),
        ] {
            let mut j = draft_json();
            j["outcome_selections"] = case.clone();
            assert!(
                serde_json::from_value::<DirectoryDraft>(j).is_err(),
                "{case}"
            );
        }
    }
}
