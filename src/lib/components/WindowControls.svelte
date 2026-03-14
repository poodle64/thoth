<script lang="ts">
  /**
   * Custom window controls for when system decorations are disabled.
   * Linux/tiling WM only — just a close (hide-to-tray) button.
   */
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { X } from 'lucide-svelte';

  async function close() {
    await getCurrentWindow().close();
  }
</script>

<div class="window-controls" style="-webkit-app-region: no-drag">
  <button class="wc-btn wc-close" onclick={close} title="Hide to tray">
    <X size={14} />
  </button>
</div>

<style>
  .window-controls {
    display: flex;
    align-items: center;
  }

  /* Use button.wc-btn for higher specificity than the global `button` reset */
  button.wc-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }

  button.wc-close:hover {
    background: #e53935;
    color: #fff;
  }
</style>
