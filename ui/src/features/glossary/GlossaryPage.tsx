import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { apiClient, readReviewed, revisionEtag, type Reviewed } from "../../api/client";
import { isCursorBaselineChanged, useCursorPages, useCursorRecovery } from "../../api/useCursorPages";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { t } from "../../i18n";
import { errorCode, isBaselineChanged, SemanticChangePreviewPanel, type SemanticChangePreview } from "../requirements/RequirementDetail";
import { ReadFailure } from "../../components/ReadFailure";

type GlossaryPage = components["schemas"]["GlossaryPage"];
type WriteResult = components["schemas"]["WriteResult"];

/** The glossary page size requested from the server. */
export const GLOSSARY_PAGE_SIZE = 25;

/** The query key of one glossary page. */
export function glossaryQueryKey(cursor: string | null) {
  return ["glossary", cursor] as const;
}

function fetchGlossary(cursor: string | null): Promise<Reviewed<GlossaryPage>> {
  const query: { cursor?: string; limit: number } = { limit: GLOSSARY_PAGE_SIZE };
  if (cursor !== null) {
    query.cursor = cursor;
  }
  return readReviewed(apiClient.GET("/api/glossary", { params: { query } }));
}

interface PendingMerge {
  readonly preview: SemanticChangePreview;
  readonly reason: string;
}

function Failure({ error }: { error: Error | null }) {
  if (error === null || !(error instanceof WriteFailedError)) {
    return null;
  }
  return (
    <p role="alert" className="text-sm text-red-800">
      {t("write.failed")} <span className="font-mono">{errorCode(error) ?? String(error.status)}</span> <span>{writeErrorMessage(error)}</span>
    </p>
  );
}

/**
 * The glossary (v2 U3): the server term list with cursor pagination, and Term merge as an explicit
 * source/target/reason choice, a server preview (merge-class proposal, intake and semantic diff)
 * and an explicit confirmation through POST /api/proposals/{id}/merge with the same reason.
 */
