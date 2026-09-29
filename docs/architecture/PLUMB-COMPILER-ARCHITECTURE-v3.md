# Plumb Compiler Architecture v3.0

**Status:** implementation architecture draft  
**Product:** Plumb 1.0  
**Depends on:** `PLUMB-METAMODEL-v3.md`, `PLUMB-VALIDATION-RULEBOOK-2026.1.md`  
**Profile:** `profile:plumb-software-2026.1`

---

## 1. Purpose

Plumb is a **Software Specification Compiler**.

Its canonical engineering chain is:

> **Evidence → Intent → Function → Quality → Architecture → Contract → Work → Proof**

The compiler takes heterogeneous enterprise evidence and incrementally produces an accepted, typed, traceable **Plumb Specification Graph (PSG)**. It does not treat generated prose, diagrams, YAML or AI responses as canonical truth.

The compiler architecture has five non-negotiable properties:

1. **Deterministic acceptance semantics.** Gate results, hashes, rule findings, projections and replay must be deterministic for the same accepted inputs.
2. **AI proposes; Plumb validates; humans decide material ambiguity.** A live model never writes accepted graph state directly.
3. **Every accepted semantic claim is traceable.** It has evidence, a deterministic derivation path, a human decision, or a combination.
4. **Every stage is replayable.** Replaying an accepted baseline does not require calling an LLM again.
5. **Every downstream artifact is a projection or proof over the PSG.** Documents, diagrams, API descriptions, implementation plans and verification reports are not parallel truth stores.

---

## 2. Compiler pipeline

The default software profile executes the following logical stages.

```text
S0  Evidence Intake                    -> I0
S1  Intent / Requirement Compiler      -> F1
S2  Functional Semantic Compiler       -> F2
S3  Resolution Compiler                -> F3
S4  Functional Proof Compiler          -> F4
S5  Quality Compiler                   -> Q1
S6  Architecture Driver Compiler       -> A1
S7  Architecture Allocation Compiler   -> A2
S8  Technical Contract Compiler        -> A3
S9  Architecture Justification         -> A4
S10 Delivery Compiler                  -> D1
S11 Verification Compiler              -> D2
S12 Conformance Compiler               -> C1
```

Gates are proof obligations over accepted PSG state. A stage can be executed before its predecessor gate passes for diagnostic purposes, but a formal accepted baseline cannot claim a later gate while a mandatory predecessor remains unproven.

---

## 3. The stage contract

A compiler stage is not "a function that calls an LLM".

Each stage has four separable concerns:

```text
PLAN -> ACQUIRE ARTIFACTS -> EVALUATE -> COMMIT
```

### 3.1 PLAN

Pure and deterministic.

Input:

- accepted graph revision
- active standards/profile version
- stage configuration
- known persisted inference/validation artifacts

Output:

- deterministic derivations available immediately
- required `InferenceRequest`s
- required `ExternalValidationRequest`s
- affected semantic scope
- expected proposal classes

### 3.2 ACQUIRE ARTIFACTS

Impure orchestration boundary.

May:

- call an LLM
- execute an OpenAPI/AsyncAPI/BPMN/DMN validator
- retrieve a connector snapshot
- inspect a code repository
- execute tests

It **cannot mutate accepted PSG state**.

Every result is persisted as a content-addressed artifact.

### 3.3 EVALUATE

Pure and deterministic.

Input:

- the original accepted graph revision
- stage plan
- persisted artifacts

Output:

- deterministic semantic patches
- semantic proposals
- findings
- questions
- gate evaluation
- impact set
- projections that can be regenerated

### 3.4 COMMIT

Transactional.

Before application:

```text
proposal.base_semantic_hash == current_branch.semantic_hash
```

If false, the proposal is stale and must be re-evaluated against the new head.

Accepted patches produce a new immutable `GraphRevision`.

---

## 4. Core compiler data structures

### 4.1 `GraphRevision`

