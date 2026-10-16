// U2.1 requirement tests: the server-filtered, cursor-paged list with advisory readiness; the
// detail with separate text, EARS, evidence, lint and link sections; the four direct writes
// through the generated client and the versioned mutation (If-Match); and the split
// preview-confirm workflow (Hotfix 073/074: server preview first, create-class proposal, exact
// server diff, explicit confirmation through /api/proposals/{id}/create with the same reason).
// Every HTTP call is answered by MSW and recorded; nothing touches the network.
//
// The generated client captures `fetch` and `Request` when its module is first evaluated, so the
// app modules are imported dynamically after MSW is listening, and relative URLs are resolved
// against the jsdom location as a browser would.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { server } from "../../test/msw";

type RequirementPage = components["schemas"]["RequirementPage"];
type RequirementDetail = components["schemas"]["RequirementDetail"];
type Preview = components["schemas"]["SemanticChangePreviewResponse"];
type WriteResult = components["schemas"]["WriteResult"];

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
});

// ------------------------------------------------------------------ fixtures

const CURRENT = "psg:sha256:current";
const CANDIDATE = "psg:sha256:candidate";
const FINAL = "psg:sha256:final";

const PAGE_1: RequirementPage = {
  items: [
    {
      id: "req:a",
      text: "Employees shall submit and cancel leave requests.",
      class: "functional",
      status: "Accepted",
      smells: ["PLUMB.LINT.COMPOUND"],
      coverage: { operations: [], scenarios: [] },
      readiness: 0.42,
      lint_current: true,
    },
    {
      id: "req:b",
      text: "Managers shall approve leave requests.",
      class: "functional",
      status: "Proposed",
      smells: [],
      coverage: { operations: [], scenarios: [] },
      readiness: null,
      lint_current: true,
    },
  ],
  next_cursor: "cursor-page-2",
};

const PAGE_2: RequirementPage = {
  items: [
    {
      id: "req:c",
      text: "The system shall record approvals.",
      class: "quality",
      status: "Accepted",
      smells: [],
      coverage: { operations: [], scenarios: [] },
      readiness: null,
      lint_current: true,
    },
  ],
  next_cursor: null,
};

const DETAIL_A: RequirementDetail = {
  id: "req:a",
  text: "Employees shall submit and cancel leave requests.",
  class: "functional",
  status: "Accepted",
  smells: ["PLUMB.LINT.COMPOUND"],
  coverage: { operations: ["op:submit"], scenarios: [] },
  readiness: 0.42,
  spans: ["frag:requirements.docx#p12", "frag:requirements.docx#p13"],
  lint_findings: ["fnd:compound-a"],
  ears_suggestion: "When an employee submits a leave request, the system shall record it.",
  linked: { operations: ["op:submit"], processes: ["proc:leave"], scenarios: ["scn:submit"] },
  lint_current: true,
};

const DETAIL_B: RequirementDetail = {
  id: "req:b",
  text: "Managers shall approve leave requests.",
  class: "functional",
  status: "Proposed",
  smells: [],
  coverage: { operations: [], scenarios: [] },
  readiness: null,
  spans: [],
  lint_findings: [],
  ears_suggestion: null,
  linked: { operations: [], processes: [], scenarios: [] },
  lint_current: true,
};

/** A server preview whose lists are deliberately not in sorted order: the UI must not reorder. */
const SPLIT_PREVIEW: Preview = {
  version: "rev:7",
  revision: "rev:7",
  semantic_hash: CURRENT,
  events: [],
  proposal_id: "prop:split-a",
  action: "create",
  intake: {
    covered_by: [],
    conflicts_with: [],
    refines: [],
    new_terms: [],
    novelty: null,
    duplicate_candidates: [],
    validation_findings: [],
    reason_codes: ["human_decision", "material_decision"],
    requires_reason: true,
  },
  diff: {
    base_semantic_hash: CURRENT,
    candidate_semantic_hash: CANDIDATE,
    added: ["req:a-2", "req:a-1", "rel:decomposes-a-2", "rel:decomposes-a-1"],
    removed: [],
    modified: ["req:a"],
  },
};

const WRITE: WriteResult = { version: "rev:8", revision: "rev:8", semantic_hash: FINAL, events: [] };

interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

