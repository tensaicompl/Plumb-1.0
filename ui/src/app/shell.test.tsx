// The application shell (design handoff "Global shell", "Overlays", "Interactions"): sidebar,
// toolbar (project, version indicator, search, Next action, Run step, Assistant), connection
// banner, command palette, settings, export, global keys and the spec §3 routes. Every HTTP call is
// answered by MSW and the WebSocket is an in-process fake.
//
// The generated client captures `fetch` and `Request` when its module is first evaluated, so the
// app modules are imported dynamically after MSW is listening, with relative URLs resolved
// against the jsdom origin.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider, type AnyRouter } from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import type { components } from "../api/schema";
import { axeViolations } from "../test/axe";
import { etagJson, server } from "../test/msw";

type Me = components["schemas"]["MeResponse"];
type GateStatus = components["schemas"]["GateStatus"];
type PendingProposal = components["schemas"]["PendingProposal"];
type Question = components["schemas"]["Question"];
type Finding = components["schemas"]["Finding"];
type JobResponse = components["schemas"]["JobResponse"];

interface AppModules {
  router: typeof import("./router");
  nav: typeof import("./nav");
  preferences: typeof import("./preferences");
  run: typeof import("./run");
  i18n: typeof import("../i18n");
  events: typeof import("../api/useEvents");
  toast: typeof import("../components/ui/Toast");
}

let app: AppModules;

class FakeWebSocket extends EventTarget {
  static instances: FakeWebSocket[] = [];
  readonly url: string;
  closed = false;
  constructor(url: string | URL) {
    super();
    this.url = String(url);
    FakeWebSocket.instances.push(this);
  }
  close(): void {
    this.closed = true;
  }
  open(): void {
    this.dispatchEvent(new Event("open"));
  }
  drop(code: number): void {
    this.dispatchEvent(new CloseEvent("close", { code }));
  }
}

class OriginRequest extends Request {
  constructor(input: RequestInfo | URL, init?: RequestInit) {
    super(typeof input === "string" ? new URL(input, window.location.href) : input, init);
  }
}

let compact = false;
let prefersDark = false;
function matchMedia(query: string): MediaQueryList {
  const matches = query.includes("max-width") ? compact : query.includes("prefers-color-scheme: dark") ? prefersDark : false;
  return {
    matches,
    media: query,
    onchange: null,
    addListener: () => undefined,
    removeListener: () => undefined,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    dispatchEvent: () => false,
  } as MediaQueryList;
}

beforeAll(async () => {
  server.listen({ onUnhandledRequest: "error" });
  vi.stubGlobal("Request", OriginRequest);
  vi.stubGlobal("WebSocket", FakeWebSocket);
  vi.stubGlobal("scrollTo", () => undefined);
  vi.stubGlobal("matchMedia", matchMedia);
  app = {
    router: await import("./router"),
    nav: await import("./nav"),
    preferences: await import("./preferences"),
    run: await import("./run"),
    i18n: await import("../i18n"),
    events: await import("../api/useEvents"),
    toast: await import("../components/ui/Toast"),
  };
});

afterAll(() => {
  server.close();
  vi.unstubAllGlobals();
});

beforeEach(() => {
  FakeWebSocket.instances = [];
  compact = false;
  prefersDark = false;
  window.localStorage.clear();
  app.i18n.useLocaleStore.getState().setLocale("en");
  app.preferences.useAppearanceStore.setState({ appearance: "system" });
  app.preferences.applyAppearance("system");
  app.preferences.useShellStore.setState({ sidebarCollapsed: false, assistantOpen: false, overlay: null });
  app.run.useRunStore.getState().clear();
});

afterEach(() => {
  cleanup();
  server.resetHandlers();
  server.events.removeAllListeners();
  act(() => app.toast.dismissToast());
});

// ------------------------------------------------------------------ fixtures

const VERSION_ETAG = '"rev:42:aaaaaaaaaaaaaaaa"';
const ME: Me = {
  user: "Marta Nowak",
  mode: "pilot",
  roles: ["analyst", "reviewer"],
  inference_available: true,
  inference_model: "claude-sonnet-4-5",
  project: { id: "project:hr-leave", name: "HR leave", database: "/srv/plumb/hr-leave/plumb.sqlite" },
};
type PipelineResponse = components["schemas"]["PipelineResponse"];
type PipelineStepWire = components["schemas"]["PipelineStep"];
function pipelineStep(recipe: PipelineStepWire["recipe"], status: PipelineStepWire["status"], extra: Partial<PipelineStepWire> = {}): PipelineStepWire {
  return { recipe, status, job_id: null, progress: null, pending_proposals: 0, error: null, updated_at: null, missing_config: [], ...extra };
}
/** No recipe has run, the next step is unknown (the pre-pipeline behaviour). */
const NO_PIPELINE: PipelineResponse = { steps: [], sources: 1, open_questions: 0, next: null };
const VERSION = {
  version: "rev:42:aaaaaaaaaaaaaaaa",
  revision: "rev:42:aaaaaaaaaaaaaaaa",
  hash: "psg:sha256:current",
  semantic_hash: "psg:sha256:current",
  evidence_hash: "ev:sha256:evidence",
};

