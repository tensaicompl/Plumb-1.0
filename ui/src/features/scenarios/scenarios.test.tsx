// Scenarios (handoff "5. Scenarios", spec §5.5, flow 6): the list with titles read from the
// scenario facts, source, confirmation and verdict pills; the inspector with the Given/When/Then
// card, the verdict callout, the trace timeline (get_scenario_run) with the first diverging step
// and the missing semantics, "Derived from"; Confirm, Correct expected outcome, Run this one, Run
// all and Confirm all derived; and the not run, running and stale run states.
import { focusManager } from "@tanstack/react-query";
import { act, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";

import { axeViolations } from "../../test/axe";
import { labelsHandler, RULES, WRITE_OK } from "../../test/modelFixtures";
import { etagJson, TEST_ETAG } from "../../test/msw";
import { setupRouteHarness } from "../../test/routeHarness";
import { SCENARIOS } from "../../test/scenarioFixtures";

const h = setupRouteHarness();

type Scenario = (typeof SCENARIOS)[number];

function backend(items: readonly Scenario[] = SCENARIOS) {
  return [
    labelsHandler,
    http.get("*/api/model/rules", () => etagJson({ items: RULES, health: [], next_cursor: null })),
    http.get("*/api/scenarios", () => etagJson({ items, health: [], next_cursor: null })),
    http.get("*/api/scenarios/:id/run", ({ params }) => {
      const run = items.find((item) => item.id === params["id"])?.run ?? null;
      return run === null ? etagJson({ code: "E_NOT_FOUND", message: "no run", details: {} }, { status: 404 }) : etagJson(run);
    }),
  ];
}
const ok = (path: string) => http.post(path, () => etagJson(WRITE_OK));
const me = (roles: string[]) => http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles, inference_available: true }));
const row = (title: string) => screen.getByRole("button", { name: new RegExp("^" + title.replace(/[()]/g, "\\$&")) }).closest("li") as HTMLElement;

describe("Scenario list", () => {
  it("reads each scenario's title and source from its facts and shows its pills", async () => {
    await h.renderAt("/scenarios", ...backend());
    expect(await screen.findByRole("heading", { level: 1, name: "Scenarios" })).toBeTruthy();
    expect(await screen.findByText("4 · 1 pass · 1 fail · 1 undecidable · 1 not run")).toBeTruthy();
    const list = await screen.findByRole("list", { name: "Scenarios" });
    await waitFor(() => expect(within(list).getByText("Submit leave request: Draft → Submitted")).toBeTruthy());
    const submit = row("Submit leave request: Draft → Submitted");
    expect(within(submit).getByText("Transition Draft → Submitted")).toBeTruthy();
    expect(within(submit).getByText("Confirmed")).toBeTruthy();
    expect(within(submit).getByText("Pass")).toBeTruthy();
    const contractor = row("Annual leave eligibility: Contractor, Annual → No");
    expect(within(contractor).getByText("Decision table Annual leave eligibility")).toBeTruthy();
    expect(within(contractor).getByText("Derived")).toBeTruthy();
    expect(within(contractor).getByText("Not run")).toBeTruthy();
    expect(within(list).getByText("Fail")).toBeTruthy();
    expect(within(list).getByText("Undecidable")).toBeTruthy();
    expect(screen.getByRole("complementary", { name: "Scenario details" }).textContent).toContain("Select a scenario");
    expect(await axeViolations()).toEqual([]);
  });

  it("filters by verdict", async () => {
    const { user } = await h.renderAt("/scenarios", ...backend());
    await user.click(await screen.findByRole("button", { name: /^Fail/ }));
    const list = screen.getByRole("list", { name: "Scenarios" });
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(1));
    expect(within(list).getByText("Approve leave request: Submitted → Approved")).toBeTruthy();
  });

  it("shows the empty state", async () => {
    await h.renderAt("/scenarios", ...backend([]));
    expect(await screen.findByRole("heading", { name: "No scenarios yet" })).toBeTruthy();
  });
});

