import { useQuery } from "@tanstack/react-query";
import { ArrowRight, Check, Info, RefreshCw, TriangleAlert } from "lucide-react";
import type { MouseEvent, ReactNode } from "react";

import { apiClient, readData } from "../../api/client";
import type { components } from "../../api/schema";
import { useModelVersion } from "../../api/useVersionedMutation";
import { useMe } from "../../app/auth";
import { PILOT_GATES } from "../../app/nav";
import { useNextAction, type NextAction } from "../../app/nextAction";
import { usePipeline, type StepState } from "../../app/pipeline";
import { useShellStore } from "../../app/preferences";
import { buildHref, useGo } from "../../app/shell/navigation";
import { useProject, useRunNextAction } from "../../app/shell/Toolbar";
import { gatesQuery, useWorkCounts } from "../../app/work";
import {
  Avatar,
  Button,
  Card,
  cn,
  EnsoMark,
  Eyebrow,
  GateLight,
  isPlainClick,
  ReadinessRing,
  Skeleton,
  SkeletonGroup,
  type GateLightState,
} from "../../components/ui";
import { t, tc, tf, useLocaleStore, type MessageKey } from "../../i18n";
import { ImportDropZone } from "../sources/ImportDropZone";
import { ALL_PAGE_SIZE, readAllPages, sourcesQuery } from "../sources/queries";
import { overviewSteps, type OverviewStep } from "./steps";
import { actorName, decisionTime, parseTimestamp, relativeTime } from "./time";

type GateStatus = components["schemas"]["GateStatus"];
type Readiness = components["schemas"]["Readiness"];
type Decision = components["schemas"]["Decision"];

// ------------------------------------------------------------------ reads

const readinessQuery = {
  queryKey: ["overview", "readiness"] as const,
  queryFn: (): Promise<Readiness> => readData(apiClient.GET("/api/readiness")),
};

const requirementsQuery = {
  queryKey: ["overview", "requirements"] as const,
  queryFn: () => readAllPages((cursor) => apiClient.GET("/api/requirements", { params: { query: { limit: ALL_PAGE_SIZE, ...(cursor === undefined ? {} : { cursor }) } } })),
};

const entitiesQuery = {
  queryKey: ["overview", "entities"] as const,
  queryFn: () => readAllPages((cursor) => apiClient.GET("/api/model/entities", { params: { query: { limit: ALL_PAGE_SIZE, ...(cursor === undefined ? {} : { cursor }) } } })),
};

const crudQuery = {
  queryKey: ["overview", "crud"] as const,
  queryFn: () => readData(apiClient.GET("/api/model/crud")),
};

const scenariosQuery = {
  queryKey: ["overview", "scenarios"] as const,
  queryFn: () => readAllPages((cursor) => apiClient.GET("/api/scenarios", { params: { query: { limit: ALL_PAGE_SIZE, ...(cursor === undefined ? {} : { cursor }) } } })),
};

const decisionsQuery = {
  queryKey: ["overview", "decisions"] as const,
  queryFn: () => readAllPages((cursor) => apiClient.GET("/api/decisions", { params: { query: { limit: ALL_PAGE_SIZE, ...(cursor === undefined ? {} : { cursor }) } } })),
};

/** How many recent decisions the Overview lists. */
const RECENT_DECISIONS = 5;

// ------------------------------------------------------------------ shared bits

/** An in-app link that navigates through the router (and still opens in a new tab). */
function AppLink({ href, className, children, ...rest }: { href: string; className?: string; children: ReactNode } & Record<`data-${string}`, string>) {
  const go = useGo();
  return (
    <a
      href={href}
      className={className}
      onClick={(event: MouseEvent<HTMLAnchorElement>) => {
        if (isPlainClick(event)) {
          event.preventDefault();
          go(href);
        }
      }}
      {...rest}
    >
      {children}
    </a>
  );
}

function SectionHeading({ id, children }: { id: string; children: ReactNode }) {
  return (
    <h2 id={id} className="m-0 font-display text-lead font-semibold tracking-[-0.01em] text-fg">
      {children}
    </h2>
  );
}

