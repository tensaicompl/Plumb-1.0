// UI kit, interactive components: keyboard operation, selection, overlays (sheet, menu, tooltip),
// the toast system and the inspector layout.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { axeViolations } from "../../test/axe";
import {
  DiffLines,
  dismissToast,
  EvidenceQuote,
  FilterChip,
  ListRow,
  ListRowError,
  Menu,
  ReasonSheet,
  SegmentedControl,
  SegmentedLinks,
  Sheet,
  toast,
  Toaster,
  Tooltip,
  TOAST_DURATION_MS,
  useToastStore,
  WithInspector,
} from ".";

afterEach(() => {
  cleanup();
  act(() => dismissToast());
  vi.useRealTimers();
});

const classOf = (element: Element) => element.getAttribute("class") ?? "";

describe("ListRow", () => {
  it("is a keyboard-operable row with selected, stale and density variants", async () => {
    const onSelect = vi.fn();
    render(
      <ul aria-label="Queue">
        <ListRow title="Add operation Submit leave request" subtitle="New operation · Flows step" meta="4 min" leading={<svg />} onSelect={onSelect} />
        <ListRow title="Link Cancel leave request to HR-019" subtitle="Link · Flows step" stale selected density="compact" trailing={<span>t</span>} />
        <ListRowError itemId="HR-017" onRetry={onSelect} />
        <ListRowError itemId="HR-018" />
      </ul>,
    );
    const first = screen.getByRole("button", { name: /Submit leave request/ });
    expect(first.getAttribute("aria-current")).toBeNull();
    expect(classOf(first)).toContain("min-h-14");
    first.focus();
    await userEvent.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledTimes(1);
    const second = screen.getByRole("button", { name: /Cancel leave request/ });
    expect(second.getAttribute("aria-current")).toBe("true");
    expect(classOf(second)).toContain("bg-surface");
    expect(classOf(second)).toContain("min-h-11");
    expect(within(second).getByText("Stale")).toBeTruthy();
    expect(screen.getByText("Couldn't load HR-017.")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onSelect).toHaveBeenCalledTimes(2);
    expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(1);
    expect(await axeViolations()).toEqual([]);
  });
});

