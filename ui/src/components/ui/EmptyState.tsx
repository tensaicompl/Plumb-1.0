import { Check } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";

import { t } from "../../i18n";
import { cn } from "./cn";
import { EnsoMark } from "./primitives";

export interface EmptyStateProps {
  /** One calm sentence: what is (not) here. */
  readonly title: string;
  readonly description?: string;
  /** The one next action (usually a Button). */
  readonly action?: ReactNode;
  /** The enso (default), the "all caught up" check, none, or a custom node. */
  readonly visual?: "enso" | "check" | "none" | ReactNode;
  readonly ensoSize?: number;
  readonly headingLevel?: 2 | 3;
  readonly className?: string;
}

/** An empty screen or panel: visual, one sentence, one action. */
export function EmptyState({ title, description, action, visual = "enso", ensoSize = 64, headingLevel = 2, className }: EmptyStateProps) {
  const Heading = headingLevel === 2 ? "h2" : "h3";
  let art: ReactNode = visual;
  if (visual === "enso") {
    art = <EnsoMark size={ensoSize} />;
  } else if (visual === "check") {
    art = (
      <span data-empty-check aria-hidden="true" className="grid size-14 place-items-center rounded-pill bg-pass-tint text-pass-fg">
        <Check size={28} strokeWidth={1.6} />
      </span>
    );
  } else if (visual === "none") {
    art = null;
  }
  return (
    <div className={cn("flex flex-col items-center justify-center gap-3 px-6 py-12 text-center", className)}>
      {art}
      <Heading className="m-0 font-display text-title-lg font-semibold tracking-display text-fg">{title}</Heading>
      {description !== undefined && <p className="m-0 max-w-[420px] text-body text-fg-3">{description}</p>}
      {action !== undefined && <div className="mt-2">{action}</div>}
    </div>
  );
}

// ------------------------------------------------------------------ Skeleton

type Length = number | string;

export interface SkeletonProps {
  readonly width?: Length;
  readonly height?: Length;
  readonly radius?: "control" | "pill" | "card" | "full";
  readonly className?: string;
}

const RADIUS = { control: "rounded-control", pill: "rounded-pill", card: "rounded-card", full: "rounded-full" } as const;

const px = (value: Length | undefined) => (typeof value === "number" ? `${value}px` : value);

/** A shimmering placeholder block (decorative; wrap groups in SkeletonGroup to announce loading). */
export function Skeleton({ width, height = 12, radius = "control", className }: SkeletonProps) {
  const style: CSSProperties = { width: px(width), height: px(height) };
  return (
    <span
      aria-hidden="true"
      data-skeleton
      style={style}
      className={cn(
        "block animate-shimmer bg-surface bg-[linear-gradient(90deg,transparent,var(--shimmer),transparent)] bg-no-repeat [background-size:200px_100%]",
        RADIUS[radius],
        className,
      )}
    />
  );
}

/** A loading region: announces "Loading" once and hides its placeholders from assistive technology. */
export function SkeletonGroup({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div role="status" aria-label={t("ui.loading")} aria-busy="true" className={className}>
      {children}
    </div>
  );
}

/** List-row placeholders matching a 56 px row (icon tile, title, subtitle, meta). */
export function SkeletonList({ rows = 5, className }: { rows?: number; className?: string }) {
  return (
    <SkeletonGroup className={className}>
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex min-h-14 items-center gap-3 border-b border-line px-5 py-2">
          <Skeleton width={30} height={30} />
          <div className="flex flex-1 flex-col gap-2">
            <Skeleton width="60%" height={12} />
            <Skeleton width="35%" height={10} />
          </div>
          <Skeleton width={36} height={10} />
        </div>
      ))}
    </SkeletonGroup>
  );
}
