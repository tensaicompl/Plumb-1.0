import type { components } from "../../api/schema";
import { t, type MessageKey } from "../../i18n";

export type ScenarioRun = components["schemas"]["ScenarioRun"];

/** The execution steps of the plumb-sim trace (plumb_sim::ExecutionTraceStep, snake_case). */
const EXECUTION_STEPS: Readonly<Record<string, MessageKey>> = {
  materialization_started: "scenarios.trace.materialization_started",
  semantic_resolved: "scenarios.trace.semantic_resolved",
  semantic_missing: "scenarios.trace.semantic_missing",
  given_materialized: "scenarios.trace.given_materialized",
  footprint_declared: "scenarios.trace.footprint_declared",
  performer_recorded: "scenarios.trace.performer_recorded",
  authorization_checked: "scenarios.trace.authorization_checked",
  precondition_evaluated: "scenarios.trace.precondition_evaluated",
  rule_evaluated: "scenarios.trace.rule_evaluated",
  rule_action_not_executed: "scenarios.trace.rule_action_not_executed",
  decision_table_evaluated: "scenarios.trace.decision_table_evaluated",
  calculation_evaluated: "scenarios.trace.calculation_evaluated",
  outcome_selected: "scenarios.trace.outcome_selected",
  transition_trigger_recorded: "scenarios.trace.transition_trigger_recorded",
  guard_evaluated: "scenarios.trace.guard_evaluated",
  transition_applied: "scenarios.trace.transition_applied",
  transition_effects_recorded: "scenarios.trace.transition_effects_recorded",
  event_occurred: "scenarios.trace.event_occurred",
  postcondition_evaluated: "scenarios.trace.postcondition_evaluated",
  invariant_evaluated: "scenarios.trace.invariant_evaluated",
  event_emitted: "scenarios.trace.event_emitted",
  execution_completed: "scenarios.trace.execution_completed",
};

/** The comparison steps of the plumb-sim trace (plumb_sim::ComparisonTraceStep, snake_case). */
const COMPARISON_STEPS: Readonly<Record<string, MessageKey>> = {
  comparison_started: "scenarios.trace.comparison_started",
  expected_value_resolved: "scenarios.trace.expected_value_resolved",
  expected_semantic_missing: "scenarios.trace.expected_semantic_missing",
  attribute_compared: "scenarios.trace.attribute_compared",
  state_compared: "scenarios.trace.state_compared",
  outcomes_compared: "scenarios.trace.outcomes_compared",
  events_compared: "scenarios.trace.events_compared",
  rule_compared: "scenarios.trace.rule_compared",
  decision_output_compared: "scenarios.trace.decision_output_compared",
  calculation_compared: "scenarios.trace.calculation_compared",
  comparison_completed: "scenarios.trace.comparison_completed",
};

/** One trace entry: a recognised step with its exact fields, or the exact server text. */
export type TraceEntry =
  | {
      readonly kind: "step";
      readonly phase: "execution" | "comparison";
      readonly step: string;
      readonly label: MessageKey;
      readonly fields: readonly (readonly [string, unknown])[];
      readonly text: string;
    }
  | { readonly kind: "unknown"; readonly text: string };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parse(text: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return undefined;
  }
}

/** Parses one canonical trace text `{phase, step: {step, ...}}`; anything else stays exact text. */
export function parseTraceEntry(text: string): TraceEntry {
  const outer = parse(text);
  if (!isRecord(outer) || !isRecord(outer["step"])) {
    return { kind: "unknown", text };
  }
  const phase = outer["phase"];
  const inner = outer["step"];
  const step = inner["step"];
  const table = phase === "execution" ? EXECUTION_STEPS : phase === "comparison" ? COMPARISON_STEPS : null;
  if (table === null || typeof step !== "string" || !Object.prototype.hasOwnProperty.call(table, step)) {
    return { kind: "unknown", text };
  }
  const label = table[step];
  if (label === undefined || (phase !== "execution" && phase !== "comparison")) {
    return { kind: "unknown", text };
  }
  const fields = Object.entries(inner).filter(([key]) => key !== "step");
  return { kind: "step", phase, step, label, fields, text };
}

/** One missing semantic value of an undecidable run. */
export interface MissingEntry {
  readonly owner_ref: string;
  readonly key: string;
  readonly kind: string;
}

/** Parses one canonical `MissingSemantic` text, or null when it is not exactly that shape. */
export function parseMissing(text: string): MissingEntry | null {
  const value = parse(text);
  if (!isRecord(value)) {
    return null;
  }
  const { owner_ref: owner, key, kind } = value;
  return typeof owner === "string" && typeof key === "string" && typeof kind === "string" ? { owner_ref: owner, key, kind } : null;
}

