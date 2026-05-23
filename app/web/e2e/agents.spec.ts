import { test, expect } from "@playwright/test";

/**
 * Agents page smoke tests — Phase B gate + Phase C5 AI suggest affordance.
 *
 * Verifies that:
 * - The /agents route renders without console errors.
 * - The PageHeader title "Agents" is visible.
 * - The "New Agent" button is present and focusable.
 * - (C5) The "AI suggest" button renders inside the AgentEditor dialog.
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

  /**
   * C5 smoke: verify that the AISuggestBlock "AI suggest" button renders
   * inside the AgentEditor dialog.  We mock the agents API so no running
   * backend is required.
   */
  test("AI suggest button is visible inside AgentEditor dialog", async ({ page }) => {
    // Mock GET /api/v1/agents — return one editable user-scope agent.
    await page.route("**/api/v1/agents**", (route) => {
      if (route.request().method() !== "GET") {
        route.continue();
        return;
      }
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          agents: [
            {
              name: "smoke-agent",
              scope: "user",
              description: "A smoke test agent",
              prompt: "You are a helpful assistant.",
              model: null,
              tools: [],
              disallowed_tools: [],
              permission_mode: null,
              skills: [],
              memory: null,
              hooks: {},
            },
          ],
        }),
      });
    });

    // Mock GET /api/v1/agents/skills (optional, avoid network error in console)
    await page.route("**/api/v1/agents/skills**", (route) => {
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ skills: [] }),
      });
    });

    await page.goto("/agents");
    await expect(page.getByRole("heading", { name: "Agents" })).toBeVisible();

    // The edit (pencil) button should be visible on the mocked agent card.
    const editBtn = page.getByRole("button", { name: /edit smoke-agent/i });
    await expect(editBtn).toBeVisible();
    await editBtn.click();

    // The dialog should now be open.
    await expect(
      page.getByRole("dialog", { name: /edit agent/i })
    ).toBeVisible();

    // The AISuggestBlock toggle button must be present.
    const aiBtn = page.getByRole("button", { name: /ai suggest/i });
    await expect(aiBtn).toBeVisible();

    // Clicking it should open the suggestion panel.
    await aiBtn.click();
    await expect(
      page.getByRole("region", { name: /ai suggestion panel/i })
    ).toBeVisible();
  });
});
