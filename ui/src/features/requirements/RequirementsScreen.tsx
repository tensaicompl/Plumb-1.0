import { useQuery } from "@tanstack/react-query";
import { ChevronDown, FlaskConical, Search, TriangleAlert, Zap } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { gatesQuery, findingsQuery } from "../../app/work";
import { buildHref, useGo } from "../../app/shell/navigation";
import { ReadFailure } from "../../components/ReadFailure";
import {
  Button,
  cn,
  EmptyState,
  FilterChip,
  KINDS,
  KindTag,
  ListRowError,
  Menu,
  MonoId,
  ReadinessRing,
  Skeleton,
  SkeletonGroup,
  WithInspector,
  type MenuItem,
} from "../../components/ui";
import { t, tc, tf, type MessageKey } from "../../i18n";
import {
  compareDocumentOrder,
  issuesOf,
  LOW_READINESS,
  requirementLabel,
  requirementsQuery,
  sourcesQuery,
  waivedFindingIds,
  type RequirementSummary,
} from "./data";
import { RequirementInspector } from "./RequirementInspector";
import { statusLabel } from "./status";

type SortOrder = "document" | "readiness" | "issues";

const SORT_LABEL: Readonly<Record<SortOrder, MessageKey>> = {
  document: "requirements.sort.document",
  readiness: "requirements.sort.readiness",
  issues: "requirements.sort.issues",
};

interface Filters {
  readonly q: string;
  readonly kind: string | null;
  readonly status: string | null;
  readonly hasIssues: boolean;
  readonly lowReadiness: boolean;
  readonly uncovered: boolean;
}

const NO_FILTERS: Filters = { q: "", kind: null, status: null, hasIssues: false, lowReadiness: false, uncovered: false };

/** The glyph shown for a readiness that is not calculated (not copy). */
const NO_VALUE = "–";

/** Whether a key event was typed into a field (screen shortcuts ignore those). */
function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

/** The Requirements search parameters: `affected` (comma-separated requirement IDs). */
export function parseRequirementsSearch(search: Record<string, unknown>): { affected?: string } {
  const value = search["affected"];
  return typeof value === "string" && value !== "" ? { affected: value } : {};
}

export interface RequirementsScreenProps {
  /** The selected requirement (`/requirements/$id`). */
  readonly selectedId?: string;
  /** Requirement IDs a gate rule affects (`?affected=a,b` from Quality → Show affected). */
  readonly affected?: readonly string[];
}

/**
 * Requirements (handoff "3. Requirements", spec §5.3): the whole list with local search, filter
 * chips and sort, item health rows, and the selected requirement's inspector.
 */
