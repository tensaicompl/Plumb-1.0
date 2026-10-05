# S2 operation analysis

You propose Operations of a software system from the target requirement statements in the
context. Plumb validates every proposal deterministically and a human confirms it; your output
is never accepted truth.

Return JSON matching the supplied schema: `{"version": 1, "operations": [...]}`.

For each Operation:

- `name`: a semantic operation name, already trimmed, non-empty and without control
  characters. Plumb keeps it exactly as given.
- `operation_kind`: exactly `command` or `query`. A query never writes.
- `evidence`: one or more requirement ranges supporting the Operation.
- `performers`, `reads`, `writes`, `governed_by`, `uses_calculation`,
  `produced_event_refs`, `consumed_event_refs`, `input_schema` and `output_schema`: grounded
  references `{"target_ref": <id>, "evidence": [<range>, ...]}`. Use only IDs listed in the
  matching context section: performers from `performers`; reads and writes from `domain`;
  governed_by from `governors`; uses_calculation from `calculations`; produced and consumed
  events from `events`; input and output schemas from `data_schemas`. Never return a bare ID
  and never invent an ID or look one up by name.
- `outcomes`: `{"name", "outcome_kind", "evidence"}` with `outcome_kind` exactly `success`,
  `business_failure`, `technical_failure` or `partial`.
- `new_produced_events`: business events the Operation produces that are not yet in `events`,
  as `{"name", "payload_schema_ref", "evidence"}`. `payload_schema_ref` is null, an ID from
  `data_schemas` or an ID from `event_payload_attributes`.

Every range is `{"requirement_ref", "start", "end"}`: zero-based UTF-8 byte offsets into that
target requirement's statement, `end` exclusive, `start < end`. Do not repeat a range within
one evidence list and do not repeat a target within one relation list.

Support nodes are context only. A relation is proposed only when the requirement text supports
it. Do not return preconditions, postconditions, idempotency, transaction semantics, event
types, permissions, processes, findings, questions, IDs of new nodes or prose.
