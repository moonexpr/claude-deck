import { test, expect } from "@playwright/test";

/**
 * MCP Servers page smoke — Phase B gate.
 *
 * Navigates to /mcp and verifies the page shell renders without console
 * errors. Does not exercise API calls — the backend may not be running.
 */
test("MCP Servers page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/mcp");

  // The page header title is the most stable element in MCPServersPage.
  await expect(
    page.getByRole("heading", { name: "MCP Servers" })
  ).toBeVisible();

  // The My Servers / Registry tabs should be visible.
  await expect(page.getByRole("tab", { name: "My Servers" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Registry" })).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /mcp: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("MCP Servers page Add Server button is present", async ({ page }) => {
  await page.goto("/mcp");

  await expect(
    page.getByRole("heading", { name: "MCP Servers" })
  ).toBeVisible();

  // The Add Server button should be rendered in the page header.
  await expect(
    page.getByRole("button", { name: /Add Server/i })
  ).toBeVisible();
});
