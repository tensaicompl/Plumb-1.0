// Plain-language readings of scenario facts and plumb-sim trace steps (every step kind the
// simulator emits; unknown material stays exact text).
import { describe, expect, it } from "vitest";

import { describeFact, describeWhen, elementHref, parseDiff, parseMissing, readTrace, scenarioSource, scenarioTitle } from "./facts";

const label = (id: string) => ({ "op:x": "Approve", "state:a": "Draft", "state:b": "Submitted", "rule:r": "Authorization", "calc:c": "Duration", "dt:t": "Eligibility", "attr:days": "Days", "event:e": "Approved", "outcome:o": "Succeeded", "req:1": "HR-001" })[id] ?? id;
const step = (phase: string, fields: Record<string, unknown>) => JSON.stringify({ phase, step: fields });

describe("trace steps", () => {
  it("reads every step kind and keeps unknown material", () => {
    const steps = readTrace(
      [
        step("execution", { step: "materialization_started" }),
        step("execution", { step: "footprint_declared", operation_ref: "op:x", writes: ["attr:days"] }),
        step("execution", { step: "footprint_declared", operation_ref: "op:x", writes: [] }),
        step("execution", { step: "authorization_checked", decision: { decision: "allowed" } }),
        step("execution", { step: "rule_evaluated", rule_ref: "rule:r", result: true }),
        step("execution", { step: "guard_evaluated" }),
        step("execution", { step: "decision_table_evaluated", decision_table_ref: "dt:t", row: 0 }),
        step("execution", { step: "decision_table_evaluated", decision_table_ref: "dt:t", row: null }),
        step("execution", { step: "calculation_evaluated", calculation_ref: "calc:c", value: 5 }),
        step("execution", { step: "outcome_selected", outcome_ref: "outcome:o", operation_ref: "op:x" }),
        step("execution", { step: "event_occurred", event_ref: "event:e" }),
        step("execution", { step: "quantum_leap" }),
        step("comparison", { step: "events_compared", matched: true }),
        step("comparison", { step: "attribute_compared", attribute_ref: "attr:days", matched: false }),
        step("comparison", { step: "comparison_completed", result: "undecidable" }),
        step("comparison", { step: "comparison_completed", result: "fail", diff_count: 2 }),
        JSON.stringify({ phase: "execution" }),
      ],
      label,
    );
    expect(steps.map((s) => [s.title, s.detail, s.status])).toEqual([
      ["Declared what Approve changes", "writes Days", "pass"],
      ["Declared what Approve changes", null, "pass"],
      ["Checked authorization", "allowed", "pass"],
      ["Evaluated Authorization", "true", "pass"],
      ["Checked a guard", null, "pass"],
      ["Evaluated Eligibility", "row 1 applies", "pass"],
      ["Evaluated Eligibility", "no row applies", "pass"],
      ["Calculated Duration", "5", "pass"],
      ["Outcome: Succeeded", "Approve", "pass"],
      ["Produced Approved", null, "pass"],
      ["quantum leap", null, "pass"],
      ["Expected events", "matched", "pass"],
      ["Expected Days", "did not match", "fail"],
      ["Compared the outcome", "could not be decided", "missing"],
      ["Compared the outcome", "2 differences", "fail"],
      ["Unrecognised step", JSON.stringify({ phase: "execution" }), "unknown"],
    ]);
  });

  it("parses missing semantics and diffs, or gives up", () => {
    expect(parseMissing('{"key":"k","kind":"rule_condition","owner_ref":"rule:r"}')).toEqual({ owner: "rule:r", key: "k", kind: "rule_condition" });
    expect(parseMissing("[]")).toBeNull();
    expect(parseDiff('{"actual":5,"expected":"state:a"}', label)).toEqual({ expected: "Draft", actual: "5" });
    expect(parseDiff("nope", label)).toBeNull();
  });
});

describe("facts", () => {
  const scenario = (given: string[], when: string, then: string[], derived: string[] = []) => ({
    id: "scenario:1",
    given,
    when,
    then,
    status: "Accepted" as const,
    confirmation_status: "Confirmed" as const,
    derived_from: derived,
    run: null,
  });

  it("reads other fact shapes and falls back to the exact text", () => {
    expect(describeFact('{"states_active":["state:a","state:b"]}', label, [])).toBe("the states are Draft, Submitted");
    expect(describeFact('{"state":"state:b"}', label, [])).toBe("the state is Submitted");
    expect(describeFact('{"event":"event:e"}', label, [])).toBe("the event Approved occurs");
    expect(describeFact('{"attribute":"attr:days","value":2}', label, [])).toBe("Days is 2");
    expect(describeFact('{"decision_outputs":{"0":{"kind":"int","value":3}}}', label, [])).toBe("column 1 is 3");
    expect(describeFact('{"other":1}', label, [])).toBe('{"other":1}');
    expect(describeFact("not json", label, [])).toBe("not json");
    expect(describeWhen('{"transition":"tr:1"}', label)).toBe("tr:1 happens");
    expect(describeWhen('{"calculation":"calc:c"}', label)).toBe("Duration is calculated");
    expect(describeWhen('{"event":"event:e"}', label)).toBe("Approved occurs");
    expect(describeWhen('{"other":"x"}', label)).toBe('{"other":"x"}');
  });

  it("titles and sources scenarios without known shapes", () => {
    expect(scenarioTitle(scenario([], '{"calculation":"calc:c"}', []), label)).toBe("Duration");
    expect(scenarioTitle(scenario([], "{}", []), label)).toBe("Scenario scenario:1");
    const kinds = (id: string) => ({ "req:1": "requirement", "rule:r": "rule", "calc:c": "calculation", "tr:1": "transition" })[id] ?? null;
    expect(scenarioSource(scenario([], "{}", [], ["req:1"]), label, kinds)).toBe("Requirement HR-001");
    expect(scenarioSource(scenario([], "{}", [], ["rule:r"]), label, kinds)).toBe("Rule Authorization");
    expect(scenarioSource(scenario([], "{}", [], ["calc:c"]), label, kinds)).toBe("Calculation Duration");
    expect(scenarioSource(scenario([], "{}", [], ["tr:1"]), label, kinds)).toBe("Transition tr:1");
    expect(scenarioSource(scenario([], "{}", [], ["x:1"]), label, kinds)).toBe("x:1");
    expect(scenarioSource(scenario([], "{}", []), label, kinds)).toBeNull();
  });

  it("links elements to their Model or Requirements page", () => {
    expect(elementHref("calc:c", "calculation")).toBe("/model/calculations/calc:c");
    expect(elementHref("tr:1", "transition")).toBe("/model/entities");
    expect(elementHref("req:1", "requirement")).toBe("/requirements/req:1");
    expect(elementHref("x:1", null)).toBeNull();
    expect(elementHref("x:1", "actor")).toBeNull();
  });
});
