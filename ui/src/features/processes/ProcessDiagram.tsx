import ELK, { type ElkExtendedEdge, type ElkNode } from "elkjs/lib/elk.bundled.js";
import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";

import type { components } from "../../api/schema";
import { t } from "../../i18n";

export type ProcessNodeView = components["schemas"]["ProcessNodeView"];
export type ProcessFlowView = components["schemas"]["ProcessFlowView"];

/** The semantic input of the diagram (Hotfix 079): never `steps`, coordinates or names. */
export interface ProcessDiagramInput {
  readonly nodes: readonly ProcessNodeView[];
  readonly flows: readonly ProcessFlowView[];
}

/** One visual swimlane: a performer ref, or the view-only Unassigned bucket (no semantic ID). */
export type Lane = { readonly kind: "performer"; readonly ref: string } | { readonly kind: "unassigned" };

const NODE_WIDTH = 180;
const NODE_HEIGHT = 48;
const LANE_HEIGHT = 96;
const LANE_LABEL_WIDTH = 160;
const MARGIN = 24;

const LAYOUT_OPTIONS: Readonly<Record<string, string>> = {
  "elk.algorithm": "layered",
  "elk.direction": "RIGHT",
  "elk.spacing.nodeNode": "30",
  "elk.layered.spacing.nodeNodeBetweenLayers": "60",
  "elk.randomSeed": "1",
};

const byText = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** Flows canonically ordered by `from`, then `to`. */
function sortedFlows(flows: readonly ProcessFlowView[]): ProcessFlowView[] {
  return [...flows].sort((a, b) => byText(a.from, b.from) || byText(a.to, b.to));
}

/**
 * The canonical ELK request of a process: nodes sorted by ID with fixed sizes, flows sorted by
 * `from` then `to`, fixed layered options and seed. Equal semantic input gives an equal request.
 */
export function processLayoutRequest(input: ProcessDiagramInput): ElkNode {
  const children: ElkNode[] = [...input.nodes]
    .sort((a, b) => byText(a.id, b.id))
    .map((node) => ({ id: node.id, width: NODE_WIDTH, height: NODE_HEIGHT }));
  const edges: ElkExtendedEdge[] = sortedFlows(input.flows).map((flow) => ({
    id: flow.from + "->" + flow.to,
    sources: [flow.from],
    targets: [flow.to],
  }));
  return { id: "process", layoutOptions: { ...LAYOUT_OPTIONS }, children, edges };
}

/** The swimlanes: sorted unique performer refs, then Unassigned when a node has no performer. */
export function processLanes(nodes: readonly ProcessNodeView[]): Lane[] {
  const refs = [...new Set(nodes.flatMap((node) => node.performer_refs))].sort(byText);
  const lanes: Lane[] = refs.map((ref) => ({ kind: "performer", ref }));
  if (nodes.some((node) => node.performer_refs.length === 0)) {
    lanes.push({ kind: "unassigned" });
  }
  return lanes;
}

/** The view-only lane index a node is drawn in: its first sorted performer, or Unassigned. */
function laneIndex(lanes: readonly Lane[], node: ProcessNodeView): number {
  const primary = [...node.performer_refs].sort(byText)[0];
  const index = lanes.findIndex((lane) => (primary === undefined ? lane.kind === "unassigned" : lane.kind === "performer" && lane.ref === primary));
  return Math.max(index, 0);
}

interface Point {
  readonly x: number;
  readonly y: number;
}

/** The serialized SVG of the rendered diagram (client-side view export only). */
export function serializeDiagram(svg: SVGSVGElement): string {
  return new XMLSerializer().serializeToString(svg);
}

/**
 * The U2.4 swimlane diagram: ELK lays out the flow direction from nodes + flows; lanes come from
 * performer_refs. Dragging a node only moves it in view state and never reaches the server.
 */
