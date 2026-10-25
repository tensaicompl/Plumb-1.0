// Fable #268 (H0.11): the UI and its tests use only error codes the server can produce. Every
// quoted error-code literal under src/ is one of the catalogued codes of the contract
// (info.x-plumb-error-codes of api/modeller.openapi.json) or one of the two client-side codes of
// writes that never got an HTTP response; a mock can no longer invent a code.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { TRANSPORT_FAILED, VIEW_NOT_LOADED } from "./useVersionedMutation";

// Vitest runs from the ui/ package root.
const SRC = join(process.cwd(), "src");
const CONTRACT = join(process.cwd(), "..", "api", "modeller.openapi.json");

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      return sources(path);
    }
    return /\.(ts|tsx)$/.test(entry.name) && !entry.name.endsWith(".d.ts") ? [path] : [];
  });
}

describe("error codes", () => {
  it("every error code the UI or a test names is catalogued by the contract or client-side", () => {
    const contract = JSON.parse(readFileSync(CONTRACT, "utf8")) as { info: { "x-plumb-error-codes": { code: string }[] } };
    const catalogued = new Set(contract.info["x-plumb-error-codes"].map((entry) => entry.code));
    expect(catalogued.size).toBeGreaterThan(50);
    const known = new Set([...catalogued, TRANSPORT_FAILED, VIEW_NOT_LOADED]);
    const literal = new RegExp("[\"'`](" + "E" + "_[A-Z0-9_]+)[\"'`]", "g");
    const unknown: string[] = [];
    for (const file of sources(SRC)) {
      for (const match of readFileSync(file, "utf8").matchAll(literal)) {
        const code = match[1] ?? "";
        if (!known.has(code)) {
          unknown.push(file.slice(SRC.length) + ": " + code);
        }
      }
    }
    expect(unknown).toEqual([]);
  });
});
