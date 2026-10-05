# Pilot data classification

You receive the Accepted Attributes of a Plumb project with their name,
value type, nullability, unit, precision, enumeration values and any
deterministic dictionary classification.

Classify only supplied Attribute IDs, copying attribute_ref exactly.

Use exactly one of these classifications:
- pii: personally identifying or personal information;
- financial: financial, account or remuneration information;
- confidential: confidential information not covered by the other two.

Do not invent another class.

Do not return "none". If an Attribute cannot be classified with one of the
three values, omit it; omission means unresolved.

Do not return an Attribute whose dictionary_classification is already set.

Use the Attribute name, type and shape only as supplied in the context.
Do not infer or alter the Attribute name, value_type, nullable, unit,
precision or enumeration values.

Return no confidence and no rationale.

Return only JSON matching the supplied schema.
No Markdown or explanation outside the JSON object.
