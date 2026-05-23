import { test, expect } from "@playwright/test";

/**
 * Memory page smoke — Phase B page port.
 *
 * Navigates to /memory and asserts that the page mounts without console
 * errors and the PageHeader h1 is visible.
 */
test("Memory page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/memory");

  // PageHeader h1 is the most stable structural element.
  await expect(
    page.getByRole("heading", { name: "Memory" })
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /memory load: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});