/** A field value as exact data: strings, numbers and booleans as is, other JSON as its text. */
function FieldValue({ value }: { value: unknown }) {
  if (typeof value === "string" || typeof value === "number") {
    return <span className="font-mono">{String(value)}</span>;
  }
  if (typeof value === "boolean") {
    return (
      <span className={"font-mono " + (value ? "text-emerald-700" : "text-red-700")} data-boolean={String(value)}>
        {String(value)}
      </span>
    );
  }
  if (value === null) {
    return <span className="font-mono">{t("scenarios.none")}</span>;
  }
  if (Array.isArray(value) && value.every((v) => typeof v === "string")) {
    return <span className="font-mono">{value.length === 0 ? t("scenarios.none") : value.join(", ")}</span>;
  }
  return <code className="break-all">{JSON.stringify(value)}</code>;
}

/**
 * The server ScenarioRun as business-semantic material (Hotfix 065 contract): each trace step with
 * its label and exact semantic IDs and results; for an undecidable run the missing semantic
 * elements (owner_ref, key, kind); for a failed run the exact server diffs. Nothing is computed.
 */
export function ScenarioTrace({ run }: { run: ScenarioRun }) {
  const entries = run.trace.map(parseTraceEntry);
  const missing = run.missing ?? [];
  const diff = run.diff ?? [];
  return (
    <div className="space-y-2 text-sm" data-testid="scenario-trace">
      {run.result === "undecidable" && (
        <section aria-label={t("scenarios.missing.title")} className="rounded border border-amber-600 p-2" data-testid="run-missing">
          <h5 className="font-semibold">{t("scenarios.missing.title")}</h5>
          {missing.length === 0 ? (
            <p>{t("scenarios.none")}</p>
          ) : (
            <ul className="space-y-1">
              {missing.map((text, index) => {
                const entry = parseMissing(text);
                return (
                  <li key={index} data-missing={entry?.owner_ref ?? ""}>
                    {entry === null ? (
                      <code className="break-all">{text}</code>
                    ) : (
                      <dl className="grid grid-cols-[auto_1fr] gap-x-2">
                        <dt>{t("scenarios.missing.element")}</dt>
                        <dd className="font-mono font-semibold" data-field="owner_ref">
                          {entry.owner_ref}
                        </dd>
                        <dt>{t("scenarios.missing.key")}</dt>
                        <dd className="font-mono" data-field="key">
                          {entry.key}
                        </dd>
                        <dt>{t("scenarios.missing.kind")}</dt>
                        <dd className="font-mono" data-field="kind">
                          {entry.kind}
                        </dd>
                      </dl>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      )}
      {run.result === "fail" && (
        <section aria-label={t("scenarios.diff.title")} className="rounded border border-red-700 p-2" data-testid="run-diff">
          <h5 className="font-semibold">{t("scenarios.diff.title")}</h5>
          <ul className="space-y-1">
            {diff.map((text, index) => (
              <li key={index}>
                <code className="break-all">{text}</code>
              </li>
            ))}
          </ul>
        </section>
      )}
      <section aria-label={t("scenarios.trace.title")}>
        <h5 className="font-semibold">{t("scenarios.trace.title")}</h5>
        <ol className="space-y-1">
          {entries.map((entry, index) =>
            entry.kind === "step" ? (
              <li key={index} data-trace-step={entry.step} data-phase={entry.phase} className="rounded border p-1">
                <span className="mr-2 text-xs uppercase text-slate-500">{t(entry.phase === "execution" ? "scenarios.trace.execution" : "scenarios.trace.comparison")}</span>
                <span className="font-semibold">{t(entry.label)}</span>
                {entry.fields.length > 0 && (
                  <dl className="ml-4 grid grid-cols-[auto_1fr] gap-x-2">
                    {entry.fields.map(([key, value]) => (
                      <div key={key} className="contents" data-trace-field={key}>
                        <dt className="font-mono text-slate-600">{key}</dt>
                        <dd>
                          <FieldValue value={value} />
                        </dd>
                      </div>
                    ))}
                  </dl>
                )}
              </li>
            ) : (
              <li key={index} data-trace-step="unknown" className="rounded border border-amber-600 p-1">
                <span className="font-semibold">{t("scenarios.trace.unknown")}</span>
                <code className="ml-2 break-all" data-testid="unknown-trace-text">
                  {entry.text}
                </code>
              </li>
            ),
          )}
        </ol>
      </section>
    </div>
  );
}