```rust
pub struct GraphRevision {
    pub id: RevisionId,
    pub parent: Option<RevisionId>,
    pub semantic_hash: Hash,
    pub evidence_hash: Hash,
    pub profile_ref: ProfileRef,
    pub rule_pack_hash: Hash,
    pub accepted_patch_ref: Option<ArtifactRef>,
    pub decision_refs: Vec<Id>,
    pub created_by: AgentRef,
    pub created_at: Timestamp,
}
```

A revision is immutable.

`Timestamp` in these structures is the canonical Plumb timestamp defined by implementation plan §6.3: a UTC-normalized instant with nanosecond precision, serialized as RFC 3339 with uppercase `T`, uppercase `Z` and exactly nine fractional-second digits (for example `2026-09-29T12:34:56.123456789Z`); RFC 3339 input with a `T` or single-space separator and any numeric offset is converted to the equivalent UTC instant, surrounding whitespace is rejected, and instants whose UTC year is outside `0000..=9999` are rejected. `Hash` values use exactly the normative forms `sha256:`, `psg:sha256:` and `ev:sha256:` followed by 64 lowercase hex digits (implementation plan §6.3).

Branches are mutable pointers to revisions:

```text
main
candidate:architecture-A
candidate:architecture-B
proposal:<id>
```

This gives Plumb snapshot restore without mutating history.

### 4.2 `CompileRun`

```rust
pub struct CompileRun {
    pub id: RunId,
    pub stage: StageId,
    pub input_revision: RevisionId,
    pub input_semantic_hash: Hash,
    pub profile_ref: ProfileRef,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,
    pub compiler_version: String,
    pub config_hash: Hash,
    pub inference_artifacts: Vec<ArtifactRef>,
    pub validation_artifacts: Vec<ArtifactRef>,
    pub output_proposal_refs: Vec<ArtifactRef>,
    pub output_finding_refs: Vec<Id>,
    pub output_hash: Hash,
}
```

Operational timestamps are not part of the semantic hash.

### 4.3 `InferenceRequest`

```rust
pub struct InferenceRequest {
    pub id: Hash,
    pub stage: StageId,
    pub task_kind: String,
    pub input_refs: Vec<Id>,
    pub evidence_refs: Vec<Id>,
    pub context_hash: Hash,
    pub prompt_template_hash: Hash,
    pub schema_hash: Hash,
    pub provider_policy: ProviderPolicy,
}
```

The request ID is deterministic from its content.

### 4.4 `InferenceArtifact`

```rust
pub struct InferenceArtifact {
    pub request_hash: Hash,
    pub provider: String,
    pub model: String,
    pub parameters: CanonicalJson,
    pub raw_response_hash: Hash,
    pub validated_output: CanonicalJson,
    pub validated_output_hash: Hash,
}
```

A replay uses the persisted artifact.

`Agent` and `DerivationRecord` are PSG provenance payloads defined once in the metamodel (§§5.3-5.4) and implemented in `plumb-psg`. The inference layer reuses those types (for example when materializing a `DerivationRecord` from persisted inference artifacts) and never defines duplicate versions.

A user explicitly choosing "re-run with model" creates a **new compile run**, not a replay of the old one.

### 4.5 `Proposal`

```rust
pub struct Proposal {
    pub id: Id,
    pub stage: StageId,
    pub base_revision: RevisionId,
    pub base_semantic_hash: Hash,
    pub patches: Vec<SemanticPatch>,
    pub evidence_refs: Vec<Id>,
    pub derivation_refs: Vec<Id>,
    pub materiality: Materiality,
    pub acceptance_policy: AcceptancePolicy,
    pub confidence: Option<f32>,
    pub impact: ImpactSet,
    pub intake: IntakeReport,
}
```

### 4.6 Acceptance policies

```text
AUTO_DERIVATION
  Deterministic fact derived completely from accepted semantics.
  Example: calculated coverage, derived finding, generated projection.

AUTO_NON_SEMANTIC
  Does not add an engineering claim.
  Example: source content hash, fragment locator, cached layout.

HUMAN_CONFIRM
  Proposed semantic interpretation.
  Example: extracted entity, inferred operation, process sequence.

HUMAN_DECISION
  Material ambiguity, architecture choice, security/quality threshold,
  technology selection, waiver, supersession.

PROFILE_POLICY
  Customer profile defines whether explicit confirmation is mandatory.
```

