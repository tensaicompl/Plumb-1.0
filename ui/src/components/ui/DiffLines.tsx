import { t } from "../../i18n";
import { cn } from "./cn";

/** One line of a model or text diff. */
export interface DiffLine {
  readonly op: "add" | "remove" | "context";
  readonly text: string;
}

export interface DiffLinesProps {
  readonly lines: readonly DiffLine[];
  /** `plain`: coloured + / − lines (proposal preview); `boxed`: a titled box with tinted rows (answer to confirm). */
  readonly variant?: "plain" | "boxed";
  readonly title?: string;
  readonly className?: string;
}

const SIGN = { add: "+", remove: "−", context: " " } as const;

/** Added and removed lines in mono 12 px; each change is announced, never colour-only. */
export function DiffLines({ lines, variant = "plain", title, className }: DiffLinesProps) {
  const rows = lines.map((line, index) => (
    <li
      key={index}
      data-diff-op={line.op}
      className={cn(
        "flex gap-2 font-mono text-caption leading-relaxed",
        variant === "boxed" && "px-3.5 py-1.5",
        line.op === "add" && "text-diff-add",
        line.op === "remove" && "text-diff-del",
        line.op === "context" && "text-fg-2",
        variant === "boxed" && line.op === "add" && "bg-diff-add-bg",
        variant === "boxed" && line.op === "remove" && "bg-diff-del-bg",
      )}
    >
      <span aria-hidden="true" className="w-3 shrink-0 select-none">
        {SIGN[line.op]}
      </span>
      {line.op !== "context" && <span className="sr-only">{t(line.op === "add" ? "ui.diff.added" : "ui.diff.removed")}</span>}
      <span className="min-w-0 select-text break-words">{line.text}</span>
    </li>
  ));
  if (variant === "plain") {
    return (
      <div className={className}>
        {title !== undefined && <p className="m-0 mb-1 text-caption font-semibold text-fg-3">{title}</p>}
        <ul aria-label={title ?? t("ui.diff.label")} className="m-0 flex list-none flex-col p-0">
          {rows}
        </ul>
      </div>
    );
  }
  return (
    <div className={cn("overflow-hidden rounded-field border border-line bg-card", className)}>
      <p className="m-0 border-b border-line bg-surface px-3.5 py-2 text-eyebrow font-semibold text-fg-3-surface">{title ?? t("ui.diff.label")}</p>
      <ul aria-label={title ?? t("ui.diff.label")} className="m-0 flex list-none flex-col p-0">
        {rows}
      </ul>
    </div>
  );
}
