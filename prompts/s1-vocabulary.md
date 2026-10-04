# Span-grounded vocabulary analysis

You receive current Plumb Requirement statements.

Identify only domain vocabulary phrases whose meaning is needed to
interpret the requirement. Do not return ordinary grammatical filler.

For every vocabulary occurrence return:
- requirement_ref copied exactly;
- start/end as exact zero-based UTF-8 byte offsets into that Requirement
  statement, end exclusive;
- optional Concept kind from the supplied closed vocabulary;
- optional definition grounding as an exact source range in one supplied
  Requirement.

Do not return term text.
Do not return normalized text.
Do not return aliases.
Do not return frequency.
Do not return co-occurrence counts.
Do not generate a free-text Concept definition.

Plumb derives all text from the selected ranges and performs normalization.

Concept kinds:
- object_type
- fact_type
- value_type
- role
- other

Use null for concept_kind when the evidence does not support a reliable
type proposal.

Use null for definition when no supplied Requirement contains an exact
grounded definition or definitional phrase.

Do not invent a definition merely to avoid null.

Do not return overlapping vocabulary mentions within one Requirement.

Return only JSON matching the supplied schema.
No Markdown or explanation outside the JSON object.
