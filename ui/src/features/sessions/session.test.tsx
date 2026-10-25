// U2.3 session tests (Hotfix 077/078): the /sessions project room (scope [] on explicit Start only);
// the shell Project room launcher; the requirement-scoped side panel (scope [requirementId] only
// for an Accepted requirement, no fallback); no scoped panel on list routes; explicit S0..S12
// stages; blank turns never sent; one pending → final request per turn (no streaming); the
// current-page transcript that a remount does not restore; server metrics refetched after turns;
// proposal cards with only [kind, reject] callable, Hotfix 078 reason gating for every positive
// kind, no second intake, stale and resolved states; and engine questions only through
// asked_question. Every HTTP call is answered by MSW and recorded; nothing touches the network.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http } from "msw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { etagJson, server, TEST_ETAG } from "../../test/msw";

type Proposal = components["schemas"]["SessionProposalView"];
type TurnResponse = components["schemas"]["SessionTurnResponse"];
type ProposalKind = components["schemas"]["ProposalKind"];

let routerModule: typeof import("../../app/router");
let mutation: typeof import("../../api/useVersionedMutation");
let nav: typeof import("../../app/nav");
let sessions: typeof import("./SessionPanel");

/** WebSocket/EventSource constructions other than the shell's DesignEvent socket. */
const sockets: string[] = [];

class FakeWebSocket extends EventTarget {
  constructor(readonly url: string | URL) {
    super();
    sockets.push(String(url));
  }
  close(): void {}
}

class FakeEventSource extends EventTarget {
  constructor(readonly url: string | URL) {
    super();
    sockets.push("sse:" + String(url));
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
  vi.stubGlobal("EventSource", FakeEventSource);
  vi.stubGlobal("scrollTo", () => undefined);
  routerModule = await import("../../app/router");
  mutation = await import("../../api/useVersionedMutation");
  nav = await import("../../app/nav");
  sessions = await import("./SessionPanel");
});

afterAll(() => {
  server.close();
  vi.unstubAllGlobals();
});

afterEach(() => {
  cleanup();
  server.resetHandlers();
  sockets.length = 0;
  mutation.useConflictStore.getState().clearConflict();
});

// ------------------------------------------------------------------ fixtures

const CURRENT = "psg:sha256:current";

function intake(requiresReason: boolean): Proposal["intake"] {
  return {
    covered_by: [],
    conflicts_with: [],
    refines: [],
    new_terms: [],
    novelty: null,
    duplicate_candidates: [],
    validation_findings: [],
    reason_codes: requiresReason ? ["material_decision"] : [],
    requires_reason: requiresReason,
  };
}

function proposal(id: string, kind: ProposalKind, requiresReason = false): Proposal {
  return {
    id,
    kind,
    payload: { op: "AddNode", node: { id: "req:new-" + id } } as unknown as Record<string, never>,
    intake: intake(requiresReason),
  };
}

function turnResponse(reply: string, proposals: Proposal[] = [], asked: string | null = null): TurnResponse {
  return { version: "rev:2", revision: "rev:2", semantic_hash: CURRENT, events: [], reply, proposals, asked_question: asked };
}

const Q_ENGINE = {
  id: "q:engine",
  text: "Who approves leave requests?",
  kind: "RoleAssignment",
  status: "Open",
  stakeholder_ref: "stakeholder:hr",
  priority: 1,
  round_ref: null,
  context_refs: [],
  answer_schema: {
    type: "object",
    properties: { performer_ref: { type: "string", enum: ["actor:manager", "actor:clerk"] } },
    required: ["performer_ref"],
    additionalProperties: false,
  },
};

interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

interface Backend {
  calls: Call[];
  requirementStatus: string | null;
  turns: TurnResponse[];
  turnFails: boolean;
  inferenceAvailable?: boolean;
  holdTurn: Promise<void> | null;
  metrics: { turns: number; proposals: number; requires_reason: number }[];
  action: (verb: string) => Response | null;
}

function newBackend(overrides: Partial<Backend> = {}): Backend {
  return {
    calls: [],
    requirementStatus: "Accepted",
    turns: [],
    turnFails: false,
    holdTurn: null,
    metrics: [{ turns: 0, proposals: 0, requires_reason: 0 }],
    action: () => null,
    ...overrides,
  };
}

