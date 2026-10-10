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

**Evidence manifest (plan S0.3).** The canonical `EvidenceManifest` (version 1) lists, for exactly the baseline-participating `SourceArtifact` and `EvidenceFragment` nodes of a graph, each source's content hash and its original/extracted artifact refs and each fragment's source, extracted ref, locator and content hash, sorted by ID, together with the graph's existing evidence hash. Its RFC 8785 bytes form an `evidence-manifest` / `application/json` artifact whose Generic hash is distinct from the `ev:sha256:` evidence hash it records. I0 evaluation receives it with the source artifacts as already-acquired inputs. A persisted manifest is accepted only as exactly one `evidence-manifest` / `application/json` artifact whose bytes are the canonical serialization of a valid manifest equal to the one rebuilt from the graph; zero or several manifests, non-canonical bytes, a stale manifest, or graph evidence that lacks the pilot artifact-link extensions all fail `PLUMB.I0.BASELINE.HASHABLE` as one ordinary violation. The universal I0 checks pass on an empty applicable set; only the provenance rule is not applicable when no `DerivationRecord` exists. Locator, hash-replay and parse-status checks apply to the built-in source kinds `markdown`, `plain_text` and `docx`; content addressing and the evidence baseline cover every source.

**Parse completeness.** Every built-in import records `plumb_import:parse` = `{status, warnings}` on its `SourceArtifact`: `complete` with no warnings, or `partial-with-explicit-unparsed-regions` with one or more located warnings (for DOCX, `W_DOCX_UNSUPPORTED_VISIBLE` with an `XmlPath`). A fatal parse failure (`E_SOURCE_ENCODING`, `E_DOCX_ARCHIVE`, `E_DOCX_MISSING_PART`, `E_DOCX_XML`, `E_DOCX_LIMIT`) produces no source at all, so the I0 parse-status rule reads only these two persisted states and never a default.

**Pilot DOCX import (plan S0.2).** `import_docx` reads `word/document.xml` (required) and `word/styles.xml` (optional) in memory under fixed ZIP safety limits, matching WordprocessingML by namespace URI. Paragraphs and table rows become extracted-text records joined by single LFs (table cells by TAB, merged cells expanded with empty slots, the first row as header); headings come from outline levels and `Heading1`..`Heading6`, list items from `w:numPr`, never from formatting or text. Visible content in unsupported structures is flattened and warned, never dropped. Headers, footers, comments, notes, embedded documents and images are outside the pilot evidence surface.

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

**Pilot requirement segmentation (plan S0.4).** Segmentation is compiler stage `S1` (the plan task number S0.4 is not a stage). Its `InferenceRequest` (`StageId::S1`, task kind `requirement_segmentation`, no input refs) is anchored only to EvidenceFragment IDs as evidence refs and a context of those IDs with their exact extracted text, sorted by ID; the prompt and schema hashes are the Generic hashes of the exact committed bytes of `prompts/s1-segment-requirements.md` and `schemas/inference/s1-segmentation.schema.json`, and the provider policy is always caller-supplied. Acquisition (live provider calls, persistence of the request, raw response and validated inference, and the `llm_inference` derivation) stays outside deterministic evaluation, which receives only a supplied `InferenceArtifact` or its absence. A supplied artifact must pass `validate_for` the request and then JSON Schema Draft 2020-12 validation before its output is decoded and used. Valid output must exactly partition every fragment into requirement_candidate / non_requirement ranges whose offsets are fragment-relative, end-exclusive UTF-8 byte offsets on character boundaries; an unknown fragment, invalid range or overlap is a typed error and a gap is `E_INTAKE_UNCOVERED`, never repaired by fallback. Only an absent, request-invalid or schema-invalid artifact uses the deterministic fallback: a list item is one requirement candidate, other fragments split into ASCII `.`/`?`/`!` sentences classified by the whole-word modals shall, must, should and can, and unselected text is retained as non_requirement flagged `unassigned_text`. Candidate IDs (`seg:`) derive from fragment, range and classification. Fallback provenance is a deterministic `recovery` DerivationRecord node (stage `S1`, inputs the fragments and request ID, outputs the candidate IDs, caller-supplied agent and time) returned detached: segmentation never mutates the PSG, and candidates become requirement nodes only through later S1 intake.

**Pilot requirement compilation (plan S1.1).** The S0.4 `SegmentCandidate` is the stable sub-fragment input: only `requirement_candidate` segments are compiled, their text is read from the parent EvidenceFragment, and each semantic node ID derives from the graph's project ID, the candidate ID and the node type (`req:`, `goal:`, `need:`, `concern:`, `constraint:`). Evidence stays the EvidenceFragment (`Node.evidence`); the exact candidate sub-range is recorded in the namespaced extension `plumb_functional:segment_origin`, so no synthetic fragment is created. A narrow deterministic parser recognizes an optional source prefix (list marker, source identifier, exact bracketed kind label) that yields `source_identifier`, an explicit requirement kind and the clean statement. `requirement_kind`, `level` and `modality` are never defaulted: an explicit kind label overrides inference, level is only inference-proposed, and modality is deterministic from the first normative token (shall, shall not, should, may; must, can, should not and may not stay unresolved). Classification is an `S1` `requirement_classification` inference that is validated against its request and Draft 2020-12 schema before use; absent inference leaves candidates unresolved and malformed inference is an error. Optional Goal, Need, Concern and Constraint nodes come only from validated inference intents, and no relations between them are guessed. Every node is emitted `Proposed`, each in its own `Semantic` / `HUMAN_CONFIRM` Proposal with one `AddNode` patch carrying the classification derivation ref; S1.1 never mutates accepted PSG, calls a provider or persists anything.

