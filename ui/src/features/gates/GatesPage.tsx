import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";

import { apiClient, readData } from "../../api/client";
import type { components } from "../../api/schema";
import { PILOT_GATES } from "../../app/nav";
import { t, type MessageKey } from "../../i18n";
import { gatesQuery } from "../../app/work";
import { ReadFailure } from "../../components/ReadFailure";

type GateStatus = components["schemas"]["GateStatus"];
type GateRuleResult = components["schemas"]["GateRuleResult"];
type GateRuleState = components["schemas"]["GateRuleState"];
type Readiness = components["schemas"]["Readiness"];

/** The project readiness query root. */
export const readinessQueryKey = ["readiness"] as const;

/** The label of each exact server rule state (Hotfix 082); no state is shown as another. */
const RULE_STATE_LABELS: Readonly<Record<GateRuleState, MessageKey>> = {
  PASS: "gates.state.pass",
  FAIL: "gates.state.fail",
  WARN: "gates.state.warn",
  NOT_APPLICABLE: "gates.state.notApplicable",
  WAIVED: "gates.state.waived",
  ERROR: "gates.state.error",
};

const SUB_SCORES: readonly (readonly [keyof Readiness["sub"], MessageKey])[] = [
  ["clarity", "gates.readiness.clarity"],
  ["completeness", "gates.readiness.completeness"],
  ["confirmation", "gates.readiness.confirmation"],
  ["consistency", "gates.readiness.consistency"],
  ["verifiability", "gates.readiness.verifiability"],
];

async function fetchGates(): Promise<GateStatus[]> {
  return (await readData(apiClient.GET("/api/gates")));
}

async function fetchReadiness(): Promise<Readiness> {
  return (await readData(apiClient.GET("/api/readiness")));
}

function Ids({ values }: { values: readonly string[] }) {
  return values.length === 0 ? <span className="text-slate-500">{t("gates.none")}</span> : <span className="font-mono">{values.join(", ")}</span>;
}

function Item({ labelKey, name, children }: { labelKey: MessageKey; name: string; children: ReactNode }) {
  return (
    <div className="contents" data-field={name}>
      <dt className="text-slate-600">{t(labelKey)}</dt>
      <dd>{children}</dd>
    </div>
  );
}

/** One rule result exactly as the server projected it from the GateReport. */
function RuleRow({ rule }: { rule: GateRuleResult }) {
  return (
    <li className="rounded border p-2 text-sm" data-rule-id={rule.rule_id} data-state={rule.state}>
      <p>
        <span className="font-mono">{rule.rule_id}</span>{" "}
        <span className="ml-2 rounded border px-1 font-semibold" data-testid="rule-state">
          {t(RULE_STATE_LABELS[rule.state])}
        </span>
      </p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-2">
        <Item labelKey="gates.rule.severity" name="effective_severity">
          <span className="font-mono">{rule.effective_severity}</span>
        </Item>
        {rule.declared_severity !== rule.effective_severity && (
          <Item labelKey="gates.rule.declaredSeverity" name="declared_severity">
            <span className="font-mono">{rule.declared_severity}</span>
          </Item>
        )}
        {rule.applicability.state === "NOT_APPLICABLE" && (
          <Item labelKey="gates.rule.notApplicableReason" name="applicability_reason">
            {rule.applicability.reason ?? t("gates.none")}
          </Item>
        )}
        {rule.error !== null && (
          <Item labelKey="gates.rule.error" name="error">
            <span className="font-mono" data-testid="error-code">
              {rule.error.code}
            </span>{" "}
            <span data-testid="error-message">{rule.error.message}</span>
          </Item>
        )}
        {rule.finding_ref !== null && (
          <Item labelKey="gates.rule.finding" name="finding_ref">
            <span className="font-mono">{rule.finding_ref}</span>
          </Item>
        )}
        {rule.waiver_ref !== null && (
          <Item labelKey="gates.rule.waiver" name="waiver_ref">
            <span className="font-mono">{rule.waiver_ref}</span>
          </Item>
        )}
        {rule.semantic_condition_key !== null && (
          <Item labelKey="gates.rule.condition" name="semantic_condition_key">
            <span className="font-mono">{rule.semantic_condition_key}</span>
          </Item>
        )}
        {rule.targets.length > 0 && (
          <Item labelKey="gates.rule.targets" name="targets">
            <Ids values={rule.targets} />
          </Item>
        )}
        {rule.evidence.length > 0 && (
          <Item labelKey="gates.rule.evidence" name="evidence">
            <Ids values={rule.evidence} />
          </Item>
        )}
      </dl>
    </li>
  );
}

/** A gate without a current report: not evaluated or stale, with the server's typed reason. */
function UnavailableGate({ status }: { status: GateStatus | undefined }) {
  return (
    <>
      <p data-testid="gate-result">{t(status?.state === "stale" ? "gate.stale" : "gate.unevaluated")}</p>
      {status?.reason !== undefined && status.reason !== null && (
        <p className="text-sm" data-testid="gate-reason">
          {t("gates.reason")} <span className="font-mono" data-testid="gate-reason-code">{status.reason.code}</span>{" "}
          <span data-testid="gate-reason-message">{status.reason.message}</span>
        </p>
      )}
    </>
  );
}

