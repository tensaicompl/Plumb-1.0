// Requirements screen (handoff "3. Requirements", spec §5.3): the list with search, filter chips
// and sort; item health rows; the inspector (statement, wording suggestion, readiness, quality
// issues, evidence, linked chips) and its governed writes (accept wording, waive, change kind,
// split with a server preview, mark as duplicate), each against the reviewed ETag of the detail
// read. Every HTTP call is answered by MSW and recorded; nothing touches the network.
//
// The generated client captures `fetch` and `Request` when its module is first evaluated, so the
// app modules are imported dynamically after MSW is listening.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { axeViolations } from "../../test/axe";
import { etagJson, server } from "../../test/msw";

type RequirementSummary = components["schemas"]["RequirementSummary"];
type RequirementDetail = components["schemas"]["RequirementDetail"];
type Finding = components["schemas"]["Finding"];
type GateStatus = components["schemas"]["GateStatus"];
type Preview = components["schemas"]["SemanticChangePreviewResponse"];

let routerModule: typeof import("../../app/router");
let toastModule: typeof import("../../components/ui/Toast");

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
});

afterAll(() => {
  server.close();
  vi.unstubAllGlobals();
});

afterEach(() => {
  cleanup();
  server.resetHandlers();
  act(() => toastModule.dismissToast());
});

// ------------------------------------------------------------------ fixtures

const LIST_ETAG = '"rev:7:list"';
const DETAIL_ETAG = '"rev:7:detail"';
const FINDINGS_ETAG = '"rev:7:findings"';

function summary(id: string, source: string | null, text: string, extra: Partial<RequirementSummary> = {}): RequirementSummary {
  return {
    id,
    source_identifier: source,
    text,
    class: "functional",
    status: "Accepted",
    smells: [],
    coverage: { operations: ["operation:submit"], scenarios: ["scenario:draft"] },
    readiness: 92,
    lint_current: true,
    ...extra,
  };
}

// Server order is by ID; document order is by source identifier.
const ITEMS: RequirementSummary[] = [
  summary("req:c", "HR-012", "A manager should decide on a submitted leave request within 2 days.", {
    class: "operational",
    readiness: 61,
    coverage: { operations: [], scenarios: [] },
  }),
  summary("req:a", "HR-009", "Submitting a valid leave request shall change its status from Draft to Submitted."),
  summary("req:b", "HR-030", "The leave-request submission operation should complete within 2 seconds.", {
    class: "quality",
    readiness: 78,
    coverage: { operations: ["operation:submit"], scenarios: [] },
  }),
];

const FINDINGS: Finding[] = [
  {
    id: "fnd:modality",
    code: "PLUMB.F1.REQ.MODALITY_EXPLICIT",
    family: "F1",
    message: "Weak modality 'should'",
    severity: "error",
    status: "Open",
    affected_refs: ["req:c"],
    rule_class: "plumb_core",
  },
  {
    id: "fnd:closed",
    code: "PLUMB.F1.REQ.GROUNDED",
    family: "F1",
    message: "Not grounded",
    severity: "blocker",
    status: "Closed",
    affected_refs: ["req:a"],
    rule_class: "plumb_core",
  },
  {
    id: "fnd:dup",
    code: "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED",
    family: "F1",
    message: "Duplicates",
    severity: "blocker",
    status: "Open",
    affected_refs: ["req:a"],
    rule_class: "plumb_core",
  },
];

// The duplicate finding is waived by the current F1 report (the finding list still says Open).
const GATES: GateStatus[] = [
  {
    gate: "F1",
    state: "current",
    passed: true,
    reason: null,
    findings: ["fnd:dup", "fnd:modality"],
    rules: [],
    waivers: [{ rule_id: "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED", finding_ref: "fnd:dup", finding_key: "sha256:x", decision_ref: "dec:w" }],
    summary: { blocker_failed: 0, blocker_waived: 1, warnings: 0 },
  },
];

