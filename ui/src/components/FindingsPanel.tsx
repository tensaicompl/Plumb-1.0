import { useMemo, useState } from "react";

import type { components } from "../api/schema";
import { t, type MessageKey } from "../i18n";

export type Finding = components["schemas"]["Finding"];

type FilterField = "severity" | "status" | "family" | "code";

const FILTERS: readonly { field: FilterField; label: MessageKey }[] = [
  { field: "severity", label: "findings.filter.severity" },
  { field: "status", label: "findings.filter.status" },
  { field: "family", label: "findings.filter.family" },
  { field: "code", label: "findings.filter.code" },
];

type Filters = Readonly<Record<FilterField, string | null>>;

const NO_FILTERS: Filters = { severity: null, status: null, family: null, code: null };

/** The distinct values of `field` in server order of first appearance (no invented ordering). */
function distinct(findings: readonly Finding[], field: FilterField): string[] {
  return [...new Set(findings.map((f) => f[field]))];
}

/**
 * Server findings with view-only filtering by severity, status, family and code, and local
 * pinning. Nothing here changes a finding; affected references open through `onOpenRef`.
 */
export function FindingsPanel({
  findings,
  onOpenRef,
}: {
  findings: readonly Finding[];
  onOpenRef: (ref: string) => void;
}) {
  const [filters, setFilters] = useState<Filters>(NO_FILTERS);
  const [pinned, setPinned] = useState<ReadonlySet<string>>(new Set());

  const visible = useMemo(
    () => findings.filter((f) => FILTERS.every(({ field }) => filters[field] === null || f[field] === filters[field])),
    [findings, filters],
  );
  const pinnedFindings = visible.filter((f) => pinned.has(f.id));
  const otherFindings = visible.filter((f) => !pinned.has(f.id));

  const togglePin = (id: string) => {
    setPinned((current) => {
      const next = new Set(current);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const renderFinding = (finding: Finding) => {
    const isPinned = pinned.has(finding.id);
    return (
      <li key={finding.id} data-finding-id={finding.id} data-pinned={isPinned} className="rounded border p-2">
        <div className="flex items-center justify-between gap-2">
          <span className="font-mono text-xs">{finding.code}</span>
          <button type="button" aria-pressed={isPinned} onClick={() => togglePin(finding.id)} className="rounded border px-2 text-xs">
            {t(isPinned ? "findings.unpin" : "findings.pin")}
          </button>
        </div>
        <p className="text-sm">{finding.message}</p>
        <dl className="grid grid-cols-[auto_1fr] gap-x-2 text-xs">
          <dt>{t("findings.filter.severity")}</dt>
          <dd data-field="severity">{finding.severity}</dd>
          <dt>{t("findings.filter.status")}</dt>
          <dd data-field="status">{finding.status}</dd>
          <dt>{t("findings.filter.family")}</dt>
          <dd data-field="family">{finding.family}</dd>
          {finding.waiver_ref !== undefined && finding.waiver_ref !== null && (
            <>
              <dt>{t("findings.waiver")}</dt>
              <dd className="font-mono">{finding.waiver_ref}</dd>
            </>
          )}
        </dl>
        {finding.affected_refs.length > 0 && (
          <ul className="mt-1 flex flex-wrap gap-1" aria-label={t("findings.affected")}>
            {finding.affected_refs.map((ref) => (
              <li key={ref}>
                <button type="button" className="font-mono text-xs underline" onClick={() => onOpenRef(ref)}>
                  {ref}
                </button>
              </li>
            ))}
          </ul>
        )}
      </li>
    );
  };

  return (
    <section aria-label={t("findings.title")} className="space-y-2">
      <div className="flex flex-wrap gap-2">
        {FILTERS.map(({ field, label }) => (
          <label key={field} className="text-xs">
            {t(label)}
            <select
              className="ml-1 rounded border"
              value={filters[field] ?? ""}
              data-filter={field}
              onChange={(event) =>
                setFilters((current) => ({ ...current, [field]: event.target.value === "" ? null : event.target.value }))
              }
            >
              <option value="">{t("findings.filter.all")}</option>
              {distinct(findings, field).map((value) => (
                <option key={value} value={value}>
                  {value}
                </option>
              ))}
            </select>
          </label>
        ))}
      </div>
      {visible.length === 0 && <p className="text-sm">{t("findings.empty")}</p>}
      {pinnedFindings.length > 0 && (
        <section aria-label={t("findings.pinned")}>
          <ul className="space-y-1" data-list="pinned">
            {pinnedFindings.map(renderFinding)}
          </ul>
        </section>
      )}
      {otherFindings.length > 0 && (
        <ul className="space-y-1" data-list="findings">
          {otherFindings.map(renderFinding)}
        </ul>
      )}
    </section>
  );
}
