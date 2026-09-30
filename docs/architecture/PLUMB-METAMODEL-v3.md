# Plumb Metamodel v3.0 — Implementable Standards-First Specification

**Status:** Architecture baseline / implementation draft  
**Product:** Plumb 1.0  
**Metamodel version:** 3.0-draft.1  
**Purpose:** Define the canonical Plumb Specification Graph (PSG), its semantic types, relationships, invariants, provenance, standards mappings, and migration from the existing functional modeller v2.

---

## 1. Product model

Plumb is a **Software Specification Compiler**.

Its canonical engineering chain is:

> **Evidence → Intent → Function → Quality → Architecture → Contract → Work → Proof**

The canonical truth is the **Plumb Specification Graph (PSG)**. Documents, diagrams, BPMN/DMN files, OpenAPI/AsyncAPI/Arazzo descriptions, ADRs, implementation plans and reports are projections or interoperable representations of that graph.

### 1.1 Normative design rule

When a recognized engineering standard already defines a useful concept, Plumb SHOULD align to that concept rather than invent a competing one. Plumb-specific semantics SHOULD be introduced where standards do not provide the deterministic compilation, ambiguity management, provenance, human-decision, executable-scenario, delivery or conformance semantics required by the product.

### 1.2 Standards baseline

The first software profile is `plumb:software:2026.1`.

Core semantic alignment:

- ISO/IEC/IEEE 29148:2018 — requirements engineering. The profile MUST version-pin it because a DIS successor is already under development.
- ISO/IEC 25010:2023 — product quality model.
- ISO/IEC/IEEE 42010:2022 — architecture description.
- ISO/IEC/IEEE 12207:2026 — software life-cycle processes.
- ISO/IEC/IEEE 15289:2019 — life-cycle information items.
- ISO/IEC/IEEE 29119-2:2021 — software test processes.

Semantic/interchange alignment:

- OMG SBVR 1.5 — vocabulary and business-rule semantics.
- OMG BPMN 2.0.2 — business/process interoperability.
- OMG DMN 1.6 — decision interoperability.
- OMG SysML 2.0 — systems-model interoperability.
- OMG PPMN 1.0 — provenance/pedigree interoperability.
- INCITS 359-2012 / NIST RBAC model — role/permission concepts.

Software-contract interoperability:

- OpenAPI 3.2.1.
- AsyncAPI 3.0.0.
- Arazzo 1.1.0.
- JSON Schema.

Recognized practices, explicitly **not** claimed as formal standards:

- C4 model.
- ADR.
- EARS.
- ATAM / quality-attribute scenarios.
- BDD/Gherkin.

Optional enterprise-integration profiles:

- ArchiMate 3.2.
- TOGAF Standard, 10th Edition.

These SHOULD remain optional because their commercial licensing must be handled deliberately before Plumb embeds or distributes their proprietary metamodel/notation content.

---

## 2. Conformance language

Every external mapping SHALL declare a `mapping_strength`:

```text
exact        direct semantic equivalence claimed and validated
compatible   semantics intentionally compatible but not complete equivalence
subset       Plumb implements a controlled subset
extension    Plumb extends the external concept
inspired_by  terminology or method influenced the design; no conformance claim
```

Every standards mapping SHALL also declare a `mapping_role`:

```text
semantic_alignment
taxonomy
interchange
validation_reference
presentation_convention
```

These two vocabularies are closed. The Rust `MappingStrength` variants are `Exact`, `Compatible`, `Subset`, `Extension` and `InspiredBy`; the Rust `MappingRole` variants are `SemanticAlignment`, `Taxonomy`, `Interchange`, `ValidationReference` and `PresentationConvention`. Each serializes exactly as the lowercase identifier listed above, and unknown values are rejected. `taxonomy` is a mapping role, never a mapping strength. Standards profiles and rule metadata MUST use only these values.

Plumb MUST NOT describe a project or export as “ISO compliant”, “BPMN compliant”, “SysML compliant”, etc. unless the relevant conformance validator exists and the declared conformance target is actually satisfied.

Clause references MAY be stored as identifiers. Normative standard text MUST NOT be copied into the product unless licensing explicitly permits it.

---

## 3. Canonical graph envelope

The canonical persisted logical representation is equivalent to:

```yaml
spec_version: "3.0"
project:
  id: project:leave-management
  name: Leave Management
  profile: profile:plumb-software-2026.1
baseline:
  graph_revision: 42
  semantic_hash: "psg:sha256:..."
  evidence_hash: "ev:sha256:..."
nodes: []
edges: []
standards_profile: {}
```

The physical store MAY remain SQLite initially. The canonical serialization MUST be deterministic.

### 3.1 Canonicalization and hashes

Plumb SHOULD use RFC 8785 JSON Canonicalization Scheme after applying Plumb-specific ordering to semantically unordered arrays.

Three hashes are defined:

- `semantic_hash` — the baseline-participating specification semantics (§17.9; exact projection in implementation plan §6.2). Excludes timestamps, UI layout, transient jobs, cached inference text, other operational metadata, and the evidence, provenance, diagnostic and runtime/proof node types `SourceArtifact`, `EvidenceFragment`, `DerivationRecord`, `Agent`, `Finding`, `Question`, `ScenarioRun`, `TestExecution`, `TestReceipt`, `CodeBinding`, `ArchitectureCheck` and `CoverageRecord`.
- `evidence_hash` — baseline-participating source-artifact and evidence-fragment identity (exact projection in implementation plan §6.2).
- `view_hash` — one explicit view definition plus layout/style metadata.

Hash strings are normative (implementation plan §6.3). One `Hash` type accepts exactly `sha256:<64 lowercase hex>` (generic/content/artifact/config/rule/output hash), `psg:sha256:<64 lowercase hex>` (PSG semantic hash) and `ev:sha256:<64 lowercase hex>` (evidence hash).

This prevents diagram movement or audit timestamps from invalidating the software specification.

### 3.2 ID policy

All IDs are immutable.

Every `Id` MUST match the normative grammar `^[a-z][a-z0-9_-]*:[A-Za-z0-9._-]+(?::[A-Za-z0-9._-]+)*$` (implementation plan §6.3). Parsing performs no normalization: invalid strings, including strings with leading or trailing whitespace, are rejected rather than trimmed or repaired, and the exact validated string is preserved.

Readable aliases are optional and mutable; identifiers are not.

Recommended namespace conventions (each concrete ID MUST also satisfy the normative grammar above):

```text
src:<hash16>
evd:<hash16>
req:<namespace>:<key>
criterion:<req>:<key>
term:<namespace>:<key>
ent:<namespace>:<key>
attr:<entity>:<key>
op:<namespace>:<key>
proc:<namespace>:<key>
scn:<namespace>:<key>
quality:<namespace>:<key>
arch:<namespace>:<key>
component:<namespace>:<key>
apiop:<namespace>:<key>
slice:<namespace>:<key>
verify:<namespace>:<key>
resolution:<ulid>
```

Compiler-created IDs SHOULD be deterministic from stable parent IDs plus a semantic key. Human-created decisions MAY use ULIDs because the human action itself is an input recorded in the ledger.

---

## 4. Base semantic structures

### 4.1 Node

```rust
pub struct Node {
    pub id: Id,
    pub revision: u32,
    pub status: ElementStatus,
    pub payload: NodePayload,
    pub evidence: Vec<EvidenceRef>,
    pub derivations: Vec<DerivationRef>,
    pub standards: Vec<StandardMapping>,
    pub tags: BTreeSet<String>,
    pub extensions: BTreeMap<ExtensionKey, Value>,
    pub audit: AuditMeta,
}
```

`NodePayload` MUST be a tagged semantic enum. Core semantics MUST NOT be represented by `kind: String + props: BTreeMap`.

### 4.2 Edge

```rust
pub struct Edge {
    pub id: Id,
    pub revision: u32,
    pub status: ElementStatus,
    pub kind: RelationKind,
    pub from: Id,
    pub to: Id,
    pub properties: RelationProperties,
    pub evidence: Vec<EvidenceRef>,
    pub derivations: Vec<DerivationRef>,
    pub standards: Vec<StandardMapping>,
    pub audit: AuditMeta,
}
```

`Edge` rejects unknown fields and exposes a deterministic `Edge::validate()` with a typed error; deserialization enforces the same local invariants: `revision != 0`, valid `AuditMeta`, and compatibility of `kind` with `properties` (§17.8). Source/target node-type validation is registry validation (§17.8), because an edge carries only IDs. There is no universal confidence field.

### 4.3 Status

```text
Proposed
Accepted
Rejected
Superseded
Deprecated
Suspect
```

`Confirmed` from v2 becomes `Accepted`; `Confirmed` does not exist in v3. The Rust `ElementStatus` variants are exactly these six names, serialized as exactly these case-sensitive strings; unknown values are rejected.

`Suspect` means the element remains in the graph but its validity is questioned by a finding or invalidated dependency.

### 4.4 Confidence

Confidence is **not** a universal semantic field.

Confidence belongs to a `DerivationRecord`, proposal or inference. Once a human accepts a semantic assertion, that assertion's accepted state is authoritative for the current model; the original inference confidence remains available in provenance.

### 4.5 Envelope primitives and element revisions

The `Node` and `Edge` envelope primitives are normative:

- `EvidenceRef(Id)` and `DerivationRef(Id)` are distinct transparent wrappers over a validated `Id`. They serialize as the underlying ID string, preserve it exactly and impose no additional prefix or namespace restriction; that the referenced ID resolves to evidence or to a `DerivationRecord` is graph-semantic validation.
- `ExtensionKey` is a validated string matching exactly `^[a-z][a-z0-9_-]*:[A-Za-z0-9._-]+$`: exactly one colon, a lowercase-initial namespace, a non-empty local key, no whitespace, no trimming and no normalization.
- `AuditMeta` is:

```rust
pub struct AuditMeta {
    pub created_by: Id,
    pub created_at: Timestamp,
    pub updated_by: Option<Id>,
    pub updated_at: Option<Timestamp>,
}
```

`created_by` and `created_at` are mandatory. `updated_by` and `updated_at` are either both present or both absent; when present, `updated_at >= created_at`. Invalid audit metadata is rejected, never repaired. Audit metadata carries no confidence, deletion, session or free-form data, and is excluded from `semantic_hash`.

`Node.revision` and `Edge.revision` are element revision counters, not the global `GraphRevision` version. The type is `u32`; `0` is invalid; a newly created node or edge starts at `1`; an unchanged element keeps the same element revision when copied into a later `GraphRevision`; and patch/commit logic increments the element revision when that element is semantically changed.

An element revision is finalized once per applied `PatchSet` (§20) from the generic **element hash** (§20): for an element that existed in the base graph and survives the patch set, the final revision is `base revision + 1` when its final element hash differs from its base element hash and `base revision` otherwise; a newly added element has revision `1` throughout its first committed patch set, even if later sub-patches modify it. Revisions are never incremented per nested operation, so two edits to one element increment it once and an edit followed by its exact reversal leaves the original revision. Because the element hash covers `status`, `payload` (with View `layout_ref`/`style_ref` omitted), `evidence`, `standards`, `tags`/`extensions` (nodes) and `kind`, `from`, `to`, `properties` (edges), audit-only, derivation-only and View layout/style-only changes do not increment the revision; `id` never changes. An increment from `u32::MAX` fails the whole patch set.

---

## 5. Provenance and evidence namespace

### 5.1 `SourceArtifact`

Required:

```text
source_kind
display_name
content_hash
media_type
```

Optional:

```text
external_uri
external_version
producer
created_at_source
language
classification
```

