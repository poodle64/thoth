<script lang="ts">
  import { toastStore } from '../stores/toast.svelte';
</script>

{#if toastStore.toasts.length > 0}
  <div class="toaster" role="status" aria-live="polite">
    {#each toastStore.toasts as toast (toast.id)}
      <div class="toast {toast.type}">
        <span class="toast-message">{toast.message}</span>
        {#if toast.dismissible}
          <button
            class="toast-dismiss"
            onclick={() => toastStore.dismiss(toast.id)}
            aria-label="Dismiss"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M18 6L6 18M6 6l12 12" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
          </button>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .toaster {
    position: fixed;
    bottom: 24px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 9999;
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    font-weight: 500;
    pointer-events: auto;
    animation: toast-in 0.2s ease;
    box-shadow: var(--shadow-md);
  }

  .toast.success {
    background: var(--color-success);
    color: white;
  }

  .toast.error {
    background: var(--color-error);
    color: white;
  }

  .toast.info {
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
    border: 1px solid var(--color-border);
  }

  .toast-message {
    flex: 1;
  }

  .toast-dismiss {
    flex-shrink: 0;
    width: 18px;
    height: 18px;
    padding: 0;
    background: none;
    border: none;
    color: inherit;
    opacity: 0.7;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .toast-dismiss:hover {
    opacity: 1;
    background: none;
  }

  .toast-dismiss svg {
    width: 100%;
    height: 100%;
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
</style>
