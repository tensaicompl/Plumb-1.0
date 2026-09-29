# Plumb Plan Hotfix 001

Purpose: correct two build-contract inconsistencies found by Claude during P0.1.

Changes:
1. Add `.node-version` to implementation plan §9 controlled path list. It was already correctly present in P0.2's task write allowlist.
2. Add `crates/plumb-functional/src/vocabulary_exceptions.rs` to S1.5's write allowlist and to plan §9.
3. Fix cosmetic ordered-list numbering in `CLAUDE.md`.
4. Regenerate `docs/plan/SUPPORTING-DOCS.sha256`.

This hotfix does NOT modify `docs/plan/EXECUTION-STATE.yaml`; P0.1 remains done.

After applying:
```bash
sha256sum -c docs/plan/SUPPORTING-DOCS.sha256
python3 scripts/verify-plan-contract.py
```

Expected:
`PLAN CONTRACT OK: 73 tasks, 133 source references, 133 validation rules`

Then commit the hotfix as a human-approved plan correction before P0.2.
