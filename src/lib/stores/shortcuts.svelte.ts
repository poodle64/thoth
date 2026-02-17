/**
 * Shortcut state management for Thoth
 *
 * Manages global keyboard shortcuts configuration and synchronisation
 * with the Tauri backend.
 */

import { invoke } from '@tauri-apps/api/core';

/** Information about a keyboard shortcut */
export interface ShortcutInfo {
  /** Unique identifier for the shortcut */
  id: string;
  /** Keyboard accelerator string (e.g., "F13", "CommandOrControl+Shift+Space") */
  accelerator: string;
  /** Human-readable description of what the shortcut does */
  description: string;
  /** Whether the shortcut is currently enabled/registered */
  isEnabled: boolean;
}

/** Shortcut configuration for persistence */
export interface ShortcutConfig {
  id: string;
  accelerator: string;
}

/** Information about a shortcut conflict */
export interface ShortcutConflict {
  /** The shortcut that failed to register */
  shortcut: string;
  /** The ID of the shortcut that was being registered */
  shortcutId: string;
  /** Human-readable reason for the conflict */
  reason: string;
  /** Suggested alternative shortcuts */
  suggestions: string[];
}

/** Result of attempting to register a shortcut with conflict detection */
export type RegistrationResult =
  | { type: 'Success'; shortcut: string; shortcut_id: string }
  | {
      type: 'Conflict';
      shortcut: string;
      shortcut_id: string;
      reason: string;
      suggestions: string[];
    };

/** Shortcut info extended with conflict state */
export interface ShortcutWithConflict extends ShortcutInfo {
  /** Conflict information if registration failed */
  conflict?: ShortcutConflict;
}

/** Valid modifier keys for shortcuts */
export const MODIFIER_KEYS = ['Control', 'Alt', 'Shift', 'Meta', 'Command'] as const;
export type ModifierKey = (typeof MODIFIER_KEYS)[number];

/** Keys that cannot be used alone as shortcuts */
const MODIFIER_ONLY_CODES = new Set([
  'ControlLeft',
  'ControlRight',
  'AltLeft',
  'AltRight',
  'ShiftLeft',
  'ShiftRight',
  'MetaLeft',
  'MetaRight',
]);

/**
 * Format a KeyboardEvent into a Tauri-compatible accelerator string
 *
 * @param event - The keyboard event to format
 * @returns The formatted accelerator string, or null if invalid
 */
export function formatAccelerator(event: KeyboardEvent): string | null {
  // Ignore modifier-only key presses
  if (MODIFIER_ONLY_CODES.has(event.code)) {
    return null;
  }

  const parts: string[] = [];

  // Add modifiers in consistent order
  if (event.metaKey) {
    parts.push('CommandOrControl');
  } else if (event.ctrlKey) {
    parts.push('CommandOrControl');
  }
  if (event.altKey) {
    parts.push('Alt');
  }
  if (event.shiftKey) {
    parts.push('Shift');
  }

  // Get the key name
  const key = normaliseKeyName(event);
  if (key) {
    parts.push(key);
  } else {
    return null;
  }

  return parts.join('+');
}

/**
 * Normalise a key name from a KeyboardEvent for Tauri compatibility
 *
 * @param event - The keyboard event
 * @returns The normalised key name, or null if invalid
 */
function normaliseKeyName(event: KeyboardEvent): string | null {
  const { code, key } = event;

  // Function keys
  if (code.startsWith('F') && /^F\d+$/.test(code)) {
    return code;
  }

  // Letter keys
  if (code.startsWith('Key')) {
    return code.slice(3).toUpperCase();
  }

  // Digit keys
  if (code.startsWith('Digit')) {
    return code.slice(5);
  }

  // Numpad keys
  if (code.startsWith('Numpad')) {
    const numKey = code.slice(6);
    if (/^\d$/.test(numKey)) {
      return `Num${numKey}`;
    }
    // Map numpad operators
    const numpadMap: Record<string, string> = {
      Add: 'NumAdd',
      Subtract: 'NumSubtract',
      Multiply: 'NumMultiply',
      Divide: 'NumDivide',
      Decimal: 'NumDecimal',
      Enter: 'NumEnter',
    };
    return numpadMap[numKey] ?? null;
  }

  // Special keys
  const specialKeyMap: Record<string, string> = {
    Space: 'Space',
    Enter: 'Enter',
    Tab: 'Tab',
    Escape: 'Escape',
    Backspace: 'Backspace',
    Delete: 'Delete',
    Insert: 'Insert',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    BracketLeft: '[',
    BracketRight: ']',
    Semicolon: ';',
    Quote: "'",
    Backquote: '`',
    Backslash: '\\',
    Comma: ',',
    Period: '.',
    Slash: '/',
    Minus: '-',
    Equal: '=',
  };

  if (code in specialKeyMap) {
    return specialKeyMap[code];
  }

  // Fallback to key value for printable characters
  if (key.length === 1 && /^[a-zA-Z0-9]$/.test(key)) {
    return key.toUpperCase();
  }

  return null;
}

