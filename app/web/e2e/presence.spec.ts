import { test, expect } from "@playwright/test";

/**
 * Phase B — Presence page smoke test.
 *
 * Navigates to /presence and verifies the page header renders
 * without console errors. Does not require the WebSocket or backend to be
 * live — the page renders in a graceful disconnected state.
 */
test("Presence page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/presence");

  // Page header title is the most stable structural element.
  await expect(page.getByRole("heading", { name: "Presence" })).toBeVisible();

  // The Connect to Deck button appears in both the header and the empty-state
  // card — use .first() to avoid strict-mode violation.
  await expect(
    page.getByRole("button", { name: /connect to deck/i }).first()
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /presence: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Presence page shows stats grid", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/presence");

  // Stats grid labels are rendered inside CardDescription elements.
  // Scope to the stats grid container to avoid matching sidebar/tab text.
  const statsGrid = page.locator(".grid.gap-4.md\\:grid-cols-4");
  await expect(statsGrid.getByText("Sessions")).toBeVisible();
  await expect(statsGrid.getByText("Active")).toBeVisible();
  await expect(statsGrid.getByText("Errors")).toBeVisible();
  await expect(statsGrid.getByText("Total Events")).toBeVisible();

  expect(
    consoleErrors,
    `Console errors on /presence: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});
