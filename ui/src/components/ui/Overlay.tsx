import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import * as RadixTooltip from "@radix-ui/react-tooltip";
import type { ReactElement, ReactNode } from "react";

import { cn } from "./cn";
import { Eyebrow } from "./primitives";

// ------------------------------------------------------------------ Tooltip

export interface TooltipProps {
  readonly content: ReactNode;
  /** One focusable element (the trigger). */
  readonly children: ReactElement;
  readonly side?: "top" | "right" | "bottom" | "left";
}

/** A short hint on hover and keyboard focus (midnight, 12 px, radius 8). */
export function Tooltip({ content, children, side = "top" }: TooltipProps) {
  return (
    <RadixTooltip.Provider delayDuration={300} skipDelayDuration={100}>
      <RadixTooltip.Root>
        <RadixTooltip.Trigger asChild>{children}</RadixTooltip.Trigger>
        <RadixTooltip.Portal>
          <RadixTooltip.Content
            side={side}
            sideOffset={6}
            className="z-[80] max-w-[280px] animate-fade-in rounded-control bg-toast px-2.5 py-1.5 text-caption font-medium text-white shadow-md"
          >
            {content}
          </RadixTooltip.Content>
        </RadixTooltip.Portal>
      </RadixTooltip.Root>
    </RadixTooltip.Provider>
  );
}

// ------------------------------------------------------------------ Menu

export interface MenuItem {
  readonly id: string;
  readonly label: string;
  /** Right-aligned status text ("Done", "Needs review"). */
  readonly meta?: string;
  readonly icon?: ReactNode;
  readonly disabled?: boolean;
  readonly destructive?: boolean;
  readonly onSelect: () => void;
}

export interface MenuProps {
  /** The trigger button (opened with click, Enter, Space or ArrowDown). */
  readonly trigger: ReactElement;
  readonly items: readonly MenuItem[];
  /** An eyebrow heading at the top ("Run a step"). */
  readonly title?: string;
  readonly align?: "start" | "end";
  /** Menu width in px (240 by default). */
  readonly width?: number;
  readonly open?: boolean;
  readonly onOpenChange?: (open: boolean) => void;
}

/** A dropdown menu: 240 px, radius 12, --shadow-lg, padding 6; items 34 px radius 8. */
export function Menu({ trigger, items, title, align = "end", width = 240, open, onOpenChange }: MenuProps) {
  return (
    <DropdownMenu.Root open={open} onOpenChange={onOpenChange} modal={false}>
      <DropdownMenu.Trigger asChild>{trigger}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align={align}
          sideOffset={6}
          style={{ width }}
          className="z-[40] animate-overlay-in rounded-tile border border-line bg-card p-1.5 shadow-lg"
        >
          {title !== undefined && (
            <DropdownMenu.Label asChild>
              <Eyebrow className="px-2.5 pb-1.5 pt-2">{title}</Eyebrow>
            </DropdownMenu.Label>
          )}
          {items.map((item) => (
            <DropdownMenu.Item
              key={item.id}
              disabled={item.disabled}
              onSelect={item.onSelect}
              className={cn(
                "flex h-[34px] cursor-pointer select-none items-center gap-2 rounded-control px-2.5 text-footnote font-medium outline-none",
                "data-[highlighted]:bg-surface data-[disabled]:cursor-not-allowed data-[disabled]:opacity-40",
                item.destructive === true ? "text-blocked-fg" : "text-fg",
              )}
            >
              {item.icon}
              <span className="min-w-0 flex-1 truncate">{item.label}</span>
              {item.meta !== undefined && <span className="shrink-0 text-eyebrow text-fg-3">{item.meta}</span>}
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
