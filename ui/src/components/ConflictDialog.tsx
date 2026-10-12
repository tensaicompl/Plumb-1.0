import * as Dialog from "@radix-ui/react-dialog";
import { useQueryClient } from "@tanstack/react-query";

import { useConflictStore } from "../api/useVersionedMutation";
import { t } from "../i18n";

/**
 * The single global 412 dialog: shown while a rejected write's conflict is recorded, it requires
 * an explicit choice and never re-invokes the rejected write.
 */
export function ConflictDialog() {
  const queryClient = useQueryClient();
  const conflict = useConflictStore((state) => state.conflict);
  const clearConflict = useConflictStore((state) => state.clearConflict);

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
        <Dialog.Content className="fixed left-1/2 top-1/2 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded bg-white p-6 shadow-lg">
          <Dialog.Title className="text-lg font-semibold">{t("conflict.title")}</Dialog.Title>
          <Dialog.Description className="mt-2 text-sm">{t("conflict.description")}</Dialog.Description>
          {conflict !== null && (
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
          <div className="mt-6 flex justify-end gap-2">
            <Dialog.Close asChild>
              <button type="button" className="rounded border px-3 py-1">
                {t("conflict.dismiss")}
              </button>
            </Dialog.Close>
            <button type="button" className="rounded bg-blue-700 px-3 py-1 text-white" onClick={refresh}>
              {t("conflict.refresh")}
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
