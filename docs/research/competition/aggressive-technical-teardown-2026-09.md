# Plumb — Aggressive Technical Teardown vs. Closest Open-Source Prior Art

_Date: September 2026_

## Purpose

This document compares Plumb's target architecture against the strongest relevant open-source implementations at the level of engineering ideas and code structures, not README feature lists.

Primary comparison set:

- agent-spec — https://github.com/ZhangHanDong/agent-spec
- Kibi — https://github.com/Looted/kibi
- NASA FRET — https://github.com/NASA-SW-VnV/fret
- Specmate — https://github.com/qualicen/specmate
- Spec Builder — https://github.com/dshills/specBuilder

## Bottom line

Plumb's present v2 plan has stronger **functional-domain modeling ambition** than any one repository reviewed, but it is weaker in several areas that those repositories have already solved well:

- agent-spec: downstream task-contract boundary, planning DAG and provenance/replay discipline;
- Kibi: proof semantics, source-bound claim inventory, typed relationships and stale evidence handling;
- FRET: formal semantics and test generation based on executable/formal meaning rather than prose;
- Specmate: treating behavioral diagrams/models as test-generating artifacts;
- Spec Builder: immutable Q&A/snapshot mechanics and explicit compiler framing.

The correct response is not to copy any project wholesale. Plumb should adopt their strongest architectural invariants while keeping a richer software-domain model.

## 1. agent-spec

### What its implementation gets right

`RequirementClause` parsing is deliberately conservative. Normative clauses are extracted into explicit structures and coverage is based on explicit scenario attribution rather than textual similarity. That is an important anti-hallucination invariant.

Its `RequirementPlan` is an actual graph/DAG representation with typed edge kinds such as dependency, child, work-unit and satisfaction relationships. This is much stronger than an LLM-generated bullet list of implementation tasks.

Its `ClarificationQuestion` is a versioned machine-readable decision envelope. It includes source, diagnostic code, blocking status, question kind, candidate decisions, evidence, scenario identity and eventual answer. Crucially, candidate options are not fabricated by the deterministic layer when they cannot be grounded.

Its `TaskContract` provides a crisp agent handoff boundary: intent, must, must-not, decisions, allowed/forbidden changes, out-of-scope and completion scenarios.

Its provenance work records input corpus/configuration/output digests and supports replay/verification of deterministic compilation output.

### Where Plumb v2 is weaker

Plumb currently says `same inputs → byte-identical outputs` while allowing live LLM calls inside stages. That is conceptually weaker than treating inference artifacts as recorded compiler inputs.

Plumb's delivery boundary is underdeveloped. Exporting `functional.yaml` is useful, but an implementation agent ultimately needs a bounded contract more like agent-spec's `TaskContract`.

The current Plumb process derivation leans on requirement order, which can easily mistake document order for causality.

### What Plumb should borrow

1. A shared decision-envelope schema for all human stops.
2. A formal compilation manifest / inference record.
3. A delivery-stage `ImplementationSlice` or `TaskContract`.
4. Requirement/architecture-derived planning DAGs rather than prose task generation.
5. Explicit distinction between deterministic compiler output and probabilistic AI proposal generation.

### Where Plumb should remain different

agent-spec's requirements structures are comparatively thin. Plumb should not reduce itself to normative clauses + scenarios + code links. Its differentiation is the richer domain semantics that exist before any code exists.

## 2. Kibi

### What its implementation gets right

Kibi has one of the most rigorous open-source proof models reviewed.

Its requirement proof ladder explicitly checks:

- semantic inventory,
- logic grounding,
- contradiction status,
- scenarios,
- scenario tests,
- passing end-to-end evidence,
- executable test symbols,
- production symbols,
- source coordinates.

This is a major strength: "covered" is not equated with "some link exists."

Its semantic inventory stores individual claim keys, claim text, source spans and states such as modeled, ambiguous, ontology gap, nonlogical and missing. This is substantially stronger than attaching one coarse provenance flag to a requirement node.

Its graph uses a controlled set of typed relationships including `specified_by`, `verified_by`, `validates`, `implements`, `covered_by` and `executable_for`.

Its proof receipts bind execution evidence to code snapshot/environment/test contract and treat stale/mismatched/failed evidence as an explicit gap.

### Where Plumb v2 is weaker

`Origin { Deterministic, Llm, Human, Recovered }` is too shallow for the product ambition. It states producer category, not derivation evidence.

Plumb lacks a conservative proof ladder. F4 is currently a gate but not yet a staged, inspectable proof object explaining why each requirement is or is not justified.

Plumb's post-export lifecycle does not yet model evidence freshness or code drift.

### What Plumb should borrow

1. Claim-level semantic inventory and source coordinates.
2. Typed relationship vocabulary rather than arbitrary strings everywhere.
3. Inspectable proof stages with typed gap codes and repairs.
4. Snapshot-bound test receipts and stale-proof detection.
5. Explicit `ambiguous`, `ontology_gap`, `missing` rather than optimistic inferred completeness.

### Where Plumb should remain different

