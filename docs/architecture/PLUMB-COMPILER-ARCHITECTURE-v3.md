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

### Pilot semantic producer completeness (R0.6, plan v3.1)

Every semantic node family the pilot gates I0-F4 and the HR pilot depend on must have a genuine production path: user input, configuration or validated inference becomes a proposal, a governed acceptance and a compare-and-swap revision, and the material reaches `Accepted` status through a governed product operation. Test fixture constructors, golden builders and direct `Graph` construction are never production paths, and a generic "create arbitrary node" operation is not one either.

**Producer census at plan v3.1 (inspected before R0).**

| Family | Production producer | v3.1 status |
|---|---|---|
| Requirement, Goal, Need, Concern, Constraint | S1.1 `compile_requirement_candidates` (extract) and requirement split; confirmation through sentence accept | exists |
| Term, Concept | S1.5 `analyze_vocabulary` (consolidate); glossary confirm, edit, merge | exists |
| Entity, Attribute, DomainRelationship | S2.1 `analyze_domain` (analyse); Attribute type, unit and class edits | exists, emitted `Proposed` only |
| State, Transition, Invariant | S2.2 `analyze_lifecycle` (analyse); invariant formula answers | exists, emitted `Proposed` only |
| Operation, Outcome, Event | S2.7 `analyze_operations` (flows) | exists, emitted `Proposed` only |
| Process, ProcessNode | S2.9 `analyze_processes` (flows); process import and step edits | exists, emitted `Proposed` only |
| Calculation | S2.6 `analyze_calculations` (calc); calculation confirm; S3.3 answers | exists |
| Scenario | S4.1 `derive_scenarios` (scenarios); scenario confirm and correction | exists |
| SecurityRole, Principal, Permission, ResourceScope, PolicyCondition, SeparationConstraint | S2.8 `analyze_authorization` over the configured `authorization_draft` (flows) | exists, emitted `Proposed` only |
| ResolutionDecision (S3.3 answers) | `resolve_question` through the HTTP answer route; S3.1 materialization only as the Hotfix 083 application service | incomplete at the CLI product boundary |
| Agent | none (the empty initializer creates no Agent; the configured actor IDs are never materialized) | repaired by R0.1 |
| Stakeholder | none | repaired by R0.6 |
| Actor, BusinessRole | none (operation, process and authorization scopes require them `Accepted`) | repaired by R0.6 |
| Calendar | none (`CalendarConfig` feeds only the expression calendar provider) | repaired by R0.6 |
| Rule, DecisionTable | none (rule-cell fill edits an existing table only; the `rules` recipe is a projection) | repaired by R0.6 |
| Assumption | pure builder `create_assumption` without a product caller | not pilot-critical: the F3 assumption rules are not applicable without assumptions; unchanged in v3.1 |
| AcceptanceCriterion | none | not pilot-critical: F1 `CRITERIA_FOR_BEHAVIOR` is satisfied by `specified_by` links, which S2.7 produces |

**Configured semantic directory (R0.6).** Stakeholder, Actor, BusinessRole, Calendar, Rule and DecisionTable material that no compiler can extract is authored explicitly in configuration as a typed directory draft with explicit IDs. A deterministic producer turns each draft element into one `Proposed` node in its own `Semantic` / `HUMAN_CONFIRM` proposal (persisted pending, never committed by the producer). Node evidence is the sorted union of the evidence of the `Accepted` Requirements a draft element names as grounding, and is empty when it names none. A Calendar draft names a configured runtime `calendar_ref` and supplies `time_zone`, `region` and `holiday_source`; its `week_pattern` is exactly `{"weekend": <the configured weekend weekday names in configured order>}`, so the PSG Calendar and the runtime calendar provider cannot disagree. A DecisionTable draft is a typed `DecisionTableSpec` encoded only through the S2.6 `encode_decision_table`. Nothing is inferred from names or prose, and no field is defaulted.

**Governed confirmation (R0.6).** Compiler and directory output lands in the graph as `Proposed` elements when its `HUMAN_CONFIRM` proposal is accepted; promotion to `Accepted` is a separate governed confirmation, as the existing Requirement, Term, Calculation and Scenario confirmations already are. A generic confirmation operation builds one `HUMAN_CONFIRM` proposal of sequential `SetStatus Proposed -> Accepted` leaves under element preconditions for an explicitly listed set of existing elements, and it is accepted through the ordinary proposal acceptance path. It never creates or edits content, never confirms governance, provenance or proof types (Agent, Finding, Question, ResolutionDecision, Assumption, DerivationRecord, SourceArtifact, EvidenceFragment, ScenarioRun and the other excluded types), never replaces a dedicated confirmation contract (Term, Calculation, Scenario), and confirms a `Proposed` edge only when both endpoints are baseline-participating after the same proposal.

**Governed answers at the product boundary (R0.6).** The CLI reaches the S3.1 materialization service (questions from the current persisted F2 report findings plus any explicitly supplied upstream finding material, routed by an explicit routing configuration) and the S3.3 answer path through the same application services HTTP uses; the answer logic lives in an application service, never in route code.

**Supported confirmation surfaces (R0.6, plan v3.2).** Every pilot-generated Proposed semantic element that requires human confirmation is discoverable and actionable through a supported product surface (CLI or the pilot UI); no essential acceptance operation exists only as a hidden backend route or a test helper. In particular:

- Requirement joins the generic governed confirmation (`SetStatus Proposed -> Accepted`): an S1.x Requirement reaches Accepted through the generic confirmation or the existing sentence acceptance, both over the existing WorkflowProposal and governed acceptance machinery. The confirmation still refuses governance, provenance and proof types; the dedicated Term, Calculation and Scenario contracts stay dedicated.
- A CLI proposal listing (`proposals`) deterministically lists every pending WorkflowProposal of the branch, sorted by proposal ID, with its ID, its class (the positive action it requires: accept, create, link, merge or supersede), its origin (stage recipe job and stage job IDs, or intake/session origin), its base revision and whether its stored intake requires a reason. `--json` emits the same records.
- Requirement EARS review is live: Requirement detail surfaces the pending S1.2 EARS WorkflowProposal of that Requirement as `ears_suggestion` (its normalized text) with `ears_proposal_ref` (its proposal ID); the UI accepts it through the existing governed proposal acceptance of that proposal. The `accept-ears` route stays available for an analyst-supplied text edit.
- No second proposal lifecycle is introduced.

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
- required `InferenceRequest`s, each paired with its exact deterministic `InferenceRequestMaterial` (`PlannedInferenceRequest`, §28, Hotfix 066)
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

For inference (Hotfix 066), acquisition calls `provider.execute(request, material)` with the exact request and material that PLAN produced together. Every provider validates the request and the binding (material `request_hash` equals `request.id`; the context, prompt and schema bytes hash to `context_hash`, `prompt_template_hash` and `schema_hash`; the context is RFC 8785 canonical; the prompt is UTF-8; the schema compiles) before any work or network. The validated provider result is persisted (`persist_inference_bundle`), and EVALUATE receives only the acquired `ArtifactInput`s, never the material.

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

### 4.7 Same-base proposal set acceptance (R0.3, plan v3.1)

A proposal is never rebased and its original `PatchSet` and identity never change. Accepting one proposal advances the branch, so a sibling pending proposal built on the same base revision is stale for the single-proposal path. The repair is governed set acceptance, not rebasing.

**Contract.** A proposal set is accepted only as a whole and only when:

- every member is a distinct pending workflow proposal whose `base_revision_id` is the current head, and the caller names each member with the explicit action it intends (`accept`, `create`, `link`, `merge` or `supersede`), which must equal the member's own classification;
- every member passes X1.3 intake on the head, and a member whose intake requires a reason has one;
- every stage-linked member is a fresh output of a `Ready` child StageJob on that base;
- the members are mutually compatible: each original patch applies alone to the base, their element effects (the touched, added, modified and removed nodes and edges of each patch applied alone) are pairwise disjoint, and the composition applies.

The committed `PatchSet` is one `Compound` on the shared base: the original member patches in ascending proposal-ID order, each unchanged, followed by one proposal-acceptance companion per member in the same order. Each companion is exactly the single-path companion of that member (metamodel §6.3), its `patch_ref` the hash of that member's original `PatchSet`, persisted as a `Patch` artifact. One compare-and-swap creates one revision whose `decision_refs` hold every companion and every introduced decision. Each workflow proposal then moves to accepted, and each stage child records all of its accepted outputs.

**Atomicity.** All validation happens before anything is committed. A stale, non-pending, misclassified, conflicting, reason-missing or stage-stale member fails the whole set with no revision, no workflow transition and no stage transition; only content-addressed patch artifacts may remain, as on the single path. Recovery after a crash between the commit and the workflow transitions uses the existing acceptance-decision reconciliation per member.

**Surface.** The pilot product path for multi-proposal acceptance is the shared application service and the CLI. No HTTP operation is added: the UI has no stage-proposal review surface, its acceptance surfaces act on single proposals, and the OpenAPI contract stays at 74 operations. The single-proposal path, its staleness behavior and its tests are unchanged.

### 4.8 Replay-stable governed semantic identity (R0.5, plan v3.1)

Two executions with identical source semantics, accepted answers, policy and governed outcomes must end with identical `semantic_hash` values, whatever their wall-clock times, audit, artifact bytes or proposal IDs. The complete decision record is still persisted and audited; only identity and the semantic projection change.

**Material classification.** For a `ResolutionDecision`:

- governed semantic material: `question_ref`, `proposal_ref` when it is a waiver subject, `answer`, `decided_by`, `rationale`, `supersedes`, status, and the `supersedes` relation;
- operational provenance: `decided_at` (the write instant), `patch_ref` (the address of an artifact whose bytes embed base hashes and audit timestamps) and audit.

**Identity.**

- S3.3 answer decisions: `dec:<first 16 hex of SHA-256(RFC 8785 {project_id, question_ref, answer, decided_by, supersedes})>`. The answer marker already names the governed subject and value; `decided_at` and `patch_ref` are no longer identity.
- Governed waiver decisions: `dec:<first 16 hex of SHA-256(RFC 8785 {project_id, waiver_subject_ref, answer, decided_by})>`.
- Proposal-acceptance companions keep their metamodel §6.3 identity unchanged, because they are excluded from the semantic projection.

**Projection (implementation plan §6.2).**

- A baseline-participating `ResolutionDecision` contributes with `decided_at` and `patch_ref` omitted from its payload projection (absent, not `null`).
- A proposal-acceptance companion (`question_ref = None` and `answer.kind = "proposal_acceptance"`) does not contribute at all, and neither does any edge with such an endpoint.

The companion records who accepted which proposal bytes, when, with which intake reason codes and rationale. The accepted semantic effect is exactly the original patch committed in the same revision, already represented by the elements it changed. The companion never satisfies a domain rule (metamodel §6.3), and F3 already excludes it from current S3 decisions. Excluding it therefore removes only acceptance provenance from the projection.

The element hash, the persisted payload, F3 artifact binding by `patch_ref`, waiver evaluation, supersession, `decisions.jsonl` and revision `decision_refs` keep using the complete record.

**Question identity (R0.5, plan v3.2).** A Question ID identifies the semantic unresolved question, never its scheduling or routing. Today `question_id` (`plumb-functional` `question.rs`) hashes `{project_id, template_id, slots}` where `slots` also carries the mutable `priority` (severity weight times blast radius) and `stakeholder_ref` (routing). From R0.5 the identity is exactly:

```text
q:<first 16 hex of SHA-256(RFC 8785 {
    "project_id":  <project Id>,
    "template_id": <QuestionTemplate.template_id>,
    "finding_ref": <the Finding Id the question resolves>,
    "subject":     <the template's base slots without finding_ref>
})>
```

`subject` is exactly the object `QuestionTemplate::base_slots` produces minus its `finding_ref` key: `affected_refs` (sorted) plus, by the template's `Expansion`, `candidate_ref` or `target_ref`, plus by its `ChoiceSpec` either `choice_refs` or `operation_refs` and `resource_scope_refs` (each sorted). The keys `priority`, `stakeholder_ref`, blast radius, `round_ref`, `prompt`, `status` and `answer_schema` never enter the identity, and duplicate suppression uses the same `(template_id, finding_ref, subject)` key. `priority`, `stakeholder_ref` and `round_ref` remain Question payload fields; a rematerialization that only changes them updates the existing Question through one non-semantic `ReplacePayload` on the same ID (Questions are excluded from `semantic_hash`), never a second Question.

### 4.9 R0.3 ratifications (plan v3.2)

The delivered R0.3 implementation is ratified with these interpretations; none changes its code.

- **Governed versus operational timestamps.** One proposal-set acceptance has one governed acceptance timestamp, read once by `accept_proposal_set`. It is every companion `decided_at`, the `created_at` of every original Patch artifact the service writes, the committed GraphRevision `created_at` and the time of every resulting workflow Accepted resolution. Post-CAS stage-store `updated_at` values and recipe-job transition times are operational lifecycle timestamps; they may be later clock reads and are not part of the governed acceptance identity.
- **One-member set recovery.** For `N > 1` reconciliation reconstructs the complete batch and every affected child records all of its owned outputs through the set recorder. For `N = 1` the committed envelope is byte-identical to the single governed-acceptance envelope and no durable field records which service produced it, so a recovered child may be in the legacy single form (`committed_output_ref` set, `committed_output_refs` empty) or the set form (`committed_output_ref` empty, one-element `committed_output_refs`). Both denote the same accepted Proposal artifact and committed revision; no marker is added to distinguish them.
- **Pre-CAS artifacts.** Atomicity means no semantic revision, workflow transition or stage transition before the CAS. Content-addressed original Patch artifacts may be persisted after all set validation and before the CAS, exactly as on the single path, so a lost CAS can leave only idempotent orphan Patch artifacts.
- **Defensive composition guard.** The deterministic `composition_not_applicable` conflict stays even though no reachable pair of individually applicable, pairwise-disjoint current patches is known to trigger it; no artificial mutation is required to exercise it.

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

### Lexical retrieval corpus (X1.2, Hotfix 058)

Intake candidate retrieval is a deterministic, in-memory lexical index over current accepted searchable truth. Only `Accepted` nodes of six families are indexed, one document per node ID: Requirement (statement), Concept (name and definition), Rule (name, condition, action), Calculation (name, expression, result type, unit, rounding), Scenario (name plus the canonical JSON of its kind, given, when and then) and EvidenceFragment (extracted text, when present). Decision tables, terms, identifiers, references and envelope metadata are not indexed, and token-empty documents are omitted. The version-1 tokenizer lowercases with Unicode simple lowercase mapping and splits on non-alphanumeric scalars, with no normalization, stop words, stemming or synonyms. Scoring is BM25 with k1 = 1.2 and b = 0.75, the IDF `ln(1 + (N - df + 0.5)/(df + 0.5))` over the whole corpus, unique query terms and no boosts; hits are positive-score only, ordered by score descending then node ID. The index records the graph's semantic and evidence hashes as companions and its own Generic `corpus_hash` over the canonical `{version, project_id, profile_id, documents[{id, kind, text}]}` projection; neither graph hash substitutes for it. Incremental refresh diffs the current projection against stored documents by ID, kind and exact text, recomputes corpus statistics globally, is atomic, rejects another project or profile, and must equal a full rebuild. Scores are advisory candidate ranking, not duplicate, coverage, conflict or novelty decisions, which belong to intake. The index is not persisted, and embeddings are not part of the NOW scope.

### Patch-centric intake analysis (X1.3, Hotfix 059)

Intake is analysis before acceptance. It dry-runs the immutable proposal's patch set with the ordinary patch engine (`apply_patch`, then `compute_impact`), so a stale or invalid patch is a typed failure without a report, and it never commits, mutates the proposal or the base graph, calls a model or reads a clock. Searchable proposal content (added, payload-changed or newly Accepted nodes of the six lexical families, Proposed or Accepted) is queried against the Accepted base lexical index that intake builds itself, reusing the X1.2 text projection and tokenizer. Coverage is exact typed payload equality with an Accepted base node; BM25 hits of the same kind are advisory duplicate candidates (at most 20 per subject) and novelty is one minus the best same-kind Jaccard token similarity, neither ever proving duplication. Refinements and conflicts are reported only from explicit `refines` and `conflicts_with` edges the patch adds or modifies; Term nodes resolve by tokenized term and alias keys against Accepted Terms. Semantic conflicts come only from supplied gate reports evaluated over the candidate graph: intake checks their binding and coherence and copies the exact generated findings whose affected elements intersect the proposal's scope, never re-deciding a rule. A reason is required for material decisions, `HUMAN_DECISION` acceptance, exact equivalents of other nodes, declared conflicts, unwaived blocker or error findings and ambiguous terms. A pure guard requires an Accepted `ResolutionDecision` for the proposal with a non-blank rationale before such a proposal is accepted; acceptance and write paths must call it.

### Transcript session pipeline (X1.4, Hotfix 060)

A session is runtime orchestration around a caller-supplied active compiler stage, not a PSG element and not a stage of its own: its inference requests and every proposal it surfaces carry that stage. Each user or assistant turn is first persisted as canonical transcript source bytes (`SourceOriginal`, `application/vnd.plumb.transcript+json`) whose hash yields the turn, `SourceArtifact` and `ConversationTurn` `EvidenceFragment` identities; the Accepted transcript nodes are offered as a non-semantic `AUTO_NON_SEMANTIC` proposal that leaves the semantic hash unchanged, reused when already present exactly, and are applied only in memory, never committed by the session. Context is retrieved from the base graph before the current transcript: mandatory scope nodes, then open Questions in scope, then X1.2 hits in rank order, selected whole within an exact canonical byte budget with no summarization. The session prepares the inference request and its material from committed prompt and schema files and stops; provider execution and `persist_inference_bundle` stay external, so only verified persisted acquisitions are consumed. Model output is typed and checked: proposals must target the active stage and current semantic baseline, avoid `AUTO_DERIVATION` and derivation references, and cite evidence the model was shown, at least one of it the user turn or previously Accepted evidence; an asked question must be an exposed open Question. Every proposal then goes through X1.3 intake with an explicit, externally prepared validation input, since the session evaluates no gate; only after all intake succeeds is the assistant reply persisted as a transcript turn. The session returns display material and never accepts, commits or writes the graph.

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

