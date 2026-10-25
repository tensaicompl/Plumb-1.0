// Overview (handoff "1. Overview", UI-SPEC §5.1, flows 1 and 9): header with readiness and
// provenance, the next-action card, the 9-step pipeline over GET /api/pipeline, the gates row, at
// a glance and recent decisions; the first-run, running, all-green, AI-unavailable and failed-step
// states. Rendered through the real router with every request answered by MSW.
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { delay, http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { axeViolations } from "../../test/axe";
import { app, calls, renderApp, resetApp, setupApp, teardownApp, uploadedFile } from "../../test/appHarness";
import { etagJson, server } from "../../test/msw";

type Me = components["schemas"]["MeResponse"];
type GateStatus = components["schemas"]["GateStatus"];
type PipelineResponse = components["schemas"]["PipelineResponse"];
type PipelineStepWire = components["schemas"]["PipelineStep"];
type PendingProposal = components["schemas"]["PendingProposal"];
type Decision = components["schemas"]["Decision"];
type RequirementSummary = components["schemas"]["RequirementPage"]["items"][number];
type ScenarioView = components["schemas"]["ScenarioPage"]["items"][number];
type JobResponse = components["schemas"]["JobResponse"];

beforeAll(setupApp);
afterAll(teardownApp);
beforeEach(() => {
  resetApp();
  vi.useFakeTimers({ shouldAdvanceTime: true, now: new Date("2026-10-25T12:04:00Z") });
});
afterEach(() => {
  vi.useRealTimers();
});

const VERSION_ETAG = '"rev:42:aaaaaaaaaaaaaaaa"';
const VERSION = {
  version: "rev:42:aaaaaaaaaaaaaaaa",
  revision: "rev:42:aaaaaaaaaaaaaaaa",
  hash: "psg:sha256:a",
  semantic_hash: "psg:sha256:a",
  evidence_hash: "ev:sha256:a",
  created_at: "2026-10-25T12:00:00.123456789Z",
  created_by: "agent:plumb",
};
const ME: Me = {
  user: "Marta Nowak",
  mode: "pilot",
  roles: ["analyst", "reviewer"],
  inference_available: true,
  inference_model: "claude-sonnet-4-5",
  project: { id: "project:hr-leave", name: "HR leave", database: "/srv/hr-leave/plumb.sqlite" },
};

function wire(recipe: PipelineStepWire["recipe"], status: PipelineStepWire["status"], extra: Partial<PipelineStepWire> = {}): PipelineStepWire {
  return { recipe, status, job_id: "job:" + recipe, progress: status === "not_started" ? null : 1, pending_proposals: 0, error: null, updated_at: null, missing_config: [], ...extra };
}
const RECIPES = ["extract", "consolidate", "lint", "directory", "analyse", "flows", "calc", "rules", "scenarios", "tags", "run"] as const;
const ALL_DONE: PipelineResponse = { steps: RECIPES.map((r) => wire(r, "done")), sources: 1, open_questions: 0, next: null };
const NEEDS_REVIEW: PipelineResponse = {
  steps: RECIPES.map((r) => (r === "flows" ? wire(r, "needs_review", { pending_proposals: 7 }) : wire(r, "done"))),
  sources: 1,
  open_questions: 2,
  next: null,
};

function gate(name: GateStatus["gate"], passed: boolean, state: GateStatus["state"] = "current", blockers = passed ? 0 : 2): GateStatus {
  return { gate: name, state, passed, reason: null, findings: [], rules: [], waivers: [], summary: { blocker_failed: blockers, blocker_waived: 0, warnings: 0 } };
}
const ALL_PASS: GateStatus[] = (["I0", "F1", "F2", "F3", "F4"] as const).map((g) => gate(g, true));

function proposal(n: number): PendingProposal {
  return { proposal_id: "prop:" + n, action: "create", origin: "stage", base_revision: VERSION.revision, discardable: false, requires_reason: false, stale: false, title: "Proposal " + n, element_kind: "operation", created_at: "2026-10-25T11:00:00Z", stage: "flows" };
}

function requirement(n: number, smells: string[] = []): RequirementSummary {
  return { id: "req:" + n, text: "Requirement " + n, class: "functional", status: "Accepted", smells, coverage: { operations: [], scenarios: [] }, readiness: null, lint_current: true } as unknown as RequirementSummary;
}

function scenario(n: number, result: string | null): ScenarioView {
  return { id: "scenario:" + n, given: [], when: "", then: [], status: "Accepted", confirmation_status: "Confirmed", derived_from: [], run: result === null ? null : { result, diff: null, missing: null, trace: [] } } as unknown as ScenarioView;
}

function decision(n: number, at: string, extra: Partial<Decision> = {}): Decision {
  return { id: "dec:" + n, decided_at: at, decided_by: "agent:marta-nowak", proposal_ref: "prop:" + n, question_ref: null, rationale: null, ...extra };
}

interface Backend {
  me: Me;
  pipeline: PipelineResponse;
  gates: GateStatus[];
  proposals: PendingProposal[];
  questions: number;
  sources: { id: string; name: string }[];
  jobs: JobResponse[];
  runs: string[];
  retries: string[];
  uploads: { name: string; type: string; ifMatch: string | null }[];
  upload: () => Response;
  readinessFails: boolean;
}

function backend(overrides: Partial<Backend> = {}): Backend {
  return {
    me: ME,
    pipeline: NEEDS_REVIEW,
    gates: ALL_PASS.map((g) => (g.gate === "F3" ? gate("F3", false) : g)),
    proposals: [1, 2, 3, 4, 5, 6, 7].map(proposal),
    questions: 2,
    sources: [{ id: "src:1", name: "requirements.md" }],
    jobs: [],
    runs: [],
    retries: [],
    uploads: [],
    upload: () => etagJson({ id: "src:2", hash: "sha256:2", spans_count: 38, events: [], revision: VERSION.revision, semantic_hash: VERSION.semantic_hash, version: VERSION.version }),
    readinessFails: false,
    ...overrides,
  };
}

function serve(b: Backend) {
  const runResponse = (n: number) => etagJson({ job_id: "job:run-" + n, events: [], revision: VERSION.revision, semantic_hash: VERSION.semantic_hash, version: VERSION.version });
  server.use(
    http.get("*/api/me", () => HttpResponse.json(b.me)),
    http.get("*/api/model/version", () => etagJson(VERSION, undefined, VERSION_ETAG)),
    http.get("*/api/pipeline", () => etagJson(b.pipeline, undefined, VERSION_ETAG)),
    http.get("*/api/gates", () => etagJson(b.gates)),
    http.get("*/api/proposals", () => etagJson({ items: b.proposals }, undefined, VERSION_ETAG)),
    http.get("*/api/questions", () =>
      etagJson({ items: Array.from({ length: b.questions }, (_, i) => ({ id: "q:" + i, text: "Q", kind: "yes_no", status: "Open" })), health: [], next_cursor: null }),
    ),
    http.get("*/api/findings", () => etagJson({ items: [], next_cursor: null })),
    http.get("*/api/readiness", () =>
      b.readinessFails
        ? HttpResponse.json({ code: "E_INTERNAL", message: "boom", details: {} }, { status: 500 })
        : etagJson({ overall: 86, contributing: [], sub: { clarity: 92, completeness: 84, confirmation: 78, consistency: 95, verifiability: null } }),
    ),
    http.get("*/api/sources", () =>
      etagJson({ items: b.sources.map((s) => ({ ...s, hash: "sha256:" + s.id, media_type: "text/markdown", source_kind: "markdown", imported_at: "2026-10-25T09:00:00Z", spans_count: 38 })) }),
    ),
    http.get("*/api/requirements", () => etagJson({ items: [requirement(1, ["weak"]), requirement(2), requirement(3, ["vague"])], health: [], next_cursor: null })),
    http.get("*/api/model/entities", () =>
      etagJson({ items: [{ id: "entity:1", name: "Leave request", attributes: [], relationships: [], states: [{}, {}, {}], transitions: [], tags: [] }, { id: "entity:2", name: "Employee", attributes: [], relationships: [], states: [{}], transitions: [], tags: [] }], health: [], next_cursor: null }),
    ),
    http.get("*/api/model/crud", () => etagJson({ operations: ["op:1", "op:2"], entities: [], cells: [] })),
    http.get("*/api/scenarios", () => etagJson({ items: [scenario(1, "pass"), scenario(2, "pass"), scenario(3, "fail"), scenario(4, null)], health: [], next_cursor: null })),
    http.get("*/api/decisions", () =>
      etagJson({
        items: [
          decision(1, "2026-10-21T13:48:00Z"),
          decision(2, "2026-10-25T08:42:00Z", { question_ref: "q:1", proposal_ref: null, rationale: "Policy §4.2 excludes statutory holidays." }),
          decision(3, "2026-10-24T14:30:00Z"),
          decision(4, "2026-10-24T12:02:00Z", { proposal_ref: null }),
          decision(5, "2026-10-23T09:20:00Z"),
          decision(6, "2026-10-25T09:00:00Z"),
        ],
        next_cursor: null,
      }),
    ),
    http.post("*/api/stages/:stage/run", ({ params }) => {
      b.runs.push(String(params["stage"]));
      return runResponse(b.runs.length);
    }),
    http.post("*/api/jobs/:id/retry", ({ params }) => {
      b.retries.push(String(params["id"]));
      return runResponse(99);
    }),
    http.get("*/api/jobs/:id", () => etagJson(b.jobs.length > 1 ? b.jobs.shift() : (b.jobs[0] ?? { status: "queued", progress: 0, base_revision: VERSION.revision }))),
    http.post("*/api/sources", async ({ request }) => {
      const file = await uploadedFile(request);
      b.uploads.push({ name: file.name, type: file.type, ifMatch: request.headers.get("if-match") });
      return b.upload();
    }),
  );
  return b;
}

const main = () => screen.getByRole("main");

describe("Overview", () => {
  it("shows the project, provenance, readiness, the next action, pipeline, gates, at a glance and recent decisions", async () => {
    const b = serve(backend());
    const { user, router } = renderApp("/overview");
    await screen.findByRole("main");
    const heading = await within(main()).findByRole("heading", { level: 1, name: "HR leave" });
    expect(heading).toBeTruthy();
    expect(within(main()).getByText("Overview")).toBeTruthy();
    expect(await within(main()).findByText("Updated 4 minutes ago by plumb · 3 requirements from requirements.md")).toBeTruthy();
    // Readiness ring and the five sub-scores (verifiability not calculated).
    const readiness = within(main()).getByRole("region", { name: "Readiness" });
    expect(await within(readiness).findByRole("img", { name: "Readiness 86%" })).toBeTruthy();
    expect(within(readiness).getByText("Clarity").parentElement?.textContent).toContain("92");
    expect(within(readiness).getByText("Verifiability").parentElement?.textContent).toContain("—");
    // The next action card (midnight hero) reuses the shell's computed next action.
    const card = within(main()).getByRole("region", { name: "Next action" });
    expect(await within(card).findByText("Review 7 proposals")).toBeTruthy();
    expect(within(card).getByText("Nothing changes until you accept.")).toBeTruthy();
    // The pipeline: nine steps, the Flows step needs review (7), Questions (2), Ready not started.
    const pipeline = within(main()).getByRole("region", { name: "Pipeline" });
    const steps = within(pipeline).getAllByRole("link");
    expect(steps.map((s) => s.getAttribute("data-step"))).toEqual(["import", "requirements", "glossary", "model", "flows", "calcRules", "scenarios", "questions", "ready"]);
    expect(steps.map((s) => s.getAttribute("data-step-state"))).toEqual(["done", "done", "done", "done", "needs_review", "done", "done", "needs_review", "not_started"]);
    expect(steps[4]?.textContent).toContain("Flows");
    expect(steps[4]?.textContent).toContain("7");
    expect(steps[4]?.textContent).toContain("Needs review");
    expect(steps[8]?.textContent).toContain("Not started");
    expect(steps[0]?.getAttribute("href")).toBe("/sources");
    // Gates: Decisions is blocked with one line of detail.
    const gates = within(main()).getByRole("region", { name: "Quality gates" });
    const decisions = within(gates).getByRole("link", { name: /Decisions/ });
    expect(decisions.textContent).toContain("Blocked · 2 blocking issues");
    expect(within(gates).getByRole("link", { name: /Evidence/ }).textContent).toContain("Passes");
    // At a glance.
    const glance = within(main()).getByRole("region", { name: "At a glance" });
    const tiles = within(glance).getAllByRole("link");
    expect(tiles.map((t) => t.textContent)).toEqual([
      "Requirements32 with issues",
      "Model2 entities4 states · 2 operations",
      "Scenarios42 pass",
      "Open questions2Waiting for an answer",
    ]);
    // Recent decisions: the latest five, newest first, with who and when.
    const recent = within(main()).getByRole("region", { name: "Recent decisions" });
    const rows = within(recent).getAllByRole("listitem");
    expect(rows).toHaveLength(5);
    expect(rows[0]?.textContent).toContain("marta-nowak");
    expect(rows[0]?.textContent).toContain("Today, ");
    expect(rows[1]?.textContent).toContain("answered a question");
    expect(rows[1]?.textContent).toContain("“Policy §4.2 excludes statutory holidays.”");
    expect(rows[3]?.textContent).toContain("Yesterday, ");
    expect(within(recent).getByText("recorded a decision")).toBeTruthy();
    expect(await axeViolations(main())).toEqual([]);
    // Navigation: a step, a gate, a tile, History and the next action deep-link.
    await user.click(steps[4] as HTMLElement);
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox"));
    const region = async (name: string) => {
      router.history.push("/overview");
      return within(await within(main()).findByRole("region", { name }));
    };
    await user.click((await region("Quality gates")).getByRole("link", { name: /Decisions/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/quality/F3"));
    await user.click((await region("Recent decisions")).getByRole("link", { name: "History" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/quality/history"));
    await user.click((await region("At a glance")).getByRole("link", { name: /Scenarios/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/scenarios"));
    await user.click(await (await region("Next action")).findByRole("button", { name: "Review proposals" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox"));
    expect(b.runs).toEqual([]);
  });

  it("shows skeletons while loading and a read failure with Retry", async () => {
    serve(backend({ readinessFails: true }));
    renderApp("/overview");
    await screen.findByRole("main");
    const readiness = await within(main()).findByRole("region", { name: "Readiness" });
    expect(await within(readiness).findByText("Readiness could not be loaded.")).toBeTruthy();
    const before = calls.filter((c) => c.path === "/api/readiness").length;
    await act(async () => within(readiness).getByRole("button", { name: "Retry" }).click());
    await waitFor(() => expect(calls.filter((c) => c.path === "/api/readiness").length).toBe(before + 1));
  });

  it("shows skeletons matching the layout while its reads load", async () => {
    serve(backend());
    const pending = () => delay("infinite");
    server.use(
      http.get("*/api/readiness", pending),
      http.get("*/api/pipeline", pending),
      http.get("*/api/decisions", pending),
      http.get("*/api/requirements", pending),
      http.get("*/api/gates", pending),
    );
    renderApp("/overview");
    await screen.findByRole("main");
    const readiness = await within(main()).findByRole("region", { name: "Readiness" });
    expect(within(readiness).getByRole("status", { name: "Loading" })).toBeTruthy();
    for (const name of ["Pipeline", "Recent decisions", "Quality gates", "At a glance"]) {
      expect(within(main()).getByRole("region", { name }).querySelectorAll("[data-skeleton]").length).toBeGreaterThan(0);
    }
  });

  it("is a calm success when every step is done and every gate passes: the next action is Export", async () => {
    serve(backend({ pipeline: ALL_DONE, gates: ALL_PASS, proposals: [], questions: 0 }));
    const { user } = renderApp("/overview");
    await screen.findByRole("main");
    const card = await within(main()).findByRole("region", { name: "Next action" });
    expect(await within(card).findByText("Your specification is ready")).toBeTruthy();
    const pipeline = within(main()).getByRole("region", { name: "Pipeline" });
    await waitFor(() => expect(within(pipeline).getAllByRole("link").every((s) => s.getAttribute("data-step-state") === "done")).toBe(true));
    await user.click(within(card).getByRole("button", { name: "Export" }));
    expect(await screen.findByRole("dialog", { name: "Export specification" })).toBeTruthy();
  });

  it("shows a running step on the stepper and the card, with progress", async () => {
    const b = serve(
      backend({
        pipeline: { ...ALL_DONE, next: "flows" },
        gates: ALL_PASS,
        proposals: [],
        questions: 0,
        jobs: [{ status: "evaluating", progress: 0.4, base_revision: VERSION.revision }],
      }),
    );
    const { user } = renderApp("/overview");
    await screen.findByRole("main");
    const card = await within(main()).findByRole("region", { name: "Next action" });
    await user.click(await within(card).findByRole("button", { name: "Run step" }));
    await waitFor(() => expect(b.runs).toEqual(["flows"]));
    expect(await within(card).findByText("Plumb is running the Flows step… 40%", undefined, { timeout: 3000 })).toBeTruthy();
    expect(within(card).getByRole("progressbar").getAttribute("aria-valuenow")).toBe("40");
    const flows = within(within(main()).getByRole("region", { name: "Pipeline" })).getAllByRole("link")[4];
    expect(flows?.getAttribute("data-step-state")).toBe("running");
    expect(flows?.textContent).toContain("40%");
    // "View details" opens the run menu.
    await user.click(within(card).getByRole("button", { name: "View details" }));
    expect(await screen.findByRole("menu")).toBeTruthy();
  });

  it("offers Retry for a failed step, retrying its job", async () => {
    const failed = { ...ALL_DONE, steps: ALL_DONE.steps.map((s) => (s.recipe === "flows" ? wire("flows", "failed", { job_id: "job:f", error: { code: "E_STAGE_EVALUATION_FAILED", message: "The model could not be reached." } }) : s)) };
    const b = serve(backend({ pipeline: failed }));
    const { user } = renderApp("/overview");
    await screen.findByRole("main");
    const card = await within(main()).findByRole("region", { name: "Next action" });
    expect(await within(card).findByText("Flows failed — Retry")).toBeTruthy();
    expect(within(card).getByText("The model could not be reached.")).toBeTruthy();
    const flows = within(within(main()).getByRole("region", { name: "Pipeline" })).getAllByRole("link")[4];
    expect(flows?.getAttribute("data-step-state")).toBe("failed");
    expect(flows?.textContent).toContain("Failed");
    await user.click(within(card).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(b.retries).toEqual(["job:f"]));
  });

  it("states quietly when AI is off and opens Settings from the notice", async () => {
    serve(backend({ me: { ...ME, inference_available: false, inference_model: null } }));
    const { user } = renderApp("/overview");
    await screen.findByRole("main");
    const notice = await within(main()).findByText("AI analysis is off. Add an API key in Settings to run AI steps. You can still review, answer and export.");
    await user.click(within(notice.closest("[role='note']") as HTMLElement).getByRole("button", { name: "Open Settings" }));
    expect(await screen.findByRole("dialog", { name: "Settings" })).toBeTruthy();
  });
});

describe("Overview first run", () => {
  const EMPTY: PipelineResponse = { steps: RECIPES.map((r) => wire(r, "not_started")), sources: 0, open_questions: 0, next: "extract" };

  it("is the import hero: drop a document, see progress, then run the Requirements step", async () => {
    const b = serve(backend({ pipeline: EMPTY, sources: [], proposals: [], questions: 0, gates: [] }));
    const { user } = renderApp("/overview");
    await screen.findByRole("main");
    expect(await within(main()).findByRole("heading", { level: 1, name: "Import your requirements document" })).toBeTruthy();
    expect(within(main()).getByText("New project")).toBeTruthy();
    expect(await axeViolations(main())).toEqual([]);
    const input = within(main()).getByLabelText("Drop a file here, or choose one");
    b.upload = () => {
      // The import made a source: the pipeline now reports it.
      b.pipeline = { ...EMPTY, sources: 1 };
      return etagJson({ id: "src:2", hash: "sha256:2", spans_count: 38, events: [], revision: VERSION.revision, semantic_hash: VERSION.semantic_hash, version: VERSION.version });
    };
    await user.upload(input, new File(["# Leave\n\nAn employee shall request leave.\n"], "requirements.md"));
    await waitFor(() => expect(b.uploads).toEqual([{ name: "requirements.md", type: "text/markdown", ifMatch: VERSION_ETAG }]));
    expect(await screen.findByText("38 passages imported from requirements.md")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Run Requirements step" }));
    await waitFor(() => expect(b.runs).toEqual(["extract"]));
    // The project now has a source: the dashboard replaces the hero.
    expect(await within(main()).findByRole("heading", { level: 1, name: "HR leave" })).toBeTruthy();
  });

  it("explains a rejected document in plain language", async () => {
    const b = serve(
      backend({
        pipeline: EMPTY,
        sources: [],
        proposals: [],
        questions: 0,
        gates: [],
        upload: () => HttpResponse.json({ code: "E_SOURCE_EXISTS", message: "exists", details: {} }, { status: 409 }),
      }),
    );
    const { user } = renderApp("/overview");
    await screen.findByRole("main");
    const input = await within(main()).findByLabelText("Drop a file here, or choose one");
    await user.upload(input, new File(["x"], "requirements.md"));
    expect(await within(main()).findByText("This file is already imported")).toBeTruthy();
    expect(within(main()).getByText("E_SOURCE_EXISTS")).toBeTruthy();
    // An unsupported type is refused before any upload.
    fireEvent.change(input, { target: { files: [new File(["%PDF"], "policy.pdf", { type: "application/pdf" })] } });
    expect(await within(main()).findByText("Unsupported file type")).toBeTruthy();
    expect(b.uploads).toHaveLength(1);
    void app;
  });
});
