// U2.2 Sentences tests (Hotfix 076): entity sections from the server entities with membership only
// from the server `entity` filter (plus Ungrouped bookkeeping); per-sentence review marking the
// sentence viewed for the page session; the closed class/level/modality edit; direct accept and
// reject; Sentence unknowns resolved to their persisted Open Question and answered through the
// shared Question proposal flow; and the warned, strictly sequential bulk accept whose first write is
// based on the reviewed list ETag and each further write on the previous result's revision, and that
// stops on the first 412. Every HTTP call is answered by MSW and
// recorded; nothing touches the network.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { etagJson, server } from "../../test/msw";

type Sentence = components["schemas"]["Sentence"];
type Entity = components["schemas"]["Entity"];
type Question = components["schemas"]["Question"];

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

const entity = (id: string, name: string): Entity => ({ id, name, attributes: [], relationships: [], states: [], tags: [], transitions: [] });
const ENTITIES: Entity[] = [entity("entity:request", "LeaveRequest"), entity("entity:approval", "ApprovalRecord")];

const sentence = (id: string, text: string, status: string, unknowns: Sentence["unknowns"] = []): Sentence => ({
  id,
  text,
  status,
  slots: { class: "functional", level: "system", modality: "shall" },
  evidence: ["frag:" + id],
  unknowns,
});

const S1 = sentence("req:s1", "Employees shall submit leave requests.", "Proposed", [{ slot: "performer", question_id: "q:performer" }]);
const S2 = sentence("req:s2", "Managers shall approve leave requests.", "Proposed");
const S3 = sentence("req:s3", "The system shall store approval records.", "Accepted");
// Mentions an entity name in its text, but no entity filter returns it: it is Ungrouped.
const S4 = sentence("req:s4", "The ApprovalRecord shall be immutable.", "Proposed", [{ slot: "owner", question_id: "q:missing" }]);
const S5 = sentence("req:s5", "Employees shall cancel leave requests.", "Proposed");

const ALL = [S1, S2, S3, S4, S5];
const BY_ENTITY: Record<string, Sentence[]> = {
  "entity:request": [S1, S2, S5],
  "entity:approval": [S2, S3],
};

const Q_PERFORMER: Question = {
  id: "q:performer",
  text: "Who submits leave requests?",
  kind: "RoleAssignment",
  status: "Open",
  stakeholder_ref: "stakeholder:hr",
  priority: 1,
  round_ref: null,
  context_refs: ["req:s1"],
  answer_schema: {
    type: "object",
    properties: { performer_ref: { type: "string", enum: ["actor:employee", "actor:clerk"] } },
    required: ["performer_ref"],
    additionalProperties: false,
  } as unknown as Record<string, never>,
};

interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

/** The fake backend: a semantic head that advances on every successful accept. */
interface Backend {
  calls: Call[];
  head: number;
  versionReads: number;
  hold: Promise<void> | null;
  conflictOn: string | null;
  /** Sentences another writer changed on the server. */
  changed?: Record<string, Sentence>;
}

