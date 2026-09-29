//! F0.3 envelope-primitive contract tests.
//!
//! Every test lives in `serde_contract` so that the task command
//! `cargo test -p plumb-psg serde` selects all of them.

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
}
