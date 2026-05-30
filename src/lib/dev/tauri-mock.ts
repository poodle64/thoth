/**
 * Reusable Tauri IPC transport shim for browser-only dev environments.
 *
 * Installs Tauri's own mockIPC + mockWindows so @tauri-apps/api invoke(),
 * listen(), once(), and emit() all work in plain Vite/browser without a
 * Rust backend. Copy this file (not thoth-mock-data.ts) to other Tauri
 * projects and supply their own command map.
 *
 * WHY: Tauri's real runtime injects __TAURI_INTERNALS__ before app code runs.
 * In the browser that global is absent, so any invoke() call throws
 * "Cannot read properties of undefined (reading 'transformCallback')".
 * mockIPC sets up that global correctly; we do not hand-roll it.
 *
 * NEVER import this file unconditionally — it must only be loaded in dev
 * environments that lack the real Tauri runtime. The guard is in the caller.
 */

import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';

/** A map from Tauri command name to its handler. */
export type CommandMap = Record<string, (args?: Record<string, unknown>) => unknown>;

/** Options for installTauriMock. */
export interface TauriMockOptions {
  /** Window labels to register (default: ["main"]). First entry is the "current" window. */
  windows?: [string, ...string[]];
}

/**
 * Install the Tauri IPC mock transport and wire app-specific command handlers.
 *
 * Must be called synchronously (module-eval time) before any store or component
 * calls invoke() or listen(). In SvelteKit, place the guarded call at the top
 * of the root +layout.svelte <script> block.
 *
 * Unknown commands resolve to undefined (not a throw) so unmocked commands do
 * not crash the app — they just produce null/undefined values that the UI
 * degrades from gracefully.
 */
export function installTauriMock(commandMap: CommandMap, options: TauriMockOptions = {}): void {
  const windows = options.windows ?? ['main'];
  mockWindows(windows[0], ...windows.slice(1));

  mockIPC(
    (cmd, args) => {
      const handler = commandMap[cmd];
      if (handler) {
        return handler(args as Record<string, unknown> | undefined);
      }
      // Quietly resolve unknown commands — app should degrade gracefully, not crash.
      console.debug('[TauriMock] unmocked command:', cmd, args);
      return undefined;
    },
    { shouldMockEvents: true }
  );
}
