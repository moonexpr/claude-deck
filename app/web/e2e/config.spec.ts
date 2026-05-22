import { test, expect } from "@playwright/test";

/**
 * Phase B smoke — Config page.
 *
 * Navigates to "/config" and asserts that the page shell has rendered
 * (the "Configuration" heading is visible via PageHeader) and that the
 * three main tabs are present. No API calls are validated here; the
 * goal is structural correctness of the route registration and component
 * mount.
 */
test("Config page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/config");

  // PageHeader title
  await expect(page.getByText("Configuration", { exact: true })).toBeVisible();

  // The three tab triggers must be present
  await expect(page.getByRole("tab", { name: "Settings Editor" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Scope Resolver" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Raw Viewer" })).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on load: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});
