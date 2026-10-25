import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Search } from "lucide-react";
import { useState } from "react";

import { apiClient } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, writeErrorMessage } from "../../api/useVersionedMutation";
import { hasRole, useMe } from "../../app/auth";
import { ReadFailure } from "../../components/ReadFailure";
import { Button, EmptyState, ListRowError, MonoId, ReasonSheet, SkeletonList, StatusPill, toast } from "../../components/ui";
import { t, tf, useLocaleStore, type MessageKey } from "../../i18n";
import { pillStatus } from "../requirements/status";
import { assumptionsQuery, decisionsQuery, warsawTime, type Decision } from "./data";

type Assumption = components["schemas"]["Assumption"];
type WriteResult = components["schemas"]["WriteResult"];

/** The role that resolves assumptions. */
const ANALYST = "analyst";

/** What a decision was about, from the reference it records. */
function decisionKind(decision: Decision): MessageKey {
  if ((decision.question_ref ?? null) !== null) {
    return "quality.history.kind.answer";
  }
  const proposal = decision.proposal_ref ?? "";
  if (proposal.startsWith("waiver-subject:")) {
    return "quality.history.kind.waiver";
  }
  return proposal === "" ? "quality.history.kind.other" : "quality.history.kind.proposal";
}

/** Quality → History: the decision log (newest first, searchable) and the assumptions. */
export function HistoryTab() {
  const decisions = useQuery(decisionsQuery);
  const locale = useLocaleStore((state) => state.locale);
  const [query, setQuery] = useState("");

  let body;
  if (decisions.isError) {
    body = (
      <div className="flex flex-col items-start gap-3 p-5 text-body text-fg-2">
        <ReadFailure error={decisions.error} messageKey="quality.history.loadFailed" />
        <Button variant="outline" size={30} onClick={() => void decisions.refetch()}>
          {t("quality.retry")}
        </Button>
      </div>
    );
  } else if (decisions.data === undefined) {
    body = <SkeletonList rows={6} />;
  } else if (decisions.data.length === 0) {
    body = <EmptyState visual="none" headingLevel={2} title={t("quality.history.empty")} />;
  } else {
    const q = query.trim().toLowerCase();
    const rows = [...decisions.data]
      .sort((a, b) => b.decided_at.localeCompare(a.decided_at))
      .filter((decision) =>
        q === ""
          ? true
          : [decision.decided_by, decision.rationale ?? "", t(decisionKind(decision)), decision.proposal_ref ?? "", decision.question_ref ?? "", decision.id].some((text) =>
              text.toLowerCase().includes(q),
            ),
      );
    body = (
      <table aria-label={t("quality.history.label")} className="w-full table-fixed border-collapse text-left text-footnote">
        <thead>
          <tr className="border-b border-line">
            <th scope="col" className="w-[150px] px-5 py-2.5 text-eyebrow font-semibold uppercase tracking-eyebrow text-fg-3">
              {t("quality.history.when")}
            </th>
            <th scope="col" className="w-[140px] py-2.5 pr-4 text-eyebrow font-semibold uppercase tracking-eyebrow text-fg-3">
              {t("quality.history.who")}
            </th>
            <th scope="col" className="py-2.5 pr-5 text-eyebrow font-semibold uppercase tracking-eyebrow text-fg-3">
              {t("quality.history.decision")}
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((decision) => (
            <tr key={decision.id} data-decision-id={decision.id} className="border-b border-line align-top">
              <td className="px-5 py-3.5 text-fg-3">
                <time dateTime={decision.decided_at}>{warsawTime(decision.decided_at, locale)}</time>
              </td>
              <td className="break-words py-3.5 pr-4 font-medium text-fg">{decision.decided_by}</td>
              <td className="py-3.5 pr-5">
                <div className="text-fg">{t(decisionKind(decision))}</div>
                {(decision.rationale ?? null) !== null && decision.rationale !== "" && (
                  <div className="mt-[3px] text-caption text-fg-3">{"“" + (decision.rationale ?? "") + "”"}</div>
                )}
              </td>
            </tr>
          ))}
          {rows.length === 0 && (
            <tr>
              <td colSpan={3} className="px-5 py-6 text-center text-fg-3">
                {t("quality.history.noMatch")}
              </td>
            </tr>
          )}
        </tbody>
      </table>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <div>
        <label className="mb-3 flex h-8 w-60 items-center gap-2 rounded-control border border-line bg-card px-2.5 text-fg-3 focus-within:border-accent focus-within:shadow-focus">
          <Search size={16} strokeWidth={1.6} aria-hidden="true" />
          <input
            type="search"
            aria-label={t("quality.history.search")}
            placeholder={t("quality.history.search")}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            className="min-w-0 flex-1 border-0 bg-transparent text-footnote text-fg outline-none placeholder:text-fg-3"
          />
        </label>
        <div className="overflow-hidden rounded-card border border-line bg-card">
          {body}
          <div className="px-5 py-2.5 text-eyebrow text-fg-3">{t("quality.history.timezone")}</div>
        </div>
      </div>
      <Assumptions />
    </div>
  );
}

