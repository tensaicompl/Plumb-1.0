import { useQuery } from "@tanstack/react-query";
import { FileText } from "lucide-react";
import type { MouseEvent, ReactNode } from "react";

import type { components } from "../../api/schema";
import { buildHref, useGo } from "../../app/shell/navigation";
import { Button, cn, EmptyState, isPlainClick, Skeleton, SkeletonGroup, Tooltip } from "../../components/ui";
import { t, tc, tf, useLocaleStore, type Locale, type MessageKey } from "../../i18n";
import { isYesterday, parseTimestamp, sameDay, shortDate } from "../overview/time";
import { ImportDropZone } from "./ImportDropZone";
import { sourceSpansQuery, sourcesQuery } from "./queries";

type SourceListItem = components["schemas"]["SourceListItem"];
type Span = components["schemas"]["Span"];

/** The plain kind of a document from its media type. */
const KIND: Readonly<Record<string, MessageKey>> = {
  "text/markdown": "sources.kind.markdown",
  "text/plain": "sources.kind.text",
  "application/vnd.openxmlformats-officedocument.wordprocessingml.document": "sources.kind.word",
};

/** The separator between facts on one line (punctuation, not copy). */
const SEPARATOR = " · ";

function importedLabel(value: string, locale: Locale): string {
  const date = parseTimestamp(value);
  const now = new Date();
  if (sameDay(date, now)) {
    return t("sources.imported.today");
  }
  if (isYesterday(date, now)) {
    return t("sources.imported.yesterday");
  }
  return tf("sources.imported.on", { date: shortDate(date, locale) });
}

