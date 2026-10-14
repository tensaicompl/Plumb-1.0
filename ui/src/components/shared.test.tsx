// U1.2 shared component tests. Every component is exercised with pure fixtures: no MSW handler,
// no backend and no network (fetch is spied and must never be called).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";

import { useConflictStore } from "../api/useVersionedMutation";
import { ConflictDialog } from "./ConflictDialog";
import { DiagramFrame, computeLayout, layoutRequest, type DiagramEdge, type DiagramNode } from "./DiagramFrame";
import { DiffViewer, type ModelDiff } from "./DiffViewer";
import { FindingsPanel, type Finding } from "./FindingsPanel";
import { BADGE_STATUSES, GateBadge } from "./GateBadge";
import { isActionEnabled, ProposalReviewBar, type IntakeReport, type ProposalReviewAction } from "./ProposalReviewBar";
import { PROVENANCE_CATEGORIES, ProvenanceDrawer, type ProvenanceItem } from "./ProvenanceDrawer";
import {
  TypedAnswerControl,
  answerRenderer,
  isAnswerComplete,
  parseAnswerSchema,
  type AnswerValue,
  type QuestionKind,
} from "./TypedAnswerControl";

let fetchSpy: MockInstance<typeof fetch>;

beforeEach(() => {
  fetchSpy = vi.spyOn(globalThis, "fetch");
});

afterEach(() => {
  cleanup();
  useConflictStore.getState().clearConflict();
  expect(fetchSpy).not.toHaveBeenCalled();
  vi.restoreAllMocks();
});

// ------------------------------------------------------------------ ProposalReviewBar

function intake(overrides: Partial<IntakeReport> = {}): IntakeReport {
  return {
    requires_reason: false,
    reason_codes: [],
    validation_findings: [],
    conflicts_with: [],
    duplicate_candidates: [],
    covered_by: [],
    refines: [],
    new_terms: [],
    novelty: null,
    ...overrides,
  };
}

function actionButton(action: ProposalReviewAction): HTMLButtonElement {
  const button = document.querySelector<HTMLButtonElement>(`button[data-action="${action}"]`);
  if (button === null) {
    throw new Error("no button for " + action);
  }
  return button;
}

