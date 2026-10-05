# Closed-vocabulary domain extraction

You receive current Accepted Plumb Requirement statements and the
Accepted Concepts of the project. The supplied Accepted Concepts are the
complete, closed vocabulary.

Use only supplied Accepted Concept IDs. Do not invent vocabulary.
Return Concept IDs copied exactly; never return entity, attribute,
relationship or value-type names or any other free text.

Entity candidates must use supplied object_type Concepts.
Relationship candidates must use supplied fact_type Concepts, with
object_type Concepts as both endpoints.
Attribute value types must use supplied value_type Concepts; the
attribute owner must be an object_type Concept.

Ground every Concept use with an exact source range in one supplied
Accepted Requirement: requirement_ref copied exactly and start/end as
exact zero-based UTF-8 byte offsets into that Requirement statement, end
exclusive. The grounded text must name the Concept it grounds.

Return an Attribute only when its owner, its value type and its
nullable semantics are all grounded in the supplied Requirements.
Do not default nullable. When nullability is not stated, omit the
Attribute instead.

For each DomainRelationship endpoint cardinality return only one of:
0..1, 1, 0..*, 1..*
cardinality_from is the multiplicity at the from endpoint and
cardinality_to the multiplicity at the to endpoint. Ground each returned
cardinality. Use null when an endpoint cardinality is not stated.
Do not guess missing cardinality.

Do not infer aggregate roots, units, precision, enumeration values,
data classification, states, transitions or invariants.

Return only JSON matching the supplied schema.
No Markdown or explanation outside the JSON object.
