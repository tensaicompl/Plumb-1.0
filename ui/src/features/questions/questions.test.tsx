// U2.2 Questions tests (Hotfix 076): the stakeholder inbox with exact server fields; typed answers
// from Question.kind + answer_schema; the answer write (generated client, If-Match, supersedes null,
// answer rationale only) returning an uncommitted pending proposal; confirmation only through the
// endpoint of the server-returned action (or reject) with the review-bar reason; stale-baseline
// handling; and the round manager (explicit integer max 1..=15, server composition). Every HTTP call
// is answered by MSW and recorded; nothing touches the network.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { server } from "../../test/msw";

type Question = components["schemas"]["Question"];
type AnswerResponse = components["schemas"]["AnswerResponse"];
type ProposalKind = components["schemas"]["ProposalKind"];

let routerModule: typeof import("../../app/router");
let mutation: typeof import("../../api/useVersionedMutation");
let questions: typeof import("./QuestionsPage");

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
  questions = await import("./QuestionsPage");
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

const PERFORMER_SCHEMA = {
  type: "object",
  properties: { performer_ref: { type: "string", enum: ["actor:clerk", "actor:manager"] } },
  required: ["performer_ref"],
  additionalProperties: false,
};

const DAYS_SCHEMA = {
  type: "object",
  properties: { days: { type: "integer", minimum: 1, maximum: 30 } },
  required: ["days"],
  additionalProperties: false,
};

const Q_PERFORMER: Question = {
  id: "q:performer",
  text: "Who approves leave requests?",
  kind: "RoleAssignment",
  status: "Open",
  stakeholder_ref: "stakeholder:hr",
  priority: 10,
  round_ref: null,
  context_refs: ["op:approve", "req:hr-003"],
  answer_schema: PERFORMER_SCHEMA as unknown as Record<string, never>,
};

const Q_DAYS: Question = {
  id: "q:days",
  text: "How many days of notice are required?",
  kind: "Number",
  status: "Open",
  stakeholder_ref: "stakeholder:hr",
  priority: 5,
  round_ref: "round:existing",
  context_refs: [],
  answer_schema: DAYS_SCHEMA as unknown as Record<string, never>,
};

const Q_ANSWERED: Question = {
  id: "q:answered",
  text: "Is half-day leave allowed?",
  kind: "YesNo",
  status: "Answered",
  stakeholder_ref: "stakeholder:finance",
  priority: null,
  round_ref: null,
  context_refs: ["term:leave"],
  answer_schema: null,
};

const Q_UNASSIGNED: Question = {
  id: "q:nobody",
  text: "Which calendar applies?",
  kind: "Calendar",
  status: "Answered",
  stakeholder_ref: null,
  priority: null,
  round_ref: null,
  context_refs: [],
  answer_schema: null,
};

const PATCH_PREVIEW = { op: "Compound", patches: [{ op: "AddNode", node: { id: "decision:1" } }, { op: "AddEdge", edge: { id: "rel:performed-by" } }] };

function answerResponse(action: ProposalKind): AnswerResponse {
  return {
    version: "rev:7",
    revision: "rev:7",
    semantic_hash: CURRENT,
    events: [],
    proposal_id: "prop:answer-1",
    action,
    intake: {
      covered_by: [],
      conflicts_with: [],
      refines: [],
      new_terms: [],
      novelty: null,
      duplicate_candidates: [],
      validation_findings: [],
      reason_codes: ["human_decision"],
      requires_reason: false,
    },
    patch_preview: PATCH_PREVIEW as unknown as Record<string, never>,
  };
}

interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

const WRITE = { version: "rev:8", revision: "rev:8", semantic_hash: "psg:sha256:after", events: [] };

