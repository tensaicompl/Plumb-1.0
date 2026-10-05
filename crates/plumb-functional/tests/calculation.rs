//! S2.6 contract tests for grounded calculations: explicit scope bindings, the reserved
//! calendar, the PSG-to-PlumbExpr type adapter, used-binding derivation, result, unit,
//! rounding and calendar qualification, origin replay and legacy overrides, dependency
//! cycles, the inference request and artifact, proposals and reconciliation, and evaluation.
//!
//! Golden values were computed independently with Python `hashlib` over RFC 8785 JSON. The HR
//! spellings come from an explicit synthetic scope; they are not derived from PSG names, and no
//! Accepted HR Calculation is claimed.

mod calculation_contract {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::str::FromStr;

    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_expr::{
        CalendarDefinition, CalendarError, CalendarProvider, PredicateError, PredicateProvider,
        RefValue, StaticCalendarProvider, Symbol, Ty, Unit, Value, ValueEnv,
    };
    use plumb_functional::calculation::*;
    use plumb_inference::{InferenceArtifact, InferenceRequest, ProviderPolicy};
    use plumb_patch::{
        apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        Attribute, AuditMeta, Calculation, Calendar, DerivationRef, Edge, ElementStatus, Entity,
        Graph, Modality, Node, NodePayload, RelationKind, RelationProperties, Requirement,
        RequirementKind, RequirementLevel,
    };
    use rust_decimal::Decimal;
    use serde_json::{json, Value as Json};
    use time::{Date, Month, Weekday};

