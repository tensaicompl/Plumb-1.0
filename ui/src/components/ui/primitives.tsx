import {
  ArrowRightLeft,
  Database,
  Gauge,
  Lock,
  Plug,
  Ruler,
  Scale,
  Settings2,
  SquareFunction,
  type LucideIcon,
} from "lucide-react";
import type { ComponentPropsWithoutRef, ElementType, ReactNode } from "react";

import ensoUrl from "../../assets/enso.svg";
import { t, type MessageKey } from "../../i18n";
import { cn } from "./cn";

// ------------------------------------------------------------------ Eyebrow

export interface EyebrowProps {
  readonly children: ReactNode;
  /** `onDark` for labels on midnight hero surfaces. */
  readonly tone?: "default" | "onDark";
  readonly as?: "span" | "p" | "h2" | "h3" | "div";
  readonly className?: string;
  readonly id?: string;
}

/** An uppercase section label: Inter 11 px / 600, tracking 0.14em. */
export function Eyebrow({ children, tone = "default", as: Tag = "span", className, id }: EyebrowProps) {
  return (
    <Tag
      id={id}
      className={cn(
        "block font-sans text-eyebrow font-semibold uppercase leading-none tracking-eyebrow",
        tone === "onDark" ? "text-hero-sub" : "text-fg-3",
        className,
      )}
    >
      {children}
    </Tag>
  );
}

// ------------------------------------------------------------------ MonoId and Kbd

export interface MonoIdProps {
  readonly children: string;
  readonly size?: 11 | 12;
  readonly tone?: "muted" | "default";
  readonly title?: string;
  readonly className?: string;
}

/** An identifier or code in JetBrains Mono, selectable for copying. */
export function MonoId({ children, size = 12, tone = "muted", title, className }: MonoIdProps) {
  return (
    <span
      title={title}
      className={cn(
        "select-text whitespace-nowrap font-mono",
        size === 12 ? "text-caption" : "text-eyebrow",
        tone === "muted" ? "text-fg-3" : "text-fg",
        className,
      )}
    >
      {children}
    </span>
  );
}

/** A keyboard hint (⌘K, esc): mono 11 px, placeholder grey. */
export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return <kbd className={cn("font-mono text-eyebrow font-medium text-fg-3", className)}>{children}</kbd>;
}

// ------------------------------------------------------------------ KindTag

/** The nine requirement classes. */
export const KINDS = ["functional", "quality", "interface", "data", "security", "operational", "compliance", "transition", "constraint"] as const;

export type RequirementKind = (typeof KINDS)[number];

const KIND: Readonly<Record<RequirementKind, { label: MessageKey; icon: LucideIcon }>> = {
  functional: { label: "ui.kind.functional", icon: SquareFunction },
  quality: { label: "ui.kind.quality", icon: Gauge },
  interface: { label: "ui.kind.interface", icon: Plug },
  data: { label: "ui.kind.data", icon: Database },
  security: { label: "ui.kind.security", icon: Lock },
  operational: { label: "ui.kind.operational", icon: Settings2 },
  compliance: { label: "ui.kind.compliance", icon: Scale },
  transition: { label: "ui.kind.transition", icon: ArrowRightLeft },
  constraint: { label: "ui.kind.constraint", icon: Ruler },
};

function isKind(value: string): value is RequirementKind {
  return (KINDS as readonly string[]).includes(value);
}

/** A requirement class tag (radius 4, surface, 11 px) with an icon per class; unknown kinds show verbatim. */
export function KindTag({ kind, withIcon = true, className }: { kind: string; withIcon?: boolean; className?: string }) {
  const key = kind.toLowerCase();
  const def = isKind(key) ? KIND[key] : null;
  const Icon = def?.icon;
  return (
    <span
      data-kind={key}
      className={cn("inline-flex h-5 shrink-0 items-center gap-1 whitespace-nowrap rounded-tag bg-surface px-1.5 text-eyebrow font-medium text-fg-2", className)}
    >
      {withIcon && Icon !== undefined && <Icon size={12} strokeWidth={1.6} aria-hidden="true" />}
      {def === null ? kind : t(def.label)}
    </span>
  );
}