LLM-originated accepted semantics MUST NOT use `AUTO_DERIVATION`.

---

## 5. Cross-cutting services

The stage pipeline shares the following services.

### 5.1 Evidence service

Responsibilities:

- content-addressed source storage
- connector snapshotting
- deterministic extraction
- `EvidenceFragment` location validation
- evidence hashing
- source version comparison

### 5.2 PSG semantic registry

Responsibilities:

- node payload schemas
- relationship source/target constraints
- cardinalities
- semantic invariants
- extension namespace registration
- standards mappings

### 5.3 Semantic patch engine

All writes use the same serializable mutation language.

```text
AddNode
RemoveNode
ReplacePayload
SetStatus
AddEdge
RemoveEdge
MergeNodes
Supersede
AttachEvidence
AttachStandardMapping
Compound
```

The patch engine provides:

- schema validation
- precondition checking
- deterministic application
- inverse where meaningful
- impact calculation
- structured diff
- compare-and-swap commit

### 5.4 Intake engine

Every proposed semantic patch, irrespective of source, runs through intake:

```text
UI
chat
document import
diagram edit
LLM
API
MCP
external synchronization
human answer
```

Intake checks:

- equivalent/covered semantics
- refinements
- contradictions
- duplicate concepts
- term conflicts
- relation/cardinality conflicts
- architecture conflicts
- profile violations
- novelty
- impact radius

### 5.5 Validation engine

Consumes the active rule pack.

Rule evaluation is pure.

External validators execute outside the rule and supply content-addressed `ValidationArtifact`s.

The rule engine returns:

```text
PASS
FAIL
WARN
NOT_APPLICABLE
WAIVED
ERROR
```

### 5.6 Impact engine

Maintains typed dependency reachability.

A semantic patch creates a `DirtySet` containing directly changed and transitively affected objects.

Uses include:

- incremental validation
- scenario re-execution
- stale verification evidence
- contract regeneration
- affected architecture views
- task-contract scope checks
- code-binding verification

### 5.7 Projection engine

Produces views from PSG:

```text
functional.yaml
requirements.yaml
architecture.yaml
OpenAPI
AsyncAPI
Arazzo
BPMN subset
DMN subset
ADRs
implementation plan
task contracts
verification matrix
standards conformance report
human-readable Markdown
diagrams
```

Projections are reproducible artifacts bound to `semantic_hash`.

### 5.8 Baseline / branch service

Supports:

- immutable accepted revisions
- named baselines
- candidate architecture branches
- proposal branches
- merge/rebase through semantic patch evaluation
- structured semantic diff

---

## 6. Stage S0 — Evidence Intake

### Goal

Produce a reproducible and addressable evidence corpus.

### Inputs

- source files
- transcript turns
- connector objects
- external API/architecture descriptions
- optional existing code/contract artifacts

### Deterministic work

- content hash
- MIME/source type
- extraction to normalized text/data
- fragment offsets/locators
- fragment content hashes
- source/version lineage
- full-coverage validation

### AI work

Optional only:

- document structure assistance
- table/section classification where deterministic parsing is insufficient
- semantic segmentation suggestions

AI segmentation is still anchored to deterministic evidence fragments.

### Human decisions

Normally none for ingestion mechanics.

Human intervention is required when:

- a source is materially unparseable
- a connector snapshot is incomplete
- source authority/version must be selected

### Persisted outputs

```text
SourceArtifact[]
EvidenceFragment[]
DerivationRecord[]
EvidenceManifest
```

### Gate

`I0`

A later semantic claim cannot rescue an unreproducible evidence corpus.

---

## 7. Stage S1 — Intent / Requirement Compiler

### Goal

Compile evidence into explicit needs, requirements, constraints, concerns and vocabulary candidates.

### Inputs

- I0 evidence baseline

### Deterministic work

- requirement-like sentence detection fallback
- modal detection
- source span coverage
- duplicate lexical candidates
- ID generation
- requirement structural validation
- lint
- evidence linking

### AI proposals

