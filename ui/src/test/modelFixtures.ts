// HR leave pilot fixtures for the Model and Scenarios screen tests, shaped exactly like the real
// API (elements referenced by ID, names from GET /api/model/labels).
import { http } from "msw";

import type { components } from "../api/schema";
import { etagJson, TEST_ETAG } from "./msw";

type S = components["schemas"];

export const WRITE_OK: S["WriteResult"] = { version: "rev:2:0000000000000000", revision: "rev:2:0000000000000000", semantic_hash: "psg:sha256:2", events: [] };

/** The element names the labels endpoint knows (lower-case states as in the pilot). */
export const LABELS: Readonly<Record<string, { kind: string; label: string }>> = {
  "state:draft": { kind: "state", label: "draft" },
  "state:submitted": { kind: "state", label: "submitted" },
  "state:approved": { kind: "state", label: "approved" },
  "state:rejected": { kind: "state", label: "rejected" },
  "state:withdrawn": { kind: "state", label: "withdrawn" },
  "state:cancelled": { kind: "state", label: "cancelled" },
  "op:submit": { kind: "operation", label: "Submit leave request" },
  "op:approve": { kind: "operation", label: "Approve leave request" },
  "op:reject": { kind: "operation", label: "Reject leave request" },
  "op:withdraw": { kind: "operation", label: "Withdraw leave request" },
  "op:cancel": { kind: "operation", label: "Cancel leave request" },
  "op:create": { kind: "operation", label: "Create leave request" },
  "op:deduct": { kind: "operation", label: "Deduct balance" },
  "entity:request": { kind: "entity", label: "leave request" },
  "entity:employee": { kind: "entity", label: "employee" },
  "actor:employee": { kind: "actor", label: "Employee" },
  "actor:manager": { kind: "actor", label: "Manager" },
  "actor:system": { kind: "actor", label: "System" },
  "event:submitted": { kind: "event", label: "Leave request submitted" },
  "tr:submit": { kind: "transition", label: "draft → submitted" },
  "tr:approve": { kind: "transition", label: "submitted → approved" },
  "dt:eligibility": { kind: "decision_table", label: "Annual leave eligibility" },
  "rule:authorization": { kind: "rule", label: "Manager decision authorization" },
  "req:hr-013": { kind: "requirement", label: "HR-013" },
  "term:request": { kind: "term", label: "leave request" },
  "term:holiday": { kind: "term", label: "public holiday" },
};

/** GET /api/model/labels over LABELS (unknown IDs omitted, sorted by ID). */
export const labelsHandler = http.get("*/api/model/labels", ({ request }) => {
  const ids = (new URL(request.url).searchParams.get("ids") ?? "").split(",").filter((id) => id !== "");
  const items = [...new Set(ids)]
    .sort()
    .filter((id) => LABELS[id] !== undefined)
    .map((id) => ({ id, kind: LABELS[id]?.kind ?? "", label: LABELS[id]?.label ?? "" }));
  return etagJson({ items });
});

export const GLOSSARY: S["GlossaryTerm"][] = [
  { id: "term:request", term: "leave request", language: "en", aliases: ["request"], definition_ref: "req:hr-013", status: "Accepted" },
  { id: "term:balance", term: "annual leave balance", language: "en", aliases: ["balance", "remaining leave"], definition_ref: null, status: "Accepted" },
  { id: "term:holiday", term: "public holiday", language: "en", aliases: ["holiday"], definition_ref: null, status: "Proposed" },
];

export const ENTITIES: S["Entity"][] = [
  {
    id: "entity:request",
    name: "leave request",
    attributes: [
      { id: "attr:start", name: "start date", type: "Date", nullable: false, unit: null, class: null },
      { id: "attr:type", name: "leave type", type: "String", nullable: false, unit: null, class: "pii" },
      { id: "attr:days", name: "requested working days", type: "Decimal(2)", nullable: false, unit: "days", class: null },
      { id: "attr:reason", name: "reason", type: "String", nullable: true, unit: null, class: null },
    ],
    relationships: [{ id: "rel:requested-by", target: "entity:employee", kind: "association", cardinality: "*:1" }],
    states: ["state:draft", "state:submitted", "state:approved", "state:rejected", "state:withdrawn", "state:cancelled"],
    transitions: [
      { from: "state:draft", to: "state:submitted", operation: "op:submit" },
      { from: "state:submitted", to: "state:approved", operation: "op:approve" },
      { from: "state:submitted", to: "state:rejected", operation: "op:reject" },
      { from: "state:submitted", to: "state:withdrawn", operation: "op:withdraw" },
      { from: "state:approved", to: "state:cancelled", operation: "op:cancel" },
    ],
    tags: [],
  },
  { id: "entity:employee", name: "employee", attributes: [], relationships: [], states: [], transitions: [], tags: [] },
];

export const CRUD: S["CrudMatrix"] = {
  operations: ["op:approve", "op:submit"],
  entities: ["entity:employee", "entity:request"],
  cells: [
    { op: "op:approve", ent: "entity:request", c: false, r: true, u: true, d: false },
    { op: "op:approve", ent: "entity:employee", c: false, r: true, u: false, d: false },
    { op: "op:submit", ent: "entity:request", c: true, r: false, u: null, d: null },
    { op: "op:submit", ent: "entity:employee", c: false, r: false, u: false, d: false },
  ],
};

const col = (name: string, ty: unknown) => JSON.stringify({ domain: null, name, ty });

