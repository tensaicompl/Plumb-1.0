# Plumb 1.0

Plumb is a **Software Specification Compiler** intended to compile enterprise software intent into a typed, traceable, executable specification and, progressively, into architecture, technical contracts, delivery work and proof.

Canonical engineering chain:

**Evidence → Intent → Function → Quality → Architecture → Contract → Work → Proof**

## Current repository state

This package is the audited **v3 foundation + F1–F4 pilot build contract** prepared for Claude Code CLI.

The authoritative implementation instructions are:

1. `CLAUDE.md`
2. `docs/plan/PLUMB-IMPLEMENTATION-PLAN-v3.md`
3. `docs/plan/PLUMB-IMPLEMENTATION-TASKS-v3.json`
4. `docs/plan/SOURCE-REFERENCE-INDEX.json`
5. `docs/plan/EXECUTION-SCOPE.json`
6. the checksummed architecture, standards, rule-pack, fixture and migration documents referenced by those files.

Do **not** treat files under `docs/research/` or `docs/context/` as implementation authority.

## Start

Read `START-HERE.md`.

The first Claude task is **P0.1 only**.

Before any implementation:

```bash
sha256sum -c docs/plan/SUPPORTING-DOCS.sha256
python3 scripts/verify-plan-contract.py
```

Both must pass.

## Product direction

The present build proves the foundation and the functional compiler through I0/F1/F2/F3/F4.

Later compiler stages already exist in the metamodel and architecture, but are outside the default NOW scope:

**Q1 → A1 → A2 → A3 → A4 → D1 → D2 → C1**

See `docs/product/PLUMB-VISION.md` for product context and `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md` for the target compiler architecture.
