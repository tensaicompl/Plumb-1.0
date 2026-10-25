import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";

import { apiClient, readReviewed, type Reviewed } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { ReadFailure } from "../../components/ReadFailure";
import { RoleGate } from "../../components/RoleGate";
import { t, type MessageKey } from "../../i18n";
import { proposalActionRequest, writeErrorCode } from "../questions/QuestionsPage";

export type PendingProposal = components["schemas"]["PendingProposal"];
type PendingProposalList = components["schemas"]["PendingProposalList"];
type ProposalKind = components["schemas"]["ProposalKind"];
type WriteResult = components["schemas"]["WriteResult"];

/** The pending-proposal list query root. */
export const proposalsQueryKey = ["proposals"] as const;

function fetchProposals(): Promise<Reviewed<PendingProposalList>> {
  return readReviewed(apiClient.GET("/api/proposals"));
}

const ACTION_LABEL: Readonly<Record<ProposalKind, MessageKey>> = {
  accept: "proposal.action.accept",
  create: "proposal.action.create",
  link: "proposal.action.link",
  merge: "proposal.action.merge",
  supersede: "proposal.action.supersede",
};

/** One governance decision on a pending proposal: its server-class action, reject or discard. */
interface Decision {
  readonly verb: ProposalKind | "reject" | "discard";
  readonly reason: string | null;
}

/** The exact origin of a proposal: its origin and the references the server returned. */
function originText(proposal: PendingProposal): string {
  return [proposal.origin, proposal.origin_ref, proposal.recipe_job_id, proposal.stage_job_id]
    .filter((part): part is string => typeof part === "string")
    .join(" ");
}

function WriteError({ error }: { error: Error | null }) {
  if (error === null || !(error instanceof WriteFailedError)) {
    return null;
  }
  return (
    <p role="alert" className="text-sm text-red-800">
      {t("write.failed")} <span className="font-mono">{writeErrorCode(error) ?? String(error.status)}</span> <span>{writeErrorMessage(error)}</span>
    </p>
  );
}

/**
 * The governance controls of one pending proposal, against the reviewed list (its ETag is the
 * If-Match): the one positive action of the server class (never derived here), reject, and
 * discard when the server says the proposal is discardable. A stale proposal is never offered its
 * positive action, which the backend refuses (it is never rebased); a required reason gates it.
 */
function ProposalControls({ proposal, etag }: { proposal: PendingProposal; etag: string | null }) {
  const queryClient = useQueryClient();
  const reasonId = useId();
  const [reason, setReason] = useState("");
  const decide = useVersionedMutation<WriteResult, Decision>((decision, ifMatch) => {
    const init = { params: { path: { id: proposal.proposal_id }, header: { "If-Match": ifMatch } }, body: { reason: decision.reason } };
    switch (decision.verb) {
      case "reject":
        return apiClient.POST("/api/proposals/{id}/reject", init);
      case "discard":
        return apiClient.POST("/api/proposals/{id}/discard", init);
      default:
        return proposalActionRequest(decision.verb, proposal.proposal_id, decision.reason, ifMatch);
    }
  }, etag);
  const trimmed = reason.trim();
  const reasonMissing = proposal.requires_reason && trimmed === "";
  const run = (verb: Decision["verb"]) =>
    decide.mutate({ verb, reason: trimmed === "" ? null : trimmed }, { onSuccess: () => void queryClient.invalidateQueries() });

  return (
    <div className="space-y-1">
      <label htmlFor={reasonId} className="block text-xs">
        {t("proposals.reason")}
      </label>
      <input id={reasonId} className="w-full rounded border p-1 text-sm" value={reason} onChange={(event) => setReason(event.target.value)} />
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className="rounded border px-2 py-1 disabled:opacity-50"
          disabled={decide.isPending || proposal.stale || reasonMissing}
          onClick={() => run(proposal.action)}
        >
          {t(ACTION_LABEL[proposal.action])}
        </button>
        <button type="button" className="rounded border px-2 py-1 disabled:opacity-50" disabled={decide.isPending} onClick={() => run("reject")}>
          {t("proposal.action.reject")}
        </button>
        {proposal.discardable && (
          <button type="button" className="rounded border px-2 py-1 disabled:opacity-50" disabled={decide.isPending} onClick={() => run("discard")}>
            {t("proposals.discard")}
          </button>
        )}
      </div>
      {proposal.stale && (
        <p className="text-xs text-slate-600" data-testid="proposal-stale-note">
          {t(proposal.discardable ? "proposals.staleNote.discardable" : "proposals.staleNote")}
        </p>
      )}
      {!proposal.stale && reasonMissing && <p className="text-xs text-slate-600">{t("proposal.reasonRequired")}</p>}
      <WriteError error={decide.error} />
    </div>
  );
}

/**
 * H0.11 proposal discovery and governance (architecture §33.12): every pending workflow proposal
 * from GET /api/proposals, as the server reports it, with its governance controls. Accepted,
 * rejected and abandoned proposals are history and are not listed.
 */
export function ProposalsPage() {
  const proposals = useQuery({ queryKey: proposalsQueryKey, queryFn: fetchProposals });
  const title = <h1 className="text-xl font-semibold">{t("proposals.title")}</h1>;
  if (proposals.isError) {
    return (
      <div className="space-y-4">
        {title}
        <ReadFailure error={proposals.error} messageKey="proposals.loadFailed" />
      </div>
    );
  }
  if (proposals.data === undefined) {
    return (
      <div className="space-y-4">
        {title}
        <p>{t("proposals.loading")}</p>
      </div>
    );
  }
  const { items, etag } = proposals.data;
  return (
    <div className="space-y-4">
      {title}
      <p className="text-sm">{t("proposals.intro")}</p>
      <RoleGate>
        <p className="text-sm">{t("proposals.governHint")}</p>
      </RoleGate>
      {items.length === 0 ? (
        <p>{t("proposals.empty")}</p>
      ) : (
        <table className="w-full text-left text-sm">
          <caption className="text-left font-semibold">{t("proposals.caption")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("proposals.column.proposal")}</th>
              <th scope="col">{t("proposals.column.action")}</th>
              <th scope="col">{t("proposals.column.origin")}</th>
              <th scope="col">{t("proposals.column.base")}</th>
              <th scope="col">{t("proposals.column.state")}</th>
              <th scope="col">{t("proposals.column.reason")}</th>
              <th scope="col">{t("proposals.column.decide")}</th>
            </tr>
          </thead>
          <tbody>
            {items.map((proposal) => (
              <tr key={proposal.proposal_id + "@" + proposal.base_revision} data-proposal-id={proposal.proposal_id} className="border-t align-top">
                <td className="font-mono">{proposal.proposal_id}</td>
                <td className="font-mono" data-field="action">
                  {proposal.action}
                </td>
                <td className="font-mono" data-field="origin">
                  {originText(proposal)}
                </td>
                <td className="font-mono" data-field="base_revision">
                  {proposal.base_revision}
                </td>
                <td data-field="state">{t(proposal.stale ? "proposals.state.stale" : "proposals.state.current")}</td>
                <td data-field="requires_reason">{t(proposal.requires_reason ? "proposal.yes" : "proposal.no")}</td>
                <td>
                  <RoleGate quiet>
                    <ProposalControls proposal={proposal} etag={etag} />
                  </RoleGate>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
