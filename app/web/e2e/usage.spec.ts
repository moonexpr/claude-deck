import { test, expect } from "@playwright/test";

/**
 * Usage page smoke tests — Phase B gate.
 *
 * Navigates to /usage and asserts that the page shell renders correctly,
 * the PageHeader title is visible, and the tab navigation is present.
 * API responses are not mocked — the page must gracefully handle empty or
 * error states from the backend.
 *
 * Note: CardTitle components render as <div>, not heading elements, so
 * "Filter" and "Export" section labels are not addressable via
 * getByRole("heading"). Assert their presence via text or child elements.
 */
test("Usage page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/usage");

  // The PageHeader title must be present
  await expect(
    page.getByRole("heading", { name: "Usage Tracking" })
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /usage: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Usage page tab navigation is present", async ({ page }) => {
  await page.goto("/usage");

  // Wait for the page to settle
  await expect(
    page.getByRole("heading", { name: "Usage Tracking" })
  ).toBeVisible();

  // All five tabs must be rendered
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Daily" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Sessions" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Monthly" })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Blocks/ })).toBeVisible();
});

test("Usage page project filter select is present", async ({ page }) => {
  await page.goto("/usage");

  await expect(
    page.getByRole("heading", { name: "Usage Tracking" })
  ).toBeVisible();

  // The project filter renders a Select; verify it is present via its trigger.
  // "All Projects" is the default SelectItem text when no project is active.
  await expect(
    page.getByRole("combobox").first()
  ).toBeVisible();
});

test("Usage page Export section is present", async ({ page }) => {
  await page.goto("/usage");

  await expect(
    page.getByRole("heading", { name: "Usage Tracking" })
  ).toBeVisible();

  // ExportCard renders a Download button regardless of API state.
  await expect(
    page.getByRole("button", { name: /Download/ })
  ).toBeVisible();
});