function handlers(
  calls: Call[],
  options: { action?: ProposalKind; confirm?: () => Response; items?: Question[] } = {},
) {
  const record = async (request: Request, method: string) => {
    const url = new URL(request.url);
    const text = method === "GET" ? "" : await request.text();
    calls.push({
      method,
      path: decodeURIComponent(url.pathname),
      search: url.searchParams,
      ifMatch: request.headers.get("if-match"),
      body: text === "" ? null : (JSON.parse(text) as unknown),
    });
  };
  const action = options.action ?? "create";
  return [
    http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles: ["analyst"] })),
    http.get("*/api/model/version", () =>
      HttpResponse.json({ version: "rev:7", revision: "rev:7", hash: CURRENT, semantic_hash: CURRENT, evidence_hash: "ev:1" }),
    ),
    http.get("*/api/questions", async ({ request }) => {
      await record(request, "GET");
      return HttpResponse.json({ items: options.items ?? [Q_PERFORMER, Q_DAYS, Q_ANSWERED, Q_UNASSIGNED], next_cursor: null });
    }),
    http.post("*/api/questions/:id/answer", async ({ request }) => {
      await record(request, "POST");
      return HttpResponse.json(answerResponse(action));
    }),
    ...(["accept", "create", "link", "merge", "supersede", "reject"] as const).map((verb) =>
      http.post("*/api/proposals/:id/" + verb, async ({ request }) => {
        await record(request, "POST");
        return verb !== "reject" && options.confirm !== undefined ? options.confirm() : HttpResponse.json(WRITE);
      }),
    ),
    http.post("*/api/rounds", async ({ request }) => {
      await record(request, "POST");
      const body = (calls.at(-1)?.body ?? {}) as { stakeholder: string; max: number };
      return HttpResponse.json({
        ...WRITE,
        round: { id: "round:server-1", stakeholder: body.stakeholder, questions: ["q:days", "q:performer"] },
      });
    }),
  ];
}

function renderQuestions() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: ["/questions"] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

const posts = (calls: Call[], path: string) => calls.filter((c) => c.method === "POST" && c.path === path);
const card = (id: string) => {
  const element = document.querySelector("[data-question-card='" + id + "']");
  if (!(element instanceof HTMLElement)) {
    throw new Error("question card " + id + " expected");
  }
  return element;
};

async function answerPerformer(user: ReturnType<typeof userEvent.setup>) {
  const performer = card("q:performer");
  const select = performer.querySelector("[data-field=performer_ref] select");
  if (!(select instanceof HTMLSelectElement)) {
    throw new Error("performer select expected");
  }
  await user.selectOptions(select, "actor:clerk");
  await user.type(within(performer).getByTestId("answer-rationale"), "Clerks approve.");
  await user.click(within(performer).getByTestId("answer-submit"));
  return within(performer).findByTestId("answer-preview");
}

// ------------------------------------------------------------------ inbox

describe("QuestionsPage inbox", () => {
  it("renders exact server fields grouped by exact stakeholder_ref with an unassigned group", async () => {
    server.use(...handlers([]));
    renderQuestions();
    await screen.findByText(Q_PERFORMER.text);

    const groups = Array.from(document.querySelectorAll("section[data-stakeholder]")).map((s) => [
      s.getAttribute("data-stakeholder"),
      Array.from(s.querySelectorAll("li[data-question-id]")).map((li) => li.getAttribute("data-question-id")),
    ]);
    expect(groups).toEqual([
      ["stakeholder:hr", ["q:performer", "q:days"]],
      ["stakeholder:finance", ["q:answered"]],
      ["", ["q:nobody"]],
    ]);
    expect(screen.getByRole("region", { name: "Unassigned" })).toBeTruthy();

    const item = document.querySelector("li[data-question-id='q:performer']");
    expect(item?.querySelector("[data-field=kind]")?.textContent).toBe("RoleAssignment");
    expect(item?.querySelector("[data-field=status]")?.textContent).toBe("Open");
    expect(item?.querySelector("[data-field=priority]")?.textContent).toBe("10");
    expect(item?.querySelector("[data-field=round]")?.textContent).toBe("None");
    // Context refs are exact IDs, shown as text without invented descriptions or links.
    const context = item?.querySelector("[data-field=context]");
    expect(Array.from(context?.querySelectorAll("li") ?? []).map((li) => li.textContent)).toEqual(Q_PERFORMER.context_refs);
    expect(context?.querySelector("a")).toBeNull();
    expect(document.querySelector("li[data-question-id='q:days'] [data-field=round]")?.textContent).toBe("round:existing");
  });

  it("drives the typed control from kind + answer_schema and offers no answer for non-Open questions", async () => {
    server.use(...handlers([]));
    renderQuestions();
    await screen.findByText(Q_PERFORMER.text);
    expect(card("q:performer").querySelector("[data-renderer]")?.getAttribute("data-renderer")).toBe("role-assignment");
    const options = Array.from(card("q:performer").querySelectorAll("[data-field=performer_ref] option")).map((o) => o.getAttribute("value"));
    expect(options.filter((v) => v !== "")).toEqual(["actor:clerk", "actor:manager"]);
    expect(card("q:days").querySelector("[data-field=days]")?.getAttribute("data-field-type")).toBe("number");
    // Incomplete answers cannot be submitted.
    expect((within(card("q:performer")).getByTestId("answer-submit") as HTMLButtonElement).disabled).toBe(true);
    // Non-Open questions show their state and no answer or supersession control.
    const answered = document.querySelector("li[data-question-id='q:answered']") as HTMLElement;
    expect(within(answered).getByTestId("question-not-open").textContent).toContain("Answered");
    expect(within(answered).queryByTestId("answer-submit")).toBeNull();
    expect(answered.querySelector("[data-renderer]")).toBeNull();
  });
});

