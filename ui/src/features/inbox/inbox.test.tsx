// The Inbox (handoff "2. Inbox", spec §5.2, flows 2, 3, 5 and 8 of §6): smart filters with
// counts, the queue, and an inspector card per item type, wired to the real API through MSW.
// Rendered through the real router inside the shell.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider, type AnyRouter } from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse, type JsonBodyType } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { axeViolations } from "../../test/axe";
import { etagJson, server } from "../../test/msw";

type PendingProposal = components["schemas"]["PendingProposal"];
type PendingProposalDetail = components["schemas"]["PendingProposalDetail"];
type Question = components["schemas"]["Question"];
type Finding = components["schemas"]["Finding"];
type GateStatus = components["schemas"]["GateStatus"];
type Me = components["schemas"]["MeResponse"];

interface AppModules {
  router: typeof import("../../app/router");
  toast: typeof import("../../components/ui/Toast");
  run: typeof import("../../app/run");
  conflict: typeof import("../../api/useVersionedMutation");
}

let app: AppModules;

class OriginRequest extends Request {
  constructor(input: RequestInfo | URL, init?: RequestInit) {
    super(typeof input === "string" ? new URL(input, window.location.href) : input, init);
  }
}

class SilentWebSocket extends EventTarget {
  close(): void {}
}

beforeAll(async () => {
  server.listen({ onUnhandledRequest: "error" });
  vi.stubGlobal("Request", OriginRequest);
  vi.stubGlobal("WebSocket", SilentWebSocket);
  vi.stubGlobal("scrollTo", () => undefined);
  app = {
    router: await import("../../app/router"),
    toast: await import("../../components/ui/Toast"),
    run: await import("../../app/run"),
    conflict: await import("../../api/useVersionedMutation"),
  };
});

afterAll(() => {
  server.close();
  vi.unstubAllGlobals();
});

beforeEach(() => {
  window.localStorage.clear();
  app.run.useRunStore.getState().clear();
  app.conflict.useConflictStore.getState().clearConflict();
});

afterEach(() => {
  cleanup();
  server.resetHandlers();
  act(() => app.toast.dismissToast());
});

// ------------------------------------------------------------------ fixtures

const ETAG = '"rev:42:aaaaaaaaaaaaaaaa"';
const HEAD = "rev:42:aaaaaaaaaaaaaaaa";
const ANALYST: Me = { user: "Marta Nowak", mode: "pilot", roles: ["analyst"], inference_available: true, project: { id: "project:pilot", name: "Pilot", database: null } };

const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();

function proposal(id: string, title: string, overrides: Partial<PendingProposal> = {}): PendingProposal {
  return {
    proposal_id: id,
    action: "create",
    origin: "stage",
    base_revision: HEAD,
    discardable: false,
    requires_reason: false,
    stale: false,
    title,
    element_kind: "operation",
    created_at: ago(4),
    stage: "flows",
    ...overrides,
  };
}

function question(id: string, text: string, answer_schema: unknown, overrides: Partial<Question> = {}): Question {
  return {
    id,
    text,
    kind: "YesNo",
    status: "Open",
    priority: 1,
    round_ref: null,
    stakeholder_ref: "stakeholder:hr",
    context_refs: ["calculation:days", "req:hr005"],
    answer_schema: answer_schema as Question["answer_schema"],
    ...overrides,
  };
}

const closed = (properties: Record<string, unknown>) => ({ type: "object", additionalProperties: false, properties, required: Object.keys(properties) });

const SUBMIT = proposal("prop:submit", "Add operation Submit leave request");
const APPROVE = proposal("prop:approve", "Add operation Approve leave request", { created_at: ago(5) });
const WITHDRAW = proposal("prop:withdraw", "Add operation Withdraw leave request", { created_at: ago(6), requires_reason: true });
const EVENT = proposal("prop:event", "Add event Leave request submitted", { created_at: ago(7), element_kind: "event" });
const LINK = proposal("prop:link", "Link Cancel leave request to HR-019", { created_at: ago(65), element_kind: "link", action: "link", stale: true, base_revision: "rev:40:bbbbbbbbbbbbbbbb" });
const EARS = proposal("prop:ears", "Change requirement HR-030", { created_at: ago(60 * 24), element_kind: "requirement", action: "accept", stage: "consolidate" });
const ANSWER = proposal("prop:answer", "Apply the answer to: Are public holidays counted as working days?", {
  created_at: ago(180),
  origin: "question",
  origin_ref: "q:holidays",
  element_kind: "answer",
  stage: null,
  discardable: true,
  requires_reason: true,
});

const DURATION = question("q:duration", "Does the requested duration include the end date?", closed({ inclusive_end: { type: "boolean" } }));
const SELF = question("q:self", "Can a manager approve their own leave request?", closed({ approver: { type: "string", enum: ["No — HR must approve", "Yes"] } }), { kind: "PickOne", priority: 2 });
const HOLIDAYS = question("q:holidays", "Are public holidays counted as working days?", closed({ counted: { type: "boolean" } }));

const DUPLICATE: Finding = {
  id: "fnd:dup",
  code: "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED",
  family: "F1",
  message: "HR-023 and HR-024 look like duplicates",
  severity: "blocker",
  status: "Open",
  affected_refs: ["req:hr023", "req:hr024"],
  rule_class: "plumb_core",
  waiver_ref: null,
};

// "Zażółć" makes the byte offsets differ from the string indices.
const SUBMIT_TEXT = "Zażółć: an employee shall be able to submit a Draft leave request to their manager. The manager is the line manager.";
const SUBMIT_HIGHLIGHT_END = new TextEncoder().encode("Zażółć: an employee shall be able to submit a Draft leave request to their manager.").length;

