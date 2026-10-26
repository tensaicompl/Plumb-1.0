import { Lock } from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import type { components } from "../../api/schema";
import { cn, SegmentedControl } from "../../components/ui";
import { t } from "../../i18n";
import { parseAnswerSchema, withConsts, type AnswerField, type AnswerValue } from "./answerSchema";

type Question = components["schemas"]["Question"];

/** Kinds whose free text is long (a multi-line field). */
const MULTILINE: ReadonlySet<Question["kind"]> = new Set(["Text", "FormulaConfirm", "RuleCell", "ArchitectureChoice", "TechnologyChoice", "InterfaceChoice"]);

/** A model element ID (`operation:30e3…`): its choices are picked as references. */
const REFERENCE = /^[a-z][a-z_]*:[A-Za-z0-9._-]+$/;

/** More choices than this get a filter field. */
const FILTER_FROM = 6;

const INPUT =
  "h-9 w-full rounded-control border border-line bg-card px-3 text-footnote text-fg outline-none placeholder:text-fg-4 focus:border-accent focus:shadow-focus focus-visible:outline-none";

function fieldLabel(name: string): string {
  return name.replace(/_/g, " ");
}

/** "Operation" for `operation:30e3…`. */
function referenceKind(id: string): string {
  const kind = fieldLabel(id.slice(0, id.indexOf(":")));
  return kind.charAt(0).toUpperCase() + kind.slice(1);
}

function FieldLabel({ id, children }: { id: string; children: ReactNode }) {
  return (
    <p id={id} className="m-0 mb-1.5 text-caption font-medium text-fg-3">
      {children}
    </p>
  );
}

/** The plain-language name of a referenced element, or its ID when it has none. */
export type ReferenceLabels = (id: string) => string;

/** The named part of a reference: its name, else its kind ("Operation"). */
function referenceName(id: string, labels: ReferenceLabels | undefined): string | null {
  const name = labels?.(id) ?? "";
  return name === "" || name === id ? null : name;
}

/**
 * A vertical radio list (radio tiles with roving focus); a reference shows its plain-language
 * name first, then its kind and mono ID (its kind first when it has no name).
 */
function ChoiceList({
  choices,
  value,
  onChange,
  labelledBy,
  labels,
}: {
  choices: readonly string[];
  value: unknown;
  onChange: (next: string) => void;
  labelledBy: string;
  labels: ReferenceLabels | undefined;
}) {
  const [query, setQuery] = useState("");
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const references = choices.every((c) => REFERENCE.test(c));
  const needle = query.trim().toLowerCase();
  const visible = needle === "" ? choices : choices.filter((c) => c.toLowerCase().includes(needle) || (references && (referenceName(c, labels) ?? "").toLowerCase().includes(needle)));
  const checkedIndex = visible.findIndex((c) => c === value);
  const move = (event: KeyboardEvent<HTMLButtonElement>, from: number) => {
    const delta = event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : event.key === "ArrowUp" || event.key === "ArrowLeft" ? -1 : 0;
    if (delta === 0) {
      return;
    }
    event.preventDefault();
    const target = (from + delta + visible.length) % visible.length;
    const choice = visible[target];
    if (choice !== undefined) {
      onChange(choice);
      refs.current[target]?.focus();
    }
  };
  return (
    <div className="flex flex-col gap-1.5">
      {choices.length > FILTER_FROM && (
        <input type="search" aria-label={t("inbox.answer.filter")} placeholder={t("inbox.answer.filter")} value={query} onChange={(e) => setQuery(e.target.value)} className={INPUT} />
      )}
      <div role="radiogroup" aria-labelledby={labelledBy} className="flex flex-col gap-1.5">
        {visible.map((choice, index) => {
          const checked = choice === value;
          return (
            <button
              key={choice}
              ref={(element) => {
                refs.current[index] = element;
              }}
              type="button"
              role="radio"
              aria-checked={checked}
              tabIndex={checked || (checkedIndex < 0 && index === 0) ? 0 : -1}
              onClick={() => onChange(choice)}
              onKeyDown={(event) => move(event, index)}
              className={cn(
                "flex min-h-9 w-full cursor-pointer items-center gap-2 rounded-control border px-3.5 py-2 text-left text-footnote font-semibold leading-snug text-fg transition-colors duration-[120ms] ease-standard",
                checked ? "border-primary bg-surface" : "border-line bg-card hover:bg-subtle-row",
              )}
            >
              {references ? (
                <>
                  <span>{referenceName(choice, labels) ?? referenceKind(choice)}</span>{" "}
                  {referenceName(choice, labels) !== null && (
                    <>
                      <span className="text-caption font-normal text-fg-3">{referenceKind(choice)}</span>{" "}
                    </>
                  )}
                  <span className="select-text font-mono text-caption font-normal text-fg-3">{choice}</span>
                </>
              ) : (
                choice
              )}
            </button>
          );
        })}
        {visible.length === 0 && <p className="m-0 text-caption text-fg-3">{t("inbox.answer.noMatch")}</p>}
      </div>
    </div>
  );
}

