/**
 * Clipboard state management store using Svelte 5 runes.
 * Manages clipboard operations, settings, and history for transcription auto-copy.
 */

import { invoke } from '@tauri-apps/api/core';
import { writeText, readText, clear } from '@tauri-apps/plugin-clipboard-manager';

/** Clipboard format options matching Rust ClipboardFormat enum */
export type ClipboardFormat = 'plain_text' | 'rich_text' | 'markdown';

/** Clipboard settings matching Rust ClipboardSettings struct */
export interface ClipboardSettings {
  autoCopyEnabled: boolean;
  format: ClipboardFormat;
  showNotification: boolean;
  preserveClipboard: boolean;
  /** Delay in milliseconds before restoring clipboard (default 1000ms) */
  restoreDelayMs: number;
  historyEnabled: boolean;
}

/** Clipboard history entry matching Rust ClipboardHistoryEntry */
export interface ClipboardHistoryEntry {
  id: string;
  text: string;
  timestamp: string;
  source: string;
}

/** Create the clipboard store with reactive state */
function createClipboardStore() {
  // Settings state
  let settings = $state<ClipboardSettings>({
    autoCopyEnabled: false,
    format: 'plain_text',
    showNotification: false,
    preserveClipboard: true,
    restoreDelayMs: 1000,
    historyEnabled: true,
  });

  // History state
  let history = $state<ClipboardHistoryEntry[]>([]);

  // Loading and error states
  let isLoading = $state<boolean>(false);
  let error = $state<string | null>(null);

  /** Load settings from backend */
  async function loadSettings(): Promise<void> {
    isLoading = true;
    error = null;

    try {
      settings = await invoke<ClipboardSettings>('get_clipboard_settings');
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load clipboard settings';
      console.error('Failed to load clipboard settings:', e);
    } finally {
      isLoading = false;
    }
  }

  /** Save settings to backend */
  async function saveSettings(): Promise<boolean> {
    try {
      await invoke('set_clipboard_settings', { settings });
      return true;
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to save clipboard settings';
      console.error('Failed to save clipboard settings:', e);
      return false;
    }
  }

  /** Update a single setting and auto-save */
  async function updateSetting<K extends keyof ClipboardSettings>(
    key: K,
    value: ClipboardSettings[K]
  ): Promise<boolean> {
    settings = { ...settings, [key]: value };
    return saveSettings();
  }

  /** Load clipboard history from backend */
  async function loadHistory(): Promise<void> {
    try {
      history = await invoke<ClipboardHistoryEntry[]>('get_clipboard_history');
    } catch (e) {
      console.error('Failed to load clipboard history:', e);
    }
  }

  /** Clear all clipboard history */
  async function clearHistory(): Promise<void> {
    try {
      await invoke('clear_clipboard_history');
      history = [];
    } catch (e) {
      console.error('Failed to clear clipboard history:', e);
    }
  }

  /** Remove a specific entry from history */
  async function removeFromHistory(id: string): Promise<boolean> {
    try {
      const removed = await invoke<boolean>('remove_clipboard_history_entry', { id });
      if (removed) {
        history = history.filter((entry) => entry.id !== id);
      }
      return removed;
    } catch (e) {
      console.error('Failed to remove from history:', e);
      return false;
    }
  }

  /**
   * Copy transcription to clipboard with auto-copy settings applied.
   *
   * This is the main entry point for copying transcription results.
   * Call this when transcription completes.
   *
   * @param text - The transcription text to copy
   * @param enhanced - Whether this is an enhanced transcription
   * @returns True if text was copied, false if auto-copy is disabled
   */
  async function copyTranscription(text: string, enhanced: boolean = false): Promise<boolean> {
    if (!text || text.trim().length === 0) {
      return false;
    }

    try {
      const copied = await invoke<boolean>('copy_transcription', { text, enhanced });

      // Refresh history if copy was successful
      if (copied && settings.historyEnabled) {
        await loadHistory();
      }

      return copied;
    } catch (e) {
      console.error('Failed to copy transcription:', e);
      error = e instanceof Error ? e.message : 'Failed to copy to clipboard';
      return false;
    }
  }

  /**
   * Copy text directly to clipboard (bypasses auto-copy settings).
   *
   * @param text - The text to copy
   * @param source - Optional source description for history
   */
  async function copyText(text: string, source: string = 'manual'): Promise<boolean> {
    try {
      await invoke('copy_to_clipboard', { text, source });

      // Refresh history
      if (settings.historyEnabled) {
        await loadHistory();
      }

      return true;
    } catch (e) {
      console.error('Failed to copy to clipboard:', e);
      error = e instanceof Error ? e.message : 'Failed to copy';
      return false;
    }
  }

  /**
   * Copy an entry from history to clipboard.
   *
   * @param id - The history entry ID to copy
   */
  async function copyFromHistory(id: string): Promise<boolean> {
    try {
      await invoke('copy_from_history', { id });
      return true;
    } catch (e) {
      console.error('Failed to copy from history:', e);
      return false;
    }
  }

  /**
   * Restore preserved clipboard content after pasting.
   *
   * Call this after auto-pasting transcription to restore
   * the user's original clipboard content.
   */
  async function restoreClipboard(): Promise<boolean> {
    try {
      return await invoke<boolean>('restore_clipboard');
    } catch (e) {
      console.error('Failed to restore clipboard:', e);
      return false;
    }
  }

  /**
   * Read current clipboard content.
   * Uses the Tauri clipboard plugin directly.
   */
  async function readClipboardText(): Promise<string | null> {
    try {
      return await readText();
    } catch (e) {
      console.error('Failed to read clipboard:', e);
      return null;
    }
  }

  /**
   * Write text directly to clipboard.
   * Uses the Tauri clipboard plugin directly.
   */
  async function writeClipboardText(text: string): Promise<boolean> {
    try {
      await writeText(text);
      return true;
    } catch (e) {
      console.error('Failed to write to clipboard:', e);
      return false;
    }
  }

  /**
   * Clear the system clipboard.
   */
  async function clearClipboard(): Promise<boolean> {
    try {
      await clear();
      return true;
    } catch (e) {
      console.error('Failed to clear clipboard:', e);
      return false;
    }
  }

  /**
   * Insert text at the current cursor position using the typing method.
   *
   * This simulates keyboard input to type the text character by character.
   * Works with most applications but may be slower for long text.
   *
   * @param text - The text to insert
   * @param keystrokeDelayMs - Optional delay between keystrokes in milliseconds
   * @param initialDelayMs - Optional delay before starting insertion
   */
  async function insertTextByTyping(
    text: string,
    keystrokeDelayMs?: number,
    initialDelayMs?: number
  ): Promise<boolean> {
    if (!text || text.trim().length === 0) {
      return false;
    }

    try {
      await invoke('insert_text_by_typing', {
        text,
        keystroke_delay_ms: keystrokeDelayMs,
        initial_delay_ms: initialDelayMs,
      });
      return true;
    } catch (e) {
      console.error('Failed to insert text by typing:', e);
      error = e instanceof Error ? e.message : 'Failed to insert text';
      return false;
    }
  }

  /**
   * Insert text at the current cursor position using clipboard paste.
   *
   * This copies the text to clipboard and simulates Cmd+V (macOS) or Ctrl+V (Linux).
   * Faster than typing but temporarily modifies clipboard contents.
   * The original clipboard content is restored after pasting.
   *
   * @param text - The text to insert
   * @param initialDelayMs - Optional delay before starting insertion
   */
  async function insertTextByPaste(text: string, initialDelayMs?: number): Promise<boolean> {
    if (!text || text.trim().length === 0) {
      return false;
    }

    try {
      await invoke('insert_text_by_paste', {
        text,
        initial_delay_ms: initialDelayMs,
      });
      return true;
    } catch (e) {
      console.error('Failed to insert text by paste:', e);
      error = e instanceof Error ? e.message : 'Failed to paste text';
      return false;
    }
  }

  /**
   * Insert text at the current cursor position.
   *
   * This is a convenience function that uses the default insertion method (typing).
   * For more control, use `insertTextByTyping` or `insertTextByPaste`.
   *
   * @param text - The text to insert
   * @param method - Optional insertion method ("typing" or "paste", defaults to "typing")
   */
  async function insertText(text: string, method?: 'typing' | 'paste'): Promise<boolean> {
    if (!text || text.trim().length === 0) {
      return false;
    }

    try {
      await invoke('insert_text', {
        text,
        method,
      });
      return true;
    } catch (e) {
      console.error('Failed to insert text:', e);
      error = e instanceof Error ? e.message : 'Failed to insert text';
      return false;
    }
  }

  /**
   * Paste transcription at the current cursor position with automatic clipboard restoration.
   *
   * This is the main entry point for pasting transcription results.
   * It handles the complete flow:
   * 1. Save current clipboard contents (if preserve_clipboard enabled)
   * 2. Copy and paste transcription
   * 3. Schedule clipboard restoration after configured delay
   *
   * @param text - The transcription text to paste
   * @param enhanced - Whether this is an enhanced transcription
   * @returns True if paste was successful
   */
  async function pasteTranscription(text: string, enhanced: boolean = false): Promise<boolean> {
    if (!text || text.trim().length === 0) {
      return false;
    }

    try {
      // Call backend command which handles:
      // - Saving clipboard if preserve_clipboard is enabled
      // - Copying transcription with formatting
      // - Pasting at cursor
      // Returns delay in ms for restoration (0 if restoration not enabled)
      const restoreDelay = await invoke<number>('paste_transcription', { text, enhanced });

      // Refresh history
      if (settings.historyEnabled) {
        await loadHistory();
      }

      // Schedule clipboard restoration after configured delay
      if (restoreDelay > 0) {
        setTimeout(async () => {
          try {
            await restoreClipboard();
          } catch (e) {
            console.error('Failed to restore clipboard after delay:', e);
          }
        }, restoreDelay);
      }

      return true;
    } catch (e) {
      console.error('Failed to paste transcription:', e);
      error = e instanceof Error ? e.message : 'Failed to paste transcription';
      return false;
    }
  }

  /**
   * Paste transcription using a specific insertion method.
   *
   * For more control over insertion method, use this function instead of pasteTranscription.
   * Note: This does NOT handle clipboard restoration; use pasteTranscription for full flow.
   *
   * @param text - The text to paste
   * @param method - Insertion method ("typing" or "paste")
   */
  async function pasteWithMethod(
    text: string,
    method: 'typing' | 'paste' = 'paste'
  ): Promise<boolean> {
    if (!text || text.trim().length === 0) {
      return false;
    }

    try {
      return await insertText(text, method);
    } catch (e) {
      console.error('Failed to paste with method:', e);
      error = e instanceof Error ? e.message : 'Failed to paste';
      return false;
    }
  }

  /** Clear error state */
  function clearError(): void {
    error = null;
  }

  /** Initialise the store by loading settings and history */
  async function initialise(): Promise<void> {
    await loadSettings();
    if (settings.historyEnabled) {
      await loadHistory();
    }
  }

  return {
    // State (getters for reactive access)
    get settings() {
      return settings;
    },
    get history() {
      return history;
    },
    get isLoading() {
      return isLoading;
    },
    get error() {
      return error;
    },

    // Derived convenience getters
    get autoCopyEnabled() {
      return settings.autoCopyEnabled;
    },
    get historyEnabled() {
      return settings.historyEnabled;
    },

    // Settings actions
    loadSettings,
    saveSettings,
    updateSetting,

    // History actions
    loadHistory,
    clearHistory,
    removeFromHistory,

    // Clipboard operations
    copyTranscription,
    copyText,
    copyFromHistory,
    restoreClipboard,
    readClipboardText,
    writeClipboardText,
    clearClipboard,

    // Paste at cursor operations
    insertText,
    insertTextByTyping,
    insertTextByPaste,
    pasteTranscription,
    pasteWithMethod,

    // Utilities
    clearError,
    initialise,
  };
}

// Export singleton instance
export const clipboardStore = createClipboardStore();
