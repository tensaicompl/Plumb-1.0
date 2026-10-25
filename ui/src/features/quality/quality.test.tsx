// Quality screen (handoff "6. Quality", spec §5.6, flows 4 and 5): the five gates with their state,
// reason, summary and grouped rules; Re-check (the owning step through run_stage, or a fresh
// get_gates) and Show affected; every finding with Waive with reason; the decision history with
// the assumptions and Resolve; the Export pill. Every HTTP call is answered by MSW and recorded.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { axeViolations } from "../../test/axe";
import { etagJson, server } from "../../test/msw";

type GateStatus = components["schemas"]["GateStatus"];
type GateRuleResult = components["schemas"]["GateRuleResult"];
type Finding = components["schemas"]["Finding"];
type Decision = components["schemas"]["Decision"];
type Assumption = components["schemas"]["Assumption"];
type RequirementSummary = components["schemas"]["RequirementSummary"];

let routerModule: typeof import("../../app/router");
let toastModule: typeof import("../../components/ui/Toast");
let preferences: typeof import("../../app/preferences");
let run: typeof import("../../app/run");

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
  toastModule = await import("../../components/ui/Toast");
  preferences = await import("../../app/preferences");
  run = await import("../../app/run");
});

afterAll(() => {
  server.close();
  vi.unstubAllGlobals();
});

afterEach(() => {
  cleanup();
  server.resetHandlers();
  act(() => toastModule.dismissToast());
  preferences.useShellStore.setState({ overlay: null });
  run.useRunStore.getState().clear();
});

// ------------------------------------------------------------------ fixtures

const VERSION_ETAG = '"rev:9:version"';
const FINDINGS_ETAG = '"rev:9:findings"';
const ASSUMPTIONS_ETAG = '"rev:9:assumptions"';

function rule(rule_id: string, state: GateRuleResult["state"], extra: Partial<GateRuleResult> = {}): GateRuleResult {
  return {
    rule_id,
    state,
    title: null,
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
  };
}

const REQ_IDS = Array.from({ length: 32 }, (_, i) => "req:" + String(i + 1).padStart(2, "0"));

const GATES: GateStatus[] = [
  {
    gate: "I0",
    state: "current",
    passed: true,
    reason: null,
    findings: [],
    rules: [rule("PLUMB.I0.SOURCE.CONTENT_ADDRESSED", "PASS", { title: "Every source artifact is content-addressed" })],
    waivers: [],
    summary: { blocker_failed: 0, blocker_waived: 0, warnings: 0 },
  },
  {
    gate: "F1",
    state: "current",
    passed: true,
    reason: null,
    findings: ["fnd:criteria", "fnd:dup", "fnd:lineage"],
    rules: [
      rule("ISO29148.F1.REQ.LINEAGE", "FAIL", { title: "Requirements have intent lineage", effective_severity: "error", declared_severity: "error", targets: REQ_IDS, finding_ref: "fnd:lineage" }),
      rule("PLUMB.F1.REQ.GROUNDED", "PASS", { title: "Accepted requirements are grounded in evidence or explicit human origin" }),
      rule("PLUMB.F1.REQ.NO_CONTRADICTION", "NOT_APPLICABLE", {
        title: "Accepted requirements have no unresolved blocking contradiction",
        applicability: { state: "NOT_APPLICABLE", reason: "No contradiction finding material exists." },
      }),
      rule("PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED", "WAIVED", {
        title: "Accepted requirement set has no unresolved semantic duplicates",
        targets: ["req:23", "req:24"],
        finding_ref: "fnd:dup",
        waiver_ref: "dec:waiver",
      }),
    ],
    waivers: [{ rule_id: "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED", finding_ref: "fnd:dup", finding_key: "sha256:dup", decision_ref: "dec:waiver" }],
    summary: { blocker_failed: 0, blocker_waived: 1, warnings: 0 },
  },
  {
    gate: "F2",
    state: "current",
    passed: false,
    reason: null,
    findings: ["fnd:event"],
    rules: [
      rule("PLUMB.F2.EVENT.CONSUMER", "FAIL", { title: "Produced material events have a consumer or explicit terminal purpose", targets: ["event:approved"], finding_ref: "fnd:event" }),
    ],
    waivers: [],
    summary: { blocker_failed: 1, blocker_waived: 0, warnings: 0 },
  },
  {
    gate: "F3",
    state: "not_evaluated",
    passed: false,
    reason: { code: "E_GATE_INPUTS_MISSING", message: "needs a current Decisions check" },
    findings: [],
    rules: [],
    waivers: [],
    summary: { blocker_failed: 0, blocker_waived: 0, warnings: 0 },
  },
  {
    gate: "F4",
    state: "stale",
    passed: false,
    reason: { code: "E_GATE_INPUTS_STALE", message: "needs a current Scenario run" },
    findings: [],
    rules: [],
    waivers: [],
    summary: { blocker_failed: 0, blocker_waived: 0, warnings: 0 },
  },
];

