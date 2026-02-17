/**
 * Dictionary store for managing vocabulary replacements
 *
 * Provides reactive state management for dictionary entries with
 * CRUD operations via Tauri IPC.
 */

import { invoke } from '@tauri-apps/api/core';

/** A dictionary entry for word replacement */
export interface DictionaryEntry {
  /** The text to search for and replace */
  from: string;
  /** The replacement text */
  to: string;
  /** Whether the match should be case-sensitive */
  caseSensitive: boolean;
}

/** Dictionary state */
interface DictionaryState {
  entries: DictionaryEntry[];
  loading: boolean;
  error: string | null;
}

/** Create the dictionary store using Svelte 5 runes */
function createDictionaryStore() {
  let state = $state<DictionaryState>({
    entries: [],
    loading: false,
    error: null,
  });

  /** Load dictionary entries from backend */
  async function load(): Promise<void> {
    state.loading = true;
    state.error = null;
    try {
      const entries = await invoke<DictionaryEntry[]>('get_dictionary_entries');
      state.entries = entries;
    } catch (e) {
      state.error = e instanceof Error ? e.message : String(e);
      console.error('Failed to load dictionary:', state.error);
    } finally {
      state.loading = false;
    }
  }

  /** Add a new dictionary entry */
  async function add(entry: DictionaryEntry): Promise<void> {
    state.error = null;
    try {
      await invoke('add_dictionary_entry', { entry });
      await load();
    } catch (e) {
      state.error = e instanceof Error ? e.message : String(e);
      throw new Error(state.error);
    }
  }

  /** Update an existing dictionary entry */
  async function update(index: number, entry: DictionaryEntry): Promise<void> {
    state.error = null;
    try {
      await invoke('update_dictionary_entry', { index, entry });
      await load();
    } catch (e) {
      state.error = e instanceof Error ? e.message : String(e);
      throw new Error(state.error);
    }
  }

  /** Remove a dictionary entry */
  async function remove(index: number): Promise<void> {
    state.error = null;
    try {
      await invoke('remove_dictionary_entry', { index });
      await load();
    } catch (e) {
      state.error = e instanceof Error ? e.message : String(e);
      throw new Error(state.error);
    }
  }

  /** Import dictionary from JSON content */
  async function importEntries(jsonContent: string, merge: boolean): Promise<number> {
    state.error = null;
    try {
      const count = await invoke<number>('import_dictionary', { jsonContent, merge });
      await load();
      return count;
    } catch (e) {
      state.error = e instanceof Error ? e.message : String(e);
      throw new Error(state.error);
    }
  }

  /** Export dictionary as JSON */
  async function exportEntries(): Promise<string> {
    state.error = null;
    try {
      return await invoke<string>('export_dictionary');
    } catch (e) {
      state.error = e instanceof Error ? e.message : String(e);
      throw new Error(state.error);
    }
  }

  /** Apply dictionary replacements to text */
  async function applyToText(text: string): Promise<string> {
    try {
      return await invoke<string>('apply_dictionary_to_text', { text });
    } catch (e) {
      console.error('Failed to apply dictionary:', e);
      return text;
    }
  }

  return {
    get entries() {
      return state.entries;
    },
    get loading() {
      return state.loading;
    },
    get error() {
      return state.error;
    },
    load,
    add,
    update,
    remove,
    importEntries,
    exportEntries,
    applyToText,
  };
}

/** Singleton dictionary store instance */
export const dictionaryStore = createDictionaryStore();
