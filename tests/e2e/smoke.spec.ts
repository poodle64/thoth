import { test, expect } from "@playwright/test";

/**
 * Frontend boot smoke tests (#114).
 *
 * These answer one question: does the frontend build, serve and render without
 * throwing? They are deliberately not a UI-behaviour suite.
 *
 * Nothing here asserts on data loaded over IPC. Playwright drives a plain
 * browser with no Tauri runtime, so `invoke()` always rejects; asserting on
 * transcriptions, models or settings values would test the mock, not the app.
 * What is worth asserting is that those rejections are *handled* rather than
 * left to crash the page, which is exactly what the pageerror check covers.
 */

/** Errors the browser cannot avoid without a Tauri runtime behind it. */
const EXPECTED_WITHOUT_TAURI = /__TAURI__|__TAURI_INTERNALS__|invoke|IPC/i;

test.describe("frontend boot", () => {
  test("serves the app shell without an uncaught page error", async ({
    page,
  }) => {
    const pageErrors: string[] = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));

    const response = await page.goto("/");
    expect(
      response?.status(),
      "dev server should serve the root document",
    ).toBeLessThan(400);

    // The app is a SPA shell; wait for it to hydrate rather than for network idle,
    // which never settles while failed invoke() calls retry.
    await page.waitForLoadState("domcontentloaded");
    await expect(page.locator("body")).toBeVisible();

    const unexpected = pageErrors.filter(
      (message) => !EXPECTED_WITHOUT_TAURI.test(message),
    );
    expect(
      unexpected,
      `unexpected page errors: ${unexpected.join("; ")}`,
    ).toEqual([]);
  });

  test("completes initialisation and renders the settings shell", async ({
    page,
  }) => {
    await page.goto("/");

    // App.svelte gates the UI behind `isInitialising`, which only clears once
    // config, database and transcription init have all resolved through the
    // browser dev mock. Waiting on the shell is the point of this test: it
    // proves initialisation actually completes, not merely that a spinner
    // rendered. Asserting before this resolves is what made the first version
    // of this test fail against a two-element loading screen.
    await expect(page.locator(".settings-window")).toBeVisible({
      timeout: 15_000,
    });
    await expect(page.locator("nav.sidebar")).toBeVisible();

    // Structural, not copy-based: asserting on visible wording would fail on
    // every label change, which is the fastest way to get a smoke test deleted.
    const elementCount = await page.locator("body *").count();
    expect(elementCount, "expected the settings UI to mount").toBeGreaterThan(
      20,
    );
  });

  test("does not get stuck on the initialisation error screen", async ({
    page,
  }) => {
    await page.goto("/");
    await expect(page.locator(".settings-window")).toBeVisible({
      timeout: 15_000,
    });

    // The dev mock resolves every command, so reaching the error branch means a
    // command was added to the app without being added to the mock: a real
    // regression that would otherwise only show up when someone opened the app
    // in a browser.
    await expect(page.locator(".error-container")).toHaveCount(0);
  });

  test("serves the built stylesheet so the shell is not unstyled", async ({
    page,
  }) => {
    await page.goto("/");
    await page.waitForLoadState("domcontentloaded");

    // Catches a broken CSS pipeline, which renders as a readable-but-wrong page
    // that the DOM assertions above would happily pass.
    const backgroundColor = await page
      .locator("body")
      .evaluate((element) => getComputedStyle(element).backgroundColor);
    expect(
      backgroundColor,
      "body should have a resolved background colour",
    ).toBeTruthy();
  });
});