/**
 * Format an accelerator string for human-readable display
 *
 * @param accelerator - The Tauri accelerator string
 * @returns Human-readable display string
 */
export function formatForDisplay(accelerator: string): string {
  if (!accelerator) return '';

  return (
    accelerator
      // Handle right-side modifier codes first (before generic replacements)
      .replace(/ShiftRight/g, 'Right ⇧')
      .replace(/ShiftLeft/g, 'Left ⇧')
      .replace(/AltRight/g, navigator.platform.includes('Mac') ? 'Right ⌥' : 'Right Alt')
      .replace(/AltLeft/g, navigator.platform.includes('Mac') ? 'Left ⌥' : 'Left Alt')
      .replace(/ControlRight/g, 'Right ⌃')
      .replace(/ControlLeft/g, 'Left ⌃')
      .replace(/MetaRight/g, 'Right ⌘')
      .replace(/MetaLeft/g, 'Left ⌘')
      // Then generic modifiers
      .replace(/CommandOrControl/g, navigator.platform.includes('Mac') ? '⌘' : 'Ctrl')
      .replace(/Command/g, '⌘')
      .replace(/Control/g, '⌃')
      .replace(/Alt/g, navigator.platform.includes('Mac') ? '⌥' : 'Alt')
      .replace(/Shift/g, '⇧')
      .replace(/\+/g, ' ')
  );
}

/**
 * Validate a shortcut accelerator string
 *
 * @param accelerator - The accelerator string to validate
 * @returns An error message if invalid, or null if valid
 */
export function validateShortcut(accelerator: string): string | null {
  if (!accelerator || accelerator.trim() === '') {
    return 'Shortcut cannot be empty';
  }

  // Check for valid format (no leading/trailing +, no empty parts)
  if (accelerator.startsWith('+') || accelerator.endsWith('+')) {
    return 'Invalid shortcut format';
  }
  if (accelerator.includes('++')) {
    return 'Invalid shortcut format';
  }

  const parts = accelerator.split('+');

  if (parts.length === 0) {
    return 'Invalid shortcut format';
  }

  // Check for at least one non-modifier key
  // Note: Right-side modifier codes (ShiftRight, AltRight, etc.) count as main keys
  const modifiers = new Set(['CommandOrControl', 'Command', 'Control', 'Alt', 'Shift', 'Meta']);
  const rightModifierCodes = new Set([
    'ShiftRight',
    'ShiftLeft',
    'ControlRight',
    'ControlLeft',
    'AltRight',
    'AltLeft',
    'MetaRight',
    'MetaLeft',
  ]);
  const hasNonModifier = parts.some((part) => !modifiers.has(part) || rightModifierCodes.has(part));

  if (!hasNonModifier) {
    return 'Shortcut must include a non-modifier key';
  }

  // Any key or key combination is valid - no requirement for modifiers
  return null;
}

/**
 * Create a shortcuts store for managing shortcut state
 */