const WRITE = { version: "rev:3", revision: "rev:3", semantic_hash: CURRENT, events: [] };

function handlers(backend: Backend) {
  const record = async (request: Request, method: string) => {
    const url = new URL(request.url);
    const text = method === "GET" ? "" : await request.text();
    backend.calls.push({
      method,
      path: decodeURIComponent(url.pathname),
      search: url.searchParams,
      ifMatch: request.headers.get("if-match"),
      body: text === "" ? null : (JSON.parse(text) as unknown),
    });
  };
  return [
    http.get("*/api/me", () => etagJson({ user: "alice", mode: "pilot", roles: ["analyst"], inference_available: backend.inferenceAvailable ?? true })),
    http.get("*/api/model/version", () =>
      etagJson({ version: "rev:1", revision: "rev:1", hash: CURRENT, semantic_hash: CURRENT, evidence_hash: "ev:1" }),
    ),
    http.get("*/api/gates", () => etagJson([])),
    http.get("*/api/requirements", () => etagJson({ items: [], next_cursor: null })),
    http.get("*/api/glossary", () => etagJson({ items: [], next_cursor: null })),
    http.get("*/api/model/entities", () => etagJson({ items: [], next_cursor: null })),
    http.get("*/api/model/sentences", () => etagJson({ items: [], next_cursor: null })),
    http.get("*/api/requirements/:id", async ({ request, params }) => {
      await record(request, "GET");
      if (backend.requirementStatus === null) {
        return etagJson({ code: "E_NOT_FOUND", message: "missing", details: {} }, { status: 404 });
      }
      return etagJson({
        id: params["id"],
        text: "Employees shall submit leave requests.",
        class: "functional",
        status: backend.requirementStatus,
        smells: [],
        coverage: { operations: [], scenarios: [] },
        readiness: null,
        spans: [],
        lint_findings: [],
        ears_suggestion: null,
        linked: { operations: [], processes: [], scenarios: [] },
      });
    }),
    http.get("*/api/questions", async ({ request }) => {
      await record(request, "GET");
      return etagJson({ items: [Q_ENGINE], next_cursor: null });
    }),
    http.post("*/api/questions/:id/answer", async ({ request }) => {
      await record(request, "POST");
      return etagJson({ code: "E_UNEXPECTED", message: "not in these tests", details: {} }, { status: 500 });
    }),
    http.post("*/api/sessions", async ({ request }) => {
      await record(request, "POST");
      return etagJson({ ...WRITE, id: "session:1" });
    }),
    http.get("*/api/sessions/:id/metrics", async ({ request }) => {
      await record(request, "GET");
      const metrics = backend.metrics.length > 1 ? backend.metrics.shift() : backend.metrics[0];
      return etagJson(metrics);
    }),
    http.post("*/api/sessions/:id/turns", async ({ request }) => {
      await record(request, "POST");
      if (backend.holdTurn !== null) {
        const hold = backend.holdTurn;
        backend.holdTurn = null;
        await hold;
      }
      if (backend.turnFails) {
        return etagJson({ code: "E_PROVIDER_FAILED", message: "provider failed", details: {} }, { status: 502 });
      }
      const next = backend.turns.shift();
      return etagJson(next ?? turnResponse("(no reply configured)"));
    }),
    http.post("*/api/proposals/:id/:verb", async ({ request, params }) => {
      await record(request, "POST");
      return backend.action(String(params["verb"])) ?? etagJson(WRITE);
    }),
  ];
}

function renderAt(path: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  const view = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { router, queryClient, view };
}

const posts = (backend: Backend, path: string) => backend.calls.filter((c) => c.method === "POST" && c.path === path);
const proposalPosts = (backend: Backend) => backend.calls.filter((c) => c.method === "POST" && c.path.startsWith("/api/proposals/"));
const panel = () => screen.getByTestId("session-panel");

async function startSession(user: ReturnType<typeof userEvent.setup>, backend: Backend) {
  await user.click(await screen.findByTestId("session-start"));
  await screen.findByTestId("session-id");
  expect(posts(backend, "/api/sessions")).toHaveLength(1);
}

