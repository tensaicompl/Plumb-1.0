import { apiClient, readData, type ApiResponse } from "../../api/client";
import type { components } from "../../api/schema";

type SourceListItem = components["schemas"]["SourceListItem"];
type Span = components["schemas"]["Span"];

/** The page size of the "read everything" reads (the server's page limit allows it). */
export const ALL_PAGE_SIZE = 500;

/** Every item of a cursor-paged read, following `next_cursor` to the last page. */
export async function readAllPages<TItem>(page: (cursor: string | undefined) => Promise<ApiResponse<{ items: TItem[]; next_cursor?: string | null }>>): Promise<TItem[]> {
  const items: TItem[] = [];
  let cursor: string | undefined = undefined;
  for (;;) {
    const data: { items: TItem[]; next_cursor?: string | null } = await readData(page(cursor));
    items.push(...data.items);
    if (data.next_cursor === null || data.next_cursor === undefined) {
      return items;
    }
    cursor = data.next_cursor;
  }
}

/** GET /api/sources: every imported document. Key ["sources"]. */
export const sourcesQuery = {
  queryKey: ["sources"] as const,
  queryFn: async (): Promise<SourceListItem[]> => (await readData(apiClient.GET("/api/sources"))).items,
};

/** GET /api/sources/{id}/spans, every page, in document order. Key ["sources", id, "spans"]. */
export function sourceSpansQuery(id: string) {
  return {
    queryKey: ["sources", id, "spans"] as const,
    queryFn: async (): Promise<Span[]> => {
      const spans = await readAllPages((cursor) =>
        apiClient.GET("/api/sources/{id}/spans", { params: { path: { id }, query: { limit: ALL_PAGE_SIZE, ...(cursor === undefined ? {} : { cursor }) } } }),
      );
      return [...spans].sort((a, b) => (a.start ?? Number.MAX_SAFE_INTEGER) - (b.start ?? Number.MAX_SAFE_INTEGER) || a.id.localeCompare(b.id));
    },
  };
}
