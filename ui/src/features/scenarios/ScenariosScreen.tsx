import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { Check, CircleHelp, Clock, Minus, Play, X } from "lucide-react";
import { useEffect, useId, useState, type MouseEvent, type ReactNode } from "react";
import { create } from "zustand";

import { apiClient, ReadFailedError, readData, readReviewed, revisionEtag } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation } from "../../api/useVersionedMutation";
import { buildHref, useGo } from "../../app/shell/navigation";
import {
  Button,
  cn,
  EmptyState,
  Eyebrow,
  FilterChip,
  isPlainClick,
  ListRow,
  ListRowError,
  SegmentedControl,
  Sheet,
  SkeletonList,
  StatusPill,
  toast,
  WithInspector,
  type PillStatus,
} from "../../components/ui";
import { t, tc, tf, type MessageKey } from "../../i18n";
import { useLabels, type Labels } from "../model/labels";
import { rulesQuery } from "../model/RulesView";
import { ReadError, useCanEdit, WriteError } from "../model/shared";
import {
  describeFact,
  describeWhen,
  elementHref,
  parseDiff,
  parseMissing,
  readTrace,
  referencedIds,
  scenarioSource,
  scenarioTitle,
  tableColumns,
  tableOf,
  type TraceStep,
} from "./facts";

type Scenario = components["schemas"]["Scenario"];
type ScenarioRun = components["schemas"]["ScenarioRun"];
type WriteResult = components["schemas"]["WriteResult"];

export const scenariosQuery = {
  queryKey: ["scenarios"] as const,
  queryFn: () => readReviewed(apiClient.GET("/api/scenarios")),
};

type Verdict = "pass" | "fail" | "undecidable" | "notRun" | "stale" | "running";

/**
 * The last verdict this browser session showed per scenario. The server reports a run that the
 * model has outdated as not run; a scenario that had a verdict a moment ago is shown as "Out of
 * date" (spec §5.5 stale run) instead of "Not run".
 */
const useRunMemory = create<{ seen: Readonly<Record<string, ScenarioRun["result"]>>; remember(items: readonly Scenario[]): void }>()((set, get) => ({
  seen: {},
  remember: (items) => {
    const next = { ...get().seen };
    let changed = false;
    for (const item of items) {
      if (item.run !== null && next[item.id] !== item.run.result) {
        next[item.id] = item.run.result;
        changed = true;
      }
    }
    if (changed) {
      set({ seen: next });
    }
  },
}));

/** Only Accepted + Confirmed scenarios run. */
function runnable(scenario: Scenario): boolean {
  return scenario.status === "Accepted" && scenario.confirmation_status === "Confirmed";
}

function confirmable(scenario: Scenario): boolean {
  return scenario.status === "Proposed" && scenario.confirmation_status === "Derived";
}

/** Correction applies to Proposed + Derived and Accepted + Confirmed scenarios. */
function correctable(scenario: Scenario): boolean {
  return confirmable(scenario) || runnable(scenario);
}

const VERDICT_PILL: Readonly<Record<Verdict, { status: PillStatus; label?: MessageKey }>> = {
  pass: { status: "pass" },
  fail: { status: "fail" },
  undecidable: { status: "undecidable" },
  notRun: { status: "notRun" },
  stale: { status: "stale", label: "scenarios.verdict.stale" },
  running: { status: "running" },
};

function VerdictPill({ verdict }: { verdict: Verdict }) {
  const pill = VERDICT_PILL[verdict];
  return <StatusPill status={pill.status} tone={verdict === "stale" ? "neutral" : undefined} label={pill.label === undefined ? undefined : t(pill.label)} className="min-w-16 justify-center" />;
}

type Filter = "all" | "fail" | "undecidable" | "notRun" | "derived";

