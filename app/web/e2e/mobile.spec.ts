import { test, expect } from "@playwright/test";

/**
 * Mobile-responsive layout smoke — Phase B gate (validator 4).
 *
 * Verifies the layout at iPhone-375 viewport width on two pages:
 *   - Dashboard "/"
 *   - Usage "/usage"
 *
 * Assertions per page:
 *   1. Desktop sidebar (<aside class="hidden lg:flex ...">)  is NOT visible.
 *   2. Hamburger menu button (class="lg:hidden") IS visible in the Header.
 *   3. Clicking the hamburger opens the MobileSidebar dialog (nav links appear).
 *   4. No horizontal overflow — scrollWidth is within 1px of the 375 viewport width.
 *   5. No console errors during load.
 */

test.use({
  viewport: { width: 375, height: 667 },
  isMobile: true,
});

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

async function collectConsoleErrors(page: import("@playwright/test").Page) {
  const errors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") {
      errors.push(msg.text());
    }
  });
  return errors;
}

// ---------------------------------------------------------------------------
// Dashboard "/"
// ---------------------------------------------------------------------------

test("Dashboard at 375px: desktop sidebar is hidden", async ({ page }) => {
  await page.goto("/");

  // The desktop sidebar carries classes "hidden lg:flex". At 375px the
  // Tailwind `lg` breakpoint (1024px) never fires, so the element must not
  // be visible. We locate it by tag + class substring that is stable and
  // unique in the layout.
  const sidebar = page.locator("aside.hidden");
  await expect(sidebar).not.toBeVisible();
});

test("Dashboard at 375px: hamburger button is visible", async ({ page }) => {
  await page.goto("/");

  // The hamburger button carries class "lg:hidden" and wraps a <Menu> icon.
  // We target it by its aria role + the svg title Playwright exposes, or
  // more robustly by locating the only button inside <header> that is visible.
  // The Button component renders as <button> with class "lg:hidden".
  const hamburger = page.locator("header button.lg\\:hidden");
  await expect(hamburger).toBeVisible();
});

test("Dashboard at 375px: hamburger opens MobileSidebar nav", async ({ page }) => {
  await page.goto("/");

  const hamburger = page.locator("header button.lg\\:hidden");
  await expect(hamburger).toBeVisible();
  await hamburger.click();

  // MobileSidebar renders as a Dialog. Its DialogContent contains a <nav>
  // with NavLink items. We assert one stable nav link is now visible.
  // "Dashboard" is always the first route (path "/").
  const mobileNav = page.locator('[role="dialog"] nav');
  await expect(mobileNav).toBeVisible();

  // At least one nav link must be visible inside the drawer
  const firstNavLink = mobileNav.locator("a").first();
  await expect(firstNavLink).toBeVisible();

  // Regression guard: the drawer must be pinned to the left edge. The shadcn
  // DialogContent base centers modals via translate(-50%,-50%); the left
  // drawer must override that, or it renders shifted half its size off the
  // top-left corner (x was -140 before the fix).
  const drawerBox = await page.locator('[role="dialog"]').first().boundingBox();
  expect(drawerBox, "drawer has no bounding box").not.toBeNull();
  expect(
    drawerBox!.x,
    `MobileSidebar drawer not pinned to the left edge: x=${drawerBox!.x}`
  ).toBeGreaterThanOrEqual(-1);
  expect(drawerBox!.x).toBeLessThanOrEqual(1);
});

test("Dashboard at 375px: no horizontal overflow", async ({ page }) => {
  await page.goto("/");

  const scrollWidth = await page.evaluate(
    () => document.documentElement.scrollWidth
  );
  // Allow a 1px tolerance for sub-pixel rounding
  expect(
    scrollWidth,
    `Horizontal overflow detected: scrollWidth=${scrollWidth}, viewport=375`
  ).toBeLessThanOrEqual(376);
});

test("Dashboard at 375px: no console errors", async ({ page }) => {
  const errors = await collectConsoleErrors(page);
  await page.goto("/");
  // Let the page settle (lazy routes, data fetches)
  await page.waitForLoadState("networkidle");
  expect(
    errors,
    `Console errors on / at 375px: ${errors.join("; ")}`
  ).toHaveLength(0);
});

// ---------------------------------------------------------------------------
// Usage "/usage"
// ---------------------------------------------------------------------------

test("Usage at 375px: desktop sidebar is hidden", async ({ page }) => {
  await page.goto("/usage");

  const sidebar = page.locator("aside.hidden");
  await expect(sidebar).not.toBeVisible();
});

test("Usage at 375px: hamburger button is visible", async ({ page }) => {
  await page.goto("/usage");

  const hamburger = page.locator("header button.lg\\:hidden");
  await expect(hamburger).toBeVisible();
});

test("Usage at 375px: hamburger opens MobileSidebar nav", async ({ page }) => {
  await page.goto("/usage");

  const hamburger = page.locator("header button.lg\\:hidden");
  await hamburger.click();

  const mobileNav = page.locator('[role="dialog"] nav');
  await expect(mobileNav).toBeVisible();

  const firstNavLink = mobileNav.locator("a").first();
  await expect(firstNavLink).toBeVisible();
});

test("Usage at 375px: no horizontal overflow", async ({ page }) => {
  await page.goto("/usage");

  const scrollWidth = await page.evaluate(
    () => document.documentElement.scrollWidth
  );
  expect(
    scrollWidth,
    `Horizontal overflow detected: scrollWidth=${scrollWidth}, viewport=375`
  ).toBeLessThanOrEqual(376);
});

test("Usage at 375px: no console errors", async ({ page }) => {
  const errors = await collectConsoleErrors(page);
  await page.goto("/usage");
  await page.waitForLoadState("networkidle");
  expect(
    errors,
    `Console errors on /usage at 375px: ${errors.join("; ")}`
  ).toHaveLength(0);
});