**Pilot EARS normalization (plan S1.2).** EARS normalization is a non-authoritative `S1` `ears_normalization` proposal for an existing Proposed or Accepted Requirement compiled by S1.1. The original wording is never stored twice: it is recovered from the node's evidence, the `plumb_functional:segment_origin` range and the S1.1 source-prefix parser, and stays recoverable after the statement is replaced. The model returns only an EARS pattern (ubiquitous, event-driven, state-driven, optional-feature, unwanted-behavior) and grounding references, each an exact byte range of the original source statement or an exact string of an Accepted semantic node; Plumb renders the statement itself with fixed connectives and the Requirement's existing modality, so ungrounded actors, triggers, conditions, responses or numbers cannot enter. Each changed Requirement gets one `Semantic` / `HUMAN_CONFIRM` Proposal with a `ReplacePayload` patch that changes only the statement under an element-hash precondition. Plumb cannot prove semantic equivalence and classifies no materiality: the human confirms that meaning is preserved, and a rewrite judged to change meaning materially is not applied through this path but routed to governed resolution, where a Finding or Question can be resolved by a `ResolutionDecision`. S1.2 itself creates no `ResolutionDecision`, mutates no graph and calls no provider.

**Pilot duplicate analysis (plan S1.3).** Duplicate analysis compares the current `Requirement.statement` of every Proposed or Accepted Requirement pair with equal kind, level, modality and owner/stakeholder scope, after deterministic normalization (Unicode NFKC, lowercase, maximal alphanumeric tokens, nothing removed). A pair matches when the exact token-set Jaccard fraction reaches the caller-supplied, profile-resolved `Decimal` threshold; equal token sequences are exact matches, others near matches. Only Accepted/Accepted matches yield F1 finding material, built with the existing `GeneratedFinding` identity for `PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED`; Proposed matches are analysis data only. A merge is proposed only for an exact match with identical payloads that the canonical patch engine accepts in an in-memory dry run: the generic `MergeNodes` extension-conflict rule is respected, so S1.1 requirements with distinct segment origins are detected but not merged, and near matches always need human review. Supersession direction comes only from a governed Accepted `ResolutionDecision` (resolving an Accepted Finding or Question, with rationale), never from wording or similarity. Merge and supersede proposals are `MaterialDecision` / `HUMAN_DECISION`; S1.3 applies, commits and persists nothing.

**Pilot requirement lint (plan S1.4).** Lint evaluates the current `Requirement.statement`, the canonical wording after any accepted EARS rewrite, with fifteen closed deterministic rules (`PLUMB.LINT.REQ.*`) whose vocabularies and default severities are fixed by the plan. Every diagnostic carries an exact UTF-8 byte range of the current statement; an exact EvidenceFragment sub-range is added only when a validated anchor shows the statement is byte-identical to a fragment slice, and no evidence offset is fabricated for transformed text. Diagnostics are plain typed analysis material, not PSG Findings; the F1 quality rule later consumes them through plumb-lint. The undefined-term rule runs only on vocabulary mention context supplied by later vocabulary analysis and is otherwise reported as not evaluated. The human-labelled benchmark corpus is external qualification data: the framework parses it, counts agreement, computes exact precision/recall and regressions, and recommends a lower severity through configuration, but never mutates rule severities and never claims an empirical precision that was not measured.

**Pilot vocabulary (plan S1.5).** Term discovery is a span-anchored AI proposal over the current `Requirement.statement`, which is the span authority; the model selects byte ranges and optional v3 concept kinds (object_type, fact_type, value_type, role, other) and definition ranges, and never supplies term text, keys or statistics. English-only pilot normalization is deterministic: NFKC, lowercase, alphanumeric tokens, removal of one leading determiner and singularization of the final token through a fixed exception table and suffix rules. Frequency and co-occurrence are computed deterministically, and the same normalized mentions become the S1.4 `TermLintContext`, whose defined keys come only from Accepted Terms and Concepts. A `Concept.definition` is always exact grounded Requirement text, never generated prose, so a typed key without a grounded definition yields no Concept. Accepted vocabulary is never overwritten; new Term and Concept nodes are Proposed in `Semantic` / `HUMAN_CONFIRM` proposals with deterministic IDs and a provenance-only `plumb_functional:vocabulary_origin` extension. Kind, definition and accepted-key conflicts affecting Accepted Requirements become finding material of the canonical `PLUMB.F1.REQ.TERMS_RESOLVED` rule, not resurrected v2 codes.

**Pilot F1 validation (plan S1.6).** F1 consumes, besides the canonical graph, deterministic supplemental inputs compiled before validation: one entry per Accepted Requirement with its S1.4 lint input and its S1.5 vocabulary dependencies (each resolved to an Accepted Term or Concept, a governed external identifier, or unresolved), the current upstream S1.3/S1.5 finding material and the lint policy. The inputs are validated against the graph (exact Accepted coverage, current statements and evidence, valid resolutions and finding identities) and their hash is bound into the F1 `GateReport`; other gates ignore them. F1 never executes S1.3, S1.5, a provider or a contradiction detector: upstream finding material may stay transient until a later stage materializes it, and Accepted Open Finding nodes count as well. Validation aggregates each rule's current condition into one result per rule. MODALITY_EXPLICIT uses the first controlling modal reading, aligned with S1.1 Requirement compilation; the evaluator does not treat subordinate modal verbs after the controlling reading as additional normative modalities, and multi-clause deontic contradictions require governed contradiction finding material and are not inferred lexically by F1 validation.

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

### Pilot domain extraction (S2.1, Hotfix 033)

Accepted Concepts form the closed vocabulary of domain extraction. AI returns Accepted Concept IDs plus exact requirement byte groundings, never new names; Plumb derives every domain name and type from the Accepted Concepts. An Entity derives from an ObjectType Concept. An Attribute has exactly one `has_attribute` owner Entity, a grounded ValueType Concept as its value type and a grounded nullable proposal; nullable is never defaulted. A DomainRelationship derives from a FactType Concept. Cardinality uses the four canonical values `0..1`, `1`, `0..*` and `1..*`, and there is no unresolved sentinel: a missing cardinality yields deterministic `PLUMB.F2.DOMAIN.RELATION_TYPED` finding material until grounded evidence or a governed human decision resolves it. Domain extraction is iterative, so dependent proposals never reference nodes absent from the current Graph; the same inference artifact is replayed after Entity proposals are applied. All domain semantics remain human-confirmed proposals. S2.1 detects conflicting duplicate domain-origin nodes but does not merge Entity, Attribute or DomainRelationship nodes (Hotfix 034): canonical MergeNodes is Requirement/Term-only. Duplicate domain identities block deterministic dependency resolution and are surfaced for later governed repair, and no edge or reference rewriting is performed by S2.1.

