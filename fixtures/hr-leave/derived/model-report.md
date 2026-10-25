# Model Report

## Baseline

- Project ID: `project:hr`
- Profile ID: `profile:plumb-software-2026.1`
- Semantic hash: `psg:sha256:06937019ab7406cb17872b23d10107772d89af0f9fe8e99ebf55c8e3f28f4e75`
- Evidence hash: `ev:sha256:2fd981c26f92d9a965a7d3afa6d5a65f13fb4f4ad8d154e2a1c91734e6d57ac4`
- Profile hash: `sha256:e6dc73ec3f1847e5e7fc5d71aef64b71c15a3e93838e64c7f246f275c65af055`
- Rule-pack hash: `sha256:b3a48aa0d0dc47b612697bbf35fa778499509343f74de43459fbcc145e21d33e`

## F4 Gate

- Result: `PASS`
- Gate-report content hash: `sha256:9a9914f636ba800c77f7b60194ebd7dbb9cfa77e1f5f61d9c6ed6506ce638321`
- Blocker failures: 0
- Blocker waivers: 0
- Warnings: 0
- Generated findings: 0
- Applied waivers: 0

## Accepted Model Counts

| Node type | Accepted |
| --- | ---: |
| Actor | 3 |
| Agent | 3 |
| Attribute | 4 |
| BusinessRole | 2 |
| Calculation | 1 |
| Calendar | 1 |
| Concept | 17 |
| DecisionTable | 1 |
| Entity | 6 |
| Event | 5 |
| EvidenceFragment | 38 |
| Finding | 6 |
| Operation | 6 |
| Outcome | 9 |
| Question | 4 |
| Requirement | 32 |
| ResolutionDecision | 132 |
| Rule | 3 |
| Scenario | 7 |
| ScenarioRun | 7 |
| SourceArtifact | 1 |
| Stakeholder | 3 |
| State | 6 |
| Term | 17 |
| Transition | 5 |

- Accepted edges: 73

## Functional Compatibility Projection

- functional.yaml content hash: `sha256:2278ac636a0e37b144d85bd4b8087b4a1d66cbe3862eb2e3860c97127a80b337`
- Projection version: 2
- Warnings: 34
- Lossy mappings: 10

### Warnings

- `V2_CALCULATION_TARGET_UNREPRESENTABLE` (`calculation:b20b0ff4a57b833f`): Calculation target is not representable.
- `V2_ENTITY_LEVEL_READ_WRITE_OMITTED` (`entity:e3da53d2646ed9d4`, `operation:30e3f63d799f6f1b`): Entity-level read or write omitted.
- `V2_ENTITY_LEVEL_READ_WRITE_OMITTED` (`entity:e3da53d2646ed9d4`, `operation:58a5e002de427674`): Entity-level read or write omitted.
- `V2_ENTITY_LEVEL_READ_WRITE_OMITTED` (`entity:e3da53d2646ed9d4`, `operation:6c542729efadf821`): Entity-level read or write omitted.
- `V2_ENTITY_LEVEL_READ_WRITE_OMITTED` (`entity:e3da53d2646ed9d4`, `operation:7e6f4955e26661a1`): Entity-level read or write omitted.
- `V2_ENTITY_LEVEL_READ_WRITE_OMITTED` (`entity:e3da53d2646ed9d4`, `operation:e80512cc10433bdd`): Entity-level read or write omitted.
- `V2_ENTITY_LEVEL_READ_WRITE_OMITTED` (`entity:e3da53d2646ed9d4`, `operation:f463e7c912d3f774`): Entity-level read or write omitted.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:0b938f2fd10972d8`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:1b97e63d46bf92bb`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:2b0813dd94bc390e`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:2f498df5cf2d4987`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:42ecef08edd26d5c`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:4c07a4479e834698`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:777193ff6dfbefbd`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:8392b3b7e690d277`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:86bd652e5aea87e1`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:8b97f064c1a5555a`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:9082a6c671f2a68f`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:96e7743f8fbb64ca`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:a66d5c2e5ebd823e`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:aec7ef2e5a483f22`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:d51202b364b70694`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:e6a648aae4fbab71`): Term has no accepted concept definition.
- `V2_GLOSSARY_DEFINITION_MISSING` (`term:f2c6d0ac5bc98c1f`): Term has no accepted concept definition.
- `V2_RULE_TABLE_UNREPRESENTABLE` (`rule:annual-leave-eligibility`): Rule is not representable as a legacy table.
- `V2_RULE_TABLE_UNREPRESENTABLE` (`rule:manager-decision-authorization`): Rule is not representable as a legacy table.
- `V2_RULE_TABLE_UNREPRESENTABLE` (`rule:overlap`): Rule is not representable as a legacy table.
- `V2_SCENARIO_OPERATION_MISSING` (`scenario:5269fc4f3d07fd3f`): Scenario has no when.operation; omitted.
- `V2_SCENARIO_OPERATION_MISSING` (`scenario:eee46713b63bef85`): Scenario has no when.operation; omitted.
- `V2_SCENARIO_REQUIREMENT_MISSING` (`scenario:6b967c24cfc79179`): Scenario has no requirement; omitted.
- `V2_SCENARIO_REQUIREMENT_MISSING` (`scenario:8158d8131e8828c9`): Scenario has no requirement; omitted.
- `V2_SCENARIO_REQUIREMENT_MISSING` (`scenario:81c869d2fda66e0b`): Scenario has no requirement; omitted.
- `V2_SCENARIO_REQUIREMENT_MISSING` (`scenario:b260626972172584`): Scenario has no requirement; omitted.
- `V2_SCENARIO_REQUIREMENT_MISSING` (`scenario:cf9fefa10f7c9482`): Scenario has no requirement; omitted.

### Lossy mappings

- `V2_ATTRIBUTE_NULLABILITY_OMITTED` (`attr:2eaadedb6f14fc4f`): Attribute nullability is not representable.
- `V2_ATTRIBUTE_NULLABILITY_OMITTED` (`attr:b4093112d1db9703`): Attribute nullability is not representable.
- `V2_ATTRIBUTE_NULLABILITY_OMITTED` (`attr:ca89055c5564138e`): Attribute nullability is not representable.
- `V2_ATTRIBUTE_NULLABILITY_OMITTED` (`attr:fcd9667aa4033ebe`): Attribute nullability is not representable.
- `V2_BUSINESS_ROLE_COLLAPSE` (`brole:employee`): Business role projected with itself as actor token.
- `V2_BUSINESS_ROLE_COLLAPSE` (`brole:manager`): Business role projected with itself as actor token.
- `V2_DECISION_TABLE_DETAIL_OMITTED` (`dt:annual-eligibility`): Decision table hit policy and outputs omitted.
- `V2_OUTCOME_KIND_COLLAPSE` (`outcome:780cce7c8999f36c`): Outcome kind collapsed to failure.
- `V2_OUTCOME_KIND_COLLAPSE` (`outcome:afe136c96f37e636`): Outcome kind collapsed to failure.
- `V2_OUTCOME_KIND_COLLAPSE` (`outcome:ddb162a78d01f0b8`): Outcome kind collapsed to failure.

## Scenarios

- Accepted Scenarios: 7
- Confirmed Scenarios: 7
- Derived Scenarios: 0
- Other/unspecified confirmation status: 0

## Decisions

- Accepted ResolutionDecisions: 132

## Readiness

Readiness is advisory only and is not calculated by S4.7. It does not alter the F4 gate result.
