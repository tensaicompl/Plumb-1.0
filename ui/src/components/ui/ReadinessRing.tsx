import { t, tf, type MessageKey } from "../../i18n";
import { cn } from "./cn";

/** The five readiness sub-scores, each 0–100 or null when not calculated. */
export type ReadinessSubScores = Readonly<
  Record<"clarity" | "completeness" | "confirmation" | "consistency" | "verifiability", number | null>
>;

const SUB_LABEL: Readonly<Record<keyof ReadinessSubScores, MessageKey>> = {
  clarity: "ui.readiness.clarity",
  completeness: "ui.readiness.completeness",
  confirmation: "ui.readiness.confirmation",
  consistency: "ui.readiness.consistency",
  verifiability: "ui.readiness.verifiability",
};

/** The ring tone by the handoff thresholds: ≥85 midnight, ≥70 slate, otherwise amber. */
export function readinessTone(value: number): "primary" | "muted" | "attention" {
  if (value >= 85) {
    return "primary";
  }
  return value >= 70 ? "muted" : "attention";
}

const STROKE = { primary: "stroke-primary", muted: "stroke-muted-ring", attention: "stroke-attention" } as const;

export interface ReadinessRingProps {
  /** 0–100 (clamped), or null when not calculated. */
  readonly value: number | null;
  /** Diameter: 20 (list rows), 72 (inspector), 96 (overview). */
  readonly size?: number;
  readonly stroke?: number;
  /** Shows the number (and "ready" at 96 px) in the centre. */
  readonly showLabel?: boolean;
  /** The five sub-scores, listed for assistive technology and in the hover title. */
  readonly subScores?: ReadinessSubScores;
  readonly className?: string;
}

/** A 0–100 % readiness ring. */
export function ReadinessRing({ value, size = 20, stroke = 3, showLabel = false, subScores, className }: ReadinessRingProps) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const pct = value === null ? null : Math.round(Math.min(100, Math.max(0, value)));
  const label = pct === null ? t("ui.readiness.notCalculated") : tf("ui.readiness.label", { value: pct });
  const subs =
    subScores === undefined
      ? null
      : (Object.keys(SUB_LABEL) as (keyof ReadinessSubScores)[]).map((key) => ({
          key,
          text: t(SUB_LABEL[key]) + " " + (subScores[key] === null ? "—" : String(subScores[key])),
        }));
  return (
    <span className={cn("relative inline-grid shrink-0 place-items-center", className)} style={{ width: size, height: size }}>
      <span
        role="img"
        aria-label={label}
        title={subs === null ? label : [label, ...subs.map((s) => s.text)].join("\n")}
        className="relative inline-grid size-full place-items-center"
      >
      <svg width={size} height={size} className="-rotate-90" aria-hidden="true">
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" strokeWidth={stroke} className="stroke-line" />
        {pct !== null && (
          <circle
            data-ring-value={pct}
            cx={size / 2}
            cy={size / 2}
            r={r}
            fill="none"
            strokeWidth={stroke}
            strokeLinecap="round"
            strokeDasharray={[(c * pct) / 100, c].join(" ")}
            className={STROKE[readinessTone(pct)]}
          />
        )}
      </svg>
      {showLabel && pct !== null && (
        <span aria-hidden="true" className="absolute inset-0 flex flex-col items-center justify-center leading-none">
          <span className={cn("font-display font-bold tracking-display text-fg", size >= 96 ? "text-headline" : "text-title-sm")}>
            {String(pct) + "%"}
          </span>
          {size >= 96 && <span className="mt-1 text-[10px] text-fg-3">{t("ui.readiness.ready")}</span>}
        </span>
      )}
      </span>
      {subs !== null && (
        <span data-subscores className="sr-only">
          {subs.map((s) => s.text).join(", ")}
        </span>
      )}
    </span>
  );
}
