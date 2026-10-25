// The Overview's 9 designed pipeline steps over the server's 11 recipes (handoff "1. Overview"
// item 4): Import ← sources; Requirements ← extract; Glossary & quality ← consolidate + lint;
// Model ← directory + analyse; Flows ← flows; Calculations & rules ← calc + rules;
// Scenarios ← scenarios + tags + run; Questions ← open questions; Ready ← gates.
import { describe, expect, it } from "vitest";

import type { components } from "../../api/schema";
import type { PipelineSnapshot, PipelineStep } from "../../app/pipeline";
import { combineSteps, OVERVIEW_STEP_IDS, overviewSteps } from "./steps";

type GateStatus = components["schemas"]["GateStatus"];

function gate(name: GateStatus["gate"], passed: boolean, state: GateStatus["state"] = "current"): GateStatus {
  return { gate: name, state, passed, reason: null, findings: [], rules: [], waivers: [], summary: { blocker_failed: passed ? 0 : 1, blocker_waived: 0, warnings: 0 } };
}
const ALL_PASS = (["I0", "F1", "F2", "F3", "F4"] as const).map((g) => gate(g, true));

function step(stage: PipelineStep["stage"], state: PipelineStep["state"], extra: Partial<PipelineStep> = {}): PipelineStep {
  return { stage, state, ...extra };
}

const DONE: PipelineStep[] = (["extract", "consolidate", "lint", "directory", "analyse", "flows", "calc", "rules", "scenarios", "tags", "run"] as const).map((s) => step(s, "done"));

function snapshot(steps: PipelineStep[], extra: Partial<PipelineSnapshot> = {}): PipelineSnapshot {
  return { steps, next: null, sources: 1, openQuestions: 0, loaded: true, ...extra };
}

describe("combineSteps", () => {
  it("is running first, then failed, needs review (summed), outdated, done, not started", () => {
    expect(combineSteps([])).toEqual({ state: "not_started" });
    expect(combineSteps([step("calc", "done"), step("rules", "running", { progress: 0.4 })])).toEqual({ state: "running", progress: 0.4 });
    expect(combineSteps([step("calc", "failed", { error: "boom", jobId: "job:c" }), step("rules", "needs_review", { reviewCount: 2 })])).toEqual({
      state: "failed",
      error: "boom",
      jobId: "job:c",
      stage: "calc",
    });
    expect(combineSteps([step("scenarios", "needs_review", { reviewCount: 2 }), step("tags", "needs_review", { reviewCount: 3 }), step("run", "done")])).toEqual({
      state: "needs_review",
      reviewCount: 5,
    });
    expect(combineSteps([step("calc", "stale"), step("rules", "done")])).toEqual({ state: "stale" });
    // An optional recipe that never ran does not hold back a step whose other recipe is done.
    expect(combineSteps([step("directory", "not_started"), step("analyse", "done")])).toEqual({ state: "done" });
    expect(combineSteps([step("directory", "not_started"), step("analyse", "not_started")])).toEqual({ state: "not_started" });
  });
});

describe("overviewSteps", () => {
  it("lists the nine designed steps in order, each with what it produced", () => {
    const steps = overviewSteps(snapshot(DONE), ALL_PASS);
    expect(steps.map((s) => s.id)).toEqual(OVERVIEW_STEP_IDS);
    expect(steps.map((s) => s.href)).toEqual([
      "/sources",
      "/requirements",
      "/model/glossary",
      "/model/entities",
      "/model/processes",
      "/model/calculations",
      "/scenarios",
      "/inbox?filter=questions",
      "/quality",
    ]);
    expect(steps.every((s) => s.state === "done")).toBe(true);
  });

  it("maps recipes onto their step and links a step needing review to the Inbox", () => {
    const steps = overviewSteps(
      snapshot(
        DONE.map((s) => (s.stage === "flows" ? step("flows", "needs_review", { reviewCount: 7 }) : s.stage === "directory" ? step("directory", "not_started") : s)),
        { openQuestions: 2 },
      ),
      ALL_PASS.map((g) => (g.gate === "F3" ? gate("F3", false) : g)),
    );
    const byId = new Map(steps.map((s) => [s.id, s]));
    expect(byId.get("flows")).toMatchObject({ state: "needs_review", reviewCount: 7, href: "/inbox" });
    expect(byId.get("model")?.state).toBe("done");
    expect(byId.get("questions")).toMatchObject({ state: "needs_review", reviewCount: 2, href: "/inbox?filter=questions" });
    expect(byId.get("ready")?.state).toBe("not_started");
  });

  it("is all not started for a new project, and Import is done once a source exists", () => {
    const empty = overviewSteps(snapshot([], { sources: 0 }), []);
    expect(empty.every((s) => s.state === "not_started")).toBe(true);
    const imported = overviewSteps(snapshot([], { sources: 1 }), []);
    expect(imported[0]?.state).toBe("done");
    expect(imported.slice(1).every((s) => s.state === "not_started")).toBe(true);
  });

  it("is Ready only when every gate is current and passes", () => {
    expect(overviewSteps(snapshot(DONE), ALL_PASS.map((g) => (g.gate === "F2" ? gate("F2", true, "stale") : g))).at(-1)?.state).toBe("not_started");
    expect(overviewSteps(snapshot(DONE), ALL_PASS.slice(1)).at(-1)?.state).toBe("not_started");
  });
});
