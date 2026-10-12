// U1.1 behaviour tests: the fixture shell and Home, global search, the versioned-write 412 path,
// DesignEvent invalidation and exact role membership. Every HTTP call is answered by MSW and the
// WebSocket is an in-process fake; nothing touches the network.
//
// The generated client captures `fetch` and `Request` when its module is first evaluated, so the
// app modules are imported dynamically after MSW is listening. Node's `Request` cannot parse the
// relative `/api/...` URLs a browser resolves against its origin, so the test resolves them
// against the jsdom location the same way.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
  type AnyRouter,
} from "@tanstack/react-router";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { server } from "../../test/msw";

type Me = components["schemas"]["MeResponse"];
type ModelVersion = components["schemas"]["ModelVersionResponse"];
type GateStatus = components["schemas"]["GateStatus"];
type SearchPage = components["schemas"]["SearchPage"];
type PreconditionFailed = components["schemas"]["PreconditionFailedResponse"];

interface AppModules {
  shell: typeof import("../../app/AppShell");
  router: typeof import("../../app/router");
  auth: typeof import("../../app/auth");
  nav: typeof import("../../app/nav");
  mutation: typeof import("../../api/useVersionedMutation");
  events: typeof import("../../api/useEvents");
  client: typeof import("../../api/client");
}

let app: AppModules;

/** An in-process WebSocket: records every socket and delivers messages on demand. */
class FakeWebSocket extends EventTarget {
  static instances: FakeWebSocket[] = [];
  readonly url: string;
  closed = false;

  constructor(url: string | URL) {
    super();
    this.url = String(url);
    FakeWebSocket.instances.push(this);
  }

  close(): void {
    this.closed = true;
  }

  deliver(data: unknown): void {
    this.dispatchEvent(new MessageEvent("message", { data }));
  }
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
  // jsdom has no scrolling; the router's scroll restoration calls it on navigation.
  vi.stubGlobal("scrollTo", () => undefined);
  app = {
    shell: await import("../../app/AppShell"),
    router: await import("../../app/router"),
    auth: await import("../../app/auth"),
    nav: await import("../../app/nav"),
    mutation: await import("../../api/useVersionedMutation"),
    events: await import("../../api/useEvents"),
    client: await import("../../api/client"),
  };
});

afterAll(() => {
  server.close();
  vi.unstubAllGlobals();
});

beforeEach(() => {
  FakeWebSocket.instances = [];
});

afterEach(() => {
  cleanup();
  server.resetHandlers();
  server.events.removeAllListeners();
  app.mutation.useConflictStore.getState().clearConflict();
});

// ------------------------------------------------------------------ fixtures

const ME: Me = { user: "alice", mode: "pilot", roles: ["analyst", "reviewer"] };

const VERSION: ModelVersion = {
  version: "rev:7:aaaaaaaaaaaaaaaa",
  revision: "rev:7:aaaaaaaaaaaaaaaa",
  hash: "psg:sha256:current",
  semantic_hash: "psg:sha256:current",
  evidence_hash: "ev:sha256:evidence",
};

const GATES: GateStatus[] = [
  { gate: "I0", passed: true, findings: [] },
  { gate: "F1", passed: false, findings: ["fnd:0123456789abcdef"] },
];

function shellHandlers(counts?: { version: number }) {
  return [
    http.get("*/api/me", () => HttpResponse.json(ME)),
    http.get("*/api/model/version", () => {
      if (counts !== undefined) {
        counts.version += 1;
      }
      return HttpResponse.json(VERSION);
    }),
    http.get("*/api/gates", () => HttpResponse.json(GATES)),
  ];
}

function newQueryClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function renderRouter(router: AnyRouter, queryClient: QueryClient) {
  return render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

function renderApp() {
  const queryClient = newQueryClient();
  const router = app.router.createAppRouter(createMemoryHistory({ initialEntries: ["/"] }));
  const view = renderRouter(router, queryClient);
  return { queryClient, router, view };
}

// ------------------------------------------------------------------ shell and Home

describe("HomePage shell", () => {
  it("renders the shell, identity, full navigation and I0-F4 from fixture data", async () => {
    server.use(...shellHandlers());
    const { router } = renderApp();

    expect(await screen.findByTestId("home-revision")).toHaveProperty("textContent", VERSION.revision);
    expect(screen.getByTestId("home-semantic-hash").textContent).toBe(VERSION.semantic_hash);
    expect(screen.getByTestId("app-name").textContent).toBe("Plumb");

    // Server identity is shown exactly as returned.
    expect((await screen.findByTestId("identity-user")).textContent).toBe("alice");
    expect(screen.getByTestId("identity-mode").textContent).toBe("pilot");
    const roles = within(screen.getByTestId("identity-roles")).getAllByRole("listitem");
    expect(roles.map((r) => r.textContent)).toEqual(["analyst", "reviewer"]);

    // The full pilot navigation, in its frozen order, with stable final hrefs.
    const nav = screen.getByRole("navigation", { name: "Pilot navigation" });
    const ids = Array.from(nav.querySelectorAll("[data-nav-id]")).map((e) => e.getAttribute("data-nav-id"));
    expect(ids).toEqual([
      "home",
      "requirements",
      "model",
      "glossary",
      "sentences",
      "entities",
      "processes",
      "scenarios",
      "findings",
      "rules",
      "calculations",
      "questions",
      "decisions",
      "gates",
      "gate-list",
    ]);
    const hrefs = Object.fromEntries(
      Array.from(nav.querySelectorAll("a[data-nav-id]")).map((a) => [a.getAttribute("data-nav-id"), a.getAttribute("href")]),
    );
    expect(hrefs).toEqual({
      home: "/",
      requirements: "/requirements",
      glossary: "/model/glossary",
      sentences: "/model/sentences",
      entities: "/model/entities",
      processes: "/model/processes",
      scenarios: "/model/scenarios",
      findings: "/model/findings",
      rules: "/model/rules",
      calculations: "/model/calculations",
      questions: "/questions",
      decisions: "/decisions",
      gates: "/gates",
    });
    const labels = Array.from(nav.querySelectorAll("[data-nav-id]")).map((e) => e.textContent);
    expect(labels).toEqual([
      "Home",
      "Requirements",
      "Model",
      "Glossary",
      "Sentences",
      "Entities",
      "Processes",
      "Scenarios",
      "Findings & Assumptions",
      "Rules",
      "Calculations",
      "Questions",
      "Decisions",
      "Gates",
      "I0F1F2F3F4",
    ]);
    // Home is the active destination.
    expect(within(nav).getByRole("link", { name: "Home" }).getAttribute("aria-current")).toBe("page");
    expect(within(nav).getByRole("link", { name: "Requirements" }).getAttribute("aria-current")).toBeNull();

    // I0-F4 in order: returned pass/fail, absent gates unevaluated (never fail).
    const rows = Array.from(document.querySelectorAll("tr[data-gate]"));
    expect(rows.map((r) => [r.getAttribute("data-gate"), r.querySelector("td")?.textContent])).toEqual([
      ["I0", "Pass"],
      ["F1", "Fail"],
      ["F2", "Unevaluated"],
      ["F3", "Unevaluated"],
      ["F4", "Unevaluated"],
    ]);

    // Only implemented pages are registered routes: no placeholder for a future page.
    expect(Object.keys(router.routesByPath).sort()).toEqual(
      ["/", "/model/glossary", "/requirements", "/requirements/$requirementId"],
    );
  });

  it("navigation data is the frozen pilot list", () => {
    expect(app.nav.PILOT_GATES).toEqual(["I0", "F1", "F2", "F3", "F4"]);
    expect(app.nav.PILOT_NAVIGATION.map((e) => (e.kind === "group" ? e.id : e.link.id))).toEqual([
      "home",
      "requirements",
      "model",
      "questions",
      "decisions",
      "gates",
    ]);
  });
});

// ------------------------------------------------------------------ global search

describe("HomePage global search", () => {
  it("never sends a blank query and renders server results without writing", async () => {
    const queries: string[] = [];
    const methods: string[] = [];
    server.events.on("request:start", ({ request }) => {
      methods.push(request.method);
    });
    const page: SearchPage = {
      items: [
        { id: "req:leave-1", kind: "requirement", path: "/api/requirements/req:leave-1", score: 0.75, snippet: "Employees shall submit leave requests." },
        { id: "term:leave", kind: "term", path: null, score: 0.5, snippet: "Leave" },
      ],
      next_cursor: null,
    };
    server.use(
      ...shellHandlers(),
      http.get("*/api/search", ({ request }) => {
        queries.push(new URL(request.url).searchParams.get("q") ?? "");
        return HttpResponse.json(page);
      }),
    );
    renderApp();
    await screen.findByTestId("home-revision");
    const user = userEvent.setup();
    const input = screen.getByRole("searchbox", { name: "Search the model" });
    const submit = screen.getByRole("button", { name: "Search" });

    await user.click(submit);
    await user.type(input, "   ");
    await user.click(submit);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(queries).toEqual([]);

    await user.clear(input);
    await user.type(input, "leave");
    await user.click(submit);
    const results = await screen.findByTestId("search-results");
    expect(queries).toEqual(["leave"]);
    const ids = within(results).getAllByTestId("search-hit-id").map((e) => e.textContent);
    expect(ids).toEqual(["req:leave-1", "term:leave"]);
    expect(results.textContent).toContain("Employees shall submit leave requests.");
    expect(results.textContent).toContain("/api/requirements/req:leave-1");
    expect(results.textContent).toContain("requirement");
    expect(results.textContent).toContain("0.75");
    // Only reads happened.
    expect(methods.length).toBeGreaterThan(0);
    expect(methods.every((m) => m === "GET")).toBe(true);
  });
});

// ------------------------------------------------------------------ versioned writes

describe("HomePage versioned mutation", () => {
  it("sends the fetched semantic_hash once and surfaces a 412 without retrying", async () => {
    const counts = { version: 0 };
    const ifMatch: (string | null)[] = [];
    const rejected: PreconditionFailed = {
      code: "E_PRECONDITION_FAILED",
      message: "the branch head moved",
      details: {},
      revision: "rev:8:bbbbbbbbbbbbbbbb",
      semantic_hash: "psg:sha256:newer",
    };
    server.use(
      ...shellHandlers(counts),
      http.post("*/api/proposals/:id/reject", ({ request }) => {
        ifMatch.push(request.headers.get("If-Match"));
        return HttpResponse.json(rejected, { status: 412 });
      }),
    );

    function Harness() {
      const mutation = app.mutation.useVersionedMutation((id: string, version: string) =>
        app.client.apiClient.POST("/api/proposals/{id}/reject", {
          params: { path: { id }, header: { "If-Match": version } },
          body: {},
        }),
      );
      return (
        <div>
          <button type="button" onClick={() => mutation.mutate("prp:0123456789abcdef")}>
            write
          </button>
          <span data-testid="mutation-status">{mutation.status}</span>
          <span data-testid="mutation-error">{mutation.error?.name}</span>
        </div>
      );
    }
    const root = createRootRoute({ component: app.shell.AppShell });
    const index = createRoute({ getParentRoute: () => root, path: "/", component: Harness });
    const router = createRouter({
      routeTree: root.addChildren([index]),
      history: createMemoryHistory({ initialEntries: ["/"] }),
    });
    const queryClient = newQueryClient();
    // A stale cached version must never become the precondition.
    queryClient.setQueryData(app.mutation.modelVersionQueryKey, { ...VERSION, semantic_hash: "psg:sha256:stale" });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    renderRouter(router, queryClient);
    const user = userEvent.setup();

    await user.click(await screen.findByRole("button", { name: "write" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("The model has changed")).toBeTruthy();
    expect(within(dialog).getByTestId("conflict-revision").textContent).toBe(rejected.revision);
    expect(within(dialog).getByTestId("conflict-semantic-hash").textContent).toBe(rejected.semantic_hash);
    expect(ifMatch).toEqual([VERSION.semantic_hash]);
    expect(counts.version).toBe(1);
    expect(invalidate).toHaveBeenCalled();
    await waitFor(() => expect(screen.getByTestId("mutation-status").textContent).toBe("error"));
    expect(screen.getByTestId("mutation-error").textContent).toBe("PreconditionFailedError");

    // Refreshing is explicit and never replays the rejected write.
    await user.click(within(dialog).getByRole("button", { name: "Refresh latest" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(ifMatch).toHaveLength(1);

    // Only a new explicit action writes again; dismissing does not replay it either.
    await user.click(screen.getByRole("button", { name: "write" }));
    const second = await screen.findByRole("dialog");
    expect(ifMatch).toHaveLength(2);
    await user.click(within(second).getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(ifMatch).toHaveLength(2);
    expect(app.mutation.useConflictStore.getState().conflict).toBeNull();
  });
});

// ------------------------------------------------------------------ design events

describe("HomePage design events", () => {
  it("invalidates server queries on a valid event and ignores malformed ones", async () => {
    server.use(...shellHandlers());
    const { queryClient, view } = renderApp();
    await screen.findByTestId("home-revision");

    expect(FakeWebSocket.instances).toHaveLength(1);
    const [socket] = FakeWebSocket.instances;
    if (socket === undefined) {
      throw new Error("the shell opened no event socket");
    }
    expect(socket.url).toBe(`ws://${window.location.host}/api/events`);
    expect(app.events.eventsUrl({ protocol: "https:", host: "plumb.example" })).toBe("wss://plumb.example/api/events");

    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const setData = vi.spyOn(queryClient, "setQueryData");
    act(() => {
      socket.deliver(JSON.stringify({ kind: "accept_proposal", ids: ["req:1"], version: "rev:9:cccccccccccccccc" }));
    });
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(setData).not.toHaveBeenCalled();
    await waitFor(() => expect(queryClient.getQueryData(app.mutation.modelVersionQueryKey)).toEqual(VERSION));

    for (const malformed of [
      "not json",
      "[1]",
      JSON.stringify({ kind: 1, ids: [], version: "v" }),
      JSON.stringify({ kind: "k", ids: "req:1", version: "v" }),
      JSON.stringify({ kind: "k", ids: [1], version: "v" }),
      JSON.stringify({ kind: "k", ids: [] }),
      new ArrayBuffer(4),
    ]) {
      expect(() => {
        act(() => {
          socket.deliver(malformed);
        });
      }).not.toThrow();
    }
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(setData).not.toHaveBeenCalled();
    expect(queryClient.getQueryData(app.mutation.modelVersionQueryKey)).toEqual(VERSION);

    view.unmount();
    expect(socket.closed).toBe(true);
  });
});

// ------------------------------------------------------------------ roles

describe("HomePage role membership", () => {
  it("uses exactly the server-returned roles", () => {
    const me: Me = { user: "alice", mode: "pilot", roles: ["analyst"] };
    expect(app.auth.hasRole(me, "analyst")).toBe(true);
    expect(app.auth.hasRole(me, "architect")).toBe(false);
    expect(app.auth.hasRole(me, "Analyst")).toBe(false);
    expect(app.auth.hasRole(undefined, "analyst")).toBe(false);
    expect(app.auth.hasAnyRole(me, ["architect", "analyst"])).toBe(true);
    expect(app.auth.hasAnyRole(me, ["architect", "business"])).toBe(false);
    expect(app.auth.hasAnyRole(me, [])).toBe(false);
  });
});