- requirement segmentation
- requirement classification
- EARS-style normalized statement
- goals/needs/concerns
- constraint classification
- acceptance criteria candidates
- vocabulary candidates

### Human decisions

Required when:

- requirement meaning materially changes through normalization
- two sources conflict
- modality is unclear
- scope/actor/object is ambiguous
- a requirement is merged/superseded
- a source is declared non-authoritative

### Accepted outputs

```text
Stakeholder
Concern
Goal
Need
Requirement
AcceptanceCriterion
Constraint
Term/Concept candidates
```

### Gate

`F1`

F1 proves requirements are grounded, not that the functional model is complete.

---

## 8. Stage S2 — Functional Semantic Compiler

### Goal

Compile accepted intent into executable domain and functional semantics.

### Inputs

- accepted F1 model

### Deterministic work

- vocabulary normalization
- type checking
- relation/cardinality validation
- PlumbExpr parse/typecheck
- decision-table overlap/coverage
- process graph reachability
- state-machine integrity
- operation read/write consistency
- authorization consistency
- gap/inconsistency registry

### AI proposals

- entity/attribute extraction
- relationships
- states/transitions
- operations
- outcomes
- rules
- calculations
- process fragments
- events
- business-role interpretation
- security-role/permission candidates

### Human decisions

Required for unresolved:

- cardinality
- units and precision
- formula semantics
- rounding
- time/calendar boundaries
- allowed actors
- authorization scope
- conflicting process/order semantics
- rule-table cells
- state transitions

### Accepted outputs

```text
Entity/Attribute/DomainRelationship
State/Transition/Invariant
Actor/BusinessRole
Operation/Outcome/Event
Process/ProcessNode
Rule/DecisionTable
Calculation/Calendar
SecurityRole/Permission/ResourceScope
```

### Gate

`F2`

---

## 9. Stage S3 — Resolution Compiler

### Goal

Turn open semantic gaps into explicitly owned decisions.

### Inputs

- F2 findings
- stakeholder map
- accepted semantics

### Deterministic work

- question template selection
- slot population
- duplicate suppression
- blast-radius calculation
- stakeholder routing
- round composition
- patch preview
- stale-question detection

### AI work

AI may explain context in business language.

It may not invent new questions that are absent from the deterministic finding/question engine unless it creates a normal proposal that first passes intake.

### Human decisions

This is the primary human-governance stage.

An answer becomes:

```text
ResolutionDecision
  -> SemanticPatch
  -> new GraphRevision
```

### Gate

`F3`

No unresolved blocker ambiguity remains in scope.

---

## 10. Stage S4 — Functional Proof Compiler

### Goal

Prove the accepted functional model is executable or otherwise explicitly verifiable.

### Deterministic work

- scenario boundary derivation
- rule-row scenario derivation
- state-transition scenario derivation
- interpreter execution
- trace generation
- dirty-set re-execution
- coverage computation

### AI proposals

- realistic scenario values
- additional acceptance/failure scenario candidates
- human-readable scenario descriptions

Every value is validated against accepted types, enums, units and ranges.

### Human decisions

- acceptance of generated scenarios where they assert intended behavior
- correction of expected outcomes
- resolution of `Undecidable`

### Important v3 difference

F4 does **not** require every requirement to have a passing scenario.

F4 requires the functional semantics that are executable through the functional VM to be coherent and exercised. Non-scenario verification methods are represented later as `VerificationObligation`s.

### Gate

`F4`

---

## 11. Stage S5 — Quality Compiler

### Goal

Turn vague NFR language into measurable quality drivers.

### Inputs

- requirements
- constraints
- functional model
- active ISO 25010 taxonomy profile

### Deterministic work

- characteristic mapping validation
- presence of measure and threshold
- unit/type validation
- duplicated/conflicting threshold checks
- affected-element validation

### AI proposals

- quality characteristic candidate
- stimulus
- environment
- affected artifact
- expected response
- measure
- threshold candidate

### Human decisions

Thresholds and business-critical trade-offs are material decisions.

AI may propose "p95 <= 400ms"; it cannot silently make that accepted truth.

### Accepted output

`QualityScenario[]`