Examples: DOCX, Markdown, PDF, transcript, Jira issue, Confluence page, API description, database schema, architecture file.

### 5.2 `EvidenceFragment`

Required:

```text
source_ref
locator
content_hash
```

`locator` is a tagged structure:

```text
TextRange { start, end }
PageRegion { page, x?, y?, width?, height? }
TableCell { table, row, column }
XmlPath { xpath }
JsonPointer { pointer }
ConversationTurn { turn_id }
ExternalObject { object_id, field? }
```

Optional:

```text
extracted_text
speaker
source_timestamp
```

Importer-specific facts such as a fragment's structural kind or a table row's header cells are not core fields; an importer records them in its own namespaced node extensions (for the pilot importers `plumb_import:source_artifacts` and `plumb_import:parse` on the source and `plumb_import:fragment` on each fragment). Parse completeness likewise stays an importer extension rather than a core `SourceArtifact` field.

### 5.3 `DerivationRecord`

This replaces the shallow v2 `Origin` enum as the actual provenance mechanism.

Required:

```text
id
kind
stage
input_refs[]
output_refs[]
created_at
```

Kinds:

```text
deterministic_rule
parser
import
human_edit
human_resolution
llm_inference
recovery
migration
external_sync
```

For `llm_inference`, additionally require:

```text
provider
model
prompt_template_hash
schema_hash
context_hash
parameters
raw_response_hash
validated_output_hash
```

Normative payload (§24.1):

```rust
pub struct DerivationRecord {
    pub id: Id,
    pub kind: DerivationKind,
    pub stage: String,
    pub input_refs: Vec<String>,
    pub output_refs: Vec<String>,
    pub created_at: Timestamp,

    pub provider: Option<String>,
    pub model: Option<String>,
    pub prompt_template_hash: Option<Hash>,
    pub schema_hash: Option<Hash>,
    pub context_hash: Option<Hash>,
    pub parameters: Option<Value>,
    pub raw_response_hash: Option<Hash>,
    pub validated_output_hash: Option<Hash>,
}
```

When `kind` is `llm_inference`, all eight LLM-specific fields are mandatory; for every other kind all eight MUST be absent. For `llm_inference`, `prompt_template_hash`, `schema_hash`, `context_hash`, `raw_response_hash` and `validated_output_hash` MUST be generic `sha256:` content hashes; semantic (`psg:sha256:`) or evidence (`ev:sha256:`) hashes there are invalid. Invalid combinations are rejected by explicit validation and by deserialization. `input_refs` and `output_refs` are strings because provenance inputs and outputs may identify either PSG IDs or content-addressed artifacts/hashes. `DerivationRecord` is a PSG payload (`NodePayload::DerivationRecord`); a node carrying it MUST have `Node.id == DerivationRecord.id`.

Reproducibility means rebuilding from source + accepted decisions + persisted inference artifacts, not re-calling a live model.

### 5.4 `Agent`

Represents a provenance actor:

```text
human
organization
software_service
llm_model
compiler_stage
external_system
```

Normative payload: `pub struct Agent { pub agent_kind: AgentKind }`. The node envelope ID identifies the actual actor, service, model or stage instance; the payload carries no name, provider details or free-form properties. `Agent` and `DerivationRecord` are defined once, in the PSG; the inference layer reuses them and never defines duplicates.

### 5.5 Standards mapping

PPMN interoperability SHOULD be implemented here as a mapping/export layer. Plumb MAY retain richer software-engineering-specific derivation fields internally.

---

## 6. Governance namespace

### 6.1 `Finding`

Required:

```text
code
family
severity
message
status
affected_refs[]
```

Optional:

```text
standard_rule_ref
suggested_resolution
waiver_ref
```

Families SHOULD include:

```text
requirements
vocabulary
domain
functional
process
decision
security
quality
architecture
contract
deployment
delivery
verification
provenance
standards
```

### 6.2 `Question`

Required:

```text
finding_ref
question_kind
prompt
status
```

Optional:

```text
answer_schema
stakeholder_ref
priority
round_ref
context_refs[]
```

Question kinds extend the v2 set:

```text
YesNo
PickOne
PickMany
Number
Text
Cardinality
Unit
Precision
Rounding
FormulaConfirm
RuleCell
Calendar
RoleAssignment
Permission
QualityThreshold
ArchitectureChoice
TechnologyChoice
InterfaceChoice
VerificationMethod
```

### 6.3 `ResolutionDecision`

This is the accepted human resolution of an ambiguity, question or proposal. It is deliberately distinct from a DMN business decision and from `ArchitectureDecision`.

Required:

```text
question_ref?
proposal_ref?
answer
decided_by
decided_at
patch_ref
```

Optional:

```text
rationale
supersedes
```

**Waiver decisions.** A `ResolutionDecision` is a governed waiver of a validation finding only when its `answer` is exactly this object (unknown fields rejected):

```json
{"kind": "waiver", "rule_id": "<rule ID>", "finding_key": "sha256:...", "finding_ref": "fnd:..."}
```

`finding_key` is the generic deterministic finding key and `finding_ref` the deterministic `Finding` node ID derived from it (compiler architecture §29). The waiver is effective only when the decision is `Accepted`, has a non-empty `rationale`, and an `Accepted` `resolves` edge links it to that `Accepted` `Finding`; `decided_by` is the waiver owner and the finding is its scope. Because graph validity does not derive a `Finding` node's ID from its payload, a waiver is accepted only when that `Finding` carries the evaluated identity: `code` equal to the rule ID, `family` equal to the gate and `affected_refs` equal to the sorted violation targets. Any other `answer` is not a waiver. `ResolutionDecision` has no review or expiry field, so the pilot evaluates no waiver expiry; a time-bounded waiver requires a future versioned schema.

### 6.4 `Assumption`

Required:

```text
statement
owner_ref
status
```

Optional:

```text
finding_ref
default_value
expires_at
risk_ref
```

---

## 7. Intent and requirements namespace

### 7.1 `Stakeholder`

Required:

```text
name
stakeholder_kind
```

Optional:

```text
organization
responsibilities[]
contact_ref
```

### 7.2 `Concern`

Required:

```text
name
description
```

Used by both requirements and architecture views.

### 7.3 `Goal`

Required:

```text
statement
```

Optional:

```text
success_measures[]
priority
```

### 7.4 `Need`

Required:

```text
statement
stakeholder_refs[]
```

Optional:

```text
goal_refs[]
context
```

### 7.5 `Requirement`

Required:

```text
statement
requirement_kind
level
modality
```

Enums:

```text
requirement_kind:
  functional
  quality
  interface
  data
  security
  operational
  compliance
  transition
  constraint

level:
  stakeholder
  system
  software
  subsystem
  component
  interface

modality:
  shall
  should
  may
  shall_not
```

Optional:

```text
title
rationale
priority
source_identifier
verification_method
owner_refs[]
stakeholder_refs[]
```

ISO/IEC/IEEE 29148 mapping: `semantic_alignment`, normally `compatible`. Plumb does not claim full 29148 conformance solely because this node exists.

### 7.6 `AcceptanceCriterion`

Required:

```text
statement
criterion_kind
```

Optional:

```text
verification_method
measure_ref
scenario_ref
```

### 7.7 `Constraint`

Required:

```text
statement
constraint_category
strength
```

Categories:

```text
business
technical
technology
security
data
integration
operational
legal
regulatory
organizational
legacy
```

Strength:

```text
mandatory
preferred
prohibited
```

A technology **constraint** and a technology **selection** are separate concepts.

---

## 8. Vocabulary and domain namespace

### 8.1 `Term`

Required:

```text
term
language
```

Optional:

```text
definition_ref
aliases[]
status
```

### 8.2 `Concept`

Required:

```text
name
definition
concept_kind
```

Kinds:

```text
object_type
fact_type
value_type
role
other
```

SBVR mapping is `compatible` or `subset`; Plumb SHOULD export richer SBVR-compatible vocabularies later without forcing the entire SBVR metamodel into the core.

### 8.3 `Actor`

Required:

```text
name
actor_kind
```

Kinds:

```text
human
system
external_system
organization
```

### 8.4 `BusinessRole`

Required:

```text
name
```

This captures business responsibility and MUST NOT be conflated with a security authorization role.

### 8.5 `Entity`

Required:

```text
name
```

Optional:

```text
description
aggregate_root
```

### 8.6 `Attribute`

Required:

```text
name
value_type
nullable
```

Optional:

```text
unit
precision
enum_values[]
data_classification
```

The owning entity is the canonical `has_attribute` relation (Entity -> Attribute), not a payload field.

### 8.7 `DomainRelationship`

Required:

```text
from_entity
to_entity
relationship_kind
cardinality_from
cardinality_to
```

Optional:

```text
name
snapshot_semantics
ownership
```

### 8.8 `State`

Required:

```text
name
```

The state owner is the canonical `has_state` relation (StateOwner -> State), not a payload field.

### 8.9 `Transition`

Required:

```text
stateful_ref
from_state
to_state
```

Optional:

```text
guard_expr
effect_refs[]
```

`from_state` and `to_state` are State node IDs (`Id`). The trigger is the canonical `transitions_via` relation (Transition -> Operation/Event), exactly one per transition, not a payload field.

### 8.10 `Invariant`

Required:

```text
scope_ref
expression
```

`expression` uses PlumbExpr.

---

## 9. Functional behaviour namespace

### 9.1 `Operation`

Required:

```text
name
operation_kind
```

Kinds:

```text
command
query
```

Optional:

```text
input_schema_ref
output_schema_ref
preconditions[]
postconditions[]
idempotency
transaction_semantics
```

Reads/writes, actors, permissions, rules and outcomes are graph relationships, not duplicated arrays inside the payload.

### 9.2 `Outcome`

Required:

```text
name
outcome_kind
```

Kinds:

```text
success
business_failure
technical_failure
partial
```

### 9.3 `Event`

Required:

```text
name
```

Optional:

```text
payload_schema_ref
semantic_type
```

### 9.4 `Process`

Required:

```text
name
```

Optional:

```text
description
process_kind
```

### 9.5 `ProcessNode`

Required:

```text
process_ref
node_kind
```

Controlled subset:

```text
start
end
human_task
service_task
exclusive_gateway
parallel_split
parallel_join
message_event
timer_event
error_event
subprocess
```

Optional:

```text
operation_ref
condition_expr
message_ref
timer_expr
```

The performer is the canonical `performed_by` relation (ProcessNode -> Actor/BusinessRole), not a payload field.

Plumb's process semantics are an executable subset with BPMN 2.0.2 interoperability, not a full BPMN engine.

### 9.6 `Rule`

Required:

```text
name
rule_kind
```

Kinds:

```text
constraint
derivation
permission
validation
business
```

Optional:

```text
condition_expr
action_expr
```

### 9.7 `DecisionTable`

Required:

```text
name
hit_policy
inputs[]
outputs[]
rows[]
```

Plumb MUST deterministically validate overlap and coverage where the input domain is enumerable or interval-analyzable.

DMN mapping is `subset` initially. Import/export MAY later support DMN XML.

### 9.8 `Calculation`

Required:

```text
name
expression
result_type
```

Optional:

```text
unit
rounding
calendar_ref
examples[]
```

### 9.9 `Calendar`

Required:

```text
time_zone
week_pattern
```

Optional:

```text
region
holiday_source
```

### 9.10 `Scenario`

Required:

```text
name
scenario_kind
given[]
when
then[]
```

Kinds:

```text
acceptance
boundary
failure
state_transition
rule_row
quality
integration
regression
```

Optional:

```text
requirement_refs[]
derived_from_refs[]
confirmation_status
```

