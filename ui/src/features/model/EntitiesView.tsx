import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { ChevronDown, Plus } from "lucide-react";
import { useId, useState, type MouseEvent } from "react";

import { apiClient, readReviewed } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation } from "../../api/useVersionedMutation";
import { buildHref, useGo } from "../../app/shell/navigation";
import { cn, EmptyState, isPlainClick, Menu, Sheet, Skeleton, SkeletonGroup, toast } from "../../components/ui";
import { t, tc, tf } from "../../i18n";
import { sentenceCase, useLabels } from "./labels";
import { ReadError, SectionLabel, SegmentBody, useCanEdit, WriteError } from "./shared";
import { StateDiagram } from "./StateDiagram";

type Entity = components["schemas"]["Entity"];
type EntityAttribute = components["schemas"]["EntityAttribute"];
type WriteResult = components["schemas"]["WriteResult"];

export const entitiesQuery = {
  queryKey: ["model", "entities"] as const,
  queryFn: () => readReviewed(apiClient.GET("/api/model/entities")),
};

/** The pilot primitive value types the server accepts besides value-type Concept names. */
export const PRIMITIVE_TYPES = ["String", "Int", "Bool", "Date", "DateTime", "Decimal(2)"] as const;

/** The pilot data classes (an existing class is never overwritten). */
const ARROW = "→";

export const DATA_CLASSES = ["pii", "financial", "confidential"] as const;

function entityMeta(entity: Entity): string {
  const parts = [tc("model.entities.attributes", entity.attributes.length)];
  if (entity.states.length > 0) {
    parts.push(tc("model.entities.states", entity.states.length));
  } else if (entity.relationships.length > 0) {
    parts.push(tc("model.entities.relations", entity.relationships.length));
  }
  return parts.join(" · ");
}

/** Model · Entities (spec §5.4.2): the entity cards and the selected entity's detail card. */
export function EntitiesView() {
  const params: { id?: string } = useParams({ strict: false });
  const go = useGo();
  const query = useQuery(entitiesQuery);
  const items = query.data?.items ?? [];
  // Without a deep link, open the richest entity (most attributes and states), as the handoff does.
  const richest = items.reduce<Entity | null>((best, entity) => (best === null || entity.attributes.length + entity.states.length > best.attributes.length + best.states.length ? entity : best), null);
  const selected = items.find((entity) => entity.id === params.id) ?? richest;
  const open = (event: MouseEvent<HTMLAnchorElement>, href: string) => {
    if (isPlainClick(event)) {
      event.preventDefault();
      go(href);
    }
  };
  if (query.isError) {
    return (
      <SegmentBody>
        <ReadError error={query.error} messageKey="model.entities.loadFailed" onRetry={() => void query.refetch()} />
      </SegmentBody>
    );
  }
  if (query.isPending) {
    return (
      <SegmentBody>
        <SkeletonGroup className="flex flex-col gap-5">
          <div className="grid grid-cols-5 gap-3">
            {[0, 1, 2, 3, 4].map((card) => (
              <Skeleton key={card} height={64} radius="card" />
            ))}
          </div>
          <Skeleton height={320} radius="card" />
        </SkeletonGroup>
      </SegmentBody>
    );
  }
  if (selected === null) {
    return (
      <SegmentBody>
        <EmptyState title={t("model.entities.empty.title")} description={t("model.entities.empty.body")} />
      </SegmentBody>
    );
  }
  return (
    <SegmentBody className="flex flex-col gap-5">
      <ul aria-label={t("nav.entities")} className="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-3 p-0 xl:grid-cols-5">
        {items.map((entity) => {
          const href = buildHref("/model/entities/$id", { id: entity.id });
          const isSelected = entity.id === selected.id;
          return (
            <li key={entity.id}>
              <a
                href={href}
                onClick={(event) => open(event, href)}
                aria-current={isSelected ? "true" : undefined}
                className={cn(
                  "block rounded-tile border bg-card p-3.5 text-left no-underline transition-shadow duration-[120ms] ease-standard hover:shadow-md",
                  isSelected ? "border-primary shadow-md" : "border-line shadow-xs",
                )}
              >
                <span className="block text-body font-semibold text-fg">{sentenceCase(entity.name)}</span>
                <span className="mt-1 block text-caption text-fg-3">{entityMeta(entity)}</span>
              </a>
            </li>
          );
        })}
      </ul>
      <EntityDetail key={selected.id} entity={selected} etag={query.data.etag} />
    </SegmentBody>
  );
}

