import { HttpResponse, type JsonBodyType, type RequestHandler } from "msw";
import { setupServer } from "msw/node";

// Test-only request interception for component tests. Feature tests register their own
// handlers (server.use); nothing here starts a service worker or performs network I/O.
export const handlers: RequestHandler[] = [];

export const server = setupServer(...handlers);

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