`Scenario` is specification. `ScenarioRun` is execution evidence and lives in verification.

---

## 10. Authorization namespace

The v2 `role.actor + operations[]` model is replaced by separate business and authorization semantics.

### 10.1 `Principal`

Represents a security subject category or concrete integration principal.

Required:

```text
name
principal_kind
```

### 10.2 `SecurityRole`

Required:

```text
name
```

Optional:

```text
description
```

### 10.3 `Permission`

Required:

```text
name
```

The operation, resource scope and policy conditions are the canonical `permits`, `scoped_to` and `conditioned_by` relations, not payload fields.

### 10.4 `ResourceScope`

Required:

```text
resource_ref
scope_kind
```

Examples:

```text
all
owned
organizational_unit
relationship_based
expression
```

### 10.5 `PolicyCondition`

Required:

```text
expression
```

### 10.6 `SeparationConstraint`

Required:

```text
constraint_kind
role_refs[]
```

Kinds:

```text
static_separation_of_duty
dynamic_separation_of_duty
mutual_exclusion
required_combination
```

Core concepts align to INCITS 359/NIST RBAC. Attribute-based policy can be added later without corrupting this role model.

---

## 11. Quality namespace

The v2 generic `nfr` bags are removed from the canonical model.

### 11.1 `QualityCharacteristic`

Required:

```text
name
scheme
```

Default scheme:

```text
ISO/IEC 25010:2023
```

Plumb SHOULD ship the nine product-quality characteristics as a profile taxonomy, not hard-code them into Rust enums, so profile updates do not require a core release.

### 11.2 `Measure`

Required:

```text
name
measure_type
unit
```

Optional:

```text
aggregation
sampling_window
measurement_method
```

### 11.3 `QualityScenario`

Required:

```text
stimulus
response
threshold
```

The characteristic and measure are the canonical `characterized_by` and `measured_by` relations (exactly one each), not payload fields.

Optional:

```text
source_ref
environment_condition
affected_refs[]
priority
```

Example:

```yaml
# characterized_by -> quality:performance-efficiency; measured_by -> measure:p95-latency
stimulus: "5000 concurrent users submit leave requests"
environment_condition: "normal production operation"
affected_refs: [api:leave]
response: "requests are accepted and processed"
threshold: "<= 400 ms"
```

Quality scenarios are architecture drivers.

---

## 12. Architecture namespace

### 12.1 `SystemOfInterest`

Required:

```text
name
```

### 12.2 `ArchitectureDescription`

Required:

```text
system_of_interest_ref
```

Optional:

```text
purpose
scope
architecture_candidate_ref
```

### 12.3 `ArchitectureCandidate`

Required:

```text
name
status
```

Statuses:

```text
exploring
candidate
accepted
rejected
superseded
```

An architecture candidate is a branch/overlay over an accepted functional baseline.

### 12.4 `Viewpoint`

Required:

```text
name
concern_refs[]
stakeholder_refs[]
model_kind_refs[]
```

Optional:

```text
purpose
conventions
```

### 12.5 `View`

This is the single normative `View` payload (it supersedes the separate field lists previously given here and in §19):

```rust
pub struct View {
    pub name: String,
    pub viewpoint_ref: Id,
    pub architecture_description_ref: Id,

    pub root_refs: Option<Vec<Id>>,
    pub filter: Option<Value>,

    pub projection_rules: Vec<String>,
    pub layout_ref: Option<Hash>,
    pub style_ref: Option<Hash>,
}
```

`projection_rules` are semantic view-definition rules. `layout_ref` and `style_ref` point to separately stored and separately hashed view metadata; coordinates, routes and style values are never embedded in the `View` payload. `semantic_hash` of a `View` is computed from a projection of the payload in which `layout_ref` and `style_ref` are absent (not serialized as `null`), so changing only layout/style metadata does not change `semantic_hash`.

### 12.6 `ModelKind`

Required:

```text
name
semantic_scope
```

Examples:

```text
system-context
container
component
process
state
sequence
deployment
rbac
traceability
```

### 12.7 Architecture element types

All share:

```text
name
```

Concrete types:

```text
SoftwareSystem
Container
Component
Module
Interface
DataStore
ExternalSystem
DeploymentNode
RuntimeEnvironment
NetworkZone
```

Optional common fields:

```text
description
responsibilities[]
owner_ref
```

Technology selections are the canonical `uses_technology` relation, not a payload field.

### 12.8 `ArchitectureDecision`

Required:

```text
question
status
alternatives[]
selected_option
rationale
```

Drivers are the canonical `justified_by` relation, not a payload field.

Optional:

```text
positive_consequences[]
negative_consequences[]
affected_refs[]
supersedes
```

Markdown ADR is a projection of this node.

### 12.9 `Technology`

Required:

```text
name
technology_kind
```

Optional:

```text
vendor
version_scheme
```

### 12.10 `TechnologySelection`

Required:

```text
technology_ref
status
```

Statuses:

```text
candidate
selected
rejected
legacy
prohibited
```

Optional:

```text
version_range
alternatives[]
rationale
architecture_decision_ref
```

The elements a selection applies to are the inverse of `uses_technology`, and its drivers are `justified_by`; neither is a payload field.

### 12.11 ISO 42010 semantics

Plumb SHOULD explicitly model stakeholder, concern, viewpoint, view, model kind, entity/system of interest and architecture description concepts.

The first Plumb architecture profile SHOULD claim `compatible` semantic alignment, not blanket ISO 42010 conformance, until a clause-level architecture-description validator is implemented.

### 12.12 C4 / SysML / ArchiMate position

C4 is a presentation convention over `SoftwareSystem`, `Container`, `Component`, relationships and views.

SysML v2 is an interoperability profile, especially for requirement, part/component, interface, action/behavior, state, constraint and verification concepts.

ArchiMate 3.2 MAY be an enterprise interoperability profile for capabilities, applications, technology and cross-domain views, subject to commercial licensing.

---

## 13. Technical-contract namespace

### 13.1 `ApiContract`

Required:

```text
name
contract_kind
```

Kinds:

```text
http
rpc
other
```

Optional:

```text
version
base_uri
external_spec_ref
```

### 13.2 `ApiOperation`

Required:

```text
api_contract_ref
operation_id
```

For HTTP:

```text
method
path
```

Optional:

```text
security_refs[]
```

Request, response and error schemas are the canonical `schema_for` relation (DataSchema -> ApiOperation) with roles `api_request`, `api_response` and `api_error`, not payload fields.

OpenAPI is the preferred HTTP interchange projection.

### 13.3 `EventContract`

Required:

```text
name
```

Optional:

```text
external_spec_ref
```

### 13.4 `Channel`

Required:

```text
name
address
```

Optional:

```text
protocol
```

### 13.5 `Message`

Required:

```text
name
```

Optional:

```text
correlation_ref
```

Payload and header schemas are the canonical `schema_for` relation (DataSchema -> Message) with roles `message_payload` and `message_headers`, not payload fields.

AsyncAPI is the preferred message/event interchange projection.

### 13.6 `DataSchema`

Required:

```text
name
schema_kind
```

Kinds MAY include:

```text
json_schema
avro
protobuf
sql
logical
```

Optional:

```text
external_ref
inline_schema
```

### 13.7 `TechnicalWorkflow`

Required:

```text
name
```

Steps are the canonical `workflow_step` relation (TechnicalWorkflow -> ApiOperation), not a payload field.

Used for concrete API-call sequences and dependencies. Arazzo is the preferred HTTP/API workflow projection.

A business `Process` and a `TechnicalWorkflow` are not the same thing.

---

## 14. Delivery namespace

### 14.1 `Capability`

Required:

```text
name
```

Optional:

```text
goal_refs[]
requirement_refs[]
```

### 14.2 `ImplementationSlice`

Required:

```text
name
intent
```

Must be linked through graph edges to its functional obligations and technical allocation.

Optional payload:

```text
sequence_hint
size_hint
```

The actual scope is expressed through relationships to requirements, operations, components, contracts, data, decisions, constraints and verification obligations.

### 14.3 `WorkPackage`

Required:

```text
name
```

Optional:

```text
owner_ref
target_release_ref
```

### 14.4 `TaskContract`

Required:

```text
name
intent
must[]
must_not[]
allowed_change_refs[]
forbidden_change_refs[]
completion_criteria[]
```

Optional:

```text
implementation_slice_ref
decision_refs[]
verification_refs[]
```

A TaskContract is the safe implementation handoff to a developer or coding agent.

### 14.5 `Migration`

Required:

```text
name
migration_kind
source_ref
target_ref
```

### 14.6 `Release`

Required:

```text
name
version
```

Optional:

```text
target_date
```

Slices are the canonical `contains` relation (Release -> ImplementationSlice), not a payload field.

---

## 15. Verification and conformance namespace

### 15.1 `VerificationObligation`

Required:

```text
name
verification_kind
```

Kinds:

```text
test
scenario
analysis
inspection
review
demonstration
formal_check
architecture_check
security_check
```

Optional:

```text
method
acceptance_condition
required_evidence_kind
```

Targets are the inverse of the canonical `verified_by` relation, not a payload field. This replaces the v2 idea that every requirement must necessarily have an executed passing scenario. Verification method depends on requirement semantics.

### 15.2 `TestCase`

Required:

```text
name
steps[]
expected[]
```

Optional:

```text
automation_ref
```

Verification obligations are the inverse of the canonical `implemented_as` relation, not a payload field.

### 15.3 `ScenarioRun`

Required:

```text
scenario_ref
specification_hash
result
trace[]
```

Results:

```text
pass
fail
undecidable
```

### 15.4 `TestExecution`

Required:

```text
test_case_ref
result
started_at
finished_at
```

### 15.5 `TestReceipt`

Required:

```text
verification_obligation_ref
specification_hash
code_revision
test_revision
environment_ref
result
executed_at
```

Optional:

```text
artifact_hashes[]
logs_ref
runner_identity
```

### 15.6 `CodeBinding`

Required:

```text
repository_ref
code_locator
code_revision
```

The bound semantic element is the inverse of the canonical `bound_to_code` relation, not a payload field.

Locators MAY describe files, symbols, modules, packages or generated artifacts.

### 15.7 `ArchitectureCheck`

Required:

```text
rule_code
architecture_hash
result
affected_refs[]
```

### 15.8 `CoverageRecord`

Required:

```text
coverage_kind
source_refs[]
target_refs[]
status
```

### 15.9 Evidence staleness

Verification evidence becomes `stale` when a dependency-reachability calculation shows a relevant semantic element, contract, test, code binding or environment assumption has changed since the receipt's bound hashes/revisions.

Stale evidence MUST NOT count as current proof.

---

## 16. Standards-profile namespace

### 16.1 `StandardsProfile`

Required:

```text
id
name
version
standards[]
validation_packs[]
```

Each standard reference:

```yaml
standard_id: ISO/IEC/IEEE 42010
version: "2022"
role: semantic_alignment
required: true
```

### 16.2 `StandardMapping`

```rust
pub struct StandardMapping {
    pub standard_id: String,
    pub version: String,
    pub concept: String,
    pub clause_ref: Option<String>,
    pub mapping_role: MappingRole,
    pub mapping_strength: MappingStrength,
    pub validator_rules: Vec<String>,
}
```

### 16.3 Rule namespaces

Example validation codes:

```text
ISO29148.REQ.NO_EVIDENCE
ISO29148.REQ.NO_VERIFICATION_METHOD
ISO25010.QUALITY.NO_CHARACTERISTIC
ISO25010.QUALITY.NO_MEASURE
ISO42010.CONCERN.UNADDRESSED
ISO42010.VIEW.NO_VIEWPOINT
BPMN.PROCESS.UNREACHABLE_NODE
DMN.TABLE.OVERLAP
DMN.TABLE.INCOMPLETE
RBAC.PERMISSION.NO_ROLE
PLUMB.FUNCTION.UNALLOCATED
PLUMB.COMPONENT.NO_JUSTIFICATION
PLUMB.API.NO_FUNCTION
PLUMB.EVENT.NO_CONSUMER
PLUMB.QUALITY.NO_ARCH_RESPONSE
PLUMB.TEST.STALE_EVIDENCE
```

The UI MUST distinguish:

```text
external-standard validation
Plumb semantic validation
organization policy
industry profile
```

---

## 17. Core relation vocabulary

Core relation kinds are closed and typed. Extensions MAY introduce namespaced relations.

**Canonical relation ownership.** When this section defines a core typed relation for a cross-node semantic association, that `Edge` is the canonical representation. The same association MUST NOT also be persisted as a duplicate payload reference field. Payload fields remain only where they express intrinsic structure for which this section has no equivalent relation, so graph validation never has to decide which of two conflicting representations is authoritative. In particular these associations exist only as relations: Attribute owner (`has_attribute`), State owner (`has_state`), Transition trigger (`transitions_via`), ProcessNode performer (`performed_by`), Permission operation/scope/conditions (`permits`, `scoped_to`, `conditioned_by`), QualityScenario characteristic/measure (`characterized_by`, `measured_by`), architecture element technology (`uses_technology`), ArchitectureDecision/TechnologySelection drivers (`justified_by`), TechnologySelection targets (inverse `uses_technology`), TechnicalWorkflow steps (`workflow_step`), Release slices (`contains`), VerificationObligation targets (inverse `verified_by`), TestCase obligations (inverse `implemented_as`), CodeBinding element (inverse `bound_to_code`) and ApiOperation/Message schemas (`schema_for`). `Operation.input_schema_ref`, `Operation.output_schema_ref` and `Event.payload_schema_ref` remain payload fields because no relation targets Operation/Event schemas.

### 17.1 Evidence and governance

| Relation | From | To | Cardinality / rule |
|---|---|---|---|
| `evidenced_by` | any semantic element | EvidenceFragment | Accepted requirements SHOULD have >=1 |
| `derived_from` | any derived element | semantic/evidence element | 0..* |
| `supersedes` | versionable semantic element | same compatible type | 0..1 outgoing |
| `conflicts_with` | semantic element | semantic element | symmetric |
| `resolves` | ResolutionDecision | Question/Finding | 1..* |
| `raises` | Finding | Question | 0..* |

### 17.2 Intent and traceability

| Relation | From | To | Rule |
|---|---|---|---|
| `addresses` | Requirement/ArchitectureElement/View | Concern/Goal | typed |
| `refines` | Requirement | Requirement | no cycles unless profile permits decomposition loop (normally forbidden) |
| `decomposes_to` | Requirement/Capability | Requirement/Capability | DAG |
| `specified_by` | Requirement | Operation/Process/Rule/QualityScenario | 0..* |
| `constrained_by` | semantic element | Constraint | 0..* |
| `satisfied_by` | Requirement | ArchitectureElement/Contract | 0..* |

### 17.3 Domain and function

| Relation | From | To | Rule |
|---|---|---|---|
| `has_attribute` | Entity | Attribute | Attribute has exactly one owning Entity in core profile |
| `has_state` | Entity/Process/etc. | State | 0..* |
| `transitions_via` | Transition | Operation/Event | exactly 1 |
| `performed_by` | Operation/ProcessNode | Actor/BusinessRole | 1..* for executable business steps |
| `reads` | Operation | Attribute/Entity | 0..* |
| `writes` | Operation | Attribute/Entity | 0..* |
| `produces` | Operation/ProcessNode | Outcome/Event | 0..* |
| `consumes` | Operation/ProcessNode | Event | 0..* |
| `governed_by` | Operation/ProcessNode | Rule/DecisionTable | 0..* |
| `uses_calculation` | Operation/Rule | Calculation | 0..* |
| `next` | ProcessNode | ProcessNode | validated according to node kind |

### 17.4 Authorization

| Relation | From | To | Rule |
|---|---|---|---|
| `assigned_role` | Principal/Actor | SecurityRole | 0..* |
| `inherits_role` | SecurityRole | SecurityRole | acyclic |
| `grants` | SecurityRole | Permission | 0..* |
| `permits` | Permission | Operation | exactly 1 in core profile |
| `scoped_to` | Permission | ResourceScope | exactly 1 |
| `conditioned_by` | Permission | PolicyCondition | 0..* |

### 17.5 Quality and architecture

| Relation | From | To | Rule |
|---|---|---|---|
| `characterized_by` | QualityScenario | QualityCharacteristic | exactly 1 |
| `measured_by` | QualityScenario | Measure | exactly 1 |
| `drives` | QualityScenario/Constraint | ArchitectureDecision/ArchitectureCandidate | 0..* |
| `allocated_to` | Operation/Process/Entity responsibility | ArchitectureElement | 1..* by gate A2 |
| `depends_on` | ArchitectureElement | ArchitectureElement | 0..* |
| `exposes` | ArchitectureElement | Interface/ApiOperation/Channel | 0..* |
| `stores_in` | Component/Container | DataStore | 0..* |
| `deployed_to` | deployable ArchitectureElement | DeploymentNode/RuntimeEnvironment | 1..* by deployment profile |
| `uses_technology` | ArchitectureElement | TechnologySelection | 0..* |
| `justified_by` | ArchitectureDecision/TechnologySelection | Requirement/QualityScenario/Constraint | 1..* for accepted decisions |

### 17.6 Contracts

| Relation | From | To | Rule |
|---|---|---|---|
| `exposed_by` | Operation | ApiOperation | 0..* |
| `publishes` | Operation/Component | Message/Event | 0..* |
| `subscribes_to` | Operation/Component | Message/Channel | 0..* |
| `schema_for` | DataSchema | Attribute/Message/ApiOperation | 0..* |
| `workflow_step` | TechnicalWorkflow | ApiOperation | 1..* |

### 17.7 Delivery and proof

| Relation | From | To | Rule |
|---|---|---|---|
| `implemented_by` | Requirement/Operation/ArchitectureElement | ImplementationSlice | 0..* |
| `contains` | WorkPackage/Release | ImplementationSlice | 0..* |
| `depends_on_slice` | ImplementationSlice | ImplementationSlice | DAG unless explicit cyclic migration profile |
| `verified_by` | Requirement/Operation/Contract/ArchitectureElement | VerificationObligation | 1..* by D2/C1 profile |
| `implemented_as` | VerificationObligation | TestCase/Scenario/ArchitectureCheck | 0..* |
| `produces_receipt` | TestExecution/ScenarioRun | TestReceipt | 0..1 |
| `bound_to_code` | semantic element | CodeBinding | 0..* |

### 17.8 Relation registry contract (normative)

**Wire model.** `RelationKind` is a closed enum of exactly the 51 core relations above plus `Extension(ExtensionKey)`. It serializes as a single JSON string: core relations as their exact snake_case name, extension relations as their `ExtensionKey` (for example `"acme:depends_on"`). A known unqualified core name deserializes to the core variant; a valid namespaced `ExtensionKey` to `Extension`; an unknown unqualified name or a malformed namespace is rejected. There is no `Other(String)` fallback.

**Edge properties.** `Edge.properties` is `RelationProperties`, serialized as a JSON object:

```rust
pub enum RelationProperties {
    None,                                         // {}
    SchemaFor(SchemaForProperties),               // {"role": "api_request"}
    Extension(BTreeMap<ExtensionKey, Value>),     // {"acme:criticality": "high"}
}

pub struct SchemaForProperties { pub role: SchemaBindingRole }

pub enum SchemaBindingRole {
    Attribute,       // attribute
    MessagePayload,  // message_payload
    MessageHeaders,  // message_headers
    ApiRequest,      // api_request
    ApiResponse,     // api_response
    ApiError,        // api_error
}
```

The Rust variant name never appears in JSON. Every core relation except `schema_for` uses `None` (`{}`); `schema_for` uses `SchemaFor` with no additional fields; extension relations use `Extension`, whose keys are all valid `ExtensionKey`s and which may be empty. Any other combination is invalid, and core semantic data MUST NOT be placed in property maps. `schema_for` role compatibility: DataSchema -> Attribute requires `attribute`; DataSchema -> Message requires `message_payload` or `message_headers`; DataSchema -> ApiOperation requires `api_request`, `api_response` or `api_error`.

**Node categories** (by `NodeType`, never by Rust type name): *Evidence* = SourceArtifact, EvidenceFragment. *Provenance* = DerivationRecord, Agent. *SemanticNode* = every node type except SourceArtifact, EvidenceFragment, DerivationRecord and Agent. *SemanticOrEvidenceNode* = every node type except DerivationRecord and Agent. *ArchitectureElement* = SoftwareSystem, Container, Component, Module, Interface, DataStore, ExternalSystem, DeploymentNode, RuntimeEnvironment, NetworkZone. *TechnicalContract* = ApiContract, ApiOperation, EventContract, Channel, Message, DataSchema, TechnicalWorkflow. *StateOwner* = Entity, Process and every ArchitectureElement type. *DeployableArchitectureElement* = SoftwareSystem, Container, Component, Module, DataStore.

**Registry.** A static machine-readable registry holds exactly one `RelationDef { kind, from, to, outgoing, incoming, directionality, cycle_policy, same_node_type, property_schema }` per core relation, with `Cardinality { min: u32, max: Option<u32> }` (`None` = unbounded), `Directionality { Directed, Symmetric }`, `CyclePolicy { Allowed, Acyclic }` and `RelationPropertySchema { None, SchemaFor }`. Extension relations have no registry entries.

**Exact source/target compatibility:**

| Relation | From | To |
|---|---|---|
| `evidenced_by` | SemanticNode | EvidenceFragment |
| `derived_from` | SemanticNode | SemanticOrEvidenceNode |
| `supersedes` | any node type | the same node type (`same_node_type`) |
| `conflicts_with` | SemanticNode | SemanticNode (symmetric) |
| `resolves` | ResolutionDecision | Question, Finding |
| `raises` | Finding | Question |
| `addresses` | Requirement, ArchitectureElement, View | Concern, Goal |
| `refines` | Requirement | Requirement |
| `decomposes_to` | Requirement, Capability | Requirement, Capability |
| `specified_by` | Requirement | Operation, Process, Rule, QualityScenario |
| `constrained_by` | SemanticNode | Constraint |
| `satisfied_by` | Requirement | ArchitectureElement, TechnicalContract |
| `has_attribute` | Entity | Attribute |
| `has_state` | StateOwner | State |
| `transitions_via` | Transition | Operation, Event |
| `performed_by` | Operation, ProcessNode | Actor, BusinessRole |
| `reads` | Operation | Attribute, Entity |
| `writes` | Operation | Attribute, Entity |
| `produces` | Operation, ProcessNode | Outcome, Event |
| `consumes` | Operation, ProcessNode | Event |
| `governed_by` | Operation, ProcessNode | Rule, DecisionTable |
| `uses_calculation` | Operation, Rule | Calculation |
| `next` | ProcessNode | ProcessNode |
| `assigned_role` | Principal, Actor | SecurityRole |
| `inherits_role` | SecurityRole | SecurityRole |
| `grants` | SecurityRole | Permission |
| `permits` | Permission | Operation |
| `scoped_to` | Permission | ResourceScope |
| `conditioned_by` | Permission | PolicyCondition |
| `characterized_by` | QualityScenario | QualityCharacteristic |
| `measured_by` | QualityScenario | Measure |
| `drives` | QualityScenario, Constraint | ArchitectureDecision, ArchitectureCandidate |
| `allocated_to` | Operation, Process, Entity | ArchitectureElement |
| `depends_on` | ArchitectureElement | ArchitectureElement |
| `exposes` | ArchitectureElement | Interface, ApiOperation, Channel |
| `stores_in` | Component, Container | DataStore |
| `deployed_to` | DeployableArchitectureElement | DeploymentNode, RuntimeEnvironment |
| `uses_technology` | ArchitectureElement | TechnologySelection |
| `justified_by` | ArchitectureDecision, TechnologySelection | Requirement, QualityScenario, Constraint |
| `exposed_by` | Operation | ApiOperation |
| `publishes` | Operation, Component | Message, Event |
| `subscribes_to` | Operation, Component | Message, Channel |
| `schema_for` | DataSchema | Attribute, Message, ApiOperation |
| `workflow_step` | TechnicalWorkflow | ApiOperation |
| `implemented_by` | Requirement, Operation, ArchitectureElement | ImplementationSlice |
| `contains` | WorkPackage, Release | ImplementationSlice |
| `depends_on_slice` | ImplementationSlice | ImplementationSlice |
| `verified_by` | Requirement, Operation, ArchitectureElement, TechnicalContract | VerificationObligation |
| `implemented_as` | VerificationObligation | TestCase, Scenario, ArchitectureCheck |
| `produces_receipt` | TestExecution, ScenarioRun | TestReceipt |
| `bound_to_code` | SemanticNode | CodeBinding |

