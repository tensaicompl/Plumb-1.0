// U2.6 analyst views tests (Hotfix 080/081): /model/findings, /model/entities, /model/rules and
// /model/calculations through the real router. Findings show the server rule_class (never derived
// from code or family) next to the family, plus the read-only Assumptions; entities show the ER
// relationships and the server CRUD matrix with null distinct from false; rules separate null from
// empty analysis and fill one explicit cell through one versioned task-endpoint POST; calculations
// show the read-only worked example or its unavailable state. Every request is recorded; MSW
// answers all of them and nothing touches the network.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { etagJson, server, TEST_ETAG } from "../../test/msw";

type Finding = components["schemas"]["Finding"];
type Assumption = components["schemas"]["Assumption"];
type Entity = components["schemas"]["Entity"];
type CrudMatrix = components["schemas"]["CrudMatrix"];
type Rule = components["schemas"]["Rule"];
type Calculation = components["schemas"]["Calculation"];
type CalculationExample = components["schemas"]["CalculationExample"];

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

interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

/** Every request the app makes, handled or not, in order. */
let calls: Call[] = [];

beforeAll(async () => {
  server.listen({ onUnhandledRequest: "error" });
  server.events.on("request:start", ({ request }) => {
    const url = new URL(request.url);
    const record = (body: unknown) =>
      calls.push({ method: request.method, path: decodeURIComponent(url.pathname), search: url.searchParams, ifMatch: request.headers.get("if-match"), body });
    if (request.method === "GET") {
      record(null);
    } else {
      void request
        .clone()
        .text()
        .then((text) => record(text === "" ? null : (JSON.parse(text) as unknown)));
    }
  });
  vi.stubGlobal("Request", OriginRequest);
  vi.stubGlobal("WebSocket", FakeWebSocket);
  vi.stubGlobal("scrollTo", () => undefined);
  routerModule = await import("../../app/router");
  mutation = await import("../../api/useVersionedMutation");
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
  mutation.useConflictStore.getState().clearConflict();
  vi.restoreAllMocks();
});

// ------------------------------------------------------------------ fixtures

const CURRENT = "psg:sha256:current";

const finding = (id: string, code: string, family: string, ruleClass: Finding["rule_class"], extra: Partial<Finding> = {}): Finding => ({
  id,
  code,
  family,
  rule_class: ruleClass,
  severity: "blocker",
  status: "Open",
  message: "Message of " + id + ".",
  affected_refs: ["req:hr-001"],
  waiver_ref: null,
  ...extra,
});

// Codes and families deliberately do not imply the server rule class: only rule_class counts.
const FINDINGS: Finding[] = [
  finding("fnd:std", "PLUMB.TEST.CUSTOM", "I0", "external_standard"),
  finding("fnd:interop", "ISO29148.F1.LOOKS.STANDARD", "F1", "external_interop"),
  finding("fnd:core", "PPMN.I0.LOOKS.INTEROP", "F2", "plumb_core", { waiver_ref: "decision:waiver-1", status: "Waived" }),
  finding("fnd:profile", "BPMN.LOOKS.EXTERNAL", "F3", "profile_policy"),
  finding("fnd:org", "DMN.LOOKS.EXTERNAL", "F4", "organization_policy"),
  finding("fnd:custom", "PLUMB.I0.SOURCE.CONTENT_ADDRESSED", "I0", null, { affected_refs: [] }),
];

const ASSUMPTIONS: Assumption[] = [
  {
    id: "assumption:half-days",
    statement: "Half days count as 0.5.",
    owner_ref: "actor:hr-lead",
    status: "Open",
    finding_ref: "fnd:core",
    default_value: 5,
    expires_at: null,
    risk_ref: null,
  },
  {
    id: "assumption:mode",
    statement: "Approval is manual.",
    owner_ref: "actor:employee",
    status: "Resolved",
    finding_ref: null,
    default_value: "manual",
    expires_at: "2026-12-31T00:00:00Z",
    risk_ref: "risk:approval",
  },
];

const ENTITIES: Entity[] = [
  {
    id: "entity:request",
    name: "Leave request",
    attributes: [
      { id: "attr:days", name: "days", type: "decimal", nullable: false, class: "quantity", unit: "day" },
      { id: "attr:note", name: "note", type: "string", nullable: true, class: null, unit: null },
    ],
    relationships: [{ id: "rel:requested-by", target: "entity:employee", kind: "references", cardinality: "many_to_one" }],
    states: ["state:submitted", "state:approved"],
    transitions: [{ from: "state:submitted", to: "state:approved", operation: "op:approve" }],
    tags: ["hr"],
  },
  { id: "entity:employee", name: "Employee", attributes: [], relationships: [], states: [], transitions: [], tags: [] },
];