Kibi centers on requirement↔scenario↔test↔code enforcement. Plumb must preserve a much richer semantic ontology: actors, roles, permissions, entities, calculations, decision tables, processes, state machines, quality scenarios, architecture, APIs and deployment.

## 3. NASA FRET

### What its implementation gets right

FRET operationalizes formal semantics. A requirement is not just stored as text; it is parsed into a controlled semantic representation and translated to temporal logic. Simulation, realizability analysis and test generation are downstream of that formal meaning.

Its test-generation engine creates formal obligations from the requirement semantics and solves those obligations using model-checking engines. This is far more rigorous than `requirement text → LLM test cases`.

FRET also demonstrates architectural mapping: requirement propositions can be mapped to model/system signals for downstream formal analysis.

### Where Plumb v2 is weaker

PlumbExpr and the scenario interpreter are promising, but current semantics are less formally specified. The model risks becoming "structured YAML interpreted by implementation convention" rather than a documented executable semantics.

The process/state/rule semantics need explicit execution contracts if they are to become the basis of proof.

### What Plumb should borrow

1. A documented operational semantics for every executable construct.
2. Tests generated from semantic coverage obligations, not directly from prose.
3. Clear distinction between simulation, consistency and realizability-like checks.
4. Formal mapping from requirements to model variables/elements.

### Where Plumb should remain different

Plumb should not force business users into a formal authoring language. It should compile messy existing requirements into richer semantics and expose questions only where safe compilation is impossible.

## 4. Specmate

### What its implementation gets right

Specmate's test generator runs against Cause-Effect Graph (`CEGModel`) or Process models. Test generation is therefore model-derived.

Its implementation validates and manipulates concrete model objects rather than merely rendering diagrams.

### What Plumb should borrow

1. Treat process/decision/state diagrams as executable model projections.
2. Derive test obligations from model structures such as branches, cause/effect combinations and boundaries.
3. Keep diagram editing connected to semantic objects, not image geometry.

### Where Plumb should remain different

Specmate is narrow by design. Plumb must integrate process, domain, role, rule, quality and architecture semantics in one graph.

## 5. Spec Builder

### What its implementation gets right

Spec Builder uses a clear compiler pipeline:

Planner → Asker → Compiler → Validator.

It has immutable/versioned answers, append-only snapshots, trace coverage and structured export.

### Weaknesses

Its model is primarily a project implementation spec rather than an executable domain model. The repository itself exposes unresolved design questions around LLM determinism and trace granularity.

### What Plumb should borrow

- append-only semantic snapshots,
- explicit answer supersession,
- compiler-oriented UX language,
- trace coverage invariants.

## Plumb v2 architecture issues exposed by comparison

### Generic node/edge payloads are too weak

Current v2:

`Node { kind: String, props: BTreeMap<String, Value> }`

This is useful persistence infrastructure but insufficient as the public semantic contract. Plumb needs typed semantic payloads and registered relationship types.

### `Patch` as trait object is the wrong durable format

`Box<dyn Patch>` is elegant runtime polymorphism but poor long-term serialization/audit state. Patches should become a serializable semantic change language (`AddNode`, `SetProperty`, `AddRelationship`, etc.).

### LLM stage purity is overstated

A stage that performs remote LLM inference is not a referentially pure compiler stage. Separate:

1. inference/proposal acquisition,
2. deterministic schema validation,
3. deterministic semantic compilation/application.

### Provenance is too coarse

Every derived assertion should be able to answer:

- which exact source spans contributed,
- which inference record/rule produced it,
- which prompt/schema/model version was used,
- which human decision accepted/modified it,
- what it superseded.

### Functional/technical separation is missing

The next architecture must never transform `functional.yaml` directly into a single "design" artifact. Multiple technical architectures may satisfy one functional model. Functions need explicit `allocated_to`/`satisfied_by` relationships into candidate technical models.

### Diagrams are underspecified

Current process swimlanes and read-only ER views are not enough. Plumb needs a semantic view system where diagrams are queries/projections over the graph and diagram edits emit semantic patches.

## Recommended architectural synthesis

### Compiler 1 — Requirements/Functional

Sources → claims → requirements → functional domain model → findings → decisions → executable scenarios.

Borrow:
- Kibi semantic inventory,
- agent-spec decision envelopes,
- FRET semantic rigor,
- Specmate model-derived verification.

### Compiler 2 — Architecture

Functional model + quality scenarios + imposed constraints → architecture drivers → candidate models → trade-offs → decisions → accepted technical model.

Use explicit function-to-component allocation.

### Compiler 3 — Delivery

Accepted functional + technical model → implementation slices → dependency DAG → task contracts → verification obligations.

Borrow agent-spec's contract boundary but enrich it with architecture scope.

### Continuous conformance

Code/test evidence → proof receipts → coverage/proof status → stale evidence/drift findings.

Borrow Kibi's snapshot-bound evidence semantics.

## Final technical judgment

Plumb should not attempt to beat these projects by implementing more AI features. It should beat them by establishing a stronger semantic spine:

**source-bound claims → typed software semantics → explicit decisions → executable behavior → allocated architecture → bounded delivery contracts → fresh proof evidence.**

That sequence is the product.
