import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { ArrowDown, ArrowUp, X } from "lucide-react";
import { useEffect, useId, useState, type MouseEvent } from "react";

import { apiClient, readReviewed, type Reviewed } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation } from "../../api/useVersionedMutation";
import { buildHref, useGo } from "../../app/shell/navigation";
import { Button, cn, EmptyState, Eyebrow, IconButton, isPlainClick, Sheet, Skeleton, SkeletonGroup, StatusPill, toast } from "../../components/ui";
import { t, tc, tf, type MessageKey } from "../../i18n";
import { crudQuery } from "./CoverageSheet";
import { useLabels, type Labels } from "./labels";
import { isTask, LANE_HEIGHT, LANE_LABEL, layoutProcess, linearSteps, type ProcessLayout } from "./processLayout";
import { ReadError, SegmentBody, useCanEdit, WriteError } from "./shared";

type Process = components["schemas"]["Process"];
type ProcessNode = components["schemas"]["ProcessNodeView"];
type ProcessStepRequest = components["schemas"]["ProcessStepRequest"];
type WriteResult = components["schemas"]["WriteResult"];

export const processesQuery = {
  queryKey: ["model", "processes"] as const,
  queryFn: () => readReviewed(apiClient.GET("/api/processes")),
};

const DOT = "·";

const KIND: Readonly<Record<ProcessNode["kind"], MessageKey>> = {
  start: "model.processes.kind.start",
  end: "model.processes.kind.end",
  human_task: "model.processes.kind.human_task",
  service_task: "model.processes.kind.service_task",
  exclusive_gateway: "model.processes.kind.exclusive_gateway",
  parallel_split: "model.processes.kind.parallel_split",
  parallel_join: "model.processes.kind.parallel_join",
  message_event: "model.processes.kind.message_event",
  timer_event: "model.processes.kind.timer_event",
  error_event: "model.processes.kind.error_event",
  subprocess: "model.processes.kind.subprocess",
};

/** The node's name: its operation, else its kind ("Start event", "Gateway"). */
function nodeName(node: ProcessNode, label: Labels): string {
  return node.operation_ref === null ? t(KIND[node.kind]) : label(node.operation_ref);
}

/** Model · Processes (spec §5.4.5): the swimlane canvas, the node panel and the step edits. */
export function ProcessesView() {
  const params: { id?: string } = useParams({ strict: false });
  const go = useGo();
  const canEdit = useCanEdit();
  const list = useQuery(processesQuery);
  const [importing, setImporting] = useState(false);
  const items = list.data?.items ?? [];
  const selectedId = items.find((item) => item.id === params.id)?.id ?? items[0]?.id ?? null;
  const open = (event: MouseEvent<HTMLAnchorElement>, href: string) => {
    if (isPlainClick(event)) {
      event.preventDefault();
      go(href);
    }
  };
  const importSheet = importing && <ImportSheet etag={list.data?.etag ?? null} onClose={() => setImporting(false)} />;
  if (list.isError) {
    return (
      <SegmentBody>
        <ReadError error={list.error} messageKey="model.processes.loadFailed" onRetry={() => void list.refetch()} />
      </SegmentBody>
    );
  }
  if (list.isPending) {
    return (
      <SegmentBody>
        <SkeletonGroup className="grid grid-cols-[minmax(0,1fr)_280px] gap-5">
          <Skeleton height={420} radius="card" />
          <Skeleton height={420} radius="card" />
        </SkeletonGroup>
      </SegmentBody>
    );
  }
  if (selectedId === null) {
    return (
      <SegmentBody>
        <EmptyState
          title={t("model.processes.empty.title")}
          description={t("model.processes.empty.body")}
          action={
            canEdit ? (
              <Button variant="outline" size={38} onClick={() => setImporting(true)}>
                {t("model.processes.importFull")}
              </Button>
            ) : undefined
          }
        />
        {importSheet}
      </SegmentBody>
    );
  }
  return (
    <SegmentBody className="flex flex-col gap-3">
      {items.length > 1 && (
        <nav aria-label={t("nav.processes")} className="flex flex-wrap gap-2">
          {items.map((item) => {
            const href = buildHref("/model/processes/$id", { id: item.id });
            return (
              <a
                key={item.id}
                href={href}
                onClick={(event) => open(event, href)}
                aria-current={item.id === selectedId ? "page" : undefined}
                className={cn(
                  "inline-flex h-[30px] items-center rounded-pill border px-3 text-footnote no-underline",
                  item.id === selectedId ? "border-primary bg-surface font-semibold text-fg" : "border-line bg-card font-medium text-fg-2 hover:bg-surface",
                )}
              >
                {item.name}
              </a>
            );
          })}
        </nav>
      )}
      <ProcessDetail key={selectedId} id={selectedId} onImport={() => setImporting(true)} />
      {importSheet}
    </SegmentBody>
  );
}

