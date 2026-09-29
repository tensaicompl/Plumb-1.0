# START HERE — Claude Code CLI

This repository contains an audited build contract. Do not ask Claude to "build Plumb" generically.

## First invocation

Start Claude Code in the repository root and give it exactly this instruction:

> Read `CLAUDE.md` completely. Then execute **P0.1 only** from the Plumb v3 implementation plan. Do not start P0.2. If anything fails or conflicts, follow the blocker protocol and stop.

P0.1 must run:

```bash
sha256sum -c docs/plan/SUPPORTING-DOCS.sha256
python3 scripts/verify-plan-contract.py
```

Expected verifier result:

```text
PLAN CONTRACT OK: 73 tasks, 133 source references, 133 validation rules
```

## After P0.1

For controlled execution, use:

> Execute the next single ready NOW task exactly as defined by the machine manifest and implementation plan. Read all referenced sources first. Respect the write allowlist. Run every specified command/test. Commit only after every acceptance condition passes. Stop after the task.

If you intentionally want Claude to run several tasks, say so explicitly; otherwise the plan requires one task at a time.

## Never tell Claude to

- improvise missing design decisions;
- modernize dependencies beyond the pinned versions;
- simplify the metamodel;
- replace specified libraries;
- edit fixture inputs to make tests pass;
- reinterpret a failed gate as acceptable;
- bypass a blocker;
- treat research/context documents as build authority.

## When Claude blocks

Do not ask it to "just choose something".

Give the blocker to the product owner/reviewer. A missing decision should become an explicit plan/specification change, then checksums/manifests must be regenerated deliberately.

## Build review

See `docs/verification/LIVE-BUILD-REVIEW-PROTOCOL.md`.

After Claude completes a task, the safest review unit is the **task commit**.