### Gate

`Q1`

---

## 12. Stage S6 — Architecture Driver Compiler

### Goal

Build the explicit architecture decision context before architecture generation.

### Inputs

- F4 accepted functional model
- Q1 quality scenarios
- technical/security/data/integration/operational constraints

### Deterministic work

- driver completeness
- conflicting constraints
- mandatory vs preferred vs prohibited
- traceability to requirements/quality
- required viewpoints
- system-of-interest boundary validation

### AI proposals

- candidate architecture drivers
- missing concern identification
- technology constraints inferred from authoritative evidence
- candidate architecture options

### Human decisions

- architecture scope
- system boundary
- true imposed constraints
- risk appetite
- required candidate set

### Outputs

```text
SystemOfInterest
ArchitectureDescription
Stakeholders/Concerns
Viewpoints
ArchitectureDriverSet
```

### Gate

`A1`

---

## 13. Stage S7 — Architecture Allocation Compiler

### Goal

Create and compare architecture candidates and allocate functional responsibilities.

### Candidate isolation

Architecture generation occurs on candidate branches:

```text
baseline:functional@H
  ├── candidate:modular-monolith
  ├── candidate:domain-services
  └── candidate:event-driven
```

No candidate pollutes accepted architecture truth before selection.

### AI proposals

AI is useful here for:

- decomposition candidates
- responsibility allocation
- interface candidates
- data ownership
- technology options
- risk/trade-off enumeration
- candidate views

### Deterministic work

- every in-scope operation allocated
- no orphan component
- dependency graph analysis
- constraint compatibility
- responsibility overlap
- forbidden technology checks
- data ownership consistency
- security-boundary validation
- required-view generation

### Human decisions

Architecture selection is always an `ArchitectureDecision`.

Material technology selections are also accepted decisions.

### Outputs

```text
ArchitectureCandidate[]
SoftwareSystem/Container/Component/Module
Interface/DataStore/ExternalSystem
Deployment concepts
TechnologySelection candidates
allocation edges
```

### Gate

`A2`

A2 is evaluated per candidate and again for the accepted candidate.

---

## 14. Stage S8 — Technical Contract Compiler

### Goal

Make architecture externally implementable.

### Inputs

- accepted architecture candidate
- functional operations/events
- interfaces
- data model
- constraints

### Deterministic work

- operation-to-contract traceability
- stable identifier checks
- schema compatibility
- request/response/error coverage
- producer/consumer checks
- contract version checks
- generated artifact validation

### AI proposals

- API shape
- operation names
- resource paths
- event/channel design
- error taxonomy
- schema design
- technical workflows

### Human decisions

Required for externally material:

- API breaking shape
- versioning policy
- event semantics
- externally visible identifiers
- security/authentication approach

### Projections

```text
OpenAPI
AsyncAPI
Arazzo
JSON Schema
```

### Gate

`A3`

---

## 15. Stage S9 — Architecture Justification Compiler

### Goal

Prove that the selected architecture is justified by explicit drivers and decisions.

### Deterministic work

- accepted decision has drivers
- alternatives recorded
- rationale present
- quality scenarios addressed
- constraints satisfied or waived
- accepted technology selections justified
- unresolved architecture findings
- candidate-selection lineage

### AI proposals

- trade-off narrative
- consequence analysis
- ADR text projection

### Human decisions

Architecture decisions and risk acceptances remain human-governed.

### Gate

`A4`

A4 is not "the diagrams look complete"; it proves why the architecture is the accepted answer to the known drivers.

---

## 16. Stage S10 — Delivery Compiler

### Goal

Compile accepted architecture and functional obligations into implementation slices.

### Inputs

- requirements
- functions
- accepted architecture
- contracts
- decisions
- constraints
- dependency graph

### Deterministic slicing inputs

The planner uses:

- functional cohesion
- architecture allocation
- component/interface boundaries
- data ownership
- dependency direction
- migration ordering
- contract dependencies
- scenario/verification locality
- release scope

AI may propose a decomposition, but deterministic validation decides whether it is structurally legal.

### `ImplementationSlice`

Each slice links to:

```text
upstream requirements
operations
architecture elements
contracts
data
decisions
constraints
dependencies
verification obligations
```

### `TaskContract`

For agent/developer-facing work:

```text
intent
must[]
must_not[]
allowed_change_refs[]
forbidden_change_refs[]
completion_criteria[]
```

### Gate

`D1`

---

## 17. Stage S11 — Verification Compiler

### Goal

Generate a typed verification plan from engineering obligations.

### Core rule

Not all requirements are tested the same way.

`VerificationObligation.verification_kind` may be:

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

### Deterministic work

- obligation coverage
- acceptance-criterion traceability
- quality threshold coverage
- architecture-rule coverage
- slice-to-verification coverage
- security-sensitive operation coverage
- typed coverage matrix

### AI proposals

- concrete test-case candidates
- additional edge cases
- manual verification procedures
- expected evidence descriptions

### Human decisions

- verification strategy where several methods are valid
- approval of manual/non-executable verification
- risk-based waivers

### Gate

`D2`

---

## 18. Stage S12 — Conformance Compiler

### Goal

Determine whether the implementation currently conforms to the accepted specification baseline.

### Inputs

- accepted PSG revision
- code bindings
- implementation contract snapshots
- architecture checks
- scenario/test execution
- environment identity

### Deterministic work

- receipt revision checks
- staleness
- code-binding integrity
- contract drift
- architecture drift
- task-contract `must_not`
- current verification coverage
- blocker status

### Evidence

`TestReceipt` binds:

```text
verification obligation
semantic hash
code revision
test/check revision
environment
result
```

### Staleness

A later change does not invalidate all evidence globally.

The impact engine calculates whether the change can reach the verified obligation.

If yes, the relevant receipt becomes stale.

If no, a proven unaffected ancestor receipt may remain valid.

### Gate

`C1`

---

## 19. AI execution boundary

The following architecture is mandatory:

```text
                      +--------------------+
                      |  LLM / Validator   |
                      |  External Runtime  |
                      +----------+---------+
                                 |
                           artifact only
                                 v
+------------+    plan    +------+-------+    evaluate   +-------------+
| PSG rev H  |----------->| Stage Runner |-------------->| Proposal Set|
+------------+            +------+-------+               +------+------+ 
                                 |                              |
                                 | persisted                    | intake
                                 v                              v
                           Artifact Store                Validation/Impact
                                                               |
                                                          human/policy
                                                               |
                                                               v
                                                        SemanticPatch
                                                               |
                                                          CAS commit
                                                               v
                                                        PSG rev H+1
```

No external runtime has a graph-write capability.

---

## 20. Inference provider architecture

`LlmProvider` remains useful but changes role.

Recommended interface:

```rust
pub trait InferenceProvider {
    fn execute(&self, request: &InferenceRequest) -> Result<InferenceArtifact>;
}
```

Provider selection belongs in `ProviderPolicy`.

Examples:

```text
anthropic
openai
azure-openai
bedrock
vertex
openai-compatible
local
null
mock
```

Plumb core must never depend semantically on one vendor.

The stored artifact includes provider/model so enterprise customers can audit which model contributed to a proposal.

---

## 21. Concurrency and asynchronous jobs

Every asynchronous stage job captures:

```text
base_revision
base_semantic_hash
stage
scope
profile_hash
config_hash
```

When the job completes:

```text
if current_head == base_revision:
    proposal may be applied after normal intake
else:
    proposal status = STALE
    re-evaluate against current head
```

Never apply an async patch to a newer graph merely because its HTTP request started earlier.

`If-Match` remains correct for API writes, but internal jobs use the same compare-and-swap semantic rule.

---

## 22. Incremental recompilation

A full project compile must be possible, but routine editing must be incremental.

Every accepted `SemanticPatch` produces:

```text
ChangedSet
DirtySet
StaleEvidenceSet
AffectedProjectionSet
AffectedGateRuleSet
```

Example:

```text
Change:
  Requirement REQ-17 threshold 400ms -> 250ms

Dirty:
  QualityScenario QS-4
  ArchitectureDecision ADR-12
  Component API-Gateway
  ApiOperation submitLeave
  ImplementationSlice S-22
  VerificationObligation V-9

Stale:
  architecture check receipt AC-44
  performance test receipt TR-90
```

This is the foundation of Plumb's future change-impact capability.

---

## 23. Diagram compilation

A diagram is a `View`.

### Read path

```text
PSG -> ViewDefinition -> Projection -> Layout -> SVG/UI
```

### Edit path

```text
Diagram interaction
 -> semantic edit command
 -> SemanticPatch proposal
 -> intake
 -> impact
 -> acceptance
 -> PSG revision
 -> regenerate all affected views
```

Layout coordinates never enter `semantic_hash`.

Examples:

- move a box visually: view metadata only
- drag an operation from Component A to B: semantic `allocated_to` patch
- move a process step from Employee to Manager lane: semantic actor/role/process patch, with authorization and scenario impact

---

## 24. Compiler artifacts

The artifact store is content addressed.

Artifact identity is based only on the stored bytes: `artifact_hash = sha256:<lowercase SHA-256 of exact stored bytes>` (a generic `Hash`, implementation plan §6.3). The artifact kind, media type and creation timestamp are persisted with the artifact but do not participate in the hash. Artifacts are keyed only by generic `sha256:` hashes; `psg:sha256:` and `ev:sha256:` hashes are not artifact-store keys.

Artifact bytes and their persisted metadata are immutable after the first successful insertion. Storing already-present bytes with the same kind and media type is idempotent: it returns the existing hash, adds no row and keeps the original creation timestamp. Storing already-present bytes with a different kind or media type is an explicit `ArtifactMetadataConflict` that leaves the existing artifact unchanged. Stored bytes that differ from the supplied bytes under the same hash are an explicit integrity error and never overwrite the existing artifact. The artifact store never updates, replaces or deletes an artifact through `put`, and never reads wall-clock time; the caller supplies the creation timestamp.

Artifact families:

```text
source-original
source-extracted
evidence-manifest

inference-request
inference-response
validated-inference

external-validation
projection

proposal
patch
diff
impact-report

gate-report
conformance-report

scenario-trace
test-receipt
architecture-check
```

Artifacts have identity independent of graph nodes.

Large artifacts should not be stuffed into graph properties.

---

## 25. Failure semantics

Compiler failure categories are explicit.

```text
PARSE_FAILURE
ARTIFACT_ACQUISITION_FAILURE
INFERENCE_FAILURE
INFERENCE_SCHEMA_FAILURE
STALE_PROPOSAL
PATCH_PRECONDITION_FAILURE
SEMANTIC_VALIDATION_FAILURE
GATE_FAILURE
EXTERNAL_VALIDATOR_FAILURE
CONFLICT_REQUIRES_DECISION
```

A failed LLM call is operational failure.

An `Undecidable` scenario is a semantic result.

A failed gate is an engineering result.

They must not be conflated.

---

## 26. Security and enterprise deployment boundary

The compiler architecture supports three deployment modes without changing semantics:

```text
Plumb SaaS
Customer-dedicated managed deployment
Customer/on-prem deployment
```

Provider policy can restrict:

- source material allowed to leave customer boundary
- model providers
- external validators
- connector classes
- artifact retention
- logging
- telemetry

This policy affects artifact acquisition, not the semantic compiler model.

---

## 27. Recommended Rust package boundaries

The v2 crate split can evolve into:

```text
plumb-core
    ids, hashes, clocks, canonicalization, artifact primitives

plumb-psg
    NodePayload, RelationKind, metamodel registry, semantic graph

plumb-store
    SQLite graph revisions, artifact store, branches, ledger

plumb-patch
    SemanticPatch, diff, CAS commit, impact seed

plumb-import
    document/source parsing and evidence fragments

plumb-inference
    request/artifact contracts and providers

plumb-compiler
    stage plan/evaluate orchestration

plumb-validation
    profile/rule registry and gate engine

plumb-expr
    PlumbExpr

plumb-sim
    functional scenario interpreter

plumb-intake
    duplicate/conflict/refinement/novelty

plumb-impact
    dirty-set and staleness reachability

plumb-architecture
    candidate overlays, allocations, architecture-specific derivations

plumb-contracts
    OpenAPI/AsyncAPI/Arazzo model projections/reconciliation

plumb-delivery
    slices, dependency DAG, TaskContracts

plumb-conformance
    code bindings, receipts, drift, C1

plumb-projection
    YAML/Markdown/diagram/standard projections

plumb-api
plumb-cli
plumb-mcp
```