describe("shared ProposalReviewBar", () => {
  it("enables create and accept by supplied eligibility when no reason is required", () => {
    render(
      <ProposalReviewBar status="pending" intake={intake()} enabledActions={["accept", "create", "reject"]} onAction={vi.fn()} />,
    );
    expect(actionButton("accept").disabled).toBe(false);
    expect(actionButton("create").disabled).toBe(false);
    expect(actionButton("reject").disabled).toBe(false);
    expect(document.querySelector('button[data-action="merge"]')).toBeNull();
    expect(screen.getByTestId("proposal-requires-reason").textContent).toBe("No");
    cleanup();
    // Eligibility comes only from the caller.
    render(<ProposalReviewBar status="pending" intake={intake()} enabledActions={["reject"]} onAction={vi.fn()} />);
    expect(actionButton("accept").disabled).toBe(true);
    expect(actionButton("create").disabled).toBe(true);
  });

  it("disables every positive commit action until a non-blank rationale exists when the server requires one (Hotfix 078)", async () => {
    const user = userEvent.setup();
    const positive = ["accept", "create", "link", "merge", "supersede"] as const;
    render(
      <ProposalReviewBar
        status="pending"
        intake={intake({ requires_reason: true, reason_codes: ["material_decision", "validation_conflict"] })}
        enabledActions={[...positive, "reject"]}
        onAction={vi.fn()}
      />,
    );
    const disabled = () => positive.map((action) => [action, actionButton(action).disabled]);
    // requires_reason gates all five positive actions alike; reject is never reason-gated.
    expect(disabled()).toEqual(positive.map((action) => [action, true]));
    expect(actionButton("reject").disabled).toBe(false);
    // The server's reason codes are shown exactly; none is invented or preselected.
    const codes = within(screen.getByTestId("proposal-reason-codes")).getAllByRole("listitem").map((li) => li.textContent);
    expect(codes).toEqual(["material_decision", "validation_conflict"]);

    const rationale = screen.getByRole("textbox", { name: "Rationale" });
    expect((rationale as HTMLTextAreaElement).value).toBe("");
    for (const blank of ["   ", "{Enter}"]) {
      await user.type(rationale, blank);
      expect(disabled()).toEqual(positive.map((action) => [action, true]));
      expect(actionButton("reject").disabled).toBe(false);
    }
    await user.type(rationale, "Confirmed with HR");
    expect(disabled()).toEqual(positive.map((action) => [action, false]));
    expect(actionButton("reject").disabled).toBe(false);
    await user.clear(rationale);
    expect(disabled()).toEqual(positive.map((action) => [action, true]));
    // The same rule as a pure function, including a newline-only rationale.
    const report = intake({ requires_reason: true, reason_codes: ["human_decision"] });
    for (const action of positive) {
      expect(isActionEnabled(action, [action], report, "\n")).toBe(false);
      expect(isActionEnabled(action, [action], report, "ok")).toBe(true);
      expect(isActionEnabled(action, [], report, "ok")).toBe(false);
    }
  });

  it("does not expose link, merge or supersede unless offered, while accept/create stay rendered (Hotfix 078)", () => {
    render(
      <ProposalReviewBar
        status="pending"
        intake={intake({ requires_reason: true, reason_codes: ["material_decision"] })}
        enabledActions={["create", "reject"]}
        onAction={vi.fn()}
      />,
    );
    expect(actionButton("create")).toBeTruthy();
    expect(actionButton("reject")).toBeTruthy();
    for (const absent of ["link", "merge", "supersede"]) {
      expect(document.querySelector('button[data-action="' + absent + '"]')).toBeNull();
    }
    // The established shared behaviour keeps accept rendered, disabled, when not offered.
    expect(actionButton("accept").disabled).toBe(true);
  });

  it("does not reason-gate reject, edit or explain (Hotfix 078)", () => {
    const report = intake({ requires_reason: true, reason_codes: ["declared_conflict"] });
    for (const action of ["reject", "edit", "explain"] as const) {
      expect(isActionEnabled(action, [action], report, "")).toBe(true);
      expect(isActionEnabled(action, [], report, "")).toBe(false);
    }
    render(<ProposalReviewBar status="pending" intake={report} enabledActions={["reject", "edit", "explain"]} onAction={vi.fn()} />);
    for (const action of ["reject", "edit", "explain"] as const) {
      expect(actionButton(action).disabled).toBe(false);
    }
  });

  it("shows the server intake report and reports actions only through the callback", async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    render(
      <ProposalReviewBar
        status="pending"
        intake={intake({
          requires_reason: true,
          reason_codes: ["human_decision"],
          validation_findings: [
            { id: "fnd:1", code: "PLUMB.F1.REQ.NO_CONTRADICTION", family: "F1", rule_class: "plumb_core", severity: "blocker", status: "Open", message: "contradiction", affected_refs: ["req:1"] },
          ],
          conflicts_with: [{ target_ref: "req:2", reason: "declared" }],
          duplicate_candidates: [{ subject_ref: "req:new", candidate_ref: "req:3", score: 0.9 }],
          covered_by: [{ subject_ref: "req:new", covered_by_ref: "req:4", score: 0.8 }],
          refines: ["req:5"],
          new_terms: ["sick leave"],
          novelty: 0.25,
        })}
        enabledActions={["accept", "create", "reject", "explain"]}
        onAction={onAction}
      />,
    );
    const bar = screen.getByRole("region", { name: "Proposal review" });
    for (const text of ["PLUMB.F1.REQ.NO_CONTRADICTION", "req:2", "declared", "req:3", "req:4", "req:5", "sick leave", "human_decision"]) {
      expect(bar.textContent).toContain(text);
    }
    expect(screen.getByTestId("proposal-novelty").textContent).toBe("0.25");

    // Disabled actions do nothing.
    await user.click(actionButton("accept"));
    expect(onAction).not.toHaveBeenCalled();
    await user.click(actionButton("explain"));
    expect(onAction).toHaveBeenLastCalledWith("explain", "");
    await user.type(screen.getByRole("textbox", { name: "Rationale" }), "Policy approved");
    await user.click(actionButton("accept"));
    expect(onAction).toHaveBeenLastCalledWith("accept", "Policy approved");
    expect(onAction).toHaveBeenCalledTimes(2);
  });

  it("shows a null novelty as not calculated", () => {
    render(<ProposalReviewBar status="pending" intake={intake()} enabledActions={[]} onAction={vi.fn()} />);
    expect(screen.getByTestId("proposal-novelty").textContent).toBe("Not calculated");
  });
});