export function ProcessDiagram({ input }: { input: ProcessDiagramInput }) {
  const [layout, setLayout] = useState<Readonly<Record<string, number>> | null>(null);
  const [offsets, setOffsets] = useState<Readonly<Record<string, Point>>>({});
  const drag = useRef<{ id: string; start: Point; origin: Point } | null>(null);
  const svgRef = useRef<SVGSVGElement>(null);

  useEffect(() => {
    let cancelled = false;
    void new ELK().layout(processLayoutRequest(input)).then((result) => {
      if (cancelled) {
        return;
      }
      const xs: Record<string, number> = {};
      for (const child of result.children ?? []) {
        xs[child.id] = child.x ?? 0;
      }
      setLayout(xs);
    });
    return () => {
      cancelled = true;
    };
  }, [input]);

  const lanes = processLanes(input.nodes);
  const nodes = [...input.nodes].sort((a, b) => byText(a.id, b.id));
  const position = (node: ProcessNodeView): Point => {
    const offset = offsets[node.id] ?? { x: 0, y: 0 };
    const x = LANE_LABEL_WIDTH + MARGIN + (layout?.[node.id] ?? 0) + offset.x;
    const y = laneIndex(lanes, node) * LANE_HEIGHT + (LANE_HEIGHT - NODE_HEIGHT) / 2 + offset.y;
    return { x, y };
  };
  const widest = Math.max(0, ...Object.values(layout ?? {}));
  const width = LANE_LABEL_WIDTH + MARGIN * 2 + widest + NODE_WIDTH;
  const height = Math.max(lanes.length, 1) * LANE_HEIGHT;

  const onMouseDown = (id: string) => (event: ReactMouseEvent) => {
    event.preventDefault();
    drag.current = { id, start: { x: event.clientX, y: event.clientY }, origin: offsets[id] ?? { x: 0, y: 0 } };
  };
  const onMouseMove = (event: ReactMouseEvent) => {
    const current = drag.current;
    if (current === null) {
      return;
    }
    const next = { x: current.origin.x + event.clientX - current.start.x, y: current.origin.y + event.clientY - current.start.y };
    setOffsets((all) => ({ ...all, [current.id]: next }));
  };
  const endDrag = () => {
    drag.current = null;
  };

  const exportSvg = () => {
    if (svgRef.current === null) {
      return;
    }
    const blob = new Blob([serializeDiagram(svgRef.current)], { type: "image/svg+xml" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = "process.svg";
    anchor.click();
    URL.revokeObjectURL(url);
  };

  if (layout === null) {
    return <p>{t("processes.layingOut")}</p>;
  }

  return (
    <section aria-label={t("processes.diagram")} className="space-y-2" data-testid="process-diagram">
      <div className="flex gap-2">
        <button type="button" className="rounded border px-3 py-1" data-testid="export-svg" onClick={exportSvg}>
          {t("processes.exportSvg")}
        </button>
        <span className="text-sm text-slate-600">{t("processes.dragHint")}</span>
      </div>
      <svg
        ref={svgRef}
        xmlns="http://www.w3.org/2000/svg"
        width={width}
        height={height}
        role="img"
        aria-label={t("processes.diagram")}
        onMouseMove={onMouseMove}
        onMouseUp={endDrag}
        onMouseLeave={endDrag}
        className="rounded border bg-white"
      >
        {lanes.map((lane, index) => (
          <g key={lane.kind === "performer" ? lane.ref : "unassigned"} data-lane={lane.kind === "performer" ? lane.ref : "unassigned"}>
            <rect x={0} y={index * LANE_HEIGHT} width={width} height={LANE_HEIGHT} fill={index % 2 === 0 ? "#f8fafc" : "#ffffff"} stroke="#cbd5e1" />
            <text x={8} y={index * LANE_HEIGHT + 20} fontSize={12} className="font-mono">
              {lane.kind === "performer" ? lane.ref : t("processes.unassigned")}
            </text>
          </g>
        ))}
        {sortedFlows(input.flows).map((flow) => {
          const from = nodes.find((n) => n.id === flow.from);
          const to = nodes.find((n) => n.id === flow.to);
          if (from === undefined || to === undefined) {
            return null;
          }
          const a = position(from);
          const b = position(to);
          return (
            <line
              key={flow.from + "->" + flow.to}
              data-flow={flow.from + "->" + flow.to}
              x1={a.x + NODE_WIDTH}
              y1={a.y + NODE_HEIGHT / 2}
              x2={b.x}
              y2={b.y + NODE_HEIGHT / 2}
              stroke="#334155"
              markerEnd="url(#arrow)"
            />
          );
        })}
        <defs>
          <marker id="arrow" viewBox="0 0 10 10" refX={10} refY={5} markerWidth={6} markerHeight={6} orient="auto">
            <path d="M 0 0 L 10 5 L 0 10 z" fill="#334155" />
          </marker>
        </defs>
        {nodes.map((node) => {
          const p = position(node);
          return (
            <g
              key={node.id}
              data-node={node.id}
              data-kind={node.kind}
              transform={"translate(" + String(p.x) + " " + String(p.y) + ")"}
              onMouseDown={onMouseDown(node.id)}
              className="cursor-move"
            >
              <rect width={NODE_WIDTH} height={NODE_HEIGHT} rx={node.kind.endsWith("_event") || node.kind === "start" || node.kind === "end" ? 24 : 4} fill="#ffffff" stroke="#0f172a" />
              <text x={8} y={20} fontSize={11} className="font-mono">
                {node.id}
              </text>
              <text x={8} y={36} fontSize={11}>
                {node.kind}
              </text>
            </g>
          );
        })}
      </svg>
      <ul className="grid gap-2 text-sm md:grid-cols-2" aria-label={t("processes.nodes")}>
        {nodes.map((node) => (
          <li key={node.id} className="rounded border p-2" data-node-detail={node.id}>
            <p className="font-mono font-semibold">{node.id}</p>
            <dl className="grid grid-cols-[auto_1fr] gap-x-2">
              <dt>{t("processes.kind")}</dt>
              <dd className="font-mono" data-field="kind">
                {node.kind}
              </dd>
              {node.operation_ref !== null && (
                <>
                  <dt>{t("processes.operation")}</dt>
                  <dd className="font-mono" data-field="operation_ref">
                    {node.operation_ref}
                  </dd>
                </>
              )}
              <dt>{t("processes.performers")}</dt>
              <dd className="font-mono" data-field="performer_refs">
                {node.performer_refs.length === 0 ? t("processes.unassigned") : node.performer_refs.join(", ")}
              </dd>
              {node.performer_refs.length > 1 && (
                <dd className="col-span-2 text-xs text-slate-600" data-testid="multi-performer-note">
                  {t("processes.multiPerformer")}
                </dd>
              )}
              <dt>{t("processes.consumes")}</dt>
              <dd className="font-mono" data-field="consumes_refs">
                {node.consumes_refs.length === 0 ? t("processes.none") : node.consumes_refs.join(", ")}
              </dd>
              <dt>{t("processes.produces")}</dt>
              <dd className="font-mono" data-field="produces_refs">
                {node.produces_refs.length === 0 ? t("processes.none") : node.produces_refs.join(", ")}
              </dd>
              {node.message_ref !== null && (
                <>
                  <dt>{t("processes.message")}</dt>
                  <dd className="font-mono" data-field="message_ref">
                    {node.message_ref}
                  </dd>
                </>
              )}
              {node.timer_expr !== null && (
                <>
                  <dt>{t("processes.timer")}</dt>
                  <dd className="font-mono" data-field="timer_expr">
                    {node.timer_expr}
                  </dd>
                </>
              )}
              {node.condition_expr !== null && (
                <>
                  <dt>{t("processes.condition")}</dt>
                  <dd className="font-mono" data-field="condition_expr">
                    {node.condition_expr}
                  </dd>
                </>
              )}
            </dl>
          </li>
        ))}
      </ul>
    </section>
  );
}
