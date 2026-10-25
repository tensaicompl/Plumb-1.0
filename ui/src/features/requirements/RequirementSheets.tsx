import { useId, useState } from "react";

import { apiClient, revisionEtag, type Reviewed } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError, writeErrorMessage } from "../../api/useVersionedMutation";
import { Button, cn, confirmShortcut, KINDS, Sheet, type RequirementKind } from "../../components/ui";
import { t, tf, type MessageKey } from "../../i18n";
import { requirementLabel, type RequirementDetail, type RequirementSummary } from "./data";

type WriteResult = components["schemas"]["WriteResult"];
type Preview = components["schemas"]["SemanticChangePreviewResponse"];

interface SheetProps {
  readonly requirement: Reviewed<RequirementDetail>;
  /** The requirement's short label ("HR-009"). */
  readonly label: string;
  readonly onClose: () => void;
  /** Called after a successful write with the toast message. */
  readonly onDone: (message: string) => void;
}

const FIELD =
  "w-full rounded-field border border-line bg-card p-3 text-callout text-fg outline-none placeholder:text-fg-4 focus:border-accent focus:shadow-focus focus-visible:outline-none";

/** The explanation of a failed write shown in a sheet (none for a 412: the conflict sheet opens). */
function failureText(error: Error | null): string | null {
  if (error === null || error.name === "PreconditionFailedError") {
    return null;
  }
  return tf("requirement.writeFailed", { message: writeErrorMessage(error) ?? error.message });
}

function WriteError({ error }: { error: Error | null }) {
  const text = failureText(error);
  return text === null ? null : (
    <p role="alert" className="m-0 text-footnote text-blocked-fg">
      {text}
    </p>
  );
}

function kindLabel(kind: string): string {
  return t(("ui.kind." + kind) as MessageKey);
}

/** Change kind (`classify_requirement`): one of the nine requirement classes. */
export function ChangeKindSheet({ requirement, label, onClose, onDone }: SheetProps) {
  const [kind, setKind] = useState<RequirementKind>(requirement.class);
  const name = useId();
  const classify = useVersionedMutation<WriteResult, RequirementKind>(
    (value, ifMatch) =>
      apiClient.POST("/api/requirements/{id}/classify", { params: { path: { id: requirement.id }, header: { "If-Match": ifMatch } }, body: { class: value } }),
    requirement.etag,
  );
  const confirm = () =>
    classify.mutate(kind, {
      onSuccess: () => {
        onClose();
        onDone(tf("requirement.kind.done", { id: label, kind: kindLabel(kind) }));
      },
      onError: (error) => {
        if (error.name === "PreconditionFailedError") {
          onClose();
        }
      },
    });
  return (
    <Sheet
      open
      onOpenChange={(open) => !open && onClose()}
      title={t("requirement.kind.title")}
      description={tf("requirement.kind.description", { id: label })}
      confirmLabel={t("requirement.kind.confirm")}
      onConfirm={confirm}
      confirmDisabled={kind === requirement.class}
      pending={classify.isPending}
    >
      <fieldset className="m-0 grid grid-cols-3 gap-1.5 border-0 p-0">
        <legend className="sr-only">{t("requirement.kind.label")}</legend>
        {KINDS.map((option) => (
          <label
            key={option}
            className={cn(
              "flex h-[34px] cursor-pointer items-center gap-2 rounded-control border px-2.5 text-footnote font-medium",
              option === kind ? "border-primary bg-surface text-fg" : "border-line bg-card text-fg-2 hover:bg-surface",
            )}
          >
            <input type="radio" name={name} value={option} checked={option === kind} onChange={() => setKind(option)} className="accent-primary" />
            {kindLabel(option)}
          </label>
        ))}
      </fieldset>
      <WriteError error={classify.error} />
    </Sheet>
  );
}

interface ReviewedPreview {
  readonly preview: Preview;
  readonly parts: string[];
  readonly reason: string;
}

function isPreviewMismatch(error: unknown): boolean {
  return (
    error instanceof WriteFailedError &&
    error.status === 409 &&
    typeof error.body === "object" &&
    error.body !== null &&
    (error.body as Record<string, unknown>)["code"] === "E_PREVIEW_MISMATCH"
  );
}

