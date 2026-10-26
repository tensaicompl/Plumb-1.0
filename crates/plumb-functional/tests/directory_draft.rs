//! The inferred semantic directory (goal-approach deviation, any-domain run): request binding,
//! per-element validation of the drafted Actors, BusinessRoles, Rules and DecisionTables, the
//! configuration-supplied Calendars and Stakeholders, and production into HUMAN_CONFIRM
//! proposals.

mod directory_draft_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, Timestamp};
    use plumb_functional::*;
    use plumb_inference::{InferenceArtifact, InferenceRequest, ProviderPolicy};
    use plumb_psg::{
        Actor, ActorKind, AuditMeta, ElementStatus, Graph, Modality, Node, NodePayload, NodeType,
        Requirement, RequirementKind, RequirementLevel,
    };
    use serde_json::{json, Value};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s2-directory-draft.md");
    const SCHEMA: &str = include_str!("../../../schemas/inference/s2-directory-draft.schema.json");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:fe2c284b1031505fc3431d157a30c033d7f9bd132bcccddd9841e07096a03ebc";
    const SCHEMA_HASH: &str =
        "sha256:13f0260262ed0a6f374de5f0306be19f69516a2b4c933ed98ce6e050ed618e49";
    const CONTEXT_HASH: &str =
        "sha256:133de487f005a4814a42f38f69e522f6e768d3944dc94e67f3f81ac8a3f3718a";

    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const APPLY: &str = "req:00000000000000a1";
    const DECIDE: &str = "req:00000000000000b2";

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn policy() -> ProviderPolicy {
        ProviderPolicy {
            provider: "mock".to_owned(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn node(node_id: &str, payload: NodePayload, status: ElementStatus) -> Node {
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
            audit: AuditMeta::new(id("actor:human"), ts(AT), None, None).unwrap(),
        }
    }

    fn requirement(node_id: &str, statement: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            NodePayload::Requirement(Requirement {
                statement: statement.to_owned(),
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
            status,
        )
    }

    fn graph(extra: Vec<Node>) -> Graph {
        let mut nodes = vec![
            requirement(
                APPLY,
                "A resident shall be able to apply for a parking permit.",
                ElementStatus::Accepted,
            ),
            requirement(
                DECIDE,
                "A permit officer shall decide a submitted application.",
                ElementStatus::Accepted,
            ),
            requirement(
                "req:00000000000000c3",
                "A warden shall check a permit.",
                ElementStatus::Proposed,
            ),
        ];
        nodes.extend(extra);
        Graph::new(
            id("project:parking"),
            id("profile:plumb-software-2026.1"),
            nodes,
            Vec::new(),
        )
        .unwrap()
    }

    fn artifact(request: &InferenceRequest, output: Value) -> InferenceArtifact {
        let raw_response = to_canonical_json(&output).unwrap();
        let validated_output = CanonicalJson::new(output);
        InferenceArtifact {
            request_hash: request.id.clone(),
            provider: request.provider_policy.provider.clone(),
            model: "mock-model".to_owned(),
            parameters: CanonicalJson::new(json!({})),
            raw_response_hash: Hash::content_sha256(&raw_response),
            validated_output_hash: validated_output.content_hash().unwrap(),
            validated_output,
        }
    }

    fn output(actors: Value, roles: Value, rules: Value, tables: Value) -> Value {
        json!({"version": 1, "actors": actors, "business_roles": roles, "rules": rules,
               "decision_tables": tables})
    }

    fn seed() -> DirectoryDraftSeed {
        DirectoryDraftSeed {
            calendar_refs: vec![id("cal:city")],
            stakeholders: vec![DirectoryDraftStakeholder {
                id: id("stakeholder:owner"),
                name: "Product Owner".to_owned(),
                stakeholder_kind: "business".to_owned(),
            }],
        }
    }

    fn eligibility_table(name: &str, rows: Value) -> Value {
        json!({
            "name": name,
            "grounding_refs": [APPLY],
            "hit_policy": "UNIQUE",
            "inputs": [{"name": "vehicle_kind", "type": "enum", "members": ["Electric", "Combustion"], "scale": null}],
            "outputs": [{"name": "discount_percent", "type": "decimal", "members": [], "scale": 2}],
            "rows": rows
        })
    }

    fn discount_rows() -> Value {
        json!([
            {"inputs": [{"cell": "enum_set", "value": null, "members": ["Electric"]}],
             "outputs": [{"cell": "decimal", "value": "50.00"}]},
            {"inputs": [{"cell": "enum_set", "value": null, "members": ["Combustion"]}],
             "outputs": [{"cell": "decimal", "value": "0.00"}]}
        ])
    }

    fn draft(graph: &Graph, out: Option<Value>) -> DirectoryDraftResult {
        let request = build_directory_draft_request(graph, policy()).unwrap();
        let artifact = out.map(|o| artifact(&request.request, o));
        draft_directory(graph, &request, artifact.as_ref()).unwrap()
    }

    fn reasons(result: &DirectoryDraftResult) -> Vec<(String, String)> {
        result
            .issues
            .iter()
            .map(|i| (i.name.clone(), i.reason.clone()))
            .collect()
    }

    #[test]
    fn directory_draft_request_binds_context_prompt_and_schema() {
        let g = graph(vec![node(
            "actor:clerk",
            NodePayload::Actor(Actor {
                name: "Clerk".to_owned(),
                actor_kind: ActorKind::Human,
            }),
            ElementStatus::Proposed,
        )]);
        let r = build_directory_draft_request(&g, policy()).unwrap();
        assert_eq!(r.request.task_kind, DIRECTORY_DRAFT_TASK_KIND);
        assert_eq!(r.request.prompt_template_hash.as_str(), PROMPT_HASH);
        assert_eq!(r.request.schema_hash.as_str(), SCHEMA_HASH);
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
        assert_eq!(r.request.context_hash.as_str(), CONTEXT_HASH);
        // Only Accepted requirements; live directory elements are listed as existing.
        let refs: Vec<&str> = r
            .context
            .requirements
            .iter()
            .map(|q| q.requirement_ref.as_str())
            .collect();
        assert_eq!(refs, [APPLY, DECIDE]);
        assert_eq!(r.context.existing.len(), 1);
        assert_eq!(r.context.existing[0].name, "Clerk");
        let input: Vec<&str> = r.request.input_refs.iter().map(Id::as_str).collect();
        assert_eq!(input, ["actor:clerk", APPLY, DECIDE]);
        let material = r.inference_material().unwrap();
        assert_eq!(material.prompt_template, PROMPT);
        assert_eq!(material.schema_json, SCHEMA.as_bytes());
        material.validate_for(&r.request).unwrap();
    }

    #[test]
    fn directory_draft_needs_an_accepted_requirement() {
        let g = Graph::new(
            id("project:parking"),
            id("profile:plumb-software-2026.1"),
            vec![requirement(
                APPLY,
                "A resident shall apply.",
                ElementStatus::Proposed,
            )],
            Vec::new(),
        )
        .unwrap();
        assert!(matches!(
            build_directory_draft_request(&g, policy()),
            Err(DirectoryDraftError::InvalidInput { .. })
        ));
    }

    #[test]
    fn directory_draft_turns_drafted_elements_into_human_confirm_proposals() {
        let g = graph(Vec::new());
        let result = draft(
            &g,
            Some(output(
                json!([{"name": "Resident", "actor_kind": "human", "grounding_refs": [APPLY]},
                       {"name": "Payment Service", "actor_kind": "external_system",
                        "grounding_refs": [DECIDE, APPLY, APPLY]}]),
                json!([{"name": "Permit Officer", "grounding_refs": [DECIDE]}]),
                json!([{"name": "HouseholdPermitLimit", "rule_kind": "constraint",
                        "grounding_refs": [APPLY]}]),
                json!([eligibility_table("EmissionDiscount", discount_rows())]),
            )),
        );
        assert!(result.issues.is_empty(), "{:?}", result.issues);
        let d = &result.draft;
        let ids = |ids: Vec<&Id>| ids.into_iter().map(|i| i.to_string()).collect::<Vec<_>>();
        assert_eq!(
            ids(d.actors.iter().map(|e| &e.id).collect()),
            ["actor:resident", "actor:payment-service"]
        );
        assert_eq!(d.actors[1].grounding_refs, [id(APPLY), id(DECIDE)]);
        assert_eq!(
            ids(d.business_roles.iter().map(|e| &e.id).collect()),
            ["brole:permit-officer"]
        );
        assert_eq!(
            ids(d.rules.iter().map(|e| &e.id).collect()),
            ["rule:householdpermitlimit"]
        );
        assert_eq!(d.decision_tables[0].id, id("dt:emissiondiscount"));
        assert!(d.calendars.is_empty() && d.stakeholders.is_empty());
        let produced = produce_directory(
            &g,
            d,
            &[DirectoryCalendarBinding {
                calendar_ref: id("cal:city"),
                weekend: vec!["saturday".to_owned(), "sunday".to_owned()],
                coverage: None,
            }],
            &DirectoryAudit {
                created_by: id("agent:compiler"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        let types: BTreeSet<NodeType> = produced.outcomes.iter().map(|o| o.node_type).collect();
        assert_eq!(produced.outcomes.len(), 5);
        assert!(types.contains(&NodeType::DecisionTable));
        for outcome in produced.outcomes {
            assert!(matches!(
                outcome.disposition,
                DirectoryDisposition::Proposed { .. }
            ));
        }
    }

    #[test]
    fn directory_draft_drops_unusable_elements_and_keeps_the_rest() {
        let g = graph(vec![node(
            "actor:clerk",
            NodePayload::Actor(Actor {
                name: "Clerk".to_owned(),
                actor_kind: ActorKind::Human,
            }),
            ElementStatus::Accepted,
        )]);
        let overlapping = json!([
            {"inputs": [{"cell": "any", "value": null, "members": []}], "outputs": [{"cell": "decimal", "value": "50.00"}]},
            {"inputs": [{"cell": "enum_set", "value": null, "members": ["Combustion"]}],
             "outputs": [{"cell": "decimal", "value": "0.00"}]}
        ]);
        let unknown_member = json!([
            {"inputs": [{"cell": "enum_set", "value": null, "members": ["Diesel"]}],
             "outputs": [{"cell": "decimal", "value": "0.00"}]}
        ]);
        let result = draft(
            &g,
            Some(output(
                json!([
                    {"name": "clerk", "actor_kind": "human", "grounding_refs": [APPLY]},
                    {"name": "Warden", "actor_kind": "human", "grounding_refs": ["req:00000000000000c3"]},
                    {"name": "Resident", "actor_kind": "human", "grounding_refs": [APPLY]},
                    {"name": "RESIDENT", "actor_kind": "human", "grounding_refs": [APPLY]},
                    {"name": "***", "actor_kind": "human", "grounding_refs": [APPLY]}
                ]),
                json!([]),
                json!([]),
                json!([
                    eligibility_table("Overlap", overlapping),
                    eligibility_table("Unknown", unknown_member),
                    eligibility_table("Discount", discount_rows())
                ]),
            )),
        );
        assert_eq!(result.draft.actors.len(), 1);
        assert_eq!(result.draft.actors[0].id, id("actor:resident"));
        let reasons = reasons(&result);
        assert_eq!(result.draft.decision_tables.len(), 1);
        let reason_of = |name: &str| {
            reasons
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, r)| r.clone())
                .unwrap_or_default()
        };
        assert!(reason_of("clerk").contains("exists"), "{reasons:?}");
        assert!(
            reason_of("Warden").contains("no supplied requirement"),
            "{reasons:?}"
        );
        assert!(
            reason_of("RESIDENT").contains("drafted twice"),
            "{reasons:?}"
        );
        assert!(reason_of("***").contains("no element ID"), "{reasons:?}");
        assert!(reason_of("Overlap").contains("overlap"), "{reasons:?}");
        assert!(reason_of("Unknown").contains("malformed"), "{reasons:?}");
    }

    #[test]
    fn directory_draft_without_inference_drafts_nothing() {
        let result = draft(&graph(Vec::new()), None);
        assert!(result.issues.is_empty());
        assert!(result.draft.actors.is_empty() && result.draft.decision_tables.is_empty());
    }

    #[test]
    fn seed_directory_proposes_the_configured_calendars_and_stakeholders() {
        let d = seed_directory(&graph(Vec::new()), &seed());
        assert_eq!(d.calendars[0].id, id("calendar:city"));
        assert_eq!(d.calendars[0].calendar_ref, id("cal:city"));
        assert_eq!(d.calendars[0].time_zone, "UTC");
        assert_eq!(d.stakeholders[0].id, id("stakeholder:owner"));
        assert!(d.actors.is_empty() && d.rules.is_empty());
        let produced = produce_directory(
            &graph(Vec::new()),
            &d,
            &[DirectoryCalendarBinding {
                calendar_ref: id("cal:city"),
                weekend: vec!["saturday".to_owned(), "sunday".to_owned()],
                coverage: None,
            }],
            &DirectoryAudit {
                created_by: id("agent:compiler"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        assert_eq!(produced.outcomes.len(), 2);
    }

    #[test]
    fn directory_draft_rejects_schema_invalid_or_misbound_output() {
        let g = graph(Vec::new());
        let request = build_directory_draft_request(&g, policy()).unwrap();
        let bad = artifact(&request.request, json!({"version": 1, "actors": []}));
        assert!(matches!(
            draft_directory(&g, &request, Some(&bad)),
            Err(DirectoryDraftError::SchemaInvalid { .. })
        ));
        let other = build_directory_draft_request(
            &graph(vec![requirement(
                "req:00000000000000d4",
                "A clerk shall print.",
                ElementStatus::Accepted,
            )]),
            policy(),
        )
        .unwrap();
        let misbound = artifact(
            &other.request,
            output(json!([]), json!([]), json!([]), json!([])),
        );
        assert!(matches!(
            draft_directory(&g, &request, Some(&misbound)),
            Err(DirectoryDraftError::InvalidInferenceArtifact { .. })
        ));
        // A request built on another graph is never evaluated against this one.
        assert!(matches!(
            draft_directory(&g, &other, None),
            Err(DirectoryDraftError::InvalidInput { .. })
        ));
    }

    #[test]
    fn directory_draft_skips_configured_elements_already_in_the_graph() {
        let g = graph(vec![node(
            "stakeholder:owner",
            NodePayload::Stakeholder(plumb_psg::Stakeholder {
                name: "Product Owner".to_owned(),
                stakeholder_kind: "business".to_owned(),
                organization: None,
                responsibilities: None,
                contact_ref: None,
            }),
            ElementStatus::Accepted,
        )]);
        let d = seed_directory(&g, &seed());
        assert!(d.stakeholders.is_empty());
        assert_eq!(d.calendars.len(), 1);
    }
}