describe("Scenario inspector", () => {
  it("shows the Given/When/Then card, the pass callout, the trace and where it comes from", async () => {
    await h.renderAt("/scenarios/scenario:submit", ...backend());
    const inspector = await screen.findByRole("complementary", { name: "Scenario details" });
    expect(await within(inspector).findByRole("heading", { name: "Submit leave request: Draft → Submitted" })).toBeTruthy();
    const card = within(inspector).getByRole("table", { name: "Given, when, then" });
    expect(within(card).getAllByRole("row").map((r) => r.textContent)).toEqual(["Giventhe state is Draft", "WhenSubmit leave request runs", "Thenthe state is Submitted"]);
    expect(within(inspector).getByText("Pass · the run matched every expected fact.")).toBeTruthy();
    const trace = await within(inspector).findByRole("list", { name: "Trace" });
    const steps = within(trace).getAllByRole("listitem").map((item) => item.textContent ?? "");
    expect(steps[0]).toContain("Set up the given facts");
    expect(steps.some((text) => text.includes("Performed by Employee"))).toBe(true);
    expect(steps.some((text) => text.includes("Moved from Draft to Submitted"))).toBe(true);
    expect(steps.some((text) => text.includes("Produced Leave request submitted"))).toBe(true);
    expect(within(inspector).getByRole("link", { name: "Transition Draft → Submitted" })).toBeTruthy();
    expect(h.reads().some((call) => call.path === "/api/scenarios/scenario:submit/run")).toBe(true);
    expect(await axeViolations()).toEqual([]);
  });

  it("highlights the first diverging step of a failed run with expected and actual", async () => {
    await h.renderAt("/scenarios/scenario:approve", ...backend());
    const inspector = await screen.findByRole("complementary", { name: "Scenario details" });
    expect(await within(inspector).findByText("Fail · 1 expected fact differs from the run.")).toBeTruthy();
    const trace = await within(inspector).findByRole("list", { name: "Trace" });
    const diverging = trace.querySelector("[data-diverging='true']") as HTMLElement;
    expect(diverging.textContent).toContain("First difference");
    expect(diverging.textContent).toContain("Expected Approved, got Submitted");
    expect(trace.querySelectorAll("[data-diverging='true']")).toHaveLength(1);
  });

  it("lists the missing semantics of an undecidable run with a link to fix them", async () => {
    await h.renderAt("/scenarios/scenario:authorize", ...backend());
    const inspector = await screen.findByRole("complementary", { name: "Scenario details" });
    expect(await within(inspector).findByText("Undecidable · the model is missing what the run needs.")).toBeTruthy();
    const missing = await within(inspector).findByRole("list", { name: "Missing semantics" });
    expect(within(missing).getByRole("link", { name: "Manager decision authorization" }).getAttribute("href")).toBe("/model/rules/rule:authorization");
    expect(missing.textContent).toContain("rule condition");
    expect(within(inspector).getByText("Unrecognised step")).toBeTruthy();
  });

  it("explains a scenario that has not run", async () => {
    await h.renderAt("/scenarios/scenario:contractor", ...backend());
    const inspector = await screen.findByRole("complementary", { name: "Scenario details" });
    expect(await within(inspector).findByText("Not run yet · Run scenarios to see results.")).toBeTruthy();
    const card = within(inspector).getByRole("table", { name: "Given, when, then" });
    expect(within(card).getAllByRole("row").map((r) => r.textContent)).toEqual([
      "Givenemployment type is Contractor; leave type is Annual",
      "WhenAnnual leave eligibility is evaluated",
      "Theneligible is No",
    ]);
  });

  it("greys a run the model has outdated since it was shown", async () => {
    await h.renderAt("/scenarios/scenario:submit", ...backend());
    await screen.findByText("Pass · the run matched every expected fact.");
    const outdated = SCENARIOS.map((item) => (item.id === "scenario:submit" ? { ...item, run: null } : item));
    h.use(...backend(outdated));
    act(() => {
      focusManager.setFocused(false);
      focusManager.setFocused(true);
    });
    expect(await screen.findByText("Out of date · the model changed since this run. Run again.")).toBeTruthy();
    expect(within(row("Submit leave request: Draft → Submitted")).getByText("Out of date")).toBeTruthy();
    act(() => focusManager.setFocused(undefined));
  });
});

