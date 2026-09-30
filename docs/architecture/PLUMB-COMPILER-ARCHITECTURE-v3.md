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

Gate identifiers are the `plumb-core` `GateId` values `I0`, `F1`, `F2`, `F3`, `F4`, `Q1`, `A1`, `A2`, `A3`, `A4`, `D1`, `D2`, `C1`, in this dependency-chain order.

Gates are proof obligations over accepted PSG state. A stage can be executed before its predecessor gate passes for diagnostic purposes, but a formal accepted baseline cannot claim a later gate while a mandatory predecessor remains unproven.

---

## 3. The stage contract

A compiler stage is not "a function that calls an LLM".

Each stage has four separable concerns:

```text
PLAN -> ACQUIRE ARTIFACTS -> EVALUATE -> COMMIT
```

### 3.1 PLAN

Pure and deterministic: `plan(graph, context)`.

Input:

- accepted graph revision
- active standards/profile version (in `CompileContext`)
- stage configuration (in `CompileContext`)

PLAN receives no artifact store, provider, validator or previously loaded artifact bytes. It deterministically describes which artifacts are required.

Output (`StagePlan`, §28):

- affected semantic scope
- deterministic planned artifacts available immediately
- required `InferenceRequest`s
- required `ExternalValidationRequest`s

Acquisition/reuse orchestration may satisfy those deterministic request identities from an existing content-addressed store instead of re-executing them.

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

- the original accepted graph revision and `CompileContext`
- stage plan
- the exact acquired `ArtifactSet` (§28), with no acquisition timestamps

Output (`StageEvaluation`, §28):

- an optional deterministic derivation `PatchSet`
- semantic `Proposal`s (§4.5)

Deterministic findings and questions are PSG `Finding`/`Question` nodes created by the derivation `PatchSet`; proposed ones are contained in proposal patch sets. Impact is derived later from the committed `GraphDelta` (§5.6), gate evaluation belongs to the gate service, and intake assesses proposals afterwards (§5.4); none of these is duplicated in stage output.

### 3.4 COMMIT

Transactional.

Before application:

```text
proposal.base_semantic_hash == current_branch.semantic_hash
```

If false, the proposal is stale and must be re-evaluated against the new head.

The check is made twice: patch application verifies the `PatchSet.base_semantic_hash` precondition against the graph it is applied to (metamodel §20.2), and the store verifies branch-head / `GraphRevision` compare-and-swap against the expected head. Both protections are required.

Accepted patches produce a new immutable `GraphRevision`.

---

## 4. Core compiler data structures

### 4.1 `GraphRevision`

```rust
pub struct GraphRevision {
    pub id: RevisionId,
    pub version: u64,
    pub parent: Option<RevisionId>,

    pub project_id: Id,
    pub psg_schema_version: u32,

    pub semantic_hash: Hash,
    pub evidence_hash: Hash,

    pub profile_ref: Id,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,

    pub accepted_patch_ref: Option<Hash>,
    pub decision_refs: Vec<Id>,

    pub created_by: Id,
    pub created_at: Timestamp,
}
```

A revision is immutable.

`semantic_hash` is a Semantic (`psg:sha256:`) hash and `evidence_hash` an Evidence (`ev:sha256:`) hash of the revision's Graph; `profile_hash`, `rule_pack_hash` and `accepted_patch_ref` are generic `sha256:` hashes. `project_id` and `profile_ref` equal the Graph `project_id` and `profile_id`; `psg_schema_version` is the `PSG_SCHEMA_VERSION` under which the snapshot was written (implementation plan §6). `version` is a database-global positive counter (initial revision 1, then `MAX(version)+1`; branching never resets it). `RevisionId` is exactly `rev:<version>:<first 16 hex digits of the semantic_hash digest>`, with the version in canonical positive decimal. Because some persisted changes are excluded from `semantic_hash`, consecutive revisions may share a semantic hash; the version in `RevisionId` and branch-head CAS distinguish them.

The initial revision fixes `project_id`, `profile_ref`, `profile_hash` and `rule_pack_hash` and has no parent and no `accepted_patch_ref`. Every revision created by a commit inherits those four values from its parent and records as `accepted_patch_ref` the artifact (kind `patch`, media type `application/json`) holding the RFC 8785 canonical JSON of the accepted `PatchSet`, written in the same transaction as the revision and the branch-head move. A `SemanticPatch` cannot change active profile metadata; profile switching requires a later explicit contract. The pilot stores one project per database.

Actor identifiers: `AuditMeta.created_by`, `ResolutionDecision.decided_by` and `GraphRevision.created_by` are validated Plumb `Id`s. At this boundary `AgentRef` means a validated `Id`: no namespace prefix is mandatory, the ID need not resolve to a PSG `Agent` node, no actor node is created automatically and the store never uses it for authorization. Identity/provider/user-directory resolution belongs to provenance and application layers.

