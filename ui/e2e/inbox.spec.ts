// S2: the Inbox triage against the real backend (spec §6 flows 1 and 2). Runs on the Inbox
// database of scripts/run-pilot-e2e.sh (tag @inbox-db): the HR document imported and the
// Requirements step replayed, so its 32 proposals wait for review. The steps share that database,
// so they run in order in one test.
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const queue = (page: Page) => page.getByRole("list", { name: "All items" });
const rows = (page: Page) => queue(page).getByRole("button");
const inspector = (page: Page) => page.getByRole("complementary", { name: "Item details" });
const toast = (page: Page) => page.locator("[data-toast]");
const waiting = (page: Page, n: number) => expect(page.getByText(`${n} waiting`, { exact: true })).toBeVisible();

async function noSeriousA11yViolations(page: Page) {
  // Colour contrast is measured on settled pixels: wait for the inspector slide-in and the toast.
  await page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== "running"));
  await page.mouse.move(0, 0);
  const results = await new AxeBuilder({ page }).analyze();
  const serious = results.violations.filter((v) => v.impact === "critical" || v.impact === "serious");
  expect(serious.map((v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(" ")).join(", ")})`)).toEqual([]);
}

test.describe("Inbox triage @inbox-db", () => {
  test("reviews the Requirements step: inspect, reject with a reason, accept two together, then the rest is out of date", async ({ page }) => {
    test.setTimeout(120_000);
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));

    // The sidebar badge counts exactly what the Inbox lists.
    await page.goto("/inbox");
    await waiting(page, 32);
    await expect(rows(page)).toHaveCount(32);
    const badge = page.getByRole("navigation", { name: "Main navigation" }).getByRole("link", { name: /Inbox/ }).locator("[data-badge]");
    await expect(badge).toHaveText("32");
    const filters = page.getByRole("navigation", { name: "Inbox filters" });
    await expect(filters.getByRole("link", { name: /^Proposals/ }).locator("[data-count]")).toHaveText("32");
    await expect(filters.getByRole("link", { name: /^From steps/ }).locator("[data-count]")).toHaveText("32");
    await expect(page.getByRole("button", { name: "Accept all from Requirements" })).toBeVisible();

    // The first proposal is open in the inspector with its facts, evidence and model change.
    await expect(rows(page).first()).toHaveAttribute("aria-current", "true");
    await expect(inspector(page).getByText("New requirement · Requirements step")).toBeVisible();
    await expect(inspector(page).getByRole("heading", { level: 2, name: "Add requirement HR-001" })).toBeVisible();
    await expect(inspector(page).getByRole("list", { name: "Facts" })).toBeVisible();
    await expect(inspector(page).getByRole("figure", { name: "Evidence from requirements.md" })).toBeVisible();
    await expect(inspector(page).getByRole("list", { name: "What changes in the model" })).toContainText("requirement HR-001");
    await noSeriousA11yViolations(page);

    // Space toggles the evidence.
    await page.keyboard.press("Space");
    await expect(inspector(page).getByRole("figure")).toHaveCount(0);
    await page.keyboard.press("Space");
    await expect(inspector(page).getByRole("figure", { name: "Evidence from requirements.md" })).toBeVisible();

    // R opens the reason sheet; ⌘↩ confirms; the row goes and focus moves to the next one.
    await page.keyboard.press("r");
    const sheet = page.getByRole("dialog", { name: "Reject proposal" });
    await expect(sheet).toBeVisible();
    await sheet.getByRole("textbox").fill("Not in scope for this release.");
    await page.keyboard.press("ControlOrMeta+Enter");
    await expect(toast(page)).toContainText("Proposal rejected");
    await waiting(page, 31);
    await expect(page).toHaveURL(/\/inbox\/prop:/);
    await expect(rows(page).first()).toBeFocused();
    await expect(inspector(page).getByRole("heading", { level: 2, name: "Add requirement HR-002" })).toBeVisible();

    // J moves down, K back up.
    await page.keyboard.press("j");
    await expect(inspector(page).getByRole("heading", { level: 2, name: "Add requirement HR-003" })).toBeVisible();
    await page.keyboard.press("k");
    await expect(inspector(page).getByRole("heading", { level: 2, name: "Add requirement HR-002" })).toBeVisible();

    // ⇧ click selects a second row; both are accepted together as one change (with confirmation).
    await rows(page).nth(1).click({ modifiers: ["Shift"] });
    await expect(inspector(page).getByText("items selected")).toBeVisible();
    await expect(inspector(page).getByText("Accepted together as one change. If any item is out of date, none are applied.")).toBeVisible();
    await inspector(page).getByRole("button", { name: "Accept 2" }).click();
    await expect(toast(page)).toContainText("2 items accepted together");
    await waiting(page, 29);
    await expect(badge).toHaveText("29");

    // The accept moved the model on: the remaining proposals were built on the older version, so
    // they are stale; Accept is disabled and the step offers no bulk acceptance any more.
    await expect(rows(page).first()).toContainText("Stale");
    await expect(inspector(page).getByText(/Proposed on an older version \(v2\)/)).toBeVisible();
    await expect(inspector(page).getByRole("button", { name: "Accept" })).toBeDisabled();
    await expect(page.getByRole("button", { name: "Accept all from Requirements" })).toHaveCount(0);
    await page.keyboard.press("a");
    await waiting(page, 29);

    // A stale proposal can still be rejected.
    await inspector(page).getByRole("button", { name: "Reject" }).click();
    await page.getByRole("dialog", { name: "Reject proposal" }).getByRole("button", { name: /^Reject/ }).click();
    await expect(toast(page)).toContainText("Proposal rejected");
    await waiting(page, 28);
    await noSeriousA11yViolations(page);

    // The two accepted requirements are in the model, confirmed; the rejected one is not.
    const requirements = (await (await page.request.get("/api/requirements")).json()) as { items: { text: string; status: string }[] };
    expect(requirements.items).toHaveLength(2);
    expect(requirements.items.every((r) => r.status === "Accepted")).toBe(true);
    expect(requirements.items.map((r) => r.text)).not.toContain("The system shall allow an employee to create a draft leave request.");
    expect(errors).toEqual([]);
  });
});
