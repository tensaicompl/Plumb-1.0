// S3: the Requirements and Quality screens against the real backend with the HR pilot data
// (scripts/run-pilot-e2e.sh builds the database with product operations only). This file runs
// after pilot.spec.ts (one worker): its last flows write (record findings, waive one).
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

async function noSeriousA11yViolations(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const serious = results.violations.filter((v) => v.impact === "critical" || v.impact === "serious");
  expect(serious.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
}

const main = (page: Page) => page.getByRole("main");
const rows = (page: Page) => main(page).getByTestId("requirement-row");

test.describe.configure({ mode: "serial" });

test.describe("Requirements", () => {
  test("lists the 32 HR requirements in document order and filters them", async ({ page }) => {
    await page.goto("/requirements");
    await expect(rows(page)).toHaveCount(32);
    await expect(rows(page).first()).toContainText("HR-001");
    await expect(rows(page).last()).toContainText("HR-032");
    await main(page).getByRole("button", { name: "Kind" }).click();
    await page.getByRole("menuitem", { name: "Quality" }).click();
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page).first()).toContainText("HR-030");
    await main(page).getByRole("button", { name: "Kind: Quality" }).click();
    await page.getByRole("menuitem", { name: "All kinds" }).click();
    await main(page).getByRole("button", { name: "Has issues" }).click();
    expect(await rows(page).count()).toBeGreaterThan(0);
    await noSeriousA11yViolations(page);
  });

  test("opens a requirement with its readiness, evidence, links and Open in source", async ({ page }) => {
    await page.goto("/requirements");
    await rows(page).filter({ hasText: "HR-009" }).getByRole("button").click();
    await expect(page).toHaveURL(/\/requirements\/req:/);
    const inspector = page.getByRole("complementary", { name: "Requirement details" });
    await expect(inspector.getByRole("paragraph").filter({ hasText: "Submitting a valid leave request shall change its status from Draft to Submitted." })).toBeVisible();
    await expect(inspector.getByRole("region", { name: "Readiness" })).toContainText("Clarity");
    const evidence = inspector.getByRole("figure", { name: "Evidence from requirements.md" });
    await expect(evidence.locator("mark")).toHaveText("Submitting a valid leave request shall change its status from Draft to Submitted.");
    await expect(inspector.getByRole("region", { name: "Linked" })).toBeVisible();
    await noSeriousA11yViolations(page);
    await evidence.getByRole("button", { name: "Open in source" }).click();
    await expect(page).toHaveURL(/\/sources\/src:/);
  });

  test("previews a split on the server without committing it", async ({ page }) => {
    await page.goto("/requirements");
    await rows(page).filter({ hasText: "HR-032" }).getByRole("button").click();
    const inspector = page.getByRole("complementary", { name: "Requirement details" });
    await inspector.getByRole("button", { name: "More actions" }).click();
    await page.getByRole("menuitem", { name: "Split into two…" }).click();
    const sheet = page.getByRole("dialog", { name: "Split HR-032 into two" });
    await sheet.getByRole("textbox", { name: "Part 1" }).fill("The system shall create an audit record for every approval and rejection decision.");
    await sheet.getByRole("textbox", { name: "Part 2" }).fill("The system shall create an audit record for every withdrawal and cancellation decision.");
    await sheet.getByRole("textbox", { name: "Reason (required)" }).fill("Two audit obligations.");
    await sheet.getByRole("button", { name: "Preview split" }).click();
    await expect(sheet.getByRole("region", { name: "Resulting requirements" })).toContainText("withdrawal and cancellation");
    await sheet.getByRole("button", { name: "Cancel" }).click();
    await expect(rows(page)).toHaveCount(32);
  });
});

test.describe("Quality", () => {
  test("shows the five gates and the waived duplicate in Requirements (F1)", async ({ page }) => {
    await page.goto("/quality");
    for (const name of ["Evidence", "Requirements", "Model", "Decisions", "Behaviour"]) {
      await expect(main(page).getByRole("button", { name: new RegExp(name) })).toBeVisible();
    }
    await main(page).getByRole("button", { name: /Requirements/ }).click();
    await expect(page).toHaveURL(/\/quality\/F1$/);
    const rules = main(page).getByRole("region", { name: "Requirements F1" });
    await expect(rules.getByText(/waived by .*HR-023 and HR-024 notify different outcomes/)).toBeVisible();
    await expect(rules.getByText("HR-023", { exact: true })).toBeVisible();
    await noSeriousA11yViolations(page);
    // Flow 4: Show affected filters Requirements to the affected items.
    await rules.getByRole("button", { name: "Show affected" }).click();
    await expect(page).toHaveURL(/\/requirements\?affected=/);
    expect(await rows(page).count()).toBeGreaterThan(0);
  });

  test("lists the decision history in Europe/Warsaw time", async ({ page }) => {
    await page.goto("/quality/history");
    await expect(page.getByText("Times in Europe/Warsaw")).toBeVisible();
    await page.getByRole("searchbox", { name: "Search decisions" }).fill("notify different outcomes");
    await expect(page.getByRole("table", { name: "Decision history" }).getByText("Waived a finding").first()).toBeVisible();
    await noSeriousA11yViolations(page);
  });

  test("flow 5: records the gates' findings and waives one with a reason", async ({ page }) => {
    await page.goto("/quality/issues");
    const issues = page.getByRole("list", { name: "Issues" });
    await expect(issues).toBeVisible();
    await noSeriousA11yViolations(page);
    await page.getByRole("button", { name: "Record findings" }).click();
    await expect(page.getByText("Findings recorded")).toBeVisible();
    const finding = issues.getByRole("listitem").filter({ hasText: "acceptance criteria" }).filter({ has: page.getByRole("button", { name: "Waive with reason" }) }).first();
    await finding.getByRole("button", { name: "Waive with reason" }).click();
    const sheet = page.getByRole("dialog", { name: "Waive this finding" });
    await sheet.getByRole("textbox").fill("Acceptance criteria follow in the next release.");
    await sheet.getByRole("button", { name: /Waive/ }).click();
    await expect(page.getByText("Finding waived · recorded in History")).toBeVisible();
    await expect(issues.getByText(/Waived by .*“Acceptance criteria follow in the next release\.”/)).toBeVisible();
  });
});
