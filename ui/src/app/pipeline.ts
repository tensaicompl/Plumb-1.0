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

/**
 * The pipeline snapshot: the single seam for pipeline data.
 *
 * The contract has no pipeline read yet (a backend lane is adding `GET /api/pipeline` with step
 * states). Until it is in the generated schema, the snapshot holds what this browser knows: the
 * stage job it started, and no `next` step. When the read lands, query it here (TanStack Query,
 * key ["pipeline"]) and merge the local job over it; `useNextAction`, the Run menu and the
 * Overview stepper then pick it up unchanged.
 */
export function usePipeline(): PipelineSnapshot {
  const job = useRunStore((state) => state.job);
  return { steps: job === null ? [] : [jobStep(job)], next: null };
}
