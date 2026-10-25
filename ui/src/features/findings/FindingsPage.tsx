import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";

import { apiClient, readData } from "../../api/client";
import type { components } from "../../api/schema";
import { t, type MessageKey } from "../../i18n";
import { ReadFailure } from "../../components/ReadFailure";

type Finding = components["schemas"]["Finding"];
type FindingRuleClass = components["schemas"]["FindingRuleClass"];
type Assumption = components["schemas"]["Assumption"];

/** The Findings list query root. */
export const findingsQueryKey = ["findings"] as const;
/** The Assumptions list query root. */
export const assumptionsQueryKey = ["assumptions"] as const;

/**
 * The presentation category of each server rule class (Hotfix 080). The two policy classes share
 * one category; their raw wire value stays visible next to it.
 */
const RULE_CLASS_LABELS: Readonly<Record<FindingRuleClass, MessageKey>> = {
  external_standard: "findings.ruleClass.externalStandard",
  external_interop: "findings.ruleClass.externalInterop",
  plumb_core: "findings.ruleClass.plumbCore",
  profile_policy: "findings.ruleClass.profileOrganization",
  organization_policy: "findings.ruleClass.profileOrganization",
};

const POLICY_CLASSES: readonly FindingRuleClass[] = ["profile_policy", "organization_policy"];

async function fetchFindings(): Promise<Finding[]> {
  return (await readData(apiClient.GET("/api/findings"))).items;
}

async function fetchAssumptions(): Promise<Assumption[]> {
  return (await readData(apiClient.GET("/api/assumptions"))).items;
}

/** The server rule class only: never derived from the code or the family. */
function RuleClassBadge({ ruleClass }: { ruleClass: FindingRuleClass | null }) {
  return (
    <span className="rounded border px-1 text-xs" data-rule-class={ruleClass ?? ""}>
      {t(ruleClass === null ? "findings.ruleClass.unclassified" : RULE_CLASS_LABELS[ruleClass])}
      {ruleClass !== null && POLICY_CLASSES.includes(ruleClass) && <code className="ml-1" data-testid="raw-rule-class">{ruleClass}</code>}
    </span>
  );
}

function Field({ labelKey, children, name }: { labelKey: MessageKey; children: ReactNode; name: string }) {
  return (
    <div className="contents" data-field={name}>
      <dt className="text-slate-600">{t(labelKey)}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function None() {
  return <span className="text-slate-500">{t("findings.none")}</span>;
}

function FindingItem({ finding }: { finding: Finding }) {
  return (
    <li className="rounded border p-2 text-sm" data-finding-id={finding.id}>
      <dl className="grid grid-cols-[auto_1fr] gap-x-2">
        <Field labelKey="findings.field.id" name="id">
          <span className="font-mono">{finding.id}</span>
        </Field>
        <Field labelKey="findings.field.code" name="code">
          <span className="font-mono">{finding.code}</span>
        </Field>
        <Field labelKey="findings.field.family" name="family">
          <span className="font-mono">{finding.family}</span>
        </Field>
        <Field labelKey="findings.field.ruleClass" name="rule_class">
          <RuleClassBadge ruleClass={finding.rule_class} />
        </Field>
        <Field labelKey="findings.field.severity" name="severity">
          {finding.severity}
        </Field>
        <Field labelKey="findings.field.status" name="status">
          {finding.status}
        </Field>
        <Field labelKey="findings.field.message" name="message">
          {finding.message}
        </Field>
        <Field labelKey="findings.field.affected" name="affected_refs">
          {finding.affected_refs.length === 0 ? <None /> : <span className="font-mono">{finding.affected_refs.join(", ")}</span>}
        </Field>
        {finding.waiver_ref !== undefined && finding.waiver_ref !== null && (
          <Field labelKey="findings.field.waiver" name="waiver_ref">
            <span className="font-mono">{finding.waiver_ref}</span>
          </Field>
        )}
      </dl>
    </li>
  );
}

/** A recorded default exactly as the server sent it: its JSON text and JSON type, never executed. */
function DefaultValue({ value }: { value: Assumption["default_value"] }) {
  if (value === undefined || value === null) {
    return <None />;
  }
  return (
    <code data-value-type={typeof value}>{JSON.stringify(value)}</code>
  );
}

function Optional({ value }: { value: string | null | undefined }) {
  return value === undefined || value === null ? <None /> : <span className="font-mono">{value}</span>;
}

function AssumptionItem({ assumption }: { assumption: Assumption }) {
  return (
    <li className="rounded border p-2 text-sm" data-assumption-id={assumption.id}>
      <dl className="grid grid-cols-[auto_1fr] gap-x-2">
        <Field labelKey="findings.field.id" name="id">
          <span className="font-mono">{assumption.id}</span>
        </Field>
        <Field labelKey="findings.assumption.statement" name="statement">
          {assumption.statement}
        </Field>
        <Field labelKey="findings.assumption.owner" name="owner_ref">
          <span className="font-mono">{assumption.owner_ref}</span>
        </Field>
        <Field labelKey="findings.field.status" name="status">
          {assumption.status}
        </Field>
        <Field labelKey="findings.assumption.finding" name="finding_ref">
          <Optional value={assumption.finding_ref} />
        </Field>
        <Field labelKey="findings.assumption.default" name="default_value">
          <DefaultValue value={assumption.default_value} />
        </Field>
        <Field labelKey="findings.assumption.expires" name="expires_at">
          <Optional value={assumption.expires_at} />
        </Field>
        <Field labelKey="findings.assumption.risk" name="risk_ref">
          <Optional value={assumption.risk_ref} />
        </Field>
      </dl>
    </li>
  );
}

/**
 * U2.6 Findings & assumptions (Hotfix 080): read-only analyst view of the server Findings, with the
 * server-projected rule class kept separate from the family, and of the recorded Assumptions.
 */
export function FindingsPage() {
  const findings = useQuery({ queryKey: findingsQueryKey, queryFn: fetchFindings });
  const assumptions = useQuery({ queryKey: assumptionsQueryKey, queryFn: fetchAssumptions });

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("findings.pageTitle")}</h1>
      <section aria-label={t("findings.list")} className="space-y-2">
        <h2 className="font-semibold">{t("findings.list")}</h2>
        {findings.isError && <ReadFailure error={findings.error} messageKey="findings.loadFailed" />}
        {findings.data === undefined && !findings.isError && <p>{t("findings.loading")}</p>}
        {findings.data !== undefined && findings.data.length === 0 && <p>{t("findings.empty")}</p>}
        {findings.data !== undefined && findings.data.length > 0 && (
          <ul className="space-y-2">
            {findings.data.map((finding) => (
              <FindingItem key={finding.id} finding={finding} />
            ))}
          </ul>
        )}
      </section>
      <section aria-label={t("findings.assumptions")} className="space-y-2">
        <h2 className="font-semibold">{t("findings.assumptions")}</h2>
        {assumptions.isError && <ReadFailure error={assumptions.error} messageKey="findings.loadFailed" />}
        {assumptions.data === undefined && !assumptions.isError && <p>{t("findings.loading")}</p>}
        {assumptions.data !== undefined && assumptions.data.length === 0 && <p>{t("findings.noAssumptions")}</p>}
        {assumptions.data !== undefined && assumptions.data.length > 0 && (
          <ul className="space-y-2">
            {assumptions.data.map((assumption) => (
              <AssumptionItem key={assumption.id} assumption={assumption} />
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
