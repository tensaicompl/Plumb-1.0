// U2.7 Gates tests (Hotfix 082): /gates renders I0-F4 in order from GET /api/gates, an absent
// gate is Unevaluated, every rule result shows its exact server state (Pass, Fail, Warning, Not
// applicable, Waived, Error) with its material, waivers and the server summary are shown as
// returned, readiness is a separate advisory and uncalibrated section that never changes a gate
// or rule, and no compliance claim exists. Every request is recorded; nothing touches the network.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render, screen, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { server } from "../../test/msw";

type GateStatus = components["schemas"]["GateStatus"];
type GateRuleResult = components["schemas"]["GateRuleResult"];
type Readiness = components["schemas"]["Readiness"];

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

/** Every request the app makes, handled or not. */
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

// ------------------------------------------------------------------ fixtures

const rule = (rule_id: string, state: GateRuleResult["state"], extra: Partial<GateRuleResult> = {}): GateRuleResult => ({
  rule_id,
  state,
  declared_severity: "blocker",
  effective_severity: "blocker",
  applicability: { state: "APPLICABLE", reason: null },
  targets: [],
  evidence: [],
  semantic_condition_key: null,
  finding_ref: null,
  waiver_ref: null,
  error: null,
  ...extra,
});

// I0 passes overall with one waived blocker; F1 fails with every other state; F4 passes.
const I0: GateStatus = {
  gate: "I0",
  passed: true,
  findings: ["fnd:waived"],
  rules: [
    rule("PLUMB.I0.SOURCE.CONTENT_ADDRESSED", "PASS"),
    rule("PLUMB.I0.EVIDENCE.LOCATABLE", "WAIVED", {
      targets: ["evd:1"],
      evidence: ["locator:p1"],
      semantic_condition_key: "locator_missing",
      finding_ref: "fnd:waived",
      waiver_ref: "decision:waiver-1",
    }),
  ],
  waivers: [{ rule_id: "PLUMB.I0.EVIDENCE.LOCATABLE", finding_ref: "fnd:waived", finding_key: "fk:sha256:abc", decision_ref: "decision:waiver-1" }],
  summary: { blocker_failed: 0, blocker_waived: 1, warnings: 0 },
};

const F1: GateStatus = {
  gate: "F1",
  passed: false,
  findings: ["fnd:fail", "fnd:warn"],
  rules: [
    rule("ISO29148.F1.REQ.STATEMENT_PRESENT", "FAIL", {
      declared_severity: "error",
      effective_severity: "blocker",
      targets: ["req:hr-001"],
      evidence: ["statement:empty"],
      semantic_condition_key: "statement_missing",
      finding_ref: "fnd:fail",
    }),
    rule("PLUMB.F1.REQ.TERMS_RESOLVED", "WARN", { declared_severity: "warn", effective_severity: "warn", finding_ref: "fnd:warn" }),
    rule("PLUMB.F1.REQ.LINEAGE", "NOT_APPLICABLE", { applicability: { state: "NOT_APPLICABLE", reason: "no accepted requirement in scope" } }),
    rule("PLUMB.F1.REQ.NO_CONTRADICTION", "ERROR", {
      declared_severity: "error",
      effective_severity: "error",
      error: { code: "F1_LINT_EVALUATION", message: "The lint input could not be evaluated.", targets: ["req:hr-002"], evidence: [] },
    }),
  ],
  waivers: [],
  summary: { blocker_failed: 1, blocker_waived: 0, warnings: 1 },
};

const F4: GateStatus = { gate: "F4", passed: true, findings: [], rules: [], waivers: [], summary: { blocker_failed: 0, blocker_waived: 0, warnings: 0 } };

const READINESS: Readiness = {
  overall: 0.99,
  contributing: [],
  sub: { clarity: null, completeness: null, confirmation: null, consistency: null, verifiability: null },
};