**Typing.** Scalars are Int and Decimal: addition, subtraction and comparison promote Int to Decimal(s) and take the larger scale; multiplication gives Int, Decimal(s) or Decimal(min(28, a+b)); division always gives Decimal(28) with checked decimal division; a Decimal product or quotient whose exact value has more than 28 fractional digits is rounded to 28 fractional digits with the rust_decimal default strategy (midpoint to even), so `0.0000000000000025 * 0.0000000000001` is `0.0000000000000000000000000002` (Fable #156); remainder is `a - trunc(a / b) * b`. Integer arithmetic is checked and never promoted on overflow; decimals never use floats. Quantity and Duration add and subtract only with the identical unit, multiply by scalars and divide by scalars; there is no conversion, derived unit or cancellation. `Date ± Duration(day)` gives Date with an integral day count, `Date - Date` gives `Duration(day)`, and DateTime supports only equality and ordering. Equality takes identical or compatible types (same enum identity, same ref type compared by reference identity); ordering takes scalars, dates, date-times and same-unit quantities or durations. The exact numeric literal zero may stand for a Quantity or Duration only in comparisons and conditional branches (`remaining_days >= 0`, `if … then RequestedWorkingDays else 0`). Boolean operators require Bool, `a -> b` is `(not a) or b`, and evaluation short-circuits `and`, `or`, `->` and conditionals. `x in list` requires a list of a compatible element type, `x in []` is false, and enum membership uses lists of contextual members.

**Callables.** Only `exists`, `direct_manager`, `working_days`, `days_between`, `sum`, `min`, `max`, `count`, `any` and `all` execute; any other call name is an unknown function, `as_of` is explicitly deferred, and there is no call-by-name facility. `exists(identifier)` tests presence without Null. `direct_manager(Ref, Ref)` asks the injected predicate provider. `working_days([start, end], calendar, inclusive_end)` returns `Quantity(working_day)`: it counts the working days of the half-open period `[start, end)` when `inclusive_end` is false and of the closed period `[start, end]` when it is true (so `start = end` gives 0 or, for a working day, 1; the start counts only when it is itself a working day), a reversed or non-two-date period is an error, and half days come from ordinary multiplication (Fable #343). `days_between(start, end)` counts calendar-day boundaries (Friday to Monday is 3) and rejects reversed periods. Aggregates `sum`, `min`, `max` fail on empty lists; `count([])` is 0, `any([])` false, `all([])` true.

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

**Supplemental inputs.** Some F2 obligations need material the PSG cannot yet express. The validation context carries optional, content-hashed F2 inputs, validated against the graph like the F1 inputs: the current S2.1 unresolved-cardinality and S2.2 unresolved-trigger findings (and nothing else); explicit scope overrides for Accepted Calculations without an S2.6 origin (forbidden for those with one); one typed decision-table specification per Accepted DecisionTable whose hit policy matches the PSG (since plan v3.1, R0.4/R0.6, the specification is decoded from the PSG DecisionTable with the S2.6 `decode_decision_table`, the inverse of the only producer encoding, and an undecodable table fails the gate explicitly); and one explicit binding set per Accepted Invariant. Missing or stale inputs are context errors; rules that need inputs report an error without them, so the gate cannot pass by omission.

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

**Templates.** A closed registry maps exactly nine typed conditions to questions using the existing `QuestionKind`: unresolved relationship cardinality (Cardinality), unresolved transition trigger (PickOne over Accepted Operations and Events), missing operation performer (RoleAssignment over Accepted Actors and BusinessRoles), missing business calendar (Calendar), non-concrete permission (Permission over Accepted Operations and ResourceScopes), inexpressible invariant (FormulaConfirm), and (Hotfix 083) three generic calculation answer families: an unresolved fractional-input semantic (Number), an unresolved `working_days` inclusive-end convention (YesNo) and an unresolved calculation holiday/calendar policy (YesNo over Accepted Calendars). Each has a versioned template ID, a fixed prompt and a closed JSON answer schema whose choices are sorted Accepted IDs; the registry is ordered by template ID. Aggregate conditions that do not say what the human must decide (for example generic calculation qualification, decision-table overlap or coverage, process structure) are returned as unmapped rather than turned into unanswerable questions; a known code with such a condition is "condition not question-resolvable", an unknown code "no question template".

**Calculation answer families (Hotfix 083).** `f2.calculation.fraction.v1` (`PLUMB.F2.CALC.TYPECHECK`, condition prefix `calculation_fraction_unresolved:`, routing `formula`, answer `{decimal, unit}` with the decimal as canonical text, never a binary number), `f2.calculation.inclusive-end.v1` (`PLUMB.F2.CALC.TYPECHECK`, prefix `calculation_inclusive_end_unresolved:`, routing `formula`, answer `{inclusive_end: boolean}`) and `f2.calculation.holiday-policy.v1` (`PLUMB.F2.TIME.CALENDAR_DEFINED`, prefix `calculation_holiday_policy_unresolved:`, routing `calendar`, answer `{calendar_ref, holiday_consumes_working_day: boolean}`) are candidate templates whose condition suffix is the exact Calculation ID; the three prefixes are public constants. Their triggering `GeneratedFinding` material is explicit upstream analysis material supplied to the kernel; the generic F2 evaluator does not derive these conditions, and no validation rule is added.

**Application orchestration (Hotfix 083).** The pure kernel is reachable through one shared application service, `ApiState::materialize_questions`, which accepts the exact `QuestionGenerationInput` (current findings and routing), audits with the configured stage actor and the injected application clock, fails before committing anything when the kernel reports a structural issue (unmapped findings are reported, not an error), and commits the kernel's exact non-semantic proposal through the ordinary compare-and-swap write. When every Finding and Question already exists identically there is no proposal and nothing is written, so a repeated identical call is a no-op. No HTTP operation or CLI command is added for it.

**Priority.** Priority is the severity weight (blocker 4, error 3, warn 2, info 1) times the finding's blast radius: the number of nodes reachable from its affected elements through the §22 impact relations and directions, computed by a shared impact-closure function rather than a synthetic patch. It is stored as a canonical integer string.

**Routing.** Each template has a routing key; the configured candidates for that key (else `default`) are a preference list, and the first that is an Accepted Stakeholder receives the question. Otherwise the smallest Accepted stakeholder holding the `analyst` role receives it, and otherwise the question stays unassigned. No stakeholder is created or matched by name.

**Identity and materialization.** Duplicate suppression and the Question ID both use the template ID and the canonical slot object (including choices, priority and route), never text similarity. Missing question-driving Findings and new Questions are added as Accepted governance nodes in one non-semantic, auto-acceptable proposal that leaves `semantic_hash` unchanged. Existing nodes are reused only when their generated content matches exactly; lifecycle fields set by later steps are ignored, and any other mismatch is a conflict rather than a rewrite.

**Ratified question decisions (Hotfix 046).** A finding that already carries a governed waiver produces no question whatever its lifecycle status; an affected element of the wrong type for a per-target template is malformed input; regenerated questions that already exist are reported alongside new ones; and the slot-based duplicate suppression is verified as a contract of its own.

**Question subject (Hotfix 047).** A generated question's `context_refs` starts with its exact subject (the relationship or transition candidate, or the expanded target) followed by the finding's other affected elements in ID order, so later resolution recovers the subject without reading the prompt; question identity is unchanged.

### Question rounds (S3.2, Hotfix 046)

**Configuration and persistence.** Round size is explicit orchestration input from the caller (the HR pilot uses 15), not validation-profile metadata. There is no Round node: a round is a composition record whose content-derived ID (project, stakeholder and ordered question IDs) is written into each member question's `round_ref`.

**Eligibility.** Only Accepted, Open questions without a round are composable, and only when routed to an Accepted Stakeholder and linked to a current Finding (Accepted, Open, unwaived, with Accepted affected elements). Answered, closed or superseded questions, already assigned questions, unrouted questions and stale questions are reported, never rerouted, moved or mutated; an unknown status or a non-canonical priority is an error.

**Ordering and capacity.** Blocking comes from the linked Finding's severity. Per stakeholder, questions are ordered blocking first, then by priority, then by ID, and each stakeholder receives at most one new round per run holding the first `question_round_size` questions; the rest wait for a later run, so resolution proceeds compose, answer, compose next. Grouping by a question's single affected Entity (direct or through an attribute's unique owner) is presentation only and forms contiguous runs without reordering.

**Assignment.** One non-semantic, auto-acceptable proposal sets only `round_ref` on the selected questions under compare-and-swap preconditions; status stays Open and `semantic_hash` is unchanged. Later answering keeps `round_ref`, which never decides whether a blocking question is resolved.

### Resolution decisions (S3.3, Hotfix 047)

**Scope.** One human answer to a generated question becomes one human-decision proposal. The answer mapper is identified by the linked finding's rule and the question kind, for exactly the nine current templates (Hotfix 083 adds the three calculation families); for those three the linked Finding must also be exactly the `prefix + subject` condition, proven through its deterministic Finding ID, so a malformed question that merely reuses the code and kind gains no mapper. Broader mapper families wait for their own question producers.

**Validation.** The question must be Open (or Answered, for an explicit supersession), its finding current and its subject recoverable from `context_refs`; the actor is an Accepted human Agent; the answer is checked against the persisted answer schema and then against the current graph. Invariant answers must parse and type-check to a Boolean under explicitly supplied bindings.

**Effects.** Cardinality and transition-trigger answers have no direct effect: the Accepted decision is the governed input that the domain and lifecycle compilers already read, and only they build the relationship or the transition with its trigger. The other answers change exactly one kind of semantic fact: an operation performer is added, a calculation calendar or an invariant expression is replaced, and a permission's existing operation and scope relations are repaired in place (retargeted and, if suspect, re-accepted) without ever adding a second relation. Every initial effect first proves the original gap still exists. The three calculation families (Hotfix 083) change only an Accepted Calculation: a fractional input adds one canonical `CalculationExampleV1` to `Calculation.examples` whose key is the shared content-addressed `calculation_fraction_semantic_key(calculation, attribute)`, whose single input is the exact S2.6 `CalculationOrigin` root binding of the one unitless Decimal input Attribute named by the question context with the fraction as a unitless canonical decimal (0 < fraction <= 1, within the input scale), and whose mandatory expected result is the same fraction in the calculation's result unit; an inclusive-end answer replaces, by AST transformation and canonical unparse, only the third argument of the single `working_days` call, from the unresolved identifier `inclusive_end` to the boolean literal; a holiday-policy answer confirms the fixed PlumbExpr policy that a configured holiday is a non-working day (an answer that a holiday consumes a working day is rejected with no proposal) and binds the selected Accepted Calendar, a current identical calendar being a governed confirmation without semantic effect and a different current calendar a stale question unless superseded. No PSG field, node type or relation is added.

**Governance.** The decision records a closed answer marker, the human actor and time, and `patch_ref`, the hash of a separate content-addressed artifact holding only the semantic effect (which avoids a circular hash). Its ID derives from the project, question, marker, actor and supersession, not the rationale (since plan v3.1, R0.5, time and `patch_ref` are operational provenance and are no longer identity; see §4.8). Resolves edges link it to the question and the finding; the question becomes Answered (round kept) and the finding closes only when no other question of it stays Open. Superseding a decision builds a fresh patch from the current graph, links new to old and marks the old decision Superseded; nothing is inverted or deleted. The three historical HR expert answers (half day, holiday inside the period, inclusive end) are reachable through exactly these three generic calculation families and their governed representation above; they are applied only through the normal question answer and proposal acceptance path, never by a fixture-only adapter.

### Assumptions and governed waivers (S3.4, Hotfix 048)

**Assumptions.** An assumption is created only as an Accepted, human-governed node with a deterministic ID derived from its content (statement, owner, linked finding, default, expiry and an always-null risk reference). Its owner is an Accepted Agent or Stakeholder and the accepting actor an Accepted human. An assumption linked to a blocker finding must carry an expiry, which stands in for the review the metamodel cannot express. The metamodel has no Risk element, so `risk_ref` must be empty; a supplied reference is rejected rather than guessed. Status vocabulary is Open, Accepted, Resolved, Expired and Superseded, and only Accepted assumptions are active.

**Expiry.** Expiry analysis reads an injected clock once and reports every active assumption whose expiry has been reached (the expiry instant itself counts) as an S3 governance finding whose identity depends on the assumption and its persisted expiry, never on the current time. It is not a validation rule and never changes the assumption; lifecycle changes are separate governance actions.

**Waivers.** Waiver creation builds exactly the decision the existing waiver evaluation already recognizes: the finding identity is derived from the rule, targets and condition, the rule's waiver policy is checked before anything is proposed (forbidden and disabled profile waivers fail with no proposal), and an existing waiver of the same finding blocks a second one. The decision carries the unchanged waiver marker, a deterministic waiver-subject reference, and the hash of a separate waiver artifact; one resolves edge links it to the finding. Waiver governance has two deterministic layers: the validation crate prepares and validates this canonical material without mutation or proposal capabilities, and the functional crate wraps it in a human-decision proposal and dry-runs it through the patch engine, so the validation crate never depends on the patch engine. The finding itself is never closed or edited: the evaluator derives WAIVED from the governance material. Waiver review and expiry remain unsupported until the metamodel can express them, and answers to questions are never treated as waivers.

### Executable F3 semantics (S3.5, Hotfix 049)

**Inputs.** F3 stays pure validation. Material it cannot derive from the graph arrives as validated supplemental inputs, as for F1 and F2: the content-addressed decision artifacts (bound to each decision's `patch_ref`) and the complete set of assumption-expiry findings produced by the S3.4 analyzer at one clock instant. F3 never reads time, replays patches or reaches an artifact store, and without these inputs every F3 rule reports an error rather than a pass.

**Rules.** Blocking questions are those linked to an Accepted F2 blocker finding, and any still Open fails the gate; waivers of the finding do not change question state. A current S3 decision counts as applied when its artifact is present and the graph shows its effect: the relationship or transition the domain or lifecycle compiler builds at the candidate ID, the performer edge, the calendar, the permission bindings, the invariant expression, for a calculation fraction exactly one governed version-1 example with the marker's key, root symbol, unitless input decimal and expected decimal in the calculation's unit (with a valid origin binding), for an inclusive end the single `working_days` call whose third argument is the marker's boolean literal (parsed through PlumbExpr), for a holiday policy a `false` policy with the Accepted Calendar bound to the calculation, or for a waiver its resolves edge. Closed markers are checked by exact field set and exact JSON type; the inclusive-end and holiday-policy markers carry JSON booleans (Hotfix 083). The six F3 rule IDs are unchanged. Decisions need a human actor, their governance edge and the rationale their family requires (cardinality, trigger, waiver and supersessions), without making S3.3's optional rationale mandatory. Blocker-linked assumptions need an accepted owner and an expiry and must not appear among the supplied expiry findings; architecture-driving assumptions wait for an architecture-driver model. Blocker and error waivers must carry exact, unique, human governance; waiver policy itself remains with the generic evaluator.

**HR.** F3 acceptance uses the current governed S3 state. Since Hotfix 083 the three HR expert answers are reachable through the generic calculation answer families and, once accepted through the normal path, are proven by the calculation postconditions above.

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

### Scenario derivation (S4.1, Hotfix 050)

**Families and inputs.** S4.1 derives deterministic Scenario skeletons of three families only: decision-table rule rows, state transitions and explicit boundaries. Material the graph cannot carry arrives as explicit derivation inputs, never as PSG truth: one typed `DecisionTableSpec` per Accepted DecisionTable (the PSG table Values have no frozen encoding and are never decoded) and boundary inputs that say which semantic boundary deserves a scenario without supplying its answer. Every derived Scenario is a Proposed, human-confirmed proposal.

**Values.** Scenario values are closed literals or explicit semantic references. A literal slot is missing test data that inference may fill within an exact typed contract (enums, units, scales and inclusive or exclusive intervals); a semantic reference is missing business meaning that inference never fills and that later execution reports as undecidable. A resolved half-day fraction lives in the Accepted Calculation's `examples` (pilot encoding v1); inclusive-end semantics are the literal third `working_days` argument; `working_days` with an Accepted calendar already excludes holidays, while concrete holiday dates stay unresolved until an execution-time calendar contract exists.

**Identity and compatibility.** A Scenario ID identifies the obligation (kind, sources, row or boundary key and assertion targets), not resolved or inferred literal values, so answering a semantic dependency keeps the same ID. The operation trigger form and grouped `then` categories keep operation scenarios representable in the legacy functional.yaml projection; other triggers stay intentionally lossy there.

### Functional interpreter execution (S4.2, Hotfix 051)

**Boundary.** The functional interpreter executes one Accepted, Confirmed Scenario against a validated, immutable execution model built from one graph baseline plus explicit supplemental inputs (typed decision-table specs, scope overrides for legacy calculations and explicit expression scopes for operations, rules, transitions and invariants). Execution is pure: the only outside material is invocation context (an optional runtime actor, allowlisted runtime environment values such as a concrete holiday date, and injected calendar and predicate providers). It computes actual semantics and an ordered typed trace; comparing them with the Scenario's expectations, structured diffs and ScenarioRun evidence belong to later steps.

**Three outcomes plus errors.** Execution ends Executed, Failed (accepted semantics explicitly reject execution: authorization, preconditions, guards, source states, postconditions, invariants, missing decision rows, data-dependent evaluation failures) or Undecidable (a business-semantic, runtime or scope value is missing, named by exact owner/key identities). An invalid model, Scenario or runtime input is a typed simulation error. Missing semantics are never defaulted: accepted assumption defaults stay uninterpreted governance material, and calculation examples answer only the exact semantic references they were accepted for.

**Representable effects only.** `writes` relations are effect footprints, not assignments; the pilot changes runtime state only through state transitions, calculation results, decision-table outputs, deterministic outcomes and produced events. Rule actions, transition effect references, policy conditions, resource scope kinds and process control flow are traced or deferred, never guessed. Authorization reuses the accepted role facts and stays role-level; functional performers are never treated as security roles.

### Scenario comparison and trace material (S4.3, Hotfix 052)

**Verdicts.** Comparison consumes a completed execution of the current Accepted, Confirmed Scenario (a run whose specification hash no longer matches is stale and rejected) and never re-executes behavior. An execution that failed is a failed scenario with one diff naming the failing owner; an undecidable execution, or an expectation that is itself an unresolved semantic reference, is undecidable; otherwise the scenario passes iff its expectations hold. Expectation references resolve only through the same accepted and runtime rules as execution.

**Assertions.** An absent expectation category asserts nothing. Value maps and active states assert their named members only; rule satisfaction requires a true result; outcomes and events, when asserted, are exact sets because unexpected observable behavior matters. Equality is typed (decimals numerically, units exactly, no coercion between kinds), and accepted expectations that contradict their target types are model errors, not mismatches. Diffs are closed, typed and canonically ordered, each naming an exact semantic element.

**Evidence material.** The execution trace is preserved unchanged and followed by a typed comparison trace. Each comparison yields the existing ScenarioRun shape and a versioned scenario-trace artifact body with canonical bytes and a content hash, with no timestamp; persisting either belongs to the caller, not to the interpreter.

### Incremental scenario execution (S4.4, Hotfix 053)

**Selection.** Re-execution follows the finished impact analysis: the dirty set computed for an accepted change is intersected with each Confirmed Scenario's exact execution and proof dependencies (its own definition, traceability, anchor, Given and Then elements, decision tables, calculation closures and calendars, and the authorization, trigger and production relations that impact analysis deliberately does not propagate). Unrelated or view-only changes rerun nothing, and there is no global rerun fallback. Each selected Scenario reruns once, in identifier order, with one caller-supplied runtime context, through the unchanged execution and comparison steps.

**Execution findings.** A failed Confirmed Scenario yields a stable scenario-fail finding, and an undecidable Scenario that the caller designates as blocking yields a stable blocking-undecidable finding; identity depends only on the code, the Scenario and the condition, never on trace details. These are execution-governance findings, not validation rules. The blocking designation is a temporary caller policy until verification obligations and profiles can derive it. Findings are returned as current material and are neither persisted nor reconciled here.

### Verification obligation applicability (S4.5, Hotfix 054)

**Applicability, not planning.** Verification obligations are proposed only to let F4 distinguish executable sources from sources that are verifiable by other means. Which requirements and operations are in scope, whether each is executable through scenarios, and whether it is blocking is explicit stage input; a requirement's kind never decides it. A non-executable requirement is verified by the method already recorded in its accepted payload; when that method is absent, unknown or deferred (tests belong to later planning), the gap is reported instead of guessed.

**Linkage.** A source is linked to its obligation by `verified_by`, and a scenario obligation to every accepted, confirmed scenario that exactly covers the source (directly, or through the operation that specifies a requirement) by `implemented_as`. An executable source without any covering scenario still receives its obligation, so missing coverage becomes visible. Obligation identity is the project, the source and the verification kind only; blocking status, implementations and audit never change it. The blocking scenarios derived here are what orchestration hands to incremental re-execution.

### F4 validation inputs and applicability (S4.6, Hotfix 055)

**Evidence, not re-execution.** F4 validates the accepted graph together with explicit, content-bound supplemental material produced upstream: the resolved verification applicability (implementation-scoped sources, executable sources, in-scope confirmed Scenarios, blocking Scenarios and any Scenario kinds the profile requires), one type-check result per in-scope Scenario bound to its specification hash, at most one current Scenario run per Scenario bound to its specification and to the model baseline through its canonical trace artifact, and the assumptions the gate output is meant to surface. Validation never depends on the scenario compiler or interpreter; stale or unbound material is an invalid context, while missing or failing proof is a rule violation.

**Applicability-aware verification.** Every implementation-scoped source needs an accepted verification obligation of some kind; only executable sources need confirmed Scenario coverage. Scenarios are grounded by their derivation sources and anchors, typed by the content-bound S4 check, and a blocking Scenario is acceptable only when its current run passed, since comparison already accounts for expected business failures. Traces must be ordered, bound to their run and carry business-semantic evidence, or the missing semantics of an undecidable run. Active assumptions whose findings touch the covered sources, obligations or Scenarios must be surfaced exactly through the rule results.

### Functional pilot projections and reports (S4.7, Hotfix 056)

**Six artifacts, one baseline.** The functional pilot exports functional.yaml, glossary.md, scenarios.yaml, decisions.jsonl, model-report.md and gate-report.json as returned material for one accepted graph baseline, each accompanied by the source semantic hash and the content hash of its exact bytes (never embedded in its own bytes). functional.yaml is the unchanged compatibility projection; scenarios, decisions and the glossary project accepted semantic truth directly in identifier order; the gate report is the canonical form of an already evaluated F4 report bound to the same semantic, evidence, profile and rule-pack identity.

**Reports never decide.** A failed gate is exported as failed. The model report summarizes the baseline, the supplied gate, accepted counts, compatibility notices, scenarios and decisions; readiness is stated to be advisory and is not calculated, so it cannot change any gate result. Persisting or delivering the artifacts belongs to the caller, and the real compiled HR reference graph and its reference projections belong to the end-to-end fixture work.

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

### Live provider request material (X1.1, Hotfix 057)

Hotfix 066 supersedes the provisioning model of this paragraph; see the following section. As frozen by Hotfix 057: `InferenceProvider::execute` still takes only an `InferenceRequest` and returns a `ProviderExecution` (the inference artifact plus the exact raw response bytes and media type). Because the request carries only content hashes, live providers are provisioned with immutable, hash-bound request material supplied in memory by orchestration: the canonical context bytes, the exact prompt-template bytes and the exact schema-source bytes, each verified against the request before any network call. The provider has no graph, artifact-store or filesystem acquisition capability and reads no environment. Vendor adapters validate output locally against the exact Plumb schema and never translate it into a vendor-specific constrained-output subset unless native equivalence has been separately proven; unparseable, truncated, refused or schema-invalid output is a typed failure, never repaired, and retries belong to orchestration.

### Material-bound inference acquisition (Hotfix 066)

`InferenceProvider::execute(&self, request: &InferenceRequest, material: &InferenceRequestMaterial)` receives the request and its exact material on every call. `InferenceRequest` identifies an inference request and remains the only request identity; `InferenceRequestMaterial { request_hash, context_json, prompt_template, schema_json }` contains the exact deterministic bytes needed to execute it. Material is not accepted PSG state, cannot mutate the graph and is not a second request identity: its `request_hash` is the request ID. `InferenceRequestMaterial::new(&request, context_json, prompt_template, schema_json)` returns only material whose binding validates, and its serde form is strict (unknown fields rejected). Every provider validates the request and the binding before any work: `NullProvider` then returns `ProviderDisabled`; `MockProvider` replays fixtures keyed by request ID; `AnthropicProvider` owns only its connection configuration (`new(api_key)`, `with_endpoint(api_key, endpoint)`), holds no material registry or global state, and one instance executes any number of distinct requests with their own material, rejecting a mismatch before any HTTP. `ProviderExecution` has a strict serde form that rejects unknown fields and revalidates on decode, including the raw-response hash, so recorded executions replay exactly.

Exactly 11 production request owners build inference requests for compiler stages and each exposes a pure `inference_material()`: `SegmentationRequest`, `RequirementClassificationRequest`, `VocabularyRequest`, `EarsRequest`, `DomainRequest`, `LifecycleRequest`, `DataClassRequest`, `CalculationRequest`, `OperationRequest`, `ProcessRequest` and `ScenarioRequest`. The material is the RFC 8785 canonical JSON of the stored context and the owning module's committed prompt and schema bytes, built through `InferenceRequestMaterial::new`; it reads no file, environment, working directory, clock or network at run time, and request, prompt, schema and context hashes are unchanged. For segmentation, `build_segmentation_request` produces the `SegmentationRequest`, `inference_material()` its material, `provider.execute(&request.request, &material)` the execution, and `evaluate_segmentation` consumes only the acquired artifact. The X1.4 session `SessionTurnPlan` carries its request and material together (built with `InferenceRequestMaterial::new`), and the A0.3 session-turn route executes exactly that pair from the same plan, off the async executor and outside every store lock; this repairs A0.3, where the provider previously received only the request.

---

### Provider optionality and declared fallback (R0.2, plan v3.1)

Provider behavior must agree with each stage's declared contract. It does not make the compiler work offline.

**Census.**

| Recipe phase | Task kind | Absence policy | Contract evidence |
|---|---|---|---|
| extract: segmentation | `requirement_segmentation` | fallback | `evaluate_segmentation` builds deterministic fallback segments and records the fallback mode |
| scenarios | `scenario_values` | fallback (partial) | `derive_scenarios` proposes slot-free skeletons deterministically; slot skeletons stay unresolved |
| tags | `data_classification` | fallback (partial) | `analyze_data_classification` always returns its deterministic dictionary proposals |
| extract: classification | `requirement_classification` | required | no proposals without classification |
| consolidate: vocabulary | `vocabulary_analysis` | required | "there is no fallback" |
| consolidate: EARS | `ears_normalization` | required | absent inference yields only unavailability issues |
| calc | `calculation_analysis` | required | absent inference yields only unavailability issues |
| analyse: domain | `domain_extraction` | required | "there is no fallback" |
| analyse: lifecycle | `lifecycle_extraction` | required | inference-only |
| flows: operation | `operation_analysis` | required | absent inference yields only unavailability issues |
| flows: process | `process_analysis` | required | absent inference yields only unavailability issues |
| session turn (HTTP) | `session_turn` | required | the turn fails with `E_INFERENCE_FAILED` (`ProviderDisabled`) |

`consolidate: duplicates`, `flows: authorization`, `lint`, `rules` and `run` plan no inference.

**Contract.**

- Every planned inference request declares its absence policy in the stage plan.
- Under the `null` provider mode:
  - A recipe phase whose plan contains a `required` request fails before any provider call with `E_INFERENCE_REQUIRED`, naming the phase and task kinds. Nothing is persisted except the failed job records.
  - A `fallback` request is recorded as declared absent (no artifact, no fabricated output). The phase then evaluates its consumer with no inference, exactly as the consumer's contract defines.
- Under `replay` and `anthropic` every planned request is executed exactly as before. A missing replay fixture or any provider error still fails the recipe with `E_INFERENCE_FAILED`, so exact replay is never weakened into absence.
- A `CompileRun` records the sorted IDs of its declared-absent requests. They are serialized only when non-empty, so every existing run identity is unchanged. Plan validation accepts a missing artifact only for a declared-absent `fallback` request.
- Request identity, material binding, artifact validation, provider-mode hashing and replay semantics are unchanged.

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

### Asynchronous stage job revision protection (X1.5, Hotfix 061)

A job also captures the branch whose head it observed, and its identity is the hash of its immutable capture, so capturing again at another revision is another job. Its status moves deterministically through queued, acquiring or evaluating to ready, stale, failed or committed; stale, failed and committed are terminal. Completion binds the existing `CompileRun` (stage, input revision, semantic hash, profile and config) and the `StagePlan` that carries the scope. Freshness is decided by revision-ID equality with the observed head alone: an evidence-only revision leaves the semantic hash unchanged but is a new revision, so the earlier job is stale. A ready job becomes stale when a later head is observed, a stale job is never rebased, and re-evaluation captures a new job at the current head. Commit guards admit only an exact canonical output of the bound run while the head is still the base revision; they prove freshness, not intake or authorization. The store commit with `expected_head` equal to the base revision remains the final race authority, and `committed` records that one selected output advanced the branch, not that every proposal of the run was accepted.

### Pilot HTTP API compatibility contract (A0.1, Hotfix 062)

The pilot HTTP API stays a task-oriented compatibility projection over the PSG. Its contract is generated from Rust before any handler exists: 72 operations, namely the v2 P and P-R operations with their pilot restrictions plus proposal acceptance, carrying v3 overrides for model version and hashes, NOW gates, job staleness and base revision. Pagination and `ETag` are pulled forward from the read handlers, every POST requires `If-Match` with the current semantic hash and documents `412` with the current revision and hash, and the WebSocket event stream appears as a documented upgrade operation. Examples come deterministically from fixed HR fixture seeds or a schema fallback and must validate against their schemas. The committed, generated OpenAPI document is the source of the UI client; runtime handlers come later.

### Pilot core HTTP runtime contract (A0.2, Hotfix 063)

Contract version 2 corrects two pilot wire decisions and splits runtime ownership. Pagination cursors are bound to the branch-head revision as well as the semantic hash, because the semantic hash is not a revision identity: evidence-only revisions (source, evidence, finding, question and run material is excluded from the semantic hash) change what a page shows while keeping it. Cursors are also bound to their request (operation, path parameters and present non-pagination filters), so they cannot be replayed against another collection or filter; a moved baseline is a conflict, a foreign or malformed cursor a bad request, and nothing is rebased. Readiness and its five sub-scores are nullable, `null` meaning not calculated by the pilot runtime rather than zero, and readiness never drives a gate. Accepting an EARS normalization requires the exact reviewed text, because suggestions are not graph state.

A0.2 implements 30 of the 72 operations: meta, gates, events, source import, requirement reads with classification and EARS acceptance, glossary confirm and edit, entity reads and attribute edits, calculation list and confirm, process reads, finding, assumption and decision reads, readiness, exports and revision diff. The other 42 (workflow, sessions, proposals, scenarios, stage jobs, sentences, search, requirement split and duplicates, waivers, Term merge, CRUD, calculation examples, rules, process editing and Mermaid, assumption resolution) belonged to A0.3, which could start only after a separate contract freeze resolved their semantics; that freeze is Hotfix 065 below, which also moved the stage/job operations to A0.4.

The API state owns one explicit branch, the revision and artifact stores, an injected identity and provenance actor, an injected clock, the event broadcaster, orchestration-registered gate reports and the validation profile; there is no global graph. A read answers from one loaded revision with its semantic hash as `ETag`. A write checks `If-Match` against the loaded head, builds a typed proposal, runs the existing intake and rationale guard and commits with the loaded revision as expected head; a moved head is `412` without retry, and each commit emits exactly one event naming the operation, the changed element IDs and the new revision. Response fields come only from the PSG, typed relations, existing projections, or gate reports that match the current head; what the pilot cannot compute is null or a typed conflict, never a placeholder.

### Pilot A0.3 compatibility semantics (contract version 3, Hotfix 065)

Hotfix 065 is the contract freeze that the A0.2 section required before A0.3. It resolves every deferred compatibility topic, moves stage orchestration out of A0.3 and publishes contract version 3. A0.3 owns 40 operations (27 POST, 13 GET); `run_stage` and `get_job` keep their paths and DTOs but move to A0.4, because no concrete `CompilerStage` implementation and no application stage runner exist at this baseline. After A0.3, 70 operations are routed and the two stage/job operations stay unregistered until A0.4.

**Runtime and identity.** The A0.2 constructor stays source compatible and builds a core state without A0.3 runtime material; an additive full-runtime constructor injects one typed A0.3 runtime: the durable workflow store, the inference provider, provider policy, session turn configuration, assistant actor, calendar and predicate providers, expression scopes (unique by owner) and the simulator's calculation scope overrides and runtime semantic values. Nothing is global, environment-selected or faked; an endpoint whose runtime material is absent answers a typed `503`. The same injected providers serve calculation examples, scenario runs and session turns, and the same expression scopes serve scenario execution and invariant answer resolution. Every deterministic identity uses the loaded graph's project ID and is computed by the owning domain module, never re-implemented in a route.

**Writes and governed acceptance.** Every graph write keeps the A0.2 pipeline (If-Match, one head load, one clock read, typed proposal, X1.3 intake against the exact head, the unchanged rationale guard, CAS on the loaded revision, `412` without retry, one event per commit). A governed acceptance (each material human decision of this contract, each explicit proposal action and each write whose intake requires a reason) adds a separate proposal-acceptance `ResolutionDecision` (metamodel §6.3) in the same atomic patch as the unchanged original proposal effect: the original `PatchSet` is persisted as a `Patch` artifact whose hash is the decision's `patch_ref`, the combined patch is dry-run on the same base before the CAS, and the revision's `decision_refs` list exactly the Accepted decisions the action introduced. The X1.3 guard is never weakened or special-cased: a waiver keeps its Hotfix 048 decision (`proposal_ref = waiver-subject:..`, resolving its Finding) and the companion decision separately records the acceptance and its rationale. The corrected `resolves` contract (no generic minimum; a question-bound decision must resolve its Question) makes proposal-only decisions valid.

**Search.** Search ranks the existing lexical index of one loaded revision by BM25 score and ID, keeps that ranking through pagination, reports the retrieval kind and score, gives a detail path only where an exact route exists (requirements and rules; otherwise null) and returns as snippet the exact first 240 Unicode scalar values of the indexed text, read through the index rather than recomputed.

**Requirements and vocabulary.** A split is an explicit human decision that adds Accepted children (the parent payload with each supplied statement, parent provenance without the segmentation origin, a split-origin extension and `decomposes_to` edges), with identities derived from parent and index rather than text, and supersedes the parent. Marking a duplicate is canonical supersession of the duplicate by the named replacement, never a node merge. A Term merge requires equal language and definition (a difference is a typed conflict, never a choice), merges with the existing keep-payload policy and then replaces the target aliases with the exact union of both vocabularies under the target's real intermediate element hash.

**Sentence and CRUD views.** A Sentence is a view of exactly one Requirement: its statement, status, evidence and typed slots (requirement kind, level and modality), with unknowns taken only from Accepted open Questions whose context names it; entity membership follows only active `specified_by` and `reads`/`writes` relations. Sentence accept, reject and slot edits are ordinary human-confirm writes. The CRUD view reports reads exactly; because the PSG does not distinguish create, update and delete, a written pair reports them as null rather than inventing them.

**Calculation examples.** Inputs and results use an exact, non-recursive tagged value form (no binary float for decimals). An example is evaluated only for an Accepted Calculation with a replayable S2.6 scope (an unavailable scope is a typed conflict, never guessed), through the existing qualification, evaluator, injected providers and example renderer; value conversion lives in the functional calculation module, and references or lists are typed as unrepresentable.

**Decision tables and rules.** The PSG `DecisionTable` stores its typed columns, rows and cells as their serde values with no default output; one strict adapter decodes and encodes them, and malformed material is a typed error, never skipped. A scalar Rule projects only its condition; a decision table projects canonical column and cell texts and the existing structural, overlap and coverage analysis, with null wherever the analysis does not apply. Editing one output cell is a governed human decision that rewrites only that cell through the adapter.

**Processes and Mermaid.** Step editing is defined only for a strict linear Accepted Process (one start, one end, single-successor interior steps, no gateways, cycles or boundary-crossing flow). Adding creates an Accepted service-task node with a deterministic identity, moving only rewires flow, and removing is refused while the node has non-flow relations; the result must pass the existing process analysis. Mermaid interchange is one canonical, byte-roundtripping representation of exactly that linear subset, using the existing deterministic process and node identities; anything else is unrepresentable or invalid, and an import never overwrites an existing process.

**Assumptions.** Resolving an assumption is a governed human decision that changes only its payload status from `Accepted` to `Resolved`; nothing else (findings, defaults, expiry, risks, owners, node status) changes.

**Waivers.** The material of a waiver comes from the current, head-bound gate report that generated the finding (its semantic condition, rule, targets and policy) and is checked against the persisted Finding before the existing waiver service runs; stale or incoherent material is a typed conflict, and a requirement-scoped waiver additionally requires the finding to affect that requirement.

**Durable workflow state.** Pending proposals, sessions and session turns are addressable and must survive a restart, so they live in the existing SQLite store (store schema version 2, migrated transactionally from version 1) as canonical opaque JSON; the store owns identity, state transitions, canonical bytes, lookup and durability, and the API layer owns typed decoding. Workflow tables never affect PSG hashes or revisions. A stored proposal is bound to the exact revision it was built on: an action on another revision is a conflict, never a rebase, and the stored intake is history while acceptance re-runs intake on the current head. Proposal actions (link, merge, supersede, create, accept) never rewrite a proposal; they differ only by the action class derived from its patch. A crash between graph commit and workflow finalization is reconciled from the proposal-acceptance decision in the graph.

**Questions, rounds and answers.** The Question API projects only persisted Accepted Questions (kind, prompt, status, answer schema, stakeholder, canonical priority, round and context references); generation-time slots are not persisted and are not part of the contract. A round targets one stakeholder through the existing S3.2 composition kernel and is a projection over `round_ref`, reconstructed and verified against its deterministic identity. Answering a Question builds the existing S3.3 resolution proposal and persists it as a pending workflow proposal without committing; acceptance happens through the proposal endpoints.

**Sessions.** A session is durable workflow state owned by its creator. A turn runs the existing X1.4 pipeline once on one baseline, calls the injected provider outside every store lock, records a prepared turn and its generated proposals, and commits the user and assistant transcript material in one non-semantic CAS; the generated proposals become pending only after that commit, bound to the post-transcript revision. A lost race marks the turn and its proposals failed, and an interrupted turn is reconciled from the presence of its exact transcript nodes. Metrics count committed turns only.

**Scenarios.** The Scenario view exposes the canonical JSON of the stored given, when and then values, the node status and the separate confirmation status, and every derivation source. Confirmation moves a Proposed, Derived Scenario to Accepted and Confirmed in one proposal with sequential element preconditions; bulk confirmation only confirms already-derived Scenarios of one decision table and never derives; a Then correction changes only Then and preserves the lifecycle. Only Accepted, Confirmed Scenarios run, matching simulator admission.

**Scenario runs and trace links.** The `ScenarioRun` node of a Scenario is its stable latest-run projection (one deterministic ID per Scenario); the `ScenarioTrace` artifact is the immutable full trace with its typed diffs, scenario identity, specification hash, model semantic hash and result; and the node extension `plumb_sim:scenario_trace_artifact` is the durable content-addressed link from the current run to that artifact, because the artifact store has no other lookup. The artifact is persisted before the CAS. A first run adds the node with its link; a re-run changes only what differs, and when both the payload and the link change they change atomically in one patch (`ReplacePayload` followed by `SetNodeExtension` under the intermediate element hash), so a committed graph never pairs a new payload with an old link; an identical re-run writes nothing. Reading a run validates the link and the linked artifact (existence, kind, canonical bytes and agreement with the node) and takes Fail diffs only from that artifact; an invalid or missing link is a typed conflict, never a fabricated or null diff.

### Stage orchestration ownership (A0.4, Hotfix 065)

`POST /api/stages/{stage}/run` and `GET /api/jobs/{id}` belong to A0.4 together with the CLI. A0.4 must not start before a separate stage-orchestration contract freeze defines the stage-name mapping to concrete application services, concrete stage planning and evaluation, provider acquisition, inference artifact persistence, job persistence and lookup, progress transitions, scope, proposal production, the automatic and human acceptance boundaries, gate report production, the final CAS, X1.5 stale-result protection, the run and job response semantics and the failure and retry policy. Hotfix 065 freezes only their unchanged wire DTOs.

Hotfix 066 repaired only the inference material-acquisition boundary that such orchestration will use (PLAN produces request and material, ACQUIRE calls `provider.execute(request, material)`, the provider validates the binding, the validated result is persisted, EVALUATE receives acquired artifacts only). It froze no orchestration semantics; that freeze is Hotfix 067 (next section), against which the still-pending A0.4 is implemented.

### A0.4 stage orchestration contract (Hotfix 067)

Hotfix 067 is the mandatory pre-A0.4 freeze required by the two sections above. It is normative for A0.4; A0.4 implements it and invents nothing. It changes no StageId, StagePlan semantics, CompileRun identity, StageJob identity or one-run binding, InferenceRequest identity, Proposal or PatchSet identity, ArtifactKind, PSG node or relation vocabulary, SemanticPatch vocabulary, OpenAPI path or DTO (contract v3 stays byte-identical; A0.4 routes exactly `run_stage` and `get_job`, reaching 72 of 72 routed operations). The only compiler-side additions are one identity-neutral X1.5 recording method and one identity-preserving inference provenance helper, both frozen below. A0.4 needs no new crate, no Cargo manifest change and no new third-party dependency.

#### Feasibility findings this freeze is built on

- **Multi-pass extraction.** Segmentation inference, `evaluate_segmentation`, `SegmentCandidate[]`, `build_requirement_classification_request`, classification inference and `compile_requirement_candidates` form a chain whose second request cannot be planned before the first output exists. `SegmentCandidate` is runtime intermediate material, not PSG state, and one compiler `StageJob` binds exactly one `CompileRun`. No SegmentCandidate node, no new `StageName` and no multi-run StageJob is introduced.
- **Inference provenance.** Inference-backed domain analysers require a `DerivationRef`. `materialize_derivation_record` needs the full `ProviderExecution` (raw bytes are used only to validate it), while EVALUATE receives only validated-inference `ArtifactInput`s. Orchestration never reproduces the DerivationRecord identity algorithm.
- **Policy-sensitive acceptance.** `propose_scenario_run_updates` returns one `NonSemantic` / `AUTO_NON_SEMANTIC` proposal that A0.3 commits immediately, and `analyze_data_classification` legitimately returns 0..N `AUTO_DERIVATION` dictionary proposals together with 0..N `HUMAN_CONFIRM` inference proposals. A blanket "never accept" or "fail on multiplicity" rule is therefore wrong; the policy below is per output set.
- **Governed acceptance.** Every A0.3 proposal action commits `build_acceptance_companion`'s `companion.patch_set` = `Compound [proposal.patch_set.patch, AddNode(Accepted ResolutionDecision {question_ref: None, proposal_ref: Some(proposal.id), patch_ref: hash(proposal.patch_set), supersedes: None})]`, so the revision's `accepted_patch_ref` is the companion hash and the existing `record_proposal_commit` (which expects the original PatchSet hash) cannot record it.
- **Typed inputs.** Every explicit input type below already has a strict serde form (`deny_unknown_fields`) and is embedded unchanged in the A0.4 configuration; no field is re-modelled as loose JSON.

#### Two job levels

- **`StageRecipeJob`** (application layer, `plumb-api`) is the public job: `run_stage` creates or reuses it and `get_job` reads it. It executes one public `StageName` compatibility recipe as an ordered list of phases, each phase optionally owning one compiler child. It owns the recipe, the initial base revision, the requested scope, the deterministic recipe configuration, the ordered step records, the overall status, the overall failure and the result revision.
- **`StageJob`** (compiler X1.5, unchanged) remains one captured compiler stage: one base revision, one `StageId`, one scope, one config hash, one `CompileRun`. Its identity, fields, transition table and `compile_run_ref: Option<Hash>` are unchanged; no second CompileRun may complete it. All semantic commit protection keeps using child StageJobs.

`StageName -> recipe -> one or more phases -> zero or more child StageJobs`. StageName values never become StageIds; compiler ownership stays S0->I0, S1->F1, S2->F2, S3->F3, S4->F4.

**Identity.** `StageRecipeJob.id` is the generic SHA-256 of the RFC 8785 JSON of exactly `{"version": 1, "branch", "initial_base_revision", "initial_base_semantic_hash", "recipe", "scope", "profile_hash", "config_hash"}`, where `recipe` is the lowercase `StageName` wire value, `scope` is `null` or the canonical sorted ID array, `profile_hash` is the base revision's and `config_hash` is the generic SHA-256 of the RFC 8785 recipe configuration projection below. It is computed once in the application service, never from child IDs, time or randomness, and is golden-tested. The same branch, base, recipe, scope, profile and deterministic configuration always give the same ID.

**Child identity.** Every child uses the existing `capture_stage_job` unchanged. Its `CompileContext.config` (hence the StageJob and CompileRun `config_hash`) is the RFC 8785 object `{"version": 1, "recipe", "phase", "audit": {"actor_ref", "created_at"}, "provider", "inputs"}`: `audit.actor_ref` is the configured `stage_actor_ref`, `audit.created_at` is the child input revision's `created_at` (deterministic replay; no wall clock), `provider` is the parent projection's provider object, and `inputs` is the phase's typed inputs including any intermediate Projection ref and value. This keeps `lint`, `extract` and `consolidate` (all S1) and the two phases of one recipe from colliding without touching StageJob identity.

**Recipe configuration projection.** `{"version": 1, "recipe", "provider", "stage_actor_ref", "inputs"}`. `provider` is `{"mode": "null"|"replay"|"anthropic", "policy": ProviderPolicy, "replay_fixtures_hash": Hash|null}` for recipes with an inference phase (extract, consolidate, calc, analyse, flows, scenarios, tags) and `null` otherwise; `replay_fixtures_hash` is the generic SHA-256 of the RFC 8785 JSON of the validated replay execution list sorted by request hash. `inputs` holds exactly the recipe-relevant configuration values listed in the recipe matrix, each as its existing typed value (`null` when an optional value is absent). Excluded: API key bytes, file path spellings, process or thread IDs, wall clock, working directory, socket addresses, logging and transport state.

#### Recipe and phase matrix

Scope column: "requirements" means Requirement IDs whose status is Proposed or Accepted (the builders' own rule); `None` means all such IDs sorted. "whole" means only `None` is accepted. An empty effective target set completes the recipe Ready with no step.

| StageName | Phase (ordinal) | StageId | Child | Inference | Intermediate | Output | Acceptance | Gate | Next base |
|---|---|---|---|---|---|---|---|---|---|
| `intake` | `i0` (0) | S0 | no | no | no | GateReport | none | I0 | - |
| `lint` | `f1` (0) | S1 | no | no | no | GateReport | none | F1 | - |
| `extract` | `segmentation` (0) | S1 | yes | `SegmentationRequest` | segmentation Projection | empty StageEvaluation | none | - | same base (head must not move) |
| `extract` | `classification` (1) | S1 | yes | `RequirementClassificationRequest` | - | proposals | pending (all HUMAN) | - | - |
| `consolidate` | `consolidate` (0) | S1 | yes | `VocabularyRequest`, `EarsRequest` | - | proposals | pending (all HUMAN) | - | - |
| `calc` | `calc` (0) | S2 | yes | `CalculationRequest` | - | proposals | pending (all HUMAN) | - | - |
| `rules` | `rules` (0) | S2 | no | no | rules Projection | analysis | none | - | - |
| `analyse` | `analyse` (0) | S2 | yes | `DomainRequest`, `LifecycleRequest` | - | proposals | pending (all HUMAN) | - | same base |
| `analyse` | `f2` (1, only when `gates.f2` is configured) | S2 | no | no | no | GateReport | none | F2 | - |
| `flows` | `flows` (0) | S2 | yes | `OperationRequest`, `ProcessRequest` | - | proposals | pending (all HUMAN) | - | - |
| `scenarios` | `scenarios` (0) | S4 | yes | one `ScenarioRequest` per skeleton with value slots | - | proposals | pending (all HUMAN) | - | - |
| `tags` | `tags` (0) | S2 | yes | `DataClassRequest` | - | proposals | Cases A-D | - | - |
| `run` | `execute` (0) | S4 | yes | no | ScenarioTrace artifacts | one AUTO_NON_SEMANTIC proposal or none | auto-commit (Case B) | - | committed revision, else same base |
| `run` | `verification` (1) | S4 | yes | no | - | proposals | pending (all HUMAN) | - | same as its input |
| `run` | `f4` (2) | S4 | no | no | no | GateReport | none | F4 | - |

Scope per recipe: `intake`, `lint`, `analyse`, `scenarios`, `tags`: whole. `extract`: EvidenceFragment IDs that are baseline-participating, carry `plumb_import:fragment` and have non-empty `extracted_text` (`None` = all such). `consolidate`, `calc`, `flows`: requirements. `rules`: Accepted DecisionTable IDs (`None` = all). `run`: Accepted, Confirmed Scenario IDs (`None` = all; the A0.3 runnable rule). The child compiler `Scope` is `Scope::Project` for `None` and `Scope::elements(ids)` otherwise.

Recipe-relevant configuration inputs: `intake` {validation_policy}; `lint` {validation_policy, f1}; `extract` {}; `consolidate` {vocabulary_policy, duplicate_policy}; `calc` {calculation_scope}; `rules` {}; `analyse` {validation_policy, f2}; `flows` {operation_scope, process_scope, authorization_draft}; `scenarios` {scenario_derivation}; `tags` {}; `run` {validation_policy, verification_applicability, f4, calendars, direct_managers, expression_scopes, calculation_scope_overrides, runtime_semantic_values}. Required (non-null) at capture: `lint` f1; `calc` calculation_scope; `flows` operation_scope and process_scope; `scenarios` scenario_derivation; `run` verification_applicability and f4. A missing required value is rejected synchronously (`E_STAGE_CONFIG_MISSING`), never discovered mid-run.

Exact helpers per phase (planning, then evaluation; every inference phase builds its requests with the domain builder, takes material from `inference_material()` (Hotfix 066) and evaluates with the domain analyser; the per-phase audit is `{created_by: stage_actor_ref, created_at: child input revision created_at}`):

- `i0`: `ValidationContext::new` with evidence artifacts = the `EvidenceManifest` artifact of `plumb_import::build_evidence_manifest(graph)` via `evidence_manifest_artifact`, plus every stored `SourceOriginal` (`content_hash`) and `SourceExtracted` (`plumb_import:source_artifacts.extracted_ref`) artifact of each baseline SourceArtifact node (a missing artifact is simply absent, so I0 reports it); then `EvaluatorRegistry::evaluate_gate(I0, ...)`.
- `f1`, `f2`: `ValidationContext::new(..)` then `with_f1_inputs` / `with_f2_inputs` with the configured value, then `evaluate_gate`.
- `segmentation`: `build_segmentation_request(fragments, policy)`; `evaluate_segmentation(request, fragments, Some(artifact), audit)`.
- `classification`: `build_requirement_classification_request(graph, candidates, policy)`; `compile_requirement_candidates(graph, candidates, request, Some(inference), audit)`. If segmentation produced no `requirement_candidate`, no classification step is created.
- `consolidate`: `build_vocabulary_request(graph, refs, vocabulary_policy, policy)`, `build_ears_request(graph, refs, policy)`; `analyze_vocabulary`, `evaluate_ears_normalization`, `analyze_requirement_duplicates(graph, duplicate_policy)`; proposals are the union of the vocabulary and EARS proposals and the `proposal` of every `DuplicateResolutionProposal`.
- `calc`: `build_calculation_request(graph, refs, calculation_scope, policy)`; `analyze_calculations`.
- `rules`: for each selected Accepted DecisionTable, `decode_decision_table` (strict; malformed encoding fails the recipe) then `plumb_validation::analyze_decision_table(spec)`; the Projection is `{"version": 1, "tables": [{"decision_table_ref", "analysis"}]}` sorted by ref. No Rule or DecisionTable producer, no LLM, no fabricated cell answer.
- `analyse`: `build_domain_request`, `build_lifecycle_request`; `analyze_domain`, `analyze_lifecycle`.
- `flows`: `build_operation_request(graph, refs, operation_scope, policy)`, `build_process_request(graph, refs, process_scope, policy)`; `analyze_operations`, `analyze_processes`; when `authorization_draft` is configured, `analyze_authorization(graph, draft, audit)` (deterministic compilation of supplied structured semantics; never inferred from operation or process output).
- `scenarios`: `analyze_scenarios(graph, scenario_derivation)`; `build_scenario_request` for each skeleton that has value slots; `derive_scenarios(graph, scenario_derivation, audit, inferences)`. Decision-table inputs come only from the configured `ScenarioDerivationInputs`; boundaries are never derived from text.
- `tags`: `build_data_class_request(graph, policy)`; `analyze_data_classification(graph, request, Some(inference))`. No new algorithm, no merged Compound proposal.
- `execute`: the A0.3 scenario execution (`build_execution_model`, `run`, `compare_run`, the `plumb_sim:scenario_trace_artifact` link) moved unchanged into one shared pure function used by both `run_scenarios` and this phase; `propose_scenario_run_updates(graph, updates, stage_actor_ref, audit.created_at)`.
- `verification`: `compile_verification_obligations(graph, verification_applicability, audit)`.
- `f4`: `ValidationContext::new(..).with_f4_inputs(graph, F4ValidationInputs)` built as frozen below, then `evaluate_gate(F4, ...)`.

A plan or evaluation error of a domain helper fails the recipe (`E_STAGE_PLAN_FAILED` / `E_STAGE_EVALUATION_FAILED`, details carry the phase and the typed error name). Only `extract` and `run` are multi-phase with dependent children; all other children plan purely from the graph and configuration.

#### Extract recipe

**Phase `segmentation` (S1).** Builds and acquires the existing `SegmentationRequest` with Hotfix 066 request/material, evaluates with `evaluate_segmentation`, and produces an empty `StageEvaluation { derivation_patch_set: None, proposals: [] }`; the `CompileRun` still records its exact inference input. The application persists the segmentation intermediate as an `ArtifactKind::Projection`, `application/json` artifact whose bytes are the RFC 8785 JSON of exactly `{"version": 1, "candidates": [SegmentCandidate...], "mode": "inference"|"fallback", "fallback_reason": "<FallbackReason wire value>"|null, "fallback_derivation": <Node>|null}` (candidates in result order; the existing serde forms of `SegmentCandidate`, the closed vocabularies and `Node`). The DTO is application-owned and strict; it adds no semantic content. The fallback DerivationRecord node stays detached provenance inside the intermediate and is never committed or turned into a patch. The step row links the intermediate's hash; the segmentation child is completed and stays Ready (it has no output to commit).

**Phase `classification` (S1).** Before capturing, the application loads the current head; if it is not the recipe's initial base the recipe becomes Stale (no classification, no rebase of candidates, no replay). Otherwise it loads the Projection by hash, requires kind, media type, canonical bytes and a strict decode, and places `{"segmentation_ref": <hash>, "segmentation": <the decoded object>}` in the child `inputs`, so the pure PLAN builds the classification request from configuration, not from a store. `compile_requirement_candidates` yields the existing `Semantic` / `HUMAN_CONFIRM` proposals, persisted pending for human review.

#### Inference acquisition and provenance

Per inference child, for each `PlannedInferenceRequest` in plan order: call `provider.execute(&planned.request, &planned.material)` outside every store lock and off the async executor; persist with `persist_inference_bundle` (operational `created_at`); give EVALUATE the validated-inference `ArtifactInput` only. Deterministic planned artifacts are stored by content hash. A non-empty `external_validation_requests` list fails the recipe with `E_EXTERNAL_VALIDATOR_UNAVAILABLE` (the pilot has no validator transport; no fabricated validation artifact, no implicit shell or network validator). Provider errors fail the recipe with `E_INFERENCE_FAILED` (details carry the `ProviderError` variant name, never a secret). Since plan v3.1 (R0.2) the `null` provider mode follows the declared absence policy of §20 instead: `required` requests fail with `E_INFERENCE_REQUIRED` before any provider call, and `fallback` requests are recorded as declared absent.

A0.4 adds to `plumb-inference` (identity-preserving):

```rust
pub fn materialize_derivation_record_from_artifact(
    request: &InferenceRequest,
    artifact: &InferenceArtifact,
    persisted: &PersistedInferenceRefs,
    output_refs: Vec<String>,
    created_at: Timestamp,
) -> Result<DerivationRecord, InferenceError>
```

It validates the request, `artifact.validate_for(request)`, `persisted.validate()` and `persisted.raw_response_ref == artifact.raw_response_hash`, then uses one private builder shared with `materialize_derivation_record`, which keeps its existing `ProviderExecution` validation and delegates. Equal logical inputs give byte-identical `DerivationRecord`s and IDs (golden-tested; existing goldens unchanged). Inside the pure EVALUATE the adapter rebuilds `PersistedInferenceRefs` from its inputs only: `request_artifact_ref` = SHA-256 of the RFC 8785 request (the artifact store's content address), `raw_response_ref` = `artifact.raw_response_hash`, `validated_inference_ref` = the `ArtifactInput` hash. `output_refs` is exactly `[validated_inference_ref.to_string()]`; `created_at` is the child input revision's `created_at`. The record ID becomes the analyser's `DerivationRef`. A0.4 never commits these records to the PSG, stores them in no new ArtifactKind and never computes a `drv:` ID itself; they are reproducible from the request, the persisted artifacts and these two rules.

#### Output classes and acceptance

A `StageEvaluation` with `derivation_patch_set != None` and non-empty `proposals` fails the recipe with `E_STAGE_MIXED_OUTPUT` before any commit (no recipe produces it; nothing is combined or rewritten). A derivation-only evaluation is committed with `validate_derivation_commit`, a plain CAS of exactly that PatchSet from the child base and `record_derivation_commit`.

For a proposal output set:

- **Case A, zero proposals:** no commit.
- **Case B, exactly one proposal whose policy is `AUTO_NON_SEMANTIC` or `AUTO_DERIVATION`:** `validate_proposal_commit`, X1.3 intake against the child base, `requires_reason` must be false (otherwise the recipe fails with `E_AUTO_PROPOSAL_REQUIRES_REASON`; no fabricated decision or rationale), a plain CAS of `proposal.patch_set` with expected head = child base (no companion), then `record_proposal_commit`. Auto-committed proposals are not inserted as workflow proposals; the child records the selected output.
- **Case C, any `HUMAN_CONFIRM`, `HUMAN_DECISION` or `PROFILE_POLICY` proposal:** every proposal of the run, including AUTO ones, is persisted pending; no commit.
- **Case D, two or more AUTO proposals and no HUMAN:** every proposal persisted pending; no commit.

Multiplicity is never a failure and no proposal is chosen by ID order or ranking. A deferred AUTO proposal keeps its policy. Pending stage proposals are inserted into `workflow_proposals` (`proposal_kind` from the A0.3 classifier, `intake_json` = X1.3 intake against the child base, `state = pending`, `origin_kind = stage`, `origin_ref = NULL`, `base_revision_id` = child base) together with their `stage_job_proposals` links in one store transaction. An ambiguous action class or a proposal ID already stored with another origin fails the recipe (`E_PROPOSAL_ACTION_AMBIGUOUS` / `E_STAGE_PROPOSAL_CONFLICT`).

**Sibling staleness.** X1.5 records one selected output per child. After one output of a run advances the branch, the other proposals of that run stay immutable and are stale against the child; they are never rebased or re-identified, and a fresh recipe run is required to regenerate still-applicable ones. For `tags` this means a result with several dictionary proposals is incorporated by accepting one, re-running `tags`, accepting another, and so on. This is the intended consequence of the one-output contract.

#### Governed acceptance of stage proposals

A0.4 adds to `StageJob` (no field, identity, serialization or schema change):

```rust
pub fn record_governed_proposal_commit(
    &mut self,
    run: &CompileRun,
    proposal: &Proposal,
    committed_patch_set: &PatchSet,
    commit: &CommitResult,
) -> Result<(), StageJobError>
```

It requires `status == Ready`, then the existing `check_run(run)` and `proposal_output(run, proposal)` (valid proposal, same stage, same base semantic hash, canonical ref in `output_proposal_refs`), then: `committed_patch_set.base_semantic_hash == proposal.patch_set.base_semantic_hash`; `committed_patch_set.patch` is exactly `Compound { patches: [proposal.patch_set.patch, AddNode { node }] }` (two children, this order, first structurally equal); `node.validate()` succeeds, `node.status == Accepted`, `node.payload` is `ResolutionDecision` with `question_ref == None`, `proposal_ref == Some(proposal.id)`, `patch_ref` = canonical SHA-256 of `proposal.patch_set`, `supersedes == None`; `commit.revision.parent == Some(base_revision)` (else the existing `CommitParentMismatch`); `commit.revision.accepted_patch_ref == Some(canonical ref of committed_patch_set)`. On success: `committed_revision = commit.revision.id`, `committed_output_ref` = the canonical ref of the original Proposal (not the companion, decision or its hash), `status = Committed`. New error `StageJobError::GovernedCommitMismatch(&'static str)` with exactly the reasons `base_semantic_hash`, `compound_shape`, `original_patch`, `decision_status`, `decision_payload`, `decision_question_ref`, `decision_proposal_ref`, `decision_patch_ref`, `decision_supersedes`, `accepted_patch_ref`. It does not re-validate decision IDs, reason codes, rationale, authorization or intake (A0.3 governance owns those) and uses no `plumb-functional` type. `record_proposal_commit`, `record_derivation_commit`, `validate_proposal_commit` and `validate_derivation_commit` are unchanged.

When a user acts on a stage-linked pending proposal through the existing proposal action routes: load the workflow proposal, its link, the parent StageRecipeJob, the child StageJob and its CompileRun, and the current head; `validate_proposal_commit(head, run, proposal)` (a stale or non-Ready child persists Stale and answers the existing `E_PROPOSAL_BASELINE_CHANGED`); re-run X1.3 intake; run the unchanged A0.3 governed path (companion, rationale validation, original Patch artifact, one CAS of `companion.patch_set`); `record_governed_proposal_commit(run, proposal, &companion.patch_set, &commit)`; persist the child Committed; resolve the workflow proposal Accepted; set the parent Committed with `result_revision` = the commit revision. Plain `record_proposal_commit` is never called after a governed commit. Rejecting a stage proposal only resolves the workflow row.

**Ordering and recovery.** The semantic CAS is first; then child Committed, workflow Accepted, parent update. If a process dies after the CAS, recovery (on any `load_reconciled` of that proposal and in the startup sweep over pending stage links) never re-commits: it finds the Accepted `proposal_acceptance` ResolutionDecision with `proposal_ref` = the proposal (the existing A0.3 rule, then resolves the workflow row), walks parents from the head to the revision R whose parent is the child base and whose `decision_refs` contain that decision, loads R's accepted Patch artifact as the committed PatchSet, rebuilds `CommitResult { revision: R, delta: apply_patch(base graph, committed PatchSet).delta }` deterministically, calls `record_governed_proposal_commit` if the child is still Ready, then updates the parent. Each step is idempotent; if no such R exists the child is observed Stale.

#### Run recipe

- **`execute`.** Against the recipe base: the shared scenario execution over the selected runnable Scenarios with the configured calendars, direct-manager predicates, expression scopes, calculation overrides and runtime semantic values; every `ScenarioTrace` artifact is persisted by its existing hash before the CAS (the application calls the same pure function as EVALUATE and requires each update link to equal the persisted hash); traces are recipe evaluation outputs, never PLAN artifacts or acquisition inputs. A present run-update proposal is committed by Case B; none means no revision.
- **`verification`.** Captured against the head after `execute`; `compile_verification_obligations` emits `Semantic` / `HUMAN_CONFIRM` proposals, persisted pending (Case C).
- **`f4`.** Evaluates the exact head after `execute`. `ScenarioRun` is in `SEMANTIC_HASH_EXCLUDED_NODE_TYPES`, so the non-semantic run-update commit creates a new revision with an unchanged semantic hash and the traces bound to that hash stay valid (A0.4 asserts this). Pending verification proposals are not applied hypothetically: missing obligations are F4 violations until accepted and re-run.

A run that committed its ScenarioRun state ends Committed even when F4 FAILs; a gate failure is an evaluated result, never `failed`.

#### Gate inputs

- **F1, F2:** the complete `F1ValidationInputs` / `F2ValidationInputs`, installed with `with_f1_inputs` / `with_f2_inputs` (which validate coverage, statements, evidence, vocabulary resolutions, findings, lint policy, decision-table specs, invariant bindings and overrides). Since plan v3.1 (R0.4) they are assembled per evaluation from the loaded revision, the latest durable analysis artifacts and the explicit external inputs, exactly as the gate input assembly matrix in this section defines; nothing is defaulted, and absent or stale required material is `E_GATE_INPUTS_MISSING` or `E_GATE_INPUTS_STALE`, never an empty-input evaluation.
- **F3 (`ready`):** `decision_artifacts` = for each Accepted ResolutionDecision whose `answer.kind` is in `plumb_validation::S3_RESOLUTION_KINDS` or equals `WAIVER_KIND` (exactly the evaluator's current-S3 selection; `proposal_acceptance` and other governance decisions are excluded), `F3DecisionArtifactInput { decision_ref, hash: decision.patch_ref, bytes }` loaded from the artifact store; `assumption_expiry_findings` = the complete result of `analyze_assumption_expiry(graph, &FixedClock::new(evaluation_at))`, where `evaluation_at` is read once from the injected application Clock when `ready` starts (never the revision's `created_at`), recorded with the persisted report and in the `ready` output. Built with `F3ValidationInputs::new` and installed with `with_f3_inputs`.
- **F4:** `applicability` = since plan v3.1 (R0.4) the sources, executable sources, blocking Scenarios and coverage sources computed from the S4.5 verification applicability input and the graph, with the explicitly configured in-scope Scenario refs and required Scenario kinds; `visible_assumption_refs` = configured; `scenario_type_checks` = one per in-scope Scenario: `specification_hash = scenario_specification_hash(id, scenario)`, `status = Valid` when `plumb_functional::validate_scenario(graph, scenario)` succeeds, else `Invalid { issues: [error text] }` (sorted, unique); `scenario_runs` = each Accepted ScenarioRun node with its linked trace artifact hash and bytes (a broken link fails context validation; nothing is reconstructed). Executable, blocking and required-kind status is never inferred from requirement kind, priority, severity, names or text.

`ready` (CLI) evaluates I0, F1, F2, F3, F4 in that order on one loaded head with the inputs above; it requires f1, f2 and f4 configuration before evaluating anything and persists every report (since plan v3.1, R0.4, the configuration holds only the external inputs of the gate input assembly matrix). Gate ownership: `intake` -> I0, `lint` -> F1, `analyse` -> F2 (when configured), `run` -> F4, `ready` -> all five; there is no StageName for F3.

#### Durable state: store schema v3

`STORE_SCHEMA_VERSION = 3`. Fresh databases are created at v3; v1 -> v3 and v2 -> v3 migrate in one `BEGIN IMMEDIATE` transaction (all or nothing; no downgrade; v4+ rejected). The v2 -> v3 step rebuilds `workflow_proposals` to admit the closed origin `stage` (SQLite cannot alter a CHECK): `CREATE TABLE workflow_proposals_v3` with the v2 columns and `origin_kind IN ('intake', 'question', 'session', 'stage')` plus `CHECK (origin_kind <> 'stage' OR origin_ref IS NULL)`, `INSERT INTO workflow_proposals_v3 SELECT <all eleven columns> FROM workflow_proposals`, `DROP TABLE workflow_proposals`, `ALTER TABLE workflow_proposals_v3 RENAME TO workflow_proposals`; rows are copied unchanged. v1 -> v3 creates the v3 workflow tables directly. Then the five tables below are created and `schema_meta` is set to 3. `WorkflowOrigin` gains `Stage => "stage"`.

```sql
CREATE TABLE stage_recipe_jobs (
  recipe_job_id TEXT PRIMARY KEY,
  recipe_job_json TEXT NOT NULL,
  config_json TEXT NOT NULL,
  recipe TEXT NOT NULL CHECK (recipe IN ('intake', 'lint', 'extract', 'consolidate', 'calc', 'rules',
    'analyse', 'flows', 'scenarios', 'tags', 'run')),
  branch TEXT NOT NULL,
  base_revision_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'acquiring', 'evaluating', 'ready', 'stale',
    'failed', 'committed')),
  result_revision_id TEXT NULL,
  failure_code TEXT NULL,
  failure_message TEXT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK ((status IN ('ready', 'committed')) = (result_revision_id IS NOT NULL)),
  CHECK ((status = 'failed') = (failure_code IS NOT NULL)),
  CHECK ((failure_code IS NULL) = (failure_message IS NULL)),
  FOREIGN KEY (base_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (result_revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE stage_jobs (
  stage_job_id TEXT PRIMARY KEY,
  stage_job_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'acquiring', 'evaluating', 'ready', 'stale',
    'failed', 'committed')),
  compile_run_ref TEXT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (compile_run_ref) REFERENCES artifacts(hash)
);

CREATE TABLE stage_recipe_job_steps (
  recipe_job_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  phase TEXT NOT NULL CHECK (phase IN ('i0', 'f1', 'f2', 'f4', 'segmentation', 'classification',
    'consolidate', 'calc', 'rules', 'analyse', 'flows', 'scenarios', 'tags', 'execute',
    'verification')),
  compiler_stage TEXT NOT NULL CHECK (compiler_stage IN ('S0', 'S1', 'S2', 'S3', 'S4')),
  input_revision_id TEXT NOT NULL,
  stage_job_id TEXT NULL,
  compile_run_ref TEXT NULL,
  intermediate_artifact_ref TEXT NULL,
  gate TEXT NULL CHECK (gate IN ('I0', 'F1', 'F2', 'F3', 'F4')),
  result_revision_id TEXT NULL,
  PRIMARY KEY (recipe_job_id, ordinal),
  UNIQUE (recipe_job_id, phase),
  FOREIGN KEY (recipe_job_id) REFERENCES stage_recipe_jobs(recipe_job_id),
  FOREIGN KEY (input_revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (stage_job_id) REFERENCES stage_jobs(stage_job_id),
  FOREIGN KEY (compile_run_ref) REFERENCES artifacts(hash),
  FOREIGN KEY (intermediate_artifact_ref) REFERENCES artifacts(hash),
  FOREIGN KEY (result_revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE stage_job_proposals (
  proposal_id TEXT PRIMARY KEY,
  recipe_job_id TEXT NOT NULL,
  stage_job_id TEXT NOT NULL,
  compile_run_ref TEXT NOT NULL,
  FOREIGN KEY (proposal_id) REFERENCES workflow_proposals(proposal_id),
  FOREIGN KEY (recipe_job_id) REFERENCES stage_recipe_jobs(recipe_job_id),
  FOREIGN KEY (stage_job_id) REFERENCES stage_jobs(stage_job_id),
  FOREIGN KEY (compile_run_ref) REFERENCES artifacts(hash)
);

CREATE TABLE gate_reports (
  revision_id TEXT NOT NULL,
  gate TEXT NOT NULL CHECK (gate IN ('I0', 'F1', 'F2', 'F3', 'F4')),
  report_hash TEXT NOT NULL,
  report_json TEXT NOT NULL,
  supplemental_input_hash TEXT NULL,
  evaluation_at TEXT NULL,
  recipe_job_id TEXT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (revision_id, gate),
  CHECK ((gate = 'F3') = (evaluation_at IS NOT NULL)),
  FOREIGN KEY (revision_id) REFERENCES graph_revisions(id),
  FOREIGN KEY (recipe_job_id) REFERENCES stage_recipe_jobs(recipe_job_id)
);
```

JSON columns hold RFC 8785 text; reading re-decodes, re-validates (`StageJob` and `GateReport` through their strict serde, the recipe capture through its identity) and requires byte equality, so corruption is an error and never repaired. `report_hash` is `GateReport::content_hash`; `supplemental_input_hash` is the installed F1-F4 input content hash (`NULL` for I0). A report is written with an upsert on `(revision_id, gate)` (the latest evaluation of that revision and gate replaces the previous row). Current gate reports: among durable rows whose report matches the head's semantic hash, evidence hash, profile and rule pack (the A0.2 currency rule), the head revision's row wins, else the latest `created_at` (ties by `revision_id`); durable reports take precedence over the startup-injected ones, which remain an additive compatibility source. Stored historical reports are never presented as current for another baseline and are never rewritten.

**Hotfix 068 amendment (owned current reports).** Durable reports are loaded from the stage store after the state is constructed, so they cannot be borrowed from the immutable startup report list: `ApiState::current_gate_reports` returns owned `Vec<GateReport>` and `current_gate_report` returns `Result<GateReport, ApiError>`, with unchanged currency and precedence semantics, a report persisted by this process becoming current for the next read, and no process-global storage, leaked memory, `OnceCell`/`OnceLock` arena or restart-only visibility. A0.4 may mechanically adapt the three pre-existing route consumers `crates/plumb-api/src/routes/gates.rs`, `crates/plumb-api/src/routes/requirements.rs` and `crates/plumb-api/src/routes/model.rs` (borrowing the owned report where a helper takes `&GateReport`); no route gains persistence, evaluation or orchestration logic. The future A0.4 implementation allowlist is therefore 37 code/test files (the 34 frozen by Hotfix 067 plus these three).

**Hotfix 069 amendment (shared CLI/HTTP application services).** HTTP and the CLI share transport-neutral public application services; business semantics exist once. Route handlers only parse HTTP transport (path, query, cursor, If-Match, multipart, body), call the service and render the existing HTTP response; the CLI parses its own arguments, calls the same service directly (never router dispatch, `tower`, a socket or copied handler logic) and renders text or JSON. Service entry points live in `crates/plumb-api/src/application.rs` and/or as public `ApiState` methods whose signatures use no axum or tower type (no `State`, `HeaderMap`, `Bytes` as an HTTP body, `Multipart`, `RawQuery`, `Response` or `Router`; plain source bytes are allowed). The services are: `create_source` (baseline, display name, declared media type, bytes; owns the importer selection, audit, collision check, artifact persistence, proposal, intake, CAS and `SourceUploadResponse`); the complete glossary projection (`GlossaryTerm` mapping and ordering unchanged; HTTP keeps pagination); the Question projection (Accepted Question material to the existing Question DTO with its validation; HTTP keeps query, filters and cursor; `questions` takes the complete set); `get_round` (the existing reconstruction and consistency checks); `create_round` (baseline, stakeholder, `max` 1..=1000 into `QuestionRoundConfig.question_round_size`, the existing RoundError mapping, dispositions, non-semantic commit and `CreateRoundResponse`); one proposal-action service for accept, create, link, merge, supersede and reject (the existing load and reconciliation, pending and baseline checks, classification and required kind, fresh intake, unchanged governed acceptance including the stage-linked path, single CAS and finalization; reject changes no PSG state; `ProposalKind` and OpenAPI are unchanged); and one pilot projection over the loaded graph, profile and current durable F4 report calling `project_functional_pilot` once, from which HTTP `export_model` and CLI `export` select one artifact and CLI `report` writes all six. CLI writes load the head once as their If-Match equivalent, call the service against that baseline and never retry or rebase; the store CAS stays final. `rounds create` therefore takes `--stakeholder <ID> --max <N>`, the transport equivalent of `CreateRoundRequest.max`, with no implicit size (the HR pilot's 15 is an example, not a default). The A0.4 allowlist becomes 39 code/test files: `crates/plumb-api/src/routes/sources.rs` and `crates/plumb-api/src/routes/questions.rs` are added, and `routes/model.rs` (glossary and export wrappers) and `routes/proposals.rs` (proposal-action wrappers) may be refactored mechanically onto these services with no semantic change.

Store writes are one transaction each; recipe proposals and their links are inserted together. A child already stored under the same StageJob ID is reused as stored (Ready, Stale or Committed children are not re-executed; a Failed one fails the new parent with `E_STAGE_CHILD_FAILED`); its proposal links keep their first parent.

#### Status, progress and result

The parent uses the unchanged `JobStatus` vocabulary: `queued` (captured, nothing begun), `acquiring` (an inference, acquisition or intermediate-loading phase is running), `evaluating` (evaluation, gate evaluation or commit preparation is running), `ready` (completed with no automatic commit: pending human proposals or analysis), `stale` (can no longer complete or be current against its required base), `failed` (operational, configuration, validation or persistence failure), `committed` (completed and at least one automatic or later selected child output advanced the branch). Children keep the X1.5 table; a child Ready whose commit fails for a non-staleness reason stays Ready while the parent becomes Failed (`E_STAGE_COMMIT_FAILED` or the intake/governance code); a moved head makes child and parent Stale. Nothing is ever revived: Stale, Failed and Committed parents and children are terminal. A Ready parent observed (by `get_job`) after the head moved from its `result_revision` becomes Stale, together with its Ready children (through `observe_head`).

`progress`: queued 0.00, acquiring 0.25, evaluating 0.75, ready, stale, failed and committed 1.00. `result_version`: null for queued, acquiring, evaluating, failed and stale; for ready the result revision (the initial base for a recipe that committed nothing); for committed the final revision the recipe produced or reached (for `run`, the ScenarioRun-update revision). `base_revision` is always the parent's initial base. `error` is `"<code>: <message>"` for failed and null otherwise; messages are bounded to 512 bytes and never contain stack traces or secrets.

**Execution and restart.** Each application instance runs recipes on one dedicated worker thread consuming a FIFO queue of recipe job IDs; a recipe starts only by a conditional `queued -> acquiring` transition in the store, so a job is executed at most once. On construction of the A0.4 runtime every parent in queued, acquiring or evaluating becomes failed with `E_JOB_INTERRUPTED` and every child in Queued, Acquiring or Evaluating is moved to Failed through `mark_failed`; Ready and Committed children are never touched and nothing resumes. A pilot database is served by one application process at a time. A repeated `run_stage` whose recipe job ID exists returns that job in any status without starting work; a terminal job is never reset; a new base or configuration is a new ID.

#### HTTP

`run_stage` (`POST /api/stages/{stage}/run`, If-Match required as frozen): load the branch head once; `check_if_match` (412 `E_PRECONDITION_FAILED`); parse the body (`StageRunRequest`); `scope` items are exact Plumb IDs (no trimming, globs, wildcards, prefixes, substring or comma splitting; malformed or duplicate -> 400 `E_INVALID_REQUEST`), sorted; an absent ID -> 404 `E_NOT_FOUND`; a present ID not eligible for the recipe, or any `Some` for a whole-scope recipe -> 422 `E_STAGE_SCOPE_INVALID`; resolve the recipe; validate required configuration (409 `E_STAGE_CONFIG_MISSING`, details `{recipe, missing}`); without the A0.3/A0.4 runtime the existing 503 runtime-unavailable error; capture and insert or reuse the parent (queued); enqueue it outside every lock; respond 200 `StageRunResponse { version: base revision, revision: same, semantic_hash: base semantic hash, events: [], job_id }`. Queueing is not a revision and emits no DesignEvent. `Some([])` captures a recipe that completes ready with no step.

`get_job` (`GET /api/jobs/{id}`): the path must parse as a generic `sha256:` hash (else 400 `E_INVALID_REQUEST`); an unknown job is 404 `E_NOT_FOUND`; it loads the head, applies only the legal Ready -> Stale observation (persisted), and returns `JobResponse` with the ETag of the current semantic hash. No other state is mutated.

#### Runtime configuration, providers and CLI

**`A04RuntimeConfig`** is the single validated application runtime, injected when building `ApiState` (additive to `A03RuntimeConfig`, whose meaning is unchanged); never global, static, OnceCell/OnceLock, environment-discovered or built from request text. Its CLI source is one strict JSON file (unknown fields rejected), RFC 8785-canonicalized for hashing:

`{"version": 1, "identity": {"user", "actor_ref", "roles", "mode"}, "stage_actor_ref", "assistant_actor_ref", "provider_policy": ProviderPolicy, "session_turn": SessionTurnConfig, "validation_policy": ValidationPolicy, "calendars": [{"calendar_ref", "weekend": ["monday".."sunday"], "holidays": ["YYYY-MM-DD"]}], "direct_managers": [{"manager_ref", "employee_ref"}], "expression_scopes": [ExecutionExpressionScopeInput], "calculation_scope_overrides": [ExecutionCalculationScopeOverride], "runtime_semantic_values": [RuntimeSemanticValue], "vocabulary_policy": VocabularyPolicy, "duplicate_policy": DuplicatePolicy, "calculation_scope": CalculationScope|null, "operation_scope": OperationScope|null, "process_scope": ProcessScope|null, "authorization_draft": AuthorizationDraft|null, "scenario_derivation": ScenarioDerivationInputs|null, "verification_applicability": VerificationApplicabilityInputs|null, "gates": {"f1": F1ValidationInputs|null, "f2": F2ValidationInputs|null, "f4": {"applicability": F4ApplicabilityInput, "visible_assumption_refs": [Id]}|null}}`

Calendars are converted by the CLI (which has `time`) into `CalendarDefinition`s and one `StaticCalendarProvider`: valid IDs, lowercase English weekday names, `YYYY-MM-DD` dates, no duplicate `calendar_ref`, no duplicate weekday or holiday within a calendar; no locale, time-zone or holiday service. Direct-manager facts build an application-local `PredicateProvider` in `plumb-api`: `direct_manager(actor, employee)` is true exactly when `(actor.value_ref, employee.value_ref)` is a configured pair, false for an unknown pair; construction rejects an exact duplicate pair, a pair with equal refs and an `employee_ref` listed twice; `RefValue` is never configuration.

Providers are built once at startup: `null` -> `NullProvider` (since plan v3.1, R0.2, `required` inference phases fail with `E_INFERENCE_REQUIRED` before any provider call and `fallback` requests are recorded as declared absent); `replay` -> `MockProvider::with_fixtures` from a strict JSON array of `ProviderExecution` values keyed by `artifact.request_hash` (duplicates rejected, no context-hash lookup); `anthropic` (only with the `llm` feature) -> `AnthropicProvider::new(key)`, the key read once from `--anthropic-api-key-file` (exactly one trailing `\n` or `\r\n` removed; then empty, surrounding whitespace or any control character is rejected; never logged, never in any hash or config).

The CLI calls the same public application services as HTTP (never loopback HTTP, never duplicated route or recipe logic). Global options: `--db <PATH>` (required by every command except `log-effort`), `--branch <NAME>` (default `main`), `--config <PATH>` (required by every command), `--provider null|replay|anthropic` (default `null`), `--replay-fixtures <PATH>` (required with and only with `replay`), `--anthropic-api-key-file <PATH>` (required with and only with `anthropic`; without the `llm` feature `anthropic` is exit 1 `E_PROVIDER_UNAVAILABLE`), `--json`. Commands: `import <FILE> --media-type <TYPE> --name <NAME>` (the `create_source` service); `lint`, `extract`, `consolidate`, `calc`, `rules`, `analyse`, `flows`, `scenarios`, `run`, `tags` (`--scope <ID>`, repeatable; capture and execute the StageRecipeJob in-process and wait); `vocab` (`list_glossary`); `questions` (`list_questions`); `rounds create --stakeholder <ID> --max <N>` / `rounds show <ID>` (`create_round`, `get_round`; Hotfix 069: `--max` is required, 1..=1000, no default); `apply <PROPOSAL_ID> --action accept|create|link|merge|supersede|reject [--reason <TEXT>]` (the proposal action services, including the governed stage path); `ready`; `export --format <FORMAT>` (`export_model`); `report --out <DIR>` (`project_functional_pilot` with the current durable F4 report, writing the six artifacts under their `file_name()`); `serve --listen <ADDR>` (default `127.0.0.1:8080`); `log-effort --file <PATH> --kind stakeholder_hours|analyst_days --amount <DECIMAL> [--note <TEXT>]`. CLI writes use the head they load as the If-Match equivalent and the store CAS as final authority.

Exit codes: 0 success (including a passing gate and a ready or committed recipe without a failing gate); 2 the operation completed but a gate it evaluated failed (`lint` F1, `analyse` F2, `run` F4, `ready` any); 1 operational, configuration, specification, validation, provider or persistence error, and a failed or stale recipe. `--json` prints the structured result (`JobResponse` plus `job_id` for stage commands; for `ready` `{revision, semantic_hash, evaluation_at, gates: [{gate, result, report_hash}]}`).

**serve.** `plumb-api` exposes `pub fn serve_blocking(state: ApiState, listen: SocketAddr) -> Result<(), ServeError>` in its own module (never in `lib.rs`, whose source guard forbids `axum::serve` and `TcpListener`); it builds a multi-thread Tokio runtime, binds the listener and serves `router(state)` until Ctrl-C. `plumb-cli` builds stores, runtime and `ApiState` and calls it; it gains no Tokio, Axum, Tower or reqwest dependency.

**log-effort.** A pilot metric append, not a PSG write: it changes no revision, semantic hash, DesignEvent or gate. It appends one RFC 8785 JSON line `{"version": 1, "recorded_at", "actor_ref", "kind", "amount", "note"}` (`recorded_at` from the CLI's injected clock, `actor_ref` from the configured identity, `amount` a decimal string matching `^[0-9]+(\.[0-9]{1,2})?$` and greater than zero, `note` a clean string or null) plus `\n` to `--file`.

#### A0.4 prohibitions

A0.4 must not change OpenAPI DTOs or paths, add a StageName, PSG node or relation type or SemanticPatch variant, change StageJob, InferenceRequest, Proposal, PatchSet or CompileRun identity or the one-run binding, change accepted domain algorithms, copy private identity functions, infer explicit stage inputs from prose, invent rules or authorization semantics, infer F4 applicability, hold a store lock while calling inference, auto-rebase or retry against a new head, report a failed gate as operational success, keep jobs only in memory, build a provider from HTTP text, read secrets from the environment, add a dependency, implement UI or start U0.1.

### Project bootstrap (R0.1, plan v3.1)

A user starting without a database creates the project through the CLI command `init --project <ID>` with the ordinary `--db` and `--config` options. It calls one application service, `bootstrap_project_database`, built on the canonical initial-revision write; the existing `initialize_project_database` and its callers stay unchanged.

- **Refusal.** It refuses an existing path, any existing file, with no write. On failure no database file remains at the path.
- **Initial revision.** It creates the `main` branch only; any other `--branch` is a usage error. It uses the built-in validation profile's ID and hashes and the configured identity as `created_by`. The graph contains the three configured Agents as `Accepted` provenance nodes: `identity.actor_ref` as human, `stage_actor_ref` as compiler stage and `assistant_actor_ref` as LLM model. Agents are excluded from `semantic_hash`, so the initial semantic hash equals that of the empty project graph.
- **Result.** The database opens immediately for every other CLI command and for `serve`.
- **No HTTP bootstrap.** A server cannot run without an opened database, so HTTP bootstrap would be circular. The CLI is the pilot product boundary.

### Gate input assembly (R0.4, plan v3.1)

F1, F2 and F4 supplemental inputs are assembled at evaluation time from the exact loaded revision. Configuration supplies only material the graph genuinely cannot know. Validation contracts (`with_f1_inputs`, `with_f2_inputs`, `with_f4_inputs`) and every rule are unchanged; only who constructs the inputs changes.

Each field belongs to one of three classes:

- **graph:** constructed mechanically from the loaded PSG;
- **durable:** reconstructed by re-running the pure analyzer over the current graph with the validated inference artifact of the latest completed run of the named recipe on the branch lineage. When the request that the current graph would produce differs from that artifact's request (a statement changed or a requirement was added), the gate fails with `E_GATE_INPUTS_STALE`, naming the recipe to re-run. Re-running that recipe is an ordinary product operation, not a restart or a configuration rewrite.
- **external:** explicit configuration, keyed by element and validated against the current graph. Entries for elements no longer in scope are ignored. A required entry that is missing is `E_GATE_INPUTS_MISSING`, naming the elements. An entry that names an element in a forbidden state is `E_GATE_INPUTS_STALE`.

| Gate input field | Class | Source |
|---|---|---|
| F1 `requirements` set, `requirement_ref` | graph | Accepted Requirement IDs |
| F1 `lint_input.requirement_ref`, `statement`, `evidence_refs` | graph | node ID, `Requirement.statement`, sorted node evidence |
| F1 `lint_input.term_context` | graph | always `None` (validation requires it) |
| F1 `lint_input.source_anchor` | graph | always `None` (never reconstructed from text) |
| F1 `vocabulary_dependencies` | durable | S1.5 `analyze_vocabulary` over the latest `consolidate` vocabulary artifact |
| F1 `analysis_findings` (`TERMS_RESOLVED`) | durable | the same S1.5 analysis |
| F1 `analysis_findings` (`NO_DUPLICATE_ACCEPTED`) | graph | `analyze_requirement_duplicates` with the configured `duplicate_policy` |
| F1 `analysis_findings` (`NO_CONTRADICTION`) | none | no producer exists; the rule keeps reading Accepted Open Finding nodes |
| F1 `lint_policy` | external | configured organization lint policy |
| F2 `analysis_findings` | durable | S2.1 `analyze_domain` and S2.2 `analyze_lifecycle` over the latest `analyse` artifacts |
| F2 `calculation_scope_overrides` | external | the existing runtime `calculation_scope_overrides`, used only for Accepted Calculations without an S2.6 origin |
| F2 `decision_tables` | graph | `decode_decision_table` of each Accepted DecisionTable; an undecodable table fails the gate explicitly |
| F2 `invariants` bindings | external | the existing runtime `expression_scopes` whose owner is an Accepted Invariant |
| F4 `applicability` sources, executable sources, blocking scenarios, coverage sources | graph over external | S4.5 `verification_applicability` input and the graph, exactly as S4.5 computes its snapshot |
| F4 `in_scope_scenario_refs`, `required_scenario_kinds` | external | explicit F4 configuration; never inferred |
| F4 `visible_assumption_refs` | external | explicit F4 configuration |
| F4 `scenario_type_checks`, `scenario_runs` | graph | already derived (unchanged) |

Nothing is inferred from names, text, kinds, priorities or severities. External policy, organization facts and applicability stay explicit.

#### Current gate truth and requirement reads (R0.4, plan v3.2)

- **Factual inputs.** Every factual F1, F2 and F4 input is assembled from the current PSG and the current durable stage artifacts exactly as the matrix above states. Startup configuration supplies only policy, execution/environment material and explicitly external facts; it never stands in for current semantic-model truth.
- **Requirement reads without a current F1 report.** Requirement list and detail never require a current F1 GateReport. `RequirementSummary` and `RequirementDetail` gain a required boolean `lint_current`: `true` exactly when a current F1 report exists for the loaded head, in which case `smells` and `lint_findings` are projected from it as today; `false` otherwise, in which case `smells` and `lint_findings` are empty and `readiness` is `null`. "No current F1 report" is therefore never indistinguishable from "a current report with zero findings". The change adds two schema properties and no HTTP operation (the inventory stays 74); the UI shows lint as unavailable when `lint_current` is false.
- **Readiness of an empty project.** The pilot `ready` outcome carries a product-level precondition list. When the loaded head has zero Accepted Requirements it contains `no_accepted_requirements`, `ReadyOutcome::passed()` is false and the CLI exits 2 with `precondition: no_accepted_requirements`. Gate reports, rule applicability and rule results are unchanged; no rule is forced to FAIL.

### UI test-runner dependency (U0.1, Hotfix 070)

The frozen UI script `"e2e": "playwright test"` is the Playwright Test runner, shipped by `@playwright/test`; `playwright` is only the browser automation library. Hotfix 070 is the human-approved plan revision that adds exactly one direct UI development dependency, `@playwright/test@1.63.0`, next to the unchanged `playwright@1.63.0` (the versions intentionally match). `U0.1` adds it to `ui/package.json` and `ui/package-lock.json` with `npm install --save-dev --save-exact --strict-peer-deps --strict-allow-scripts @playwright/test@1.63.0`, and `playwright test` resolves from that local dependency graph only (never npx remote execution, a global install, `@playwright/cli`, a custom runner or Vitest browser mode); `ui/playwright.config.ts` may import `defineConfig` from `@playwright/test`. The JavaScript installation policy is unchanged: `npm ci --strict-peer-deps --strict-allow-scripts`, `npm ls --depth=0` and `npm audit --audit-level=high` must succeed with zero HIGH and CRITICAL findings, no peer conflict, unsupported engine or unauthorized install script, and without `--force`, `--legacy-peer-deps`, `--ignore-scripts`, overrides or dynamic versions; failing that, U0.1 stops BLOCKED-UI-DEPENDENCY. No other JavaScript dependency changes; `@stoplight/prism-cli` stays forbidden and MSW stays the pilot API mocking dependency. Because U0.1 owns no unit-test file while running `vitest run`, `ui/vite.config.ts` sets Vitest `passWithNoTests: true` so the empty scaffold passes; this never suppresses failing tests once tests exist. The package scripts are unchanged, and Hotfix 070 left the U0.1 allowlist at its 13 files (Hotfix 071 below makes it 15).

### UI scaffold and cross-task integration ownership (Hotfix 071)

Hotfix 071 repairs UI ownership gaps without redesigning product UX, adding a dependency, a task or a dependency edge (73 tasks; E0.1 still depends on A0.4 and U2.7, so the UI chain precedes E0.3 transitively).

- **Vite entry.** U0.1 owns `ui/index.html` (its allowlist becomes 15 files with `ui/src/i18n/index.ts`): a minimal deterministic Vite HTML entry with one `id="root"` element and the module entry `/src/main.tsx`; no product shell, CDN, external script or Tailwind Play CDN.
- **Tailwind.** `ui/vite.config.ts` configures `@tailwindcss/vite` and `ui/src/main.tsx` imports the package stylesheet `tailwindcss/index.css`, activating the installed exact `tailwindcss@4.3.3` and `@tailwindcss/vite@4.3.3`; there is no local CSS, `tailwind.config.*` or `postcss.config.*` file.
- **Typed i18n.** `ui/src/i18n/index.ts` loads `en.json` and `pl.json`, exports `Locale = "en" | "pl"`, derives `MessageKey` from the English catalogue and offers one typed lookup with English as default and fallback; no i18n dependency, network loading or eval. Both catalogues are flat maps of stable dot-separated keys to strings with identical key sets (English authoritative, Polish may reuse an English value but never omits a key). U1.1, U1.2 and U2.1-U2.7 may each add or adjust only the entries their own user-facing UI needs, never deleting or repurposing unrelated keys. The hard-coded JSX string lint stays: visible JSX text and user-facing literal labels are rejected in production TSX, implementation-only strings (className, ids, route paths, data attributes, technical enum values) are allowed, tests and i18n files are exempt, and no ESLint dependency is added.
- **Explicit router evolution.** U0.1 creates the typed TanStack Router scaffold (a minimal root and index route without product content); U1.1 mounts AppShell, HomePage and the shell route structure; each of U2.1-U2.7 registers only its own new product route(s). No task rewrites unrelated routes, and there is no dynamic discovery, import-glob routing, runtime route reflection or placeholder route to a future component. U1.1 implements the full pilot navigation in `nav.ts`; an unimplemented destination may appear in navigation but is never routed to a fake feature component.
- **ConflictDialog sequencing.** U1.1 creates the minimum real shared `ui/src/components/ConflictDialog.tsx` through which `useVersionedMutation` surfaces a 412 and requires explicit user action (no automatic retry); U1.2 refines and tests the same component; no second dialog exists.
- **Session shell integration.** U2.3 may modify `ui/src/app/AppShell.tsx` only to mount the project/scoped session panel and register any project-room route its frozen behavior requires.
- **Dev proxy.** In development `ui/vite.config.ts` proxies `/api` (including the `/api/events` WebSocket) to `http://127.0.0.1:8080`, overridable only as Vite dev configuration; the UI keeps relative `/api` paths and the package scripts stay the six frozen ones (no `dev`, `build` or `start`).
- **E0.3 Makefile ownership.** E0.3 owns `Makefile` and turns the existing `e2e` target into a real run of `scripts/run-pilot-e2e.sh` (optionally also the backend E2E test), no longer a file-existence check; the runner starts the seeded real backend, the locally installed Vite (`ui/node_modules/.bin/vite` or an invocation provably resolved from the frozen local graph; never `npm install`, remote `npx` or a CDN) and Playwright Test, and tears its child processes down. E0.4 may extend `make all` and quality thresholds but must not undo this.

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
    pub inference_requests: Vec<PlannedInferenceRequest>,
    pub external_validation_requests: Vec<ExternalValidationRequest>,
}

pub struct PlannedInferenceRequest {
    pub request: InferenceRequest,
    pub material: InferenceRequestMaterial,
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
- **ArtifactSet** lists are each sorted by hash, and hashes are unique across the set. Inference inputs are `validated-inference` / `application/json`; validation inputs are `external-validation` / `application/json`. Against its `StagePlan` it matches one-to-one: one identical deterministic input per planned artifact, one canonical `InferenceArtifact` input valid for each planned inference request, one canonical `ExternalValidationArtifact` input valid for each validation request, and nothing extra.
- **PlannedInferenceRequest** (Hotfix 066) pairs a request with its exact material; construction and decoding (unknown fields rejected) require a valid request and a material binding that validates against it.
- **StagePlan** lists are ordered by content hash, request ID and request ID respectively, without duplicates; every planned inference request carries the stage's `StageId`. It contains no timestamps, provider clients or store handles. `ArtifactSet` matching uses each planned request; the material is never passed to EVALUATE.
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

## 33. Pilot hardening contracts (plan v3.2)

An independent audit after plan v3.1 was checked against the repository; only the confirmed defects below are contracted. Each subsection is the frozen contract of one H0 task; tasks run in order H0.1 through H0.11 after R0.6 and before R0.7.

### 33.1 Bounded untrusted input (H0.1)

Untrusted input never reaches an unbounded allocation, unbounded recursion or an allocator/stack abort; every limit fails with a typed error. The frozen constants are:

| Constant | Value | Applies to |
|---|---|---|
| `DOCX_MAX_INPUT_BYTES` | 134217728 (128 MiB, unchanged) | DOCX bytes |
| `DOCX_MAX_ENTRIES` | 4096 (unchanged) | ZIP entries |
| `DOCX_MAX_TOTAL_UNCOMPRESSED_BYTES` | 536870912 (512 MiB, unchanged) | declared uncompressed total |
| `DOCX_MAX_XML_PART_BYTES` | 33554432 (32 MiB, unchanged) | one XML part |
| `DOCX_MAX_XML_DEPTH` | 256 | open-element nesting while parsing one XML part |
| `DOCX_MAX_TABLE_COLUMNS` | 64 | one `w:gridSpan` value and the summed span of one table row |
| `DOCX_MAX_TABLE_ROWS` | 10000 | rows of one table |
| `TEXT_MAX_INPUT_BYTES` | 16777216 (16 MiB) | Markdown and plain-text bytes |
| `SOURCE_MAX_FRAGMENTS` | 20000 | EvidenceFragments produced by one import (every importer) |
| `EXPR_MAX_SOURCE_BYTES` | 4096 | one PlumbExpr source |
| `EXPR_MAX_NESTING_DEPTH` | 64 | parenthesis, call-argument and consecutive prefix-operator nesting |
| `EXPR_MAX_AST_NODES` | 1024 | nodes of one built AST |
| `MAX_SOURCE_UPLOAD_BODY_BYTES` | 135266304 (129 MiB) | HTTP body of `POST /api/sources` |
| `MAX_JSON_BODY_BYTES` | 4194304 (4 MiB) | HTTP body of every other route |

- **DOCX.** The workspace `quick-xml` dependency moves from 0.37.5 to a 0.41 release (`>= 0.41.0`). XML parsing counts open elements and fails at depth 257. `w:gridSpan` is parsed as an integer and accepted only in `1..=64`; a row whose summed span exceeds 64 or a table with more than 10000 rows fails; no allocation is ever sized from an unchecked attacker value. Every violation is `ImportError::DocxLimit` (`E_DOCX_LIMIT`) naming the limit.
- **Every importer.** An import that would produce more than 20000 fragments fails with `E_SOURCE_LIMIT`. Markdown and plain text above 16 MiB fail with `E_SOURCE_LIMIT` before decoding.
- **CLI.** `import` checks the file length from metadata against the limit of the declared media type before reading and then reads at most limit + 1 bytes; an oversized file fails without being read into memory.
- **Expressions.** `plumb_expr::parse` rejects sources above 4096 bytes and a lexical pre-scan rejects nesting above 64 before the recursive grammar runs; the built AST is rejected above 1024 nodes. The error is a typed `ExprError::LimitExceeded {limit, actual, max}` with code `E_EXPRESSION_LIMIT`. X1.3 intake checks every expression-bearing payload field of a proposal — `Transition.guard_expr`, `Invariant.expression`, `ProcessNode.condition_expr`, `ProcessNode.timer_expr`, `Rule.condition_expr`, `Rule.action_expr`, `Calculation.expression`, `PolicyCondition.expression` — against the same limits, so an oversized or pathological expression can never be persisted in the PSG.
- **HTTP.** PLUMB owns its body limits: 129 MiB on `POST /api/sources`, 4 MiB elsewhere. An oversized body is 413 `E_PAYLOAD_TOO_LARGE` in the ordinary typed JSON envelope with `details.limit_bytes`. No OpenAPI operation changes.

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #367] validate_display_name rejects '/', '\\', '.', '..', Unicode bidi and format control characters and names longer than 255 characters with a typed import error, and the core Id grammar rejects complete Id strings longer than ID_MAX_BYTES = 255 bytes (the whole serialized Id: namespace, colons and body segments); because the grammar is ASCII-only this is also 255 characters; 255 is accepted and 256 is rejected, with no normalization or truncation, the existing grammar unchanged and the existing CoreError::InvalidId error.


### 33.2 Revision-complete optimistic concurrency (H0.2)

Proposed elements do not contribute to `semantic_hash`, so a semantic-hash `If-Match` lets a write based on a stale view of Proposed state pass. The concurrency token becomes the exact branch-head GraphRevision ID.

- Every GET that sets `ETag` sets it to the loaded GraphRevision ID (`rev:<version>:<16 hex>`), the exact string.
- Every write that requires `If-Match` (and `POST /api/stages/{stage}/run`) requires it to equal the exact current head revision ID; a missing header stays 400; any other value is the existing 412 `E_PRECONDITION_FAILED` whose body still carries the current revision and semantic hash.
- `semantic_hash` stays available (version endpoint, write results, 412 body) as semantic identity; it is no longer a concurrency token. Cursors keep their existing revision-and-semantic-hash baseline protection unchanged.
- The UI carries the ETag of the data the user actually reviewed: each query keeps the response `ETag`, and a mutation sends that value. `useVersionedMutation` no longer fetches `/api/model/version` inside the mutation; a 412 keeps the existing conflict dialog and never retries.
- OpenAPI descriptions of `ETag`/`If-Match` change to the revision ID; the operation inventory stays 74 and the generated client is regenerated.

**Entity-tag form (plan v3.2 hotfix).** The `ETag` value is the RFC 9110 strong entity-tag form of the revision ID (the revision ID in double quotes); `If-Match` accepts that quoted form or the bare revision ID; weak validators and `*` are 412 `E_PRECONDITION_FAILED`. This refines the "exact string" wording above.

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #209] ETag is emitted as an RFC 9110 strong entity-tag (the quoted revision ID) and If-Match accepts exactly that quoted form or the bare revision ID; weak and '*' validators are 412 E_PRECONDITION_FAILED, as documented in OpenAPI.


### 33.3 Retryable and process-owned stage execution (H0.3)

- **Identity versus attempts.** The content-addressed recipe job ID and child StageJob ID stay exactly as captured. Each recipe job row gains `attempt` (starting at 1) and an attempt record table `stage_recipe_job_attempts(recipe_job_id, attempt, owner_id, started_at, ended_at, outcome, error)`; `STORE_SCHEMA_VERSION` increases by exactly 1 with an in-place additive migration.
- **Retry.** A new application service and CLI command `retry <JOB_ID>` re-queue a Failed recipe job as attempt N+1 under the same ID, and move each of its Failed children to Queued through a new StageJob `retry` transition (Failed -> Queued, nothing else). Retry requires the branch head to still equal the job's captured base revision; otherwise it is 409 `E_STAGE_RETRY_BASE_MOVED` and nothing changes. Stale, Ready and Committed jobs are never retried; Stale semantic work is never silently re-run or rebased. No HTTP operation is added.
- **Ownership.** Each process that executes stage work has an `owner_id` = `own:<first 16 hex of SHA-256(RFC 8785 {pid, process start time, db path})>`. Claiming a job writes `owner_id` and `lease_expires_at` = now + `STAGE_LEASE_SECONDS` (60); the executing worker renews the lease every `STAGE_HEARTBEAT_SECONDS` (15).
- **Recovery.** Startup recovery interrupts (`E_JOB_INTERRUPTED`) only in-flight jobs whose lease has expired or that have no owner (pre-H0.3 rows); a second live process never interrupts a job with an unexpired lease owned by another process.
- **Read-only commands.** The application has an explicit open mode: `Interactive` (worker, recovery, reconciliation) and `ReadOnly` (no worker, no recovery or reconciliation writes). Read-only CLI commands open `ReadOnly`.
- **Worker errors.** The worker never discards an execution error: any `Err` from executing a claimed job persists the job as Failed with `E_INTERNAL` and the bounded message, and is logged.

**Graceful shutdown (plan v3.2 hotfix).** On SIGINT/SIGTERM or server shutdown the queue closes to new claims; the in-flight job reaches its next bounded safe step boundary (after the current provider call returns and its bundle is persisted, or before the next phase starts) within `STAGE_SHUTDOWN_GRACE_SECONDS` = 30, otherwise it is persisted Failed with `E_JOB_INTERRUPTED`; the worker `JoinHandle` is joined with that bound. An in-flight paid inference is never abandoned in an ambiguous job state.

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #62] Acquisition reuses an already persisted ValidatedInference whose request hash equals the planned request ID instead of calling the provider again (so a retried attempt never re-bills or re-generates a completed request), and the live provider retries 429/529/5xx with bounded backoff honoring retry-after without repairing output.
- [Fable #179] run_stage fails with a typed 503 when the job cannot be enqueued because the worker is gone, and a panic while executing a claimed job is caught, persists that job as Failed with E_INTERNAL and is logged, and the worker keeps serving later jobs.
- [Fable #183] Interactive startup recovery and reconciliation are per-row fault tolerant: a row that cannot be decoded or reconciled is marked failed with a typed corruption error and logged, the remaining rows are processed, and opening the application does not fail because of it.
- [Fable #188] Recovery of an interrupted recipe job whose child proposals were already persisted either settles the parent to Ready over those proposals or marks the proposals stale in the same transaction; a pending stage proposal is never left linked to a Failed parent that cannot reflect its acceptance.
- [Fable #253] Every failed inference acquisition records in its failed attempt and job error a bounded typed detail (provider error class, HTTP status and provider request id when present, request ID and task kind, sorted schema-validation errors), and stores any rejected raw provider response as a content-addressed InferenceResponse artifact referenced from that failure.


### 33.4 Executable business failures and outcomes (H0.4)

The simulator maps every `ExecutionResult::Failed` to Fail today and makes any Operation with more than one produced Outcome Undecidable. The following is frozen.

- **Failure classes.** Business rejections are exactly the `ExecutionFailure` kinds `authorization_denied`, `precondition_rejected`, `transition_guard_rejected`, `invalid_source_state`, `invariant_violation` and `no_decision_row`. `postcondition_violation` and `evaluation_failed` are unexpected model failures. Missing semantics stay Undecidable. A simulator `Err` (infrastructure or internal error) produces no verdict and is never converted into Pass.
- **Outcome selection material.** An Outcome may carry the node extension `plumb_sim:outcome_selection` = `{"version": 1, "selections": [...]}` whose entries, sorted by `operation_ref`, are either `{"operation_ref": Id, "when": <PlumbExpr Bool source over the post-execution state>}` (only on `success` or `partial` Outcomes) or `{"operation_ref": Id, "on_failures": [sorted unique business failure kind wire names]}` (only on `business_failure` Outcomes). `technical_failure` Outcomes are never selected by the simulator. The extension is written only through a governed `SetNodeExtension` proposal produced from the R0.6 semantic directory (`outcome_selections` drafts) and confirmed like other directory material.
- **Successful execution.** The candidates are the Operation's Accepted `produces` Outcomes of kind `success` or `partial`. One candidate without a selection entry is selected as today. With several candidates, every candidate needs a `when` entry for that Operation; exactly one true selects it; none or several true, or a missing entry, is Undecidable with `OUTCOME_SELECTION`.
- **Business rejection.** For a business failure kind the candidates are the Operation's Accepted `business_failure` Outcomes whose entry for that Operation lists the kind. Exactly one candidate becomes the observed outcome, the state stays the pre-state, no event is produced, and `then` is compared normally — so a Scenario whose `then.outcomes` names that Outcome passes. No candidate is Fail with the existing execution-failure diff (unexpected rejection); several candidates are Undecidable with `OUTCOME_SELECTION`.
- **Model failures.** `postcondition_violation` and `evaluation_failed` remain Fail whatever the Scenario expects.
- The HR negative scenarios (for example SCN-010 insufficient balance, SCN-013 non-manager approval, SCN-022 withdraw approved) are the regression shapes; E0.1 maps the fixture `then.outcome: business_failure` to the Operation's selected `business_failure` Outcome reference.

**Holiday-policy question (plan v3.2 hotfix).** The `f2.calculation.holiday-policy.v1` answer schema offers only recordable answers: `{"type":"object","additionalProperties":false,"required":["holiday_consumes_working_day","calendar_ref"],"properties":{"holiday_consumes_working_day":{"const":false},"calendar_ref":{"type":"string","enum":[<sorted Accepted Calendar IDs>]}}}`. The prompt states that a configured holiday never consumes a working day and asks which Accepted Calendar applies. The resolver mapping is unchanged (false + `calendar_ref` binds the Accepted Calendar; `UnsupportedHolidayPolicy` remains a defensive error), so the immutable HR expert answer stays valid.

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #116] content-dependent model conditions in a scenario run yield a per-scenario Undecidable/Fail verdict with a typed reason while other scenarios still execute; UNIQUE decision-table overlaps are rejected at model build and Given literals are type-checked at scenario validation.
- [Fable #117] an Operation governed by several DecisionTables routes each supplied decision_input to its table (or reports a dedicated typed condition when routing is ambiguous), with a test.
- [Fable #119] an unparseable expression at any simulation site (precondition, postcondition, rule condition, transition guard, invariant) compiles to Unparseable and makes only dependent scenarios Undecidable; type errors in parseable expressions remain build errors.


### 33.5 Replay-safe semantic stages (H0.5)

A deterministic producer recognizes its own accepted output by its deterministic ID, node type, origin extension value and the payload fields it owns; later enrichment of other fields is legitimate and never overwritten.

| Producer | Node type | Producer-owned payload fields | Enrichable fields |
|---|---|---|---|
| S2.1 domain | Entity | `name` | `description`, `aggregate_root` |
| S2.1 domain | Attribute | `name` | `value_type`, `nullable`, `unit`, `precision`, `enum_values`, `data_classification` |
| S2.1 domain | DomainRelationship | `from_entity`, `to_entity`, `relationship_kind` | `cardinality_from`, `cardinality_to`, `name`, `snapshot_semantics`, `ownership` |
| S1.5 vocabulary | Term | `term`, `language` | `definition_ref`, `aliases`, `status` |
| S1.5 vocabulary | Concept | `name`, `concept_kind` | `definition` |

- A rerun whose existing node matches on ID, type, origin and owned fields is a no-op for that node (no proposal). A mismatch of any owned field or of the origin is a real conflict (`ExistingCandidateConflict` / `ExistingVocabularyCandidateConflict`). Vocabulary no longer accepts any Accepted node without comparing its owned fields.
- Batch analysis is per-target tolerant: one unsupported, stale or origin-less target yields a deterministic per-target disposition in the analysis result instead of aborting the batch. EARS analysis of a Requirement without the S1.1 segment origin yields the disposition `not_applicable: missing_segment_origin` for that Requirement.

**Analyzer issues (plan v3.2 hotfix).** Production never discards analyzer findings, conflicts or issues required by governed resolution: each phase persists its batch-level issues and per-target dispositions in its CompileRun intermediate projection and surfaces them in the job result, or classifies them explicitly as non-governable. No caller reads only `.proposals`.

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #105] the owned-field reconciliation table also covers S2 lifecycle Transition (enrichable guard_expr, effect_refs), S2.6 Calculation (enrichable examples and governed inclusive_end rewrite) and S4.5 verification obligations (enrichable acceptance_condition, required_evidence_kind).
- [Fable #106] a deterministic candidate whose ID holds a Rejected or otherwise inactive node gets an explicit rejected/inactive disposition (never ExistingEquivalent), and dependent candidates report a typed dependency-rejected issue instead of staying pending.
- [Fable #138] When a same-type Proposed Term or Concept exists at the deterministic ID with a different vocabulary_origin or evidence, analyze_vocabulary records a deterministic per-key disposition (pending_candidate_differs) and continues with every other key; only a different node type at that ID, or an Accepted owned-field mismatch, is ExistingVocabularyCandidateConflict, and no conflict aborts the whole analysis.


### 33.6 Finding and waiver governance closure (H0.6)

- **Durable findings.** `materialize_gate_findings` turns the findings of the current persisted F1, F2 or F4 GateReport into durable Accepted Finding nodes: every `GeneratedFinding` whose ID is absent from the graph is added in one non-semantic AutoNonSemantic proposal (Findings are excluded from `semantic_hash`); present IDs are no-ops. It is exposed as the CLI `findings materialize` and listed by the read-only `findings list`.
- **Dispositions.** Each finding has exactly one disposition: `question_resolvable` when a frozen Question template matches its code and condition (it is materialized through S3.1 and resolved through S3.3); `waivable` when its rule's `waiver_policy` is `decision_required`, or `profile_allow` and the active profile allows waivers (it is governed through the existing waiver path); `non_waivable` otherwise (`forbidden`): it can only be fixed in the model.
- **Aggregate checks.** `PLUMB.F2.NO_UNRESOLVED_SEMANTIC_BLOCKER` (and any equivalent aggregate) counts a blocker as unresolved only when it is Open and not governed. A finding is governed when an Accepted, non-superseded ResolutionDecision answers a Question whose `finding_ref` is that finding, or when a current applicable waiver decision covers it. The same rule applies to supplemental `analysis_findings`, by finding ID. No finding is excluded from the check; only governed ones stop counting.
- **Waiver actor.** The existing check is retained: a waiver decision's `decided_by` must be an Accepted Agent of kind `human`; an unknown, non-Accepted or non-human actor never satisfies a waiver.

**Correction and lifecycle (plan v3.2 hotfix).** The waiver actor check above exists today only in the producer (`prepare_waiver_material`); the gate-time consumer (`governed_waiver` / `check_waiver_decision`) does not check `decided_by`. H0.6 adds the consumer-side check: a waiver decision whose `decided_by` is not an Accepted human Agent is malformed and never applied. In addition: a rule never consumes a Finding it generated itself; an Open Finding whose affected refs no longer all exist as baseline elements is reported stale and closed through one governed non-semantic status update; a governed Finding is closed (Resolved or Waived) through governance; findings of the current I0 report are materialized as well.

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #124] materialize_gate_findings also materializes findings of the current persisted I0 GateReport, so waivable I0 rules (PARSE_STATUS decision_required, AGENT_IDENTIFIED profile_allow) have a reachable governance path.
- [Fable #125] governed_waiver/check_waiver_decision require decision.decided_by to be an Accepted Agent of kind human at gate evaluation (not only at waiver creation); a waiver decision by a non-human, non-Accepted or unknown actor is MalformedWaiverDecision and never applied, with consumer-side regression tests.
- [Fable #234] The waiver staleness test first proves the finding is current on a fresh app, then moves the head and asserts 409 E_FINDING_NOT_CURRENT; waiver test finding IDs are derived from the evaluated report, never hard-coded.


### 33.7 Blocking rule fidelity and semantic edit guards (H0.7)

- **Permission.** `RBAC.F2.PERMISSION.CONCRETE` follows the profile: at least one `permits` target, every target an Accepted Operation, and exactly one `scoped_to` Accepted ResourceScope.
- **Process.** Every node-shape violation of the process analyzer is reported by one of the four F2 process rules, attributed by node kind: start and end events to `BPMN.F2.PROCESS.START_END`, gateways to `PLUMB.F2.PROCESS.GATEWAY_BALANCED`, tasks to `PLUMB.F2.PROCESS.TASK_RESOLVES`, every other node kind to `BPMN.F2.PROCESS.REACHABLE`. The attribution is one exhaustive match, and a process is qualified (`is_qualified`) exactly when all four rules pass for it.
- **Events.** An Event may carry the node extension `plumb_functional:event_disposition` = `{"version": 1, "external_source": bool, "terminal_purpose": null | "terminal" | "audit"}`, written only through a governed `SetNodeExtension` proposal from the R0.6 directory (`event_dispositions` drafts). `PLUMB.F2.EVENT.PRODUCER` passes for a consumed event without a producer exactly when `external_source` is true; `PLUMB.F2.EVENT.CONSUMER` passes for a produced event without a consumer exactly when `terminal_purpose` is set; otherwise the rule reports a violation. Neither rule returns an evaluator error for an unclassified event any more.
- **Lint.** Numeric tokenization treats as one non-violating token: digit groups with thousands separators (`1,000`, `12,500.50`); ISO dates (`2026-01-01`); a number joined by a hyphen to a word (`5-day`, which counts as having a unit); `HH:MM` times (`09:30`); a parenthesised list numeral (`(3)`); and decimals (`1.5`).
- **Requirement edits.** `classify_requirement` and `accept_requirement_ears` act only on Proposed or Accepted Requirements; any other status is 422 `E_SEMANTIC_INVARIANT` with error `RequirementNotEditable`.

**Rule adjudications (plan v3.2 hotfix).** Each of the following is resolved against the authoritative profile, rulebook and metamodel into one contract: `PLUMB.F1.REQ.NO_CONTRADICTION` gets its deterministic producer or a recorded NOT_APPLICABLE reason when no producer input exists; `PLUMB.F2.STATE.REACHABILITY` evaluates reachability from the declared initial state instead of a permanent evaluator ERROR; Dynamic SoD, MutualExclusion and RequiredCombination constraints present in the graph are evaluated, never NOT_APPLICABLE; `PLUMB.F1.REQ.MODALITY_EXPLICIT` applies exactly per `applies_when` and treats `must` per the profile modality list; `PLUMB.F3.ASSUMPTION.OWNER` evaluates exactly the profile condition; `OPERATION.PERFORMER` and `INVARIANT.EXPRESSIBLE` profile alternatives become representable through governed directory material or are reported as violations, never evaluator errors.

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #71] numeric tokenization also treats a slash-joined numeric ratio/frequency form (e.g. '24/7') as one non-violating token, with a negative fixture.
- [Fable #72] clause-boundary detection uses the same numeric tokenizer, so a '.' or ',' inside a numeric token is never a clause boundary, with fixtures '0.5 days by the system' and '2.5 days without approval'.
- [Fable #73] MISSING_ACTOR looks back across a comma-delimited parenthetical to the preceding segment before reporting, with negative fixtures 'The system, upon approval, shall ...' and 'The manager, or a delegate, shall approve ...'.
- [Fable #74] RELATIVE_TIME_NO_ANCHOR accepts an optional qualifier (working/business/calendar) before the unit and treats an operation-duration bound ('complete/respond within N <time unit>') as anchored, and fixture-contract expectations record the lint outcome for HR-030.
- [Fable #75] each Error-severity lint rule (NUMBER_NO_UNIT, RELATIVE_TIME_NO_ANCHOR, MISSING_ACTOR) has a table-driven adversarial fixture set of ordinary-English negatives and true positives, and run_benchmark/compare_to_baseline/recommend_severities error paths are tested.
- [Fable #76] every public lint benchmark entry point (including recommend_severities) validates its threshold Ratio (denominator > 0, value <= 1) and returns LintError::Benchmark otherwise.
- [Fable #126] PLUMB.F1.QUALITY_FINDINGS_CLEAR skips Requirements whose statement is empty or whitespace-only (owned by ISO29148.F1.REQ.STATEMENT_PRESENT) and evaluates every other Requirement; a lint error on one Requirement never turns the rule into an evaluator ERROR for the others.
- [Fable #131] ISO/PLUMB I0 LOCATABLE, HASH_MATCH and PARSE_STATUS return NOT_APPLICABLE with a recorded reason when the baseline contains no evidence of a built-in source kind, and any baseline EvidenceFragment of a non-built-in source kind is reported, never silently treated as verified.
- [Fable #164] PLUMB.F2 decision-table NO_OVERLAP is NOT_APPLICABLE for overlap-permitting hit policies (ANY, COLLECT, RULE ORDER, PRIORITY) per the profile applies_when, while coverage is still analysed; only genuinely unsupported structure is an ERROR.
- [Fable #165] F4 TRACE_COMPLETE additionally requires the execution phase to end with its S4.2 terminal step (execution_completed for a completed run) and reports trace_execution_incomplete otherwise.
- [Fable #299] PLUMB.F1.REQ.TYPE_KNOWN is declared pass-by-construction in the profile, F3 QUESTION.NO_BLOCKING_OPEN treats Questions on blocker Findings of any family as blocking, and F1 CRITERIA_FOR_BEHAVIOR honours its 'in implementation scope' qualifier, each matching the profile text.


### 33.8 Inference-provider and pilot network security (H0.8)

- **Anthropic.** The native Messages API is authenticated with `x-api-key: <key>`, never `Authorization: Bearer`. The configured `anthropic-version` is retained, and `anthropic-workspace-id` is sent exactly when configured. Redirects stay disabled and the key stays redacted. Contract tests assert the exact headers. The `anthropic`/`llm` features compile (`cargo build -p plumb-cli --features llm`).
- **Replay.** `MockProvider::execute` validates the fixture's validated output against the request material's compiled task output schema before returning it, exactly as the live provider does before admission; an invalid fixture is `ProviderError::SchemaInvalidOutput`. A replay fixture is never a schema bypass.
- **Loopback-only serving.** The product has no user authentication yet. `serve` refuses any non-loopback bind address with a typed configuration error. Every request must carry a `Host` whose host part is `127.0.0.1`, `[::1]` or `localhost`, otherwise 403 `E_FORBIDDEN_HOST`. A request with an `Origin` header must have an `http` origin whose host is one of those loopback hosts (any port), otherwise 403 `E_FORBIDDEN_ORIGIN`. No pretend authentication is added.

**Prompt-injection boundary (plan v3.2 hotfix).** Every prompt states that all document and context text supplied to the model is untrusted source material and that instructions inside it must not be followed. Deterministic structured validation (schema, binding and domain checks) remains the primary security boundary; prompting is never claimed as a security control.

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #63] the Anthropic adapter's total request timeout is an explicit, validated, identity-bound adapter config value (with a separate connect timeout) whose default is sized for the configured max_tokens, and a timeout is a typed ProviderError distinct from transport failure.
- [Fable #64] every prompt in prompts/ states exactly the constraints of its task output schema and consumer (including 'a definition requires a non-null concept_kind' for s1-vocabulary), verified by a test that each prompt's documented output shape validates against its schema.
- [Fable #65] each inference output schema encodes the pilot contract its prompt states (no forbidden process node kinds, minLength 1 on timer_expr, no AUTO_DERIVATION in session-turn, constraint enums named in s1-classify-requirement), so schema validation rejects prompt-forbidden shapes.
- [Fable #66] the persisted inference artifact's parameters equal the effective request-body generation parameters actually sent (explicitly none beyond model/max_tokens if so documented), and golden-test 'anthropic' policies use only configuration the real adapter accepts.


### 33.9 Database and runtime-configuration boundaries (H0.9)

- **Opening existing projects.** Every non-init open uses an existing-database mode that never creates the PLUMB schema: a zero-byte file, a foreign SQLite database or a database without PLUMB tables is `E_DATABASE_NOT_INITIALIZED`. Only `init` (R0.1) creates a fresh project.
- **Read-only commands.** The read-only CLI commands are `vocab`, `questions`, `rounds show`, `export`, `report`, `proposals` and `findings list`. They open the database read-only (SQLite read-only open, no journal-mode change, no worker, no recovery or reconciliation writes) and succeed on a read-only database file.
- **Typed store errors.** Store failures map to typed errors instead of `E_INTERNAL`: branch not found is 404 `E_BRANCH_NOT_FOUND`; uninitialized database is 409 `E_DATABASE_NOT_INITIALIZED`; SQLite busy/locked is 503 `E_DATABASE_BUSY`; read-only database on a write is 409 `E_DATABASE_READ_ONLY`; detected corruption is 500 `E_STORE_CORRUPT`.
- **Eager configuration validation.** Building the application runtime validates every policy a later stage or session will use, before any paid inference: `duplicate_policy`, `vocabulary_policy`, `session_turn`, `provider_policy`, calendars, direct managers, expression scopes, `scenario_derivation` and `verification_applicability`. The CLI configuration rejects duplicate JSON object keys at any depth.
- **Identity.** `identity.actor_ref`, `stage_actor_ref` and `assistant_actor_ref` must be pairwise distinct whenever an application is opened with runtime configuration, not only at `init`.

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #3] A stored revision whose psg_schema_version differs from PSG_SCHEMA_VERSION fails with a typed unsupported-version store error (never E_STORE_CORRUPT), and any change to the serialized form of Node, Edge or NodePayload must bump PSG_SCHEMA_VERSION, enforced by a test pinning the canonical bytes of a fixture node of every payload variant.
- [Fable #199] The store durability profile is frozen and documented in the operations notes: the revision and workflow stores use synchronous=FULL (or NORMAL with an explicit documented power-loss window), and a test asserts the configured pragma.


### 33.10 Deferred performance architecture (POST.0)

The following are post-pilot optimizations, recorded under POST.0. They are not H0 blockers unless benchmark evidence shows customer-zero cannot complete. They never permit unbounded inputs (§33.1 stays in force):

- an immutable revision cache keyed by RevisionId;
- avoiding full graph deserialization, re-canonicalization and rehashing on every request;
- content-addressed reuse of node and edge storage instead of full JSON snapshots per revision;
- touched-element-only revision finalization;
- removing duplicate head verification under the write lock while preserving the CAS;
- a cached inverted BM25 index;
- pagination that limits before building a whole projection;
- large-document profiling and resource budgets.

**Pre-release trigger (plan v3.2 hotfix).** E0.4 runs a representative-scale benchmark reproducing the audit's measured shapes (a 50,000-paragraph Markdown import, a 10,000-node graph, 8 concurrent `GET /api/model/version` requests) plus customer-zero. Each CLI command must complete within 120 s, peak RSS must stay at or below 4 GiB, and the 8 concurrent version reads must complete within 4 s in total. If the benchmark fails, the corresponding item above becomes a pre-release blocker instead of accepted performance debt.

### 33.11 Semantic integrity and crash recovery (H0.10, plan v3.2 hotfix)

- **Artifact integrity.** Every artifact read recomputes the content hash from the stored bytes and requires it to equal the requested and stored key; a mismatch is typed store corruption (`E_STORE_CORRUPT` through the H0.9 mapping) and is never returned as a valid Artifact.
- **Plain stage commit reconciliation.** An automatic/plain stage child whose semantic revision was committed before a crash is settled on recovery to that revision (child Committed with its output ref, parent Committed with that result revision), found by walking from the head to the revision whose parent is the child base and whose `accepted_patch_ref` is the child's output PatchSet. This runs before any expired or ownerless in-flight attempt is interrupted; committed work is never marked Failed and never committed twice.
- **Session-turn revision binding.** Recovery of a prepared or committed session turn keeps the base revision its proposals were produced against and never rebinds them to the head current at read time.
- **Proposal occurrence identity.** The content-addressed semantic Proposal ID is unchanged. Workflow rows are keyed by `(proposal_id, base_revision_id)` with the durable occurrence key `occ:<first 16 hex of SHA-256(RFC 8785 {proposal_id, base_revision_id})>`. Historical occurrences are immutable. A lookup or action by proposal ID resolves exactly the pending occurrence whose `base_revision_id` is the loaded head; if there is none the existing `E_PROPOSAL_BASELINE_CHANGED` semantics apply. An older rejected, stale or resolved occurrence never blocks persisting and governing the identical proposal on a later baseline. `STORE_SCHEMA_VERSION` increases by exactly 1 with an in-place migration that keeps each existing row as its occurrence.
- **Scenario dependency invalidation.** A Scenario's dirty-dependency set contains the Accepted Invariants scoped to its footprint, its footprint Entities and Attributes, and the Calculation, DecisionTable, Operation and Transition dependencies already tracked. Editing any of them re-executes the scenario deterministically; a ScenarioRun for an older dependency state is reported stale, never current.
- **Graph-bound payload references.** Graph validation rejects a missing, wrong-type or forbidden-status graph-bound reference with a typed GraphViolation. A baseline-participating element's graph-bound reference targets a baseline-participating node (Accepted, Suspect, Superseded or Deprecated) except where the table names Agent (Accepted only); a Proposed element may also reference Proposed nodes; a Rejected target is never valid. External identifiers are never treated as graph references. Where the metamodel names a different target type than the table below, the metamodel wins and H0.10 stops with BLOCKED-SPEC-CONFLICT instead of guessing. Rows marked "existence" check only that the target exists (any node or edge); a Rejected owner is checked for existence and type only. `EvidenceFragment.source_ref` and a question-bound `ResolutionDecision.question_ref` keep their dedicated violations (`EvidenceSource*`, `DecisionQuestion*`). The census is `plumb_psg::payload_refs` and the violations are `GraphViolation::PayloadRefMissing`, `PayloadRefNotANode`, `PayloadRefWrongType` and `PayloadRefForbiddenStatus`.

| Payload field | Class | Expected target |
|---|---|---|
| `EvidenceFragment.source_ref` | graph | SourceArtifact |
| `Finding.affected_refs` | graph | any node or edge (existence) |
| `Finding.waiver_ref` | graph | ResolutionDecision |
| `Question.finding_ref` | graph | Finding |
| `Question.stakeholder_ref` | graph | Stakeholder |
| `Question.round_ref` | external | workflow round identifier |
| `Question.context_refs` | external | subject-first context; the subject may be a candidate ID that a compiler builds only after the answer (metamodel Hotfix 047) |
| `ResolutionDecision.question_ref` | graph | Question |
| `ResolutionDecision.proposal_ref` | external | workflow proposal identifier |
| `ResolutionDecision.decided_by` | graph | Agent (Accepted) |
| `ResolutionDecision.supersedes` | graph | ResolutionDecision |
| `Assumption.owner_ref` | graph | Agent or Stakeholder |
| `Assumption.finding_ref` | graph | Finding |
| `Assumption.risk_ref` | graph | node of the metamodel risk type (post-pilot; H0.7 adjudicates the rule) |
| `Need.stakeholder_refs` | graph | Stakeholder |
| `Need.goal_refs` | graph | Goal |
| `Requirement.owner_refs` | graph | Agent, Stakeholder or Actor |
| `Requirement.stakeholder_refs` | graph | Stakeholder |
| `AcceptanceCriterion.measure_ref` | graph | node of the metamodel measure type (existence) |
| `AcceptanceCriterion.scenario_ref` | graph | Scenario |
| `Term.definition_ref` | graph | Concept |
| `DomainRelationship.from_entity / to_entity` | graph | Entity |
| `Transition.stateful_ref` | graph | Entity |
| `Transition.from_state / to_state` | graph | State |
| `Transition.effect_refs` | graph | Event or Operation |
| `Invariant.scope_ref` | graph | Entity, Attribute, Process or Operation |
| `Operation.input_schema_ref / output_schema_ref` | external | schema document identifier (not a PSG node in the pilot) |
| `Event.payload_schema_ref` | external | schema document identifier (not a PSG node in the pilot) |
| `ProcessNode.process_ref` | graph | Process |
| `ProcessNode.operation_ref` | graph | Operation |
| `ProcessNode.message_ref` | graph | Event |
| `Calculation.calendar_ref` | graph | Calendar |
| `Scenario.requirement_refs` | graph | Requirement |
| `Scenario.derived_from_refs` | graph | any node |
| `ResourceScope.resource_ref` | graph | Operation, Entity or Attribute (the plan S2.8 Hotfix 042 pilot resource subset) |
| `SeparationConstraint.role_refs` | graph | BusinessRole or SecurityRole |
| `ScenarioRun.scenario_ref` | graph | Scenario |
| `EvidenceLocator.ConversationTurn.turn_id` | external | session turn identifier |
| `DerivationRecord.id, StandardsProfile.id` | self | the node's own identity |
| `all post-pilot payload references (QualityScenario, Architecture*, View, Viewpoint, Technology*, ApiOperation, Message, Capability, WorkPackage, TaskContract, Migration, TestExecution, TestReceipt, ArchitectureCheck, CoverageRecord)` | graph | existence only in the pilot; type contracts arrive with their post-pilot stages |

- **Numeric type and scale integrity.** `Decimal(n)` scale applies equally to scalar Decimal values and unit-bearing Quantity values and results. Calculation result checking follows the frozen order parse -> type -> evaluate -> round per declared rounding -> scale check -> unit check; a value violating its declared scale fails with a typed error with or without a unit, and a division followed by the declared business rounding can satisfy its declared scale.
- **Calendar coverage.** A Calendar declares coverage as a closed list of covered years, carried identically by the runtime calendar configuration and the PSG Calendar material. Business-day evaluation of a date outside coverage is the typed `EvalError::CalendarCoverageUnavailable` and makes dependent calculations and scenarios Undecidable, never an assumed working day. In `plumb-expr` a `CalendarDefinition` declares coverage with `with_coverage(years)`; a calendar without declared coverage answers every date (the behaviour before H0.10).
- **Scenario calculation result references.** `Then.calculation_results` entries and `result:<key>` semantic references resolve deterministically to the named Calculation's execution result in the scenario trace (the HR boundary scenarios require it); an unresolvable reference is a typed scenario validation error at derivation.
- **Supersession lifecycle.** A Supersede patch accepts only an Accepted, Suspect or Deprecated predecessor; superseding a Proposed or Rejected node is a typed PatchError and never promotes it into the baseline; re-superseding a Superseded target is the typed conflict `SupersedeAlreadyApplied`; a governed requirement-supersession decision that no longer applies yields a per-decision disposition instead of failing the duplicate batch.
- **Additional integrity adjudications.** EvidenceManifest validation cross-checks every fragment's `source_ref` against a manifest source. A session-generated Proposal's materiality and acceptance policy are recomputed from its actual semantic delta (a semantic delta is never labelled non-semantic). A persisted GateReport is current only when it contains a result for every rule of its gate in the active profile and its supplemental-input hash equals the current assembled inputs. Finding self-feeding and stale Open Findings are owned by H0.6 (§33.6).

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #1] WorkflowStore::resolve_proposal(Accepted) requires the resolution revision to be on the proposal's branch lineage with version > base_revision_id's version and returns a typed workflow error otherwise; every acceptance path either resolves the proposal in the revision-commit transaction or is reconciled on startup from the committed acceptance companion.
- [Fable #18] X1.3 intake computes Jaccard novelty for Scenario subjects from the scenario name and the string leaves of given/when/then only (no JSON keys or scenario_kind), and excludes Scenario duplicate candidates whose only shared terms occur in every Scenario document, under a RETRIEVAL_INDEX_VERSION bump.
- [Fable #20] Every importer normalises extracted text to Unicode NFC before building EvidenceFragments, so retrieval documents and Term keys never differ between NFC and NFD spellings of the same text.
- [Fable #21] An IntakeReport subject_novelty entry has nearest_ref Some exactly when its similarity is greater than 0, and IntakeReport validation rejects any other combination.
- [Fable #27] plumb_core::to_canonical_json and canonical_hash fail with CoreError::Canonicalization when any serialized object emits a duplicate member name, including stringified map keys, instead of keeping the first.
- [Fable #28] Canonicalisation rejects with CoreError::Canonicalization any integer value or integer map key outside the I-JSON range ±(2^53−1), and no hashed payload field may carry an integer that can legitimately exceed it.
- [Fable #29] plumb-core tests pin CoreError::Canonicalization for NaN/±Infinity, RawValue, non-string keys, duplicate keys and out-of-range integers, and the canonical property tests draw integers only from the I-JSON safe range and assert a parse round-trip.
- [Fable #35] Graph::new and Graph::validate report every cycle of each acyclic relation kind (each disjoint cycle exactly once), pinned by a test with two disjoint cycles of the same relation.
- [Fable #40] EvidenceLocator::validate rejects a TextRange whose start is greater than its end, so no evidence ID is minted for an inverted range.
- [Fable #46] The DOCX importer flattens only mc:Choice content of an mc:AlternateContent element (mc:Fallback only when no Choice is present), so text-box text is extracted exactly once.
- [Fable #47] The DOCX importer treats numPr with numId 0, or without numId, as removing numbering, including numbering inherited from the style chain, so such paragraphs are classified as Paragraph, not ListItem.
- [Fable #48] The deterministic segmentation fallback does not end a sentence at a period that follows a purely numeric leading ordinal (^\s*\d+[.)]\s+), and the HR DOCX fallback test asserts exactly one candidate per requirement fragment.
- [Fable #49] The Markdown importer treats indented (>= 2 spaces or tab) non-blank lines immediately following a list item as that item's continuation (GFM semantics), and the S0.1 contract is amended accordingly.
- [Fable #51] The Markdown and plain-text line classifiers accept an optional leading U+FEFF on the first line, so a BOM never changes its heading or list classification (offsets and text unchanged).
- [Fable #52] A Markdown table block starts only at a line whose successor is a delimiter row with the same cell count, and lines before it never join the table block.
- [Fable #53] The DOCX importer applies the same CRLF/CR-to-LF normalisation as Markdown and plain-text decoding before building records and fragments.
- [Fable #54] A DOCX table with only a header row yields a Paragraph fragment of its text (or a typed W_DOCX warning with partial parse status), never a silent omission.
- [Fable #55] The DOCX importer creates no EvidenceFragment for a record whose text is only whitespace; the text stays in the extracted artifact.
- [Fable #77] compare_to_baseline rejects a report whose sentence_count differs from the baseline corpus_sentence_count with a typed benchmark error.
- [Fable #85] compute_impact seeds edge endpoints only when the endpoint node is baseline-participating in the base or result graph, so DirtySet never contains a non-baseline node (test: demote + retarget in one patch).
- [Fable #88] Proposal.confidence is represented so its wire form and its canonical identity bytes are identical (f64 or fixed-point), or it is excluded from the identity projection, with a cross-language ID reproduction test.
- [Fable #94] prepare_turn performs every validation (context selection, budget, allowed refs, plan validation) before its first store write, so a rejected turn persists no artifact (test: store has no puts after ContextBudgetExceeded).
- [Fable #96] stage-job governed commit recording reports a structurally invalid decision node as a distinct 'decision_node' mismatch, separate from 'decision_status', in both the single and batch recorders.
- [Fable #97] planned and input artifact media types are validated with the require_text rule (non-empty, no surrounding whitespace or control characters) and a type/subtype shape.
- [Fable #109] S2.6 calculation name grounding rejects ranges with surrounding whitespace/control characters exactly like the state and domain producers, and identity never uses a trimmed name that differs from the persisted range.
- [Fable #139] The S1.2 EARS grounding catalog is built from an explicit per-payload-type allow-list of human-text fields (for example name, statement, definition, term, aliases, description) and never contains payload type tags, enum wire strings or Id values; a grounding to any other value is rejected.
- [Fable #143] Requirement modality extraction ignores modal verbs inside relative clauses (who/which/that) and raises an issue instead of silently choosing when a statement contains more than one distinct main-clause modal.
- [Fable #147] PlumbExpr date arithmetic whose result falls outside years 0000..=9999 fails with EvalError::DateOutOfRange in both directions, with a regression test for 0000-01-01 - 1 day.
- [Fable #148] round_decimal returns RoundingError::Overflow whenever the rounded value cannot carry exactly the requested scale, with tests at scale 28 (10.5) and scale 2 (Decimal::MAX).
- [Fable #149] PlumbExpr list type checking is order independent: an empty-list element without context is deferred and re-checked against the element type established by the other items, exactly as check_pair does.
- [Fable #150] PlumbExpr evaluation distinguishes an absent path from a present path that navigates through a non-Ref value: the latter is ValueTypeMismatch in both identifier lookup and exists(), never MissingValue or false.
- [Fable #156] The S2.5 numeric contract states that Decimal products and quotients are rounded to 28 fractional digits with the rust_decimal default strategy, and a test pins a product whose exact scale exceeds 28.
- [Fable #162] Decision-table Decimal output and default-output literals are scale-checked after normalization (trailing zeros removed), and numeric input interval bounds are checked against their column scale the same way.
- [Fable #195] Every gate evaluation persists its canonical GateReport bytes as an immutable content-addressed GateReport artifact; the (revision, gate) row is only the current index and never the sole record of an evaluation.
- [Fable #196] Every reconciliation of an interrupted acceptance resolves the workflow proposal with the revision whose decision_refs contain its acceptance decision (found by walking the branch), never the current head.
- [Fable #259] The s2-operation and s2-process inference output schemas use the same shared ID pattern as every other inference schema, and the affected request IDs and replay fixtures are regenerated under that change.
- [Fable #298] Every F1 GateReport binds a hash of the effective lint rule registry (rule IDs with default and overridden severities), so a changed lint default changes the report's input binding and currency, and UI tests use only rule IDs present in the profile or lint registry.
- [Fable #343] working_days semantics are frozen as the half-open period [start, end) when inclusive_end is false and [start, end] when true, and the architecture §calculation callables text and the plumb-expr table tests state exactly that.
- [Fable #368] Import artifacts are persisted in the same store transaction as the revision commit, or an unreferenced-artifact sweep reclaims artifacts no revision, job, proposal or turn references, never deleting a referenced one.

### 33.12 Product, API and audit-quality closure (H0.11, plan v3.2 hotfix)

- **Cursor recovery.** A list whose page-2+ cursor fails with `E_CURSOR_BASELINE_CHANGED` resets to its first page on the current baseline and refetches exactly once; a historical cursor is never retried.
- **Read errors.** Reads are retried automatically only on a network failure or HTTP 502, 503 or 504, at most 2 retries with backoff; every other status fails immediately and the UI shows the backend typed code and message.
- **Mutation transport failures.** A network or non-HTTP failure during a write is shown to the user with a typed transport-failure message; no mutation fails silently.
- **WebSocket lifecycle.** The DesignEvent stream reconnects with exponential backoff from 1 s doubling to a 30 s cap with jitter; after a reconnect or a lag close it invalidates the current model queries exactly once; unmounting stops reconnection.
- **Preview/discard lifecycle.** Preview endpoints never persist a pending WorkflowProposal; a proposal becomes durable only on submission. Answer and question flows that persist proposals expose an explicit discard that moves the pending occurrence to the new terminal state `abandoned`. Accepted, rejected and abandoned history is never deleted.
- **Locale reachability.** The UI offers English and Polish through a supported locale selection persisted per browser, defaulting to the browser language with English fallback for any missing key.
- **HTTP error envelope.** Every framework/extractor failure on the supported API boundary (400 malformed JSON, query or path; 404 unknown route; 405 method; 413 body limit; 415 media type) returns the typed `{code, message, details}` envelope. `crates/plumb-api/src/error_catalogue.rs` is the authoritative catalogue (code, HTTP status, meaning); a test proves every ApiError code used in the source is catalogued and documented in OpenAPI. No raw SQLite or Rust debug text appears in a product error.
- **F3 API reachability.** The HTTP product flow can evaluate F3 through the existing gate surface, and `GET /api/gates` reports each gate as `not_evaluated`, `current` or `stale`.
- **Collection robustness.** Scenario, DecisionTable, Question and Assumption lists apply cheap filters first and project each item independently; an item that fails projection appears with a typed health marker (code and element ID) and the rest are returned; the item's detail route fails strictly with its typed error. Corruption is never hidden.
- **UI workflow completeness.** EARS review, glossary confirmation and edit, proposal discovery and governance, search result dismissal and currentness, and provenance rendering work end to end; a control the backend deterministically rejects for the displayed lifecycle state is not offered.

**Audit closure (plan v3.2 hotfix).** The following Fable audit findings are closed by this task (see `docs/plan/FABLE-AUDIT-DISPOSITION-v3.md`):

- [Fable #144] glossary.md and model-report.md escape line-start block markers ('-', '+', '>', ordered-list numerals and setext underlines) or render multi-line semantic strings in fenced blocks, so every semantic string renders unchanged.
- [Fable #181] A run or rules recipe with no eligible targets either still evaluates its gate on the base revision or fails with a typed E_STAGE_SCOPE_EMPTY; it never reports success without evaluating its gate, and the CLI prints the outcome.
- [Fable #182] Stage plan and evaluation failures carry a non-empty details.error naming the typed error (never derived from the Debug form of a string), with a test asserting it for a phase plan failure.
- [Fable #187] The CLI provider-option check names the actually offending flag for every conflicting combination (e.g. --provider replay with --anthropic-api-key-file reports --anthropic-api-key-file), and cli tests assert each exact message.
- [Fable #202] plumb serve emits a tracing record per request (method, route, status, latency) and logs the underlying cause at every ApiError::internal site; ui/package.json gains dev and build scripts and the supported way to run the UI against plumb serve is documented and used by E0.3.
- [Fable #205] Every API error message or detail that echoes client-supplied text truncates it to at most 64 characters followed by an ellipsis through one shared helper.
- [Fable #207] A turn request for a session that already has a turn in flight is rejected with 409 E_SESSION_TURN_IN_FLIGHT before any workflow write or provider call, and a test with a blocking provider proves it.
- [Fable #218] Write controls that require a role the current identity lacks (per GET /api/me roles) are hidden or disabled with an explanation, and a test proves a non-analyst is not offered analyst actions.
- [Fable #221] Edit forms seeded from server data (e.g. SentenceReview slots) reset or show a conflict when the underlying server value changes, so a submitted patch always equals the visible change against the reviewed version.
- [Fable #224] Positional UI lists (scenario given/then rows, trace diffs, spans, lint findings, smells) are keyed by index or a stable unique ID, never by display text, with a test rendering two identical rows.
- [Fable #225] The router defines catalogue-backed not-found and error components, nested routes mark their nav section aria-current, and data tables have a caption and scoped headers.
- [Fable #226] Each H0.11 UI behaviour (cursor reset, transport failure, discard/abandon, socket close and reconnect, search dismissal, not-found) has a Vitest regression test that fails on the pre-fix behaviour.
- [Fable #235] HTTP tests cover a concurrent turn on one session with a blocking provider and each of E_SOURCE_CONFLICT, E_ROUND_INCONSISTENT, E_GATE_REPORT_INCOHERENT, E_SCENARIO_PROJECTION_INVALID and E_QUESTION_PROJECTION_INVALID from persisted data.
- [Fable #237] The /api/events forwarder reads the socket concurrently (answering Ping, detecting Close promptly) and closes a lagged subscriber with a distinct close code and reason meaning events were missed.
- [Fable #269] GET /api/search declares q as a required query parameter in OpenAPI and the generated client, matching the handler's 400 on an absent or blank q.
- [Fable #307] Every artifact written by report --out (including scenarios.yaml and gate-report.json) is exportable via export --format and GET /api/model/export, or the CLI help and OpenAPI ExportFormat state the restriction explicitly.
- [Fable #314] create_source returns 409 E_SOURCE_EXISTS when the existing SourceArtifact node's payload and extensions equal the freshly imported node's (audit metadata ignored) and E_SOURCE_CONFLICT only when they differ, with a regression test under two different clock instants.
- [Fable #321] Every closed-enum wire field in the OpenAPI contract (including ClassifyRequest.class and RequirementSummary.class) references its closed enum schema, and every importer rejects a source yielding zero EvidenceFragments with a typed error (HTTP 400 / CLI non-zero) before any artifact or revision is written.
- [Fable #350] GET /api/search either includes Proposed Requirement and Term nodes as hits labelled with their lifecycle status or the contract and UI state explicitly that only Accepted elements are searchable, with a test pinning the chosen behaviour.
- [Fable #352] The UI router renders a shell-integrated typed not-found page for unknown routes, and every routed page including /sessions has exactly one h1.
- [Fable #355] A failed session turn shows a turn-specific message carrying the backend typed code and message, and session creation is refused with a typed error (and the UI Start control disabled) when the configured provider mode is null.