/** A passage as read: markdown heading marks and emphasis removed; headings are rendered as such. */
export function readablePassage(text: string): { heading: boolean; text: string } {
  const heading = /^#{1,6}\s+/.test(text);
  const plain = text
    .replace(/^#{1,6}\s+/, "")
    .replace(/(\*\*|__)(.+?)\1/g, "$2")
    .replace(/`([^`]+)`/g, "$1");
  return { heading, text: plain };
}

/** How a requirement a passage became is named: its source identifier, else its ID. */
function requirementName(requirement: Span["requirements"][number]): string {
  return requirement.source_identifier ?? requirement.id;
}

function AppLink({ href, children, className, current }: { href: string; children: ReactNode; className?: string; current?: boolean }) {
  const go = useGo();
  return (
    <a
      href={href}
      aria-current={current === true ? "page" : undefined}
      className={className}
      onClick={(event: MouseEvent<HTMLAnchorElement>) => {
        if (isPlainClick(event)) {
          event.preventDefault();
          go(href);
        }
      }}
    >
      {children}
    </a>
  );
}

// ------------------------------------------------------------------ list column

function SourceRow({ source, selected }: { source: SourceListItem; selected: boolean }) {
  const locale = useLocaleStore((state) => state.locale);
  const kind = KIND[source.media_type];
  const facts = [kind === undefined ? source.media_type : t(kind), tc("sources.passages", source.spans_count), importedLabel(source.imported_at, locale)];
  return (
    <li>
      <AppLink
        href={buildHref("/sources/$id", { id: source.id })}
        current={selected}
        className={cn(
          "flex items-center gap-2.5 rounded-field px-3 py-2.5 no-underline transition-colors duration-[120ms]",
          selected ? "bg-surface" : "hover:bg-surface",
        )}
      >
        <span aria-hidden="true" className="flex size-8 shrink-0 items-center justify-center rounded-control border border-line bg-card text-fg-2">
          <FileText size={16} strokeWidth={1.6} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate font-mono text-footnote font-medium leading-[1.3] text-fg">{source.name}</span>
          <span className={cn("block text-eyebrow leading-snug", selected ? "text-fg-3-surface" : "text-fg-3")}>{facts.join(SEPARATOR)}</span>
        </span>
      </AppLink>
    </li>
  );
}

// ------------------------------------------------------------------ reading view

function Passage({ span }: { span: Span }) {
  const go = useGo();
  const { heading, text } = readablePassage(span.text ?? "");
  if (heading) {
    return (
      <h2 data-span={span.id} className="m-0 mb-4 font-display text-headline font-semibold tracking-[-0.01em] text-fg">
        {text}
      </h2>
    );
  }
  const became = span.requirements;
  const first = became[0];
  if (first === undefined) {
    return (
      <p data-span={span.id} className="m-0 mb-3.5">
        {text}
      </p>
    );
  }
  const names = became.map(requirementName).join(", ");
  const href = buildHref("/requirements/$id", { id: first.id });
  return (
    <p data-span={span.id} className="m-0 mb-3.5">
      <Tooltip content={tf("sources.reading.became", { id: names })}>
        <a
          href={href}
          onClick={(event: MouseEvent<HTMLAnchorElement>) => {
            if (isPlainClick(event)) {
              event.preventDefault();
              go(href);
            }
          }}
          className="cursor-pointer rounded-[3px] bg-accent-wash py-px text-fg no-underline transition-colors duration-[120ms] [box-decoration-break:clone] hover:bg-accent-wash-hover focus-visible:outline-2 focus-visible:outline-accent"
        >
          {text}
        </a>
      </Tooltip>{" "}
      <sup className="font-mono text-[10px] font-medium leading-none text-fg-3">{names}</sup>
    </p>
  );
}

function ReadingView({ source }: { source: SourceListItem }) {
  const spans = useQuery(sourceSpansQuery(source.id));
  const items = spans.data ?? [];
  const became = items.filter((s) => s.requirements.length > 0).length;
  return (
    <article aria-label={source.name} className="mx-auto max-w-[720px] px-8 pb-20 pt-10 text-[15px] leading-[1.75] text-fg-2">
      {spans.isError ? (
        <div role="alert" className="flex items-center gap-3 text-footnote text-blocked-fg">
          <span>{t("sources.reading.failed")}</span>
          <Button variant="outline" size={30} onClick={() => void spans.refetch()}>
            {t("sources.retry")}
          </Button>
        </div>
      ) : spans.data === undefined ? (
        <SkeletonGroup className="flex flex-col gap-3">
          <Skeleton width="40%" height={12} />
          <Skeleton width="55%" height={24} />
          {[0, 1, 2, 3, 4].map((row) => (
            <Skeleton key={row} width={row % 2 === 0 ? "95%" : "80%"} height={14} />
          ))}
        </SkeletonGroup>
      ) : (
        <>
          <p className="m-0 mb-5 font-mono text-caption font-medium leading-none text-fg-3">
            {[source.name, tc("sources.passages", items.length), tf("sources.reading.became.count", { count: became })].join(SEPARATOR)}
          </p>
          {items
            .filter((s) => (s.text ?? "") !== "")
            .map((s) => (
              <Passage key={s.id} span={s} />
            ))}
        </>
      )}
    </article>
  );
}

// ------------------------------------------------------------------ screen

/** /sources and /sources/$id: import documents and read them with the evidence overlay. */
export function SourcesScreen({ id }: { id?: string }) {
  const sources = useQuery(sourcesQuery);
  const list = sources.data;
  const selected = list === undefined ? undefined : id === undefined ? list[0] : list.find((s) => s.id === id);
  return (
    <div className="flex min-h-0 flex-1">
      <div className="flex w-[300px] shrink-0 flex-col gap-3 overflow-y-auto border-r border-line bg-card px-3 py-5">
        <h1 className="m-0 px-2 font-display text-title font-semibold leading-none text-fg">{t("sources.title")}</h1>
        <ImportDropZone variant="compact" />
        {sources.isError ? (
          <div role="alert" className="flex flex-col items-start gap-2 px-2 text-footnote text-blocked-fg">
            <span>{t("sources.list.failed")}</span>
            <Button variant="outline" size={30} onClick={() => void sources.refetch()}>
              {t("sources.retry")}
            </Button>
          </div>
        ) : list === undefined ? (
          <SkeletonGroup className="flex flex-col gap-2 px-3">
            <Skeleton width="70%" height={14} />
            <Skeleton width="90%" height={10} />
          </SkeletonGroup>
        ) : (
          <ul aria-label={t("sources.list")} className="m-0 flex list-none flex-col gap-1 p-0">
            {list.map((source) => (
              <SourceRow key={source.id} source={source} selected={source.id === selected?.id} />
            ))}
          </ul>
        )}
      </div>
      <div className="min-w-0 flex-1 overflow-y-auto bg-card">
        {list !== undefined && list.length === 0 ? (
          <EmptyState title={t("sources.empty.title")} description={t("sources.empty.body")} className="pt-24" />
        ) : list !== undefined && selected === undefined ? (
          <EmptyState title={t("sources.missing.title")} description={t("sources.missing.body")} className="pt-24" />
        ) : selected === undefined ? null : (
          <ReadingView key={selected.id} source={selected} />
        )}
      </div>
    </div>
  );
}
