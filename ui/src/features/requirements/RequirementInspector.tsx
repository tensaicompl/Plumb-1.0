import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FlaskConical, MoreHorizontal, TriangleAlert, Workflow, Zap } from "lucide-react";
import { useState, type ReactNode } from "react";

import { apiClient } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, writeErrorMessage } from "../../api/useVersionedMutation";
import { hasRole, useMe } from "../../app/auth";
import { buildHref, useGo } from "../../app/shell/navigation";
import { findingsQuery, gatesQuery } from "../../app/work";
import { ReadFailure } from "../../components/ReadFailure";
import {
  Button,
  EvidenceQuote,
  IconButton,
  KindTag,
  Menu,
  MonoId,
  ReadinessRing,
  ReasonSheet,
  Skeleton,
  SkeletonGroup,
  StatusPill,
  toast,
  type MenuItem,
} from "../../components/ui";
import { t, tf, type MessageKey } from "../../i18n";
import { issuesOf, readinessQuery, requirementLabel, requirementQuery, waivedFindingIds, type Finding, type RequirementDetail, type RequirementSummary } from "./data";
import { ChangeKindSheet, DuplicateSheet, SplitSheet } from "./RequirementSheets";
import { pillStatus } from "./status";

type SourceListItem = components["schemas"]["SourceListItem"];
type WriteResult = components["schemas"]["WriteResult"];

/** The role that changes requirements. */
const ANALYST = "analyst";

/** The glyph shown for a value that is not calculated (not copy). */
const NO_VALUE = "–";

const SUB_SCORES: readonly { key: "clarity" | "completeness" | "confirmation" | "consistency" | "verifiability"; label: MessageKey }[] = [
  { key: "clarity", label: "ui.readiness.clarity" },
  { key: "completeness", label: "ui.readiness.completeness" },
  { key: "confirmation", label: "ui.readiness.confirmation" },
  { key: "consistency", label: "ui.readiness.consistency" },
  { key: "verifiability", label: "ui.readiness.verifiability" },
];

/** Markdown emphasis markers the importer keeps in a passage (shown as plain text). */
function plainPassage(text: string): string {
  return text.replace(/\*\*|__/g, "");
}

function Section({ label, children }: { label: string; children: ReactNode }) {
  return (
    <section aria-label={label} className="flex flex-col">
      <h2 className="m-0 mb-2 text-caption font-semibold text-fg-2">{label}</h2>
      {children}
    </section>
  );
}

type OpenSheet = { kind: "kind" } | { kind: "split" } | { kind: "duplicate" } | { kind: "waive"; finding: Finding } | null;

export interface RequirementInspectorProps {
  readonly id: string;
  /** Every requirement (the duplicate picker offers the others). */
  readonly requirements: readonly RequirementSummary[];
  readonly sources: readonly SourceListItem[];
}

