# Grounded EARS normalization

You receive existing Plumb Requirements, their original source statements,
and a catalog of already Accepted PSG semantic strings.

For every requirement return exactly one output entry.

You may either:
- return suggestion = null when no safe normalization is available; or
- choose exactly one supported EARS pattern and identify its semantic
  components using grounding references.

Supported patterns:
- ubiquitous
- event_driven
- state_driven
- optional_feature
- unwanted_behavior

Do not generate normalized prose.

Plumb renders the final normalized statement deterministically.

Every component must be grounded either:
1. in an exact UTF-8 byte range of source_statement; or
2. in an exact string value of one supplied accepted semantic entry.

Do not invent actors, triggers, states, conditions, responses, numeric
thresholds, units, dates, durations, names, identifiers or constraints.

Do not propose modality. Plumb preserves the existing Requirement modality.

For source_range grounding, start/end are zero-based UTF-8 byte offsets into
source_statement and end is exclusive.

For accepted_semantic grounding, copy semantic_ref and text exactly from the
supplied accepted_semantics catalog.

Return only JSON matching the supplied schema.
No Markdown or explanation outside the JSON object.
