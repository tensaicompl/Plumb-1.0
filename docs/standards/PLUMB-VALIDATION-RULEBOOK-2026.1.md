# Plumb Software Engineering Profile 2026.1 — Validation Rulebook

**Status:** implementation draft 1  
**Metamodel:** Plumb Metamodel v3.0-draft.1  
**Profile ID:** `profile:plumb-software-2026.1`  
**Purpose:** make the Plumb v3 metamodel executable through deterministic, auditable engineering gates while distinguishing standards alignment from Plumb-specific semantic rigor.

---

## 1. Normative position

Plumb uses recognized standards as a **semantic and interoperability spine**, not as a marketing label. The profile distinguishes:

1. **External-standard rules** — operationalize concepts from ISO/IEC/IEEE or INCITS/NIST standards. Unless Plumb has verified full clause-level coverage and applicable licensing/conformance requirements, the product reports **aligned/compatible**, not “certified” or “ISO compliant”.
2. **External-interoperability rules** — validate generated/imported artifacts against specifications such as BPMN, DMN, OpenAPI, AsyncAPI and Arazzo, usually as a declared subset or exact document-format conformance target.
3. **Plumb core rules** — stricter semantic invariants that make the graph executable, deterministic, traceable and safe for downstream architecture and coding agents.
4. **Profile/organization rules** — optional overlays for regulated sectors or company policy.

A gate is therefore a **proof obligation over the PSG**, not a readiness percentage.

## 2. Gate evaluation model

Every rule returns one of: `PASS`, `FAIL`, `WARN`, `NOT_APPLICABLE`, `WAIVED`, `ERROR`.

A gate passes when **every applicable blocking rule passes**, except a `WAIVED` result may satisfy the project gate only when (a) the rule explicitly allows waiver, (b) the active standards/profile policy allows it, and (c) a `ResolutionDecision` records owner, scope, rationale and review/expiry when required. A project waiver never silently converts into external-standard conformance.

`ERROR` means Plumb could not evaluate the rule and therefore a blocking rule is treated as not proven. `NOT_APPLICABLE` is permitted only when the applicability predicate is deterministic and recorded in the gate report.

### 2.1 Severity

- `blocker` — prevents the owning gate from passing.
- `error` — must be resolved for normal profile completion; active profile MAY promote it to blocker.
- `warn` — engineering weakness or incomplete best practice; does not block by default.
- `info` — trace/advisory only.

### 2.2 Rule identity and deterministic findings

Finding identity SHOULD be deterministic from:

```text
finding_key = sha256(rule_id || sorted(target_ids) || semantic_condition_key)
```

Re-running validation against the same semantic graph therefore upserts the same finding rather than generating duplicates.

### 2.3 Waivers

Waiver policies:

- `forbidden` — cannot be waived in the profile; model must change or element must leave scope.
- `decision_required` — may be waived only via accepted `ResolutionDecision`.
- `profile_allow` — may be waived only if active project/industry profile explicitly enables that waiver class.

The conformance report MUST list every waiver separately from passes.

## 3. Standards baseline

| Area | Baseline | Plumb role |
|---|---|---|
| ISO29148 | ISO/IEC/IEEE 29148 2018 | semantic_alignment |
| ISO25010 | ISO/IEC 25010 2023 | taxonomy |
| ISO42010 | ISO/IEC/IEEE 42010 2022 | semantic_alignment |
| ISO12207 | ISO/IEC/IEEE 12207 2026 | semantic_alignment |
| ISO15289 | ISO/IEC/IEEE 15289 2019 | semantic_alignment |
| ISO29119-2 | ISO/IEC/IEEE 29119-2 2021 | semantic_alignment |
| BPMN | OMG BPMN 2.0.2 | interchange |
| DMN | OMG DMN 1.6 | interchange |
| SYSML | OMG SysML 2.0 | interchange |
| PPMN | OMG PPMN 1.0 | interchange |
| RBAC | INCITS 359 2012 | semantic_alignment |
| OPENAPI | OpenAPI Specification 3.2.1 | interchange |
| ASYNCAPI | AsyncAPI Specification 3.0.0 | interchange |
| ARAZZO | Arazzo Specification 1.1.0 | interchange |
| JSONSCHEMA | JSON Schema 2020-12 | interchange |

No copyrighted normative standard text is embedded in the rule pack. `clause_ref` may be added when Plumb has licensed/verified clause-level mapping.

## 4. Gate dependency chain

```text
I0 → F1 → F2 → F3 → F4 → Q1 → A1 → A2 → A3 → A4 → D1 → D2 → C1
```

Projects MAY run gates out of order for diagnostics, but a formal baseline cannot claim a later gate unless all mandatory predecessor gates for that profile are satisfied.

## I0 — Evidence corpus is reproducible and addressable.

**Pilot evaluation conventions.** `PLUMB.I0.SOURCE.CONTENT_ADDRESSED`, `PLUMB.I0.EVIDENCE.LOCATABLE`, `PLUMB.I0.EVIDENCE.HASH_MATCH` and `PLUMB.I0.SOURCE.PARSE_STATUS` are universal checks: with no applicable element they pass with no targets. Content addressing applies to every baseline source; locator, hash-replay and parse-status checks apply to the built-in import kinds `markdown`, `plain_text` and `docx` and ignore other source kinds. `PPMN.I0.PROVENANCE.AGENT_IDENTIFIED` is the only rule that is not applicable on an empty set. `PLUMB.I0.BASELINE.HASHABLE` always applies and requires exactly one canonical evidence manifest.

**Rules:** 6 total; **5 blocker(s)** in the default profile.

### `PLUMB.I0.SOURCE.CONTENT_ADDRESSED` — Every source artifact is content-addressed

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each SourceArtifact has a stable content_hash over the stored source bytes or normalized connector snapshot.
- **Pass condition:** No in-scope SourceArtifact lacks content_hash or stored content/snapshot.
- **Waiver:** `forbidden`

### `PLUMB.I0.EVIDENCE.LOCATABLE` — Evidence fragments are locatable

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every EvidenceFragment resolves to an existing SourceArtifact and a valid locator/range.
- **Pass condition:** Every EvidenceFragment resolves to an existing SourceArtifact and a valid locator/range.
- **Waiver:** `forbidden`

### `PLUMB.I0.EVIDENCE.HASH_MATCH` — Evidence fragment hashes match extracted content

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Re-reading the fragment locator yields the same content_hash after canonical text extraction.
- **Pass condition:** Re-reading the fragment locator yields the same content_hash after canonical text extraction.
- **Waiver:** `forbidden`

### `PLUMB.I0.SOURCE.PARSE_STATUS` — Required sources are parseable enough for evidence extraction

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** SourceArtifact is included in the active gate scope.
- **Deterministic check:** Source parse_status is complete or partial-with-explicit-unparsed-regions; fatal parse failures are absent.
- **Pass condition:** Source parse_status is complete or partial-with-explicit-unparsed-regions; fatal parse failures are absent.
- **Waiver:** `decision_required`
- **Typical remediation:** Repair/import with another adapter or explicitly exclude the source from scope by ResolutionDecision.

### `PPMN.I0.PROVENANCE.AGENT_IDENTIFIED` — Provenance-producing agent is identified

- **Class:** `external_interop`
- **Severity:** `error`
- **Source/mapping:** OMG PPMN 1.0 (compatible; interchange)
- **Applies when:** A derivation, import or human action created semantic output.
- **Deterministic check:** DerivationRecord references a known Agent and activity kind.
- **Pass condition:** DerivationRecord references a known Agent and activity kind.
- **Waiver:** `profile_allow`
- **Pilot interpretation:** the Agent of a `DerivationRecord` node is its envelope `audit.created_by`, which must resolve to a baseline `Agent` node; the activity kind is `DerivationRecord.kind`. Evidence acquisition (source import) is not a derivation, so a baseline without `DerivationRecord` nodes makes this rule `NOT_APPLICABLE`, never a vacuous `PASS`.

### `PLUMB.I0.BASELINE.HASHABLE` — Evidence baseline is reproducibly hashable

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Evidence hash can be recomputed from scoped sources and equals the baseline evidence_hash.
- **Pass condition:** Evidence hash can be recomputed from scoped sources and equals the baseline evidence_hash.
- **Waiver:** `forbidden`
- **Pilot interpretation:** the recomputed value is the graph's existing evidence hash; the expected value is the baseline evidence hash supplied with the evaluation, and a supplied canonical evidence manifest must match both and the graph exactly. A mismatch, or a missing or stale manifest, fails the rule; it is not a context error.

