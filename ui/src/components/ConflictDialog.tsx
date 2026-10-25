import { useQueryClient } from "@tanstack/react-query";
import { useRef } from "react";

import { useConflictStore } from "../api/useVersionedMutation";
import { t, tf } from "../i18n";
import { Button, confirmShortcut, Sheet } from "./ui";

/** "v8" from a revision ID "rev:8:…" (the revision itself when it has another form). */
function versionOf(revision: string): string {
  const match = /^rev:(\d+):/.exec(revision);
  return match === null ? revision : tf("shell.version.number", { n: match[1] ?? "" });
}

/**
 * The single global 412 sheet, "This changed since you looked" (design handoff: write-conflict
 * sheet). It shows while a rejected write's conflict is recorded and closes only on an explicit
 * choice: Review again (invalidate server queries so every view shows the latest version) or
 * Dismiss (button or Escape). A click outside does not close it. Neither choice re-invokes the
 * rejected write; there is no retry and no merge. The server's revision values are in a Details
 * disclosure.
 */
export function ConflictDialog() {
  const queryClient = useQueryClient();
  const conflict = useConflictStore((state) => state.conflict);
  const clearConflict = useConflictStore((state) => state.clearConflict);
  const reviewRef = useRef<HTMLButtonElement>(null);

  const reviewAgain = () => {
    clearConflict();
    void queryClient.invalidateQueries();
  };
  const reported = conflict !== null && (conflict.revision !== null || conflict.semanticHash !== null);

  return (
    <Sheet
      open={conflict !== null}
      onOpenChange={(open) => {
        if (!open) {
          clearConflict();
        }
      }}
      data-testid="conflict-dialog"
      title={t("conflict.sheet.title")}
      description={t("conflict.sheet.description")}
      scrimDismiss={false}
      initialFocus={reviewRef}
      onConfirm={reviewAgain}
      footer={
        <div className="mt-1 flex justify-end gap-2">
          <Button variant="outline" size={38} onClick={clearConflict}>
            {t("conflict.dismiss")}
          </Button>
          <Button ref={reviewRef} variant="primary" size={38} className="px-[18px]" onClick={reviewAgain} shortcut={confirmShortcut()}>
            {t("conflict.reviewAgain")}
          </Button>
        </div>
      }
    >
      {conflict?.revision !== null && conflict?.revision !== undefined && (
        <p className="m-0 text-callout font-medium text-fg">{tf("conflict.nowAt", { version: versionOf(conflict.revision) })}</p>
      )}
      <details className="rounded-field bg-surface px-3.5 py-2.5 text-caption text-fg-2">
        <summary className="cursor-pointer font-semibold text-fg-3-surface">{t("conflict.details")}</summary>
        <p className="mb-0 mt-2" data-testid="conflict-status">
          {t("conflict.status")}
        </p>
        {reported && (
          <dl className="m-0 mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
            {conflict.revision !== null && (
              <>
                <dt>{t("conflict.revision")}</dt>
                <dd className="m-0 select-text font-mono" data-testid="conflict-revision">
                  {conflict.revision}
                </dd>
              </>
            )}
            {conflict.semanticHash !== null && (
              <>
                <dt>{t("conflict.semanticHash")}</dt>
                <dd className="m-0 select-text break-all font-mono" data-testid="conflict-semantic-hash">
                  {conflict.semanticHash}
                </dd>
              </>
            )}
          </dl>
        )}
        {conflict !== null && !reported && (
          <p className="mb-0 mt-2" data-testid="conflict-unreported">
            {t("conflict.unreported")}
          </p>
        )}
      </details>
    </Sheet>
  );
}