**Structural cardinality** (unconditional only): `supersedes` outgoing 0..1; `resolves` outgoing 1..*; `has_attribute` incoming (per Attribute) exactly 1; `transitions_via`, `permits`, `scoped_to`, `characterized_by` and `measured_by` outgoing exactly 1; `workflow_step` outgoing 1..*; `produces_receipt` outgoing 0..1. Every other relation is outgoing 0..* and incoming 0..*. A minimum applies to every node whose type matches the relation's source (outgoing) or target (incoming) predicate. Conditional gate/profile requirements (evidence for requirements, `performed_by` for executable business steps, `allocated_to` by A2, `deployed_to` by a deployment profile, `justified_by` for accepted decisions, `verified_by` by D2/C1) are later validation rules, not structural cardinality.

**Directionality and cycles.** `conflicts_with` is `Symmetric`: one edge represents the unordered pair and no mirrored edge is required or created. Every other core relation is `Directed`. `supersedes`, `refines`, `decomposes_to`, `inherits_role` and `depends_on_slice` are `Acyclic` in the core profile (a future exception requires an explicit plan revision/profile mechanism); every other relation allows cycles unless a later semantic rule forbids them.

### 17.9 Graph validity boundary (normative)

**Baseline-participating statuses.** `Accepted`, `Suspect`, `Superseded` and `Deprecated` are *baseline-participating*: such elements participate in structural graph constraints (cardinality, cycles), in duplicate-semantic-edge checks, and are candidates for the semantic and evidence hash projections. `Proposed` and `Rejected` elements are still persisted graph elements and MUST still pass local Node/Edge validation, ID validation, endpoint existence, relation source/target type compatibility, relation-property compatibility and typed evidence/derivation reference resolution; they simply do not count toward baseline cardinalities, cycles, duplicate semantic relations or hashes.

**Baseline edge endpoints.** An edge whose status is baseline-participating MUST have both endpoints baseline-participating. A `Proposed` edge may point to `Proposed` or baseline nodes; a `Rejected` edge need only point to existing, structurally valid nodes.

**Shape versus constraints.** Relation *shape* validation (edge local validation, endpoint existence, source/target type, same-node-type, `schema_for` role) runs over all persisted edges regardless of status. Relation *constraint* validation (unconditional cardinality and the five core-acyclic relations) runs over the baseline-participating subset only.

**Canonical `conflicts_with` orientation.** A `conflicts_with` edge MUST satisfy `from < to` by exact `Id` ordering, for every edge status. The reverse orientation and self-conflicts are invalid, so each unordered pair has exactly one stored representation.

**Duplicate semantic edges.** A baseline graph MUST NOT contain two distinct baseline-participating edges with the same *semantic relation key*: `(kind, from, to)` for core relations (including canonically oriented `conflicts_with`); `(schema_for, from, to, role)` for `schema_for`; and `(extension key, from, to, RFC 8785 canonical properties JSON)` for extension relations. Edge id, revision, status, evidence, derivations, standards and audit are not part of the key. Proposed duplicates may coexist while under review.

**Envelope collection uniqueness.** Within one node or edge, `evidence`, `derivations` and `standards` MUST NOT contain exact duplicates; local validation and deserialization reject them. Their persisted order is a presentation detail; the semantic hash projection normalizes it.

**Provenance references.** Every `EvidenceRef` MUST resolve to an existing `EvidenceFragment` node and every `DerivationRef` to an existing `DerivationRecord` node; when the owning node/edge is baseline-participating, the referenced node MUST be baseline-participating too. Every `EvidenceFragment.source_ref` MUST resolve to an existing `SourceArtifact`, baseline-participating when the fragment is. `SourceArtifact.content_hash` and `EvidenceFragment.content_hash` MUST be generic `sha256:` hashes.

**Deterministic evidence identity.** A `SourceArtifact` node ID MUST equal `src:<first 16 hex of its content_hash digest>`. An `EvidenceFragment` node ID MUST equal `evd:<first 16 hex of SHA-256(source_ref || "|" || RFC 8785 canonical JSON of locator)>`, concatenated as exact UTF-8 bytes using the stored `source_ref` string (implementation plan §6.1). `plumb-psg` exposes these as `source_artifact_id` and `evidence_fragment_id`; graph validation and every importer use those same functions rather than copies of the formulas.

**Global ID uniqueness.** A graph MUST reject duplicate node IDs, duplicate edge IDs and an ID used by both a node and an edge; construction never silently overwrites a duplicate.

**Envelope evidence versus `evidenced_by`.** The envelope `evidence` vector is direct provenance attached to the node/edge itself; `evidenced_by` is a first-class typed graph assertion modeled independently when an explicit relation is required. They are not required to mirror each other and no `evidenced_by` edge is generated from the envelope vector. This is an explicit exception to canonical relation ownership (§17), because envelope evidence is provenance metadata, not a `NodePayload` semantic reference.

---

## 18. Mandatory semantic invariants

The following are Plumb invariants, independent of any external standard claim.

### 18.1 Graph integrity

1. IDs are globally unique and immutable.
2. Accepted edges MUST reference existing non-rejected nodes.
3. An extension key MUST be namespace-qualified.
4. Core node types and core relations MUST validate against their registered schemas.
5. A supersession chain MUST be acyclic.

### 18.2 Evidence and provenance

6. Every imported semantic element MUST have an evidence or derivation path to a source fragment.
7. Every LLM-derived accepted element MUST retain its `InferenceRecord`/`DerivationRecord`.
8. A live model call MUST NOT be required to reproduce an accepted graph revision.
9. Human acceptance MUST be a recorded ResolutionDecision or explicit accepted proposal event.

### 18.3 Requirements

10. An accepted requirement MUST have evidence, rationale/goal lineage, or an explicit human-origin source.
11. A requirement marked verifiable MUST have at least one VerificationObligation before gate D2.
12. Requirement decomposition MUST be acyclic.
13. A requirement superseded by another MUST not count independently in readiness/coverage.

### 18.4 Functional semantics

14. Every executable ProcessNode task MUST resolve to an Operation or an explicitly non-system human activity.
15. Every state transition MUST identify a source state, target state and trigger.
16. Every Calculation MUST type-check under PlumbExpr.
17. Every DecisionTable used for deterministic execution MUST pass overlap/coverage checks appropriate to its hit policy and analyzable domain.
18. A scenario run MUST return `Undecidable` rather than invent a missing value or semantic decision.

### 18.5 Authorization

19. BusinessRole and SecurityRole MUST remain distinct types.
20. Every baseline-participating Permission MUST resolve to exactly one concrete Operation through `permits` and exactly one ResourceScope through `scoped_to`; `conditioned_by` remains optional (0..*).
21. Static role inheritance MUST be acyclic.
22. SeparationConstraint violations are blocking for security-sensitive gates unless explicitly waived.

### 18.6 Quality

23. A blocking QualityScenario MUST identify a characteristic, measurable response and threshold before A1 passes.
24. Accepted architecture decisions justified by quality MUST link to the corresponding QualityScenario.

### 18.7 Architecture

25. Functional elements MUST NOT mutate into architecture elements; allocation is a relationship.
26. Accepted ArchitectureDecision nodes MUST have at least one driver (`justified_by` relation) and a rationale.
27. An accepted architecture candidate MUST be uniquely designated for a given baseline unless an explicit multi-target deployment profile is active.
28. Layout coordinates MUST NOT influence `semantic_hash`.

### 18.8 Contracts

29. An API operation satisfying a functional operation MUST preserve stable traceability through `exposed_by`.
30. Contract projections MUST use stable operation/channel/message identifiers so round-trip reconciliation is possible.

### 18.9 Delivery

31. An ImplementationSlice MUST have at least one upstream obligation and at least one downstream verification obligation before D2.
32. TaskContract `must_not` constraints override agent/developer-proposed edits unless a new ResolutionDecision changes them.

### 18.10 Verification

33. A TestReceipt MUST bind specification hash, code revision, test revision and environment.
34. Stale evidence MUST NOT satisfy a current verification obligation.
35. Coverage MUST be computed from typed relations, never inferred from matching names alone.

---

## 19. View model

A diagram is a `View`, not a second semantic model.

The `View` payload is defined normatively in §12.5 (name, viewpoint_ref, architecture_description_ref, root_refs, filter, projection_rules, layout_ref, style_ref).

View metadata contains:

```text
node_positions
edge_routes
collapsed_groups
manual_labels
zoom_defaults
```

View metadata is separately hashed.

### 19.1 Initial Plumb view profiles

Functional:

```text
System / User Context
Actor & Business Role
Authorization / RBAC
Process / Swimlane
Decision
Domain / Entity
State
Scenario Sequence
```

Architecture:

```text
System Context
Container
Component
Interface
Event / Channel
Deployment
Technology
```

Engineering:

```text
Requirements Traceability
Impact
Allocation
Verification Coverage
Standards Conformance
```

Diagram edits MUST produce semantic patches. They MUST NOT directly mutate a private diagram-only copy of semantics.

---

## 20. Mutation model

The v2 `Box<dyn Patch>` is replaced by a serializable change AST. This section is normative.

### 20.1 Element hashes

The **element hash** is a generic `sha256:` hash (`HashKind::Generic`) of the RFC 8785 canonical JSON of an element projection. It applies to every node and edge of every status and node type, and it is neither the global `semantic_hash` nor the element revision. A node projection is exactly `{"element_kind": "node", "id", "status", "payload", "evidence", "standards", "tags", "extensions"}`; an edge projection is exactly `{"element_kind": "edge", "id", "status", "kind", "from", "to", "properties", "evidence", "standards"}`. `revision`, `derivations` and `audit` are excluded, and there is no `project_id`/`profile_id`. `evidence`, `standards`, `validator_rules` and View payloads (`layout_ref`/`style_ref` omitted) are normalized exactly as in the semantic hash projection (implementation plan §6.2).

`NodePayload::validate()` validates the typed sub-structures that deserialization validates (`EvidenceFragment.locator`, `DerivationRecord`, `ResolutionDecision`), and `Node::validate()` delegates to it, so programmatically constructed payloads are checked exactly like deserialized ones.

