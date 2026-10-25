// Assistant panel (handoff "8. Assistant panel", UI-SPEC §5.8, flow 7): scope chip, bubbles,
// inline proposal cards decided through the proposal endpoints, the typing indicator and a
// composer that is disabled while a turn is in flight or AI is off; on create_session,
// create_session_turn and get_session. Rendered inside the real shell with MSW.
import { screen, waitFor, within } from "@testing-library/react";
import { delay, http, HttpResponse } from "msw";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";

import type { components } from "../../api/schema";
import { axeViolations } from "../../test/axe";
import { app, calls, renderApp, resetApp, setupApp, teardownApp } from "../../test/appHarness";
import { etagJson, server } from "../../test/msw";
import { useAssistantStore } from "./store";

type Me = components["schemas"]["MeResponse"];
type SessionTurnResponse = components["schemas"]["SessionTurnResponse"];
type PendingProposalDetail = components["schemas"]["PendingProposalDetail"];

beforeAll(setupApp);
afterAll(teardownApp);
beforeEach(() => {
  resetApp();
  useAssistantStore.getState().reset();
  app().preferences.useShellStore.setState({ assistantOpen: true });
});

const VERSION_ETAG = '"rev:42:aaaaaaaaaaaaaaaa"';
const ME: Me = { user: "Marta Nowak", mode: "pilot", roles: ["analyst"], inference_available: true, inference_model: "m", project: { id: "project:hr", name: "HR", database: null } };

const INTAKE = { conflicts_with: [], covered_by: [], duplicate_candidates: [], new_terms: [], novelty: null, reason_codes: [], refines: [], requires_reason: false, validation_findings: [] };

function turnResponse(reply: string, extra: Partial<SessionTurnResponse> = {}): SessionTurnResponse {
  return { reply, proposals: [], events: [], revision: "rev:43:bbbbbbbbbbbbbbbb", semantic_hash: "psg:b", version: "rev:43:bbbbbbbbbbbbbbbb", asked_question: null, ...extra };
}

const DETAIL: PendingProposalDetail = {
  proposal_id: "prop:balance",
  action: "create",
  base_revision: "rev:43:bbbbbbbbbbbbbbbb",
  created_at: "2026-10-25T12:00:00Z",
  diff: { added: [], modified: [], removed: [] },
  discardable: false,
  element_kind: "operation",
  evidence: [],
  facts: [
    { label: "Performed by", value: "Employee" },
    { label: "Reads", value: "Employee → annual leave balance" },
  ],
  origin: "session",
  requires_reason: false,
  stale: false,
  title: "View leave balance",
};

interface Backend {
  me: Me;
  sessions: { scope: unknown; ifMatch: string | null }[];
  turns: { session: string; text: string; stage: string; ifMatch: string | null }[];
  accepts: { body: unknown; ifMatch: string | null }[];
  rejects: { id: string; body: unknown; ifMatch: string | null }[];
  sessionReads: number;
  replies: (() => Response | Promise<Response>)[];
  requirementStatus: string;
}

