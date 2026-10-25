import { useMatch } from "@tanstack/react-router";
import { RefreshCw, X } from "lucide-react";

import { useConnectionStore } from "../../api/useEvents";
import { IconButton } from "../../components/ui";
import { AssistantConversation } from "../../features/assistant/AssistantConversation";
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
 * The Assistant (right panel, 400 px, from any screen; handoff "8. Assistant panel"): a 52 px
 * header with the scope chip and close, then the conversation, scoped to the open requirement on
 * a requirement detail route (Hotfix 077) and to the project elsewhere.
 */
export function AssistantPanel() {
  const setOpen = useShellStore((state) => state.setAssistantOpen);
  const requirementId = useMatch({ from: "/requirements/$id", shouldThrow: false, select: (match) => match.params.id });
  return (
    <aside aria-label={t("shell.assistant.title")} data-testid="assistant-panel" className="flex min-h-0 w-[400px] shrink-0 flex-col border-l border-line bg-assistant">
      <div className="flex h-[52px] shrink-0 items-center gap-2.5 border-b border-line bg-card px-4">
        <span className="text-body font-semibold text-fg">{t("shell.assistant.title")}</span>
        <span className="h-[22px] truncate rounded-pill border border-line px-2 text-eyebrow font-medium leading-5 text-fg-2">
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
      <AssistantConversation key={requirementId ?? "project"} requirementId={requirementId ?? null} />
    </aside>
  );
}
