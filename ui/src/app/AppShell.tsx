import { Outlet } from "@tanstack/react-router";
import { useCallback, useEffect } from "react";

import { useEvents } from "../api/useEvents";
import { ConflictDialog } from "../components/ConflictDialog";
import { Toaster } from "../components/ui";
import { t, useLocaleStore } from "../i18n";
import { applyAppearance, useAppearanceStore, useShellStore } from "./preferences";
import { useJobPolling } from "./run";
import { CommandPalette } from "./shell/CommandPalette";
import { ExportSheet } from "./shell/ExportSheet";
import { useGo } from "./shell/navigation";
import { AssistantPanel, ConnectionBanner } from "./shell/Panels";
import { SettingsSheet } from "./shell/SettingsSheet";
import { Sidebar } from "./shell/Sidebar";
import { Toolbar } from "./shell/Toolbar";
import { useGlobalKeys } from "./shell/useGlobalKeys";

/**
 * The application shell (design handoff "Global shell"): sidebar, toolbar, connection banner, the
 * routed screen, the Assistant panel, and the global overlays (command palette, settings, export,
 * the write-conflict sheet and toasts). It owns the DesignEvent subscription, the stage-job poll
 * and the global keys; it owns no model data.
 */
export function AppShell() {
  useEvents();
  useGlobalKeys();
  const go = useGo();
  useJobPolling({ openInbox: useCallback(() => go("/inbox"), [go]) });
  const locale = useLocaleStore((state) => state.locale);
  const appearance = useAppearanceStore((state) => state.appearance);
  const assistantOpen = useShellStore((state) => state.assistantOpen);
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);
  useEffect(() => {
    applyAppearance(appearance);
  }, [appearance]);
  // A locale change re-renders every view in the new locale (view state below the shell resets).
  return (
    <div key={locale} className="flex h-screen overflow-hidden bg-page font-sans text-body text-fg">
      <a
        href="#main"
        className="sr-only z-[90] rounded-control bg-card px-3 py-2 text-footnote font-semibold text-fg shadow-md focus:not-sr-only focus:fixed focus:left-3 focus:top-3"
      >
        {t("shell.skip")}
      </a>
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <Toolbar />
        <ConnectionBanner />
        <div className="flex min-h-0 flex-1">
          <main id="main" tabIndex={-1} className="flex min-h-0 min-w-0 flex-1 flex-col outline-none">
            <Outlet />
          </main>
          {assistantOpen && <AssistantPanel />}
        </div>
      </div>
      <CommandPalette />
      <SettingsSheet />
      <ExportSheet />
      <ConflictDialog />
      <Toaster />
    </div>
  );
}
