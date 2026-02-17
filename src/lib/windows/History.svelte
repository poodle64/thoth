<script lang="ts">
  /**
   * History window - standalone window wrapping the shared HistoryPane.
   *
   * Provides window chrome (title bar with drag region) around the
   * reusable HistoryPane component.
   */

  import { getCurrentWindow } from '@tauri-apps/api/window';
  import HistoryPane from '../components/HistoryPane.svelte';

  /** Handle window dragging from title bar */
  function handleDragRegionMouseDown(event: MouseEvent): void {
    if (event.buttons !== 1) return;
    const target = event.target as HTMLElement;
    const interactive = target.closest('button, input, a, [role="button"]');
    if (!interactive) {
      getCurrentWindow()
        .startDragging()
        .catch(() => {});
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="history-window">
  <!-- Title bar with drag region -->
  <header class="title-bar" onmousedown={handleDragRegionMouseDown}>
    <span class="title-text">History</span>
  </header>

  <div class="history-content">
    <HistoryPane />
  </div>
</div>

<style>
  .history-window {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100vh;
    background: var(--color-bg-primary);
    position: relative;
  }

  /* Title bar */
  .title-bar {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    height: var(--header-height);
    background-color: var(--color-bg-primary);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    /* Enable window dragging */
    -webkit-app-region: drag;
    app-region: drag;
    /* Prevent text selection */
    -webkit-user-select: none;
    user-select: none;
  }

  .title-text {
    font-size: 13px;
    font-weight: 500;
    color: var(--color-text-primary);
    user-select: none;
  }

  .history-content {
    flex: 1;
    min-height: 0;
  }
</style>