function CheckList({ choices, value, onChange, labelledBy }: { choices: readonly string[]; value: unknown; onChange: (next: string[]) => void; labelledBy: string }) {
  const selected = Array.isArray(value) ? (value as string[]) : [];
  return (
    <div role="group" aria-labelledby={labelledBy} className="flex flex-col gap-1.5">
      {choices.map((choice) => {
        const checked = selected.includes(choice);
        return (
          <label
            key={choice}
            className={cn(
              "flex min-h-9 cursor-pointer items-center gap-2.5 rounded-control border px-3.5 py-2 text-footnote font-semibold text-fg",
              checked ? "border-primary bg-surface" : "border-line bg-card hover:bg-subtle-row",
            )}
          >
            <input
              type="checkbox"
              className="size-4 accent-primary"
              checked={checked}
              onChange={(event) => onChange(event.target.checked ? choices.filter((c) => c === choice || selected.includes(c)) : selected.filter((c) => c !== choice))}
            />
            {choice}
          </label>
        );
      })}
    </div>
  );
}

function YesNo({ value, onChange, labelledBy }: { value: unknown; onChange: (next: boolean) => void; labelledBy: string }) {
  return (
    <SegmentedControl
      labelledBy={labelledBy}
      fill
      value={value === true ? "yes" : value === false ? "no" : null}
      onChange={(next) => onChange(next === "yes")}
      options={[
        { value: "yes", label: t("answer.yes") },
        { value: "no", label: t("answer.no") },
      ]}
    />
  );
}

function FieldControl({
  field,
  multiline,
  value,
  onChange,
  labelledBy,
  labels,
}: {
  field: AnswerField;
  multiline: boolean;
  value: unknown;
  onChange: (next: unknown) => void;
  labelledBy: string;
  labels: ReferenceLabels | undefined;
}) {
  switch (field.type) {
    case "boolean":
      return <YesNo value={value} onChange={onChange} labelledBy={labelledBy} />;
    case "enum":
      return <ChoiceList choices={field.choices} value={value} onChange={onChange} labelledBy={labelledBy} labels={labels} />;
    case "enum-set":
      return <CheckList choices={field.choices} value={value} onChange={onChange} labelledBy={labelledBy} />;
    case "string":
      return multiline ? (
        <textarea
          aria-labelledby={labelledBy}
          value={typeof value === "string" ? value : ""}
          onChange={(event) => onChange(event.target.value === "" ? undefined : event.target.value)}
          className={cn(INPUT, "h-auto min-h-[72px] resize-y py-2.5 leading-normal")}
        />
      ) : (
        <input
          type="text"
          aria-labelledby={labelledBy}
          value={typeof value === "string" ? value : ""}
          onChange={(event) => onChange(event.target.value === "" ? undefined : event.target.value)}
          className={INPUT}
        />
      );
    case "number":
      return (
        <input
          type="number"
          aria-labelledby={labelledBy}
          step={field.integer ? 1 : "any"}
          min={field.minimum ?? undefined}
          max={field.maximum ?? undefined}
          value={typeof value === "number" ? value : ""}
          onChange={(event) => onChange(event.target.value === "" ? undefined : Number(event.target.value))}
          className={cn(INPUT, "font-mono")}
        />
      );
    case "const":
      return (
        <div data-testid={"fixed-" + field.name} className="flex h-9 items-center gap-2 rounded-control bg-surface px-3 text-footnote font-semibold text-fg">
          <Lock size={14} strokeWidth={1.6} aria-hidden="true" className="text-fg-3-surface" />
          <span>{typeof field.value === "boolean" ? t(field.value ? "answer.yes" : "answer.no") : String(field.value)}</span>
          <span className="ml-auto text-caption font-normal text-fg-3-surface">{t("inbox.answer.fixed")}</span>
        </div>
      );
  }
}