/** The selected requirement: statement, wording suggestion, readiness, issues, evidence and links. */
export function RequirementInspector({ id, requirements, sources }: RequirementInspectorProps) {
  const detail = useQuery(requirementQuery(id));
  const readiness = useQuery(readinessQuery(id));
  const findings = useQuery(findingsQuery);
  const gates = useQuery(gatesQuery);
  const me = useMe();
  const go = useGo();
  const queryClient = useQueryClient();
  const [sheet, setSheet] = useState<OpenSheet>(null);
  const etag = detail.data?.etag;
  const path = { id };

  const acceptWording = useVersionedMutation<WriteResult, string>(
    (text, ifMatch) => apiClient.POST("/api/requirements/{id}/accept-ears", { params: { path, header: { "If-Match": ifMatch } }, body: { text } }),
    etag,
  );
  const waive = useVersionedMutation<WriteResult, { finding_id: string; reason: string }>(
    (body, ifMatch) => apiClient.POST("/api/requirements/{id}/waive-finding", { params: { path, header: { "If-Match": ifMatch } }, body }),
    etag,
  );

  if (detail.isError) {
    return <ReadFailure error={detail.error} messageKey="requirement.loadFailed" />;
  }
  if (detail.data === undefined) {
    return (
      <SkeletonGroup className="flex flex-col gap-3">
        <Skeleton width="40%" />
        <Skeleton width="90%" height={18} />
        <Skeleton width="70%" height={18} />
      </SkeletonGroup>
    );
  }
  const requirement = detail.data;
  const label = requirementLabel(requirement);
  const canEdit = hasRole(me.data, ANALYST);
  const accepted = requirement.status === "Accepted";
  const editable = accepted || requirement.status === "Proposed";
  const waived = waivedFindingIds(gates.data);
  const issues = issuesOf(requirement.id, findings.data, waived);
  const pill = pillStatus(requirement.status);
  const written = (message: string) => {
    void queryClient.invalidateQueries();
    toast({ message });
  };
  const failure = (error: Error | null) => {
    if (error === null || error.name === "PreconditionFailedError") {
      return undefined;
    }
    return tf("requirement.writeFailed", { message: writeErrorMessage(error) ?? error.message });
  };

  const moreItems: MenuItem[] = [
    ...(editable ? [{ id: "kind", label: t("requirement.action.changeKind"), onSelect: () => setSheet({ kind: "kind" }) }] : []),
    ...(accepted
      ? [
          { id: "split", label: t("requirement.action.split"), onSelect: () => setSheet({ kind: "split" }) },
          { id: "duplicate", label: t("requirement.action.duplicate"), onSelect: () => setSheet({ kind: "duplicate" }) },
        ]
      : []),
  ];

  return (
    <>
      <div className="flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <MonoId title={requirement.id}>{label}</MonoId>
          <KindTag kind={requirement.class} withIcon={false} className="h-[22px] px-2" />
          {pill === null ? <StatusPill status="proposed" label={requirement.status} /> : <StatusPill status={pill} />}
          {canEdit && moreItems.length > 0 && (
            <span className="ml-auto">
              <Menu
                width={220}
                items={moreItems}
                trigger={<IconButton label={t("requirement.more")} size={30} icon={<MoreHorizontal size={18} strokeWidth={1.6} aria-hidden="true" />} />}
              />
            </span>
          )}
        </div>
        <p className="m-0 select-text text-title-sm font-medium leading-[1.45] text-fg [text-wrap:pretty]">{requirement.text}</p>
      </div>

      {(requirement.ears_suggestion ?? null) !== null && (
        <section aria-label={t("requirement.wording.title")} className="rounded-tile border border-line p-3.5">
          <h2 className="m-0 mb-1.5 text-caption font-semibold text-fg-2">{t("requirement.wording.title")}</h2>
          <p className="m-0 select-text text-footnote leading-normal text-fg">{requirement.ears_suggestion}</p>
          {canEdit && editable && (
            <Button
              variant="primary"
              size={30}
              className="mt-2.5"
              pending={acceptWording.isPending}
              onClick={() =>
                acceptWording.mutate(requirement.ears_suggestion ?? "", {
                  onSuccess: () => written(tf("requirement.wording.accepted", { id: label })),
                  onError: (error) => {
                    const message = failure(error);
                    if (message !== undefined) {
                      toast({ tone: "error", message });
                    }
                  },
                })
              }
            >
              {t("requirement.wording.accept")}
            </Button>
          )}
        </section>
      )}

      <ReadinessSection value={readiness.data?.overall ?? requirement.readiness ?? null} sub={readiness.data?.sub} />

      {(issues.length > 0 || requirement.smells.length > 0) && (
        <Section label={t("requirement.issues.title")}>
          <ul className="m-0 flex list-none flex-col gap-1.5 p-0">
            {issues.map((finding) => (
              <IssueRow
                key={finding.id}
                message={finding.message}
                code={finding.code}
                action={
                  canEdit && accepted ? (
                    <Button variant="outline" size={30} className="h-[26px] px-2.5 text-eyebrow" onClick={() => setSheet({ kind: "waive", finding })}>
                      {t("requirement.issues.waive")}
                    </Button>
                  ) : null
                }
              />
            ))}
            {requirement.smells.map((smell) => (
              <IssueRow key={smell} message={t("requirement.issues.lint")} code={smell} action={null} />
            ))}
          </ul>
        </Section>
      )}

      <Section label={t("requirement.evidence.title")}>
        <EvidenceList requirement={requirement} sources={sources} onOpen={(sourceId) => go(buildHref("/sources/$id", { id: sourceId }))} />
      </Section>

      <Section label={t("requirement.linked.title")}>
        <LinkedChips requirement={requirement} onNavigate={go} />
      </Section>

      <ReasonSheet
        open={sheet?.kind === "waive"}
        onOpenChange={(open) => {
          if (!open) {
            setSheet(null);
            waive.reset();
          }
        }}
        title={t("requirement.waive.title")}
        description={t("requirement.waive.description")}
        reasonPlaceholder={t("requirement.waive.placeholder")}
        confirmLabel={t("requirement.waive.confirm")}
        pending={waive.isPending}
        error={failure(waive.error)}
        onConfirm={(reason) => {
          if (sheet?.kind !== "waive") {
            return;
          }
          waive.mutate(
            { finding_id: sheet.finding.id, reason },
            {
              onSuccess: () => {
                setSheet(null);
                written(t("requirement.waive.done"));
              },
              onError: (error) => {
                if (error.name === "PreconditionFailedError") {
                  setSheet(null);
                }
              },
            },
          );
        }}
      />
      {sheet?.kind === "kind" && <ChangeKindSheet requirement={requirement} label={label} onClose={() => setSheet(null)} onDone={written} />}
      {sheet?.kind === "split" && <SplitSheet requirement={requirement} label={label} onClose={() => setSheet(null)} onDone={written} />}
      {sheet?.kind === "duplicate" && (
        <DuplicateSheet requirement={requirement} label={label} candidates={requirements} onClose={() => setSheet(null)} onDone={written} />
      )}
    </>
  );
}

