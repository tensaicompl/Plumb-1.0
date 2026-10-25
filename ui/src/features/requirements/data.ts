// Requirement reads shared by the Requirements and Quality screens: the whole requirement list
// (filters and document order are local), one requirement with its reviewed ETag, its readiness,
// the imported sources, and the open quality issues of a requirement (findings minus waived and
// closed ones).
import { apiClient, readData, readReviewed, type Reviewed } from "../../api/client";
import type { components } from "../../api/schema";

export type RequirementSummary = components["schemas"]["RequirementSummary"];
export type RequirementDetail = components["schemas"]["RequirementDetail"];
export type Finding = components["schemas"]["Finding"];
export type GateStatus = components["schemas"]["GateStatus"];
type ItemHealth = components["schemas"]["ItemHealth"];
type Readiness = components["schemas"]["Readiness"];
type SourceListItem = components["schemas"]["SourceListItem"];

/** The page size of the full-list read (a project's requirements fit in a few pages). */
export const REQUIREMENTS_PAGE_LIMIT = 500;

export interface RequirementList {
  readonly items: RequirementSummary[];
  readonly health: ItemHealth[];
}

/** Every requirement (all pages of GET /api/requirements). Key ["requirements", "all"]. */
export const requirementsQuery = {
  queryKey: ["requirements", "all"] as const,
  queryFn: async (): Promise<RequirementList> => {
    const items: RequirementSummary[] = [];
    const health: ItemHealth[] = [];
    let cursor: string | null = null;
    do {
      const query: { limit: number; cursor?: string } = { limit: REQUIREMENTS_PAGE_LIMIT };
      if (cursor !== null) {
        query.cursor = cursor;
      }
      const page = await readData(apiClient.GET("/api/requirements", { params: { query } }));
      items.push(...page.items);
      health.push(...(page.health ?? []));
      cursor = page.next_cursor ?? null;
    } while (cursor !== null);
    return { items, health };
  },
};

/** One requirement, reviewed for writes. Key ["requirements", "detail", id]. */
export function requirementQuery(id: string) {
  return {
    queryKey: ["requirements", "detail", id] as const,
    queryFn: (): Promise<Reviewed<RequirementDetail>> => readReviewed(apiClient.GET("/api/requirements/{id}", { params: { path: { id } } })),
  };
}

/** One requirement's readiness. Key ["requirements", "readiness", id]. */
export function readinessQuery(id: string) {
  return {
    queryKey: ["requirements", "readiness", id] as const,
    queryFn: (): Promise<Readiness> => readData(apiClient.GET("/api/requirements/{id}/readiness", { params: { path: { id } } })),
  };
}

/** The imported sources. Key ["sources"]. */
export const sourcesQuery = {
  queryKey: ["sources"] as const,
  queryFn: async (): Promise<SourceListItem[]> => (await readData(apiClient.GET("/api/sources"))).items,
};

/** The IDs of findings a current gate report waives (the finding list may still call them Open). */
export function waivedFindingIds(gates: readonly GateStatus[] | undefined): ReadonlySet<string> {
  const ids = new Set<string>();
  for (const gate of gates ?? []) {
    for (const waiver of gate.waivers) {
      ids.add(waiver.finding_ref);
    }
  }
  return ids;
}

/** Whether a finding still needs attention: open, and not waived. */
export function isOpenIssue(finding: Finding, waived: ReadonlySet<string>): boolean {
  return finding.status.toLowerCase() === "open" && (finding.waiver_ref ?? null) === null && !waived.has(finding.id);
}

/** The open quality issues (findings) affecting one requirement. */
export function issuesOf(requirementId: string, findings: readonly Finding[] | undefined, waived: ReadonlySet<string>): Finding[] {
  return (findings ?? []).filter((finding) => finding.affected_refs.includes(requirementId) && isOpenIssue(finding, waived));
}

/** A requirement's short label: its source identifier ("HR-009"), else its ID. */
export function requirementLabel(requirement: Pick<RequirementSummary, "id" | "source_identifier">): string {
  const source = requirement.source_identifier ?? null;
  return source !== null && source.trim() !== "" ? source : requirement.id;
}

const collator = new Intl.Collator("en", { numeric: true, sensitivity: "base" });

/** Document order: by source identifier (numeric-aware), requirements without one last, then by ID. */
export function compareDocumentOrder(a: RequirementSummary, b: RequirementSummary): number {
  const sa = a.source_identifier ?? null;
  const sb = b.source_identifier ?? null;
  if (sa !== null && sb !== null) {
    return collator.compare(sa, sb) || a.id.localeCompare(b.id);
  }
  if (sa !== null) {
    return -1;
  }
  if (sb !== null) {
    return 1;
  }
  return a.id.localeCompare(b.id);
}

/** The readiness below which a requirement counts as low (the ring's amber threshold). */
export const LOW_READINESS = 70;
