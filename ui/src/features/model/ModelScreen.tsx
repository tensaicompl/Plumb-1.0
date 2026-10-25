import { Outlet, useRouterState } from "@tanstack/react-router";
import { Diff, LayoutGrid } from "lucide-react";
import { useState } from "react";

import { buttonClasses, SegmentedLinks } from "../../components/ui";
import { t, type MessageKey } from "../../i18n";
import { useGo } from "../../app/shell/navigation";
import { ChangesSheet, useModelChanges } from "./ChangesSheet";
import { CoverageSheet } from "./CoverageSheet";

export type ModelSegment = "glossary" | "entities" | "rules" | "calculations" | "processes";

const SEGMENTS: readonly { segment: ModelSegment; label: MessageKey }[] = [
  { segment: "glossary", label: "nav.glossary" },
  { segment: "entities", label: "nav.entities" },
  { segment: "rules", label: "nav.rules" },
  { segment: "calculations", label: "nav.calculations" },
  { segment: "processes", label: "nav.processes" },
];

const PILL = buttonClasses({ variant: "outline", size: 30, className: "h-8 gap-1.5 px-3" });

/**
 * /model/*: the "Model" header with the five segments as links, the Coverage (CRUD matrix) and
 * "Changes +N" (model diff since the last visit) pills, and the routed segment below.
 */
export function ModelScreen() {
  const go = useGo();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const [sheet, setSheet] = useState<"coverage" | "changes" | null>(null);
  const changes = useModelChanges();
  const added = changes.diff?.added.length;
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-4 px-7 pt-6">
        <h1 className="m-0 font-display text-h2 font-bold tracking-display text-fg">{t("nav.model")}</h1>
        <SegmentedLinks
          label={t("model.views")}
          onNavigate={go}
          items={SEGMENTS.map(({ segment, label }) => ({
            href: "/model/" + segment,
            label: t(label),
            active: pathname === "/model/" + segment || pathname.startsWith("/model/" + segment + "/"),
          }))}
        />
        <div className="flex-1" />
        <button type="button" className={PILL} onClick={() => setSheet("coverage")}>
          <LayoutGrid size={14} strokeWidth={1.6} aria-hidden="true" />
          {t("model.coverage.open")}
        </button>
        <button type="button" className={PILL} onClick={() => setSheet("changes")}>
          <Diff size={14} strokeWidth={1.6} aria-hidden="true" />
          {t("model.changes.open")}
          {added !== undefined && added > 0 && <span className="font-mono text-eyebrow font-medium text-fg-3">{"+" + String(added)}</span>}
        </button>
      </div>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        <Outlet />
      </div>
      <CoverageSheet open={sheet === "coverage"} onOpenChange={(open) => setSheet(open ? "coverage" : null)} />
      <ChangesSheet
        open={sheet === "changes"}
        changes={changes}
        onOpenChange={(open) => {
          setSheet(open ? "changes" : null);
          if (!open) {
            changes.markSeen();
          }
        }}
      />
    </div>
  );
}
