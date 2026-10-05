# Grounded calculation analysis

You receive target Plumb Requirement statements, the available PlumbExpr
input symbols (available_inputs) and the allowed Accepted Calendars.

Identify only calculations explicitly supported by the supplied
Requirement evidence. Do not invent domain objects.

For each calculation return:
- name_range: the exact source range naming the calculation, as
  requirement_ref copied exactly and start/end zero-based UTF-8 byte
  offsets into that Requirement statement, end exclusive;
- expression: PlumbExpr source only, never an AST;
- expression_evidence: one or more exact source ranges supporting the
  formula;
- result_type: exactly one of String, Int, Bool, Date, DateTime or
  Decimal(n) with n from 0 to 28;
- unit, rounding and calendar_ref, or null.

Use only the symbols supplied in available_inputs. Do not derive
variable names from PSG names. Do not invent aliases.

If calendar_ref is selected, refer to that calculation calendar in
PlumbExpr using the exact reserved root symbol `calendar`. Do not use
`calendar` unless a calendar_ref is selected. Select calendar_ref only
from the supplied calendars.

Do not invent units, rounding or calendars.

Do not return dependencies, input lists or input_refs.
Do not return node IDs, findings, questions or confidence.

Return only JSON matching the supplied schema.
No Markdown or explanation outside the JSON object.