async function sendTurn(user: ReturnType<typeof userEvent.setup>, text: string, stage = "S1") {
  const select = screen.getByTestId("session-stage") as HTMLSelectElement;
  if (select.value !== stage) {
    await user.selectOptions(select, stage);
  }
  await user.type(screen.getByTestId("session-text"), text);
  await user.click(screen.getByTestId("session-send"));
}

async function cardAfterTurn(backend: Backend, card: Proposal) {
  backend.turns.push(turnResponse("Here is a proposal.", [card]));
  const user = userEvent.setup();
  await startSession(user, backend);
  await sendTurn(user, "Propose something.");
  const element = await screen.findByText(card.id, { selector: "[data-testid=card-proposal]" });
  return { user, card: element.closest("article") as HTMLElement };
}

const button = (card: HTMLElement, action: string) => card.querySelector("button[data-action=" + action + "]") as HTMLButtonElement | null;

// ------------------------------------------------------------------ project room and launcher

describe("Project room", () => {
  it("is the real /sessions session panel and creates scope [] only on Start session", async () => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderAt("/sessions");
    const user = userEvent.setup();
    expect(await screen.findByTestId("session-panel")).toBeTruthy();
    expect(panel().getAttribute("data-session-mode")).toBe("project");
    expect(within(panel()).getByTestId("session-scope").textContent).toBe("Project");
    // Opening the room sends nothing; there is no duplicate scoped side panel.
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(posts(backend, "/api/sessions")).toEqual([]);
    expect(screen.queryByTestId("scoped-session-aside")).toBeNull();
    await startSession(user, backend);
    expect(posts(backend, "/api/sessions").map((c) => [c.ifMatch, c.body])).toEqual([[TEST_ETAG, { scope: [] }]]);
  });

  it("is reached through the shell Project room launcher without changing the frozen navigation", async () => {
    server.use(...handlers(newBackend()));
    const { router } = renderAt("/");
    const user = userEvent.setup();
    const launcher = await screen.findByTestId("project-room-launcher");
    expect(launcher.getAttribute("href")).toBe("/sessions");
    await user.click(launcher);
    expect(await screen.findByTestId("session-panel")).toBeTruthy();
    expect(router.state.location.pathname).toBe("/sessions");
    const hrefs = nav.PILOT_NAVIGATION.flatMap((e) => (e.kind === "group" ? e.links.map((l) => l.href) : [e.link.href]));
    expect(hrefs).not.toContain("/sessions");
    expect(hrefs).toEqual([
      "/",
      "/requirements",
      "/model/glossary",
      "/model/sentences",
      "/model/entities",
      "/model/processes",
      "/model/scenarios",
      "/model/findings",
      "/model/rules",
      "/model/calculations",
      "/questions",
      "/proposals",
      "/decisions",
      "/gates",
    ]);
  });
});

// ------------------------------------------------------------------ requirement scope

describe("Requirement-scoped session", () => {
  it("scopes an Accepted requirement to exactly [requirementId]", async () => {
    const backend = newBackend({ requirementStatus: "Accepted" });
    server.use(...handlers(backend));
    renderAt("/requirements/req:accepted");
    const user = userEvent.setup();
    const aside = await screen.findByTestId("scoped-session-aside");
    expect(within(aside).getByTestId("session-panel").getAttribute("data-session-mode")).toBe("requirement");
    expect(within(aside).getByTestId("session-scope").textContent).toBe("req:accepted");
    await waitFor(() => expect((within(aside).getByTestId("session-start") as HTMLButtonElement).disabled).toBe(false));
    await user.click(within(aside).getByTestId("session-start"));
    await within(aside).findByTestId("session-id");
    expect(posts(backend, "/api/sessions").map((c) => [c.ifMatch, c.body])).toEqual([[TEST_ETAG, { scope: ["req:accepted"] }]]);
  });

  it("refuses a non-Accepted requirement without falling back to project scope", async () => {
    const backend = newBackend({ requirementStatus: "Proposed" });
    server.use(...handlers(backend));
    renderAt("/requirements/req:proposed");
    const user = userEvent.setup();
    const aside = await screen.findByTestId("scoped-session-aside");
    expect(within(aside).getByTestId("session-scope").textContent).toBe("req:proposed");
    const unavailable = await within(aside).findByTestId("scope-unavailable");
    expect(unavailable.textContent).toContain("req:proposed");
    expect(unavailable.textContent).toContain("Proposed");
    const start = within(aside).getByTestId("session-start") as HTMLButtonElement;
    expect(start.disabled).toBe(true);
    await user.click(start);
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(posts(backend, "/api/sessions")).toEqual([]);
  });

  it("creates no session when the scope lookup fails", async () => {
    const backend = newBackend({ requirementStatus: null });
    server.use(...handlers(backend));
    renderAt("/requirements/req:ghost");
    const aside = await screen.findByTestId("scoped-session-aside");
    expect((await within(aside).findByTestId("scope-error")).textContent).toContain("req:ghost");
    expect((within(aside).getByTestId("session-start") as HTMLButtonElement).disabled).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(posts(backend, "/api/sessions")).toEqual([]);
  });

  it.each(["/", "/requirements", "/model/glossary", "/model/sentences", "/questions"])("mounts no scoped panel on %s", async (path) => {
    server.use(...handlers(newBackend()));
    renderAt(path);
    await screen.findByTestId("project-room-launcher");
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(screen.queryByTestId("scoped-session-aside")).toBeNull();
    expect(screen.queryByTestId("session-panel")).toBeNull();
  });
});

