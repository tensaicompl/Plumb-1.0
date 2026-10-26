// The typed answer control of a question card (handoff "Question": Yes/No segmented, radio list,
// pick many, number + unit, text, reference picker and a fixed value). Its fields come only from
// the question's closed `answer_schema`; the answer object is exactly what `answer_question` gets.
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";

import type { components } from "../../api/schema";
import { axeViolations } from "../../test/axe";
import { AnswerControl } from "./AnswerControl";
import { isAnswerComplete, parseAnswerSchema, type AnswerValue } from "./answerSchema";

type Question = components["schemas"]["Question"];

afterEach(cleanup);

function q(kind: Question["kind"], answer_schema: unknown): Pick<Question, "id" | "kind" | "answer_schema"> {
  return { id: "q:1", kind, answer_schema: answer_schema as Question["answer_schema"] };
}

function object(properties: Record<string, unknown>) {
  return { type: "object", additionalProperties: false, properties, required: Object.keys(properties) };
}

function Harness({ question, initial = {}, labels }: { question: Pick<Question, "id" | "kind" | "answer_schema">; initial?: AnswerValue; labels?: (id: string) => string }) {
  const [value, setValue] = useState<AnswerValue>(initial);
  return (
    <div>
      <p id="answer-label">Answer</p>
      <AnswerControl question={question} value={value} onChange={setValue} labelledBy="answer-label" {...(labels === undefined ? {} : { labels })} />
      <output data-testid="value">{JSON.stringify(value)}</output>
      <output data-testid="complete">{String(isAnswerComplete(question.answer_schema, value))}</output>
    </div>
  );
}

const value = () => JSON.parse(screen.getByTestId("value").textContent ?? "{}") as Record<string, unknown>;
const complete = () => screen.getByTestId("complete").textContent;

