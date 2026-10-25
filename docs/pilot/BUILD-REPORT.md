# Plumb HR pilot — build report

Branch `plumb-goal-approach`. This report records what the pilot build demonstrably does, how it
was verified, and what it does not do yet. Nothing here is claimed beyond what a test executes.

## Build identity

| Item | Value |
|---|---|
| Base commit of this report | `b6a5459` (plus the E0.1/E0.3/E0.4/E0.5 commit that adds this file) |
| Rust toolchain | 1.98.1 (`rust-toolchain.toml`) |
| Final pilot revision | `rev:60:06937019ab7406cb` |
| Final pilot `semantic_hash` | `psg:sha256:06937019ab7406cb17872b23d10107772d89af0f9fe8e99ebf55c8e3f28f4e75` |
| Evidence hash | `ev:sha256:2fd981c26f92d9a965a7d3afa6d5a65f13fb4f4ad8d154e2a1c91734e6d57ac4` |
| Rule pack hash | `sha256:b3a48aa0d0dc47b612697bbf35fa778499509343f74de43459fbcc145e21d33e` |
| Pilot import input | `fixtures/hr-leave/requirements.md` only (importing both the `.md` and `.docx` into one project is unsupported) |

## How the pilot runs

`crates/plumb-cli/tests/sim_hr.rs` (`hr_pilot_simulation`) drives the real `plumb-cli` binary on
a fresh project using product operations only: `init`, `import`, every stage under
`--provider replay`, `accept-set` / `apply`, `confirm`, `directory`, `set-attribute-type`,
`findings list|materialize|waive`, `ask`, `rounds create`, `answer`, `proposals --refresh`,
`report`, `export`, `ready`. Each inference stage is first answered by a scripted model in-process
(the model answers like a correct model for the HR fixture and, like a model following the
prompts, returns no term definitions) and the recorded executions are replayed through the CLI.

Determinism: the five deterministic outputs (`functional.yaml`, `glossary.md`, `scenarios.yaml`,
`model-report.md`, `gate-report.json`) are compared byte for byte with the committed references in
`fixtures/hr-leave/derived/` on every run (E0.1); repeated runs reproduced them exactly.
`decisions.jsonl` carries wall-clock decision times and is not compared.

## Gates at the end of the pilot

| Gate | Result | Notes |
|---|---|---|
| I0 | pass | |
| F1 | pass | `PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED` **waived** (see Waivers); `ISO29148.F1.REQ.LINEAGE` and `PLUMB.F1.REQ.CRITERIA_FOR_BEHAVIOR` fail at severity error (non-blocking) |
| F2 | pass | `PLUMB.F2.EVENT.CONSUMER` and `PLUMB.F2.STATE.REACHABILITY` end in ERROR (non-blocking); reachability is unrepresentable in the v3 State payload |
| F3 | pass | all four expert questions answered and governed |
| F4 | pass | vacuous: the configuration lists no in-scope scenarios |

## Waivers

