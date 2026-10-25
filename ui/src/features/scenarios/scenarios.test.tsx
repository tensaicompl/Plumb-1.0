// U2.5 Scenarios tests: the /model/scenarios route with server filters; cards with Given/When/Then,
// statuses and run state from the server; confirmation candidates only from the API's
// status=Proposed&derived=true query; DecisionTable bulk confirmation behind a three-sample prompt
// (one confirm-derived POST); correction through the task endpoint with the canonical Then shown
// only from server data; run scope limited to Accepted + Confirmed and sorted; business-semantic
// traces, undecidable missing elements and unknown-step fallback. Every HTTP call is answered by MSW
// and recorded; nothing touches the network.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http } from "msw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { etagJson, server, TEST_ETAG } from "../../test/msw";

type Scenario = components["schemas"]["Scenario"];

let routerModule: typeof import("../../app/router");
let mutation: typeof import("../../api/useVersionedMutation");

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
const OLD_THEN = '{"attribute":"attr:balance","value":20}';
const NEW_THEN = '{"attribute":"attr:balance","value":18}';

function scenario(id: string, status: Scenario["status"], confirmation: Scenario["confirmation_status"], when: string, derivedFrom: string[], extra: Partial<Scenario> = {}): Scenario {
  return {
    id,
    given: ['{"attribute":"attr:days","value":2}'],
    when,
    then: ['{"state":"state:approved"}'],
    status,
    confirmation_status: confirmation,
    derived_from: derivedFrom,
    run: null,
    ...extra,
  };
}

const dt = (id: string) => scenario(id, "Proposed", "Derived", '{"decision_table":"dt:leave"}', ["dt:leave", "req:hr-001"]);
const DT_A = dt("scenario:dt-a");
const DT_B = dt("scenario:dt-b");
const DT_C = dt("scenario:dt-c");
const DT_D = dt("scenario:dt-d");
const TRANSITION = scenario("scenario:tr", "Proposed", "Derived", '{"transition":"tr:approve"}', ["tr:approve"]);
const ACCEPTED_DERIVED = scenario("scenario:acc-derived", "Accepted", "Derived", '{"decision_table":"dt:leave"}', ["dt:leave"]);

const trace = (phase: string, step: Record<string, unknown>) => JSON.stringify({ phase, step });
const PASS_RUN: Scenario["run"] = {
  result: "pass",
  trace: [
    trace("execution", { step: "performer_recorded", operation_ref: "op:approve", performer_refs: ["actor:manager"] }),
    trace("execution", { step: "rule_evaluated", rule_ref: "rule:positive", result: false }),
    trace("execution", { step: "transition_applied", transition_ref: "tr:approve", from_state: "state:submitted", to_state: "state:approved" }),
    trace("comparison", { step: "state_compared", state_ref: "state:approved", matched: true }),
    trace("comparison", { step: "comparison_completed", result: "pass", diff_count: 0 }),
  ],
};
const UNKNOWN_TRACE = trace("execution", { step: "quantum_leap", target_ref: "x:1" });
const UNDECIDABLE_RUN: Scenario["run"] = {
  result: "undecidable",
  missing: ['{"key":"calendar","kind":"semantic_ref","owner_ref":"calc:working-days"}'],
  trace: [trace("execution", { step: "semantic_missing", missing: [] }), UNKNOWN_TRACE, "not json at all"],
};
const FAIL_RUN: Scenario["run"] = {
  result: "fail",
  diff: ['{"actual":"state:submitted","expected":"state:approved","kind":"state"}'],
  trace: [trace("comparison", { step: "decision_output_compared", decision_table_ref: "dt:leave", column: 2, matched: false })],
};

const R_PASS = scenario("scenario:r-pass", "Accepted", "Confirmed", '{"operation":"op:approve"}', [], { run: PASS_RUN, then: [OLD_THEN] });
const R_UNDECIDABLE = scenario("scenario:r-undecidable", "Accepted", "Confirmed", '{"calculation":"calc:working-days"}', [], { run: UNDECIDABLE_RUN });
const R_FAIL = scenario("scenario:r-fail", "Accepted", "Confirmed", '{"decision_table":"dt:leave"}', ["dt:leave"], { run: FAIL_RUN });
const R_NEVER = scenario("scenario:r-never", "Accepted", "Confirmed", '{"event":"event:requested"}', []);

