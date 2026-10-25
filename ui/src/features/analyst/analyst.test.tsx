// U2.6 analyst views tests (Hotfix 080/081): findings through the real router (/quality/issues).
// Findings show the server rule_class (never derived from code or family) next to the family, plus
// the read-only Assumptions. (The Entities, Rules and Calculations cases moved to the S4 Model
// screen tests in src/features/model.) Every request is recorded; MSW
// answers all of them and nothing touches the network.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { etagJson, server } from "../../test/msw";

type Finding = components["schemas"]["Finding"];
type Assumption = components["schemas"]["Assumption"];

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
const fieldOf = (element: HTMLElement, name: string) => element.querySelector("[data-field='" + name + "'] dd") as HTMLElement;

// ------------------------------------------------------------------ findings and assumptions

describe("FindingsPage", () => {
  it("renders every server finding field with the server rule class next to the family", async () => {
    const { router } = await renderAt("/quality/issues");
    await screen.findByText("Message of fnd:std.");
    expect(router.state.location.pathname).toBe("/quality/issues");
    const std = item("data-finding-id", "fnd:std");
    for (const [name, text] of [
      ["id", "fnd:std"],
      ["code", "PLUMB.TEST.CUSTOM"],
      ["family", "I0"],
      ["severity", "blocker"],
      ["status", "Open"],
      ["message", "Message of fnd:std."],
      ["affected_refs", "req:hr-001"],
    ] as const) {
      expect(fieldOf(std, name).textContent).toBe(text);
    }
    expect(std.querySelector("[data-field='waiver_ref']")).toBeNull();
    expect(fieldOf(item("data-finding-id", "fnd:core"), "waiver_ref").textContent).toBe("decision:waiver-1");
    // Family and rule class are separate fields.
    expect(fieldOf(std, "rule_class").textContent).toBe("External standard");
    // Plain list reads: no limit, cursor or filter parameter.
    expect(gets("/api/findings")).toHaveLength(1);
    expect([...(gets("/api/findings")[0]?.search.keys() ?? [])]).toEqual([]);
  });

  it("labels only the server rule_class, keeping the raw policy wire value", async () => {
    await renderAt("/quality/issues");
    await screen.findByText("Message of fnd:std.");
    const badge = (id: string) => fieldOf(item("data-finding-id", id), "rule_class").querySelector("[data-rule-class]") as HTMLElement;
    const expected: [string, string, string][] = [
      ["fnd:std", "external_standard", "External standard"],
      ["fnd:interop", "external_interop", "External interoperability"],
      ["fnd:core", "plumb_core", "Plumb core"],
      ["fnd:profile", "profile_policy", "Profile / organizationprofile_policy"],
      ["fnd:org", "organization_policy", "Profile / organizationorganization_policy"],
      ["fnd:custom", "", "Unclassified / custom"],
    ];
    for (const [id, raw, text] of expected) {
      expect(badge(id).getAttribute("data-rule-class")).toBe(raw);
      expect(badge(id).textContent).toBe(text);
    }
    expect(within(badge("fnd:profile")).getByTestId("raw-rule-class").textContent).toBe("profile_policy");
    expect(within(badge("fnd:org")).getByTestId("raw-rule-class").textContent).toBe("organization_policy");
    // No conformance claim anywhere on the page.
    expect(document.body.textContent).not.toMatch(/complian|certif/i);
  });

  it("renders the server assumptions read-only with exact typed defaults", async () => {
    await renderAt("/quality/issues");
    await screen.findByText("Half days count as 0.5.");
    expect(gets("/api/assumptions")).toHaveLength(1);
    const half = item("data-assumption-id", "assumption:half-days");
    expect(fieldOf(half, "statement").textContent).toBe("Half days count as 0.5.");
    expect(fieldOf(half, "owner_ref").textContent).toBe("actor:hr-lead");
    expect(fieldOf(half, "status").textContent).toBe("Open");
    expect(fieldOf(half, "finding_ref").textContent).toBe("fnd:core");
    expect(fieldOf(half, "default_value").textContent).toBe("5");
    expect(fieldOf(half, "default_value").querySelector("[data-value-type]")?.getAttribute("data-value-type")).toBe("number");
    expect(fieldOf(half, "expires_at").textContent).toBe("None");
    const mode = item("data-assumption-id", "assumption:mode");
    expect(fieldOf(mode, "default_value").textContent).toBe('"manual"');
    expect(fieldOf(mode, "finding_ref").textContent).toBe("None");
    expect(fieldOf(mode, "expires_at").textContent).toBe("2026-12-31T00:00:00Z");
    expect(fieldOf(mode, "risk_ref").textContent).toBe("risk:approval");
    // Read-only: no controls and no waiver or resolution request.
    expect(document.querySelectorAll("main input, main textarea, main select, main form")).toHaveLength(0);
    expect(writes()).toEqual([]);
  });
});
