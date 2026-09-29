//! Hotfix 015 contract tests for the canonical `Proposal`.
//!
//! The golden ID was calculated independently from the literal canonical JSON below.

mod proposal_contract {
    use plumb_core::{to_canonical_json, Hash, Id, StageId};
    use plumb_patch::*;
    use plumb_psg::{DerivationRef, ElementStatus, EvidenceRef};
    use serde_json::{json, Value};

    const GOLDEN_ID: &str = "prop:512079ebd217cddf";
    const GOLDEN_JSON: &str = r#"{"acceptance_policy":"HUMAN_CONFIRM","confidence":0.75,"derivation_refs":["drv:48d777dc5cc38d3c"],"evidence_refs":["evd:0123456789abcdef","evd:fedcba9876543210"],"id":"prop:512079ebd217cddf","materiality":"semantic","patch_set":{"base_semantic_hash":"psg:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","patch":{"from":"Proposed","op":"SetStatus","target":{"expected_hash":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","id":"req:HR-001"},"to":"Accepted"}},"stage":"S1"}"#;

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn hash(s: &str) -> Hash {
        s.parse().unwrap()
    }

    fn patch_set(base: char) -> PatchSet {
        PatchSet {
            base_semantic_hash: hash(&format!("psg:sha256:{}", base.to_string().repeat(64))),
            patch: SemanticPatch::SetStatus {
                target: ElementPrecondition {
                    id: id("req:HR-001"),
                    expected_hash: hash(&format!("sha256:{}", "b".repeat(64))),
                },
                from: ElementStatus::Proposed,
                to: ElementStatus::Accepted,
            },
        }
    }

    fn evidence(ids: &[&str]) -> Vec<EvidenceRef> {
        ids.iter().map(|s| EvidenceRef::from(id(s))).collect()
    }

    fn derivations(ids: &[&str]) -> Vec<DerivationRef> {
        ids.iter().map(|s| DerivationRef::from(id(s))).collect()
    }

    /// The golden proposal, built with evidence refs in reverse order.
    fn golden() -> Proposal {
        Proposal::new(
            StageId::S1,
            patch_set('a'),
            evidence(&["evd:fedcba9876543210", "evd:0123456789abcdef"]),
            derivations(&["drv:48d777dc5cc38d3c"]),
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            Some(0.75),
        )
        .unwrap()
    }

    fn wire() -> Value {
        serde_json::from_str(GOLDEN_JSON).unwrap()
    }

    #[test]
    fn golden_proposal_json_and_id() {
        let p = golden();
        assert_eq!(p.id.as_str(), GOLDEN_ID);
        assert_eq!(p.recompute_id().unwrap(), p.id);
        assert_eq!(to_canonical_json(&p).unwrap(), GOLDEN_JSON.as_bytes());
        assert_eq!(
            p.evidence_refs,
            evidence(&["evd:0123456789abcdef", "evd:fedcba9876543210"])
        );
        let parsed: Proposal = serde_json::from_str(GOLDEN_JSON).unwrap();
        assert_eq!(parsed, p);
        let mut projection = wire();
        projection.as_object_mut().unwrap().remove("id");
        assert_eq!(
            to_canonical_json(&p.identity_projection()).unwrap(),
            to_canonical_json(&projection).unwrap()
        );
        assert_eq!(p.validate(), Ok(()));
    }

