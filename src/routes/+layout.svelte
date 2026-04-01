<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { Sonner } from '$components/ui/sonner';
  import { ModeWatcher } from 'mode-watcher';
  import type { Snippet } from 'svelte';

  interface Props {
    children?: Snippet;
  }

  let { children }: Props = $props();

  onMount(async () => {
    try {
      await getCurrentWindow().setIcon('icons/128x128@2x.png');
    } catch {
      // Silently ignore — icon may already be set via bundle in production
    }
  });
</script>

<ModeWatcher defaultMode="dark" />

{#if children}
  {@render children()}
{/if}

<Sonner position="bottom-center" richColors />
