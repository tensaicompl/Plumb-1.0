# plumb-goal-approach: deltas from the v3.2 plan

The `plumb-goal-approach` branch no longer follows the one-task-at-a-time contract (CLAUDE.md was
removed). This file lists where the implementation now deliberately differs from the plan and the
architecture text, so the plan can be updated when it is picked up again.

## Behaviour that replaces plan text

| Plan / architecture text | Now |
|---|---|
| Architecture S1.5: "a typed key without a grounded definition yields no Concept" | A typed key without an explicit definition yields a Concept whose definition is the exact statement of the first Requirement that uses it (still exact Requirement text, never prose). |
| Hotfix 083 / E0.1: the three HR calculation gap findings are "explicit upstream analysis material" built by the E0.1 test helper | `plumb_functional::ambiguity` detects them in production (unresolved `working_days` inclusive end, unconfirmed holiday policy, ungoverned unitless Decimal input); `ask` feeds them to question generation. The unresolved `inclusive_end` argument qualifies as a Bool placeholder. |
| R0.4: a gate is stale (`E_GATE_INPUTS_STALE`, `inference_request_changed`) until the analysis recipe reruns | The latest completed run's artifact is replayed against that run's input revision; only findings on still-Accepted elements (and vocabulary mentions in unchanged statements) are kept. Only a never-run phase is stale. `GET /api/gates`, `ask`, `export`, `report` and waivers evaluate missing reports on demand. |
| `ask` uses only the F2 report findings | It also uses the per-candidate F2 analysis findings (e.g. `state_transition_trigger_unresolved:<id>`), which the aggregate F2 rules fold away. |
| Generic confirmation refuses Term / Calculation / Scenario | The CLI `confirm` dispatches them to their dedicated confirmations; confirming a node also confirms its own Proposed relations whose other endpoint is (or becomes) baseline. |
| `set_attribute_type` accepts only Accepted value_type Concept names | It also accepts the pilot primitive types (String, Int, Bool, Date, DateTime, Decimal(n)); CLI `set-attribute-type`. |
| Process analysis forbids `performed_by` on Operation tasks | Allowed as the task's swimlane (Hotfix 079 lane move). |
| Lint RELATIVE_TIME_NO_ANCHOR flags "complete within 2 seconds" | complete/finish/execute + within N units is anchored by the action itself. |
| Proposals are never rebased | Still true. New: stale pending proposals are flagged; `proposals --refresh` re-submits stale Question answers from their recorded decision (a re-resolution of the governed answer, not a patch rebase). |
| (none) | Rejection memory: a stage proposal rejected on its own revision is not proposed again; rejecting an already stale proposal is not remembered. |
| E0.1 builds a reference PSG from fixture-contract.yaml | The product builds the pilot (CLI simulation); E0.1 keeps byte references of the five deterministic outputs in `fixtures/hr-leave/derived/`. |
| E0.2 test host `crates/plumb-api/tests/e2e_hr.rs` | `crates/plumb-cli/tests/sim_hr.rs` (`hr_pilot_simulation` + `hr_oracle`). |

## H0.7 rule adjudications (lane D)

| Rule / item | Contract on this branch |
|---|---|
| `PLUMB.F1.REQ.NO_CONTRADICTION` | No producer exists, so the rule is NOT_APPLICABLE ("No contradiction finding material exists: …") unless supplemental or Accepted Finding material of its code (any finding status) is present; then it evaluates as before. |
| `PLUMB.F1.REQ.TYPE_KNOWN` (Fable #299) | Pass-by-construction is declared in the result evidence `pass_by_construction:closed_requirement_kind_and_level` (the checksummed profile YAML is not edited). |
| `PLUMB.F1.REQ.CRITERIA_FOR_BEHAVIOR` (Fable #299) | "In implementation scope" = not named by a governed `outside_implementation_scope {requirement_ref}` decision marker (new S1.6 marker kind). |
| `PLUMB.F1.REQ.MODALITY_EXPLICIT` | Unchanged behaviour, documented: every Accepted Requirement is in `applies_when` (closed typed modality); `must` is not in the profile modality list and fails. |
| `PLUMB.F2.STATE.REACHABILITY` | Initial / terminal / externally-entered states are declared by the governed State extension `plumb_functional:state_disposition {version 1, initial, terminal, externally_entered}`, produced only by directory `state_dispositions` (HUMAN_CONFIRM SetNodeExtension). Machines (owner -> `has_state` States) without a declared initial state are outside applies_when (NOT_APPLICABLE). |
| `PLUMB.F2.EVENT.PRODUCER/CONSUMER` | `plumb_functional:event_disposition` from directory `event_dispositions`, exactly per §33.7. |
| Directory disposition families | `event_dispositions` and `state_dispositions` default to `[]` (the six element families stay required), so existing directory configurations remain valid. |
| `PLUMB.F2.OPERATION.PERFORMER` | The "owning system/component placeholder" alternative is an Accepted `allocated_to` to an Accepted SoftwareSystem, Container, Component, Module or ExternalSystem. |
| `PLUMB.F2.INVARIANT.EXPRESSIBLE` | The manual/formal alternative is not representable in the pilot (no field, `verified_by` excludes Invariant); scope problems are violations, not evaluator errors. |
| `PLUMB.F2.SOD.NO_VIOLATION` | All four constraint kinds are evaluated over static assignments; dynamic SoD = no single assigned role (with inheritance) activates two governed roles. Condition key `separation_constraint_violated`. |

## Plan tasks: status on this branch

| Task | Status |
|---|---|
| H0.1 | done on main |
| H0.3 | partial: a second process no longer interrupts a live process's jobs (`<db>.jobs.lock`); retry of a killed job not done |
| H0.5 | done for extract, vocabulary, domain, lifecycle transitions and calculations; analyser dispositions persisted and shown; verification obligations not covered |
| H0.6 | done (governed blocker rule, waiver actor at evaluation, findings list/materialize/waive, stale Open findings closed by materialize); I0 findings are materialized through the same path |
| H0.10 | partial: stale flag, answer refresh, rejection memory; everything else in the task list open |
| H0.2, H0.4, H0.7, H0.8, H0.9, H0.11, R0.7 | open |
| E0.1–E0.3 | done as described above |
| E0.4 | partial: `make all` runs fmt/clippy/tests/UI/openapi/placeholders; `make cov` enforces the line thresholds (all met, 92% overall); mutation testing and UI coverage not done |
| E0.5 | `docs/pilot/BUILD-REPORT.md`; the standards-alignment report is not written |
