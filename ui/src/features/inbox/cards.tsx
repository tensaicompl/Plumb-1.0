// The Inbox inspector cards (handoff "2. Inbox"): one per item type, plus the stale banner, the
// evidence quotes and the multi-select panel. Cards only render; the screen owns every action.
import { useQuery } from "@tanstack/react-query";
import { Check, ChevronDown, TriangleAlert } from "lucide-react";
import { useId, type ReactNode } from "react";

import type { components } from "../../api/schema";
import { Button, cn, DiffLines, EvidenceQuote, Eyebrow, gateName, KINDS, Skeleton, SkeletonGroup, type DiffLine, type GateName } from "../../components/ui";
import { ReadFailure } from "../../components/ReadFailure";
import { t, tf, type MessageKey } from "../../i18n";
import { AnswerControl } from "./AnswerControl";
import { useLabels } from "../model/labels";
import { parseAnswerSchema, type AnswerValue } from "./answerSchema";
import { utf8RangeToUtf16 as utf8Range, type InboxItem } from "./model";
import { proposalDetailQuery, requirementIdentifier, requirementQuery, sentencesQuery, sourceNamesQuery } from "./queries";
import { earsPattern, modalityOf, wordDiff } from "./wording";

type PendingProposalDetail = components["schemas"]["PendingProposalDetail"];
type ProposalFact = components["schemas"]["ProposalFact"];
type DiffItem = components["schemas"]["DiffItem"];

const SEPARATOR = " · ";

/** Node types whose diff line names the kind ("operation Submit leave request"); relations carry a full label. */
const NODE_KINDS: ReadonlySet<string> = new Set([
  "operation",
  "event",
  "outcome",
  "requirement",
  "term",
  "entity",
  "attribute",
  "state",
  "transition",
  "invariant",
  "calculation",
  "scenario",
  "rule",
  "decision_table",
  "process",
  "actor",
  "business_role",
  "stakeholder",
  "calendar",
]);

function itemText(item: DiffItem): string {
  return NODE_KINDS.has(item.kind) ? item.kind.replace(/_/g, " ") + " " + item.label : item.label;
}

