// The Inbox (handoff "2. Inbox", spec §5.2): one keyboard-first triage queue for every pending
// decision. Three panes: smart filters with counts, the queue, and the inspector card of the
// selected item. Every write sends the ETag of the data the user reviewed; accepting goes through
// the atomic set acceptance (with confirmation) so accepted elements land in the model.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCheck, CircleHelp, Ellipsis, GitMerge, Link2, PenLine, Reply, Replace, SquarePlus, TriangleAlert, type LucideIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from "react";

import { apiClient, readData, revisionEtag } from "../../api/client";
import type { components } from "../../api/schema";
import { PreconditionFailedError, useVersionedMutation, writeErrorMessage, type WriteResponse } from "../../api/useVersionedMutation";
import { hasRole, useMe } from "../../app/auth";
import { stageLabel, usePipeline, type StageName } from "../../app/pipeline";
import { useRunStage } from "../../app/run";
import { buildHref, useGo } from "../../app/shell/navigation";
import { findingsQuery, gatesQuery, openQuestionsQuery, proposalsQuery } from "../../app/work";
import { ANALYST_ROLE } from "../../components/RoleGate";
import { ReadFailure } from "../../components/ReadFailure";
import {
  Button,
  cn,
  EmptyState,
  Eyebrow,
  IconTile,
  isPlainClick,
  ListRow,
  Menu,
  IconButton,
  ReasonSheet,
  SkeletonList,
  StatusPill,
  toast,
  WithInspector,
} from "../../components/ui";
import { t, tc, tf, type MessageKey } from "../../i18n";
import { isAnswerComplete, type AnswerValue } from "./answerSchema";
import { AnswerCard, CheckIcon, IssueCard, MultiPanel, ProposalCard, QuestionCard, StaleBanner, useWordingSuggestion, WordingCard } from "./cards";
import { buildInboxItems, elementKindLabel, filterCounts, formatAge, INBOX_FILTERS, matchesFilter, type InboxFilter, type InboxItem, type InboxItemType } from "./model";
import { proposalDetailQuery } from "./queries";

type PendingProposalDetail = components["schemas"]["PendingProposalDetail"];
type WriteResult = components["schemas"]["WriteResult"];
type AcceptSetResult = components["schemas"]["AcceptProposalSetResponse"];
type AnswerResponse = components["schemas"]["AnswerResponse"];
type RefreshResult = components["schemas"]["RefreshProposalResponse"];
type IssueToDecide = NonNullable<InboxItem["issue"]>;
type ElementKind = components["schemas"]["ProposalElementKind"];

const FILTER_LABEL: Readonly<Record<InboxFilter, MessageKey>> = {
  all: "inbox.filter.all",
  proposals: "inbox.filter.proposals",
  questions: "inbox.filter.questions",
  wording: "inbox.filter.wording",
  issues: "inbox.filter.issues",
  steps: "inbox.filter.steps",
  fromq: "inbox.filter.fromq",
  assistant: "inbox.filter.assistant",
};

const QUEUE_TITLE: Readonly<Record<InboxFilter, MessageKey>> = { ...FILTER_LABEL, all: "inbox.queue.all" };

const TYPE_ICON: Readonly<Record<InboxItemType, LucideIcon>> = {
  create: SquarePlus,
  link: Link2,
  merge: GitMerge,
  supersede: Replace,
  wording: PenLine,
  answer: Reply,
  question: CircleHelp,
  issue: TriangleAlert,
};

/** Where "View in Model" leads for an element kind. */
const KIND_HREF: Readonly<Partial<Record<string, string>>> = {
  operation: "/model/processes",
  event: "/model/processes",
  outcome: "/model/processes",
  process: "/model/processes",
  entity: "/model/entities",
  attribute: "/model/entities",
  state: "/model/entities",
  transition: "/model/entities",
  invariant: "/model/entities",
  glossary_term: "/model/glossary",
  term: "/model/glossary",
  calculation: "/model/calculations",
  rule: "/model/rules",
  decision_table: "/model/rules",
  requirement: "/requirements",
  link: "/requirements",
  scenario: "/scenarios",
};