// ------------------------------------------------------------------ turns

describe("Session turns", () => {
  it("offers exactly S0..S12 with no default and never sends a blank turn", async () => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderAt("/sessions");
    const user = userEvent.setup();
    await startSession(user, backend);
    const options = Array.from((screen.getByTestId("session-stage") as HTMLSelectElement).options).map((o) => o.value);
    expect(options).toEqual(["", ...sessions.SESSION_STAGES]);
    expect(sessions.SESSION_STAGES).toEqual(["S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10", "S11", "S12"]);
    expect((screen.getByTestId("session-stage") as HTMLSelectElement).value).toBe("");
    await user.type(screen.getByTestId("session-text"), "Hello");
    expect((screen.getByTestId("session-send") as HTMLButtonElement).disabled).toBe(true);
    await user.selectOptions(screen.getByTestId("session-stage"), "S7");
    const text = screen.getByTestId("session-text");
    for (const blank of ["", " ", "   ", "\n"]) {
      await user.clear(text);
      if (blank !== "") {
        await user.type(text, blank === "\n" ? "{Enter}" : blank);
      }
      expect((screen.getByTestId("session-send") as HTMLButtonElement).disabled).toBe(true);
      (text as HTMLTextAreaElement).form?.requestSubmit();
    }
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(posts(backend, "/api/sessions/session:1/turns")).toEqual([]);
    backend.turns.push(turnResponse("Noted."));
    await user.clear(text);
    await user.type(text, "Employees request leave.");
    await user.click(screen.getByTestId("session-send"));
    await waitFor(() => expect(posts(backend, "/api/sessions/session:1/turns")).toHaveLength(1));
    expect(posts(backend, "/api/sessions/session:1/turns").map((c) => [c.ifMatch, c.body])).toEqual([
      [TEST_ETAG, { text: "Employees request leave.", stage: "S7" }],
    ]);
  });

  it("shows one pending request and the final reply only from the final response", async () => {
    let release: () => void = () => undefined;
    const backend = newBackend({
      holdTurn: new Promise<void>((resolve) => {
        release = resolve;
      }),
    });
    backend.turns.push(turnResponse("The final assistant reply."));
    server.use(...handlers(backend));
    renderAt("/sessions");
    const user = userEvent.setup();
    await startSession(user, backend);
    await sendTurn(user, "Tell me more.");
    expect(await screen.findByTestId("session-pending")).toBeTruthy();
    expect(posts(backend, "/api/sessions/session:1/turns")).toHaveLength(1);
    expect(screen.queryByText("The final assistant reply.", { exact: false })).toBeNull();
    const requestsWhilePending = backend.calls.length;
    await new Promise((resolve) => setTimeout(resolve, 50));
    // Nothing is polled or streamed while the one request is outstanding.
    expect(backend.calls.slice(requestsWhilePending).filter((c) => c.path.startsWith("/api/sessions"))).toEqual([]);
    await act(async () => {
      release();
    });
    expect(await screen.findByText("The final assistant reply.", { exact: false })).toBeTruthy();
    expect(screen.queryByTestId("session-pending")).toBeNull();
    expect(posts(backend, "/api/sessions/session:1/turns")).toHaveLength(1);
    // Only the shell's DesignEvent socket exists: no chat WebSocket or SSE.
    expect(sockets.every((url) => url.endsWith("/api/events"))).toBe(true);
  });

  it("keeps a current-page transcript that a fresh panel does not restore", async () => {
    const backend = newBackend();
    backend.turns.push(turnResponse("Assistant one."), turnResponse("Assistant two."));
    server.use(...handlers(backend));
    const { view } = renderAt("/sessions");
    const user = userEvent.setup();
    await startSession(user, backend);
    await sendTurn(user, "User one.");
    await screen.findByText("Assistant one.", { exact: false });
    await sendTurn(user, "User two.");
    await screen.findByText("Assistant two.", { exact: false });
    const lines = Array.from(screen.getByTestId("session-transcript").querySelectorAll("[data-role]")).map((p) => [
      p.getAttribute("data-role"),
      p.textContent,
    ]);
    expect(lines).toEqual([
      ["user", "You: User one."],
      ["assistant", "Assistant: Assistant one."],
      ["user", "You: User two."],
      ["assistant", "Assistant: Assistant two."],
    ]);
    // A fresh mount starts without a session or transcript; nothing is restored or fetched.
    view.unmount();
    const before = backend.calls.length;
    renderAt("/sessions");
    expect(await screen.findByTestId("session-start")).toBeTruthy();
    expect(screen.queryByTestId("session-transcript")).toBeNull();
    expect(screen.queryByText("Assistant one.", { exact: false })).toBeNull();
    expect(backend.calls.slice(before).filter((c) => c.path.startsWith("/api/sessions"))).toEqual([]);
    expect(window.localStorage.length).toBe(0);
    expect(window.sessionStorage.length).toBe(0);
  });

  it("disables Start with an explanation when the server has no inference provider (Fable #355)", async () => {
    const backend = newBackend({ inferenceAvailable: false });
    server.use(...handlers(backend));
    renderAt("/sessions");
    const start = (await screen.findByTestId("session-start")) as HTMLButtonElement;
    await waitFor(() => expect(start.disabled).toBe(true));
    expect(screen.getByTestId("session-inference-unavailable").textContent).toBe(
      "Sessions need an AI inference provider, and none is configured for this server.",
    );
    expect(posts(backend, "/api/sessions")).toHaveLength(0);
  });

  it("appends nothing on a failed turn, keeps the draft and does not retry", async () => {
    const backend = newBackend({ turnFails: true });
    server.use(...handlers(backend));
    renderAt("/sessions");
    const user = userEvent.setup();
    await startSession(user, backend);
    await sendTurn(user, "Keep this draft.");
    // Fable #355: a turn-specific failure with the backend typed code and message.
    const failure = await screen.findByTestId("turn-failed");
    expect(failure.textContent).toBe("The turn failed: E_PROVIDER_FAILED provider failed");
    expect(screen.getByTestId("session-transcript").querySelectorAll("[data-role]")).toHaveLength(0);
    expect((screen.getByTestId("session-text") as HTMLTextAreaElement).value).toBe("Keep this draft.");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(posts(backend, "/api/sessions/session:1/turns")).toHaveLength(1);
  });

  it("shows the server metrics and refetches them after a successful turn", async () => {
    const backend = newBackend({
      metrics: [
        { turns: 0, proposals: 0, requires_reason: 0 },
        { turns: 2, proposals: 1, requires_reason: 1 },
      ],
    });
    backend.turns.push(turnResponse("Done."));
    server.use(...handlers(backend));
    renderAt("/sessions");
    const user = userEvent.setup();
    await startSession(user, backend);
    const metric = (name: string) => screen.getByTestId("session-metrics").querySelector("[data-metric=" + name + "]")?.textContent;
    await waitFor(() => expect(metric("turns")).toBe("0"));
    expect([metric("proposals"), metric("requires_reason")]).toEqual(["0", "0"]);
    await sendTurn(user, "Go.");
    await waitFor(() => expect(metric("turns")).toBe("2"));
    expect([metric("proposals"), metric("requires_reason")]).toEqual(["1", "1"]);
    expect(backend.calls.filter((c) => c.path === "/api/sessions/session:1/metrics").length).toBe(2);
  });
});