function detail(id: string, extra: Partial<RequirementDetail> = {}): RequirementDetail {
  const base = ITEMS.find((item) => item.id === id) ?? (ITEMS[1] as RequirementSummary);
  return {
    ...base,
    spans: ["evd:1"],
    evidence: [{ id: "evd:1", source_ref: "src:1", text: "12. **" + (base.source_identifier ?? "") + "** [functional] " + base.text }],
    lint_findings: [],
    ears_suggestion: null,
    ears_proposal_ref: null,
    linked: { operations: base.coverage.operations, processes: [], scenarios: base.coverage.scenarios },
    labels: [
      { id: "operation:submit", label: "Submit leave request" },
      { id: "scenario:draft", label: "Submit valid draft" },
    ],
    ...extra,
  };
}

const PREVIEW: Preview = {
  proposal_id: "prop:split",
  built_at: "2026-05-01T12:00:00.000000000Z",
  action: "create",
  revision: "rev:7:preview",
  version: "rev:7:preview",
  semantic_hash: "psg:sha256:preview",
  events: [],
  diff: { base_semantic_hash: "psg:sha256:base", candidate_semantic_hash: "psg:sha256:cand", added: ["req:a1", "req:a2"], removed: [], modified: ["req:a"] },
  intake: { requires_reason: true, reason_codes: [], conflicts_with: [], duplicate_candidates: [], validation_findings: [] },
} as unknown as Preview;

interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

interface Backend {
  items: RequirementSummary[];
  health: components["schemas"]["ItemHealth"][];
  roles: string[];
  details: Record<string, RequirementDetail>;
  listStatus: number;
  splitConflict: boolean;
  findings: Finding[];
  calls: Call[];
}

function newBackend(overrides: Partial<Backend> = {}): Backend {
  return {
    items: ITEMS,
    health: [{ id: "req:broken", code: "E_STORE_CORRUPT", message: "The requirement could not be projected." }],
    roles: ["analyst"],
    details: {},
    listStatus: 200,
    splitConflict: false,
    findings: FINDINGS,
    calls: [],
    ...overrides,
  };
}

const WRITE = { version: "rev:8", revision: "rev:8", semantic_hash: "psg:sha256:8", events: [] };

