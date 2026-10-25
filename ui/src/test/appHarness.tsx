// Renders the whole app (shell and routed screen) through the real router for screen tests, with
// every HTTP call answered by MSW and the live event socket replaced by an inert fake. Every
// request the app makes is recorded in `calls`.
//
// The generated client captures `fetch` and `Request` when its module is first evaluated, so the
// app modules are imported dynamically (`setupApp` in `beforeAll`) after MSW is listening, with
// relative URLs resolved against the jsdom origin.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider, type AnyRouter } from "@tanstack/react-router";
import { act, cleanup, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";

import { server } from "./msw";

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

export interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly ifMatch: string | null;
}

/** Every request the app made since the last `resetApp`, in order. */
export const calls: Call[] = [];

interface AppModules {
  router: typeof import("../app/router");
  preferences: typeof import("../app/preferences");
  run: typeof import("../app/run");
  i18n: typeof import("../i18n");
  toast: typeof import("../components/ui/Toast");
}

let modules: AppModules | null = null;

/** The app modules (after `setupApp`). */
export function app(): AppModules {
  if (modules === null) {
    throw new Error("setupApp() must run in beforeAll");
  }
  return modules;
}

/** Starts MSW (unhandled requests fail), stubs the browser globals and imports the app. */
export async function setupApp(): Promise<void> {
  server.listen({ onUnhandledRequest: "error" });
  vi.stubGlobal("Request", OriginRequest);
  vi.stubGlobal("WebSocket", FakeWebSocket);
  vi.stubGlobal("scrollTo", () => undefined);
  modules = {
    router: await import("../app/router"),
    preferences: await import("../app/preferences"),
    run: await import("../app/run"),
    i18n: await import("../i18n"),
    toast: await import("../components/ui/Toast"),
  };
}

/** Stops MSW and restores the globals. */
export function teardownApp(): void {
  server.close();
  vi.unstubAllGlobals();
}

/** Resets view state, handlers and the request log between tests. */
export function resetApp(): void {
  cleanup();
  server.resetHandlers();
  server.events.removeAllListeners();
  calls.length = 0;
  const m = app();
  act(() => m.toast.dismissToast());
  window.localStorage.clear();
  m.i18n.useLocaleStore.getState().setLocale("en");
  m.preferences.useShellStore.setState({ sidebarCollapsed: false, assistantOpen: false, overlay: null });
  m.run.useRunStore.getState().clear();
  server.events.on("request:start", ({ request }) => {
    const url = new URL(request.url);
    calls.push({ method: request.method, path: decodeURIComponent(url.pathname), search: url.searchParams, ifMatch: request.headers.get("if-match") });
  });
}

/** The file part of a `create_source` upload: its file name, media type and text. */
export async function uploadedFile(request: Request): Promise<{ name: string; type: string; text: string }> {
  const raw = await request.text();
  const name = /filename="([^"]*)"/.exec(raw)?.[1] ?? "";
  const type = /\r\nContent-Type: ([^\r\n]*)\r\n\r\n/.exec(raw)?.[1] ?? "";
  const start = raw.indexOf("\r\n\r\n") + 4;
  const end = raw.lastIndexOf("\r\n--");
  return { name, type, text: raw.slice(start, end) };
}

/** Renders the app at `path`. */
export function renderApp(path: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router: AnyRouter = app().router.createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  const view = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { queryClient, router, view, user: userEvent.setup() };
}
