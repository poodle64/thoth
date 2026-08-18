import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright configuration for Thoth's frontend smoke tests.
 *
 * Scope is deliberately narrow (#114): this drives the SvelteKit frontend in a
 * plain browser, not the Tauri window. Driving native windows and IPC from
 * Playwright is a much larger lift, and the value here is catching "the
 * frontend fails to boot" regressions for near-zero maintenance.
 *
 * Because there is no Tauri runtime in the browser, `window.__TAURI__` is
 * absent and every `invoke()` rejects. The tests assert on what renders anyway
 * rather than on data that can only arrive over IPC; see tests/e2e/smoke.spec.ts.
 */
export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: true,
  // Fail the build if a `test.only` is committed.
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: process.env.CI ? "list" : "html",

  use: {
    // Port 1422 is the project's fixed dev-server port; see .claude/CLAUDE.md.
    baseURL: "http://localhost:1422",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },

  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],

  webServer: {
    command: "pnpm dev --port 1422",
    port: 1422,
    // Reuse a dev server already running locally; always start a fresh one in CI.
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
