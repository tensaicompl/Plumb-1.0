//! F0.5 contract tests for `RelationKind`, relation properties, the relation registry and
//! the `Edge` envelope.
//!
//! Every test lives in `relations_contract` so that the task command
//! `cargo test -p plumb-psg relations` selects all of them.

mod relations_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::Id;
    use plumb_psg::*;
    use serde_json::{json, Value};

    use NodeType as N;
    use RelationKind as R;

    const T1: &str = "2026-09-29T08:00:00.000000000Z";
    const T2: &str = "2026-09-29T09:30:00.250000000Z";

    const CORE_NAMES: [&str; 51] = [
        "evidenced_by",
        "derived_from",
        "supersedes",
        "conflicts_with",
        "resolves",
        "raises",
        "addresses",
        "refines",
        "decomposes_to",
        "specified_by",
        "constrained_by",
        "satisfied_by",
        "has_attribute",
        "has_state",
        "transitions_via",
        "performed_by",
        "reads",
        "writes",
        "produces",
        "consumes",
        "governed_by",
        "uses_calculation",
        "next",
        "assigned_role",
        "inherits_role",
        "grants",
        "permits",
        "scoped_to",
        "conditioned_by",
        "characterized_by",
        "measured_by",
        "drives",
        "allocated_to",
        "depends_on",
        "exposes",
        "stores_in",
        "deployed_to",
        "uses_technology",
        "justified_by",
        "exposed_by",
        "publishes",
        "subscribes_to",
        "schema_for",
        "workflow_step",
        "implemented_by",
        "contains",
        "depends_on_slice",
        "verified_by",
        "implemented_as",
        "produces_receipt",
        "bound_to_code",
    ];

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn kind(s: &str) -> RelationKind {
        s.parse().unwrap()
    }

    fn edge_json(edge_id: &str, kind: &str, from: &str, to: &str, properties: Value) -> Value {
        json!({
            "id": edge_id,
            "revision": 1,
            "status": "Accepted",
            "kind": kind,
            "from": from,
            "to": to,
            "properties": properties,
            "evidence": ["evd:0123456789abcdef"],
            "derivations": ["drv:s2:domain-1"],
            "standards": [],
            "audit": {
                "created_by": "actor:analyst",
                "created_at": T1,
                "updated_by": null,
                "updated_at": null
            }
        })
    }

    fn parse_edge(value: Value) -> Result<Edge, serde_json::Error> {
        serde_json::from_value(value)
    }

    fn edge_with(edge_id: &str, kind: &str, from: &str, to: &str, properties: Value) -> Edge {
        parse_edge(edge_json(edge_id, kind, from, to, properties))
            .unwrap_or_else(|e| panic!("edge {edge_id} ({kind}) should parse: {e}"))
    }

    fn edge(edge_id: &str, kind: &str, from: &str, to: &str) -> Edge {
        let properties = if kind == "schema_for" {
            json!({"role": "attribute"})
        } else {
            json!({})
        };
        edge_with(edge_id, kind, from, to, properties)
    }

    fn nodes(entries: &[(&str, NodeType)]) -> BTreeMap<Id, NodeType> {
        entries.iter().map(|(n, t)| (id(n), *t)).collect()
    }

    fn violations(node_types: &BTreeMap<Id, NodeType>, edges: &[Edge]) -> Vec<RelationViolation> {
        validate_relations(node_types, edges)
            .err()
            .unwrap_or_default()
    }

    fn type_violations(v: Vec<RelationViolation>) -> Vec<RelationViolation> {
        v.into_iter()
            .filter(|v| {
                matches!(
                    v,
                    RelationViolation::SourceTypeNotAllowed { .. }
                        | RelationViolation::TargetTypeNotAllowed { .. }
                        | RelationViolation::NodeTypeMismatch { .. }
                        | RelationViolation::SchemaRoleNotAllowed { .. }
                        | RelationViolation::MissingEndpoint { .. }
                )
            })
            .collect()
    }

    fn cardinality_violations(v: Vec<RelationViolation>) -> Vec<RelationViolation> {
        v.into_iter()
            .filter(|v| {
                matches!(
                    v,
                    RelationViolation::OutgoingCardinality { .. }
                        | RelationViolation::IncomingCardinality { .. }
                )
            })
            .collect()
    }

    fn cycle_violations(v: Vec<RelationViolation>) -> Vec<RelationViolation> {
        v.into_iter()
            .filter(|v| matches!(v, RelationViolation::Cycle { .. }))
            .collect()
    }

    // ------------------------------------------------------------------ RelationKind

    /// Exhaustive: adding any variant (such as an `Other(String)` fallback) fails to compile.
    fn wire_name(kind: &RelationKind) -> String {
        match kind {
            R::EvidencedBy => "evidenced_by",
            R::DerivedFrom => "derived_from",
            R::Supersedes => "supersedes",
            R::ConflictsWith => "conflicts_with",
            R::Resolves => "resolves",
            R::Raises => "raises",
            R::Addresses => "addresses",
            R::Refines => "refines",
            R::DecomposesTo => "decomposes_to",
            R::SpecifiedBy => "specified_by",
            R::ConstrainedBy => "constrained_by",
            R::SatisfiedBy => "satisfied_by",
            R::HasAttribute => "has_attribute",
            R::HasState => "has_state",
            R::TransitionsVia => "transitions_via",
            R::PerformedBy => "performed_by",
            R::Reads => "reads",
            R::Writes => "writes",
            R::Produces => "produces",
            R::Consumes => "consumes",
            R::GovernedBy => "governed_by",
            R::UsesCalculation => "uses_calculation",
            R::Next => "next",
            R::AssignedRole => "assigned_role",
            R::InheritsRole => "inherits_role",
            R::Grants => "grants",
            R::Permits => "permits",
            R::ScopedTo => "scoped_to",
            R::ConditionedBy => "conditioned_by",
            R::CharacterizedBy => "characterized_by",
            R::MeasuredBy => "measured_by",
            R::Drives => "drives",
            R::AllocatedTo => "allocated_to",
            R::DependsOn => "depends_on",
            R::Exposes => "exposes",
            R::StoresIn => "stores_in",
            R::DeployedTo => "deployed_to",
            R::UsesTechnology => "uses_technology",
            R::JustifiedBy => "justified_by",
            R::ExposedBy => "exposed_by",
            R::Publishes => "publishes",
            R::SubscribesTo => "subscribes_to",
            R::SchemaFor => "schema_for",
            R::WorkflowStep => "workflow_step",
            R::ImplementedBy => "implemented_by",
            R::Contains => "contains",
            R::DependsOnSlice => "depends_on_slice",
            R::VerifiedBy => "verified_by",
            R::ImplementedAs => "implemented_as",
            R::ProducesReceipt => "produces_receipt",
            R::BoundToCode => "bound_to_code",
            R::Extension(key) => return key.to_string(),
        }
        .to_string()
    }

    #[test]
    fn there_are_exactly_51_core_relations_in_metamodel_order() {
        assert_eq!(RelationKind::CORE.len(), 51);
        let names: Vec<&str> = RelationKind::CORE.iter().map(|k| k.as_str()).collect();
        assert_eq!(names, CORE_NAMES);
        assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), 51);
        assert!(RelationKind::CORE.iter().all(RelationKind::is_core));
    }

    #[test]
    fn every_core_relation_round_trips_exactly() {
        for name in CORE_NAMES {
            let k: RelationKind = serde_json::from_value(json!(name)).unwrap();
            assert!(k.is_core());
            assert_eq!(k.as_str(), name);
            assert_eq!(wire_name(&k), name);
            assert_eq!(serde_json::to_value(&k).unwrap(), json!(name));
            assert_eq!(k.to_string(), name);
        }
    }

    #[test]
    fn unknown_unqualified_relations_are_rejected() {
        for bad in [
            "",
            "relates_to",
            "depends",
            "Depends_On",
            "EvidencedBy",
            "has-attribute",
            " reads",
        ] {
            assert_eq!(
                bad.parse::<RelationKind>(),
                Err(RelationKindError::UnknownCoreRelation(bad.to_owned())),
                "{bad:?}"
            );
            assert!(serde_json::from_value::<RelationKind>(json!(bad)).is_err());
        }
    }

    #[test]
    fn valid_extension_relations_are_accepted() {
        for name in ["acme:depends_on", "jira:blocks", "customer-x:relates.v2"] {
            let k: RelationKind = serde_json::from_value(json!(name)).unwrap();
            assert!(!k.is_core());
            assert!(matches!(&k, RelationKind::Extension(key) if key.as_str() == name));
            assert_eq!(serde_json::to_value(&k).unwrap(), json!(name));
            assert_eq!(wire_name(&k), name);
            assert!(relation_def(&k).is_none());
        }
    }

    #[test]
    fn malformed_extension_relations_are_rejected() {
        for bad in [
            "Acme:depends_on",
            "acme:",
            "acme::x",
            "acme:a:b",
            ":reads",
            "acme:bad key",
        ] {
            assert!(
                matches!(
                    bad.parse::<RelationKind>(),
                    Err(RelationKindError::InvalidExtension(_))
                ),
                "{bad:?}"
            );
            assert!(serde_json::from_value::<RelationKind>(json!(bad)).is_err());
        }
        assert!(serde_json::from_value::<RelationKind>(json!(3)).is_err());
    }

    // ------------------------------------------------------------------ registry shape

    #[test]
    fn registry_has_exactly_one_core_entry_per_core_relation() {
        assert_eq!(RELATION_REGISTRY.len(), 51);
        let kinds: Vec<&RelationKind> = RELATION_REGISTRY.iter().map(|d| &d.kind).collect();
        assert_eq!(
            kinds.iter().collect::<BTreeSet<_>>().len(),
            51,
            "duplicate entries"
        );
        assert!(kinds.iter().all(|k| k.is_core()));
        assert_eq!(kinds, RelationKind::CORE.iter().collect::<Vec<_>>());
        for k in RelationKind::CORE {
            assert_eq!(&relation_def(k).unwrap().kind, k);
        }
    }

    #[test]
    fn registry_declares_directionality_cycles_properties_and_same_type_exactly() {
        for def in RELATION_REGISTRY.iter() {
            let name = def.kind.as_str();
            let symmetric = name == "conflicts_with";
            assert_eq!(
                def.directionality == Directionality::Symmetric,
                symmetric,
                "{name}"
            );
            let acyclic = [
                "supersedes",
                "refines",
                "decomposes_to",
                "inherits_role",
                "depends_on_slice",
            ]
            .contains(&name);
            assert_eq!(def.cycle_policy == CyclePolicy::Acyclic, acyclic, "{name}");
            assert_eq!(
                def.property_schema == RelationPropertySchema::SchemaFor,
                name == "schema_for",
                "{name}"
            );
            assert_eq!(def.same_node_type, name == "supersedes", "{name}");
        }
    }

    #[test]
    fn registry_declares_exactly_the_unconditional_cardinalities() {
        let any = Cardinality { min: 0, max: None };
        let expected: BTreeMap<&str, (Cardinality, Cardinality)> = BTreeMap::from([
            (
                "supersedes",
                (
                    Cardinality {
                        min: 0,
                        max: Some(1),
                    },
                    any,
                ),
            ),
            ("resolves", (Cardinality { min: 1, max: None }, any)),
            (
                "has_attribute",
                (
                    any,
                    Cardinality {
                        min: 1,
                        max: Some(1),
                    },
                ),
            ),
            (
                "transitions_via",
                (
                    Cardinality {
                        min: 1,
                        max: Some(1),
                    },
                    any,
                ),
            ),
            (
                "permits",
                (
                    Cardinality {
                        min: 1,
                        max: Some(1),
                    },
                    any,
                ),
            ),
            (
                "scoped_to",
                (
                    Cardinality {
                        min: 1,
                        max: Some(1),
                    },
                    any,
                ),
            ),
            (
                "characterized_by",
                (
                    Cardinality {
                        min: 1,
                        max: Some(1),
                    },
                    any,
                ),
            ),
            (
                "measured_by",
                (
                    Cardinality {
                        min: 1,
                        max: Some(1),
                    },
                    any,
                ),
            ),
            ("workflow_step", (Cardinality { min: 1, max: None }, any)),
            (
                "produces_receipt",
                (
                    Cardinality {
                        min: 0,
                        max: Some(1),
                    },
                    any,
                ),
            ),
        ]);
        for def in RELATION_REGISTRY.iter() {
            let (out, inc) = expected
                .get(def.kind.as_str())
                .copied()
                .unwrap_or((any, any));
            assert_eq!(def.outgoing, out, "{} outgoing", def.kind);
            assert_eq!(def.incoming, inc, "{} incoming", def.kind);
        }
    }

    // ------------------------------------------------------------------ categories

    fn set(types: &[NodeType]) -> BTreeSet<NodeType> {
        types.iter().copied().collect()
    }

    #[test]
    fn node_categories_have_exactly_the_specified_members() {
        let arch = [
            N::SoftwareSystem,
            N::Container,
            N::Component,
            N::Module,
            N::Interface,
            N::DataStore,
            N::ExternalSystem,
            N::DeploymentNode,
            N::RuntimeEnvironment,
            N::NetworkZone,
        ];
        assert_eq!(
            set(&NodeCategory::Evidence.members()),
            set(&[N::SourceArtifact, N::EvidenceFragment])
        );
        assert_eq!(
            set(&NodeCategory::Provenance.members()),
            set(&[N::DerivationRecord, N::Agent])
        );
        assert_eq!(
            set(&NodeCategory::ArchitectureElement.members()),
            set(&arch)
        );
        assert_eq!(
            set(&NodeCategory::TechnicalContract.members()),
            set(&[
                N::ApiContract,
                N::ApiOperation,
                N::EventContract,
                N::Channel,
                N::Message,
                N::DataSchema,
                N::TechnicalWorkflow
            ])
        );
        let mut state_owner = arch.to_vec();
        state_owner.extend([N::Entity, N::Process]);
        assert_eq!(set(&NodeCategory::StateOwner.members()), set(&state_owner));
        assert_eq!(
            set(&NodeCategory::DeployableArchitectureElement.members()),
            set(&[
                N::SoftwareSystem,
                N::Container,
                N::Component,
                N::Module,
                N::DataStore
            ])
        );
        let all = set(NodeType::ALL);
        let semantic: BTreeSet<_> = all
            .iter()
            .copied()
            .filter(|t| {
                ![
                    N::SourceArtifact,
                    N::EvidenceFragment,
                    N::DerivationRecord,
                    N::Agent,
                ]
                .contains(t)
            })
            .collect();
        assert_eq!(set(&NodeCategory::SemanticNode.members()), semantic);
        assert_eq!(semantic.len(), 82);
        let semantic_or_evidence: BTreeSet<_> = all
            .iter()
            .copied()
            .filter(|t| ![N::DerivationRecord, N::Agent].contains(t))
            .collect();
        assert_eq!(
            set(&NodeCategory::SemanticOrEvidenceNode.members()),
            semantic_or_evidence
        );
        assert_eq!(semantic_or_evidence.len(), 84);
        assert_eq!(set(&NodeCategory::Any.members()), all);
    }

    // ------------------------------------------------------------------ source/target pairs

    /// (relation, valid source, valid target, invalid source, invalid target)
    const PAIRS: [(&str, NodeType, NodeType, NodeType, NodeType); 51] = [
        (
            "evidenced_by",
            N::Requirement,
            N::EvidenceFragment,
            N::SourceArtifact,
            N::SourceArtifact,
        ),
        (
            "derived_from",
            N::Requirement,
            N::EvidenceFragment,
            N::Agent,
            N::DerivationRecord,
        ),
        (
            "supersedes",
            N::Requirement,
            N::Requirement,
            N::Requirement,
            N::Goal,
        ),
        (
            "conflicts_with",
            N::Requirement,
            N::Constraint,
            N::EvidenceFragment,
            N::Agent,
        ),
        (
            "resolves",
            N::ResolutionDecision,
            N::Question,
            N::Assumption,
            N::Requirement,
        ),
        ("raises", N::Finding, N::Question, N::Question, N::Finding),
        ("addresses", N::View, N::Concern, N::Viewpoint, N::Need),
        ("refines", N::Requirement, N::Requirement, N::Need, N::Goal),
        (
            "decomposes_to",
            N::Capability,
            N::Requirement,
            N::Goal,
            N::ImplementationSlice,
        ),
        (
            "specified_by",
            N::Requirement,
            N::QualityScenario,
            N::Need,
            N::Calculation,
        ),
        (
            "constrained_by",
            N::Operation,
            N::Constraint,
            N::SourceArtifact,
            N::Rule,
        ),
        (
            "satisfied_by",
            N::Requirement,
            N::DataSchema,
            N::Need,
            N::Operation,
        ),
        (
            "has_attribute",
            N::Entity,
            N::Attribute,
            N::Concept,
            N::Entity,
        ),
        (
            "has_state",
            N::Container,
            N::State,
            N::Attribute,
            N::Transition,
        ),
        (
            "transitions_via",
            N::Transition,
            N::Event,
            N::State,
            N::Process,
        ),
        (
            "performed_by",
            N::ProcessNode,
            N::BusinessRole,
            N::Process,
            N::SecurityRole,
        ),
        (
            "reads",
            N::Operation,
            N::Attribute,
            N::ProcessNode,
            N::DataStore,
        ),
        ("writes", N::Operation, N::Entity, N::Process, N::Concept),
        ("produces", N::ProcessNode, N::Outcome, N::Rule, N::Message),
        ("consumes", N::Operation, N::Event, N::Component, N::Message),
        (
            "governed_by",
            N::ProcessNode,
            N::DecisionTable,
            N::Process,
            N::Calculation,
        ),
        (
            "uses_calculation",
            N::Rule,
            N::Calculation,
            N::DecisionTable,
            N::Rule,
        ),
        (
            "next",
            N::ProcessNode,
            N::ProcessNode,
            N::Process,
            N::Operation,
        ),
        (
            "assigned_role",
            N::Principal,
            N::SecurityRole,
            N::BusinessRole,
            N::BusinessRole,
        ),
        (
            "inherits_role",
            N::SecurityRole,
            N::SecurityRole,
            N::BusinessRole,
            N::Permission,
        ),
        (
            "grants",
            N::SecurityRole,
            N::Permission,
            N::BusinessRole,
            N::Operation,
        ),
        (
            "permits",
            N::Permission,
            N::Operation,
            N::SecurityRole,
            N::ApiOperation,
        ),
        (
            "scoped_to",
            N::Permission,
            N::ResourceScope,
            N::SecurityRole,
            N::Entity,
        ),
        (
            "conditioned_by",
            N::Permission,
            N::PolicyCondition,
            N::Rule,
            N::Rule,
        ),
        (
            "characterized_by",
            N::QualityScenario,
            N::QualityCharacteristic,
            N::Requirement,
            N::Measure,
        ),
        (
            "measured_by",
            N::QualityScenario,
            N::Measure,
            N::Requirement,
            N::QualityCharacteristic,
        ),
        (
            "drives",
            N::Constraint,
            N::ArchitectureCandidate,
            N::Requirement,
            N::Technology,
        ),
        (
            "allocated_to",
            N::Entity,
            N::Module,
            N::Attribute,
            N::TechnologySelection,
        ),
        (
            "depends_on",
            N::Container,
            N::ExternalSystem,
            N::ApiContract,
            N::TechnologySelection,
        ),
        (
            "exposes",
            N::Component,
            N::Channel,
            N::ApiContract,
            N::Operation,
        ),
        (
            "stores_in",
            N::Container,
            N::DataStore,
            N::DataStore,
            N::Container,
        ),
        (
            "deployed_to",
            N::DataStore,
            N::RuntimeEnvironment,
            N::Interface,
            N::NetworkZone,
        ),
        (
            "uses_technology",
            N::DeploymentNode,
            N::TechnologySelection,
            N::ApiContract,
            N::Technology,
        ),
        (
            "justified_by",
            N::TechnologySelection,
            N::Constraint,
            N::Technology,
            N::Goal,
        ),
        (
            "exposed_by",
            N::Operation,
            N::ApiOperation,
            N::Process,
            N::ApiContract,
        ),
        (
            "publishes",
            N::Component,
            N::Event,
            N::Container,
            N::Channel,
        ),
        (
            "subscribes_to",
            N::Operation,
            N::Channel,
            N::ProcessNode,
            N::Event,
        ),
        (
            "schema_for",
            N::DataSchema,
            N::Attribute,
            N::Message,
            N::Entity,
        ),
        (
            "workflow_step",
            N::TechnicalWorkflow,
            N::ApiOperation,
            N::Process,
            N::Operation,
        ),
        (
            "implemented_by",
            N::Module,
            N::ImplementationSlice,
            N::Capability,
            N::WorkPackage,
        ),
        (
            "contains",
            N::Release,
            N::ImplementationSlice,
            N::Capability,
            N::TaskContract,
        ),
        (
            "depends_on_slice",
            N::ImplementationSlice,
            N::ImplementationSlice,
            N::WorkPackage,
            N::Release,
        ),
        (
            "verified_by",
            N::Channel,
            N::VerificationObligation,
            N::Rule,
            N::TestCase,
        ),
        (
            "implemented_as",
            N::VerificationObligation,
            N::ArchitectureCheck,
            N::TestCase,
            N::TestExecution,
        ),
        (
            "produces_receipt",
            N::ScenarioRun,
            N::TestReceipt,
            N::TestCase,
            N::CodeBinding,
        ),
        (
            "bound_to_code",
            N::Operation,
            N::CodeBinding,
            N::EvidenceFragment,
            N::Module,
        ),
    ];

    #[test]
    fn every_relation_accepts_its_valid_pair_and_rejects_invalid_endpoints() {
        let names: Vec<&str> = PAIRS.iter().map(|p| p.0).collect();
        assert_eq!(names, CORE_NAMES, "one pair per relation, in order");
        for (name, from, to, bad_from, bad_to) in PAIRS {
            let e = [edge("rel:e:1", name, "node:a", "node:b")];
            let ok = type_violations(violations(&nodes(&[("node:a", from), ("node:b", to)]), &e));
            assert!(ok.is_empty(), "{name} {from:?}->{to:?}: {ok:?}");

            let bad_source = type_violations(violations(
                &nodes(&[("node:a", bad_from), ("node:b", to)]),
                &e,
            ));
            let bad_target = type_violations(violations(
                &nodes(&[("node:a", from), ("node:b", bad_to)]),
                &e,
            ));
            if name == "supersedes" {
                assert!(
                    matches!(
                        bad_target.as_slice(),
                        [RelationViolation::NodeTypeMismatch { .. }]
                    ),
                    "{bad_target:?}"
                );
                continue;
            }
            assert!(
                bad_source.iter().any(|v| matches!(v, RelationViolation::SourceTypeNotAllowed { node_type, .. } if *node_type == bad_from)),
                "{name} accepted source {bad_from:?}: {bad_source:?}"
            );
            assert!(
                bad_target.iter().any(|v| matches!(v, RelationViolation::TargetTypeNotAllowed { node_type, .. } if *node_type == bad_to)),
                "{name} accepted target {bad_to:?}: {bad_target:?}"
            );
        }
    }

    #[test]
    fn category_predicates_cover_every_member_for_category_based_relations() {
        // has_state accepts every StateOwner, deployed_to every deployable element.
        for owner in NodeCategory::StateOwner.members() {
            let e = [edge("rel:e:1", "has_state", "node:a", "node:b")];
            assert!(
                type_violations(violations(
                    &nodes(&[("node:a", owner), ("node:b", N::State)]),
                    &e
                ))
                .is_empty(),
                "{owner:?}"
            );
        }
        for element in NodeCategory::DeployableArchitectureElement.members() {
            let e = [edge("rel:e:1", "deployed_to", "node:a", "node:b")];
            assert!(type_violations(violations(
                &nodes(&[("node:a", element), ("node:b", N::DeploymentNode)]),
                &e
            ))
            .is_empty());
        }
        for non_deployable in [
            N::Interface,
            N::ExternalSystem,
            N::DeploymentNode,
            N::RuntimeEnvironment,
            N::NetworkZone,
        ] {
            let e = [edge("rel:e:1", "deployed_to", "node:a", "node:b")];
            assert!(!type_violations(violations(
                &nodes(&[("node:a", non_deployable), ("node:b", N::DeploymentNode)]),
                &e
            ))
            .is_empty());
        }
    }

    #[test]
    fn missing_endpoints_are_reported() {
        let e = [edge("rel:e:1", "reads", "op:a", "attr:missing")];
        let v = violations(&nodes(&[("op:a", N::Operation)]), &e);
        assert!(v.contains(&RelationViolation::MissingEndpoint {
            edge: id("rel:e:1"),
            node: id("attr:missing")
        }));
    }

    #[test]
    fn extension_relations_have_no_type_constraints() {
        let e = [edge_with(
            "rel:e:1",
            "acme:traces",
            "node:a",
            "node:b",
            json!({"acme:weight": 2}),
        )];
        assert!(validate_relations(
            &nodes(&[("node:a", N::Agent), ("node:b", N::SourceArtifact)]),
            &e
        )
        .is_ok());
    }

    // ------------------------------------------------------------------ schema_for

    #[test]
    fn schema_for_roles_must_fit_the_target_type() {
        let allowed = [
            (N::Attribute, vec!["attribute"]),
            (N::Message, vec!["message_payload", "message_headers"]),
            (
                N::ApiOperation,
                vec!["api_request", "api_response", "api_error"],
            ),
        ];
        let all_roles = [
            "attribute",
            "message_payload",
            "message_headers",
            "api_request",
            "api_response",
            "api_error",
        ];
        for (target, ok_roles) in allowed {
            for role in all_roles {
                let e = [edge_with(
                    "rel:s:1",
                    "schema_for",
                    "schema:a",
                    "node:b",
                    json!({"role": role}),
                )];
                let v = type_violations(violations(
                    &nodes(&[("schema:a", N::DataSchema), ("node:b", target)]),
                    &e,
                ));
                if ok_roles.contains(&role) {
                    assert!(v.is_empty(), "{target:?} {role}: {v:?}");
                } else {
                    assert!(
                        matches!(
                            v.as_slice(),
                            [RelationViolation::SchemaRoleNotAllowed { .. }]
                        ),
                        "{target:?} {role}: {v:?}"
                    );
                }
            }
        }
        let e = [edge_with(
            "rel:s:1",
            "schema_for",
            "schema:a",
            "node:b",
            json!({"role": "attribute"}),
        )];
        let v = type_violations(violations(
            &nodes(&[("schema:a", N::DataSchema), ("node:b", N::Entity)]),
            &e,
        ));
        assert!(
            matches!(
                v.as_slice(),
                [RelationViolation::TargetTypeNotAllowed { .. }]
            ),
            "{v:?}"
        );
    }

    // ------------------------------------------------------------------ cardinality

    #[test]
    fn supersedes_allows_at_most_one_outgoing_edge() {
        let n = nodes(&[
            ("req:a", N::Requirement),
            ("req:b", N::Requirement),
            ("req:c", N::Requirement),
        ]);
        assert!(cardinality_violations(violations(
            &n,
            &[edge("rel:1", "supersedes", "req:a", "req:b")]
        ))
        .is_empty());
        let two = [
            edge("rel:1", "supersedes", "req:a", "req:b"),
            edge("rel:2", "supersedes", "req:a", "req:c"),
        ];
        assert!(matches!(
            cardinality_violations(violations(&n, &two)).as_slice(),
            [RelationViolation::OutgoingCardinality {
                count: 2,
                max: Some(1),
                ..
            }]
        ));
    }

    #[test]
    fn resolves_requires_at_least_one_outgoing_edge() {
        let n = nodes(&[
            ("dec:a", N::ResolutionDecision),
            ("q:a", N::Question),
            ("fnd:a", N::Finding),
        ]);
        assert!(matches!(
            cardinality_violations(violations(&n, &[])).as_slice(),
            [RelationViolation::OutgoingCardinality {
                count: 0,
                min: 1,
                ..
            }]
        ));
        let two = [
            edge("rel:1", "resolves", "dec:a", "q:a"),
            edge("rel:2", "resolves", "dec:a", "fnd:a"),
        ];
        assert!(cardinality_violations(violations(&n, &two)).is_empty());
    }

    #[test]
    fn every_attribute_has_exactly_one_owning_entity() {
        let n = nodes(&[
            ("ent:a", N::Entity),
            ("ent:b", N::Entity),
            ("attr:x", N::Attribute),
        ]);
        assert!(matches!(
            cardinality_violations(violations(&n, &[])).as_slice(),
            [RelationViolation::IncomingCardinality { count: 0, .. }]
        ));
        assert!(cardinality_violations(violations(
            &n,
            &[edge("rel:1", "has_attribute", "ent:a", "attr:x")]
        ))
        .is_empty());
        let two = [
            edge("rel:1", "has_attribute", "ent:a", "attr:x"),
            edge("rel:2", "has_attribute", "ent:b", "attr:x"),
        ];
        assert!(matches!(
            cardinality_violations(violations(&n, &two)).as_slice(),
            [RelationViolation::IncomingCardinality { count: 2, .. }]
        ));
    }

    fn assert_exactly_one(
        name: &str,
        from_type: NodeType,
        to_type: NodeType,
        required_elsewhere: &[(&str, NodeType, &str)],
    ) {
        let mut entries = vec![
            ("node:src", from_type),
            ("node:t1", to_type),
            ("node:t2", to_type),
        ];
        let mut base = Vec::new();
        for (i, (other, other_type, other_kind)) in required_elsewhere.iter().enumerate() {
            entries.push((other, *other_type));
            base.push(edge(
                &format!("rel:base:{i}"),
                other_kind,
                "node:src",
                other,
            ));
        }
        let n = nodes(&entries);
        let zero = cardinality_violations(violations(&n, &base));
        assert!(
            matches!(
                zero.as_slice(),
                [RelationViolation::OutgoingCardinality { count: 0, .. }]
            ),
            "{name} zero: {zero:?}"
        );
        let mut one = base.clone();
        one.push(edge("rel:1", name, "node:src", "node:t1"));
        assert!(
            cardinality_violations(violations(&n, &one)).is_empty(),
            "{name} one"
        );
        let mut two = one.clone();
        two.push(edge("rel:2", name, "node:src", "node:t2"));
        let two = cardinality_violations(violations(&n, &two));
        assert!(
            matches!(
                two.as_slice(),
                [RelationViolation::OutgoingCardinality { count: 2, .. }]
            ),
            "{name} two: {two:?}"
        );
    }

    #[test]
    fn exactly_one_outgoing_relations_are_enforced() {
        assert_exactly_one("transitions_via", N::Transition, N::Operation, &[]);
        assert_exactly_one(
            "permits",
            N::Permission,
            N::Operation,
            &[("scope:x", N::ResourceScope, "scoped_to")],
        );
        assert_exactly_one(
            "scoped_to",
            N::Permission,
            N::ResourceScope,
            &[("op:x", N::Operation, "permits")],
        );
        assert_exactly_one(
            "characterized_by",
            N::QualityScenario,
            N::QualityCharacteristic,
            &[("measure:x", N::Measure, "measured_by")],
        );
        assert_exactly_one(
            "measured_by",
            N::QualityScenario,
            N::Measure,
            &[("quality:x", N::QualityCharacteristic, "characterized_by")],
        );
    }

    #[test]
    fn workflow_step_requires_at_least_one_outgoing_edge() {
        let n = nodes(&[
            ("wf:a", N::TechnicalWorkflow),
            ("apiop:a", N::ApiOperation),
            ("apiop:b", N::ApiOperation),
        ]);
        assert!(matches!(
            cardinality_violations(violations(&n, &[])).as_slice(),
            [RelationViolation::OutgoingCardinality {
                count: 0,
                min: 1,
                ..
            }]
        ));
        let two = [
            edge("rel:1", "workflow_step", "wf:a", "apiop:a"),
            edge("rel:2", "workflow_step", "wf:a", "apiop:b"),
        ];
        assert!(cardinality_violations(violations(&n, &two)).is_empty());
    }

    #[test]
    fn produces_receipt_allows_at_most_one_outgoing_edge() {
        let n = nodes(&[
            ("exec:a", N::TestExecution),
            ("rcpt:a", N::TestReceipt),
            ("rcpt:b", N::TestReceipt),
        ]);
        assert!(cardinality_violations(violations(&n, &[])).is_empty());
        let two = [
            edge("rel:1", "produces_receipt", "exec:a", "rcpt:a"),
            edge("rel:2", "produces_receipt", "exec:a", "rcpt:b"),
        ];
        assert!(matches!(
            cardinality_violations(violations(&n, &two)).as_slice(),
            [RelationViolation::OutgoingCardinality { count: 2, .. }]
        ));
    }

    #[test]
    fn conditional_gate_requirements_are_not_structural_cardinality() {
        let n = nodes(&[
            ("req:a", N::Requirement),
            ("op:a", N::Operation),
            ("pn:a", N::ProcessNode),
            ("ent:a", N::Entity),
            ("container:a", N::Container),
            ("adr:a", N::ArchitectureDecision),
        ]);
        assert!(validate_relations(&n, &[]).is_ok());
    }

    // ------------------------------------------------------------------ cycles

    fn cycle_case(name: &str, node_type: NodeType) {
        let n = nodes(&[
            ("node:a", node_type),
            ("node:b", node_type),
            ("node:c", node_type),
        ]);
        let chain = [
            edge("rel:1", name, "node:a", "node:b"),
            edge("rel:2", name, "node:b", "node:c"),
        ];
        assert!(
            cycle_violations(violations(&n, &chain)).is_empty(),
            "{name} chain"
        );
        let direct = [
            edge("rel:1", name, "node:a", "node:b"),
            edge("rel:2", name, "node:b", "node:a"),
        ];
        assert!(
            matches!(
                cycle_violations(violations(&n, &direct)).as_slice(),
                [RelationViolation::Cycle { nodes, .. }] if nodes.len() == 2
            ),
            "{name} direct"
        );
        let multi = [
            edge("rel:1", name, "node:a", "node:b"),
            edge("rel:2", name, "node:b", "node:c"),
            edge("rel:3", name, "node:c", "node:a"),
        ];
        assert!(
            matches!(
                cycle_violations(violations(&n, &multi)).as_slice(),
                [RelationViolation::Cycle { nodes, .. }] if nodes.len() == 3
            ),
            "{name} multi"
        );
        let self_loop = [edge("rel:1", name, "node:a", "node:a")];
        assert_eq!(
            cycle_violations(violations(&n, &self_loop)).len(),
            1,
            "{name} self loop"
        );
    }

    #[test]
    fn core_acyclic_relations_reject_cycles() {
        cycle_case("supersedes", N::Requirement);
        cycle_case("refines", N::Requirement);
        cycle_case("decomposes_to", N::Capability);
        cycle_case("inherits_role", N::SecurityRole);
        cycle_case("depends_on_slice", N::ImplementationSlice);
    }

    #[test]
    fn next_loops_are_allowed() {
        let n = nodes(&[
            ("pn:a", N::ProcessNode),
            ("pn:b", N::ProcessNode),
            ("pn:c", N::ProcessNode),
        ]);
        let loop_edges = [
            edge("rel:1", "next", "pn:a", "pn:b"),
            edge("rel:2", "next", "pn:b", "pn:c"),
            edge("rel:3", "next", "pn:c", "pn:a"),
        ];
        assert!(validate_relations(&n, &loop_edges).is_ok());
    }

    // ------------------------------------------------------------------ conflicts_with

    #[test]
    fn conflicts_with_is_one_symmetric_edge() {
        assert_eq!(
            relation_def(&R::ConflictsWith).unwrap().directionality,
            Directionality::Symmetric
        );
        let n = nodes(&[("req:a", N::Requirement), ("req:b", N::Requirement)]);
        let single = [edge("rel:1", "conflicts_with", "req:a", "req:b")];
        assert!(
            validate_relations(&n, &single).is_ok(),
            "no mirrored edge is required"
        );
    }

    // ------------------------------------------------------------------ relation properties

    #[test]
    fn non_schema_core_relations_accept_only_empty_properties() {
        for name in CORE_NAMES.iter().filter(|n| **n != "schema_for") {
            let ok = parse_edge(edge_json("rel:1", name, "node:a", "node:b", json!({}))).unwrap();
            assert_eq!(ok.properties, RelationProperties::None);
            for props in [
                json!({"weight": 1}),
                json!({"role": "api_request"}),
                json!({"acme:criticality": "high"}),
            ] {
                assert!(
                    parse_edge(edge_json("rel:1", name, "node:a", "node:b", props.clone()))
                        .is_err(),
                    "{name} accepted {props}"
                );
            }
        }
    }

    #[test]
    fn schema_for_requires_typed_schema_properties() {
        for bad in [
            json!({}),
            json!({"role": "request"}),
            json!({"role": "api_request", "status": 200}),
            json!({"acme:role": "x"}),
        ] {
            assert!(
                parse_edge(edge_json(
                    "rel:1",
                    "schema_for",
                    "schema:a",
                    "node:b",
                    bad.clone()
                ))
                .is_err(),
                "{bad}"
            );
        }
        for (role, variant) in [
            ("attribute", SchemaBindingRole::Attribute),
            ("message_payload", SchemaBindingRole::MessagePayload),
            ("message_headers", SchemaBindingRole::MessageHeaders),
            ("api_request", SchemaBindingRole::ApiRequest),
            ("api_response", SchemaBindingRole::ApiResponse),
            ("api_error", SchemaBindingRole::ApiError),
        ] {
            assert_eq!(serde_json::to_value(variant).unwrap(), json!(role));
            assert_eq!(
                serde_json::from_value::<SchemaBindingRole>(json!(role)).unwrap(),
                variant
            );
            let e = parse_edge(edge_json(
                "rel:1",
                "schema_for",
                "schema:a",
                "node:b",
                json!({"role": role}),
            ))
            .unwrap();
            assert_eq!(
                e.properties,
                RelationProperties::SchemaFor(SchemaForProperties { role: variant })
            );
            assert_eq!(
                serde_json::to_value(&e).unwrap()["properties"],
                json!({"role": role})
            );
        }
    }

    #[test]
    fn extension_relations_take_namespaced_properties_only() {
        let e = parse_edge(edge_json(
            "rel:1",
            "acme:traces",
            "node:a",
            "node:b",
            json!({"acme:criticality": "high"}),
        ))
        .unwrap();
        let expected = BTreeMap::from([(
            "acme:criticality".parse::<ExtensionKey>().unwrap(),
            json!("high"),
        )]);
        assert_eq!(e.properties, RelationProperties::Extension(expected));
        let empty = parse_edge(edge_json(
            "rel:1",
            "acme:traces",
            "node:a",
            "node:b",
            json!({}),
        ))
        .unwrap();
        assert_eq!(
            empty.properties,
            RelationProperties::Extension(BTreeMap::new())
        );
        for bad in [
            json!({"criticality": "high"}),
            json!({"Acme:x": 1}),
            json!({"acme:x:y": 1}),
        ] {
            assert!(
                parse_edge(edge_json(
                    "rel:1",
                    "acme:traces",
                    "node:a",
                    "node:b",
                    bad.clone()
                ))
                .is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn kind_and_properties_variant_must_match() {
        let base = parse_edge(edge_json("rel:1", "reads", "op:a", "attr:a", json!({}))).unwrap();
        let ext_props =
            RelationProperties::Extension(BTreeMap::from([("acme:x".parse().unwrap(), json!(1))]));
        let schema_props = RelationProperties::SchemaFor(SchemaForProperties {
            role: SchemaBindingRole::ApiRequest,
        });
        for (k, props, ok) in [
            (kind("reads"), RelationProperties::None, true),
            (kind("reads"), ext_props.clone(), false),
            (kind("reads"), schema_props.clone(), false),
            (kind("schema_for"), schema_props.clone(), true),
            (kind("schema_for"), RelationProperties::None, false),
            (kind("schema_for"), ext_props.clone(), false),
            (kind("acme:traces"), ext_props.clone(), true),
            (kind("acme:traces"), RelationProperties::None, false),
            (kind("acme:traces"), schema_props.clone(), false),
        ] {
            let e = Edge {
                kind: k.clone(),
                properties: props,
                ..base.clone()
            };
            assert_eq!(e.validate().is_ok(), ok, "{k}");
            if !ok {
                assert!(matches!(
                    e.validate(),
                    Err(EdgeError::InvalidProperties(
                        RelationPropertiesError::KindMismatch { .. }
                    ))
                ));
            }
        }
    }

    // ------------------------------------------------------------------ Edge envelope

    #[test]
    fn core_schema_for_and_extension_edges_round_trip() {
        for value in [
            edge_json("rel:1", "reads", "op:hr:approve", "attr:hr:days", json!({})),
            edge_json(
                "rel:2",
                "schema_for",
                "schema:hr:leave",
                "apiop:hr:approve",
                json!({"role": "api_request"}),
            ),
            edge_json(
                "rel:3",
                "acme:traces",
                "req:HR-001",
                "op:hr:approve",
                json!({"acme:criticality": "high"}),
            ),
        ] {
            let e = parse_edge(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&e).unwrap(), value);
            assert_eq!(e.validate(), Ok(()));
        }
    }

    #[test]
    fn edge_revision_one_is_accepted_and_zero_rejected() {
        let mut value = edge_json("rel:1", "reads", "op:a", "attr:a", json!({}));
        assert_eq!(parse_edge(value.clone()).unwrap().revision, 1);
        value["revision"] = json!(0);
        assert!(parse_edge(value).is_err());
        let mut e = edge("rel:1", "reads", "op:a", "attr:a");
        e.revision = 0;
        assert_eq!(e.validate(), Err(EdgeError::ZeroRevision));
    }

    #[test]
    fn edge_rejects_invalid_audit_meta() {
        let mut value = edge_json("rel:1", "reads", "op:a", "attr:a", json!({}));
        value["audit"]["updated_by"] = json!("actor:reviewer");
        assert!(parse_edge(value.clone()).is_err());
        value["audit"]["updated_at"] = json!("2020-01-01T00:00:00.000000000Z");
        assert!(parse_edge(value.clone()).is_err());
        value["audit"]["updated_at"] = json!(T2);
        assert!(parse_edge(value).is_ok());
        let mut e = edge("rel:1", "reads", "op:a", "attr:a");
        e.audit.updated_at = Some(T2.parse().unwrap());
        assert_eq!(
            e.validate(),
            Err(EdgeError::InvalidAudit(
                AuditMetaError::UpdatedAtWithoutUpdatedBy
            ))
        );
    }

    #[test]
    fn edge_keeps_typed_evidence_and_derivation_refs() {
        let e = edge("rel:1", "reads", "op:a", "attr:a");
        assert_eq!(
            e.evidence,
            vec![EvidenceRef::from(id("evd:0123456789abcdef"))]
        );
        assert_eq!(
            e.derivations,
            vec![DerivationRef::from(id("drv:s2:domain-1"))]
        );
        for (field, bad) in [
            ("evidence", json!(["Not An Id"])),
            ("derivations", json!(["drv:"])),
            ("from", json!("")),
            ("to", json!("X:y")),
        ] {
            let mut value = edge_json("rel:1", "reads", "op:a", "attr:a", json!({}));
            value[field] = bad;
            assert!(parse_edge(value).is_err(), "{field}");
        }
    }

    #[test]
    fn edge_rejects_unknown_fields_and_escape_hatches() {
        for (field, value) in [
            ("confidence", json!(0.9)),
            ("props", json!({"a": 1})),
            ("label", json!("reads")),
            ("tags", json!([])),
            ("extensions", json!({})),
        ] {
            let mut edge_value = edge_json("rel:1", "reads", "op:a", "attr:a", json!({}));
            edge_value[field] = value;
            assert!(parse_edge(edge_value).is_err(), "Edge accepted {field}");
        }
        for (field, value) in [
            ("kind", json!("Reads")),
            ("kind", json!({"type": "reads"})),
            ("kind", json!("reads_data")),
            ("properties", json!([])),
            ("properties", json!(null)),
        ] {
            let mut edge_value = edge_json("rel:1", "reads", "op:a", "attr:a", json!({}));
            edge_value[field] = value.clone();
            assert!(
                parse_edge(edge_value).is_err(),
                "Edge accepted {field}={value}"
            );
        }
        let keys: BTreeSet<String> = serde_json::to_value(edge("rel:1", "reads", "op:a", "attr:a"))
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            keys,
            [
                "audit",
                "derivations",
                "evidence",
                "from",
                "id",
                "kind",
                "properties",
                "revision",
                "standards",
                "status",
                "to"
            ]
            .into_iter()
            .map(String::from)
            .collect()
        );
    }

    // ------------------------------------------------------------------ Hotfix 011 regressions

    #[test]
    fn conflicts_with_requires_canonical_orientation() {
        assert!(parse_edge(edge_json(
            "rel:1",
            "conflicts_with",
            "req:a",
            "req:b",
            json!({})
        ))
        .is_ok());
        for (from, to) in [("req:b", "req:a"), ("req:a", "req:a")] {
            assert!(
                parse_edge(edge_json("rel:1", "conflicts_with", from, to, json!({}))).is_err(),
                "{from}->{to}"
            );
            let mut e = edge("rel:1", "conflicts_with", "req:a", "req:b");
            e.from = id(from);
            e.to = id(to);
            assert_eq!(
                e.validate(),
                Err(EdgeError::NonCanonicalConflictOrientation {
                    from: id(from),
                    to: id(to)
                })
            );
        }
        // Other relations keep free orientation.
        assert!(parse_edge(edge_json(
            "rel:1",
            "depends_on",
            "container:b",
            "container:a",
            json!({})
        ))
        .is_ok());
        assert!(parse_edge(edge_json("rel:1", "next", "pn:a", "pn:a", json!({}))).is_ok());
    }

    #[test]
    fn edge_collections_reject_exact_duplicates() {
        let mapping = json!({
            "standard_id": "ISO/IEC 25010", "version": "2023", "concept": "performance",
            "clause_ref": null, "mapping_role": "taxonomy", "mapping_strength": "compatible",
            "validator_rules": []
        });
        for (field, value) in [
            (
                "evidence",
                json!(["evd:0123456789abcdef", "evd:0123456789abcdef"]),
            ),
            ("derivations", json!(["drv:a:1", "drv:a:1"])),
            ("standards", json!([mapping.clone(), mapping.clone()])),
        ] {
            let mut v = edge_json("rel:1", "reads", "op:a", "attr:a", json!({}));
            v[field] = value;
            assert!(parse_edge(v).is_err(), "duplicate {field} accepted");
        }
        let base = edge("rel:1", "reads", "op:a", "attr:a");
        let dup_evidence = Edge {
            evidence: vec![base.evidence[0].clone(), base.evidence[0].clone()],
            ..base.clone()
        };
        assert!(matches!(
            dup_evidence.validate(),
            Err(EdgeError::DuplicateEvidenceRef(_))
        ));
        let dup_derivations = Edge {
            derivations: vec![base.derivations[0].clone(), base.derivations[0].clone()],
            ..base.clone()
        };
        assert!(matches!(
            dup_derivations.validate(),
            Err(EdgeError::DuplicateDerivationRef(_))
        ));
        let m: StandardMapping = serde_json::from_value(mapping).unwrap();
        let dup_standards = Edge {
            standards: vec![m.clone(), m],
            ..base
        };
        assert_eq!(
            dup_standards.validate(),
            Err(EdgeError::DuplicateStandardMapping { index: 1 })
        );
    }

    #[test]
    fn shape_and_constraint_validation_are_separate() {
        use plumb_psg::registry::{validate_relation_constraints, validate_relation_shapes};
        // Shape error (wrong target type) plus a cardinality error (Attribute without owner).
        let n = nodes(&[
            ("op:a", N::Operation),
            ("proc:x", N::Process),
            ("attr:x", N::Attribute),
        ]);
        let e = [edge("rel:1", "reads", "op:a", "proc:x")];
        let shapes = validate_relation_shapes(&n, &e).unwrap_err();
        assert!(
            shapes
                .iter()
                .all(|v| matches!(v, RelationViolation::TargetTypeNotAllowed { .. })),
            "{shapes:?}"
        );
        let constraints = validate_relation_constraints(&n, &e).unwrap_err();
        assert!(
            constraints
                .iter()
                .all(|v| matches!(v, RelationViolation::IncomingCardinality { .. })),
            "{constraints:?}"
        );
        let mut both = shapes.clone();
        both.extend(constraints.clone());
        assert_eq!(validate_relations(&n, &e).unwrap_err(), both);
        // A cycle is a constraint, not a shape, violation.
        let cyc = nodes(&[("req:a", N::Requirement), ("req:b", N::Requirement)]);
        let cycle = [
            edge("rel:1", "refines", "req:a", "req:b"),
            edge("rel:2", "refines", "req:b", "req:a"),
        ];
        assert!(validate_relation_shapes(&cyc, &cycle).is_ok());
        assert!(matches!(
            validate_relation_constraints(&cyc, &cycle)
                .unwrap_err()
                .as_slice(),
            [RelationViolation::Cycle { .. }]
        ));
    }
}