const CONFIRMABLE = [DT_A, DT_B, DT_C, DT_D, TRANSITION];

interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

interface Backend {
  calls: Call[];
  corrected: boolean;
  fail: (path: string) => Response | null;
  extra?: Scenario[];
}

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
  const write = async ({ request }: { request: Request }) => {
    await record(request, "POST");
    const path = decodeURIComponent(new URL(request.url).pathname);
    const failure = backend.fail(path);
    if (failure !== null) {
      return failure;
    }
    if (path.endsWith("/correct")) {
      backend.corrected = true;
    }
    return etagJson({ version: "rev:2", revision: "rev:2", semantic_hash: CURRENT, events: [] });
  };
  return [
    http.get("*/api/me", () => etagJson({ user: "alice", mode: "pilot", roles: ["analyst"] })),
    http.get("*/api/model/version", () =>
      etagJson({ version: "rev:1", revision: "rev:1", hash: CURRENT, semantic_hash: CURRENT, evidence_hash: "ev:1" }),
    ),
    http.get("*/api/scenarios", async ({ request }) => {
      await record(request, "GET");
      const q = new URL(request.url).searchParams;
      if (q.get("status") === "Proposed" && q.get("derived") === "true" && q.get("requirement") === null) {
        return etagJson({ items: CONFIRMABLE, next_cursor: null });
      }
      const pass = backend.corrected ? { ...R_PASS, then: [NEW_THEN] } : R_PASS;
      return etagJson({ items: [...CONFIRMABLE, ACCEPTED_DERIVED, pass, R_UNDECIDABLE, R_FAIL, R_NEVER, ...(backend.extra ?? [])], next_cursor: null });
    }),
    http.post("*/api/scenarios/confirm-derived", write),
    http.post("*/api/scenarios/run", write),
    http.post("*/api/scenarios/:id/confirm", write),
    http.post("*/api/scenarios/:id/correct", write),
    // Recorded so that any generic proposal call is visible (U2.5 must never make one).
    http.post("*/api/proposals/*", write),
  ];
}

function newBackend(overrides: Partial<Backend> = {}): Backend {
  return { calls: [], corrected: false, fail: () => null, ...overrides };
}

async function renderScenarios(backend: Backend) {
  server.use(...handlers(backend));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: ["/model/scenarios"] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByText("scenario:r-pass");
  return userEvent.setup();
}

const card = (id: string) => document.querySelector("li[data-scenario-id='" + id + "']") as HTMLElement;
const posts = (backend: Backend) => backend.calls.filter((c) => c.method === "POST");
const lists = (backend: Backend) => backend.calls.filter((c) => c.method === "GET" && c.path === "/api/scenarios");

// ------------------------------------------------------------------ route, cards, filters

