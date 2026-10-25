// Route bodies that keep the app usable inside the new shell until each screen lane replaces them:
// the old pages, wrapped in a scrolling padded column, with segmented navigation where one
// destination groups several old pages (Model, Quality). See src/README-SCREENS.md.
import { Outlet, useRouterState } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { SegmentedLinks } from "../../components/ui";
import { t, type MessageKey } from "../../i18n";
import { useGo } from "../shell/navigation";

/** An old page inside the new shell: a scrolling column with the page padding. */
export function Legacy({ children }: { children: ReactNode }) {
  return <div className="min-h-0 flex-1 overflow-y-auto px-8 pb-16 pt-8">{children}</div>;
}

function Views({ labelKey, items }: { labelKey: MessageKey; items: readonly { href: string; label: string; active: boolean }[] }) {
  const go = useGo();
  return <SegmentedLinks label={t(labelKey)} items={items} onNavigate={go} className="mb-6" />;
}

// ------------------------------------------------------------------ Model and Quality

const MODEL_SEGMENTS: readonly { segment: string; label: MessageKey }[] = [
  { segment: "glossary", label: "nav.glossary" },
  { segment: "entities", label: "nav.entities" },
  { segment: "rules", label: "nav.rules" },
  { segment: "calculations", label: "nav.calculations" },
  { segment: "processes", label: "nav.processes" },
];

/** /model/*: the five Model segments as segmented navigation above the segment's old page. */
export function ModelInterim() {
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  return (
    <Legacy>
      <Views
        labelKey="model.views"
        items={MODEL_SEGMENTS.map(({ segment, label }) => ({
          href: "/model/" + segment,
          label: t(label),
          active: pathname === "/model/" + segment || pathname.startsWith("/model/" + segment + "/"),
        }))}
      />
      <Outlet />
    </Legacy>
  );
}

/** /quality/*: Gates · Issues · History above the old pages. */
export function QualityInterim() {
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const tab = pathname.startsWith("/quality/history") ? "history" : pathname.startsWith("/quality/issues") ? "issues" : "gates";
  return (
    <Legacy>
      <Views
        labelKey="quality.views"
        items={[
          { href: "/quality", label: t("quality.view.gates"), active: tab === "gates" },
          { href: "/quality/issues", label: t("quality.view.issues"), active: tab === "issues" },
          { href: "/quality/history", label: t("quality.view.history"), active: tab === "history" },
        ]}
      />
      <Outlet />
    </Legacy>
  );
}
