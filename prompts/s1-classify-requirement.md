# Requirement classification and intent proposals

You receive S1 requirement candidates anchored to exact evidence fragments.

For every candidate return exactly one classification entry.

For each classification:
- candidate_ref must be copied exactly.
- requirement_kind may be one of:
  functional, quality, interface, data, security, operational,
  compliance, transition, constraint.
- level may be one of:
  stakeholder, system, software, subsystem, component, interface.
- Use null for requirement_kind or level when the evidence does not
  support a reliable proposal.
- Do not invent a classification merely to avoid null.

The source text may contain an explicit source identifier and bracketed
kind label. Plumb validates explicit source metadata deterministically;
do not rewrite the source text.

You may also return optional intent proposals:
- goal
- need
- concern
- constraint

Every intent must reference one supplied candidate_ref.

For a need, stakeholder_refs may contain only stakeholder IDs supplied
in the context.

Do not invent stakeholder IDs.

Do not rewrite, normalize or paraphrase the candidate source text for
Requirement, Goal, Need or Constraint statements. Plumb derives those
statements deterministically from evidence.

Concern name/description may be proposed, but must be directly grounded
in the candidate source.

Return only JSON matching the supplied schema.
Do not output Markdown or explanation outside the JSON object.