/** Scenarios (spec §5.5): the list, the inspector and the run, confirm and correct actions. */
export function ScenariosScreen() {
  const params: { id?: string } = useParams({ strict: false });
  const go = useGo();
  const canEdit = useCanEdit();
  const queryClient = useQueryClient();
  const query = useQuery(scenariosQuery);
  const rules = useQuery(rulesQuery);
  const items = query.data?.items ?? [];
  const health = query.data?.health ?? [];
  const label = useLabels(items.flatMap(referencedIds));
  const seen = useRunMemory((state) => state.seen);
  const remember = useRunMemory((state) => state.remember);
  const [running, setRunning] = useState<ReadonlySet<string>>(new Set());
  const [filter, setFilter] = useState<Filter>("all");
  const [confirmingAll, setConfirmingAll] = useState(false);
  const data = query.data;
  useEffect(() => {
    if (data !== undefined) {
      remember(data.items);
    }
  }, [data, remember]);

  const verdictOf = (scenario: Scenario): Verdict => {
    if (running.has(scenario.id)) {
      return "running";
    }
    if (scenario.run !== null) {
      return scenario.run.result;
    }
    return seen[scenario.id] !== undefined ? "stale" : "notRun";
  };
  const run = useVersionedMutation<WriteResult, string[]>(
    (scope, ifMatch) => apiClient.POST("/api/scenarios/run", { params: { header: { "If-Match": ifMatch } }, body: { scope } }),
    query.data?.etag,
  );
  const runScope = (scope: string[]) => {
    setRunning(new Set(scope));
    run.mutate(scope, {
      onSuccess: async () => {
        await queryClient.invalidateQueries({ queryKey: ["scenarios"] });
        const fresh = queryClient.getQueryData<{ items: Scenario[] }>(scenariosQuery.queryKey)?.items ?? [];
        const passed = fresh.filter((item) => scope.includes(item.id) && item.run?.result === "pass").length;
        toast({ message: tc("scenarios.ran", scope.length, { pass: passed }) });
      },
      onSettled: () => setRunning(new Set()),
    });
  };
  const runAll = () => {
    const scope = items.filter(runnable).map((item) => item.id).sort();
    if (scope.length === 0) {
      toast({ message: t("scenarios.nothingToRun"), tone: "info" });
      return;
    }
    runScope(scope);
  };

  const counts = { pass: 0, fail: 0, undecidable: 0, notRun: 0 };
  for (const item of items) {
    const verdict = item.run?.result ?? "notRun";
    counts[verdict] += 1;
  }
  const summary = [String(items.length), ...(["pass", "fail", "undecidable", "notRun"] as const).filter((key) => counts[key] > 0 || key === "pass").map((key) => tf(("scenarios.summary." + key) as MessageKey, { count: counts[key] }))].join(" · ");
  const derived = items.filter(confirmable);
  const verdicts = new Set(items.map((item) => item.run?.result ?? "notRun"));
  const showFilters = verdicts.size > 1 || derived.length > 0;
  const shown = items.filter((item) =>
    filter === "all" ? true : filter === "derived" ? item.confirmation_status === "Derived" : (item.run?.result ?? "notRun") === filter,
  );
  const selected = items.find((item) => item.id === params.id) ?? null;

  // J / K move through the shown rows and ⌘. closes the inspector; keys typed into fields are ignored.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target !== null && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) || document.querySelector("[role=dialog]") !== null) {
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key === ".") {
        event.preventDefault();
        go("/scenarios");
        return;
      }
      if (event.metaKey || event.ctrlKey || event.altKey || (event.key !== "j" && event.key !== "k")) {
        return;
      }
      const index = shown.findIndex((item) => item.id === selected?.id);
      const next = event.key === "j" ? shown[Math.min(shown.length - 1, index + 1)] : shown[Math.max(0, index - 1)];
      if (next !== undefined && next.id !== selected?.id) {
        event.preventDefault();
        go(buildHref("/scenarios/$id", { id: next.id }));
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  const select = (event: MouseEvent<HTMLButtonElement>, id: string) => {
    event.preventDefault();
    go(buildHref("/scenarios/$id", { id }));
  };

  let list: ReactNode;
  if (query.isError) {
    list = (
      <div className="p-6">
        <ReadError error={query.error} messageKey="scenarios.loadFailed" onRetry={() => void query.refetch()} />
      </div>
    );
  } else if (query.isPending || !label.ready) {
    list = <SkeletonList rows={6} />;
  } else if (items.length === 0 && health.length === 0) {
    list = <EmptyState title={t("scenarios.empty.title")} description={t("scenarios.empty.body")} />;
  } else {
    list = (
      <ul aria-label={t("nav.scenarios")} className="m-0 list-none p-0">
        {shown.map((item) => {
          const verdict = verdictOf(item);
          return (
            <ListRow
              key={item.id}
              selected={item.id === selected?.id}
              title={scenarioTitle(item, label)}
              subtitle={scenarioSource(item, label, label.kind) ?? undefined}
              onSelect={(event) => select(event, item.id)}
              className="min-h-[60px] gap-3.5 px-6 py-2.5"
              trailing={
                <span className="flex shrink-0 items-center gap-2">
                  <StatusPill status={item.confirmation_status === "Confirmed" ? "confirmed" : "derived"} tone="neutral" />
                  <VerdictPill verdict={verdict} />
                </span>
              }
            />
          );
        })}
        {health.map((item) => (
          <ListRowError key={item.id} itemId={item.id} onRetry={() => void query.refetch()} />
        ))}
        {shown.length === 0 && <li className="list-none px-6 py-8 text-center text-callout text-fg-3">{t("scenarios.filter.none")}</li>}
      </ul>
    );
  }

  return (
    <WithInspector
      width="wide"
      label={t("scenarios.details")}
      empty={t("scenarios.select")}
      inspectorKey={selected?.id}
      inspector={selected === null || !label.ready ? null : <ScenarioDetail scenario={selected} verdict={verdictOf(selected)} label={label} rule={rules.data?.items.find((rule) => rule.id === tableOf(selected))} />}
      actions={
        selected === null || !canEdit ? undefined : (
          <ScenarioActions scenario={selected} etag={query.data?.etag ?? null} running={run.isPending} onRun={() => runScope([selected.id])} rule={rules.data?.items.find((rule) => rule.id === tableOf(selected))} label={label} />
        )
      }
    >
      <div className="flex min-h-full flex-col">
        <div className="flex flex-wrap items-center gap-3 border-b border-line px-6 pb-4 pt-6">
          <h1 className="m-0 font-display text-h2 font-bold tracking-display text-fg">{t("nav.scenarios")}</h1>
          {query.data !== undefined && <span className="text-footnote text-fg-3">{summary}</span>}
          <div className="flex-1" />
          {canEdit && query.data !== undefined && (
            <>
              <Button variant="outline" size={34} onClick={() => (derived.length === 0 ? toast({ message: t("scenarios.confirmAll.none"), tone: "info" }) : setConfirmingAll(true))}>
                {t("scenarios.confirmAll.open")}
              </Button>
              <Button variant="primary" size={34} pending={run.isPending} icon={<Play size={14} strokeWidth={1.6} aria-hidden="true" />} onClick={runAll}>
                {t("scenarios.runAll")}
              </Button>
            </>
          )}
        </div>
        {showFilters && (
          <div role="group" aria-label={t("scenarios.filter.label")} className="flex flex-wrap gap-2 border-b border-line px-6 py-2.5">
            {(["all", "fail", "undecidable", "notRun", "derived"] as const).map((key) => {
              const count = key === "all" ? items.length : key === "derived" ? items.filter((item) => item.confirmation_status === "Derived").length : counts[key];
              if (key !== "all" && count === 0) {
                return null;
              }
              return <FilterChip key={key} label={t(("scenarios.filter." + key) as MessageKey)} count={count} active={filter === key} onToggle={() => setFilter(key)} />;
            })}
          </div>
        )}
        <WriteError error={run.error} className="px-6 py-2" />
        {list}
      </div>
      {confirmingAll && <ConfirmAllSheet derived={derived} etag={query.data?.etag ?? null} label={label} onClose={() => setConfirmingAll(false)} />}
    </WithInspector>
  );
}

