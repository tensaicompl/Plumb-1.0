// Plain-language readings of Scenario material (canonical JSON facts, plumb-sim trace steps,
// missing semantics and diffs). Element IDs become names through `label`; anything not
// recognised stays the exact server text. No I/O.
import type { components } from "../../api/schema";
import { t, tf, type MessageKey } from "../../i18n";
import { parseColumn } from "../model/ruleCells";

type Scenario = components["schemas"]["Scenario"];
type Rule = components["schemas"]["Rule"];

export type Label = (id: string) => string;
export type Kind = (id: string) => string | null;

function parse(text: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return undefined;
  }
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function strings(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

/** The single `{key: value}` of a canonical fact, or null. */
function single(text: string): [string, unknown] | null {
  const value = record(parse(text));
  const entries = value === null ? [] : Object.entries(value);
  return entries.length === 1 && entries[0] !== undefined ? entries[0] : null;
}

/** The DecisionTable a scenario evaluates (`when: {"decision_table": id}`), or null. */
export function tableOf(scenario: Scenario): string | null {
  const when = single(scenario.when);
  return when !== null && when[0] === "decision_table" && typeof when[1] === "string" ? when[1] : null;
}

/** Every element ID the scenario's facts reference (for one labels request). */
export function referencedIds(scenario: Scenario): string[] {
  const ids: string[] = [...scenario.derived_from];
  const collect = (value: unknown) => {
    if (typeof value === "string" && /^[a-z_]+:[^\s]+$/.test(value)) {
      ids.push(value);
    } else if (Array.isArray(value)) {
      value.forEach(collect);
    } else if (record(value) !== null) {
      Object.values(value as Record<string, unknown>).forEach(collect);
    }
  };
  [...scenario.given, scenario.when, ...scenario.then].forEach((text) => collect(parse(text)));
  return ids;
}

function decisionValue(value: unknown): string {
  const v = record(value);
  if (v?.["kind"] === "bool") {
    return t(v["value"] === true ? "model.rules.yes" : "model.rules.no");
  }
  return v !== null && (typeof v["value"] === "string" || typeof v["value"] === "number") ? String(v["value"]) : JSON.stringify(value);
}

function decisionValues(value: unknown): string[] {
  const v = record(value);
  return v === null
    ? []
    : Object.entries(v)
        .sort(([a], [b]) => Number(a) - Number(b))
        .map(([, cell]) => decisionValue(cell));
}

function decisionFacts(value: unknown, columns: readonly string[]): string[] {
  const v = record(value);
  return v === null
    ? []
    : Object.entries(v)
        .sort(([a], [b]) => Number(a) - Number(b))
        .map(([index, cell]) => tf("scenarios.fact.is", { name: columns[Number(index)] ?? tf("scenarios.fact.column", { n: Number(index) + 1 }), value: decisionValue(cell) }));
}

/** The input and output column names of the scenario's table (from the rule view). */
export function tableColumns(rule: Rule | undefined): { inputs: string[]; outputs: string[] } {
  return { inputs: (rule?.conditions ?? []).map((c) => parseColumn(c).name), outputs: (rule?.outputs ?? []).map((c) => parseColumn(c).name) };
}

function statesFact(value: unknown, label: Label): string {
  const states = strings(value).map(label);
  return states.length === 1 ? tf("scenarios.fact.state", { state: states[0] ?? "" }) : tf("scenarios.fact.states", { states: states.join(", ") });
}

/** One given or then fact in words (the exact text when it is not a known shape). */
export function describeFact(text: string, label: Label, columns: readonly string[]): string {
  const fact = single(text);
  if (fact === null) {
    const value = record(parse(text));
    if (value !== null && typeof value["attribute"] === "string" && "value" in value) {
      return tf("scenarios.fact.is", { name: label(value["attribute"]), value: typeof value["value"] === "string" ? value["value"] : JSON.stringify(value["value"]) });
    }
    return text;
  }
  const [key, value] = fact;
  if (key === "states_active") {
    return statesFact(value, label);
  }
  if (key === "state" && typeof value === "string") {
    return tf("scenarios.fact.state", { state: label(value) });
  }
  if (key === "decision_inputs" || key === "decision_outputs") {
    return decisionFacts(value, columns).join("; ");
  }
  if (key === "event" && typeof value === "string") {
    return tf("scenarios.fact.event", { event: label(value) });
  }
  return text;
}

/** The When in words. */
export function describeWhen(text: string, label: Label): string {
  const when = single(text);
  if (when === null || typeof when[1] !== "string") {
    return text;
  }
  switch (when[0]) {
    case "operation":
      return tf("scenarios.when.operation", { name: label(when[1]) });
    case "decision_table":
      return tf("scenarios.when.table", { name: label(when[1]) });
    case "transition":
      return tf("scenarios.when.transition", { name: label(when[1]) });
    case "calculation":
      return tf("scenarios.when.calculation", { name: label(when[1]) });
    case "event":
      return tf("scenarios.when.event", { name: label(when[1]) });
    default:
      return text;
  }
}

function firstStates(facts: readonly string[]): string[] {
  for (const text of facts) {
    const fact = single(text);
    if (fact !== null && fact[0] === "states_active") {
      return strings(fact[1]);
    }
  }
  return [];
}

function firstDecision(facts: readonly string[], key: string): string[] {
  for (const text of facts) {
    const fact = single(text);
    if (fact !== null && fact[0] === key) {
      return decisionValues(fact[1]);
    }
  }
  return [];
}

/** A readable title: "Submit leave request: Draft → Submitted", "Annual leave eligibility: Contractor → No". */
export function scenarioTitle(scenario: Scenario, label: Label): string {
  const when = single(scenario.when);
  const subject = when !== null && typeof when[1] === "string" ? label(when[1]) : null;
  const from = firstStates(scenario.given);
  const to = firstStates(scenario.then);
  if (subject !== null && from.length > 0 && to.length > 0) {
    return tf("scenarios.title.transition", { subject, from: from.map(label).join(", "), to: to.map(label).join(", ") });
  }
  const inputs = firstDecision(scenario.given, "decision_inputs");
  const outputs = firstDecision(scenario.then, "decision_outputs");
  if (subject !== null && inputs.length > 0 && outputs.length > 0) {
    return tf("scenarios.title.transition", { subject, from: inputs.join(", "), to: outputs.join(", ") });
  }
  return subject ?? tf("scenarios.title.fallback", { id: scenario.id });
}

/** Where the scenario comes from: "Transition Draft → Submitted", "Decision table …". */
export function scenarioSource(scenario: Scenario, label: Label, kind: Kind): string | null {
  const ref = scenario.derived_from[0];
  if (ref === undefined) {
    return null;
  }
  const from = firstStates(scenario.given);
  const to = firstStates(scenario.then);
  switch (kind(ref)) {
    case "transition":
      return from.length > 0 && to.length > 0
        ? tf("scenarios.source.transition", { from: from.map(label).join(", "), to: to.map(label).join(", ") })
        : tf("scenarios.source.transitionNamed", { name: label(ref) });
    case "decision_table":
      return tf("scenarios.source.table", { name: label(ref) });
    case "requirement":
      return tf("scenarios.source.requirement", { name: label(ref) });
    case "rule":
      return tf("scenarios.source.rule", { name: label(ref) });
    case "calculation":
      return tf("scenarios.source.calculation", { name: label(ref) });
    default:
      return label(ref);
  }
}

/** The Model or Requirements page of a referenced element, when it has one. */
export function elementHref(id: string, kind: string | null): string | null {
  const segment: Record<string, string> = { decision_table: "/model/rules/", rule: "/model/rules/", calculation: "/model/calculations/", entity: "/model/entities/", term: "/model/glossary/", process: "/model/processes/", requirement: "/requirements/" };
  if (kind === "transition" || kind === "state") {
    return "/model/entities";
  }
  const base = kind === null ? undefined : segment[kind];
  return base === undefined ? null : base + encodeURIComponent(id).replace(/%3A/gi, ":");
}

// ------------------------------------------------------------------ trace

export type StepStatus = "pass" | "fail" | "missing" | "unknown";

export interface TraceStep {
  readonly title: string;
  readonly detail: string | null;
  readonly status: StepStatus;
}

/** A missing semantic value of an undecidable run. */
export interface Missing {
  readonly owner: string;
  readonly key: string;
  readonly kind: string;
}

export function parseMissing(text: string): Missing | null {
  const value = record(parse(text));
  return value !== null && typeof value["owner_ref"] === "string" && typeof value["key"] === "string" && typeof value["kind"] === "string"
    ? { owner: value["owner_ref"], key: value["key"], kind: value["kind"] }
    : null;
}

/** One expected-versus-actual difference of a failed run. */
export function parseDiff(text: string, label: Label): { expected: string; actual: string } | null {
  const value = record(parse(text));
  if (value === null || !("expected" in value) || !("actual" in value)) {
    return null;
  }
  const show = (v: unknown) => (typeof v === "string" ? (/^[a-z_]+:/.test(v) ? label(v) : v) : JSON.stringify(v));
  return { expected: show(value["expected"]), actual: show(value["actual"]) };
}

const SKIPPED = new Set(["materialization_started", "execution_completed", "comparison_started"]);

const COMPARED: Readonly<Record<string, "state_ref" | "attribute_ref" | "decision_table_ref" | "rule_ref" | "calculation_ref" | null>> = {
  state_compared: "state_ref",
  attribute_compared: "attribute_ref",
  decision_output_compared: "decision_table_ref",
  rule_compared: "rule_ref",
  calculation_compared: "calculation_ref",
  outcomes_compared: null,
  events_compared: null,
};

/** The trace in words: each recognised step with its detail, unknown steps kept as exact text. */
export function readTrace(trace: readonly string[], label: Label): TraceStep[] {
  const steps: TraceStep[] = [];
  for (const text of trace) {
    const outer = record(parse(text));
    const inner = record(outer?.["step"]);
    const name = inner?.["step"];
    if (inner === null || typeof name !== "string") {
      steps.push({ title: t("scenarios.trace.unknown"), detail: text, status: "unknown" });
      continue;
    }
    if (SKIPPED.has(name)) {
      continue;
    }
    const ref = (key: string) => (typeof inner[key] === "string" ? label(inner[key]) : "");
    const refs = (key: string) => strings(inner[key]).map(label).join(", ");
    switch (name) {
      case "given_materialized": {
        const count = strings(inner["state_refs"]).length + strings(inner["entity_refs"]).length + strings(inner["attribute_refs"]).length;
        steps.push({ title: t("scenarios.trace.given"), detail: count === 0 ? null : tf("scenarios.trace.facts", { count }), status: "pass" });
        break;
      }
      case "footprint_declared":
        steps.push({ title: tf("scenarios.trace.footprint", { name: ref("operation_ref") }), detail: refs("writes") === "" ? null : tf("scenarios.trace.writes", { names: refs("writes") }), status: "pass" });
        break;
      case "performer_recorded":
        steps.push({ title: tf("scenarios.trace.performer", { names: refs("performer_refs") }), detail: ref("operation_ref"), status: "pass" });
        break;
      case "authorization_checked":
        {
          const decision = record(inner["decision"])?.["decision"];
          steps.push({ title: t("scenarios.trace.authorization"), detail: typeof decision === "string" ? decision.replace(/_/g, " ") : JSON.stringify(inner["decision"] ?? null), status: "pass" });
        }
        break;
      case "precondition_evaluated":
      case "postcondition_evaluated":
      case "invariant_evaluated":
      case "guard_evaluated":
        steps.push({ title: t(("scenarios.trace." + name.replace("_evaluated", "")) as MessageKey), detail: "result" in inner ? JSON.stringify(inner["result"]) : null, status: "pass" });
        break;
      case "rule_evaluated":
        steps.push({ title: tf("scenarios.trace.evaluated", { name: ref("rule_ref") }), detail: "result" in inner ? JSON.stringify(inner["result"]) : null, status: "pass" });
        break;
      case "decision_table_evaluated":
        steps.push({ title: tf("scenarios.trace.table", { name: ref("decision_table_ref") }), detail: typeof inner["row"] === "number" ? tf("scenarios.trace.row", { row: inner["row"] + 1 }) : t("scenarios.trace.noRow"), status: "pass" });
        break;
      case "calculation_evaluated":
        steps.push({ title: tf("scenarios.trace.calculation", { name: ref("calculation_ref") }), detail: "value" in inner ? JSON.stringify(inner["value"]) : null, status: "pass" });
        break;
      case "outcome_selected":
        steps.push({ title: tf("scenarios.trace.outcome", { name: ref("outcome_ref") }), detail: ref("operation_ref"), status: "pass" });
        break;
      case "transition_applied":
        steps.push({ title: tf("scenarios.trace.transition", { from: ref("from_state"), to: ref("to_state") }), detail: null, status: "pass" });
        break;
      case "event_emitted":
      case "event_occurred":
        steps.push({ title: tf("scenarios.trace.event", { name: ref("event_ref") }), detail: null, status: "pass" });
        break;
      case "semantic_missing":
        steps.push({ title: t("scenarios.trace.missing"), detail: null, status: "missing" });
        break;
      case "comparison_completed":
        steps.push({
          title: t("scenarios.trace.compared"),
          detail:
            inner["result"] === "undecidable"
              ? t("scenarios.trace.undecided")
              : typeof inner["diff_count"] === "number"
                ? inner["diff_count"] === 0
                  ? t("scenarios.trace.matches")
                  : tf("scenarios.trace.differences", { count: inner["diff_count"] })
                : null,
          status: inner["result"] === "fail" ? "fail" : inner["result"] === "undecidable" ? "missing" : "pass",
        });
        break;
      default: {
        if (name in COMPARED) {
          const key = COMPARED[name];
          const subject = key === null || key === undefined ? name.replace(/_compared$/, "").replace(/_/g, " ") : ref(key);
          const matched = inner["matched"] !== false;
          steps.push({ title: tf("scenarios.trace.expected", { name: subject }), detail: t(matched ? "scenarios.trace.matched" : "scenarios.trace.differs"), status: matched ? "pass" : "fail" });
        } else {
          steps.push({ title: name.replace(/_/g, " "), detail: null, status: "pass" });
        }
      }
    }
  }
  return steps;
}