### Pilot lifecycle extraction (S2.2, Hotfix 035)

State extraction is inference-only and grounded: a State exists only because validated lifecycle inference names an exact Accepted Requirement byte range, and there is no capitalization, enum-word or field-name fallback. The State name derives deterministically from the grounded text through the S1.5 vocabulary normalization, and the State owner is the canonical `has_state` relation from an active StateOwner. The Transition trigger is the canonical `transitions_via` relation to exactly one Operation or Event; a grounded transition candidate without a resolved trigger is not a Transition node but deterministic `PLUMB.F2.STATE.TRANSITION_COMPLETE` finding material (`state_transition_trigger_unresolved:<candidate-id>`), resolvable by a governed human decision. State proposals may be applied before Transition proposals, and the same request and artifact are replayed because States are not part of the lifecycle context. An Invariant expression is an unqualified Proposed PlumbExpr candidate until the S2.4 grammar and S2.5 typechecker exist; S2.2 makes no parse or typecheck claim. No initial, terminal or externally-entered state semantics are invented.

### Pilot data classification (S2.3, Hotfix 036)

S2.3 classifies Accepted Attributes only. A versioned deterministic dictionary (version 1) is looked up by exact equality of the S1.5-normalized Attribute name: email, e mail, email address and e mail address map to `pii`; dob, date of birth and birth date to `pii`; salary to `financial`; iban and international bank account number to `financial`. A dictionary hit on an unclassified Accepted Attribute yields a Semantic `AUTO_DERIVATION` `ReplacePayload` proposal that changes only `data_classification`. The pilot classifications are exactly `pii`, `financial` and `confidential`; `None` means unresolved, nothing is defaulted, and `"none"` is never stored (it is only the legacy functional-v2 projection of `None`). Inference classifies only non-dictionary Attributes, is restricted to the same three values, and each inference-origin change is a separate `HUMAN_CONFIRM` proposal. Existing classifications are never overwritten; disagreements and unsupported stored values are reported as conflicts. The inference request excludes the current classification, so the same artifact stays replayable across applied classification proposals. No Finding or Question is created, because no active validation rule requires a classification.

### Pilot PlumbExpr language v1 (S2.4, Hotfix 037)

**Version and boundary.** `PLUMB_EXPR_LANGUAGE_VERSION = 1`; any incompatible lexical, grammar, precedence or canonical-printing change increments it. S2.4 defines syntax only: a pest grammar, a strong Rust AST of explicit syntax variants (not semantically typed nodes), `Ty` definitions and a canonical unparser. TypeEnv, identifier resolution, function signatures, semantic typing, unit arithmetic, evaluation, calendars and rounding belong to S2.5 and later. Expressions in the PSG remain `String`; the canonical persisted form is canonical source text, and there is no AST JSON wire contract.

**Lexis.** A symbol is `[A-Za-z_][A-Za-z0-9_]*`, case-sensitive; a dotted identifier is `symbol ("." symbol)*` and is a static symbolic path, not reflection, method invocation or PSG traversal. Reserved words (lowercase, token-bounded): `true false and or not in if then else date datetime duration quantity`. ASCII space, tab, CR and LF are insignificant between tokens; there are no comments.

**Literals.** Int `0 | [1-9][0-9]*` (i64; overflow is an error). Decimal `0.[0-9]+ | [1-9][0-9]*.[0-9]+` (`rust_decimal::Decimal`, scale preserved; no exponents, leading zeros or bare points). Bool `true | false`. String in double quotes with direct UTF-8 except quote, backslash and ASCII controls, and exactly the escapes `\" \\ \n \r \t \b \f`. `date("YYYY-MM-DD")` (real Gregorian date, years 0000..=9999). `datetime("<RFC 3339>")` reusing `plumb_core::Timestamp` (offsets normalized to UTC; canonical nine fractional digits and `Z`). `duration(<number>, <unit>)` and `quantity(<number>, <unit>)` with an unsigned integer or decimal and a Unit; negative values use unary minus outside the constructor. A Unit is an open canonical symbol `[a-z][a-z0-9_]*` compared by exact equality; no conversion or dimensional algebra exists. There are no enum, ref or null literals: such names are identifiers resolved by S2.5.

**Syntax.** Lists `[]`, `[e, ...]` and calls `f()`, `f(e, ...)` without trailing commas; a callee is one simple non-reserved symbol, any such symbol parses, and a parsed call claims nothing about existence, typing or execution. Operators: unary `-` and `not`; `+ - * / %`; `== != < <= > >= in`; `and`, `or`; implication `->`; and `if c then a else b` with mandatory `else`. No aliases (`=`, `<>`, `&&`, `||`, `!`, `**`, `^`, `?:`).

**Precedence (lowest to highest).** 1 `if ... then ... else ...`; 2 `->`; 3 `or`; 4 `and`; 5 `== != < <= > >= in`; 6 `+ -`; 7 `* / %`; 8 unary `-` and `not`; 9 primaries (literal, identifier, list, call, parenthesized expression). `->` is right-associative; `or`, `and`, `+ -` and `* / %` are left-associative; comparisons are non-associative, so unparenthesized chains are rejected; unary operators are right-associative. Parentheses only group.