function detail(p: PendingProposal): PendingProposalDetail {
  const base = {
    action: p.action,
    base_revision: p.base_revision,
    created_at: p.created_at,
    discardable: p.discardable,
    element_kind: p.element_kind,
    origin: p.origin,
    origin_ref: p.origin_ref ?? null,
    proposal_id: p.proposal_id,
    requires_reason: p.requires_reason,
    stage: p.stage ?? null,
    stale: p.stale,
    title: p.title,
  };
  if (p.proposal_id === "prop:submit") {
    return {
      ...base,
      facts: [
        { label: "Performed by", value: "Employee" },
        { label: "Reads", value: "Leave request (Draft)" },
        { label: "Writes", value: "Leave request → Submitted" },
        { label: "Produces", value: "Leave request submitted" },
      ],
      evidence: [
        { id: "req:hr008", source_identifier: "HR-008", source_ref: "src:req", text: SUBMIT_TEXT, highlight_start: 0, highlight_end: SUBMIT_HIGHLIGHT_END },
        { id: "evd:frag", source_identifier: null, source_ref: null, text: null, highlight_start: null, highlight_end: null },
      ],
      diff: {
        added: [
          { id: "operation:submit", kind: "operation", label: "Submit leave request" },
          { id: "rel:1", kind: "performed_by", label: "Submit leave request performed by Employee" },
        ],
        modified: [{ id: "entity:leave", kind: "entity", label: "Leave request" }],
        removed: [],
      },
    };
  }
  if (p.proposal_id === "prop:link") {
    return {
      ...base,
      facts: [
        { label: "From", value: "Cancel leave request" },
        { label: "Relation", value: "implements" },
        { label: "To", value: "HR-019" },
      ],
      evidence: [],
      diff: { added: [{ id: "rel:2", kind: "implements", label: "Cancel leave request implements HR-019" }], modified: [], removed: [] },
    };
  }
  if (p.proposal_id === "prop:answer") {
    return {
      ...base,
      facts: [
        { label: "Question", value: "Are public holidays counted as working days?" },
        { label: "Answer", value: "counted: false" },
      ],
      evidence: [],
      diff: { added: [{ id: "dec:1", kind: "resolution_decision", label: "public holidays are excluded" }], modified: [{ id: "calculation:days", kind: "calculation", label: "RequestedWorkingDays" }], removed: [{ id: "assumption:1", kind: "assumption", label: "holidays count" }] },
    };
  }
  if (p.proposal_id === "prop:ears") {
    return { ...base, facts: [], evidence: [], diff: { added: [], modified: [{ id: "req:hr030", kind: "requirement", label: "HR-030" }], removed: [] } };
  }
  return { ...base, facts: [{ label: "Performed by", value: "Manager" }], evidence: [], diff: { added: [{ id: "operation:" + p.proposal_id, kind: "operation", label: p.title }], modified: [], removed: [] } };
}

function requirement(id: string, text: string, overrides: Record<string, unknown> = {}) {
  return {
    id,
    text,
    class: "quality",
    status: "Accepted",
    smells: [],
    coverage: { operations: [], scenarios: [] },
    readiness: null,
    spans: ["evd:" + id],
    evidence: [{ id: "evd:" + id, source_ref: "src:req", text: "26. **" + id.replace("req:hr", "HR-") + "** " + text }],
    lint_findings: [],
    ears_suggestion: null,
    ears_proposal_ref: null,
    linked: { operations: [], processes: [], scenarios: [] },
    lint_current: true,
    ...overrides,
  };
}

const REQUIREMENTS: Record<string, ReturnType<typeof requirement>> = {
  "req:hr030": requirement("req:hr030", "The leave-request submission operation should complete within 2 seconds under normal office load.", {
    ears_suggestion: "When an employee submits a leave request, the system shall complete the submission within 2 seconds under normal office load.",
    ears_proposal_ref: "prop:ears",
  }),
  "req:hr023": requirement("req:hr023", "The system shall notify the employee when their leave request is approved.", { class: "functional" }),
  "req:hr024": requirement("req:hr024", "The system shall notify the employee when their leave request is rejected.", { class: "functional" }),
};

const SENTENCES = [{ id: "req:hr030", text: REQUIREMENTS["req:hr030"]?.text ?? "", slots: { class: "quality", level: "system", modality: "should" }, status: "Accepted", evidence: [], unknowns: [] }];

interface Write {
  path: string;
  ifMatch: string | null;
  body: unknown;
}

interface Backend {
  me: Me;
  proposals: PendingProposal[];
  questions: Question[];
  findings: Finding[];
  gates: GateStatus[];
  pipelineNext: string | null;
  writes: Write[];
  /** Status code to answer the next write with (e.g. 412), once. */
  failNext: number | null;
}

function newBackend(overrides: Partial<Backend> = {}): Backend {
  return {
    me: ANALYST,
    proposals: [SUBMIT, APPROVE, WITHDRAW, EVENT, LINK, EARS, ANSWER],
    questions: [DURATION, SELF, HOLIDAYS],
    findings: [DUPLICATE],
    gates: [],
    pipelineNext: "directory",
    writes: [],
    failNext: null,
    ...overrides,
  };
}

const writeResult = { version: HEAD, revision: "rev:43:cccccccccccccccc", semantic_hash: "psg:sha256:c", events: [] };