function proposal(n: number, stale = false): PendingProposal {
  return { proposal_id: "prop:" + n, action: "create", origin: "stage", base_revision: VERSION.revision, discardable: false, requires_reason: false, stale, title: "Proposal " + n, element_kind: "other", created_at: "2026-05-01T12:00:00.000000000Z", stage: null };
}

function question(n: number): Question {
  return { id: "q:" + n, text: "Question " + n, kind: "yes_no", status: "Open", priority: null, round_ref: null, stakeholder_ref: null, context_refs: [], answer_schema: null } as unknown as Question;
}

const FINDINGS: Finding[] = [
  { id: "fnd:1", code: "F1.dup", family: "F1", message: "Duplicate", severity: "blocker", status: "Open", affected_refs: [], rule_class: null },
  { id: "fnd:2", code: "F1.weak", family: "F1", message: "Weak", severity: "warning", status: "Open", affected_refs: [], rule_class: null },
  { id: "fnd:3", code: "F1.dup2", family: "F1", message: "Waived", severity: "blocker", status: "Waived", affected_refs: [], rule_class: null },
];

function gate(name: GateStatus["gate"], passed: boolean, state: GateStatus["state"] = "current"): GateStatus {
  return { gate: name, state, passed, reason: null, findings: [], rules: [], waivers: [], summary: { blocker_failed: passed ? 0 : 1, blocker_waived: 0, warnings: 0 } };
}

const ALL_PASS: GateStatus[] = (["I0", "F1", "F2", "F3", "F4"] as const).map((g) => gate(g, true));

interface Backend {
  proposals: PendingProposal[];
  questions: Question[];
  gates: GateStatus[];
  me: Me;
  jobs: JobResponse[];
  runs: { stage: string; ifMatch: string | null }[];
  retries: { job: string; ifMatch: string | null }[];
  pipeline: PipelineResponse;
  searches: string[];
  exports: string[];
  versionReads: number;
}

function newBackend(overrides: Partial<Backend> = {}): Backend {
  return {
    proposals: [1, 2, 3, 4, 5, 6].map((n) => proposal(n)).concat(proposal(7, true)),
    questions: [question(1), question(2)],
    gates: ALL_PASS.map((g) => (g.gate === "F3" ? gate("F3", false) : g)),
    me: ME,
    jobs: [],
    runs: [],
    retries: [],
    pipeline: NO_PIPELINE,
    searches: [],
    exports: [],
    versionReads: 0,
    ...overrides,
  };
}

function handlers(backend: Backend) {
  return [
    http.get("*/api/me", () => HttpResponse.json(backend.me)),
    http.get("*/api/model/version", () => {
      backend.versionReads += 1;
      return etagJson(VERSION, undefined, VERSION_ETAG);
    }),
    http.get("*/api/proposals", () => etagJson({ items: backend.proposals }, undefined, VERSION_ETAG)),
    http.get("*/api/questions", ({ request }) => {
      const status = new URL(request.url).searchParams.get("status");
      return etagJson({ items: backend.questions.filter((q) => status === null || q.status === status), health: [] }, undefined, VERSION_ETAG);
    }),
    http.get("*/api/findings", () => etagJson({ items: FINDINGS, next_cursor: null })),
    http.get("*/api/gates", () => etagJson(backend.gates)),
    http.get("*/api/search", ({ request }) => {
      const q = new URL(request.url).searchParams.get("q") ?? "";
      backend.searches.push(q);
      const items = q.includes("leave")
        ? [
            { id: "req:leave-1", kind: "requirement", path: "/api/requirements/req:leave-1", score: 0.9, snippet: "An employee shall be able to submit a leave request." },
            { id: "term:leave", kind: "term", path: null, score: 0.5, snippet: "leave request" },
            { id: "x:1", kind: "mystery", path: null, score: 0.1, snippet: "unknown kind" },
          ]
        : [];
      return etagJson({ items, next_cursor: null });
    }),
    http.post("*/api/stages/:stage/run", ({ request, params }) => {
      backend.runs.push({ stage: String(params["stage"]), ifMatch: request.headers.get("If-Match") });
      return etagJson({ job_id: "job:" + backend.runs.length, events: [], revision: VERSION.revision, semantic_hash: VERSION.semantic_hash, version: VERSION.version });
    }),
    http.get("*/api/pipeline", () => etagJson(backend.pipeline)),
    http.post("*/api/jobs/:id/retry", ({ request, params }) => {
      backend.retries.push({ job: String(params["id"]), ifMatch: request.headers.get("If-Match") });
      return etagJson({ job_id: "job:retry-" + backend.retries.length, events: [], revision: VERSION.revision, semantic_hash: VERSION.semantic_hash, version: VERSION.version });
    }),
    http.get("*/api/jobs/:id", () => {
      const next = backend.jobs.length > 1 ? backend.jobs.shift() : backend.jobs[0];
      return etagJson(next ?? { status: "queued", progress: 0, base_revision: VERSION.revision });
    }),
    http.get("*/api/model/export", ({ request }) => {
      backend.exports.push(new URL(request.url).searchParams.get("format") ?? "");
      return new HttpResponse("content", { headers: { "Content-Type": "application/octet-stream", ETag: VERSION_ETAG } });
    }),
    // Reads of the old pages rendered inside the new routes.
    http.get("*/api/readiness", () =>
      HttpResponse.json({ overall: null, contributing: [], sub: { clarity: null, completeness: null, confirmation: null, consistency: null, verifiability: null } }),
    ),
  ];
}