/**
 * Split into two (`preview_requirement_split`, then `split_requirement`): the server previews the
 * exact change; Split submits that reviewed preview against its own revision. A project change in
 * between makes the preview out of date: preview again.
 */
export function SplitSheet({ requirement, label, onClose, onDone }: SheetProps) {
  const ids = useId();
  const [parts, setParts] = useState<[string, string]>(["", ""]);
  const [reason, setReason] = useState("");
  const [reviewed, setReviewed] = useState<ReviewedPreview | null>(null);
  const [stale, setStale] = useState(false);
  const path = { id: requirement.id };
  const preview = useVersionedMutation<Preview, { parts: string[]; reason: string }>(
    (body, ifMatch) => apiClient.POST("/api/requirements/{id}/split/preview", { params: { path, header: { "If-Match": ifMatch } }, body }),
    requirement.etag,
  );
  const split = useVersionedMutation<WriteResult, ReviewedPreview>(
    (target, ifMatch) =>
      apiClient.POST("/api/requirements/{id}/split", {
        params: { path, header: { "If-Match": ifMatch } },
        body: { parts: target.parts, reason: target.reason, preview: { proposal_id: target.preview.proposal_id, built_at: target.preview.built_at } },
      }),
    (target) => revisionEtag(target.preview.revision),
  );
  const trimmed = parts.map((part) => part.trim());
  const ready = trimmed.every((part) => part !== "") && reason.trim() !== "";
  const edit = (update: () => void) => {
    update();
    setReviewed(null);
    split.reset();
  };
  const requestPreview = () => {
    setStale(false);
    split.reset();
    const body = { parts: trimmed, reason: reason.trim() };
    preview.mutate(body, {
      onSuccess: (data) => setReviewed({ preview: data, parts: body.parts, reason: body.reason }),
      onError: (error) => {
        if (error.name === "PreconditionFailedError") {
          onClose();
        }
      },
    });
  };
  const confirm = () => {
    if (reviewed === null) {
      return;
    }
    split.mutate(reviewed, {
      onSuccess: () => {
        onClose();
        onDone(tf("requirement.split.done", { id: label, count: reviewed.parts.length }));
      },
      onError: (error) => {
        if (isPreviewMismatch(error)) {
          setReviewed(null);
          setStale(true);
        } else if (error.name === "PreconditionFailedError") {
          onClose();
        }
      },
    });
  };
  const diff = reviewed?.preview.diff;
  return (
    <Sheet
      open
      onOpenChange={(open) => !open && onClose()}
      title={tf("requirement.split.title", { id: label })}
      description={t("requirement.split.description")}
      onConfirm={confirm}
      confirmDisabled={reviewed === null}
      pending={split.isPending}
      footer={
        <div className="mt-1 flex justify-end gap-2">
          <Button variant="outline" size={38} onClick={onClose}>
            {t("ui.sheet.cancel")}
          </Button>
          <Button variant="outline" size={38} disabled={!ready} pending={preview.isPending} onClick={requestPreview}>
            {t(stale ? "requirement.split.previewAgain" : "requirement.split.preview")}
          </Button>
          <Button variant="primary" size={38} className="px-[18px]" disabled={reviewed === null} pending={split.isPending} onClick={confirm} shortcut={confirmShortcut()}>
            {t("requirement.split.confirm")}
          </Button>
        </div>
      }
    >
      {parts.map((part, index) => (
        <div key={index} className="flex flex-col gap-1">
          <label htmlFor={ids + "-part-" + index} className="text-caption font-semibold text-fg-2">
            {tf("requirement.split.part", { n: index + 1 })}
          </label>
          <textarea
            id={ids + "-part-" + index}
            value={part}
            rows={2}
            onChange={(event) => edit(() => setParts((current) => (index === 0 ? [event.target.value, current[1]] : [current[0], event.target.value])))}
            className={cn(FIELD, "resize-y")}
          />
        </div>
      ))}
      <div className="flex flex-col gap-1">
        <label htmlFor={ids + "-reason"} className="text-caption font-semibold text-fg-2">
          {t("requirement.split.reason")}
        </label>
        <textarea id={ids + "-reason"} value={reason} rows={2} required onChange={(event) => edit(() => setReason(event.target.value))} className={cn(FIELD, "resize-y")} />
      </div>
      {stale && (
        <div role="alert" className="rounded-field bg-attention-tint px-3.5 py-2.5 text-footnote text-attention-fg">
          <p className="m-0 font-semibold">{t("requirement.split.stale")}</p>
          <p className="m-0">{t("requirement.split.staleBody")}</p>
        </div>
      )}
      {reviewed !== null && diff !== undefined && (
        <section aria-label={t("requirement.split.result")} className="flex flex-col gap-2 rounded-tile border border-line p-3">
          <h3 className="m-0 text-caption font-semibold text-fg-2">{t("requirement.split.result")}</h3>
          <ol className="m-0 flex list-none flex-col gap-1.5 p-0">
            {reviewed.parts.map((part, index) => (
              <li key={index} className="rounded-control bg-surface px-3 py-2 text-footnote text-fg">
                {part}
              </li>
            ))}
          </ol>
          <p className="m-0 font-mono text-eyebrow text-fg-3">
            {tf("requirement.split.summary", { added: diff.added.length, modified: diff.modified.length, removed: diff.removed.length })}
          </p>
        </section>
      )}
      <WriteError error={preview.error} />
      {!stale && <WriteError error={split.error} />}
    </Sheet>
  );
}

