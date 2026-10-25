import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouterState } from "@tanstack/react-router";
import { ArrowUp } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";

import { apiClient, readData, readReviewed } from "../../api/client";
import type { components } from "../../api/schema";
import { modelVersionQueryKey, useModelVersion, useVersionedMutation, WriteFailedError } from "../../api/useVersionedMutation";
import { useMe } from "../../app/auth";
import { buildHref, useGo } from "../../app/shell/navigation";
import { cn, isPlainClick } from "../../components/ui";
import { t, type MessageKey } from "../../i18n";
import { ProposalCard } from "./ProposalCard";
import { useAssistantStore, useConversation, type AssistantMessage } from "./store";

type SessionStage = components["schemas"]["SessionStage"];

/**
 * The compiler stage a turn works in, from where the user is (UI-SPEC §5.8 sends text and
 * stage): requirements are S1 (intent), scenarios S4 (functional proof), quality S5; the model
 * and everything else S2 (functional semantics).
 */
export function assistantStage(pathname: string): SessionStage {
  if (pathname.startsWith("/requirements")) {
    return "S1";
  }
  if (pathname.startsWith("/scenarios")) {
    return "S4";
  }
  return pathname.startsWith("/quality") ? "S5" : "S2";
}

const TURN_FAILURE: Readonly<Record<string, MessageKey>> = {
  E_SESSION_TURN_IN_FLIGHT: "assistant.error.inFlight",
  E_INFERENCE_FAILED: "assistant.error.inference",
  E_INFERENCE_REQUIRED: "assistant.error.aiOff",
  E_INFERENCE_UNAVAILABLE: "assistant.error.aiOff",
};

function failureOf(error: unknown): { text: string; code: string | null } {
  const body = error instanceof WriteFailedError && typeof error.body === "object" && error.body !== null ? (error.body as Record<string, unknown>) : null;
  const code = typeof body?.["code"] === "string" ? body["code"] : null;
  return { text: t((code === null ? undefined : TURN_FAILURE[code]) ?? "assistant.error.generic"), code };
}

function TypingIndicator() {
  return (
    <div role="status" aria-label={t("assistant.typing")} className="flex w-[60px] gap-1 rounded-card border border-line bg-card px-3.5 py-3">
      {[0, 1, 2].map((dot) => (
        <span key={dot} aria-hidden="true" className="size-1.5 animate-pulse-dot rounded-pill bg-fg-3" style={{ animationDelay: String(dot * 0.15) + "s" }} />
      ))}
    </div>
  );
}

function QuestionBack({ id }: { id: string }) {
  const go = useGo();
  const href = buildHref("/inbox/$id", { id }, { filter: "questions" });
  return (
    <div className="rounded-tile border border-line bg-card p-3.5 shadow-sm">
      <p className="m-0 text-footnote text-fg">{t("assistant.question.body")}</p>
      <a
        href={href}
        className="mt-2 inline-block text-footnote font-semibold text-fg underline-offset-2 hover:underline"
        onClick={(event: MouseEvent<HTMLAnchorElement>) => {
          if (isPlainClick(event)) {
            event.preventDefault();
            go(href);
          }
        }}
      >
        {t("assistant.question.answer")}
      </a>
    </div>
  );
}

function Bubble({ message }: { message: AssistantMessage }) {
  const user = message.role === "user";
  return (
    <div className={cn("flex", user ? "justify-end" : "justify-start")} data-role={message.role}>
      <div className="flex max-w-[88%] flex-col gap-2">
        {message.role === "error" ? (
          <div role="alert" className="rounded-card border border-line bg-blocked-tint px-3.5 py-[11px] text-callout leading-[1.55] text-blocked-fg">
            {message.text}
            {(message.code ?? null) !== null && <span className="ml-2 font-mono text-eyebrow">{message.code}</span>}
          </div>
        ) : (
          <div className={cn("whitespace-pre-wrap rounded-card px-3.5 py-[11px] text-callout leading-[1.55]", user ? "bg-primary text-on-primary" : "border border-line bg-card text-fg")}>
            {message.text}
          </div>
        )}
        {message.proposals?.map((proposal) => (
          <ProposalCard key={proposal.id} proposal={proposal} revision={message.revision ?? ""} />
        ))}
        {(message.askedQuestion ?? null) !== null && <QuestionBack id={message.askedQuestion ?? ""} />}
      </div>
    </div>
  );
}

export interface AssistantConversationProps {
  /** The open requirement the conversation is about; null for the whole project. */
  readonly requirementId: string | null;
}

/**
 * The Assistant conversation (handoff "8. Assistant panel"): bubbles, inline proposal cards, the
 * typing indicator and the composer. The session starts with the first message (`create_session`
 * with the scope), each message is one turn (`create_session_turn`, one at a time), and an open
 * session is confirmed with `get_session` when the panel reopens.
 */