function handlers(gates: GateStatus[], readiness: Readiness = READINESS) {
  return [
    http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles: ["analyst"] })),
    http.get("*/api/model/version", () =>
      HttpResponse.json({ version: "rev:1", revision: "rev:1", hash: "psg:1", semantic_hash: "psg:1", evidence_hash: "ev:1" }),
    ),
    http.get("*/api/gates", () => HttpResponse.json(gates)),
    http.get("*/api/readiness", () => HttpResponse.json(readiness)),
  ];
}

async function renderGates(gates: GateStatus[] = [I0, F1, F4], readiness?: Readiness) {
  server.use(...handlers(gates, readiness));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: ["/gates"] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByText("PLUMB.I0.SOURCE.CONTENT_ADDRESSED");
  return router;
}

const gate = (name: string) => document.querySelector("main [data-gate='" + name + "']") as HTMLElement;
const ruleOf = (id: string) => document.querySelector("[data-rule-id='" + id + "']") as HTMLElement;
const stateOf = (id: string) => within(ruleOf(id)).getByTestId("rule-state").textContent;
const fieldOf = (element: HTMLElement, name: string) => element.querySelector("[data-field='" + name + "'] dd") as HTMLElement | null;

// ------------------------------------------------------------------ tests

describe("GatesPage", () => {
  it("renders I0-F4 in order and an absent gate as Unevaluated without synthetic rules", async () => {
    const router = await renderGates();
    expect(router.state.location.pathname).toBe("/gates");
    expect([...document.querySelectorAll("main [data-gate]")].map((li) => li.getAttribute("data-gate"))).toEqual(["I0", "F1", "F2", "F3", "F4"]);
    for (const name of ["F2", "F3"]) {
      expect(within(gate(name)).getByTestId("gate-result").textContent).toBe("Unevaluated");
      expect(gate(name).querySelectorAll("[data-rule-id], [data-testid='gate-summary'], [data-testid='gate-waivers']")).toHaveLength(0);
    }
    expect(within(gate("I0")).getByTestId("gate-result").textContent).toBe("Pass");
    expect(within(gate("F1")).getByTestId("gate-result").textContent).toBe("Fail");
    expect(within(gate("F4")).getByTestId("gate-result").textContent).toBe("Pass");
    // Rules stay in report order.
    expect([...gate("F1").querySelectorAll("[data-rule-id]")].map((li) => li.getAttribute("data-rule-id"))).toEqual(F1.rules.map((r) => r.rule_id));
  });

  it("shows all six rule states distinctly with their exact material", async () => {
    await renderGates();
    expect(stateOf("PLUMB.I0.SOURCE.CONTENT_ADDRESSED")).toBe("Pass");
    expect(stateOf("ISO29148.F1.REQ.STATEMENT_PRESENT")).toBe("Fail");
    expect(stateOf("PLUMB.F1.REQ.TERMS_RESOLVED")).toBe("Warning");
    expect(stateOf("PLUMB.F1.REQ.LINEAGE")).toBe("Not applicable");
    expect(stateOf("PLUMB.I0.EVIDENCE.LOCATABLE")).toBe("Waived");
    expect(stateOf("PLUMB.F1.REQ.NO_CONTRADICTION")).toBe("Error");
    expect(new Set([...document.querySelectorAll("[data-testid='rule-state']")].map((e) => e.textContent)).size).toBe(6);
    // Not applicable with its exact reason.
    expect(fieldOf(ruleOf("PLUMB.F1.REQ.LINEAGE"), "applicability_reason")?.textContent).toBe("no accepted requirement in scope");
    // Error with its code and message.
    const error = ruleOf("PLUMB.F1.REQ.NO_CONTRADICTION");
    expect(within(error).getByTestId("error-code").textContent).toBe("F1_LINT_EVALUATION");
    expect(within(error).getByTestId("error-message").textContent).toBe("The lint input could not be evaluated.");
    // Effective severity everywhere; the declared one too when it differs.
    const fail = ruleOf("ISO29148.F1.REQ.STATEMENT_PRESENT");
    expect(fieldOf(fail, "effective_severity")?.textContent).toBe("blocker");
    expect(fieldOf(fail, "declared_severity")?.textContent).toBe("error");
    expect(fieldOf(ruleOf("PLUMB.F1.REQ.TERMS_RESOLVED"), "effective_severity")?.textContent).toBe("warn");
    expect(fieldOf(ruleOf("PLUMB.F1.REQ.TERMS_RESOLVED"), "declared_severity")).toBeNull();
    // References as plain server material.
    expect(fieldOf(fail, "finding_ref")?.textContent).toBe("fnd:fail");
    expect(fieldOf(fail, "semantic_condition_key")?.textContent).toBe("statement_missing");
    expect(fieldOf(fail, "targets")?.textContent).toBe("req:hr-001");
    expect(fieldOf(fail, "evidence")?.textContent).toBe("statement:empty");
    expect(fail.querySelector("a")).toBeNull();
  });

  it("keeps a waived rule Waived, never Pass, with its finding, waiver and decision", async () => {
    await renderGates();
    expect(within(gate("I0")).getByTestId("gate-result").textContent).toBe("Pass");
    const waived = ruleOf("PLUMB.I0.EVIDENCE.LOCATABLE");
    expect(stateOf("PLUMB.I0.EVIDENCE.LOCATABLE")).toBe("Waived");
    expect(stateOf("PLUMB.I0.EVIDENCE.LOCATABLE")).not.toBe("Pass");
    expect(fieldOf(waived, "finding_ref")?.textContent).toBe("fnd:waived");
    expect(fieldOf(waived, "waiver_ref")?.textContent).toBe("decision:waiver-1");
    const waiver = gate("I0").querySelector("[data-waiver-rule='PLUMB.I0.EVIDENCE.LOCATABLE']") as HTMLElement;
    expect(fieldOf(waiver, "finding_ref")?.textContent).toBe("fnd:waived");
    expect(fieldOf(waiver, "finding_key")?.textContent).toBe("fk:sha256:abc");
    expect(fieldOf(waiver, "decision_ref")?.textContent).toBe("decision:waiver-1");
    expect(fieldOf(within(gate("I0")).getByTestId("gate-summary"), "blocker_waived")?.textContent).toBe("1");
  });

  it("shows the server summary as returned", async () => {
    await renderGates();
    const summary = within(gate("F1")).getByTestId("gate-summary");
    expect(fieldOf(summary, "blocker_failed")?.textContent).toBe("1");
    expect(fieldOf(summary, "blocker_waived")?.textContent).toBe("0");
    expect(fieldOf(summary, "warnings")?.textContent).toBe("1");
    expect(fieldOf(summary, "findings")?.textContent).toBe("fnd:fail, fnd:warn");
  });

  it("keeps a failed blocker failed with 0.99 readiness, shown only as advisory and uncalibrated", async () => {
    await renderGates();
    expect(within(gate("F1")).getByTestId("gate-result").textContent).toBe("Fail");
    expect(stateOf("ISO29148.F1.REQ.STATEMENT_PRESENT")).toBe("Fail");
    expect(fieldOf(ruleOf("ISO29148.F1.REQ.STATEMENT_PRESENT"), "effective_severity")?.textContent).toBe("blocker");
    const readiness = screen.getByTestId("readiness");
    expect(readiness.textContent).toContain("Advisory readiness");
    expect(readiness.textContent).toContain("Uncalibrated");
    expect(fieldOf(readiness, "overall")?.textContent).toBe("0.99");
    // Null sub-scores are not calculated, never 0 or a percentage.
    expect(fieldOf(readiness, "clarity")?.textContent).toBe("Not calculated");
    expect(readiness.textContent).not.toMatch(/%|score/i);
    expect(gate("F1").contains(readiness)).toBe(false);
  });

  it("renders no compliance claim and makes only the two GET reads", async () => {
    await renderGates();
    expect(document.body.textContent).not.toMatch(/compliant|certified|certification|conformance score/i);
    expect(calls.every((c) => c.method === "GET")).toBe(true);
    const reads = new Set(calls.map((c) => c.path));
    expect([...reads].filter((p) => p !== "/api/me" && p !== "/api/model/version").sort()).toEqual(["/api/gates", "/api/readiness"]);
    expect(calls.find((c) => c.path === "/api/readiness")?.search.toString()).toBe("");
  });
});
