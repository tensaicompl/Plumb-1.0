import { QueryClient } from "@tanstack/react-query";
import createClient from "openapi-fetch";
import { createContext, useContext } from "react";

import type { paths } from "./schema";

// The typed API client of contract v3, generated from api/modeller.openapi.json. Paths are
// relative (/api/...): the browser origin serves them (the Vite dev proxy in development).
export const apiClient = createClient<paths>();

export type ApiClient = typeof apiClient;

/** The typed error envelope of every unsuccessful API response. */
export interface ErrorResponse {
  readonly code: string;
  readonly message: string;
  readonly details: unknown;
}

/**
 * Server data the user reviews, with the `ETag` of the read that loaded it (architecture §33.2):
 * the strong entity-tag of the branch-head revision. A mutation based on this data sends exactly
 * that value as If-Match; null when the response carried no ETag.
 */
export type Reviewed<T> = T & { readonly etag: string | null };

/** The strong entity-tag of a revision ID, the form GET responses carry in `ETag`. */
export function revisionEtag(revision: string): string {
  return `"${revision}"`;
}

function typedError(body: unknown): ErrorResponse | null {
  if (typeof body !== "object" || body === null) {
    return null;
  }
  const v = body as Record<string, unknown>;
  return typeof v["code"] === "string" && typeof v["message"] === "string" ? (body as ErrorResponse) : null;
}

/** A read the server answered unsuccessfully, with its typed `{code, message}` when present. */
export class ReadFailedError extends Error {
  readonly status: number;
  readonly code: string | null;
  readonly serverMessage: string | null;

  constructor(status: number, body: unknown) {
    super(`read failed with HTTP ${status}`);
    this.name = "ReadFailedError";
    this.status = status;
    const typed = typedError(body);
    this.code = typed?.code ?? null;
    this.serverMessage = typed?.message ?? null;
  }
}

/** The result shape of a generated openapi-fetch call. */
export interface ApiResponse<TData> {
  readonly data?: TData;
  readonly error?: unknown;
  readonly response: Response;
}

/**
 * Awaits one generated GET call: its data together with the response `ETag`, or a
 * ReadFailedError carrying the typed backend error. A network failure propagates unchanged.
 */
export async function readReviewed<TData extends object>(call: Promise<ApiResponse<TData>>): Promise<Reviewed<TData>> {
  const { data, error, response } = await call;
  if (!response.ok || data === undefined) {
    throw new ReadFailedError(response.status, error);
  }
  return { ...data, etag: response.headers.get("etag") };
}

/** Awaits one generated GET call whose data is not reviewed for a write (e.g. a JSON array). */
export async function readData<TData>(call: Promise<ApiResponse<TData>>): Promise<TData> {
  const { data, error, response } = await call;
  if (!response.ok || data === undefined) {
    throw new ReadFailedError(response.status, error);
  }
  return data;
}

/** The reviewed items of a page read (`{items, ...}`), for lists that render items only. */
export interface ReviewedItems<TItem> {
  readonly items: TItem[];
  readonly etag: string | null;
}

/**
 * The ETag of the list read whose items a subtree renders, so an item's write control sends the
 * version of the list the user reviewed (undefined outside a provider).
 */
export const ReviewedEtag = createContext<string | null | undefined>(undefined);

/** The ETag of the enclosing reviewed list read. */
export function useReviewedEtag(): string | null | undefined {
  return useContext(ReviewedEtag);
}

/** Statuses whose reads are retried (transient gateway and availability failures). */
const RETRIED_STATUSES: readonly number[] = [502, 503, 504];

/** Retries a read at most twice, and only after a network failure or HTTP 502, 503 or 504. */
export function shouldRetryRead(failureCount: number, error: unknown): boolean {
  if (failureCount >= 2) {
    return false;
  }
  if (error instanceof ReadFailedError) {
    return RETRIED_STATUSES.includes(error.status);
  }
  return error instanceof TypeError;
}

/** Exponential read retry backoff: 500 ms, then 1 s. */
export function readRetryDelay(attempt: number): number {
  return 500 * 2 ** attempt;
}

/** The application QueryClient: the read retry policy; writes are never retried. */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: shouldRetryRead, retryDelay: readRetryDelay },
      mutations: { retry: false },
    },
  });
}
