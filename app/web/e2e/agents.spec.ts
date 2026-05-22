import { test, expect } from "@playwright/test";

/**
 * Agents page smoke tests — Phase B gate.
 *
 * Verifies that:
 * - The /agents route renders without console errors.
 * - The PageHeader title "Agents" is visible.
 * - The "New Agent" button is present and focusable.
 */
test.describe("Agents page", () => {
  test("renders without console errors", async ({ page }) => {
    const consoleErrors: string[] = [];

    page.on("console", (msg) => {
      if (msg.type() === "error") {
        consoleErrors.push(msg.text());
      }
    });

    await page.goto("/agents");

    // The PageHeader title is the most stable structural element.
    await expect(page.getByRole("heading", { name: "Agents" })).toBeVisible();

    expect(
      consoleErrors,
      `Console errors on /agents: ${consoleErrors.join("; ")}`
    ).toHaveLength(0);
  });

  test("New Agent button is visible and keyboard-accessible", async ({ page }) => {
    await page.goto("/agents");

    const newAgentBtn = page.getByRole("button", { name: /new agent/i });
    await expect(newAgentBtn).toBeVisible();

    // Keyboard accessibility — tab to it and confirm focused
    await newAgentBtn.focus();
    await expect(newAgentBtn).toBeFocused();
  });

  test("navigating to /agents from sidebar works", async ({ page }) => {
    await page.goto("/");

    // The sidebar link for Agents should be present
    const agentsLink = page.getByRole("link", { name: /agents/i });
    await expect(agentsLink).toBeVisible();

    await agentsLink.click();
    await expect(page).toHaveURL(/\/agents/);
    await expect(page.getByRole("heading", { name: "Agents" })).toBeVisible();
  });
});
