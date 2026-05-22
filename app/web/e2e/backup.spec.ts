import { test, expect } from "@playwright/test";

/**
 * Backup & Restore page smoke — Phase B page port.
 *
 * Navigates to /backup and asserts that:
 * 1. The page heading "Backup & Restore" is visible.
 * 2. No console errors are emitted during load.
 * 3. The "Create Backup" button is present and interactive.
 *
 * The backup list depends on a live backend and may be empty; the empty
 * state ("No backups found") is the expected fallback.
 */
test("Backup page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/backup");

  // Page heading must be visible
  await expect(
    page.getByRole("heading", { name: "Backup & Restore" })
  ).toBeVisible();

  // Create Backup button is present
  await expect(
    page.getByRole("button", { name: /create backup/i })
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /backup load: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Backup page shows empty state or backup list after load", async ({
  page,
}) => {
  await page.goto("/backup");

  // Heading persists after data load
  await expect(
    page.getByRole("heading", { name: "Backup & Restore" })
  ).toBeVisible();

  // Stats cards are present (Available Backups card heading)
  await expect(
    page.getByRole("heading", { name: /available backups/i })
  ).toBeVisible();
});

test("Backup page Create Backup button opens wizard", async ({ page }) => {
  await page.goto("/backup");

  await page.getByRole("button", { name: /create backup/i }).click();

  // Wizard dialog should appear with "Create Backup" title
  await expect(
    page.getByRole("dialog").getByRole("heading", { name: /create backup/i })
  ).toBeVisible();
});