## F1 — Requirements are grounded, interpretable and free of unresolved blocking quality defects.

**Rules:** 11 total; **7 blocker(s)** in the default profile.

### `ISO29148.F1.REQ.STATEMENT_PRESENT` — Accepted requirements have an explicit statement

- **Class:** `external_standard`
- **Severity:** `blocker`
- **Source/mapping:** ISO/IEC/IEEE 29148 2018 (compatible; semantic_alignment)
- **Applies when:** Requirement status = Accepted.
- **Deterministic check:** Requirement.statement is non-empty after normalization.
- **Pass condition:** Requirement.statement is non-empty after normalization.
- **Waiver:** `forbidden`

### `PLUMB.F1.REQ.GROUNDED` — Accepted requirements are grounded in evidence or explicit human origin

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** At least one evidenced_by/derived_from path reaches EvidenceFragment, or the requirement carries an explicit human-origin ResolutionDecision.
- **Pass condition:** At least one evidenced_by/derived_from path reaches EvidenceFragment, or the requirement carries an explicit human-origin ResolutionDecision.
- **Waiver:** `decision_required`
- **Pilot evaluation note:** Canonical `Node.evidence` is direct grounding equivalent to a direct `evidenced_by` provenance relationship for this rule; redundant edge materialization is not required. Accepted `evidenced_by` edges, cycle-protected Accepted `derived_from` paths to an Accepted EvidenceFragment and a governed `human_requirement_origin` decision also ground a requirement.
- **Typical remediation:** Attach source evidence or record the requirement as an explicit human-origin decision.

### `ISO29148.F1.REQ.LINEAGE` — Requirements have intent lineage

- **Class:** `external_standard`
- **Severity:** `error`
- **Source/mapping:** ISO/IEC/IEEE 29148 2018 (compatible; semantic_alignment)
- **Applies when:** Requirement is not a root stakeholder need.
- **Deterministic check:** Requirement addresses/refines/decomposes from at least one Goal, Need, Concern, parent Requirement, or explicit origin decision.
- **Pass condition:** Requirement addresses/refines/decomposes from at least one Goal, Need, Concern, parent Requirement, or explicit origin decision.
- **Waiver:** `profile_allow`
- **Pilot evaluation note:** Accepted stakeholder-level requirements are treated as possible roots and excluded. The representable pilot lineage forms are: `addresses` to an Accepted Goal or Concern, `refines` to an Accepted parent Requirement, an incoming `decomposes_to` from an Accepted parent Requirement, `derived_from` to an Accepted Need, Goal or Concern (the representable Need form), or a governed `human_requirement_origin` decision. Source provenance such as `plumb_functional:segment_origin` is not intent lineage.

### `PLUMB.F1.REQ.TYPE_KNOWN` — Requirement kind and level are known

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** requirement_kind and level are set to registered values.
- **Pass condition:** requirement_kind and level are set to registered values.
- **Waiver:** `decision_required`
- **Pilot evaluation note:** The typed v3 `RequirementKind` and `RequirementLevel` enums make unknown values structurally unrepresentable in a valid PSG, so every Accepted Requirement satisfies this rule.
- **Typical remediation:** Classify as functional, quality, interface, constraint, data, security, etc., and set stakeholder/system/software/interface level.

### `PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED` — Accepted requirement set has no unresolved semantic duplicates

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** No open blocking duplicate finding links two accepted requirements with equivalent obligation semantics.
- **Pass condition:** No open blocking duplicate finding links two accepted requirements with equivalent obligation semantics.
- **Waiver:** `decision_required`
- **Detection note:** The deterministic configured lexical detector identifies unresolved duplicate candidates among same-scope Accepted Requirements; an open finding blocks F1 until governed resolution establishes that the requirements are merged, superseded, waived/declared distinct, or otherwise no longer form an active duplicate pair.

### `PLUMB.F1.REQ.NO_CONTRADICTION` — Accepted requirements have no unresolved blocking contradiction

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** No open blocking conflict finding exists between accepted requirements in the same scope.
- **Pass condition:** No open blocking conflict finding exists between accepted requirements in the same scope.
- **Waiver:** `decision_required`
- **Pilot evaluation note:** The evaluator checks current open contradiction Finding material; contradiction discovery is an upstream concern and is not performed by validation.

### `PLUMB.F1.REQ.TERMS_RESOLVED` — Requirement terms needed for interpretation resolve to concepts

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** All terms classified as semantic dependencies resolve to accepted Term/Concept nodes or are explicitly declared external identifiers.
- **Pass condition:** All terms classified as semantic dependencies resolve to accepted Term/Concept nodes or are explicitly declared external identifiers.
- **Waiver:** `decision_required`
- **Pilot evaluation note:** Validation consumes neutral F1 vocabulary dependency input (normalized key, current-statement range and resolution) produced upstream; each dependency resolves to an Accepted Term, an Accepted Concept or a governed external identifier decision, and active vocabulary conflict findings also fail the rule. Validation performs no second normalization.

### `PLUMB.F1.REQ.MODALITY_EXPLICIT` — Normative requirement modality is explicit

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Requirement expresses an obligation, prohibition or option.
- **Deterministic check:** Requirement obligation strength is classified and contradictions in modality are absent.
- **Pass condition:** Requirement obligation strength is classified and contradictions in modality are absent.
- **Waiver:** `profile_allow`
- **Pilot evaluation note:** The primary controlling obligation strength must be explicit and agree with the typed `Requirement.modality`. The statement is tokenized into ASCII word runs; the first token among shall, should, may, must and can controls, exactly as in S1.1 requirement compilation, and is negated only by an immediately following `not` separated by spaces, tabs, CR or LF. shall, shall not, should and may are the supported readings; should not, may not, must, must not, can and can not are unsupported; `cannot` is not a candidate token. A missing, unsupported or mismatching controlling reading fails the rule. Modal words after the controlling reading are not evaluated, so secondary-clause contradictions are not inferred by this evaluator and must come through deterministic conflict finding material.

### `PLUMB.F1.REQ.CRITERIA_FOR_BEHAVIOR` — Behavioral requirements have acceptance criteria or executable semantics

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Requirement kind is functional/interface/security behavior and is in implementation scope.
- **Deterministic check:** Requirement has AcceptanceCriterion, specified_by links, or an explicit deferred-verification decision.
- **Pass condition:** Requirement has AcceptanceCriterion, specified_by links, or an explicit deferred-verification decision.
- **Waiver:** `decision_required`
- **Pilot evaluation note:** The NOW representation is an Accepted AcceptanceCriterion `derived_from` the Requirement, a Requirement `specified_by` an Accepted Operation, Process, Rule or QualityScenario, or a governed `deferred_verification` decision; no relation is invented. The pilot scope is functional, interface and security requirements.

### `PLUMB.F1.REQ.QUALITY_FINDINGS_CLEAR` — No unresolved high-severity requirement-quality findings

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** All requirement-quality findings marked blocker/error by active profile are resolved or validly waived.
- **Pass condition:** All requirement-quality findings marked blocker/error by active profile are resolved or validly waived.
- **Waiver:** `decision_required`
- **Pilot evaluation note:** plumb-lint Error diagnostics are high severity; Warn and Info are non-blocking. The UNDEFINED_TERM lint is excluded from this rule because TERMS_RESOLVED is canonical for vocabulary.

### `PLUMB.F1.REQ.SUPERSEDED_EXCLUDED` — Superseded requirements do not count as active obligations

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Superseded requirements are excluded from gate scope, coverage denominators and generated contracts.
- **Pass condition:** Superseded requirements are excluded from gate scope, coverage denominators and generated contracts.
- **Waiver:** `forbidden`
- **Pilot evaluation note:** A requirement targeted by an Accepted `supersedes` relation while still Accepted is invalid active scope; the governed supersession transition marks it Superseded.

## F2 — Functional/domain/authorization semantics are internally coherent.

**Rules:** 24 total; **17 blocker(s)** in the default profile.

### `PLUMB.F2.DOMAIN.ATTRIBUTE_OWNER` — Every attribute has exactly one owning entity in the core profile

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each accepted Attribute has exactly one incoming has_attribute from an accepted Entity.
- **Pass condition:** Each accepted Attribute has exactly one incoming has_attribute from an accepted Entity.
- **Waiver:** `decision_required`

