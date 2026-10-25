// U1.1 behaviour tests: Home inside the shell, the versioned-write 412 path (the conflict sheet),
// read retries, DesignEvent invalidation, locale selection and exact role membership. Every HTTP call is answered by MSW and the
// WebSocket is an in-process fake; nothing touches the network.
//
// The generated client captures `fetch` and `Request` when its module is first evaluated, so the
// app modules are imported dynamically after MSW is listening. Node's `Request` cannot parse the
// relative `/api/...` URLs a browser resolves against its origin, so the test resolves them
// against the jsdom location the same way.

import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
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

  open(): void {
    this.dispatchEvent(new Event("open"));
  }

  /** The connection ends from the server or network side (e.g. 4001 events missed). */
  drop(code: number): void {
    this.dispatchEvent(new CloseEvent("close", { code }));
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

const ME: Me = { user: "alice", mode: "pilot", roles: ["analyst", "reviewer"], inference_available: true };

const VERSION: ModelVersion = {
  version: "rev:7:aaaaaaaaaaaaaaaa",
  revision: "rev:7:aaaaaaaaaaaaaaaa",
  hash: "psg:sha256:current",
  semantic_hash: "psg:sha256:current",
  evidence_hash: "ev:sha256:evidence",
  created_at: "2026-05-01T12:00:00.000000000Z",
  created_by: "agent:cli",
};

const GATES: GateStatus[] = [
  { gate: "I0", state: "current", reason: null, passed: true, findings: [], rules: [], waivers: [], summary: { blocker_failed: 0, blocker_waived: 0, warnings: 0 } },
  {
    gate: "F1",
    state: "current",
    reason: null,
    passed: false,
    findings: ["fnd:0123456789abcdef"],
    rules: [
      {
        rule_id: "PLUMB.F1.REQ.NO_CONTRADICTION",
        title: null,
        state: "FAIL",
        declared_severity: "blocker",
        effective_severity: "blocker",
        applicability: { state: "APPLICABLE", reason: null },
        targets: ["req:hr-001"],
        evidence: [],
        semantic_condition_key: "contradiction",
        finding_ref: "fnd:0123456789abcdef",
        waiver_ref: null,
        error: null,
      },
    ],
    waivers: [],
    summary: { blocker_failed: 1, blocker_waived: 0, warnings: 0 },
  },
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

// ------------------------------------------------------------------ Home (inside the new shell)

// The shell itself (sidebar, toolbar, palette, settings, routes) is covered by src/app/shell.test.tsx.
describe("HomePage", () => {
  it("renders the current revision and I0-F4 from fixture data on /overview, with / redirecting there", async () => {
    server.use(...shellHandlers());
    const { router } = renderApp();

    expect(await screen.findByTestId("home-revision")).toHaveProperty("textContent", VERSION.revision);
    expect(router.state.location.pathname).toBe("/overview");
    expect(screen.getByTestId("home-semantic-hash").textContent).toBe(VERSION.semantic_hash);
    expect(within(screen.getByRole("navigation", { name: "Main navigation" })).getByRole("link", { name: "Overview" }).getAttribute("aria-current")).toBe("page");

    // I0-F4 in order: returned pass/fail, absent gates unevaluated (never fail).
    const rows = Array.from(document.querySelectorAll("tr[data-gate]"));
    expect(rows.map((r) => [r.getAttribute("data-gate"), r.querySelector("td")?.textContent])).toEqual([
      ["I0", "Pass"],
      ["F1", "Fail"],
      ["F2", "Unevaluated"],
      ["F3", "Unevaluated"],
      ["F4", "Unevaluated"],
    ]);
    expect(app.nav.PILOT_GATES).toEqual(["I0", "F1", "F2", "F3", "F4"]);
  });
});

// ------------------------------------------------------------------ versioned writes

describe("HomePage versioned mutation", () => {
  it("sends the ETag of the reviewed read once, never fetches a version inside the mutation, and surfaces a 412 without retrying", async () => {
    const counts = { version: 0, reviewed: 0 };
    const ifMatch: (string | null)[] = [];
    const versionsAtWrite: number[] = [];
    const rejected: PreconditionFailed = {
      code: "E_PRECONDITION_FAILED",
      message: "the branch head moved",
      details: {},
      revision: "rev:8:bbbbbbbbbbbbbbbb",
      semantic_hash: "psg:sha256:newer",
    };
    server.use(
      ...shellHandlers(counts),
      http.get("*/api/requirements/:id", () => {
        counts.reviewed += 1;
        return HttpResponse.json({ id: "req:1" }, { headers: { ETag: '"rev:6:reviewedreviewed"' } });
      }),
      http.post("*/api/proposals/:id/reject", ({ request }) => {
        ifMatch.push(request.headers.get("If-Match"));
        versionsAtWrite.push(counts.version);
        return HttpResponse.json(rejected, { status: 412 });
      }),
    );

    function Harness() {
      const reviewed = useQuery({
        queryKey: ["reviewed"],
        queryFn: () => app.client.readReviewed(app.client.apiClient.GET("/api/requirements/{id}", { params: { path: { id: "req:1" } } })),
      });
      const mutation = app.mutation.useVersionedMutation(
        (id: string, version: string) =>
          app.client.apiClient.POST("/api/proposals/{id}/reject", {
            params: { path: { id }, header: { "If-Match": version } },
            body: {},
          }),
        reviewed.data?.etag,
      );
      return (
        <div>
          <button type="button" disabled={reviewed.data === undefined} onClick={() => mutation.mutate("prp:0123456789abcdef")}>
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
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    renderRouter(router, queryClient);
    const user = userEvent.setup();

    const write = await screen.findByRole("button", { name: "write" });
    await waitFor(() => expect((write as HTMLButtonElement).disabled).toBe(false));
    const versionsBefore = counts.version;
    await user.click(write);
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("This changed since you looked")).toBeTruthy();
    expect(within(dialog).getByTestId("conflict-revision").textContent).toBe(rejected.revision);
    expect(within(dialog).getByTestId("conflict-semantic-hash").textContent).toBe(rejected.semantic_hash);
    // The precondition is exactly the reviewed read's ETag; no version is fetched to make it.
    expect(ifMatch).toEqual(['"rev:6:reviewedreviewed"']);
    // The shell may refresh its version display after the 412 invalidation, never before the write.
    expect(versionsAtWrite).toEqual([versionsBefore]);
    expect(invalidate).toHaveBeenCalled();
    await waitFor(() => expect(screen.getByTestId("mutation-status").textContent).toBe("error"));
    expect(screen.getByTestId("mutation-error").textContent).toBe("PreconditionFailedError");

    // Refreshing is explicit and never replays the rejected write.
    await user.click(within(dialog).getByRole("button", { name: /Review again/ }));
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

  it("shows a typed transport failure when the write never reaches the server", async () => {
    server.use(
      ...shellHandlers(),
      http.post("*/api/proposals/:id/reject", () => HttpResponse.error()),
    );
    function Harness() {
      const mutation = app.mutation.useVersionedMutation(
        (id: string, version: string) =>
          app.client.apiClient.POST("/api/proposals/{id}/reject", { params: { path: { id }, header: { "If-Match": version } }, body: {} }),
        '"rev:7:aaaaaaaaaaaaaaaa"',
      );
      return (
        <div>
          <button type="button" onClick={() => mutation.mutate("prp:0123456789abcdef")}>
            write
          </button>
          <span data-testid="mutation-error">{mutation.error === null ? "" : app.mutation.writeErrorMessage(mutation.error)}</span>
          <span data-testid="mutation-code">{mutation.error instanceof app.mutation.WriteFailedError ? String((mutation.error.body as { code: string }).code) : ""}</span>
        </div>
      );
    }
    const root = createRootRoute({ component: app.shell.AppShell });
    const index = createRoute({ getParentRoute: () => root, path: "/", component: Harness });
    const router = createRouter({ routeTree: root.addChildren([index]), history: createMemoryHistory({ initialEntries: ["/"] }) });
    renderRouter(router, newQueryClient());
    await userEvent.setup().click(await screen.findByRole("button", { name: "write" }));
    await waitFor(() => expect(screen.getByTestId("mutation-code").textContent).toBe("E_TRANSPORT_FAILED"));
    expect(screen.getByTestId("mutation-error").textContent).toBe("The server could not be reached; the change was not saved. Check the connection and try again.");
  });
});

describe("HomePage reads", () => {
  it("retries only network failures and 502/503/504, at most twice, and keeps the typed error", async () => {
    const { shouldRetryRead, ReadFailedError, readReviewed } = app.client;
    const failure = (status: number) => new ReadFailedError(status, { code: "E_INTERNAL", message: "m", details: {} });
    expect(shouldRetryRead(0, new TypeError("Failed to fetch"))).toBe(true);
    for (const status of [502, 503, 504]) {
      expect(shouldRetryRead(0, failure(status))).toBe(true);
      expect(shouldRetryRead(1, failure(status))).toBe(true);
      expect(shouldRetryRead(2, failure(status))).toBe(false);
    }
    for (const status of [400, 404, 409, 412, 422, 500]) {
      expect(shouldRetryRead(0, failure(status))).toBe(false);
    }
    server.use(http.get("*/api/requirements/:id", () => HttpResponse.json({ code: "E_NOT_FOUND", message: "no requirement req:9", details: {} }, { status: 404 })));
    const error = await readReviewed(app.client.apiClient.GET("/api/requirements/{id}", { params: { path: { id: "req:9" } } })).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ReadFailedError);
    expect((error as InstanceType<typeof ReadFailedError>).code).toBe("E_NOT_FOUND");
    expect((error as InstanceType<typeof ReadFailedError>).serverMessage).toBe("no requirement req:9");
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
    await waitFor(() => expect(queryClient.getQueryData(app.mutation.modelVersionQueryKey)).toMatchObject(VERSION));

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
    expect(queryClient.getQueryData(app.mutation.modelVersionQueryKey)).toMatchObject(VERSION);

    view.unmount();
    expect(socket.closed).toBe(true);
  });
});

describe("HomePage event stream lifecycle", () => {
  it("backs off from 1 s doubling to a 30 s cap with jitter", () => {
    const { reconnectDelay } = app.events;
    expect([0, 1, 2, 3, 4, 5, 6, 10].map((n) => reconnectDelay(n, () => 1))).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]);
    expect(reconnectDelay(0, () => 0)).toBe(500);
    expect(reconnectDelay(10, () => 0)).toBe(15000);
  });

  it("reconnects after a close or a lag close, invalidates once per reconnect and stops on unmount", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    server.use(...shellHandlers());
    const { queryClient, view } = renderApp();
    await screen.findByTestId("home-revision");
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const [first] = FakeWebSocket.instances;
    if (first === undefined) {
      throw new Error("the shell opened no event socket");
    }
    act(() => first.open());
    expect(invalidate).not.toHaveBeenCalled();

    // Network loss: no immediate reconnect, a new socket after the first backoff step.
    act(() => first.drop(1006));
    expect(FakeWebSocket.instances).toHaveLength(1);
    await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2), { timeout: 2000 });
    const second = FakeWebSocket.instances[1] as FakeWebSocket;
    expect(invalidate).not.toHaveBeenCalled();
    act(() => second.open());
    expect(invalidate).toHaveBeenCalledTimes(1);

    // The server's events-missed close: one more reconnect and exactly one more invalidation.
    act(() => second.drop(4001));
    await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(3), { timeout: 2000 });
    const third = FakeWebSocket.instances[2] as FakeWebSocket;
    act(() => third.open());
    expect(invalidate).toHaveBeenCalledTimes(2);

    // Unmounting closes the socket and never reconnects.
    view.unmount();
    expect(third.closed).toBe(true);
    act(() => third.drop(1006));
    await new Promise((resolve) => setTimeout(resolve, 700));
    expect(FakeWebSocket.instances).toHaveLength(3);
    vi.restoreAllMocks();
  });
});

