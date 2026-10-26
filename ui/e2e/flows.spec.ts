// The nine end-to-end flows of the UI spec (docs/design/UI-SPEC-FOR-CLAUDE-DESIGN.md §6) and the
// system states, against the real backend. scripts/run-pilot-e2e.sh serves one database per tag,
// each built only by product operations:
// - @empty-db: an initialized project served with the replay provider (the recorded Requirements
//   step), for the first run (flow 1) and the Assistant with AI on (flow 7);
// - @triage-db: a copy of the Inbox database (32 Requirements proposals), for keyboard triage
//   (flow 2) and the concurrency conflict (flow 8);
// - @questions-db: the HR pilot stopped where the expert's questions are open, for the answer
//   flow (flow 3), a failing gate (flow 4) and failing scenarios (flow 6);
// - untagged: the finished pilot, for Export (flow 9) and the system states.
// Flow 5 (waive with governance) is in requirements-quality.spec.ts.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Download, type Locator, type Page } from "@playwright/test";

const PILOT_DOCUMENT = fileURLToPath(new URL("../../fixtures/hr-leave/requirements.md", import.meta.url));

async function noSeriousA11yViolations(page: Page) {
  // Colour contrast is measured on settled pixels: wait for slide-ins and toasts.
  // (Endless ones, such as a loading shimmer, never settle and are left out.)
  await page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== "running" || a.effect?.getTiming().iterations === Infinity));
  await page.mouse.move(0, 0);
  const results = await new AxeBuilder({ page }).analyze();
  const serious = results.violations.filter((v) => v.impact === "critical" || v.impact === "serious");
  expect(serious.map((v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(" ")).join(", ")})`)).toEqual([]);
}

const main = (page: Page) => page.getByRole("main");
const banner = (page: Page) => page.getByRole("banner");
const toast = (page: Page) => page.locator("[data-toast]");
const conflictSheet = (page: Page) => page.getByRole("dialog", { name: "This changed since you looked" });
const pipelineStep = (page: Page, step: string) => main(page).getByRole("region", { name: "Pipeline" }).locator(`[data-step='${step}']`);

/** Drops a file on a drop zone the way a browser does (dragenter, dragover, drop). */
async function dropFile(page: Page, zone: Locator, path: string, name: string, type: string) {
  const content = readFileSync(path, "utf8");
  const dataTransfer = await page.evaluateHandle(
    ({ content, name, type }) => {
      const dt = new DataTransfer();
      dt.items.add(new File([content], name, { type }));
      return dt;
    },
    { content, name, type },
  );
  for (const event of ["dragenter", "dragover", "drop"]) {
    await zone.dispatchEvent(event, { dataTransfer });
  }
}

test.describe("Flow 1: first run, empty to first proposals @empty-db", () => {
  test("drop requirements.md on the Overview, run the Requirements step, accept all, see the next step", async ({ page }) => {
    test.setTimeout(120_000);
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));

    // An empty project: the Overview is the import hero.
    await page.goto("/overview");
    await expect(main(page).getByRole("heading", { level: 1, name: "Import your requirements document" })).toBeVisible();
    await expect(main(page).getByText("New project")).toBeVisible();
    await noSeriousA11yViolations(page);

    // Drop the document: 38 passages imported, and the toast offers the Requirements step.
    await dropFile(page, main(page).getByRole("button", { name: "Drop a file here, or choose one" }), PILOT_DOCUMENT, "requirements.md", "text/markdown");
    await expect(toast(page)).toContainText("38 passages imported from requirements.md");
    await expect(main(page).getByRole("heading", { level: 1, name: "HR" })).toBeVisible();
    await expect(pipelineStep(page, "import")).toHaveAttribute("data-step-state", "done");

    // Run it from the toast: the step runs (stepper, toolbar progress), then finishes.
    await toast(page).getByRole("button", { name: "Run Requirements step" }).click();
    await expect(main(page).getByRole("progressbar", { name: /Plumb is running the Requirements step/ })).toBeVisible();
    await expect(banner(page).getByRole("button", { name: /^Requirements step running, \d+%$/ })).toBeVisible();
    await expect(toast(page).filter({ hasText: "Requirements step finished" })).toBeVisible({ timeout: 30_000 });
    await expect(conflictSheet(page)).toHaveCount(0);
    const card = main(page).getByRole("region", { name: "Next action" });
    await expect(card.getByText("Review 32 proposals")).toBeVisible();
    await expect(pipelineStep(page, "requirements")).toHaveAttribute("data-step-state", "needs_review");
    await expect(pipelineStep(page, "requirements")).toContainText("32");
    await noSeriousA11yViolations(page);

    // The step opens the Inbox; every proposal is from that step and is accepted at once.
    await pipelineStep(page, "requirements").click();
    await expect(page).toHaveURL(/\/inbox/);
    await expect(page.getByText("32 waiting", { exact: true })).toBeVisible();
    const filters = page.getByRole("navigation", { name: "Inbox filters" });
    await filters.getByRole("link", { name: /^From steps/ }).click();
    await expect(page).toHaveURL(/filter=steps/);
    await expect(page.getByText("32 waiting", { exact: true })).toBeVisible();
    await noSeriousA11yViolations(page);
    await page.getByRole("button", { name: "Accept all from Requirements" }).click();
    await expect(toast(page).filter({ hasText: /32 .*accepted/ })).toBeVisible();
    await expect(page.getByRole("heading", { name: "You're all caught up" })).toBeVisible();

    // Back on the Overview: the 32 requirements are in, and the next action moves on.
    await page.getByRole("navigation", { name: "Main navigation" }).getByRole("link", { name: "Overview" }).click();
    await expect(main(page).getByRole("region", { name: "At a glance" }).getByRole("link", { name: /Requirements/ })).toContainText("32");
    await expect(pipelineStep(page, "requirements")).toHaveAttribute("data-step-state", "done");
    // Glossary & quality (consolidate) follows Requirements, on the card and in the toolbar.
    await expect(main(page).getByRole("region", { name: "Next action" }).getByText("Run the Glossary step")).toBeVisible();
    await expect(banner(page).getByRole("button", { name: "Run Glossary" })).toBeVisible();
    await expect(banner(page).getByRole("status")).toHaveAccessibleName(/v4: Up to date/);
    await noSeriousA11yViolations(page);
    expect(errors).toEqual([]);
  });
});

const API_URL = `http://127.0.0.1:${process.env["PLUMB_E2E_API_PORT"] ?? "8790"}`;
const queue = (page: Page) => page.getByRole("list", { name: "All items" });
const rows = (page: Page) => queue(page).getByRole("button");
const inspector = (page: Page) => page.getByRole("complementary", { name: "Item details" });
const waiting = (page: Page, n: number) => expect(page.getByText(`${n} waiting`, { exact: true })).toBeVisible();

