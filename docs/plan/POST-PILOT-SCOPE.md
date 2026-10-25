# Post-pilot scope (POST.0)

This file marks the boundary of the `pilot-v3-foundation` release. Nothing listed here is
implemented in NOW scope.

## Compiler stages and gates

| Stage | Architecture | Gate(s) |
|---|---|---|
| S5 Quality Compiler | `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md` §11 | Q1 |
| S6 Architecture Driver Compiler | §12 | A1 |
| S7 Architecture Allocation Compiler | §13 | A2 |
| S8 Technical Contract Compiler | §14 | A3 |
| S9 Architecture Justification Compiler | §15 | A4 |
| S10 Delivery Compiler | §16 | D1 |
| S11 Verification Compiler | §17 | D2 |
| S12 Conformance Compiler | §18 | C1 |

Rule definitions for Q1–C1 are in `config/profiles/plumb-software-2026.1-rules.yaml` and
`docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md`.

## Deferred capabilities

Source: `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md` §5.

- Full architecture generation.
- Full BPMN semantics.
- DMN XML interoperability.
- SysML v2 API integration.
- ArchiMate/TOGAF integration.
- Code binding.
- CI proof receipts.
- Conformance/staleness UI.
- Automated implementation slicing.
- Multi-cloud deployment model.
- Formal certification packs.

## Deferred performance architecture

Source: architecture §33.10.

- Immutable revision cache keyed by RevisionId.
- No full graph deserialize/recanonicalize/rehash per request.
- Content-addressed node/edge storage reuse.
- Touched-element-only revision finalization.
- No duplicate head verification under the write lock (CAS preserved).
- Cached inverted BM25 index.
- Pagination that limits before building a projection.
- Large-document profiling and resource budgets.

These items are not H0 blockers and never permit unbounded inputs. Each one becomes a pre-release
blocker if the E0.4 performance release gate (`scripts/benchmark-representative-scale.sh`) fails
for the shape it addresses.
