# Plumb 1.0 — Product Vision

## Thesis

Software projects still lose intent between stakeholder documents, analysis, architecture, tickets, code and tests. Existing requirements-management platforms are generally strong at storing artifacts and trace links, while modern AI development tools are strong at generating prose, designs and code. Neither category reliably establishes that the underlying software specification is semantically complete, internally coherent and implementation-ready.

Plumb is intended to close that gap.

Its job is to progressively compile human intent into a software specification that can be inspected by humans, visualized through recognized engineering views, consumed by coding agents, and verified against implementation evidence.

## Product category

Preferred category language:

- **Software Specification Compiler**
- **Executable Requirements Engineering**
- **Intent-to-Implementation Specification System**

Avoid positioning Plumb primarily as:

- an AI business analyst,
- a generic requirements-management system,
- a ticket generator,
- a diagram generator,
- a spec-driven-development prompt framework.

Those categories describe pieces of Plumb, not the product.

## Canonical lifecycle

1. **Evidence** — ingest documents, diagrams, existing APIs, technical constraints, conversations and legacy specifications while preserving source provenance.
2. **Intent** — identify goals, stakeholders, requirements, assumptions, terminology and unresolved contradictions.
3. **Functional specification** — compile domain entities, operations, processes, rules, calculations, states, events, roles and executable scenarios.
4. **Quality & constraints** — make NFRs measurable through quality scenarios; import imposed technical, security, data, regulatory and platform constraints.
5. **Architecture** — derive architecture drivers, generate candidates, analyze trade-offs and record explicit architecture decisions.
6. **Technical contracts** — allocate functions to components and define APIs, events, data ownership and deployment contracts.
7. **Delivery** — derive implementation slices, dependencies and bounded task contracts rather than unconstrained prose tasks.
8. **Proof** — bind scenarios and test obligations to concrete implementation/test evidence and detect drift when either side changes.

## De-facto-standard ambition

For Plumb to become a standard rather than merely an application, its semantic model should be openly documented and machine consumable. The commercial moat should come from compiler quality, rule coverage, inference quality, collaboration/governance, connectors, architecture reasoning, verification and enterprise operation—not from making the specification format proprietary.

The long-term network effect is other tools consuming or producing Plumb-compatible specification artifacts: IDEs, agent frameworks, test systems, CI/CD systems, architecture tools and consultancies.