const hash = (n: number) => "psg:sha256:v" + String(n);
/** The ETag of head revision `n` (the strong entity-tag of its revision ID). */
const tag = (n: number) => '"rev:' + String(n) + '"';

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
  const write = () => {
    backend.head += 1;
    return HttpResponse.json({ version: "rev:" + String(backend.head), revision: "rev:" + String(backend.head), semantic_hash: hash(backend.head), events: [] });
  };
  return [
    http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles: ["analyst"] })),
    http.get("*/api/model/version", () => {
      backend.versionReads += 1;
      return HttpResponse.json({
        version: "rev:" + String(backend.head),
        revision: "rev:" + String(backend.head),
        hash: hash(backend.head),
        semantic_hash: hash(backend.head),
        evidence_hash: "ev:1",
      });
    }),
    http.get("*/api/model/entities", async ({ request }) => {
      await record(request, "GET");
      return etagJson({ items: ENTITIES, next_cursor: null }, undefined, tag(backend.head));
    }),
    http.get("*/api/model/sentences", async ({ request }) => {
      await record(request, "GET");
      const entityId = new URL(request.url).searchParams.get("entity");
      const items = (entityId === null ? ALL : (BY_ENTITY[entityId] ?? [])).map((s) => backend.changed?.[s.id] ?? s);
      return etagJson({ items, next_cursor: null }, undefined, tag(backend.head));
    }),
    http.get("*/api/questions", async ({ request }) => {
      await record(request, "GET");
      return etagJson({ items: [Q_PERFORMER], next_cursor: null }, undefined, tag(backend.head));
    }),
    http.post("*/api/model/sentences/:id/accept", async ({ request, params }) => {
      await record(request, "POST");
      if (backend.hold !== null) {
        const hold = backend.hold;
        backend.hold = null;
        await hold;
      }
      if (params["id"] === backend.conflictOn || request.headers.get("if-match") !== tag(backend.head)) {
        return HttpResponse.json(
          { code: "E_PRECONDITION_FAILED", message: "moved", details: {}, revision: "rev:99", semantic_hash: "psg:sha256:other" },
          { status: 412 },
        );
      }
      return write();
    }),
    http.post("*/api/model/sentences/:id/reject", async ({ request }) => {
      await record(request, "POST");
      return write();
    }),
    http.post("*/api/model/sentences/:id/edit", async ({ request }) => {
      await record(request, "POST");
      return write();
    }),
    http.post("*/api/questions/:id/answer", async ({ request }) => {
      await record(request, "POST");
      return HttpResponse.json({
        version: "rev:0",
        revision: "rev:0",
        semantic_hash: hash(backend.head),
        events: [],
        proposal_id: "prop:unknown-1",
        action: "create",
        intake: {
          covered_by: [],
          conflicts_with: [],
          refines: [],
          new_terms: [],
          novelty: null,
          duplicate_candidates: [],
          validation_findings: [],
          reason_codes: [],
          requires_reason: false,
        },
        patch_preview: { op: "Compound", patches: [] },
      });
    }),
    http.post("*/api/proposals/:id/create", async ({ request }) => {
      await record(request, "POST");
      return write();
    }),
  ];
}

let lastClient: QueryClient | null = null;

function newBackend(): Backend {
  return { calls: [], head: 0, versionReads: 0, hold: null, conflictOn: null };
}

function renderSentences() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  lastClient = queryClient;
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: ["/model/sentences"] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

const section = (id: string) => {
  const element = document.querySelector("section[data-section='" + id + "']");
  if (!(element instanceof HTMLElement)) {
    throw new Error("section " + id + " expected");
  }
  return element;
};
const idsIn = (id: string) => Array.from(section(id).querySelectorAll("li[data-sentence-id]")).map((li) => li.getAttribute("data-sentence-id"));
const item = (sectionId: string, id: string) => section(sectionId).querySelector("li[data-sentence-id='" + id + "']") as HTMLElement;
const accepts = (backend: Backend) => backend.calls.filter((c) => c.method === "POST" && c.path.endsWith("/accept"));

async function loaded() {
  await screen.findByRole("heading", { name: "Sentences" });
  await waitFor(() => expect(document.querySelector("section[data-section='entity:request']")).not.toBeNull());
}

// ------------------------------------------------------------------ grouping and review