const CRUD: CrudMatrix = {
  operations: ["op:approve", "op:submit"],
  entities: ["entity:employee", "entity:request"],
  cells: [
    { op: "op:approve", ent: "entity:request", c: null, r: true, u: true, d: false },
    { op: "op:submit", ent: "entity:request", c: true, r: false, u: null, d: null },
  ],
};

const SCALAR: Rule = { id: "rule:long-leave", name: "Long leave", kind: "rule", conditions: ["days > 10"], rows: null, uncovered_cells: null, overlaps: null };
const DT_LEAVE: Rule = {
  id: "dt:leave",
  name: "Leave approval",
  kind: "decision_table",
  conditions: ["days", "role"],
  rows: [
    { when: ["<= 5", "employee"], then: ['"approve"'] },
    { when: ["> 5", "manager"], then: ['"review"'] },
  ],
  uncovered_cells: ['{"days":"> 5","role":"employee"}'],
  overlaps: [],
};
const DT_BONUS: Rule = {
  id: "dt:bonus",
  name: "Bonus",
  kind: "decision_table",
  conditions: ["score"],
  rows: [
    { when: [">= 0"], then: ["1"] },
    { when: [">= 5"], then: ["2"] },
  ],
  uncovered_cells: [],
  overlaps: ["rows 0 and 1"],
};
const DT_OPAQUE: Rule = { id: "dt:opaque", name: "Opaque", kind: "decision_table", conditions: [], rows: [], uncovered_cells: null, overlaps: null };
const RULES: Rule[] = [SCALAR, DT_LEAVE, DT_BONUS, DT_OPAQUE];

const CALCULATIONS: Calculation[] = [
  { id: "calc:days", name: "Leave days", expr: "end - start + 1" },
  { id: "calc:balance", name: "Balance", expr: "entitlement - taken" },
];
const EXAMPLE: CalculationExample = { expr: "end - start + 1", rendered_example: "5 - 2 + 1 = 4.10", value: { type: "decimal", value: "4.10" } };

interface Backend {
  cellFailure: Response | null;
  rulesPage?: unknown;
  roles?: string[];
}

function handlers(backend: Backend) {
  return [
    http.get("*/api/me", () => etagJson({ user: "alice", mode: "pilot", roles: backend.roles ?? ["analyst"] })),
    http.get("*/api/model/version", () =>
      etagJson({ version: "rev:1", revision: "rev:1", hash: CURRENT, semantic_hash: CURRENT, evidence_hash: "ev:1" }),
    ),
    http.get("*/api/gates", () => etagJson({ items: [], next_cursor: null })),
    http.get("*/api/findings", () => etagJson({ items: FINDINGS, next_cursor: null })),
    http.get("*/api/assumptions", () => etagJson({ items: ASSUMPTIONS, next_cursor: null })),
    http.get("*/api/model/entities", () => etagJson({ items: ENTITIES, next_cursor: null })),
    http.get("*/api/model/crud", () => etagJson(CRUD)),
    http.get("*/api/model/rules", () => etagJson((backend.rulesPage ?? { items: RULES, next_cursor: null }) as Record<string, unknown>)),
    http.get("*/api/model/calculations", () => etagJson({ items: CALCULATIONS, next_cursor: null })),
    http.get("*/api/model/calculations/:id/example", ({ params }) =>
      params["id"] === "calc:days"
        ? etagJson(EXAMPLE)
        : etagJson({ code: "E_INVALID_REQUEST", message: 'input "taken": a root input value is required', details: {} }, { status: 400 }),
    ),
    http.post("*/api/model/rules/:id/cells", () =>
      backend.cellFailure !== null ? backend.cellFailure : etagJson({ version: "rev:2", revision: "rev:2", semantic_hash: CURRENT, events: [] }),
    ),
  ];
}

async function renderAt(path: string, backend: Backend = { cellFailure: null }) {
  server.use(...handlers(backend));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { user: userEvent.setup(), router };
}

const writes = () => calls.filter((c) => c.method !== "GET");
const gets = (path: string) => calls.filter((c) => c.method === "GET" && c.path === path);
const item = (attribute: string, id: string) => document.querySelector("[" + attribute + "='" + id + "']") as HTMLElement;

// ------------------------------------------------------------------ entities