describe("Scenario writes", () => {
  it("runs every runnable scenario, showing the rows as running", async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { user } = await h.renderAt(
      "/scenarios",
      ...backend(),
      http.post("*/api/scenarios/run", async () => {
        await gate;
        return etagJson(WRITE_OK);
      }),
    );
    await screen.findByText("Submit leave request: Draft → Submitted");
    await user.click(screen.getByRole("button", { name: "Run all" }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/scenarios/run", ifMatch: TEST_ETAG, body: { scope: ["scenario:approve", "scenario:authorize", "scenario:submit"] } });
    expect(within(row("Submit leave request: Draft → Submitted")).getByText("Running")).toBeTruthy();
    expect(within(row("Annual leave eligibility: Contractor, Annual → No")).queryByText("Running")).toBeNull();
    release();
    expect(await screen.findByText("3 scenarios ran · 1 pass")).toBeTruthy();
  });

  it("runs the selected scenario only", async () => {
    const { user } = await h.renderAt("/scenarios/scenario:submit", ...backend(), ok("*/api/scenarios/run"));
    const inspector = await screen.findByRole("complementary", { name: "Scenario details" });
    await user.click(await within(inspector).findByRole("button", { name: "Run this one" }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ body: { scope: ["scenario:submit"] } });
  });

  it("confirms a derived scenario", async () => {
    const { user } = await h.renderAt("/scenarios/scenario:contractor", ...backend(), ok("*/api/scenarios/:id/confirm"));
    const inspector = await screen.findByRole("complementary", { name: "Scenario details" });
    expect(within(inspector).queryByRole("button", { name: "Run this one" })).toBeNull();
    await user.click(await within(inspector).findByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/scenarios/scenario:contractor/confirm", ifMatch: TEST_ETAG });
    expect(await screen.findByText("Scenario confirmed")).toBeTruthy();
  });

  it("confirms all derived scenarios after reviewing samples", async () => {
    const { user } = await h.renderAt("/scenarios", ...backend(), ok("*/api/scenarios/confirm-derived"));
    await screen.findByText("Submit leave request: Draft → Submitted");
    await user.click(screen.getByRole("button", { name: "Confirm all derived" }));
    const sheet = await screen.findByRole("dialog", { name: "Confirm 1 derived scenario" });
    expect(within(sheet).getByText("Annual leave eligibility: Contractor, Annual → No")).toBeTruthy();
    await user.click(within(sheet).getByRole("button", { name: /Confirm all/ }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/scenarios/confirm-derived", ifMatch: TEST_ETAG, body: { table_id: "dt:eligibility" } });
    expect(await screen.findByText("1 derived scenario confirmed")).toBeTruthy();
  });

  it("corrects the expected outcome of a decision-table scenario", async () => {
    const { user } = await h.renderAt("/scenarios/scenario:contractor", ...backend(), ok("*/api/scenarios/:id/correct"));
    const inspector = await screen.findByRole("complementary", { name: "Scenario details" });
    await user.click(await within(inspector).findByRole("button", { name: "Correct expected outcome" }));
    const sheet = await screen.findByRole("dialog", { name: "Correct the expected outcome" });
    await user.click(within(sheet).getByRole("radio", { name: "Yes" }));
    await user.click(within(sheet).getByRole("button", { name: /Save/ }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/scenarios/scenario:contractor/correct", body: { then: ['{"decision_outputs":{"0":{"kind":"bool","value":true}}}'] } });
  });

  it("corrects any other expected fact as its exact text", async () => {
    const { user } = await h.renderAt("/scenarios/scenario:submit", ...backend(), ok("*/api/scenarios/:id/correct"));
    const inspector = await screen.findByRole("complementary", { name: "Scenario details" });
    await user.click(await within(inspector).findByRole("button", { name: "Correct expected outcome" }));
    const sheet = await screen.findByRole("dialog", { name: "Correct the expected outcome" });
    const field = within(sheet).getByLabelText("Expected fact 1");
    await user.clear(field);
    await user.click(field);
    await user.paste('{"states_active":["state:approved"]}');
    await user.click(within(sheet).getByRole("button", { name: /Save/ }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ body: { then: ['{"states_active":["state:approved"]}'] } });
  });

  it("hides every write control from a role that cannot change the model", async () => {
    await h.renderAt("/scenarios/scenario:contractor", ...backend(), me(["reviewer"]));
    const inspector = await screen.findByRole("complementary", { name: "Scenario details" });
    await within(inspector).findByText("Not run yet · Run scenarios to see results.");
    expect(screen.queryByRole("button", { name: "Run all" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Confirm all derived" })).toBeNull();
    expect(within(inspector).queryByRole("button", { name: "Confirm" })).toBeNull();
    expect(within(inspector).queryByRole("button", { name: "Correct expected outcome" })).toBeNull();
  });
});
