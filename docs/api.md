# Plumb Pilot API

> Generated from the Rust OpenAPI contract. Do not edit by hand.

Contract version 1, API version 0.1.0, 72 operations. The machine-readable contract is `api/modeller.openapi.json`, the source of the UI client.

A0.1 defines the pilot contract only; A0.2 and A0.3 provide the runtime handlers. An operation listed here is not operational until its handler task is done.

## Conventions

- The API is a task-oriented compatibility projection over the PSG. Requests and responses are JSON unless stated otherwise; errors carry `code`, `message` and `details`.
- Every ordinary `GET` success response carries an `ETag` header equal to the current `semantic_hash`.
- Every `POST` (including dry-run intake, stage runs and session creation and turns) requires an `If-Match` header with the current `semantic_hash`. A mismatch returns `412 Precondition Failed` with `code`, `message`, `details` and the current branch-head `revision` and `semantic_hash`.
- Every successful write returns `version`, `revision`, `semantic_hash` and `events` (DesignEvent `{kind, ids, version}`); `version` is the compatibility alias of `revision`. Endpoint-specific results add fields to these four.
- `GET /api/model/version` returns `version`, `hash`, `revision`, `semantic_hash` and `evidence_hash`, where `version` == `revision` and `hash` == `semantic_hash`.
- Paged lists return `{items, next_cursor}` and accept `cursor` and `limit` (minimum 1). A non-null cursor is the lowercase hex of the UTF-8 RFC 8785 JSON `{"after_id":"<last-returned-id>","semantic_hash":"psg:sha256:..."}`; a cursor issued for another `semantic_hash` returns `409` with `E_CURSOR_BASELINE_CHANGED`.
- `/api/events` is a WebSocket upgrade endpoint; the OpenAPI GET operation exists for compatibility/documentation and must not be treated as an ordinary JSON GET call. Server messages are DesignEvent (`x-plumb-websocket`).
- `GET /api/gates` reports the NOW gates I0, F1, F2, F3 and F4 in this order (`x-plumb-gate-order`); later gates are not in the NOW API.
- Stage jobs report `status` (queued, acquiring, evaluating, ready, stale, failed, committed) and the `base_revision` they captured; `stale` is a status, and stale output is never committed directly.
- Readiness `overall` is nullable and advisory: the composite is uncalibrated and non-authoritative in the pilot; the sub-scores are clarity, completeness, consistency, verifiability and confirmation.
- Examples come from `fixtures/hr-leave/fixture-contract.yaml` (requirement HR-001, entity ApprovalRecord, calculation AnnualBalanceDelta, rule AnnualLeaveEligibility, process CancelApprovedLeave) or from the deterministic schema fallback.

## Pilot restrictions and exclusions

- D test endpoints are not in the pilot API.
- POST calculation edit is not in NOW P-R scope; calculation examples are read-only.
- Process branch mutation is not in NOW P-R scope; step operations are add, move and remove.
- Process SVG export is not in NOW P-R scope; process import and export use mermaid only.
- The additional generic v3 engineering endpoints are introduced gradually and are not part of A0.1 except proposal accept.

## Operations