/** A current gate: its server Pass/Fail, summary, every rule result in report order and waivers. */
function CurrentGate({ status }: { status: GateStatus }) {
  return (
    <>
      <p className="font-semibold" data-testid="gate-result">
        {t(status.passed ? "gate.pass" : "gate.fail")}
      </p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-2 text-sm" data-testid="gate-summary">
        <Item labelKey="gates.summary.blockerFailed" name="blocker_failed">
          {status.summary.blocker_failed}
        </Item>
        <Item labelKey="gates.summary.blockerWaived" name="blocker_waived">
          {status.summary.blocker_waived}
        </Item>
        <Item labelKey="gates.summary.warnings" name="warnings">
          {status.summary.warnings}
        </Item>
        <Item labelKey="gates.findings" name="findings">
          <Ids values={status.findings} />
        </Item>
      </dl>
      <h3 className="text-sm font-semibold">{t("gates.rules")}</h3>
      {status.rules.length === 0 ? (
        <p className="text-sm">{t("gates.noRules")}</p>
      ) : (
        <ol className="space-y-1" data-testid="gate-rules">
          {status.rules.map((rule) => (
            <RuleRow key={rule.rule_id} rule={rule} />
          ))}
        </ol>
      )}
      <h3 className="text-sm font-semibold">{t("gates.waivers")}</h3>
      {status.waivers.length === 0 ? (
        <p className="text-sm">{t("gates.noWaivers")}</p>
      ) : (
        <ul className="space-y-1 text-sm" data-testid="gate-waivers">
          {status.waivers.map((waiver) => (
            <li key={waiver.finding_ref} className="rounded border p-1" data-waiver-rule={waiver.rule_id}>
              <dl className="grid grid-cols-[auto_1fr] gap-x-2">
                <Item labelKey="gates.waiver.rule" name="rule_id">
                  <span className="font-mono">{waiver.rule_id}</span>
                </Item>
                <Item labelKey="gates.waiver.finding" name="finding_ref">
                  <span className="font-mono">{waiver.finding_ref}</span>
                </Item>
                <Item labelKey="gates.waiver.findingKey" name="finding_key">
                  <span className="break-all font-mono">{waiver.finding_key}</span>
                </Item>
                <Item labelKey="gates.waiver.decision" name="decision_ref">
                  <span className="font-mono">{waiver.decision_ref}</span>
                </Item>
              </dl>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function Score({ value }: { value: number | null | undefined }) {
  return value === undefined || value === null ? <span className="text-slate-500">{t("gates.readiness.notCalculated")}</span> : <span className="font-mono">{String(value)}</span>;
}

/** Advisory, uncalibrated readiness: secondary data that never affects any gate or rule. */
function ReadinessSection({ readiness }: { readiness: Readiness }) {
  return (
    <section aria-label={t("gates.readiness.title")} className="space-y-1 rounded border border-dashed p-2 text-sm text-slate-700" data-testid="readiness">
      <h2 className="font-semibold">
        {t("gates.readiness.title")} <span className="ml-1 rounded border px-1 text-xs">{t("gates.readiness.uncalibrated")}</span>
      </h2>
      <p className="text-xs">{t("gates.readiness.note")}</p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-2">
        <Item labelKey="gates.readiness.overall" name="overall">
          <Score value={readiness.overall} />
        </Item>
        {SUB_SCORES.map(([name, labelKey]) => (
          <Item key={name} labelKey={labelKey} name={name}>
            <Score value={readiness.sub[name]} />
          </Item>
        ))}
      </dl>
    </section>
  );
}

/**
 * U2.7 Gates (Hotfix 082): the I0-F4 gates in order from GET /api/gates; a gate without a
 * current report is Unevaluated or Stale with the server's typed reason (H0.11). Every rule result, waiver and summary is the server projection
 * of the current GateReport; readiness is a separate advisory section.
 */
export function GatesPage() {
  const gates = useQuery({ queryKey: gatesQuery.queryKey, queryFn: fetchGates });
  const readiness = useQuery({ queryKey: readinessQueryKey, queryFn: fetchReadiness });

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("gates.title")}</h1>
      {gates.isError && <ReadFailure error={gates.error} messageKey="gates.loadFailed" />}
      {gates.data === undefined && !gates.isError && <p>{t("gates.loading")}</p>}
      {gates.data !== undefined && (
        <ol className="space-y-3">
          {PILOT_GATES.map((gate) => {
            const status = gates.data.find((s) => s.gate === gate);
            return (
              <li key={gate} className="space-y-2 rounded border p-2" data-gate={gate}>
                <h2 className="font-mono text-lg font-semibold">{gate}</h2>
                {status === undefined || status.state !== "current" ? (
                  <UnavailableGate status={status} />
                ) : (
                  <CurrentGate status={status} />
                )}
              </li>
            );
          })}
        </ol>
      )}
      {readiness.data !== undefined && <ReadinessSection readiness={readiness.data} />}
    </div>
  );
}
