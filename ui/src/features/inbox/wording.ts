// The Wording (EARS) card: a word diff of the original statement and the suggested structured one,
// and the EARS pattern and modality the suggestion uses.
import type { MessageKey } from "../../i18n";

/** A run of words that is kept, or only in one of the two texts. */
export interface WordRun {
  readonly text: string;
  readonly changed: boolean;
}

function words(text: string): string[] {
  return text.split(/\s+/).filter((w) => w !== "");
}

function runs(tokens: readonly string[], keep: readonly boolean[]): WordRun[] {
  const out: WordRun[] = [];
  tokens.forEach((token, index) => {
    const changed = !keep[index];
    const last = out[out.length - 1];
    if (last !== undefined && last.changed === changed) {
      out[out.length - 1] = { text: last.text + " " + token, changed };
    } else {
      out.push({ text: token, changed });
    }
  });
  return out;
}

/**
 * The word diff of `original` and `suggested` (longest common subsequence of words): the original
 * with its removed runs and the suggestion with its added runs.
 */
export function wordDiff(original: string, suggested: string): { removed: WordRun[]; added: WordRun[] } {
  const a = words(original);
  const b = words(suggested);
  const lcs: number[][] = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i -= 1) {
    for (let j = b.length - 1; j >= 0; j -= 1) {
      const row = lcs[i] as number[];
      row[j] = a[i] === b[j] ? (lcs[i + 1]?.[j + 1] ?? 0) + 1 : Math.max(lcs[i + 1]?.[j] ?? 0, row[j + 1] ?? 0);
    }
  }
  const keepA = new Array<boolean>(a.length).fill(false);
  const keepB = new Array<boolean>(b.length).fill(false);
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      keepA[i] = true;
      keepB[j] = true;
      i += 1;
      j += 1;
    } else if ((lcs[i + 1]?.[j] ?? 0) >= (lcs[i]?.[j + 1] ?? 0)) {
      i += 1;
    } else {
      j += 1;
    }
  }
  return { removed: runs(a, keepA), added: runs(b, keepB) };
}

/** The EARS pattern of a structured statement, from its opening keyword. */
export function earsPattern(statement: string): MessageKey {
  const first = statement.trim().split(/\s+/)[0]?.toLowerCase() ?? "";
  switch (first) {
    case "when":
      return "inbox.wording.pattern.event";
    case "while":
      return "inbox.wording.pattern.state";
    case "if":
      return "inbox.wording.pattern.unwanted";
    case "where":
      return "inbox.wording.pattern.optional";
    default:
      return "inbox.wording.pattern.ubiquitous";
  }
}

/** The modality keyword a statement uses ("shall", "shall not", "should", "may"), or null. */
export function modalityOf(statement: string): string | null {
  const text = " " + statement.toLowerCase().replace(/[.,;:]/g, " ") + " ";
  for (const modality of ["shall not", "shall", "should", "may"]) {
    if (text.includes(" " + modality + " ")) {
      return modality;
    }
  }
  return null;
}
