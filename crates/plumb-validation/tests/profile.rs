//! F0.12 contract tests: closed validation vocabularies, the supplied profile, profile
//! integrity, NOW scope, standards references, hashes and the metadata registry.
//!
//! Every test lives in `profile_contract` so that `cargo test -p plumb-validation profile`
//! selects them.

mod profile_contract {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Debug;
    use std::str::FromStr;

    use plumb_core::{GateId, HashKind};
    use plumb_psg::{MappingRole, MappingStrength};
    use plumb_validation::*;
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use serde_yaml::Value;

    /// The authoritative supplied profile, unmodified.
    const SUPPLIED: &str =
        include_str!("../../../config/profiles/plumb-software-2026.1-rules.yaml");

    // Calculated outside this crate (Python: PyYAML load, the documented normalization,
    // json.dumps(sort_keys=True, separators=(",", ":"), ensure_ascii=False), hashlib.sha256)
    // over 95145 and 92373 canonical bytes respectively.
    const PROFILE_HASH: &str =
        "sha256:08b5b1535eedf401dd002c26ffc7301cfc35a119680f75b664de691b41c3a52f";
    const RULE_PACK_HASH: &str =
        "sha256:6677e02dd7740bb294533513a4d5294576c62c65eb8606973ca998b565f1b2e7";

    const GATE_RULE_COUNTS: [(GateId, usize); 13] = [
        (GateId::I0, 6),
        (GateId::F1, 11),
        (GateId::F2, 24),
        (GateId::F3, 6),
        (GateId::F4, 7),
        (GateId::Q1, 8),
        (GateId::A1, 9),
        (GateId::A2, 9),
        (GateId::A3, 14),
        (GateId::A4, 9),
        (GateId::D1, 9),
        (GateId::D2, 10),
        (GateId::C1, 11),
    ];

    // ------------------------------------------------------------------ helpers

    fn supplied() -> ValidationProfile {
        load_profile_yaml(SUPPLIED).unwrap()
    }

    fn registry() -> ValidationRegistry {
        ValidationRegistry::new(supplied()).unwrap()
    }

    fn supplied_value() -> Value {
        serde_yaml::from_str(SUPPLIED).unwrap()
    }

    fn load_value(value: &Value) -> Result<ValidationProfile, ValidationError> {
        load_profile_yaml(&serde_yaml::to_string(value).unwrap())
    }

    /// Loads the supplied profile after `mutate` changed its YAML document.
    fn load_mutated(mutate: impl FnOnce(&mut Value)) -> Result<ValidationProfile, ValidationError> {
        let mut value = supplied_value();
        mutate(&mut value);
        load_value(&value)
    }

