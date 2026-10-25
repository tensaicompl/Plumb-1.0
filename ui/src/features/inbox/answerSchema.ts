// The closed answer schema of a Question (Hotfix 065/076): `answer_schema` is the typed-answer
// authority. Its fields are parsed strictly; a schema that cannot be rendered safely is refused
// and never produces an answer.

/** The answer object exactly as it becomes `AnswerRequest.answer`. */
export type AnswerValue = Readonly<Record<string, unknown>>;

/** One renderable field of a closed answer schema. */
export type AnswerField =
  | { readonly name: string; readonly type: "enum"; readonly choices: readonly string[] }
  | { readonly name: string; readonly type: "enum-set"; readonly choices: readonly string[]; readonly minItems: number }
  | { readonly name: string; readonly type: "string"; readonly minLength: number; readonly maxLength: number | null }
  | { readonly name: string; readonly type: "number"; readonly integer: boolean; readonly minimum: number | null; readonly maximum: number | null }
  | { readonly name: string; readonly type: "boolean" }
  | { readonly name: string; readonly type: "const"; readonly value: string | number | boolean };

const STRING_KEYS = new Set(["type", "enum", "minLength", "maxLength", "description", "title"]);
const NUMBER_KEYS = new Set(["type", "minimum", "maximum", "description", "title"]);
const ARRAY_KEYS = new Set(["type", "items", "uniqueItems", "minItems", "description", "title"]);
const CONST_KEYS = new Set(["const", "description", "title"]);
const BOOLEAN_KEYS = new Set(["type", "description", "title"]);

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
  // A fixed scalar value (JSON Schema `const`) with no other constraint: shown, never edited.
  if ("const" in schema) {
    const value = schema["const"];
    const scalar = typeof value === "string" || typeof value === "boolean" || (typeof value === "number" && Number.isFinite(value));
    return scalar && onlyKeys(schema, CONST_KEYS) ? { name, type: "const", value } : null;
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
      return onlyKeys(schema, BOOLEAN_KEYS) ? { name, type: "boolean" } : null;
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
    case "const":
      return value === field.value;
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

/** The answer of `fields` from `value`: unset fields omitted, every const field at its value. */
export function withConsts(fields: readonly AnswerField[], value: AnswerValue): AnswerValue {
  const updated: Record<string, unknown> = {};
  for (const field of fields) {
    const current = field.type === "const" ? field.value : value[field.name];
    if (current !== undefined) {
      updated[field.name] = current;
    }
  }
  return updated;
}
