import { defineConfig } from "@playwright/test";

// Processes (seeded backend and Vite) are started by scripts/run-pilot-e2e.sh (E0.3), never here.
export default defineConfig({
  testDir: "./e2e",
  forbidOnly: true,
  retries: 0,
  use: {
    baseURL: "http://127.0.0.1:5173",
  },
});
