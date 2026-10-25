import { useEffect, useState } from "react";

import { ReadFailedError } from "./client";

/** The typed error of a cursor issued at another branch-head revision. */
export const CURSOR_BASELINE_CHANGED = "E_CURSOR_BASELINE_CHANGED";

/** Whether a read failed because its page cursor belongs to an earlier baseline. */
export function isCursorBaselineChanged(error: unknown): boolean {
  return error instanceof ReadFailedError && error.code === CURSOR_BASELINE_CHANGED;
}

/** The cursor stack of a paged list. */
export interface CursorPages {
  /** The current page's cursor (null on the first page). */
  readonly cursor: string | null;
  readonly isFirst: boolean;
  /** Set after a stale cursor reset the list, until the next explicit navigation. */
  readonly recovered: boolean;
  next(cursor: string): void;
  previous(): void;
  first(): void;
  recover(): void;
}

/** The cursor stack of a paged list; pair it with [`useCursorRecovery`]. */
export function useCursorPages(): CursorPages {
  // Cursors of the pages before the current one; the last entry is the current page's cursor.
  const [cursors, setCursors] = useState<(string | null)[]>([null]);
  const [recovered, setRecovered] = useState(false);
  return {
    cursor: cursors[cursors.length - 1] ?? null,
    isFirst: cursors.length === 1,
    recovered,
    next: (next) => {
      setRecovered(false);
      setCursors((current) => [...current, next]);
    },
    previous: () => {
      setRecovered(false);
      setCursors((current) => (current.length > 1 ? current.slice(0, -1) : current));
    },
    first: () => {
      setRecovered(false);
      setCursors([null]);
    },
    recover: () => {
      setRecovered(true);
      setCursors([null]);
    },
  };
}

/**
 * Cursor recovery (architecture §33.12): when the current page-2+ query failed with
 * E_CURSOR_BASELINE_CHANGED the list resets to its first page on the current baseline, which its
 * query then fetches once. The historical cursor is never retried (reads retry only transport and
 * gateway failures).
 */
export function useCursorRecovery(pages: CursorPages, error: unknown): void {
  const stale = pages.cursor !== null && isCursorBaselineChanged(error);
  const { recover } = pages;
  useEffect(() => {
    if (stale) {
      recover();
    }
    // `recover` is a fresh closure every render; only the stale transition matters.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stale]);
}
