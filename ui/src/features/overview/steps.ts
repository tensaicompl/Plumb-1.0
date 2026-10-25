import type { components } from "../../api/schema";
import { PILOT_GATES } from "../../app/nav";
import type { PipelineSnapshot, PipelineStep, StageName, StepState } from "../../app/pipeline";
import type { MessageKey } from "../../i18n";

type GateStatus = components["schemas"]["GateStatus"];

/** The nine designed pipeline steps of the Overview stepper, in order. */
export const OVERVIEW_STEP_IDS = ["import", "requirements", "glossary", "model", "flows", "calcRules", "scenarios", "questions", "ready"] as const;

export type OverviewStepId = (typeof OVERVIEW_STEP_IDS)[number];

/** One designed step: its combined state and the link to what it produced. */
export interface OverviewStep {
  readonly id: OverviewStepId;
  readonly label: MessageKey;
  readonly state: StepState;
  /** 0–1 while running. */
  readonly progress?: number;
  /** Items waiting for review. */
  readonly reviewCount?: number;
  /** The plain failure, when failed. */
  readonly error?: string | null;
  /** The failed recipe and its job (what Retry re-runs). */
  readonly stage?: StageName;
  readonly jobId?: string | null;
  readonly href: string;
}

/** The recipes behind each recipe-backed step and the destination showing what it produced. */
const RECIPE_STEPS: Readonly<Partial<Record<OverviewStepId, { readonly stages: readonly StageName[]; readonly href: string }>>> = {
  requirements: { stages: ["extract"], href: "/requirements" },
  glossary: { stages: ["consolidate", "lint"], href: "/model/glossary" },
  model: { stages: ["directory", "analyse"], href: "/model/entities" },
  flows: { stages: ["flows"], href: "/model/processes" },
  calcRules: { stages: ["calc", "rules"], href: "/model/calculations" },
  scenarios: { stages: ["scenarios", "tags", "run"], href: "/scenarios" },
};

const LABEL: Readonly<Record<OverviewStepId, MessageKey>> = {
  import: "overview.step.import",
  requirements: "overview.step.requirements",
  glossary: "overview.step.glossary",
  model: "overview.step.model",
  flows: "overview.step.flows",
  calcRules: "overview.step.calcRules",
  scenarios: "overview.step.scenarios",
  questions: "overview.step.questions",
  ready: "overview.step.ready",
};

/** The Inbox, where a step's proposals wait for review. */
const INBOX = "/inbox";
const QUESTIONS = "/inbox?filter=questions";

type Combined = Pick<OverviewStep, "state" | "progress" | "reviewCount" | "error" | "stage" | "jobId">;

/**
 * One state for the recipes of a step: running first, then failed, then needs review (counts
 * summed), then outdated, then done. A recipe that never ran does not hold back a step whose other
 * recipes are done (the directory recipe is optional).
 */
export function combineSteps(steps: readonly PipelineStep[]): Combined {
  const running = steps.filter((s) => s.state === "running");
  if (running.length > 0) {
    return { state: "running", progress: Math.min(...running.map((s) => s.progress ?? 0)) };
  }
  const failed = steps.find((s) => s.state === "failed");
  if (failed !== undefined) {
    return { state: "failed", error: failed.error ?? null, jobId: failed.jobId ?? null, stage: failed.stage };
  }
  const review = steps.filter((s) => s.state === "needs_review");
  if (review.length > 0) {
    return { state: "needs_review", reviewCount: review.reduce((sum, s) => sum + (s.reviewCount ?? 0), 0) };
  }
  if (steps.some((s) => s.state === "stale")) {
    return { state: "stale" };
  }
  return { state: steps.some((s) => s.state === "done") ? "done" : "not_started" };
}

/** Whether every pilot gate is current and passes. */
function allGatesPass(gates: readonly GateStatus[] | undefined): boolean {
  const byGate = new Map((gates ?? []).map((g) => [g.gate, g]));
  return PILOT_GATES.every((g) => {
    const status = byGate.get(g);
    return status !== undefined && status.state === "current" && status.passed;
  });
}

/** The nine designed steps from the pipeline snapshot and the gates. */
export function overviewSteps(pipeline: PipelineSnapshot, gates: readonly GateStatus[] | undefined): OverviewStep[] {
  const anyRecipeRan = pipeline.steps.some((s) => s.state !== "not_started");
  return OVERVIEW_STEP_IDS.map((id): OverviewStep => {
    const label = LABEL[id];
    if (id === "import") {
      return { id, label, state: (pipeline.sources ?? 0) > 0 ? "done" : "not_started", href: "/sources" };
    }
    if (id === "questions") {
      const open = pipeline.openQuestions ?? 0;
      if (open > 0) {
        return { id, label, state: "needs_review", reviewCount: open, href: QUESTIONS };
      }
      return { id, label, state: anyRecipeRan ? "done" : "not_started", href: QUESTIONS };
    }
    if (id === "ready") {
      return { id, label, state: anyRecipeRan && allGatesPass(gates) ? "done" : "not_started", href: "/quality" };
    }
    const recipe = RECIPE_STEPS[id];
    const stages = recipe?.stages ?? [];
    const combined = combineSteps(pipeline.steps.filter((s) => stages.includes(s.stage)));
    return { id, label, ...combined, href: combined.state === "needs_review" ? INBOX : (recipe?.href ?? INBOX) };
  });
}
