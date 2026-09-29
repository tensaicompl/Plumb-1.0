# Plumb v2 → v3 Implementation Delta

**Purpose:** reconcile the existing Functional Modeller v2 implementation plan with the v3 metamodel, validation profile and compiler architecture without throwing away the useful pilot design.

---

## 1. Executive decision

Do **not** discard the v2 plan.

Approximately half of its hard engineering work remains directly useful:

- Rust core
- SQLite pilot storage
- source blob hashing
- Clock abstraction
- deterministic collections/hashing
- PlumbExpr
- gap/inconsistency rules
- question engine
- decisions/assumptions
- scenario interpreter and `Undecidable`
- intake-before-write
- OpenAPI-generated API contract
- UI generated from API
- fixtures/property/mutation testing

The change is architectural:

> v2 is a functional modeller with a generic graph.  
> v3 is a typed software-specification compiler whose first implemented vertical slice is the functional modeller.

That distinction must be reflected in the foundation before more code is built around v2 assumptions.

---

## 2. KEEP — preserve with minimal semantic change

### Rust + deterministic engineering

Keep:

- Rust stable
- explicit sorting/BTree structures
- canonical hashing discipline
- injected `Clock`
- seeded RNG
- no network in tests
- strong property tests
- mutation testing on semantic rules

### SQLite pilot

Keep SQLite.

Change only the store semantics:

```text
mutable current Graph
```

becomes:

```text
immutable GraphRevision + branch head pointers
```

### Source/blob storage

Keep content-addressed blobs.

Upgrade `Source + Span` into:

```text
SourceArtifact
EvidenceFragment
```

without losing the existing offset semantics.

### PlumbExpr

Keep almost entirely.

It remains one of the strongest differentiating assets.

### Gap / inconsistency registry

Keep the registry concept.

Move it under the generic validation engine and assign each rule:

```text
gate
class
severity
applicability
waiver policy
standard mapping
```

### Question engine

Keep:

- typed questions
- stakeholder routing
- rounds
- deterministic ordering
- blast radius

Extend question kinds later for:

- quality threshold
- architecture choice
- technology selection
- interface choice
- verification method

### Functional interpreter

Keep:

```text
run(model, scenario) -> Pass | Fail | Undecidable
```

The model adapter changes from v2 `functional.yaml` structures to PSG functional semantics.

### Intake gate

Keep and make more central.

Every mutation channel should still use one intake path.

### API/UI contract discipline

Keep:

- Rust-generated OpenAPI
- committed API description
- generated TS client
- contract drift CI
- `If-Match`

---

## 3. CHANGE NOW — foundation corrections before more implementation

### 3.1 Replace generic core semantic nodes

v2:

```rust
Node {
  kind: String,
  props: BTreeMap<String, Value>,
  origin: Origin,
  ...
}
```

v3:

```rust
Node {
  payload: NodePayload,
  evidence: Vec<EvidenceRef>,
  derivations: Vec<DerivationRef>,
  standards: Vec<StandardMapping>,
  ...
}
```

The physical graph can remain generic internally, but all core product semantics must validate through the typed layer.

### 3.2 Replace `Origin` as provenance

Do not delete the idea; demote it to a convenience label.

Add:

```text
DerivationRecord
InferenceRequest
InferenceArtifact
Agent
```

An LLM contribution must be replayable from stored artifacts.

### 3.3 Replace `Box<dyn Patch>`

This is the biggest persistence problem in the current design.

Use serializable:

```rust
SemanticPatch
```

with tagged operations.

All UI/chat/import/diagram/AI edits converge on the same AST.

### 3.4 Make `functional.yaml` a projection

Do not remove it.

It remains an important integration artifact and compatibility format.

But the authoritative state becomes PSG.

### 3.5 Split live inference from pure stages

Current v2 says stages are pure while also allowing LLM work inside stages.

Replace with:

```text
plan()       pure
acquire()    impure orchestration
evaluate()   pure
commit()     transactional
```

### 3.6 Add compare-and-swap to async stages

Every job captures its base revision/hash.

A result computed on revision 40 cannot silently apply to revision 47.

### 3.7 Add immutable graph revisions

Current `GraphStore` needs real revision semantics and restore capability.

Recommended:

```rust
load_revision(id)
head(branch)
commit(branch, expected_head, patch)
create_branch(from)
move_head(...)
```

### 3.8 Separate business and security roles

v2 `roles: [{actor, operations[]}]` conflates two concerns.

Introduce:

```text
BusinessRole
SecurityRole
Permission
ResourceScope
PolicyCondition
```

The pilot UI can still display a simple role/operation matrix.

