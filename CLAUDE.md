# Plumb 1.0 — Claude Code Rules

This repository is governed by `docs/plan/PLUMB-IMPLEMENTATION-PLAN-v3.md`.

Before editing code:

1. Read the implementation plan completely.
2. Read `docs/plan/EXECUTION-SCOPE.yaml`.
3. Read `docs/plan/PLUMB-IMPLEMENTATION-TASKS-v3.json` (machine authority) and the YAML mirror.
4. Read `docs/plan/SOURCE-REFERENCE-INDEX.json` (machine authority) and the YAML mirror.
5. Run `sha256sum -c docs/plan/SUPPORTING-DOCS.sha256` and `python3 scripts/verify-plan-contract.py`.
6. Resolve the current task's `source_refs` IDs through the JSON source-reference index and read those exact local locators.
7. Execute one ready task only.

Hard rules:

- Never guess a missing product/architecture decision.
- Never invent a dependency, field, enum, relation, endpoint, rule, acceptance criterion or test expectation.
- Never edit supporting specification documents during an implementation task.
- Only modify the current task's write allowlist, `docs/plan/EXECUTION-STATE.yaml`, and a blocker file if blocked.
- One successful task = one exact commit message from the manifest.
- No network in tests.
- No live LLM in tests.
- LLM output is a persisted proposal artifact, never accepted truth.
- `PSG` is canonical. `functional.yaml` is a projection.
- All semantic writes are `SemanticPatch` + intake + CAS commit.
- Stale async proposals cannot commit.
- A failed blocker gate cannot be overridden by readiness.
- If specs are missing/conflicting, write `docs/blockers/<TASK_ID>.md`, mark task blocked, and stop.
- A `blocked` task returns to `pending` only after the blocking condition is corrected or a human-approved spec hotfix is applied. Keep the blocker file, verify the environment, set only that task to `pending`, rerun both preflight commands, and re-execute the task from the start; if it blocks again, update the existing blocker file and set it back to `blocked`. See plan §14 "Human-approved unblock procedure".
