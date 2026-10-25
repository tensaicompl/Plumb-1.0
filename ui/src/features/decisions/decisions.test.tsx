// U2.7 Decisions tests (Hotfix 082): /decisions renders the GET /api/decisions records read-only,
// each labeled Resolution decision with its exact fields; the page states that the pilot API
// exposes ResolutionDecision records and that architecture/business decisions are distinct and
// not exposed; no type is inferred from IDs or answer text, and nothing is written.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render, screen } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { server } from "../../test/msw";

type Decision = components["schemas"]["Decision"];
type DecisionPage = components["schemas"]["DecisionPage"];

let routerModule: typeof import("../../app/router");

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

interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
}

let calls: Call[] = [];

beforeAll(async () => {
  server.listen({ onUnhandledRequest: "error" });
  server.events.on("request:start", ({ request }) => {
    const url = new URL(request.url);
    calls.push({ method: request.method, path: decodeURIComponent(url.pathname), search: url.searchParams });
  });
  vi.stubGlobal("Request", OriginRequest);
  vi.stubGlobal("WebSocket", FakeWebSocket);
  vi.stubGlobal("scrollTo", () => undefined);
  routerModule = await import("../../app/router");
});

beforeEach(() => {
  calls = [];
});

afterAll(() => {
  server.close();
  vi.unstubAllGlobals();
});

afterEach(() => {
  cleanup();
  server.resetHandlers();
  vi.restoreAllMocks();
});

// IDs and refs deliberately mention architecture/business: they never decide the type.
const DECISIONS: Decision[] = [
  {
    id: "decision:architecture-choice",
    proposal_ref: "proposal:business-rules",
    question_ref: "question:performer",
    decided_by: "actor:hr-lead",
    decided_at: "2026-03-01T09:00:00Z",
    rationale: "Managers approve leave.",
  },
  { id: "decision:business-waiver", proposal_ref: null, question_ref: null, decided_by: "actor:reviewer", decided_at: "2026-03-02T10:30:00Z", rationale: null },
];
const PAGE: DecisionPage = { items: DECISIONS, next_cursor: null };

async function renderDecisions() {
  server.use(
    http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles: ["analyst"] })),
    http.get("*/api/model/version", () =>
      HttpResponse.json({ version: "rev:1", revision: "rev:1", hash: "psg:1", semantic_hash: "psg:1", evidence_hash: "ev:1" }),
    ),
    http.get("*/api/decisions", () => HttpResponse.json(PAGE)),
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: ["/quality/history"] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByText("decision:architecture-choice");
  return router;
}

const row = (id: string) => document.querySelector("[data-decision-id='" + id + "']") as HTMLElement;
const fieldOf = (element: HTMLElement, name: string) => (element.querySelector("[data-field='" + name + "'] dd") as HTMLElement).textContent;

describe("DecisionsPage", () => {
  it("renders every record as a Resolution decision with its exact fields", async () => {
    const router = await renderDecisions();
    expect(router.state.location.pathname).toBe("/quality/history");
    expect(document.querySelectorAll("[data-decision-id]")).toHaveLength(2);
    // The type is never inferred from IDs, refs or rationale.
    expect([...document.querySelectorAll("[data-testid='decision-type']")].map((e) => e.textContent)).toEqual(["Resolution decision", "Resolution decision"]);
    const first = row("decision:architecture-choice");
    expect(fieldOf(first, "id")).toBe("decision:architecture-choice");
    expect(fieldOf(first, "proposal_ref")).toBe("proposal:business-rules");
    expect(fieldOf(first, "question_ref")).toBe("question:performer");
    expect(fieldOf(first, "decided_by")).toBe("actor:hr-lead");
    expect(fieldOf(first, "decided_at")).toBe("2026-03-01T09:00:00Z");
    expect(fieldOf(first, "rationale")).toBe("Managers approve leave.");
    const second = row("decision:business-waiver");
    expect(fieldOf(second, "proposal_ref")).toBe("None");
    expect(fieldOf(second, "question_ref")).toBe("None");
    expect(fieldOf(second, "rationale")).toBe("None");
    expect(document.body.textContent).not.toMatch(/Architecture decision|Business decision/);
  });

  it("states the ResolutionDecision-only boundary without fabricated records", async () => {
    await renderDecisions();
    const boundary = screen.getByTestId("decisions-boundary").textContent ?? "";
    expect(boundary).toContain("ResolutionDecision");
    expect(boundary).toMatch(/Architecture and business decisions are distinct/);
    expect(boundary).toContain("not exposed by this view");
    expect(document.querySelectorAll("[data-decision-id]")).toHaveLength(DECISIONS.length);
  });

  it("reads GET /api/decisions only and writes nothing", async () => {
    await renderDecisions();
    expect(calls.every((c) => c.method === "GET")).toBe(true);
    const decisions = calls.filter((c) => c.path === "/api/decisions");
    expect(decisions).toHaveLength(1);
    expect(decisions[0]?.search.toString()).toBe("");
    // No proposal action is ever called (the shell's own pending-proposal count is a plain read).
    expect(calls.filter((c) => c.path.startsWith("/api/proposals/"))).toEqual([]);
    expect(document.querySelectorAll("main form, main input, main textarea, main select, main button")).toHaveLength(0);
  });
});
