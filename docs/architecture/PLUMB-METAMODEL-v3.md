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

- `semantic_hash` — accepted semantic nodes/edges, accepted decisions, and semantic constraints. Excludes timestamps, UI layout, transient jobs, cached inference text and other operational metadata.
- `evidence_hash` — source artifacts and evidence-fragment identity.
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
    pub properties: BTreeMap<String, Value>,
    pub evidence: Vec<EvidenceRef>,
    pub derivations: Vec<DerivationRef>,
    pub standards: Vec<StandardMapping>,
    pub audit: AuditMeta,
}
```

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

When `kind` is `llm_inference`, all eight LLM-specific fields are mandatory; for every other kind all eight MUST be absent. Invalid combinations are rejected by explicit validation and by deserialization. `input_refs` and `output_refs` are strings because provenance inputs and outputs may identify either PSG IDs or content-addressed artifacts/hashes. `DerivationRecord` is a PSG payload (`NodePayload::DerivationRecord`); a node carrying it MUST have `Node.id == DerivationRecord.id`.

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
entity_ref
unit
precision
enum_values[]
data_classification
```

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
owner_ref
```

### 8.9 `Transition`

Required:

```text
stateful_ref
from_state
to_state
trigger_ref
```

Optional:

```text
guard_expr
effect_refs[]
```

The trigger MAY be an `Operation` or `Event`.

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
actor_ref
condition_expr
message_ref
timer_expr
```

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
operation_ref
resource_scope_ref
```

Optional:

```text
policy_condition_refs[]
```

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
quality_characteristic_ref
stimulus
response
measure_ref
threshold
```

Optional:

```text
source_ref
environment_condition
affected_refs[]
priority
```

Example:

```yaml
quality_characteristic_ref: quality:performance-efficiency
stimulus: "5000 concurrent users submit leave requests"
environment_condition: "normal production operation"
affected_refs: [api:leave]
response: "requests are accepted and processed"
measure_ref: measure:p95-latency
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

`projection_rules` are semantic view-definition rules. `layout_ref` and `style_ref` point to separately stored and separately hashed view metadata; coordinates, routes and style values are never embedded in the `View` payload. `semantic_hash` of a `View` excludes `layout_ref` and `style_ref`, so changing only layout/style metadata does not change `semantic_hash`.

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
technology_selection_refs[]
owner_ref
```

### 12.8 `ArchitectureDecision`

Required:

```text
question
status
drivers[]
alternatives[]
selected_option
rationale
```

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
applies_to_refs[]
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
drivers[]
rationale
architecture_decision_ref
```

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
request_schema_ref
response_schema_refs[]
error_schema_refs[]
security_refs[]
```

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
payload_schema_ref
```

Optional:

```text
headers_schema_ref
correlation_ref
```

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
step_refs[]
```

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
slice_refs[]
```

---

## 15. Verification and conformance namespace

### 15.1 `VerificationObligation`

Required:

```text
name
verification_kind
target_refs[]
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

This replaces the v2 idea that every requirement must necessarily have an executed passing scenario. Verification method depends on requirement semantics.

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
verification_obligation_refs[]
```

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
semantic_ref
repository_ref
code_locator
code_revision
```

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
20. Every Permission MUST resolve to one or more concrete operations and a resource scope.
21. Static role inheritance MUST be acyclic.
22. SeparationConstraint violations are blocking for security-sensitive gates unless explicitly waived.

### 18.6 Quality

23. A blocking QualityScenario MUST identify a characteristic, measurable response and threshold before A1 passes.
24. Accepted architecture decisions justified by quality MUST link to the corresponding QualityScenario.

### 18.7 Architecture

25. Functional elements MUST NOT mutate into architecture elements; allocation is a relationship.
26. Accepted ArchitectureDecision nodes MUST include at least one driver and one rationale.
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

The v2 `Box<dyn Patch>` is replaced by a serializable change AST.

```rust
#[serde(tag = "op")]
pub enum SemanticPatch {
    AddNode { node: Node },
    RemoveNode { id: Id, expected_hash: Hash },
    ReplacePayload { id: Id, expected_hash: Hash, payload: NodePayload },
    SetStatus { id: Id, from: ElementStatus, to: ElementStatus },
    AddEdge { edge: Edge },
    RemoveEdge { id: Id, expected_hash: Hash },
    MergeNodes { keep: Id, merge: Vec<Id>, field_policy: MergePolicy },
    Supersede { old: Id, new: Id },
    AttachEvidence { target: Id, evidence: EvidenceRef },
    AttachStandardMapping { target: Id, mapping: StandardMapping },
    Compound { patches: Vec<SemanticPatch> },
}
```

Every patch MUST:

- validate preconditions against a base graph hash/revision;
- be serializable;
- be replayable;
- emit a semantic diff;
- support a deterministic inverse where logically possible;
- pass through the same intake/conflict engine regardless of whether it came from UI, chat, import, API, diagram edit or AI.

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

Merging a branch uses semantic conflict detection plus compare-and-swap against its captured base revision.

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

**Exceptions.** `StandardMapping.clause_ref` → `Option<String>`; `Finding.standard_rule_ref` → `Option<String>`; `ResolutionDecision.patch_ref` → `Hash`; `DerivationRecord.input_refs` and `output_refs` → `Vec<String>`; `View.layout_ref`, `View.style_ref` and `TestReceipt.logs_ref` → `Option<Hash>`; `CodeBinding.repository_ref` → `String`.

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
    pub technology_selection_refs: Option<Vec<Id>>, pub owner_ref: Option<Id>,
}

pub struct ApiOperation {
    pub api_contract_ref: Id, pub operation_id: String,
    pub method: Option<String>, pub path: Option<String>,
    pub request_schema_ref: Option<Id>, pub response_schema_refs: Option<Vec<Id>>,
    pub error_schema_refs: Option<Vec<Id>>, pub security_refs: Option<Vec<Id>>,
}

pub struct StandardsProfileStandard { pub standard_id: String, pub version: String, pub role: MappingRole, pub required: bool }

pub struct StandardsProfile {
    pub id: Id, pub name: String, pub version: String,
    pub standards: Vec<StandardsProfileStandard>, pub validation_packs: Vec<String>,
}

pub struct ExtensionPayload { pub extension_type: ExtensionKey, pub data: Value }
```

`DerivationRecord` is defined in §5.3 and `View` in §12.5. `ResolutionDecision` requires at least one of `question_ref` and `proposal_ref` (both may be present). Whether `ApiOperation.method`/`path` are mandatory for an HTTP contract is later graph-semantic validation. `ArchitectureElement` is the payload of all ten architecture element variants (`SoftwareSystem`, `Container`, `Component`, `Module`, `Interface`, `DataStore`, `ExternalSystem`, `DeploymentNode`, `RuntimeEnvironment`, `NetworkZone`); the variant itself is the architectural type, so there is no `architecture_element_kind` field. `ArchitectureDecision` uses `drivers: Vec<Id>`, `alternatives: Vec<String>`, `selected_option: String`, `affected_refs: Option<Vec<Id>>`, `supersedes: Option<Id>`. `TechnologySelection` uses `applies_to_refs: Vec<Id>`, `alternatives: Option<Vec<Id>>`, `drivers: Option<Vec<Id>>`, `architecture_decision_ref: Option<Id>`. `ExtensionPayload` is the only generic semantic escape hatch and MUST NOT carry core semantics.

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

A projection MUST record the PSG semantic hash from which it was generated.

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

