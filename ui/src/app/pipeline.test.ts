// The pipeline seam (S1): GET /api/pipeline mapped onto the step states the shell and the Overview
// show, with the stage job this browser tracks merged over the server's view of that stage.
import { describe, expect, it } from "vitest";

import type { components } from "../api/schema";
import { INFERENCE_STAGES, mergeJob, RUNNABLE_STAGES, snapshotFromResponse, stageLabel, usesInference } from "./pipeline";

type PipelineResponse = components["schemas"]["PipelineResponse"];
type PipelineStepWire = components["schemas"]["PipelineStep"];

function wire(recipe: PipelineStepWire["recipe"], status: PipelineStepWire["status"], extra: Partial<PipelineStepWire> = {}): PipelineStepWire {
  return { recipe, status, job_id: null, progress: null, pending_proposals: 0, error: null, updated_at: null, missing_config: [], ...extra };
}

const RESPONSE: PipelineResponse = {
  steps: [
    wire("extract", "done", { job_id: "job:x", progress: 1 }),
    wire("consolidate", "needs_review", { job_id: "job:c", progress: 1, pending_proposals: 4 }),
    wire("lint", "failed", { job_id: "job:l", progress: 1, error: { code: "E_STAGE_EVALUATION_FAILED", message: "The lint step failed." } }),
    wire("flows", "queued", { job_id: "job:f", progress: 0 }),
    wire("calc", "running", { job_id: "job:k", progress: 0.4 }),
    wire("rules", "not_started", { missing_config: ["gates.f1"] }),
    wire("run", "stale", { job_id: "job:r", progress: 1 }),
  ],
  sources: 1,
  open_questions: 2,
  next: "directory",
};

describe("snapshotFromResponse", () => {
  it("maps every recipe's status, progress, review count, error, job and missing configuration", () => {
    const snapshot = snapshotFromResponse(RESPONSE);
    expect(snapshot.next).toBe("directory");
    expect(snapshot.sources).toBe(1);
    expect(snapshot.openQuestions).toBe(2);
    expect(snapshot.loaded).toBe(true);
    expect(snapshot.steps.map((s) => [s.stage, s.state])).toEqual([
      ["extract", "done"],
      ["consolidate", "needs_review"],
      ["lint", "failed"],
      ["flows", "running"],
      ["calc", "running"],
      ["rules", "not_started"],
      ["run", "stale"],
    ]);
    const [extract, consolidate, lint, flows, calc, rules] = snapshot.steps;
    expect(extract?.jobId).toBe("job:x");
    expect(consolidate?.reviewCount).toBe(4);
    expect(lint?.error).toBe("The lint step failed.");
    expect(lint?.errorCode).toBe("E_STAGE_EVALUATION_FAILED");
    expect(flows?.progress).toBe(0);
    expect(calc?.progress).toBe(0.4);
    expect(rules?.missingConfig).toEqual(["gates.f1"]);
  });
});

describe("mergeJob", () => {
  it("lets the tracked job replace the server's view of its stage", () => {
    const snapshot = snapshotFromResponse(RESPONSE);
    const merged = mergeJob(snapshot, { id: "job:new", stage: "extract", status: "evaluating", progress: 0.5, error: null });
    expect(merged.steps.find((s) => s.stage === "extract")).toMatchObject({ state: "running", progress: 0.5, jobId: "job:new" });
    expect(merged.steps).toHaveLength(snapshot.steps.length);
    // A stage the server did not list is appended.
    const appended = mergeJob(snapshot, { id: "job:t", stage: "tags", status: "failed", progress: 1, error: "boom" });
    expect(appended.steps.at(-1)).toMatchObject({ stage: "tags", state: "failed", error: "boom", jobId: "job:t" });
  });

  it("keeps the server's view once the tracked job is finished, so pending proposals still show", () => {
    const snapshot = snapshotFromResponse(RESPONSE);
    const merged = mergeJob(snapshot, { id: "job:c", stage: "consolidate", status: "committed", progress: 1, error: null });
    expect(merged.steps.find((s) => s.stage === "consolidate")?.state).toBe("needs_review");
  });

  it("is the job alone before the pipeline has loaded", () => {
    const merged = mergeJob(undefined, { id: "job:1", stage: "flows", status: "queued", progress: 0, error: null });
    expect(merged).toEqual({ steps: [{ stage: "flows", state: "running", progress: 0, jobId: "job:1" }], next: null, loaded: false });
    expect(mergeJob(undefined, null)).toEqual({ steps: [], next: null, loaded: false });
  });
});

describe("usesInference", () => {
  it("names the recipes with an inference phase", () => {
    expect(INFERENCE_STAGES).toEqual(["extract", "consolidate", "analyse", "flows", "calc", "scenarios", "tags"]);
    expect(usesInference("lint")).toBe(false);
    expect(usesInference("extract")).toBe(true);
  });
});

describe("stageLabel", () => {
  it("names each step after the Overview stepper item it belongs to", () => {
    // Consolidate runs under "Glossary & quality" on the stepper, not under "Model" (analyse).
    expect(stageLabel("consolidate")).toBe("Glossary");
    expect(stageLabel("lint")).toBe("Glossary & quality");
    expect(stageLabel("analyse")).toBe("Analysis");
    expect(new Set(RUNNABLE_STAGES.map(stageLabel)).size).toBe(RUNNABLE_STAGES.length);
  });
});