const FINDINGS: Finding[] = [
  {
    id: "fnd:dup",
    code: "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED",
    family: "F1",
    message: "HR-023 and HR-024 look like duplicates",
    severity: "blocker",
    status: "Open",
    affected_refs: ["req:23", "req:24"],
    rule_class: "plumb_core",
  },
  {
    id: "fnd:criteria",
    code: "PLUMB.F1.REQ.CRITERIA_FOR_BEHAVIOR",
    family: "F1",
    message: "Behaviour without acceptance criteria",
    severity: "error",
    status: "Open",
    affected_refs: REQ_IDS.slice(0, 16),
    rule_class: "plumb_core",
  },
  {
    id: "fnd:modality",
    code: "PLUMB.LINT.WEAK_MODALITY",
    family: "F1",
    message: "Weak modality 'should'",
    severity: "warning",
    status: "Open",
    affected_refs: ["req:12"],
    rule_class: "plumb_core",
  },
  {
    id: "fnd:closed",
    code: "PLUMB.F2.CALC.TYPECHECK",
    family: "F2",
    message: "Calculation does not type-check",
    severity: "blocker",
    status: "Closed",
    affected_refs: ["calculation:days"],
    rule_class: "plumb_core",
  },
];

const DECISIONS: Decision[] = [
  { id: "dec:old", decided_by: "agent:cli", decided_at: "2026-10-20T08:00:00Z", proposal_ref: "prop:1", question_ref: null, rationale: "Reviewed in simulation." },
  { id: "dec:waiver", decided_by: "Marta Nowak", decided_at: "2026-10-25T08:30:00Z", proposal_ref: "waiver-subject:1", question_ref: null, rationale: "they notify different outcomes" },
  { id: "dec:answer", decided_by: "HR expert", decided_at: "2026-10-22T12:15:00Z", proposal_ref: null, question_ref: "q:1", rationale: "The end date is included." },
];

const ASSUMPTIONS: Assumption[] = [
  { id: "asm:half", statement: "Half days count as 0.5.", owner_ref: "actor:hr-lead", status: "Active", expires_at: "2026-12-31T00:00:00Z" },
];

function requirement(id: string, source: string): RequirementSummary {
  return { id, source_identifier: source, text: "Requirement " + source, class: "functional", status: "Accepted", smells: [], coverage: { operations: [], scenarios: [] }, readiness: null, lint_current: true };
}

const REQUIREMENTS: RequirementSummary[] = REQ_IDS.map((id, i) => requirement(id, "HR-" + String(i + 1).padStart(3, "0")));

