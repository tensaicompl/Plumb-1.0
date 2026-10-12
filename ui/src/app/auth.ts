import { useQuery } from "@tanstack/react-query";

import { apiClient } from "../api/client";
import type { components } from "../api/schema";

/** The server-authoritative identity of the current user (GET /api/me). */
export type Me = components["schemas"]["MeResponse"];

/** The query key of the current identity. */
export const meQueryKey = ["me"] as const;

/** Reads GET /api/me. */
export async function fetchMe(): Promise<Me> {
  const { data, response } = await apiClient.GET("/api/me");
  if (data === undefined) {
    throw new Error(`GET /api/me failed with HTTP ${response.status}`);
  }
  return data;
}

/** The current identity, owned by TanStack Query. */
export function useMe() {
  return useQuery({ queryKey: meQueryKey, queryFn: fetchMe });
}

/** Whether the server listed exactly `role` for the user (no hierarchy or inference). */
export function hasRole(me: Me | undefined, role: string): boolean {
  return me?.roles.includes(role) ?? false;
}

/** Whether the server listed at least one of `roles` for the user. */
export function hasAnyRole(me: Me | undefined, roles: readonly string[]): boolean {
  return roles.some((role) => hasRole(me, role));
}
