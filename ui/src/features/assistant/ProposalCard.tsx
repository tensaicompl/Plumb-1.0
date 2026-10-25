import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { apiClient, readData, revisionEtag } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { Button, Eyebrow, ReasonSheet, Skeleton, toast } from "../../components/ui";
import { t, tf, type MessageKey } from "../../i18n";
import { useAssistantStore, type SessionProposalView } from "./store";

type ProposalElementKind = components["schemas"]["ProposalElementKind"];

const KIND_LABEL: Readonly<Record<ProposalElementKind, MessageKey>> = {
  operation: "assistant.kind.operation",
  event: "assistant.kind.event",
  outcome: "assistant.kind.outcome",
  requirement: "assistant.kind.requirement",
  glossary_term: "assistant.kind.glossaryTerm",
  entity: "assistant.kind.entity",
  attribute: "assistant.kind.attribute",
  state: "assistant.kind.state",
  transition: "assistant.kind.transition",
  invariant: "assistant.kind.invariant",
  calculation: "assistant.kind.calculation",
  scenario: "assistant.kind.scenario",
  rule: "assistant.kind.rule",
  decision_table: "assistant.kind.decisionTable",
  process: "assistant.kind.process",
  link: "assistant.kind.link",
  merge: "assistant.kind.merge",
  supersede: "assistant.kind.supersede",
  answer: "assistant.kind.answer",
  directory_element: "assistant.kind.directoryElement",
  other: "assistant.kind.other",
};

/** The facts shown on one line of a proposal card. */
const CARD_FACTS = 2;

/** The separator between facts on one line (punctuation, not copy). */
const SEPARATOR = " · ";

type Sheet = "accept" | "reject" | null;

/**
 * A proposal the Assistant made, inline in the conversation: what it adds (from the pending
 * proposal) with Accept and Reject. Accept commits it and confirms what it created
 * (`accept-set`); Reject records the rejection. Both run against the revision of the turn that
 * made it; a reason is asked for when the proposal's intake requires one.
 */
export function ProposalCard({ proposal, revision }: { proposal: SessionProposalView; revision: string }) {
  const queryClient = useQueryClient();
  const decision = useAssistantStore((state) => state.decisions[proposal.id]);
  const decide = useAssistantStore((state) => state.decide);
  const [sheet, setSheet] = useState<Sheet>(null);
  const detail = useQuery({
    queryKey: ["assistant", "proposal", proposal.id],
    queryFn: () => readData(apiClient.GET("/api/proposals/{id}", { params: { path: { id: proposal.id } } })),
    enabled: decision === undefined,
    retry: false,
  });
  const reviewed = revisionEtag(revision);
  const accept = useVersionedMutation(
    (reason: string | null, ifMatch: string) =>
      apiClient.POST("/api/proposals/accept-set", { params: { header: { "If-Match": ifMatch } }, body: { proposal_ids: [proposal.id], reason, confirm: true } }),
    reviewed,
  );
  const reject = useVersionedMutation(
    (reason: string | null, ifMatch: string) => apiClient.POST("/api/proposals/{id}/reject", { params: { path: { id: proposal.id }, header: { "If-Match": ifMatch } }, body: { reason } }),
    reviewed,
  );
  const requiresReason = proposal.intake.requires_reason || detail.data?.requires_reason === true;
  const title = detail.data?.title ?? null;
  const kind = detail.data === undefined ? null : t(KIND_LABEL[detail.data.element_kind]);
  const facts = (detail.data?.facts ?? []).slice(0, CARD_FACTS).map((f) => f.label + " " + f.value);
  const pending = accept.isPending || reject.isPending;
  const failure = accept.error ?? reject.error;

  const onDecided = (outcome: "accepted" | "rejected") => {
    setSheet(null);
    decide(proposal.id, outcome);
    void queryClient.invalidateQueries();
    toast(
      outcome === "accepted"
        ? { message: tf("assistant.proposal.addedToast", { title: title ?? t("assistant.proposal.fallbackTitle") }) }
        : { tone: "info", message: t("assistant.proposal.rejectedToast") },
    );
  };
  const runAccept = (reason: string | null) => accept.mutate(reason, { onSuccess: () => onDecided("accepted") });
  const runReject = (reason: string | null) => reject.mutate(reason, { onSuccess: () => onDecided("rejected") });

  return (
    <article aria-label={title ?? t("assistant.proposal.fallbackTitle")} data-proposal-id={proposal.id} className="rounded-tile border border-line bg-card p-3.5 shadow-sm">
      <Eyebrow className="mb-2 text-[10px]">{kind === null ? t("assistant.proposal.eyebrowPlain") : tf("assistant.proposal.eyebrow", { kind })}</Eyebrow>
      {detail.data === undefined && detail.isPending && decision === undefined ? (
        <span className="flex flex-col gap-1.5">
          <Skeleton width="60%" height={14} />
          <Skeleton width="85%" height={10} />
        </span>
      ) : (
        <>
          <p className="m-0 text-body font-semibold text-fg">{title ?? t("assistant.proposal.fallbackTitle")}</p>
          {facts.length > 0 && <p className="m-0 mt-1 text-caption text-fg-2">{facts.join(SEPARATOR)}</p>}
        </>
      )}
      {decision === undefined ? (
        <>
          <div className="mt-3 flex gap-1.5">
            <Button variant="primary" size={30} pending={accept.isPending} disabled={pending} onClick={() => (requiresReason ? setSheet("accept") : runAccept(null))}>
              {t("assistant.proposal.accept")}
            </Button>
            <Button variant="outline" size={30} disabled={pending} onClick={() => setSheet("reject")}>
              {t("assistant.proposal.reject")}
            </Button>
          </div>
          {failure instanceof WriteFailedError && sheet === null && (
            <p role="alert" className="m-0 mt-2 text-caption text-blocked-fg">
              {writeErrorMessage(failure) ?? t("assistant.proposal.failed")}
            </p>
          )}
        </>
      ) : (
        <p className={decision === "accepted" ? "m-0 mt-2.5 text-caption font-semibold text-pass-fg" : "m-0 mt-2.5 text-caption font-semibold text-fg-3"}>
          {t(decision === "accepted" ? "assistant.proposal.accepted" : "assistant.proposal.rejected")}
        </p>
      )}
      <ReasonSheet
        open={sheet !== null}
        onOpenChange={(open) => {
          if (!open) {
            setSheet(null);
          }
        }}
        title={t(sheet === "accept" ? "assistant.proposal.acceptTitle" : "assistant.proposal.rejectTitle")}
        description={tf(sheet === "accept" ? "assistant.proposal.acceptBody" : "assistant.proposal.rejectBody", { title: title ?? t("assistant.proposal.fallbackTitle") })}
        confirmLabel={t(sheet === "accept" ? "assistant.proposal.accept" : "assistant.proposal.reject")}
        destructive={sheet === "reject"}
        reasonRequired={requiresReason}
        reasonPlaceholder={t(requiresReason ? "assistant.proposal.reasonRequired" : "assistant.proposal.reasonOptional")}
        pending={pending}
        error={failure instanceof WriteFailedError ? (writeErrorMessage(failure) ?? t("assistant.proposal.failed")) : undefined}
        onConfirm={(reason) => (sheet === "accept" ? runAccept(reason === "" ? null : reason) : runReject(reason === "" ? null : reason))}
      />
    </article>
  );
}
