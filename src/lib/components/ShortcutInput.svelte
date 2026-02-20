<script lang="ts">
  /**
   * ShortcutInput - Key capture component for configuring keyboard shortcuts
   *
   * Uses native Rust-based keyboard capture via device_query to capture keys
   * at the system level, bypassing webview limitations that prevent capturing
   * keys intercepted by macOS (like Cmd+letter combinations).
   *
   * On Wayland, falls back to webview keyboard events since device_query
   * doesn't work there (X11-only).
   */

  import { onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    formatForDisplay,
    validateShortcut,
    shortcutsStore,
    type ShortcutInfo,
  } from '../stores/shortcuts.svelte';

  /** Debug logging — only active in development builds */
  const debug = import.meta.env.DEV
    ? (...args: unknown[]) => console.log('[ShortcutInput]', ...args)
    : () => {};

  interface Props {
    /** Current shortcut accelerator string */
    value: string;
    /** Unique identifier for this shortcut (used to skip re-registration) */
    shortcutId?: string;
    /** Callback when shortcut changes (can be async) */
    onchange?: (accelerator: string) => void | Promise<void>;
    /** Callback when shortcut is cleared */
    onclear?: () => void;
    /** Placeholder text when no shortcut is set */
    placeholder?: string;
    /** Whether the input is disabled */
    disabled?: boolean;
    /** Default value for reset functionality */
    defaultValue?: string;
    /** Callback when reset to default is clicked */
    onreset?: () => void;
  }

  let {
    value = '',
    shortcutId,
    onchange,
    onclear,
    placeholder = 'Click to record shortcut',
    disabled = false,
    defaultValue,
    onreset,
  }: Props = $props();

  let isCapturing = $state(false);
  let validationError = $state<string | null>(null);
  let pendingKeys = $state<string[]>([]);
  let buttonRef = $state<HTMLButtonElement | null>(null);
  let shortcutsPaused = $state(false);
  /** Store shortcuts to restore after capture */
  let savedShortcuts = $state<ShortcutInfo[]>([]);
  /** Event listeners to clean up */
  let unlisteners: UnlistenFn[] = [];
  /** Whether using webview mode (Wayland) */
  let webviewMode = $state(false);

  /** Formatted display value for the current shortcut */
  let displayValue = $derived(formatForDisplay(value));

  /** Whether the current value differs from the default */
  let isModified = $derived(defaultValue !== undefined && value !== defaultValue);

  /** Display string for currently pressed keys */
  let pendingDisplay = $derived(pendingKeys.join(' + '));

  // Clean up on destroy
  onDestroy(async () => {
    await cleanup();
  });

  /**
   * Clean up event listeners and stop capture
   */
  async function cleanup(): Promise<void> {
    for (const unlisten of unlisteners) {
      unlisten();
    }
    unlisteners = [];

    if (isCapturing) {
      try {
        await invoke('stop_key_capture');
      } catch {
        // Ignore errors during cleanup
      }
    }
  }

  /**
   * Pause global shortcuts so the captured keys can be used
   */
  async function pauseGlobalShortcuts(): Promise<void> {
    if (shortcutsPaused) return;
    try {
      savedShortcuts = [...shortcutsStore.shortcuts];
      await invoke('unregister_all_shortcuts');
      shortcutsPaused = true;
    } catch (e) {
      console.error('Failed to pause shortcuts:', e);
    }
  }

  /**
   * Resume global shortcuts after capture
   *
   * Re-registers shortcuts that were paused, EXCEPT for the one being edited
   * (identified by shortcutId). The onchange handler will register the new
   * shortcut for the one being edited.
   */
  async function resumeGlobalShortcuts(): Promise<void> {
    if (!shortcutsPaused) return;
    try {
      // Re-register all shortcuts EXCEPT the one we're currently editing
      // (the onchange handler will register that one with the new value)
      for (const shortcut of savedShortcuts) {
        // Skip the shortcut being edited - identified by shortcutId
        // The onchange handler has already registered the new accelerator for this one
        if (shortcutId && shortcut.id === shortcutId) {
          continue;
        }
        if (shortcut.accelerator) {
          await invoke('register_shortcut', {
            id: shortcut.id,
            accelerator: shortcut.accelerator,
            description: shortcut.description,
          });
        }
      }
      shortcutsPaused = false;
      savedShortcuts = [];
      await shortcutsStore.loadRegistered();
    } catch (e) {
      console.error('Failed to resume shortcuts:', e);
    }
  }

  /**
   * Start capturing keyboard input using native capture
   */
  async function startCapture(): Promise<void> {
    if (disabled || isCapturing) return;

    debug('Starting capture...');

    // Pause global shortcuts first
    await pauseGlobalShortcuts();

    isCapturing = true;
    validationError = null;
    pendingKeys = [];

    try {
      // Set up event listeners for capture events
      const updateUnlisten = await listen<{ keys: string[]; accelerator: string; isValid: boolean }>(
        'key-capture-update',
        (event) => {
          debug('Key update:', event.payload);
          pendingKeys = event.payload.keys;

          // Show validation error only if we have a non-modifier key and it's invalid
          if (event.payload.accelerator && !event.payload.isValid) {
            // Only show error if there's a non-modifier key
            const hasNonModifier = event.payload.keys.some(
              (k) => !['Cmd', 'Ctrl', 'Alt', 'Shift', 'Super', 'Meta'].includes(k)
            );
            if (hasNonModifier) {
              validationError = 'Invalid shortcut combination';
            } else {
              validationError = null;
            }
          } else {
            validationError = null;
          }
        }
      );
      unlisteners.push(updateUnlisten);

      const completeUnlisten = await listen<{ accelerator: string; keys: string[]; isValid: boolean }>(
        'key-capture-complete',
        async (event) => {
          debug('Capture complete:', event.payload);

          if (event.payload.isValid) {
            // Validate with our frontend validator as well
            const error = validateShortcut(event.payload.accelerator);
            if (error) {
              validationError = error;
            } else {
              // Wait for onchange to complete before resuming shortcuts
              // This ensures the new shortcut is registered before we
              // re-register the other shortcuts
              await onchange?.(event.payload.accelerator);
              validationError = null;
            }
          }

          await stopCapture();
        }
      );
      unlisteners.push(completeUnlisten);

      // Start key capture - returns 'native' or 'webview' mode
      const mode = await invoke<string>('start_key_capture');
      webviewMode = mode === 'webview';
      debug(`Capture started in ${mode} mode`);

      // Focus button for visual feedback
      buttonRef?.focus();
    } catch (e) {
      console.error('Failed to start capture:', e);
      const errorMsg = String(e);

      // Check if it's a permission error
      if (errorMsg.includes('Input Monitoring')) {
        validationError = 'Input Monitoring permission required';
        // Offer to open settings
        try {
          await invoke('request_input_monitoring');
        } catch {
          // Ignore if opening settings fails
        }
      } else {
        validationError = `Failed to start capture: ${e}`;
      }
      await stopCapture();
    }
  }

  /**
   * Stop capturing and clean up
   */
  async function stopCapture(): Promise<void> {
    if (!isCapturing) return;

    isCapturing = false;
    pendingKeys = [];
    webviewMode = false;

    // Clean up listeners
    for (const unlisten of unlisteners) {
      unlisten();
    }
    unlisteners = [];

    try {
      await invoke('stop_key_capture');
    } catch (e) {
      console.error('Failed to stop key capture:', e);
    }

    // Resume global shortcuts
    await resumeGlobalShortcuts();
  }

  /**
   * Cancel capture without applying changes
   */
  async function cancelCapture(): Promise<void> {
    validationError = null;
    await stopCapture();
  }

  /**
   * Handle keyboard events
   * - In webview mode (Wayland): report keys to backend
   * - In native mode: only handle Escape to cancel
   */
  async function handleKeyDown(event: KeyboardEvent): Promise<void> {
    if (!isCapturing) return;

    // Escape cancels capture
    if (event.key === 'Escape') {
      event.preventDefault();
      await cancelCapture();
      return;
    }

    // In webview mode, report key events to backend
    if (webviewMode) {
      event.preventDefault();

      // Ignore pure modifier keydowns (they're tracked separately)
      const isModifier = ['Control', 'Shift', 'Alt', 'Meta'].includes(event.key);
      if (isModifier) {
        // Update pending display for modifiers
        const modNames: string[] = [];
        if (event.ctrlKey) modNames.push('Ctrl');
        if (event.shiftKey) modNames.push('Shift');
        if (event.altKey) modNames.push('Alt');
        if (event.metaKey) modNames.push('Super');
        pendingKeys = modNames;
        return;
      }

      // Report key event to backend
      try {
        await invoke('report_key_event', {
          key: event.key,
          code: event.code,
          ctrl: event.ctrlKey,
          shift: event.shiftKey,
          alt: event.altKey,
          meta: event.metaKey,
          event_type: 'keydown',
        });
      } catch (e) {
        console.error('Failed to report key event:', e);
      }
    }
  }

  /**
   * Handle blur event - stop capture when focus is lost
   */
  async function handleBlur(): Promise<void> {
    if (isCapturing) {
      debug('Blur - stopping capture');
      await stopCapture();
    }
  }

  /**
   * Clear the current shortcut
   */
  function handleClear(event: MouseEvent): void {
    event.stopPropagation();
    onclear?.();
    validationError = null;
  }

  /**
   * Reset to default value
   */
  function handleReset(event: MouseEvent): void {
    event.stopPropagation();
    onreset?.();
    validationError = null;
  }
