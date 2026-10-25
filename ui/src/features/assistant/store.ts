import { create } from "zustand";

import type { components } from "../../api/schema";

export type SessionProposalView = components["schemas"]["SessionProposalView"];

/** One bubble of the conversation (view state of this page; the server keeps the session). */
export interface AssistantMessage {
  readonly key: number;
  readonly role: "user" | "assistant" | "error";
  readonly text: string;
  /** The typed code of a failed turn. */
  readonly code?: string | null;
  /** Proposals the turn made, decided against the turn's revision. */
  readonly proposals?: readonly SessionProposalView[];
  readonly revision?: string;
  /** A question the Assistant asked back (answered in the Inbox). */
  readonly askedQuestion?: string | null;
}

export interface Conversation {
  readonly sessionId: string | null;
  readonly messages: readonly AssistantMessage[];
  /** A turn is being answered. */
  readonly inFlight: boolean;
}

export type ProposalDecision = "accepted" | "rejected";

interface AssistantState {
  /** Conversations by scope ("project" or "requirement:<id>"). */
  readonly conversations: Readonly<Record<string, Conversation>>;
  /** Proposals decided in a conversation, by proposal ID. */
  readonly decisions: Readonly<Record<string, ProposalDecision>>;
  append(scope: string, message: Omit<AssistantMessage, "key">): void;
  setSession(scope: string, sessionId: string): void;
  setInFlight(scope: string, inFlight: boolean): void;
  decide(proposalId: string, decision: ProposalDecision): void;
  reset(): void;
}

const EMPTY: Conversation = { sessionId: null, messages: [], inFlight: false };

let nextKey = 0;

/** The Assistant's conversations for this page: closing the panel keeps them; a reload does not. */
export const useAssistantStore = create<AssistantState>()((set) => {
  const update = (scope: string, patch: (c: Conversation) => Partial<Conversation>) =>
    set((state) => {
      const current = state.conversations[scope] ?? EMPTY;
      return { conversations: { ...state.conversations, [scope]: { ...current, ...patch(current) } } };
    });
  return {
    conversations: {},
    decisions: {},
    append: (scope, message) => {
      nextKey += 1;
      const key = nextKey;
      update(scope, (c) => ({ messages: [...c.messages, { ...message, key }] }));
    },
    setSession: (scope, sessionId) => update(scope, () => ({ sessionId })),
    setInFlight: (scope, inFlight) => update(scope, () => ({ inFlight })),
    decide: (proposalId, decision) => set((state) => ({ decisions: { ...state.decisions, [proposalId]: decision } })),
    reset: () => set({ conversations: {}, decisions: {} }),
  };
});

/** The conversation of `scope` (empty until its first message). */
export function useConversation(scope: string): Conversation {
  return useAssistantStore((state) => state.conversations[scope] ?? EMPTY);
}
