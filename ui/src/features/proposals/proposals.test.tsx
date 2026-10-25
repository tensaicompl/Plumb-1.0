// H0.11 Proposals screen tests (architecture §33.12 proposal discovery and governance): every
// pending workflow proposal from GET /api/proposals with its exact server fields; the one positive
// action of its server class, reject and (answer and intake proposals only) discard, each sent with
// the ETag of the reviewed list; a stale proposal is never offered its positive action (the backend
// refuses it, nothing is rebased); a non-analyst is offered no write control. Every HTTP call is
// answered by MSW and recorded; nothing touches the network.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http } from "msw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { etagJson, server, TEST_ETAG } from "../../test/msw";

type PendingProposal = components["schemas"]["PendingProposal"];

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

const ANSWER: PendingProposal = {
  proposal_id: "prop:answer-1",
  action: "create",
  origin: "question",
  origin_ref: "q:performer",
  recipe_job_id: null,
  stage_job_id: null,
  base_revision: "rev:1:0000000000000000",
  requires_reason: true,
  stale: false,
  discardable: true,
  title: "Proposal",
  element_kind: "other",
  created_at: "2026-05-01T12:00:00.000000000Z",
  stage: null,
};

const STAGE: PendingProposal = {
  proposal_id: "prop:stage-1",
  action: "accept",
  origin: "stage",
  origin_ref: null,
  recipe_job_id: "sha256:recipe",
  stage_job_id: "sha256:stage",
  base_revision: "rev:1:0000000000000000",
  requires_reason: false,
  stale: false,
  discardable: false,
  title: "Proposal",
  element_kind: "other",
  created_at: "2026-05-01T12:00:00.000000000Z",
  stage: null,
};

const STALE: PendingProposal = {
  proposal_id: "prop:stale-1",
  action: "link",
  origin: "intake",
  origin_ref: null,
  recipe_job_id: null,
  stage_job_id: null,
  base_revision: "rev:0:ffffffffffffffff",
  requires_reason: false,
  stale: true,
  discardable: true,
  title: "Proposal",
  element_kind: "other",
  created_at: "2026-05-01T12:00:00.000000000Z",
  stage: null,
};

const WRITE = { version: "rev:1:0000000000000000", revision: "rev:1:0000000000000000", semantic_hash: CURRENT, events: [] };

