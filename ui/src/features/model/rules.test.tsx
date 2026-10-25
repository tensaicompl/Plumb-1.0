// Model · Rules (handoff "4. Model" Rules, spec §5.4.3): the rule list with gap and overlap badges,
// the decision-table grid (conditions as columns, rows when → then), highlighted overlaps, and
// uncovered points as dashed "Uncovered · Set outcome" cells that set the outcome with a reason
// (set_rule_cell `uncovered:<k>:output:0`).
import { screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";

import { axeViolations } from "../../test/axe";
import { modelBase, RULES, WRITE_OK } from "../../test/modelFixtures";
import { etagJson, TEST_ETAG } from "../../test/msw";
import { setupRouteHarness } from "../../test/routeHarness";
import { describeInputCell, describeOutputCell } from "./ruleCells";

const h = setupRouteHarness();

const rules = (items = RULES, health: unknown[] = []) => http.get("*/api/model/rules", () => etagJson({ items, health, next_cursor: null }));
const me = (roles: string[]) => http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles, inference_available: true }));

describe("Rule list and decision table", () => {
  it("lists rules by name with their coverage badge and shows the first table", async () => {
    await h.renderAt("/model/rules", ...modelBase, rules());
    const list = await screen.findByRole("navigation", { name: "Rules" });
    const links = within(list).getAllByRole("link");
    await waitFor(() => expect(links[0]?.textContent).toBe("Annual leave eligibility1 gap · 1 overlap"));
    expect(links[1]?.textContent).toBe("Manager decision authorization");
    expect(links[2]?.textContent).toBe("Approval routingComplete");
    const detail = screen.getByRole("region", { name: "Annual leave eligibility" });
    expect(within(detail).getByText("Decision table · 2 conditions · 1 outcome")).toBeTruthy();
    const table = within(detail).getByRole("table", { name: "Annual leave eligibility" });
    expect(within(table).getAllByRole("columnheader").map((c) => c.textContent)).toEqual(["When employment type", "and leave type", "Then eligible"]);
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows[0]?.textContent).toBe("EmployeeAnyYes");
    // Overlapping rows say so in words, not only colour.
    expect(within(rows[1] as HTMLElement).getByText("Overlaps with row 3")).toBeTruthy();
    expect(within(rows[2] as HTMLElement).getByText("Overlaps with row 2")).toBeTruthy();
    expect(within(rows[3] as HTMLElement).getByRole("button", { name: "Uncovered · Set outcome" })).toBeTruthy();
    expect(rows[3]?.textContent).toContain("Contractor");
    expect(rows[3]?.textContent).toContain("Sick");
    expect(await axeViolations()).toEqual([]);
  });

  it("shows a scalar rule's condition instead of a grid", async () => {
    const { user } = await h.renderAt("/model/rules", ...modelBase, rules());
    await user.click(await screen.findByRole("link", { name: "Manager decision authorization" }));
    const detail = await screen.findByRole("region", { name: "Manager decision authorization" });
    expect(within(detail).getByText("actor == direct_manager(employee)")).toBeTruthy();
    expect(within(detail).queryByRole("table")).toBeNull();
  });

  it("lists a rule the server could not project as a row error while the rest works", async () => {
    await h.renderAt("/model/rules", ...modelBase, rules(RULES, [{ id: "dt:broken", code: "E_DECISION_TABLE_ENCODING_INVALID", message: "malformed" }]));
    expect(await screen.findByText("Couldn't load dt:broken.")).toBeTruthy();
    expect(screen.getByRole("region", { name: "Annual leave eligibility" })).toBeTruthy();
  });

  it("shows the empty state", async () => {
    await h.renderAt("/model/rules", ...modelBase, rules([]));
    expect(await screen.findByRole("heading", { name: "No rules yet" })).toBeTruthy();
  });
});

describe("Set outcome", () => {
  it("sets an uncovered outcome with a reason and the reviewed ETag", async () => {
    const { user } = await h.renderAt("/model/rules", ...modelBase, rules(), http.post("*/api/model/rules/:id/cells", () => etagJson(WRITE_OK)));
    await user.click(await screen.findByRole("button", { name: "Uncovered · Set outcome" }));
    const sheet = await screen.findByRole("dialog", { name: "Set outcome · Contractor × Sick" });
    const confirm = within(sheet).getByRole("button", { name: /Set outcome/ });
    expect((confirm as HTMLButtonElement).disabled).toBe(true);
    await user.click(within(sheet).getByRole("radio", { name: "No" }));
    await user.type(within(sheet).getByLabelText("Reason (required)"), "Contractors are not employees.");
    await user.click(confirm);
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({
      path: "/api/model/rules/dt:eligibility/cells",
      ifMatch: TEST_ETAG,
      body: { cell: "uncovered:0:output:0", outcome: '{"cell":"bool","value":false}', reason: "Contractors are not employees." },
    });
    expect(await screen.findByText("Rule updated · decision recorded")).toBeTruthy();
  });

  it("offers no Set outcome to a role that cannot change the model", async () => {
    await h.renderAt("/model/rules", ...modelBase, rules(), me(["reviewer"]));
    await screen.findByRole("region", { name: "Annual leave eligibility" });
    expect(screen.queryByRole("button", { name: "Uncovered · Set outcome" })).toBeNull();
    expect(screen.getByText("Uncovered")).toBeTruthy();
  });
});

describe("Cell wording", () => {
  it("reads input and output cells in plain words", () => {
    expect(describeInputCell('{"cell":"any"}')).toBe("Any");
    expect(describeInputCell('{"cell":"bool","value":true}')).toBe("Yes");
    expect(describeInputCell('{"cell":"enum_set","members":["Annual","Sick"]}')).toBe("Annual or Sick");
    expect(describeInputCell('{"cell":"interval","interval":{"lower":{"inclusive":false,"value":"5"},"upper":{"inclusive":true,"value":"10"}}}')).toBe("> 5 and ≤ 10");
    expect(describeInputCell("not json")).toBe("not json");
    expect(describeOutputCell('{"cell":"enum","member":"Manager"}')).toBe("Manager");
    expect(describeOutputCell('{"cell":"int","value":3}')).toBe("3");
    expect(describeOutputCell('{"cell":"bool","value":false}')).toBe("No");
  });
});
