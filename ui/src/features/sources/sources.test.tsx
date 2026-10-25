// Sources (handoff "7. Sources", UI-SPEC §5.7): the document list (GET /api/sources), the dashed
// import zone with upload progress and plain refusals, and the reading view (get_source,
// get_source_spans) with the passages that became requirements highlighted: hover explains, click
// opens the requirement. Rendered through the real router with every request answered by MSW.
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { delay, http, HttpResponse } from "msw";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import type { components } from "../../api/schema";
import { axeViolations } from "../../test/axe";
import { calls, renderApp, resetApp, setupApp, teardownApp, uploadedFile } from "../../test/appHarness";
import { etagJson, server } from "../../test/msw";

type SourceListItem = components["schemas"]["SourceListItem"];
type Span = components["schemas"]["Span"];

beforeAll(setupApp);
afterAll(teardownApp);
beforeEach(() => {
  resetApp();
  vi.useFakeTimers({ shouldAdvanceTime: true, now: new Date("2026-10-25T12:00:00Z") });
});
afterEach(() => {
  vi.useRealTimers();
});

const VERSION_ETAG = '"rev:42:aaaaaaaaaaaaaaaa"';

const SOURCES: SourceListItem[] = [
  { id: "src:1", name: "requirements.md", hash: "sha256:1", media_type: "text/markdown", source_kind: "markdown", imported_at: "2026-10-25T08:00:00.000000000Z", spans_count: 4 },
  { id: "src:2", name: "leave-policy.docx", hash: "sha256:2", media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document", source_kind: "docx", imported_at: "2026-10-21T08:00:00Z", spans_count: 1 },
];

function span(id: string, start: number, text: string | null, requirements: Span["requirements"] = []): Span {
  return { id, source_ref: "src:1", locator_kind: "TextRange", start, end: start + (text?.length ?? 0), content_hash: "sha256:" + id, text, requirements };
}

// Served out of document order and over two pages: the view sorts by position.
const PAGE_1: Span[] = [
  span("evd:c", 120, "Each request has a clear scope. Leave types are annual, sick and unpaid."),
  span("evd:b", 40, "5. **HR-002** A leave request shall specify a start date.", [{ id: "req:2", source_identifier: "HR-002" }]),
];
const PAGE_2: Span[] = [
  span("evd:a", 0, "## 3. Leave requests"),
  span("evd:d", 200, "6. **HR-003** The end date shall not be earlier than the start date.", [{ id: "req:3", source_identifier: null }]),
];

interface Backend {
  sources: SourceListItem[];
  uploads: { name: string; type: string; text: string; ifMatch: string | null }[];
  upload: () => Response;
  spansFail: boolean;
}

function serve(overrides: Partial<Backend> = {}): Backend {
  const b: Backend = {
    sources: SOURCES,
    uploads: [],
    upload: () => etagJson({ id: "src:3", hash: "sha256:3", spans_count: 12, events: [], revision: "rev:43:b", semantic_hash: "psg:b", version: "rev:43:b" }),
    spansFail: false,
    ...overrides,
  };
  server.use(
    http.get("*/api/model/version", () =>
      etagJson({ version: "rev:42:a", revision: "rev:42:aaaaaaaaaaaaaaaa", hash: "h", semantic_hash: "h", evidence_hash: "e", created_at: "2026-10-25T11:00:00Z", created_by: "agent:plumb" }, undefined, VERSION_ETAG),
    ),
    http.get("*/api/sources", () => etagJson({ items: b.sources })),
    http.get("*/api/sources/:id", ({ params }) => {
      const source = b.sources.find((s) => s.id === params["id"]);
      return source === undefined
        ? HttpResponse.json({ code: "E_NOT_FOUND", message: "missing", details: {} }, { status: 404 })
        : etagJson({ id: source.id, hash: source.hash, media_type: source.media_type, source_kind: source.source_kind });
    }),
    http.get("*/api/sources/:id/spans", ({ params, request }) => {
      if (b.spansFail) {
        return HttpResponse.json({ code: "E_INTERNAL", message: "boom", details: {} }, { status: 500 });
      }
      if (params["id"] === "src:2") {
        return etagJson({ items: [{ ...span("evd:z", 0, "The policy applies to all employees."), source_ref: "src:2" }], next_cursor: null });
      }
      const cursor = new URL(request.url).searchParams.get("cursor");
      return etagJson(cursor === null ? { items: PAGE_1, next_cursor: "page-2" } : { items: PAGE_2, next_cursor: null });
    }),
    http.post("*/api/sources", async ({ request }) => {
      const file = await uploadedFile(request);
      b.uploads.push({ ...file, ifMatch: request.headers.get("if-match") });
      return b.upload();
    }),
  );
  return b;
}

const main = () => screen.getByRole("main");

describe("Sources", () => {
  it("lists the documents and reads the first, highlighting the passages that became requirements", async () => {
    serve();
    const { user, router } = renderApp("/sources");
    await screen.findByRole("main");
    expect(await within(main()).findByRole("heading", { level: 1, name: "Sources" })).toBeTruthy();
    const list = within(main()).getByRole("list", { name: "Documents" });
    const rows = await within(list).findAllByRole("link");
    expect(rows.map((r) => r.textContent)).toEqual(["requirements.mdMarkdown · 4 passages · imported today", "leave-policy.docxWord · 1 passage · imported 21 Oct"]);
    expect(rows[0]?.getAttribute("aria-current")).toBe("page");
    const reading = within(main()).getByRole("article");
    expect(await within(reading).findByText("requirements.md · 4 passages · 2 became requirements")).toBeTruthy();
    // Document order, the heading passage as a heading, markdown emphasis removed.
    expect(within(reading).getByRole("heading", { name: "3. Leave requests" })).toBeTruthy();
    const passages = reading.querySelectorAll("[data-span]");
    expect(Array.from(passages).map((p) => p.getAttribute("data-span"))).toEqual(["evd:a", "evd:b", "evd:c", "evd:d"]);
    const hr2 = within(reading).getByRole("link", { name: /5\. HR-002 A leave request shall specify a start date\./ });
    expect(hr2.closest("p")?.textContent).toContain("HR-002");
    expect(within(reading).getByText("req:3")).toBeTruthy();
    expect(within(reading).queryAllByRole("link")).toHaveLength(2);
    expect(await axeViolations(main())).toEqual([]);
    // Hover (or focus) explains; click opens the requirement.
    await user.hover(hr2);
    expect(await screen.findByRole("tooltip")).toBeTruthy();
    expect((await screen.findAllByText("Became requirement HR-002 · click to open")).length).toBeGreaterThan(0);
    await user.click(hr2);
    await waitFor(() => expect(router.state.location.pathname).toBe("/requirements/req:2"));
  });

  it("opens a document from the list and deep-links to it", async () => {
    serve();
    const { user, router } = renderApp("/sources/src:2");
    await screen.findByRole("main");
    const reading = within(main()).getByRole("article");
    expect(await within(reading).findByText("leave-policy.docx · 1 passage · 0 became requirements")).toBeTruthy();
    expect(within(reading).getByText("The policy applies to all employees.")).toBeTruthy();
    await user.click(within(within(main()).getByRole("list", { name: "Documents" })).getByRole("link", { name: /requirements\.md/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/sources/src:1"));
    expect(await within(main()).findByText("requirements.md · 4 passages · 2 became requirements")).toBeTruthy();
  });

  it("imports a document with progress and offers the Requirements step", async () => {
    const b = serve();
    const { user } = renderApp("/sources");
    await screen.findByRole("main");
    const input = await within(main()).findByLabelText("Import document");
    await user.upload(input, new File(["Leave policy text"], "leave-policy-2027.txt"));
    await waitFor(() => expect(b.uploads).toEqual([{ name: "leave-policy-2027.txt", type: "text/plain", text: "Leave policy text", ifMatch: VERSION_ETAG }]));
    expect(await screen.findByText("12 passages imported from leave-policy-2027.txt")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Run Requirements step" })).toBeTruthy();
  });

  it("accepts a document only once the model version it is imported against has loaded", async () => {
    serve();
    server.use(http.get("*/api/model/version", () => delay("infinite")));
    renderApp("/sources");
    await screen.findByRole("main");
    const input = await within(main()).findByLabelText("Import document");
    expect(input.hasAttribute("disabled")).toBe(true);
  });

  it("imports a document dropped on the zone", async () => {
    const b = serve();
    renderApp("/sources");
    await screen.findByRole("main");
    const zone = (await within(main()).findByLabelText("Import document")).closest("label") as HTMLElement;
    const file = new File(["# Policy"], "policy.md");
    fireEvent.dragOver(zone, { dataTransfer: { files: [file] } });
    expect(zone.className).toContain("bg-surface");
    fireEvent.dragLeave(zone);
    fireEvent.drop(zone, { dataTransfer: { files: [file] } });
    await waitFor(() => expect(b.uploads.map((u) => [u.name, u.type])).toEqual([["policy.md", "text/markdown"]]));
    expect(await screen.findByText("12 passages imported from policy.md")).toBeTruthy();
  });

  it("shows upload progress while the file is sent, and a large-document note", async () => {
    serve({ upload: () => etagJson({}) });
    server.use(http.post("*/api/sources", () => delay("infinite")));
    renderApp("/sources");
    await screen.findByRole("main");
    const input = await within(main()).findByLabelText("Import document");
    const big = new File(["x"], "big.docx");
    Object.defineProperty(big, "size", { value: 2_000_000 });
    fireEvent.change(input, { target: { files: [big] } });
    const progress = await within(main()).findByRole("progressbar", { name: "Uploading big.docx" });
    expect(progress.getAttribute("aria-valuenow")).toBe("0");
    expect(within(main()).getByText("This is a large document. Analysis may take several minutes.")).toBeTruthy();
  });

  it("explains refused documents in plain language", async () => {
    const b = serve({ upload: () => HttpResponse.json({ code: "E_SOURCE_EMPTY", message: "empty", details: {} }, { status: 400 }) });
    const { user } = renderApp("/sources");
    await screen.findByRole("main");
    const input = await within(main()).findByLabelText("Import document");
    await user.upload(input, new File([""], "empty.md"));
    expect(await within(main()).findByRole("alert")).toBeTruthy();
    expect(within(main()).getByText("The file is empty")).toBeTruthy();
    b.upload = () => HttpResponse.json({ code: "E_UNSUPPORTED_MEDIA_TYPE", message: "no", details: {} }, { status: 415 });
    await user.upload(input, new File(["x"], "notes.md"));
    expect(await within(main()).findByText("Unsupported file type")).toBeTruthy();
    b.upload = () => HttpResponse.json({ code: "E_INTERNAL", message: "boom", details: {} }, { status: 500 });
    await user.upload(input, new File(["x"], "notes.md"));
    expect(await within(main()).findByText("The file could not be imported")).toBeTruthy();
    expect(within(main()).getByText("E_INTERNAL")).toBeTruthy();
  });

  it("is an empty state with the import zone when nothing is imported", async () => {
    serve({ sources: [] });
    renderApp("/sources");
    await screen.findByRole("main");
    expect(await within(main()).findByRole("heading", { name: "No documents yet" })).toBeTruthy();
    expect(within(main()).getByLabelText("Import document")).toBeTruthy();
    expect(calls.some((c) => c.path.endsWith("/spans"))).toBe(false);
  });

  it("shows skeletons while loading, and a read failure with Retry", async () => {
    const b = serve({ spansFail: true });
    server.use(http.get("*/api/sources", () => delay("infinite")));
    renderApp("/sources");
    await screen.findByRole("main");
    expect(within(main()).getAllByRole("status", { name: "Loading" }).length).toBeGreaterThan(0);
    resetApp();
    serve({ spansFail: true });
    renderApp("/sources/src:1");
    await screen.findByRole("main");
    expect(await within(main()).findByText("This document could not be loaded.")).toBeTruthy();
    const before = calls.filter((c) => c.path.endsWith("/spans")).length;
    await act(async () => within(main()).getByRole("button", { name: "Retry" }).click());
    await waitFor(() => expect(calls.filter((c) => c.path.endsWith("/spans")).length).toBe(before + 1));
    void b;
  });
});
