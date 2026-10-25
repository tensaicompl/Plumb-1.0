import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { useId, type MouseEvent } from "react";

import { apiClient, readData, readReviewed } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation } from "../../api/useVersionedMutation";
import { buildHref, useGo } from "../../app/shell/navigation";
import { Button, cn, EmptyState, isPlainClick, Skeleton, SkeletonGroup, StatusPill, toast } from "../../components/ui";
import { t, tc, tf, useLocaleStore } from "../../i18n";
import { sentenceCase } from "./labels";
import { exampleWeek, expressionInputs, plainExpression } from "./plainExpr";
import { ReadError, SectionLabel, SegmentBody, useCanEdit, WriteError } from "./shared";

type Calculation = components["schemas"]["Calculation"];
type CalculationValue = components["schemas"]["CalculationValue"];
type WriteResult = components["schemas"]["WriteResult"];

export const calculationsQuery = {
  queryKey: ["model", "calculations"] as const,
  queryFn: () => readReviewed(apiClient.GET("/api/model/calculations")),
};

/** The example inputs for a date-range calculation: start and end date, full days. */
function rangeInputs(expression: string, start: string, end: string): Record<string, CalculationValue> | null {
  const inputs = expressionInputs(expression);
  if (!inputs.has("start_date") || !inputs.has("end_date")) {
    return null;
  }
  const values: Record<string, CalculationValue> = { start_date: { type: "date", value: start }, end_date: { type: "date", value: end } };
  if (inputs.has("day_fraction")) {
    values["day_fraction"] = { type: "decimal", value: "1" };
  }
  return values;
}

function fetchExample(id: string, inputs: Record<string, CalculationValue>) {
  return readData(apiClient.GET("/api/model/calculations/{id}/example", { params: { path: { id }, query: { inputs: JSON.stringify(inputs) } } }));
}

function numeric(value: CalculationValue): number | null {
  return "value" in value && (typeof value.value === "string" || typeof value.value === "number") ? Number(value.value) : null;
}

function resultText(value: CalculationValue): string {
  const amount = numeric(value);
  if (value.type === "quantity" || value.type === "duration") {
    if (value.unit === "working_day" && amount !== null) {
      return tc("model.calculations.workingDays", amount, { count: value.value });
    }
    return value.value + " " + value.unit.replace(/_/g, " ");
  }
  if (value.type === "enum") {
    return value.member;
  }
  return String(value.value);
}

/** Model · Calculations (spec §5.4.4): the calculation card (one selectable when there are several). */
export function CalculationsView() {
  const params: { id?: string } = useParams({ strict: false });
  const go = useGo();
  const query = useQuery(calculationsQuery);
  const items = query.data?.items ?? [];
  const selected = items.find((calculation) => calculation.id === params.id) ?? items[0] ?? null;
  const open = (event: MouseEvent<HTMLAnchorElement>, href: string) => {
    if (isPlainClick(event)) {
      event.preventDefault();
      go(href);
    }
  };
  if (query.isError) {
    return (
      <SegmentBody>
        <ReadError error={query.error} messageKey="model.calculations.loadFailed" onRetry={() => void query.refetch()} />
      </SegmentBody>
    );
  }
  if (query.isPending) {
    return (
      <SegmentBody>
        <SkeletonGroup className="max-w-[880px]">
          <Skeleton height={360} radius="card" />
        </SkeletonGroup>
      </SegmentBody>
    );
  }
  if (selected === null) {
    return (
      <SegmentBody>
        <EmptyState title={t("model.calculations.empty.title")} description={t("model.calculations.empty.body")} />
      </SegmentBody>
    );
  }
  const card = <CalculationCard key={selected.id} calculation={selected} etag={query.data.etag} />;
  if (items.length === 1) {
    return <SegmentBody className="max-w-[936px]">{card}</SegmentBody>;
  }
  return (
    <SegmentBody className="grid max-w-[1200px] grid-cols-[260px_minmax(0,1fr)] items-start gap-5">
      <nav aria-label={t("nav.calculations")} className="overflow-hidden rounded-tile border border-line bg-card">
        <ul className="m-0 list-none p-0">
          {items.map((calculation) => {
            const href = buildHref("/model/calculations/$id", { id: calculation.id });
            const isSelected = calculation.id === selected.id;
            return (
              <li key={calculation.id} className="border-b border-line last:border-b-0">
                <a
                  href={href}
                  onClick={(event) => open(event, href)}
                  aria-current={isSelected ? "page" : undefined}
                  className={cn("block px-4 py-3 text-footnote no-underline", isSelected ? "bg-surface font-semibold text-fg" : "text-fg-2 hover:bg-subtle-row")}
                >
                  {sentenceCase(calculation.name)}
                </a>
              </li>
            );
          })}
        </ul>
      </nav>
      {card}
    </SegmentBody>
  );
}