/**
 * Holds the design events the server pushes to this page until `release` (the live socket stays
 * connected to the real server). A concurrent write is then seen the way a slow connection sees
 * it: the page still shows what the user reviewed when they act.
 */
async function holdDesignEvents(page: Page) {
  const held: (string | Buffer)[] = [];
  let holding = true;
  let deliver: ((message: string | Buffer) => void) | null = null;
  await page.routeWebSocket(/\/api\/events$/, (ws) => {
    const server = ws.connectToServer();
    deliver = (message) => ws.send(message);
    server.onMessage((message) => (holding ? held.push(message) : ws.send(message)));
  });
  return {
    release() {
      holding = false;
      held.splice(0).forEach((message) => deliver?.(message));
    },
  };
}

test.describe.serial("Inbox triage and conflicts @triage-db", () => {
  test("flow 8: a write lands between review and Accept; the conflict sheet appears and Review again reloads", async ({ page, playwright }) => {
    test.setTimeout(120_000);
    const events = await holdDesignEvents(page);
    await page.goto("/inbox");
    await waiting(page, 32);
    // The user reviews the first proposal.
    await expect(inspector(page).getByRole("heading", { level: 2, name: "Add requirement HR-001" })).toBeVisible();
    await noSeriousA11yViolations(page);

    // Meanwhile another client accepts the last proposal against the current version: the model
    // moves on.
    const api = await playwright.request.newContext({ baseURL: API_URL });
    const listed = await api.get("/api/proposals");
    const etag = listed.headers()["etag"] ?? "";
    const { items } = (await listed.json()) as { items: { proposal_id: string; title: string }[] };
    const other = items.find((p) => p.title === "Add requirement HR-032");
    expect(other).toBeDefined();
    const write = await api.post("/api/proposals/accept-set", { headers: { "If-Match": etag }, data: { proposal_ids: [other?.proposal_id], confirm: true, reason: null } });
    expect(write.status()).toBe(200);
    await api.dispose();

    // Accept on what the user reviewed: the server refuses it, nothing is applied.
    await inspector(page).getByRole("button", { name: "Accept" }).click();
    const sheet = conflictSheet(page);
    await expect(sheet).toBeVisible();
    await expect(sheet.getByText("The model changed after you loaded this view, so your change was not applied. Review the latest version and decide again.")).toBeVisible();
    await expect(sheet.getByText(/^The model is now at version v\d+\.$/)).toBeVisible();
    await noSeriousA11yViolations(page);
    events.release();
    await sheet.getByRole("button", { name: "Review again" }).click();
    await expect(sheet).toHaveCount(0);

    // The Inbox shows the latest version: the other acceptance is in, HR-001 was not accepted and
    // is now out of date (proposed on the older version), so it cannot be accepted as it is.
    await waiting(page, 31);
    await expect(queue(page).getByText("Add requirement HR-032")).toHaveCount(0);
    await expect(inspector(page).getByRole("heading", { level: 2, name: "Add requirement HR-001" })).toBeVisible();
    await expect(inspector(page).getByText(/Proposed on an older version/)).toBeVisible();
    await expect(inspector(page).getByRole("button", { name: "Accept" })).toBeDisabled();
    const requirements = (await (await page.request.get("/api/requirements")).json()) as { items: { text: string }[] };
    expect(requirements.items.map((r) => r.text)).toEqual(["The system shall create an audit record for every approval, rejection, withdrawal, and cancellation decision."]);
  });

  test("flow 2: keyboard triage J/K/A/R with the reason sheet, a stale item refreshed, then all caught up", async ({ page }) => {
    test.setTimeout(180_000);
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto("/inbox");
    await waiting(page, 31);
    await expect(rows(page).first()).toHaveAttribute("aria-current", "true");
    // J moves down, K back up.
    await page.keyboard.press("j");
    await expect(inspector(page).getByRole("heading", { level: 2, name: "Add requirement HR-002" })).toBeVisible();
    await page.keyboard.press("k");
    await expect(inspector(page).getByRole("heading", { level: 2, name: "Add requirement HR-001" })).toBeVisible();
    // The items are out of date (flow 8 moved the model): A does nothing on a stale item.
    await page.keyboard.press("a");
    await waiting(page, 31);
    // R opens the reason sheet; the reason is typed and ⌘↩ confirms.
    await page.keyboard.press("r");
    const sheet = page.getByRole("dialog", { name: "Reject proposal" });
    await expect(sheet).toBeVisible();
    await noSeriousA11yViolations(page);
    await sheet.getByRole("textbox").fill("Drafts are out of scope for the pilot.");
    await page.keyboard.press("ControlOrMeta+Enter");
    await expect(toast(page).filter({ hasText: "Proposal rejected" })).toBeVisible();
    await waiting(page, 30);
    await expect(rows(page).first()).toBeFocused();
    await expect(inspector(page).getByRole("heading", { level: 2, name: "Add requirement HR-002" })).toBeVisible();
    // Refresh the stale item: the Requirements step runs again on the current model, and its
    // fresh proposals arrive next to the out-of-date ones (which stay until they are decided).
    await inspector(page).getByRole("button", { name: "Refresh proposal" }).click();
    await expect(toast(page).filter({ hasText: "Running the Requirements step again" })).toBeVisible();
    await expect(toast(page).filter({ hasText: "Requirements step finished" })).toBeVisible({ timeout: 30_000 });
    await waiting(page, 61);
    const fresh = rows(page).filter({ hasNotText: "Stale" });
    await expect(fresh).toHaveCount(31);
    await noSeriousA11yViolations(page);

    // A accepts a fresh item; focus moves on and every other item is now out of date.
    await fresh.filter({ hasText: "Add requirement HR-009" }).click();
    await expect(inspector(page).getByRole("button", { name: "Accept" })).toBeEnabled();
    await page.keyboard.press("a");
    await expect(toast(page).filter({ hasText: /Requirement added/ })).toBeVisible();
    await waiting(page, 60);
    await expect(fresh).toHaveCount(0);

    // The out-of-date items are rejected from the keyboard, each through the reason sheet.
    await rows(page).first().click();
    for (let left = 60; left > 0; left--) {
      await page.keyboard.press("r");
      await expect(sheet).toBeVisible();
      await page.keyboard.press("ControlOrMeta+Enter");
      await expect(sheet).toHaveCount(0);
      if (left > 1) {
        await waiting(page, left - 1);
      }
    }
    await expect(page.getByRole("heading", { name: "You're all caught up" })).toBeVisible();
    await expect(page.getByText("Nothing is waiting for your decision. The next step is Glossary.")).toBeVisible();
    await expect(page.getByRole("navigation", { name: "Main navigation" }).getByRole("link", { name: /Inbox/ }).locator("[data-badge]")).toHaveCount(0);
    await noSeriousA11yViolations(page);
    const requirements = (await (await page.request.get("/api/requirements")).json()) as { items: { text: string }[] };
    expect(requirements.items).toHaveLength(2);
    expect(errors).toEqual([]);
  });
});

