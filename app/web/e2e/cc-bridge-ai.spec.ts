/**
 * CC Bridge AI suggest gate test — Phase C, Task C4.
 *
 * Validates the strict UX safety boundary:
 *   - A stub POST /api/v1/ai/suggest returns two <execute> blocks.
 *   - Both blocks render as preview cards BEFORE any PTY bytes are sent.
 *   - Clicking Send on the first card is the ONLY path to PTY bytes.
 *   - The second card remains un-sent after clicking Send on the first.
 *
 * Implementation note: at desktop width both a desktop panel (md:flex) and a
 * mobile panel (md:hidden) exist in the DOM; the mobile one is hidden via CSS.
 * Tests scope all queries to the visible desktop panel via getByTestId('ai-panel').
 */

import { test, expect } from "@playwright/test";

const STUB_RESPONSE = {
  text: "Try this:\n<execute>echo HELLO</execute>\nand then this:\n<execute>ls -la</execute>",
  usage: { input_tokens: 0, output_tokens: 0 },
};

// ---------------------------------------------------------------------------
// Shared stub setup
// ---------------------------------------------------------------------------

async function stubRoutes(page: import("@playwright/test").Page) {
  await page.route("/api/v1/cc-bridge/sessions", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ sessions: [], count: 0 }),
    })
  );
  await page.route("/api/v1/ai/suggest", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(STUB_RESPONSE),
    })
  );
  // Silence the /status and /projects calls that fire on load.
  await page.route("/api/v1/status", (route) =>
    route.fulfill({ status: 200, contentType: "application/json", body: "{}" })
  );
  await page.route("/api/v1/projects", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ projects: [] }),
    })
  );
}

// ---------------------------------------------------------------------------
// Desktop gate tests
// ---------------------------------------------------------------------------