function modelHref(item: InboxItem, detail: PendingProposalDetail | undefined): string {
  const fromDiff = detail === undefined ? undefined : [...detail.diff.added, ...detail.diff.modified].map((d) => KIND_HREF[d.kind]).find((href) => href !== undefined);
  return fromDiff ?? KIND_HREF[item.proposal?.element_kind ?? "other"] ?? "/model/glossary";
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** The toast of an accepted item. */
function acceptedMessage(item: InboxItem): string {
  switch (item.type) {
    case "wording":
      return t("inbox.toast.wording");
    case "answer":
      return t("inbox.toast.answer");
    case "link":
      return t("inbox.toast.link");
    case "merge":
      return t("inbox.toast.merge");
    case "supersede":
      return t("inbox.toast.supersede");
    default: {
      const kind = elementKindLabel((item.proposal?.element_kind ?? "other") as ElementKind);
      return tf("inbox.toast.added", { kind, Kind: capitalize(kind) });
    }
  }
}

function rowDomId(id: string): string {
  return "inbox-row-" + id.replace(/[^A-Za-z0-9_-]/g, "_");
}

function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

interface SheetState {
  readonly kind: "accept" | "reject" | "discard" | "waive";
  readonly title: string;
  readonly description?: string;
  readonly confirmLabel: string;
  readonly destructive: boolean;
  readonly required: boolean;
  readonly placeholder?: string;
  readonly run: (reason: string | null) => void;
}

interface Draft {
  readonly value: AnswerValue;
  readonly rationale: string;
}

const EMPTY_DRAFT: Draft = { value: {}, rationale: "" };

// ------------------------------------------------------------------ filters pane

/** Key names and separators of the keyboard hint (glyphs, not copy). */
const SEPARATOR = " · ";
const CLICK_KEYS = "⇧/⌘";
const KEY_HINTS: readonly (readonly [string, MessageKey])[] = [
  ["J K", "inbox.keys.move"],
  ["A", "inbox.keys.accept"],
  ["R", "inbox.keys.reject"],
  [CLICK_KEYS, "inbox.keys.many"],
];

function FiltersPane({ filter, counts, hrefFor }: { filter: InboxFilter; counts: Record<InboxFilter, number> | null; hrefFor: (filter: InboxFilter) => string }) {
  const go = useGo();
  const link = (f: InboxFilter) => {
    const active = f === filter;
    const href = hrefFor(f);
    return (
      <a
        key={f}
        href={href}
        data-filter={f}
        aria-current={active ? "page" : undefined}
        onClick={(event: MouseEvent<HTMLAnchorElement>) => {
          if (isPlainClick(event)) {
            event.preventDefault();
            go(href);
          }
        }}
        className={cn(
          "flex h-[34px] w-full items-center justify-between rounded-control px-2.5 text-footnote no-underline transition-colors duration-[120ms] ease-standard",
          active ? "bg-surface font-semibold text-fg" : "font-medium text-fg-2 hover:bg-subtle-row",
        )}
      >
        <span className="truncate">{t(FILTER_LABEL[f])}</span>
        {counts !== null && (
          // The handoff's 70 % opacity count is below WCAG AA at 11 px; the muted text tokens meet it.
          <span data-count className={cn("font-mono text-eyebrow font-medium", active ? "text-fg-3-surface" : "text-fg-3")}>
            {counts[f]}
          </span>
        )}
      </a>
    );
  };
  return (
    <div style={{ width: "clamp(180px, 16vw, 220px)" }} className="flex min-h-0 shrink-0 flex-col overflow-y-auto border-r border-line bg-card px-2.5 py-5">
      <h1 className="m-0 px-2.5 pb-3 font-display text-title font-semibold leading-none tracking-[-0.01em] text-fg">{t("inbox.title")}</h1>
      <nav aria-label={t("inbox.filters")} className="flex flex-col">
        {INBOX_FILTERS.slice(0, 5).map(link)}
        <Eyebrow as="p" className="m-0 px-2.5 pb-2 pt-[18px]">
          {t("inbox.byOrigin")}
        </Eyebrow>
        {INBOX_FILTERS.slice(5).map(link)}
      </nav>
      <div className="mx-2.5 mt-6 rounded-field bg-surface p-3 text-eyebrow leading-[1.8] text-fg-3-surface">
        <p className="m-0 mb-0.5 font-semibold text-fg-2">{t("inbox.keys.title")}</p>
        <p className="m-0">
          {KEY_HINTS.map(([keys, label], index) => (
            <span key={label}>
              {index > 0 && SEPARATOR}
              <span className="font-mono">{keys === CLICK_KEYS ? keys + " " + t("inbox.keys.click") : keys}</span> {t(label)}
            </span>
          ))}
        </p>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ screen

export function InboxScreen({ filter = "all", selectedId }: { filter?: InboxFilter; selectedId?: string }) {
  const go = useGo();
  const queryClient = useQueryClient();
  const me = useMe();
  const canDecide = hasRole(me.data, ANALYST_ROLE);
  const proposals = useQuery(proposalsQuery);
  const questions = useQuery(openQuestionsQuery);
  const findings = useQuery(findingsQuery);
  const gates = useQuery(gatesQuery);
  const pipeline = usePipeline();
  const runStage = useRunStage();

  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  const [multi, setMulti] = useState<readonly string[]>([]);
  const [cleared, setCleared] = useState(false);
  const [evidenceOpen, setEvidenceOpen] = useState(true);
  const [drafts, setDrafts] = useState<Readonly<Record<string, Draft>>>({});
  const [editing, setEditing] = useState<{ id: string; text: string | null } | null>(null);
  const [sheet, setSheet] = useState<SheetState | null>(null);
  const [sheetError, setSheetError] = useState<string | null>(null);
  const [focusId, setFocusId] = useState<string | null>(null);

  const reads = [proposals, questions, findings];
  const failed = reads.find((r) => r.isError);
  const loaded = proposals.data !== undefined && questions.data !== undefined && findings.data !== undefined;

  const items = useMemo(
    () =>
      loaded
        ? buildInboxItems({ proposals: proposals.data.items, questions: questions.data.items, findings: findings.data, gates: Array.isArray(gates.data) ? gates.data : [] }).filter(
            (item) => !hidden.has(item.id),
          )
        : [],
    [loaded, proposals.data, questions.data, findings.data, gates.data, hidden],
  );
  const visible = useMemo(() => items.filter((item) => matchesFilter(item, filter)), [items, filter]);
  const counts = loaded ? filterCounts(items) : null;
  const multiItems = multi.map((id) => visible.find((i) => i.id === id)).filter((i): i is InboxItem => i !== undefined);
  const isMulti = multiItems.length > 1;
  const selected = isMulti ? null : (visible.find((i) => i.id === selectedId) ?? (cleared ? null : (visible[0] ?? null)));

  // Hidden IDs (removed optimistically after a write) are forgotten once the server no longer
  // lists them, so an item that comes back later (a discarded answer's question) shows again.
  if (loaded && hidden.size > 0) {
    const listed = new Set([...proposals.data.items.map((p) => p.proposal_id), ...questions.data.items.map((q) => q.id), ...findings.data.map((f) => f.id)]);
    if ([...hidden].some((id) => !listed.has(id))) {
      setHidden(new Set([...hidden].filter((id) => listed.has(id))));
    }
  }

  // After a row is removed (or J/K), focus moves to that row once it is rendered.
  const focusReady = focusId !== null && visible.some((i) => i.id === focusId);
  useEffect(() => {
    if (focusReady && focusId !== null) {
      document.getElementById(rowDomId(focusId))?.focus();
    }
  }, [focusId, focusReady]);

  const filterSearch = useCallback((f: InboxFilter): Record<string, string> => (f === "all" ? {} : { filter: f }), []);
  const itemHref = useCallback((id: string) => buildHref("/inbox/$id", { id }, filterSearch(filter)), [filter, filterSearch]);
  const hrefForFilter = useCallback((f: InboxFilter) => buildHref("/inbox", {}, filterSearch(f)), [filterSearch]);

  const select = (id: string, focus = false) => {
    setCleared(false);
    setMulti([]);
    setEditing(null);
    go(itemHref(id));
    setFocusId(focus ? id : null);
  };

  /** Removes rows after a write; the selection moves to the next row (focused). */
  const removeItems = (ids: readonly string[]) => {
    const anchor = selected?.id ?? ids[0];
    const index = Math.max(0, visible.findIndex((i) => i.id === anchor));
    const rest = visible.filter((i) => !ids.includes(i.id));
    const next = rest[Math.min(index, rest.length - 1)];
    setHidden((prev) => new Set([...prev, ...ids]));
    setMulti([]);
    setEditing(null);
    if (next !== undefined) {
      select(next.id, true);
    } else {
      go(hrefForFilter(filter));
    }
  };

  const refetchAll = () => void queryClient.invalidateQueries();

  const failure = (error: Error) => {
    if (error instanceof PreconditionFailedError) {
      return;
    }
    toast({ tone: "error", message: writeErrorMessage(error) ?? t("inbox.write.failed") });
  };

  // ---------------------------------------------------------------- writes

  const reviewed = proposals.data?.etag;
  const acceptSet = useVersionedMutation<AcceptSetResult, { ids: readonly string[]; reason: string | null }>(
    ({ ids, reason }, ifMatch) =>
      apiClient.POST("/api/proposals/accept-set", { params: { header: { "If-Match": ifMatch } }, body: { proposal_ids: [...ids], confirm: true, reason } }),
    reviewed,
  );
  const reject = useVersionedMutation<WriteResult, { id: string; reason: string | null }>(
    ({ id, reason }, ifMatch) => apiClient.POST("/api/proposals/{id}/reject", { params: { path: { id }, header: { "If-Match": ifMatch } }, body: { reason } }),
    reviewed,
  );
  const discard = useVersionedMutation<WriteResult, { id: string; reason: string | null }>(
    ({ id, reason }, ifMatch) => apiClient.POST("/api/proposals/{id}/discard", { params: { path: { id }, header: { "If-Match": ifMatch } }, body: { reason } }),
    reviewed,
  );
  const refresh = useVersionedMutation<RefreshResult, string>(
    (id, ifMatch) => apiClient.POST("/api/proposals/{id}/refresh", { params: { path: { id }, header: { "If-Match": ifMatch } } }),
    reviewed,
  );
  const answer = useVersionedMutation<AnswerResponse, { id: string; value: AnswerValue; rationale: string | null }>(
    ({ id, value, rationale }, ifMatch) =>
      apiClient.POST("/api/questions/{id}/answer", {
        params: { path: { id }, header: { "If-Match": ifMatch } },
        body: { answer: value as Record<string, never>, rationale, supersedes: null },
      }),
    questions.data?.etag,
  );
  // A blocker without a Finding node is materialized first; the waiver is then based on that write.
  const waive = useVersionedMutation<WriteResult, { issue: IssueToDecide; reason: string }>(async ({ issue, reason }, ifMatch) => {
    const waiveFinding = (id: string, etag: string) =>
      apiClient.POST("/api/findings/{id}/waive", { params: { path: { id }, header: { "If-Match": etag } }, body: { reason } }) as Promise<WriteResponse<WriteResult>>;
    if (issue.findingId !== null) {
      return waiveFinding(issue.findingId, ifMatch);
    }
    const materialized = await apiClient.POST("/api/findings/materialize", { params: { header: { "If-Match": ifMatch } }, body: {} });
    if (materialized.data === undefined) {
      return materialized as unknown as WriteResponse<WriteResult>;
    }
    const all = (await readData(apiClient.GET("/api/findings"))).items;
    const found = all.find((f) => f.code === issue.code && materialized.data.finding_ids.includes(f.id)) ?? all.find((f) => f.code === issue.code && f.status.toLowerCase() === "open");
    if (found === undefined) {
      return { error: { code: "E_NOT_FOUND", message: t("inbox.issue.notMaterialized"), details: {} }, response: new Response(null, { status: 404 }) };
    }
    return waiveFinding(found.id, revisionEtag(materialized.data.revision));
  }, reviewed);
  const wordingSuggestion = useWordingSuggestion(selected);
  // An edited wording replaces the statement directly; the suggestion it replaces is then retired.
  const editWording = useVersionedMutation<WriteResult, { requirementId: string; proposalId: string; text: string }>(async ({ requirementId, proposalId, text }, ifMatch) => {
    const accepted = await apiClient.POST("/api/requirements/{id}/accept-ears", { params: { path: { id: requirementId }, header: { "If-Match": ifMatch } }, body: { text } });
    if (accepted.data === undefined) {
      return accepted;
    }
    const retired = await apiClient.POST("/api/proposals/{id}/reject", {
      params: { path: { id: proposalId }, header: { "If-Match": revisionEtag(accepted.data.revision) } },
      body: { reason: t("inbox.wording.replacedReason") },
    });
    return retired.data === undefined ? retired : accepted;
  }, wordingSuggestion?.etag);

  const pending = acceptSet.isPending || reject.isPending || discard.isPending || waive.isPending || answer.isPending || refresh.isPending || editWording.isPending;

  // ---------------------------------------------------------------- actions

  const acceptIds = (ids: readonly string[], reason: string | null, done: () => void) =>
    acceptSet.mutate(
      { ids, reason },
      {
        onSuccess: () => {
          setSheet(null);
          done();
          refetchAll();
        },
        onError: (error) => {
          setSheetError(writeErrorMessage(error));
          failure(error);
        },
      },
    );

  const acceptOne = (item: InboxItem, reason: string | null) =>
    acceptIds([item.id], reason, () => {
      const detail = queryClient.getQueryData<PendingProposalDetail>(proposalDetailQuery(item.id).queryKey);
      removeItems([item.id]);
      toast({ message: acceptedMessage(item), action: { label: t("inbox.toast.viewInModel"), onAction: () => go(modelHref(item, detail)) } });
    });

  const accept = (item: InboxItem | null) => {
    if (item === null || !canDecide || !item.acceptable || pending) {
      return;
    }
    if (item.proposal?.requires_reason === true) {
      openSheet({ kind: "accept", title: t("inbox.sheet.accept.title"), confirmLabel: t("inbox.action.accept"), destructive: false, required: true, run: (reason) => acceptOne(item, reason) });
      return;
    }
    acceptOne(item, null);
  };

  const openSheet = (next: SheetState) => {
    setSheetError(null);
    setSheet(next);
  };

  const governed = (mutation: typeof reject, id: string, reason: string | null, message: string) =>
    mutation.mutate(
      { id, reason },
      {
        onSuccess: () => {
          setSheet(null);
          removeItems([id]);
          toast({ message });
          refetchAll();
        },
        onError: (error) => {
          setSheetError(writeErrorMessage(error));
          failure(error);
        },
      },
    );

  const rejectItem = (item: InboxItem | null) => {
    if (item === null || !canDecide || pending) {
      return;
    }
    switch (item.type) {
      case "question":
        skip(item);
        return;
      case "issue":
        waiveItem(item);
        return;
      case "answer":
        discardItem(item);
        return;
      default:
        openSheet({
          kind: "reject",
          title: t("inbox.sheet.reject.title"),
          description: tf("inbox.sheet.reject.description", { title: item.title }),
          confirmLabel: t("inbox.action.reject"),
          destructive: true,
          required: item.proposal?.requires_reason === true,
          run: (reason) => governed(reject, item.id, reason, t("inbox.toast.rejected")),
        });
    }
  };

  const discardItem = (item: InboxItem) =>
    openSheet({
      kind: "discard",
      title: t(item.type === "answer" ? "inbox.sheet.discard.title" : "inbox.sheet.discardProposal.title"),
      description: t(item.type === "answer" ? "inbox.sheet.discard.description" : "inbox.sheet.discardProposal.description"),
      confirmLabel: t("inbox.action.discard"),
      destructive: true,
      required: true,
      run: (reason) => governed(discard, item.id, reason, t(item.type === "answer" ? "inbox.toast.discarded" : "inbox.toast.discardedProposal")),
    });

  const waiveItem = (item: InboxItem) => {
    const issue = item.issue;
    if (issue === undefined) {
      return;
    }
    openSheet({
      kind: "waive",
      title: t("inbox.sheet.waive.title"),
      description: t("inbox.sheet.waive.description"),
      confirmLabel: t("inbox.action.waive"),
      destructive: false,
      required: true,
      placeholder: t("inbox.sheet.waive.placeholder"),
      run: (reason) =>
        waive.mutate(
          { issue, reason: reason ?? "" },
          {
            onSuccess: () => {
              setSheet(null);
              removeItems([item.id]);
              toast({ message: t("inbox.toast.waived") });
              refetchAll();
            },
            onError: (error) => {
              setSheetError(writeErrorMessage(error));
              failure(error);
            },
          },
        ),
    });
  };

  const skip = (item: InboxItem) => {
    const index = visible.findIndex((i) => i.id === item.id);
    const next = visible[index + 1] ?? visible[index - 1];
    if (next !== undefined && next.id !== item.id) {
      select(next.id, true);
    }
  };

  const submitAnswer = (item: InboxItem) => {
    const question = item.question;
    const draft = drafts[item.id] ?? EMPTY_DRAFT;
    if (question === undefined || !isAnswerComplete(question.answer_schema, draft.value) || pending) {
      return;
    }
    answer.mutate(
      { id: question.id, value: draft.value, rationale: draft.rationale.trim() === "" ? null : draft.rationale.trim() },
      {
        onSuccess: (response) => {
          setHidden((prev) => new Set([...prev, question.id]));
          setDrafts((prev) => ({ ...prev, [question.id]: EMPTY_DRAFT }));
          toast({ message: t("inbox.toast.answered") });
          refetchAll();
          go(itemHref(response.proposal_id));
        },
        onError: failure,
      },
    );
  };

  const refreshItem = (item: InboxItem) => {
    const p = item.proposal;
    if (p === undefined || pending) {
      return;
    }
    if (p.origin === "question") {
      refresh.mutate(item.id, {
        onSuccess: (response) => {
          toast({ message: t("inbox.toast.refreshed") });
          refetchAll();
          go(itemHref(response.proposal_id));
        },
        onError: failure,
      });
      return;
    }
    if (item.stage !== null) {
      runStage.run(item.stage);
      toast({ message: tf("inbox.toast.rerun", { step: stageLabel(item.stage) }) });
    }
  };

  const refreshable = (item: InboxItem) => canDecide && item.proposal !== undefined && (item.proposal.origin === "question" || item.stage !== null);

  const startEdit = () => {
    if (selected?.type === "wording" && canDecide && wordingSuggestion !== null) {
      setEditing({ id: selected.id, text: wordingSuggestion.suggestion });
    }
  };

  const saveEdit = () => {
    if (selected === null || editing === null || wordingSuggestion === null || editing.text === null) {
      return;
    }
    const text = editing.text.trim();
    const item = selected;
    editWording.mutate(
      { requirementId: wordingSuggestion.requirementId, proposalId: item.id, text },
      {
        onSuccess: () => {
          removeItems([item.id]);
          toast({ message: t("inbox.toast.wording"), action: { label: t("inbox.toast.viewInModel"), onAction: () => go(buildHref("/requirements/$id", { id: wordingSuggestion.requirementId })) } });
          refetchAll();
        },
        onError: failure,
      },
    );
  };

  // "Accept all from <step>": the selected item's step, else the step with the most current proposals.
  const stepCandidates = visible.filter((i) => i.acceptable && i.stage !== null && i.proposal !== undefined && i.type !== "answer");
  const bulkStage: StageName | null =
    selected?.stage !== null && selected?.stage !== undefined && stepCandidates.some((i) => i.stage === selected.stage)
      ? selected.stage
      : (stepCandidates
          .map((i) => i.stage as StageName)
          .sort((a, b) => stepCandidates.filter((i) => i.stage === b).length - stepCandidates.filter((i) => i.stage === a).length)[0] ?? null);

  const acceptAllFromStep = () => {
    if (bulkStage === null || pending) {
      return;
    }
    const batch = stepCandidates.filter((i) => i.stage === bulkStage);
    const staleCount = visible.filter((i) => i.stale && i.stage === bulkStage).length;
    const step = stageLabel(bulkStage);
    const run = (reason: string | null) =>
      acceptIds(
        batch.map((i) => i.id),
        reason,
        () => {
          removeItems(batch.map((i) => i.id));
          const message = tc("inbox.toast.acceptedAll", batch.length) + (staleCount > 0 ? " · " + tc("inbox.toast.needRefresh", staleCount) : "");
          const first = batch[0];
          toast({ message, ...(first === undefined ? {} : { action: { label: t("inbox.toast.viewInModel"), onAction: () => go(modelHref(first, undefined)) } }) });
        },
      );
    if (batch.some((i) => i.proposal?.requires_reason === true)) {
      openSheet({
        kind: "accept",
        title: tf("inbox.sheet.acceptAll.title", { n: batch.length, step }),
        confirmLabel: tf("inbox.multi.accept", { n: batch.length }),
        destructive: false,
        required: true,
        run,
      });
      return;
    }
    run(null);
  };

  const acceptMulti = () => {
    const batch = multiItems.filter((i) => i.acceptable);
    if (batch.length === 0 || pending) {
      return;
    }
    const run = (reason: string | null) =>
      acceptIds(
        batch.map((i) => i.id),
        reason,
        () => {
          removeItems(batch.map((i) => i.id));
          toast({ message: tf("inbox.toast.acceptedTogether", { n: batch.length }) });
        },
      );
    if (batch.some((i) => i.proposal?.requires_reason === true)) {
      openSheet({ kind: "accept", title: tf("inbox.sheet.acceptMany.title", { n: batch.length }), confirmLabel: tf("inbox.multi.accept", { n: batch.length }), destructive: false, required: true, run });
      return;
    }
    run(null);
  };

  const onRowClick = (item: InboxItem, event: MouseEvent<HTMLButtonElement>) => {
    if (event.shiftKey || event.metaKey || event.ctrlKey) {
      const base = multi.length > 0 ? multi : selected !== null ? [selected.id] : [];
      const next = base.includes(item.id) ? base.filter((id) => id !== item.id) : [...base, item.id];
      if (next.length > 1) {
        setMulti(next);
      } else {
        select(next[0] ?? item.id);
      }
      return;
    }
    select(item.id);
  };

  const move = (delta: number) => {
    if (visible.length === 0) {
      return;
    }
    const index = selected === null ? -1 : visible.findIndex((i) => i.id === selected.id);
    const next = visible[Math.min(visible.length - 1, Math.max(0, index + delta))];
    if (next !== undefined) {
      select(next.id, true);
    }
  };

  // ---------------------------------------------------------------- keyboard

  const onKey = (event: KeyboardEvent) => {
    if (event.defaultPrevented || isTyping(event.target) || document.querySelector('[role="dialog"]') !== null) {
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === ".") {
      event.preventDefault();
      setMulti([]);
      setCleared(true);
      go(hrefForFilter(filter));
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey) {
      return;
    }
    const target = event.target instanceof HTMLElement ? event.target : null;
    const key = event.key.toLowerCase();
    if (["j", "k", "a", "r", "e"].includes(key)) {
      // The letter must not reach a sheet or field the shortcut opens.
      event.preventDefault();
    }
    switch (key) {
      case "j":
        move(1);
        break;
      case "k":
        move(-1);
        break;
      case "a":
        accept(selected);
        break;
      case "r":
        rejectItem(selected);
        break;
      case "e":
        startEdit();
        break;
      case " ":
        // Space on another control presses it; on the page or a queue row it toggles the evidence.
        if (target === null || target === document.body || target.id.startsWith("inbox-row-")) {
          event.preventDefault();
          setEvidenceOpen((open) => !open);
        }
        break;
      default:
    }
  };
  const keys = useRef(onKey);
  useEffect(() => {
    keys.current = onKey;
  });
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => keys.current(event);
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // ---------------------------------------------------------------- inspector

  const draft = selected === null ? EMPTY_DRAFT : (drafts[selected.id] ?? EMPTY_DRAFT);
  const setDraft = (id: string, patch: Partial<Draft>) => setDrafts((prev) => ({ ...prev, [id]: { ...(prev[id] ?? EMPTY_DRAFT), ...patch } }));
  const openSource = (sourceId: string) => go(buildHref("/sources/$id", { id: sourceId }));

  let inspector: ReactNode = null;
  let actions: ReactNode = undefined;
  if (isMulti) {
    inspector = <MultiPanel count={multiItems.length} acceptable={multiItems.filter((i) => i.acceptable).length} onAccept={acceptMulti} onClear={() => setMulti([])} pending={acceptSet.isPending} canDecide={canDecide} />;
  } else if (selected !== null) {
    const stale = selected.stale && selected.proposal !== undefined ? (
      <StaleBanner baseRevision={selected.proposal.base_revision} refreshing={refresh.isPending} {...(refreshable(selected) ? { onRefresh: () => refreshItem(selected) } : {})} />
    ) : null;
    const editText = editing?.id === selected.id ? (editing.text ?? wordingSuggestion?.suggestion ?? "") : null;
    switch (selected.type) {
      case "question":
        inspector = (
          <QuestionCard
            item={selected}
            value={draft.value}
            rationale={draft.rationale}
            onValue={(value) => setDraft(selected.id, { value })}
            onRationale={(rationale) => setDraft(selected.id, { rationale })}
            canAnswer={canDecide}
          />
        );
        break;
      case "issue":
        inspector = <IssueCard item={selected} />;
        break;
      case "answer":
        inspector = <AnswerCard item={selected} stale={stale} />;
        break;
      case "wording":
        inspector = (
          <WordingCard
            item={selected}
            stale={stale}
            editText={editText}
            onEditText={(text) => setEditing({ id: selected.id, text })}
            evidenceOpen={evidenceOpen}
            onToggleEvidence={() => setEvidenceOpen((open) => !open)}
            onOpenSource={openSource}
          />
        );
        break;
      default:
        inspector = <ProposalCard item={selected} stale={stale} evidenceOpen={evidenceOpen} onToggleEvidence={() => setEvidenceOpen((open) => !open)} onOpenSource={openSource} />;
    }
    if (canDecide) {
      actions = (
        <div role="group" aria-label={t("inbox.actions")} className="flex flex-1 items-center gap-2">
          {inspectorActions(selected, editText)}
        </div>
      );
    }
  }

  function inspectorActions(item: InboxItem, editText: string | null): ReactNode {
    const primary = (label: string, onClick: () => void, disabled = false, icon: ReactNode = <CheckIcon />) => (
      <Button variant="primary" size={38} className="px-[18px]" icon={icon} disabled={disabled || pending} onClick={onClick}>
        {label}
      </Button>
    );
    const secondary = (label: string, onClick: () => void) => (
      <Button variant="outline" size={38} disabled={pending} onClick={onClick}>
        {label}
      </Button>
    );
    switch (item.type) {
      case "question": {
        const complete = item.question !== undefined && isAnswerComplete(item.question.answer_schema, draft.value);
        return (
          <>
            {primary(t("inbox.action.answer"), () => submitAnswer(item), !complete)}
            {secondary(t("inbox.action.skip"), () => skip(item))}
          </>
        );
      }
      case "issue": {
        const firstRequirement = item.issue?.affected.find((ref) => ref.startsWith("req:"));
        const fixHref = firstRequirement !== undefined ? buildHref("/requirements/$id", { id: firstRequirement }) : buildHref("/quality/$gate", { gate: item.issue?.gate ?? "" });
        return (
          <>
            {primary(t("inbox.action.fix"), () => go(fixHref), false, null)}
            {secondary(t("inbox.action.waive"), () => waiveItem(item))}
          </>
        );
      }
      case "answer":
        return (
          <>
            {primary(t("inbox.action.accept"), () => accept(item), !item.acceptable)}
            {secondary(t("inbox.action.discard"), () => discardItem(item))}
          </>
        );
      case "wording":
        if (editText !== null) {
          return (
            <>
              {primary(t("inbox.action.saveAccept"), saveEdit, editText.trim() === "" || editText.trim() === wordingSuggestion?.original)}
              {secondary(t("inbox.action.cancel"), () => setEditing(null))}
            </>
          );
        }
        return (
          <>
            {primary(t("inbox.action.accept"), () => accept(item), !item.acceptable)}
            {wordingSuggestion !== null && item.acceptable && secondary(t("inbox.action.edit"), startEdit)}
            {secondary(t("inbox.action.reject"), () => rejectItem(item))}
          </>
        );
      default:
        return (
          <>
            {primary(t("inbox.action.accept"), () => accept(item), !item.acceptable)}
            {secondary(t("inbox.action.reject"), () => rejectItem(item))}
            {item.proposal?.discardable === true && (
              <>
                <span className="flex-1" />
                <Menu
                  trigger={<IconButton label={t("inbox.action.more")} variant="outline" size={34} className="size-[38px] rounded-pill" icon={<Ellipsis size={16} strokeWidth={1.6} aria-hidden="true" />} />}
                  items={[{ id: "discard", label: t("inbox.action.discard"), destructive: true, onSelect: () => discardItem(item) }]}
                />
              </>
            )}
          </>
        );
    }
  }

  // ---------------------------------------------------------------- queue

  const nextStep = pipeline.next;
  let main: ReactNode;
  if (failed !== undefined) {
    main = (
      <div className="flex flex-col items-start gap-3 p-6">
        <ReadFailure error={failed.error} messageKey="inbox.loadFailed" />
        <Button variant="outline" size={34} onClick={() => reads.filter((r) => r.isError).forEach((r) => void r.refetch())}>
          {t("inbox.retry")}
        </Button>
      </div>
    );
  } else if (!loaded) {
    main = (
      <div data-testid="inbox-skeleton" className="px-5 py-3">
        <SkeletonList rows={8} />
      </div>
    );
  } else if (items.length === 0) {
    main = (
      <EmptyState
        className="flex-1"
        visual="check"
        title={t("inbox.empty.title")}
        description={nextStep === null ? t("inbox.empty.none") : tf("inbox.empty.next", { step: stageLabel(nextStep) })}
        action={
          nextStep === null ? (
            <Button variant="outline" size={34} onClick={() => go("/quality")}>
              {t("inbox.empty.quality")}
            </Button>
          ) : canDecide ? (
            <Button variant="primary" size={34} className="h-9 px-[18px]" pending={runStage.starting} onClick={() => runStage.run(nextStep)}>
              {tf("inbox.empty.run", { step: stageLabel(nextStep) })}
            </Button>
          ) : undefined
        }
      />
    );
  } else if (visible.length === 0) {
    main = (
      <EmptyState
        className="flex-1"
        visual="none"
        headingLevel={3}
        title={t("inbox.empty.filtered")}
        action={
          <Button variant="outline" size={34} onClick={() => go(hrefForFilter("all"))}>
            {t("inbox.empty.showAll")}
          </Button>
        }
      />
    );
  } else {
    main = (
      <ul aria-label={t(QUEUE_TITLE[filter])} className="m-0 p-0">
        {visible.map((item) => {
          const isSelected = isMulti ? multi.includes(item.id) : item.id === selected?.id;
          const Icon = TYPE_ICON[item.type];
          return (
            <ListRow
              key={item.id}
              id={rowDomId(item.id)}
              selected={isSelected}
              onSelect={(event) => onRowClick(item, event)}
              leading={<IconTile icon={<Icon size={16} strokeWidth={1.6} />} selected={isSelected} />}
              title={<span data-row-title>{item.title}</span>}
              subtitle={item.typeLabel + " · " + item.originLabel}
              trailing={
                item.stale ? (
                  // On a white base the attention tint keeps its text at AA contrast, also on a selected row.
                  <span className="rounded-pill bg-card">
                    <StatusPill status="stale" />
                  </span>
                ) : undefined
              }
              meta={formatAge(item.createdAt)}
            />
          );
        })}
      </ul>
    );
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1">
      <FiltersPane filter={filter} counts={counts} hrefFor={hrefForFilter} />
      <WithInspector
        label={t("inbox.inspector")}
        inspector={inspector}
        actions={actions}
        empty={items.length === 0 && loaded ? <span /> : undefined}
        {...(selected === null ? {} : { inspectorKey: selected.id })}
      >
        <div className="flex min-h-full flex-col">
          <div className="sticky top-0 z-10 flex h-[52px] shrink-0 items-center gap-3 border-b border-line bg-card px-5">
            <h2 className="m-0 text-body font-semibold text-fg">{t(QUEUE_TITLE[filter])}</h2>
            {loaded && <span className="text-footnote text-fg-3">{tf("inbox.queue.waiting", { n: visible.length })}</span>}
            <span className="flex-1" />
            {canDecide && bulkStage !== null && (
              <Button variant="outline" size={30} icon={<CheckCheck size={14} strokeWidth={1.6} />} disabled={pending} onClick={acceptAllFromStep}>
                {tf("inbox.acceptAll", { step: stageLabel(bulkStage) })}
              </Button>
            )}
          </div>
          {main}
        </div>
      </WithInspector>
      <ReasonSheet
        open={sheet !== null}
        onOpenChange={(open) => {
          if (!open) {
            setSheet(null);
          }
        }}
        title={sheet?.title ?? ""}
        {...(sheet?.description === undefined ? {} : { description: sheet.description })}
        confirmLabel={sheet?.confirmLabel ?? ""}
        destructive={sheet?.destructive ?? false}
        reasonRequired={sheet?.required ?? true}
        {...(sheet?.placeholder === undefined ? {} : { reasonPlaceholder: sheet.placeholder })}
        pending={pending}
        {...(sheetError === null ? {} : { error: sheetError })}
        onConfirm={(reason) => sheet?.run(reason === "" ? null : reason)}
      />
    </div>
  );
}
