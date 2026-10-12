import { useId } from "react";

import type { components } from "../api/schema";
import { t, type MessageKey } from "../i18n";

export type Question = components["schemas"]["Question"];
export type QuestionKind = components["schemas"]["QuestionKind"];

/** The answer object exactly as it becomes `AnswerRequest.answer`. */
export type AnswerValue = Readonly<Record<string, unknown>>;

/** How a kind presents its schema fields; every kind maps to one explicitly. */
export type AnswerRenderer =
  | "yes-no"
  | "pick-one"
  | "pick-many"
  | "number"
  | "text"
  | "cardinality"
  | "unit"
  | "precision"
  | "rounding"
  | "formula"
  | "rule-cell"
  | "calendar"
  | "role-assignment"
  | "permission"
  | "quality-threshold"
  | "architecture-choice"
  | "technology-choice"
  | "interface-choice"
  | "verification-method";

/** The renderer of `kind`. Exhaustive: a new QuestionKind fails to compile until it is mapped. */
export function answerRenderer(kind: QuestionKind): AnswerRenderer {
  switch (kind) {
    case "YesNo":
      return "yes-no";
    case "PickOne":
      return "pick-one";
    case "PickMany":
      return "pick-many";
    case "Number":
      return "number";
    case "Text":
      return "text";
    case "Cardinality":
      return "cardinality";
    case "Unit":
      return "unit";
    case "Precision":
      return "precision";
    case "Rounding":
      return "rounding";
    case "FormulaConfirm":
      return "formula";
    case "RuleCell":
      return "rule-cell";
    case "Calendar":
      return "calendar";
    case "RoleAssignment":
      return "role-assignment";
    case "Permission":
      return "permission";
    case "QualityThreshold":
      return "quality-threshold";
    case "ArchitectureChoice":
      return "architecture-choice";
    case "TechnologyChoice":
      return "technology-choice";
    case "InterfaceChoice":
      return "interface-choice";
    case "VerificationMethod":
      return "verification-method";
    default: {
      const unmapped: never = kind;
      throw new Error("unmapped question kind " + String(unmapped));
    }
  }
}

/** Renderers whose free strings are long text (multi-line input). */
const MULTILINE: ReadonlySet<AnswerRenderer> = new Set(["text", "formula", "rule-cell"]);

/** One renderable field of a closed answer schema. */
export type AnswerField =
  | { readonly name: string; readonly type: "enum"; readonly choices: readonly string[] }
  | { readonly name: string; readonly type: "enum-set"; readonly choices: readonly string[]; readonly minItems: number }
  | { readonly name: string; readonly type: "string"; readonly minLength: number; readonly maxLength: number | null }
  | { readonly name: string; readonly type: "number"; readonly integer: boolean; readonly minimum: number | null; readonly maximum: number | null }
  | { readonly name: string; readonly type: "boolean" };

const STRING_KEYS = new Set(["type", "enum", "minLength", "maxLength", "description", "title"]);
const NUMBER_KEYS = new Set(["type", "minimum", "maximum", "description", "title"]);
const ARRAY_KEYS = new Set(["type", "items", "uniqueItems", "minItems", "description", "title"]);

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function onlyKeys(value: Record<string, unknown>, allowed: ReadonlySet<string>): boolean {
  return Object.keys(value).every((key) => allowed.has(key));
}

function stringEnum(value: unknown): string[] | null {
  if (!Array.isArray(value) || value.length === 0 || !value.every((v) => typeof v === "string")) {
    return null;
  }
  return value.length === new Set(value).size ? (value as string[]) : null;
}