/** MSW handlers for the shell and every requirement endpoint, recording each call in order. */
function handlers(calls: Call[], overrides: { create?: () => Response } = {}) {
  const record = async (request: Request, method: string) => {
    const url = new URL(request.url);
    const text = method === "GET" ? "" : await request.text();
    calls.push({
      method,
      path: url.pathname,
      search: url.searchParams,
      ifMatch: request.headers.get("if-match"),
      body: text === "" ? null : (JSON.parse(text) as unknown),
    });
  };
  const write = (status = 200) => async ({ request }: { request: Request }) => {
    await record(request, "POST");
    return HttpResponse.json(WRITE, { status });
  };
  return [
    http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles: ["analyst"] })),
    http.get("*/api/model/version", () =>
      HttpResponse.json({ version: "rev:7", revision: "rev:7", hash: CURRENT, semantic_hash: CURRENT, evidence_hash: "ev:1" }),
    ),
    http.get("*/api/requirements", async ({ request }) => {
      await record(request, "GET");
      const cursor = new URL(request.url).searchParams.get("cursor");
      return HttpResponse.json(cursor === "cursor-page-2" ? PAGE_2 : PAGE_1);
    }),
    http.get("*/api/requirements/:id", async ({ request, params }) => {
      await record(request, "GET");
      return HttpResponse.json(params["id"] === "req:b" ? DETAIL_B : DETAIL_A);
    }),
    http.post("*/api/requirements/:id/classify", write()),
    http.post("*/api/requirements/:id/accept-ears", write()),
    http.post("*/api/requirements/:id/waive-finding", write()),
    http.post("*/api/requirements/:id/mark-duplicate", write()),
    http.post("*/api/requirements/:id/split", write()),
    http.post("*/api/requirements/:id/split/preview", async ({ request }) => {
      await record(request, "POST");
      return HttpResponse.json(SPLIT_PREVIEW);
    }),
    http.post("*/api/proposals/:id/create", async ({ request }) => {
      await record(request, "POST");
      return overrides.create === undefined ? HttpResponse.json(WRITE) : overrides.create();
    }),
    http.post("*/api/proposals/:id/supersede", write()),
  ];
}

function renderAt(path: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { router, queryClient };
}

const posts = (calls: Call[], suffix: string) => calls.filter((c) => c.method === "POST" && c.path.endsWith(suffix));
const decoded = (path: string) => decodeURIComponent(path);

// ------------------------------------------------------------------ list