function serve(overrides: Partial<Backend> = {}): Backend {
  const b: Backend = {
    me: ME,
    sessions: [],
    turns: [],
    accepts: [],
    rejects: [],
    sessionReads: 0,
    replies: [],
    requirementStatus: "Accepted",
    ...overrides,
  };
  server.use(
    http.get("*/api/me", () => HttpResponse.json(b.me)),
    http.get("*/api/model/version", () =>
      etagJson({ version: "rev:42:a", revision: "rev:42:aaaaaaaaaaaaaaaa", hash: "h", semantic_hash: "h", evidence_hash: "e", created_at: "2026-10-25T11:00:00Z", created_by: "agent:plumb" }, undefined, VERSION_ETAG),
    ),
    http.post("*/api/sessions", async ({ request }) => {
      b.sessions.push({ scope: ((await request.json()) as { scope: unknown }).scope, ifMatch: request.headers.get("if-match") });
      return etagJson({ id: "session:" + b.sessions.length, events: [], revision: "rev:42:aaaaaaaaaaaaaaaa", semantic_hash: "h", version: "rev:42:a" });
    }),
    http.get("*/api/sessions/:id", ({ params }) => {
      b.sessionReads += 1;
      return etagJson({ id: String(params["id"]), assistant_actor_ref: "agent:assistant", scope: b.sessions[0]?.scope ?? [] });
    }),
    http.post("*/api/sessions/:id/turns", async ({ request, params }) => {
      const body = (await request.json()) as { text: string; stage: string };
      b.turns.push({ session: String(params["id"]), text: body.text, stage: body.stage, ifMatch: request.headers.get("if-match") });
      const reply = b.replies.shift();
      return reply === undefined ? etagJson(turnResponse("Noted.")) : reply();
    }),
    http.get("*/api/proposals/:id", ({ params }) => etagJson({ ...DETAIL, proposal_id: String(params["id"]) })),
    http.post("*/api/proposals/accept-set", async ({ request }) => {
      b.accepts.push({ body: await request.json(), ifMatch: request.headers.get("if-match") });
      return etagJson({ accepted: ["prop:balance"], confirmed: [], events: [], revision: "rev:44:c", semantic_hash: "c", version: "rev:44:c" });
    }),
    http.post("*/api/proposals/:id/reject", async ({ request, params }) => {
      b.rejects.push({ id: String(params["id"]), body: await request.json(), ifMatch: request.headers.get("if-match") });
      return etagJson({ events: [], revision: "rev:44:c", semantic_hash: "c", version: "rev:44:c" });
    }),
    http.get("*/api/requirements/:id", ({ params }) =>
      etagJson(
        { id: String(params["id"]), text: "Approving annual leave shall deduct the balance.", class: "functional", status: b.requirementStatus, smells: [], coverage: { operations: [], scenarios: [] }, readiness: null, spans: [], evidence: [], lint_findings: [], linked: { operations: [], processes: [], scenarios: [] }, lint_current: true },
        undefined,
        '"rev:42:requirement"',
      ),
    ),
  );
  return b;
}

const panel = () => screen.getByRole("complementary", { name: "Assistant" });
const composer = () => within(panel()).getByRole("textbox", { name: "Message the Assistant" });
/** Waits until the composer accepts input (identity and version loaded). */
const ready = () => waitFor(() => expect(composer().hasAttribute("disabled")).toBe(false));

