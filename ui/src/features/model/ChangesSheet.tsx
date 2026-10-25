import { useQuery } from "@tanstack/react-query";

import { apiClient, ReadFailedError, readData } from "../../api/client";
import type { components } from "../../api/schema";
import { Sheet, SkeletonList } from "../../components/ui";
import { t, tf, type MessageKey } from "../../i18n";
import { useLabels } from "./labels";
import { ReadError } from "./shared";

type ModelDiff = components["schemas"]["ModelDiff"];

/** Where the revision the user last reviewed in the Changes sheet is kept (this browser only). */
export const LAST_SEEN_KEY = "plumb.model.lastSeenRevision";

/** The most changed elements listed (the counts always cover every change). */
const MAX_ROWS = 60;

function lastSeen(): string | null {
  try {
    return globalThis.localStorage?.getItem(LAST_SEEN_KEY) ?? null;
  } catch {
    return null;
  }
}

/** "39" from "rev:39:…" (the revision itself otherwise). */
export function versionNumber(revision: string): string {
  return /^rev:(\d+):/.exec(revision)?.[1] ?? revision;
}

type ChangesResult = { readonly kind: "diff"; readonly diff: ModelDiff } | { readonly kind: "first" };

async function fetchChanges(from: string | null): Promise<ChangesResult> {
  const read = async (query: { from?: string }): Promise<ChangesResult> => {
    try {
      return { kind: "diff", diff: await readData(apiClient.GET("/api/model/diff", { params: { query } })) };
    } catch (error) {
      if (error instanceof ReadFailedError && error.code === "E_DIFF_NO_PARENT") {
        return { kind: "first" };
      }
      throw error;
    }
  };
  if (from === null) {
    return read({});
  }
  try {
    return await read({ from });
  } catch (error) {
    // The remembered revision is gone (another database): compare with the previous version.
    if (error instanceof ReadFailedError && (error.status === 404 || error.status === 400)) {
      return read({});
    }
    throw error;
  }
}

export interface ModelChanges {
  readonly diff: ModelDiff | undefined;
  readonly first: boolean;
  readonly query: ReturnType<typeof useQuery<ChangesResult>>;
  /** Remembers the compared head as seen, so the next comparison starts there. */
  markSeen(): void;
}

/** The model diff since the user's last visit (get_model_diff), or since the previous version. */
export function useModelChanges(): ModelChanges {
  const from = lastSeen();
  const query = useQuery<ChangesResult>({ queryKey: ["model", "diff", from], queryFn: () => fetchChanges(from) });
  const diff = query.data?.kind === "diff" ? query.data.diff : undefined;
  return {
    diff,
    first: query.data?.kind === "first",
    query,
    markSeen: () => {
      if (diff === undefined) {
        return;
      }
      try {
        globalThis.localStorage?.setItem(LAST_SEEN_KEY, diff.to);
      } catch {
        // Not remembered when storage is unavailable; the next comparison uses the previous version.
      }
    },
  };
}

const CHANGE: Readonly<Record<"added" | "modified" | "removed", MessageKey>> = {
  added: "model.changes.added",
  modified: "model.changes.modified",
  removed: "model.changes.removed",
};

/** "Changes since your last visit": added, modified and removed elements by name. */
export function ChangesSheet({ open, onOpenChange, changes }: { open: boolean; onOpenChange: (open: boolean) => void; changes: ModelChanges }) {
  const { diff, first, query } = changes;
  const rows = diff === undefined ? [] : (["added", "modified", "removed"] as const).flatMap((change) => diff[change].map((id) => ({ change, id })));
  const shown = rows.slice(0, MAX_ROWS);
  const label = useLabels(open ? shown.map((row) => row.id) : []);
  const description =
    diff === undefined
      ? undefined
      : tf("model.changes.summary", {
          from: versionNumber(diff.from),
          to: versionNumber(diff.to),
          added: diff.added.length,
          modified: diff.modified.length,
          removed: diff.removed.length,
        });
  return (
    <Sheet open={open} onOpenChange={onOpenChange} title={t("model.changes.title")} description={description} cancelLabel={t("model.close")}>
      {query.isError && <ReadError error={query.error} messageKey="model.changes.loadFailed" onRetry={() => void query.refetch()} />}
      {first && <p className="m-0 text-callout text-fg-3">{t("model.changes.first")}</p>}
      {(query.isPending || (diff !== undefined && !label.ready)) && <SkeletonList rows={3} />}
      {diff !== undefined && label.ready && rows.length === 0 && <p className="m-0 text-callout text-fg-3">{t("model.changes.none")}</p>}
      {diff !== undefined && label.ready && rows.length > 0 && (
        <ul aria-label={t("model.changes.list")} className="m-0 flex max-h-[50vh] list-none flex-col overflow-y-auto rounded-tile border border-line p-0">
          {shown.map((row) => (
            <li key={row.change + row.id} className="flex items-center gap-2.5 border-b border-line px-3.5 py-[11px] text-footnote last:border-b-0">
              <span className="w-[72px] shrink-0 font-semibold text-fg">{t(CHANGE[row.change])}</span>
              <span className="min-w-0 flex-1 truncate font-medium text-fg">{label(row.id)}</span>
              {label.kind(row.id) !== null && <span className="shrink-0 font-mono text-caption text-fg-3">{label.kind(row.id)?.replace(/_/g, " ")}</span>}
            </li>
          ))}
          {rows.length > shown.length && <li className="px-3.5 py-[11px] text-footnote text-fg-3">{tf("model.changes.more", { count: rows.length - shown.length })}</li>}
        </ul>
      )}
    </Sheet>
  );
}
