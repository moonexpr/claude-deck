import { test, expect } from "@playwright/test";

/**
 * Phase B smoke tests — Skills page.
 *
 * Verifies the page loads, the route is registered, and the primary
 * UI elements are present. Does not exercise API-dependent behavior
 * (mocking is left for the full integration sweep).
 */
test("Skills page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/skills");

  // The PageHeader title is the most stable structural element.
  await expect(page.getByRole("heading", { name: "Skills" })).toBeVisible();

  // The Installed tab should be visible and active by default.
  await expect(page.getByRole("tab", { name: /Installed/i })).toBeVisible();

  // The Discover tab should be present.
  await expect(page.getByRole("tab", { name: /Discover/i })).toBeVisible();

  expect(
    consoleErrors,
    `Console errors on /skills: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Skills page Discover tab is navigable", async ({ page }) => {
  await page.goto("/skills");

  // Click the Discover tab
  await page.getByRole("tab", { name: /Discover/i }).click();

  // Search input should appear in the Discover tab
  await expect(
    page.getByPlaceholder(/Search skills/i)
  ).toBeVisible();
});
