import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { apiClient, readReviewed, ReviewedEtag, useReviewedEtag, type ReviewedItems } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { t } from "../../i18n";
import { writeErrorCode } from "../questions/QuestionsPage";
import { ScenarioTrace } from "./ScenarioTrace";
import { HealthMarkers, type ItemHealth } from "../../components/HealthMarkers";
import { ReadFailure } from "../../components/ReadFailure";
import { RoleGate } from "../../components/RoleGate";

type Scenario = components["schemas"]["Scenario"];
type ScenarioStatus = components["schemas"]["ScenarioStatus"];
type WriteResult = components["schemas"]["WriteResult"];

/** Every scenario query shares this root and is refetched after a scenario write. */
export const scenariosQueryKey = ["scenarios"] as const;

const STATUSES: readonly ScenarioStatus[] = ["Proposed", "Accepted", "Rejected", "Superseded", "Deprecated", "Suspect"];

/** The server filters of GET /api/scenarios; empty values are omitted. */
export interface ScenarioFilters {
  readonly requirement: string;
  readonly status: ScenarioStatus | "";
  readonly derived: "" | "true" | "false";
}

const NO_FILTERS: ScenarioFilters = { requirement: "", status: "", derived: "" };

function fetchScenarios(filters: ScenarioFilters): Promise<ReviewedItems<Scenario> & { readonly health?: ItemHealth[] }> {
  const query: { requirement?: string; status?: ScenarioStatus; derived?: boolean } = {};
  if (filters.requirement !== "") {
    query.requirement = filters.requirement;
  }
  if (filters.status !== "") {
    query.status = filters.status;
  }
  if (filters.derived !== "") {
    query.derived = filters.derived === "true";
  }
  return readReviewed(apiClient.GET("/api/scenarios", { params: { query } }));
}

/** The server-confirmable scenarios: exactly the Proposed + Derived ones the API returns. */
function fetchConfirmable(): Promise<ReviewedItems<Scenario>> {
  return readReviewed(apiClient.GET("/api/scenarios", { params: { query: { status: "Proposed", derived: true } } }));
}

