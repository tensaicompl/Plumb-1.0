import { RotateCw } from "lucide-react";
import type { ReactNode } from "react";

import { WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { hasRole, useMe } from "../../app/auth";
import { ReadFailure } from "../../components/ReadFailure";
import { Button, cn } from "../../components/ui";
import { t, type MessageKey } from "../../i18n";

/** The role whose holders change the model (types, units, outcomes, steps, confirmations). */
export const EDITOR_ROLE = "analyst";

/** Whether the current identity may change the model; false while the identity loads. */
export function useCanEdit(): boolean {
  return hasRole(useMe().data, EDITOR_ROLE);
}

/** The typed code of a failed write, when the server sent one. */
export function writeCode(error: unknown): string | null {
  if (error instanceof WriteFailedError && typeof error.body === "object" && error.body !== null) {
    const code = (error.body as Record<string, unknown>)["code"];
    return typeof code === "string" ? code : null;
  }
  return null;
}

/** A failed write in plain language (the server message), with the code as a detail. */
export function WriteError({ error, className }: { error: Error | null; className?: string }) {
  if (error === null || !(error instanceof WriteFailedError) || error.status === 412) {
    return null;
  }
  const code = writeCode(error);
  return (
    <p role="alert" className={cn("m-0 text-footnote text-blocked-fg", className)}>
      {t("write.failed")} {writeErrorMessage(error)}
      {code !== null && <span className="ml-1 font-mono text-eyebrow opacity-80">{code}</span>}
    </p>
  );
}

/** A failed read with Retry (reads already retry 502–504 twice). */
export function ReadError({ error, messageKey, onRetry }: { error: unknown; messageKey: MessageKey; onRetry: () => void }) {
  return (
    <div className="flex flex-col items-start gap-3 rounded-tile border border-line bg-card p-5 text-callout text-fg-2">
      <ReadFailure error={error} messageKey={messageKey} />
      <Button variant="outline" size={30} icon={<RotateCw size={14} strokeWidth={1.6} aria-hidden="true" />} onClick={onRetry}>
        {t("model.retry")}
      </Button>
    </div>
  );
}

/** The padded content column of a Model segment. */
export function SegmentBody({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("px-7 pb-12 pt-5", className)}>{children}</div>;
}

/** A small section label inside a card ("States", "In plain language"). */
export function SectionLabel({ children, id, className }: { children: ReactNode; id?: string; className?: string }) {
  return (
    <h3 id={id} className={cn("m-0 text-caption font-semibold text-fg-2", className)}>
      {children}
    </h3>
  );
}
