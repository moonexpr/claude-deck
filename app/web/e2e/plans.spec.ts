import { test, expect } from "@playwright/test";

/**
 * Phase B — Plans page smoke test.
 *
 * Navigates to /plans and verifies the page structure renders
 * without console errors.
 *
 * The detail route (/plans/:filename) is not tested here: any synthetic
 * filename causes a backend 404 that generates a console error. Smoke
 * coverage of the list route is sufficient.
 */
test("Plans page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/plans");

  // Page header title is the most stable structural element.
  await expect(page.getByRole("heading", { name: "Plans" })).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /plans: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Plans page shows search input", async ({ page }) => {
  await page.goto("/plans");

  // Search input should always be rendered
  await expect(
    page.getByPlaceholder(/search plans/i)
  ).toBeVisible();
});
