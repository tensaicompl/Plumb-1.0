// S4: the Model and Scenarios screens against the real backend with the HR pilot data
// (scripts/run-pilot-e2e.sh). Runs after the pilot spec (workers: 1, file order) and asserts only
// visible state; axe runs on each screen and sheet.
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

async function noSeriousA11yViolations(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const serious = results.violations.filter((v) => v.impact === "critical" || v.impact === "serious");
  expect(serious.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
}

test.describe("Model", () => {
  test("segments, Coverage and Changes", async ({ page }) => {
    await page.goto("/model");
    await expect(page).toHaveURL(/\/model\/glossary$/);
    await expect(page.getByRole("heading", { level: 1, name: "Model" })).toBeVisible();
    await expect(page.getByRole("table", { name: "Glossary" }).getByRole("link", { name: "leave request", exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Coverage" }).click();
    const coverage = page.getByRole("dialog", { name: "Coverage" });
    await expect(coverage.getByRole("rowheader", { name: "Submit leave request" })).toBeVisible();
    await noSeriousA11yViolations(page);
    await coverage.getByRole("button", { name: "Close" }).click();
    await page.getByRole("button", { name: /^Changes/ }).click();
    await expect(page.getByRole("dialog", { name: "Changes since your last visit" }).getByText(/^Version \d+ → \d+ · /)).toBeVisible();
    await page.keyboard.press("Escape");
    for (const segment of ["Entities", "Rules", "Calculations", "Processes"]) {
      await page.getByRole("navigation", { name: "Model views" }).getByRole("link", { name: segment }).click();
      await expect(page).toHaveURL(new RegExp("/model/" + segment.toLowerCase()));
      await page.waitForLoadState("networkidle");
      await noSeriousA11yViolations(page);
    }
  });

  test("the glossary term inspector edits aliases", async ({ page }) => {
    await page.goto("/model/glossary");
    await page.getByRole("link", { name: "contractor", exact: true }).click();
    const inspector = page.getByRole("complementary", { name: "Term details" });
    await expect(inspector.getByRole("heading", { name: "contractor" })).toBeVisible();
    await inspector.getByRole("button", { name: "Edit aliases" }).click();
    const sheet = page.getByRole("dialog", { name: "Edit aliases" });
    await sheet.getByLabel("Aliases").fill("contractors");
    await sheet.getByRole("button", { name: /Save/ }).click();
    await expect(page.getByText("Aliases saved")).toBeVisible();
    await expect(inspector.getByText("contractors", { exact: true })).toBeVisible();
  });

  test("the leave request states and the worked example come from the model", async ({ page }) => {
    await page.goto("/model/entities");
    const diagram = page.getByRole("img", { name: "States of Leave request" });
    await expect(diagram.getByText("Draft", { exact: true })).toBeVisible();
    await expect(diagram.getByText("Submit leave request")).toBeVisible();
    await page.goto("/model/calculations");
    await expect(page.getByRole("group", { name: "Worked example" }).getByText(/^→ \d+ working days?$/)).toBeVisible();
    await expect(page.getByText("In plain language")).toBeVisible();
  });

  test("a process is imported from Mermaid, edited and exported", async ({ page }) => {
    const crud = (await (await page.request.get("/api/model/crud")).json()) as { operations: string[] };
    const ops = crud.operations.slice(0, 3);
    const text = ["flowchart TD", '%% plumb-process-name: "S4 approval"', "  start((Start))", ...ops.map((op, i) => `  step_${String(i).padStart(4, "0")}["${op}"]`), "  end((End))"];
    const order = ["start", ...ops.map((_, i) => `step_${String(i).padStart(4, "0")}`), "end"];
    order.slice(1).forEach((to, i) => text.push(`  ${order[i]} --> ${to}`));
    await page.goto("/model/processes");
    await page.getByRole("button", { name: /^Import (from )?Mermaid$/ }).first().click();
    const sheet = page.getByRole("dialog", { name: "Import a process" });
    await sheet.getByLabel("Mermaid text").fill(text.join("\n") + "\n");
    await sheet.getByRole("button", { name: /Import/ }).click();
    await expect(page.getByText("Process imported")).toBeVisible();
    const canvas = page.getByRole("region", { name: "S4 approval" });
    await expect(canvas.getByRole("button", { name: "Start event" })).toBeVisible();
    await noSeriousA11yViolations(page);
    await page.getByRole("button", { name: "Edit steps" }).click();
    const steps = page.getByRole("dialog", { name: "Edit steps" });
    await steps.getByRole("button", { name: /^Move .* down$/ }).first().click();
    await expect(page.getByText("Step moved")).toBeVisible();
    await page.keyboard.press("Escape");
    const download = page.waitForEvent("download");
    await canvas.getByRole("button", { name: "Export" }).click();
    expect((await download).suggestedFilename()).toBe("s4-approval.mmd");
  });
});

test.describe("Scenarios", () => {
  test("a scenario shows its facts, verdict and trace and runs alone", async ({ page }) => {
    await page.goto("/scenarios");
    await expect(page.getByRole("heading", { level: 1, name: "Scenarios" })).toBeVisible();
    const list = page.getByRole("list", { name: "Scenarios" });
    await list.getByRole("button", { name: /^Submit leave request: Draft → Submitted/ }).click();
    const inspector = page.getByRole("complementary", { name: "Scenario details" });
    await expect(inspector.getByRole("table", { name: "Given, when, then" })).toContainText("the state is Draft");
    await expect(inspector.getByText(/^Pass · /)).toBeVisible();
    await expect(inspector.getByRole("list", { name: "Trace" })).toContainText("Moved from Draft to Submitted");
    await noSeriousA11yViolations(page);
    await inspector.getByRole("button", { name: "Run this one" }).click();
    await expect(page.getByText(/^1 scenario ran · \d pass$/)).toBeVisible();
  });

  test("an undecidable scenario lists the missing semantics with a link to the rule", async ({ page }) => {
    await page.goto("/scenarios");
    await page.getByRole("button", { name: /^Undecidable/ }).click();
    await page.getByRole("list", { name: "Scenarios" }).getByRole("button").first().click();
    const missing = page.getByRole("list", { name: "Missing semantics" });
    await expect(missing).toBeVisible();
    await missing.getByRole("link").first().click();
    await expect(page).toHaveURL(/\/model\/rules\/rule:/);
  });
});