### `PLUMB.F2.DOMAIN.RELATION_TYPED` — Domain relationships have known endpoints and cardinality when semantically required

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Relationship endpoints exist and any required cardinality/optionality constraints are resolved.
- **Pass condition:** Relationship endpoints exist and any required cardinality/optionality constraints are resolved.
- **Waiver:** `decision_required`
- **Pilot evaluation note:** An Accepted pilot `DomainRelationship` must have valid `Entity` endpoints, and both `cardinality_from` and `cardinality_to` must be one of the four canonical pilot values `0..1`, `1`, `0..*` and `1..*`. A relationship candidate whose cardinality is unresolved is not a node with a sentinel value; S2.1 reports it as deterministic finding material for this rule with semantic condition key `domain_relationship_cardinality_unresolved:<domainrel-id>`.

### `PLUMB.F2.STATE.TRANSITION_COMPLETE` — State transitions are complete

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each Transition has source state, target state and exactly one trigger via Operation/Event.
- **Pass condition:** Each Transition has source state, target state and exactly one trigger via Operation/Event.
- **Waiver:** `decision_required`

### `PLUMB.F2.STATE.REACHABILITY` — State models have no unreachable accepted states

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Entity/process declares a state machine with an initial state.
- **Deterministic check:** Every accepted non-terminal state is reachable from an initial state unless explicitly marked externally-entered.
- **Pass condition:** Every accepted non-terminal state is reachable from an initial state unless explicitly marked externally-entered.
- **Waiver:** `decision_required`

### `PLUMB.F2.OPERATION.PERFORMER` — Executable operations have a performer or owning system

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each operation requiring execution has performed_by Actor/BusinessRole or an explicit owning system/component placeholder.
- **Pass condition:** Each operation requiring execution has performed_by Actor/BusinessRole or an explicit owning system/component placeholder.
- **Waiver:** `decision_required`

### `PLUMB.F2.OPERATION.IO_TYPED` — Operation inputs, reads, writes and outputs reference typed domain elements

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** All referenced entities/attributes/messages exist and type constraints resolve.
- **Pass condition:** All referenced entities/attributes/messages exist and type constraints resolve.
- **Waiver:** `decision_required`

### `PLUMB.F2.CALC.TYPECHECK` — Calculations type-check

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every accepted Calculation parses and type-checks under PlumbExpr.
- **Pass condition:** Every accepted Calculation parses and type-checks under PlumbExpr.
- **Waiver:** `forbidden`

### `PLUMB.F2.CALC.NO_CYCLE` — Calculation dependency cycles are resolved

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Calculation dependency graph is acyclic unless an explicitly supported fixed-point construct is used.
- **Pass condition:** Calculation dependency graph is acyclic unless an explicitly supported fixed-point construct is used.
- **Waiver:** `forbidden`

### `PLUMB.F2.TIME.CALENDAR_DEFINED` — Business-time expressions have calendar/timezone semantics

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Calculation/rule uses working days, local dates, holidays or business-time boundaries.
- **Deterministic check:** A Calendar and timezone/boundary convention are linked.
- **Pass condition:** A Calendar and timezone/boundary convention are linked.
- **Waiver:** `decision_required`

### `DMN.F2.TABLE.NO_OVERLAP` — Deterministic decision tables have no illegal overlap

- **Class:** `external_interop`
- **Severity:** `blocker`
- **Source/mapping:** OMG DMN 1.6 (subset; interchange)
- **Applies when:** DecisionTable is executable and its hit policy does not permit overlapping matches.
- **Deterministic check:** Static/solver analysis finds no overlapping rule cells.
- **Pass condition:** Static/solver analysis finds no overlapping rule cells.
- **Waiver:** `decision_required`

### `DMN.F2.TABLE.COVERAGE` — Required decision domains are covered

- **Class:** `external_interop`
- **Severity:** `error`
- **Source/mapping:** OMG DMN 1.6 (subset; interchange)
- **Applies when:** DecisionTable is executable and profile requires total decision semantics.
- **Deterministic check:** Enumerated/boolean/interval domains are complete or a default/else rule is explicit.
- **Pass condition:** Enumerated/boolean/interval domains are complete or a default/else rule is explicit.
- **Waiver:** `decision_required`

### `BPMN.F2.PROCESS.START_END` — Executable process has valid entry and completion semantics

- **Class:** `external_interop`
- **Severity:** `blocker`
- **Source/mapping:** OMG BPMN 2.0.2 (subset; interchange)
- **Applies when:** Process is executable/in-scope.
- **Deterministic check:** At least one start/trigger exists and every reachable terminal path ends in an outcome/end node.
- **Pass condition:** At least one start/trigger exists and every reachable terminal path ends in an outcome/end node.
- **Waiver:** `decision_required`

### `BPMN.F2.PROCESS.REACHABLE` — Process contains no unreachable executable node

- **Class:** `external_interop`
- **Severity:** `blocker`
- **Source/mapping:** OMG BPMN 2.0.2 (subset; interchange)
- **Applies when:** Process is executable/in-scope.
- **Deterministic check:** All accepted executable ProcessNodes are reachable from a start/trigger.
- **Pass condition:** All accepted executable ProcessNodes are reachable from a start/trigger.
- **Waiver:** `decision_required`

### `PLUMB.F2.PROCESS.TASK_RESOLVES` — Executable process tasks resolve to operations or explicit human activities

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each executable task references Operation, or is marked non-system human activity with responsibility and outcome semantics.
- **Pass condition:** Each executable task references Operation, or is marked non-system human activity with responsibility and outcome semantics.
- **Waiver:** `decision_required`

### `PLUMB.F2.PROCESS.GATEWAY_BALANCED` — Parallel semantics are structurally resolvable

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Process contains parallel split/join semantics.
- **Deterministic check:** Parallel branches have compatible join/termination semantics; no branch can deadlock solely due to model structure.
- **Pass condition:** Parallel branches have compatible join/termination semantics; no branch can deadlock solely due to model structure.
- **Waiver:** `decision_required`

### `PLUMB.F2.EVENT.PAYLOAD_TYPED` — Events have typed payload semantics

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Event carries business data.
- **Deterministic check:** Each payload field maps to Attribute/DataSchema with resolved types.
- **Pass condition:** Each payload field maps to Attribute/DataSchema with resolved types.
- **Waiver:** `decision_required`

### `PLUMB.F2.EVENT.PRODUCER` — Consumed internal events have a producer

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Event is internal and consumed by an in-scope process/operation.
- **Deterministic check:** At least one in-scope Operation/ProcessNode produces the event, or source is declared external.
- **Pass condition:** At least one in-scope Operation/ProcessNode produces the event, or source is declared external.
- **Waiver:** `decision_required`

### `PLUMB.F2.EVENT.CONSUMER` — Produced material events have a consumer or explicit terminal purpose

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Event is produced and classified material/integration-relevant.
- **Deterministic check:** Event has at least one consumer/subscriber or is explicitly terminal/audit-only.
- **Pass condition:** Event has at least one consumer/subscriber or is explicitly terminal/audit-only.
- **Waiver:** `decision_required`

### `RBAC.F2.PERMISSION.CONCRETE` — Permissions resolve to operations and resource scope

- **Class:** `external_standard`
- **Severity:** `blocker`
- **Source/mapping:** INCITS 359 2012 (compatible; semantic_alignment)
- **Applies when:** Permission status = Accepted.
- **Deterministic check:** Permission permits at least one concrete Operation and has exactly one ResourceScope.
- **Pass condition:** Permission permits at least one concrete Operation and has exactly one ResourceScope.
- **Waiver:** `decision_required`

### `RBAC.F2.ROLE.HIERARCHY_ACYCLIC` — Security-role hierarchy is acyclic

- **Class:** `external_standard`
- **Severity:** `blocker`
- **Source/mapping:** INCITS 359 2012 (compatible; semantic_alignment)
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** inherits_role graph has no cycle.
- **Pass condition:** inherits_role graph has no cycle.
- **Waiver:** `forbidden`

### `PLUMB.F2.ROLE.SEPARATION` — Business roles and security roles remain semantically distinct

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** No node is simultaneously typed BusinessRole and SecurityRole; mappings are explicit edges.
- **Pass condition:** No node is simultaneously typed BusinessRole and SecurityRole; mappings are explicit edges.
- **Waiver:** `forbidden`