export function GlossaryPage() {
  const queryClient = useQueryClient();
  const pages = useCursorPages();
  const cursor = pages.cursor;
  const page = useQuery({ queryKey: glossaryQueryKey(cursor), queryFn: () => fetchGlossary(cursor) });
  useCursorRecovery(pages, page.error);
  const [source, setSource] = useState("");
  const [target, setTarget] = useState("");
  const [reason, setReason] = useState("");
  const [pending, setPending] = useState<PendingMerge | null>(null);
  const [stale, setStale] = useState(false);

  const preview = useVersionedMutation<SemanticChangePreview, { source: string; into: string; reason: string }>(
    (choice, ifMatch) =>
      apiClient.POST("/api/glossary/{id}/merge/preview", {
        params: { path: { id: choice.source }, header: { "If-Match": ifMatch } },
        body: { into: choice.into, reason: choice.reason },
      }),
    page.data?.etag,
  );
  // The confirmation is based on the reviewed preview, which states its baseline revision.
  const confirm = useVersionedMutation<WriteResult, PendingMerge>(
    (merge, ifMatch) =>
      apiClient.POST("/api/proposals/{id}/merge", {
        params: { path: { id: merge.preview.proposal_id }, header: { "If-Match": ifMatch } },
        body: { reason: merge.reason },
      }),
    (merge) => revisionEtag(merge.preview.revision),
  );

  const locked = pending !== null;

  const requestPreview = () => {
    setStale(false);
    confirm.reset();
    const choice = { source, into: target, reason };
    preview.mutate(choice, { onSuccess: (data) => setPending({ preview: data, reason: choice.reason }) });
  };

  const confirmPreview = () => {
    if (pending === null) {
      return;
    }
    confirm.mutate(pending, {
      onSuccess: () => {
        setPending(null);
        setSource("");
        setTarget("");
        setReason("");
        void queryClient.invalidateQueries();
      },
      onError: (error) => {
        if (isBaselineChanged(error)) {
          setPending(null);
          setStale(true);
          void queryClient.invalidateQueries();
        }
      },
    });
  };

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("glossary.title")}</h1>
      {pages.recovered && (
        <p role="status" className="text-sm" data-testid="cursor-reset">
          {t("pagination.reset")}
        </p>
      )}
      {page.isError && !isCursorBaselineChanged(page.error) && <ReadFailure error={page.error} messageKey="glossary.loadFailed" />}
      {page.data === undefined && !page.isError && <p>{t("glossary.loading")}</p>}
      {page.data !== undefined && (
        <>
          {page.data.items.length === 0 ? (
            <p>{t("glossary.empty")}</p>
          ) : (
            <table className="w-full text-left text-sm">
              <thead>
                <tr>
                  <th>{t("glossary.column.term")}</th>
                  <th>{t("glossary.column.id")}</th>
                  <th>{t("glossary.column.language")}</th>
                  <th>{t("glossary.column.status")}</th>
                  <th>{t("glossary.column.aliases")}</th>
                  <th>{t("glossary.column.merge")}</th>
                </tr>
              </thead>
              <tbody>
                {page.data.items.map((term) => (
                  <tr key={term.id} data-term-id={term.id} data-role={term.id === source ? "source" : term.id === target ? "target" : "none"}>
                    <td>{term.term}</td>
                    <td className="font-mono">{term.id}</td>
                    <td className="font-mono">{term.language}</td>
                    <td className="font-mono">{term.status}</td>
                    <td>{term.aliases.join(", ")}</td>
                    <td className="space-x-1">
                      <button type="button" className="rounded border px-2 text-xs" disabled={locked} data-select="source" onClick={() => setSource(term.id)}>
                        {t("glossary.merge.asSource")}
                      </button>
                      <button type="button" className="rounded border px-2 text-xs" disabled={locked} data-select="target" onClick={() => setTarget(term.id)}>
                        {t("glossary.merge.asTarget")}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <div className="flex gap-2">
            <button type="button" className="rounded border px-3 py-1" disabled={pages.isFirst} onClick={pages.previous}>
              {t("pagination.previous")}
            </button>
            <button
              type="button"
              className="rounded border px-3 py-1"
              disabled={(page.data.next_cursor ?? null) === null}
              onClick={() => {
                const next = page.data.next_cursor ?? null;
                if (next !== null) {
                  pages.next(next);
                }
              }}
            >
              {t("pagination.next")}
            </button>
          </div>
        </>
      )}

      <section aria-label={t("glossary.merge.title")} className="space-y-2 rounded border p-3">
        <h2 className="font-semibold">{t("glossary.merge.title")}</h2>
        <label className="block text-sm">
          {t("glossary.merge.source")}
          <input className="ml-2 rounded border p-1 font-mono" value={source} disabled={locked} data-testid="merge-source" onChange={(event) => setSource(event.target.value)} />
        </label>
        <label className="block text-sm">
          {t("glossary.merge.target")}
          <input className="ml-2 rounded border p-1 font-mono" value={target} disabled={locked} data-testid="merge-target" onChange={(event) => setTarget(event.target.value)} />
        </label>
        <label className="block text-sm">
          {t("requirement.reason")}
          <input className="ml-2 w-full rounded border p-1" value={reason} disabled={locked} data-testid="merge-reason" onChange={(event) => setReason(event.target.value)} />
        </label>
        {pending === null && (
          <button
            type="button"
            className="rounded border px-3 py-1"
            disabled={preview.isPending || source === "" || target === "" || reason === ""}
            onClick={requestPreview}
            data-testid="merge-preview"
          >
            {t("preview.request")}
          </button>
        )}
        {stale && (
          <p role="alert" className="text-sm text-red-800" data-testid="preview-stale">
            {t("preview.stale")}
          </p>
        )}
        <Failure error={preview.error} />
        {!stale && <Failure error={confirm.error} />}
        {pending !== null && (
          <SemanticChangePreviewPanel
            preview={pending.preview}
            reason={pending.reason}
            confirming={confirm.isPending}
            onConfirm={confirmPreview}
            onDiscard={() => setPending(null)}
          />
        )}
      </section>
    </div>
  );
}