function Assumptions() {
  const assumptions = useQuery(assumptionsQuery);
  const me = useMe();
  const locale = useLocaleStore((state) => state.locale);
  const queryClient = useQueryClient();
  const [resolving, setResolving] = useState<Assumption | null>(null);
  const resolve = useVersionedMutation<WriteResult, { id: string; reason: string }>(
    ({ id, reason }, ifMatch) => apiClient.POST("/api/assumptions/{id}/resolve", { params: { path: { id }, header: { "If-Match": ifMatch } }, body: { reason } }),
    assumptions.data?.etag,
  );
  const canResolve = hasRole(me.data, ANALYST);
  const failure =
    resolve.error === null || resolve.error.name === "PreconditionFailedError" ? undefined : tf("quality.writeFailed", { message: writeErrorMessage(resolve.error) ?? resolve.error.message });

  let body;
  if (assumptions.isError) {
    body = (
      <div className="flex flex-col items-start gap-3 p-5 text-body text-fg-2">
        <ReadFailure error={assumptions.error} messageKey="quality.assumptions.loadFailed" />
        <Button variant="outline" size={30} onClick={() => void assumptions.refetch()}>
          {t("quality.retry")}
        </Button>
      </div>
    );
  } else if (assumptions.data === undefined) {
    body = <SkeletonList rows={2} />;
  } else if (assumptions.data.items.length === 0 && assumptions.data.health.length === 0) {
    body = <p className="m-0 px-5 py-4 text-footnote text-fg-3">{t("quality.assumptions.empty")}</p>;
  } else {
    body = (
      <ul className="m-0 list-none p-0">
        {assumptions.data.items.map((assumption) => {
          const pill = pillStatus(assumption.status);
          const resolved = assumption.status.toLowerCase() === "resolved";
          const expires = assumption.expires_at ?? null;
          return (
            <li key={assumption.id} data-assumption-id={assumption.id} className="flex items-center gap-3.5 border-b border-line px-5 py-3.5 last:border-b-0">
              <div className="min-w-0 flex-1">
                <div className="text-callout font-medium text-fg">{assumption.statement}</div>
                <div className="mt-0.5 flex flex-wrap gap-x-3 text-caption text-fg-3">
                  <span>{tf("quality.assumptions.owner", { owner: assumption.owner_ref })}</span>
                  {expires !== null && <span>{tf("quality.assumptions.expires", { date: warsawTime(expires, locale) })}</span>}
                  <MonoId size={11}>{assumption.id}</MonoId>
                </div>
              </div>
              {pill === null ? <StatusPill status="open" label={assumption.status} /> : <StatusPill status={pill} />}
              {canResolve && !resolved && (
                <Button variant="outline" size={30} onClick={() => setResolving(assumption)}>
                  {t("quality.assumptions.resolve")}
                </Button>
              )}
            </li>
          );
        })}
        {assumptions.data.health.map((health) => (
          <ListRowError key={health.id} itemId={health.id} onRetry={() => void assumptions.refetch()} />
        ))}
      </ul>
    );
  }

  return (
    <section aria-label={t("quality.assumptions.title")} className="flex flex-col gap-3">
      <h2 className="m-0 font-display text-title-sm font-semibold tracking-display text-fg">{t("quality.assumptions.title")}</h2>
      <div className="overflow-hidden rounded-card border border-line bg-card">{body}</div>
      <ReasonSheet
        open={resolving !== null}
        onOpenChange={(open) => {
          if (!open) {
            setResolving(null);
            resolve.reset();
          }
        }}
        title={t("quality.assumptions.sheet.title")}
        description={tf("quality.assumptions.sheet.description", { statement: resolving?.statement ?? "" })}
        confirmLabel={t("quality.assumptions.sheet.confirm")}
        pending={resolve.isPending}
        error={failure}
        onConfirm={(reason) => {
          if (resolving === null) {
            return;
          }
          resolve.mutate(
            { id: resolving.id, reason },
            {
              onSuccess: () => {
                setResolving(null);
                void queryClient.invalidateQueries();
                toast({ message: t("quality.assumptions.done") });
              },
              onError: (error) => {
                if (error.name === "PreconditionFailedError") {
                  setResolving(null);
                }
              },
            },
          );
        }}
      />
    </section>
  );
}
