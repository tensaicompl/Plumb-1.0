import { useQuery } from "@tanstack/react-query";

import { apiClient, readData } from "../../api/client";

/** The most IDs one GET /api/model/labels request may name. */
const CHUNK = 200;

/** "leave request" → "Leave request" (labels are names; the UI shows them in sentence case). */
export function sentenceCase(text: string): string {
  return text.length === 0 ? text : text.charAt(0).toUpperCase() + text.slice(1);
}

/** "AnnualLeaveEligibility" or "annual_leave" → "Annual leave eligibility" / "Annual leave". */
export function humanize(name: string): string {
  if (/\s/.test(name) || name.includes(":")) {
    return sentenceCase(name);
  }
  const words = name
    .replace(/[_-]+/g, " ")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/([A-Z])([A-Z][a-z])/g, "$1 $2")
    .trim()
    .split(/\s+/)
    .map((word, index) => (index === 0 || word.length < 2 || word !== word.toUpperCase() ? (index === 0 ? word : word.toLowerCase()) : word));
  return sentenceCase(words.join(" "));
}

interface Entry {
  readonly label: string;
  readonly kind: string;
}

async function fetchLabels(ids: readonly string[]): Promise<ReadonlyMap<string, Entry>> {
  const labels = new Map<string, Entry>();
  for (let start = 0; start < ids.length; start += CHUNK) {
    const page = await readData(apiClient.GET("/api/model/labels", { params: { query: { ids: ids.slice(start, start + CHUNK).join(",") } } }));
    for (const item of page.items) {
      labels.set(item.id, { label: item.label, kind: item.kind });
    }
  }
  return labels;
}

export interface Labels {
  /** The element's name in sentence case (terms as written); its ID when unknown; "" while loading. */
  (id: string): string;
  readonly ready: boolean;
  /** The element's node type or relation kind (`operation`, `performed_by`), or null when unknown. */
  kind(id: string): string | null;
}

/**
 * The plain-language names of the model elements a view references by ID (resolve_labels).
 * One request per distinct ID set; design events refresh it with every other query.
 */
export function useLabels(ids: readonly (string | null | undefined)[]): Labels {
  const unique = [...new Set(ids.filter((id): id is string => typeof id === "string" && id !== ""))].sort();
  const query = useQuery({
    queryKey: ["model", "labels", unique],
    queryFn: () => fetchLabels(unique),
    enabled: unique.length > 0,
    staleTime: 30_000,
  });
  const ready = unique.length === 0 || query.data !== undefined || query.isError;
  const label = (id: string): string => {
    const entry = query.data?.get(id);
    if (entry !== undefined) {
      // Glossary terms are vocabulary and keep their own case ("leave request").
      return entry.kind === "term" ? entry.label : sentenceCase(entry.label);
    }
    return ready ? id : "";
  };
  return Object.assign(label, { ready, kind: (id: string) => query.data?.get(id)?.kind ?? null });
}