/**
 * The typed answer of one Question: fields only from its closed `answer_schema` (names, required
 * set, choices and bounds). One boolean is a Yes/No segmented control, choices a radio list (a
 * filterable reference picker for element IDs), several choices a pick-many list, a number and a
 * unit sit side by side, and a fixed (`const`) value is shown read-only and always answered. A
 * schema that cannot be rendered is explained and never produces an answer. Controlled: `onChange`
 * receives the exact answer object.
 */
export function AnswerControl({
  question,
  value,
  onChange,
  labelledBy,
  labels,
}: {
  question: Pick<Question, "id" | "kind" | "answer_schema">;
  value: AnswerValue;
  onChange: (next: AnswerValue) => void;
  /** The ID of the "Answer" heading that names a single-field control. */
  labelledBy: string;
  /** The names of the elements its choices reference (`useLabels`). */
  labels?: ReferenceLabels;
}) {
  const baseId = useId();
  const fields = parseAnswerSchema(question.answer_schema);
  const missingConst = fields?.some((field) => field.type === "const" && value[field.name] !== field.value) ?? false;
  useEffect(() => {
    if (fields !== null && missingConst) {
      onChange(withConsts(fields, value));
    }
  });
  if (fields === null) {
    return (
      <p role="alert" className="m-0 rounded-field bg-blocked-tint px-3.5 py-3 text-footnote text-blocked-fg">
        {t("inbox.answer.unsupported")}
      </p>
    );
  }
  const setField = (name: string, next: unknown) => onChange(withConsts(fields, { ...value, [name]: next }));
  const editable = fields.filter((f) => f.type !== "const");
  const multiline = MULTILINE.has(question.kind);
  // A single choice (Yes/No, a list) is named by the card's "Answer" heading; inputs and the fields
  // of a multi-field answer carry their own label.
  const single = editable.length === 1;
  const control = (field: AnswerField, index: number) => {
    const labelId = `${baseId}-${index}`;
    const named = single && (field.type === "boolean" || field.type === "enum" || field.type === "enum-set");
    return (
      <div key={field.name} data-field={field.name}>
        {!named && <FieldLabel id={labelId}>{fieldLabel(field.name)}</FieldLabel>}
        <FieldControl field={field} multiline={multiline} value={value[field.name]} onChange={(next) => setField(field.name, next)} labelledBy={named ? labelledBy : labelId} labels={labels} />
      </div>
    );
  };
  const unit = fields.find((f) => f.name === "unit" && f.type === "string");
  const amount = fields.find((f) => f !== unit && (f.type === "number" || f.type === "string"));
  if (unit !== undefined && amount !== undefined && fields.length === 2) {
    return (
      <div data-testid="number-unit" className="grid grid-cols-[1fr_120px] gap-2">
        {[amount, unit].map((field, index) => (
          <div key={field.name}>
            <FieldLabel id={baseId + "-" + String(index)}>{fieldLabel(field.name)}</FieldLabel>
            <FieldControl field={field} multiline={false} value={value[field.name]} onChange={(next) => setField(field.name, next)} labelledBy={baseId + "-" + String(index)} labels={labels} />
          </div>
        ))}
      </div>
    );
  }
  return <div className="flex flex-col gap-3">{fields.map(control)}</div>;
}