test.describe.serial("Questions, gates and scenarios @questions-db", () => {
  test("flow 3: the expert answers a question; the answer becomes a proposal, is accepted and is in History", async ({ page }) => {
    test.setTimeout(120_000);
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    // Before the answer the calculation leaves the end date open.
    await page.goto("/model/calculations");
    await expect(main(page).getByText("working_days([start_date, end_date], calendar, inclusive_end) * day_fraction")).toBeVisible();
    await page.goto("/overview");
    await main(page).getByRole("region", { name: "Next action" }).getByRole("button", { name: "Open questions" }).click();
    await expect(page).toHaveURL(/\/inbox\?filter=questions/);
    const questions = page.getByRole("list", { name: "Questions" });
    await expect(questions.getByRole("button")).toHaveCount(4);
    await questions.getByRole("button", { name: /include its end date\?/ }).click();
    await expect(inspector(page).getByRole("heading", { level: 2, name: /include its end date\?/ })).toBeVisible();
    const answerButton = inspector(page).getByRole("button", { name: "Answer" });
    await expect(answerButton).toBeDisabled();
    await inspector(page).getByRole("radio", { name: "Yes" }).check();
    await inspector(page).getByRole("textbox", { name: /Rationale/ }).fill("Both start date and end date are included.");
    await noSeriousA11yViolations(page);
    await answerButton.click();
    await expect(toast(page).filter({ hasText: "Answer recorded · it now needs your confirmation" })).toBeVisible();
    // The answer is now a proposal from questions, open in the inspector with its model change.
    await expect(page).toHaveURL(/\/inbox\/prop:/);
    await expect(inspector(page).getByText("Answer to confirm · From a question")).toBeVisible();
    await expect(inspector(page).getByRole("list", { name: "Facts" })).toContainText("inclusive end: yes");
    await expect(inspector(page).getByRole("list", { name: "Model change" })).toContainText("calculation requested leave duration (changed)");
    await noSeriousA11yViolations(page);
    // Accepting an answer is governed: the reason is recorded with the decision.
    await inspector(page).getByRole("button", { name: "Accept" }).click();
    const reason = page.getByRole("dialog", { name: "Accept proposal" });
    await reason.getByRole("textbox", { name: "Reason (required)" }).fill("Confirmed with HR: the end date is a leave day.");
    await noSeriousA11yViolations(page);
    await page.keyboard.press("ControlOrMeta+Enter");
    await expect(toast(page).filter({ hasText: "Model updated" })).toBeVisible();
    await expect(page.getByRole("list", { name: "Questions" })).toHaveCount(0);
    await page.getByRole("navigation", { name: "Inbox filters" }).getByRole("link", { name: /^Questions/ }).click();
    await expect(page.getByRole("list", { name: "Questions" }).getByRole("button")).toHaveCount(3);
    await expect(page.getByRole("list", { name: "Questions" }).getByRole("button", { name: /include its end date\?/ })).toHaveCount(0);

    // The calculation now counts the end date (its expression shows the inclusive period).
    await toast(page).getByRole("button", { name: "View in Model" }).click();
    await expect(page).toHaveURL(/\/model\/calculations/);
    await expect(main(page).getByText("working_days([start_date, end_date], calendar, true) * day_fraction")).toBeVisible();
    await expect(main(page).getByText("working_days([start_date, end_date], calendar, inclusive_end) * day_fraction")).toHaveCount(0);

    // History shows the decision: the answer, who gave it and its rationale.
    await page.getByRole("navigation", { name: "Main navigation" }).getByRole("link", { name: "Quality" }).click();
    await page.getByRole("navigation", { name: "Quality views" }).getByRole("link", { name: "History" }).click();
    await expect(page).toHaveURL(/\/quality\/history$/);
    await page.getByRole("searchbox", { name: "Search decisions" }).fill("end date");
    const history = page.getByRole("table", { name: "Decision history" });
    await expect(history.getByText("Answered a question").first()).toBeVisible();
    await expect(history.getByText(/Both start date and end date are included\./).first()).toBeVisible();
    await noSeriousA11yViolations(page);
    expect(errors).toEqual([]);
  });

  test("flow 4: the Model gate is blocked; Show affected, fix it by answering, Re-check, it passes", async ({ page }) => {
    test.setTimeout(180_000);
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    // The Overview shows the Model gate blocked.
    await page.goto("/overview");
    const gates = main(page).getByRole("region", { name: "Quality gates" });
    await expect(gates.getByRole("link", { name: /^Model F2 Blocked · 2 blocking issues/ })).toBeVisible();
    await gates.getByRole("link", { name: /^Model F2/ }).click();
    await expect(page).toHaveURL(/\/quality\/F2$/);
    const model = main(page).getByRole("region", { name: "Model F2" });
    await expect(model.getByText("State transitions are complete")).toBeVisible();
    await expect(model.getByText(/Produced material events have a consumer/)).toBeVisible();
    await noSeriousA11yViolations(page);

    // Show affected: the requirements behind the incomplete transition.
    await model.getByRole("button", { name: "Show affected" }).click();
    await expect(page).toHaveURL(/\/requirements\?affected=/);
    const affected = main(page).getByTestId("requirement-row");
    await expect(affected).toHaveCount(2);
    await expect(affected.filter({ hasText: "HR-017" })).toHaveCount(1);
    await expect(affected.filter({ hasText: "HR-018" })).toHaveCount(1);
    await noSeriousA11yViolations(page);

    // Fix: the expert names the trigger of the transition, and answers the round's other questions.
    await page.goto("/inbox?filter=questions");
    const questions = page.getByRole("list", { name: "Questions" });
    const answer = async (question: RegExp, fill: () => Promise<void>, rationale: string) => {
      await questions.getByRole("button", { name: question }).click();
      await expect(inspector(page).getByRole("heading", { level: 2, name: question })).toBeVisible();
      await fill();
      await inspector(page).getByRole("textbox", { name: /Rationale/ }).fill(rationale);
      await inspector(page).getByRole("button", { name: "Answer" }).click();
      await expect(toast(page).filter({ hasText: "Answer recorded" })).toBeVisible();
      await page.goto("/inbox?filter=questions");
    };
    await answer(
      /triggers transition/,
      async () => {
        const choices = inspector(page).getByRole("radiogroup", { name: "Answer" });
        await inspector(page).getByRole("searchbox", { name: "Filter choices" }).fill("withdraw");
        // The choices are named: the Withdraw operation and the event it produces.
        await expect(choices.getByRole("radio")).toHaveCount(2);
        await expect(choices.getByRole("radio", { name: /^Leave withdrawn Event event:/ })).toBeVisible();
        await noSeriousA11yViolations(page);
        await choices.getByRole("radio", { name: /^Withdraw leave request Operation operation:/ }).click();
      },
      "HR-017 lets the employee withdraw a Submitted request.",
    );
    await answer(/configured holiday never consumes a working day/, () => inspector(page).getByRole("radio", { name: /calendar:office/ }).click(), "A configured public holiday consumes 0 working days.");
    await answer(
      /fractional input/,
      async () => {
        await inspector(page).getByRole("textbox", { name: "decimal" }).fill("0.5");
        await inspector(page).getByRole("textbox", { name: "unit" }).fill("working_day");
      },
      "A half day consumes 0.5 working day.",
    );

    // The three answers wait for confirmation. Each acceptance moves the model on, so the answers
    // left are out of date: each is refreshed against the current model before it is accepted.
    await page.goto("/inbox?filter=fromq");
    const answers = page.getByRole("list", { name: "From questions" }).getByRole("button");
    await expect(answers).toHaveCount(3);
    for (let left = 3; left > 0; left--) {
      await answers.first().click();
      const accept = inspector(page).getByRole("button", { name: "Accept" });
      if (left < 3) {
        await expect(answers.filter({ hasText: "Stale" })).toHaveCount(left);
        await expect(accept).toBeDisabled();
        await inspector(page).getByRole("button", { name: "Refresh proposal" }).click();
        await expect(toast(page).filter({ hasText: "Proposal refreshed against the current model" })).toBeVisible();
        await expect(answers.filter({ hasText: "Stale" })).toHaveCount(left - 1);
        await expect(inspector(page).getByRole("button", { name: "Refresh proposal" })).toHaveCount(0);
      }
      await expect(accept).toBeEnabled();
      await accept.click();
      await page.getByRole("dialog", { name: "Accept proposal" }).getByRole("textbox", { name: "Reason (required)" }).fill("Expert answer.");
      await page.keyboard.press("ControlOrMeta+Enter");
      await expect(answers).toHaveCount(left - 1);
    }
    await expect(conflictSheet(page)).toHaveCount(0);

    // Re-check: the Model gate's step (Analysis) runs again on the fixed model, and the gate passes.
    await page.goto("/quality/F2");
    await main(page).getByRole("region", { name: "Model F2" }).getByRole("button", { name: "Re-check" }).click();
    await expect(toast(page).filter({ hasText: "Analysis step finished" })).toBeVisible({ timeout: 30_000 });
    await expect(main(page).getByRole("heading", { level: 2, name: /^Model F2 Passes 0 blockers failed/ })).toBeVisible();
    await expect(model.getByText("State transitions are complete")).toBeVisible();
    await noSeriousA11yViolations(page);

    // The analysis proposes the missing transition the answer named; once it is accepted the
    // answer is reflected in the model and the Decisions gate passes too.
    await expect(main(page).getByRole("heading", { level: 2, name: /^Decisions F3 Blocked/ })).toBeVisible();
    await toast(page).getByRole("button", { name: "Review" }).click();
    await expect(page).toHaveURL(/\/inbox/);
    await expect(inspector(page).getByRole("heading", { level: 2, name: "Add transition submitted → withdrawn" })).toBeVisible();
    await inspector(page).getByRole("button", { name: "Accept" }).click();
    await expect(toast(page).filter({ hasText: /added to the model/ })).toBeVisible();
    await page.goto("/quality");
    for (const gate of ["Evidence I0", "Requirements F1", "Model F2", "Decisions F3", "Behaviour F4"]) {
      await expect(main(page).getByRole("heading", { level: 2, name: new RegExp(`^${gate} Passes`) })).toBeVisible();
    }
    expect(errors).toEqual([]);
  });

  test("flow 6: a failing scenario is traced, its expected outcome corrected, and it passes when run again", async ({ page }) => {
    test.setTimeout(120_000);
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto("/scenarios");
    const list = page.getByRole("list", { name: "Scenarios" });
    const details = page.getByRole("complementary", { name: "Scenario details" });
    const correct = async (then: "Yes" | "No") => {
      await details.getByRole("button", { name: "Correct expected outcome" }).click();
      const sheet = page.getByRole("dialog", { name: "Correct the expected outcome" });
      await sheet.getByRole("radiogroup", { name: "Then eligible" }).getByRole("radio", { name: then }).click();
      await noSeriousA11yViolations(page);
      await page.keyboard.press("ControlOrMeta+Enter");
      await expect(toast(page).filter({ hasText: "Expected outcome corrected" })).toBeVisible();
      await expect(sheet).toHaveCount(0);
    };

    // A contractor is not eligible for annual leave; someone recorded the opposite expectation.
    await list.getByRole("button", { name: /^Annual leave eligibility: Contractor → No/ }).click();
    await expect(details.getByText(/^Pass · /)).toBeVisible();
    await correct("Yes");
    await expect(details.getByRole("row", { name: "Then eligible is Yes" })).toBeVisible();
    await details.getByRole("button", { name: "Run this one" }).click();
    await expect(page.getByText(/^1 scenario ran · 0 pass$/)).toBeVisible({ timeout: 30_000 });

    // Filter Fail: the trace shows where the run diverged from the expectation.
    await page.getByRole("group", { name: "Filter scenarios" }).getByRole("button", { name: /^Fail 1$/ }).click();
    await expect(list.getByRole("button")).toHaveCount(1);
    await list.getByRole("button").first().click();
    await expect(details.getByText(/^Fail · /)).toBeVisible();
    const trace = details.getByRole("list", { name: "Trace" });
    const diverging = trace.locator("[data-diverging='true']");
    await expect(diverging).toHaveCount(1);
    await expect(diverging).toContainText("First difference");
    await expect(diverging).toContainText("Expected Yes, got No");
    // The rule it was derived from is one click away.
    await expect(details.getByRole("link", { name: "Decision table Annual leave eligibility" })).toHaveAttribute("href", /^\/model\/rules\/dt:/);
    await noSeriousA11yViolations(page);

    // The rule is right, the expectation was wrong: correct it back and run again.
    await correct("No");
    await details.getByRole("button", { name: "Run this one" }).click();
    await expect(page.getByText(/^1 scenario ran · 1 pass$/)).toBeVisible({ timeout: 30_000 });
    await expect(details.getByText(/^Pass · /)).toBeVisible();
    await expect(trace.getByText("Compared the outcome")).toBeVisible();
    expect(errors).toEqual([]);
  });
});