`Timestamp` in these structures is the canonical Plumb timestamp defined by implementation plan §6.3: a UTC-normalized instant with nanosecond precision, serialized as RFC 3339 with uppercase `T`, uppercase `Z` and exactly nine fractional-second digits (for example `2026-09-29T12:34:56.123456789Z`); RFC 3339 input with a `T` or single-space separator and any numeric offset is converted to the equivalent UTC instant, surrounding whitespace is rejected, and instants whose UTC year is outside `0000..=9999` are rejected. `Hash` values use exactly the normative forms `sha256:`, `psg:sha256:` and `ev:sha256:` followed by 64 lowercase hex digits (implementation plan §6.3).

Branches are mutable pointers to revisions:

```text
main
candidate:architecture-A
candidate:architecture-B
proposal:<id>
```

Branch names (`BranchName`) are 1..=255 bytes, start with an ASCII alphanumeric character and otherwise contain only ASCII letters, digits and `.`, `_`, `:`, `/`, `-`. A branch head moves only by compare-and-swap against an expected head; explicit restore moves a head to an existing revision and never deletes or rewrites revisions.

This gives Plumb snapshot restore without mutating history.

A successful commit returns `CommitResult { revision: GraphRevision, delta: GraphDelta }`. `delta` is exactly the `GraphDelta` produced by the single semantic-patch application (metamodel §20) that the commit persisted; it is not recomputed, reconstructed from snapshots or persisted. Loading a committed revision verifies that the accepted Patch artifact bytes equal the RFC 8785 canonical JSON of the `PatchSet` they deserialize to; semantically equivalent but noncanonical bytes are revision corruption.

### 4.2 `CompileRun`

```rust
pub struct CompileRun {
    pub id: Hash,
    pub stage: StageId,

    pub input_revision: RevisionId,
    pub input_semantic_hash: Hash,

    pub profile_ref: Id,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,

    pub compiler_version: String,
    pub config_hash: Hash,

    pub deterministic_artifacts: Vec<Hash>,
    pub inference_artifacts: Vec<Hash>,
    pub validation_artifacts: Vec<Hash>,

    pub output_derivation_patch_ref: Option<Hash>,
    pub output_proposal_refs: Vec<Hash>,

    pub output_hash: Hash,
}
```

`input_semantic_hash` is Semantic; every other hash and artifact ref is a generic `sha256:` hash, and every ref vector is sorted and unique. The input metadata and `config_hash` (the canonical content hash of `CompileContext.config`) come from the `CompileContext`; the three input-artifact vectors are exactly the hashes of the `ArtifactSet` supplied to EVALUATE. `output_hash` is the generic SHA-256 of the RFC 8785 canonical `StageEvaluation`; `output_derivation_patch_ref` is the hash of the canonical derivation `PatchSet` (absent when there is none) and `output_proposal_refs` the sorted hashes of each canonical `Proposal`. `id` is the generic SHA-256 of the canonical run excluding only `id`, and deserialization verifies it. There are no finding or question arrays: findings and questions have one representation, as PSG nodes inside stage patch output.

Operational timestamps are not part of the semantic hash, and a `CompileRun` contains none. It is persisted as a `compile-run` / `application/json` artifact together with its `patch` and `proposal` output artifacts, using a caller-supplied timestamp and outside the graph-commit transaction (an artifact stored before a later failure may remain; retry is idempotent; no accepted PSG state changes). The run artifact reference hashes the complete run including `id` and is distinct from `id`.

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

pub struct ProviderPolicy {
    pub provider: String,
    pub config: CanonicalJson,
}
```

Unknown fields are rejected in both structures. `id`, `context_hash`, `prompt_template_hash` and `schema_hash` are generic `sha256:` hashes. `task_kind` is non-empty, has no leading/trailing whitespace or control characters and is otherwise preserved exactly. `input_refs` and `evidence_refs` have set semantics: duplicates are invalid and both are stored and serialized sorted by `Id`, so identity does not depend on caller order. `ProviderPolicy.provider` matches exactly `^[a-z][a-z0-9._-]*$` (no normalization); `config` holds a JSON object (`{}` is valid) that is opaque to the inference core but contributes to request identity.

The request ID is deterministic from its content: `id` is the generic SHA-256 of the RFC 8785 canonical JSON of exactly

```json
{"stage", "task_kind", "input_refs", "evidence_refs", "context_hash", "prompt_template_hash", "schema_hash", "provider_policy"}
```

with `id` itself excluded. Deserialization recomputes the ID and rejects a mismatch. The request ID is distinct from the request artifact reference: the persisted request artifact holds the canonical JSON of the complete request including `id`, and its artifact hash is not required to equal `id`.

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

pub struct ProviderExecution {
    pub artifact: InferenceArtifact,
    pub raw_response_media_type: String,
    pub raw_response: Vec<u8>,
}
```

Unknown fields are rejected. `request_hash`, `raw_response_hash` and `validated_output_hash` are generic hashes; `provider` follows the `ProviderPolicy` grammar; `model` is non-empty without leading/trailing whitespace or control characters. For its request: `request_hash == request.id`, `provider == request.provider_policy.provider`, and `validated_output_hash` is the generic SHA-256 of the canonical `validated_output`. A `ProviderExecution` additionally requires a non-empty raw-response media type without control characters and `raw_response_hash` equal to the SHA-256 of the exact `raw_response` bytes; raw bytes stay outside `InferenceArtifact` so the replay artifact never embeds large provider payloads.

