// U2.4 process tests (Hotfix 079): the /model/processes route with explicit process selection;
// the diagram driven only by Process.nodes + Process.flows (never steps) with swimlanes from
// performer_refs and a view-only Unassigned lane; events, inputs, outputs, gateways and validation
// shown from server data; a deterministic canonical ELK request; view-only drag and client SVG
// export with no requests; and the three semantic edits (add, move, move_lane), each exactly one
// POST /api/processes/{id}/steps with the current If-Match, refetching after success and never
// retrying. Every HTTP call is answered by MSW and recorded; nothing touches the network.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http } from "msw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { etagJson, server, TEST_ETAG } from "../../test/msw";

type Process = components["schemas"]["Process"];
type NodeView = components["schemas"]["ProcessNodeView"];

let routerModule: typeof import("../../app/router");
let mutation: typeof import("../../api/useVersionedMutation");
let diagram: typeof import("./ProcessDiagram");

class FakeWebSocket extends EventTarget {
  constructor(readonly url: string | URL) {
    super();
  }
  close(): void {}
}

class OriginRequest extends Request {
  constructor(input: RequestInfo | URL, init?: RequestInit) {
    super(typeof input === "string" ? new URL(input, window.location.href) : input, init);
  }
}

beforeAll(async () => {
  server.listen({ onUnhandledRequest: "error" });
  vi.stubGlobal("Request", OriginRequest);
  vi.stubGlobal("WebSocket", FakeWebSocket);
  vi.stubGlobal("scrollTo", () => undefined);
  routerModule = await import("../../app/router");
  mutation = await import("../../api/useVersionedMutation");
  diagram = await import("./ProcessDiagram");
});

afterAll(() => {
  server.close();
  vi.unstubAllGlobals();
});

afterEach(() => {
  cleanup();
  server.resetHandlers();
  mutation.useConflictStore.getState().clearConflict();
  vi.restoreAllMocks();
});

// ------------------------------------------------------------------ fixtures

const CURRENT = "psg:sha256:current";

function pnode(id: string, kind: NodeView["kind"], extra: Partial<NodeView> = {}): NodeView {
  return {
    id,
    kind,
    operation_ref: null,
    performer_refs: [],
    consumes_refs: [],
    produces_refs: [],
    condition_expr: null,
    message_ref: null,
    timer_expr: null,
    ...extra,
  };
}

/** Node IDs are chosen so that their lexical order (steps) is not the flow order. */
const NODES: NodeView[] = [
  pnode("pn:a-review", "human_task", {
    operation_ref: "op:review",
    performer_refs: ["actor:manager", "brole:hr"],
    produces_refs: ["outcome:reviewed"],
  }),
  pnode("pn:b-file", "service_task", { operation_ref: "op:file", performer_refs: ["actor:clerk"] }),
  pnode("pn:c-timer", "timer_event", { timer_expr: "P2D" }),
  pnode("pn:k-gate", "exclusive_gateway", { condition_expr: "days > 3" }),
  pnode("pn:m-wait", "message_event", { message_ref: "event:requested", consumes_refs: ["event:requested"] }),
  pnode("pn:y-end", "end"),
  pnode("pn:z-start", "start"),
];

const FLOWS = [
  { from: "pn:a-review", to: "pn:k-gate" },
  { from: "pn:b-file", to: "pn:y-end" },
  { from: "pn:c-timer", to: "pn:y-end" },
  { from: "pn:k-gate", to: "pn:b-file" },
  { from: "pn:k-gate", to: "pn:c-timer" },
  { from: "pn:m-wait", to: "pn:a-review" },
  { from: "pn:z-start", to: "pn:m-wait" },
];

const LEAVE: Process = {
  id: "proc:leave",
  name: "Leave",
  lanes: ["actor:clerk", "actor:manager", "brole:hr"],
  steps: NODES.map((n) => n.id),
  branches: ["pn:k-gate"],
  validation: ["unreachable:pn:ghost"],
  nodes: NODES,
  flows: FLOWS,
};

const SIMPLE: Process = {
  id: "proc:simple",
  name: "Simple",
  lanes: [],
  steps: ["pn:s-end", "pn:s-start"],
  branches: [],
  validation: [],
  nodes: [pnode("pn:s-end", "end"), pnode("pn:s-start", "start")],
  flows: [{ from: "pn:s-start", to: "pn:s-end" }],
};

