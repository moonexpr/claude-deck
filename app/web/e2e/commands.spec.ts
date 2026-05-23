import { test, expect } from "@playwright/test";

/**
 * Commands page smoke — Phase B page port.
 *
 * Navigates to /commands and asserts that the page mounts without console
 * errors and the PageHeader h1 is visible.
 */
test("Commands page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/commands");

  // PageHeader h1 is the most stable structural element.
  await expect(
    page.getByRole("heading", { name: "Slash Commands" })
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /commands load: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});
