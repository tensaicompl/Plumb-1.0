import { FileText, FlaskConical, House, Inbox, LayoutGrid, List, ShieldCheck, type LucideIcon } from "lucide-react";

import type { components } from "../api/schema";
import type { MessageKey } from "../i18n";

/** A pilot gate identifier (technical value, never translated). */
export type GateName = components["schemas"]["GateName"];

/** The pilot gates, in their fixed order. */
export const PILOT_GATES: readonly GateName[] = ["I0", "F1", "F2", "F3", "F4"];

export type DestinationId = "overview" | "inbox" | "requirements" | "model" | "scenarios" | "quality" | "sources";

/** One sidebar destination (spec §3): ⌘1–7 jump to them in this order. */
export interface Destination {
  readonly id: DestinationId;
  readonly label: MessageKey;
  /** The destination's landing URL. */
  readonly href: string;
  /** Every URL under this prefix belongs to the destination. */
  readonly prefix: string;
  readonly icon: LucideIcon;
}

/** The seven sidebar destinations, in display and shortcut order. */
export const DESTINATIONS: readonly Destination[] = [
  { id: "overview", label: "nav.overview", href: "/overview", prefix: "/overview", icon: House },
  { id: "inbox", label: "nav.inbox", href: "/inbox", prefix: "/inbox", icon: Inbox },
  { id: "requirements", label: "nav.requirements", href: "/requirements", prefix: "/requirements", icon: List },
  { id: "model", label: "nav.model", href: "/model/glossary", prefix: "/model", icon: LayoutGrid },
  { id: "scenarios", label: "nav.scenarios", href: "/scenarios", prefix: "/scenarios", icon: FlaskConical },
  { id: "quality", label: "nav.quality", href: "/quality", prefix: "/quality", icon: ShieldCheck },
  { id: "sources", label: "nav.sources", href: "/sources", prefix: "/sources", icon: FileText },
];

/** The destination a pathname belongs to (null for unknown routes). */
export function destinationOf(pathname: string): DestinationId | null {
  const match = DESTINATIONS.find((d) => pathname === d.prefix || pathname.startsWith(d.prefix + "/"));
  return match?.id ?? null;
}

/** `page` on the destination's landing URL, `location` anywhere else inside it. */
export function navCurrent(destination: Destination, pathname: string): "page" | "location" | undefined {
  if (pathname === destination.href) {
    return "page";
  }
  return destinationOf(pathname) === destination.id ? "location" : undefined;
}