function ReadinessSection({ value, sub }: { value: number | null; sub: components["schemas"]["ReadinessSubScores"] | undefined }) {
  const pct = value === null ? null : Math.round(value);
  return (
    <Section label={t("requirement.readiness.title")}>
      <div className="flex items-center gap-4">
        <span className="relative inline-grid size-[72px] shrink-0 place-items-center">
          <ReadinessRing value={pct} size={72} stroke={6} />
          <span aria-hidden="true" className="absolute inset-0 flex items-center justify-center font-display text-title-sm font-bold leading-none tracking-display text-fg">
            {pct === null ? NO_VALUE : String(pct)}
          </span>
        </span>
        <div className="flex flex-1 flex-col gap-1.5">
          {SUB_SCORES.map(({ key, label }) => {
            const score = sub?.[key] ?? null;
            return (
              <div key={key} className="flex justify-between gap-3 text-caption">
                <span className="text-fg-2">{t(label)}</span>
                <span className="text-fg-3">{score === null ? t("requirement.readiness.notCalculated") : String(Math.round(score))}</span>
              </div>
            );
          })}
        </div>
      </div>
    </Section>
  );
}

function IssueRow({ message, code, action }: { message: string; code: string; action: ReactNode }) {
  return (
    <li className="flex items-start gap-2.5 rounded-field border border-line p-3">
      <TriangleAlert size={16} strokeWidth={1.6} aria-hidden="true" className="mt-px shrink-0 text-attention-fg" />
      <div className="min-w-0 flex-1">
        <div className="text-footnote font-medium text-fg">{message}</div>
        <div className="break-all font-mono text-eyebrow leading-relaxed text-fg-3">{code}</div>
      </div>
      {action}
    </li>
  );
}

function EvidenceList({ requirement, sources, onOpen }: { requirement: RequirementDetail; sources: readonly SourceListItem[]; onOpen: (sourceId: string) => void }) {
  const passages = requirement.evidence.filter((excerpt) => (excerpt.text ?? null) !== null);
  if (passages.length === 0) {
    return <p className="m-0 text-footnote text-fg-3">{t("requirement.evidence.none")}</p>;
  }
  return (
    <div className="flex flex-col gap-2">
      {passages.map((excerpt) => {
        const text = plainPassage(excerpt.text ?? "");
        const start = text.indexOf(requirement.text);
        const sourceRef = excerpt.source_ref ?? null;
        const source = sources.find((s) => s.id === sourceRef);
        return (
          <EvidenceQuote
            key={excerpt.id}
            text={text}
            highlight={start < 0 ? undefined : { start, end: start + requirement.text.length }}
            sourceId={requirement.source_identifier ?? undefined}
            sourceName={source?.name ?? sourceRef ?? undefined}
            onOpenSource={sourceRef === null ? undefined : () => onOpen(sourceRef)}
          />
        );
      })}
    </div>
  );
}

function LinkedChips({ requirement, onNavigate }: { requirement: RequirementDetail; onNavigate: (href: string) => void }) {
  const labelOf = (id: string) => requirement.labels.find((entry) => entry.id === id)?.label ?? id;
  const chips = [
    ...requirement.linked.operations.map((id) => ({ id, label: labelOf(id), icon: <Zap size={14} strokeWidth={1.6} aria-hidden="true" />, href: "/model/processes" })),
    ...requirement.linked.processes.map((id) => ({
      id,
      label: tf("requirement.linked.process", { name: labelOf(id) }),
      icon: <Workflow size={14} strokeWidth={1.6} aria-hidden="true" />,
      href: buildHref("/model/processes/$id", { id }),
    })),
    ...requirement.linked.scenarios.map((id) => ({
      id,
      label: tf("requirement.linked.scenario", { name: labelOf(id) }),
      icon: <FlaskConical size={14} strokeWidth={1.6} aria-hidden="true" />,
      href: buildHref("/scenarios/$id", { id }),
    })),
  ];
  if (chips.length === 0) {
    return <p className="m-0 text-footnote text-fg-3">{t("requirement.linked.none")}</p>;
  }
  return (
    <div className="flex flex-wrap gap-1.5">
      {chips.map((chip) => (
        <button
          key={chip.id}
          type="button"
          onClick={() => onNavigate(chip.href)}
          className="inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-pill border border-line bg-surface px-2.5 text-caption font-medium text-fg transition-colors duration-[120ms] ease-standard hover:bg-card"
        >
          {chip.icon}
          {chip.label}
        </button>
      ))}
    </div>
  );
}
