// The Inbox item model (handoff "2. Inbox", spec §5.2): which pending decisions the queue lists,
// how each is typed and attributed, the smart-filter counts, ages and the evidence offsets.
import { describe, expect, it } from "vitest";

import type { components } from "../../api/schema";
import { issuesToDecide, questionsAwaitingAnswer } from "../../app/work";
import { buildInboxItems, filterCounts, formatAge, matchesFilter, parseInboxSearch, utf8RangeToUtf16, type InboxItem } from "./model";

type PendingProposal = components["schemas"]["PendingProposal"];
type Question = components["schemas"]["Question"];
type Finding = components["schemas"]["Finding"];
type GateStatus = components["schemas"]["GateStatus"];

function proposal(id: string, overrides: Partial<PendingProposal> = {}): PendingProposal {
  return {
    proposal_id: id,
    action: "create",
    origin: "stage",
    base_revision: "rev:42:aaaa",
    discardable: false,
    requires_reason: false,
    stale: false,
    title: "Add operation " + id,
    element_kind: "operation",
    created_at: "2026-10-25T11:56:00Z",
    stage: "flows",
    ...overrides,
  };
}

function question(id: string, overrides: Partial<Question> = {}): Question {
  return { id, text: "Question " + id, kind: "YesNo", status: "Open", priority: null, round_ref: null, stakeholder_ref: null, context_refs: [], answer_schema: null, ...overrides };
}

function finding(id: string, overrides: Partial<Finding> = {}): Finding {
  return { id, code: "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED", family: "F1", message: "Duplicate " + id, severity: "blocker", status: "Open", affected_refs: ["req:a", "req:b"], rule_class: "plumb_core", waiver_ref: null, ...overrides };
}

function gate(name: GateStatus["gate"], overrides: Partial<GateStatus> = {}): GateStatus {
  return { gate: name, state: "current", passed: true, reason: null, findings: [], rules: [], waivers: [], summary: { blocker_failed: 0, blocker_waived: 0, warnings: 0 }, ...overrides };
}

const rule = (overrides: Partial<GateStatus["rules"][number]>): GateStatus["rules"][number] => ({
  rule_id: "PLUMB.F2.EVENT.CONSUMER",
  state: "FAIL",
  declared_severity: "blocker",
  effective_severity: "blocker",
  applicability: { state: "APPLICABLE", reason: null },
  targets: ["event:a"],
  evidence: [],
  semantic_condition_key: null,
  finding_ref: null,
  waiver_ref: null,
  error: null,
  ...overrides,
});

describe("questions awaiting an answer", () => {
  it("leaves out an open question whose answer already waits for confirmation", () => {
    const questions = [question("q:1"), question("q:2")];
    const proposals = [proposal("prop:a", { origin: "question", origin_ref: "q:1", element_kind: "answer", stage: null })];
    expect(questionsAwaitingAnswer(questions, proposals).map((q) => q.id)).toEqual(["q:2"]);
  });
});

describe("issues to decide", () => {
  it("lists open unwaived blockers and unmaterialized failing blockers of current gates, never what a gate already waived", () => {
    const findings = [
      finding("fnd:open"),
      finding("fnd:gate-waived"),
      finding("fnd:warning", { severity: "error" }),
      finding("fnd:closed", { status: "Closed" }),
      finding("fnd:waived", { waiver_ref: "dec:1" }),
    ];
    const gates = [
      gate("F1", { waivers: [{ decision_ref: "dec:2", finding_key: "k", finding_ref: "fnd:gate-waived", rule_id: "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED" }] }),
      gate("F2", {
        passed: false,
        rules: [
          rule({}),
          rule({ rule_id: "PLUMB.F2.MATERIALIZED", finding_ref: "fnd:open" }),
          rule({ rule_id: "PLUMB.F2.WARN", effective_severity: "warn" }),
          rule({ rule_id: "PLUMB.F2.WAIVED", state: "WAIVED" }),
        ],
      }),
      gate("F3", { state: "stale", rules: [rule({ rule_id: "PLUMB.F3.OLD" })] }),
    ];
    const issues = issuesToDecide(findings, gates);
    expect(issues.map((i) => i.id)).toEqual(["fnd:open", "F2:PLUMB.F2.EVENT.CONSUMER"]);
    expect(issues[0]).toMatchObject({ findingId: "fnd:open", code: "PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED", gate: "F1", affected: ["req:a", "req:b"] });
    expect(issues[1]).toMatchObject({ findingId: null, code: "PLUMB.F2.EVENT.CONSUMER", gate: "F2", affected: ["event:a"], message: null });
  });
});

