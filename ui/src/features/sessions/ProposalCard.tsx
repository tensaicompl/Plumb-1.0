import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { apiClient } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { ProposalReviewBar, type ProposalReviewAction } from "../../components/ProposalReviewBar";
import { t } from "../../i18n";
import { invalidateU22, isBaselineChanged, proposalActionRequest, writeErrorCode } from "../questions/QuestionsPage";

export type SessionProposalView = components["schemas"]["SessionProposalView"];
type ProposalKind = components["schemas"]["ProposalKind"];
type WriteResult = components["schemas"]["WriteResult"];

type CardState = "pending" | "resolved" | "stale";

interface Decision {
  readonly kind: ProposalKind | "reject";
  readonly reason: string | null;
}

/** The review-bar action of the server ProposalKind (the five values are shared). */
function reviewAction(kind: ProposalKind): ProposalReviewAction {
  return kind;
}

/**
 * One session proposal card (Hotfix 077/078): the exact server proposal id, kind, raw payload and the
 * intake the session already ran (never a second intake call). The only callable actions are the
 * server-classified kind and reject, committed through the proposal endpoints with {reason} only;
 * the shared ProposalReviewBar owns the reason gating. A moved baseline marks the card stale and
 * nothing is retried; a decided card is resolved locally.
 */
export function ProposalCard({ proposal }: { proposal: SessionProposalView }) {
  const queryClient = useQueryClient();
  const [state, setState] = useState<CardState>("pending");
  const decide = useVersionedMutation<WriteResult, Decision>((decision, ifMatch) =>
    decision.kind === "reject"
      ? apiClient.POST("/api/proposals/{id}/reject", {
          params: { path: { id: proposal.id }, header: { "If-Match": ifMatch } },
          body: { reason: decision.reason },
        })
      : proposalActionRequest(decision.kind, proposal.id, decision.reason, ifMatch),
  );

  const onAction = (action: ProposalReviewAction, rationale: string) => {
    if (state !== "pending" || decide.isPending) {
      return;
    }
    let kind: ProposalKind | "reject";
    if (action === "reject") {
      kind = "reject";
    } else if (action === reviewAction(proposal.kind)) {
      kind = proposal.kind;
    } else {
      return;
    }
    decide.mutate(
      { kind, reason: rationale === "" ? null : rationale },
      {
        onSuccess: () => {
          setState("resolved");
          void invalidateU22(queryClient);
        },
        onError: (error) => {
          if (isBaselineChanged(error)) {
            setState("stale");
            void invalidateU22(queryClient);
          }
        },
      },
    );
  };

  const actionable = state === "pending" && !decide.isPending;

  return (
    <article aria-label={t("session.proposal.title")} className="space-y-2 rounded border p-2 text-sm" data-proposal-id={proposal.id} data-state={state}>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4">
        <dt>{t("preview.proposal")}</dt>
        <dd className="font-mono" data-testid="card-proposal">
          {proposal.id}
        </dd>
        <dt>{t("preview.action")}</dt>
        <dd className="font-mono" data-testid="card-kind">
          {proposal.kind}
        </dd>
      </dl>
      <h4 className="font-semibold">{t("session.proposal.payload")}</h4>
      <pre className="overflow-x-auto rounded bg-slate-100 p-2 text-xs" data-testid="card-payload">
        {JSON.stringify(proposal.payload, null, 2)}
      </pre>
      {state === "resolved" && <p data-testid="card-resolved">{t("session.proposal.resolved")}</p>}
      {state === "stale" && (
        <p role="alert" className="text-red-800" data-testid="card-stale">
          {t("session.proposal.stale")}
        </p>
      )}
      {decide.error instanceof WriteFailedError && state === "pending" && (
        <p role="alert" className="text-red-800" data-testid="card-failed">
          {t("write.failed")} <span className="font-mono">{writeErrorCode(decide.error) ?? String(decide.error.status)}</span> <span>{writeErrorMessage(decide.error)}</span>
        </p>
      )}
      <ProposalReviewBar
        status={t(state === "pending" ? "question.preview.pending" : state === "resolved" ? "session.proposal.resolved" : "session.proposal.staleShort")}
        intake={proposal.intake}
        enabledActions={actionable ? [reviewAction(proposal.kind), "reject"] : []}
        onAction={onAction}
      />
    </article>
  );
}
