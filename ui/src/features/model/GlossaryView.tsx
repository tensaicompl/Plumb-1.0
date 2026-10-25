import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { X } from "lucide-react";
import { useId, useState, type MouseEvent } from "react";

import { apiClient, readReviewed, revisionEtag } from "../../api/client";
import type { components } from "../../api/schema";
import { useVersionedMutation, WriteFailedError } from "../../api/useVersionedMutation";
import { buildHref, useGo } from "../../app/shell/navigation";
import { Button, cn, DiffLines, EmptyState, Eyebrow, IconButton, isPlainClick, MonoId, Sheet, Skeleton, SkeletonGroup, StatusPill, toast, WithInspector, type DiffLine } from "../../components/ui";
import { t, tf } from "../../i18n";
import { useLabels } from "./labels";
import { ReadError, SegmentBody, useCanEdit, WriteError, writeCode } from "./shared";

type GlossaryTerm = components["schemas"]["GlossaryTerm"];
type WriteResult = components["schemas"]["WriteResult"];
type Preview = components["schemas"]["SemanticChangePreviewResponse"];

export const glossaryQuery = {
  queryKey: ["model", "glossary"] as const,
  queryFn: () => readReviewed(apiClient.GET("/api/glossary")),
};

const COLS = "grid-cols-[200px_minmax(0,1fr)_220px_110px]";

function TermStatus({ status }: { status: string }) {
  if (status === "Accepted") {
    return <StatusPill status="confirmed" />;
  }
  if (status === "Proposed") {
    return <StatusPill status="proposed" />;
  }
  return <StatusPill status="proposed" label={status} />;
}

function Aliases({ aliases }: { aliases: readonly string[] }) {
  return (
    <ul className="m-0 flex list-none flex-wrap gap-1 p-0">
      {aliases.map((alias) => (
        <li key={alias} className="inline-flex h-[22px] items-center rounded-pill bg-surface px-2 text-eyebrow font-medium text-fg">
          {alias}
        </li>
      ))}
    </ul>
  );
}

