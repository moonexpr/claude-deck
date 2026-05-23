import { test, expect } from "@playwright/test";

/**
 * A8 nav smoke — Phase A gate.
 *
 * Navigates to "/" and asserts that the MainLayout shell has rendered
 * (the sidebar brand mark "Claude Deck" is visible) and that no console
 * errors were emitted during page load.
 *
 * One test only — the full per-page sweep is Phase B.
 */
test("MainLayout shell renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/");

  // The sidebar brand mark is the most stable structural element in MainLayout.
  // It is always present regardless of active project or API state.
  await expect(
    page.getByText("Claude Deck", { exact: true })
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on load: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});
