import { test, expect } from "@playwright/test";

/**
 * Plugins page smoke tests — Phase B gate.
 *
 * Verifies that:
 * - The /plugins route renders without console errors.
 * - The PageHeader title "Plugins" is visible.
 * - The three tab triggers (Installed, Marketplace, All Available) are present.
 * - The Refresh and Manage Marketplaces buttons are present.
 */
test.describe("Plugins page", () => {
  test("renders without console errors", async ({ page }) => {
    const consoleErrors: string[] = [];

    page.on("console", (msg) => {
      if (msg.type() === "error") {
        consoleErrors.push(msg.text());
      }
    });

    await page.goto("/plugins");

    // The PageHeader title is the most stable structural element.
    await expect(page.getByRole("heading", { name: "Plugins" })).toBeVisible();

    expect(
      consoleErrors,
      `Console errors on /plugins: ${consoleErrors.join("; ")}`
    ).toHaveLength(0);
  });

  test("tab navigation renders all three tabs", async ({ page }) => {
    await page.goto("/plugins");

    await expect(page.getByRole("tab", { name: /installed/i })).toBeVisible();
    await expect(page.getByRole("tab", { name: /marketplace/i })).toBeVisible();
    await expect(page.getByRole("tab", { name: /all available/i })).toBeVisible();
  });

  test("Refresh and Manage Marketplaces buttons are present", async ({ page }) => {
    await page.goto("/plugins");

    await expect(page.getByRole("button", { name: /refresh/i })).toBeVisible();
    await expect(page.getByRole("button", { name: /manage marketplaces/i })).toBeVisible();
  });

  test("switching to Marketplace tab works", async ({ page }) => {
    await page.goto("/plugins");

    const marketplaceTab = page.getByRole("tab", { name: /marketplace/i });
    await marketplaceTab.click();

    // After clicking, the tab should be selected
    await expect(marketplaceTab).toHaveAttribute("data-state", "active");
  });

  test("navigating to /plugins from sidebar works", async ({ page }) => {
    await page.goto("/");

    const pluginsLink = page.getByRole("link", { name: /plugins/i });
    await expect(pluginsLink).toBeVisible();

    await pluginsLink.click();
    await expect(page).toHaveURL(/\/plugins/);
    await expect(page.getByRole("heading", { name: "Plugins" })).toBeVisible();
  });
});