const CALLOUT: Readonly<Record<Verdict, { tone: string; icon: ReactNode }>> = {
  pass: { tone: "bg-pass-tint text-pass-fg", icon: <Check size={16} strokeWidth={1.6} aria-hidden="true" /> },
  fail: { tone: "bg-blocked-tint text-blocked-fg", icon: <X size={16} strokeWidth={1.6} aria-hidden="true" /> },
  undecidable: { tone: "bg-neutral-tint text-neutral-fg", icon: <CircleHelp size={16} strokeWidth={1.6} aria-hidden="true" /> },
  notRun: { tone: "bg-neutral-tint text-neutral-fg", icon: <Minus size={16} strokeWidth={1.6} aria-hidden="true" /> },
  stale: { tone: "bg-neutral-tint text-neutral-fg", icon: <Clock size={16} strokeWidth={1.6} aria-hidden="true" /> },
  running: { tone: "bg-running-tint text-running-fg", icon: <Clock size={16} strokeWidth={1.6} aria-hidden="true" /> },
};

function fetchRun(id: string) {
  return readData(apiClient.GET("/api/scenarios/{id}/run", { params: { path: { id } } })).catch((error: unknown) => {
    if (error instanceof ReadFailedError && error.status === 404) {
      return null;
    }
    throw error;
  });
}

