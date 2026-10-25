import { Outlet, useRouterState } from "@tanstack/react-router";
import { Download } from "lucide-react";

import { useShellStore } from "../../app/preferences";
import { useGo } from "../../app/shell/navigation";
import { Button, SegmentedLinks } from "../../components/ui";
import { t } from "../../i18n";

/**
 * Quality (handoff "6. Quality", spec §5.6): a 1040 px column with the title, the Gates · Issues ·
 * History tabs (each a URL) and the Export pill, above the active tab.
 */
export function QualityScreen() {
  const go = useGo();
  const openOverlay = useShellStore((state) => state.openOverlay);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const tab = pathname.startsWith("/quality/history") ? "history" : pathname.startsWith("/quality/issues") ? "issues" : "gates";
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto max-w-[1040px] px-7 pb-16 pt-6">
        <div className="mb-5 flex items-center gap-4">
          <h1 className="m-0 font-display text-h2 font-bold tracking-display text-fg">{t("quality.title")}</h1>
          <SegmentedLinks
            label={t("quality.views")}
            onNavigate={go}
            items={[
              { href: "/quality", label: t("quality.view.gates"), active: tab === "gates" },
              { href: "/quality/issues", label: t("quality.view.issues"), active: tab === "issues" },
              { href: "/quality/history", label: t("quality.view.history"), active: tab === "history" },
            ]}
          />
          <span className="flex-1" />
          <Button variant="outline" size={34} className="px-3.5" icon={<Download size={16} strokeWidth={1.6} aria-hidden="true" />} onClick={() => openOverlay("export")}>
            {t("quality.export")}
          </Button>
        </div>
        <Outlet />
      </div>
    </div>
  );
}