// ------------------------------------------------------------------ IconTile

/** A decorative icon tile: 30 px radius 8 on the surface colour (white when its row is selected). */
export function IconTile({ icon, size = 30, selected = false, className }: { icon: ReactNode; size?: 28 | 30 | 56; selected?: boolean; className?: string }) {
  return (
    <span
      aria-hidden="true"
      data-icon-tile
      className={cn(
        "inline-grid shrink-0 place-items-center text-fg-2",
        size === 56 ? "size-14 rounded-pill" : size === 30 ? "size-[30px] rounded-control" : "size-7 rounded-control",
        selected ? "bg-card" : "bg-surface",
        className,
      )}
    >
      {icon}
    </span>
  );
}

// ------------------------------------------------------------------ Card

type CardTag = "div" | "section" | "article" | "li" | "aside";

export type CardProps = ComponentPropsWithoutRef<"div"> & {
  readonly as?: CardTag;
  /** none · sm 16 · md 24 (default) · lg 32 (hero). */
  readonly padding?: "none" | "sm" | "md" | "lg";
  /** card 16 (default) · tile 12. */
  readonly radius?: "card" | "tile";
  readonly elevation?: "xs" | "sm";
  /** Hover lifts one shadow level and darkens the border one step. */
  readonly interactive?: boolean;
  readonly selected?: boolean;
};

const PADDING = { none: "", sm: "p-4", md: "p-6", lg: "p-8" } as const;

/** A white card on the off-white page: 1 px border, radius 16, --shadow-sm. */
export function Card({ as = "div", padding = "md", radius = "card", elevation = "sm", interactive = false, selected = false, className, ...rest }: CardProps) {
  const Tag = as as ElementType;
  return (
    <Tag
      data-card
      className={cn(
        "border bg-card",
        radius === "card" ? "rounded-card" : "rounded-tile",
        elevation === "sm" ? "shadow-sm" : "shadow-xs",
        PADDING[padding],
        selected ? "border-primary shadow-md" : "border-line",
        interactive && "transition-[box-shadow,border-color] duration-[120ms] ease-standard hover:border-line-strong hover:shadow-md",
        className,
      )}
      {...rest}
    />
  );
}

// ------------------------------------------------------------------ Avatar and the enso

/** Up to two initials of a display name ("Marta Nowak" → "MN", "alice" → "AL"). */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) {
    return "?";
  }
  if (words.length === 1) {
    return (words[0] ?? "").slice(0, 2).toUpperCase();
  }
  return ((words[0]?.[0] ?? "") + (words[1]?.[0] ?? "")).toUpperCase();
}

/** A midnight initials circle (32 px footer, 28 px lists). `decorative` when the name is shown next to it. */
export function Avatar({ name, size = 32, decorative = false, className }: { name: string; size?: 28 | 32; decorative?: boolean; className?: string }) {
  return (
    <span
      role={decorative ? undefined : "img"}
      aria-label={decorative ? undefined : name}
      aria-hidden={decorative ? true : undefined}
      className={cn(
        "inline-grid shrink-0 select-none place-items-center rounded-pill bg-primary font-sans font-bold text-on-primary",
        size === 32 ? "size-8 text-caption" : "size-7 text-eyebrow",
        className,
      )}
    >
      {initials(name)}
    </span>
  );
}

/**
 * The enso, the brand's single ornament (never redrawn): the black brush mark, inverted to white
 * on dark appearance. Decorative unless `label` is given.
 */
export function EnsoMark({ size = 40, label, className }: { size?: number; label?: string; className?: string }) {
  return (
    <img
      data-enso
      src={ensoUrl}
      width={size}
      height={size}
      alt={label ?? ""}
      draggable={false}
      className={cn("shrink-0 select-none [filter:var(--logo-filter)]", className)}
    />
  );
}
