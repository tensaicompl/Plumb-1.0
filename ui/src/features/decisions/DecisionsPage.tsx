import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";

import { apiClient } from "../../api/client";
import type { components } from "../../api/schema";
import { t, type MessageKey } from "../../i18n";

type Decision = components["schemas"]["Decision"];

/** The decisions list query root. */
export const decisionsQueryKey = ["decisions"] as const;

async function fetchDecisions(): Promise<Decision[]> {
  const { data, response } = await apiClient.GET("/api/decisions");
  if (data === undefined) {
    throw new Error("GET /api/decisions failed with HTTP " + String(response.status));
  }
  return data.items;
}

function Item({ labelKey, name, children }: { labelKey: MessageKey; name: string; children: ReactNode }) {
  return (
    <div className="contents" data-field={name}>
      <dt className="text-slate-600">{t(labelKey)}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function Optional({ value }: { value: string | null | undefined }) {
  return value === undefined || value === null ? <span className="text-slate-500">{t("decisions.none")}</span> : <span className="font-mono">{value}</span>;
}

/**
 * U2.7 Decisions (Hotfix 082): the read-only ResolutionDecision log from GET /api/decisions.
 * The API enumerates ResolutionDecision records only, so every row is a Resolution decision;
 * no type is ever derived from an ID or an answer.
 */
export function DecisionsPage() {
  const decisions = useQuery({ queryKey: decisionsQueryKey, queryFn: fetchDecisions });

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("decisions.title")}</h1>
      <p className="text-sm" data-testid="decisions-boundary">
        {t("decisions.boundary")}
      </p>
      {decisions.isError && <p role="alert">{t("decisions.loadFailed")}</p>}
      {decisions.data === undefined && !decisions.isError && <p>{t("decisions.loading")}</p>}
      {decisions.data !== undefined && decisions.data.length === 0 && <p>{t("decisions.empty")}</p>}
      {decisions.data !== undefined && decisions.data.length > 0 && (
        <ul className="space-y-2">
          {decisions.data.map((decision) => (
            <li key={decision.id} className="rounded border p-2 text-sm" data-decision-id={decision.id}>
              <p className="font-semibold" data-testid="decision-type">
                {t("decisions.type.resolution")}
              </p>
              <dl className="grid grid-cols-[auto_1fr] gap-x-2">
                <Item labelKey="decisions.field.id" name="id">
                  <span className="font-mono">{decision.id}</span>
                </Item>
                <Item labelKey="decisions.field.proposal" name="proposal_ref">
                  <Optional value={decision.proposal_ref} />
                </Item>
                <Item labelKey="decisions.field.question" name="question_ref">
                  <Optional value={decision.question_ref} />
                </Item>
                <Item labelKey="decisions.field.decidedBy" name="decided_by">
                  <span className="font-mono">{decision.decided_by}</span>
                </Item>
                <Item labelKey="decisions.field.decidedAt" name="decided_at">
                  <span className="font-mono">{decision.decided_at}</span>
                </Item>
                <Item labelKey="decisions.field.rationale" name="rationale">
                  {decision.rationale === undefined || decision.rationale === null ? <span className="text-slate-500">{t("decisions.none")}</span> : decision.rationale}
                </Item>
              </dl>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
