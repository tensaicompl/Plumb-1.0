import createClient from "openapi-fetch";

import type { paths } from "./schema";

// The typed API client of contract v3, generated from api/modeller.openapi.json. Paths are
// relative (/api/...): the browser origin serves them (the Vite dev proxy in development).
export const apiClient = createClient<paths>();

export type ApiClient = typeof apiClient;
