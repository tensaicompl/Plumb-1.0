# Audit — Plumb Implementation Plan v3.0

**Audit date:** 2026-09-29  
**Verdict:** **PASS**  
**Open critical findings:** 0  
**Audit target:** Claude Code CLI implementation contract for `pilot-v3-foundation`

## 1. Audit conclusion

**PASS.** The plan is ready to hand to Claude Code CLI for the declared NOW scope.

This is an audit of the **implementation specification**, not of code that has not yet been built. It establishes that the plan does not leave an identified product/architecture choice to the executing agent. When an unforeseen missing or conflicting decision is encountered, the authorized behavior is to create a typed blocker and stop rather than guess.

## 2. Mechanical checks

| Check | Result | Evidence |
|---|---|---|
| Supporting-document checksum verification | PASS | sha256sum -c exits 0 |
| Machine contract verification | PASS | PLAN CONTRACT OK: 73 tasks, 133 source references, 133 validation rules |
| Task JSON/YAML mirror equality | PASS | parsed structures equal |
| Source-reference JSON/YAML mirror equality | PASS | parsed structures equal |
| Scope JSON/YAML mirror equality | PASS | parsed structures equal |
| Unique task IDs | PASS | 73 unique IDs |
| Dependency order valid | PASS | all dependencies exist and precede dependents |
| NOW/LATER dependency boundary valid | PASS | no NOW task depends on LATER |
| Exact commands on every NOW task | PASS | 72 NOW tasks |
| Tests on every NOW task | PASS | no empty test list |
| Acceptance criteria on every NOW task | PASS | no empty acceptance list |
| All task references machine-resolvable | PASS | 133 indexed references |
| No free-form task references remain | PASS | source_refs are IDs only |
| No implementation task writes checksummed authority | PASS | no protected write overlap |
| Plan task headings match manifest | PASS | 73 detailed task headings |
| Validation rule total | PASS | 133 rules |
| Validation gate counts | PASS | {'I0': 6, 'F1': 11, 'F2': 24, 'F3': 6, 'F4': 7, 'Q1': 8, 'A1': 9, 'A2': 9, 'A3': 14, 'A4': 9, 'D1': 9, 'D2': 10, 'C1': 11} |
| Validation rule IDs unique | PASS | 133 unique rule IDs |
| HR fixture counts exact | PASS | {'requirements': 32, 'entities': 9, 'operations': 13, 'processes': 6, 'invariants': 5, 'calculations': 2, 'rules': 3, 'events': 6, 'scenarios': 44, 'ambiguities': 3} |
| Toolchain and quality tools pinned | PASS | Rust 1.98.1 / Node 24.21.0 / npm 11.19.0 / cargo-llvm-cov 0.9.1 / cargo-mutants 27.1.0 |
| Core no-guess governance explicit | PASS | three core governance statements present verbatim |
| No unresolved fuzzy/placeholder wording in task contracts | PASS | none |

## 3. Audited inventory

- **73 tasks**: 72 NOW, 1 LATER boundary marker.
- **133 source references**, each resolved through the machine index.
- **133 validation rules** with exact gate counts.
- HR fixture: **32 requirements, 9 entities, 13 operations, 6 processes, 5 invariants, 2 calculations, 3 rules, 6 events, 44 scenarios, 3 deliberate ambiguities**.
- Supporting documents and immutable fixture inputs are checksum-protected.
- Every NOW task has a file write allowlist, command sequence, tests, acceptance criteria and supporting references.

## 4. Defects found and corrected during the audit

| ID | Finding | Correction |
|---|---|---|
| A-001 | Free-form supporting references | Replaced with 133 machine-resolvable SRCREF IDs plus JSON/YAML source-reference index. |
| A-002 | Empty per-task command lists | Every NOW task now contains an exact command sequence; UI package scripts are explicitly defined. |
| A-003 | Moving/unpinned execution environment | Pinned Rust 1.98.1, Node 24.21.0, npm 11.19.0, ubuntu-24.04, cargo-llvm-cov 0.9.1 and cargo-mutants 27.1.0. |
| A-004 | Vague Markdown/plain-text extraction boundary | Replaced with exact regex classifications, normalization and byte-offset semantics. |
| A-005 | OpenAPI example generation discretion | Replaced with exact fixture-selection and recursive schema-example rules plus blocker behavior. |
| A-006 | Pagination cursor described as opaque | Pinned exact RFC-8785 canonical JSON -> UTF-8 -> lowercase-hex encoding and stale-baseline response. |
| A-007 | Expert-answer path described as normal path | Pinned to S3.3 ResolutionDecision/SemanticPatch and F0.8 CAS commit. |
| A-008 | Human shorthand v2 references such as §7:M1.1 | Moved reference resolution into machine-verifiable locators. |
| A-009 | Core governance detectable only indirectly | Added explicit PSG/LLM/stale-async governance statements. |

## 5. Controls that prevent agent guessing

1. Explicit document precedence.
2. JSON machine manifest plus human-readable YAML mirror.
3. Machine-resolvable source-reference IDs.
4. Checksum-protected authority documents.
5. Per-task write allowlists.
6. One task / one commit.
7. Exact command and test sequences.
8. Exact pinned runtime/compiler environment.
9. `BLOCKED-SPEC-*`, `BLOCKED-DEPENDENCY`, `BLOCKED-ENVIRONMENT`, `BLOCKED-TEST-CONTRACT`, and `BLOCKED-SOURCE-DATA` stop paths.
10. PSG-only canonical semantics.
11. LLM inference isolated as persisted proposal artifacts.
12. SemanticPatch-only accepted mutation.
13. Intake and compare-and-swap before commit.
14. Stale async results cannot commit.
15. Immutable HR fixture oracle.
16. Deterministic gates; readiness cannot override blockers.

## 6. Residual limits of the claim

No prompt or plan can mathematically guarantee that a generative model will never make a mistake. The stronger and auditable claim made here is:

> Any implementation choice unsupported by the task contract or an authoritative referenced document is unauthorized; the executor is instructed and mechanically constrained to stop rather than invent it.

The audit also does not establish that all pinned third-party APIs will compile together; P0.2 is the explicit compile/dependency proof. If that fails, the authorized result is `BLOCKED-DEPENDENCY`, not package substitution.

External standards are treated as alignment/interchange mappings unless a dedicated conformance validator proves a stronger claim.

## 7. Start instruction

The first executable task is **P0.1 — Verify specification pack and execution scope**.

Claude Code should not start P0.2 until both commands succeed:

```bash
sha256sum -c docs/plan/SUPPORTING-DOCS.sha256
python3 scripts/verify-plan-contract.py
```
