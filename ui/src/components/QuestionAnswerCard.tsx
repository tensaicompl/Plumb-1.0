// The old answer card and proposal-action helpers, kept for the old pages that still use them
// (the Assistant session panel, Rules, Scenarios) until their screen lanes replace them. The new
// Inbox (src/features/inbox) answers questions in its own inspector card.
import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { apiClient, readReviewed, revisionEtag, useReviewedEtag, type ReviewedItems } from "../api/client";
import type { components } from "../api/schema";
import { modelVersionQueryKey, useVersionedMutation, WriteFailedError, writeErrorMessage } from "../api/useVersionedMutation";
import { ProposalReviewBar, type ProposalReviewAction } from "./ProposalReviewBar";
import { isAnswerComplete, TypedAnswerControl, type AnswerValue } from "./TypedAnswerControl";
import { t } from "../i18n";
import type { ItemHealth } from "./HealthMarkers";

export type Question = components["schemas"]["Question"];
export type AnswerResponse = components["schemas"]["AnswerResponse"];
export type ProposalKind = components["schemas"]["ProposalKind"];
type QuestionPage = components["schemas"]["QuestionPage"];
type WriteResult = components["schemas"]["WriteResult"];

/** The query keys U2.2 reads; every successful U2.2 write invalidates them. */
export const questionsQueryKey = ["questions"] as const;
export const sentencesQueryKey = ["sentences"] as const;
export const entitiesQueryKey = ["entities"] as const;

/** Refetches the U2.2 server data and the current model version after a write. */
export async function invalidateU22(queryClient: QueryClient): Promise<void> {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: questionsQueryKey }),
    queryClient.invalidateQueries({ queryKey: sentencesQueryKey }),
    queryClient.invalidateQueries({ queryKey: entitiesQueryKey }),
    queryClient.invalidateQueries({ queryKey: modelVersionQueryKey }),
  ]);
}

/** GET /api/questions with optional exact filters and no page limit (all remaining items). */
export async function fetchQuestions(status?: Question["status"]): Promise<ReviewedItems<Question> & { readonly health?: ItemHealth[] }> {
  const page = await readReviewed(apiClient.GET("/api/questions", { params: { query: status === undefined ? {} : { status } } }));
  return { items: (page as QuestionPage).items, health: (page as Partial<QuestionPage>).health, etag: page.etag };
}

/** The server error code of a failed write, when the body carries one. */
export function writeErrorCode(error: unknown): string | null {
  if (error instanceof WriteFailedError && typeof error.body === "object" && error.body !== null) {
    const code = (error.body as Record<string, unknown>)["code"];
    return typeof code === "string" ? code : null;
  }
  return null;
}

/** A confirmation refused because the pending proposal's baseline revision is no longer the head. */
export function isBaselineChanged(error: unknown): boolean {
  return error instanceof WriteFailedError && error.status === 409 && writeErrorCode(error) === "E_PROPOSAL_BASELINE_CHANGED";
}

/**
 * Transport only (Hotfix 076): the existing proposal action endpoint for the server-returned
 * ProposalKind. It consumes the server's classification and never derives one; an unknown value
 * fails instead of guessing.
 */
export function proposalActionRequest(action: ProposalKind, id: string, reason: string | null, ifMatch: string) {
  const init = { params: { path: { id }, header: { "If-Match": ifMatch } }, body: { reason } };
  switch (action) {
    case "accept":
      return apiClient.POST("/api/proposals/{id}/accept", init);
    case "create":
      return apiClient.POST("/api/proposals/{id}/create", init);
    case "link":
      return apiClient.POST("/api/proposals/{id}/link", init);
    case "merge":
      return apiClient.POST("/api/proposals/{id}/merge", init);
    case "supersede":
      return apiClient.POST("/api/proposals/{id}/supersede", init);
    default: {
      const unknown: never = action;
      throw new Error("unsupported proposal action " + String(unknown));
    }
  }
}