### 20.2 PatchSet and preconditions

```rust
#[serde(deny_unknown_fields)]
pub struct PatchSet { pub base_semantic_hash: Hash, pub patch: SemanticPatch }

#[serde(deny_unknown_fields)]
pub struct ElementPrecondition { pub id: Id, pub expected_hash: Hash }
```

`PatchSet` is the serialized replay and compare-and-swap unit. `base_semantic_hash` MUST be `HashKind::Semantic` and MUST equal the base graph's `semantic_hash` before any operation runs (semantic-hash precondition); otherwise the patch set is stale and produces no candidate graph. Branch-head / `GraphRevision` compare-and-swap is a separate store-level protection; both are required. `ElementPrecondition.expected_hash` MUST be `HashKind::Generic` and MUST equal the element hash of the current working element immediately before that leaf operation executes (so a later operation on the same element describes the state produced by an earlier one).

### 20.3 SemanticPatch AST

```rust
#[serde(tag = "op")]
pub enum SemanticPatch {
    AddNode { node: Node },
    RemoveNode { target: ElementPrecondition },
    ReplacePayload { target: ElementPrecondition, payload: NodePayload },
    SetStatus { target: ElementPrecondition, from: ElementStatus, to: ElementStatus },
    AddEdge { edge: Edge },
    ReplaceEdge { target: ElementPrecondition, kind: RelationKind, from: Id, to: Id, properties: RelationProperties },
    RemoveEdge { target: ElementPrecondition },
    MergeNodes { keep: ElementPrecondition, merge: Vec<ElementPrecondition>, field_policy: MergePolicy },
    Supersede { old: ElementPrecondition, new: ElementPrecondition, edge: Edge },
    AttachEvidence { target: ElementPrecondition, evidence: EvidenceRef },
    AttachStandardMapping { target: ElementPrecondition, mapping: StandardMapping },
    Compound { patches: Vec<SemanticPatch> },
}

pub enum MergePolicy { KeepPayloadUnionMetadata } // serialized "keep_payload_union_metadata"
```

There are exactly 12 variants; the `op` string is the Rust variant name. Unknown variants and unknown fields are rejected; there is no generic operation or property escape hatch.

### 20.4 Operation semantics

- **AddNode / AddEdge:** the ID is absent from both nodes and edges, the revision is exactly `1`, and local validation passes. Endpoint existence and graph-wide validity are checked only on the final graph, so a Compound may add endpoints and edges in either order.
- **ReplacePayload:** the target is a node whose expected hash matches; the replacement has the same `NodeType` (changing type is forbidden); all other envelope fields are preserved. `SourceArtifact.content_hash` and `EvidenceFragment.source_ref`/`locator` define the derived node ID and MUST NOT change (typed derived-identity error); changing them requires a new, correctly derived node and explicit reference changes.
- **SetStatus:** the target is a node or edge whose expected hash matches, its status equals `from`, and `from != to`; only the status changes and no relation is created or removed automatically.
- **ReplaceEdge:** the only mechanism to retarget an edge; it changes exactly `kind`, `from`, `to` and `properties` and preserves `id`, `status`, `evidence`, `derivations`, `standards` and `audit`.
- **RemoveNode / RemoveEdge:** the expected hash matches; removal never cascades. A node with any incident edge cannot be removed (remove or retarget those edges earlier in the same Compound). A conservative reference guard then rejects the removal if any JSON string value (at any depth, including typed fields, arrays, open JSON and extension values; object property names are not inspected) of any other surviving node or edge equals the removed ID exactly (typed `ReferencedElement` naming the removed and referencing IDs); references are never rewritten automatically.
- **MergeNodes:** only for `Requirement` or `Term`, with keep and every merged node of the same type. `merge` is non-empty, unique and excludes `keep`; every expected hash matches; merged nodes have no incident edges and no surviving element contains their ID as a JSON string value. The result keeps `id`, `status`, `payload` and `audit` of the keep node; `evidence` and `derivations` are the exact union sorted by ID, `standards` the exact union sorted by RFC 8785 canonical bytes, `tags` the set union, and `extensions` the key union (equal values deduplicate; differing values fail with `MergeExtensionConflict`). Merged nodes are removed; MergeNodes never retargets edges or rewrites references.
- **Supersede:** `old` and `new` are distinct nodes with matching expected hashes; the supplied edge has a fresh ID, revision `1`, kind `supersedes`, `from = new`, `to = old`, status `Accepted`, and passes local validation. The old node's status becomes `Superseded`, the new node is unchanged, and exactly the supplied edge is added (the engine never generates IDs).
- **AttachEvidence / AttachStandardMapping:** the target is a node or edge with a matching expected hash and the exact reference/mapping is not already attached; it is appended without creating an `evidenced_by` edge or merging mappings.
- **Compound:** non-empty; nested Compounds are flattened depth-first in declared order. Each leaf enforces its own existence, hash and local structural preconditions against the working candidate, but graph-wide validity (endpoints, cardinality, cycles, cross-element references) is evaluated once on the final graph. Any failure discards the candidate; the base graph is never modified and no partial graph or delta is returned.
- **ID recycling:** within one patch set an ID that has existed or been removed cannot be reused for a new node or edge.
- **Local validation:** a locally changed node or edge is validated immediately (payload, revision, audit, `conflicts_with` orientation, relation/property pairing, envelope duplicates).
- **Revision finalization:** after all leaf operations and before final graph construction, revisions are finalized by the element-hash rule of §4.5.
- **Inverse:** an inverse is returned only when it is derivable entirely from the patch input: `AddNode` -> `RemoveNode` and `AddEdge` -> `RemoveEdge` using the supplied element's hash, and a Compound whose children are all invertible (children reversed). Every other variant has no inverse; missing previous values are never fabricated, and a `PatchSet` has no inverse because its resulting base hash is only known after application.

Every patch MUST be serializable, replayable, emit a semantic diff, and pass through the same intake/conflict engine regardless of whether it came from UI, chat, import, API, diagram edit or AI.

---

## 21. Branches, proposals and candidate designs

Plumb needs explicit model overlays.

```rust
pub struct ModelBranch {
    id: Id,
    branch_kind: BranchKind,
    base_semantic_hash: Hash,
    patches: Vec<SemanticPatch>,
    status: BranchStatus,
}
```

Kinds:

```text
proposal
architecture_candidate
migration_candidate
what_if
external_sync
```

This allows three candidate architectures to allocate the same accepted functional model differently without contaminating the accepted baseline.

Merging a branch uses semantic conflict detection plus both protections: the `PatchSet` semantic-hash precondition (§20.2) and branch-head / `GraphRevision` compare-and-swap against its captured base revision.

---

## 22. Stage contract

The v2 claim that every stage is pure should be preserved but clarified.

Deterministic stage:

```rust
fn validate(graph: &Graph, config: &Config) -> StageOutput
```

LLM-assisted stage is split:

```text
build_inference_request(graph, config) -> InferenceRequest
external provider call -> persisted InferenceArtifact
validate_inference(graph, artifact, config) -> StageOutput
```

The provider call is I/O and is not part of the pure semantic transform.

---

## 23. Project gates

### I0 — Evidence captured

Passes when all scoped source artifacts are stored, content-addressed and parseable enough to provide evidence fragments.

### F1 — Requirements grounded

Passes when blocking requirements are traced to evidence/human origin, parsed into accepted requirement semantics and have no unresolved high-severity requirement-quality findings.

### F2 — Functional semantics coherent

Passes when vocabulary, entities, states, operations, rules, calculations and processes satisfy blocking Plumb invariants.

### F3 — Functional decisions resolved

Passes when no blocking functional finding remains unresolved except explicitly governed assumptions/waivers.

### F4 — Functional model executable/verifiable

Passes when relevant functional obligations have suitable accepted scenarios/verification obligations, deterministic execution has no unresolved blocking `Undecidable`, and assumptions are visible.

### Q1 — Quality measurable

Passes when architecture-driving quality requirements have characteristics, measures and thresholds.

### A1 — Architecture drivers complete

Passes when relevant functional obligations, quality scenarios and technical constraints are identified as drivers.

### A2 — Architecture allocated

Passes when software responsibilities/operations requiring implementation are allocated to architecture elements and critical data/interfaces are owned.

### A3 — Technical contracts complete

Passes when required synchronous/event interfaces have sufficient technical contracts and traceability.

### A4 — Architecture justified

Passes when accepted architecture/technology decisions are linked to drivers, alternatives and consequences; blocking architecture findings are resolved.

### D1 — Delivery plan complete

Passes when accepted scope can be partitioned into implementation slices with an acyclic dependency order or explicit migration strategy.

### D2 — Verification obligations complete

Passes when each in-scope obligation has an appropriate verification method and downstream executable/manual evidence plan.

### C1 — Implementation conforms

Passes when required current evidence exists and no blocking drift/conformance finding remains.

---

## 24. `NodePayload` skeleton

```rust
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "data")]
pub enum NodePayload {
    // Evidence / governance
    SourceArtifact(SourceArtifact),
    EvidenceFragment(EvidenceFragment),
    DerivationRecord(DerivationRecord),
    Agent(Agent),
    Finding(Finding),
    Question(Question),
    ResolutionDecision(ResolutionDecision),
    Assumption(Assumption),

    // Intent / requirements
    Stakeholder(Stakeholder),
    Concern(Concern),
    Goal(Goal),
    Need(Need),
    Requirement(Requirement),
    AcceptanceCriterion(AcceptanceCriterion),
    Constraint(Constraint),

    // Vocabulary / domain
    Term(Term),
    Concept(Concept),
    Actor(Actor),
    BusinessRole(BusinessRole),
    Entity(Entity),
    Attribute(Attribute),
    DomainRelationship(DomainRelationship),
    State(State),
    Transition(Transition),
    Invariant(Invariant),

    // Functional
    Operation(Operation),
    Outcome(Outcome),
    Event(Event),
    Process(Process),
    ProcessNode(ProcessNode),
    Rule(Rule),
    DecisionTable(DecisionTable),
    Calculation(Calculation),
    Calendar(Calendar),
    Scenario(Scenario),

    // Authorization
    Principal(Principal),
    SecurityRole(SecurityRole),
    Permission(Permission),
    ResourceScope(ResourceScope),
    PolicyCondition(PolicyCondition),
    SeparationConstraint(SeparationConstraint),

    // Quality
    QualityCharacteristic(QualityCharacteristic),
    Measure(Measure),
    QualityScenario(QualityScenario),

    // Architecture
    SystemOfInterest(SystemOfInterest),
    ArchitectureDescription(ArchitectureDescription),
    ArchitectureCandidate(ArchitectureCandidate),
    Viewpoint(Viewpoint),
    View(View),
    ModelKind(ModelKind),
    SoftwareSystem(ArchitectureElement),
    Container(ArchitectureElement),
    Component(ArchitectureElement),
    Module(ArchitectureElement),
    Interface(ArchitectureElement),
    DataStore(ArchitectureElement),
    ExternalSystem(ArchitectureElement),
    DeploymentNode(ArchitectureElement),
    RuntimeEnvironment(ArchitectureElement),
    NetworkZone(ArchitectureElement),
    ArchitectureDecision(ArchitectureDecision),
    Technology(Technology),
    TechnologySelection(TechnologySelection),

    // Technical contracts
    ApiContract(ApiContract),
    ApiOperation(ApiOperation),
    EventContract(EventContract),
    Channel(Channel),
    Message(Message),
    DataSchema(DataSchema),
    TechnicalWorkflow(TechnicalWorkflow),

    // Delivery
    Capability(Capability),
    ImplementationSlice(ImplementationSlice),
    WorkPackage(WorkPackage),
    TaskContract(TaskContract),
    Migration(Migration),
    Release(Release),

    // Verification
    VerificationObligation(VerificationObligation),
    TestCase(TestCase),
    ScenarioRun(ScenarioRun),
    TestExecution(TestExecution),
    TestReceipt(TestReceipt),
    CodeBinding(CodeBinding),
    ArchitectureCheck(ArchitectureCheck),
    CoverageRecord(CoverageRecord),

    // Standards
    StandardsProfile(StandardsProfile),

    // Namespaced extension
    Extension(ExtensionPayload),
}
```

