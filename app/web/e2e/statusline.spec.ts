import { test, expect } from "@playwright/test";

/**
 * Status Line page smoke — Phase B page port.
 *
 * Navigates to /statusline and asserts that the page heading "Status Line"
 * is visible and no console errors are emitted during load.
 *
 * Note: CardTitle components render as <div>, not heading elements, so
 * section headings like "Current Status", "Presets", and "Configuration"
 * are not addressable via getByRole("heading"). The smoke floor is the
 * PageHeader h1 plus zero console errors.
 */
test("Status Line page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/statusline");

  // Page heading must be visible
  await expect(
    page.getByRole("heading", { name: "Status Line" })
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /statusline load: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Status Line page shows enabled/disabled toggle", async ({ page }) => {
  await page.goto("/statusline");

  // The Switch for enabling/disabling the status line is always rendered.
  await expect(page.locator("#enabled")).toBeAttached();
});

test("Status Line page shows Presets section text", async ({ page }) => {
  await page.goto("/statusline");

  // "Presets" section label renders as a CardTitle div — match via text.
  await expect(page.getByText("Presets").first()).toBeVisible();
});

test("Status Line page shows Configuration section text", async ({ page }) => {
  await page.goto("/statusline");

  // "Configuration" section label renders as a CardTitle div — match via text.
  await expect(page.getByText("Configuration").first()).toBeVisible();
});
