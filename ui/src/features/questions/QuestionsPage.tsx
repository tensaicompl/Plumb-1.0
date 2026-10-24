import { useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { apiClient } from "../../api/client";
import type { components } from "../../api/schema";
import { modelVersionQueryKey, useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { ProposalReviewBar, type ProposalReviewAction } from "../../components/ProposalReviewBar";
import { isAnswerComplete, TypedAnswerControl, type AnswerValue } from "../../components/TypedAnswerControl";
import { t } from "../../i18n";

export type Question = components["schemas"]["Question"];
export type AnswerResponse = components["schemas"]["AnswerResponse"];
export type ProposalKind = components["schemas"]["ProposalKind"];
type QuestionPage = components["schemas"]["QuestionPage"];
type WriteResult = components["schemas"]["WriteResult"];
type CreateRoundResponse = components["schemas"]["CreateRoundResponse"];

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
export async function fetchQuestions(status?: Question["status"]): Promise<Question[]> {
  const { data, response } = await apiClient.GET("/api/questions", {
    params: { query: status === undefined ? {} : { status } },
  });
  if (data === undefined) {
    throw new Error("GET /api/questions failed with HTTP " + String(response.status));
  }
  return (data as QuestionPage).items;
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
 * rationale. A moved baseline discards the preview; nothing is retried.
 */
export function QuestionAnswerCard({ question }: { question: Question }) {
  const queryClient = useQueryClient();
  const [value, setValue] = useState<AnswerValue>({});
  const [rationale, setRationale] = useState("");
  const [preview, setPreview] = useState<AnswerResponse | null>(null);
  const [stale, setStale] = useState(false);
  const [done, setDone] = useState(false);

  const answer = useVersionedMutation<AnswerResponse, { answer: AnswerValue; rationale: string | null }>((body, ifMatch) =>
    apiClient.POST("/api/questions/{id}/answer", {
      params: { path: { id: question.id }, header: { "If-Match": ifMatch } },
      body: { answer: body.answer as Record<string, never>, rationale: body.rationale, supersedes: null },
    }),
  );
  const decide = useVersionedMutation<WriteResult, ProposalDecision>((decision, ifMatch) =>
    decision.kind === "reject"
      ? apiClient.POST("/api/proposals/{id}/reject", {
          params: { path: { id: decision.proposalId }, header: { "If-Match": ifMatch } },
          body: { reason: decision.reason },
        })
      : proposalActionRequest(decision.kind, decision.proposalId, decision.reason, ifMatch),
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
    decide.reset();
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
      <WriteError error={answer.error} />
      {!stale && <WriteError error={decide.error} />}
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
        </section>
      )}
    </div>
  );
}

/** The analyst round manager (Hotfix 076): explicit stakeholder and integer max in 1..=15; the server composes. */
export const ROUND_MAX = 15;

/** The explicit round size, or null when the input is not an integer in 1..=ROUND_MAX. */
export function parseRoundMax(input: string): number | null {
  if (!/^[0-9]+$/.test(input)) {
    return null;
  }
  const value = Number(input);
  return value >= 1 && value <= ROUND_MAX ? value : null;
}

function RoundManager({ stakeholders }: { stakeholders: readonly string[] }) {
  const queryClient = useQueryClient();
  const [stakeholder, setStakeholder] = useState("");
  const [max, setMax] = useState("");
  const [round, setRound] = useState<CreateRoundResponse["round"] | null>(null);
  const create = useVersionedMutation<CreateRoundResponse, { stakeholder: string; max: number }>((body, ifMatch) =>
    apiClient.POST("/api/rounds", { params: { header: { "If-Match": ifMatch } }, body }),
  );
  const parsed = parseRoundMax(max);
  const invalid = max !== "" && parsed === null;

  return (
    <form
      aria-label={t("rounds.title")}
      className="space-y-2 rounded border p-3"
      onSubmit={(event) => {
        event.preventDefault();
        if (stakeholder === "" || parsed === null) {
          return;
        }
        create.mutate(
          { stakeholder, max: parsed },
          {
            onSuccess: (response) => {
              setRound(response.round);
              void invalidateU22(queryClient);
            },
          },
        );
      }}
    >
      <h2 className="font-semibold">{t("rounds.title")}</h2>
      <label className="block text-sm">
        {t("rounds.stakeholder")}
        <select className="ml-2 rounded border" value={stakeholder} data-testid="round-stakeholder" onChange={(event) => setStakeholder(event.target.value)}>
          <option value="">{t("rounds.chooseStakeholder")}</option>
          {stakeholders.map((ref) => (
            <option key={ref} value={ref}>
              {ref}
            </option>
          ))}
        </select>
      </label>
      <label className="block text-sm">
        {t("rounds.max")}
        <input
          type="number"
          min={1}
          max={ROUND_MAX}
          step={1}
          className="ml-2 rounded border p-1"
          value={max}
          aria-invalid={invalid}
          data-testid="round-max"
          onChange={(event) => setMax(event.target.value)}
        />
      </label>
      {invalid && (
        <p role="alert" className="text-sm text-red-800" data-testid="round-max-invalid">
          {t("rounds.maxInvalid")}
        </p>
      )}
      <button type="submit" className="rounded border px-3 py-1" data-testid="round-submit" disabled={create.isPending || stakeholder === "" || parsed === null}>
        {t("rounds.compose")}
      </button>
      <WriteError error={create.error} />
      {round !== null && (
        <section aria-label={t("rounds.result")} data-testid="round-result" className="text-sm">
          <h3 className="font-semibold">{t("rounds.result")}</h3>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4">
            <dt>{t("rounds.id")}</dt>
            <dd className="font-mono" data-testid="round-id">
              {round.id}
            </dd>
            <dt>{t("rounds.stakeholder")}</dt>
            <dd className="font-mono">{round.stakeholder}</dd>
          </dl>
          {round.questions.length === 0 ? (
            <p>{t("rounds.empty")}</p>
          ) : (
            <ol className="font-mono" data-testid="round-questions">
              {round.questions.map((id) => (
                <li key={id}>{id}</li>
              ))}
            </ol>
          )}
        </section>
      )}
    </form>
  );
}

/** One Question with its exact server fields; context refs are shown as exact IDs. */
function QuestionItem({ question }: { question: Question }) {
  return (
    <li className="space-y-2 rounded border p-3" data-question-id={question.id}>
      <p data-testid="question-text">{question.text}</p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 text-sm">
        <dt>{t("question.id")}</dt>
        <dd className="font-mono">{question.id}</dd>
        <dt>{t("question.kind")}</dt>
        <dd className="font-mono" data-field="kind">
          {question.kind}
        </dd>
        <dt>{t("question.status")}</dt>
        <dd className="font-mono" data-field="status">
          {question.status}
        </dd>
        <dt>{t("question.priority")}</dt>
        <dd data-field="priority">{question.priority === null ? t("question.none") : String(question.priority)}</dd>
        <dt>{t("question.round")}</dt>
        <dd className="font-mono" data-field="round">
          {question.round_ref ?? t("question.none")}
        </dd>
        <dt>{t("question.context")}</dt>
        <dd>
          {question.context_refs.length === 0 ? (
            t("question.none")
          ) : (
            <ul className="font-mono" data-field="context">
              {question.context_refs.map((ref) => (
                <li key={ref}>{ref}</li>
              ))}
            </ul>
          )}
        </dd>
      </dl>
      <QuestionAnswerCard question={question} />
    </li>
  );
}

/** The stakeholder Question inbox and the analyst round manager (v2 U5 as refined by Hotfix 076). */
export function QuestionsPage() {
  const questions = useQuery({ queryKey: [...questionsQueryKey, "inbox"], queryFn: () => fetchQuestions() });

  if (questions.isError) {
    return <p role="alert">{t("questions.loadFailed")}</p>;
  }
  if (questions.data === undefined) {
    return <p>{t("questions.loading")}</p>;
  }
  // Groups by exact stakeholder_ref in server order of first appearance; unassigned last.
  const groups = new Map<string, Question[]>();
  const unassigned: Question[] = [];
  for (const question of questions.data) {
    if (question.stakeholder_ref === null) {
      unassigned.push(question);
    } else {
      const group = groups.get(question.stakeholder_ref) ?? [];
      group.push(question);
      groups.set(question.stakeholder_ref, group);
    }
  }

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-semibold">{t("questions.title")}</h1>
      <RoundManager stakeholders={[...groups.keys()]} />
      {questions.data.length === 0 && <p>{t("questions.empty")}</p>}
      {[...groups.entries()].map(([stakeholder, items]) => (
        <section key={stakeholder} aria-label={stakeholder} data-stakeholder={stakeholder} className="space-y-2">
          <h2 className="font-semibold">
            {t("questions.stakeholder")} <span className="font-mono">{stakeholder}</span>
          </h2>
          <ul className="space-y-2">
            {items.map((question) => (
              <QuestionItem key={question.id} question={question} />
            ))}
          </ul>
        </section>
      ))}
      {unassigned.length > 0 && (
        <section aria-label={t("questions.unassigned")} data-stakeholder="" className="space-y-2">
          <h2 className="font-semibold">{t("questions.unassigned")}</h2>
          <ul className="space-y-2">
            {unassigned.map((question) => (
              <QuestionItem key={question.id} question={question} />
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