### 3.9 Replace canonical NFR bags

Do not let:

```yaml
nfr:
  consistency:
  availability:
  ...
```

become the canonical long-term model.

Add:

```text
QualityCharacteristic
Measure
QualityScenario
```

A compatibility projection may still emit a simplified NFR block for the old designer.

### 3.10 Fix F4 semantics

Remove:

> every requirement must have >=1 executed-pass-confirmed scenario

Replace with:

- functional/executable obligations require scenario/interpreter coverage where applicable
- all verifiable requirements later receive typed `VerificationObligation`s
- verification can be test/scenario/analysis/inspection/review/etc.

### 3.11 Stop treating requirement order as process truth

Requirement order may be weak evidence.

It must not be a deterministic source of process sequence.

If used:

```text
requirement-order candidate
 -> low-confidence proposal
 -> validation/human confirmation
```

### 3.12 Make readiness non-authoritative

Keep readiness scores for UX prioritization.

Do not use them as proof.

Gates are deterministic proof obligations.

---

## 4. ADD NOW — small foundational capabilities

These are cheap now and expensive later.

### Standards/profile registry

Add:

```text
StandardsProfile
StandardMapping
ResolvedProfile
RulePack
```

### Rule result model

```text
PASS
FAIL
WARN
NOT_APPLICABLE
WAIVED
ERROR
```

### Semantic hash vs view hash

Separate:

```text
semantic_hash
evidence_hash
view_hash
```

### Artifact store

Needed for:

- LLM outputs
- external validation
- projections
- traces
- future proof receipts

### Impact seed graph

You do not need the full future impact product now.

But every semantic patch should already identify changed nodes/edges and support reachability indexes.

---

## 5. DEFER — do not explode pilot scope

Do not attempt in the current functional pilot:

- full architecture generation
- full BPMN semantics
- DMN XML interoperability
- SysML v2 API integration
- ArchiMate/TOGAF integration
- code binding
- CI proof receipts
- conformance/staleness UI
- automated implementation slicing
- multi-cloud deployment model
- formal certification packs

Design the foundation so these are additive.

---

## 6. Crate-by-crate delta

### `plumb-core`

v2 responsibility is too broad.

Near-term keep name, but move toward:

```text
ids
hashes
canonical JSON
Clock
artifact refs
base errors
```

### New `plumb-psg`

Add now.

Owns:

```text
NodePayload
RelationKind
Graph
semantic registry
typed relation validation
standards mappings
```

### New `plumb-patch`

Add now or inside `plumb-psg` initially.

Owns:

```text
SemanticPatch
GraphDelta
diff
CAS preconditions
```

### `plumb-model-core`

Do not let this remain the long-term "everything" crate.

Short-term it can host functional semantic payload implementations.

Eventually split validation/compiler/projection.

### `plumb-model-import`

Keep.

Upgrade outputs to `SourceArtifact + EvidenceFragment`.

### `plumb-model-lint`

Keep.

Port rule metadata to validation profile.

### `plumb-expr`

Keep.

### `plumb-model-sim`

Keep.

Rename only if useful later.

### `plumb-model-intake`

Keep.

Make input a `Proposal<SemanticPatch>` rather than area-specific payloads.

### `plumb-model-session`

Keep.

Chat can only create proposals, never direct writes.

### New `plumb-validation`

Recommended now.

Owns:

```text
ValidationRule
GateEvaluator
RuleResult
WaiverPolicy
profiles/rule packs
```

### New `plumb-artifacts` or store module

Recommended now if `plumb-core` would otherwise become bloated.

Owns content-addressed compiler artifacts.

### `plumb-model-api`

Keep.

API objects should expose PSG-backed resources while preserving current functional endpoints for the pilot.

---

## 7. Revised v3-compatible pilot sequence

Do not expand the pilot to A1-C1.

Instead make the existing F1-F4 pilot run on the final foundation.

### Foundation sprint

Before continuing existing M-phases:

```text
V3.0 typed PSG envelope
V3.1 SemanticPatch AST
V3.2 GraphRevision / branch head / CAS
V3.3 DerivationRecord + InferenceArtifact
V3.4 StandardsProfile + validation engine shell
V3.5 functional.yaml projection adapter
```

This should be treated as a prerequisite, not a second product.

### Then reuse v2 sequence

```text
Evidence/import
Requirements/lint
Vocabulary
Domain model
PlumbExpr
Rules/calculations/operations
Questions/decisions
Processes/scenarios
Interpreter
F1-F4 rule packs
API/CLI/UI
Intake/chat
e2e
```

---

## 8. Revised stage mapping from v2

