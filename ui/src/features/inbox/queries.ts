// The Inbox inspector reads (TanStack Query). Each keeps the read's ETag where a write is based on it.
import { apiClient, readData, readReviewed } from "../../api/client";

/** GET /api/proposals/{id}: facts, evidence and model change of one pending proposal. */
export function proposalDetailQuery(id: string) {
  return {
    queryKey: ["proposal", id] as const,
    queryFn: () => readReviewed(apiClient.GET("/api/proposals/{id}", { params: { path: { id } } })),
  };
}

/** GET /api/requirements/{id}: the statement and its pending EARS suggestion. */
export function requirementQuery(id: string) {
  return {
    queryKey: ["requirement", id] as const,
    queryFn: () => readReviewed(apiClient.GET("/api/requirements/{id}", { params: { path: { id } } })),
  };
}

/** GET /api/model/sentences: the typed slots (class, level, modality) of every requirement. */
export const sentencesQuery = {
  queryKey: ["sentences"] as const,
  queryFn: async () => (await readData(apiClient.GET("/api/model/sentences"))).items,
};

/** GET /api/sources: source names by ID, for evidence footers. */
export const sourceNamesQuery = {
  queryKey: ["sources", "names"] as const,
  queryFn: async (): Promise<ReadonlyMap<string, string>> => new Map((await readData(apiClient.GET("/api/sources"))).items.map((s) => [s.id, s.name] as const)),
};

/** The requirement identifier ("HR-023") its source passage gives it, when the passage names one. */
export function requirementIdentifier(evidenceText: string | null | undefined): string | null {
  const match = /\*\*([A-Z][A-Z0-9]*-\d+)\*\*/.exec(evidenceText ?? "");
  return match?.[1] ?? null;
}
