import type { components } from "../../api/schema";
import { t, type MessageKey } from "../../i18n";
import { cn } from "./cn";

export type GateName = components["schemas"]["GateName"];

/** A gate light's state: pass, fail (blocked), stale (outdated), not evaluated, running. */
export type GateLightState = "pass" | "fail" | "stale" | "not_evaluated" | "running";

const GATE_NAME: Readonly<Record<GateName, MessageKey>> = {
  I0: "ui.gateName.I0",
  F1: "ui.gateName.F1",
  F2: "ui.gateName.F2",
  F3: "ui.gateName.F3",
  F4: "ui.gateName.F4",
};

/** The plain-language name of a gate ("Evidence", "Requirements"…). */
export function gateName(gate: GateName): string {
  return t(GATE_NAME[gate]);
}

const STATE_LABEL: Readonly<Record<GateLightState, MessageKey>> = {
  pass: "ui.gate.pass",
  fail: "ui.gate.fail",
  stale: "ui.gate.stale",
  not_evaluated: "ui.gate.notEvaluated",
  running: "ui.gate.running",
};

/** The catalogue text of a gate state ("Passes", "Blocked", "Outdated"…). */
export function gateStateLabel(state: GateLightState): string {
  return t(STATE_LABEL[state]);
}

const DOT: Readonly<Record<GateLightState, string>> = {
  pass: "bg-pass",
  fail: "bg-blocked",
  // Outdated is a hollow ring: data exists but no longer describes the current model.
  stale: "border-2 border-stale",
  not_evaluated: "border border-dashed border-line-strong",
  running: "bg-running animate-pulse-dot",
};

const TEXT: Readonly<Record<GateLightState, string>> = {
  pass: "text-pass-fg",
  fail: "text-blocked-fg",
  stale: "text-neutral-fg",
  not_evaluated: "text-neutral-fg",
  running: "text-running-fg",
};

/** The gate's dot alone (decorative; pair it with the state text). */
export function GateDot({ state, size = 10 }: { state: GateLightState; size?: 10 | 12 }) {
  return (
    <span
      aria-hidden="true"
      data-gate-dot={state}
      className={cn("inline-block shrink-0 rounded-pill", size === 10 ? "size-2.5" : "size-3", DOT[state])}
    />
  );
}

export interface GateLightProps {
  readonly gate: GateName;
  readonly state: GateLightState;
  /** Replaces the plain gate name. */
  readonly label?: string;
  /** Shows the gate code (I0…F4) small and secondary (default true). */
  readonly showCode?: boolean;
  /** One line explaining a non-passing state ("2 blocking issues"). */
  readonly detail?: string;
  readonly size?: 10 | 12;
  readonly className?: string;
}

/** A gate with its dot, plain name, secondary code and state text (status is never colour-only). */
export function GateLight({ gate, state, label, showCode = true, detail, size = 10, className }: GateLightProps) {
  return (
    <span data-gate={gate} data-gate-state={state} className={cn("inline-flex min-w-0 flex-col gap-1", className)}>
      <span className="inline-flex items-center gap-2">
        <GateDot state={state} size={size} />
        <span className="text-body font-semibold text-fg">{label ?? gateName(gate)}</span>
        {showCode && <span className="ml-auto pl-2 font-mono text-eyebrow text-fg-4">{gate}</span>}
      </span>
      <span className={cn("text-caption font-medium", TEXT[state])}>
        {gateStateLabel(state)}
        {detail !== undefined && " · " + detail}
      </span>
    </span>
  );
}