| v2 phase | v3 destination | action |
|---|---|---|
| M0 core | PSG/store/compiler foundation | MODIFY |
| M1 import | S0/S1 | KEEP + rename semantics |
| M2 lint | S1 validation | KEEP |
| M3 vocabulary | S1/S2 | KEEP |
| M4 entities | S2 | KEEP |
| M5 PlumbExpr | S2/S4 | KEEP |
| M6 rules/calcs/ops | S2 | KEEP |
| M7 questions | S3 | KEEP |
| M8 processes/scenarios | S2/S4 | MODIFY process inference |
| M9 interpreter | S4 | KEEP |
| M10 gates/exports | validation/projection | REBUILD around profiles |
| M11 API/CLI | platform | KEEP + extend |
| M12 LLM provider | artifact acquisition | MODIFY boundary |
| M13 intake | cross-cutting | KEEP + patch-centric |
| M14 sessions | cross-cutting | KEEP |
| M15 readiness | UX metric only | DEPRIORITIZE |
| M16 generic testgen | S11 later | REPLACE with obligation-driven verification |

---

## 9. Specific v2 requirements to delete or rewrite

### Delete as normative statement

```text
same inputs -> byte-identical outputs
```

if "inputs" includes a live LLM call.

Rewrite:

> Same accepted graph revision, compiler version, profile/rule-pack versions, configuration, human decisions and persisted inference/validation artifacts produce byte-identical semantic outputs and projections.

### Rewrite

```text
Stage functions are pure fn stage(&Graph,&Config)->StageOutput
```

to:

```text
plan(graph, context) -> StagePlan          // pure
evaluate(graph, context, artifacts)        // pure
```

Artifact acquisition is external.

### Rewrite

```text
Decision { patch: Box<dyn Patch> }
```

to:

```text
ResolutionDecision { patch_ref: ArtifactRef }
SemanticPatch
```

### Rewrite

```text
Origin { Deterministic, Llm, Human, Recovered }
```

as optional summary plus full `DerivationRecord`.

### Rewrite F4

Remove universal scenario requirement and replace with applicability-aware verification.

### Rewrite processes

Do not infer sequence from requirement document order except as explicitly weak proposal evidence.

---

## 10. API compatibility strategy

Do not force the UI to understand the entire v3 graph immediately.

Keep domain APIs:

```text
/api/requirements
/api/model/entities
/api/processes
/api/scenarios
/api/questions
...
```

Their backing implementation reads/writes typed PSG.

Add generic engineering APIs gradually:

```text
GET /api/baselines
GET /api/model/diff
GET /api/gates/{gate}
GET /api/provenance/{id}
GET /api/impact/{id}

POST /api/proposals/{id}/accept
POST /api/proposals/{id}/reject
```

Later:

```text
/api/architecture
/api/contracts
/api/delivery
/api/verification
/api/conformance
```

The UI should remain task-oriented, not a generic graph editor.

---

## 11. Data migration from an existing v2 graph

If v2 implementation already exists, use a one-way migration compiler:

```text
v2 Node(kind,props)
    -> infer registered v3 NodePayload type
    -> create migration DerivationRecord
    -> map edges to RelationKind
    -> preserve v2 IDs where compatible
    -> create aliases for changed IDs
    -> emit migration findings for untyped/ambiguous data
```

Never silently drop unknown v2 properties.

Store them under a namespaced migration extension until explicitly resolved.

`functional.yaml` v2 remains importable/exportable as a compatibility format.

---

## 12. Pilot success criteria after v3 correction

The HR pilot remains valid, but "done" should additionally require:

1. Accepted functional state is PSG-backed.
2. `functional.yaml` is generated from PSG and validates.
3. Every LLM semantic proposal has a persisted inference artifact.
4. Replay uses stored inference artifacts and reproduces identical semantic output.
5. Every accepted semantic edit is a serialized `SemanticPatch`.
6. Async stage result cannot apply to a changed baseline without re-evaluation.
7. F1-F4 are evaluated through the new validation registry.
8. Gate status cannot be overridden by readiness percentage.
9. BusinessRole/SecurityRole are distinct even if the fixture uses simple role mappings.
10. Semantic hash is unchanged by UI layout edits.

---

## 13. Recommended implementation decision

**Freeze the metamodel and compiler contracts now.**

Then continue the functional pilot.

Do not attempt to implement the entire broader Plumb vision in the pilot.

This gives the best combination:

```text
short-term:
  usable executable functional modeller

without creating:
  long-term architectural debt that makes architecture/delivery/proof a rewrite
```

That is the key v2 → v3 move.
