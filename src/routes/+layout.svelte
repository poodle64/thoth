<script lang="ts">
  import '../app.css';
  import { onMount, onDestroy } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import Toaster from '$lib/components/Toaster.svelte';
  import { toastStore } from '$lib/stores/toast.svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    children?: Snippet;
  }

  let { children }: Props = $props();
  let unlisteners: UnlistenFn[] = [];

  onMount(async () => {
    // Set window icon (needed for dev mode where no .app bundle exists)
    try {
      await getCurrentWindow().setIcon('icons/128x128@2x.png');
    } catch {
      // Silently ignore — icon may already be set via bundle in production
    }

    // Listen for toast events emitted by background stores (e.g. dictionary quick-add)
    const toastUnlisten = await listen<{ kind: 'success' | 'error' | 'info'; message: string }>(
      'toast',
      (event) => {
        const { kind, message } = event.payload;
        if (kind === 'success') toastStore.success(message);
        else if (kind === 'error') toastStore.error(message);
        else toastStore.info?.(message) ?? toastStore.success(message);
      }
    );
    unlisteners.push(toastUnlisten);
  });

  onDestroy(() => {
    for (const u of unlisteners) u();
  });
</script>

{#if children}
  {@render children()}
{/if}

<Toaster />