function RetryRead({ message, onRetry }: { message: MessageKey; onRetry: () => void }) {
  return (
    <div role="alert" className="flex items-center gap-3 text-footnote text-blocked-fg">
      <span>{t(message)}</span>
      <Button variant="outline" size={30} onClick={onRetry}>
        {t("overview.retry")}
      </Button>
    </div>
  );
}

// ------------------------------------------------------------------ header

const SUB_SCORES = ["clarity", "completeness", "confirmation", "consistency", "verifiability"] as const;

const SUB_LABEL: Readonly<Record<(typeof SUB_SCORES)[number], MessageKey>> = {
  clarity: "ui.readiness.clarity",
  completeness: "ui.readiness.completeness",
  confirmation: "ui.readiness.confirmation",
  consistency: "ui.readiness.consistency",
  verifiability: "ui.readiness.verifiability",
};

function ReadinessCard() {
  const readiness = useQuery(readinessQuery);
  return (
    <Card as="section" aria-label={t("overview.readiness")} padding="none" className="flex shrink-0 items-center gap-5 px-5 py-4">
      {readiness.isError ? (
        <RetryRead message="overview.readiness.failed" onRetry={() => void readiness.refetch()} />
      ) : readiness.data === undefined ? (
        <SkeletonGroup className="flex items-center gap-5">
          <Skeleton width={96} height={96} radius="full" />
          <span className="flex w-[180px] flex-col gap-2">
            {SUB_SCORES.map((key) => (
              <Skeleton key={key} height={10} />
            ))}
          </span>
        </SkeletonGroup>
      ) : (
        <>
          <span className="relative inline-grid">
            <ReadinessRing value={readiness.data.overall ?? null} size={96} stroke={8} showLabel subScores={readiness.data.sub} />
            {(readiness.data.overall ?? null) === null && (
              <span aria-hidden="true" className="absolute inset-0 flex flex-col items-center justify-center leading-none">
                <span className="font-display text-headline font-bold text-fg-3">{t("overview.readiness.none")}</span>
                <span className="mt-1 text-[10px] text-fg-3">{t("ui.readiness.ready")}</span>
              </span>
            )}
          </span>
          <ul aria-hidden="true" className="m-0 flex w-[180px] list-none flex-col gap-1.5 p-0">
            {SUB_SCORES.map((key) => {
              const value = readiness.data.sub[key];
              return (
                <li key={key} className="flex items-center gap-2 text-caption text-fg-2">
                  <span className="w-[84px]">{t(SUB_LABEL[key])}</span>
                  <span className="h-1 flex-1 overflow-hidden rounded-[2px] bg-surface">
                    <span className="block h-full bg-primary" style={{ width: String(Math.max(0, Math.min(100, value ?? 0))) + "%" }} />
                  </span>
                  <span className="w-[26px] text-right font-mono text-eyebrow font-medium text-fg-3">{value === null ? t("overview.readiness.none") : String(Math.round(value))}</span>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </Card>
  );
}

function HeaderMeta() {
  const version = useModelVersion();
  const requirements = useQuery(requirementsQuery);
  const sources = useQuery(sourcesQuery);
  const locale = useLocaleStore((state) => state.locale);
  if (version.data === undefined || requirements.data === undefined || sources.data === undefined) {
    return <Skeleton width={360} height={13} className="mt-2.5" />;
  }
  const updated = tf("overview.updated", { when: relativeTime(version.data.created_at, new Date(), locale), who: actorName(version.data.created_by) });
  const names = sources.data.map((s) => s.name).join(", ");
  const count = tc("overview.requirementsFrom", requirements.data.length, { names });
  return <p className="m-0 mt-2.5 text-footnote text-fg-3">{[updated, count].join(SEPARATOR)}</p>;
}

/** The separator between facts on one line (punctuation, not copy). */
const SEPARATOR = " · ";

// ------------------------------------------------------------------ next action

function NextActionCard() {
  const action = useNextAction();
  const runAction = useRunNextAction();
  const setRunMenuOpen = useShellStore((state) => state.setRunMenuOpen);
  const onAction = (next: NextAction) => (next.kind === "running" ? setRunMenuOpen(true) : runAction(next));
  return (
    <section aria-label={t("overview.next")} className="flex items-center gap-6 rounded-sheet bg-hero px-8 py-7 text-on-hero">
      <div className="min-w-0 flex-1">
        <Eyebrow tone="onDark" className="mb-2.5">
          {t("overview.next")}
        </Eyebrow>
        {action === null ? (
          <SkeletonGroup className="flex flex-col gap-2">
            <Skeleton width="60%" height={24} className="bg-hero-raised" />
            <Skeleton width="40%" height={14} className="bg-hero-raised" />
          </SkeletonGroup>
        ) : (
          <>
            <p className="m-0 font-display text-headline font-semibold leading-tight tracking-[-0.01em] [text-wrap:pretty]">{action.title}</p>
            {action.description !== "" && <p className="m-0 mt-1.5 text-body text-hero-sub">{action.description}</p>}
            {action.progress !== undefined && (
              <div
                role="progressbar"
                aria-label={action.title}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={action.progress}
                className="mt-4 h-1 overflow-hidden rounded-[2px] bg-hero-raised"
              >
                <div className="h-full bg-accent-on-dark transition-[width] duration-200 ease-standard" style={{ width: String(action.progress) + "%" }} />
              </div>
            )}
          </>
        )}
      </div>
      {action !== null && action.kind !== "import" && (
        <Button variant="inverse" size={44} trailingIcon={<ArrowRight size={16} strokeWidth={1.6} aria-hidden="true" />} onClick={() => onAction(action)}>
          {action.cta}
        </Button>
      )}
    </section>
  );
}

// ------------------------------------------------------------------ pipeline

const STEP_META: Readonly<Record<StepState, MessageKey>> = {
  done: "overview.stepState.done",
  needs_review: "overview.stepState.review",
  running: "overview.stepState.running",
  failed: "overview.stepState.failed",
  stale: "overview.stepState.stale",
  not_started: "overview.stepState.notStarted",
};

const STEP_META_TONE: Readonly<Record<StepState, string>> = {
  done: "text-fg-3",
  needs_review: "text-attention-fg",
  running: "text-running-fg",
  failed: "text-blocked-fg",
  stale: "text-neutral-fg",
  not_started: "text-fg-4",
};

function StepMark({ step, index }: { step: OverviewStep; index: number }) {
  const base = "flex size-[30px] items-center justify-center rounded-pill font-mono text-eyebrow font-semibold";
  switch (step.state) {
    case "done":
      return (
        <span aria-hidden="true" className={cn(base, "bg-primary text-on-primary")}>
          <Check size={14} strokeWidth={2} />
        </span>
      );
    case "needs_review":
      return <span className={cn(base, "border-[1.5px] border-primary bg-card text-fg")}>{String(step.reviewCount ?? 0)}</span>;
    case "running":
      return (
        <span aria-hidden="true" className={cn(base, "bg-running-tint text-running-fg")}>
          <RefreshCw size={14} strokeWidth={1.6} className="motion-safe:animate-spin" />
        </span>
      );
    case "failed":
      return (
        <span aria-hidden="true" className={cn(base, "bg-blocked-tint text-blocked-fg")}>
          <TriangleAlert size={14} strokeWidth={1.6} />
        </span>
      );
    case "stale":
      return <span aria-hidden="true" className={cn(base, "border-2 border-stale bg-card")} />;
    case "not_started":
      return (
        <span aria-hidden="true" className={cn(base, "border border-dashed border-line-strong bg-card text-fg-4")}>
          {String(index + 1)}
        </span>
      );
  }
}

function PipelineCard() {
  const pipeline = usePipeline();
  const gates = useQuery(gatesQuery);
  const steps = overviewSteps(pipeline, Array.isArray(gates.data) ? gates.data : undefined);
  return (
    <Card as="section" aria-labelledby="overview-pipeline">
      <div className="mb-5 flex items-baseline justify-between gap-4">
        <SectionHeading id="overview-pipeline">{t("overview.pipeline")}</SectionHeading>
        <span className="text-caption text-fg-3">{t("overview.pipeline.hint")}</span>
      </div>
      {!pipeline.loaded && pipeline.steps.length === 0 ? (
        <SkeletonGroup className="grid grid-cols-9 gap-1">
          {steps.map((step) => (
            <span key={step.id} className="flex flex-col items-center gap-2 py-2">
              <Skeleton width={30} height={30} radius="full" />
              <Skeleton width="70%" height={10} />
            </span>
          ))}
        </SkeletonGroup>
      ) : (
        <ol className="m-0 grid list-none grid-cols-9 gap-1 p-0">
          {steps.map((step, index) => (
            <li key={step.id} className="min-w-0">
              <AppLink
                href={step.href}
                data-step={step.id}
                data-step-state={step.state}
                className="flex h-full flex-col items-center gap-2 rounded-field px-0.5 py-2 text-center no-underline transition-colors duration-[120ms] hover:bg-surface"
              >
                <StepMark step={step} index={index} />
                <span className="text-caption font-medium leading-tight text-fg">{t(step.label)}</span>
                <span className={cn("text-eyebrow", STEP_META_TONE[step.state])}>
                  {step.state === "running" ? tf("overview.stepState.percent", { pct: Math.round((step.progress ?? 0) * 100) }) : t(STEP_META[step.state])}
                </span>
              </AppLink>
            </li>
          ))}
        </ol>
      )}
    </Card>
  );
}

// ------------------------------------------------------------------ gates

function gateLightState(status: GateStatus | undefined): GateLightState {
  if (status === undefined || status.state === "not_evaluated") {
    return "not_evaluated";
  }
  if (status.state === "stale") {
    return "stale";
  }
  return status.passed ? "pass" : "fail";
}

function GatesRow() {
  const gates = useQuery(gatesQuery);
  const byGate = new Map((Array.isArray(gates.data) ? gates.data : []).map((g) => [g.gate, g]));
  return (
    <section aria-labelledby="overview-gates">
      <div className="mb-3 flex items-baseline justify-between">
        <SectionHeading id="overview-gates">{t("overview.gates")}</SectionHeading>
        <AppLink href="/quality" className="text-footnote font-medium text-fg no-underline hover:underline">
          {t("overview.gates.open")}
        </AppLink>
      </div>
      {gates.isError ? (
        <RetryRead message="overview.gates.failed" onRetry={() => void gates.refetch()} />
      ) : (
        <ul className="m-0 grid list-none grid-cols-5 gap-3 p-0">
          {PILOT_GATES.map((gate) => {
            const status = byGate.get(gate);
            const state = gateLightState(status);
            const blockers = status?.summary.blocker_failed ?? 0;
            return (
              <li key={gate} className="min-w-0">
                {gates.data === undefined ? (
                  <SkeletonGroup className="rounded-tile border border-line bg-card px-4 py-3.5">
                    <Skeleton width="70%" />
                  </SkeletonGroup>
                ) : (
                  <AppLink
                    href={buildHref("/quality/$gate", { gate })}
                    className="flex h-full flex-col rounded-tile border border-line bg-card px-4 py-3.5 no-underline shadow-xs transition-[box-shadow,border-color] duration-[120ms] hover:border-line-strong hover:shadow-md"
                  >
                    <GateLight gate={gate} state={state} detail={state === "fail" && blockers > 0 ? tc("overview.gates.blocking", blockers) : undefined} className="gap-2" />
                  </AppLink>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

// ------------------------------------------------------------------ at a glance

function GlanceTile({ label, value, sub, href }: { label: string; value: string | null; sub: string | null; href: string }) {
  return (
    <li className="min-w-0">
      <AppLink
        href={href}
        className="block h-full rounded-tile border border-line bg-card p-4 no-underline shadow-xs transition-[box-shadow,border-color] duration-[120ms] hover:border-line-strong hover:shadow-md"
      >
        <span className="block text-caption font-medium text-fg-3">{label}</span>
        {value === null ? (
          <SkeletonGroup className="mt-2 flex flex-col gap-2">
            <Skeleton width="50%" height={24} />
            <Skeleton width="70%" height={10} />
          </SkeletonGroup>
        ) : (
          <>
            <span className="mt-1.5 block font-display text-[28px] font-bold leading-[1.2] tracking-display text-fg">{value}</span>
            <span className="mt-0.5 block text-caption text-fg-2">{sub}</span>
          </>
        )}
      </AppLink>
    </li>
  );
}

function AtAGlance() {
  const requirements = useQuery(requirementsQuery);
  const entities = useQuery(entitiesQuery);
  const crud = useQuery(crudQuery);
  const scenarios = useQuery(scenariosQuery);
  const work = useWorkCounts();
  const reqs = requirements.data;
  const issues = reqs?.filter((r) => r.smells.length > 0).length ?? 0;
  const ents = entities.data;
  const states = ents?.reduce((sum, e) => sum + e.states.length, 0) ?? 0;
  const operations = crud.data?.operations.length;
  const scens = scenarios.data;
  const passing = scens?.filter((s) => s.run?.result === "pass").length ?? 0;
  const open = work.openQuestions;
  return (
    <section aria-labelledby="overview-glance">
      <SectionHeading id="overview-glance">{t("overview.glance")}</SectionHeading>
      <ul className="m-0 mt-3 grid list-none grid-cols-4 gap-3 p-0">
        <GlanceTile
          label={t("overview.glance.requirements")}
          value={reqs === undefined ? null : String(reqs.length)}
          sub={issues > 0 ? tc("overview.glance.withIssues", issues) : t("overview.glance.noIssues")}
          href="/requirements"
        />
        <GlanceTile
          label={t("overview.glance.model")}
          value={ents === undefined ? null : tc("overview.glance.entities", ents.length)}
          sub={[tc("overview.glance.states", states), ...(operations === undefined ? [] : [tc("overview.glance.operations", operations)])].join(SEPARATOR)}
          href="/model/entities"
        />
        <GlanceTile
          label={t("overview.glance.scenarios")}
          value={scens === undefined ? null : String(scens.length)}
          sub={tf("overview.glance.pass", { count: passing })}
          href="/scenarios"
        />
        <GlanceTile
          label={t("overview.glance.questions")}
          value={open === undefined ? null : String(open)}
          sub={t(open !== undefined && open > 0 ? "overview.glance.waiting" : "overview.glance.noneWaiting")}
          href="/inbox?filter=questions"
        />
      </ul>
    </section>
  );
}

// ------------------------------------------------------------------ recent decisions

function decisionWhat(decision: Decision): MessageKey {
  if ((decision.question_ref ?? null) !== null) {
    return "overview.decision.answered";
  }
  return (decision.proposal_ref ?? null) !== null ? "overview.decision.proposal" : "overview.decision.other";
}

function RecentDecisions() {
  const decisions = useQuery(decisionsQuery);
  const locale = useLocaleStore((state) => state.locale);
  const now = new Date();
  const recent = [...(decisions.data ?? [])].sort((a, b) => parseTimestamp(b.decided_at).getTime() - parseTimestamp(a.decided_at).getTime()).slice(0, RECENT_DECISIONS);
  return (
    <Card as="section" aria-labelledby="overview-decisions" padding="none">
      <div className="flex items-baseline justify-between px-6 pb-2.5 pt-[18px]">
        <SectionHeading id="overview-decisions">{t("overview.decisions")}</SectionHeading>
        <AppLink href="/quality/history" className="text-footnote font-medium text-fg no-underline hover:underline">
          {t("overview.decisions.history")}
        </AppLink>
      </div>
      {decisions.isError ? (
        <div className="border-t border-line px-6 py-3">
          <RetryRead message="overview.decisions.failed" onRetry={() => void decisions.refetch()} />
        </div>
      ) : decisions.data === undefined ? (
        <SkeletonGroup className="flex flex-col">
          {[0, 1, 2].map((row) => (
            <span key={row} className="flex items-center gap-3 border-t border-line px-6 py-3">
              <Skeleton width={28} height={28} radius="full" />
              <Skeleton width="50%" />
            </span>
          ))}
        </SkeletonGroup>
      ) : recent.length === 0 ? (
        <p className="m-0 border-t border-line px-6 py-3 text-footnote text-fg-3">{t("overview.decisions.empty")}</p>
      ) : (
        <ul className="m-0 list-none p-0">
          {recent.map((decision) => {
            const who = actorName(decision.decided_by);
            const rationale = decision.rationale ?? null;
            return (
              <li key={decision.id} className="flex items-center gap-3 border-t border-line px-6 py-3 text-footnote">
                <Avatar name={who} size={28} decorative className="bg-surface text-fg-2" />
                <span className="min-w-0 flex-1 text-fg">
                  <b className="font-semibold">{who}</b> {t(decisionWhat(decision))}
                  {rationale !== null && rationale !== "" && <span className="text-fg-3">{SEPARATOR + tf("overview.decision.rationale", { rationale })}</span>}
                </span>
                <span className="whitespace-nowrap text-caption text-fg-3">{decisionTime(decision.decided_at, now, locale)}</span>
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}

// ------------------------------------------------------------------ screen

function AiOffNotice() {
  const openOverlay = useShellStore((state) => state.openOverlay);
  return (
    <div role="note" className="flex items-center gap-2.5 rounded-tile bg-surface px-4 py-3 text-footnote text-fg-2">
      <Info size={16} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />
      <span className="flex-1">{t("overview.aiOff")}</span>
      <Button variant="ghost" size={30} className="text-fg" onClick={() => openOverlay("settings")}>
        {t("overview.aiOff.settings")}
      </Button>
    </div>
  );
}

function FirstRun() {
  return (
    <div className="mx-auto flex max-w-[720px] flex-col items-center gap-4 px-8 py-24 text-center">
      <EnsoMark size={160} className="opacity-90" />
      <Eyebrow>{t("overview.firstRun.eyebrow")}</Eyebrow>
      <h1 className="m-0 font-display text-h1 font-bold leading-[1.15] tracking-display text-fg [text-wrap:pretty]">{t("overview.firstRun.title")}</h1>
      <p className="m-0 max-w-[480px] text-lead text-fg-2">{t("overview.firstRun.body")}</p>
      <ImportDropZone variant="hero" className="mt-4" />
    </div>
  );
}

/** /overview: "Where am I, and what's next?" (handoff "1. Overview", UI-SPEC §5.1). */
export function OverviewScreen() {
  const pipeline = usePipeline();
  const me = useMe();
  const project = useProject();
  const firstRun = pipeline.loaded === true && pipeline.sources === 0;
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      {firstRun ? (
        <FirstRun />
      ) : (
        <div className="mx-auto flex max-w-[960px] flex-col gap-6 px-8 pb-16 pt-10">
          {me.data?.inference_available === false && <AiOffNotice />}
          <div className="flex items-center gap-8">
            <div className="min-w-0 flex-1">
              <Eyebrow className="mb-2.5">{t("overview.eyebrow")}</Eyebrow>
              {me.data === undefined ? (
                <Skeleton width="60%" height={44} />
              ) : (
                <h1 className="m-0 font-display text-h1 font-bold leading-[1.1] tracking-display text-fg">{project.name}</h1>
              )}
              <HeaderMeta />
            </div>
            <ReadinessCard />
          </div>
          <NextActionCard />
          <PipelineCard />
          <GatesRow />
          <AtAGlance />
          <RecentDecisions />
        </div>
      )}
    </div>
  );
}
