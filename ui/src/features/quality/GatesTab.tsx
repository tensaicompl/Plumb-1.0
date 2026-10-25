import { useQuery } from "@tanstack/react-query";
import { Check, ChevronDown, CircleAlert, Info, RefreshCw, ShieldCheck, TriangleAlert, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import type { components } from "../../api/schema";
import { hasRole, useMe } from "../../app/auth";
import { PILOT_GATES } from "../../app/nav";
import { isJobActive } from "../../app/pipeline";
import { useRunStage, useRunStore } from "../../app/run";
import { useGo } from "../../app/shell/navigation";
import { findingsQuery, gatesQuery } from "../../app/work";
import { ReadFailure } from "../../components/ReadFailure";
import { Button, cn, Eyebrow, GateDot, gateName, gateStateLabel, Skeleton, SkeletonGroup, toast, type GateLightState } from "../../components/ui";
import { t, tc, tf, type MessageKey } from "../../i18n";
import { requirementsQuery, type GateStatus } from "../requirements/data";
import { affectedChips, affectedHref, decisionsQuery, GATE_OWNER, requirementLabels, waiverDecision } from "./data";

type GateName = components["schemas"]["GateName"];
type GateRuleResult = components["schemas"]["GateRuleResult"];
type GateRuleState = components["schemas"]["GateRuleState"];
type Finding = components["schemas"]["Finding"];

/** The role that runs steps. */
const ANALYST = "analyst";

const GROUPS: readonly { state: GateRuleState; label: MessageKey; icon: LucideIcon; tone: string }[] = [
  { state: "ERROR", label: "quality.group.error", icon: CircleAlert, tone: "text-blocked-fg" },
  { state: "FAIL", label: "quality.group.fail", icon: TriangleAlert, tone: "text-blocked-fg" },
  { state: "WAIVED", label: "quality.group.waived", icon: ShieldCheck, tone: "text-fg-3" },
  { state: "WARN", label: "quality.group.warn", icon: TriangleAlert, tone: "text-attention-fg" },
  { state: "PASS", label: "quality.group.pass", icon: Check, tone: "text-pass-fg" },
  { state: "NOT_APPLICABLE", label: "quality.group.notApplicable", icon: Info, tone: "text-fg-4" },
];

const STATE_TONE: Readonly<Record<GateLightState, string>> = {
  pass: "text-pass-fg",
  fail: "text-blocked-fg",
  stale: "text-neutral-fg",
  not_evaluated: "text-neutral-fg",
  running: "text-running-fg",
};

/** A gate's light: current pass or fail, outdated (stale), or not checked yet. */
function lightState(gate: GateStatus | undefined): GateLightState {
  if (gate === undefined || gate.state === "not_evaluated") {
    return "not_evaluated";
  }
  if (gate.state === "stale") {
    return "stale";
  }
  return gate.passed ? "pass" : "fail";
}

function stateLabel(state: GateLightState): string {
  return state === "not_evaluated" ? t("quality.gate.notChecked") : gateStateLabel(state);
}

function summaryText(gate: GateStatus | undefined): string {
  if (gate === undefined) {
    return "";
  }
  if (gate.state !== "current") {
    return gate.reason?.message ?? "";
  }
  return [
    tc("quality.gate.blockersFailed", gate.summary.blocker_failed),
    tf("quality.gate.waived", { count: gate.summary.blocker_waived }),
    tc("quality.gate.warnings", gate.summary.warnings),
  ].join(" · ");
}

/** The rule targets that need attention (not passing, not inapplicable). */
function attentionTargets(gate: GateStatus): string[] {
  return gate.rules.filter((rule) => rule.state !== "PASS" && rule.state !== "NOT_APPLICABLE").flatMap((rule) => [...rule.targets, ...(rule.error?.targets ?? [])]);
}

/** Quality → Gates: the five gates as expandable rows; `expanded` is the gate in the URL. */
export function GatesTab({ expanded }: { expanded: string | null }) {
  const gates = useQuery(gatesQuery);
  const go = useGo();
  if (gates.isError) {
    return (
      <div className="flex flex-col items-start gap-3 text-body text-fg-2">
        <ReadFailure error={gates.error} messageKey="quality.loadFailed" />
        <Button variant="outline" size={30} onClick={() => void gates.refetch()}>
          {t("quality.retry")}
        </Button>
      </div>
    );
  }
  if (gates.data === undefined) {
    return (
      <SkeletonGroup className="flex flex-col gap-2.5">
        {PILOT_GATES.map((gate) => (
          <div key={gate} className="flex items-center gap-3.5 rounded-[14px] border border-line bg-card px-5 py-[18px]">
            <Skeleton width={12} height={12} radius="full" />
            <Skeleton width={140} height={14} />
            <Skeleton width="40%" height={12} />
          </div>
        ))}
      </SkeletonGroup>
    );
  }
  return (
    <div className="flex flex-col gap-2.5">
      {PILOT_GATES.map((name) => (
        <GateRow
          key={name}
          name={name}
          gate={gates.data.find((g) => g.gate === name)}
          open={expanded === name}
          onToggle={() => go(expanded === name ? "/quality" : "/quality/" + name)}
          onRecheckEvaluated={async () => {
            await gates.refetch();
            toast({ message: tf("quality.recheck.evaluated", { gate: gateName(name) }) });
          }}
        />
      ))}
    </div>
  );
}

function GateRow({
  name,
  gate,
  open,
  onToggle,
  onRecheckEvaluated,
}: {
  name: GateName;
  gate: GateStatus | undefined;
  open: boolean;
  onToggle: () => void;
  onRecheckEvaluated: () => Promise<void>;
}) {
  const job = useRunStore((state) => state.job);
  const owner = GATE_OWNER[name];
  const checking = owner !== undefined && isJobActive(job) && job?.stage === owner;
  const state: GateLightState = checking ? "running" : lightState(gate);
  const panelId = "gate-panel-" + name;
  return (
    <div className="overflow-hidden rounded-[14px] border border-line bg-card shadow-xs" data-gate={name} data-gate-state={state}>
      {/* The accordion pattern: each gate is a heading whose button expands its rules. */}
      <h2 className="m-0 text-body font-normal">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={onToggle}
        className="flex w-full cursor-pointer items-center gap-3.5 border-0 bg-transparent px-5 py-[18px] text-left"
      >
        <GateDot state={state} size={12} />
        <span className="w-[150px] shrink-0">
          <span className="text-subhead font-semibold text-fg">{gateName(name)}</span> <span className="font-mono text-eyebrow font-medium text-fg-3">{name}</span>
        </span>
        <span className={cn("w-[110px] shrink-0 text-footnote font-semibold", STATE_TONE[state])}>{stateLabel(state)}</span>
        <span className="min-w-0 flex-1 text-footnote text-fg-2">{summaryText(gate)}</span>
        <ChevronDown
          size={18}
          strokeWidth={1.6}
          aria-hidden="true"
          className={cn("shrink-0 text-fg-3 transition-transform duration-200 ease-standard", open && "rotate-180")}
        />
      </button>
      </h2>
      {open && <GatePanel id={panelId} name={name} gate={gate} checking={checking} onRecheckEvaluated={onRecheckEvaluated} />}
    </div>
  );
}

function GatePanel({
  id,
  name,
  gate,
  checking,
  onRecheckEvaluated,
}: {
  id: string;
  name: GateName;
  gate: GateStatus | undefined;
  checking: boolean;
  onRecheckEvaluated: () => Promise<void>;
}) {
  const me = useMe();
  const go = useGo();
  const findings = useQuery(findingsQuery);
  const decisions = useQuery(decisionsQuery);
  const requirements = useQuery(requirementsQuery);
  const gates = useQuery(gatesQuery);
  const { run, starting } = useRunStage();
  const labels = requirementLabels(requirements.data?.items);
  const rules = gate?.rules ?? [];
  const owner = GATE_OWNER[name];
  const affected = gate === undefined ? null : affectedHref(attentionTargets(gate), labels);
  const recheck = () => {
    if (owner !== undefined) {
      run(owner);
    } else {
      void onRecheckEvaluated();
    }
  };
  return (
    <section id={id} aria-label={gateName(name) + " " + name} className="border-t border-line px-5 pb-4 pt-2">
      {rules.length === 0 && <p className="m-0 py-3.5 text-footnote text-fg-3">{gate?.state === "current" || gate === undefined ? t("quality.gate.noRules") : (gate.reason?.message ?? "")}</p>}
      {GROUPS.map((group) => {
        const rows = rules.filter((rule) => rule.state === group.state);
        if (rows.length === 0) {
          return null;
        }
        const Icon = group.icon;
        return (
          <div key={group.state}>
            <Eyebrow as="h3" className="m-0 pb-2 pt-3.5">
              {t(group.label)}
            </Eyebrow>
            <ul className="m-0 list-none p-0">
              {rows.map((rule) => (
                <RuleRow
                  key={rule.rule_id}
                  rule={rule}
                  icon={<Icon size={16} strokeWidth={1.6} aria-hidden="true" className={cn("mt-px shrink-0", group.tone)} />}
                  finding={findings.data?.find((f) => f.id === rule.finding_ref) ?? null}
                  note={ruleNote(rule, gates.data, decisions.data)}
                  chips={
                    // A passing rule's targets are what it checked, not what it affects.
                    rule.state === "PASS" || rule.state === "NOT_APPLICABLE" ? [] : affectedChips([...rule.targets, ...(rule.error?.targets ?? [])], labels)
                  }
                />
              ))}
            </ul>
          </div>
        );
      })}
      <div className="mt-3.5 flex gap-2">
        {hasRole(me.data, ANALYST) && (
          <Button
            variant="outline"
            size={30}
            pending={starting || checking || gates.isFetching}
            icon={<RefreshCw size={14} strokeWidth={1.6} aria-hidden="true" />}
            onClick={recheck}
          >
            {t("quality.recheck")}
          </Button>
        )}
        {affected !== null && (
          <Button variant="outline" size={30} onClick={() => go(affected)}>
            {t("quality.showAffected")}
          </Button>
        )}
      </div>
    </section>
  );
}

/** The secondary note of a rule: who waived it and why, why it does not apply, or non-blocking. */
function ruleNote(rule: GateRuleResult, gates: readonly GateStatus[] | undefined, decisions: readonly components["schemas"]["Decision"][] | undefined): string | null {
  if (rule.state === "WAIVED") {
    const decision = waiverDecision(rule.finding_ref ?? "", rule.waiver_ref, gates, decisions);
    return decision === null ? t("quality.rule.waived") : tf("quality.rule.waivedBy", { who: decision.decided_by, reason: decision.rationale ?? "" });
  }
  if (rule.state === "NOT_APPLICABLE") {
    return rule.applicability.reason ?? null;
  }
  if (rule.state === "ERROR") {
    return rule.error?.message ?? null;
  }
  if ((rule.state === "FAIL" || rule.state === "WARN") && rule.effective_severity !== "blocker") {
    return t("quality.rule.nonBlocking");
  }
  return null;
}

function RuleRow({ rule, icon, finding, note, chips }: { rule: GateRuleResult; icon: ReactNode; finding: Finding | null; note: string | null; chips: string[] }) {
  const problem = rule.state !== "PASS" && rule.state !== "NOT_APPLICABLE";
  const message = (problem ? finding?.message : undefined) ?? rule.title ?? rule.rule_id;
  return (
    <li className="flex items-start gap-3 border-b border-surface py-2.5 text-footnote last:border-b-0">
      {icon}
      <div className="min-w-0 flex-1">
        <div className="font-medium text-fg">{message}</div>
        <div className="mt-0.5 text-caption text-fg-3">
          <span className="select-text font-mono">{rule.rule_id}</span>
          {note !== null && note !== "" && " · " + note}
        </div>
      </div>
      {chips.length > 0 && (
        <ul aria-label={t("quality.affected.label")} className="m-0 flex max-w-[260px] list-none flex-wrap justify-end gap-1 p-0">
          {chips.map((chip) => (
            <li key={chip} className="h-[22px] whitespace-nowrap rounded-pill border border-line bg-surface px-2 font-mono text-eyebrow font-medium leading-5 text-fg">
              {chip}
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}