describe("RequirementsPage", () => {
  it("sends the status, class and text filters to the server and pages with its cursor", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderAt("/requirements");
    const user = userEvent.setup();

    expect(await screen.findByText(PAGE_1.items[0]?.text ?? "")).toBeTruthy();
    const first = calls.find((c) => c.path === "/api/requirements");
    expect(first?.search.get("cursor")).toBeNull();
    expect(first?.search.get("limit")).toBe("25");

    const form = screen.getByRole("form", { name: "Requirement filters" });
    await user.type(within(form).getByLabelText("Status"), "Accepted");
    await user.type(within(form).getByLabelText("Class"), "functional");
    await user.type(within(form).getByLabelText("Text"), "leave");
    await user.click(within(form).getByRole("button", { name: "Apply filters" }));
    await waitFor(() => {
      const last = calls.filter((c) => c.path === "/api/requirements").at(-1);
      expect(last?.search.get("status")).toBe("Accepted");
      expect(last?.search.get("class")).toBe("functional");
      expect(last?.search.get("q")).toBe("leave");
      expect(last?.search.get("cursor")).toBeNull();
    });

    // Cursor pagination: the next page sends the server's next_cursor with the same filters.
    await user.click(screen.getByRole("button", { name: "Next page" }));
    expect(await screen.findByText("The system shall record approvals.")).toBeTruthy();
    const paged = calls.filter((c) => c.path === "/api/requirements").at(-1);
    expect(paged?.search.get("cursor")).toBe("cursor-page-2");
    expect(paged?.search.get("status")).toBe("Accepted");
    expect((screen.getByRole("button", { name: "Next page" }) as HTMLButtonElement).disabled).toBe(true);
    await user.click(screen.getByRole("button", { name: "Previous page" }));
    expect(await screen.findByText(PAGE_1.items[1]?.text ?? "")).toBeTruthy();
  });

  it("shows readiness as advisory and null readiness as not calculated", async () => {
    server.use(...handlers([]));
    renderAt("/requirements");
    const rowA = (await screen.findByText("req:a")).closest("tr");
    const rowB = screen.getByText("req:b").closest("tr");
    expect(rowA?.querySelector("[data-readiness]")?.textContent).toBe("0.42");
    expect(rowB?.querySelector("[data-readiness]")?.getAttribute("data-readiness")).toBe("null");
    expect(rowB?.querySelector("[data-readiness]")?.textContent).toBe("Not calculated");
    expect(screen.getByText("Readiness is advisory only and never decides a gate.")).toBeTruthy();
    // No gate vocabulary is attached to readiness.
    expect(screen.queryByText(/PASS|FAIL/)).toBeNull();
  });

  it("shows lint smells only when the F1 report is current, never unavailable lint as zero", async () => {
    // R0.4: a requirement read without a current F1 report carries lint_current false.
    const unavailable: RequirementPage = {
      items: [
        {
          id: "req:u",
          text: "The system shall record approvals.",
          class: "quality",
          status: "Accepted",
          smells: [],
          coverage: { operations: [], scenarios: [] },
          readiness: null,
          lint_current: false,
        },
      ],
      next_cursor: null,
    };
    server.use(http.get("*/api/requirements", () => HttpResponse.json(unavailable)), ...handlers([]));
    renderAt("/requirements");
    const row = (await screen.findByText("req:u")).closest("tr");
    const cell = row?.querySelector("[data-lint]");
    expect(cell?.getAttribute("data-lint")).toBe("unavailable");
    expect(cell?.textContent).toBe("Not calculated");
    expect(cell?.textContent).not.toBe("0");
    cleanup();

    server.use(...handlers([]));
    renderAt("/requirements");
    const current = (await screen.findByText("req:a")).closest("tr")?.querySelector("[data-lint]");
    expect(current?.getAttribute("data-lint")).toBe("current");
    expect(current?.textContent).toBe("1");
  });

  it("navigates to the requirement detail route", async () => {
    server.use(...handlers([]));
    const { router } = renderAt("/requirements");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("link", { name: "req:a" }));
    expect(await screen.findByTestId("requirement-text")).toBeTruthy();
    expect(decoded(router.state.location.pathname)).toBe("/requirements/req:a");
  });
});

// ------------------------------------------------------------------ detail

