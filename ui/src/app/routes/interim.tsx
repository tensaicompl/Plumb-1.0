// Route bodies that keep the app usable inside the new shell until each screen lane replaces them:
// the old pages, wrapped in a scrolling padded column, with segmented navigation where one
// destination groups several old pages (Model, Quality). See src/README-SCREENS.md.
import { useQuery } from "@tanstack/react-query";
import { Outlet, useRouterState } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { apiClient, readData } from "../../api/client";
import { Button, EmptyState, MonoId, SegmentedLinks, Skeleton, SkeletonGroup } from "../../components/ui";
import { ReadFailure } from "../../components/ReadFailure";
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

// ------------------------------------------------------------------ Sources

/** /sources: no old page exists and the contract has no source list; sources open from evidence. */
export function SourcesInterim() {
  const go = useGo();
  return (
    <Legacy>
      <h1 className="m-0 font-display text-h2 font-bold tracking-display text-fg">{t("sources.title")}</h1>
      <EmptyState
        title={t("sources.empty.title")}
        description={t("sources.empty.body")}
        action={
          <Button variant="outline" size={38} onClick={() => go("/requirements")}>
            {t("sources.empty.action")}
          </Button>
        }
      />
    </Legacy>
  );
}

/** /sources/$id: the source record (`get_source`). */
export function SourceInterim({ id }: { id: string }) {
  const go = useGo();
  const source = useQuery({
    queryKey: ["source", id],
    queryFn: () => readData(apiClient.GET("/api/sources/{id}", { params: { path: { id } } })),
  });
  return (
    <Legacy>
      <h1 className="m-0 mb-6 font-display text-h2 font-bold tracking-display text-fg">{t("sources.detail.title")}</h1>
      {source.isError && <ReadFailure error={source.error} messageKey="sources.loadFailed" />}
      {source.isPending && (
        <SkeletonGroup className="flex flex-col gap-3">
          <Skeleton width="40%" />
          <Skeleton width="60%" />
        </SkeletonGroup>
      )}
      {source.data !== undefined && (
        <dl className="m-0 grid max-w-[720px] grid-cols-[160px_1fr] gap-x-4 gap-y-3 text-body">
          <dt className="text-fg-3">{t("sources.field.id")}</dt>
          <dd className="m-0">
            <MonoId tone="default">{source.data.id}</MonoId>
          </dd>
          <dt className="text-fg-3">{t("sources.field.mediaType")}</dt>
          <dd className="m-0 font-mono text-caption">{source.data.media_type}</dd>
          <dt className="text-fg-3">{t("sources.field.kind")}</dt>
          <dd className="m-0">{source.data.source_kind}</dd>
          <dt className="text-fg-3">{t("sources.field.hash")}</dt>
          <dd className="m-0 break-all font-mono text-caption">{source.data.hash}</dd>
        </dl>
      )}
      <Button variant="ghost" size={34} className="mt-6" onClick={() => go("/sources")}>
        {t("sources.back")}
      </Button>
    </Legacy>
  );
}
