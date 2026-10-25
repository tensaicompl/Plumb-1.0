// The README "Next action" priority order: running → failed step → pending proposals → open
// questions → stale / unchecked gate → next pipeline step → blocked gate → ready (Export).
import { describe, expect, it } from "vitest";

import type { components } from "../api/schema";
import { computeNextAction, type NextActionInput } from "./nextAction";

type GateStatus = components["schemas"]["GateStatus"];

function gate(name: GateStatus["gate"], state: GateStatus["state"], passed: boolean): GateStatus {
  return { gate: name, state, passed, reason: null, findings: [], rules: [], waivers: [], summary: { blocker_failed: passed ? 0 : 1, blocker_waived: 0, warnings: 0 } };
}

const ALL_PASS: GateStatus[] = (["I0", "F1", "F2", "F3", "F4"] as const).map((g) => gate(g, "current", true));

const base: NextActionInput = { job: null, pipeline: undefined, proposals: 0, openQuestions: 0, gates: ALL_PASS };

describe("computeNextAction", () => {
  it("waits for the counts and gates before deciding", () => {
    expect(computeNextAction({ ...base, proposals: undefined })).toBeNull();
    expect(computeNextAction({ ...base, openQuestions: undefined })).toBeNull();
    expect(computeNextAction({ ...base, gates: undefined })).toBeNull();
  });

  it("shows a running step first, with its progress", () => {
    const action = computeNextAction({ ...base, proposals: 3, job: { id: "job:1", stage: "flows", status: "evaluating", progress: 0.75, error: null } });
    expect(action?.kind).toBe("running");
    expect(action?.short).toBe("Analysing… 75%");
    expect(action?.title).toBe("Plumb is running the Flows step… 75%");
    expect(action?.progress).toBe(75);
    expect(action?.target).toEqual({ type: "none" });
  });

  it("then a failed step with Retry", () => {
    const action = computeNextAction({ ...base, proposals: 3, job: { id: "job:1", stage: "extract", status: "failed", progress: 1, error: "E_INFERENCE_FAILED: timeout" } });
    expect(action?.kind).toBe("retry");
    expect(action?.title).toBe("Requirements failed — Retry");
    expect(action?.description).toBe("E_INFERENCE_FAILED: timeout");
    expect(action?.target).toEqual({ type: "run", stage: "extract" });
    // A failed step reported by the pipeline counts the same way.
    const fromPipeline = computeNextAction({ ...base, pipeline: { steps: [{ stage: "lint", state: "failed", error: null }], next: null } });
    expect(fromPipeline?.kind).toBe("retry");
    expect(fromPipeline?.target).toEqual({ type: "run", stage: "lint" });
    expect(fromPipeline?.description).toBe("");
  });

  it("then pending proposals, then open questions", () => {
    const proposals = computeNextAction({ ...base, proposals: 7, openQuestions: 2 });
    expect(proposals?.kind).toBe("proposals");
    expect(proposals?.short).toBe("Review 7 proposals");
    expect(proposals?.target).toEqual({ type: "route", to: "/inbox" });
    expect(computeNextAction({ ...base, proposals: 1 })?.short).toBe("Review 1 proposal");
    const questions = computeNextAction({ ...base, openQuestions: 2 });
    expect(questions?.kind).toBe("questions");
    expect(questions?.short).toBe("Answer 2 questions");
    expect(questions?.title).toBe("2 questions are waiting for an answer");
    expect(questions?.target).toEqual({ type: "route", to: "/inbox", search: { filter: "questions" } });
  });

  it("then a stale or unchecked gate, then the next pipeline step, then a blocked gate", () => {
    const stale = computeNextAction({ ...base, gates: [gate("I0", "current", true), gate("F2", "stale", false)] });
    expect(stale?.kind).toBe("staleGate");
    expect(stale?.short).toBe("Re-check Model");
    expect(stale?.target).toEqual({ type: "route", to: "/quality/$gate", params: { gate: "F2" } });
    // A gate absent from the response has never been evaluated.
    const unchecked = computeNextAction({ ...base, gates: ALL_PASS.filter((g) => g.gate !== "F4") });
    expect(unchecked?.kind).toBe("staleGate");
    expect(unchecked?.title).toBe("The Behaviour check has not run yet");
    const step = computeNextAction({ ...base, gates: ALL_PASS.map((g) => (g.gate === "F3" ? gate("F3", "current", false) : g)), pipeline: { steps: [], next: "scenarios" } });
    expect(step?.kind).toBe("step");
    expect(step?.short).toBe("Run Scenarios");
    expect(step?.target).toEqual({ type: "run", stage: "scenarios" });
    const blocked = computeNextAction({ ...base, gates: ALL_PASS.map((g) => (g.gate === "F3" ? gate("F3", "current", false) : g)) });
    expect(blocked?.kind).toBe("blockedGate");
    expect(blocked?.title).toBe("The Decisions gate is blocked");
  });

  it("is ready to export when nothing is waiting and every gate passes", () => {
    const ready = computeNextAction(base);
    expect(ready?.kind).toBe("ready");
    expect(ready?.short).toBe("Export");
    expect(ready?.title).toBe("Your specification is ready");
    expect(ready?.target).toEqual({ type: "export" });
    // A finished job does not block the next decision.
    expect(computeNextAction({ ...base, job: { id: "job:1", stage: "flows", status: "committed", progress: 1, error: null } })?.kind).toBe("ready");
  });
});
