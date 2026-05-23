import { test, expect, type Page } from "@playwright/test";

/**
 * Chat page smoke tests — Phase C3b gate.
 *
 * Verifies that:
 * - The /chat route renders without console errors originating from the chat
 *   feature itself.
 * - The "New Chat" button is present in the sidebar.
 * - Clicking "New Chat" (with the CRUD endpoint stubbed) adds a conversation
 *   to the sidebar.
 *
 * Streaming is NOT tested here — that requires a real Anthropic key.
 * All /api/v1/* endpoints are stubbed with route.fulfill so the tests are
 * fully self-contained (no running backend required).
 */

/** Stub the shell's background API calls that always fire on page load. */
async function stubShellApis(page: Page) {
  await page.route("**/api/v1/status", (r) =>
    r.fulfill({ status: 200, contentType: "application/json", body: "{}" })
  );
  await page.route("**/api/v1/projects", (r) =>
    r.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ projects: [] }),
    })
  );
}

/** Stub GET /api/v1/chat → empty conversation list. */
async function stubChatList(page: Page, conversations: unknown[] = []) {
  await page.route("**/api/v1/chat", async (route) => {
    if (route.request().method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ conversations }),
      });
    } else {
      await route.continue();
    }
  });
}

test.describe("Chat page", () => {
  test("renders without console errors", async ({ page }) => {
    const consoleErrors: string[] = [];

    page.on("console", (msg) => {
      if (msg.type() === "error") {
        consoleErrors.push(msg.text());
      }
    });

    await stubShellApis(page);
    await stubChatList(page);

    await page.goto("/chat");

    await expect(page.getByRole("heading", { name: "Chat" })).toBeVisible();

    expect(
      consoleErrors,
      `Console errors on /chat: ${consoleErrors.join("; ")}`
    ).toHaveLength(0);
  });

  test("New Chat button is visible", async ({ page }) => {
    await stubShellApis(page);
    await stubChatList(page);

    await page.goto("/chat");

    const newChatBtn = page.getByRole("button", { name: /new chat/i });
    await expect(newChatBtn).toBeVisible();
  });

  test("clicking New Chat adds a conversation to the sidebar", async ({
    page,
  }) => {
    const now = new Date().toISOString();
    const stubConv = {
      id: 1,
      title: "New Chat",
      messages: [],
      created_at: now,
      updated_at: now,
    };

    await stubShellApis(page);

    // Route handler: GET → empty list; POST → new conversation.
    await page.route("**/api/v1/chat", async (route) => {
      if (route.request().method() === "GET") {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ conversations: [] }),
        });
      } else if (route.request().method() === "POST") {
        await route.fulfill({
          status: 201,
          contentType: "application/json",
          body: JSON.stringify(stubConv),
        });
      } else {
        await route.continue();
      }
    });

    await page.goto("/chat");

    // Confirm the sidebar starts empty — use first() since desktop + mobile
    // both render the same sidebar element (one hidden via CSS).
    await expect(
      page.getByText(/no conversations yet/i).first()
    ).toBeVisible();

    // Click New Chat.
    await page.getByRole("button", { name: /new chat/i }).first().click();

    // The new conversation title should appear in the sidebar.
    await expect(page.getByText("New Chat").first()).toBeVisible();
  });

  test("navigating to /chat from sidebar works", async ({ page }) => {
    await stubShellApis(page);
    await stubChatList(page);

    await page.goto("/");

    const chatLink = page.getByRole("link", { name: /^chat$/i });
    await expect(chatLink).toBeVisible();
    await chatLink.click();

    await expect(page).toHaveURL(/\/chat/);
    await expect(page.getByRole("heading", { name: "Chat" })).toBeVisible();
  });
});