</script>

<div class="shortcut-input-container">
  <button
    bind:this={buttonRef}
    type="button"
    class="shortcut-input"
    class:capturing={isCapturing}
    class:has-value={!!value}
    class:has-error={!!validationError}
    class:disabled
    onclick={startCapture}
    onkeydown={handleKeyDown}
    onblur={handleBlur}
    {disabled}
  >
    {#if isCapturing}
      <span class="capture-hint">
        {#if pendingKeys.length > 0}
          {pendingDisplay}
        {:else}
          Press keys...
        {/if}
      </span>
    {:else if value}
      <span class="shortcut-display">{displayValue}</span>
    {:else}
      <span class="placeholder">{placeholder}</span>
    {/if}
  </button>

  <div class="actions">
    {#if value && !disabled}
      <button
        type="button"
        class="action-btn clear-btn"
        title="Clear shortcut"
        onclick={handleClear}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>
    {/if}
    {#if isModified && onreset && !disabled}
      <button
        type="button"
        class="action-btn reset-btn"
        title="Reset to default"
        onclick={handleReset}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"></path>
          <path d="M3 3v5h5"></path>
        </svg>
      </button>
    {/if}
  </div>

  {#if validationError}
    <span class="error-message">{validationError}</span>
  {/if}
</div>

<style>
  .shortcut-input-container {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .shortcut-input {
    min-width: 180px;
    padding: 8px 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-secondary);
    color: var(--color-text-primary);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
    transition:
      border-color var(--transition-fast),
      background var(--transition-fast);
  }

  .shortcut-input:hover:not(.disabled) {
    border-color: var(--color-accent);
  }

  .shortcut-input:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .shortcut-input.capturing {
    border-color: var(--color-accent);
    background: var(--color-bg-tertiary);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-accent) 20%, transparent);
  }

  .shortcut-input.has-error {
    border-color: var(--color-error);
  }

  .shortcut-input.disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .capture-hint {
    color: var(--color-accent);
    font-style: italic;
  }

  .shortcut-display {
    color: var(--color-text-primary);
    letter-spacing: 0.5px;
  }

  .placeholder {
    color: var(--color-text-tertiary);
  }

  .actions {
    display: flex;
    gap: 4px;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }

  .action-btn:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
  }

  .clear-btn:hover {
    color: var(--color-error);
  }

  .reset-btn:hover {
    color: var(--color-accent);
  }

  .error-message {
    width: 100%;
    margin-top: 4px;
    font-size: var(--text-xs);
    color: var(--color-error);
  }
</style>
