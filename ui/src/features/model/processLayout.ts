// The swimlane layout of a Process (get_process): ELK orders the flow left to right; lanes come
// from the performers. View geometry only, never semantic state. No I/O.
import ELK, { type ElkNode } from "elkjs/lib/elk.bundled.js";

import type { components } from "../../api/schema";

type Process = components["schemas"]["Process"];
type ProcessNode = components["schemas"]["ProcessNodeView"];

export const LANE_HEIGHT = 120;
export const LANE_LABEL = 96;
const PAD = 24;
const TASK_HEIGHT = 40;

export function isTask(node: ProcessNode): boolean {
  return node.kind === "human_task" || node.kind === "service_task" || node.kind === "subprocess";
}

function isRound(node: ProcessNode): boolean {
  return node.kind === "start" || node.kind === "end" || node.kind.endsWith("_event");
}

function isGateway(node: ProcessNode): boolean {
  return node.kind === "exclusive_gateway" || node.kind === "parallel_split" || node.kind === "parallel_join";
}

/** The interior steps in flow order: breadth-first from the start, each node once. */
export function linearSteps(process: Process): ProcessNode[] {
  const byId = new Map(process.nodes.map((node) => [node.id, node]));
  const start = process.nodes.find((node) => node.kind === "start") ?? process.nodes[0];
  const seen = new Set<string>();
  const order: ProcessNode[] = [];
  const queue = start === undefined ? [] : [start.id];
  while (queue.length > 0) {
    const id = queue.shift() ?? "";
    if (seen.has(id)) {
      continue;
    }
    seen.add(id);
    const node = byId.get(id);
    if (node !== undefined) {
      order.push(node);
    }
    for (const flow of process.flows) {
      if (flow.from === id) {
        queue.push(flow.to);
      }
    }
  }
  for (const node of process.nodes) {
    if (!seen.has(node.id)) {
      order.push(node);
    }
  }
  return order.filter((node) => node.kind !== "start" && node.kind !== "end");
}

/** The lane of each node: its first performer; a start takes its successor's lane, others their predecessor's. */
export function nodeLanes(process: Process): { lanes: string[]; laneOf: Map<string, number> } {
  const lanes = [...process.lanes];
  const laneOf = new Map<string, number>();
  for (const node of process.nodes) {
    const performer = [...node.performer_refs].sort()[0];
    if (performer !== undefined) {
      if (!lanes.includes(performer)) {
        lanes.push(performer);
      }
      laneOf.set(node.id, lanes.indexOf(performer));
    }
  }
  for (let pass = 0; pass < process.nodes.length; pass += 1) {
    for (const node of process.nodes) {
      if (laneOf.has(node.id)) {
        continue;
      }
      const neighbour =
        node.kind === "start"
          ? process.flows.find((flow) => flow.from === node.id && laneOf.has(flow.to))?.to
          : process.flows.find((flow) => flow.to === node.id && laneOf.has(flow.from))?.from;
      if (neighbour !== undefined) {
        laneOf.set(node.id, laneOf.get(neighbour) ?? 0);
      }
    }
  }
  const unassigned = process.nodes.filter((node) => !laneOf.has(node.id));
  if (unassigned.length > 0) {
    const index = lanes.length;
    lanes.push("");
    unassigned.forEach((node) => laneOf.set(node.id, index));
  }
  return { lanes, laneOf };
}

export interface PlacedNode {
  readonly node: ProcessNode;
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly lane: number;
}

export interface PlacedFlow {
  readonly from: string;
  readonly to: string;
  readonly path: string;
}

export interface ProcessLayout {
  /** Lane performer refs ("" is the Unassigned lane). */
  readonly lanes: readonly string[];
  readonly nodes: readonly PlacedNode[];
  readonly flows: readonly PlacedFlow[];
  readonly width: number;
  readonly height: number;
}

function size(node: ProcessNode, label: string): { width: number; height: number } {
  if (isRound(node)) {
    return { width: 24, height: 24 };
  }
  if (isGateway(node)) {
    return { width: 36, height: 36 };
  }
  return { width: Math.round(Math.min(200, Math.max(120, label.length * 6.6 + 32))), height: TASK_HEIGHT };
}

/** The swimlane geometry: ELK layered x, lane y (nodes sharing a lane and column are spread). */
export async function layoutProcess(process: Process, label: (node: ProcessNode) => string): Promise<ProcessLayout> {
  const { lanes, laneOf } = nodeLanes(process);
  const sizes = new Map(process.nodes.map((node) => [node.id, size(node, label(node))]));
  const request: ElkNode = {
    id: "process",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "RIGHT",
      "elk.spacing.nodeNode": "24",
      "elk.layered.spacing.nodeNodeBetweenLayers": "40",
      "elk.randomSeed": "1",
    },
    children: [...process.nodes].sort((a, b) => (a.id < b.id ? -1 : 1)).map((node) => ({ id: node.id, ...sizes.get(node.id) })),
    edges: process.flows.map((flow) => ({ id: flow.from + "->" + flow.to, sources: [flow.from], targets: [flow.to] })),
  };
  const result = await new ELK().layout(request);
  const xs = new Map((result.children ?? []).map((child) => [child.id, child.x ?? 0]));
  const placed = process.nodes.map((node) => {
    const { width, height } = sizes.get(node.id) ?? { width: 120, height: TASK_HEIGHT };
    const lane = laneOf.get(node.id) ?? 0;
    return { node, width, height, lane, x: LANE_LABEL + PAD + (xs.get(node.id) ?? 0), y: lane * LANE_HEIGHT + LANE_HEIGHT / 2 - height / 2 };
  });
  // Nodes of one lane whose columns overlap are spread vertically (Approve above Reject).
  const spread: PlacedNode[] = [];
  for (let lane = 0; lane < lanes.length; lane += 1) {
    const inLane = placed.filter((p) => p.lane === lane).sort((a, b) => a.x - b.x);
    let cluster: typeof inLane = [];
    const flush = () => {
      cluster.forEach((p, index) => spread.push({ ...p, y: p.y + (index - (cluster.length - 1) / 2) * 52 }));
      cluster = [];
    };
    for (const p of inLane) {
      const last = cluster[cluster.length - 1];
      if (last !== undefined && p.x >= last.x + last.width) {
        flush();
      }
      cluster.push(p);
    }
    flush();
  }
  const at = new Map(spread.map((p) => [p.node.id, p]));
  const flows: PlacedFlow[] = process.flows.flatMap((flow) => {
    const a = at.get(flow.from);
    const b = at.get(flow.to);
    if (a === undefined || b === undefined) {
      return [];
    }
    const x1 = a.x + a.width;
    const y1 = a.y + a.height / 2;
    const x2 = b.x - 2;
    const y2 = b.y + b.height / 2;
    if (x2 > x1 + 16) {
      const mid = Math.round(x1 + Math.min(14, (x2 - x1) / 2));
      return [{ from: flow.from, to: flow.to, path: `M${x1} ${y1} H${mid} V${y2} H${x2}` }];
    }
    const below = Math.max(a.y + a.height, b.y + b.height) + 24;
    return [{ from: flow.from, to: flow.to, path: `M${x1} ${y1} H${x1 + 12} V${below} H${x2 - 12} V${y2} H${x2}` }];
  });
  const width = Math.max(LANE_LABEL + PAD * 2, ...spread.map((p) => p.x + p.width + PAD * 2));
  return { lanes, nodes: spread, flows, width, height: Math.max(1, lanes.length) * LANE_HEIGHT };
}
