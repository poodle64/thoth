<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import type { Snippet } from 'svelte';

  interface Props {
    children?: Snippet;
  }

  let { children }: Props = $props();

  onMount(async () => {
    // Set window icon (needed for dev mode where no .app bundle exists)
    try {
      await getCurrentWindow().setIcon('icons/128x128@2x.png');
    } catch {
      // Silently ignore — icon may already be set via bundle in production
    }
  });
</script>

{#if children}
  {@render children()}
{/if}
