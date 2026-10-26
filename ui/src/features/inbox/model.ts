// The Inbox item model (handoff "2. Inbox", spec §5.2): one triage queue for every pending
// decision. Proposals come from GET /api/proposals, questions from the open questions without a
// pending answer, issues from the blockers waiting for a decision (src/app/work.ts), so the
// sidebar badge counts exactly what the queue lists.
import type { components } from "../../api/schema";
import { stageLabel, type StageName } from "../../app/pipeline";
import { issuesToDecide, questionsAwaitingAnswer, type IssueToDecide } from "../../app/work";
import { t, tf, type MessageKey } from "../../i18n";

type PendingProposal = components["schemas"]["PendingProposal"];
type Question = components["schemas"]["Question"];
type Finding = components["schemas"]["Finding"];
type GateStatus = components["schemas"]["GateStatus"];
type ElementKind = components["schemas"]["ProposalElementKind"];

export type InboxItemType = "create" | "link" | "merge" | "supersede" | "wording" | "answer" | "question" | "issue";

/** The "By origin" groups. */
export type InboxOrigin = "steps" | "questions" | "assistant";

/** The smart filters and origin groups (`?filter=`). */
export const INBOX_FILTERS = ["all", "proposals", "questions", "wording", "issues", "steps", "fromq", "assistant"] as const;
export type InboxFilter = (typeof INBOX_FILTERS)[number];

export interface InboxItem {
  /** The route ID (`/inbox/$id`): the proposal, question or finding ID. */
  readonly id: string;
  readonly type: InboxItemType;
  readonly title: string;
  /** "New operation", "Link", "Question", … */
  readonly typeLabel: string;
  readonly origin: InboxOrigin;
  /** "Flows step", "From a question", … */
  readonly originLabel: string;
  /** The stage of a stage-origin proposal. */
  readonly stage: StageName | null;
  readonly createdAt: string | null;
  readonly stale: boolean;
  /** Whether Accept (accept-set) applies: a proposal that is not stale. */
  readonly acceptable: boolean;
  readonly proposal?: PendingProposal;
  readonly question?: Question;
  readonly issue?: IssueToDecide;
}

const STAGES: readonly string[] = ["intake", "extract", "consolidate", "lint", "directory", "analyse", "flows", "calc", "rules", "scenarios", "tags", "run"];

function asStage(stage: string | null | undefined): StageName | null {
  return typeof stage === "string" && STAGES.includes(stage) ? (stage as StageName) : null;
}

const ELEMENT_KIND: Readonly<Record<ElementKind, MessageKey>> = {
  operation: "inbox.kind.operation",
  event: "inbox.kind.event",
  outcome: "inbox.kind.outcome",
  requirement: "inbox.kind.requirement",
  glossary_term: "inbox.kind.glossaryTerm",
  entity: "inbox.kind.entity",
  attribute: "inbox.kind.attribute",
  state: "inbox.kind.state",
  transition: "inbox.kind.transition",
  invariant: "inbox.kind.invariant",
  calculation: "inbox.kind.calculation",
  scenario: "inbox.kind.scenario",
  rule: "inbox.kind.rule",
  decision_table: "inbox.kind.decisionTable",
  process: "inbox.kind.process",
  link: "inbox.kind.link",
  merge: "inbox.kind.merge",
  supersede: "inbox.kind.supersede",
  answer: "inbox.kind.answer",
  directory_element: "inbox.kind.directoryElement",
  other: "inbox.kind.other",
};

/** The plain name of an element kind ("operation"). */
export function elementKindLabel(kind: ElementKind): string {
  return t(ELEMENT_KIND[kind]);
}

const TYPE_LABEL: Readonly<Record<Exclude<InboxItemType, "create">, MessageKey>> = {
  link: "inbox.type.link",
  merge: "inbox.type.merge",
  supersede: "inbox.type.supersede",
  wording: "inbox.type.wording",
  answer: "inbox.type.answer",
  question: "inbox.type.question",
  issue: "inbox.type.issue",
};

/** Whether a pending proposal is an S1.2 EARS wording suggestion (a consolidate-step requirement change). */
export function isWordingProposal(p: PendingProposal): boolean {
  return p.element_kind === "requirement" && p.origin === "stage" && p.stage === "consolidate";
}

function proposalType(p: PendingProposal): InboxItemType {
  if (p.origin === "question") {
    return "answer";
  }
  if (isWordingProposal(p)) {
    return "wording";
  }
  if (p.element_kind === "link" || p.element_kind === "merge" || p.element_kind === "supersede") {
    return p.element_kind;
  }
  return "create";
}

function proposalOrigin(p: PendingProposal): { origin: InboxOrigin; label: string } {
  switch (p.origin) {
    case "question":
      return { origin: "questions", label: t("inbox.origin.question") };
    case "session":
      return { origin: "assistant", label: t("inbox.origin.assistant") };
    case "intake":
    case "stage": {
      // A directory job's configuration-supplied elements are intake proposals of a step.
      if (p.origin === "intake" && (p.stage ?? null) === null) {
        return { origin: "steps", label: t("inbox.origin.intake") };
      }
      const stage = asStage(p.stage);
      return { origin: "steps", label: stage === null ? t("inbox.origin.step") : tf("inbox.origin.stage", { step: stageLabel(stage) }) };
    }
  }
}