function parse(text: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return undefined;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** The DecisionTable of a scenario whose `when` is exactly {"decision_table": "<id>"} and which is derived from it. */
export function decisionTableOf(scenario: Scenario): string | null {
  const when = parse(scenario.when);
  if (!isRecord(when)) {
    return null;
  }
  const keys = Object.keys(when);
  const table = when["decision_table"];
  if (keys.length !== 1 || keys[0] !== "decision_table" || typeof table !== "string") {
    return null;
  }
  return scenario.derived_from.includes(table) ? table : null;
}

/** DecisionTable bulk groups, built only from the server-confirmable scenarios (server order). */
export function decisionTableGroups(confirmable: readonly Scenario[]): Map<string, string[]> {
  const groups = new Map<string, string[]>();
  for (const scenario of confirmable) {
    const table = decisionTableOf(scenario);
    if (table !== null) {
      groups.set(table, [...(groups.get(table) ?? []), scenario.id]);
    }
  }
  return groups;
}

/** Correction is offered for Proposed + Derived and Accepted + Confirmed scenarios only. */
function correctable(scenario: Scenario): boolean {
  return (scenario.status === "Proposed" && scenario.confirmation_status === "Derived") || (scenario.status === "Accepted" && scenario.confirmation_status === "Confirmed");
}

/** Only Accepted + Confirmed scenarios can be run. */
function runnable(scenario: Scenario): boolean {
  return scenario.status === "Accepted" && scenario.confirmation_status === "Confirmed";
}

function WriteError({ error }: { error: Error | null }) {
  if (error === null || !(error instanceof WriteFailedError)) {
    return null;
  }
  return (
    <p role="alert" className="text-sm text-red-800" data-testid="scenario-write-error">
      {t("scenarios.writeFailed")} <span className="font-mono">{writeErrorCode(error) ?? String(error.status)}</span> <span>{writeErrorMessage(error)}</span>
    </p>
  );
}

/** One canonical JSON text as semantic material: its fields when it is an object, with the exact text kept. */
function SemanticValue({ text }: { text: string }) {
  const value = parse(text);
  return (
    <div data-semantic-text={text}>
      {isRecord(value) ? (
        <dl className="grid grid-cols-[auto_1fr] gap-x-2">
          {Object.entries(value).map(([key, field]) => (
            <div key={key} className="contents">
              <dt className="font-mono text-slate-600">{key}</dt>
              <dd className="font-mono">{typeof field === "string" ? field : JSON.stringify(field)}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <code className="break-all">{text}</code>
      )}
    </div>
  );
}

function CorrectionEditor({ scenario, onDone }: { scenario: Scenario; onDone: () => void }) {
  const queryClient = useQueryClient();
  const [rows, setRows] = useState<string[]>(scenario.then);
  const correct = useVersionedMutation<WriteResult, string[]>(
    (then, ifMatch) => apiClient.POST("/api/scenarios/{id}/correct", { params: { path: { id: scenario.id }, header: { "If-Match": ifMatch } }, body: { then } }),
    useReviewedEtag(),
  );
  return (
    <form
      aria-label={t("scenarios.correct.title")}
      className="space-y-1 rounded border p-2"
      onSubmit={(event) => {
        event.preventDefault();
        correct.mutate(rows, {
          onSuccess: () => {
            void queryClient.invalidateQueries({ queryKey: scenariosQueryKey });
            onDone();
          },
        });
      }}
    >
      <h4 className="font-semibold">{t("scenarios.correct.title")}</h4>
      {rows.map((row, index) => (
        <div key={index} className="flex gap-1">
          <textarea
            className="w-full rounded border p-1 font-mono text-xs"
            value={row}
            data-testid="correct-then"
            onChange={(event) => setRows((current) => current.map((r, i) => (i === index ? event.target.value : r)))}
          />
          <button type="button" className="rounded border px-2 text-xs" onClick={() => setRows((current) => current.filter((_, i) => i !== index))}>
            {t("scenarios.correct.removeRow")}
          </button>
        </div>
      ))}
      <button type="button" className="rounded border px-2 text-xs" onClick={() => setRows((current) => [...current, ""])}>
        {t("scenarios.correct.addRow")}
      </button>
      <button type="submit" className="ml-2 rounded border px-3 py-1" data-testid="correct-submit" disabled={correct.isPending || rows.length === 0}>
        {t("scenarios.correct.submit")}
      </button>
      <WriteError error={correct.error} />
    </form>
  );
}

function ScenarioCard({
  scenario,
  confirmable,
  selected,
  onSelect,
}: {
  scenario: Scenario;
  confirmable: boolean;
  selected: boolean;
  onSelect: (id: string, on: boolean) => void;
}) {
  const queryClient = useQueryClient();
  const [correcting, setCorrecting] = useState(false);
  const confirm = useVersionedMutation<WriteResult>(
    (_, ifMatch) => apiClient.POST("/api/scenarios/{id}/confirm", { params: { path: { id: scenario.id }, header: { "If-Match": ifMatch } } }),
    useReviewedEtag(),
  );
  const when = parse(scenario.when);
  const anchor = isRecord(when) && Object.keys(when).length === 1 ? Object.entries(when)[0] : undefined;
  return (
    <li className="space-y-2 rounded border p-3 text-sm" data-scenario-id={scenario.id}>
      <div className="flex items-center justify-between gap-2">
        <span className="font-mono font-semibold">{scenario.id}</span>
        <span className="font-mono" data-testid="run-status" data-run={scenario.run === null ? "none" : scenario.run.result}>
          {scenario.run === null ? t("scenarios.run.notRun") : t(scenario.run.result === "pass" ? "scenarios.run.pass" : scenario.run.result === "fail" ? "scenarios.run.fail" : "scenarios.run.undecidable")}
        </span>
      </div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-2">
        <dt>{t("scenarios.status")}</dt>
        <dd className="font-mono" data-field="status">
          {scenario.status}
        </dd>
        <dt>{t("scenarios.confirmationStatus")}</dt>
        <dd className="font-mono" data-field="confirmation_status">
          {scenario.confirmation_status}
        </dd>
        <dt>{t("scenarios.derivedFrom")}</dt>
        <dd className="font-mono" data-field="derived_from">
          {scenario.derived_from.length === 0 ? t("scenarios.none") : scenario.derived_from.join(", ")}
        </dd>
      </dl>
      <section aria-label={t("scenarios.given")}>
        <h4 className="font-semibold">{t("scenarios.given")}</h4>
        {scenario.given.length === 0 ? (
          <p>{t("scenarios.none")}</p>
        ) : (
          <ul className="space-y-1" data-testid="given">
            {scenario.given.map((text, index) => (
              <li key={index}>
                <SemanticValue text={text} />
              </li>
            ))}
          </ul>
        )}
      </section>
      <section aria-label={t("scenarios.when")}>
        <h4 className="font-semibold">{t("scenarios.when")}</h4>
        {anchor === undefined ? (
          <code className="break-all" data-testid="when">
            {scenario.when}
          </code>
        ) : (
          <p data-testid="when" data-semantic-text={scenario.when}>
            <span className="font-mono text-slate-600">{anchor[0]}</span> <span className="font-mono">{typeof anchor[1] === "string" ? anchor[1] : JSON.stringify(anchor[1])}</span>
          </p>
        )}
      </section>
      <section aria-label={t("scenarios.then")}>
        <h4 className="font-semibold">{t("scenarios.then")}</h4>
        <ul className="space-y-1" data-testid="then">
          {scenario.then.map((text, index) => (
            <li key={index}>
              <SemanticValue text={text} />
            </li>
          ))}
        </ul>
      </section>
      <div className="flex flex-wrap items-center gap-2">
        <RoleGate quiet>
        {confirmable && (
          <button
            type="button"
            className="rounded border px-3 py-1"
            data-testid="scenario-confirm"
            disabled={confirm.isPending}
            onClick={() => confirm.mutate(undefined, { onSuccess: () => void queryClient.invalidateQueries({ queryKey: scenariosQueryKey }) })}
          >
            {t("scenarios.confirm")}
          </button>
        )}
        {correctable(scenario) && !correcting && (
          <button type="button" className="rounded border px-3 py-1" data-testid="scenario-correct" onClick={() => setCorrecting(true)}>
            {t("scenarios.correct.title")}
          </button>
        )}
        </RoleGate>
        {runnable(scenario) && (
          <label className="text-sm">
            <input type="checkbox" checked={selected} data-testid="run-select" onChange={(event) => onSelect(scenario.id, event.target.checked)} />{" "}
            {t("scenarios.run.select")}
          </label>
        )}
      </div>
      <WriteError error={confirm.error} />
      {/* Seeded from the server Then rows; a server change resets the editor (Fable #221). */}
      {correcting && <CorrectionEditor key={JSON.stringify(scenario.then)} scenario={scenario} onDone={() => setCorrecting(false)} />}
      {scenario.run !== null && <ScenarioTrace run={scenario.run} />}
    </li>
  );
}

/** Bulk confirmation of one DecisionTable's derived scenarios, behind an explicit three-sample prompt. */
function BulkConfirm({ groups, etag }: { groups: Map<string, string[]>; etag: string | null | undefined }) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState<string | null>(null);
  const bulk = useVersionedMutation<WriteResult, string>(
    (tableId, ifMatch) => apiClient.POST("/api/scenarios/confirm-derived", { params: { header: { "If-Match": ifMatch } }, body: { table_id: tableId } }),
    etag,
  );
  if (groups.size === 0) {
    return null;
  }
  const samples = open === null ? [] : (groups.get(open) ?? []);
  return (
    <section aria-label={t("scenarios.bulk.title")} className="space-y-2 rounded border p-3 text-sm">
      <h2 className="font-semibold">{t("scenarios.bulk.title")}</h2>
      <ul className="space-y-1">
        {[...groups.entries()].map(([table, ids]) => (
          <li key={table} data-bulk-table={table}>
            <span className="font-mono">{table}</span> <span>{ids.length}</span>{" "}
            <button type="button" className="rounded border px-2" data-testid="bulk-open" disabled={open !== null || bulk.isPending} onClick={() => setOpen(table)}>
              {t("scenarios.bulk.open")}
            </button>
          </li>
        ))}
      </ul>
      {open !== null && (
        <div role="alertdialog" aria-label={t("scenarios.bulk.title")} className="space-y-2 rounded border border-amber-600 p-2" data-testid="bulk-prompt">
          <p>
            {t("scenarios.bulk.table")} <span className="font-mono" data-testid="bulk-table">{open}</span>
          </p>
          <p>{t("scenarios.bulk.allWarning")}</p>
          <ul className="font-mono" data-testid="bulk-samples">
            {samples.slice(0, 3).map((id, index) => (
              <li key={index}>{id}</li>
            ))}
          </ul>
          {samples.length > 3 && <p data-testid="bulk-sample-note">{t("scenarios.bulk.sampleOnly")}</p>}
          <div className="flex gap-2">
            <button
              type="button"
              className="rounded border px-3 py-1"
              data-testid="bulk-confirm"
              disabled={bulk.isPending}
              onClick={() =>
                bulk.mutate(open, {
                  onSuccess: () => {
                    setOpen(null);
                    void queryClient.invalidateQueries({ queryKey: scenariosQueryKey });
                  },
                })
              }
            >
              {t("scenarios.bulk.confirm")}
            </button>
            <button type="button" className="rounded border px-3 py-1" data-testid="bulk-cancel" onClick={() => setOpen(null)}>
              {t("scenarios.cancel")}
            </button>
          </div>
        </div>
      )}
      <WriteError error={bulk.error} />
    </section>
  );
}

/** The U2.5 Scenarios view (v2 U8 as refined by the v3 task): cards, server filters, confirmation, correction, run scope and traces. */
export function ScenariosPage() {
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState<ScenarioFilters>(NO_FILTERS);
  const [filters, setFilters] = useState<ScenarioFilters>(NO_FILTERS);
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const scenarios = useQuery({ queryKey: [...scenariosQueryKey, "list", filters], queryFn: () => fetchScenarios(filters) });
  const confirmable = useQuery({ queryKey: [...scenariosQueryKey, "confirmable"], queryFn: fetchConfirmable });
  const run = useVersionedMutation<WriteResult, string[]>(
    (scope, ifMatch) => apiClient.POST("/api/scenarios/run", { params: { header: { "If-Match": ifMatch } }, body: { scope } }),
    scenarios.data?.etag,
  );

  const confirmableIds = new Set((confirmable.data?.items ?? []).map((s) => s.id));
  const groups = decisionTableGroups(confirmable.data?.items ?? []);
  const runnableIds = new Set((scenarios.data?.items ?? []).filter(runnable).map((s) => s.id));
  const scope = [...selected].filter((id) => runnableIds.has(id)).sort();

  const onSelect = (id: string, on: boolean) =>
    setSelected((current) => {
      const next = new Set(current);
      if (on) {
        next.add(id);
      } else {
        next.delete(id);
      }
      return next;
    });

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("scenarios.title")}</h1>
      <form
        aria-label={t("scenarios.filters")}
        className="flex flex-wrap items-end gap-2 text-sm"
        onSubmit={(event) => {
          event.preventDefault();
          setFilters(draft);
        }}
      >
        <label>
          {t("scenarios.filter.requirement")}
          <input className="ml-1 rounded border p-1 font-mono" value={draft.requirement} data-filter="requirement" onChange={(e) => setDraft((d) => ({ ...d, requirement: e.target.value }))} />
        </label>
        <label>
          {t("scenarios.filter.status")}
          <select className="ml-1 rounded border" value={draft.status} data-filter="status" onChange={(e) => setDraft((d) => ({ ...d, status: e.target.value as ScenarioStatus | "" }))}>
            <option value="">{t("scenarios.filter.any")}</option>
            {STATUSES.map((status) => (
              <option key={status} value={status}>
                {status}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("scenarios.filter.derived")}
          <select className="ml-1 rounded border" value={draft.derived} data-filter="derived" onChange={(e) => setDraft((d) => ({ ...d, derived: e.target.value as ScenarioFilters["derived"] }))}>
            <option value="">{t("scenarios.filter.any")}</option>
            <option value="true">{t("scenarios.filter.derivedYes")}</option>
            <option value="false">{t("scenarios.filter.derivedNo")}</option>
          </select>
        </label>
        <button type="submit" className="rounded border px-3 py-1">
          {t("scenarios.applyFilters")}
        </button>
      </form>

      <RoleGate>
        <BulkConfirm groups={groups} etag={confirmable.data?.etag} />
      </RoleGate>

      <RoleGate>
      <section aria-label={t("scenarios.run.title")} className="flex flex-wrap items-center gap-2 text-sm">
        <span>
          {t("scenarios.run.selected")} <span data-testid="run-count">{scope.length}</span>
        </span>
        <button
          type="button"
          className="rounded border px-3 py-1"
          data-testid="run-submit"
          disabled={run.isPending || scope.length === 0}
          onClick={() => {
            if (scope.length === 0) {
              return;
            }
            run.mutate(scope, {
              onSuccess: () => {
                setSelected(new Set());
                void queryClient.invalidateQueries({ queryKey: scenariosQueryKey });
              },
            });
          }}
        >
          {t("scenarios.run.submit")}
        </button>
        <WriteError error={run.error} />
      </section>
      </RoleGate>

      {scenarios.isError && <ReadFailure error={scenarios.error} messageKey="scenarios.loadFailed" />}
      {scenarios.data === undefined && !scenarios.isError && <p>{t("scenarios.loading")}</p>}
      <HealthMarkers health={scenarios.data?.health} />
      {scenarios.data !== undefined && scenarios.data.items.length === 0 && <p>{t("scenarios.empty")}</p>}
      {scenarios.data !== undefined && scenarios.data.items.length > 0 && (
        <ReviewedEtag.Provider value={scenarios.data.etag}>
        <ul className="space-y-3">
          {scenarios.data.items.map((scenario) => (
            <ScenarioCard
              key={scenario.id}
              scenario={scenario}
              confirmable={confirmableIds.has(scenario.id)}
              selected={selected.has(scenario.id)}
              onSelect={onSelect}
            />
          ))}
        </ul>
        </ReviewedEtag.Provider>
      )}
    </div>
  );
}
