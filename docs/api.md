# Plumb Pilot API

> Generated from the Rust OpenAPI contract. Do not edit by hand.

Contract version 3, API version 0.1.0, 74 operations. The machine-readable contract is `api/modeller.openapi.json`, the source of the UI client.

A0.1 defines the pilot contract only; A0.2, A0.3 and A0.4 provide the runtime handlers of operations 1-72 and U2.1 those of operations 73-74. An operation listed here is not operational until its handler task is done.

## Conventions

- The API is a task-oriented compatibility projection over the PSG. Requests and responses are JSON unless stated otherwise; errors carry `code`, `message` and `details`.
- Every ordinary `GET` success response carries an `ETag` header equal to the current `semantic_hash`.
- Every `POST` (including dry-run intake, stage runs and session creation and turns) requires an `If-Match` header with the current `semantic_hash`. A mismatch returns `412 Precondition Failed` with `code`, `message`, `details` and the current branch-head `revision` and `semantic_hash`.
- Every successful write returns `version`, `revision`, `semantic_hash` and `events` (DesignEvent `{kind, ids, version}`); `version` is the compatibility alias of `revision`. Endpoint-specific results add fields to these four.
- `GET /api/model/version` returns `version`, `hash`, `revision`, `semantic_hash` and `evidence_hash`, where `version` == `revision` and `hash` == `semantic_hash`.
- Paged lists return `{items, next_cursor}` and accept `cursor` and `limit` (minimum 1). Items are filtered, sorted by stable public ID ascending, continued exclusively after the cursor's `after_id` and then limited; without `limit` all remaining items are returned and `next_cursor` is null.
- Cursor v2: a non-null cursor is the lowercase hex of the UTF-8 RFC 8785 JSON `{"after_id":"<last-returned-id>","request_hash":"sha256:...","revision":"rev:...","semantic_hash":"psg:sha256:..."}`. `revision` is the branch-head revision of the page: `semantic_hash` is not a revision identity, and evidence-only revisions can change page content while keeping it. `request_hash` is the generic SHA-256 of the RFC 8785 JSON `{operation_id, path, query}` (all path parameters; only the present query parameters other than `cursor` and `limit`). Another revision or semantic hash returns `409` `E_CURSOR_BASELINE_CHANGED`; another request returns `400` `E_CURSOR_REQUEST_MISMATCH`; a malformed or non-canonical cursor returns `400` `E_CURSOR_INVALID`.
- `/api/events` is a WebSocket upgrade endpoint; the OpenAPI GET operation exists for compatibility/documentation and must not be treated as an ordinary JSON GET call. Server messages are DesignEvent (`x-plumb-websocket`).
- `GET /api/gates` reports the NOW gates I0, F1, F2, F3 and F4 in this order (`x-plumb-gate-order`); later gates are not in the NOW API.
- Stage jobs report `status` (queued, acquiring, evaluating, ready, stale, failed, committed) and the `base_revision` they captured; `stale` is a status, and stale output is never committed directly.
- Readiness `overall` and the sub-scores clarity, completeness, consistency, verifiability and confirmation are nullable and advisory: the composite is uncalibrated and non-authoritative in the pilot. `null` means not calculated by the current pilot runtime, not zero; readiness never influences a gate.
- `POST /api/requirements/{id}/accept-ears` requires `text`: the exact EARS-normalized statement the caller reviewed. EARS suggestions are not graph state and are never rediscovered by the server.
- Examples come from `fixtures/hr-leave/fixture-contract.yaml` (requirement HR-001, entity ApprovalRecord, calculation AnnualBalanceDelta, rule AnnualLeaveEligibility, process CancelApprovedLeave) or from the deterministic schema fallback.

## Pilot restrictions and exclusions

- D test endpoints are not in the pilot API.
- POST calculation edit is not in NOW P-R scope; calculation examples are read-only.
- Process branch mutation is not in NOW P-R scope; step operations are add, move and remove.
- Process SVG export is not in NOW P-R scope; process import and export use mermaid only.
- The additional generic v3 engineering endpoints are introduced gradually and are not part of A0.1 except proposal accept.

## Implementation ownership

