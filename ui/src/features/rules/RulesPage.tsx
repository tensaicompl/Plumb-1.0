import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";

import { apiClient, readReviewed, ReviewedEtag, useReviewedEtag, type ReviewedItems } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { t, type MessageKey } from "../../i18n";
import { writeErrorCode } from "../../components/QuestionAnswerCard";
import { HealthMarkers, type ItemHealth } from "../../components/HealthMarkers";
import { ReadFailure } from "../../components/ReadFailure";
import { RoleGate } from "../../components/RoleGate";

type Rule = components["schemas"]["Rule"];
type RuleCellRequest = components["schemas"]["RuleCellRequest"];
type WriteResult = components["schemas"]["WriteResult"];

/** Every rule query shares this root and is refetched after a cell write. */
export const rulesQueryKey = ["rules"] as const;

function fetchRules(): Promise<ReviewedItems<Rule> & { readonly health?: ItemHealth[] }> {
  return readReviewed(apiClient.GET("/api/model/rules"));
}

/**
 * One analysis result (Hotfix 065): `null` is not applicable / not analyzable, `[]` is analysed
 * with nothing found, and a non-empty list shows the exact server strings.
 */
function Analysis({ name, titleKey, noneKey, items }: { name: string; titleKey: MessageKey; noneKey: MessageKey; items: readonly string[] | null | undefined }) {
  const state = items === undefined || items === null ? "not-analyzable" : items.length === 0 ? "none" : "found";
  return (
    <div className="text-sm" data-analysis={name} data-state={state}>
      <h4 className="font-semibold">{t(titleKey)}</h4>
      {(items === undefined || items === null) && <p className="text-slate-600">{t("rules.notAnalyzable")}</p>}
      {items !== undefined && items !== null && items.length === 0 && <p>{t(noneKey)}</p>}
      {items !== undefined && items !== null && items.length > 0 && (
        <ul className="font-mono">
          {items.map((item, index) => (
            <li key={index}>{item}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** The decision-table rows exactly as the server returns them; `null` is not a table. */
function Rows({ rows }: { rows: Rule["rows"] }) {
  if (rows === undefined || rows === null) {
    return (
      <p className="text-sm text-slate-600" data-testid="rows-not-applicable">
        {t("rules.rowsNotApplicable")}
      </p>
    );
  }
  if (rows.length === 0) {
    return <p className="text-sm">{t("rules.noRows")}</p>;
  }
  return (
    <table className="text-sm" data-testid="rule-rows">
            <caption className="sr-only">{t("rules.caption")}</caption>
      <thead>
        <tr>
          <th scope="col" className="pr-2 text-left">{t("rules.row")}</th>
          <th scope="col" className="pr-2 text-left">{t("rules.when")}</th>
          <th scope="col" className="pr-2 text-left">{t("rules.then")}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row, index) => (
          <tr key={index} data-row={index}>
            <td className="pr-2 font-mono">{index}</td>
            <td className="pr-2 font-mono" data-cells="when">
              {row.when.join(" | ")}
            </td>
            <td className="pr-2 font-mono" data-cells="then">
              {row.then.join(" | ")}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/**
 * The explicit DecisionTable cell fill (Hotfix 080): the user enters the exact cell address
 * (row:<r>:output:<c>), the exact canonical outcome JSON and a reason; one submission is one
 * versioned POST /api/model/rules/{id}/cells, after which the server rules are refetched.
 */
function CellFill({ rule }: { rule: Rule }) {
  const queryClient = useQueryClient();
  const fill = useVersionedMutation<WriteResult, RuleCellRequest>(
    (body, ifMatch) =>
      apiClient.POST("/api/model/rules/{id}/cells", {
        params: { path: { id: rule.id }, header: { "If-Match": ifMatch } },
        body,
      }),
    useReviewedEtag(),
  );
  const [cell, setCell] = useState("");
  const [outcome, setOutcome] = useState("");
  const [reason, setReason] = useState("");

  const submit = (event: FormEvent) => {
    event.preventDefault();
    fill.mutate({ cell, outcome, reason }, { onSuccess: () => void queryClient.invalidateQueries({ queryKey: rulesQueryKey }) });
  };

  return (
    <form aria-label={t("rules.fill.title")} className="space-y-1 rounded border p-2" onSubmit={submit} data-testid="cell-fill">
      <h4 className="font-semibold">{t("rules.fill.title")}</h4>
      {fill.error instanceof WriteFailedError && (
        <p role="alert" className="text-sm text-red-800" data-testid="rule-write-error">
          {t("rules.writeFailed")} <span className="font-mono">{writeErrorCode(fill.error) ?? String(fill.error.status)}</span> <span>{writeErrorMessage(fill.error)}</span>
        </p>
      )}
      <label className="block text-sm">
        {t("rules.fill.cell")}
        <input className="ml-2 rounded border p-1 font-mono" value={cell} data-testid="fill-cell" onChange={(e) => setCell(e.target.value)} />
      </label>
      <label className="block text-sm">
        {t("rules.fill.outcome")}
        <textarea className="ml-2 rounded border p-1 font-mono" value={outcome} data-testid="fill-outcome" onChange={(e) => setOutcome(e.target.value)} />
      </label>
      <label className="block text-sm">
        {t("rules.fill.reason")}
        <input className="ml-2 rounded border p-1" value={reason} data-testid="fill-reason" onChange={(e) => setReason(e.target.value)} />
      </label>
      <button
        type="submit"
        className="rounded border px-3 py-1"
        data-testid="fill-submit"
        disabled={fill.isPending || cell === "" || outcome === "" || reason.trim() === ""}
      >
        {t("rules.fill.submit")}
      </button>
    </form>
  );
}

function RuleItem({ rule }: { rule: Rule }) {
  return (
    <li className="space-y-2 rounded border p-2" data-rule-id={rule.id} data-kind={rule.kind}>
      <h3 className="font-semibold">
        {rule.name} <span className="font-mono text-sm">{rule.id}</span>
      </h3>
      <p className="text-sm">{t(rule.kind === "decision_table" ? "rules.kind.decisionTable" : "rules.kind.rule")}</p>
      <div className="text-sm">
        <h4 className="font-semibold">{t("rules.conditions")}</h4>
        {rule.conditions.length === 0 ? (
          <p>{t("rules.none")}</p>
        ) : (
          <ul className="font-mono">
            {rule.conditions.map((condition, index) => (
              <li key={index}>{condition}</li>
            ))}
          </ul>
        )}
      </div>
      <Rows rows={rule.rows} />
      <Analysis name="uncovered" titleKey="rules.uncovered" noneKey="rules.noUncovered" items={rule.uncovered_cells} />
      <Analysis name="overlaps" titleKey="rules.overlaps" noneKey="rules.noOverlaps" items={rule.overlaps} />
      {rule.kind === "decision_table" && (
        <RoleGate>
          <CellFill rule={rule} />
        </RoleGate>
      )}
    </li>
  );
}

/** U2.6 Rules (Hotfix 080): server rules and decision tables with coverage and one cell-fill write. */
export function RulesPage() {
  const rules = useQuery({ queryKey: rulesQueryKey, queryFn: fetchRules });

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("rules.title")}</h1>
      {rules.isError && <ReadFailure error={rules.error} messageKey="rules.loadFailed" />}
      {rules.data === undefined && !rules.isError && <p>{t("rules.loading")}</p>}
      <HealthMarkers health={rules.data?.health} />
      {rules.data !== undefined && rules.data.items.length === 0 && <p>{t("rules.empty")}</p>}
      {rules.data !== undefined && rules.data.items.length > 0 && (
        <ReviewedEtag.Provider value={rules.data.etag}>
          <ul className="space-y-2">
            {rules.data.items.map((rule) => (
              <RuleItem key={rule.id} rule={rule} />
            ))}
          </ul>
        </ReviewedEtag.Provider>
      )}
    </div>
  );
}