A replay uses the persisted artifact: a persisted `validated-inference` artifact deserializes to the identical `InferenceArtifact`, whose fields suffice for deterministic semantic evaluation without a live provider call or the raw response.

**Acquisition.** Inference acquisition persists through the standalone artifact store, never inside the graph-commit transaction: inference happens before semantic acceptance and rejected or stale inference artifacts may remain for audit and replay. One acquisition bundle stores, with a caller-supplied timestamp:

| Artifact | kind | media type | bytes |
|---|---|---|---|
| request | `inference-request` | `application/json` | canonical JSON of the complete `InferenceRequest` |
| raw response | `inference-response` | `ProviderExecution.raw_response_media_type` | exact `raw_response` bytes |
| validated inference | `validated-inference` | `application/json` | canonical JSON of the complete `InferenceArtifact` |

The result is `PersistedInferenceRefs { request_artifact_ref, raw_response_ref, validated_inference_ref }` (all generic), with `raw_response_ref == raw_response_hash`; `validated_inference_ref` hashes the complete `InferenceArtifact` bytes, not `validated_output_hash`. The three puts are not one transaction: if a bundle fails after an immutable artifact was stored, that artifact may remain, a retry is idempotent and no accepted PSG state has changed, so this is never a semantic partial commit.

**Provenance materialization.** A deterministic `DerivationRecord` is materialized from a validated request, execution and persisted refs, caller-supplied `output_refs` and a timestamp: `kind = llm_inference`; `stage = request.stage.as_str()`; `provider`, `model`, `parameters` (the underlying JSON), `raw_response_hash` and `validated_output_hash` from the artifact; `prompt_template_hash`, `schema_hash` and `context_hash` from the request; `input_refs` is the sorted unique union of every input and evidence ref, `request.id` and the three persisted refs; `output_refs` must be unique and are stored sorted. The record ID is `drv:<first 16 lowercase hex of the SHA-256 of the RFC 8785 canonical JSON of the complete record excluding only id>`, and the record must validate. No `Agent` node is created automatically and no Agent-node ID convention is implied.

`Agent` and `DerivationRecord` are PSG provenance payloads defined once in the metamodel (§§5.3-5.4) and implemented in `plumb-psg`. The inference layer reuses those types (for example when materializing a `DerivationRecord` from persisted inference artifacts) and never defines duplicate versions.

A user explicitly choosing "re-run with model" creates a **new compile run**, not a replay of the old one.

### 4.5 `Proposal`

`Proposal` is owned by the semantic patch layer (`plumb-patch`):

```rust
pub struct Proposal {
    pub id: Id,
    pub stage: StageId,
    pub patch_set: PatchSet,

    pub evidence_refs: Vec<EvidenceRef>,
    pub derivation_refs: Vec<DerivationRef>,

    pub materiality: ProposalMateriality,
    pub acceptance_policy: AcceptancePolicy,
    pub confidence: Option<f32>,
}

pub enum ProposalMateriality { NonSemantic, Semantic, MaterialDecision }
```

`ProposalMateriality` serializes as `non_semantic` (adds or changes no engineering claim), `semantic` (adds or changes engineering semantics without being a material decision) or `material_decision` (a material ambiguity, waiver, architecture/security/quality choice or comparable decision requiring governed human resolution). `AcceptancePolicy` serializes as exactly the uppercase names of §4.6. Acceptance policy is never inferred from materiality alone. Unknown values are rejected.

The semantic base is `patch_set.base_semantic_hash`; there is no separate base field. `evidence_refs` and `derivation_refs` are sets: the constructor sorts them by ID and rejects duplicates, and deserialization requires canonical sorted order. `confidence`, when present, is finite and within `0.0..=1.0`; it is advisory metadata and never drives gates, intake, acceptance or readiness. The ID is `prop:<first 16 lowercase hex of the SHA-256 of the RFC 8785 canonical JSON of the complete proposal excluding only id>`; deserialization verifies it and rejects unknown fields.

A proposal carries no job `base_revision` (job state, §21), no impact (derived from `GraphDelta`, §5.6) and no intake report (an assessment of the proposal against a specific graph, §5.4). Findings and questions it proposes are PSG nodes inside its `PatchSet`.

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

Intake receives an immutable `Proposal` (§4.5) and returns a separate `IntakeReport` against a specific current graph. It never mutates the proposal or embeds the report in it; re-running intake against another baseline may produce a different report for the same proposal.

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

External validators execute outside the rule and supply content-addressed `ValidationArtifact`s. At implementation level that input is the canonical `ExternalValidationArtifact` (§28) stored as `external-validation` / `application/json`; no separate `ValidationArtifact` type exists. `ExternalValidationRequest`, `ExternalValidationArtifact` and `ExternalValidationError` are owned by `plumb-validation`, which never depends on `plumb-compiler`; `plumb-compiler` re-exports the two contract types.

The rule engine returns:

```text
PASS
FAIL
WARN
NOT_APPLICABLE
WAIVED
ERROR
```

