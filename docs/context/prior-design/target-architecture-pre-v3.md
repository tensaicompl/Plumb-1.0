# Plumb 1.0 — Target Architecture

## Architectural thesis

Plumb should be implemented as three connected compilers over one versioned semantic specification graph.

### Compiler 1 — Intent / Functional

Inputs:
- Word/Markdown/text/PDF requirements,
- conversations,
- existing process/architecture diagrams,
- legacy requirements tools,
- policies and standards.

Outputs:
- source-bound semantic claims,
- requirements,
- glossary,
- actors/roles,
- entities/states,
- operations,
- processes,
- rules/calculations,
- functional findings/questions/decisions,
- executable scenarios.

### Compiler 2 — Architecture

Inputs:
- accepted functional graph,
- measurable quality scenarios,
- technical/security/data constraints,
- project technology policy.

Outputs:
- architecture drivers,
- candidate architecture branches,
- trade-off findings,
- ADRs / technology selections,
- accepted software systems/containers/components,
- function-to-component allocations,
- interfaces, event channels, data ownership,
- deployment model.

### Compiler 3 — Delivery

Inputs:
- accepted functional + technical graph.

Outputs:
- implementation slices,
- dependency DAG,
- bounded task contracts,
- test/verification obligations,
- agent/IDE context bundles.

### Continuous conformance

Implementation and test evidence flows back into the graph:
- code bindings,
- test receipts,
- architecture checks,
- coverage/proof status,
- stale evidence and drift findings.

## Core architecture invariant

**The graph is truth. Documents and diagrams are views.**

A diagram edit must generate a semantic patch. A Markdown/HTML report must be a render. Layout metadata is separate from model semantics and must not affect semantic hashes.

## Functional vs. technical truth

Functional constructs are never rewritten into architecture constructs.

Example:

`op:ApproveLeave`

may be allocated to:
- `cmp:LeaveModule` in architecture candidate A,
- `ctr:LeaveService` in candidate B.

Both can satisfy the same functional requirement. This allows architecture alternatives and future redesign without corrupting requirement history.

## Runtime architecture recommendation

Short term:
- retain Rust core and SQLite for local/pilot operation,
- preserve `LlmProvider`, `EmbeddingProvider`, `Clock` and storage abstractions,
- use typed domain payloads over generic graph storage,
- keep deterministic compiler logic independent of HTTP/UI.

Longer term:
- persistent event/decision history,
- pluggable repository/storage backend,
- enterprise auth and multi-user collaboration,
- read-oriented MCP/API surface for agents,
- optional server deployment with PostgreSQL/object storage.

## Inference boundary

LLM inference is not part of deterministic compilation.

Every inference that may affect semantic state should produce an immutable `InferenceRecord` containing at least:

- source/input hashes,
- stage,
- prompt-template hash,
- output schema hash,
- provider/model,
- model parameters,
- context hash,
- raw response digest,
- validated structured output,
- timestamp.

The deterministic compiler consumes validated inference artifacts plus accepted human decisions.

## Mutation boundary

Replace durable trait-object patches with a serializable semantic mutation language:

- `AddNode`,
- `RemoveNode`,
- `SetProperty`,
- `UnsetProperty`,
- `AddRelationship`,
- `RemoveRelationship`,
- `SetStatus`,
- `AddAlias`,
- `SupersedeDecision`,
- compound transaction.

Every change source—UI, chat, import, API, diagram editor—must produce the same mutation representation and pass through the same intake/conflict checks.

## View architecture

First-class view types should include:

- stakeholder/system context,
- RBAC matrix/graph,
- process/swimlane,
- decision model,
- domain/ER,
- state machine,
- functional scenario sequence,
- C4 system context,
- C4 container,
- optional C4 component,
- API/event interaction,
- technical dynamic/sequence,
- deployment,
- trace/impact graph.

Each `ViewDefinition` is a query/projection over the canonical graph plus independent layout metadata.