    use ElementStatus::{Accepted, Proposed, Rejected};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s2-calculation.md");
    const SCHEMA: &str = include_str!("../../../schemas/inference/s2-calculation.schema.json");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:bd06b37b7194922d67118fede3ad816dea10e96047a1d340cf1ac01ce3cb0241";
    const SCHEMA_HASH: &str =
        "sha256:b45f4f50c334aa0328078b68c03bc2f8dd7cda7076bf59459774bcc31dea6bee";
    const GOLDEN_CONTEXT_HASH: &str =
        "sha256:370d119628f0aa8aac79656940cf3da034592d6b0eba05ae776cd0787fa49bc7";
    const GOLDEN_REQUEST_ID: &str =
        "sha256:a4d5cefac27295d5708e3040a8873c4770225a8c909e7aae93a7d08f6c266cd6";
    const GOLDEN_CALCULATION_ID: &str = "calculation:23fc4dc09b5ebff7";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const R1: &str = "Requested Working Days equal the working days from the start date to the end date, including the end date, multiplied by the day fraction.";
    const ENTITY: &str = "entity:leave-request";
    const CAL: &str = "cal:PL-Office";
    const HALF_DAY: &str =
        "working_days([start_date, end_date], calendar, inclusive_end) * day_fraction";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn date(y: i32, m: u8, d: u8) -> Date {
        Date::from_calendar_date(y, Month::try_from(m).unwrap(), d).unwrap()
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
            audit: AuditMeta::new(id("actor:human"), ts(AT), None, None).unwrap(),
        }
    }

    fn attribute(
        node_id: &str,
        status: ElementStatus,
        name: &str,
        value_type: &str,
        unit: Option<&str>,
        members: Option<Vec<&str>>,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Attribute(Attribute {
                name: name.into(),
                value_type: value_type.into(),
                nullable: false,
                unit: unit.map(Into::into),
                precision: None,
                enum_values: members.map(|m| m.into_iter().map(Into::into).collect()),
                data_classification: None,
            }),
        )
    }

    fn calculation_node(
        node_id: &str,
        status: ElementStatus,
        name: &str,
        expression: &str,
        result_type: &str,
        unit: Option<&str>,
    ) -> Node {
        node(
            node_id,
            status,
            NodePayload::Calculation(Calculation {
                name: name.into(),
                expression: expression.into(),
                result_type: result_type.into(),
                unit: unit.map(Into::into),
                rounding: None,
                calendar_ref: None,
                examples: None,
            }),
        )
    }

    fn calendar(node_id: &str, status: ElementStatus) -> Node {
        node(
            node_id,
            status,
            NodePayload::Calendar(Calendar {
                time_zone: "Europe/Warsaw".into(),
                week_pattern: json!(["Mon", "Tue", "Wed", "Thu", "Fri"]),
                region: Some("PL".into()),
                holiday_source: None,
            }),
        )
    }

    fn edge(
        edge_id: &str,
        kind: RelationKind,
        from: &str,
        to: &str,
        status: ElementStatus,
    ) -> Edge {
        Edge {
            id: id(edge_id),
            revision: 1,
            status,
            kind,
            from: id(from),
            to: id(to),
            properties: RelationProperties::None,
            evidence: Vec::new(),
            derivations: Vec::new(),
            standards: Vec::new(),
            audit: AuditMeta::new(id("actor:human"), ts(AT), None, None).unwrap(),
        }
    }

    /// The synthetic graph: one Accepted requirement, an Accepted Entity named "Leave Request"
    /// owning Accepted Attributes (two of them with non-symbol names), Accepted and Proposed
    /// Calendars. No evidence, so evidence_refs are empty.
    fn base_graph(extra: Vec<Node>, extra_edges: Vec<Edge>) -> Graph {
        let mut nodes = vec![
            node(
                "req:r1",
                Accepted,
                NodePayload::Requirement(Requirement {
                    statement: R1.into(),
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
            ),
            node(
                ENTITY,
                Accepted,
                NodePayload::Entity(Entity {
                    name: "Leave Request".into(),
                    description: None,
                    aggregate_root: None,
                }),
            ),
            attribute(
                "attr:start-date",
                Accepted,
                "Start Date",
                "Date",
                None,
                None,
            ),
            attribute("attr:end-date", Accepted, "End Date", "Date", None, None),
            attribute(
                "attr:inclusive-end",
                Accepted,
                "inclusive_end",
                "Bool",
                None,
                None,
            ),
            attribute(
                "attr:day-fraction",
                Accepted,
                "day_fraction",
                "Decimal(2)",
                None,
                None,
            ),
            attribute(
                "attr:leave-type",
                Accepted,
                "leave_type",
                "Enum",
                None,
                Some(vec!["Annual", "Sick", "Unpaid"]),
            ),
            attribute(
                "attr:balance",
                Accepted,
                "remaining_days",
                "Decimal(2)",
                Some("working_day"),
                None,
            ),
            attribute("attr:count", Accepted, "count_days", "Int", None, None),
            attribute("attr:notes", Accepted, "notes", "String", None, None),
            attribute("attr:bad-enum", Accepted, "bad_enum", "Enum", None, None),
            attribute("attr:text", Accepted, "text_value", "Text", None, None),
            attribute("attr:draft", Proposed, "draft_value", "Int", None, None),
            calendar(CAL, Accepted),
            calendar("cal:draft", Proposed),
        ];
        nodes.extend(extra);
        let mut edges: Vec<Edge> = [
            "attr:start-date",
            "attr:end-date",
            "attr:inclusive-end",
            "attr:day-fraction",
            "attr:leave-type",
            "attr:balance",
            "attr:count",
            "attr:notes",
            "attr:bad-enum",
            "attr:text",
        ]
        .iter()
        .map(|a| {
            edge(
                &format!("rel:own-{}", &a[5..]),
                RelationKind::HasAttribute,
                ENTITY,
                a,
                Accepted,
            )
        })
        .collect();
        edges.push(edge(
            "rel:own-draft",
            RelationKind::HasAttribute,
            ENTITY,
            "attr:draft",
            Proposed,
        ));
        edges.extend(extra_edges);
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn root(node_ref: &str, symbol: &str) -> CalculationScopeBinding {
        CalculationScopeBinding {
            node_ref: id(node_ref),
            symbol: symbol.into(),
            exposure: CalculationBindingExposure::Root,
        }
    }

    fn field(node_ref: &str, owner: &str, symbol: &str) -> CalculationScopeBinding {
        CalculationScopeBinding {
            node_ref: id(node_ref),
            symbol: symbol.into(),
            exposure: CalculationBindingExposure::Field {
                owner_ref: id(owner),
            },
        }
    }

    /// The explicit synthetic HR scope (the spellings are explicit aliases).
    fn hr_scope() -> CalculationScope {
        CalculationScope {
            bindings: vec![
                field("attr:start-date", ENTITY, "start_date"),
                root("attr:start-date", "start_date"),
                root("attr:end-date", "end_date"),
                root("attr:inclusive-end", "inclusive_end"),
                root("attr:day-fraction", "day_fraction"),
                root(ENTITY, "LeaveRequest"),
            ],
            calendar_refs: vec![id(CAL)],
        }
    }

    fn with_bindings(extra: Vec<CalculationScopeBinding>) -> CalculationScope {
        let mut scope = hr_scope();
        scope.bindings.extend(extra);
        scope
    }

    fn provider() -> ProviderPolicy {
        ProviderPolicy {
            provider: "mock".into(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn derivation() -> DerivationRef {
        DerivationRef::from(id("drv:00000000000000c6"))
    }

    fn audit() -> CalculationAudit {
        CalculationAudit {
            created_by: id("agent:calculation"),
            created_at: ts(AT),
        }
    }

    fn artifact_for(request: &InferenceRequest, output: Json) -> InferenceArtifact {
        let raw = to_canonical_json(&output).unwrap();
        let validated_output = CanonicalJson::new(output);
        InferenceArtifact {
            request_hash: request.id.clone(),
            provider: request.provider_policy.provider.clone(),
            model: "mock-model".to_owned(),
            parameters: CanonicalJson::new(json!({})),
            raw_response_hash: Hash::content_sha256(&raw),
            validated_output_hash: validated_output.content_hash().unwrap(),
            validated_output,
        }
    }

    fn range(needle: &str) -> Json {
        let start = R1.find(needle).unwrap_or_else(|| panic!("{needle}"));
        json!({"requirement_ref": "req:r1", "start": start, "end": start + needle.len()})
    }

    fn candidate(
        name: &str,
        expression: &str,
        result_type: &str,
        unit: Option<&str>,
        rounding: Option<&str>,
        calendar: Option<&str>,
    ) -> Json {
        json!({
            "name_range": range(name),
            "expression": expression,
            "expression_evidence": [range("multiplied by the day fraction"), range("working days from the start date to the end date")],
            "result_type": result_type,
            "unit": unit,
            "rounding": rounding,
            "calendar_ref": calendar,
        })
    }

    fn half_day() -> Json {
        candidate(
            "Requested Working Days",
            "working_days( [start_date,end_date], calendar, inclusive_end )*day_fraction",
            "Decimal(2)",
            Some("working_day"),
            None,
            Some(CAL),
        )
    }

    fn output(candidates: Vec<Json>) -> Json {
        json!({"version": 1, "calculations": candidates})
    }

    fn request(graph: &Graph, scope: &CalculationScope) -> CalculationRequest {
        build_calculation_request(graph, &[id("req:r1")], scope, provider()).unwrap()
    }

    fn run(
        graph: &Graph,
        scope: &CalculationScope,
        candidates: Vec<Json>,
    ) -> Result<CalculationAnalysisResult, CalculationError> {
        let r = request(graph, scope);
        let artifact = artifact_for(&r.request, output(candidates));
        analyze_calculations(
            graph,
            &r,
            Some(CalculationInference {
                artifact: &artifact,
                derivation_ref: derivation(),
            }),
            &audit(),
        )
    }

    /// The single candidate analysis of a one-candidate run.
    fn one(graph: &Graph, scope: &CalculationScope, c: Json) -> CalculationCandidateAnalysis {
        let mut result = run(graph, scope, vec![c]).unwrap();
        assert_eq!(result.candidates.len(), 1);
        result.candidates.remove(0)
    }

    fn issues(c: &CalculationCandidateAnalysis) -> &[CalculationIssue] {
        &c.qualification.issues
    }

    fn scope_error(graph: &Graph, scope: &CalculationScope) -> CalculationIssue {
        match build_calculation_request(graph, &[id("req:r1")], scope, provider()) {
            Err(CalculationError::Scope(issue)) => issue,
            other => panic!("{other:?}"),
        }
    }

    fn apply(graph: &Graph, proposals: &[Proposal]) -> Graph {
        let mut g = graph.clone();
        for p in proposals {
            let patch_set = PatchSet {
                base_semantic_hash: g.semantic_hash().unwrap(),
                patch: p.patch_set.patch.clone(),
            };
            g = apply_patch(&g, &patch_set).unwrap().graph;
        }
        g.validate().unwrap();
        g
    }

    fn set_status(graph: &Graph, target: &str, status: ElementStatus) -> Graph {
        let nodes = graph
            .nodes()
            .values()
            .cloned()
            .map(|mut n| {
                if n.id.as_str() == target {
                    n.status = status;
                }
                n
            })
            .collect();
        // Incident edges follow a non-Accepted endpoint so the graph stays valid.
        let edges = graph
            .edges()
            .values()
            .cloned()
            .map(|mut e| {
                if status != Accepted && (e.from.as_str() == target || e.to.as_str() == target) {
                    e.status = status;
                }
                e
            })
            .collect();
        Graph::new(
            graph.project_id().clone(),
            graph.profile_id().clone(),
            nodes,
            edges,
        )
        .unwrap()
    }

    fn hr_calendars() -> StaticCalendarProvider {
        StaticCalendarProvider::new([CalendarDefinition::new(
            id(CAL),
            [Weekday::Saturday, Weekday::Sunday],
            [date(2027, 1, 1), date(2027, 1, 6)],
        )])
        .unwrap()
    }

    struct NoPredicates;

    impl PredicateProvider for NoPredicates {
        fn direct_manager(&self, _: &RefValue, _: &RefValue) -> Result<bool, PredicateError> {
            Err(PredicateError {
                reason: "unused".into(),
            })
        }
    }

    /// Records the calendar references it is asked about.
    struct Recording(RefCell<Vec<Id>>);

    impl CalendarProvider for Recording {
        fn is_working_day(&self, calendar_ref: &Id, _: Date) -> Result<bool, CalendarError> {
            self.0.borrow_mut().push(calendar_ref.clone());
            Ok(true)
        }
    }

    fn sym(s: &str) -> Symbol {
        Symbol::new(s).unwrap()
    }

    fn half_day_values() -> ValueEnv {
        let mut values = ValueEnv::new();
        values
            .bind(sym("start_date"), Value::Date(date(2027, 2, 1)))
            .unwrap();
        values
            .bind(sym("end_date"), Value::Date(date(2027, 2, 1)))
            .unwrap();
        values
            .bind(sym("inclusive_end"), Value::Bool(true))
            .unwrap();
        values
            .bind(
                sym("day_fraction"),
                Value::Decimal(Decimal::from_str("0.5").unwrap()),
            )
            .unwrap();
        values
    }

    // ------------------------------------------------------------------ scope

    #[test]
    fn calculation_scope_binding_rules() {
        let g = base_graph(vec![], vec![]);
        assert_eq!(CALCULATION_CALENDAR_SYMBOL, "calendar");
        // The reserved root.
        assert_eq!(
            scope_error(&g, &with_bindings(vec![root("attr:count", "calendar")])),
            CalculationIssue::ReservedSymbol {
                node_ref: id("attr:count")
            }
        );
        request(
            &g,
            &with_bindings(vec![field("attr:count", ENTITY, "calendar")]),
        );
        // Collisions and duplicates.
        assert!(matches!(
            scope_error(&g, &with_bindings(vec![root("attr:count", "start_date")])),
            CalculationIssue::ScopeBindingConflict {
                owner_ref: None,
                ..
            }
        ));
        assert!(matches!(
            scope_error(
                &g,
                &with_bindings(vec![field("attr:end-date", ENTITY, "start_date")])
            ),
            CalculationIssue::ScopeBindingConflict {
                owner_ref: Some(_),
                ..
            }
        ));
        assert!(matches!(
            scope_error(&g, &with_bindings(vec![root("attr:end-date", "end_date")])),
            CalculationIssue::ScopeBindingConflict { .. }
        ));
        // Exact symbols only; no transformation.
        for bad in ["start date", "Start Date", "2x", "if", "a.b", ""] {
            assert!(
                matches!(
                    scope_error(&g, &with_bindings(vec![root("attr:count", bad)])),
                    CalculationIssue::InvalidScope { .. }
                ),
                "{bad}"
            );
        }
        // Owners, node types, statuses and unknown nodes.
        assert!(matches!(
            scope_error(
                &g,
                &with_bindings(vec![field("attr:count", "attr:notes", "c")])
            ),
            CalculationIssue::InvalidScope { .. }
        ));
        assert!(matches!(
            scope_error(&g, &with_bindings(vec![root("req:r1", "r")])),
            CalculationIssue::InvalidScope { .. }
        ));
        assert!(matches!(
            scope_error(&g, &with_bindings(vec![root(CAL, "cal")])),
            CalculationIssue::InvalidScope { .. }
        ));
        assert!(matches!(
            scope_error(&g, &with_bindings(vec![root("attr:draft", "draft")])),
            CalculationIssue::InvalidScope { .. }
        ));
        assert!(matches!(
            scope_error(&g, &with_bindings(vec![root("attr:ghost", "ghost")])),
            CalculationIssue::UnknownInputRef { .. }
        ));
        assert!(matches!(
            scope_error(&g, &with_bindings(vec![root("attr:bad-enum", "bad_enum")])),
            CalculationIssue::UnsupportedInputType { .. }
        ));
        assert!(matches!(
            scope_error(&g, &with_bindings(vec![root("attr:text", "text_value")])),
            CalculationIssue::UnsupportedInputType { .. }
        ));
        let with_calc = base_graph(
            vec![calculation_node(
                "calculation:x",
                Accepted,
                "x",
                "1",
                "Int",
                None,
            )],
            vec![],
        );
        assert!(matches!(
            scope_error(
                &with_calc,
                &with_bindings(vec![field("calculation:x", ENTITY, "x")])
            ),
            CalculationIssue::InvalidScope { .. }
        ));
        // Calendars inference may select.
        let mut scope = hr_scope();
        scope.calendar_refs.push(id("cal:draft"));
        assert_eq!(
            scope_error(&g, &scope),
            CalculationIssue::CalendarUnresolved {
                calendar_ref: id("cal:draft")
            }
        );
        let mut scope = hr_scope();
        scope.calendar_refs.push(id(CAL));
        assert!(matches!(
            scope_error(&g, &scope),
            CalculationIssue::InvalidScope { .. }
        ));
        // The canonical order: Root before Field, then owner, symbol, node.
        let r = request(&g, &hr_scope());
        let order: Vec<(String, bool)> = r
            .scope
            .bindings
            .iter()
            .map(|b| {
                (
                    b.symbol.clone(),
                    matches!(b.exposure, CalculationBindingExposure::Field { .. }),
                )
            })
            .collect();
        assert_eq!(
            order,
            [
                ("LeaveRequest", false),
                ("day_fraction", false),
                ("end_date", false),
                ("inclusive_end", false),
                ("start_date", false),
                ("start_date", true)
            ]
            .map(|(s, f)| (s.to_owned(), f))
        );
    }

    #[test]
    fn calculation_default_bindings() {
        let g = base_graph(vec![], vec![]);
        assert_eq!(
            default_root_binding(&g, &id("attr:day-fraction")),
            Ok(root("attr:day-fraction", "day_fraction"))
        );
        assert_eq!(
            default_field_binding(&g, &id("attr:inclusive-end"), &id(ENTITY)),
            Ok(field("attr:inclusive-end", ENTITY, "inclusive_end"))
        );
        assert_eq!(
            default_root_binding(&g, &id("attr:start-date")),
            Err(CalculationIssue::UnbindablePayloadName {
                node_ref: id("attr:start-date"),
                name: "Start Date".into()
            })
        );
        assert!(matches!(
            default_root_binding(&g, &id(ENTITY)),
            Err(CalculationIssue::UnbindablePayloadName { .. })
        ));
        let named_calendar = base_graph(
            vec![attribute(
                "attr:cal", Accepted, "calendar", "Int", None, None,
            )],
            vec![edge(
                "rel:own-cal",
                RelationKind::HasAttribute,
                ENTITY,
                "attr:cal",
                Accepted,
            )],
        );
        assert!(matches!(
            default_root_binding(&named_calendar, &id("attr:cal")),
            Err(CalculationIssue::UnbindablePayloadName { .. })
        ));
        // An explicit alias over the non-symbol name works; the PSG name is unchanged.
        let c = one(
            &g,
            &hr_scope(),
            candidate(
                "Requested Working Days",
                "start_date <= end_date",
                "Bool",
                None,
                None,
                None,
            ),
        );
        assert!(c.qualification.is_qualified(), "{:?}", issues(&c));
        assert!(
            matches!(&g.node(&id("attr:start-date")).unwrap().payload, NodePayload::Attribute(a) if a.name == "Start Date")
        );
    }

    // ------------------------------------------------------------------ request

    #[test]
    fn calculation_request_golden() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
        let g = base_graph(vec![], vec![]);
        let r = request(&g, &hr_scope());
        assert_eq!(r.request.context_hash.as_str(), GOLDEN_CONTEXT_HASH);
        assert_eq!(r.request.id.as_str(), GOLDEN_REQUEST_ID);
        assert_eq!(r.request.stage, StageId::S2);
        assert_eq!(r.request.task_kind, "calculation_analysis");
        assert!(r.request.evidence_refs.is_empty());
        assert_eq!(CALCULATION_CONTEXT_VERSION, 1);
        assert_eq!(CALCULATION_OUTPUT_VERSION, 1);
        assert_eq!(CALCULATION_TASK_KIND, "calculation_analysis");
        assert_eq!(
            CALCULATION_ORIGIN_EXTENSION,
            "plumb_functional:calculation_origin"
        );
        let calendars = serde_json::to_value(&r.context.calendars).unwrap();
        assert_eq!(
            calendars,
            json!([{"calendar_ref": CAL, "time_zone": "Europe/Warsaw", "region": "PL"}])
        );
        // No project-wide context: only the explicit bindings and calendars.
        assert_eq!(r.context.available_inputs.len(), 6);
        assert!(build_calculation_request(&g, &[], &hr_scope(), provider()).is_err());
        let prompt = std::str::from_utf8(PROMPT).unwrap();
        for required in [
            "Use only the symbols supplied in available_inputs",
            "reserved root symbol `calendar`",
            "Do not use\n`calendar` unless a calendar_ref is selected",
            "Do not invent units, rounding or calendars",
            "Do not return dependencies",
        ] {
            assert!(prompt.contains(required), "{required}");
        }
        for forbidden in ["input_refs\"", "G_CALC", "G_TIME", "G_QTY"] {
            assert!(!SCHEMA.contains(forbidden), "{forbidden}");
        }
    }

    // ------------------------------------------------------------------ proposals and evaluation

    #[test]
    fn calculation_half_day_proposal_and_evaluation() {
        let g = base_graph(vec![], vec![]);
        let result = run(&g, &hr_scope(), vec![half_day()]).unwrap();
        assert!(result.issues.is_empty());
        let c = &result.candidates[0];
        assert!(c.qualification.is_qualified(), "{:?}", issues(c));
        assert_eq!(c.calculation_ref.as_str(), GOLDEN_CALCULATION_ID);
        assert_eq!(c.name, "Requested Working Days");
        assert_eq!(
            c.qualification.canonical_expression.as_deref(),
            Some(HALF_DAY)
        );
        assert_eq!(
            c.qualification.actual_ty,
            Some(Ty::Quantity(Unit::new("working_day").unwrap()))
        );
        assert!(c.qualification.requires_calendar);
        // Used bindings come from the AST: no LeaveRequest, no Field, no calendar.
        assert_eq!(
            c.qualification.used_bindings,
            vec![
                root("attr:day-fraction", "day_fraction"),
                root("attr:end-date", "end_date"),
                root("attr:inclusive-end", "inclusive_end"),
                root("attr:start-date", "start_date")
            ]
        );
        assert!(c.qualification.dependencies.is_empty());
        assert_eq!(result.proposals.len(), 1);
        let p = &result.proposals[0];
        assert!(
            matches!(c.disposition, CalculationDisposition::Proposed { ref proposal_ref } if *proposal_ref == p.id)
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
        assert_eq!(p.derivation_refs, vec![derivation()]);
        let SemanticPatch::AddNode { node } = &p.patch_set.patch else {
            panic!()
        };
        assert_eq!(node.status, Proposed);
        assert_eq!(node.revision, 1);
        assert_eq!(
            node.payload,
            NodePayload::Calculation(Calculation {
                name: "Requested Working Days".into(),
                expression: HALF_DAY.into(),
                result_type: "Decimal(2)".into(),
                unit: Some("working_day".into()),
                rounding: None,
                calendar_ref: Some(id(CAL)),
                examples: None,
            })
        );
        let origin = node
            .extensions
            .iter()
            .find(|(k, _)| k.as_str() == CALCULATION_ORIGIN_EXTENSION)
            .map(|(_, v)| v.clone())
            .unwrap();
        let origin: CalculationOrigin = serde_json::from_value(origin).unwrap();
        assert_eq!(origin.used_bindings, c.qualification.used_bindings);
        assert_eq!(origin.expression_evidence.len(), 2);
        assert!(origin.expression_evidence.windows(2).all(|w| w[0] < w[1]));
        apply(&g, &result.proposals);
        // The reserved calendar: the Calendar node ID is the Ref type and value identity.
        let prepared = c.qualification.prepared.as_ref().unwrap();
        assert_eq!(prepared.type_env.calendar_ref_type(), Some(&id(CAL)));
        assert_eq!(
            prepared.type_env.root(&sym("calendar")),
            Some(&Ty::Ref(id(CAL)))
        );
        let value =
            evaluate_calculation(prepared, &half_day_values(), &hr_calendars(), &NoPredicates)
                .unwrap();
        assert_eq!(
            value,
            Value::Quantity {
                value: Decimal::from_str("0.5").unwrap(),
                unit: Unit::new("working_day").unwrap()
            }
        );
        let recording = Recording(RefCell::new(Vec::new()));
        evaluate_calculation(prepared, &half_day_values(), &recording, &NoPredicates).unwrap();
        assert_eq!(recording.0.borrow().as_slice(), [id(CAL)]);
        // A caller-supplied calendar root conflicts with the reserved binding.
        let mut conflicting = half_day_values();
        conflicting.bind(sym("calendar"), Value::Int(1)).unwrap();
        assert_eq!(
            evaluate_calculation(prepared, &conflicting, &hr_calendars(), &NoPredicates),
            Err(CalculationEvalError::ReservedBindingConflict)
        );
    }

    #[test]
    fn calculation_hr_explicit_spellings() {
        // Leave Request -> LeaveRequest and Start Date -> Field start_date (explicit aliases).
        let g = base_graph(vec![], vec![]);
        let c = one(
            &g,
            &hr_scope(),
            candidate(
                "Requested Working Days",
                "LeaveRequest.start_date <= end_date",
                "Bool",
                None,
                None,
                None,
            ),
        );
        assert!(c.qualification.is_qualified(), "{:?}", issues(&c));
        assert_eq!(
            c.qualification.used_bindings,
            vec![
                root(ENTITY, "LeaveRequest"),
                root("attr:end-date", "end_date"),
                field("attr:start-date", ENTITY, "start_date")
            ]
        );
        assert_eq!(
            one(
                &g,
                &hr_scope(),
                candidate(
                    "Requested Working Days",
                    "LeaveRequest.end_date <= end_date",
                    "Bool",
                    None,
                    None,
                    None
                )
            )
            .qualification
            .undefined_inputs,
            vec!["LeaveRequest.end_date".to_owned()]
        );
        // A stored Calculation named "Requested Working Days" bound as RequestedWorkingDays.
        let g = base_graph(
            vec![calculation_node(
                "calculation:rwd",
                Accepted,
                "Requested Working Days",
                HALF_DAY,
                "Decimal(2)",
                Some("working_day"),
            )],
            vec![],
        );
        let scope = with_bindings(vec![
            root("calculation:rwd", "RequestedWorkingDays"),
            root("attr:leave-type", "leave_type"),
        ]);
        let annual = one(
            &g,
            &scope,
            candidate(
                "Requested Working Days",
                "if leave_type == Annual then RequestedWorkingDays else 0",
                "Decimal(2)",
                Some("working_day"),
                None,
                None,
            ),
        );
        assert!(annual.qualification.is_qualified(), "{:?}", issues(&annual));
        assert_eq!(
            annual.qualification.actual_ty,
            Some(Ty::Quantity(Unit::new("working_day").unwrap()))
        );
        // The contextual enum member Annual is not a binding; the Calculation is a dependency.
        assert_eq!(
            annual.qualification.used_bindings,
            vec![
                root("calculation:rwd", "RequestedWorkingDays"),
                root("attr:leave-type", "leave_type")
            ]
        );
        assert_eq!(
            annual.qualification.dependencies,
            vec![id("calculation:rwd")]
        );
        assert!(
            matches!(&g.node(&id("calculation:rwd")).unwrap().payload, NodePayload::Calculation(c) if c.name == "Requested Working Days")
        );
    }

    #[test]
    fn calculation_result_unit_rounding_and_calendar_qualification() {
        let g = base_graph(vec![], vec![]);
        let scope = with_bindings(vec![
            root("attr:count", "count_days"),
            root("attr:balance", "remaining_days"),
            root("attr:notes", "notes"),
        ]);
        let q = |expr: &str,
                 rt: &str,
                 unit: Option<&str>,
                 rounding: Option<&str>,
                 cal: Option<&str>| {
            one(
                &g,
                &scope,
                candidate("Requested Working Days", expr, rt, unit, rounding, cal),
            )
        };
        let ok = |c: CalculationCandidateAnalysis| {
            assert!(
                c.qualification.is_qualified(),
                "{:?}",
                c.qualification.issues
            )
        };
        let has = |c: CalculationCandidateAnalysis, f: fn(&CalculationIssue) -> bool| {
            assert!(
                c.qualification.issues.iter().any(f),
                "{:?}",
                c.qualification.issues
            )
        };
        ok(q(
            "days_between(start_date, end_date)",
            "Int",
            None,
            None,
            None,
        ));
        ok(q("count_days + 1", "Decimal(0)", None, None, None));
        ok(q("day_fraction * 2", "Decimal(2)", None, None, None));
        ok(q(
            "remaining_days",
            "Decimal(2)",
            Some("working_day"),
            None,
            None,
        ));
        ok(q("notes", "String", None, None, None));
        // Result type and unit mismatches.
        has(
            q("day_fraction * 1.5", "Decimal(2)", None, None, None),
            |i| matches!(i, CalculationIssue::ResultTypeMismatch { .. }),
        );
        has(q("count_days", "Bool", None, None, None), |i| {
            matches!(i, CalculationIssue::ResultTypeMismatch { .. })
        });
        has(
            q(
                "day_fraction",
                "Decimal(2)",
                Some("working_day"),
                None,
                None,
            ),
            |i| matches!(i, CalculationIssue::UnitMismatch { .. }),
        );
        has(q("remaining_days", "Decimal(2)", None, None, None), |i| {
            matches!(i, CalculationIssue::UnitMismatch { .. })
        });
        has(
            q(HALF_DAY, "Decimal(2)", Some("hour"), None, Some(CAL)),
            |i| matches!(i, CalculationIssue::UnitMismatch { .. }),
        );
        has(q("notes", "String", Some("day"), None, None), |i| {
            matches!(i, CalculationIssue::UnitMismatch { .. })
        });
        // Parse errors and undefined inputs.
        has(q("start_date <=", "Bool", None, None, None), |i| {
            matches!(i, CalculationIssue::ParseError { .. })
        });
        has(
            q("unknown_symbol + 1", "Int", None, None, None),
            |i| matches!(i, CalculationIssue::UndefinedInput { identifier } if identifier == "unknown_symbol"),
        );
        has(
            q("calendar == calendar", "Bool", None, None, None),
            |i| matches!(i, CalculationIssue::UndefinedInput { identifier } if identifier == "calendar"),
        );
        // Rounding: Decimal and Quantity results only.
        ok(q(
            "day_fraction * 2",
            "Decimal(2)",
            None,
            Some("half_up(2)"),
            None,
        ));
        ok(q(
            HALF_DAY,
            "Decimal(2)",
            Some("working_day"),
            Some("to_step(0.5)"),
            Some(CAL),
        ));
        has(
            q("count_days", "Int", None, Some("half_up(0)"), None),
            |i| matches!(i, CalculationIssue::RoundingNotApplicable { .. }),
        );
        has(
            q(
                "start_date <= end_date",
                "Bool",
                None,
                Some("half_up(0)"),
                None,
            ),
            |i| matches!(i, CalculationIssue::RoundingNotApplicable { .. }),
        );
        has(
            q("day_fraction", "Decimal(2)", None, Some("half-up(2)"), None),
            |i| matches!(i, CalculationIssue::InvalidRounding { .. }),
        );
        // Business time.
        has(
            q(
                "working_days([start_date, end_date], LeaveRequest, true)",
                "Decimal(2)",
                Some("working_day"),
                None,
                None,
            ),
            |i| matches!(i, CalculationIssue::CalendarRequired),
        );
        ok(q(
            "days_between(start_date, end_date)",
            "Int",
            None,
            None,
            None,
        ));
        ok(q("day_fraction", "Decimal(2)", None, None, Some(CAL)));
        // Schema-level result types: no Enum, no Decimal(29), no Text.
        for rt in ["Enum", "Decimal(29)", "Text", "Money", "Decimal(02)"] {
            assert!(
                matches!(
                    run(
                        &g,
                        &scope,
                        vec![candidate(
                            "Requested Working Days",
                            "count_days",
                            rt,
                            None,
                            None,
                            None
                        )]
                    ),
                    Err(CalculationError::SchemaInvalid { .. })
                ),
                "{rt}"
            );
        }
    }

    #[test]
    fn calculation_rounding_evaluation() {
        let g = base_graph(vec![], vec![]);
        let c = one(
            &g,
            &hr_scope(),
            candidate(
                "Requested Working Days",
                HALF_DAY,
                "Decimal(2)",
                Some("working_day"),
                Some("half_up(0)"),
                Some(CAL),
            ),
        );
        let value = evaluate_calculation(
            c.qualification.prepared.as_ref().unwrap(),
            &half_day_values(),
            &hr_calendars(),
            &NoPredicates,
        )
        .unwrap();
        assert_eq!(
            value,
            Value::Quantity {
                value: Decimal::from_str("1").unwrap(),
                unit: Unit::new("working_day").unwrap()
            }
        );
        let c = one(
            &g,
            &hr_scope(),
            candidate(
                "Requested Working Days",
                "day_fraction / 3",
                "Decimal(28)",
                None,
                Some("half_even(2)"),
                None,
            ),
        );
        let value = evaluate_calculation(
            c.qualification.prepared.as_ref().unwrap(),
            &half_day_values(),
            &hr_calendars(),
            &NoPredicates,
        )
        .unwrap();
        assert_eq!(value, Value::Decimal(Decimal::from_str("0.17").unwrap()));
    }

    // ------------------------------------------------------------------ inference

    #[test]
    fn calculation_inference_availability_and_artifacts() {
        let g = base_graph(vec![], vec![]);
        let r = request(&g, &hr_scope());
        let absent = analyze_calculations(&g, &r, None, &audit()).unwrap();
        assert_eq!(absent.issues, vec![CalculationIssue::InferenceUnavailable]);
        assert!(absent.proposals.is_empty() && absent.candidates.is_empty());
        let good = artifact_for(&r.request, output(vec![half_day()]));
        let mut wrong = good.clone();
        wrong.request_hash = Hash::content_sha256(b"other");
        assert!(matches!(
            analyze_calculations(
                &g,
                &r,
                Some(CalculationInference {
                    artifact: &wrong,
                    derivation_ref: derivation()
                }),
                &audit()
            ),
            Err(CalculationError::InvalidInferenceArtifact { .. })
        ));
        let mut with_inputs = half_day();
        with_inputs["input_refs"] = json!(["attr:start-date"]);
        assert!(matches!(
            run(&g, &hr_scope(), vec![with_inputs]),
            Err(CalculationError::SchemaInvalid { .. })
        ));
        let mut out_of_range = half_day();
        out_of_range["name_range"] = json!({"requirement_ref": "req:r1", "start": 0, "end": 9999});
        assert!(matches!(
            run(&g, &hr_scope(), vec![out_of_range]),
            Err(CalculationError::InvalidGrounding { .. })
        ));
        let mut unknown_req = half_day();
        unknown_req["name_range"] = json!({"requirement_ref": "req:r9", "start": 0, "end": 3});
        assert!(matches!(
            run(&g, &hr_scope(), vec![unknown_req]),
            Err(CalculationError::InvalidGrounding { .. })
        ));
        let mut duplicate_evidence = half_day();
        duplicate_evidence["expression_evidence"] =
            json!([range("day fraction"), range("day fraction")]);
        assert!(matches!(
            run(&g, &hr_scope(), vec![duplicate_evidence]),
            Err(CalculationError::InvalidGrounding { .. })
        ));
        let mut not_allowed = half_day();
        not_allowed["calendar_ref"] = json!("cal:draft");
        assert!(matches!(
            run(&g, &hr_scope(), vec![not_allowed]),
            Err(CalculationError::CalendarNotAllowed { .. })
        ));
        assert!(matches!(
            run(&g, &hr_scope(), vec![half_day(), half_day()]),
            Err(CalculationError::DuplicateCandidate { .. })
        ));
        // A blank grounded name is ineligible, never repaired.
        let mut blank = half_day();
        let space = R1.find(' ').unwrap();
        blank["name_range"] =
            json!({"requirement_ref": "req:r1", "start": space, "end": space + 1});
        let c = one(&g, &hr_scope(), blank);
        assert!(c
            .qualification
            .issues
            .iter()
            .any(|i| matches!(i, CalculationIssue::InvalidName { .. })));
        assert_eq!(c.disposition, CalculationDisposition::Ineligible);
        // A stale graph is rejected.
        let changed = set_status(&g, "attr:start-date", Rejected);
        assert!(analyze_calculations(&changed, &r, None, &audit()).is_err());
    }

    #[test]
    fn calculation_reconciliation() {
        let g = base_graph(vec![], vec![]);
        let first = run(&g, &hr_scope(), vec![half_day()]).unwrap();
        let applied = apply(&g, &first.proposals);
        let replay = one(&applied, &hr_scope(), half_day());
        assert_eq!(replay.disposition, CalculationDisposition::IdempotentReplay);
        let accepted = set_status(&applied, GOLDEN_CALCULATION_ID, Accepted);
        let equivalent = run(&accepted, &hr_scope(), vec![half_day()]).unwrap();
        assert_eq!(
            equivalent.candidates[0].disposition,
            CalculationDisposition::ExistingEquivalent
        );
        assert!(equivalent.proposals.is_empty());
        let conflicting = base_graph(
            vec![calculation_node(
                GOLDEN_CALCULATION_ID,
                Proposed,
                "Requested Working Days",
                "1",
                "Int",
                None,
            )],
            vec![],
        );
        let conflict = one(&conflicting, &hr_scope(), half_day());
        assert_eq!(
            conflict.disposition,
            CalculationDisposition::ExistingConflict
        );
        assert!(conflict.qualification.issues.contains(
            &CalculationIssue::ExistingCalculationConflict {
                node_ref: id(GOLDEN_CALCULATION_ID)
            }
        ));
    }

    // ------------------------------------------------------------------ accepted qualification

    #[test]
    fn calculation_accepted_origin_replay_and_legacy_overrides() {
        let g = base_graph(vec![], vec![]);
        let applied = apply(
            &g,
            &run(&g, &hr_scope(), vec![half_day()]).unwrap().proposals,
        );
        let accepted = set_status(&applied, GOLDEN_CALCULATION_ID, Accepted);
        let calc = id(GOLDEN_CALCULATION_ID);
        let replay = qualify_accepted_calculation(&accepted, &calc, None).unwrap();
        assert!(replay.from_origin);
        assert!(
            replay.qualification.is_qualified(),
            "{:?}",
            replay.qualification.issues
        );
        let unexpected = qualify_accepted_calculation(&accepted, &calc, Some(&hr_scope())).unwrap();
        assert_eq!(
            unexpected.qualification.issues,
            vec![CalculationIssue::UnexpectedScopeOverride {
                calculation_ref: calc.clone()
            }]
        );
        // A used binding whose node is no longer Accepted is stale.
        let stale = set_status(&accepted, "attr:day-fraction", Rejected);
        let stale_q = qualify_accepted_calculation(&stale, &calc, None).unwrap();
        assert!(
            stale_q
                .qualification
                .issues
                .iter()
                .any(|i| matches!(i, CalculationIssue::StaleScopeBinding { .. })),
            "{:?}",
            stale_q.qualification.issues
        );
        // A legacy Accepted Calculation needs an explicit override; no scope is guessed.
        let legacy = base_graph(
            vec![calculation_node(
                "calculation:legacy",
                Accepted,
                "day_fraction_twice",
                "day_fraction * 2",
                "Decimal(2)",
                None,
            )],
            vec![],
        );
        let legacy_id = id("calculation:legacy");
        let unavailable = qualify_accepted_calculation(&legacy, &legacy_id, None).unwrap();
        assert!(!unavailable.from_origin);
        assert_eq!(
            unavailable.qualification.issues,
            vec![CalculationIssue::ScopeUnavailable {
                calculation_ref: legacy_id.clone()
            }]
        );
        let with_override =
            qualify_accepted_calculation(&legacy, &legacy_id, Some(&hr_scope())).unwrap();
        assert!(
            with_override.qualification.is_qualified(),
            "{:?}",
            with_override.qualification.issues
        );
        // A payload calendar that is no longer Accepted is unresolved.
        let mut cal_calc = calculation_node(
            "calculation:cal",
            Accepted,
            "x",
            HALF_DAY,
            "Decimal(2)",
            Some("working_day"),
        );
        if let NodePayload::Calculation(c) = &mut cal_calc.payload {
            c.calendar_ref = Some(id("cal:draft"));
        }
        let g2 = base_graph(vec![cal_calc], vec![]);
        let q =
            qualify_accepted_calculation(&g2, &id("calculation:cal"), Some(&hr_scope())).unwrap();
        assert!(q
            .qualification
            .issues
            .contains(&CalculationIssue::CalendarUnresolved {
                calendar_ref: id("cal:draft")
            }));
        assert!(qualify_accepted_calculation(&g, &id("attr:count"), None).is_err());
    }

    #[test]
    fn calculation_dependency_cycles() {
        let calc = |n: &str, expr: &str| {
            calculation_node(&format!("calculation:{n}"), Accepted, n, expr, "Int", None)
        };
        let g = base_graph(
            vec![
                calc("a", "b_value + 1"),
                calc("b", "a_value + 1"),
                calc("c", "c_value + 1"),
                calc("d", "b_value * 2"),
                calc("e", "1"),
            ],
            vec![],
        );
        let scope = |bindings: Vec<CalculationScopeBinding>| CalculationScope {
            bindings,
            calendar_refs: vec![],
        };
        let mut overrides = BTreeMap::new();
        overrides.insert(
            id("calculation:a"),
            scope(vec![root("calculation:b", "b_value")]),
        );
        overrides.insert(
            id("calculation:b"),
            scope(vec![root("calculation:a", "a_value")]),
        );
        overrides.insert(
            id("calculation:c"),
            scope(vec![root("calculation:c", "c_value")]),
        );
        overrides.insert(
            id("calculation:d"),
            scope(vec![root("calculation:b", "b_value")]),
        );
        let analysis = analyze_calculation_cycles(&g, &overrides).unwrap();
        assert_eq!(
            analysis.cycles,
            vec![
                vec![id("calculation:a"), id("calculation:b")],
                vec![id("calculation:c")]
            ]
        );
        let in_cycle: Vec<(&str, bool)> = analysis
            .qualifications
            .iter()
            .map(|q| (q.calculation_ref.as_str(), q.in_cycle))
            .collect();
        assert_eq!(
            in_cycle,
            [
                ("calculation:a", true),
                ("calculation:b", true),
                ("calculation:c", true),
                ("calculation:d", false),
                ("calculation:e", false)
            ]
        );
        // e has no origin and no override: its dependencies are unknown, not guessed.
        assert_eq!(analysis.incomplete, vec![id("calculation:e")]);
        let d = analysis
            .qualifications
            .iter()
            .find(|q| q.calculation_ref.as_str() == "calculation:d")
            .unwrap();
        assert_eq!(d.qualification.dependencies, vec![id("calculation:b")]);
        // An acyclic chain.
        let chain = base_graph(
            vec![
                calc("x", "y_value + 1"),
                calc("y", "z_value + 1"),
                calc("z", "1"),
            ],
            vec![],
        );
        let mut chain_overrides = BTreeMap::new();
        chain_overrides.insert(
            id("calculation:x"),
            scope(vec![root("calculation:y", "y_value")]),
        );
        chain_overrides.insert(
            id("calculation:y"),
            scope(vec![root("calculation:z", "z_value")]),
        );
        chain_overrides.insert(id("calculation:z"), scope(vec![]));
        let chain_analysis = analyze_calculation_cycles(&chain, &chain_overrides).unwrap();
        assert!(chain_analysis.cycles.is_empty() && chain_analysis.incomplete.is_empty());
        // p -> q -> p in prose, but q is legacy without an override: the cycle cannot be
        // established, so it is reported as incomplete rather than guessed either way.
        let blocked = base_graph(
            vec![calc("p", "q_value + 1"), calc("q", "p_value + 1")],
            vec![],
        );
        let mut blocked_overrides = BTreeMap::new();
        blocked_overrides.insert(
            id("calculation:p"),
            scope(vec![root("calculation:q", "q_value")]),
        );
        let blocked_analysis = analyze_calculation_cycles(&blocked, &blocked_overrides).unwrap();
        assert!(blocked_analysis.cycles.is_empty());
        assert_eq!(blocked_analysis.incomplete, vec![id("calculation:q")]);
        let q = blocked_analysis
            .qualifications
            .iter()
            .find(|q| q.calculation_ref.as_str() == "calculation:q")
            .unwrap();
        assert_eq!(
            q.qualification.issues,
            vec![CalculationIssue::ScopeUnavailable {
                calculation_ref: id("calculation:q")
            }]
        );
        assert!(!q.qualification.is_qualified());
    }

    // ------------------------------------------------------------------ guards

    #[test]
    fn calculation_source_guard() {
        let source = include_str!("../src/calculation.rs");
        for forbidden in [
            "std::fs",
            "File::open",
            "reqwest",
            ".execute(",
            "MockProvider",
            "ArtifactStore",
            "RevisionStore",
            "rusqlite",
            "SystemClock",
            "Clock::now",
            "Utc::now",
            "Instant::now",
            "unsafe",
            "f64",
            "f32",
            "rand::",
            "commit(",
            "PLUMB.F2",
            "DMN.F2",
            "GeneratedFinding",
            "G_QTY",
            "G_CALC",
            "G_TIME",
            "UsesCalculation",
            "uses_calculation",
            "NodePayload::Operation",
            "NodePayload::Event",
            "to_lowercase",
            "to_ascii_lowercase",
            "replace(' '",
            "snake_case(",
            "region.as",
            "time_zone.as",
            "holiday_source.as",
            "LeaveRequest",
            "start_date",
            "Annual",
            "cal:PL",
        ] {
            assert!(
                !source.contains(forbidden),
                "calculation.rs contains {forbidden}"
            );
        }
        let lib = include_str!("../src/lib.rs");
        assert!(!lib.contains("Proposal"));
        assert!(lib.contains("pub mod calculation;") && lib.contains("pub mod decision_table;"));
    }
}