These six values are the closed `RuleResultState` vocabulary. No seventh state exists; in particular `NOT_IMPLEMENTED` is not a rule result.

**Pilot validation-profile contract.** The active rule pack is `config/profiles/plumb-software-2026.1-rules.yaml`. It is read-only and is loaded, never rewritten, into the typed `ValidationProfile` with exactly its nine top-level fields:

```rust
pub struct ValidationProfile {
    pub profile_id: Id,
    pub profile_version: String,
    pub metamodel: String,
    pub status: String,
    pub rule_classes: BTreeMap<RuleClass, String>,
    pub result_states: Vec<RuleResultState>,
    pub standards: BTreeMap<String, StandardDefinition>,
    pub gates: Vec<GateMetadata>,
    pub rules: Vec<RuleMetadata>,
}

pub struct StandardDefinition { pub standard: String, pub version: String, pub role: MappingRole, pub note: String }

pub struct StandardRef {
    pub standard: String, pub version: String, pub role: MappingRole, pub note: String,
    pub mapping_strength: MappingStrength,
}

pub struct GateMetadata { pub id: GateId, pub purpose: String, pub rule_ids: Vec<String>, pub pass_algorithm: String }

pub struct RuleMetadata {
    pub id: String, pub gate: GateId, pub title: String,
    pub severity: Severity, pub rule_class: RuleClass,
    pub applies_when: String, pub check: String, pub pass_condition: String,
    pub waiver_policy: WaiverPolicy,
    pub standard_reference: Option<StandardRef>, pub remediation: Option<String>,
    pub evaluation: EvaluationMode, pub finding_status_on_fail: String,
}
```

Every fixed-shape struct rejects unknown fields. `GateId` is the `plumb-core` type and `MappingRole` / `MappingStrength` are the `plumb-psg` types; none is redefined. The closed vocabularies and their exact wire strings are:

```text
RuleClass        external_standard, external_interop, plumb_core, profile_policy, organization_policy
RuleResultState  PASS, FAIL, WARN, NOT_APPLICABLE, WAIVED, ERROR
Severity         blocker, error, warn, info
WaiverPolicy     forbidden, decision_required, profile_allow
EvaluationMode   deterministic
```

Rule IDs, severities, classes, waiver policies and texts come from the YAML; no rule-ID enum, per-rule constant or list of rule IDs exists in Rust. A valid profile has exactly the five rule classes, exactly the six result states each once, a standards catalog without duplicate `(standard, version, role)` triples to which every rule `StandardRef` resolves exactly once, 13 gates with each `GateId` once, and 133 uniquely identified rules whose ID set equals the union of the gates' `rule_ids`, each rule listed by exactly the gate it names. Profile identity text, rule text, gate text, rule-class descriptions, standards-catalog keys and the `standard`, `version` and `note` of every `StandardDefinition` and `StandardRef` are non-empty, not surrounded by whitespace and free of control characters (rule text may contain an embedded newline); nothing is trimmed or normalized.

A loaded profile is normalized: `rule_classes` and `standards` in map key order, `result_states` in `RuleResultState::ALL` order, `gates` in `GateId::ALL` order, each gate's `rule_ids` and the `rules` in lexicographic ID order. Two generic `sha256:` hashes are defined over RFC 8785 canonical JSON of that normalized typed model, never over raw YAML bytes, so YAML formatting, comments and ordering do not affect them:

```text
profile_hash     the complete ValidationProfile
rule_pack_hash   exactly { result_states, gates, rules }
```

`profile_hash` identifies the complete active validation/profile declaration; `rule_pack_hash` identifies the gate/rule pack independently of profile identity, rule-class descriptions and the standards catalog. They are the `profile_hash` and `rule_pack_hash` persisted by `GraphRevision`, `CompileContext` and `GateReport`.

The profile loader, this metadata model and the metadata registry are one layer (plan F0.12); the evaluator framework is a later layer (plan F0.13) and is not part of the metadata registry. The pilot's evaluator scope is the NOW gates `I0`, `F1`, `F2`, `F3`, `F4`: for the supplied profile 54 rules require an evaluator and 79 are deferred metadata. Metadata of every gate and rule is always loaded and queryable; explicitly requesting evaluation of a later gate yields the typed `ValidationError::GateNotImplemented(GateId)`, an evaluator-availability error and not a rule result.

**Pilot evaluator framework.** The framework (registration, rule execution, finding generation, waiver application, gate aggregation, `GateReport`; §29) contains no production rule evaluator. Production evaluators are registered gate by gate by the stage tasks that implement them (`I0`, then `F1`, `F2`, `F3`, `F4`), so evaluator availability is gate-incremental: a gate is evaluatable exactly when every rule it owns has exactly one evaluator, unbound rules of one gate never prevent evaluation of another, and no global all-54 binding check exists before the last NOW gate's rules are implemented. Evaluation is pure: it mutates no graph, persists nothing, executes no external validator and has no clock, store, provider, network or compiler capability.

### 5.6 Impact engine

Maintains typed dependency reachability.

