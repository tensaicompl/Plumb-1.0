import type { ReactNode } from "react";

import { t } from "../../i18n";
import { cn } from "./cn";

export interface WithInspectorProps {
  /** The main content (list, board, canvas); it scrolls on its own. */
  readonly children: ReactNode;
  /** The selected item's detail, or null for the empty state. */
  readonly inspector: ReactNode | null;
  /** Replaces "Select an item". */
  readonly empty?: ReactNode;
  /** The sticky action bar (38 px pill buttons), shown with an inspector. */
  readonly actions?: ReactNode;
  /** default clamp(320px, 32vw, 420px); wide (Scenarios) up to 440 px. */
  readonly width?: "default" | "wide";
  /** The inspector's accessible name (default "Details"). */
  readonly label?: string;
  /** Changing it replays the 200 ms slide-in (e.g. the selected item's ID). */
  readonly inspectorKey?: string;
  readonly className?: string;
}

/**
 * The list + inspector layout: main content and a right panel (white, left border) whose
 * content has padding 24 and 20 px gaps, with a sticky action bar (top border, 14×20 padding).
 */
export function WithInspector({ children, inspector, empty, actions, width = "default", label, inspectorKey, className }: WithInspectorProps) {
  return (
    <div className={cn("flex min-h-0 min-w-0 flex-1", className)}>
      <div className="min-h-0 min-w-0 flex-1 overflow-y-auto">{children}</div>
      <aside
        aria-label={label ?? t("ui.inspector.label")}
        style={{ width: width === "wide" ? "clamp(320px, 32vw, 440px)" : "clamp(320px, 32vw, 420px)" }}
        className="flex min-h-0 shrink-0 flex-col border-l border-line bg-inspector"
      >
        {inspector === null ? (
          <div className="flex flex-1 items-center justify-center p-6 text-body text-fg-3">{empty ?? t("ui.inspector.empty")}</div>
        ) : (
          <>
            <div key={inspectorKey} className="flex min-h-0 flex-1 animate-inspector-in flex-col gap-5 overflow-y-auto p-6">
              {inspector}
            </div>
            {actions !== undefined && (
              <div className="sticky bottom-0 flex shrink-0 flex-wrap items-center gap-2 border-t border-line bg-inspector px-5 py-3.5">{actions}</div>
            )}
          </>
        )}
      </aside>
    </div>
  );
}