function ScenarioDetail({ scenario, verdict, label, rule }: { scenario: Scenario; verdict: Verdict; label: Labels; rule: components["schemas"]["Rule"] | undefined }) {
  const go = useGo();
  const runQuery = useQuery({ queryKey: ["scenarios", "run", scenario.id, scenario.run?.result ?? null], queryFn: () => fetchRun(scenario.id), enabled: scenario.run !== null });
  const run = scenario.run === null ? null : (runQuery.data ?? scenario.run);
  const traceIds = run === null ? [] : [...run.trace.flatMap((text) => referencedIds({ ...scenario, given: [text], when: "{}", then: [], derived_from: [] })), ...(run.missing ?? []).map((m) => parseMissing(m)?.owner), ...(run.diff ?? []).flatMap((d) => referencedIds({ ...scenario, given: [d], when: "{}", then: [], derived_from: [] }))];
  const traceLabel = useLabels(traceIds);
  const columns = tableColumns(rule);
  const failures = run?.result === "fail" ? (run.diff ?? []).length : 0;
  const message: Record<Verdict, string> = {
    pass: t("scenarios.callout.pass"),
    fail: tc("scenarios.callout.fail", failures),
    undecidable: t("scenarios.callout.undecidable"),
    notRun: t("scenarios.callout.notRun"),
    stale: t("scenarios.callout.stale"),
    running: t("scenarios.callout.running"),
  };
  const source = scenarioSource(scenario, label, label.kind);
  const sourceRef = scenario.derived_from[0];
  const sourceHref = sourceRef === undefined ? null : elementHref(sourceRef, label.kind(sourceRef));
  const navigate = (event: MouseEvent<HTMLAnchorElement>, href: string) => {
    if (isPlainClick(event)) {
      event.preventDefault();
      go(href);
    }
  };
  const rows: { key: MessageKey; text: ReactNode }[] = [
    { key: "scenarios.given", text: scenario.given.length === 0 ? t("model.none") : scenario.given.map((fact) => describeFact(fact, label, columns.inputs)).join("; ") },
    { key: "scenarios.when", text: describeWhen(scenario.when, label) },
    { key: "scenarios.then", text: scenario.then.map((fact) => describeFact(fact, label, columns.outputs)).join("; ") },
  ];
  return (
    <>
      <div>
        <Eyebrow className="mb-2.5">{t("scenarios.eyebrow")}</Eyebrow>
        <h2 className="m-0 font-display text-title font-semibold leading-snug text-fg">{scenarioTitle(scenario, label)}</h2>
      </div>
      <table className="w-full overflow-hidden rounded-tile border-separate border-spacing-0 border border-line text-footnote leading-normal">
        <caption className="sr-only">{t("scenarios.gwt")}</caption>
        <tbody>
          {rows.map((row, index) => (
            <tr key={row.key}>
              <th scope="row" className={cn("w-16 py-3 pl-3.5 pr-2 text-left align-top text-eyebrow font-semibold uppercase leading-[1.9] tracking-[0.1em] text-fg-3", index > 0 && "border-t border-line")}>
                {t(row.key)}
              </th>
              <td className={cn("py-3 pr-3.5 align-top text-fg", index > 0 && "border-t border-line")}>{row.text}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className={cn("m-0 flex items-center gap-2.5 rounded-tile px-3.5 py-3 text-footnote font-medium", CALLOUT[verdict].tone)}>
        {CALLOUT[verdict].icon}
        {message[verdict]}
      </p>
      {run !== null && (verdict === "pass" || verdict === "fail" || verdict === "undecidable") && <Trace run={run} label={traceLabel} />}
      {source !== null && (
        <p className="m-0 text-footnote text-fg-2">
          {t("scenarios.derivedFrom")}{" "}
          {sourceHref === null ? (
            <span className="font-semibold text-fg">{source}</span>
          ) : (
            <a href={sourceHref} onClick={(event) => navigate(event, sourceHref)} className="font-semibold text-fg underline-offset-2 hover:underline">
              {source}
            </a>
          )}
        </p>
      )}
    </>
  );
}

const STEP_ICON: Readonly<Record<TraceStep["status"], { tone: string; icon: ReactNode }>> = {
  pass: { tone: "bg-pass-tint text-pass-fg", icon: <Check size={12} strokeWidth={2} aria-hidden="true" /> },
  fail: { tone: "bg-blocked-tint text-blocked-fg", icon: <X size={12} strokeWidth={2} aria-hidden="true" /> },
  missing: { tone: "bg-neutral-tint text-neutral-fg", icon: <CircleHelp size={12} strokeWidth={2} aria-hidden="true" /> },
  unknown: { tone: "bg-attention-tint text-attention-fg", icon: <CircleHelp size={12} strokeWidth={2} aria-hidden="true" /> },
};

const STEP_STATUS: Readonly<Record<TraceStep["status"], MessageKey>> = {
  pass: "scenarios.trace.status.pass",
  fail: "scenarios.trace.status.fail",
  missing: "scenarios.trace.status.missing",
  unknown: "scenarios.trace.status.unknown",
};

function Trace({ run, label }: { run: ScenarioRun; label: Labels }) {
  const go = useGo();
  const titleId = useId();
  const missingId = useId();
  const steps = readTrace(run.trace, label);
  const firstFail = steps.findIndex((step) => step.status === "fail");
  const diff = (run.diff ?? []).map((text) => parseDiff(text, label));
  const missing = (run.missing ?? []).map(parseMissing);
  return (
    <div className="flex flex-col gap-2.5">
      {run.result === "undecidable" && missing.length > 0 && (
        <div className="flex flex-col gap-2 rounded-tile border border-line p-3.5">
          <h3 id={missingId} className="m-0 text-caption font-semibold text-fg-2">
            {t("scenarios.missing.title")}
          </h3>
          <ul aria-labelledby={missingId} className="m-0 flex list-none flex-col gap-1.5 p-0 text-footnote">
            {missing.map((entry, index) => {
              if (entry === null) {
                return (
                  <li key={index} className="break-all font-mono text-caption text-fg-2">
                    {run.missing?.[index]}
                  </li>
                );
              }
              const href = elementHref(entry.owner, label.kind(entry.owner));
              return (
                <li key={index} className="flex flex-wrap items-baseline gap-x-1.5">
                  {href === null ? (
                    <span className="font-semibold text-fg">{label(entry.owner)}</span>
                  ) : (
                    <a
                      href={href}
                      onClick={(event) => {
                        if (isPlainClick(event)) {
                          event.preventDefault();
                          go(href);
                        }
                      }}
                      className="font-semibold text-fg underline underline-offset-2"
                    >
                      {label(entry.owner)}
                    </a>
                  )}
                  <span className="text-fg-3">{tf("scenarios.missing.needs", { key: entry.key.replace(/_/g, " "), kind: entry.kind.replace(/_/g, " ") })}</span>
                </li>
              );
            })}
          </ul>
        </div>
      )}
      <h3 id={titleId} className="m-0 text-caption font-semibold text-fg-2">
        {t("scenarios.trace.title")}
      </h3>
      <ol aria-labelledby={titleId} className="m-0 list-none p-0">
        {steps.map((step, index) => {
          const diverging = index === firstFail;
          const expected = diverging ? diff[0] : null;
          return (
            <li key={index} data-diverging={diverging ? "true" : undefined} className="flex gap-3">
              <div className="flex flex-col items-center">
                <span className={cn("grid size-5 shrink-0 place-items-center rounded-pill", STEP_ICON[step.status].tone)}>
                  {STEP_ICON[step.status].icon}
                  <span className="sr-only">{t(STEP_STATUS[step.status])}</span>
                </span>
                {index < steps.length - 1 && <span aria-hidden="true" className="min-h-4 w-px flex-1 bg-line" />}
              </div>
              <div className={cn("min-w-0 flex-1 pb-3.5 text-footnote", diverging && "-mt-1 mb-2 rounded-field bg-blocked-tint px-2.5 py-1.5")}>
                {diverging && <span className="block text-eyebrow font-semibold uppercase tracking-eyebrow text-blocked-fg">{t("scenarios.trace.firstDifference")}</span>}
                <span className={cn("block font-medium", diverging ? "text-blocked-fg" : "text-fg")}>{step.title}</span>
                {step.detail !== null && step.detail !== "" && <span className="block break-words font-mono text-[11.5px] leading-relaxed text-fg-3">{step.detail}</span>}
                {expected !== null && expected !== undefined && (
                  <span className="mt-1 block font-mono text-[11.5px] text-blocked-fg">{tf("scenarios.trace.expectedActual", { expected: expected.expected, actual: expected.actual })}</span>
                )}
              </div>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

function ScenarioActions({
  scenario,
  etag,
  running,
  onRun,
  rule,
  label,
}: {
  scenario: Scenario;
  etag: string | null;
  running: boolean;
  onRun: () => void;
  rule: components["schemas"]["Rule"] | undefined;
  label: Labels;
}) {
  const queryClient = useQueryClient();
  const [correcting, setCorrecting] = useState(false);
  const confirm = useVersionedMutation<WriteResult>(
    (_, ifMatch) => apiClient.POST("/api/scenarios/{id}/confirm", { params: { path: { id: scenario.id }, header: { "If-Match": ifMatch } } }),
    etag,
  );
  return (
    <>
      {confirmable(scenario) && (
        <Button
          variant="outline"
          size={34}
          pending={confirm.isPending}
          onClick={() =>
            confirm.mutate(undefined, {
              onSuccess: () => {
                void queryClient.invalidateQueries({ queryKey: ["scenarios"] });
                toast({ message: t("scenarios.confirmed") });
              },
            })
          }
        >
          {t("model.confirm")}
        </Button>
      )}
      {correctable(scenario) && (
        <Button variant="outline" size={34} onClick={() => setCorrecting(true)}>
          {t("scenarios.correct.open")}
        </Button>
      )}
      {runnable(scenario) ? (
        <Button variant="outline" size={34} pending={running} onClick={onRun}>
          {t("scenarios.runOne")}
        </Button>
      ) : (
        <span className="text-caption text-fg-3">{t("scenarios.runAfterConfirm")}</span>
      )}
      <WriteError error={confirm.error} className="w-full" />
      {correcting && <CorrectSheet scenario={scenario} etag={etag} rule={rule} label={label} onClose={() => setCorrecting(false)} />}
    </>
  );
}

function ConfirmAllSheet({ derived, etag, label, onClose }: { derived: readonly Scenario[]; etag: string | null; label: Labels; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [pending, setPending] = useState(false);
  const write = useVersionedMutation<WriteResult, { kind: "table" | "one"; id: string; etag: string | null }>(
    (target, ifMatch) =>
      target.kind === "table"
        ? apiClient.POST("/api/scenarios/confirm-derived", { params: { header: { "If-Match": ifMatch } }, body: { table_id: target.id } })
        : apiClient.POST("/api/scenarios/{id}/confirm", { params: { path: { id: target.id }, header: { "If-Match": ifMatch } } }),
    (target) => target.etag,
  );
  const tables = [...new Set(derived.map(tableOf).filter((id): id is string => id !== null))];
  const singles = derived.filter((item) => tableOf(item) === null).map((item) => item.id);
  const confirmAll = async () => {
    setPending(true);
    let current = etag;
    try {
      // Each confirmation is one revision; the next one is based on the revision it wrote.
      for (const target of [...tables.map((id) => ({ kind: "table" as const, id })), ...singles.map((id) => ({ kind: "one" as const, id }))]) {
        const result = await write.mutateAsync({ ...target, etag: current });
        current = revisionEtag(result.revision);
      }
      await queryClient.invalidateQueries({ queryKey: ["scenarios"] });
      toast({ message: tc("scenarios.confirmAll.done", derived.length) });
      onClose();
    } catch {
      // The error is shown in the sheet (a 412 opens the conflict sheet).
    } finally {
      setPending(false);
    }
  };
  return (
    <Sheet
      open
      onOpenChange={(open) => !open && onClose()}
      title={tc("scenarios.confirmAll.title", derived.length)}
      description={t("scenarios.confirmAll.description")}
      confirmLabel={t("scenarios.confirmAll.confirm")}
      pending={pending}
      onConfirm={() => void confirmAll()}
    >
      <ul className="m-0 flex list-none flex-col rounded-tile border border-line p-0">
        {derived.slice(0, 3).map((item) => (
          <li key={item.id} className="border-b border-line px-3.5 py-2.5 text-footnote font-medium text-fg last:border-b-0">
            {scenarioTitle(item, label)}
          </li>
        ))}
      </ul>
      {derived.length > 3 && <p className="m-0 text-caption text-fg-3">{tf("scenarios.confirmAll.more", { count: derived.length - 3 })}</p>}
      <WriteError error={write.error} />
    </Sheet>
  );
}

function parseFact(text: string): Record<string, unknown> | null {
  try {
    const value = JSON.parse(text) as unknown;
    return typeof value === "object" && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** A canonical decision-output fact with `index` set to the boolean `value` (sorted keys). */
function boolOutputs(text: string, index: string, value: boolean): string {
  const outputs = (parseFact(text)?.["decision_outputs"] ?? {}) as Record<string, unknown>;
  const next: Record<string, unknown> = {};
  for (const key of Object.keys({ ...outputs, [index]: null }).sort()) {
    next[key] = key === index ? { kind: "bool", value } : outputs[key];
  }
  return JSON.stringify({ decision_outputs: next });
}

/** Correct the expected outcome (correct_scenario): Yes/No for decision outputs, exact text otherwise. */
function CorrectSheet({ scenario, etag, rule, label, onClose }: { scenario: Scenario; etag: string | null; rule: components["schemas"]["Rule"] | undefined; label: Labels; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [facts, setFacts] = useState<string[]>(scenario.then);
  const baseId = useId();
  const columns = tableColumns(rule);
  const write = useVersionedMutation<WriteResult, string[]>(
    (then, ifMatch) => apiClient.POST("/api/scenarios/{id}/correct", { params: { path: { id: scenario.id }, header: { "If-Match": ifMatch } }, body: { then } }),
    etag,
  );
  const valid = facts.length > 0 && facts.every((fact) => parseFact(fact) !== null);
  return (
    <Sheet
      open
      onOpenChange={(open) => !open && onClose()}
      title={t("scenarios.correct.title")}
      description={t("scenarios.correct.description")}
      confirmLabel={t("model.save")}
      confirmDisabled={!valid}
      pending={write.isPending}
      onConfirm={() =>
        write.mutate(facts, {
          onSuccess: () => {
            void queryClient.invalidateQueries({ queryKey: ["scenarios"] });
            toast({ message: t("scenarios.corrected") });
            onClose();
          },
        })
      }
    >
      {facts.map((fact, index) => {
        const outputs = parseFact(fact)?.["decision_outputs"];
        const boolEntries =
          typeof outputs === "object" && outputs !== null
            ? Object.entries(outputs as Record<string, unknown>).filter(([, cell]) => (cell as Record<string, unknown> | null)?.["kind"] === "bool")
            : [];
        if (boolEntries.length > 0) {
          return boolEntries.map(([key, cell]) => {
            const name = columns.outputs[Number(key)] ?? tf("scenarios.fact.column", { n: Number(key) + 1 });
            const value = (cell as Record<string, unknown>)["value"] === true ? "yes" : "no";
            return (
              <div key={index + key} className="flex flex-col gap-1.5">
                <span className="text-caption font-semibold text-fg-2">{tf("scenarios.correct.then", { name })}</span>
                <SegmentedControl
                  label={tf("scenarios.correct.then", { name })}
                  value={value}
                  options={[
                    { value: "yes", label: t("model.rules.yes") },
                    { value: "no", label: t("model.rules.no") },
                  ]}
                  onChange={(next) => setFacts((current) => current.map((f, i) => (i === index ? boolOutputs(f, key, next === "yes") : f)))}
                />
              </div>
            );
          });
        }
        const fieldId = baseId + String(index);
        return (
          <div key={index} className="flex flex-col gap-1.5">
            <label htmlFor={fieldId} className="text-caption font-semibold text-fg-2">
              {tf("scenarios.correct.fact", { n: index + 1 })}
            </label>
            <p className="m-0 text-caption text-fg-3">{describeFact(scenario.then[index] ?? "", label, columns.outputs)}</p>
            <textarea
              id={fieldId}
              value={fact}
              spellCheck={false}
              onChange={(event) => setFacts((current) => current.map((f, i) => (i === index ? event.target.value : f)))}
              className="min-h-16 w-full resize-y rounded-field border border-line bg-card p-3 font-mono text-caption text-fg outline-none focus:border-accent focus:shadow-focus"
            />
          </div>
        );
      })}
      {!valid && <p className="m-0 text-caption text-blocked-fg">{t("scenarios.correct.invalid")}</p>}
      <WriteError error={write.error} />
    </Sheet>
  );
}
