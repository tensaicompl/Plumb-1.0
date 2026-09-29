# ADR-0001 — One Semantic Model, Many Views

**Status:** Accepted for Plumb 1.0 architecture

## Context

Plumb must support requirements text, domain models, process diagrams, RBAC views, decision tables, state machines, architecture diagrams, API contracts and implementation plans. Allowing each artifact to become independently authoritative would create semantic drift and make AI-generated changes unsafe.

## Decision

Plumb maintains one canonical typed specification graph. Documents, tables and diagrams are projections over that graph. Diagram layout is stored separately from semantic content. Diagram edits create semantic patches that pass through the same intake/decision mechanisms as every other write path.

## Consequences

Positive:
- no parallel truth,
- global impact analysis,
- consistent traceability,
- all views update after one semantic change.

Negative:
- stronger metamodel and view compiler required,
- import/export adapters must preserve semantic identity.