function handlers(backend: Backend) {
  const record = async (request: Request, path: string): Promise<HttpResponse<JsonBodyType> | null> => {
    const text = await request.text();
    backend.writes.push({ path, ifMatch: request.headers.get("If-Match"), body: text === "" ? null : (JSON.parse(text) as unknown) });
    if (backend.failNext !== null) {
      const status = backend.failNext;
      backend.failNext = null;
      return status === 412
        ? HttpResponse.json({ code: "E_PRECONDITION_FAILED", message: "changed", details: {}, revision: "rev:44:dddddddddddddddd", semantic_hash: "psg:sha256:d" }, { status })
        : HttpResponse.json({ code: "E_PROPOSAL_BASELINE_CHANGED", message: "The proposal is out of date.", details: {} }, { status });
    }
    return null;
  };
  const drop = (ids: readonly string[]) => {
    backend.proposals = backend.proposals.filter((p) => !ids.includes(p.proposal_id));
  };
  return [
    http.get("*/api/me", () => HttpResponse.json(backend.me)),
    http.get("*/api/proposals", () => etagJson({ items: backend.proposals }, undefined, ETAG)),
    http.get("*/api/proposals/:id", ({ params }) => {
      const p = backend.proposals.find((x) => x.proposal_id === params["id"]);
      return p === undefined ? HttpResponse.json({ code: "E_NOT_FOUND", message: "proposal not found", details: {} }, { status: 404 }) : etagJson(detail(p), undefined, ETAG);
    }),
    http.get("*/api/questions", () => etagJson({ items: backend.questions, health: [], next_cursor: null }, undefined, ETAG)),
    http.get("*/api/findings", () => etagJson({ items: backend.findings, next_cursor: null }, undefined, ETAG)),
    http.get("*/api/gates", () => etagJson(backend.gates, undefined, ETAG)),
    http.get("*/api/pipeline", () => etagJson({ steps: [], next: backend.pipelineNext, open_questions: 0, sources: 1 }, undefined, ETAG)),
    http.get("*/api/requirements/:id", ({ params }) => etagJson(REQUIREMENTS[String(params["id"])] ?? {}, undefined, ETAG)),
    http.get("*/api/model/sentences", () => etagJson({ items: SENTENCES, next_cursor: null }, undefined, ETAG)),
    http.get("*/api/sources", () =>
      etagJson({ items: [{ id: "src:req", hash: "sha256:0", media_type: "text/markdown", source_kind: "markdown", name: "requirements.md", imported_at: ago(60), spans_count: 38 }] }, undefined, ETAG),
    ),
    http.post("*/api/proposals/accept-set", async ({ request }) => {
      const failed = await record(request, "accept-set");
      if (failed !== null) {
        return failed;
      }
      const ids = (backend.writes.at(-1)?.body as { proposal_ids: string[] }).proposal_ids;
      drop(ids);
      return HttpResponse.json({ ...writeResult, accepted: ids, confirmed: [] });
    }),
    http.post("*/api/proposals/:id/:action", async ({ request, params }) => {
      const id = String(params["id"]);
      const action = String(params["action"]);
      const failed = await record(request, action + ":" + id);
      if (failed !== null) {
        return failed;
      }
      if (action === "refresh") {
        const stale = backend.proposals.find((p) => p.proposal_id === id) as PendingProposal;
        backend.proposals = backend.proposals.map((p) => (p.proposal_id === id ? { ...stale, proposal_id: "prop:fresh", stale: false, base_revision: HEAD } : p));
        return HttpResponse.json({ ...writeResult, revision: HEAD, refreshed: id, proposal_id: "prop:fresh", action: "create" });
      }
      drop([id]);
      return HttpResponse.json(writeResult);
    }),
    http.post("*/api/questions/:id/answer", async ({ request, params }) => {
      const id = String(params["id"]);
      const failed = await record(request, "answer:" + id);
      if (failed !== null) {
        return failed;
      }
      backend.proposals = [
        ...backend.proposals,
        proposal("prop:answered", "Apply the answer to: Does the requested duration include the end date?", { origin: "question", origin_ref: id, element_kind: "answer", stage: null, discardable: true, created_at: ago(0) }),
      ];
      return HttpResponse.json({ ...writeResult, revision: HEAD, proposal_id: "prop:answered", action: "create", intake: {}, patch_preview: {} });
    }),
    http.post("*/api/findings/materialize", async ({ request }) => {
      await record(request, "materialize");
      backend.gates = [];
      backend.findings = [...backend.findings, { ...DUPLICATE, id: "fnd:new", code: "PLUMB.F2.EVENT.CONSUMER", family: "F2", message: "Event without consumer" }];
      return HttpResponse.json({ ...writeResult, revision: "rev:43:materialized00000", finding_ids: ["fnd:new"] });
    }),
    http.post("*/api/findings/:id/waive", async ({ request, params }) => {
      const id = String(params["id"]);
      const failed = await record(request, "waive:" + id);
      if (failed !== null) {
        return failed;
      }
      backend.findings = backend.findings.filter((f) => f.id !== id);
      return HttpResponse.json(writeResult);
    }),
    http.post("*/api/requirements/:id/accept-ears", async ({ request, params }) => {
      await record(request, "accept-ears:" + String(params["id"]));
      return HttpResponse.json({ ...writeResult, revision: "rev:43:eeeeeeeeeeeeeeee" });
    }),
    http.post("*/api/stages/:stage/run", async ({ request, params }) => {
      await record(request, "run:" + String(params["stage"]));
      return HttpResponse.json({ ...writeResult, job_id: "job:1" });
    }),
    http.get("*/api/jobs/:id", () => etagJson({ status: "queued", progress: 0, base_revision: HEAD })),
    http.get("*/api/model/version", () => etagJson({ version: HEAD, revision: HEAD, hash: "h", semantic_hash: "h", evidence_hash: "e", created_at: ago(4), created_by: "agent:cli" }, undefined, ETAG)),
  ];
}

