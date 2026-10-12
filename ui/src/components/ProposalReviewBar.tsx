import { useId, useState } from "react";

import type { components } from "../api/schema";
import { t, type MessageKey } from "../i18n";

export type IntakeReport = components["schemas"]["IntakeReportView"];

/** The proposal review actions this bar can offer. */
export type ProposalReviewAction = "accept" | "edit" | "reject" | "link" | "merge" | "supersede" | "create" | "explain";

export const PROPOSAL_REVIEW_ACTIONS: readonly ProposalReviewAction[] = [
  "accept",
  "create",
  "link",
  "merge",
  "supersede",
  "edit",
  "reject",
  "explain",
];

const ACTION_LABEL: Readonly<Record<ProposalReviewAction, MessageKey>> = {
  accept: "proposal.action.accept",
  create: "proposal.action.create",
  link: "proposal.action.link",
  merge: "proposal.action.merge",
  supersede: "proposal.action.supersede",
  edit: "proposal.action.edit",
  reject: "proposal.action.reject",
  explain: "proposal.action.explain",
};

/** Actions that commit the proposal and therefore need a rationale when intake requires one. */
const REASON_GATED: ReadonlySet<ProposalReviewAction> = new Set(["accept", "create"]);

/** Whether `action` can be used: offered by the caller and, for accept/create, reasoned when required. */
export function isActionEnabled(
  action: ProposalReviewAction,
  enabledActions: readonly ProposalReviewAction[],
  intake: IntakeReport,
  rationale: string,
): boolean {
  if (!enabledActions.includes(action)) {
    return false;
  }
  return !(REASON_GATED.has(action) && intake.requires_reason && rationale.trim() === "");
}

/**
 * Review controls for one proposal. The server intake report is shown as-is (never recomputed);
 * which actions exist comes from the caller, and the bar only reports a chosen action and the
 * rationale through `onAction`. It calls no API.
 */
export function ProposalReviewBar({
  status,
  intake,
  enabledActions,
  onAction,
  initialRationale = "",
}: {
  status: string;
  intake: IntakeReport;
  enabledActions: readonly ProposalReviewAction[];
  onAction: (action: ProposalReviewAction, rationale: string) => void;
  initialRationale?: string;
}) {
  const [rationale, setRationale] = useState(initialRationale);
  const rationaleId = useId();
  const reasonMissing = intake.requires_reason && rationale.trim() === "";

  return (
    <section aria-label={t("proposal.title")} className="space-y-3 rounded border p-3 text-sm">
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
        <dt>{t("proposal.status")}</dt>
        <dd data-testid="proposal-status">{status}</dd>
        <dt>{t("proposal.requiresReason")}</dt>
        <dd data-testid="proposal-requires-reason">{t(intake.requires_reason ? "proposal.yes" : "proposal.no")}</dd>
        <dt>{t("proposal.reasonCodes")}</dt>
        <dd>
          {intake.reason_codes.length === 0 ? (
            t("proposal.none")
          ) : (
            <ul className="font-mono" data-testid="proposal-reason-codes">
              {intake.reason_codes.map((code) => (
                <li key={code}>{code}</li>
              ))}
            </ul>
          )}
        </dd>
        <dt>{t("proposal.novelty")}</dt>
        <dd data-testid="proposal-novelty">{intake.novelty ?? t("proposal.notCalculated")}</dd>
      </dl>

      {intake.validation_findings.length > 0 && (
        <section aria-label={t("proposal.validationFindings")}>
          <h3 className="font-semibold">{t("proposal.validationFindings")}</h3>
          <ul>
            {intake.validation_findings.map((finding) => (
              <li key={finding.id}>
                <span className="font-mono">{finding.code}</span> {finding.message}
              </li>
            ))}
          </ul>
        </section>
      )}
      {intake.conflicts_with.length > 0 && (
        <section aria-label={t("proposal.conflictsWith")}>
          <h3 className="font-semibold">{t("proposal.conflictsWith")}</h3>
          <ul>
            {intake.conflicts_with.map((conflict) => (
              <li key={conflict.target_ref + conflict.reason}>
                <span className="font-mono">{conflict.target_ref}</span> {conflict.reason}
              </li>
            ))}
          </ul>
        </section>
      )}
      {intake.duplicate_candidates.length > 0 && (
        <section aria-label={t("proposal.duplicateCandidates")}>
          <h3 className="font-semibold">{t("proposal.duplicateCandidates")}</h3>
          <ul>
            {intake.duplicate_candidates.map((d) => (
              <li key={d.subject_ref + d.candidate_ref}>
                <span className="font-mono">{d.candidate_ref}</span> {d.score}
              </li>
            ))}
          </ul>
        </section>
      )}
      {intake.covered_by.length > 0 && (
        <section aria-label={t("proposal.coveredBy")}>
          <h3 className="font-semibold">{t("proposal.coveredBy")}</h3>
          <ul>
            {intake.covered_by.map((c) => (
              <li key={c.subject_ref + c.covered_by_ref}>
                <span className="font-mono">{c.covered_by_ref}</span> {c.score}
              </li>
            ))}
          </ul>
        </section>
      )}
      {intake.refines.length > 0 && (
        <section aria-label={t("proposal.refines")}>
          <h3 className="font-semibold">{t("proposal.refines")}</h3>
          <ul className="font-mono">
            {intake.refines.map((ref) => (
              <li key={ref}>{ref}</li>
            ))}
          </ul>
        </section>
      )}
      {intake.new_terms.length > 0 && (
        <section aria-label={t("proposal.newTerms")}>
          <h3 className="font-semibold">{t("proposal.newTerms")}</h3>
          <ul>
            {intake.new_terms.map((term) => (
              <li key={term}>{term}</li>
            ))}
          </ul>
        </section>
      )}

      <div>
        <label htmlFor={rationaleId} className="block font-semibold">
          {t("proposal.rationale")}
        </label>
        <textarea
          id={rationaleId}
          value={rationale}
          onChange={(event) => setRationale(event.target.value)}
          aria-invalid={reasonMissing}
          className="w-full rounded border p-1"
        />
        {reasonMissing && <p className="text-xs text-red-800">{t("proposal.reasonRequired")}</p>}
      </div>

      <div className="flex flex-wrap gap-2">
        {PROPOSAL_REVIEW_ACTIONS.filter((action) => enabledActions.includes(action) || REASON_GATED.has(action)).map(
          (action) => {
            const enabled = isActionEnabled(action, enabledActions, intake, rationale);
            return (
              <button
                key={action}
                type="button"
                disabled={!enabled}
                data-action={action}
                onClick={() => {
                  if (enabled) {
                    onAction(action, rationale);
                  }
                }}
                className="rounded border px-3 py-1 disabled:opacity-50"
              >
                {t(ACTION_LABEL[action])}
              </button>
            );
          },
        )}
      </div>
    </section>
  );
}
