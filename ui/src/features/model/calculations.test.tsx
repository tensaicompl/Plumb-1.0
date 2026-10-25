// Model · Calculations (handoff "4. Model" Calculations, spec §5.4.4): the calculation card with
// its exact expression (midnight, mono), the plain-language reading, the worked example from the
// server calendar (get_calculation_example: a full week and one request per day for the tiles)
// and "Confirm calculation" (confirm_calculation) for a proposed calculation.
import { screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";

import { axeViolations } from "../../test/axe";
import { CALCULATIONS, calculationExampleHandler, modelBase, WRITE_OK } from "../../test/modelFixtures";
import { etagJson, TEST_ETAG } from "../../test/msw";
import { setupRouteHarness } from "../../test/routeHarness";
import { exampleWeek, plainExpression } from "./plainExpr";

const h = setupRouteHarness();

const calculations = (items = CALCULATIONS) => http.get("*/api/model/calculations", () => etagJson({ items, next_cursor: null }));
const me = (roles: string[]) => http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles, inference_available: true }));

describe("Calculation card", () => {
  it("shows the exact expression, its plain reading and the worked example from the server calendar", async () => {
    const week = exampleWeek(new Date());
    await h.renderAt("/model/calculations", ...modelBase, calculations(), calculationExampleHandler(week[2] ?? null));
    const card = await screen.findByRole("region", { name: "Requested leave duration" });
    expect(within(card).getByText("Proposed")).toBeTruthy();
    expect(within(card).getByText("working_days([start_date, end_date], calendar, true) * day_fraction")).toBeTruthy();
    expect(
      within(card).getByText(
        "The number of working days from the start date to the end date, counting both, using the calendar, multiplied by the day fraction.",
      ),
    ).toBeTruthy();
    const example = within(card).getByRole("group", { name: "Worked example" });
    // Monday to Friday minus the Wednesday holiday the server calendar knows.
    expect(await within(example).findByText("→ 4 working days")).toBeTruthy();
    const tiles = within(example).getAllByRole("listitem");
    expect(tiles).toHaveLength(7);
    await waitFor(() => expect(tiles.map((tile) => tile.getAttribute("data-working"))).toEqual(["true", "true", "false", "true", "true", "false", "false"]));
    expect(tiles[2]?.textContent).toContain("not a working day");
    const inputs = h.reads().filter((call) => call.path.endsWith("/example")).map((call) => JSON.parse(call.search.get("inputs") ?? "{}") as Record<string, { type: string; value: string }>);
    expect(inputs[0]).toEqual({ start_date: { type: "date", value: week[0] }, end_date: { type: "date", value: week[6] }, day_fraction: { type: "decimal", value: "1" } });
    expect(await axeViolations()).toEqual([]);
  });

  it("explains when the server cannot work an example", async () => {
    await h.renderAt(
      "/model/calculations",
      ...modelBase,
      calculations([{ id: "calc:balance", name: "balance", expr: "entitlement - taken", status: "Accepted" }]),
      http.get("*/api/model/calculations/:id/example", () => etagJson({ code: "E_INVALID_REQUEST", message: 'input "taken": a root input value is required', details: {} }, { status: 400 })),
    );
    const card = await screen.findByRole("region", { name: "Balance" });
    expect(within(card).getByText("Confirmed")).toBeTruthy();
    expect(await within(card).findByText("No worked example yet: the example needs inputs Plumb can't choose.")).toBeTruthy();
    expect(within(card).queryByRole("button", { name: "Confirm calculation" })).toBeNull();
  });

  it("confirms a proposed calculation with the reviewed ETag", async () => {
    const { user } = await h.renderAt("/model/calculations", ...modelBase, calculations(), calculationExampleHandler(), http.post("*/api/model/calculations/:id/confirm", () => etagJson(WRITE_OK)));
    await user.click(await screen.findByRole("button", { name: "Confirm calculation" }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/model/calculations/calc:days/confirm", ifMatch: TEST_ETAG });
    expect(await screen.findByText("Calculation confirmed")).toBeTruthy();
  });

  it("hides Confirm calculation from a role that cannot change the model", async () => {
    await h.renderAt("/model/calculations", ...modelBase, calculations(), calculationExampleHandler(), me(["reviewer"]));
    await screen.findByRole("region", { name: "Requested leave duration" });
    expect(screen.queryByRole("button", { name: "Confirm calculation" })).toBeNull();
  });

  it("lists several calculations and selects one by link", async () => {
    const { user, router } = await h.renderAt(
      "/model/calculations",
      ...modelBase,
      calculations([...CALCULATIONS, { id: "calc:balance", name: "balance", expr: "entitlement - taken", status: "Accepted" }]),
      calculationExampleHandler(),
    );
    await user.click(await screen.findByRole("link", { name: "Balance" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/model/calculations/calc:balance"));
    expect(await screen.findByRole("region", { name: "Balance" })).toBeTruthy();
  });

  it("shows the empty state", async () => {
    await h.renderAt("/model/calculations", ...modelBase, calculations([]));
    expect(await screen.findByRole("heading", { name: "No calculations yet" })).toBeTruthy();
  });
});

describe("Plain-language expressions", () => {
  it("reads the PlumbExpr subset in words and gives up on anything else", () => {
    expect(plainExpression("days_between(start_date, end_date) + 1")).toBe("The number of days between the start date and the end date plus 1.");
    expect(plainExpression("max(balance - taken, 0)")).toBe("The larger of the balance minus the taken and 0.");
    expect(plainExpression("working_days([a, b], calendar, false)")).toBe("The number of working days from the a to the b, not counting the end, using the calendar.");
    expect(plainExpression("days > 5 and not approved")).toBe("The days is greater than 5 and not the approved.");
    expect(plainExpression("mystery(x) / 2")).toBe("Mystery(the x) divided by 2.");
    expect(plainExpression("a +")).toBeNull();
  });

  it("chooses the first full week of the next year", () => {
    expect(exampleWeek(new Date("2026-10-25T12:00:00Z"))).toEqual(["2027-01-04", "2027-01-05", "2027-01-06", "2027-01-07", "2027-01-08", "2027-01-09", "2027-01-10"]);
  });
});
