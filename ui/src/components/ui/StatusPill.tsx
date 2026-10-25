import {
  Archive,
  ArrowRightLeft,
  Check,
  CircleDashed,
  CircleHelp,
  CircleSlash,
  Clock,
  GitBranch,
  Minus,
  OctagonAlert,
  RefreshCw,
  ShieldCheck,
  TriangleAlert,
  X,
  type LucideIcon,
} from "lucide-react";

import { t, type MessageKey } from "../../i18n";
import { cn } from "./cn";

/** The semantic tone of a status (never the only cue: every pill also has an icon and text). */
export type Tone = "pass" | "attention" | "blocked" | "running" | "neutral";

/** Every status a pill shows (element lifecycle, questions, scenario verdicts, confirmations). */
export const PILL_STATUSES = [
  "accepted",
  "proposed",
  "rejected",
  "superseded",
  "suspect",
  "deprecated",
  "open",
  "answered",
  "closed",
  "pass",
  "fail",
  "undecidable",
  "notRun",
  "derived",
  "confirmed",
  "stale",
  "waived",
  "running",
  "warning",
  "blocker",
] as const;

export type PillStatus = (typeof PILL_STATUSES)[number];

interface StatusDef {
  readonly tone: Tone;
  readonly icon: LucideIcon;
  readonly label: MessageKey;
}

const STATUS: Readonly<Record<PillStatus, StatusDef>> = {
  accepted: { tone: "pass", icon: Check, label: "ui.status.accepted" },
  proposed: { tone: "neutral", icon: CircleDashed, label: "ui.status.proposed" },
  rejected: { tone: "blocked", icon: X, label: "ui.status.rejected" },
  superseded: { tone: "neutral", icon: ArrowRightLeft, label: "ui.status.superseded" },
  suspect: { tone: "attention", icon: TriangleAlert, label: "ui.status.suspect" },
  deprecated: { tone: "neutral", icon: Archive, label: "ui.status.deprecated" },
  open: { tone: "neutral", icon: CircleHelp, label: "ui.status.open" },
  answered: { tone: "pass", icon: Check, label: "ui.status.answered" },
  closed: { tone: "neutral", icon: CircleSlash, label: "ui.status.closed" },
  pass: { tone: "pass", icon: Check, label: "ui.status.pass" },
  fail: { tone: "blocked", icon: X, label: "ui.status.fail" },
  undecidable: { tone: "neutral", icon: CircleHelp, label: "ui.status.undecidable" },
  notRun: { tone: "neutral", icon: Minus, label: "ui.status.notRun" },
  derived: { tone: "neutral", icon: GitBranch, label: "ui.status.derived" },
  confirmed: { tone: "pass", icon: Check, label: "ui.status.confirmed" },
  stale: { tone: "attention", icon: Clock, label: "ui.status.stale" },
  waived: { tone: "neutral", icon: ShieldCheck, label: "ui.status.waived" },
  running: { tone: "running", icon: RefreshCw, label: "ui.status.running" },
  warning: { tone: "attention", icon: TriangleAlert, label: "ui.status.warning" },
  blocker: { tone: "blocked", icon: OctagonAlert, label: "ui.status.blocker" },
};

/** Tone classes: tinted background + darker text of the same hue. */
export const TONE_PILL: Readonly<Record<Tone, string>> = {
  pass: "bg-pass-tint text-pass-fg",
  attention: "bg-attention-tint text-attention-fg",
  blocked: "bg-blocked-tint text-blocked-fg",
  running: "bg-running-tint text-running-fg",
  neutral: "bg-neutral-tint text-neutral-fg",
};

export interface StatusPillProps {
  readonly status: PillStatus;
  /** Replaces the catalogue label (e.g. a server value shown verbatim). */
  readonly label?: string;
  /** Overrides the status's default tone (e.g. a neutral "Confirmed" in Scenarios). */
  readonly tone?: Tone;
  readonly className?: string;
}

/** A short status label with an icon, toned by meaning: 22 px pill, 11 px semibold text. */
export function StatusPill({ status, label, tone, className }: StatusPillProps) {
  const def = STATUS[status];
  const Icon = def.icon;
  return (
    <span
      data-status={status}
      className={cn(
        "inline-flex h-[22px] shrink-0 items-center gap-1 whitespace-nowrap rounded-pill px-2 text-eyebrow font-semibold",
        TONE_PILL[tone ?? def.tone],
        className,
      )}
    >
      <Icon size={12} strokeWidth={1.6} aria-hidden="true" />
      {label ?? t(def.label)}
    </span>
  );
}
