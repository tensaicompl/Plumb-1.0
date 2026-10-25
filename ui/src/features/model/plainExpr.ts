// A plain-language reading of the PlumbExpr subset the pilot uses (calls, lists, arithmetic,
// comparisons, and/or/not, identifiers and literals). It only rephrases the exact expression the
// server returned; anything it cannot parse has no reading (null). No I/O.
import { t, tf, type MessageKey } from "../../i18n";

type Node =
  | { readonly kind: "number"; readonly text: string }
  | { readonly kind: "string"; readonly text: string }
  | { readonly kind: "bool"; readonly value: boolean }
  | { readonly kind: "ident"; readonly name: string }
  | { readonly kind: "list"; readonly items: readonly Node[] }
  | { readonly kind: "call"; readonly name: string; readonly args: readonly Node[] }
  | { readonly kind: "unary"; readonly op: string; readonly expr: Node }
  | { readonly kind: "binary"; readonly op: string; readonly left: Node; readonly right: Node };

const TOKEN = /\s*(\d+(?:\.\d+)?|"(?:[^"\\]|\\.)*"|[A-Za-z_][A-Za-z0-9_.]*|==|!=|<=|>=|[-+*/%<>()[\],])/y;

function tokenize(text: string): string[] | null {
  const tokens: string[] = [];
  TOKEN.lastIndex = 0;
  let index = 0;
  while (index < text.length) {
    if (/^\s+$/.test(text.slice(index))) {
      break;
    }
    TOKEN.lastIndex = index;
    const match = TOKEN.exec(text);
    if (match === null || match[1] === undefined) {
      return null;
    }
    tokens.push(match[1]);
    index = TOKEN.lastIndex;
  }
  return tokens;
}

const LEVELS: readonly (readonly string[])[] = [["implies"], ["or"], ["and"], ["==", "!=", "<", "<=", ">", ">=", "in"], ["+", "-"], ["*", "/", "%"]];

function parse(tokens: readonly string[]): Node | null {
  let position = 0;
  const peek = () => tokens[position];
  const take = () => tokens[position++];
  const expect = (token: string) => {
    if (take() !== token) {
      throw new Error("unexpected token");
    }
  };
  const list = (close: string): Node[] => {
    const items: Node[] = [];
    if (peek() === close) {
      take();
      return items;
    }
    for (;;) {
      items.push(level(0));
      const next = take();
      if (next === close) {
        return items;
      }
      if (next !== ",") {
        throw new Error("expected a comma");
      }
    }
  };
  const primary = (): Node => {
    const token = take();
    if (token === undefined) {
      throw new Error("unexpected end");
    }
    if (token === "(") {
      const inner = level(0);
      expect(")");
      return inner;
    }
    if (token === "[") {
      return { kind: "list", items: list("]") };
    }
    if (token === "not" || token === "-") {
      return { kind: "unary", op: token, expr: primary() };
    }
    if (/^\d/.test(token)) {
      return { kind: "number", text: token };
    }
    if (token.startsWith('"')) {
      return { kind: "string", text: token.slice(1, -1) };
    }
    if (token === "true" || token === "false") {
      return { kind: "bool", value: token === "true" };
    }
    if (/^[A-Za-z_]/.test(token)) {
      if (peek() === "(") {
        take();
        return { kind: "call", name: token, args: list(")") };
      }
      return { kind: "ident", name: token };
    }
    throw new Error("unexpected token");
  };
  const level = (depth: number): Node => {
    const ops = LEVELS[depth];
    if (ops === undefined) {
      return primary();
    }
    let left = level(depth + 1);
    for (let op = peek(); op !== undefined && ops.includes(op); op = peek()) {
      take();
      left = { kind: "binary", op, left, right: level(depth + 1) };
    }
    return left;
  };
  try {
    const node = level(0);
    return position === tokens.length ? node : null;
  } catch {
    return null;
  }
}

const BINARY: Readonly<Record<string, MessageKey>> = {
  "+": "model.calc.op.plus",
  "-": "model.calc.op.minus",
  "*": "model.calc.op.times",
  "/": "model.calc.op.divided",
  "%": "model.calc.op.remainder",
  "==": "model.calc.op.eq",
  "!=": "model.calc.op.ne",
  "<": "model.calc.op.lt",
  "<=": "model.calc.op.le",
  ">": "model.calc.op.gt",
  ">=": "model.calc.op.ge",
  in: "model.calc.op.in",
  and: "model.calc.op.and",
  or: "model.calc.op.or",
  implies: "model.calc.op.implies",
};

