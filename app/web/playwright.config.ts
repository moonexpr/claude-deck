import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright configuration for app/web e2e smoke tests.
 *
 * webServer: builds the app then serves via `vite preview` (port 4173).
 * This makes `npx playwright test` fully self-contained — no running
 * server required.
 */
export default defineConfig({
  testDir: "e2e",
  // Fail fast on CI; allow retries locally
  retries: process.env.CI ? 2 : 0,
  // One worker keeps the webServer simple
  workers: 1,
  reporter: "list",
  use: {
    baseURL: "http://localhost:4173",
    // Collect console messages so nav.spec.ts can assert no errors
    trace: "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    // Build the app then serve with vite preview (port 4173)
    command: "npm run build && npm run preview",
    url: "http://localhost:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