export function RequirementsScreen({ selectedId, affected }: RequirementsScreenProps) {
  const go = useGo();
  const list = useQuery(requirementsQuery);
  const findings = useQuery(findingsQuery);
  const gates = useQuery(gatesQuery);
  const sources = useQuery(sourcesQuery);
  const [filters, setFilters] = useState<Filters>(NO_FILTERS);
  const [sort, setSort] = useState<SortOrder>("document");

  const waived = useMemo(() => waivedFindingIds(gates.data), [gates.data]);
  const issueCount = useMemo(() => {
    const counts = new Map<string, number>();
    for (const item of list.data?.items ?? []) {
      counts.set(item.id, issuesOf(item.id, findings.data, waived).length + item.smells.length);
    }
    return counts;
  }, [list.data, findings.data, waived]);

  const items = useMemo(() => list.data?.items ?? [], [list.data]);
  const affectedSet = useMemo(() => (affected === undefined || affected.length === 0 ? null : new Set(affected)), [affected]);
  const visible = useMemo(() => {
    const q = filters.q.trim().toLowerCase();
    const shown = items.filter((item) => {
      if (affectedSet !== null && !affectedSet.has(item.id)) {
        return false;
      }
      if (q !== "" && ![item.text, item.source_identifier ?? "", item.id].some((text) => text.toLowerCase().includes(q))) {
        return false;
      }
      if (filters.kind !== null && item.class !== filters.kind) {
        return false;
      }
      if (filters.status !== null && item.status !== filters.status) {
        return false;
      }
      if (filters.hasIssues && (issueCount.get(item.id) ?? 0) === 0) {
        return false;
      }
      if (filters.lowReadiness && !(item.readiness !== undefined && item.readiness !== null && item.readiness < LOW_READINESS)) {
        return false;
      }
      if (filters.uncovered && item.coverage.operations.length > 0 && item.coverage.scenarios.length > 0) {
        return false;
      }
      return true;
    });
    const readiness = (item: RequirementSummary) => item.readiness ?? Number.POSITIVE_INFINITY;
    return shown.sort((a, b) => {
      if (sort === "readiness") {
        return readiness(a) - readiness(b) || compareDocumentOrder(a, b);
      }
      if (sort === "issues") {
        return (issueCount.get(b.id) ?? 0) - (issueCount.get(a.id) ?? 0) || compareDocumentOrder(a, b);
      }
      return compareDocumentOrder(a, b);
    });
  }, [items, affectedSet, filters, sort, issueCount]);

  const search: Readonly<Record<string, string>> = affected !== undefined && affected.length > 0 ? { affected: affected.join(",") } : {};
  const select = (id: string) => go(buildHref("/requirements/$id", { id }, search));

  // J / K move the selection; ⌘. closes the inspector.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (isTyping(event.target) || document.querySelector("[role='dialog']") !== null) {
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key === ".") {
        event.preventDefault();
        go(buildHref("/requirements", {}, search));
        return;
      }
      if (event.metaKey || event.ctrlKey || event.altKey || (event.key !== "j" && event.key !== "k")) {
        return;
      }
      const index = visible.findIndex((item) => item.id === selectedId);
      const next = event.key === "j" ? (index < 0 ? 0 : Math.min(visible.length - 1, index + 1)) : Math.max(0, index - 1);
      const target = visible[next];
      if (target !== undefined && target.id !== selectedId) {
        event.preventDefault();
        go(buildHref("/requirements/$id", { id: target.id }, search));
        document.getElementById("requirement-" + target.id)?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const filtered = filters !== NO_FILTERS && JSON.stringify(filters) !== JSON.stringify(NO_FILTERS);
  const statuses = [...new Set(items.map((item) => item.status))].sort();
  const sourceNames = (sources.data ?? []).map((source) => source.name);
  const selected = selectedId === undefined ? null : selectedId;

  const kindItems: MenuItem[] = [
    { id: "all", label: t("requirements.filter.allKinds"), onSelect: () => setFilters((f) => ({ ...f, kind: null })) },
    ...KINDS.map((kind) => ({ id: kind, label: t(("ui.kind." + kind) as MessageKey), onSelect: () => setFilters((f) => ({ ...f, kind })) })),
  ];
  const statusItems: MenuItem[] = [
    { id: "all", label: t("requirements.filter.allStatuses"), onSelect: () => setFilters((f) => ({ ...f, status: null })) },
    ...statuses.map((status) => ({ id: status, label: statusLabel(status), onSelect: () => setFilters((f) => ({ ...f, status })) })),
  ];
  const sortItems: MenuItem[] = (Object.keys(SORT_LABEL) as SortOrder[]).map((order) => ({ id: order, label: t(SORT_LABEL[order]), onSelect: () => setSort(order) }));

  let body;
  if (list.isError) {
    body = (
      <div className="flex flex-col items-start gap-3 px-6 py-6 text-body text-fg-2">
        <ReadFailure error={list.error} messageKey="requirements.loadFailed" />
        <Button variant="outline" size={30} onClick={() => void list.refetch()}>
          {t("requirements.retry")}
        </Button>
      </div>
    );
  } else if (list.data === undefined) {
    body = (
      <SkeletonGroup>
        {Array.from({ length: 8 }, (_, index) => (
          <div key={index} className="flex min-h-16 items-center gap-3.5 border-b border-line px-6 py-2.5">
            <Skeleton width={44} height={10} />
            <div className="flex flex-1 flex-col gap-2">
              <Skeleton width="70%" height={12} />
              <Skeleton width="40%" height={12} />
            </div>
            <Skeleton width={64} height={20} />
            <Skeleton width={20} height={20} radius="full" />
          </div>
        ))}
      </SkeletonGroup>
    );
  } else if (items.length === 0 && list.data.health.length === 0) {
    body = (
      <EmptyState
        title={t("requirements.empty.title")}
        action={
          <Button variant="primary" size={38} onClick={() => go("/sources")}>
            {t("requirements.empty.action")}
          </Button>
        }
      />
    );
  } else if (visible.length === 0 && (filtered || affectedSet !== null)) {
    body = (
      <EmptyState
        visual="none"
        headingLevel={2}
        title={t("requirements.noMatch.title")}
        action={
          <Button
            variant="outline"
            size={34}
            onClick={() => {
              setFilters(NO_FILTERS);
              if (affectedSet !== null) {
                go("/requirements");
              }
            }}
          >
            {t("requirements.noMatch.action")}
          </Button>
        }
      />
    );
  } else {
    body = (
      <ul aria-label={t("requirements.list")} className="m-0 list-none p-0">
        {visible.map((item) => (
          <RequirementRow key={item.id} item={item} issues={issueCount.get(item.id) ?? 0} selected={item.id === selected} onSelect={() => select(item.id)} />
        ))}
        {!filtered &&
          affectedSet === null &&
          list.data.health.map((health) => (
            <ListRowError key={health.id} itemId={health.id} onRetry={() => void list.refetch()} className="min-h-16 px-6" />
          ))}
      </ul>
    );
  }

  return (
    <WithInspector
      label={t("requirement.inspector")}
      inspectorKey={selected ?? undefined}
      empty={t("requirements.inspector.empty")}
      inspector={selected === null ? null : <RequirementInspector id={selected} requirements={items} sources={sources.data ?? []} />}
    >
      <div className="sticky top-0 z-10 bg-page">
        <div className="flex items-center gap-3 px-6 pt-6">
          <h1 className="m-0 font-display text-h2 font-bold tracking-display text-fg">{t("requirements.title")}</h1>
          {list.data !== undefined && (
            <span className="text-footnote text-fg-3">
              {sourceNames.length === 0 ? String(items.length) : tf("requirements.meta", { count: items.length, source: sourceNames.join(", ") })}
            </span>
          )}
        </div>
        <div role="group" aria-label={t("requirements.filters")} className="flex flex-wrap items-center gap-2 border-b border-line px-6 py-4">
          <label className="flex h-8 w-60 items-center gap-2 rounded-control border border-line bg-card px-2.5 text-fg-3 focus-within:border-accent focus-within:shadow-focus">
            <Search size={16} strokeWidth={1.6} aria-hidden="true" />
            <input
              type="search"
              aria-label={t("requirements.search")}
              placeholder={t("requirements.search")}
              value={filters.q}
              onChange={(event) => setFilters((f) => ({ ...f, q: event.target.value }))}
              className="min-w-0 flex-1 border-0 bg-transparent text-footnote text-fg outline-none placeholder:text-fg-3"
            />
          </label>
          <Menu
            width={200}
            align="start"
            items={kindItems}
            trigger={
              <FilterChip
                menu
                active={filters.kind !== null}
                label={filters.kind === null ? t("requirements.filter.kind") : tf("requirements.filter.kindValue", { value: t(("ui.kind." + filters.kind) as MessageKey) })}
              />
            }
          />
          <Menu
            width={200}
            align="start"
            items={statusItems}
            trigger={
              <FilterChip
                menu
                active={filters.status !== null}
                label={filters.status === null ? t("requirements.filter.status") : tf("requirements.filter.statusValue", { value: statusLabel(filters.status) })}
              />
            }
          />
          <FilterChip label={t("requirements.filter.hasIssues")} active={filters.hasIssues} onToggle={() => setFilters((f) => ({ ...f, hasIssues: !f.hasIssues }))} />
          <FilterChip label={t("requirements.filter.lowReadiness")} active={filters.lowReadiness} onToggle={() => setFilters((f) => ({ ...f, lowReadiness: !f.lowReadiness }))} />
          <FilterChip label={t("requirements.filter.uncovered")} active={filters.uncovered} onToggle={() => setFilters((f) => ({ ...f, uncovered: !f.uncovered }))} />
          {affectedSet !== null && (
            <FilterChip
              label={tf("requirements.filter.affected", { count: affectedSet.size })}
              active
              onToggle={() => go(selected === null ? "/requirements" : buildHref("/requirements/$id", { id: selected }))}
            />
          )}
          <span className="flex-1" />
          <Menu
            width={200}
            items={sortItems}
            trigger={
              <button type="button" className="inline-flex cursor-pointer items-center gap-1 rounded-control px-1 text-caption text-fg-3 hover:text-fg">
                {tf("requirements.sort.label", { order: t(SORT_LABEL[sort]) })}
                <ChevronDown size={14} strokeWidth={1.6} aria-hidden="true" />
              </button>
            }
          />
        </div>
      </div>
      {body}
    </WithInspector>
  );
}