export function AssistantConversation({ requirementId }: AssistantConversationProps) {
  const scopeKey = requirementId === null ? "project" : "requirement:" + requirementId;
  const conversation = useConversation(scopeKey);
  const append = useAssistantStore((state) => state.append);
  const setSession = useAssistantStore((state) => state.setSession);
  const setInFlight = useAssistantStore((state) => state.setInFlight);
  const queryClient = useQueryClient();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const me = useMe();
  const version = useModelVersion();
  const [draft, setDraft] = useState("");
  const endRef = useRef<HTMLDivElement>(null);

  const requirement = useQuery({
    queryKey: ["assistant", "scope", requirementId],
    queryFn: () => readReviewed(apiClient.GET("/api/requirements/{id}", { params: { path: { id: requirementId ?? "" } } })),
    enabled: requirementId !== null,
  });
  // An open session is the server's: confirm it when the panel shows it again.
  useQuery({
    queryKey: ["assistant", "session", conversation.sessionId],
    queryFn: () => readData(apiClient.GET("/api/sessions/{id}", { params: { path: { id: conversation.sessionId ?? "" } } })),
    enabled: conversation.sessionId !== null,
  });

  const create = useVersionedMutation(
    (scope: string[], ifMatch: string) => apiClient.POST("/api/sessions", { params: { header: { "If-Match": ifMatch } }, body: { scope } }),
    requirementId === null ? version.data?.etag : requirement.data?.etag,
  );
  const turn = useVersionedMutation(
    (input: { sessionId: string; text: string; stage: SessionStage }, ifMatch: string) =>
      apiClient.POST("/api/sessions/{id}/turns", { params: { path: { id: input.sessionId }, header: { "If-Match": ifMatch } }, body: { text: input.text, stage: input.stage } }),
    version.data?.etag,
  );

  const aiOff = me.data?.inference_available === false;
  const scopeBlocked = requirementId !== null && requirement.data !== undefined && requirement.data.status !== "Accepted";
  const scopeLoading = requirementId !== null && requirement.data === undefined;
  const composerOff = aiOff || scopeBlocked || scopeLoading || conversation.inFlight || me.data === undefined;
  const canSend = !composerOff && draft.trim() !== "";

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [conversation.messages.length, conversation.inFlight]);

  const send = async () => {
    if (!canSend) {
      return;
    }
    const text = draft.trim();
    setDraft("");
    append(scopeKey, { role: "user", text });
    setInFlight(scopeKey, true);
    try {
      let sessionId = conversation.sessionId;
      if (sessionId === null) {
        const created = await create.mutateAsync(requirementId === null ? [] : [requirementId]);
        sessionId = created.id;
        setSession(scopeKey, sessionId);
      }
      const response = await turn.mutateAsync({ sessionId, text, stage: assistantStage(pathname) });
      append(scopeKey, { role: "assistant", text: response.reply, proposals: response.proposals, revision: response.revision, askedQuestion: response.asked_question ?? null });
      void queryClient.invalidateQueries({ queryKey: modelVersionQueryKey });
    } catch (error) {
      // A 412 opens the shell's conflict sheet; every other failure is explained in the thread.
      if (!(error instanceof Error && error.name === "PreconditionFailedError")) {
        const failure = failureOf(error);
        append(scopeKey, { role: "error", text: failure.text, code: failure.code });
      }
    } finally {
      setInFlight(scopeKey, false);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      void send();
    }
  };

  return (
    <div data-testid="assistant-conversation" data-scope={requirementId === null ? "project" : "requirement"} className="flex min-h-0 flex-1 flex-col">
      <div className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-y-auto px-4 py-5">
        {conversation.messages.length === 0 && <p className="m-0 px-1 text-footnote leading-normal text-fg-3">{t("assistant.intro")}</p>}
        {conversation.messages.map((message) => (
          <Bubble key={message.key} message={message} />
        ))}
        {conversation.inFlight && <TypingIndicator />}
        <div ref={endRef} />
      </div>
      <div className="px-4 pb-4 pt-3">
        {aiOff && <p className="m-0 px-1 pb-2 text-caption text-fg-3">{t("assistant.aiOff")}</p>}
        {scopeBlocked && <p className="m-0 px-1 pb-2 text-caption text-fg-3">{t("assistant.scopeBlocked")}</p>}
        {requirement.isError && <p className="m-0 px-1 pb-2 text-caption text-blocked-fg">{t("assistant.scopeFailed")}</p>}
        <form
          aria-label={t("assistant.composer")}
          className="flex items-end gap-2 rounded-card border border-line bg-card py-2.5 pl-3.5 pr-2.5 transition-colors duration-[120ms] focus-within:border-accent"
          onSubmit={(event) => {
            event.preventDefault();
            void send();
          }}
        >
          <textarea
            aria-label={t("assistant.message")}
            placeholder={t("assistant.placeholder")}
            rows={2}
            value={draft}
            disabled={composerOff}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={onKeyDown}
            className="min-w-0 flex-1 resize-none border-0 bg-transparent text-callout leading-normal text-fg outline-none placeholder:text-fg-4 disabled:cursor-not-allowed"
          />
          <button
            type="submit"
            aria-label={t("assistant.send")}
            title={t("assistant.send")}
            disabled={!canSend}
            className="flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-pill bg-primary text-on-primary transition-colors duration-[120ms] hover:bg-primary-hover disabled:cursor-not-allowed disabled:opacity-40"
          >
            <ArrowUp size={16} strokeWidth={1.6} aria-hidden="true" />
          </button>
        </form>
      </div>
    </div>
  );
}