// ------------------------------------------------------------------ proposal cards

describe("Session proposal cards", () => {
  it("shows the exact server proposal and makes only create and reject callable", async () => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderAt("/sessions");
    const create = proposal("prop:create", "create");
    const { user, card } = await cardAfterTurn(backend, create);
    expect(within(card).getByTestId("card-kind").textContent).toBe("create");
    expect(JSON.parse(within(card).getByTestId("card-payload").textContent ?? "")).toEqual(create.payload);
    expect(within(card).getByTestId("proposal-requires-reason").textContent).toBe("No");
    const enabled = Array.from(card.querySelectorAll("button[data-action]"))
      .filter((b) => !(b as HTMLButtonElement).disabled)
      .map((b) => b.getAttribute("data-action"));
    expect(enabled.sort()).toEqual(["create", "reject"]);
    for (const absent of ["link", "merge", "supersede"]) {
      expect(button(card, absent)).toBeNull();
    }
    await user.click(button(card, "accept") as HTMLButtonElement);
    expect(proposalPosts(backend)).toEqual([]);
    await user.click(button(card, "create") as HTMLButtonElement);
    await within(card).findByTestId("card-resolved");
    expect(proposalPosts(backend).map((c) => [c.path, c.ifMatch, c.body])).toEqual([
      ["/api/proposals/prop:create/create", `"rev:2"`, { reason: null }],
    ]);
    // A resolved card cannot be acted on again.
    expect(Array.from(card.querySelectorAll("button[data-action]")).every((b) => (b as HTMLButtonElement).disabled)).toBe(true);
    expect(posts(backend, "/api/proposals/intake")).toEqual([]);
  });

  it.each(["create", "link"] as const)("gates a reason-required %s until a non-blank rationale (Hotfix 078)", async (kind) => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderAt("/sessions");
    const { user, card } = await cardAfterTurn(backend, proposal("prop:" + kind, kind, true));
    const positive = button(card, kind) as HTMLButtonElement;
    expect(positive.disabled).toBe(true);
    const rationale = within(card).getByRole("textbox", { name: "Rationale" });
    for (const blank of ["   ", "{Enter}"]) {
      await user.type(rationale, blank);
      expect((button(card, kind) as HTMLButtonElement).disabled).toBe(true);
    }
    await user.click(button(card, kind) as HTMLButtonElement);
    expect(proposalPosts(backend)).toEqual([]);
    await user.clear(rationale);
    await user.type(rationale, "HR confirmed the conflict is intended.");
    expect((button(card, kind) as HTMLButtonElement).disabled).toBe(false);
    await user.click(button(card, kind) as HTMLButtonElement);
    await within(card).findByTestId("card-resolved");
    expect(proposalPosts(backend).map((c) => [c.path, c.body])).toEqual([
      ["/api/proposals/prop:" + kind + "/" + kind, { reason: "HR confirmed the conflict is intended." }],
    ]);
    expect(posts(backend, "/api/proposals/intake")).toEqual([]);
  });

  it("keeps reject callable without a rationale on a reason-required proposal", async () => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderAt("/sessions");
    const { user, card } = await cardAfterTurn(backend, proposal("prop:reject", "merge", true));
    expect((button(card, "merge") as HTMLButtonElement).disabled).toBe(true);
    expect((button(card, "reject") as HTMLButtonElement).disabled).toBe(false);
    await user.click(button(card, "reject") as HTMLButtonElement);
    await within(card).findByTestId("card-resolved");
    expect(proposalPosts(backend).map((c) => [c.path, c.body])).toEqual([["/api/proposals/prop:reject/reject", { reason: null }]]);
    expect(Array.from(card.querySelectorAll("button[data-action]")).every((b) => (b as HTMLButtonElement).disabled)).toBe(true);
    expect(posts(backend, "/api/proposals/intake")).toEqual([]);
  });

  it("marks a card stale on E_PROPOSAL_BASELINE_CHANGED with no retry or alternate action", async () => {
    const backend = newBackend({
      action: (verb) =>
        verb === "supersede"
          ? etagJson({ code: "E_PROPOSAL_BASELINE_CHANGED", message: "moved", details: {} }, { status: 409 })
          : null,
    });
    server.use(...handlers(backend));
    renderAt("/sessions");
    const { user, card } = await cardAfterTurn(backend, proposal("prop:stale", "supersede"));
    await user.click(button(card, "supersede") as HTMLButtonElement);
    expect(await within(card).findByTestId("card-stale")).toBeTruthy();
    expect(card.getAttribute("data-state")).toBe("stale");
    expect(Array.from(card.querySelectorAll("button[data-action]")).every((b) => (b as HTMLButtonElement).disabled)).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(proposalPosts(backend).map((c) => c.path)).toEqual(["/api/proposals/prop:stale/supersede"]);
    expect(posts(backend, "/api/proposals/intake")).toEqual([]);
  });

  it("shows other proposal failures without resolving the card or retrying", async () => {
    const backend = newBackend({
      action: () => etagJson({ code: "E_INTAKE_REASON_REQUIRED", message: "reason", details: {} }, { status: 409 }),
    });
    server.use(...handlers(backend));
    renderAt("/sessions");
    const { user, card } = await cardAfterTurn(backend, proposal("prop:fail", "accept"));
    await user.click(button(card, "accept") as HTMLButtonElement);
    expect((await within(card).findByTestId("card-failed")).textContent).toContain("E_INTAKE_REASON_REQUIRED");
    expect(card.getAttribute("data-state")).toBe("pending");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(proposalPosts(backend)).toHaveLength(1);
  });
});

