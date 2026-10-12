import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import type { components } from "./schema";

export type DesignEvent = components["schemas"]["DesignEvent"];

/** Whether `value` is a well-formed DesignEvent `{kind, ids, version}`. */
export function isDesignEvent(value: unknown): value is DesignEvent {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const v = value as Record<string, unknown>;
  return (
    typeof v["kind"] === "string" &&
    typeof v["version"] === "string" &&
    Array.isArray(v["ids"]) &&
    v["ids"].every((id) => typeof id === "string")
  );
}

/** The same-origin event stream URL (ws for http, wss for https). */
export function eventsUrl(location: Pick<Location, "protocol" | "host">): string {
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${location.host}/api/events`;
}

/** Parses one socket message; null for anything that is not a DesignEvent. */
export function parseDesignEvent(data: unknown): DesignEvent | null {
  if (typeof data !== "string") {
    return null;
  }
  try {
    const value: unknown = JSON.parse(data);
    return isDesignEvent(value) ? value : null;
  } catch {
    return null;
  }
}

/**
 * Subscribes to the forward-only DesignEvent stream while mounted. A valid event only
 * invalidates server queries (one commit may affect several projections); its fields are never
 * written into the cache. Malformed messages are ignored.
 */
export function useEvents(): void {
  const queryClient = useQueryClient();
  useEffect(() => {
    const socket = new WebSocket(eventsUrl(window.location));
    const onMessage = (message: MessageEvent) => {
      if (parseDesignEvent(message.data) !== null) {
        void queryClient.invalidateQueries();
      }
    };
    socket.addEventListener("message", onMessage);
    return () => {
      socket.removeEventListener("message", onMessage);
      socket.close();
    };
  }, [queryClient]);
}