function EntityDetail({ entity, etag }: { entity: Entity; etag: string | null }) {
  const headingId = useId();
  const label = useLabels([...entity.states, ...entity.transitions.map((tr) => tr.operation), ...entity.relationships.map((rel) => rel.target)]);
  const name = sentenceCase(entity.name);
  return (
    <section aria-labelledby={headingId} className="rounded-card border border-line bg-card shadow-sm">
      <div className="flex items-baseline gap-3 border-b border-line px-6 py-5">
        <h2 id={headingId} className="m-0 font-display text-title-lg font-semibold tracking-display text-fg">
          {name}
        </h2>
        <span className="text-footnote text-fg-3">{entityMeta(entity)}</span>
      </div>
      {entity.attributes.length === 0 ? (
        <p className="m-0 px-6 py-5 text-footnote text-fg-3">{t("model.entities.noAttributes")}</p>
      ) : (
        <table className="w-full border-collapse text-footnote">
          <caption className="sr-only">{t("model.entities.attributesCaption")}</caption>
          <thead>
            <tr>
              {(["model.entities.attribute", "model.entities.typeUnit", "model.entities.nullable", "model.entities.dataClass"] as const).map((key, index) => (
                <th
                  key={key}
                  scope="col"
                  className={cn(
                    "py-2.5 text-left text-eyebrow font-semibold uppercase tracking-eyebrow text-fg-3",
                    index === 0 ? "pl-[134px] pr-4" : "pr-4",
                    index === 2 && "w-[90px]",
                    index === 3 && "w-[120px] pr-6",
                  )}
                >
                  {t(key)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {entity.attributes.map((attribute) => (
              <AttributeRow key={attribute.id} attribute={attribute} etag={etag} />
            ))}
          </tbody>
        </table>
      )}
      {entity.relationships.length > 0 && (
        <div className="border-t border-line px-6 py-5">
          <SectionLabel className="mb-3">{t("model.entities.relationships")}</SectionLabel>
          <ul className="m-0 flex list-none flex-col gap-2 p-0 text-footnote">
            {entity.relationships.map((rel) => (
              <li key={rel.id} className="flex items-center gap-2">
                <span aria-hidden="true" className="text-fg-3">
                  {ARROW}
                </span>
                <span className="font-medium text-fg">{label(rel.target)}</span>
                <span className="text-fg-3">{rel.kind.replace(/_/g, " ")}</span>
                {rel.cardinality !== null && rel.cardinality !== undefined && <span className="font-mono text-caption text-fg-3">{rel.cardinality}</span>}
              </li>
            ))}
          </ul>
        </div>
      )}
      {entity.states.length > 0 && (
        <div className="border-t border-line px-6 pb-6 pt-5">
          <SectionLabel className="mb-4">{t("model.entities.statesTitle")}</SectionLabel>
          {label.ready ? (
            <>
              <StateDiagram entityName={name} states={entity.states} transitions={entity.transitions} label={label} />
              <ul aria-label={t("model.entities.transitions")} className="sr-only">
                {entity.transitions.map((tr) => (
                  <li key={tr.from + tr.to + (tr.operation ?? "")}>
                    {tr.operation === null || tr.operation === undefined
                      ? tf("model.entities.transition", { from: label(tr.from), to: label(tr.to) })
                      : tf("model.entities.transitionVia", { from: label(tr.from), to: label(tr.to), operation: label(tr.operation) })}
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <SkeletonGroup>
              <Skeleton height={120} radius="card" />
            </SkeletonGroup>
          )}
        </div>
      )}
    </section>
  );
}

function typeText(attribute: EntityAttribute): string {
  return attribute.unit === null || attribute.unit === undefined || attribute.unit === "" ? attribute.type : attribute.type + " · " + attribute.unit;
}

function AttributeRow({ attribute, etag }: { attribute: EntityAttribute; etag: string | null }) {
  const canEdit = useCanEdit();
  const queryClient = useQueryClient();
  const [unitOpen, setUnitOpen] = useState(false);
  const done = (message: string) => () => {
    void queryClient.invalidateQueries({ queryKey: ["model"] });
    toast({ message });
  };
  const setType = useVersionedMutation<WriteResult, string>(
    (type, ifMatch) => apiClient.POST("/api/model/attributes/{id}/type", { params: { path: { id: attribute.id }, header: { "If-Match": ifMatch } }, body: { type } }),
    etag,
  );
  const setClass = useVersionedMutation<WriteResult, string>(
    (value, ifMatch) => apiClient.POST("/api/model/attributes/{id}/class", { params: { path: { id: attribute.id }, header: { "If-Match": ifMatch } }, body: { class: value } }),
    etag,
  );
  const type = typeText(attribute);
  const chip = "inline-flex h-[26px] items-center gap-1 rounded-[6px] border border-line bg-surface px-2.5 font-mono text-caption font-medium text-fg";
  return (
    <tr data-attribute={attribute.name} className="border-t border-line">
      <th scope="row" className="py-2.5 pl-[134px] pr-4 text-left font-medium text-fg">
        {attribute.name}
      </th>
      <td className="py-2.5 pr-4">
        {canEdit ? (
          <Menu
            width={220}
            title={t("model.entities.typeMenu")}
            align="start"
            trigger={
              <button type="button" aria-label={tf("model.entities.typeButton", { type })} className={cn(chip, "cursor-pointer hover:border-edge")}>
                {type}
                <ChevronDown size={12} strokeWidth={1.6} aria-hidden="true" />
              </button>
            }
            items={[
              ...PRIMITIVE_TYPES.filter((option) => option !== attribute.type).map((option) => ({
                id: option,
                label: option,
                onSelect: () => setType.mutate(option, { onSuccess: done(tf("model.entities.typeSet", { name: attribute.name, type: option })) }),
              })),
              { id: "unit", label: t("model.entities.setUnit"), onSelect: () => setUnitOpen(true) },
            ]}
          />
        ) : (
          <span className={chip}>{type}</span>
        )}
        <WriteError error={setType.error} className="mt-1" />
        {unitOpen && <UnitSheet attribute={attribute} etag={etag} onClose={() => setUnitOpen(false)} />}
      </td>
      <td className="py-2.5 pr-4 text-fg-3">{t(attribute.nullable ? "model.entities.isNullable" : "model.entities.isRequired")}</td>
      <td className="py-2.5 pr-6">
        {attribute.class !== null && attribute.class !== undefined && attribute.class !== "" ? (
          <span className="inline-flex h-[22px] items-center rounded-tag bg-attention-tint px-2 font-mono text-eyebrow font-semibold text-attention-fg">{attribute.class}</span>
        ) : (
          canEdit && (
            <Menu
              width={180}
              title={t("model.entities.classMenu")}
              align="start"
              trigger={
                <button
                  type="button"
                  aria-label={tf("model.entities.setClass", { name: attribute.name })}
                  className="inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-tag px-1.5 text-eyebrow font-medium text-fg-3 hover:bg-surface hover:text-fg"
                >
                  <Plus size={12} strokeWidth={1.6} aria-hidden="true" />
                  {t("model.entities.classAdd")}
                </button>
              }
              items={DATA_CLASSES.map((value) => ({
                id: value,
                label: value,
                onSelect: () => setClass.mutate(value, { onSuccess: done(tf("model.entities.classSet", { name: attribute.name, value })) }),
              }))}
            />
          )
        )}
        <WriteError error={setClass.error} className="mt-1" />
      </td>
    </tr>
  );
}

function UnitSheet({ attribute, etag, onClose }: { attribute: EntityAttribute; etag: string | null; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [unit, setUnit] = useState(attribute.unit ?? "");
  const fieldId = useId();
  const save = useVersionedMutation<WriteResult, string>(
    (value, ifMatch) => apiClient.POST("/api/model/attributes/{id}/unit", { params: { path: { id: attribute.id }, header: { "If-Match": ifMatch } }, body: { unit: value } }),
    etag,
  );
  return (
    <Sheet
      open
      onOpenChange={(open) => !open && onClose()}
      title={tf("model.entities.unitTitle", { name: attribute.name })}
      description={t("model.entities.unitDescription")}
      confirmLabel={t("model.save")}
      confirmDisabled={unit.trim() === ""}
      pending={save.isPending}
      onConfirm={() =>
        save.mutate(unit.trim(), {
          onSuccess: () => {
            void queryClient.invalidateQueries({ queryKey: ["model"] });
            toast({ message: tf("model.entities.unitSet", { name: attribute.name, unit: unit.trim() }) });
            onClose();
          },
        })
      }
    >
      <label htmlFor={fieldId} className="text-caption font-semibold text-fg-2">
        {t("model.entities.unit")}
      </label>
      <input
        id={fieldId}
        value={unit}
        onChange={(event) => setUnit(event.target.value)}
        className="h-[38px] w-full rounded-field border border-line bg-card px-3 font-mono text-callout text-fg outline-none focus:border-accent focus:shadow-focus"
      />
      <WriteError error={save.error} />
    </Sheet>
  );
}
