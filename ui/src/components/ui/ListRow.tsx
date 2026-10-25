import type { MouseEvent, ReactNode } from "react";

import { t, tf } from "../../i18n";
import { cn } from "./cn";
import { StatusPill } from "./StatusPill";

export interface ListRowProps {
  readonly title: ReactNode;
  /** The secondary line ("Type · Origin"). */
  readonly subtitle?: ReactNode;
  /** A leading IconTile, status or ID. */
  readonly leading?: ReactNode;
  /** Trailing content (pills, rings, coverage icons). */
  readonly trailing?: ReactNode;
  /** Trailing meta text (age, count). */
  readonly meta?: ReactNode;
  readonly selected?: boolean;
  /** Built on an older model version: shows the "Stale" pill. */
  readonly stale?: boolean;
  /** compact 44 px · regular 56 px. */
  readonly density?: "compact" | "regular";
  /** Title line clamp (1 or 2). */
  readonly lines?: 1 | 2;
  readonly onSelect?: (event: MouseEvent<HTMLButtonElement>) => void;
  readonly className?: string;
  readonly "data-testid"?: string;
  readonly id?: string;
}

/**
 * One row of a master list, rendered as `<li>` with a full-width button (keyboard: Tab + Enter or
 * Space; screens add J/K). Selected rows use the surface colour and a semibold title, and expose
 * `aria-current`.
 */
export function ListRow({ title, subtitle, leading, trailing, meta, selected = false, stale = false, density = "regular", lines = 1, onSelect, className, id, ...rest }: ListRowProps) {
  return (
    <li className="list-none" data-testid={rest["data-testid"]}>
      <button
        type="button"
        id={id}
        onClick={onSelect}
        aria-current={selected ? "true" : undefined}
        className={cn(
          "flex w-full cursor-pointer items-center gap-3 border-b border-line px-5 py-2 text-left transition-colors duration-[120ms] ease-standard",
          density === "regular" ? "min-h-14" : "min-h-11",
          selected ? "bg-surface" : "bg-card hover:bg-subtle-row",
          className,
        )}
      >
        {leading}
        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="flex min-w-0 items-center gap-2">
            <span
              className={cn(
                "min-w-0 text-callout text-fg",
                selected ? "font-semibold" : "font-medium",
                lines === 1 ? "truncate" : "line-clamp-2",
              )}
            >
              {title}
            </span>
            {stale && <StatusPill status="stale" />}
          </span>
          {subtitle !== undefined && <span className={cn("truncate text-caption", selected ? "text-fg-3-surface" : "text-fg-3")}>{subtitle}</span>}
        </span>
        {trailing}
        {meta !== undefined && <span className={cn("shrink-0 text-caption", selected ? "text-fg-3-surface" : "text-fg-3")}>{meta}</span>}
      </button>
    </li>
  );
}

/** The row of an item whose projection failed (list `health`): the rest of the list still works. */
export function ListRowError({ itemId, onRetry, className }: { itemId: string; onRetry?: () => void; className?: string }) {
  return (
    <li className={cn("flex min-h-14 list-none items-center gap-2 border-b border-line bg-card px-5 py-2 text-callout text-blocked-fg", className)}>
      <span>{tf("ui.row.loadFailed", { id: itemId })}</span>
      {onRetry !== undefined && (
        <button type="button" onClick={onRetry} className="cursor-pointer font-semibold underline underline-offset-2">
          {t("ui.row.retry")}
        </button>
      )}
    </li>
  );
}