**AST and types.** `Ast` is exactly Literal, Identifier, List, Call {function: Symbol, args}, Unary {op, expr}, Binary {op, left, right} and Conditional {condition, then_expr, else_expr}. `Literal` is Int, Decimal, Bool, String, Date, DateTime, Duration {value, unit} and Quantity {value, unit}. `BinaryOp` is Add, Subtract, Multiply, Divide, Remainder, Equal, NotEqual, LessThan, LessThanOrEqual, GreaterThan, GreaterThanOrEqual, In, And, Or and Implies. `Ty` is Int, Decimal(scale 0..=28), Bool, String (added by Hotfix 037 for the existing HR String value type), Date, DateTime, Duration(Unit), Quantity(Unit), Enum(Id), Ref(Id) and List(Ty); there is no Optional, Null or Any type.

**Canonical unparse and errors.** `parse` and `unparse` are pure; canonical output has single spaces around binary operators, `-a`, `not a`, `", "` separators, no inner padding, preserved decimal scale and exactly the parentheses needed so that `parse(unparse(ast)) == ast`; canonicalization is idempotent. `ParseError` distinguishes Empty, Syntax, InvalidInteger, InvalidDecimal, InvalidString, InvalidDate, InvalidDateTime and InvalidUnit with zero-based UTF-8 byte offsets; pest errors are never exposed.

**No scripting or execution.** There are no assignments, declarations, lambdas, loops, imports, reflection, eval, method calls, indexing, statements or blocks, and parsing never evaluates, resolves, calls functions or reads graphs, clocks, files, network or environment.

**Forward contract (S2.5, S2.10).** S2.5 defines TypeEnv and semantic rules over this AST, including mappings for PSG value_type strings covering at least String, Enum, Date, DateTime, Int, Bool and Decimal(n); freezes callable signatures and evaluation instead of dynamically executing call names; defines the typing of `in`, implication, conditionals, numeric operators, lists, Date/DateTime, Duration/Quantity and unit equality; and does not change this grammar for convenience. The PSG-to-PlumbExpr type-environment mapping is frozen after S2.5 and before `PLUMB.F2.INVARIANT.EXPRESSIBLE` is implemented (S2.10).

### Pilot PlumbExpr semantics (S2.5, Hotfix 038)

**Boundary.** `plumb-expr` stays PSG-agnostic. Type environments, value environments, calendars and the domain predicate service are explicit inputs; the crate never inspects a graph, searches by name, infers enum identities, reads fixtures, configuration or the clock, persists anything or calls external code. A future PSG adapter builds the environments.

**TypeEnv and ValueEnv.** `TypeEnv` holds root bindings `Symbol -> Ty` (a conflicting rebinding is an error; identical rebinding is idempotent), Ref field schemas `ref type Id -> Symbol -> Ty`, enum members `enum Id -> {Symbol}` and an optional calendar ref type. A dotted identifier resolves its root and then requires `Ref(R)` with the next symbol in R's schema. Bare names resolve only through root bindings, or as a registered member of `Enum(E)` where the context expects that enum; a member without enum context is an unknown identifier, and no enum is chosen by uniqueness. An enum-valued Attribute uses its Attribute ID as `Ty::Enum` identity. The pure pilot mapper accepts exactly `String`, `Int`, `Bool`, `Date`, `DateTime`, `Decimal(n)` (n 0..=28) and `Enum` (identity mandatory); `Int` or `Decimal(n)` with a unit maps to `Quantity(unit)`; a unit on another type and any other string (no `Text` alias) are errors. Runtime values are Int, Decimal, Bool, String, Date, DateTime, Duration, Quantity, Enum {enum_ref, member}, Ref {type_ref, value_ref, fields} and List; there is no Null, Optional or Any. A known identifier without a runtime value is `MissingValue`, not a type error; consumed values are checked against their types.

**Typing.** Scalars are Int and Decimal: addition, subtraction and comparison promote Int to Decimal(s) and take the larger scale; multiplication gives Int, Decimal(s) or Decimal(min(28, a+b)); division always gives Decimal(28) with checked decimal division; remainder is `a - trunc(a / b) * b`. Integer arithmetic is checked and never promoted on overflow; decimals never use floats. Quantity and Duration add and subtract only with the identical unit, multiply by scalars and divide by scalars; there is no conversion, derived unit or cancellation. `Date ± Duration(day)` gives Date with an integral day count, `Date - Date` gives `Duration(day)`, and DateTime supports only equality and ordering. Equality takes identical or compatible types (same enum identity, same ref type compared by reference identity); ordering takes scalars, dates, date-times and same-unit quantities or durations. The exact numeric literal zero may stand for a Quantity or Duration only in comparisons and conditional branches (`remaining_days >= 0`, `if … then RequestedWorkingDays else 0`). Boolean operators require Bool, `a -> b` is `(not a) or b`, and evaluation short-circuits `and`, `or`, `->` and conditionals. `x in list` requires a list of a compatible element type, `x in []` is false, and enum membership uses lists of contextual members.

**Callables.** Only `exists`, `direct_manager`, `working_days`, `days_between`, `sum`, `min`, `max`, `count`, `any` and `all` execute; any other call name is an unknown function, `as_of` is explicitly deferred, and there is no call-by-name facility. `exists(identifier)` tests presence without Null. `direct_manager(Ref, Ref)` asks the injected predicate provider. `working_days([start, end], calendar, inclusive_end)` returns `Quantity(working_day)`: the start always counts, the end only when inclusive, a reversed or non-two-date period is an error, and half days come from ordinary multiplication. `days_between(start, end)` counts calendar-day boundaries (Friday to Monday is 3) and rejects reversed periods. Aggregates `sum`, `min`, `max` fail on empty lists; `count([])` is 0, `any([])` false, `all([])` true.

**Calendars.** `CalendarProvider::is_working_day(calendar, date)` is injected; the static provider holds programmatic definitions (ID, weekend weekdays, holidays), a working day is a non-weekend non-holiday date, and an unknown calendar is an error. The business-calendar API is Date-only; no time zone is consulted.

