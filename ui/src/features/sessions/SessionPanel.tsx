import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";

import { apiClient, readReviewed, revisionEtag, ReviewedEtag, type Reviewed } from "../../api/client";
import type { components } from "../../api/schema";
import { modelVersionQueryKey, useModelVersion, useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { t } from "../../i18n";
import { fetchQuestions, QuestionAnswerCard, questionsQueryKey, writeErrorCode } from "../questions/QuestionsPage";
import { useMe } from "../../app/auth";
import { ProposalCard } from "./ProposalCard";

type SessionStage = components["schemas"]["SessionStage"];
type SessionTurnResponse = components["schemas"]["SessionTurnResponse"];
type CreateSessionResponse = components["schemas"]["CreateSessionResponse"];
type SessionMetrics = components["schemas"]["SessionMetrics"];
type RequirementDetail = components["schemas"]["RequirementDetail"];

/** The explicit session stages (SessionStage S0..S12); there is no default stage. */
export const SESSION_STAGES: readonly SessionStage[] = ["S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10", "S11", "S12"];

/** The project room (/sessions, scope []) or the requirement-scoped side panel (scope [requirementId]). */
export type SessionPanelProps = { readonly mode: "project" } | { readonly mode: "requirement"; readonly requirementId: string };

/** One successful turn of this mounted panel; transient view state, never canonical server data. */
interface LocalTurn {
  readonly key: number;
  readonly text: string;
  readonly response: SessionTurnResponse;
}

function fetchRequirement(id: string): Promise<Reviewed<RequirementDetail>> {
  return readReviewed(apiClient.GET("/api/requirements/{id}", { params: { path: { id } } }));
}

function fetchMetrics(id: string): Promise<SessionMetrics> {
  return readReviewed(apiClient.GET("/api/sessions/{id}/metrics", { params: { path: { id } } }));
}

function sessionMetricsKey(sessionId: string) {
  return ["sessions", sessionId, "metrics"] as const;
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

/** The engine Question of a turn: only an exact existing Open Question is actionable. */
function EngineQuestion({ questionId }: { questionId: string }) {
  const open = useQuery({ queryKey: [...questionsQueryKey, "open"], queryFn: () => fetchQuestions("Open") });
  const question = open.data?.items.find((q) => q.id === questionId);
  return (
    <section aria-label={t("session.engineQuestion")} className="space-y-1 rounded border p-2" data-asked-question={questionId}>
      <h4 className="font-semibold">
        {t("session.engineQuestion")} <span className="font-mono">{questionId}</span>
      </h4>
      {open.data === undefined && !open.isError && <p>{t("questions.loading")}</p>}
      {(open.isError || (open.data !== undefined && question === undefined)) && (
        <p data-testid="engine-question-unavailable">
          {t("session.questionUnavailable")} <span className="font-mono">{questionId}</span>
        </p>
      )}
      {question !== undefined && (
        <>
          <p>{question.text}</p>
          <ReviewedEtag.Provider value={open.data?.etag}>
            <QuestionAnswerCard question={question} />
          </ReviewedEtag.Provider>
        </>
      )}
    </section>
  );
}

/**
 * The U2.3 chat session view (Hotfix 077/078). A session starts only on explicit user action, with
 * scope [] in the project room or [requirementId] once that requirement is confirmed Accepted (no
 * fallback). Turns are ordinary request → final response with an explicit stage; the transcript is
 * the current mounted panel's successful turns only (no history, resume or browser storage);
 * metrics are the server's.
 */
export function SessionPanel(props: SessionPanelProps) {
  const queryClient = useQueryClient();
  const requirementId = props.mode === "requirement" ? props.requirementId : null;
  const requirement = useQuery({
    queryKey: ["sessions", "scope", "requirement", requirementId],
    queryFn: () => fetchRequirement(requirementId ?? ""),
    enabled: requirementId !== null,
  });
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [stage, setStage] = useState<SessionStage | "">("");
  const [text, setText] = useState("");
  const [turns, setTurns] = useState<readonly LocalTurn[]>([]);
  const nextKey = useRef(0);

  const scope: string[] = requirementId === null ? [] : [requirementId];
  const scopeReady = requirementId === null || requirement.data?.status === "Accepted";

  // The session works on the model as currently displayed: the requirement it is scoped to, or
  // the current model version (refetched after every write the panel makes).
  const version = useModelVersion();
  const create = useVersionedMutation<CreateSessionResponse, string[]>(
    (body, ifMatch) => apiClient.POST("/api/sessions", { params: { header: { "If-Match": ifMatch } }, body: { scope: body } }),
    requirementId === null ? version.data?.etag : requirement.data?.etag,
  );
  const turn = useVersionedMutation<SessionTurnResponse, { sessionId: string; text: string; stage: SessionStage }>(
    (body, ifMatch) =>
      apiClient.POST("/api/sessions/{id}/turns", {
        params: { path: { id: body.sessionId }, header: { "If-Match": ifMatch } },
        body: { text: body.text, stage: body.stage },
      }),
    version.data?.etag,
  );
  const versionReady = version.data !== undefined && !version.isFetching;
  // Fable #355: the server refuses sessions without an inference provider; so does the UI.
  const me = useMe();
  const inferenceAvailable = me.data?.inference_available !== false;
  const metrics = useQuery({
    queryKey: sessionMetricsKey(sessionId ?? ""),
    queryFn: () => fetchMetrics(sessionId ?? ""),
    enabled: sessionId !== null,
  });

  const startSession = () => {
    if (!scopeReady) {
      return;
    }
    create.mutate(scope, {
      onSuccess: (response) => {
        setSessionId(response.id);
        setTurns([]);
        setText("");
      },
    });
  };

  const sendTurn = () => {
    if (sessionId === null || stage === "" || text.trim() === "") {
      return;
    }
    const submitted = text;
    turn.mutate(
      { sessionId, text: submitted, stage },
      {
        onSuccess: (response) => {
          nextKey.current += 1;
          const key = nextKey.current;
          setTurns((current) => [...current, { key, text: submitted, response }]);
          setText("");
          void queryClient.invalidateQueries({ queryKey: sessionMetricsKey(sessionId) });
          void queryClient.invalidateQueries({ queryKey: modelVersionQueryKey });
        },
      },
    );
  };

  return (
    <section
      aria-label={t(props.mode === "project" ? "session.projectRoom" : "session.scopedTitle")}
      className="space-y-3"
      data-session-mode={props.mode}
      data-testid="session-panel"
    >
      {/* The project room is a routed page (its h1); the scoped panel is an aside next to one. */}
      {props.mode === "project" ? (
        <h1 className="text-xl font-semibold">{t("session.projectRoom")}</h1>
      ) : (
        <h2 className="text-lg font-semibold">{t("session.scopedTitle")}</h2>
      )}
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 text-sm">
        <dt>{t("session.scope")}</dt>
        <dd data-testid="session-scope">
          {requirementId === null ? (
            <span className="rounded border px-1">{t("session.scopeProject")}</span>
          ) : (
            <span className="rounded border px-1 font-mono">{requirementId}</span>
          )}
        </dd>
      </dl>
      {requirementId !== null && requirement.data !== undefined && requirement.data.status === "Accepted" && (
        <p className="text-sm text-slate-600">{requirement.data.text}</p>
      )}
      {requirementId !== null && requirement.isError && (
        <p role="alert" className="text-sm text-red-800" data-testid="scope-error">
          {t("session.scopeLookupFailed")} <span className="font-mono">{requirementId}</span>
        </p>
      )}
      {requirementId !== null && requirement.data !== undefined && requirement.data.status !== "Accepted" && (
        <p role="alert" className="text-sm text-red-800" data-testid="scope-unavailable">
          {t("session.scopeUnavailable")} <span className="font-mono">{requirementId}</span>{" "}
          <span className="font-mono">{requirement.data.status}</span>
        </p>
      )}

      {sessionId === null ? (
        <>
          <button
            type="button"
            className="rounded border px-3 py-1"
            data-testid="session-start"
            disabled={!scopeReady || !inferenceAvailable || me.data === undefined || create.isPending}
            onClick={startSession}
          >
            {t("session.start")}
          </button>
          {!inferenceAvailable && (
            <p className="text-sm text-slate-600" data-testid="session-inference-unavailable">
              {t("session.inferenceUnavailable")}
            </p>
          )}
          <WriteError error={create.error} />
        </>
      ) : (
        <>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 text-sm" data-testid="session-header">
            <dt>{t("session.id")}</dt>
            <dd className="font-mono" data-testid="session-id">
              {sessionId}
            </dd>
          </dl>
          <section aria-label={t("session.metrics")} className="text-sm" data-testid="session-metrics">
            <h3 className="font-semibold">{t("session.metrics")}</h3>
            {metrics.data === undefined ? (
              <p>{metrics.isError ? t("session.metricsFailed") : t("session.loading")}</p>
            ) : (
              <dl className="grid grid-cols-[auto_1fr] gap-x-4">
                <dt>{t("session.metrics.turns")}</dt>
                <dd data-metric="turns">{metrics.data.turns}</dd>
                <dt>{t("session.metrics.proposals")}</dt>
                <dd data-metric="proposals">{metrics.data.proposals}</dd>
                <dt>{t("session.metrics.requiresReason")}</dt>
                <dd data-metric="requires_reason">{metrics.data.requires_reason}</dd>
              </dl>
            )}
          </section>

          <section aria-label={t("session.transcript")} className="space-y-3" data-testid="session-transcript">
            <h3 className="font-semibold">{t("session.transcript")}</h3>
            {turns.map((entry) => (
              <div key={entry.key} className="space-y-2 rounded border p-2 text-sm" data-turn={entry.key}>
                <p data-role="user">
                  <span className="font-semibold">{t("session.you")}</span> {entry.text}
                </p>
                <p data-role="assistant">
                  <span className="font-semibold">{t("session.assistant")}</span> {entry.response.reply}
                </p>
                {entry.response.proposals.map((proposal) => (
                  <ProposalCard key={proposal.id} proposal={proposal} reviewedEtag={revisionEtag(entry.response.revision)} />
                ))}
                {entry.response.asked_question !== undefined && entry.response.asked_question !== null && (
                  <EngineQuestion questionId={entry.response.asked_question} />
                )}
              </div>
            ))}
          </section>

          <form
            aria-label={t("session.compose")}
            className="space-y-2"
            onSubmit={(event) => {
              event.preventDefault();
              sendTurn();
            }}
          >
            <label className="block text-sm">
              {t("session.stage")}
              <select className="ml-2 rounded border" value={stage} data-testid="session-stage" onChange={(event) => setStage(event.target.value as SessionStage | "")}>
                <option value="">{t("session.chooseStage")}</option>
                {SESSION_STAGES.map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
            </label>
            <label className="block text-sm">
              {t("session.message")}
              <textarea className="w-full rounded border p-1" value={text} data-testid="session-text" onChange={(event) => setText(event.target.value)} />
            </label>
            <button type="submit" className="rounded border px-3 py-1" data-testid="session-send" disabled={turn.isPending || !versionReady || stage === "" || text.trim() === ""}>
              {t("session.send")}
            </button>
            {turn.isPending && (
              <p role="status" data-testid="session-pending">
                {t("session.waiting")}
              </p>
            )}
            {turn.error instanceof WriteFailedError && (
              <p role="alert" className="text-sm text-red-800" data-testid="turn-failed">
                {t("session.turnFailed")} <span className="font-mono">{writeErrorCode(turn.error) ?? String(turn.error.status)}</span>{" "}
                <span>{writeErrorMessage(turn.error)}</span>
              </p>
            )}
          </form>
        </>
      )}
    </section>
  );
}