describe("ScenariosPage", () => {
  it("renders two identical Given and Then rows positionally, never keyed by their text (Fable #224)", async () => {
    const errors = vi.spyOn(console, "error");
    const twice = scenario("scenario:twice", "Accepted", "Confirmed", '{"operation":"op:approve"}', [], {
      given: [OLD_THEN, OLD_THEN],
      then: [NEW_THEN, NEW_THEN],
      run: { ...FAIL_RUN, diff: [OLD_THEN, OLD_THEN] } as Scenario["run"],
    });
    await renderScenarios(newBackend({ extra: [twice] }));
    const twiceCard = card("scenario:twice");
    expect(twiceCard.querySelectorAll("[data-testid='given'] > li")).toHaveLength(2);
    expect(twiceCard.querySelectorAll("[data-testid='then'] > li")).toHaveLength(2);
    expect(errors.mock.calls.some((call) => String(call[0]).includes("same key"))).toBe(false);
    errors.mockRestore();
  });

  it("renders the server scenarios on /model/scenarios with Given/When/Then, statuses and run state", async () => {
    const backend = newBackend();
    await renderScenarios(backend);
    const pass = card("scenario:r-pass");
    expect(pass.querySelector("[data-field=status]")?.textContent).toBe("Accepted");
    expect(pass.querySelector("[data-field=confirmation_status]")?.textContent).toBe("Confirmed");
    expect(within(pass).getByTestId("given").textContent).toContain("attr:days");
    expect(within(pass).getByTestId("when").textContent).toBe("operation op:approve");
    expect(within(pass).getByTestId("then").querySelector("[data-semantic-text]")?.getAttribute("data-semantic-text")).toBe(OLD_THEN);
    expect(within(pass).getByTestId("run-status").textContent).toBe("Pass");
    // A never-run scenario is not a failure.
    expect(within(card("scenario:r-never")).getByTestId("run-status").textContent).toBe("Not run");
    expect(card("scenario:r-never").querySelector("[data-testid=scenario-trace]")).toBeNull();
    expect(within(card("scenario:r-fail")).getByTestId("run-status").textContent).toBe("Fail");
    expect(card("scenario:dt-a").querySelector("[data-field=derived_from]")?.textContent).toBe("dt:leave, req:hr-001");
    // The list carries the run; no per-scenario run reads.
    expect(backend.calls.some((c) => c.path.endsWith("/run") && c.method === "GET")).toBe(false);
  });

  it("sends the requirement, status and derived filters to the server and omits blank ones", async () => {
    const backend = newBackend();
    const user = await renderScenarios(backend);
    const first = lists(backend).find((c) => !(c.search.get("status") === "Proposed" && c.search.get("derived") === "true"));
    expect([...(first?.search.keys() ?? [])]).toEqual([]);
    const form = screen.getByRole("form", { name: "Scenario filters" });
    await user.type(within(form).getByLabelText("Requirement ID"), "req:hr-001");
    await user.selectOptions(within(form).getByLabelText("Status"), "Accepted");
    await user.selectOptions(within(form).getByLabelText("Derived"), "false");
    await user.click(within(form).getByRole("button", { name: "Apply filters" }));
    await waitFor(() => expect(lists(backend).some((c) => c.search.get("requirement") === "req:hr-001")).toBe(true));
    const filtered = lists(backend).find((c) => c.search.get("requirement") === "req:hr-001");
    expect(Object.fromEntries(filtered?.search ?? [])).toEqual({ requirement: "req:hr-001", status: "Accepted", derived: "false" });
    // Status alone.
    await user.clear(within(form).getByLabelText("Requirement ID"));
    await user.selectOptions(within(form).getByLabelText("Derived"), "");
    await user.click(within(form).getByRole("button", { name: "Apply filters" }));
    await waitFor(() => expect(lists(backend).at(-1)?.search.toString()).toBe("status=Accepted"));
  });
});

// ------------------------------------------------------------------ confirmation