/** Fable #225: every data table has a caption and every header cell a scope. */
function expectAccessibleTables() {
  const tables = [...document.querySelectorAll("main table")];
  expect(tables.length).toBeGreaterThan(0);
  for (const table of tables) {
    expect(table.querySelector("caption")?.textContent ?? "").not.toBe("");
    for (const th of table.querySelectorAll("th")) {
      expect(["col", "row"]).toContain(th.getAttribute("scope"));
    }
  }
}

describe("Role-gated write controls (Fable #218)", () => {
  it("offers a non-analyst no analyst actions and explains why", async () => {
    await renderAt("/model/rules", { cellFailure: null, roles: ["reviewer"] });
    await screen.findByText("Leave approval");
    expect(screen.queryByTestId("cell-fill")).toBeNull();
    expect(screen.getAllByTestId("role-required").length).toBeGreaterThan(0);
    expect(screen.getAllByTestId("role-required")[0]?.textContent).toBe("These changes need the role: analyst");
    cleanup();
    await renderAt("/model/entities", { cellFailure: null, roles: ["reviewer"] });
    await screen.findByText("Leave request");
    expect(screen.queryByTestId("attribute-type-save")).toBeNull();
    // The value itself stays visible.
    expect(screen.getAllByText("decimal").length).toBeGreaterThan(0);
  });

  it("offers an analyst the same controls", async () => {
    await renderAt("/model/rules");
    await screen.findByText("Leave approval");
    expect(screen.getAllByTestId("cell-fill").length).toBeGreaterThan(0);
    expect(screen.queryByTestId("role-required")).toBeNull();
  });
});

describe("EntitiesPage", () => {
  it("gives the attribute, relationship and CRUD tables captions and scoped headers", async () => {
    await renderAt("/model/entities");
    await screen.findByText("Leave request");
    await screen.findByTestId("crud-matrix");
    expectAccessibleTables();
  });

  it("renders attributes and relationships exactly from the entity list", async () => {
    const { router } = await renderAt("/model/entities");
    await screen.findByText("Leave request");
    expect(router.state.location.pathname).toBe("/model/entities");
    const request = item("data-entity-id", "entity:request");
    const days = request.querySelector("[data-attribute='days']") as HTMLElement;
    // The type cell shows the current type plus the type editor.
    const cells = (row: HTMLElement) =>
      [...row.querySelectorAll(":scope > td")].map((td, i) => (i === 1 ? td.querySelector(".font-mono")?.textContent : td.textContent));
    expect(cells(days)).toEqual(["days", "decimal", "No", "quantity", "day"]);
    const note = request.querySelector("[data-attribute='note']") as HTMLElement;
    expect(cells(note)).toEqual(["note", "string", "Yes", "None", "None"]);
    const rel = request.querySelector("[data-relationship='rel:requested-by']") as HTMLElement;
    expect([...rel.querySelectorAll("td")].map((td) => td.textContent)).toEqual(["rel:requested-by", "references", "entity:employee", "many_to_one"]);
    expect(within(request).getByTestId("entity-tags").textContent).toBe("Tags: hr");
    expect(within(item("data-entity-id", "entity:employee")).getByText("No relationships.")).toBeTruthy();
    // No per-entity detail request (no N+1), no state-machine view.
    expect(calls.filter((c) => c.path.startsWith("/api/model/entities/"))).toEqual([]);
    expect(document.body.textContent).not.toContain("state:submitted");
  });

  it("renders the server CRUD matrix with null distinct from false and sends no write", async () => {
    await renderAt("/model/entities");
    await screen.findByTestId("crud-matrix");
    expect(gets("/api/model/crud")).toHaveLength(1);
    const flag = (op: string, ent: string, letter: string) =>
      document.querySelector("[data-crud-cell='" + op + "|" + ent + "'] [data-crud='" + letter + "']") as HTMLElement;
    expect(flag("op:approve", "entity:request", "c").getAttribute("data-value")).toBe("null");
    expect(flag("op:approve", "entity:request", "c").textContent).toBe("c Not distinguishable");
    expect(flag("op:approve", "entity:request", "r").textContent).toBe("r Yes");
    expect(flag("op:approve", "entity:request", "u").textContent).toBe("u Yes");
    expect(flag("op:approve", "entity:request", "d").textContent).toBe("d No");
    expect(flag("op:submit", "entity:request", "c").textContent).toBe("c Yes");
    expect(flag("op:submit", "entity:request", "r").textContent).toBe("r No");
    expect(flag("op:submit", "entity:request", "u").textContent).toBe("u Not distinguishable");
    expect(flag("op:submit", "entity:request", "d").textContent).toBe("d Not distinguishable");
    // null and false never look alike.
    expect(flag("op:submit", "entity:request", "d").textContent).not.toBe(flag("op:approve", "entity:request", "d").textContent);
    expect(flag("op:approve", "entity:employee", "c")).toBeNull();
    // The only controls are the attribute type editors; viewing writes nothing.
    expect(document.querySelectorAll("main input, main textarea, main form")).toHaveLength(0);
    expect(document.querySelectorAll("main select:not([data-testid='attribute-type-choice'])")).toHaveLength(0);
    expect(writes()).toEqual([]);
  });
});