describe("RequirementDetail", () => {
  it("shows original text, EARS, evidence, lint findings and links in separate sections", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderAt("/requirements/req:a");
    const user = userEvent.setup();

    const original = await screen.findByRole("region", { name: "Original text" });
    expect(within(original).getByTestId("requirement-text").textContent).toBe(DETAIL_A.text);
    const ears = screen.getByRole("region", { name: "EARS suggestion" });
    expect(within(ears).getByTestId("ears-suggestion").textContent).toBe(DETAIL_A.ears_suggestion);
    // The EARS text is not the original text and the original is not in the EARS section.
    expect(within(ears).queryByText(DETAIL_A.text)).toBeNull();

    const evidence = screen.getByRole("region", { name: "Source evidence" });
    expect(within(evidence).getAllByRole("listitem").map((li) => li.textContent)).toEqual(DETAIL_A.spans);
    // The drawer presents the server refs as evidence, without invented source text.
    await user.click(within(evidence).getByRole("button", { name: "Show provenance" }));
    const drawer = await screen.findByRole("dialog");
    const items = Array.from(drawer.querySelectorAll("li[data-category]"));
    expect(items.map((li) => [li.getAttribute("data-category"), li.textContent])).toEqual(DETAIL_A.spans.map((s) => ["evidence", s]));
    await user.click(within(drawer).getByRole("button", { name: "Close" }));

    const lint = screen.getByRole("region", { name: "Lint findings" });
    expect(within(lint).getByTestId("lint-findings").textContent).toBe("fnd:compound-a");
    expect(within(lint).getByTestId("lint-smells").textContent).toBe("PLUMB.LINT.COMPOUND");
    const linked = screen.getByRole("region", { name: "Linked elements" });
    expect(linked.querySelector("[data-linked=operations]")?.textContent).toBe("op:submit");
    expect(linked.querySelector("[data-linked=processes]")?.textContent).toBe("proc:leave");
    expect(linked.querySelector("[data-linked=scenarios]")?.textContent).toBe("scn:submit");
    // Reading performs no write.
    expect(calls.filter((c) => c.method === "POST")).toEqual([]);
  });

  it("shows lint as unavailable without a current F1 report and as findings when current", async () => {
    // R0.4: lint_current false is unavailable lint, never "no findings".
    const unavailable: RequirementDetail = { ...DETAIL_A, smells: [], lint_findings: [], readiness: null, lint_current: false };
    server.use(http.get("*/api/requirements/:id", () => HttpResponse.json(unavailable)), ...handlers([]));
    renderAt("/requirements/req:a");
    const lint = await screen.findByRole("region", { name: "Lint findings" });
    expect(lint.getAttribute("data-lint")).toBe("unavailable");
    expect(within(lint).getByTestId("lint-unavailable").textContent).toBe("Not calculated");
    expect(within(lint).queryByText("None")).toBeNull();
    expect(within(lint).queryByTestId("lint-findings")).toBeNull();
    expect(within(lint).queryByTestId("lint-smells")).toBeNull();
    cleanup();

    server.use(...handlers([]));
    renderAt("/requirements/req:b");
    const current = await screen.findByRole("region", { name: "Lint findings" });
    expect(current.getAttribute("data-lint")).toBe("current");
    expect(within(current).queryByTestId("lint-unavailable")).toBeNull();
    expect(within(current).getAllByText("None")).toHaveLength(2);
  });

  it("shows a null EARS suggestion and null readiness as not calculated, with no accept action", async () => {
    server.use(...handlers([]));
    renderAt("/requirements/req:b");
    const ears = await screen.findByRole("region", { name: "EARS suggestion" });
    expect(within(ears).getByTestId("ears-unavailable").textContent).toBe("Not calculated: no EARS suggestion is available.");
    expect(screen.queryByTestId("ears-accept")).toBeNull();
    expect(screen.getByTestId("requirement-readiness").textContent).toBe("Not calculated");
    expect(within(screen.getByRole("region", { name: "Source evidence" })).getByText("No evidence references.")).toBeTruthy();
  });

  it("sends classify, accept-EARS, waive and mark-duplicate through the versioned client with If-Match", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderAt("/requirements/req:a");
    const user = userEvent.setup();
    await screen.findByTestId("requirement-text");

    await user.type(screen.getByTestId("classify-class"), "quality");
    await user.click(screen.getByRole("button", { name: "Reclassify" }));
    await waitFor(() => expect(posts(calls, "/classify")).toHaveLength(1));

    await user.click(screen.getByTestId("ears-accept"));
    await waitFor(() => expect(posts(calls, "/accept-ears")).toHaveLength(1));

    await user.selectOptions(screen.getByTestId("waive-finding"), "fnd:compound-a");
    await user.type(screen.getByTestId("waive-reason"), "Accepted risk.");
    await user.click(screen.getByRole("button", { name: "Waive" }));
    await waitFor(() => expect(posts(calls, "/waive-finding")).toHaveLength(1));

    await user.type(screen.getByTestId("duplicate-of"), "req:b");
    await user.type(screen.getByTestId("duplicate-reason"), "Same intent.");
    await user.click(screen.getByRole("button", { name: "Mark duplicate" }));
    await waitFor(() => expect(posts(calls, "/mark-duplicate")).toHaveLength(1));

    const writes = calls.filter((c) => c.method === "POST");
    expect(writes.map((c) => [decoded(c.path), c.ifMatch, c.body])).toEqual([
      ["/api/requirements/req:a/classify", CURRENT, { class: "quality" }],
      ["/api/requirements/req:a/accept-ears", CURRENT, { text: DETAIL_A.ears_suggestion }],
      ["/api/requirements/req:a/waive-finding", CURRENT, { finding_id: "fnd:compound-a", reason: "Accepted risk." }],
      ["/api/requirements/req:a/mark-duplicate", CURRENT, { of: "req:b", reason: "Same intent." }],
    ]);
  });
});

// ------------------------------------------------------------------ split

