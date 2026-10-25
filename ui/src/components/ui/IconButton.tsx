import type { ComponentPropsWithRef, ReactNode } from "react";

import { cn } from "./cn";

export interface IconButtonProps extends Omit<ComponentPropsWithRef<"button">, "children"> {
  /** The accessible name (also the tooltip text through `title`). */
  readonly label: string;
  readonly icon: ReactNode;
  readonly size?: 30 | 34;
  readonly variant?: "ghost" | "outline";
  /** A toggle's state (e.g. the Assistant panel is open). */
  readonly pressed?: boolean;
}

/** A square icon-only button: 34×34 (toolbar) or 30×30, radius 8. */
export function IconButton({ label, icon, size = 34, variant = "ghost", pressed, className, type, ...rest }: IconButtonProps) {
  return (
    <button
      type={type ?? "button"}
      aria-label={label}
      title={label}
      aria-pressed={pressed === undefined ? undefined : pressed}
      className={cn(
        "inline-flex shrink-0 cursor-pointer items-center justify-center rounded-control text-fg-2 transition-colors duration-[120ms] ease-standard",
        "hover:bg-surface disabled:cursor-not-allowed disabled:opacity-40",
        size === 34 ? "size-[34px]" : "size-[30px]",
        variant === "outline" ? "border border-line bg-card" : "bg-transparent",
        pressed === true && "bg-surface text-fg",
        className,
      )}
      {...rest}
    >
      {icon}
    </button>
  );
}