// ------------------------------------------------------------------ rules

describe("RulesPage", () => {
  const state = (ruleId: string, analysis: string) => item("data-rule-id", ruleId).querySelector("[data-analysis='" + analysis + "']") as HTMLElement;

  it("lists a rule the server could not project as a typed health marker next to the others (H0.11)", async () => {
    await renderAt("/model/rules", {
      cellFailure: null,
      rulesPage: {
        items: [SCALAR],
        health: [{ id: "dt:broken", code: "E_DECISION_TABLE_ENCODING_INVALID", message: "the persisted decision table encoding is malformed" }],
        next_cursor: null,
      },
    });
    await screen.findByText("Long leave");
    const markers = await screen.findByTestId("health-markers");
    const broken = markers.querySelector("[data-health-id='dt:broken']") as HTMLElement;
    expect(within(broken).getByTestId("health-code").textContent).toBe("E_DECISION_TABLE_ENCODING_INVALID");
    expect(broken.textContent).toContain("the persisted decision table encoding is malformed");
  });

  it("separates scalar rules, decision tables and null versus empty analysis", async () => {
    const { router } = await renderAt("/model/rules");
    await screen.findByText("Leave approval");
    expect(router.state.location.pathname).toBe("/model/rules");
    const scalar = item("data-rule-id", "rule:long-leave");
    expect(scalar.textContent).toContain("days > 10");
    expect(within(scalar).getByTestId("rows-not-applicable")).toBeTruthy();
    expect(state("rule:long-leave", "uncovered").getAttribute("data-state")).toBe("not-analyzable");
    expect(state("rule:long-leave", "overlaps").getAttribute("data-state")).toBe("not-analyzable");
    expect(within(scalar).queryByTestId("cell-fill")).toBeNull();
    expect(scalar.textContent).not.toMatch(/no uncovered|no overlaps/i);

    const leave = item("data-rule-id", "dt:leave");
    const row1 = leave.querySelector("[data-row='1']") as HTMLElement;
    expect(row1.querySelector("[data-cells='when']")?.textContent).toBe("> 5 | manager");
    expect(row1.querySelector("[data-cells='then']")?.textContent).toBe('"review"');
    expect(state("dt:leave", "uncovered").getAttribute("data-state")).toBe("found");
    expect(state("dt:leave", "uncovered").textContent).toContain('{"days":"> 5","role":"employee"}');
    expect(state("dt:leave", "overlaps").getAttribute("data-state")).toBe("none");
    expect(state("dt:leave", "overlaps").textContent).toBe("OverlapsAnalysed: no overlaps.");

    expect(state("dt:bonus", "uncovered").getAttribute("data-state")).toBe("none");
    expect(state("dt:bonus", "uncovered").textContent).toBe("Coverage gapsAnalysed: no uncovered gaps.");
    expect(state("dt:bonus", "overlaps").getAttribute("data-state")).toBe("found");
    expect(state("dt:bonus", "overlaps").textContent).toContain("rows 0 and 1");

    // A decision table whose analysis is null is not the same as an empty analysis.
    expect(state("dt:opaque", "uncovered").getAttribute("data-state")).toBe("not-analyzable");
    expect(state("dt:opaque", "uncovered").textContent).toBe("Coverage gapsNot applicable / not analyzable");
    expect(state("dt:opaque", "overlaps").textContent).toBe("OverlapsNot applicable / not analyzable");
    expect(state("dt:opaque", "uncovered").textContent).not.toBe(state("dt:bonus", "uncovered").textContent);
    expect(writes()).toEqual([]);
  });

  it("fills one explicit cell with one versioned task-endpoint POST and refetches the server rules", async () => {
    const { user } = await renderAt("/model/rules");
    await screen.findByText("Leave approval");
    const leave = item("data-rule-id", "dt:leave");
    const submit = within(leave).getByTestId("fill-submit") as HTMLButtonElement;
    // Nothing is prefilled from the coverage witness; submission needs every explicit input.
    expect((within(leave).getByTestId("fill-cell") as HTMLInputElement).value).toBe("");
    expect((within(leave).getByTestId("fill-outcome") as HTMLTextAreaElement).value).toBe("");
    expect(submit.disabled).toBe(true);
    const outcome = '{"output":"reject" }';
    await user.type(within(leave).getByTestId("fill-cell"), "row:1:output:0");
    await user.click(within(leave).getByTestId("fill-outcome"));
    await user.paste(outcome);
    await user.type(within(leave).getByTestId("fill-reason"), "   ");
    expect(submit.disabled).toBe(true);
    await user.type(within(leave).getByTestId("fill-reason"), "Managers review long leave.");
    const rulesBefore = gets("/api/model/rules").length;
    await user.click(submit);
    await waitFor(() => expect(gets("/api/model/rules").length).toBeGreaterThan(rulesBefore));
    await waitFor(() => expect(writes()).toHaveLength(1));
    const [post] = writes();
    expect(post?.method).toBe("POST");
    expect(post?.path).toBe("/api/model/rules/dt:leave/cells");
    expect(post?.ifMatch).toBe(TEST_ETAG);
    // The exact entered text: no reparsing or recanonicalisation in the browser.
    expect(post?.body).toEqual({ cell: "row:1:output:0", outcome, reason: "   Managers review long leave." });
    expect(calls.filter((c) => c.path.startsWith("/api/proposals/"))).toEqual([]);
    // The rows stay the server's: nothing was patched locally from the form.
    const row1 = item("data-rule-id", "dt:leave").querySelector("[data-row='1']") as HTMLElement;
    expect(row1.querySelector("[data-cells='then']")?.textContent).toBe('"review"');
  });

  it("shows a server write error once, without retry or generic proposal call", async () => {
    const { user } = await renderAt("/model/rules", {
      cellFailure: etagJson({ code: "E_NO_CHANGE", message: "The cell already has this outcome.", details: {} }, { status: 422 }),
    });
    await screen.findByText("Bonus");
    const bonus = item("data-rule-id", "dt:bonus");
    await user.type(within(bonus).getByTestId("fill-cell"), "row:0:output:0");
    await user.click(within(bonus).getByTestId("fill-outcome"));
    await user.paste("1");
    await user.type(within(bonus).getByTestId("fill-reason"), "Same value.");
    await user.click(within(bonus).getByTestId("fill-submit"));
    expect((await within(bonus).findByTestId("rule-write-error")).textContent).toContain("E_NO_CHANGE");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(writes()).toHaveLength(1);
    expect(writes()[0]?.path).toBe("/api/model/rules/dt:bonus/cells");
    expect(calls.filter((c) => c.path.startsWith("/api/proposals/"))).toEqual([]);
  });
});

