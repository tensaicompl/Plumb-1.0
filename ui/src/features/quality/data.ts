// Quality reads and helpers: decisions (waiver who and why), reviewed findings and assumptions for
// governed writes, affected-item labels and where "Show affected" goes.
import { apiClient, readData, readReviewed, type ReviewedItems } from "../../api/client";
import type { components } from "../../api/schema";
import type { StageName } from "../../app/pipeline";
import { tc } from "../../i18n";
import { requirementLabel, type GateStatus, type RequirementSummary } from "../requirements/data";

export type Decision = components["schemas"]["Decision"];
type Finding = components["schemas"]["Finding"];
type Assumption = components["schemas"]["Assumption"];
type ItemHealth = components["schemas"]["ItemHealth"];
type GateName = components["schemas"]["GateName"];

/** Every decision (all pages of GET /api/decisions). Key ["decisions"]. */
export const decisionsQuery = {
  queryKey: ["decisions"] as const,
  queryFn: async (): Promise<Decision[]> => {
    const items: Decision[] = [];
    let cursor: string | null = null;
    do {
      const query: { limit: number; cursor?: string } = { limit: 500 };
      if (cursor !== null) {
        query.cursor = cursor;
      }
      const page = await readData(apiClient.GET("/api/decisions", { params: { query } }));
      items.push(...page.items);
      cursor = page.next_cursor ?? null;
    } while (cursor !== null);
    return items;
  },
};

/** The findings, reviewed for a waiver. Key ["findings", "reviewed"]. */
export const reviewedFindingsQuery = {
  queryKey: ["findings", "reviewed"] as const,
  queryFn: async (): Promise<ReviewedItems<Finding>> => {
    const page = await readReviewed(apiClient.GET("/api/findings"));
    return { items: page.items, etag: page.etag };
  },
};

/** The assumptions with their health markers, reviewed for Resolve. Key ["assumptions"]. */
export const assumptionsQuery = {
  queryKey: ["assumptions"] as const,
  queryFn: async (): Promise<ReviewedItems<Assumption> & { readonly health: ItemHealth[] }> => {
    const page = await readReviewed(apiClient.GET("/api/assumptions"));
    return { items: page.items, health: page.health, etag: page.etag };
  },
};

/** The step whose run re-evaluates a gate; I0 and F3 have none (they are evaluated with the gates). */
export const GATE_OWNER: Readonly<Partial<Record<GateName, StageName>>> = { F1: "lint", F2: "analyse", F4: "run" };

/** The decision that waived a finding: the finding's own waiver reference, else a gate report waiver. */
export function waiverDecision(findingId: string, waiverRef: string | null | undefined, gates: readonly GateStatus[] | undefined, decisions: readonly Decision[] | undefined): Decision | null {
  const ref = waiverRef ?? gates?.flatMap((gate) => gate.waivers).find((waiver) => waiver.finding_ref === findingId)?.decision_ref ?? null;
  if (ref === null) {
    return null;
  }
  return decisions?.find((decision) => decision.id === ref) ?? null;
}

/** Whether an element ID names a requirement. */
function isRequirementRef(id: string, labels: ReadonlyMap<string, string>): boolean {
  return labels.has(id) || id.startsWith("req:");
}

/** Requirement ID → short label ("HR-009"). */
export function requirementLabels(requirements: readonly RequirementSummary[] | undefined): ReadonlyMap<string, string> {
  return new Map((requirements ?? []).map((requirement) => [requirement.id, requirementLabel(requirement)]));
}

/** The most chips shown before affected items collapse into one count chip. */
const MAX_CHIPS = 3;

/** The chips of affected items: each label, or one count ("32 requirements") when there are many. */
export function affectedChips(ids: readonly string[], labels: ReadonlyMap<string, string>): string[] {
  if (ids.length > MAX_CHIPS) {
    return [ids.every((id) => isRequirementRef(id, labels)) ? tc("quality.affected.requirements", ids.length) : tc("quality.affected.items", ids.length)];
  }
  return ids.map((id) => labels.get(id) ?? id);
}

/** The Model segment an element belongs to, from its ID prefix (null when not a model element). */
function modelSegment(id: string): string | null {
  const prefix = id.split(":")[0] ?? "";
  if (["entity", "attr", "attribute", "state", "transition", "event", "relation", "rel"].includes(prefix)) {
    return "/model/entities";
  }
  if (["operation", "op", "process", "proc", "pn"].includes(prefix)) {
    return "/model/processes";
  }
  if (["calculation", "calc"].includes(prefix)) {
    return "/model/calculations";
  }
  if (["rule", "dt"].includes(prefix)) {
    return "/model/rules";
  }
  if (["term", "concept"].includes(prefix)) {
    return "/model/glossary";
  }
  return null;
}

/** Where "Show affected" goes: the filtered Requirements list, else the Model segment (null: nowhere). */
export function affectedHref(ids: readonly string[], labels: ReadonlyMap<string, string>): string | null {
  const requirements = [...new Set(ids.filter((id) => isRequirementRef(id, labels)))];
  if (requirements.length > 0) {
    return "/requirements?" + new URLSearchParams({ affected: requirements.join(",") }).toString();
  }
  for (const id of ids) {
    const segment = modelSegment(id);
    if (segment !== null) {
      return segment;
    }
  }
  return null;
}

/** A recorded time in the business time zone (Europe/Warsaw), in the UI locale. */
export function warsawTime(iso: string, locale: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return iso;
  }
  return new Intl.DateTimeFormat(locale === "pl" ? "pl-PL" : "en-GB", {
    timeZone: "Europe/Warsaw",
    day: "numeric",
    month: "short",
    year: date.getUTCFullYear() === new Date().getUTCFullYear() ? undefined : "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}
