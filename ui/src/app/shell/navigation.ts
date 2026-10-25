import { useRouter } from "@tanstack/react-router";
import { useCallback } from "react";

/** A path segment, percent-encoded except ":" and "@" (valid in segments; IDs read naturally). */
function encodeSegment(value: string): string {
  return encodeURIComponent(value).replace(/%3A/gi, ":").replace(/%40/g, "@");
}

/** An app URL from a route pattern ("/quality/$gate"), its params and search (all encoded). */
export function buildHref(to: string, params: Readonly<Record<string, string>> = {}, search: Readonly<Record<string, string>> = {}): string {
  const path = to.replace(/\$(\w+)/g, (_, name: string) => encodeSegment(params[name] ?? ""));
  const query = new URLSearchParams(search).toString();
  return query === "" ? path : path + "?" + query;
}

/** Client-side navigation to an app URL (through the router history). */
export function useGo(): (href: string) => void {
  const router = useRouter();
  return useCallback((href: string) => router.history.push(href), [router]);
}