// ------------------------------------------------------------------ answer and proposal

describe("Question answer proposal", () => {
  it("answers through the versioned client, shows the uncommitted server proposal and confirms only through /create", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls, { action: "create" }));
    renderQuestions();
    const user = userEvent.setup();
    await screen.findByText(Q_PERFORMER.text);
    const preview = await answerPerformer(user);

    expect(posts(calls, "/api/questions/q:performer/answer").map((c) => [c.ifMatch, c.body])).toEqual([
      [CURRENT, { answer: { performer_ref: "actor:clerk" }, rationale: "Clerks approve.", supersedes: null }],
    ]);
    // The answer committed nothing: no proposal action was sent and the preview is pending.
    expect(calls.filter((c) => c.path.startsWith("/api/proposals/"))).toEqual([]);
    expect(within(preview).getByTestId("answer-proposal").textContent).toBe("prop:answer-1");
    expect(within(preview).getByTestId("answer-action").textContent).toBe("create");
    expect(within(preview).getByTestId("proposal-status").textContent).toBe("Pending");
    expect(within(preview).getByTestId("proposal-reason-codes").textContent).toBe("human_decision");
    // The raw server patch is shown as is; no diff lists are derived.
    expect(JSON.parse(within(preview).getByTestId("answer-patch-preview").textContent ?? "")).toEqual(PATCH_PREVIEW);
    expect(preview.querySelector("[data-diff]")).toBeNull();

    // Only the server action and reject are actionable; accept stays disabled and is never sent.
    const enabled = Array.from(preview.querySelectorAll("button[data-action]"))
      .filter((b) => !(b as HTMLButtonElement).disabled)
      .map((b) => b.getAttribute("data-action"));
    expect(enabled.sort()).toEqual(["create", "reject"]);
    await user.click(preview.querySelector("button[data-action=accept]") as HTMLButtonElement);
    expect(calls.filter((c) => c.path.startsWith("/api/proposals/"))).toEqual([]);

    // The proposal reason is the review-bar rationale, distinct from the answer rationale.
    await user.type(within(preview).getByRole("textbox"), "HR confirmed.");
    await user.click(preview.querySelector("button[data-action=create]") as HTMLButtonElement);
    await waitFor(() => expect(posts(calls, "/api/proposals/prop:answer-1/create")).toHaveLength(1));
    expect(posts(calls, "/api/proposals/prop:answer-1/create").map((c) => [c.ifMatch, c.body])).toEqual([
      [CURRENT, { reason: "HR confirmed." }],
    ]);
    expect(posts(calls, "/api/proposals/prop:answer-1/accept")).toEqual([]);
    expect(await within(card("q:performer")).findByTestId("answer-done")).toBeTruthy();
  });

  it("rejects the pending proposal through /reject", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls, { action: "create" }));
    renderQuestions();
    const user = userEvent.setup();
    await screen.findByText(Q_PERFORMER.text);
    const preview = await answerPerformer(user);
    await user.click(preview.querySelector("button[data-action=reject]") as HTMLButtonElement);
    await waitFor(() => expect(posts(calls, "/api/proposals/prop:answer-1/reject")).toHaveLength(1));
    expect(posts(calls, "/api/proposals/prop:answer-1/reject")[0]?.body).toEqual({ reason: null });
    expect(calls.filter((c) => c.path.startsWith("/api/proposals/") && !c.path.endsWith("/reject"))).toEqual([]);
  });

  it.each(["accept", "link", "merge", "supersede"] as const)(
    "confirms a server %s action through exactly that endpoint",
    async (action) => {
      const calls: Call[] = [];
      server.use(...handlers(calls, { action }));
      renderQuestions();
      const user = userEvent.setup();
      await screen.findByText(Q_PERFORMER.text);
      const preview = await answerPerformer(user);
      const enabled = Array.from(preview.querySelectorAll("button[data-action]"))
        .filter((b) => !(b as HTMLButtonElement).disabled)
        .map((b) => b.getAttribute("data-action"));
      expect(enabled.sort()).toEqual([action, "reject"].sort());
      await user.click(preview.querySelector("button[data-action=" + action + "]") as HTMLButtonElement);
      await waitFor(() => expect(calls.filter((c) => c.path.startsWith("/api/proposals/"))).toHaveLength(1));
      expect(calls.filter((c) => c.path.startsWith("/api/proposals/")).map((c) => c.path)).toEqual([
        "/api/proposals/prop:answer-1/" + action,
      ]);
    },
  );

  it("maps every ProposalKind to its own endpoint and fails on an unknown value", () => {
    expect(() => questions.proposalActionRequest("bogus" as ProposalKind, "p", null, CURRENT)).toThrow("unsupported proposal action");
  });

  it("discards the preview on E_PROPOSAL_BASELINE_CHANGED without retrying", async () => {
    const calls: Call[] = [];
    server.use(
      ...handlers(calls, {
        action: "create",
        confirm: () => HttpResponse.json({ code: "E_PROPOSAL_BASELINE_CHANGED", message: "moved", details: {} }, { status: 409 }),
      }),
    );
    renderQuestions();
    const user = userEvent.setup();
    await screen.findByText(Q_PERFORMER.text);
    const preview = await answerPerformer(user);
    await user.click(preview.querySelector("button[data-action=create]") as HTMLButtonElement);
    expect(await within(card("q:performer")).findByTestId("answer-stale")).toBeTruthy();
    expect(within(card("q:performer")).queryByTestId("answer-preview")).toBeNull();
    // A new answer preview is required; nothing is retried or re-answered automatically.
    expect(within(card("q:performer")).getByTestId("answer-submit")).toBeTruthy();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(posts(calls, "/api/proposals/prop:answer-1/create")).toHaveLength(1);
    expect(posts(calls, "/api/questions/q:performer/answer")).toHaveLength(1);
  });
});