// ------------------------------------------------------------------ ProvenanceDrawer

describe("shared ProvenanceDrawer", () => {
  it("distinguishes the four provenance classes as declared, never by reference text", () => {
    // References deliberately look like other classes: the category comes only from the caller.
    const items: ProvenanceItem[] = [
      { category: "evidence", ref: "drv:looks-derived", detail: "requirements.md 12-48" },
      { category: "deterministic_derivation", ref: "sha256:looks-like-artifact" },
      { category: "human_resolution", ref: "dec:0011", detail: "alice" },
      { category: "llm_inference", ref: "frag:looks-like-evidence", detail: "test-model" },
    ];
    render(<ProvenanceDrawer open onOpenChange={vi.fn()} subjectRef="req:leave-1" items={items} />);
    const drawer = screen.getByRole("dialog");
    expect(within(drawer).getByTestId("provenance-subject").textContent).toBe("req:leave-1");
    const sections = Array.from(drawer.querySelectorAll("section[data-category]"));
    expect(sections.map((s) => s.getAttribute("data-category"))).toEqual([...PROVENANCE_CATEGORIES]);
    const headings = sections.map((s) => s.querySelector("h3")?.textContent);
    expect(headings).toEqual(["Evidence", "Deterministic derivation", "Human resolution", "LLM inference"]);
    expect(new Set(headings).size).toBe(4);
    const placed = Object.fromEntries(sections.map((s) => [s.getAttribute("data-category"), s.querySelector(".font-mono")?.textContent]));
    expect(placed).toEqual({
      evidence: "drv:looks-derived",
      deterministic_derivation: "sha256:looks-like-artifact",
      human_resolution: "dec:0011",
      llm_inference: "frag:looks-like-evidence",
    });
  });
});

// ------------------------------------------------------------------ FindingsPanel

const FINDINGS: Finding[] = [
  { id: "fnd:a", code: "PLUMB.F1.REQ.EARS", family: "F1", rule_class: null, severity: "blocker", status: "Open", message: "not EARS", affected_refs: ["req:1"] },
  { id: "fnd:b", code: "PLUMB.F2.DOMAIN.RELATION_TYPED", family: "F2", rule_class: "plumb_core", severity: "major", status: "Open", message: "untyped", affected_refs: ["rel:2", "ent:3"] },
  { id: "fnd:c", code: "PLUMB.F1.REQ.EARS", family: "F1", rule_class: null, severity: "blocker", status: "Resolved", message: "fixed", affected_refs: [], waiver_ref: "dec:9" },
];

function shownFindings(): string[] {
  return Array.from(document.querySelectorAll("li[data-finding-id]")).map((li) => li.getAttribute("data-finding-id") ?? "");
}

