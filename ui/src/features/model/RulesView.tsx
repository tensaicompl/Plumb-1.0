import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { TriangleAlert } from "lucide-react";
import { useId, useState, type MouseEvent } from "react";

import { apiClient, readReviewed } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation } from "../../api/useVersionedMutation";
import { buildHref, useGo } from "../../app/shell/navigation";
import { cn, EmptyState, isPlainClick, ListRowError, ReasonSheet, SegmentedControl, Skeleton, SkeletonGroup, toast, TONE_PILL, type Tone } from "../../components/ui";
import { t, tc, tf } from "../../i18n";
import { humanize, useLabels } from "./labels";
import { describeInputCell, describeOutputCell, describePoint, outcomeCell, overlapPartners, parseColumn, type Column } from "./ruleCells";
import { ReadError, SegmentBody, useCanEdit, WriteError } from "./shared";

type Rule = components["schemas"]["Rule"];
type WriteResult = components["schemas"]["WriteResult"];

export const rulesQuery = {
  queryKey: ["model", "rules"] as const,
  queryFn: () => readReviewed(apiClient.GET("/api/model/rules")),
};

function Badge({ rule }: { rule: Rule }) {
  if (rule.kind !== "decision_table" || rule.uncovered_cells === null || rule.overlaps === null) {
    return null;
  }
  const parts: string[] = [];
  if (rule.uncovered_cells.length > 0) {
    parts.push(tc("model.rules.gaps", rule.uncovered_cells.length));
  }
  if (rule.overlaps.length > 0) {
    parts.push(tc("model.rules.overlaps", rule.overlaps.length));
  }
  const tone: Tone = parts.length === 0 ? "pass" : "attention";
  return (
    <span className={cn("inline-flex h-5 shrink-0 items-center whitespace-nowrap rounded-pill px-2 text-eyebrow font-semibold", TONE_PILL[tone])}>
      {parts.length === 0 ? t("model.rules.complete") : parts.join(" · ")}
    </span>
  );
}

/** Model · Rules (spec §5.4.3): the rule list and the selected rule's decision table. */
export function RulesView() {
  const params: { id?: string } = useParams({ strict: false });
  const go = useGo();
  const query = useQuery(rulesQuery);
  const items = query.data?.items ?? [];
  const health = query.data?.health ?? [];
  const label = useLabels(items.map((rule) => rule.id));
  const name = (rule: Rule) => (label.kind(rule.id) === null ? humanize(rule.name) : label(rule.id));
  const selected = items.find((rule) => rule.id === params.id) ?? items[0] ?? null;
  const open = (event: MouseEvent<HTMLAnchorElement>, href: string) => {
    if (isPlainClick(event)) {
      event.preventDefault();
      go(href);
    }
  };
  if (query.isError) {
    return (
      <SegmentBody>
        <ReadError error={query.error} messageKey="model.rules.loadFailed" onRetry={() => void query.refetch()} />
      </SegmentBody>
    );
  }
  if (query.isPending) {
    return (
      <SegmentBody>
        <SkeletonGroup className="grid grid-cols-[260px_1fr] gap-5">
          <Skeleton height={140} radius="card" />
          <Skeleton height={280} radius="card" />
        </SkeletonGroup>
      </SegmentBody>
    );
  }
  if (items.length === 0 && health.length === 0) {
    return (
      <SegmentBody>
        <EmptyState title={t("model.rules.empty.title")} description={t("model.rules.empty.body")} />
      </SegmentBody>
    );
  }
  return (
    <SegmentBody className="grid grid-cols-[260px_minmax(0,1fr)] items-start gap-5">
      <nav aria-label={t("nav.rules")} className="overflow-hidden rounded-tile border border-line bg-card">
        <ul className="m-0 list-none p-0">
          {items.map((rule) => {
            const href = buildHref("/model/rules/$id", { id: rule.id });
            const isSelected = rule.id === selected?.id;
            return (
              <li key={rule.id} className="border-b border-line last:border-b-0">
                <a
                  href={href}
                  onClick={(event) => open(event, href)}
                  aria-current={isSelected ? "page" : undefined}
                  className={cn(
                    "flex items-center justify-between gap-2 px-4 py-3 text-footnote no-underline",
                    isSelected ? "bg-surface font-semibold text-fg" : "text-fg-2 hover:bg-subtle-row",
                  )}
                >
                  <span className="min-w-0 truncate">{name(rule)}</span>
                  <Badge rule={rule} />
                </a>
              </li>
            );
          })}
          {health.map((item) => (
            <ListRowError key={item.id} itemId={item.id} onRetry={() => void query.refetch()} className="min-h-0 px-4 py-3 text-footnote" />
          ))}
        </ul>
      </nav>
      {selected !== null && <RuleDetail key={selected.id} rule={selected} name={name(selected)} etag={query.data.etag} />}
    </SegmentBody>
  );
}

