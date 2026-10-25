import { http, HttpResponse, type JsonBodyType, type RequestHandler } from "msw";
import { setupServer } from "msw/node";

/** The ETag the fixture backends answer with unless a test sets another (architecture §33.2). */
export const TEST_ETAG = '"rev:1:0000000000000000"';

/** A JSON response carrying `ETag` (TEST_ETAG unless `etag` is given), as every real read does. */
export function etagJson<T extends JsonBodyType>(body: T, init?: ResponseInit, etag: string = TEST_ETAG) {
  const headers = new Headers(init?.headers);
  if (!headers.has("ETag")) {
    headers.set("ETag", etag);
  }
  return HttpResponse.json(body, { ...init, headers });
}

/**
 * The reads the app shell makes on every route: identity (footer, Settings), the model version
 * (version indicator, Run step If-Match) and the work counts behind the Inbox badge and the Next
 * action. Tests that assert which reads a page makes leave these out.
 */
export const SHELL_READS: readonly string[] = ["/api/me", "/api/model/version", "/api/proposals", "/api/questions", "/api/findings", "/api/gates", "/api/pipeline"];

/** The fallback pipeline: one imported source, no recipe run yet and no known next step. */
export const FALLBACK_PIPELINE = { steps: [], sources: 1, open_questions: 0, next: null };

/** Whether a request path is one of the shell's own reads. */
export function isShellRead(path: string): boolean {
  return SHELL_READS.includes(path);
}

// Test-only request interception for component tests. These fallbacks answer the shell's reads
// with an empty project; feature tests register their own handlers (server.use), which take
// precedence. Nothing here starts a service worker or performs network I/O.
export const handlers: RequestHandler[] = [
  http.get("*/api/me", () =>
    HttpResponse.json({
      user: "alice",
      mode: "pilot",
      roles: ["analyst"],
      inference_available: true,
      inference_model: null,
      project: { id: "project:pilot", name: "Pilot", database: null },
    }),
  ),
  http.get("*/api/pipeline", () => etagJson(FALLBACK_PIPELINE)),
  http.get("*/api/model/version", () =>
    etagJson({ version: "rev:1:0000000000000000", revision: "rev:1:0000000000000000", hash: "psg:sha256:0", semantic_hash: "psg:sha256:0", evidence_hash: "ev:sha256:0" }),
  ),
  http.get("*/api/proposals", () => etagJson({ items: [] })),
  http.get("*/api/questions", () => etagJson({ items: [], health: [], next_cursor: null })),
  http.get("*/api/findings", () => etagJson({ items: [], next_cursor: null })),
  http.get("*/api/gates", () => etagJson([])),
];

export const server = setupServer(...handlers);
