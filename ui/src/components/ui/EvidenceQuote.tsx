import { ArrowUpRight } from "lucide-react";

import { t, tf } from "../../i18n";
import { cn } from "./cn";
import { MonoId } from "./primitives";

/** The footer separator glyph (not copy). */
const SEPARATOR = "·";

export interface EvidenceQuoteProps {
  /** The passage text. */
  readonly text: string;
  /** The exact evidence range within `text` ([start, end) in UTF-16 code units), highlighted. */
  readonly highlight?: { readonly start: number; readonly end: number };
  /** The requirement or passage ID shown in the footer ("HR-008"). */
  readonly sourceId?: string;
  /** The source document name ("requirements.md"). */
  readonly sourceName?: string;
  /** "Open in source" as an action… */
  readonly onOpenSource?: () => void;
  /** …or as a link. */
  readonly openHref?: string;
  readonly className?: string;
}

/** A source passage with the evidence range highlighted, its source, and "Open in source". */
export function EvidenceQuote({ text, highlight, sourceId, sourceName, onOpenSource, openHref, className }: EvidenceQuoteProps) {
  let body = <>{text}</>;
  if (highlight !== undefined) {
    const start = Math.max(0, Math.min(text.length, highlight.start));
    const end = Math.max(start, Math.min(text.length, highlight.end));
    body = (
      <>
        {text.slice(0, start)}
        <mark className="rounded-[3px] bg-accent-wash px-0.5 text-fg">{text.slice(start, end)}</mark>
        {text.slice(end)}
      </>
    );
  }
  const openLabel = t("ui.evidence.open");
  const openClass = "ml-auto inline-flex cursor-pointer items-center gap-1 text-caption font-semibold text-fg hover:text-accent-hover";
  return (
    <figure
      aria-label={sourceName === undefined ? undefined : tf("ui.evidence.label", { source: sourceName })}
      className={cn("m-0 flex flex-col gap-2 rounded-field border border-line bg-card px-3.5 py-3", className)}
    >
      <blockquote className="m-0 select-text text-footnote leading-relaxed text-fg-2">{body}</blockquote>
      {(sourceId !== undefined || sourceName !== undefined || onOpenSource !== undefined || openHref !== undefined) && (
        <figcaption className="flex items-center gap-2 text-caption text-fg-3">
          {sourceId !== undefined && <MonoId>{sourceId}</MonoId>}
          {sourceId !== undefined && sourceName !== undefined && <span aria-hidden="true">{SEPARATOR}</span>}
          {sourceName !== undefined && <span>{sourceName}</span>}
          {onOpenSource !== undefined && (
            <button type="button" onClick={onOpenSource} className={openClass}>
              {openLabel}
              <ArrowUpRight size={14} strokeWidth={1.6} aria-hidden="true" />
            </button>
          )}
          {onOpenSource === undefined && openHref !== undefined && (
            <a href={openHref} className={openClass}>
              {openLabel}
              <ArrowUpRight size={14} strokeWidth={1.6} aria-hidden="true" />
            </a>
          )}
        </figcaption>
      )}
    </figure>
  );
}