describe("AnswerControl", () => {
  it("answers a yes/no question with a segmented control", async () => {
    const user = userEvent.setup();
    render(<Harness question={q("YesNo", object({ inclusive_end: { type: "boolean" } }))} />);
    const group = screen.getByRole("radiogroup", { name: "Answer" });
    expect(within(group).getAllByRole("radio").map((r) => r.textContent)).toEqual(["Yes", "No"]);
    expect(complete()).toBe("false");
    await user.click(within(group).getByRole("radio", { name: "Yes" }));
    expect(value()).toEqual({ inclusive_end: true });
    expect(complete()).toBe("true");
    await user.click(within(group).getByRole("radio", { name: "No" }));
    expect(value()).toEqual({ inclusive_end: false });
    expect(await axeViolations()).toEqual([]);
  });

  it("answers a pick-one question with a vertical radio list", async () => {
    const user = userEvent.setup();
    render(<Harness question={q("PickOne", object({ approver: { type: "string", enum: ["No — HR must approve", "Yes", "Yes, and HR is notified"] } }))} />);
    const group = screen.getByRole("radiogroup", { name: "Answer" });
    const yes = within(group).getByRole("radio", { name: "Yes" });
    await user.click(yes);
    expect(yes.getAttribute("aria-checked")).toBe("true");
    expect(yes.className).toContain("border-primary");
    expect(value()).toEqual({ approver: "Yes" });
    expect(await axeViolations()).toEqual([]);
  });

  it("picks references with a filter and plain kind labels", async () => {
    const user = userEvent.setup();
    const refs = ["event:4f49", "event:6c1c", "event:d8b3", "operation:30e3", "operation:58a5", "operation:6c54", "operation:9a10"];
    render(<Harness question={q("PickOne", object({ trigger_ref: { type: "string", enum: refs } }))} />);
    const filter = screen.getByRole("searchbox", { name: "Filter choices" });
    await user.type(filter, "operation");
    const group = screen.getByRole("radiogroup", { name: "Answer" });
    expect(within(group).getAllByRole("radio")).toHaveLength(4);
    await user.click(within(group).getByRole("radio", { name: /operation:58a5/ }));
    expect(value()).toEqual({ trigger_ref: "operation:58a5" });
    expect(within(group).getByRole("radio", { name: /operation:58a5/ }).textContent).toContain("Operation");
    await user.clear(filter);
    await user.type(filter, "nothing");
    expect(screen.getByText("No choice matches.")).toBeTruthy();
  });

  it("names referenced elements in plain language, keeping the kind and ID second, and filters by name", async () => {
    const user = userEvent.setup();
    const refs = ["event:4f49", "event:6c1c", "event:d8b3", "operation:30e3", "operation:58a5", "operation:6c54", "operation:f463"];
    const names: Record<string, string> = { "event:4f49": "Leave approved", "operation:30e3": "Submit leave request", "operation:f463": "Withdraw leave request" };
    render(<Harness question={q("PickOne", object({ trigger_ref: { type: "string", enum: refs } }))} labels={(id) => names[id] ?? id} />);
    const group = screen.getByRole("radiogroup", { name: "Answer" });
    const withdraw = within(group).getByRole("radio", { name: /^Withdraw leave request/ });
    expect(withdraw.textContent).toContain("Operation");
    expect(withdraw.textContent).toContain("operation:f463");
    // An element without a name keeps its kind first.
    expect(within(group).getByRole("radio", { name: /^Event event:6c1c/ })).toBeTruthy();
    await user.type(screen.getByRole("searchbox", { name: "Filter choices" }), "withdraw");
    expect(within(group).getAllByRole("radio")).toHaveLength(1);
    await user.click(within(group).getByRole("radio", { name: /^Withdraw leave request/ }));
    expect(value()).toEqual({ trigger_ref: "operation:f463" });
    expect(await axeViolations()).toEqual([]);
  });

  it("picks many, in schema order, and requires the minimum", async () => {
    const user = userEvent.setup();
    render(<Harness question={q("PickMany", object({ days: { type: "array", uniqueItems: true, minItems: 1, items: { type: "string", enum: ["mon", "tue", "wed"] } } }))} />);
    const group = screen.getByRole("group", { name: "Answer" });
    await user.click(within(group).getByRole("checkbox", { name: "wed" }));
    await user.click(within(group).getByRole("checkbox", { name: "mon" }));
    expect(value()).toEqual({ days: ["mon", "wed"] });
    expect(complete()).toBe("true");
    await user.click(within(group).getByRole("checkbox", { name: "mon" }));
    await user.click(within(group).getByRole("checkbox", { name: "wed" }));
    expect(value()).toEqual({ days: [] });
    expect(complete()).toBe("false");
  });

  it("answers a number with its unit side by side", async () => {
    const user = userEvent.setup();
    render(<Harness question={q("Number", object({ decimal: { type: "string", minLength: 1 }, unit: { type: "string", minLength: 1 } }))} />);
    const row = screen.getByTestId("number-unit");
    await user.type(within(row).getByRole("textbox", { name: "decimal" }), "0.5");
    await user.type(within(row).getByRole("textbox", { name: "unit" }), "day");
    expect(value()).toEqual({ decimal: "0.5", unit: "day" });
    expect(complete()).toBe("true");
  });

  it("answers numbers within their bounds and clears an emptied field", async () => {
    const user = userEvent.setup();
    render(<Harness question={q("QualityThreshold", object({ threshold: { type: "integer", minimum: 1, maximum: 10 } }))} />);
    const input = screen.getByRole("spinbutton", { name: "threshold" });
    await user.type(input, "12");
    expect(value()).toEqual({ threshold: 12 });
    expect(complete()).toBe("false");
    await user.clear(input);
    expect(value()).toEqual({});
  });

  it("answers text in a multiline field for formulas and free text", async () => {
    const user = userEvent.setup();
    render(<Harness question={q("Text", object({ note: { type: "string", minLength: 3, maxLength: 20 } }))} />);
    const area = screen.getByRole("textbox", { name: "note" });
    expect(area.tagName).toBe("TEXTAREA");
    await user.type(area, "ok");
    expect(complete()).toBe("false");
    await user.type(area, "ay");
    expect(value()).toEqual({ note: "okay" });
    expect(complete()).toBe("true");
  });

  it("shows a fixed value read-only and always includes it in the answer", async () => {
    const user = userEvent.setup();
    render(
      <Harness
        question={q("YesNo", object({ holiday_consumes_working_day: { const: false }, calendar_ref: { type: "string", enum: ["calendar:office"] } }))}
      />,
    );
    expect(screen.getByText("Fixed value")).toBeTruthy();
    expect(screen.getByTestId("fixed-holiday_consumes_working_day").textContent).toContain("No");
    expect(value()).toEqual({ holiday_consumes_working_day: false });
    await user.click(screen.getByRole("radio", { name: /calendar:office/ }));
    expect(value()).toEqual({ holiday_consumes_working_day: false, calendar_ref: "calendar:office" });
    expect(complete()).toBe("true");
  });

  it("labels each field of a multi-field schema and offers yes/no for a boolean among others", async () => {
    const user = userEvent.setup();
    render(<Harness question={q("Rounding", object({ rounding_mode: { type: "string", enum: ["half_up", "down"] }, applies: { type: "boolean" } }))} />);
    expect(screen.getByText("rounding mode")).toBeTruthy();
    const applies = screen.getByRole("radiogroup", { name: "applies" });
    await user.click(within(applies).getByRole("radio", { name: "Yes" }));
    expect(value()).toEqual({ applies: true });
  });

  it("explains an answer schema it cannot render and never produces an answer", () => {
    render(<Harness question={q("Text", { type: "object", properties: { x: { type: "string" } } })} />);
    expect(screen.getByRole("alert").textContent).toBe("This question cannot be answered here: its answer format is not supported.");
    expect(complete()).toBe("false");
  });
});