interface Call {
  readonly method: string;
  readonly path: string;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

interface Backend {
  gates: GateStatus[];
  roles: string[];
  gatesStatus: number;
  calls: Call[];
}

function newBackend(overrides: Partial<Backend> = {}): Backend {
  return { gates: GATES, roles: ["analyst"], gatesStatus: 200, calls: [], ...overrides };
}

const WRITE = { version: "rev:10", revision: "rev:10", semantic_hash: "psg:sha256:10", events: [] };

function handlers(backend: Backend) {
  const record = async (request: Request) => {
    const url = new URL(request.url);
    const body: unknown = request.method === "GET" ? null : await request.clone().json();
    backend.calls.push({ method: request.method, path: decodeURIComponent(url.pathname), ifMatch: request.headers.get("If-Match"), body });
  };
  return [
    http.get("*/api/me", () => HttpResponse.json({ user: "Marta Nowak", mode: "pilot", roles: backend.roles, inference_available: true })),
    http.get("*/api/model/version", () =>
      etagJson({ version: "rev:9", revision: "rev:9", hash: "psg:sha256:9", semantic_hash: "psg:sha256:9", evidence_hash: "ev:9" }, undefined, VERSION_ETAG),
    ),
    http.get("*/api/gates", async ({ request }) => {
      await record(request);
      if (backend.gatesStatus !== 200) {
        return HttpResponse.json({ code: "E_INTERNAL", message: "boom", details: {} }, { status: backend.gatesStatus });
      }
      return etagJson(backend.gates);
    }),
    http.get("*/api/findings", () => etagJson({ items: FINDINGS, next_cursor: null }, undefined, FINDINGS_ETAG)),
    http.get("*/api/decisions", () => etagJson({ items: DECISIONS, next_cursor: null })),
    http.get("*/api/assumptions", () => etagJson({ items: ASSUMPTIONS, health: [{ id: "asm:broken", code: "E_STORE_CORRUPT", message: "broken" }], next_cursor: null }, undefined, ASSUMPTIONS_ETAG)),
    http.get("*/api/requirements", () => etagJson({ items: REQUIREMENTS, health: [], next_cursor: null })),
    http.post("*/api/stages/:stage/run", async ({ request }) => {
      await record(request);
      return HttpResponse.json({ job_id: "job:1", version: "rev:9", revision: "rev:9", semantic_hash: "psg:sha256:9", events: [] });
    }),
    http.get("*/api/jobs/:id", () => HttpResponse.json({ status: "evaluating", progress: 0.5, base_revision: "rev:9", result_version: null, error: null })),
    http.post("*/api/findings/materialize", async ({ request }) => {
      await record(request);
      return HttpResponse.json({ ...WRITE, finding_ids: ["fnd:lineage", "fnd:event"] });
    }),
    http.post("*/api/findings/:id/waive", async ({ request }) => {
      await record(request);
      return HttpResponse.json(WRITE);
    }),
    http.post("*/api/assumptions/:id/resolve", async ({ request }) => {
      await record(request);
      return HttpResponse.json(WRITE);
    }),
  ];
}

function renderAt(path: string, backend: Backend = newBackend()) {
  server.use(...handlers(backend));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { user: userEvent.setup(), router, backend };
}

const gateRow = (name: RegExp) => within(screen.getByRole("main")).getByRole("button", { name });
const writes = (backend: Backend) => backend.calls.filter((c) => c.method !== "GET");

// ------------------------------------------------------------------ gates

describe("Quality · Gates", () => {
  it("shows the five gates with their state, reason and summary, all collapsed on /quality", async () => {
    renderAt("/quality");
    expect(await screen.findByRole("heading", { level: 1, name: "Quality" })).toBeTruthy();
    const views = screen.getByRole("navigation", { name: "Quality views" });
    expect(within(views).getByRole("link", { name: "Gates" }).getAttribute("aria-current")).toBe("page");
    const evidence = await within(await screen.findByRole("main")).findByRole("button", { name: /Evidence/ });
    expect(evidence.textContent).toContain("I0");
    expect(evidence.textContent).toContain("Passes");
    expect(evidence.textContent).toContain("0 blockers failed · 0 waived · 0 warnings");
    expect(evidence.getAttribute("aria-expanded")).toBe("false");
    expect(gateRow(/Requirements/).textContent).toContain("0 blockers failed · 1 waived · 0 warnings");
    expect(gateRow(/Model/).textContent).toContain("Blocked");
    expect(gateRow(/Model/).textContent).toContain("1 blocker failed");
    expect(gateRow(/Decisions/).textContent).toContain("Not checked yet");
    expect(gateRow(/Decisions/).textContent).toContain("needs a current Decisions check");
    const behaviour = gateRow(/Behaviour/);
    expect(behaviour.textContent).toContain("Outdated");
    expect(behaviour.textContent).toContain("needs a current Scenario run");
    // Outdated is a hollow ring.
    expect(behaviour.querySelector("[data-gate-dot='stale']")).not.toBeNull();
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("expands a gate from its URL with rules grouped Fail · Waived · Pass · Not applicable", async () => {
    renderAt("/quality/F1");
    const row = await within(await screen.findByRole("main")).findByRole("button", { name: /Requirements/ });
    expect(row.getAttribute("aria-expanded")).toBe("true");
    const rules = await screen.findByRole("region", { name: "Requirements F1" });
    const groups = within(rules)
      .getAllByRole("heading", { level: 3 })
      .map((h) => h.textContent);
    expect(groups).toEqual(["Fail", "Waived", "Pass", "Not applicable"]);
    // The waived rule: the finding's plain message, the code, who waived it and why.
    const waived = within(rules).getByText("HR-023 and HR-024 look like duplicates").closest("li") as HTMLElement;
    expect(within(waived).getByText("PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED")).toBeTruthy();
    expect(await within(waived).findByText(/waived by Marta Nowak: they notify different outcomes/)).toBeTruthy();
    expect(within(waived).getByText("HR-023")).toBeTruthy();
    expect(within(waived).getByText("HR-024")).toBeTruthy();
    // A non-blocking failure collapses many affected requirements into one chip.
    const lineage = within(rules).getByText("Requirements have intent lineage").closest("li") as HTMLElement;
    expect(within(lineage).getByText(/non-blocking/)).toBeTruthy();
    expect(within(lineage).getByText("32 requirements")).toBeTruthy();
    // Not applicable shows why.
    expect(within(rules).getByText(/No contradiction finding material exists\./)).toBeTruthy();
    expect(within(rules).getByText("Accepted requirements are grounded in evidence or explicit human origin")).toBeTruthy();
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("toggles a gate through its URL", async () => {
    const { user, router } = renderAt("/quality");
    await user.click(await within(await screen.findByRole("main")).findByRole("button", { name: /Model/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/quality/F2"));
    expect(gateRow(/Model/).getAttribute("aria-expanded")).toBe("true");
    await user.click(gateRow(/Model/));
    await waitFor(() => expect(router.state.location.pathname).toBe("/quality"));
  });

  it("re-checks a gate by running the step that owns it, against the version the user sees", async () => {
    const { user, backend } = renderAt("/quality/F1");
    const rules = await screen.findByRole("region", { name: "Requirements F1" });
    await user.click(within(rules).getByRole("button", { name: "Re-check" }));
    await waitFor(() => expect(writes(backend)).toHaveLength(1));
    expect(writes(backend)[0]).toMatchObject({ path: "/api/stages/lint/run", ifMatch: VERSION_ETAG });
    cleanup();
    const second = renderAt("/quality/F2");
    const model = await screen.findByRole("region", { name: "Model F2" });
    await second.user.click(within(model).getByRole("button", { name: "Re-check" }));
    await waitFor(() => expect(writes(second.backend)).toHaveLength(1));
    expect(writes(second.backend)[0]?.path).toBe("/api/stages/analyse/run");
  });

  it("re-checks Evidence and Decisions by evaluating the gates again (no step owns them)", async () => {
    const { user, backend } = renderAt("/quality/I0");
    const rules = await screen.findByRole("region", { name: "Evidence I0" });
    const reads = () => backend.calls.filter((c) => c.path === "/api/gates").length;
    const before = reads();
    await user.click(within(rules).getByRole("button", { name: "Re-check" }));
    await waitFor(() => expect(reads()).toBeGreaterThan(before));
    expect(writes(backend)).toEqual([]);
    expect(await screen.findByText("Evidence gate checked again")).toBeTruthy();
  });

  it("shows the affected requirements or model elements", async () => {
    const { user, router } = renderAt("/quality/F1");
    const rules = await screen.findByRole("region", { name: "Requirements F1" });
    await user.click(within(rules).getByRole("button", { name: "Show affected" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/requirements"));
    expect(String((router.state.location.search as Record<string, unknown>)["affected"]).split(",")).toHaveLength(32);
    cleanup();
    const second = renderAt("/quality/F2");
    const model = await screen.findByRole("region", { name: "Model F2" });
    await second.user.click(within(model).getByRole("button", { name: "Show affected" }));
    await waitFor(() => expect(second.router.state.location.pathname).toBe("/model/entities"));
  });

  it("hides Re-check from a role that cannot run steps", async () => {
    renderAt("/quality/F1", newBackend({ roles: ["reviewer"] }));
    const rules = await screen.findByRole("region", { name: "Requirements F1" });
    expect(within(rules).queryByRole("button", { name: "Re-check" })).toBeNull();
    expect(within(rules).getByRole("button", { name: "Show affected" })).toBeTruthy();
  });

  it("shows a read failure with Retry", async () => {
    const backend = newBackend({ gatesStatus: 500 });
    const { user } = renderAt("/quality", backend);
    expect((await screen.findByRole("alert")).textContent).toContain("The gates could not be loaded.");
    backend.gatesStatus = 200;
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await within(await screen.findByRole("main")).findByRole("button", { name: /Evidence/ })).toBeTruthy();
  });

  it("opens the export sheet from the Export pill", async () => {
    const { user } = renderAt("/quality");
    await user.click(await screen.findByRole("button", { name: "Export" }));
    expect(await screen.findByRole("dialog", { name: "Export specification" })).toBeTruthy();
  });
});

// ------------------------------------------------------------------ issues

describe("Quality · Issues", () => {
  it("lists every finding with severity, message, code and affected items; waived ones say who and why", async () => {
    renderAt("/quality/issues");
    const list = await screen.findByRole("list", { name: "Issues" });
    const rows = within(list).getAllByRole("listitem");
    expect(rows).toHaveLength(4);
    const criteria = within(list).getByText("Behaviour without acceptance criteria").closest("li") as HTMLElement;
    expect(within(criteria).getByText("Error")).toBeTruthy();
    expect(within(criteria).getByText("PLUMB.F1.REQ.CRITERIA_FOR_BEHAVIOR")).toBeTruthy();
    expect(within(criteria).getByText(/16 requirements/)).toBeTruthy();
    const modality = within(list).getByText("Weak modality 'should'").closest("li") as HTMLElement;
    expect(await within(modality).findByText(/HR-012/)).toBeTruthy();
    expect(within(modality).getByText("Warning")).toBeTruthy();
    const dup = within(list).getByText("HR-023 and HR-024 look like duplicates").closest("li") as HTMLElement;
    expect(within(dup).getByText("Waived")).toBeTruthy();
    expect(await within(dup).findByText("Waived by Marta Nowak: “they notify different outcomes”")).toBeTruthy();
    expect(within(dup).queryByRole("button", { name: "Waive with reason" })).toBeNull();
    const closed = within(list).getByText("Calculation does not type-check").closest("li") as HTMLElement;
    expect(within(closed).getByText("Resolved")).toBeTruthy();
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("waives a finding with a reason against the reviewed findings version", async () => {
    const { user, backend } = renderAt("/quality/issues");
    const list = await screen.findByRole("list", { name: "Issues" });
    const criteria = within(list).getByText("Behaviour without acceptance criteria").closest("li") as HTMLElement;
    await user.click(within(criteria).getByRole("button", { name: "Waive with reason" }));
    const sheet = await screen.findByRole("dialog", { name: "Waive this finding" });
    await user.type(within(sheet).getByRole("textbox"), "Deferred to the next release.");
    await user.click(within(sheet).getByRole("button", { name: /Waive/ }));
    await waitFor(() => expect(writes(backend)).toHaveLength(1));
    expect(writes(backend)[0]).toMatchObject({ path: "/api/findings/fnd:criteria/waive", ifMatch: FINDINGS_ETAG, body: { reason: "Deferred to the next release." } });
    expect(await screen.findByText("Finding waived · recorded in History")).toBeTruthy();
  });

  it("offers no waiver to a role that cannot waive", async () => {
    renderAt("/quality/issues", newBackend({ roles: ["reviewer"] }));
    await screen.findByRole("list", { name: "Issues" });
    expect(screen.queryByRole("button", { name: "Waive with reason" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Record findings" })).toBeNull();
  });

  it("waives only findings a current gate check produced, and records the gates' findings so they can be waived", async () => {
    const { user, backend } = renderAt("/quality/issues");
    const list = await screen.findByRole("list", { name: "Issues" });
    // Not produced by a current gate report: the server would refuse a waiver.
    const modality = within(list).getByText("Weak modality 'should'").closest("li") as HTMLElement;
    expect(within(modality).queryByRole("button", { name: "Waive with reason" })).toBeNull();
    // The F1 lineage and F2 event findings are in the gates but not recorded yet.
    const notice = await screen.findByText("2 findings from the current gate checks are not recorded yet. Record them to waive them.");
    await user.click(within(notice.parentElement as HTMLElement).getByRole("button", { name: "Record findings" }));
    await waitFor(() => expect(writes(backend)).toHaveLength(1));
    expect(writes(backend)[0]).toMatchObject({ path: "/api/findings/materialize", ifMatch: FINDINGS_ETAG, body: {} });
    expect(await screen.findByText("Findings recorded")).toBeTruthy();
  });
});

// ------------------------------------------------------------------ history

describe("Quality · History", () => {
  it("lists the decisions newest first with time in Europe/Warsaw, who, what and why", async () => {
    const { user } = renderAt("/quality/history");
    const table = await screen.findByRole("table", { name: "Decision history" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(3);
    expect((rows[0] as HTMLElement).textContent).toMatch(/25 Oct.*09:30/);
    expect(within((rows[0] as HTMLElement)).getByText("Marta Nowak")).toBeTruthy();
    expect(within((rows[0] as HTMLElement)).getByText("Waived a finding")).toBeTruthy();
    expect(within((rows[0] as HTMLElement)).getByText("“they notify different outcomes”")).toBeTruthy();
    expect(within((rows[1] as HTMLElement)).getByText("Answered a question")).toBeTruthy();
    expect(within((rows[2] as HTMLElement)).getByText("Decided on a proposal")).toBeTruthy();
    expect(screen.getByText("Times in Europe/Warsaw")).toBeTruthy();
    await user.type(screen.getByRole("searchbox", { name: "Search decisions" }), "end date");
    expect(within(table).getAllByRole("row").slice(1)).toHaveLength(1);
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("lists the assumptions with Resolve and the rows that failed to load", async () => {
    const { user, backend } = renderAt("/quality/history");
    const section = await screen.findByRole("region", { name: "Assumptions" });
    expect(await within(section).findByText("Half days count as 0.5.")).toBeTruthy();
    expect(within(section).getByText("Couldn't load asm:broken.")).toBeTruthy();
    await user.click(within(section).getByRole("button", { name: "Resolve" }));
    const sheet = await screen.findByRole("dialog", { name: "Resolve this assumption" });
    await user.type(within(sheet).getByRole("textbox"), "HR confirmed half days.");
    await user.click(within(sheet).getByRole("button", { name: /Resolve/ }));
    await waitFor(() => expect(writes(backend)).toHaveLength(1));
    expect(writes(backend)[0]).toMatchObject({ path: "/api/assumptions/asm:half/resolve", ifMatch: ASSUMPTIONS_ETAG, body: { reason: "HR confirmed half days." } });
    expect(await screen.findByText("Assumption resolved")).toBeTruthy();
  });
});
