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
| A0.4: a repeated run_stage returns the existing job in any status; terminal jobs are never reset | Except an `E_JOB_INTERRUPTED` job, which a rerun on the same head re-queues as a new attempt under the same ID (H0.3 retry) |
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

## H0.4 simulation adjudications (lane C)

| Item | Contract on this branch |
|---|---|
| Directory `outcome_selections` | Optional family (`#[serde(default)]`); each selection becomes a HUMAN_CONFIRM `SetNodeExtension` of `plumb_sim:outcome_selection` on the Accepted Outcome. |
| Content-dependent conditions (Fable #116, #117) | Reported as `Undecidable` with `MissingSemanticKind::ModelCondition` (keys such as `conflicting_transitions`, `decision_input_routing`, `decision_input_mismatch:<i>`); the trace envelope is unchanged. |
| Calculation result refs (§33.11) | A Then `result:<key>` resolves to the named Calculation's actual execution result; if it did not run, `ScenarioDiff::MissingExecutionResult` (Fail). |
| Scenario-run staleness | Fine-grained: `scenario_run_currency_since` compares the scenario's exact dependency set between the run's own baseline and now; the API `run_view` uses it (a stale run shows as not run, no new `stale` flag). Unavailable baseline, runtime or model = stale. |
| Fable #118 | SCN-019/025/034 attribute-write expectations are a frozen interpreter verdict (Fail); fixtures unchanged. |
| Holiday question | Answer schema is exact (`holiday_consumes_working_day` const false + calendar enum); the UI renders a scalar `const` as a fixed read-only value that is always in the answer. |

## H0.8 inference/serving adjudications (lane E)

| Item | Contract on this branch |
|---|---|
| Serving | `serve` refuses non-loopback binds (`E_NON_LOOPBACK_BIND`); every request needs a loopback `Host` (missing Host = 403 `E_FORBIDDEN_HOST`); a non-loopback `Origin` = 403 `E_FORBIDDEN_ORIGIN`. |
| Provider timeouts (Fable #63) | Optional `request_timeout_ms` / `connect_timeout_ms` (part of the request ID); default total = 120 s + 30 ms x max_tokens, capped at 2 h; `ProviderError::Timeout`. |
| Recorded parameters (Fable #66) | The inference artifact `parameters` is exactly `{model, max_tokens}`. |
| Replay | MockProvider refuses fixtures whose output fails the schema (`SchemaInvalidOutput`). |
| Prompts (Fable #64, #65) | Every prompt carries the same prompt-injection boundary paragraph and an "Output shape" example validated against its schema; s2-process drops the deferred node kinds; session-turn forbids AUTO_DERIVATION. |
| Open | compiler `stage.rs` goldens still use provider "anthropic" with config `{}`. |

## H0.9 / H0.10 store and core adjudications (lanes A, F1, F2)

| Item | Contract on this branch |
|---|---|
| Proposal occurrences | Store schema v4: proposals keyed by `(proposal_id, base_revision_id)` with `occ:<16 hex>`; in-place v2/v3 migration; lookups by proposal ID resolve the pending occurrence on the loaded head. |
| Store open modes | Only `init` and tests create a database; read-only commands (`vocab`, `questions`, `rounds show`, `export`, `report`, `proposals`, `findings list`) open SQLite read-only; typed store error codes (`E_DATABASE_NOT_INITIALIZED`, `E_DATABASE_BUSY`, `E_DATABASE_READ_ONLY`, `E_STORE_CORRUPT`, `E_STORE_VERSION_UNSUPPORTED`). |
| Artifact integrity | Every artifact read recomputes its hash (`E_STORE_CORRUPT` on mismatch); gate reports are stored as content-addressed artifacts. |
| Graph-bound payload references | Graph validation rejects missing, wrong-type and wrong-status payload refs (census in `plumb_psg::payload_refs`). Deviations: `Question.context_refs` is external (Hotfix 047); `ResourceScope.resource_ref` also allows Operation (Hotfix 042). Analyzer branches for such refs are now unreachable. |
| Canonical JSON | Fail-closed: duplicate member names, integers outside +/-(2^53-1) (including u64 locators) are rejected. |
| Calendar coverage | Optional `coverage` years in the CLI calendar config; carried into the PSG Calendar inside `week_pattern` (`"coverage"` key, no new payload field); a date outside coverage makes the scenario Undecidable (`ModelCondition`, key `calendar_coverage:<year>`). |
| Proposal confidence | f64 (wire form equals identity bytes). |
| Architecture text | §33.11 census and S2.5 working_days text were edited; `SUPPORTING-DOCS.sha256` was not updated. |

## H0.3 / H0.11 / ID-free configuration adjudications (lanes P, Q, S)

| Item | Contract on this branch |
|---|---|
| Store schema | v5 = H0.11 `abandoned` proposal state; v6 = H0.3 recipe-job `attempt`, `owner_id`, `lease_expires_at` and `stage_recipe_job_attempts`. v1-v5 migrate in place. |
| Inference reuse on retry | Only for retried attempts (attempt > 1); unconditional reuse by request hash broke the recorded-replay harness. |
| Recovery | `<db>.jobs.lock` still decides: a sole opener recovers every in-flight job at once; otherwise only expired or ownerless leases are interrupted. Graceful shutdown (30 s grace) is wired to `serve` only; the CLI has `retry <JOB_ID>`. |
| Previews | Never persisted. A preview returns `built_at`; the client resends the split/merge request with `preview {proposal_id, built_at}`, and the server rebuilds and commits only an identical proposal (`E_PREVIEW_MISMATCH` otherwise). Replaces Hotfix 073 "persists it pending". |
| Discard | `POST /api/proposals/{id}/discard` moves a pending question- or intake-origin proposal to `abandoned` (session/stage: `E_PROPOSAL_NOT_DISCARDABLE`); abandoned is not rejection memory. |
| Operations | 76 (was 74): `list_proposals` (not paged: the pending set changes without a revision) and `discard_proposal`; UI Proposals screen after Questions. |
| Stage configuration | Any ID position in the runtime config accepts `{"$one": QUERY}` / `{"$all": QUERY}` selectors (exact payload-field match on Accepted nodes), resolved at stage time and recorded under `inputs.selections`; explicit IDs still work unchanged. An ambiguous or empty `$one` is 422 `E_STAGE_CONFIG_UNRESOLVED` (in keyed entry lists an unmatched entry is dropped). Architecture "F4 in-scope inputs are explicit; never inferred" is read as: a selector is explicit user configuration. |

## Plan tasks: status on this branch

| Task | Status |
|---|---|
| H0.1 | done on main |
| H0.2 | done (ETag = quoted revision ID, If-Match quoted or bare, UI sends the reviewed ETag) |
| H0.3 | done (attempt history, owner leases with heartbeat, CLI `retry`, graceful `serve` shutdown, transient Anthropic retries, typed failure detail; deviations above) |
| H0.4 | done (business failure classes, outcome selection, Fable #116/#117/#119, #118 frozen) |
| H0.5 | done for extract, vocabulary, domain, lifecycle transitions and calculations; analyser dispositions persisted and shown; verification obligations not covered |
| H0.6 | done (governed blocker rule, waiver actor at evaluation, findings list/materialize/waive, stale Open findings closed by materialize); I0 findings are materialized through the same path |
| H0.7 | done (see rule adjudications) |
| H0.8 | done; eager CLI validation of the anthropic policy is part of H0.9 runtime validation |
| H0.9 | done (open modes, typed store errors, eager runtime validation, durability profile documented in docs/operations.md) |
| H0.10 | done (calendar coverage, fine-grained scenario currency and per-decision supersession dispositions closed the last items) |
| H0.11 | done (preview/discard, `abandoned`, Proposals screen, E_GATE_REPORT_INCOHERENT; deviations above) |
| R0.7 | done: `crates/plumb-cli/tests/customer_zero.rs` (a second, non-HR domain driven through the CLI from committed replay, with an oracle and a determinism check); lint diagnostics shown by `stage` |
| E0.1–E0.3 | done as described above |
| E0.4 | partial: `make all` runs fmt/clippy/tests/UI/openapi/placeholders; `make cov` enforces the line thresholds (all met, 92% overall); mutation testing and UI coverage not done |
| E0.5 | done: `docs/pilot/BUILD-REPORT.md`, `docs/pilot/STANDARDS-ALIGNMENT-REPORT.md`; POST.0: `docs/plan/POST-PILOT-SCOPE.md` |