describe("EvidenceQuote", () => {
  it("highlights the exact range and links to the source", async () => {
    const onOpen = vi.fn();
    const text = "Submit. An employee shall be able to submit a Draft leave request. The manager decides.";
    const start = text.indexOf("An employee");
    const end = text.indexOf(" The manager");
    render(<EvidenceQuote text={text} highlight={{ start, end }} sourceId="HR-008" sourceName="requirements.md" onOpenSource={onOpen} />);
    const mark = document.querySelector("mark") as HTMLElement;
    expect(mark.textContent).toBe("An employee shall be able to submit a Draft leave request.");
    expect(classOf(mark)).toContain("bg-accent-wash");
    expect(screen.getByText("HR-008")).toBeTruthy();
    expect(screen.getByText("requirements.md")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Open in source" }));
    expect(onOpen).toHaveBeenCalled();
    expect(await axeViolations()).toEqual([]);
  });

  it("renders without a highlight or source action, and clamps a bad range", () => {
    const { rerender } = render(<EvidenceQuote text="Plain passage." />);
    expect(document.querySelector("mark")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
    rerender(<EvidenceQuote text="Short" highlight={{ start: 3, end: 99 }} openHref="/sources/src:1" sourceName="a.md" />);
    expect(document.querySelector("mark")?.textContent).toBe("rt");
    expect(screen.getByRole("link", { name: "Open in source" }).getAttribute("href")).toBe("/sources/src:1");
  });
});

describe("DiffLines", () => {
  it("renders + and − lines plain and boxed, announcing each change", async () => {
    const lines = [
      { op: "remove", text: "working_days([start, end], office calendar)" },
      { op: "add", text: "working_days([start, end], office calendar − public holidays)" },
      { op: "context", text: "unchanged" },
    ] as const;
    const { container, rerender } = render(<DiffLines lines={lines} />);
    const rows = [...container.querySelectorAll("[data-diff-op]")];
    expect(rows.map((r) => r.getAttribute("data-diff-op"))).toEqual(["remove", "add", "context"]);
    expect(classOf(rows[1] as Element)).toContain("text-diff-add");
    expect(rows[1]?.textContent).toContain("Added");
    expect(rows[0]?.textContent).toContain("Removed");
    expect(await axeViolations()).toEqual([]);
    rerender(<DiffLines lines={lines} variant="boxed" title="What changes in the model" />);
    expect(screen.getByText("What changes in the model")).toBeTruthy();
    expect(classOf(container.querySelector("[data-diff-op='add']") as Element)).toContain("bg-diff-add-bg");
    expect(classOf(container.querySelector("[data-diff-op='remove']") as Element)).toContain("bg-diff-del-bg");
  });
});

describe("SegmentedControl", () => {
  function Harness({ onChange }: { onChange: (value: string) => void }) {
    const [value, setValue] = useState("entities");
    return (
      <SegmentedControl
        label="Model view"
        value={value}
        onChange={(next) => {
          setValue(next);
          onChange(next);
        }}
        options={[
          { value: "glossary", label: "Glossary" },
          { value: "entities", label: "Entities" },
          { value: "rules", label: "Rules", disabled: true },
          { value: "calculations", label: "Calculations" },
        ]}
      />
    );
  }

  it("keeps one tab stop on the first enabled option while nothing is chosen yet", () => {
    render(
      <SegmentedControl
        label="Outcome"
        value=""
        onChange={() => undefined}
        options={[
          { value: "maybe", label: "Maybe", disabled: true },
          { value: "yes", label: "Yes" },
          { value: "no", label: "No" },
        ]}
      />,
    );
    const group = screen.getByRole("radiogroup", { name: "Outcome" });
    expect(within(group).getAllByRole("radio").map((radio) => radio.tabIndex)).toEqual([-1, 0, -1]);
  });

  it("is a radio group with roving focus and arrow keys that skip disabled options", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    const group = screen.getByRole("radiogroup", { name: "Model view" });
    const entities = within(group).getByRole("radio", { name: "Entities" });
    expect(entities.getAttribute("aria-checked")).toBe("true");
    expect(entities.tabIndex).toBe(0);
    expect(within(group).getByRole("radio", { name: "Glossary" }).tabIndex).toBe(-1);
    expect(classOf(entities)).toContain("shadow-segment");
    entities.focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenLastCalledWith("calculations");
    expect(document.activeElement?.textContent).toBe("Calculations");
    await userEvent.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenLastCalledWith("glossary");
    await userEvent.keyboard("{ArrowLeft}");
    expect(onChange).toHaveBeenLastCalledWith("calculations");
    await userEvent.keyboard("{Home}");
    expect(onChange).toHaveBeenLastCalledWith("glossary");
    await userEvent.keyboard("{End}");
    expect(onChange).toHaveBeenLastCalledWith("calculations");
    await userEvent.click(within(group).getByRole("radio", { name: "Entities" }));
    expect(onChange).toHaveBeenLastCalledWith("entities");
    expect(await axeViolations()).toEqual([]);
  });

  it("fills its container, can be named by another element and stays reachable with nothing chosen (Inbox Yes/No answer)", async () => {
    const onChange = vi.fn();
    render(
      <div>
        <p id="answer-heading">Answer</p>
        <SegmentedControl
          labelledBy="answer-heading"
          fill
          value={null}
          onChange={onChange}
          options={[
            { value: "yes", label: "Yes" },
            { value: "no", label: "No" },
          ]}
        />
      </div>,
    );
    const group = screen.getByRole("radiogroup", { name: "Answer" });
    expect(classOf(group)).toContain("w-full");
    const [yes, no] = within(group).getAllByRole("radio") as [HTMLElement, HTMLElement];
    expect(classOf(yes)).toContain("flex-1");
    expect(yes.getAttribute("aria-checked")).toBe("false");
    expect(yes.tabIndex).toBe(0);
    expect(no.tabIndex).toBe(-1);
    yes.focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenLastCalledWith("no");
    expect(await axeViolations()).toEqual([]);
  });

  it("renders navigation segments as links that navigate on a plain click", async () => {
    const onNavigate = vi.fn();
    render(
      <SegmentedLinks
        label="Quality views"
        onNavigate={onNavigate}
        items={[
          { href: "/quality", label: "Gates", active: true },
          { href: "/quality/history", label: "History", active: false },
        ]}
      />,
    );
    const nav = screen.getByRole("navigation", { name: "Quality views" });
    expect(within(nav).getByRole("link", { name: "Gates" }).getAttribute("aria-current")).toBe("page");
    await userEvent.click(within(nav).getByRole("link", { name: "History" }));
    expect(onNavigate).toHaveBeenCalledWith("/quality/history");
    fireEvent.click(within(nav).getByRole("link", { name: "History" }), { metaKey: true });
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(await axeViolations()).toEqual([]);
  });
});

describe("FilterChip", () => {
  it("toggles with a pressed state and an optional count", async () => {
    const onToggle = vi.fn();
    const { rerender } = render(<FilterChip label="Has issues" onToggle={onToggle} />);
    const chip = screen.getByRole("button", { name: "Has issues" });
    expect(chip.getAttribute("aria-pressed")).toBe("false");
    await userEvent.click(chip);
    expect(onToggle).toHaveBeenCalled();
    rerender(<FilterChip label="Has issues" active count={2} onToggle={onToggle} />);
    const active = screen.getByRole("button", { name: /Has issues/ });
    expect(active.getAttribute("aria-pressed")).toBe("true");
    expect(active.textContent).toContain("2");
    expect(classOf(active)).toContain("border-primary");
    expect(await axeViolations()).toEqual([]);
  });

  it("works as a Menu trigger (Kind, Status): a chevron, no pressed state, the menu opens", async () => {
    const onPick = vi.fn();
    render(
      <Menu
        trigger={<FilterChip label="Kind: Functional" active menu />}
        items={[{ id: "quality", label: "Quality", onSelect: onPick }]}
      />,
    );
    const chip = screen.getByRole("button", { name: "Kind: Functional" });
    expect(chip.getAttribute("aria-pressed")).toBeNull();
    expect(chip.querySelector("svg")).not.toBeNull();
    expect(classOf(chip)).toContain("border-primary");
    await userEvent.click(chip);
    await userEvent.click(await screen.findByRole("menuitem", { name: "Quality" }));
    expect(onPick).toHaveBeenCalled();
  });
});

describe("Tooltip and Menu", () => {
  it("shows a tooltip on keyboard focus", async () => {
    render(
      <Tooltip content="Approve leave request updates Leave request">
        <button type="button">U</button>
      </Tooltip>,
    );
    await userEvent.tab();
    expect((await screen.findAllByText("Approve leave request updates Leave request")).length).toBeGreaterThan(0);
  });

  it("opens a menu of steps with meta text and runs the chosen item", async () => {
    const onExtract = vi.fn();
    render(
      <Menu
        title="Run a step"
        trigger={<button type="button">Steps</button>}
        items={[
          { id: "extract", label: "Requirements step", meta: "Done", onSelect: onExtract },
          { id: "lint", label: "Glossary step", disabled: true, onSelect: vi.fn() },
        ]}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Steps" }));
    const menu = await screen.findByRole("menu");
    expect(within(menu).getByText("Run a step")).toBeTruthy();
    expect(within(menu).getByRole("menuitem", { name: /Requirements step/ }).textContent).toContain("Done");
    expect(within(menu).getByRole("menuitem", { name: /Glossary step/ }).getAttribute("data-disabled")).not.toBeNull();
    await userEvent.click(within(menu).getByRole("menuitem", { name: /Requirements step/ }));
    expect(onExtract).toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  });
});

describe("Sheet", () => {
  it("is a titled dialog; Esc cancels and ⌘↩ confirms", async () => {
    const onConfirm = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <Sheet open onOpenChange={onOpenChange} title="Export specification" description="Choose the files." confirmLabel="Download" onConfirm={onConfirm}>
        <p>Body</p>
      </Sheet>,
    );
    const dialog = screen.getByRole("dialog", { name: "Export specification" });
    expect(within(dialog).getByText("Choose the files.")).toBeTruthy();
    expect(classOf(dialog)).toContain("rounded-sheet");
    expect(classOf(dialog)).toContain("w-[480px]");
    const confirm = within(dialog).getByRole("button", { name: /Download/ });
    expect(classOf(confirm)).toContain("bg-primary");
    expect(confirm.textContent).toMatch(/(⌘|Ctrl)↩/);
    await userEvent.keyboard("{Meta>}{Enter}{/Meta}");
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await userEvent.keyboard("{Control>}{Enter}{/Control}");
    expect(onConfirm).toHaveBeenCalledTimes(2);
    await userEvent.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(await axeViolations()).toEqual([]);
  });

  it("supports a custom footer, a header action, the wide width, and a non-dismissible mode", async () => {
    const onOpenChange = vi.fn();
    render(
      <Sheet open onOpenChange={onOpenChange} title="Coverage" width={760} footer={<span>custom footer</span>} headerAction={<button type="button">x</button>} dismissible={false}>
        <p>Matrix</p>
      </Sheet>,
    );
    const dialog = screen.getByRole("dialog", { name: "Coverage" });
    expect(classOf(dialog)).toContain("w-[760px]");
    expect(within(dialog).getByText("custom footer")).toBeTruthy();
    await userEvent.keyboard("{Escape}");
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("renders the cancel-only footer when there is no confirm action", async () => {
    const onOpenChange = vi.fn();
    render(<Sheet open onOpenChange={onOpenChange} title="Changes" cancelLabel="Close" />);
    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    await userEvent.keyboard("{Meta>}{Enter}{/Meta}");
  });
});

describe("ReasonSheet", () => {
  it("disables Confirm until a reason is typed, confirms with ⌘↩ and sends the trimmed reason", async () => {
    const onConfirm = vi.fn();
    render(
      <ReasonSheet
        open
        onOpenChange={() => undefined}
        title="Reject proposal"
        description="It will not be added."
        confirmLabel="Reject"
        destructive
        onConfirm={onConfirm}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Reject proposal" });
    const confirm = within(dialog).getByRole("button", { name: /Reject/ }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
    expect(classOf(confirm)).toContain("bg-blocked");
    const reason = within(dialog).getByRole("textbox", { name: "Reason (required)" });
    expect(document.activeElement).toBe(reason);
    await userEvent.keyboard("{Meta>}{Enter}{/Meta}");
    expect(onConfirm).not.toHaveBeenCalled();
    await userEvent.type(reason, "   ");
    expect(confirm.disabled).toBe(true);
    await userEvent.type(reason, "Duplicate of HR-019 ");
    expect(confirm.disabled).toBe(false);
    await userEvent.keyboard("{Meta>}{Enter}{/Meta}");
    expect(onConfirm).toHaveBeenCalledWith("Duplicate of HR-019");
    expect(await axeViolations()).toEqual([]);
  });

  it("shows a governed choice above the reason and keeps Confirm disabled until it is made", async () => {
    const onConfirm = vi.fn();
    const { rerender } = render(
      <ReasonSheet open onOpenChange={() => undefined} title="Set outcome" confirmLabel="Set outcome" confirmDisabled onConfirm={onConfirm}>
        <p>Choose the outcome</p>
      </ReasonSheet>,
    );
    const dialog = screen.getByRole("dialog", { name: "Set outcome" });
    expect(within(dialog).getByText("Choose the outcome")).toBeTruthy();
    await userEvent.type(within(dialog).getByRole("textbox", { name: "Reason (required)" }), "Policy");
    const confirm = within(dialog).getByRole("button", { name: /Set outcome/ }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
    rerender(
      <ReasonSheet open onOpenChange={() => undefined} title="Set outcome" confirmLabel="Set outcome" confirmDisabled={false} onConfirm={onConfirm}>
        <p>Choose the outcome</p>
      </ReasonSheet>,
    );
    expect(confirm.disabled).toBe(false);
    await userEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalledWith("Policy");
  });

  it("allows an optional reason and shows a pending state and an error", async () => {
    const onConfirm = vi.fn();
    const { rerender } = render(
      <ReasonSheet open onOpenChange={() => undefined} title="Waive" confirmLabel="Waive" reasonRequired={false} reasonPlaceholder="Why?" onConfirm={onConfirm} />,
    );
    const confirm = screen.getByRole("button", { name: /Waive/ }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(false);
    expect(screen.getByRole("textbox", { name: "Reason" }).getAttribute("placeholder")).toBe("Why?");
    await userEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalledWith("");
    rerender(
      <ReasonSheet open onOpenChange={() => undefined} title="Waive" confirmLabel="Waive" reasonRequired={false} pending error="The server refused." onConfirm={onConfirm} />,
    );
    expect(screen.getByRole("alert").textContent).toBe("The server refused.");
    expect((screen.getByRole("button", { name: /Waive/ }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("clears the reason each time it opens", async () => {
    function Harness() {
      const [open, setOpen] = useState(true);
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>
            reopen
          </button>
          <ReasonSheet open={open} onOpenChange={setOpen} title="Discard" confirmLabel="Discard" onConfirm={() => setOpen(false)} />
        </>
      );
    }
    render(<Harness />);
    await userEvent.type(screen.getByRole("textbox"), "first");
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await userEvent.click(screen.getByRole("button", { name: "reopen" }));
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("");
  });
});

describe("Toast", () => {
  it("shows one toast bottom-centre with an action pill and auto-dismisses after 3.8 s", async () => {
    vi.useFakeTimers();
    const onAction = vi.fn();
    render(<Toaster />);
    act(() => toast({ message: "Operation added to the model", action: { label: "View in Model", onAction } }));
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("Operation added to the model");
    const region = screen.getByRole("region", { name: "Notifications" });
    expect(classOf(region.firstElementChild as Element)).toContain("bottom-7");
    act(() => {
      vi.advanceTimersByTime(TOAST_DURATION_MS - 10);
    });
    expect(screen.queryByText("Operation added to the model")).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(20);
    });
    expect(screen.queryByText("Operation added to the model")).toBeNull();
    expect(TOAST_DURATION_MS).toBe(3800);
  });

  it("runs the action and dismisses; a new toast replaces the current one; errors are alerts", async () => {
    const onAction = vi.fn();
    render(<Toaster />);
    act(() => toast({ message: "First" }));
    act(() => toast({ message: "Proposal rejected", action: { label: "Undo", onAction } }));
    expect(screen.queryByText("First")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(onAction).toHaveBeenCalled();
    expect(screen.queryByText("Proposal rejected")).toBeNull();
    act(() => toast({ message: "The step failed", tone: "error" }));
    expect(screen.getByRole("alert").textContent).toContain("The step failed");
    act(() => toast({ message: "Info", tone: "info" }));
    expect(useToastStore.getState().current?.tone).toBe("info");
    expect(await axeViolations()).toEqual([]);
  });

  it("pauses while hovered", () => {
    vi.useFakeTimers();
    render(<Toaster />);
    act(() => toast({ message: "Hold on" }));
    const card = screen.getByText("Hold on").closest("[data-toast]") as HTMLElement;
    fireEvent.mouseEnter(card);
    act(() => {
      vi.advanceTimersByTime(TOAST_DURATION_MS * 2);
    });
    expect(screen.queryByText("Hold on")).not.toBeNull();
    fireEvent.mouseLeave(card);
    act(() => {
      vi.advanceTimersByTime(TOAST_DURATION_MS + 1);
    });
    expect(screen.queryByText("Hold on")).toBeNull();
  });
});

describe("WithInspector", () => {
  it("lays out main + inspector with the clamp width, a sticky action bar and an empty state", async () => {
    const { rerender } = render(
      <WithInspector inspector={null}>
        <h1>List</h1>
      </WithInspector>,
    );
    const aside = screen.getByRole("complementary", { name: "Details" });
    expect(aside.getAttribute("style") ?? "").toContain("clamp(320px, 32vw, 420px)");
    expect(within(aside).getByText("Select an item")).toBeTruthy();
    rerender(
      <WithInspector inspector={<p>HR-009</p>} actions={<button type="button">Accept</button>} width="wide" label="Requirement" inspectorKey="HR-009">
        <h1>List</h1>
      </WithInspector>,
    );
    const filled = screen.getByRole("complementary", { name: "Requirement" });
    expect(filled.getAttribute("style") ?? "").toContain("clamp(320px, 32vw, 440px)");
    expect(within(filled).getByText("HR-009")).toBeTruthy();
    const bar = within(filled).getByRole("button", { name: "Accept" }).parentElement as HTMLElement;
    expect(classOf(bar)).toContain("border-t");
    expect(classOf(bar)).toContain("sticky");
    expect(await axeViolations()).toEqual([]);
    rerender(
      <WithInspector inspector={null} empty={<span>Pick a requirement</span>}>
        <h1>List</h1>
      </WithInspector>,
    );
    expect(screen.getByText("Pick a requirement")).toBeTruthy();
  });
});