describe("shared FindingsPanel", () => {
  it("filters by server fields, pins locally and opens refs through the callback", async () => {
    const user = userEvent.setup();
    const onOpenRef = vi.fn();
    const before = structuredClone(FINDINGS);
    render(<FindingsPanel findings={FINDINGS} onOpenRef={onOpenRef} />);
    expect(shownFindings()).toEqual(["fnd:a", "fnd:b", "fnd:c"]);

    const severity = screen.getByRole("combobox", { name: "Severity" });
    const status = screen.getByRole("combobox", { name: "Status" });
    // Filter options are the server values in first-appearance order.
    expect(Array.from((severity as HTMLSelectElement).options).map((o) => o.value)).toEqual(["", "blocker", "major"]);
    await user.selectOptions(severity, "blocker");
    expect(shownFindings()).toEqual(["fnd:a", "fnd:c"]);
    await user.selectOptions(status, "Resolved");
    expect(shownFindings()).toEqual(["fnd:c"]);
    await user.selectOptions(severity, "");
    await user.selectOptions(status, "");
    await user.selectOptions(screen.getByRole("combobox", { name: "Family" }), "F2");
    expect(shownFindings()).toEqual(["fnd:b"]);
    await user.selectOptions(screen.getByRole("combobox", { name: "Family" }), "");
    await user.selectOptions(screen.getByRole("combobox", { name: "Code" }), "PLUMB.F1.REQ.EARS");
    expect(shownFindings()).toEqual(["fnd:a", "fnd:c"]);
    await user.selectOptions(screen.getByRole("combobox", { name: "Code" }), "");

    // Pinning is local view state: pinned findings list first, nothing changes in the data.
    const findingB = document.querySelector('li[data-finding-id="fnd:b"]') as HTMLElement;
    await user.click(within(findingB).getByRole("button", { name: "Pin" }));
    expect(shownFindings()).toEqual(["fnd:b", "fnd:a", "fnd:c"]);
    expect(document.querySelector('ul[data-list="pinned"] li[data-finding-id="fnd:b"]')).not.toBeNull();
    const pinnedB = document.querySelector('li[data-finding-id="fnd:b"]') as HTMLElement;
    expect(within(pinnedB).getByRole("button", { name: "Unpin" }).getAttribute("aria-pressed")).toBe("true");
    expect(within(pinnedB).getByText("Open")).toBeTruthy();
    await user.click(within(pinnedB).getByRole("button", { name: "Unpin" }));
    expect(shownFindings()).toEqual(["fnd:a", "fnd:b", "fnd:c"]);
    expect(FINDINGS).toEqual(before);

    // Affected refs open only through the supplied callback.
    await user.click(screen.getByRole("button", { name: "ent:3" }));
    expect(onOpenRef).toHaveBeenCalledWith("ent:3");
    expect(onOpenRef).toHaveBeenCalledTimes(1);
  });
});

// ------------------------------------------------------------------ DiffViewer

describe("shared DiffViewer", () => {
  it("shows added, removed and modified separately in server order", () => {
    const diff: ModelDiff = {
      from: "rev:1:aaaaaaaaaaaaaaaa",
      to: "rev:2:bbbbbbbbbbbbbbbb",
      added: ["req:z", "req:a"],
      removed: ["term:old"],
      modified: [],
    };
    render(<DiffViewer diff={diff} />);
    expect(screen.getByTestId("diff-from").textContent).toBe(diff.from);
    expect(screen.getByTestId("diff-to").textContent).toBe(diff.to);
    const section = (key: string) => document.querySelector(`section[data-diff="${key}"]`) as HTMLElement;
    const ids = (key: string) => Array.from(section(key).querySelectorAll("li")).map((li) => li.textContent);
    expect(ids("added")).toEqual(["req:z", "req:a"]);
    expect(ids("removed")).toEqual(["term:old"]);
    expect(ids("modified")).toEqual([]);
    expect(within(section("modified")).getByText("None")).toBeTruthy();
    expect(within(section("added")).getByTestId("diff-count").textContent).toBe("2");
  });
});

// ------------------------------------------------------------------ GateBadge

describe("shared GateBadge", () => {
  it("renders every status as exact text and never as a score", () => {
    expect(BADGE_STATUSES).toEqual(["PASS", "FAIL", "WARN", "WAIVED", "ERROR"]);
    for (const status of BADGE_STATUSES) {
      const { container, unmount } = render(<GateBadge status={status} label="F1" />);
      const badge = container.querySelector("[data-status]") as HTMLElement;
      expect(badge.getAttribute("data-status")).toBe(status);
      expect(within(badge).getByTestId("badge-status").textContent).toBe(status);
      // Accessible text names the status; color classes are not the only signal.
      expect(badge.textContent).toContain(status);
      expect(badge.textContent).not.toMatch(/%|\d{2,}|score|readiness/i);
      expect(container.querySelector('[role="progressbar"], [role="meter"], progress, meter')).toBeNull();
      unmount();
    }
  });
});

// ------------------------------------------------------------------ TypedAnswerControl

