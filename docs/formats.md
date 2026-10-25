# Plumb file formats

## `functional.yaml` v2 — compatibility projection

`functional.yaml` is a **compatibility output only**. The PSG is the canonical model; `functional.yaml` is generated from it by `plumb_functional::project_functional_v2(graph, profile)` and is never read back. There is no importer, no round trip and no automatic writeback: projected YAML must never be applied to the PSG automatically, and no projection produces a `SemanticPatch` or `Proposal`.

The machine schema is `schemas/functional-v2.schema.json`. Every generated document is validated against it at runtime before the projection succeeds.

### Document

The document has exactly these top-level fields, in this order. All thirteen collections are always present, including when empty; no other top-level field exists.

```yaml
version: 2
model_hash: "psg:sha256:<64 hex>"
glossary: []
actors: []
roles: []
entities: []
calendars: []
calculations: []
rules: []
operations: []
processes: []
events: []
requirements: []
scenarios: []
assumptions: []
```

- `version` is always `2`.
- `model_hash` is exactly the PSG semantic hash of the projected graph (`psg:sha256:`). The v2 plan's `fm:` hash was never defined, so the v3 projection records the PSG semantic hash instead of inventing a second hash system.
- Nested objects follow the v2 plan §4.1 shapes. Optional fields (`unit`, `precision`, `enum`, `precondition`, `when`, calendar `region` and `holidays_ref`, and the entity/operation `nfr` objects) are omitted when absent. Collections are sorted by ID; set-like reference lists are sorted; ordered content (`pre`, `post`, `enum`, `week_pattern`, `conditions`, `rows`, `given`) keeps its PSG order.
- The YAML bytes are exactly `serde_yaml`'s serialization of the typed document, with no post-processing.

### Companion projection metadata

Projection metadata is companion data returned next to the YAML. It is **never** inside `functional.yaml`.

```rust
pub struct ProjectionMetadata {
    pub source_semantic_hash: Hash,   // Semantic: the graph's semantic hash
    pub profile_hash: Hash,           // Generic: ValidationProfile::profile_hash()
    pub projection_version: u32,      // 2
    pub content_hash: Hash,           // Generic: SHA-256 of the exact YAML bytes
    pub warnings: Vec<ProjectionNotice>,
    pub lossy_mappings: Vec<ProjectionNotice>,
}

pub struct ProjectionNotice {
    pub code: String,
    pub source_refs: Vec<Id>,   // sorted, unique
    pub message: String,
}
```

Notices carry no timestamp and are sorted by `(code, source_refs, message)`. `warnings` record content that was omitted or could not be represented; `lossy_mappings` record a deterministic collapse that was projected. Neither list affects the YAML bytes or `content_hash`.

### Accepted-only view

Only `Accepted` nodes and edges contribute; `functional.yaml` is the confirmed legacy model.

- `Proposed` and `Rejected` elements are ignored without any notice.
- `Suspect`, `Superseded` and `Deprecated` elements of a projected type, and such edges of a projected relation, are omitted with `V2_NON_ACCEPTED_BASELINE_OMITTED`.
- Associations come only from typed payload fields, typed `Accepted` relations and typed relation properties, never from names, ID prefixes, text, array order or capitalization. Views are never projected, so layout and style never change the output.
- Nothing is fabricated: a required legacy field that typed PSG cannot supply means the legacy object is omitted with a notice, unless a collapse below is defined.

### Roles and security

- **BusinessRole** → one role whose `actor` is the BusinessRole's own ID (a projection-only self-token) and whose `operations` are the operations it performs. Recorded as `V2_BUSINESS_ROLE_COLLAPSE`. The token never creates a PSG Actor and asserts nothing.
- **SecurityRole** → one role whose `operations` are reached through `grants` → Permission → `permits`. `actor` is the single Actor assigned through `assigned_role`; with zero or several, the SecurityRole's own ID is used (`V2_SECURITY_ROLE_COLLAPSE`), never an arbitrary choice.
- **Permission** scope and conditions cannot be represented; every such permission records `V2_PERMISSION_SCOPE_OMITTED`. The legacy role/operation matrix is display compatibility only; the PSG stays authoritative for security semantics.

### Quality scenarios

A QualityScenario becomes a legacy NFR value only when it has exactly one accepted characteristic, exactly one affected Entity or Operation, and the characteristic name equals a legacy key exactly: `consistency`, `availability`, `volatility`, `residency`, `retention` for an entity; `latency_ms` (number), `audit` (boolean), `idempotent` (boolean) for an operation. Even then `V2_QUALITY_SCENARIO_COLLAPSE` records that stimulus, response, environment, source, priority and measure are dropped. Other scenarios are omitted with `V2_QUALITY_SCENARIO_OMITTED`. Equal values for one target and key are emitted once; different values fail the projection (`ConflictingLegacyNfr`). `Operation.idempotency` `"true"`/`"false"` feeds `nfr.idempotent` under the same rule.

