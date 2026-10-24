import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { create } from "zustand";

import { apiClient } from "./client";
import type { components } from "./schema";

export type ModelVersion = components["schemas"]["ModelVersionResponse"];
export type PreconditionFailed = components["schemas"]["PreconditionFailedResponse"];

/** The query key of the current model version (GET /api/model/version). */
export const modelVersionQueryKey = ["model", "version"] as const;

/** Reads GET /api/model/version. */
export async function fetchModelVersion(): Promise<ModelVersion> {
  const { data, response } = await apiClient.GET("/api/model/version");
  if (data === undefined) {
    throw new Error(`GET /api/model/version failed with HTTP ${response.status}`);
  }
  return data;
}

/** The current model version, owned by TanStack Query. */
export function useModelVersion() {
  return useQuery({ queryKey: modelVersionQueryKey, queryFn: fetchModelVersion });
}

/** Fetches the model version from the server now (never a stale cache entry). */
export function fetchLatestModelVersion(queryClient: QueryClient): Promise<ModelVersion> {
  return queryClient.fetchQuery({ queryKey: modelVersionQueryKey, queryFn: fetchModelVersion, staleTime: 0 });
}

/** A rejected write precondition, as the server reported it (null fields when not reported). */
export interface Conflict {
  readonly revision: string | null;
  readonly semanticHash: string | null;
}

interface ConflictViewState {
  readonly conflict: Conflict | null;
  setConflict(conflict: Conflict): void;
  clearConflict(): void;
}

/** View state of the global ConflictDialog only; server data never lives here. */
export const useConflictStore = create<ConflictViewState>()((set) => ({
  conflict: null,
  setConflict: (conflict) => set({ conflict }),
  clearConflict: () => set({ conflict: null }),
}));

function isPreconditionFailed(value: unknown): value is PreconditionFailed {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const v = value as Record<string, unknown>;
  return typeof v["revision"] === "string" && typeof v["semantic_hash"] === "string";
}

/** A write rejected with 412: the head moved; it is never retried automatically. */
export class PreconditionFailedError extends Error {
  readonly conflict: Conflict;

  constructor(conflict: Conflict) {
    super("the model changed on the server (HTTP 412)");
    this.name = "PreconditionFailedError";
    this.conflict = conflict;
  }
}

/** Any other unsuccessful write response. */
export class WriteFailedError extends Error {
  readonly status: number;
  readonly body: unknown;

  constructor(status: number, body: unknown) {
    super(`write failed with HTTP ${status}`);
    this.name = "WriteFailedError";
    this.status = status;
    this.body = body;
  }
}

/** The human-readable explanation of a failed write: the server `message`, or a plain
 * explanation when the AI inference provider is unavailable. */
export function writeErrorMessage(error: unknown): string | null {
  if (!(error instanceof WriteFailedError) || typeof error.body !== "object" || error.body === null) {
    return null;
  }
  const body = error.body as Record<string, unknown>;
  if (body["code"] === "E_INFERENCE_FAILED" || body["code"] === "E_INFERENCE_REQUIRED") {
    return "The AI model is not available: no inference provider is configured for this server.";
  }
  return typeof body["message"] === "string" ? body["message"] : null;
}

/** The result shape of a generated openapi-fetch call. */
export interface WriteResponse<TData> {
  readonly data?: TData;
  readonly error?: unknown;
  readonly response: Response;
}

/**
 * A typed write executed against the latest model version: the server's current `semantic_hash`
 * is fetched at execution time and passed to `write` as its If-Match value; the write runs
 * exactly once. A 412 invalidates every server query, opens the global ConflictDialog and fails
 * the mutation; nothing is retried, rebased or replayed.
 */
export function useVersionedMutation<TData, TVariables = void>(
  write: (variables: TVariables, ifMatch: string) => Promise<WriteResponse<TData>>,
) {
  const queryClient = useQueryClient();
  const setConflict = useConflictStore((state) => state.setConflict);
  return useMutation<TData, Error, TVariables>({
    retry: false,
    mutationFn: async (variables) => {
      const version = await fetchLatestModelVersion(queryClient);
      const result = await write(variables, version.semantic_hash);
      if (result.response.status === 412) {
        const body = isPreconditionFailed(result.error) ? result.error : null;
        const conflict: Conflict = {
          revision: body?.revision ?? null,
          semanticHash: body?.semantic_hash ?? null,
        };
        setConflict(conflict);
        await queryClient.invalidateQueries();
        throw new PreconditionFailedError(conflict);
      }
      if (!result.response.ok || result.data === undefined) {
        throw new WriteFailedError(result.response.status, result.error);
      }
      return result.data;
    },
  });
}
