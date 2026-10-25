import { useIsFetching, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, ChevronDown, MessageSquare, Play, RefreshCw, Search } from "lucide-react";

import { useConnectionStore } from "../../api/useEvents";
import { modelVersionQueryKey, useModelVersion } from "../../api/useVersionedMutation";
import { cn, IconButton, Kbd, Menu, modKey, Skeleton, type MenuItem } from "../../components/ui";
import { t, tf, type MessageKey } from "../../i18n";
import { useNextAction, type NextAction } from "../nextAction";
import { isJobActive, RUNNABLE_STAGES, stageLabel, usePipeline, type StepState } from "../pipeline";
import { useIsCompact, useShellStore } from "../preferences";
import { useRunStage, useRunStore } from "../run";
import { buildHref, useGo } from "./navigation";

// ------------------------------------------------------------------ project and version

/** "v42" from a revision ID "rev:42:…" (the revision itself when it has another form). */
export function versionLabel(revision: string): string {
  const match = /^rev:(\d+):/.exec(revision);
  return match === null ? revision : tf("shell.version.number", { n: match[1] ?? "" });
}

/**
 * The project's display name. The contract has no project read yet (no name, identifier or
 * database path in any response), so this is the catalogue's "Untitled project" until one lands.
 */
export function useProjectName(): string {
  return t("shell.project.untitled");
}

export type VersionState = "upToDate" | "updating" | "changed";

/** The version indicator state: changed while the live connection is lost, updating while a step runs or the version reloads. */
export function useVersionState(): { label: string | null; state: VersionState } {
  const version = useModelVersion();
  const connection = useConnectionStore((state) => state.status);
  const job = useRunStore((state) => state.job);
  const reloading = useIsFetching({ queryKey: modelVersionQueryKey }) > 0 && version.data !== undefined;
  const state: VersionState = connection === "lost" ? "changed" : isJobActive(job) || reloading ? "updating" : "upToDate";
  return { label: version.data === undefined ? null : versionLabel(version.data.revision), state };
}

const VERSION_TEXT: Readonly<Record<VersionState, MessageKey>> = {
  upToDate: "shell.version.upToDate",
  updating: "shell.version.updating",
  changed: "shell.version.changed",
};

const VERSION_DOT: Readonly<Record<VersionState, string>> = {
  upToDate: "bg-pass",
  updating: "bg-running",
  changed: "bg-attention",
};

function VersionIndicator({ compact }: { compact: boolean }) {
  const queryClient = useQueryClient();
  const { label, state } = useVersionState();
  if (label === null) {
    return <Skeleton width={96} height={26} radius="pill" />;
  }
  const text = t(VERSION_TEXT[state]);
  // The status text returns in compact mode when it matters (updating, changed).
  const showText = !compact || state !== "upToDate";
  const name = tf("shell.version.label", { version: label, state: text });
  const body = (
    <>
      <span aria-hidden="true" className={cn("size-1.5 rounded-pill", VERSION_DOT[state])} />
      <span aria-hidden="true" className="font-mono text-fg-3">
        {label}
      </span>
      {showText && <span aria-hidden="true">{text}</span>}
    </>
  );
  const pill = "flex h-[26px] shrink-0 items-center gap-1.5 whitespace-nowrap rounded-pill border border-line bg-card px-2.5 text-caption font-medium text-fg-2";
  if (state === "changed") {
    return (
      <button type="button" aria-label={name} title={name} data-version-state={state} onClick={() => void queryClient.invalidateQueries()} className={cn(pill, "cursor-pointer hover:bg-surface")}>
        {body}
      </button>
    );
  }
  return (
    <span role="status" aria-label={name} title={name} data-version-state={state} className={pill}>
      {body}
    </span>
  );
}

// ------------------------------------------------------------------ next action

/** Runs a next action: navigate, run a stage, or open the export sheet. */
export function useRunNextAction(): (action: NextAction) => void {
  const go = useGo();
  const { run } = useRunStage();
  const openOverlay = useShellStore((state) => state.openOverlay);
  return (action) => {
    const target = action.target;
    if (target.type === "route") {
      go(buildHref(target.to, target.params, target.search));
    } else if (target.type === "run") {
      run(target.stage);
    } else if (target.type === "export") {
      openOverlay("export");
    }
  };
}

function NextActionButton() {
  const action = useNextAction();
  const runAction = useRunNextAction();
  if (action === null) {
    return <Skeleton width={160} height={34} radius="pill" />;
  }
  return (
    <button
      type="button"
      onClick={() => runAction(action)}
      title={action.title}
      className="inline-flex h-[34px] shrink-0 cursor-pointer items-center gap-2 whitespace-nowrap rounded-pill bg-accent px-4 text-footnote font-semibold leading-none text-on-accent transition-colors duration-[120ms] ease-standard hover:bg-accent-hover active:bg-accent-pressed"
    >
      {action.short}
      <ArrowRight size={16} strokeWidth={1.6} aria-hidden="true" />
    </button>
  );
}

// ------------------------------------------------------------------ run step

const STEP_META: Partial<Readonly<Record<StepState, MessageKey>>> = {
  done: "shell.run.meta.done",
  needs_review: "shell.run.meta.review",
  running: "shell.run.meta.running",
  failed: "shell.run.meta.failed",
};

