import { test, expect } from "@playwright/test";

/**
 * Permissions page smoke — Phase B page port.
 *
 * Navigates to /permissions and asserts that:
 * 1. The page heading "Permissions" is visible.
 * 2. No console errors are emitted during load.
 * 3. The rule type tabs (Allow / Ask / Deny) are present.
 * 4. The Permission Settings card is visible.
 *
 * Rule counts depend on backend state and are not asserted here.
 */
test("Permissions page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/permissions");

  // Page heading must be visible
  await expect(
    page.getByRole("heading", { name: "Permissions" })
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /permissions load: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Permissions page shows rule tabs", async ({ page }) => {
  await page.goto("/permissions");

  // All three rule type tabs must be present
  await expect(page.getByRole("tab", { name: /allow/i })).toBeVisible();
  await expect(page.getByRole("tab", { name: /ask/i })).toBeVisible();
  await expect(page.getByRole("tab", { name: /deny/i })).toBeVisible();
});

test("Permissions page shows settings card", async ({ page }) => {
  await page.goto("/permissions");

  // Permission Settings heading
  await expect(
    page.getByRole("heading", { name: /permission settings/i })
  ).toBeVisible();
});

test("Permissions page Add Allow Rule button opens rule builder", async ({
  page,
}) => {
  await page.goto("/permissions");

  // Click the Add Allow Rule button
  await page.getByRole("button", { name: /add allow rule/i }).click();

  // Rule builder dialog should appear
  await expect(
    page.getByRole("dialog").getByRole("heading", { name: /create permission rule/i })
  ).toBeVisible();
});