describe("SentencesPage grouping", () => {
  it("builds sections from server entities and membership only from the server entity filter", async () => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderSentences();
    await loaded();

    expect(Array.from(document.querySelectorAll("section[data-section]")).map((s) => s.getAttribute("data-section"))).toEqual([
      "entity:request",
      "entity:approval",
      "ungrouped",
    ]);
    expect(within(section("entity:request")).getByRole("heading", { level: 2 }).textContent).toContain("LeaveRequest");
    // Exact server membership; req:s2 is returned for both entities and appears in both.
    expect(idsIn("entity:request")).toEqual(["req:s1", "req:s2", "req:s5"]);
    expect(idsIn("entity:approval")).toEqual(["req:s2", "req:s3"]);
    // req:s4 names ApprovalRecord in its text but no entity filter returns it: Ungrouped.
    expect(idsIn("ungrouped")).toEqual(["req:s4"]);
    const filters = backend.calls.filter((c) => c.path === "/api/model/sentences").map((c) => c.search.get("entity"));
    expect(filters.sort()).toEqual([null, "entity:approval", "entity:request"].sort());
    expect(backend.calls.filter((c) => c.path === "/api/model/sentences").every((c) => c.search.get("limit") === null)).toBe(true);
    expect(backend.calls.find((c) => c.path === "/api/questions")?.search.get("status")).toBe("Open");
  });

  it("marks a sentence viewed when its review opens, for the page session", async () => {
    server.use(...handlers(newBackend()));
    renderSentences();
    await loaded();
    const user = userEvent.setup();
    const row = item("entity:request", "req:s2");
    expect(row.getAttribute("data-viewed")).toBe("false");
    await user.click(within(row).getByTestId("sentence-review-toggle"));
    expect(item("entity:request", "req:s2").getAttribute("data-viewed")).toBe("true");
    // The same sentence in another section is viewed too, and closing keeps it viewed.
    expect(item("entity:approval", "req:s2").getAttribute("data-viewed")).toBe("true");
    await user.click(within(item("entity:request", "req:s2")).getByTestId("sentence-review-toggle"));
    expect(item("entity:request", "req:s2").getAttribute("data-viewed")).toBe("true");
  });

  it("edits only class, level and modality from the closed vocabularies, sending only changes", async () => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderSentences();
    await loaded();
    const user = userEvent.setup();
    const row = item("entity:approval", "req:s3");
    await user.click(within(row).getByTestId("sentence-review-toggle"));
    const review = within(row).getByTestId("sentence-review");
    const options = (slot: string) => Array.from(review.querySelectorAll("select[data-slot=" + slot + "] option")).map((o) => o.getAttribute("value"));
    expect(options("class")).toEqual(["functional", "quality", "interface", "data", "security", "operational", "compliance", "transition", "constraint"]);
    expect(options("level")).toEqual(["stakeholder", "system", "software", "subsystem", "component", "interface"]);
    expect(options("modality")).toEqual(["shall", "should", "may", "shall_not"]);
    // The sentence text is not editable.
    expect(Array.from(review.querySelectorAll("input, textarea")).some((e) => (e as HTMLInputElement).value === S3.text)).toBe(false);
    const save = within(review).getByTestId("sentence-save") as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    await user.selectOptions(review.querySelector("select[data-slot=class]") as HTMLSelectElement, "quality");
    await user.click(save);
    await waitFor(() => expect(backend.calls.filter((c) => c.path.endsWith("/edit"))).toHaveLength(1));
    expect(backend.calls.filter((c) => c.path.endsWith("/edit")).map((c) => [c.path, c.ifMatch, c.body])).toEqual([
      ["/api/model/sentences/req:s3/edit", tag(0), { slots: { class: "quality" } }],
    ]);
  });

  it("resets the slot form when the reviewed sentence changes on the server, so a save never mixes versions (Fable #221)", async () => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderSentences();
    await loaded();
    const user = userEvent.setup();
    const row = () => item("entity:approval", "req:s3");
    await user.click(within(row()).getByTestId("sentence-review-toggle"));
    const select = (slot: string) => within(row()).getByTestId("sentence-review").querySelector("select[data-slot=" + slot + "]") as HTMLSelectElement;
    await user.selectOptions(select("class"), "quality");
    expect(select("class").value).toBe("quality");
    // Another writer changes the level; the list refetches with the new head.
    backend.head += 1;
    backend.changed = { "req:s3": { ...S3, slots: { ...S3.slots, level: "software" } } };
    await act(async () => {
      await lastClient?.invalidateQueries();
    });
    await waitFor(() => expect(select("level").value).toBe("software"));
    // The unsaved local change was dropped with the old version: nothing stale can be sent.
    expect(select("class").value).toBe("functional");
    expect((within(row()).getByTestId("sentence-save") as HTMLButtonElement).disabled).toBe(true);
  });

  it("accepts and rejects a single Proposed sentence through its endpoints with If-Match", async () => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderSentences();
    await loaded();
    const user = userEvent.setup();
    await user.click(within(item("entity:request", "req:s1")).getByTestId("sentence-review-toggle"));
    await user.click(within(item("entity:request", "req:s1")).getByTestId("sentence-accept"));
    await waitFor(() => expect(accepts(backend)).toHaveLength(1));
    await user.click(within(item("entity:request", "req:s5")).getByTestId("sentence-review-toggle"));
    await user.click(within(item("entity:request", "req:s5")).getByTestId("sentence-reject"));
    await waitFor(() => expect(backend.calls.filter((c) => c.path.endsWith("/reject"))).toHaveLength(1));
    expect(backend.calls.filter((c) => c.method === "POST").map((c) => [c.path, c.ifMatch])).toEqual([
      ["/api/model/sentences/req:s1/accept", tag(0)],
      ["/api/model/sentences/req:s5/reject", tag(1)],
    ]);
    // Accepted sentences offer no accept/reject.
    await user.click(within(item("entity:approval", "req:s3")).getByTestId("sentence-review-toggle"));
    expect(within(item("entity:approval", "req:s3")).queryByTestId("sentence-accept")).toBeNull();
  });
});