    fn seq<'a>(value: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
        value[key].as_sequence_mut().unwrap()
    }

    fn entry<'a>(value: &'a mut Value, list: &str, id: &str) -> &'a mut Value {
        seq(value, list)
            .iter_mut()
            .find(|item| item["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("no {list} entry {id}"))
    }

    fn rule<'a>(value: &'a mut Value, id: &str) -> &'a mut Value {
        entry(value, "rules", id)
    }

    fn gate<'a>(value: &'a mut Value, id: &str) -> &'a mut Value {
        entry(value, "gates", id)
    }

    fn gate_rule_ids<'a>(value: &'a mut Value, id: &str) -> &'a mut Vec<Value> {
        gate(value, id)["rule_ids"].as_sequence_mut().unwrap()
    }

    fn is_yaml_error(result: Result<ValidationProfile, ValidationError>) -> bool {
        matches!(result, Err(ValidationError::Yaml(_)))
    }

    fn hashes(profile: &ValidationProfile) -> (String, String) {
        (
            profile.profile_hash().unwrap().to_string(),
            profile.rule_pack_hash().unwrap().to_string(),
        )
    }

    /// Exhaustive wire contract of one closed vocabulary.
    fn assert_vocabulary<T>(
        all: &[T],
        as_str: fn(T) -> &'static str,
        wires: &[&str],
        wrong: &[&str],
    ) where
        T: Copy + Debug + PartialEq + Serialize + DeserializeOwned + FromStr,
    {
        assert_eq!(all.len(), wires.len());
        for (value, wire) in all.iter().zip(wires) {
            assert_eq!(as_str(*value), *wire);
            assert_eq!(serde_json::to_string(value).unwrap(), format!("\"{wire}\""));
            assert_eq!(
                serde_json::from_str::<T>(&format!("\"{wire}\"")).unwrap(),
                *value
            );
            assert_eq!(serde_yaml::from_str::<T>(wire).unwrap(), *value);
            assert!(wire.parse::<T>().ok() == Some(*value));
        }
        for bad in wrong {
            assert!(
                serde_json::from_str::<T>(&format!("\"{bad}\"")).is_err(),
                "{bad} accepted"
            );
            assert!(bad.parse::<T>().is_err(), "{bad} parsed");
        }
    }

    // ------------------------------------------------------------------ closed vocabularies (§41)

    #[test]
    fn rule_class_has_exactly_five_wire_values() {
        assert_vocabulary(
            &RuleClass::ALL,
            RuleClass::as_str,
            &[
                "external_standard",
                "external_interop",
                "plumb_core",
                "profile_policy",
                "organization_policy",
            ],
            &["PLUMB_CORE", "PlumbCore", "plumb-core", "core", ""],
        );
    }

    #[test]
    fn rule_result_state_has_exactly_six_wire_values() {
        assert_vocabulary(
            &RuleResultState::ALL,
            RuleResultState::as_str,
            &["PASS", "FAIL", "WARN", "NOT_APPLICABLE", "WAIVED", "ERROR"],
            &[
                "pass",
                "Pass",
                "NOT_IMPLEMENTED",
                "NotApplicable",
                "SKIPPED",
                "",
            ],
        );
    }

    #[test]
    fn severity_has_exactly_four_wire_values() {
        assert_vocabulary(
            &Severity::ALL,
            Severity::as_str,
            &["blocker", "error", "warn", "info"],
            &["BLOCKER", "Blocker", "warning", "critical", ""],
        );
    }

    #[test]
    fn waiver_policy_has_exactly_three_wire_values() {
        assert_vocabulary(
            &WaiverPolicy::ALL,
            WaiverPolicy::as_str,
            &["forbidden", "decision_required", "profile_allow"],
            &[
                "FORBIDDEN",
                "DecisionRequired",
                "allowed",
                "decision-required",
                "",
            ],
        );
    }

    #[test]
    fn evaluation_mode_has_exactly_one_wire_value() {
        assert_vocabulary(
            &EvaluationMode::ALL,
            EvaluationMode::as_str,
            &["deterministic"],
            &["DETERMINISTIC", "Deterministic", "llm", "external", ""],
        );
    }

    #[test]
    fn gate_and_mapping_vocabularies_are_reused_not_redefined() {
        let profile = supplied();
        let _: GateId = profile.gates[0].id;
        let _: GateId = profile.rules[0].gate;
        let definition = profile.standards.values().next().unwrap();
        let _: MappingRole = definition.role;
        let reference = profile
            .rules
            .iter()
            .find_map(|r| r.standard_reference.as_ref())
            .unwrap();
        let _: MappingRole = reference.role;
        let _: MappingStrength = reference.mapping_strength;
        for source in PRODUCTION_SOURCES {
            for redefinition in [
                "enum GateId",
                "enum MappingRole",
                "enum MappingStrength",
                "StandardId",
            ] {
                assert!(!source.contains(redefinition), "source has {redefinition}");
            }
        }
    }

    // ------------------------------------------------------------------ supplied profile (§42)

    #[test]
    fn supplied_profile_loads_with_exact_identity_and_counts() {
        let profile = supplied();
        assert_eq!(profile.profile_id.as_str(), "profile:plumb-software-2026.1");
        assert_eq!(profile.profile_version, "2026.1-draft.1");
        assert_eq!(profile.metamodel, "3.0-draft.1");
        assert_eq!(profile.status, "implementation draft");
        assert_eq!(profile.rule_classes.len(), 5);
        assert!(profile.rule_classes.keys().copied().eq(RuleClass::ALL));
        assert_eq!(profile.result_states, RuleResultState::ALL);
        assert_eq!(profile.standards.len(), 15);
        assert_eq!(profile.gates.len(), 13);
        assert_eq!(profile.rules.len(), 133);
        assert_eq!(profile.rules.len(), EXPECTED_RULE_COUNT);
        let references = profile
            .rules
            .iter()
            .filter(|r| r.standard_reference.is_some())
            .count();
        assert_eq!(references, 23);
        assert_eq!(
            profile
                .rules
                .iter()
                .filter(|r| r.remediation.is_some())
                .count(),
            3
        );
    }

    #[test]
    fn supplied_profile_rules_use_exactly_the_expected_vocabulary() {
        let profile = supplied();
        let severities: BTreeSet<_> = profile.rules.iter().map(|r| r.severity).collect();
        assert_eq!(
            severities,
            BTreeSet::from([Severity::Blocker, Severity::Error, Severity::Warn])
        );
        // `info` is unused by the supplied rules and still part of the closed vocabulary.
        assert!(Severity::ALL.contains(&Severity::Info));
        let classes: BTreeSet<_> = profile.rules.iter().map(|r| r.rule_class).collect();
        assert_eq!(
            classes,
            BTreeSet::from([
                RuleClass::ExternalStandard,
                RuleClass::ExternalInterop,
                RuleClass::PlumbCore
            ])
        );
        let waivers: BTreeSet<_> = profile.rules.iter().map(|r| r.waiver_policy).collect();
        assert_eq!(waivers, BTreeSet::from(WaiverPolicy::ALL));
        assert!(profile
            .rules
            .iter()
            .all(|r| r.evaluation == EvaluationMode::Deterministic));
        assert!(profile
            .rules
            .iter()
            .all(|r| r.finding_status_on_fail == "Open"));
    }

    #[test]
    fn supplied_profile_text_is_loaded_verbatim() {
        let registry = registry();
        let rule = registry.rule("PLUMB.I0.SOURCE.PARSE_STATUS").unwrap();
        assert_eq!(rule.gate, GateId::I0);
        assert_eq!(
            rule.title,
            "Required sources are parseable enough for evidence extraction"
        );
        assert_eq!(rule.severity, Severity::Blocker);
        assert_eq!(rule.rule_class, RuleClass::PlumbCore);
        assert_eq!(
            rule.applies_when,
            "SourceArtifact is included in the active gate scope."
        );
        assert_eq!(rule.waiver_policy, WaiverPolicy::DecisionRequired);
        assert_eq!(
            rule.remediation.as_deref(),
            Some("Repair/import with another adapter or explicitly exclude the source from scope by ResolutionDecision.")
        );
        assert_eq!(rule.standard_reference, None);
        // The rule ID is kept exactly as the YAML spells it.
        let interop = registry
            .rule("PPMN.I0.PROVENANCE.AGENT_IDENTIFIED")
            .unwrap();
        assert_eq!(
            interop.standard_reference,
            Some(StandardRef {
                standard: "OMG PPMN".to_owned(),
                version: "1.0".to_owned(),
                role: MappingRole::Interchange,
                note: "Pedigree/provenance interoperability.".to_owned(),
                mapping_strength: MappingStrength::Compatible,
            })
        );
    }

    #[test]
    fn builtin_profile_is_the_supplied_yaml_through_the_same_loader() {
        assert_eq!(load_builtin_software_profile().unwrap(), supplied());
    }

    #[test]
    fn loaded_profile_is_normalized() {
        let profile = supplied();
        assert!(profile.gates.iter().map(|g| g.id).eq(GateId::ALL));
        assert!(profile.rules.windows(2).all(|w| w[0].id < w[1].id));
        for gate in &profile.gates {
            assert!(gate.rule_ids.windows(2).all(|w| w[0] < w[1]));
        }
        let mut again = profile.clone();
        again.normalize();
        assert_eq!(again, profile);
        assert_eq!(profile.validate(), Ok(()));
    }

    // ------------------------------------------------------------------ gate integrity (§43)

    #[test]
    fn supplied_profile_gate_rule_counts_are_exact() {
        let registry = registry();
        for (gate, count) in GATE_RULE_COUNTS {
            assert_eq!(registry.gate(gate).id, gate);
            assert_eq!(registry.gate(gate).rule_ids.len(), count, "{gate}");
            assert_eq!(registry.rules_for_gate(gate).len(), count, "{gate}");
            assert!(registry.rules_for_gate(gate).iter().all(|r| r.gate == gate));
        }
        let listed: BTreeSet<&str> = registry
            .profile()
            .gates
            .iter()
            .flat_map(|g| g.rule_ids.iter().map(String::as_str))
            .collect();
        let defined: BTreeSet<&str> = registry
            .profile()
            .rules
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(listed.len(), 133);
        assert_eq!(listed, defined);
    }

    #[test]
    fn duplicate_rule_metadata_fails_profile_loading() {
        let result = load_mutated(|v| {
            let copy = rule(v, "PLUMB.F1.REQ.GROUNDED").clone();
            seq(v, "rules").push(copy);
        });
        assert_eq!(
            result,
            Err(ValidationError::DuplicateRuleId(
                "PLUMB.F1.REQ.GROUNDED".to_owned()
            ))
        );
        // Also when the count stays 133: one rule replaced by a copy of another.
        let result = load_mutated(|v| {
            let copy = rule(v, "PLUMB.F1.REQ.GROUNDED").clone();
            *rule(v, "PLUMB.F1.REQ.TYPE_KNOWN") = copy;
        });
        assert_eq!(
            result,
            Err(ValidationError::DuplicateRuleId(
                "PLUMB.F1.REQ.GROUNDED".to_owned()
            ))
        );
    }

    #[test]
    fn missing_rule_metadata_is_rejected() {
        let result = load_mutated(|v| {
            seq(v, "rules").retain(|r| r["id"].as_str() != Some("PLUMB.F1.REQ.GROUNDED"));
        });
        assert_eq!(
            result,
            Err(ValidationError::RuleCountMismatch {
                expected: 133,
                actual: 132
            })
        );
    }

    #[test]
    fn extra_rule_metadata_is_rejected() {
        let result = load_mutated(|v| {
            let mut extra = rule(v, "PLUMB.F1.REQ.GROUNDED").clone();
            extra["id"] = Value::from("ORG.F1.REQ.EXTRA");
            seq(v, "rules").push(extra);
        });
        assert_eq!(
            result,
            Err(ValidationError::RuleCountMismatch {
                expected: 133,
                actual: 134
            })
        );
    }

    #[test]
    fn duplicate_gate_is_rejected() {
        let result = load_mutated(|v| gate(v, "F1")["id"] = Value::from("I0"));
        assert_eq!(result, Err(ValidationError::DuplicateGate(GateId::I0)));
    }

    #[test]
    fn missing_gate_is_rejected() {
        let result = load_mutated(|v| {
            seq(v, "gates").retain(|g| g["id"].as_str() != Some("C1"));
        });
        assert_eq!(
            result,
            Err(ValidationError::GateCountMismatch {
                expected: 13,
                actual: 12
            })
        );
    }

    #[test]
    fn rule_duplicated_within_a_gate_is_rejected() {
        let result = load_mutated(|v| {
            gate_rule_ids(v, "F1").push(Value::from("PLUMB.F1.REQ.GROUNDED"));
        });
        assert_eq!(
            result,
            Err(ValidationError::DuplicateGateRule {
                rule_id: "PLUMB.F1.REQ.GROUNDED".to_owned(),
                gate: GateId::F1,
                other_gate: GateId::F1,
            })
        );
    }

    #[test]
    fn rule_listed_by_two_gates_is_rejected() {
        let result = load_mutated(|v| {
            gate_rule_ids(v, "F2").push(Value::from("PLUMB.F1.REQ.GROUNDED"));
        });
        assert_eq!(
            result,
            Err(ValidationError::DuplicateGateRule {
                rule_id: "PLUMB.F1.REQ.GROUNDED".to_owned(),
                gate: GateId::F2,
                other_gate: GateId::F1,
            })
        );
    }

    #[test]
    fn unknown_rule_in_gate_is_rejected() {
        let result = load_mutated(|v| {
            gate_rule_ids(v, "F1").push(Value::from("ORG.F1.REQ.UNKNOWN"));
        });
        assert_eq!(
            result,
            Err(ValidationError::UnknownGateRule {
                gate: GateId::F1,
                rule_id: "ORG.F1.REQ.UNKNOWN".to_owned(),
            })
        );
    }

    #[test]
    fn orphan_rule_is_rejected() {
        let result = load_mutated(|v| {
            gate_rule_ids(v, "F1").retain(|id| id.as_str() != Some("PLUMB.F1.REQ.GROUNDED"));
        });
        assert_eq!(
            result,
            Err(ValidationError::OrphanRule(
                "PLUMB.F1.REQ.GROUNDED".to_owned()
            ))
        );
    }

    #[test]
    fn rule_gate_different_from_containing_gate_is_rejected() {
        let result = load_mutated(|v| rule(v, "PLUMB.F1.REQ.GROUNDED")["gate"] = Value::from("F2"));
        assert_eq!(
            result,
            Err(ValidationError::RuleGateMismatch {
                rule_id: "PLUMB.F1.REQ.GROUNDED".to_owned(),
                declared: GateId::F2,
                listed_by: GateId::F1,
            })
        );
    }

    #[test]
    fn gate_without_rules_and_unknown_gate_id_are_rejected() {
        let result = load_mutated(|v| gate_rule_ids(v, "F3").clear());
        assert!(matches!(
            result,
            Err(ValidationError::InvalidProfile { .. })
        ));
        assert!(is_yaml_error(load_mutated(|v| {
            gate(v, "F3")["id"] = Value::from("F9")
        })));
        assert!(is_yaml_error(load_mutated(|v| {
            rule(v, "PLUMB.F1.REQ.GROUNDED")["gate"] = Value::from("f1")
        })));
    }

    // ------------------------------------------------------------------ NOW scope (§44)

    #[test]
    fn now_scope_requires_exactly_the_rules_of_the_five_now_gates() {
        assert_eq!(
            NOW_GATES,
            [GateId::I0, GateId::F1, GateId::F2, GateId::F3, GateId::F4]
        );
        let registry = registry();
        let required = registry.required_now_rule_ids();
        let deferred = registry.deferred_rule_ids();
        assert_eq!(required.len(), 54);
        assert_eq!(required.len(), EXPECTED_NOW_RULE_COUNT);
        assert_eq!(deferred.len(), 79);
        assert!(required.is_disjoint(&deferred));
        for id in &required {
            assert!(NOW_GATES.contains(&registry.rule(id).unwrap().gate), "{id}");
        }
        for id in &deferred {
            assert!(
                !NOW_GATES.contains(&registry.rule(id).unwrap().gate),
                "{id}"
            );
        }
        for gate in NOW_GATES {
            for rule in registry.rules_for_gate(gate) {
                assert!(required.contains(rule.id.as_str()), "{}", rule.id);
            }
        }
        assert_eq!(required, registry.required_now_rule_ids());
        assert!(required
            .iter()
            .zip(required.iter().skip(1))
            .all(|(a, b)| a < b));
    }

    #[test]
    fn later_gates_are_not_implemented_but_their_metadata_stays_queryable() {
        let registry = registry();
        for gate in GateId::ALL {
            let now = NOW_GATES.contains(&gate);
            assert_eq!(ValidationRegistry::is_now_gate(gate), now);
            let expected = if now {
                Ok(())
            } else {
                Err(ValidationError::GateNotImplemented(gate))
            };
            assert_eq!(ValidationRegistry::require_now_gate(gate), expected);
            assert_eq!(registry.gate(gate).id, gate);
            assert!(!registry.gate(gate).purpose.is_empty());
            assert!(!registry.rules_for_gate(gate).is_empty());
        }
        let later: Vec<GateId> = GateId::ALL
            .into_iter()
            .filter(|g| ValidationRegistry::require_now_gate(*g).is_err())
            .collect();
        assert_eq!(
            later,
            [
                GateId::Q1,
                GateId::A1,
                GateId::A2,
                GateId::A3,
                GateId::A4,
                GateId::D1,
                GateId::D2,
                GateId::C1
            ]
        );
        assert_eq!(
            registry.rule("PLUMB.C1.NO_BLOCKING_DRIFT").map(|r| r.gate),
            Some(GateId::C1)
        );
    }

    #[test]
    fn registry_rejects_a_profile_whose_now_rule_count_differs() {
        // A valid profile in which one Q1 rule is owned by F4 instead.
        let mut value = supplied_value();
        let moved = gate_rule_ids(&mut value, "Q1").remove(0);
        let moved_id = moved.as_str().unwrap().to_owned();
        gate_rule_ids(&mut value, "F4").push(moved);
        rule(&mut value, &moved_id)["gate"] = Value::from("F4");
        let profile = load_value(&value).unwrap();
        assert_eq!(
            ValidationRegistry::new(profile),
            Err(ValidationError::NowRuleCountMismatch {
                expected: 54,
                actual: 55
            })
        );
    }

    // ------------------------------------------------------------------ standards (§45)

    #[test]
    fn every_standard_reference_resolves_to_exactly_one_definition() {
        let profile = supplied();
        assert_eq!(profile.standards.len(), 15);
        let triples: BTreeSet<_> = profile
            .standards
            .values()
            .map(|d| (d.standard.as_str(), d.version.as_str(), d.role))
            .collect();
        assert_eq!(triples.len(), 15);
        let references: Vec<&StandardRef> = profile
            .rules
            .iter()
            .filter_map(|r| r.standard_reference.as_ref())
            .collect();
        assert_eq!(references.len(), 23);
        for reference in references {
            let matches = profile
                .standards
                .values()
                .filter(|d| {
                    d.standard == reference.standard
                        && d.version == reference.version
                        && d.role == reference.role
                })
                .count();
            assert_eq!(matches, 1, "{reference:?}");
        }
        assert_eq!(
            profile.standards["ISO29119-2"],
            StandardDefinition {
                standard: "ISO/IEC/IEEE 29119-2".to_owned(),
                version: "2021".to_owned(),
                role: MappingRole::SemanticAlignment,
                note: "Software test processes.".to_owned(),
            }
        );
    }

    #[test]
    fn unresolvable_standard_reference_is_rejected() {
        const RULE: &str = "PPMN.I0.PROVENANCE.AGENT_IDENTIFIED";
        for (field, value) in [
            ("standard", "OMG PPMN 2"),
            ("version", "9.9"),
            ("role", "taxonomy"),
        ] {
            let result =
                load_mutated(|v| rule(v, RULE)["standard_reference"][field] = Value::from(value));
            assert!(
                matches!(
                    &result,
                    Err(ValidationError::UnknownStandardReference { rule_id, .. }) if rule_id == RULE
                ),
                "{field}: {result:?}"
            );
        }
    }

    #[test]
    fn duplicate_standard_definition_is_rejected() {
        let result = load_mutated(|v| {
            let copy = v["standards"]["BPMN"].clone();
            v["standards"]["BPMN-AGAIN"] = copy;
        });
        assert_eq!(
            result,
            Err(ValidationError::DuplicateStandardDefinition {
                standard: "OMG BPMN".to_owned(),
                version: "2.0.2".to_owned(),
                role: MappingRole::Interchange,
            })
        );
        // The same standard and version under another role is a distinct definition.
        let result = load_mutated(|v| {
            let mut copy = v["standards"]["BPMN"].clone();
            copy["role"] = Value::from("taxonomy");
            v["standards"]["BPMN-TAXONOMY"] = copy;
        });
        assert_eq!(result.unwrap().standards.len(), 16);
    }

    #[test]
    fn unknown_mapping_vocabulary_and_standard_ref_fields_are_rejected() {
        const RULE: &str = "PPMN.I0.PROVENANCE.AGENT_IDENTIFIED";
        assert!(is_yaml_error(load_mutated(|v| {
            rule(v, RULE)["standard_reference"]["role"] = Value::from("marketing")
        })));
        assert!(is_yaml_error(load_mutated(|v| {
            v["standards"]["BPMN"]["role"] = Value::from("marketing")
        })));
        assert!(is_yaml_error(load_mutated(|v| {
            rule(v, RULE)["standard_reference"]["mapping_strength"] = Value::from("certified")
        })));
        assert!(is_yaml_error(load_mutated(|v| {
            rule(v, RULE)["standard_reference"]["clause_ref"] = Value::from("5.2")
        })));
        // Every field of an existing block is required.
        assert!(is_yaml_error(load_mutated(|v| {
            let reference = rule(v, RULE)["standard_reference"]
                .as_mapping_mut()
                .unwrap();
            reference.remove("mapping_strength");
        })));
        assert!(is_yaml_error(load_mutated(|v| {
            let definition = v["standards"]["BPMN"].as_mapping_mut().unwrap();
            definition.remove("note");
        })));
    }

    // ------------------------------------------------------------------ strictness (§46)

    #[test]
    fn unknown_fields_are_rejected_at_every_fixed_shape() {
        assert!(is_yaml_error(load_mutated(|v| {
            v["owner"] = Value::from("someone")
        })));
        assert!(is_yaml_error(load_mutated(|v| {
            gate(v, "F1")["owner"] = Value::from("someone")
        })));
        assert!(is_yaml_error(load_mutated(|v| {
            rule(v, "PLUMB.F1.REQ.GROUNDED")["owner"] = Value::from("someone")
        })));
        assert!(is_yaml_error(load_mutated(|v| {
            v["standards"]["BPMN"]["owner"] = Value::from("someone")
        })));
        assert!(is_yaml_error(load_mutated(|v| {
            v.as_mapping_mut().unwrap().remove("metamodel");
        })));
        assert!(is_yaml_error(load_profile_yaml("not: [a, profile")));
    }

    #[test]
    fn result_states_must_be_exactly_the_six_states() {
        let states = |v: &mut Value, list: &[&str]| {
            v["result_states"] = Value::Sequence(list.iter().map(|s| Value::from(*s)).collect());
        };
        // wrong case
        assert!(is_yaml_error(load_mutated(|v| {
            states(
                v,
                &["Pass", "FAIL", "WARN", "NOT_APPLICABLE", "WAIVED", "ERROR"],
            )
        })));
        // extra, unknown
        assert!(is_yaml_error(load_mutated(|v| {
            states(
                v,
                &[
                    "PASS",
                    "FAIL",
                    "WARN",
                    "NOT_APPLICABLE",
                    "WAIVED",
                    "ERROR",
                    "NOT_IMPLEMENTED",
                ],
            )
        })));
        // missing
        assert_eq!(
            load_mutated(|v| states(v, &["PASS", "FAIL", "WARN", "NOT_APPLICABLE", "WAIVED"])),
            Err(ValidationError::ResultStatesMismatch)
        );
        // duplicate in place of a missing state
        assert_eq!(
            load_mutated(|v| states(
                v,
                &["PASS", "FAIL", "WARN", "NOT_APPLICABLE", "WAIVED", "PASS"]
            )),
            Err(ValidationError::ResultStatesMismatch)
        );
        // duplicate in addition to all six
        assert_eq!(
            load_mutated(|v| states(
                v,
                &[
                    "PASS",
                    "FAIL",
                    "WARN",
                    "NOT_APPLICABLE",
                    "WAIVED",
                    "ERROR",
                    "ERROR"
                ]
            )),
            Err(ValidationError::ResultStatesMismatch)
        );
        // a valid declaration in another order is stored in contract order
        let reordered = load_mutated(|v| {
            states(
                v,
                &["ERROR", "WAIVED", "NOT_APPLICABLE", "WARN", "FAIL", "PASS"],
            )
        })
        .unwrap();
        assert_eq!(reordered.result_states, RuleResultState::ALL);
    }

    #[test]
    fn rule_classes_must_be_exactly_the_five_classes() {
        assert!(is_yaml_error(load_mutated(|v| {
            v["rule_classes"]["vendor_policy"] = Value::from("Vendor rule pack.")
        })));
        assert_eq!(
            load_mutated(|v| {
                v["rule_classes"]
                    .as_mapping_mut()
                    .unwrap()
                    .remove("organization_policy");
            }),
            Err(ValidationError::RuleClassesMismatch)
        );
        assert_eq!(
            load_mutated(|v| v["rule_classes"]["profile_policy"] = Value::from("")),
            Err(ValidationError::InvalidProfileText {
                field: "rule_classes.profile_policy".to_owned(),
                reason: "empty",
            })
        );
        assert!(is_yaml_error(load_mutated(|v| {
            rule(v, "PLUMB.F1.REQ.GROUNDED")["rule_class"] = Value::from("vendor_policy")
        })));
    }

    #[test]
    fn invalid_textual_metadata_is_rejected() {
        const RULE: &str = "PLUMB.F1.REQ.GROUNDED";
        let text_error = |result: Result<ValidationProfile, ValidationError>| match result {
            Err(ValidationError::InvalidProfileText { field, reason }) => (field, reason),
            other => panic!("expected InvalidProfileText, got {other:?}"),
        };
        for field in [
            "title",
            "applies_when",
            "check",
            "pass_condition",
            "finding_status_on_fail",
        ] {
            let (name, reason) =
                text_error(load_mutated(|v| rule(v, RULE)[field] = Value::from("")));
            assert_eq!((name, reason), (format!("rules.{RULE}.{field}"), "empty"));
        }
        assert_eq!(
            text_error(load_mutated(
                |v| rule(v, RULE)["remediation"] = Value::from("")
            ))
            .1,
            "empty"
        );
        assert_eq!(
            text_error(load_mutated(
                |v| rule(v, RULE)["title"] = Value::from("Grounded ")
            ))
            .1,
            "leading or trailing whitespace"
        );
        assert_eq!(
            text_error(load_mutated(
                |v| rule(v, RULE)["check"] = Value::from("a\tb")
            ))
            .1,
            "control character"
        );
        assert_eq!(
            text_error(load_mutated(|v| rule(v, RULE)["id"] = Value::from(" "))).0,
            "rules.id"
        );
        for field in ["profile_version", "metamodel", "status"] {
            assert_eq!(
                text_error(load_mutated(|v| v[field] = Value::from(""))),
                (field.to_owned(), "empty")
            );
            assert_eq!(
                text_error(load_mutated(|v| v[field] = Value::from(" draft"))).1,
                "leading or trailing whitespace"
            );
            assert_eq!(
                text_error(load_mutated(|v| v[field] = Value::from("two\nlines"))).1,
                "control character"
            );
        }
        assert_eq!(
            text_error(load_mutated(|v| gate(v, "F1")["purpose"] = Value::from(""))).0,
            "gates.F1.purpose"
        );
        assert_eq!(
            text_error(load_mutated(
                |v| gate(v, "F1")["pass_algorithm"] = Value::from("")
            ))
            .0,
            "gates.F1.pass_algorithm"
        );
        assert!(is_yaml_error(load_mutated(|v| {
            v["profile_id"] = Value::from("not an id")
        })));
    }

    #[test]
    fn invalid_standard_metadata_text_is_rejected() {
        const RULE: &str = "PPMN.I0.PROVENANCE.AGENT_IDENTIFIED";
        let text_error = |result: Result<ValidationProfile, ValidationError>| match result {
            Err(ValidationError::InvalidProfileText { field, reason }) => (field, reason),
            other => panic!("expected InvalidProfileText, got {other:?}"),
        };
        let bad_texts = [
            ("", "empty"),
            (" padded", "leading or trailing whitespace"),
            ("padded ", "leading or trailing whitespace"),
            ("a\tb", "control character"),
            ("two\nlines", "control character"),
        ];
        for field in ["standard", "version", "note"] {
            for (text, reason) in bad_texts {
                assert_eq!(
                    text_error(load_mutated(|v| {
                        v["standards"]["BPMN"][field] = Value::from(text)
                    })),
                    (format!("standards.BPMN.{field}"), reason)
                );
                assert_eq!(
                    text_error(load_mutated(|v| {
                        rule(v, RULE)["standard_reference"][field] = Value::from(text)
                    })),
                    (format!("rules.{RULE}.standard_reference.{field}"), reason)
                );
            }
        }
        for (key, reason) in bad_texts {
            let result = load_mutated(|v| {
                let standards = v["standards"].as_mapping_mut().unwrap();
                let definition = standards.remove("SYSML").unwrap();
                standards.insert(Value::from(key), definition);
            });
            assert_eq!(text_error(result), ("standards key".to_owned(), reason));
        }
        // Values are checked, never trimmed or rewritten.
        let profile = supplied();
        assert_eq!(profile.standards["JSONSCHEMA"].version, "2020-12");
        assert_eq!(
            profile.standards["RBAC"].note,
            "RBAC reference-model concepts: users, roles, permissions, operations, objects."
        );
    }

    #[test]
    fn embedded_newline_in_rule_text_is_kept_verbatim() {
        let profile = load_mutated(|v| {
            rule(v, "PLUMB.F1.REQ.GROUNDED")["check"] = Value::from("first line\nsecond line")
        })
        .unwrap();
        let registry = ValidationRegistry::new(profile).unwrap();
        assert_eq!(
            registry.rule("PLUMB.F1.REQ.GROUNDED").unwrap().check,
            "first line\nsecond line"
        );
    }

    // ------------------------------------------------------------------ hashes (§47)

    #[test]
    fn supplied_profile_hashes_match_independently_calculated_values() {
        let profile = supplied();
        let profile_hash = profile.profile_hash().unwrap();
        let rule_pack_hash = profile.rule_pack_hash().unwrap();
        assert_eq!(profile_hash.kind(), HashKind::Generic);
        assert_eq!(rule_pack_hash.kind(), HashKind::Generic);
        assert_eq!(profile_hash.as_str(), PROFILE_HASH);
        assert_eq!(rule_pack_hash.as_str(), RULE_PACK_HASH);
        assert_ne!(profile_hash, rule_pack_hash);
        assert_eq!(hashes(&supplied()), hashes(&profile));
    }

    #[test]
    fn profile_hashes_ignore_yaml_ordering_and_formatting() {
        let expected = (PROFILE_HASH.to_owned(), RULE_PACK_HASH.to_owned());
        // Re-serialized YAML: different formatting, same content.
        assert_eq!(hashes(&load_value(&supplied_value()).unwrap()), expected);
        let reordered_rules = load_mutated(|v| seq(v, "rules").reverse()).unwrap();
        assert_eq!(hashes(&reordered_rules), expected);
        let reordered_rule_ids = load_mutated(|v| {
            for gate in GateId::ALL {
                gate_rule_ids(v, gate.as_str()).reverse();
            }
        })
        .unwrap();
        assert_eq!(hashes(&reordered_rule_ids), expected);
        let reordered_gates = load_mutated(|v| seq(v, "gates").reverse()).unwrap();
        assert_eq!(hashes(&reordered_gates), expected);
        assert_eq!(reordered_gates, supplied());

        // The hashes normalize by themselves, even for a profile built out of order.
        let mut unnormalized: ValidationProfile = serde_yaml::from_str(SUPPLIED).unwrap();
        unnormalized.rules.reverse();
        unnormalized.gates.reverse();
        unnormalized.result_states.reverse();
        assert_eq!(hashes(&unnormalized), expected);
    }

    #[test]
    fn rule_severity_change_alters_both_hashes() {
        let changed =
            load_mutated(|v| rule(v, "PLUMB.F1.REQ.GROUNDED")["severity"] = Value::from("warn"))
                .unwrap();
        let (profile_hash, rule_pack_hash) = hashes(&changed);
        assert_ne!(profile_hash, PROFILE_HASH);
        assert_ne!(rule_pack_hash, RULE_PACK_HASH);
    }

    #[test]
    fn profile_status_change_alters_only_the_profile_hash() {
        let changed = load_mutated(|v| v["status"] = Value::from("released")).unwrap();
        let (profile_hash, rule_pack_hash) = hashes(&changed);
        assert_ne!(profile_hash, PROFILE_HASH);
        assert_eq!(rule_pack_hash, RULE_PACK_HASH);
    }

    #[test]
    fn catalog_note_change_alters_only_the_profile_hash() {
        let changed =
            load_mutated(|v| v["standards"]["BPMN"]["note"] = Value::from("Process interchange."))
                .unwrap();
        let (profile_hash, rule_pack_hash) = hashes(&changed);
        assert_ne!(profile_hash, PROFILE_HASH);
        assert_eq!(rule_pack_hash, RULE_PACK_HASH);
    }

    #[test]
    fn rule_standard_ref_note_change_alters_both_hashes() {
        let changed = load_mutated(|v| {
            rule(v, "PPMN.I0.PROVENANCE.AGENT_IDENTIFIED")["standard_reference"]["note"] =
                Value::from("Rule-specific provenance note.")
        })
        .unwrap();
        let (profile_hash, rule_pack_hash) = hashes(&changed);
        assert_ne!(profile_hash, PROFILE_HASH);
        assert_ne!(rule_pack_hash, RULE_PACK_HASH);
    }

    // ------------------------------------------------------------------ registry (§48)

    #[test]
    fn registry_profile_lookups_return_the_loaded_metadata() {
        let profile = supplied();
        let registry = ValidationRegistry::new(profile.clone()).unwrap();
        assert_eq!(registry.profile(), &profile);
        for rule in &profile.rules {
            assert_eq!(registry.rule(&rule.id), Some(rule));
        }
        assert_eq!(registry.rule("ORG.F1.REQ.UNKNOWN"), None);
        assert_eq!(registry.rule(""), None);
        for gate in &profile.gates {
            assert_eq!(registry.gate(gate.id), gate);
            let ids: Vec<&str> = registry
                .rules_for_gate(gate.id)
                .iter()
                .map(|r| r.id.as_str())
                .collect();
            assert_eq!(ids, gate.rule_ids);
            assert!(ids.windows(2).all(|w| w[0] < w[1]));
        }
    }

    #[test]
    fn registry_profile_is_the_only_copy_of_rule_metadata() {
        let registry = registry();
        let rules = &registry.profile().rules;
        for (index, rule) in rules.iter().enumerate() {
            assert!(std::ptr::eq(
                registry.rule(&rule.id).unwrap(),
                &rules[index]
            ));
        }
        for gate in GateId::ALL {
            assert!(std::ptr::eq(
                registry.gate(gate),
                &registry.profile().gates[gate.ordinal()]
            ));
            for rule in registry.rules_for_gate(gate) {
                assert!(rules.iter().any(|r| std::ptr::eq(r, rule)));
            }
        }
    }

    #[test]
    fn registry_profile_derivation_is_deterministic_and_validating() {
        let reordered = load_mutated(|v| {
            seq(v, "rules").reverse();
            seq(v, "gates").reverse();
        })
        .unwrap();
        assert_eq!(ValidationRegistry::new(reordered).unwrap(), registry());

        // A hand-built, unnormalized profile is normalized by the registry.
        let mut unnormalized: ValidationProfile = serde_yaml::from_str(SUPPLIED).unwrap();
        unnormalized.rules.reverse();
        assert_eq!(ValidationRegistry::new(unnormalized).unwrap(), registry());

        // An invalid one is rejected rather than indexed.
        let mut broken = supplied();
        broken.rules.pop();
        assert_eq!(
            ValidationRegistry::new(broken),
            Err(ValidationError::RuleCountMismatch {
                expected: 133,
                actual: 132
            })
        );
    }

    // ------------------------------------------------------------------ source guards (§§35, 49)

    /// The F0.12 metadata layer: model, loader and metadata registry.
    const PRODUCTION_SOURCES: [&str; 3] = [
        include_str!("../src/model.rs"),
        include_str!("../src/profile.rs"),
        include_str!("../src/registry.rs"),
    ];

    #[test]
    fn production_source_of_the_profile_crate_contains_no_rule_id() {
        let profile = supplied();
        let prefixes: BTreeMap<&str, usize> =
            profile.rules.iter().fold(BTreeMap::new(), |mut map, rule| {
                let prefix = rule.id.split('.').next().unwrap();
                *map.entry(prefix).or_default() += 1;
                map
            });
        for source in PRODUCTION_SOURCES {
            for rule in &profile.rules {
                assert!(!source.contains(rule.id.as_str()), "source has {}", rule.id);
            }
            // No rule-ID namespace either, so no per-rule constant or match arm can exist.
            for prefix in prefixes.keys() {
                assert!(
                    !source.contains(&format!("\"{prefix}.")),
                    "source has a {prefix}. rule literal"
                );
            }
        }
    }

    #[test]
    fn production_source_of_the_profile_crate_has_no_execution_types() {
        for source in PRODUCTION_SOURCES {
            for premature in [
                "ValidationContext",
                "Applicability",
                "RuleEvaluator",
                "GateReport",
                "struct ValidationArtifact",
                "struct RuleResult ",
                "struct Waiver ",
                "plumb_compiler",
                "fn evaluate",
            ] {
                assert!(!source.contains(premature), "source has {premature}");
            }
        }
        assert!(!include_str!("../Cargo.toml").contains("plumb-compiler"));
        // The external-validation contracts live in this crate and never reach back.
        let external = include_str!("../src/external.rs");
        assert!(!external.contains("plumb_compiler"));
        assert!(!external.contains("struct ValidationArtifact"));
    }
}