describe("Assistant panel", () => {
  it("opens scoped to the project with an introduction and an empty composer", async () => {
    serve();
    renderApp("/overview");
    await screen.findByRole("complementary", { name: "Assistant" });
    expect(within(panel()).getByText("Scope · Project")).toBeTruthy();
    expect(within(panel()).getByText("Ask about the specification, or ask for a change. Plumb answers and proposes; nothing changes until you accept.")).toBeTruthy();
    expect(composer().getAttribute("placeholder")).toBe("Ask about the specification, or ask for a change");
    expect(within(panel()).getByRole("button", { name: "Send" }).hasAttribute("disabled")).toBe(true);
    expect(await axeViolations(panel())).toEqual([]);
  });

  it("starts a session on the first message, shows the typing indicator, the reply and an inline proposal it accepts", async () => {
    let release: () => void = () => undefined;
    const b = serve({
      replies: [
        async () => {
          await new Promise<void>((resolve) => {
            release = resolve;
          });
          return etagJson(
            turnResponse("I can add an operation View leave balance performed by Employee.", {
              proposals: [{ id: "prop:balance", kind: "create", intake: INTAKE, payload: {} }],
            }),
          );
        },
      ],
    });
    const { user } = renderApp("/overview");
    await screen.findByRole("complementary", { name: "Assistant" });
    await ready();
    await user.type(composer(), "Employees should be able to see their remaining balance.{Enter}");
    // The turn is in flight: the message shows, Plumb is typing, the composer waits.
    expect(await within(panel()).findByText("Employees should be able to see their remaining balance.")).toBeTruthy();
    expect(await within(panel()).findByRole("status", { name: "Plumb is typing" })).toBeTruthy();
    expect(composer().hasAttribute("disabled")).toBe(true);
    await waitFor(() => expect(b.turns).toHaveLength(1));
    expect(b.sessions).toEqual([{ scope: [], ifMatch: VERSION_ETAG }]);
    expect(b.turns[0]).toEqual({ session: "session:1", text: "Employees should be able to see their remaining balance.", stage: "S2", ifMatch: VERSION_ETAG });
    release();
    expect(await within(panel()).findByText("I can add an operation View leave balance performed by Employee.")).toBeTruthy();
    await waitFor(() => expect(within(panel()).queryByRole("status", { name: "Plumb is typing" })).toBeNull());
    expect(composer().hasAttribute("disabled")).toBe(false);
    // The inline proposal card reads the pending proposal.
    const card = await within(panel()).findByRole("article", { name: "View leave balance" });
    expect(within(card).getByText("Proposal · New operation")).toBeTruthy();
    expect(within(card).getByText("Performed by Employee · Reads Employee → annual leave balance")).toBeTruthy();
    expect(await axeViolations(panel())).toEqual([]);
    await user.click(within(card).getByRole("button", { name: "Accept" }));
    await waitFor(() => expect(b.accepts).toEqual([{ body: { proposal_ids: ["prop:balance"], reason: null, confirm: true }, ifMatch: '"rev:43:bbbbbbbbbbbbbbbb"' }]));
    expect(await within(card).findByText("Accepted · added to the model")).toBeTruthy();
    expect(within(card).queryByRole("button", { name: "Accept" })).toBeNull();
    expect(await screen.findByText("View leave balance added to the model")).toBeTruthy();
    // A second message reuses the session.
    await user.type(composer(), "Thanks{Enter}");
    expect(await within(panel()).findByText("Noted.")).toBeTruthy();
    expect(b.sessions).toHaveLength(1);
    expect(b.turns.map((t) => t.session)).toEqual(["session:1", "session:1"]);
  });

  it("rejects a proposal with a reason when its intake requires one", async () => {
    const b = serve({
      replies: [() => etagJson(turnResponse("Here is a proposal.", { proposals: [{ id: "prop:balance", kind: "create", intake: { ...INTAKE, requires_reason: true }, payload: {} }] }))],
    });
    const { user } = renderApp("/overview");
    await screen.findByRole("complementary", { name: "Assistant" });
    await ready();
    await user.type(composer(), "Propose something{Enter}");
    const card = await within(panel()).findByRole("article", { name: "View leave balance" });
    await user.click(within(card).getByRole("button", { name: "Reject" }));
    const sheet = await screen.findByRole("dialog", { name: "Reject proposal" });
    const confirm = within(sheet).getByRole("button", { name: /Reject/ });
    expect(confirm.hasAttribute("disabled")).toBe(true);
    await user.type(within(sheet).getByRole("textbox"), "Not in scope for this release.");
    await user.click(confirm);
    await waitFor(() => expect(b.rejects).toEqual([{ id: "prop:balance", body: { reason: "Not in scope for this release." }, ifMatch: '"rev:43:bbbbbbbbbbbbbbbb"' }]));
    expect(await within(card).findByText("Rejected")).toBeTruthy();
  });

  it("asks for a reason before accepting when required, and explains a refused decision", async () => {
    const b = serve({
      replies: [() => etagJson(turnResponse("Here is a proposal.", { proposals: [{ id: "prop:balance", kind: "create", intake: { ...INTAKE, requires_reason: true }, payload: {} }] }))],
    });
    server.use(
      http.post("*/api/proposals/accept-set", async ({ request }) => {
        b.accepts.push({ body: await request.json(), ifMatch: request.headers.get("if-match") });
        return HttpResponse.json({ code: "E_PROPOSAL_NOT_PENDING", message: "This proposal is no longer pending.", details: {} }, { status: 409 });
      }),
    );
    const { user } = renderApp("/overview");
    await screen.findByRole("complementary", { name: "Assistant" });
    await ready();
    await user.type(composer(), "Propose something{Enter}");
    const card = await within(panel()).findByRole("article", { name: "View leave balance" });
    await user.click(within(card).getByRole("button", { name: "Accept" }));
    const sheet = await screen.findByRole("dialog", { name: "Accept proposal" });
    await user.type(within(sheet).getByRole("textbox"), "Confirmed with HR.");
    await user.click(within(sheet).getByRole("button", { name: /Accept/ }));
    await waitFor(() => expect(b.accepts).toEqual([{ body: { proposal_ids: ["prop:balance"], reason: "Confirmed with HR.", confirm: true }, ifMatch: '"rev:43:bbbbbbbbbbbbbbbb"' }]));
    expect(await within(sheet).findByText("This proposal is no longer pending.")).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(await within(card).findByText("This proposal is no longer pending.")).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Accept" })).toBeTruthy();
  });

  it("is read-only while AI is off", async () => {
    const b = serve({ me: { ...ME, inference_available: false, inference_model: null } });
    renderApp("/overview");
    await screen.findByRole("complementary", { name: "Assistant" });
    expect(await within(panel()).findByText("AI is off, so the Assistant is read-only. Add an API key in Settings.")).toBeTruthy();
    expect(composer().hasAttribute("disabled")).toBe(true);
    expect(b.sessions).toEqual([]);
  });

  it("scopes the session to the open accepted requirement", async () => {
    const b = serve();
    const { user } = renderApp("/requirements/req:7");
    await screen.findByRole("complementary", { name: "Assistant" });
    expect(within(panel()).getByText("Scope · req:7")).toBeTruthy();
    await waitFor(() => expect(composer().hasAttribute("disabled")).toBe(false));
    await user.type(composer(), "Does this cover withdrawn requests?{Enter}");
    await waitFor(() => expect(b.sessions).toEqual([{ scope: ["req:7"], ifMatch: '"rev:42:requirement"' }]));
    await waitFor(() => expect(b.turns[0]?.stage).toBe("S1"));
  });

  it("explains why a requirement that is not accepted cannot be the scope", async () => {
    serve({ requirementStatus: "Proposed" });
    renderApp("/requirements/req:8");
    await screen.findByRole("complementary", { name: "Assistant" });
    expect(await within(panel()).findByText("The Assistant works on accepted requirements. Accept this requirement first, or close it to ask about the whole project.")).toBeTruthy();
    expect(composer().hasAttribute("disabled")).toBe(true);
  });

  it("explains a failed turn in plain language, with its code", async () => {
    serve({
      replies: [
        () => HttpResponse.json({ code: "E_SESSION_TURN_IN_FLIGHT", message: "turn in flight", details: {} }, { status: 409 }),
        () => HttpResponse.json({ code: "E_INFERENCE_FAILED", message: "provider timeout", details: {} }, { status: 502 }),
      ],
    });
    const { user } = renderApp("/overview");
    await screen.findByRole("complementary", { name: "Assistant" });
    await ready();
    await user.type(composer(), "First{Enter}");
    const first = await within(panel()).findByRole("alert");
    expect(first.textContent).toContain("Another message is still being answered. Wait for it to finish, then try again.");
    expect(first.textContent).toContain("E_SESSION_TURN_IN_FLIGHT");
    await user.type(composer(), "Second{Enter}");
    await waitFor(() => expect(within(panel()).getAllByRole("alert")).toHaveLength(2));
    expect(within(panel()).getAllByRole("alert")[1]?.textContent).toContain("E_INFERENCE_FAILED");
  });

  it("keeps the conversation when the panel closes and reopens, confirming the session", async () => {
    const b = serve({ replies: [() => etagJson(turnResponse("Balances are deducted on approval.", { asked_question: "q:7" }))] });
    const { user, router } = renderApp("/overview");
    await screen.findByRole("complementary", { name: "Assistant" });
    await ready();
    await user.type(composer(), "When is the balance deducted?{Enter}");
    expect(await within(panel()).findByText("Balances are deducted on approval.")).toBeTruthy();
    // The Assistant asked a question back: it is answered in the Inbox.
    const question = within(panel()).getByRole("link", { name: "Answer in the Inbox" });
    await user.click(within(panel()).getByRole("button", { name: "Close the Assistant" }));
    expect(screen.queryByRole("complementary", { name: "Assistant" })).toBeNull();
    app().preferences.useShellStore.setState({ assistantOpen: true });
    expect(await within(await screen.findByRole("complementary", { name: "Assistant" })).findByText("Balances are deducted on approval.")).toBeTruthy();
    await waitFor(() => expect(b.sessionReads).toBeGreaterThan(0));
    expect(calls.some((c) => c.method === "GET" && c.path === "/api/sessions/session:1")).toBe(true);
    void question;
    await user.click(within(panel()).getByRole("link", { name: "Answer in the Inbox" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/inbox/q:7"));
  });

  it("does not send a blank message and keeps Shift+Enter for a new line", async () => {
    const b = serve();
    server.use(http.post("*/api/sessions/:id/turns", () => delay("infinite")));
    const { user } = renderApp("/overview");
    await screen.findByRole("complementary", { name: "Assistant" });
    await ready();
    await user.type(composer(), "   {Enter}");
    await user.type(composer(), "Line one{Shift>}{Enter}{/Shift}line two");
    expect((composer() as HTMLTextAreaElement).value).toBe("   Line one\nline two");
    expect(b.sessions).toEqual([]);
  });
});