### `PLUMB.F2.SOD.NO_VIOLATION` — Blocking separation-of-duty constraints are satisfiable

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Security-sensitive operations have SeparationConstraint.
- **Deterministic check:** No accepted role assignment grants a forbidden combination without explicit profile-authorized exception.
- **Pass condition:** No accepted role assignment grants a forbidden combination without explicit profile-authorized exception.
- **Waiver:** `decision_required`

### `PLUMB.F2.INVARIANT.EXPRESSIBLE` — Accepted invariants are executable or explicitly non-executable

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Invariant has a valid PlumbExpr AST or a declared manual/formal verification method.
- **Pass condition:** Invariant has a valid PlumbExpr AST or a declared manual/formal verification method.
- **Waiver:** `decision_required`

### `PLUMB.F2.NO_UNRESOLVED_SEMANTIC_BLOCKER` — No unresolved blocking functional-semantic finding remains

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Active F2 rule registry contains no unresolved blocker finding.
- **Pass condition:** Active F2 rule registry contains no unresolved blocker finding.
- **Waiver:** `decision_required`

## F3 — Blocking ambiguities and semantic decisions are governed and resolved.

**Rules:** 6 total; **4 blocker(s)** in the default profile.

### `PLUMB.F3.QUESTION.NO_BLOCKING_OPEN` — No blocking functional question remains open

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** All Question nodes with blocking=true in functional scope are Answered/Closed/Superseded.
- **Pass condition:** All Question nodes with blocking=true in functional scope are Answered/Closed/Superseded.
- **Waiver:** `decision_required`

### `PLUMB.F3.DECISION.PATCH_APPLIED` — Accepted resolution decisions are reflected in the graph

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each Accepted ResolutionDecision has a replayable SemanticPatch whose resulting semantic effects are present at current baseline.
- **Pass condition:** Each Accepted ResolutionDecision has a replayable SemanticPatch whose resulting semantic effects are present at current baseline.
- **Waiver:** `decision_required`

### `PLUMB.F3.DECISION.EVIDENCE` — Material decisions retain rationale and provenance

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** ResolutionDecision changes a blocking semantic ambiguity.
- **Deterministic check:** Decision records actor, timestamp, answer, target question/finding and rationale/evidence as required by profile.
- **Pass condition:** Decision records actor, timestamp, answer, target question/finding and rationale/evidence as required by profile.
- **Waiver:** `decision_required`

### `PLUMB.F3.ASSUMPTION.OWNER` — Assumptions have owner and expiry/review policy

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Assumption affects a blocking or architecture-driving semantic element.
- **Deterministic check:** owner is set and expires/review_at is set or profile explicitly marks assumption permanent.
- **Pass condition:** owner is set and expires/review_at is set or profile explicitly marks assumption permanent.
- **Waiver:** `decision_required`

### `PLUMB.F3.ASSUMPTION.NOT_EXPIRED` — Expired assumptions do not silently satisfy gates

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** No active blocking assumption is beyond expiry/review date without renewed ResolutionDecision.
- **Pass condition:** No active blocking assumption is beyond expiry/review date without renewed ResolutionDecision.
- **Waiver:** `decision_required`

### `PLUMB.F3.WAIVER.DECISION` — Blocking waivers have explicit governance

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** A blocker/error is marked Waived.
- **Deterministic check:** Waiver references ResolutionDecision, owner, rationale, scope and review/expiry if required.
- **Pass condition:** Waiver references ResolutionDecision, owner, rationale, scope and review/expiry if required.
- **Waiver:** `decision_required`

## F4 — Functional behavior is executable or otherwise verifiable without invented semantics.

**Rules:** 7 total; **5 blocker(s)** in the default profile.

### `PLUMB.F4.FUNCTION.VERIFICATION_LINK` — In-scope functional obligations have a verification obligation

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Requirement/Operation is functional and implementation-scoped.
- **Deterministic check:** At least one verified_by link reaches an accepted VerificationObligation.
- **Pass condition:** At least one verified_by link reaches an accepted VerificationObligation.
- **Waiver:** `decision_required`

### `PLUMB.F4.SCENARIO.FOR_EXECUTABLE_BEHAVIOR` — Executable business behavior has representative scenarios

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Operation/Process/Rule participates in deterministic executable business behavior.
- **Deterministic check:** At least one accepted Scenario covers normal behavior and each blocking outcome/boundary class required by profile.
- **Pass condition:** At least one accepted Scenario covers normal behavior and each blocking outcome/boundary class required by profile.
- **Waiver:** `decision_required`

### `PLUMB.F4.SCENARIO.GROUNDED` — Scenarios trace to obligations

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each accepted Scenario references Requirement/AcceptanceCriterion/Rule/Transition from which it is derived.
- **Pass condition:** Each accepted Scenario references Requirement/AcceptanceCriterion/Rule/Transition from which it is derived.
- **Waiver:** `decision_required`

### `PLUMB.F4.SCENARIO.TYPED` — Scenario Given/When/Then values are type-valid

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** All scenario fixtures satisfy entity types, units, enum domains and operation inputs.
- **Pass condition:** All scenario fixtures satisfy entity types, units, enum domains and operation inputs.
- **Waiver:** `forbidden`

### `PLUMB.F4.RUN.NO_BLOCKING_UNDECIDABLE` — Blocking executable scenarios are decidable

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Latest current ScenarioRun for every blocking executable scenario is Pass or an accepted expected-failure case; none is Undecidable.
- **Pass condition:** Latest current ScenarioRun for every blocking executable scenario is Pass or an accepted expected-failure case; none is Undecidable.
- **Waiver:** `decision_required`

### `PLUMB.F4.RUN.TRACE_COMPLETE` — Scenario execution produces semantic trace

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** ScenarioRun records operation/rule/calculation/transition steps sufficient to explain the result.
- **Pass condition:** ScenarioRun records operation/rule/calculation/transition steps sufficient to explain the result.
- **Waiver:** `decision_required`

### `PLUMB.F4.ASSUMPTIONS.VISIBLE` — Assumptions affecting execution are visible in gate output

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Gate report lists every active assumption reachable from covered obligations.
- **Pass condition:** Gate report lists every active assumption reachable from covered obligations.
- **Waiver:** `forbidden`

## Q1 — Architecture-driving quality requirements are measurable.

**Rules:** 8 total; **7 blocker(s)** in the default profile.

### `ISO25010.Q1.CHARACTERISTIC` — Quality requirements map to a recognized quality characteristic

- **Class:** `external_standard`
- **Severity:** `blocker`
- **Source/mapping:** ISO/IEC 25010 2023 (compatible; taxonomy)
- **Applies when:** Requirement kind = quality, or QualityScenario is architecture-driving.
- **Deterministic check:** QualityScenario has exactly one characterized_by QualityCharacteristic from active taxonomy or approved extension.
- **Pass condition:** QualityScenario has exactly one characterized_by QualityCharacteristic from active taxonomy or approved extension.
- **Waiver:** `decision_required`

### `PLUMB.Q1.SCENARIO.STIMULUS` — Quality scenario identifies stimulus

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Architecture-driving QualityScenario.stimulus is populated.
- **Pass condition:** Architecture-driving QualityScenario.stimulus is populated.
- **Waiver:** `decision_required`

### `PLUMB.Q1.SCENARIO.ENVIRONMENT` — Quality scenario identifies operating environment/condition

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Architecture-driving QualityScenario.environment is explicit.
- **Pass condition:** Architecture-driving QualityScenario.environment is explicit.
- **Waiver:** `decision_required`

### `PLUMB.Q1.SCENARIO.TARGET` — Quality scenario identifies affected artifact/scope

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** QualityScenario references at least one system/function/architecture target.
- **Pass condition:** QualityScenario references at least one system/function/architecture target.
- **Waiver:** `decision_required`

### `PLUMB.Q1.SCENARIO.MEASURE` — Quality scenario is measurable

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** QualityScenario has exactly one measured_by Measure and a defined unit/scale where applicable.
- **Pass condition:** QualityScenario has exactly one measured_by Measure and a defined unit/scale where applicable.
- **Waiver:** `decision_required`