// ------------------------------------------------------------------ round manager

describe("Question round manager", () => {
  it("rejects missing, 0, 16 and non-integer max without posting", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderQuestions();
    const user = userEvent.setup();
    await screen.findByText(Q_PERFORMER.text);
    await user.selectOptions(screen.getByTestId("round-stakeholder"), "stakeholder:hr");
    const max = screen.getByTestId("round-max") as HTMLInputElement;
    const submit = screen.getByTestId("round-submit") as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    for (const bad of ["0", "16", "2.5"]) {
      await user.clear(max);
      await user.type(max, bad);
      expect(submit.disabled).toBe(true);
      expect(screen.getByTestId("round-max-invalid")).toBeTruthy();
    }
    // Submitting the form directly with an invalid value still sends nothing (no clamping).
    max.form?.requestSubmit();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(posts(calls, "/api/rounds")).toEqual([]);
    expect(questions.parseRoundMax("")).toBeNull();
    expect(questions.parseRoundMax("abc")).toBeNull();
    expect(questions.parseRoundMax("1e1")).toBeNull();
  });

  it.each(["1", "15"])("composes a round on the server with explicit max %s and shows the server round", async (value) => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderQuestions();
    const user = userEvent.setup();
    await screen.findByText(Q_PERFORMER.text);
    // Stakeholder choices are the exact non-null stakeholder refs of the inbox.
    const options = Array.from(screen.getByTestId("round-stakeholder").querySelectorAll("option")).map((o) => o.getAttribute("value"));
    expect(options).toEqual(["", "stakeholder:hr", "stakeholder:finance"]);
    await user.selectOptions(screen.getByTestId("round-stakeholder"), "stakeholder:hr");
    await user.type(screen.getByTestId("round-max"), value);
    await user.click(screen.getByTestId("round-submit"));
    await waitFor(() => expect(posts(calls, "/api/rounds")).toHaveLength(1));
    expect(posts(calls, "/api/rounds").map((c) => [c.ifMatch, c.body])).toEqual([[CURRENT, { stakeholder: "stakeholder:hr", max: Number(value) }]]);
    // The server round is shown exactly, in server order; the UI composes nothing.
    const result = await screen.findByTestId("round-result");
    expect(within(result).getByTestId("round-id").textContent).toBe("round:server-1");
    expect(Array.from(within(result).getByTestId("round-questions").querySelectorAll("li")).map((li) => li.textContent)).toEqual([
      "q:days",
      "q:performer",
    ]);
    // The inbox is refetched so server round assignments become visible.
    await waitFor(() => expect(calls.filter((c) => c.method === "GET" && c.path === "/api/questions").length).toBeGreaterThan(1));
  });
});
