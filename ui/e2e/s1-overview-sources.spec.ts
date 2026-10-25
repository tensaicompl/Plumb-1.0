// S1: the Overview, Sources, the Assistant panel and Settings against the real backend with the HR
// pilot data (scripts/run-pilot-e2e.sh builds and serves it; the pilot runs without an AI
// provider). Nothing here changes the database: the import check re-uploads the pilot document,
// which the server refuses as already imported.
import { fileURLToPath } from "node:url";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const PILOT_DOCUMENT = fileURLToPath(new URL("../../fixtures/hr-leave/requirements.md", import.meta.url));

async function noSeriousA11yViolations(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const serious = results.violations.filter((v) => v.impact === "critical" || v.impact === "serious");
  expect(serious.map((v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(" ")).join(", ")})`)).toEqual([]);
}

test.describe("S1 Overview, Sources, Assistant and Settings", () => {
  test("the Overview shows the project, the pipeline, the gates, at a glance and recent decisions", async ({ page }) => {
    await page.goto("/overview");
    const main = page.getByRole("main");
    await expect(main.getByRole("heading", { level: 1, name: "HR" })).toBeVisible();
    await expect(main.getByText(/^Updated .* by cli · 32 requirements from requirements\.md$/)).toBeVisible();
    // The pilot runs without an AI provider.
    await expect(main.getByRole("note")).toContainText("AI analysis is off.");
    await expect(main.getByRole("region", { name: "Next action" })).toBeVisible();
    const pipeline = main.getByRole("region", { name: "Pipeline" });
    await expect(pipeline.getByRole("link")).toHaveCount(9);
    await expect(pipeline.locator("[data-step='import']")).toHaveAttribute("data-step-state", "done");
    await expect(pipeline.locator("[data-step='requirements']")).toHaveAttribute("data-step-state", "done");
    const gates = main.getByRole("region", { name: "Quality gates" });
    await expect(gates.getByText("Passes")).toHaveCount(5);
    const glance = main.getByRole("region", { name: "At a glance" });
    await expect(glance.getByRole("link", { name: /Requirements/ })).toContainText("32");
    await expect(main.getByRole("region", { name: "Recent decisions" }).getByRole("listitem")).toHaveCount(5);
    await noSeriousA11yViolations(page);
    // Each step opens what it produced.
    await pipeline.locator("[data-step='import']").click();
    await expect(page).toHaveURL(/\/sources$/);
  });

  test("Sources reads the pilot document with the passages that became requirements", async ({ page }) => {
    await page.goto("/sources");
    const main = page.getByRole("main");
    await expect(main.getByRole("heading", { level: 1, name: "Sources" })).toBeVisible();
    await expect(main.getByRole("list", { name: "Documents" }).getByRole("link")).toContainText(["requirements.md"]);
    const reading = main.getByRole("article");
    await expect(reading.getByText("requirements.md · 38 passages · 32 became requirements")).toBeVisible();
    await expect(reading.getByRole("heading", { name: "HR Leave Management — Pilot Requirements" })).toBeVisible();
    const passage = reading.getByRole("link", { name: /HR-001 \[functional\] The system shall allow an employee to create a draft leave request\./ });
    await passage.hover();
    await expect(page.getByRole("tooltip")).toContainText("Became requirement HR-001 · click to open");
    await noSeriousA11yViolations(page);
    await passage.click();
    await expect(page).toHaveURL(/\/requirements\/req:/);
  });

  test("importing the pilot document again is refused in plain language", async ({ page }) => {
    await page.goto("/sources");
    const main = page.getByRole("main");
    const input = main.getByLabel("Import document");
    // The zone accepts a file once the model version it is imported against has loaded.
    await expect(input).toBeEnabled();
    await input.setInputFiles(PILOT_DOCUMENT);
    await expect(main.getByRole("alert")).toContainText("This file is already imported");
    await expect(main.getByRole("alert")).toContainText("E_SOURCE_EXISTS");
  });

  test("Settings shows the project, its database and the AI status; the toolbar names the project", async ({ page }) => {
    await page.goto("/overview");
    await expect(page.getByRole("banner").getByRole("button", { name: /^HR/ })).toBeVisible();
    await page.getByRole("banner").getByRole("button", { name: /^HR/ }).click();
    const dialog = page.getByRole("dialog", { name: "Settings" });
    await expect(dialog.getByTestId("settings-project")).toHaveText(/^HR · project:hr · v\d+$/);
    await expect(dialog.getByText(/plumb\.sqlite$/)).toBeVisible();
    await expect(dialog.getByText("Off — no API key configured")).toBeVisible();
  });

  test("the Assistant opens read-only while AI is off", async ({ page }) => {
    await page.goto("/overview");
    await page.getByRole("banner").getByRole("button", { name: "Assistant" }).click();
    const panel = page.getByRole("complementary", { name: "Assistant" });
    await expect(panel.getByText("Scope · Project")).toBeVisible();
    await expect(panel.getByText("AI is off, so the Assistant is read-only. Add an API key in Settings.")).toBeVisible();
    await expect(panel.getByRole("textbox", { name: "Message the Assistant" })).toBeDisabled();
    await noSeriousA11yViolations(page);
  });
});