// ------------------------------------------------------------------ engine questions

describe("Engine questions", () => {
  it("never makes a free-form assistant question actionable without asked_question", async () => {
    const backend = newBackend();
    backend.turns.push(turnResponse("Which actor approves the request?", [], null));
    server.use(...handlers(backend));
    renderAt("/sessions");
    const user = userEvent.setup();
    await startSession(user, backend);
    await sendTurn(user, "Who approves?");
    expect(await screen.findByText("Which actor approves the request?", { exact: false })).toBeTruthy();
    expect(document.querySelector("[data-asked-question]")).toBeNull();
    expect(document.querySelector("[data-question-card]")).toBeNull();
    expect(document.querySelector("[data-renderer]")).toBeNull();
    expect(backend.calls.filter((c) => c.path.startsWith("/api/questions"))).toEqual([]);
  });

  it("renders the U2.2 answer flow for an exact Open engine question", async () => {
    const backend = newBackend();
    backend.turns.push(turnResponse("Please answer the engine question.", [], "q:engine"));
    server.use(...handlers(backend));
    renderAt("/sessions");
    const user = userEvent.setup();
    await startSession(user, backend);
    await sendTurn(user, "Continue.");
    const section = await waitFor(() => {
      const element = document.querySelector("[data-asked-question='q:engine'] [data-question-card='q:engine']");
      expect(element).not.toBeNull();
      return element as HTMLElement;
    });
    expect(section.querySelector("[data-renderer]")?.getAttribute("data-renderer")).toBe("role-assignment");
    const options = Array.from(section.querySelectorAll("[data-field=performer_ref] option")).map((o) => o.getAttribute("value"));
    expect(options.filter((v) => v !== "")).toEqual(["actor:manager", "actor:clerk"]);
    expect(backend.calls.find((c) => c.path === "/api/questions")?.search.get("status")).toBe("Open");
  });

  it("shows an asked_question absent from the Open questions as unavailable", async () => {
    const backend = newBackend();
    backend.turns.push(turnResponse("A question was asked.", [], "q:gone"));
    server.use(...handlers(backend));
    renderAt("/sessions");
    const user = userEvent.setup();
    await startSession(user, backend);
    await sendTurn(user, "Continue.");
    expect((await screen.findByTestId("engine-question-unavailable")).textContent).toContain("q:gone");
    expect(document.querySelector("[data-renderer]")).toBeNull();
    expect(document.querySelector("[data-question-card]")).toBeNull();
  });
});