- A0.2: 30 operations (9 POST, 21 GET).
- A0.3: 40 operations (27 POST, 13 GET).
- A0.4: 2 operations (1 POST, 1 GET).
- U2.1: 2 operations (2 POST, 0 GET).
- Hotfix 065 froze the deferred A0.3 compatibility semantics (contract version 3).
- A0.4 owns `run_stage` and `get_job` and must not start before its separate stage-orchestration contract freeze.
- Hotfix 073 appends operations 73 and 74, the U2.1 semantic change previews: the server builds the exact domain Proposal, persists it pending on the baseline revision and returns its semantic element diff without committing; `candidate_semantic_hash` is not a prediction of the committed `semantic_hash`. Confirmation is the existing proposal action: create for a requirement split (Hotfix 074), merge for a Term merge.

## Operations

| # | Method | Path | operationId | Tag | Owner |
|---|---|---|---|---|---|
| 1 | GET | `/api/me` | `get_me` | meta | A0.2 |
| 2 | GET | `/api/model/version` | `get_model_version` | meta | A0.2 |
| 3 | GET | `/api/gates` | `get_gates` | meta | A0.2 |
| 4 | GET | `/api/search` | `search_model` | meta | A0.3 |
| 5 | GET | `/api/events` | `events_websocket` | meta | A0.2 |
| 6 | POST | `/api/sources` | `create_source` | sources | A0.2 |
| 7 | GET | `/api/sources/{id}` | `get_source` | sources | A0.2 |
| 8 | GET | `/api/sources/{id}/spans` | `get_source_spans` | sources | A0.2 |
| 9 | POST | `/api/stages/{stage}/run` | `run_stage` | stages | A0.4 |
| 10 | GET | `/api/jobs/{id}` | `get_job` | stages | A0.4 |
| 11 | GET | `/api/requirements` | `list_requirements` | requirements | A0.2 |
| 12 | GET | `/api/requirements/{id}` | `get_requirement` | requirements | A0.2 |
| 13 | POST | `/api/requirements/{id}/classify` | `classify_requirement` | requirements | A0.2 |
| 14 | POST | `/api/requirements/{id}/accept-ears` | `accept_requirement_ears` | requirements | A0.2 |
| 15 | POST | `/api/requirements/{id}/split` | `split_requirement` | requirements | A0.3 |
| 16 | POST | `/api/requirements/{id}/waive-finding` | `waive_requirement_finding` | requirements | A0.3 |
| 17 | POST | `/api/requirements/{id}/mark-duplicate` | `mark_requirement_duplicate` | requirements | A0.3 |
| 18 | GET | `/api/glossary` | `list_glossary` | glossary | A0.2 |
| 19 | POST | `/api/glossary/{id}/confirm` | `confirm_glossary` | glossary | A0.2 |
| 20 | POST | `/api/glossary/{id}/edit` | `edit_glossary` | glossary | A0.2 |
| 21 | POST | `/api/glossary/{id}/merge` | `merge_glossary` | glossary | A0.3 |
| 22 | GET | `/api/model/sentences` | `list_sentences` | sentences | A0.3 |
| 23 | POST | `/api/model/sentences/{id}/accept` | `accept_sentence` | sentences | A0.3 |
| 24 | POST | `/api/model/sentences/{id}/edit` | `edit_sentence` | sentences | A0.3 |
| 25 | POST | `/api/model/sentences/{id}/reject` | `reject_sentence` | sentences | A0.3 |
| 26 | GET | `/api/model/entities` | `list_entities` | entities | A0.2 |
| 27 | GET | `/api/model/entities/{id}` | `get_entity` | entities | A0.2 |
| 28 | GET | `/api/model/crud` | `get_crud_matrix` | entities | A0.3 |
| 29 | POST | `/api/model/attributes/{id}/type` | `set_attribute_type` | entities | A0.2 |
| 30 | POST | `/api/model/attributes/{id}/unit` | `set_attribute_unit` | entities | A0.2 |
| 31 | POST | `/api/model/attributes/{id}/class` | `set_attribute_class` | entities | A0.2 |
| 32 | GET | `/api/model/calculations` | `list_calculations` | calculations | A0.2 |
| 33 | GET | `/api/model/calculations/{id}/example` | `get_calculation_example` | calculations | A0.3 |
| 34 | POST | `/api/model/calculations/{id}/confirm` | `confirm_calculation` | calculations | A0.2 |
| 35 | GET | `/api/model/rules` | `list_rules` | rules | A0.3 |
| 36 | GET | `/api/model/rules/{id}` | `get_rule` | rules | A0.3 |
| 37 | POST | `/api/model/rules/{id}/cells` | `set_rule_cell` | rules | A0.3 |
| 38 | GET | `/api/processes` | `list_processes` | processes | A0.2 |
| 39 | GET | `/api/processes/{id}` | `get_process` | processes | A0.2 |
| 40 | POST | `/api/processes/{id}/steps` | `mutate_process_steps` | processes | A0.3 |
| 41 | POST | `/api/processes/import` | `import_process` | processes | A0.3 |
| 42 | GET | `/api/processes/{id}/export` | `export_process` | processes | A0.3 |
| 43 | GET | `/api/scenarios` | `list_scenarios` | scenarios | A0.3 |
| 44 | POST | `/api/scenarios/{id}/confirm` | `confirm_scenario` | scenarios | A0.3 |
| 45 | POST | `/api/scenarios/{id}/correct` | `correct_scenario` | scenarios | A0.3 |
| 46 | POST | `/api/scenarios/confirm-derived` | `confirm_derived_scenarios` | scenarios | A0.3 |
| 47 | POST | `/api/scenarios/run` | `run_scenarios` | scenarios | A0.3 |
| 48 | GET | `/api/scenarios/{id}/run` | `get_scenario_run` | scenarios | A0.3 |
| 49 | GET | `/api/findings` | `list_findings` | findings | A0.2 |
| 50 | POST | `/api/findings/{id}/waive` | `waive_finding` | findings | A0.3 |
| 51 | GET | `/api/assumptions` | `list_assumptions` | assumptions | A0.2 |
| 52 | POST | `/api/assumptions/{id}/resolve` | `resolve_assumption` | assumptions | A0.3 |
| 53 | GET | `/api/questions` | `list_questions` | questions | A0.3 |
| 54 | POST | `/api/questions/{id}/answer` | `answer_question` | questions | A0.3 |
| 55 | POST | `/api/rounds` | `create_round` | questions | A0.3 |
| 56 | GET | `/api/rounds/{id}` | `get_round` | questions | A0.3 |
| 57 | POST | `/api/proposals/intake` | `intake_proposal` | proposals | A0.3 |
| 58 | POST | `/api/proposals/{id}/link` | `link_proposal` | proposals | A0.3 |
| 59 | POST | `/api/proposals/{id}/merge` | `merge_proposal` | proposals | A0.3 |
| 60 | POST | `/api/proposals/{id}/supersede` | `supersede_proposal` | proposals | A0.3 |
| 61 | POST | `/api/proposals/{id}/create` | `create_proposal` | proposals | A0.3 |
| 62 | POST | `/api/proposals/{id}/reject` | `reject_proposal` | proposals | A0.3 |
| 63 | POST | `/api/proposals/{id}/accept` | `accept_proposal` | proposals | A0.3 |
| 64 | POST | `/api/sessions` | `create_session` | sessions | A0.3 |
| 65 | POST | `/api/sessions/{id}/turns` | `create_session_turn` | sessions | A0.3 |
| 66 | GET | `/api/sessions/{id}` | `get_session` | sessions | A0.3 |
| 67 | GET | `/api/sessions/{id}/metrics` | `get_session_metrics` | sessions | A0.3 |
| 68 | GET | `/api/decisions` | `list_decisions` | decisions | A0.2 |
| 69 | GET | `/api/requirements/{id}/readiness` | `get_requirement_readiness` | readiness | A0.2 |
| 70 | GET | `/api/readiness` | `get_readiness` | readiness | A0.2 |
| 71 | GET | `/api/model/export` | `export_model` | export | A0.2 |
| 72 | GET | `/api/model/diff` | `get_model_diff` | export | A0.2 |
| 73 | POST | `/api/requirements/{id}/split/preview` | `preview_requirement_split` | requirements | U2.1 |
| 74 | POST | `/api/glossary/{id}/merge/preview` | `preview_glossary_merge` | glossary | U2.1 |