### `PLUMB.Q1.SCENARIO.THRESHOLD` — Quality scenario has acceptance threshold

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Threshold/range/target semantics are explicit and type-compatible with the Measure.
- **Pass condition:** Threshold/range/target semantics are explicit and type-compatible with the Measure.
- **Waiver:** `decision_required`

### `PLUMB.Q1.QUALITY.NO_CONFLICT` — Quality thresholds have no unresolved internal conflict

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** No two accepted quality obligations impose contradictory thresholds on the same scoped condition without precedence/decision.
- **Pass condition:** No two accepted quality obligations impose contradictory thresholds on the same scoped condition without precedence/decision.
- **Waiver:** `decision_required`

### `PLUMB.Q1.QUALITY.PRIORITY` — Architecture-driving quality scenarios have priority/criticality

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** criticality or priority is classified for all architecture-driving QualityScenarios.
- **Pass condition:** criticality or priority is classified for all architecture-driving QualityScenarios.
- **Waiver:** `decision_required`

## A1 — Architecture scope, stakeholders, concerns, drivers and constraints are complete enough to design against.

**Rules:** 9 total; **8 blocker(s)** in the default profile.

### `ISO42010.A1.SYSTEM_OF_INTEREST` — Architecture scope identifies the system/entity of interest

- **Class:** `external_standard`
- **Severity:** `blocker`
- **Source/mapping:** ISO/IEC/IEEE 42010 2022 (compatible; semantic_alignment)
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Exactly one primary SystemOfInterest is designated for the active architecture baseline or profile explicitly supports a federation.
- **Pass condition:** Exactly one primary SystemOfInterest is designated for the active architecture baseline or profile explicitly supports a federation.
- **Waiver:** `decision_required`

### `ISO42010.A1.STAKEHOLDERS` — Relevant architecture stakeholders are identified

- **Class:** `external_standard`
- **Severity:** `blocker`
- **Source/mapping:** ISO/IEC/IEEE 42010 2022 (compatible; semantic_alignment)
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** ArchitectureDescription references at least one Stakeholder and all mandatory stakeholder classes in the active profile are represented.
- **Pass condition:** ArchitectureDescription references at least one Stakeholder and all mandatory stakeholder classes in the active profile are represented.
- **Waiver:** `decision_required`

### `ISO42010.A1.CONCERNS` — Relevant architecture concerns are identified

- **Class:** `external_standard`
- **Severity:** `blocker`
- **Source/mapping:** ISO/IEC/IEEE 42010 2022 (compatible; semantic_alignment)
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Architecture-driving stakeholder concerns are accepted and linked to stakeholders.
- **Pass condition:** Architecture-driving stakeholder concerns are accepted and linked to stakeholders.
- **Waiver:** `decision_required`

### `ISO42010.A1.CONCERNS.ADDRESSED` — Architecture concerns are addressed

- **Class:** `external_standard`
- **Severity:** `blocker`
- **Source/mapping:** ISO/IEC/IEEE 42010 2022 (compatible; semantic_alignment)
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every blocking Concern is addressed by at least one View, Requirement, QualityScenario, Constraint or ArchitectureDecision.
- **Pass condition:** Every blocking Concern is addressed by at least one View, Requirement, QualityScenario, Constraint or ArchitectureDecision.
- **Waiver:** `decision_required`

### `PLUMB.A1.DRIVER.FUNCTIONAL` — Architecture-driving functional obligations are selected

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** All high-impact operations/processes/data responsibilities are marked as drivers or explicitly classified non-driving.
- **Pass condition:** All high-impact operations/processes/data responsibilities are marked as drivers or explicitly classified non-driving.
- **Waiver:** `decision_required`

### `PLUMB.A1.DRIVER.QUALITY` — Architecture-driving quality scenarios are selected

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** All blocking/critical QualityScenarios are linked via drives to candidate/decision analysis.
- **Pass condition:** All blocking/critical QualityScenarios are linked via drives to candidate/decision analysis.
- **Waiver:** `decision_required`

### `PLUMB.A1.DRIVER.CONSTRAINTS` — Technical constraints are classified and scoped

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every in-scope Technical/Integration/Data/Security/Deployment constraint has scope and strength: mandatory/preferred/prohibited.
- **Pass condition:** Every in-scope Technical/Integration/Data/Security/Deployment constraint has scope and strength: mandatory/preferred/prohibited.
- **Waiver:** `decision_required`

### `PLUMB.A1.DRIVER.NO_CONFLICT` — Architecture drivers contain no unresolved blocker conflict

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** No open blocking conflict exists among mandatory constraints, critical quality thresholds and required functionality.
- **Pass condition:** No open blocking conflict exists among mandatory constraints, critical quality thresholds and required functionality.
- **Waiver:** `decision_required`

### `PLUMB.A1.EXTERNAL_SYSTEMS` — Material external systems and boundaries are identified

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** System exchanges data/control with external systems or actors.
- **Deterministic check:** ExternalSystem/Actor nodes and interaction responsibilities exist for all material boundary interactions.
- **Pass condition:** ExternalSystem/Actor nodes and interaction responsibilities exist for all material boundary interactions.
- **Waiver:** `decision_required`

## A2 — Software responsibilities, data and interfaces are allocated to architecture elements.

**Rules:** 9 total; **7 blocker(s)** in the default profile.

### `PLUMB.A2.OPERATION.ALLOCATED` — Implementation-scoped operations are allocated

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each implementation-scoped Operation has >=1 allocated_to accepted ArchitectureElement.
- **Pass condition:** Each implementation-scoped Operation has >=1 allocated_to accepted ArchitectureElement.
- **Waiver:** `decision_required`

### `PLUMB.A2.PROCESS.RESPONSIBILITY` — Process/service responsibilities are allocated

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Process or ProcessNode implies software responsibility.
- **Deterministic check:** Software-executed responsibilities are allocated to an accepted ArchitectureElement.
- **Pass condition:** Software-executed responsibilities are allocated to an accepted ArchitectureElement.
- **Waiver:** `decision_required`

### `PLUMB.A2.DATA.OWNERSHIP` — Persistent business data has ownership

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Entity/Attribute is persistent or authoritative.
- **Deterministic check:** At least one accepted Component/Container is designated owner/system-of-record and associated DataStore where relevant.
- **Pass condition:** At least one accepted Component/Container is designated owner/system-of-record and associated DataStore where relevant.
- **Waiver:** `decision_required`

### `PLUMB.A2.INTERFACE.OWNERSHIP` — Interfaces have owning architecture elements

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every accepted Interface/ApiContract/EventContract has at least one exposing/owning ArchitectureElement.
- **Pass condition:** Every accepted Interface/ApiContract/EventContract has at least one exposing/owning ArchitectureElement.
- **Waiver:** `decision_required`

### `PLUMB.A2.COMPONENT.JUSTIFIED` — Architecture elements have upstream justification

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Component/Container is in accepted candidate.
- **Deterministic check:** At least one Requirement, Operation, QualityScenario, Constraint or ArchitectureDecision justifies the element.
- **Pass condition:** At least one Requirement, Operation, QualityScenario, Constraint or ArchitectureDecision justifies the element.
- **Waiver:** `decision_required`

### `PLUMB.A2.NO_ORPHAN_DEPENDENCY` — Architecture dependencies reference accepted elements

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every depends_on edge connects accepted elements in the same candidate/baseline or explicitly external elements.
- **Pass condition:** Every depends_on edge connects accepted elements in the same candidate/baseline or explicitly external elements.
- **Waiver:** `forbidden`

### `PLUMB.A2.ALLOCATION.NO_CONTRADICTION` — Allocations respect technical constraints

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** No allocated element violates a mandatory/prohibited scoped Constraint or accepted TechnologySelection.
- **Pass condition:** No allocated element violates a mandatory/prohibited scoped Constraint or accepted TechnologySelection.
- **Waiver:** `decision_required`

### `PLUMB.A2.CRITICAL_SINGLE_OWNER` — Critical stateful responsibility has unambiguous ownership

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Responsibility is classified stateful/authoritative/critical.
- **Deterministic check:** Ownership is single or an explicit replication/coordination model is defined.
- **Pass condition:** Ownership is single or an explicit replication/coordination model is defined.
- **Waiver:** `decision_required`

### `ISO42010.A2.VIEWS.DEFINED` — Required architecture views exist

