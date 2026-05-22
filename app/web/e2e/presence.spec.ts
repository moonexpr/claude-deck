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

  // The WebSocket indicator or Connect button should be present.
  await expect(
    page.getByRole("button", { name: /connect to deck/i })
  ).toBeVisible();

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on /presence: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("Presence page shows empty state when no sessions", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/presence");

  // Stats grid should be present with four stat cards
  await expect(page.getByText("Sessions")).toBeVisible();
  await expect(page.getByText("Active")).toBeVisible();
  await expect(page.getByText("Errors")).toBeVisible();
  await expect(page.getByText("Total Events")).toBeVisible();

  expect(
    consoleErrors,
    `Console errors on /presence: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});