function handlers(backend: Backend) {
  const record = async (request: Request) => {
    const url = new URL(request.url);
    let body: unknown = null;
    if (request.method !== "GET") {
      body = await request.clone().json();
    }
    backend.calls.push({ method: request.method, path: decodeURIComponent(url.pathname), search: url.searchParams, ifMatch: request.headers.get("If-Match"), body });
  };
  return [
    http.get("*/api/me", () => HttpResponse.json({ user: "Marta Nowak", mode: "pilot", roles: backend.roles, inference_available: true })),
    http.get("*/api/requirements", async ({ request }) => {
      await record(request);
      if (backend.listStatus !== 200) {
        return HttpResponse.json({ code: "E_INTERNAL", message: "boom", details: {} }, { status: backend.listStatus });
      }
      // Two pages: the screen reads the whole list (filters and document order are local).
      const cursor = new URL(request.url).searchParams.get("cursor");
      if (cursor === null && backend.items.length > 2) {
        return etagJson({ items: backend.items.slice(0, 2), health: backend.health, next_cursor: "page-2" }, undefined, LIST_ETAG);
      }
      return etagJson({ items: cursor === null ? backend.items : backend.items.slice(2), health: cursor === null ? backend.health : [], next_cursor: null }, undefined, LIST_ETAG);
    }),
    http.get("*/api/requirements/:id/readiness", async ({ request }) => {
      await record(request);
      return etagJson({ overall: 92, sub: { clarity: 95, completeness: 90, confirmation: null, consistency: 100, verifiability: 81 }, contributing: [] });
    }),
    http.get("*/api/requirements/:id", async ({ request, params }) => {
      await record(request);
      const id = String(params["id"]);
      return etagJson(backend.details[id] ?? detail(id), undefined, DETAIL_ETAG);
    }),
    http.get("*/api/findings", () => etagJson({ items: backend.findings, next_cursor: null }, undefined, FINDINGS_ETAG)),
    http.get("*/api/gates", () => etagJson(GATES)),
    http.get("*/api/sources", () =>
      etagJson({ items: [{ id: "src:1", name: "requirements.md", hash: "sha256:1", media_type: "text/markdown", source_kind: "markdown", imported_at: "2026-05-01T12:00:00Z", spans_count: 38 }] }),
    ),
    http.post("*/api/requirements/:id/accept-ears", async ({ request }) => {
      await record(request);
      return HttpResponse.json(WRITE);
    }),
    http.post("*/api/requirements/:id/waive-finding", async ({ request }) => {
      await record(request);
      return HttpResponse.json(WRITE);
    }),
    http.post("*/api/requirements/:id/classify", async ({ request }) => {
      await record(request);
      return HttpResponse.json(WRITE);
    }),
    http.post("*/api/requirements/:id/mark-duplicate", async ({ request }) => {
      await record(request);
      return HttpResponse.json(WRITE);
    }),
    http.post("*/api/requirements/:id/split/preview", async ({ request }) => {
      await record(request);
      return HttpResponse.json(PREVIEW);
    }),
    http.post("*/api/requirements/:id/split", async ({ request }) => {
      await record(request);
      if (backend.splitConflict) {
        backend.splitConflict = false;
        return HttpResponse.json({ code: "E_PREVIEW_MISMATCH", message: "the change is no longer the previewed one", details: {} }, { status: 409 });
      }
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

const list = () => screen.getByRole("list", { name: "Requirements" });
const rowIds = () => within(list()).queryAllByTestId("requirement-row").map((row) => row.getAttribute("data-requirement-id"));
const inspector = () => screen.getByRole("complementary", { name: "Requirement details" });
const writes = (backend: Backend) => backend.calls.filter((c) => c.method !== "GET");

// ------------------------------------------------------------------ list

describe("Requirements list", () => {
  it("lists every requirement in document order with ID, statement, kind, issues, coverage and readiness", async () => {
    const { backend } = renderAt("/requirements");
    expect(await screen.findByRole("heading", { level: 1, name: "Requirements" })).toBeTruthy();
    await waitFor(() => expect(rowIds()).toEqual(["req:a", "req:c", "req:b"]));
    // Both pages were read, without server filters.
    const reads = backend.calls.filter((c) => c.method === "GET" && c.path === "/api/requirements");
    expect(reads.map((c) => c.search.get("cursor"))).toEqual([null, "page-2"]);
    expect(reads.every((c) => c.search.get("q") === null && c.search.get("class") === null)).toBe(true);
    expect(screen.getByText("3 · from requirements.md")).toBeTruthy();
    const hr12 = within(list()).getAllByTestId("requirement-row")[1] as HTMLElement;
    expect(within(hr12).getByText("HR-012")).toBeTruthy();
    expect(within(hr12).getByText("A manager should decide on a submitted leave request within 2 days.")).toBeTruthy();
    expect(within(hr12).getByText("Operational")).toBeTruthy();
    // One open finding; the closed and the waived ones do not count.
    expect(within(hr12).getByText("1 issue")).toBeTruthy();
    expect(within(hr12).getByText("No operation yet")).toBeTruthy();
    expect(within(hr12).getByText("No scenario yet")).toBeTruthy();
    expect(within(hr12).getByRole("img", { name: "Readiness 61%" })).toBeTruthy();
    const hr9 = within(list()).getAllByTestId("requirement-row")[0] as HTMLElement;
    expect(within(hr9).queryByText(/issue/)).toBeNull();
    expect(within(hr9).getByText("Implemented by an operation")).toBeTruthy();
    expect(within(hr9).getByText("Verified by a scenario")).toBeTruthy();
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("shows a row that failed to load with Retry while the rest of the list works", async () => {
    const { user, backend } = renderAt("/requirements");
    const failed = await screen.findByText("Couldn't load req:broken.");
    expect(rowIds()).toHaveLength(3);
    const before = backend.calls.filter((c) => c.path === "/api/requirements").length;
    await user.click(within(failed.closest("li") as HTMLElement).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(backend.calls.filter((c) => c.path === "/api/requirements").length).toBeGreaterThan(before));
  });

  it("searches, filters by kind, status, issues, readiness and coverage, and sorts", async () => {
    const { user } = renderAt("/requirements");
    await waitFor(() => expect(rowIds()).toHaveLength(3));
    await user.type(screen.getByRole("searchbox", { name: "Search requirements" }), "hr-030");
    expect(rowIds()).toEqual(["req:b"]);
    await user.clear(screen.getByRole("searchbox", { name: "Search requirements" }));
    await user.type(screen.getByRole("searchbox", { name: "Search requirements" }), "manager");
    expect(rowIds()).toEqual(["req:c"]);
    await user.clear(screen.getByRole("searchbox", { name: "Search requirements" }));

    await user.click(screen.getByRole("button", { name: "Kind" }));
    await user.click(await screen.findByRole("menuitem", { name: "Quality" }));
    expect(rowIds()).toEqual(["req:b"]);
    await user.click(screen.getByRole("button", { name: "Kind: Quality" }));
    await user.click(await screen.findByRole("menuitem", { name: "All kinds" }));
    expect(rowIds()).toHaveLength(3);

    await user.click(screen.getByRole("button", { name: "Status" }));
    await user.click(await screen.findByRole("menuitem", { name: "Accepted" }));
    expect(rowIds()).toHaveLength(3);
    await user.click(screen.getByRole("button", { name: "Status: Accepted" }));
    await user.click(await screen.findByRole("menuitem", { name: "All statuses" }));

    await user.click(screen.getByRole("button", { name: "Has issues" }));
    expect(rowIds()).toEqual(["req:c"]);
    await user.click(screen.getByRole("button", { name: "Has issues" }));

    await user.click(screen.getByRole("button", { name: "Low readiness" }));
    expect(rowIds()).toEqual(["req:c"]);
    await user.click(screen.getByRole("button", { name: "Low readiness" }));

    await user.click(screen.getByRole("button", { name: "Uncovered" }));
    expect(rowIds()).toEqual(["req:c", "req:b"]);
    await user.click(screen.getByRole("button", { name: "Uncovered" }));

    await user.click(screen.getByRole("button", { name: "Sort: Document order" }));
    await user.click(await screen.findByRole("menuitem", { name: "Readiness" }));
    expect(rowIds()).toEqual(["req:c", "req:b", "req:a"]);
    await user.click(screen.getByRole("button", { name: "Sort: Readiness" }));
    await user.click(await screen.findByRole("menuitem", { name: "Issues" }));
    expect(rowIds()[0]).toBe("req:c");

    await user.type(screen.getByRole("searchbox", { name: "Search requirements" }), "nothing like this");
    expect(screen.getByRole("heading", { name: "No requirements match these filters" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(rowIds()).toHaveLength(3);
  });

  it("filters to the items a gate rule affects (Quality → Show affected) and clears it", async () => {
    const { user, router } = renderAt("/requirements?affected=req:a,req:b");
    await waitFor(() => expect(rowIds()).toEqual(["req:a", "req:b"]));
    await user.click(screen.getByRole("button", { name: "Affected items · 2" }));
    await waitFor(() => expect(router.state.location.search).toEqual({}));
    expect(rowIds()).toHaveLength(3);
  });

  it("shows the empty state with an Import action when nothing was imported", async () => {
    const { user, router } = renderAt("/requirements", newBackend({ items: [], health: [] }));
    expect(await screen.findByRole("heading", { name: "Import a document to see its requirements" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Import" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/sources"));
  });

  it("shows a read failure with Retry", async () => {
    const backend = newBackend({ listStatus: 500 });
    const { user } = renderAt("/requirements", backend);
    expect((await screen.findByRole("alert")).textContent).toContain("The requirements could not be loaded.");
    backend.listStatus = 200;
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(rowIds()).toHaveLength(3));
  });

  it("moves the selection with J and K and opens the selected requirement's URL", async () => {
    const { user, router } = renderAt("/requirements");
    await waitFor(() => expect(rowIds()).toHaveLength(3));
    await user.click((within(list()).getAllByTestId("requirement-row")[0] as HTMLElement).querySelector("button") as HTMLElement);
    await waitFor(() => expect(router.state.location.pathname).toBe("/requirements/req:a"));
    await user.keyboard("j");
    await waitFor(() => expect(router.state.location.pathname).toBe("/requirements/req:c"));
    await user.keyboard("k");
    await waitFor(() => expect(router.state.location.pathname).toBe("/requirements/req:a"));
  });
});

// ------------------------------------------------------------------ inspector

describe("Requirement inspector", () => {
  it("shows the statement, kind, status, readiness with sub-scores, evidence and linked chips", async () => {
    const { user, router } = renderAt("/requirements/req:a");
    const panel = await waitFor(() => inspector());
    expect(await within(panel).findByText("Submitting a valid leave request shall change its status from Draft to Submitted.", { selector: "p" })).toBeTruthy();
    expect(within(panel).getAllByText("HR-009")[0]).toBeTruthy();
    expect(within(panel).getByText("Functional")).toBeTruthy();
    expect(within(panel).getByText("Accepted")).toBeTruthy();
    // Readiness: the ring and the five sub-scores.
    const readiness = within(panel).getByRole("region", { name: "Readiness" });
    expect(await within(readiness).findByText("92")).toBeTruthy();
    expect(within(readiness).getByText("Clarity").nextElementSibling?.textContent).toBe("95");
    expect(within(readiness).getByText("Confirmation").nextElementSibling?.textContent).toBe("Not calculated");
    // Evidence: the passage without Markdown, the statement highlighted, the source named.
    const evidence = within(panel).getByRole("figure", { name: "Evidence from requirements.md" });
    expect(evidence.querySelector("mark")?.textContent).toBe("Submitting a valid leave request shall change its status from Draft to Submitted.");
    expect(evidence.textContent).not.toContain("**");
    // The waived duplicate and the closed finding are not open issues.
    expect(within(panel).queryByRole("region", { name: "Quality issues" })).toBeNull();
    await user.click(within(evidence).getByRole("button", { name: "Open in source" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/sources/src:1"));
    act(() => router.history.push("/requirements/req:a"));
    const linked = await within(await waitFor(() => inspector())).findByRole("region", { name: "Linked" });
    await user.click(within(linked).getByRole("button", { name: "Scenario: Submit valid draft" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/scenarios/scenario:draft"));
    act(() => router.history.push("/requirements/req:a"));
    const again = await within(await waitFor(() => inspector())).findByRole("region", { name: "Linked" });
    await user.click(within(again).getByRole("button", { name: "Submit leave request" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/model/processes"));
  });

  it("waives a quality issue with a reason against the reviewed version", async () => {
    const { user, backend } = renderAt("/requirements/req:c");
    const issues = await within(await waitFor(() => inspector())).findByRole("region", { name: "Quality issues" });
    expect(within(issues).getByText("Weak modality 'should'")).toBeTruthy();
    expect(within(issues).getByText("PLUMB.F1.REQ.MODALITY_EXPLICIT")).toBeTruthy();
    await user.click(within(issues).getByRole("button", { name: "Waive" }));
    const sheet = await screen.findByRole("dialog", { name: "Waive this finding" });
    expect((within(sheet).getByRole("button", { name: /Waive/ }) as HTMLButtonElement).disabled).toBe(true);
    await user.type(within(sheet).getByRole("textbox"), "Accepted for the pilot.");
    await user.click(within(sheet).getByRole("button", { name: /Waive/ }));
    await waitFor(() => expect(writes(backend)).toHaveLength(1));
    expect(writes(backend)[0]).toMatchObject({
      path: "/api/requirements/req:c/waive-finding",
      ifMatch: DETAIL_ETAG,
      body: { finding_id: "fnd:modality", reason: "Accepted for the pilot." },
    });
    expect(await screen.findByText("Finding waived")).toBeTruthy();
  });

  it("offers no Waive for an issue no current gate check produced (the server would refuse it)", async () => {
    const stale: Finding = { ...FINDINGS[0], id: "fnd:recorded", affected_refs: ["req:b"], message: "Recorded before the last check" } as Finding;
    renderAt("/requirements/req:b", newBackend({ findings: [...FINDINGS, stale] }));
    const issues = await within(await waitFor(() => inspector())).findByRole("region", { name: "Quality issues" });
    expect(within(issues).getByText("Recorded before the last check")).toBeTruthy();
    expect(within(issues).queryByRole("button", { name: "Waive" })).toBeNull();
  });

  it("accepts the wording suggestion through accept_requirement_ears", async () => {
    const backend = newBackend({
      details: { "req:b": detail("req:b", { ears_suggestion: "When an employee submits a leave request, the system shall complete the submission within 2 seconds.", ears_proposal_ref: "prop:ears" }) },
    });
    const { user } = renderAt("/requirements/req:b", backend);
    const box = await within(await waitFor(() => inspector())).findByRole("region", { name: "Wording suggestion" });
    await user.click(within(box).getByRole("button", { name: "Accept wording" }));
    await waitFor(() => expect(writes(backend)).toHaveLength(1));
    expect(writes(backend)[0]).toMatchObject({
      path: "/api/requirements/req:b/accept-ears",
      ifMatch: DETAIL_ETAG,
      body: { text: "When an employee submits a leave request, the system shall complete the submission within 2 seconds." },
    });
    expect(await screen.findByText("Wording accepted for HR-030")).toBeTruthy();
  });

  it("changes the kind from the More menu", async () => {
    const { user, backend } = renderAt("/requirements/req:a");
    const panel = await waitFor(() => inspector());
    await within(panel).findByRole("button", { name: "More actions" });
    await user.click(within(panel).getByRole("button", { name: "More actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Change kind…" }));
    const sheet = await screen.findByRole("dialog", { name: "Change kind" });
    expect((within(sheet).getByRole("radio", { name: "Functional" }) as HTMLInputElement).checked).toBe(true);
    await user.click(within(sheet).getByRole("radio", { name: "Security" }));
    await user.click(within(sheet).getByRole("button", { name: /Change kind/ }));
    await waitFor(() => expect(writes(backend)).toHaveLength(1));
    expect(writes(backend)[0]).toMatchObject({ path: "/api/requirements/req:a/classify", ifMatch: DETAIL_ETAG, body: { class: "security" } });
    expect(await screen.findByText("HR-009 is now Security")).toBeTruthy();
  });

  it("splits into two after a server preview, and asks for a new preview when it is out of date", async () => {
    const backend = newBackend({ splitConflict: true });
    const { user } = renderAt("/requirements/req:a", backend);
    const panel = await waitFor(() => inspector());
    await within(panel).findByRole("button", { name: "More actions" });
    await user.click(within(panel).getByRole("button", { name: "More actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Split into two…" }));
    const sheet = await screen.findByRole("dialog", { name: "Split HR-009 into two" });
    const confirm = () => within(sheet).getByRole("button", { name: /^Split/ }) as HTMLButtonElement;
    expect(confirm().disabled).toBe(true);
    await user.type(within(sheet).getByRole("textbox", { name: "Part 1" }), "Submitting a valid leave request shall record it.");
    await user.type(within(sheet).getByRole("textbox", { name: "Part 2" }), "A submitted leave request shall have status Submitted.");
    await user.type(within(sheet).getByRole("textbox", { name: "Reason (required)" }), "Two obligations.");
    await user.click(within(sheet).getByRole("button", { name: "Preview split" }));
    const result = await within(sheet).findByRole("region", { name: "Resulting requirements" });
    expect(within(result).getByText("Submitting a valid leave request shall record it.")).toBeTruthy();
    expect(within(result).getByText("2 added · 1 changed · 0 removed")).toBeTruthy();
    expect(writes(backend)[0]).toMatchObject({
      path: "/api/requirements/req:a/split/preview",
      ifMatch: DETAIL_ETAG,
      body: { parts: ["Submitting a valid leave request shall record it.", "A submitted leave request shall have status Submitted."], reason: "Two obligations." },
    });
    // The first confirmation finds the project changed: the preview is out of date.
    await user.click(confirm());
    expect(await within(sheet).findByText("Preview is out of date")).toBeTruthy();
    expect(confirm().disabled).toBe(true);
    await user.click(within(sheet).getByRole("button", { name: "Preview again" }));
    await within(sheet).findByRole("region", { name: "Resulting requirements" });
    await user.click(confirm());
    await waitFor(() => expect(writes(backend).filter((c) => c.path === "/api/requirements/req:a/split")).toHaveLength(2));
    expect(writes(backend).at(-1)).toMatchObject({
      ifMatch: '"rev:7:preview"',
      body: { reason: "Two obligations.", preview: { proposal_id: "prop:split", built_at: "2026-05-01T12:00:00.000000000Z" } },
    });
    expect(await screen.findByText("HR-009 split into 2 requirements")).toBeTruthy();
  });

  it("marks the requirement as a duplicate of another one", async () => {
    const { user, backend } = renderAt("/requirements/req:a");
    const panel = await waitFor(() => inspector());
    await within(panel).findByRole("button", { name: "More actions" });
    await user.click(within(panel).getByRole("button", { name: "More actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Mark as duplicate of…" }));
    const sheet = await screen.findByRole("dialog", { name: "Mark HR-009 as a duplicate" });
    await user.selectOptions(within(sheet).getByRole("combobox", { name: "Duplicate of" }), "req:b");
    await user.type(within(sheet).getByRole("textbox", { name: "Reason (required)" }), "Same obligation.");
    await user.click(within(sheet).getByRole("button", { name: /Mark as duplicate/ }));
    await waitFor(() => expect(writes(backend)).toHaveLength(1));
    expect(writes(backend)[0]).toMatchObject({ path: "/api/requirements/req:a/mark-duplicate", ifMatch: DETAIL_ETAG, body: { of: "req:b", reason: "Same obligation." } });
    expect(await screen.findByText("HR-009 marked as a duplicate")).toBeTruthy();
  });

  it("offers no change controls to a role that cannot change requirements", async () => {
    const backend = newBackend({
      roles: ["reviewer"],
      details: { "req:c": detail("req:c", { ears_suggestion: "When a leave request is submitted, the manager shall decide within 2 working days.", ears_proposal_ref: "prop:ears" }) },
    });
    renderAt("/requirements/req:c", backend);
    const panel = await waitFor(() => inspector());
    await within(panel).findByRole("region", { name: "Quality issues" });
    expect(within(panel).queryByRole("button", { name: "More actions" })).toBeNull();
    expect(within(panel).queryByRole("button", { name: "Waive" })).toBeNull();
    expect(within(panel).queryByRole("button", { name: "Accept wording" })).toBeNull();
    expect(await axeViolations(document.body)).toEqual([]);
  });
});
