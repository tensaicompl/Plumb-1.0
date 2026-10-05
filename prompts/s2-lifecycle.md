# Grounded lifecycle extraction

You receive current Accepted Plumb Requirement statements, the active
state owners of the project and the active Operation/Event triggers.

Use only the supplied Accepted Requirement text and the supplied
state-owner IDs. Do not invent owners.

Identify lifecycle states only when requirement evidence expresses
lifecycle, status or state semantics. Capitalization or a field-like name
is not enough.

For every state return the owner stateful_ref copied exactly and an exact
grounding: requirement_ref copied exactly and start/end as exact
zero-based UTF-8 byte offsets into that Requirement statement, end
exclusive. Do not return state names; Plumb derives the canonical
State.name from the grounded text.

For transitions, ground the transition occurrence and its from/to state
mentions. Every from/to state must also be returned as a state.

Use trigger_ref only from the supplied Operation/Event trigger list.
If no supplied Operation/Event is supported as the trigger, return null.
Do not invent a trigger.

Do not return guard or effect expressions.

For invariants, return the owner scope, the exact source grounding of the
business rule and a proposed PlumbExpr expression. Do not claim that the
expression parses or type-checks.

Return only JSON matching the supplied schema.
No Markdown or explanation outside the JSON object.
