# ADR-0002 — Separate Functional Truth from Technical Design

**Status:** Accepted for Plumb 1.0 architecture

## Context

One functional specification can be realized through multiple valid technical architectures. If Plumb writes technical selections directly into functional truth, it becomes impossible to compare designs, revisit technology, or determine whether an architecture element is required by intent or merely chosen as a solution.

## Decision

Functional and technical elements remain separate namespaces. Technical architectures are attached through typed allocation/satisfaction relationships such as `allocated_to`, `realized_by` and `satisfied_by`. Architecture alternatives live in candidate branches until explicitly accepted.

## Consequences

Positive:
- supports architecture alternatives,
- preserves requirement independence,
- makes architecture decisions auditable,
- enables future re-platforming without rewriting business requirements.

Negative:
- introduces cross-layer mapping work,
- requires explicit architecture-driver and allocation rules.
