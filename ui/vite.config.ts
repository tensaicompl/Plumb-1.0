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
    // jsdom shims for browser APIs Radix and the shell use (ResizeObserver, matchMedia...).
    setupFiles: ["src/test/setup.ts"],
    // U0.1 owns no unit-test file; once tests exist, failing tests still fail.
    passWithNoTests: true,
    // E0.4: `npm run test:coverage` (make ui-cov) enforces >= 80% on the feature screens and
    // shared components; plain `npm test` does not collect coverage.
    coverage: {
      provider: "v8",
      include: ["src/features/**", "src/components/**"],
      reporter: ["text-summary", "json-summary"],
      thresholds: {
        lines: 80,
        statements: 80,
        functions: 80,
        branches: 80,
      },
    },
  },
});