**Rounding.** `RoundingSpec` is HalfUp, HalfEven, Floor or Ceil with a scale 0..=28, or ToStep with a positive step. HalfUp rounds midpoints away from zero, HalfEven to the even digit, Floor toward negative infinity, Ceil toward positive infinity, and ToStep to the nearest multiple with ties away from zero at the step's scale. The canonical `Calculation.rounding` strings are exactly `half_up(n)`, `half_even(n)`, `floor(n)`, `ceil(n)` and `to_step(d)`. Scale-based results carry exactly the requested scale (`1.2` with `half_up(2)` is `1.20`); `d` uses rust_decimal value syntax, so `to_step(5)` is valid and exponential notation is not (Hotfix 039).

**Renderer.** The example renderer is one deterministic English template: `Given <id> = <value>, …; <canonical expression> evaluates to <result>.`, with referenced identifiers deduplicated and sorted and absent values shown as `<missing>`. It is documentation output, not natural-language generation. Since Hotfix 039 the renderer also receives the TypeEnv: an identifier is a Given input iff its root is an explicit TypeEnv root binding, so a known but absent input still shows `<missing>`, while a contextual enum member (no root binding, resolved by typing as a member of the expected enum, e.g. `Approved` in `LeaveRequest.status == Approved`) is a constant and is never listed.

### Pilot calculations and decision tables (S2.6, Hotfix 040)

**Boundary.** S2.6 proposes grounded Calculations and qualifies them through the S2.4 parser and S2.5 type checker; it also provides a typed DecisionTable analysis engine. It emits no F2 Finding: S2.10 maps S2.6 analysis material to `PLUMB.F2.CALC.TYPECHECK`, `PLUMB.F2.CALC.NO_CYCLE`, `PLUMB.F2.TIME.CALENDAR_DEFINED`, `DMN.F2.TABLE.NO_OVERLAP` and `DMN.F2.TABLE.COVERAGE`, reusing this adapter and analysis rather than rebuilding them. No v2 gap code is restored and no relation kind is added; calculation dependencies are analysis material, not edges.

**Explicit scope bindings.** PSG names are semantic labels and are never rewritten into PlumbExpr identifiers. A calculation scope is a list of explicit bindings `{node_ref, symbol, exposure}`: a Root exposure binds an Accepted Entity (`Ref(entity id)`), Attribute (its pilot-mapped type; enums use the Attribute ID and its exact members) or Calculation (its declared result type and unit) to a top-level symbol; a Field exposure binds an Accepted Attribute as a field of its canonical owning Entity. Symbols must already be valid S2.4 symbols; one Attribute may have both exposures; root and field collisions are rejected. A default binding from a payload name exists only when that exact name is already a valid symbol (`Start Date` needs an explicit alias such as `start_date`). The scope also lists the Accepted Calendars inference may select.

**Reserved calendar.** The root symbol `calendar` is reserved and never an ordinary binding. Its only source is `Calculation.calendar_ref`: for an Accepted Calendar C the TypeEnv uses C itself as the calendar ref type, binds `calendar` to `Ref(C)`, and evaluation injects `Ref {type_ref: C, value_ref: C}`, so no Calendar name or synthetic type ID is needed. A `working_days` call requires a valid calendar reference; `days_between` does not. The canonical Calendar payload is not turned into holiday data: evaluation always uses an injected CalendarProvider.

**Qualification and provenance.** Inference returns grounded name and evidence ranges and a PlumbExpr string, never dependencies or input lists. Plumb stores the canonical unparsed expression, derives the bindings the expression actually uses from its AST (contextual enum members and the reserved calendar are not bindings), and checks the actual type against the declared result type and unit (Decimal(n) accepts Int and Decimal(m ≤ n); no unit is gained or lost) and rounding (S2.5 RoundingSpec, Decimal and Quantity results only, applied to the final value). The calculation origin records the exact used bindings, so an Accepted Calculation created by S2.6 replays the same TypeEnv; stale bindings are reported, and a legacy Accepted Calculation without that origin needs an explicit scope override instead of a guessed one. Dependencies follow Root bindings to Accepted Calculations, and self-cycles and multi-node cycles are reported deterministically.

**Decision tables.** No canonical encoding of `DecisionTable.inputs`, `outputs` and `rows` is frozen, so S2.6 invents none and produces no DecisionTable proposal. Its typed analysis supports Bool, Enum, Int and Decimal input columns with Any, value, enum-set and exact interval cells (Int discrete, Decimal continuous, no floats), the hit policies `UNIQUE` (overlaps illegal) and `FIRST` (row order significant, overlaps informational), a total default output, exact finite coverage up to 4096 points with at most 32 witnesses, and exact single-column numeric coverage over a declared finite domain; other mixes are reported as not analyzable rather than approximated.

### Pilot operations, outcomes and events (S2.7, Hotfix 041)

**Boundary.** S2.7 proposes AI-assisted Operations (kind `command` or `query`) with their typed relations, Outcomes and Events. AI proposes, Plumb validates, a human confirms: every Operation proposal is Semantic and HumanConfirm. It uses the existing relations `performed_by`, `reads`, `writes`, `produces`, `consumes`, `governed_by`, `uses_calculation` and `specified_by` exactly as registered and adds none. Authorization (Principal, SecurityRole, Permission and related nodes) belongs to S2.8, processes and sequencing to S2.9 and F2 findings to S2.10; S2.7 creates no DataSchema, EventContract, Message, Channel or API schema and extracts no Rule or DecisionTable.

**Closed operation scope.** Inference sees an explicit, sorted scope instead of the whole graph: Accepted Actor or BusinessRole performers (never SecurityRole), Accepted Entity or Attribute domain elements (the only legal read and write targets), Accepted Calculations whose S2.6 qualification replays from their origin without a legacy override, Accepted Rule or DecisionTable governors (no Invariant), Accepted Events, Accepted DataSchemas, Accepted Attributes usable as Event payloads and read-only support context (criteria, invariants, rules, decision tables, calculations) that never creates relations by itself. Every relation the model proposes is a scope ID grounded in exact Requirement byte ranges; an unknown or out-of-scope ID is an artifact error, never resolved by name or fuzzy matching.