### Notice codes

| Code | List | `source_refs` |
|---|---|---|
| `V2_NON_ACCEPTED_BASELINE_OMITTED` | warnings | the element (plus a Transition whose only trigger edge it is) |
| `V2_GLOSSARY_DEFINITION_MISSING` | warnings | Term |
| `V2_ORGANIZATION_ACTOR_COLLAPSE` | lossy | Actor |
| `V2_BUSINESS_ROLE_COLLAPSE` | lossy | BusinessRole |
| `V2_SECURITY_ROLE_COLLAPSE` | lossy | SecurityRole and its assigned Actors |
| `V2_PERMISSION_SCOPE_OMITTED` | lossy | SecurityRole, Permission, ResourceScopes, PolicyConditions |
| `V2_ATTRIBUTE_NULLABILITY_OMITTED` | lossy | Attribute |
| `V2_RELATIONSHIP_DETAIL_OMITTED` | lossy | DomainRelationship |
| `V2_EVENT_TRIGGER_TRANSITION_OMITTED` | warnings | Transition, Event |
| `V2_OPERATION_PERFORMER_MISSING` | warnings | Operation (with the Transition or ProcessNode that needed it) |
| `V2_MULTIPLE_PERFORMERS_COLLAPSED` | lossy | the Operation or ProcessNode and all performers |
| `V2_ENTITY_LEVEL_READ_WRITE_OMITTED` | warnings | Operation and the Entity targets |
| `V2_OUTCOME_KIND_COLLAPSE` | lossy | Outcome |
| `V2_IDEMPOTENCY_UNREPRESENTABLE` | warnings | Operation |
| `V2_CALCULATION_TARGET_UNREPRESENTABLE` | warnings | Calculation |
| `V2_RULE_TABLE_UNREPRESENTABLE` | warnings | Rule, or a DecisionTable with a non-object input or row |
| `V2_DECISION_TABLE_DETAIL_OMITTED` | lossy | DecisionTable |
| `V2_EVENT_PAYLOAD_UNREPRESENTABLE` | warnings | Event and its payload schema reference |
| `V2_ACCEPTANCE_CRITERION_UNMAPPED` | warnings | AcceptanceCriterion |
| `V2_PROCESS_TRIGGER_UNREPRESENTABLE` | warnings | Process and its start nodes |
| `V2_PROCESS_STEP_OPERATION_MISSING` | warnings | ProcessNode |
| `V2_QUALITY_SCENARIO_COLLAPSE` | lossy | QualityScenario and its legacy target |
| `V2_QUALITY_SCENARIO_OMITTED` | warnings | QualityScenario |
| `V2_SCENARIO_MULTI_REQUIREMENT_COLLAPSE` | lossy | Scenario and all its requirements |
| `V2_SCENARIO_REQUIREMENT_MISSING` | warnings | Scenario (it has no requirement and is omitted) |
| `V2_SCENARIO_OPERATION_MISSING` | warnings | Scenario |
| `V2_SCENARIO_THEN_CONFLICT` | warnings | Scenario |
| `V2_ASSUMPTION_UNREPRESENTABLE` | warnings | Assumption |

No other code exists. An accepted Scenario whose `given` or `then` contains a non-object entry cannot be carried by any code and fails the projection (`UnrepresentableElement`) rather than being dropped silently.

### Known compatibility limits

- **Calculations** are never projected: no PSG field or core relation identifies the legacy target attribute.
- **Generic Rules** are never projected: condition tables are not fabricated from expressions. DecisionTables with object inputs and rows are projected as `conditions`/`rows`.
- **Acceptance criteria** stay empty: no core relation attaches an AcceptanceCriterion to a Requirement.
- **Event-triggered transitions** are omitted: the legacy transition names an operation.
- **Scenario runs** are omitted: no core relation links a Scenario to a ScenarioRun.
- **Attribute classification** outside `pii`, `financial`, `confidential`, `none` is not remapped and fails schema validation.

### Schema dialect

`schemas/functional-v2.schema.json` declares JSON Schema draft 2020-12 and is compiled and evaluated with the draft 2020-12 validator of the workspace `jsonschema` 0.18 (built with its `draft202012` feature). No reference is resolved over the network.

## CLI configuration — ID-free stage configuration (selectors)

