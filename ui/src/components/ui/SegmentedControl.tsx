import { ChevronDown } from "lucide-react";
import { useRef, type ComponentPropsWithRef, type KeyboardEvent, type MouseEvent } from "react";

import { cn, isPlainClick } from "./cn";

const TRACK = "inline-flex w-max items-center gap-0.5 rounded-field bg-surface p-[3px]";
const SEGMENT =
  "inline-flex h-[30px] cursor-pointer items-center justify-center whitespace-nowrap rounded-control px-3.5 text-footnote font-semibold transition-[background-color,color,box-shadow] duration-[120ms] ease-standard";
const ACTIVE = "bg-card text-fg shadow-segment";
const INACTIVE = "bg-transparent text-fg-3-surface hover:text-fg";

export interface SegmentOption<T extends string> {
  readonly value: T;
  readonly label: string;
  readonly disabled?: boolean;
}

export interface SegmentedControlProps<T extends string> {
  /** The group's accessible name. */
  readonly label: string;
  readonly options: readonly SegmentOption<T>[];
  readonly value: T;
  readonly onChange: (value: T) => void;
  readonly className?: string;
}

/**
 * A segmented choice (Language, Appearance, Yes/No answers): a radio group with one tab stop;
 * arrow keys, Home and End move and select, skipping disabled segments. Track surface radius 10
 * padding 3; segment 30 px radius 8; the active segment is white with a soft shadow.
 */
export function SegmentedControl<T extends string>({ label, options, value, onChange, className }: SegmentedControlProps<T>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const enabled = options.map((option, index) => ({ option, index })).filter(({ option }) => option.disabled !== true);
  const move = (event: KeyboardEvent<HTMLButtonElement>, from: number) => {
    const position = enabled.findIndex(({ index }) => index === from);
    let target: number | undefined;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      target = enabled[(position + 1) % enabled.length]?.index;
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      target = enabled[(position - 1 + enabled.length) % enabled.length]?.index;
    } else if (event.key === "Home") {
      target = enabled[0]?.index;
    } else if (event.key === "End") {
      target = enabled[enabled.length - 1]?.index;
    }
    if (target === undefined) {
      return;
    }
    event.preventDefault();
    const option = options[target];
    if (option !== undefined) {
      onChange(option.value);
      refs.current[target]?.focus();
    }
  };
  return (
    <div role="radiogroup" aria-label={label} className={cn(TRACK, className)}>
      {options.map((option, index) => {
        const checked = option.value === value;
        return (
          <button
            key={option.value}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            disabled={option.disabled}
            tabIndex={checked ? 0 : -1}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => move(event, index)}
            className={cn(SEGMENT, checked ? ACTIVE : INACTIVE, "disabled:cursor-not-allowed disabled:opacity-40")}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

export interface SegmentLink {
  readonly href: string;
  readonly label: string;
  readonly active: boolean;
}

/** Navigation styled as a segmented control (Model segments, Quality tabs): links with aria-current. */
export function SegmentedLinks({ label, items, onNavigate, className }: { label: string; items: readonly SegmentLink[]; onNavigate: (href: string) => void; className?: string }) {
  const onClick = (event: MouseEvent<HTMLAnchorElement>, href: string) => {
    if (!isPlainClick(event)) {
      return;
    }
    event.preventDefault();
    onNavigate(href);
  };
  return (
    <nav aria-label={label} className={cn(TRACK, className)}>
      {items.map((item) => (
        <a
          key={item.href}
          href={item.href}
          aria-current={item.active ? "page" : undefined}
          onClick={(event) => onClick(event, item.href)}
          className={cn(SEGMENT, "no-underline", item.active ? ACTIVE : INACTIVE)}
        >
          {item.label}
        </a>
      ))}
    </nav>
  );
}

// ------------------------------------------------------------------ FilterChip

export interface FilterChipProps extends Omit<ComponentPropsWithRef<"button">, "children"> {
  readonly label: string;
  readonly active?: boolean;
  readonly count?: number;
  readonly onToggle?: () => void;
  /** The chip opens a Menu (Kind, Status): a chevron instead of a pressed state. */
  readonly menu?: boolean;
  readonly className?: string;
}

/**
 * A filter chip: 30 px outline pill; active chips have a midnight border. A toggle chip is
 * pressed when active; a `menu` chip is a Menu trigger (button props pass through).
 */
export function FilterChip({ label, active = false, count, onToggle, menu = false, className, ...rest }: FilterChipProps) {
  return (
    <button
      type="button"
      aria-pressed={menu ? undefined : active}
      onClick={onToggle}
      {...rest}
      className={cn(
        "inline-flex h-[30px] cursor-pointer items-center gap-1.5 whitespace-nowrap rounded-pill border px-3 text-footnote transition-colors duration-[120ms] ease-standard",
        active ? "border-primary bg-surface font-semibold text-fg" : "border-line bg-card font-medium text-fg-2 hover:bg-surface",
        className,
      )}
    >
      {label}
      {count !== undefined && <span className="font-mono text-eyebrow opacity-70">{count}</span>}
      {menu && <ChevronDown size={14} strokeWidth={1.6} aria-hidden="true" />}
    </button>
  );
}