interface Call {
  readonly method: string;
  readonly path: string;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

function handlers(
  calls: Call[],
  options: { items?: PendingProposal[]; roles?: string[]; action?: () => Response } = {},
) {
  const record = async (request: Request, method: string) => {
    const url = new URL(request.url);
    const text = method === "GET" ? "" : await request.text();
    calls.push({
      method,
      path: decodeURIComponent(url.pathname),
      ifMatch: request.headers.get("if-match"),
      body: text === "" ? null : (JSON.parse(text) as unknown),
    });
  };
  return [
    http.get("*/api/me", () => etagJson({ user: "alice", mode: "pilot", roles: options.roles ?? ["analyst"] })),
    http.get("*/api/model/version", () =>
      etagJson({ version: "rev:1", revision: "rev:1", hash: CURRENT, semantic_hash: CURRENT, evidence_hash: "ev:1" }),
    ),
    http.get("*/api/proposals", async ({ request }) => {
      await record(request, "GET");
      return etagJson({ items: options.items ?? [ANSWER, STAGE, STALE] });
    }),
    ...(["accept", "create", "link", "merge", "supersede", "reject", "discard"] as const).map((verb) =>
      http.post("*/api/proposals/:id/" + verb, async ({ request }) => {
        await record(request, "POST");
        return options.action !== undefined ? options.action() : etagJson(WRITE);
      }),
    ),
  ];
}

function renderProposals() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: ["/proposals"] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

const posts = (calls: Call[]) => calls.filter((c) => c.method === "POST");
const row = (id: string) => {
  const element = document.querySelector("tr[data-proposal-id='" + id + "']");
  if (!(element instanceof HTMLElement)) {
    throw new Error("proposal row " + id + " expected");
  }
  return element;
};
const field = (id: string, name: string) => row(id).querySelector("[data-field=" + name + "]")?.textContent;

// ------------------------------------------------------------------ tests

describe("ProposalsPage", () => {
  it("lists every pending proposal with its exact server fields in an accessible table", async () => {
    server.use(...handlers([]));
    renderProposals();
    expect(await screen.findByText("prop:answer-1")).toBeTruthy();
    expect(screen.getAllByRole("heading", { level: 1 }).map((h) => h.textContent)).toEqual(["Proposals"]);
    const table = document.querySelector("main table") as HTMLElement;
    expect(table.querySelector("caption")?.textContent).toBe("Pending proposals");
    expect([...table.querySelectorAll("th")].every((th) => th.getAttribute("scope") === "col")).toBe(true);
    expect([...document.querySelectorAll("tr[data-proposal-id]")].map((r) => r.getAttribute("data-proposal-id"))).toEqual([
      "prop:answer-1",
      "prop:stage-1",
      "prop:stale-1",
    ]);
    expect(field("prop:answer-1", "action")).toBe("create");
    expect(field("prop:answer-1", "origin")).toBe("question q:performer");
    expect(field("prop:stage-1", "origin")).toBe("stage sha256:recipe sha256:stage");
    expect(field("prop:answer-1", "base_revision")).toBe("rev:1:0000000000000000");
    expect(field("prop:answer-1", "state")).toBe("Current");
    expect(field("prop:stale-1", "state")).toBe("Stale");
    expect(field("prop:answer-1", "requires_reason")).toBe("Yes");
    expect(field("prop:stage-1", "requires_reason")).toBe("No");
  });

  it("commits through the one action of the server class with the reviewed list ETag and a required reason", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderProposals();
    const user = userEvent.setup();
    await screen.findByText("prop:answer-1");
    const answer = within(row("prop:answer-1"));
    const create = answer.getByRole("button", { name: "Create" }) as HTMLButtonElement;
    // Intake requires a reason: the positive action waits for one.
    expect(create.disabled).toBe(true);
    await user.type(answer.getByLabelText("Reason"), "HR confirmed.");
    expect(create.disabled).toBe(false);
    await user.click(create);
    await waitFor(() => expect(posts(calls)).toHaveLength(1));
    expect(posts(calls).map((c) => [c.path, c.ifMatch, c.body])).toEqual([
      ["/api/proposals/prop:answer-1/create", TEST_ETAG, { reason: "HR confirmed." }],
    ]);
    // The list is read again after the write.
    await waitFor(() => expect(calls.filter((c) => c.method === "GET" && c.path === "/api/proposals").length).toBeGreaterThan(1));
    // Only the action of the server class is offered.
    expect(within(row("prop:stage-1")).getByRole("button", { name: "Accept" })).toBeTruthy();
    expect(within(row("prop:stage-1")).queryByRole("button", { name: "Create" })).toBeNull();
  });

  it("does not offer the positive action of a stale proposal, which can only be rejected or discarded", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderProposals();
    const user = userEvent.setup();
    await screen.findByText("prop:stale-1");
    const stale = within(row("prop:stale-1"));
    expect((stale.getByRole("button", { name: "Link" }) as HTMLButtonElement).disabled).toBe(true);
    expect(stale.getByTestId("proposal-stale-note").textContent).toBe(
      "Built on an older revision: it is never rebased. Reject or discard it, or rerun what produced it.",
    );
    await user.click(stale.getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(posts(calls)).toHaveLength(1));
    expect(posts(calls).map((c) => [c.path, c.ifMatch, c.body])).toEqual([["/api/proposals/prop:stale-1/discard", TEST_ETAG, { reason: null }]]);
  });

  it("offers discard only for answer and intake proposals and reject for every one", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderProposals();
    const user = userEvent.setup();
    await screen.findByText("prop:stage-1");
    expect(within(row("prop:stage-1")).queryByRole("button", { name: "Discard" })).toBeNull();
    expect(within(row("prop:answer-1")).getByRole("button", { name: "Discard" })).toBeTruthy();
    await user.click(within(row("prop:stage-1")).getByRole("button", { name: "Reject" }));
    await waitFor(() => expect(posts(calls)).toHaveLength(1));
    expect(posts(calls).map((c) => [c.path, c.ifMatch, c.body])).toEqual([["/api/proposals/prop:stage-1/reject", TEST_ETAG, { reason: null }]]);
  });

  it("shows the backend typed code and message of a refused action", async () => {
    const calls: Call[] = [];
    server.use(
      ...handlers(calls, {
        action: () => etagJson({ code: "E_PROPOSAL_NOT_PENDING", message: "the proposal is not pending", details: {} }, { status: 409 }),
      }),
    );
    renderProposals();
    const user = userEvent.setup();
    await screen.findByText("prop:stage-1");
    await user.click(within(row("prop:stage-1")).getByRole("button", { name: "Accept" }));
    const alert = await within(row("prop:stage-1")).findByRole("alert");
    expect(alert.textContent).toContain("E_PROPOSAL_NOT_PENDING");
    expect(alert.textContent).toContain("the proposal is not pending");
  });

  it("offers a non-analyst no write control (Fable #218)", async () => {
    server.use(...handlers([], { roles: ["stakeholder"] }));
    renderProposals();
    await screen.findByText("prop:answer-1");
    expect(await screen.findByTestId("role-required")).toBeTruthy();
    expect(document.querySelectorAll("tr[data-proposal-id] button")).toHaveLength(0);
  });

  it("says when nothing is pending", async () => {
    server.use(...handlers([], { items: [] }));
    renderProposals();
    expect(await screen.findByText("No proposal is pending.")).toBeTruthy();
  });
});