const ALL_KINDS: Record<QuestionKind, true> = {
  YesNo: true,
  PickOne: true,
  PickMany: true,
  Number: true,
  Text: true,
  Cardinality: true,
  Unit: true,
  Precision: true,
  Rounding: true,
  FormulaConfirm: true,
  RuleCell: true,
  Calendar: true,
  RoleAssignment: true,
  Permission: true,
  QualityThreshold: true,
  ArchitectureChoice: true,
  TechnologyChoice: true,
  InterfaceChoice: true,
  VerificationMethod: true,
};
const KINDS = Object.keys(ALL_KINDS) as QuestionKind[];

function closed(properties: Record<string, unknown>): Record<string, unknown> {
  return { type: "object", additionalProperties: false, required: Object.keys(properties), properties };
}

const enumOf = (...values: string[]) => ({ type: "string", enum: values });

/** A schema per kind, exercising the closed-object primitives (field names chosen by the schema). */
const SCHEMAS: Record<QuestionKind, Record<string, unknown>> = {
  YesNo: closed({ confirmed: { type: "boolean" } }),
  PickOne: closed({ trigger_ref: enumOf("evt:submitted", "op:approve") }),
  PickMany: closed({ channel_refs: { type: "array", uniqueItems: true, minItems: 1, items: enumOf("ch:email", "ch:sms", "ch:web") } }),
  Number: closed({ value: { type: "number", minimum: 0, maximum: 365 } }),
  Text: closed({ text: { type: "string", minLength: 1 } }),
  Cardinality: closed({ cardinality_from: enumOf("one", "many"), cardinality_to: enumOf("one", "many") }),
  Unit: closed({ unit: enumOf("day", "hour") }),
  Precision: closed({ scale: { type: "integer", minimum: 0, maximum: 28 } }),
  Rounding: closed({ mode: enumOf("half_up", "half_even") }),
  FormulaConfirm: closed({ expression: { type: "string", minLength: 1 } }),
  RuleCell: closed({ cell: { type: "string", minLength: 1 } }),
  Calendar: closed({ calendar_ref: enumOf("cal:office") }),
  RoleAssignment: closed({ performer_ref: enumOf("role:manager", "role:hr") }),
  Permission: closed({ operation_ref: enumOf("op:approve"), resource_scope_ref: enumOf("scope:team", "scope:all") }),
  QualityThreshold: closed({ threshold: { type: "number" } }),
  ArchitectureChoice: closed({ option_ref: enumOf("arch:a", "arch:b") }),
  TechnologyChoice: closed({ option_ref: enumOf("tech:a") }),
  InterfaceChoice: closed({ option_ref: enumOf("if:rest") }),
  VerificationMethod: closed({ method: enumOf("test", "inspection") }),
};

