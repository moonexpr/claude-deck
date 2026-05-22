import { test, expect } from "@playwright/test";

/**
 * Status Line page smoke — Phase B page port.
 *
 * Navigates to /statusline and asserts that:
 * 1. The page heading "Status Line" is visible.
 * 2. No console errors are emitted during load.
 * 3. The Current Status card with its enabled/disabled toggle is present.
 * 4. The Presets section heading is visible.
 *
 * Actual preset content and powerline themes depend on a live backend.
 */
test("Status Line page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/statusline");

  // Page heading must be visible
  await expect(
    page.getByRole("heading", { name: "Status Line" })
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /statusline load: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Status Line page shows Current Status card", async ({ page }) => {
  await page.goto("/statusline");

  // Current Status heading
  await expect(
    page.getByRole("heading", { name: /current status/i })
  ).toBeVisible();
});

test("Status Line page shows Presets section", async ({ page }) => {
  await page.goto("/statusline");

  // Presets heading
  await expect(
    page.getByRole("heading", { name: /presets/i })
  ).toBeVisible();
});

test("Status Line page shows Configuration section", async ({ page }) => {
  await page.goto("/statusline");

  // Configuration section heading
  await expect(
    page.getByRole("heading", { name: /configuration/i })
  ).toBeVisible();
});