describe("Scenario confirmation", () => {
  it("takes confirmation candidates only from the status=Proposed&derived=true query", async () => {
    const backend = newBackend();
    await renderScenarios(backend);
    expect(lists(backend).some((c) => c.search.get("status") === "Proposed" && c.search.get("derived") === "true")).toBe(true);
    const confirmButtons = Array.from(document.querySelectorAll("[data-testid=scenario-confirm]")).map((b) => b.closest("li")?.getAttribute("data-scenario-id"));
    expect(confirmButtons.sort()).toEqual(CONFIRMABLE.map((s) => s.id).sort());
    expect(card("scenario:acc-derived").querySelector("[data-testid=scenario-confirm]")).toBeNull();
  });

  it("confirms one scenario with one versioned POST and refetches", async () => {
    const backend = newBackend();
    const user = await renderScenarios(backend);
    const before = lists(backend).length;
    await user.click(within(card("scenario:tr")).getByTestId("scenario-confirm"));
    await waitFor(() => expect(lists(backend).length).toBeGreaterThan(before));
    expect(posts(backend).map((c) => [c.path, c.ifMatch, c.body])).toEqual([["/api/scenarios/scenario:tr/confirm", TEST_ETAG, null]]);
  });

  it("groups only exact DecisionTable-derived confirmable scenarios for bulk confirmation", async () => {
    const backend = newBackend();
    await renderScenarios(backend);
    const tables = Array.from(document.querySelectorAll("[data-bulk-table]")).map((li) => li.getAttribute("data-bulk-table"));
    expect(tables).toEqual(["dt:leave"]);
    expect(document.querySelector("[data-bulk-table='tr:approve']")).toBeNull();
    expect(document.querySelector("[data-bulk-table='dt:leave']")?.textContent).toContain("4");
  });

  it("asks with three samples before one confirm-derived POST and sends nothing on cancel", async () => {
    const backend = newBackend();
    const user = await renderScenarios(backend);
    await user.click(screen.getByTestId("bulk-open"));
    const prompt = screen.getByTestId("bulk-prompt");
    expect(posts(backend)).toEqual([]);
    expect(within(prompt).getByTestId("bulk-table").textContent).toBe("dt:leave");
    expect(within(prompt).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["scenario:dt-a", "scenario:dt-b", "scenario:dt-c"]);
    expect(within(prompt).getByTestId("bulk-sample-note")).toBeTruthy();
    expect(prompt.textContent).toContain("not only the examples shown");
    await user.click(within(prompt).getByTestId("bulk-cancel"));
    expect(screen.queryByTestId("bulk-prompt")).toBeNull();
    expect(posts(backend)).toEqual([]);
    await user.click(screen.getByTestId("bulk-open"));
    await user.click(screen.getByTestId("bulk-confirm"));
    await waitFor(() => expect(posts(backend)).toHaveLength(1));
    expect(posts(backend).map((c) => [c.path, c.ifMatch, c.body])).toEqual([["/api/scenarios/confirm-derived", TEST_ETAG, { table_id: "dt:leave" }]]);
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(posts(backend)).toHaveLength(1);
  });
});

// ------------------------------------------------------------------ correction

describe("Scenario correction", () => {
  it("offers correction only for Proposed+Derived and Accepted+Confirmed scenarios", async () => {
    await renderScenarios(newBackend());
    expect(card("scenario:r-pass").querySelector("[data-testid=scenario-correct]")).not.toBeNull();
    expect(card("scenario:dt-a").querySelector("[data-testid=scenario-correct]")).not.toBeNull();
    expect(card("scenario:acc-derived").querySelector("[data-testid=scenario-correct]")).toBeNull();
  });

  it("posts the exact entered Then and shows the new Then only after the server refetch", async () => {
    const backend = newBackend();
    const user = await renderScenarios(backend);
    const shownThen = () => card("scenario:r-pass").querySelector("[data-testid=then] [data-semantic-text]")?.getAttribute("data-semantic-text");
    await user.click(within(card("scenario:r-pass")).getByTestId("scenario-correct"));
    const field = within(card("scenario:r-pass")).getByTestId("correct-then");
    await user.clear(field);
    await user.click(field);
    await user.paste(NEW_THEN);
    // The draft does not change the displayed canonical scenario.
    expect(shownThen()).toBe(OLD_THEN);
    await user.click(within(card("scenario:r-pass")).getByTestId("correct-submit"));
    await waitFor(() => expect(shownThen()).toBe(NEW_THEN));
    expect(posts(backend).map((c) => [c.path, c.ifMatch, c.body])).toEqual([["/api/scenarios/scenario:r-pass/correct", TEST_ETAG, { then: [NEW_THEN] }]]);
    expect(backend.calls.some((c) => c.path.startsWith("/api/proposals"))).toBe(false);
  });
});

// ------------------------------------------------------------------ traces

