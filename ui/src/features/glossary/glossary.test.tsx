// U2.1 glossary tests: the server term list with cursor pagination, explicit source/target/reason
// selection, the server merge preview (merge-class proposal, intake, exact semantic diff), explicit
// confirmation through /api/proposals/{id}/merge with the same reason, and stale-preview handling
// (E_PROPOSAL_BASELINE_CHANGED discards the preview and nothing is retried). Every HTTP call is
// answered by MSW and recorded; nothing touches the network.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { server } from "../../test/msw";

type GlossaryPage = components["schemas"]["GlossaryPage"];
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

const PAGE_1: GlossaryPage = {
  items: [
    { id: "term:leave", term: "leave request", language: "en", aliases: ["absence"], definition_ref: null, status: "Accepted" },
    { id: "term:vacation", term: "vacation", language: "en", aliases: ["holiday"], definition_ref: null, status: "Proposed" },
  ],
  next_cursor: "cursor-page-2",
};

const PAGE_2: GlossaryPage = {
  items: [{ id: "term:urlaub", term: "Urlaub", language: "de", aliases: [], definition_ref: null, status: "Accepted" }],
  next_cursor: null,
};

/** The server preview of merging the Proposed term:vacation into term:leave (Hotfix 074: the
 * Proposed source is outside the semantic projection, so `removed` is empty). */
const MERGE_PREVIEW: Preview = {
  version: "rev:7",
  revision: "rev:7",
  semantic_hash: CURRENT,
  events: [],
  proposal_id: "prop:merge-vacation",
  action: "merge",
  intake: {
    covered_by: [],
    conflicts_with: [{ target_ref: "term:holiday", reason: "declared conflict" }],
    refines: [],
    new_terms: [],
    novelty: null,
    duplicate_candidates: [],
    validation_findings: [],
    reason_codes: ["declared_conflict", "material_decision"],
    requires_reason: true,
  },
  diff: {
    base_semantic_hash: CURRENT,
    candidate_semantic_hash: CANDIDATE,
    added: [],
    removed: [],
    modified: ["term:leave"],
  },
};

const WRITE: WriteResult = { version: "rev:8", revision: "rev:8", semantic_hash: "psg:sha256:final", events: [] };

interface Call {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly ifMatch: string | null;
  readonly body: unknown;
}

function handlers(calls: Call[], merge: () => Response = () => HttpResponse.json(WRITE)) {
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
  return [
    http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles: ["analyst"] })),
    http.get("*/api/model/version", () =>
      HttpResponse.json({ version: "rev:7", revision: "rev:7", hash: CURRENT, semantic_hash: CURRENT, evidence_hash: "ev:1" }),
    ),
    http.get("*/api/glossary", async ({ request }) => {
      await record(request, "GET");
      return HttpResponse.json(new URL(request.url).searchParams.get("cursor") === "cursor-page-2" ? PAGE_2 : PAGE_1);
    }),
    http.post("*/api/glossary/:id/merge/preview", async ({ request }) => {
      await record(request, "POST");
      return HttpResponse.json(MERGE_PREVIEW);
    }),
    http.post("*/api/glossary/:id/merge", async ({ request }) => {
      await record(request, "POST");
      return HttpResponse.json(WRITE);
    }),
    http.post("*/api/proposals/:id/merge", async ({ request }) => {
      await record(request, "POST");
      return merge();
    }),
  ];
}

function renderGlossary() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = routerModule.createAppRouter(createMemoryHistory({ initialEntries: ["/model/glossary"] }));
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

const posts = (calls: Call[], path: string) => calls.filter((c) => c.method === "POST" && decodeURIComponent(c.path) === path);

async function chooseAndPreview() {
  const user = userEvent.setup();
  const source = (await screen.findByText("vacation")).closest("tr");
  const target = screen.getByText("leave request").closest("tr");
  if (source === null || target === null) {
    throw new Error("term rows expected");
  }
  await user.click(within(source).getByRole("button", { name: "Merge this" }));
  await user.click(within(target).getByRole("button", { name: "Merge into this" }));
  expect((screen.getByTestId("merge-source") as HTMLInputElement).value).toBe("term:vacation");
  expect((screen.getByTestId("merge-target") as HTMLInputElement).value).toBe("term:leave");
  await user.type(screen.getByTestId("merge-reason"), "Synonym.");
  await user.click(screen.getByTestId("merge-preview"));
  await screen.findByTestId("semantic-preview");
  return user;
}

// ------------------------------------------------------------------ list

