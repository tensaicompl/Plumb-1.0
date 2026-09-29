# Requirements-to-Architecture Research Notes

_Date: September 2026_

## Objective

Identify proven approaches for moving from software requirements into visual models, architecture, interfaces and implementable delivery plans without collapsing all concerns into one notation.

## Strongest patterns found

### EARS / ISO 29148 — requirements normalization

EARS is useful because it constrains natural-language requirements without demanding a heavy formal notation. ISO/IEC/IEEE 29148 provides a broader requirements-engineering quality framework.

Plumb use:
- lint and normalization profile,
- human-readable requirement rendering,
- rule-pack mappings.

Do not make EARS the canonical semantic representation; Plumb's graph must be richer.

References:
- https://www.iso.org/standard/72089.html
- https://www.researchgate.net/publication/224079416_Easy_approach_to_requirements_syntax_EARS

### BPMN — process semantics

BPMN is the dominant process notation and provides a standardized semantic interchange format.

Plumb use:
- support a software-focused semantic subset,
- map Plumb process semantics to/from BPMN,
- keep Plumb's own semantic graph canonical.

Reference:
- https://www.omg.org/spec/BPMN/

### DMN — business decisions

DMN provides an established representation for decision tables and decision requirements.

Plumb use:
- align decision-table semantics with DMN,
- support import/export later,
- derive coverage/gap analysis from typed rule spaces.

Reference:
- https://www.omg.org/spec/DMN/

### C4 + Structurizr / LikeC4 — software architecture views

The most important lesson is **one architecture model, many views**. C4's context/container/component/dynamic/deployment levels are well suited to software and more accessible than general-purpose UML/SysML for most product teams.

Plumb use:
- principal technical architecture visual vocabulary,
- context/container as standard views,
- component only when useful,
- dynamic/sequence and deployment as supporting views.

References:
- https://c4model.com/
- https://docs.structurizr.com/
- https://github.com/likec4/likec4

### ISO/IEC/IEEE 42010 — viewpoints

42010 reinforces that architecture descriptions should be organized into viewpoints/models serving different stakeholder concerns rather than one universal diagram.

Plumb use:
- first-class `ViewDefinition`,
- explicit audience/purpose per diagram,
- multiple projections over one canonical graph.

Reference:
- https://www.iso.org/standard/74393.html

### SEI ADD / QAW / ATAM — requirements to architecture reasoning

This is the strongest methodological bridge from requirements into architecture.

Attribute-Driven Design uses:
- functional requirements,
- quality-attribute scenarios,
- constraints,
then selects architectural tactics/patterns and allocates responsibilities.

QAW/ATAM emphasize measurable quality scenarios and architecture trade-offs rather than vague NFR adjectives.

Plumb use:
- `QualityScenario` as first-class architecture driver,
- `ArchitectureDriver` compiled from function + quality + constraint,
- candidate architectures,
- explicit trade-off/decision records,
- architecture findings when a driver lacks mitigation.

References:
- https://www.sei.cmu.edu/library/attribute-driven-design-method-collection/
- https://www.sei.cmu.edu/library/the-architecture-tradeoff-analysis-method-2/

### Arcadia / Eclipse Capella — need → functional → logical → physical

Arcadia is highly relevant to the desired Plumb progression. It separates operational need analysis, system/function analysis, logical architecture and physical architecture while maintaining allocation and traceability.

Plumb use:
- keep requirement/functional truth separate from solution architecture,
- introduce explicit `allocated_to` relationships,
- support architecture candidates without mutating functional truth,
- maintain end-to-end trace from need to implementing component.

Do not copy Capella's systems-engineering-heavy UX. Plumb should be software-specific and AI-native.

References:
- https://www.eclipse.org/capella/
- https://www.eclipse.org/community/eclipse_newsletter/2017/december/article3.php

### SysML v2 — semantic model + textual/graphical/API forms

SysML v2 demonstrates a modern model-based architecture with formal semantics, requirements, behavior, structure, verification, textual/graphical views and APIs.

Plumb use:
- study the separation of semantics from presentation,
- keep machine-readable model APIs central,
- do not adopt the full systems-engineering ontology.

Reference:
- https://www.omg.org/spec/SysML/2.0/

### OpenAPI / AsyncAPI / Arazzo — technical contracts

Once architecture is accepted, Plumb should align interface artifacts with ecosystem standards rather than invent proprietary protocol schemas.

Plumb use:
- OpenAPI for HTTP interfaces,
- AsyncAPI for messaging/event interfaces,
- Arazzo where cross-operation technical workflows are valuable.

References:
- https://spec.openapis.org/oas/latest.html
- https://www.asyncapi.com/docs/reference/specification/latest
- https://spec.openapis.org/arazzo/latest.html

### NIST RBAC — authorization structure

Functional roles and security roles should not be conflated. NIST RBAC provides the conceptual separation needed for user-role assignment, permission-role assignment, role hierarchy and separation of duty.

Plumb use:
- Actor,
- BusinessRole,
- SecurityRole,
- Permission,
- ResourceScope,
- policy constraints / separation-of-duty.

Reference:
- https://csrc.nist.gov/projects/role-based-access-control

## Synthesis for Plumb

The most successful approaches are specialized rather than universal:

- prose requirements use a requirements discipline,
- process uses BPMN-like semantics,
- decisions use DMN-like semantics,
- software architecture uses C4 views,
- quality requirements use measurable scenarios,
- architecture decisions use ADR/trade-off records,
- interfaces use OpenAPI/AsyncAPI.

Plumb's opportunity is not to replace these notations with a proprietary mega-language. It is to unify them through one software-specific semantic graph and one decision/provenance system.

## Design principle

**Own the semantic integration layer; interoperate with successful external notations.**