| Finding | Rule | Requirements | Rationale |
|---|---|---|---|
| `fnd:14f0eaebd698b9c9` | `PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED` (decision_required) | HR-023, HR-024 | "HR-023 and HR-024 notify different outcomes; they are distinct requirements." (the fixture's Jaccard threshold 0.8 flags them) |

## Expert questions (production path)

Raised by the product (not injected), routed to `stakeholder:hr`, composed into a round, answered
and governed:

| Question template | Answer (fixtures/hr-leave/expert-answers.yaml) | Effect |
|---|---|---|
| `f2.calculation.inclusive-end.v1` | inclusive_end: true | `working_days(..., calendar, true)` |
| `f2.calculation.holiday-policy.v1` | calendar_ref: calendar:office, holiday_consumes_working_day: false | calendar set, decision recorded |
| `f2.calculation.fraction.v1` | 0.5 working_day | governed calculation example (half day = 0.5) |
| `f2.state.transition.trigger.v1` | WithdrawLeaveRequest | Submitted → Withdrawn transition created |

## HR semantic oracle (E0.2)

Production output asserted against `fixtures/hr-leave/fixture-contract.yaml`:

| Section | Produced and in contract / contract | Assertion |
|---|---|---|
| requirements | 32 / 32 | exact statements and kinds |
| states | 6 / 6 | exact names |
| rules | 3 / 3 | exact names |
| ambiguities | 3 / 3 | post-answer semantics in effect |
| operations | 6 / 13 | everything produced is in the contract |
| events | 5 / 6 | everything produced is in the contract |
| entities | 5 / 9 | reported (Department, ManagerAssignment, EmploymentContract are never mentioned in the requirements) |
| calculations | 1 / 2 | reported (AnnualBalanceDelta not produced) |
| processes | 0 / 6 | reported (process analysis not exercised) |
| invariants | 0 / 5 | reported |
| scenarios | 7 / 44 | reported |

## Test commands and results

| Command | Result |
|---|---|
| `cargo test --workspace` | 1,748+ tests, 0 failures (includes the HR pilot, oracle and reference outputs) |
| `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cd ui && npm run typecheck && npm run lint && npm test` | clean; 131 tests |
| `make openapi` | committed contract equals the generated one; UI client matches |
| `make placeholders` | no placeholder markers in production source |
| `make e2e` (`scripts/run-pilot-e2e.sh`) | 6/6 Playwright pilot tests against the real backend; axe on every route with no serious or critical violations |
| `make cov` | see Coverage |

## Coverage

`cargo llvm-cov --workspace` (all tests): **92.0% of lines** overall (89.7% regions, 82.6% functions). Every crate meets the E0.4 line thresholds, enforced by `make cov` (`scripts/check-coverage.py`).

| Crate | Line coverage | Lines | Plan target |
|---|---|---|---|
| plumb-api | 87.2% | 11986 | 70% |
| plumb-artifacts | 94.7% | 150 | 70% |
| plumb-cli | 91.5% | 1069 | 70% |
| plumb-compiler | 97.3% | 1312 | 70% |
| plumb-core | 99.1% | 460 | 70% |
| plumb-expr | 91.9% | 1746 | 85% |
| plumb-functional | 92.2% | 17277 | 70% |
| plumb-import | 94.3% | 2208 | 70% |
| plumb-inference | 97.1% | 522 | 70% |
| plumb-intake | 97.5% | 854 | 85% |
| plumb-lint | 96.9% | 963 | 70% |
| plumb-patch | 96.9% | 1116 | 85% |
| plumb-psg | 96.9% | 1242 | 85% |
| plumb-session | 94.9% | 835 | 70% |
| plumb-sim | 87.2% | 2545 | 85% |
| plumb-store | 92.1% | 1979 | 70% |
| plumb-validation | 95.3% | 7550 | 85% |

## Lint corpus

lint corpus qualification: not run — external human-labelled corpus not supplied. No lint-precision
claim is made.

## Producer coverage of pilot-critical semantic families

Produced by the pipeline in the pilot: Requirement, Term, Concept, Entity, Attribute, State,
Transition, Operation, Outcome, Event, Calculation, Scenario (decision-table rows, transitions),
ScenarioRun, Finding, Question, ResolutionDecision, plus the configured directory (Actor,
BusinessRole, Stakeholder, Calendar, Rule, DecisionTable). Not produced in the pilot: Process,
Invariant, DomainRelationship, Assumption, AcceptanceCriterion, VerificationObligation instances,
authorization elements.

## Real-model run (claude-opus-5-5)

`PLUMB_REAL_KEY_FILE=<key file> cargo test -p plumb-cli --features llm --test sim_hr
hr_pilot_real_model -- --ignored --nocapture` runs the same product flow with every inference
stage answered by the Anthropic API (effort high). The analyst steps are generic:
- accept and confirm each stage;
- waive the flagged duplicate;
- type and bind only the Attributes the model produced;
- answer the questions the product raises.

Result on 2026-10-25, branch `plumb-goal-approach` (build 7286def):

| Item | Result |
|---|---|
| Stages | every stage completed; `ready` passes I0, F1 (duplicate waived), F2, F3, F4 |
| Requirements | 32/32 accepted, kinds 32/32; 29/32 statements byte-identical to the contract (the rest are accepted EARS rewordings) |
| States | 6/6, names exactly the contract's |
| Entities | 12 accepted (3 with the contract's exact names; the rest are reasonable variants such as "annual leave request") |
| Operations / events | 7 of 13 / 2 of 6 |
| Invariants / processes | 1 of 5 / 0 of 6 |
| Expert questions | 1 raised and answered |

The fixes this run required are listed in `docs/plan/GOAL-APPROACH-DELTA.md` (Real-model run adjudications):
- structured outputs in authored schema order;
- the grammar-size merge;
- the effort setting;
- tolerant optional intents;
- re-asking on retry;
- three prompt clarifications;
- default invariant scopes.

Model output is not deterministic, so these counts vary between runs.

## Not done / known limitations

- Real-model coverage gaps: operations, events, processes and invariants are produced only partly (see Real-model run).
- H0.10 beyond stale answers and rejection memory (proposal occurrence identity with a store
  migration, artifact integrity checks, crash-recovery settlement, numeric/calendar integrity,
  importer Unicode normalization, and the other listed audit items) is not implemented.
- H0.2–H0.4, H0.7–H0.9, H0.11 not implemented.
- Graph-dependent stage configuration (operation, process and calculation scopes, F4 in-scope
  scenarios) still names generated IDs in the JSON configuration.
- Mutation testing (`cargo mutants`) is not run; UI coverage is not measured.
