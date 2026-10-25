//! F0.3 envelope-primitive contract tests, and the Fable #3 pin of the serialized PSG form.
//!
//! Every test lives in `serde_contract` so that the task command
//! `cargo test -p plumb-psg serde` selects all of them.

#[path = "support/payload_fixtures.rs"]
mod payload_fixtures;

mod serde_contract {
    use std::collections::BTreeMap;

    use plumb_core::{Id, Timestamp};
    use plumb_psg::{
        AuditMeta, AuditMetaError, DerivationRef, ElementStatus, EvidenceRef, ExtensionKey,
        InvalidExtensionKey, MappingRole, MappingStrength, StandardMapping,
    };
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use serde_json::{json, Value};

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    /// Asserts `value` serializes to exactly `expected` JSON and deserializes back to itself.
    fn assert_json_round_trip<T>(value: &T, expected: Value)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
        let text = serde_json::to_string(value).unwrap();
        assert_eq!(&serde_json::from_str::<T>(&text).unwrap(), value);
    }

    fn rejects<T: DeserializeOwned + std::fmt::Debug>(json_value: Value) {
        assert!(
            serde_json::from_value::<T>(json_value.clone()).is_err(),
            "{json_value} should be rejected"
        );
    }

    // ---------------------------------------------------------------- ElementStatus

    #[test]
    fn element_status_serializes_to_exactly_six_values() {
        for (status, text) in [
            (ElementStatus::Proposed, "Proposed"),
            (ElementStatus::Accepted, "Accepted"),
            (ElementStatus::Rejected, "Rejected"),
            (ElementStatus::Superseded, "Superseded"),
            (ElementStatus::Deprecated, "Deprecated"),
            (ElementStatus::Suspect, "Suspect"),
        ] {
            assert_json_round_trip(&status, json!(text));
        }
    }

    #[test]
    fn element_status_rejects_unknown_values() {
        for bad in ["Confirmed", "proposed", "ACCEPTED", "Draft", ""] {
            rejects::<ElementStatus>(json!(bad));
        }
        rejects::<ElementStatus>(json!(1));
    }

    // ---------------------------------------------------------------- MappingRole

    #[test]
    fn mapping_role_serializes_to_exactly_five_values() {
        for (role, text) in [
            (MappingRole::SemanticAlignment, "semantic_alignment"),
            (MappingRole::Taxonomy, "taxonomy"),
            (MappingRole::Interchange, "interchange"),
            (MappingRole::ValidationReference, "validation_reference"),
            (
                MappingRole::PresentationConvention,
                "presentation_convention",
            ),
        ] {
            assert_json_round_trip(&role, json!(text));
        }
    }

    #[test]
    fn mapping_role_rejects_unknown_values() {
        for bad in [
            "lifecycle_alignment",
            "information_item_alignment",
            "test_process_alignment",
            "SemanticAlignment",
            "semantic-alignment",
            "compatible",
            "",
        ] {
            rejects::<MappingRole>(json!(bad));
        }
    }

    // ---------------------------------------------------------------- MappingStrength

    #[test]
    fn mapping_strength_serializes_to_exactly_five_values() {
        for (strength, text) in [
            (MappingStrength::Exact, "exact"),
            (MappingStrength::Compatible, "compatible"),
            (MappingStrength::Subset, "subset"),
            (MappingStrength::Extension, "extension"),
            (MappingStrength::InspiredBy, "inspired_by"),
        ] {
            assert_json_round_trip(&strength, json!(text));
        }
    }

    #[test]
    fn mapping_strength_rejects_unknown_values_including_taxonomy() {
        rejects::<MappingStrength>(json!("taxonomy"));
        for bad in ["Exact", "inspired-by", "inspiredBy", "full", ""] {
            rejects::<MappingStrength>(json!(bad));
        }
    }

    // ---------------------------------------------------------------- StandardMapping

    fn iso25010_mapping() -> StandardMapping {
        StandardMapping {
            standard_id: "ISO/IEC 25010".into(),
            version: "2023".into(),
            concept: "Performance efficiency".into(),
            clause_ref: Some("4.2.2".into()),
            mapping_role: MappingRole::Taxonomy,
            mapping_strength: MappingStrength::Compatible,
            validator_rules: vec!["ISO25010.Q1.CHARACTERISTIC".into()],
        }
    }

    #[test]
    fn standard_mapping_round_trips_with_real_role_and_strength() {
        assert_json_round_trip(
            &iso25010_mapping(),
            json!({
                "standard_id": "ISO/IEC 25010",
                "version": "2023",
                "concept": "Performance efficiency",
                "clause_ref": "4.2.2",
                "mapping_role": "taxonomy",
                "mapping_strength": "compatible",
                "validator_rules": ["ISO25010.Q1.CHARACTERISTIC"]
            }),
        );
        let no_clause = StandardMapping {
            clause_ref: None,
            mapping_role: MappingRole::SemanticAlignment,
            mapping_strength: MappingStrength::InspiredBy,
            validator_rules: vec![],
            ..iso25010_mapping()
        };
        let text = serde_json::to_string(&no_clause).unwrap();
        assert_eq!(
            serde_json::from_str::<StandardMapping>(&text).unwrap(),
            no_clause
        );
    }

    #[test]
    fn standard_mapping_rejects_invalid_enums_and_unknown_fields() {
        let valid = serde_json::to_value(iso25010_mapping()).unwrap();
        for (field, bad) in [
            ("mapping_role", json!("lifecycle_alignment")),
            ("mapping_strength", json!("taxonomy")),
            ("mapping_role", json!("compatible")),
            ("mapping_strength", json!(null)),
        ] {
            let mut v = valid.clone();
            v[field] = bad;
            rejects::<StandardMapping>(v);
        }
        let mut extra = valid.clone();
        extra["confidence"] = json!(0.9);
        rejects::<StandardMapping>(extra);
        let mut missing = valid;
        missing.as_object_mut().unwrap().remove("concept");
        rejects::<StandardMapping>(missing);
    }

    // ---------------------------------------------------------------- EvidenceRef / DerivationRef

    #[test]
    fn evidence_ref_is_a_transparent_validated_id() {
        let raw = "evd:0123456789abcdef";
        let r = EvidenceRef::from(id(raw));
        assert_eq!(r.as_str(), raw);
        assert_eq!(r.as_id(), &id(raw));
        assert_json_round_trip(&r, json!(raw));
        // No prefix restriction at the primitive level.
        let other: EvidenceRef = serde_json::from_value(json!("src:0123456789abcdef")).unwrap();
        assert_eq!(other.as_str(), "src:0123456789abcdef");
        for bad in ["Evd:x", "evd:", "evd", "evd:a b", ""] {
            rejects::<EvidenceRef>(json!(bad));
        }
        rejects::<EvidenceRef>(json!(7));
    }

    #[test]
    fn derivation_ref_is_a_transparent_validated_id() {
        let raw = "drv:hr:segmentation.1";
        let r = DerivationRef::from(id(raw));
        assert_eq!(r.as_str(), raw);
        assert_eq!(r.as_id(), &id(raw));
        assert_json_round_trip(&r, json!(raw));
        let other: DerivationRef = serde_json::from_value(json!("resolution:01K1")).unwrap();
        assert_eq!(other.as_str(), "resolution:01K1");
        for bad in ["Drv:x", "drv:", "drv::x", " drv:x", ""] {
            rejects::<DerivationRef>(json!(bad));
        }
    }

    // ---------------------------------------------------------------- ExtensionKey

    const VALID_KEYS: &[&str] = &[
        "acme:priority",
        "jira:issue_key",
        "customer-x:classification.v2",
    ];

    const INVALID_KEYS: &[&str] = &[
        "priority",
        "Acme:priority",
        "acme:",
        "acme::priority",
        "acme:priority:extra",
        "acme:bad key",
    ];

    #[test]
    fn extension_key_accepts_the_approved_examples() {
        for s in VALID_KEYS {
            let key: ExtensionKey = s.parse().unwrap();
            assert_eq!(key.as_str(), *s);
            assert_eq!(key.to_string(), *s);
            assert_eq!(key.as_ref(), *s);
            assert_eq!(ExtensionKey::try_from((*s).to_owned()).unwrap(), key);
            assert_json_round_trip(&key, json!(s));
        }
    }

    #[test]
    fn extension_key_rejects_the_approved_invalid_examples() {
        for s in INVALID_KEYS {
            assert_eq!(
                s.parse::<ExtensionKey>(),
                Err(InvalidExtensionKey((*s).to_owned()))
            );
            rejects::<ExtensionKey>(json!(s));
        }
        for s in [
            "",
            ":priority",
            " acme:priority",
            "acme:priority ",
            "acme:prio\trity",
        ] {
            assert!(s.parse::<ExtensionKey>().is_err(), "{s:?}");
        }
        rejects::<ExtensionKey>(json!(1));
    }

    #[test]
    fn extension_key_map_round_trips_and_rejects_invalid_keys() {
        let mut map = BTreeMap::new();
        map.insert(
            "jira:issue_key".parse::<ExtensionKey>().unwrap(),
            json!("HR-42"),
        );
        map.insert("acme:priority".parse::<ExtensionKey>().unwrap(), json!(3));
        assert_json_round_trip(&map, json!({"acme:priority": 3, "jira:issue_key": "HR-42"}));
        rejects::<BTreeMap<ExtensionKey, Value>>(json!({"priority": 1}));
    }

    // ---------------------------------------------------------------- AuditMeta

    const CREATED: &str = "2026-09-29T12:00:00.000000000Z";
    const LATER: &str = "2026-09-30T08:00:00.000000000Z";
    const EARLIER: &str = "2026-09-28T08:00:00.000000000Z";

    #[test]
    fn audit_meta_create_only_round_trips() {
        let meta = AuditMeta::new(id("actor:analyst"), ts(CREATED), None, None).unwrap();
        assert_json_round_trip(
            &meta,
            json!({
                "created_by": "actor:analyst",
                "created_at": CREATED,
                "updated_by": null,
                "updated_at": null
            }),
        );
        let omitted: AuditMeta =
            serde_json::from_value(json!({"created_by": "actor:analyst", "created_at": CREATED}))
                .unwrap();
        assert_eq!(omitted, meta);
    }

    #[test]
    fn audit_meta_create_and_update_round_trips() {
        let meta = AuditMeta::new(
            id("actor:analyst"),
            ts(CREATED),
            Some(id("actor:reviewer")),
            Some(ts(LATER)),
        )
        .unwrap();
        assert_json_round_trip(
            &meta,
            json!({
                "created_by": "actor:analyst",
                "created_at": CREATED,
                "updated_by": "actor:reviewer",
                "updated_at": LATER
            }),
        );
    }

    #[test]
    fn audit_meta_rejects_only_updated_by() {
        assert_eq!(
            AuditMeta::new(id("actor:a"), ts(CREATED), Some(id("actor:b")), None),
            Err(AuditMetaError::UpdatedByWithoutUpdatedAt)
        );
        rejects::<AuditMeta>(json!({
            "created_by": "actor:a", "created_at": CREATED, "updated_by": "actor:b"
        }));
    }

    #[test]
    fn audit_meta_rejects_only_updated_at() {
        assert_eq!(
            AuditMeta::new(id("actor:a"), ts(CREATED), None, Some(ts(LATER))),
            Err(AuditMetaError::UpdatedAtWithoutUpdatedBy)
        );
        rejects::<AuditMeta>(json!({
            "created_by": "actor:a", "created_at": CREATED, "updated_at": LATER
        }));
    }

    #[test]
    fn audit_meta_rejects_update_before_creation() {
        assert_eq!(
            AuditMeta::new(
                id("actor:a"),
                ts(CREATED),
                Some(id("actor:b")),
                Some(ts(EARLIER))
            ),
            Err(AuditMetaError::UpdatedBeforeCreated {
                created_at: ts(CREATED),
                updated_at: ts(EARLIER)
            })
        );
        rejects::<AuditMeta>(json!({
            "created_by": "actor:a", "created_at": CREATED,
            "updated_by": "actor:b", "updated_at": EARLIER
        }));
    }

    #[test]
    fn audit_meta_accepts_update_at_the_creation_instant() {
        let meta = AuditMeta::new(
            id("actor:a"),
            ts(CREATED),
            Some(id("actor:b")),
            Some(ts(CREATED)),
        )
        .unwrap();
        assert_eq!(meta.validate(), Ok(()));
        let text = serde_json::to_string(&meta).unwrap();
        assert_eq!(serde_json::from_str::<AuditMeta>(&text).unwrap(), meta);
    }

    #[test]
    fn audit_meta_explicit_validation_reports_invalid_programmatic_values() {
        let only_by = AuditMeta {
            created_by: id("actor:a"),
            created_at: ts(CREATED),
            updated_by: Some(id("actor:b")),
            updated_at: None,
        };
        assert_eq!(
            only_by.validate(),
            Err(AuditMetaError::UpdatedByWithoutUpdatedAt)
        );
        let only_at = AuditMeta {
            updated_by: None,
            updated_at: Some(ts(LATER)),
            ..only_by.clone()
        };
        assert_eq!(
            only_at.validate(),
            Err(AuditMetaError::UpdatedAtWithoutUpdatedBy)
        );
        let backwards = AuditMeta {
            updated_by: Some(id("actor:b")),
            updated_at: Some(ts(EARLIER)),
            ..only_by
        };
        assert!(matches!(
            backwards.validate(),
            Err(AuditMetaError::UpdatedBeforeCreated { .. })
        ));
    }

    #[test]
    fn audit_meta_rejects_missing_mandatory_fields_invalid_values_and_extra_fields() {
        rejects::<AuditMeta>(json!({"created_at": CREATED}));
        rejects::<AuditMeta>(json!({"created_by": "actor:a"}));
        rejects::<AuditMeta>(json!({"created_by": "Actor:a", "created_at": CREATED}));
        rejects::<AuditMeta>(json!({"created_by": "actor:a", "created_at": "yesterday"}));
        rejects::<AuditMeta>(json!({
            "created_by": "actor:a", "created_at": CREATED, "session": "ses:1"
        }));
    }

    // ------------------------------------------------------------------ Fable #3 pin

    /// The PSG schema version the canonical bytes below were pinned under.
    const PINNED_PSG_SCHEMA_VERSION: u32 = 1;

    /// SHA-256 of the RFC 8785 bytes of one fixture Node per payload variant (and one Edge).
    /// A change to the serialized form of Node, Edge or NodePayload changes these bytes: bump
    /// `plumb_psg::PSG_SCHEMA_VERSION`, then re-pin both this table and its version.
    const PINNED: &[(&str, &str)] = &[
        (
            "SourceArtifact",
            "sha256:8956d2b904da5ee0e8a3db7748205daf96c7f1fdd078430ef22fbc793539fd11",
        ),
        (
            "EvidenceFragment",
            "sha256:d001a62bf696316773d6453c6d81900c7d0a7fb905002a838b39f1bc578ca62d",
        ),
        (
            "DerivationRecord",
            "sha256:4331169d513e293c1367bd986af55c7ae9e82faa89ed0487166c86e0e098d3ba",
        ),
        (
            "Agent",
            "sha256:89429bf18c19be84a2450e4438c47f0163411067cae2bd9c67d2515a002e7c4f",
        ),
        (
            "Finding",
            "sha256:3b1be29d1a159829351a2a9fde2b10e28e7c5ceef37cc1d0a4cec69b924fbdb8",
        ),
        (
            "Question",
            "sha256:3d8aac5236448e23bee6e3f9989024734bb95080ef5f0ffbe1134c37860a882f",
        ),
        (
            "ResolutionDecision",
            "sha256:b2430bb0426dfea6193e57779df54515de447b60d4ea3a4c06a1b3cb6711cd91",
        ),
        (
            "Assumption",
            "sha256:3eef29c06d4b7a956f13ab47e4e4c87fddd57103e4e73558a69b7d9ee3499475",
        ),
        (
            "Stakeholder",
            "sha256:eb58c05f58da4bc0b8c14896e7511c886eaacde81d009a50240ae22884d0c379",
        ),
        (
            "Concern",
            "sha256:0216b86d61b1886d44f62e98c40818b00e3dc6eba54af20dbe6f939bf93e6ac6",
        ),
        (
            "Goal",
            "sha256:d4d5c5d55ceb90c21a1d59b2051d4e20077dba29257dc5299011c7551a5edc36",
        ),
        (
            "Need",
            "sha256:8ab8e402eb170751e6988b06ed9f96e16f8397cbe585891e707a953b1b73ca10",
        ),
        (
            "Requirement",
            "sha256:4d046618fa1591987ce2aad97f67d652302003a27679c142202ad11cc1b44067",
        ),
        (
            "AcceptanceCriterion",
            "sha256:70dbb10ac98142e0855c54b70f2e0d8050e94155eb49a118577f2b5062ce8fa6",
        ),
        (
            "Constraint",
            "sha256:0dbb27e606feb998453184719ba5e65bc11cb4cc6f20e61d09aa0dd81cef71eb",
        ),
        (
            "Term",
            "sha256:2884a9f2f18de3d9b7cbf2f9acacae4f285c0456cef493b909f6a96b5bc0f764",
        ),
        (
            "Concept",
            "sha256:79ec7161a2c546f84655b60bc39204de216c110dad918e7580605061c2c265c6",
        ),
        (
            "Actor",
            "sha256:ebf0979efb696ffe24066f7598f8f26f2cfec203b1bae16aae3cec293f07a76b",
        ),
        (
            "BusinessRole",
            "sha256:adcbe8447f96008bd3d2e7fbcb0b9fca2aa837f6be2f90d604c1ee253b3df674",
        ),
        (
            "Entity",
            "sha256:e05f6e904a33223e88f8a849da1ce4538213057d0aa8b8378a97a051edbe7ee6",
        ),
        (
            "Attribute",
            "sha256:6bd7352ba7e1a0a6be85329a8b546bba5514fcc36a1892e60a50f7901b5de86b",
        ),
        (
            "DomainRelationship",
            "sha256:f275ad110058f46aa5b3ef3b2b40cef31b7fde728a51d7e991fb22903c8dab1f",
        ),
        (
            "State",
            "sha256:9df63448157df693d65d06c251192379d5e4845c184f66822280155811bc0a6b",
        ),
        (
            "Transition",
            "sha256:bce1d39378438dcd283d12b23d7264c2d4612169d82159c4f3ed4d0b54ea2826",
        ),
        (
            "Invariant",
            "sha256:49ff32295c21421448d3d12c1d8d638a443668b4a3cfb2ed0a733160ee28c221",
        ),
        (
            "Operation",
            "sha256:aa6ba8db2298146789dab04e986af6ce8959c90efcf37689f44e192757b2c4b6",
        ),
        (
            "Outcome",
            "sha256:efde696f2f1678a970aa6fba45e9b9843e5865adac4429bd27fd97462c8eac80",
        ),
        (
            "Event",
            "sha256:006dedf88aaaea6013a38a6e5164652a440491adfcafbca80e76dab32a585f02",
        ),
        (
            "Process",
            "sha256:65df37a2130919124e54fb6e5e618b5932f57a410c950efb86cacbfacc50e5a1",
        ),
        (
            "ProcessNode",
            "sha256:250c97ca3c8b939b446e1a7131908992e0c3820e5df49aeb4c418a9e8800ca17",
        ),
        (
            "Rule",
            "sha256:cbe45b2a02aa48b48a5edf8107c0b62bd7248bde42400003db1d06751ee9c2a9",
        ),
        (
            "DecisionTable",
            "sha256:8904ecfb96003cc0dfffadd10431a6c130979d37c64cf9a9b00dcca44bfc53b4",
        ),
        (
            "Calculation",
            "sha256:c849f46209f078da236947e371ff912db5b7f186e2f55b26120b189b8f43499e",
        ),
        (
            "Calendar",
            "sha256:752f6d89ef9c78c08819a711fce9e2a74360c80761a07e134479800ed8d4de73",
        ),
        (
            "Scenario",
            "sha256:3cce29404e038abab311802f1c567e5cef6e3d96a9d2c11473eed2cbaaa2550a",
        ),
        (
            "Principal",
            "sha256:5a1ff958c06537a1b23c0253bf6a596e8de1960572c5c5f6fdc551a253ad3356",
        ),
        (
            "SecurityRole",
            "sha256:ac50e1dc318ef4c376c50a20c0709609714113059b09d9f4f440975a2841fced",
        ),
        (
            "Permission",
            "sha256:d50c1dd2a2be98aec343fb441e3ff9b5e0ba58cb049274d8c68a1e39e7024599",
        ),
        (
            "ResourceScope",
            "sha256:ee5cc60b48a769db5412e144ebdf576081c919eed2a70ad878f48b16ced4d558",
        ),
        (
            "PolicyCondition",
            "sha256:be0f898bbf8a2feb143e73459bcf855bfcaeffbdade7cfc959f9749f1ce107fa",
        ),
        (
            "SeparationConstraint",
            "sha256:f37a0cacfb21b3174845ac60e4863384b95cc4cb48dc07904cab7ea0c09576a7",
        ),
        (
            "QualityCharacteristic",
            "sha256:01f7582ff4567b17f404bc66cf95bb89d6d4c5a10da3a35bce5538cb3ab577e1",
        ),
        (
            "Measure",
            "sha256:75cee9bd1d6dcb13c8d583fd3842957e933d6a98c933fe5b8c19ebc86c98e870",
        ),
        (
            "QualityScenario",
            "sha256:303e9f15da64adcbfc69a247d207e72cc5e635c818962ca0d6cfcbc145438351",
        ),
        (
            "SystemOfInterest",
            "sha256:0b8733dda31e42548018bc660a1138f2821ffbdd0211030910a7535d88949bd5",
        ),
        (
            "ArchitectureDescription",
            "sha256:91f45a484979fc9e4359ca180b8f2afe28f0202eec7c05c300f473b22655410d",
        ),
        (
            "ArchitectureCandidate",
            "sha256:e3b9ec9ceb80a09ebbd354540115f9c978217a1747b2047be3a603a452954820",
        ),
        (
            "Viewpoint",
            "sha256:50d3eef7023b32a2dca8600f85e42aac7020815d9e334cd5b0350f3a7055da81",
        ),
        (
            "View",
            "sha256:b3868159a6cec831b04d2875d89f5a1d5c032c9f7810d4c1caacf6e16648e669",
        ),
        (
            "ModelKind",
            "sha256:976fb6c9aed265c86c72625abeed8280c3197279eb1ea62e1d958f5a1caad288",
        ),
        (
            "SoftwareSystem",
            "sha256:afd581f9d0bcadbfd73c8574db862bbc079413c93ab5c5f2715241cc791035ab",
        ),
        (
            "Container",
            "sha256:9e4d25b70a62ffe14ef1ac3e9b040c67b3addeabc4789b0b3409e572681122b4",
        ),
        (
            "Component",
            "sha256:3ccbbed4dac0f8cd899f885d452ad084d754d436931645f2063004fdcd9028de",
        ),
        (
            "Module",
            "sha256:a58db2336801b47f3b0b8521bc121cdfb64d9ef409ba2412c2531615604507f0",
        ),
        (
            "Interface",
            "sha256:98a59e8fedb4f97f69b3c201f863f5993a576d6be6ab50bd2e94b3cd464c844b",
        ),
        (
            "DataStore",
            "sha256:42b9fdee9111ac4339e7634c72a4ff34f0cbc5ffeb857fd28a6170c908dd5589",
        ),
        (
            "ExternalSystem",
            "sha256:c07d271b90c6ad02471c2d36f53ef97b099f520adec4069f0e98f171dad08778",
        ),
        (
            "DeploymentNode",
            "sha256:453189dd85610665000136469b66e8ecded2daa22ed4d8a090f33c339e815ea9",
        ),
        (
            "RuntimeEnvironment",
            "sha256:50371f243e9f6213bdffa51a3caff14f2c63b51790880362a3013ea11f87cc88",
        ),
        (
            "NetworkZone",
            "sha256:e63c4d84859a37f8ae9e4aae03feaaf896ff17cefb3250caddb741db23bc5b1b",
        ),
        (
            "ArchitectureDecision",
            "sha256:2772eef9198c4576f06bd257869811d70d9396bf8d94e2cb089a798163e589cb",
        ),
        (
            "Technology",
            "sha256:91046be1c8cff71feea6d554d0d7c987300a61852c8c4b81caa7d38754a47b78",
        ),
        (
            "TechnologySelection",
            "sha256:cfef8e65a869ef709907d55c79ec055345c2c7df20464ead788a075804877581",
        ),
        (
            "ApiContract",
            "sha256:b66cb370eace447a374d2b0b0b85721f2f4afe249c268c90b04a0e4773f7b9da",
        ),
        (
            "ApiOperation",
            "sha256:c558b0a2d5c75fb9cd1b732857677edfe206beab16eb1583298e7c38050b8d1f",
        ),
        (
            "EventContract",
            "sha256:e360b38d0ec16e9480009f7abf8c6cd35853032a25de19c81242facff146db75",
        ),
        (
            "Channel",
            "sha256:1fe5a03a1399b98c010f2e088914d9ead9b940a838931aeb676d048304dddda9",
        ),
        (
            "Message",
            "sha256:536b7f0aef878978dc3f910ebf95d1e53731913324559bd51244e731663ba271",
        ),
        (
            "DataSchema",
            "sha256:88a4274937ced7cf9fdf8b250beced5ebd04ad6659083414b331c8ebe9a9beeb",
        ),
        (
            "TechnicalWorkflow",
            "sha256:7c29855b61c2d40605d56c180ef61deb8ea29776fcc5ad6e5dd315b109a91f72",
        ),
        (
            "Capability",
            "sha256:f52f1bd971d07700656dcefc0e4404c5e1fcbc149dcca4aa27a804919fa6c0ab",
        ),
        (
            "ImplementationSlice",
            "sha256:f720997ac202281d72c4d094eb94d3b55fa0dbd9a9c5140726ef5075fb2e6ae9",
        ),
        (
            "WorkPackage",
            "sha256:ce2ab2192a851614076202f4eb520f69762c147f5178541b9a68e4b3e0d305e0",
        ),
        (
            "TaskContract",
            "sha256:8b850d5b62d50a22c42eaaab30df4128cedfff7f56cbb90050a78ca4447ef53c",
        ),
        (
            "Migration",
            "sha256:1f08230474af3f12a58f165c5509f22ca9c821b5e9883f4c6df973d3f3d05daf",
        ),
        (
            "Release",
            "sha256:625cf6cc84dd64e0623e074e8b83d4aae9e9c3092c5364d1d5dcef22d42aa6b7",
        ),
        (
            "VerificationObligation",
            "sha256:526e4259558854d5910f042abd86b9563c81e120d5d619455308ca8736a32cfe",
        ),
        (
            "TestCase",
            "sha256:856b7682f495c9671fe31330f61f5cae726da31354ff8b314a5e9daa89a9431b",
        ),
        (
            "ScenarioRun",
            "sha256:04ac1c94d107de0d5056211e6e14278d2390841cd2370b6e8451daa6e2e52bdb",
        ),
        (
            "TestExecution",
            "sha256:992a6e298c8ad1115760ae6936c3a71921145b6aa09af71768a603d9cc60b6c7",
        ),
        (
            "TestReceipt",
            "sha256:f6279f5e88b93bbd981a7b4d5f574f62231dda07dbe5aef138b4463f67bc08d1",
        ),
        (
            "CodeBinding",
            "sha256:ee7b36affb57b1a08d212b1d5c622658af889c1f3e561d1bd6f94c9771c5ecf3",
        ),
        (
            "ArchitectureCheck",
            "sha256:dd148b56c0015be43310e9381d0b9454ed27c343f0cf9c643c0075185fc4024f",
        ),
        (
            "CoverageRecord",
            "sha256:f3f434d4e818a1cf462ac6bc2d3c7c10a080a52fbea8da1c00d5b7ceec33fd09",
        ),
        (
            "StandardsProfile",
            "sha256:682ed816df2f6d561e590258d4adf51b56aac1f20be23d0d50dafe5035816396",
        ),
        (
            "Extension",
            "sha256:db57ab9d2458c1105f8d02e70bbbaf85d9aa2fe117cdaf68450400f3ed3168f8",
        ),
        (
            "Edge",
            "sha256:b336be37bd523ccda3be6f38cceec296948791ceeb8449dd73fd4a0914986587",
        ),
    ];

    fn fixture_node(tag: &str, data: &Value) -> plumb_psg::Node {
        let id = match tag {
            "DerivationRecord" | "StandardsProfile" => data["id"].as_str().unwrap().to_owned(),
            _ => format!("fixture:{tag}"),
        };
        serde_json::from_value(json!({
            "id": id, "revision": 2, "status": "Accepted",
            "payload": {"type": tag, "data": data},
            "evidence": ["evd:0123456789abcdef"], "derivations": ["drv:s1:classify-1"],
            "standards": [], "tags": ["pilot"], "extensions": {"acme:priority": 2},
            "audit": {"created_by": "actor:analyst", "created_at": "2026-09-29T08:00:00.000000000Z",
                      "updated_by": "actor:reviewer", "updated_at": "2026-09-29T09:00:00.000000000Z"}
        }))
        .unwrap_or_else(|e| panic!("{tag}: {e}"))
    }

    fn fixture_edge() -> plumb_psg::Edge {
        serde_json::from_value(json!({
            "id": "rel:fixture", "revision": 1, "status": "Accepted", "kind": "constrained_by",
            "from": "req:a", "to": "con:eu", "properties": {}, "evidence": [],
            "derivations": [], "standards": [],
            "audit": {"created_by": "actor:analyst", "created_at": "2026-09-29T08:00:00.000000000Z",
                      "updated_by": null, "updated_at": null}
        }))
        .unwrap()
    }

    fn digest<T: Serialize>(value: &T) -> String {
        plumb_core::Hash::content_sha256(&plumb_core::to_canonical_json(value).unwrap()).to_string()
    }

    #[test]
    fn canonical_bytes_of_every_payload_variant_are_pinned_to_the_psg_schema_version() {
        let mut actual: Vec<(String, String)> = crate::payload_fixtures::fixtures()
            .into_iter()
            .map(|f| (f.tag.to_owned(), digest(&fixture_node(f.tag, &f.data))))
            .collect();
        assert_eq!(actual.len(), plumb_psg::NodeType::ALL.len());
        actual.push(("Edge".to_owned(), digest(&fixture_edge())));
        let pinned: Vec<(String, String)> = PINNED
            .iter()
            .map(|(t, h)| ((*t).to_owned(), (*h).to_owned()))
            .collect();
        if pinned != actual {
            let table: String = actual
                .iter()
                .map(|(t, h)| format!("        (\"{t}\", \"{h}\"),\n"))
                .collect();
            panic!("the canonical PSG bytes changed: bump PSG_SCHEMA_VERSION and re-pin:\n{table}");
        }
        assert_eq!(
            plumb_psg::PSG_SCHEMA_VERSION,
            PINNED_PSG_SCHEMA_VERSION,
            "PSG_SCHEMA_VERSION changed: re-pin the canonical bytes under the new version"
        );
    }
}
