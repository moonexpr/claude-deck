import { test, expect } from "@playwright/test";

/**
 * Hooks page smoke — Phase B gate.
 *
 * Navigates to /hooks and verifies the page shell renders without console
 * errors. Does not exercise API calls — the backend may not be running.
 */
test("Hooks page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/hooks");

  // The page header title is the most stable element in HooksPage.
  await expect(
    page.getByRole("heading", { name: "Hooks" })
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /hooks: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Hooks page Add Hook button is present", async ({ page }) => {
  await page.goto("/hooks");

  await expect(
    page.getByRole("heading", { name: "Hooks" })
  ).toBeVisible();

  // The Add Hook button should be rendered in the page header.
  await expect(
    page.getByRole("button", { name: /Add Hook/i })
  ).toBeVisible();
});

test("Hooks page event tabs are rendered", async ({ page }) => {
  await page.goto("/hooks");

  await expect(
    page.getByRole("heading", { name: "Hooks" })
  ).toBeVisible();

  // The hooks-by-event section heading should be present.
  await expect(
    page.getByText("Hooks by Event Type")
  ).toBeVisible();
});
