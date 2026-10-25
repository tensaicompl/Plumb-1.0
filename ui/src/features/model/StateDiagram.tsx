import { useId } from "react";

import type { components } from "../../api/schema";
import { tf } from "../../i18n";

type Transition = components["schemas"]["EntityTransition"];

const PILL_HEIGHT = 36;
const ROW_GAP = 10;
const CHAR = 7.4;
const LABEL_CHAR = 5.6;
const TRUNK = 14;
const MIN_GAP = 56;
const MARGIN = 2;
const BACK_LANE = 26;

export interface StateNode {
  readonly id: string;
  readonly label: string;
  readonly column: number;
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  /** The initial and branching states carry the midnight outline. */
  readonly emphasis: boolean;
}

export interface StateEdge {
  readonly from: string;
  readonly to: string;
  readonly label: string;
  readonly path: string;
  readonly labelX: number;
  readonly labelY: number;
  /** A transition to an earlier or the same column, routed below the diagram. */
  readonly back: boolean;
}

export interface StateLayout {
  readonly nodes: readonly StateNode[];
  readonly edges: readonly StateEdge[];
  readonly width: number;
  readonly height: number;
}

/**
 * A deterministic left-to-right layout: each state's column is its shortest distance from an
 * initial state (one without incoming transitions), rows follow discovery order, and edges are
 * orthogonal with the operation on the segment entering the target.
 */
export function layoutStates(states: readonly string[], transitions: readonly Transition[], label: (id: string) => string): StateLayout {
  const known = new Set(states);
  const edges = transitions.filter((tr) => known.has(tr.from) && known.has(tr.to));
  const incoming = new Set(edges.filter((tr) => tr.from !== tr.to).map((tr) => tr.to));
  const roots = states.filter((id) => !incoming.has(id));
  const depth = new Map<string, number>();
  const order: string[] = [];
  const queue = (roots.length > 0 ? roots : states.slice(0, 1)).map((id) => ({ id, d: 0 }));
  while (queue.length > 0) {
    const next = queue.shift();
    if (next === undefined || depth.has(next.id)) {
      continue;
    }
    depth.set(next.id, next.d);
    order.push(next.id);
    for (const tr of edges) {
      if (tr.from === next.id && !depth.has(tr.to)) {
        queue.push({ id: tr.to, d: next.d + 1 });
      }
    }
  }
  const maxDepth = Math.max(0, ...depth.values());
  for (const id of states) {
    if (!depth.has(id)) {
      depth.set(id, maxDepth + 1);
      order.push(id);
    }
  }
  const columns: string[][] = [];
  for (const id of order) {
    const d = depth.get(id) ?? 0;
    (columns[d] ??= []).push(id);
  }
  const widthOf = (id: string) => Math.round(label(id).length * CHAR + 30);
  const opLabel = (tr: Transition) => (tr.operation === null || tr.operation === undefined ? "" : label(tr.operation));
  const tallest = Math.max(1, ...columns.map((column) => column.length));
  const span = tallest * PILL_HEIGHT + (tallest - 1) * ROW_GAP;
  const nodes = new Map<string, StateNode>();
  let x = MARGIN;
  columns.forEach((column, index) => {
    const into = edges.filter((tr) => depth.get(tr.to) === index && (depth.get(tr.from) ?? 0) < index);
    const gap = index === 0 ? 0 : Math.max(MIN_GAP, TRUNK + 24 + Math.max(0, ...into.map((tr) => opLabel(tr).length * LABEL_CHAR)));
    x += gap;
    const height = column.length * PILL_HEIGHT + (column.length - 1) * ROW_GAP;
    const top = MARGIN + (span - height) / 2;
    const width = Math.max(...column.map(widthOf));
    column.forEach((id, row) => {
      const outgoing = new Set(edges.filter((tr) => tr.from === id).map((tr) => tr.to));
      nodes.set(id, {
        id,
        label: label(id),
        column: index,
        x,
        y: top + row * (PILL_HEIGHT + ROW_GAP),
        width: widthOf(id),
        height: PILL_HEIGHT,
        emphasis: !incoming.has(id) || outgoing.size > 1,
      });
    });
    x += width;
  });
  const bottom = MARGIN + span;
  let backCount = 0;
  const laid: StateEdge[] = edges.map((tr) => {
    const a = nodes.get(tr.from);
    const b = nodes.get(tr.to);
    if (a === undefined || b === undefined) {
      throw new Error("unreachable: both ends are known states");
    }
    const text = opLabel(tr);
    if (b.column > a.column) {
      const x1 = a.x + a.width;
      const y1 = a.y + PILL_HEIGHT / 2;
      const x2 = b.x;
      const y2 = b.y + PILL_HEIGHT / 2;
      const trunk = x1 + TRUNK;
      return { from: tr.from, to: tr.to, label: text, back: false, path: `M${x1} ${y1} H${trunk} V${y2} H${x2}`, labelX: (trunk + x2) / 2, labelY: y2 - 6 };
    }
    backCount += 1;
    const lane = bottom + BACK_LANE * backCount;
    const x1 = a.x + a.width / 2;
    const x2 = b.x + b.width / 2;
    return {
      from: tr.from,
      to: tr.to,
      label: text,
      back: true,
      path: `M${x1} ${a.y + PILL_HEIGHT} V${lane} H${x2} V${b.y + PILL_HEIGHT}`,
      labelX: (x1 + x2) / 2,
      labelY: lane - 5,
    };
  });
  return { nodes: [...nodes.values()], edges: laid, width: x + MARGIN, height: bottom + BACK_LANE * backCount + MARGIN + (backCount > 0 ? 6 : 0) };
}

/**
 * The entity's state diagram (get_entity states and transitions): states as pills, transitions as
 * orthogonal edges labelled with their operation. The same transitions are listed for screen readers.
 */
export function StateDiagram({ entityName, states, transitions, label }: { entityName: string; states: readonly string[]; transitions: readonly Transition[]; label: (id: string) => string }) {
  const marker = useId().replace(/:/g, "");
  const layout = layoutStates(states, transitions, label);
  return (
    <div className="overflow-x-auto pb-1">
      <svg role="img" aria-label={tf("model.entities.statesOf", { name: entityName })} width={layout.width} height={layout.height} className="block overflow-visible">
        <defs>
          <marker id={marker} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M0 0 10 5 0 10z" className="fill-edge" />
          </marker>
        </defs>
        {layout.edges.map((edge) => (
          <g key={edge.from + edge.to + edge.label}>
            <path d={edge.path} fill="none" strokeWidth={1} className="stroke-edge" markerEnd={"url(#" + marker + ")"} />
            {edge.label !== "" && (
              <text x={edge.labelX} y={edge.labelY} textAnchor="middle" className="fill-fg-3 font-sans text-eyebrow">
                {edge.label}
              </text>
            )}
          </g>
        ))}
        {layout.nodes.map((node) => (
          <g key={node.id} data-state={node.id}>
            <rect x={node.x} y={node.y} width={node.width} height={node.height} rx={node.height / 2} strokeWidth={1} className={node.emphasis ? "fill-card stroke-primary" : "fill-card stroke-line-strong"} />
            <text x={node.x + node.width / 2} y={node.y + node.height / 2 + 4.5} textAnchor="middle" className="fill-fg font-sans text-footnote font-semibold">
              {node.label}
            </text>
          </g>
        ))}
      </svg>
    </div>
  );
}
