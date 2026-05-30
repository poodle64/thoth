/**
 * Auto-update state management using Svelte 5 runes.
 *
 * Manages update checking, downloading, and installation via tauri-plugin-updater.
 * User-facing notifications are handled via sonner toasts.
 */

import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'svelte-sonner';

/** GitHub releases page for manual download fallback */
export const RELEASES_URL = 'https://github.com/poodle64/thoth/releases/latest';

/** Update state visible to the Overview pane */
export type UpdateState =
  | 'idle'
  | 'checking'
  | 'available'
  | 'downloading'
  | 'up-to-date'
  | 'error';

interface UpdaterState {
  state: UpdateState;
  update: Update | null;
  updateVersion: string | null;
  error: string | null;
}

const updaterState = $state<UpdaterState>({
  state: 'idle',
  update: null,
  updateVersion: null,
  error: null,
});

function openReleasesPage() {
  invoke('open_url', { url: RELEASES_URL }).catch((err) =>
    console.error('Failed to open releases page:', err)
  );
}

/** Download and install the available update, then relaunch */
async function downloadAndInstall(): Promise<void> {
  if (!updaterState.update) return;

  const toastId = toast.loading('Downloading update...');

  try {
    updaterState.state = 'downloading';

    let downloaded = 0;
    await updaterState.update.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Progress': {
          const chunk = event.data as { chunkLength: number; contentLength?: number };
          downloaded += chunk.chunkLength;
          if (chunk.contentLength && chunk.contentLength > 0) {
            const pct = Math.min(99, Math.round((downloaded / chunk.contentLength) * 100));
            toast.loading(`Downloading update... ${pct}%`, { id: toastId });
          }
          break;
        }
        case 'Finished':
          toast.success('Update installed. Restarting...', { id: toastId });
          break;
      }
    });

    await relaunch();
  } catch (err) {
    const message = describeUpdateError(err);
    updaterState.state = 'error';
    updaterState.error = message;
    toast.error('Update failed', {
      id: toastId,
      description: message,
      action: { label: 'Retry', onClick: () => void checkForUpdate() },
    });
  }
}

/** Translate raw update errors into actionable user-facing messages */
function describeUpdateError(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  const lower = raw.toLowerCase();

  if (lower.includes('permission') || lower.includes('privilege') || lower.includes('cancel')) {
    return 'Update requires administrator access. Please try again and enter your password when prompted.';
  }
  if (
    lower.includes('network') ||
    lower.includes('connect') ||
    lower.includes('timed out') ||
    lower.includes('fetch')
  ) {
    return 'Download interrupted. Check your internet connection and try again.';
  }
  if (lower.includes('signature') || lower.includes('verify')) {
    return 'Update signature verification failed. The download may be corrupted. Please try again.';
  }

  return `Update failed: ${raw}`;
}

/**
 * Check for available updates.
 * Shows a toast when an update is found or when the check fails.
 */
export async function checkForUpdate(): Promise<void> {
  updaterState.state = 'checking';
  updaterState.error = null;
  updaterState.update = null;
  updaterState.updateVersion = null;

  try {
    const update = await check();

    if (update) {
      updaterState.state = 'available';
      updaterState.update = update;
      updaterState.updateVersion = update.version;

      toast.info(`Update available: v${update.version}`, {
        duration: Infinity,
        action: { label: 'Update Now', onClick: () => void downloadAndInstall() },
      });
    } else {
      updaterState.state = 'up-to-date';
    }
  } catch (err) {
    updaterState.state = 'error';
    updaterState.error = err instanceof Error ? err.message : 'Failed to check for updates';
    console.error('Update check failed:', err);

    toast.error('Failed to check for updates', {
      description: updaterState.error,
      action: { label: 'Download Manually', onClick: openReleasesPage },
    });
  }
}

/** Get the current updater state (reactive, for OverviewPane status display) */
export function getUpdaterState(): UpdaterState {
  return updaterState;
}
