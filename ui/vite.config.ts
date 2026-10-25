import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vitest/config";

// Development transport only: the UI always calls relative /api paths, which the Vite dev
// server forwards (including the /api/events WebSocket) to the local backend.
const apiTarget = process.env["PLUMB_API_TARGET"] ?? "http://127.0.0.1:8080";

export default defineConfig({
  plugins: [tailwindcss()],
  server: {
    proxy: {
      "/api": {
        target: apiTarget,
        ws: true,
      },
    },
  },
  test: {
    environment: "jsdom",
    // Playwright specs (e2e/) run against the real backend through scripts/run-pilot-e2e.sh.
    exclude: ["node_modules/**", "e2e/**"],
    // U0.1 owns no unit-test file; once tests exist, failing tests still fail.
    passWithNoTests: true,
  },
});
