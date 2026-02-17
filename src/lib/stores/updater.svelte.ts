/**
 * Auto-update state management store using Svelte 5 runes.
 *
 * Manages update checking, downloading, and installation via tauri-plugin-updater.
 */

import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

/** Update availability state */
export type UpdateState =
  | 'idle' // Not checked or dismissed
  | 'checking' // Checking for updates
  | 'available' // Update available
  | 'downloading' // Downloading update
  | 'ready' // Downloaded and ready to install
  | 'up-to-date' // No update available
  | 'error'; // Error occurred

/** Updater store state */
interface UpdaterState {
  /** Current update state */
  state: UpdateState;
  /** Available update object (if any) */
  update: Update | null;
  /** Update version string */
  updateVersion: string | null;
  /** Release notes (body from update manifest) */
  releaseNotes: string | null;
  /** Download progress (0-100) */
  downloadProgress: number;
  /** Error message (if state is 'error') */
  error: string | null;
}

/** Updater store singleton */
const updaterState = $state<UpdaterState>({
  state: 'idle',
  update: null,
  updateVersion: null,
  releaseNotes: null,
  downloadProgress: 0,
  error: null,
});

/**
 * Check for available updates
 */
export async function checkForUpdate(): Promise<void> {
  // Reset state
  updaterState.state = 'checking';
  updaterState.error = null;
  updaterState.update = null;
  updaterState.updateVersion = null;
  updaterState.releaseNotes = null;

  try {
    const update = await check();

    if (update) {
      // Update available
      updaterState.state = 'available';
      updaterState.update = update;
      updaterState.updateVersion = update.version;
      updaterState.releaseNotes = update.body || null;
    } else {
      // No update available
      updaterState.state = 'up-to-date';
    }
  } catch (err) {
    // Error checking for updates (likely offline or endpoint unreachable)
    updaterState.state = 'error';
    updaterState.error = err instanceof Error ? err.message : 'Failed to check for updates';
    console.error('Update check failed:', err);
  }
}

/**
 * Download and install the available update, then relaunch
 */
export async function downloadAndInstall(): Promise<void> {
  if (!updaterState.update) {
    console.error('No update available to download');
    return;
  }

  try {
    updaterState.state = 'downloading';
    updaterState.downloadProgress = 0;

    // Download and install with progress callback
    let downloaded = 0;
    await updaterState.update.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Started':
          updaterState.downloadProgress = 0;
          break;
        case 'Progress': {
          // Accumulate downloaded bytes and calculate progress
          const chunk = event.data as { chunkLength: number; contentLength?: number };
          downloaded += chunk.chunkLength;
          if (chunk.contentLength && chunk.contentLength > 0) {
            updaterState.downloadProgress = Math.min(
              99,
              Math.round((downloaded / chunk.contentLength) * 100)
            );
          }
          break;
        }
        case 'Finished':
          updaterState.downloadProgress = 100;
          break;
      }
    });

    // Installation complete
    updaterState.state = 'ready';

    // Relaunch the application
    await relaunch();
  } catch (err) {
    updaterState.state = 'error';
    updaterState.error =
      err instanceof Error ? err.message : 'Failed to download or install update';
    console.error('Update download/install failed:', err);
  }
}

/**
 * Dismiss the current update notification (for this session)
 */
export function dismissUpdate(): void {
  updaterState.state = 'idle';
  updaterState.update = null;
  updaterState.updateVersion = null;
  updaterState.releaseNotes = null;
  updaterState.error = null;
}

/**
 * Get the current updater state (reactive)
 */
export function getUpdaterState(): UpdaterState {
  return updaterState;
}

/** Derived computed properties (as getter functions) */
export function updateAvailable(): boolean {
  return updaterState.state === 'available';
}

export function isDownloading(): boolean {
  return updaterState.state === 'downloading';
}

export function isReady(): boolean {
  return updaterState.state === 'ready';
}

export function hasError(): boolean {
  return updaterState.state === 'error';
}