A semantic patch creates a `DirtySet` containing directly changed and transitively affected objects. The impact engine receives the exact `GraphDelta` produced by patch application and surfaced through `CommitResult.delta`; it never reapplies the `PatchSet` or diffs revision snapshots to recover the changed set.

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

**Pilot text import contract (plan S0.1).** Importing is a deterministic parse/build step separate from impure acquisition: `import_markdown` / `import_plain_text(display_name, bytes, audit)` return the `SourceArtifact` node, its `EvidenceFragment` nodes and two immutable artifact records, read no clock and persist nothing. The original artifact (`source-original`, the source media type) holds the exact input bytes, and the `SourceArtifact.content_hash` and ID derive from them. The extracted artifact (`source-extracted`, `application/json`) holds the RFC 8785 JSON `{"version":1,"text":<normalized text>}`; the wrapper keeps the two roles byte-distinct because the artifact store keys by byte hash alone. Every fragment locator is a `TextRange` of UTF-8 byte offsets into that normalized text, whose only normalization is CRLF/CR → LF. Source and fragment IDs come from the shared `plumb-psg` helpers that graph validation itself uses. Importer-profile metadata (fragment kind, table header cells, the artifact links) lives in the namespaced node extensions `plumb_import:source_artifacts` and `plumb_import:fragment`, not in new core payload fields.

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
    fn execute(&self, request: &InferenceRequest) -> Result<ProviderExecution, ProviderError>;
}
```

A provider receives only the request: no graph, revision, store, branch, commit capability or artifact store. Artifact persistence belongs to acquisition orchestration outside the provider. A `NullProvider` always returns a typed provider-disabled error; a `MockProvider` replays only executions registered for an exact request ID (it never synthesizes output and may replay requests whose policy names another provider, keeping the recorded provider and model).

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

The job, not the `Proposal`, owns `base_revision`, `base_semantic_hash`, `stage`, `scope`, `profile_hash` and `config_hash`. Staleness is decided from the job's captured revision/head state plus the normal `PatchSet` semantic-hash precondition.

`If-Match` remains correct for API writes, but internal jobs use the same compare-and-swap semantic rule.

---

## 22. Incremental recompilation

A full project compile must be possible, but routine editing must be incremental.

In the pilot every accepted `SemanticPatch` produces an `ImpactReport` computed from the base graph, the result graph and the exact `GraphDelta` of the commit (`CommitResult.delta`):

```text
ChangedSet
DirtySet
AffectedProjectionSet
AffectedGateNamespaceSet
```

`StaleEvidenceSet` (C1/proof staleness) and per-rule affected sets are later work; the pilot works at gate-namespace granularity and exact rules are resolved later through the validation registry.

**Input check.** The delta's base and result semantic hashes must equal those of the two graphs, both graphs have the same project and profile, and the delta's added/removed/modified node and edge sets must equal those recomputed from base-vs-result persisted equality.

**ChangedSet** holds, among the delta's added/removed/modified elements (never the broader touched sets), every added or removed element and every modified element whose element hash (metamodel §20.1) changed. Revision, audit, derivation and View `layout_ref`/`style_ref`-only changes are excluded; status, payload, evidence, standards, tags, extensions and edge semantic fields count, for every node type including types excluded from `semantic_hash`.

**DirtySet.** A changed element is directly dirty when it is baseline-participating in the base or the result graph; elements that are Proposed/Rejected in both stay only in ChangedSet. Directly dirty changed edges form `DirtySet.edge_ids` and add their base and result endpoints to the node seeds, whatever their relation. From the seeds, impact follows the union of the base and result graphs' baseline-participating arcs of exactly these relations, from changed dependency to affected dependent, never in reverse:

| Relation | Impact direction |
|---|---|
| `derived_from`, `constrained_by`, `reads`, `writes`, `governed_by`, `uses_calculation` | `to` → `from` |
| `specified_by`, `satisfied_by`, `allocated_to`, `exposed_by`, `implemented_by`, `verified_by`, `implemented_as` | `from` → `to` |

Removed elements keep their base type, status and topology; added elements use the result graph. Traversal is deterministic, terminates on cycles and never filters by `semantic_hash` participation.

**AffectedProjectionSet** is empty for an empty DirtySet; otherwise `markdown` and `standards-conformance-report` are always affected, a dirty `Extension` node affects all 15 projection kinds, and each dirty node affects the projection families of its node type (implementation plan task F0.11 lists the exact mapping). **AffectedGateNamespaceSet** maps each dirty node's type to exactly one earliest owning gate and marks that gate and every later gate in `GateId::ALL`; all 86 node types are mapped exactly once.

Illustrative long-term example (including post-pilot staleness):

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

compile-run
```

These 18 kinds are the closed artifact-kind list.

Compiler output conventions: an external-validation result is stored as `external-validation` / `application/json` holding the canonical `ExternalValidationArtifact`; a stage derivation patch as `patch`, a proposal as `proposal` and a compile run as `compile-run`, each `application/json` holding canonical JSON.

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

Normative Rust interfaces (`plumb-compiler`). PLAN and EVALUATE are pure contract boundaries: the trait exposes no inference provider, artifact store, revision store, branch, clock, HTTP/network client or commit capability, and a stage cannot acquire artifacts or commit a graph through it.

