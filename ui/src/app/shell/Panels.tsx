import { useMatch } from "@tanstack/react-router";
import { RefreshCw, X } from "lucide-react";

import { useConnectionStore } from "../../api/useEvents";
import { IconButton } from "../../components/ui";
import { SessionPanel } from "../../features/sessions/SessionPanel";
import { t, tf } from "../../i18n";
import { useShellStore } from "../preferences";

/** The banner below the toolbar while the live event stream is down (34 px, amber). */
export function ConnectionBanner() {
  const status = useConnectionStore((state) => state.status);
  if (status !== "lost") {
    return null;
  }
  return (
    <div role="status" className="flex h-[34px] shrink-0 items-center justify-center gap-2.5 bg-banner text-footnote font-medium text-on-banner">
      <RefreshCw size={16} strokeWidth={1.6} aria-hidden="true" />
      {t("shell.connection.lost")}
    </div>
  );
}

/**
 * The Assistant (right panel, 400 px, from any screen): the session view, scoped to the open
 * requirement on a requirement detail route (Hotfix 077) and to the project elsewhere.
 */
export function AssistantPanel() {
  const setOpen = useShellStore((state) => state.setAssistantOpen);
  const requirementId = useMatch({ from: "/requirements/$id", shouldThrow: false, select: (match) => match.params.id });
  return (
    <aside aria-label={t("shell.assistant.title")} data-testid="assistant-panel" className="flex w-[400px] shrink-0 flex-col border-l border-line bg-assistant">
      <div className="flex h-[52px] shrink-0 items-center gap-2.5 border-b border-line px-4">
        <span className="text-body font-semibold text-fg">{t("shell.assistant.title")}</span>
        <span className="truncate rounded-pill border border-line bg-card px-2 py-0.5 text-eyebrow font-medium text-fg-2">
          {requirementId === undefined ? t("shell.assistant.scopeProject") : tf("shell.assistant.scope", { scope: requirementId })}
        </span>
        <IconButton
          className="ml-auto"
          size={30}
          label={t("shell.assistant.close")}
          icon={<X size={16} strokeWidth={1.6} aria-hidden="true" />}
          onClick={() => setOpen(false)}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {requirementId === undefined ? (
          <SessionPanel key="project" mode="project" />
        ) : (
          <SessionPanel key={requirementId} mode="requirement" requirementId={requirementId} />
        )}
      </div>
    </aside>
  );
}