- **Class:** `external_standard`
- **Severity:** `error`
- **Source/mapping:** ISO/IEC/IEEE 42010 2022 (compatible; semantic_alignment)
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every required Viewpoint in the active profile has at least one corresponding View for the accepted candidate.
- **Pass condition:** Every required Viewpoint in the active profile has at least one corresponding View for the accepted candidate.
- **Waiver:** `decision_required`

## A3 — Cross-boundary technical contracts are defined, valid and traceable.

**Rules:** 14 total; **12 blocker(s)** in the default profile.

### `PLUMB.A3.BOUNDARY.CONTRACT` — Every material software boundary interaction has a technical contract

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Cross-element synchronous/event interaction has Interface plus ApiContract/EventContract or an explicit non-API protocol contract.
- **Pass condition:** Cross-element synchronous/event interaction has Interface plus ApiContract/EventContract or an explicit non-API protocol contract.
- **Waiver:** `decision_required`

### `OPENAPI.A3.DOCUMENT.VALID` — HTTP API projections validate against declared OpenAPI version

- **Class:** `external_interop`
- **Severity:** `blocker`
- **Source/mapping:** OpenAPI Specification 3.2.1 (exact; interchange)
- **Applies when:** ApiContract.protocol = HTTP and projection format = OpenAPI.
- **Deterministic check:** Generated/imported OpenAPI document passes official/schema-aware validator for the declared OAS version.
- **Pass condition:** Generated/imported OpenAPI document passes official/schema-aware validator for the declared OAS version.
- **Waiver:** `forbidden`

### `PLUMB.A3.API.TRACE` — API operations trace to functional semantics

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each implementation-relevant ApiOperation has exposed_by/specifies trace to Operation/Requirement unless explicitly infrastructure-only.
- **Pass condition:** Each implementation-relevant ApiOperation has exposed_by/specifies trace to Operation/Requirement unless explicitly infrastructure-only.
- **Waiver:** `decision_required`

### `PLUMB.A3.API.OPERATION_ID` — API operations have stable identifiers

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each ApiOperation has a stable operation_id unique within project contract namespace.
- **Pass condition:** Each ApiOperation has a stable operation_id unique within project contract namespace.
- **Waiver:** `forbidden`

### `PLUMB.A3.API.SCHEMA` — API request/response data is schema-defined

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** ApiOperation carries structured request/response body or parameters.
- **Deterministic check:** Input/output schemas resolve to DataSchema and semantic domain types.
- **Pass condition:** Input/output schemas resolve to DataSchema and semantic domain types.
- **Waiver:** `decision_required`

### `PLUMB.A3.API.FAILURE` — Material failure outcomes are represented in contracts

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Functional Operation defines blocking/material failure Outcome.
- **Deterministic check:** Technical contract exposes corresponding error/outcome semantics or explicit transport-independent mapping.
- **Pass condition:** Technical contract exposes corresponding error/outcome semantics or explicit transport-independent mapping.
- **Waiver:** `decision_required`

### `ASYNCAPI.A3.DOCUMENT.VALID` — Event API projections validate against AsyncAPI 3.0

- **Class:** `external_interop`
- **Severity:** `blocker`
- **Source/mapping:** AsyncAPI Specification 3.0.0 (exact; interchange)
- **Applies when:** EventContract projects to AsyncAPI.
- **Deterministic check:** AsyncAPI document is syntactically/specification-valid for declared version.
- **Pass condition:** AsyncAPI document is syntactically/specification-valid for declared version.
- **Waiver:** `forbidden`

### `PLUMB.A3.EVENT.CHANNEL` — Event operations reference channel and message

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each publish/subscribe technical interaction resolves to accepted Channel and Message.
- **Pass condition:** Each publish/subscribe technical interaction resolves to accepted Channel and Message.
- **Waiver:** `decision_required`

### `PLUMB.A3.EVENT.PRODUCER_CONSUMER` — Material event contracts have producer and consumer semantics

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each material event/message has at least one publisher and subscriber or explicit external endpoint.
- **Pass condition:** Each material event/message has at least one publisher and subscriber or explicit external endpoint.
- **Waiver:** `decision_required`

### `PLUMB.A3.MESSAGE.SCHEMA` — Message payloads are schema-defined

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Message has payload.
- **Deterministic check:** Payload maps to accepted DataSchema/domain attributes and version semantics.
- **Pass condition:** Payload maps to accepted DataSchema/domain attributes and version semantics.
- **Waiver:** `decision_required`

### `ARAZZO.A3.WORKFLOW.VALID` — API workflow projection validates against Arazzo

- **Class:** `external_interop`
- **Severity:** `blocker`
- **Source/mapping:** Arazzo Specification 1.1.0 (exact; interchange)
- **Applies when:** TechnicalWorkflow projects to Arazzo.
- **Deterministic check:** Arazzo description is valid and references resolvable source descriptions/API operations.
- **Pass condition:** Arazzo description is valid and references resolvable source descriptions/API operations.
- **Waiver:** `forbidden`

### `PLUMB.A3.WORKFLOW.TRACE` — Technical workflows trace to user/business outcomes

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** TechnicalWorkflow is not purely infrastructure maintenance.
- **Deterministic check:** Workflow links to Scenario/Process/Requirement outcome.
- **Pass condition:** Workflow links to Scenario/Process/Requirement outcome.
- **Waiver:** `decision_required`

### `PLUMB.A3.CONTRACT.VERSION_POLICY` — Externally consumed contracts have version/change policy

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Contract is external/public/cross-team according to profile.
- **Deterministic check:** version identifier and compatibility/change policy are present.
- **Pass condition:** version identifier and compatibility/change policy are present.
- **Waiver:** `profile_allow`

### `PLUMB.A3.SECURITY.AUTHZ_TRACE` — Security-sensitive contract operations trace to authorization semantics

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** ApiOperation/Channel action is protected.
- **Deterministic check:** Contract operation traces to Permission/PolicyCondition or explicit external authorization policy.
- **Pass condition:** Contract operation traces to Permission/PolicyCondition or explicit external authorization policy.
- **Waiver:** `decision_required`

## A4 — Accepted architecture and technology choices are justified against drivers and tradeoffs.

**Rules:** 9 total; **5 blocker(s)** in the default profile.

### `ISO42010.A4.VIEW.VIEWPOINT` — Architecture views conform to declared viewpoints

- **Class:** `external_standard`
- **Severity:** `error`
- **Source/mapping:** ISO/IEC/IEEE 42010 2022 (compatible; semantic_alignment)
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each required View references one Viewpoint and satisfies that Viewpoint's required model kinds/concerns.
- **Pass condition:** Each required View references one Viewpoint and satisfies that Viewpoint's required model kinds/concerns.
- **Waiver:** `decision_required`

### `PLUMB.A4.CANDIDATE.UNIQUE_ACCEPTED` — Exactly one architecture candidate is accepted per baseline

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** One and only one candidate is Accepted unless active profile explicitly allows multi-target accepted architectures.
- **Pass condition:** One and only one candidate is Accepted unless active profile explicitly allows multi-target accepted architectures.
- **Waiver:** `decision_required`

### `PLUMB.A4.DECISION.DRIVER` — Accepted architecture decisions have drivers

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every accepted ArchitectureDecision has >=1 justified_by Requirement/QualityScenario/Constraint.
- **Pass condition:** Every accepted ArchitectureDecision has >=1 justified_by Requirement/QualityScenario/Constraint.
- **Waiver:** `decision_required`

### `PLUMB.A4.DECISION.ALTERNATIVES` — Material architecture decisions record considered alternatives

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Decision materially affects architecture, technology, data, integration, deployment, security or quality.
- **Deterministic check:** At least two alternatives or an explicit reason why no meaningful alternative existed is recorded.
- **Pass condition:** At least two alternatives or an explicit reason why no meaningful alternative existed is recorded.
- **Waiver:** `decision_required`

### `PLUMB.A4.DECISION.RATIONALE` — Accepted architecture decisions include rationale

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** rationale is non-empty and linked to drivers/tradeoffs.
- **Pass condition:** rationale is non-empty and linked to drivers/tradeoffs.
- **Waiver:** `decision_required`

### `PLUMB.A4.DECISION.CONSEQUENCES` — Material decisions record consequences

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** positive/negative consequence set is non-empty for material decisions.
- **Pass condition:** positive/negative consequence set is non-empty for material decisions.
- **Waiver:** `decision_required`

