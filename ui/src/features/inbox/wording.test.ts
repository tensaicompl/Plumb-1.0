// The Wording (EARS) card helpers: the word diff, the EARS pattern and the modality.
import { describe, expect, it } from "vitest";

import { earsPattern, modalityOf, wordDiff } from "./wording";

describe("wording", () => {
  it("diffs words, grouping the runs that are only in one text", () => {
    expect(wordDiff("The system should respond fast.", "The system shall respond fast.")).toEqual({
      removed: [
        { text: "The system", changed: false },
        { text: "should", changed: true },
        { text: "respond fast.", changed: false },
      ],
      added: [
        { text: "The system", changed: false },
        { text: "shall", changed: true },
        { text: "respond fast.", changed: false },
      ],
    });
    expect(wordDiff("", "When it rains")).toEqual({ removed: [], added: [{ text: "When it rains", changed: true }] });
  });

  it("names the EARS pattern from the opening keyword", () => {
    expect(earsPattern("When an employee submits, the system shall …")).toBe("inbox.wording.pattern.event");
    expect(earsPattern("While a request is open, the system shall …")).toBe("inbox.wording.pattern.state");
    expect(earsPattern("If the balance is negative, then the system shall …")).toBe("inbox.wording.pattern.unwanted");
    expect(earsPattern("Where payroll is enabled, the system shall …")).toBe("inbox.wording.pattern.optional");
    expect(earsPattern("The system shall keep an audit log.")).toBe("inbox.wording.pattern.ubiquitous");
    expect(earsPattern("")).toBe("inbox.wording.pattern.ubiquitous");
  });

  it("finds the modality keyword, preferring the negative form", () => {
    expect(modalityOf("The system shall not delete approved requests.")).toBe("shall not");
    expect(modalityOf("The system shall, on request, export.")).toBe("shall");
    expect(modalityOf("A manager should decide within 2 days.")).toBe("should");
    expect(modalityOf("An employee may withdraw a request.")).toBe("may");
    expect(modalityOf("Requests are stored.")).toBeNull();
  });
});