export function createShortcutsStore() {
  let shortcuts = $state<ShortcutWithConflict[]>([]);
  let defaults = $state<ShortcutInfo[]>([]);
  let isLoading = $state<boolean>(false);
  let error = $state<string | null>(null);
  let conflicts = $state<ShortcutConflict[]>([]);

  /**
   * Load default shortcuts from the backend
   */
  async function loadDefaults(): Promise<void> {
    try {
      const result = await invoke<ShortcutInfo[]>('get_default_shortcuts');
      // Convert snake_case to camelCase
      defaults = result.map((s) => ({
        id: s.id,
        accelerator: s.accelerator,
        description: s.description,
        isEnabled: (s as unknown as { is_enabled?: boolean }).is_enabled ?? s.isEnabled ?? false,
      }));
    } catch (e) {
      console.error('Failed to load default shortcuts:', e);
      error = `Failed to load default shortcuts: ${e}`;
    }
  }

  /**
   * Load currently registered shortcuts from the backend
   */
  async function loadRegistered(): Promise<void> {
    isLoading = true;
    error = null;
    try {
      const result = await invoke<ShortcutInfo[]>('list_registered_shortcuts');
      // Convert snake_case to camelCase
      shortcuts = result.map((s) => ({
        id: s.id,
        accelerator: s.accelerator,
        description: s.description,
        isEnabled: (s as unknown as { is_enabled?: boolean }).is_enabled ?? s.isEnabled ?? false,
      }));
    } catch (e) {
      console.error('Failed to load registered shortcuts:', e);
      error = `Failed to load shortcuts: ${e}`;
    } finally {
      isLoading = false;
    }
  }

  /**
   * Register a shortcut with the backend
   */
  async function register(
    id: string,
    accelerator: string,
    description: string
  ): Promise<{ success: boolean; error?: string }> {
    const validationError = validateShortcut(accelerator);
    if (validationError) {
      return { success: false, error: validationError };
    }

    try {
      await invoke('register_shortcut', { id, accelerator, description });
      await loadRegistered();
      return { success: true };
    } catch (e) {
      const errorMsg = `${e}`;
      console.error('Failed to register shortcut:', errorMsg);
      return { success: false, error: errorMsg };
    }
  }

  /**
   * Unregister a shortcut
   */
  async function unregister(id: string): Promise<{ success: boolean; error?: string }> {
    try {
      await invoke('unregister_shortcut', { id });
      await loadRegistered();
      return { success: true };
    } catch (e) {
      const errorMsg = `${e}`;
      console.error('Failed to unregister shortcut:', errorMsg);
      return { success: false, error: errorMsg };
    }
  }

  /**
   * Update a shortcut's accelerator
   */
  async function update(
    id: string,
    newAccelerator: string
  ): Promise<{ success: boolean; error?: string }> {
    // Find the existing shortcut
    const existing = shortcuts.find((s) => s.id === id);
    const defaultShortcut = defaults.find((s) => s.id === id);

    if (!existing && !defaultShortcut) {
      return { success: false, error: `Shortcut '${id}' not found` };
    }

    const description = existing?.description ?? defaultShortcut?.description ?? '';

    // Try to unregister existing if it appears registered
    // We don't fail if unregister fails - the shortcut may have already been
    // unregistered (e.g., by pauseGlobalShortcuts during capture)
    if (existing) {
      const unregResult = await unregister(id);
      if (!unregResult.success) {
        // Log but don't fail - shortcut may already be unregistered
        console.warn(
          `[Shortcuts] unregister returned error (may be expected): ${unregResult.error}`
        );
      }
    }

    // Register with new accelerator
    return register(id, newAccelerator, description);
  }

  /**
   * Reset a shortcut to its default value
   */
  async function resetToDefault(id: string): Promise<{ success: boolean; error?: string }> {
    const defaultShortcut = defaults.find((s) => s.id === id);
    if (!defaultShortcut) {
      return { success: false, error: `No default found for shortcut '${id}'` };
    }

    return update(id, defaultShortcut.accelerator);
  }

  /**
   * Register all default shortcuts
   */
  async function registerDefaults(): Promise<{ success: boolean; error?: string }> {
    try {
      await invoke('register_default_shortcuts');
      await loadRegistered();
      return { success: true };
    } catch (e) {
      const errorMsg = `${e}`;
      console.error('Failed to register default shortcuts:', errorMsg);
      return { success: false, error: errorMsg };
    }
  }

  /**
   * Unregister all shortcuts
   */
  async function unregisterAll(): Promise<{ success: boolean; error?: string }> {
    try {
      await invoke('unregister_all_shortcuts');
      await loadRegistered();
      return { success: true };
    } catch (e) {
      const errorMsg = `${e}`;
      console.error('Failed to unregister all shortcuts:', errorMsg);
      return { success: false, error: errorMsg };
    }
  }

  /**
   * Initialise the store by loading defaults and registered shortcuts
   */
  async function initialise(): Promise<void> {
    await Promise.all([loadDefaults(), loadRegistered()]);
  }

  /**
   * Try to register a shortcut with conflict detection
   * Returns detailed information about success or failure
   */
  async function tryRegister(
    id: string,
    accelerator: string,
    description: string
  ): Promise<RegistrationResult> {
    const validationError = validateShortcut(accelerator);
    if (validationError) {
      // Return as a conflict with local validation error
      const conflict: ShortcutConflict = {
        shortcut: accelerator,
        shortcutId: id,
        reason: validationError,
        suggestions: [],
      };
      // Get suggestions from backend
      try {
        conflict.suggestions = await invoke<string[]>('get_shortcut_suggestions', {
          shortcut: accelerator,
        });
      } catch {
        // Ignore suggestion fetch failure
      }
      return {
        type: 'Conflict',
        shortcut: accelerator,
        shortcut_id: id,
        reason: validationError,
        suggestions: conflict.suggestions,
      };
    }

    try {
      const result = await invoke<RegistrationResult>('try_register_shortcut', {
        id,
        accelerator,
        description,
      });

      if (result.type === 'Success') {
        // Update local state
        const existingIndex = shortcuts.findIndex((s) => s.id === id);
        const newInfo: ShortcutWithConflict = {
          id,
          accelerator,
          description,
          isEnabled: true,
          conflict: undefined,
        };
        if (existingIndex >= 0) {
          shortcuts[existingIndex] = newInfo;
        } else {
          shortcuts = [...shortcuts, newInfo];
        }
        // Remove from conflicts list if present
        conflicts = conflicts.filter((c) => c.shortcutId !== id);
      } else {
        // Handle conflict
        const conflict: ShortcutConflict = {
          shortcut: result.shortcut,
          shortcutId: result.shortcut_id,
          reason: result.reason,
          suggestions: result.suggestions,
        };
        // Update conflicts list
        const existingConflictIndex = conflicts.findIndex((c) => c.shortcutId === id);
        if (existingConflictIndex >= 0) {
          conflicts[existingConflictIndex] = conflict;
        } else {
          conflicts = [...conflicts, conflict];
        }
        // Update shortcut state with conflict
        const existingIndex = shortcuts.findIndex((s) => s.id === id);
        const newInfo: ShortcutWithConflict = {
          id,
          accelerator,
          description,
          isEnabled: false,
          conflict,
        };
        if (existingIndex >= 0) {
          shortcuts[existingIndex] = newInfo;
        } else {
          shortcuts = [...shortcuts, newInfo];
        }
      }

      return result;
    } catch (e) {
      const errorMsg = `${e}`;
      console.error('Failed to try register shortcut:', errorMsg);
      // Return as conflict
      return {
        type: 'Conflict',
        shortcut: accelerator,
        shortcut_id: id,
        reason: errorMsg,
        suggestions: [],
      };
    }
  }

  /**
   * Check if a shortcut is available for registration
   */
  async function checkAvailable(accelerator: string): Promise<boolean> {
    try {
      return await invoke<boolean>('check_shortcut_available', { accelerator });
    } catch (e) {
      console.error('Failed to check shortcut availability:', e);
      return false;
    }
  }

  /**
   * Get alternative shortcut suggestions from the backend
   */
  async function getSuggestions(shortcut: string): Promise<string[]> {
    try {
      return await invoke<string[]>('get_shortcut_suggestions', { shortcut });
    } catch (e) {
      console.error('Failed to get shortcut suggestions:', e);
      return [];
    }
  }

  /**
   * Clear a conflict for a specific shortcut
   */
  function clearConflict(id: string): void {
    conflicts = conflicts.filter((c) => c.shortcutId !== id);
    const index = shortcuts.findIndex((s) => s.id === id);
    if (index >= 0 && shortcuts[index].conflict) {
      shortcuts[index] = { ...shortcuts[index], conflict: undefined };
    }
  }

  /**
   * Get all shortcuts that have conflicts
   */
  function getShortcutsWithConflicts(): ShortcutWithConflict[] {
    return shortcuts.filter((s) => s.conflict !== undefined);
  }

  return {
    get shortcuts() {
      return shortcuts;
    },
    get defaults() {
      return defaults;
    },
    get isLoading() {
      return isLoading;
    },
    get error() {
      return error;
    },
    get conflicts() {
      return conflicts;
    },
    get hasConflicts() {
      return conflicts.length > 0;
    },
    loadDefaults,
    loadRegistered,
    register,
    unregister,
    update,
    resetToDefault,
    registerDefaults,
    unregisterAll,
    initialise,
    tryRegister,
    checkAvailable,
    getSuggestions,
    clearConflict,
    getShortcutsWithConflicts,
  };
}

/** Singleton shortcuts store instance */
export const shortcutsStore = createShortcutsStore();
