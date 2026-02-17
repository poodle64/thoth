<script lang="ts">
  /**
   * About dialog - displays app identity, version, credits, and links.
   *
   * Rendered as a modal overlay. Uses Tauri API for runtime version.
   */
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { invoke } from '@tauri-apps/api/core';

  interface Props {
    open: boolean;
    onclose: () => void;
  }

  let { open, onclose }: Props = $props();

  let version = $state('');

  onMount(async () => {
    try {
      version = await getVersion();
    } catch {
      version = '';
    }
  });

  function openExternal(url: string) {
    invoke('open_url', { url }).catch((err) =>
      console.error('Failed to open URL:', err)
    );
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onclose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onclose();
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={handleBackdropClick} onkeydown={handleKeydown}>
    <div class="dialog" role="dialog" aria-labelledby="about-title" aria-modal="true">
      <!-- App identity -->
      <div class="identity">
        <span class="ibis-glyph">𓅝</span>
        <h1 id="about-title" class="app-name">Thoth</h1>
        <p class="tagline">Scribe to the gods. Typist to you.</p>
        {#if version}
          <p class="version">Version {version}</p>
        {/if}
      </div>

      <!-- Credits -->
      <div class="credits">
        <p class="credit-line">
          Created by
          <button class="link-inline" onclick={() => openExternal('https://github.com/poodle64')}>poodle64</button>
        </p>
        <p class="credit-line">
          Contributions from
          <button class="link-inline" onclick={() => openExternal('https://github.com/nephalemsec')}>nephalemsec</button>
        </p>
      </div>

      <!-- Links -->
      <div class="links">
        <button class="link-item" onclick={() => openExternal('https://github.com/poodle64/thoth')}>
          GitHub
        </button>
        <span class="link-separator">·</span>
        <button class="link-item" onclick={() => openExternal('https://github.com/poodle64/thoth/blob/main/LICENCE')}>
          MIT Licence
        </button>
      </div>

      <!-- Acknowledgements -->
      <div class="acknowledgements">
        <p class="ack-title">Built with</p>
        <p class="ack-list">
          Tauri · Svelte · whisper.cpp · Sherpa-ONNX · Ollama
        </p>
      </div>

      <button class="close-btn" onclick={onclose}>Close</button>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(4px);
    animation: fade-in 0.15s ease;
  }

  .dialog {
    width: 320px;
    padding: 32px 28px 24px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    text-align: center;
    display: flex;
    flex-direction: column;
    gap: 20px;
    animation: scale-in 0.15s ease;
  }

  .identity {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .ibis-glyph {
    font-size: 72px;
    line-height: 1;
    margin-bottom: 8px;
  }

  .app-name {
    margin: 0;
    font-size: 20px;
    font-weight: 700;
    color: var(--color-text-primary);
    letter-spacing: 0.5px;
  }

  .tagline {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-tertiary);
    font-style: italic;
  }

  .version {
    margin: 4px 0 0;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .credits {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .credit-line {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .link-inline {
    display: inline;
    padding: 0;
    margin: 0;
    background: none;
    border: none;
    color: var(--color-accent);
    font: inherit;
    cursor: pointer;
  }

  .link-inline:hover {
    text-decoration: underline;
  }

  .links {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  .link-item {
    padding: 0;
    background: none;
    border: none;
    font-size: var(--text-xs);
    color: var(--color-accent);
    cursor: pointer;
  }

  .link-item:hover {
    text-decoration: underline;
  }

  .link-separator {
    color: var(--color-text-tertiary);
    font-size: var(--text-xs);
  }

  .acknowledgements {
    padding-top: 12px;
    border-top: 1px solid var(--color-border);
  }

  .ack-title {
    margin: 0 0 4px;
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .ack-list {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    line-height: 1.6;
  }

  .close-btn {
    padding: 6px 20px;
    font-size: var(--text-sm);
    background: var(--color-bg-tertiary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-text-primary);
    cursor: pointer;
    transition: background var(--transition-fast);
    align-self: center;
  }

  .close-btn:hover {
    background: var(--color-bg-hover);
  }

  @keyframes fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes scale-in {
    from {
      opacity: 0;
      transform: scale(0.95);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }
</style>