```rust
pub trait CompilerStage {
    fn id(&self) -> StageId;

    fn plan(
        &self,
        graph: &Graph,
        ctx: &CompileContext,
    ) -> Result<StagePlan, CompilerError>;

    fn evaluate(
        &self,
        graph: &Graph,
        ctx: &CompileContext,
        plan: &StagePlan,
        artifacts: &ArtifactSet,
    ) -> Result<StageEvaluation, CompilerError>;
}
```

```rust
pub enum Scope {
    Project,
    Elements { refs: Vec<Id> },
}

pub struct CompileContext {
    pub input_revision: RevisionId,
    pub input_semantic_hash: Hash,
    pub profile_ref: Id,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,
    pub compiler_version: String,
    pub config: CanonicalJson,
}

pub struct PlannedArtifact {
    pub kind: ArtifactKind,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

pub struct ExternalValidationRequest {
    pub id: Hash,
    pub validator: String,
    pub task_kind: String,
    pub input_artifact_refs: Vec<Hash>,
    pub config: CanonicalJson,
}

pub struct ExternalValidationArtifact {
    pub request_hash: Hash,
    pub validator: String,
    pub validated_output: CanonicalJson,
    pub validated_output_hash: Hash,
}

pub struct ArtifactInput {
    pub hash: Hash,
    pub kind: ArtifactKind,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

pub struct ArtifactSet {
    pub deterministic_artifacts: Vec<ArtifactInput>,
    pub inference_artifacts: Vec<ArtifactInput>,
    pub validation_artifacts: Vec<ArtifactInput>,
}

pub struct StagePlan {
    pub scope: Scope,
    pub deterministic_artifacts: Vec<PlannedArtifact>,
    pub inference_requests: Vec<InferenceRequest>,
    pub external_validation_requests: Vec<ExternalValidationRequest>,
}

pub struct StageEvaluation {
    pub derivation_patch_set: Option<PatchSet>,
    pub proposals: Vec<Proposal>,
}
```

Rules:

- **Scope** serializes as `{"kind":"project"}` or `{"kind":"elements","refs":[...]}`; element refs are non-empty, sorted and unique, and each must be an existing node or edge ID of the graph. Scope is never inferred by name matching.
- **CompileContext** has no timestamp. `input_semantic_hash` is Semantic; `profile_hash` and `rule_pack_hash` are generic; `compiler_version` is non-empty without surrounding whitespace or control characters; `config` is a JSON object whose canonical content hash is `config_hash()`. It is constructed from a loaded revision so revision, semantic hash, profile and rule-pack metadata cannot drift, and it must match the graph's semantic hash and profile ID. No clock or environment is read.
- **PlannedArtifact** has no timestamp (a pure PLAN cannot manufacture one); its media type is non-empty without control characters and its content hash is the generic SHA-256 of its bytes. Acquisition persists it later with an explicit timestamp. Its `bytes` serialize in serde's ordinary JSON byte-array form (`"bytes":[0,1,2,255]`); that is the fixed pilot wire form.
- **ExternalValidationRequest** rejects unknown fields; `validator` matches `^[a-z][a-z0-9._-]*$`; `task_kind` follows the `InferenceRequest` text rule; `input_artifact_refs` are generic, sorted and unique; `config` is a JSON object; `id` is the generic SHA-256 of the canonical `{validator, task_kind, input_artifact_refs, config}` and is verified on deserialization.
- **ExternalValidationArtifact** rejects unknown fields; its hashes are generic, `validated_output_hash` is the content hash of `validated_output`, and against its request `request_hash == request.id` and `validator == request.validator`. It is the replayable validator result supplied to EVALUATE. `artifact_hash()` is the generic SHA-256 of the RFC 8785 canonical JSON of the complete artifact, equal to its artifact-store content hash when persisted as `external-validation` / `application/json`.
- **Ownership.** `ExternalValidationRequest` and `ExternalValidationArtifact` are defined once, in `plumb-validation`, with `ExternalValidationError { InvalidRequest(String), InvalidArtifact(String) }`; `plumb-compiler` re-exports both types under its own public names and maps the error into `CompilerError::InvalidExternalValidationRequest` / `InvalidExternalValidationArtifact`. Wire forms, request IDs and validation rules are those stated above.
- **ArtifactInput** is an artifact without `created_at`: its generic hash equals the SHA-256 of its bytes and its media type is non-empty without control characters. Converting a stored artifact drops the timestamp, so acquisition time is unobservable to evaluation.
- **ArtifactSet** lists are each sorted by hash, and hashes are unique across the set. Inference inputs are `validated-inference` / `application/json`; validation inputs are `external-validation` / `application/json`. Against its `StagePlan` it matches one-to-one: one identical deterministic input per planned artifact, one canonical `InferenceArtifact` input valid for each inference request, one canonical `ExternalValidationArtifact` input valid for each validation request, and nothing extra.
- **StagePlan** lists are ordered by content hash, request ID and request ID respectively, without duplicates; every inference request carries the stage's `StageId`. It contains no timestamps, provider clients or store handles.
- **StageEvaluation** contains only the optional derivation `PatchSet` and the proposals, sorted by ID and unique. The derivation patch set and every proposal patch set have `base_semantic_hash == ctx.input_semantic_hash`, every proposal has the stage's `StageId` and validates. Evaluation never applies or commits its patches.
- **CompilerError** distinguishes contract failures (`Core`, `Artifact`, `InvalidContext`, `InvalidScope`, `InvalidPlannedArtifact`, `InvalidExternalValidationRequest`, `InvalidExternalValidationArtifact`, `InvalidArtifactInput`, `InvalidArtifactSet`, `InvalidStagePlan`, `InvalidStageEvaluation`, `InvalidCompileRun`) from an explicit `StageFailure { stage, code, message }`.

