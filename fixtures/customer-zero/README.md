# Customer-zero fixture (R0.7)

A second, independent pilot used as the horizontal regression flow of the H0 tranche. It is a
small, deliberately messy requirements source for a university equipment loan desk: mixed list
markers, plain and bold identifiers, notes that are not requirements, a vague quality
requirement, a line with no modal verb and a working-day loan length with open semantics (end
date inclusion, holidays inside the period, half-day loans).

Files:

- `source.md` — the only import input.
- `config.json` — the complete initial CLI configuration (replay provider, gate inputs,
  directory, question routing). The operation, process and calculation scopes name the
  elements earlier stages generate by selector (`$one` / `$all`, docs/formats.md), so the
  file is written once before the flow and never edited during it.
- `expected.yaml` — the frozen expected-results contract the flow asserts section by section.
- `replay/` — the recorded inference executions, one file per stage invocation, generated only
  by the ignored test `regenerate_customer_zero_replay` in `crates/plumb-cli/tests/customer_zero.rs`
  from the scripted model in that file (which introduces no fact absent from `source.md` or
  `config.json`). The normal test replays these files and never writes them.

Provenance: authored for Plumb on 2026-10-25; it shares no content with `fixtures/hr-leave`.