This is a target architecture, not a requirement to rename all crates before the pilot.

---

## 28. Stage API

Recommended high-level Rust interfaces:

```rust
pub trait CompilerStage {
    fn id(&self) -> StageId;

    fn plan(
        &self,
        graph: &Graph,
        ctx: &CompileContext,
    ) -> Result<StagePlan>;

    fn evaluate(
        &self,
        graph: &Graph,
        ctx: &CompileContext,
        plan: &StagePlan,
        artifacts: &ArtifactSet,
    ) -> Result<StageEvaluation>;
}
```

```rust
pub struct StagePlan {
    pub scope: Scope,
    pub deterministic_artifacts: Vec<Artifact>,
    pub inference_requests: Vec<InferenceRequest>,
    pub external_validation_requests: Vec<ExternalValidationRequest>,
}
```

```rust
pub struct StageEvaluation {
    pub derivation_patches: Vec<SemanticPatch>,
    pub proposals: Vec<Proposal>,
    pub findings: Vec<Finding>,
    pub questions: Vec<Question>,
    pub impact: ImpactSet,
}
```

`plan()` and `evaluate()` are pure for the same graph/context/artifacts.

---

## 29. Gate API

```rust
pub fn evaluate_gate(
    graph: &Graph,
    profile: &ResolvedProfile,
    rule_pack: &RulePack,
    gate: GateId,
    artifacts: &ArtifactSet,
) -> GateReport;
```

The result is reproducible for:

```text
semantic_hash
profile_hash
rule_pack_hash
validation_artifact_hashes
```

A readiness score can be computed for UI prioritization, but it never overrides a blocker.

---

## 30. Projection contract

Each projection declares:

```text
projection_kind
source_semantic_hash
profile_hash
projection_version
content_hash
```

Imports of a Plumb-generated projection must carry enough stable identifiers to reconcile changes back to PSG.

Generated files should therefore include non-visual stable IDs wherever the target format permits them.

---

## 31. End-to-end enterprise flow

```text
1. Upload requirements / policy / architecture evidence
2. I0 proves source reproducibility
3. F1 compiles grounded requirements
4. F2 compiles domain + functional semantics
5. F3 routes unresolved decisions to stakeholders
6. F4 executes/proves functional semantics
7. Q1 makes quality measurable
8. A1 identifies architecture drivers
9. A2 generates/compares/accepts architecture candidate
10. A3 generates implementable technical contracts
11. A4 records architecture rationale/trade-offs
12. D1 generates implementation slices/task contracts
13. D2 generates verification obligations
14. coding agents/humans implement against task contracts
15. C1 binds code + proof to the accepted baseline
16. later change triggers incremental impact/recompile
```

That is the complete Plumb product loop.

---

## 32. What must be built now vs later

### Must shape the pilot architecture now

Even if architecture/delivery features are not yet exposed:

- typed PSG
- semantic patch AST
- immutable graph revisions / branch head
- DerivationRecord / InferenceArtifact
- standards/profile registry
- gate/rule registry
- deterministic stage plan/evaluate boundary
- impact seeds/typed dependency graph
- `functional.yaml` as projection
- BusinessRole/SecurityRole separation
- QualityScenario type, even if Q1 UI comes later

### Can remain post-pilot

- architecture candidate generation
- full C4 view suite
- BPMN/DMN import/export
- SysML interoperability
- OpenAPI/AsyncAPI/Arazzo reconciliation
- implementation slicing UI
- code bindings
- proof receipts
- C1 implementation conformance
- ArchiMate/TOGAF profiles

The purpose of the v3 refactor is to make those later features additive rather than requiring a second foundation rewrite.