/** A stateful harness: TypedAnswerControl is controlled. */
function AnswerHarness({ kind, schema, onChange }: { kind: QuestionKind; schema: unknown; onChange: (v: AnswerValue) => void }) {
  const [value, setValue] = useState<AnswerValue>({});
  return (
    <TypedAnswerControl
      question={{ id: "q:1", kind, answer_schema: schema as Record<string, never> | null }}
      value={value}
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}

function lastValue(onChange: ReturnType<typeof vi.fn>): unknown {
  return onChange.mock.calls.at(-1)?.[0];
}

describe("shared TypedAnswerControl", () => {
  it("maps every generated QuestionKind to its own explicit renderer", () => {
    expect(KINDS).toHaveLength(19);
    const renderers = KINDS.map(answerRenderer);
    expect(new Set(renderers).size).toBe(KINDS.length);
    for (const kind of KINDS) {
      const { container, unmount } = render(<AnswerHarness kind={kind} schema={SCHEMAS[kind]} onChange={vi.fn()} />);
      const control = container.querySelector("[data-renderer]") as HTMLElement;
      expect(control.getAttribute("data-renderer")).toBe(answerRenderer(kind));
      expect(control.getAttribute("data-answer-state")).toBe("ready");
      // Field names come from the schema.
      const fields = Array.from(control.querySelectorAll("[data-field]")).map((f) => f.getAttribute("data-field"));
      expect(fields).toEqual(Object.keys((SCHEMAS[kind]["properties"] as object) ?? {}));
      unmount();
    }
  });

  it("emits exact answer objects for representative kinds", async () => {
    const user = userEvent.setup();
    const cases: [QuestionKind, (c: HTMLElement) => Promise<void>, unknown][] = [
      ["YesNo", (c) => user.click(within(c).getByRole("radio", { name: "Yes" })), { confirmed: true }],
      ["PickOne", (c) => user.selectOptions(within(c).getByRole("combobox", { name: "trigger_ref" }), "op:approve"), { trigger_ref: "op:approve" }],
      [
        "PickMany",
        async (c) => {
          await user.click(within(c).getByRole("checkbox", { name: "ch:web" }));
          await user.click(within(c).getByRole("checkbox", { name: "ch:email" }));
        },
        { channel_refs: ["ch:email", "ch:web"] },
      ],
      ["Number", (c) => user.type(within(c).getByRole("spinbutton", { name: "value" }), "12"), { value: 12 }],
      ["Text", (c) => user.type(within(c).getByRole("textbox", { name: "text" }), "Annual leave"), { text: "Annual leave" }],
      [
        "Cardinality",
        async (c) => {
          await user.selectOptions(within(c).getByRole("combobox", { name: "cardinality_from" }), "one");
          await user.selectOptions(within(c).getByRole("combobox", { name: "cardinality_to" }), "many");
        },
        { cardinality_from: "one", cardinality_to: "many" },
      ],
      ["FormulaConfirm", (c) => user.type(within(c).getByRole("textbox", { name: "expression" }), "days > 0"), { expression: "days > 0" }],
      [
        "Permission",
        async (c) => {
          await user.selectOptions(within(c).getByRole("combobox", { name: "operation_ref" }), "op:approve");
          await user.selectOptions(within(c).getByRole("combobox", { name: "resource_scope_ref" }), "scope:team");
        },
        { operation_ref: "op:approve", resource_scope_ref: "scope:team" },
      ],
    ];
    for (const [kind, act, expected] of cases) {
      const onChange = vi.fn();
      const { container, unmount } = render(<AnswerHarness kind={kind} schema={SCHEMAS[kind]} onChange={onChange} />);
      await act(container);
      expect(lastValue(onChange)).toEqual(expected);
      expect(isAnswerComplete(SCHEMAS[kind], expected as AnswerValue)).toBe(true);
      unmount();
    }
  });

  it("takes field names, required fields and choices only from answer_schema", async () => {
    const user = userEvent.setup();
    // The same kind with a different schema renders the schema's fields, not kind-implied ones.
    const schema = closed({ performer_ref: enumOf("role:a", "role:b", "role:c") });
    const onChange = vi.fn();
    render(<AnswerHarness kind="PickOne" schema={schema} onChange={onChange} />);
    const select = screen.getByRole("combobox", { name: "performer_ref" }) as HTMLSelectElement;
    expect(Array.from(select.options).map((o) => o.value)).toEqual(["", "role:a", "role:b", "role:c"]);
    expect(screen.queryByRole("combobox", { name: "trigger_ref" })).toBeNull();
    await user.selectOptions(select, "role:c");
    expect(lastValue(onChange)).toEqual({ performer_ref: "role:c" });

    // FormulaConfirm uses `expression` because the schema names it, as a multi-line input.
    cleanup();
    render(<AnswerHarness kind="FormulaConfirm" schema={SCHEMAS.FormulaConfirm} onChange={vi.fn()} />);
    expect(screen.getByRole("textbox", { name: "expression" }).tagName).toBe("TEXTAREA");

    // Completeness follows the schema's required set and constraints.
    expect(isAnswerComplete(SCHEMAS.Permission, { operation_ref: "op:approve" })).toBe(false);
    expect(isAnswerComplete(SCHEMAS.Permission, { operation_ref: "op:approve", resource_scope_ref: "scope:x" })).toBe(false);
    expect(isAnswerComplete(SCHEMAS.Permission, { operation_ref: "op:approve", resource_scope_ref: "scope:all", extra: 1 })).toBe(false);
    expect(isAnswerComplete(SCHEMAS.Text, { text: "" })).toBe(false);
    expect(isAnswerComplete(SCHEMAS.Precision, { scale: 1.5 })).toBe(false);
    expect(isAnswerComplete(SCHEMAS.Number, { value: 400 })).toBe(false);
  });

  it("shows an unsupported state and produces no answer for null or unsafe schemas", () => {
    const unsafe: unknown[] = [
      null,
      { type: "object", properties: { a: { type: "string" } }, required: ["a"] },
      { type: "object", additionalProperties: true, properties: { a: { type: "string" } }, required: ["a"] },
      { type: "object", additionalProperties: false, properties: { a: { type: "string" }, b: { type: "string" } }, required: ["a"] },
      closed({ nested: { type: "object" } }),
      closed({ choice: { type: "string", enum: [1, 2] } }),
      closed({ choice: { type: "string", enum: [] } }),
      closed({ text: { type: "string", pattern: "^x" } }),
      closed({}),
      [],
      "schema",
    ];
    for (const schema of unsafe) {
      expect(parseAnswerSchema(schema)).toBeNull();
      const onChange = vi.fn();
      const { container, unmount } = render(<AnswerHarness kind="Text" schema={schema} onChange={onChange} />);
      const control = container.querySelector("[data-renderer]") as HTMLElement;
      expect(control.getAttribute("data-answer-state")).toBe("unsupported");
      expect(control.getAttribute("role")).toBe("alert");
      // No free-text (or any) fallback input exists.
      expect(container.querySelector("input, textarea, select")).toBeNull();
      expect(onChange).not.toHaveBeenCalled();
      expect(isAnswerComplete(schema, {})).toBe(false);
      unmount();
    }
  });
});

// ------------------------------------------------------------------ DiagramFrame

const NODES: DiagramNode[] = [
  { id: "step:b", label: "Approve" },
  { id: "step:a", label: "Submit" },
  { id: "step:c", label: "Notify" },
];
const EDGES: DiagramEdge[] = [
  { id: "flow:2", source: "step:b", target: "step:c", label: "approved" },
  { id: "flow:1", source: "step:a", target: "step:b" },
];

describe("shared DiagramFrame", () => {
  it("lays out deterministically from canonical input", async () => {
    const reversedNodes = [...NODES].reverse();
    const reversedEdges = [...EDGES].reverse();
    expect(layoutRequest(reversedNodes, reversedEdges)).toEqual(layoutRequest(NODES, EDGES));
    expect(layoutRequest(NODES, EDGES).children?.map((c) => c.id)).toEqual(["step:a", "step:b", "step:c"]);
    const first = await computeLayout(NODES, EDGES);
    const second = await computeLayout(reversedNodes, reversedEdges);
    expect(second).toEqual(first);
  });

  it("keeps zoom, pan, fit, toggle and export separate from the semantic callback", async () => {
    const user = userEvent.setup();
    const onSemanticAction = vi.fn();
    const onExportSvg = vi.fn();
    const { container } = render(
      <DiagramFrame title="Leave process" nodes={NODES} edges={EDGES} onSemanticAction={onSemanticAction} onExportSvg={onExportSvg} />,
    );
    const svg = await screen.findByTestId("diagram-svg");
    await waitFor(() => expect(svg.querySelectorAll("g[data-node-id]")).toHaveLength(3));
    expect(Array.from(svg.querySelectorAll("g[data-node-id]")).map((g) => g.getAttribute("data-node-id"))).toEqual([
      "step:a",
      "step:b",
      "step:c",
    ]);
    expect(svg.textContent).toContain("Approve");
    expect(svg.querySelectorAll("polyline[data-edge-id]")).toHaveLength(2);
    const frame = container.querySelector("section[data-diagram-mode]") as HTMLElement;

    for (const name of ["Zoom in", "Zoom out", "Zoom in", "Pan left", "Pan right", "Pan up", "Pan down", "Pan right"]) {
      await user.click(screen.getByRole("button", { name }));
    }
    expect(frame.getAttribute("data-zoom")).toBe("1.25");
    expect(frame.getAttribute("data-pan")).toBe("40,0");
    await user.click(screen.getByRole("button", { name: "Fit" }));
    expect(frame.getAttribute("data-zoom")).toBe("1");
    expect(frame.getAttribute("data-pan")).toBe("0,0");

    await user.click(screen.getByRole("button", { name: "Export SVG" }));
    expect(onExportSvg).toHaveBeenCalledTimes(1);
    const exported = String(onExportSvg.mock.calls[0]?.[0]);
    expect(exported.startsWith("<svg")).toBe(true);
    for (const label of ["Submit", "Approve", "Notify"]) {
      expect(exported).toContain(label);
    }

    // The list view exposes the same semantic content.
    await user.click(screen.getByRole("button", { name: "Show list" }));
    const list = screen.getByTestId("diagram-list");
    expect(Array.from(list.querySelectorAll("li[data-node-id]")).map((li) => li.getAttribute("data-node-id"))).toEqual([
      "step:a",
      "step:b",
      "step:c",
    ]);
    expect(Array.from(list.querySelectorAll("li[data-edge-id]")).map((li) => li.getAttribute("data-edge-id"))).toEqual(["flow:1", "flow:2"]);
    expect(list.textContent).toContain("approved");
    await user.click(screen.getByRole("button", { name: "Show diagram" }));
    await screen.findByTestId("diagram-svg");

    // No view operation reached the semantic callback.
    expect(onSemanticAction).not.toHaveBeenCalled();

    // Only an explicit semantic action does.
    await user.click(screen.getByRole("button", { name: "Open Approve" }));
    expect(onSemanticAction).toHaveBeenCalledTimes(1);
    expect(onSemanticAction).toHaveBeenCalledWith("step:b");
  });

  it("offers no semantic action when no callback is supplied", async () => {
    render(<DiagramFrame title="Leave process" nodes={NODES} edges={EDGES} />);
    await screen.findByTestId("diagram-svg");
    expect(document.querySelector("[data-semantic-action]")).toBeNull();
    expect(screen.queryByRole("button", { name: "Export SVG" })).toBeNull();
  });
});

// ------------------------------------------------------------------ ConflictDialog

function renderConflict() {
  const queryClient = new QueryClient();
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const mutations: unknown[] = [];
  queryClient.getMutationCache().subscribe((event) => {
    if (event.type === "added") {
      mutations.push(event.mutation);
    }
  });
  render(
    <QueryClientProvider client={queryClient}>
      <ConflictDialog />
    </QueryClientProvider>,
  );
  return { invalidate, mutations };
}

describe("shared ConflictDialog", () => {
  it("offers only Refresh latest and Dismiss and never replays a write", async () => {
    const user = userEvent.setup();
    const { invalidate, mutations } = renderConflict();
    expect(screen.queryByRole("dialog")).toBeNull();
    act(() => {
      useConflictStore.getState().setConflict({ revision: "rev:8:bbbbbbbbbbbbbbbb", semanticHash: "psg:sha256:newer" });
    });
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getAllByRole("button").map((b) => b.textContent)).toEqual(["Dismiss", "Refresh latest"]);
    expect(within(dialog).getByTestId("conflict-revision").textContent).toBe("rev:8:bbbbbbbbbbbbbbbb");
    expect(within(dialog).getByTestId("conflict-semantic-hash").textContent).toBe("psg:sha256:newer");
    expect(within(dialog).getByTestId("conflict-status").textContent).toContain("412");
    // Focus starts on the explicit refresh action; a click outside does not close the dialog.
    await waitFor(() => expect(document.activeElement?.textContent).toBe("Refresh latest"));
    fireEvent.pointerDown(document.body);
    fireEvent.click(document.body);
    expect(screen.queryByRole("dialog")).not.toBeNull();

    await user.click(within(dialog).getByRole("button", { name: "Refresh latest" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(useConflictStore.getState().conflict).toBeNull();

    act(() => {
      useConflictStore.getState().setConflict({ revision: null, semanticHash: null });
    });
    const second = await screen.findByRole("dialog");
    // Unreported values are stated, never manufactured.
    expect(within(second).getByTestId("conflict-unreported")).toBeTruthy();
    expect(within(second).queryByTestId("conflict-revision")).toBeNull();
    await user.click(within(second).getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(invalidate).toHaveBeenCalledTimes(1);

    act(() => {
      useConflictStore.getState().setConflict({ revision: "rev:9:cccccccccccccccc", semanticHash: "psg:sha256:latest" });
    });
    await screen.findByRole("dialog");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    // No mutation was ever created or re-run by the dialog.
    expect(mutations).toEqual([]);
  });
});