function optionalNumber(value: unknown): number | null | undefined {
  if (value === undefined) {
    return null;
  }
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function parseField(name: string, schema: unknown): AnswerField | null {
  if (!isObject(schema)) {
    return null;
  }
  switch (schema["type"]) {
    case "string": {
      if (!onlyKeys(schema, STRING_KEYS)) {
        return null;
      }
      if (schema["enum"] !== undefined) {
        const choices = stringEnum(schema["enum"]);
        return choices === null ? null : { name, type: "enum", choices };
      }
      const minLength = optionalNumber(schema["minLength"]);
      const maxLength = optionalNumber(schema["maxLength"]);
      if (minLength === undefined || maxLength === undefined) {
        return null;
      }
      return { name, type: "string", minLength: minLength ?? 0, maxLength };
    }
    case "number":
    case "integer": {
      const minimum = optionalNumber(schema["minimum"]);
      const maximum = optionalNumber(schema["maximum"]);
      if (!onlyKeys(schema, NUMBER_KEYS) || minimum === undefined || maximum === undefined) {
        return null;
      }
      return { name, type: "number", integer: schema["type"] === "integer", minimum, maximum };
    }
    case "boolean":
      return Object.keys(schema).every((key) => key === "type" || key === "description" || key === "title")
        ? { name, type: "boolean" }
        : null;
    case "array": {
      const items = schema["items"];
      const minItems = optionalNumber(schema["minItems"]);
      if (!onlyKeys(schema, ARRAY_KEYS) || schema["uniqueItems"] !== true || minItems === undefined) {
        return null;
      }
      if (!isObject(items) || items["type"] !== "string" || !onlyKeys(items, new Set(["type", "enum"]))) {
        return null;
      }
      const choices = stringEnum(items["enum"]);
      return choices === null ? null : { name, type: "enum-set", choices, minItems: minItems ?? 0 };
    }
    default:
      return null;
  }
}

/**
 * The fields of a closed answer schema (`type: object`, `additionalProperties: false`, every
 * property required), in schema property order; null when the schema cannot be rendered safely.
 */
export function parseAnswerSchema(schema: unknown): AnswerField[] | null {
  if (!isObject(schema) || schema["type"] !== "object" || schema["additionalProperties"] !== false) {
    return null;
  }
  const properties = schema["properties"];
  const required = schema["required"];
  if (!isObject(properties) || !Array.isArray(required)) {
    return null;
  }
  const names = Object.keys(properties);
  if (names.length === 0 || required.length !== names.length || !names.every((name) => required.includes(name))) {
    return null;
  }
  const fields: AnswerField[] = [];
  for (const name of names) {
    const field = parseField(name, properties[name]);
    if (field === null) {
      return null;
    }
    fields.push(field);
  }
  return fields;
}

function fieldComplete(field: AnswerField, value: unknown): boolean {
  switch (field.type) {
    case "enum":
      return typeof value === "string" && field.choices.includes(value);
    case "enum-set":
      return Array.isArray(value) && value.length >= field.minItems && value.every((v) => field.choices.includes(v as string));
    case "string":
      return typeof value === "string" && value.length >= field.minLength && (field.maxLength === null || value.length <= field.maxLength);
    case "number":
      return (
        typeof value === "number" &&
        Number.isFinite(value) &&
        (!field.integer || Number.isInteger(value)) &&
        (field.minimum === null || value >= field.minimum) &&
        (field.maximum === null || value <= field.maximum)
      );
    case "boolean":
      return typeof value === "boolean";
  }
}

/** Whether `value` has exactly the schema's fields, each valid. */
export function isAnswerComplete(schema: unknown, value: AnswerValue): boolean {
  const fields = parseAnswerSchema(schema);
  if (fields === null) {
    return false;
  }
  const keys = Object.keys(value);
  return keys.length === fields.length && fields.every((field) => fieldComplete(field, value[field.name]));
}

const RENDERER_LABEL: Readonly<Record<AnswerRenderer, MessageKey>> = {
  "yes-no": "answer.kind.yesNo",
  "pick-one": "answer.kind.pickOne",
  "pick-many": "answer.kind.pickMany",
  number: "answer.kind.number",
  text: "answer.kind.text",
  cardinality: "answer.kind.cardinality",
  unit: "answer.kind.unit",
  precision: "answer.kind.precision",
  rounding: "answer.kind.rounding",
  formula: "answer.kind.formula",
  "rule-cell": "answer.kind.ruleCell",
  calendar: "answer.kind.calendar",
  "role-assignment": "answer.kind.roleAssignment",
  permission: "answer.kind.permission",
  "quality-threshold": "answer.kind.qualityThreshold",
  "architecture-choice": "answer.kind.architectureChoice",
  "technology-choice": "answer.kind.technologyChoice",
  "interface-choice": "answer.kind.interfaceChoice",
  "verification-method": "answer.kind.verificationMethod",
};

function FieldControl({
  field,
  renderer,
  value,
  onChange,
}: {
  field: AnswerField;
  renderer: AnswerRenderer;
  value: unknown;
  onChange: (next: unknown) => void;
}) {
  const id = useId();
  const label = (
    <label htmlFor={id} className="block font-mono text-xs">
      {field.name}
    </label>
  );
  switch (field.type) {
    case "enum":
      return (
        <div data-field={field.name} data-field-type="enum">
          {label}
          <select
            id={id}
            value={typeof value === "string" ? value : ""}
            onChange={(event) => onChange(event.target.value === "" ? undefined : event.target.value)}
            className="rounded border"
          >
            <option value="">{t("answer.choose")}</option>
            {field.choices.map((choice) => (
              <option key={choice} value={choice}>
                {choice}
              </option>
            ))}
          </select>
        </div>
      );
    case "enum-set": {
      const selected = Array.isArray(value) ? (value as string[]) : [];
      return (
        <fieldset data-field={field.name} data-field-type="enum-set">
          <legend className="font-mono text-xs">{field.name}</legend>
          {field.choices.map((choice) => (
            <label key={choice} className="mr-3 inline-flex items-center gap-1">
              <input
                type="checkbox"
                checked={selected.includes(choice)}
                onChange={(event) => {
                  const next = event.target.checked
                    ? field.choices.filter((c) => c === choice || selected.includes(c))
                    : selected.filter((c) => c !== choice);
                  onChange(next);
                }}
              />
              {choice}
            </label>
          ))}
        </fieldset>
      );
    }
    case "string":
      return (
        <div data-field={field.name} data-field-type="string">
          {label}
          {MULTILINE.has(renderer) ? (
            <textarea
              id={id}
              value={typeof value === "string" ? value : ""}
              onChange={(event) => onChange(event.target.value === "" ? undefined : event.target.value)}
              className="w-full rounded border font-mono"
            />
          ) : (
            <input
              id={id}
              type="text"
              value={typeof value === "string" ? value : ""}
              onChange={(event) => onChange(event.target.value === "" ? undefined : event.target.value)}
              className="rounded border"
            />
          )}
        </div>
      );
    case "number":
      return (
        <div data-field={field.name} data-field-type="number">
          {label}
          <input
            id={id}
            type="number"
            step={field.integer ? 1 : "any"}
            min={field.minimum ?? undefined}
            max={field.maximum ?? undefined}
            value={typeof value === "number" ? value : ""}
            onChange={(event) => onChange(event.target.value === "" ? undefined : Number(event.target.value))}
            className="rounded border"
          />
        </div>
      );
    case "boolean":
      return (
        <fieldset data-field={field.name} data-field-type="boolean">
          <legend className="font-mono text-xs">{field.name}</legend>
          <label className="mr-3 inline-flex items-center gap-1">
            <input type="radio" name={id} checked={value === true} onChange={() => onChange(true)} />
            {t("answer.yes")}
          </label>
          <label className="inline-flex items-center gap-1">
            <input type="radio" name={id} checked={value === false} onChange={() => onChange(false)} />
            {t("answer.no")}
          </label>
        </fieldset>
      );
  }
}

/**
 * The answer control of one Question. Its kind selects an explicit renderer; its fields come only
 * from `answer_schema` (property names, required set, enum choices and constraints). A missing or
 * unrenderable schema is shown as unsupported and never produces an answer. The control is
 * controlled: `onChange` receives the exact answer object, without rationale or supersession.
 */
export function TypedAnswerControl({
  question,
  value,
  onChange,
}: {
  question: Pick<Question, "id" | "kind" | "answer_schema">;
  value: AnswerValue;
  onChange: (next: AnswerValue) => void;
}) {
  const renderer = answerRenderer(question.kind);
  const fields = parseAnswerSchema(question.answer_schema);
  if (fields === null) {
    return (
      <div role="alert" data-renderer={renderer} data-answer-state="unsupported" className="rounded border border-red-700 p-2 text-sm">
        {t("answer.unsupported")}
      </div>
    );
  }
  const setField = (name: string, next: unknown) => {
    const updated: Record<string, unknown> = {};
    for (const field of fields) {
      const current = field.name === name ? next : value[field.name];
      if (current !== undefined) {
        updated[field.name] = current;
      }
    }
    onChange(updated);
  };
  return (
    <fieldset data-renderer={renderer} data-answer-state="ready" className="space-y-2 rounded border p-2 text-sm">
      <legend className="font-semibold">{t(RENDERER_LABEL[renderer])}</legend>
      {fields.map((field) => (
        <FieldControl key={field.name} field={field} renderer={renderer} value={value[field.name]} onChange={(next) => setField(field.name, next)} />
      ))}
    </fieldset>
  );
}
