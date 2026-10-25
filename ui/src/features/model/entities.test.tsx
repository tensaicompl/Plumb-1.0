// Model · Entities (handoff "4. Model" Entities, spec §5.4.2): the entity card grid, the detail
// card with the attribute table (type · unit chip with set_attribute_type and
// set_attribute_unit, Nullable, data class with set_attribute_class), relationships, and the
// state diagram with each transition labelled by its operation.
import { screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";

import { axeViolations } from "../../test/axe";
import { ENTITIES, modelBase, WRITE_OK } from "../../test/modelFixtures";
import { etagJson, TEST_ETAG } from "../../test/msw";
import { setupRouteHarness } from "../../test/routeHarness";
import { layoutStates } from "./StateDiagram";

const h = setupRouteHarness();

const entities = (items = ENTITIES) => http.get("*/api/model/entities", () => etagJson({ items, next_cursor: null }));
const ok = (path: string) => http.post(path, () => etagJson(WRITE_OK));
const me = (roles: string[]) => http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles, inference_available: true }));
const attributeRow = (name: string) => screen.getByRole("rowheader", { name }).closest("tr") as HTMLElement;

describe("Entity grid and detail", () => {
  it("shows a card per entity and the first entity's attributes, relationships and states", async () => {
    await h.renderAt("/model/entities", ...modelBase, entities());
    const grid = await screen.findByRole("list", { name: "Entities" });
    const cards = within(grid).getAllByRole("link");
    expect(cards.map((card) => card.textContent)).toEqual(["Leave request4 attributes · 6 states", "Employee0 attributes"]);
    expect(cards[0]?.getAttribute("aria-current")).toBe("true");
    const detail = screen.getByRole("region", { name: "Leave request" });
    const table = within(detail).getByRole("table", { name: "Attributes" });
    expect(within(table).getAllByRole("columnheader").map((c) => c.textContent)).toEqual(["Attribute", "Type · unit", "Nullable", "Data class"]);
    expect(within(attributeRow("requested working days")).getByRole("button", { name: /Type · unit: Decimal\(2\) · days/ })).toBeTruthy();
    expect(within(attributeRow("reason")).getByText("nullable")).toBeTruthy();
    expect(within(attributeRow("start date")).getByText("required")).toBeTruthy();
    expect(within(attributeRow("leave type")).getByText("pii")).toBeTruthy();
    expect(await within(detail).findByText("Employee")).toBeTruthy();
    expect(within(detail).getByText("*:1")).toBeTruthy();
    const diagram = await within(detail).findByRole("img", { name: "States of Leave request" });
    expect(within(diagram).getByText("Draft")).toBeTruthy();
    expect(within(diagram).getByText("Submit leave request")).toBeTruthy();
    const transitions = within(detail).getByRole("list", { name: "Transitions" });
    expect(within(transitions).getAllByRole("listitem").map((item) => item.textContent)).toContain("Submitted → Approved: Approve leave request");
    expect(await axeViolations()).toEqual([]);
  });

  it("selects an entity by its card and deep link", async () => {
    const { user, router } = await h.renderAt("/model/entities", ...modelBase, entities());
    await user.click(await screen.findByRole("link", { name: /^Employee/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/model/entities/entity:employee"));
    const detail = await screen.findByRole("region", { name: "Employee" });
    expect(within(detail).getByText("This entity has no attributes yet.")).toBeTruthy();
    expect(within(detail).queryByRole("img")).toBeNull();
  });

  it("shows the empty state without entities", async () => {
    await h.renderAt("/model/entities", ...modelBase, entities([]));
    expect(await screen.findByRole("heading", { name: "No entities yet" })).toBeTruthy();
  });
});

describe("Attribute writes", () => {
  it("sets the type from the chip menu with the reviewed ETag", async () => {
    const { user } = await h.renderAt("/model/entities", ...modelBase, entities(), ok("*/api/model/attributes/:id/type"));
    await screen.findByRole("region", { name: "Leave request" });
    await user.click(within(attributeRow("leave type")).getByRole("button", { name: /Type · unit: String/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Int" }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/model/attributes/attr:type/type", ifMatch: TEST_ETAG, body: { type: "Int" } });
    expect(await screen.findByText("leave type is now Int")).toBeTruthy();
  });

  it("sets the unit through a short sheet", async () => {
    const { user } = await h.renderAt("/model/entities", ...modelBase, entities(), ok("*/api/model/attributes/:id/unit"));
    await screen.findByRole("region", { name: "Leave request" });
    await user.click(within(attributeRow("start date")).getByRole("button", { name: /Type · unit: Date/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Set unit…" }));
    const sheet = await screen.findByRole("dialog", { name: "Set the unit of start date" });
    await user.type(within(sheet).getByLabelText("Unit"), "day");
    await user.click(within(sheet).getByRole("button", { name: /Save/ }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/model/attributes/attr:start/unit", body: { unit: "day" } });
  });

  it("sets a data class only where none is recorded", async () => {
    const { user } = await h.renderAt("/model/entities", ...modelBase, entities(), ok("*/api/model/attributes/:id/class"));
    await screen.findByRole("region", { name: "Leave request" });
    expect(within(attributeRow("leave type")).queryByRole("button", { name: /data class/ })).toBeNull();
    await user.click(within(attributeRow("reason")).getByRole("button", { name: "Set the data class of reason" }));
    await user.click(await screen.findByRole("menuitem", { name: "pii" }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/model/attributes/attr:reason/class", body: { class: "pii" } });
  });

  it("shows types as text, without menus, to a role that cannot change the model", async () => {
    await h.renderAt("/model/entities", ...modelBase, entities(), me(["reviewer"]));
    await screen.findByRole("region", { name: "Leave request" });
    expect(within(attributeRow("leave type")).queryByRole("button")).toBeNull();
    expect(within(attributeRow("leave type")).getByText("String")).toBeTruthy();
    expect(within(attributeRow("reason")).queryByRole("button")).toBeNull();
  });

  it("explains a refused write", async () => {
    const { user } = await h.renderAt(
      "/model/entities",
      ...modelBase,
      entities(),
      http.post("*/api/model/attributes/:id/type", () => etagJson({ code: "E_SEMANTIC_INVARIANT", message: "the type is not known", details: {} }, { status: 422 })),
    );
    await screen.findByRole("region", { name: "Leave request" });
    await user.click(within(attributeRow("leave type")).getByRole("button", { name: /Type · unit: String/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Int" }));
    expect((await screen.findByRole("alert")).textContent).toContain("the type is not known");
  });
});

describe("State layout", () => {
  it("lays states out left to right by depth from the initial state", () => {
    const entity = ENTITIES[0];
    if (entity === undefined) {
      throw new Error("fixture");
    }
    const layout = layoutStates(entity.states, entity.transitions, (id) => id.replace("state:", ""));
    const column = (id: string) => layout.nodes.find((node) => node.id === id)?.column;
    expect([column("state:draft"), column("state:submitted"), column("state:approved"), column("state:rejected"), column("state:cancelled")]).toEqual([0, 1, 2, 2, 3]);
    expect(layout.nodes.find((node) => node.id === "state:draft")?.emphasis).toBe(true);
    expect(layout.nodes.find((node) => node.id === "state:submitted")?.emphasis).toBe(true);
    expect(layout.nodes.find((node) => node.id === "state:approved")?.emphasis).toBe(false);
    expect(layout.edges).toHaveLength(5);
    expect(layout.width).toBeGreaterThan(0);
  });

  it("routes a transition back to an earlier state below the diagram", () => {
    const layout = layoutStates(["s:a", "s:b"], [{ from: "s:a", to: "s:b", operation: null }, { from: "s:b", to: "s:a", operation: "op:reopen" }], (id) => id);
    const back = layout.edges.find((edge) => edge.from === "s:b");
    expect(back?.back).toBe(true);
    expect(layout.height).toBeGreaterThan(layout.nodes[0]?.height ?? 0);
  });
});
