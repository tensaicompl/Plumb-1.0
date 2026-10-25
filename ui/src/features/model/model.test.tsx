// The Model screen frame (handoff "4. Model", spec §5.4, §5.4.6, §5.4.7): the "Model" header, the
// five segments as links (deep-linkable, /model redirects to Glossary), and the Coverage (CRUD
// matrix) and "Changes +N" (model diff since the last visit) sheets.
import { screen, waitFor, within } from "@testing-library/react";
import { http } from "msw";
import { describe, expect, it } from "vitest";

import { axeViolations } from "../../test/axe";
import { CRUD, DIFF, GLOSSARY, modelBase } from "../../test/modelFixtures";
import { etagJson } from "../../test/msw";
import { setupRouteHarness } from "../../test/routeHarness";

const h = setupRouteHarness();

const glossary = http.get("*/api/glossary", () => etagJson({ items: GLOSSARY, next_cursor: null }));

describe("Model header", () => {
  it("redirects /model to Glossary and shows the five segments with the current one marked", async () => {
    const { router } = await h.renderAt("/model", ...modelBase, glossary);
    expect(await screen.findByRole("heading", { level: 1, name: "Model" })).toBeTruthy();
    await waitFor(() => expect(router.state.location.pathname).toBe("/model/glossary"));
    const views = screen.getByRole("navigation", { name: "Model views" });
    expect(within(views).getAllByRole("link").map((link) => link.textContent)).toEqual(["Glossary", "Entities", "Rules", "Calculations", "Processes"]);
    expect((within(views).getByRole("link", { name: "Glossary" })).getAttribute("aria-current")).toBe("page");
  });

  it("navigates between segments by link", async () => {
    const { user, router } = await h.renderAt("/model/glossary", ...modelBase, glossary, http.get("*/api/model/entities", () => etagJson({ items: [], next_cursor: null })));
    await user.click(await screen.findByRole("link", { name: "Entities" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/model/entities"));
  });

  it("shows the number of added elements on the Changes pill", async () => {
    await h.renderAt("/model/glossary", ...modelBase, glossary);
    const changes = await screen.findByRole("button", { name: /Changes/ });
    await waitFor(() => expect((changes).textContent).toContain("+3"));
  });
});

describe("Coverage sheet", () => {
  it("shows the CRUD matrix with element names, letters for each access and a dot for none", async () => {
    const { user } = await h.renderAt("/model/glossary", ...modelBase, glossary);
    await user.click(await screen.findByRole("button", { name: "Coverage" }));
    const sheet = await screen.findByRole("dialog", { name: "Coverage" });
    const table = await within(sheet).findByRole("table", { name: "Operations × entities" });
    const headers = within(table).getAllByRole("columnheader").map((cell) => cell.textContent);
    expect(headers).toEqual(["Operation", "Employee", "Leave request"]);
    const approve = within(table).getByRole("rowheader", { name: "Approve leave request" }).closest("tr") as HTMLElement;
    const cells = within(approve).getAllByRole("cell");
    expect(cells[0]?.textContent).toContain("R");
    expect(cells[1]?.textContent).toContain("RU");
    // The explanation is the cell's accessible description (and tooltip).
    expect(within(table).getByRole("cell", { name: "Approve leave request reads and updates Leave request" })).toBe(cells[1]);
    const submit = within(table).getByRole("rowheader", { name: "Submit leave request" }).closest("tr") as HTMLElement;
    expect(within(submit).getAllByRole("cell")[0]?.textContent).toContain("·");
    expect(within(table).getByRole("cell", { name: "Submit leave request does not touch Employee" })).toBe(within(submit).getAllByRole("cell")[0]);
    // null is "not distinguishable", never "no".
    expect(within(table).getByRole("cell", { name: "Submit leave request creates Leave request; update and delete not distinguishable" })).toBe(within(submit).getAllByRole("cell")[1]);
    expect(await axeViolations(sheet)).toEqual([]);
    expect(CRUD.cells.length).toBe(4);
  });

  it("says so when the model has no operations yet", async () => {
    const { user } = await h.renderAt(
      "/model/glossary",
      ...modelBase.slice(0, 1),
      http.get("*/api/model/crud", () => etagJson({ operations: [], entities: [], cells: [] })),
      http.get("*/api/model/diff", () => etagJson(DIFF)),
      glossary,
    );
    await user.click(await screen.findByRole("button", { name: "Coverage" }));
    expect(await screen.findByText("No operations or entities yet. They appear after the Model and Flows steps.")).toBeTruthy();
  });
});

describe("Changes sheet", () => {
  it("lists added and modified elements by name with the version range", async () => {
    const { user } = await h.renderAt("/model/glossary", ...modelBase, glossary);
    await user.click(await screen.findByRole("button", { name: /Changes/ }));
    const sheet = await screen.findByRole("dialog", { name: "Changes since your last visit" });
    expect(within(sheet).getByText("Version 39 → 42 · 3 added, 1 modified, 0 removed.")).toBeTruthy();
    const added = await within(sheet).findAllByText("Added");
    expect(added).toHaveLength(3);
    expect(within(sheet).getByText("Submit leave request")).toBeTruthy();
    expect(within(sheet).getByText("Leave request submitted")).toBeTruthy();
    expect(within(sheet).getByText("Modified")).toBeTruthy();
    expect(within(sheet).getByText("Leave request")).toBeTruthy();
    expect(await axeViolations(sheet)).toEqual([]);
  });

  it("compares from the version seen last time once the sheet was closed", async () => {
    const { user } = await h.renderAt("/model/glossary", ...modelBase, glossary);
    await user.click(await screen.findByRole("button", { name: /Changes/ }));
    await user.click(await screen.findByRole("button", { name: "Close" }));
    expect(localStorage.getItem("plumb.model.lastSeenRevision")).toBe(DIFF.to);
    localStorage.removeItem("plumb.model.lastSeenRevision");
  });

  it("explains when there is no earlier version", async () => {
    const { user } = await h.renderAt(
      "/model/glossary",
      ...modelBase.slice(0, 2),
      http.get("*/api/model/diff", () => etagJson({ code: "E_DIFF_NO_PARENT", message: "the target revision has no parent", details: {} }, { status: 422 })),
      glossary,
    );
    await user.click(await screen.findByRole("button", { name: /Changes/ }));
    expect(await screen.findByText("This is the first version of the model, so there is nothing to compare yet.")).toBeTruthy();
  });
});