**Entity and attribute reads and writes.** Reads and writes target Entities or Attributes. A query cannot write. Writes come only from grounded inference, and reads or writes suggested by criteria, invariants or rules remain AI proposals for human confirmation; no regex or identifier parser turns support prose into edges.

**Calculation read completeness.** Selecting a Calculation (`uses_calculation`) triggers a deterministic consistency check: Plumb replays the S2.6 qualification of the Calculation and, recursively, of its Accepted Calculation dependencies, and collects the Entities and Attributes in their used bindings. An Attribute is covered by reading it or its owning Entity; an Entity only by reading the Entity. An uncovered input blocks the proposal (`MissingCalculationRead`), and a dependency that cannot be replayed, is unqualified, has unresolved scope or is cyclic makes the check unavailable; Plumb never silently adds reads or writes.

**Outcomes and events.** Each Outcome is a typed node (`success`, `business_failure`, `technical_failure`, `partial`) whose identity is scoped to its Operation and which the Operation `produces`. Events are functional business events whose identity is project-global by name; a new Event may be produced, an existing Accepted Event may be produced or consumed, and no Event is created only to be consumed. A new Event's payload reference is absent, an existing Accepted DataSchema or a single Accepted Attribute, never an Entity; its `semantic_type` stays empty. Operation input and output schemas may only reference existing Accepted DataSchemas. Existing Events are reused only when compatible and never overwritten; a same-name Event under another ID is a conflict.

**Unfrozen vocabularies.** The pilot leaves Operation preconditions, postconditions, idempotency and transaction semantics empty: there is no persisted operation-expression scope comparable to the S2.6 calculation origin and no authoritative vocabulary, so none is invented. Event semantic types are likewise left empty until S2.10 freezes an explicit applicability contract.

**HR reference.** The immutable HR fixture's `operation_specs` is the S2.7 oracle for operation kinds, performers, reads, writes, outcome names and event names, not for outcome kinds, schemas, conditions or event types. It supersedes the inherited v2 assertion that ApproveLeaveRequest reads `LeaveBalance.remaining_days`, `Employee.manager` and `Contract.type`: ApproveLeaveRequest reads the five fixture Entities LeaveRequest, ManagerAssignment, EmploymentContract, LeaveBalance and LeaveType and writes LeaveRequest, ApprovalRecord, LeaveBalance and AuditRecord, and no legacy Attribute is invented. Its performer is `ManagerRole` from `operation_specs`; the older `critical_relations` entry naming `ManagerBusinessRole`, which `role_specs` does not define, is stale for S2.7 operation acceptance.

**Downstream boundaries.** S2.10 validates the canonical relations and payload references that S2.7 proposals produce once accepted, without rerunning S2.7 inference or inferring Event relations from names. S2.8 maps authorization separately; BusinessRole and SecurityRole stay distinct. S2.9 must not assume Operation preconditions or postconditions exist.

### Pilot authorization semantics (S2.8, Hotfix 042)

**Boundary.** Authorization is security-sensitive, so S2.8 runs no inference: it deterministically compiles explicitly supplied, structured authorization intent (from a UI, a human decision, an import or a fixture adapter) into HumanConfirm proposals and provides reusable hierarchy and separation-of-duty analysis. It uses the existing Principal, SecurityRole, Permission, ResourceScope, PolicyCondition and SeparationConstraint payloads and the relations `assigned_role`, `inherits_role`, `grants`, `permits`, `scoped_to` and `conditioned_by` unchanged. It does not do authentication, runtime policy evaluation, ABAC, condition execution, dynamic separation of duty or F2 findings.

**Separate role namespaces.** BusinessRole (who does the work) and SecurityRole (what a subject may do) are distinct node types that are never aliased or merged, even when their names coincide. There is no relation from BusinessRole to SecurityRole and none is inferred, by name or otherwise; security membership exists only as `assigned_role` from a Principal or Actor to a SecurityRole.

**Structured drafts.** A draft lists security roles, principals, permissions, assignments, inheritances and separation constraints. Every reference is an explicit ID: an existing node or a node declared in the same draft under its deterministic ID. A permission names its owning SecurityRole, an Accepted Operation, an explicit resource (an Accepted Operation, Entity or Attribute in the pilot), a scope kind and optional conditions. Scope kinds such as `owned` or `direct_reports` are preserved verbatim and never evaluated; condition expressions are opaque text, not PlumbExpr. Each permission compiles to one Permission (named `<role> -> <operation> [<scope kind>]`) granted by exactly one SecurityRole, with exactly one `permits`, exactly one `scoped_to` and one `conditioned_by` per condition.

**Hierarchy.** `A inherits_role B` means role A inherits B's permissions and membership, so effective roles follow outgoing inheritance transitively. Self-cycles and longer cycles are reported deterministically; a cycle in the candidate overlay blocks the proposal, since inheritance must stay acyclic.

**Separation of duty.** All four constraint kinds can be represented, but only static separation of duty is evaluated: a subject violates a constraint when at least two of its roles are among the subject's direct or inherited roles. Violations are reported, not resolved, and do not remove the constraint; the other kinds are reported as not evaluated, and no protected operations are inferred.

**Reconciliation and proposals.** Existing identical nodes and edges are reused; any difference at a deterministic identity, or an existing SecurityRole with the same name under another ID, is a conflict that is never repaired in place. One draft yields at most one compound proposal containing every missing element in a fixed order, dry-run against the full graph and registry validation.

**HR reference.** The HR fixture's two security roles and nine permission rows compile exactly. Its business roles stay business roles; it has no principals, assignments, hierarchy or separation constraints, and none is invented (in particular, no business-to-security role mapping and no separation constraint derived from the self-decision invariant). Because the fixture names no protected resource, its test adapter uses each row's operation as the resource; production input always states the resource explicitly.

**Downstream.** S2.10 reuses these analyzers and the canonical relations for the RBAC and separation rules instead of re-implementing them, and must freeze explicit contracts before evaluating the other separation kinds or policy conditions.

