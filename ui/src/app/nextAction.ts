import type { components } from "../api/schema";
import { gateName, type GateName } from "../components/ui";
import { t, tc, tf } from "../i18n";
import { PILOT_GATES } from "./nav";
import { isJobActive, stageLabel, usePipeline, type PipelineSnapshot, type StageName } from "./pipeline";
import { useRunStore, type RunJob } from "./run";
import { useWorkCounts } from "./work";

type GateStatus = components["schemas"]["GateStatus"];

export type NextActionKind = "running" | "retry" | "proposals" | "questions" | "staleGate" | "step" | "blockedGate" | "ready";

/** Where the next action leads. */
export type NextActionTarget =
  | { readonly type: "route"; readonly to: string; readonly params?: Readonly<Record<string, string>>; readonly search?: Readonly<Record<string, string>> }
  | { readonly type: "run"; readonly stage: StageName }
  | { readonly type: "export" }
  | { readonly type: "none" };

/** The one obvious next action, with the copy for the toolbar (`short`) and the Overview card. */
export interface NextAction {
  readonly kind: NextActionKind;
  /** The Overview card title. */
  readonly title: string;
  /** The toolbar button label. */
  readonly short: string;
  readonly description: string;
  /** The Overview card button label. */
  readonly cta: string;
  readonly target: NextActionTarget;
  /** 0–100 while a step runs. */
  readonly progress?: number;
}

export interface NextActionInput {
  readonly job: RunJob | null;
  readonly pipeline: PipelineSnapshot | undefined;
  /** Pending proposals (undefined while loading). */
  readonly proposals: number | undefined;
  /** Open questions (undefined while loading). */
  readonly openQuestions: number | undefined;
  readonly gates: readonly GateStatus[] | undefined;
}

/**
 * The README priority order: a running step → a failed step ("Extract failed — Retry") → pending
 * proposals → open questions → a stale or never-evaluated gate → the next pipeline step → a
 * blocked gate → "Your specification is ready — Export". Null until the counts and gates load.
 */
export function computeNextAction({ job, pipeline, proposals, openQuestions, gates }: NextActionInput): NextAction | null {
  if (job !== null && isJobActive(job)) {
    const pct = Math.round(job.progress * 100);
    const step = stageLabel(job.stage);
    return {
      kind: "running",
      title: tf("next.running.title", { step, pct }),
      short: tf("next.running.short", { pct }),
      description: t("next.running.description"),
      cta: t("next.running.cta"),
      target: { type: "none" },
      progress: pct,
    };
  }
  const failed = job?.status === "failed" ? { stage: job.stage, error: job.error } : pipeline?.steps.find((s) => s.state === "failed");
  if (failed !== undefined) {
    const step = stageLabel(failed.stage);
    return {
      kind: "retry",
      title: tf("next.retry.title", { step }),
      short: tf("next.retry.short", { step }),
      description: failed.error ?? "",
      cta: t("next.retry.cta"),
      target: { type: "run", stage: failed.stage },
    };
  }
  if (proposals === undefined || openQuestions === undefined || gates === undefined) {
    return null;
  }
  if (proposals > 0) {
    return {
      kind: "proposals",
      title: tc("next.proposals.title", proposals),
      short: tc("next.proposals.title", proposals),
      description: t("next.proposals.description"),
      cta: t("next.proposals.cta"),
      target: { type: "route", to: "/inbox" },
    };
  }
  if (openQuestions > 0) {
    return {
      kind: "questions",
      title: tc("next.questions.title", openQuestions),
      short: tc("next.questions.short", openQuestions),
      description: t("next.questions.description"),
      cta: t("next.questions.cta"),
      target: { type: "route", to: "/inbox", search: { filter: "questions" } },
    };
  }
  const byGate = new Map(gates.map((status) => [status.gate, status]));
  // An outdated gate first, then one that was never evaluated (absent from the response).
  const outdated =
    PILOT_GATES.find((g) => byGate.get(g)?.state === "stale") ??
    PILOT_GATES.find((g) => {
      const status = byGate.get(g);
      return status === undefined || status.state !== "current";
    });
  if (outdated !== undefined) {
    const name = gateName(outdated);
    const stale = byGate.get(outdated)?.state === "stale";
    return {
      kind: "staleGate",
      title: tf(stale ? "next.staleGate.title" : "next.uncheckedGate.title", { gate: name }),
      short: tf(stale ? "next.staleGate.short" : "next.uncheckedGate.short", { gate: name }),
      description: t("next.staleGate.description"),
      cta: t("next.staleGate.cta"),
      target: gateTarget(outdated),
    };
  }
  if (pipeline?.next !== undefined && pipeline.next !== null) {
    const step = stageLabel(pipeline.next);
    return {
      kind: "step",
      title: tf("next.step.title", { step }),
      short: tf("next.step.short", { step }),
      description: t("next.step.description"),
      cta: t("next.step.cta"),
      target: { type: "run", stage: pipeline.next },
    };
  }
  const blocked = PILOT_GATES.find((g) => byGate.get(g)?.passed === false);
  if (blocked !== undefined) {
    const name = gateName(blocked);
    return {
      kind: "blockedGate",
      title: tf("next.blockedGate.title", { gate: name }),
      short: tf("next.blockedGate.short", { gate: name }),
      description: t("next.blockedGate.description"),
      cta: t("next.blockedGate.cta"),
      target: gateTarget(blocked),
    };
  }
  return {
    kind: "ready",
    title: t("next.ready.title"),
    short: t("next.ready.short"),
    description: t("next.ready.description"),
    cta: t("next.ready.cta"),
    target: { type: "export" },
  };
}

function gateTarget(gate: GateName): NextActionTarget {
  return { type: "route", to: "/quality/$gate", params: { gate } };
}

/** The current next action from the shared work queries, the tracked job and the pipeline. */
export function useNextAction(): NextAction | null {
  const job = useRunStore((state) => state.job);
  const pipeline = usePipeline();
  const work = useWorkCounts();
  return computeNextAction({ job, pipeline, proposals: work.proposals, openQuestions: work.openQuestions, gates: work.gates });
}