function CalculationCard({ calculation, etag }: { calculation: Calculation; etag: string | null }) {
  const headingId = useId();
  const canEdit = useCanEdit();
  const queryClient = useQueryClient();
  const plain = plainExpression(calculation.expr);
  const proposed = calculation.status === "Proposed";
  const confirm = useVersionedMutation<WriteResult>(
    (_, ifMatch) => apiClient.POST("/api/model/calculations/{id}/confirm", { params: { path: { id: calculation.id }, header: { "If-Match": ifMatch } } }),
    etag,
  );
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-5 rounded-card border border-line bg-card p-7 shadow-sm">
      <div className="flex items-center gap-2.5">
        <h2 id={headingId} className="m-0 font-display text-title-lg font-semibold tracking-display text-fg">
          {sentenceCase(calculation.name)}
        </h2>
        {proposed ? <StatusPill status="proposed" /> : calculation.status === "Accepted" ? <StatusPill status="confirmed" /> : <StatusPill status="proposed" label={calculation.status} />}
      </div>
      <pre className="m-0 whitespace-pre-wrap break-words rounded-field bg-hero px-[18px] py-4 font-mono text-body leading-[1.7] text-on-hero">{calculation.expr}</pre>
      {plain !== null && (
        <div>
          <SectionLabel className="mb-1.5">{t("model.calculations.plain")}</SectionLabel>
          <p className="m-0 text-body leading-relaxed text-fg">{plain}</p>
        </div>
      )}
      <WorkedExample calculation={calculation} />
      {canEdit && proposed && (
        <div className="flex flex-col items-start gap-2">
          <Button
            variant="primary"
            size={38}
            className="h-9 px-[18px]"
            pending={confirm.isPending}
            onClick={() =>
              confirm.mutate(undefined, {
                onSuccess: () => {
                  void queryClient.invalidateQueries({ queryKey: ["model"] });
                  toast({ message: t("model.calculations.confirmed") });
                },
              })
            }
          >
            {t("model.calculations.confirm")}
          </Button>
          <WriteError error={confirm.error} />
        </div>
      )}
    </section>
  );
}

/**
 * The worked example (get_calculation_example): a full week of a date-range calculation and one
 * request per day, so each tile shows whether the server calendar counts that day.
 */
function WorkedExample({ calculation }: { calculation: Calculation }) {
  const groupId = useId();
  const locale = useLocaleStore((state) => state.locale);
  const week = exampleWeek(new Date());
  const start = week[0] ?? "";
  const end = week[6] ?? "";
  const weekInputs = rangeInputs(calculation.expr, start, end);
  const total = useQuery({
    queryKey: ["model", "calculation-example", calculation.id, start, end],
    queryFn: () => fetchExample(calculation.id, weekInputs ?? {}),
    retry: false,
  });
  const days = useQueries({
    queries: week.map((day) => ({
      queryKey: ["model", "calculation-example", calculation.id, day, day],
      queryFn: () => fetchExample(calculation.id, rangeInputs(calculation.expr, day, day) ?? {}),
      enabled: weekInputs !== null && total.isSuccess,
      retry: false,
    })),
  });
  const wrapper = "flex flex-col gap-2.5 rounded-tile border border-line p-4";
  const title = (
    <SectionLabel id={groupId} className="mb-0.5">
      {t("model.calculations.example")}
    </SectionLabel>
  );
  if (weekInputs === null || total.isError) {
    return (
      <div role="group" aria-labelledby={groupId} className={wrapper}>
        {title}
        <p className="m-0 text-footnote text-fg-3">{t("model.calculations.noExample")}</p>
      </div>
    );
  }
  const date = (iso: string, options: Intl.DateTimeFormatOptions) => new Intl.DateTimeFormat(locale, { ...options, timeZone: "UTC" }).format(new Date(iso + "T00:00:00Z"));
  return (
    <div role="group" aria-labelledby={groupId} className={wrapper}>
      {title}
      <div className="flex flex-wrap items-center gap-1.5">
        <ul className="m-0 flex list-none gap-1.5 p-0">
          {week.map((day, index) => {
            const value = days[index]?.data?.value;
            const amount = value === undefined ? null : numeric(value);
            const working = amount === null ? null : amount > 0;
            return (
              <li
                key={day}
                data-working={working === null ? undefined : String(working)}
                className={cn(
                  "w-14 rounded-control py-2 text-center text-caption",
                  working === null ? "animate-shimmer bg-surface text-fg-3-surface" : working ? "bg-primary text-on-primary" : "bg-surface text-fg-3-surface",
                )}
              >
                <b className="block text-body">{date(day, { day: "numeric" })}</b>
                {date(day, { weekday: "short" })}
                {working !== null && <span className="sr-only">{t(working ? "model.calculations.working" : "model.calculations.notWorking")}</span>}
              </li>
            );
          })}
        </ul>
        {total.data !== undefined && <span className="ml-3 text-subhead font-semibold text-fg">{"→ " + resultText(total.data.value)}</span>}
      </div>
      <p className="m-0 text-caption text-fg-3">
        {tf("model.calculations.exampleCaption", { from: date(start, { day: "numeric", month: "short" }), to: date(end, { day: "numeric", month: "short", year: "numeric" }) })}
      </p>
    </div>
  );
}