/** The review-bar action of a server ProposalKind (the five values are shared). */
function reviewAction(action: ProposalKind): ProposalReviewAction {
  return action;
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

interface ProposalDecision {
  readonly kind: ProposalKind | "reject";
  readonly proposalId: string;
  readonly reason: string | null;
}

/**
 * Answers one persisted Question (Hotfix 076): the typed answer comes only from Question.kind and
 * answer_schema; POST /api/questions/{id}/answer returns a pending proposal, its server action, its
 * intake and the raw patch_preview, nothing committed. Confirmation uses only the endpoint of the
 * server-returned action (or reject) with the review-bar reason, which is distinct from the answer
 * rationale. Discard (H0.11) moves the persisted pending proposal to abandoned on the server, so no
 * unreachable pending proposal is left behind. A moved baseline discards the preview; nothing is
 * retried.
 */
export function QuestionAnswerCard({ question }: { question: Question }) {
  const queryClient = useQueryClient();
  const [value, setValue] = useState<AnswerValue>({});
  const [rationale, setRationale] = useState("");
  const [preview, setPreview] = useState<AnswerResponse | null>(null);
  const [stale, setStale] = useState(false);
  const [done, setDone] = useState(false);
  const [discarded, setDiscarded] = useState(false);

  const answer = useVersionedMutation<AnswerResponse, { answer: AnswerValue; rationale: string | null }>(
    (body, ifMatch) =>
      apiClient.POST("/api/questions/{id}/answer", {
        params: { path: { id: question.id }, header: { "If-Match": ifMatch } },
        body: { answer: body.answer as Record<string, never>, rationale: body.rationale, supersedes: null },
      }),
    useReviewedEtag(),
  );
  // A decision is based on the reviewed preview, which states its baseline revision.
  const decide = useVersionedMutation<WriteResult, ProposalDecision>(
    (decision, ifMatch) =>
      decision.kind === "reject"
        ? apiClient.POST("/api/proposals/{id}/reject", {
            params: { path: { id: decision.proposalId }, header: { "If-Match": ifMatch } },
            body: { reason: decision.reason },
          })
        : proposalActionRequest(decision.kind, decision.proposalId, decision.reason, ifMatch),
    preview === null ? null : revisionEtag(preview.revision),
  );
  // H0.11: the explicit discard of the persisted pending proposal (pending -> abandoned).
  const discard = useVersionedMutation<WriteResult, string>(
    (proposalId, ifMatch) =>
      apiClient.POST("/api/proposals/{id}/discard", {
        params: { path: { id: proposalId }, header: { "If-Match": ifMatch } },
        body: { reason: null },
      }),
    preview === null ? null : revisionEtag(preview.revision),
  );

  if (question.status !== "Open") {
    return (
      <p className="text-sm" data-testid="question-not-open">
        {t("question.notOpen")} <span className="font-mono">{question.status}</span>
      </p>
    );
  }

  const submitAnswer = () => {
    setStale(false);
    setDone(false);
    setDiscarded(false);
    decide.reset();
    discard.reset();
    answer.mutate(
      { answer: value, rationale: rationale === "" ? null : rationale },
      { onSuccess: (response) => setPreview(response) },
    );
  };

  const onAction = (action: ProposalReviewAction, reason: string) => {
    if (preview === null) {
      return;
    }
    let kind: ProposalKind | "reject";
    if (action === "reject") {
      kind = "reject";
    } else if (action === reviewAction(preview.action)) {
      kind = preview.action;
    } else {
      return;
    }
    decide.mutate(
      { kind, proposalId: preview.proposal_id, reason: reason === "" ? null : reason },
      {
        onSuccess: () => {
          setPreview(null);
          setValue({});
          setRationale("");
          setDone(true);
          void invalidateU22(queryClient);
        },
        onError: (error) => {
          if (isBaselineChanged(error)) {
            setPreview(null);
            setStale(true);
            void invalidateU22(queryClient);
          }
        },
      },
    );
  };

  const discardPreview = () => {
    if (preview === null) {
      return;
    }
    discard.mutate(preview.proposal_id, {
      onSuccess: () => {
        setPreview(null);
        setDiscarded(true);
        void invalidateU22(queryClient);
      },
    });
  };

  return (
    <div className="space-y-2" data-question-card={question.id}>
      {preview === null && (
        <>
          <TypedAnswerControl question={question} value={value} onChange={setValue} />
          <label className="block text-sm">
            {t("question.answerRationale")}
            <input className="ml-2 w-full rounded border p-1" value={rationale} data-testid="answer-rationale" onChange={(event) => setRationale(event.target.value)} />
          </label>
          <button
            type="button"
            className="rounded border px-3 py-1"
            data-testid="answer-submit"
            disabled={answer.isPending || !isAnswerComplete(question.answer_schema, value)}
            onClick={submitAnswer}
          >
            {t("question.answerSubmit")}
          </button>
        </>
      )}
      {stale && (
        <p role="alert" className="text-sm text-red-800" data-testid="answer-stale">
          {t("question.stale")}
        </p>
      )}
      {done && <p data-testid="answer-done">{t("question.decided")}</p>}
      {discarded && <p data-testid="answer-discarded">{t("question.discarded")}</p>}
      <WriteError error={answer.error} />
      {!stale && <WriteError error={decide.error} />}
      <WriteError error={discard.error} />
      {preview !== null && (
        <section aria-label={t("question.preview.title")} className="space-y-2 rounded border border-amber-600 p-3 text-sm" data-testid="answer-preview">
          <h4 className="font-semibold">{t("question.preview.title")}</h4>
          <p>{t("question.preview.notCommitted")}</p>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
            <dt>{t("preview.proposal")}</dt>
            <dd className="font-mono" data-testid="answer-proposal">
              {preview.proposal_id}
            </dd>
            <dt>{t("preview.action")}</dt>
            <dd className="font-mono" data-testid="answer-action">
              {preview.action}
            </dd>
          </dl>
          <h5 className="font-semibold">{t("question.preview.patch")}</h5>
          <pre className="overflow-x-auto rounded bg-slate-100 p-2 text-xs" data-testid="answer-patch-preview">
            {JSON.stringify(preview.patch_preview, null, 2)}
          </pre>
          <ProposalReviewBar
            status={t("question.preview.pending")}
            intake={preview.intake}
            enabledActions={[reviewAction(preview.action), "reject"]}
            onAction={onAction}
          />
          <button
            type="button"
            className="rounded border px-3 py-1"
            data-testid="answer-discard"
            disabled={discard.isPending || decide.isPending}
            onClick={discardPreview}
          >
            {t("question.preview.discard")}
          </button>
          <p className="text-xs text-slate-600">{t("question.preview.discardNote")}</p>
        </section>
      )}
    </div>
  );
}