function RequirementRow({ item, issues, selected, onSelect }: { item: RequirementSummary; issues: number; selected: boolean; onSelect: () => void }) {
  const hasOperation = item.coverage.operations.length > 0;
  const hasScenario = item.coverage.scenarios.length > 0;
  const readiness = item.readiness ?? null;
  return (
    <li data-testid="requirement-row" data-requirement-id={item.id} className="list-none">
      <button
        type="button"
        id={"requirement-" + item.id}
        onClick={onSelect}
        aria-current={selected ? "true" : undefined}
        className={cn(
          "flex min-h-16 w-full cursor-pointer items-center gap-3.5 border-b border-line px-6 py-2.5 text-left transition-colors duration-[120ms] ease-standard",
          selected ? "bg-surface" : "bg-card hover:bg-subtle-row",
        )}
      >
        <MonoId className="w-[52px] shrink-0 truncate" title={item.id}>
          {requirementLabel(item)}
        </MonoId>
        <span className="line-clamp-2 min-w-0 flex-1 text-callout leading-[1.45] text-fg">{item.text}</span>
        <KindTag kind={item.class} withIcon={false} className="h-[22px] px-2" />
        <span className="flex w-11 shrink-0 justify-end">
          {issues > 0 && (
            <span className="inline-flex items-center gap-1 text-caption font-semibold text-attention-fg">
              <TriangleAlert size={14} strokeWidth={1.6} aria-hidden="true" />
              <span aria-hidden="true">{issues}</span>
              <span className="sr-only">{tc("requirements.row.issues", issues)}</span>
            </span>
          )}
        </span>
        <span className="flex w-10 shrink-0 gap-1.5">
          <span className={hasOperation ? "text-fg" : "text-line-strong"}>
            <Zap size={16} strokeWidth={1.6} aria-hidden="true" />
            <span className="sr-only">{t(hasOperation ? "requirements.row.operation" : "requirements.row.noOperation")}</span>
          </span>
          <span className={hasScenario ? "text-fg" : "text-line-strong"}>
            <FlaskConical size={16} strokeWidth={1.6} aria-hidden="true" />
            <span className="sr-only">{t(hasScenario ? "requirements.row.scenario" : "requirements.row.noScenario")}</span>
          </span>
        </span>
        <span className="flex w-[60px] shrink-0 items-center justify-end gap-1.5">
          <span aria-hidden="true" className="font-mono text-eyebrow font-medium text-fg-3">
            {readiness === null ? NO_VALUE : String(Math.round(readiness))}
          </span>
          <ReadinessRing value={readiness} size={20} stroke={3} />
        </span>
      </button>
    </li>
  );
}
