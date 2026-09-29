# Live Build Review Protocol

This document describes how the product owner can have ChatGPT independently review Claude Code's implementation as the build progresses.

It is **review workflow guidance**, not an implementation authority.

## Preferred review unit

Review one completed task commit at a time.

For each review, provide:

1. the completed task ID, e.g. `F0.4`;
2. the Git commit SHA or updated repository access;
3. Claude's task summary and test output if available.

## Review procedure

The reviewer should verify the implementation against:

- `docs/plan/PLUMB-IMPLEMENTATION-TASKS-v3.json`;
- the task's `source_refs` resolved through `docs/plan/SOURCE-REFERENCE-INDEX.json`;
- its exact write allowlist;
- task commands/tests/acceptance criteria;
- cross-cutting `CLAUDE.md` rules;
- the metamodel/compiler/rule-pack authority applicable to the task.

The review should check at minimum:

### Scope integrity
- no files outside the write allowlist were changed;
- no protected supporting document or fixture input was modified;
- no unapproved dependency/version was introduced.

### Semantic correctness
- code matches the referenced metamodel types and relation semantics;
- no `kind + arbitrary props` shortcut replaces typed PSG semantics;
- all accepted semantic writes use `SemanticPatch`;
- provenance/inference rules are preserved;
- AI does not mutate accepted state directly;
- stale revision/CAS behavior is respected.

### Test integrity
- all task-specified tests pass;
- tests have not been weakened or rewritten to match incorrect implementation;
- fixture oracle inputs remain unchanged;
- no hidden live network/model dependency was introduced into tests.

### Determinism
- stable IDs/hashes/order follow the plan;
- repeated deterministic paths produce equal output;
- timestamps/layout/operational data do not contaminate semantic hashing where forbidden.

### Quality / security
- no placeholder/TODO implementation is accepted;
- error handling matches specified failure semantics;
- version conflicts do not silently retry writes;
- role/permission paths respect server authority.

## Review verdict

Use one of:

- `PASS` — task conforms; proceed.
- `PASS WITH NON-BLOCKING FINDINGS` — implementation conforms but has maintainability/documentation issues.
- `FAIL` — task violates the build contract; fix before proceeding.
- `BLOCKED-SPEC` — implementation exposed an actual specification gap; do not guess.

## Milestone reviews

In addition to per-task reviews, perform full reviews after:

- Foundation `F0.*`
- Validation `V0.*`
- Evidence `S0.*`
- Requirements `S1.*`
- Functional semantics `S2.*`
- Resolution `S3.*`
- Functional proof `S4.*`
- API/CLI `A0.*`
- UI `U*`
- E2E `E0.*`

The final E2E review should rerun the plan verifier, all repository tests, mutation/coverage gates, and compare produced artifacts with the accepted fixture/gate contracts.