| # | Method | Path | operationId | Tag |
|---|---|---|---|---|
| 1 | GET | `/api/me` | `get_me` | meta |
| 2 | GET | `/api/model/version` | `get_model_version` | meta |
| 3 | GET | `/api/gates` | `get_gates` | meta |
| 4 | GET | `/api/search` | `search_model` | meta |
| 5 | GET | `/api/events` | `events_websocket` | meta |
| 6 | POST | `/api/sources` | `create_source` | sources |
| 7 | GET | `/api/sources/{id}` | `get_source` | sources |
| 8 | GET | `/api/sources/{id}/spans` | `get_source_spans` | sources |
| 9 | POST | `/api/stages/{stage}/run` | `run_stage` | stages |
| 10 | GET | `/api/jobs/{id}` | `get_job` | stages |
| 11 | GET | `/api/requirements` | `list_requirements` | requirements |
| 12 | GET | `/api/requirements/{id}` | `get_requirement` | requirements |
| 13 | POST | `/api/requirements/{id}/classify` | `classify_requirement` | requirements |
| 14 | POST | `/api/requirements/{id}/accept-ears` | `accept_requirement_ears` | requirements |
| 15 | POST | `/api/requirements/{id}/split` | `split_requirement` | requirements |
| 16 | POST | `/api/requirements/{id}/waive-finding` | `waive_requirement_finding` | requirements |
| 17 | POST | `/api/requirements/{id}/mark-duplicate` | `mark_requirement_duplicate` | requirements |
| 18 | GET | `/api/glossary` | `list_glossary` | glossary |
| 19 | POST | `/api/glossary/{id}/confirm` | `confirm_glossary` | glossary |
| 20 | POST | `/api/glossary/{id}/edit` | `edit_glossary` | glossary |
| 21 | POST | `/api/glossary/{id}/merge` | `merge_glossary` | glossary |
| 22 | GET | `/api/model/sentences` | `list_sentences` | sentences |
| 23 | POST | `/api/model/sentences/{id}/accept` | `accept_sentence` | sentences |
| 24 | POST | `/api/model/sentences/{id}/edit` | `edit_sentence` | sentences |
| 25 | POST | `/api/model/sentences/{id}/reject` | `reject_sentence` | sentences |
| 26 | GET | `/api/model/entities` | `list_entities` | entities |
| 27 | GET | `/api/model/entities/{id}` | `get_entity` | entities |
| 28 | GET | `/api/model/crud` | `get_crud_matrix` | entities |
| 29 | POST | `/api/model/attributes/{id}/type` | `set_attribute_type` | entities |
| 30 | POST | `/api/model/attributes/{id}/unit` | `set_attribute_unit` | entities |
| 31 | POST | `/api/model/attributes/{id}/class` | `set_attribute_class` | entities |
| 32 | GET | `/api/model/calculations` | `list_calculations` | calculations |
| 33 | GET | `/api/model/calculations/{id}/example` | `get_calculation_example` | calculations |
| 34 | POST | `/api/model/calculations/{id}/confirm` | `confirm_calculation` | calculations |
| 35 | GET | `/api/model/rules` | `list_rules` | rules |
| 36 | GET | `/api/model/rules/{id}` | `get_rule` | rules |
| 37 | POST | `/api/model/rules/{id}/cells` | `set_rule_cell` | rules |
| 38 | GET | `/api/processes` | `list_processes` | processes |
| 39 | GET | `/api/processes/{id}` | `get_process` | processes |
| 40 | POST | `/api/processes/{id}/steps` | `mutate_process_steps` | processes |
| 41 | POST | `/api/processes/import` | `import_process` | processes |
| 42 | GET | `/api/processes/{id}/export` | `export_process` | processes |
| 43 | GET | `/api/scenarios` | `list_scenarios` | scenarios |
| 44 | POST | `/api/scenarios/{id}/confirm` | `confirm_scenario` | scenarios |
| 45 | POST | `/api/scenarios/{id}/correct` | `correct_scenario` | scenarios |
| 46 | POST | `/api/scenarios/confirm-derived` | `confirm_derived_scenarios` | scenarios |
| 47 | POST | `/api/scenarios/run` | `run_scenarios` | scenarios |
| 48 | GET | `/api/scenarios/{id}/run` | `get_scenario_run` | scenarios |
| 49 | GET | `/api/findings` | `list_findings` | findings |
| 50 | POST | `/api/findings/{id}/waive` | `waive_finding` | findings |
| 51 | GET | `/api/assumptions` | `list_assumptions` | assumptions |
| 52 | POST | `/api/assumptions/{id}/resolve` | `resolve_assumption` | assumptions |
| 53 | GET | `/api/questions` | `list_questions` | questions |
| 54 | POST | `/api/questions/{id}/answer` | `answer_question` | questions |
| 55 | POST | `/api/rounds` | `create_round` | questions |
| 56 | GET | `/api/rounds/{id}` | `get_round` | questions |
| 57 | POST | `/api/proposals/intake` | `intake_proposal` | proposals |
| 58 | POST | `/api/proposals/{id}/link` | `link_proposal` | proposals |
| 59 | POST | `/api/proposals/{id}/merge` | `merge_proposal` | proposals |
| 60 | POST | `/api/proposals/{id}/supersede` | `supersede_proposal` | proposals |
| 61 | POST | `/api/proposals/{id}/create` | `create_proposal` | proposals |
| 62 | POST | `/api/proposals/{id}/reject` | `reject_proposal` | proposals |
| 63 | POST | `/api/proposals/{id}/accept` | `accept_proposal` | proposals |
| 64 | POST | `/api/sessions` | `create_session` | sessions |
| 65 | POST | `/api/sessions/{id}/turns` | `create_session_turn` | sessions |
| 66 | GET | `/api/sessions/{id}` | `get_session` | sessions |
| 67 | GET | `/api/sessions/{id}/metrics` | `get_session_metrics` | sessions |
| 68 | GET | `/api/decisions` | `list_decisions` | decisions |
| 69 | GET | `/api/requirements/{id}/readiness` | `get_requirement_readiness` | readiness |
| 70 | GET | `/api/readiness` | `get_readiness` | readiness |
| 71 | GET | `/api/model/export` | `export_model` | export |
| 72 | GET | `/api/model/diff` | `get_model_diff` | export |