### Pilot process semantics (S2.9, Hotfix 043)

**Boundary.** S2.9 proposes AI-assisted Processes as HumanConfirm proposals over a closed scope of Accepted Operations, performers, Events, Outcomes and Transitions, and provides a reusable structural analyzer. It is not a process engine: no interpreter, BPMN XML, Mermaid, scenario execution, authorization or F2 findings.

**What the current PSG can represent.** `next` links two ProcessNodes and carries no properties, and `condition_expr` is a single optional string on a node, so distinct conditions on the outgoing flows of an exclusive gateway cannot be expressed. Error events have no error reference and subprocesses have no reference to the called process. S2.9 therefore supports starts (manual, Event-triggered or timer-triggered), ends, human and service tasks backed by an Operation, operation-less human activities that have both a performer and a produced Outcome or Event, Event waits, timer waits and simple structured parallel split/join. Exclusive gateways, error events and subprocesses are recognized but never proposed, nodes created by S2.9 carry no condition, and an existing node with a condition marks its Process as not fully qualified. Nothing is added to the metamodel to work around these limits.

**Membership and flow.** A ProcessNode belongs to the Process named by its `process_ref`; there is no containment relation. Control flow is `next` between members of the same Process. Tasks and waits have exactly one incoming and one outgoing flow, starts only an outgoing one and ends only incoming ones; branching and joining happen only at explicit parallel split and join nodes. Every node must be reachable from a start and every node without successors must be an end. `message_ref` names a functional Event (not a technical Message) that the start or wait consumes; timer text is preserved and never parsed.

**Parallel structure.** For each split, the matched join is the unique nearest join reachable from all of its branches. Each branch must reach that join without merging into another branch first, branches may not contain further splits or joins (no nesting in the pilot), every join must belong to exactly one split, and parallel flow with a cycle is not supported.

**Where the analyzer lives.** The pure process analyzer sits in `plumb-validation`, below `plumb-functional`, with an Accepted-only mode for later validation and an active (Proposed plus Accepted) mode used to qualify S2.9 proposals on an in-memory dry-run. It contains no evaluator. S2.10 consumes it directly instead of writing a second analyzer, must not report a Process whose correctness depends on unsupported semantics as proven, and must first resolve the fact that the S2.6 and S2.8 analysis kernels live in `plumb-functional`, which `plumb-validation` cannot import without a dependency cycle.

**Order is not sequence.** Sequence comes only from grounded inference confirmed by a human. Requirement and paragraph order never create, remove or reorder `next` relations, and the inference context contains no document positions, so reordering requirements cannot change accepted process flow.

**HR reference.** The immutable fixture's `process_specs` (six processes with ordered operation lists) is the current HR process reference and compiles to linear processes, read literally, including UpdateLeaveBalance as reserve then restore. The inherited v2 expectation of a contractor branch is superseded for current HR reference testing and is not fabricated.

### F2 validation boundary (S2.10, Hotfix 044)

**Dependency direction.** `plumb-functional` depends on `plumb-validation`, never the reverse. The pure analysis kernels that F2 validation reuses therefore live in `plumb-validation`: the S2.9 process analyzer, an expression-scope adapter (explicit PSG-node-to-PlumbExpr symbol bindings and type environment, exactly the S2.6 binding semantics), calculation qualification and dependency/cycle analysis, typed decision-table analysis, and authorization hierarchy and static separation-of-duty analysis. A prerequisite relocation commit moves the S2.6 and S2.8 kernels there before S2.10; `plumb-functional` keeps inference, requests, proposals and compilation and calls the relocated kernels through source-compatible facades. Each algorithm exists exactly once.

**Supplemental inputs.** Some F2 obligations need material the PSG cannot yet express. The validation context carries optional, content-hashed F2 inputs, validated against the graph like the F1 inputs: the current S2.1 unresolved-cardinality and S2.2 unresolved-trigger findings (and nothing else); explicit scope overrides for Accepted Calculations without an S2.6 origin (forbidden for those with one); one typed decision-table specification per Accepted DecisionTable whose hit policy matches the PSG (the generic row encoding is not frozen and is never decoded); and one explicit binding set per Accepted Invariant. Missing or stale inputs are context errors; rules that need inputs report an error without them, so the gate cannot pass by omission.

**Evaluator semantics.** All 24 F2 rules are registered. Evaluators read Accepted PSG semantics, the supplemental inputs, existing finding state and the shared kernels; they never infer, match by name, mutate or persist. Where the PSG cannot represent what a rule needs, the result is an error with a stable code rather than a pass or an invented convention: state reachability (no initial or terminal markers), event producer and consumer obligations (no internal/external or purpose classification), and processes containing exclusive gateways, error events, subprocesses or conditions. Static separation of duty is the only separation kind evaluated, policy conditions are never claimed as PlumbExpr, and the semantic-blocker rule checks open F2 blocker findings entering the gate rather than results of the same run.

**Ratified decisions (Hotfix 045).** An Invariant is a predicate: it must parse, type-check under its explicit scope and produce a Boolean, so a well-typed non-Boolean expression fails. When some applicable material fails while other applicable material cannot be analyzed, the rule reports an error, because a trustworthy failure needs the complete applicable evaluation. Process entry and completion consider the boundary violations that touch start or end nodes, while reachability considers every cross-process flow violation. A supplemental finding blocks only while all its affected elements remain Accepted. The core profile narrows a concrete Permission to exactly one Operation and one ResourceScope. Because baseline graph validation already rejects an Accepted role-hierarchy cycle, that rule's failure branch is qualified through the graph-level rejection and the shared kernel rather than an unvalidated graph.

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

### Deterministic question generation (S3.1, Hotfix 045)

**Input.** Question generation consumes current `GeneratedFinding` material, because the persisted Finding payload has no semantic condition key, plus a parsed stakeholder routing configuration and caller audit. It never evaluates gates, reruns inference, reads files or derives conditions from Finding IDs. Waived findings produce no question, and findings whose affected targets are no longer Accepted are reported as stale.

