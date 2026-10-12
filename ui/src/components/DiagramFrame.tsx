import ELK, { type ElkExtendedEdge, type ElkNode } from "elkjs/lib/elk.bundled.js";
import { useEffect, useMemo, useRef, useState } from "react";

import { t } from "../i18n";

/** One semantic element shown as a node (label is server data). */
export interface DiagramNode {
  readonly id: string;
  readonly label: string;
}

/** One semantic relation shown as an edge. */
export interface DiagramEdge {
  readonly id: string;
  readonly source: string;
  readonly target: string;
  readonly label?: string;
}

/** Laid-out geometry; view data only, never semantic state. */
export interface DiagramLayout {
  readonly width: number;
  readonly height: number;
  readonly nodes: readonly { readonly id: string; readonly x: number; readonly y: number; readonly width: number; readonly height: number }[];
  readonly edges: readonly { readonly id: string; readonly points: readonly { readonly x: number; readonly y: number }[] }[];
}

const NODE_HEIGHT = 40;
const ZOOM_STEP = 1.25;
const PAN_STEP = 40;

/**
 * The canonical ELK request: nodes and edges sorted by ID, fixed sizes derived only from labels
 * and fixed layout options, so equal semantic inputs always produce equal layout requests.
 */
export function layoutRequest(nodes: readonly DiagramNode[], edges: readonly DiagramEdge[]): ElkNode {
  const byId = (a: { id: string }, b: { id: string }) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  const children: ElkNode[] = [...nodes].sort(byId).map((node) => ({
    id: node.id,
    width: 40 + 8 * node.label.length,
    height: NODE_HEIGHT,
  }));
  const elkEdges: ElkExtendedEdge[] = [...edges].sort(byId).map((edge) => ({
    id: edge.id,
    sources: [edge.source],
    targets: [edge.target],
  }));
  return {
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.spacing.nodeNode": "30",
      "elk.layered.spacing.nodeNodeBetweenLayers": "50",
      "elk.randomSeed": "1",
    },
    children,
    edges: elkEdges,
  };
}

/** Runs ELK on the canonical request. */
export async function computeLayout(nodes: readonly DiagramNode[], edges: readonly DiagramEdge[]): Promise<DiagramLayout> {
  const elk = new ELK();
  const result = await elk.layout(layoutRequest(nodes, edges));
  return {
    width: result.width ?? 0,
    height: result.height ?? 0,
    nodes: (result.children ?? []).map((node) => ({
      id: node.id,
      x: node.x ?? 0,
      y: node.y ?? 0,
      width: node.width ?? 0,
      height: node.height ?? 0,
    })),
    edges: (result.edges ?? []).map((edge) => ({
      id: edge.id,
      points: (edge.sections ?? []).flatMap((section) => [section.startPoint, ...(section.bendPoints ?? []), section.endPoint]),
    })),
  };
}

interface ViewState {
  readonly zoom: number;
  readonly panX: number;
  readonly panY: number;
}

const FITTED: ViewState = { zoom: 1, panX: 0, panY: 0 };

/**
 * A deterministic diagram of semantic nodes and edges with zoom, pan, fit, a list-equivalent view
 * and SVG export. Zoom, pan, fit, layout and the view toggle are view state only; the semantic
 * callback runs only when the user explicitly activates a node's action.
 */
