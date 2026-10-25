// HR leave pilot scenarios shaped like the real API (canonical JSON facts, plumb-sim traces).
import type { components } from "../api/schema";

type Scenario = components["schemas"]["Scenario"];
type ScenarioRun = components["schemas"]["ScenarioRun"];

const step = (phase: "execution" | "comparison", fields: Record<string, unknown>) => JSON.stringify({ phase, step: fields });

export const PASS_RUN: ScenarioRun = {
  result: "pass",
  diff: null,
  missing: null,
  trace: [
    step("execution", { scenario_ref: "scenario:submit", step: "materialization_started" }),
    step("execution", { attribute_refs: [], entity_refs: [], state_refs: ["state:draft"], step: "given_materialized" }),
    step("execution", { operation_ref: "op:submit", performer_refs: ["actor:employee"], step: "performer_recorded" }),
    step("execution", { from_state: "state:draft", step: "transition_applied", to_state: "state:submitted", transition_ref: "tr:submit" }),
    step("execution", { event_ref: "event:submitted", step: "event_emitted" }),
    step("execution", { step: "execution_completed" }),
    step("comparison", { scenario_ref: "scenario:submit", step: "comparison_started" }),
    step("comparison", { matched: true, state_ref: "state:submitted", step: "state_compared" }),
    step("comparison", { diff_count: 0, result: "pass", step: "comparison_completed" }),
  ],
};

export const FAIL_RUN: ScenarioRun = {
  result: "fail",
  diff: ['{"actual":"state:submitted","expected":"state:approved","kind":"state"}'],
  missing: null,
  trace: [
    step("execution", { attribute_refs: [], entity_refs: [], state_refs: ["state:submitted"], step: "given_materialized" }),
    step("execution", { operation_ref: "op:approve", performer_refs: ["actor:manager"], step: "performer_recorded" }),
    step("comparison", { matched: false, state_ref: "state:approved", step: "state_compared" }),
    step("comparison", { diff_count: 1, result: "fail", step: "comparison_completed" }),
  ],
};

export const UNDECIDABLE_RUN: ScenarioRun = {
  result: "undecidable",
  diff: null,
  missing: ['{"key":"condition_expr","kind":"rule_condition","owner_ref":"rule:authorization"}'],
  trace: [
    step("execution", { attribute_refs: [], entity_refs: [], state_refs: ["state:submitted"], step: "given_materialized" }),
    step("execution", { missing: [{ key: "condition_expr", kind: "rule_condition", owner_ref: "rule:authorization" }], step: "semantic_missing" }),
    "not a trace step",
  ],
};

export const SCENARIOS: Scenario[] = [
  {
    id: "scenario:submit",
    given: ['{"states_active":["state:draft"]}'],
    when: '{"operation":"op:submit"}',
    then: ['{"states_active":["state:submitted"]}'],
    status: "Accepted",
    confirmation_status: "Confirmed",
    derived_from: ["tr:submit"],
    run: PASS_RUN,
  },
  {
    id: "scenario:approve",
    given: ['{"states_active":["state:submitted"]}'],
    when: '{"operation":"op:approve"}',
    then: ['{"states_active":["state:approved"]}'],
    status: "Accepted",
    confirmation_status: "Confirmed",
    derived_from: ["tr:approve"],
    run: FAIL_RUN,
  },
  {
    id: "scenario:contractor",
    given: ['{"decision_inputs":{"0":{"kind":"enum","value":"Contractor"},"1":{"kind":"enum","value":"Annual"}}}'],
    when: '{"decision_table":"dt:eligibility"}',
    then: ['{"decision_outputs":{"0":{"kind":"bool","value":false}}}'],
    status: "Proposed",
    confirmation_status: "Derived",
    derived_from: ["dt:eligibility"],
    run: null,
  },
  {
    id: "scenario:authorize",
    given: ['{"states_active":["state:submitted"]}'],
    when: '{"operation":"op:approve"}',
    then: ['{"states_active":["state:approved"]}'],
    status: "Accepted",
    confirmation_status: "Confirmed",
    derived_from: ["tr:approve"],
    run: UNDECIDABLE_RUN,
  },
];