export const RULES: S["Rule"][] = [
  {
    id: "dt:eligibility",
    name: "AnnualLeaveEligibility",
    kind: "decision_table",
    conditions: [col("employment_type", { members: ["Contractor", "Employee"], type: "enum" }), col("leave_type", { members: ["Annual", "Sick"], type: "enum" })],
    outputs: [col("eligible", { type: "bool" })],
    rows: [
      { when: ['{"cell":"enum_set","members":["Employee"]}', '{"cell":"any"}'], then: ['{"cell":"bool","value":true}'] },
      { when: ['{"cell":"enum_set","members":["Contractor"]}', '{"cell":"enum_set","members":["Annual"]}'], then: ['{"cell":"bool","value":false}'] },
      { when: ['{"cell":"enum_set","members":["Contractor"]}', '{"cell":"enum_set","members":["Annual","Sick"]}'], then: ['{"cell":"bool","value":false}'] },
    ],
    uncovered_cells: ['[{"kind":"enum","member":"Contractor"},{"kind":"enum","member":"Sick"}]'],
    overlaps: ["1:2"],
  },
  { id: "rule:authorization", name: "ManagerDecisionAuthorization", kind: "rule", conditions: ["actor == direct_manager(employee)"], outputs: null, rows: null, uncovered_cells: null, overlaps: null },
  {
    id: "dt:routing",
    name: "ApprovalRouting",
    kind: "decision_table",
    conditions: [col("days", { type: "int" })],
    outputs: [col("route", { members: ["Director", "Manager"], type: "enum" })],
    rows: [{ when: ['{"cell":"interval","interval":{"lower":null,"upper":{"inclusive":true,"value":"5"}}}'], then: ['{"cell":"enum","member":"Manager"}'] }],
    uncovered_cells: [],
    overlaps: [],
  },
];

export const CALCULATIONS: S["Calculation"][] = [
  { id: "calc:days", name: "requested leave duration", expr: "working_days([start_date, end_date], calendar, true) * day_fraction", status: "Proposed" },
];

/** The worked example: the week 4–10 Jan 2027 (5 working days) and one day each. */
export function calculationExampleHandler(holiday: string | null = null) {
  return http.get("*/api/model/calculations/:id/example", ({ request }) => {
    const inputs = JSON.parse(new URL(request.url).searchParams.get("inputs") ?? "{}") as Record<string, { value: string }>;
    const start = inputs["start_date"]?.value ?? "";
    const end = inputs["end_date"]?.value ?? "";
    const days: string[] = [];
    for (let d = new Date(start + "T00:00:00Z"); d <= new Date(end + "T00:00:00Z"); d.setUTCDate(d.getUTCDate() + 1)) {
      days.push(d.toISOString().slice(0, 10));
    }
    const working = days.filter((day) => {
      const weekday = new Date(day + "T00:00:00Z").getUTCDay();
      return weekday !== 0 && weekday !== 6 && day !== holiday;
    }).length;
    return etagJson({
      expr: CALCULATIONS[0]?.expr ?? "",
      rendered_example: `Given calendar = calendar:office, day_fraction = 1, end_date = ${end}, start_date = ${start}; evaluates to ${working} working_day.`,
      value: { type: "quantity", value: String(working), unit: "working_day" },
    });
  });
}

export const PROCESS: S["Process"] = {
  id: "proc:approval",
  name: "Leave request approval",
  lanes: ["actor:employee", "actor:manager", "actor:system"],
  steps: ["pn:approve", "pn:end", "pn:start", "pn:submit", "pn:deduct"],
  branches: [],
  validation: [],
  nodes: [
    { id: "pn:approve", kind: "human_task", operation_ref: "op:approve", performer_refs: ["actor:manager"], consumes_refs: [], produces_refs: [], condition_expr: null, message_ref: null, timer_expr: null },
    { id: "pn:deduct", kind: "service_task", operation_ref: "op:deduct", performer_refs: ["actor:system"], consumes_refs: [], produces_refs: [], condition_expr: null, message_ref: null, timer_expr: null },
    { id: "pn:end", kind: "end", operation_ref: null, performer_refs: [], consumes_refs: [], produces_refs: [], condition_expr: null, message_ref: null, timer_expr: null },
    { id: "pn:start", kind: "start", operation_ref: null, performer_refs: [], consumes_refs: [], produces_refs: [], condition_expr: null, message_ref: null, timer_expr: null },
    { id: "pn:submit", kind: "human_task", operation_ref: "op:submit", performer_refs: ["actor:employee"], consumes_refs: [], produces_refs: ["event:submitted"], condition_expr: null, message_ref: null, timer_expr: null },
  ],
  flows: [
    { from: "pn:approve", to: "pn:deduct" },
    { from: "pn:deduct", to: "pn:end" },
    { from: "pn:start", to: "pn:submit" },
    { from: "pn:submit", to: "pn:approve" },
  ],
};

export const DIFF: S["ModelDiff"] = {
  from: "rev:39:0000000000000000",
  to: "rev:42:0000000000000000",
  added: ["op:submit", "op:approve", "event:submitted"],
  modified: ["entity:request"],
  removed: [],
};

/** The reads every Model segment shares (header pills) plus the labels endpoint. */
export const modelBase = [
  labelsHandler,
  http.get("*/api/model/crud", () => etagJson(CRUD)),
  http.get("*/api/model/diff", () => etagJson(DIFF)),
];

export { TEST_ETAG };