function RuleDetail({ rule, name, etag }: { rule: Rule; name: string; etag: string | null }) {
  const headingId = useId();
  const canEdit = useCanEdit();
  const [point, setPoint] = useState<number | null>(null);
  const inputs = rule.conditions.map(parseColumn);
  const outputs = (rule.outputs ?? []).map(parseColumn);
  const partners = overlapPartners(rule.overlaps);
  const card = "rounded-card border border-line bg-card p-6 shadow-sm";
  if (rule.kind !== "decision_table") {
    return (
      <section aria-labelledby={headingId} className={card}>
        <h2 id={headingId} className="m-0 font-display text-title font-semibold tracking-display text-fg">
          {name}
        </h2>
        <p className="mb-5 mt-1 text-footnote text-fg-3">{t("model.rules.scalar")}</p>
        {rule.conditions.map((condition) => (
          <pre key={condition} className="m-0 whitespace-pre-wrap rounded-field bg-hero px-[18px] py-4 font-mono text-body leading-[1.7] text-on-hero">
            {condition}
          </pre>
        ))}
        {rule.conditions.length === 0 && <p className="m-0 text-footnote text-fg-3">{t("model.rules.noCondition")}</p>}
      </section>
    );
  }
  const head = "bg-surface px-3.5 py-2.5 text-left text-caption font-semibold text-fg-2";
  const cell = "border-t border-line px-3.5 py-3 align-middle";
  return (
    <section aria-labelledby={headingId} className={card}>
      <h2 id={headingId} className="m-0 font-display text-title font-semibold tracking-display text-fg">
        {name}
      </h2>
      <p className="mb-5 mt-1 text-footnote text-fg-3">
        {[t("model.rules.decisionTable"), tc("model.rules.conditions", inputs.length), tc("model.rules.outcomes", outputs.length)].join(" · ")}
      </p>
      {rule.rows === null || rule.rows.length === 0 ? (
        <p className="m-0 text-footnote text-fg-3">{t("model.rules.noRows")}</p>
      ) : (
        <div className="overflow-hidden rounded-field border border-line">
          <table className="w-full border-collapse text-footnote">
            <caption className="sr-only">{name}</caption>
            <thead>
              <tr>
                {inputs.map((column, index) => (
                  <th key={"in" + String(index)} scope="col" className={head}>
                    {tf(index === 0 ? "model.rules.when" : "model.rules.andWhen", { name: column.name })}
                  </th>
                ))}
                {outputs.map((column, index) => (
                  <th key={"out" + String(index)} scope="col" className={head}>
                    {tf(index === 0 ? "model.rules.then" : "model.rules.andThen", { name: column.name })}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rule.rows.map((row, index) => {
                const overlap = partners.get(index);
                return (
                  <tr key={index} className={overlap === undefined ? undefined : "bg-attention-tint/40"}>
                    {row.when.map((text, column) => (
                      <td key={"w" + String(column)} className={cell}>
                        {column === 0 && overlap !== undefined && (
                          <span className="mr-2 inline-flex items-center gap-1 text-eyebrow font-semibold text-attention-fg">
                            <TriangleAlert size={12} strokeWidth={1.6} aria-hidden="true" />
                            {tf("model.rules.overlapsWith", { rows: overlap.map((r) => String(r + 1)).join(", ") })}
                          </span>
                        )}
                        {describeInputCell(text)}
                      </td>
                    ))}
                    {row.then.map((text, column) => (
                      <td key={"t" + String(column)} className={cn(cell, "font-semibold")}>
                        {describeOutputCell(text)}
                      </td>
                    ))}
                  </tr>
                );
              })}
              {(rule.uncovered_cells ?? []).map((text, index) => (
                <tr key={"gap" + String(index)}>
                  {describePoint(text).map((value, column) => (
                    <td key={column} className={cell}>
                      {value}
                    </td>
                  ))}
                  <td colSpan={Math.max(1, outputs.length)} className="border-t border-line px-2 py-1.5">
                    {canEdit && outputs.length === 1 ? (
                      <button
                        type="button"
                        onClick={() => setPoint(index)}
                        className="h-8 w-full cursor-pointer rounded-[6px] border border-dashed border-attention bg-attention-tint/60 text-caption font-semibold text-attention-fg hover:bg-attention-tint"
                      >
                        {t("model.rules.setOutcome")}
                      </button>
                    ) : (
                      <span className="inline-flex h-8 w-full items-center justify-center rounded-[6px] border border-dashed border-attention text-caption font-semibold text-attention-fg">
                        {t("model.rules.uncovered")}
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {rule.uncovered_cells === null && <p className="mb-0 mt-3 text-caption text-fg-3">{t("model.rules.notAnalyzable")}</p>}
      {point !== null && outputs[0] !== undefined && (
        <SetOutcomeSheet rule={rule} point={point} values={describePoint(rule.uncovered_cells?.[point] ?? "")} output={outputs[0]} etag={etag} onClose={() => setPoint(null)} />
      )}
    </section>
  );
}

function SetOutcomeSheet({ rule, point, values, output, etag, onClose }: { rule: Rule; point: number; values: readonly string[]; output: Column; etag: string | null; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [choice, setChoice] = useState("");
  const fieldId = useId();
  const write = useVersionedMutation<WriteResult, string>(
    (reason, ifMatch) =>
      apiClient.POST("/api/model/rules/{id}/cells", {
        params: { path: { id: rule.id }, header: { "If-Match": ifMatch } },
        body: { cell: "uncovered:" + String(point) + ":output:0", outcome: outcomeCell(output, choice), reason },
      }),
    etag,
  );
  const options =
    output.type === "bool"
      ? [
          { value: "yes", label: t("model.rules.yes") },
          { value: "no", label: t("model.rules.no") },
        ]
      : output.members.map((member) => ({ value: member, label: member }));
  return (
    <ReasonSheet
      open
      onOpenChange={(open) => !open && onClose()}
      title={tf("model.rules.setOutcomeTitle", { point: values.join(" × ") })}
      description={tf("model.rules.setOutcomeDescription", { point: values.join(" + "), output: output.name })}
      confirmLabel={t("model.rules.setOutcomeConfirm")}
      confirmDisabled={choice === ""}
      pending={write.isPending}
      error={write.error === null ? undefined : <WriteError error={write.error} />}
      onConfirm={(reason) =>
        write.mutate(reason, {
          onSuccess: () => {
            void queryClient.invalidateQueries({ queryKey: ["model"] });
            toast({ message: t("model.rules.outcomeSet") });
            onClose();
          },
        })
      }
    >
      <div className="flex flex-col gap-1.5">
        <span id={fieldId} className="text-caption font-semibold text-fg-2">
          {tf("model.rules.outcomeLabel", { output: output.name })}
        </span>
        {options.length > 0 ? (
          <SegmentedControl label={tf("model.rules.outcomeLabel", { output: output.name })} options={options} value={choice} onChange={setChoice} />
        ) : (
          <input
            aria-labelledby={fieldId}
            value={choice}
            onChange={(event) => setChoice(event.target.value.trim())}
            className="h-[38px] w-full rounded-field border border-line bg-card px-3 font-mono text-callout text-fg outline-none focus:border-accent focus:shadow-focus"
          />
        )}
      </div>
    </ReasonSheet>
  );
}
