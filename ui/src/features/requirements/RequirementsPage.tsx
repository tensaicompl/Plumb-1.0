import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";

import { apiClient, readData } from "../../api/client";
import { isCursorBaselineChanged, useCursorPages, useCursorRecovery } from "../../api/useCursorPages";
import type { components } from "../../api/schema";
import { t } from "../../i18n";
import { ReadFailure } from "../../components/ReadFailure";

type RequirementPage = components["schemas"]["RequirementPage"];

/** The requirement page size requested from the server. */
export const REQUIREMENTS_PAGE_SIZE = 25;

/** The server filters of GET /api/requirements: exact wire strings, absent when empty. */
export interface RequirementFilters {
  readonly status: string;
  readonly class: string;
  readonly q: string;
}

const NO_FILTERS: RequirementFilters = { status: "", class: "", q: "" };

/** The query key of one requirement page. */
export function requirementsQueryKey(filters: RequirementFilters, cursor: string | null) {
  return ["requirements", "list", filters, cursor] as const;
}

async function fetchRequirements(filters: RequirementFilters, cursor: string | null): Promise<RequirementPage> {
  const query: { status?: string; class?: string; q?: string; cursor?: string; limit: number } = { limit: REQUIREMENTS_PAGE_SIZE };
  // Status values are capitalized (Accepted) and classes lowercase (functional) on the wire;
  // typed filters are normalized so "accepted" or "Functional" do not fail.
  const status = filters.status.trim();
  if (status !== "") {
    query.status = status.charAt(0).toUpperCase() + status.slice(1).toLowerCase();
  }
  if (filters.class.trim() !== "") {
    query.class = filters.class.trim().toLowerCase();
  }
  if (filters.q !== "") {
    query.q = filters.q;
  }
  if (cursor !== null) {
    query.cursor = cursor;
  }
  return (await readData(apiClient.GET("/api/requirements", { params: { query } })));
}

/**
 * The requirement list (v2 U2.1): server-side status, class and text filters, cursor pagination and
 * the advisory readiness chip. Readiness is shown, never treated as a gate.
 */
export function RequirementsPage() {
  const [draft, setDraft] = useState<RequirementFilters>(NO_FILTERS);
  const [filters, setFilters] = useState<RequirementFilters>(NO_FILTERS);
  const pages = useCursorPages();
  const cursor = pages.cursor;
  const page = useQuery({
    queryKey: requirementsQueryKey(filters, cursor),
    queryFn: () => fetchRequirements(filters, cursor),
  });
  useCursorRecovery(pages, page.error);

  const fields: readonly { key: keyof RequirementFilters; label: "requirements.filter.status" | "requirements.filter.class" | "requirements.filter.q" }[] = [
    { key: "status", label: "requirements.filter.status" },
    { key: "class", label: "requirements.filter.class" },
    { key: "q", label: "requirements.filter.q" },
  ];

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("requirements.title")}</h1>
      <form
        aria-label={t("requirements.filters")}
        className="flex flex-wrap items-end gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          setFilters(draft);
          pages.first();
        }}
      >
        {fields.map(({ key, label }) => (
          <label key={key} className="text-sm">
            {t(label)}
            <input
              className="ml-1 rounded border p-1"
              value={draft[key]}
              data-filter={key}
              onChange={(event) => setDraft((current) => ({ ...current, [key]: event.target.value }))}
            />
          </label>
        ))}
        <button type="submit" className="rounded border px-3 py-1">
          {t("requirements.filter.apply")}
        </button>
      </form>
      <p className="text-sm text-slate-600">{t("readiness.advisory")}</p>
      {pages.recovered && (
        <p role="status" className="text-sm" data-testid="cursor-reset">
          {t("pagination.reset")}
        </p>
      )}
      {page.isError && !isCursorBaselineChanged(page.error) && <ReadFailure error={page.error} messageKey="requirements.loadFailed" />}
      {page.data === undefined && !page.isError && <p>{t("requirements.loading")}</p>}
      {page.data !== undefined && (
        <>
          {page.data.items.length === 0 ? (
            <p>{t("requirements.empty")}</p>
          ) : (
            <table className="w-full text-left text-sm">
            <caption className="sr-only">{t("requirements.caption")}</caption>
              <thead>
                <tr>
                  <th scope="col">{t("requirements.column.id")}</th>
                  <th scope="col">{t("requirements.column.text")}</th>
                  <th scope="col">{t("requirements.column.class")}</th>
                  <th scope="col">{t("requirements.column.status")}</th>
                  <th scope="col">{t("requirements.column.smells")}</th>
                  <th scope="col">{t("requirements.column.readiness")}</th>
                </tr>
              </thead>
              <tbody>
                {page.data.items.map((item) => (
                  <tr key={item.id} data-requirement-id={item.id}>
                    <td className="font-mono">
                      <Link to="/requirements/$id" params={{ id: item.id }}>
                        {item.id}
                      </Link>
                    </td>
                    <td>{item.text}</td>
                    <td className="font-mono">{item.class}</td>
                    <td className="font-mono">{item.status}</td>
                    <td data-lint={item.lint_current ? "current" : "unavailable"}>
                      {item.lint_current ? item.smells.length : t("readiness.notCalculated")}
                    </td>
                    <td data-readiness={(item.readiness ?? null) === null ? "null" : "value"}>
                      {item.readiness === undefined || item.readiness === null ? t("readiness.notCalculated") : String(item.readiness)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <div className="flex gap-2">
            <button type="button" className="rounded border px-3 py-1" disabled={pages.isFirst} onClick={pages.previous}>
              {t("pagination.previous")}
            </button>
            <button
              type="button"
              className="rounded border px-3 py-1"
              disabled={(page.data.next_cursor ?? null) === null}
              onClick={() => {
                const next = page.data.next_cursor ?? null;
                if (next !== null) {
                  pages.next(next);
                }
              }}
            >
              {t("pagination.next")}
            </button>
          </div>
        </>
      )}
    </div>
  );
}
