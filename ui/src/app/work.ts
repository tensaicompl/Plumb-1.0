import { useQuery } from "@tanstack/react-query";

import { apiClient, readData, readReviewed, type ReviewedItems } from "../api/client";
import type { components } from "../api/schema";

type Question = components["schemas"]["Question"];
type ItemHealth = components["schemas"]["ItemHealth"];
type Finding = components["schemas"]["Finding"];
type GateStatus = components["schemas"]["GateStatus"];

// The shared "work" reads behind the Inbox badge and the Next action. Their keys and data shapes
// are the same as the screens' reads of the same endpoints, so a screen and the shell share one
// cache entry (and one request) instead of fetching twice. Screens should reuse these.

/** GET /api/proposals (every pending proposal), reviewed for writes. Key ["proposals"]. */
export const proposalsQuery = {
  queryKey: ["proposals"] as const,
  queryFn: () => readReviewed(apiClient.GET("/api/proposals")),
};

/** GET /api/questions?status=Open, reviewed for writes. Key ["questions", "open"]. */
export const openQuestionsQuery = {
  queryKey: ["questions", "open"] as const,
  queryFn: async (): Promise<ReviewedItems<Question> & { readonly health?: ItemHealth[] }> => {
    const page = await readReviewed(apiClient.GET("/api/questions", { params: { query: { status: "Open" } } }));
    return { items: page.items, health: page.health, etag: page.etag };
  },
};

/** GET /api/findings (all, unfiltered). Key ["findings"]. */
export const findingsQuery = {
  queryKey: ["findings"] as const,
  queryFn: async (): Promise<Finding[]> => (await readData(apiClient.GET("/api/findings"))).items,
};

/** GET /api/gates. Key ["gates"]. */
export const gatesQuery = {
  queryKey: ["gates"] as const,
  queryFn: (): Promise<GateStatus[]> => readData(apiClient.GET("/api/gates")),
};

/** A finding that needs a human decision: an open, unwaived blocker (fix it or waive it with a reason). */
export function needsDecision(finding: Finding): boolean {
  return finding.status.toLowerCase() === "open" && finding.severity.toLowerCase() === "blocker" && (finding.waiver_ref ?? null) === null;
}

function listLength(items: unknown): number | undefined {
  return Array.isArray(items) ? items.length : undefined;
}

export interface WorkCounts {
  /** Pending proposals (undefined while loading or failed). */
  readonly proposals: number | undefined;
  readonly openQuestions: number | undefined;
  readonly findingsToDecide: number | undefined;
  /** The Inbox badge: pending proposals + open questions + findings needing a decision. */
  readonly inbox: number | undefined;
  readonly gates: GateStatus[] | undefined;
}

/** The counts behind the Inbox badge and the Next action (refreshed by every design event). */
export function useWorkCounts(): WorkCounts {
  const proposals = useQuery(proposalsQuery);
  const questions = useQuery(openQuestionsQuery);
  const findings = useQuery(findingsQuery);
  const gates = useQuery(gatesQuery);
  // The shell never breaks a screen: a malformed list counts as not loaded.
  const p = listLength(proposals.data?.items);
  const q = listLength(questions.data?.items);
  const f = Array.isArray(findings.data) ? findings.data.filter(needsDecision).length : undefined;
  return {
    proposals: p,
    openQuestions: q,
    findingsToDecide: f,
    inbox: p === undefined || q === undefined ? undefined : p + q + (f ?? 0),
    gates: Array.isArray(gates.data) ? gates.data : undefined,
  };
}