export function DiagramFrame({
  title,
  nodes,
  edges,
  onSemanticAction,
  onExportSvg,
}: {
  title: string;
  nodes: readonly DiagramNode[];
  edges: readonly DiagramEdge[];
  onSemanticAction?: (nodeId: string) => void;
  onExportSvg?: (svg: string) => void;
}) {
  const [layout, setLayout] = useState<DiagramLayout | null>(null);
  const [view, setView] = useState<ViewState>(FITTED);
  const [mode, setMode] = useState<"diagram" | "list">("diagram");
  const svgRef = useRef<SVGSVGElement>(null);
  const labels = useMemo(() => new Map(nodes.map((node) => [node.id, node.label])), [nodes]);
  const sortedNodes = useMemo(() => [...nodes].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)), [nodes]);
  const sortedEdges = useMemo(() => [...edges].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)), [edges]);

  useEffect(() => {
    let current = true;
    void computeLayout(nodes, edges).then((next) => {
      if (current) {
        setLayout(next);
      }
    });
    return () => {
      current = false;
    };
  }, [nodes, edges]);

  const zoomBy = (factor: number) => setView((v) => ({ ...v, zoom: v.zoom * factor }));
  const panBy = (dx: number, dy: number) => setView((v) => ({ ...v, panX: v.panX + dx, panY: v.panY + dy }));
  const exportSvg = () => {
    if (svgRef.current !== null && onExportSvg !== undefined) {
      onExportSvg(svgRef.current.outerHTML);
    }
  };

  const viewBox = layout === null ? "0 0 0 0" : ["0", "0", String(Math.max(layout.width, 1)), String(Math.max(layout.height, 1))].join(" ");
  const transform = ["translate(", String(view.panX), " ", String(view.panY), ") scale(", String(view.zoom), ")"].join("");

  return (
    <section aria-label={title} className="rounded border" data-diagram-mode={mode} data-zoom={view.zoom} data-pan={[view.panX, view.panY].join(",")}>
      <div role="toolbar" aria-label={t("diagram.toolbar")} className="flex flex-wrap gap-1 border-b p-1 text-xs">
        <button type="button" onClick={() => zoomBy(ZOOM_STEP)} disabled={mode !== "diagram"}>
          {t("diagram.zoomIn")}
        </button>
        <button type="button" onClick={() => zoomBy(1 / ZOOM_STEP)} disabled={mode !== "diagram"}>
          {t("diagram.zoomOut")}
        </button>
        <button type="button" onClick={() => panBy(-PAN_STEP, 0)} disabled={mode !== "diagram"}>
          {t("diagram.panLeft")}
        </button>
        <button type="button" onClick={() => panBy(PAN_STEP, 0)} disabled={mode !== "diagram"}>
          {t("diagram.panRight")}
        </button>
        <button type="button" onClick={() => panBy(0, -PAN_STEP)} disabled={mode !== "diagram"}>
          {t("diagram.panUp")}
        </button>
        <button type="button" onClick={() => panBy(0, PAN_STEP)} disabled={mode !== "diagram"}>
          {t("diagram.panDown")}
        </button>
        <button type="button" onClick={() => setView(FITTED)} disabled={mode !== "diagram"}>
          {t("diagram.fit")}
        </button>
        <button type="button" aria-pressed={mode === "list"} onClick={() => setMode((m) => (m === "diagram" ? "list" : "diagram"))}>
          {t(mode === "diagram" ? "diagram.showList" : "diagram.showDiagram")}
        </button>
        {onExportSvg !== undefined && (
          <button type="button" onClick={exportSvg} disabled={mode !== "diagram" || layout === null}>
            {t("diagram.exportSvg")}
          </button>
        )}
      </div>

      {mode === "diagram" ? (
        <svg ref={svgRef} role="img" aria-label={title} viewBox={viewBox} xmlns="http://www.w3.org/2000/svg" className="h-96 w-full" data-testid="diagram-svg">
          {layout !== null && (
            <g transform={transform}>
              {layout.edges.map((edge) => (
                <polyline
                  key={edge.id}
                  data-edge-id={edge.id}
                  points={edge.points.map((p) => [p.x, p.y].join(",")).join(" ")}
                  fill="none"
                  stroke="currentColor"
                />
              ))}
              {layout.nodes.map((node) => (
                <g key={node.id} data-node-id={node.id} transform={["translate(", String(node.x), " ", String(node.y), ")"].join("")}>
                  <rect width={node.width} height={node.height} rx={4} fill="white" stroke="currentColor" />
                  <text x={8} y={node.height / 2 + 4} fontSize={12}>
                    {labels.get(node.id)}
                  </text>
                </g>
              ))}
            </g>
          )}
        </svg>
      ) : (
        <div className="p-2 text-sm" data-testid="diagram-list">
          <h3 className="font-semibold">{t("diagram.nodes")}</h3>
          <ul>
            {sortedNodes.map((node) => (
              <li key={node.id} data-node-id={node.id}>
                <span className="font-mono">{node.id}</span> {node.label}
              </li>
            ))}
          </ul>
          <h3 className="mt-2 font-semibold">{t("diagram.edges")}</h3>
          <ul>
            {sortedEdges.map((edge) => (
              <li key={edge.id} data-edge-id={edge.id}>
                <span className="font-mono">{edge.source}</span> {t("diagram.to")} <span className="font-mono">{edge.target}</span>{" "}
                {edge.label}
              </li>
            ))}
          </ul>
        </div>
      )}

      {onSemanticAction !== undefined && (
        <ul aria-label={t("diagram.actions")} className="flex flex-wrap gap-1 border-t p-1 text-xs">
          {sortedNodes.map((node) => (
            <li key={node.id}>
              <button type="button" data-semantic-action={node.id} onClick={() => onSemanticAction(node.id)}>
                {t("diagram.open")} {node.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
