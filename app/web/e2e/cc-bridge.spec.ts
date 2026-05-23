import { test, expect } from "@playwright/test";

/**
 * CC Bridge smoke test — Phase B gate.
 *
 * Verifies that:
 *   1. The CC Bridge route is reachable and renders without JS errors.
 *   2. The page header ("CC Bridge") is visible.
 *   3. The session list sidebar renders (sessions panel or empty-state message).
 *   4. No console errors are emitted during page load.
 *
 * This test does NOT exercise the WebSocket terminal — that requires a live
 * server with tmux sessions. It only validates the route registration and
 * static shell render.
 */
test("CC Bridge page renders without console errors", async ({ page }) => {
  const consoleErrors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });

  await page.goto("/cc-bridge");

  // The page header is the most stable landmark.
  await expect(page.getByText("CC Bridge", { exact: true }).first()).toBeVisible();

  // The session list or its empty-state should be present.
  // Either the "Sessions" label or the "No CC sessions found" message must appear.
  const sessionListVisible =
    (await page.getByText("Sessions", { exact: false }).count()) > 0 ||
    (await page.getByText("No CC sessions found").count()) > 0;

  expect(sessionListVisible, "Session list or empty-state should be visible").toBe(true);

  // No console errors during load
  expect(
    consoleErrors,
    `Console errors on load: ${consoleErrors.join("; ")}`
  ).toHaveLength(0);
});

test("CC Bridge appears in sidebar navigation", async ({ page }) => {
  await page.goto("/");

  // The sidebar should contain a CC Bridge nav link.
  await expect(page.getByRole("link", { name: /cc bridge/i })).toBeVisible();
});
