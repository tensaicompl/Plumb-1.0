import type { ComponentPropsWithRef, ReactNode } from "react";

import { cn } from "./cn";

/**
 * Button variants: `primary` midnight (the default action), `accent` violet (the ONE primary
 * action of a surface), `outline`, `destructive` (reject, discard), `ghost`, and `inverse` (the
 * white pill on a dark hero card).
 */
export type ButtonVariant = "primary" | "accent" | "outline" | "destructive" | "ghost" | "inverse";

/** Pill heights from the handoff: 30 (inline), 34 (toolbar), 38 (sheets, action bars), 44 (hero). */
export type ButtonSize = 30 | 34 | 38 | 44;

const VARIANT: Readonly<Record<ButtonVariant, string>> = {
  primary: "bg-primary text-on-primary hover:bg-primary-hover",
  accent: "bg-accent text-on-accent hover:bg-accent-hover active:bg-accent-pressed",
  outline: "border border-line bg-card text-fg hover:bg-surface",
  destructive: "bg-blocked text-white hover:bg-blocked-hover",
  ghost: "bg-transparent text-fg-2 hover:bg-surface",
  inverse: "bg-on-hero text-hero hover:bg-surface",
};

const SIZE: Readonly<Record<ButtonSize, string>> = {
  30: "h-[30px] px-3 text-caption",
  34: "h-[34px] px-4 text-footnote",
  38: "h-[38px] px-4 text-footnote",
  44: "h-11 px-[22px] text-body",
};

/** The classes of a button-styled element (for anchors that look like buttons). */
export function buttonClasses({ variant = "primary", size = 38, className }: { variant?: ButtonVariant; size?: ButtonSize; className?: string }): string {
  return cn(
    "inline-flex shrink-0 cursor-pointer select-none items-center justify-center gap-2 whitespace-nowrap rounded-pill font-semibold leading-none",
    "transition-colors duration-[120ms] ease-standard disabled:cursor-not-allowed disabled:opacity-40",
    VARIANT[variant],
    SIZE[size],
    className,
  );
}

export interface ButtonProps extends ComponentPropsWithRef<"button"> {
  readonly variant?: ButtonVariant;
  readonly size?: ButtonSize;
  /** A leading icon (16 px Lucide). */
  readonly icon?: ReactNode;
  readonly trailingIcon?: ReactNode;
  /** A keyboard hint shown after the label (e.g. ⌘↩). */
  readonly shortcut?: string;
  /** A write in flight: the button is disabled and busy. */
  readonly pending?: boolean;
}

/** A pill button (always `type="button"` unless a type is given). */
export function Button({ variant, size, icon, trailingIcon, shortcut, pending = false, className, children, disabled, type, ...rest }: ButtonProps) {
  return (
    <button
      type={type ?? "button"}
      className={buttonClasses({ variant, size, className })}
      disabled={disabled === true || pending}
      aria-busy={pending ? true : undefined}
      {...rest}
    >
      {icon}
      {children}
      {trailingIcon}
      {shortcut !== undefined && <span className="ml-1 font-mono text-eyebrow font-medium opacity-60">{shortcut}</span>}
    </button>
  );
}