// ------------------------------------------------------------------ unknowns

describe("Sentence unknowns", () => {
  it("answers an unknown through its persisted Open Question with the shared proposal flow", async () => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderSentences();
    await loaded();
    const user = userEvent.setup();
    const row = item("entity:request", "req:s1");
    await user.click(within(row).getByTestId("sentence-review-toggle"));
    const unknown = row.querySelector("[data-unknown='q:performer']") as HTMLElement;
    expect(unknown.textContent).toContain("performer");
    expect(unknown.textContent).toContain(Q_PERFORMER.text);
    // The typed control comes from the Question's kind and answer_schema, not the slot.
    expect(unknown.querySelector("[data-renderer]")?.getAttribute("data-renderer")).toBe("role-assignment");
    await user.selectOptions(unknown.querySelector("[data-field=performer_ref] select") as HTMLSelectElement, "actor:clerk");
    await user.click(within(unknown).getByTestId("answer-submit"));
    const preview = await within(unknown).findByTestId("answer-preview");
    expect(within(preview).getByTestId("answer-action").textContent).toBe("create");
    await user.click(preview.querySelector("button[data-action=create]") as HTMLButtonElement);
    await waitFor(() => expect(backend.calls.filter((c) => c.path === "/api/proposals/prop:unknown-1/create")).toHaveLength(1));
    expect(backend.calls.filter((c) => c.method === "POST").map((c) => c.path)).toEqual([
      "/api/questions/q:performer/answer",
      "/api/proposals/prop:unknown-1/create",
    ]);
  });

  it("shows an unknown whose Question is not an Open server Question as unavailable", async () => {
    server.use(...handlers(newBackend()));
    renderSentences();
    await loaded();
    const user = userEvent.setup();
    const row = item("ungrouped", "req:s4");
    await user.click(within(row).getByTestId("sentence-review-toggle"));
    expect(within(row).getByTestId("unknown-unavailable").textContent).toContain("q:missing");
    expect(row.querySelector("[data-renderer]")).toBeNull();
  });
});

// ------------------------------------------------------------------ bulk accept