describe("HomePage locale selection", () => {
  it("defaults to the browser language, switches to Polish, persists it per browser and renders routes in it", async () => {
    const i18n = await import("../../i18n");
    window.localStorage.removeItem(i18n.LOCALE_STORAGE_KEY);
    expect(i18n.initialLocale()).toBe("en");
    const language = vi.spyOn(window.navigator, "language", "get").mockReturnValue("pl-PL");
    expect(i18n.initialLocale()).toBe("pl");
    language.mockReturnValue("de-DE");
    expect(i18n.initialLocale()).toBe("en");
    language.mockRestore();

    server.use(
      ...shellHandlers(),
      http.get("*/api/readiness", () =>
        HttpResponse.json({ overall: null, contributing: [], sub: { clarity: null, completeness: null, confirmation: null, consistency: null, verifiability: null } }),
      ),
    );
    const { router } = renderApp();
    await screen.findByTestId("home-revision");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(within(await screen.findByRole("radiogroup", { name: "Language" })).getByRole("radio", { name: "Polski" }));
    await user.keyboard("{Escape}");
    expect(await screen.findByRole("heading", { level: 1, name: "Strona główna" })).toBeTruthy();
    expect(window.localStorage.getItem(i18n.LOCALE_STORAGE_KEY)).toBe("pl");
    expect(document.documentElement.lang).toBe("pl");
    expect(i18n.initialLocale()).toBe("pl");
    // Another route renders in Polish too.
    act(() => {
      void router.navigate({ to: "/quality" });
    });
    expect(await screen.findByRole("heading", { level: 1, name: "Jakość" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Ustawienia" }));
    await user.click(within(await screen.findByRole("radiogroup", { name: "Język" })).getByRole("radio", { name: "English" }));
    await user.keyboard("{Escape}");
    expect(await screen.findByRole("heading", { level: 1, name: "Quality" })).toBeTruthy();
    window.localStorage.removeItem(i18n.LOCALE_STORAGE_KEY);
  });
});

// ------------------------------------------------------------------ roles

describe("HomePage role membership", () => {
  it("uses exactly the server-returned roles", () => {
    const me: Me = { user: "alice", mode: "pilot", roles: ["analyst"], inference_available: true };
    expect(app.auth.hasRole(me, "analyst")).toBe(true);
    expect(app.auth.hasRole(me, "architect")).toBe(false);
    expect(app.auth.hasRole(me, "Analyst")).toBe(false);
    expect(app.auth.hasRole(undefined, "analyst")).toBe(false);
    expect(app.auth.hasAnyRole(me, ["architect", "analyst"])).toBe(true);
    expect(app.auth.hasAnyRole(me, ["architect", "business"])).toBe(false);
    expect(app.auth.hasAnyRole(me, [])).toBe(false);
  });
});
