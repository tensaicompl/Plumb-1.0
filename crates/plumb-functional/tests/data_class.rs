//! S2.3 contract tests for pilot data classification: the dictionary, the request,
//! dictionary AUTO_DERIVATION and inference HUMAN_CONFIRM ReplacePayload proposals, existing
//! classifications, replay across applied proposals, CAS, determinism, functional-v2
//! projection compatibility and the real HR diagnostic.
//!
//! Every test is named `s2_data_class_*` so that `cargo test -p plumb-functional
//! s2_data_class_` selects exactly them. Golden values were computed independently with
//! Python `hashlib` over RFC 8785 JSON, never with the helpers under test. The synthetic
//! Attributes are a structural conformance fixture, not a classification benchmark.

mod data_class_contract {
    use std::collections::{BTreeMap, BTreeSet};

    use jsonschema::{Draft, JSONSchema};
    use plumb_core::{to_canonical_json, CanonicalJson, Hash, Id, StageId, Timestamp};
    use plumb_functional::data_class::*;
    use plumb_functional::{
        analyze_domain, build_domain_request, build_requirement_classification_request,
        compile_requirement_candidates, normalize_vocabulary_term, project_functional_v2,
        DomainAudit, DomainInference, RequirementClassificationInference,
        RequirementCompilationAudit,
    };
    use plumb_import::{
        build_segmentation_request, evaluate_segmentation, import_markdown, import_plain_text,
        ImportAudit, SegmentationAudit,
    };
    use plumb_inference::{InferenceArtifact, InferenceRequest, ProviderPolicy};
    use plumb_patch::{
        apply_patch, AcceptancePolicy, PatchError, Proposal, ProposalMateriality, SemanticPatch,
    };
    use plumb_psg::{
        node_element_hash, Attribute, AuditMeta, DerivationRef, Edge, ElementStatus, Entity,
        EvidenceRef, Graph, Node, NodePayload, RelationKind, RelationProperties,
    };
    use plumb_validation::load_builtin_software_profile;
    use serde_json::{json, Value};

    use ElementStatus::{Accepted, Deprecated, Proposed, Rejected, Superseded, Suspect};

    const PROMPT: &[u8] = include_bytes!("../../../prompts/s2-data-classification.md");
    const SCHEMA: &str =
        include_str!("../../../schemas/inference/s2-data-classification.schema.json");
    const HR_MD: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.md");

    // Independently computed goldens (Python hashlib + canonical JSON).
    const PROMPT_HASH: &str =
        "sha256:d6b5933bbe7ac4c82e2bd2ae880112463fca7492457c8d56489dee73f64b4c75";
    const SCHEMA_HASH: &str =
        "sha256:0fd31a1b09e8aa1da9cef149e5d43669ea1c094abe0e955c4925231da395e271";
    const GOLDEN_CONTEXT_HASH: &str =
        "sha256:4d4c5ed53c607e6b798a0b5a0b3b09e2be36f10414da522e37f1114063aabd4d";
    const GOLDEN_REQUEST_ID: &str =
        "sha256:1b922bbfa7c58289235edbc05b7b2d6ded228a0d9bca706e5a8d1b0f7ef76113";

    const PROJECT: &str = "project:pilot";
    const PROFILE: &str = "profile:plumb-software-2026.1";
    const AT: &str = "2026-01-01T00:00:00.000000000Z";
    const OWNER: &str = "entity:customer";

