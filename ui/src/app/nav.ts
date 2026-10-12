import type { components } from "../api/schema";
import type { MessageKey } from "../i18n";

/** A pilot gate identifier (technical value, never translated). */
export type GateName = components["schemas"]["GateName"];

/** The pilot gates, in their fixed order. */
export const PILOT_GATES: readonly GateName[] = ["I0", "F1", "F2", "F3", "F4"];

/** One navigable destination with its stable final href. */
export interface NavLink {
  readonly id: string;
  readonly label: MessageKey;
  readonly href: string;
}

/** A top-level navigation entry: a link, a group of links, or the gates entry. */
export type NavEntry =
  | { readonly kind: "link"; readonly link: NavLink }
  | { readonly kind: "group"; readonly id: string; readonly label: MessageKey; readonly links: readonly NavLink[] }
  | { readonly kind: "gates"; readonly link: NavLink; readonly gates: readonly GateName[] };

/**
 * The frozen pilot navigation (v3 U1.1, Hotfix 071), in display order. Every destination carries
 * its final href; a page that is not implemented yet is registered by its own later task, never
 * by a placeholder route.
 */
export const PILOT_NAVIGATION: readonly NavEntry[] = [
  { kind: "link", link: { id: "home", label: "nav.home", href: "/" } },
  { kind: "link", link: { id: "requirements", label: "nav.requirements", href: "/requirements" } },
  {
    kind: "group",
    id: "model",
    label: "nav.model",
    links: [
      { id: "glossary", label: "nav.glossary", href: "/model/glossary" },
      { id: "sentences", label: "nav.sentences", href: "/model/sentences" },
      { id: "entities", label: "nav.entities", href: "/model/entities" },
      { id: "processes", label: "nav.processes", href: "/model/processes" },
      { id: "scenarios", label: "nav.scenarios", href: "/model/scenarios" },
      { id: "findings", label: "nav.findings", href: "/model/findings" },
      { id: "rules", label: "nav.rules", href: "/model/rules" },
      { id: "calculations", label: "nav.calculations", href: "/model/calculations" },
    ],
  },
  { kind: "link", link: { id: "questions", label: "nav.questions", href: "/questions" } },
  { kind: "link", link: { id: "decisions", label: "nav.decisions", href: "/decisions" } },
  { kind: "gates", link: { id: "gates", label: "nav.gates", href: "/gates" }, gates: PILOT_GATES },
];