/** The "What changes in the model" lines: removed, changed, then added elements. */
export function diffLines(diff: PendingProposalDetail["diff"]): DiffLine[] {
  return [
    ...diff.removed.map((i) => ({ op: "remove" as const, text: itemText(i) })),
    ...diff.modified.map((i) => ({ op: "context" as const, text: tf("inbox.diff.modified", { item: itemText(i) }) })),
    ...diff.added.map((i) => ({ op: "add" as const, text: itemText(i) })),
  ];
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

// ------------------------------------------------------------------ shared pieces

/** The card header: "NEW OPERATION · FLOWS STEP" and the title. */
export function CardHeader({ item }: { item: InboxItem }) {
  return (
    <div>
      <Eyebrow as="p" className="m-0 mb-2.5">
        {item.typeLabel + SEPARATOR + item.originLabel}
      </Eyebrow>
      <h2 className="m-0 select-text text-pretty font-display text-title font-semibold leading-[1.3] tracking-[-0.01em] text-fg">{item.title}</h2>
    </div>
  );
}

/** "Proposed on an older version (v40)…" with Refresh proposal (hidden when it cannot be refreshed). */
export function StaleBanner({ baseRevision, onRefresh, refreshing }: { baseRevision: string; onRefresh?: () => void; refreshing: boolean }) {
  const version = /^rev:(\d+):/.exec(baseRevision)?.[1];
  return (
    <div role="note" className="flex items-center gap-2.5 rounded-tile bg-attention-tint px-3.5 py-3 text-footnote text-on-banner">
      <TriangleAlert size={16} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />
      <span className="flex-1 leading-normal">{version === undefined ? t("inbox.stale.bannerUnknown") : tf("inbox.stale.banner", { version: "v" + version })}</span>
      {onRefresh !== undefined && (
        <button
          type="button"
          onClick={onRefresh}
          disabled={refreshing}
          className="h-7 shrink-0 cursor-pointer whitespace-nowrap rounded-pill bg-card px-3 text-caption font-semibold text-on-banner disabled:opacity-50"
        >
          {t("inbox.stale.refresh")}
        </button>
      )}
    </div>
  );
}

function Facts({ facts }: { facts: readonly ProposalFact[] }) {
  if (facts.length === 0) {
    return null;
  }
  return (
    <ul aria-label={t("inbox.facts")} className="m-0 flex list-none flex-col gap-2.5 p-0 text-footnote">
      {facts.map((fact, index) => (
        <li key={index} className="grid grid-cols-[110px_1fr] gap-x-2">
          <span className="text-fg-3">{fact.label}</span>
          <span className="select-text font-medium text-fg">{fact.value}</span>
        </li>
      ))}
    </ul>
  );
}

function Evidence({ detail, open, onToggle, onOpenSource }: { detail: PendingProposalDetail; open: boolean; onToggle: () => void; onOpenSource: (sourceId: string) => void }) {
  const names = useQuery(sourceNamesQuery);
  const passages = detail.evidence.filter((e) => typeof e.text === "string" && e.text !== "");
  const id = useId();
  if (passages.length === 0) {
    return null;
  }
  return (
    <section aria-labelledby={id}>
      <div className="mb-2 flex items-center justify-between">
        <button
          type="button"
          id={id}
          aria-expanded={open}
          onClick={onToggle}
          className="inline-flex cursor-pointer items-center gap-1 text-caption font-semibold text-fg-2 hover:text-fg"
        >
          {t("inbox.evidence")}
          <ChevronDown size={14} strokeWidth={1.6} aria-hidden="true" className={cn("transition-transform duration-200 ease-standard", !open && "-rotate-90")} />
        </button>
        <span className="text-eyebrow text-fg-3">{t("inbox.evidence.toggle")}</span>
      </div>
      {open && (
        <div className="flex flex-col gap-2">
          {passages.map((e) => {
            const text = e.text as string;
            const range = utf8Range(text, e.highlight_start, e.highlight_end);
            const source = e.source_ref ?? null;
            return (
              <EvidenceQuote
                key={e.id}
                text={text}
                {...(range === undefined ? {} : { highlight: range })}
                sourceId={e.source_identifier ?? e.id}
                {...(source !== null && names.data?.get(source) !== undefined ? { sourceName: names.data.get(source) as string } : {})}
                {...(source !== null ? { onOpenSource: () => onOpenSource(source) } : {})}
              />
            );
          })}
        </div>
      )}
    </section>
  );
}

function ModelChange({ detail }: { detail: PendingProposalDetail }) {
  const lines = diffLines(detail.diff);
  if (lines.length === 0) {
    return null;
  }
  return (
    <div className="rounded-tile border border-line px-3.5 py-3">
      <DiffLines lines={lines} title={t("inbox.diff.title")} className="[&>p]:mb-2 [&>p]:text-fg-2" />
    </div>
  );
}

function DetailSkeleton() {
  return (
    <SkeletonGroup className="flex flex-col gap-3">
      <Skeleton width="70%" />
      <Skeleton width="55%" />
      <Skeleton width="80%" />
      <Skeleton height={72} radius="card" />
    </SkeletonGroup>
  );
}

/** Loads the proposal detail and renders `children` with it (skeleton while loading, typed failure). */
function WithDetail({ id, children }: { id: string; children: (detail: PendingProposalDetail) => ReactNode }) {
  const detail = useQuery(proposalDetailQuery(id));
  if (detail.isError) {
    return <ReadFailure error={detail.error} messageKey="inbox.detail.loadFailed" />;
  }
  if (detail.data === undefined) {
    return <DetailSkeleton />;
  }
  return <>{children(detail.data)}</>;
}

// ------------------------------------------------------------------ proposal cards

const SIDES: Readonly<Record<"link" | "merge" | "supersede", readonly [string, string]>> = {
  link: ["From", "To"],
  merge: ["Keep", "Merge"],
  supersede: ["Old", "New"],
};

function SideBySide({ type, facts }: { type: "link" | "merge" | "supersede"; facts: readonly ProposalFact[] }) {
  const [left, right] = SIDES[type];
  const sides = [facts.find((f) => f.label === left), facts.find((f) => f.label === right)].filter((f): f is ProposalFact => f !== undefined);
  const relation = facts.find((f) => f.label === "Relation");
  if (sides.length < 2) {
    return <Facts facts={facts} />;
  }
  return (
    <>
      <div className="grid grid-cols-2 gap-2.5">
        {sides.map((side) => (
          <div key={side.label} data-testid="link-side" className="rounded-tile border border-line p-3.5">
            <p className="m-0 mb-1.5 text-eyebrow text-fg-3">{side.label}</p>
            <p className="m-0 select-text text-callout font-semibold leading-snug text-fg">{side.value}</p>
          </div>
        ))}
      </div>
      {relation !== undefined && <p className="m-0 text-footnote text-fg-2">{tf("inbox.link.relation", { relation: relation.value })}</p>}
    </>
  );
}

export interface ProposalCardProps {
  readonly item: InboxItem;
  readonly evidenceOpen: boolean;
  readonly onToggleEvidence: () => void;
  readonly onOpenSource: (sourceId: string) => void;
  readonly stale: ReactNode;
}

/** New element, link, merge and supersede: facts (or both sides), evidence and the model change. */
export function ProposalCard({ item, evidenceOpen, onToggleEvidence, onOpenSource, stale }: ProposalCardProps) {
  return (
    <>
      <CardHeader item={item} />
      {stale}
      <WithDetail id={item.id}>
        {(detail) => (
          <>
            {item.type === "link" || item.type === "merge" || item.type === "supersede" ? <SideBySide type={item.type} facts={detail.facts} /> : <Facts facts={detail.facts} />}
            <Evidence detail={detail} open={evidenceOpen} onToggle={onToggleEvidence} onOpenSource={onOpenSource} />
            <ModelChange detail={detail} />
          </>
        )}
      </WithDetail>
    </>
  );
}

/** Answer to confirm: the recorded answer and the model change it causes. */
export function AnswerCard({ item, stale }: { item: InboxItem; stale: ReactNode }) {
  return (
    <>
      <CardHeader item={item} />
      {stale}
      <WithDetail id={item.id}>
        {(detail) => (
          <>
            <Facts facts={detail.facts} />
            {diffLines(detail.diff).length > 0 && <DiffLines variant="boxed" title={t("inbox.answer.change")} lines={diffLines(detail.diff)} />}
          </>
        )}
      </WithDetail>
    </>
  );
}

// ------------------------------------------------------------------ wording

function Slot({ label, value }: { label: string; value: string }) {
  return (
    <div data-testid="slot" className="rounded-field border border-line px-3 py-2.5">
      <p className="m-0 text-eyebrow text-fg-3">{label}</p>
      <p className="m-0 mt-0.5 text-footnote font-semibold text-fg">{value}</p>
    </div>
  );
}

function kindLabel(kind: string): string {
  return (KINDS as readonly string[]).includes(kind) ? t(("ui.kind." + kind) as MessageKey) : capitalize(kind);
}

export interface WordingCardProps {
  readonly item: InboxItem;
  readonly stale: ReactNode;
  /** The edited text while editing (E), else null. */
  readonly editText: string | null;
  readonly onEditText: (text: string) => void;
  readonly evidenceOpen: boolean;
  readonly onToggleEvidence: () => void;
  readonly onOpenSource: (sourceId: string) => void;
}

/** The requirement an EARS proposal rewords (its one modified requirement). */
export function wordingTarget(detail: PendingProposalDetail): string | null {
  return detail.diff.modified.find((d) => d.kind === "requirement")?.id ?? null;
}

/** Wording (EARS): original versus suggested, struck and added words, and the slot tiles. */
export function WordingCard({ item, stale, editText, onEditText, evidenceOpen, onToggleEvidence, onOpenSource }: WordingCardProps) {
  const detail = useQuery(proposalDetailQuery(item.id));
  const target = detail.data === undefined ? null : wordingTarget(detail.data);
  const requirement = useQuery({ ...requirementQuery(target ?? ""), enabled: target !== null });
  const sentences = useQuery({ ...sentencesQuery, enabled: target !== null });
  const fieldId = useId();
  if (detail.isError || requirement.isError) {
    return (
      <>
        <CardHeader item={item} />
        <ReadFailure error={detail.error ?? requirement.error} messageKey="inbox.detail.loadFailed" />
      </>
    );
  }
  if (detail.data !== undefined && target === null) {
    return <ProposalCard item={item} stale={stale} evidenceOpen={evidenceOpen} onToggleEvidence={onToggleEvidence} onOpenSource={onOpenSource} />;
  }
  const req = requirement.data;
  if (req === undefined) {
    return (
      <>
        <CardHeader item={item} />
        {stale}
        <DetailSkeleton />
      </>
    );
  }
  const suggestion = req.ears_proposal_ref === item.id ? (req.ears_suggestion ?? null) : null;
  if (suggestion === null) {
    return <ProposalCard item={item} stale={stale} evidenceOpen={evidenceOpen} onToggleEvidence={onToggleEvidence} onOpenSource={onOpenSource} />;
  }
  const diff = wordDiff(req.text, suggestion);
  const slots = sentences.data?.find((s) => s.id === req.id)?.slots;
  const modality = modalityOf(suggestion) ?? slots?.modality ?? null;
  return (
    <>
      <CardHeader item={item} />
      {stale}
      <div className="flex flex-col gap-2">
        <p className="m-0 text-caption font-semibold text-fg-3">{t("inbox.wording.original")}</p>
        <p data-testid="wording-original" className="m-0 select-text rounded-field bg-surface px-3.5 py-3 text-callout leading-[1.55] text-fg-2">
          {diff.removed.map((run, index) => (
            <span key={index}>
              {index > 0 && " "}
              {run.changed ? (
                <del className="rounded-[3px] bg-diff-del-word text-diff-del line-through">
                  <span className="sr-only">{t("ui.diff.removed")} </span>
                  {run.text}
                </del>
              ) : (
                run.text
              )}
            </span>
          ))}
        </p>
        <p className="m-0 mt-1.5 text-caption font-semibold text-fg-3">
          {editText === null ? t("inbox.wording.suggested") : <label htmlFor={fieldId}>{t("inbox.wording.suggested")}</label>}
        </p>
        {editText === null ? (
          <p data-testid="wording-suggested" className="m-0 select-text rounded-field border border-line px-3.5 py-3 text-callout leading-[1.55] text-fg">
            {diff.added.map((run, index) => (
              <span key={index}>
                {index > 0 && " "}
                {run.changed ? (
                  <ins className="rounded-[3px] bg-diff-add-word text-diff-add no-underline">
                    <span className="sr-only">{t("ui.diff.added")} </span>
                    {run.text}
                  </ins>
                ) : (
                  run.text
                )}
              </span>
            ))}
          </p>
        ) : (
          <textarea
            id={fieldId}
            aria-label={t("inbox.wording.field")}
            value={editText}
            onChange={(event) => onEditText(event.target.value)}
            className="min-h-24 w-full resize-y rounded-field border border-line bg-card p-3 text-callout leading-[1.55] text-fg outline-none focus:border-accent focus:shadow-focus focus-visible:outline-none"
          />
        )}
      </div>
      <div className="grid grid-cols-2 gap-2">
        <Slot label={t("inbox.wording.class")} value={kindLabel(slots?.class ?? req.class)} />
        <Slot label={t("inbox.wording.level")} value={capitalize(slots?.level ?? "—")} />
        <Slot label={t("inbox.wording.modality")} value={capitalize(modality ?? "—")} />
        <Slot label={t("inbox.wording.pattern")} value={t(earsPattern(suggestion))} />
      </div>
    </>
  );
}

/** The suggestion an EARS proposal makes, from the cached requirement read (null until loaded). */
export function useWordingSuggestion(item: InboxItem | null): { requirementId: string; original: string; suggestion: string; etag: string | null } | null {
  const isWording = item?.type === "wording";
  const detail = useQuery({ ...proposalDetailQuery(item?.id ?? ""), enabled: isWording });
  const target = detail.data === undefined ? null : wordingTarget(detail.data);
  const requirement = useQuery({ ...requirementQuery(target ?? ""), enabled: isWording && target !== null });
  const req = requirement.data;
  if (!isWording || req === undefined || req.ears_proposal_ref !== item.id || typeof req.ears_suggestion !== "string") {
    return null;
  }
  return { requirementId: req.id, original: req.text, suggestion: req.ears_suggestion, etag: req.etag };
}

// ------------------------------------------------------------------ question

export interface QuestionCardProps {
  readonly item: InboxItem;
  readonly value: AnswerValue;
  readonly rationale: string;
  readonly onValue: (value: AnswerValue) => void;
  readonly onRationale: (rationale: string) => void;
  /** False for a role that cannot answer: the controls are not shown. */
  readonly canAnswer: boolean;
}

/** Question: linked items, the typed answer and an optional rationale. */
export function QuestionCard({ item, value, rationale, onValue, onRationale, canAnswer }: QuestionCardProps) {
  const question = item.question;
  const answerId = useId();
  const rationaleId = useId();
  // The elements the question links and its choices reference, named in plain language.
  const choices = (parseAnswerSchema(question?.answer_schema) ?? []).flatMap((field) => (field.type === "enum" || field.type === "enum-set" ? field.choices : []));
  const labels = useLabels([...(question?.context_refs ?? []), ...choices]);
  if (question === undefined) {
    return null;
  }
  return (
    <>
      <CardHeader item={item} />
      {question.context_refs.length > 0 && (
        <ul aria-label={t("inbox.question.linked")} className="m-0 flex list-none flex-wrap gap-1.5 p-0">
          {question.context_refs.map((ref) => {
            const name = labels(ref);
            const named = name !== "" && name !== ref;
            return (
              <li
                key={ref}
                title={ref}
                className={cn("inline-flex h-6 items-center rounded-pill border border-line bg-surface px-2.5 text-caption font-medium text-fg-2", !named && "font-mono")}
              >
                {named ? name : ref}
              </li>
            );
          })}
        </ul>
      )}
      {canAnswer && (
        <>
          <div>
            <p id={answerId} className="m-0 mb-2 text-caption font-semibold text-fg-2">
              {t("inbox.question.answer")}
            </p>
            <AnswerControl question={question} value={value} onChange={onValue} labelledBy={answerId} labels={labels} />
          </div>
          <div>
            <label htmlFor={rationaleId} className="mb-2 block text-caption font-semibold text-fg-2">
              {t("inbox.question.rationale")} <span className="font-normal text-fg-3">{SEPARATOR.trim() + " " + t("inbox.question.optional")}</span>
            </label>
            <textarea
              id={rationaleId}
              value={rationale}
              placeholder={t("inbox.question.rationalePlaceholder")}
              onChange={(event) => onRationale(event.target.value)}
              className="min-h-[72px] w-full resize-y rounded-control border border-line bg-card px-3 py-2.5 text-footnote text-fg outline-none placeholder:text-fg-4 focus:border-accent focus:shadow-focus focus-visible:outline-none"
            />
          </div>
        </>
      )}
      {question.stakeholder_ref !== null && <p className="m-0 text-caption text-fg-3">{tf("inbox.question.routed", { who: question.stakeholder_ref })}</p>}
    </>
  );
}

// ------------------------------------------------------------------ issue

function AffectedRequirement({ id }: { id: string }) {
  const requirement = useQuery(requirementQuery(id));
  const label = requirementIdentifier(requirement.data?.evidence[0]?.text) ?? id;
  return (
    <div data-testid="affected-requirement" className="rounded-tile border border-line p-3 text-[12.5px] leading-[1.45] text-fg">
      <p className="m-0 mb-1.5 font-mono text-eyebrow font-medium text-fg-3">{label}</p>
      {requirement.data === undefined ? <Skeleton width="90%" /> : <p className="m-0 select-text">{requirement.data.text}</p>}
    </div>
  );
}

const GATES: readonly string[] = ["I0", "F1", "F2", "F3", "F4"];

/** The gate's plain name ("Requirements"), or its code. */
export function gateLabel(gate: string): string {
  return GATES.includes(gate) ? gateName(gate as GateName) : gate;
}

/** Issue to decide: the blocker callout and the affected items (requirements side by side). */
export function IssueCard({ item }: { item: InboxItem }) {
  const issue = item.issue;
  if (issue === undefined) {
    return null;
  }
  const requirements = issue.affected.filter((ref) => ref.startsWith("req:"));
  const others = issue.affected.filter((ref) => !ref.startsWith("req:"));
  return (
    <>
      <CardHeader item={item} />
      <div role="note" className="flex items-start gap-2.5 rounded-tile bg-blocked-tint px-3.5 py-3 text-footnote text-blocked-fg">
        <TriangleAlert size={16} strokeWidth={1.6} aria-hidden="true" className="mt-0.5 shrink-0" />
        <span className="flex-1 leading-normal">
          <b>{t("inbox.issue.blocker")}</b> {tf("inbox.issue.inGate", { gate: gateLabel(issue.gate) })}
          <br />
          <span className="select-text font-mono text-eyebrow opacity-80">{issue.code}</span>
        </span>
      </div>
      {requirements.length > 0 && (
        <div className={cn("grid gap-2.5", requirements.length > 1 && "grid-cols-2")}>
          {requirements.slice(0, 4).map((ref) => (
            <AffectedRequirement key={ref} id={ref} />
          ))}
        </div>
      )}
      {others.length > 0 && (
        <ul aria-label={t("inbox.issue.affected")} className="m-0 flex list-none flex-wrap gap-1.5 p-0">
          {others.map((ref) => (
            <li key={ref} className="inline-flex h-6 items-center rounded-pill border border-line bg-surface px-2.5 font-mono text-caption text-fg-2">
              {ref}
            </li>
          ))}
        </ul>
      )}
      <p className="m-0 text-footnote text-fg-2">{t("inbox.issue.help")}</p>
    </>
  );
}

// ------------------------------------------------------------------ multi-select

export function MultiPanel({ count, acceptable, onAccept, onClear, pending, canDecide }: { count: number; acceptable: number; onAccept: () => void; onClear: () => void; pending: boolean; canDecide: boolean }) {
  const skipped = count - acceptable;
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center">
      <p className="m-0 font-display text-h1 font-bold leading-none text-fg">{count}</p>
      <p className="m-0 text-body text-fg-2">{t("inbox.multi.selected")}</p>
      <div className="mt-3 flex gap-2">
        {canDecide && acceptable > 0 && (
          <Button variant="primary" size={38} className="h-10 px-5" onClick={onAccept} pending={pending} icon={<CheckIcon />}>
            {tf("inbox.multi.accept", { n: acceptable })}
          </Button>
        )}
        <Button variant="outline" size={38} className="h-10 px-[18px]" onClick={onClear}>
          {t("inbox.multi.clear")}
        </Button>
      </div>
      <p className="m-0 max-w-[260px] text-caption text-fg-3">{t("inbox.multi.note")}</p>
      {skipped > 0 && <p className="m-0 max-w-[260px] text-caption text-fg-3">{tf(skipped === 1 ? "inbox.multi.skippedOne" : "inbox.multi.skipped", { n: skipped })}</p>}
    </div>
  );
}

export function CheckIcon() {
  return <Check size={16} strokeWidth={1.8} aria-hidden="true" />;
}