`plan()` and `evaluate()` are pure for the same graph/context/artifacts: equal inputs give byte-identical canonical `StagePlan` and `StageEvaluation`.

---

## 29. Gate API

```rust
pub struct ValidationPolicy {
    pub promoted_to_blocker_rule_ids: BTreeSet<String>,
    pub profile_allow_waiver_rule_ids: BTreeSet<String>,
}

pub struct ValidationContext {
    pub baseline_semantic_hash: Hash,
    pub profile_id: Id,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,
    pub policy: ValidationPolicy,
    pub external_validation_artifacts: Vec<ExternalValidationArtifact>,
}

pub enum Applicability { Applicable, NotApplicable { reason: String } }

pub enum RuleEvaluation {
    Pass { targets: Vec<Id>, evidence: Vec<String> },
    Violation {
        targets: Vec<Id>, evidence: Vec<String>,
        semantic_condition_key: String, message: String, suggested_resolution: Option<String>,
    },
    NotApplicable { reason: String },
}

pub struct EvaluatorFailure { pub code: String, pub message: String, pub targets: Vec<Id>, pub evidence: Vec<String> }

pub type RuleEvaluator =
    fn(&Graph, &ValidationContext, &RuleMetadata) -> Result<RuleEvaluation, EvaluatorFailure>;

pub struct RuleResult {
    pub rule_id: String,
    pub state: RuleResultState,
    pub declared_severity: Severity,
    pub effective_severity: Severity,
    pub applicability: Applicability,
    pub targets: Vec<Id>,
    pub evidence: Vec<String>,
    pub semantic_condition_key: Option<String>,
    pub finding_ref: Option<Id>,
    pub waiver_ref: Option<Id>,
    pub error: Option<EvaluatorFailure>,
}

pub struct GeneratedFinding { pub key: Hash, pub id: Id, pub semantic_condition_key: String, pub payload: Finding }

pub struct AppliedWaiver { pub rule_id: String, pub finding_ref: Id, pub finding_key: Hash, pub decision_ref: Id }

pub enum GateResult { Pass, Fail }

pub struct GateSummary { pub blocker_failed: u32, pub blocker_waived: u32, pub warnings: u32 }

pub struct GateReport {
    pub gate: GateId,
    pub baseline_semantic_hash: Hash,
    pub profile_id: Id,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,
    pub policy: ValidationPolicy,
    pub validation_artifact_refs: Vec<Hash>,
    pub result: GateResult,
    pub rules: Vec<RuleResult>,
    pub findings: Vec<GeneratedFinding>,
    pub waivers: Vec<AppliedWaiver>,
    pub summary: GateSummary,
}

impl EvaluatorRegistry {
    pub fn evaluate_gate(&self, gate: GateId, graph: &Graph, ctx: &ValidationContext)
        -> Result<GateReport, EvaluationError>;
}
```

The report is reproducible for:

```text
semantic_hash
profile_hash
rule_pack_hash
validation policy
validation_artifact_hashes
```

`profile_hash` and `rule_pack_hash` are the two hashes of the typed `ValidationProfile` defined in §5.5; the validation inputs are `ExternalValidationArtifact`s (§28), identified by `artifact_hash()` and held by the context in hash order without duplicates.