describe("buildInboxItems", () => {
  const items = buildInboxItems({
    proposals: [
      proposal("prop:old", { created_at: "2026-10-25T11:00:00Z", element_kind: "link", action: "link", title: "Link Cancel leave request to HR-019", stale: true }),
      proposal("prop:new", { created_at: "2026-10-25T11:56:00Z" }),
      proposal("prop:event", { created_at: "2026-10-25T11:56:00Z", element_kind: "event", title: "Add event Leave request submitted" }),
      proposal("prop:ears", { element_kind: "requirement", stage: "consolidate", action: "accept", title: "Change requirement HR-030" }),
      proposal("prop:answer", { origin: "question", origin_ref: "q:answered", element_kind: "answer", stage: null, discardable: true, title: "Apply the answer to: Q" }),
      proposal("prop:session", { origin: "session", stage: null, element_kind: "operation", title: "Add operation View leave balance" }),
      proposal("prop:intake", { origin: "intake", stage: null, element_kind: "merge", action: "merge", title: "Merge 'a' into 'b'" }),
    ],
    questions: [question("q:low", { priority: 5 }), question("q:answered"), question("q:high", { priority: 1 })],
    findings: [finding("fnd:dup")],
    gates: [],
  });

  it("orders proposals newest first (then by title), then questions by priority, wording, answers to confirm and issues", () => {
    expect(items.map((i) => i.id)).toEqual(["prop:event", "prop:new", "prop:session", "prop:intake", "prop:old", "q:high", "q:low", "prop:ears", "prop:answer", "fnd:dup"]);
  });

  it("types and attributes every item", () => {
    const byId = new Map(items.map((i) => [i.id, i] as const));
    const pick = (id: string): Pick<InboxItem, "type" | "origin" | "originLabel" | "typeLabel" | "acceptable"> => {
      const item = byId.get(id) as InboxItem;
      return { type: item.type, origin: item.origin, originLabel: item.originLabel, typeLabel: item.typeLabel, acceptable: item.acceptable };
    };
    expect(pick("prop:new")).toEqual({ type: "create", origin: "steps", originLabel: "Flows step", typeLabel: "New operation", acceptable: true });
    expect(pick("prop:event")).toMatchObject({ type: "create", typeLabel: "New event" });
    expect(pick("prop:old")).toEqual({ type: "link", origin: "steps", originLabel: "Flows step", typeLabel: "Link", acceptable: false });
    expect(pick("prop:ears")).toEqual({ type: "wording", origin: "steps", originLabel: "Model step", typeLabel: "Wording", acceptable: true });
    expect(pick("prop:answer")).toEqual({ type: "answer", origin: "questions", originLabel: "From a question", typeLabel: "Answer to confirm", acceptable: true });
    expect(pick("prop:session")).toEqual({ type: "create", origin: "assistant", originLabel: "From the Assistant", typeLabel: "New operation", acceptable: true });
    expect(pick("prop:intake")).toEqual({ type: "merge", origin: "steps", originLabel: "From an import", typeLabel: "Merge", acceptable: true });
    expect(pick("q:high")).toEqual({ type: "question", origin: "steps", originLabel: "Questions step", typeLabel: "Question", acceptable: false });
    expect(pick("fnd:dup")).toEqual({ type: "issue", origin: "steps", originLabel: "Quality check", typeLabel: "Issue to decide", acceptable: false });
    expect(byId.get("q:high")?.title).toBe("Question q:high");
    expect(byId.get("fnd:dup")?.title).toBe("Duplicate fnd:dup");
  });

  it("counts the smart filters and the origins", () => {
    expect(filterCounts(items)).toEqual({ all: 10, proposals: 6, questions: 2, wording: 1, issues: 1, steps: 8, fromq: 1, assistant: 1 });
    expect(items.filter((i) => matchesFilter(i, "proposals")).map((i) => i.type)).toEqual(["create", "create", "create", "merge", "link", "answer"]);
    expect(items.filter((i) => matchesFilter(i, "assistant")).map((i) => i.id)).toEqual(["prop:session"]);
    expect(items.filter((i) => matchesFilter(i, "fromq")).map((i) => i.id)).toEqual(["prop:answer"]);
  });
});

describe("helpers", () => {
  it("parses the filter search parameter, keeping the old view names", () => {
    expect(parseInboxSearch({ filter: "questions" })).toEqual({ filter: "questions" });
    expect(parseInboxSearch({ filter: "fromq" })).toEqual({ filter: "fromq" });
    expect(parseInboxSearch({ filter: "nonsense" })).toEqual({});
    expect(parseInboxSearch({})).toEqual({});
  });

  it("formats ages like the queue (now, minutes, hours, days)", () => {
    const now = new Date("2026-10-25T12:00:00Z");
    expect(formatAge("2026-10-25T11:59:40Z", now)).toBe("now");
    expect(formatAge("2026-10-25T11:56:00Z", now)).toBe("4 min");
    expect(formatAge("2026-10-25T11:00:00Z", now)).toBe("1 h");
    expect(formatAge("2026-10-23T09:00:00Z", now)).toBe("2 d");
    expect(formatAge(null, now)).toBe("");
    expect(formatAge("not a date", now)).toBe("");
  });

  it("converts UTF-8 byte offsets to string indices", () => {
    const text = "Zażółć — leave request";
    // "Zażółć" is 10 bytes (a, ż, ó, ł, ć are 1+2+2+2+2), " — " is 5 bytes.
    expect(utf8RangeToUtf16(text, 0, 10)).toEqual({ start: 0, end: 6 });
    expect(utf8RangeToUtf16(text, 15, 20)).toEqual({ start: 9, end: 14 });
    expect(utf8RangeToUtf16("🙂 ok", 5, 7)).toEqual({ start: 3, end: 5 });
    expect(utf8RangeToUtf16(text, null, null)).toBeUndefined();
  });
});
