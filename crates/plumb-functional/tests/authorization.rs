//! S2.8 contract tests for deterministic authorization compilation: separate business and
//! security role namespaces, structured drafts, deterministic identities, Permission bundles,
//! assignments, A-inherits-B hierarchy and cycles, static separation of duty, reconciliation,
//! the compound proposal and the HR role_specs reference.
//!
//! Golden values were computed independently with Python `hashlib` over RFC 8785 JSON, never
//! with the helpers under test (the helpers are used only to link draft elements). Graphs are
//! synthetic; the HR test adapts the immutable fixture role_specs and claims nothing beyond it.

mod authorization_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::{Id, StageId, Timestamp};
    use plumb_functional::authorization::*;
    use plumb_import::{import_plain_text, ImportAudit};
    use plumb_patch::{
        apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        Actor, ActorKind, Attribute, AuditMeta, BusinessRole, Edge, ElementStatus, Entity,
        EvidenceRef, Graph, Node, NodePayload, NodeType, Operation, OperationKind, Permission,
        PolicyCondition, Principal, RelationKind, RelationProperties, ResourceScope, SecurityRole,
        SeparationConstraint, SeparationConstraintKind,
    };

    use ElementStatus::{Accepted, Proposed, Rejected};
    use SeparationConstraintKind as K;

    const HR_CONTRACT: &str = include_str!("../../../fixtures/hr-leave/fixture-contract.yaml");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const APPROVER: &str = "security_role:9b40624a385db0a5";
    const REQUESTER: &str = "security_role:f60361b53e4e1f3a";
    const ALICE: &str = "principal:a04006b13b86f008";
    const SCOPE: &str = "resource_scope:21bbbf77e26ba1ed";
    const CONDITION: &str = "policy_condition:fb5f30e11c524f09";
    const PERMISSION: &str = "permission:08fa2630fb61217b";
    const SOD: &str = "separation_constraint:8283f1860d9a11ab";
    const ASSIGNED: &str = "rel:849d39fff9d6930d";
    const INHERITS: &str = "rel:3631b95d4198843c";
    const GRANTS: &str = "rel:19cecd17fa422dc9";
    const PERMITS: &str = "rel:0d32b1d2fe67b2f1";
    const SCOPED: &str = "rel:8b4c69941f40d240";
    const CONDITIONED: &str = "rel:20e836e6c620e189";
    const PERMISSION_NAME: &str = "Approver -> ApproveRequest [direct_reports]";
    const EU: &str = "region == \"EU\"";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const OP: &str = "operation:approve";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn meta() -> AuditMeta {
        AuditMeta::new(id("actor:human"), ts(AT), None, None).unwrap()
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
                "rel:fx-{}-{}",
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

    fn operation(node_id: &str, status: ElementStatus, name: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::Operation(Operation {
                name: name.into(),
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

    fn security_role(node_id: &str, status: ElementStatus, name: &str) -> Node {
        node(
            node_id,
            status,
            NodePayload::SecurityRole(SecurityRole {
                name: name.into(),
                description: None,
            }),
        )
    }

    fn imported() -> (Vec<Node>, Vec<EvidenceRef>) {
        let imported = import_plain_text(
            "policy.txt",
            b"Approvers approve requests of their direct reports.\n\nRequesters submit requests.",
            &ImportAudit {
                created_by: id("actor:importer"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        let refs = imported
            .fragments
            .iter()
            .map(|f| EvidenceRef::from(f.id.clone()))
            .collect();
        let mut nodes = vec![imported.source];
        nodes.extend(imported.fragments);
        (nodes, refs)
    }

    fn base_nodes() -> Vec<Node> {
        let (mut nodes, _) = imported();
        nodes.extend([
            operation(OP, Accepted, "ApproveRequest"),
            operation("operation:submit", Accepted, "SubmitRequest"),
            operation("operation:draft", Proposed, "DraftOperation"),
            node(
                "entity:request",
                Accepted,
                NodePayload::Entity(Entity {
                    name: "Request".into(),
                    description: None,
                    aggregate_root: None,
                }),
            ),
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
            node(
                "brole:manager",
                Accepted,
                NodePayload::BusinessRole(BusinessRole {
                    name: "Manager".into(),
                }),
            ),
            node(
                "actor:clerk",
                Accepted,
                NodePayload::Actor(Actor {
                    name: "Clerk".into(),
                    actor_kind: ActorKind::Human,
                }),
            ),
            node(
                "principal:bob",
                Accepted,
                NodePayload::Principal(Principal {
                    name: "Bob".into(),
                    principal_kind: "human".into(),
                }),
            ),
            node(
                "principal:pending",
                Proposed,
                NodePayload::Principal(Principal {
                    name: "Pending".into(),
                    principal_kind: "human".into(),
                }),
            ),
        ]);
        nodes
    }

    fn base_edges() -> Vec<Edge> {
        vec![edge(
            RelationKind::HasAttribute,
            "entity:request",
            "attr:amount",
            Accepted,
        )]
    }

    fn graph_of(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn base_graph() -> Graph {
        graph_of(base_nodes(), base_edges())
    }

    fn extend(graph: &Graph, nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        let mut all: Vec<Node> = graph.nodes().values().cloned().collect();
        all.extend(nodes);
        let mut all_edges: Vec<Edge> = graph.edges().values().cloned().collect();
        all_edges.extend(edges);
        graph_of(all, all_edges)
    }

    fn audit() -> AuthorizationAudit {
        AuthorizationAudit {
            created_by: id("agent:authorization"),
            created_at: ts(AT),
        }
    }

    fn role(name: &str) -> SecurityRoleDraft {
        SecurityRoleDraft {
            name: name.into(),
            description: None,
            evidence: vec![],
        }
    }

    fn role_id(name: &str) -> Id {
        security_role_id(&id(PROJECT), name).unwrap()
    }

    fn permission(
        role: &str,
        operation: &str,
        resource: &str,
        scope: &str,
        conditions: &[&str],
    ) -> PermissionDraft {
        PermissionDraft {
            security_role_ref: id(role),
            operation_ref: id(operation),
            resource_ref: id(resource),
            scope_kind: scope.into(),
            conditions: conditions
                .iter()
                .map(|c| PolicyConditionDraft {
                    expression: (*c).into(),
                    evidence: vec![],
                })
                .collect(),
            evidence: vec![],
        }
    }

    fn assign(subject: &str, role: &Id) -> RoleAssignmentDraft {
        RoleAssignmentDraft {
            subject_ref: id(subject),
            security_role_ref: role.clone(),
            evidence: vec![],
        }
    }

    fn inherit(role: &Id, inherited: &Id) -> RoleInheritanceDraft {
        RoleInheritanceDraft {
            role_ref: role.clone(),
            inherited_role_ref: inherited.clone(),
            evidence: vec![],
        }
    }

    fn constraint(kind: K, roles: &[&Id]) -> SeparationConstraintDraft {
        SeparationConstraintDraft {
            constraint_kind: kind,
            role_refs: roles.iter().map(|r| (*r).clone()).collect(),
            evidence: vec![],
        }
    }

    /// The golden draft: two roles, Alice, one permission with a condition, an assignment, an
    /// inheritance and a static SOD constraint.
    fn golden_draft() -> AuthorizationDraft {
        AuthorizationDraft {
            security_roles: vec![role("Approver"), role("Requester")],
            principals: vec![PrincipalDraft {
                name: "Alice".into(),
                principal_kind: "human".into(),
                evidence: vec![],
            }],
            permissions: vec![permission(APPROVER, OP, OP, "direct_reports", &[EU])],
            assignments: vec![assign(ALICE, &id(APPROVER))],
            inheritances: vec![inherit(&id(APPROVER), &id(REQUESTER))],
            separation_constraints: vec![constraint(
                K::StaticSeparationOfDuty,
                &[&id(APPROVER), &id(REQUESTER)],
            )],
        }
    }

    fn analyze(graph: &Graph, draft: &AuthorizationDraft) -> AuthorizationAnalysisResult {
        analyze_authorization(graph, draft, &audit()).unwrap()
    }

    fn issues(graph: &Graph, draft: &AuthorizationDraft) -> Vec<AuthorizationIssue> {
        let r = analyze(graph, draft);
        assert!(r.proposal.is_none(), "{:?}", r.issues);
        assert_eq!(r.disposition, AuthorizationDisposition::Rejected);
        r.issues
    }

    fn patches(p: &Proposal) -> &[SemanticPatch] {
        let SemanticPatch::Compound { patches } = &p.patch_set.patch else {
            panic!("not compound")
        };
        patches
    }

    fn added_nodes(p: &Proposal) -> Vec<&Node> {
        patches(p)
            .iter()
            .filter_map(|s| {
                if let SemanticPatch::AddNode { node } = s {
                    Some(node)
                } else {
                    None
                }
            })
            .collect()
    }

    fn added_edges(p: &Proposal) -> Vec<&Edge> {
        patches(p)
            .iter()
            .filter_map(|s| {
                if let SemanticPatch::AddEdge { edge } = s {
                    Some(edge)
                } else {
                    None
                }
            })
            .collect()
    }

    fn apply(graph: &Graph, p: &Proposal) -> Graph {
        let patch_set = PatchSet {
            base_semantic_hash: graph.semantic_hash().unwrap(),
            patch: p.patch_set.patch.clone(),
        };
        let g = apply_patch(graph, &patch_set).unwrap().graph;
        g.validate().unwrap();
        g
    }

    fn accept_all(graph: &Graph) -> Graph {
        let keep = |s: ElementStatus| if s == Proposed { Accepted } else { s };
        let nodes = graph
            .nodes()
            .values()
            .cloned()
            .map(|mut n| {
                if n.id.as_str() != "operation:draft" && n.id.as_str() != "principal:pending" {
                    n.status = keep(n.status);
                }
                n
            })
            .collect();
        let edges = graph
            .edges()
            .values()
            .cloned()
            .map(|mut e| {
                e.status = keep(e.status);
                e
            })
            .collect();
        graph_of(nodes, edges)
    }

    fn proposed(
        graph: &Graph,
        draft: &AuthorizationDraft,
    ) -> (AuthorizationAnalysisResult, Proposal) {
        let r = analyze(graph, draft);
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        let p = r.proposal.clone().expect("proposal");
        (r, p)
    }

    // ------------------------------------------------------------------ goldens and compilation

    #[test]
    fn authorization_golden_compilation() {
        let g = base_graph();
        let (r, p) = proposed(&g, &golden_draft());
        assert!(
            matches!(&r.disposition, AuthorizationDisposition::Proposed { proposal_ref } if *proposal_ref == p.id)
        );
        assert_eq!(
            (p.stage, p.materiality, p.acceptance_policy, p.confidence),
            (
                StageId::S2,
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
                None
            )
        );
        assert!(p.derivation_refs.is_empty());
        // Mandatory order: principals, roles, scopes, conditions, permissions, constraints, edges.
        let node_ids: Vec<&str> = added_nodes(&p).iter().map(|n| n.id.as_str()).collect();
        assert_eq!(
            node_ids,
            [ALICE, APPROVER, REQUESTER, SCOPE, CONDITION, PERMISSION, SOD]
        );
        let all: Vec<&str> = patches(&p)
            .iter()
            .map(|s| match s {
                SemanticPatch::AddNode { node } => node.id.as_str(),
                SemanticPatch::AddEdge { edge } => edge.id.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        let mut edge_ids = [ASSIGNED, INHERITS, GRANTS, PERMITS, SCOPED, CONDITIONED];
        edge_ids.sort();
        assert_eq!(all[7..], edge_ids[..]);
        for n in added_nodes(&p) {
            assert_eq!((n.status, n.revision), (Proposed, 1));
            assert!(n.derivations.is_empty() && n.extensions.is_empty());
        }
        for e in added_edges(&p) {
            assert_eq!(
                (e.status, e.revision, &e.properties),
                (Proposed, 1, &RelationProperties::None)
            );
            assert!(e.derivations.is_empty());
        }
        let payload = |nid: &str| {
            added_nodes(&p)
                .into_iter()
                .find(|n| n.id.as_str() == nid)
                .unwrap()
                .payload
                .clone()
        };
        assert_eq!(
            payload(PERMISSION),
            NodePayload::Permission(Permission {
                name: PERMISSION_NAME.into()
            })
        );
        assert_eq!(
            payload(SCOPE),
            NodePayload::ResourceScope(ResourceScope {
                resource_ref: id(OP),
                scope_kind: "direct_reports".into()
            })
        );
        assert_eq!(
            payload(CONDITION),
            NodePayload::PolicyCondition(PolicyCondition {
                expression: EU.into()
            })
        );
        assert_eq!(
            payload(ALICE),
            NodePayload::Principal(Principal {
                name: "Alice".into(),
                principal_kind: "human".into()
            })
        );
        assert_eq!(
            payload(SOD),
            NodePayload::SeparationConstraint(SeparationConstraint {
                constraint_kind: K::StaticSeparationOfDuty,
                role_refs: vec![id(APPROVER), id(REQUESTER)]
            })
        );
        let edge_of = |eid: &str| {
            added_edges(&p)
                .into_iter()
                .find(|e| e.id.as_str() == eid)
                .unwrap()
        };
        let ends = |eid: &str| {
            let e = edge_of(eid);
            (e.kind.clone(), e.from.to_string(), e.to.to_string())
        };
        assert_eq!(
            ends(ASSIGNED),
            (RelationKind::AssignedRole, ALICE.into(), APPROVER.into())
        );
        assert_eq!(
            ends(INHERITS),
            (
                RelationKind::InheritsRole,
                APPROVER.into(),
                REQUESTER.into()
            )
        );
        assert_eq!(
            ends(GRANTS),
            (RelationKind::Grants, APPROVER.into(), PERMISSION.into())
        );
        assert_eq!(
            ends(PERMITS),
            (RelationKind::Permits, PERMISSION.into(), OP.into())
        );
        assert_eq!(
            ends(SCOPED),
            (RelationKind::ScopedTo, PERMISSION.into(), SCOPE.into())
        );
        assert_eq!(
            ends(CONDITIONED),
            (
                RelationKind::ConditionedBy,
                PERMISSION.into(),
                CONDITION.into()
            )
        );
        // Dry-run plus exact Permission cardinality in the applied graph.
        let applied = apply(&g, &p);
        let perm = id(PERMISSION);
        let count = |kind: RelationKind, outgoing: bool| {
            let ids = if outgoing {
                applied.outgoing_edge_ids(&perm)
            } else {
                applied.incoming_edge_ids(&perm)
            };
            ids.iter()
                .filter(|e| applied.edge(e).unwrap().kind == kind)
                .count()
        };
        assert_eq!(
            (
                count(RelationKind::Grants, false),
                count(RelationKind::Permits, true),
                count(RelationKind::ScopedTo, true),
                count(RelationKind::ConditionedBy, true)
            ),
            (1, 1, 1, 1)
        );
        // The opaque condition round-trips exactly.
        assert_eq!(
            applied.node(&id(CONDITION)).unwrap().payload,
            NodePayload::PolicyCondition(PolicyCondition {
                expression: EU.into()
            })
        );
        // Static SOD over the candidate overlay: Alice holds Approver and, by inheritance, Requester.
        assert_eq!(
            r.static_sod_violations,
            vec![StaticSodViolation {
                constraint_ref: id(SOD),
                subject_ref: id(ALICE),
                conflicting_role_refs: vec![id(APPROVER), id(REQUESTER)]
            }]
        );
        assert!(r.hierarchy_cycles.is_empty() && r.unevaluated_separation_constraints.is_empty());
        // The violation does not remove the policy: the constraint is still proposed.
        assert!(added_nodes(&p).iter().any(|n| n.id.as_str() == SOD));
        // Evidence on drafts is carried into nodes and the proposal.
        let (_, refs) = imported();
        let mut draft = golden_draft();
        draft.security_roles[0].evidence = vec![refs[1].clone(), refs[0].clone()];
        let (_, p) = proposed(&g, &draft);
        let approver = added_nodes(&p)
            .into_iter()
            .find(|n| n.id.as_str() == APPROVER)
            .unwrap()
            .clone();
        let mut sorted = refs.clone();
        sorted.sort();
        assert_eq!(approver.evidence, sorted[..2].to_vec());
        assert_eq!(p.evidence_refs, sorted[..2].to_vec());
    }

    #[test]
    fn authorization_business_and_security_roles_stay_distinct() {
        let g = base_graph();
        let (_, p) = proposed(
            &g,
            &AuthorizationDraft {
                security_roles: vec![role("Manager")],
                ..AuthorizationDraft::default()
            },
        );
        let security = added_nodes(&p)[0].clone();
        let business = g.node(&id("brole:manager")).unwrap();
        assert_eq!(security.payload.node_type(), NodeType::SecurityRole);
        assert_eq!(business.payload.node_type(), NodeType::BusinessRole);
        assert_ne!(security.id, business.id);
        assert!(matches!(&security.payload, NodePayload::SecurityRole(r) if r.name == "Manager"));
        assert!(matches!(&business.payload, NodePayload::BusinessRole(r) if r.name == "Manager"));
        let applied = apply(&g, &p);
        assert_eq!(applied.node(&id("brole:manager")).unwrap(), business);
        assert_eq!(
            applied.edges().len(),
            g.edges().len(),
            "no mapping edge is invented"
        );
        // A BusinessRole is never a SecurityRole reference, assignment source or inheritance end.
        let wrong = |draft: AuthorizationDraft| issues(&g, &draft);
        let manager = role_id("Manager");
        assert!(wrong(AuthorizationDraft { security_roles: vec![role("Manager")], assignments: vec![assign("brole:manager", &manager)], ..Default::default() })
            .iter()
            .any(|i| matches!(i, AuthorizationIssue::WrongTargetType { field, .. } if field == "assignment.subject_ref")));
        assert!(wrong(AuthorizationDraft { assignments: vec![assign("actor:clerk", &id("brole:manager"))], ..Default::default() })
            .iter()
            .any(|i| matches!(i, AuthorizationIssue::WrongTargetType { field, .. } if field == "assignment.security_role_ref")));
        for endpoint in ["brole:manager", "actor:clerk", "principal:bob"] {
            for (a, b) in [
                (id(endpoint), manager.clone()),
                (manager.clone(), id(endpoint)),
            ] {
                let draft = AuthorizationDraft {
                    security_roles: vec![role("Manager")],
                    inheritances: vec![inherit(&a, &b)],
                    ..Default::default()
                };
                assert!(
                    wrong(draft)
                        .iter()
                        .any(|i| matches!(i, AuthorizationIssue::WrongTargetType { .. })),
                    "{endpoint}"
                );
            }
        }
        // The registry itself rejects BusinessRole -assigned_role-> SecurityRole.
        let bad = extend(&g, vec![security_role("secrole:x", Accepted, "X")], vec![]);
        let mut nodes: Vec<Node> = bad.nodes().values().cloned().collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        let mut edges: Vec<Edge> = bad.edges().values().cloned().collect();
        edges.push(edge(
            RelationKind::AssignedRole,
            "brole:manager",
            "secrole:x",
            Accepted,
        ));
        assert!(Graph::new(id(PROJECT), id(PROFILE), nodes, edges).is_err());
    }

    // ------------------------------------------------------------------ validation

    #[test]
    fn authorization_draft_validation() {
        let g = base_graph();
        let approver = id(APPROVER);
        let with_role = |mut d: AuthorizationDraft| {
            d.security_roles.push(role("Approver"));
            d
        };
        let has = |d: AuthorizationDraft, f: fn(&AuthorizationIssue) -> bool| {
            let i = issues(&g, &d);
            assert!(i.iter().any(f), "{i:?}");
        };
        has(
            AuthorizationDraft {
                security_roles: vec![role(" Approver")],
                ..Default::default()
            },
            |i| matches!(i, AuthorizationIssue::InvalidName { .. }),
        );
        has(
            AuthorizationDraft {
                security_roles: vec![SecurityRoleDraft {
                    name: "A".into(),
                    description: Some("".into()),
                    evidence: vec![],
                }],
                ..Default::default()
            },
            |i| matches!(i, AuthorizationIssue::InvalidName { .. }),
        );
        has(
            AuthorizationDraft {
                principals: vec![PrincipalDraft {
                    name: "Al\u{1}ice".into(),
                    principal_kind: "human".into(),
                    evidence: vec![],
                }],
                ..Default::default()
            },
            |i| matches!(i, AuthorizationIssue::InvalidName { .. }),
        );
        has(
            AuthorizationDraft {
                principals: vec![PrincipalDraft {
                    name: "Alice".into(),
                    principal_kind: " ".into(),
                    evidence: vec![],
                }],
                ..Default::default()
            },
            |i| matches!(i, AuthorizationIssue::InvalidPrincipalKind { .. }),
        );
        has(
            with_role(AuthorizationDraft {
                permissions: vec![permission(APPROVER, OP, OP, "owned ", &[])],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::InvalidScopeKind { .. }),
        );
        has(
            with_role(AuthorizationDraft {
                permissions: vec![permission(APPROVER, OP, OP, "owned", &["a\nb"])],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::InvalidConditionExpression { .. }),
        );
        // Operation targets: Accepted Operation only.
        has(
            with_role(AuthorizationDraft {
                permissions: vec![permission(APPROVER, "operation:draft", OP, "owned", &[])],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::NonAcceptedOperation { .. }),
        );
        has(
            with_role(AuthorizationDraft {
                permissions: vec![permission(APPROVER, "brole:manager", OP, "owned", &[])],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::WrongTargetType { .. }),
        );
        has(
            with_role(AuthorizationDraft {
                permissions: vec![permission(APPROVER, "operation:ghost", OP, "owned", &[])],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::UnknownReference { .. }),
        );
        // Resources: Accepted Operation, Entity or Attribute only.
        for resource in [OP, "entity:request", "attr:amount"] {
            let (_, p) = proposed(
                &g,
                &with_role(AuthorizationDraft {
                    permissions: vec![permission(APPROVER, OP, resource, "owned", &[])],
                    ..Default::default()
                }),
            );
            apply(&g, &p);
        }
        let (_, fragments) = imported();
        let fragment = fragments[0].as_id().to_string();
        for resource in [APPROVER, "brole:manager", fragment.as_str()] {
            let mut d = with_role(AuthorizationDraft {
                permissions: vec![permission(APPROVER, OP, resource, "owned", &[])],
                ..Default::default()
            });
            if resource == APPROVER {
                // An existing SecurityRole as resource.
                d.security_roles.clear();
                let g2 = extend(
                    &g,
                    vec![security_role(APPROVER, Accepted, "Approver")],
                    vec![],
                );
                assert!(issues(&g2, &d).iter().any(|i| matches!(i, AuthorizationIssue::WrongTargetType { field, .. } if field == "permission.resource_ref")));
                continue;
            }
            has(
                d,
                |i| matches!(i, AuthorizationIssue::WrongTargetType { field, .. } if field == "permission.resource_ref"),
            );
        }
        has(
            with_role(AuthorizationDraft {
                permissions: vec![permission(APPROVER, OP, "entity:ghost", "owned", &[])],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::UnknownReference { .. }),
        );
        has(
            with_role(AuthorizationDraft {
                permissions: vec![permission(APPROVER, OP, "operation:draft", "owned", &[])],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::NonAcceptedResource { .. }),
        );
        // Roles must be declared or active SecurityRoles.
        has(
            AuthorizationDraft {
                permissions: vec![permission(APPROVER, OP, OP, "owned", &[])],
                ..Default::default()
            },
            |i| matches!(i, AuthorizationIssue::UnknownReference { .. }),
        );
        let rejected = extend(
            &g,
            vec![security_role(APPROVER, Rejected, "Approver")],
            vec![],
        );
        assert!(issues(
            &rejected,
            &AuthorizationDraft {
                permissions: vec![permission(APPROVER, OP, OP, "owned", &[])],
                ..Default::default()
            }
        )
        .iter()
        .any(|i| matches!(i, AuthorizationIssue::UnknownReference { .. })));
        // Assignment subjects: Accepted Principal/Actor or a declared Principal.
        has(
            with_role(AuthorizationDraft {
                assignments: vec![assign("principal:pending", &approver)],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::NonAcceptedSubject { .. }),
        );
        has(
            with_role(AuthorizationDraft {
                assignments: vec![assign("entity:request", &approver)],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::WrongTargetType { .. }),
        );
        for subject in ["principal:bob", "actor:clerk"] {
            let (_, p) = proposed(
                &g,
                &with_role(AuthorizationDraft {
                    assignments: vec![assign(subject, &approver)],
                    ..Default::default()
                }),
            );
            apply(&g, &p);
        }
        // Wrong assignment target.
        has(
            AuthorizationDraft {
                assignments: vec![assign("principal:bob", &id("entity:request"))],
                ..Default::default()
            },
            |i| matches!(i, AuthorizationIssue::WrongTargetType { .. }),
        );
        // Separation constraints: at least two unique SecurityRoles.
        has(
            with_role(AuthorizationDraft {
                separation_constraints: vec![constraint(K::StaticSeparationOfDuty, &[&approver])],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::InvalidSeparationConstraint { .. }),
        );
        has(
            with_role(AuthorizationDraft {
                separation_constraints: vec![constraint(
                    K::StaticSeparationOfDuty,
                    &[&approver, &approver],
                )],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::InvalidSeparationConstraint { .. }),
        );
        has(
            with_role(AuthorizationDraft {
                separation_constraints: vec![constraint(
                    K::MutualExclusion,
                    &[&approver, &id("brole:manager")],
                )],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::WrongTargetType { .. }),
        );
        // Duplicate draft identities and evidence.
        has(
            AuthorizationDraft {
                security_roles: vec![role("Approver"), role("Approver")],
                ..Default::default()
            },
            |i| matches!(i, AuthorizationIssue::DuplicateDraftIdentity { .. }),
        );
        has(
            with_role(AuthorizationDraft {
                permissions: vec![
                    permission(APPROVER, OP, OP, "owned", &[]),
                    permission(APPROVER, OP, OP, "owned", &[]),
                ],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::DuplicateDraftIdentity { .. }),
        );
        has(
            with_role(AuthorizationDraft {
                permissions: vec![permission(APPROVER, OP, OP, "owned", &[EU, EU])],
                ..Default::default()
            }),
            |i| matches!(i, AuthorizationIssue::DuplicateDraftIdentity { .. }),
        );
        has(
            AuthorizationDraft {
                security_roles: vec![SecurityRoleDraft {
                    name: "A".into(),
                    description: None,
                    evidence: vec![EvidenceRef::from(id(OP))],
                }],
                ..Default::default()
            },
            |i| matches!(i, AuthorizationIssue::InvalidEvidence { .. }),
        );
        has(
            AuthorizationDraft {
                security_roles: vec![SecurityRoleDraft {
                    name: "A".into(),
                    description: None,
                    evidence: vec![fragments[0].clone(), fragments[0].clone()],
                }],
                ..Default::default()
            },
            |i| matches!(i, AuthorizationIssue::InvalidEvidence { .. }),
        );
        // Scope kinds and descriptions are preserved verbatim.
        let (_, p) = proposed(
            &g,
            &AuthorizationDraft {
                security_roles: vec![SecurityRoleDraft {
                    name: "Approver".into(),
                    description: Some("Approves, per policy §4.".into()),
                    evidence: vec![],
                }],
                permissions: vec![permission(
                    APPROVER,
                    OP,
                    OP,
                    "organizational_unit:finance",
                    &[],
                )],
                ..Default::default()
            },
        );
        assert!(added_nodes(&p).iter().any(|n| matches!(&n.payload, NodePayload::ResourceScope(s) if s.scope_kind == "organizational_unit:finance")));
        assert!(added_nodes(&p).iter().any(|n| matches!(&n.payload, NodePayload::SecurityRole(r) if r.description.as_deref() == Some("Approves, per policy §4."))));
    }

    // ------------------------------------------------------------------ hierarchy

    #[test]
    fn authorization_role_hierarchy() {
        let g = base_graph();
        let (admin, manager, employee) =
            (role_id("Admin"), role_id("Manager"), role_id("Employee"));
        let roles = vec![role("Admin"), role("Manager"), role("Employee")];
        let acyclic = AuthorizationDraft {
            security_roles: roles.clone(),
            assignments: vec![assign("principal:bob", &admin)],
            inheritances: vec![inherit(&admin, &manager), inherit(&manager, &employee)],
            ..Default::default()
        };
        let (r, p) = proposed(&g, &acyclic);
        assert!(r.hierarchy_cycles.is_empty());
        let applied = accept_all(&apply(&g, &p));
        assert!(accepted_role_hierarchy_cycles(&applied).is_empty());
        let facts = authorization_facts(&applied, true);
        let mut expected = vec![admin.clone(), manager.clone(), employee.clone()];
        expected.sort();
        assert_eq!(
            effective_security_roles(&facts, &id("principal:bob")),
            expected
        );
        assert_eq!(
            effective_security_roles(&facts, &id("actor:clerk")),
            Vec::<Id>::new()
        );
        // Direction: Manager holders do not gain Admin.
        let manager_only = AuthorizationFacts {
            assignments: vec![(id("principal:bob"), manager.clone())],
            inheritances: facts.inheritances.clone(),
            constraints: vec![],
        };
        let mut mgr = vec![manager.clone(), employee.clone()];
        mgr.sort();
        assert_eq!(
            effective_security_roles(&manager_only, &id("principal:bob")),
            mgr
        );
        // Cycles block the proposal.
        let cycle = |inheritances: Vec<RoleInheritanceDraft>| {
            let r = analyze(
                &g,
                &AuthorizationDraft {
                    security_roles: roles.clone(),
                    inheritances,
                    ..Default::default()
                },
            );
            assert!(r.proposal.is_none());
            r
        };
        let mut two = vec![admin.clone(), manager.clone()];
        two.sort();
        let r = cycle(vec![inherit(&admin, &manager), inherit(&manager, &admin)]);
        assert_eq!(r.hierarchy_cycles, vec![two.clone()]);
        assert_eq!(
            r.issues,
            vec![AuthorizationIssue::RoleHierarchyCycle { role_refs: two }]
        );
        let mut three = vec![admin.clone(), manager.clone(), employee.clone()];
        three.sort();
        assert_eq!(
            cycle(vec![
                inherit(&admin, &manager),
                inherit(&manager, &employee),
                inherit(&employee, &admin)
            ])
            .hierarchy_cycles,
            vec![three]
        );
        assert_eq!(
            cycle(vec![inherit(&admin, &admin)]).hierarchy_cycles,
            vec![vec![admin.clone()]]
        );
        // A cycle closed against existing active roles is detected through the overlay.
        let existing = extend(
            &g,
            vec![
                security_role("secrole:a", Proposed, "A"),
                security_role("secrole:b", Proposed, "B"),
            ],
            vec![edge(
                RelationKind::InheritsRole,
                "secrole:a",
                "secrole:b",
                Proposed,
            )],
        );
        let r = analyze(
            &existing,
            &AuthorizationDraft {
                inheritances: vec![inherit(&id("secrole:b"), &id("secrole:a"))],
                ..Default::default()
            },
        );
        assert_eq!(
            r.hierarchy_cycles,
            vec![vec![id("secrole:a"), id("secrole:b")]]
        );
        // The pure analyzer orders cycles deterministically; a rejected edge is inactive.
        assert_eq!(
            role_hierarchy_cycles(&[
                (id("r:c"), id("r:c")),
                (id("r:b"), id("r:a")),
                (id("r:a"), id("r:b"))
            ]),
            vec![vec![id("r:a"), id("r:b")], vec![id("r:c")]]
        );
        let inactive = extend(
            &g,
            vec![
                security_role("secrole:a", Accepted, "A"),
                security_role("secrole:b", Accepted, "B"),
            ],
            vec![edge(
                RelationKind::InheritsRole,
                "secrole:a",
                "secrole:b",
                Rejected,
            )],
        );
        assert!(analyze(
            &inactive,
            &AuthorizationDraft {
                inheritances: vec![inherit(&id("secrole:b"), &id("secrole:a"))],
                ..Default::default()
            }
        )
        .issues
        .is_empty());
    }

    // ------------------------------------------------------------------ separation of duty

    fn facts(
        assignments: &[(&str, &str)],
        inheritances: &[(&str, &str)],
        constraints: &[(&str, K, &[&str])],
    ) -> AuthorizationFacts {
        AuthorizationFacts {
            assignments: assignments.iter().map(|(s, r)| (id(s), id(r))).collect(),
            inheritances: inheritances.iter().map(|(a, b)| (id(a), id(b))).collect(),
            constraints: constraints
                .iter()
                .map(|(c, k, roles)| {
                    (
                        id(c),
                        SeparationConstraint {
                            constraint_kind: *k,
                            role_refs: roles.iter().map(|r| id(r)).collect(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn violation(c: &str, s: &str, roles: &[&str]) -> StaticSodViolation {
        StaticSodViolation {
            constraint_ref: id(c),
            subject_ref: id(s),
            conflicting_role_refs: roles.iter().map(|r| id(r)).collect(),
        }
    }

    #[test]
    fn authorization_static_separation_of_duty() {
        let sod = K::StaticSeparationOfDuty;
        let ab: &[&str] = &["r:a", "r:b"];
        // Direct conflict.
        let a = separation_analysis(&facts(
            &[("p:1", "r:a"), ("p:1", "r:b")],
            &[],
            &[("sc:1", sod, ab)],
        ));
        assert_eq!(a.static_sod_violations, vec![violation("sc:1", "p:1", ab)]);
        // Inherited conflict.
        let a = separation_analysis(&facts(
            &[("p:1", "r:a")],
            &[("r:a", "r:b")],
            &[("sc:1", sod, ab)],
        ));
        assert_eq!(a.static_sod_violations, vec![violation("sc:1", "p:1", ab)]);
        // No conflict; reverse inheritance does not give r:a to holders of r:b.
        assert!(
            separation_analysis(&facts(&[("p:1", "r:a")], &[], &[("sc:1", sod, ab)]))
                .static_sod_violations
                .is_empty()
        );
        assert!(separation_analysis(&facts(
            &[("p:1", "r:b")],
            &[("r:a", "r:b")],
            &[("sc:1", sod, ab)]
        ))
        .static_sod_violations
        .is_empty());
        // Roles inheriting one another without a subject are no violation.
        assert!(
            separation_analysis(&facts(&[], &[("r:a", "r:b")], &[("sc:1", sod, ab)]))
                .static_sod_violations
                .is_empty()
        );
        // Multi-role intersection, multiple subjects and constraints, sorted globally.
        let abc: &[&str] = &["r:a", "r:b", "r:c"];
        let a = separation_analysis(&facts(
            &[
                ("p:2", "r:a"),
                ("p:2", "r:c"),
                ("p:1", "r:b"),
                ("p:1", "r:c"),
                ("p:3", "r:a"),
            ],
            &[],
            &[("sc:2", sod, abc), ("sc:1", sod, &["r:b", "r:c"])],
        ));
        assert_eq!(
            a.static_sod_violations,
            vec![
                violation("sc:1", "p:1", &["r:b", "r:c"]),
                violation("sc:2", "p:1", &["r:b", "r:c"]),
                violation("sc:2", "p:2", &["r:a", "r:c"])
            ]
        );
        // Other kinds are not evaluated and never reported as violations or passes.
        let a = separation_analysis(&facts(
            &[("p:1", "r:a"), ("p:1", "r:b")],
            &[],
            &[
                ("sc:3", K::DynamicSeparationOfDuty, ab),
                ("sc:1", K::MutualExclusion, ab),
                ("sc:2", K::RequiredCombination, ab),
            ],
        ));
        assert!(a.static_sod_violations.is_empty());
        assert_eq!(a.not_evaluated, vec![id("sc:1"), id("sc:2"), id("sc:3")]);
    }

    #[test]
    fn authorization_separation_constraint_kinds_compile() {
        let g = base_graph();
        let (a, b) = (role_id("A"), role_id("B"));
        let kinds = [
            K::StaticSeparationOfDuty,
            K::DynamicSeparationOfDuty,
            K::MutualExclusion,
            K::RequiredCombination,
        ];
        let draft = AuthorizationDraft {
            security_roles: vec![role("B"), role("A")],
            principals: vec![PrincipalDraft {
                name: "Carol".into(),
                principal_kind: "service".into(),
                evidence: vec![],
            }],
            assignments: vec![
                assign(
                    principal_id(&id(PROJECT), "Carol", "service")
                        .unwrap()
                        .as_str(),
                    &a,
                ),
                assign("principal:bob", &b),
                assign("principal:bob", &a),
            ],
            separation_constraints: kinds.iter().map(|k| constraint(*k, &[&b, &a])).collect(),
            ..Default::default()
        };
        let (r, p) = proposed(&g, &draft);
        let constraints: Vec<&Node> = added_nodes(&p)
            .into_iter()
            .filter(|n| n.payload.node_type() == NodeType::SeparationConstraint)
            .collect();
        assert_eq!(constraints.len(), 4);
        let mut ab = vec![a.clone(), b.clone()];
        ab.sort();
        for n in &constraints {
            let NodePayload::SeparationConstraint(c) = &n.payload else {
                panic!()
            };
            assert_eq!(c.role_refs, ab);
            let json = serde_json::to_value(&n.payload).unwrap();
            assert_eq!(
                serde_json::from_value::<NodePayload>(json).unwrap(),
                n.payload
            );
        }
        let static_id =
            separation_constraint_id(&id(PROJECT), K::StaticSeparationOfDuty, &ab).unwrap();
        assert_eq!(
            r.static_sod_violations,
            vec![StaticSodViolation {
                constraint_ref: static_id.clone(),
                subject_ref: id("principal:bob"),
                conflicting_role_refs: ab.clone()
            }]
        );
        let mut others: Vec<Id> = constraints
            .iter()
            .map(|n| n.id.clone())
            .filter(|i| *i != static_id)
            .collect();
        others.sort();
        assert_eq!(r.unevaluated_separation_constraints, others);
        // The accepted-only analyzer sees the same once accepted, and nothing while Proposed.
        let applied = apply(&g, &p);
        assert!(accepted_separation_analysis(&applied)
            .static_sod_violations
            .is_empty());
        let accepted = accept_all(&applied);
        assert_eq!(
            accepted_separation_analysis(&accepted).static_sod_violations,
            r.static_sod_violations
        );
        assert_eq!(
            accepted_separation_analysis(&accepted).not_evaluated,
            others
        );
    }

    // ------------------------------------------------------------------ reconciliation

    #[test]
    fn authorization_reconciliation() {
        let g = base_graph();
        let (_, p) = proposed(&g, &golden_draft());
        let applied = apply(&g, &p);
        // Everything exists identically: nothing to propose, both Proposed and Accepted.
        let again = analyze(&applied, &golden_draft());
        assert!(again.issues.is_empty(), "{:?}", again.issues);
        assert_eq!(again.disposition, AuthorizationDisposition::AllExisting);
        assert!(again.proposal.is_none());
        let accepted = accept_all(&applied);
        assert_eq!(
            analyze(&accepted, &golden_draft()).disposition,
            AuthorizationDisposition::AllExisting
        );
        // Partially existing: only the missing elements are proposed.
        let mut more = golden_draft();
        more.assignments.push(assign("actor:clerk", &id(REQUESTER)));
        let (_, extra) = proposed(&accepted, &more);
        assert!(added_nodes(&extra).is_empty());
        assert_eq!(added_edges(&extra).len(), 1);
        // Conflicts at deterministic identities.
        let conflict = |g: &Graph, d: &AuthorizationDraft, f: fn(&AuthorizationIssue) -> bool| {
            let i = issues(g, d);
            assert!(i.iter().any(f), "{i:?}");
        };
        let described = |g: &Graph| {
            let mut d = golden_draft();
            d.security_roles[0].description = Some("changed".into());
            conflict(g, &d, |i| {
                matches!(i, AuthorizationIssue::ExistingSecurityRoleConflict { .. })
            });
        };
        described(&accepted);
        let renamed = extend(
            &g,
            vec![security_role("secrole:legacy", Accepted, "Approver")],
            vec![],
        );
        conflict(&renamed, &golden_draft(), |i| {
            matches!(
                i,
                AuthorizationIssue::ExistingSecurityRoleNameConflict { .. }
            )
        });
        let principal = extend(
            &g,
            vec![node(
                ALICE,
                Accepted,
                NodePayload::Principal(Principal {
                    name: "Alice".into(),
                    principal_kind: "robot".into(),
                }),
            )],
            vec![],
        );
        conflict(&principal, &golden_draft(), |i| {
            matches!(i, AuthorizationIssue::ExistingPrincipalConflict { .. })
        });
        let scope = extend(
            &g,
            vec![node(
                SCOPE,
                Accepted,
                NodePayload::ResourceScope(ResourceScope {
                    resource_ref: id(OP),
                    scope_kind: "owned".into(),
                }),
            )],
            vec![],
        );
        conflict(&scope, &golden_draft(), |i| {
            matches!(i, AuthorizationIssue::ExistingResourceScopeConflict { .. })
        });
        let cond = extend(
            &g,
            vec![node(
                CONDITION,
                Accepted,
                NodePayload::PolicyCondition(PolicyCondition {
                    expression: "other".into(),
                }),
            )],
            vec![],
        );
        conflict(&cond, &golden_draft(), |i| {
            matches!(
                i,
                AuthorizationIssue::ExistingPolicyConditionConflict { .. }
            )
        });
        let sod = extend(
            &g,
            vec![node(
                SOD,
                Accepted,
                NodePayload::SeparationConstraint(SeparationConstraint {
                    constraint_kind: K::MutualExclusion,
                    role_refs: vec![id(APPROVER), id(REQUESTER)],
                }),
            )],
            vec![],
        );
        conflict(&sod, &golden_draft(), |i| {
            matches!(
                i,
                AuthorizationIssue::ExistingSeparationConstraintConflict { .. }
            )
        });
        // Permission conflicts: different condition set, operation or scope, or grant owner.
        let mut fewer = golden_draft();
        fewer.permissions[0].conditions.clear();
        conflict(&accepted, &fewer, |i| {
            matches!(i, AuthorizationIssue::ExistingPermissionConflict { .. })
        });
        let mut extra_permits = accepted.clone();
        extra_permits = extend(
            &extra_permits,
            vec![],
            vec![edge(RelationKind::Grants, REQUESTER, PERMISSION, Accepted)],
        );
        conflict(&extra_permits, &golden_draft(), |i| {
            matches!(i, AuthorizationIssue::ExistingPermissionConflict { .. })
        });
        let other_operation: Vec<Node> = accepted.nodes().values().cloned().collect();
        let other_edges: Vec<Edge> = accepted
            .edges()
            .values()
            .cloned()
            .map(|mut e| {
                if e.kind == RelationKind::Permits {
                    e.to = id("operation:submit");
                }
                e
            })
            .collect();
        conflict(
            &graph_of(other_operation, other_edges),
            &golden_draft(),
            |i| matches!(i, AuthorizationIssue::ExistingPermissionConflict { .. }),
        );
        let renamed_permission: Vec<Node> = accepted
            .nodes()
            .values()
            .cloned()
            .map(|mut n| {
                if n.id.as_str() == PERMISSION {
                    n.payload = NodePayload::Permission(Permission {
                        name: "Approver -> ApproveRequest [owned]".into(),
                    });
                }
                n
            })
            .collect();
        conflict(
            &graph_of(
                renamed_permission,
                accepted.edges().values().cloned().collect(),
            ),
            &golden_draft(),
            |i| matches!(i, AuthorizationIssue::ExistingPermissionConflict { .. }),
        );
    }

    // ------------------------------------------------------------------ determinism

    #[test]
    fn authorization_reversed_input_determinism() {
        let (_, refs) = imported();
        let (admin, manager) = (role_id("Admin"), role_id("Manager"));
        let draft = AuthorizationDraft {
            security_roles: vec![
                role("Approver"),
                role("Requester"),
                role("Admin"),
                role("Manager"),
            ],
            principals: vec![
                PrincipalDraft {
                    name: "Alice".into(),
                    principal_kind: "human".into(),
                    evidence: vec![refs[0].clone(), refs[1].clone()],
                },
                PrincipalDraft {
                    name: "Dan".into(),
                    principal_kind: "human".into(),
                    evidence: vec![],
                },
            ],
            permissions: vec![
                permission(APPROVER, OP, OP, "direct_reports", &[EU, "amount < 1000"]),
                permission(
                    REQUESTER,
                    "operation:submit",
                    "entity:request",
                    "owned",
                    &[],
                ),
            ],
            assignments: vec![
                assign(ALICE, &id(APPROVER)),
                assign("principal:bob", &admin),
                assign("actor:clerk", &manager),
            ],
            inheritances: vec![
                inherit(&admin, &manager),
                inherit(&id(APPROVER), &id(REQUESTER)),
            ],
            separation_constraints: vec![
                constraint(K::StaticSeparationOfDuty, &[&id(APPROVER), &id(REQUESTER)]),
                constraint(K::StaticSeparationOfDuty, &[&manager, &admin]),
            ],
        };
        let mut reversed = draft.clone();
        reversed.security_roles.reverse();
        reversed.principals.reverse();
        reversed.principals[1].evidence.reverse();
        reversed.permissions.reverse();
        for p in &mut reversed.permissions {
            p.conditions.reverse();
        }
        reversed.assignments.reverse();
        reversed.inheritances.reverse();
        reversed.separation_constraints.reverse();
        for c in &mut reversed.separation_constraints {
            c.role_refs.reverse();
        }
        let mut nodes = base_nodes();
        nodes.reverse();
        let mut edges = base_edges();
        edges.reverse();
        let a = analyze(&base_graph(), &draft);
        let b = analyze(&graph_of(nodes, edges), &reversed);
        assert!(a.issues.is_empty(), "{:?}", a.issues);
        assert_eq!(a.proposal, b.proposal);
        assert_eq!(a.disposition, b.disposition);
        assert_eq!(a.static_sod_violations, b.static_sod_violations);
        assert_eq!(a.static_sod_violations.len(), 2);
        let p = a.proposal.unwrap();
        let ids: Vec<&Id> = patches(&p)
            .iter()
            .map(|s| match s {
                SemanticPatch::AddNode { node } => &node.id,
                SemanticPatch::AddEdge { edge } => &edge.id,
                _ => unreachable!(),
            })
            .collect();
        let edges_part: Vec<&&Id> = ids
            .iter()
            .filter(|i| i.as_str().starts_with("rel:"))
            .collect();
        assert!(edges_part.windows(2).all(|w| w[0] < w[1]));
    }

    // ------------------------------------------------------------------ HR role_specs reference

    /// HR role_specs adapter. Business roles stay business roles; the two security roles and
    /// nine permission rows compile against synthetic Accepted Operations named as in the
    /// fixture. Test-only convention: each row's resource_ref is its operation_ref, because the
    /// fixture names no protected resource. No principal, assignment, hierarchy or separation
    /// constraint is created, because the fixture contains none.
    #[test]
    fn authorization_hr_role_specs_reference() {
        let contract: serde_yaml::Value = serde_yaml::from_str(HR_CONTRACT).unwrap();
        let roles = &contract["role_specs"];
        let list = |v: &serde_yaml::Value| {
            v.as_sequence()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        let business_roles = list(&roles["business_roles"]);
        let security_roles = list(&roles["security_roles"]);
        assert_eq!(
            business_roles,
            ["EmployeeRole", "ManagerRole", "HRPolicyOwner"]
        );
        assert_eq!(security_roles, ["EmployeeUser", "ManagerUser"]);
        let rows: Vec<(String, String, String)> = roles["permissions"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["role"].as_str().unwrap().to_owned(),
                    r["operation"].as_str().unwrap().to_owned(),
                    r["scope"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(rows.len(), 9);
        let operation_names: Vec<String> = contract["operations"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|o| o.as_str().unwrap().to_owned())
            .collect();
        let mut nodes: Vec<Node> = operation_names
            .iter()
            .map(|o| operation(&format!("operation:{o}"), Accepted, o))
            .collect();
        nodes.extend(business_roles.iter().map(|r| {
            node(
                &format!("brole:{r}"),
                Accepted,
                NodePayload::BusinessRole(BusinessRole { name: r.clone() }),
            )
        }));
        let g = graph_of(nodes, vec![]);
        let project = id(PROJECT);
        let draft = AuthorizationDraft {
            security_roles: security_roles.iter().map(|r| role(r)).collect(),
            permissions: rows
                .iter()
                .map(|(r, o, s)| {
                    let op = format!("operation:{o}");
                    permission(
                        security_role_id(&project, r).unwrap().as_str(),
                        &op,
                        &op,
                        s,
                        &[],
                    )
                })
                .collect(),
            ..Default::default()
        };
        let (r, p) = proposed(&g, &draft);
        assert!(
            r.static_sod_violations.is_empty()
                && r.hierarchy_cycles.is_empty()
                && r.unevaluated_separation_constraints.is_empty()
        );
        let applied = apply(&g, &p);
        let count_nodes = |t: NodeType| applied.node_ids_by_type(t).len();
        let count_edges = |k: RelationKind| applied.edge_ids_by_kind(&k).len();
        assert_eq!(count_nodes(NodeType::SecurityRole), 2);
        assert_eq!(count_nodes(NodeType::Permission), 9);
        assert_eq!(count_nodes(NodeType::BusinessRole), 3);
        assert_eq!(count_nodes(NodeType::Principal), 0);
        assert_eq!(count_nodes(NodeType::PolicyCondition), 0);
        assert_eq!(count_nodes(NodeType::SeparationConstraint), 0);
        assert_eq!(count_edges(RelationKind::Grants), 9);
        assert_eq!(count_edges(RelationKind::Permits), 9);
        assert_eq!(count_edges(RelationKind::ScopedTo), 9);
        assert_eq!(count_edges(RelationKind::ConditionedBy), 0);
        assert_eq!(count_edges(RelationKind::AssignedRole), 0);
        assert_eq!(count_edges(RelationKind::InheritsRole), 0);
        // Business roles were not compiled into security roles.
        let security_names: BTreeSet<String> = applied
            .node_ids_by_type(NodeType::SecurityRole)
            .iter()
            .map(|i| match &applied.node(i).unwrap().payload {
                NodePayload::SecurityRole(r) => r.name.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            security_names,
            BTreeSet::from(["EmployeeUser".to_owned(), "ManagerUser".to_owned()])
        );
        // Every row compiles exactly: owner, operation target and verbatim scope string.
        let mut seen_scopes = BTreeSet::new();
        for (role_name, op, scope) in &rows {
            let role_ref = security_role_id(&project, role_name).unwrap();
            let op_ref = id(&format!("operation:{op}"));
            let permission_ref = applied
                .outgoing_edge_ids(&role_ref)
                .iter()
                .map(|e| applied.edge(e).unwrap())
                .filter(|e| e.kind == RelationKind::Grants)
                .map(|e| e.to.clone())
                .find(|perm| {
                    applied.outgoing_edge_ids(perm).iter().any(|e| {
                        let e = applied.edge(e).unwrap();
                        e.kind == RelationKind::Permits && e.to == op_ref
                    })
                })
                .unwrap_or_else(|| panic!("{role_name} {op}"));
            assert_eq!(
                applied.node(&permission_ref).unwrap().payload,
                NodePayload::Permission(Permission {
                    name: format!("{role_name} -> {op} [{scope}]")
                })
            );
            let scope_ref = applied
                .outgoing_edge_ids(&permission_ref)
                .iter()
                .map(|e| applied.edge(e).unwrap())
                .find(|e| e.kind == RelationKind::ScopedTo)
                .unwrap()
                .to
                .clone();
            assert_eq!(
                applied.node(&scope_ref).unwrap().payload,
                NodePayload::ResourceScope(ResourceScope {
                    resource_ref: op_ref.clone(),
                    scope_kind: scope.clone()
                })
            );
            seen_scopes.insert(scope.clone());
        }
        assert_eq!(
            seen_scopes,
            BTreeSet::from(["direct_reports".to_owned(), "owned".to_owned()])
        );
    }

    // ------------------------------------------------------------------ guards

    #[test]
    fn authorization_source_guard() {
        let source = include_str!("../src/authorization.rs");
        for forbidden in [
            "std::fs",
            "File::open",
            "reqwest",
            "std::net",
            "SystemClock",
            "Clock::now",
            "Utc::now",
            "Instant::now",
            "ArtifactStore",
            "RevisionStore",
            "rusqlite",
            "Sqlite",
            ".execute(",
            "MockProvider",
            "InferenceRequest",
            "InferenceArtifact",
            "include_bytes!",
            "include_str!",
            "prompts/",
            "schemas/",
            "rand::",
            "f64",
            "f32",
            "unsafe",
            "fixtures/",
            "hr-leave",
            "EmployeeUser",
            "ManagerUser",
            "EmployeeRole",
            "ManagerRole",
            "LeaveRequest",
            "PLUMB.F2",
            "RBAC.F2",
            "GeneratedFinding",
            "EvaluatorRegistry",
            "BusinessRole",
            "ReplacePayload",
            "MergeNodes",
            "Supersede",
            "SetStatus",
            "ResolutionDecision",
            "plumb_expr",
        ] {
            assert!(
                !source.contains(forbidden),
                "authorization.rs contains {forbidden}"
            );
        }
        let lib = include_str!("../src/lib.rs");
        assert!(!lib.contains("Proposal"));
        assert!(lib.contains("pub mod authorization;"));
    }
}
