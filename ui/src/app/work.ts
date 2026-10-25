import { useQuery } from "@tanstack/react-query";

import { apiClient, readData, readReviewed, type ReviewedItems } from "../api/client";
import type { components } from "../api/schema";

type Question = components["schemas"]["Question"];
type ItemHealth = components["schemas"]["ItemHealth"];
type Finding = components["schemas"]["Finding"];
type GateStatus = components["schemas"]["GateStatus"];
type PendingProposal = components["schemas"]["PendingProposal"];

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

/**
 * The open questions still waiting for an answer: an answered question stays open until its
 * answer proposal (origin `question`, `origin_ref` = the question) is confirmed or discarded, and
 * meanwhile the Inbox lists that "Answer to confirm" instead of the question.
 */
export function questionsAwaitingAnswer(questions: readonly Question[], proposals: readonly PendingProposal[]): Question[] {
  const answered = new Set(proposals.filter((p) => p.origin === "question" && typeof p.origin_ref === "string").map((p) => p.origin_ref));
  return questions.filter((q) => !answered.has(q.id));
}

/** A blocker waiting for a human decision (Inbox "Issue to decide"). */
export interface IssueToDecide {
  /** The finding ID, or `<gate>:<rule>` for a failing rule that has no Finding node yet. */
  readonly id: string;
  /** Null until the finding is materialized (`POST /api/findings/materialize`). */
  readonly findingId: string | null;
  readonly code: string;
  /** The gate (validation family), e.g. F1. */
  readonly gate: string;
  /** The finding message; null for a rule result without a Finding node. */
  readonly message: string | null;
  readonly affected: readonly string[];
}

/**
 * The issues to decide: open unwaived blocker findings that no current gate report waived, then
 * the failing blocker rules of current gate reports that have no Finding node yet (they are
 * materialized before a waiver).
 */
export function issuesToDecide(findings: readonly Finding[], gates: readonly GateStatus[]): IssueToDecide[] {
  const current = gates.filter((g) => g.state === "current");
  const waived = new Set<string>();
  for (const g of current) {
    g.waivers.forEach((w) => waived.add(w.finding_ref));
    g.rules.filter((r) => r.state === "WAIVED" && r.finding_ref !== null).forEach((r) => waived.add(r.finding_ref as string));
  }
  const issues: IssueToDecide[] = findings
    .filter((f) => needsDecision(f) && !waived.has(f.id))
    .map((f) => ({ id: f.id, findingId: f.id, code: f.code, gate: f.family, message: f.message, affected: f.affected_refs }));
  for (const g of current) {
    for (const r of g.rules) {
      if (r.state === "FAIL" && r.effective_severity.toLowerCase() === "blocker" && r.finding_ref === null) {
        issues.push({ id: g.gate + ":" + r.rule_id, findingId: null, code: r.rule_id, gate: g.gate, message: null, affected: r.targets });
      }
    }
  }
  return issues;
}

function listLength(items: unknown): number | undefined {
  return Array.isArray(items) ? items.length : undefined;
}

export interface WorkCounts {
  /** Pending proposals (undefined while loading or failed). */
  readonly proposals: number | undefined;
  readonly openQuestions: number | undefined;
  readonly findingsToDecide: number | undefined;
  /** The Inbox badge: exactly what the Inbox lists (pending proposals, questions awaiting an answer, issues to decide). */
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
  const proposalItems = Array.isArray(proposals.data?.items) ? proposals.data.items : undefined;
  const questionItems = Array.isArray(questions.data?.items) ? questions.data.items : undefined;
  const p = listLength(proposalItems);
  const q = questionItems === undefined ? undefined : questionsAwaitingAnswer(questionItems, proposalItems ?? []).length;
  const findingItems = Array.isArray(findings.data) ? findings.data : undefined;
  const f = findingItems === undefined ? undefined : issuesToDecide(findingItems, Array.isArray(gates.data) ? gates.data : []).length;
  return {
    proposals: p,
    openQuestions: q,
    findingsToDecide: f,
    inbox: p === undefined || q === undefined ? undefined : p + q + (f ?? 0),
    gates: Array.isArray(gates.data) ? gates.data : undefined,
  };
}
