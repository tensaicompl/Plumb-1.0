import { useQuery } from "@tanstack/react-query";

import { apiClient, readData } from "../api/client";
import type { components } from "../api/schema";
import { t, type MessageKey } from "../i18n";
import { useRunStore, type RunJob } from "./run";

/** A pipeline stage the server can run (`run_stage`). */
export type StageName = components["schemas"]["StageName"];

type PipelineResponse = components["schemas"]["PipelineResponse"];
type PipelineStepStatus = components["schemas"]["PipelineStepStatus"];

/**
 * The stages offered in the Run step menu, in pipeline order (the HR pilot simulation order).
 * `intake` is excluded: importing a document is a Sources action.
 */
export const RUNNABLE_STAGES: readonly StageName[] = ["extract", "consolidate", "lint", "directory", "analyse", "flows", "calc", "rules", "scenarios", "tags", "run"];

/**
 * The recipes with an inference phase (the server's `Recipe::uses_inference`): they cannot run
 * while the server has no AI provider (`/api/me` `inference_available: false`).
 */
export const INFERENCE_STAGES: readonly StageName[] = ["extract", "consolidate", "analyse", "flows", "calc", "scenarios", "tags"];

/** Whether `stage` needs the AI provider. */
export function usesInference(stage: StageName): boolean {
  return INFERENCE_STAGES.includes(stage);
}

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
  /** The typed code of the failure, when the server reported one. */
  readonly errorCode?: string;
  /** The latest job of this stage (the one Retry re-runs). */
  readonly jobId?: string | null;
  /** Configuration the stage needs and the server lacks (it cannot run). */
  readonly missingConfig?: readonly string[];
}

/** What the server knows about the pipeline: each step's state and the next step to run. */
export interface PipelineSnapshot {
  readonly steps: readonly PipelineStep[];
  readonly next: StageName | null;
  /** Imported sources (undefined before the pipeline read loads). */
  readonly sources?: number;
  /** Open questions (undefined before the pipeline read loads). */
  readonly openQuestions?: number;
  /** Whether the server's pipeline read has loaded. */
  readonly loaded?: boolean;
}

const ACTIVE = new Set(["queued", "acquiring", "evaluating"]);

/** Whether a stage job is still in flight. */
export function isJobActive(job: RunJob | null | undefined): boolean {
  return job !== null && job !== undefined && ACTIVE.has(job.status);
}

/** The step a locally tracked job contributes to the snapshot. */
export function jobStep(job: RunJob): PipelineStep {
  if (job.status === "failed") {
    return { stage: job.stage, state: "failed", error: job.error, jobId: job.id };
  }
  if (job.status === "stale") {
    return { stage: job.stage, state: "stale", jobId: job.id };
  }
  return isJobActive(job) ? { stage: job.stage, state: "running", progress: job.progress, jobId: job.id } : { stage: job.stage, state: "done", jobId: job.id };
}

const WIRE_STATE: Readonly<Record<PipelineStepStatus, StepState>> = {
  not_started: "not_started",
  queued: "running",
  running: "running",
  needs_review: "needs_review",
  done: "done",
  failed: "failed",
  stale: "stale",
};

/** The snapshot of a `GET /api/pipeline` response. */
export function snapshotFromResponse(response: PipelineResponse): PipelineSnapshot {
  return {
    steps: response.steps.map((step) => ({
      stage: step.recipe,
      state: WIRE_STATE[step.status],
      ...(step.progress === null || step.progress === undefined ? {} : { progress: step.progress }),
      ...(step.pending_proposals > 0 ? { reviewCount: step.pending_proposals } : {}),
      ...(step.error === null || step.error === undefined ? {} : { error: step.error.message, errorCode: step.error.code }),
      jobId: step.job_id ?? null,
      missingConfig: step.missing_config,
    })),
    next: response.next ?? null,
    sources: response.sources,
    openQuestions: response.open_questions,
    loaded: true,
  };
}

/**
 * The snapshot with the tracked job over the server's view of its stage. An active or failed job
 * replaces that step (it is newer than the last pipeline read); a finished one leaves the server's
 * view, which knows the proposals it produced.
 */
export function mergeJob(snapshot: PipelineSnapshot | undefined, job: RunJob | null): PipelineSnapshot {
  const base = snapshot ?? { steps: [], next: null, loaded: false };
  if (job === null) {
    return base;
  }
  const local = jobStep(job);
  const index = base.steps.findIndex((s) => s.stage === job.stage);
  if (index === -1) {
    return { ...base, steps: [...base.steps, local] };
  }
  if (local.state === "done" || local.state === "stale") {
    return base;
  }
  return { ...base, steps: base.steps.map((s, i) => (i === index ? { ...s, ...local } : s)) };
}

/** GET /api/pipeline. Key ["pipeline"] (design events refresh it). */
export const pipelineQuery = {
  queryKey: ["pipeline"] as const,
  queryFn: async (): Promise<PipelineSnapshot> => snapshotFromResponse(await readData(apiClient.GET("/api/pipeline"))),
};

/**
 * The pipeline snapshot: the single seam for pipeline data. It is the server's `GET
 * /api/pipeline` (every recipe's state, pending proposals, failure and missing configuration,
 * the imported sources, open questions and the next step) with the stage job this browser
 * started merged over it. `useNextAction`, the Run menu and the Overview stepper read it.
 */
export function usePipeline(): PipelineSnapshot {
  const job = useRunStore((state) => state.job);
  const pipeline = useQuery(pipelineQuery);
  // The shell never breaks a screen: a failed or malformed read counts as not loaded.
  const data = Array.isArray(pipeline.data?.steps) ? pipeline.data : undefined;
  return mergeJob(data, job);
}
