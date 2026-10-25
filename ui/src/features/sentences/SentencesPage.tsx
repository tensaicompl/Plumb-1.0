import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";

import { apiClient, readReviewed, revisionEtag, ReviewedEtag, useReviewedEtag, type ReviewedItems } from "../../api/client";
import type { components } from "../../api/schema";
import { PreconditionFailedError, useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { ReadFailure } from "../../components/ReadFailure";
import { t } from "../../i18n";
import {
  entitiesQueryKey,
  fetchQuestions,
  invalidateU22,
  QuestionAnswerCard,
  questionsQueryKey,
  sentencesQueryKey,
  writeErrorCode,
  type Question,
} from "../questions/QuestionsPage";
import { RoleGate } from "../../components/RoleGate";

type Sentence = components["schemas"]["Sentence"];
type SentencePage = components["schemas"]["SentencePage"];
type Entity = components["schemas"]["Entity"];
type EntityPage = components["schemas"]["EntityPage"];
type WriteResult = components["schemas"]["WriteResult"];
type SlotPatch = components["schemas"]["SentenceSlotPatch"];

/** The closed metamodel vocabularies of the structured edit form (Hotfix 076). */
export const REQUIREMENT_KINDS = ["functional", "quality", "interface", "data", "security", "operational", "compliance", "transition", "constraint"] as const;
export const REQUIREMENT_LEVELS = ["stakeholder", "system", "software", "subsystem", "component", "interface"] as const;
export const MODALITIES = ["shall", "should", "may", "shall_not"] as const;

const PROPOSED = "Proposed";

/** The open Questions by ID, with the ETag of their reviewed read. */
interface ReviewedQuestions {
  readonly byId: ReadonlyMap<string, Question>;
  readonly etag: string | null;
}

async function fetchEntities(): Promise<Entity[]> {
  return ((await readReviewed(apiClient.GET("/api/model/entities"))) as EntityPage).items;
}

/** GET /api/model/sentences, optionally filtered by the server to one entity's sentences. */
async function fetchSentences(entity?: string): Promise<ReviewedItems<Sentence>> {
  const page = await readReviewed(apiClient.GET("/api/model/sentences", { params: { query: entity === undefined ? {} : { entity } } }));
  return { items: (page as SentencePage).items, etag: page.etag };
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

function SlotSelect({
  labelKey,
  values,
  value,
  field,
  onChange,
}: {
  labelKey: "sentences.edit.class" | "sentences.edit.level" | "sentences.edit.modality";
  values: readonly string[];
  value: string;
  field: string;
  onChange: (next: string) => void;
}) {
  return (
    <label className="block text-sm">
      {t(labelKey)}
      <select className="ml-2 rounded border" value={value} data-slot={field} onChange={(event) => onChange(event.target.value)}>
        {values.map((v) => (
          <option key={v} value={v}>
            {v}
          </option>
        ))}
      </select>
    </label>
  );
}

/** The review controls of one sentence: structured slot edit, accept/reject and inline unknowns. */
function SentenceReview({ sentence, openQuestions }: { sentence: Sentence; openQuestions: ReviewedQuestions | undefined }) {
  const queryClient = useQueryClient();
  const [slots, setSlots] = useState(sentence.slots);
  const changes: SlotPatch = {};
  if (slots.class !== sentence.slots.class) {
    changes.class = slots.class;
  }
  if (slots.level !== sentence.slots.level) {
    changes.level = slots.level;
  }
  if (slots.modality !== sentence.slots.modality) {
    changes.modality = slots.modality;
  }
  const changed = Object.keys(changes).length > 0;
  const path = { id: sentence.id };
  const onWritten = { onSuccess: () => void invalidateU22(queryClient) };
  const reviewed = useReviewedEtag();
  const edit = useVersionedMutation<WriteResult, SlotPatch>(
    (patch, ifMatch) => apiClient.POST("/api/model/sentences/{id}/edit", { params: { path, header: { "If-Match": ifMatch } }, body: { slots: patch } }),
    reviewed,
  );
  const accept = useVersionedMutation<WriteResult>(
    (_, ifMatch) => apiClient.POST("/api/model/sentences/{id}/accept", { params: { path, header: { "If-Match": ifMatch } } }),
    reviewed,
  );
  const reject = useVersionedMutation<WriteResult>(
    (_, ifMatch) => apiClient.POST("/api/model/sentences/{id}/reject", { params: { path, header: { "If-Match": ifMatch } } }),
    reviewed,
  );

  return (
    <div className="space-y-3 border-t pt-2" data-testid="sentence-review">
      <RoleGate>
      <form
        aria-label={t("sentences.edit.title")}
        className="space-y-1"
        onSubmit={(event) => {
          event.preventDefault();
          if (changed) {
            edit.mutate(changes, onWritten);
          }
        }}
      >
        <SlotSelect labelKey="sentences.edit.class" field="class" values={REQUIREMENT_KINDS} value={slots.class} onChange={(v) => setSlots((s) => ({ ...s, class: v }))} />
        <SlotSelect labelKey="sentences.edit.level" field="level" values={REQUIREMENT_LEVELS} value={slots.level} onChange={(v) => setSlots((s) => ({ ...s, level: v }))} />
        <SlotSelect labelKey="sentences.edit.modality" field="modality" values={MODALITIES} value={slots.modality} onChange={(v) => setSlots((s) => ({ ...s, modality: v }))} />
        <button type="submit" className="rounded border px-3 py-1" disabled={!changed || edit.isPending} data-testid="sentence-save">
          {t("sentences.edit.save")}
        </button>
        <WriteError error={edit.error} />
      </form>
      {sentence.status === PROPOSED && (
        <div className="flex gap-2">
          <button type="button" className="rounded border px-3 py-1" data-testid="sentence-accept" disabled={accept.isPending} onClick={() => accept.mutate(undefined, onWritten)}>
            {t("sentences.accept")}
          </button>
          <button type="button" className="rounded border px-3 py-1" data-testid="sentence-reject" disabled={reject.isPending} onClick={() => reject.mutate(undefined, onWritten)}>
            {t("sentences.reject")}
          </button>
        </div>
      )}
      <WriteError error={accept.error} />
      <WriteError error={reject.error} />
      </RoleGate>
      {sentence.unknowns.length > 0 && (
        <section aria-label={t("sentences.unknowns")} className="space-y-2">
          <h4 className="font-semibold">{t("sentences.unknowns")}</h4>
          {sentence.unknowns.map((unknown) => {
            const question = openQuestions?.byId.get(unknown.question_id);
            return (
              <div key={unknown.question_id + unknown.slot} className="rounded border p-2" data-unknown={unknown.question_id}>
                <p className="text-sm">
                  {t("sentences.unknownSlot")} <span className="font-mono">{unknown.slot}</span>
                </p>
                {question === undefined ? (
                  <p className="text-sm" data-testid="unknown-unavailable">
                    {t("sentences.unknownUnavailable")} <span className="font-mono">{unknown.question_id}</span>
                  </p>
                ) : (
                  <>
                    <p className="text-sm">{question.text}</p>
                    <ReviewedEtag.Provider value={openQuestions?.etag}>
                      <QuestionAnswerCard question={question} />
                    </ReviewedEtag.Provider>
                  </>
                )}
              </div>
            );
          })}
        </section>
      )}
    </div>
  );
}

function SentenceItem({
  sentence,
  viewed,
  onViewed,
  openQuestions,
}: {
  sentence: Sentence;
  viewed: boolean;
  onViewed: (id: string) => void;
  openQuestions: ReviewedQuestions | undefined;
}) {
  const [open, setOpen] = useState(false);
  return (
    <li className="space-y-1 rounded border p-2 text-sm" data-sentence-id={sentence.id} data-viewed={viewed}>
      <p data-testid="sentence-text">{sentence.text}</p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4">
        <dt>{t("sentences.id")}</dt>
        <dd className="font-mono">{sentence.id}</dd>
        <dt>{t("sentences.status")}</dt>
        <dd className="font-mono" data-field="status">
          {sentence.status}
        </dd>
        <dt>{t("sentences.edit.class")}</dt>
        <dd className="font-mono">{sentence.slots.class}</dd>
        <dt>{t("sentences.edit.level")}</dt>
        <dd className="font-mono">{sentence.slots.level}</dd>
        <dt>{t("sentences.edit.modality")}</dt>
        <dd className="font-mono">{sentence.slots.modality}</dd>
        <dt>{t("sentences.evidence")}</dt>
        <dd className="font-mono">{sentence.evidence.length === 0 ? t("sentences.none") : sentence.evidence.join(", ")}</dd>
        <dt>{t("sentences.unknowns")}</dt>
        <dd className="font-mono">{sentence.unknowns.length === 0 ? t("sentences.none") : sentence.unknowns.map((u) => u.slot + " " + u.question_id).join(", ")}</dd>
      </dl>
      <button
        type="button"
        className="rounded border px-2"
        aria-expanded={open}
        data-testid="sentence-review-toggle"
        onClick={() => {
          if (!open) {
            onViewed(sentence.id);
          }
          setOpen(!open);
        }}
      >
        {t(open ? "sentences.reviewClose" : "sentences.review")}
      </button>
      {open && <SentenceReview sentence={sentence} openQuestions={openQuestions} />}
    </li>
  );
}

interface BulkProgress {
  readonly total: number;
  readonly accepted: number;
  readonly state: "running" | "done" | "conflict" | "failed";
}

/**
 * One section of sentences with sequential bulk accept of its Proposed sentences: each accept is a
 * separate versioned write awaited before the next, so every request carries the semantic hash
 * current after the previous success; the first 412 (or any failure) stops the run with no retry.
 * Unviewed targets require an explicit warning confirmation first; cancelling sends nothing.
 */
function SentenceSection({
  sectionId,
  label,
  title,
  sentences,
  etag,
  viewed,
  onViewed,
  openQuestions,
}: {
  sectionId: string;
  label: string;
  title: ReactNode;
  sentences: readonly Sentence[];
  etag: string | null;
  viewed: ReadonlySet<string>;
  onViewed: (id: string) => void;
  openQuestions: ReviewedQuestions | undefined;
}) {
  const queryClient = useQueryClient();
  // The first write is based on the reviewed section; each further write on the revision the
  // previous accept of this run produced (no other change can have intervened without a 412).
  const accept = useVersionedMutation<WriteResult, { id: string; etag: string | null }>(
    (target, ifMatch) => apiClient.POST("/api/model/sentences/{id}/accept", { params: { path: { id: target.id }, header: { "If-Match": ifMatch } } }),
    (target) => target.etag,
  );
  const [warning, setWarning] = useState<readonly string[] | null>(null);
  const [progress, setProgress] = useState<BulkProgress | null>(null);
  const targets = sentences.filter((s) => s.status === PROPOSED).map((s) => s.id);

  const run = async (ids: readonly string[]) => {
    setWarning(null);
    let accepted = 0;
    let based = etag;
    setProgress({ total: ids.length, accepted, state: "running" });
    for (const id of ids) {
      try {
        based = revisionEtag((await accept.mutateAsync({ id, etag: based })).revision);
      } catch (error) {
        setProgress({ total: ids.length, accepted, state: error instanceof PreconditionFailedError ? "conflict" : "failed" });
        await invalidateU22(queryClient);
        return;
      }
      accepted += 1;
      setProgress({ total: ids.length, accepted, state: "running" });
    }
    setProgress({ total: ids.length, accepted, state: "done" });
    await invalidateU22(queryClient);
  };

  const start = () => {
    const ids = [...targets];
    const unviewed = ids.filter((id) => !viewed.has(id));
    if (unviewed.length > 0) {
      setWarning(ids);
    } else {
      void run(ids);
    }
  };

  const running = progress?.state === "running";
  const unviewedCount = warning === null ? 0 : warning.filter((id) => !viewed.has(id)).length;

  return (
    <section aria-label={label} data-section={sectionId} className="space-y-2 rounded border p-3">
      <h2 className="font-semibold">{title}</h2>
      {sentences.length === 0 ? (
        <p className="text-sm">{t("sentences.sectionEmpty")}</p>
      ) : (
        <ReviewedEtag.Provider value={etag}>
          <ul className="space-y-2">
            {sentences.map((sentence) => (
              <SentenceItem key={sentence.id} sentence={sentence} viewed={viewed.has(sentence.id)} onViewed={onViewed} openQuestions={openQuestions} />
            ))}
          </ul>
        </ReviewedEtag.Provider>
      )}
      <RoleGate>
      <button type="button" className="rounded border px-3 py-1" data-testid="bulk-accept" disabled={targets.length === 0 || running || warning !== null} onClick={start}>
        {t("sentences.bulk.accept")} <span data-testid="bulk-targets">{targets.length}</span>
      </button>
      </RoleGate>
      {warning !== null && (
        <div role="alertdialog" aria-label={t("sentences.bulk.warningTitle")} className="space-y-2 rounded border border-amber-600 p-2 text-sm" data-testid="bulk-warning">
          <p>
            {t("sentences.bulk.warning")} <span data-testid="bulk-unviewed">{unviewedCount}</span>
          </p>
          <div className="flex gap-2">
            <button type="button" className="rounded border px-3 py-1" data-testid="bulk-warning-confirm" onClick={() => void run(warning)}>
              {t("sentences.bulk.confirm")}
            </button>
            <button type="button" className="rounded border px-3 py-1" data-testid="bulk-warning-cancel" onClick={() => setWarning(null)}>
              {t("sentences.bulk.cancel")}
            </button>
          </div>
        </div>
      )}
      {progress !== null && (
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 text-sm" data-testid="bulk-progress" data-state={progress.state}>
          <dt>{t("sentences.bulk.accepted")}</dt>
          <dd data-testid="bulk-accepted">{progress.accepted}</dd>
          <dt>{t("sentences.bulk.remaining")}</dt>
          <dd data-testid="bulk-remaining">{progress.total - progress.accepted}</dd>
        </dl>
      )}
      {progress?.state === "conflict" && (
        <p role="alert" className="text-sm text-red-800" data-testid="bulk-conflict">
          {t("sentences.bulk.conflict")}
        </p>
      )}
      {progress?.state === "failed" && (
        <p role="alert" className="text-sm text-red-800" data-testid="bulk-failed">
          {t("sentences.bulk.failed")}
        </p>
      )}
    </section>
  );
}

/**
 * Sentences grouped by entity (v2 U4 as refined by Hotfix 076): sections come from the server
 * entities and membership from the server `entity` filter; sentences no entity filter returns
 * appear under Ungrouped. Nothing is inferred from text or IDs.
 */
export function SentencesPage() {
  const [viewed, setViewed] = useState<ReadonlySet<string>>(new Set());
  const entities = useQuery({ queryKey: [...entitiesQueryKey, "all"], queryFn: fetchEntities });
  const all = useQuery({ queryKey: [...sentencesQueryKey, "all"], queryFn: () => fetchSentences() });
  const openQuestions = useQuery({ queryKey: [...questionsQueryKey, "open"], queryFn: () => fetchQuestions("Open") });
  const perEntity = useQueries({
    queries: (entities.data ?? []).map((entity) => ({
      queryKey: [...sentencesQueryKey, "entity", entity.id],
      queryFn: () => fetchSentences(entity.id),
    })),
  });

  const onViewed = (id: string) =>
    setViewed((current) => {
      if (current.has(id)) {
        return current;
      }
      const next = new Set(current);
      next.add(id);
      return next;
    });

  const title = <h1 className="text-xl font-semibold">{t("sentences.title")}</h1>;
  if (entities.isError || all.isError || perEntity.some((q) => q.isError)) {
    return (
      <div className="space-y-4">
        {title}
        <ReadFailure error={entities.error ?? all.error ?? perEntity.find((q) => q.isError)?.error} messageKey="sentences.loadFailed" />
      </div>
    );
  }
  if (entities.data === undefined || all.data === undefined || perEntity.some((q) => q.data === undefined)) {
    return (
      <div className="space-y-4">
        {title}
        <p>{t("sentences.loading")}</p>
      </div>
    );
  }
  const questionMap: ReviewedQuestions | undefined =
    openQuestions.data === undefined ? undefined : { byId: new Map(openQuestions.data.items.map((q) => [q.id, q] as const)), etag: openQuestions.data.etag };
  const grouped = new Set<string>();
  for (const query of perEntity) {
    for (const sentence of query.data?.items ?? []) {
      grouped.add(sentence.id);
    }
  }
  const ungrouped = all.data.items.filter((sentence) => !grouped.has(sentence.id));

  return (
    <div className="space-y-4">
      {title}
      {entities.data.map((entity, index) => (
        <SentenceSection
          key={entity.id}
          sectionId={entity.id}
          label={entity.name}
          title={
            <>
              {entity.name} <span className="font-mono text-sm">{entity.id}</span>
            </>
          }
          sentences={perEntity[index]?.data?.items ?? []}
          etag={perEntity[index]?.data?.etag ?? null}
          viewed={viewed}
          onViewed={onViewed}
          openQuestions={questionMap}
        />
      ))}
      {ungrouped.length > 0 && (
        <SentenceSection sectionId="ungrouped" label={t("sentences.ungrouped")} title={t("sentences.ungrouped")} sentences={ungrouped} etag={all.data.etag} viewed={viewed} onViewed={onViewed} openQuestions={questionMap} />
      )}
    </div>
  );
}
