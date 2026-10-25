// Model · Glossary (handoff "4. Model" Glossary, spec §5.4.1): the term table (Term, Definition
// with its source, Aliases chips, Status), the term inspector at /model/glossary/$id, and the
// governed writes Confirm (confirm_glossary), Edit aliases (edit_glossary) and Merge into…
// (preview_glossary_merge, then merge_glossary with the reviewed preview).
import { screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";

import { axeViolations } from "../../test/axe";
import { GLOSSARY, modelBase, WRITE_OK } from "../../test/modelFixtures";
import { etagJson, TEST_ETAG } from "../../test/msw";
import { setupRouteHarness } from "../../test/routeHarness";

const h = setupRouteHarness();

const glossary = (items = GLOSSARY) => http.get("*/api/glossary", () => etagJson({ items, next_cursor: null }));
const ok = (path: string) => http.post(path, () => etagJson(WRITE_OK));
const me = (roles: string[]) => http.get("*/api/me", () => HttpResponse.json({ user: "alice", mode: "pilot", roles, inference_available: true }));

const PREVIEW = {
  action: "merge",
  built_at: "2026-10-25T10:00:00Z",
  proposal_id: "prop:merge-1",
  revision: "rev:7:0000000000000000",
  semantic_hash: "psg:sha256:7",
  version: "rev:7:0000000000000000",
  events: [],
  intake: { proposal_id: "prop:merge-1", requires_reason: true, reasons: [], conflicts: [], duplicates: [], coverage: [] },
  diff: { added: [], removed: ["term:holiday"], modified: ["term:request"], base_semantic_hash: "a", candidate_semantic_hash: "b" },
};

describe("Glossary table", () => {
  it("lists every term with its definition source, aliases and status", async () => {
    await h.renderAt("/model/glossary", ...modelBase, glossary());
    const table = await screen.findByRole("table", { name: "Glossary" });
    expect(within(table).getAllByRole("columnheader").map((c) => c.textContent)).toEqual(["Term", "Definition", "Aliases", "Status"]);
    const row = within(table).getByRole("link", { name: "leave request" }).closest("tr") as HTMLElement;
    expect(within(row).getByText("request")).toBeTruthy();
    expect(await within(row).findByText("HR-013")).toBeTruthy();
    expect(within(row).getByText("Confirmed")).toBeTruthy();
    const holiday = within(table).getByRole("link", { name: "public holiday" }).closest("tr") as HTMLElement;
    expect(within(holiday).getByText("Proposed")).toBeTruthy();
    expect(within(holiday).getByText("No definition recorded")).toBeTruthy();
    expect(screen.queryByRole("complementary", { name: "Term details" })).toBeNull();
    expect(await axeViolations()).toEqual([]);
  });

  it("shows skeletons while loading, then the empty state", async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await h.renderAt(
      "/model/glossary",
      ...modelBase,
      http.get("*/api/glossary", async () => {
        await gate;
        return etagJson({ items: [], next_cursor: null });
      }),
    );
    expect(await screen.findByRole("status", { name: "Loading" })).toBeTruthy();
    release();
    expect(await screen.findByRole("heading", { name: "No glossary terms yet" })).toBeTruthy();
  });

  it("explains a failed read and retries", async () => {
    let fail = true;
    const { user } = await h.renderAt(
      "/model/glossary",
      ...modelBase,
      http.get("*/api/glossary", () =>
        fail ? etagJson({ code: "E_DATABASE_BUSY", message: "the store is busy", details: {} }, { status: 503 }) : etagJson({ items: GLOSSARY, next_cursor: null }),
      ),
    );
    expect(await screen.findByText(/The glossary could not be loaded/)).toBeTruthy();
    fail = false;
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("link", { name: "leave request" })).toBeTruthy();
  });

  it("opens the term inspector from the term link and closes it", async () => {
    const { user, router } = await h.renderAt("/model/glossary", ...modelBase, glossary());
    await user.click(await screen.findByRole("link", { name: "public holiday" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/model/glossary/term:holiday"));
    const inspector = await screen.findByRole("complementary", { name: "Term details" });
    expect(within(inspector).getByRole("heading", { name: "public holiday" })).toBeTruthy();
    expect(within(inspector).getByText("holiday")).toBeTruthy();
    await user.click(within(inspector).getByRole("button", { name: "Close details" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/model/glossary"));
  });
});

describe("Glossary writes", () => {
  it("confirms a proposed term with the reviewed ETag and a toast", async () => {
    const { user } = await h.renderAt("/model/glossary/term:holiday", ...modelBase, glossary(), ok("*/api/glossary/:id/confirm"));
    const inspector = await screen.findByRole("complementary", { name: "Term details" });
    await user.click(within(inspector).getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/glossary/term:holiday/confirm", ifMatch: TEST_ETAG });
    expect(await screen.findByText("Term confirmed")).toBeTruthy();
  });

  it("offers Confirm only for a proposed term", async () => {
    await h.renderAt("/model/glossary/term:request", ...modelBase, glossary());
    const inspector = await screen.findByRole("complementary", { name: "Term details" });
    expect(within(inspector).queryByRole("button", { name: "Confirm" })).toBeNull();
    expect(within(inspector).getByRole("button", { name: "Edit aliases" })).toBeTruthy();
  });

  it("edits the aliases, keeping the term as its identity", async () => {
    const { user } = await h.renderAt("/model/glossary/term:balance", ...modelBase, glossary(), ok("*/api/glossary/:id/edit"));
    await user.click(await screen.findByRole("button", { name: "Edit aliases" }));
    const sheet = await screen.findByRole("dialog", { name: "Edit aliases" });
    expect((within(sheet).getByLabelText("Term")).hasAttribute("readonly")).toBe(true);
    const aliases = within(sheet).getByLabelText("Aliases");
    expect((aliases as HTMLInputElement).value).toBe("balance, remaining leave");
    await user.clear(aliases);
    await user.type(aliases, "balance, leave left, ");
    await user.click(within(sheet).getByRole("button", { name: /Save/ }));
    await waitFor(() => expect(h.writes()).toHaveLength(1));
    expect(h.writes()[0]).toMatchObject({ path: "/api/glossary/term:balance/edit", ifMatch: TEST_ETAG, body: { term: "annual leave balance", aliases: ["balance", "leave left"] } });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Edit aliases" })).toBeNull());
  });

  it("merges into an accepted term after reviewing the preview", async () => {
    const { user } = await h.renderAt(
      "/model/glossary/term:holiday",
      ...modelBase,
      glossary(),
      http.post("*/api/glossary/:id/merge/preview", () => etagJson(PREVIEW)),
      ok("*/api/glossary/:id/merge"),
    );
    await user.click(await screen.findByRole("button", { name: "Merge into…" }));
    const sheet = await screen.findByRole("dialog", { name: "Merge “public holiday” into…" });
    const target = within(sheet).getByLabelText("Merge into");
    // Only accepted terms other than this one can be the target.
    expect(within(target).getAllByRole("option").map((o) => o.textContent)).toEqual(["Choose a term", "annual leave balance", "leave request"]);
    await user.selectOptions(target, "term:request");
    expect((within(sheet).getByRole("button", { name: /Preview merge/ }) as HTMLButtonElement).disabled).toBe(true);
    await user.type(within(sheet).getByLabelText("Reason (required)"), "Same concept.");
    await user.click(within(sheet).getByRole("button", { name: /Preview merge/ }));
    expect((await within(sheet).findByRole("list", { name: "What changes in the model" })).textContent).toContain("leave request");
    expect(within(sheet).getByText("public holiday")).toBeTruthy();
    await user.click(within(sheet).getByRole("button", { name: /^Merge/ }));
    await waitFor(() => expect(h.writes()).toHaveLength(2));
    expect(h.writes()[0]).toMatchObject({ path: "/api/glossary/term:holiday/merge/preview", ifMatch: TEST_ETAG, body: { into: "term:request", reason: "Same concept." } });
    expect(h.writes()[1]).toMatchObject({
      path: "/api/glossary/term:holiday/merge",
      ifMatch: '"rev:7:0000000000000000"',
      body: { into: "term:request", reason: "Same concept.", preview: { proposal_id: "prop:merge-1", built_at: "2026-10-25T10:00:00Z" } },
    });
  });

  it("asks to preview again when the reviewed preview is outdated", async () => {
    const { user } = await h.renderAt(
      "/model/glossary/term:holiday",
      ...modelBase,
      glossary(),
      http.post("*/api/glossary/:id/merge/preview", () => etagJson(PREVIEW)),
      http.post("*/api/glossary/:id/merge", () => etagJson({ code: "E_PREVIEW_MISMATCH", message: "the change is no longer the reviewed preview", details: {} }, { status: 409 })),
    );
    await user.click(await screen.findByRole("button", { name: "Merge into…" }));
    const sheet = await screen.findByRole("dialog");
    await user.selectOptions(within(sheet).getByLabelText("Merge into"), "term:request");
    await user.type(within(sheet).getByLabelText("Reason (required)"), "Same concept.");
    await user.click(within(sheet).getByRole("button", { name: /Preview merge/ }));
    await user.click(await within(sheet).findByRole("button", { name: /^Merge/ }));
    expect((await within(sheet).findByRole("alert")).textContent).toContain("The model changed since this preview. Preview the merge again.");
    expect(within(sheet).getByRole("button", { name: /Preview merge/ })).toBeTruthy();
  });

  it("hides every write control from a role that cannot change the model", async () => {
    await h.renderAt("/model/glossary/term:holiday", ...modelBase, glossary(), me(["reviewer"]));
    const inspector = await screen.findByRole("complementary", { name: "Term details" });
    expect(within(inspector).getByRole("heading", { name: "public holiday" })).toBeTruthy();
    expect(within(inspector).queryByRole("button", { name: "Confirm" })).toBeNull();
    expect(within(inspector).queryByRole("button", { name: "Edit aliases" })).toBeNull();
    expect(within(inspector).queryByRole("button", { name: "Merge into…" })).toBeNull();
  });
});
