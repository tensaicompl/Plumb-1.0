import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { create } from "zustand";

import { t } from "../i18n";
import { apiClient, readReviewed, type ApiResponse, type Reviewed } from "./client";
import type { components } from "./schema";

export type ModelVersion = components["schemas"]["ModelVersionResponse"];
export type PreconditionFailed = components["schemas"]["PreconditionFailedResponse"];

/** The query key of the current model version (GET /api/model/version). */
export const modelVersionQueryKey = ["model", "version"] as const;

/** Reads GET /api/model/version. */
export function fetchModelVersion(): Promise<Reviewed<ModelVersion>> {
  return readReviewed(apiClient.GET("/api/model/version"));
}

/** The current model version, owned by TanStack Query (display only, never a write precondition). */
export function useModelVersion() {
  return useQuery({ queryKey: modelVersionQueryKey, queryFn: fetchModelVersion });
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

/** Client-side typed code of a write that never received an HTTP response. */
export const TRANSPORT_FAILED = "E_TRANSPORT_FAILED";

/** Client-side typed code of a write attempted before its reviewed data was loaded. */
export const VIEW_NOT_LOADED = "E_VIEW_NOT_LOADED";

/** A write that failed without an HTTP response (network loss, aborted or non-HTTP failure). */
export class TransportFailedError extends WriteFailedError {
  constructor() {
    super(0, { code: TRANSPORT_FAILED, message: t("write.transportFailed"), details: {} });
    this.name = "TransportFailedError";
  }
}

/** The human-readable explanation of a failed write: the server `message`, the typed transport
 * failure, or a plain explanation when the AI inference provider is unavailable. */
export function writeErrorMessage(error: unknown): string | null {
  if (!(error instanceof WriteFailedError) || typeof error.body !== "object" || error.body === null) {
    return null;
  }
  const body = error.body as Record<string, unknown>;
  if (body["code"] === "E_INFERENCE_FAILED" || body["code"] === "E_INFERENCE_REQUIRED") {
    return t("write.inferenceUnavailable");
  }
  return typeof body["message"] === "string" ? body["message"] : null;
}

/** The result shape of a generated openapi-fetch call. */
export type WriteResponse<TData> = ApiResponse<TData>;

/** The reviewed version of a write: the ETag the user's data was loaded with (or null when not loaded yet). */
export type ReviewedEtag<TVariables> = string | null | undefined | ((variables: TVariables) => string | null | undefined);

/**
 * A typed write against the version of the data the user reviewed (architecture §33.2): `reviewed`
 * is the `ETag` of that data's read (or, for data a write returned, the strong entity-tag of that
 * result's revision) and is passed to `write` as its If-Match value; nothing is fetched first and
 * the write runs exactly once. A 412 invalidates every server query, opens the global
 * ConflictDialog and fails the mutation; nothing is retried, rebased or replayed. A write without
 * an HTTP response fails with the typed TransportFailedError.
 */
export function useVersionedMutation<TData, TVariables = void>(
  write: (variables: TVariables, ifMatch: string) => Promise<WriteResponse<TData>>,
  reviewed: ReviewedEtag<TVariables>,
) {
  const queryClient = useQueryClient();
  const setConflict = useConflictStore((state) => state.setConflict);
  return useMutation<TData, Error, TVariables>({
    retry: false,
    mutationFn: async (variables) => {
      const etag = typeof reviewed === "function" ? reviewed(variables) : reviewed;
      if (etag === null || etag === undefined) {
        throw new WriteFailedError(0, { code: VIEW_NOT_LOADED, message: t("write.notLoaded"), details: {} });
      }
      let result: WriteResponse<TData>;
      try {
        result = await write(variables, etag);
      } catch {
        throw new TransportFailedError();
      }
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