function renderAt(path: string, backend: Backend = newBackend()) {
  server.use(...handlers(backend));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router: AnyRouter = app.router.createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { backend, router, user: userEvent.setup() };
}

const queue = () => screen.getByRole("list", { name: /^(All items|Proposals|Questions|Wording|Issues to decide|From steps|From questions|From the Assistant)$/ });
const rows = () => within(queue()).getAllByRole("button");
const inspector = () => screen.getByRole("complementary", { name: "Item details" });
const filters = () => screen.getByRole("navigation", { name: "Inbox filters" });
const rowFor = (title: string) => within(queue()).getByRole("button", { name: new RegExp(title) });

/** The toast currently shown. */
async function shownToast(): Promise<HTMLElement> {
  return waitFor(() => {
    const element = document.querySelector<HTMLElement>("[data-toast]");
    expect(element).not.toBeNull();
    return element as HTMLElement;
  });
}

async function ready() {
  await screen.findByRole("heading", { level: 1, name: "Inbox" });
  await waitFor(() => expect(rows().length).toBeGreaterThan(0));
}

// ------------------------------------------------------------------ layout

describe("Inbox layout", () => {
  it("shows smart filters with counts, the queue and the first item in the inspector; the badge equals what is listed", async () => {
    renderAt("/inbox");
    await ready();
    const nav = filters();
    const counts = within(nav)
      .getAllByRole("link")
      .map((a) => [a.getAttribute("data-filter"), a.querySelector("[data-count]")?.textContent]);
    expect(counts).toEqual([
      ["all", "10"],
      ["proposals", "6"],
      ["questions", "2"],
      ["wording", "1"],
      ["issues", "1"],
      ["steps", "9"],
      ["fromq", "1"],
      ["assistant", "0"],
    ]);
    expect(within(nav).getByRole("link", { name: /^All/ }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByText("By origin")).toBeTruthy();
    expect(screen.getByText("Keyboard")).toBeTruthy();
    expect(screen.getByRole("heading", { level: 2, name: "All items" })).toBeTruthy();
    expect(screen.getByText("10 waiting")).toBeTruthy();
    expect(rows().map((r) => r.querySelector("[data-row-title]")?.textContent)).toEqual([
      "Add operation Submit leave request",
      "Add operation Approve leave request",
      "Add operation Withdraw leave request",
      "Add event Leave request submitted",
      "Link Cancel leave request to HR-019",
      "Does the requested duration include the end date?",
      "Can a manager approve their own leave request?",
      "Change requirement HR-030",
      "Apply the answer to: Are public holidays counted as working days?",
      "HR-023 and HR-024 look like duplicates",
    ]);
    const first = rowFor("Submit leave request");
    expect(first.getAttribute("aria-current")).toBe("true");
    expect(first.textContent).toContain("New operation · Flows step");
    expect(first.textContent).toContain("4 min");
    const link = rowFor("Link Cancel");
    expect(link.textContent).toContain("Stale");
    expect(link.textContent).toContain("1 h");
    expect(rowFor("Does the requested").textContent).toContain("Question · Questions step");
    expect(rowFor("look like duplicates").textContent).toContain("Issue to decide · Quality check");
    // The sidebar badge counts exactly the listed items.
    const sidebar = screen.getByRole("navigation", { name: "Main navigation" });
    await waitFor(() => expect(within(sidebar).getByRole("link", { name: /Inbox/ }).querySelector("[data-badge]")?.textContent).toBe("10"));
    expect(screen.getByRole("button", { name: /Accept all from Flows/ })).toBeTruthy();
  });

  it("renders a new element with its facts, evidence highlight, model change and actions, axe-clean", async () => {
    renderAt("/inbox");
    await ready();
    const panel = inspector();
    expect(await within(panel).findByText("New operation · Flows step")).toBeTruthy();
    expect(within(panel).getByRole("heading", { level: 2, name: "Add operation Submit leave request" })).toBeTruthy();
    const facts = await within(panel).findByRole("list", { name: "Facts" });
    expect(within(facts).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "Performed byEmployee",
      "ReadsLeave request (Draft)",
      "WritesLeave request → Submitted",
      "ProducesLeave request submitted",
    ]);
    const quote = within(panel).getByRole("figure", { name: "Evidence from requirements.md" });
    expect(quote.querySelector("mark")?.textContent).toBe("Zażółć: an employee shall be able to submit a Draft leave request to their manager.");
    expect(quote.textContent).toContain("HR-008");
    expect(quote.textContent).toContain("Open in source");
    expect(within(panel).getAllByRole("figure")).toHaveLength(1);
    const diff = within(panel).getByRole("list", { name: "What changes in the model" });
    expect(within(diff).getAllByRole("listitem").map((li) => li.getAttribute("data-diff-op") + " " + li.textContent?.replace(/^[+−]?\s*/, ""))).toEqual([
      "context entity Leave request (changed)",
      "add Addedoperation Submit leave request",
      "add AddedSubmit leave request performed by Employee",
    ]);
    const bar = within(panel).getByRole("group", { name: "Actions" });
    expect(within(bar).getAllByRole("button").map((b) => b.getAttribute("aria-label") ?? b.textContent)).toEqual(["Accept", "Reject"]);
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("opens a deep-linked item and filters the queue from the URL", async () => {
    const { user, router } = renderAt("/inbox/q:self?filter=questions");
    await ready();
    expect(rows()).toHaveLength(2);
    expect(screen.getByRole("heading", { level: 2, name: "Questions" })).toBeTruthy();
    expect(rowFor("Can a manager").getAttribute("aria-current")).toBe("true");
    await user.click(within(filters()).getByRole("link", { name: /^Issues to decide/ }));
    await waitFor(() => expect(router.state.location.search).toEqual({ filter: "issues" }));
    expect(rows()).toHaveLength(1);
    await user.click(within(filters()).getByRole("link", { name: /^From the Assistant/ }));
    expect(await screen.findByText("Nothing is waiting in this view.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Show all items" }));
    await waitFor(() => expect(router.state.location.search).toEqual({}));
  });

  it("shows skeletons while loading and a read failure with Retry", async () => {
    const backend = newBackend();
    let fail = true;
    renderAt("/inbox", backend);
    server.use(
      http.get("*/api/proposals", () =>
        fail ? HttpResponse.json({ code: "E_A03_RUNTIME_UNAVAILABLE", message: "not configured", details: {} }, { status: 503 }) : etagJson({ items: backend.proposals }, undefined, ETAG),
      ),
    );
    expect(await screen.findByTestId("inbox-skeleton")).toBeTruthy();
    const alert = await screen.findByTestId("read-failure");
    expect(alert.textContent).toContain("E_A03_RUNTIME_UNAVAILABLE");
    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await ready();
  });

  it("is all caught up when nothing waits, offering the next pipeline step", async () => {
    const { user, backend } = renderAt("/inbox", newBackend({ proposals: [], questions: [], findings: [] }));
    expect(await screen.findByRole("heading", { name: "You're all caught up" })).toBeTruthy();
    expect(screen.getByText("Nothing is waiting for your decision. The next step is Directory.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Run Directory" }));
    await waitFor(() => expect(backend.writes.map((w) => w.path)).toEqual(["run:directory"]));
    expect(backend.writes[0]?.ifMatch).toBe(ETAG);
  });

  it("is all caught up with nothing left to run, pointing to Quality", async () => {
    const { user, router } = renderAt("/inbox", newBackend({ proposals: [], questions: [], findings: [], pipelineNext: null }));
    expect(await screen.findByText("Nothing is waiting for your decision.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Open Quality" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/quality"));
  });

  it("never shows decision controls to a role that cannot decide", async () => {
    renderAt("/inbox", newBackend({ me: { ...ANALYST, roles: ["viewer"] } }));
    await ready();
    await within(inspector()).findByRole("list", { name: "Facts" });
    expect(within(inspector()).queryByRole("group", { name: "Actions" })).toBeNull();
    expect(screen.queryByRole("button", { name: /Accept all from/ })).toBeNull();
  });
});

// ------------------------------------------------------------------ accepting and rejecting

describe("Accept and reject", () => {
  it("accepts with accept-set and the reviewed ETag, removes the row, focuses the next one and offers View in Model", async () => {
    const { user, backend, router } = renderAt("/inbox");
    await ready();
    await within(inspector()).findByRole("list", { name: "Facts" });
    await user.click(within(inspector()).getByRole("button", { name: "Accept" }));
    await waitFor(() => expect(backend.writes).toHaveLength(1));
    expect(backend.writes[0]).toEqual({ path: "accept-set", ifMatch: ETAG, body: { proposal_ids: ["prop:submit"], confirm: true, reason: null } });
    await waitFor(() => expect(within(queue()).queryByRole("button", { name: /Submit leave request/ })).toBeNull());
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox/prop:approve"));
    await waitFor(() => expect(document.activeElement).toBe(rowFor("Approve leave request")));
    const toast = await shownToast();
    expect(toast.textContent).toContain("Operation added to the model");
    await user.click(within(toast).getByRole("button", { name: "View in Model" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/model/processes"));
  });

  it("asks for the reason an accept requires", async () => {
    const { user, backend } = renderAt("/inbox/prop:withdraw");
    await ready();
    await user.click(await within(inspector()).findByRole("button", { name: "Accept" }));
    const sheet = await screen.findByRole("dialog", { name: "Accept proposal" });
    const confirm = within(sheet).getByRole("button", { name: /^Accept/ });
    expect((confirm as HTMLButtonElement).disabled).toBe(true);
    await user.type(within(sheet).getByRole("textbox"), "Matches HR-018.");
    await user.click(confirm);
    await waitFor(() => expect(backend.writes[0]?.body).toEqual({ proposal_ids: ["prop:withdraw"], confirm: true, reason: "Matches HR-018." }));
  });

  it("rejects through the destructive reason sheet, required only when the proposal requires it", async () => {
    const { user, backend } = renderAt("/inbox/prop:approve");
    await ready();
    await user.click(await within(inspector()).findByRole("button", { name: "Reject" }));
    const sheet = await screen.findByRole("dialog", { name: "Reject proposal" });
    expect(within(sheet).getByText(/“Add operation Approve leave request” will not be added/)).toBeTruthy();
    const confirm = within(sheet).getByRole("button", { name: /^Reject/ });
    expect(confirm.className).toContain("bg-blocked");
    expect((confirm as HTMLButtonElement).disabled).toBe(false);
    await user.click(confirm);
    await waitFor(() => expect(backend.writes[0]).toEqual({ path: "reject:prop:approve", ifMatch: ETAG, body: { reason: null } }));
    expect((await shownToast()).textContent).toContain("Proposal rejected");
  });

  it("drives the triage from the keyboard: J/K move, A accepts, R rejects, ⌘↩ confirms, typing is ignored", async () => {
    const { user, backend, router } = renderAt("/inbox");
    await ready();
    await user.keyboard("j");
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox/prop:approve"));
    await user.keyboard("j");
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox/prop:withdraw"));
    await user.keyboard("k");
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox/prop:approve"));
    await within(inspector()).findByRole("list", { name: "Facts" });
    await user.keyboard("a");
    await waitFor(() => expect(backend.writes.map((w) => w.path)).toEqual(["accept-set"]));
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox/prop:withdraw"));
    await user.keyboard("r");
    const sheet = await screen.findByRole("dialog", { name: "Reject proposal" });
    const reason = within(sheet).getByRole("textbox");
    await user.type(reason, "jk ar");
    expect(router.state.location.pathname).toBe("/inbox/prop:withdraw");
    await user.keyboard("{Meta>}{Enter}{/Meta}");
    await waitFor(() => expect(backend.writes.at(-1)).toEqual({ path: "reject:prop:withdraw", ifMatch: ETAG, body: { reason: "jk ar" } }));
  });

  it("keeps the selection when a write is refused and explains why", async () => {
    const backend = newBackend({ failNext: 409 });
    const { user } = renderAt("/inbox", backend);
    await ready();
    await user.click(await within(inspector()).findByRole("button", { name: "Accept" }));
    expect((await shownToast()).textContent).toContain("The proposal is out of date.");
    expect(rowFor("Submit leave request").getAttribute("aria-current")).toBe("true");
  });

  it("opens the conflict sheet when the model changed since the user looked (412)", async () => {
    const { user } = renderAt("/inbox", newBackend({ failNext: 412 }));
    await ready();
    await user.click(await within(inspector()).findByRole("button", { name: "Accept" }));
    await waitFor(() => expect(app.conflict.useConflictStore.getState().conflict).not.toBeNull());
    expect(await screen.findByRole("dialog", { name: /changed since you looked/i })).toBeTruthy();
  });
});

// ------------------------------------------------------------------ multi-select and bulk

describe("Multi-select and bulk acceptance", () => {
  it("selects several rows with ⇧/⌘ click and accepts them together as one change", async () => {
    const { user, backend } = renderAt("/inbox");
    await ready();
    await user.keyboard("{Shift>}");
    await user.click(rowFor("Approve leave request"));
    await user.keyboard("{/Shift}");
    await user.keyboard("{Meta>}");
    await user.click(rowFor("Leave request submitted"));
    await user.click(rowFor("Does the requested"));
    await user.keyboard("{/Meta}");
    const panel = inspector();
    expect(within(panel).getByText("4")).toBeTruthy();
    expect(within(panel).getByText("items selected")).toBeTruthy();
    expect(within(panel).getByText("Accepted together as one change. If any item is out of date, none are applied.")).toBeTruthy();
    expect(within(panel).getByText("1 selected item can't be accepted this way (a question, an issue or an out-of-date item).")).toBeTruthy();
    await user.click(within(panel).getByRole("button", { name: "Accept 3" }));
    await waitFor(() =>
      expect(backend.writes[0]).toEqual({ path: "accept-set", ifMatch: ETAG, body: { proposal_ids: ["prop:submit", "prop:approve", "prop:event"], confirm: true, reason: null } }),
    );
    expect((await shownToast()).textContent).toContain("3 items accepted together");
  });

  it("clears the selection", async () => {
    const { user } = renderAt("/inbox");
    await ready();
    await user.keyboard("{Shift>}");
    await user.click(rowFor("Approve leave request"));
    await user.keyboard("{/Shift}");
    await user.click(within(inspector()).getByRole("button", { name: "Clear" }));
    expect(within(inspector()).queryByText("items selected")).toBeNull();
    expect(await within(inspector()).findByRole("heading", { level: 2, name: "Add operation Submit leave request" })).toBeTruthy();
  });

  it("accepts all current proposals from one step, asking once for a required reason", async () => {
    const { user, backend } = renderAt("/inbox");
    await ready();
    await user.click(screen.getByRole("button", { name: "Accept all from Flows" }));
    const sheet = await screen.findByRole("dialog", { name: "Accept 4 proposals from Flows" });
    await user.type(within(sheet).getByRole("textbox"), "Reviewed with HR.");
    await user.click(within(sheet).getByRole("button", { name: /^Accept 4/ }));
    await waitFor(() =>
      expect(backend.writes[0]?.body).toEqual({ proposal_ids: ["prop:submit", "prop:approve", "prop:withdraw", "prop:event"], confirm: true, reason: "Reviewed with HR." }),
    );
    expect((await shownToast()).textContent).toContain("4 proposals accepted · 1 needs a refresh");
  });
});

// ------------------------------------------------------------------ item types

describe("Item cards", () => {
  it("shows a stale link side by side with the refresh banner; Accept is disabled and Refresh re-runs its step", async () => {
    const { user, backend } = renderAt("/inbox/prop:link");
    await ready();
    const panel = inspector();
    expect(await within(panel).findByText("Proposed on an older version (v40). Refresh to check it against the current model.")).toBeTruthy();
    const accept = within(panel).getByRole("button", { name: "Accept" }) as HTMLButtonElement;
    expect(accept.disabled).toBe(true);
    const sides = await within(panel).findAllByTestId("link-side");
    expect(sides.map((s) => s.textContent)).toEqual(["FromCancel leave request", "ToHR-019"]);
    expect(within(panel).getByText("Relation: implements")).toBeTruthy();
    await user.keyboard("a");
    expect(backend.writes).toEqual([]);
    await user.click(within(panel).getByRole("button", { name: "Refresh proposal" }));
    await waitFor(() => expect(backend.writes.map((w) => w.path)).toEqual(["run:flows"]));
    expect((await shownToast()).textContent).toContain("Running the Flows step again");
  });

  it("refreshes a stale answer through the refresh endpoint and selects the fresh proposal", async () => {
    const backend = newBackend({ proposals: [{ ...ANSWER, stale: true, base_revision: "rev:41:ffffffffffffffff" }] });
    const { user, router } = renderAt("/inbox/prop:answer", backend);
    await ready();
    await user.click(await within(inspector()).findByRole("button", { name: "Refresh proposal" }));
    await waitFor(() => expect(backend.writes[0]).toEqual({ path: "refresh:prop:answer", ifMatch: ETAG, body: null }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox/prop:fresh"));
    expect((await shownToast()).textContent).toContain("Proposal refreshed against the current model");
  });

  it("answers a question with Yes and a rationale; the item becomes an answer to confirm", async () => {
    const { user, backend, router } = renderAt("/inbox/q:duration");
    await ready();
    const panel = inspector();
    expect(within(panel).getByText("Question · Questions step")).toBeTruthy();
    expect(within(panel).getByText("calculation:days")).toBeTruthy();
    expect(within(panel).getByText("Routed to stakeholder:hr")).toBeTruthy();
    const answer = within(panel).getByRole("button", { name: "Answer" }) as HTMLButtonElement;
    expect(answer.disabled).toBe(true);
    await user.click(within(panel).getByRole("radio", { name: "Yes" }));
    await user.type(within(panel).getByRole("textbox", { name: /Rationale/ }), "Both dates count in our policy.");
    expect(answer.disabled).toBe(false);
    await user.click(answer);
    await waitFor(() =>
      expect(backend.writes[0]).toEqual({ path: "answer:q:duration", ifMatch: ETAG, body: { answer: { inclusive_end: true }, rationale: "Both dates count in our policy.", supersedes: null } }),
    );
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox/prop:answered"));
    expect((await shownToast()).textContent).toContain("Answer recorded · it now needs your confirmation");
    await waitFor(() => expect(within(queue()).queryByRole("button", { name: /Does the requested duration include the end date\?$/ })).toBeNull());
    expect(rowFor("Apply the answer to: Does the requested").textContent).toContain("Answer to confirm · From a question");
  });

  it("answering in the Questions view opens the answer under From questions, in the inspector", async () => {
    const { user, router } = renderAt("/inbox/q:duration?filter=questions");
    await ready();
    await user.click(within(inspector()).getByRole("radio", { name: "Yes" }));
    await user.click(within(inspector()).getByRole("button", { name: "Answer" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox/prop:answered"));
    // The Questions view lists no answers: the answer opens where it is listed.
    expect(router.state.location.search).toEqual({ filter: "fromq" });
    expect(await within(inspector()).findByRole("heading", { level: 2, name: "Apply the answer to: Does the requested duration include the end date?" })).toBeTruthy();
    expect(within(inspector()).getByRole("button", { name: "Accept" })).toBeTruthy();
  });

  it("skips a question for now without a write", async () => {
    const { user, backend, router } = renderAt("/inbox/q:duration");
    await ready();
    await user.click(within(inspector()).getByRole("button", { name: "Skip for now" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox/q:self"));
    expect(backend.writes).toEqual([]);
  });

  it("confirms an answer with its model change, or discards it with a required reason", async () => {
    const { user, backend } = renderAt("/inbox/prop:answer");
    await ready();
    const panel = inspector();
    const diff = await within(panel).findByRole("list", { name: "Model change" });
    expect(within(diff).getAllByRole("listitem").map((li) => li.getAttribute("data-diff-op"))).toEqual(["remove", "context", "add"]);
    expect(within(panel).getByText("counted: false")).toBeTruthy();
    await user.click(within(panel).getByRole("button", { name: "Discard" }));
    const sheet = await screen.findByRole("dialog", { name: "Discard this answer" });
    const confirm = within(sheet).getByRole("button", { name: /^Discard/ }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
    await user.type(within(sheet).getByRole("textbox"), "HR will re-answer.");
    await user.click(confirm);
    await waitFor(() => expect(backend.writes[0]).toEqual({ path: "discard:prop:answer", ifMatch: ETAG, body: { reason: "HR will re-answer." } }));
    expect((await shownToast()).textContent).toContain("Answer discarded");
  });

  it("shows a wording suggestion with struck and added words and its slots, and accepts it", async () => {
    const { user, backend } = renderAt("/inbox/prop:ears");
    await ready();
    const panel = inspector();
    const original = await within(panel).findByTestId("wording-original");
    expect([...original.querySelectorAll("del")].map((d) => d.textContent?.replace(/^Removed /, ""))).toEqual(["The leave-request submission operation should"]);
    const suggested = within(panel).getByTestId("wording-suggested");
    expect([...suggested.querySelectorAll("ins")].map((d) => d.textContent?.replace(/^Added /, ""))).toEqual(["When an employee submits a leave request, the system shall", "the submission"]);
    expect(original.querySelector("del")?.className).toContain("line-through");
    const slots = within(panel).getAllByTestId("slot");
    expect(slots.map((s) => s.textContent)).toEqual(["ClassQuality", "LevelSystem", "ModalityShall", "PatternEvent-driven (When …)"]);
    await user.click(within(panel).getByRole("button", { name: "Accept" }));
    await waitFor(() => expect(backend.writes[0]?.body).toEqual({ proposal_ids: ["prop:ears"], confirm: true, reason: null }));
    expect((await shownToast()).textContent).toContain("Wording accepted");
  });

  it("edits the suggested wording (E), accepts the edited text and retires the suggestion", async () => {
    const { user, backend } = renderAt("/inbox/prop:ears");
    await ready();
    await within(inspector()).findByTestId("wording-suggested");
    await user.keyboard("e");
    const field = within(inspector()).getByRole("textbox", { name: "Suggested wording" });
    await user.clear(field);
    await user.type(field, "When an employee submits a leave request, the system shall respond within 2 seconds.");
    await user.click(within(inspector()).getByRole("button", { name: "Save and accept" }));
    await waitFor(() => expect(backend.writes.map((w) => w.path)).toEqual(["accept-ears:req:hr030", "reject:prop:ears"]));
    expect(backend.writes[0]).toMatchObject({ ifMatch: ETAG, body: { text: "When an employee submits a leave request, the system shall respond within 2 seconds." } });
    expect(backend.writes[1]).toEqual({ path: "reject:prop:ears", ifMatch: '"rev:43:eeeeeeeeeeeeeeee"', body: { reason: "Replaced by an edited wording." } });
  });

  it("shows an issue to decide with the affected requirements side by side; Fix deep-links and Waive needs a reason", async () => {
    const { user, backend } = renderAt("/inbox/fnd:dup");
    await ready();
    const panel = inspector();
    expect(within(panel).getByText(/Blocker/)).toBeTruthy();
    expect(within(panel).getByText("PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED")).toBeTruthy();
    const sides = await within(panel).findAllByTestId("affected-requirement");
    await waitFor(() => expect(sides.map((s) => s.textContent)).toEqual([
      "HR-023The system shall notify the employee when their leave request is approved.",
      "HR-024The system shall notify the employee when their leave request is rejected.",
    ]));
    await user.click(within(panel).getByRole("button", { name: "Waive with reason" }));
    const sheet = await screen.findByRole("dialog", { name: "Waive this finding" });
    const confirm = within(sheet).getByRole("button", { name: /^Waive/ }) as HTMLButtonElement;
    expect(confirm.className).not.toContain("bg-blocked");
    expect(confirm.disabled).toBe(true);
    await user.type(within(sheet).getByRole("textbox"), "They notify different outcomes.");
    await user.click(confirm);
    await waitFor(() => expect(backend.writes[0]).toEqual({ path: "waive:fnd:dup", ifMatch: ETAG, body: { reason: "They notify different outcomes." } }));
    expect((await shownToast()).textContent).toContain("Finding waived");
    cleanup();
    const again = renderAt("/inbox/fnd:dup");
    await ready();
    await again.user.click(within(inspector()).getByRole("button", { name: "Fix" }));
    await waitFor(() => expect(again.router.state.location.pathname).toBe("/requirements/req:hr023"));
  });

  it("materializes a failing blocker that has no finding yet before waiving it", async () => {
    const gates: GateStatus[] = [
      {
        gate: "F2",
        state: "current",
        passed: false,
        reason: null,
        findings: [],
        waivers: [],
        summary: { blocker_failed: 1, blocker_waived: 0, warnings: 0 },
        rules: [
          {
            rule_id: "PLUMB.F2.EVENT.CONSUMER",
            title: null,
            state: "FAIL",
            declared_severity: "blocker",
            effective_severity: "blocker",
            applicability: { state: "APPLICABLE", reason: null },
            targets: ["event:approved"],
            evidence: [],
            semantic_condition_key: null,
            finding_ref: null,
            waiver_ref: null,
            error: null,
          },
        ],
      },
    ];
    const backend = newBackend({ proposals: [], questions: [], findings: [], gates });
    const { user } = renderAt("/inbox", backend);
    await ready();
    expect(rowFor("Blocker PLUMB.F2.EVENT.CONSUMER needs a decision")).toBeTruthy();
    expect(within(inspector()).getByText("event:approved")).toBeTruthy();
    await user.click(within(inspector()).getByRole("button", { name: "Waive with reason" }));
    const sheet = await screen.findByRole("dialog", { name: "Waive this finding" });
    await user.type(within(sheet).getByRole("textbox"), "Notification is out of scope.");
    await user.click(within(sheet).getByRole("button", { name: /^Waive/ }));
    await waitFor(() => expect(backend.writes.map((w) => w.path)).toEqual(["materialize", "waive:fnd:new"]));
    expect(backend.writes[0]?.ifMatch).toBe(ETAG);
    expect(backend.writes[1]?.ifMatch).toBe('"rev:43:materialized00000"');
    expect(await screen.findByRole("button", { name: "Run Directory" })).toBeTruthy();
  });

  it("toggles the evidence with Space and clears the selection with ⌘.", async () => {
    const { user } = renderAt("/inbox");
    await ready();
    const panel = inspector();
    await within(panel).findByRole("figure", { name: "Evidence from requirements.md" });
    expect(within(panel).getByText("Space to toggle")).toBeTruthy();
    await user.keyboard(" ");
    expect(within(panel).queryByRole("figure")).toBeNull();
    await user.keyboard(" ");
    expect(within(panel).getByRole("figure", { name: "Evidence from requirements.md" })).toBeTruthy();
    await user.keyboard("{Meta>}.{/Meta}");
    expect(await within(inspector()).findByText("Select an item")).toBeTruthy();
  });
});