function renderAt(path: string, backend: Backend = newBackend()) {
  server.use(...handlers(backend));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router: AnyRouter = app.router.createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  const view = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { backend, queryClient, router, view, user: userEvent.setup() };
}

const sidebarNav = () => screen.getByRole("navigation", { name: "Main navigation" });
const toolbar = () => screen.getByRole("banner");

// ------------------------------------------------------------------ sidebar

describe("Sidebar", () => {
  it("lists the seven destinations in order with the active one in midnight, the Inbox badge and the identity footer", async () => {
    renderAt("/overview");
    const nav = await screen.findByRole("navigation", { name: "Main navigation" });
    const links = within(nav).getAllByRole("link");
    expect(links.map((a) => a.getAttribute("data-nav-id"))).toEqual(["overview", "inbox", "requirements", "model", "scenarios", "quality", "sources"]);
    expect(links.map((a) => a.getAttribute("href"))).toEqual(["/overview", "/inbox", "/requirements", "/model/glossary", "/scenarios", "/quality", "/sources"]);
    const overview = within(nav).getByRole("link", { name: "Overview" });
    expect(overview.getAttribute("aria-current")).toBe("page");
    expect(overview.getAttribute("class")).toContain("bg-primary");
    expect(within(nav).getByRole("link", { name: /Requirements/ }).getAttribute("aria-current")).toBeNull();
    // 7 pending proposals + 2 open questions + 1 open blocker finding.
    const inbox = within(nav).getByRole("link", { name: /Inbox/ });
    await waitFor(() => expect(inbox.querySelector("[data-badge]")?.textContent).toBe("10"));
    expect(inbox.textContent).toContain("10 items waiting");
    const sidebar = nav.closest("aside") as HTMLElement;
    expect(within(sidebar).getByRole("button", { name: "Assistant" })).toBeTruthy();
    expect(within(sidebar).getByRole("button", { name: "Settings" })).toBeTruthy();
    expect(await within(sidebar).findByText("Marta Nowak")).toBeTruthy();
    expect(within(sidebar).getByText("Analyst · Reviewer")).toBeTruthy();
    expect(within(sidebar).getByText("MN")).toBeTruthy();
    expect(sidebar.style.width).toBe("232px");
  });

  it("caps the badge at 99+, hides it at zero, and marks the destination of a nested route", async () => {
    const backend = newBackend({ proposals: Array.from({ length: 120 }, (_, n) => proposal(n)) });
    renderAt("/model/rules", backend);
    const nav = await screen.findByRole("navigation", { name: "Main navigation" });
    await waitFor(() => expect(within(nav).getByRole("link", { name: /Inbox/ }).querySelector("[data-badge]")?.textContent).toBe("99+"));
    expect(within(nav).getByRole("link", { name: "Model" }).getAttribute("aria-current")).toBe("location");
    cleanup();
    renderAt("/overview", newBackend({ proposals: [], questions: [] }));
    const fresh = await screen.findByRole("navigation", { name: "Main navigation" });
    await waitFor(() => expect(within(fresh).getByRole("link", { name: /Inbox/ }).textContent).toContain("1 item waiting"));
  });

  it("collapses to 64 px with icon-only items that keep their names, and remembers it", async () => {
    const { user } = renderAt("/overview");
    await screen.findByRole("navigation", { name: "Main navigation" });
    await user.click(screen.getByRole("button", { name: "Collapse sidebar" }));
    const sidebar = sidebarNav().closest("aside") as HTMLElement;
    expect(sidebar.style.width).toBe("64px");
    expect(within(sidebarNav()).getByRole("link", { name: "Overview" })).toBeTruthy();
    expect(within(sidebar).queryByText("Marta Nowak")).toBeNull();
    expect(window.localStorage.getItem(app.preferences.SIDEBAR_STORAGE_KEY)).toBe("collapsed");
    await user.click(screen.getByRole("button", { name: "Expand sidebar" }));
    expect(sidebar.style.width).toBe("232px");
  });

  it("navigates client-side on a plain click", async () => {
    const { user, router } = renderAt("/overview");
    await screen.findByRole("navigation", { name: "Main navigation" });
    await user.click(within(sidebarNav()).getByRole("link", { name: "Scenarios" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/scenarios"));
  });
});

// ------------------------------------------------------------------ toolbar

describe("Toolbar", () => {
  it("shows the project, the version, search, the computed Next action, Run step and the Assistant toggle", async () => {
    const { user, router } = renderAt("/overview");
    const bar = await screen.findByRole("banner");
    expect(await within(bar).findByRole("button", { name: /HR leave/ })).toBeTruthy();
    const version = await within(bar).findByText("v42");
    expect(version.closest("[data-version-state]")?.getAttribute("data-version-state")).toBe("upToDate");
    expect(within(bar).getByText("Up to date")).toBeTruthy();
    expect(within(bar).getByRole("button", { name: /Search or run a command/ })).toBeTruthy();
    const next = await within(bar).findByRole("button", { name: /Review 7 proposals/ });
    expect(next.getAttribute("class")).toContain("bg-accent");
    expect(within(bar).getByRole("button", { name: "Run next step" })).toBeTruthy();
    const assistant = within(bar).getByRole("button", { name: "Assistant" });
    expect(assistant.getAttribute("aria-pressed")).toBe("false");
    await user.click(next);
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox"));
    // The project name opens Settings (project details).
    await user.click(within(toolbar()).getByRole("button", { name: /HR leave/ }));
    expect(await screen.findByRole("dialog", { name: "Settings" })).toBeTruthy();
  });

  it("follows the priority order: questions next, then the blocked gate, then Export", async () => {
    renderAt("/overview", newBackend({ proposals: [] }));
    expect(await within(await screen.findByRole("banner")).findByRole("button", { name: /Answer 2 questions/ })).toBeTruthy();
    cleanup();
    renderAt("/overview", newBackend({ proposals: [], questions: [] }));
    expect(await within(await screen.findByRole("banner")).findByRole("button", { name: /Resolve Decisions/ })).toBeTruthy();
    cleanup();
    const { user } = renderAt("/overview", newBackend({ proposals: [], questions: [], gates: ALL_PASS }));
    const exportButton = await within(await screen.findByRole("banner")).findByRole("button", { name: /^Export/ });
    await user.click(exportButton);
    expect(await screen.findByRole("dialog", { name: "Export specification" })).toBeTruthy();
  });

  it("shows Changed — Refresh and the connection banner while the live connection is lost", async () => {
    const { user, queryClient } = renderAt("/overview");
    await screen.findByText("Up to date");
    const [socket] = FakeWebSocket.instances;
    act(() => socket?.open());
    expect(screen.queryByText(/Live connection lost/)).toBeNull();
    act(() => socket?.drop(1006));
    const banner = await screen.findByText("Live connection lost. Reconnecting… Your data refreshes when it's back.");
    expect(banner.closest("[role='status']")).not.toBeNull();
    const refresh = within(toolbar()).getByRole("button", { name: /Changed — Refresh/ });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    await user.click(refresh);
    expect(invalidate).toHaveBeenCalled();
    await waitFor(() => expect(FakeWebSocket.instances.length).toBe(2), { timeout: 3000 });
    act(() => FakeWebSocket.instances[1]?.open());
    await waitFor(() => expect(screen.queryByText(/Live connection lost/)).toBeNull());
    expect(within(toolbar()).getByText("Up to date")).toBeTruthy();
  });

  it("runs a step from the menu against the reviewed version, polls the job with progress and toasts the result", async () => {
    const backend = newBackend({
      proposals: [],
      questions: [],
      gates: ALL_PASS,
      jobs: [
        { status: "evaluating", progress: 0.75, base_revision: VERSION.revision },
        { status: "committed", progress: 1, base_revision: VERSION.revision, result_version: "rev:43:bbbbbbbbbbbbbbbb" },
      ],
    });
    const { user, router } = renderAt("/overview", backend);
    await within(await screen.findByRole("banner")).findByRole("button", { name: /^Export/ });
    await user.click(within(toolbar()).getByRole("button", { name: "Choose a step to run" }));
    const menu = await screen.findByRole("menu");
    expect(within(menu).getByText("Run a step")).toBeTruthy();
    expect(within(menu).getAllByRole("menuitem").map((i) => i.textContent)).toEqual([
      "Requirements step",
      "Model step",
      "Glossary & quality step",
      "Directory step",
      "Analysis step",
      "Flows step",
      "Calculations step",
      "Rules step",
      "Scenarios step",
      "Tags step",
      "Scenario run step",
    ]);
    await user.click(within(menu).getByRole("menuitem", { name: "Flows step" }));
    await waitFor(() => expect(backend.runs).toEqual([{ stage: "flows", ifMatch: VERSION_ETAG }]));
    expect(await within(toolbar()).findByRole("button", { name: /Analysing… 75%/ }, { timeout: 3000 })).toBeTruthy();
    const run = within(toolbar()).getByRole("button", { name: "Flows step running, 75%" });
    expect(run.querySelector("[data-run-progress]")?.getAttribute("style")).toContain("75%");
    const toastText = await screen.findByText("Flows step finished", undefined, { timeout: 4000 });
    expect(toastText).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Review" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox"));
  });

  it("offers Retry after a failed step and opens the menu when the next step is unknown", async () => {
    const backend = newBackend({ jobs: [{ status: "failed", progress: 1, base_revision: VERSION.revision, error: "E_INFERENCE_FAILED: timeout" }] });
    const { user } = renderAt("/overview", backend);
    await within(await screen.findByRole("banner")).findByRole("button", { name: /Review 7 proposals/ });
    // Without pipeline data the primary half opens the step menu.
    await user.click(within(toolbar()).getByRole("button", { name: "Run next step" }));
    await user.click(await screen.findByRole("menuitem", { name: "Requirements step" }));
    expect(await screen.findByText("Requirements step failed", undefined, { timeout: 3000 })).toBeTruthy();
    const retry = await within(toolbar()).findByRole("button", { name: /Retry Requirements/ });
    await user.click(retry);
    // S1: Retry re-runs the failed job against the reviewed version.
    await waitFor(() => expect(backend.retries).toEqual([{ job: "job:1", ifMatch: VERSION_ETAG }]));
    expect(backend.runs.map((r) => r.stage)).toEqual(["extract"]);
  });

  it("runs the server's next step and retries a step the pipeline reports failed", async () => {
    const backend = newBackend({
      proposals: [],
      questions: [],
      gates: ALL_PASS,
      pipeline: { steps: [pipelineStep("extract", "done", { job_id: "job:x", progress: 1 })], sources: 1, open_questions: 0, next: "lint" },
    });
    const { user } = renderAt("/overview", backend);
    expect(await within(await screen.findByRole("banner")).findByRole("button", { name: /Run Glossary & quality/ })).toBeTruthy();
    await user.click(within(toolbar()).getByRole("button", { name: "Run next step" }));
    await waitFor(() => expect(backend.runs).toEqual([{ stage: "lint", ifMatch: VERSION_ETAG }]));
    cleanup();
    app.run.useRunStore.getState().clear();
    const failing = newBackend({
      pipeline: { steps: [pipelineStep("flows", "failed", { job_id: "job:f", error: { code: "E_STAGE_EVALUATION_FAILED", message: "The flows step failed." } })], sources: 1, open_questions: 0, next: null },
    });
    const second = renderAt("/overview", failing);
    const retry = await within(await screen.findByRole("banner")).findByRole("button", { name: /Retry Flows/ });
    expect(retry.getAttribute("title")).toBe("Flows failed — Retry");
    await second.user.click(retry);
    await waitFor(() => expect(failing.retries).toEqual([{ job: "job:f", ifMatch: VERSION_ETAG }]));
  });

  it("marks steps that cannot run in the menu: AI steps while AI is off, steps missing configuration", async () => {
    const backend = newBackend({
      me: { ...ME, inference_available: false, inference_model: null },
      pipeline: { steps: [pipelineStep("extract", "done"), pipelineStep("lint", "not_started", { missing_config: ["gates.f1"] })], sources: 1, open_questions: 0, next: null },
    });
    const { user } = renderAt("/overview", backend);
    await within(await screen.findByRole("banner")).findByRole("button", { name: /Review 7 proposals/ });
    await user.click(within(toolbar()).getByRole("button", { name: "Choose a step to run" }));
    const menu = await screen.findByRole("menu");
    const item = (name: string) => within(menu).getByRole("menuitem", { name: new RegExp("^" + name) });
    expect(item("Requirements step").getAttribute("aria-disabled")).toBe("true");
    expect(item("Requirements step").textContent).toContain("Needs AI");
    expect(item("Glossary & quality step").getAttribute("aria-disabled")).toBe("true");
    expect(item("Glossary & quality step").textContent).toContain("Not configured");
    expect(item("Directory step").getAttribute("aria-disabled")).toBeNull();
  });

  it("is compact below 1280 px: icon-only search and run, the version pill without its status text", async () => {
    compact = true;
    renderAt("/overview");
    const bar = await screen.findByRole("banner");
    await within(bar).findByText("v42");
    expect(within(bar).queryByText("Up to date")).toBeNull();
    const search = within(bar).getByRole("button", { name: "Search or run a command" });
    expect(search.textContent).toBe("");
    const run = within(bar).getByRole("button", { name: "Run next step" });
    expect(run.textContent).toBe("");
  });
});

// ------------------------------------------------------------------ command palette

describe("Command palette", () => {
  it("opens with ⌘K, lists commands, searches the model grouped by kind and opens a hit", async () => {
    const { user, router, backend } = renderAt("/overview");
    await screen.findByRole("banner");
    await user.keyboard("{Meta>}k{/Meta}");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    const input = within(dialog).getByRole("combobox");
    expect(document.activeElement).toBe(input);
    const listbox = within(dialog).getByRole("listbox");
    expect(within(listbox).getByRole("group", { name: "Commands" })).toBeTruthy();
    const commands = within(listbox).getAllByRole("option").map((o) => o.getAttribute("data-label"));
    expect(commands).toEqual(expect.arrayContaining(["Run next step", "Go to Inbox", "Go to Quality", "Export specification", "Switch to dark appearance"]));
    expect(backend.searches).toEqual([]);
    // A blank query never reaches the server.
    await user.type(input, "   ");
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(backend.searches).toEqual([]);
    await user.clear(input);
    await user.type(input, "leave");
    const requirements = await within(dialog).findByRole("group", { name: "Requirements" });
    expect(within(requirements).getByRole("option").textContent).toContain("An employee shall be able to submit a leave request.");
    expect(within(dialog).getByRole("group", { name: "Glossary" })).toBeTruthy();
    expect(within(dialog).getByRole("group", { name: "mystery" })).toBeTruthy();
    expect(backend.searches.at(-1)).toBe("leave");
    // Search covers Accepted elements only (search_model), and the palette says so.
    expect(within(dialog).getByText("Only accepted elements are searchable.")).toBeTruthy();
    // Arrow keys move the active option; Enter opens it.
    const options = () => within(dialog).getAllByRole("option");
    const target = options().findIndex((o) => o.getAttribute("data-label") === "req:leave-1");
    for (let i = 0; i < target; i += 1) {
      await user.keyboard("{ArrowDown}");
    }
    expect(input.getAttribute("aria-activedescendant")).toBe(options()[target]?.id);
    await user.keyboard("{Enter}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/requirements/req:leave-1"));
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
    // The opened hit is now a recent item.
    await user.click(within(toolbar()).getByRole("button", { name: /Search or run a command/ }));
    const reopened = await screen.findByRole("dialog", { name: "Command palette" });
    expect(within(within(reopened).getByRole("group", { name: "Recent" })).getByRole("option").getAttribute("data-label")).toBe("req:leave-1");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull());
  });

  it("says when nothing matches and runs commands such as switching appearance", async () => {
    const { user } = renderAt("/overview");
    await screen.findByRole("banner");
    await user.keyboard("{Control>}k{/Control}");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    await user.type(within(dialog).getByRole("combobox"), "zzz");
    expect(await within(dialog).findByText("No results for “zzz”. Try a requirement ID like HR-009.")).toBeTruthy();
    await user.clear(within(dialog).getByRole("combobox"));
    await user.type(within(dialog).getByRole("combobox"), "dark");
    await user.click(await within(dialog).findByRole("option", { name: /Switch to dark appearance/ }));
    expect(document.documentElement.dataset["appearance"]).toBe("dark");
    expect(window.localStorage.getItem(app.preferences.APPEARANCE_STORAGE_KEY)).toBe("dark");
    await user.keyboard("{Meta>}k{/Meta}");
    expect(await screen.findByRole("option", { name: /Switch to light appearance/ })).toBeTruthy();
  });

  it("goes to a destination and opens the Assistant from commands", async () => {
    const { user, router } = renderAt("/overview");
    await screen.findByRole("banner");
    await user.keyboard("{Meta>}k{/Meta}");
    await user.click(await screen.findByRole("option", { name: /Go to Quality/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/quality"));
    await user.keyboard("{Meta>}k{/Meta}");
    await user.type(await screen.findByRole("combobox"), "assistant");
    await user.click(await screen.findByRole("option", { name: /Open the Assistant/ }));
    expect(await screen.findByRole("complementary", { name: "Assistant" })).toBeTruthy();
  });

  it("exports every selected format from the Export sheet", async () => {
    const createObjectURL = vi.fn(() => "blob:plumb");
    const revokeObjectURL = vi.fn();
    vi.stubGlobal("URL", Object.assign(URL, { createObjectURL, revokeObjectURL }));
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const { user, backend } = renderAt("/overview");
    await screen.findByRole("banner");
    await user.keyboard("{Meta>}k{/Meta}");
    await user.click(await screen.findByRole("option", { name: /Export specification/ }));
    const sheet = await screen.findByRole("dialog", { name: "Export specification" });
    const boxes = within(sheet).getAllByRole("checkbox");
    expect(boxes).toHaveLength(6);
    await user.click(within(sheet).getByRole("checkbox", { name: /Decisions/ }));
    await user.click(within(sheet).getByRole("button", { name: /Download/ }));
    await waitFor(() => expect(backend.exports).toEqual(["functional", "glossary", "scenarios", "report", "gate_report"]));
    expect(click).toHaveBeenCalledTimes(5);
    expect(await screen.findByText("5 files downloaded")).toBeTruthy();
    click.mockRestore();
  });
});

// ------------------------------------------------------------------ settings

describe("Settings", () => {
  it("switches language and appearance (persisted per browser) and shows project, database and AI status", async () => {
    const { user } = renderAt("/overview");
    await screen.findByRole("banner");
    await user.click(screen.getByRole("button", { name: "Settings" }));
    const dialog = await screen.findByRole("dialog", { name: "Settings" });
    // S1: the project, its identifier, the database and the AI model come from GET /api/me.
    expect(await within(dialog).findByText("Available · claude-sonnet-4-5")).toBeTruthy();
    expect(within(dialog).getByTestId("settings-project").textContent).toBe("HR leave · project:hr-leave · v42");
    expect(within(dialog).getByText("/srv/plumb/hr-leave/plumb.sqlite")).toBeTruthy();
    expect(within(dialog).getByText("PLUMB_API_KEY")).toBeTruthy();
    const appearance = within(dialog).getByRole("radiogroup", { name: "Appearance" });
    expect(within(appearance).getByRole("radio", { name: "System" }).getAttribute("aria-checked")).toBe("true");
    await user.click(within(appearance).getByRole("radio", { name: "Dark" }));
    expect(document.documentElement.dataset["appearance"]).toBe("dark");
    expect(window.localStorage.getItem("plumb.appearance")).toBe("dark");
    await user.click(within(appearance).getByRole("radio", { name: "Light" }));
    expect(document.documentElement.dataset["appearance"]).toBe("light");
    await user.click(within(appearance).getByRole("radio", { name: "System" }));
    expect(document.documentElement.dataset["appearance"]).toBeUndefined();
    const language = within(dialog).getByRole("radiogroup", { name: "Language" });
    await user.click(within(language).getByRole("radio", { name: "Polski" }));
    const polish = await screen.findByRole("dialog", { name: "Ustawienia" });
    expect(window.localStorage.getItem(app.i18n.LOCALE_STORAGE_KEY)).toBe("pl");
    expect(document.documentElement.lang).toBe("pl");
    await user.click(within(polish).getByRole("button", { name: "Zamknij ustawienia" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    const polishNav = await screen.findByRole("navigation", { name: "Nawigacja główna" });
    expect(within(polishNav).getByRole("link", { name: "Przegląd" })).toBeTruthy();
  });

  it("states when AI is off", async () => {
    const { user } = renderAt("/overview", newBackend({ me: { ...ME, inference_available: false } }));
    await screen.findByRole("banner");
    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Off — no API key configured")).toBeTruthy();
  });

  it("falls back to an untitled project when the server reports none", async () => {
    const { user } = renderAt("/overview", newBackend({ me: { user: "alice", mode: "pilot", roles: [], inference_available: true } as unknown as Me }));
    expect(await within(await screen.findByRole("banner")).findByRole("button", { name: /Untitled project/ })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Settings" }));
    const dialog = await screen.findByRole("dialog", { name: "Settings" });
    expect(await within(dialog).findByText("Available")).toBeTruthy();
    expect(within(dialog).getByText("Not reported by this server")).toBeTruthy();
  });
});

// ------------------------------------------------------------------ global keys, assistant

describe("Global keys and the Assistant panel", () => {
  it("jumps to destinations with ⌘1–7 and closes overlays with Esc", async () => {
    const { user, router } = renderAt("/overview");
    await screen.findByRole("banner");
    await user.keyboard("{Meta>}2{/Meta}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox"));
    await user.keyboard("{Control>}5{/Control}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/scenarios"));
    await user.keyboard("{Meta>}7{/Meta}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/sources"));
    await user.keyboard("{Meta>}1{/Meta}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/overview"));
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await screen.findByRole("dialog", { name: "Settings" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    // Plain digits do nothing.
    fireEvent.keyDown(window, { key: "3" });
    expect(router.state.location.pathname).toBe("/overview");
  });

  it("toggles the Assistant panel, scoped to the project or to the open requirement", async () => {
    const { user } = renderAt("/overview");
    const bar = await screen.findByRole("banner");
    await user.click(within(bar).getByRole("button", { name: "Assistant" }));
    const panel = await screen.findByRole("complementary", { name: "Assistant" });
    expect(within(panel).getByText("Scope · Project")).toBeTruthy();
    expect(within(panel).getByTestId("assistant-conversation").getAttribute("data-scope")).toBe("project");
    expect(within(bar).getByRole("button", { name: "Assistant" }).getAttribute("aria-pressed")).toBe("true");
    await user.click(within(panel).getByRole("button", { name: "Close the Assistant" }));
    expect(screen.queryByRole("complementary", { name: "Assistant" })).toBeNull();
    cleanup();
    server.use(http.get("*/api/requirements/:id", () => etagJson({ code: "E_NOT_FOUND", message: "missing", details: {} }, { status: 404 })));
    app.preferences.useShellStore.setState({ assistantOpen: true });
    renderAt("/requirements/req:7");
    const scoped = await screen.findByRole("complementary", { name: "Assistant" });
    expect(within(scoped).getByText("Scope · req:7")).toBeTruthy();
    expect(within(scoped).getByTestId("assistant-conversation").getAttribute("data-scope")).toBe("requirement");
  });
});

// ------------------------------------------------------------------ routes

describe("Routes", () => {
  it("registers exactly the spec §3 URL structure and redirects / to /overview", async () => {
    const { router } = renderAt("/");
    await waitFor(() => expect(router.state.location.pathname).toBe("/overview"));
    expect(Object.keys(router.routesByPath).sort()).toEqual(
      [
        "/",
        "/inbox",
        "/inbox/$id",
        "/model",
        "/model/calculations",
        "/model/calculations/$id",
        "/model/entities",
        "/model/entities/$id",
        "/model/glossary",
        "/model/glossary/$id",
        "/model/processes",
        "/model/processes/$id",
        "/model/rules",
        "/model/rules/$id",
        "/overview",
        "/quality",
        "/quality/$gate",
        "/quality/history",
        "/quality/issues",
        "/requirements",
        "/requirements/$id",
        "/scenarios",
        "/scenarios/$id",
        "/sources",
        "/sources/$id",
      ].sort(),
    );
  });

  const ROUTES = [
    "/overview",
    "/inbox",
    "/inbox?filter=questions",
    "/inbox?filter=wording",
    "/inbox/prop:1",
    "/requirements",
    "/requirements/req:a",
    "/model/glossary",
    "/model/glossary/term:a",
    "/model/entities",
    "/model/entities/ent:a",
    "/model/rules",
    "/model/rules/rule:a",
    "/model/calculations",
    "/model/calculations/calc:a",
    "/model/processes",
    "/model/processes/proc:a",
    "/scenarios",
    "/scenarios/scn:a",
    "/quality",
    "/quality/F1",
    "/quality/history",
    "/quality/issues",
    "/sources",
    "/sources/src:a",
  ];

  it("renders every route inside the shell with exactly one main h1, in failure states too", async () => {
    for (const path of ROUTES) {
      server.use(
        http.get("*/api/me", () => HttpResponse.json(ME)),
        http.get("*/api/*", () => HttpResponse.json({ code: "E_A03_RUNTIME_UNAVAILABLE", message: "not configured", details: {} }, { status: 503 })),
      );
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      const router = app.router.createAppRouter(createMemoryHistory({ initialEntries: [path] }));
      const view = render(
        <QueryClientProvider client={queryClient}>
          <RouterProvider router={router} />
        </QueryClientProvider>,
      );
      await waitFor(() => expect(router.state.location.href).toBe(path));
      expect(screen.getByRole("navigation", { name: "Main navigation" })).toBeTruthy();
      await waitFor(() => expect(queryClient.isFetching()).toBe(0));
      expect(document.querySelectorAll("main h1"), path).toHaveLength(1);
      view.unmount();
      server.resetHandlers();
    }
  });

  it("switches Model and Quality views with segmented navigation and Inbox filters with links", async () => {
    const { user, router } = renderAt("/model/glossary");
    const views = await screen.findByRole("navigation", { name: "Model views" });
    expect(within(views).getByRole("link", { name: "Glossary" }).getAttribute("aria-current")).toBe("page");
    await user.click(within(views).getByRole("link", { name: "Rules" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/model/rules"));
    act(() => {
      void router.navigate({ to: "/quality/history" });
    });
    const quality = await screen.findByRole("navigation", { name: "Quality views" });
    expect(within(quality).getByRole("link", { name: "History" }).getAttribute("aria-current")).toBe("page");
    act(() => {
      void router.navigate({ to: "/inbox" });
    });
    const inbox = await screen.findByRole("navigation", { name: "Inbox filters" });
    await user.click(within(inbox).getByRole("link", { name: /^Questions/ }));
    await waitFor(() => expect(router.state.location.search).toEqual({ filter: "questions" }));
  });

  it("renders a typed not-found page inside the shell", async () => {
    renderAt("/no/such/page");
    expect(await screen.findByRole("heading", { level: 1, name: "Page not found" })).toBeTruthy();
    expect(screen.getByRole("navigation", { name: "Main navigation" })).toBeTruthy();
  });

  it("has no axe violations on the Overview shell", async () => {
    renderAt("/overview");
    await within(await screen.findByRole("banner")).findByRole("button", { name: /Review 7 proposals/ });
    expect(await axeViolations(document.body)).toEqual([]);
  });
});