function ProcessDetail({ id, onImport }: { id: string; onImport: () => void }) {
  const canEdit = useCanEdit();
  const headingId = useId();
  const query = useQuery({ queryKey: ["model", "process", id], queryFn: () => readReviewed(apiClient.GET("/api/processes/{id}", { params: { path: { id } } })) });
  const process = query.data;
  const label = useLabels(process === undefined ? [] : [...process.lanes, ...process.nodes.flatMap((node) => [node.operation_ref, ...node.performer_refs, ...node.produces_refs])]);
  const [layout, setLayout] = useState<ProcessLayout | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  useEffect(() => {
    if (process === undefined || !label.ready) {
      return undefined;
    }
    let cancelled = false;
    void layoutProcess(process, (node) => nodeName(node, label)).then((next) => {
      if (!cancelled) {
        setLayout(next);
      }
    });
    return () => {
      cancelled = true;
    };
    // The layout depends on the process data and the resolved names only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [process, label.ready]);
  if (query.isError) {
    return <ReadError error={query.error} messageKey="model.processes.loadFailed" onRetry={() => void query.refetch()} />;
  }
  if (process === undefined) {
    return (
      <SkeletonGroup className="grid grid-cols-[minmax(0,1fr)_280px] gap-5">
        <Skeleton height={420} radius="card" />
        <Skeleton height={420} radius="card" />
      </SkeletonGroup>
    );
  }
  const steps = linearSteps(process);
  const current = process.nodes.find((node) => node.id === selected) ?? steps[0] ?? process.nodes[0] ?? null;
  return (
    <div className="grid grid-cols-[minmax(0,1fr)_280px] items-start gap-5">
      <section aria-labelledby={headingId} className="min-w-0 overflow-hidden rounded-card border border-line bg-card shadow-sm">
        <div className="flex items-center gap-2.5 border-b border-line px-5 py-4">
          <h2 id={headingId} className="m-0 text-body font-semibold text-fg">
            {process.name}
          </h2>
          {process.validation.length === 0 ? (
            <StatusPill status="accepted" label={t("model.processes.valid")} />
          ) : (
            <StatusPill status="warning" label={tc("model.processes.issues", process.validation.length)} />
          )}
          <div className="flex-1" />
          {canEdit && (
            <button type="button" onClick={onImport} className="cursor-pointer text-caption text-fg-3 hover:text-fg">
              {t("model.processes.import")}
            </button>
          )}
          {canEdit && (
            <span aria-hidden="true" className="text-caption text-fg-3">
              {DOT}
            </span>
          )}
          <ExportButton process={process} />
        </div>
        {process.validation.length > 0 && (
          <ul className="m-0 list-none border-b border-line bg-attention-tint/40 px-5 py-2.5 text-caption text-attention-fg">
            {process.validation.map((issue) => (
              <li key={issue} className="font-mono">
                {issue}
              </li>
            ))}
          </ul>
        )}
        <div className="overflow-x-auto">
          {layout === null ? (
            <SkeletonGroup className="p-5">
              <Skeleton height={LANE_HEIGHT * Math.max(1, process.lanes.length) - 40} radius="card" />
            </SkeletonGroup>
          ) : (
            <Canvas layout={layout} label={label} selected={current?.id ?? null} onSelect={setSelected} />
          )}
        </div>
      </section>
      <aside aria-label={t("model.processes.details")} className="flex flex-col gap-3 rounded-card border border-line bg-card p-5">
        {current !== null && (
          <>
            <Eyebrow>{t(KIND[current.kind])}</Eyebrow>
            <h3 className="m-0 font-display text-title-sm font-semibold leading-snug text-fg">{nodeName(current, label)}</h3>
            <dl className="m-0 grid grid-cols-[90px_1fr] gap-y-2 text-footnote">
              <dt className="text-fg-3">{t("model.processes.performer")}</dt>
              <dd className="m-0 text-fg">{current.performer_refs.length === 0 ? "—" : current.performer_refs.map(label).join(", ")}</dd>
              <dt className="text-fg-3">{t("model.processes.operation")}</dt>
              <dd className="m-0 text-fg">{current.operation_ref === null ? "—" : label(current.operation_ref)}</dd>
              <dt className="text-fg-3">{t("model.processes.produces")}</dt>
              <dd className="m-0 text-fg">{current.produces_refs.length === 0 ? "—" : current.produces_refs.map(label).join(", ")}</dd>
              {current.condition_expr !== null && (
                <>
                  <dt className="text-fg-3">{t("model.processes.condition")}</dt>
                  <dd className="m-0 font-mono text-caption text-fg">{current.condition_expr}</dd>
                </>
              )}
            </dl>
          </>
        )}
        <div className="h-px bg-line" />
        <h3 id={headingId + "-steps"} className="m-0 text-caption font-semibold text-fg-2">
          {t("model.processes.steps")}
        </h3>
        <ol aria-labelledby={headingId + "-steps"} className="m-0 flex list-none flex-col gap-2 p-0">
          {steps.map((node, index) => (
            <li key={node.id} className="flex items-center gap-2 text-caption text-fg-2">
              <span className="w-4 font-mono text-eyebrow font-medium text-fg-3">{index + 1}</span>
              {nodeName(node, label)}
            </li>
          ))}
        </ol>
        {canEdit && (
          <Button variant="outline" size={30} onClick={() => setEditing(true)}>
            {t("model.processes.editSteps")}
          </Button>
        )}
        {editing && <EditStepsSheet process={process} steps={steps} label={label} onClose={() => setEditing(false)} />}
      </aside>
    </div>
  );
}

function Canvas({ layout, label, selected, onSelect }: { layout: ProcessLayout; label: Labels; selected: string | null; onSelect: (id: string) => void }) {
  const marker = useId().replace(/:/g, "");
  return (
    <div className="relative" style={{ width: layout.width, height: layout.height }}>
      <ul aria-label={t("model.processes.lanes")} className="m-0 list-none p-0">
        {layout.lanes.map((lane, index) => (
          <li
            key={lane === "" ? "unassigned" : lane}
            className={cn("absolute inset-x-0 border-b border-line", index % 2 === 1 ? "bg-subtle-row" : "bg-card")}
            style={{ top: index * LANE_HEIGHT, height: LANE_HEIGHT }}
          >
            <span className="absolute inset-y-0 left-0 flex items-center justify-center border-r border-line text-caption font-semibold text-fg-2" style={{ width: LANE_LABEL }}>
              {lane === "" ? t("model.processes.unassigned") : label(lane)}
            </span>
          </li>
        ))}
      </ul>
      <svg aria-hidden="true" width={layout.width} height={layout.height} className="pointer-events-none absolute left-0 top-0">
        <defs>
          <marker id={marker} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M0 0 10 5 0 10z" className="fill-edge" />
          </marker>
        </defs>
        {layout.flows.map((flow) => (
          <path key={flow.from + flow.to} data-flow={flow.from + "->" + flow.to} d={flow.path} fill="none" strokeWidth={1.2} className="stroke-edge" markerEnd={"url(#" + marker + ")"} />
        ))}
      </svg>
      {layout.nodes.map(({ node, x, y, width, height }) => {
        const isSelected = node.id === selected;
        const round = node.kind === "start" || node.kind === "end" || node.kind.endsWith("_event");
        const gateway = !round && !isTask(node);
        const name = node.operation_ref === null ? t(KIND[node.kind]) : tf("model.processes.nodeLabel", { kind: t(KIND[node.kind]), name: label(node.operation_ref) });
        return (
          <button
            key={node.id}
            type="button"
            aria-label={name}
            aria-pressed={isSelected}
            onClick={() => onSelect(node.id)}
            style={{ left: x, top: y, width, height }}
            className={cn(
              "absolute flex cursor-pointer items-center justify-center px-2 text-caption font-medium leading-tight text-fg",
              round ? "rounded-pill bg-card" : gateway ? "rotate-45 rounded-tag bg-card" : "rounded-[10px]",
              node.kind === "service_task" ? "bg-surface" : !round && !gateway && "bg-card",
              node.kind === "end" ? "border-[3px] border-primary" : isSelected ? "border-[1.5px] border-primary" : "border border-line-strong",
              isSelected && "shadow-[0_0_0_3px_var(--muted-ring)]",
            )}
          >
            {!round && !gateway && <span aria-hidden="true">{label(node.operation_ref ?? "")}</span>}
          </button>
        );
      })}
    </div>
  );
}

function ExportButton({ process }: { process: Process }) {
  const [error, setError] = useState(false);
  const exportText = async () => {
    setError(false);
    const { data, response } = await apiClient.GET("/api/processes/{id}/export", { params: { path: { id: process.id }, query: { format: "mermaid" } }, parseAs: "text" });
    if (!response.ok || data === undefined) {
      setError(true);
      return;
    }
    const url = URL.createObjectURL(new Blob([data], { type: "text/plain" }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = process.name.replace(/[^\p{L}\p{N}]+/gu, "-").toLowerCase() + ".mmd";
    anchor.click();
    URL.revokeObjectURL(url);
    toast({ message: t("model.processes.exported") });
  };
  return (
    <>
      <button type="button" onClick={() => void exportText()} className="cursor-pointer text-caption text-fg-3 hover:text-fg">
        {t("model.processes.export")}
      </button>
      {error && (
        <span role="alert" className="text-caption text-blocked-fg">
          {t("model.processes.exportFailed")}
        </span>
      )}
    </>
  );
}

function ImportSheet({ etag, onClose }: { etag: string | null; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [text, setText] = useState("");
  const fieldId = useId();
  const write = useVersionedMutation<WriteResult, string>(
    (body, ifMatch) => apiClient.POST("/api/processes/import", { params: { header: { "If-Match": ifMatch } }, body: { format: "mermaid", text: body } }),
    etag,
  );
  return (
    <Sheet
      open
      onOpenChange={(open) => !open && onClose()}
      title={t("model.processes.importTitle")}
      description={t("model.processes.importDescription")}
      confirmLabel={t("model.processes.importConfirm")}
      confirmDisabled={text.trim() === ""}
      pending={write.isPending}
      onConfirm={() =>
        write.mutate(text, {
          onSuccess: () => {
            void queryClient.invalidateQueries({ queryKey: ["model"] });
            toast({ message: t("model.processes.imported") });
            onClose();
          },
        })
      }
    >
      <label htmlFor={fieldId} className="text-caption font-semibold text-fg-2">
        {t("model.processes.mermaid")}
      </label>
      <textarea
        id={fieldId}
        value={text}
        onChange={(event) => setText(event.target.value)}
        spellCheck={false}
        className="min-h-48 w-full resize-y rounded-field border border-line bg-card p-3 font-mono text-caption text-fg outline-none focus:border-accent focus:shadow-focus"
      />
      <WriteError error={write.error} />
    </Sheet>
  );
}

function EditStepsSheet({ process, steps, label, onClose }: { process: Reviewed<Process>; steps: readonly ProcessNode[]; label: Labels; onClose: () => void }) {
  const queryClient = useQueryClient();
  const crud = useQuery(crudQuery);
  const operations = crud.data?.operations ?? [];
  const opLabel = useLabels(operations);
  const [operation, setOperation] = useState("");
  const fieldId = useId();
  const write = useVersionedMutation<WriteResult, ProcessStepRequest>(
    (body, ifMatch) => apiClient.POST("/api/processes/{id}/steps", { params: { path: { id: process.id }, header: { "If-Match": ifMatch } }, body }),
    process.etag,
  );
  const run = (body: ProcessStepRequest, message: string) =>
    write.mutate(body, {
      onSuccess: () => {
        void queryClient.invalidateQueries({ queryKey: ["model"] });
        toast({ message });
      },
    });
  return (
    <Sheet open onOpenChange={(open) => !open && onClose()} title={t("model.processes.editSteps")} description={t("model.processes.editDescription")} cancelLabel={t("model.close")}>
      <ol aria-label={t("model.processes.steps")} className="m-0 flex list-none flex-col rounded-tile border border-line p-0">
        {steps.map((node, index) => {
          const name = nodeName(node, label);
          return (
            <li key={node.id} className="flex items-center gap-2 border-b border-line px-3.5 py-2 text-footnote last:border-b-0">
              <span className="w-4 font-mono text-eyebrow text-fg-3">{index + 1}</span>
              <span className="min-w-0 flex-1 truncate font-medium text-fg">{name}</span>
              <IconButton
                size={30}
                label={tf("model.processes.moveUp", { name })}
                disabled={index === 0 || write.isPending}
                icon={<ArrowUp size={14} strokeWidth={1.6} aria-hidden="true" />}
                onClick={() => run({ op: "move", node_ref: node.id, position: index - 1 }, t("model.processes.moved"))}
              />
              <IconButton
                size={30}
                label={tf("model.processes.moveDown", { name })}
                disabled={index === steps.length - 1 || write.isPending}
                icon={<ArrowDown size={14} strokeWidth={1.6} aria-hidden="true" />}
                onClick={() => run({ op: "move", node_ref: node.id, position: index + 1 }, t("model.processes.moved"))}
              />
              <IconButton
                size={30}
                label={tf("model.processes.remove", { name })}
                disabled={write.isPending}
                icon={<X size={14} strokeWidth={1.6} aria-hidden="true" />}
                onClick={() => run({ op: "remove", node_ref: node.id }, t("model.processes.removed"))}
              />
            </li>
          );
        })}
      </ol>
      <div className="flex items-end gap-2">
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <label htmlFor={fieldId} className="text-caption font-semibold text-fg-2">
            {t("model.processes.operation")}
          </label>
          <select
            id={fieldId}
            value={operation}
            onChange={(event) => setOperation(event.target.value)}
            className="h-[38px] w-full rounded-field border border-line bg-card px-3 text-callout text-fg outline-none focus:border-accent focus:shadow-focus"
          >
            <option value="">{t("model.processes.chooseOperation")}</option>
            {operations.map((op) => (
              <option key={op} value={op}>
                {opLabel(op)}
              </option>
            ))}
          </select>
        </div>
        <Button variant="outline" size={38} disabled={operation === "" || write.isPending} onClick={() => run({ op: "add", operation_ref: operation, position: steps.length }, t("model.processes.added"))}>
          {t("model.processes.addStep")}
        </Button>
      </div>
      <WriteError error={write.error} />
    </Sheet>
  );
}