// ------------------------------------------------------------------ calculations

describe("CalculationsPage", () => {
  it("lists read-only expressions and renders the exact server worked example", async () => {
    const { user, router } = await renderAt("/model/calculations");
    await screen.findByText("Leave days");
    expect(router.state.location.pathname).toBe("/model/calculations");
    const days = item("data-calculation-id", "calc:days");
    expect(within(days).getByTestId("calculation-expr").textContent).toBe("end - start + 1");
    await user.click(within(days).getByTestId("show-example"));
    expect((await within(days).findByTestId("rendered-example")).textContent).toBe("5 - 2 + 1 = 4.10");
    // The exact decimal string, never a binary float ("4.1").
    expect(within(days).getByTestId("example-value").textContent).toBe("4.10");
    const example = gets("/api/model/calculations/calc:days/example");
    expect(example).toHaveLength(1);
    expect(example[0]?.search.has("inputs")).toBe(false);
  });

  it("shows the unavailable state with the server error and offers an inputs field but no write", async () => {
    const { user } = await renderAt("/model/calculations");
    await screen.findByText("Balance");
    const balance = item("data-calculation-id", "calc:balance");
    await user.click(within(balance).getByTestId("show-example"));
    const unavailable = await within(balance).findByTestId("example-unavailable");
    expect(unavailable.textContent).toContain("Worked example unavailable");
    expect(unavailable.textContent).toContain("E_INVALID_REQUEST");
    expect(unavailable.textContent).toContain('input "taken": a root input value is required');
    // The only control is the read-only example inputs field; nothing is ever written.
    expect(document.querySelectorAll("main input, main select, main form, main [contenteditable]")).toHaveLength(0);
    expect(within(balance).getByTestId("example-inputs")).toBeTruthy();
    expect(writes()).toEqual([]);
  });
});