### `PLUMB.A4.QUALITY.RESPONSE` — Critical quality scenarios have architecture responses

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each critical QualityScenario links to ArchitectureDecision/element/tactic or explicit accepted risk.
- **Pass condition:** Each critical QualityScenario links to ArchitectureDecision/element/tactic or explicit accepted risk.
- **Waiver:** `decision_required`

### `PLUMB.A4.TECH.SELECTION` — Selected technologies are scoped and justified

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** TechnologySelection status = selected.
- **Deterministic check:** technology/version range/applies_to and justification/decision are present.
- **Pass condition:** technology/version range/applies_to and justification/decision are present.
- **Waiver:** `decision_required`

### `PLUMB.A4.ARCH.NO_BLOCKING_FINDING` — No unresolved blocking architecture finding remains

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** All A1-A4 blocker findings are resolved or permitted waivers are recorded.
- **Pass condition:** All A1-A4 blocker findings are resolved or permitted waivers are recorded.
- **Waiver:** `decision_required`

## D1 — Accepted scope is decomposed into governed implementation slices with workable dependency ordering.

**Rules:** 9 total; **5 blocker(s)** in the default profile.

### `PLUMB.D1.SLICE.UPSTREAM` — Implementation slices have upstream obligations

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each ImplementationSlice references >=1 Requirement/Operation/ArchitectureElement/Contract obligation.
- **Pass condition:** Each ImplementationSlice references >=1 Requirement/Operation/ArchitectureElement/Contract obligation.
- **Waiver:** `decision_required`

### `PLUMB.D1.SLICE.ARCH_SCOPE` — Implementation slices identify architecture scope

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each implementation slice identifies affected Component/Module/Interface/DataStore or explicit reason for non-code slice.
- **Pass condition:** Each implementation slice identifies affected Component/Module/Interface/DataStore or explicit reason for non-code slice.
- **Waiver:** `decision_required`

### `PLUMB.D1.SLICE.CONTRACT_SCOPE` — Interface-impacting slices identify affected contracts

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Slice changes cross-boundary behavior/data.
- **Deterministic check:** Affected ApiContract/EventContract/DataSchema references are present.
- **Pass condition:** Affected ApiContract/EventContract/DataSchema references are present.
- **Waiver:** `decision_required`

### `PLUMB.D1.SLICE.DEPENDENCY_DAG` — Implementation dependency graph is acyclic

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** depends_on_slice graph is a DAG, or every cycle is covered by an accepted Migration strategy.
- **Pass condition:** depends_on_slice graph is a DAG, or every cycle is covered by an accepted Migration strategy.
- **Waiver:** `forbidden`

### `PLUMB.D1.SCOPE.COVERAGE` — Accepted implementation scope is covered by slices

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every in-scope implementation obligation maps to >=1 ImplementationSlice.
- **Pass condition:** Every in-scope implementation obligation maps to >=1 ImplementationSlice.
- **Waiver:** `decision_required`

### `PLUMB.D1.NO_ORPHAN_SLICE` — No implementation slice exists without engineering justification

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every slice has upstream obligation and accepted owner/planning status.
- **Pass condition:** Every slice has upstream obligation and accepted owner/planning status.
- **Waiver:** `decision_required`

### `PLUMB.D1.TASK.CONTRACT` — Agent/developer-facing tasks have explicit task contracts

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Slice is delegated to autonomous/semi-autonomous coding or cross-team implementation.
- **Deterministic check:** TaskContract defines must, must_not, allowed_changes, forbidden_changes and completion criteria.
- **Pass condition:** TaskContract defines must, must_not, allowed_changes, forbidden_changes and completion criteria.
- **Waiver:** `decision_required`

### `ISO12207.D1.LIFECYCLE.OWNERSHIP` — Delivery work has lifecycle responsibility

- **Class:** `external_standard`
- **Severity:** `error`
- **Source/mapping:** ISO/IEC/IEEE 12207 2026 (compatible; semantic_alignment)
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every active WorkPackage/Slice has responsible owner/role and lifecycle state.
- **Pass condition:** Every active WorkPackage/Slice has responsible owner/role and lifecycle state.
- **Waiver:** `decision_required`

### `ISO15289.D1.INFO.ITEMS` — Required lifecycle information outputs are identified

- **Class:** `external_standard`
- **Severity:** `warn`
- **Source/mapping:** ISO/IEC/IEEE 15289 2019 (compatible; semantic_alignment)
- **Applies when:** Active profile requires human-readable lifecycle information items.
- **Deterministic check:** Required generated/projected information items are declared with owners and target formats.
- **Pass condition:** Required generated/projected information items are declared with owners and target formats.
- **Waiver:** `profile_allow`

## D2 — Every in-scope obligation has an explicit verification plan and downstream evidence path.

**Rules:** 10 total; **8 blocker(s)** in the default profile.

### `ISO29148.D2.REQ.VERIFICATION_METHOD` — In-scope verifiable requirements have a verification method

- **Class:** `external_standard`
- **Severity:** `blocker`
- **Source/mapping:** ISO/IEC/IEEE 29148 2018 (compatible; semantic_alignment)
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every in-scope Requirement classified verifiable has >=1 accepted VerificationObligation with method.
- **Pass condition:** Every in-scope Requirement classified verifiable has >=1 accepted VerificationObligation with method.
- **Waiver:** `decision_required`

### `PLUMB.D2.OBLIGATION.TYPED` — Verification obligations use an explicit method

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** method is one of test, scenario, analysis, inspection, review, demonstration, formal_check, architecture_check, security_check, or registered extension.
- **Pass condition:** method is one of test, scenario, analysis, inspection, review, demonstration, formal_check, architecture_check, security_check, or registered extension.
- **Waiver:** `decision_required`

### `PLUMB.D2.OBLIGATION.CRITERIA` — Verification obligations have success criteria

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each obligation defines expected result/pass condition or references AcceptanceCriterion/QualityScenario threshold.
- **Pass condition:** Each obligation defines expected result/pass condition or references AcceptanceCriterion/QualityScenario threshold.
- **Waiver:** `decision_required`

### `PLUMB.D2.OBLIGATION.OWNER` — Verification obligations have responsibility

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** owner/responsible role is assigned.
- **Pass condition:** owner/responsible role is assigned.
- **Waiver:** `decision_required`

### `PLUMB.D2.OBLIGATION.IMPLEMENTATION` — Blocking verification obligations have a planned implementation

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each blocking VerificationObligation has implemented_as TestCase/Scenario/ArchitectureCheck or explicit manual verification plan.
- **Pass condition:** Each blocking VerificationObligation has implemented_as TestCase/Scenario/ArchitectureCheck or explicit manual verification plan.
- **Waiver:** `decision_required`

### `ISO29119.D2.TEST.PROCESS_CONTEXT` — Executable tests have governing test context

- **Class:** `external_standard`
- **Severity:** `error`
- **Source/mapping:** ISO/IEC/IEEE 29119-2 2021 (compatible; semantic_alignment)
- **Applies when:** VerificationObligation.method = test or scenario and is release-blocking.
- **Deterministic check:** Test/Scenario has environment/context, responsibility and expected outcome metadata sufficient for controlled execution.
- **Pass condition:** Test/Scenario has environment/context, responsibility and expected outcome metadata sufficient for controlled execution.
- **Waiver:** `decision_required`

### `PLUMB.D2.COVERAGE.TYPED` — Coverage is computed from semantic relations

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** CoverageRecord derives solely from verified_by/implemented_as/trace relations, not name similarity.
- **Pass condition:** CoverageRecord derives solely from verified_by/implemented_as/trace relations, not name similarity.
- **Waiver:** `forbidden`

### `PLUMB.D2.NO_UNCOVERED_BLOCKER` — No blocking obligation lacks a verification plan

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Coverage report shows zero uncovered blocker obligations.
- **Pass condition:** Coverage report shows zero uncovered blocker obligations.
- **Waiver:** `decision_required`

### `PLUMB.D2.SLICE.DOWNSTREAM` — Implementation slices have downstream verification

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every implementation slice has >=1 verification obligation covering its changed obligations.
- **Pass condition:** Every implementation slice has >=1 verification obligation covering its changed obligations.
- **Waiver:** `decision_required`