describe("Sentence bulk accept", () => {
  it("warns about unviewed targets before any write and sends nothing on cancel", async () => {
    const backend = newBackend();
    server.use(...handlers(backend));
    renderSentences();
    await loaded();
    const user = userEvent.setup();
    // Review one of the three Proposed targets; two remain unviewed.
    await user.click(within(item("entity:request", "req:s1")).getByTestId("sentence-review-toggle"));
    await user.click(within(section("entity:request")).getByTestId("bulk-accept"));
    const warning = within(section("entity:request")).getByTestId("bulk-warning");
    expect(within(warning).getByTestId("bulk-unviewed").textContent).toBe("2");
    expect(accepts(backend)).toEqual([]);
    await user.click(within(warning).getByTestId("bulk-warning-cancel"));
    expect(within(section("entity:request")).queryByTestId("bulk-warning")).toBeNull();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(accepts(backend)).toEqual([]);
  });

  it("accepts strictly one by one, each write on the revision the previous accept produced, never fetching a version", async () => {
    const backend = newBackend();
    let release: () => void = () => undefined;
    backend.hold = new Promise<void>((resolve) => {
      release = resolve;
    });
    server.use(...handlers(backend));
    renderSentences();
    await loaded();
    const user = userEvent.setup();
    await user.click(within(section("entity:request")).getByTestId("bulk-accept"));
    await user.click(within(section("entity:request")).getByTestId("bulk-warning-confirm"));

    // While the first accept is held open, the second is not sent.
    await waitFor(() => expect(accepts(backend)).toHaveLength(1));
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(accepts(backend)).toHaveLength(1);
    const readsBeforeRelease = backend.versionReads;
    release();

    await waitFor(() => expect(within(section("entity:request")).getByTestId("bulk-progress").getAttribute("data-state")).toBe("done"));
    // The first write used the reviewed section's ETag, each further one the revision the
    // previous accept returned; no version was fetched to make a precondition (§33.2).
    expect(accepts(backend).map((c) => [c.path, c.ifMatch])).toEqual([
      ["/api/model/sentences/req:s1/accept", tag(0)],
      ["/api/model/sentences/req:s2/accept", tag(1)],
      ["/api/model/sentences/req:s5/accept", tag(2)],
    ]);
    expect(backend.versionReads - readsBeforeRelease).toBe(0);
    expect(within(section("entity:request")).getByTestId("bulk-accepted").textContent).toBe("3");
    expect(within(section("entity:request")).getByTestId("bulk-remaining").textContent).toBe("0");
  });

  it("stops on the first 412 without posting later targets and shows partial counts", async () => {
    const backend = newBackend();
    backend.conflictOn = "req:s2";
    server.use(...handlers(backend));
    renderSentences();
    await loaded();
    const user = userEvent.setup();
    for (const id of ["req:s1", "req:s2", "req:s5"]) {
      await user.click(within(item("entity:request", id)).getByTestId("sentence-review-toggle"));
    }
    // All targets were viewed, so bulk acceptance starts without a warning.
    await user.click(within(section("entity:request")).getByTestId("bulk-accept"));
    expect(within(section("entity:request")).queryByTestId("bulk-warning")).toBeNull();

    await waitFor(() => expect(within(section("entity:request")).getByTestId("bulk-progress").getAttribute("data-state")).toBe("conflict"));
    expect(accepts(backend).map((c) => c.path)).toEqual(["/api/model/sentences/req:s1/accept", "/api/model/sentences/req:s2/accept"]);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(accepts(backend)).toHaveLength(2);
    expect(within(section("entity:request")).getByTestId("bulk-accepted").textContent).toBe("1");
    expect(within(section("entity:request")).getByTestId("bulk-remaining").textContent).toBe("2");
    expect(within(section("entity:request")).getByTestId("bulk-conflict")).toBeTruthy();
    // The global 412 behaviour of the versioned mutation is unchanged.
    expect(mutation.useConflictStore.getState().conflict).not.toBeNull();
  });
});
