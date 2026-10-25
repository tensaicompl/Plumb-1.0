// Model · Processes (handoff "4. Model" Processes, spec §5.4.5): the swimlane canvas from
// get_process (lanes per performer; tasks, gateways, start and end; orthogonal edges), the node
// panel with the step list, Edit steps (mutate_process_steps), Import from Mermaid
// (import_process), Export (export_process) and the empty state.
import { screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";

import { axeViolations } from "../../test/axe";
import { modelBase, PROCESS, WRITE_OK } from "../../test/modelFixtures";
import { etagJson, TEST_ETAG } from "../../test/msw";
import { setupRouteHarness } from "../../test/routeHarness";
import { linearSteps } from "./processLayout";

const h = setupRouteHarness();

const processes = (items = [PROCESS]) => [
  http.get("*/api/processes", () => etagJson({ items, next_cursor: null })),
  http.get("*/api/processes/:id", ({ params }) => {
    const found = items.find((item) => item.id === params["id"]);
    return found === undefined ? etagJson({ code: "E_NOT_FOUND", message: "not found", details: {} }, { status: 404 }) : etagJson(found);
  }),
];
const ok = (path: string) => http.post(path, () => etagJson(WRITE_OK));
const me = (roles: string[]) => http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles, inference_available: true }));

describe("Process canvas", () => {
  it("draws the lanes, nodes and flows and details the selected node", async () => {
    const { user } = await h.renderAt("/model/processes", ...modelBase, ...processes());
    const canvas = await screen.findByRole("region", { name: "Leave request approval" });
    expect(within(canvas).getByText("Valid")).toBeTruthy();
    const lanes = await within(canvas).findByRole("list", { name: "Lanes" });
    expect(within(lanes).getAllByRole("listitem").map((lane) => lane.textContent)).toEqual(["Employee", "Manager", "System"]);
    const submit = await within(canvas).findByRole("button", { name: "Human task: Submit leave request" });
    expect(within(canvas).getByRole("button", { name: "Service task: Deduct balance" })).toBeTruthy();
    expect(within(canvas).getByRole("button", { name: "Start event" })).toBeTruthy();
    expect(within(canvas).getByRole("button", { name: "End event" })).toBeTruthy();
    expect(canvas.querySelectorAll("[data-flow]")).toHaveLength(PROCESS.flows.length);
    await user.click(submit);
    expect(submit.getAttribute("aria-pressed")).toBe("true");
    const panel = screen.getByRole("complementary", { name: "Step details" });
    expect(within(panel).getByText("Human task")).toBeTruthy();
    expect(within(panel).getByRole("heading", { name: "Submit leave request" })).toBeTruthy();
    expect(within(panel).getByText("Employee")).toBeTruthy();
    expect(within(panel).getByText("Leave request submitted")).toBeTruthy();
    const steps = within(panel).getByRole("list", { name: "Steps" });
    expect(within(steps).getAllByRole("listitem").map((step) => step.textContent)).toEqual(["1Submit leave request", "2Approve leave request", "3Deduct balance"]);
    expect(await axeViolations()).toEqual([]);
  });

  it("shows the validation issues instead of Valid", async () => {
    await h.renderAt("/model/processes", ...modelBase, ...processes([{ ...PROCESS, validation: ["G_PROCESS_UNREACHABLE pn:deduct"] }]));
    const canvas = await screen.findByRole("region", { name: "Leave request approval" });
    expect(within(canvas).getByText("1 issue")).toBeTruthy();
  });

  it("shows the empty state with Import", async () => {
    const { user } = await h.renderAt("/model/processes", ...modelBase, ...processes([]));
    expect(await screen.findByRole("heading", { name: "No processes yet" })).toBeTruthy();
    expect(screen.getByText("They appear after the Flows step, or import one.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Import from Mermaid" }));
    expect(await screen.findByRole("dialog", { name: "Import a process" })).toBeTruthy();
  });
});

describe("Process writes", () => {
  it("imports Mermaid text with the reviewed ETag", async () => {
    const { user } = await h.renderAt("/model/processes", ...modelBase, ...processes(), ok("*/api/processes/import"));
    await user.click(await screen.findByRole("button", { name: "Import Mermaid" }));
    const sheet = await screen.findByRole("dialog", { name: "Import a process" });
    await user.type(within(sheet).getByLabelText("Mermaid text"), "flowchart TD");
    await user.click(within(sheet).getByRole("button", { name: /Import/ }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/processes/import", ifMatch: TEST_ETAG, body: { format: "mermaid", text: "flowchart TD" } });
    expect(await screen.findByText("Process imported")).toBeTruthy();
  });

  it("exports the process as Mermaid text", async () => {
    const createObjectURL = vi.fn(() => "blob:process");
    const revokeObjectURL = vi.fn();
    vi.stubGlobal("URL", Object.assign(URL, { createObjectURL, revokeObjectURL }));
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const { user } = await h.renderAt(
      "/model/processes",
      ...modelBase,
      ...processes(),
      http.get("*/api/processes/:id/export", () => new HttpResponse("flowchart TD\n", { headers: { "Content-Type": "text/plain" } })),
    );
    await user.click(await screen.findByRole("button", { name: "Export" }));
    await waitFor(() => expect(click).toHaveBeenCalled());
    expect(h.reads().find((call) => call.path === "/api/processes/proc:approval/export")?.search.get("format")).toBe("mermaid");
    expect(createObjectURL).toHaveBeenCalled();
  });

  it("moves, removes and adds steps, one write each", async () => {
    const { user } = await h.renderAt("/model/processes", ...modelBase, ...processes(), ok("*/api/processes/:id/steps"));
    await user.click(await screen.findByRole("button", { name: "Edit steps" }));
    const sheet = await screen.findByRole("dialog", { name: "Edit steps" });
    await user.click(within(sheet).getByRole("button", { name: "Move Approve leave request up" }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/processes/proc:approval/steps", ifMatch: TEST_ETAG, body: { op: "move", node_ref: "pn:approve", position: 0 } });
    await user.click(within(sheet).getByRole("button", { name: "Remove Deduct balance" }));
    await waitFor(() => expect(h.writes()).toHaveLength(2));
    expect(h.writes()[1]).toMatchObject({ body: { op: "remove", node_ref: "pn:deduct" } });
    await user.selectOptions(within(sheet).getByLabelText("Operation"), "op:submit");
    await user.click(within(sheet).getByRole("button", { name: "Add step" }));
    await waitFor(() => expect(h.writes()).toHaveLength(3));
    expect(h.writes()[2]).toMatchObject({ body: { op: "add", operation_ref: "op:submit", position: 3 } });
  });

  it("offers no edits to a role that cannot change the model", async () => {
    await h.renderAt("/model/processes", ...modelBase, ...processes(), me(["reviewer"]));
    await screen.findByRole("region", { name: "Leave request approval" });
    expect(screen.queryByRole("button", { name: "Edit steps" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Import Mermaid" })).toBeNull();
    expect(screen.getByRole("button", { name: "Export" })).toBeTruthy();
  });
});

describe("Linear steps", () => {
  it("orders the steps by following the flows from the start", () => {
    expect(linearSteps(PROCESS).map((node) => node.id)).toEqual(["pn:submit", "pn:approve", "pn:deduct"]);
  });
});
