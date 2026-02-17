<script lang="ts">
  /**
   * Layout for the History window.
   * Uses the default app styles from the root layout.
   */
  import type { Snippet } from 'svelte';

  interface Props {
    children?: Snippet;
  }

  let { children }: Props = $props();
</script>

{#if children}
  {@render children()}
{/if}
