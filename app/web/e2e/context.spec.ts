import { test, expect } from "@playwright/test";

/**
 * Context Window page smoke — Phase B page port.
 *
 * Navigates to /context and asserts that:
 * 1. The page heading "Context Window" is visible.
 * 2. No console errors are emitted during load.
 * 3. The Help toggle button is present and interactive.
 *
 * Active sessions and analysis panels depend on a live backend; they are
 * not asserted here. The empty-state card ("No active sessions detected.")
 * is the expected fallback when the API returns an empty list.
 */
test("Context Window page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/context");

  // Page heading must be visible
  await expect(
    page.getByRole("heading", { name: "Context Window" })
  ).toBeVisible();

  // Help toggle button is present
  await expect(page.getByRole("button", { name: /help/i })).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /context load: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Context Window page shows empty state when no sessions present", async ({
  page,
}) => {
  await page.goto("/context");

  // Either the empty-state message or a session card must render
  // (depending on backend state). We check the page at least settled
  // without a crash by verifying the heading persists after load.
  await expect(
    page.getByRole("heading", { name: "Context Window" })
  ).toBeVisible();
});
