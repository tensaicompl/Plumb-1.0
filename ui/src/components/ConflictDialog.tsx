import * as Dialog from "@radix-ui/react-dialog";
import { useQueryClient } from "@tanstack/react-query";
import { useRef } from "react";

import { useConflictStore } from "../api/useVersionedMutation";
import { t } from "../i18n";

/**
 * The single global 412 dialog (created in U1.1, refined in U1.2). It is shown while a rejected
 * write's conflict is recorded and closes only on an explicit choice: Refresh latest (invalidate
 * server queries) or Dismiss (button or Escape). A click outside does not close it. Neither choice
 * re-invokes the rejected write; there is no retry and no merge.
 */
export function ConflictDialog() {
  const queryClient = useQueryClient();
  const conflict = useConflictStore((state) => state.conflict);
  const clearConflict = useConflictStore((state) => state.clearConflict);
  const refreshRef = useRef<HTMLButtonElement>(null);

  const refresh = () => {
    clearConflict();
    void queryClient.invalidateQueries();
  };

  return (
    <Dialog.Root
      open={conflict !== null}
      onOpenChange={(open) => {
        if (!open) {
          clearConflict();
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content
          data-testid="conflict-dialog"
          className="fixed left-1/2 top-1/2 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded bg-white p-6 shadow-lg"
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            refreshRef.current?.focus();
          }}
          onPointerDownOutside={(event) => event.preventDefault()}
          onInteractOutside={(event) => event.preventDefault()}
        >
          <Dialog.Title className="text-lg font-semibold">{t("conflict.title")}</Dialog.Title>
          <Dialog.Description className="mt-2 text-sm">{t("conflict.description")}</Dialog.Description>
          <p className="mt-2 text-sm" data-testid="conflict-status">
            {t("conflict.status")}
          </p>
          {conflict !== null && (conflict.revision !== null || conflict.semanticHash !== null) && (
            <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              {conflict.revision !== null && (
                <>
                  <dt>{t("conflict.revision")}</dt>
                  <dd className="font-mono" data-testid="conflict-revision">
                    {conflict.revision}
                  </dd>
                </>
              )}
              {conflict.semanticHash !== null && (
                <>
                  <dt>{t("conflict.semanticHash")}</dt>
                  <dd className="break-all font-mono" data-testid="conflict-semantic-hash">
                    {conflict.semanticHash}
                  </dd>
                </>
              )}
            </dl>
          )}
          {conflict !== null && conflict.revision === null && conflict.semanticHash === null && (
            <p className="mt-4 text-sm" data-testid="conflict-unreported">
              {t("conflict.unreported")}
            </p>
          )}
          <div className="mt-6 flex justify-end gap-2">
            <Dialog.Close asChild>
              <button type="button" className="rounded border px-3 py-1">
                {t("conflict.dismiss")}
              </button>
            </Dialog.Close>
            <button ref={refreshRef} type="button" className="rounded bg-blue-700 px-3 py-1 text-white" onClick={refresh}>
              {t("conflict.refresh")}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