    // ------------------------------------------------------------------ builders

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ids(v: &[&str]) -> Vec<Id> {
        v.iter().map(|s| id(s)).collect()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn provider(name: &str) -> ProviderPolicy {
        ProviderPolicy {
            provider: name.to_owned(),
            config: CanonicalJson::new(json!({})),
        }
    }

    fn derivation() -> DerivationRef {
        DerivationRef::from(id("drv:00000000000000f3"))
    }

    fn artifact_for(request: &InferenceRequest, output: Value) -> InferenceArtifact {
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

    /// One synthetic Attribute row: ID, status, name, value type, nullable, unit,
    /// precision, enum values, classification.
    #[derive(Clone)]
    struct Row {
        id: &'static str,
        status: ElementStatus,
        attribute: Attribute,
    }

    fn text(id: &'static str, name: &str, class: Option<&str>) -> Row {
        Row {
            id,
            status: Accepted,
            attribute: Attribute {
                name: name.into(),
                value_type: "Text".into(),
                nullable: false,
                unit: None,
                precision: None,
                enum_values: None,
                data_classification: class.map(str::to_owned),
            },
        }
    }

    fn with_status(mut row: Row, status: ElementStatus) -> Row {
        row.status = status;
        row
    }

    /// An Accepted Entity owning each Attribute through a has_attribute edge whose status is
    /// Accepted for baseline Attributes and Proposed otherwise.
    fn graph_of(rows: &[Row], evidence: bool) -> Graph {
        let mut nodes = vec![node(
            OWNER,
            Accepted,
            NodePayload::Entity(Entity {
                name: "Customer".into(),
                description: None,
                aggregate_root: None,
            }),
        )];
        let mut fragments = Vec::new();
        if evidence {
            let imported = import_plain_text(
                "synthetic.txt",
                b"Customers have an email address.\n\nCustomers keep internal notes.",
                &ImportAudit {
                    created_by: id("actor:importer"),
                    created_at: ts(AT),
                },
            )
            .unwrap();
            fragments = imported
                .fragments
                .iter()
                .map(|f| EvidenceRef::from(f.id.clone()))
                .collect();
            nodes.push(imported.source);
            nodes.extend(imported.fragments);
        }
        let mut edges = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let mut n = node(
                row.id,
                row.status,
                NodePayload::Attribute(row.attribute.clone()),
            );
            if evidence {
                n.evidence = vec![fragments[i % fragments.len()].clone()];
            }
            nodes.push(n);
            let baseline = matches!(row.status, Accepted | Suspect | Superseded | Deprecated);
            edges.push(Edge {
                id: id(&format!("rel:own-{}", &row.id[5..])),
                revision: 1,
                status: if baseline { Accepted } else { Proposed },
                kind: RelationKind::HasAttribute,
                from: id(OWNER),
                to: id(row.id),
                properties: RelationProperties::None,
                evidence: Vec::new(),
                derivations: Vec::new(),
                standards: Vec::new(),
                audit: AuditMeta::new(id("actor:human"), ts(AT), None, None).unwrap(),
            });
        }
        Graph::new(id(PROJECT), id(PROFILE), nodes, edges).unwrap_or_else(|v| panic!("{v:?}"))
    }

    /// The golden rows: a dictionary hit, a non-dictionary text and a shaped Decimal.
    fn golden_rows() -> Vec<Row> {
        let mut amount = text("attr:c", "Amount", None);
        amount.attribute.value_type = "Decimal".into();
        amount.attribute.unit = Some("EUR".into());
        amount.attribute.precision = Some(2);
        amount.attribute.enum_values = Some(vec!["low".into(), "high".into()]);
        let mut notes = text("attr:b", "Internal Notes", None);
        notes.attribute.nullable = true;
        vec![text("attr:a", "Email Address", None), notes, amount]
    }

    fn map_nodes(graph: &Graph, f: impl Fn(&mut Node)) -> Graph {
        let nodes = graph
            .nodes()
            .values()
            .cloned()
            .map(|mut n| {
                f(&mut n);
                n
            })
            .collect();
        Graph::new(
            graph.project_id().clone(),
            graph.profile_id().clone(),
            nodes,
            graph.edges().values().cloned().collect(),
        )
        .unwrap_or_else(|v| panic!("{v:?}"))
    }

    fn edit_attribute(graph: &Graph, target: &'static str, f: fn(&mut Attribute)) -> Graph {
        map_nodes(graph, move |n| {
            if n.id.as_str() == target {
                if let NodePayload::Attribute(a) = &mut n.payload {
                    f(a);
                }
            }
        })
    }

    fn request(graph: &Graph) -> DataClassRequest {
        build_data_class_request(graph, provider("mock")).unwrap()
    }

    fn classifications(items: &[(&str, &str)]) -> Value {
        json!({"version": 1, "classifications": items
            .iter()
            .map(|(a, c)| json!({"attribute_ref": a, "classification": c}))
            .collect::<Vec<_>>()})
    }

    fn analyze_with(
        graph: &Graph,
        request: &DataClassRequest,
        artifact: Option<&InferenceArtifact>,
    ) -> Result<DataClassAnalysisResult, DataClassError> {
        analyze_data_classification(
            graph,
            request,
            artifact.map(|artifact| DataClassInference {
                artifact,
                derivation_ref: derivation(),
            }),
        )
    }

    fn analyze(
        graph: &Graph,
        output: Option<Value>,
    ) -> Result<DataClassAnalysisResult, DataClassError> {
        let r = request(graph);
        let artifact = output.map(|o| artifact_for(&r.request, o));
        analyze_with(graph, &r, artifact.as_ref())
    }

    fn replaced(p: &Proposal) -> (&Id, &Attribute) {
        let SemanticPatch::ReplacePayload { target, payload } = &p.patch_set.patch else {
            panic!("not a ReplacePayload: {:?}", p.patch_set.patch);
        };
        let NodePayload::Attribute(a) = payload else {
            panic!("not an Attribute payload");
        };
        (&target.id, a)
    }

    fn proposal_for<'r>(result: &'r DataClassAnalysisResult, attribute: &str) -> &'r Proposal {
        result
            .proposals
            .iter()
            .find(|p| replaced(p).0 == &id(attribute))
            .unwrap_or_else(|| panic!("no proposal for {attribute}"))
    }

    fn apply_one(graph: &Graph, p: &Proposal) -> Graph {
        let g = apply_patch(graph, &p.patch_set).unwrap().graph;
        g.validate().unwrap();
        g
    }

    fn current_class(graph: &Graph, attribute: &str) -> Option<String> {
        match &graph.node(&id(attribute)).unwrap().payload {
            NodePayload::Attribute(a) => a.data_classification.clone(),
            _ => unreachable!(),
        }
    }

    // ------------------------------------------------------------------ schema, prompt, dictionary

    #[test]
    fn s2_data_class_schema_contract() {
        let value: Value = serde_json::from_str(SCHEMA).unwrap();
        assert_eq!(
            value["$schema"],
            json!("https://json-schema.org/draft/2020-12/schema")
        );
        let schema = JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&value)
            .unwrap();
        let valid = |v: &Value| schema.is_valid(v);
        assert!(valid(&json!({"version": 1, "classifications": []})));
        assert!(!valid(
            &json!({"version": 1, "classifications": [], "extra": 1})
        ));
        assert!(!valid(&json!({"version": 2, "classifications": []})));
        assert!(!valid(&json!({"classifications": []})));
        assert!(!valid(&json!({"version": 1})));
        for class in ["pii", "financial", "confidential"] {
            assert!(valid(&classifications(&[("attr:b", class)])), "{class}");
        }
        for class in [
            "none",
            "unknown",
            "unclassified",
            "public",
            "internal",
            "restricted",
            "PII",
            "Pii",
            "pii_financial",
            "email",
            "salary",
            "sensitive",
            "",
        ] {
            assert!(!valid(&classifications(&[("attr:b", class)])), "{class}");
        }
        assert!(!valid(
            &json!({"version": 1, "classifications": [{"attribute_ref": "attr:b", "classification": null}]})
        ));
        assert!(!valid(
            &json!({"version": 1, "classifications": [{"attribute_ref": "attr:b"}]})
        ));
        assert!(!valid(
            &json!({"version": 1, "classifications": [{"classification": "pii"}]})
        ));
        for extra in ["confidence", "rationale", "extra"] {
            let mut v = classifications(&[("attr:b", "pii")]);
            v["classifications"][0][extra] = json!("x");
            assert!(!valid(&v), "{extra}");
        }
        fn walk(v: &Value) {
            match v {
                Value::Object(map) => {
                    assert!(map.get("$ref").is_none());
                    if map.get("type") == Some(&json!("object")) {
                        assert_eq!(map.get("additionalProperties"), Some(&json!(false)));
                    }
                    map.values().for_each(walk);
                }
                Value::Array(items) => items.iter().for_each(walk),
                _ => {}
            }
        }
        walk(&value);
    }

    #[test]
    fn s2_data_class_prompt_and_schema_hashes() {
        assert_eq!(Hash::content_sha256(PROMPT).as_str(), PROMPT_HASH);
        assert_eq!(
            Hash::content_sha256(SCHEMA.as_bytes()).as_str(),
            SCHEMA_HASH
        );
        let prompt = std::str::from_utf8(PROMPT).unwrap();
        for required in [
            "Classify only supplied Attribute IDs",
            "- pii:",
            "- financial:",
            "- confidential:",
            "Do not invent another class",
            "Do not return \"none\"",
            "omission means unresolved",
            "dictionary_classification is already set",
            "Do not infer or alter the Attribute name",
            "Return only JSON",
        ] {
            assert!(prompt.contains(required), "{required}");
        }
        for forbidden in [
            "TODO",
            "FIXME",
            "{{",
            "G_DATA",
            "public",
            "internal,",
            "restricted",
        ] {
            assert!(!prompt.contains(forbidden), "{forbidden}");
            assert!(!SCHEMA.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn s2_data_class_dictionary_invariants() {
        assert_eq!(DATA_CLASS_DICTIONARY_VERSION, 1);
        assert_eq!(DATA_CLASS_CONTEXT_VERSION, 1);
        assert_eq!(DATA_CLASS_OUTPUT_VERSION, 1);
        assert_eq!(DATA_CLASS_TASK_KIND, "data_classification");
        let expected: Vec<(&str, PilotDataClass)> = vec![
            ("e mail", PilotDataClass::Pii),
            ("e mail address", PilotDataClass::Pii),
            ("email", PilotDataClass::Pii),
            ("email address", PilotDataClass::Pii),
            ("birth date", PilotDataClass::Pii),
            ("date of birth", PilotDataClass::Pii),
            ("dob", PilotDataClass::Pii),
            ("salary", PilotDataClass::Financial),
            ("iban", PilotDataClass::Financial),
            (
                "international bank account number",
                PilotDataClass::Financial,
            ),
        ];
        assert_eq!(DATA_CLASS_DICTIONARY.to_vec(), expected);
        let mut seen = BTreeSet::new();
        for (alias, class) in DATA_CLASS_DICTIONARY {
            assert_eq!(
                normalize_vocabulary_term(alias).as_deref(),
                Some(*alias),
                "canonical {alias}"
            );
            assert!(seen.insert(*alias), "unique {alias}");
            assert!(PilotDataClass::ALL.contains(class));
            assert_ne!(*class, PilotDataClass::Confidential);
        }
        for class in PilotDataClass::ALL {
            assert_eq!(PilotDataClass::parse(class.as_str()), Some(class));
            assert_eq!(serde_json::to_value(class).unwrap(), json!(class.as_str()));
        }
        assert_eq!(
            PilotDataClass::ALL.map(PilotDataClass::as_str),
            ["pii", "financial", "confidential"]
        );
        for bad in [
            "none",
            "PII",
            "Financial",
            " pii",
            "pii ",
            "",
            "pii_financial",
            "personal",
        ] {
            assert_eq!(PilotDataClass::parse(bad), None, "{bad}");
            assert!(serde_json::from_value::<PilotDataClass>(json!(bad)).is_err());
        }
    }

    #[test]
    fn s2_data_class_dictionary_matching() {
        for (name, class) in [
            ("Email", PilotDataClass::Pii),
            ("EMAIL", PilotDataClass::Pii),
            ("Email Address", PilotDataClass::Pii),
            ("Email Addresses", PilotDataClass::Pii),
            ("e-mail", PilotDataClass::Pii),
            ("e-mail address", PilotDataClass::Pii),
            ("DOB", PilotDataClass::Pii),
            ("Date of Birth", PilotDataClass::Pii),
            ("Birth Date", PilotDataClass::Pii),
            ("Salary", PilotDataClass::Financial),
            ("IBAN", PilotDataClass::Financial),
            (
                "International Bank Account Number",
                PilotDataClass::Financial,
            ),
        ] {
            assert_eq!(
                dictionary_classification(name).map(|(_, c)| c),
                Some(class),
                "{name}"
            );
        }
        // Exact whole-name matching only: no substring, prefix, suffix or synonym.
        for name in [
            "Customer Email Preference",
            "Bank Account Number",
            "Account Number",
            "Wage",
            "Status",
            "Internal Notes",
            "Emails Sent",
            "Primary Email",
            "Salary Band",
            "Compensation",
            "Birthday",
        ] {
            assert_eq!(dictionary_classification(name), None, "{name}");
        }
        assert_eq!(
            dictionary_classification("Email Addresses").unwrap().0,
            "email address"
        );
    }

    // ------------------------------------------------------------------ request

    #[test]
    fn s2_data_class_request_golden_and_context() {
        // Non-Accepted Attributes are never context.
        let mut rows = golden_rows();
        for (i, status) in [Proposed, Suspect, Rejected, Superseded, Deprecated]
            .into_iter()
            .enumerate()
        {
            let ids = ["attr:x1", "attr:x2", "attr:x3", "attr:x4", "attr:x5"];
            rows.push(with_status(text(ids[i], "Email", None), status));
        }
        let g = graph_of(&rows, false);
        let r = request(&g);
        assert_eq!(r.request.id.as_str(), GOLDEN_REQUEST_ID);
        assert_eq!(r.request.context_hash.as_str(), GOLDEN_CONTEXT_HASH);
        assert_eq!(
            r.context.content_hash().unwrap().as_str(),
            GOLDEN_CONTEXT_HASH
        );
        assert_eq!(r.request.stage, StageId::S2);
        assert_eq!(r.request.task_kind, "data_classification");
        assert_eq!(r.request.prompt_template_hash.as_str(), PROMPT_HASH);
        assert_eq!(r.request.schema_hash.as_str(), SCHEMA_HASH);
        assert_eq!(r.request.input_refs, ids(&["attr:a", "attr:b", "attr:c"]));
        assert!(r.request.evidence_refs.is_empty());
        assert_eq!(r.context.dictionary_version, 1);
        assert_eq!(
            serde_json::to_value(&r.context.attributes[2]).unwrap(),
            json!({"attribute_ref": "attr:c", "name": "Amount", "value_type": "Decimal",
                   "nullable": false, "unit": "EUR", "precision": 2, "enum_values": ["low", "high"],
                   "dictionary_classification": null})
        );
        assert_eq!(
            r.context.attributes[0].dictionary_classification,
            Some(PilotDataClass::Pii)
        );
        let wire = serde_json::to_value(&r.context).unwrap();
        assert!(!wire.to_string().contains("data_classification\""));
        // dictionary_version is part of the canonical hash.
        let mut other = r.context.clone();
        other.dictionary_version = 2;
        assert_ne!(
            Hash::content_sha256(&to_canonical_json(&other).unwrap()),
            r.context.content_hash().unwrap()
        );
        assert!(other.validate().is_err());
        // The current classification is not context.
        let classified = edit_attribute(&g, "attr:b", |a| {
            a.data_classification = Some("confidential".into())
        });
        assert_eq!(request(&classified), r);
    }

    #[test]
    fn s2_data_class_request_evidence_and_provider_policy() {
        let g = graph_of(&golden_rows(), true);
        let r = request(&g);
        let expected: BTreeSet<Id> = ["attr:a", "attr:b", "attr:c"]
            .iter()
            .flat_map(|a| {
                g.node(&id(a))
                    .unwrap()
                    .evidence
                    .iter()
                    .map(|e| e.as_id().clone())
            })
            .collect();
        assert_eq!(expected.len(), 2);
        assert_eq!(
            r.request.evidence_refs,
            expected.into_iter().collect::<Vec<_>>()
        );
        let other = build_data_class_request(&g, provider("other")).unwrap();
        assert_ne!(other.request.id, r.request.id);
        assert_eq!(other.context, r.context);
        let a = analyze_with(&g, &r, None).unwrap();
        let b = analyze_with(&g, &other, None).unwrap();
        assert_eq!(a.dictionary_hits, b.dictionary_hits);
        assert_eq!(a.proposals, b.proposals);
    }

    #[test]
    fn s2_data_class_request_staleness() {
        let g = graph_of(&golden_rows(), true);
        let r = request(&g);
        let stale = |changed: Graph| {
            assert!(matches!(
                analyze_with(&changed, &r, None),
                Err(DataClassError::InvalidInput { .. })
            ));
        };
        stale(edit_attribute(&g, "attr:c", |a| a.name = "Total".into()));
        stale(edit_attribute(&g, "attr:c", |a| {
            a.value_type = "Integer".into()
        }));
        stale(edit_attribute(&g, "attr:c", |a| a.nullable = true));
        stale(edit_attribute(&g, "attr:c", |a| {
            a.unit = Some("USD".into())
        }));
        stale(edit_attribute(&g, "attr:c", |a| a.precision = Some(4)));
        stale(edit_attribute(&g, "attr:c", |a| a.enum_values = None));
        // A changed name changes the dictionary classification too.
        stale(edit_attribute(&g, "attr:b", |a| a.name = "Salary".into()));
        stale(map_nodes(&g, |n| {
            if n.id.as_str() == "attr:c" {
                n.status = Suspect;
            }
        }));
        let mut rows = golden_rows();
        rows.push(text("attr:d", "Nickname", None));
        stale(graph_of(&rows, true));
        stale(graph_of(&golden_rows()[..2], true));
        stale(
            Graph::new(
                id("project:other"),
                g.profile_id().clone(),
                g.nodes().values().cloned().collect(),
                g.edges().values().cloned().collect(),
            )
            .unwrap(),
        );
        // Only the classification changed: the request stays valid.
        let classified =
            edit_attribute(&g, "attr:b", |a| a.data_classification = Some("pii".into()));
        analyze_with(&classified, &r, None).unwrap();
    }

    // ------------------------------------------------------------------ dictionary proposals

    #[test]
    fn s2_data_class_dictionary_proposal_structure() {
        let g = graph_of(&golden_rows(), true);
        let result = analyze(&g, None).unwrap();
        assert_eq!(
            result.dictionary_hits,
            vec![DataClassDictionaryHit {
                attribute_ref: id("attr:a"),
                normalized_name: "email address".into(),
                classification: PilotDataClass::Pii,
            }]
        );
        assert_eq!(result.proposals.len(), 1);
        let p = &result.proposals[0];
        let SemanticPatch::ReplacePayload { target, payload } = &p.patch_set.patch else {
            panic!()
        };
        // Golden behavior: target, classification string and policy.
        assert_eq!(target.id, id("attr:a"));
        assert_eq!(
            target.expected_hash,
            node_element_hash(g.node(&id("attr:a")).unwrap()).unwrap()
        );
        let NodePayload::Attribute(a) = payload else {
            panic!()
        };
        let NodePayload::Attribute(before) = &g.node(&id("attr:a")).unwrap().payload else {
            panic!()
        };
        let mut expected = before.clone();
        expected.data_classification = Some("pii".into());
        assert_eq!(a, &expected);
        assert_eq!(p.stage, StageId::S2);
        assert_eq!(p.materiality, ProposalMateriality::Semantic);
        assert_eq!(p.acceptance_policy, AcceptancePolicy::AutoDerivation);
        assert_eq!(p.confidence, None);
        assert!(p.derivation_refs.is_empty());
        assert_eq!(p.evidence_refs, g.node(&id("attr:a")).unwrap().evidence);
        assert_eq!(p.patch_set.base_semantic_hash, g.semantic_hash().unwrap());
        p.validate().unwrap();
        // Applied: only the payload field changed; identity and metadata are untouched.
        let applied = apply_one(&g, p);
        let (old, new) = (
            g.node(&id("attr:a")).unwrap(),
            applied.node(&id("attr:a")).unwrap(),
        );
        assert_eq!(current_class(&applied, "attr:a").as_deref(), Some("pii"));
        assert_eq!(
            (
                &new.audit,
                &new.evidence,
                &new.derivations,
                &new.standards,
                &new.tags,
                &new.extensions,
                new.status
            ),
            (
                &old.audit,
                &old.evidence,
                &old.derivations,
                &old.standards,
                &old.tags,
                &old.extensions,
                old.status
            )
        );
        // Non-dictionary Attributes await inference.
        assert_eq!(result.unresolved_refs, ids(&["attr:b", "attr:c"]));
        assert_eq!(
            result.issues,
            vec![DataClassIssue::InferenceUnavailable {
                attribute_refs: ids(&["attr:b", "attr:c"])
            }]
        );
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn s2_data_class_proposed_attribute_excluded() {
        for status in [Proposed, Suspect, Rejected, Superseded, Deprecated] {
            let g = graph_of(
                &[with_status(text("attr:p", "Email Address", None), status)],
                false,
            );
            let result = analyze(&g, None).unwrap();
            assert!(result.dictionary_hits.is_empty(), "{status:?}");
            assert!(result.proposals.is_empty());
            assert!(result.unresolved_refs.is_empty());
            assert!(result.issues.is_empty());
        }
    }

    #[test]
    fn s2_data_class_existing_classifications() {
        // Same dictionary value: existing equivalent, no proposal.
        let g = graph_of(&[text("attr:a", "Email", Some("pii"))], false);
        let result = analyze(&g, None).unwrap();
        assert_eq!(result.dictionary_hits.len(), 1);
        assert!(
            result.proposals.is_empty() && result.conflicts.is_empty() && result.issues.is_empty()
        );
        // Dictionary disagreement: conflict, no overwrite.
        let g = graph_of(&[text("attr:a", "Email", Some("financial"))], false);
        let result = analyze(&g, None).unwrap();
        assert!(result.proposals.is_empty());
        assert_eq!(
            result.conflicts,
            vec![DataClassConflict::ExistingClassificationMismatch {
                attribute_ref: id("attr:a"),
                existing: PilotDataClass::Financial,
                proposed: PilotDataClass::Pii,
                source: DataClassSource::Dictionary,
            }]
        );
        // Unsupported stored values, including "none", are surfaced and block both paths.
        for value in [
            "none",
            "PII",
            "Financial",
            "personal",
            "sensitive",
            "pii_financial",
            " pii ",
            "",
        ] {
            let g = graph_of(
                &[
                    text("attr:a", "Email", Some(value)),
                    text("attr:b", "Notes", Some(value)),
                ],
                false,
            );
            let result = analyze(&g, Some(classifications(&[("attr:b", "confidential")]))).unwrap();
            assert!(result.proposals.is_empty(), "{value:?}");
            assert_eq!(
                result.conflicts,
                vec![
                    DataClassConflict::UnsupportedExistingClassification {
                        attribute_ref: id("attr:a"),
                        existing: value.into()
                    },
                    DataClassConflict::UnsupportedExistingClassification {
                        attribute_ref: id("attr:b"),
                        existing: value.into()
                    },
                ]
            );
            assert!(result.unresolved_refs.is_empty());
            // Nothing is repaired.
            assert_eq!(current_class(&g, "attr:a").as_deref(), Some(value));
        }
        // Replayed inference: agreement is equivalent, disagreement a conflict.
        let g = graph_of(&[text("attr:b", "Notes", Some("confidential"))], false);
        let agree = analyze(&g, Some(classifications(&[("attr:b", "confidential")]))).unwrap();
        assert!(
            agree.proposals.is_empty()
                && agree.conflicts.is_empty()
                && agree.unresolved_refs.is_empty()
        );
        let disagree = analyze(&g, Some(classifications(&[("attr:b", "pii")]))).unwrap();
        assert!(disagree.proposals.is_empty());
        assert_eq!(
            disagree.conflicts,
            vec![DataClassConflict::ExistingClassificationMismatch {
                attribute_ref: id("attr:b"),
                existing: PilotDataClass::Confidential,
                proposed: PilotDataClass::Pii,
                source: DataClassSource::Inference,
            }]
        );
    }

    // ------------------------------------------------------------------ inference

    #[test]
    fn s2_data_class_inference_proposals() {
        let rows = vec![
            text("attr:b", "Internal Notes", None),
            text("attr:n", "Nickname", None),
            text("attr:w", "Wage", None),
        ];
        let g = graph_of(&rows, true);
        let result = analyze(
            &g,
            Some(classifications(&[
                ("attr:b", "confidential"),
                ("attr:n", "pii"),
                ("attr:w", "financial"),
            ])),
        )
        .unwrap();
        assert!(result.dictionary_hits.is_empty());
        assert!(result.unresolved_refs.is_empty() && result.issues.is_empty());
        assert_eq!(result.proposals.len(), 3);
        for (attribute, class) in [
            ("attr:b", "confidential"),
            ("attr:n", "pii"),
            ("attr:w", "financial"),
        ] {
            let p = proposal_for(&result, attribute);
            let (_, a) = replaced(p);
            let NodePayload::Attribute(before) = &g.node(&id(attribute)).unwrap().payload else {
                panic!()
            };
            let mut expected = before.clone();
            expected.data_classification = Some(class.into());
            assert_eq!(a, &expected);
            assert_eq!(p.acceptance_policy, AcceptancePolicy::HumanConfirm);
            assert_eq!(p.materiality, ProposalMateriality::Semantic);
            assert_eq!(p.stage, StageId::S2);
            assert_eq!(p.confidence, None);
            assert_eq!(p.derivation_refs, vec![derivation()]);
            assert_eq!(p.evidence_refs, g.node(&id(attribute)).unwrap().evidence);
            apply_one(&g, p);
        }
        // Inference-origin proposals never use AUTO_DERIVATION, and nothing stores "none".
        for p in &result.proposals {
            assert_ne!(p.acceptance_policy, AcceptancePolicy::AutoDerivation);
            let (_, a) = replaced(p);
            assert!(PilotDataClass::parse(a.data_classification.as_deref().unwrap()).is_some());
        }
    }

    #[test]
    fn s2_data_class_inference_validation() {
        let g = graph_of(&golden_rows(), false);
        let r = request(&g);
        let good = artifact_for(&r.request, classifications(&[("attr:b", "confidential")]));
        analyze_with(&g, &r, Some(&good)).unwrap();
        let mut wrong_request = good.clone();
        wrong_request.request_hash = Hash::content_sha256(b"other request");
        let mut wrong_provider = good.clone();
        wrong_provider.provider = "other".into();
        let mut wrong_hash = good.clone();
        wrong_hash.validated_output_hash = Hash::content_sha256(b"other output");
        for artifact in [wrong_request, wrong_provider, wrong_hash] {
            assert!(matches!(
                analyze_with(&g, &r, Some(&artifact)),
                Err(DataClassError::InvalidInferenceArtifact { .. })
            ));
        }
        let run = |v: Value| analyze_with(&g, &r, Some(&artifact_for(&r.request, v)));
        for class in [
            "none",
            "unknown",
            "public",
            "restricted",
            "PII",
            "pii_financial",
            "email",
            "salary",
        ] {
            assert!(matches!(
                run(classifications(&[("attr:b", class)])),
                Err(DataClassError::SchemaInvalid { .. })
            ));
        }
        assert!(matches!(
            run(classifications(&[("attr:zz", "pii")])),
            Err(DataClassError::UnknownAttributeRef { attribute_ref }) if attribute_ref == id("attr:zz")
        ));
        // The dictionary owns attr:a (Email Address -> pii).
        assert!(matches!(
            run(classifications(&[("attr:a", "confidential")])),
            Err(DataClassError::DictionaryOwnedAttribute { attribute_ref }) if attribute_ref == id("attr:a")
        ));
        assert!(matches!(
            run(classifications(&[("attr:a", "pii")])),
            Err(DataClassError::DictionaryOwnedAttribute { .. })
        ));
        assert!(matches!(
            run(classifications(&[("attr:b", "pii"), ("attr:b", "pii")])),
            Err(DataClassError::DuplicateCandidate { attribute_ref }) if attribute_ref == id("attr:b")
        ));
        assert!(matches!(
            run(classifications(&[
                ("attr:b", "pii"),
                ("attr:b", "financial")
            ])),
            Err(DataClassError::DuplicateCandidate { .. })
        ));
    }

    #[test]
    fn s2_data_class_inference_absence_and_omission() {
        // Targets without inference: issue, no error, no fallback.
        let g = graph_of(&golden_rows(), false);
        let absent = analyze(&g, None).unwrap();
        assert_eq!(absent.unresolved_refs, ids(&["attr:b", "attr:c"]));
        assert_eq!(absent.proposals.len(), 1, "the dictionary still runs");
        // No inference-eligible Attribute: no issue.
        let g2 = graph_of(
            &[
                text("attr:a", "Email", None),
                text("attr:s", "Salary", None),
                text("attr:n", "Notes", Some("confidential")),
            ],
            false,
        );
        let none = analyze(&g2, None).unwrap();
        assert!(none.issues.is_empty() && none.unresolved_refs.is_empty());
        assert_eq!(none.proposals.len(), 2);
        // Omission leaves the Attribute unresolved, with no issue and no finding.
        let omitted = analyze(&g, Some(classifications(&[("attr:b", "confidential")]))).unwrap();
        assert_eq!(omitted.unresolved_refs, ids(&["attr:c"]));
        assert!(omitted.issues.is_empty());
        assert_eq!(omitted.proposals.len(), 2);
        let empty = analyze(&g, Some(classifications(&[]))).unwrap();
        assert_eq!(empty.unresolved_refs, ids(&["attr:b", "attr:c"]));
        assert!(empty.issues.is_empty());
    }

    // ------------------------------------------------------------------ replay and CAS

    #[test]
    fn s2_data_class_replay_and_cas() {
        let rows = vec![
            text("attr:a", "Email", None),
            text("attr:b", "Internal Notes", None),
            text("attr:i", "IBAN", None),
            text("attr:n", "Nickname", None),
        ];
        let a = graph_of(&rows, true);
        let r = request(&a);
        let artifact = artifact_for(
            &r.request,
            classifications(&[("attr:b", "confidential"), ("attr:n", "pii")]),
        );
        let first = analyze_with(&a, &r, Some(&artifact)).unwrap();
        assert_eq!(first.proposals.len(), 4);
        // Apply ONE proposal in test code.
        let applied = proposal_for(&first, "attr:a").clone();
        let b = apply_one(&a, &applied);
        // An old sibling proposal is stale against Graph B; CAS is not weakened.
        let sibling = proposal_for(&first, "attr:i");
        assert!(matches!(
            apply_patch(&b, &sibling.patch_set),
            Err(PatchError::StaleBase { .. })
        ));
        // The same request and artifact still validate and re-evaluate against Graph B.
        r.validate().unwrap();
        assert_eq!(request(&b), r);
        let second = analyze_with(&b, &r, Some(&artifact)).unwrap();
        assert_eq!(second.proposals.len(), 3);
        assert!(second
            .proposals
            .iter()
            .all(|p| replaced(p).0 != &id("attr:a")));
        for p in &second.proposals {
            assert_eq!(p.patch_set.base_semantic_hash, b.semantic_hash().unwrap());
        }
        let regenerated = proposal_for(&second, "attr:i");
        let c = apply_one(&b, regenerated);
        // The inference-origin proposal also leaves the request valid; replay is idempotent.
        let d = apply_one(
            &c,
            proposal_for(&analyze_with(&c, &r, Some(&artifact)).unwrap(), "attr:b"),
        );
        assert_eq!(request(&d), r);
        let e = apply_one(
            &d,
            proposal_for(&analyze_with(&d, &r, Some(&artifact)).unwrap(), "attr:n"),
        );
        let last = analyze_with(&e, &r, Some(&artifact)).unwrap();
        assert!(
            last.proposals.is_empty()
                && last.conflicts.is_empty()
                && last.unresolved_refs.is_empty()
        );
        assert_eq!(last.dictionary_hits.len(), 2);
        for (attribute, class) in [
            ("attr:a", "pii"),
            ("attr:b", "confidential"),
            ("attr:i", "financial"),
            ("attr:n", "pii"),
        ] {
            assert_eq!(current_class(&e, attribute).as_deref(), Some(class));
        }
    }

    #[test]
    fn s2_data_class_determinism() {
        let rows = vec![
            text("attr:a", "Email", None),
            text("attr:b", "Internal Notes", None),
            text("attr:c", "Email", Some("financial")),
            text("attr:d", "Notes", Some("none")),
            text("attr:n", "Nickname", None),
        ];
        let g = graph_of(&rows, false);
        let reversed = Graph::new(
            g.project_id().clone(),
            g.profile_id().clone(),
            g.nodes().values().rev().cloned().collect(),
            g.edges().values().rev().cloned().collect(),
        )
        .unwrap();
        let x = analyze(
            &g,
            Some(classifications(&[
                ("attr:b", "confidential"),
                ("attr:n", "pii"),
            ])),
        )
        .unwrap();
        let y = analyze(
            &reversed,
            Some(classifications(&[
                ("attr:n", "pii"),
                ("attr:b", "confidential"),
            ])),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_vec(&x).unwrap(),
            serde_json::to_vec(&y).unwrap()
        );
        let order: Vec<&Id> = x.proposals.iter().map(|p| &p.id).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted);
        assert_eq!(
            x.conflicts
                .iter()
                .map(|c| serde_json::to_value(c).unwrap()["attribute_ref"].clone())
                .collect::<Vec<_>>(),
            vec![json!("attr:c"), json!("attr:d")]
        );
    }

    // ------------------------------------------------------------------ F0.14 compatibility

    #[test]
    fn s2_data_class_projection_compatibility() {
        let rows = vec![
            text("attr:a", "Email", Some("pii")),
            text("attr:i", "IBAN", Some("financial")),
            text("attr:b", "Internal Notes", Some("confidential")),
            text("attr:n", "Nickname", None),
        ];
        let g = graph_of(&rows, false);
        let projection =
            project_functional_v2(&g, &load_builtin_software_profile().unwrap()).unwrap();
        let document = serde_json::to_value(&projection.document).unwrap();
        let mut classes = BTreeMap::new();
        fn walk(v: &Value, out: &mut BTreeMap<String, String>) {
            match v {
                Value::Object(map) => {
                    if let (Some(Value::String(id)), Some(Value::String(class))) =
                        (map.get("id"), map.get("class"))
                    {
                        out.insert(id.clone(), class.clone());
                    }
                    map.values().for_each(|x| walk(x, out));
                }
                Value::Array(items) => items.iter().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        walk(&document, &mut classes);
        assert_eq!(
            classes,
            BTreeMap::from([
                ("attr:a".to_owned(), "pii".to_owned()),
                ("attr:b".to_owned(), "confidential".to_owned()),
                ("attr:i".to_owned(), "financial".to_owned()),
                // v3 None is projected as the legacy "none"; S2.3 never stores it.
                ("attr:n".to_owned(), "none".to_owned()),
            ])
        );
        assert_eq!(current_class(&g, "attr:n"), None);
    }

    // ------------------------------------------------------------------ real HR

    fn hr_graph() -> Graph {
        let imported = import_markdown(
            "requirements.md",
            HR_MD,
            &ImportAudit {
                created_by: id("actor:importer"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        let fragments = imported.fragments.clone();
        let mut nodes = vec![imported.source];
        nodes.extend(imported.fragments);
        let mut g = Graph::new(id(PROJECT), id(PROFILE), nodes, vec![]).unwrap();
        let segmentation = build_segmentation_request(&fragments, provider("mock")).unwrap();
        let candidates = evaluate_segmentation(
            &segmentation,
            &fragments,
            None,
            &SegmentationAudit {
                created_by: id("agent:segmenter"),
                created_at: ts(AT),
            },
        )
        .unwrap()
        .candidates;
        let classification =
            build_requirement_classification_request(&g, &candidates, provider("mock")).unwrap();
        let entries: Vec<Value> = classification
            .context
            .candidates
            .iter()
            .map(|c| json!({"candidate_ref": c.candidate_ref, "requirement_kind": null, "level": "software"}))
            .collect();
        let artifact = artifact_for(
            &classification.request,
            json!({"version": 1, "classifications": entries, "intents": []}),
        );
        let compiled = compile_requirement_candidates(
            &g,
            &candidates,
            &classification,
            Some(RequirementClassificationInference {
                artifact: &artifact,
                derivation_ref: DerivationRef::from(id("drv:00000000000000c1")),
            }),
            &RequirementCompilationAudit {
                created_by: id("agent:requirements"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        for p in &compiled.proposals {
            g = apply_patch(&g, &p.patch_set).unwrap().graph;
        }
        let g = map_nodes(&g, |n| {
            if matches!(n.payload, NodePayload::Requirement(_)) {
                n.status = Accepted;
            }
        });
        let domain = build_domain_request(&g, provider("mock")).unwrap();
        let domain_artifact = artifact_for(
            &domain.request,
            json!({"version": 1, "entities": [], "attributes": [], "relationships": []}),
        );
        let entities = analyze_domain(
            &g,
            &domain,
            Some(DomainInference {
                artifact: &domain_artifact,
                derivation_ref: DerivationRef::from(id("drv:00000000000000c3")),
            }),
            &DomainAudit {
                created_by: id("agent:domain"),
                created_at: ts(AT),
            },
        )
        .unwrap();
        assert!(entities.proposals.is_empty());
        g
    }

    #[test]
    fn s2_data_class_real_hr_diagnostic() {
        let g = hr_graph();
        let attributes = g
            .nodes()
            .values()
            .filter(|n| n.status == Accepted && matches!(n.payload, NodePayload::Attribute(_)))
            .count();
        // Stop and inspect rather than force this if upstream ever produces Attributes.
        assert_eq!(attributes, 0);
        let r = request(&g);
        assert!(r.context.attributes.is_empty());
        let result = analyze_with(&g, &r, None).unwrap();
        assert!(result.dictionary_hits.is_empty());
        assert!(result.proposals.is_empty());
        assert!(result.unresolved_refs.is_empty());
        assert!(result.conflicts.is_empty());
        assert!(
            result.issues.is_empty(),
            "no InferenceUnavailable without targets"
        );
        // No classification accuracy is computed or claimed.
    }

    // ------------------------------------------------------------------ guards

    #[test]
    fn s2_data_class_production_source_guard() {
        let source = include_str!("../src/data_class.rs");
        for forbidden in [
            "std::fs",
            "File::open",
            "reqwest",
            ".execute(",
            "MockProvider",
            "ArtifactStore",
            "RevisionStore",
            "Sqlite",
            "rusqlite",
            "SystemClock",
            "Clock::now",
            "Utc::now",
            "Instant::now",
            "Timestamp",
            "commit(",
            "Commit",
            "unsafe",
            "GeneratedFinding",
            "load_builtin_software_profile",
            "RuleMetadata",
            "NodePayload::Question",
            "ResolutionDecision",
            "\"none\".to",
            "Some(\"none\")",
            ".contains(",
            "starts_with(",
            "ends_with(",
            "Regex",
            "levenshtein",
            "to_lowercase",
            "to_ascii_lowercase",
            "RemoveNode",
            "AddNode",
            "SetStatus",
            "Supersede",
            "MergeNodes",
            "\"wage\"",
            "\"bank account",
            "\"account number\"",
            "\"internal notes\"",
            "G_DATA",
        ] {
            assert!(
                !source.contains(forbidden),
                "data_class.rs contains {forbidden}"
            );
        }
        assert!(source.contains("ReplacePayload"));
        assert!(source.contains("apply_patch"));
        let lib = include_str!("../src/lib.rs");
        assert!(!lib.contains("Proposal"));
        assert!(lib.contains("pub mod data_class;"));
    }
}