describe("GlossaryPage", () => {
  it("lists the server terms with their fields and pages with the server cursor", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderGlossary();
    const user = userEvent.setup();

    const row = (await screen.findByText("vacation")).closest("tr");
    expect(Array.from(row?.querySelectorAll("td") ?? []).slice(0, 5).map((td) => td.textContent)).toEqual([
      "vacation",
      "term:vacation",
      "en",
      "Proposed",
      "holiday",
    ]);
    expect(calls[0]?.search.get("limit")).toBe("25");
    expect(calls[0]?.search.get("cursor")).toBeNull();

    await user.click(screen.getByRole("button", { name: "Next page" }));
    expect(await screen.findByText("Urlaub")).toBeTruthy();
    expect(calls.at(-1)?.search.get("cursor")).toBe("cursor-page-2");
    expect((screen.getByRole("button", { name: "Next page" }) as HTMLButtonElement).disabled).toBe(true);
  });
});

// ------------------------------------------------------------------ merge

describe("Glossary merge", () => {
  it("previews first, shows the server intake and exact diff, and commits only on confirmation through /merge", async () => {
    const calls: Call[] = [];
    server.use(...handlers(calls));
    renderGlossary();
    const user = await chooseAndPreview();

    expect(posts(calls, "/api/glossary/term:vacation/merge/preview").map((c) => [c.ifMatch, c.body])).toEqual([
      [CURRENT, { into: "term:leave", reason: "Synonym." }],
    ]);
    const panel = screen.getByTestId("semantic-preview");
    expect(within(panel).getByTestId("preview-action").textContent).toBe("merge");
    // The server intake, including its conflicts, is shown as reported.
    expect(within(panel).getByTestId("preview-requires-reason").textContent).toBe("Yes");
    expect(within(panel).getByTestId("preview-reason-codes").textContent).toBe("declared_conflict, material_decision");
    expect(within(panel).getByTestId("preview-conflicts").textContent).toBe("term:holiday declared conflict");
    // The exact server diff: the Proposed source is not added to removed by the UI.
    const section = (key: string) =>
      Array.from(panel.querySelectorAll("[data-diff=" + key + "] li")).map((li) => li.textContent);
    expect(section("added")).toEqual([]);
    expect(section("removed")).toEqual([]);
    expect(section("modified")).toEqual(["term:leave"]);
    expect(within(panel).getByTestId("diff-from").textContent).toBe(CURRENT);
    expect(within(panel).getByTestId("diff-to").textContent).toBe(CANDIDATE);
    expect(within(panel).getByTestId("preview-candidate-hash").previousElementSibling?.textContent).toBe(
      "Candidate semantic hash (proposal only, not committed)",
    );
    expect(within(panel).getByTestId("preview-reason").textContent).toBe("Synonym.");

    // Nothing is committed before the explicit confirmation.
    expect(posts(calls, "/api/proposals/prop:merge-vacation/merge")).toEqual([]);
    await user.click(within(panel).getByTestId("preview-confirm"));
    await waitFor(() => expect(screen.queryByTestId("semantic-preview")).toBeNull());
    expect(posts(calls, "/api/proposals/prop:merge-vacation/merge").map((c) => [c.ifMatch, c.body])).toEqual([
      [CURRENT, { reason: "Synonym." }],
    ]);
    // The direct compatibility merge is never used by the UI.
    expect(posts(calls, "/api/glossary/term:vacation/merge")).toEqual([]);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("discards the preview on E_PROPOSAL_BASELINE_CHANGED and never retries or re-previews", async () => {
    const calls: Call[] = [];
    server.use(
      ...handlers(calls, () =>
        HttpResponse.json({ code: "E_PROPOSAL_BASELINE_CHANGED", message: "baseline changed", details: {} }, { status: 409 }),
      ),
    );
    renderGlossary();
    const user = await chooseAndPreview();
    await user.click(screen.getByTestId("preview-confirm"));

    expect((await screen.findByTestId("preview-stale")).textContent).toBe(
      "The model changed since this preview, so it was discarded. Request a new preview.",
    );
    expect(screen.queryByTestId("semantic-preview")).toBeNull();
    expect(screen.getByTestId("merge-preview")).toBeTruthy();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(posts(calls, "/api/proposals/prop:merge-vacation/merge")).toHaveLength(1);
    expect(posts(calls, "/api/glossary/term:vacation/merge/preview")).toHaveLength(1);
    expect(posts(calls, "/api/glossary/term:vacation/merge")).toEqual([]);

    // A new preview is an explicit user action and yields a fresh confirmable preview.
    await user.click(screen.getByTestId("merge-preview"));
    await screen.findByTestId("semantic-preview");
    expect(screen.queryByTestId("preview-stale")).toBeNull();
    expect(posts(calls, "/api/glossary/term:vacation/merge/preview")).toHaveLength(2);
  });

  it("does not offer a preview until source, target and reason are chosen", async () => {
    server.use(...handlers([]));
    renderGlossary();
    await screen.findByText("vacation");
    expect((screen.getByTestId("merge-preview") as HTMLButtonElement).disabled).toBe(true);
  });
});
