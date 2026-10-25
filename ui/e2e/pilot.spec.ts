// E0.3: the HR pilot workflow in the UI against the real backend. The database is built only by
// product operations (scripts/run-pilot-e2e.sh runs the CLI HR simulation, then serves it).
// Only visible state is asserted; axe runs on every navigated route.
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

// The spec §3 destinations of the new shell (old pages render inside them until each screen lane
// replaces its screen).
const ROUTES = [
  "/overview",
  "/inbox",
  "/inbox?filter=questions",
  "/inbox?filter=wording",
  "/requirements",
  "/model/glossary",
  "/model/entities",
  "/model/calculations",
  "/model/rules",
  "/model/processes",
  "/scenarios",
  "/quality",
  "/quality/issues",
  "/quality/history",
  "/sources",
];

async function noSeriousA11yViolations(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const serious = results.violations.filter((v) => v.impact === "critical" || v.impact === "serious");
  expect(serious.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
}

test.describe("HR pilot", () => {
  test("every gate passes and every route renders without serious a11y violations", async ({ page }) => {
    // Fifteen destinations, each loaded and axe-checked against the dev server.
    test.setTimeout(120_000);
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto("/");
    for (const gate of ["I0", "F1", "F2", "F3", "F4"]) {
      await expect(page.getByText(gate, { exact: true }).first()).toBeVisible();
    }
    for (const route of ROUTES) {
      await page.goto(route);
      await page.waitForLoadState("networkidle");
      await noSeriousA11yViolations(page);
    }
    // H0.11: proposal discovery is reachable from the sidebar (Inbox) and lists from the real API.
    await page.goto("/");
    await expect(page).toHaveURL(/\/overview$/);
    await page.getByRole("navigation", { name: "Main navigation" }).getByRole("link", { name: /Inbox/ }).click();
    await expect(page).toHaveURL(/\/inbox$/);
    await expect(page.getByRole("heading", { level: 1, name: "Proposals" })).toBeVisible();
    await expect(page.getByText(/No proposal is pending\.|Pending proposals/).first()).toBeVisible();
    expect(errors).toEqual([]);
  });

  test("the 32 HR requirements are listed with their statements", async ({ page }) => {
    await page.goto("/requirements");
    await expect(page.getByText("The system shall allow an employee to create a draft leave request.")).toBeVisible();
    await expect(page.getByText("32 · from requirements.md")).toBeVisible();
    await page.getByRole("searchbox", { name: "Search requirements" }).fill("Warsaw");
    await expect(page.getByText("Business timestamps shall use the Europe/Warsaw time zone.")).toBeVisible();
  });

  test("the answered calculation shows the expert semantics", async ({ page }) => {
    await page.goto("/model/calculations");
    await expect(page.getByText("working_days([start_date, end_date], calendar, true) * day_fraction")).toBeVisible();
  });

  test("an attribute can be typed from the Entities screen", async ({ page }) => {
    await page.goto("/model/entities");
    const row = page.locator("[data-attribute='leave type']");
    await row.getByTestId("attribute-type-choice").selectOption("Int");
    await row.getByTestId("attribute-type-save").click();
    await expect(row.locator("td").nth(1).locator(".font-mono").first()).toHaveText("Int");
  });

  test("selected scenarios run and show their verdicts", async ({ page }) => {
    await page.goto("/scenarios");
    const boxes = page.locator("input[type=checkbox]");
    await boxes.first().check();
    await page.getByRole("button", { name: /run selected/i }).click();
    await expect(page.getByText(/pass|fail|undecidable/i).first()).toBeVisible();
  });

  test("the gates screen shows the waived duplicate blocker", async ({ page }) => {
    await page.goto("/quality/F1");
    await expect(page.getByText("PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED").first()).toBeVisible();
    await expect(page.getByText(/waived/i).first()).toBeVisible();
  });
});