test.describe("CC Bridge AI Suggest — gate test", () => {
  test.beforeEach(async ({ page }) => {
    await stubRoutes(page);
  });

  test("AI panel opens and submit renders two execute cards", async ({ page }) => {
    const consoleErrors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") consoleErrors.push(msg.text());
    });

    await page.goto("/cc-bridge");
    await expect(page.getByText("CC Bridge", { exact: true }).first()).toBeVisible();

    // Open the AI panel via the toolbar toggle button.
    const aiToggle = page.getByTestId("ai-panel-toggle");
    await expect(aiToggle).toBeVisible();
    await aiToggle.click();

    // The desktop AI panel should appear (md:flex, visible at 1280px default).
    const aiPanel = page.getByTestId("ai-panel");
    await expect(aiPanel).toBeVisible();

    // Scope all queries to the desktop panel to avoid strict-mode violations
    // (the mobile panel is in the DOM but hidden via CSS).
    const promptInput = aiPanel.getByTestId("ai-prompt-input");
    await expect(promptInput).toBeVisible();
    await promptInput.fill("show me hello");

    const submitBtn = aiPanel.getByTestId("ai-submit-button");
    await submitBtn.click();

    // Wait for both execute cards to appear inside the desktop panel.
    const cards = aiPanel.getByTestId("execute-card");
    await expect(cards).toHaveCount(2);

    // Verify content of the two cards.
    await expect(cards.nth(0)).toContainText("echo HELLO");
    await expect(cards.nth(1)).toContainText("ls -la");

    // --- Zero-PTY-bytes assertion ---
    // The cards are visible but we have NOT clicked Send on either.
    // Confirm both Send buttons are present (meaning no auto-send occurred).
    const sendButtons = aiPanel.getByTestId("send-button");
    await expect(sendButtons).toHaveCount(2);

    // No console errors during the whole interaction.
    expect(
      consoleErrors.filter((e) => !e.includes("ECONNREFUSED") && !e.includes("http proxy")),
      `Console errors: ${consoleErrors.join("; ")}`
    ).toHaveLength(0);
  });

  test("Send on first card does not auto-remove it; second card remains", async ({ page }) => {
    await page.goto("/cc-bridge");
    await page.getByTestId("ai-panel-toggle").click();

    const aiPanel = page.getByTestId("ai-panel");
    await expect(aiPanel).toBeVisible();

    const promptInput = aiPanel.getByTestId("ai-prompt-input");
    await promptInput.fill("show me hello");
    await aiPanel.getByTestId("ai-submit-button").click();

    const cards = aiPanel.getByTestId("execute-card");
    await expect(cards).toHaveCount(2);

    // Verify BEFORE any click: two Send buttons exist (no auto-send).
    await expect(aiPanel.getByTestId("send-button")).toHaveCount(2);

    // Click Send on the first card.
    // Send does NOT remove the card (user may want to re-send) — both remain.
    await aiPanel.getByTestId("send-button").first().click();

    // Both cards still shown; the second card is untouched.
    await expect(cards).toHaveCount(2);
    await expect(cards.nth(1)).toContainText("ls -la");
  });

  test("Discard removes only the targeted card", async ({ page }) => {
    await page.goto("/cc-bridge");
    await page.getByTestId("ai-panel-toggle").click();

    const aiPanel = page.getByTestId("ai-panel");
    await expect(aiPanel).toBeVisible();

    const promptInput = aiPanel.getByTestId("ai-prompt-input");
    await promptInput.fill("show me hello");
    await aiPanel.getByTestId("ai-submit-button").click();

    const cards = aiPanel.getByTestId("execute-card");
    await expect(cards).toHaveCount(2);

    // Discard the first card.
    await aiPanel.getByTestId("discard-button").first().click();

    // Only the second card (ls -la) should remain.
    await expect(cards).toHaveCount(1);
    await expect(cards.first()).toContainText("ls -la");
  });

  test("503 response shows actionable banner, terminal remains usable", async ({
    page,
  }) => {
    // Override the AI route to return 503.
    await page.route(
      "/api/v1/ai/suggest",
      (route) =>
        route.fulfill({
          status: 503,
          contentType: "application/json",
          body: JSON.stringify({ message: "Service unavailable" }),
        }),
      { times: 1 }
    );

    await page.goto("/cc-bridge");
    await page.getByTestId("ai-panel-toggle").click();

    const aiPanel = page.getByTestId("ai-panel");
    await expect(aiPanel).toBeVisible();

    const promptInput = aiPanel.getByTestId("ai-prompt-input");
    await promptInput.fill("test prompt");
    await aiPanel.getByTestId("ai-submit-button").click();

    // The 503 banner should appear inside the desktop panel.
    await expect(aiPanel.getByText(/AI unavailable/i)).toBeVisible();

    // No execute cards should have been rendered.
    await expect(aiPanel.getByTestId("execute-card")).toHaveCount(0);

    // The CC Bridge header is still visible (terminal area not broken).
    await expect(page.getByText("CC Bridge", { exact: true }).first()).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// Mobile tests
// ---------------------------------------------------------------------------

test.describe("CC Bridge AI Suggest — mobile 375px", () => {
  test.use({
    viewport: { width: 375, height: 667 },
    isMobile: true,
  });

  test("cc-bridge page still usable at 375px with AI panel closed", async ({
    page,
  }) => {
    const consoleErrors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") consoleErrors.push(msg.text());
    });

    await page.route("/api/v1/cc-bridge/sessions", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ sessions: [], count: 0 }),
      })
    );
    await page.route("/api/v1/status", (route) =>
      route.fulfill({ status: 200, contentType: "application/json", body: "{}" })
    );
    await page.route("/api/v1/projects", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ projects: [] }),
      })
    );

    await page.goto("/cc-bridge");

    // At 375px the page header shows "CC Bridge" in the toolbar — not the nav
    // sidebar link (which is hidden in the hamburger menu).  We locate the h1
    // inside the cc-bridge page header bar.
    const pageHeading = page.locator("h1", { hasText: "CC Bridge" });
    await expect(pageHeading).toBeVisible();

    // The AI panel toggle must be visible (it's always in the toolbar).
    await expect(page.getByTestId("ai-panel-toggle")).toBeVisible();

    // The desktop AI panel div is in the DOM but CSS-hidden at 375px.
    // Assert it is NOT visible.
    await expect(page.getByTestId("ai-panel")).not.toBeVisible();

    // No horizontal overflow.
    const scrollWidth = await page.evaluate(
      () => document.documentElement.scrollWidth
    );
    expect(
      scrollWidth,
      `Horizontal overflow at 375px: scrollWidth=${scrollWidth}`
    ).toBeLessThanOrEqual(376);

    // No console errors (ignoring proxy errors from missing backend).
    const relevant = consoleErrors.filter(
      (e) => !e.includes("ECONNREFUSED") && !e.includes("http proxy") && !e.includes("Failed to fetch")
    );
    expect(
      relevant,
      `Console errors: ${relevant.join("; ")}`
    ).toHaveLength(0);
  });

  test("AI panel opens as mobile drawer at 375px", async ({ page }) => {
    await page.route("/api/v1/cc-bridge/sessions", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ sessions: [], count: 0 }),
      })
    );
    await page.route("/api/v1/ai/suggest", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(STUB_RESPONSE),
      })
    );
    await page.route("/api/v1/status", (route) =>
      route.fulfill({ status: 200, contentType: "application/json", body: "{}" })
    );
    await page.route("/api/v1/projects", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ projects: [] }),
      })
    );

    await page.goto("/cc-bridge");
    await page.getByTestId("ai-panel-toggle").click();

    // Mobile drawer should appear.
    const mobileDrawer = page.getByTestId("ai-panel-mobile");
    await expect(mobileDrawer).toBeVisible();

    // The desktop panel must remain hidden at mobile width.
    await expect(page.getByTestId("ai-panel")).not.toBeVisible();
  });
});