    #[test]
    fn materiality_and_policy_wire_values_are_exact() {
        let materiality = ["non_semantic", "semantic", "material_decision"];
        assert_eq!(ProposalMateriality::ALL.len(), 3);
        for (value, text) in ProposalMateriality::ALL.iter().zip(materiality) {
            assert_eq!(value.as_str(), text);
            assert_eq!(serde_json::to_value(value).unwrap(), json!(text));
            assert_eq!(
                serde_json::from_value::<ProposalMateriality>(json!(text)).unwrap(),
                *value
            );
        }
        let policies = [
            "AUTO_DERIVATION",
            "AUTO_NON_SEMANTIC",
            "HUMAN_CONFIRM",
            "HUMAN_DECISION",
            "PROFILE_POLICY",
        ];
        assert_eq!(AcceptancePolicy::ALL.len(), 5);
        for (value, text) in AcceptancePolicy::ALL.iter().zip(policies) {
            assert_eq!(value.as_str(), text);
            assert_eq!(serde_json::to_value(value).unwrap(), json!(text));
            assert_eq!(
                serde_json::from_value::<AcceptancePolicy>(json!(text)).unwrap(),
                *value
            );
        }
        for bad in ["Semantic", "SEMANTIC", "material-decision", "", "other"] {
            assert!(serde_json::from_value::<ProposalMateriality>(json!(bad)).is_err());
        }
        for bad in [
            "human_confirm",
            "HumanConfirm",
            "AUTO",
            "",
            "HUMAN_CONFIRM ",
        ] {
            assert!(serde_json::from_value::<AcceptancePolicy>(json!(bad)).is_err());
        }
        let mut w = wire();
        w["materiality"] = json!("Semantic");
        assert!(serde_json::from_value::<Proposal>(w).is_err());
        let mut w = wire();
        w["acceptance_policy"] = json!("human_confirm");
        assert!(serde_json::from_value::<Proposal>(w).is_err());
    }

    #[test]
    fn provenance_refs_are_canonical_sets() {
        let dup = Proposal::new(
            StageId::S1,
            patch_set('a'),
            evidence(&["evd:a", "evd:a"]),
            vec![],
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        );
        assert!(matches!(
            dup,
            Err(ProposalError::DuplicateRef {
                field: "evidence_refs",
                ..
            })
        ));
        let dup = Proposal::new(
            StageId::S1,
            patch_set('a'),
            vec![],
            derivations(&["drv:x", "drv:x"]),
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        );
        assert!(matches!(
            dup,
            Err(ProposalError::DuplicateRef {
                field: "derivation_refs",
                ..
            })
        ));
        let sorted = Proposal::new(
            StageId::S1,
            patch_set('a'),
            vec![],
            derivations(&["drv:b", "drv:a"]),
            ProposalMateriality::Semantic,
            AcceptancePolicy::HumanConfirm,
            None,
        )
        .unwrap();
        assert_eq!(sorted.derivation_refs, derivations(&["drv:a", "drv:b"]));

        let mut w = wire();
        w["evidence_refs"] = json!(["evd:fedcba9876543210", "evd:0123456789abcdef"]);
        assert!(serde_json::from_value::<Proposal>(w).is_err());
        let mut w = wire();
        w["evidence_refs"] = json!(["evd:0123456789abcdef", "evd:0123456789abcdef"]);
        assert!(serde_json::from_value::<Proposal>(w).is_err());
        let mut w = wire();
        w["derivation_refs"] = json!(["drv:b", "drv:a"]);
        assert!(serde_json::from_value::<Proposal>(w).is_err());
        let mut w = wire();
        w["derivation_refs"] = json!(["Not An Id"]);
        assert!(serde_json::from_value::<Proposal>(w).is_err());
    }

    #[test]
    fn wrong_id_hash_kind_and_unknown_fields_are_rejected() {
        let mut w = wire();
        w["id"] = json!("prop:0000000000000000");
        let err = serde_json::from_value::<Proposal>(w).unwrap_err();
        assert!(
            err.to_string().contains("does not match recomputed id"),
            "{err}"
        );
        let mut w = wire();
        w["patch_set"]["base_semantic_hash"] = json!(format!("sha256:{}", "a".repeat(64)));
        assert!(serde_json::from_value::<Proposal>(w).is_err());
        let mut p = golden();
        p.patch_set.base_semantic_hash = hash(&format!("ev:sha256:{}", "a".repeat(64)));
        assert!(matches!(
            p.validate(),
            Err(ProposalError::InvalidBaseHashKind(_))
        ));
        for extra in [
            "base_revision",
            "impact",
            "intake",
            "base_semantic_hash",
            "findings",
        ] {
            let mut w = wire();
            w[extra] = json!(null);
            assert!(serde_json::from_value::<Proposal>(w).is_err(), "{extra}");
        }
        let mut w = wire();
        w["patch_set"]["extra"] = json!(1);
        assert!(serde_json::from_value::<Proposal>(w).is_err());
    }

