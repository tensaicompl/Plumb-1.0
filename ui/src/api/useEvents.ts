import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { create } from "zustand";

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

/** The live connection: connecting (first attempt), open, or lost (reconnecting after a close). */
export type ConnectionStatus = "connecting" | "open" | "lost";

interface ConnectionState {
  readonly status: ConnectionStatus;
  setStatus(status: ConnectionStatus): void;
}

/** View state of the event stream connection (the shell's connection banner reads it). */
export const useConnectionStore = create<ConnectionState>()((set) => ({
  status: "connecting",
  setStatus: (status) => set({ status }),
}));

/** The first reconnect delay and the cap of the exponential backoff (architecture §33.12). */
export const RECONNECT_BASE_MS = 1000;
export const RECONNECT_CAP_MS = 30000;

/**
 * The delay before reconnect attempt `attempt` (0-based): 1 s doubling to a 30 s cap, with
 * jitter drawn from the upper half of the step so concurrent clients spread out.
 */
export function reconnectDelay(attempt: number, random: () => number = Math.random): number {
  const step = Math.min(RECONNECT_BASE_MS * 2 ** attempt, RECONNECT_CAP_MS);
  return Math.round(step / 2 + (random() * step) / 2);
}

/**
 * Subscribes to the forward-only DesignEvent stream while mounted. A valid event only
 * invalidates server queries (one commit may affect several projections); its fields are never
 * written into the cache. Malformed messages are ignored. After a close or network loss (including
 * the server's events-missed lag close) the stream reconnects with exponential backoff and, once
 * connected again, invalidates the server queries exactly once, since events may have been missed.
 * Unmounting stops reconnection. The connection status is mirrored into `useConnectionStore`.
 */
export function useEvents(): void {
  const queryClient = useQueryClient();
  useEffect(() => {
    let stopped = false;
    let attempt = 0;
    let missed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let socket: WebSocket | undefined;

    const onMessage = (message: MessageEvent) => {
      if (parseDesignEvent(message.data) !== null) {
        void queryClient.invalidateQueries();
      }
    };
    const setStatus = useConnectionStore.getState().setStatus;
    const onOpen = () => {
      attempt = 0;
      setStatus("open");
      if (missed) {
        missed = false;
        void queryClient.invalidateQueries();
      }
    };
    const onClose = () => {
      if (stopped) {
        return;
      }
      missed = true;
      setStatus("lost");
      detach();
      timer = setTimeout(connect, reconnectDelay(attempt));
      attempt += 1;
    };
    function detach() {
      socket?.removeEventListener("message", onMessage);
      socket?.removeEventListener("open", onOpen);
      socket?.removeEventListener("close", onClose);
    }
    function connect() {
      timer = undefined;
      socket = new WebSocket(eventsUrl(window.location));
      socket.addEventListener("message", onMessage);
      socket.addEventListener("open", onOpen);
      socket.addEventListener("close", onClose);
    }

    connect();
    return () => {
      stopped = true;
      if (timer !== undefined) {
        clearTimeout(timer);
      }
      detach();
      socket?.close();
      setStatus("connecting");
    };
  }, [queryClient]);
}
