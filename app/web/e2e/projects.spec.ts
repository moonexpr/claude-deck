import { test, expect } from "@playwright/test";

/**
 * Phase B — Projects page smoke test.
 *
 * Navigates to /projects and verifies the page structure renders
 * without console errors.
 */
test("Projects page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/projects");

  // Page header title is the most stable structural element.
  await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /projects: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Projects page shows discovery toggle button", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/projects");

  // Discover Projects button should be present
  await expect(
    page.getByRole("button", { name: /discover projects/i })
  ).toBeVisible();

  expect(
    consoleErrors,
    `Console errors on /projects: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Projects page shows tracked projects card", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/projects");

  await expect(page.getByText("Tracked Projects")).toBeVisible();

  expect(
    consoleErrors,
    `Console errors on /projects: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});