describe("Requirement split", () => {
  async function previewSplit(calls: Call[]) {
    const user = userEvent.setup();
    await screen.findByTestId("requirement-text");
    const [first, second] = screen.getAllByTestId("split-part");
    if (first === undefined || second === undefined) {
      throw new Error("two split parts expected");
    }
    await user.type(first, "Employees shall submit leave requests.");
    await user.type(second, "Employees shall cancel leave requests.");
    await user.type(screen.getByTestId("split-reason"), "Two behaviours.");
    await user.click(screen.getByTestId("split-preview"));
    await screen.findByTestId("semantic-preview");
    expect(posts(calls, "/split/preview")).toHaveLength(1);
    return user;
  }

  it("previews first, shows the exact server diff and commits only on confirmation through /create", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderAt("/requirements/req:a");
    const user = await previewSplit(calls);

    const preview = posts(calls, "/split/preview")[0];
    expect(decoded(preview?.path ?? "")).toBe("/api/requirements/req:a/split/preview");
    expect(preview?.ifMatch).toBe(CURRENT);
    expect(preview?.body).toEqual({
      parts: ["Employees shall submit leave requests.", "Employees shall cancel leave requests."],
      reason: "Two behaviours.",
    });

    // The server action and the exact server diff, unchanged and unsorted.
    const panel = screen.getByTestId("semantic-preview");
    expect(within(panel).getByTestId("preview-action").textContent).toBe("create");
    expect(within(panel).getByTestId("preview-proposal").textContent).toBe("prop:split-a");
    const section = (key: string) =>
      Array.from(panel.querySelectorAll("[data-diff=" + key + "] li")).map((li) => li.textContent);
    expect(section("added")).toEqual(SPLIT_PREVIEW.diff.added);
    expect(section("removed")).toEqual([]);
    expect(section("modified")).toEqual(SPLIT_PREVIEW.diff.modified);
    expect(within(panel).getByTestId("diff-from").textContent).toBe(CURRENT);
    expect(within(panel).getByTestId("diff-to").textContent).toBe(CANDIDATE);
    // The candidate hash is labelled as the uncommitted candidate, never committed or final.
    const candidateLabel = within(panel).getByTestId("preview-candidate-hash").previousElementSibling?.textContent ?? "";
    expect(candidateLabel).toBe("Candidate semantic hash (proposal only, not committed)");
    expect(within(panel).queryByText(/^Committed|final hash/i)).toBeNull();
    expect(within(panel).getByText("Proposed change (not committed)")).toBeTruthy();

    // Nothing is committed before the explicit confirmation.
    expect(posts(calls, "/create")).toEqual([]);
    await user.click(within(panel).getByTestId("preview-confirm"));
    await waitFor(() => expect(screen.queryByTestId("semantic-preview")).toBeNull());

    const created = posts(calls, "/create");
    expect(created.map((c) => [decoded(c.path), c.ifMatch, c.body])).toEqual([
      ["/api/proposals/prop:split-a/create", CURRENT, { reason: "Two behaviours." }],
    ]);
    // Never the direct split nor the supersede action; no stale warning although the committed
    // hash differs from the candidate hash.
    expect(calls.filter((c) => c.method === "POST" && c.path.endsWith("/split"))).toEqual([]);
    expect(posts(calls, "/supersede")).toEqual([]);
    expect(screen.queryByTestId("preview-stale")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("discards a stale preview on E_PROPOSAL_BASELINE_CHANGED without retrying", async () => {
    const calls: Call[] = [];
    server.use(
      ...handlers(calls, {
        create: () =>
          HttpResponse.json(
            { code: "E_PROPOSAL_BASELINE_CHANGED", message: "baseline changed", details: {} },
            { status: 409 },
          ),
      }),
    );
    renderAt("/requirements/req:a");
    const user = await previewSplit(calls);
    await user.click(screen.getByTestId("preview-confirm"));

    expect((await screen.findByTestId("preview-stale")).textContent).toBe(
      "The model changed since this preview, so it was discarded. Request a new preview.",
    );
    expect(screen.queryByTestId("semantic-preview")).toBeNull();
    // A new preview must be requested explicitly: the preview control is offered again.
    expect(screen.getByTestId("split-preview")).toBeTruthy();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(posts(calls, "/create")).toHaveLength(1);
    expect(posts(calls, "/split/preview")).toHaveLength(1);
    expect(posts(calls, "/supersede")).toEqual([]);
  });
});