- **ValidationPolicy** is a deterministic input separate from the profile: the rulebook's severity promotion and `profile_allow` waiver enablement are project choices the profile YAML does not encode. Both sets default to empty; every ID must exist, `profile_allow_waiver_rule_ids` may hold only `profile_allow` rules, and `policy_hash()` is the generic hash of its canonical JSON. Effective severity is the declared severity, or `blocker` for a promoted rule; nothing demotes a severity.
- **ValidationContext** is built from the graph, the metadata registry, the policy and the external validation artifacts, and is re-checked against the graph and registry before evaluation. Both require `graph.profile_id()` to equal the registry profile's `profile_id`, so a graph is never evaluated under another profile's metadata even though the profile ID also feeds the semantic hash. It has no timestamp, clock, store, provider, network client or compiler type.
- **Applicability** serializes as `{"state":"APPLICABLE"}` or `{"state":"NOT_APPLICABLE","reason":"..."}`. `NOT_APPLICABLE` exists only when an evaluator returns it with a reason; it is never derived from the free-text `applies_when`.
- **State derivation.** Evaluators never choose the final state: `Pass` → `PASS`; `NotApplicable` → `NOT_APPLICABLE`; `Err(EvaluatorFailure)` → `ERROR` (remaining rules still run); `Violation` with effective `blocker`/`error` → `FAIL`; `Violation` with effective `warn`/`info` → `WARN`; a violation with a valid governed waiver → `WAIVED`.
- **Finding identity.** The finding key is the generic SHA-256 of exactly `UTF-8(rule_id) 0x00 (UTF-8(target) 0x00)* UTF-8(semantic_condition_key)` with targets in `Id` order; the Finding node ID is `fnd:` followed by the first 16 hex digits of that digest. Identity depends only on rule, targets and condition key. A `GeneratedFinding` payload has `code` = rule ID, `family` = gate, `severity` = the effective severity mapped one-to-one onto `FindingSeverity`, `status` = the rule's `finding_status_on_fail`, `affected_refs` = the sorted targets, `standard_rule_ref` = the rule ID when the rule has a standard reference, and `waiver_ref` = the applied decision. The framework returns finding material only; node envelopes and persistence belong to stage logic.
- **Waivers** follow metamodel §6.3: an `Accepted` `ResolutionDecision` with the waiver marker and a rationale, linked by an `Accepted` `resolves` edge to the `Accepted` deterministic `Finding`. `forbidden` rules reject a waiver (`ForbiddenWaiver`), `decision_required` rules accept it, `profile_allow` rules accept it only when the policy enables the rule (`ProfileWaiverNotEnabled` otherwise); before any waiver claim is accepted the `Finding` at the deterministic ID must carry the evaluated identity (`code` = rule ID, `family` = gate, `affected_refs` = the sorted targets), otherwise `FindingIdentityMismatch`, because graph validity does not derive Finding IDs from payloads; a malformed marker is `MalformedWaiverDecision` and two qualifying decisions are `AmbiguousWaiver`. No clock is consulted and no waiver expiry is evaluated in the pilot.
- **Gate result.** `blocker_failed` counts effective-blocker rules in `FAIL` or `ERROR`, `blocker_waived` effective-blocker rules in `WAIVED`, `warnings` all `WARN` results. The gate is `PASS` exactly when `blocker_failed == 0`. `evaluate_gate` does not require predecessor gates; claiming a later gate for a formal baseline is an orchestration concern.
- **GateReport** has no `evaluated_at`, timestamp, readiness score, branch or revision ID; the rulebook's sample `evaluated_at` is operational presentation metadata. Rules are ordered by rule ID, findings by Finding ID, waivers by `(rule_id, finding_ref, decision_ref)` and artifact refs by hash, all unique; `content_hash()` is the generic hash of the canonical report.
- **EvaluatorRegistry** owns one metadata registry and a rule-ID → evaluator map. Registration rejects an unknown rule, a rule of a deferred gate and a second evaluator for the same rule. `evaluate_gate` returns `GateNotImplemented` for a later gate and `MissingGateEvaluators { gate, rule_ids }` when the requested gate is not completely bound.
- **EvaluationError** means the framework cannot produce a trustworthy report (`Core`, `Metadata`, `ExternalValidation`, `InvalidContext`, `InvalidPolicy`, `UnknownEvaluatorRule`, `EvaluatorForDeferredRule`, `DuplicateEvaluator`, `MissingGateEvaluators`, `InvalidEvaluatorOutput`, `InvalidSemanticConditionKey`, `InvalidFindingKey`, `InvalidFindingId`, `FindingIdentityMismatch`, `MalformedWaiverDecision`, `ForbiddenWaiver`, `ProfileWaiverNotEnabled`, `AmbiguousWaiver`, `GateReportInvalid`); an `EvaluatorFailure` is rule data that becomes an `ERROR` result.

A readiness score can be computed for UI prioritization, but it never overrides a blocker and is not part of the evaluator API.

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

**functional.yaml v2 compatibility projection (pilot).** `project_functional_v2(graph, profile)` is a one-way PSG → legacy projection: it reads Accepted nodes and edges only, never mutates the graph, produces no patch or proposal, and no importer or writeback from `functional.yaml` exists. The document has exactly `version` (2), `model_hash` and the thirteen legacy collections; `model_hash` is the PSG semantic hash (`psg:sha256:`), replacing the v2 plan's undefined `fm:` hash. The declared projection fields live in companion metadata, never inside the YAML:

```rust
pub struct ProjectionMetadata {
    pub source_semantic_hash: Hash,   // Semantic, the graph's semantic hash
    pub profile_hash: Hash,           // Generic, ValidationProfile::profile_hash()
    pub projection_version: u32,      // 2
    pub content_hash: Hash,           // Generic, SHA-256 of the exact YAML bytes
    pub warnings: Vec<ProjectionNotice>,
    pub lossy_mappings: Vec<ProjectionNotice>,
}
```

Every omission or deterministic collapse is recorded as a `ProjectionNotice { code, source_refs, message }` under a fixed closed code list (plan F0.14); legacy fields are never fabricated from names or text. The document is validated at runtime against `schemas/functional-v2.schema.json`. The full-HR export later runs this same projector over the compiled graph.

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