This looks large, but it is intentionally a semantic type system rather than a generic graph with undocumented conventions.

### 24.1 Payload binding contract (normative)

This section fixes the Rust/JSON binding of every `NodePayload` variant (86 variants, including `DerivationRecord` and `Extension`). Types not derivable from these rules MUST NOT be inferred.

**Required / optional.** A field listed under *Required* is `T`; under *Optional* it is `Option<T>`. A required list is `Vec<T>`; an optional list is `Option<Vec<T>>`. `None` serializes as JSON `null` (no `skip_serializing_if`); a missing optional field deserializes as `None`; a missing required field fails deserialization.

**Unknown fields.** Every core payload struct and every nested typed struct rejects unknown fields (`#[serde(deny_unknown_fields)]`). Extensibility exists only through the node `extensions` map and `NodePayload::Extension`; unknown core fields are never silently preserved.

**Default scalar binding.** Unless another rule applies, a scalar field is `String` and a list of scalars is `Vec<String>`. No enum is created merely because a field name ends in `kind`, `status`, `type`, `class`, `mode`, `policy` or similar; only the closed vocabularies below are enums.

**References, hashes, timestamps.** Unless excepted: `*_ref` → `Id`, `*_refs[]` → `Vec<Id>`, `*_hash` → `Hash`, `*_hashes[]` → `Vec<Hash>` (`plumb_core` types). The fields `created_at_source`, `source_timestamp`, `created_at`, `decided_at`, `expires_at`, `started_at`, `finished_at` and `executed_at` are `Timestamp`. `target_date` is a calendar date and is `String`.

**Exceptions.** `StandardMapping.clause_ref` → `Option<String>`; `Finding.standard_rule_ref` → `Option<String>`; `ResolutionDecision.patch_ref` → `Hash`; `DerivationRecord.input_refs` and `output_refs` → `Vec<String>`; `View.layout_ref`, `View.style_ref` and `TestReceipt.logs_ref` → `Option<Hash>`; `CodeBinding.repository_ref` → `String`. Fields that identify PSG nodes although their names do not end in `_ref`: `DomainRelationship.from_entity`, `DomainRelationship.to_entity`, `Transition.from_state` and `Transition.to_state` → `Id`. Genuinely external references (URIs, addresses, paths or external identifiers; no `Id` grammar, normalization or URI validation): `Stakeholder.contact_ref`, `ApiContract.external_spec_ref`, `EventContract.external_spec_ref`, `DataSchema.external_ref` and `TestCase.automation_ref` → `Option<String>`.

**Other primitives.** `Attribute.nullable` → `bool`; `Entity.aggregate_root` → `Option<bool>`; `Attribute.precision` → `Option<u32>`; `DomainRelationship.snapshot_semantics` → `Option<bool>`; `StandardsProfileStandard.required` → `bool`. Evidence-locator integer indexes are `u64`; page-region coordinates are `f64`.

**Open JSON (`serde_json::Value`).** Allowed only for: `DerivationRecord.parameters` → `Option<Value>`; `Question.answer_schema` → `Option<Value>`; `ResolutionDecision.answer` → `Value`; `Assumption.default_value` → `Option<Value>`; `DecisionTable.inputs`, `outputs`, `rows` → `Vec<Value>`; `Calculation.examples` → `Option<Vec<Value>>`; `Calendar.week_pattern` → `Value`; `Scenario.given` → `Vec<Value>`; `Scenario.when` → `Value`; `Scenario.then` → `Vec<Value>`; `QualityScenario.threshold` → `Value`; `View.filter` → `Option<Value>`; `DataSchema.inline_schema` → `Option<Value>`; `TestCase.steps`, `expected` → `Vec<Value>`; `ScenarioRun.trace` → `Vec<Value>`; `ExtensionPayload.data` → `Value`. No other core payload field uses `Value`. Later tasks may add deterministic interpretation of these fields but may not change their persisted type without a human-approved plan revision.

**Expressions.** The PSG does not depend on `plumb-expr`. Expression-bearing fields (`guard_expr`, `condition_expr`, `preconditions[]`, `postconditions[]`, `action_expr`, `expression`, `timer_expr`) are `String` / `Vec<String>` / `Option<...>` per their optionality; parsing and typechecking belong to `plumb-expr`.

**Closed vocabularies.** Only these are Rust enums; each serializes exactly as listed, and unknown values are rejected:

| Enum | Field | Serialized values |
|---|---|---|
| `DerivationKind` | `DerivationRecord.kind` | `deterministic_rule`, `parser`, `import`, `human_edit`, `human_resolution`, `llm_inference`, `recovery`, `migration`, `external_sync` |
| `AgentKind` | `Agent.agent_kind` | `human`, `organization`, `software_service`, `llm_model`, `compiler_stage`, `external_system` |
| `FindingSeverity` | `Finding.severity` | `blocker`, `error`, `warn`, `info` |
| `QuestionKind` | `Question.question_kind` | `YesNo`, `PickOne`, `PickMany`, `Number`, `Text`, `Cardinality`, `Unit`, `Precision`, `Rounding`, `FormulaConfirm`, `RuleCell`, `Calendar`, `RoleAssignment`, `Permission`, `QualityThreshold`, `ArchitectureChoice`, `TechnologyChoice`, `InterfaceChoice`, `VerificationMethod` (exact spellings, not snake_case) |
| `RequirementKind` | `Requirement.requirement_kind` | `functional`, `quality`, `interface`, `data`, `security`, `operational`, `compliance`, `transition`, `constraint` |
| `RequirementLevel` | `Requirement.level` | `stakeholder`, `system`, `software`, `subsystem`, `component`, `interface` |
| `Modality` | `Requirement.modality` | `shall`, `should`, `may`, `shall_not` |
| `ConstraintCategory` | `Constraint.constraint_category` | `business`, `technical`, `technology`, `security`, `data`, `integration`, `operational`, `legal`, `regulatory`, `organizational`, `legacy` |
| `ConstraintStrength` | `Constraint.strength` | `mandatory`, `preferred`, `prohibited` |
| `ConceptKind` | `Concept.concept_kind` | `object_type`, `fact_type`, `value_type`, `role`, `other` |
| `ActorKind` | `Actor.actor_kind` | `human`, `system`, `external_system`, `organization` |
| `OperationKind` | `Operation.operation_kind` | `command`, `query` |
| `OutcomeKind` | `Outcome.outcome_kind` | `success`, `business_failure`, `technical_failure`, `partial` |
| `ProcessNodeKind` | `ProcessNode.node_kind` | `start`, `end`, `human_task`, `service_task`, `exclusive_gateway`, `parallel_split`, `parallel_join`, `message_event`, `timer_event`, `error_event`, `subprocess` |
| `RuleKind` | `Rule.rule_kind` | `constraint`, `derivation`, `permission`, `validation`, `business` |
| `ScenarioKind` | `Scenario.scenario_kind` | `acceptance`, `boundary`, `failure`, `state_transition`, `rule_row`, `quality`, `integration`, `regression` |
| `SeparationConstraintKind` | `SeparationConstraint.constraint_kind` | `static_separation_of_duty`, `dynamic_separation_of_duty`, `mutual_exclusion`, `required_combination` |
| `ArchitectureCandidateStatus` | `ArchitectureCandidate.status` | `exploring`, `candidate`, `accepted`, `rejected`, `superseded` |
| `TechnologySelectionStatus` | `TechnologySelection.status` | `candidate`, `selected`, `rejected`, `legacy`, `prohibited` |
| `ApiContractKind` | `ApiContract.contract_kind` | `http`, `rpc`, `other` |
| `VerificationKind` | `VerificationObligation.verification_kind` | `test`, `scenario`, `analysis`, `inspection`, `review`, `demonstration`, `formal_check`, `architecture_check`, `security_check` |
| `ScenarioRunResult` | `ScenarioRun.result` | `pass`, `fail`, `undecidable` |

**Fields that remain `String`** (no closed authoritative vocabulary yet; optionality per the metamodel): `SourceArtifact.source_kind`, `SourceArtifact.classification`, `Finding.family`, `Finding.status`, `Question.status`, `Assumption.status`, `Stakeholder.stakeholder_kind`, `AcceptanceCriterion.criterion_kind`, `Term.status`, `Attribute.value_type`, `Attribute.data_classification`, `DomainRelationship.relationship_kind`, `DomainRelationship.cardinality_from`, `DomainRelationship.cardinality_to`, `DomainRelationship.ownership`, `Operation.idempotency`, `Operation.transaction_semantics`, `Process.process_kind`, `DecisionTable.hit_policy`, `Calculation.result_type`, `Calculation.rounding`, `Scenario.confirmation_status`, `Principal.principal_kind`, `ResourceScope.scope_kind`, `Measure.measure_type`, `Measure.aggregation`, `ArchitectureDecision.status`, `Technology.technology_kind`, `DataSchema.schema_kind`, `Migration.migration_kind`, `TestExecution.result`, `TestReceipt.result`, `ArchitectureCheck.result`, `CoverageRecord.coverage_kind`, `CoverageRecord.status`.

**`EvidenceLocator`** (`EvidenceFragment.locator`) is `#[serde(tag = "kind", content = "data")]` with exactly these variants (tag strings equal the variant names); nested locator objects reject unknown fields, and only structural validation happens at this layer:

```rust
pub enum EvidenceLocator {
    TextRange { start: u64, end: u64 },
    PageRegion { page: u64, x: Option<f64>, y: Option<f64>, width: Option<f64>, height: Option<f64> },
    TableCell { table: u64, row: u64, column: u64 },
    XmlPath { xpath: String },
    JsonPointer { pointer: String },
    ConversationTurn { turn_id: Id },
    ExternalObject { object_id: String, field: Option<String> },
}
```

`PageRegion` coordinates `x`, `y`, `width` and `height`, when present, MUST be finite (no NaN or infinity), and `width` and `height` MUST be `>= 0`; this is enforced by explicit validation and on deserialization. No range is imposed on `x`/`y`.

**Explicit payload structures.**