function RunStepButton({ compact }: { compact: boolean }) {
  const job = useRunStore((state) => state.job);
  const pipeline = usePipeline();
  const { run, starting } = useRunStage();
  const menuOpen = useShellStore((state) => state.runMenuOpen);
  const setMenuOpen = useShellStore((state) => state.setRunMenuOpen);
  const activeJob = job !== null && isJobActive(job) ? job : null;
  const running = activeJob !== null;
  const pct = activeJob === null ? 0 : Math.round(activeJob.progress * 100);
  const busy = running || starting;
  const items: MenuItem[] = RUNNABLE_STAGES.map((stage) => {
    const state = pipeline.steps.find((s) => s.stage === stage)?.state;
    const meta = state === undefined ? undefined : STEP_META[state];
    return {
      id: stage,
      label: tf("shell.run.stepLabel", { step: stageLabel(stage) }),
      meta: meta === undefined ? undefined : t(meta),
      disabled: busy,
      onSelect: () => run(stage),
    };
  });
  const primaryLabel = activeJob === null ? t("shell.run.next") : tf("shell.run.runningLabel", { step: stageLabel(activeJob.stage), pct });
  const onPrimary = () => {
    if (busy) {
      return;
    }
    if (pipeline.next !== null) {
      run(pipeline.next);
    } else {
      // Without pipeline data the next step is unknown: let the user choose it.
      setMenuOpen(true);
    }
  };
  return (
    <div className="relative flex shrink-0">
      <button
        type="button"
        onClick={onPrimary}
        aria-label={compact || running ? primaryLabel : undefined}
        title={primaryLabel}
        className="relative flex h-[34px] cursor-pointer items-center gap-2 overflow-hidden whitespace-nowrap rounded-l-pill border border-r-0 border-line bg-card pl-3.5 pr-3 text-footnote font-semibold leading-none text-fg transition-colors duration-[120ms] hover:bg-surface"
      >
        {running && (
          <span
            data-run-progress
            aria-hidden="true"
            style={{ width: String(pct) + "%" }}
            className="absolute bottom-0 left-0 h-0.5 bg-accent transition-[width] duration-200 ease-standard"
          />
        )}
        {running ? (
          <RefreshCw size={14} strokeWidth={1.6} aria-hidden="true" className="motion-safe:animate-spin" />
        ) : (
          <Play size={14} strokeWidth={1.6} aria-hidden="true" />
        )}
        {running ? <span aria-hidden="true">{tf("shell.run.percent", { pct })}</span> : !compact && t("shell.run.next")}
      </button>
      <Menu
        title={t("shell.run.menuTitle")}
        open={menuOpen}
        onOpenChange={setMenuOpen}
        items={items}
        trigger={
          <button
            type="button"
            aria-label={t("shell.run.menu")}
            title={t("shell.run.menu")}
            className="flex h-[34px] w-[30px] cursor-pointer items-center justify-center rounded-r-pill border border-line bg-card text-fg-2 transition-colors duration-[120ms] hover:bg-surface"
          >
            <ChevronDown size={16} strokeWidth={1.6} aria-hidden="true" />
          </button>
        }
      />
    </div>
  );
}

// ------------------------------------------------------------------ toolbar

/** The 56 px toolbar: project · version · spacer · search · Next action · Run step · Assistant. */
export function Toolbar() {
  const compact = useIsCompact();
  const openOverlay = useShellStore((state) => state.openOverlay);
  const assistantOpen = useShellStore((state) => state.assistantOpen);
  const setAssistantOpen = useShellStore((state) => state.setAssistantOpen);
  const projectName = useProjectName();
  const searchLabel = t("shell.search.trigger");
  return (
    <header className="flex h-14 shrink-0 items-center gap-3 border-b border-line bg-toolbar px-5 backdrop-blur-md">
      <button
        type="button"
        onClick={() => openOverlay("settings")}
        title={t("shell.project.details")}
        className="flex min-w-0 shrink cursor-pointer items-center gap-2 overflow-hidden whitespace-nowrap rounded-control px-2 py-1.5 font-display text-subhead font-semibold leading-none tracking-[-0.01em] text-fg hover:bg-surface"
      >
        <span className="truncate">{projectName}</span>
        <ChevronDown size={16} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />
      </button>
      <VersionIndicator compact={compact} />
      <div className="min-w-0 flex-[1_1_0]" />
      <button
        type="button"
        onClick={() => openOverlay("palette")}
        aria-label={compact ? searchLabel : undefined}
        title={compact ? searchLabel : undefined}
        className={cn(
          "flex h-[34px] cursor-pointer items-center gap-2 overflow-hidden whitespace-nowrap rounded-control border border-line bg-surface text-left text-footnote text-fg-3-surface transition-colors duration-[120ms] hover:border-line-strong",
          compact ? "w-[34px] shrink-0 justify-center" : "min-w-[120px] max-w-[260px] flex-[1_1_260px] px-3",
        )}
      >
        <Search size={16} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />
        {!compact && (
          <>
            <span className="min-w-0 flex-1 truncate">{searchLabel}</span>
            <Kbd className="text-fg-3-surface">{modKey() + "K"}</Kbd>
          </>
        )}
      </button>
      <NextActionButton />
      <RunStepButton compact={compact} />
      <IconButton
        label={t("shell.assistant.toggle")}
        icon={<MessageSquare size={16} strokeWidth={1.6} aria-hidden="true" />}
        pressed={assistantOpen}
        onClick={() => setAssistantOpen(!assistantOpen)}
      />
    </header>
  );
}