describe("answer schema", () => {
  it("accepts only closed object schemas with renderable fields", () => {
    expect(parseAnswerSchema(null)).toBeNull();
    expect(parseAnswerSchema(object({}))).toBeNull();
    expect(parseAnswerSchema({ ...object({ a: { type: "string" } }), required: [] })).toBeNull();
    expect(parseAnswerSchema(object({ a: { type: "string", pattern: "x" } }))).toBeNull();
    expect(parseAnswerSchema(object({ a: { type: "string", enum: ["x", "x"] } }))).toBeNull();
    expect(parseAnswerSchema(object({ a: { type: "number", minimum: "1" } }))).toBeNull();
    expect(parseAnswerSchema(object({ a: { type: "boolean", default: true } }))).toBeNull();
    expect(parseAnswerSchema(object({ a: { type: "array", items: { type: "string", enum: ["x"] } } }))).toBeNull();
    expect(parseAnswerSchema(object({ a: { type: "array", uniqueItems: true, items: { type: "number" } } }))).toBeNull();
    expect(parseAnswerSchema(object({ a: { const: { nested: true } } }))).toBeNull();
    expect(parseAnswerSchema(object({ a: { type: "object" } }))).toBeNull();
    expect(parseAnswerSchema(object({ a: "string" }))).toBeNull();
    expect(parseAnswerSchema(object({ a: { type: "number", minimum: 0 } }))).toEqual([{ name: "a", type: "number", integer: false, minimum: 0, maximum: null }]);
  });

  it("checks completeness field by field", () => {
    const schema = object({ a: { type: "string", enum: ["x"] }, b: { type: "boolean" } });
    expect(isAnswerComplete(schema, { a: "x", b: true })).toBe(true);
    expect(isAnswerComplete(schema, { a: "y", b: true })).toBe(false);
    expect(isAnswerComplete(schema, { a: "x" })).toBe(false);
    expect(isAnswerComplete(schema, { a: "x", b: true, c: 1 })).toBe(false);
    expect(isAnswerComplete({}, {})).toBe(false);
    expect(isAnswerComplete(object({ n: { type: "integer" } }), { n: 1.5 })).toBe(false);
  });
});