```rust
pub struct Agent { pub agent_kind: AgentKind }

pub struct SourceArtifact {
    pub source_kind: String, pub display_name: String, pub content_hash: Hash, pub media_type: String,
    pub external_uri: Option<String>, pub external_version: Option<String>, pub producer: Option<String>,
    pub created_at_source: Option<Timestamp>, pub language: Option<String>, pub classification: Option<String>,
}

pub struct EvidenceFragment {
    pub source_ref: Id, pub locator: EvidenceLocator, pub content_hash: Hash,
    pub extracted_text: Option<String>, pub speaker: Option<String>, pub source_timestamp: Option<Timestamp>,
}

pub struct Finding {
    pub code: String, pub family: String, pub severity: FindingSeverity, pub message: String,
    pub status: String, pub affected_refs: Vec<Id>,
    pub standard_rule_ref: Option<String>, pub suggested_resolution: Option<String>, pub waiver_ref: Option<Id>,
}

pub struct Question {
    pub finding_ref: Id, pub question_kind: QuestionKind, pub prompt: String, pub status: String,
    pub answer_schema: Option<Value>, pub stakeholder_ref: Option<Id>, pub priority: Option<String>,
    pub round_ref: Option<Id>, pub context_refs: Option<Vec<Id>>,
}

pub struct ResolutionDecision {
    pub question_ref: Option<Id>, pub proposal_ref: Option<Id>, pub answer: Value,
    pub decided_by: Id, pub decided_at: Timestamp, pub patch_ref: Hash,
    pub rationale: Option<String>, pub supersedes: Option<Id>,
}

pub struct ArchitectureElement {
    pub name: String, pub description: Option<String>, pub responsibilities: Option<Vec<String>>,
    pub owner_ref: Option<Id>,
}

pub struct ApiOperation {
    pub api_contract_ref: Id, pub operation_id: String,
    pub method: Option<String>, pub path: Option<String>,
    pub security_refs: Option<Vec<Id>>,
}

pub struct StandardsProfileStandard { pub standard_id: String, pub version: String, pub role: MappingRole, pub required: bool }

pub struct StandardsProfile {
    pub id: Id, pub name: String, pub version: String,
    pub standards: Vec<StandardsProfileStandard>, pub validation_packs: Vec<String>,
}

pub struct ExtensionPayload { pub extension_type: ExtensionKey, pub data: Value }
```

`DerivationRecord` is defined in §5.3 and `View` in §12.5. `ResolutionDecision` requires at least one of `question_ref` and `proposal_ref` (both may be present). Whether `ApiOperation.method`/`path` are mandatory for an HTTP contract is later graph-semantic validation. `ArchitectureElement` is the payload of all ten architecture element variants (`SoftwareSystem`, `Container`, `Component`, `Module`, `Interface`, `DataStore`, `ExternalSystem`, `DeploymentNode`, `RuntimeEnvironment`, `NetworkZone`); the variant itself is the architectural type, so there is no `architecture_element_kind` field. `ArchitectureDecision` uses `alternatives: Vec<String>`, `selected_option: String`, `affected_refs: Option<Vec<Id>>`, `supersedes: Option<Id>`. `TechnologySelection` uses `alternatives: Option<Vec<Id>>`, `architecture_decision_ref: Option<Id>`. Payload reference fields that duplicate a §17 relation do not exist (§17, canonical relation ownership). `ExtensionPayload` is the only generic semantic escape hatch and MUST NOT carry core semantics.

**Remaining payloads.** Every other payload in §§5-16 preserves its exact field names and Required/Optional split, applies the rules above, and adds no fields.

**Wire contract.** `NodePayload` is `#[serde(tag = "type", content = "data")]`; the `type` value is exactly the Rust variant name (not snake_case). Unknown variant names and extra top-level keys are rejected.

**`NodeType`.** A closed enum with exactly one variant per `NodePayload` variant (86), with `NodePayload::node_type() -> NodeType` and `NodeType::as_str() -> &'static str` returning exactly the `type` tag. It is the stable discriminator for registries and indexes; type identity is never derived from Rust type-name strings.

**Node.** `Node` (§4.1, §4.5) rejects unknown fields and exposes a deterministic `Node::validate()` with a typed error; deserialization enforces the same invariants: `revision != 0`, valid `AuditMeta`, and `Node.id == payload.id` for `DerivationRecord` and `StandardsProfile` payloads. Invalid nodes are never repaired. `tags` has no additional grammar and is preserved exactly. There is no universal confidence field.

---

## 25. Registry design

The core should expose a machine-readable registry rather than scattering type logic through Rust code.

```rust
pub struct TypeDef {
    type_name: &'static str,
    schema_id: &'static str,
    namespace: Namespace,
    allowed_statuses: &'static [ElementStatus],
    standard_defaults: &'static [StandardMappingTemplate],
}

pub struct RelationDef {
    relation: RelationKind,
    allowed_from: TypePredicate,
    allowed_to: TypePredicate,
    directionality: Directionality,
    semantic_properties_schema: Option<&'static str>,
}
```

JSON Schemas are generated/committed for interchange and API validation. Rust typed structures remain compile-time truth for the product implementation.

---

## 26. Projection architecture

The PSG is canonical. Outputs are projections.

### Functional projections

```text
functional.yaml
requirements.yaml
glossary.md
scenarios.yaml
processes/*.bpmn
decisions/*.dmn
```

### Architecture projections

```text
architecture.yaml
architecture.md
views/*.json
adrs/*.md
sysml/*
archimate/*          optional licensed profile
```

### Technical projections

```text
openapi/*.yaml
asyncapi/*.yaml
arazzo/*.yaml
schemas/*
deployment.yaml
```

### Delivery projections

```text
implementation-plan.yaml
implementation-plan.md
task-contracts/*.yaml
```

### Assurance projections

```text
verification-obligations.yaml
traceability.json
conformance-report.md
standards-conformance.json
provenance.jsonl
receipts/*.json
```

A projection MUST record the PSG semantic hash from which it was generated. The `functional.yaml` v2 compatibility projection records it as its `model_hash` and in its companion projection metadata; it is export-only and never imported back automatically.

Round-trip-capable formats MUST include stable Plumb IDs through permitted extension mechanisms or companion mapping files.

---

## 27. Migration from v2

The current v2 functional model remains valuable and should not be discarded.

### 27.1 Direct mappings

```text
Source             -> SourceArtifact
Span               -> EvidenceFragment
glossary entry     -> Term + Concept
actor              -> Actor
entity             -> Entity
attribute          -> Attribute
relationship       -> DomainRelationship
state              -> State
transition         -> Transition
invariant          -> Invariant
calculation        -> Calculation
rule               -> Rule or DecisionTable
operation          -> Operation
process            -> Process + ProcessNode
business event     -> Event
requirement        -> Requirement
scenario           -> Scenario
assumption         -> Assumption
finding            -> Finding
question           -> Question
Decision           -> ResolutionDecision
```

### 27.2 Required semantic splits

`roles` MUST split into:

```text
BusinessRole
SecurityRole
Permission
ResourceScope
```

The current `nfr` bags MUST migrate into:

```text
Requirement(kind=quality)
QualityCharacteristic
QualityScenario
Measure
Constraint
```

The generic `Origin` MUST become provenance/derivation records.

`Box<dyn Patch>` MUST become `SemanticPatch`.

The generic `Node.kind + props` store MAY remain underneath temporarily, but all new code MUST go through typed `NodePayload` and relation registries.

### 27.3 Compatibility commitment

During migration, `functional.yaml` v2 remains a supported export projection so the existing designer/API/UI work does not break.

The first v3 implementation is therefore an additive semantic layer, not a rewrite of every pilot feature.

---

## 28. Implementation sequence

### Phase V3.0 — Foundation now

Implement before expanding the current pilot substantially:

1. `plumb-spec-core` type registry and typed `NodePayload`.
2. Typed `RelationKind` registry.
3. `StandardMapping` and `StandardsProfile`.
4. `DerivationRecord` / `InferenceRecord` and evidence refs.
5. Serializable `SemanticPatch`.
6. Semantic/evidence/view hash separation and deterministic canonicalization.
7. v2-to-v3 migration adapters plus unchanged `functional.yaml` export.

### Phase V3.1 — Functional normalization

1. Split BusinessRole / SecurityRole / Permission.
2. Move NFRs into QualityRequirement/QualityScenario.
3. Replace simplistic process semantics with controlled BPMN-compatible subset.
4. Align decision tables to controlled DMN subset.
5. Add verification-obligation semantics by requirement type.

### Phase V3.2 — Architecture compiler

1. ISO-42010-style architecture description concepts.
2. Architecture candidates.
3. SoftwareSystem / Container / Component / Interface / DataStore.
4. Functional-to-architecture allocation.
5. ArchitectureDecision and TechnologySelection.
6. Quality-driver/trade-off checks.
7. C4-style views over the PSG.

### Phase V3.3 — Technical contracts

1. OpenAPI projection/import.
2. AsyncAPI projection/import.
3. Arazzo projection/import.
4. Data schemas.
5. Deployment model and technical validation.

### Phase V3.4 — Delivery compiler

1. ImplementationSlice.
2. TaskContract.
3. dependency graph.
4. architecture-aware slicing.
5. agent/developer handoff.

### Phase V3.5 — Conformance loop

1. CodeBinding.
2. TestReceipt.
3. stale-evidence calculation.
4. architecture checks.
5. change-impact engine.
6. C1 conformance gate.

---

## 29. What is deliberately *not* in the core

Plumb v3 should NOT:

- reproduce the entire SysML v2 metamodel;
- become a full BPMN workflow engine;
- become a full DMN runtime beyond semantics needed by Plumb;
- embed TOGAF or ArchiMate proprietary content without appropriate licensing;
- treat C4, ADR or EARS as formal international standards;
- encode every industry regulation into the core type system;
- make implementation technology choices indistinguishable from requirements;
- require a scenario for requirement types better verified by analysis, inspection or architecture checks;
- allow diagram state to become a parallel truth source.

Industry and organization semantics belong in namespaced profiles and rule packs.

---

## 30. Example digital thread

```text
EvidenceFragment
  evd:policy-p14
      |
      | evidenced_by
      v
Requirement
  req:LEAVE-143
  "Only the employee's manager may approve the request"
      |
      | specified_by
      v
Operation
  op:ApproveLeave
      |
      +---- performed_by ----> BusinessRole:Manager
      |
      +---- governed_by -----> Permission:ApproveOwnTeamLeave
      |                           |
      |                           +-- scoped_to --> ResourceScope:DirectReports
      |
      +---- allocated_to ----> Component:LeaveApproval
      |                           |
      |                           +-- uses_technology --> TechnologySelection:Rust
      |
      +---- exposed_by ------> ApiOperation:approveLeave
      |                           |
      |                           +-- contract --> OpenAPI operationId
      |
      +---- implemented_by --> ImplementationSlice:S17
      |                           |
      |                           +-- task contract --> TaskContract:S17
      |
      +---- verified_by -----> VerificationObligation:V143
                                  |
                                  +-- implemented_as --> Scenario:SCN143-01
                                  |
                                  +-- receipt --> TestReceipt:TR845
                                                    |
                                                    +-- code revision: 62bc...
                                                    +-- semantic hash: psg:...
```

That connected semantic thread is the central product asset.

---

## 31. Enterprise positioning enabled by the metamodel

Plumb can credibly describe itself as:

> **A standards-aligned software specification compiler that converts evidence and stakeholder intent into an executable specification graph spanning requirements, functional semantics, quality, architecture, technical contracts, delivery and verification.**

The stronger enterprise message is:

> **Plumb does not replace established engineering standards. It operationalizes and connects them into one traceable digital thread.**

This is materially different from a requirements repository, generic AI business analyst, architecture drawing tool or spec-to-code prompt system.

---

## 32. Decision to freeze

The following should be treated as v3 architectural decisions unless implementation proves them untenable:

1. PSG, not `functional.yaml`, is canonical.
2. Core semantics are typed; generic graph storage is implementation detail.
3. Functional truth and technical architecture remain distinct and are connected by allocation/satisfaction relations.
4. Standards are versioned profiles and mappings, not baked-in inheritance trees.
5. Diagrams are semantic views over the graph.
6. LLM output is proposal/provenance, never truth by itself.
7. All mutations use one serializable semantic patch language.
8. Verification evidence is revision-bound and can become stale.
9. `Undecidable` remains a first-class valid execution result.
10. `functional.yaml` v2 remains a compatibility projection during migration.

