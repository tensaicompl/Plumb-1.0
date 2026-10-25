// Plain-language readings of decision-table material (canonical JSON cells and columns). No I/O.
import { t } from "../../i18n";

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

/** "employment_type" → "employment type". */
function words(name: string): string {
  return name.replace(/_/g, " ");
}

export interface Column {
  readonly name: string;
  readonly type: string;
  readonly members: readonly string[];
}

/** A canonical decision column `{name, ty, domain}`. */
export function parseColumn(text: string): Column {
  const value = record(parse(text));
  const ty = record(value?.["ty"]);
  const members = Array.isArray(ty?.["members"]) ? (ty["members"] as unknown[]).filter((m): m is string => typeof m === "string") : [];
  return { name: typeof value?.["name"] === "string" ? words(value["name"]) : text, type: typeof ty?.["type"] === "string" ? ty["type"] : "", members };
}

function yesNo(value: boolean): string {
  return t(value ? "model.rules.yes" : "model.rules.no");
}

function bound(value: unknown, lower: boolean): string | null {
  const b = record(value);
  if (b === null || (typeof b["value"] !== "string" && typeof b["value"] !== "number")) {
    return null;
  }
  const inclusive = b["inclusive"] === true;
  const sign = lower ? (inclusive ? "≥" : ">") : inclusive ? "≤" : "<";
  return sign + " " + String(b["value"]);
}

/** An input cell in plain words ("Any", "Yes", "Annual or Sick", "> 5 and ≤ 10"). */
export function describeInputCell(text: string): string {
  const cell = record(parse(text));
  switch (cell?.["cell"]) {
    case "any":
      return t("model.rules.any");
    case "bool":
      return yesNo(cell["value"] === true);
    case "enum_set":
      return Array.isArray(cell["members"]) ? (cell["members"] as unknown[]).map(String).join(t("model.rules.or")) : text;
    case "interval": {
      const interval = record(cell["interval"]);
      const parts = [bound(interval?.["lower"], true), bound(interval?.["upper"], false)].filter((p): p is string => p !== null);
      return parts.length === 0 ? t("model.rules.any") : parts.join(t("model.rules.and"));
    }
    default:
      return text;
  }
}

/** An output cell in plain words ("Yes", "Manager", "3"). */
export function describeOutputCell(text: string): string {
  const cell = record(parse(text));
  switch (cell?.["cell"]) {
    case "bool":
      return yesNo(cell["value"] === true);
    case "enum":
      return typeof cell["member"] === "string" ? cell["member"] : text;
    case "int":
    case "decimal":
      return String(cell["value"]);
    default:
      return text;
  }
}

/** The values of one uncovered point `[{kind, member|value}]`. */
export function describePoint(text: string): string[] {
  const point = parse(text);
  if (!Array.isArray(point)) {
    return [text];
  }
  return point.map((value) => {
    const v = record(value);
    if (v?.["kind"] === "bool") {
      return yesNo(v["value"] === true);
    }
    return typeof v?.["member"] === "string" ? v["member"] : JSON.stringify(value);
  });
}

/** Pairs "1:2" → overlapping partners per zero-based row. */
export function overlapPartners(overlaps: readonly string[] | null): Map<number, number[]> {
  const partners = new Map<number, number[]>();
  for (const pair of overlaps ?? []) {
    const [a, b] = pair.split(":").map(Number);
    if (a === undefined || b === undefined || Number.isNaN(a) || Number.isNaN(b)) {
      continue;
    }
    partners.set(a, [...(partners.get(a) ?? []), b]);
    partners.set(b, [...(partners.get(b) ?? []), a]);
  }
  return partners;
}

/** The canonical (RFC 8785) DecisionOutputCell for one choice of an output column. */
export function outcomeCell(output: Column, choice: string): string {
  if (output.type === "bool") {
    return JSON.stringify({ cell: "bool", value: choice === "yes" });
  }
  if (output.type === "enum") {
    return JSON.stringify({ cell: "enum", member: choice });
  }
  if (output.type === "int") {
    return JSON.stringify({ cell: "int", value: Number(choice) });
  }
  return JSON.stringify({ cell: "decimal", value: choice });
}