    #[test]
    fn confidence_bounds() {
        for ok in [Some(0.0), Some(1.0), Some(0.5), None] {
            let p = Proposal::new(
                StageId::S1,
                patch_set('a'),
                vec![],
                vec![],
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
                ok,
            )
            .unwrap();
            assert_eq!(p.confidence, ok);
            let back: Proposal = serde_json::from_slice(&to_canonical_json(&p).unwrap()).unwrap();
            assert_eq!(back, p);
        }
        for bad in [-0.01f32, 1.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let err = Proposal::new(
                StageId::S1,
                patch_set('a'),
                vec![],
                vec![],
                ProposalMateriality::Semantic,
                AcceptancePolicy::HumanConfirm,
                Some(bad),
            )
            .unwrap_err();
            assert!(matches!(err, ProposalError::InvalidConfidence(_)), "{bad}");
            let mut p = golden();
            p.confidence = Some(bad);
            assert!(matches!(
                p.validate(),
                Err(ProposalError::InvalidConfidence(_))
            ));
        }
        let mut w = wire();
        w["confidence"] = json!(1.5);
        assert!(serde_json::from_value::<Proposal>(w).is_err());
        let mut w = wire();
        w["confidence"] = json!(-1);
        assert!(serde_json::from_value::<Proposal>(w).is_err());
    }

    #[test]
    fn every_identity_field_changes_the_id() {
        let build = |stage, ps, ev: &[&str], dv: &[&str], m, a, c| {
            Proposal::new(stage, ps, evidence(ev), derivations(dv), m, a, c)
                .unwrap()
                .id
        };
        let ev = ["evd:0123456789abcdef", "evd:fedcba9876543210"];
        let dv = ["drv:48d777dc5cc38d3c"];
        use AcceptancePolicy as A;
        use ProposalMateriality as M;
        let base = build(
            StageId::S1,
            patch_set('a'),
            &ev,
            &dv,
            M::Semantic,
            A::HumanConfirm,
            Some(0.75),
        );
        assert_eq!(base.as_str(), GOLDEN_ID);
        let variants = [
            build(
                StageId::S2,
                patch_set('a'),
                &ev,
                &dv,
                M::Semantic,
                A::HumanConfirm,
                Some(0.75),
            ),
            build(
                StageId::S1,
                patch_set('c'),
                &ev,
                &dv,
                M::Semantic,
                A::HumanConfirm,
                Some(0.75),
            ),
            build(
                StageId::S1,
                patch_set('a'),
                &ev[..1],
                &dv,
                M::Semantic,
                A::HumanConfirm,
                Some(0.75),
            ),
            build(
                StageId::S1,
                patch_set('a'),
                &ev,
                &[],
                M::Semantic,
                A::HumanConfirm,
                Some(0.75),
            ),
            build(
                StageId::S1,
                patch_set('a'),
                &ev,
                &dv,
                M::MaterialDecision,
                A::HumanConfirm,
                Some(0.75),
            ),
            build(
                StageId::S1,
                patch_set('a'),
                &ev,
                &dv,
                M::Semantic,
                A::HumanDecision,
                Some(0.75),
            ),
            build(
                StageId::S1,
                patch_set('a'),
                &ev,
                &dv,
                M::Semantic,
                A::HumanConfirm,
                Some(0.5),
            ),
            build(
                StageId::S1,
                patch_set('a'),
                &ev,
                &dv,
                M::Semantic,
                A::HumanConfirm,
                None,
            ),
        ];
        let mut all: Vec<Id> = variants.to_vec();
        all.push(base);
        let unique: std::collections::BTreeSet<&Id> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
    }
}