describe("Scenario traces", () => {
  it("names the missing semantic element of an undecidable run", async () => {
    await renderScenarios(newBackend());
    const missing = within(card("scenario:r-undecidable")).getByRole("region", { name: "Missing semantic values" });
    expect(missing.querySelector("[data-field=owner_ref]")?.textContent).toBe("calc:working-days");
    expect(missing.querySelector("[data-field=key]")?.textContent).toBe("calendar");
    expect(missing.querySelector("[data-field=kind]")?.textContent).toBe("semantic_ref");
    expect(missing.textContent).toContain("calc:working-days");
    expect(within(card("scenario:r-undecidable")).getByTestId("run-status").textContent).toBe("Undecidable");
  });

  it("presents execution and comparison steps with business labels and exact IDs", async () => {
    await renderScenarios(newBackend());
    const pass = card("scenario:r-pass");
    const step = (name: string) => pass.querySelector("[data-trace-step=" + name + "]") as HTMLElement;
    expect(step("performer_recorded").textContent).toContain("Performer recorded");
    expect(step("performer_recorded").textContent).toContain("op:approve");
    expect(step("performer_recorded").textContent).toContain("actor:manager");
    expect(step("transition_applied").textContent).toContain("Transition applied");
    expect(step("transition_applied").textContent).toContain("state:submitted");
    expect(step("transition_applied").textContent).toContain("state:approved");
    expect(step("rule_evaluated").textContent).toContain("Rule evaluated");
    expect(step("rule_evaluated").querySelector("[data-boolean=false]")).not.toBeNull();
    expect(step("comparison_completed").getAttribute("data-phase")).toBe("comparison");
    expect(step("comparison_completed").textContent).toContain("diff_count");
    const fail = card("scenario:r-fail");
    expect(fail.querySelector("[data-trace-step=decision_output_compared] [data-boolean=false]")).not.toBeNull();
    expect(within(fail).getByTestId("run-diff").textContent).toContain("state:approved");
  });

  it("keeps unknown or malformed trace entries visible as exact server text", async () => {
    await renderScenarios(newBackend());
    const unknown = within(card("scenario:r-undecidable")).getAllByTestId("unknown-trace-text").map((e) => e.textContent);
    expect(unknown).toEqual([UNKNOWN_TRACE, "not json at all"]);
    expect(card("scenario:r-undecidable").textContent).toContain("Unknown trace step");
  });
});

// ------------------------------------------------------------------ run scope

describe("Scenario runs", () => {
  it("runs only selected Accepted+Confirmed scenarios with a sorted scope and refetches", async () => {
    const backend = newBackend();
    const user = await renderScenarios(backend);
    expect(card("scenario:dt-a").querySelector("[data-testid=run-select]")).toBeNull();
    expect(card("scenario:acc-derived").querySelector("[data-testid=run-select]")).toBeNull();
    expect((screen.getByTestId("run-submit") as HTMLButtonElement).disabled).toBe(true);
    await user.click(within(card("scenario:r-pass")).getByTestId("run-select"));
    await user.click(within(card("scenario:r-fail")).getByTestId("run-select"));
    const before = lists(backend).length;
    await user.click(screen.getByTestId("run-submit"));
    await waitFor(() => expect(lists(backend).length).toBeGreaterThan(before));
    expect(posts(backend).map((c) => [c.path, c.ifMatch, c.body])).toEqual([
      ["/api/scenarios/run", TEST_ETAG, { scope: ["scenario:r-fail", "scenario:r-pass"] }],
    ]);
  });

  it("never sends an empty run scope", async () => {
    const backend = newBackend();
    const user = await renderScenarios(backend);
    const submit = screen.getByTestId("run-submit") as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    await user.click(submit);
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(posts(backend)).toEqual([]);
  });

  it("shows a write failure code and never retries", async () => {
    const backend = newBackend({
      fail: (path) =>
        path === "/api/scenarios/run" ? etagJson({ code: "E_SCENARIO_NOT_RUNNABLE", message: "not runnable", details: {} }, { status: 409 }) : null,
    });
    const user = await renderScenarios(backend);
    await user.click(within(card("scenario:r-never")).getByTestId("run-select"));
    await user.click(screen.getByTestId("run-submit"));
    expect((await screen.findByTestId("scenario-write-error")).textContent).toContain("E_SCENARIO_NOT_RUNNABLE");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(posts(backend).map((c) => c.path)).toEqual(["/api/scenarios/run"]);
  });
});