**Templates.** A closed registry maps exactly six typed conditions to questions using the existing `QuestionKind`: unresolved relationship cardinality (Cardinality), unresolved transition trigger (PickOne over Accepted Operations and Events), missing operation performer (RoleAssignment over Accepted Actors and BusinessRoles), missing business calendar (Calendar), non-concrete permission (Permission over Accepted Operations and ResourceScopes) and inexpressible invariant (FormulaConfirm). Each has a versioned template ID, a fixed prompt and a closed JSON answer schema whose choices are sorted Accepted IDs. Aggregate conditions that do not say what the human must decide (for example calculation qualification, decision-table overlap or coverage, process structure) are returned as unmapped rather than turned into unanswerable questions.

**Priority.** Priority is the severity weight (blocker 4, error 3, warn 2, info 1) times the finding's blast radius: the number of nodes reachable from its affected elements through the §22 impact relations and directions, computed by a shared impact-closure function rather than a synthetic patch. It is stored as a canonical integer string.

**Routing.** Each template has a routing key; the configured candidates for that key (else `default`) are a preference list, and the first that is an Accepted Stakeholder receives the question. Otherwise the smallest Accepted stakeholder holding the `analyst` role receives it, and otherwise the question stays unassigned. No stakeholder is created or matched by name.

**Identity and materialization.** Duplicate suppression and the Question ID both use the template ID and the canonical slot object (including choices, priority and route), never text similarity. Missing question-driving Findings and new Questions are added as Accepted governance nodes in one non-semantic, auto-acceptable proposal that leaves `semantic_hash` unchanged. Existing nodes are reused only when their generated content matches exactly; lifecycle fields set by later steps are ignored, and any other mismatch is a conflict rather than a rewrite.

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
- **ValidationContext** is built from the graph, the metadata registry, the policy and the external validation artifacts, and is re-checked against the graph and registry before evaluation. Both require `graph.profile_id()` to equal the registry profile's `profile_id`, so a graph is never evaluated under another profile's metadata even though the profile ID also feeds the semantic hash. The context also carries `baseline_evidence_hash` (an Evidence hash supplied by orchestration, whose agreement with the graph is the I0 `PLUMB.I0.BASELINE.HASHABLE` rule result rather than a context invariant) and `evidence_artifacts`: already-acquired `ValidationArtifactInput { hash, kind, media_type, bytes }` values of kind `source-original`, `source-extracted` or `evidence-manifest`, each with a Generic hash of its exact bytes, without `created_at`, sorted by hash and unique. Missing evidence a rule needs is that rule's violation. Evaluators read these bytes but never acquire them: no store, filesystem, clock, network or compiler capability reaches a rule. It has no timestamp, clock, store, provider, network client or compiler type.
- **Applicability** serializes as `{"state":"APPLICABLE"}` or `{"state":"NOT_APPLICABLE","reason":"..."}`. `NOT_APPLICABLE` exists only when an evaluator returns it with a reason; it is never derived from the free-text `applies_when`.
- **State derivation.** Evaluators never choose the final state: `Pass` → `PASS`; `NotApplicable` → `NOT_APPLICABLE`; `Err(EvaluatorFailure)` → `ERROR` (remaining rules still run); `Violation` with effective `blocker`/`error` → `FAIL`; `Violation` with effective `warn`/`info` → `WARN`; a violation with a valid governed waiver → `WAIVED`.
- **Finding identity.** The finding key is the generic SHA-256 of exactly `UTF-8(rule_id) 0x00 (UTF-8(target) 0x00)* UTF-8(semantic_condition_key)` with targets in `Id` order; the Finding node ID is `fnd:` followed by the first 16 hex digits of that digest. Identity depends only on rule, targets and condition key. A `GeneratedFinding` payload has `code` = rule ID, `family` = gate, `severity` = the effective severity mapped one-to-one onto `FindingSeverity`, `status` = the rule's `finding_status_on_fail`, `affected_refs` = the sorted targets, `standard_rule_ref` = the rule ID when the rule has a standard reference, and `waiver_ref` = the applied decision. The framework returns finding material only; node envelopes and persistence belong to stage logic.
- **Waivers** follow metamodel §6.3: an `Accepted` `ResolutionDecision` with the waiver marker and a rationale, linked by an `Accepted` `resolves` edge to the `Accepted` deterministic `Finding`. `forbidden` rules reject a waiver (`ForbiddenWaiver`), `decision_required` rules accept it, `profile_allow` rules accept it only when the policy enables the rule (`ProfileWaiverNotEnabled` otherwise); before any waiver claim is accepted the `Finding` at the deterministic ID must carry the evaluated identity (`code` = rule ID, `family` = gate, `affected_refs` = the sorted targets), otherwise `FindingIdentityMismatch`, because graph validity does not derive Finding IDs from payloads; a malformed marker is `MalformedWaiverDecision` and two qualifying decisions are `AmbiguousWaiver`. No clock is consulted and no waiver expiry is evaluated in the pilot.
- **Gate result.** `blocker_failed` counts effective-blocker rules in `FAIL` or `ERROR`, `blocker_waived` effective-blocker rules in `WAIVED`, `warnings` all `WARN` results. The gate is `PASS` exactly when `blocker_failed == 0`. `evaluate_gate` does not require predecessor gates; claiming a later gate for a formal baseline is an orchestration concern.
- **GateReport** also records `baseline_evidence_hash` and the sorted `evidence_artifact_refs` of the evidence inputs next to `baseline_semantic_hash` and `validation_artifact_refs`; it has no `evaluated_at`, timestamp, readiness score, branch or revision ID; the rulebook's sample `evaluated_at` is operational presentation metadata. Rules are ordered by rule ID, findings by Finding ID, waivers by `(rule_id, finding_ref, decision_ref)` and artifact refs by hash, all unique; `content_hash()` is the generic hash of the canonical report.
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