/** Mark as duplicate of (`mark_requirement_duplicate`): another requirement supersedes this one. */
export function DuplicateSheet({ requirement, label, candidates, onClose, onDone }: SheetProps & { candidates: readonly RequirementSummary[] }) {
  const ids = useId();
  const [of, setOf] = useState("");
  const [reason, setReason] = useState("");
  const mark = useVersionedMutation<WriteResult, { of: string; reason: string }>(
    (body, ifMatch) =>
      apiClient.POST("/api/requirements/{id}/mark-duplicate", { params: { path: { id: requirement.id }, header: { "If-Match": ifMatch } }, body }),
    requirement.etag,
  );
  const confirm = () =>
    mark.mutate(
      { of, reason: reason.trim() },
      {
        onSuccess: () => {
          onClose();
          onDone(tf("requirement.duplicate.done", { id: label }));
        },
        onError: (error) => {
          if (error.name === "PreconditionFailedError") {
            onClose();
          }
        },
      },
    );
  const others = candidates.filter((candidate) => candidate.id !== requirement.id && candidate.status === "Accepted");
  return (
    <Sheet
      open
      onOpenChange={(open) => !open && onClose()}
      title={tf("requirement.duplicate.title", { id: label })}
      description={t("requirement.duplicate.description")}
      confirmLabel={t("requirement.duplicate.confirm")}
      onConfirm={confirm}
      confirmDisabled={of === "" || reason.trim() === ""}
      pending={mark.isPending}
    >
      <div className="flex flex-col gap-1">
        <label htmlFor={ids + "-of"} className="text-caption font-semibold text-fg-2">
          {t("requirement.duplicate.of")}
        </label>
        <select id={ids + "-of"} value={of} onChange={(event) => setOf(event.target.value)} className={cn(FIELD, "h-10 py-0")}>
          <option value="">{t("requirement.duplicate.choose")}</option>
          {others.map((candidate) => (
            <option key={candidate.id} value={candidate.id}>
              {requirementLabel(candidate) + " · " + candidate.text}
            </option>
          ))}
        </select>
      </div>
      <div className="flex flex-col gap-1">
        <label htmlFor={ids + "-reason"} className="text-caption font-semibold text-fg-2">
          {t("requirement.duplicate.reason")}
        </label>
        <textarea id={ids + "-reason"} value={reason} rows={3} required onChange={(event) => setReason(event.target.value)} className={cn(FIELD, "min-h-24 resize-y")} />
      </div>
      <WriteError error={mark.error} />
    </Sheet>
  );
}