### `PLUMB.D2.SECURITY.VERIFICATION` — Security-sensitive permissions/contracts have security verification

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Operation/contract is marked security-sensitive.
- **Deterministic check:** At least one security_check/test obligation covers authorization/authentication/policy semantics.
- **Pass condition:** At least one security_check/test obligation covers authorization/authentication/policy semantics.
- **Waiver:** `decision_required`

## C1 — The implementation has current evidence that it conforms to the accepted specification and architecture.

**Rules:** 11 total; **9 blocker(s)** in the default profile.

### `PLUMB.C1.CODE.BINDING` — Implemented semantic elements bind to code/artifacts

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Obligation is marked implemented.
- **Deterministic check:** CodeBinding identifies repository/artifact and revision/path/symbol as supported by binding type.
- **Pass condition:** CodeBinding identifies repository/artifact and revision/path/symbol as supported by binding type.
- **Waiver:** `decision_required`

### `PLUMB.C1.RECEIPT.CURRENT_SPEC` — Verification evidence matches current specification

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Each receipt used for gate satisfaction binds the current semantic_hash or a proven unaffected ancestor baseline.
- **Pass condition:** Each receipt used for gate satisfaction binds the current semantic_hash or a proven unaffected ancestor baseline.
- **Waiver:** `forbidden`

### `PLUMB.C1.RECEIPT.CURRENT_CODE` — Verification evidence matches current code revision

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Receipt binds the code revision under evaluation; later relevant changes mark it stale.
- **Pass condition:** Receipt binds the code revision under evaluation; later relevant changes mark it stale.
- **Waiver:** `forbidden`

### `PLUMB.C1.RECEIPT.TEST_REV` — Verification receipts bind test/check revision

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** TestReceipt includes test/check definition revision/hash.
- **Pass condition:** TestReceipt includes test/check definition revision/hash.
- **Waiver:** `forbidden`

### `PLUMB.C1.RECEIPT.ENV` — Verification receipts identify environment

- **Class:** `plumb_core`
- **Severity:** `error`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Execution environment/profile is recorded for automated/manual evidence where environment can affect result.
- **Pass condition:** Execution environment/profile is recorded for automated/manual evidence where environment can affect result.
- **Waiver:** `decision_required`

### `PLUMB.C1.STALE.NOT_COUNTED` — Stale evidence cannot satisfy current obligations

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** No CoverageRecord counts TestReceipt with staleness=true or invalid dependency fingerprint.
- **Pass condition:** No CoverageRecord counts TestReceipt with staleness=true or invalid dependency fingerprint.
- **Waiver:** `forbidden`

### `PLUMB.C1.CONTRACT.NO_DRIFT` — Implemented technical contracts do not drift from accepted contract

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Generated/observed implementation contract is available.
- **Deterministic check:** Semantic diff contains no unapproved breaking/material change against accepted Api/Event/Data contracts.
- **Pass condition:** Semantic diff contains no unapproved breaking/material change against accepted Api/Event/Data contracts.
- **Waiver:** `decision_required`

### `PLUMB.C1.ARCH.NO_DRIFT` — Implementation does not violate accepted architecture allocation/constraints

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** ArchitectureCheck finds no blocking unapproved dependency, ownership, technology or deployment drift.
- **Pass condition:** ArchitectureCheck finds no blocking unapproved dependency, ownership, technology or deployment drift.
- **Waiver:** `decision_required`

### `PLUMB.C1.TASK.MUST_NOT` — Task-contract prohibitions were respected

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** Implementation performed under TaskContract.
- **Deterministic check:** No changed artifact violates must_not/forbidden_changes unless superseded by ResolutionDecision.
- **Pass condition:** No changed artifact violates must_not/forbidden_changes unless superseded by ResolutionDecision.
- **Waiver:** `forbidden`

### `PLUMB.C1.OBLIGATION.ALL_CURRENT` — All release-blocking verification obligations have current passing evidence

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** Every release-blocking obligation is satisfied by current acceptable evidence with no unresolved blocker finding.
- **Pass condition:** Every release-blocking obligation is satisfied by current acceptable evidence with no unresolved blocker finding.
- **Waiver:** `decision_required`

### `PLUMB.C1.NO_BLOCKING_DRIFT` — No unresolved blocking drift/conformance finding remains

- **Class:** `plumb_core`
- **Severity:** `blocker`
- **Source/mapping:** Plumb v3 normative semantics
- **Applies when:** All in-scope accepted elements of the relevant type.
- **Deterministic check:** All C1 blocker findings are resolved or deployment is stopped.
- **Pass condition:** All C1 blocker findings are resolved or deployment is stopped.
- **Waiver:** `decision_required`

## 5. Gate result object

```yaml
gate: A3
baseline_hash: "psg:sha256:..."
profile: "profile:plumb-software-2026.1"
result: FAIL
evaluated_at: "..."
rules:
  - rule: OPENAPI.A3.DOCUMENT.VALID
    result: PASS
    targets: [api:leave-v1]
    evidence: [projection:openapi:sha256:...]
  - rule: PLUMB.A3.API.TRACE
    result: FAIL
    targets: [apiop:cancel-leave]
    finding: fnd:...
waivers: []
summary:
  blocker_failed: 1
  blocker_waived: 0
  warnings: 2
```

Gate reports MUST be reproducible for a fixed semantic graph/profile/rule-pack version. Operational timestamps do not participate in semantic hashes.

## 6. External conformance reporting

Plumb SHOULD expose four distinct assertions instead of a single vague “compliant” badge:

1. **Plumb Profile Conformant** — all mandatory rules in this profile pass under the declared waiver policy.
2. **Standards Aligned** — PSG concepts are mapped to the named standard/version with declared mapping strengths; this is not external certification.
3. **Interchange Valid** — generated/imported BPMN/DMN/OpenAPI/AsyncAPI/Arazzo artifact passes the selected syntax/semantic validator and declared subset restrictions.
4. **Organization Policy Conformant** — customer rule packs pass.

The report MUST include standard version, mapping strength, validator version, rule-pack version, deviations and waivers.

## 7. Rule-engine implementation contract

Recommended Rust shape:

```rust
pub struct ValidationRule {
    pub id: &'static str,
    pub gate: GateId,
    pub class: RuleClass,
    pub default_severity: Severity,
    pub applies: fn(&Graph, &ValidationContext) -> Applicability,
    pub evaluate: fn(&Graph, &ValidationContext) -> Vec<RuleResult>,
    pub standard: Option<StandardRef>,
    pub waiver_policy: WaiverPolicy,
}
```

`evaluate` MUST be pure. Any external validator execution (for example an OpenAPI validator) runs outside the pure stage and supplies a content-addressed `ValidationArtifact` that becomes an input to deterministic evaluation, following the same pattern as persisted LLM inference artifacts.

## 8. Profile inheritance

```text
plumb:core
   └── plumb:software:2026.1
         ├── plumb:software:financial-services:<version>
         ├── plumb:software:public-sector:<version>
         ├── plumb:software:aviation:<version>
         └── org:<customer>:<version>
```

A child profile MAY: enable additional rules, promote severity, tighten waiver policy, add required viewpoints/information items, add standard mappings and register namespaced node/relation extensions. It MUST NOT weaken a non-waivable core invariant without creating a distinct incompatible profile family.

## 9. Immediate implementation order

1. Implement rule registry, result states, gate evaluator and deterministic finding keys.
2. Port existing v2 gap/inconsistency registry into F1/F2/F3/F4 rule namespaces.
3. Implement Q1 structural rules before building architecture generation.
4. Implement A1/A2 graph rules and candidate-aware validation.
5. Add OpenAPI/AsyncAPI/Arazzo validation artifacts for A3.
6. Implement A4 decision/justification rules.
7. Add slice/dependency validation D1 and verification planning D2.
8. Implement revision-bound receipts/staleness and conformance checks for C1.

## 10. What this deliberately does not claim

- The rulebook does **not** claim complete clause-by-clause certification to ISO/IEC/IEEE 29148, 42010, 12207, 15289 or 29119.
- It does **not** reproduce proprietary standard text.
- It does **not** make C4, ADR, EARS or ATAM formal standards.
- It does **not** treat a syntactically valid OpenAPI/BPMN/DMN artifact as proof that the software system is semantically correct.
- It does **not** let a readiness score override a failed blocker rule.

This separation is intentional: it makes Plumb credible to enterprise engineering organizations while preserving room for formal certification packs later if commercially justified.