test.describe("Flow 9: Export", () => {
  test("flow 9: the Overview's last step leads to Ready — Export; the chosen artifacts download", async ({ page }) => {
    test.setTimeout(120_000);
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto("/overview");
    const card = main(page).getByRole("region", { name: "Next action" });
    // The pilot never ran the Directory step as a recipe: it is the one step left.
    await expect(card.getByText("Run the Directory step")).toBeVisible();
    await card.getByRole("button", { name: "Run step" }).click();
    await expect(toast(page).filter({ hasText: "Directory step finished" })).toBeVisible({ timeout: 30_000 });
    await expect(card.getByText("Your specification is ready")).toBeVisible();
    await expect(card.getByText("All five gates pass. Export the functional model, glossary, scenarios and reports.")).toBeVisible();
    await expect(banner(page).getByRole("button", { name: "Export" })).toBeVisible();
    await noSeriousA11yViolations(page);

    // The export sheet: the six artifacts, all chosen; one is left out.
    await card.getByRole("button", { name: "Export" }).click();
    const sheet = page.getByRole("dialog", { name: "Export specification" });
    await expect(sheet.getByRole("checkbox")).toHaveCount(6);
    await noSeriousA11yViolations(page);
    await sheet.getByRole("checkbox", { name: "Decisions" }).click();
    await expect(sheet.getByRole("checkbox", { name: "Decisions" })).not.toBeChecked();
    const downloads: Download[] = [];
    page.on("download", (download) => downloads.push(download));
    await sheet.getByRole("button", { name: /^Download/ }).click();
    await expect(toast(page).filter({ hasText: "5 files downloaded" })).toBeVisible();
    await expect(sheet).toHaveCount(0);
    await expect.poll(() => downloads.length).toBe(5);
    expect(downloads.map((d) => d.suggestedFilename()).sort()).toEqual(["functional-model.yaml", "gate-report.json", "glossary.md", "report.md", "scenarios.yaml"]);
    for (const download of downloads) {
      expect(await download.failure()).toBeNull();
      const path = await download.path();
      const content = readFileSync(path, "utf8");
      expect(content.length, download.suggestedFilename()).toBeGreaterThan(0);
      if (download.suggestedFilename() === "gate-report.json") {
        const gates = JSON.parse(content) as unknown;
        expect(JSON.stringify(gates)).toContain("F4");
      }
      if (download.suggestedFilename() === "glossary.md") {
        expect(content).toContain("leave request");
      }
    }
    expect(errors).toEqual([]);
  });
});

