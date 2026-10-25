// A route-level test harness (Model and Scenarios screens): renders the real router at a path,
// answers every HTTP call with MSW and records each request so tests can assert the reads and the
// reviewed If-Match of writes. The generated client captures `fetch` and `Request` when its
// module is first evaluated, so the app modules are imported after MSW is listening.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { type RequestHandler } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, vi } from "vitest";

import { isShellRead, server } from "./msw";

/** One recorded request. */
export interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

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

type RouterModule = typeof import("../app/router");
type MutationModule = typeof import("../api/useVersionedMutation");
type ToastModule = typeof import("../components/ui/Toast");

export interface RouteHarness {
  /** Every request since the test started (shell reads included). */
  readonly calls: Call[];
  /** Renders the app at `path` with `handlers` taking precedence over the shell fallbacks. */
  renderAt(path: string, ...handlers: RequestHandler[]): Promise<{ user: ReturnType<typeof userEvent.setup>; router: ReturnType<RouterModule["createAppRouter"]> }>;
  /** Non-GET requests. */
  writes(): Call[];
  /** GET requests other than the shell's own reads. */
  reads(): Call[];
  mutation(): MutationModule;
  toast(): ToastModule;
}

/** Registers the lifecycle hooks and returns the harness (call once at the top of a test file). */
export function setupRouteHarness(): RouteHarness {
  const calls: Call[] = [];
  let routerModule: RouterModule;
  let mutationModule: MutationModule;
  let toastModule: ToastModule;

  beforeAll(async () => {
    server.listen({ onUnhandledRequest: "error" });
    vi.stubGlobal("Request", OriginRequest);
    vi.stubGlobal("WebSocket", FakeWebSocket);
    vi.stubGlobal("scrollTo", () => undefined);
    routerModule = await import("../app/router");
    mutationModule = await import("../api/useVersionedMutation");
    toastModule = await import("../components/ui/Toast");
  });

  beforeEach(() => {
    calls.length = 0;
    server.events.on("request:start", ({ request }) => {
      const url = new URL(request.url);
      const record = (body: unknown) =>
        calls.push({ method: request.method, path: decodeURIComponent(url.pathname), search: url.searchParams, ifMatch: request.headers.get("if-match"), body });
      if (request.method === "GET") {
        record(null);
        return;
      }
      void request
        .clone()
        .text()
        .then((text) => {
          let body: unknown = text;
          try {
            body = text === "" ? null : (JSON.parse(text) as unknown);
          } catch {
            // Non-JSON bodies are kept as text.
          }
          record(body);
        });
    });
  });

  afterEach(() => {
    cleanup();
    server.events.removeAllListeners();
    server.resetHandlers();
    mutationModule.useConflictStore.getState().clearConflict();
    toastModule.dismissToast();
    vi.restoreAllMocks();
  });

  afterAll(() => {
    server.close();
    vi.unstubAllGlobals();
  });

  return {
    calls,
    async renderAt(path, ...handlers) {
      server.use(...handlers);
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
      const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: [path] }));
      render(
        <QueryClientProvider client={queryClient}>
          <RouterProvider router={router} />
        </QueryClientProvider>,
      );
      return { user: userEvent.setup(), router };
    },
    writes: () => calls.filter((call) => call.method !== "GET"),
    reads: () => calls.filter((call) => call.method === "GET" && !isShellRead(call.path)),
    mutation: () => mutationModule,
    toast: () => toastModule,
  };
}
