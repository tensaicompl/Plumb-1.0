import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Info, OctagonAlert, ShieldCheck, TriangleAlert, type LucideIcon } from "lucide-react";
import { useState } from "react";

import { apiClient } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, writeErrorMessage } from "../../api/useVersionedMutation";
import { hasRole, useMe } from "../../app/auth";
import { gatesQuery } from "../../app/work";
import { ReadFailure } from "../../components/ReadFailure";
import { Button, cn, EmptyState, ReasonSheet, SkeletonList, StatusPill, toast } from "../../components/ui";
import { t, tf, type MessageKey } from "../../i18n";
import { requirementsQuery, waivedFindingIds } from "../requirements/data";
import { affectedChips, decisionsQuery, requirementLabels, reviewedFindingsQuery, waiverDecision } from "./data";

type Finding = components["schemas"]["Finding"];
type WriteResult = components["schemas"]["WriteResult"];

/** The role that waives findings. */
const ANALYST = "analyst";

interface Severity {
  readonly label: MessageKey;
  readonly icon: LucideIcon;
  readonly tone: string;
  readonly rank: number;
}

const SEVERITY: Readonly<Record<string, Severity>> = {
  blocker: { label: "quality.severity.blocker", icon: OctagonAlert, tone: "text-blocked-fg", rank: 0 },
  error: { label: "quality.severity.error", icon: TriangleAlert, tone: "text-blocked-fg", rank: 1 },
  warning: { label: "quality.severity.warning", icon: TriangleAlert, tone: "text-attention-fg", rank: 2 },
  warn: { label: "quality.severity.warning", icon: TriangleAlert, tone: "text-attention-fg", rank: 2 },
  info: { label: "quality.severity.info", icon: Info, tone: "text-neutral-fg", rank: 3 },
};

function severityOf(finding: Finding): Severity {
  return SEVERITY[finding.severity.toLowerCase()] ?? { label: "quality.severity.info", icon: Info, tone: "text-neutral-fg", rank: 4 };
}

type Disposition = "open" | "waived" | "resolved";

/** Quality → Issues: every finding with severity, message, code and affected items. */
export function IssuesTab() {
  const findings = useQuery(reviewedFindingsQuery);
  const gates = useQuery(gatesQuery);
  const decisions = useQuery(decisionsQuery);
  const requirements = useQuery(requirementsQuery);
  const me = useMe();
  const queryClient = useQueryClient();
  const [waiving, setWaiving] = useState<Finding | null>(null);
  const waive = useVersionedMutation<WriteResult, { id: string; reason: string }>(
    ({ id, reason }, ifMatch) => apiClient.POST("/api/findings/{id}/waive", { params: { path: { id }, header: { "If-Match": ifMatch } }, body: { reason } }),
    findings.data?.etag,
  );

  if (findings.isError) {
    return (
      <div className="flex flex-col items-start gap-3 text-body text-fg-2">
        <ReadFailure error={findings.error} messageKey="quality.issues.loadFailed" />
        <Button variant="outline" size={30} onClick={() => void findings.refetch()}>
          {t("quality.retry")}
        </Button>
      </div>
    );
  }
  if (findings.data === undefined) {
    return <SkeletonList rows={5} className="overflow-hidden rounded-card border border-line bg-card" />;
  }
  if (findings.data.items.length === 0) {
    return <EmptyState visual="check" title={t("quality.issues.empty")} />;
  }
  const waived = waivedFindingIds(gates.data);
  const labels = requirementLabels(requirements.data?.items);
  const canWaive = hasRole(me.data, ANALYST);
  const disposition = (finding: Finding): Disposition => {
    if ((finding.waiver_ref ?? null) !== null || waived.has(finding.id)) {
      return "waived";
    }
    return finding.status.toLowerCase() === "open" ? "open" : "resolved";
  };
  const order: Readonly<Record<Disposition, number>> = { open: 0, waived: 1, resolved: 2 };
  const rows = [...findings.data.items].sort(
    (a, b) => order[disposition(a)] - order[disposition(b)] || severityOf(a).rank - severityOf(b).rank || a.code.localeCompare(b.code),
  );
  const failure = waive.error === null || waive.error.name === "PreconditionFailedError" ? undefined : tf("quality.writeFailed", { message: writeErrorMessage(waive.error) ?? waive.error.message });

  return (
    <>
      <ul aria-label={t("quality.issues.label")} className="m-0 list-none overflow-hidden rounded-card border border-line bg-card p-0">
        {rows.map((finding) => {
          const state = disposition(finding);
          const severity = severityOf(finding);
          const Icon = state === "waived" ? ShieldCheck : severity.icon;
          const decision = state === "waived" ? waiverDecision(finding.id, finding.waiver_ref, gates.data, decisions.data) : null;
          const affected = affectedChips(finding.affected_refs, labels).join(", ");
          return (
            <li key={finding.id} data-finding-id={finding.id} className="flex items-center gap-3.5 border-b border-line px-5 py-3.5 last:border-b-0">
              <span className={cn("flex w-24 shrink-0 items-center gap-1.5 text-caption font-semibold", state === "waived" ? "text-fg-3" : severity.tone)}>
                <Icon size={14} strokeWidth={1.6} aria-hidden="true" />
                {t(severity.label)}
              </span>
              <div className="min-w-0 flex-1">
                <div className="text-callout font-medium text-fg">{finding.message}</div>
                <div className="mt-0.5 text-caption text-fg-3">
                  <span className="select-text font-mono">{finding.code}</span>
                  {affected !== "" && " · " + affected}
                </div>
                {decision !== null && <div className="mt-1 text-caption text-fg-2">{tf("quality.issues.waivedBy", { who: decision.decided_by, reason: decision.rationale ?? "" })}</div>}
              </div>
              {state === "open" && canWaive && (
                <Button variant="outline" size={30} onClick={() => setWaiving(finding)}>
                  {t("quality.issues.waive")}
                </Button>
              )}
              {state === "waived" && <StatusPill status="waived" />}
              {state === "resolved" && <StatusPill status="closed" label={t("quality.issues.resolved")} />}
            </li>
          );
        })}
      </ul>
      <ReasonSheet
        open={waiving !== null}
        onOpenChange={(open) => {
          if (!open) {
            setWaiving(null);
            waive.reset();
          }
        }}
        title={t("quality.issues.sheet.title")}
        description={tf("quality.issues.sheet.description", { message: waiving?.message ?? "" })}
        reasonPlaceholder={t("quality.issues.sheet.placeholder")}
        confirmLabel={t("quality.issues.sheet.confirm")}
        pending={waive.isPending}
        error={failure}
        onConfirm={(reason) => {
          if (waiving === null) {
            return;
          }
          waive.mutate(
            { id: waiving.id, reason },
            {
              onSuccess: () => {
                setWaiving(null);
                void queryClient.invalidateQueries();
                toast({ message: t("quality.issues.done") });
              },
              onError: (error) => {
                if (error.name === "PreconditionFailedError") {
                  setWaiving(null);
                }
              },
            },
          );
        }}
      />
    </>
  );
}
