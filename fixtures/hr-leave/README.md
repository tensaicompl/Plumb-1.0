# HR Leave Pilot Fixture

This directory is an **authoritative test fixture** for the Plumb v3 implementation plan.

The original v2 plan specified only fixture counts and named ambiguities; it did not include the exact fixture content. To remove implementation ambiguity, this pack fixes the exact pilot input here.

Authoritative inputs:

- `requirements.md`
- `requirements.docx`
- `fixture-contract.yaml`
- `stakeholders.yaml`
- `profile.yaml`
- `expert-answers.yaml`
- `scenarios.yaml`

`fixture-contract.yaml` is the semantic oracle for expected entities, operations, processes, rules, calculations, events, roles/permissions, counts, and post-answer ambiguity semantics.

The implementation may generate derived reference artifacts such as `reference-psg.yaml`, `reference-functional.yaml`, and request-hash keyed mock inference artifacts, but it may not change the authoritative fixture inputs to make tests pass.

The three deliberately unresolved pre-answer ambiguities are:

1. half-day fraction;
2. treatment of a public holiday inside the requested period;
3. whether the end date is inclusive.

They must be `Undecidable` before the corresponding expert answers are applied through the normal question/decision path.
