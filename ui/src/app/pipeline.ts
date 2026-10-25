import { useQuery } from "@tanstack/react-query";

import { apiClient, readData } from "../api/client";
import type { components } from "../api/schema";
import { t, type MessageKey } from "../i18n";
import { useRunStore, type RunJob } from "./run";

/** A pipeline stage the server can run (`run_stage`). */
export type StageName = components["schemas"]["StageName"];

/**
 * The stages offered in the Run step menu, in pipeline order (the HR pilot simulation order).
 * `intake` is excluded: importing a document is a Sources action.
 */
export const RUNNABLE_STAGES: readonly StageName[] = ["extract", "consolidate", "lint", "directory", "analyse", "flows", "calc", "rules", "scenarios", "tags", "run"];

const STAGE_LABEL: Readonly<Record<StageName, MessageKey>> = {
  intake: "step.intake",
  extract: "step.extract",
  consolidate: "step.consolidate",
  lint: "step.lint",
  directory: "step.directory",
  analyse: "step.analyse",
  flows: "step.flows",
  calc: "step.calc",
  rules: "step.rules",
  scenarios: "step.scenarios",
  tags: "step.tags",
  run: "step.run",
};

/** The plain-language name of a stage ("Requirements" for extract). */
export function stageLabel(stage: StageName): string {
  return t(STAGE_LABEL[stage]);
}

/** One pipeline step's state, as the Overview stepper and the Run menu show it. */
export type StepState = "done" | "needs_review" | "running" | "failed" | "stale" | "not_started";

export interface PipelineStep {
  readonly stage: StageName;
  readonly state: StepState;
  /** 0–1 while running. */
  readonly progress?: number;
  /** Items waiting for review, when `needs_review`. */
  readonly reviewCount?: number;
  /** The plain failure, when `failed`. */
  readonly error?: string | null;
}

/** What the server knows about the pipeline: each step's state and the next step to run. */
export interface PipelineSnapshot {
  readonly steps: readonly PipelineStep[];
  readonly next: StageName | null;
}

const ACTIVE = new Set(["queued", "acquiring", "evaluating"]);

/** Whether a stage job is still in flight. */
export function isJobActive(job: RunJob | null | undefined): boolean {
  return job !== null && job !== undefined && ACTIVE.has(job.status);
}

/** The step a locally tracked job contributes to the snapshot. */
export function jobStep(job: RunJob): PipelineStep {
  if (job.status === "failed") {
    return { stage: job.stage, state: "failed", error: job.error };
  }
  if (job.status === "stale") {
    return { stage: job.stage, state: "stale" };
  }
  return isJobActive(job) ? { stage: job.stage, state: "running", progress: job.progress } : { stage: job.stage, state: "done" };
}

type PipelineResponse = components["schemas"]["PipelineResponse"];
type ServerStep = components["schemas"]["PipelineStep"];

/** GET /api/pipeline (`get_pipeline`): every stage recipe's state and the next step. Key ["pipeline"]. */
export const pipelineQuery = {
  queryKey: ["pipeline"] as const,
  queryFn: (): Promise<PipelineResponse> => readData(apiClient.GET("/api/pipeline")),
};

const STEP_STATE: Readonly<Record<ServerStep["status"], StepState>> = {
  not_started: "not_started",
  queued: "running",
  running: "running",
  needs_review: "needs_review",
  done: "done",
  failed: "failed",
  stale: "stale",
};

/** One server step as the snapshot shows it. */
export function serverStep(step: ServerStep): PipelineStep {
  return {
    stage: step.recipe,
    state: STEP_STATE[step.status],
    ...(typeof step.progress === "number" ? { progress: step.progress } : {}),
    reviewCount: step.pending_proposals,
    error: step.error?.message ?? null,
  };
}

/**
 * The pipeline snapshot: the single seam for pipeline data. The server's `get_pipeline` steps,
 * with the stage job this browser tracks laid over its own step (it is fresher while it runs).
 */
export function usePipeline(): PipelineSnapshot {
  const job = useRunStore((state) => state.job);
  const pipeline = useQuery(pipelineQuery);
  const server = Array.isArray(pipeline.data?.steps) ? pipeline.data.steps.map(serverStep) : [];
  const local = job === null ? null : jobStep(job);
  const steps = local === null ? server : server.some((s) => s.stage === local.stage) ? server.map((s) => (s.stage === local.stage ? local : s)) : [...server, local];
  return { steps, next: pipeline.data?.next ?? null };
}