interface Call {
  readonly method: string;
  readonly path: string;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

function handlers(calls: Call[], write: () => Response = () => etagJson({ version: "rev:2", revision: "rev:2", semantic_hash: CURRENT, events: [] })) {
  const record = async (request: Request, method: string) => {
    const url = new URL(request.url);
    const text = method === "GET" ? "" : await request.text();
    calls.push({
      method,
      path: decodeURIComponent(url.pathname + url.search),
      ifMatch: request.headers.get("if-match"),
      body: text === "" ? null : (JSON.parse(text) as unknown),
    });
  };
  return [
    http.get("*/api/me", () => etagJson({ user: "alice", mode: "pilot", roles: ["analyst"] })),
    http.get("*/api/model/version", () =>
      etagJson({ version: "rev:1", revision: "rev:1", hash: CURRENT, semantic_hash: CURRENT, evidence_hash: "ev:1" }),
    ),
    http.get("*/api/processes", async ({ request }) => {
      await record(request, "GET");
      return etagJson({ items: [LEAVE, SIMPLE], next_cursor: null });
    }),
    http.get("*/api/processes/:id", async ({ request, params }) => {
      await record(request, "GET");
      return etagJson(params["id"] === "proc:simple" ? SIMPLE : LEAVE);
    }),
    http.post("*/api/processes/:id/steps", async ({ request }) => {
      await record(request, "POST");
      return write();
    }),
    // Recorded so that any generic proposal call is visible (U2.4 must never make one).
    http.post("*/api/proposals/*", async ({ request }) => {
      await record(request, "POST");
      return etagJson({ code: "E_INTERNAL", message: "not used by U2.4", details: {} }, { status: 500 });
    }),
  ];
}

async function renderProcess(calls: Call[], id = "proc:leave") {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: ["/model/processes"] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  const user = userEvent.setup();
  await user.selectOptions(await screen.findByTestId("process-select"), id);
  await screen.findByTestId("process-diagram");
  return { user, calls };
}

const posts = (calls: Call[]) => calls.filter((c) => c.method === "POST");
const detailReads = (calls: Call[]) => calls.filter((c) => c.method === "GET" && c.path === "/api/processes/proc:leave").length;
const nodeGroup = (id: string) => document.querySelector("g[data-node='" + id + "']") as SVGGElement;
const detail = (id: string, field: string) => document.querySelector("[data-node-detail='" + id + "'] [data-field=" + field + "]")?.textContent;
const translateY = (id: string) => Number((nodeGroup(id).getAttribute("transform") ?? "").replace(/^translate\(\S+ (\S+)\)$/, "$1"));

// ------------------------------------------------------------------ route and projection

describe("ProcessesPage", () => {
  it("selects a server process on /model/processes and reads it through the generated client", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    await renderProcess(calls);
    expect(calls.map((c) => c.path)).toEqual(["/api/processes", "/api/processes/proc:leave"]);
    expect(document.querySelector("[data-process]")?.getAttribute("data-process")).toBe("proc:leave");
    expect(posts(calls)).toEqual([]);
  });

  it("draws the topology from flows, never from the steps order", async () => {
    server.use(...handlers([]));
    await renderProcess([]);
    const drawn = Array.from(document.querySelectorAll("line[data-flow]")).map((l) => l.getAttribute("data-flow"));
    expect(drawn).toEqual(FLOWS.map((f) => f.from + "->" + f.to));
    // Adjacent steps entries are not edges.
    expect(drawn).not.toContain("pn:a-review->pn:b-file");
    expect(drawn).not.toContain("pn:y-end->pn:z-start");
  });

  it("builds swimlanes from performer_refs with a view-only Unassigned lane", async () => {
    server.use(...handlers([]));
    await renderProcess([]);
    const lanes = Array.from(document.querySelectorAll("g[data-lane]")).map((g) => g.getAttribute("data-lane"));
    expect(lanes).toEqual(["actor:clerk", "actor:manager", "brole:hr", "unassigned"]);
    expect(document.querySelector("g[data-lane=unassigned]")?.textContent).toBe("Unassigned");
    // A multi-performer node lists all of its performers and notes its placement.
    expect(detail("pn:a-review", "performer_refs")).toBe("actor:manager, brole:hr");
    expect(document.querySelector("[data-node-detail='pn:a-review'] [data-testid=multi-performer-note]")).not.toBeNull();
    // Placement: first sorted performer lane; zero performers go to Unassigned.
    expect(translateY("pn:b-file")).toBeLessThan(translateY("pn:a-review"));
    expect(translateY("pn:a-review")).toBeLessThan(translateY("pn:z-start"));
    expect(translateY("pn:z-start")).toBe(translateY("pn:k-gate"));
    expect(detail("pn:z-start", "performer_refs")).toBe("Unassigned");
    // The operation never stands in for a performer.
    expect(detail("pn:b-file", "operation_ref")).toBe("op:file");
  });

  it("shows events, inputs, outputs, gateways, branches and validation from the server data", async () => {
    server.use(...handlers([]));
    await renderProcess([]);
    expect(nodeGroup("pn:m-wait").getAttribute("data-kind")).toBe("message_event");
    expect(nodeGroup("pn:c-timer").getAttribute("data-kind")).toBe("timer_event");
    expect(detail("pn:m-wait", "consumes_refs")).toBe("event:requested");
    expect(detail("pn:m-wait", "message_ref")).toBe("event:requested");
    expect(detail("pn:c-timer", "timer_expr")).toBe("P2D");
    expect(detail("pn:a-review", "produces_refs")).toBe("outcome:reviewed");
    expect(nodeGroup("pn:k-gate").getAttribute("data-kind")).toBe("exclusive_gateway");
    expect(detail("pn:k-gate", "condition_expr")).toBe("days > 3");
    expect(screen.getByTestId("branch-list").textContent).toBe("pn:k-gate");
    expect(screen.getByTestId("validation-list").textContent).toBe("unreachable:pn:ghost");
    // Exactly three semantic forms: no remove and no branch editing.
    const edits = screen.getByRole("region", { name: "Change the process" });
    const forms = Array.from(edits.querySelectorAll("form")).map((f) => f.getAttribute("aria-label"));
    expect(forms).toEqual(["Add existing operation", "Reorder step", "Move to another lane"]);
    expect(screen.queryByRole("button", { name: /remove|branch|bpmn|mermaid/i })).toBeNull();
  });

  it("distinguishes empty validation from an error", async () => {
    server.use(...handlers([]));
    await renderProcess([], "proc:simple");
    expect(screen.getByTestId("validation-empty").textContent).toBe("No validation findings.");
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

// ------------------------------------------------------------------ layout determinism

describe("Process layout", () => {
  it("canonicalizes the ELK request and lanes regardless of input order", () => {
    const reversed = { nodes: [...NODES].reverse().map((n) => ({ ...n, performer_refs: [...n.performer_refs].reverse() })), flows: [...FLOWS].reverse() };
    expect(diagram.processLayoutRequest(reversed)).toEqual(diagram.processLayoutRequest({ nodes: NODES, flows: FLOWS }));
    expect(diagram.processLanes(reversed.nodes)).toEqual(diagram.processLanes(NODES));
    const request = diagram.processLayoutRequest({ nodes: NODES, flows: FLOWS });
    expect(request.children?.map((c) => c.id)).toEqual([...NODES.map((n) => n.id)].sort());
    expect(request.edges?.map((e) => e.id)).toEqual(FLOWS.map((f) => f.from + "->" + f.to));
    expect(request.layoutOptions?.["elk.randomSeed"]).toBe("1");
  });
});

// ------------------------------------------------------------------ view-only actions

describe("View-only actions", () => {
  it("moves a node visually on drag without any request", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    await renderProcess(calls);
    const before = nodeGroup("pn:b-file").getAttribute("transform");
    const requestsBefore = calls.length;
    // Any request outside the generated client would go through the global fetch.
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const svg = screen.getByRole("img", { name: "Process diagram" });
    fireEvent.mouseDown(nodeGroup("pn:b-file"), { clientX: 10, clientY: 10 });
    fireEvent.mouseMove(svg, { clientX: 70, clientY: 50 });
    fireEvent.mouseUp(svg);
    const after = nodeGroup("pn:b-file").getAttribute("transform");
    expect(after).not.toBe(before);
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(calls.slice(requestsBefore)).toEqual([]);
    expect(posts(calls)).toEqual([]);
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("exports the rendered SVG on the client without any request", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    await renderProcess(calls);
    const blobs: Blob[] = [];
    vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: (blob: Blob) => (blobs.push(blob), "blob:svg"), revokeObjectURL: () => undefined }));
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const requestsBefore = calls.length;
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    fireEvent.click(screen.getByTestId("export-svg"));
    expect(click).toHaveBeenCalledTimes(1);
    expect(blobs).toHaveLength(1);
    const text = await new Promise<string>((resolve) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.readAsText(blobs[0] as Blob);
    });
    expect(text.startsWith("<svg")).toBe(true);
    expect(text).toContain("pn:k-gate");
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(calls.slice(requestsBefore)).toEqual([]);
    expect(calls.some((c) => c.path.includes("/export"))).toBe(false);
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});

// ------------------------------------------------------------------ semantic edits

describe("Semantic process edits", () => {
  it("adds an existing operation with one versioned POST and refetches", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    const { user } = await renderProcess(calls);
    const reads = detailReads(calls);
    await user.type(screen.getByTestId("add-operation"), "op:notify");
    await user.type(screen.getByTestId("add-position"), "2");
    await user.click(screen.getByTestId("add-submit"));
    await waitFor(() => expect(detailReads(calls)).toBeGreaterThan(reads));
    expect(posts(calls).map((c) => [c.path, c.ifMatch, c.body])).toEqual([
      ["/api/processes/proc:leave/steps", TEST_ETAG, { op: "add", operation_ref: "op:notify", position: 2, reason: null }],
    ]);
  });

  it("reorders a step by its semantic node ID with one versioned POST", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    const { user } = await renderProcess(calls);
    await user.selectOptions(screen.getByTestId("move-node"), "pn:b-file");
    await user.type(screen.getByTestId("move-position"), "0");
    await user.type(screen.getByTestId("move-reason"), "File first.");
    await user.click(screen.getByTestId("move-submit"));
    await waitFor(() => expect(posts(calls)).toHaveLength(1));
    expect(posts(calls).map((c) => [c.path, c.ifMatch, c.body])).toEqual([
      ["/api/processes/proc:leave/steps", TEST_ETAG, { op: "move", node_ref: "pn:b-file", position: 0, reason: "File first." }],
    ]);
    expect(calls.some((c) => c.path.startsWith("/api/proposals"))).toBe(false);
  });

  it("moves a task to another lane with one versioned POST and refetches", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    const { user } = await renderProcess(calls);
    // Only human and service tasks can change lane.
    const options = Array.from((screen.getByTestId("lane-node") as HTMLSelectElement).options).map((o) => o.value);
    expect(options).toEqual(["", "pn:a-review", "pn:b-file"]);
    const reads = detailReads(calls);
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    await user.selectOptions(screen.getByTestId("lane-node"), "pn:b-file");
    await user.type(screen.getByTestId("lane-performer"), "brole:hr");
    await user.click(screen.getByTestId("lane-submit"));
    await waitFor(() => expect(detailReads(calls)).toBeGreaterThan(reads));
    expect(posts(calls).map((c) => [c.path, c.ifMatch, c.body])).toEqual([
      ["/api/processes/proc:leave/steps", TEST_ETAG, { op: "move_lane", node_ref: "pn:b-file", performer_ref: "brole:hr", reason: null }],
    ]);
    expect(calls.some((c) => c.path.startsWith("/api/proposals"))).toBe(false);
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("shows a write failure code and never retries", async () => {
    const calls: Call[] = [];
    server.use(
      ...handlers(calls, () =>
        etagJson({ code: "E_NO_CHANGE", message: "no change", details: {} }, { status: 422 }),
      ),
    );
    const { user } = await renderProcess(calls);
    await user.selectOptions(screen.getByTestId("lane-node"), "pn:b-file");
    await user.type(screen.getByTestId("lane-performer"), "actor:clerk");
    await user.click(screen.getByTestId("lane-submit"));
    expect((await screen.findByTestId("process-write-error")).textContent).toContain("E_NO_CHANGE");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(posts(calls)).toHaveLength(1);
    expect(calls.some((c) => c.path.startsWith("/api/proposals"))).toBe(false);
    expect(within(screen.getByTestId("process-diagram")).queryByText(/remove/i)).toBeNull();
  });
});