The `--config` file (one strict JSON object, duplicate keys rejected; compiler architecture
§33.9) carries the stage inputs. Several of them name PSG elements that earlier stages generate
(Entities, Attributes, Operations, Calculations, Scenarios, Requirements, ...), whose IDs cannot
be known before those stages run. Any ID position of the values below may therefore hold a
**selector** instead of an ID:

| Field | Selectable |
|---|---|
| `operation_scope`, `process_scope`, `calculation_scope`, `authorization_draft`, `scenario_derivation`, `verification_applicability`, `gates.f4` | the whole value |
| `expression_scopes`, `calculation_scope_overrides`, `runtime_semantic_values` | each entry |

Selector forms (an object with exactly one key):

- `{"$one": QUERY}` is the ID of exactly one matching element.
- `{"$all": QUERY}` is the sorted list of every matching element. As the value of an ID list it is
  that list; as an element of an ID list its IDs are spliced in, next to explicit IDs.

`QUERY` is `{"type": NodeType | [NodeType, ...], "where": {field: value, ...}}` (`where` is
optional): the **Accepted** nodes of those types whose payload `data` fields equal the given JSON
values exactly (for example `name`, `source_identifier`, `confirmation_status`). Nothing else is
matched: no text search, prefixes or inference. A position that admits only certain node types
(for example `operation_scope.domain_refs`: Entity or Attribute; `gates.f4.in_scope_scenario_refs`:
Scenario) rejects a selector of another type.

```json
"calculation_scope": {
  "bindings": [
    {"node_ref": {"$one": {"type": "Attribute", "where": {"name": "start date"}}},
     "symbol": "start_date", "exposure": {"kind": "root"}}
  ],
  "calendar_refs": {"$all": {"type": "Calendar"}}
},
"operation_scope": {
  "performer_refs": {"$all": {"type": ["BusinessRole", "Actor"]}},
  "domain_refs": {"$all": {"type": ["Entity", "Attribute"]}},
  "calculation_refs": [], "governor_refs": {"$all": {"type": "Rule"}},
  "event_refs": [], "data_schema_refs": [], "event_payload_attribute_refs": [], "support_refs": []
},
"gates": {
  "f1": {"lint_policy": {"severity_overrides": {}}},
  "f4": {
    "in_scope_scenario_refs": {"$all": {"type": "Scenario", "where": {"confirmation_status": "Confirmed"}}},
    "required_scenario_kinds": [
      {"source_ref": {"$one": {"type": "Operation", "where": {"name": "SubmitLeaveRequest"}}},
       "required_scenario_kinds": []}
    ],
    "visible_assumption_refs": []
  }
}
```

Semantics:

- **Explicit form unchanged.** A value without selectors is decoded strictly as before and its
  stage inputs, job IDs and request hashes are exactly those of earlier releases.
- **Resolved when the stage runs.** Selectors are resolved against the graph the stage reads (the
  captured base revision of a recipe, the input revision of each phase, the revision a gate or
  scenario run evaluates). Resolution is a pure function of that graph: the same graph always
  selects the same IDs, and the stage builds its inference requests from the resolved typed value,
  so a selector that selects the IDs an explicit configuration names produces the same requests
  and replay request hashes.
- **Recorded.** The stage inputs (`config_json` of the recipe job and each child
  `CompileContext.config`) hold the resolved value under the field's usual key and, only when a
  selector is configured, the configured form under `inputs.selections.<field>`.
- **Errors.** A malformed selector, a selector of an inadmissible node type, or a selector form
  that would not have its typed shape is `E_RUNTIME_CONFIG_INVALID` naming the field, before any
  store opens (an explicit value is decoded strictly when the file is read, as before). A `$one`
  that matches several elements, or no element, is `E_STAGE_CONFIG_UNRESOLVED` (422, details
  `{field, path, kind, selector, matches}`): a recipe is rejected when it is captured, before any
  job is queued; a gate or scenario run that reads the value fails with the same error. The one
  exception mirrors the explicit form, where an entry naming an absent element is ignored: in the
  keyed entry lists (`expression_scopes`, `calculation_scope_overrides`, `runtime_semantic_values`
  entries, `verification_applicability.sources`, and `gates.f4` `in_scope_scenario_refs`,
  `required_scenario_kinds`, `visible_assumption_refs`) an entry whose `$one` matches nothing is
  left out. Resolved values are canonicalized where their consumer requires an order
  (`scenario_derivation` entries); a resolved duplicate where uniqueness is required (an
  expression-scope owner, a verification source) is `E_STAGE_CONFIG_UNRESOLVED`.
- **Reserved keys.** Object keys starting with `$` are reserved for selectors inside these values.