/** Model · Glossary (spec §5.4.1): the term table and, at /model/glossary/$id, the term inspector. */
export function GlossaryView() {
  const params: { id?: string } = useParams({ strict: false });
  const selectedId = params.id;
  const go = useGo();
  const glossary = useQuery(glossaryQuery);
  const terms = glossary.data?.items ?? [];
  const label = useLabels(terms.map((term) => term.definition_ref));
  const selected = terms.find((term) => term.id === selectedId) ?? null;
  const open = (event: MouseEvent<HTMLAnchorElement>, href: string) => {
    if (isPlainClick(event)) {
      event.preventDefault();
      go(href);
    }
  };

  let body;
  if (glossary.isError) {
    body = <ReadError error={glossary.error} messageKey="model.glossary.loadFailed" onRetry={() => void glossary.refetch()} />;
  } else if (glossary.isPending) {
    body = (
      <SkeletonGroup className="flex flex-col gap-4 rounded-card border border-line bg-card p-5">
        {[0, 1, 2, 3].map((row) => (
          <div key={row} className={cn("grid items-center gap-4", COLS)}>
            <Skeleton width="70%" />
            <Skeleton width="85%" />
            <Skeleton width="50%" height={18} radius="pill" />
            <Skeleton width={70} height={18} radius="pill" />
          </div>
        ))}
      </SkeletonGroup>
    );
  } else if (terms.length === 0) {
    body = <EmptyState title={t("model.glossary.empty.title")} description={t("model.glossary.empty.body")} />;
  } else {
    body = (
      <div className="overflow-hidden rounded-card border border-line bg-card">
        <table className="w-full table-fixed border-collapse text-footnote">
          <caption className="sr-only">{t("nav.glossary")}</caption>
          <colgroup>
            <col className="w-[200px]" />
            <col />
            <col className="w-[220px]" />
            <col className="w-[110px]" />
          </colgroup>
          <thead>
            <tr className="border-b border-line">
              {(["model.glossary.term", "model.glossary.definition", "model.glossary.aliases", "model.glossary.status"] as const).map((key) => (
                <th key={key} scope="col" className="px-5 py-2.5 text-left text-eyebrow font-semibold uppercase tracking-eyebrow text-fg-3 first:pl-5">
                  {t(key)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {terms.map((term) => {
              const href = buildHref("/model/glossary/$id", { id: term.id });
              const isSelected = term.id === selectedId;
              return (
                <tr key={term.id} data-term-id={term.id} className={cn("border-b border-line last:border-b-0", isSelected ? "bg-surface" : "hover:bg-subtle-row")}>
                  <td className="px-5 py-3.5 align-middle">
                    <a href={href} onClick={(event) => open(event, href)} aria-current={isSelected ? "true" : undefined} className="font-semibold text-fg no-underline hover:underline">
                      {term.term}
                    </a>
                  </td>
                  <td className="py-3.5 pr-4 align-middle leading-[1.45] text-fg-2">
                    {term.definition_ref === null || term.definition_ref === undefined ? (
                      <span className="text-fg-3">{t("model.glossary.noDefinition")}</span>
                    ) : (
                      <span className="font-mono text-eyebrow font-medium text-fg-3">{label(term.definition_ref)}</span>
                    )}
                  </td>
                  <td className="py-3.5 pr-4 align-middle">
                    <Aliases aliases={term.aliases} />
                  </td>
                  <td className="py-3.5 pr-4 align-middle">
                    <TermStatus status={term.status} />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    );
  }

  const table = <SegmentBody>{body}</SegmentBody>;
  if (selected === null || glossary.data === undefined) {
    return table;
  }
  return (
    <WithInspector
      label={t("model.glossary.details")}
      inspectorKey={selected.id}
      inspector={<TermInspector term={selected} definition={selected.definition_ref ? label(selected.definition_ref) : null} onClose={() => go("/model/glossary")} />}
      actions={<TermActions term={selected} terms={terms} etag={glossary.data.etag} />}
    >
      {table}
    </WithInspector>
  );
}

function TermInspector({ term, definition, onClose }: { term: GlossaryTerm; definition: string | null; onClose: () => void }) {
  return (
    <>
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-2.5">
          <Eyebrow>{t("model.glossary.eyebrow")}</Eyebrow>
          <h2 className="m-0 font-display text-title font-semibold leading-snug text-fg">{term.term}</h2>
          <div>
            <TermStatus status={term.status} />
          </div>
        </div>
        <IconButton label={t("model.closeDetails")} size={30} icon={<X size={16} strokeWidth={1.6} aria-hidden="true" />} onClick={onClose} />
      </div>
      <dl className="m-0 grid grid-cols-[96px_1fr] gap-x-3 gap-y-3 text-footnote">
        <dt className="text-fg-3">{t("model.glossary.definition")}</dt>
        <dd className="m-0 text-fg">{definition ?? <span className="text-fg-3">{t("model.glossary.noDefinition")}</span>}</dd>
        <dt className="text-fg-3">{t("model.glossary.aliases")}</dt>
        <dd className="m-0">{term.aliases.length === 0 ? <span className="text-fg-3">{t("model.none")}</span> : <Aliases aliases={term.aliases} />}</dd>
        <dt className="text-fg-3">{t("model.glossary.language")}</dt>
        <dd className="m-0 font-mono text-caption text-fg">{term.language}</dd>
        <dt className="text-fg-3">{t("model.id")}</dt>
        <dd className="m-0">
          <MonoId>{term.id}</MonoId>
        </dd>
      </dl>
    </>
  );
}

function TermActions({ term, terms, etag }: { term: GlossaryTerm; terms: readonly GlossaryTerm[]; etag: string | null }) {
  const canEdit = useCanEdit();
  const queryClient = useQueryClient();
  const [sheet, setSheet] = useState<"edit" | "merge" | null>(null);
  const confirm = useVersionedMutation<WriteResult>(
    (_, ifMatch) => apiClient.POST("/api/glossary/{id}/confirm", { params: { path: { id: term.id }, header: { "If-Match": ifMatch } } }),
    etag,
  );
  if (!canEdit) {
    return undefined;
  }
  return (
    <>
      {term.status === "Proposed" && (
        <Button
          variant="primary"
          size={38}
          pending={confirm.isPending}
          onClick={() =>
            confirm.mutate(undefined, {
              onSuccess: () => {
                void queryClient.invalidateQueries({ queryKey: ["model"] });
                toast({ message: t("model.glossary.confirmed") });
              },
            })
          }
        >
          {t("model.confirm")}
        </Button>
      )}
      <Button variant="outline" size={38} onClick={() => setSheet("edit")}>
        {t("model.glossary.edit.open")}
      </Button>
      <Button variant="outline" size={38} onClick={() => setSheet("merge")}>
        {t("model.glossary.merge.open")}
      </Button>
      <WriteError error={confirm.error} className="w-full" />
      <EditAliasesSheet key={"edit" + term.id} term={term} etag={etag} open={sheet === "edit"} onOpenChange={(open) => setSheet(open ? "edit" : null)} />
      <MergeSheet key={"merge" + term.id} term={term} terms={terms} etag={etag} open={sheet === "merge"} onOpenChange={(open) => setSheet(open ? "merge" : null)} />
    </>
  );
}

/** Comma-separated aliases: trimmed, blanks and duplicates dropped. */
export function parseAliases(text: string): string[] {
  return [...new Set(text.split(",").map((alias) => alias.trim()).filter((alias) => alias !== ""))];
}

function EditAliasesSheet({ term, etag, open, onOpenChange }: { term: GlossaryTerm; etag: string | null; open: boolean; onOpenChange: (open: boolean) => void }) {
  const queryClient = useQueryClient();
  const [aliases, setAliases] = useState(term.aliases.join(", "));
  const termId = useId();
  const aliasesId = useId();
  const edit = useVersionedMutation<WriteResult, string[]>(
    (next, ifMatch) => apiClient.POST("/api/glossary/{id}/edit", { params: { path: { id: term.id }, header: { "If-Match": ifMatch } }, body: { term: term.term, aliases: next } }),
    etag,
  );
  const field = "h-[38px] w-full rounded-field border border-line bg-card px-3 text-callout text-fg outline-none focus:border-accent focus:shadow-focus";
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t("model.glossary.edit.title")}
      description={t("model.glossary.edit.description")}
      confirmLabel={t("model.save")}
      pending={edit.isPending}
      onConfirm={() =>
        edit.mutate(parseAliases(aliases), {
          onSuccess: () => {
            void queryClient.invalidateQueries({ queryKey: ["model"] });
            onOpenChange(false);
            toast({ message: t("model.glossary.edited") });
          },
        })
      }
    >
      <div className="flex flex-col gap-1.5">
        <label htmlFor={termId} className="text-caption font-semibold text-fg-2">
          {t("model.glossary.term")}
        </label>
        <input id={termId} readOnly value={term.term} aria-describedby={termId + "-hint"} className={cn(field, "bg-surface text-fg-2")} />
        <p id={termId + "-hint"} className="m-0 text-caption text-fg-3">
          {t("model.glossary.edit.termFixed")}
        </p>
      </div>
      <div className="flex flex-col gap-1.5">
        <label htmlFor={aliasesId} className="text-caption font-semibold text-fg-2">
          {t("model.glossary.aliases")}
        </label>
        <input id={aliasesId} value={aliases} onChange={(event) => setAliases(event.target.value)} aria-describedby={aliasesId + "-hint"} className={field} />
        <p id={aliasesId + "-hint"} className="m-0 text-caption text-fg-3">
          {t("model.glossary.edit.aliasesHint")}
        </p>
      </div>
      <WriteError error={edit.error} />
    </Sheet>
  );
}

function MergeSheet({ term, terms, etag, open, onOpenChange }: { term: GlossaryTerm; terms: readonly GlossaryTerm[]; etag: string | null; open: boolean; onOpenChange: (open: boolean) => void }) {
  const queryClient = useQueryClient();
  const [target, setTarget] = useState("");
  const [reason, setReason] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [stale, setStale] = useState(false);
  const targetId = useId();
  const reasonId = useId();
  // The backend merges only into an Accepted term (Fable #356).
  const targets = terms.filter((other) => other.id !== term.id && other.status === "Accepted").sort((a, b) => a.term.localeCompare(b.term));
  const requestPreview = useVersionedMutation<Preview>(
    (_, ifMatch) =>
      apiClient.POST("/api/glossary/{id}/merge/preview", { params: { path: { id: term.id }, header: { "If-Match": ifMatch } }, body: { into: target, reason: reason.trim() } }),
    etag,
  );
  // The confirmation is based on the reviewed preview, which states its baseline revision.
  const merge = useVersionedMutation<WriteResult, Preview>(
    (reviewed, ifMatch) =>
      apiClient.POST("/api/glossary/{id}/merge", {
        params: { path: { id: term.id }, header: { "If-Match": ifMatch } },
        body: { into: target, reason: reason.trim(), preview: { proposal_id: reviewed.proposal_id, built_at: reviewed.built_at } },
      }),
    (reviewed) => revisionEtag(reviewed.revision),
  );
  const diffIds = preview === null ? [] : [...preview.diff.added, ...preview.diff.removed, ...preview.diff.modified];
  const label = useLabels(diffIds);
  const lines: DiffLine[] =
    preview === null
      ? []
      : [
          ...preview.diff.added.map((id) => ({ op: "add" as const, text: label(id) })),
          ...preview.diff.removed.map((id) => ({ op: "remove" as const, text: label(id) })),
          ...preview.diff.modified.map((id) => ({ op: "context" as const, text: tf("model.glossary.merge.changed", { name: label(id) }) })),
        ];
  const field = "w-full rounded-field border border-line bg-card px-3 text-callout text-fg outline-none focus:border-accent focus:shadow-focus disabled:bg-surface";
  const submit = () => {
    if (preview === null) {
      setStale(false);
      requestPreview.mutate(undefined, { onSuccess: (data) => setPreview(data) });
      return;
    }
    merge.mutate(preview, {
      onSuccess: () => {
        void queryClient.invalidateQueries({ queryKey: ["model"] });
        onOpenChange(false);
        toast({ message: t("model.glossary.merged") });
      },
      onError: (error) => {
        if (error instanceof WriteFailedError && writeCode(error) === "E_PREVIEW_MISMATCH") {
          setPreview(null);
          setStale(true);
        }
      },
    });
  };
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={tf("model.glossary.merge.title", { term: term.term })}
      description={t("model.glossary.merge.description")}
      confirmLabel={preview === null ? t("model.glossary.merge.preview") : t("model.glossary.merge.confirm")}
      confirmDisabled={target === "" || reason.trim() === ""}
      pending={requestPreview.isPending || merge.isPending}
      onConfirm={submit}
    >
      <div className="flex flex-col gap-1.5">
        <label htmlFor={targetId} className="text-caption font-semibold text-fg-2">
          {t("model.glossary.merge.into")}
        </label>
        <select id={targetId} value={target} disabled={preview !== null} onChange={(event) => setTarget(event.target.value)} className={cn(field, "h-[38px]")}>
          <option value="">{t("model.glossary.merge.choose")}</option>
          {targets.map((other) => (
            <option key={other.id} value={other.id}>
              {other.term}
            </option>
          ))}
        </select>
      </div>
      <div className="flex flex-col gap-1.5">
        <label htmlFor={reasonId} className="text-caption font-semibold text-fg-2">
          {t("ui.sheet.reasonRequired")}
        </label>
        <textarea id={reasonId} value={reason} required aria-required="true" disabled={preview !== null} onChange={(event) => setReason(event.target.value)} className={cn(field, "min-h-24 resize-y py-3")} />
      </div>
      {preview !== null && (
        <div className="flex flex-col gap-2">
          <DiffLines variant="boxed" title={t("model.glossary.merge.diff")} lines={lines} />
          <Button variant="ghost" size={30} className="self-start" onClick={() => setPreview(null)}>
            {t("model.glossary.merge.back")}
          </Button>
        </div>
      )}
      {stale && (
        <p role="alert" className="m-0 text-footnote text-blocked-fg">
          {t("model.glossary.merge.stale")}
        </p>
      )}
      <WriteError error={requestPreview.error} />
      {!stale && <WriteError error={merge.error} />}
    </Sheet>
  );
}