const CALLS: Readonly<Record<string, MessageKey>> = {
  days_between: "model.calc.fn.daysBetween",
  min: "model.calc.fn.min",
  max: "model.calc.fn.max",
  sum: "model.calc.fn.sum",
  count: "model.calc.fn.count",
  exists: "model.calc.fn.exists",
  any: "model.calc.fn.any",
  all: "model.calc.fn.all",
  direct_manager: "model.calc.fn.directManager",
};

function joinAnd(items: readonly string[]): string {
  if (items.length <= 1) {
    return items.join("");
  }
  return items.slice(0, -1).join(", ") + t("model.calc.and") + items[items.length - 1];
}

function render(node: Node): string {
  switch (node.kind) {
    case "number":
      return node.text;
    case "string":
      return "“" + node.text + "”";
    case "bool":
      return t(node.value ? "model.rules.yes" : "model.rules.no").toLowerCase();
    case "ident":
      return tf("model.calc.ident", { name: node.name.replace(/[_.]/g, " ") });
    case "list":
      return joinAnd(node.items.map(render));
    case "unary":
      return node.op === "not" ? tf("model.calc.op.not", { a: render(node.expr) }) : tf("model.calc.op.negative", { a: render(node.expr) });
    case "binary": {
      const key = BINARY[node.op];
      const left = render(node.left);
      // A left operand with its own clauses ("…, counting both, using the calendar") ends with a comma.
      const a = left.includes(", ") ? left + "," : left;
      return key === undefined ? a + " " + node.op + " " + render(node.right) : tf(key, { a, b: render(node.right) });
    }
    case "call": {
      const [first, second, third] = node.args;
      if (node.name === "working_days" && first?.kind === "list" && first.items.length === 2 && second !== undefined) {
        const [from, to] = first.items as [Node, Node];
        const inclusive = third === undefined || (third.kind === "bool" && third.value);
        return tf(inclusive ? "model.calc.fn.workingDays" : "model.calc.fn.workingDaysExclusive", { from: render(from), to: render(to), calendar: render(second) });
      }
      const key = CALLS[node.name];
      if (key !== undefined && first !== undefined) {
        return tf(key, { a: render(first), b: second === undefined ? "" : render(second), list: joinAnd(node.args.map(render)) });
      }
      return node.name + "(" + node.args.map(render).join(", ") + ")";
    }
  }
}

/** The expression in plain words, as one sentence, or null when it cannot be read. */
export function plainExpression(expression: string): string | null {
  const tokens = tokenize(expression);
  const node = tokens === null || tokens.length === 0 ? null : parse(tokens);
  if (node === null) {
    return null;
  }
  const text = render(node);
  return text.charAt(0).toUpperCase() + text.slice(1) + ".";
}

/** The identifiers the expression reads (function names excluded). */
export function expressionInputs(expression: string): Set<string> {
  const inputs = new Set<string>();
  const visit = (node: Node) => {
    if (node.kind === "ident") {
      inputs.add(node.name);
    } else if (node.kind === "list" || node.kind === "call") {
      (node.kind === "list" ? node.items : node.args).forEach(visit);
    } else if (node.kind === "unary") {
      visit(node.expr);
    } else if (node.kind === "binary") {
      visit(node.left);
      visit(node.right);
    }
  };
  const tokens = tokenize(expression);
  const node = tokens === null ? null : parse(tokens);
  if (node !== null) {
    visit(node);
  }
  return inputs;
}

/** The seven ISO dates (Monday to Sunday) of the first full week of the year after `today`. */
export function exampleWeek(today: Date): string[] {
  const year = today.getUTCFullYear() + 1;
  const first = new Date(Date.UTC(year, 0, 1));
  const offset = (8 - first.getUTCDay()) % 7;
  return Array.from({ length: 7 }, (_, index) => new Date(Date.UTC(year, 0, 1 + offset + index)).toISOString().slice(0, 10));
}