test.describe("System states", () => {
  test("the ⌘K palette finds a requirement and runs commands", async ({ page }) => {
    await page.goto("/overview");
    await expect(main(page).getByRole("heading", { level: 1, name: "HR" })).toBeVisible();
    await page.keyboard.press("ControlOrMeta+k");
    const palette = page.getByRole("dialog", { name: "Command palette" });
    await expect(palette).toBeVisible();
    await expect(palette.getByRole("option", { name: /Export specification/ })).toBeVisible();
    await noSeriousA11yViolations(page);
    await palette.getByRole("combobox", { name: "Command palette" }).fill("HR-009");
    const hit = palette.getByRole("group", { name: "Requirements" }).getByRole("option", { name: /^HR-009 Submitting a valid leave request/ });
    await expect(hit).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(palette).toHaveCount(0);
    await expect(page).toHaveURL(/\/requirements\/req:/);
    await expect(page.getByRole("complementary", { name: "Requirement details" })).toContainText("Submitting a valid leave request shall change its status from Draft to Submitted.");
    // A command: go to Scenarios.
    await page.keyboard.press("ControlOrMeta+k");
    await palette.getByRole("combobox", { name: "Command palette" }).fill("Go to Scenarios");
    await palette.getByRole("option", { name: /Go to Scenarios/ }).click();
    await expect(page).toHaveURL(/\/scenarios$/);
    // Esc closes it.
    await page.keyboard.press("ControlOrMeta+k");
    await expect(palette).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(palette).toHaveCount(0);
  });

  test("Settings switches to the dark appearance; screens stay accessible in dark", async ({ page }) => {
    test.setTimeout(120_000);
    await page.goto("/overview");
    await page.getByRole("complementary").getByRole("button", { name: "Settings" }).first().click();
    const settings = page.getByRole("dialog", { name: "Settings" });
    await expect(settings.getByTestId("settings-project")).toHaveText(/^HR · project:hr · v\d+$/);
    await expect(settings.getByText("Signed in")).toBeVisible();
    await noSeriousA11yViolations(page);
    await settings.getByRole("radio", { name: "Dark" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-appearance", "dark");
    await settings.getByRole("button", { name: "Close Settings" }).click();
    await expect(settings).toHaveCount(0);
    for (const route of ["/overview", "/inbox", "/requirements", "/model/entities", "/scenarios", "/quality/F1", "/sources"]) {
      await page.goto(route);
      await page.waitForLoadState("networkidle");
      // The choice is remembered across navigations.
      await expect(page.locator("html")).toHaveAttribute("data-appearance", "dark");
      await noSeriousA11yViolations(page);
    }
    // Back to light through the palette command.
    await page.keyboard.press("ControlOrMeta+k");
    await page.getByRole("dialog", { name: "Command palette" }).getByRole("option", { name: /Switch to light appearance/ }).click();
    await expect(page.locator("html")).toHaveAttribute("data-appearance", "light");
  });

  test("when the API goes away the connection banner shows, reads fail with Retry, and it recovers", async ({ page }) => {
    test.setTimeout(90_000);
    // The only route mocks of the suite: the server becomes unreachable, then comes back.
    let down = false;
    await page.routeWebSocket(/\/api\/events$/, (ws) => {
      if (down) {
        ws.close();
      } else {
        ws.connectToServer();
      }
    });
    await page.route(
      (url) => url.pathname.startsWith("/api/"),
      (route) => (down ? route.abort("connectionrefused") : route.continue()),
    );
    await page.goto("/requirements");
    await expect(main(page).getByTestId("requirement-row")).toHaveCount(32);
    const lost = page.getByRole("status").filter({ hasText: "Live connection lost. Reconnecting… Your data refreshes when it's back." });
    await expect(lost).toHaveCount(0);

    down = true;
    await page.reload();
    await expect(lost).toBeVisible({ timeout: 15_000 });
    await expect(main(page).getByRole("alert")).toContainText("The requirements could not be loaded. The server could not be reached.");
    await noSeriousA11yViolations(page);

    // Retry while the server is still away fails the same honest way.
    await main(page).getByRole("button", { name: "Retry" }).click();
    await expect(main(page).getByRole("alert")).toContainText("The server could not be reached.");

    // The server is back: the socket reconnects, the banner goes and the data refreshes by itself.
    down = false;
    await expect(lost).toHaveCount(0, { timeout: 45_000 });
    await expect(main(page).getByTestId("requirement-row")).toHaveCount(32);
    await expect(banner(page).getByRole("button", { name: /^HR/ })).toBeVisible();
  });
});

// The replay provider answers only recorded requests, keyed by their request hash. An Assistant
// turn's request names its new session and the conversation, so no recording can match it: with
// AI on (the replay provider), the turn fails honestly and nothing is proposed. The read-only
// state with AI off is in s1-overview-sources.spec.ts.
test.describe("Flow 7: the Assistant @empty-db", () => {
  test("with AI on but no answer available, the turn fails in plain language and nothing is proposed", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto("/overview");
    const waitingBefore = ((await (await page.request.get("/api/proposals")).json()) as { items: unknown[] }).items.length;
    await banner(page).getByRole("button", { name: "Assistant" }).click();
    const panel = page.getByRole("complementary", { name: "Assistant" });
    await expect(panel.getByText("Scope · Project")).toBeVisible();
    const box = panel.getByRole("textbox", { name: "Message the Assistant" });
    await expect(box).toBeEnabled();
    await noSeriousA11yViolations(page);
    await box.fill("Which requirements mention approval?");
    await expect(panel.getByRole("button", { name: "Send" })).toBeEnabled();
    await page.keyboard.press("Enter");
    await expect(panel.getByText("Which requirements mention approval?")).toBeVisible();
    await expect(panel.getByRole("alert")).toContainText("The AI model could not answer. Try again in a moment.");
    await expect(panel.getByRole("alert")).toContainText("E_INFERENCE_FAILED");
    await noSeriousA11yViolations(page);
    // Nothing was proposed: the Inbox does not grow.
    const waitingAfter = ((await (await page.request.get("/api/proposals")).json()) as { items: unknown[] }).items.length;
    expect(waitingAfter).toBe(waitingBefore);
    await panel.getByRole("button", { name: "Close the Assistant" }).click();
    await expect(panel).toHaveCount(0);
    expect(errors).toEqual([]);
  });
});
