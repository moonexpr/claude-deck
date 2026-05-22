import { test, expect } from "@playwright/test";

/**
 * Phase B — Plans page smoke test.
 *
 * Navigates to /plans and verifies the page structure renders
 * without console errors.
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
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/plans");

  // Search input should always be rendered
  await expect(
    page.getByPlaceholder(/search plans/i)
  ).toBeVisible();

  expect(
    consoleErrors,
    `Console errors on /plans: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Plans detail page renders for a given filename param", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  // Navigate to a detail page with a placeholder filename.
  // The backend will 404 but the page should still render the error state
  // without a JS crash.
  await page.goto("/plans/test-plan.md");

  // Back button should always be present regardless of API state
  await expect(
    page.getByRole("button", { name: /back to plans/i })
  ).toBeVisible();

  expect(
    consoleErrors,
    `Console errors on /plans/:filename: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});