function proposalItem(p: PendingProposal): InboxItem {
  const type = proposalType(p);
  const origin = proposalOrigin(p);
  return {
    id: p.proposal_id,
    type,
    title: p.title,
    typeLabel: type === "create" ? tf("inbox.type.create", { kind: elementKindLabel(p.element_kind) }) : t(TYPE_LABEL[type]),
    origin: origin.origin,
    originLabel: origin.label,
    stage: p.origin === "stage" || p.origin === "intake" ? asStage(p.stage) : null,
    createdAt: p.created_at,
    stale: p.stale,
    acceptable: !p.stale,
    proposal: p,
  };
}

function questionItem(q: Question): InboxItem {
  return {
    id: q.id,
    type: "question",
    title: q.text,
    typeLabel: t("inbox.type.question"),
    origin: "steps",
    originLabel: t("inbox.origin.questions"),
    stage: null,
    createdAt: null,
    stale: false,
    acceptable: false,
    question: q,
  };
}

function issueItem(issue: IssueToDecide): InboxItem {
  return {
    id: issue.id,
    type: "issue",
    title: issue.message ?? tf("inbox.issue.ruleTitle", { rule: issue.code }),
    typeLabel: t("inbox.type.issue"),
    origin: "steps",
    originLabel: t("inbox.origin.quality"),
    stage: null,
    createdAt: null,
    stale: false,
    acceptable: false,
    issue,
  };
}

function byNewest(a: InboxItem, b: InboxItem): number {
  const time = (b.createdAt ?? "").localeCompare(a.createdAt ?? "");
  return time !== 0 ? time : a.title.localeCompare(b.title);
}

export interface InboxSources {
  readonly proposals: readonly PendingProposal[];
  readonly questions: readonly Question[];
  readonly findings: readonly Finding[];
  readonly gates: readonly GateStatus[];
}

/**
 * Every pending decision, in queue order: proposals newest first, then questions by priority,
 * wording suggestions, answers to confirm and issues to decide.
 */
export function buildInboxItems({ proposals, questions, findings, gates }: InboxSources): InboxItem[] {
  const fromProposals = proposals.map(proposalItem);
  const group = (type: InboxItemType) => fromProposals.filter((i) => i.type === type).sort(byNewest);
  const changes = fromProposals.filter((i) => i.type !== "wording" && i.type !== "answer").sort(byNewest);
  const asked = questionsAwaitingAnswer(questions, proposals)
    .slice()
    .sort((a, b) => (a.priority ?? Number.MAX_SAFE_INTEGER) - (b.priority ?? Number.MAX_SAFE_INTEGER))
    .map(questionItem);
  return [...changes, ...asked, ...group("wording"), ...group("answer"), ...issuesToDecide(findings, gates).map(issueItem)];
}

const PROPOSAL_TYPES: ReadonlySet<InboxItemType> = new Set(["create", "link", "merge", "supersede", "answer"]);

/** Whether `item` is listed under `filter`. */
export function matchesFilter(item: InboxItem, filter: InboxFilter): boolean {
  switch (filter) {
    case "all":
      return true;
    case "proposals":
      return PROPOSAL_TYPES.has(item.type);
    case "questions":
      return item.type === "question";
    case "wording":
      return item.type === "wording";
    case "issues":
      return item.type === "issue";
    case "steps":
      return item.origin === "steps";
    case "fromq":
      return item.origin === "questions";
    case "assistant":
      return item.origin === "assistant";
  }
}

/** The count of every filter. */
export function filterCounts(items: readonly InboxItem[]): Record<InboxFilter, number> {
  const counts = Object.fromEntries(INBOX_FILTERS.map((f) => [f, 0])) as Record<InboxFilter, number>;
  for (const item of items) {
    for (const f of INBOX_FILTERS) {
      if (matchesFilter(item, f)) {
        counts[f] += 1;
      }
    }
  }
  return counts;
}

/** The Inbox search parameters: `filter` (unknown values fall back to All). */
export function parseInboxSearch(search: Record<string, unknown>): { filter?: InboxFilter } {
  const value = search["filter"];
  return typeof value === "string" && (INBOX_FILTERS as readonly string[]).includes(value) ? { filter: value as InboxFilter } : {};
}

/** The queue age ("now", "4 min", "1 h", "2 d"); empty when unknown. */
export function formatAge(createdAt: string | null, now: Date = new Date()): string {
  if (createdAt === null) {
    return "";
  }
  const time = Date.parse(createdAt);
  if (Number.isNaN(time)) {
    return "";
  }
  const minutes = Math.floor((now.getTime() - time) / 60_000);
  if (minutes < 1) {
    return t("inbox.age.now");
  }
  if (minutes < 60) {
    return tf("inbox.age.minutes", { n: minutes });
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return tf("inbox.age.hours", { n: hours });
  }
  return tf("inbox.age.days", { n: Math.floor(hours / 24) });
}

/** The string index range of a UTF-8 byte range of `text` (the API's evidence highlight offsets). */
export function utf8RangeToUtf16(text: string, start: number | null | undefined, end: number | null | undefined): { start: number; end: number } | undefined {
  if (typeof start !== "number" || typeof end !== "number") {
    return undefined;
  }
  let bytes = 0;
  let from = -1;
  let to = -1;
  let index = 0;
  for (const char of text) {
    if (from < 0 && bytes >= start) {
      from = index;
    }
    if (to < 0 && bytes >= end) {
      to = index;
    }
    const code = char.codePointAt(0) ?? 0;
    bytes += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
    index += char.length;
  }
  return { start: from < 0 ? index : from, end: to < 0 ? index : to };
}
