import { test, expect } from "@playwright/test";

/**
 * Phase B smoke tests — Output Styles page.
 *
 * Verifies the page loads, the route is registered, and the primary
 * UI elements are present. Does not exercise API-dependent behavior
 * (mocking is left for the full integration sweep).
 */
test("Output Styles page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/output-styles");

  // The PageHeader title is the most stable structural element.
  await expect(
    page.getByRole("heading", { name: "Output Styles" })
  ).toBeVisible();

  // The "New Style" button must be present.
  await expect(
    page.getByRole("button", { name: /New Style/i })
  ).toBeVisible();

  expect(
    consoleErrors,
    `Console errors on /output-styles: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Output Styles wizard opens when New Style is clicked", async ({ page }) => {
  await page.goto("/output-styles");

  await page.getByRole("button", { name: /New Style/i }).click();

  // The wizard dialog should appear with step 1 content.
  await expect(
    page.getByRole("dialog", { name: /Create New Output Style/i })
  ).toBeVisible();

  // Step 1 has a Style Name input.
  await expect(page.getByLabel("Style Name")).toBeVisible();
});
