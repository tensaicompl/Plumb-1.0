import { t, type MessageKey } from "../i18n";

/**
 * The presentation states a badge can show. PASS and FAIL are also the overall GateResult values;
 * WARN, WAIVED and ERROR exist only at rule level, so the caller decides what each badge stands for.
 */
export type BadgeStatus = "PASS" | "FAIL" | "WARN" | "WAIVED" | "ERROR";

export const BADGE_STATUSES: readonly BadgeStatus[] = ["PASS", "FAIL", "WARN", "WAIVED", "ERROR"];

// Shape cues are decorative; the exact status text is always rendered.
const GLYPH: Readonly<Record<BadgeStatus, string>> = {
  PASS: "✓",
  FAIL: "✗",
  WARN: "!",
  WAIVED: "−",
  ERROR: "⚠",
};

const STYLE: Readonly<Record<BadgeStatus, string>> = {
  PASS: "border-green-700 text-green-800",
  FAIL: "border-red-700 text-red-800",
  WARN: "border-amber-700 text-amber-800",
  WAIVED: "border-slate-600 text-slate-700",
  ERROR: "border-purple-700 text-purple-800",
};

const DESCRIPTION: Readonly<Record<BadgeStatus, MessageKey>> = {
  PASS: "gateBadge.pass",
  FAIL: "gateBadge.fail",
  WARN: "gateBadge.warn",
  WAIVED: "gateBadge.waived",
  ERROR: "gateBadge.error",
};

/**
 * A categorical status badge: the exact status text plus a decorative shape cue, never a score,
 * percentage or progress value. `label` (for example a gate ID) is shown before the status.
 */
export function GateBadge({ status, label }: { status: BadgeStatus; label?: string }) {
  return (
    <span
      className={"inline-flex items-center gap-1 rounded border px-2 py-0.5 text-xs font-semibold " + STYLE[status]}
      data-status={status}
      title={t(DESCRIPTION[status])}
    >
      <span aria-hidden="true">{GLYPH[status]}</span>
      {label !== undefined && <span className="font-mono">{label}</span>}
      <span data-testid="badge-status">{status}</span>
    </span>
  );
}
