import type { RequestHandler } from "msw";
import { setupServer } from "msw/node";

// Test-only request interception for component tests. Feature tests register their own
// handlers (server.use); nothing here starts a service worker or performs network I/O.
export const handlers: RequestHandler[] = [];

export const server = setupServer(...handlers);
